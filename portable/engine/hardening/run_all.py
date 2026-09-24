# -*- coding: utf-8 -*-
"""run_all —— WP-103 加固包编排入口（MD2 篇 3 / MD3 WP-103 施工要点）。

子命令：
  deploy    身份确认 -> 四板斧 -> 附带项 -> BCD 闸记录 ->（可选）VC 六件套
            -> 落 deploy 报告（含 VC 哈希表 + BCD 闸基准）。生产（cli 后端）
            必须 --yes 显式确认；fake 后端零副作用。
  recheck   只读复检四项，退出码 2=有红（详见 recheck.py）。
  rollback  按 journal.jsonl 反向恢复注册表（cli 后端同样必须 --yes）。
  selftest  宿主侧七组证据（B-302~B-307 + 装载/回滚纪律），全程 fake 后端
            + 临时目录，零真实注册表接触——证据可复现：本命令即复现命令。

设计口径：deploy 与 recheck 之间唯一的契约文件是 deploy 报告 JSON
（gate.sha256 + vcruntime.hashes）；journal 是回滚账本；遗言日志逐行 flush。
"""

import argparse
import json
import os
import re
import shutil
import sys
import tempfile

import harden_quad
import harden_vcruntime
import recheck as recheck_mod
import vxlib

HARDENING_DIR = os.path.dirname(os.path.abspath(__file__))
DEFAULT_REPORT_DIR = os.path.join(HARDENING_DIR, "reports")

# B-302 静态审计：harden_quad.py（四板斧本体）禁止出现的文件操作 token。
# 扫描对象只有 harden_quad.py——harden_vcruntime 是有清单+哈希的豁免件，
# vxlib/run_all 是基础设施（报告/日志/临时件）。
FORBIDDEN_TOKENS = [
    "shutil", "os.remove", "os.unlink", "os.rename", "os.replace",
    "copyfile", "copy2", "rmtree", "open(",
]


# --------------------------------------------------------------------------
# deploy
# --------------------------------------------------------------------------

def do_deploy(backend, lw, cs_root, bcd_path=None, src_dir=None, dst_dir=None,
              report_dir=None):
    """一次完整部署。返回 summary dict；report_dir 给定则落盘报告+账本。"""
    journal = vxlib.Journal(os.path.join(report_dir or ".", "journal.jsonl"))
    summary = {"ts": vxlib._now(), "cs_root": cs_root}

    quad = harden_quad.run_quad(backend, lw, journal, cs_root, bcd_path=bcd_path)
    summary["quad"] = quad
    if not quad["ok"]:
        lw.log("deploy 中止：四板斧存在红项/身份不符（详见上方行）")
        journal.flush()
        summary["ok"] = False
        return summary

    # VC 六件套（可选）：给了 src+dst 才执行；哈希表进 deploy 报告。
    vc_hashes = None
    if src_dir and dst_dir:
        v_rows, v_ok = harden_vcruntime.ensure_vcruntime(src_dir, dst_dir, lw)
        vc_hashes = harden_vcruntime.collect_hashes(dst_dir)
        summary["vcruntime"] = {"rows": v_rows, "ok": v_ok, "hashes": vc_hashes}
        if not v_ok:
            lw.log("deploy 中止：VC 六件套存在红项")
            journal.flush()
            summary["ok"] = False
            return summary

    journal.flush()

    # deploy 报告：recheck 的唯一契约（BCD 闸基准 + VC 哈希基准）。
    summary["ok"] = True
    if report_dir:
        os.makedirs(report_dir, exist_ok=True)
        report = {
            "ts": summary["ts"], "cs_root": cs_root,
            "gate": quad.get("bcd"),
            "vcruntime": {"hashes": vc_hashes},
        }
        rp = os.path.join(report_dir, "deploy_report.json")
        with open(rp, "w", encoding="utf-8") as f:
            json.dump(report, f, ensure_ascii=False, indent=2)
        summary["report_path"] = rp
        lw.log("deploy 报告: " + rp)
    return summary


def cmd_deploy(args):
    lw = vxlib.LastWords(os.path.join(args.report_dir, "deploy.log"))
    try:
        os.makedirs(args.report_dir, exist_ok=True)
        if args.backend == "fake":
            backend = vxlib.FakeRegBackend()
        else:
            if not args.yes:
                print("生产 deploy 将真实写注册表/hive：必须加 --yes 显式确认。")
                return 3
            backend = vxlib.RegCliBackend(lw)

        if args.backend == "fake":
            cs_root, mount = args.cs, None
        elif args.mount:
            mount = vxlib.HiveMount(backend, "HKLM\\VXHARD", lw)
            hive = os.path.join(args.mount, "Windows", "System32",
                                "config", "SYSTEM")
            mount.load(hive)
            cs_root = "HKLM\\VXHARD\\ControlSet001"
        else:
            mount = None
            cs_root = args.cs

        try:
            summary = do_deploy(
                backend, lw, cs_root,
                bcd_path=args.bcd,
                src_dir=args.src, dst_dir=args.dst,
                report_dir=args.report_dir)
        finally:
            if mount:
                mount.unload()   # 装载纪律：正常/异常路径都卸载
        lw.log("=== DEPLOY {} ===".format(
            "ALL GREEN" if summary.get("ok") else "HAS RED / ABORTED"))
        return 0 if summary.get("ok") else 2
    finally:
        lw.close()


def cmd_recheck(args):
    argv = ["--backend", args.backend, "--cs", args.cs, "--log",
            os.path.join(args.report_dir, "recheck.log")]
    for flag, val in (("--dst", args.dst), ("--report", args.report),
                      ("--bcd", args.bcd), ("--handoff", args.handoff)):
        if val:
            argv += [flag, val]
    return recheck_mod.main(argv)


def cmd_rollback(args):
    lw = vxlib.LastWords(os.path.join(args.report_dir, "rollback.log"))
    try:
        jpath = os.path.join(args.report_dir, "journal.jsonl")
        if not os.path.isfile(jpath):
            lw.log("无账本可回滚: " + jpath)
            return 2
        if args.backend == "fake":
            backend = vxlib.FakeRegBackend()
        else:
            if not args.yes:
                print("rollback 将真实改写注册表：必须加 --yes 显式确认。")
                return 3
            backend = vxlib.RegCliBackend(lw)
        journal = vxlib.Journal(jpath)
        with open(jpath, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                row = json.loads(line)
                journal.rows.append(row)
        ok = journal.rollback(backend, lw)
        lw.log("=== ROLLBACK 完成，还原 {} 项 ===".format(ok))
        return 0
    finally:
        lw.close()


# --------------------------------------------------------------------------
# selftest：七组宿主证据（B-302~307 + 装载/回滚纪律）
# --------------------------------------------------------------------------

def _seed_portable(backend):
    """预置便携身份位（identify 主指纹所需）。"""
    backend.set_value("HKLM\\VXFAKE\\ControlSet001\\Control",
                      "PortableOperatingSystem", "REG_DWORD", 1)


def _sample_source_dir():
    """selftest 样本源：直接用 System32 真六件（与产线 SRC 同源）。
    实证教训：不能用"一份样本 DLL 冒充六个名字"——内容与名字错配的
    副本会掉进导入绑定地狱（WinError 127），且核心 DLL 副本被进程
    钉死锁文件。真六件按本名复制，导入绑定/版本/加载三段全部成立。
    六件不齐则返回 None（诚实中止，不静默降级）。"""
    sys32 = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"),
                         "System32")
    if all(os.path.isfile(os.path.join(sys32, f))
           for f in harden_vcruntime.FILES):
        return sys32
    return None


def _snapshot(backend):
    return dict(backend.store)


def selftest(lw, tmp):
    """跑七组证据，返回 (results, all_ok)。results 元组 (id, name, ok, detail)。"""
    results = []
    quad_cs = "HKLM\\VXFAKE\\ControlSet001"

    # --- 证据① 身份防呆：不是便携系统就零写动作 ---------------------------
    lw.enter("证据① 身份防呆（PortableOperatingSystem=0 中止）")
    b = vxlib.FakeRegBackend()
    b.set_value(quad_cs + "\\Control", "PortableOperatingSystem", "REG_DWORD", 0)
    b.ops = []          # 预置不算数：只审计 run_quad 期间的动作
    j = vxlib.Journal(os.path.join(tmp, "j0.jsonl"))
    s = harden_quad.run_quad(b, lw, j, quad_cs)
    wrote = [op for op in b.ops if op[0] in ("set", "delete")]
    e1 = (not s["ok"]) and (not wrote) and ("中止" in s["identity"])
    results.append(("①", "身份防呆：非便携系统零写动作中止", e1, s["identity"]))

    # --- 证据② 写后必读 B-303：每步写完读回==期望 --------------------------
    lw.enter("证据② 写后必读（B-303）")
    b = vxlib.FakeRegBackend()
    _seed_portable(b)
    j = vxlib.Journal(os.path.join(tmp, "j1.jsonl"))
    s = harden_quad.run_quad(b, lw, j, quad_cs)
    ok2 = s["ok"]
    for blade, rel, name, vtype, want in harden_quad.SPEC:
        key = harden_quad.spec_key(quad_cs, rel)
        ok2 = ok2 and vxlib.value_match(vtype, b.query(key, name), want)
    results.append(("②", "写后必读：终态逐键==SPEC（B-303）", ok2,
                    "rows={} 全部 ok".format(len(s["rows"]))))

    # --- 证据③ 幂等三连跑 B-304：第三遍全 SKIP 且终态不漂 -----------------
    lw.enter("证据③ 幂等三连跑（B-304）")
    b = vxlib.FakeRegBackend()
    _seed_portable(b)
    j = vxlib.Journal(os.path.join(tmp, "j2.jsonl"))
    snaps, statuses = [], []
    for _ in range(3):
        s = harden_quad.run_quad(b, lw, j, quad_cs)
        statuses.append([st for _, st, _, _ in s["rows"]])
        snaps.append(_snapshot(b))
    ok3 = (all(st == "skip" for st in statuses[2])
           and all(st == "skip" for st in statuses[1])
           and snaps[0] == snaps[1] == snaps[2])
    results.append(("③", "幂等三连跑：第 2/3 遍全 SKIP、终态零漂移（B-304）",
                    ok3, "第3遍 SKIP 数={}".format(len(statuses[2]))))

    # --- 证据④ 破坏后报警 B-306：篡改注册表 + BCD 字节 -> recheck 红灯 ----
    lw.enter("证据④ 破坏后报警（B-306）")
    b = vxlib.FakeRegBackend()
    _seed_portable(b)
    # 用 System32 真六件当样本（与产线 SRC 同源，按本名复制）。
    # 前置硬检查：六件不齐（精简系统）则明确中止——
    # 诚实失败优于破碎降级，绝不静默跳过证据。
    sample_src = _sample_source_dir()
    if sample_src is None:
        raise RuntimeError("selftest 需要 System32 真六件套（与产线 SRC 同源），"
                           "宿主上不齐，无法执行 VC 相关证据")
    tmpvc_src = os.path.join(tmp, "vcsrc")
    tmpvc_dst = os.path.join(tmp, "vcdst")
    os.makedirs(tmpvc_src)
    os.makedirs(tmpvc_dst)
    for fname in harden_vcruntime.FILES:
        shutil.copyfile(os.path.join(sample_src, fname),
                        os.path.join(tmpvc_src, fname))
    bcd = os.path.join(tmp, "BCD")
    with open(bcd, "wb") as f:
        f.write(b"VARIX-BCD-STAND-IN-" + bytes(range(64)))
    rpt_dir = os.path.join(tmp, "rpt306")
    os.makedirs(rpt_dir)
    s = do_deploy(b, lw, quad_cs, bcd_path=bcd, src_dir=tmpvc_src,
                  dst_dir=tmpvc_dst, report_dir=rpt_dir)
    ok4 = s.get("ok") is True
    # 篡改一：IoTimeoutValue 0x50 -> 0xf（模拟大更新拆家）
    b.inject_value(harden_quad.spec_key(quad_cs, "UASPStor\\Parameters"),
                   "IoTimeoutValue", 15)
    # 篡改二：BCD 字节翻转（模拟 BCD 重写）
    with open(bcd, "rb") as f:
        blob = bytearray(f.read())
    blob[0] ^= 0xFF
    with open(bcd, "wb") as f:
        f.write(blob)
    rb = vxlib.FakeRegBackend()
    rb.store = dict(b.store)          # recheck 用独立 fake 读同一状态
    rb.subkey_set = set(b.subkey_set)
    lw2 = vxlib.LastWords(os.path.join(tmp, "lw306.log"))
    all_ok, report = recheck_mod.run_recheck(
        rb, lw2, quad_cs, dst_dir=tmpvc_dst,
        report_path=s.get("report_path"), bcd_path=bcd)
    lw2.close()   # 临时目录清理前必须落柄（Windows 句柄占用即 PermissionError）
    iot_row = [r for r in report["quad"]["rows"]
               if r[2] == "IoTimeoutValue"][0]
    ok4 = ok4 and (all_ok is False) and (iot_row[3] == "fail") \
        and (report["bcd"]["status"] == "fail")
    results.append(("④", "破坏后报警：篡改 IoTimeoutValue+BCD -> 全线红灯（B-306）",
                    ok4, "recheck all_ok={} bcd={} iot={}".format(
                        all_ok, report["bcd"]["status"], iot_row[3])))

    # --- 证据⑤ 遗言机制 B-307：致命异常 -> 时间戳+步骤+全栈，无 CLEAN EXIT --
    lw.enter("证据⑤ 遗言机制（B-307）")
    b = vxlib.FakeRegBackend()
    _seed_portable(b)
    b.fail_on = ("query", "ControlSet001")   # identity 首查即炸 -> 冒泡为致命
    lw3 = vxlib.LastWords(os.path.join(tmp, "lw307.log"))
    old_hook = sys.excepthook
    try:
        try:
            harden_quad.run_quad(b, lw3, vxlib.Journal(
                os.path.join(tmp, "j3.jsonl")), quad_cs)
            fatal = False
        except OSError:
            fatal = True
            lw3._hook(OSError, OSError("injected"), sys.exc_info()[2])
    finally:
        sys.excepthook = old_hook
        lw3.fh.close()
    with open(os.path.join(tmp, "lw307.log"), "r", encoding="utf-8") as f:
        txt = f.read()
    ok5 = (fatal and "FATAL@" in txt and "injected" in txt
           and "Traceback" in txt and "CLEAN EXIT" not in txt
           and "run_quad" in txt)
    results.append(("⑤", "遗言：FATAL+时间戳+全栈落盘、无 CLEAN EXIT（B-307）",
                    ok5, "fatal={} log={}B".format(fatal, len(txt))))

    # --- 证据⑤b 单值写失败 -> fail 行如实记红（不炸、不无声） -------------
    lw.enter("证据⑤b 写失败记红")
    b = vxlib.FakeRegBackend()
    _seed_portable(b)
    b.fail_on = ("set", "UASPStor")
    s = harden_quad.run_quad(b, lw, vxlib.Journal(
        os.path.join(tmp, "j4.jsonl")), quad_cs)
    fail_rows = [r for r in s["rows"] if r[1] == "fail"]
    ok5b = (not s["ok"]) and len(fail_rows) == 1   # 注入吃掉 ImagePath 一次写
    results.append(("⑤b", "写失败：fail 行如实记红、汇总 not ok", ok5b,
                    "fail 行数={}".format(len(fail_rows))))

    # --- 证据⑥ 零文件操作静态审计 B-302 -----------------------------------
    lw.enter("证据⑥ 零文件操作静态审计（B-302）")
    with open(os.path.join(HARDENING_DIR, "harden_quad.py"),
              "r", encoding="utf-8") as f:
        src = f.read()
    hits = [t for t in FORBIDDEN_TOKENS if t in src]
    results.append(("⑥", "静态审计：harden_quad.py 零文件操作 token（B-302）",
                    not hits, "命中={}".format(hits or "无")))

    # --- 证据⑦ VC 哈希/版本/加载三段逻辑（B-305 宿主面） -------------------
    lw.enter("证据⑦ VC 三段逻辑宿主面（B-305）")
    tmpvc2 = os.path.join(tmp, "vcdst2")
    os.makedirs(tmpvc2)
    rows, ok_vc = harden_vcruntime.ensure_vcruntime(tmpvc_src, tmpvc2, lw)
    copied = [r for r in rows if r[1] == "copied"]
    rows2, _ = harden_vcruntime.ensure_vcruntime(tmpvc_src, tmpvc2, lw)
    skips = [r for r in rows2 if r[1] == "skip-existing"]
    ver = harden_vcruntime.version_of(
        os.path.join(tmpvc2, "vcruntime140.dll"))
    probe_true = harden_vcruntime.load_probe(
        os.path.join(tmpvc2, "vcruntime140.dll")) is True
    probe_false = harden_vcruntime.load_probe(
        os.path.join(tmpvc2, "no_such.dll")) is False
    hashes = harden_vcruntime.collect_hashes(tmpvc2)
    arows, aok = harden_vcruntime.audit_vcruntime(tmpvc2, hashes, lw)
    ok7 = (ok_vc and len(copied) == 6 and len(skips) == 6
           and bool(ver) and re.match(r"\d+", ver)
           and probe_true and probe_false and aok)
    results.append(("⑦", "VC 三段：哈希幂等复制/版本提取/加载探针双向（B-305）",
                    bool(ok7), "ver={} audit_ok={}".format(ver, aok)))

    # --- 证据⑧ 装载纪律 + 回滚账本 ----------------------------------------
    lw.enter("证据⑧ 装载纪律与回滚")
    b = vxlib.FakeRegBackend()
    _seed_portable(b)
    hm = vxlib.HiveMount(b, "HKLM\\VXFAKE", lw)
    hm.load("X:\\Windows\\System32\\config\\SYSTEM")
    loaded_in = hm._loaded
    try:
        raise RuntimeError("boom")
    except RuntimeError:
        pass
    finally:
        hm.unload()                    # 异常路径也卸载
    mount_ok = loaded_in and (not hm._loaded)
    # 回滚：deploy 后按账本反向恢复 == 部署前快照
    b2 = vxlib.FakeRegBackend()
    _seed_portable(b2)
    pre = _snapshot(b2)
    jr = vxlib.Journal(os.path.join(tmp, "j5.jsonl"))
    harden_quad.run_quad(b2, lw, jr, quad_cs)
    jr.flush()
    jr2 = vxlib.Journal(os.path.join(tmp, "j5.jsonl"))
    with open(jr2.path, "r", encoding="utf-8") as f:
        for line in f:
            if line.strip():
                jr2.rows.append(json.loads(line))
    jr2.rollback(b2, lw)
    rollback_ok = _snapshot(b2) == pre
    results.append(("⑧", "装载纪律（异常路径卸载）+ 回滚还原部署前快照",
                    mount_ok and rollback_ok,
                    "mount={} rollback={}".format(mount_ok, rollback_ok)))

    return results, all(r[2] for r in results)


def cmd_selftest(args):
    os.makedirs(args.report_dir, exist_ok=True)
    lw = vxlib.LastWords(os.path.join(args.report_dir, "selftest.log"))
    try:
        with tempfile.TemporaryDirectory() as tmp:
            results, all_ok = selftest(lw, tmp)
        lw.enter("selftest 汇总")
        lines = [
            "# WP-103 加固包 selftest 证据（B-302~307 宿主面）",
            "",
            "- 执行时间：{}".format(vxlib._now()),
            "- 复现命令：`python portable/engine/hardening/run_all.py selftest`",
            "- 口径：fake 注册表后端 + 临时目录，零真实系统接触",
            "",
        ]
        for eid, name, ok, detail in results:
            mark = "PASS" if ok else "FAIL"
            lw.log("  [{}] {} {} —— {}".format(mark, eid, name, detail))
            lines.append("- **{}** {} {}（{}）".format(mark, eid, name, detail))
        lines.append("")
        lines.append("**总体：{}**".format("ALL GREEN" if all_ok else "HAS RED"))
        ev = os.path.join(args.report_dir, "selftest_evidence.md")
        with open(ev, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
        lw.log("证据报告: " + ev)
        lw.log("=== SELFTEST {} ===".format("ALL GREEN" if all_ok else "HAS RED"))
        print("SELFTEST:", "ALL GREEN" if all_ok else "HAS RED")
        print("证据报告:", ev)
        return 0 if all_ok else 2
    finally:
        lw.close()


def main(argv=None):
    ap = argparse.ArgumentParser(description="WP-103 加固包编排入口")
    ap.add_argument("cmd", choices=["deploy", "recheck", "rollback", "selftest"])
    ap.add_argument("--backend", choices=["cli", "fake"], default="cli")
    ap.add_argument("--mount", default=None,
                    help="U 盘 Windows 分区盘根（如 X:）——离线 hive 模式")
    ap.add_argument("--cs", default="HKLM\\SYSTEM\\CurrentControlSet",
                    help="在线模式 ControlSet 根（与 --mount 二选一）")
    ap.add_argument("--bcd", default=None, help="BCD 文件路径（板斧四闸对象）")
    ap.add_argument("--src", default=None, help="VC 六件套源目录（内置盘 System32）")
    ap.add_argument("--dst", default=None, help="VC 六件套落盘目录（U 盘 System32）")
    ap.add_argument("--report", default=None, help="recheck 用的 deploy 报告 JSON")
    ap.add_argument("--report-dir", default=DEFAULT_REPORT_DIR)
    ap.add_argument("--yes", action="store_true",
                    help="生产写确认（cli 后端的 deploy/rollback 必需）")
    args = ap.parse_args(argv)

    os.makedirs(args.report_dir, exist_ok=True)
    if args.cmd == "deploy":
        return cmd_deploy(args)
    if args.cmd == "recheck":
        return cmd_recheck(args)
    if args.cmd == "rollback":
        return cmd_rollback(args)
    if args.cmd == "selftest":
        return cmd_selftest(args)
    return 2


if __name__ == "__main__":
    sys.exit(main())
