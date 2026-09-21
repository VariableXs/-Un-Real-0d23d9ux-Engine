# -*- coding: utf-8 -*-
"""收尾：清理取证残留 QEMU 进程与临时大文件，确认工作树干净。"""
import subprocess
from pathlib import Path

# 清理可能的 QEMU 残留
r = subprocess.run(["tasklist"], capture_output=True, text=True)
qemu_lines = [l for l in r.stdout.splitlines() if "qemu-system" in l.lower()]
print("qemu procs:", len(qemu_lines))

ATTIC = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic")
# 删除 197MB 安装包与解包目录（取证完成，不再需要）
for name in ["qemu-stable-setup.exe"]:
    f = ATTIC / name
    if f.exists():
        f.unlink()
        print("removed", name)
pack_dir = ATTIC / "qemu-stable"
if pack_dir.exists():
    import shutil
    shutil.rmtree(pack_dir, ignore_errors=True)
    print("removed dir qemu-stable")

# git 状态确认
r = subprocess.run(["git", "status", "--short", "--",
                    "kernel/varix/src/drivers/", "kernel/varix/src/inputsvc.rs",
                    "kernel/varix/src/main.rs", "docs/acceptance/"],
                   cwd=str(ROOT) if (ROOT := Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main")) else ".",
                   capture_output=True, text=True)
print("git status (my paths):")
print(r.stdout.strip() or "  (clean)")
