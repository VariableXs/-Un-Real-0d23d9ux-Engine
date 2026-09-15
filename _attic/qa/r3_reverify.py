# R3 re-verify after rebuild: R3-B2 calendar close, R3-B1 steam typing, snapshots for R3-B4/R3-B7
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32

def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)
def fg():
    u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None))

fg(); time.sleep(1.0)

# dismiss Wallpaper-Engine conflict toast ("知道了")
pyautogui.click(393, 89); time.sleep(1.0)
s("rv_01_toast_dismissed")

# ---------- Part A: R3-B2 calendar popup outside-click + Esc ----------
# open calendar via taskbar clock
pyautogui.click(1872, 1036); time.sleep(1.2)
s("rv_A1_cal_open")
# outside click on desktop area
pyautogui.click(700, 500); time.sleep(0.9)
s("rv_A2_cal_outclick")
# reopen then Esc
pyautogui.click(1872, 1036); time.sleep(1.2)
s("rv_A3_cal_reopen")
pyautogui.press("esc"); time.sleep(0.9)
s("rv_A4_cal_esc")

# ---------- snapshot taskbar right half for audio-icon calibration ----------
s("rv_A5_taskbar_right")

# ---------- Part C: R3-B1 steam typing into embedded login ----------
# open start menu and search steam
pyautogui.click(759, 1041); time.sleep(1.4)
pyautogui.click(955, 94); time.sleep(0.7)
pyautogui.typewrite("steam", interval=0.08); time.sleep(1.2)
s("rv_C1_steam_search")
# click result row (same as phaseC first-run position)
pyautogui.click(500, 396); time.sleep(8)
s("rv_C2_steam_launching")
time.sleep(10)
s("rv_C3_steam_embed")
# click username field and type
pyautogui.click(787, 428); time.sleep(1.0)
pyautogui.typewrite("VARIABLE_QA_R3", interval=0.05); time.sleep(1.2)
s("rv_C4_steam_typed")

# close embedded steam via red button, back to desktop
pyautogui.click(1667, 48); time.sleep(1.5)
s("rv_C5_steam_detached")

# ---------- snapshot start menu for settings-entry calibration ----------
pyautogui.click(759, 1041); time.sleep(1.4)
s("rv_D1_startmenu")
pyautogui.press("esc"); time.sleep(0.6)
s("rv_done")
print("reverify script done", flush=True)
