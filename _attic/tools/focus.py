"""Focus the variable.exe main window."""
import ctypes, ctypes.wintypes as wt, sys, subprocess
u = ctypes.windll.user32; u.SetProcessDPIAware()
out = subprocess.run(["tasklist","/FI","IMAGENAME eq variable.exe","/FO","CSV"],capture_output=True).stdout.decode("gbk","replace")
pids = [int(l.split('","')[1]) for l in out.splitlines() if l.startswith('"variable')]
targets = []
@ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
def cb(h, l):
    pid = wt.DWORD()
    u.GetWindowThreadProcessId(h, ctypes.byref(pid))
    r = wt.RECT(); u.GetWindowRect(h, ctypes.byref(r))
    if pid.value in pids and u.IsWindowVisible(h) and (r.right-r.left) > 100:
        buf = ctypes.create_unicode_buffer(256); u.GetWindowTextW(h, buf, 256)
        print(h, pid.value, repr(buf.value), (r.left, r.top, r.right, r.bottom))
        targets.append((h, (r.right-r.left)*(r.bottom-r.top)))
    return True
u.EnumWindows(cb, 0)
if targets:
    targets.sort(key=lambda t: -t[1])
    h = targets[0][0]
    u.ShowWindow(h, 9)
    # workaround foreground lock: attach thread input
    fg = u.GetForegroundWindow()
    tid_f = u.GetWindowThreadProcessId(fg, None); tid_me = u.GetWindowThreadProcessId(h, None)
    u.AttachThreadInput(tid_f, tid_me, True)
    u.SetForegroundWindow(h); u.SetFocus(h)
    u.AttachThreadInput(tid_f, tid_me, False)
    print("focused", h)
else:
    print("no window found")
