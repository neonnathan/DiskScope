# Small UI helpers used by the test scripts.
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class UiProbe {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);

  public static IntPtr FindDialog(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid) {
        var c = new StringBuilder(64); GetClassName(h, c, 64);
        if (c.ToString() == "#32770") { found = h; return false; }
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  public static IntPtr FindButton(IntPtr dlg, int id) {
    IntPtr found = IntPtr.Zero;
    EnumChildWindows(dlg, (h, l) => {
      if (GetDlgCtrlID(h) == id) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  /// Find a push button inside the dialog whose caption contains \p text
  /// (ignoring '&' accelerators and case).
  public static IntPtr FindButtonText(IntPtr dlg, string text) {
    IntPtr found = IntPtr.Zero;
    EnumChildWindows(dlg, (h, l) => {
      var c = new StringBuilder(64); GetClassName(h, c, 64);
      if (c.ToString() != "Button") return true;
      var t = new StringBuilder(256); GetWindowText(h, t, 256);
      string cap = t.ToString().Replace("&", "").Trim();
      if (cap.Equals(text, StringComparison.OrdinalIgnoreCase)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  /// Click a button the way a user would: post BM_CLICK so the dialog gets the
  /// same notification it would from a real click.
  public static bool ClickButton(IntPtr dlg, string text) {
    IntPtr b = FindButtonText(dlg, text);
    if (b == IntPtr.Zero) return false;
    PostMessage(b, 0x00F5, IntPtr.Zero, IntPtr.Zero);
    return true;
  }

  public static string Describe(uint pid) {
    var sb = new StringBuilder();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid) return true;
      var c = new StringBuilder(64); GetClassName(h, c, 64);
      var t = new StringBuilder(256); GetWindowText(h, t, 256);
      sb.AppendLine("TOP " + h + " " + c + " '" + t + "'");
      EnumChildWindows(h, (ch, ll) => {
        var cc = new StringBuilder(64); GetClassName(ch, cc, 64);
        var tt = new StringBuilder(256); GetWindowText(ch, tt, 256);
        sb.AppendLine("  child " + ch + " id=" + GetDlgCtrlID(ch) + " " + cc + " '" + tt + "'");
        return true;
      }, IntPtr.Zero);
      return true;
    }, IntPtr.Zero);
    return sb.ToString();
  }
}
"@
