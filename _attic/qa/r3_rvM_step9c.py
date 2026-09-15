# R3-B1 step9c: UIA GetFocusedElement -> ValuePattern.SetValue
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

import comtypes, comtypes.client
UIA = comtypes.client.GetModule("UIAutomationCore.dll")
automation = comtypes.client.CreateObject(
    "{ff48dba4-60ef-4201-aa87-54103eef594e}", interface=UIA.IUIAutomation)

u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.0)
# click username field (embedded view forwards click to steam)
pyautogui.click(787, 428); time.sleep(1.2)

focused = automation.GetFocusedElement()
try:
    name = focused.CurrentName
except Exception:
    name = "?"
ct = focused.CurrentControlType   # ControlType
print("focused:", name, "controlType:", ct, flush=True)
vp = focused.GetCurrentPattern(10002)  # ValuePatternId
if vp:
    pat = vp.QueryInterface(UIA.IUIValuePattern)
    pat.SetValue("VARIABLE_QA_R3")
    print("SetValue done", flush=True)
else:
    print("no ValuePattern on focused element", flush=True)
time.sleep(1.2)
s("rv_M2_uia_setvalue")
print("step9c done", flush=True)
