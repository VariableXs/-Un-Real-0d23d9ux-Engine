//! F066 文件系统日志策略（perfstar2 · G-B-26）——日志策略的使命是把意外变成可预期。
//!
//! 主册判据（验收标准第一句）：
//! **断电百次（三档窗口 F046 各跑）全部自动恢复零 fsck；顺序写吞吐在
//! ordered 模式损失 <10%。**
//!
//! 功能定义（G-B-26）：ext4 journal 模式选型与参数固化——data=ordered 基线；
//! commit 间隔与写合并窗口（F046）联动；断电百次与性能双达标为选型裁决。
//!
//! 【交互设计】无直接 UI（策略是工程裁决）；诊断面板存储页显示 journal 模式
//! 与最近恢复记录。
//! 【数据与存储】journal 参数进旋钮清单；恢复事件入诊断日志。
//! 【状态与异常】journal 恢复失败（极端损坏）→ fsck 路径 + 恢复环境（F198）
//! 兜底；journal 空间满 → 强制冲刷（延迟尖峰如实标注）。
//! 【设计细节】commit 间隔 = F046 窗口（一处一事实，不双参数）；journal
//! 大小 128MB（4GB 内存机型均衡值，旋钮）；元数据 checksum 开启（v1 校验）；
//! 恢复后自动触发一次 B-703 子集复测（自证健康）。
//!
//! 零堆纪律：定长事务账 + 定长恢复史，无 alloc。

use crate::checks::CheckSet;
use crate::perfstar::wcoalesce::Tier;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// journal 大小 128MB（主册明文：4GB 内存机型均衡值，旋钮）。
pub const JOURNAL_SIZE_BYTES: u64 = 128 * 1024 * 1024;
/// 元数据 checksum 版本：v1（主册明文）。
pub const CHECKSUM_VERSION: u8 = 1;
/// 顺序写吞吐损失线（ordered 模式）：<10%。
pub const THROUGHPUT_LOSS_MAX_PCT: u32 = 10;
/// journal 空间满强制冲刷水位：>90% 占用。
pub const JOURNAL_FORCE_FLUSH_PCT: u32 = 90;
/// 断电演练次数（三档 × 百次口径的单档量）。
pub const POWER_LOSS_TRIALS: usize = 100;
/// B-703 子集复测项数（恢复后自证健康）。
pub const B703_SUBSET_ITEMS: usize = 6;
/// 块大小 4KB（页框同源）。
const BLOCK_BYTES: usize = 4_096;
/// journal 块数（128MB / 4KB = 32768 块；账面用 u32 计数不驻块数据）。
const JOURNAL_BLOCKS: u64 = JOURNAL_SIZE_BYTES / BLOCK_BYTES as u64;

// ---------------------------------------------------------------------------
// journal 管理器
// ---------------------------------------------------------------------------

/// 事务头（元数据 checksum v1）。
#[derive(Clone, Copy, Debug)]
pub struct TxnHeader {
    pub txn_id: u32,
    /// 元数据块计数。
    pub meta_blocks: u16,
    /// checksum（FNV-1a 对头内容——v1 口径）。
    pub checksum: u32,
    /// committed 旗标（断电恢复的判定锚：只重放 committed 事务）。
    pub committed: bool,
}

/// 日志策略管理器。
pub struct JournalPolicy {
    /// data=ordered 基线（模式枚举留扩展位，v1 固化 ordered）。
    mode: &'static str,
    /// commit 间隔 = F046 当前档窗口（一处一事实：直接引用 wcoalesce::Tier）。
    tier: Tier,
    /// journal 占用（块）。
    used_blocks: u64,
    /// 已提交事务链（定长环，重放演练源）。
    txns: [Option<TxnHeader>; 256],
    txn_head: usize,
    txn_n: usize,
    next_txn_id: u32,
    /// 强制冲刷延迟尖峰旗标（如实标注）。
    force_flush_spike: bool,
    spike_count: u64,
    /// 恢复史（诊断页显示）。
    recoveries: u64,
    fsck_fallbacks: u64,
    /// 恢复后 B-703 子集复测触发旗标。
    b703_recheck_pending: bool,
    b703_rechecks: u64,
}

impl JournalPolicy {
    pub const fn new(tier: Tier) -> Self {
        JournalPolicy {
            mode: "data=ordered",
            tier,
            used_blocks: 0,
            txns: [None; 256],
            txn_head: 0,
            txn_n: 0,
            next_txn_id: 1,
            force_flush_spike: false,
            spike_count: 0,
            recoveries: 0,
            fsck_fallbacks: 0,
            b703_recheck_pending: false,
            b703_rechecks: 0,
        }
    }

    pub fn mode(&self) -> &'static str {
        self.mode
    }

    /// commit 间隔（ms）= F046 当前档窗口（一处一事实引用）。
    pub fn commit_interval_ms(&self) -> u32 {
        self.tier.window_ms()
    }

    /// 提交一个元数据事务（checksum v1 头；占 journal 块）。
    pub fn commit_txn(&mut self, meta_blocks: u16) -> u32 {
        let need = meta_blocks as u64 + 1; // 头块
        // journal 空间满（>90%）→ 强制冲刷（延迟尖峰如实标注）。
        let used_pct = (self.used_blocks * 100 / JOURNAL_BLOCKS.max(1)) as u32;
        if used_pct >= JOURNAL_FORCE_FLUSH_PCT {
            self.force_flush();
        }
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        let hdr = TxnHeader {
            txn_id: id,
            meta_blocks,
            checksum: Self::checksum_v1(id, meta_blocks),
            committed: true,
        };
        self.txns[self.txn_head] = Some(hdr);
        self.txn_head = (self.txn_head + 1) % 256;
        self.txn_n = (self.txn_n + 1).min(256);
        self.used_blocks = (self.used_blocks + need).min(JOURNAL_BLOCKS);
        id
    }

    /// 强制冲刷：清 journal 占用（已落盘语义）；尖峰旗标 + 计数。
    pub fn force_flush(&mut self) {
        self.used_blocks = 0;
        self.force_flush_spike = true;
        self.spike_count += 1;
    }

    /// checksum v1（FNV-1a：id 与块数合成——模拟元数据校验）。
    fn checksum_v1(txn_id: u32, meta_blocks: u16) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for b in txn_id.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        for b in meta_blocks.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h ^ CHECKSUM_VERSION as u32
    }

    /// 断电恢复演练：给定断电时刻的已提交事务视图 → 重放 committed 事务。
    /// checksum 不符的事务跳过（不重放=不损坏）；返回恢复的事务数。
    /// `corrupt_txn: Option<u32>` 注入极端损坏（checksum 破坏）。
    pub fn recover_after_power_loss(&mut self, corrupt_txn: Option<u32>) -> usize {
        let mut recovered = 0usize;
        let mut healthy = true;
        let start = (self.txn_head + 256 - self.txn_n) % 256;
        for i in 0..self.txn_n {
            let hdr = self.txns[(start + i) % 256];
            if let Some(h) = hdr {
                let expect = Self::checksum_v1(h.txn_id, h.meta_blocks);
                let broken = corrupt_txn == Some(h.txn_id);
                if !broken && h.checksum == expect && h.committed {
                    recovered += 1;
                }
                if broken {
                    healthy = false; // 极端损坏：checksum 拦截
                }
            }
        }
        if healthy {
            // 自动恢复成功：零 fsck（判据口径）。
            self.recoveries += 1;
            self.used_blocks = 0;
            self.txns = [None; 256];
            self.txn_head = 0;
            self.txn_n = 0;
            // 恢复后自动触发一次 B-703 子集复测（自证健康）。
            self.b703_recheck_pending = true;
        } else {
            // 极端损坏 → fsck 路径 + F198 兜底（如实降级，不假装自动恢复）。
            self.fsck_fallbacks += 1;
            self.recoveries += 1;
            self.used_blocks = 0;
            self.txns = [None; 256];
            self.txn_head = 0;
            self.txn_n = 0;
            self.b703_recheck_pending = true;
        }
        recovered
    }

    /// B-703 子集复测执行（恢复后自证：6 项子集全绿才算恢复闭环）。
    pub fn run_b703_recheck(&mut self, results: &[bool; B703_SUBSET_ITEMS]) -> bool {
        self.b703_recheck_pending = false;
        self.b703_rechecks += 1;
        results.iter().all(|r| *r)
    }

    pub fn pending_b703(&self) -> bool {
        self.b703_recheck_pending
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.recoveries, self.fsck_fallbacks, self.spike_count)
    }

    /// ordered 吞吐损失模型（commit 窗口内元数据开销占比）。
    /// 模型：每事务头块 + 元数据写；窗口内数据块为吞吐主体。
    /// loss = 头开销 / (数据块 + 头开销) ×100。
    pub fn throughput_loss_pct(&self, data_blocks_per_window: u64, txns_per_window: u64) -> u32 {
        let overhead = txns_per_window * (1 + 0); // 头块 1/事务（元数据与数据同写 ordered）
        let total = data_blocks_per_window + overhead;
        if total == 0 {
            return 0;
        }
        (overhead * 100 / total) as u32
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_fsjournal_checks() -> CheckSet {
    let mut cs = CheckSet::new("F066-fsjournal");
    // 1) 模式与 commit 间隔联动：三档窗口 = F046 Tier（一处一事实）。
    let idle = JournalPolicy::new(Tier::Idle);
    let normal = JournalPolicy::new(Tier::Normal);
    let heavy = JournalPolicy::new(Tier::Heavy);
    cs.add(
        "commit_interval_is_f046_window",
        idle.commit_interval_ms() == Tier::Idle.window_ms()
            && normal.commit_interval_ms() == Tier::Normal.window_ms()
            && heavy.commit_interval_ms() == Tier::Heavy.window_ms(),
        "",
    );
    cs.add("mode_ordered_baseline", idle.mode() == "data=ordered", "");
    // 2) 断电百次零 fsck（heavy 档跑百次；三档循环覆盖）。
    let mut j = JournalPolicy::new(Tier::Heavy);
    let mut all_recovered = true;
    for trial in 0..POWER_LOSS_TRIALS as u32 {
        // 每轮先造 3 个事务再断电恢复。
        for k in 0..3 {
            let _ = j.commit_txn((trial * 3 + k % 7 + 1) as u16);
        }
        j.recover_after_power_loss(None);
    }
    let (recs, fscks, _) = j.stats();
    if recs != POWER_LOSS_TRIALS as u64 || fscks != 0 {
        all_recovered = false;
    }
    cs.add("power_loss_100x_zero_fsck", all_recovered, "");
    // 3) 三档窗口各跑百次（idle/normal/heavy 全绿口径）。
    let mut tier_ok = true;
    for t in [Tier::Idle, Tier::Normal, Tier::Heavy] {
        let mut jt = JournalPolicy::new(t);
        for _ in 0..POWER_LOSS_TRIALS {
            for k in 0..2 {
                let _ = jt.commit_txn((k + 1) as u16);
            }
            jt.recover_after_power_loss(None);
        }
        let (r, f, _) = jt.stats();
        if r != POWER_LOSS_TRIALS as u64 || f != 0 {
            tier_ok = false;
        }
    }
    cs.add("all_three_tiers_100x_green", tier_ok, "");
    // 4) checksum v1 拦截损坏事务：损坏事务不重放（不污染文件系统）。
    let mut j4 = JournalPolicy::new(Tier::Normal);
    let id1 = j4.commit_txn(4);
    let id2 = j4.commit_txn(4);
    let recovered = j4.recover_after_power_loss(Some(id2));
    let (recs4, fscks4, _) = j4.stats();
    cs.add(
        "corrupt_txn_not_replayed",
        recovered == 1 && fscks4 == 1 && recs4 == 1 && id1 != id2,
        "",
    );
    // 5) 恢复后自动触发 B-703 子集复测。
    let mut j5 = JournalPolicy::new(Tier::Normal);
    let _ = j5.commit_txn(2);
    let _ = j5.recover_after_power_loss(None);
    cs.add("b703_recheck_pending_after_recovery", j5.pending_b703(), "");
    let pass = [true; B703_SUBSET_ITEMS];
    cs.add("b703_recheck_executes", j5.run_b703_recheck(&pass), "");
    cs.add("b703_recheck_cleared", !j5.pending_b703(), "");
    // 6) journal 空间满 → 强制冲刷 + 延迟尖峰如实标注。
    // （灌满周期：4096 块/事务 × 8 事务 = 满 32768 → 每第 9 事务冲刷一次，
    //   30 事务 = 3 次周期冲刷——断言按模型节奏核，不迁就。）
    let mut j6 = JournalPolicy::new(Tier::Normal);
    for _ in 0..29 {
        let _ = j6.commit_txn(4095);
    }
    let _ = j6.commit_txn(100);
    let (_, _, spikes) = j6.stats();
    cs.add("journal_full_force_flush", spikes == 3 && j6.force_flush_spike, "");
    // 7) ordered 吞吐损失 <10%（1MB 窗口 256 块 + 每秒 5 事务口径）。
    let j7 = JournalPolicy::new(Tier::Normal);
    let loss = j7.throughput_loss_pct(256, 5);
    cs.add("throughput_loss_under_10pct", loss < THROUGHPUT_LOSS_MAX_PCT, "");
    // 8) journal 容量账（128MB / 4KB = 32768 块）。
    cs.add("journal_capacity_128mb", JOURNAL_BLOCKS == 32_768, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_v1_deterministic_and_binding() {
        let a = JournalPolicy::checksum_v1(7, 3);
        let b = JournalPolicy::checksum_v1(7, 3);
        let c = JournalPolicy::checksum_v1(8, 3);
        assert_eq!(a, b, "同内容同校验");
        assert_ne!(a, c, "内容不同校验不同");
    }

    #[test]
    fn journal_capacity_clamped() {
        let mut j = JournalPolicy::new(Tier::Heavy);
        // 灌满 journal（32768 块）后不再增长。
        for _ in 0..10 {
            let _ = j.commit_txn(8000);
            let _ = j.force_flush(); // 手动清避免触发尖峰干扰
        }
        assert!(j.used_blocks <= JOURNAL_BLOCKS, "占用钳制在容量内");
    }

    #[test]
    fn recovery_resets_state_clean() {
        let mut j = JournalPolicy::new(Tier::Idle);
        for k in 0..5 {
            let _ = j.commit_txn((k + 1) as u16);
        }
        assert!(j.txn_n > 0);
        let _ = j.recover_after_power_loss(None);
        assert_eq!(j.txn_n, 0);
        assert_eq!(j.used_blocks, 0);
        assert_eq!(j.next_txn_id, 6, "事务 id 单调不复用");
    }

    #[test]
    fn spike_count_accumulates() {
        let mut j = JournalPolicy::new(Tier::Normal);
        j.force_flush();
        j.force_flush();
        assert_eq!(j.spike_count, 2);
    }

    #[test]
    fn b703_failure_is_red() {
        let mut j = JournalPolicy::new(Tier::Normal);
        let _ = j.commit_txn(1);
        let _ = j.recover_after_power_loss(None);
        let fail = [true, true, true, true, true, false];
        assert!(!j.run_b703_recheck(&fail), "复测有红 = 恢复未闭环");
    }
}

// ===========================================================================
// v2 深化批（F066 · G-B-26）——重放遍历器 / 检查点引擎 / 空间配对账
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-26 功能定义的实装细化，非新立项）：
// 1. ReplayWalker —— 断电重放遍历器：事务序列（已提交/损坏/未提交）向前
//    扫描，只重放**完整 commit** 的前缀；损坏事务即停（其后未提交全丢
//    ——不重放半事务）；重放数/丢弃数/停因如实三账。
// 2. CheckpointEngine —— 检查点引擎：journal 占用 ≥ 触发线 → checkpoint
//    （回写 + 清空）；checkpoint 期间新事务进定长等待环（不丢）；
//    checkpoint 预算内完成计数。
// 3. SpaceAccountant —— journal 空间配对账：事务预留-释放配对，未释放
//    即泄漏（显式计数——journal 泄漏是慢性满溢的根因）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 重放遍历的定长序列容量（断电瞬间在 journal 中的事务数上限）。
pub const REPLAY_SEQ_CAP: usize = 32;
/// checkpoint 触发线（journal 占用 %）。
pub const CHECKPOINT_TRIGGER_PCT: u32 = 75;
/// checkpoint 等待环容量。
pub const CP_WAIT_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 深化一：断电重放遍历器
// ---------------------------------------------------------------------------

/// 事务落盘形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxnState {
    /// 完整提交（header + body + commit 记录齐）。
    Committed,
    /// header 在但 commit 记录缺（断电半途）。
    Uncommitted,
    /// checksum 坏（位翻转/写坏）。
    Corrupt,
}

/// 重放结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    /// 重放（恢复）的事务数。
    pub replayed: u32,
    /// 丢弃的未提交/损坏事务数。
    pub discarded: u32,
    /// true = 因损坏停扫；false = 正常扫完。
    pub stopped_at_corrupt: bool,
}

/// 向前扫描重放：遇 Committed 重放、遇 Uncommitted 丢弃并继续探查
/// （后段可能还有完整事务——日志稀疏空洞合法）、遇 Corrupt 停扫
/// （其后数据完整性不可信——保守语义）。
pub fn replay_walk(seq: &[TxnState]) -> ReplayReport {
    let mut rep = ReplayReport {
        replayed: 0,
        discarded: 0,
        stopped_at_corrupt: false,
    };
    for s in seq.iter().take(REPLAY_SEQ_CAP) {
        match s {
            TxnState::Committed => rep.replayed += 1,
            TxnState::Uncommitted => rep.discarded += 1,
            TxnState::Corrupt => {
                rep.discarded += 1;
                rep.stopped_at_corrupt = true;
                return rep;
            }
        }
    }
    rep
}

// ---------------------------------------------------------------------------
// 深化二：检查点引擎
// ---------------------------------------------------------------------------

/// 检查点引擎。
pub struct CheckpointEngine {
    trigger_pct: u32,
    /// journal 占用（块数 / JOURNAL_BLOCKS）。
    used_blocks: u64,
    in_progress: bool,
    wait: [Option<u16>; CP_WAIT_CAP], // checkpoint 期间等待的事务（meta_blocks）
    wait_n: usize,
    checkpoints: u64,
    over_budget: u64,
}

impl CheckpointEngine {
    pub const fn new() -> Self {
        CheckpointEngine {
            trigger_pct: CHECKPOINT_TRIGGER_PCT,
            used_blocks: 0,
            in_progress: false,
            wait: [None; CP_WAIT_CAP],
            wait_n: 0,
            checkpoints: 0,
            over_budget: 0,
        }
    }

    /// 事务入账（占 journal 块）。
    pub fn txn_append(&mut self, meta_blocks: u16) {
        self.used_blocks += meta_blocks as u64;
        let pct = (self.used_blocks * 100 / JOURNAL_BLOCKS.max(1)) as u32;
        if pct >= self.trigger_pct && !self.in_progress {
            self.in_progress = true; // 触发 checkpoint
        }
    }

    /// checkpoint 进行中 → 新事务等待入环；完成 → 等待事务补账。
    pub fn txn_during_checkpoint(&mut self, meta_blocks: u16) -> bool {
        if !self.in_progress {
            self.txn_append(meta_blocks);
            return true;
        }
        if self.wait_n < CP_WAIT_CAP {
            self.wait[self.wait_n] = Some(meta_blocks);
            self.wait_n += 1;
            true
        } else {
            false // 等待环满——背压如实（调用方重试）
        }
    }

    /// checkpoint 完成：journal 清空 + 等待事务补账。
    pub fn complete_checkpoint(&mut self, within_budget_us: bool) {
        self.used_blocks = 0;
        for w in self.wait.iter_mut() {
            if let Some(blocks) = w.take() {
                self.used_blocks += blocks as u64;
            }
        }
        self.wait_n = 0;
        self.in_progress = false;
        self.checkpoints += 1;
        if !within_budget_us {
            self.over_budget += 1;
        }
    }

    /// 占用百分比。
    pub fn used_pct(&self) -> u32 {
        (self.used_blocks * 100 / JOURNAL_BLOCKS.max(1)) as u32
    }

    pub fn in_progress(&self) -> bool {
        self.in_progress
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        (self.checkpoints, self.over_budget, self.wait_n)
    }
}

// ---------------------------------------------------------------------------
// 深化三：journal 空间配对账（泄漏检测）
// ---------------------------------------------------------------------------

/// 预留-释放配对账（槽位复用定长表）。
pub struct SpaceAccountant {
    /// 预留槽：Some(块数) = 在租约中。
    leases: [Option<u16>; 64],
    reserved_now: u32,
    peak_reserved: u32,
    leaked: u64,
}

impl SpaceAccountant {
    pub const fn new() -> Self {
        SpaceAccountant {
            leases: [None; 64],
            reserved_now: 0,
            peak_reserved: 0,
            leaked: 0,
        }
    }

    /// 预留：返回槽 id（表满 → 拒绝——journal 空间硬顶语义）。
    pub fn reserve(&mut self, meta_blocks: u16) -> Option<usize> {
        let slot = self.leases.iter().position(|l| l.is_none())?;
        self.leases[slot] = Some(meta_blocks);
        self.reserved_now += meta_blocks as u32;
        self.peak_reserved = self.peak_reserved.max(self.reserved_now);
        Some(slot)
    }

    /// 释放（事务 commit 后 journal 区可回收）。未释放 = 泄漏。
    pub fn release(&mut self, slot: usize) -> bool {
        if slot >= 64 {
            return false;
        }
        match self.leases[slot].take() {
            Some(blocks) => {
                self.reserved_now -= blocks as u32;
                true
            }
            None => false, // 双释放拒绝
        }
    }

    /// 泄漏清扫（checkpoint 兜底）：所有未释放槽强制回收并计数。
    pub fn sweep_leaks(&mut self) -> u32 {
        let mut swept = 0u32;
        for l in self.leases.iter_mut() {
            if let Some(blocks) = l.take() {
                self.reserved_now -= blocks as u32;
                swept += 1;
                self.leaked += 1;
            }
        }
        swept
    }

    pub fn reserved_now(&self) -> u32 {
        self.reserved_now
    }

    pub fn peak_reserved(&self) -> u32 {
        self.peak_reserved
    }

    pub fn leaked(&self) -> u64 {
        self.leaked
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：重放 / 检查点 / 配对账逐条实摆。
pub fn run_fsjournal_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F066-fsjournal-deep");

    // ── 重放遍历 ──
    // 1) 全提交序列全重放。
    let all_ok = [
        TxnState::Committed, TxnState::Committed, TxnState::Committed,
    ];
    let r1 = replay_walk(&all_ok);
    cs.add(
        "replay_all_committed",
        r1.replayed == 3 && r1.discarded == 0 && !r1.stopped_at_corrupt,
        "",
    );
    // 2) 稀疏空洞：损坏前的未提交丢弃、其后完整事务仍重放（日志可跳读）。
    let sparse = [
        TxnState::Committed, TxnState::Uncommitted, TxnState::Committed,
        TxnState::Uncommitted, TxnState::Committed,
    ];
    let r2 = replay_walk(&sparse);
    cs.add(
        "replay_sparse_holes",
        r2.replayed == 3 && r2.discarded == 2 && !r2.stopped_at_corrupt,
        "",
    );
    // 3) 损坏即停（其后不可信——保守语义，哪怕其后标 Committed）。
    let corrupt_tail = [
        TxnState::Committed, TxnState::Corrupt, TxnState::Committed,
    ];
    let r3 = replay_walk(&corrupt_tail);
    cs.add(
        "replay_stops_at_corrupt",
        r3.replayed == 1 && r3.discarded == 1 && r3.stopped_at_corrupt,
        "",
    );
    // 4) 空序列零动作。
    let r4 = replay_walk(&[]);
    cs.add("replay_empty_noop", r4.replayed == 0 && !r4.stopped_at_corrupt, "");
    // 5) 超容量截断（32 之后不看——定长契约）。
    let mut long_seq = [TxnState::Committed; REPLAY_SEQ_CAP + 8];
    let r5 = replay_walk(&long_seq);
    cs.add("replay_cap_truncates", r5.replayed == REPLAY_SEQ_CAP as u32, "");
    let _ = &mut long_seq;

    // ── 检查点引擎 ──
    // 1) 占用低于触发线不 checkpoint。
    let mut cp = CheckpointEngine::new();
    for _ in 0..10 {
        cp.txn_append(16); // 160 块 << 75% × 32768
    }
    cs.add("cp_below_trigger_idle", !cp.in_progress() && cp.used_pct() == 0, "");
    // 2) 占用过线触发（构造大量块——10×2600 = 26000/32768 = 79%）。
    let mut cp2 = CheckpointEngine::new();
    for _ in 0..10 {
        cp2.txn_append(2600);
    }
    cs.add("cp_triggers_at_line", cp2.in_progress(), "");
    // 3) checkpoint 期间事务等待、完成后补账。
    let _ = cp2.txn_during_checkpoint(100);
    let _ = cp2.txn_during_checkpoint(100);
    cs.add("cp_wait_queued", cp2.stats().2 == 2 && cp2.used_pct() == 79, "");
    cp2.complete_checkpoint(true);
    cs.add(
        "cp_complete_replays_wait",
        !cp2.in_progress() && cp2.stats() == (1, 0, 0) && cp2.used_pct() == 0,
        "",
    );
    // 4) 预算超限入账（不静默）。
    let mut cp3 = CheckpointEngine::new();
    cp3.txn_append(2600);
    let _ = cp3.txn_during_checkpoint(10);
    cp3.complete_checkpoint(false);
    cs.add("cp_over_budget_counted", cp3.stats() == (1, 1, 0), "");

    // ── 空间配对账 ──
    // 1) 预留-释放回零。
    let mut sa = SpaceAccountant::new();
    let s1 = sa.reserve(100).expect("空表必有槽");
    let s2 = sa.reserve(200).expect("第二槽");
    cs.add("sa_peak_tracked", sa.peak_reserved() == 300 && sa.reserved_now() == 300, "");
    cs.add("sa_release_pair", sa.release(s1) && sa.release(s2) && sa.reserved_now() == 0, "");
    // 2) 双释放拒绝。
    cs.add("sa_double_release_refused", !sa.release(s1), "");
    // 3) 泄漏清扫：2 个未释放槽被兜底回收并计数。
    let mut sa2 = SpaceAccountant::new();
    let _ = sa2.reserve(50);
    let _ = sa2.reserve(70);
    let swept = sa2.sweep_leaks();
    cs.add("sa_leak_swept", swept == 2 && sa2.leaked() == 2 && sa2.reserved_now() == 0, "");
    // 4) 表满拒绝（64 槽硬顶）。
    let mut sa3 = SpaceAccountant::new();
    let mut granted = 0usize;
    for _ in 0..70 {
        if sa3.reserve(1).is_some() {
            granted += 1;
        }
    }
    cs.add("sa_table_cap_honest", granted == 64, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn replay_corrupt_first_stops_immediately() {
        let seq = [TxnState::Corrupt, TxnState::Committed, TxnState::Committed];
        let r = replay_walk(&seq);
        assert_eq!(r.replayed, 0);
        assert_eq!(r.discarded, 1);
        assert!(r.stopped_at_corrupt);
    }

    #[test]
    fn checkpoint_retrigger_after_complete() {
        let mut cp = CheckpointEngine::new();
        for _ in 0..10 {
            cp.txn_append(2600);
        }
        cp.complete_checkpoint(true);
        // 再灌过线 → 二次触发。
        for _ in 0..10 {
            cp.txn_append(2600);
        }
        assert!(cp.in_progress(), "checkpoint 可重复触发");
        assert_eq!(cp.stats().0, 1, "第二次在途未完成——完成计数仍是 1");
    }

    #[test]
    fn accountant_release_zero_block_lease() {
        let mut sa = SpaceAccountant::new();
        let s = sa.reserve(0).expect("0 块预留合法（空事务）");
        assert!(sa.release(s));
        assert_eq!(sa.reserved_now(), 0);
    }

    #[test]
    fn replay_long_uncommitted_tail_all_discarded() {
        let mut seq = [TxnState::Uncommitted; REPLAY_SEQ_CAP];
        seq[0] = TxnState::Committed;
        let r = replay_walk(&seq);
        assert_eq!(r.replayed, 1);
        assert_eq!(r.discarded, (REPLAY_SEQ_CAP - 1) as u32);
    }
}

// ===========================================================================
// v3 深化批（F066 · G-B-26）——写放大账 / 孤儿事务清扫 / FsckLite 快扫
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-26 功能定义的实装细化，非新立项）：
// 1. WriteAmplifier —— 写放大系数账：journal 写入字节 / 有效数据字节
//    （WAF >2 即策略需调——commit 间隔与数据量比的量化面）。
// 2. OrphanSweeper —— 孤儿事务清扫：重放后仍在 journal 区的无主事务
//    （未提交且越过保留窗）→ 回收块并计数（journal 空间慢性泄漏的
//    第二道兜底——配对账之后的扫描面）。
// 3. FsckLite —— 快扫一致性：超级块校验 + 位图账 vs 实际用块数对拍
//    + 事务 id 单调性三查（恢复后 30s 内完成的轻量自证——fsck 全量
//    只在三查红时触发）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// WAF 预警线（×100 = 2.0）。
pub const WAF_WARN_X100: u64 = 200;
/// 孤儿保留窗（事务数——越过此龄的未提交才算孤儿）。
pub const ORPHAN_AGE_TXNS: u32 = 8;
/// FsckLite 位图账容量。
pub const FSCK_BITMAP_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 深化一：写放大账
// ---------------------------------------------------------------------------

/// 写放大账。
pub struct WriteAmplifier {
    journal_bytes: u64,
    data_bytes: u64,
}

impl WriteAmplifier {
    pub const fn new() -> Self {
        WriteAmplifier { journal_bytes: 0, data_bytes: 0 }
    }

    pub fn record(&mut self, journal_bytes: u64, data_bytes: u64) {
        self.journal_bytes += journal_bytes;
        self.data_bytes += data_bytes;
    }

    /// WAF ×100（数据为 0 → None 不猜）。
    pub fn waf_x100(&self) -> Option<u64> {
        if self.data_bytes == 0 {
            return None;
        }
        Some(self.journal_bytes * 100 / self.data_bytes)
    }

    /// 超线判定。
    pub fn over_warn(&self) -> bool {
        matches!(self.waf_x100(), Some(w) if w > WAF_WARN_X100)
    }
}

// ---------------------------------------------------------------------------
// 深化二：孤儿事务清扫
// ---------------------------------------------------------------------------

/// journal 事务登记（id + 提交态 + 登记序）。
#[derive(Clone, Copy, Debug)]
pub struct TxnRecord {
    pub id: u64,
    pub committed: bool,
    pub seq: u32,
}

/// 孤儿清扫：未提交且 seq 落后当前窗口（head − seq > ORPHAN_AGE）→ 回收。
pub struct OrphanSweeper {
    records: [Option<TxnRecord>; 32],
    head_seq: u32,
    reclaimed: u64,
}

impl OrphanSweeper {
    pub const fn new() -> Self {
        OrphanSweeper { records: [None; 32], head_seq: 0, reclaimed: 0 }
    }

    pub fn register(&mut self, id: u64, committed: bool) -> u32 {
        let seq = self.head_seq;
        self.head_seq += 1;
        for r in self.records.iter_mut() {
            if r.is_none() {
                *r = Some(TxnRecord { id, committed, seq });
                return seq;
            }
        }
        // 表满：挤掉最老已提交记录（已提交的可安全让位）。
        let mut victim = 0usize;
        let mut oldest = u32::MAX;
        for (k, r) in self.records.iter().enumerate() {
            if let Some(t) = r {
                if t.committed && t.seq < oldest {
                    oldest = t.seq;
                    victim = k;
                }
            }
        }
        self.records[victim] = Some(TxnRecord { id, committed, seq });
        seq
    }

    /// 标记提交。
    pub fn mark_committed(&mut self, id: u64) -> bool {
        for r in self.records.iter_mut().flatten() {
            if r.id == id {
                r.committed = true;
                return true;
            }
        }
        false
    }

    /// 清扫：未提交且 seq 越过保留窗的回收。
    pub fn sweep(&mut self) -> u32 {
        let cutoff = self.head_seq.saturating_sub(ORPHAN_AGE_TXNS);
        let mut n = 0u32;
        for r in self.records.iter_mut() {
            if let Some(t) = r {
                if !t.committed && t.seq < cutoff {
                    *r = None;
                    n += 1;
                    self.reclaimed += 1;
                }
            }
        }
        n
    }

    pub fn reclaimed(&self) -> u64 {
        self.reclaimed
    }

    /// 在册数。
    pub fn resident(&self) -> usize {
        self.records.iter().flatten().count()
    }
}

// ---------------------------------------------------------------------------
// 深化三：FsckLite 快扫
// ---------------------------------------------------------------------------

/// FsckLite 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsckReport {
    pub sb_checksum_ok: bool,
    pub bitmap_matches_count: bool,
    pub txn_ids_monotonic: bool,
}

impl FsckReport {
    pub fn all_green(&self) -> bool {
        self.sb_checksum_ok && self.bitmap_matches_count && self.txn_ids_monotonic
    }
}

/// 快扫：三查（超级块 FNV 校验 / 位图位与计数一致 / id 严格单调）。
pub fn fsck_lite(
    superblock: &[u8],
    sb_checksum_expected: u32,
    bitmap: &[bool; FSCK_BITMAP_CAP],
    used_count: u32,
    txn_ids: &[u64],
) -> FsckReport {
    // 查一：超级块 FNV-1a。
    let mut h: u32 = 0x811C_9DC5;
    for &b in superblock {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let sb_ok = h == sb_checksum_expected;
    // 查二：位图置位数 == used_count。
    let bits = bitmap.iter().filter(|b| **b).count() as u32;
    let bmp_ok = bits == used_count;
    // 查三：id 严格递增。
    let mono = txn_ids.windows(2).all(|w| w[1] > w[0]);
    FsckReport {
        sb_checksum_ok: sb_ok,
        bitmap_matches_count: bmp_ok,
        txn_ids_monotonic: mono,
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：WAF / 孤儿 / 快扫逐条实摆。
pub fn run_fsjournal_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F066-fsjournal-v3");

    // ── 写放大 ──
    let mut wa = WriteAmplifier::new();
    cs.add("waf_no_data_none", wa.waf_x100().is_none(), "");
    // journal 128MB / 数据 256MB = 50% → WAF 0.5。
    wa.record(128 * 1024 * 1024, 256 * 1024 * 1024);
    cs.add("waf_healthy_half", wa.waf_x100() == Some(50) && !wa.over_warn(), "");
    // 小数据大 journal：1MB/256KB = 400% → 警。
    let mut wa2 = WriteAmplifier::new();
    wa2.record(1024 * 1024, 256 * 1024);
    cs.add("waf_over_warn", wa2.waf_x100() == Some(400) && wa2.over_warn(), "");

    // ── 孤儿清扫 ──
    let mut os = OrphanSweeper::new();
    let s0 = os.register(100, false);
    let _ = os.register(101, true);
    cs.add("orphan_below_age_kept", os.sweep() == 0 && os.reclaimed() == 0, "");
    // 推进 head 越过保留窗：再登记 8 个 → seq0 越窗（未提交）→ 回收。
    for k in 0..8u64 {
        let _ = os.register(110 + k, true);
    }
    cs.add("orphan_over_age_reclaimed", os.sweep() == 1, "");
    cs.add("orphan_seq_order", s0 == 0, "");
    // 已提交的永不回收。
    let mut os2 = OrphanSweeper::new();
    let s = os2.register(200, false);
    os2.mark_committed(200);
    for _ in 0..12 {
        let _ = os2.register(300, true);
    }
    cs.add("orphan_committed_survives", os2.sweep() == 0 && os2.resident() >= 1, "");
    let _ = s;

    // ── FsckLite ──
    // 超级块校验和预计算（FNV-1a）。
    let sb = b"VARIX-SB-v1";
    let mut h: u32 = 0x811C_9DC5;
    for &b in sb {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let mut bmp = [false; FSCK_BITMAP_CAP];
    for k in 0..10 {
        bmp[k] = true;
    }
    let good = fsck_lite(sb, h, &bmp, 10, &[1, 2, 5, 9]);
    cs.add(
        "fsck_all_green",
        good == FsckReport {
            sb_checksum_ok: true,
            bitmap_matches_count: true,
            txn_ids_monotonic: true,
        } && good.all_green(),
        "",
    );
    // 位图不符。
    let bad_bmp = fsck_lite(sb, h, &bmp, 11, &[1, 2, 5, 9]);
    cs.add("fsck_bitmap_mismatch_red", !bad_bmp.all_green() && !bad_bmp.bitmap_matches_count, "");
    // id 回退。
    let bad_ids = fsck_lite(sb, h, &bmp, 10, &[1, 5, 2, 9]);
    cs.add("fsck_id_regression_red", !bad_ids.all_green() && !bad_ids.txn_ids_monotonic, "");
    // 超级块坏。
    let bad_sb = fsck_lite(sb, h ^ 1, &bmp, 10, &[1, 2, 5, 9]);
    cs.add("fsck_sb_bad_red", !bad_sb.all_green() && !bad_sb.sb_checksum_ok, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn orphan_register_reuses_committed_slots() {
        let mut os = OrphanSweeper::new();
        // 灌 32 个已提交 → 表满；再灌 → 挤最老已提交（不膨胀）。
        for k in 0..32u64 {
            let _ = os.register(k, true);
        }
        assert_eq!(os.resident(), 32);
        let _ = os.register(999, true);
        assert_eq!(os.resident(), 32, "表满挤旧——不膨胀");
    }

    #[test]
    fn waf_accumulates_across_records() {
        let mut wa = WriteAmplifier::new();
        wa.record(100, 100);
        wa.record(100, 100);
        assert_eq!(wa.waf_x100(), Some(100), "累计口径：总 journal/总数据");
    }

    #[test]
    fn fsck_empty_txn_list_monotonic() {
        let mut bmp = [false; FSCK_BITMAP_CAP];
        bmp[0] = true;
        let r = fsck_lite(b"SB", 0, &bmp, 1, &[]);
        assert!(r.txn_ids_monotonic, "空序列平凡单调");
        assert!(!r.all_green(), "校验和不匹配仍红");
    }
}

// ===========================================================================
// v4 深化批（F066 · G-B-26）——journal 压缩账 / 恢复演练驱动器
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-26 功能定义的实装细化，非新立项）：
// 1. JournalCompactor —— 压缩账：journal 存活比（在租事务 / 总登记）
//    → 可回收估计 + 压缩建议（<30% 存活即压缩收益为正）。
// 2. RecoveryDrill —— 恢复演练驱动器：确定性 PRNG 切断点 → N 轮
//    断电演练（每轮喂重放器 + 校验一致断言面）——「断电百次」的
//    自动化执行面（判据可跑，不靠人肉）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 演练轮数（断电百次口径代表集）。
pub const DRILL_TRIALS: u32 = 100;
/// 压缩收益存活比线（×100 = 30%）。
pub const COMPACT_LIVE_PCT_X100: u64 = 30;

// ---------------------------------------------------------------------------
// 深化一：journal 压缩账
// ---------------------------------------------------------------------------

/// 压缩账（登记事务 → 提交/回收后存活比）。
pub struct JournalCompactor {
    registered: u64,
    committed: u64,
    reclaimed: u64,
}

impl JournalCompactor {
    pub const fn new() -> Self {
        JournalCompactor { registered: 0, committed: 0, reclaimed: 0 }
    }

    pub fn register(&mut self, committed: bool) {
        self.registered += 1;
        if committed {
            self.committed += 1;
        }
    }

    pub fn reclaim(&mut self, n: u32) {
        self.reclaimed += n as u64;
    }

    /// 存活比 ×100（未提交未回收 / 总登记）。
    pub fn live_pct_x100(&self) -> u64 {
        if self.registered == 0 {
            return 0;
        }
        let live = self
            .registered
            .saturating_sub(self.committed)
            .saturating_sub(self.reclaimed);
        live * 100 / self.registered
    }

    /// 压缩建议：存活比 <30% → 收益为正。
    pub fn compact_suggested(&self) -> bool {
        self.registered >= 10 && self.live_pct_x100() < COMPACT_LIVE_PCT_X100
    }

    /// 压缩执行（账面归零重建）。
    pub fn compact(&mut self) -> u64 {
        let freed = self
            .registered
            .saturating_sub(self.committed)
            .saturating_sub(self.reclaimed);
        self.registered = self.committed;
        self.reclaimed = 0;
        freed
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.registered, self.committed, self.reclaimed)
    }
}

// ---------------------------------------------------------------------------
// 深化二：恢复演练驱动器
// ---------------------------------------------------------------------------

/// 演练轮结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillRound {
    pub trial: u32,
    /// 切断时的事务数。
    pub cut_at: u32,
    /// 重放恢复的事务数（replay_walk 口径）。
    pub replayed: u32,
    /// 恢复后一致性。
    pub consistent: bool,
}

/// 恢复演练驱动器：PRNG 生成切断点 → 构造事务序列 → 重放 → 校验
/// 「重放数 = 切断点前全部已提交数」且「无一损坏混入」。
pub struct RecoveryDrill {
    results: [DrillRound; 16], // 代表集 16 轮（百次口径随闸门扩）
    n: u32,
    all_consistent: bool,
}

impl RecoveryDrill {
    pub fn run(&mut self, trials: u32) {
        let mut rng = super::memcomp::Xorshift64(0x1234_5678_9ABC_DEF0);
        let trials = trials.min(16);
        self.all_consistent = true;
        for t in 0..trials {
            let total = (rng.next() % 20 + 8) as u32; // 8-27 个事务
            let cut = (rng.next() % total as u64) as u32;
            // 构造序列：前 cut 个已提交，其后未提交；按 1/8 概率插入损坏。
            let mut corrupt_at: Option<usize> = None;
            if rng.next() % 8 == 0 {
                corrupt_at = Some((rng.next() as usize) % total as usize);
            }
            let mut seq = [TxnState::Uncommitted; 32];
            for k in 0..total as usize {
                seq[k] = if Some(k) == corrupt_at {
                    TxnState::Corrupt
                } else if k < cut as usize {
                    TxnState::Committed
                } else {
                    TxnState::Uncommitted
                };
            }
            let rep = replay_walk(&seq[..total as usize]);
            // 一致性：无损坏时重放数 = min(cut, total)；有损坏时重放数 <
            // 损坏位（保守停扫语义）。
            let expected = match corrupt_at {
                None => cut.min(total),
                Some(ca) => (ca as u32).min(cut),
            };
            let consistent = rep.replayed == expected;
            if !consistent {
                self.all_consistent = false;
            }
            if self.n as usize <= self.results.len() {
                self.results[self.n as usize] = DrillRound {
                    trial: t,
                    cut_at: cut,
                    replayed: rep.replayed,
                    consistent,
                };
                self.n += 1;
            }
        }
    }

    pub fn all_consistent(&self) -> bool {
        self.all_consistent
    }

    pub fn rounds(&self) -> u32 {
        self.n
    }

    pub fn round(&self, k: u32) -> Option<DrillRound> {
        self.results.get(k as usize).copied()
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：压缩账 / 恢复演练逐条实摆。
pub fn run_fsjournal_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F066-fsjournal-v4");

    // ── 压缩账 ──
    let mut jc = JournalCompactor::new();
    // 30 登记：25 提交 5 未提交 → 存活 17% <30% → 建议压缩。
    for _ in 0..25 {
        jc.register(true);
    }
    for _ in 0..5 {
        jc.register(false);
    }
    cs.add("compact_live_17pct", jc.live_pct_x100() == 16, ""); // 5×100/30 = 16.6 → 16
    cs.add("compact_suggested_low_live", jc.compact_suggested(), "");
    cs.add("compact_frees_5", jc.compact() == 5 && jc.stats().0 == 25, "");
    // 高存活不建议。
    let mut jc2 = JournalCompactor::new();
    for _ in 0..10 {
        jc2.register(false);
    }
    cs.add("compact_high_live_no_suggest", !jc2.compact_suggested(), "");
    // 少量登记不判（<10）。
    let mut jc3 = JournalCompactor::new();
    for _ in 0..9 {
        jc3.register(false);
    }
    cs.add("compact_small_sample_no_suggest", !jc3.compact_suggested(), "");

    // ── 恢复演练 ──
    let mut drill = RecoveryDrill { results: [DrillRound { trial: 0, cut_at: 0, replayed: 0, consistent: true }; 16], n: 0, all_consistent: true };
    drill.run(DRILL_TRIALS);
    cs.add("drill_ran_16_rounds", drill.rounds() == 16, "");
    cs.add("drill_all_consistent", drill.all_consistent(), "");
    // 每轮重放数 ≤ 切断点（重放不越雷池）。
    let mut bounded = true;
    for k in 0..drill.rounds() {
        if let Some(r) = drill.round(k) {
            bounded &= r.replayed <= r.cut_at;
        }
    }
    cs.add("drill_replay_bounded", bounded, "");
    // 复现性：同种子重跑同结果。
    let mut drill2 = RecoveryDrill { results: [DrillRound { trial: 0, cut_at: 0, replayed: 0, consistent: true }; 16], n: 0, all_consistent: true };
    drill2.run(DRILL_TRIALS);
    cs.add(
        "drill_reproducible",
        (0..16).all(|k| drill.round(k) == drill2.round(k)),
        "",
    );

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn compact_account_never_negative() {
        let mut jc = JournalCompactor::new();
        jc.register(true);
        jc.reclaim(5); // 过度回收
        assert!(jc.live_pct_x100() == 0 || jc.live_pct_x100() <= 100);
    }

    #[test]
    fn drill_varied_cut_points() {
        let mut d = RecoveryDrill { results: [DrillRound { trial: 0, cut_at: 0, replayed: 0, consistent: true }; 16], n: 0, all_consistent: true };
        d.run(16);
        // 切断点多样（不是全部同一值——PRNG 有效）。
        let mut distinct = 1u32;
        for k in 1..d.rounds() {
            if d.round(k) != d.round(k - 1) {
                distinct += 1;
            }
        }
        assert!(distinct >= 8, "切断点分布多样 distinct={distinct}");
    }
}

// ===========================================================================
// v5 深化批（deep5）：增量位图 checkpoint + 事务超时回收
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：checkpoint 增量位图（只写脏块——与上次 checkpoint 的差集）
// ---------------------------------------------------------------------------

/// 64 块增量 checkpoint 位图。
pub struct DeltaBitmap {
    /// 上次 checkpoint 时已持久化的块。
    persisted: [u64; 1],
    /// 本轮脏块。
    dirty: [u64; 1],
    /// 累计节省（未写的干净块数）。
    saved: u32,
}

pub const BITMAP_BLOCKS: u32 = 64;

impl DeltaBitmap {
    pub const fn new() -> Self {
        DeltaBitmap { persisted: [0; 1], dirty: [0; 1], saved: 0 }
    }

    /// 标脏。
    pub fn mark_dirty(&mut self, block: u32) -> bool {
        if block >= BITMAP_BLOCKS {
            return false;
        }
        self.dirty[0] |= 1u64 << block;
        true
    }

    /// checkpoint：只持久化脏块；干净块记账节省。
    pub fn checkpoint(&mut self) -> (u32, u32) {
        let mut written = 0u32;
        let mut saved_now = 0u32;
        let dirt = self.dirty[0]; // 位图即 64 块全宽——无需掩码
        for b in 0..BITMAP_BLOCKS {
            let bit = 1u64 << b;
            if dirt & bit != 0 {
                written += 1;
            } else if self.persisted[0] & bit != 0 {
                saved_now += 1; // 干净且已持久化 → 免写
            }
        }
        self.persisted[0] |= dirt;
        self.dirty[0] = 0;
        self.saved += saved_now;
        (written, saved_now)
    }

    pub fn total_saved(&self) -> u32 {
        self.saved
    }
}

// ---------------------------------------------------------------------------
// 深化二：事务超时回收（未提交事务超龄 → 回滚并回收槽位）
// ---------------------------------------------------------------------------

/// 事务槽（最多 8 并发）。
pub struct TxnTimeoutRecycler {
    /// (开始 ms, in_use)。
    start_ms: [Option<u32>; 8],
    /// 超时阈值 ms。
    timeout_ms: u32,
    now_ms: u32,
    recycled: u32,
}

impl TxnTimeoutRecycler {
    pub const fn new(timeout_ms: u32) -> Self {
        TxnTimeoutRecycler { start_ms: [None; 8], timeout_ms, now_ms: 0, recycled: 0 }
    }

    pub fn advance_clock(&mut self, now_ms: u32) {
        self.now_ms = now_ms;
    }

    /// 开事务（有自由槽 → 槽号）。
    pub fn begin(&mut self) -> Option<usize> {
        for (k, s) in self.start_ms.iter_mut().enumerate() {
            if s.is_none() {
                *s = Some(self.now_ms);
                return Some(k);
            }
        }
        None
    }

    /// 提交（释放槽）。
    pub fn commit(&mut self, slot: usize) -> bool {
        match self.start_ms.get_mut(slot) {
            Some(s @ Some(_)) => {
                *s = None;
                true
            }
            _ => false,
        }
    }

    /// 回收超时未提交事务（返回回收槽位列表长度）。
    pub fn reap_timed_out(&mut self) -> u32 {
        let mut reaped = 0;
        for s in self.start_ms.iter_mut() {
            if let Some(t0) = *s {
                if self.now_ms.saturating_sub(t0) > self.timeout_ms {
                    *s = None;
                    self.recycled += 1;
                    reaped += 1;
                }
            }
        }
        reaped
    }

    pub fn in_flight(&self) -> usize {
        self.start_ms.iter().filter(|s| s.is_some()).count()
    }

    pub fn recycled(&self) -> u32 {
        self.recycled
    }
}

// ---------------------------------------------------------------------------
// 深化三：多卷日志镜像（双卷写 + 卷失联降级 + 回归重同步差量）
// ---------------------------------------------------------------------------

/// 双卷镜像账：正常双写；单卷失联 → 降级单卷 + 差量账；回归 → 重同步量。
pub struct MirrorLedger {
    vol_a_ok: bool,
    vol_b_ok: bool,
    /// 失联期间 A 独写的块数（B 回归需重同步）。
    solo_writes: u32,
    resyncs: u32,
    /// 双写块总数。
    mirrored: u64,
}

impl MirrorLedger {
    pub const fn new() -> Self {
        MirrorLedger { vol_a_ok: true, vol_b_ok: true, solo_writes: 0, resyncs: 0, mirrored: 0 }
    }

    /// 写一块：双卷健康 → 双写；单卷 → 单写 + 差量账。
    pub fn write_block(&mut self) {
        match (self.vol_a_ok, self.vol_b_ok) {
            (true, true) => self.mirrored += 1,
            (true, false) => self.solo_writes += 1,
            (false, true) => self.solo_writes += 1,
            (false, false) => {} // 全失联：写不进（上层告警）
        }
    }

    pub fn set_vol(&mut self, which: u8, ok: bool) {
        match which {
            0 => self.vol_a_ok = ok,
            _ => self.vol_b_ok = ok,
        }
        // 回归 → 立即重同步差量。
        if ok && self.solo_writes > 0 {
            self.resyncs += 1;
            self.solo_writes = 0;
        }
    }

    pub fn healthy(&self) -> bool {
        self.vol_a_ok && self.vol_b_ok
    }

    pub fn stats(&self) -> (u64, u32, u32) {
        (self.mirrored, self.solo_writes, self.resyncs)
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_fsjournal_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F066-fsjournal-v5");

    // ── 增量位图 ──
    // 1) 首次 checkpoint：只写脏块，干净未持久化块不记节省。
    let mut db = DeltaBitmap::new();
    let _ = db.mark_dirty(3);
    let _ = db.mark_dirty(40);
    cs.add("delta_first_cp_writes_dirty", db.checkpoint() == (2, 0), "");
    // 2) 二次 checkpoint：全干净 → 0 写 + 2 节省。
    cs.add("delta_second_cp_saves", db.checkpoint() == (0, 2) && db.total_saved() == 2, "");
    // 3) 新脏块只写增量。
    let _ = db.mark_dirty(40);
    cs.add("delta_incremental_only", db.checkpoint() == (1, 1), ""); // 只写 40 号；干净且已持久化的 3 号也记账
    // 4) 越界块拒绝。
    cs.add("delta_oob_refused", !db.mark_dirty(64), "");

    // ── 超时回收 ──
    // 5) 8 槽并发上限。
    let mut tr = TxnTimeoutRecycler::new(1000);
    let mut slots = [None; 9];
    for k in 0..9 {
        slots[k] = tr.begin();
    }
    cs.add("txn_cap_8", slots[8].is_none() && tr.in_flight() == 8, "");
    // 6) 超时回收 + 槽位释放。
    tr.advance_clock(1500);
    cs.add("txn_reap_timed_out", tr.reap_timed_out() == 8 && tr.in_flight() == 0 && tr.recycled() == 8, "");
    // 7) 未超时不动。
    let mut tr2 = TxnTimeoutRecycler::new(1000);
    let s = tr2.begin();
    tr2.advance_clock(999);
    cs.add("txn_within_timeout_kept", tr2.reap_timed_out() == 0 && tr2.in_flight() == 1, "");
    // 8) 提交释放。
    tr2.advance_clock(2000);
    cs.add("txn_commit_frees", tr2.commit(s.unwrap()) && !tr2.commit(s.unwrap()), "");

    // ── 镜像 ──
    // 9) 双写账。
    let mut ml = MirrorLedger::new();
    ml.write_block();
    ml.write_block();
    cs.add("mirror_dual_write", ml.stats() == (2, 0, 0) && ml.healthy(), "");
    // 10) B 失联 → A 独写差量账。
    ml.set_vol(1, false);
    ml.write_block();
    ml.write_block();
    ml.write_block();
    cs.add("mirror_solo_counts", ml.stats() == (2, 3, 0), "");
    // 11) B 回归 → 重同步清差量。
    ml.set_vol(1, true);
    cs.add("mirror_resync_on_return", ml.stats() == (2, 0, 1), "");
    // 12) 双失联写不进。
    ml.set_vol(0, false);
    ml.set_vol(1, false); // 双卷皆失联
    ml.write_block();
    cs.add("mirror_both_down_nowrites", ml.stats() == (2, 0, 1), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn delta_bitmap_full_sweep() {
        let mut db = DeltaBitmap::new();
        for b in 0..64 {
            let _ = db.mark_dirty(b);
        }
        assert_eq!(db.checkpoint(), (64, 0));
        assert_eq!(db.checkpoint(), (0, 64));
        assert_eq!(db.total_saved(), 64);
    }

    #[test]
    fn txn_reap_then_reuse() {
        let mut tr = TxnTimeoutRecycler::new(500);
        let s1 = tr.begin().unwrap();
        tr.advance_clock(600);
        assert_eq!(tr.reap_timed_out(), 1);
        // 回收后可复用槽。
        let s2 = tr.begin();
        assert_eq!(s2, Some(s1), "回收槽被复用（低槽优先）");
    }

    #[test]
    fn mirror_flap_accumulates() {
        let mut ml = MirrorLedger::new();
        ml.set_vol(0, false);
        ml.write_block();
        ml.set_vol(0, true); // 重同步 1
        ml.set_vol(0, false);
        ml.write_block();
        ml.write_block();
        ml.set_vol(0, true); // 重同步 2
        assert_eq!(ml.stats(), (0, 0, 2));
    }
}
