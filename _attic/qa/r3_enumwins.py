# R3: enumerate all top-level windows with pid/process/class/title/rect
import ctypes, ctypes.wintypes as wt
try: ctypes.windll.shcore.SetProcessDPIAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
u32 = ctypes.windll.user32; k32 = ctypes.windll.kernel32

rows = []
@ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
def cb(h, l):
    if not u32.IsWindowVisible(h): return True
    buf = ctypes.create_unicode_buffer(256)
    u32.GetClassNameW(h, buf, 256); cls = buf.value
    u32.GetWindowTextW(h, buf, 256); title = buf.value
    pid = wt.DWORD(); u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
    r = wt.RECT(); u32.GetWindowRect(h, ctypes.byref(r))
    rows.append((h, pid.value, cls, title[:40], r.left, r.top, r.right, r.bottom))
    return True
u32.EnumWindows(cb, 0)

# map pid->exe
import collections
pids = collections.defaultdict(set)
for _, pid, *_ in rows: pids[pid]
def exe_of(pid):
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    h = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if h:
        buf = ctypes.create_unicode_buffer(512); sz = wt.DWORD(512)
        k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(sz))
        k32.CloseHandle(h)
        return buf.value.split("\\")[-1]
    return "?"
seen = {}
for h, pid, cls, title, l, t, rr, b in rows:
    if pid not in seen: seen[pid] = exe_of(pid)
    print(f"hwnd={h} pid={pid} exe={seen[pid]:<28} cls={cls:<26} rect=({l},{t},{rr},{b}) title={title}")
