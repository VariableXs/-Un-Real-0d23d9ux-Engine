# R3: enumerate variable.exe top-level windows, WM_CLOSE the project-analysis one
import ctypes, ctypes.wintypes as wt
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
u32 = ctypes.windll.user32; k32 = ctypes.windll.kernel32

TARGET_PID = 11680
print("variable pid:", TARGET_PID)

rows = []
@ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
def cb(h, l):
    pid = wt.DWORD(); u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
    if pid.value != TARGET_PID: return True
    buf = ctypes.create_unicode_buffer(256)
    u32.GetWindowTextW(h, buf, 256); title = buf.value
    u32.GetClassNameW(h, buf, 256); cls = buf.value
    vis = u32.IsWindowVisible(h)
    r = wt.RECT(); u32.GetWindowRect(h, ctypes.byref(r))
    rows.append((h, cls, title, vis, (r.left, r.top, r.right, r.bottom)))
    return True
u32.EnumWindows(cb, 0)
for h, cls, title, vis, rect in rows:
    print(f"hwnd={h} cls={cls} vis={vis} rect={rect} title={title!r}")

# close windows whose title contains 项目分析
WM_CLOSE = 0x0010
closed = []
for h, cls, title, vis, rect in rows:
    if "项目分析" in title:
        u32.PostMessageW(h, WM_CLOSE, 0, 0)
        closed.append((h, title))
print("posted WM_CLOSE to:", closed)
