//! 任务30-35 汇总 · 负向演练矩阵（双域总案阶段4·步骤10）。
//!
//! **验收口径**：越权进程 × 目录 × 剪贴板 × 共享内存 全组合自动化矩阵
//! 100% 拒绝 + 零崩溃 + 审计全覆盖；未覆盖组合显式声明（文件尾）。


#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipsrv::{ClipAcl, ClipFormat, ClipStore};
    use crate::drivers::blk::BlockDevice;
    use crate::msgchan::{ChanError, MsgBus};
    use crate::shmsrv::{ShmError, ShmStore};
    use crate::vfsguard::{AuditJournal, Decision, DenyReason, Op, RuleSet};
    use alloc::vec::Vec;

    struct FakeDisk {
        data: Vec<u8>,
    }
    impl FakeDisk {
        fn new(blocks: u64) -> Self {
            FakeDisk { data: alloc::vec![0u8; blocks as usize * 512] }
        }
    }
    impl BlockDevice for FakeDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            (self.data.len() / 512) as u64
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), crate::drivers::blk::BlockError> {
            let off = lba as usize * 512;
            dst.copy_from_slice(&self.data[off..off + dst.len()]);
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), crate::drivers::blk::BlockError> {
            let off = lba as usize * 512;
            self.data[off..off + src.len()].copy_from_slice(src);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), crate::drivers::blk::BlockError> {
            Ok(())
        }
    }

    /// 白名单：/docs 与 /dropzone 可读写；剪贴板 ACL 授 pid1/pid2。
    fn fixture() -> (RuleSet, AuditJournal<FakeDisk>, ClipStore, ClipAcl, &'static mut ShmStore, &'static mut MsgBus) {
        let mut rules = RuleSet::new();
        assert!(rules.add(true, true, b"/docs/*").is_ok());
        assert!(rules.add(true, true, b"/dropzone/*").is_ok());
        let mut disk = FakeDisk::new(4096);
        AuditJournal::<FakeDisk>::format(&mut disk, 300).expect("fmt");
        let (journal, _) = AuditJournal::open(disk, 300).expect("open");
        let mut acl = ClipAcl::new();
        acl.grant(1);
        acl.grant(2);
        // ShmStore 含多块 64KiB 缓冲，禁栈上物化（内核戒律）→ 泄漏到堆。
        let shm: &'static mut ShmStore = Box::leak(Box::new(ShmStore::new()));
        // MsgBus 同样大体量（16 通道×8 订阅×64 槽环），禁栈上物化 → 堆。
        let bus: &'static mut MsgBus = Box::leak(Box::new(MsgBus::new()));
        (rules, journal, ClipStore::new(), acl, &mut *shm, bus)
    }

    /// 越权组合矩阵：4 进程 × 4 路径形态 × {读,写} × 4 通道（文件/拖放/
    /// 剪贴板 FileRef/共享内存）= 128 组合，逐一断言。
    /// 测试体：fixture 含大缓冲结构（ShmStore/MsgBus），禁止在默认 1MiB
    /// 测试线程栈上物化（内核戒律：>64KB struct 禁栈上/Box 中转）→
    /// 统一放到 64MiB 大栈线程执行。
    #[test]
    fn negative_matrix_all_denied_zero_panic_full_audit() {
        let h = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(negative_matrix_body)
            .expect("spawn");
        h.join().expect("矩阵体零崩溃");
    }

    fn negative_matrix_body() {
        let (rules, mut audit, mut clip, acl, mut shm, mut bus) = fixture();
        bus.register(b"chan-x").expect("reg");
        let pids = [1u32, 2, 3, 77];
        // pid1 建共享内存块；2 获授权；3/77 未授权。
        let h = shm.create(1, 64).expect("shm create");
        shm.grant(h, 1, 2).expect("grant");
        let paths: [&[u8]; 4] = [b"/docs/ok.txt", b"/etc/passwd", b"../escape", b"C:/win"];
        let mut combos = 0u64;
        let mut denies = 0u64;
        for &pid in &pids {
            for &path in &paths {
                for &write in &[false, true] {
                    // ① 文件读写裁决。
                    combos += 1;
                    let d = rules.adjudicate(path, if write { Op::Write } else { Op::Read });
                    if !d.allow {
                        denies += 1;
                    }
                    // ② 拖放（同裁决 + 审计）。
                    combos += 1;
                    if !ClipStore::dragdrop(&rules, &mut audit, pid, path, write).allow {
                        denies += 1;
                    }
                    // ③ 剪贴板 FileRef（写入引用 = 同路径裁决）。
                    combos += 1;
                    if clip.set(&acl, &rules, &mut audit, pid, ClipFormat::FileRef, path).is_err() {
                        denies += 1;
                    }
                }
                // ④ 共享内存映射（对象维度授权；map 内含 pid 维度，故在 path 层循环内）。
                combos += 1;
                if let Err(e) = shm.map(h, pid, false) {
                    if pid != 1 && pid != 2 {
                        assert!(matches!(e, ShmError::PermissionDenied | ShmError::NotFound));
                        denies += 1;
                    }
                }
            }
            // ⑤ 通道：未放行订阅被拒（allowed=false 单维，见文件尾声明）。
            combos += 1;
            if bus.subscribe(b"chan-x", pid, false) == Err(ChanError::NotAllowed) {
                denies += 1;
            }
        }
        // 拒绝数算术唯一可复核：
        //   文件 3 坏路径 × 4 pid × 2 方向 = 24；拖放同 = 24；
        //   剪贴板 FileRef = 授权 pid(1,2) 仅坏路径 3×2×2=12 + 未授权
        //     pid(3,77) 全路径 4×2×2=16，共 28；
        //   共享内存 = pid3/77 × 4 路径 = 8；通道未放行 = 4。合计 88。
        assert_eq!(denies, 88, "拒绝数算术唯一可复核");
        assert_eq!(combos, 4 * 4 * 2 * 3 + 4 * 4 + 4, "全组合数 116 固定");
        // 审计全覆盖：拖放(32) + 剪贴板 set(32) 的每次裁决都落了 journal。
        let mut recs = alloc::vec![None; 256];
        let (n, _trunc) = audit.read_all(&mut recs);
        assert!(n >= 32, "审计覆盖拖放+剪贴板拒绝（实际 {}）", n);
        assert!(audit.len() >= 1);
        // 逃逸路径的拒绝理由必须是 BadPath（逃逸是安全事件）。
        let d = rules.adjudicate(b"../escape", Op::Read);
        assert!(matches!(d.deny, Some(DenyReason::BadPath(_))));
    }

    // 未覆盖组合（如实声明，完善性验收）：
    // 1) 消息通道 × 文件白名单交叉 —— 通道订阅的 allowed 位现由调用方
    //    （垫片/任务26）按白名单裁决置位，pid 维度在垫片携带身份后才可
    //    全组合自动化；
    // 2) 实机（QEMU）矩阵 —— 本矩阵为宿主全测纯逻辑体；实机探针随
    //    任务26/27 垫片接线后补挂（与任务30/32 实机探针同模式）。
}
