# -*- coding: utf-8 -*-
"""AI-4 精确 stage：共享文件剥离并行会话（AI-3/AI-5）的改动段，
构造「HEAD + 仅 AI-4 段」的 blob 并写入 index（工作区零改动）。"""
import io, subprocess, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"


def git(*args):
    return subprocess.run(["git", "-C", REPO] + list(args), capture_output=True, text=True,
                          encoding="utf-8", errors="replace").stdout


def stage_from_worktree(path, removals, label):
    """工作区内容删掉 removals（精确匹配列表）后写入 index blob。"""
    p = REPO + "\\" + path.replace("/", "\\")
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        work = f.read()
    staged = work
    for old in removals:
        if old not in staged:
            print(f"FAIL {label}: removal block not found:\n{old[:120]}")
            sys.exit(1)
        staged = staged.replace(old, "", 1)
    blob = subprocess.run(["git", "-C", REPO, "hash-object", "-w", "--stdin"],
                          input=staged.encode("utf-8"), capture_output=True).stdout.decode().strip()
    subprocess.run(["git", "-C", REPO, "update-index", "--cacheinfo", f"100644,{blob},{path}"], check=True)
    # 回读验证：index 内容应无 xhci/S2.01 段
    idx = git("cat-file", "-p", f"blob:{blob}") if False else subprocess.run(
        ["git", "-C", REPO, "cat-file", "blob", blob], capture_output=True, text=True,
        encoding="utf-8", errors="replace").stdout
    bad = [kw for kw in ("xhci::target::hid_pump", "drivers::xhci::target::probe_and_selftest",
                         "SHIM PROTOCOL GATE", "S2.01 垫片协议生成物") if kw in idx]
    good = ("winsurf::win_probe" in idx) if "main.rs" in path else True
    print(f"OK {label}: blob={blob[:12]} foreign-free={not bad} mine-present={good}")
    return not bad and good


ok = True

# ---- main.rs：剥 AI-5 xhci 探针（2 行+注释+空行），保留我的 winsurf 挂载 ----
ok &= stage_from_worktree("kernel/varix/src/main.rs", ["""    // --- usb stack probe（S4.1·AI-5：xHCI 最小栈——真机 USB 键鼠，PS/2 增量不替代）----
    varix::drivers::xhci::target::probe_and_selftest();

"""], "main.rs")

# ---- inputsvc.rs：剥 AI-5 xhci 泵（4 行注释+1 调用），保留我的焦点路由全部 ----
ok &= stage_from_worktree("kernel/varix/src/inputsvc.rs", ["""        // S4.1（AI-5）：xHCI HID 增量泵——USB 键鼠事件经同构字节汇入同一
        // 队列（feed_key_byte/feed_mouse_byte 既有公开汇点，菜单/ushell/
        // 桌面全部既有消费者零改动受益）。未初始化/无控制器时零开销返回；
        // PS/2 通道的字节序与语义零改动（增量不替代）。
        crate::drivers::xhci::target::hid_pump(self);
"""], "inputsvc.rs")

# ---- audit.cjs：剥 AI-3 S2.01 段 + a-z0-9_ 修复（他人），保留我的 S2.10 段 ----
acp = "tools/audit.cjs"
p = REPO + "\\" + acp.replace("/", "\\")
with io.open(p, "r", encoding="utf-8", newline="") as f:
    work = f.read()
start = work.find("// ---- 6. S2.01")
if start >= 0:
    tail = work.find("})();\n", start)
    block = work[start:tail + len("})();\n") + 1]  # 连同其后空行
    staged = work.replace(block, "", 1)
    # a-z0-9_ 修复（他人）：还原为 HEAD 的单行旧正则
    new_fix = '    // [a-z0-9_]+：命令名含数字（如 a11y_probe），纯 [a-z_]+ 会在数字处截断产生幽灵名。\n    const mm = t.match(/^(?:[a-z0-9_]+\\s*::\\s*)*([a-z0-9_]+)\\s*,?\\s*$/);'
    old_line = '    const mm = t.match(/^(?:[a-z_]+\\s*::\\s*)*([a-z_]+)\\s*,?\\s*$/);'
    if new_fix in staged:
        staged = staged.replace(new_fix, old_line, 1)
    blob = subprocess.run(["git", "-C", REPO, "hash-object", "-w", "--stdin"],
                          input=staged.encode("utf-8"), capture_output=True).stdout.decode().strip()
    subprocess.run(["git", "-C", REPO, "update-index", "--cacheinfo", f"100644,{blob},{acp}"], check=True)
    back = subprocess.run(["git", "-C", REPO, "cat-file", "blob", blob], capture_output=True, text=True,
                          encoding="utf-8", errors="replace").stdout
    print(f"OK audit.cjs: blob={blob[:12]} s201-free={'SHIM PROTOCOL GATE' not in back} "
          f"s210-present={'SHIM DEGRADE GATE' in back}")
    ok &= ("SHIM DEGRADE GATE" in back)
else:
    print("WARN audit.cjs: S2.01 block not in worktree (maybe already committed)")

sys.exit(0 if ok else 1)
