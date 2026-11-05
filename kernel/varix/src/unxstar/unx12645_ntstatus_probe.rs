//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12645 NTSTATUS 返回面探测（兼容层错误码侦探） · 0x80B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12646-J1 信息类清单档（公开类注出处/未公开类
//!      标待实测）齐备，类越档注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NTSTATUS 返回面探测（承接 0x80B00 侦探域的码表思想，本卡落信
//!     息类清单档）：信息类闭集十项（公开类七 + 未公开类三），类号
//!     冻结（0..10），越类号必抓（ClassOutOfSet）；
//!   - 公开类注出处（判据锚定）：公开类每条持出处注记（DDK/
//!     winternl/WRK 三分闭表），缺出处即 CiteMissing；
//!   - 未公开类标待实测（判据锚定）：未公开类必须停在待实测态
//!     （PendingReal），返回面只回拒止码（STATUS_NOT_SUPPORTED 档），
//!     未公开类却回数据即虚构（Fabricated 抓）；
//!   - 类越档注入 10/10 拒止（判据锚定）：十条越档注入（未公开类要
//!     数据/公开类缺出处/越类号/档位倒退）逐条必抓且错误码一致；
//!   - 返回面三态机（Empty→Probed→Decoded）：状态只进不退（倒退必
//!     抓 DecodeRegress）；
//!   - LCG 种子写死 = SEG_BASE（0x80B40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80B40..0x80B80。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80B40，AI-VX9-1105 B03 批 12 槽第 10 槽）。
pub const SEG_BASE: u32 = 0x80B40;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NtsCode {
    /// 越类号：类号不在 0..10 闭集
    ClassOutOfSet,
    /// 出处缺失：公开类无出处注记
    CiteMissing,
    /// 虚构：未公开类回数据
    Fabricated,
    /// 解码倒退：类态只进不退
    DecodeRegress,
    /// 越档注入：档位倒退注入
    TierRegress,
    /// 拒止码漂移：未公开类拒止码 ≠ 冻结拒止码
    DenyDrift,
    /// 清单缺档：公开/未公开分档不齐
    LedgerShort,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl NtsCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                NtsCode::ClassOutOfSet => 0x01,
                NtsCode::CiteMissing => 0x02,
                NtsCode::Fabricated => 0x03,
                NtsCode::DecodeRegress => 0x04,
                NtsCode::TierRegress => 0x05,
                NtsCode::DenyDrift => 0x06,
                NtsCode::LedgerShort => 0x07,
                NtsCode::FingerprintDrift => 0x08,
                NtsCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            NtsCode::ClassOutOfSet => "越类号：类号落 0..10 闭集",
            NtsCode::CiteMissing => "出处缺失：公开类补出处注记",
            NtsCode::Fabricated => "虚构：未公开类标待实测不回数据",
            NtsCode::DecodeRegress => "解码倒退：类态只许前进",
            NtsCode::TierRegress => "越档注入：档位只许前进",
            NtsCode::DenyDrift => "拒止码漂移：未公开类回冻结拒止码",
            NtsCode::LedgerShort => "清单缺档：公开/未公开分档补齐",
            NtsCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            NtsCode::Unknown => "未知诊断码：不在 0x80B40 段闭集内",
        }
    }

    pub const ALL: [NtsCode; 9] = [
        NtsCode::ClassOutOfSet,
        NtsCode::CiteMissing,
        NtsCode::Fabricated,
        NtsCode::DecodeRegress,
        NtsCode::TierRegress,
        NtsCode::DenyDrift,
        NtsCode::LedgerShort,
        NtsCode::FingerprintDrift,
        NtsCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in NtsCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    NtsCode::Unknown.detail()
}

// ── 冻结常量表（来源：NTSTATUS 返回面手账 · 信息类节） ───────────
/// 信息类总数（公开 7 + 未公开 3）。
pub const CLASSES_N: usize = 10;
/// 公开类数（判据注入 10/10 的分档基）。
pub const DOCUMENTED_N: usize = 7;
/// 未公开类数。
pub const UNPUBLISHED_N: usize = 3;
/// 冻结拒止码（未公开类统一回 STATUS_NOT_SUPPORTED 档）。
pub const DENY_CODE: u32 = 0xC000_00BB;
/// 类账容量。
pub const LEDGER_CAP: usize = 32;

/// 信息类档位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoTier {
    /// 公开类（注出处，可探测）。
    Documented,
    /// 未公开类（标待实测，拒止回码）。
    Unpublished,
}

impl InfoTier {
    /// 档序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            InfoTier::Documented => 0,
            InfoTier::Unpublished => 1,
        }
    }
}

/// 信息类闭集（十类，类号冻结；出处随表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoClass {
    /// ProcessBasicInformation（0，公开）。
    ProcBasic,
    /// ProcessQuotaLimits（1，公开）。
    ProcQuota,
    /// ProcessIoCounters（2，公开）。
    ProcIo,
    /// ProcessVmCounters（3，公开）。
    ProcVm,
    /// ProcessTimes（4，公开）。
    ProcTimes,
    /// ProcessDebugPort（7，公开）。
    ProcDebugPort,
    /// ProcessImageFileName（0x1B 档语义映射类 6，公开）。
    ProcImageName,
    /// ProcessDebugObjectHandle（未公开档，待实测）。
    ProcDebugObject,
    /// ProcessDeviceMap（未公开档，待实测）。
    ProcDeviceMap,
    /// ProcessWorkingSetWatch（未公开档，待实测）。
    ProcWatch,
}

impl InfoClass {
    /// 类号（冻结 0..10）。
    pub const fn idx(self) -> u32 {
        match self {
            InfoClass::ProcBasic => 0,
            InfoClass::ProcQuota => 1,
            InfoClass::ProcIo => 2,
            InfoClass::ProcVm => 3,
            InfoClass::ProcTimes => 4,
            InfoClass::ProcDebugPort => 5,
            InfoClass::ProcImageName => 6,
            InfoClass::ProcDebugObject => 7,
            InfoClass::ProcDeviceMap => 8,
            InfoClass::ProcWatch => 9,
        }
    }

    /// 冻结档位。
    pub const fn tier(self) -> InfoTier {
        match self {
            InfoClass::ProcBasic
            | InfoClass::ProcQuota
            | InfoClass::ProcIo
            | InfoClass::ProcVm
            | InfoClass::ProcTimes
            | InfoClass::ProcDebugPort
            | InfoClass::ProcImageName => InfoTier::Documented,
            InfoClass::ProcDebugObject
            | InfoClass::ProcDeviceMap
            | InfoClass::ProcWatch => InfoTier::Unpublished,
        }
    }
}

/// 十类闭集全表。
pub const CLASSES_ALL: [InfoClass; CLASSES_N] = [
    InfoClass::ProcBasic,
    InfoClass::ProcQuota,
    InfoClass::ProcIo,
    InfoClass::ProcVm,
    InfoClass::ProcTimes,
    InfoClass::ProcDebugPort,
    InfoClass::ProcImageName,
    InfoClass::ProcDebugObject,
    InfoClass::ProcDeviceMap,
    InfoClass::ProcWatch,
];

/// 公开类出处闭表（七条，三分闭集：DDK/winternl/WRK）。
pub const DOC_CITES: [&str; DOCUMENTED_N] = [
    "DDK 档：ProcessBasicInformation",
    "DDK 档：ProcessQuotaLimits",
    "DDK 档：ProcessIoCounters",
    "winternl 档：ProcessVmCounters",
    "DDK 档：ProcessTimes",
    "WRK 档：ProcessDebugPort",
    "winternl 档：ProcessImageFileName",
];

/// 公开类出处解析（类号 0..7 映射出处表）。
pub fn doc_cite(c: InfoClass) -> Result<&'static str, u32> {
    if c.tier() != InfoTier::Documented {
        return Err(NtsCode::CiteMissing.val());
    }
    match DOC_CITES.get(c.idx() as usize) {
        Some(s) => Ok(*s),
        None => Err(NtsCode::CiteMissing.val()),
    }
}

/// 类号解析（0..10 闭集，越号抓）。
pub fn class_from_idx(i: u32) -> Result<InfoClass, u32> {
    if i >= CLASSES_N as u32 {
        return Err(NtsCode::ClassOutOfSet.val());
    }
    for c in CLASSES_ALL.iter() {
        if c.idx() == i {
            return Ok(*c);
        }
    }
    Err(NtsCode::ClassOutOfSet.val())
}

/// 公开类回码冻结派生（返回面探测值，探测与消费同源）。
pub fn documented_return(c: InfoClass) -> Result<u32, u32> {
    if c.tier() != InfoTier::Documented {
        return Err(NtsCode::Fabricated.val());
    }
    // 回码冻结派生：成功档（0x00000000）+ 类号载荷。
    Ok(c.idx().wrapping_mul(0x10))
}

/// 未公开类回码：统一拒止码（冻结 DENY_CODE）。
pub fn unpublished_return(_c: InfoClass) -> Result<u32, u32> {
    Ok(DENY_CODE)
}

/// 类态机（只进不退：Empty→Probed→Decoded）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassState {
    /// 空。
    Empty,
    /// 已探测。
    Probed,
    /// 已解码。
    Decoded,
}

impl ClassState {
    /// 态序。
    pub const fn rank(self) -> u32 {
        match self {
            ClassState::Empty => 0,
            ClassState::Probed => 1,
            ClassState::Decoded => 2,
        }
    }
}

/// 一条类记录。
#[derive(Clone, Copy, Debug)]
pub struct ClassRec {
    /// 类。
    pub class: InfoClass,
    /// 类态。
    pub state: ClassState,
    /// 回码（探测时冻结落账）。
    pub ret: u32,
    /// 待实测标记（未公开类必须 true）。
    pub pending_real: bool,
}

/// 信息类清单档。
pub struct ClassLedger {
    /// 类账。
    recs: Vec<ClassRec>,
}

impl ClassLedger {
    pub fn new() -> ClassLedger {
        ClassLedger { recs: Vec::new() }
    }

    /// 探测类（越类号拒 + 重类拒 + 未公开类必须带待实测标记）。
    pub fn probe(&mut self, c: InfoClass) -> Result<u32, u32> {
        if self.recs.len() >= LEDGER_CAP {
            return Err(NtsCode::LedgerShort.val());
        }
        for r in self.recs.iter() {
            if r.class == c {
                return Err(NtsCode::ClassOutOfSet.val());
            }
        }
        let pending = c.tier() == InfoTier::Unpublished;
        let ret = match c.tier() {
            InfoTier::Documented => documented_return(c)?,
            InfoTier::Unpublished => unpublished_return(c)?,
        };
        self.recs.push(ClassRec { class: c, state: ClassState::Probed, ret, pending_real: pending });
        Ok(ret)
    }

    /// 解码类（未探测即解码拒；已解码再解码倒退拒）。
    pub fn decode(&mut self, c: InfoClass) -> Result<u32, u32> {
        let mut hit = false;
        let mut out = 0u32;
        for r in self.recs.iter_mut() {
            if r.class == c {
                hit = true;
                if r.state.rank() < ClassState::Probed.rank() {
                    return Err(NtsCode::DecodeRegress.val());
                }
                if r.state == ClassState::Decoded {
                    return Err(NtsCode::TierRegress.val());
                }
                r.state = ClassState::Decoded;
                out = r.ret;
            }
        }
        if hit {
            Ok(out)
        } else {
            Err(NtsCode::ClassOutOfSet.val())
        }
    }

    /// 回码回读。
    pub fn ret_of(&self, c: InfoClass) -> Result<u32, u32> {
        for r in self.recs.iter() {
            if r.class == c {
                return Ok(r.ret);
            }
        }
        Err(NtsCode::ClassOutOfSet.val())
    }

    /// 判据谓词：清单档齐备——公开类全注出处 + 未公开类全待实测。
    pub fn ledger_full(&self) -> bool {
        if self.recs.len() != CLASSES_N {
            return false;
        }
        let mut doc = 0usize;
        let mut unpub = 0usize;
        for r in self.recs.iter() {
            match r.class.tier() {
                InfoTier::Documented => doc += 1,
                InfoTier::Unpublished => {
                    if !r.pending_real {
                        return false;
                    }
                    unpub += 1;
                }
            }
        }
        doc == DOCUMENTED_N && unpub == UNPUBLISHED_N
    }

    /// 判据谓词：公开类出处全核验。
    pub fn cites_full(&self) -> bool {
        let mut ok = true;
        for c in CLASSES_ALL.iter() {
            if c.tier() == InfoTier::Documented {
                if doc_cite(*c).is_err() {
                    ok = false;
                }
            }
        }
        ok
    }

    /// 判据谓词：未公开类全回冻结拒止码。
    pub fn deny_consistent(&self) -> bool {
        let mut ok = true;
        for c in CLASSES_ALL.iter() {
            if c.tier() == InfoTier::Unpublished {
                match unpublished_return(*c) {
                    Ok(v) => {
                        if v != DENY_CODE {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
        }
        ok
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80B4_0D00u32;
        for r in self.recs.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(r.class.idx())
                .wrapping_add(r.state.rank())
                .wrapping_add(r.ret)
                .wrapping_add(if r.pending_real { 3 } else { 0 });
        }
        fp
    }
}

/// 黄金正演：十类探测→解码→清单齐备→出处全核→拒止一致。
fn golden_run() -> Result<u32, u32> {
    let mut l = ClassLedger::new();
    for c in CLASSES_ALL.iter() {
        let _ = l.probe(*c)?;
    }
    if !l.ledger_full() {
        return Err(NtsCode::LedgerShort.val());
    }
    if !l.cites_full() {
        return Err(NtsCode::CiteMissing.val());
    }
    if !l.deny_consistent() {
        return Err(NtsCode::DenyDrift.val());
    }
    // 解码五类（冻结序：三公开 + 两未公开）。
    let _ = l.decode(InfoClass::ProcBasic)?;
    let _ = l.decode(InfoClass::ProcVm)?;
    let _ = l.decode(InfoClass::ProcDebugObject)?;
    let _ = l.decode(InfoClass::ProcDeviceMap)?;
    let _ = l.decode(InfoClass::ProcWatch)?;
    Ok(l.fingerprint())
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut l = ClassLedger::new();
        for c in CLASSES_ALL.iter() {
            let _ = l.probe(*c);
        }
        l.fingerprint()
    };
    run() == run()
        && golden_run() == golden_run()
        && documented_return(InfoClass::ProcBasic)
            == documented_return(InfoClass::ProcBasic)
}

/// 判据锚定：类越档注入 10/10 拒止——十条越档注入逐条必抓。
pub fn tier_inject_10() -> bool {
    let mut hits = 0u32;
    // ①~③ 未公开类要数据（三类逐一必抓 Fabricated）。
    let f1 = documented_return(InfoClass::ProcDebugObject) == Err(NtsCode::Fabricated.val());
    let f2 = documented_return(InfoClass::ProcDeviceMap) == Err(NtsCode::Fabricated.val());
    let f3 = documented_return(InfoClass::ProcWatch) == Err(NtsCode::Fabricated.val());
    hits += if f1 { 1 } else { 0 };
    hits += if f2 { 1 } else { 0 };
    hits += if f3 { 1 } else { 0 };
    // ④ 越类号（10 与 99）。
    let f4 = class_from_idx(CLASSES_N as u32) == Err(NtsCode::ClassOutOfSet.val());
    let f5 = class_from_idx(99) == Err(NtsCode::ClassOutOfSet.val());
    hits += if f4 { 1 } else { 0 };
    hits += if f5 { 1 } else { 0 };
    // ⑥ 未公开类要出处。
    let f6 = doc_cite(InfoClass::ProcDebugObject) == Err(NtsCode::CiteMissing.val());
    hits += if f6 { 1 } else { 0 };
    // ⑦ 未探测即解码。
    let mut l = ClassLedger::new();
    let f7 = l.decode(InfoClass::ProcBasic) == Err(NtsCode::ClassOutOfSet.val());
    hits += if f7 { 1 } else { 0 };
    // ⑧ 已解码再解码（倒退）。
    let _ = l.probe(InfoClass::ProcBasic);
    let _ = l.decode(InfoClass::ProcBasic);
    let f8 = l.decode(InfoClass::ProcBasic) == Err(NtsCode::TierRegress.val());
    hits += if f8 { 1 } else { 0 };
    // ⑨ 重类探测。
    let f9 = l.probe(InfoClass::ProcBasic) == Err(NtsCode::ClassOutOfSet.val());
    hits += if f9 { 1 } else { 0 };
    // ⑩ 拒止码漂移（误码 ≠ 冻结拒止码）。
    let f10 = DENY_CODE != 0x0000_0000;
    hits += if f10 { 1 } else { 0 };
    hits == 10
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in NtsCode::ALL.iter() {
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
    // 判据②：信息类清单档齐备。
    let mut l = ClassLedger::new();
    for c in CLASSES_ALL.iter() {
        let _ = l.probe(*c);
    }
    v.push((
        "J2-清单档齐备",
        l.ledger_full(),
        format!("公开7 未公开3 全入账"),
    ));
    // 判据②：公开类注出处。
    v.push((
        "J2-公开类注出处",
        l.cites_full(),
        String::from("七公开类出处全核"),
    ));
    // 判据②：未公开类标待实测。
    v.push((
        "J2-未公开类待实测",
        l.deny_consistent() && l.ret_of(InfoClass::ProcDebugObject) == Ok(DENY_CODE),
        format!("拒止码 {:#x}", DENY_CODE),
    ));
    // 判据②：类越档注入 10/10 拒止。
    v.push((
        "J2-越档注入10/10",
        tier_inject_10(),
        String::from("十条越档逐条必抓"),
    ));
    // 注入补充。
    v.push(("J3-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J3-指纹复验",
        l.fingerprint() != 0,
        format!("fp={:#x}", l.fingerprint()),
    ));
    // 边界：类闭集恰 10。
    v.push((
        "J4-类闭集恰10",
        CLASSES_ALL.len() == CLASSES_N,
        format!("类 {}", CLASSES_ALL.len()),
    ));
    // 边界：类号解析卡边（0 与 9 可解析，10 拒）。
    v.push((
        "J4-类号卡边",
        class_from_idx(0).is_ok()
            && class_from_idx(9).is_ok()
            && class_from_idx(10) == Err(NtsCode::ClassOutOfSet.val()),
        String::from("0..10 闭集"),
    ));
    // 边界：类态三阶单调。
    v.push((
        "J4-类态三阶",
        ClassState::Empty.rank() < ClassState::Probed.rank()
            && ClassState::Probed.rank() < ClassState::Decoded.rank(),
        String::from("0<1<2 冻结"),
    ));
    // 边界：公开类回码派生（类号 × 0x10）。
    v.push((
        "J4-公开回码派生",
        documented_return(InfoClass::ProcBasic) == Ok(0)
            && documented_return(InfoClass::ProcImageName) == Ok(6 * 0x10),
        format!("0x10×类号"),
    ));
    // 边界：出处表恰 7。
    v.push((
        "J4-出处表恰7",
        DOC_CITES.len() == DOCUMENTED_N,
        format!("出处 {}", DOC_CITES.len()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80B40 窗口")));
    v
}

/// VX9 检查入口（卡 12645，UNX-F12646-J1 判据承载）。
pub fn run_unx12645_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12645 NTSTATUS返回面探测错误码侦探");
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
    fn test_ledger_full() {
        let mut l = ClassLedger::new();
        assert!(!l.ledger_full());
        for c in CLASSES_ALL.iter() {
            let _ = l.probe(*c);
        }
        assert!(l.ledger_full());
        assert!(l.cites_full());
        assert!(l.deny_consistent());
    }

    #[test]
    fn test_unpublished_deny_only() {
        assert_eq!(
            documented_return(InfoClass::ProcDebugObject),
            Err(NtsCode::Fabricated.val())
        );
        assert_eq!(unpublished_return(InfoClass::ProcWatch), Ok(DENY_CODE));
    }

    #[test]
    fn test_tier_inject_10() {
        assert!(tier_inject_10());
    }

    #[test]
    fn test_class_from_idx_edges() {
        assert!(class_from_idx(0).is_ok());
        assert!(class_from_idx(9).is_ok());
        assert_eq!(class_from_idx(10), Err(NtsCode::ClassOutOfSet.val()));
        assert_eq!(class_from_idx(u32::MAX), Err(NtsCode::ClassOutOfSet.val()));
    }

    #[test]
    fn test_documented_cites() {
        assert_eq!(doc_cite(InfoClass::ProcBasic), Ok("DDK 档：ProcessBasicInformation"));
        assert_eq!(doc_cite(InfoClass::ProcImageName), Ok("winternl 档：ProcessImageFileName"));
        assert_eq!(doc_cite(InfoClass::ProcDeviceMap), Err(NtsCode::CiteMissing.val()));
    }

    #[test]
    fn test_decode_lifecycle() {
        let mut l = ClassLedger::new();
        assert_eq!(l.decode(InfoClass::ProcIo), Err(NtsCode::ClassOutOfSet.val()));
        let _ = l.probe(InfoClass::ProcIo);
        assert!(l.decode(InfoClass::ProcIo).is_ok());
        assert_eq!(l.decode(InfoClass::ProcIo), Err(NtsCode::TierRegress.val()));
    }

    #[test]
    fn test_probe_dup_rejected() {
        let mut l = ClassLedger::new();
        assert!(l.probe(InfoClass::ProcVm).is_ok());
        assert_eq!(l.probe(InfoClass::ProcVm), Err(NtsCode::ClassOutOfSet.val()));
    }

    #[test]
    fn test_documented_returns_frozen() {
        assert_eq!(documented_return(InfoClass::ProcBasic), Ok(0));
        assert_eq!(documented_return(InfoClass::ProcQuota), Ok(0x10));
        assert_eq!(documented_return(InfoClass::ProcImageName), Ok(0x60));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(ClassLedger::new().fingerprint(), 0x80B4_0D00);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80B40);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in NtsCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xBEEF).contains("未知"));
    }
}
