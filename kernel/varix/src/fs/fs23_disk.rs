//! 任务17 · fs23_journal 块设备后端（双域总案 2.3 真驱动·步骤 10）。
//!
//! 内存 64 槽模型（`fs23_journal::Journal`）替换为真块设备 WAL 后端，
//! journal 语义逐条保持不变：
//!
//! - **seq 严格递增**：槽 i 的 seq 必须等于 `base_seq + i`（超块水位），
//!   断裂/乱序等价撕裂，该点及之后全部丢弃。
//! - **torn 丢弃该点及之后全部**：CRC 复算不符（含撕裂半写、位腐坏、
//!   seq 断裂）→ 停止重放，之前条目全部有效。
//! - **幂等重放**：重放推进 `applied_to` 水位，重复重放零增量；`open`
//!   可对同一盘面重复调用，结果恒定。
//! - **档位矩阵不变**：off 不落日志 / soft 落盘不 flush / ordered+ 落盘
//!   即 flush / full 三重写（三副本区，读侧"至少一份完好"）。
//! - **检查点仅 tier≥2**，checkpoint 后 truncate 回收槽区。
//!
//! 盘面布局（块大小固定 512——实现边界如实声明；base_lba 参数化，
//! 实机探针用高区与 loopback 探针区无交集）：
//!
//! ```text
//! LBA base+0          超块（magic/version/base_seq/crc）
//! LBA base+1..+64     主日志区（64 槽，1 条/块）
//! LBA base+65..+128   副本1（仅 full 档写入）
//! LBA base+129..+192  副本2（仅 full 档写入）
//! ```
//!
//! 槽记录（块内前 24 字节，其余零）：seq u64 LE、blk u64 LE、
//! tag u32 LE（0=空 1=Write 2=Delete 3=Checkpoint）、crc u32 LE =
//! `crc23(seq, blk, tag)`。撕裂检测窗口 = 记录尾部（tag/crc）未落盘
//! 时判 Bad；记录整体未到达时判 Empty（日志自然终点）。
//!
//! 掉电一致性：`append` 的持久化点 = 槽写 + flush 返回（tier≥2）；
//! flush 返回后掉电，条目必须可恢复（×10 注入回归逐点验证）。

use crate::drivers::blk::{BlockDevice, BlockError};
use crate::fs::fs23_journal::{crc23, LogOp, JOURNAL_DEFAULT, JOURNAL_TIERS};

/// 日志槽数——与内存版 `MAX_ENTRIES` 同容量语义。
pub const SLOTS: usize = 64;
/// full 档副本区数（主 + 2 副 = 三重写）。
pub const REPLICA_REGIONS: usize = 3;
/// 超块 magic "FS23"（LE）。
const SUPER_MAGIC: u32 = 0x4653_3233;
const SUPER_VERSION: u32 = 1;
const SUPER_CRC_TAG: u32 = 0x5A;

const TAG_WRITE: u32 = 1;
const TAG_DELETE: u32 = 2;
const TAG_CHECKPOINT: u32 = 3;

fn tag_of(op: LogOp) -> u32 {
    match op {
        LogOp::Write { .. } => TAG_WRITE,
        LogOp::Delete { .. } => TAG_DELETE,
        LogOp::Checkpoint => TAG_CHECKPOINT,
    }
}

fn blk_of(op: LogOp) -> u64 {
    match op {
        LogOp::Write { blk } | LogOp::Delete { blk } => blk,
        LogOp::Checkpoint => 0,
    }
}

/// 超块 CRC（与槽 CRC 同族，tag 用保留魔数防与日志条目混淆）。
fn super_crc(base_seq: u64) -> u32 {
    crc23(base_seq, 0, SUPER_CRC_TAG)
}

/// `open` 的结果报告——串口验收行直接格式化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenReport {
    /// 重放恢复的条目数（= 当前日志长度）。
    pub entries: usize,
    /// 撕裂丢弃事件数（0 = 干净恢复）。
    pub torn: u32,
    /// 重放推进数（首次 open = entries）。
    pub replayed: u64,
    /// 超块水位 base_seq。
    pub base_seq: u64,
}

/// fs23_journal 块设备后端——WAL 落盘。
///
/// 泛型后端：宿主注入器与目标态 NVMe 同一套逻辑。
pub struct DiskJournal<B: BlockDevice> {
    dev: B,
    tier: usize,
    base_lba: u64,
    /// append 指针（重放扫描后建立）。
    count: usize,
    /// 超块水位：槽 i 期望 seq = base_seq + i。
    base_seq: u64,
    applied_to: u64,
    torn: u32,
    clamped: u32,
}

impl<B: BlockDevice> DiskJournal<B> {
    /// 格式化：清零全部槽区 + 写超块（base_seq=1）+ flush。幂等。
    pub fn format(dev: &mut B, tier: usize, base_lba: u64) -> Result<(), BlockError> {
        let _ = tier;
        if dev.block_size() != 512 {
            return Err(BlockError::Unsupported);
        }
        let zero = [0u8; 512];
        // 副本区无条件清，档位切换无残留。
        for r in 0..REPLICA_REGIONS as u64 {
            for i in 0..SLOTS as u64 {
                dev.write_blocks(base_lba + 1 + r * SLOTS as u64 + i, &zero)?;
            }
        }
        dev.write_blocks(base_lba, &super_block(1))?;
        dev.flush()
    }

    /// 打开：读超块 + 重放扫描（CRC/seq 连续性判定，torn 丢弃该点及
    /// 之后全部）。超块损坏返回 Err——保守拒绝，绝不静默覆盖旧日志。
    pub fn open(mut dev: B, tier: usize, base_lba: u64) -> Result<(Self, OpenReport), BlockError> {
        let t = if tier < JOURNAL_TIERS.len() { tier } else { JOURNAL_DEFAULT };
        let clamped = if t != tier { 1 } else { 0 };
        if dev.block_size() != 512 {
            return Err(BlockError::Unsupported);
        }
        let mut sb = [0u8; 512];
        dev.read_blocks(base_lba, &mut sb)?;
        if le_u32(&sb[0..4]) != SUPER_MAGIC || le_u32(&sb[4..8]) != SUPER_VERSION {
            return Err(BlockError::Io);
        }
        let base_seq = le_u64(&sb[8..16]);
        if le_u32(&sb[16..20]) != super_crc(base_seq) {
            return Err(BlockError::Io);
        }
        let mut j = DiskJournal {
            dev,
            tier: t,
            base_lba,
            count: 0,
            base_seq,
            applied_to: 0,
            torn: 0,
            clamped,
        };
        let mut entries = 0usize;
        let mut torn = 0u32;
        let replayed = j.rescan(&mut entries, &mut torn);
        let rep = OpenReport { entries, torn, replayed, base_seq };
        Ok((j, rep))
    }

    /// 重放扫描主区（full 档副本救援）：返回重放推进数。
    fn rescan(&mut self, entries_out: &mut usize, torn_out: &mut u32) -> u64 {
        let mut replayed = 0u64;
        let full = self.tier >= 4;
        let mut buf = [0u8; 512];
        let mut i = 0usize;
        while i < SLOTS {
            let lba = self.base_lba + 1 + i as u64;
            if self.dev.read_blocks(lba, &mut buf).is_err() {
                self.torn += 1; // 读失败按撕裂处理（该点及之后丢弃）。
                break;
            }
            match unpack(&buf) {
                SlotRead::Empty => break, // 日志自然终点
                SlotRead::Bad => {
                    if full && self.replica_good(i) {
                        let (seq, _blk, _tag) = self.read_replica(i);
                        self.apply(seq, &mut replayed);
                        i += 1;
                        continue;
                    }
                    self.torn += 1;
                    break;
                }
                SlotRead::Good(seq, _blk, _tag) => {
                    if seq != self.base_seq + i as u64 {
                        self.torn += 1; // seq 断裂/乱序 = 等价撕裂。
                        break;
                    }
                    self.apply(seq, &mut replayed);
                    i += 1;
                }
            }
        }
        *entries_out = self.count;
        *torn_out = self.torn;
        replayed
    }

    fn apply(&mut self, seq: u64, replayed: &mut u64) {
        self.count += 1;
        if seq > self.applied_to {
            self.applied_to = seq;
            *replayed += 1;
        }
    }

    /// full 档：副本区槽 i 是否至少一份完好。
    fn replica_good(&mut self, i: usize) -> bool {
        for r in 1..REPLICA_REGIONS as u64 {
            let mut buf = [0u8; 512];
            let lba = self.base_lba + 1 + r * SLOTS as u64 + i as u64;
            if self.dev.read_blocks(lba, &mut buf).is_ok()
                && matches!(unpack(&buf), SlotRead::Good(..))
            {
                return true;
            }
        }
        false
    }

    /// full 档：从副本读回（调用前 `replica_good` 已确认存在）。
    fn read_replica(&mut self, i: usize) -> (u64, u64, u32) {
        for r in 1..REPLICA_REGIONS as u64 {
            let mut buf = [0u8; 512];
            let lba = self.base_lba + 1 + r * SLOTS as u64 + i as u64;
            if self.dev.read_blocks(lba, &mut buf).is_ok() {
                if let SlotRead::Good(seq, blk, tag) = unpack(&buf) {
                    return (seq, blk, tag);
                }
            }
        }
        (0, 0, 0)
    }

    /// off 档不落日志；满容量明确拒绝；tier≥2 写后 flush。
    /// 持久化点语义：`Some(seq)` 返回即已按档位完成持久化承诺。
    pub fn append(&mut self, op: LogOp) -> Option<u64> {
        if self.tier == 0 {
            return None;
        }
        if self.count >= SLOTS {
            return None;
        }
        let seq = self.base_seq + self.count as u64;
        let blk = blk_of(op);
        let tag = tag_of(op);
        let buf = pack(seq, blk, tag);
        let lba = self.base_lba + 1 + self.count as u64;
        if self.dev.write_blocks(lba, &buf).is_err() {
            return None;
        }
        if self.tier >= 4 {
            for r in 1..REPLICA_REGIONS as u64 {
                let rl = self.base_lba + 1 + r * SLOTS as u64 + self.count as u64;
                if self.dev.write_blocks(rl, &buf).is_err() {
                    return None;
                }
            }
        }
        if self.tier >= 2 && self.dev.flush().is_err() {
            return None;
        }
        self.count += 1;
        self.applied_to = seq;
        Some(seq)
    }

    /// 撕裂检测：重放扫描（崩溃恢复入口，幂等——返回本次新增重放数，
    /// open 已扫过则为 0）。
    pub fn verify_and_replay(&mut self) -> u64 {
        let mut entries = self.count;
        let mut torn = self.torn;
        self.rescan(&mut entries, &mut torn)
    }

    pub fn torn_count(&self) -> u32 {
        self.torn
    }

    /// 检查点仅 tier≥2（盘上落 tag=3 条目）。
    pub fn checkpoint(&mut self) -> bool {
        if self.tier < 2 {
            return false;
        }
        self.append(LogOp::Checkpoint).is_some()
    }

    /// 检查点后截断：清槽区 + 超块水位推进到 next_seq + flush。
    /// 返回截断条数（tier<2 拒绝）。
    pub fn truncate_committed(&mut self) -> usize {
        if self.tier < 2 || self.count == 0 {
            return 0;
        }
        let n = self.count;
        let next = self.base_seq + n as u64;
        let zero = [0u8; 512];
        for r in 0..REPLICA_REGIONS as u64 {
            for i in 0..SLOTS as u64 {
                if self
                    .dev
                    .write_blocks(self.base_lba + 1 + r * SLOTS as u64 + i, &zero)
                    .is_err()
                {
                    return 0;
                }
            }
        }
        if self.dev.write_blocks(self.base_lba, &super_block(next)).is_err() {
            return 0;
        }
        if self.dev.flush().is_err() {
            return 0;
        }
        self.count = 0;
        self.base_seq = next;
        n
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 回滚净身：清槽区 + 超块水位推进（掉电/重开后 seq 仍延续，
    /// 与内存版"水位不动、seq 继续递增"语义对齐）。
    pub fn discard_all(&mut self) -> bool {
        let zero = [0u8; 512];
        for r in 0..REPLICA_REGIONS as u64 {
            for i in 0..SLOTS as u64 {
                if self
                    .dev
                    .write_blocks(self.base_lba + 1 + r * SLOTS as u64 + i, &zero)
                    .is_err()
                {
                    return false;
                }
            }
        }
        let next = self.base_seq + self.count as u64;
        if self.dev.write_blocks(self.base_lba, &super_block(next)).is_err() {
            return false;
        }
        if self.dev.flush().is_err() {
            return false;
        }
        self.base_seq = next;
        self.count = 0;
        true
    }

    /// 资源降级：off/soft 档拒绝 full 档才有的三重写（语义与内存版一致）。
    pub fn triple_write(&self) -> bool {
        self.tier >= 4
    }

    pub fn tier(&self) -> usize {
        self.tier
    }

    pub fn clamped(&self) -> u32 {
        self.clamped
    }

    pub fn applied_seq(&self) -> u64 {
        self.applied_to
    }

    pub fn next_seq(&self) -> u64 {
        self.base_seq + self.count as u64
    }

    pub fn into_inner(self) -> B {
        self.dev
    }
}

fn pack(seq: u64, blk: u64, tag: u32) -> [u8; 512] {
    let mut buf = [0u8; 512];
    buf[0..8].copy_from_slice(&seq.to_le_bytes());
    buf[8..16].copy_from_slice(&blk.to_le_bytes());
    buf[16..20].copy_from_slice(&tag.to_le_bytes());
    let crc = crc23(seq, blk, tag);
    buf[20..24].copy_from_slice(&crc.to_le_bytes());
    buf
}

fn super_block(base_seq: u64) -> [u8; 512] {
    let mut buf = [0u8; 512];
    buf[0..4].copy_from_slice(&SUPER_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&SUPER_VERSION.to_le_bytes());
    buf[8..16].copy_from_slice(&base_seq.to_le_bytes());
    buf[16..20].copy_from_slice(&super_crc(base_seq).to_le_bytes());
    buf
}

enum SlotRead {
    /// tag=0 且余量零——日志自然终点。
    Empty,
    /// CRC 不符 / tag 非法 / 半写残留。
    Bad,
    Good(u64, u64, u32),
}

fn unpack(buf: &[u8]) -> SlotRead {
    let tag = le_u32(&buf[16..20]);
    let crc = le_u32(&buf[20..24]);
    if tag == 0 {
        // 空槽：余量必须全零（format/truncate 清零保证）；有残留即
        // 半写（Bad），交由撕裂语义处理。
        return if buf[0..16].iter().all(|&b| b == 0) && crc == 0 {
            SlotRead::Empty
        } else {
            SlotRead::Bad
        };
    }
    if tag > TAG_CHECKPOINT {
        return SlotRead::Bad;
    }
    let seq = le_u64(&buf[0..8]);
    let blk = le_u64(&buf[8..16]);
    if crc23(seq, blk, tag) != crc {
        return SlotRead::Bad;
    }
    SlotRead::Good(seq, blk, tag)
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

// ---------------------------------------------------------------------------
// 目标态：QEMU NVMe 实机掉电注入探针（main.rs 自检链挂接）。
//
// 轮次协议（外部脚本 kill QEMU 模拟掉电，盘面跨进程持久）：
//   每轮：open（首轮自动 format）→ 报告 entries/torn → append 3 条 →
//   报告 total → verdict（torn==0 → ok）。11 轮后 total=33 零 torn。
// ---------------------------------------------------------------------------
pub mod target {
    use super::*;

    /// NVMe 盘高区（loopback 探针最高 LBA ≈ 16007，此处之后无交集）。
    pub const FS23_BASE_LBA: u64 = 20_000;

    pub fn fs23_powercut_probe(mut dev: &mut dyn BlockDevice) {
        const TIER: usize = JOURNAL_DEFAULT;
        // &mut *dev reborrow：经 `&mut T` blanket impl 直构泛型实例。
        let mut opened = DiskJournal::open(&mut *dev, TIER, FS23_BASE_LBA);
        if opened.is_err() {
            // 未格式化（首轮）。显式重建——open Err 仅超块级损坏才会
            // 走到这里，不存在静默覆盖有效日志的路径。
            if DiskJournal::format(&mut dev, TIER, FS23_BASE_LBA).is_err() {
                crate::kwarn!("fs23-disk format failed base_lba={}", FS23_BASE_LBA);
                return;
            }
            opened = DiskJournal::open(&mut *dev, TIER, FS23_BASE_LBA);
        }
        let Ok((mut j, rep)) = opened else {
            crate::kwarn!("fs23-disk open failed after format base_lba={}", FS23_BASE_LBA);
            return;
        };
        crate::kinfo!(
            "fs23-disk open entries={} torn={} replayed={} base_seq={} base_lba={}",
            rep.entries,
            rep.torn,
            rep.replayed,
            rep.base_seq,
            FS23_BASE_LBA
        );
        let mut appended = 0usize;
        for _ in 0..3u64 {
            let blk = j.next_seq();
            if j.append(LogOp::Write { blk }).is_some() {
                appended += 1;
            }
        }
        let verdict = if j.torn_count() == 0 { "ok" } else { "tainted" };
        crate::kinfo!(
            "fs23-disk appended={} total={} next_seq={} torn={}",
            appended,
            j.len(),
            j.next_seq(),
            j.torn_count()
        );
        crate::kinfo!("fs23-disk verify verdict={}", verdict);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // -- 掉电注入器 ---------------------------------------------------------

    /// 字节级掉电注入器：写进易失层，flush 才落盘；可注入
    /// ①第 n 次写后崩溃（易失层丢失，flush 未达）②第 n 次写撕裂
    /// （前 k 字节直接落盘，模拟半扇区到达介质）。`power_cut` 丢弃
    /// 易失层返回冻结盘面（= 掉电后磁盘真实内容）。
    struct CrashSim {
        disk: Vec<u8>,
        /// 易失写缓存：key = 块起始字节偏移。
        volatile: BTreeMap<usize, Vec<u8>>,
        crash_after_write: Option<usize>,
        torn_write: Option<(usize, usize)>,
        writes: usize,
        crashed: bool,
    }

    impl CrashSim {
        fn new(blocks: u64) -> Self {
            CrashSim {
                disk: vec![0u8; blocks as usize * 512],
                volatile: BTreeMap::new(),
                crash_after_write: None,
                torn_write: None,
                writes: 0,
                crashed: false,
            }
        }

        fn span(&self, lba: u64, len: usize) -> Result<(usize, usize), BlockError> {
            let end = lba
                .checked_add(len as u64 / 512)
                .ok_or(BlockError::InvalidRange)?;
            if len == 0 || len % 512 != 0 || end > self.blocks() {
                return Err(BlockError::InvalidRange);
            }
            Ok((lba as usize * 512, end as usize * 512))
        }

        fn blocks(&self) -> u64 {
            (self.disk.len() / 512) as u64
        }

        /// 掉电：易失层消失，返回冻结盘面。
        fn power_cut(self) -> FrozenDisk {
            FrozenDisk { disk: self.disk }
        }
    }

    impl BlockDevice for CrashSim {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.blocks()
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            if self.crashed {
                return Err(BlockError::Io);
            }
            let (s, _) = self.span(lba, dst.len())?;
            for (i, out) in dst.iter_mut().enumerate() {
                let off = s + i;
                let blk_base = off - off % 512;
                *out = match self.volatile.get(&blk_base) {
                    Some(v) => v[off - blk_base],
                    None => self.disk[off],
                };
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            if self.crashed {
                return Err(BlockError::Io);
            }
            let (s, e) = self.span(lba, src.len())?;
            self.writes += 1;
            if let Some((n, keep)) = self.torn_write {
                if n == self.writes {
                    // 撕裂：前 keep 字节直接到达介质，其余保持旧内容；
                    // 易失层不进（写未完成）。
                    self.disk[s..s + keep].copy_from_slice(&src[..keep]);
                    return Ok(());
                }
            }
            self.volatile.insert(s, src.to_vec());
            if self.crash_after_write == Some(self.writes) {
                // 第 n 次写完成后即掉电：flush 永远不会发生。
                self.crashed = true;
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            if self.crashed {
                return Err(BlockError::Io);
            }
            let taken = std::mem::take(&mut self.volatile);
            for (off, v) in taken {
                self.disk[off..off + v.len()].copy_from_slice(&v);
            }
            Ok(())
        }
    }

    /// 掉电后的冻结盘面（只读语义足够恢复验证）。
    struct FrozenDisk {
        disk: Vec<u8>,
    }

    impl BlockDevice for FrozenDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            (self.disk.len() / 512) as u64
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            let s = lba as usize * 512;
            let e = s + dst.len();
            if dst.is_empty() || dst.len() % 512 != 0 || e > self.disk.len() {
                return Err(BlockError::InvalidRange);
            }
            dst.copy_from_slice(&self.disk[s..e]);
            Ok(())
        }
        /// 掉电后介质完好，新会话可直接续写（写直落，无易失层）。
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            let s = lba as usize * 512;
            let e = s + src.len();
            if src.is_empty() || src.len() % 512 != 0 || e > self.disk.len() {
                return Err(BlockError::InvalidRange);
            }
            self.disk[s..e].copy_from_slice(src);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    /// 盘面容量：超块 + 3 区 + 余量。
    const CAP: u64 = 1 + 3 * SLOTS as u64 + 8;

    fn opened_after_crash(sim: CrashSim, tier: usize) -> (DiskJournal<FrozenDisk>, OpenReport) {
        let frozen = sim.power_cut();
        DiskJournal::open(frozen, tier, 0).expect("掉电盘面 open 必须成功")
    }

    fn append_n<B: BlockDevice>(j: &mut DiskJournal<B>, n: u64) -> Vec<u64> {
        (0..n)
            .map(|i| {
                j.append(LogOp::Write { blk: 100 + i })
                    .expect("append 必须成功（容量未满）")
            })
            .collect()
    }

    /// 在冻结盘面上破坏主区槽 i 的 CRC 字段（等价介质位腐坏/撕裂）。
    fn corrupt_crc(dev: &mut FrozenDisk, slot: usize) {
        let off = (1 + slot) * 512 + 20;
        dev.disk[off..off + 4].copy_from_slice(&0xdead_beefu32.to_le_bytes());
    }

    // -- ×10 掉电注入回归 ---------------------------------------------------

    /// ×1：8 条 append（每条 flush）后干净掉电 → 8/8 零丢失。
    #[test]
    fn x1_clean_crash_after_flushed_appends() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        let seqs = append_n(&mut j, 8);
        assert_eq!(seqs.len(), 8);
        assert_eq!(seqs[0], 1, "seq 从 1 起（与内存版一致）");
        let (j2, rep) = opened_after_crash(j.into_inner(), 3);
        assert_eq!(rep.entries, 8, "8 条已 flush 条目必须全部恢复");
        assert_eq!(rep.torn, 0);
        assert_eq!(rep.replayed, 8);
        assert_eq!(j2.tier(), 3);
        assert_eq!(j2.applied_seq(), seqs[7]);
    }

    /// ×2：末条撕裂写（记录尾部未达介质）→ 前 7 条恢复 + torn=1。
    #[test]
    fn x2_torn_final_entry_discards_from_point() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 7);
        let mut sim2 = j.into_inner();
        // 下一次写只落前 12 字节（seq 到达、tag/crc 位置仍是旧零值但有
        // seq 残留 → 半写残留 → Bad）。
        sim2.torn_write = Some((sim2.writes + 1, 12));
        let mut j2 = DiskJournal::open(sim2, 3, 0).unwrap().0;
        append_n(&mut j2, 1);
        let (j3, rep) = opened_after_crash(j2.into_inner(), 3);
        assert_eq!(rep.entries, 7, "撕裂条目丢弃，之前 7 条必须完整");
        assert_eq!(rep.torn, 1);
        assert_eq!(j3.applied_seq(), 7);
    }

    /// ×3：第 5 条写后、flush 前掉电 → 已确认的 4 条恢复；第 5 条
    /// 从未确认（append 未返回 Some），如实丢弃。
    #[test]
    fn x3_crash_between_write_and_flush() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 4);
        let mut sim2 = j.into_inner();
        sim2.crash_after_write = Some(sim2.writes + 1);
        let mut j2 = DiskJournal::open(sim2, 3, 0).unwrap().0;
        // 第 5 条 append：槽写进易失层后设备崩溃，flush 返回 Err
        // → append 必须返回 None（持久化承诺未达成）。
        assert_eq!(j2.append(LogOp::Write { blk: 999 }), None, "崩溃后 append 必须拒绝");
        let (j3, rep) = opened_after_crash(j2.into_inner(), 3);
        assert_eq!(rep.entries, 4, "未 flush 的第 5 条丢失，已确认 4 条全在");
        assert_eq!(rep.torn, 0);
        assert_eq!(j3.applied_seq(), 4);
    }

    /// ×4：档位语义——soft 落盘不 flush 掉电全丢；ordered flush 后保。
    #[test]
    fn x4_soft_tier_volatile_loss_ordered_survives() {
        // soft：8 条全部在易失层，掉电即丢。
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 1, 0).unwrap();
        let mut j = DiskJournal::open(sim, 1, 0).unwrap().0;
        append_n(&mut j, 8);
        let (_, rep) = opened_after_crash(j.into_inner(), 1);
        assert_eq!(rep.entries, 0, "soft 档未 flush 掉电必须如实全丢");
        assert_eq!(rep.torn, 0);

        // ordered：append 内部 flush，掉电不丢。
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 2, 0).unwrap();
        let mut j = DiskJournal::open(sim, 2, 0).unwrap().0;
        append_n(&mut j, 8);
        let (j2, rep) = opened_after_crash(j.into_inner(), 2);
        assert_eq!(rep.entries, 8, "ordered 档 flush 后掉电零丢失");
        assert_eq!(rep.torn, 0);
        assert_eq!(j2.applied_seq(), 8);
    }

    /// ×5：full 档三重写——主份损坏，副本救援，8/8 零丢失。
    #[test]
    fn x5_full_tier_replica_rescues_corrupt_primary() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 4, 0).unwrap();
        let mut j = DiskJournal::open(sim, 4, 0).unwrap().0;
        assert!(j.triple_write(), "full 档三重写能力声明");
        append_n(&mut j, 8);
        let mut dev = j.into_inner().power_cut();
        corrupt_crc(&mut dev, 2); // 主区第 3 条（index 2）CRC 破坏。
        let (j2, rep) = DiskJournal::open(dev, 4, 0).unwrap();
        assert_eq!(rep.entries, 8, "full 档主份坏，副本必须救援，8/8 恢复");
        assert_eq!(rep.torn, 0, "副本完好时不得计撕裂");
        assert_eq!(j2.applied_seq(), 8);
    }

    /// ×6：中间条目损坏且其后条目完整在盘 → 该点及之后全部丢弃
    /// （第 6-8 条虽完好也弃——"撕裂点后全部丢弃"语义铁律）。
    #[test]
    fn x6_gap_discards_everything_after_point() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 8);
        let mut dev = j.into_inner().power_cut();
        corrupt_crc(&mut dev, 4); // 第 5 条（index 4）。
        let (j2, rep) = DiskJournal::open(dev, 3, 0).unwrap();
        assert_eq!(rep.entries, 4, "第 5 条损坏：前 4 条恢复");
        assert_eq!(rep.torn, 1);
        assert_eq!(j2.len(), 4);
        assert_eq!(j2.applied_seq(), 4);
    }

    /// ×7：链式恢复——checkpoint+truncate→再 append→掉电→恢复；
    /// 同盘面二次 open 恒定；二次重放零增量（幂等）。
    #[test]
    fn x7_chain_recover_checkpoint_truncate_reappend() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 5);
        assert!(j.checkpoint(), "tier3 检查点可用");
        assert_eq!(j.truncate_committed(), 6, "checkpoint+truncate 回收 6 条");
        append_n(&mut j, 3);
        let (j2, rep) = opened_after_crash(j.into_inner(), 3);
        assert_eq!(rep.entries, 3, "truncate 后新 3 条恢复");
        assert_eq!(rep.torn, 0);
        assert_eq!(j2.applied_seq(), 9, "truncate 后 seq 水位连续（7..9）");
        // 幂等：同一盘面二次 open 恒定；verify_and_replay 零增量。
        let frozen2 = j2.into_inner();
        let (mut j3, rep2) = DiskJournal::open(frozen2, 3, 0).unwrap();
        assert_eq!(rep2.entries, 3);
        assert_eq!(rep2.replayed, 3);
        assert_eq!(j3.verify_and_replay(), 0, "二次重放零增量");
    }

    /// ×8：64 槽满 → 第 65 条明确拒绝；掉电恢复后仍满；truncate 后可再写。
    #[test]
    fn x8_slots_full_rejects_then_recyclable() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        for i in 0..SLOTS as u64 {
            assert!(j.append(LogOp::Write { blk: i }).is_some(), "槽 {} 必须可写", i);
        }
        assert_eq!(j.append(LogOp::Write { blk: 9999 }), None, "满容量必须明确拒绝");
        assert_eq!(j.len(), SLOTS);
        // 掉电恢复：64 条全在。
        let (mut j2, rep) = opened_after_crash(j.into_inner(), 3);
        assert_eq!(rep.entries, SLOTS);
        assert_eq!(rep.torn, 0);
        assert_eq!(j2.append(LogOp::Write { blk: 9999 }), None, "恢复后仍满");
        assert_eq!(j2.truncate_committed(), SLOTS);
        assert!(j2.append(LogOp::Write { blk: 1 }).is_some(), "truncate 后可再写");
        assert_eq!(j2.len(), 1);
    }

    /// ×9：超块损坏（CRC/magic 破坏）→ open 保守拒绝，绝不静默覆盖。
    #[test]
    fn x9_superblock_corrupt_fails_safe() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 8);
        let mut dev = j.into_inner().power_cut();
        // 破坏超块 CRC 字段（模拟超块半写）。
        dev.disk[16..20].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        assert!(DiskJournal::open(dev, 3, 0).is_err(), "超块损坏必须拒绝 open");

        // magic 损坏同理。
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 8);
        let mut dev = j.into_inner().power_cut();
        dev.disk[0..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(DiskJournal::open(dev, 3, 0).is_err());
    }

    /// ×10：损坏恢复后 re-append 覆盖坏槽——seq 从水位延续不跳号；
    /// 再掉电再恢复只认新链，重扫零撕裂。
    #[test]
    fn x10_reappend_overwrites_corrupt_slot() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 7);
        let mut dev = j.into_inner().power_cut();
        corrupt_crc(&mut dev, 4); // 第 5 条坏（前 4 条有效）。
        let (mut j2, rep) = DiskJournal::open(dev, 3, 0).unwrap();
        assert_eq!(rep.entries, 4);
        assert_eq!(rep.torn, 1);
        // re-append 3 条：从撕裂点重建（槽 4/5/6 覆盖写）。
        let seqs = append_n(&mut j2, 3);
        assert_eq!(seqs, vec![5, 6, 7], "seq 从水位延续，不跳号");
        assert_eq!(j2.torn_count(), 1, "历史撕裂计数保留（如实上报）");
        // 再掉电：新链 7 条全在；坏槽已被合法条目覆盖，重扫零撕裂。
        // （j2 的后端是冻结盘面本身——写已直落，直接重开。）
        let (j3, rep2) = DiskJournal::open(j2.into_inner(), 3, 0).unwrap();
        assert_eq!(rep2.entries, 7, "重建后 7 条全恢复");
        assert_eq!(rep2.torn, 0, "坏槽已被覆盖，重扫零撕裂");
        assert_eq!(j3.applied_seq(), 7);
    }

    // -- 语义保持（与内存版逐条对齐） ---------------------------------------

    #[test]
    fn disk_tier_matrix_matches_memory_semantics() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 0, 0).unwrap();
        let mut j = DiskJournal::open(sim, 0, 0).unwrap().0;
        assert!(j.append(LogOp::Write { blk: 1 }).is_none(), "off 不落日志");
        assert!(!j.checkpoint(), "off 无检查点");
        assert!(!j.triple_write());

        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 9, 0).unwrap();
        let j = DiskJournal::open(sim, 9, 0).unwrap().0;
        assert_eq!(j.tier(), JOURNAL_DEFAULT, "非法档回默认");
        assert_eq!(j.clamped(), 1);
        assert!(!j.triple_write(), "非法档钳到 writeahead(3)，无三重写（与内存版一致）");

        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 4, 0).unwrap();
        let j = DiskJournal::open(sim, 4, 0).unwrap().0;
        assert!(j.triple_write(), "恰为 full(4) 才有三重写");
    }

    #[test]
    fn disk_delete_and_checkpoint_ops_replay() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        assert!(j.append(LogOp::Write { blk: 8 }).is_some());
        assert!(j.append(LogOp::Delete { blk: 8 }).is_some());
        assert!(j.append(LogOp::Checkpoint).is_some());
        let (j2, rep) = opened_after_crash(j.into_inner(), 3);
        assert_eq!(rep.entries, 3);
        assert_eq!(rep.torn, 0);
        assert_eq!(j2.applied_seq(), 3);
    }

    #[test]
    fn disk_format_is_idempotent_and_wipes() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 10);
        let mut dev = j.into_inner();
        DiskJournal::format(&mut dev, 3, 0).unwrap();
        let (j2, rep) = DiskJournal::open(dev, 3, 0).unwrap();
        assert_eq!(rep.entries, 0, "重格式化后日志为空");
        assert!(j2.is_empty());
    }

    #[test]
    fn disk_discard_all_keeps_seq_watermark() {
        let mut sim = CrashSim::new(CAP);
        DiskJournal::format(&mut sim, 3, 0).unwrap();
        let mut j = DiskJournal::open(sim, 3, 0).unwrap().0;
        append_n(&mut j, 4);
        assert!(j.discard_all());
        assert_eq!(j.len(), 0);
        let seqs = append_n(&mut j, 2);
        assert_eq!(seqs, vec![5, 6], "discard 后 seq 水位不动");
    }

    #[test]
    fn disk_open_rejects_wrong_block_size() {
        struct WeirdBs;
        impl BlockDevice for WeirdBs {
            fn block_size(&self) -> u32 {
                4096
            }
            fn capacity_blocks(&self) -> u64 {
                256
            }
            fn read_blocks(&mut self, _: u64, _: &mut [u8]) -> Result<(), BlockError> {
                Ok(())
            }
            fn write_blocks(&mut self, _: u64, _: &[u8]) -> Result<(), BlockError> {
                Ok(())
            }
            fn flush(&mut self) -> Result<(), BlockError> {
                Ok(())
            }
        }
        assert_eq!(
            DiskJournal::open(WeirdBs, 3, 0).err(),
            Some(BlockError::Unsupported),
            "仅 512B 块实现（当前实现边界如实声明）"
        );
    }
}
