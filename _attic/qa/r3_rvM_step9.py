# R3-B1 step9: UIA ValuePattern.SetValue into steam edit field
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

import comtypes
import comtypes.client
UIA = comtypes.client.GetModule("UIAutomationCore.dll")
automation = comtypes.client.CreateObject(
    "{ff48dba4-60ef-4201-aa87-54103eef594e}",
    interface=UIA.IUIAutomation)
TREE = automation.TreeScope_Descendants
cond_true = automation.CreateTrueCondition()
HWND = 1573622
el = automation.ElementFromHandle(HWND)
found = None
def walk(elem, depth=0):
    global found
    if found is not None or depth > 14: return
    try:
        name = elem.CurrentName or ""
        ct = elem.CurrentControlType
    except Exception:
        return
    if ct == UIA.UIA_EditControlTypeId if hasattr(UIA, "UIA_EditControlTypeId") else False:
        found = elem; return
    try: walker = automation.ControlViewWalker
    except Exception: walker = None
    try:
        child = automation.ControlViewWalker.GetFirstChildElement(elem)
    except Exception:
        child = None
    while child:
        walk(child, depth + 1)
        if found is not None: return
        try:
            child = automation.ControlViewWalker.GetNextSiblingElement(child)
        except Exception:
            break

# simpler: FindFirst for edit control type
import ctypes as _c
edit_cond = automation.CreatePropertyCondition(
    UIA.UIA_ControlTypePropertyId, UIA.UIA_EditControlTypeId)
found = el.FindFirst(TREE, edit_cond)
print("edit found:", found, flush=True)
if found:
    vp = found.GetCurrentPattern(UIA.UIA_ValuePatternId)
    pat = vp.QueryInterface(UIA.IUIValuePattern)
    pat.SetValue("VARIABLE_QA_R3")
    print("SetValue done", flush=True)
u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.2)
s("rv_M1_uia_setvalue")
print("step9 done", flush=True)
