//! H2 域账目引擎 · 深化批次一（账目与留痕层纵深——骨架→器官）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F275 开始菜单电源菜单**：两路入口（开始菜单电源菜单 / 桌面
//!   独立关机按钮）同走一套电源账目（B-2902 同构）——账本不区分入口
//!   只记事实；长按物理电源键=硬件级兜底（文档化事件也入账留痕）；
//! - **F269 长复制暂停与恢复**：断点续传校验用例（人为截断后恢复，
//!   跳过已传块）——断点账逐块记校验和，恢复时校验匹配才跳过；
//! - **F295 时间同步与准确性**：NTP 静默修正留痕（>2s 阈值）、漂移
//!   补偿按上次校准斜率外推、手动改时提醒一次；
//! - **完整性通用件**：全账本走**哈希链**（F037 判据同源——改一条
//!   序号断链即检出），篡改注入演练进自检。
//!
//! 时间纪律：分钟戳全部注入；存储纪律：账本可编解码（[`h2snap`] 层
//! 序列化），本模块只管账目协议不管介质。

use crate::checks::CheckSet;
use crate::h2star::h2persist::fnv1a64;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 通用哈希链账本
// ---------------------------------------------------------------------------

/// 账本容量上限（域内定容——满则拒绝追加并如实上报，不静默丢）。
pub const LEDGER_CAP: usize = 512;

/// 一条账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub seq: u64,
    pub at_min: u64,
    /// 账目类别（域内固定词表：`power`/`resume`/`timesync`）。
    pub kind: &'static str,
    /// 事实载荷（人话可读——总日志中心 F188 直读）。
    pub payload: String,
    prev_hash: u64,
    hash: u64,
}

impl LedgerEntry {
    /// 人话行（账本导出格式）。
    pub fn line(&self) -> String {
        alloc::format!("#{} [{}] {} {}", self.seq, self.kind, self.at_min, self.payload)
    }
}

/// 追加式哈希链账本：`hash = FNV1a(seq|at|kind|payload|prev_hash)`——
/// 任何一条被改/删/插，后续全链校验失败（F037「审计不可篡改」机制）。
pub struct HashLedger {
    entries: Vec<LedgerEntry>,
    /// 拒绝追加计数（满载诚实记账——异常零静默）。
    pub rejected: u64,
}

impl HashLedger {
    pub fn new() -> HashLedger {
        HashLedger { entries: Vec::new(), rejected: 0 }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 追加一条（哈希链自动续接）。
    pub fn append(&mut self, kind: &'static str, payload: String, at_min: u64) -> Option<u64> {
        if self.entries.len() >= LEDGER_CAP {
            self.rejected += 1;
            return None;
        }
        let seq = self.entries.len() as u64 + 1;
        let prev_hash = self.entries.last().map(|e| e.hash).unwrap_or(0);
        let hash = fnv1a64(
            alloc::format!("{}|{}|{}|{}|{}", seq, at_min, kind, payload, prev_hash).as_bytes(),
        );
        self.entries.push(LedgerEntry { seq, at_min, kind, payload, prev_hash, hash });
        Some(seq)
    }

    /// 全链校验：返回 Ok(()) 或 Err(首条断链位置)。
    pub fn verify(&self) -> Result<(), usize> {
        let mut prev = 0u64;
        for (i, e) in self.entries.iter().enumerate() {
            let expect = fnv1a64(
                alloc::format!("{}|{}|{}|{}|{}", e.seq, e.at_min, e.kind, e.payload, e.prev_hash)
                    .as_bytes(),
            );
            if e.seq != i as u64 + 1 || e.prev_hash != prev || e.hash != expect {
                return Err(i);
            }
            prev = e.hash;
        }
        Ok(())
    }

    /// 按类别取账（保持时序）。
    pub fn by_kind(&self, kind: &str) -> Vec<&LedgerEntry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }

    /// 演练口：篡改第 `i` 条载荷（自检证明断链可检出）。
    pub fn inject_tamper(&mut self, i: usize, new_payload: &str) -> bool {
        match self.entries.get_mut(i) {
            Some(e) => {
                e.payload = String::from(new_payload);
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F275 电源账目（B-2902 同构）
// ---------------------------------------------------------------------------

/// 电源动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Sleep,
    Shutdown,
    Reboot,
}

impl PowerAction {
    pub fn label(self) -> &'static str {
        match self {
            PowerAction::Sleep => "睡眠",
            PowerAction::Shutdown => "关机",
            PowerAction::Reboot => "重启",
        }
    }
}

/// 电源入口（两路图形入口 + 长按兜底——账本只记事实不评判入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSource {
    StartMenu,
    DeskButton,
    /// 长按物理电源键 4s——硬件级兜底（文档化：先走软件关机）。
    LongPressFallback,
}

/// 电源账目。判据「两路入口同账目」的机制保证：开始菜单与桌面按钮
/// 追加的是**同一个账本**，查询不区分入口也能对出全账。
pub struct PowerLedger {
    pub ledger: HashLedger,
}

impl PowerLedger {
    pub fn new() -> PowerLedger {
        PowerLedger { ledger: HashLedger::new() }
    }

    /// 记一笔电源动作（B-2902 对账口径：动作+入口+时刻三要素齐）。
    /// 载荷措辞纪律：动作标签（「关机/重启/睡眠」）必须是条目里唯一的
    /// 动作词——尾部注记用「B-2902 完整链」不带动作字眼，否则
    /// `accounted()` 的关键词查询会被注记污染（本条即缺陷账 #16 的根因）。
    pub fn record(&mut self, action: PowerAction, source: PowerSource, at_min: u64) -> Option<u64> {
        let payload = alloc::format!("{}（入口 {:?}）——B-2902 完整链", action.label(), source);
        self.ledger.append("power", payload, at_min)
    }

    /// 两路入口同账审计：给定两个时刻的记录，无论入口如何，账本里
    /// 都能对到（「开始菜单关的机」与「按钮关的机」同一本账）。
    pub fn accounted(&self, action: PowerAction) -> usize {
        self.ledger
            .by_kind("power")
            .iter()
            .filter(|e| e.payload.contains(action.label()))
            .count()
    }
}

// ---------------------------------------------------------------------------
// F269 断点账（块校验表）
// ---------------------------------------------------------------------------

/// 一个复制任务的断点账。块校验和 = FNV1a(块内容)——恢复时重算比对，
/// 匹配才跳过（判据「人为截断后恢复，跳过已传块」的机制保证）。
pub struct ResumeLedger {
    /// 任务标识（源→目标）。
    pub task: String,
    pub block_bytes: u64,
    pub total_blocks: u64,
    /// 已完成块的校验和（下标=块号）。
    done: Vec<Option<u64>>,
    /// 断点暂停时刻（恢复展示「从 N% 继续」）。
    pub paused_at_min: Option<u64>,
}

impl ResumeLedger {
    pub fn new(task: &str, total_bytes: u64, block_bytes: u64) -> ResumeLedger {
        let total_blocks = if block_bytes == 0 {
            0
        } else {
            total_bytes.div_ceil(block_bytes)
        };
        ResumeLedger {
            task: String::from(task),
            block_bytes,
            total_blocks,
            done: alloc::vec![None; total_blocks as usize],
            paused_at_min: None,
        }
    }

    /// 标记一块完成（校验和入账）。
    pub fn mark_done(&mut self, block: usize, content: &[u8]) -> bool {
        if block >= self.done.len() {
            return false;
        }
        self.done[block] = Some(fnv1a64(content));
        true
    }

    /// 暂停（记断点时刻）。
    pub fn pause(&mut self, at_min: u64) {
        self.paused_at_min = Some(at_min);
    }

    /// 进度（已完成块占比——进度条直读）。
    pub fn progress_permille(&self) -> u64 {
        if self.total_blocks == 0 {
            return 0;
        }
        self.done.iter().filter(|d| d.is_some()).count() as u64 * 1000 / self.total_blocks
    }

    /// 恢复：对「人为截断」的块重算校验——匹配才跳过，不匹配（内容
    /// 变了/写了一半）重传。返回 (跳过块数, 需重传块号表)。
    pub fn plan_resume(&self, source_bytes: &[u8]) -> (usize, Vec<usize>) {
        let mut skipped = 0usize;
        let mut resend: Vec<usize> = Vec::new();
        for (i, slot) in self.done.iter().enumerate() {
            match slot {
                None => resend.push(i),
                Some(sum) => {
                    let start = i as u64 * self.block_bytes;
                    let end = (start + self.block_bytes).min(source_bytes.len() as u64);
                    let slice = &source_bytes[start as usize..end as usize];
                    if fnv1a64(slice) == *sum {
                        skipped += 1;
                    } else {
                        resend.push(i);
                    }
                }
            }
        }
        (skipped, resend)
    }
}

// ---------------------------------------------------------------------------
// F295 校时留痕账
// ---------------------------------------------------------------------------

/// 校时来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncSource {
    /// NTP 静默修正（偏差 >2s 触发——阈值由调用方判定后入账）。
    Ntp,
    /// 离线漂移补偿（按上次校准斜率外推——机器内自愈，也留痕）。
    DriftComp,
    /// 用户手动改时。
    Manual,
}

/// 校时账。
pub struct TimesyncLedger {
    pub ledger: HashLedger,
    /// NTP 静默修正阈值（ms，主册定值 2s；旋钮 h2.f295.ntp_s 同源）。
    pub ntp_threshold_ms: i64,
}

impl TimesyncLedger {
    pub fn new() -> TimesyncLedger {
        TimesyncLedger { ledger: HashLedger::new(), ntp_threshold_ms: 2_000 }
    }

    /// 记一次校准（偏差与来源如实入账——「修正事件留痕」）。
    pub fn record(&mut self, source: SyncSource, offset_ms: i64, at_min: u64) -> Option<u64> {
        let payload = alloc::format!("校准 来源 {:?} 偏差 {}ms", source, offset_ms);
        self.ledger.append("timesync", payload, at_min)
    }

    /// NTP 修正是否该触发（>2s 硬线）。
    pub fn needs_ntp(&self, offset_ms: i64) -> bool {
        offset_ms.abs() > self.ntp_threshold_ms
    }

    /// 漂移斜率估计（ms/分钟）：取最近 `n` 条 NTP 账做最小二乘——
    /// 深化点（对账 ugly corner #F295「线性外推未建模」→ 显式最小二乘
    /// 而非两点差分，多校准点去抖）。少于 2 条返回 None（不编造）。
    pub fn drift_slope_per_min(&self, n: usize) -> Option<f32> {
        let pts: Vec<(f32, f32)> = self
            .ledger
            .by_kind("timesync")
            .iter()
            .filter(|e| e.payload.contains("Ntp"))
            .map(|e| {
                // 载荷结构化回读：「偏差 Nms」字段是偏移的唯一落账处，
                // 人话与机器读同一字符串（一处一事实）。
                let at = e.at_min as f32;
                let off = parse_offset(&e.payload) as f32;
                (at, off)
            })
            .collect();
        let pts = if pts.len() > n { pts[pts.len() - n..].to_vec() } else { pts };
        if pts.len() < 2 {
            return None;
        }
        let n = pts.len() as f32;
        let sx: f32 = pts.iter().map(|p| p.0).sum();
        let sy: f32 = pts.iter().map(|p| p.1).sum();
        let sxx: f32 = pts.iter().map(|p| p.0 * p.0).sum();
        let sxy: f32 = pts.iter().map(|p| p.0 * p.1).sum();
        let den = n * sxx - sx * sx;
        if den.abs() < f32::EPSILON {
            return None;
        }
        Some((n * sxy - sx * sy) / den)
    }

    /// 手动改时提醒：改回历史（新时刻 < 最近账目时刻）给一次提醒的
    /// 判定位——「可能影响文件排序判断」。
    pub fn manual_backwards(&self, new_min: u64) -> bool {
        match self.ledger.by_kind("timesync").last() {
            Some(e) => new_min < e.at_min,
            None => false,
        }
    }
}

/// 从载荷解析「偏差 Nms」（账本载荷的机器回读口径——负号处理齐）。
fn parse_offset(payload: &str) -> i64 {
    let start = match payload.find("偏差 ") {
        Some(i) => i + "偏差 ".len(),
        None => return 0,
    };
    let end = payload[start..]
        .find("ms")
        .map(|i| start + i)
        .unwrap_or(payload.len());
    payload[start..end].parse().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2ledger_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2ledger");
    // --- 哈希链：追加-校验绿；篡改一条→断链定位准确。 ---
    let mut lg = HashLedger::new();
    for i in 0..5 {
        let _ = lg.append("power", alloc::format!("事件{i}"), 100 + i);
    }
    set.add("h2ledger chain green", lg.verify().is_ok(), "intact");
    let _ = lg.inject_tamper(2, "被改的事件");
    set.add("h2ledger tamper caught", lg.verify() == Err(2), "F037 断链即检出");
    // --- F275 两路入口同账。 ---
    let mut pl = PowerLedger::new();
    let _ = pl.record(PowerAction::Shutdown, PowerSource::StartMenu, 10);
    let _ = pl.record(PowerAction::Reboot, PowerSource::DeskButton, 20);
    let _ = pl.record(PowerAction::Sleep, PowerSource::StartMenu, 30);
    set.add(
        "h2ledger F275 one book",
        pl.accounted(PowerAction::Shutdown) == 1
            && pl.accounted(PowerAction::Reboot) == 1
            && pl.ledger.verify().is_ok(),
        "B-2902 same ledger",
    );
    let _ = pl.record(PowerAction::Shutdown, PowerSource::LongPressFallback, 40);
    set.add("h2ledger F275 fallback logged", pl.accounted(PowerAction::Shutdown) == 2, "documented path");
    // --- F269 断点账：截断恢复，坏块重传、好块跳过。 ---
    let src: Vec<u8> = (0..160u8).collect(); // 160B，块 64B → 3 块。
    let mut rl = ResumeLedger::new("a→b", 160, 64);
    set.add("h2ledger F269 blocks", rl.total_blocks == 3, "ceil div");
    let _ = rl.mark_done(0, &src[0..64]);
    let _ = rl.mark_done(1, &src[64..128]); // 此块随后被「截断改写」。
    rl.pause(500);
    let mut corrupted = src.clone();
    corrupted[100] = corrupted[100].wrapping_add(1); // 人为截断场景：第 1 块内容变了。
    let (skip, resend) = rl.plan_resume(&corrupted);
    set.add(
        "h2ledger F269 resume plan",
        skip == 1 && resend == alloc::vec![1, 2] && rl.progress_permille() == 666,
        "skip good resend bad",
    );
    // --- F295 校时账：阈值、斜率、手动回拨提醒。 ---
    let mut tl = TimesyncLedger::new();
    set.add("h2ledger F295 threshold", tl.needs_ntp(2_100) && !tl.needs_ntp(1_900), "2s line");
    let _ = tl.record(SyncSource::Ntp, 100, 60);
    let _ = tl.record(SyncSource::Ntp, 160, 120);
    let _ = tl.record(SyncSource::Ntp, 220, 180);
    let slope = tl.drift_slope_per_min(10);
    set.add(
        "h2ledger F295 slope",
        slope.map(|s| (s - 1.0).abs() < 0.05).unwrap_or(false),
        "1ms/min fit",
    );
    set.add(
        "h2ledger F295 backwards",
        tl.manual_backwards(100) && !tl.manual_backwards(500),
        "manual notice",
    );
    // --- 容量纪律：满则拒收并计数（不静默丢）。 ---
    let mut full = HashLedger::new();
    let mut ok = true;
    for i in 0..LEDGER_CAP {
        ok &= full.append("resume", alloc::format!("e{i}"), i as u64).is_some();
    }
    let rejected_before = full.rejected;
    let over = full.append("resume", String::from("溢出"), 0);
    set.add(
        "h2ledger cap honest",
        ok && over.is_none() && full.rejected == rejected_before + 1,
        "no silent drop",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2ledger_all_green() {
        let set = run_h2ledger_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2ledger 自检红 {f}/{p}");
    }

    #[test]
    fn delete_breaks_chain_too() {
        // 删一条（重排 seq）同样断链——改/删/插三攻全覆盖的补充用例。
        let mut lg = HashLedger::new();
        for i in 0..4 {
            let _ = lg.append("timesync", alloc::format!("t{i}"), i as u64);
        }
        lg.entries.remove(1);
        assert!(lg.verify().is_err(), "删除必须断链");
    }

    #[test]
    fn slope_none_when_insufficient() {
        let tl = TimesyncLedger::new();
        assert!(tl.drift_slope_per_min(10).is_none(), "单点不编造斜率");
    }
}
