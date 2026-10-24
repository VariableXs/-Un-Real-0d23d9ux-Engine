#!/usr/bin/env python3
"""fuc_one.py — 单文件 fuck-u-code 复检器（行为保持整改验收用）。

用法:
  python fuc_one.py <file.rs>              # 输出单文件分数与指标明细
  python fuc_one.py --batch <list.txt>     # 批量: 每行一个文件路径, 输出 TSV 摘要

原理: fuck-u-code analyze 只接受目录, 故把目标文件拷入临时目录单独扫描。
单文件各项指标只取决于文件本身, 与仓库其余部分无关。
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

FUC = r"C:/Users/varia/.workbuddy/binaries/node/workspace/fuckucode/node_modules/eff-u-code/bin/fuck-u-code.js"


def scan_file(path: str) -> dict:
    path = os.path.abspath(path)
    if not os.path.isfile(path):
        return {"err_msg": f"not found: {path}"}
    tmpd = tempfile.mkdtemp(prefix="fuc_one_")
    try:
        shutil.copy2(path, tmpd)
        r = subprocess.run(
            ["node", FUC, "analyze", tmpd, "-f", "json"],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=300,
        )
        out = r.stdout or r.stderr
        idx = out.find("{")
        if idx < 0:
            return {"err_msg": f"no json: {out[:200]}"}
        d = json.loads(out[idx:])
        files = d.get("files") or []
        if not files:
            return {"err_msg": "no files in report"}
        f = files[0]
        m = {x["name"]: x for x in f["metrics"]}
        sev = {x["name"]: x["severity"] for x in f["metrics"]}
        crit = sum(1 for s in sev.values() if s == "critical")
        err = sum(1 for s in sev.values() if s == "error")
        warn = sum(1 for s in sev.values() if s == "warning")
        pr = f["parseResult"]
        return {
            "path": path, "score": f["score"], "critical": crit, "error": err,
            "warning": warn, "lines": pr["totalLines"], "code": pr["codeLines"],
            "comment": pr["commentLines"], "metrics": m,
        }
    finally:
        shutil.rmtree(tmpd, ignore_errors=True)


def brief(r: dict) -> str:
    if "err_msg" in r:
        return f"ERR {r['err_msg']}"
    low = sorted(
        ((v["normalizedScore"], k) for k, v in r["metrics"].items())
    )
    lows = " ".join(f"{k}={s:.0f}" for s, k in low if s < 95)
    return (
        f"score={r['score']:.1f} C={r['critical']} E={r['error']} W={r['warning']} "
        f"lines={r['lines']} | {lows}"
    )


def main() -> None:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return
    if args[0] == "--batch":
        with open(args[1], encoding="utf-8") as fh:
            for line in fh:
                p = line.strip()
                if not p or p.startswith("#"):
                    continue
                r = scan_file(p)
                print(p, "->", brief(r), flush=True)
    else:
        r = scan_file(args[0])
        if "err_msg" in r:
            print("ERR", r["err_msg"])
            return
        print(brief(r))
        for k, v in r["metrics"].items():
            print(f"  {k:28s} val={str(v['value'])[:24]:24s} "
                  f"norm={v['normalizedScore']:6.1f} sev={v['severity']}")


if __name__ == "__main__":
    main()
