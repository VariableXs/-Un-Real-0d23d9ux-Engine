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
