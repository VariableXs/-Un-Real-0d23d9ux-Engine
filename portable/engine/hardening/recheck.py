# -*- coding: utf-8 -*-
"""recheck —— 只读复检（WP-103 · MD2 3.8 复检清单 / B-306 破坏后报警）。

复检四项，只读，零写动作（不碰注册表、不改文件）：
  1. 四板斧关键值读回 vs harden_quad.SPEC（同一份单一事实源）；
  2. VC 六件套三段审计（audit_vcruntime：存在/哈希/版本/注册面）；
  3. BCD 防自愈闸：重算 SHA-256 与 deploy 报告里的记录值比对——
     大更新重写 BCD 即哈希漂移，立刻红灯；
  4. 交接分区可达性：指定卷 + 标记文件存在（VARIX 域入口可及）。

退出码：0=全绿；2=有红（B-306：任何一项被篡改/漂移都以红灯报警）。
未执行项不记绿（"没测"绝不许冒充"通过"），这是复检的诚实底线。
selftest 用 fake 后端注入篡改，验证"破坏后报警"这条命根子。
"""

import argparse
import json
import os
import sys

import harden_quad
import harden_vcruntime
import vxlib


def recheck_quad(backend, lw, cs_root):
    """读回 SPEC 全部键值并逐项判定。返回 (rows, ok)。"""
    lw.enter("复检 1/4 四板斧关键值读回")
    rows = []
    ok = True
    for blade, rel, name, vtype, want in harden_quad.SPEC:
        key = harden_quad.spec_key(cs_root, rel)
        actual = backend.query(key, name)
        if vxlib.value_match(vtype, actual, want):
            rows.append((blade, key, name, "pass", str(actual)))
        else:
            rows.append((blade, key, name, "fail",
                         "读回 {} 期望 {}".format(actual, want)))
            ok = False
    return (rows, ok)


def recheck_bcd(lw, bcd_path, expected):
    """BCD 闸比对。无基准记录 = 闸没上 = fail：复检立场宁红勿漏。"""
    lw.enter("复检 3/4 BCD 防自愈闸")
    if not expected:
        lw.log("  FAIL 无 deploy 记录的基准哈希——闸未上")
        return ("fail", None)
    if not os.path.isfile(bcd_path):
        lw.log("  FAIL BCD 文件不存在: {}".format(bcd_path))
        return ("fail", None)
    good, actual, _ = harden_quad.blade4_bcd_gate(bcd_path, expected, lw)
    return ("pass" if good else "fail", actual)


def recheck_handoff(lw, volume, marker):
    """交接分区可达性：卷 + 标记文件（默认 VARIX 的 EFI 引导件）。"""
    lw.enter("复检 4/4 交接分区可达性")
    p = os.path.join(volume, marker.lstrip("\\/"))
    if os.path.isdir(volume) and os.path.isfile(p):
        lw.log("  PASS {} 可达，标记在位: {}".format(volume, marker))
        return ("pass", p)
    lw.log("  FAIL 交接分区不可达或标记缺失: {}".format(p))
    return ("fail", p)


def run_recheck(backend, lw, cs_root, dst_dir=None, report_path=None,
                bcd_path=None, handoff_volume=None, handoff_marker=None):
    """返回 (all_ok, report_dict)。report_dict 供 main 打印与 selftest 断言。

    判定口径：明确 fail 拉红灯；未执行项为 None（不红不绿）；
    但至少要有一项明确 pass 才算整体绿——防"全部没跑也报绿"。"""
    report = {"quad": None, "vc": None, "bcd": None, "handoff": None}

    # 复检 1/4：四板斧读回
    q_rows, q_ok = recheck_quad(backend, lw, cs_root)
    report["quad"] = {"rows": q_rows, "ok": q_ok}

    # deploy 报告（若有）：VC 哈希基准 + BCD 闸基准，只读一次
    rep = None
    if report_path and os.path.isfile(report_path):
        with open(report_path, "r", encoding="utf-8") as f:
            rep = json.load(f)
        report["report_path"] = report_path

    # 复检 2/4：VC 六件套三段审计
    if dst_dir:
        lw.enter("复检 2/4 VC 六件套三段审计")
        expected_hashes = (rep or {}).get("vcruntime", {}).get("hashes")
        v_rows, v_ok = harden_vcruntime.audit_vcruntime(
            dst_dir, expected_hashes, lw)
        report["vc"] = {"rows": v_rows, "ok": v_ok}
    else:
        lw.log("  SKIP 未指定 --dst，VC 审计未执行（不记绿）")

    # 复检 3/4：BCD 闸
    if bcd_path:
        gate_expected = (rep or {}).get("gate", {}).get("sha256")
        st, actual = recheck_bcd(lw, bcd_path, gate_expected)
        report["bcd"] = {"status": st, "sha256": actual}
    else:
        lw.log("  SKIP 未指定 --bcd，BCD 闸未执行（不记绿）")

    # 复检 4/4：交接分区可达性
    if handoff_volume:
        st, p = recheck_handoff(
            lw, handoff_volume, handoff_marker or "\\EFI\\BOOT\\BOOTX64.EFI")
        report["handoff"] = {"status": st, "path": p}
    else:
        lw.log("  SKIP 未指定 --handoff，交接分区检查未执行（不记绿）")

    parts = [q_ok]
    parts.append(report["vc"]["ok"] if report["vc"] else None)
    parts.append((report["bcd"] or {}).get("status") == "pass"
                 if report["bcd"] else None)
    parts.append((report["handoff"] or {}).get("status") == "pass"
                 if report["handoff"] else None)
    all_ok = all(p is not False for p in parts) and any(p is True for p in parts)
    return (all_ok, report)


def print_report(lw, report):
    lw.enter("复检汇总")
    for blade, key, name, st, detail in report["quad"]["rows"]:
        lw.log("  [{}] {} {}\\{} = {}".format(st.upper(), blade, key, name, detail))
    if report["vc"]:
        for fname, stage, st, detail in report["vc"]["rows"]:
            lw.log("  [{}] VC {} {}: {}".format(st.upper(), stage, fname, detail))
    if report["bcd"]:
        lw.log("  [{}] BCD 闸 sha256={}".format(
            report["bcd"]["status"].upper(),
            (report["bcd"]["sha256"] or "")[:16]))
    if report["handoff"]:
        lw.log("  [{}] 交接分区 {}".format(
            report["handoff"]["status"].upper(), report["handoff"]["path"]))


def main(argv=None):
    ap = argparse.ArgumentParser(description="WP-103 只读复检（退出码 2=有红）")
    ap.add_argument("--backend", choices=["cli", "fake"], default="cli",
                    help="注册表后端：cli=reg.exe（生产）；fake=内存（selftest）")
    ap.add_argument("--cs", default="HKLM\\SYSTEM\\CurrentControlSet",
                    help="ControlSet 根；离线 hive 用 HKLM\\VXHARD\\ControlSet001")
    ap.add_argument("--dst", default=None, help="VC 六件套落盘目录（System32）")
    ap.add_argument("--report", default=None, help="deploy 报告 JSON（哈希/闸基准）")
    ap.add_argument("--bcd", default=None, help="BCD 文件路径")
    ap.add_argument("--handoff", default=None, help="交接分区卷根，如 F:")
    ap.add_argument("--marker", default="\\EFI\\BOOT\\BOOTX64.EFI")
    ap.add_argument("--log", default="recheck.log", help="遗言/复检日志路径")
    args = ap.parse_args(argv)

    lw = vxlib.LastWords(args.log)
    try:
        if args.backend == "fake":
            raise SystemExit("fake 后端仅供 selftest 使用（见 run_all.py selftest）")
        backend = vxlib.RegCliBackend(lw)
        all_ok, report = run_recheck(
            backend, lw, args.cs, dst_dir=args.dst, report_path=args.report,
            bcd_path=args.bcd, handoff_volume=args.handoff,
            handoff_marker=args.marker)
        print_report(lw, report)
        lw.log("=== RECHECK {} ===".format("ALL GREEN" if all_ok else "HAS RED"))
        return 0 if all_ok else 2
    finally:
        lw.close()


if __name__ == "__main__":
    sys.exit(main())
