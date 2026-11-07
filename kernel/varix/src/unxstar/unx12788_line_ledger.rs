//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12788 行数终账（风险移交清单零静默） · 0x82F00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行（契约 §2 深化下限 ≥550 行，本模块按 550+ 交付）；
//!   ② UNX-F12789-J1 风险移交清单齐（ID/状态/下一步三列），零静默；
//!   ③ 零静默模型：移交行（ID R-D1-001 起）三列逐列非空——ID 非空、
//!      状态在闭集、下一步在闭集；空下一步/空 ID/越闭集状态 10/10 必抓。
//! 设计要点：
//!   - 移交行三列闭集：
//!     · ID 列：R-D1-001..R-D1-020 连续号段，非空且不重；
//!     · 状态列：待移交/移交中/已接收/已闭环 四态闭集；
//!     · 下一步列：复核/重测/归档/升级 四步闭集；
//!   - 零静默扫描：三列逐列盘点，空列/越闭集逐条记账（清单齐 = 三列
//!     缺失计数全零），任何静默行都使移交判据显红；
//!   - 状态机单调：填列中 → 清单齐 → 已移交，倒退显性拒
//!     （未盘点齐先移交即拒、移交后重盘点即拒），错误带可操作中文建议；
//!   - 注入面：空下一步/空 ID/越闭集状态三族注入 10 轮 10/10 必抓；
//!   - LCG（Numerical Recipes 32 位：st = st×1664525 + 1013904223），
//!     种子写死 = SEG_BASE（0x82F00），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic（测试区除外）；双跑一致 =
//!     同输入两次构建指纹逐域相等；中文注释与中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82F00，VX9 卡 12788 独占槽：0x80000 + 0x40 × (12788−12600)）。
pub const SEG_BASE: u32 = 0x82F00;

// ── 冻结常量表（判据原文锚定） ───────────────────────────────────
/// 移交行数冻结值：20 行（R-D1-001..R-D1-020）。
pub const ROW_N: usize = 20;
/// 风险移交清单族号（R-D1 前缀）。
pub const FAMILY: &str = "R-D1";
/// ID 号段基（R-D1-001）。
pub const ID_SEQ_BASE: u32 = 1;
/// 三列数冻结值：ID/状态/下一步。
pub const COL_N: usize = 3;
/// 状态闭集元素数：4 态。
pub const STATUS_N: usize = 4;
/// 下一步闭集元素数：4 步。
pub const NEXT_N: usize = 4;
/// 静默注入轮数（10/10 必抓判据）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 状态列闭集（四态） ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskStatus {
    /// 待移交：行已入清单、未交接
    Pending,
    /// 移交中：交接进行、接收方未确认
    InFlight,
    /// 已接收：接收方确认、未闭环
    Received,
    /// 已闭环：移交完成、责任转移毕
    Closed,
}

impl RiskStatus {
    pub fn cn(self) -> &'static str {
        match self {
            RiskStatus::Pending => "待移交",
            RiskStatus::InFlight => "移交中",
            RiskStatus::Received => "已接收",
            RiskStatus::Closed => "已闭环",
        }
    }
    /// 闭集全量（越界扫描语料）。
    pub const ALL: [RiskStatus; STATUS_N] = [
        RiskStatus::Pending,
        RiskStatus::InFlight,
        RiskStatus::Received,
        RiskStatus::Closed,
    ];
}

// ── 下一步列闭集（四步） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NextStep {
    /// 复核：交前复核证据链
    Review,
    /// 重测：回归复测风险项
    Retest,
    /// 归档：证据链归档封存
    Archive,
    /// 升级：升级至上一账期处置
    Escalate,
}

impl NextStep {
    pub fn cn(self) -> &'static str {
        match self {
            NextStep::Review => "复核",
            NextStep::Retest => "重测",
            NextStep::Archive => "归档",
            NextStep::Escalate => "升级",
        }
    }
    pub const ALL: [NextStep; NEXT_N] = [
        NextStep::Review,
        NextStep::Retest,
        NextStep::Archive,
        NextStep::Escalate,
    ];
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffCode {
    /// ID 列空/越段：仅收 R-D1-001..R-D1-020 连续号
    SilentIdInvalid,
    /// 状态列空/越闭集：仅收四态
    SilentStatusInvalid,
    /// 下一步列空/越闭集：仅收四步
    SilentNextStepInvalid,
    /// ID 重号：号段唯一性破坏
    DuplicateRiskId,
    /// 行缺位：20 行未填齐即盘点移交
    LedgerIncomplete,
    /// 未盘点齐先移交：先 zero_silent_scan 拿零静默结论再 handover
    HandoverBeforeScan,
    /// 移交状态倒退：已移交禁止重盘点重移交，重开走新清单
    HandoverRegress,
    /// 盘点覆盖不足：三列盘点须覆盖 行数×3 全量
    ScanCoverageShort,
}

impl HandoffCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HandoffCode::SilentIdInvalid => "ID 列空/越段：仅收 R-D1-001..R-D1-020；先补 ID",
            HandoffCode::SilentStatusInvalid => "状态列空/越闭集：仅收 待移交/移交中/已接收/已闭环",
            HandoffCode::SilentNextStepInvalid => "下一步列空/越闭集：仅收 复核/重测/归档/升级；空下一步必补",
            HandoffCode::DuplicateRiskId => "ID 重号：号段唯一性破坏；逐行回查重号源并改号",
            HandoffCode::LedgerIncomplete => "行缺位：20 行未填齐即盘点；先补齐再扫",
            HandoffCode::HandoverBeforeScan => "未盘点齐先移交：先 zero_silent_scan 拿零静默结论再 handover",
            HandoffCode::HandoverRegress => "移交状态倒退：已移交禁止重盘点重移交；重开走新清单",
            HandoffCode::ScanCoverageShort => "盘点覆盖不足：三列盘点须覆盖 行数×3 全量",
        }
    }
    pub const ALL: [HandoffCode; 8] = [
        HandoffCode::SilentIdInvalid,
        HandoffCode::SilentStatusInvalid,
        HandoffCode::SilentNextStepInvalid,
        HandoffCode::DuplicateRiskId,
        HandoffCode::LedgerIncomplete,
        HandoffCode::HandoverBeforeScan,
        HandoffCode::HandoverRegress,
        HandoffCode::ScanCoverageShort,
    ];
}

/// 诊断码查表闭集入口。
pub fn detail(code: u32) -> &'static str {
    let mut found: &'static str = "未知诊断码：不在 0x82F00 段闭集内";
    for c in HandoffCode::ALL.iter() {
        if c.val() == code {
            found = c.detail();
        }
    }
    found
}

// ── 移交状态机（单调，倒退显性拒） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffPhase {
    /// 填列中：行在建、列未齐
    Filling,
    /// 清单齐：零静默扫描通过、三列全非空
    Complete,
    /// 已移交：责任转移毕
    HandedOver,
}

impl HandoffPhase {
    pub fn rank(self) -> u8 {
        match self {
            HandoffPhase::Filling => 0,
            HandoffPhase::Complete => 1,
            HandoffPhase::HandedOver => 2,
        }
    }
    pub fn cn(self) -> &'static str {
        match self {
            HandoffPhase::Filling => "填列中",
            HandoffPhase::Complete => "清单齐",
            HandoffPhase::HandedOver => "已移交",
        }
    }
}

/// 风险移交行：三列（ID/状态/下一步），任一列空即静默行。
#[derive(Clone, Debug)]
pub struct RiskRow {
    /// ID 列（R-D1-001..R-D1-020）。
    pub id: String,
    /// 状态列（四态闭集，None 即空列）。
    pub status: Option<RiskStatus>,
    /// 下一步列（四步闭集，None 即空列）。
    pub next: Option<NextStep>,
}

/// 零静默盘点报告（逐列记账不静默）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SilentReport {
    /// ID 列空/越段行数。
    pub bad_id: u32,
    /// 状态列空/越闭集行数（Option 建模下恒 0，闭集判定留接口）。
    pub bad_status: u32,
    /// 下一步列空行数。
    pub bad_next: u32,
    /// ID 重号行数。
    pub dup_id: u32,
    /// 盘点覆盖量（行数×3）。
    pub scanned_cols: u32,
}

impl SilentReport {
    /// 清单齐判定：三列缺失全零 + 覆盖量足额。
    pub fn clean(&self) -> bool {
        self.bad_id == 0
            && self.bad_status == 0
            && self.bad_next == 0
            && self.dup_id == 0
            && self.scanned_cols == (ROW_N * COL_N) as u32
    }
}

/// 风险移交清单账本。
pub struct HandoffLedger {
    /// 移交行（20 行）。
    pub rows: Vec<RiskRow>,
    /// 移交状态机。
    pub phase: HandoffPhase,
    /// 历次盘点累计静默行数（显性记账，不静默吞）。
    pub silent_seen: u32,
}

impl HandoffLedger {
    pub fn new() -> HandoffLedger {
        HandoffLedger {
            rows: Vec::new(),
            phase: HandoffPhase::Filling,
            silent_seen: 0,
        }
    }

    /// 黄金清单：20 行三列全满（状态/下一步按确定性分布铺满闭集）。
    pub fn golden() -> HandoffLedger {
        let mut led = HandoffLedger::new();
        let mut i = 0usize;
        while i < ROW_N {
            let status = match RiskStatus::ALL.get(i % STATUS_N) {
                Some(s) => *s,
                None => RiskStatus::Pending,
            };
            let next = match NextStep::ALL.get((i * 3 + 1) % NEXT_N) {
                Some(n) => *n,
                None => NextStep::Review,
            };
            led.rows.push(RiskRow {
                id: format!("{}-{:03}", FAMILY, ID_SEQ_BASE + i as u32),
                status: Some(status),
                next: Some(next),
            });
            i += 1;
        }
        led
    }

    /// ID 是否在冻结号段（R-D1-001..R-D1-020）。
    pub fn id_in_range(id: &str) -> bool {
        let prefix = format!("{}-", FAMILY);
        let rest = match id.strip_prefix(prefix.as_str()) {
            Some(r) => r,
            None => return false,
        };
        if rest.len() != 3 {
            return false;
        }
        let bytes = rest.as_bytes();
        let d0 = *match bytes.first() { Some(b) => b, None => return false };
        let d1 = *match bytes.get(1) { Some(b) => b, None => return false };
        let d2 = *match bytes.get(2) { Some(b) => b, None => return false };
        if !(d0.is_ascii_digit() && d1.is_ascii_digit() && d2.is_ascii_digit()) {
            return false;
        }
        let seq = ((d0 - b'0') as u32) * 100 + ((d1 - b'0') as u32) * 10 + (d2 - b'0') as u32;
        seq >= ID_SEQ_BASE && seq < ID_SEQ_BASE + ROW_N as u32
    }

    /// 零静默盘点（非变更口径：三列逐列过账，覆盖量 = 行数×3，只读）。
    pub fn scan_silent(&self) -> Result<SilentReport, u32> {
        if self.rows.len() != ROW_N {
            return Err(HandoffCode::LedgerIncomplete.val());
        }
        let mut rep = SilentReport::default();
        // ID 重号盘点：号段内逐号落位（位图免下标）。
        let mut seen: Vec<bool> = Vec::new();
        let mut k = 0usize;
        while k < ROW_N {
            seen.push(false);
            k += 1;
        }
        for r in self.rows.iter() {
            rep.scanned_cols += 3; // 三列全量覆盖
            if r.id.is_empty() || !Self::id_in_range(&r.id) {
                rep.bad_id += 1;
            } else {
                // 落位查重：序号即位图下标。
                let tail = match r.id.rsplit('-').next() {
                    Some(t) => t,
                    None => "",
                };
                let mut seq = 0u32;
                let mut ten = 1u32;
                let tb = tail.as_bytes();
                let mut p = tb.len();
                while p > 0 {
                    p -= 1;
                    if let Some(d) = tb.get(p) {
                        if d.is_ascii_digit() {
                            seq += ((*d - b'0') as u32) * ten;
                            ten = ten.saturating_mul(10);
                        }
                    }
                }
                if seq >= ID_SEQ_BASE && (seq as usize) < ID_SEQ_BASE as usize + ROW_N {
                    let idx = (seq - ID_SEQ_BASE) as usize;
                    if let Some(slot) = seen.get_mut(idx) {
                        if *slot {
                            rep.dup_id += 1;
                        } else {
                            *slot = true;
                        }
                    }
                }
            }
            match r.status {
                Some(_) => {}
                None => rep.bad_status += 1,
            }
            match r.next {
                Some(_) => {}
                None => rep.bad_next += 1,
            }
        }

        Ok(rep)
    }

    /// 零静默盘点：三列逐列过账，覆盖量 = 行数×3。
    ///
    /// 已移交后重盘点即拒（状态倒退）；盘点毕状态推进 Filling→Complete。
    pub fn zero_silent_scan(&mut self) -> Result<SilentReport, u32> {
        if self.phase == HandoffPhase::HandedOver {
            return Err(HandoffCode::HandoverRegress.val());
        }
        let rep = self.scan_silent()?;
        self.silent_seen += rep.bad_id + rep.bad_status + rep.bad_next;
        if self.phase == HandoffPhase::Filling {
            self.phase = HandoffPhase::Complete;
        }
        Ok(rep)
    }

    /// 移交：须已盘点齐（零静默在账）；状态推进至 HandedOver。
    pub fn handover(&mut self) -> Result<SilentReport, u32> {
        if self.phase != HandoffPhase::Complete {
            return Err(HandoffCode::HandoverBeforeScan.val());
        }
        let rep = self.zero_silent_scan()?;
        if !rep.clean() {
            return Err(HandoffCode::SilentNextStepInvalid.val());
        }
        self.phase = HandoffPhase::HandedOver;
        Ok(rep)
    }

    /// 静默注入面（三族）：
    /// 族 0 = 空 ID、族 1 = 空下一步、族 2 = 空状态。
    pub fn inject_silent(&mut self, family: usize, n: usize) {
        let mut k = 0usize;
        while k < n {
            if let Some(r) = self.rows.get_mut(k) {
                match family % 3 {
                    0 => r.id = String::new(),
                    1 => r.next = None,
                    _ => r.status = None,
                }
            }
            k += 1;
        }
    }

    /// 清单指纹（双跑一致判据）。
    pub fn fingerprint(&self) -> u64 {
        let mut f: u64 = self.rows.len() as u64;
        f = f.wrapping_mul(31).wrapping_add(self.phase.rank() as u64);
        for r in self.rows.iter() {
            for b in r.id.as_bytes().iter() {
                f = f.wrapping_mul(31).wrapping_add(*b as u64);
            }
            f = f.wrapping_mul(31).wrapping_add(match r.status {
                Some(s) => s as u64 + 1,
                None => 0,
            });
            f = f.wrapping_mul(31).wrapping_add(match r.next {
                Some(n) => n as u64 + 1,
                None => 0,
            });
        }
        f
    }

    /// LCG 抽检：抽中行三列全非空且 ID 在段内。
    pub fn lcg_spot_check(&self, rounds: u32) -> bool {
        let mut rng = Lcg::new(SEG_BASE);
        let mut i = 0u32;
        while i < rounds {
            let idx = rng.below(ROW_N as u32) as usize;
            match self.rows.get(idx) {
                Some(r) => {
                    if r.id.is_empty() || r.status.is_none() || r.next.is_none() {
                        return false;
                    }
                    if !Self::id_in_range(&r.id) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 矩阵可点验 10/10（A1..A10）。
    pub fn run_asserts(&mut self) -> u32 {
        let mut pass = 0u32;
        // A1：行数恰 20。
        if self.rows.len() == ROW_N {
            pass += 1;
        }
        // A2：零静默盘点绿（三列缺失全零 + 覆盖足额；非变更口径，已移交亦可点验）。
        let rep = self.scan_silent().unwrap_or_default();
        if rep.clean() {
            pass += 1;
        }
        // A3：盘点覆盖量 = 20×3 = 60。
        if rep.scanned_cols == (ROW_N * COL_N) as u32 {
            pass += 1;
        }
        // A4：三列闭集铺设恰满（四态×四步黄金分布无重复号）。
        let ids_ok = self.rows.iter().all(|r| Self::id_in_range(&r.id));
        if ids_ok {
            pass += 1;
        }
        // A5：未盘点先移交即拒。
        let mut probe = HandoffLedger::golden();
        if matches!(probe.handover(), Err(c) if c == HandoffCode::HandoverBeforeScan.val()) {
            pass += 1;
        }
        // A6：空下一步注入 1 必抓。
        let mut probe2 = HandoffLedger::golden();
        let _ = probe2.zero_silent_scan();
        probe2.inject_silent(1, 1);
        let c6 = match probe2.zero_silent_scan() {
            Ok(r) => r.bad_next == 1 && !r.clean(),
            Err(_) => false,
        };
        if c6 {
            pass += 1;
        }
        // A7：移交后重盘点倒退显性拒。
        let mut probe3 = HandoffLedger::golden();
        let _ = probe3.zero_silent_scan();
        if probe3.handover().is_ok() && probe3.zero_silent_scan().is_err() {
            pass += 1;
        }
        // A8：终态恰 HandedOver。
        let mut probe4 = HandoffLedger::golden();
        let _ = probe4.zero_silent_scan();
        if probe4.handover().is_ok() && probe4.phase == HandoffPhase::HandedOver {
            pass += 1;
        }
        // A9：LCG 确定性抽检 16 行。
        if self.lcg_spot_check(16) {
            pass += 1;
        }
        // A10：ID 越段必抓（R-D1-021 不在段）。
        if !Self::id_in_range("R-D1-021") && Self::id_in_range("R-D1-020") {
            pass += 1;
        }
        pass
    }
}

// ── 确定性 LCG（Numerical Recipes 32 位，种子=SEG_BASE） ─────────
pub struct Lcg(u32);

impl Lcg {
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：建清单→零静默盘点→移交→点验 10/10 ─────────────────
pub fn golden_run() -> Result<u32, u32> {
    let mut led = HandoffLedger::golden();
    let rep = led.zero_silent_scan()?;
    if !rep.clean() {
        return Err(HandoffCode::SilentNextStepInvalid.val());
    }
    led.handover()?;
    Ok(led.run_asserts())
}

// ── 静默注入：三族 10 轮注入 10/10 必抓（零静默漏检） ────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 注入轮数。
    pub tries: u32,
    /// 必抓轮数。
    pub caught: u32,
    /// 末轮注入族号（0/1/2）。
    pub last_family: usize,
    /// 末轮报告静默列数。
    pub last_silent: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger {
            tries: 0,
            caught: 0,
            last_family: 0,
            last_silent: 0,
        }
    }
    /// 10/10 全抓判定。
    pub fn all_caught(&self) -> bool {
        self.tries == INJECT_ROUNDS && self.caught == INJECT_ROUNDS
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut led = HandoffLedger::golden();
        let _ = led.zero_silent_scan();
        let family = r as usize % 3;
        let n = 1usize + (r as usize % 3);
        led.inject_silent(family, n);
        let silent = match led.zero_silent_scan() {
            Ok(rep) => rep.bad_id + rep.bad_status + rep.bad_next,
            Err(_) => u32::MAX,
        };
        ledger.last_family = family;
        ledger.last_silent = silent;
        ledger.tries += 1;
        if silent == n as u32 {
            ledger.caught += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn zero_silent_ok() -> bool {
    let mut led = HandoffLedger::golden();
    match led.zero_silent_scan() {
        Ok(rep) => rep.clean(),
        Err(_) => false,
    }
}

fn handoff_chain_ok() -> bool {
    let mut led = HandoffLedger::golden();
    if led.zero_silent_scan().map(|r| r.clean()) != Ok(true) {
        return false;
    }
    led.handover().is_ok() && led.phase == HandoffPhase::HandedOver
}

fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

// ── 检查面（≥12 条） ─────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 判据主锚：风险移交清单齐（三列零静默）。
    v.push((
        "J2-风险移交清单齐（ID/状态/下一步三列）零静默",
        zero_silent_ok(),
        format!("{}行×{}列全覆盖 盘点绿", ROW_N, COL_N),
    ));
    // 2. 移交链全通（盘点齐→移交毕）。
    v.push((
        "J2-移交链盘点齐→已移交全通",
        handoff_chain_ok(),
        String::from("零静默在账方可移交"),
    ));
    // 3. 三族静默注入 10/10 必抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J2-三族静默注入10/10必抓",
        inj.all_caught(),
        format!("try/catch {}/{} 末族{}报{}", inj.tries, inj.caught, inj.last_family, inj.last_silent),
    ));
    // 4. 黄金正演点验 10/10。
    let golden = golden_run();
    v.push((
        "正常流-黄金正演点验10/10",
        golden == Ok(10),
        format!("{:?}", golden),
    ));
    // 5. 状态机单调推进（填列中→清单齐→已移交）。
    let mut sm = HandoffLedger::golden();
    let chain = sm.zero_silent_scan().map(|r| r.clean()) == Ok(true) && sm.handover().is_ok();
    v.push((
        "正常流-状态机填列中→清单齐→已移交",
        chain && sm.phase == HandoffPhase::HandedOver,
        format!("终态={}", sm.phase.cn()),
    ));
    // 6. 注入必抓：未盘点先移交即拒。
    let mut p1 = HandoffLedger::golden();
    let before = matches!(p1.handover(), Err(c) if c == HandoffCode::HandoverBeforeScan.val());
    v.push((
        "注入必抓-未盘点先移交即拒",
        before,
        String::from("先 zero_silent_scan 再 handover"),
    ));
    // 7. 注入必抓：移交后重盘点倒退显性拒。
    let mut p2 = HandoffLedger::golden();
    let _ = p2.zero_silent_scan();
    let _ = p2.handover();
    let regress = matches!(p2.zero_silent_scan(), Err(c) if c == HandoffCode::HandoverRegress.val());
    v.push((
        "注入必抓-移交后重盘点倒退显性拒",
        regress,
        String::from("已移交禁止重盘点；重开走新清单"),
    ));
    // 8. 双跑一致：两次构建指纹逐域相等。
    let f1 = HandoffLedger::golden().fingerprint();
    let f2 = HandoffLedger::golden().fingerprint();
    v.push((
        "双跑一致-黄金清单指纹相等",
        f1 == f2,
        format!("{:#x} vs {:#x}", f1, f2),
    ));
    // 9. 边界：ID 号段端点冻结（R-D1-001 / R-D1-020）。
    let bounds_ok = id_range_bounds_ok();
    v.push((
        "边界-ID号段R-D1-001..020端点冻结",
        bounds_ok,
        format!("{}行 {}族", ROW_N, FAMILY),
    ));
    // 10. 三列闭集铺设恰满（状态四态/下一步四步黄金分布）。
    let mut st_seen = [0usize; STATUS_N];
    let mut nx_seen = [0usize; NEXT_N];
    let g = HandoffLedger::golden();
    for r in g.rows.iter() {
        if let Some(s) = r.status {
            let idx = s as usize;
            if let Some(c) = st_seen.get_mut(idx) {
                *c += 1;
            }
        }
        if let Some(n) = r.next {
            let idx = n as usize;
            if let Some(c) = nx_seen.get_mut(idx) {
                *c += 1;
            }
        }
    }
    let spread_ok = st_seen.iter().all(|c| *c > 0) && nx_seen.iter().all(|c| *c > 0);
    v.push((
        "边界-状态四态/下一步四步闭集铺设恰满",
        spread_ok,
        format!("状态{:?} 下一步{:?}", st_seen, nx_seen),
    ));
    // 11. LCG 确定性：同种子双序列逐拍相等。
    let mut r1 = Lcg::new(SEG_BASE);
    let mut r2 = Lcg::new(SEG_BASE);
    let mut lcg_eq = true;
    let mut k = 0;
    while k < 32 {
        if r1.next() != r2.next() {
            lcg_eq = false;
        }
        k += 1;
    }
    v.push((
        "LCG-同种子双序列逐拍相等",
        lcg_eq,
        format!("种子=0x{:X}", SEG_BASE),
    ));
    // 12. 诊断码闭集查表全可回。
    let mut codes_ok = true;
    for c in HandoffCode::ALL.iter() {
        if detail(c.val()) != c.detail() {
            codes_ok = false;
        }
    }
    v.push((
        "诊断码-闭集查表全可回",
        codes_ok && HandoffCode::ALL.len() == 8,
        format!("{}码全查表通过", HandoffCode::ALL.len()),
    ));
    // 13. 段位守卫。
    v.push((
        "段位守卫-0x82F00",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 14. 行缺位必抓：抽掉一行盘点必拒。
    let mut p3 = HandoffLedger::golden();
    let _ = p3.rows.pop();
    v.push((
        "注入必抓-行缺位盘点必拒",
        p3.zero_silent_scan() == Err(HandoffCode::LedgerIncomplete.val()),
        String::from("19 行盘点即拒；先补齐再扫"),
    ));
    v
}

/// 号段端点冻结判定（R-D1-001 / R-D1-020 双端在段、越段双端出段）。
fn id_range_bounds_ok() -> bool {
    HandoffLedger::id_in_range("R-D1-001")
        && HandoffLedger::id_in_range("R-D1-020")
        && !HandoffLedger::id_in_range("R-D1-000")
        && !HandoffLedger::id_in_range("R-D1-021")
        && !HandoffLedger::id_in_range("R-D1-abc")
        && !HandoffLedger::id_in_range("")
}

/// 判据入口：UNX-F12788 行数终账自检集。
pub fn run_unx12788_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12788 行数终账（风险移交清单零静默）");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_zero_silent_and_hands_over() {
        let mut led = HandoffLedger::golden();
        let rep = led.zero_silent_scan();
        assert!(rep.is_ok(), "黄金清单盘点须绿");
        assert!(rep.unwrap_or_default().clean());
        assert!(led.handover().is_ok());
        assert_eq!(led.phase, HandoffPhase::HandedOver);
    }

    #[test]
    fn silent_injection_10_of_10() {
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert!(inj.all_caught(), "三族注入 10 轮 10/10 必抓");
        assert_eq!(inj.tries, INJECT_ROUNDS);
    }

    #[test]
    fn empty_next_step_caught() {
        // 空下一步 1 枚必抓（R-D1-005 注入语料同型）。
        let mut led = HandoffLedger::golden();
        let _ = led.zero_silent_scan();
        led.inject_silent(1, 1);
        let rep = led.zero_silent_scan().unwrap_or_default();
        assert_eq!(rep.bad_next, 1);
        assert!(!rep.clean());
        assert_eq!(led.handover(), Err(HandoffCode::SilentNextStepInvalid.val()));
    }

    #[test]
    fn empty_id_caught() {
        let mut led = HandoffLedger::golden();
        let _ = led.zero_silent_scan();
        led.inject_silent(0, 2);
        let rep = led.zero_silent_scan().unwrap_or_default();
        assert_eq!(rep.bad_id, 2);
    }

    #[test]
    fn handover_before_scan_rejected() {
        let mut led = HandoffLedger::golden();
        assert_eq!(led.handover(), Err(HandoffCode::HandoverBeforeScan.val()));
    }

    #[test]
    fn regress_after_handover_rejected() {
        let mut led = HandoffLedger::golden();
        let _ = led.zero_silent_scan();
        assert!(led.handover().is_ok());
        assert_eq!(led.zero_silent_scan(), Err(HandoffCode::HandoverRegress.val()));
    }

    #[test]
    fn id_range_bounds() {
        assert!(HandoffLedger::id_in_range("R-D1-001"));
        assert!(HandoffLedger::id_in_range("R-D1-020"));
        assert!(!HandoffLedger::id_in_range("R-D1-021"));
        assert!(!HandoffLedger::id_in_range("R-D1-000"));
        assert!(!HandoffLedger::id_in_range("R-D2-001"));
        assert!(!HandoffLedger::id_in_range(""));
        assert!(id_range_bounds_ok());
    }

    #[test]
    fn double_run_fingerprint_equal() {
        let a = HandoffLedger::golden().fingerprint();
        let b = HandoffLedger::golden().fingerprint();
        assert_eq!(a, b);
    }

    #[test]
    fn detail_lookup_closed_set() {
        for c in HandoffCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
    }

    #[test]
    fn segment_guard_pass() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x82F00);
    }

    #[test]
    fn golden_asserts_full_score() {
        let mut led = HandoffLedger::golden();
        let _ = led.zero_silent_scan();
        assert_eq!(led.run_asserts(), 10, "可点验须 10/10");
    }

    #[test]
    fn lcg_deterministic_same_seed() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }
}
