# -*- coding: utf-8 -*-
"""AI-4：s207 脚本健壮化——cmd/key 捕获 ConnectionReset（QEMU 外部死亡时优雅收轮）。"""
import io

P = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\qemu-s207-suite-walkthrough.py"

with io.open(P, "r", encoding="utf-8", newline="") as f:
    src = f.read()

old = '''    def cmd(self, line):
        self.sock.sendall((line + "\\n").encode())
        time.sleep(0.4)
        try:
            return self.sock.recv(65536).decode("utf-8", "replace")
        except OSError:
            return ""'''
new = '''    def cmd(self, line):
        try:
            self.sock.sendall((line + "\\n").encode())
        except OSError:
            return ""  # QEMU 已死（外部终止）——调用方经存活检查收轮
        time.sleep(0.4)
        try:
            return self.sock.recv(65536).decode("utf-8", "replace")
        except OSError:
            return ""'''

assert old in src, "cmd anchor missing"
src = src.replace(old, new, 1)

with io.open(P, "w", encoding="utf-8", newline="") as f:
    f.write(src)

with io.open(P, "r", encoding="utf-8") as f:
    back = f.read()
print("verify:", "调用方经存活检查收轮" in back)
