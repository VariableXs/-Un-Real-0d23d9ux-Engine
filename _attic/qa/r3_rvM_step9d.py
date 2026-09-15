# R3-B1 step9d: retry UIA after Chromium a11y tree builds; walk down from focused
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
pyautogui.click(787, 428); time.sleep(1.0)
# poke the a11y tree: query document-level element to trigger Chromium accessibility
el = automation.ElementFromHandle(1573622)
_ = el.CurrentName
time.sleep(5.0)

walker = automation.ControlViewWalker
node = walker.GetFirstChildElement(el)
target = None; count = 0
def scan(elem, depth):
    global target, count
    if target is not None or depth > 18 or elem is None: return
    count += 1
    try: ct = elem.CurrentControlType
    except Exception: return
    if ct == 50004:  # Edit
        target = elem; return
    child = walker.GetFirstChildElement(elem)
    while child is not None:
        scan(child, depth + 1)
        if target is not None: return
        child = walker.GetNextSiblingElement(child)

scan(el, 0)
print("scanned", count, "nodes; edit:", target, flush=True)
if target is not None:
    vp = target.GetCurrentPattern(10002)
    if vp:
        vp.QueryInterface(UIA.IUIValuePattern).SetValue("VARIABLE_QA_R3")
        print("SetValue done", flush=True)
u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.3)
s("rv_M3_uia_walk_setvalue")
print("step9d done", flush=True)
