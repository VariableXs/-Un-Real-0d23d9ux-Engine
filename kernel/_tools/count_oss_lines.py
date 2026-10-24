#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-4 · 207 项目真实行数普查（os.scandir 迭代，避开 find 的进程开销）。

口径：只数源码后缀，分 C系 / Rust / Python 三桶；同时给出每仓文件数与字节数。
输出：TSV（可直接喂给判定表生成器）+ 终端汇总。
"""
import os
import sys
import json

ROOT = r"X:\OpenSource"
EXT = {
    "c":   (".c", ".h"),
    "cpp": (".cpp", ".hpp", ".cc", ".cxx", ".c++", ".hh", ".hxx", ".ipp", ".inl", ".m", ".mm"),
    "rs":  (".rs",),
    "py":  (".py",),
}
SKIP_DIRS = {".git", "target", "build", "node_modules", ".mypy_cache",
             "__pycache__", ".venv", "venv", "out", "dist", ".cache"}


def bucket(name: str):
    for k, exts in EXT.items():
        if name.endswith(exts):
            return k
    return None


def scan_repo(repo: str):
    """单仓遍历。返回 {c,cpp,rs,py} 行数、文件数、字节数。"""
    out = {"lines": 0, "files": 0, "bytes": 0}
    per_ext = {"c": 0, "cpp": 0, "rs": 0, "py": 0}
    nfiles = {"c": 0, "cpp": 0, "rs": 0, "py": 0}
    stack = [repo]
    while stack:
        d = stack.pop()
        try:
            with os.scandir(d) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            if e.name not in SKIP_DIRS:
                                stack.append(e.path)
                            continue
                        b = bucket(e.name)
                        if b is None:
                            continue
                        # 跳过明显是 vendored 二进制/巨型生成物
                        try:
                            sz = e.stat(follow_symlinks=False).st_size
                        except OSError:
                            continue
                        if sz > 4 * 1024 * 1024:
                            continue
                        with open(e.path, "rb") as fh:
                            data = fh.read()
                        out["lines"] += data.count(b"\n") + (1 if data and not data.endswith(b"\n") else 0)
                        out["files"] += 1
                        out["bytes"] += sz
                        per_ext[b] += data.count(b"\n") + (1 if data and not data.endswith(b"\n") else 0)
                        nfiles[b] += 1
                    except OSError:
                        continue
        except OSError:
            continue
    return out, per_ext, nfiles


def main():
    repos = sorted(d for d in os.listdir(ROOT)
                   if os.path.isdir(os.path.join(ROOT, d)) and not d.startswith("_"))
    rows = []
    for i, r in enumerate(repos, 1):
        tot, per, nf = scan_repo(os.path.join(ROOT, r))
        c_like = per["c"] + per["cpp"]
        rows.append({
            "repo": r, "lines": tot["lines"], "files": tot["files"],
            "c": per["c"], "cpp": per["cpp"], "rs": per["rs"], "py": per["py"],
            "nf_c": nf["c"], "nf_cpp": nf["cpp"], "nf_rs": nf["rs"], "nf_py": nf["py"],
            "bytes": tot["bytes"],
        })
        if i % 20 == 0:
            print(f"  ... {i}/{len(repos)}", file=sys.stderr, flush=True)

    rows.sort(key=lambda x: -x["lines"])
    out_tsv = os.path.join(ROOT, "_a4_real_lines.tsv")
    with open(out_tsv, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("repo\tlines\tfiles\tc_lines\tcpp_lines\trs_lines\tpy_lines\tc_files\tcpp_files\trs_files\tpy_files\tbytes\n")
        for r in rows:
            fh.write(f"{r['repo']}\t{r['lines']}\t{r['files']}\t{r['c']}\t{r['cpp']}\t"
                     f"{r['rs']}\t{r['py']}\t{r['nf_c']}\t{r['nf_cpp']}\t{r['nf_rs']}\t{r['nf_py']}\t{r['bytes']}\n")

    gl = sum(r["lines"] for r in rows)
    gc = sum(r["c"] + r["cpp"] for r in rows)
    grs = sum(r["rs"] for r in rows)
    gpy = sum(r["py"] for r in rows)
    print(f"仓库数        : {len(rows)}")
    print(f"源码总行数    : {gl:,}")
    print(f"  C / C++     : {gc:,}")
    print(f"  Rust        : {grs:,}")
    print(f"  Python      : {gpy:,}")
    print(f"总文件数      : {sum(r['files'] for r in rows):,}")
    print(f"总字节        : {sum(r['bytes'] for r in rows):,}")
    print("\n=== Top 20 ===")
    for r in rows[:20]:
        print(f"{r['lines']:>12,}  {r['files']:>7,}f  {r['repo']}")
    print(f"\nTSV -> {out_tsv}")


if __name__ == "__main__":
    main()
