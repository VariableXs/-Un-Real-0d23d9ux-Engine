# -*- coding: utf-8 -*-
"""MiniDumpWriteDump a pid, then roughly localize thread stacks by module."""
import sys, io, ctypes, struct, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from ctypes import wintypes

pid = int(sys.argv[1])
out = sys.argv[2]

dbg = ctypes.windll.dbghelp
k32 = ctypes.windll.kernel32
k32.OpenProcess.restype = wintypes.HANDLE
k32.CreateFileW.restype = wintypes.HANDLE
k32.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
dbg.MiniDumpWriteDump.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
h = k32.OpenProcess(0x1FFFFF, False, pid)
if not h:
    print("OpenProcess failed", ctypes.GetLastError()); sys.exit(1)
f = k32.CreateFileW(out, 0x40000000, 0, None, 2, 0, None)
class MINIDUMP_EXCEPTION_INFORMATION(ctypes.Structure):
    _fields_ = [("ThreadId", ctypes.c_ulong), ("ExceptionPointers", ctypes.c_void_p), ("ClientPointers", ctypes.c_int)]
ok = dbg.MiniDumpWriteDump(h, pid, f, 2, None, None, None)  # MiniDumpWithFullMemory? 2=Normal
ctypes.windll.kernel32.CloseHandle(f)
if not ok:
    print("MiniDumpWriteDump failed", ctypes.GetLastError()); sys.exit(1)
print("dumped", os.path.getsize(out), "bytes")
