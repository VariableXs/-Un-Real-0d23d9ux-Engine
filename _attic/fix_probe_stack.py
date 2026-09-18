# -*- coding: utf-8 -*-
"""quota.rs 探针栈溢出修复：探针线程表改 .bss static（SpinProtected），恢复全局 current"""
import io

p = r"kernel/varix/src/quota.rs"
s = io.open(p, encoding="utf-8").read()

# A. target 模块去掉 Box 导入（改用 static 后不再需要）
old = "pub mod target {\n    use super::*;\n    use crate::sched::engine::{PRIO_NORMAL, SchedClass};\n    use alloc::boxed::Box;\n"
new = "pub mod target {\n    use super::*;\n    use crate::sched::engine::{PRIO_NORMAL, SchedClass};\n"
assert s.count(old) == 1, ("A", s.count(old))
s = s.replace(old, new, 1)

# B. 探针 statics + 全局 current 保存
old = ("    /// 探针主体。四步：①矩阵饱和份额+保底 ②单方独占压测 ③水位演练\n"
       "    /// ④GPU 通道预留/记账。\n"
       "    pub fn quota_probe() -> bool {\n"
       "        let mut all_ok = true;\n")
new = ("    // 探针专用线程表放 .bss（const 初始化）：ThreadTable ≈115KB，绝不能在引导栈上\n"
       "    // 物化——`Box::new(ThreadTable::new())` 的栈中转同样会撑爆 64KB 引导栈\n"
       "    // （实机静默崩溃坑；宿主测试栈 8MB 不受影响，仍可 Box）。沿 sched::SCHED 同款\n"
       "    // static 模式，A/B 各承载一步避免跨步重置。\n"
       "    static PROBE_TABLE_A: crate::cpu::sync::SpinProtected<ThreadTable> =\n"
       "        crate::cpu::sync::SpinProtected::new(ThreadTable::new());\n"
       "    static PROBE_TABLE_B: crate::cpu::sync::SpinProtected<ThreadTable> =\n"
       "        crate::cpu::sync::SpinProtected::new(ThreadTable::new());\n"
       "\n"
       "    /// 探针主体。四步：①矩阵饱和份额+保底 ②单方独占压测 ③水位演练\n"
       "    /// ④GPU 通道预留/记账。\n"
       "    pub fn quota_probe() -> bool {\n"
       "        let saved_current = crate::sched::engine::current_tid();\n"
       "        let mut all_ok = true;\n")
assert s.count(old) == 1, ("B", s.count(old))
s = s.replace(old, new, 1)

# C. step1 改用 PROBE_TABLE_A
old = "        let step1 = {\n            let mut table = Box::new(ThreadTable::new());\n"
new = "        let step1 = {\n            let mut table = PROBE_TABLE_A.lock();\n"
assert s.count(old) == 1, ("C", s.count(old))
s = s.replace(old, new, 1)

# D. step2 改用 PROBE_TABLE_B
old = "        let step2 = {\n            let mut table = Box::new(ThreadTable::new());\n"
new = "        let step2 = {\n            let mut table = PROBE_TABLE_B.lock();\n"
assert s.count(old) == 1, ("D", s.count(old))
s = s.replace(old, new, 1)

# E. 结束前恢复全局 current
old = "        if all_ok {\n            crate::kinfo!(\"QUOTA PROBE PASS\");\n"
new = ("        // 恢复内核级 current 视图（探针 pick 会改写 engine 全局 current）。\n"
       "        crate::sched::engine::set_current(saved_current);\n"
       "        if all_ok {\n            crate::kinfo!(\"QUOTA PROBE PASS\");\n")
assert s.count(old) == 1, ("E", s.count(old))
s = s.replace(old, new, 1)

io.open(p, "w", encoding="utf-8", newline="").write(s)
s2 = io.open(p, encoding="utf-8").read()
assert "PROBE_TABLE_A.lock()" in s2 and "PROBE_TABLE_B.lock()" in s2
assert "Box::new(ThreadTable::new())" not in s2, "target 模块不应再有 Box 构表"
assert s2.count("Box::new(ThreadTable::new())") == 0
assert "saved_current" in s2 and "use alloc::boxed::Box;\n    \n" not in s2
print("probe static fix ok")
