# -*- coding: utf-8 -*-
"""U 盘部署全量对账（全只读）：以部署脚本清单为准，逐项 sha256 对比。
盘符 Y:(ESP) X:(U盘Win11) S:(SHARED) 当前已挂载（上轮残留，本脚本只读不动它们）。"""
import hashlib
import sys
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "_attic", "vx-full-audit.rpt")
out = []


def sha256_file(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest().upper()


def mtime(p):
    import datetime
    return datetime.datetime.fromtimestamp(os.path.getmtime(p)).strftime("%Y-%m-%d %H:%M:%S")


def cmp_file(label, local, esp):
    le, ee = os.path.exists(local), os.path.exists(esp)
    if not le or not ee:
        out.append(f"[{label}] local={le} esp={ee} -> {'LOCAL-MISSING' if not le else 'ESP-MISSING'}")
        return
    lh, eh = sha256_file(local), sha256_file(esp)
    ok = lh == eh
    out.append(f"[{label}] {'MATCH' if ok else 'DIFF'}")
    if not ok:
        out.append(f"    local {lh[:16]}.. mtime={mtime(local)} size={os.path.getsize(local)}")
        out.append(f"    esp   {eh[:16]}.. mtime={mtime(esp)} size={os.path.getsize(esp)}")


def main():
    # 1) 内核（部署源 = isoroot，由 02:30 make-iso 刷新）
    cmp_file("kernel/varix", os.path.join(ROOT, "build", "isoroot", "kernel", "varix"),
             "Y:\\kernel\\varix")
    # 2) limine.conf（单一事实源 = repo 根；部署时 WriteAllText UTF8 无 BOM）
    cmp_file("limine.conf", os.path.join(ROOT, "limine.conf"), "Y:\\limine.conf")
    # 3) 壁纸（Limine internal module 卷根路径）
    cmp_file("wallpaper.rgb565",
             os.path.join(ROOT, "_attic", "wallpaper-src", "wallpaper-rgb565.bin"),
             "Y:\\wallpaper.rgb565")
    # 4) BOOTX64.EFI（S0 时代安装、部署脚本不更新——记录现状 vs 本地基准）
    cmp_file("BOOTX64.EFI(信息性)",
             os.path.join(ROOT, "tools", "limine", "limine-binary", "BOOTX64.EFI"),
             "Y:\\EFI\\BOOT\\BOOTX64.EFI")
    # 5) boot-select.json 双副本语义对比
    try:
        repo = json.load(open(os.path.join(ROOT, "boot-select.json"), encoding="utf-8"))
        out.append(f"repo cfg: {json.dumps(repo, ensure_ascii=False)}")
        for tag, p in (("ESP", "Y:\\boot-select.json"), ("SHARED", "S:\\boot-select.json")):
            if os.path.exists(p):
                cur = json.load(open(p, encoding="utf-8"))
                same = cur == repo
                out.append(f"[boot-select.json/{tag}] {'SAME-AS-REPO' if same else 'DIFFERS-FROM-REPO'} "
                           f"mtime={mtime(p)}")
                if not same:
                    out.append(f"    esp cfg: {json.dumps(cur, ensure_ascii=False)}")
            else:
                out.append(f"[boot-select.json/{tag}] MISSING")
    except Exception as ex:
        out.append(f"cfg compare EXC: {ex!r}")
    # 6) U 盘 Win11 里的 Variable.exe vs dist-portable
    cmp_file("X:\\Variable\\Variable.exe",
             os.path.join(ROOT, "dist-portable", "Variable.exe"),
             "X:\\Variable\\Variable.exe")
    # 7) ESP 根清单（部署面完整性目视）
    try:
        names = sorted(os.listdir("Y:\\"))
        out.append(f"ESP root: {names}")
        kdir = sorted(os.listdir("Y:\\kernel")) if os.path.isdir("Y:\\kernel") else []
        out.append(f"ESP kernel dir: {kdir}")
    except Exception as ex:
        out.append(f"listdir EXC: {ex!r}")
    out.append("VX-FULL-AUDIT-DONE")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        try:
            main()
        except Exception as ex:
            out.append("EXC: " + repr(ex))
        with open(OUT, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    else:
        import ctypes
        import time
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(120):
            time.sleep(2)
            if os.path.exists(OUT):
                print(open(OUT, encoding="utf-8").read()[-4000:])
                break
        else:
            print("timeout waiting report")
