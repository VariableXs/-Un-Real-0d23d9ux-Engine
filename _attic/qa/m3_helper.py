# M3 acceptance phase A: capture desktop state, enum Variable embed state
import ctypes, sys
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time, json, ctypes.wintypes as wt

pyautogui.FAILSAFE = False
SHOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"

u32 = ctypes.windll.user32
EnumWindows = u32.EnumWindows
IsWindowVisible = u32.IsWindowVisible
GetWindowTextW = u32.GetWindowTextW
GetClassNameW = u32.GetClassNameW
GetWindowThreadProcessId = u32.GetWindowThreadProcessId
GetWindowRect = u32.GetWindowRect

WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)

def win_info(h):
    buf = ctypes.create_unicode_buffer(256)
    GetWindowTextW(h, buf, 256)
    title = buf.value
    GetClassNameW(h, buf, 256)
    cls = buf.value
    pid = wt.DWORD(0)
    GetWindowThreadProcessId(h, ctypes.byref(pid))
    rc = wt.RECT()
    GetWindowRect(h, ctypes.byref(rc))
    return {"hwnd": h, "title": title[:60], "cls": cls, "pid": pid.value,
            "rect": [rc.left, rc.top, rc.right, rc.bottom],
            "visible": bool(IsWindowVisible(h))}

def enum():
    out = []
    def cb(h, l):
        out.append(win_info(h))
        return True
    EnumWindows(WNDENUMPROC(cb), 0)
    return out

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "state":
        wins = [w for w in enum() if w["visible"] and (w["cls"].startswith(("Chrome", "WebView", "Notepad")) or "Variable" in w["title"] or w["cls"] == "Tauri Window")]
        print(json.dumps(wins, ensure_ascii=False, indent=1))
    elif cmd == "shot":
        p = pyautogui.screenshot()
        p.save(f"{SHOT}/{sys.argv[2]}.png")
        print("saved", sys.argv[2], p.size)
    elif cmd == "notepad":
        # launch notepad, report new window
        import subprocess
        before = {w["hwnd"] for w in enum()}
        subprocess.Popen(["notepad.exe"])
        for _ in range(20):
            time.sleep(0.5)
            for w in enum():
                if w["hwnd"] not in before and w["cls"] == "Notepad" and w["visible"]:
                    print(json.dumps(w, ensure_ascii=False))
                    sys.exit(0)
        print("NOTEPAD_NOT_FOUND")
