# R3-B1 reverify step4: switch IME to EN via Shift, then type into steam login
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.0)
# clear any IME composition
pyautogui.press("esc"); time.sleep(0.5)
# switch IME to English mode (Microsoft Pinyin: Shift toggles)
pyautogui.press("shift"); time.sleep(0.8)
# click username field again to ensure focus
pyautogui.click(787, 428); time.sleep(1.0)
pyautogui.typewrite("VARIABLE_QA_R3", interval=0.06); time.sleep(1.5)
s("rv_H1_typed_en")
print("step4 done - inspect rv_H1", flush=True)
