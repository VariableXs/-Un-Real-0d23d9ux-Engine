# -*- coding: utf-8 -*-
"""验收 Task#8 第一步：E: 盘根目录内容 + PhysicalDrive 映射 + GPT/MBR 分区表只读解析。"""
import ctypes
import ctypes.wintypes as wt
import os
import struct
import sys

# ---------- 1. E: 根目录内容 ----------
E = 'E:\\'
print('=' * 70)
print('[1] E: root listing')
print('=' * 70)
try:
    for ent in sorted(os.scandir(E), key=lambda e: e.name.lower()):
        kind = 'DIR ' if ent.is_dir() else 'FILE'
        size = ent.stat().st_size if ent.is_file() else 0
        print('  %-6s %10d  %s' % (kind, size, ent.name))
    if not any(os.scandir(E)):
        print('  <empty>')
except Exception as e:
    print('  ERROR: %r' % e)

# ---------- 2. 盘符 -> PhysicalDrive 编号 ----------
k32 = ctypes.windll.kernel32
k32.CreateFileW.restype = ctypes.c_void_p
k32.DeviceIoControl.argtypes = [ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, wt.DWORD,
                                ctypes.c_void_p, wt.DWORD, ctypes.POINTER(wt.DWORD), ctypes.c_void_p]
IOCTL_STORAGE_GET_DEVICE_NUMBER = 0x2D1080

def device_number(letter):
    h = k32.CreateFileW('\\\\.\\' + letter, 0,
                        3, None, 3, 0, None)  # OPEN_EXISTING
    if not h or h == ctypes.c_void_p(-1).value:
        return None
    out = (ctypes.c_byte * 12)()
    ret = wt.DWORD(0)
    ok = k32.DeviceIoControl(h, IOCTL_STORAGE_GET_DEVICE_NUMBER,
                             None, 0, out, 12,
                             ctypes.byref(ret), None)
    k32.CloseHandle(ctypes.c_void_p(h))
    if not ok:
        return None
    # STORAGE_DEVICE_NUMBER: DeviceType(4B) DeviceNumber(4B) PartitionNumber(4B)
    import struct as _s
    _t, dnum, _p = _s.unpack('<III', bytes(out))
    return dnum

n = device_number('E:')
print()
print('=' * 70)
print('[2] E: -> PhysicalDrive%s' % n)
print('=' * 70)

# ---------- 3. 读 PhysicalDrive 分区表（只读） ----------
GENERIC_READ = 0x80000000
FILE_SHARE_READ = 1
FILE_SHARE_WRITE = 2
OPEN_EXISTING = 3
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

def read_sectors(path, lba, count, sector_size=512):
    h = k32.CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None, OPEN_EXISTING, 0, None)
    if h == INVALID_HANDLE_VALUE or h == 0xFFFFFFFFFFFFFFFF:
        err = k32.GetLastError()
        return None, err
    try:
        k32.SetFilePointer(h, ctypes.c_long(lba * sector_size & 0xFFFFFFFF),
                           ctypes.byref(wt.DWORD((lba * sector_size) >> 32)), 0)
        buf = ctypes.create_string_buffer(count * sector_size)
        got = wt.DWORD(0)
        ok = k32.ReadFile(h, buf, count * sector_size, ctypes.byref(got), None)
        if not ok:
            return None, k32.GetLastError()
        return buf.raw[:got.value], 0
    finally:
        k32.CloseHandle(h)

pd = '\\\\.\\PhysicalDrive%d' % n
# 物理扇区大小探测
IOCTL_DISK_GET_DRIVE_GEOMETRY = 0x70000
geom = (ctypes.c_byte * 24)()
h = k32.CreateFileW(pd, 0, 3, None, 3, 0, None)
sector_size = 512
if h != INVALID_HANDLE_VALUE and h != 0xFFFFFFFFFFFFFFFF:
    if k32.DeviceIoControl(h, IOCTL_DISK_GET_DRIVE_GEOMETRY, None, 0,
                           geom, 24, ctypes.byref(wt.DWORD()), None):
        # DISK_GEOMETRY: LARGE_INTEGER Cylinders(8) MEDIA_TYPE(4) DWORD TracksPerCylinder(4)
        #                DWORD SectorsPerTrack(4) DWORD BytesPerSector(4)
        sector_size = struct.unpack_from('<I', geom, 20)[0]
    k32.CloseHandle(h)
print('  sector size: %d' % sector_size)

data, err = read_sectors(pd, 0, 1, 512)
if data is None:
    print('  MBR read FAILED err=%d (可能需要管理员权限)' % err)
    sys.exit(0)

sig = data[510:512]
print('  MBR signature: %s' % (sig.hex(),))
mbr_parts = []
if sig == b'\x55\xaa':
    # 探测保护性 MBR：type 0xEE
    types = [data[446 + 16 * i + 4] for i in range(4)]
    print('  MBR partition types: %s' % ([hex(t) for t in types],))
    for i in range(4):
        t = types[i]
        lba = struct.unpack_from('<I', data, 446 + 16 * i + 8)[0]
        cnt = struct.unpack_from('<I', data, 446 + 16 * i + 12)[0]
        boot = data[446 + 16 * i]
        print('    slot%d boot=0x%02x type=0x%02x lba=%d count=%d (%.2fGB)' % (
            i, boot, t, lba, cnt, cnt * 512 / 2**30))
        if t == 0xEE:
            mbr_parts.append('GPT')
if 'GPT' in mbr_parts:
    print('  -> GPT disk, reading GPT header (LBA1) + entries')
    gpt, err = read_sectors(pd, 1, 1, 512)
    if gpt is None:
        print('  GPT header read FAILED err=%d' % err)
    else:
        plba = struct.unpack_from('<Q', gpt, 72)[0]
        pnum = struct.unpack_from('<I', gpt, 80)[0]
        psz = struct.unpack_from('<I', gpt, 84)[0]
        disk_guid = gpt[56:72]
        print('  PartitionEntryLBA=%d Num=%d Size=%d' % (plba, pnum, psz))
        total_sz = pnum * psz
        lbas = (total_sz + 511) // 512
        tbl, err = read_sectors(pd, plba, lbas, 512)
        if tbl is None:
            print('  GPT entries read FAILED err=%d' % err)
        else:
            for i in range(pnum):
                ent = tbl[i * psz:(i + 1) * psz]
                tguid = ent[0:16]
                if tguid == b'\x00' * 16:
                    continue
                first = struct.unpack_from('<Q', ent, 32)[0]
                last = struct.unpack_from('<Q', ent, 40)[0]
                name = ent[56:128].decode('utf-16-le').rstrip('\x00')
                # 常见类型 GUID 速查
                guid_names = {
                    'c12a7328f81f11d2ba4b00a0c93ec93b': 'EFI System',
                    'e3c9e3160b5c4db88171f2e6e9e94f6e': 'Microsoft Reserved',
                    'ebd0a0a2b9e5443387c068b6b72699c7': 'Microsoft Basic Data',
                    '0fc63daf84834772b0b1f2b6f0e0f5f0': 'Linux filesystem',
                    '0657fd6da4ab43c484e50933c44b2349': 'Linux swap',
                    '2168614864496e6f744e656564804566': 'BIOS boot',
                }
                gs = '%032x' % int.from_bytes(tguid, 'little')
                tn = guid_names.get(gs, gs)
                print('    part%02d first=%d last=%d size=%.2fGB type=%s name=%r' % (
                    i + 1, first, last, (last - first + 1) * sector_size / 2**30, tn, name))
