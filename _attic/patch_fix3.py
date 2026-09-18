# -*- coding: utf-8 -*-
"""修复：pci unused import / irqdma 括号告警 / poll_cqe 借用顺序。"""
from pathlib import Path

# 1) pci.rs：顶层 use super::blk 移入测试 mod
p = Path("kernel/varix/src/drivers/pci.rs")
s = p.read_text(encoding="utf-8")
s = s.replace("use super::blk;\n", "", 1)
old = "mod tests {\n    use super::*;"
assert old in s
s = s.replace(old, "mod tests {\n    use super::super::blk;\n    use super::*;", 1)
p.write_text(s, encoding="utf-8")

# 2) irqdma.rs：去方法参数括号（drivers 首次编译暴露的既有告警）
p2 = Path("kernel/varix/src/drivers/irqdma.rs")
s2 = p2.read_text(encoding="utf-8")
old = 's.add("X07527 参数与配置面", { t.free(32) && !t.is_used(32) && t.count == 0 && t.alloc(40) }, "默认档=现状，向量可记忆");'
new = 's.add("X07527 参数与配置面", t.free(32) && !t.is_used(32) && t.count == 0 && t.alloc(40), "默认档=现状，向量可记忆");'
assert old in s2
s2 = s2.replace(old, new)
p2.write_text(s2, encoding="utf-8")

# 3) nvme.rs：io_submit_and_wait 先算 doorbell 再借 bar
p3 = Path("kernel/varix/src/drivers/nvme.rs")
s3 = p3.read_text(encoding="utf-8")
old = """        let deadline = deadline_of(self.now, self.timeout_ns);
        let cqe = poll_cqe(
            &mut self.bar,
            &self.mem,
            self.now,
            self.io_cq_phys,
            &mut self.io_cq_head,
            &mut self.io_phase,
            self.io_doorbell(1),
            deadline,
        )?;"""
new = """        let deadline = deadline_of(self.now, self.timeout_ns);
        let db = self.io_doorbell(1); // 先取值再借 bar（参数求值顺序借用隔离）。
        let cqe = poll_cqe(
            &mut self.bar,
            &self.mem,
            self.now,
            self.io_cq_phys,
            &mut self.io_cq_head,
            &mut self.io_phase,
            db,
            deadline,
        )?;"""
assert old in s3, "nvme borrow fix"
s3 = s3.replace(old, new)
p3.write_text(s3, encoding="utf-8")

s3 = p3.read_text(encoding="utf-8")
assert "let db = self.io_doorbell(1);" in s3
print("all three fixed")
