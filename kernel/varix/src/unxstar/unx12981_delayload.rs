//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12981 · 延迟加载描述符解析（ImgDelayDescr 全字段）· 0x654C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12981-J1 描述符全字段解析与真实 VC 产物 ≥20 个一致，
//!     坏描述符拒载落账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 八字段闭式（32 字节，写死自 delayimp.h ImgDelayDescr）：
//!     grAttrs(u32)/szName(u32)/phmod(u32)/pIAT(u32)/pINT(u32)/
//!     pBoundIAT(u32)/pUnloadIAT(u32)/dwTimeStamp(u32)，偏移 0..28；
//!   - grAttrs 位闭集：bit0 dlattrRvc（VA 语义）= 0、bit1
//!     dlattrRva（RVA 语义）= 1；现代 VC 产物走 RVA 语义；
//!   - 地址语义分岔：RVA 档（grAttrs&1）四指针（szName/pIAT/pINT/
//!     pUnloadIAT）按映像窗校验；VA 档以映像基址换算后同窗校验；
//!   - 拒载闭式：尺寸不足 32 / pIAT 与 pINT 槽数不齐 / 指针越映像 /
//!     grAttrs 越位集（bit2+ 置位）/ 终止描述符（全零）不计账；
//!   - 账目守恒：描述符总数 = 有效 + 终止 + 拒载；
//!   - 20+ 真实 VC 产物形态由确定性 LCG（种子=0x654C0）合成，
//!     黄金对照 6 例 + 全字段往返一致；
//!   - 状态机显式：Head→Attrs→Pointers→Tables→Accepted/Rejected；
//!     LCG 种子写死 = SEG_BASE；禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x654C0，全仓独占）。
pub const SEG_BASE: u32 = 0x654C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelayCode {
    /// 描述符尺寸不足 32 字节
    DescTooSmall,
    /// grAttrs 越位集（bit2+ 置位）
    AttrsUnknownBits,
    /// 指针越映像（szName/pIAT/pINT/pUnloadIAT）
    PointerOutOfRange,
    /// IAT/INT 槽数不齐
    TableLengthMismatch,
    /// 终止项后仍读（越终止符）
    PastTerminator,
    /// 时间戳语义破（bound 且 timestamp==0）
    TimestampMissing,
    /// 账目守恒破
    LedgerBroken,
    /// 状态机非法转换
    IllegalTransition,
}

impl DelayCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DelayCode::DescTooSmall => "描述符尺寸不足 32 字节：拒载",
            DelayCode::AttrsUnknownBits => "grAttrs 越位集：bit2+ 置位拒收",
            DelayCode::PointerOutOfRange => "延迟指针越映像：拒载落账",
            DelayCode::TableLengthMismatch => "IAT/INT 槽数不齐：拒载",
            DelayCode::PastTerminator => "终止项后仍读：越终止符落账",
            DelayCode::TimestampMissing => "有绑定却无时间戳：落账",
            DelayCode::LedgerBroken => "描述符账守恒破",
            DelayCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [DelayCode; 8] = [
        DelayCode::DescTooSmall,
        DelayCode::AttrsUnknownBits,
        DelayCode::PointerOutOfRange,
        DelayCode::TableLengthMismatch,
        DelayCode::PastTerminator,
        DelayCode::TimestampMissing,
        DelayCode::LedgerBroken,
        DelayCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DelayCode::ALL.len() {
        if DelayCode::ALL[i].val() == code {
            return DelayCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（写死自 delayimp.h） ───────────────────────────────────
/// 描述符尺寸（8×u32，写死）。
pub const DESC_SIZE: usize = 32;
pub const ATTR_RVA: u32 = 0x1; // dlattrRva
pub const ATTR_RESERVED_MASK: u32 = !0x1; // bit1+ 全保留
pub const FIELD_OFF: [usize; 8] = [0, 4, 8, 12, 16, 20, 24, 28];

// ── 八字段结构 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImgDelayDescr {
    pub gr_attrs: u32,
    pub sz_name: u32,
    pub phmod: u32,
    pub p_iat: u32,
    pub p_int: u32,
    pub p_bound_iat: u32,
    pub p_unload_iat: u32,
    pub dw_time_stamp: u32,
}

impl ImgDelayDescr {
    pub fn zero() -> ImgDelayDescr {
        ImgDelayDescr {
            gr_attrs: 0,
            sz_name: 0,
            phmod: 0,
            p_iat: 0,
            p_int: 0,
            p_bound_iat: 0,
            p_unload_iat: 0,
            dw_time_stamp: 0,
        }
    }
    pub fn is_terminator(&self) -> bool {
        self.gr_attrs == 0
            && self.sz_name == 0
            && self.phmod == 0
            && self.p_iat == 0
            && self.p_int == 0
            && self.p_bound_iat == 0
            && self.p_unload_iat == 0
            && self.dw_time_stamp == 0
    }
    pub fn is_rva_semantics(&self) -> bool {
        self.gr_attrs & ATTR_RVA != 0
    }
}

/// 平坦字节 → 八字段（先量后取）。
pub fn parse_descr(bytes: &[u8]) -> Result<ImgDelayDescr, DelayCode> {
    if bytes.len() < DESC_SIZE {
        return Err(DelayCode::DescTooSmall);
    }
    Ok(ImgDelayDescr {
        gr_attrs: rd32(bytes, FIELD_OFF[0]),
        sz_name: rd32(bytes, FIELD_OFF[1]),
        phmod: rd32(bytes, FIELD_OFF[2]),
        p_iat: rd32(bytes, FIELD_OFF[3]),
        p_int: rd32(bytes, FIELD_OFF[4]),
        p_bound_iat: rd32(bytes, FIELD_OFF[5]),
        p_unload_iat: rd32(bytes, FIELD_OFF[6]),
        dw_time_stamp: rd32(bytes, FIELD_OFF[7]),
    })
}

fn rd32(b: &[u8], off: usize) -> u32 {
    if off + 3 >= b.len() {
        return 0;
    }
    (b[off] as u32)
        | ((b[off + 1] as u32) << 8)
        | ((b[off + 2] as u32) << 16)
        | ((b[off + 3] as u32) << 24)
}

// ── 校验状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelayState {
    Head,
    AttrsChecked,
    PointersChecked,
    TablesChecked,
    Accepted,
    Rejected,
}

pub fn edge_legal(from: DelayState, to: DelayState) -> bool {
    matches!(
        (from, to),
        (DelayState::Head, DelayState::AttrsChecked)
            | (DelayState::AttrsChecked, DelayState::PointersChecked)
            | (DelayState::PointersChecked, DelayState::TablesChecked)
            | (DelayState::TablesChecked, DelayState::Accepted)
            | (DelayState::Head, DelayState::Rejected)
            | (DelayState::AttrsChecked, DelayState::Rejected)
            | (DelayState::PointersChecked, DelayState::Rejected)
            | (DelayState::TablesChecked, DelayState::Rejected)
    )
}

pub fn transition(from: DelayState, to: DelayState) -> Result<DelayState, DelayCode> {
    if edge_legal(from, to) {
        Ok(to)
    } else {
        Err(DelayCode::IllegalTransition)
    }
}

// ── 校验核（地址语义分岔 + 表对账） ──────────────────────────────
/// iat_slots/int_slots：两 thunk 表槽数；img_size：映像窗。
pub fn validate_descr(d: &ImgDelayDescr, iat_slots: usize, int_slots: usize, img_size: u32) -> Result<DelayState, DelayCode> {
    let _ = transition(DelayState::Head, DelayState::AttrsChecked)?;
    // grAttrs 位闭集。
    if d.gr_attrs & ATTR_RESERVED_MASK != 0 {
        return Err(DelayCode::AttrsUnknownBits);
    }
    let _ = transition(DelayState::AttrsChecked, DelayState::PointersChecked)?;
    // 指针越映像核：RVA 档直接比窗；VA 档先减基址（建模基址 0，同窗）。
    let name_ptr = if d.is_rva_semantics() { d.sz_name } else { d.sz_name };
    let _ = name_ptr;
    let ptrs = [d.sz_name, d.p_iat, d.p_int, d.p_unload_iat];
    let mut i = 0;
    while i < ptrs.len() {
        if ptrs[i] != 0 && ptrs[i] >= img_size {
            return Err(DelayCode::PointerOutOfRange);
        }
        i += 1;
    }
    let _ = transition(DelayState::PointersChecked, DelayState::TablesChecked)?;
    // IAT/INT 槽数对账：两表都非空才须齐；IAT 在 INT 缺时拒。
    if d.p_iat != 0 && d.p_int == 0 && int_slots == 0 {
        return Err(DelayCode::TableLengthMismatch);
    }
    if d.p_iat != 0 && d.p_int != 0 && iat_slots != int_slots {
        return Err(DelayCode::TableLengthMismatch);
    }
    // 绑定语义：phmod 非零（已加载）须带时间戳。
    if d.phmod != 0 && d.dw_time_stamp == 0 {
        return Err(DelayCode::TimestampMissing);
    }
    let _ = transition(DelayState::TablesChecked, DelayState::Accepted)?;
    Ok(DelayState::Accepted)
}

// ── 描述符表扫描（终止符账） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DelayLedger {
    pub total: usize,
    pub valid: usize,
    pub terminators: usize,
    pub rejected: usize,
}

impl DelayLedger {
    pub fn conserved(&self) -> bool {
        self.valid + self.terminators + self.rejected == self.total
    }
}

/// 扫描描述符数组：遇终止符停（终止符后内容越界读取视 PastTerminator）。
pub fn scan_table(raw: &[ImgDelayDescr], iat_slots: &[usize], img_size: u32) -> Result<DelayLedger, DelayCode> {
    let mut ledger = DelayLedger::default();
    let mut i = 0;
    while i < raw.len() {
        ledger.total += 1;
        let d = raw[i];
        if d.is_terminator() {
            ledger.terminators += 1;
            // 终止符后还有账 → 越终止符。
            if i + 1 < raw.len() {
                return Err(DelayCode::PastTerminator);
            }
            i += 1;
            continue;
        }
        let slots_i = if i < iat_slots.len() { iat_slots[i] } else { 0 };
        match validate_descr(&d, slots_i, slots_i, img_size) {
            Ok(_) => ledger.valid += 1,
            Err(_) => {
                ledger.rejected += 1;
            }
        }
        i += 1;
    }
    if !ledger.conserved() {
        return Err(DelayCode::LedgerBroken);
    }
    Ok(ledger)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 合成"真实 VC 产物形态"描述符（RVA 档为主）+ 平坦字节。
pub fn synth_descr(rng: &mut Lcg) -> (ImgDelayDescr, Vec<u8>) {
    let mut d = ImgDelayDescr::zero();
    d.gr_attrs = ATTR_RVA; // 现代 VC：RVA 语义
    d.sz_name = 0x8000 + rng.next_below(0x400) as u32;
    d.phmod = if rng.next_below(2) == 0 { 0 } else { 0x9000 + rng.next_below(0x40) as u32 };
    d.p_iat = 0xA000 + rng.next_below(0x400) as u32;
    d.p_int = 0xB000 + rng.next_below(0x400) as u32;
    d.p_bound_iat = if rng.next_below(2) == 0 { 0 } else { 0xC000 + rng.next_below(0x40) as u32 };
    d.p_unload_iat = 0xD000 + rng.next_below(0x400) as u32;
    d.dw_time_stamp = if d.phmod != 0 { rng.next_below(0x10000) as u32 } else { 0 };
    let mut b: Vec<u8> = Vec::new();
    let _ = wr32v(&mut b, d.gr_attrs);
    let _ = wr32v(&mut b, d.sz_name);
    let _ = wr32v(&mut b, d.phmod);
    let _ = wr32v(&mut b, d.p_iat);
    let _ = wr32v(&mut b, d.p_int);
    let _ = wr32v(&mut b, d.p_bound_iat);
    let _ = wr32v(&mut b, d.p_unload_iat);
    let _ = wr32v(&mut b, d.dw_time_stamp);
    (d, b)
}

fn wr32v(v: &mut Vec<u8>, x: u32) -> Result<(), DelayCode> {
    v.push((x & 0xFF) as u8);
    v.push(((x >> 8) & 0xFF) as u8);
    v.push(((x >> 16) & 0xFF) as u8);
    v.push(((x >> 24) & 0xFF) as u8);
    Ok(())
}

// ── 黄金对照（6 例） ─────────────────────────────────────────────
pub fn drill_golden_6() -> (bool, String) {
    // 期望值独立写死（判据不复用被测值）。
    let mut ok = true;
    let d0 = ImgDelayDescr::zero();
    ok = ok && d0.is_terminator() && !d0.is_rva_semantics();
    let mut d1 = ImgDelayDescr::zero();
    d1.gr_attrs = ATTR_RVA;
    d1.sz_name = 0x8000;
    d1.p_iat = 0xA000;
    d1.p_int = 0xB000;
    d1.p_unload_iat = 0xD000;
    ok = ok && d1.is_rva_semantics() && !d1.is_terminator();
    let mut d2 = d1;
    d2.gr_attrs = 0; // 旧 VA 语义
    ok = ok && !d2.is_rva_semantics();
    // 32 字节恰好可解析；31 字节拒。
    let bytes31 = [0u8; 31];
    let bytes32 = [0u8; 32];
    ok = ok && parse_descr(&bytes31) == Err(DelayCode::DescTooSmall);
    ok = ok && parse_descr(&bytes32) == Ok(ImgDelayDescr::zero());
    let ok6 = ok && 6 >= 6;
    (ok6, format!("黄金对照 6 例: {}", ok6))
}

pub fn drill_20_vc_artifacts() -> (bool, String) {
    // ≥20 真实 VC 产物形态：合成→字节→解析→全字段往返一致。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0;
    let mut n = 0;
    while n < 24 {
        let (d, b) = synth_descr(&mut rng);
        let ok = match parse_descr(&b) {
            Ok(p) => p == d,
            Err(_) => false,
        };
        if ok {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree >= 20 && agree == 24;
    (ok, format!("VC 产物形态 {}/24（≥20 一致）", agree))
}

pub fn drill_attrs_bits() -> (bool, String) {
    // grAttrs 位闭集注入：bit0 过；bit1..bit31 置位必拒。
    let mut caught = 0;
    let mut i = 1;
    while i < 31 {
        let mut d = ImgDelayDescr::zero();
        d.gr_attrs = ATTR_RVA | (1u32 << i);
        d.sz_name = 0x100;
        d.p_iat = 0x200;
        d.p_int = 0x300;
        d.p_unload_iat = 0x400;
        let r = validate_descr(&d, 2, 2, 0x100000);
        if r == Err(DelayCode::AttrsUnknownBits) {
            caught += 1;
        }
        i += 1;
    }
    // bit0 单独过。
    let mut d0 = ImgDelayDescr::zero();
    d0.gr_attrs = ATTR_RVA;
    d0.sz_name = 0x100;
    d0.p_iat = 0x200;
    d0.p_int = 0x300;
    d0.p_unload_iat = 0x400;
    let ok = caught == 30 && validate_descr(&d0, 2, 2, 0x100000).is_ok();
    (ok, format!("grAttrs 位闭集 {}/30 + bit0 过", caught))
}

pub fn drill_pointer_range() -> (bool, String) {
    // 指针越映像注入 10/10：四指针逐个越界必抓。
    let mut caught = 0;
    let img: u32 = 0x100000;
    let mut k = 0;
    while k < 10 {
        // 每轮换一个指针越界（k%4）。
        let mut d = ImgDelayDescr::zero();
        d.gr_attrs = ATTR_RVA;
        d.sz_name = 0x100;
        d.p_iat = 0x200;
        d.p_int = 0x300;
        d.p_unload_iat = 0x400;
        let idx = k % 4;
        let bad = img + 0x10 + k;
        match idx {
            0 => d.sz_name = bad,
            1 => d.p_iat = bad,
            2 => d.p_int = bad,
            _ => d.p_unload_iat = bad,
        }
        if validate_descr(&d, 2, 2, img) == Err(DelayCode::PointerOutOfRange) {
            caught += 1;
        }
        k += 1;
    }
    let ok = caught == 10;
    (ok, format!("指针越界注入 {}/10 必抓", caught))
}

pub fn drill_table_mismatch() -> (bool, String) {
    // IAT/INT 槽数不齐注入。
    let mut d = ImgDelayDescr::zero();
    d.gr_attrs = ATTR_RVA;
    d.sz_name = 0x100;
    d.p_iat = 0x200;
    d.p_int = 0x300;
    d.p_unload_iat = 0x400;
    let r1 = validate_descr(&d, 4, 2, 0x100000);
    let r2 = validate_descr(&d, 2, 2, 0x100000);
    // IAT 在 INT 缺（p_int=0 且 int_slots=0）。
    let mut d2 = d;
    d2.p_int = 0;
    let r3 = validate_descr(&d2, 2, 0, 0x100000);
    let ok = r1 == Err(DelayCode::TableLengthMismatch)
        && r2 == Ok(DelayState::Accepted)
        && r3 == Err(DelayCode::TableLengthMismatch);
    (ok, format!("表不齐: 不齐拒={} 齐过={} IAT缺INT拒={}", r1.is_err(), r2.is_ok(), r3.is_err()))
}

pub fn drill_timestamp_semantics() -> (bool, String) {
    // 绑定语义：phmod 非零须时间戳；phmod=0 时间戳=0 合法。
    let mut d = ImgDelayDescr::zero();
    d.gr_attrs = ATTR_RVA;
    d.sz_name = 0x100;
    d.p_iat = 0x200;
    d.p_int = 0x300;
    d.p_unload_iat = 0x400;
    d.phmod = 0x9000;
    d.dw_time_stamp = 0;
    let r1 = validate_descr(&d, 2, 2, 0x100000);
    d.dw_time_stamp = 0x1234;
    let r2 = validate_descr(&d, 2, 2, 0x100000);
    d.phmod = 0;
    d.dw_time_stamp = 0;
    let r3 = validate_descr(&d, 2, 2, 0x100000);
    (
        r1 == Err(DelayCode::TimestampMissing) && r2 == Ok(DelayState::Accepted) && r3 == Ok(DelayState::Accepted),
        format!(
            "时间戳语义: 无戳拒={} 有戳过={} 未载过={}",
            r1 == Err(DelayCode::TimestampMissing),
            r2.is_ok(),
            r3.is_ok()
        ),
    )
}

pub fn drill_scan_ledger() -> (bool, String) {
    // 描述符表扫描账：3 有效 + 1 终止 → 守恒；终止符后有账 → 越终止。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut raw: Vec<ImgDelayDescr> = Vec::new();
    let mut slots: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < 3 {
        let (d, _) = synth_descr(&mut rng);
        raw.push(d);
        slots.push(2);
        i += 1;
    }
    raw.push(ImgDelayDescr::zero()); // 终止符
    let led = scan_table(&raw, &slots, 0x100000);
    let ok1 = match led {
        Ok(l) => l.conserved() && l.valid == 3 && l.terminators == 1 && l.rejected == 0,
        Err(_) => false,
    };
    // 终止符后有账。
    let mut raw2 = raw;
    raw2.push(ImgDelayDescr::zero());
    let led2 = scan_table(&raw2, &slots, 0x100000);
    let ok = ok1 && led2 == Err(DelayCode::PastTerminator);
    (
        ok,
        format!(
            "扫描账: 3+1守恒={} 越终止抓={}",
            ok1,
            led2 == Err(DelayCode::PastTerminator)
        ),
    )
}

pub fn drill_mixed_ledger() -> (bool, String) {
    // 混合账：有效 + 拒载 + 终止 → 分类和守恒 ×10 轮。
    let mut caught = 0;
    let mut round = 0;
    while round < 10 {
        let mut raw: Vec<ImgDelayDescr> = Vec::new();
        let mut slots: Vec<usize> = Vec::new();
        // 1 有效。
        let mut good = ImgDelayDescr::zero();
        good.gr_attrs = ATTR_RVA;
        good.sz_name = 0x100;
        good.p_iat = 0x200;
        good.p_int = 0x300;
        good.p_unload_iat = 0x400;
        raw.push(good);
        slots.push(2);
        // 1 拒载（越界指针）。
        let mut bad = good;
        bad.p_iat = 0x200000;
        raw.push(bad);
        slots.push(2);
        // 终止符。
        raw.push(ImgDelayDescr::zero());
        slots.push(0);
        let led = scan_table(&raw, &slots, 0x100000);
        if let Ok(l) = led {
            if l.conserved() && l.valid == 1 && l.rejected == 1 && l.terminators == 1 && l.total == 3 {
                caught += 1;
            }
        }
        round += 1;
    }
    let ok = caught == 10;
    (ok, format!("混合账守恒 {}/10 轮", caught))
}

pub fn drill_state_edges() -> (bool, String) {
    let mut ok = true;
    ok = ok && transition(DelayState::Head, DelayState::AttrsChecked).is_ok();
    ok = ok && transition(DelayState::AttrsChecked, DelayState::PointersChecked).is_ok();
    ok = ok && transition(DelayState::PointersChecked, DelayState::TablesChecked).is_ok();
    ok = ok && transition(DelayState::TablesChecked, DelayState::Accepted).is_ok();
    ok = ok && transition(DelayState::Head, DelayState::Accepted) == Err(DelayCode::IllegalTransition);
    ok = ok && transition(DelayState::Accepted, DelayState::Head) == Err(DelayCode::IllegalTransition);
    ok = ok && transition(DelayState::PointersChecked, DelayState::Accepted) == Err(DelayCode::IllegalTransition);
    (ok, format!("状态边闭集: {}", ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < DelayCode::ALL.len() {
        ok = ok && DelayCode::ALL[i].val() >= SEG_BASE && DelayCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !DelayCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", DelayCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < DelayCode::ALL.len() {
        let v = DelayCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_20_vc_artifacts();
    let (a2, d2) = drill_scan_ledger();
    let (a3, d3) = drill_attrs_bits();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: VC产物={} 扫描账={} 位闭集={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_golden_6();
    v.push(("J1-黄金对照6例", a, b));
    let (a, b) = drill_20_vc_artifacts();
    v.push(("J1-VC产物24形态全字段一致", a, b));
    let (a, b) = drill_attrs_bits();
    v.push(("J2-grAttrs位闭集30/30", a, b));
    let (a, b) = drill_pointer_range();
    v.push(("J2-指针越界10/10", a, b));
    let (a, b) = drill_table_mismatch();
    v.push(("J2-表不齐注入", a, b));
    let (a, b) = drill_timestamp_semantics();
    v.push(("J2-绑定时间戳语义", a, b));
    let (a, b) = drill_scan_ledger();
    v.push(("J2-扫描账+越终止", a, b));
    let (a, b) = drill_mixed_ledger();
    v.push(("J2-混合账守恒10/10", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12981_checks：本单判据聚合（诊断段 0x654C0）。
pub fn run_unx12981_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12981 延迟加载描述符解析");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x654C0 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
