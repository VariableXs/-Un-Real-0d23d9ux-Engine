# -*- coding: utf-8 -*-
"""harden_quad —— 四板斧本体（WP-103 · MD2 篇 3.2~3.5）。

从 _attic/vx-stability4.py 收编升格。v1 是一次性手工脚本；本件按四纪律
（遗言/装载/写后必读/幂等）重构为可重跑的产品函数：

  板斧一  UAS -> BOT：UASPStor.ImagePath 改指 usbstor.sys（让 UAS 服务
          跑 BOT 本体，绕开 Y7000 USB 桥 UASP 固件超时丢盘），四个 USB
          存储相关驱动 Start 全部钉 0。
  板斧二  禁 USB 选择性挂起：Services\\USB\\DisableSelectiveSuspend=1，
          并对 ENUM\\USB 下每个设备实例尽力而为关 SelectiveSuspendEnabled。
  板斧三  放宽 UASPStor 超时：IoTimeoutValue 0xf(15) -> 0x50(80)。
  板斧四  BCD 防自愈闸：对 BCD 文件做 SHA-256 记录（record）与比对
          （check）——大更新"顺手重写 BCD"即哈希变化，复检立刻红灯。
          文件级哈希离线可用，不依赖 bcdedit（也不给它写坏 BCD 的机会）。

附带项（MD2 3.5）：PortableOperatingSystem=1（便携身份标志）、
CrashControl.AutoReboot=0（蓝屏停在屏幕上供诊断）、DumpEnabled=1（留转储）。

身份防呆（先于一切写动作）：Control\\PortableOperatingSystem==1 才认是
U 盘便携系统；值缺失时回退 UASPStor.Start==0 指纹（v1 的原始身份确认）；
对不上就中止并输出人话——绝不对内置盘 Windows 动刀。

零文件操作红线（B-302）：本件只准碰注册表（BCD 哈希经 vxlib.sha256_file
只读）；任何复制/删除/改名/写文件 API 一律不出现，由 run_all.py selftest
对本文件做静态 token 扫描执行审计。

键路径口径：cs_root 为 ControlSet 根，例如离线 HKLM\\VXHARD\\ControlSet001
或在线 HKLM\\SYSTEM\\CurrentControlSet。全部具体键/值集中在下方 SPEC，
recheck.py 复检时 import 同一份——期望值永不写两遍。
"""

import vxlib

# --------------------------------------------------------------------------
# 单一事实源：键与期望值（deploy 与 recheck 共用，写两遍必漂移）
# --------------------------------------------------------------------------

IMAGE_PATH_WANT = "\\SystemRoot\\System32\\drivers\\usbstor.sys"
START_DRIVERS = ("usbstor", "USBXHCI", "USBHUB3", "UASPStor")
IO_TIMEOUT_WANT = 80          # 0x50；出厂 0xf=15
ENUM_BLADE2 = "b2-suspend"
ENUM_DEV_VALUE = "SelectiveSuspendEnabled"

# (板斧, Services 相对子键, 值名, 类型, 期望)
SPEC = [
    ("b1-uas-bot",    "UASPStor",             "ImagePath",               "REG_EXPAND_SZ", IMAGE_PATH_WANT),
    ("b1-uas-bot",    "usbstor",              "Start",                   "REG_DWORD",     0),
    ("b1-uas-bot",    "USBXHCI",              "Start",                   "REG_DWORD",     0),
    ("b1-uas-bot",    "USBHUB3",              "Start",                   "REG_DWORD",     0),
    ("b1-uas-bot",    "UASPStor",             "Start",                   "REG_DWORD",     0),
    ("b2-suspend",    "USB",                  "DisableSelectiveSuspend", "REG_DWORD",     1),
    ("b3-iotimeout",  "UASPStor\\Parameters", "IoTimeoutValue",          "REG_DWORD",     IO_TIMEOUT_WANT),
    ("附带",          "..\\Control",                    "PortableOperatingSystem", "REG_DWORD", 1),
    ("附带",          "..\\Control\\CrashControl",      "AutoReboot",              "REG_DWORD", 0),
    ("附带",          "..\\Control\\CrashControl",      "DumpEnabled",             "REG_DWORD", 1),
]

SPEC_RELATIVE_ROOT = ".."   # 附带项相对 Services 根回退一级到 ControlSet 根


def spec_key(cs_root, rel):
    """把 SPEC 的相对子键解析为完整键路径。"""
    if rel.startswith(SPEC_RELATIVE_ROOT):
        return cs_root + rel[len(SPEC_RELATIVE_ROOT):]
    return cs_root + "\\Services\\" + rel


def svc(cs_root, name):
    return cs_root + "\\Services\\" + name


# --------------------------------------------------------------------------
# 身份防呆：先确认这是 U 盘便携系统，再谈动手
# --------------------------------------------------------------------------

def identify(backend, lw, cs_root):
    """返回 (ok, message)。不通过绝不写任何值。

    主指纹：Control\\PortableOperatingSystem==1（Windows 便携系统的官方
    身份位）。值缺失时回退 v1 的 UASPStor.Start==0 指纹（U 盘 hive 特征，
    内置盘上 UASPStor.Start 为 0x3 按需启动）。"""
    ctrl = cs_root + "\\Control"
    portable = backend.query(ctrl, "PortableOperatingSystem")
    if portable is not None:
        if vxlib.value_match("REG_DWORD", portable, 1):
            return (True, "身份确认：PortableOperatingSystem=1（便携系统）")
        return (False, "中止：PortableOperatingSystem={}，这不是 U 盘便携系统"
                "（或身份位被改过）。四板斧拒绝执行——别把内置盘当 U 盘修。".format(portable))
    # 回退指纹：UASPStor.Start == 0
    start = backend.query(svc(cs_root, "UASPStor"), "Start")
    if vxlib.value_match("REG_DWORD", start, 0):
        return (True, "身份确认（回退指纹）：PortableOperatingSystem 缺失但 "
                "UASPStor.Start=0，判定为 U 盘 hive；将补写便携标志")
    return (False, "中止：PortableOperatingSystem 缺失且 UASPStor.Start={}，"
            "无法确认这是 U 盘便携系统 hive，拒绝动手。".format(start))


# --------------------------------------------------------------------------
# 四板斧（每板斧一个函数：吃 backend / lw / journal / cs_root，返回行）
# --------------------------------------------------------------------------

def blade1_uas_to_bot(backend, lw, journal, cs_root):
    lw.enter("板斧一 UAS->BOT 重定向")
    rows = _apply_spec(backend, lw, journal, cs_root, "b1-uas-bot")
    return rows


def blade2_suspend(backend, lw, journal, cs_root):
    lw.enter("板斧二 禁 USB 选择性挂起")
    rows = _apply_spec(backend, lw, journal, cs_root, ENUM_BLADE2)
    # 设备实例级尽力而为：ENUM\\USB 下每个实例的 Device Parameters。
    # 离线 hive 枚举子键可能为空、个别实例拒绝写入都属常态——记日志不记红。
    enum_root = cs_root + "\\Enum\\USB"
    try:
        for vidpid in backend.subkeys(enum_root):
            for inst in backend.subkeys(vidpid):
                dp = inst + "\\Device Parameters"
                try:
                    st, _, _ = vxlib.ensure_value(
                        backend, lw, journal, dp, ENUM_DEV_VALUE,
                        "REG_DWORD", 0)
                    rows.append((ENUM_BLADE2, st, dp, ENUM_DEV_VALUE))
                except Exception as ex:                      # 尽力而为
                    lw.log("  尽力而为失败 {}: {}".format(dp, ex))
    except Exception as ex:
        lw.log("  ENUM\\USB 枚举不可用（尽力而为跳过）: {}".format(ex))
    return rows


def blade3_iotimeout(backend, lw, journal, cs_root):
    lw.enter("板斧三 放宽 UASPStor 超时 0xf->0x50")
    return _apply_spec(backend, lw, journal, cs_root, "b3-iotimeout")


def blade4_bcd_gate(bcd_path, expected=None, lw=None):
    """record：expected=None，返回 (True, sha256, None) 由调用方落盘记录；
    check：expected=记录值，重算比对。文件级 SHA-256，离线可用。"""
    actual = vxlib.sha256_file(bcd_path)
    if lw:
        lw.log("  BCD sha256({}) = {}{}".format(
            bcd_path, actual[:16] + "...", "" if expected is None
            else (" 与记录一致" if actual == expected else " != 记录 " + expected[:16] + "...")))
    if expected is None:
        return (True, actual, None)
    return (actual == expected, actual, expected)


def incidentals(backend, lw, journal, cs_root):
    lw.enter("附带项 便携标志/崩溃不复启/转储")
    return _apply_spec(backend, lw, journal, cs_root, "附带")


def _apply_spec(backend, lw, journal, cs_root, blade):
    rows = []
    for b, rel, name, vtype, want in SPEC:
        if b != blade:
            continue
        key = spec_key(cs_root, rel)
        try:
            st, before, after = vxlib.ensure_value(
                backend, lw, journal, key, name, vtype, want)
            rows.append((blade, st, key, name))
        except Exception as ex:
            lw.log("  FAIL {}\\{}: {}".format(key, name, ex))
            rows.append((blade, "fail", key, name))
    return rows


# --------------------------------------------------------------------------
# 总装：身份确认 -> 四板斧 -> 附带 -> BCD 闸
# --------------------------------------------------------------------------

def run_quad(backend, lw, journal, cs_root,
             bcd_path=None, bcd_expected=None):
    """返回 summary：{"ok": bool, "rows": [...], "bcd": {...}, "identity": msg}。
    rows 元组 (blade, status, key, name)；status ∈ {ok, skip, fail}。"""
    lw.enter("run_quad 身份确认")
    ok, msg = identify(backend, lw, cs_root)
    lw.log("  " + msg)
    summary = {"ok": False, "rows": [], "bcd": None, "identity": msg}
    if not ok:
        # 身份不对：遗言已落，交由上层决定退出码；这里直接返回。
        return summary

    rows = []
    rows += blade1_uas_to_bot(backend, lw, journal, cs_root)
    rows += blade2_suspend(backend, lw, journal, cs_root)
    rows += blade3_iotimeout(backend, lw, journal, cs_root)
    rows += incidentals(backend, lw, journal, cs_root)
    summary["rows"] = rows
    summary["ok"] = all(st != "fail" for _, st, _, _ in rows)

    if bcd_path:
        lw.enter("板斧四 BCD 防自愈闸")
        good, actual, exp = blade4_bcd_gate(bcd_path, bcd_expected, lw)
        summary["bcd"] = {"path": bcd_path, "sha256": actual,
                          "expected": exp, "ok": good}
        summary["ok"] = summary["ok"] and good
    return summary
