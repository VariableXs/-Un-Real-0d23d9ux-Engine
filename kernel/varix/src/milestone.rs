//! 任务21 · 里程碑整合——「内核可运行用户态程序读写真盘」整链演示
//! （双域总案步骤14 内核侧证据链；前置任务 15/17/18）。
//!
//! 串联叙事（外部驱动脚本 `_attic/milestone-demo.py` 按标志序核对）：
//!
//! ```text
//! 会话1（空盘启动）：
//!   M2  milestone: M2 journal fresh — format base_lba=40000
//!       milestone: M2 journal sealed entries=3 (tier=3 flush-persisted)
//!       milestone: ready-for-powercut
//!   M3  milestone: M3 exfat-read ok /apps.json len=…
//!   M4  milestone: M4 snapshot written slot=…
//!   M1  ring3: task15 lifecycle complete      ← 启动链既有终点（未改）
//!   —— 脚本 kill QEMU = 模拟断电（NVMe 盘面跨进程持久）——
//! 会话2（同一盘面重启）：
//!   M2  milestone: M2 journal open entries=3 torn=0 (recovery path)
//!       milestone: journal-recovered verdict=ok resumed_seq=4
//!   M3  milestone: M3 exfat-read ok …
//!   M4  milestone: snapshot-persisted ok（会话1 marker 仍在盘面）
//!   M1  ring3: task15 lifecycle complete
//! ```
//!
//! M1（用户态程序）零新增代码：ring3 演示是启动链既有终点
//! （`proc::ring3::run_demo`，任务15 已验收）；里程碑探针只在它之前
//! 把「读写真盘」三路打点——journal WAL（M2）、exFAT 读（M3）、
//! 快照区写（M4）。探针挂在 nvme 探针内部（`drivers::nvme::target`），
//! 盘句柄即既有 ctrl#1（init 盘）/ ctrl#2（SHARED 盘），与任务 17/18
//! 探针共用同一实机链路，不另开设备路径。

pub mod target {
    use crate::drivers::blk::BlockDevice;
    use crate::fs::exfat_ro::{ExfatVolume, SnapshotArea};
    use crate::fs::fs23_disk::DiskJournal;
    use crate::fs::fs23_journal::{LogOp, JOURNAL_DEFAULT};

    /// 盘1（init 512MiB）高区 journal 区：loopback 探针最高 ≈16007、
    /// 任务17 探针区 20000..20193 之后，互不交集。
    pub const MS_BASE_LBA: u64 = 40_000;
    /// SHARED 卷快照区契约位（与 shared_probe 同区，追加式互不冲突）。
    pub const MS_SNAP_BASE: u64 = 3000;
    /// 快照区里程碑标记——会话1 落盘、会话2 以内容匹配判持久化。
    pub const MS_SNAP_MARKER: &[u8] = b"VARIX-M21-milestone";
    /// 会话1 封条日志条数（Write 700..702）。
    const MS_SEAL_WRITES: u64 = 3;
    /// 恢复会话追加的续写记录 blk 号（与封条号段区分，便于盘面取证）。
    const MS_RECOVERY_BLK: u64 = 900;

    /// M2 探针回执（宿主测试断言用；目标态以 kinfo 打印为准）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct JournalVerdict {
        /// "fresh"（会话1）| "recovered"（会话2+）。
        pub session: &'static str,
        /// open 报告/盘面条目数（fresh 会话 = 封条后总长）。
        pub entries: usize,
        /// 撕裂事件数（0 = 干净）。
        pub torn: u32,
        /// 本会话封条成功条数（recovered 会话恒 0）。
        pub sealed_writes: u64,
        /// 恢复会话续写记录 seq（None = 续写被拒）。
        pub resumed_seq: Option<u64>,
    }

    /// M3/M4 探针回执。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SharedVerdict {
        /// M3 exFAT 读 /apps.json 成功且内容契约前缀吻合。
        pub m3_ok: bool,
        /// M3 读到字节数。
        pub m3_len: u64,
        /// M4 会话1 marker 在盘面（跨断电持久判据）。
        pub marker_persisted: bool,
        /// M4 本会话 marker 落盘槽位。
        pub slot: Option<u64>,
    }

    /// M2：journal 真盘双会话。空盘（open Err）= format → 封条 3 条 →
    /// ready-for-powercut；有超块（open Ok）= 恢复会话：核对盘面
    /// （entries ≥ 封条数且零撕裂）后追加续写记录。
    pub fn ms_journal_probe(mut dev: &mut dyn BlockDevice) -> JournalVerdict {
        match DiskJournal::open(&mut *dev, JOURNAL_DEFAULT, MS_BASE_LBA) {
            Ok((mut j, rep)) => {
                crate::kinfo!(
                    "milestone: M2 journal open entries={} torn={} replayed={} base_seq={} (powercut-recovery path)",
                    rep.entries, rep.torn, rep.replayed, rep.base_seq
                );
                if rep.torn != 0 || rep.entries < MS_SEAL_WRITES as usize {
                    crate::kwarn!("milestone: M2 seal mismatch — verdict=tainted");
                    return JournalVerdict {
                        session: "recovered",
                        entries: rep.entries,
                        torn: rep.torn,
                        sealed_writes: 0,
                        resumed_seq: None,
                    };
                }
                let resumed_seq = j.append(LogOp::Write { blk: MS_RECOVERY_BLK });
                if let Some(seq) = resumed_seq {
                    crate::kinfo!("milestone: journal-recovered verdict=ok resumed_seq={}", seq);
                } else {
                    crate::kwarn!("milestone: journal-recovered verdict=tainted — resume append rejected");
                }
                JournalVerdict {
                    session: "recovered",
                    entries: rep.entries,
                    torn: rep.torn,
                    sealed_writes: 0,
                    resumed_seq,
                }
            }
            Err(_) => {
                crate::kinfo!("milestone: M2 journal fresh — format base_lba={}", MS_BASE_LBA);
                if DiskJournal::format(&mut dev, JOURNAL_DEFAULT, MS_BASE_LBA).is_err() {
                    crate::kwarn!("milestone: M2 format failed");
                    return JournalVerdict {
                        session: "fresh",
                        entries: 0,
                        torn: 0,
                        sealed_writes: 0,
                        resumed_seq: None,
                    };
                }
                let Ok((mut j, rep)) = DiskJournal::open(&mut *dev, JOURNAL_DEFAULT, MS_BASE_LBA)
                else {
                    crate::kwarn!("milestone: M2 open after format failed");
                    return JournalVerdict {
                        session: "fresh",
                        entries: 0,
                        torn: 0,
                        sealed_writes: 0,
                        resumed_seq: None,
                    };
                };
                crate::kinfo!("milestone: M2 journal formatted base_seq={}", rep.base_seq);
                let mut sealed = 0u64;
                for i in 0..MS_SEAL_WRITES {
                    if j.append(LogOp::Write { blk: 700 + i }).is_some() {
                        sealed += 1;
                    }
                }
                if sealed == MS_SEAL_WRITES {
                    crate::kinfo!(
                        "milestone: M2 journal sealed entries={} (tier={} flush-persisted)",
                        j.len(),
                        JOURNAL_DEFAULT
                    );
                    crate::kinfo!("milestone: ready-for-powercut");
                } else {
                    crate::kwarn!("milestone: M2 seal incomplete {}/{}", sealed, MS_SEAL_WRITES);
                }
                JournalVerdict {
                    session: "fresh",
                    entries: j.len(),
                    torn: j.torn_count(),
                    sealed_writes: sealed,
                    resumed_seq: None,
                }
            }
        }
    }

    /// M3+M4：SHARED 卷。M3 exFAT 读 /apps.json（真盘读路径）；M4 快照区
    /// marker 持久化核对（会话2 检出会话1 落盘内容 → snapshot-persisted）。
    /// M3 失败不阻断 M4——两路证据独立，失败均如实上报。
    pub fn ms_shared_probe(dev: &mut dyn BlockDevice) -> SharedVerdict {
        let mut m3_ok = false;
        let mut m3_len = 0u64;
        match ExfatVolume::mount(&mut *dev) {
            Ok(mut vol) => match vol.read_file("/apps.json") {
                Ok(data) => {
                    m3_len = data.len() as u64;
                    m3_ok = data.starts_with(b"{");
                    crate::kinfo!(
                        "milestone: M3 exfat-read ok /apps.json len={} content_ok={}",
                        m3_len,
                        m3_ok
                    );
                }
                Err(e) => crate::kwarn!("milestone: M3 read /apps.json failed {:?}", e),
            },
            Err(e) => crate::kwarn!("milestone: M3 mount failed {:?}", e),
        }
        // vol 已 drop，dev 借用释放 → M4 复用同一设备（shared_probe 同范式）。
        let mut snap = SnapshotArea::new(&mut *dev, MS_SNAP_BASE);
        let mut marker_persisted = false;
        match snap.list() {
            Ok(records) => {
                marker_persisted = records.iter().any(|r| r.content == MS_SNAP_MARKER);
                crate::kinfo!(
                    "milestone: M4 snapshot list={} marker_persisted={}",
                    records.len(),
                    marker_persisted
                );
            }
            Err(e) => crate::kwarn!("milestone: M4 snapshot list failed {:?}", e),
        }
        if marker_persisted {
            crate::kinfo!("milestone: snapshot-persisted ok");
        }
        let slot = match snap.append("/milestone/m21", MS_SNAP_MARKER) {
            Ok(s) => {
                crate::kinfo!("milestone: M4 snapshot written slot={}", s);
                Some(s)
            }
            Err(e) => {
                crate::kwarn!("milestone: M4 snapshot append failed {:?}", e);
                None
            }
        };
        SharedVerdict { m3_ok, m3_len, marker_persisted, slot }
    }
}

#[cfg(test)]
mod tests {
    use super::target::{ms_journal_probe, ms_shared_probe, JournalVerdict, SharedVerdict};
    use crate::drivers::blk::{BlockDevice, BlockError};
    use std::collections::BTreeMap;

    /// 内存块设备（宿主测试后端，与 fs23/exfat 测试注入器同族）。
    struct MemDisk {
        blocks: BTreeMap<u64, [u8; 512]>,
        total: u64,
    }

    impl MemDisk {
        fn new(total: u64) -> Self {
            MemDisk { blocks: BTreeMap::new(), total }
        }
    }

    impl BlockDevice for MemDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.total
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            if dst.is_empty() || dst.len() % 512 != 0 {
                return Err(BlockError::InvalidRange);
            }
            let n = (dst.len() / 512) as u64;
            if lba.checked_add(n).ok_or(BlockError::InvalidRange)? > self.total {
                return Err(BlockError::InvalidRange);
            }
            let zero = [0u8; 512];
            for (i, chunk) in dst.chunks_mut(512).enumerate() {
                let b = self.blocks.get(&(lba + i as u64)).unwrap_or(&zero);
                chunk.copy_from_slice(b);
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            if src.is_empty() || src.len() % 512 != 0 {
                return Err(BlockError::InvalidRange);
            }
            let n = (src.len() / 512) as u64;
            if lba.checked_add(n).ok_or(BlockError::InvalidRange)? > self.total {
                return Err(BlockError::InvalidRange);
            }
            for (i, chunk) in src.chunks(512).enumerate() {
                let mut b = [0u8; 512];
                b.copy_from_slice(chunk);
                self.blocks.insert(lba + i as u64, b);
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    #[test]
    fn milestone_journal_two_session_recovery() {
        let mut disk = MemDisk::new(65_536);
        // 会话1：空盘 → format → 封条 3 条。
        let v1: JournalVerdict = ms_journal_probe(&mut disk);
        assert_eq!(v1.session, "fresh");
        assert_eq!(v1.entries, 3);
        assert_eq!(v1.torn, 0);
        assert_eq!(v1.sealed_writes, 3);
        assert_eq!(v1.resumed_seq, None);
        // 会话2：跨「断电」盘面 → 恢复 3 条 + 续写 seq=4。
        let v2 = ms_journal_probe(&mut disk);
        assert_eq!(v2.session, "recovered");
        assert_eq!(v2.entries, 3);
        assert_eq!(v2.torn, 0);
        assert_eq!(v2.resumed_seq, Some(4));
        // 会话3：条目继续增长（续写记录也算盘面条目）仍可恢复。
        let v3 = ms_journal_probe(&mut disk);
        assert_eq!(v3.session, "recovered");
        assert_eq!(v3.entries, 4);
        assert_eq!(v3.resumed_seq, Some(5));
    }

    #[test]
    fn milestone_snapshot_marker_persistence() {
        let mut disk = MemDisk::new(65_536);
        // 会话1：宿主无 exFAT 盘面 → M3 如实失败（目标态证据见串口日志）；
        // M4 首写 marker。
        let s1: SharedVerdict = ms_shared_probe(&mut disk);
        assert!(!s1.m3_ok);
        assert_eq!(s1.m3_len, 0);
        assert!(!s1.marker_persisted);
        assert_eq!(s1.slot, Some(0));
        // 会话2：marker 跨「断电」仍在盘面 → 持久化判据命中；追加推进槽位。
        let s2 = ms_shared_probe(&mut disk);
        assert!(s2.marker_persisted);
        assert_eq!(s2.slot, Some(1));
    }
}
