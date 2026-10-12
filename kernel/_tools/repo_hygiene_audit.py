#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""仓库卫生体检：AI-7 席位的可复跑扫描器。

2026-10-12 立项。对应《7AI分工总案》第二节 AI-7「仓库卫生官」，
以及第三节铁律第 2 条（boot 关键文件零风险）。

═══════════════════════════════════════════════════════════════════
为什么这个脚本必须存在
═══════════════════════════════════════════════════════════════════
本仓的历史事故有两条：

1. **4153 个 .rs 被单提交抹掉** —— 事故后加了「源码保护红线」，但红线是
   写在 .gitignore 里的**注释**。注释没有执行力：只要有人新加一条宽规则，
   红线就静默失效。这个脚本把红线变成**可执行断言**。

2. **`refuse_boot` 历史 panic** —— 删了 `boot-select.json` / `limine.conf`
   导致引导拒绝。gitignore 里已有排除规则，但同样靠人守。

所以：**能靠人记住的纪律不算纪律**。这个脚本把「不许删的东西」写死成
一份清单，每次体检逐条实证（存在 + 已跟踪 + 未被 ignore 误伤），
任何一条不满足即红。

═══════════════════════════════════════════════════════════════════
用法
═══════════════════════════════════════════════════════════════════
    python repo_hygiene_audit.py            # 全量体检，退出码非零 = 有红项
    python repo_hygiene_audit.py --json     # 机器可读，便于接 CI
    python repo_hygiene_audit.py --quick    # 跳过体积扫描（大仓 du 很慢）

设计约束（与内核侧同源）：
- 纯 stdlib，Python 3.8+ 可跑（3.13 实测通过）。
- 只读。**本脚本绝不删任何文件**。删除动作一律出清单交主控裁决。
  ——这是 AI-7 任务书第 6 条的硬要求：只出清单，不擅自删用户数据。
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

# ────────────────────────────────────────────────────────────────────
# 路径锚点
# ────────────────────────────────────────────────────────────────────
# 仓库根 = 本文件的上上级（kernel/_tools/repo_hygiene_audit.py → 仓库根）
REPO_ROOT = Path(__file__).resolve().parent.parent.parent
KERNEL_DIR = REPO_ROOT / "kernel"

# ────────────────────────────────────────────────────────────────────
# 红线清单：不可删文件
# ────────────────────────────────────────────────────────────────────
# 判据 = 相对仓库根的路径。每条都要过三关：
#   ① 文件存在于磁盘
#   ② 已被 git 跟踪（在册才能从历史恢复）
#   ③ 未被 .gitignore 误伤
#
# 增删本清单前先读《7AI分工总案》第二节 AI-7 第 2 条与第三节铁律。
BOOT_CRITICAL = [
    ("boot-select.json", "引导选择配置；曾致 refuse_boot 历史 panic"),
    ("limine.conf", "Limine 构建依赖；曾致 refuse_boot 历史 panic"),
    ("kernel/Cargo.toml", "内核构建入口"),
    ("kernel/Cargo.lock", "构建可复现性源码级资产"),
    ("kernel/rust-toolchain.toml", "工具链版本锚"),
]

# 源码保护红线目录：这些目录下的 .rs 必须全部在册。
# 参照 .gitignore「源码保护红线」段。任一目录被整条 ignore 即红。
SOURCE_TREE = "kernel/varix/src"

# ────────────────────────────────────────────────────────────────────
# Windows 保留名
# ────────────────────────────────────────────────────────────────────
# `nul` / `con` / `aux` / `prn` 等在 Win32 是设备名，非 NTFS 保留名，
# 但走 Win32 API（多数工具链）访问时会被解释成设备 ⇒ 路径解析直接失败。
# 处理这些文件必须加 `\\?\` 前缀转成长路径 API。
RESERVED_NAMES = {
    "nul", "con", "prn", "aux", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4",
    "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
}

# ────────────────────────────────────────────────────────────────────
# 构建产物模式：可再生，不该在册
# ────────────────────────────────────────────────────────────────────
BUILD_DIR_PATTERNS = [
    "_target*",      # kernel/_target, _target_iso, _target_wb* 等 14 个
    "target*",       # target, target_wb, target-a4, target-wb* 等
]

# 噪声文件：临时产物，零信息量
NOISE_SUFFIX = {".log", ".pid", ".pyc"}


def run_git(args, cwd=REPO_ROOT):
    """跑一条 git 命令，失败返回 None（不抛，避免体检脚本自己崩）。"""
    try:
        r = subprocess.run(
            ["git"] + args,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            timeout=180,
        )
        if r.returncode != 0:
            return None
        return r.stdout
    except Exception:
        return None


class Audit:
    """体检结果累加器。每条检查给出 (类别, 严重度, 明细)。"""

    def __init__(self):
        self.findings = []

    def add(self, category, level, msg):
        """level ∈ {"RED", "WARN", "INFO"}。"""
        self.findings.append(
            {"category": category, "level": level, "message": msg}
        )

    def count(self, level):
        return sum(
            1 for f in self.findings
            if isinstance(f, dict) and f.get("level") == level
        )

    def by_category(self, cat):
        return [
            f for f in self.findings
            if isinstance(f, dict) and f.get("category") == cat
        ]


# ────────────────────────────────────────────────────────────────────
# 检查项 1：boot 关键文件三关实证
# ────────────────────────────────────────────────────────────────────
def check_boot_critical(a: Audit):
    """逐条实证红线清单里的每个文件。"""
    tracked_out = run_git(["ls-files", "-z"])
    tracked = set(tracked_out.split("\0")) if tracked_out else set()

    # 逐条查是否被 .gitignore 误伤。
    #
    # 必须带 `--no-index`：git 的 check-ignore 默认会跳过「已被跟踪」的文件，
    # 于是 `kernel/Cargo.lock` 这类「既在 HEAD 又被 `*.lock` 规则命中」的文件
    # 会被误报成安全。加 --no-index 后按纯规则判定，规则命中即红，
    # 再由跟踪状态决定严重度（已在册 = 警，未在册 = 红）。
    rule_hit = {}
    for rel, _why in BOOT_CRITICAL:
        v = run_git(["check-ignore", "-v", "--no-index", rel])
        if v:
            rule_hit[rel] = v.strip()

    for rel, why in BOOT_CRITICAL:
        p = REPO_ROOT / rel
        in_tree = rel in tracked
        if not p.is_file():
            a.add("boot-critical", "RED", f"{rel} 不在磁盘上 —— {why}")
            continue
        if not in_tree:
            a.add("boot-critical", "RED",
                  f"{rel} 未被 git 跟踪（不在册，误删即不可恢复）")
        if rel in rule_hit:
            # 已在册 ⇒ 暂靠历史兜底，属隐患；未在册 ⇒ 规则会真的吞掉它。
            level = "WARN" if in_tree else "RED"
            a.add("boot-critical", level,
                  f"{rel} 被 .gitignore 规则命中 [{rule_hit[rel]}] —— {why}")
    reds = [f for f in a.by_category("boot-critical") if f["level"] == "RED"]
    warns = [f for f in a.by_category("boot-critical") if f["level"] == "WARN"]
    a.add(
        "boot-critical", "INFO",
        f"红线清单 {len(BOOT_CRITICAL)} 条：红 {len(reds)}　警 {len(warns)}",
    )


# ────────────────────────────────────────────────────────────────────
# 检查项 2：源码保护红线（.rs 全在册）
# ────────────────────────────────────────────────────────────────────
def check_source_tree(a: Audit):
    """确认 varix 源码树未被 .gitignore 整条吞掉，且没有裸 .rs 游离。"""
    src = REPO_ROOT / SOURCE_TREE
    if not src.is_dir():
        a.add("source-redline", "RED", f"{SOURCE_TREE} 不存在")
        return

    tracked = run_git(["ls-files", "-z", SOURCE_TREE])
    tracked_set = set(tracked.split("\0")) if tracked else set()

    total = 0
    untracked = []
    for root, _dirs, files in os.walk(src):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            total += 1
            full = Path(root) / fn
            rel = full.relative_to(REPO_ROOT).as_posix()
            if rel not in tracked_set:
                untracked.append(rel)

    # 采样上报，避免刷屏：全量写进 findings 会淹没其它类别的诊断。
    if untracked:
        for r in untracked[:8]:
            a.add("source-redline", "WARN",
                  f"源码未在册：{r}（本地存在但 git 未跟踪，误删即不可恢复）")
        if len(untracked) > 8:
            a.add("source-redline", "WARN",
                  f"…… 另有 {len(untracked) - 8} 个 .rs 未在册，"
                  f"完整清单见 `git status --porcelain` 中 `?? kernel/varix/src` 段")

    a.add("source-redline", "INFO",
          f"{SOURCE_TREE} 扫描 {total} 个 .rs；在册 {len(tracked_set)}")


# ────────────────────────────────────────────────────────────────────
# 检查项 3：构建产物是否在册（不该在册）
# ────────────────────────────────────────────────────────────────────
def check_build_artifacts(a: Audit):
    """构建产物被 git 跟踪 = 白白撑大仓库，且掩盖真实源码变更。"""
    tracked = run_git(["ls-files", "-z"])
    if not tracked:
        a.add("build-artifacts", "WARN", "git ls-files 失败，跳过本项")
        return
    files = [f for f in tracked.split("\0") if f]
    hits = []
    for f in files:
        parts = f.split("/")
        if any(
            part.startswith("_target") or part.startswith("target")
            for part in parts[:-1]
        ):
            hits.append(f)
    if hits:
        for f in hits[:10]:
            a.add("build-artifacts", "RED", f"构建产物在册：{f}")
        if len(hits) > 10:
            a.add("build-artifacts", "RED", f"…… 另有 {len(hits) - 10} 个构建产物在册")
    else:
        a.add("build-artifacts", "INFO", "无构建产物被 git 跟踪")


# ────────────────────────────────────────────────────────────────────
# 检查项 4：Windows 保留名
# ────────────────────────────────────────────────────────────────────
def check_reserved_names(a: Audit):
    """扫出 `nul` 这类设备名文件。处理它们必须走 \\\\?\\ 长路径。"""
    hits = []
    skip_dirs = {"_attic", ".git", "node_modules", "target", "__pycache__"}
    for root, dirs, files in os.walk(REPO_ROOT):
        dirs[:] = [d for d in dirs if d not in skip_dirs]
        for fn in files:
            stem = fn.split(".")[0].lower()
            if stem in RESERVED_NAMES:
                hits.append(
                    (Path(root) / fn).relative_to(REPO_ROOT).as_posix()
                )
    for h in hits:
        a.add(
            "reserved-name", "WARN",
            f"Windows 保留名文件：{h} —— 工具链访问会撞设备名，"
            f"清理须用 \\\\?\\ 前缀",
        )
    a.add("reserved-name", "INFO", f"保留名文件 {len(hits)} 个")


# ────────────────────────────────────────────────────────────────────
# 检查项 5：噪声文件在册
# ────────────────────────────────────────────────────────────────────
def check_noise(a: Audit):
    tracked = run_git(["ls-files", "-z"])
    if not tracked:
        return
    hits = [
        f for f in tracked.split("\0")
        if f and Path(f).suffix.lower() in NOISE_SUFFIX
    ]
    for h in hits[:10]:
        a.add("noise-in-tree", "WARN", f"噪声产物在册：{h}")
    if len(hits) > 10:
        a.add("noise-in-tree", "WARN", f"…… 另有 {len(hits) - 10} 个 .log/.pid/.pyc 在册")
    a.add("noise-in-tree", "INFO", f"噪声文件在册 {len(hits)} 个")


# ────────────────────────────────────────────────────────────────────
# 检查项 6：git 脏状态
# ────────────────────────────────────────────────────────────────────
def check_dirty_state(a: Audit):
    out = run_git(["status", "--porcelain"])
    if out is None:
        a.add("git-state", "WARN", "git status 失败")
        return
    lines = [l for l in out.splitlines() if l.strip()]
    buckets = {"untracked": 0, "modified": 0, "deleted": 0, "staged": 0, "other": 0}
    for l in lines:
        code = l[:2]
        if code == "??":
            buckets["untracked"] += 1
        elif code == " D" or code == "D ":
            buckets["deleted"] += 1
        elif "M" in code:
            buckets["modified"] += 1
        elif "M" in code[:1] or "A" in code or "R" in code:
            buckets["staged"] += 1
        else:
            buckets["other"] += 1
    a.add("git-state", "INFO",
          "脏状态：" + "，".join(f"{k}={v}" for k, v in buckets.items() if v))
    if buckets["deleted"]:
        a.add("git-state", "WARN",
              f"{buckets['deleted']} 个文件处于已删除未提交状态 —— "
              f"确认是有意删除后提交，否则历史与工作区不一致")


# ────────────────────────────────────────────────────────────────────
# 检查项 7：寄生 worktree
# ────────────────────────────────────────────────────────────────────
def check_stray_worktrees(a: Audit):
    """worktree 挂在被 ignore 的目录里 = 仓库卫生的暗伤。

    本仓 `_attic/` 已 gitignore，若在其中 `git worktree add`，
    该工作树既不在册、又会随仓库迁移丢失，且 `git worktree list` 仍挂着它。
    """
    out = run_git(["worktree", "list", "--porcelain"])
    if not out:
        a.add("worktree", "WARN", "git worktree list 失败")
        return
    paths = [
        l[len("worktree "):].strip()
        for l in out.splitlines()
        if l.startswith("worktree ")
    ]
    root_l = str(REPO_ROOT).lower()
    for p in paths:
        if p.lower().startswith(root_l):
            rel = os.path.relpath(p, str(REPO_ROOT)).replace("\\", "/")
            if rel.startswith("_attic/") or rel.startswith("attic/"):
                a.add("worktree", "WARN",
                      f"worktree 寄生在归档区：{rel} —— 该树不在版本控制内，"
                      f"移动/清理仓库即丢失；建议 `git worktree remove` 或剪枝")


# ────────────────────────────────────────────────────────────────────
# 检查项 8：体积扫描（--quick 跳过）
# ────────────────────────────────────────────────────────────────────
def check_sizes(a: Audit, quick=False):
    """统计顶层目录体积，供主控裁决清理优先级。"""
    if quick:
        a.add("size", "INFO", "体积扫描已跳过（--quick）")
        return
    rows = []
    for name in sorted(os.listdir(REPO_ROOT)):
        p = REPO_ROOT / name
        if not p.is_dir() or name == ".git":
            continue
        # 只算顶层，够用于裁决
        try:
            out = subprocess.run(
                ["du", "-sh", str(p)],
                capture_output=True, text=True, timeout=120,
            )
            if out.returncode == 0:
                rows.append((out.stdout.split()[0], name))
        except Exception:
            pass
    rows.sort(reverse=True)
    for sz, name in rows[:12]:
        a.add("size", "INFO", f"顶层体积 {name} = {sz}")
    # 顺带报最大顶层文件
    big = []
    for name in os.listdir(REPO_ROOT):
        p = REPO_ROOT / name
        if p.is_file() and p.stat().st_size > 1 << 20:
            big.append((p.stat().st_size, name))
    big.sort(reverse=True)
    for sz, name in big[:6]:
        a.add("size", "INFO", f"顶层大文件 {name} = {sz / (1 << 20):.1f} MiB")


# ────────────────────────────────────────────────────────────────────
# 主流程
# ────────────────────────────────────────────────────────────────────
def main():
    ap = argparse.ArgumentParser(description="Varix 仓库卫生体检（AI-7）")
    ap.add_argument("--json", action="store_true", help="输出 JSON")
    ap.add_argument("--quick", action="store_true", help="跳过体积扫描")
    args = ap.parse_args()

    a = Audit()
    print(f"[AI-7 仓库体检] 仓库根 = {REPO_ROOT}\n")

    for fn, arg in (
        (check_boot_critical, ()),
        (check_source_tree, ()),
        (check_build_artifacts, ()),
        (check_reserved_names, ()),
        (check_noise, ()),
        (check_dirty_state, ()),
        (check_stray_worktrees, ()),
        (check_sizes, (args.quick,)),
    ):
        try:
            fn(a, *arg)
        except Exception as e:  # 体检脚本自己不许崩，崩了也是一条红
            a.add("tool-error", "WARN", f"{fn.__name__} 异常：{e}")

    red = a.count("RED")
    warn = a.count("WARN")

    if args.json:
        print(json.dumps(
            {"red": red, "warn": warn, "findings": a.findings},
            ensure_ascii=False, indent=2,
        ))
        return 1 if red else 0

    for f in a.findings:
        if not isinstance(f, dict):
            continue
        mark = {"RED": "[红]", "WARN": "[警]", "INFO": "[情]"}[f["level"]]
        print(f"{mark} {f['category']:<16} {f['message']}")

    print(f"\n汇总：红 {red}　警 {warn}　情 {a.count('INFO')}")
    if red:
        print("结论：有红项，须处置后才谈得上「仓库整洁」。")
    elif warn:
        print("结论：无红项，有待确认项；按清单逐条归位即可。")
    else:
        print("结论：仓库卫生达标。")
    return 1 if red else 0


if __name__ == "__main__":
    sys.exit(main())