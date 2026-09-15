# R3 Phase K: tour all settings tabs (skip 编辑器/思维导图 per user exclusion)
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.25
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# nav items x=514 physical; y from 外观=204 pitch=50
tabs = {
    "通用": 354, "环境": 404, "浏览器": 453, "生态": 553, "网络": 603,
    "安全工作台": 652, "执行格": 702, "快捷键": 752, "输入手感": 802,
    "氛围": 851, "窗口手感": 901, "性能与维护": 951, "开放接口": 1001,
    "声音与通知": 1050,
}
for name, y in tabs.items():
    pyautogui.click(514, y); time.sleep(1.1)
    s("r3_13x_tab_" + name)
print("done")
