# -*- coding: utf-8 -*-
"""harden_vcruntime —— VC++ 2015-2022 运行库六件套收编（WP-103 · MD2 3.6）。

从 _attic/vx-vcruntime.py 收编升格。v1 只做"复制 + SHA256 复核"；本件按
MD2 升格为三段校验，并把清单锁死为模块常量——六件一件不多一件不少：

  段一  哈希（离线可用）：源与落盘双验 SHA-256；已在位且一致则幂等跳过。
  段二  版本（离线可用）：GetFileVersionInfo 提取 FileVersion 字符串，
        确认落盘件真是带版本的 DLL 而不是残缺文件。
  段三  注册面（仅在线）：ctypes.WinDLL 实际加载成功才算"注册面通过"；
        离线（非 Windows 宿主）如实返回 None，绝不把"没测"记成"通过"。

本件是四板斧之外唯一允许文件操作的部件（B-302 豁免件）：复制有清单、
有哈希、有幂等，审计扫描只针对 harden_quad.py。
"""

import ctypes
import os
import shutil
from ctypes import wintypes

import vxlib

# 六件套名单（x64）——Variable.exe 是 x64，缺 VCRUNTIME140.dll 即起不来。
FILES = [
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140.dll",
    "msvcp140_1.dll",
    "msvcp140_2.dll",
    "concrt140.dll",
]


# --------------------------------------------------------------------------
# 段一：复制 + 双哈希（幂等）
# --------------------------------------------------------------------------

def ensure_vcruntime(src_dir, dst_dir, lw):
    """把六件套从 src（内置盘 System32）补到 dst（U 盘 System32）。
    返回 (rows, ok)；rows 元组 (file, status, detail)，
    status ∈ {skip-existing, copied, missing-src, fail-hash}。"""
    lw.enter("VC 六件套 段一 哈希")
    rows = []
    ok = True
    for fname in FILES:
        s = os.path.join(src_dir, fname)
        d = os.path.join(dst_dir, fname)
        try:
            if not os.path.isfile(s):
                lw.log("  missing-src {}: 内置盘缺失".format(fname))
                rows.append((fname, "missing-src", s))
                ok = False
                continue
            src_hash = vxlib.sha256_file(s)
            if os.path.isfile(d):
                dst_hash = vxlib.sha256_file(d)
                if dst_hash == src_hash:
                    lw.log("  skip-existing {}（哈希一致，幂等跳过）".format(fname))
                    rows.append((fname, "skip-existing", dst_hash))
                    continue
            shutil.copyfile(s, d)
            dst_hash = vxlib.sha256_file(d)
            if dst_hash != src_hash:
                lw.log("  fail-hash {}: 复制后哈希不一致".format(fname))
                rows.append((fname, "fail-hash", d))
                ok = False
            else:
                lw.log("  copied {} {}B sha256={}".format(
                    fname, os.path.getsize(d), dst_hash[:16] + "..."))
                rows.append((fname, "copied", dst_hash))
        except Exception as ex:
            lw.log("  EXC {}: {}".format(fname, ex))
            rows.append((fname, "fail-hash", repr(ex)))
            ok = False
    return (rows, ok)


def collect_hashes(dst_dir):
    """落盘六件的 {file: sha256}（缺件无键）——deploy 报告与 recheck 对拍用。"""
    out = {}
    for fname in FILES:
        p = os.path.join(dst_dir, fname)
        if os.path.isfile(p):
            out[fname] = vxlib.sha256_file(p)
    return out


# --------------------------------------------------------------------------
# 段二：版本提取（GetFileVersionInfo，离线可用）
# --------------------------------------------------------------------------

_VERSION_SUBBLOCKS = (
    "\\StringFileInfo\\040904B0\\FileVersion",
    "\\StringFileInfo\\040904E4\\FileVersion",
)


def version_of(path):
    """提取 FileVersion 字符串；取不到或离线（非 Windows）返回 None。"""
    if os.name != "nt":
        return None
    try:
        ver = ctypes.windll.version
        ver.GetFileVersionInfoSizeW.argtypes = [ctypes.c_wchar_p,
                                                wintypes.LPDWORD]
        ver.GetFileVersionInfoSizeW.restype = wintypes.DWORD
        size = ver.GetFileVersionInfoSizeW(path, None)
        if not size:
            return None
        data = ctypes.create_string_buffer(size)
        ver.GetFileVersionInfoW.argtypes = [ctypes.c_wchar_p, wintypes.DWORD,
                                            wintypes.DWORD, wintypes.LPVOID]
        ver.GetFileVersionInfoW.restype = wintypes.BOOL
        if not ver.GetFileVersionInfoW(path, 0, size, data):
            return None
        ver.VerQueryValueW.argtypes = [wintypes.LPCVOID, ctypes.c_wchar_p,
                                       ctypes.POINTER(ctypes.c_void_p),
                                       ctypes.POINTER(wintypes.UINT)]
        ver.VerQueryValueW.restype = wintypes.BOOL
        buf = ctypes.c_void_p()
        blen = wintypes.UINT()
        for sub in _VERSION_SUBBLOCKS:
            if ver.VerQueryValueW(data, sub, ctypes.byref(buf),
                                  ctypes.byref(blen)) and buf.value and blen.value:
                return ctypes.wstring_at(buf, blen.value - 1)
        return None
    except Exception:
        return None


# --------------------------------------------------------------------------
# 段三：注册面（WinDLL 实际加载；仅在线）
# --------------------------------------------------------------------------

_K32 = None


def _kernel32():
    """kernel32 with 显式句型：FreeLibrary 的 HANDLE 必须按 c_void_p 传，
    否则 64 位句柄塞 32 位默认转换 -> OverflowError -> 映射残留锁文件。"""
    global _K32
    if _K32 is None:
        _K32 = ctypes.WinDLL("kernel32")
        _K32.FreeLibrary.argtypes = [ctypes.c_void_p]
        _K32.FreeLibrary.restype = ctypes.c_int
    return _K32


def load_probe(path):
    """返回 True/False/None：True 加载成功、False 加载失败、
    None=离线环境无法探测（调用方必须如实标注，不许当绿记账）。
    探测完立即 FreeLibrary——探测不该把 DLL 常驻映射在本进程
    （残留映射会锁死文件，连临时目录清理都会被绊倒）。"""
    if os.name != "nt":
        return None
    lib = None
    try:
        lib = ctypes.WinDLL(path)
        return True
    except Exception:
        return False
    finally:
        if lib is not None:
            try:
                _kernel32().FreeLibrary(lib._handle)
            except Exception:
                pass


def audit_vcruntime(dst_dir, expected_hashes=None, lw=None):
    """复检视角的三段审计（只读）：存在 + 哈希对拍 + 版本 + 注册面。
    expected_hashes 来自 deploy 报告；None 时退化为"存在 + 版本"。
    返回 (rows, ok)；rows 元组 (file, stage, pass/fail/degraded, detail)。"""
    rows = []
    ok = True
    for fname in FILES:
        p = os.path.join(dst_dir, fname)
        if not os.path.isfile(p):
            rows.append((fname, "段一", "fail", "落盘缺失"))
            ok = False
            continue
        # 段一：哈希对拍（有期望值才比，离线无报告则如实降级）
        if expected_hashes is not None:
            actual = vxlib.sha256_file(p)
            if expected_hashes.get(fname) != actual:
                rows.append((fname, "段一", "fail",
                             "哈希漂移 {}".format(actual[:16])))
                ok = False
                continue
            rows.append((fname, "段一", "pass", actual[:16] + "..."))
        else:
            rows.append((fname, "段一", "degraded", "无基准哈希，仅验存在"))
        # 段二：版本
        ver = version_of(p)
        if ver:
            rows.append((fname, "段二", "pass", ver))
        else:
            rows.append((fname, "段二", "degraded", "版本提取不可用（离线或无资源段）"))
        # 段三：注册面
        probe = load_probe(p)
        if probe is True:
            rows.append((fname, "段三", "pass", "WinDLL 加载成功"))
        elif probe is False:
            rows.append((fname, "段三", "fail", "WinDLL 加载失败"))
            ok = False
        else:
            rows.append((fname, "段三", "degraded",
                         "加载探测需在线，未执行（如实降级，不记绿）"))
    return (rows, ok)
