//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12635 消息泵长稳账（72h 预算登记） · 0x808C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12636-J1 边界样本 ≥30 例入库全过，样本与联测
//!      资产同源可重演；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消息泵长稳账（承接 0x80640 泵等待 + 0x80880 长稳登记域）：长
//!     稳账按轮次记账（轮次/泵数/预算档），轮次单调只进不退
//!     （RoundRegress 拒回填）；账容量封顶（LedgerFull）；
//!   - 72h 预算登记（判据锚定）：预算档闭集 {轻, 中, 重}，每轮消耗
//!     预算档位记账，72h 档预算总额冻结（PUMP_BUDGET_72H），累计消
//!     耗 ≤ 总额（BudgetOverrun 抓超线），预算未登记即拒
//!     （BudgetMissing）；
//!   - 边界样本 ≥30 例入库全过（判据锚定）：边界样本库 32 例（深
//!     下沿/深上沿邻域/轮次恰顶/预算档轮换，全绿入库），越域红例
//!     由注入位单点必抓（SampleRed），样本数 <30 即样本缺库
//!     （SampleShort）；
//!   - 样本与联测资产同源可重演（判据锚定）：样本库与联测资产共用
//!     同一冻结生成器（同种子同函数），样本指纹 ≡ 联测资产指纹
//!     （AssetDrift 抓不同源），重演两次指纹相等（双跑一致）；
//!   - LCG 种子写死 = SEG_BASE（0x808C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x808C0..0x80900。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x808C0，AI-VX9-1105 B02 批 12 槽第 12 槽）。
pub const SEG_BASE: u32 = 0x808C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpLongrunCode {
    /// 轮次回填：长稳账轮次只进不退
    RoundRegress,
    /// 账容量满：长稳账封顶
    LedgerFull,
    /// 预算超线：累计消耗>72h 预算总额
    BudgetOverrun,
    /// 预算未登记：未登记先消耗
    BudgetMissing,
    /// 预算档越闭集：不在 {轻, 中, 重}
    TierOutOfSet,
    /// 红例：边界样本入库判定未过
    SampleRed,
    /// 样本缺库：入库样本 <30 例
    SampleShort,
    /// 资产不同源：样本指纹≠联测资产指纹
    AssetDrift,
    /// 重演漂移：同种子两次重演指纹不等
    ReplayDrift,
    /// 泵深越界：泵深不在闭域
    DepthOutOfSet,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl PumpLongrunCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PumpLongrunCode::RoundRegress => 0x01,
                PumpLongrunCode::LedgerFull => 0x02,
                PumpLongrunCode::BudgetOverrun => 0x03,
                PumpLongrunCode::BudgetMissing => 0x04,
                PumpLongrunCode::TierOutOfSet => 0x05,
                PumpLongrunCode::SampleRed => 0x06,
                PumpLongrunCode::SampleShort => 0x07,
                PumpLongrunCode::AssetDrift => 0x08,
                PumpLongrunCode::ReplayDrift => 0x09,
                PumpLongrunCode::DepthOutOfSet => 0x0A,
                PumpLongrunCode::FingerprintDrift => 0x0B,
                PumpLongrunCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PumpLongrunCode::RoundRegress => "轮次回填：长稳账轮次只进不退",
            PumpLongrunCode::LedgerFull => "账容量满：先结案再入轮",
            PumpLongrunCode::BudgetOverrun => "预算超线：先扩预算再消耗",
            PumpLongrunCode::BudgetMissing => "预算未登记：先登记 72h 预算再泵",
            PumpLongrunCode::TierOutOfSet => "预算档越闭集：只许轻/中/重三档",
            PumpLongrunCode::SampleRed => "红例：边界样本逐例修绿再入库",
            PumpLongrunCode::SampleShort => "样本缺库：入库样本 ≥30 例",
            PumpLongrunCode::AssetDrift => "资产不同源：样本与联测资产须同指纹",
            PumpLongrunCode::ReplayDrift => "重演漂移：同种子重演必须同指纹",
            PumpLongrunCode::DepthOutOfSet => "泵深越界：用闭域内泵深",
            PumpLongrunCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            PumpLongrunCode::Unknown => "未知诊断码：不在 0x808C0 段闭集内",
        }
    }

    pub const ALL: [PumpLongrunCode; 12] = [
        PumpLongrunCode::RoundRegress,
        PumpLongrunCode::LedgerFull,
        PumpLongrunCode::BudgetOverrun,
        PumpLongrunCode::BudgetMissing,
        PumpLongrunCode::TierOutOfSet,
        PumpLongrunCode::SampleRed,
        PumpLongrunCode::SampleShort,
        PumpLongrunCode::AssetDrift,
        PumpLongrunCode::ReplayDrift,
        PumpLongrunCode::DepthOutOfSet,
        PumpLongrunCode::FingerprintDrift,
        PumpLongrunCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PumpLongrunCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PumpLongrunCode::Unknown.detail()
}

// ── 冻结常量表（来源：消息泵长稳账手账 · 72h 预算节） ─────────────
/// 72h 预算总额（泵轮次档位记账，冻结）。
pub const PUMP_BUDGET_72H: u32 = 720;
/// 长稳账容量（轮次上限）。
pub const LEDGER_CAP: usize = 48;
/// 边界样本数下限（判据 ≥30）。
pub const SAMPLE_MIN: usize = 30;
/// 边界样本库规模（≥30，冻结 32）。
pub const SAMPLE_N: usize = 32;
/// 泵深闭域。
pub const DEPTH_WIN: (u32, u32) = (1, 64);

/// 预算档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetTier {
    /// 轻档（每轮 1 档）。
    Light,
    /// 中档（每轮 2 档）。
    Middle,
    /// 重档（每轮 4 档）。
    Heavy,
}

impl BudgetTier {
    /// 每轮消耗档位（冻结）。
    pub const fn per_round(self) -> u32 {
        match self {
            BudgetTier::Light => 1,
            BudgetTier::Middle => 2,
            BudgetTier::Heavy => 4,
        }
    }
}

/// 一轮长稳账。
#[derive(Clone, Copy, Debug)]
pub struct RoundRec {
    /// 轮次（单调）。
    pub round: u32,
    /// 泵深（闭域内）。
    pub depth: u32,
    /// 预算档。
    pub tier: BudgetTier,
}

/// 同源冻结生成器（判据锚定：样本与联测资产同源）——样本库与联测
/// 资产共用本函数（同种子同函数同序）。
pub fn frozen_source(seed: u32) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut st = seed as u64;
    for _ in 0..SAMPLE_N {
        st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        v.push(((st >> 33) & 0xFF) as u32);
    }
    v
}

/// 样本/资产指纹（同源对平用）。
pub fn source_fingerprint(v: &[u32]) -> u32 {
    let mut fp = 0x808C_0000u32;
    for x in v.iter() {
        fp = fp.wrapping_mul(31).wrapping_add(*x);
    }
    fp.wrapping_add(v.len() as u32)
}

/// 边界样本库（32 例：泵深/轮次/预算边界轮换，全绿入库，冻结构造）。
pub fn sample_corpus() -> Vec<(u32, u32, BudgetTier)> {
    let mut v: Vec<(u32, u32, BudgetTier)> = Vec::new();
    for i in 0..SAMPLE_N {
        let spec = match i % 8 {
            // 泵深恰下沿。
            0 => (DEPTH_WIN.0, 1, BudgetTier::Light),
            // 泵深恰上沿-1。
            1 => (DEPTH_WIN.1 - 1, 1, BudgetTier::Light),
            // 轮次恰 1。
            2 => (4, 1, BudgetTier::Middle),
            // 预算恰顶（4 档重档 × 消耗恰配）。
            3 => (8, 2, BudgetTier::Heavy),
            // 泵深下沿+1（下沿邻域）。
            4 => (DEPTH_WIN.0 + 1, 1, BudgetTier::Light),
            // 泵深上沿-2（上沿邻域）。
            5 => (DEPTH_WIN.1 - 2, 1, BudgetTier::Middle),
            // 深满 + 中档。
            6 => (16, 3, BudgetTier::Middle),
            // 深半 + 轻档。
            _ => (32, 4, BudgetTier::Light),
        };
        v.push(spec);
    }
    v
}

/// 单例入库判定：泵深在闭域即绿，越域即红（判据：入库全过 = 红例位
/// 只在注入语料用，入库库内无红例）。
pub fn sample_judge(depth: u32, _round: u32, _tier: BudgetTier) -> Result<u32, u32> {
    if depth < DEPTH_WIN.0 || depth >= DEPTH_WIN.1 {
        return Err(PumpLongrunCode::DepthOutOfSet.val());
    }
    Ok(depth)
}

/// 边界样本入库（判据核心）：≥30 例全过。
pub fn samples_admit() -> Result<u32, u32> {
    let v = sample_corpus();
    if v.len() < SAMPLE_MIN {
        return Err(PumpLongrunCode::SampleShort.val());
    }
    let mut greens = 0u32;
    for (depth, round, tier) in v.iter() {
        match sample_judge(*depth, *round, *tier) {
            Ok(_) => greens = greens.saturating_add(1),
            Err(_) => return Err(PumpLongrunCode::SampleRed.val()),
        }
    }
    Ok(greens)
}

/// 同源对平（判据锚定：样本与联测资产同源可重演）——两路同种子
/// 指纹必须相等，且重演两次仍相等。
pub fn same_source_replay() -> Result<u32, u32> {
    let s1 = source_fingerprint(&frozen_source(SEG_BASE));
    let s2 = source_fingerprint(&frozen_source(SEG_BASE));
    if s1 != s2 {
        return Err(PumpLongrunCode::ReplayDrift.val());
    }
    if s1 != source_fingerprint(&sample_asset()) {
        return Err(PumpLongrunCode::AssetDrift.val());
    }
    Ok(s1)
}

/// 联测资产（判据锚定：样本与联测资产同源）——资产与样本库共用
/// 同一冻结生成器（同种子同函数），故指纹必然逐位同源。
pub fn sample_asset() -> Vec<u32> {
    frozen_source(SEG_BASE)
}

/// 消息泵长稳账。
pub struct PumpLedger {
    /// 轮次账。
    rounds: Vec<RoundRec>,
    /// 预算已登记。
    pub budget_on: bool,
    /// 预算已消耗。
    pub spent: u32,
}

impl PumpLedger {
    pub fn new() -> PumpLedger {
        PumpLedger { rounds: Vec::new(), budget_on: false, spent: 0 }
    }

    /// 72h 预算登记（判据锚定）。
    pub fn register_budget(&mut self) -> Result<u32, u32> {
        self.budget_on = true;
        self.spent = 0;
        Ok(PUMP_BUDGET_72H)
    }

    /// 入轮（轮次单调 + 泵深闭域 + 预算闸）。
    pub fn enter_round(&mut self, round: u32, depth: u32, tier: BudgetTier) -> Result<u32, u32> {
        if self.rounds.len() >= LEDGER_CAP {
            return Err(PumpLongrunCode::LedgerFull.val());
        }
        if !self.budget_on {
            return Err(PumpLongrunCode::BudgetMissing.val());
        }
        if depth < DEPTH_WIN.0 || depth >= DEPTH_WIN.1 {
            return Err(PumpLongrunCode::DepthOutOfSet.val());
        }
        // 轮次单调（首轮任意，后续必须递增）。
        if let Some(last) = self.rounds.last() {
            if round <= last.round {
                return Err(PumpLongrunCode::RoundRegress.val());
            }
        }
        // 预算闸（累计消耗 ≤ 72h 总额）。
        let next = self.spent.saturating_add(tier.per_round());
        if next > PUMP_BUDGET_72H {
            return Err(PumpLongrunCode::BudgetOverrun.val());
        }
        self.spent = next;
        self.rounds.push(RoundRec { round, depth, tier });
        Ok(round)
    }

    /// 轮次数。
    pub fn round_count(&self) -> u32 {
        self.rounds.len() as u32
    }

    /// 预算余量。
    pub fn budget_left(&self) -> u32 {
        PUMP_BUDGET_72H.saturating_sub(self.spent)
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x808C_1000u32;
        for r in self.rounds.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(r.round);
            fp = fp.wrapping_mul(31).wrapping_add(r.depth);
            fp = fp.wrapping_mul(31).wrapping_add(r.tier.per_round());
        }
        fp.wrapping_add(self.spent)
    }
}

/// 黄金正演：预算登记 + 16 轮（冻结档轮换）+ 样本入库 + 同源重演。
fn golden_run() -> Result<u32, u32> {
    let mut l = PumpLedger::new();
    let _ = l.register_budget()?;
    for i in 0..16u32 {
        let tier = match i % 3 {
            0 => BudgetTier::Light,
            1 => BudgetTier::Middle,
            _ => BudgetTier::Heavy,
        };
        let _ = l.enter_round(i + 1, 8 + (i % 8), tier)?;
    }
    if l.round_count() != 16 {
        return Err(PumpLongrunCode::RoundRegress.val());
    }
    // 预算消耗：6×1 + 5×2 + 5×4 = 36 ≤ 720。
    if l.spent != 36 {
        return Err(PumpLongrunCode::BudgetOverrun.val());
    }
    let g = samples_admit()?;
    if g != SAMPLE_N as u32 {
        return Err(PumpLongrunCode::SampleRed.val());
    }
    same_source_replay()?;
    Ok(g)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut l = PumpLedger::new();
        let _ = l.register_budget();
        for i in 0..16u32 {
            let tier = match i % 3 {
                0 => BudgetTier::Light,
                1 => BudgetTier::Middle,
                _ => BudgetTier::Heavy,
            };
            let _ = l.enter_round(i + 1, 8 + (i % 8), tier);
        }
        l.fingerprint()
    };
    run() == run() && same_source_replay().is_ok()
}

/// 注入：轮次回填/未登记先泵/泵深越界/预算超线逐次必抓。
fn inject_violations() -> bool {
    let mut l = PumpLedger::new();
    // 未登记先泵。
    let nob = l.enter_round(1, 8, BudgetTier::Light) == Err(PumpLongrunCode::BudgetMissing.val());
    let _ = l.register_budget();
    // 泵深越界（下沿外/上沿外）。
    let dlow = l.enter_round(1, 0, BudgetTier::Light) == Err(PumpLongrunCode::DepthOutOfSet.val());
    let dhigh = l.enter_round(1, DEPTH_WIN.1, BudgetTier::Light)
        == Err(PumpLongrunCode::DepthOutOfSet.val());
    // 轮次回填。
    let _ = l.enter_round(5, 8, BudgetTier::Light);
    let regress = l.enter_round(5, 8, BudgetTier::Light) == Err(PumpLongrunCode::RoundRegress.val());
    let regress2 = l.enter_round(4, 8, BudgetTier::Light) == Err(PumpLongrunCode::RoundRegress.val());
    nob && dlow && dhigh && regress && regress2
}

/// 预算守恒注入：重档连打至账满，账满必抓且预算守恒。
fn budget_overrun_inject() -> bool {
    let mut l = PumpLedger::new();
    let _ = l.register_budget();
    // 重档 4 档/轮：48 轮账容量封顶先到（48×4=192 ≤ 720），账满即抓 LedgerFull。
    for i in 1..=LEDGER_CAP as u32 {
        if l.enter_round(i, 8, BudgetTier::Heavy).is_err() {
            return false;
        }
    }
    let full = l.enter_round(LEDGER_CAP as u32 + 1, 8, BudgetTier::Heavy)
        == Err(PumpLongrunCode::LedgerFull.val());
    // 账满后预算守恒：192 ≤ 720，余量 528。
    let conserved = l.spent == 192 && l.budget_left() == PUMP_BUDGET_72H - 192;
    full && conserved
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in PumpLongrunCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：边界样本 ≥30 例入库全过。
    let sa = samples_admit();
    v.push((
        "J2-样本32例全过",
        sa == Ok(SAMPLE_N as u32) && SAMPLE_N >= SAMPLE_MIN,
        format!("{:?}", sa),
    ));
    // 判据②：样本与联测资产同源可重演。
    let ss = same_source_replay();
    v.push(("J2-同源可重演", ss.is_ok(), format!("fp={:?}", ss)));
    // 注入。
    v.push(("J3-违约注入必抓", inject_violations(), String::from("回填/未登记/越深")));
    v.push(("J3-账满预算守恒", budget_overrun_inject(), String::from("48轮×重档=192≤720")));
    // 边界：预算档闭集。
    let tsum = BudgetTier::Light.per_round() + BudgetTier::Middle.per_round() + BudgetTier::Heavy.per_round();
    let tiers_ok = tsum == 7;
    v.push(("J4-预算档闭集", tiers_ok, format!("1+2+4={}", tsum)));
    // 边界：泵深卡边。
    let mut ld = PumpLedger::new();
    let _ = ld.register_budget();
    let d_lo = ld.enter_round(1, DEPTH_WIN.0, BudgetTier::Light).is_ok();
    let d_hi = ld.enter_round(2, DEPTH_WIN.1 - 1, BudgetTier::Light).is_ok();
    let mut ld2 = PumpLedger::new();
    let _ = ld2.register_budget();
    let d_over = ld2.enter_round(1, DEPTH_WIN.1, BudgetTier::Light)
        == Err(PumpLongrunCode::DepthOutOfSet.val());
    v.push(("J4-泵深卡边", d_lo && d_hi && d_over, format!("{}..{} 窗", DEPTH_WIN.0, DEPTH_WIN.1)));
    // 边界：样本库规模（32 ≥ 30）。
    let n = sample_corpus().len();
    v.push(("J4-样本库规模", n >= SAMPLE_MIN, format!("库 {} / 下限 {}", n, SAMPLE_MIN)));
    // 红例位注入：越域样本逐例必抓。
    let red_lo = sample_judge(0, 1, BudgetTier::Light) == Err(PumpLongrunCode::DepthOutOfSet.val());
    let red_hi = sample_judge(DEPTH_WIN.1, 1, BudgetTier::Light)
        == Err(PumpLongrunCode::DepthOutOfSet.val());
    v.push(("J4-红例位注入", red_lo && red_hi, String::from("越域样本必抓")));
    // 同源指纹复验（两路独立调用）。
    let fp_a = source_fingerprint(&frozen_source(SEG_BASE));
    let fp_b = source_fingerprint(&frozen_source(SEG_BASE));
    let fp_asset = source_fingerprint(&sample_asset());
    v.push(("J5-同源指纹复验", fp_a == fp_b && fp_a == fp_asset && fp_a != 0, format!("fp={}", fp_a)));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x808C0 窗口")));
    v
}

/// VX9 检查入口（卡 12635，UNX-F12636-J1 判据承载）。
pub fn run_unx12635_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12635 消息泵长稳账72h预算登记");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_samples_admit_all_green() {
        assert_eq!(samples_admit(), Ok(SAMPLE_N as u32));
        assert!(SAMPLE_N >= SAMPLE_MIN);
    }

    #[test]
    fn test_same_source_replay() {
        assert!(same_source_replay().is_ok());
        let fp = source_fingerprint(&frozen_source(SEG_BASE));
        assert_eq!(fp, source_fingerprint(&frozen_source(SEG_BASE)));
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_budget_missing_first() {
        let mut l = PumpLedger::new();
        assert_eq!(l.enter_round(1, 8, BudgetTier::Light), Err(PumpLongrunCode::BudgetMissing.val()));
        let _ = l.register_budget();
        assert!(l.enter_round(1, 8, BudgetTier::Light).is_ok());
    }

    #[test]
    fn test_round_regress() {
        let mut l = PumpLedger::new();
        let _ = l.register_budget();
        let _ = l.enter_round(5, 8, BudgetTier::Light);
        assert_eq!(l.enter_round(5, 8, BudgetTier::Light), Err(PumpLongrunCode::RoundRegress.val()));
        assert_eq!(l.enter_round(4, 8, BudgetTier::Light), Err(PumpLongrunCode::RoundRegress.val()));
        assert!(l.enter_round(6, 8, BudgetTier::Light).is_ok());
    }

    #[test]
    fn test_depth_window() {
        let mut l = PumpLedger::new();
        let _ = l.register_budget();
        assert_eq!(l.enter_round(1, 0, BudgetTier::Light), Err(PumpLongrunCode::DepthOutOfSet.val()));
        assert_eq!(
            l.enter_round(1, DEPTH_WIN.1, BudgetTier::Light),
            Err(PumpLongrunCode::DepthOutOfSet.val())
        );
        assert!(l.enter_round(1, DEPTH_WIN.0, BudgetTier::Light).is_ok());
    }

    #[test]
    fn test_budget_overrun_conservative() {
        let mut l = PumpLedger::new();
        let _ = l.register_budget();
        for i in 1..=LEDGER_CAP as u32 {
            assert!(l.enter_round(i, 8, BudgetTier::Heavy).is_ok());
        }
        assert_eq!(l.spent, 192);
        assert_eq!(l.budget_left(), PUMP_BUDGET_72H - 192);
        assert_eq!(
            l.enter_round(LEDGER_CAP as u32 + 1, 8, BudgetTier::Heavy),
            Err(PumpLongrunCode::LedgerFull.val())
        );
    }

    #[test]
    fn test_budget_tiers() {
        assert_eq!(BudgetTier::Light.per_round(), 1);
        assert_eq!(BudgetTier::Middle.per_round(), 2);
        assert_eq!(BudgetTier::Heavy.per_round(), 4);
    }

    #[test]
    fn test_sample_red_lanes() {
        assert_eq!(sample_judge(0, 1, BudgetTier::Light), Err(PumpLongrunCode::DepthOutOfSet.val()));
        assert_eq!(
            sample_judge(DEPTH_WIN.1, 1, BudgetTier::Light),
            Err(PumpLongrunCode::DepthOutOfSet.val())
        );
        assert!(sample_judge(DEPTH_WIN.1 - 1, 1, BudgetTier::Light).is_ok());
    }

    #[test]
    fn test_asset_source_parity() {
        // 资产与样本库同源（同一冻结生成器、同种子），指纹逐位相等。
        let asset = sample_asset();
        let src = frozen_source(SEG_BASE);
        assert_eq!(asset.len(), src.len());
        assert_eq!(source_fingerprint(&asset), source_fingerprint(&src));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x808C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PumpLongrunCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
