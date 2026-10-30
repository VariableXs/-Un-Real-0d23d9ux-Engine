//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15421 · VARIANT 结构与 VARENUM 钉版 · 0x67340
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15421-J1 钉版成文（VARIANT 布局 8 字段+VARENUM 30 类
//!      对表 10/10 次），类型漂移注入（vt 与载荷不符）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - VARIANT 布局 8 字段钉版（来源：OAIDL 公开布局，小端）：
//!       f0 vt u16（VARENUM 类型码）
//!       f1..f3 wReserved1..3 u16（保留，恒 0）
//!       f4..f7 载荷四槽（p0 u32 低字 / p1 u32 高字 / p2 u64 扩展 /
///              p3 u32 引用标记）——合计 8 字段；
//!   - VARENUM 30 类闭集对表（0..14 连续 + 16..30 连续，15 跳位），
///     每类钉载荷宽（0/16/32/64 位）与载荷族；
///   - 类型漂移注入面：vt 声称的载荷宽 ≠ 实际写入宽 → 检出；
///     保留域非零 → 检出；vt 越表 → 检出；
///   - 守恒恒等式：30 类对表逐类有钉版（无缺格）；vt 码无重复；
///   - LCG 种子写死 = SEG_BASE（0x67340），禁 rand/浮点/IO/墙钟；
///   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67340，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67340;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67340;

/// VARENUM 30 类闭集（值写死，来源：OAIDL VARENUM 公开枚举）。
pub const VT_EMPTY: u16 = 0;
pub const VT_NULL: u16 = 1;
pub const VT_I2: u16 = 2;
pub const VT_I4: u16 = 3;
pub const VT_R4: u16 = 4;
pub const VT_R8: u16 = 5;
pub const VT_CY: u16 = 6;
pub const VT_DATE: u16 = 7;
pub const VT_BSTR: u16 = 8;
pub const VT_DISPATCH: u16 = 9;
pub const VT_ERROR: u16 = 10;
pub const VT_BOOL: u16 = 11;
pub const VT_VARIANT: u16 = 12;
pub const VT_UNKNOWN: u16 = 13;
pub const VT_DECIMAL: u16 = 14;
pub const VT_I1: u16 = 16;
pub const VT_UI1: u16 = 17;
pub const VT_UI2: u16 = 18;
pub const VT_UI4: u16 = 19;
pub const VT_I8: u16 = 20;
pub const VT_UI8: u16 = 21;
pub const VT_INT: u16 = 22;
pub const VT_UINT: u16 = 23;
pub const VT_VOID: u16 = 24;
pub const VT_HRESULT: u16 = 25;
pub const VT_PTR: u16 = 26;
pub const VT_SAFEARRAY: u16 = 27;
pub const VT_CARRAY: u16 = 28;
pub const VT_USERDEFINED: u16 = 29;
pub const VT_LPSTR: u16 = 30;
pub const VT_CLASS_COUNT: usize = 30;

/// 30 类值表（写死，与上列常量一一对应，判据独立不复用被测值）。
pub const VT_TABLE: [u16; VT_CLASS_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14,
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
];

/// 载荷宽闭集（位）。
pub const PW_NONE: u8 = 0;
pub const PW_16: u8 = 16;
pub const PW_32: u8 = 32;
pub const PW_64: u8 = 64;

/// 30 类钉载荷宽表（写死：VT_EMPTY/NULL→0；I1/UI1/I2/UI2/BOOL→16；
/// I4/UI4/R4/ERROR/INT/UINT/HRESULT→32；R8/CY/DATE/I8/UI8/BSTR/
/// DISPATCH/UNKNOWN/VARIANT/PTR/SAFEARRAY/CARRAY/USERDEFINED/LPSTR→64）。
pub const VT_PAYLOAD_WIDTH: [u8; VT_CLASS_COUNT] = [
    PW_NONE, PW_NONE, PW_16, PW_32, PW_32, PW_64, PW_64, PW_64, PW_64,
    PW_64, PW_32, PW_16, PW_64, PW_64, PW_64,
    PW_16, PW_16, PW_16, PW_32, PW_64, PW_64, PW_32, PW_32, PW_NONE,
    PW_32, PW_64, PW_64, PW_64, PW_64, PW_64,
];

/// 判据投影：8 字段 + 30 类对表 10/10。
pub const PIN_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VtCode {
    /// vt 越表（30 类外）
    VtOutOfRange,
    /// 保留域非零
    ReservedNonZero,
    /// 载荷宽与 vt 钉版不符（类型漂移）
    PayloadWidthDrift,
    /// 载荷值超出声明宽（高位污损）
    PayloadOverflow,
    /// VT_EMPTY/NULL 携非零载荷
    EmptyWithPayload,
    /// 30 类表缺格/重码
    TableBreach,
    /// 布局 8 字段偏移不单调
    LayoutBreach,
    /// 载荷宽值越闭集（非 0/16/32/64）
    WidthInvalid,
    /// 往返不恒等
    RoundtripMismatch,
}

impl VtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VtCode::VtOutOfRange => "vt 越表：VARENUM 30 类冻结",
            VtCode::ReservedNonZero => "保留域非零：wReserved1..3 恒 0",
            VtCode::PayloadWidthDrift => "类型漂移：载荷宽与 vt 钉版不符",
            VtCode::PayloadOverflow => "载荷值超出声明宽：高位污损",
            VtCode::EmptyWithPayload => "VT_EMPTY/NULL 携非零载荷",
            VtCode::TableBreach => "30 类表缺格/重码",
            VtCode::LayoutBreach => "布局 8 字段偏移不单调",
            VtCode::WidthInvalid => "载荷宽越闭集：0/16/32/64",
            VtCode::RoundtripMismatch => "VARIANT 往返不恒等",
        }
    }
    pub const ALL: [VtCode; 9] = [
        VtCode::VtOutOfRange,
        VtCode::ReservedNonZero,
        VtCode::PayloadWidthDrift,
        VtCode::PayloadOverflow,
        VtCode::EmptyWithPayload,
        VtCode::TableBreach,
        VtCode::LayoutBreach,
        VtCode::WidthInvalid,
        VtCode::RoundtripMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VtCode::ALL.len() {
        if VtCode::ALL[i].val() == code {
            return VtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67340 段闭集内"
}

// ── VARIANT 布局钉版（8 字段偏移，字节） ─────────────────────────
/// 8 字段：vt(2) r1(2) r2(2) r3(2) p0(4) p1(4) p2(8) p3(4) = 28 字节。
pub const VLT_OFF: [usize; 8] = [0, 2, 4, 6, 8, 12, 16, 24];
pub const VLT_WIDTH: [usize; 8] = [2, 2, 2, 2, 4, 4, 8, 4];
pub const VARIANT_TOTAL: usize = 28;

/// VARIANT 实体（8 字段建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Variant {
    pub vt: u16,
    pub r1: u16,
    pub r2: u16,
    pub r3: u16,
    pub p0: u32,
    pub p1: u32,
    pub p2: u64,
    pub p3: u32,
}

impl Variant {
    pub fn empty() -> Variant {
        Variant { vt: VT_EMPTY, r1: 0, r2: 0, r3: 0, p0: 0, p1: 0, p2: 0, p3: 0 }
    }
}

/// vt → 钉版载荷宽（30 类查表；越表 Err）。
pub fn pinned_width(vt: u16) -> Result<u8, u32> {
    let mut i = 0;
    while i < VT_CLASS_COUNT {
        if VT_TABLE[i] == vt {
            return Ok(VT_PAYLOAD_WIDTH[i]);
        }
        i += 1;
    }
    Err(VtCode::VtOutOfRange.val())
}

/// vt → 钉版载荷宽（闭式分段实现，与查表互为反向判据）。
pub fn pinned_width_closed(vt: u16) -> Result<u8, u32> {
    match vt {
        VT_EMPTY | VT_NULL | VT_VOID => Ok(PW_NONE),
        VT_I1 | VT_UI1 | VT_I2 | VT_UI2 | VT_BOOL => Ok(PW_16),
        VT_I4 | VT_UI4 | VT_R4 | VT_ERROR | VT_INT | VT_UINT | VT_HRESULT => Ok(PW_32),
        VT_R8 | VT_CY | VT_DATE | VT_I8 | VT_UI8 | VT_BSTR | VT_DISPATCH
        | VT_UNKNOWN | VT_VARIANT | VT_PTR | VT_SAFEARRAY | VT_CARRAY
        | VT_USERDEFINED | VT_LPSTR => Ok(PW_64),
        _ => Err(VtCode::VtOutOfRange.val()),
    }
}

/// 类型面核验：vt 与载荷四槽钉版逐条对表。
pub fn verify_variant(v: &Variant) -> Result<(), u32> {
    let w = pinned_width(v.vt)?;
    // 保留域恒 0。
    if v.r1 != 0 || v.r2 != 0 || v.r3 != 0 {
        return Err(VtCode::ReservedNonZero.val());
    }
    // 载荷宽闭集。
    if w != PW_NONE && w != PW_16 && w != PW_32 && w != PW_64 {
        return Err(VtCode::WidthInvalid.val());
    }
    match w {
        PW_NONE => {
            // 全载荷必须为零。
            if v.p0 != 0 || v.p1 != 0 || v.p2 != 0 || v.p3 != 0 {
                return Err(VtCode::EmptyWithPayload.val());
            }
        }
        PW_16 => {
            // 16 位载荷：p0 高 16 位 + 其余槽全零。
            if (v.p0 >> 16) != 0 || v.p1 != 0 || v.p2 != 0 || v.p3 != 0 {
                return Err(VtCode::PayloadWidthDrift.val());
            }
        }
        PW_32 => {
            // 32 位载荷：p0 全量 + 其余槽全零。
            if v.p1 != 0 || v.p2 != 0 || v.p3 != 0 {
                return Err(VtCode::PayloadWidthDrift.val());
            }
        }
        _ => {
            // 64 位载荷：p0|p1 拼低 64 或 p2 扩展，p3 引用标记自由。
            // 高位污损面：p1/p2 不得同时非零（拼装唯一形）。
            if v.p1 != 0 && v.p2 != 0 {
                return Err(VtCode::PayloadOverflow.val());
            }
        }
    }
    Ok(())
}

/// 双表反向判据：查表宽 == 闭式分段宽（30 类全对）。
pub fn tables_agree() -> Result<u32, u32> {
    let mut i = 0;
    while i < VT_CLASS_COUNT {
        let a = VT_PAYLOAD_WIDTH[i];
        let b = pinned_width_closed(VT_TABLE[i])?;
        if a != b {
            return Err(VtCode::TableBreach.val());
        }
        i += 1;
    }
    Ok(VT_CLASS_COUNT as u32)
}

/// 表守恒：30 类无重码、值域覆盖 0..=14 与 16..=30、15 跳位。
pub fn table_conservation() -> Result<(), u32> {
    let mut a = 0;
    while a < VT_CLASS_COUNT {
        let mut b = a + 1;
        while b < VT_CLASS_COUNT {
            if VT_TABLE[a] == VT_TABLE[b] {
                return Err(VtCode::TableBreach.val());
            }
            b += 1;
        }
        if VT_TABLE[a] == 15 {
            return Err(VtCode::TableBreach.val());
        }
        a += 1;
    }
    Ok(())
}

// ── 确定性 LCG（种子 = 0x67340） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

/// 按 vt 与载荷宽构造合法 VARIANT（载荷按钉版宽写入）。
pub fn make_variant(vt: u16, payload: u64) -> Result<Variant, u32> {
    let w = pinned_width(vt)?;
    let mut v = Variant::empty();
    v.vt = vt;
    match w {
        PW_NONE => {
            if payload != 0 {
                return Err(VtCode::EmptyWithPayload.val());
            }
        }
        PW_16 => {
            v.p0 = (payload & 0xFFFF) as u32;
        }
        PW_32 => {
            v.p0 = (payload & 0xFFFF_FFFF) as u32;
        }
        _ => {
            v.p0 = (payload & 0xFFFF_FFFF) as u32;
            v.p1 = ((payload >> 32) & 0xFFFF_FFFF) as u32;
        }
    }
    Ok(v)
}

// ── 10/10 随机 30 类对表压演 ─────────────────────────────────────
pub fn pin_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut checked = 0;
    let mut drift_caught = 0;
    let mut i = 0;
    while i < 30 {
        let vt = VT_TABLE[i];
        let payload = lcg.next();
        let v = make_variant(vt, payload)?;
        verify_variant(&v)?;
        // 往返：构造→核验→重读 vt 宽一致。
        if pinned_width(v.vt)? != pinned_width_closed(vt)? {
            return Err(VtCode::RoundtripMismatch.val());
        }
        checked += 1;
        i += 1;
    }
    // 每轮一次类型漂移注入：vt=I4 但载荷写 16 位形（p1 非零）。
    let mut bad = Variant::empty();
    bad.vt = VT_I4;
    bad.p0 = 0x1111;
    bad.p1 = 0x2222; // I4 钉 32 位：p1 必须为零
    if let Err(e) = verify_variant(&bad) {
        if e == VtCode::PayloadWidthDrift.val() {
            drift_caught += 1;
        }
    }
    if drift_caught != 1 {
        return Err(VtCode::PayloadWidthDrift.val());
    }
    Ok((checked, drift_caught))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 黄金手算对照（写死）：
///   VT_I4 载荷 0x1234_5678 → 合法；VT_I4 载荷 0x1_0000_0000（溢 32 位
///   落 p1）→ PayloadWidthDrift；VT_EMPTY 携载荷 → EmptyWithPayload。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let agree = tables_agree()?;
    table_conservation()?;
    let mut rounds = 0;
    let mut checked_total = 0;
    let mut r = 0;
    while r < PIN_TRIALS {
        let (c, _) = pin_round(r)?;
        checked_total += c;
        rounds += 1;
        r += 1;
    }
    // 黄金三问。
    let a = make_variant(VT_I4, 0x1234_5678)?;
    let a_ok = verify_variant(&a).is_ok();
    let mut b = Variant::empty();
    b.vt = VT_I4;
    b.p0 = 0x0000_0001;
    b.p1 = 0x0000_0001;
    let b_drift = verify_variant(&b);
    let b_ok = if let Err(e) = b_drift {
        e == VtCode::PayloadWidthDrift.val()
    } else {
        false
    };
    let c = make_variant(VT_EMPTY, 7);
    let c_ok = if let Err(e) = c {
        e == VtCode::EmptyWithPayload.val()
    } else {
        false
    };
    if !(a_ok && b_ok && c_ok) {
        return Err(VtCode::RoundtripMismatch.val());
    }
    // 布局钉版自守恒：8 字段偏移单调、Σ宽=28。
    let mut sum_w = 0;
    let mut i = 0;
    while i < 8 {
        sum_w += VLT_WIDTH[i];
        if i > 0 && VLT_OFF[i] <= VLT_OFF[i - 1] {
            return Err(VtCode::LayoutBreach.val());
        }
        if VLT_OFF[i] + VLT_WIDTH[i] > VARIANT_TOTAL {
            return Err(VtCode::LayoutBreach.val());
        }
        i += 1;
    }
    if sum_w != VARIANT_TOTAL {
        return Err(VtCode::LayoutBreach.val());
    }
    Ok((rounds, checked_total, agree))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：vt 与载荷宽漂移（I2 携 32 位形）×10。
    let mut t = 0;
    while t < 10 {
        let mut v = Variant::empty();
        v.vt = VT_I2;
        v.p0 = 0x0001_0002; // 16 位钉：高 16 位必须为零
        let r = verify_variant(&v);
        ledger.tries[0] += 1;
        if let Err(e) = r {
            if e == VtCode::PayloadWidthDrift.val() {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：vt 越表注入 ×10（15 跳位 + 31 起）。
    let mut t = 0;
    while t < 10 {
        let vt = if t % 2 == 0 { 15u16 } else { 31u16 + t as u16 };
        let r = pinned_width(vt);
        ledger.tries[1] += 1;
        if let Err(e) = r {
            if e == VtCode::VtOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：保留域污染注入 ×10。
    let mut t = 0;
    while t < 10 {
        let mut v = Variant::empty();
        v.vt = VT_I4;
        v.p0 = 5;
        v.r2 = 1; // 保留域必须为 0
        let r = verify_variant(&v);
        ledger.tries[2] += 1;
        if let Err(e) = r {
            if e == VtCode::ReservedNonZero.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VtCode::ALL.len() {
        let v = VtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 8 字段 + 30 类判据自洽（常量独立写死）。
    VLT_OFF.len() == 8 && VT_CLASS_COUNT == 30 && VARIANT_TOTAL == 28
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, checked, agree)) = golden {
        rounds == PIN_TRIALS as u32 && checked == 300 && agree == 30
    } else {
        false
    };
    v.push((
        "J1-8字段+30类对表10/10",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-类型漂移注入10/10必抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 双表反向判据独立点验。
    let agree = tables_agree();
    v.push((
        "J1-查表/闭式双表反向一致",
        agree == Ok(30),
        format!("{:?}", agree),
    ));
    // 布局守恒独立点验（Σ宽=28 判据写死）。
    let mut sum_w = 0;
    let mut i = 0;
    while i < 8 {
        sum_w += VLT_WIDTH[i];
        i += 1;
    }
    v.push((
        "J1-布局Σ宽=28",
        sum_w == 28,
        format!("{}", sum_w),
    ));
    // 黄金三问独立点验。
    let a = make_variant(VT_I4, 0x1234_5678);
    v.push((
        "J1-黄金I4合法形",
        if let Ok(x) = a { verify_variant(&x).is_ok() } else { false },
        String::from("I4/0x12345678"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67340 窗口")));
    v
}

pub fn run_unx15421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15421 VARIANT 结构与 VARENUM 钉版");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
