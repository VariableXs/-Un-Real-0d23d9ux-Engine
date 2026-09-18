# -*- coding: utf-8 -*-
"""枚举盘符定位 U 盘（盘符级：类型/卷标/FS/容量）。"""
import ctypes
import ctypes.wintypes as wt

k32 = ctypes.windll.kernel32
GetLogicalDrives = k32.GetLogicalDrives
GetDriveTypeW = k32.GetDriveTypeW
GetVolumeInformationW = k32.GetVolumeInformationW
GetDiskFreeSpaceExW = k32.GetDiskFreeSpaceExW

mask = GetLogicalDrives()
names = {2: 'REMOVABLE', 3: 'FIXED', 4: 'REMOTE', 5: 'CDROM', 6: 'RAMDISK'}
for i in range(26):
    if mask >> i & 1:
        letter = chr(65 + i) + ':\\'
        t = GetDriveTypeW(letter)
        if t in (2, 3, 4, 5):
            vn = ctypes.create_unicode_buffer(260)
            fs = ctypes.create_unicode_buffer(64)
            flags = wt.DWORD(0)
            ok = GetVolumeInformationW(letter, vn, 260, None, None, ctypes.byref(flags), fs, 64)
            free = ctypes.c_ulonglong()
            total = ctypes.c_ulonglong()
            GetDiskFreeSpaceExW(letter, ctypes.byref(free), ctypes.byref(total), None)
            label = vn.value if ok else ''
            print('%s type=%s label=%r fs=%s total=%.2fGB free=%.2fGB' % (
                letter, names.get(t, t), label, fs.value, total.value / 2**30, free.value / 2**30))
