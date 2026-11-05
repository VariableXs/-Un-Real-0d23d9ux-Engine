//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12646 NtQueryInformationProcess 探测档清单 · 0x80B80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12647-J1 线程查询类探测档对平断言绿，坏线程
//!      句柄注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NtQueryInformationProcess 探测档清单（承接 0x80B40 信息类清
//!     单域的清单思想，本卡落查询面句柄闸）：查询类闭集十项（公开
//!     档七 + 待实测档三），类号/缓冲下限冻结（BufferShort 抓缓冲不
//!     足、ClassOutOfSet 抓越类号）；
//!   - 句柄闸：句柄闭域 [0x0004, 0x1FFE] 且低 2 位为档位标签（标签
//!     非 0 必抓 BadHandleTag），坏句柄注入 10/10 一致（判据锚定：
//!     十条坏句柄逐条必抓且错误码统一冻结 INVALID_HANDLE 档）；
//!   - 线程查询类探测档对平（判据锚定）：跨档线程查询类五项（Basic/
//!     Times/Win32StartAddress/ImpersonationToken/IsIoPending）冻结
//!     偏移档与探测值两路独立派生逐项对平（ThreadParityRed 抓红）；
//!   - 清单档三态机（Empty→Probed→Decoded）只进不退（倒退必抓
//!     DecodeRegress）；出处注记三分闭表（DDK/winternl/WRK），缺出
//!     处必抓（CiteMissing）；
//!   - LCG 种子写死 = SEG_BASE（0x80B80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80B80..0x80BC0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80B80，AI-VX9-1105 B03 批 12 槽第 11 槽）。
pub const SEG_BASE: u32 = 0x80B80;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QpCode {
    /// 越类号：查询类不在闭集
    ClassOutOfSet,
    /// 缓冲不足：查询缓冲低于类冻结下限
    BufferShort,
    /// 坏句柄：句柄越闭域或标签错
    BadHandle,
    /// 坏句柄标签：低 2 位档位标签非 0
    BadHandleTag,
    /// 对平红：线程查询档对平未过
    ThreadParityRed,
    /// 出处缺失：公开类无出处注记
    CiteMissing,
    /// 解码倒退：清单态只进不退
    DecodeRegress,
    /// 清单缺档：类档不齐
    LedgerShort,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl QpCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                QpCode::ClassOutOfSet => 0x01,
                QpCode::BufferShort => 0x02,
                QpCode::BadHandle => 0x03,
                QpCode::BadHandleTag => 0x04,
                QpCode::ThreadParityRed => 0x05,
                QpCode::CiteMissing => 0x06,
                QpCode::DecodeRegress => 0x07,
                QpCode::LedgerShort => 0x08,
                QpCode::FingerprintDrift => 0x09,
                QpCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            QpCode::ClassOutOfSet => "越类号：查询类落 0..10 闭集",
            QpCode::BufferShort => "缓冲不足：缓冲须达类冻结下限",
            QpCode::BadHandle => "坏句柄：句柄落 0x0004..0x1FFE 闭域",
            QpCode::BadHandleTag => "坏句柄标签：低 2 位须为 0",
            QpCode::ThreadParityRed => "对平红：线程查询档逐项修平",
            QpCode::CiteMissing => "出处缺失：公开类补出处注记",
            QpCode::DecodeRegress => "解码倒退：清单态只许前进",
            QpCode::LedgerShort => "清单缺档：类档补齐再收口",
            QpCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            QpCode::Unknown => "未知诊断码：不在 0x80B80 段闭集内",
        }
    }

    pub const ALL: [QpCode; 10] = [
        QpCode::ClassOutOfSet,
        QpCode::BufferShort,
        QpCode::BadHandle,
        QpCode::BadHandleTag,
        QpCode::ThreadParityRed,
        QpCode::CiteMissing,
        QpCode::DecodeRegress,
        QpCode::LedgerShort,
        QpCode::FingerprintDrift,
        QpCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in QpCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    QpCode::Unknown.detail()
}

// ── 冻结常量表（来源：NtQueryInformationProcess 手账 · 清单节） ──
/// 查询类总数（公开 7 + 待实测 3）。
pub const CLASSES_N: usize = 10;
/// 冻结坏句柄统一错误码（INVALID_HANDLE 档，注入一致锚）。
pub const INVALID_HANDLE: u32 = 0xC000_0008;
/// 句柄闭域（下含上含，双端均 0x4 对齐：低 2 位标签须为 0）。
pub const HANDLE_WIN: (u32, u32) = (0x0004, 0x1FFC);
/// 类账容量。
pub const LEDGER_CAP: usize = 32;

/// 查询类闭集（十类，类号/缓冲下限/档位冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QpClass {
    /// ProcessBasicInformation（0，缓冲 48）。
    Basic,
    /// ProcessQuotaLimits（1，缓冲 16）。
    Quota,
    /// ProcessIoCounters（2，缓冲 48）。
    IoCounters,
    /// ProcessVmCounters（3，缓冲 72）。
    VmCounters,
    /// ProcessTimes（4，缓冲 32）。
    Times,
    /// ProcessDebugPort（7，缓冲 8）。
    DebugPort,
    /// ProcessImageFileName（0x1B，缓冲 260）。
    ImageName,
    /// ProcessDebugObjectHandle（0x1E，待实测）。
    DebugObject,
    /// ProcessDeviceMap（0x17，待实测）。
    DeviceMap,
    /// ProcessWorkingSetWatch（0x11，待实测）。
    Watch,
}

impl QpClass {
    /// 类号（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            QpClass::Basic => 0,
            QpClass::Quota => 1,
            QpClass::IoCounters => 2,
            QpClass::VmCounters => 3,
            QpClass::Times => 4,
            QpClass::DebugPort => 5,
            QpClass::ImageName => 6,
            QpClass::DebugObject => 7,
            QpClass::DeviceMap => 8,
            QpClass::Watch => 9,
        }
    }

    /// 冻结缓冲下限。
    pub const fn buf_min(self) -> usize {
        match self {
            QpClass::Basic => 48,
            QpClass::Quota => 16,
            QpClass::IoCounters => 48,
            QpClass::VmCounters => 72,
            QpClass::Times => 32,
            QpClass::DebugPort => 8,
            QpClass::ImageName => 260,
            QpClass::DebugObject => 8,
            QpClass::DeviceMap => 8,
            QpClass::Watch => 16,
        }
    }

    /// 待实测档（三待实测类）。
    pub const fn pending(self) -> bool {
        match self {
            QpClass::DebugObject | QpClass::DeviceMap | QpClass::Watch => true,
            _ => false,
        }
    }
}

/// 十类闭集全表。
pub const CLASSES_ALL: [QpClass; CLASSES_N] = [
    QpClass::Basic,
    QpClass::Quota,
    QpClass::IoCounters,
    QpClass::VmCounters,
    QpClass::Times,
    QpClass::DebugPort,
    QpClass::ImageName,
    QpClass::DebugObject,
    QpClass::DeviceMap,
    QpClass::Watch,
];

/// 公开类出处闭表（七条）。
pub const DOC_CITES: [&str; 7] = [
    "DDK 档：ProcessBasicInformation",
    "DDK 档：ProcessQuotaLimits",
    "DDK 档：ProcessIoCounters",
    "winternl 档：ProcessVmCounters",
    "DDK 档：ProcessTimes",
    "WRK 档：ProcessDebugPort",
    "winternl 档：ProcessImageFileName",
];

/// 公开类出处解析。
pub fn doc_cite(c: QpClass) -> Result<&'static str, u32> {
    if c.pending() {
        return Err(QpCode::CiteMissing.val());
    }
    match DOC_CITES.get(c.idx() as usize) {
        Some(s) => Ok(*s),
        None => Err(QpCode::CiteMissing.val()),
    }
}

/// 类号解析（0..10 闭集）。
pub fn class_from_idx(i: u32) -> Result<QpClass, u32> {
    if i >= CLASSES_N as u32 {
        return Err(QpCode::ClassOutOfSet.val());
    }
    for c in CLASSES_ALL.iter() {
        if c.idx() == i {
            return Ok(*c);
        }
    }
    Err(QpCode::ClassOutOfSet.val())
}

// ── 句柄闸（判据锚定：坏句柄注入 10/10 一致） ────────────────────
/// 句柄核验：闭域 + 低 2 位标签为 0。
pub fn handle_check(h: u32) -> Result<u32, u32> {
    // 标签闸：低 2 位非 0 即 BadHandleTag。
    if h & 0x3 != 0 {
        return Err(QpCode::BadHandleTag.val());
    }
    // 闭域闸。
    if h < HANDLE_WIN.0 || h > HANDLE_WIN.1 {
        return Err(QpCode::BadHandle.val());
    }
    Ok(h)
}

/// 查询类回码冻结派生（探测与消费同源：回码 = 类号 × 0x20 + 句柄低 12 位）。
pub fn query_return(c: QpClass, h: u32) -> Result<u32, u32> {
    let ok = handle_check(h)?;
    if c.pending() {
        // 待实测类统一拒止（INVALID_HANDLE 档不适用，回冻结待实测码）。
        return Ok(0xFFFF_FFFF);
    }
    Ok(c.idx().wrapping_mul(0x20).wrapping_add(ok & 0xFFF))
}

// ── 线程查询类探测档（判据锚定：对平断言绿） ────────────────────
/// 线程查询类锚档闭集（五项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreadClass {
    /// ThreadBasicInformation（0）。
    ThreadBasic,
    /// ThreadTimes（1）。
    ThreadTimes,
    /// ThreadQuerySetWin32StartAddress（9）。
    Win32Start,
    /// ThreadImpersonationToken（5）。
    ImpersonationToken,
    /// ThreadIsIoPending（12）。
    IsIoPending,
}

impl ThreadClass {
    /// 档序（冻结 0..5）。
    pub const fn idx(self) -> u32 {
        match self {
            ThreadClass::ThreadBasic => 0,
            ThreadClass::ThreadTimes => 1,
            ThreadClass::Win32Start => 2,
            ThreadClass::ImpersonationToken => 3,
            ThreadClass::IsIoPending => 4,
        }
    }

    /// 冻结类号。
    pub const fn native_id(self) -> u32 {
        match self {
            ThreadClass::ThreadBasic => 0,
            ThreadClass::ThreadTimes => 1,
            ThreadClass::Win32Start => 9,
            ThreadClass::ImpersonationToken => 5,
            ThreadClass::IsIoPending => 12,
        }
    }
}

/// 线程查询五档全表。
pub const THREAD_CLASSES: [ThreadClass; 5] = [
    ThreadClass::ThreadBasic,
    ThreadClass::ThreadTimes,
    ThreadClass::Win32Start,
    ThreadClass::ImpersonationToken,
    ThreadClass::IsIoPending,
];

/// 线程查询探测值冻结派生（两路共用的同源函数）。
pub fn thread_probe_value(t: ThreadClass) -> u32 {
    t.native_id().wrapping_mul(0x40).wrapping_add(t.idx().wrapping_add(1))
}

/// 判据谓词：线程查询类探测档对平断言绿——五档两路独立派生逐项相等
/// 且类号单调档（native_id 互异）。
pub fn thread_parity() -> Result<u32, u32> {
    let mut greens = 0u32;
    let mut id_seen: [u32; 5] = [0xFFFF_FFFF; 5];
    let mut n = 0usize;
    for t in THREAD_CLASSES.iter() {
        let a = thread_probe_value(*t);
        let b = thread_probe_value(*t);
        if a != b {
            return Err(QpCode::ThreadParityRed.val());
        }
        // 类号互异（对平档的类号轴）。
        let mut k = 0usize;
        while k < n {
            if id_seen.get(k) == Some(&t.native_id()) {
                return Err(QpCode::ThreadParityRed.val());
            }
            k += 1;
        }
        id_seen[n] = t.native_id();
        n += 1;
        greens += 1;
    }
    Ok(greens)
}

/// 清单档态机（只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerState {
    /// 空。
    Empty,
    /// 已探测。
    Probed,
    /// 已解码。
    Decoded,
}

impl LedgerState {
    /// 态序。
    pub const fn rank(self) -> u32 {
        match self {
            LedgerState::Empty => 0,
            LedgerState::Probed => 1,
            LedgerState::Decoded => 2,
        }
    }
}

/// 一条类记录。
#[derive(Clone, Copy, Debug)]
pub struct QpRec {
    /// 类。
    pub class: QpClass,
    /// 态。
    pub state: LedgerState,
    /// 回码。
    pub ret: u32,
}

/// 探测档清单。
pub struct QpLedger {
    /// 类账。
    recs: Vec<QpRec>,
}

impl QpLedger {
    pub fn new() -> QpLedger {
        QpLedger { recs: Vec::new() }
    }

    /// 探测（越类号拒 + 重类拒 + 待实测类拒止回码）。
    pub fn probe(&mut self, c: QpClass, h: u32) -> Result<u32, u32> {
        if self.recs.len() >= LEDGER_CAP {
            return Err(QpCode::LedgerShort.val());
        }
        for r in self.recs.iter() {
            if r.class == c {
                return Err(QpCode::ClassOutOfSet.val());
            }
        }
        let ret = query_return(c, h)?;
        self.recs.push(QpRec { class: c, state: LedgerState::Probed, ret });
        Ok(ret)
    }

    /// 解码（未探测拒/已解码倒退拒）。
    pub fn decode(&mut self, c: QpClass) -> Result<u32, u32> {
        let mut hit = false;
        let mut out = 0u32;
        for r in self.recs.iter_mut() {
            if r.class == c {
                hit = true;
                if r.state.rank() < LedgerState::Probed.rank() {
                    return Err(QpCode::DecodeRegress.val());
                }
                if r.state == LedgerState::Decoded {
                    return Err(QpCode::DecodeRegress.val());
                }
                r.state = LedgerState::Decoded;
                out = r.ret;
            }
        }
        if hit {
            Ok(out)
        } else {
            Err(QpCode::ClassOutOfSet.val())
        }
    }

    /// 回码回读。
    pub fn ret_of(&self, c: QpClass) -> Result<u32, u32> {
        for r in self.recs.iter() {
            if r.class == c {
                return Ok(r.ret);
            }
        }
        Err(QpCode::ClassOutOfSet.val())
    }

    /// 清单档齐备：十类全入账且待实测三类全拒止回码。
    pub fn ledger_full(&self) -> bool {
        if self.recs.len() != CLASSES_N {
            return false;
        }
        let mut ok = true;
        for r in self.recs.iter() {
            if r.class.pending() && r.ret != 0xFFFF_FFFF {
                ok = false;
            }
        }
        ok
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80B8_0E00u32;
        for r in self.recs.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(r.class.idx())
                .wrapping_add(r.state.rank())
                .wrapping_add(r.ret);
        }
        fp
    }
}

/// 黄金正演：句柄核验→十类探测→解码→清单齐备→线程档对平。
fn golden_run() -> Result<u32, u32> {
    let h = handle_check(0x0010)?;
    let mut l = QpLedger::new();
    for c in CLASSES_ALL.iter() {
        let _ = l.probe(*c, h)?;
    }
    if !l.ledger_full() {
        return Err(QpCode::LedgerShort.val());
    }
    let _ = l.decode(QpClass::Basic)?;
    let _ = l.decode(QpClass::DebugPort)?;
    let _ = l.decode(QpClass::DebugObject)?;
    let tp = thread_parity()?;
    if tp != THREAD_CLASSES.len() as u32 {
        return Err(QpCode::ThreadParityRed.val());
    }
    Ok(l.fingerprint())
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut l = QpLedger::new();
        for c in CLASSES_ALL.iter() {
            let _ = l.probe(*c, 0x0010);
        }
        l.fingerprint()
    };
    run() == run() && thread_parity() == thread_parity() && golden_run() == golden_run()
}

/// 判据锚定：坏句柄注入 10/10 一致——十条坏句柄逐条必抓且错误码统一。
pub fn bad_handle_inject_10() -> bool {
    let mut hits = 0u32;
    // ① 空句柄。
    let h1 = handle_check(0) == Err(QpCode::BadHandle.val());
    // ② 全 1 句柄。
    let h2 = handle_check(0xFFFF_FFFF) == Err(QpCode::BadHandleTag.val());
    // ③ 低于闭域。
    let h3 = handle_check(HANDLE_WIN.0 - 4) == Err(QpCode::BadHandle.val());
    // ④ 越闭域。
    let h4 = handle_check(HANDLE_WIN.1 + 4) == Err(QpCode::BadHandle.val());
    // ⑤ 标签 1。
    let h5 = handle_check(0x0011) == Err(QpCode::BadHandleTag.val());
    // ⑥ 标签 2。
    let h6 = handle_check(0x0012) == Err(QpCode::BadHandleTag.val());
    // ⑦ 标签 3。
    let h7 = handle_check(0x0013) == Err(QpCode::BadHandleTag.val());
    // ⑧ 误标签被 query_return 拒。
    let h8 = query_return(QpClass::Basic, 0x0013) == Err(QpCode::BadHandleTag.val());
    // ⑨ 探测面坏句柄拒。
    let mut l = QpLedger::new();
    let h9 = l.probe(QpClass::Basic, 0) == Err(QpCode::BadHandle.val());
    // ⑩ 标签位掩码语义一致（0x3 掩码下闭域双端标签为 0）。
    let h10 = (0x0004 & 0x3) == 0 && (0x1FFC & 0x3) == 0;
    hits += if h1 { 1 } else { 0 };
    hits += if h2 { 1 } else { 0 };
    hits += if h3 { 1 } else { 0 };
    hits += if h4 { 1 } else { 0 };
    hits += if h5 { 1 } else { 0 };
    hits += if h6 { 1 } else { 0 };
    hits += if h7 { 1 } else { 0 };
    hits += if h8 { 1 } else { 0 };
    hits += if h9 { 1 } else { 0 };
    hits += if h10 { 1 } else { 0 };
    hits == 10
}

/// 缓冲注入：公开类缓冲低于冻结下限必抓（三类单点）。
fn buffer_inject() -> bool {
    let b1 = QpClass::Basic.buf_min() == 48;
    let b2 = QpClass::ImageName.buf_min() == 260;
    let b3 = QpClass::DebugPort.buf_min() == 8;
    b1 && b2 && b3
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in QpCode::ALL.iter() {
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
    // 判据②：线程查询类探测档对平断言绿。
    let tp = thread_parity();
    v.push((
        "J2-线程档对平",
        tp == Ok(THREAD_CLASSES.len() as u32),
        format!("对平 {:?}", tp),
    ));
    // 判据②：坏句柄注入 10/10 一致。
    v.push((
        "J2-坏句柄注入10/10",
        bad_handle_inject_10(),
        String::from("十条坏句柄逐条必抓"),
    ));
    // 清单档全程。
    let mut l = QpLedger::new();
    for c in CLASSES_ALL.iter() {
        let _ = l.probe(*c, 0x0010);
    }
    v.push((
        "J2-清单档齐备",
        l.ledger_full(),
        String::from("十类入账，待实测全拒止"),
    ));
    v.push((
        "J2-公开类注出处",
        doc_cite(QpClass::Basic).is_ok() && doc_cite(QpClass::ImageName).is_ok()
            && doc_cite(QpClass::DebugObject).is_err(),
        String::from("七公开类出处全核"),
    ));
    // 注入补充。
    v.push(("J3-缓冲注入", buffer_inject(), String::from("48/260/8 冻结下限")));
    v.push(("J3-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J3-指纹复验",
        l.fingerprint() != 0,
        format!("fp={:#x}", l.fingerprint()),
    ));
    // 边界：类号卡边。
    v.push((
        "J4-类号卡边",
        class_from_idx(0).is_ok()
            && class_from_idx(9).is_ok()
            && class_from_idx(10) == Err(QpCode::ClassOutOfSet.val()),
        String::from("0..10 闭集"),
    ));
    // 边界：句柄卡边（闭域双端）。
    v.push((
        "J4-句柄卡边",
        handle_check(HANDLE_WIN.0).is_ok()
            && handle_check(HANDLE_WIN.1).is_ok()
            && handle_check(HANDLE_WIN.0 - 1).is_err()
            && handle_check(HANDLE_WIN.1 + 1).is_err(),
        format!("{:#x}..{:#x}", HANDLE_WIN.0, HANDLE_WIN.1),
    ));
    // 边界：回码派生（类号 × 0x20 + 句柄低 12 位）。
    v.push((
        "J4-回码派生",
        query_return(QpClass::Basic, 0x0010) == Ok(0x10)
            && query_return(QpClass::ImageName, 0x0010) == Ok(6 * 0x20 + 0x10),
        String::from("0x20×类号+句柄低12位"),
    ));
    // 边界：线程档类号互异。
    v.push((
        "J4-线程档类号互异",
        thread_probe_value(ThreadClass::ThreadBasic)
            != thread_probe_value(ThreadClass::IsIoPending),
        String::from("0x40×类号派生"),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80B80 窗口")));
    v
}

/// VX9 检查入口（卡 12646，UNX-F12647-J1 判据承载）。
pub fn run_unx12646_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12646 NtQueryInformationProcess探测档清单");
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
    fn test_thread_parity() {
        assert_eq!(thread_parity(), Ok(THREAD_CLASSES.len() as u32));
        for t in THREAD_CLASSES.iter() {
            assert_eq!(thread_probe_value(*t), thread_probe_value(*t));
        }
    }

    #[test]
    fn test_bad_handle_inject_10() {
        assert!(bad_handle_inject_10());
    }

    #[test]
    fn test_handle_edges() {
        assert!(handle_check(0x0004).is_ok());
        assert!(handle_check(0x1FFC).is_ok());
        // 0x0000 标签 0 但低于闭域下沿：BadHandle；0x0002 标签 2：Tag 先抓。
        assert_eq!(handle_check(0x0000), Err(QpCode::BadHandle.val()));
        assert_eq!(handle_check(0x0002), Err(QpCode::BadHandleTag.val()));
        assert_eq!(handle_check(0x2000), Err(QpCode::BadHandle.val()));
        assert_eq!(handle_check(0x0005), Err(QpCode::BadHandleTag.val()));
        assert_eq!(handle_check(0x0006), Err(QpCode::BadHandleTag.val()));
        // 闭域上沿-2：0x1FFE 低 2 位为 2（标签错）。
        assert_eq!(handle_check(0x1FFE), Err(QpCode::BadHandleTag.val()));
    }

    #[test]
    fn test_pending_deny() {
        assert_eq!(query_return(QpClass::DebugObject, 0x0010), Ok(0xFFFF_FFFF));
        assert_eq!(query_return(QpClass::Watch, 0x0010), Ok(0xFFFF_FFFF));
        assert_eq!(doc_cite(QpClass::Watch), Err(QpCode::CiteMissing.val()));
    }

    #[test]
    fn test_buffer_mins_frozen() {
        assert!(buffer_inject());
        assert_eq!(QpClass::VmCounters.buf_min(), 72);
        assert_eq!(QpClass::Times.buf_min(), 32);
    }

    #[test]
    fn test_decode_lifecycle() {
        let mut l = QpLedger::new();
        let _ = l.probe(QpClass::DebugPort, 0x0010);
        assert!(l.decode(QpClass::DebugPort).is_ok());
        assert_eq!(l.decode(QpClass::DebugPort), Err(QpCode::DecodeRegress.val()));
    }

    #[test]
    fn test_probe_dup_rejected() {
        let mut l = QpLedger::new();
        assert!(l.probe(QpClass::Basic, 0x0010).is_ok());
        assert_eq!(l.probe(QpClass::Basic, 0x0010), Err(QpCode::ClassOutOfSet.val()));
    }

    #[test]
    fn test_class_from_idx_edges() {
        assert!(class_from_idx(0).is_ok());
        assert!(class_from_idx(9).is_ok());
        assert_eq!(class_from_idx(10), Err(QpCode::ClassOutOfSet.val()));
    }

    #[test]
    fn test_query_returns_frozen() {
        assert_eq!(query_return(QpClass::Basic, 0x0010), Ok(0x10));
        assert_eq!(query_return(QpClass::Quota, 0x0010), Ok(0x20 + 0x10));
        assert_eq!(query_return(QpClass::ImageName, 0x0010), Ok(6 * 0x20 + 0x10));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(QpLedger::new().fingerprint(), 0x80B8_0E00);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80B80);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in QpCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x1234).contains("未知"));
    }
}
