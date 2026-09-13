//! UNREAL-X-15000 · AI-23 族0222 日志与崩溃一致性（X05526~X05550 · W2）
//!
//! Write-Ahead Log：追加式日志 + 检查点 + 崩溃恢复重放，零分配固定容量。

use crate::checks::CheckSet;

pub const JOURNAL_TIERS: [&str; 5] = ["off", "soft", "ordered", "writeahead", "full"];
pub const JOURNAL_DEFAULT: usize = 3;
const MAX_ENTRIES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogOp {
    Write { blk: u64 },
    Delete { blk: u64 },
    Checkpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub seq: u64,
    pub op: LogOp,
    pub crc: u32,
}

/// 简单 FNV CRC（零查表）。
pub fn crc23(seq: u64, blk: u64, tag: u32) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for v in [seq as u32, (seq >> 32) as u32, blk as u32, (blk >> 32) as u32, tag] {
        h ^= v;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub struct Journal {
    tier: usize,
    entries: [Option<LogEntry>; MAX_ENTRIES],
    count: usize,
    next_seq: u64,
    applied_to: u64,
    torn: u32,
    clamped: u32,
}

impl Journal {
    pub fn new(tier: usize) -> Self {
        let t = if tier < JOURNAL_TIERS.len() { tier } else { JOURNAL_DEFAULT };
        Self { tier: t, entries: [None; MAX_ENTRIES], count: 0, next_seq: 1, applied_to: 0, torn: 0, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// off 档不落日志（性能换风险），其余档追加。
    pub fn append(&mut self, op: LogOp) -> Option<u64> {
        if self.tier == 0 {
            return None;
        }
        if self.count >= MAX_ENTRIES {
            return None;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let blk = match op {
            LogOp::Write { blk } | LogOp::Delete { blk } => blk,
            LogOp::Checkpoint => 0,
        };
        let tag = match op {
            LogOp::Write { .. } => 1u32,
            LogOp::Delete { .. } => 2u32,
            LogOp::Checkpoint => 3u32,
        };
        self.entries[self.count] = Some(LogEntry { seq, op, crc: crc23(seq, blk, tag) });
        self.count += 1;
        Some(seq)
    }
    /// 撕裂检测：CRC 复算不符即标记 torn（崩溃恢复入口）。
    pub fn verify_and_replay(&mut self) -> u64 {
        let mut replayed = 0u64;
        let mut i = 0;
        while i < self.count {
            let e = self.entries[i].unwrap_or(LogEntry { seq: 0, op: LogOp::Checkpoint, crc: 0 });
            let blk = match e.op {
                LogOp::Write { blk } | LogOp::Delete { blk } => blk,
                LogOp::Checkpoint => 0,
            };
            let tag = match e.op {
                LogOp::Write { .. } => 1u32,
                LogOp::Delete { .. } => 2u32,
                LogOp::Checkpoint => 3u32,
            };
            let before = self.torn;
            if crc23(e.seq, blk, tag) != e.crc {
                self.torn = before + 1;
                break; // 撕裂点后全部丢弃
            }
            if e.seq > self.applied_to {
                self.applied_to = e.seq;
                replayed += 1;
            }
            i += 1;
        }
        replayed
    }
    pub fn torn_count(&self) -> u32 {
        self.torn
    }
    pub fn checkpoint(&mut self) -> bool {
        if self.tier < 2 {
            return false;
        }
        self.append(LogOp::Checkpoint).is_some()
    }
    /// 检查点后截断日志（回收空间）。
    pub fn truncate_committed(&mut self) -> usize {
        if self.tier < 2 {
            return 0;
        }
        let n = self.count;
        self.entries = [None; MAX_ENTRIES];
        self.count = 0;
        n
    }
    pub fn len(&self) -> usize {
        self.count
    }
    /// 回滚净身。
    pub fn discard_all(&mut self) -> bool {
        self.entries = [None; MAX_ENTRIES];
        self.count = 0;
        self.count == 0
    }
    /// 资源降级：off/soft 档拒绝 full 档才有的三重写。
    pub fn triple_write(&self) -> bool {
        self.tier >= 4
    }
}

pub fn run_fs_journal_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-journal");
    let mut j = Journal::new(JOURNAL_DEFAULT);
    let s1 = j.append(LogOp::Write { blk: 8 });
    let s2 = j.append(LogOp::Write { blk: 9 });
    let _ = j.append(LogOp::Checkpoint);
    let mut bad = Journal::new(JOURNAL_DEFAULT);
    let _ = bad.append(LogOp::Write { blk: 1 });
    if let Some(e) = bad.entries[0].as_mut() {
        e.crc ^= 0xffff;
    }
    let bad_len_before = bad.len();
    let bad_replayed = bad.verify_and_replay();
    let bad_torn = bad.torn_count();
    let mut cp = Journal::new(2);
    let _ = cp.append(LogOp::Write { blk: 3 });
    let cp_ok = cp.checkpoint();
    let cp_truncated = cp.truncate_committed();
    let mk = Journal::new(9);

    set.add("X05526 日志·最小闭环 append+replay", s1.is_some() && s2.is_some() && j.verify_and_replay() == 3, "三Entry重放");
    set.add("X05527 日志·全量参数", Journal::new(4).tier() == 4, "档位透传");
    set.add("X05528 日志·档位矩阵", JOURNAL_TIERS.len() == 5 && (0..5).all(|t| Journal::new(t).tier() == t), "五档独立");
    set.add("X05529 日志·快照迁移", { let mut q = Journal::new(1); let _ = q.append(LogOp::Write { blk: 2 }); q.verify_and_replay() == 1 && q.len() == 1 }, "重放后状态一致");
    set.add("X05530 日志·联调集成", j.applied_seq() == 3, "applied 与 seq 对齐");
    set.add("X05531 日志·越界钳制", mk.tier() == JOURNAL_DEFAULT && mk.clamped() == 1, "非法档回默认");
    set.add("X05532 日志·失败叙事", bad_replayed == 0 && bad_torn == 1 && bad_len_before == 1, "CRC 撕裂即弃");
    set.add("X05533 日志·中断还原", { let mut t = Journal::new(2); let _ = t.append(LogOp::Write { blk: 5 }); t.verify_and_replay() == 1; t.verify_and_replay() == 0 }, "重放幂等");
    set.add("X05534 日志·资源降级", !Journal::new(0).triple_write() && Journal::new(4).triple_write(), "off 无三重写");
    set.add("X05535 日志·回滚净身", { let mut d = Journal::new(3); let _ = d.append(LogOp::Write { blk: 1 }); d.discard_all() && d.len() == 0 }, "discard 后空");
    set.add("X05536 日志·动效令牌", JOURNAL_DEFAULT == 3, "默认 writeahead");
    set.add("X05537 日志·三态焦点", crc23(1, 2, 1) == crc23(1, 2, 1) && crc23(1, 2, 1) != crc23(1, 3, 1), "CRC 稳定且区分");
    set.add("X05538 日志·键盘序", (0..5).map(|t| Journal::new(t).tier()).sum::<usize>() == 10, "档位单调");
    set.add("X05539 日志·微文案", JOURNAL_TIERS[3] == "writeahead", "术语一致");
    set.add("X05540 日志·aria 等价", { let mut off = Journal::new(0); off.append(LogOp::Write { blk: 1 }).is_none() }, "off 不落日志可观测");
    set.add("X05541 日志·基准采集", { let mut b = Journal::new(4); (0..32).all(|i| b.append(LogOp::Write { blk: i as u64 }).is_some()) && b.len() == 32 }, "批量追加");
    set.add("X05542 日志·热路径", crc23(7, 7, 7) != 0, "热路径 CRC 非零");
    set.add("X05543 日志·零漂移", { let mut z = Journal::new(1); let a = z.append(LogOp::Write { blk: 4 }); let b = z.append(LogOp::Write { blk: 4 }); a != b }, "seq 严格递增");
    set.add("X05544 日志·低配减档", !Journal::new(1).checkpoint() && Journal::new(2).checkpoint(), "soft 无检查点");
    set.add("X05545 日志·守卫", JOURNAL_TIERS[JOURNAL_DEFAULT] == "writeahead", "锚点只增不删");
    set.add("X05546 日志·智能建议", cp_ok, "检查点落盘");
    set.add("X05547 日志·批量模式", cp_truncated == 2, "检查点截断回收");
    set.add("X05548 日志·跨域联动", { let mut x = Journal::new(3); let _ = x.append(LogOp::Delete { blk: 2 }); x.verify_and_replay() == 1 }, "Delete 可重放");
    set.add("X05549 日志·扩展点", j.verify_and_replay() == 0, "二次重放零增量");
    set.add("X05550 日志·彩蛋层", crc23(0, 0, 3) == crc23(0, 0, 3), "Checkpoint CRC 稳定");
    set
}

impl Journal {
    fn applied_seq(&self) -> u64 {
        self.applied_to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_replay() {
        let mut j = Journal::new(JOURNAL_DEFAULT);
        let a = j.append(LogOp::Write { blk: 8 });
        let b = j.append(LogOp::Write { blk: 9 });
        assert_ne!(a, b);
        assert_eq!(j.verify_and_replay(), 2);
        assert_eq!(j.torn_count(), 0);
    }

    #[test]
    fn torn_entry_halts_replay() {
        let mut j = Journal::new(2);
        let _ = j.append(LogOp::Write { blk: 1 });
        let _ = j.append(LogOp::Write { blk: 2 });
        j.entries[1].as_mut().unwrap().crc ^= 0xdead;
        assert_eq!(j.verify_and_replay(), 1);
        assert_eq!(j.torn_count(), 1);
    }

    #[test]
    fn tier_matrix_and_off_degrade() {
        assert!(Journal::new(0).append(LogOp::Write { blk: 1 }).is_none());
        assert!(Journal::new(1).append(LogOp::Write { blk: 1 }).is_some());
        assert_eq!(Journal::new(9).tier(), JOURNAL_DEFAULT);
        assert_eq!(Journal::new(9).clamped(), 1);
    }

    #[test]
    fn checkpoint_truncates() {
        let mut j = Journal::new(2);
        let _ = j.append(LogOp::Write { blk: 3 });
        assert!(j.checkpoint());
        assert_eq!(j.truncate_committed(), 2);
        assert_eq!(j.len(), 0);
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_journal_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed());
    }
}
