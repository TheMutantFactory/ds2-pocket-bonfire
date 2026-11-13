param([int]$ProcessId, [string]$Name = 'DarkSoulsII')
# Debugging aid for a stuck game: returns every thread's suspend count and call stack in another process, using dbghelp.
if (-not $ProcessId) { $ProcessId = (Get-Process $Name -ErrorAction Stop | Select-Object -First 1).Id }
Add-Type -Language CSharp @"
using System;
using System.Text;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class Stacks {
  [DllImport("kernel32")] static extern IntPtr OpenProcess(uint a, bool i, int pid);
  [DllImport("kernel32")] static extern IntPtr OpenThread(uint a, bool i, int tid);
  [DllImport("kernel32")] static extern int SuspendThread(IntPtr t);
  [DllImport("kernel32")] static extern int ResumeThread(IntPtr t);
  [DllImport("kernel32")] static extern bool GetThreadContext(IntPtr t, IntPtr ctx);
  [DllImport("kernel32")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32")] static extern IntPtr LoadLibrary(string n);
  [DllImport("kernel32")] static extern IntPtr GetProcAddress(IntPtr m, string n);
  [DllImport("psapi", CharSet=CharSet.Unicode)] static extern uint GetModuleBaseNameW(IntPtr p, IntPtr m, StringBuilder b, uint n);
  [DllImport("dbghelp")] static extern uint SymSetOptions(uint o);
  [DllImport("dbghelp")] static extern bool SymInitialize(IntPtr p, string path, bool invade);
  [DllImport("dbghelp")] static extern ulong SymGetModuleBase64(IntPtr p, ulong a);
  [DllImport("dbghelp")] static extern bool SymFromAddr(IntPtr p, ulong a, out ulong disp, IntPtr sym);
  [DllImport("dbghelp")] static extern bool StackWalk64(uint machine, IntPtr p, IntPtr t, IntPtr frame, IntPtr ctx, IntPtr read, IntPtr fta, IntPtr gmb, IntPtr tr);

  static string Where(IntPtr p, ulong a) {
    ulong b = SymGetModuleBase64(p, a);
    string mod = "?";
    if (b != 0) { var sb = new StringBuilder(260); GetModuleBaseNameW(p, (IntPtr)(long)b, sb, 260); mod = sb.ToString(); }
    IntPtr sym = Marshal.AllocHGlobal(88 + 512);
    for (int i = 0; i < 88 + 512; i++) Marshal.WriteByte(sym, i, 0);
    Marshal.WriteInt32(sym, 0, 88); Marshal.WriteInt32(sym, 80, 500);
    ulong disp; string name = null;
    if (SymFromAddr(p, a, out disp, sym)) name = Marshal.PtrToStringAnsi(sym + 84) + "+0x" + disp.ToString("x");
    Marshal.FreeHGlobal(sym);
    return mod + (name != null ? "!" + name : "") + " (+0x" + (a - b).ToString("x") + ")";
  }

  public static string Dump(int pid) {
    var o = new StringBuilder();
    IntPtr p = OpenProcess(0x1F0FFF, false, pid);
    if (p == IntPtr.Zero) { return "can't open process " + pid; }
    SymSetOptions(0x2 | 0x4 | 0x10);
    SymInitialize(p, null, true);
    IntPtr dbg = LoadLibrary("dbghelp.dll");
    IntPtr fta = GetProcAddress(dbg, "SymFunctionTableAccess64"), gmb = GetProcAddress(dbg, "SymGetModuleBase64");
    foreach (ProcessThread th in Process.GetProcessById(pid).Threads) {
      IntPtr t = OpenThread(0x1FFFFF, false, th.Id);
      if (t == IntPtr.Zero) continue;
      int before = SuspendThread(t);
      IntPtr raw = Marshal.AllocHGlobal(0x4D0 + 16);
      IntPtr ctx = (IntPtr)(((long)raw + 15) & ~15L);
      for (int i = 0; i < 0x4D0; i++) Marshal.WriteByte(ctx, i, 0);
      Marshal.WriteInt32(ctx, 0x30, 0x10000B);
      o.AppendLine("thread " + th.Id + ": suspended " + before + " times by others, state " + th.ThreadState + (th.ThreadState == System.Diagnostics.ThreadState.Wait ? " (" + th.WaitReason + ")" : ""));
      if (GetThreadContext(t, ctx)) {
        IntPtr f = Marshal.AllocHGlobal(0x400);
        for (int i = 0; i < 0x400; i++) Marshal.WriteByte(f, i, 0);
        Marshal.WriteInt64(f, 0x00, Marshal.ReadInt64(ctx, 0xF8)); Marshal.WriteInt32(f, 0x0C, 3);
        Marshal.WriteInt64(f, 0x20, Marshal.ReadInt64(ctx, 0xA0)); Marshal.WriteInt32(f, 0x2C, 3);
        Marshal.WriteInt64(f, 0x30, Marshal.ReadInt64(ctx, 0x98)); Marshal.WriteInt32(f, 0x3C, 3);
        for (int n = 0; n < 40 && StackWalk64(0x8664, p, t, f, ctx, IntPtr.Zero, fta, gmb, IntPtr.Zero); n++) {
          ulong pc = (ulong)Marshal.ReadInt64(f, 0);
          if (pc == 0) break;
          o.AppendLine("    " + Where(p, pc));
        }
        Marshal.FreeHGlobal(f);
      }
      Marshal.FreeHGlobal(raw);
      ResumeThread(t);
      CloseHandle(t);
    }
    return o.ToString();
  }
}
"@
[Stacks]::Dump($ProcessId)
'modules:'
foreach ($m in (Get-Process -Id $ProcessId).Modules) { '    0x{0:x} 0x{1:x} {2}' -f [int64]$m.BaseAddress, ([int64]$m.BaseAddress + $m.ModuleMemorySize), $m.ModuleName }
