# R3-B11 decisive experiment: wake CEF main window render, then reparent into Variable desktop
import ctypes, time, subprocess
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
u32 = ctypes.windll.user32
STEAM_H = 919278

# 1) find Variable desktop window (Tauri Window of variable.exe)
r = subprocess.run(["tasklist", "/FI", "IMAGENAME eq variable.exe", "/FO", "CSV"], capture_output=True)
line = [l for l in r.stdout.decode("gbk", "replace").splitlines() if l.lower().startswith('"variable.exe"')]
vpid = int(line[0].split('","')[1])
found = []
WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
def _cb(hwnd, _lp):
    if not u32.IsWindowVisible(hwnd): return True
    wpid = ctypes.c_uint(0)
    u32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
    if wpid.value != vpid: return True
    buf = ctypes.create_unicode_buffer(64)
    u32.GetClassNameW(hwnd, buf, 64)
    if buf.value == "Tauri Window": found.append(hwnd)
    return True
u32.EnumWindows(WNDENUMPROC(_cb), None)
vd = found[0]
print("variable desktop hwnd", vd, flush=True)

s = ctypes.c_void_p(STEAM_H)
# 2) wake render: topmost + visible + size jiggle + click
u32.SetWindowPos(s, ctypes.c_void_p(-1), 100, 100, 875, 850, 0x0040)
time.sleep(0.5)
u32.SetWindowPos(s, ctypes.c_void_p(-1), 100, 100, 876, 851, 0x0040); time.sleep(0.3)
u32.SetWindowPos(s, ctypes.c_void_p(-1), 100, 100, 875, 850, 0x0040); time.sleep(0.8)
pyautogui.click(300, 300); time.sleep(2.0)
pyautogui.screenshot().save("rt3_rep_0_wake.png"); print("shot rt3_rep_0_wake", flush=True)

# 3) reparent into Variable desktop (keep popup style first: raw SetParent)
GWL_STYLE = -16
WS_CHILD = 0x40000000
WS_POPUP = 0x80000000
style = u32.GetWindowLongW(s, GWL_STYLE)
new_style = ctypes.c_long((style & ~WS_POPUP) | WS_CHILD)
u32.SetWindowLongW(s, GWL_STYLE, new_style)
ok = u32.SetParent(s, ctypes.c_void_p(vd))
print("SetParent ->", ok, flush=True)
# child coordinates relative to desktop (physical 1920x1080)
u32.SetWindowPos(s, None, 480, 120, 875, 850, 0x0040)
time.sleep(2.5)
pyautogui.screenshot().save("rt3_rep_1_reparent.png"); print("shot rt3_rep_1_reparent", flush=True)
