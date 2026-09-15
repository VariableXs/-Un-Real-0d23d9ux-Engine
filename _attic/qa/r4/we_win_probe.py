# R5 诊断：枚举 Wallpaper Engine 进程的顶层窗口（类名/ExStyle/可见/矩形/置顶）
import ctypes
from ctypes import wintypes

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

WE = ("wallpaper64.exe", "wallpaper32.exe", "wallpaperservice64.exe", "wallpaperengine.exe")

def img_of(pid):
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    h = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        return None
    buf = ctypes.create_unicode_buffer(1024)
    size = wintypes.DWORD(1024)
    ok = kernel32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size))
    kernel32.CloseHandle(h)
    return buf.value.rsplit("\\", 1)[-1].lower() if ok else None

rows = []
@ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
def cb(hwnd, _):
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    img = img_of(pid.value)
    if not img or img not in WE:
        return True
    cls = ctypes.create_unicode_buffer(64)
    user32.GetClassNameW(hwnd, cls, 64)
    ex = user32.GetWindowLongPtrW(hwnd, -20)  # GWL_EXSTYLE
    r = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    title = ctypes.create_unicode_buffer(128)
    user32.GetWindowTextW(hwnd, title, 128)
    rows.append((hwnd, img, cls.value, bool(ex & 0x8), bool(user32.IsWindowVisible(hwnd)),
                 (r.left, r.top, r.right, r.bottom), title.value))
    return True

user32.EnumWindows(cb, 0)
for r in rows:
    print(r)
print("total:", len(rows))
