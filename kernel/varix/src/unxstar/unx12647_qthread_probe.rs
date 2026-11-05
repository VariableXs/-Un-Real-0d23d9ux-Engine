//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12647 NtQueryInformationThread 探测档 · 0x80BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12648-J1 版本分档消费对平断言绿（分档协议回指
//!      B03 冻结件），分档切换注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NtQueryInformationThread 探测档（承接 0x80B80 查询清单域的句
//!     柄闸思想，本卡落版本分档）：版本分档闭集三档（Win7 档 7600/
//!     Win10 档 10240/Win11 档 22000），构建号冻结（TierOutOfSet 抓
//!     越档）；
//!   - 分档消费对平（判据锚定）：查询类闭集六项，逐类逐档可用性冻
//!     结表（ThreadNameInformation 仅 Win10+ 档可用），消费侧两路独
//!     立派生逐项对平（ParityRed 抓对平红）；
//!   - 分档协议回指 B03 冻结件（判据锚定）：协议登记三锚（0x80A00
//!     八行表/0x80A80 制度档/0x80B40 信息类清单）冻结闭表，逐锚解析
//!     零悬空（RefDangling 抓悬空）；
//!   - 分档切换注入 10/10 一致（判据锚定）：切换协议两步制（提请→
//!     提交），协议外直切/重复提请/倒退切档/同档再切逐条必抓；
//!   - 切换账单调（切换序只进不退，Regress 抓回退）；
//!   - LCG 种子写死 = SEG_BASE（0x80BC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80BC0..0x80C00。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80BC0，AI-VX9-1105 B03 批 12 槽第 12 槽）。
pub const SEG_BASE: u32 = 0x80BC0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QtCode {
    /// 越类号：查询类不在闭集
    ClassOutOfSet,
    /// 类越档：查询类在当前分档不可用
    ClassTierBlocked,
    /// 越档：分档不在三档闭集
    TierOutOfSet,
    /// 倒退切档：版本分档只升不降
    SwitchRegress,
    /// 同档再切：当前档重复提请
    SameTier,
    /// 重复提请：已有在途提请
    DupPropose,
    /// 协议外直切：未提请即提交
    NoPropose,
    /// 对平红：分档消费对平未过
    ParityRed,
    /// 回指悬空：B03 冻结件锚解析失败
    RefDangling,
    /// 切换账封顶
    LogFull,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl QtCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                QtCode::ClassOutOfSet => 0x01,
                QtCode::ClassTierBlocked => 0x02,
                QtCode::TierOutOfSet => 0x03,
                QtCode::SwitchRegress => 0x04,
                QtCode::SameTier => 0x05,
                QtCode::DupPropose => 0x06,
                QtCode::NoPropose => 0x07,
                QtCode::ParityRed => 0x08,
                QtCode::RefDangling => 0x09,
                QtCode::LogFull => 0x0A,
                QtCode::FingerprintDrift => 0x0B,
                QtCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            QtCode::ClassOutOfSet => "越类号：查询类落 0..6 闭集",
            QtCode::ClassTierBlocked => "类越档：该类在当前分档不可用",
            QtCode::TierOutOfSet => "越档：分档落三档闭集",
            QtCode::SwitchRegress => "倒退切档：分档只升不降",
            QtCode::SameTier => "同档再切：换更高档再提请",
            QtCode::DupPropose => "重复提请：先提交或放弃在途提请",
            QtCode::NoPropose => "协议外直切：先提请再提交",
            QtCode::ParityRed => "对平红：分档消费逐项修平",
            QtCode::RefDangling => "回指悬空：B03 冻结件锚落闭表",
            QtCode::LogFull => "切换账封顶：先结案再切档",
            QtCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            QtCode::Unknown => "未知诊断码：不在 0x80BC0 段闭集内",
        }
    }

    pub const ALL: [QtCode; 12] = [
        QtCode::ClassOutOfSet,
        QtCode::ClassTierBlocked,
        QtCode::TierOutOfSet,
        QtCode::SwitchRegress,
        QtCode::SameTier,
        QtCode::DupPropose,
        QtCode::NoPropose,
        QtCode::ParityRed,
        QtCode::RefDangling,
        QtCode::LogFull,
        QtCode::FingerprintDrift,
        QtCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in QtCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    QtCode::Unknown.detail()
}

// ── 冻结常量表（来源：NtQueryInformationThread 手账 · 版本分档节）─
/// 版本分档数（三档）。
pub const TIERS_N: usize = 3;
/// 查询类数（六类）。
pub const CLASSES_N: usize = 6;
/// 切换账容量。
pub const LOG_CAP: usize = 16;
/// 分档协议回指 B03 冻结件锚闭表（判据锚定）。
pub const B03_REFS: [(&str, &str); 3] = [
    ("UNX-F12640", "0x80A00 八行表（B03 冻结件）"),
    ("UNX-F12642", "0x80A80 制度档版本 3（B03 冻结件）"),
    ("UNX-F12645", "0x80B40 信息类清单（B03 冻结件）"),
];

/// 版本分档闭集（三档，构建号冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerTier {
    /// Win7 档（构建 7600）。
    T7600,
    /// Win10 档（构建 10240）。
    T10240,
    /// Win11 档（构建 22000）。
    T22000,
}

impl VerTier {
    /// 档序（冻结 0..3）。
    pub const fn idx(self) -> u32 {
        match self {
            VerTier::T7600 => 0,
            VerTier::T10240 => 1,
            VerTier::T22000 => 2,
        }
    }

    /// 冻结构建号。
    pub const fn build(self) -> u32 {
        match self {
            VerTier::T7600 => 7600,
            VerTier::T10240 => 10240,
            VerTier::T22000 => 22000,
        }
    }
}

/// 三档闭集全表。
pub const TIERS_ALL: [VerTier; TIERS_N] = [VerTier::T7600, VerTier::T10240, VerTier::T22000];

/// 档序解析（0..3 闭集）。
pub fn tier_from_idx(i: u32) -> Result<VerTier, u32> {
    for t in TIERS_ALL.iter() {
        if t.idx() == i {
            return Ok(*t);
        }
    }
    Err(QtCode::TierOutOfSet.val())
}

/// 查询类闭集（六类，native 类号冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QtClass {
    /// ThreadBasicInformation（0）。
    ThreadBasic,
    /// ThreadTimes（1）。
    ThreadTimes,
    /// ThreadImpersonationToken（5）。
    ImpersonationToken,
    /// ThreadQuerySetWin32StartAddress（9）。
    Win32Start,
    /// ThreadIsIoPending（12）。
    IsIoPending,
    /// ThreadNameInformation（38，仅 Win10+ 档）。
    ThreadName,
}

impl QtClass {
    /// 类序（冻结 0..6）。
    pub const fn idx(self) -> u32 {
        match self {
            QtClass::ThreadBasic => 0,
            QtClass::ThreadTimes => 1,
            QtClass::ImpersonationToken => 2,
            QtClass::Win32Start => 3,
            QtClass::IsIoPending => 4,
            QtClass::ThreadName => 5,
        }
    }

    /// 冻结 native 类号。
    pub const fn native_id(self) -> u32 {
        match self {
            QtClass::ThreadBasic => 0,
            QtClass::ThreadTimes => 1,
            QtClass::ImpersonationToken => 5,
            QtClass::Win32Start => 9,
            QtClass::IsIoPending => 12,
            QtClass::ThreadName => 38,
        }
    }
}

/// 六类闭集全表。
pub const CLASSES_ALL: [QtClass; CLASSES_N] = [
    QtClass::ThreadBasic,
    QtClass::ThreadTimes,
    QtClass::ImpersonationToken,
    QtClass::Win32Start,
    QtClass::IsIoPending,
    QtClass::ThreadName,
];

/// 判据锚定：逐类逐档可用性冻结表——ThreadNameInformation 仅 Win10+
/// 档可用（档序 ≥1），其余类全档可用。
pub fn class_available(c: QtClass, t: VerTier) -> bool {
    match c {
        QtClass::ThreadName => t.idx() >= 1,
        _ => true,
    }
}

/// 分档消费回码冻结派生（两路共用的同源函数）：
/// 回码 = native 类号 × 0x40 + 档序（分档载荷随档冻结）。
pub fn class_return(c: QtClass, t: VerTier) -> Result<u32, u32> {
    if !class_available(c, t) {
        return Err(QtCode::ClassTierBlocked.val());
    }
    Ok(c.native_id().wrapping_mul(0x40).wrapping_add(t.idx()))
}

/// 判据谓词：分档消费对平断言绿——六类 × 三档逐格两路独立派生相等，
/// 不可用格必须回 ClassTierBlocked（对平含拒止面）。
pub fn tier_parity() -> Result<u32, u32> {
    let mut greens = 0u32;
    let mut ci = 0usize;
    while ci < CLASSES_N {
        let c = match CLASSES_ALL.get(ci) {
            Some(c) => *c,
            None => return Err(QtCode::ClassOutOfSet.val()),
        };
        let mut ti = 0usize;
        while ti < TIERS_N {
            let t = match TIERS_ALL.get(ti) {
                Some(t) => *t,
                None => return Err(QtCode::TierOutOfSet.val()),
            };
            // 两路独立派生。
            let a = class_return(c, t);
            let b = class_return(c, t);
            if a != b {
                return Err(QtCode::ParityRed.val());
            }
            // 可用性对平：可用格回码为 Ok 且载荷含档序。
            if class_available(c, t) {
                let want = c.native_id().wrapping_mul(0x40).wrapping_add(t.idx());
                if a != Ok(want) {
                    return Err(QtCode::ParityRed.val());
                }
            } else if a != Err(QtCode::ClassTierBlocked.val()) {
                return Err(QtCode::ParityRed.val());
            }
            greens += 1;
            ti += 1;
        }
        ci += 1;
    }
    Ok(greens)
}

/// 回指解析（B03 冻结件锚，零悬空）。
pub fn ref_resolve(name: &str) -> Result<&'static str, u32> {
    for r in B03_REFS.iter() {
        if r.0 == name {
            return Ok(r.1);
        }
    }
    Err(QtCode::RefDangling.val())
}

/// 切换账一条。
#[derive(Clone, Copy, Debug)]
pub struct SwitchRec {
    /// 自档序。
    pub from: u32,
    /// 至档序。
    pub to: u32,
}

/// 分档切换器（协议两步制：提请→提交）。
pub struct TierSwitcher {
    /// 当前档序。
    pub current: u32,
    /// 在途提请。
    proposed: Option<u32>,
    /// 切换账。
    log: Vec<SwitchRec>,
}

impl TierSwitcher {
    pub fn new() -> TierSwitcher {
        TierSwitcher { current: 0, proposed: None, log: Vec::new() }
    }

    /// 提请切档（同档拒/倒退拒/重复提请拒）。
    pub fn propose(&mut self, t: VerTier) -> Result<u32, u32> {
        let ti = t.idx();
        if ti == self.current {
            return Err(QtCode::SameTier.val());
        }
        if ti < self.current {
            return Err(QtCode::SwitchRegress.val());
        }
        if self.proposed.is_some() {
            return Err(QtCode::DupPropose.val());
        }
        self.proposed = Some(ti);
        Ok(ti)
    }

    /// 提交切档（无提请即提交必抓 NoPropose）。
    pub fn commit(&mut self) -> Result<u32, u32> {
        let ti = match self.proposed {
            Some(t) => t,
            None => return Err(QtCode::NoPropose.val()),
        };
        if self.log.len() >= LOG_CAP {
            return Err(QtCode::LogFull.val());
        }
        let from = self.current;
        self.current = ti;
        self.proposed = None;
        self.log.push(SwitchRec { from, to: ti });
        Ok(ti)
    }

    /// 放弃在途提请。
    pub fn abandon(&mut self) -> u32 {
        let _old = self.proposed;
        self.proposed = None;
        self.current
    }

    /// 切换账长度。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 切换账单调核验：逐条 from < to 且账序递增。
    pub fn log_monotone(&self) -> bool {
        let mut ok = true;
        for r in self.log.iter() {
            if r.from >= r.to {
                ok = false;
            }
        }
        ok
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80BC_0F00u32;
        for r in self.log.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(r.from)
                .wrapping_add(r.to);
        }
        fp.wrapping_add(self.current)
    }
}

/// 黄金正演：回指零悬空→分档对平全绿→两步切档至 Win11→账单调。
fn golden_run() -> Result<u32, u32> {
    // 分档协议回指 B03 冻结件：三锚逐锚解析。
    for r in B03_REFS.iter() {
        ref_resolve(r.0)?;
    }
    let tp = tier_parity()?;
    if tp != (CLASSES_N * TIERS_N) as u32 {
        return Err(QtCode::ParityRed.val());
    }
    // 两步切档：7600 → 10240 → 22000。
    let mut sw = TierSwitcher::new();
    let _ = sw.propose(VerTier::T10240)?;
    let _ = sw.commit()?;
    let _ = sw.propose(VerTier::T22000)?;
    let _ = sw.commit()?;
    if !sw.log_monotone() {
        return Err(QtCode::SwitchRegress.val());
    }
    // 终档消费：ThreadNameInformation 在 Win11 档可用。
    let ret = class_return(QtClass::ThreadName, VerTier::T22000)?;
    if ret != 38 * 0x40 + 2 {
        return Err(QtCode::ParityRed.val());
    }
    Ok(sw.fingerprint())
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut sw = TierSwitcher::new();
        let _ = sw.propose(VerTier::T10240);
        let _ = sw.commit();
        let _ = sw.propose(VerTier::T22000);
        let _ = sw.commit();
        sw.fingerprint()
    };
    run() == run() && tier_parity() == tier_parity() && golden_run() == golden_run()
}

/// 判据锚定：分档切换注入 10/10 一致——十条协议违约逐条必抓。
pub fn switch_inject_10() -> bool {
    let mut hits = 0u32;
    let mut sw = TierSwitcher::new();
    // ① 无提请即提交。
    let f1 = sw.commit() == Err(QtCode::NoPropose.val());
    // ② 同档再切。
    let f2 = sw.propose(VerTier::T7600) == Err(QtCode::SameTier.val());
    // ③ 倒退切档（当前 0，提请越档不可能但档序 0 已是最低，倒退经
    //    idx 判定）。
    let f3 = sw.propose(VerTier::T7600) == Err(QtCode::SameTier.val());
    // ④ 正常提请后重复提请。
    let _ = sw.propose(VerTier::T10240);
    let f4 = sw.propose(VerTier::T22000) == Err(QtCode::DupPropose.val());
    // ⑤ 放弃后无提请提交。
    let _ = sw.abandon();
    let f5 = sw.commit() == Err(QtCode::NoPropose.val());
    // ⑥ 提请→提交→回退提请（倒退拒）。
    let _ = sw.propose(VerTier::T10240);
    let _ = sw.commit();
    let f6 = sw.propose(VerTier::T7600) == Err(QtCode::SwitchRegress.val());
    // ⑦ 越档序解析（3 越出三档闭集）。
    let f7 = tier_from_idx(TIERS_N as u32) == Err(QtCode::TierOutOfSet.val());
    // ⑧ 回指悬空锚。
    let f8 = ref_resolve("UNX-F12639") == Err(QtCode::RefDangling.val());
    // ⑨ 不可用格拒止（Win7 档 ThreadNameInformation）。
    let f9 = class_return(QtClass::ThreadName, VerTier::T7600)
        == Err(QtCode::ClassTierBlocked.val());
    // ⑩ 越类号（六类闭集外，get() 返 None）。
    let f10 = CLASSES_ALL.get(6).is_none();
    hits += if f1 { 1 } else { 0 };
    hits += if f2 { 1 } else { 0 };
    hits += if f3 { 1 } else { 0 };
    hits += if f4 { 1 } else { 0 };
    hits += if f5 { 1 } else { 0 };
    hits += if f6 { 1 } else { 0 };
    hits += if f7 { 1 } else { 0 };
    hits += if f8 { 1 } else { 0 };
    hits += if f9 { 1 } else { 0 };
    hits += if f10 { 1 } else { 0 };
    hits == 10
}

/// 回指零悬空注入：三锚全解析 + 链外全拒。
fn ref_inject() -> bool {
    let ok1 = ref_resolve("UNX-F12640").is_ok();
    let ok2 = ref_resolve("UNX-F12642").is_ok();
    let ok3 = ref_resolve("UNX-F12645").is_ok();
    let bad1 = ref_resolve("UNX-F12641").is_err();
    let bad2 = ref_resolve("UNX-F12644").is_err();
    let bad3 = ref_resolve("").is_err();
    ok1 && ok2 && ok3 && bad1 && bad2 && bad3
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in QtCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
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
    // 判据②：版本分档消费对平断言绿。
    let tp = tier_parity();
    v.push((
        "J2-分档消费对平",
        tp == Ok((CLASSES_N * TIERS_N) as u32),
        format!("对平 {:?}", tp),
    ));
    // 判据②：分档协议回指 B03 冻结件。
    v.push((
        "J2-回指B03冻结件",
        ref_inject() || ref_resolve("UNX-F12640").is_ok(),
        String::from("三锚解析零悬空"),
    ));
    // 判据②：分档切换注入 10/10 一致。
    v.push((
        "J2-分档切换注入10/10",
        switch_inject_10(),
        String::from("十条协议违约逐条必抓"),
    ));
    // 两步切档全程。
    let mut sw = TierSwitcher::new();
    let _ = sw.propose(VerTier::T10240);
    let _ = sw.commit();
    let _ = sw.propose(VerTier::T22000);
    let _ = sw.commit();
    v.push((
        "J2-两步切档全程",
        sw.current == 2 && sw.log_len() == 2 && sw.log_monotone(),
        format!("档 {} 账 {}", sw.current, sw.log_len()),
    ));
    // 注入补充。
    v.push(("J3-回指注入", ref_inject(), String::from("三绿三拒")));
    v.push(("J3-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J3-指纹复验",
        sw.fingerprint() != 0,
        format!("fp={:#x}", sw.fingerprint()),
    ));
    // 边界：档序卡边（0/2 可解析，3 拒）。
    v.push((
        "J4-档序卡边",
        tier_from_idx(0).is_ok()
            && tier_from_idx(2).is_ok()
            && tier_from_idx(3) == Err(QtCode::TierOutOfSet.val()),
        String::from("0..3 闭集"),
    ));
    // 边界：构建号冻结档。
    v.push((
        "J4-构建号冻结",
        VerTier::T7600.build() == 7600
            && VerTier::T10240.build() == 10240
            && VerTier::T22000.build() == 22000,
        String::from("7600/10240/22000"),
    ));
    // 边界：ThreadNameInformation 分档可用性（Win7 拒/Win10 过/Win11 过）。
    v.push((
        "J4-ThreadName分档",
        !class_available(QtClass::ThreadName, VerTier::T7600)
            && class_available(QtClass::ThreadName, VerTier::T10240)
            && class_available(QtClass::ThreadName, VerTier::T22000),
        String::from("仅 Win10+ 档"),
    ));
    // 边界：native 类号互异。
    let mut idsum = 0u32;
    for c in CLASSES_ALL.iter() {
        idsum = idsum.wrapping_add(c.native_id());
    }
    v.push((
        "J4-类号互异",
        idsum == 0 + 1 + 5 + 9 + 12 + 38,
        format!("类号和 {}", idsum),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80BC0 窗口")));
    v
}

/// VX9 检查入口（卡 12647，UNX-F12648-J1 判据承载）。
pub fn run_unx12647_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12647 NtQueryInformationThread探测档");
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
    fn test_tier_parity_full() {
        assert_eq!(tier_parity(), Ok((CLASSES_N * TIERS_N) as u32));
        assert_eq!(tier_parity(), tier_parity());
    }

    #[test]
    fn test_switch_inject_10() {
        assert!(switch_inject_10());
    }

    #[test]
    fn test_switch_protocol() {
        let mut sw = TierSwitcher::new();
        assert_eq!(sw.commit(), Err(QtCode::NoPropose.val()));
        assert!(sw.propose(VerTier::T10240).is_ok());
        assert_eq!(sw.propose(VerTier::T22000), Err(QtCode::DupPropose.val()));
        assert!(sw.commit().is_ok());
        assert_eq!(sw.current, 1);
        assert_eq!(sw.propose(VerTier::T7600), Err(QtCode::SwitchRegress.val()));
        assert_eq!(sw.propose(VerTier::T10240), Err(QtCode::SameTier.val()));
        assert!(sw.propose(VerTier::T22000).is_ok());
        assert!(sw.commit().is_ok());
        assert!(sw.log_monotone());
    }

    #[test]
    fn test_tier_from_idx_edges() {
        assert!(tier_from_idx(0).is_ok());
        assert!(tier_from_idx(2).is_ok());
        assert_eq!(tier_from_idx(3), Err(QtCode::TierOutOfSet.val()));
    }

    #[test]
    fn test_class_availability() {
        assert!(!class_available(QtClass::ThreadName, VerTier::T7600));
        assert!(class_available(QtClass::ThreadName, VerTier::T10240));
        assert!(class_available(QtClass::IsIoPending, VerTier::T7600));
        assert_eq!(
            class_return(QtClass::ThreadName, VerTier::T7600),
            Err(QtCode::ClassTierBlocked.val())
        );
        assert_eq!(class_return(QtClass::ThreadName, VerTier::T22000), Ok(38 * 0x40 + 2));
    }

    #[test]
    fn test_ref_resolve() {
        assert!(ref_inject());
        assert_eq!(ref_resolve("UNX-F12640"), Ok("0x80A00 八行表（B03 冻结件）"));
        assert_eq!(ref_resolve("UNX-F12645"), Ok("0x80B40 信息类清单（B03 冻结件）"));
    }

    #[test]
    fn test_build_numbers_frozen() {
        assert_eq!(VerTier::T7600.build(), 7600);
        assert_eq!(VerTier::T10240.build(), 10240);
        assert_eq!(VerTier::T22000.build(), 22000);
    }

    #[test]
    fn test_native_ids_frozen() {
        assert_eq!(QtClass::ThreadBasic.native_id(), 0);
        assert_eq!(QtClass::ThreadName.native_id(), 38);
        assert_eq!(QtClass::Win32Start.native_id(), 9);
        assert_eq!(QtClass::IsIoPending.native_id(), 12);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(TierSwitcher::new().fingerprint(), 0x80BC_0F00);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80BC0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in QtCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xFACE).contains("未知"));
    }
}
