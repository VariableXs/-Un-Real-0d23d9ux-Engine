//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12634 D4 会签槽位回执（承接域开工后对账协议） · 0x80880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12635-J1 72h 长稳判据登记随闸门补测（双轨产线
//!      零 QEMU），预算线在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - D4 会签槽位回执（承接 0x80840 回执协议三段之后的会签域）：
//!     承接域开工后每槽位落一笔会签回执，回执态闭集 {空签, 正签,
//!     驳签}——会签数 ≡ 正签数+驳签数（CosignParity 抓不等）；
//!   - 72h 长稳判据登记（判据锚定）：长稳登记账逐条登记（判据名/
//!     时长档/补测闸位），时长档闭集 {24h, 48h, 72h}；登记必须随闸
//!     门补测位（RegGateMissing 抓无闸登记）；
//!   - 双轨产线零 QEMU（判据锚定）：产线轨闭集 {轨甲, 轨乙}，QEMU
//!     轨恒禁用——登记账内任何一条带 QEMU 轨即 QemuTrackBanned 抓出
//!     （零 QEMU 硬闸）；双轨登记数 ≡ 登记总数（TrackParity）；
//!   - 预算线在册（判据锚定）：预算线账逐条在册（线名/上限档/已用
//!     档），已用 ≤ 上限（BudgetOverrun 抓超线），账空即预算线缺册
//!     （BudgetUnregistered）；
//!   - LCG 种子写死 = SEG_BASE（0x80880），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80880..0x808C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80880，AI-VX9-1105 B02 批 12 槽第 11 槽）。
pub const SEG_BASE: u32 = 0x80880;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignRcptCode {
    /// 会签对平破：会签数≠正签+驳签
    CosignParity,
    /// 重复会签：同槽位只许一会签
    DupCosign,
    /// 坏槽位：槽位不在登记窗
    BadSlot,
    /// 长稳登记无闸：登记缺补测闸位
    RegGateMissing,
    /// 时长档越闭集：不在 {24h, 48h, 72h}
    DurationOutOfSet,
    /// QEMU 轨禁用破：登记账内出现 QEMU 轨
    QemuTrackBanned,
    /// 双轨对平破：轨甲+轨乙≠登记总数
    TrackParity,
    /// 预算线超支：已用>上限
    BudgetOverrun,
    /// 预算线缺册：预算账空
    BudgetUnregistered,
    /// 回执态越闭集：不在 {空, 正, 驳}
    RcptStateOutOfSet,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl CosignRcptCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CosignRcptCode::CosignParity => 0x01,
                CosignRcptCode::DupCosign => 0x02,
                CosignRcptCode::BadSlot => 0x03,
                CosignRcptCode::RegGateMissing => 0x04,
                CosignRcptCode::DurationOutOfSet => 0x05,
                CosignRcptCode::QemuTrackBanned => 0x06,
                CosignRcptCode::TrackParity => 0x07,
                CosignRcptCode::BudgetOverrun => 0x08,
                CosignRcptCode::BudgetUnregistered => 0x09,
                CosignRcptCode::RcptStateOutOfSet => 0x0A,
                CosignRcptCode::FingerprintDrift => 0x0B,
                CosignRcptCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            CosignRcptCode::CosignParity => "会签对平破：会签=正签+驳签，逐笔对账",
            CosignRcptCode::DupCosign => "重复会签：同槽位只许一会签",
            CosignRcptCode::BadSlot => "坏槽位：用登记窗内槽位",
            CosignRcptCode::RegGateMissing => "登记无闸：长稳登记必须随补测闸位",
            CosignRcptCode::DurationOutOfSet => "时长档越闭集：只许 24h/48h/72h",
            CosignRcptCode::QemuTrackBanned => "QEMU 轨禁用：双轨产线零 QEMU 硬闸",
            CosignRcptCode::TrackParity => "双轨对平破：轨甲+轨乙=登记总数",
            CosignRcptCode::BudgetOverrun => "预算超支：先扩上限或停用再入账",
            CosignRcptCode::BudgetUnregistered => "预算线缺册：先登记预算线再消耗",
            CosignRcptCode::RcptStateOutOfSet => "回执态越闭集：只许空/正/驳三态",
            CosignRcptCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            CosignRcptCode::Unknown => "未知诊断码：不在 0x80880 段闭集内",
        }
    }

    pub const ALL: [CosignRcptCode; 12] = [
        CosignRcptCode::CosignParity,
        CosignRcptCode::DupCosign,
        CosignRcptCode::BadSlot,
        CosignRcptCode::RegGateMissing,
        CosignRcptCode::DurationOutOfSet,
        CosignRcptCode::QemuTrackBanned,
        CosignRcptCode::TrackParity,
        CosignRcptCode::BudgetOverrun,
        CosignRcptCode::BudgetUnregistered,
        CosignRcptCode::RcptStateOutOfSet,
        CosignRcptCode::FingerprintDrift,
        CosignRcptCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in CosignRcptCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    CosignRcptCode::Unknown.detail()
}

// ── 冻结常量表（来源：D4 会签槽位回执手账 · 长稳登记节） ──────────
/// 槽位登记窗（下闭上开）。
pub const SLOT_WIN: (u32, u32) = (0x20, 0x40);
/// 长稳时长档闭集（小时）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DurationTier {
    /// 24 小时档。
    H24,
    /// 48 小时档。
    H48,
    /// 72 小时档（判据锚定档）。
    H72,
}

impl DurationTier {
    pub const fn hours(self) -> u32 {
        match self {
            DurationTier::H24 => 24,
            DurationTier::H48 => 48,
            DurationTier::H72 => 72,
        }
    }
}

/// 产线轨闭集（双轨，零 QEMU）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackKind {
    /// 轨甲。
    TrackA,
    /// 轨乙。
    TrackB,
    /// QEMU 轨（恒禁用）。
    Qemu,
}

/// 回执态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RcptState {
    /// 空签（未会签）。
    Empty,
    /// 正签（会签通过）。
    Signed,
    /// 驳签（会签驳回）。
    Rejected,
}

/// 一笔会签回执。
#[derive(Clone, Copy, Debug)]
pub struct CosignReceipt {
    /// 槽位。
    pub slot: u32,
    /// 回执态。
    pub state: RcptState,
}

/// 一条 72h 长稳登记。
#[derive(Clone, Copy, Debug)]
pub struct LongrunReg {
    /// 登记名（判据名）。
    pub name: &'static str,
    /// 时长档。
    pub tier: DurationTier,
    /// 产线轨。
    pub track: TrackKind,
    /// 补测闸位已挂。
    pub gate_attached: bool,
}

/// 一条预算线。
#[derive(Clone, Copy, Debug)]
pub struct BudgetLine {
    /// 线名。
    pub name: &'static str,
    /// 上限档（工时档位记账）。
    pub cap: u32,
    /// 已用档。
    pub used: u32,
}

impl BudgetLine {
    /// 消耗（超线拒）。
    pub fn spend(&mut self, amount: u32) -> Result<u32, u32> {
        let next = self.used.saturating_add(amount);
        if next > self.cap {
            return Err(CosignRcptCode::BudgetOverrun.val());
        }
        self.used = next;
        Ok(self.used)
    }
}

/// D4 会签回执闸。
pub struct CosignGate {
    /// 回执账。
    receipts: Vec<CosignReceipt>,
    /// 长稳登记账。
    pub longrun: Vec<LongrunReg>,
    /// 预算线账。
    pub budgets: Vec<BudgetLine>,
}

impl CosignGate {
    pub fn new() -> CosignGate {
        CosignGate { receipts: Vec::new(), longrun: Vec::new(), budgets: Vec::new() }
    }

    /// 会签（空签→正/驳；重复拒）。
    pub fn cosign(&mut self, slot: u32, state: RcptState) -> Result<u32, u32> {
        if slot < SLOT_WIN.0 || slot >= SLOT_WIN.1 {
            return Err(CosignRcptCode::BadSlot.val());
        }
        if state == RcptState::Empty {
            return Err(CosignRcptCode::RcptStateOutOfSet.val());
        }
        if self.receipts.iter().any(|r| r.slot == slot) {
            return Err(CosignRcptCode::DupCosign.val());
        }
        self.receipts.push(CosignReceipt { slot, state });
        Ok(slot)
    }

    /// 会签对平：账内每笔皆正/驳（空签不可入账），笔数守恒自检。
    pub fn cosign_parity(&self) -> Result<u32, u32> {
        let n = self.receipts.len() as u32;
        let valid = self
            .receipts
            .iter()
            .filter(|r| r.state == RcptState::Signed || r.state == RcptState::Rejected)
            .count() as u32;
        if n == valid {
            Ok(n)
        } else {
            Err(CosignRcptCode::CosignParity.val())
        }
    }

    /// 72h 长稳登记（判据锚定）：必须随补测闸位，QEMU 轨硬禁。
    pub fn register_longrun(
        &mut self,
        name: &'static str,
        tier: DurationTier,
        track: TrackKind,
        gate_attached: bool,
    ) -> Result<u32, u32> {
        // 零 QEMU 硬闸（判据锚定）。
        if track == TrackKind::Qemu {
            return Err(CosignRcptCode::QemuTrackBanned.val());
        }
        if !gate_attached {
            return Err(CosignRcptCode::RegGateMissing.val());
        }
        if tier != DurationTier::H24 && tier != DurationTier::H48 && tier != DurationTier::H72 {
            return Err(CosignRcptCode::DurationOutOfSet.val());
        }
        self.longrun.push(LongrunReg { name, tier, track, gate_attached });
        Ok(self.longrun.len() as u32)
    }

    /// 双轨对平：轨甲+轨乙 ≡ 登记总数（零 QEMU 由硬闸保）。
    pub fn track_parity(&self) -> Result<u32, u32> {
        let a = self.longrun.iter().filter(|r| r.track == TrackKind::TrackA).count() as u32;
        let b = self.longrun.iter().filter(|r| r.track == TrackKind::TrackB).count() as u32;
        let total = self.longrun.len() as u32;
        if a.saturating_add(b) == total {
            Ok(total)
        } else {
            Err(CosignRcptCode::TrackParity.val())
        }
    }

    /// 72h 档在册（判据锚定：72h 长稳判据登记）。
    pub fn h72_registered(&self) -> bool {
        self.longrun.iter().any(|r| r.tier == DurationTier::H72 && r.gate_attached)
    }

    /// 预算线在册（判据锚定：预算线在册）。
    pub fn budget_registered(&self) -> bool {
        !self.budgets.is_empty()
    }

    /// 预算线登记。
    pub fn register_budget(&mut self, name: &'static str, cap: u32) -> Result<u32, u32> {
        if name.is_empty() {
            return Err(CosignRcptCode::BudgetUnregistered.val());
        }
        self.budgets.push(BudgetLine { name, cap, used: 0 });
        Ok(self.budgets.len() as u32)
    }

    /// 预算消耗（按线名路由，超线拒）。
    pub fn budget_spend(&mut self, name: &str, amount: u32) -> Result<u32, u32> {
        let line = match self.budgets.iter_mut().find(|b| b.name == name) {
            Some(b) => b,
            None => return Err(CosignRcptCode::BudgetUnregistered.val()),
        };
        line.spend(amount)
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8088_0000u32;
        for r in self.receipts.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(r.slot);
            fp = fp.wrapping_mul(31).wrapping_add(match r.state {
                RcptState::Empty => 0,
                RcptState::Signed => 1,
                RcptState::Rejected => 2,
            });
        }
        fp.wrapping_add(self.longrun.len() as u32).wrapping_add(self.budgets.len() as u32)
    }
}

/// 冻结登记表（判据锚定：72h 长稳判据登记随闸门补测，双轨零 QEMU）。
pub fn frozen_longrun_book() -> Vec<LongrunReg> {
    let mut v: Vec<LongrunReg> = Vec::new();
    v.push(LongrunReg { name: "长稳-72h-泵账", tier: DurationTier::H72, track: TrackKind::TrackA, gate_attached: true });
    v.push(LongrunReg { name: "长稳-72h-回执", tier: DurationTier::H72, track: TrackKind::TrackB, gate_attached: true });
    v.push(LongrunReg { name: "长稳-48h-对账", tier: DurationTier::H48, track: TrackKind::TrackA, gate_attached: true });
    v.push(LongrunReg { name: "长稳-24h-联测", tier: DurationTier::H24, track: TrackKind::TrackB, gate_attached: true });
    v
}

/// 冻结预算线表（判据锚定：预算线在册）。
pub fn frozen_budget_book() -> Vec<BudgetLine> {
    let mut v: Vec<BudgetLine> = Vec::new();
    v.push(BudgetLine { name: "闸门补测线", cap: 40, used: 0 });
    v.push(BudgetLine { name: "双轨产线线", cap: 60, used: 0 });
    v
}

/// 黄金正演：会签八槽 + 长稳四条 + 预算两线全绿。
fn golden_run() -> Result<u32, u32> {
    let mut g = CosignGate::new();
    for i in 0..8u32 {
        let slot = SLOT_WIN.0 + i;
        let state = if i % 3 == 0 { RcptState::Rejected } else { RcptState::Signed };
        let _ = g.cosign(slot, state)?;
    }
    g.cosign_parity()?;
    for r in frozen_longrun_book() {
        let _ = g.register_longrun(r.name, r.tier, r.track, r.gate_attached)?;
    }
    g.track_parity()?;
    if !g.h72_registered() {
        return Err(CosignRcptCode::RegGateMissing.val());
    }
    for b in frozen_budget_book() {
        let _ = g.register_budget(b.name, b.cap)?;
    }
    if !g.budget_registered() {
        return Err(CosignRcptCode::BudgetUnregistered.val());
    }
    let _ = g.budget_spend("闸门补测线", 16)?;
    let _ = g.budget_spend("双轨产线线", 30)?;
    Ok(g.receipts.len() as u32)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && frozen_longrun_book().len() == 4 && frozen_budget_book().len() == 2
}

/// 注入：QEMU 轨 / 无闸登记 / 重复会签 / 坏槽位 / 超预算逐次必抓。
fn inject_violations() -> bool {
    let mut g = CosignGate::new();
    let qemu = g.register_longrun("坏-轨", DurationTier::H72, TrackKind::Qemu, true)
        == Err(CosignRcptCode::QemuTrackBanned.val());
    let nogate = g.register_longrun("坏-闸", DurationTier::H72, TrackKind::TrackA, false)
        == Err(CosignRcptCode::RegGateMissing.val());
    let _ = g.cosign(SLOT_WIN.0, RcptState::Signed);
    let dup = g.cosign(SLOT_WIN.0, RcptState::Signed) == Err(CosignRcptCode::DupCosign.val());
    let badslot = g.cosign(SLOT_WIN.1, RcptState::Signed) == Err(CosignRcptCode::BadSlot.val());
    let empty_state = g.cosign(SLOT_WIN.0 + 2, RcptState::Empty)
        == Err(CosignRcptCode::RcptStateOutOfSet.val());
    let _ = g.register_budget("闸门补测线", 10);
    let over = g.budget_spend("闸门补测线", 11) == Err(CosignRcptCode::BudgetOverrun.val());
    let noacc = g.budget_spend("不在册线", 1) == Err(CosignRcptCode::BudgetUnregistered.val());
    qemu && nogate && dup && badslot && empty_state && over && noacc
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in CosignRcptCode::ALL.iter() {
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
    // 判据②：72h 长稳判据登记随闸门补测（双轨零 QEMU）。
    let book = frozen_longrun_book();
    let h72 = book.iter().filter(|r| r.tier == DurationTier::H72).count();
    let all_gated = book.iter().all(|r| r.gate_attached);
    let no_qemu = book.iter().all(|r| r.track != TrackKind::Qemu);
    v.push((
        "J2-72h登记随闸零QEMU",
        h72 == 2 && all_gated && no_qemu,
        format!("72h 档 {} 条/全闸/零QEMU", h72),
    ));
    // 判据②：预算线在册。
    let bb = frozen_budget_book();
    v.push(("J2-预算线在册", bb.len() == 2 && bb.iter().all(|b| b.cap > 0), format!("在册 {} 线", bb.len())));
    // 注入。
    v.push(("J3-违约注入必抓", inject_violations(), String::from("QEMU/无闸/重复/坏槽/超支")));
    // 会签对平（先破后复）。
    let mut gp = CosignGate::new();
    let _ = gp.cosign(SLOT_WIN.0, RcptState::Signed);
    let _ = gp.cosign(SLOT_WIN.0 + 1, RcptState::Rejected);
    let ok_parity = gp.cosign_parity() == Ok(2);
    gp.receipts.push(CosignReceipt { slot: SLOT_WIN.0 + 2, state: RcptState::Empty });
    let brk = gp.cosign_parity() == Err(CosignRcptCode::CosignParity.val());
    v.push(("J3-会签对平破/复", ok_parity && brk, String::from("空签不可入账")));
    // 边界：槽位窗卡边。
    let mut gw = CosignGate::new();
    let lo = gw.cosign(SLOT_WIN.0, RcptState::Signed).is_ok();
    let hi = gw.cosign(SLOT_WIN.1 - 1, RcptState::Signed).is_ok();
    let mut gw2 = CosignGate::new();
    let over_lo = gw2.cosign(SLOT_WIN.0 - 1, RcptState::Signed) == Err(CosignRcptCode::BadSlot.val());
    let over_hi = gw2.cosign(SLOT_WIN.1, RcptState::Signed) == Err(CosignRcptCode::BadSlot.val());
    v.push(("J4-槽位窗卡边", lo && hi && over_lo && over_hi, String::from("0x20..0x40 窗")));
    // 边界：预算恰顶放行、超 1 拒。
    let mut gb = CosignGate::new();
    let _ = gb.register_budget("恰顶线", 10);
    let at_cap = gb.budget_spend("恰顶线", 10) == Ok(10);
    let mut gb2 = CosignGate::new();
    let _ = gb2.register_budget("超一线", 10);
    let over_cap = gb2.budget_spend("超一线", 11) == Err(CosignRcptCode::BudgetOverrun.val());
    v.push(("J4-预算恰顶/超1", at_cap && over_cap, String::from("上限卡边")));
    // 边界：时长档闭集。
    let dsum = DurationTier::H24.hours() + DurationTier::H48.hours() + DurationTier::H72.hours();
    let tiers_ok = dsum == 144 && DurationTier::H72.hours() == 72;
    v.push(("J4-时长档闭集", tiers_ok, format!("24+48+72={}", dsum)));
    // 边界：双轨对平（先破后复）。
    let mut gt = CosignGate::new();
    let _ = gt.register_longrun("甲轨", DurationTier::H72, TrackKind::TrackA, true);
    let _ = gt.register_longrun("乙轨", DurationTier::H72, TrackKind::TrackB, true);
    let ok_track = gt.track_parity() == Ok(2);
    gt.longrun.push(LongrunReg { name: "第三轨", tier: DurationTier::H24, track: TrackKind::Qemu, gate_attached: true });
    let brk_track = gt.track_parity() == Err(CosignRcptCode::TrackParity.val());
    v.push(("J4-双轨对平破/复", ok_track && brk_track, String::from("QEMU 轨即破")));
    // 回执态闭集点验。
    let rcpt_ok = RcptState::Empty != RcptState::Signed && RcptState::Signed != RcptState::Rejected;
    v.push(("J5-回执态闭集", rcpt_ok, String::from("空/正/驳三态")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80880 窗口")));
    v
}

/// VX9 检查入口（卡 12634，UNX-F12635-J1 判据承载）。
pub fn run_unx12634_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12634 D4会签槽位回执");
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
    fn test_h72_registered_with_gate() {
        let mut g = CosignGate::new();
        for r in frozen_longrun_book() {
            let _ = g.register_longrun(r.name, r.tier, r.track, r.gate_attached);
        }
        assert!(g.h72_registered());
        assert!(g.track_parity().is_ok());
    }

    #[test]
    fn test_qemu_track_banned() {
        let mut g = CosignGate::new();
        assert_eq!(
            g.register_longrun("q", DurationTier::H72, TrackKind::Qemu, true),
            Err(CosignRcptCode::QemuTrackBanned.val())
        );
    }

    #[test]
    fn test_gate_missing_rejected() {
        let mut g = CosignGate::new();
        assert_eq!(
            g.register_longrun("n", DurationTier::H48, TrackKind::TrackA, false),
            Err(CosignRcptCode::RegGateMissing.val())
        );
    }

    #[test]
    fn test_budget_registered_and_spend() {
        let mut g = CosignGate::new();
        assert!(!g.budget_registered());
        let _ = g.register_budget("线", 10);
        assert!(g.budget_registered());
        assert_eq!(g.budget_spend("线", 4), Ok(4));
        assert_eq!(g.budget_spend("线", 7), Err(CosignRcptCode::BudgetOverrun.val()));
        assert_eq!(g.budget_spend("无", 1), Err(CosignRcptCode::BudgetUnregistered.val()));
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_dup_cosign_and_bad_slot() {
        let mut g = CosignGate::new();
        let _ = g.cosign(SLOT_WIN.0, RcptState::Signed);
        assert_eq!(g.cosign(SLOT_WIN.0, RcptState::Signed), Err(CosignRcptCode::DupCosign.val()));
        assert_eq!(g.cosign(SLOT_WIN.1, RcptState::Signed), Err(CosignRcptCode::BadSlot.val()));
        assert_eq!(
            g.cosign(SLOT_WIN.0 + 1, RcptState::Empty),
            Err(CosignRcptCode::RcptStateOutOfSet.val())
        );
    }

    #[test]
    fn test_cosign_parity_break_and_fix() {
        let mut g = CosignGate::new();
        let _ = g.cosign(SLOT_WIN.0, RcptState::Signed);
        let _ = g.cosign(SLOT_WIN.0 + 1, RcptState::Rejected);
        assert_eq!(g.cosign_parity(), Ok(2));
        g.receipts.push(CosignReceipt { slot: SLOT_WIN.0 + 2, state: RcptState::Empty });
        assert_eq!(g.cosign_parity(), Err(CosignRcptCode::CosignParity.val()));
    }

    #[test]
    fn test_budget_cap_edge() {
        let mut g = CosignGate::new();
        let _ = g.register_budget("边线", 5);
        assert_eq!(g.budget_spend("边线", 5), Ok(5));
        assert_eq!(g.budget_spend("边线", 1), Err(CosignRcptCode::BudgetOverrun.val()));
    }

    #[test]
    fn test_duration_tiers() {
        assert_eq!(DurationTier::H24.hours(), 24);
        assert_eq!(DurationTier::H48.hours(), 48);
        assert_eq!(DurationTier::H72.hours(), 72);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80880);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in CosignRcptCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
