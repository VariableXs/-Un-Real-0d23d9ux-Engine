# -*- coding: utf-8 -*-
"""vx-vcruntime：把 VC++ 2015-2022 运行库六件套（x64）从内置盘补到 U 盘 Windows。
Variable.exe 是 x64，缺 VCRUNTIME140.dll。拷贝 + SHA256 复核。"""
import os, shutil, hashlib, datetime, sys, traceback

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-vcruntime.rpt"
BS = chr(92)
SRC = "C:" + BS + "Windows" + BS + "System32"
DST = "X:" + BS + "Windows" + BS + "System32"
FILES = ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll",
         "msvcp140_1.dll", "msvcp140_2.dll", "concrt140.dll"]
_fh = open(OUT, "w", encoding="utf-8")

def _excepthook(t, v, tb):
    try:
        log("FATAL " + "".join(traceback.format_exception(t, v, tb)))
    except Exception:
        pass
sys.excepthook = _excepthook

def log(s):
    _fh.write(str(s) + "\n")
    _fh.flush()

def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for blk in iter(lambda: f.read(1 << 20), b""):
            h.update(blk)
    return h.hexdigest()

log(f"=== vx-vcruntime start {datetime.datetime.now()} ===")
allok = True
for f in FILES:
    s = os.path.join(SRC, f)
    d = os.path.join(DST, f)
    try:
        if not os.path.isfile(s):
            log(f"  SKIP {f}: 内置盘缺失")
            allok = False
            continue
        need = True
        if os.path.isfile(d) and os.path.getsize(d) == os.path.getsize(s) and sha(d) == sha(s):
            log(f"  OK(existing) {f}: 已在位且一致")
            need = False
        if need:
            shutil.copyfile(s, d)
            same = sha(d) == sha(s)
            log(f"  {'COPIED' if same else 'HASH-MISMATCH!'} {f}: {os.path.getsize(d)}B")
            if not same:
                allok = False
    except Exception:
        log(f"  EXC {f}: " + traceback.format_exc())
        allok = False

log(f"=== 结果: {'ALL GREEN' if allok else 'HAS FAILURES'} ===")
_fh.close()
print("done")
