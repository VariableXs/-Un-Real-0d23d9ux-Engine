# -*- coding: utf-8 -*-
"""nvme.rs 手术：cid 统一分配 / poll_cqe 自由函数 / io_transfer 拆分 / probe 入口。"""
from pathlib import Path

p = Path("kernel/varix/src/drivers/nvme.rs")
s = p.read_text(encoding="utf-8")

# ---- A) admin_submit：cid 由栈统一分配并覆写进命令 ----
old = """    fn admin_submit(&mut self, cmd: Submission) -> Result<Completion, BlockError> {
        let cid = self.cid;
        self.cid = self.cid.wrapping_add(1);
        let slot = self.admin_tail;"""
new = """    fn admin_submit(&mut self, mut cmd: Submission) -> Result<Completion, BlockError> {
        // cid 由栈统一分配并覆写进命令——调用方构造时无需预占，杜绝
        // "调用方取的 cid 与栈内自增错位"的错配。
        let cid = self.next_cid();
        cmd.0[0] = (cmd.0[0] & 0xFFFF) | ((cid as u32) << 16);
        let slot = self.admin_tail;"""
assert old in s, "A"
s = s.replace(old, new)

# ---- B) admin_submit 的 poll 调用改自由函数 ----
old = """        // 3) 轮询 CQE phase 翻转。
        let cqe = self.poll_cqe(
            self.admin_cq_phys,
            &mut self.admin_cq_head,
            &mut self.admin_phase,
            DB_ADMIN_CQ,
            deadline_of(self.now, self.timeout_ns),
        )?;"""
new = """        // 3) 轮询 CQE phase 翻转。
        let deadline = deadline_of(self.now, self.timeout_ns);
        let cqe = poll_cqe(
            &mut self.bar,
            &self.mem,
            self.now,
            self.admin_cq_phys,
            &mut self.admin_cq_head,
            &mut self.admin_phase,
            DB_ADMIN_CQ,
            deadline,
        )?;"""
assert old in s, "B"
s = s.replace(old, new)

# ---- C) identify/create 的 cid 预取清除 ----
old = """        // identify namespace（CNS=1, NSID=1）。
        self.mem.write_bytes(id_page, 0, &zeros);
        let cid = self.cid; // admin_submit 内部自增前取——对齐用。
        self.admin_submit(Submission::admin_identify(cid, 1, self.nsid, id_page))?;"""
new = """        // identify namespace（CNS=1, NSID=1）。
        self.mem.write_bytes(id_page, 0, &zeros);
        self.admin_submit(Submission::admin_identify(0, 1, self.nsid, id_page))?;"""
assert old in s, "C1"
s = s.replace(old, new)

old = """        let cid_cq = self.next_cid();
        self.admin_submit(Submission::admin_create_cq(cid_cq, 1, cq, QUEUE_ENTRIES))?;
        let cid_sq = self.next_cid();
        self.admin_submit(Submission::admin_create_sq(cid_sq, 1, sq, QUEUE_ENTRIES, 1))?;"""
new = """        self.admin_submit(Submission::admin_create_cq(0, 1, cq, QUEUE_ENTRIES))?;
        self.admin_submit(Submission::admin_create_sq(0, 1, sq, QUEUE_ENTRIES, 1))?;"""
assert old in s, "C2"
s = s.replace(old, new)

# ---- D) io_transfer → io_submit_and_wait + io_read_chunk/io_write_chunk ----
old_start = "    /// IO 提交+等待（read/write 共用；write=true 数据进设备）。"
i0 = s.index(old_start)
i1 = s.index("    /// 容量与块大小（验收证据与宿主断言用）。")
new_io = """    /// IO 提交 + 等待完成（read/write/flush 共用尾段）。
    fn io_submit_and_wait(&mut self, cmd: Submission, cid: u16) -> Result<(), BlockError> {
        let slot = self.io_tail;
        let bytes = cmd.le_bytes();
        self.mem.write_bytes(self.io_sq_phys, slot as u64 * 64, &bytes);
        // 门铃写序：entry 全局可见先于 doorbell（Release）。
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.bar.write32(self.io_doorbell(0), ((slot + 1) % QUEUE_ENTRIES) as u32);
        self.io_tail = (slot + 1) % QUEUE_ENTRIES;
        let deadline = deadline_of(self.now, self.timeout_ns);
        let cqe = poll_cqe(
            &mut self.bar,
            &self.mem,
            self.now,
            self.io_cq_phys,
            &mut self.io_cq_head,
            &mut self.io_phase,
            self.io_doorbell(1),
            deadline,
        )?;
        if cqe.cid() != cid || cqe.status() != 0 {
            return Err(BlockError::Io);
        }
        Ok(())
    }

    /// 单片读（≤4KiB，单 PRP）：设备 → DMA 桶 → dst。
    fn io_read_chunk(&mut self, lba: u64, nblocks: u32, dst: &mut [u8]) -> Result<(), BlockError> {
        let cid = self.next_cid();
        let cmd = Submission::io_read(cid, self.nsid, lba, nblocks, self.io_buf);
        self.io_submit_and_wait(cmd, cid)?;
        self.mem.read_bytes(self.io_buf, 0, dst);
        Ok(())
    }

    /// 单片写（≤4KiB，单 PRP）：src → DMA 桶 → 设备。
    fn io_write_chunk(&mut self, lba: u64, nblocks: u32, src: &[u8]) -> Result<(), BlockError> {
        let cid = self.next_cid();
        self.mem.write_bytes(self.io_buf, 0, src);
        let cmd = Submission::io_write(cid, self.nsid, lba, nblocks, self.io_buf);
        self.io_submit_and_wait(cmd, cid)
    }

"""
s = s[:i0] + new_io + s[i1:]

# ---- E) read_blocks 循环调用改 io_read_chunk ----
old = """            self.io_transfer(false, lba + (done / bs) as u64, chunks as u32, &mut dst[done..done + len])?;"""
new = """            self.io_read_chunk(lba + (done / bs) as u64, chunks as u32, &mut dst[done..done + len])?;"""
assert old in s, "E"
s = s.replace(old, new)

# ---- F) write_blocks 循环：消灭 unsafe 切片伪造 ----
old = """            // write 分支只读 data（io_transfer 契约），临时独占借用安全：
            // src 是共享切片，io_transfer 不写回（write=true 时）。
            let chunk: &mut [u8] = unsafe {
                let ptr = src[done..done + len].as_ptr() as *mut u8;
                core::slice::from_raw_parts_mut(ptr, len)
            };
            self.io_transfer(true, lba + (done / bs) as u64, chunks as u32, chunk)?;"""
new = """            self.io_write_chunk(lba + (done / bs) as u64, chunks as u32, &src[done..done + len])?;"""
assert old in s, "F"
s = s.replace(old, new)

# ---- G) flush 尾段改 io_submit_and_wait ----
old = """    fn flush(&mut self) -> Result<(), BlockError> {
        let cid = self.next_cid();
        let cmd = Submission::io_flush(cid, self.nsid);
        let slot = self.io_tail;
        let bytes = cmd.le_bytes();
        self.mem.write_bytes(self.io_sq_phys, slot as u64 * 64, &bytes);
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.bar.write32(self.io_doorbell(0), ((slot + 1) % QUEUE_ENTRIES) as u32);
        self.io_tail = (slot + 1) % QUEUE_ENTRIES;
        let deadline = deadline_of(self.now, self.timeout_ns);
        let cqe = self.poll_cqe(self.io_cq_phys, &mut self.io_cq_head, &mut self.io_phase, self.io_doorbell(1), deadline)?;
        if cqe.cid() != cid || cqe.status() != 0 {
            return Err(BlockError::Io);
        }
        Ok(())
    }"""
new = """    fn flush(&mut self) -> Result<(), BlockError> {
        let cid = self.next_cid();
        self.io_submit_and_wait(Submission::io_flush(cid, self.nsid), cid)
    }"""
assert old in s, "G"
s = s.replace(old, new)

# ---- H) poll_cqe 方法 → 自由函数（放 deadline_of 旁） ----
old = """    /// 轮询一条 CQE：phase 翻转即取，head 推进 + wrap 翻 phase + 写 head 门铃。
    fn poll_cqe(
        &mut self,
        cq_phys: u64,
        head: &mut usize,
        phase: &mut bool,
        cq_doorbell: u16,
        deadline: u64,
    ) -> Result<Completion, BlockError> {
        loop {
            let mut b = [0u8; 16];
            self.mem.read_bytes(cq_phys, *head as u64 * 16, &mut b);
            let c = parse_completion(&b);
            if c.phase() == *phase {
                core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
                *head = (*head + 1) % QUEUE_ENTRIES;
                if *head == 0 {
                    *phase = !*phase; // wrap 翻转 phase 口径。
                }
                self.bar.write32(cq_doorbell, *head as u32);
                return Ok(c);
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
    }

"""
new = ""
assert old in s, "H"
s = s.replace(old, new)

old = """fn deadline_of(now: fn() -> u64, timeout_ns: u64) -> u64 {
    now() + timeout_ns
}"""
new = """fn deadline_of(now: fn() -> u64, timeout_ns: u64) -> u64 {
    now() + timeout_ns
}

/// 轮询一条 CQE：phase 翻转即取，head 推进 + wrap 翻 phase + 写 head 门铃。
/// 自由函数形态：bar 可变 / mem 共享 / head+phase 可变三者是同一 self 的
/// 不同字段——字段级 disjoint borrow 只有显式拆参才立得住。
fn poll_cqe<B: BarAccess, M: DmaMem>(
    bar: &mut B,
    mem: &M,
    now: fn() -> u64,
    cq_phys: u64,
    head: &mut usize,
    phase: &mut bool,
    cq_doorbell: u16,
    deadline: u64,
) -> Result<Completion, BlockError> {
    loop {
        let mut b = [0u8; 16];
        mem.read_bytes(cq_phys, *head as u64 * 16, &mut b);
        let c = parse_completion(&b);
        if c.phase() == *phase {
            // CQE 读全先于 head 推进门铃（Acquire）。
            core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
            *head = (*head + 1) % QUEUE_ENTRIES;
            if *head == 0 {
                *phase = !*phase; // wrap 翻转 phase 口径。
            }
            bar.write32(cq_doorbell, *head as u32);
            return Ok(c);
        }
        if now() > deadline {
            return Err(BlockError::Timeout);
        }
    }
}"""
assert old in s, "H2"
s = s.replace(old, new)

p.write_text(s, encoding="utf-8")
s2 = p.read_text(encoding="utf-8")
assert "io_transfer" not in s2, "io_transfer 残留"
assert s2.count("fn poll_cqe") == 1
assert "unsafe {" not in s2.split("pub mod target")[0].split("mod tests")[0], "主体残留 unsafe"
print("surgery ok")
