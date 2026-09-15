"""List all top-level windows of a given process name. ASCII-only."""
import sys, ctypes
from ctypes import wintypes

user32 = ctypes.windll.user32
target = sys.argv[1].lower() if len(sys.argv) > 1 else "variable.exe"

import subprocess
out = subprocess.run(["tasklist", "/FO", "CSV"], capture_output=True).stdout.decode("gbk", "replace")
pids = []
for line in out.splitlines():
    if target in line.lower():
        pids.append(int(line.split('","')[1]))

res = []
@ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
def cb(h, l):
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(h, ctypes.byref(pid))
    if pid.value in pids:
        n = user32.GetWindowTextLengthW(h)
        buf = ctypes.create_unicode_buffer(n + 1) if n else None
        if n: user32.GetWindowTextW(h, buf, n + 1)
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(h, cls, 256)
        r = wintypes.RECT()
        user32.GetWindowRect(h, ctypes.byref(r))
        vis = user32.IsWindowVisible(h)
        res.append((h, vis, cls.value, buf.value if buf else "", r.left, r.top, r.right-r.left, r.bottom-r.top))
    return True
user32.EnumWindows(cb, 0)
for h, v, c, t, x, y, w, hh in res:
    print(f"hwnd={h} vis={v} class={c!r} title={t!r} rect={x},{y} {w}x{hh}")
print("pids:", pids)
