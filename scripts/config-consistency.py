#!/usr/bin/env python3
"""任务68（AI-P）· 三处配置一致性校验器（md5 对齐，单一事实源=SHARED）。

总案阶段9 第 6 条：boot-select.json / apps.json / 白名单（vfsguard-rules.json）
在 引导器 / VARIX / Windows 三处的副本，与 SHARED 分区上的单一事实源做
md5 对齐；冲突时以 SHARED 为准收敛（--fix 执行拷贝覆盖，默认只报告）。

三处布局（U 盘五分区语义，本地演练用目录模拟）：
  --boot-dir     引导器分区（boot-select.json）
  --varix-dir    VARIX 系统分区（apps.json / vfsguard-rules.json）
  --windows-dir  Windows 引擎半区（apps.json / vfsguard-rules.json）
  --shared-dir   SHARED 分区 = 单一事实源（三份配置的基准副本）

用法：
  python scripts/config-consistency.py --root <usb-root>          # root/{boot,varix,windows,shared}
  python scripts/config-consistency.py --boot-dir A --varix-dir B --windows-dir C --shared-dir S
  python scripts/config-consistency.py --root <usb-root> --fix    # 冲突时以 SHARED 覆盖三处
  python scripts/config-consistency.py --selftest                 # 内置断言（一致/漂移/修复）

退出码：0=全部一致；1=存在冲突（未 --fix 或 --fix 后仍有缺失）；2=输入缺失。
纳入常规门禁：verify.sh 的灾备段调用（任务89 收口时接线）。
"""
import argparse
import hashlib
import os
import shutil
import sys

# 受管配置清单（相对文件名）：三处副本 + SHARED 基准。
MANAGED = ["boot-select.json", "apps.json", "vfsguard-rules.json"]


def md5_of(path):
    h = hashlib.md5()
    with open(path, "rb") as f:
        h.update(f.read())
    return h.hexdigest()


def audit(places, shared_dir, fix=False):
    """places = {名称: 目录}；SHARED 为基准。返回 (报告行列表, 冲突数, 缺失数)。"""
    lines, conflicts, missing = [], 0, 0
    for name in MANAGED:
        shared_path = os.path.join(shared_dir, name)
        if not os.path.isfile(shared_path):
            lines.append(f"MISSING  shared/{name} —— 单一事实源缺基准，无法收敛（需重建 SHARED）")
            missing += 1
            continue
        base = md5_of(shared_path)
        for place, d in places.items():
            p = os.path.join(d, name)
            if not os.path.isfile(p):
                missing += 1
                lines.append(f"MISSING  {place}/{name}（SHARED md5={base[:12]}）")
                if fix:
                    shutil.copy2(shared_path, p)
                    lines.append(f"  -> FIXED  以 SHARED 回灌 {place}/{name}")
                continue
            m = md5_of(p)
            if m == base:
                lines.append(f"OK       {place}/{name} md5={m[:12]}")
            else:
                conflicts += 1
                lines.append(f"DRIFT    {place}/{name} md5={m[:12]} != SHARED {base[:12]}")
                if fix:
                    shutil.copy2(shared_path, p)
                    lines.append(f"  -> FIXED  以 SHARED 覆盖 {place}/{name}（冲突仲裁：SHARED 为准）")
    return lines, conflicts, missing


def selftest():
    """内置断言：一致全绿 / 漂移检出 / --fix 收敛 / SHARED 缺失如实报。"""
    import tempfile
    ok = True

    def mk(base):
        d = tempfile.mkdtemp(prefix=base)
        return d

    # 场景1：三处与 SHARED 一致 → 0 冲突。
    root = mk("c1-")
    dirs = {k: mk(f"c1-{k}-") for k in ("boot", "varix", "windows", "shared")}
    for name in MANAGED:
        data = (name + ":v1").encode()
        for d in dirs.values():
            with open(os.path.join(d, name), "wb") as f:
                f.write(data)
    _, c, m = audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"])
    ok &= (c == 0 and m == 0)
    print(f"  selftest 一致全绿: {'PASS' if c == 0 and m == 0 else 'FAIL'}")

    # 场景2：windows 处漂移 → 检出 1 冲突；--fix 后再次审计应全绿。
    with open(os.path.join(dirs["windows"], "apps.json"), "w", encoding="utf-8") as f:
        f.write("drifted")
    _, c, _ = audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"])
    drift_seen = c == 1
    audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"], fix=True)
    _, c2, _ = audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"])
    fixed = c2 == 0 and md5_of(os.path.join(dirs["windows"], "apps.json")) == md5_of(
        os.path.join(dirs["shared"], "apps.json"))
    print(f"  selftest 漂移检出: {'PASS' if drift_seen else 'FAIL'}；--fix 以 SHARED 收敛: {'PASS' if fixed else 'FAIL'}")
    ok &= drift_seen and fixed

    # 场景3：varix 缺文件 → MISSING 计数；--fix 回灌后消失。
    os.remove(os.path.join(dirs["varix"], "boot-select.json"))
    _, c3, m3 = audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"])
    miss_seen = m3 >= 1
    audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"], fix=True)
    _, _, m4 = audit({k: dirs[k] for k in ("boot", "varix", "windows")}, dirs["shared"])
    backfilled = m4 == 0
    print(f"  selftest 缺失检出: {'PASS' if miss_seen else 'FAIL'}；--fix 回灌: {'PASS' if backfilled else 'FAIL'}")
    ok &= miss_seen and backfilled

    # 清理。
    for d in list(dirs.values()) + [root]:
        shutil.rmtree(d, ignore_errors=True)
    print("SELFTEST:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description="三处配置一致性校验器（单一事实源=SHARED）")
    ap.add_argument("--root", help="演练根目录（含 boot/ varix/ windows/ shared/ 子目录）")
    ap.add_argument("--boot-dir"); ap.add_argument("--varix-dir")
    ap.add_argument("--windows-dir"); ap.add_argument("--shared-dir")
    ap.add_argument("--fix", action="store_true", help="冲突/缺失时以 SHARED 为准收敛（默认只报告）")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    if args.root:
        root = args.root
        places = {
            "boot": os.path.join(root, "boot"),
            "varix": os.path.join(root, "varix"),
            "windows": os.path.join(root, "windows"),
        }
        shared = os.path.join(root, "shared")
    elif all([args.boot_dir, args.varix_dir, args.windows_dir, args.shared_dir]):
        places = {"boot": args.boot_dir, "varix": args.varix_dir, "windows": args.windows_dir}
        shared = args.shared_dir
    else:
        ap.error("需要 --root 或四个目录参数（或 --selftest）")
        return 2

    for label, d in list(places.items()) + [("shared", shared)]:
        if not os.path.isdir(d):
            print(f"输入缺失：{label} 目录不存在：{d}")
            return 2

    lines, conflicts, missing = audit(places, shared, fix=args.fix)
    print("=== 三处配置一致性校验（单一事实源 = SHARED） ===")
    for l in lines:
        print(" ", l)
    tail = "全部一致" if (conflicts == 0 and missing == 0) else (
        f"已收敛（--fix）" if args.fix and conflicts + missing > 0 else f"冲突 {conflicts} / 缺失 {missing}")
    print(f"结论：{tail}")
    if conflicts == 0 and missing == 0:
        return 0
    return 0 if args.fix else 1


if __name__ == "__main__":
    sys.exit(main())
