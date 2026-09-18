# -*- coding: utf-8 -*-
"""任务56 测试数学修正：显式指派三方 + 成员数期望 + GPU 越限用 400"""
import io

p = r"kernel/varix/src/quota.rs"
s = io.open(p, encoding="utf-8").read()

# ① 探针 step1：三方显式指派（10=Front 默认，11=Engine，12=Wine）
old = ("        // ① 三方全占满：份额 ≈ targets（±80‰），每方 ≥ min − 80‰（保底不丢）。\n"
       "        let quota = CpuQuota::new();\n")
new = ("        // ① 三方全占满：份额 ≈ targets（±80‰），每方 ≥ min − 80‰（保底不丢）。\n"
       "        let mut quota = CpuQuota::new();\n"
       "        quota.assign_tid(11, Party::Engine).unwrap();\n"
       "        quota.assign_tid(12, Party::Wine).unwrap();\n")
assert s.count(old) == 1, ("probe decl", s.count(old))
s = s.replace(old, new, 1)

# ② 份额宿主用例：同款显式指派
old = ("        spawn3(&mut table, [10, 11, 12]);\n"
       "        quota.apply_to_table(&mut table);\n")
new = ("        spawn3(&mut table, [10, 11, 12]);\n"
       "        let mut quota = quota; // 移动为可变绑定供指派\n"
       "        quota.assign_tid(11, Party::Engine).unwrap();\n"
       "        quota.assign_tid(12, Party::Wine).unwrap();\n"
       "        quota.apply_to_table(&mut table);\n")
assert s.count(old) == 1, ("test spawn3", s.count(old))
s = s.replace(old, new, 1)

# ③ 独占用例（成员数=2 的 Front 均分 10）
old = "        // tid 1/2 未指派 → Front（1 成员）→ 20。\n"
new = "        // tid 1/2 未指派 → Front（2 成员）→ 20/2 = 10。\n"
assert s.count(old) == 1, ("split comment", s.count(old))
s = s.replace(old, new, 1)
old = "        assert_eq!(table.thread(1).unwrap().slice_total, 20);"
new = "        assert_eq!(table.thread(1).unwrap().slice_total, 10);"
assert s.count(old) == 1, ("split assert", s.count(old))
s = s.replace(old, new, 1)

# ④ GPU 宿主用例：200 不越限（750+200=950≤1000 且会替换 Wine 旧值）→ 改 400
old = "        // Σ=900，再要 200 → Overcommit。\n"
new = "        // Σ=900，再要 400（750+400=1150>1000）→ Overcommit（拒绝且不落账）。\n"
assert s.count(old) == 1, ("gpu comment", s.count(old))
s = s.replace(old, new, 1)
old = "assert_eq!(gpu.reserve(Party::Wine, 200), Err(GpuError::Overcommit));"
new = "assert_eq!(gpu.reserve(Party::Wine, 400), Err(GpuError::Overcommit));"
assert s.count(old) == 1, ("gpu assert", s.count(old))
s = s.replace(old, new, 1)

# ⑤ 探针 step4：同款 200→400
old = "            let over = gpu.reserve(Party::Wine, 200);"
new = "            let over = gpu.reserve(Party::Wine, 400);"
assert s.count(old) == 1, ("probe gpu", s.count(old))
s = s.replace(old, new, 1)

io.open(p, "w", encoding="utf-8", newline="").write(s)
s2 = io.open(p, encoding="utf-8").read()
assert "quota.assign_tid(11, Party::Engine).unwrap();" in s2
assert s2.count("assign_tid(11, Party::Engine)") == 2
assert s2.count("reserve(Party::Wine, 400)") == 2
assert s2.count("slice_total, 10)") == 1
print("test math fixed ok")
