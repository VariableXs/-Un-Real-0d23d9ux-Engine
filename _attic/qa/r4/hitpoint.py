# R4 诊断：任务栏若干点位 WindowFromPoint 命中窗口
import ctypes
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
from ctypes import wintypes
u32 = ctypes.windll.user32
class PT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]
for x, y, label in [(734,1050,"V按钮"),(1861,1012,"时钟"),(742,1010,"空间面板b"),(1560,1010,"托盘B"),(960,1050,"dock中部")]:
    pt = PT(x, y)
    hwnd = u32.WindowFromPoint(pt)
    buf = ctypes.create_unicode_buffer(64)
    ctypes.windll.user32.GetClassNameW(hwnd, buf, 64)
    tbuf = ctypes.create_unicode_buffer(128)
    ctypes.windll.user32.GetWindowTextW(hwnd, tbuf, 128)
    print(f"({x},{y}) {label}: hwnd={hwnd} class={buf.value!r} title={tbuf.value[:50]!r}")
