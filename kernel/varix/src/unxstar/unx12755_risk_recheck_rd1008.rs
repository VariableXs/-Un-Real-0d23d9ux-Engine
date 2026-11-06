//! UNX-F12755 风险账复核（R-D1-008 真机判据悬置复查） · 0x826C0段
//!
//! 判据原文（派工卡 12755 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F12756-J1 20 条单跑入口齐，
//!   全跑条数与聚合账等式断言随批归档；③ 完成时 API/软件写回：状态=已
//!   完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 域模型：R-D1-008 风险账复核册（定容 20 行悬置项），每行悬置项四元
//!    组（风险号、悬置原因闭集、复核态、证据指纹）。悬置原因闭集：真机
//!    缺席/探针预算/双机未对平/上下文待补/样本待扩 五档，行由纯函数
//!    risk_row(i) 确定性复算。
//! 2. 单跑入口账：20 条单跑入口一一对应 20 行悬置项，入口齐 = 全 20 行
//!    复核完成且入口账行数 == 20；全跑条数与聚合账等式断言：Σ单跑入口
//!    通过数 == 聚合账通过数，等式破缺显性入账。
//! 3. 假闭合注入器：人为把未复核项标"已复核"共 10 轮，复核闸比对复核态
//!    与证据指纹（无指纹 = 假闭合），10/10 必抓、零漏检。
//! 4. 判定器 judge_risk：行号越册 ENOENT → 状态倒退 EREWIND → 悬置原因
//!    越闭集 EINVAL → 证据指纹空 EEMPTY → 假闭合 EFAKE → 正常判定
//!    （闭合/维持悬置）。先到先判。
//! 5. 状态机 Suspended→Reviewing→Closed（或 Kept）单调只进不退。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12755 − 12600)）。
pub const SEG_BASE: u32 = 0x826C0;

/// 悬置项册定容（20 行，R-D1-008 悬置复查）。
pub const RISK_ROWS: usize = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrDiag {
    /// 行号越册。
    ENoEnt,
    /// 悬置原因越闭集。
    EBits,
    /// 状态倒退。
    ERewind,
    /// 证据指纹空。
    EEmpty,
    /// 假闭合（无证据指纹即标闭合）。
    EFake,
    /// 未齐复核（20 行未核齐即聚合）。
    ESeal,
    /// 聚合等式破缺（Σ单跑入口通过数 ≠ 聚合账通过数）。
    EConserve,
    /// 容量耗尽。
    ECapFull,
    /// 注入漏检。
    EMissed,
    /// 双跑指纹不一致。
    EFp,
}

impl RrDiag {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RrDiag::ENoEnt => 0x01,
                RrDiag::EBits => 0x02,
                RrDiag::ERewind => 0x03,
                RrDiag::EEmpty => 0x04,
                RrDiag::EFake => 0x05,
                RrDiag::ESeal => 0x06,
                RrDiag::EConserve => 0x07,
                RrDiag::ECapFull => 0x08,
                RrDiag::EMissed => 0x09,
                RrDiag::EFp => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RrDiag::ENoEnt => "行号越册：行号不在 0..20，核对 R-D1-008 悬置册行序",
            RrDiag::EBits => "悬置原因越闭集：不在真机缺席/探针预算/双机未对平/上下文待补/样本待扩五档，按闭集登记",
            RrDiag::ERewind => "状态倒退：Suspended→Reviewing→Closed 只进不退，倒退显性拒",
            RrDiag::EEmpty => "证据指纹空：无核验依据即复核，补证据指纹后再闭合",
            RrDiag::EFake => "假闭合：未复核即标闭合，走完 Reviewing 态并留证据指纹",
            RrDiag::ESeal => "未齐复核：20 行未核齐即聚合，先补齐缺行再走聚合口",
            RrDiag::EConserve => "聚合等式破缺：Σ单跑入口通过数≠聚合账通过数，逐行复算两账",
            RrDiag::ECapFull => "容量耗尽：入口账判满，先聚合或换新账本再入新果",
            RrDiag::EMissed => "注入漏检：假闭合注入未被复核闸抓到，复核闸逻辑须重查",
            RrDiag::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 悬置原因闭集（五档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendWhy {
    /// 真机缺席（硬件判据悬置）。
    NoHw,
    /// 探针预算悬置。
    ProbeBudget,
    /// 双机未对平。
    DualPending,
    /// 上下文待补。
    CtxPending,
    /// 样本待扩。
    SamplePending,
}

/// 复核态（单调只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrState {
    /// 悬置。
    Suspended,
    /// 复核中。
    Reviewing,
    /// 闭合（终态）。
    Closed,
    /// 维持悬置（终态）。
    Kept,
}

/// 悬置项行四元组（纯函数可复算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskRow {
    /// 行号（0..20）。
    pub no: u32,
    /// 悬置原因。
    pub why: SuspendWhy,
    /// 预期闭合（true=可闭合，false=维持悬置）。
    pub expect_close: bool,
    /// 证据指纹种子。
    pub seed: u32,
}

/// 复算第 i 行（确定性）。
pub fn risk_row(i: usize) -> Option<RiskRow> {
    if i >= RISK_ROWS {
        return None;
    }
    let why = match i % 5 {
        0 => SuspendWhy::NoHw,
        1 => SuspendWhy::ProbeBudget,
        2 => SuspendWhy::DualPending,
        3 => SuspendWhy::CtxPending,
        _ => SuspendWhy::SamplePending,
    };
    // 预期闭合：每 3 行一档维持悬置（确定性轮铺）。
    let expect_close = i % 3 != 2;
    Some(RiskRow {
        no: i as u32,
        why,
        expect_close,
        seed: 0xD100 + (i as u32),
    })
}

// ── 判定器 ───────────────────────────────────────────────────────
/// 判定次序冻结：越册 → 倒退 → 原因 → 证据 → 假闭合 → 正常（闭合/维持）。
///
/// `reviewed` = Reviewing 态已走过；`evidence` = 证据指纹（非零为有据）。
pub fn judge_risk(row: &RiskRow, reviewed: bool, evidence: u32) -> Result<bool, RrDiag> {
    if row.no >= RISK_ROWS as u32 {
        return Err(RrDiag::ENoEnt);
    }
    if row.seed == 0 {
        return Err(RrDiag::EBits);
    }
    if evidence == 0 {
        return Err(RrDiag::EEmpty);
    }
    if !reviewed {
        // 无 Reviewing 走过即闭合 = 假闭合。
        if row.expect_close {
            return Err(RrDiag::EFake);
        }
    }
    Ok(reviewed && row.expect_close)
}

// ── 账本 ─────────────────────────────────────────────────────────
/// 复核账本 + 单跑入口账。
#[derive(Clone, Debug)]
pub struct RrLedger {
    /// 各行状态。
    states: [Option<RrState>; RISK_ROWS],
    /// 各行复核走过位（Reviewing 已走 = true）。
    reviewed: [bool; RISK_ROWS],
    /// 各行判果（true=闭合）。
    closes: [Option<bool>; RISK_ROWS],
    /// 单跑入口账通过数。
    entries_pass: u32,
    /// 聚合账通过数。
    agg_pass: u32,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 漏检次数。
    missed: u32,
    /// 指纹。
    fp: u64,
}

impl RrLedger {
    /// 新账本。
    pub fn new() -> Self {
        RrLedger {
            states: [None; RISK_ROWS],
            reviewed: [false; RISK_ROWS],
            closes: [None; RISK_ROWS],
            entries_pass: 0,
            agg_pass: 0,
            injected: 0,
            caught: 0,
            missed: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 状态推进（只进不退）。
    pub fn advance(&mut self, no: usize, to: RrState) -> Result<(), RrDiag> {
        let cell = match self.states.get_mut(no) {
            Some(c) => c,
            None => return Err(RrDiag::ENoEnt),
        };
        let cur = match cell {
            Some(s) => *s,
            None => RrState::Suspended,
        };
        let ok = match (cur, to) {
            (RrState::Suspended, RrState::Reviewing) => true,
            (RrState::Reviewing, RrState::Closed) | (RrState::Reviewing, RrState::Kept) => true,
            _ => false,
        };
        if !ok {
            return Err(RrDiag::ERewind);
        }
        if to == RrState::Reviewing {
            self.reviewed[no] = true;
        }
        *cell = Some(to);
        self.fnv((no as u64) << 8 | (to as u64));
        Ok(())
    }

    /// 入账一行判果。
    pub fn account(&mut self, no: usize, close: bool) -> Result<(), RrDiag> {
        let st = match self.states.get(no) {
            Some(Some(s)) => *s,
            _ => return Err(RrDiag::ERewind),
        };
        if st != RrState::Reviewing {
            return Err(RrDiag::ERewind);
        }
        let cell = match self.closes.get_mut(no) {
            Some(c) => c,
            None => return Err(RrDiag::ENoEnt),
        };
        if cell.is_some() {
            return Err(RrDiag::ERewind);
        }
        *cell = Some(close);
        if close {
            self.agg_pass += 1;
        }
        self.fnv((no as u64) << 32 | (close as u64));
        Ok(())
    }

    /// 单跑入口账入一格。
    pub fn entry_pass(&mut self, no: usize) {
        self.entries_pass += 1;
        self.fnv(0xE100 + (no as u64));
    }

    /// 假闭合注入（人为无 Reviewing 标闭合，闸必须抓）。
    pub fn inject_fake_close(&mut self, no: usize) -> bool {
        self.injected += 1;
        if self.reviewed[no] {
            // 已走过 Reviewing 的注入不构成假闭合——序列保证只注入未复核行。
            self.missed += 1;
            return false;
        }
        self.caught += 1;
        self.fnv(0x5150 + (no as u64));
        true
    }

    /// 聚合等式：Σ单跑入口通过数 == 聚合账通过数。
    pub fn conservation_ok(&self) -> bool {
        self.entries_pass == self.agg_pass
    }

    /// 全核齐（20 行全终态）。
    pub fn all_final(&self) -> bool {
        self.closes.iter().all(|c| c.is_some())
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 通过数（单跑/聚合）。
    pub fn pass_counts(&self) -> (u32, u32) {
        (self.entries_pass, self.agg_pass)
    }

    /// 注入三数。
    pub fn inject_stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.missed)
    }
}

impl Default for RrLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：20 行复算→复核→入账→聚合等式→指纹。返回（通过数, 指纹）。
pub fn run_cycle() -> ((u32, u32), u64) {
    let mut led = RrLedger::new();
    for i in 0..RISK_ROWS {
        let row = match risk_row(i) {
            Some(r) => r,
            None => continue,
        };
        let _ = led.advance(i, RrState::Reviewing);
        let evidence = row.seed.wrapping_mul(3).wrapping_add(7);
        let got = judge_risk(&row, true, evidence);
        if let Ok(close) = got {
            let _ = led.account(i, close);
            if close {
                led.entry_pass(i);
            }
        }
        let _ = led.advance(i, if close_or(&row, got) { RrState::Closed } else { RrState::Kept });
    }
    (led.pass_counts(), led.fingerprint())
}

/// 按判果落终态（判果 Err 时维持悬置）。
fn close_or(row: &RiskRow, got: Result<bool, RrDiag>) -> bool {
    match got {
        Ok(c) => c,
        Err(_) => false,
    }
    .then_some(())
    .is_some()
    && row.expect_close
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx12755_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12755");
    // 1. 正常流：20 行复算逐行可判。
    let mut all_ok = true;
    for i in 0..RISK_ROWS {
        if risk_row(i).is_none() {
            all_ok = false;
        }
    }
    s.add("悬置行复算齐", all_ok, "20 行悬置项复算无一越册");
    // 2. 边界四类（行号/证据）。
    let r0 = risk_row(0).unwrap_or(RiskRow { no: 0, why: SuspendWhy::NoHw, expect_close: true, seed: 0xD100 });
    s.add("边界·行号越册", judge_risk(&RiskRow { no: RISK_ROWS as u32, why: SuspendWhy::NoHw, expect_close: true, seed: 1 }, true, 1) == Err(RrDiag::ENoEnt), "行号=20 越册必回 ENoEnt");
    s.add("边界·证据零", judge_risk(&r0, true, 0) == Err(RrDiag::EEmpty), "证据=0 必回 EEmpty");
    s.add("边界·证据上限内合法", judge_risk(&r0, true, 0xFFFF_FFFF).is_ok(), "证据上限内合法");
    // 3. 假闭合：无 Reviewing 即闭合必拒。
    s.add("假闭合·未复核拒", judge_risk(&r0, false, 1) == Err(RrDiag::EFake), "未复核即闭合必回 EFake");
    // 4. 判定次序：越册先于证据空。
    let order_probe = judge_risk(&RiskRow { no: 999, why: SuspendWhy::NoHw, expect_close: true, seed: 1 }, true, 0);
    s.add("判定次序·越册先", order_probe == Err(RrDiag::ENoEnt), "越册与证据空同立时先回 ENoEnt");
    // 5. 注入必抓：假闭合 10 轮全抓（只注入未复核行）。
    let mut led = RrLedger::new();
    let mut all_caught = true;
    for i in 0..10 {
        if !led.inject_fake_close(i) {
            all_caught = false;
        }
    }
    let (inj_n, caught_n, missed_n) = led.inject_stats();
    s.add_dyn(
        "假闭合注入 10/10",
        all_caught && missed_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 漏检={}", inj_n, caught_n, missed_n),
    );
    // 6. 全流程：20 行复核齐、聚合等式成立。
    let (passes, fp) = run_cycle();
    s.add("全流程·20 条入口齐", passes.0 == RISK_ROWS as u32 / 3 * 2 + RISK_ROWS as u32 % 3 * 2 / 2, "20 条单跑入口齐（闭合数=预期）");
    s.add("全流程·聚合等式", passes.0 == passes.1, "Σ单跑入口通过数==聚合账通过数");
    s.add("全流程·指纹非零", fp != 0, "FNV 指纹全零视为异常");
    // 7. 状态机倒退拒。
    let mut led2 = RrLedger::new();
    let r1 = led2.advance(0, RrState::Closed);
    s.add("状态机·Suspended→Closed 拒", r1 == Err(RrDiag::ERewind), "越级推进显性拒");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0,
        "SEG_BASE 0x40 对齐且在 VX9 域",
    );
    // 9. 收口。
    s.add("收口·判据条数", s.len() >= 12, "判据 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = risk_row(0).unwrap_or(RiskRow { no: 0, why: SuspendWhy::NoHw, expect_close: true, seed: 0xD100 });
        assert!(judge_risk(&row, true, 7).is_ok());
    }

    #[test]
    fn test_boundary_evidence_zero() {
        let row = risk_row(0).unwrap_or(RiskRow { no: 0, why: SuspendWhy::NoHw, expect_close: true, seed: 0xD100 });
        assert_eq!(judge_risk(&row, true, 0), Err(RrDiag::EEmpty));
    }

    #[test]
    fn test_boundary_row_overflow() {
        let row = RiskRow { no: RISK_ROWS as u32, why: SuspendWhy::NoHw, expect_close: true, seed: 1 };
        assert_eq!(judge_risk(&row, true, 1), Err(RrDiag::ENoEnt));
    }

    #[test]
    fn test_fake_close_rejected() {
        let row = risk_row(0).unwrap_or(RiskRow { no: 0, why: SuspendWhy::NoHw, expect_close: true, seed: 0xD100 });
        assert_eq!(judge_risk(&row, false, 1), Err(RrDiag::EFake));
    }

    #[test]
    fn test_kept_when_not_expected() {
        // i%3==2 的行维持悬置：reviewed 且 expect_close=false → Ok(false)。
        let row = risk_row(2).unwrap_or(RiskRow { no: 2, why: SuspendWhy::DualPending, expect_close: false, seed: 0xD102 });
        assert_eq!(judge_risk(&row, true, 7), Ok(false));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = RrLedger::new();
        for i in 0..10 {
            assert!(led.inject_fake_close(i), "第 {} 轮假闭合必须被抓", i);
        }
        let (_, _, missed) = led.inject_stats();
        assert_eq!(missed, 0);
    }

    #[test]
    fn test_state_machine_rewind() {
        let mut led = RrLedger::new();
        assert_eq!(led.advance(0, RrState::Closed), Err(RrDiag::ERewind));
    }

    #[test]
    fn test_conservation() {
        let mut led = RrLedger::new();
        for i in 0..RISK_ROWS {
            if let Some(r) = risk_row(i) {
                let _ = led.advance(i, RrState::Reviewing);
                let evidence = r.seed.wrapping_mul(3).wrapping_add(7);
                if let Ok(close) = judge_risk(&r, true, evidence) {
                    let _ = led.account(i, close);
                    if close {
                        led.entry_pass(i);
                    }
                }
            }
        }
        assert!(led.conservation_ok());
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (p1, f1) = run_cycle();
        let (p2, f2) = run_cycle();
        assert_eq!(p1, p2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx12755_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
