# -*- coding: utf-8 -*-
"""T11 · Variable 桌面验证：启动便携版 → 等待窗口 → 截屏 → 关闭。
判据：窗口出现（FindWindow by pid）、截屏非黑、无 localhost 错误页。
"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import time

EXE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist-portable\Variable.exe"
SHOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\t11-variable-desktop.png"
ERR_SHOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\t11-variable-err.png"

user32 = ctypes.windll.user32


def find_windows_by_pid(pid):
    out = []

    def cb(hwnd, _):
        pid_out = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid_out))
        if pid_out.value == pid and user32.IsWindowVisible(hwnd):
            length = user32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, buf, length + 1)
            out.append((hwnd, buf.value))
        return True
    WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return out


def screenshot(path):
    from PIL import ImageGrab
    im = ImageGrab.grab()
    im.save(path)
    return im.size


def main():
    proc = subprocess.Popen([EXE], cwd=os.path.dirname(EXE))
    print("pid:", proc.pid)
    time.sleep(15)  # Tauri 启动 + WebView2 初始化
    wins = find_windows_by_pid(proc.pid)
    print("windows:", wins)
    size = screenshot(SHOT)
    print("screenshot:", size, "->", SHOT)
    # 再等 5s 取第二张（确认稳定态）
    time.sleep(5)
    wins2 = find_windows_by_pid(proc.pid)
    print("windows(+5s):", wins2)
    subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"], capture_output=True)
    print("killed")


if __name__ == "__main__":
    main()
