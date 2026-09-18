# -*- coding: utf-8 -*-
"""诊断 \\.\E: 句柄与 IOCTL，随后读 PhysicalDrive GPT。"""
import ctypes
import ctypes.wintypes as wt
import struct

k32 = ctypes.windll.kernel32
k32.CreateFileW.restype = ctypes.c_void_p
k32.DeviceIoControl.argtypes = [ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, wt.DWORD,
                                ctypes.c_void_p, wt.DWORD, ctypes.POINTER(wt.DWORD), ctypes.c_void_p]

h = k32.CreateFileW('\\\\.\\E:', 0, 3, None, 3, 0, None)
print('handle raw =', h, 'invalid?', h in (None, 0, 0xFFFFFFFFFFFFFFFF),
      'gle after CreateFile =', k32.GetLastError())

if h and h != 0xFFFFFFFFFFFFFFFF:
    out = (ctypes.c_byte * 12)()
    ret = wt.DWORD(0)
    ok = k32.DeviceIoControl(h, 0x2D1080, None, 0, out, 12, ctypes.byref(ret), None)
    print('DeviceIoControl ok =', ok, 'gle =', k32.GetLastError(), 'bytes =', ret.value)
    if ok:
        dtype, dnum, pnum = struct.unpack('<III', bytes(out))
        print('STORAGE_DEVICE_NUMBER: type=%d number=%d partition=%d' % (dtype, dnum, pnum))
    k32.CloseHandle(ctypes.c_void_p(h))
