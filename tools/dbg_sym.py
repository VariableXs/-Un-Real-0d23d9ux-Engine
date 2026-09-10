# -*- coding: utf-8 -*-
"""Symbolize offsets in a PE via dbghelp SymLoadModuleEx (offline PDB).
Usage: python dbg_sym.py <exe> <off1> <off2> ..."""
import sys, io, ctypes
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from ctypes import wintypes

exe = sys.argv[1]
offs = [int(x, 0) for x in sys.argv[2:]]

dbghelp = ctypes.windll.dbghelp
IMAGE_FILE = 0
SymInitialize = dbghelp.SymInitializeW
SymInitialize.argtypes = [wintypes.HANDLE, wintypes.LPCWSTR, wintypes.BOOL]
SymInitialize(IMAGE_FILE, None, False)

SymLoadModuleEx = dbghelp.SymLoadModuleExW
SymLoadModuleEx.argtypes = [wintypes.HANDLE, wintypes.HANDLE, wintypes.LPCWSTR,
                            wintypes.LPCWSTR, ctypes.c_uint64, wintypes.DWORD,
                            ctypes.c_void_p, wintypes.DWORD]
base = SymLoadModuleEx(IMAGE_FILE, None, exe, None, 0x180000000, 0, None, 0)
if not base:
    print("SymLoadModuleEx failed", ctypes.GetLastError()); sys.exit(1)

class SYMBOL_INFOW(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wintypes.ULONG),
                ("TypeIndex", wintypes.ULONG),
                ("Reserved", ctypes.c_uint64 * 2),
                ("Index", wintypes.ULONG),
                ("Size", wintypes.ULONG),
                ("ModBase", ctypes.c_uint64),
                ("Flags", wintypes.ULONG),
                ("Value", ctypes.c_uint64),
                ("Address", ctypes.c_uint64),
                ("Register", wintypes.ULONG),
                ("Scope", wintypes.ULONG),
                ("Tag", wintypes.ULONG),
                ("NameLen", wintypes.ULONG),
                ("MaxNameLen", wintypes.ULONG),
                ("Name", ctypes.c_wchar * 512)]

for off in offs:
    sym = SYMBOL_INFOW()
    sym.SizeOfStruct = ctypes.sizeof(SYMBOL_INFOW) - 512 * ctypes.sizeof(ctypes.c_wchar)
    sym.MaxNameLen = 512
    disp = ctypes.c_int64()
    addr = 0x180000000 + off
    ok = dbghelp.SymFromAddrW(IMAGE_FILE, ctypes.c_uint64(addr), ctypes.byref(disp), ctypes.byref(sym))
    name = sym.Name if ok else "?"
    print(f"+0x{off:x}  {name} +0x{disp.value:x}" if ok else f"+0x{off:x}  ?")
