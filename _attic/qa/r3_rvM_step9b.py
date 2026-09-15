# R3-B1 step9b: UIA ValuePattern.SetValue into steam edit field (raw constants)
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
HWND = 1573622
el = automation.ElementFromHandle(HWND)
# TreeScope_Descendants=4, UIA_ControlTypePropertyId=30005, Edit=50004, ValuePatternId=10002
from comtypes.automation import VARIANT
edit_cond = automation.CreatePropertyCondition(30005, VARIANT(50004))
found = el.FindFirst(4, edit_cond)
print("edit found:", found, flush=True)
if found:
    vp = found.GetCurrentPattern(10002)
    pat = vp.QueryInterface(UIA.IUIValuePattern)
    pat.SetValue("VARIABLE_QA_R3")
    print("SetValue done", flush=True)
else:
    # fallback: enumerate all descendants and print edit-ish controls
    allc = el.FindFirst(4, automation.CreateTrueCondition())
    print("no edit control via FindFirst", flush=True)
u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.2)
s("rv_M1_uia_setvalue")
print("step9b done", flush=True)
