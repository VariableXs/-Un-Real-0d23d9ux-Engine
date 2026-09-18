# -*- coding: utf-8 -*-
"""nvme.rs 追加 tests mod + 修恢复语义（一次性 patch 脚本）。"""
from pathlib import Path

p = Path("kernel/varix/src/drivers/nvme.rs")
s = p.read_text(encoding="utf-8")

# ---- 1) 恢复耗尽语义：Timeout → DeviceReset ----
old = """                Err((b, m, BlockError::Timeout)) if resets < 1 => {
                    // 恢复路径：整段重初始化（重试一次），计数留证。
                    resets += 1;
                    bar = b;
                    mem = m;
                }
                Err((_, _, e)) => return Err(e),"""
new = """                Err((b, m, BlockError::Timeout)) if resets < 1 => {
                    // 恢复路径：整段重初始化（重试一次），计数留证。
                    resets += 1;
                    bar = b;
                    mem = m;
                }
                // 重试耗尽仍超时 = 控制器复位后依然不 ready —— 如实
                // 上报 DeviceReset（不是简单 Timeout）。
                Err((_, _, BlockError::Timeout)) => return Err(BlockError::DeviceReset),
                Err((_, _, e)) => return Err(e),"""
assert old in s, "recovery block not found"
s = s.replace(old, new)

# ---- 2) 追加 tests mod ----
TESTS = r'''
// ---------------------------------------------------------------------------
// 宿主寄存器模拟器（tests only）：与真控制器同构的行为模型。
// doorbell 写入即同步应答（真硬件是异步完成，行为学等价：命令入队后
// 经 pump 执行并写 CQE）。enable 故障注入驱动恢复路径用例。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::vec::Vec;

    // 可控时钟：每次调用自增（poll 轮询自然推进到超时）。
    static NOW: AtomicU64 = AtomicU64::new(0);
    fn fake_now() -> u64 {
        NOW.fetch_add(1, Ordering::Relaxed) + 1
    }
    fn reset_clock() {
        NOW.store(0, Ordering::Relaxed);
    }

    /// 宿主"物理内存"：8 帧池。
    struct HostMem {
        data: Vec<u8>,
        next: usize,
    }
    impl HostMem {
        fn new() -> HostMem {
            HostMem {
                data: vec![0u8; 8 * PAGE as usize],
                next: 0,
            }
        }
        fn read(&self, phys: u64, off: u64, out: &mut [u8]) {
            let s = (phys + off) as usize;
            out.copy_from_slice(&self.data[s..s + out.len()]);
        }
        fn write(&mut self, phys: u64, off: u64, data: &[u8]) {
            let s = (phys + off) as usize;
            self.data[s..s + data.len()].copy_from_slice(data);
        }
    }
    impl DmaMem for HostMem {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next >= 8 {
                return None;
            }
            let f = (self.next * PAGE as usize) as u64;
            self.next += 1;
            Some(f)
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            self.write(phys, off, data);
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            self.read(phys, off, out);
        }
    }

    /// 控制器模拟器：寄存器状态 + SQ/CQ 消费语义 + 故障注入。
    struct RegModel {
        r: HashMap<u16, u32>,
        asq: u64,
        acq: u64,
        admin_sq_tail: usize,
        admin_cq_head: usize,
        admin_phase: bool,
        io_sq_phys: u64,
        io_cq_phys: u64,
        io_sq_tail: usize,
        io_cq_head: usize,
        io_phase: bool,
        blocks: Vec<u8>, // 1MiB 虚拟盘
        ns_blocks: u64,
        block_size: u32,
        /// 前 N 次 enable 拒绝置 RDY（超时/恢复路径注入）。
        enable_failures: u32,
        strict_enable: bool,
    }
    impl RegModel {
        fn new() -> RegModel {
            RegModel {
                r: HashMap::new(),
                asq: 0,
                acq: 0,
                admin_sq_tail: 0,
                admin_cq_head: 0,
                admin_phase: true,
                io_sq_phys: 0,
                io_cq_phys: 0,
                io_sq_tail: 0,
                io_cq_head: 0,
                io_phase: true,
                blocks: vec![0u8; 1 << 20],
                ns_blocks: 2048,
                block_size: 512,
                enable_failures: 0,
                strict_enable: true,
            }
        }
        fn write_cqe(
            &self,
            mem: &mut HostMem,
            cq_phys: u64,
            idx: usize,
            cid: u16,
            status: u16,
            phase: bool,
        ) {
            let mut c = [0u8; 16];
            let dw3 = ((cid as u32) << 17) | ((status as u32) << 1) | (phase as u32);
            c[12..16].copy_from_slice(&dw3.to_le_bytes());
            mem.write(cq_phys, (idx * 16) as u64, &c);
        }
        fn db(&self, off: u16) -> usize {
            *self.r.get(&off).unwrap_or(&0) as usize
        }
        /// 消费 admin SQ 至 doorbell tail（identify / create cq/sq）。
        fn admin_pump(&mut self, mem: &mut HostMem, upto: usize) {
            while self.admin_sq_tail != upto {
                let mut s = [0u8; 64];
                mem.read(self.asq, (self.admin_sq_tail * 64) as u64, &mut s);
                let op = s[0];
                let cid = (u32::from_le_bytes(s[0..4].try_into().unwrap()) >> 16) as u16;
                let dw10 = u32::from_le_bytes(s[40..44].try_into().unwrap());
                let prp1 = u64::from_le_bytes(s[8..16].try_into().unwrap());
                match op {
                    0x06 => {
                        let cns = dw10 & 0xFF;
                        let mut page = vec![0u8; PAGE as usize];
                        if cns == 0 {
                            page[0xD4..0xD8].copy_from_slice(&1u32.to_le_bytes()); // NN=1
                        } else {
                            page[0..8].copy_from_slice(&self.ns_blocks.to_le_bytes());
                            page[26] = 0; // FLBAS=0
                            page[0x82] = 9; // LBAF0.LBADS=9 → 512B
                        }
                        mem.write(prp1, 0, &page);
                    }
                    0x05 => self.io_cq_phys = prp1,
                    0x01 => self.io_sq_phys = prp1,
                    _ => {}
                }
                self.write_cqe(mem, self.acq, self.admin_cq_head, cid, 0, self.admin_phase);
                self.admin_cq_head = (self.admin_cq_head + 1) % QUEUE_ENTRIES;
                if self.admin_cq_head == 0 {
                    self.admin_phase = !self.admin_phase;
                }
                self.admin_sq_tail = (self.admin_sq_tail + 1) % QUEUE_ENTRIES;
            }
        }
        /// 消费 io SQ（read / write / flush）。
        fn io_pump(&mut self, mem: &mut HostMem, upto: usize) {
            let bs = self.block_size as usize;
            while self.io_sq_tail != upto {
                let mut s = [0u8; 64];
                mem.read(self.io_sq_phys, (self.io_sq_tail * 64) as u64, &mut s);
                let op = s[0];
                let cid = (u32::from_le_bytes(s[0..4].try_into().unwrap()) >> 16) as u16;
                let prp1 = u64::from_le_bytes(s[8..16].try_into().unwrap());
                let lba = u64::from_le_bytes(s[40..48].try_into().unwrap());
                let nlb = (u32::from_le_bytes(s[48..52].try_into().unwrap()) & 0xFFFF) as usize + 1;
                let span = nlb * bs;
                let dst = (lba as usize) * bs;
                match op {
                    0x02 => {
                        let mut buf = vec![0u8; span];
                        buf.copy_from_slice(&self.blocks[dst..dst + span]);
                        mem.write(prp1, 0, &buf);
                    }
                    0x01 => {
                        let mut buf = vec![0u8; span];
                        mem.read(prp1, 0, &mut buf);
                        self.blocks[dst..dst + span].copy_from_slice(&buf);
                    }
                    _ => {}
                }
                self.write_cqe(mem, self.io_cq_phys, self.io_cq_head, cid, 0, self.io_phase);
                self.io_cq_head = (self.io_cq_head + 1) % QUEUE_ENTRIES;
                if self.io_cq_head == 0 {
                    self.io_phase = !self.io_phase;
                }
                self.io_sq_tail = (self.io_sq_tail + 1) % QUEUE_ENTRIES;
            }
        }
        fn read32(&mut self, off: u16) -> u32 {
            match off {
                REG_CAP => 0x3F,       // MQES=63
                REG_CAP + 4 => 0,      // DSTRD=0
                REG_VS => 0x0001_0400, // NVMe 1.4
                REG_CST => *self.r.get(&REG_CST).unwrap_or(&0),
                _ => *self.r.get(&off).unwrap_or(&0),
            }
        }
        fn write32(&mut self, off: u16, val: u32) {
            match off {
                REG_CC => {
                    self.r.insert(REG_CC, val);
                    if val & 1 != 0 {
                        let ok = if self.strict_enable {
                            ((val >> 16) & 0xF == 6)
                                && ((val >> 20) & 0xF == 4)
                                && self.asq != 0
                                && self.acq != 0
                                && self.enable_failures == 0
                        } else {
                            true
                        };
                        if self.enable_failures > 0 {
                            self.enable_failures -= 1;
                        }
                        self.r.insert(REG_CST, if ok { 1 } else { 0 });
                    } else {
                        self.r.insert(REG_CST, 0);
                    }
                }
                REG_ASQ => self.asq = (self.asq & 0xFFFF_FFFF_0000_0000) | val as u64,
                REG_ASQ + 4 => self.asq = (self.asq & 0xFFFF_FFFF) | ((val as u64) << 32),
                REG_ACQ => self.acq = (self.acq & 0xFFFF_FFFF_0000_0000) | val as u64,
                REG_ACQ + 4 => self.acq = (self.acq & 0xFFFF_FFFF) | ((val as u64) << 32),
                _ => {
                    self.r.insert(off, val);
                }
            }
        }
    }

    /// 共享句柄：NvmeCtrl 持有其一，测试持另一份做 pump。
    #[derive(Clone)]
    struct SharedDev {
        rm: Rc<RefCell<RegModel>>,
        mem: Rc<RefCell<HostMem>>,
    }
    impl SharedDev {
        fn new() -> SharedDev {
            SharedDev {
                rm: Rc::new(RefCell::new(RegModel::new())),
                mem: Rc::new(RefCell::new(HostMem::new())),
            }
        }
        fn rm(&self) -> std::cell::RefMut<'_, RegModel> {
            self.rm.borrow_mut()
        }
    }
    impl BarAccess for SharedDev {
        fn read32(&mut self, off: u16) -> u32 {
            self.rm.borrow_mut().read32(off)
        }
        fn write32(&mut self, off: u16, val: u32) {
            self.rm.borrow_mut().write32(off, val);
            // doorbell 写 = 控制器开始消费（同步应答）。
            match off {
                DB_ADMIN_SQ => {
                    let tail = self.rm.borrow().db(off);
                    let mut rm = self.rm.borrow_mut();
                    let mut mem = self.mem.borrow_mut();
                    rm.admin_pump(&mut mem, tail);
                }
                0x1008 => {
                    // IO SQ 门铃：stride=4, qid=1, kind=0。
                    let tail = self.rm.borrow().db(off);
                    let mut rm = self.rm.borrow_mut();
                    let mut mem = self.mem.borrow_mut();
                    rm.io_pump(&mut mem, tail);
                }
                _ => {}
            }
        }
    }
    impl DmaMem for SharedDev {
        fn alloc_frame(&mut self) -> Option<u64> {
            self.mem.borrow_mut().alloc_frame()
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            self.mem.borrow_mut().write(phys, off, data);
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            self.mem.borrow().read(phys, off, out);
        }
    }

    #[test]
    fn nvme_submission_field_packing() {
        let s = Submission::io_read(0x1234, 1, 0x1_0000_0002, 8, 0xABCD_E000);
        assert_eq!(s.0[0], 0x02 | (0x1234 << 16));
        assert_eq!(s.0[1], 1);
        assert_eq!(s.0[2], 0xABCD_E000);
        assert_eq!(s.0[3], 1);
        assert_eq!(s.0[10], 2, "SLBA low");
        assert_eq!(s.0[11], 1, "SLBA high");
        assert_eq!(s.0[12], 7, "NLB-1");
        let c = Submission::admin_create_cq(9, 1, 0x2000, 64);
        assert_eq!(c.0[0], 0x05 | (9 << 16));
        assert_eq!(c.0[10], (1 << 16) | 63, "qid<<16 | size-1");
        assert_eq!(c.0[11], 0x3, "IEN|PC");
        let q = Submission::admin_create_sq(3, 1, 0x3000, 64, 1);
        assert_eq!(q.0[10], (1 << 24) | (1 << 16) | 63, "cqid<<24|qid<<16|size-1");
        let f = Submission::io_flush(7, 1);
        assert_eq!(f.0[0], 0x08 | (7 << 16));
        assert_eq!(f.0[1], 1);
    }

    #[test]
    fn nvme_completion_parse_fields() {
        let mut b = [0u8; 16];
        let dw3 = (0x42u32 << 17) | (0u32 << 1) | 1;
        b[12..16].copy_from_slice(&dw3.to_le_bytes());
        let c = parse_completion(&b);
        assert_eq!(c.cid(), 0x42);
        assert_eq!(c.status(), 0);
        assert!(c.phase());
        // 非零 status + phase 0。
        let dw3 = (0x10u32 << 17) | (0x0102u32 << 1);
        b[12..16].copy_from_slice(&dw3.to_le_bytes());
        let c = parse_completion(&b);
        assert_eq!(c.status(), 0x0102);
        assert!(!c.phase());
    }

    #[test]
    fn nvme_full_sequence_read_write_roundtrip() {
        reset_clock();
        let mut dev = SharedDev::new();
        let mut ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000)
            .expect("完整初始化必须成功");
        assert_eq!(ctrl.resets, 0, "无故障注入不应触发恢复");
        assert_eq!(ctrl.geometry(), (512, 2048), "identify 解析：512B × 2048 块");
        // 写 2 块（1024B）→ 读回逐字节一致。
        let mut w = [0u8; 1024];
        for (i, b) in w.iter_mut().enumerate() {
            *b = (i * 7 + 3) as u8;
        }
        ctrl.write_blocks(100, &w).expect("write 必须成功");
        let mut r = [0xA5u8; 1024];
        ctrl.read_blocks(100, &mut r).expect("read 必须成功");
        assert_eq!(w, r, "读写回环逐字节一致");
        // 越界与非整块如实拒绝。
        assert_eq!(ctrl.read_blocks(2048, &mut r), Err(BlockError::InvalidRange));
        assert_eq!(ctrl.read_blocks(0, &mut r[..100]), Err(BlockError::InvalidRange));
        // flush 成功。
        ctrl.flush().expect("flush 必须成功");
    }

    #[test]
    fn nvme_timeout_recovery_path_retries_once() {
        reset_clock();
        let mut dev = SharedDev::new();
        // 第一次 enable 拒绝置 RDY（超时）→ 恢复重试成功。
        dev.rm().enable_failures = 1;
        let ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 1_000)
            .expect("恢复路径后必须初始化成功");
        assert_eq!(ctrl.resets, 1, "恢复路径必须留下重试证据");
        assert_eq!(ctrl.geometry(), (512, 2048));
    }

    #[test]
    fn nvme_timeout_exhausted_reports_device_reset() {
        reset_clock();
        let mut dev = SharedDev::new();
        dev.rm().enable_failures = 99; // 两次 enable 都失败。
        let e = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 1_000)
            .expect_err("持续失败必须报错");
        assert_eq!(e, BlockError::DeviceReset, "重试耗尽=DeviceReset 口径");
    }

    #[test]
    fn nvme_enable_param_strict_validation() {
        reset_clock();
        let mut dev = SharedDev::new();
        dev.rm().strict_enable = false; // 宽松模式（校验由注入替代）。
        let ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000).unwrap();
        let cc = dev.rm().r.get(&REG_CC).copied().unwrap_or(0);
        assert_eq!(cc & 1, 1, "EN 置位");
        assert_eq!((cc >> 16) & 0xF, 6, "IOSQES=6（64B SQE）");
        assert_eq!((cc >> 20) & 0xF, 4, "IOCQES=4（16B CQE）");
        assert_eq!(ctrl.geometry(), (512, 2048));
    }

    #[test]
    fn nvme_io_cqe_phase_wraps_at_queue_end() {
        reset_clock();
        let mut dev = SharedDev::new();
        let mut ctrl = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 100_000).unwrap();
        // io 队列 64 条：65 次单块读驱动 CQE head wrap（第 65 条翻转
        // phase 口径），wrap 后读写仍正确——队列滚动语义完整验证。
        let mut buf = [0u8; 512];
        for i in 0..65u64 {
            ctrl.read_blocks(i, &mut buf).expect("单块读必须成功");
        }
        let mut w = [0xEEu8; 512];
        ctrl.write_blocks(999, &w).unwrap();
        let mut r = [0u8; 512];
        ctrl.read_blocks(999, &mut r).unwrap();
        assert_eq!(w, r);
    }
}
'''

s = s.rstrip() + "\n" + TESTS
p.write_text(s, encoding="utf-8")
s2 = p.read_text(encoding="utf-8")
assert s2.count("#[test]") >= 6, s2.count("#[test]")
assert "DeviceReset" in s2

# ---- 3) wire 进 drivers/mod.rs ----
m = Path("kernel/varix/src/drivers/mod.rs")
ms = m.read_text(encoding="utf-8")
old_mod = """pub mod acpi;
pub mod bt;
pub mod driver;"""
new_mod = """pub mod acpi;
pub mod blk;
pub mod bt;
pub mod driver;
pub mod nvme;
pub mod pci;"""
if "pub mod blk;" not in ms:
    assert old_mod in ms
    ms = ms.replace(old_mod, new_mod)
    m.write_text(ms, encoding="utf-8")
ms2 = m.read_text(encoding="utf-8")
assert "pub mod blk;" in ms2 and "pub mod nvme;" in ms2 and "pub mod pci;" in ms2
print("patched: nvme tests=%d, mod wired" % s2.count("#[test]"))
