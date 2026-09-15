# R3-B1 reverify step3: commit IME, click Steam row, wait embed, type into login, screenshot
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

# commit IME composition: first candidate is "steam" itself
pyautogui.press("enter"); time.sleep(1.0)
s("rv_G1_ime_committed")
# click Steam row (最近使用) - display (281,171) -> physical (499,304)
pyautogui.click(499, 304); time.sleep(8)
s("rv_G2_launching")
time.sleep(10)
s("rv_G3_embed")
# click username field (same as phaseD verified position)
pyautogui.click(787, 428); time.sleep(1.0)
s("rv_G4_user_focus")
pyautogui.typewrite("VARIABLE_QA_R3", interval=0.05); time.sleep(1.4)
s("rv_G5_typed")
print("step3 done - inspect rv_G3/rv_G5", flush=True)
