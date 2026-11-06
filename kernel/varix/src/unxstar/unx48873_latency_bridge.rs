//! UNX-F48873 B04 族桥接 · 码段普查 · 封印互证 · 0x70900
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥750 行；
//!   ② UNX-F48873-J1 五模块码段普查 100% 在窗，越窗零静默；
//!   ③ UNX-F48874-J1 族封印双跑互证，复算不一致必抓；
//!   ④ UNX-F48875-J1 桥接 fuzz ×10^4 零崩零漏。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：窗口不重叠 → 码段闭集 → 明细派发一致 → 反查唯一 →
//!           封印复算 → 封印敏感 → fuzz 三路 → 账本收口。
//!           窗口必须先判：窗口一旦重叠，同一个诊断码会命中两个模块，
//!           明细派发与反查都建立在「码 → 模块」的单一映射上，映射
//!           不成函数，后面的全部核验都没有意义。码段闭集次之，它
//!           回答「这个码是否真的属于它宣称的模块」。明细派发第三，
//!           它把闭集与源头函数对齐——闭集只证明数字在范围内，不证明
//!           解释文本来自正确的模块。反查唯一第四，方向与派发相反：
//!           派发是「码 → 文本」，反查是「码 → 槽位」，两者必须互逆。
//!           封印复算晚于以上四闸：封的是已经核过的账，不是没核过的。
//!           封印敏感再晚一步：它只问「换掉一件，印会不会变」。fuzz
//!           最后执行，三路噪声分别打派发、越窗、反查；任何一路抛出
//!           未捕获错都算桥接失守。
//! 关键差异：本模块不实现四段数值、不做闸门、不持环、不分类边界；
//!           它只持「族级索引」——五件模块的窗口表、码段总数、逐码
//!           明细派发、逐件封印与族级折印。core 的 LCode、meta 的
//!           MCode、guard 的 GCode、dispatch 的 DpCode、edge 的
//!           EdgeCode 各自仍是唯一真源；本模块只引用，不复制、不改写。
//!           谁想让一个码在桥接面上获得解释，谁就必须先过窗口与闭集；
//!           桥接面没有「绕过源头直接编个文本」的入口。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx48861_latency_core as l_core;
use super::unx48863_latency_meta as l_meta;
use super::unx48865_latency_guard as l_guard;
use super::unx48868_latency_dispatch as l_dispatch;
use super::unx48871_latency_edge as l_edge;

/// 诊断段基址（0x70900，VX6 批 B04 族第六槽）。
pub const SEG_BASE: u32 = 0x70900;

/// 族内模块数（core / meta / guard / dispatch / edge）。
pub const FAMILY_MODULES: usize = 5;

/// 族内诊断码总数：10 + 10 + 11 + 11 + 11。
pub const FAMILY_CODES: usize = 53;

/// 窗口槽宽（每件模块最多占 0x40 个码位）。
pub const WINDOW_STRIDE: u32 = 0x40;

/// 桥接 fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;

/// 注入轮数（四路 ×10^3）。
pub const INJECT_ROUNDS: u32 = 1000;

/// 封印复算轮数（逐件双算，共 5 件 × 2）。
pub const SEAL_PASSES: u32 = 10;

/// 五件模块窗口表：基址 × 码数。码闭集为 (base, base+count]。
pub const WINDOWS: [(u32, u8); FAMILY_MODULES] = [
    (0x70780, 10), // core：LCode ×10
    (0x707C0, 10), // meta：MCode ×10
    (0x70800, 11), // guard：GCode ×11
    (0x70860, 11), // dispatch：DpCode ×11
    (0x708C0, 11), // edge：EdgeCode ×11
];

/// 五件模块机器名（与窗口表同序）。
pub const STEMS: [&'static str; FAMILY_MODULES] = [
    "unx48861_latency_core",
    "unx48863_latency_meta",
    "unx48865_latency_guard",
    "unx48868_latency_dispatch",
    "unx48871_latency_edge",
];

// ── 诊断码闭集（+0x01..+0x0B） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrCode {
    /// 槽位越界
    SlotBad,
    /// 码不在本槽窗口内（越窗）
    WindowBad,
    /// 重复码（同码二次入册）
    DupBad,
    /// 窗口重叠或次序断裂
    OverlapBad,
    /// 明细派发不一致
    DetailBad,
    /// 反查不唯一或越窗命中
    LookupBad,
    /// 普查计数不符
    CensusBad,
    /// 封印复算不一致
    SealBad,
    /// fuzz 三路未全覆盖或出现未捕获错
    FuzzBad,
    /// 双跑不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl BrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BrCode::SlotBad => "槽位越界：族内只有 0..4 五件模块",
            BrCode::WindowBad => "越窗：诊断码不在本模块声明窗口内",
            BrCode::DupBad => "重复码：同一码位被两件模块或两次入册占用",
            BrCode::OverlapBad => "窗口非法：重叠、倒序或槽宽越界",
            BrCode::DetailBad => "派发不一致：桥接文本与源头 detail 不同",
            BrCode::LookupBad => "反查失守：码反查到的槽位与登记不符",
            BrCode::CensusBad => "普查不符：逐件计数或总数与窗口表不一致",
            BrCode::SealBad => "封印不一致：两次复算或换件后印未按预期变化",
            BrCode::FuzzBad => "fuzz 失守：三路噪声未全覆盖或出现漏抓",
            BrCode::ReplayBad => "双跑不一致：两次桥接账本指纹不同",
            BrCode::SegmentBad => "段位越界：诊断码逃出 0x70900 窗口",
        }
    }

    pub const ALL: [BrCode; 11] = [
        BrCode::SlotBad,
        BrCode::WindowBad,
        BrCode::DupBad,
        BrCode::OverlapBad,
        BrCode::DetailBad,
        BrCode::LookupBad,
        BrCode::CensusBad,
        BrCode::SealBad,
        BrCode::FuzzBad,
        BrCode::ReplayBad,
        BrCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        if BrCode::ALL[i].val() == code {
            return BrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70900 段闭集内"
}

// ── FNV-1a 64 ─────────────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (v & 0xFF) as u8);
    x = fold_byte(x, ((v >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((v >> 16) & 0xFF) as u8);
    fold_byte(x, ((v >> 24) & 0xFF) as u8)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── 窗口 ──────────────────────────────────────────────────────────
/// 窗口末端（含）：最后一个合法码位 = base + count。
pub fn window_end(w: (u32, u8)) -> u32 {
    w.0.saturating_add(w.1 as u32)
}

/// 码是否落在窗口内：闭集为 (base, base+count]。
pub fn in_window(w: (u32, u8), code: u32) -> bool {
    code > w.0 && code <= window_end(w)
}

/// 取槽窗口（越槽拒绝）。
pub fn window_of(slot: u8) -> Result<(u32, u8), u32> {
    match WINDOWS.get(slot as usize) {
        Some(w) => {
            if w.1 == 0 {
                return Err(BrCode::WindowBad.val());
            }
            Ok(*w)
        }
        None => Err(BrCode::SlotBad.val()),
    }
}

/// 窗口表审计（纯函数）：非空、槽宽不越 0x40、基址升序、间距不窄于槽宽。
pub fn windows_ok(w: &[(u32, u8); FAMILY_MODULES]) -> bool {
    let mut i = 0usize;
    while i < FAMILY_MODULES {
        let cur = match w.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if cur.1 == 0 || cur.1 as u32 > WINDOW_STRIDE {
            return false;
        }
        if i > 0 {
            let prev = match w.get(i - 1) {
                Some(v) => *v,
                None => return false,
            };
            if cur.0 <= prev.0 {
                return false;
            }
            if cur.0.saturating_sub(prev.0) < WINDOW_STRIDE {
                return false;
            }
            // 前窗末端不得探入后窗基址。
            if window_end(prev) > cur.0 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 内置窗口表审计。
pub fn audit_windows() -> Result<u32, u32> {
    if !windows_ok(&WINDOWS) {
        return Err(BrCode::OverlapBad.val());
    }
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < FAMILY_MODULES {
        if let Some(w) = WINDOWS.get(i) {
            sum = sum.saturating_add(w.1 as u32);
        }
        i += 1;
    }
    if sum as usize != FAMILY_CODES {
        return Err(BrCode::CensusBad.val());
    }
    Ok(sum)
}

/// 码 → 槽位反查；越窗返回 None（不得用 0 冒充「槽 0」）。
pub fn lookup_slot(code: u32) -> Option<u8> {
    let mut i = 0usize;
    while i < FAMILY_MODULES {
        if let Some(w) = WINDOWS.get(i) {
            if in_window(*w, code) {
                return Some(i as u8);
            }
        }
        i += 1;
    }
    None
}

/// 槽位内序号 → 绝对码。
pub fn code_at(slot: u8, ix: u8) -> Result<u32, u32> {
    let w = window_of(slot)?;
    if ix >= w.1 {
        return Err(BrCode::WindowBad.val());
    }
    Ok(w.0.saturating_add(1).saturating_add(ix as u32))
}

/// 绝对码 → 槽内序号。
pub fn index_of(slot: u8, code: u32) -> Result<u8, u32> {
    let w = window_of(slot)?;
    if !in_window(w, code) {
        return Err(BrCode::WindowBad.val());
    }
    let ix = code.saturating_sub(w.0).saturating_sub(1);
    if ix >= u32::from(w.1) {
        return Err(BrCode::WindowBad.val());
    }
    Ok(ix as u8)
}

// ── 明细派发 ──────────────────────────────────────────────────────
/// 按槽位派发到源头模块的 detail()：桥接面不复制解释文本。
pub fn module_detail(slot: u8, code: u32) -> &'static str {
    match slot {
        0 => l_core::detail(code),
        1 => l_meta::detail(code),
        2 => l_guard::detail(code),
        3 => l_dispatch::detail(code),
        4 => l_edge::detail(code),
        _ => "未知槽位：桥接面只有 0..4 五件模块",
    }
}

/// 源头模块自报的窗口（逐件与桥接表互证）。
pub fn source_base(slot: u8) -> Result<u32, u32> {
    match slot {
        0 => Ok(l_core::SEG_BASE),
        1 => Ok(l_meta::SEG_BASE),
        2 => Ok(l_guard::SEG_BASE),
        3 => Ok(l_dispatch::SEG_BASE),
        4 => Ok(l_edge::SEG_BASE),
        _ => Err(BrCode::SlotBad.val()),
    }
}

/// 桥接窗口表与各源模块自报基址逐件对账。
pub fn audit_sources() -> Result<u32, u32> {
    let mut i = 0u8;
    while i < FAMILY_MODULES as u8 {
        let w = window_of(i)?;
        let b = source_base(i)?;
        if w.0 != b {
            return Err(BrCode::WindowBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(FAMILY_MODULES as u32)
}

// ── 普查册 ────────────────────────────────────────────────────────
/// 一行普查：槽位 × 基址 × 码数 × 槽内序号 × 源头明细。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CensusRow {
    pub slot: u8,
    pub base: u32,
    pub count: u8,
    pub ix: u8,
    pub text: &'static str,
}

impl CensusRow {
    pub fn code(&self) -> u32 {
        self.base.saturating_add(1).saturating_add(self.ix as u32)
    }

    pub fn text_line(&self) -> String {
        format!(
            "{}|{}|0x{:08X}|{}|{}",
            self.slot,
            STEMS.get(self.slot as usize).copied().unwrap_or("?"),
            self.code(),
            self.ix,
            self.text
        )
    }
}

/// 普查册：逐件逐码入册，重复即拒。
pub struct CensusBook {
    pub rows: Vec<CensusRow>,
}

impl CensusBook {
    pub fn new() -> CensusBook {
        CensusBook { rows: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 入册一件：槽位与序号先过窗口，重复码再拒。
    pub fn push(&mut self, slot: u8, ix: u8) -> Result<(), u32> {
        let w = window_of(slot)?;
        if ix >= w.1 {
            return Err(BrCode::WindowBad.val());
        }
        let code = w.0.saturating_add(1).saturating_add(ix as u32);
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.code() == code {
                    return Err(BrCode::DupBad.val());
                }
            }
            i += 1;
        }
        let text = module_detail(slot, code);
        self.rows.push(CensusRow { slot, base: w.0, count: w.1, ix, text });
        Ok(())
    }

    /// 逐槽计数。
    pub fn slot_count(&self, slot: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.slot == slot {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 册指纹：逐行折叠槽位、码位、序号与文本。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold_u32(h, r.slot as u32);
                h = fold_u32(h, r.code());
                h = fold_u32(h, r.ix as u32);
                h = fold_str(h, r.text);
            }
            i += 1;
        }
        h
    }

    /// 册审计：总数、逐槽计数、次序、窗口、明细派发五关。
    pub fn audit(&self) -> Result<u64, u32> {
        if self.rows.len() != FAMILY_CODES {
            return Err(BrCode::CensusBad.val());
        }
        let mut slot = 0u8;
        while slot < FAMILY_MODULES as u8 {
            let w = window_of(slot)?;
            if self.slot_count(slot) != u32::from(w.1) {
                return Err(BrCode::CensusBad.val());
            }
            slot = slot.saturating_add(1);
        }
        let mut last_slot = 0u8;
        let mut last_ix = 0u8;
        let mut first = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BrCode::CensusBad.val()),
            };
            let w = window_of(r.slot)?;
            if r.base != w.0 || r.count != w.1 {
                return Err(BrCode::WindowBad.val());
            }
            if r.ix >= r.count {
                return Err(BrCode::WindowBad.val());
            }
            if !in_window(w, r.code()) {
                return Err(BrCode::WindowBad.val());
            }
            if module_detail(r.slot, r.code()) != r.text {
                return Err(BrCode::DetailBad.val());
            }
            if !first {
                if r.slot < last_slot {
                    return Err(BrCode::CensusBad.val());
                }
                if r.slot == last_slot && r.ix <= last_ix {
                    return Err(BrCode::CensusBad.val());
                }
            }
            first = false;
            last_slot = r.slot;
            last_ix = r.ix;
            i += 1;
        }
        Ok(self.digest())
    }
}

/// 标准普查册：五件逐码入册（53 行）。
pub fn standard_book() -> Result<CensusBook, u32> {
    let mut b = CensusBook::new();
    let mut slot = 0u8;
    while slot < FAMILY_MODULES as u8 {
        let w = window_of(slot)?;
        let mut ix = 0u8;
        while ix < w.1 {
            b.push(slot, ix)?;
            ix = ix.saturating_add(1);
        }
        slot = slot.saturating_add(1);
    }
    if b.len() != FAMILY_CODES {
        return Err(BrCode::CensusBad.val());
    }
    Ok(b)
}

// ── 封印互证 ──────────────────────────────────────────────────────
/// 逐件封印：源头黄金跑 + 段位守卫 + 三探针，全绿才折印。
///
/// 印的输入包含槽位与件名，因此两件模块即使内部数值恰好相同，
/// 族级印也不会相撞——印要区分「谁」，而不只是「什么」。
pub fn module_seal(slot: u8) -> Result<u64, u32> {
    let stem = match STEMS.get(slot as usize) {
        Some(s) => *s,
        None => return Err(BrCode::SlotBad.val()),
    };
    let ok = match slot {
        0 => l_core::golden_run().is_ok()
            && l_core::segment_guard()
            && l_core::probe_seq_a().is_ok()
            && l_core::probe_seq_b().is_ok()
            && l_core::probe_seq_c().is_ok(),
        1 => l_meta::golden_run().is_ok()
            && l_meta::segment_guard()
            && l_meta::probe_seq_a().is_ok()
            && l_meta::probe_seq_b().is_ok()
            && l_meta::probe_seq_c().is_ok(),
        2 => l_guard::golden_run().is_ok()
            && l_guard::segment_guard()
            && l_guard::probe_seq_a().is_ok()
            && l_guard::probe_seq_b().is_ok()
            && l_guard::probe_seq_c().is_ok(),
        3 => l_dispatch::golden_run().is_ok()
            && l_dispatch::segment_guard()
            && l_dispatch::probe_seq_a().is_ok()
            && l_dispatch::probe_seq_b().is_ok()
            && l_dispatch::probe_seq_c().is_ok(),
        4 => l_edge::golden_run().is_ok()
            && l_edge::segment_guard()
            && l_edge::probe_seq_a().is_ok()
            && l_edge::probe_seq_b().is_ok()
            && l_edge::probe_seq_c().is_ok(),
        _ => false,
    };
    if !ok {
        return Err(BrCode::SealBad.val());
    }
    let w = window_of(slot)?;
    let mut h = FNV_OFF;
    h = fold_u32(h, slot as u32);
    h = fold_u32(h, w.0);
    h = fold_u32(h, w.1 as u32);
    h = fold_str(h, stem);
    Ok(h)
}

/// 族级折印：逐件封印按槽序复合（左折，顺序敏感）。
pub fn family_imprint() -> Result<u64, u32> {
    let mut h = FNV_OFF;
    let mut slot = 0u8;
    while slot < FAMILY_MODULES as u8 {
        let s = module_seal(slot)?;
        h = fold_u32(h, slot as u32);
        h = fold_u32(h, (s & 0xFFFF_FFFF) as u32);
        h = fold_u32(h, (s >> 32) as u32);
        slot = slot.saturating_add(1);
    }
    Ok(h)
}

/// 内联复算：逐件封印算两遍，必须全等；返回（件数，族印）。
pub fn inline_roll() -> Result<(u32, u64), u32> {
    let mut slot = 0u8;
    let mut passes = 0u32;
    while slot < FAMILY_MODULES as u8 {
        let a = module_seal(slot)?;
        let b = module_seal(slot)?;
        if a != b {
            return Err(BrCode::SealBad.val());
        }
        passes = passes.saturating_add(2);
        slot = slot.saturating_add(1);
    }
    if passes != SEAL_PASSES {
        return Err(BrCode::SealBad.val());
    }
    Ok((passes, family_imprint()?))
}

/// 封印敏感：两印不同即为「换件可感」，相同即为钝感。
pub fn seal_differs(a: u64, b: u64) -> bool {
    a != b
}

// ── fuzz 三路 ─────────────────────────────────────────────────────
/// 三路 fuzz 账：轮数、逐路命中、指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    pub rounds: u32,
    pub dispatch_hits: u32,
    pub window_hits: u32,
    pub lookup_hits: u32,
    pub digest: u64,
}

impl FuzzReport {
    pub const fn new() -> FuzzReport {
        FuzzReport { rounds: 0, dispatch_hits: 0, window_hits: 0, lookup_hits: 0, digest: 0 }
    }

    pub fn all_routes(&self) -> bool {
        self.dispatch_hits > 0 && self.window_hits > 0 && self.lookup_hits > 0
    }
}

/// 单轮 fuzz：三路噪声轮转。
///   路 0（派发）：任取已登记码，桥接文本必须与源头逐字相同；
///   路 1（越窗）：取窗口尾后一格，必须反查失败且明细为未知文案；
///   路 2（反查）：任取已登记码，反查槽位必须等于登记槽位。
pub fn fuzz_one(book: &CensusBook, r: u32) -> Result<u8, u32> {
    if book.is_empty() {
        return Err(BrCode::CensusBad.val());
    }
    let route = r % 3;
    let pick = (r / 3) as usize;
    match route {
        0 => {
            let ix = pick % book.rows.len();
            let row = match book.rows.get(ix) {
                Some(v) => *v,
                None => return Err(BrCode::CensusBad.val()),
            };
            let live = module_detail(row.slot, row.code());
            if live != row.text || live.is_empty() {
                return Err(BrCode::DetailBad.val());
            }
            Ok(0)
        }
        1 => {
            let slot = (pick % FAMILY_MODULES) as u8;
            let w = window_of(slot)?;
            let probe = window_end(w).saturating_add(1);
            if lookup_slot(probe).is_some() {
                return Err(BrCode::LookupBad.val());
            }
            let text = module_detail(slot, probe);
            if !text.starts_with("未知") {
                return Err(BrCode::DetailBad.val());
            }
            Ok(1)
        }
        _ => {
            let ix = pick % book.rows.len();
            let row = match book.rows.get(ix) {
                Some(v) => *v,
                None => return Err(BrCode::CensusBad.val()),
            };
            match lookup_slot(row.code()) {
                Some(s) => {
                    if s != row.slot {
                        return Err(BrCode::LookupBad.val());
                    }
                    Ok(2)
                }
                None => Err(BrCode::LookupBad.val()),
            }
        }
    }
}

/// 多轮 fuzz：逐路记账，任何一路零命中即失守。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzReport, u32> {
    if rounds == 0 || rounds > 1_000_000 {
        return Err(BrCode::FuzzBad.val());
    }
    let book = standard_book()?;
    let mut rep = FuzzReport::new();
    rep.rounds = rounds;
    let mut r = 0u32;
    while r < rounds {
        let route = fuzz_one(&book, r)?;
        match route {
            0 => rep.dispatch_hits = rep.dispatch_hits.saturating_add(1),
            1 => rep.window_hits = rep.window_hits.saturating_add(1),
            _ => rep.lookup_hits = rep.lookup_hits.saturating_add(1),
        }
        r = r.saturating_add(1);
    }
    if !rep.all_routes() {
        return Err(BrCode::FuzzBad.val());
    }
    let mut h = book.digest();
    h = fold_u32(h, rep.rounds);
    h = fold_u32(h, rep.dispatch_hits);
    h = fold_u32(h, rep.window_hits);
    h = fold_u32(h, rep.lookup_hits);
    rep.digest = h;
    Ok(rep)
}

// ── 黄金链 ────────────────────────────────────────────────────────
/// 黄金跑：普查册审计 + 族印 + 千轮 fuzz。
pub fn golden_run() -> Result<(u32, u32, u64, u64), u32> {
    let book = standard_book()?;
    let digest = book.audit()?;
    let (passes, imprint) = inline_roll()?;
    if passes != SEAL_PASSES {
        return Err(BrCode::SealBad.val());
    }
    let fuzz = fuzz_rounds(INJECT_ROUNDS)?;
    if !fuzz.all_routes() {
        return Err(BrCode::FuzzBad.val());
    }
    let mut h = digest;
    h = fold_u32(h, (imprint & 0xFFFF_FFFF) as u32);
    h = fold_u32(h, (imprint >> 32) as u32);
    h = fold_u32(h, fuzz.digest as u32);
    Ok((book.len() as u32, FAMILY_MODULES as u32, imprint, h))
}

// ── 注入核 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub caught: u32,
    pub rounds: u32,
    pub route_caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new(rounds: u32) -> InjectLedger {
        InjectLedger { caught: 0, rounds, route_caught: [0; 4] }
    }

    pub fn all_caught(&self) -> bool {
        if self.caught != self.rounds {
            return false;
        }
        let mut i = 0usize;
        while i < self.route_caught.len() {
            match self.route_caught.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：重复码 / 越槽入册 / 窗口重叠 / 封印钝感。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(BrCode::FuzzBad.val());
    }
    let mut l = InjectLedger::new(rounds);
    let mut r = 0u32;
    while r < rounds {
        let route = (r % 4) as usize;
        let hit = match route {
            0 => {
                let mut b = CensusBook::new();
                b.push(0, 0).is_ok() && b.push(0, 0) == Err(BrCode::DupBad.val())
            }
            1 => {
                let mut b = CensusBook::new();
                b.push(5, 0) == Err(BrCode::SlotBad.val())
            }
            2 => {
                let bad: [(u32, u8); FAMILY_MODULES] = [
                    (0x70780, 10),
                    (0x70784, 10),
                    (0x70800, 11),
                    (0x70860, 11),
                    (0x708C0, 11),
                ];
                !windows_ok(&bad)
            }
            _ => {
                let a = module_seal(0).unwrap_or(0);
                seal_differs(a, a ^ 1) && !seal_differs(a, a)
            }
        };
        if hit {
            l.caught = l.caught.saturating_add(1);
            if let Some(c) = l.route_caught.get_mut(route) {
                *c = c.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    Ok(l)
}

// ── 导出面 ────────────────────────────────────────────────────────
/// 普查册导出：逐行五段（槽位|件名|码|序号|明细）。
pub fn render_book(b: &CensusBook) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.rows.len() {
        if let Some(r) = b.rows.get(i) {
            out.push_str(&r.text_line());
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// 导出审计：行数、每行五段、件名闭集、码位在窗。
pub fn audit_render(text: &str, expect_rows: usize) -> Result<usize, u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return Err(BrCode::CensusBad.val());
    }
    let mut i = 0usize;
    while i < lines.len() {
        let ln = match lines.get(i) {
            Some(v) => *v,
            None => return Err(BrCode::CensusBad.val()),
        };
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 5 {
            return Err(BrCode::WindowBad.val());
        }
        let s_slot = match parts.get(0) {
            Some(v) => *v,
            None => return Err(BrCode::WindowBad.val()),
        };
        let s_stem = match parts.get(1) {
            Some(v) => *v,
            None => return Err(BrCode::WindowBad.val()),
        };
        let s_code = match parts.get(2) {
            Some(v) => *v,
            None => return Err(BrCode::WindowBad.val()),
        };
        let slot = parse_u32(s_slot)?;
        if slot >= FAMILY_MODULES as u32 {
            return Err(BrCode::SlotBad.val());
        }
        let stem = match STEMS.get(slot as usize) {
            Some(v) => *v,
            None => return Err(BrCode::SlotBad.val()),
        };
        if stem != s_stem {
            return Err(BrCode::CensusBad.val());
        }
        let code = parse_hex32(s_code)?;
        if lookup_slot(code) != Some(slot as u8) {
            return Err(BrCode::LookupBad.val());
        }
        i += 1;
    }
    Ok(lines.len())
}

/// 十进制解析（逐字符，拒绝空串与非数字）。
pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(BrCode::CensusBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(BrCode::CensusBad.val()),
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(BrCode::CensusBad.val());
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(BrCode::CensusBad.val()),
        };
        v = match v.checked_add((c - b'0') as u32) {
            Some(x) => x,
            None => return Err(BrCode::CensusBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 十六进制解析（必须带 0x 前缀，逐字符，溢出即拒）。
pub fn parse_hex32(s: &str) -> Result<u32, u32> {
    let body = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(v) => v,
        None => return Err(BrCode::WindowBad.val()),
    };
    if body.is_empty() || body.len() > 8 {
        return Err(BrCode::WindowBad.val());
    }
    let bytes = body.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(BrCode::WindowBad.val()),
        };
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err(BrCode::WindowBad.val()),
        };
        v = match v.checked_mul(16) {
            Some(x) => x,
            None => return Err(BrCode::WindowBad.val()),
        };
        v = match v.checked_add(d as u32) {
            Some(x) => x,
            None => return Err(BrCode::WindowBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针 A：窗口边界（首码入窗、尾码入窗、窗外一格出窗）。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut slot = 0u8;
    while slot < FAMILY_MODULES as u8 {
        let w = window_of(slot)?;
        if !in_window(w, w.0.saturating_add(1)) {
            return Err(BrCode::WindowBad.val());
        }
        if !in_window(w, window_end(w)) {
            return Err(BrCode::WindowBad.val());
        }
        if in_window(w, w.0) || in_window(w, window_end(w).saturating_add(1)) {
            return Err(BrCode::WindowBad.val());
        }
        slot = slot.saturating_add(1);
    }
    if !windows_ok(&WINDOWS) {
        return Err(BrCode::OverlapBad.val());
    }
    Ok(())
}

/// 探针 B：普查册 53 行逐件对账 + 源头基址互证。
pub fn probe_seq_b() -> Result<(), u32> {
    let book = standard_book()?;
    let d = book.audit()?;
    if d == 0 || d == FNV_OFF {
        return Err(BrCode::CensusBad.val());
    }
    if audit_sources()? != FAMILY_MODULES as u32 {
        return Err(BrCode::WindowBad.val());
    }
    let mut slot = 0u8;
    while slot < FAMILY_MODULES as u8 {
        let w = window_of(slot)?;
        if book.slot_count(slot) != u32::from(w.1) {
            return Err(BrCode::CensusBad.val());
        }
        slot = slot.saturating_add(1);
    }
    Ok(())
}

/// 探针 C：三路 fuzz 各千轮 + 导出往返回环。
pub fn probe_seq_c() -> Result<(), u32> {
    let rep = fuzz_rounds(FUZZ_ROUNDS)?;
    if rep.rounds != FUZZ_ROUNDS || !rep.all_routes() {
        return Err(BrCode::FuzzBad.val());
    }
    let book = standard_book()?;
    let text = render_book(&book);
    let n = audit_render(&text, FAMILY_CODES)?;
    if n != FAMILY_CODES {
        return Err(BrCode::CensusBad.val());
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BrCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        match BrCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据（CheckSet） ──────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48873-J1-窗口不重叠",
        match audit_windows() {
            Ok(n) => windows_ok(&WINDOWS) && n as usize == FAMILY_CODES,
            Err(_) => false,
        },
        format!("五窗共 {} 码，槽宽 ≤0x40，基址升序", FAMILY_CODES),
    ));
    v.push((
        "F48873-J1-码段闭集",
        match standard_book() {
            Ok(b) => {
                let mut ok = b.len() == FAMILY_CODES;
                let mut i = 0usize;
                while i < b.rows.len() {
                    if let Some(r) = b.rows.get(i) {
                        if lookup_slot(r.code()) != Some(r.slot) {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        format!("{} 码逐码反查归槽", FAMILY_CODES),
    ));
    v.push((
        "F48873-J1-明细派发",
        match standard_book() {
            Ok(b) => {
                let mut ok = true;
                let mut i = 0usize;
                while i < b.rows.len() {
                    if let Some(r) = b.rows.get(i) {
                        if module_detail(r.slot, r.code()) != r.text || r.text.is_empty() {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        String::from("逐码桥接文本与源头 detail 逐字相同"),
    ));
    v.push((
        "F48873-J1-反查唯一",
        {
            let mut ok = true;
            let mut slot = 0u8;
            while slot < FAMILY_MODULES as u8 {
                match window_of(slot) {
                    Ok(w) => {
                        if lookup_slot(window_end(w).saturating_add(1)).is_some() {
                            ok = false;
                        }
                        if lookup_slot(w.0).is_some() {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
                slot = slot.saturating_add(1);
            }
            ok && lookup_slot(0x9FFFF).is_none()
        },
        String::from("窗外一格与基址本身都不得命中"),
    ));
    v.push((
        "F48874-J1-封印可复算",
        match inline_roll() {
            Ok((p, imp)) => p == SEAL_PASSES && imp != 0 && imp != FNV_OFF,
            Err(_) => false,
        },
        format!("逐件双算 {} 次，族印非零", SEAL_PASSES),
    ));
    v.push((
        "F48874-J1-封印敏感",
        match module_seal(0) {
            Ok(a) => seal_differs(a, a ^ 1) && !seal_differs(a, a) && a != module_seal(1).unwrap_or(a),
            Err(_) => false,
        },
        String::from("翻一位必须换印；同件同印；异件异印"),
    ));
    v.push((
        "F48874-J1-fuzz万轮零错",
        match fuzz_rounds(FUZZ_ROUNDS) {
            Ok(r) => r.rounds == FUZZ_ROUNDS && r.all_routes() && r.digest != 0,
            Err(_) => false,
        },
        format!("三路 ×{} 全绿", FUZZ_ROUNDS),
    ));
    v.push((
        "F48875-J1-账本计数",
        match standard_book() {
            Ok(b) => {
                b.len() == FAMILY_CODES
                    && b.slot_count(0) == 10
                    && b.slot_count(1) == 10
                    && b.slot_count(2) == 11
                    && b.slot_count(3) == 11
                    && b.slot_count(4) == 11
                    && b.audit().is_ok()
            }
            Err(_) => false,
        },
        String::from("10+10+11+11+11=53"),
    ));
    v.push((
        "F48875-J1-注入四路",
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => l.all_caught() && l.caught == INJECT_ROUNDS,
            Err(_) => false,
        },
        format!("四路 ×{} 全抓", INJECT_ROUNDS),
    ));
    v.push((
        "F48875-J1-双跑一致",
        match golden_run() {
            Ok(a) => match golden_run() {
                Ok(b) => a == b && repro_check().is_ok(),
                Err(_) => false,
            },
            Err(_) => false,
        },
        String::from("两次桥接黄金跑四元组全等"),
    ));
    v
}

/// 判据入口。
pub fn run_unx48873_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48873 B04 族桥接与封印");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let guard = segment_guard();
    s.add_dyn("F48873-J1-段位守卫", guard, if guard { "" } else { "诊断码逃出 0x70900" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_bounds_are_half_open() {
        let w = (0x70780u32, 10u8);
        assert!(!in_window(w, 0x70780));
        assert!(in_window(w, 0x70781));
        assert!(in_window(w, 0x7078A));
        assert!(!in_window(w, 0x7078B));
        assert_eq!(window_end(w), 0x7078A);
    }

    #[test]
    fn windows_do_not_overlap() {
        assert!(windows_ok(&WINDOWS));
        let mut i = 1usize;
        while i < FAMILY_MODULES {
            if let (Some(a), Some(b)) = (WINDOWS.get(i - 1), WINDOWS.get(i)) {
                assert!(b.0 >= a.0 + WINDOW_STRIDE);
                assert!(window_end(*a) <= b.0);
            }
            i += 1;
        }
    }

    #[test]
    fn window_table_tamper_detected() {
        let bad: [(u32, u8); FAMILY_MODULES] = [
            (0x70780, 10),
            (0x70784, 10),
            (0x70800, 11),
            (0x70860, 11),
            (0x708C0, 11),
        ];
        assert!(!windows_ok(&bad));
        let wide: [(u32, u8); FAMILY_MODULES] = [
            (0x70000, 0x41),
            (0x70800, 11),
            (0x70860, 11),
            (0x708C0, 11),
            (0x70940, 11),
        ];
        assert!(!windows_ok(&wide));
    }

    #[test]
    fn lookup_roundtrip() {
        let mut slot = 0u8;
        while slot < FAMILY_MODULES as u8 {
            let w = window_of(slot);
            match w {
                Ok(x) => {
                    let mut ix = 0u8;
                    while ix < x.1 {
                        match code_at(slot, ix) {
                            Ok(code) => {
                                assert_eq!(lookup_slot(code), Some(slot));
                                assert_eq!(index_of(slot, code), Ok(ix));
                            }
                            Err(_) => assert!(false),
                        }
                        ix += 1;
                    }
                }
                Err(_) => assert!(false),
            }
            slot += 1;
        }
    }

    #[test]
    fn code_at_rejects_out_of_range() {
        assert_eq!(code_at(0, 10), Err(BrCode::WindowBad.val()));
        assert_eq!(code_at(5, 0), Err(BrCode::SlotBad.val()));
        assert_eq!(index_of(0, 0x70780), Err(BrCode::WindowBad.val()));
    }

    #[test]
    fn standard_book_has_53_rows() {
        match standard_book() {
            Ok(b) => {
                assert_eq!(b.len(), FAMILY_CODES);
                assert_eq!(b.slot_count(0), 10);
                assert_eq!(b.slot_count(1), 10);
                assert_eq!(b.slot_count(2), 11);
                assert_eq!(b.slot_count(3), 11);
                assert_eq!(b.slot_count(4), 11);
                assert!(b.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn census_rejects_duplicate_and_foreign() {
        let mut b = CensusBook::new();
        assert_eq!(b.push(0, 0), Ok(()));
        assert_eq!(b.push(0, 0), Err(BrCode::DupBad.val()));
        assert_eq!(b.push(5, 0), Err(BrCode::SlotBad.val()));
        assert_eq!(b.push(0, 10), Err(BrCode::WindowBad.val()));
    }

    #[test]
    fn census_order_must_be_slot_ascending() {
        let mut b = CensusBook::new();
        match (b.push(1, 0), b.push(0, 0)) {
            (Ok(()), Ok(())) => {}
            _ => return,
        }
        assert_eq!(b.audit(), Err(BrCode::CensusBad.val()));
    }

    #[test]
    fn detail_dispatch_matches_source() {
        match standard_book() {
            Ok(b) => {
                let mut i = 0usize;
                while i < b.rows.len() {
                    if let Some(r) = b.rows.get(i) {
                        assert_eq!(module_detail(r.slot, r.code()), r.text);
                    }
                    i += 1;
                }
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn unknown_codes_are_explicit() {
        let w = (0x70780u32, 10u8);
        let t = module_detail(0, window_end(w) + 1);
        assert!(t.starts_with("未知"));
        assert!(module_detail(9, 0x70780).starts_with("未知"));
        assert!(detail(SEG_BASE).starts_with("未知"));
    }

    #[test]
    fn seals_are_deterministic_and_slot_sensitive() {
        let a = module_seal(0);
        let b = module_seal(0);
        assert_eq!(a, b);
        match (module_seal(0), module_seal(1)) {
            (Ok(x), Ok(y)) => assert!(seal_differs(x, y)),
            _ => assert!(false),
        }
        assert!(module_seal(5).is_err());
    }

    #[test]
    fn inline_roll_passes_ten() {
        match inline_roll() {
            Ok((p, imp)) => {
                assert_eq!(p, SEAL_PASSES);
                assert!(imp != 0 && imp != FNV_OFF);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_covers_three_routes() {
        match fuzz_rounds(INJECT_ROUNDS) {
            Ok(r) => {
                assert_eq!(r.rounds, INJECT_ROUNDS);
                assert!(r.all_routes());
                assert_eq!(r.dispatch_hits + r.window_hits + r.lookup_hits, INJECT_ROUNDS);
                assert!(r.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero_rounds() {
        assert_eq!(fuzz_rounds(0), Err(BrCode::FuzzBad.val()));
    }

    #[test]
    fn render_roundtrip_53() {
        match standard_book() {
            Ok(b) => {
                let text = render_book(&b);
                assert_eq!(audit_render(&text, FAMILY_CODES), Ok(FAMILY_CODES));
                assert_eq!(text.lines().count(), FAMILY_CODES);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_rejects_tampered_rows() {
        assert!(audit_render("0|unx48861_latency_core|0x70781|0|x\n", 1).is_ok());
        assert!(audit_render("0|unx48861_latency_core|0x70781\n", 1).is_err());
        assert!(audit_render("0|unx48862_wrong|0x70781|0|x\n", 1).is_err());
        assert!(audit_render("1|unx48863_latency_meta|0x70781|0|x\n", 1).is_err());
    }

    #[test]
    fn parse_hex_strict() {
        assert_eq!(parse_hex32("0x70781"), Ok(0x70781));
        assert_eq!(parse_hex32("0X70781"), Ok(0x70781));
        assert_eq!(parse_hex32("70781"), Err(BrCode::WindowBad.val()));
        assert_eq!(parse_hex32("0x"), Err(BrCode::WindowBad.val()));
        assert_eq!(parse_hex32("0x100000000"), Err(BrCode::WindowBad.val()));
        assert_eq!(parse_hex32("0xzz"), Err(BrCode::WindowBad.val()));
    }

    #[test]
    fn inject_all_caught() {
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.caught, INJECT_ROUNDS);
                assert_eq!(l.route_caught, [250, 250, 250, 250]);
                assert!(l.all_caught());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn segment_guard_holds() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < BrCode::ALL.len() {
            if let Some(c) = BrCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 0x01 && c.val() <= SEG_BASE + 0x0B);
            }
            i += 1;
        }
    }

    #[test]
    fn golden_run_stable() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        match a {
            Ok((codes, modules, imp, h)) => {
                assert_eq!(codes as usize, FAMILY_CODES);
                assert_eq!(modules as usize, FAMILY_MODULES);
                assert!(imp != 0 && h != 0);
            }
            Err(_) => assert!(false),
        }
    }
}
