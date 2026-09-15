# R3-B1 reverify step2: dismiss toast precisely, then search steam
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(0.8)
# dismiss toast: "知道了" button at display (446,116) -> physical (793,206)
pyautogui.click(793, 206); time.sleep(1.2)
s("rv_F1_toast_gone")
pyautogui.press("esc"); time.sleep(0.8)          # close start menu (reset state)
pyautogui.click(759, 1041); time.sleep(1.8)      # reopen start menu
pyautogui.click(959, 97); time.sleep(1.0)        # focus search box (now unobstructed)
pyautogui.typewrite("steam", interval=0.09); time.sleep(1.6)
s("rv_F2_typed")
print("step2 done - inspect rv_F2", flush=True)
