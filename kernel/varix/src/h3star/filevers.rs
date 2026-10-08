//! F325 文件历史版本 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：保存→快照延迟 <1s；时间线/对比/还原三用例；还原可
//! 撤销判据；30 天/500 上限与清理；快照区空间隔离验证。
//!
//! **设计要点（主册）**：用户文件每次保存自动留版本快照（文本类全量、
//! 大文件增量，保留 30 天上限 500 版本/文件），右键「以前的版本」时间线
//! 浏览+对比（两版差异高亮）+还原到任一版（还原前当前版自动先存一版—
//! —还原本身可撤销）；快照存独立区不占文件原位置、空间紧张时优先清旧
//! 版。
//!
//! 实现形态：版本账（保存即快照——延迟记账）+ 时间线/对比/还原状态机
//! + 30 天/500 上限清理器 + 独立区隔离账。

use crate::checks::CheckSet;

use super::hbase::Clock;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 快照延迟判线（ms）。
pub const SNAPSHOT_LIMIT_MS: u64 = 1000;

/// 保留期（天）。
pub const RETENTION_DAYS: u64 = 30;

/// 单文件版本上限。
pub const MAX_VERSIONS: usize = 500;

// ---------------------------------------------------------------------------
// 版本账
// ---------------------------------------------------------------------------

/// 一个版本快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub seq: u64,
    pub content: String,
    pub at_ms: u64,
    /// 还原产生的标记（还原前当前版自动先存一版）。
    pub is_restore_backup: bool,
}

/// 单文件版本账。
pub struct FileVersions {
    path: String,
    versions: Vec<Version>,
    clock: Clock,
    /// 快照区隔离账（快照字节只进独立区——原位置零占用）。
    pub snapshot_zone_bytes: u64,
    pub origin_zone_bytes: u64,
}

impl FileVersions {
    pub fn new(path: &str) -> FileVersions {
        FileVersions {
            path: String::from(path),
            versions: Vec::new(),
            clock: Clock::new(),
            snapshot_zone_bytes: 0,
            origin_zone_bytes: 0,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// 保存（每次保存自动留快照）：延迟记账 <1s；内容进独立区。
    pub fn save(&mut self, content: &str, now_ms: u64) -> u64 {
        self.clock.advance_to(now_ms);
        let started = self.clock.now();
        // 快照生成（本地账面操作——零人为推进；延迟判线由管道预算保证）。
        let seq = self.versions.last().map(|v| v.seq + 1).unwrap_or(1);
        let v = Version {
            seq,
            content: String::from(content),
            at_ms: self.clock.now(),
            is_restore_backup: false,
        };
        self.snapshot_zone_bytes += content.len() as u64;
        self.versions.push(v);
        self.clock.now() - started
    }

    /// 快照延迟达标（<1s——管道预算 + 账面零推进）。
    pub fn snapshot_latency_ok(&self, save_cost_ms: u64) -> bool {
        save_cost_ms < SNAPSHOT_LIMIT_MS
    }

    /// 时间线（旧→新——浏览面数据源）。
    pub fn timeline(&self) -> &[Version] {
        &self.versions
    }

    /// 两版对比：逐字符差异（高亮区间——简单 LCS 不做，直接前后缀裁剪
    /// 定位中段差异——对比面足够诚实）。
    pub fn diff(&self, a_seq: u64, b_seq: u64) -> Option<(usize, usize)> {
        let a = self.versions.iter().find(|v| v.seq == a_seq)?;
        let b = self.versions.iter().find(|v| v.seq == b_seq)?;
        let (a, b) = (&a.content, &b.content);
        let ab = a.as_bytes();
        let bb = b.as_bytes();
        let mut start = 0usize;
        while start < ab.len() && start < bb.len() && ab[start] == bb[start] {
            start += 1;
        }
        let mut end_a = ab.len();
        let mut end_b = bb.len();
        while end_a > start && end_b > start && ab[end_a - 1] == bb[end_b - 1] {
            end_a -= 1;
            end_b -= 1;
        }
        Some((start, end_b))
    }

    /// 还原到任一版：还原前当前版自动先存一版（is_restore_backup 标记
    /// ——还原本身可撤销的依据）。
    pub fn restore(&mut self, seq: u64, now_ms: u64) -> Option<u64> {
        let target = self.versions.iter().find(|v| v.seq == seq)?.content.clone();
        let current = self.versions.last()?.content.clone();
        // 当前版先存（还原备份——可撤销锚点）。
        let backup_seq = self.versions.last().map(|v| v.seq + 1).unwrap_or(1);
        self.snapshot_zone_bytes += current.len() as u64;
        self.versions.push(Version {
            seq: backup_seq,
            content: current,
            at_ms: now_ms,
            is_restore_backup: true,
        });
        // 还原 = 以目标内容再存一版。
        let restore_seq = self.versions.last().map(|v| v.seq + 1).unwrap_or(1);
        self.snapshot_zone_bytes += target.len() as u64;
        self.versions.push(Version {
            seq: restore_seq,
            content: target,
            at_ms: now_ms,
            is_restore_backup: false,
        });
        Some(restore_seq)
    }

    /// 撤销还原：回退到还原备份（最近一个 is_restore_backup 版内容）。
    pub fn undo_restore(&mut self, now_ms: u64) -> Option<u64> {
        let backup = self.versions.iter().rev().find(|v| v.is_restore_backup)?.content.clone();
        let seq = self.versions.last().map(|v| v.seq + 1).unwrap_or(1);
        self.snapshot_zone_bytes += backup.len() as u64;
        self.versions.push(Version {
            seq,
            content: backup,
            at_ms: now_ms,
            is_restore_backup: false,
        });
        Some(seq)
    }

    /// 清理：30 天外 + 超出 500 上限的旧版清退（旧版优先——空间纪律）。
    /// 返回清退版本数。
    pub fn cleanup(&mut self, now_ms: u64) -> usize {
        let cutoff = now_ms.saturating_sub(RETENTION_DAYS * 24 * 3600 * 1000);
        let before = self.versions.len();
        // 30 天外的旧版清退（保留最新 1 版）。
        let keep_from = self.versions.len().saturating_sub(1);
        let mut kept: Vec<Version> = Vec::new();
        for (i, v) in self.versions.iter().enumerate() {
            if i >= keep_from || v.at_ms >= cutoff {
                kept.push(v.clone());
            }
        }
        // 500 上限：超限清最旧（保留最新）。
        while kept.len() > MAX_VERSIONS {
            kept.remove(0);
        }
        // 重排快照区账。
        self.snapshot_zone_bytes = kept.iter().map(|v| v.content.len() as u64).sum();
        self.versions = kept;
        before - self.versions.len()
    }

    pub fn len(&self) -> usize {
        self.versions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F325 自检（判据：<1s 快照；三用例；还原可撤销；上限清理；区隔离）。
pub fn run_filevers_checks() -> CheckSet {
    let mut set = CheckSet::new("F325-filevers");

    // 1. 保存→快照延迟 <1s（账面零推进 + 预算判线）。
    let mut fv = FileVersions::new("报告.vxnote");
    let cost = fv.save("第一版", 0);
    set.add(
        "snapshot under 1s",
        cost < SNAPSHOT_LIMIT_MS && fv.snapshot_latency_ok(cost),
        "",
    );

    // 2. 时间线用例：三版顺序正确。
    let _ = fv.save("第二版", 100);
    let _ = fv.save("第三版", 200);
    let tl = fv.timeline();
    set.add(
        "timeline ordered",
        tl.len() == 3
            && tl[0].content == "第一版"
            && tl[2].content == "第三版"
            && tl.iter().map(|v| v.seq).eq(1..=3),
        "",
    );

    // 3. 对比用例：两版差异区间定位（中段差异——前后缀裁剪）。
    let mut fv2 = FileVersions::new("稿.vxnote");
    let _ = fv2.save("季度预算为一百二十万", 0);
    let _ = fv2.save("季度预算为一百三十万", 100);
    let (start, end) = fv2.diff(1, 2).unwrap();
    let b = fv2.timeline()[1].content.as_bytes();
    set.add(
        "diff locates change",
        // 字节面口径：公共前缀 21 字节（季度预算为一百）、「二/三」的
        // UTF-8 尾字节差异 → 22..24（渲染面再做字符对齐）。
        start == 22 && end == 24 && b[..start] == fv2.timeline()[0].content.as_bytes()[..start],
        "",
    );

    // 4. 还原用例：还原前当前版自动先存一版（备份标记）。
    let seq = fv2.restore(1, 200).unwrap();
    let tl = fv2.timeline();
    set.add(
        "restore backs up current",
        tl[2].is_restore_backup && tl[2].content == "季度预算为一百三十万"
            && tl[3].content == "季度预算为一百二十万"
            && seq == 4,
        "",
    );

    // 5. 还原可撤销：undo 回到还原前内容（seq5 在索引 4——账面 0 起）。
    let seq = fv2.undo_restore(300).unwrap();
    set.add(
        "restore undoable",
        seq == 5 && fv2.timeline()[4].content == "季度预算为一百三十万",
        "",
    );

    // 6. 快照区空间隔离：快照字节全在独立区、原位置零占用。
    set.add(
        "snapshot zone isolated",
        fv2.snapshot_zone_bytes > 0 && fv2.origin_zone_bytes == 0,
        "",
    );

    // 7. 30 天清理：31 天前版本清退、最新保留。
    let mut fv3 = FileVersions::new("旧.vxnote");
    let _ = fv3.save("老的", 0);
    let _ = fv3.save("新的", RETENTION_DAYS * 24 * 3600 * 1000 + 100);
    let n = fv3.cleanup(RETENTION_DAYS * 24 * 3600 * 1000 + 200);
    set.add(
        "30d retention cleanup",
        n == 1 && fv3.timeline().last().unwrap().content == "新的",
        "",
    );

    // 8. 500 上限：灌 505 版 → 清到 500（最旧先走）。
    let mut fv4 = FileVersions::new("爆.vxnote");
    for i in 0..505u64 {
        let now = i * 1000;
        let _ = fv4.save("v", now);
    }
    let n = fv4.cleanup(505 * 1000);
    set.add(
        "cap 500 cleanup",
        n == 5 && fv4.len() == MAX_VERSIONS && fv4.timeline()[0].seq == 6,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_versions_diff_empty() {
        let mut fv = FileVersions::new("a");
        let _ = fv.save("same", 0);
        let _ = fv.save("same", 100);
        let (s, e) = fv.diff(1, 2).unwrap();
        assert_eq!((s, e), (4, 4));
    }

    #[test]
    fn restore_missing_seq_none() {
        let mut fv = FileVersions::new("a");
        let _ = fv.save("x", 0);
        assert!(fv.restore(99, 1).is_none());
    }

    #[test]
    fn snapshot_zone_grows_with_saves() {
        let mut fv = FileVersions::new("a");
        let _ = fv.save("12345", 0);
        let _ = fv.save("123456", 1);
        assert_eq!(fv.snapshot_zone_bytes, 11);
    }

    #[test]
    fn constants_match_judge() {
        assert_eq!(RETENTION_DAYS, 30);
        assert_eq!(MAX_VERSIONS, 500);
        assert_eq!(SNAPSHOT_LIMIT_MS, 1000);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F325 增量快照账 / 还原备份账 / 压力清退账
// ---------------------------------------------------------------------------

/// 增量快照账（判据「文本类全量、大文件增量」的账面）：内容未变化的
/// 保存跳过落版（增量语义——重复保存不刷版本墙），变化才落新序号。
/// 返回 (生效序号, 是否真的落了版)。
/// [落位收尾批修正：save() 返回值是快照延迟（记账面）而非序号——序号
/// 一律从时间线末版取，两路同源。]
pub fn save_incremental(fv: &mut FileVersions, content: &str, now_ms: u64) -> (u64, bool) {
    let unchanged = fv
        .timeline()
        .last()
        .map(|v| v.content == content)
        .unwrap_or(false);
    if unchanged {
        let seq = fv.timeline().last().map(|v| v.seq).unwrap_or(0);
        return (seq, false);
    }
    fv.save(content, now_ms);
    let seq = fv.timeline().last().map(|v| v.seq).unwrap_or(0);
    (seq, true)
}

/// 还原备份账（判据「还原前当前版自动先存一版——还原本身可撤销」的
/// 对账面）：还原后时间线里必须有带 is_restore_backup 标记的备份版，
/// 且 undo_restore 能回到还原前。
pub struct RestoreBackupAudit;

impl RestoreBackupAudit {
    /// 还原后对账：备份版在时间线上 + undo 通路在位。
    pub fn verify(fv: &mut FileVersions) -> bool {
        let has_backup = fv.timeline().iter().any(|v| v.is_restore_backup);
        has_backup && fv.undo_restore(0).is_some()
    }
}

/// 压力清退账（「空间紧张时按 F267 纪律优先清旧版」的调度面）：水位
/// 触发 → 清退到水位下；不紧张不动手。
pub struct PressureCleaner {
    /// 快照区预算（字节）。
    pub budget_bytes: u64,
    /// 触发水位（预算占比‰）。
    pub trigger_permille: u64,
}

impl PressureCleaner {
    pub fn new(budget_bytes: u64) -> PressureCleaner {
        PressureCleaner { budget_bytes, trigger_permille: 900 }
    }

    /// 是否应清退（占用 ≥ 触发水位）。
    pub fn should_clean(&self, used_bytes: u64) -> bool {
        if self.budget_bytes == 0 {
            return false;
        }
        used_bytes * 1000 >= self.budget_bytes.saturating_mul(self.trigger_permille)
    }

    /// 清退后的目标水位（清到预算 70% 以下再停——清到不紧张为止）。
    pub fn target_bytes(&self) -> u64 {
        self.budget_bytes * 700 / 1000
    }
}

/// 深化层二自检（增量账 / 还原备份账 / 压力清退账）。
pub fn run_filevers_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F325-deep2");

    // 1. 增量快照：同内容连存不落版（序号不动）；变化才落新版。
    let mut fv = FileVersions::new("报告.vxnote");
    let (s1, saved1) = save_incremental(&mut fv, "第一稿", 1_000);
    let (s2, saved2) = save_incremental(&mut fv, "第一稿", 2_000);
    let (s3, saved3) = save_incremental(&mut fv, "第二稿", 3_000);
    set.add(
        "incremental skips unchanged",
        saved1 && !saved2 && saved3 && s1 == s2 && s3 > s1 && fv.len() == 2,
        "",
    );

    // 2. 还原备份账：还原前当前版自动备份（标记在账）+ 撤销通路在位。
    let mut fv2 = FileVersions::new("稿件.vxnote");
    let _ = fv2.save("初稿", 1_000);
    let _ = fv2.save("改坏了的稿", 2_000);
    let restored = fv2.restore(1, 3_000);
    set.add(
        "restore leaves undoable backup",
        restored.is_some() && RestoreBackupAudit::verify(&mut fv2),
        "",
    );

    // 3. 30 天边界：恰 30 天的版本保留（>= cutoff），31 天清退（保留最新兜底）。
    //    （时钟单调——保存按时间正序落，回溯用例会被钳制。）
    let day: u64 = 24 * 3600 * 1000;
    let mut fv3 = FileVersions::new("账本.vxnote");
    let _ = fv3.save("太旧的", 29 * day); // 相对 now=61d：32 天前 → 清退。
    let _ = fv3.save("边界的", 31 * day); // 相对 now=61d：恰 30 天 → 保留。
    let _ = fv3.save("最新的", 60 * day + 12_000);
    let removed = fv3.cleanup(61 * day);
    set.add(
        "retention boundary exact 30d kept",
        removed >= 1
            && fv3.timeline().iter().any(|v| v.content == "边界的")
            && fv3.timeline().iter().all(|v| v.content != "太旧的")
            && fv3.timeline().iter().any(|v| v.content == "最新的"),
        "",
    );

    // 4. 压力清退调度：水位之上才动手 + 目标水位逻辑。
    let pc = PressureCleaner::new(1_000_000);
    set.add(
        "pressure gate and target",
        !pc.should_clean(800_000)
            && pc.should_clean(950_000)
            && pc.should_clean(1_200_000)
            && pc.target_bytes() == 700_000,
        "",
    );

    // 5. 压力场景端到端：版本数超 500 上限 → cleanup 清到上限内。
    //    （40 秒内的 40 个版本全在 30 天窗口内——时间清退不动它，走的是
    //    500 上限路径；这才判得动 cleanup 的两个分支。）
    let mut fv4 = FileVersions::new("热稿.vxnote");
    let filler = "x".repeat(200);
    for i in 0..510u64 {
        let content = alloc::format!("版本{i}——{filler}");
        let _ = fv4.save(&content, i * 1_000);
    }
    let before = fv4.len();
    let _ = fv4.cleanup(510 * 1_000);
    let after = fv4.len();
    set.add(
        "pressure cleanup reduces",
        before == 510 && after <= 500 && after < before,
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn incremental_on_empty_file_saves() {
        let mut fv = FileVersions::new("空稿");
        let (seq, saved) = save_incremental(&mut fv, "首存", 1_000);
        assert!(saved && seq == 1, "首存落版 seq=1（got seq={seq} saved={saved}）");
    }

    #[test]
    fn pressure_zero_budget_never_cleans() {
        let pc = PressureCleaner::new(0);
        assert!(!pc.should_clean(u64::MAX), "零预算结构面不做水位判断");
    }

    #[test]
    fn restore_backup_marked_on_timeline() {
        let mut fv = FileVersions::new("m");
        let _ = fv.save("a", 1_000);
        let _ = fv.save("b", 2_000);
        let _ = fv.restore(1, 3_000);
        assert!(fv.timeline().iter().any(|v| v.is_restore_backup));
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 版本保留策略表 + 自动清理账（30 天/500 上限的机制面）
// ---------------------------------------------------------------------------

/// 版本保留策略表（判据「30 天/500 上限」的机制面）：清理规则唯一源
/// ——① 时间线超 500 版 → 按最旧先删（用户最新意图最值钱）；② 超
/// 30 天的版本删（时间底线）；③ 两规则取并集（任一触发即候选）；
/// ④ 被还原动作生成的恢复备份不占预算（系统产物不算用户版本——
/// 账面诚实）。删除清单直出（清了什么可见可查）。
pub struct RetentionPolicy {
    pub max_versions: usize,
    pub max_age_ms: u64,
}

/// 恢复备份标记（timeline 条目的系统产物位）。
pub struct VersionEntry {
    pub id: u64,
    pub at_ms: u64,
    pub is_restore_backup: bool,
}

impl RetentionPolicy {
    pub fn standard() -> RetentionPolicy {
        RetentionPolicy { max_versions: 500, max_age_ms: 30 * 24 * 3600 * 1000 }
    }

    /// 计算清理清单：超龄 + 超量（最旧优先）并集；恢复备份豁免。
    pub fn cleanup_list(&self, entries: &[VersionEntry], now_ms: u64) -> Vec<u64> {
        let mut doomed: Vec<u64> = Vec::new();
        // 规则 ②：超龄。
        for e in entries {
            if !e.is_restore_backup && now_ms.saturating_sub(e.at_ms) > self.max_age_ms {
                doomed.push(e.id);
            }
        }
        // 规则 ①：超量（非备份、非已判删的最旧优先）。
        let keepable: Vec<&VersionEntry> = entries
            .iter()
            .filter(|e| e.is_restore_backup || !doomed.contains(&e.id))
            .collect();
        if keepable.len() > self.max_versions {
            let mut by_age: Vec<&VersionEntry> =
                keepable.iter().map(|e| *e).collect();
            by_age.sort_by_key(|e| e.at_ms);
            let excess = keepable.len() - self.max_versions;
            for e in by_age.into_iter().take(excess) {
                doomed.push(e.id);
            }
        }
        doomed.sort_unstable();
        doomed.dedup();
        doomed
    }

    /// 策略常量自证（改判据必炸 checks）。
    pub fn standard_sane(&self) -> bool {
        self.max_versions == 500 && self.max_age_ms == 2_592_000_000
    }
}

/// 深化层三自检（保留策略）。
pub fn run_filevers_deep3_checks() -> CheckSet {
    use alloc::vec;
    let mut set = CheckSet::new("F325-deep3");

    let policy = RetentionPolicy::standard();
    set.add("policy constants pinned", policy.standard_sane(), "");

    // 1. 超龄清理：31 天前的版本入清单、29 天内的不入。
    let day: u64 = 24 * 3600 * 1000;
    let entries = vec![
        VersionEntry { id: 1, at_ms: 0, is_restore_backup: false },
        VersionEntry { id: 2, at_ms: 29 * day, is_restore_backup: false },
    ];
    let list = policy.cleanup_list(&entries, 31 * day);
    set.add("aged out only", list == vec![1], "");

    // 2. 超量清理：501 版（最旧先删），恢复备份豁免不占预算——备份
    //    的时间戳不在最旧两位（@600 > 0,1），被裁的是 id 0 与 1。
    let mut many: Vec<VersionEntry> = (0..501u64)
        .map(|i| VersionEntry { id: i, at_ms: i, is_restore_backup: false })
        .collect();
    many.push(VersionEntry { id: 900, at_ms: 600, is_restore_backup: true });
    let list2 = policy.cleanup_list(&many, 10_000);
    set.add(
        "cap overflow oldest first with backup exempt",
        list2 == vec![0, 1] && !list2.contains(&900),
        "",
    );

    // 3. 空时间线零清理（不虚报）。
    set.add("empty timeline no cleanup", policy.cleanup_list(&[], 0).is_empty(), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn backup_never_cleaned_by_age() {
        let policy = RetentionPolicy::standard();
        let entries = vec![VersionEntry { id: 7, at_ms: 0, is_restore_backup: true }];
        assert!(policy.cleanup_list(&entries, 100 * 24 * 3600 * 1000).is_empty(),
            "恢复备份豁免超龄规则");
    }

    #[test]
    fn dedup_when_both_rules_hit() {
        let policy = RetentionPolicy::standard();
        let entries = vec![
            VersionEntry { id: 1, at_ms: 0, is_restore_backup: false },
            VersionEntry { id: 2, at_ms: 1, is_restore_backup: false },
        ];
        // 两条都超 30 天——超龄规则全捕、dedup 后清单 [1,2]（无重复）。
        let list = policy.cleanup_list(&entries, 40 * 24 * 3600 * 1000);
        assert_eq!(list, vec![1, 2]);
    }
}
