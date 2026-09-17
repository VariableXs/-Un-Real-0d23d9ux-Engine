# 探测 U 盘卷 -> PhysicalDrive 号（IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS）
import ctypes
import struct
import sys

k32 = ctypes.windll.kernel32
GENERIC_READ = 0x80000000
SHARE_RW = 3
OPEN_EXISTING = 3
IOCTL_VOL_DISK_EXTENTS = 0x00560000

letter = sys.argv[1] if len(sys.argv) > 1 else "E"
path = "\\\\.\\%s:" % letter
h = k32.CreateFileW(path, GENERIC_READ, SHARE_RW, None, OPEN_EXISTING, 0, None)
if h == -1 or h == 0xFFFFFFFFFFFFFFFF:
    print("open %s failed err=%d" % (path, k32.GetLastError()))
    sys.exit(1)
buf = ctypes.create_string_buffer(1024)
got = ctypes.c_uint(0)
ok = k32.DeviceIoControl(h, IOCTL_VOL_DISK_EXTENTS, None, 0, buf, 1024, ctypes.byref(got), None)
k32.CloseHandle(h)
if not ok:
    print("ioctl failed err=%d" % k32.GetLastError())
    sys.exit(1)
n = struct.unpack_from("<I", buf.raw, 0)[0]
for i in range(n):
    dn, start, length = struct.unpack_from("<QQQ", buf.raw, 8 + i * 24)
    print("%s: -> PhysicalDrive%d (ext start=%d len=%d)" % (letter, dn, start, length))
