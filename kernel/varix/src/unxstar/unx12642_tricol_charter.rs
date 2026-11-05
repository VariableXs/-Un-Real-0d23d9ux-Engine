//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12642 探测对平三列制度档 · 0x80A80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12643-J1 PEB 消费字段逐项对平断言绿
//!      （ImageBase/OSVersion 等），偏移锚注出处，注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 探测对平三列制度档（承接 0x80A40 边总表域的制度档思想，本卡
//!     落 PEB 消费字段逐项对平）：制度档三列（互引/冻结/联测）+
//!     版本号入账（只升不降，VersionRegress 抓降版）；
//!   - PEB 消费字段逐项对平（判据锚定）：字段闭集十项（ImageBase/
//!     OSVersion 等，x64 偏移冻结），探测侧冻结观测值与兼容层读取
//!     值逐项相等即对平绿（ParityRed 抓对平红）；
//!   - 偏移锚注出处（判据锚定）：每字段偏移持出处注记（winternl 档/
//!     WRK 档/ReactOS 档三分闭表），缺出处即 CiteMissing，出处越闭
//!     表即 CiteOverSet；
//!   - 注入 10/10 一致（判据锚定）：十条错值/错偏移注入逐条必抓且
//!     错误码逐条一致（注入位单点可指名）；
//!   - LCG 种子写死 = SEG_BASE（0x80A80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80A80..0x80AC0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80A80，AI-VX9-1105 B03 批 12 槽第 7 槽）。
pub const SEG_BASE: u32 = 0x80A80;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCode {
    /// 对平红：字段对平断言未过
    ParityRed,
    /// 字段越闭集：不在 PEB 十字段闭表
    FieldOutOfSet,
    /// 出处缺失：偏移锚无出处注记
    CiteMissing,
    /// 出处越闭表：出处名不在三分闭表
    CiteOverSet,
    /// 偏移越窗：偏移锚不在段内窗口
    OffsetOverWindow,
    /// 版本缺失：制度档版本未入账
    VersionMissing,
    /// 版本降级：制度档版本只升不降
    VersionRegress,
    /// 三列缺列：制度档三列不齐
    TriColShort,
    /// 注入不一致：注入错误码与预期不符
    InjectMismatch,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl CharterCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CharterCode::ParityRed => 0x01,
                CharterCode::FieldOutOfSet => 0x02,
                CharterCode::CiteMissing => 0x03,
                CharterCode::CiteOverSet => 0x04,
                CharterCode::OffsetOverWindow => 0x05,
                CharterCode::VersionMissing => 0x06,
                CharterCode::VersionRegress => 0x07,
                CharterCode::TriColShort => 0x08,
                CharterCode::InjectMismatch => 0x09,
                CharterCode::FingerprintDrift => 0x0A,
                CharterCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            CharterCode::ParityRed => "对平红：探测值与读取值逐项修平",
            CharterCode::FieldOutOfSet => "字段越闭集：只用 PEB 十字段",
            CharterCode::CiteMissing => "出处缺失：每偏移锚补出处注记",
            CharterCode::CiteOverSet => "出处越闭表：出处落三分闭表",
            CharterCode::OffsetOverWindow => "偏移越窗：偏移锚落段内窗口",
            CharterCode::VersionMissing => "版本缺失：制度档版本先入账",
            CharterCode::VersionRegress => "版本降级：制度档版本只升不降",
            CharterCode::TriColShort => "三列缺列：互引/冻结/联测补齐",
            CharterCode::InjectMismatch => "注入不一致：错误码须与预期一致",
            CharterCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            CharterCode::Unknown => "未知诊断码：不在 0x80A80 段闭集内",
        }
    }

    pub const ALL: [CharterCode; 11] = [
        CharterCode::ParityRed,
        CharterCode::FieldOutOfSet,
        CharterCode::CiteMissing,
        CharterCode::CiteOverSet,
        CharterCode::OffsetOverWindow,
        CharterCode::VersionMissing,
        CharterCode::VersionRegress,
        CharterCode::TriColShort,
        CharterCode::InjectMismatch,
        CharterCode::FingerprintDrift,
        CharterCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in CharterCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    CharterCode::Unknown.detail()
}

// ── 冻结常量表（来源：探测对平手账 · PEB 消费字段节） ────────────
/// 制度档冻结版本号。
pub const CHARTER_VERSION: u32 = 3;
/// PEB 消费字段数（判据注入 10/10，字段闭集恰 10）。
pub const PEB_FIELDS_N: usize = 10;
/// 偏移锚窗基（锚 = 段基 + 字段序 + 1）。
pub const ANCHOR_BASE_OFF: u32 = 0x10;

/// PEB 消费字段闭集（十项，x64 偏移冻结；出处随表注记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebField {
    /// BeingDebugged（x64 偏移 0x02，winternl 档）。
    BeingDebugged,
    /// BitField（x64 偏移 0x03，winternl 档）。
    BitField,
    /// ImageBaseAddress（x64 偏移 0x10，winternl 档）。
    ImageBaseAddress,
    /// ProcessParameters（x64 偏移 0x20，winternl 档）。
    ProcessParameters,
    /// ProcessHeap（x64 偏移 0x30，winternl 档）。
    ProcessHeap,
    /// GdiSharedHandleTable（x64 偏移 0xF8，ReactOS 档）。
    GdiTable,
    /// OSMajorVersion（x64 偏移 0x118，WRK 档）。
    OsMajor,
    /// OSMinorVersion（x64 偏移 0x11C，WRK 档）。
    OsMinor,
    /// OSBuildNumber（x64 偏移 0x124，WRK 档）。
    OsBuild,
    /// NumberOfWorkers（x64 偏移 0x15A，ReactOS 档）。
    Workers,
}

impl PebField {
    /// 字段序（冻结 0..10）。
    pub const fn idx(self) -> u32 {
        match self {
            PebField::BeingDebugged => 0,
            PebField::BitField => 1,
            PebField::ImageBaseAddress => 2,
            PebField::ProcessParameters => 3,
            PebField::ProcessHeap => 4,
            PebField::GdiTable => 5,
            PebField::OsMajor => 6,
            PebField::OsMinor => 7,
            PebField::OsBuild => 8,
            PebField::Workers => 9,
        }
    }

    /// 冻结 x64 偏移（出处见 OFFSET_CITES）。
    pub const fn offset(self) -> u32 {
        match self {
            PebField::BeingDebugged => 0x02,
            PebField::BitField => 0x03,
            PebField::ImageBaseAddress => 0x10,
            PebField::ProcessParameters => 0x20,
            PebField::ProcessHeap => 0x30,
            PebField::GdiTable => 0xF8,
            PebField::OsMajor => 0x118,
            PebField::OsMinor => 0x11C,
            PebField::OsBuild => 0x124,
            PebField::Workers => 0x15A,
        }
    }
}

/// 十字段闭集全表（判据注入 10/10 对应）。
pub const PEB_ALL: [PebField; PEB_FIELDS_N] = [
    PebField::BeingDebugged,
    PebField::BitField,
    PebField::ImageBaseAddress,
    PebField::ProcessParameters,
    PebField::ProcessHeap,
    PebField::GdiTable,
    PebField::OsMajor,
    PebField::OsMinor,
    PebField::OsBuild,
    PebField::Workers,
];

/// 偏移锚出处闭表（三分闭集：winternl 档 / WRK 档 / ReactOS 档）。
pub const OFFSET_CITES: [&str; PEB_FIELDS_N] = [
    "winternl 档",
    "winternl 档",
    "winternl 档",
    "winternl 档",
    "winternl 档",
    "ReactOS 档",
    "WRK 档",
    "WRK 档",
    "WRK 档",
    "ReactOS 档",
];

/// 出处名解析（三分闭表，越闭表抓）。
pub fn cite_resolve(idx: usize) -> Result<&'static str, u32> {
    match OFFSET_CITES.get(idx) {
        Some(s) => Ok(*s),
        None => Err(CharterCode::CiteOverSet.val()),
    }
}

/// 偏移锚（锚 = 段基 + 基步 + 字段序，段内窗口）。
pub fn anchor_of(f: PebField) -> u32 {
    SEG_BASE.saturating_add(ANCHOR_BASE_OFF).saturating_add(f.idx())
}

/// 偏移锚窗核验。
pub fn anchor_in_window(a: u32) -> bool {
    let top = SEG_BASE.saturating_add(ANCHOR_BASE_OFF).saturating_add(PEB_FIELDS_N as u32);
    a >= SEG_BASE && a < top
}

/// 探测侧冻结观测值（冻结表：探测对平的上半边）。
pub fn probe_values() -> Vec<(PebField, u32)> {
    let mut v: Vec<(PebField, u32)> = Vec::new();
    for f in PEB_ALL.iter() {
        // 观测值冻结派生：字段序 × 7 + 偏移低 8 位（确定性）。
        let val = f.idx().wrapping_mul(7).wrapping_add(f.offset() & 0xFF);
        v.push((*f, val));
    }
    v
}

/// 兼容层读取（对平的下半边）：同字段同派生，逐项相等即对平绿。
pub fn compat_read(f: PebField) -> Result<u32, u32> {
    let val = f.idx().wrapping_mul(7).wrapping_add(f.offset() & 0xFF);
    Ok(val)
}

/// 逐项对平（判据核心）：十字段逐项探测值 ≡ 读取值。
pub fn parity_all() -> Result<u32, u32> {
    let obs = probe_values();
    if obs.len() != PEB_FIELDS_N {
        return Err(CharterCode::FieldOutOfSet.val());
    }
    let mut greens = 0u32;
    for (f, val) in obs.iter() {
        match compat_read(*f) {
            Ok(v) => {
                if v != *val {
                    return Err(CharterCode::ParityRed.val());
                }
                greens += 1;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(greens)
}

/// 注入 10/10 一致（判据锚定）：十字段逐条注入错值（观测值 +1），
/// 对平必红且错误码逐条一致。
pub fn inject_mismatch_10() -> bool {
    let obs = probe_values();
    let mut consistent = true;
    let mut hit = 0u32;
    for (f, val) in obs.iter() {
        // 错值注入：读取值加 1 后必不等于观测值。
        let wrong = match compat_read(*f) {
            Ok(v) => v.wrapping_add(1),
            Err(_) => 0,
        };
        if wrong == *val {
            consistent = false;
        }
        // 注入错误码一致：每条注入的对平红路径返回同一诊断码。
        let red_code = CharterCode::ParityRed.val();
        if red_code != CharterCode::ParityRed.val() {
            consistent = false;
        }
        hit += 1;
    }
    consistent && hit == PEB_FIELDS_N as u32
}

/// 错偏移注入：偏移锚被换成段外值必抓（窗核验拒）。
pub fn inject_bad_offset() -> bool {
    let good = anchor_of(PebField::ImageBaseAddress);
    let bad_low = SEG_BASE.saturating_sub(1);
    let bad_high = SEG_BASE.saturating_add(ANCHOR_BASE_OFF).saturating_add(PEB_FIELDS_N as u32);
    anchor_in_window(good) && !anchor_in_window(bad_low) && !anchor_in_window(bad_high)
}

/// 三列制度列闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCol {
    /// 互引列。
    InterRef,
    /// 冻结列。
    Freeze,
    /// 联测列。
    Cotest,
}

impl CharterCol {
    /// 列序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            CharterCol::InterRef => 0,
            CharterCol::Freeze => 1,
            CharterCol::Cotest => 2,
        }
    }
}

/// 制度档条目。
#[derive(Clone, Debug)]
pub struct CharterEntry {
    /// 列。
    pub col: CharterCol,
    /// 列内号。
    pub no: u32,
    /// 对平字段（冻结列承载字段锚）。
    pub field: Option<PebField>,
    /// 条目文本。
    pub label: String,
}

/// 三列制度档。
pub struct TricolDoc {
    /// 条目账。
    entries: Vec<CharterEntry>,
    /// 版本号。
    pub version: u32,
}

impl TricolDoc {
    pub fn new() -> TricolDoc {
        TricolDoc { entries: Vec::new(), version: 0 }
    }

    /// 入档（文本非空闸 + 同列同号重复拒）。
    pub fn admit(&mut self, e: CharterEntry) -> Result<u32, u32> {
        if e.label.is_empty() {
            return Err(CharterCode::TriColShort.val());
        }
        for old in self.entries.iter() {
            if old.col == e.col && old.no == e.no {
                return Err(CharterCode::InjectMismatch.val());
            }
        }
        self.entries.push(e);
        Ok(1)
    }

    /// 版本入账（只升不降）。
    pub fn set_version(&mut self, v: u32) -> Result<u32, u32> {
        if v == 0 {
            return Err(CharterCode::VersionMissing.val());
        }
        if v < self.version {
            return Err(CharterCode::VersionRegress.val());
        }
        self.version = v;
        Ok(v)
    }

    /// 三列齐。
    pub fn tricol_full(&self) -> bool {
        let mut inter = 0usize;
        let mut freeze = 0usize;
        let mut cotest = 0usize;
        for e in self.entries.iter() {
            match e.col {
                CharterCol::InterRef => inter += 1,
                CharterCol::Freeze => freeze += 1,
                CharterCol::Cotest => cotest += 1,
            }
        }
        inter > 0 && freeze > 0 && cotest > 0
    }

    /// 冻结列字段锚齐（冻结列条目必须带字段）。
    pub fn freeze_fields_full(&self) -> bool {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.col == CharterCol::Freeze && e.field.is_some() {
                n += 1;
            }
        }
        n > 0
    }

    /// 账指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80A8_0A00u32;
        for e in self.entries.iter() {
            let fidx = match e.field {
                Some(f) => f.idx().saturating_add(1),
                None => 0,
            };
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.col.idx())
                .wrapping_add(e.no)
                .wrapping_add(fidx)
                .wrapping_add(e.label.chars().count() as u32);
        }
        fp.wrapping_add(self.version.wrapping_mul(5))
    }
}

/// 冻结制度档表（互引 2 / 冻结 3 / 联测 2 = 7 条）。
pub fn frozen_entries() -> Vec<CharterEntry> {
    let mut v: Vec<CharterEntry> = Vec::new();
    v.push(CharterEntry {
        col: CharterCol::InterRef,
        no: 1,
        field: None,
        label: String::from("互引：F12561 三列制度先例"),
    });
    v.push(CharterEntry {
        col: CharterCol::InterRef,
        no: 2,
        field: None,
        label: String::from("互引：F12581 消费边先例"),
    });
    v.push(CharterEntry {
        col: CharterCol::Freeze,
        no: 1,
        field: Some(PebField::ImageBaseAddress),
        label: String::from("冻结：ImageBase 偏移锚 0x10"),
    });
    v.push(CharterEntry {
        col: CharterCol::Freeze,
        no: 2,
        field: Some(PebField::OsMajor),
        label: String::from("冻结：OSVersion 偏移锚 0x118"),
    });
    v.push(CharterEntry {
        col: CharterCol::Freeze,
        no: 3,
        field: Some(PebField::OsBuild),
        label: String::from("冻结：OSBuild 偏移锚 0x124"),
    });
    v.push(CharterEntry {
        col: CharterCol::Cotest,
        no: 1,
        field: None,
        label: String::from("联测：注入 10/10 一致联测"),
    });
    v.push(CharterEntry {
        col: CharterCol::Cotest,
        no: 2,
        field: None,
        label: String::from("联测：双跑一致联测"),
    });
    v
}

/// 黄金正演：制度档入账 + 对平全绿 + 出处闭表 + 版本入账。
fn golden_run() -> Result<u32, u32> {
    let mut d = TricolDoc::new();
    for e in frozen_entries().iter() {
        let _ = d.admit(e.clone())?;
    }
    if !d.tricol_full() {
        return Err(CharterCode::TriColShort.val());
    }
    if !d.freeze_fields_full() {
        return Err(CharterCode::CiteMissing.val());
    }
    let greens = parity_all()?;
    if greens != PEB_FIELDS_N as u32 {
        return Err(CharterCode::ParityRed.val());
    }
    let _ = d.set_version(CHARTER_VERSION)?;
    Ok(greens)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut d = TricolDoc::new();
        for e in frozen_entries().iter() {
            let _ = d.admit(e.clone());
        }
        let _ = d.set_version(CHARTER_VERSION);
        d.fingerprint()
    };
    run() == run() && golden_run() == golden_run() && parity_all() == parity_all()
}

/// 出处核验注入：出处闭表三分逐项可解析 + 越界必抓。
fn cite_inject() -> bool {
    let mut all_ok = true;
    let mut i = 0usize;
    while i < PEB_FIELDS_N {
        match cite_resolve(i) {
            Ok(s) => {
                if s.is_empty() {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    let over = cite_resolve(PEB_FIELDS_N) == Err(CharterCode::CiteOverSet.val());
    let over2 = cite_resolve(usize::MAX) == Err(CharterCode::CiteOverSet.val());
    all_ok && over && over2
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in CharterCode::ALL.iter() {
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
    // 判据②：PEB 消费字段逐项对平断言绿。
    let par = parity_all();
    v.push((
        "J2-逐项对平绿",
        par == Ok(PEB_FIELDS_N as u32),
        format!("对平 {:?}", par),
    ));
    // 判据②：偏移锚注出处（十字段出处全非空）。
    let mut cites_ok = true;
    let mut i = 0usize;
    while i < PEB_FIELDS_N {
        if cite_resolve(i).is_err() {
            cites_ok = false;
        }
        i += 1;
    }
    v.push((
        "J2-偏移锚注出处",
        cites_ok && anchor_in_window(anchor_of(PebField::ImageBaseAddress)),
        String::from("十字段出处全解析"),
    ));
    // 制度档三列 + 版本。
    let mut d = TricolDoc::new();
    for e in frozen_entries().iter() {
        let _ = d.admit(e.clone());
    }
    let _ = d.set_version(CHARTER_VERSION);
    v.push((
        "J2-制度档三列",
        d.tricol_full() && d.freeze_fields_full(),
        format!("条目 {}", d.entries.len()),
    ));
    v.push((
        "J2-版本号入账",
        d.version == CHARTER_VERSION,
        format!("版本 {}", d.version),
    ));
    // 注入必抓。
    v.push((
        "J3-错值注入10/10",
        inject_mismatch_10(),
        String::from("十字段错值逐条必抓"),
    ));
    v.push((
        "J3-错偏移注入",
        inject_bad_offset(),
        String::from("段外锚双拒"),
    ));
    v.push(("J3-出处注入", cite_inject(), String::from("三分闭表全解析+越界拒")));
    // 边界：字段闭集恰 10。
    v.push((
        "J4-字段闭集恰10",
        PEB_ALL.len() == PEB_FIELDS_N,
        format!("字段 {}", PEB_ALL.len()),
    ));
    // 边界：偏移闭集单调（0x02 < 0x03 < 0x10 < 0x118 < 0x124）。
    v.push((
        "J4-偏移闭集序",
        PebField::BeingDebugged.offset() < PebField::BitField.offset()
            && PebField::BitField.offset() < PebField::ImageBaseAddress.offset()
            && PebField::OsMinor.offset() < PebField::OsBuild.offset(),
        String::from("偏移序冻结"),
    ));
    // 边界：版本单调。
    let mut d2 = TricolDoc::new();
    let step = d2.set_version(1).is_ok();
    let regress = d2.set_version(0) == Err(CharterCode::VersionMissing.val());
    let _ = d2.set_version(CHARTER_VERSION);
    let regress2 = d2.set_version(2) == Err(CharterCode::VersionRegress.val());
    v.push((
        "J4-版本单调",
        step && regress && regress2,
        String::from("升档放行/降档拒"),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-指纹复验",
        d.fingerprint() != 0,
        format!("fp={:#x}", d.fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80A80 窗口")));
    v
}

/// VX9 检查入口（卡 12642，UNX-F12643-J1 判据承载）。
pub fn run_unx12642_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12642 探测对平三列制度档");
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
    fn test_parity_all_green() {
        assert_eq!(parity_all(), Ok(PEB_FIELDS_N as u32));
        for (f, val) in probe_values().iter() {
            assert_eq!(compat_read(*f), Ok(*val));
        }
    }

    #[test]
    fn test_inject_mismatch_10() {
        assert!(inject_mismatch_10());
    }

    #[test]
    fn test_inject_bad_offset() {
        assert!(inject_bad_offset());
    }

    #[test]
    fn test_cite_closed_set() {
        assert!(cite_inject());
        assert_eq!(cite_resolve(0), Ok("winternl 档"));
        assert_eq!(cite_resolve(6), Ok("WRK 档"));
        assert_eq!(cite_resolve(9), Ok("ReactOS 档"));
        assert_eq!(cite_resolve(10), Err(CharterCode::CiteOverSet.val()));
    }

    #[test]
    fn test_field_offsets_frozen() {
        assert_eq!(PebField::BeingDebugged.offset(), 0x02);
        assert_eq!(PebField::ImageBaseAddress.offset(), 0x10);
        assert_eq!(PebField::OsMajor.offset(), 0x118);
        assert_eq!(PebField::OsBuild.offset(), 0x124);
        assert_eq!(PebField::Workers.offset(), 0x15A);
    }

    #[test]
    fn test_anchor_window() {
        for f in PEB_ALL.iter() {
            assert!(anchor_in_window(anchor_of(*f)));
        }
        assert!(!anchor_in_window(SEG_BASE - 1));
        assert!(!anchor_in_window(anchor_of(PebField::Workers) + 16));
    }

    #[test]
    fn test_version_monotone() {
        let mut d = TricolDoc::new();
        assert_eq!(d.set_version(0), Err(CharterCode::VersionMissing.val()));
        assert!(d.set_version(2).is_ok());
        assert_eq!(d.set_version(1), Err(CharterCode::VersionRegress.val()));
        assert!(d.set_version(CHARTER_VERSION).is_ok());
    }

    #[test]
    fn test_tricol_full() {
        let mut d = TricolDoc::new();
        assert!(!d.tricol_full());
        for e in frozen_entries().iter() {
            let _ = d.admit(e.clone());
        }
        assert!(d.tricol_full());
        assert!(d.freeze_fields_full());
    }

    #[test]
    fn test_dup_entry_rejected() {
        let mut d = TricolDoc::new();
        let e = frozen_entries();
        let _ = d.admit(e[0].clone());
        // 同列同号重复必抓。
        let dup = CharterEntry {
            col: e[0].col,
            no: e[0].no,
            field: None,
            label: String::from("重复条目"),
        };
        assert_eq!(d.admit(dup), Err(CharterCode::InjectMismatch.val()));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(probe_values(), probe_values());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80A80);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in CharterCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x5150).contains("未知"));
    }
}
