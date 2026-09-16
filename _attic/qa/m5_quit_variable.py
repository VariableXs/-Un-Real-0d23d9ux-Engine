import ctypes, sys, time
u32 = ctypes.WinDLL("user32", use_last_error=True)
WM_CLOSE = 0x0010
pid_target = int(sys.argv[1])
targets = []
EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
def cb(hwnd, _l):
    hwnd = hwnd if isinstance(hwnd, int) else hwnd.value
    pid = ctypes.c_uint(0)
    u32.GetWindowThreadProcessId(ctypes.c_void_p(hwnd), ctypes.byref(pid))
    if pid.value == pid_target and u32.IsWindowVisible(ctypes.c_void_p(hwnd)):
        targets.append(hwnd)
    return True
u32.EnumWindows(EnumProc(cb), None)
print("targets:", targets)
for h in targets:
    u32.PostMessageW(ctypes.c_void_p(h), WM_CLOSE, None, None)
print("WM_CLOSE posted")
