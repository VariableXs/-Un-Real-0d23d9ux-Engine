# R3 Phase G: taskbar icon tour (identify each app)
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.25
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# taskbar app icons (physical coords), y=1041
xs = [794, 860, 914, 969, 1024, 1074]
for i, x in enumerate(xs):
    pyautogui.click(x, 1041); time.sleep(1.6)
    s(f"r3_08{i}_tb{i}")
    # close whatever opened via Esc (safe) then F for next
    pyautogui.press("esc"); time.sleep(0.7)
s("r3_089_after_tour")
print("done")
