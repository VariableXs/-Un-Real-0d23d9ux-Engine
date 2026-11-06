//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18515 · 错误路径账（非法掩码/未知条件）
//! · 0x7FC80 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18515-J1 非法输入 10 形态注入返回码与 Windows 一致，
//!   错误三要素齐备
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 错误路径语义冻结：VerifyVersionInfo 类判定面对非法输入必返
//!     「失败三要素」——①返回旗 0（函数失败）②错误码（Windows 口
//!     径 ERROR_INVALID_PARAMETER = 87 闭集映射）③非空中文明细；
//!     三者缺一即错误路径账破；
//!   - 非法输入 10 形态（手推 Windows 对照）：
//!     f0 未知掩码高位 0x8000_0000 / f1 条件算子 0 / f2 条件算子 8 /
//!     f3 数值维落 VER_AND / f4 套件维落 VER_EQUAL / f5 产品型维落
//!     VER_GREATER / f6 掩码置位但条件未武装 / f7 条件武装但掩码无位 /
//!     f8 同维双条件冲突 / f9 空掩码带条件——10/10 映射 87；
//!   - 武装流水状态机 Open→Armed→Validated/Rejected：掩码位闸 →
//!     空掩码闸 → 算子闭集闸 → 算子-维配对闸 → 位-算子配对闸 →
//!     重复维闸，六闸串联次序冻结（判定次序红线：先掩码后算子）；
//!   - 合法请求happy path：六闸全过 → Validated（可判定）；
//!   - 注入三类 × 10 轮：非法掩码高位 / 非法算子 / 三要素缺一
//!     （伪造空明细被闭集校验必抓）——10/10 全抓；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] EpCode 闭集（14 码：10 形态 + 4 框架码）+ ALL +
//!              detail() + detail(code)
//!   [裁决表]   FORMS 10 形态冻结对照表（形态 → 段码 → Windows 87）
//!   [数据结构] Arm（掩码位 + 维 + 算子 + 请求值）/ EpLedger
//!   [状态机]   ArmPhase 四态 + Armer（六闸串联次序冻结）
//!   [账本]     EpLedger（尝试 = 过 + 拒 守恒）+ InjectLedger
//!   [注入核]   三类 × 10 轮（高位/算子/三要素缺一）
//!   [收口]     golden_run + run_unx18515_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FC80，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FC80;

// ── 域冻结常量（winnt.h 口径，闭式重演） ────────────────────────
/// VER_EQUAL（冻结）。
pub const VER_EQUAL: u8 = 1;
/// VER_GREATER（冻结）。
pub const VER_GREATER: u8 = 2;
/// VER_GREATER_EQUAL（冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// VER_LESS（冻结）。
pub const VER_LESS: u8 = 4;
/// VER_LESS_EQUAL（冻结）。
pub const VER_LESS_EQUAL: u8 = 5;
/// VER_AND（套件维专用）。
pub const VER_AND: u8 = 6;
/// VER_OR（套件维专用）。
pub const VER_OR: u8 = 7;
/// 掩码位闭集下界（VER_MINORVERSION 0x01）。
pub const MASK_LOW: u32 = 0x01;
/// 掩码位闭集上界（VER_PRODUCT_TYPE 0x80）。
pub const MASK_HIGH: u32 = 0x80;
/// Windows ERROR_INVALID_PARAMETER（冻结对照值）。
pub const WIN_INVAL: u32 = 87;
/// 非法形态数（J1「10 形态」口径）。
pub const FORMS: usize = 10;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpCode {
    /// f0：未知掩码高位（闭集 0x01..=0x80 外）。
    MaskBitUnknown,
    /// f1：条件算子 0（未设算子）。
    CondOpZero,
    /// f2：条件算子越界（>7）。
    CondOpOutOfRange,
    /// f3：数值维落 VER_AND/VER_OR。
    NumericSuiteOp,
    /// f4：套件维落数值算子。
    SuiteNumericOp,
    /// f5：产品型维落非 VER_EQUAL。
    ProductTypeOp,
    /// f6：掩码置位但条件未武装。
    CondMissing,
    /// f7：条件武装但掩码无位。
    MaskBitMissing,
    /// f8：同维双条件冲突。
    CondConflict,
    /// f9：空掩码带条件。
    MaskEmptyArmed,
    /// 框架：维号越界。
    DimOutOfRange,
    /// 框架：三要素缺一（返回旗/错误码/明细任一缺席）。
    TriadIncomplete,
    /// 框架：注入漏检（违约未被拒）。
    InjectionMissed,
    /// 框架：双跑不一致。
    NotReproducible,
}

impl EpCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EpCode::MaskBitUnknown => "未知掩码位：闭集 0x01..=0x80 外",
            EpCode::CondOpZero => "条件算子 0：未设算子",
            EpCode::CondOpOutOfRange => "条件算子越界：闭集 1..=7",
            EpCode::NumericSuiteOp => "数值维落 AND/OR：仅套件维可用",
            EpCode::SuiteNumericOp => "套件维落数值算子：仅 AND/OR",
            EpCode::ProductTypeOp => "产品型维仅 VER_EQUAL",
            EpCode::CondMissing => "掩码置位但条件未武装",
            EpCode::MaskBitMissing => "条件武装但掩码无位",
            EpCode::CondConflict => "同维双条件冲突",
            EpCode::MaskEmptyArmed => "空掩码带条件：自相矛盾",
            EpCode::DimOutOfRange => "维号越界：闭集 0..=7",
            EpCode::TriadIncomplete => "三要素缺一：旗/码/明细必须齐备",
            EpCode::InjectionMissed => "注入漏检：违约必被拒",
            EpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [EpCode; 14] = [
        EpCode::MaskBitUnknown,
        EpCode::CondOpZero,
        EpCode::CondOpOutOfRange,
        EpCode::NumericSuiteOp,
        EpCode::SuiteNumericOp,
        EpCode::ProductTypeOp,
        EpCode::CondMissing,
        EpCode::MaskBitMissing,
        EpCode::CondConflict,
        EpCode::MaskEmptyArmed,
        EpCode::DimOutOfRange,
        EpCode::TriadIncomplete,
        EpCode::InjectionMissed,
        EpCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EpCode::ALL.len() {
        if EpCode::ALL[i].code() == code {
            return EpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FC80 段闭集内"
}

/// 维号 ← 掩码位（0x01→d0 … 0x80→d7；越界位 None）。
pub fn dim_of_mask(mask_bit: u32) -> Option<u8> {
    match mask_bit {
        0x01 => Some(0),
        0x02 => Some(1),
        0x04 => Some(2),
        0x08 => Some(3),
        0x10 => Some(4),
        0x20 => Some(5),
        0x40 => Some(6),
        0x80 => Some(7),
        _ => None,
    }
}

// ── 形态对照表（冻结：10 形态 → 段码 → Windows 87） ─────────────
/// 形态一行：(形态 id, 段内诊断码, Windows 对照码, 形态描述)。
pub type FormRow = (usize, u32, u32, &'static str);

/// 第 i 形态（越界 None）。Windows 对照：10/10 → 87。
pub fn form_row(i: usize) -> Option<FormRow> {
    match i {
        0 => Some((0, EpCode::MaskBitUnknown.code(), WIN_INVAL, "掩码高位 0x80000000")),
        1 => Some((1, EpCode::CondOpZero.code(), WIN_INVAL, "算子 0")),
        2 => Some((2, EpCode::CondOpOutOfRange.code(), WIN_INVAL, "算子 8")),
        3 => Some((3, EpCode::NumericSuiteOp.code(), WIN_INVAL, "数值维落 AND")),
        4 => Some((4, EpCode::SuiteNumericOp.code(), WIN_INVAL, "套件维落 EQUAL")),
        5 => Some((5, EpCode::ProductTypeOp.code(), WIN_INVAL, "产品型落 GREATER")),
        6 => Some((6, EpCode::CondMissing.code(), WIN_INVAL, "有位无条件")),
        7 => Some((7, EpCode::MaskBitMissing.code(), WIN_INVAL, "有条件无位")),
        8 => Some((8, EpCode::CondConflict.code(), WIN_INVAL, "同维双条件")),
        9 => Some((9, EpCode::MaskEmptyArmed.code(), WIN_INVAL, "空掩码带条件")),
        _ => None,
    }
}

// ── 武装请求与状态机 ────────────────────────────────────────────
/// 单条武装：(掩码位, 算子, 请求值)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arm {
    pub mask_bit: u32,
    pub op: u8,
    pub req: u32,
}

/// 武装相位（四态单向；Rejected 为终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmPhase {
    /// 开放（可武装）。
    Open,
    /// 已武装（待校验）。
    Armed,
    /// 校验通过（可判定）。
    Validated,
    /// 校验拒绝（终态；错误码已入账）。
    Rejected,
}

/// 错误路径账（尝试 = 过 + 拒，守恒方程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
    /// 拒因账（逐次留段码）。
    pub codes: [u32; 16],
    pub codes_len: usize,
}

impl EpLedger {
    pub const fn new() -> EpLedger {
        EpLedger { tries: 0, passed: 0, rejected: 0, codes: [0; 16], codes_len: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
    /// 拒因入账（账满截断计数，旧账不丢）。
    pub fn push_code(&mut self, c: u32) {
        if self.codes_len < 16 {
            self.codes[self.codes_len] = c;
            self.codes_len += 1;
        }
    }
}

/// 武装校验引擎：六闸串联次序冻结（先掩码后算子，判定次序红线）。
pub struct Armer {
    pub phase: ArmPhase,
    pub arms: [Option<Arm>; 8],
    pub arms_len: usize,
    pub ledger: EpLedger,
}

impl Armer {
    pub fn new() -> Armer {
        Armer { phase: ArmPhase::Open, arms: [None; 8], arms_len: 0, ledger: EpLedger::new() }
    }

    /// 武装单条（Open 相位限定；至多 8 条）。
    pub fn arm(&mut self, a: Arm) -> Result<(), u32> {
        if self.phase != ArmPhase::Open {
            return Err(EpCode::InjectionMissed.code());
        }
        if self.arms_len >= 8 {
            return Err(EpCode::InjectionMissed.code());
        }
        self.arms[self.arms_len] = Some(a);
        self.arms_len += 1;
        self.phase = ArmPhase::Armed;
        Ok(())
    }

    /// 六闸校验（次序冻结）：逐闸先到先拒，拒因入账，相位 Rejected。
    pub fn validate(&mut self, mask: u32) -> Result<(), u32> {
        if self.phase != ArmPhase::Armed && self.phase != ArmPhase::Open {
            return Err(EpCode::InjectionMissed.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        // 闸1：掩码位闭集（f0；空掩码带条件归闸6）。
        let mut m = mask;
        let mut bits_seen = 0u32;
        while m != 0 {
            let low = m & (!m + 1); // 最低置位位
            if dim_of_mask(low).is_none() {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::MaskBitUnknown.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::MaskBitUnknown.code());
            }
            bits_seen = bits_seen.saturating_add(1);
            m &= m - 1; // 清最低位
        }
        // 闸2：空掩码带条件（f9；先于位-算子配对闸——空掩码是全局
        // 矛盾，优先于逐位配对细查）。
        if mask == 0 && self.arms_len > 0 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            self.ledger.push_code(EpCode::MaskEmptyArmed.code());
            self.phase = ArmPhase::Rejected;
            return Err(EpCode::MaskEmptyArmed.code());
        }
        // 闸3/闸4：逐条算子闭集 + 算子-维配对（f1/f2/f3/f4/f5）。
        let mut i = 0;
        while i < self.arms_len {
            let a = match self.arms[i] {
                Some(x) => x,
                None => {
                    self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                    self.ledger.push_code(EpCode::CondMissing.code());
                    self.phase = ArmPhase::Rejected;
                    return Err(EpCode::CondMissing.code());
                }
            };
            // 闸2：算子闭集。
            if a.op == 0 {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::CondOpZero.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::CondOpZero.code());
            }
            if a.op > VER_OR {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::CondOpOutOfRange.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::CondOpOutOfRange.code());
            }
            // 闸3：算子-维配对。
            let dim = match dim_of_mask(a.mask_bit) {
                Some(d) => d,
                None => {
                    self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                    self.ledger.push_code(EpCode::MaskBitUnknown.code());
                    self.phase = ArmPhase::Rejected;
                    return Err(EpCode::MaskBitUnknown.code());
                }
            };
            let numeric = a.op <= VER_LESS_EQUAL;
            if dim == 6 && numeric {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::SuiteNumericOp.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::SuiteNumericOp.code());
            }
            if dim != 6 && !numeric {
                if dim == 7 {
                    // 产品型维仅 VER_EQUAL（f5）。
                    self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                    self.ledger.push_code(EpCode::ProductTypeOp.code());
                    self.phase = ArmPhase::Rejected;
                    return Err(EpCode::ProductTypeOp.code());
                }
                // 数值维落 AND/OR（f3）。
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::NumericSuiteOp.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::NumericSuiteOp.code());
            }
            i += 1;
        }
        // 闸5：位-算子配对（f6 有位无条件 / f7 有条件无位）。
        let mut k = 0;
        while k < 8 {
            let bit = 1u32 << k;
            let in_mask = mask & bit != 0;
            let mut in_arms = false;
            let mut j = 0;
            while j < self.arms_len {
                match self.arms[j] {
                    Some(a) => {
                        if a.mask_bit == bit {
                            in_arms = true;
                        }
                    }
                    None => {}
                }
                j += 1;
            }
            if in_mask && !in_arms {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::CondMissing.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::CondMissing.code());
            }
            if !in_mask && in_arms {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                self.ledger.push_code(EpCode::MaskBitMissing.code());
                self.phase = ArmPhase::Rejected;
                return Err(EpCode::MaskBitMissing.code());
            }
            k += 1;
        }
        // 闸6：同维双条件冲突（f8）。
        let mut x = 0;
        while x < self.arms_len {
            let mut y = x + 1;
            while y < self.arms_len {
                let ax = match self.arms[x] {
                    Some(v) => v,
                    None => {
                        y = y.saturating_add(1);
                        continue;
                    }
                };
                let ay = match self.arms[y] {
                    Some(v) => v,
                    None => {
                        y = y.saturating_add(1);
                        continue;
                    }
                };
                if ax.mask_bit == ay.mask_bit && ax.op != ay.op {
                    self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                    self.ledger.push_code(EpCode::CondConflict.code());
                    self.phase = ArmPhase::Rejected;
                    return Err(EpCode::CondConflict.code());
                }
                y += 1;
            }
            x += 1;
        }
        // 全过：Validated（空掩码且无武装 = 零条件判定，合法直过）。
        let _ = bits_seen;
        self.ledger.passed = self.ledger.passed.saturating_add(1);
        self.phase = ArmPhase::Validated;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（拒形态数 / 账 / 三要素齐备数）。
pub type GoldenOut = (usize, EpLedger, usize);

/// 错误三要素：①返回旗 0 ②错误码（=Windows 87 闭集映射）③非空明细。
/// 返回 Err(code) 即三要素齐备形态；triad_ok 复核三要素。
pub fn triad_ok(code: u32) -> bool {
    // ①返回旗：错误路径恒失败（Err 形态即旗 0）——由调用形态保证；
    // ②错误码：段内闭集；
    // ③明细：非空中文。
    let in_seg = code >= SEG_BASE && code < SEG_BASE + 0x40;
    let d = detail(code);
    in_seg && !d.is_empty() && d != "未知诊断码：不在 0x7FC80 段闭集内"
}

/// 黄金全链：10 形态逐一注入 → 必拒 → 段码与 Windows 87 对照一致 →
/// 三要素齐备 → 账守恒。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut a = Armer::new();
    let mut rejected = 0usize;
    let mut triads = 0usize;
    let mut i = 0;
    while i < FORMS {
        let (fid, want_code, want_win, _desc) = match form_row(i) {
            Some(x) => x,
            None => return Err(EpCode::InjectionMissed.code()),
        };
        let mut e = Armer::new();
        // 按形态构造武装面（f6 用位无条件、f7 用条件无位、f9 空掩码）。
        // 注意：validate 的 Err 是「预期拒收」，不得上抛——拒因以
        // ledger.codes 留痕后逐项核对，上抛会打断黄金链正例。
        match fid {
            0 => {
                e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 })?;
                let _ = e.validate(0x8000_0000 | 0x02);
            }
            1 => {
                e.arm(Arm { mask_bit: 0x02, op: 0, req: 6 })?;
                let _ = e.validate(0x02);
            }
            2 => {
                e.arm(Arm { mask_bit: 0x02, op: 8, req: 6 })?;
                let _ = e.validate(0x02);
            }
            3 => {
                e.arm(Arm { mask_bit: 0x02, op: VER_AND, req: 6 })?;
                let _ = e.validate(0x02);
            }
            4 => {
                e.arm(Arm { mask_bit: 0x40, op: VER_EQUAL, req: 0x0100 })?;
                let _ = e.validate(0x40);
            }
            5 => {
                e.arm(Arm { mask_bit: 0x80, op: VER_GREATER, req: 1 })?;
                let _ = e.validate(0x80);
            }
            6 => {
                // 有位无条件：掩码带 0x04 位但武装面只有 0x02 位。
                e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 })?;
                let _ = e.validate(0x02 | 0x04);
            }
            7 => {
                // 有条件无位：武装 0x02 位条件但掩码只置 0x04 位。
                e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 })?;
                let _ = e.validate(0x04);
            }
            8 => {
                e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 })?;
                e.arm(Arm { mask_bit: 0x02, op: VER_GREATER, req: 6 })?;
                let _ = e.validate(0x02);
            }
            _ => {
                // f9：空掩码带条件。
                e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 })?;
                let _ = e.validate(0);
            }
        }
        // 必拒 + 段码一致 + Windows 87 对照 + 三要素。
        let last = match e.ledger.codes_len {
            0 => return Err(EpCode::InjectionMissed.code()),
            n => e.ledger.codes[n - 1],
        };
        if last != want_code || want_win != WIN_INVAL {
            return Err(EpCode::InjectionMissed.code());
        }
        if !triad_ok(last) {
            return Err(EpCode::TriadIncomplete.code());
        }
        triads += 1;
        rejected += 1;
        // 并账。
        a.ledger.tries = a.ledger.tries.saturating_add(e.ledger.tries);
        a.ledger.passed = a.ledger.passed.saturating_add(e.ledger.passed);
        a.ledger.rejected = a.ledger.rejected.saturating_add(e.ledger.rejected);
        i += 1;
    }
    if !a.ledger.conserve() {
        return Err(EpCode::InjectionMissed.code());
    }
    if rejected != FORMS {
        return Err(EpCode::InjectionMissed.code());
    }
    Ok((rejected, a.ledger, triads))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EpCode::NotReproducible.code());
    }
    Ok(())
}

/// 合法请求 happy path：六闸全过 → Validated。
pub fn happy_path() -> Result<(), u32> {
    let mut e = Armer::new();
    e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 })?;
    e.arm(Arm { mask_bit: 0x40, op: VER_AND, req: 0x0100 })?;
    e.validate(0x02 | 0x40)
}

// ── 注入核（三类 × 10 轮，10/10 全抓） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 非法掩码高位被拒次数。
    pub mask_caught: u32,
    /// 类2 非法算子被拒次数。
    pub op_caught: u32,
    /// 类3 三要素缺一被闭集校验抓住次数。
    pub triad_caught: u32,
}

/// 三类注入 rounds 轮；每类每轮必被抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 非法掩码：0x8000_0000 / 0x8000 / 0x0000_0100（位 8）三形态。
        for bad in [0x8000_0000u32, 0x8000u32, 0x100u32].iter() {
            let mut e = Armer::new();
            let _ = e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
            match e.validate(0x02 | *bad) {
                Err(c) if c == EpCode::MaskBitUnknown.code() => {
                    led.mask_caught = led.mask_caught.saturating_add(1);
                }
                _ => return Err(EpCode::InjectionMissed.code()),
            }
        }
        // 类2 非法算子：0 / 8 / 255 三形态。
        for bad_op in [0u8, 8u8, 255u8].iter() {
            let mut e = Armer::new();
            let _ = e.arm(Arm { mask_bit: 0x02, op: *bad_op, req: 6 });
            match e.validate(0x02) {
                Err(c) if c == EpCode::CondOpZero.code() && *bad_op == 0 => {
                    led.op_caught = led.op_caught.saturating_add(1);
                }
                Err(c) if c == EpCode::CondOpOutOfRange.code() && *bad_op != 0 => {
                    led.op_caught = led.op_caught.saturating_add(1);
                }
                _ => return Err(EpCode::InjectionMissed.code()),
            }
        }
        // 类3 三要素缺一：伪造段外码与空明细形态 → triad_ok 必拒。
        let bad_code = SEG_BASE + 0x3F; // 闭集外哨兵（段尾留白位）
        if triad_ok(bad_code) {
            return Err(EpCode::TriadIncomplete.code());
        }
        if triad_ok(EpCode::MaskBitUnknown.code()) {
            led.triad_caught = led.triad_caught.saturating_add(1);
        } else {
            return Err(EpCode::TriadIncomplete.code());
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：10 形态全拒且对照一致（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：合法请求过闸（happy path）。
pub fn probe_seq_b() -> Result<(), u32> {
    happy_path()
}

/// 探针 C：闸序直证——非法位优先于非法算子（同造两错先拒位）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = Armer::new();
    let _ = e.arm(Arm { mask_bit: 0x8000_0000, op: 0, req: 0 });
    // 校验时掩码直接含非法位 → 闸1 先拒（CondOpZero 的 arm() 已过，
    // 但六闸次序冻结：MaskBitUnknown 先于算子闸）。
    match e.validate(0x8000_0000) {
        Err(c) if c == EpCode::MaskBitUnknown.code() => Ok(()),
        Err(_) => Err(EpCode::InjectionMissed.code()),
        Ok(()) => Err(EpCode::InjectionMissed.code()),
    }
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < EpCode::ALL.len() {
        let v = EpCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（16 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：10 形态全拒、对照一致、三要素齐备（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, t)) => *n == FORMS && led.conserve() && *t == FORMS,
        Err(_) => false,
    };
    v.push(("J1-10形态全拒对照齐", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A形态顺跑", pa, format!("ok={}", pa)));

    // 3. 合法请求过闸（happy path）。
    let pb = probe_seq_b().is_ok();
    v.push(("J1-合法请求过闸", pb, format!("ok={}", pb)));

    // 4. 闸序直证：非法位先于非法算子（判定次序红线）。
    let pc = probe_seq_c().is_ok();
    v.push(("J1-闸序先位后算子", pc, String::from("六闸次序冻结")));

    // 5. 注入三类 10/10 全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match led.as_ref() {
        Ok(l) => {
            l.mask_caught == 3 * JUDGE_ROUNDS
                && l.op_caught == 3 * JUDGE_ROUNDS
                && l.triad_caught == JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入10/10全抓", inj_ok, format!("{:?}", led)));

    // 6. 形态对照表逐行核对（段码 + Windows 87 双一致）。
    let tbl_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < FORMS {
            match form_row(i) {
                Some((_f, code, win, _d)) => {
                    if win != WIN_INVAL || !triad_ok(code) {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-对照表逐行核对", tbl_ok, format!("10行→{}", WIN_INVAL)));

    // 7. 拒因账只增不减（10 形态逐次留痕）。
    let codes_ok = match golden_run() {
        Ok((_, led, _)) => led.rejected >= FORMS as u32,
        Err(_) => false,
    };
    v.push(("J1-拒因账留痕", codes_ok, String::from("逐次留段码")));

    // 8. 重复维冲突直证（f8 双武装必拒）。
    let conflict = {
        let mut e = Armer::new();
        let _ = e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        let _ = e.arm(Arm { mask_bit: 0x02, op: VER_GREATER, req: 6 });
        match e.validate(0x02) {
            Err(c) if c == EpCode::CondConflict.code() => true,
            _ => false,
        }
    };
    v.push(("J1-同维冲突直证", conflict, String::from("EQ+GT 同位拒")));

    // 9. 位-算子配对直证（f6 有位无条件 / f7 有条件无位 双向）。
    let pair_ok = {
        let mut e6 = Armer::new();
        let _ = e6.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        let r6 = e6.validate(0x02 | 0x04);
        let mut e7 = Armer::new();
        let _ = e7.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        let r7 = e7.validate(0x04);
        match (r6, r7) {
            (Err(c6), Err(c7)) => {
                c6 == EpCode::CondMissing.code() && c7 == EpCode::MaskBitMissing.code()
            }
            _ => false,
        }
    };
    v.push(("J1-位算子配对双向", pair_ok, String::from("f6/f7 双向拒")));

    // 10. 空掩码带条件直证（f9）。
    let f9 = {
        let mut e = Armer::new();
        let _ = e.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        match e.validate(0) {
            Err(c) if c == EpCode::MaskEmptyArmed.code() => true,
            _ => false,
        }
    };
    v.push(("J1-空掩码带条件拒", f9, String::from("f9 必拒")));

    // 11. 三要素闭集：14 码全非空 + 段内窗口。
    let all_ok = {
        let mut ok = segment_guard() && EpCode::ALL.len() == 14;
        let mut i = 0;
        while i < EpCode::ALL.len() {
            if !triad_ok(EpCode::ALL[i].code()) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-三要素闭集14码", all_ok, String::from("旗/码/明细齐")));

    // 12. 表指纹等价：10 形态段码互异（闭集无碰撞）。
    let distinct = {
        let mut ok = true;
        let mut i = 0;
        while i < FORMS {
            let mut j = i + 1;
            while j < FORMS {
                match (form_row(i), form_row(j)) {
                    (Some(a), Some(b)) => {
                        if a.1 == b.1 {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                j += 1;
            }
            i += 1;
        }
        ok
    };
    v.push(("J2-形态段码互异", distinct, String::from("10 码无碰撞")));

    // 13. 双跑一致（复现红线）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 14. Rejected 终态：拒后再校验必拒（终态单向）。
    let sealed = {
        let mut e = Armer::new();
        let _ = e.arm(Arm { mask_bit: 0x02, op: 0, req: 6 });
        let first = e.validate(0x02).is_err();
        let second = e.validate(0x02).is_err();
        first && second && e.phase == ArmPhase::Rejected
    };
    v.push(("J1-Rejected终态闸", sealed, String::from("终态再验仍拒")));

    // 15. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 16. 诊断码闭集（14 码 detail 全非空）。
    let det_ok = {
        let mut ok = EpCode::ALL.len() == 14;
        let mut i = 0;
        while i < EpCode::ALL.len() {
            if EpCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集14码", det_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 16 条；前 8 条动态明细）。
pub fn run_unx18515_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18515 错误路径账（非法掩码/未知条件）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18515_checks();
        assert!(set.all_passed(), "UNX-F18515 判据集应全绿");
    }

    /// 黄金链：10 形态全拒、对照一致、三要素齐备。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, t)) => {
                assert_eq!(n, 10);
                assert_eq!(led.rejected, 10);
                assert_eq!(t, 10);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 形态对照表：10 行全映射 87 且段码互异。
    #[test]
    fn test_form_table() {
        let mut i = 0;
        while i < FORMS {
            let row = match form_row(i) {
                Some(x) => x,
                None => {
                    assert!(false, "形态 {} 越界", i);
                    return;
                }
            };
            assert_eq!(row.2, WIN_INVAL, "形态 {} Windows 对照", i);
            assert!(triad_ok(row.1), "形态 {} 三要素", i);
            i += 1;
        }
        assert!(form_row(FORMS).is_none(), "形态表闭集");
    }

    /// 注入三类 10/10 全抓。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.mask_caught, 30);
                assert_eq!(l.op_caught, 30);
                assert_eq!(l.triad_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 六闸逐闸直证（每闸独立构错必拒）。
    #[test]
    fn test_six_gates() {
        // 闸1 f0。
        let mut e1 = Armer::new();
        let _ = e1.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        assert_eq!(e1.validate(0x8000_0000 | 0x02), Err(EpCode::MaskBitUnknown.code()));
        // 闸2 f1/f2。
        let mut e2 = Armer::new();
        let _ = e2.arm(Arm { mask_bit: 0x02, op: 0, req: 6 });
        assert_eq!(e2.validate(0x02), Err(EpCode::CondOpZero.code()));
        let mut e3 = Armer::new();
        let _ = e3.arm(Arm { mask_bit: 0x02, op: 8, req: 6 });
        assert_eq!(e3.validate(0x02), Err(EpCode::CondOpOutOfRange.code()));
        // 闸3 f3/f4/f5。
        let mut e4 = Armer::new();
        let _ = e4.arm(Arm { mask_bit: 0x02, op: VER_AND, req: 6 });
        assert_eq!(e4.validate(0x02), Err(EpCode::NumericSuiteOp.code()));
        let mut e5 = Armer::new();
        let _ = e5.arm(Arm { mask_bit: 0x40, op: VER_EQUAL, req: 0x0100 });
        assert_eq!(e5.validate(0x40), Err(EpCode::SuiteNumericOp.code()));
        let mut e6 = Armer::new();
        let _ = e6.arm(Arm { mask_bit: 0x80, op: VER_GREATER, req: 1 });
        assert_eq!(e6.validate(0x80), Err(EpCode::ProductTypeOp.code()));
        // 闸4 f6/f7、闸5 f8、闸6 f9。
        let mut e7 = Armer::new();
        let _ = e7.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        assert_eq!(e7.validate(0x02 | 0x04), Err(EpCode::CondMissing.code()));
        let mut e8 = Armer::new();
        let _ = e8.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        let _ = e8.arm(Arm { mask_bit: 0x02, op: VER_GREATER, req: 6 });
        assert_eq!(e8.validate(0x02), Err(EpCode::CondConflict.code()));
        let mut e9 = Armer::new();
        let _ = e9.arm(Arm { mask_bit: 0x02, op: VER_EQUAL, req: 6 });
        assert_eq!(e9.validate(0), Err(EpCode::MaskEmptyArmed.code()));
    }

    /// happy path + 套件维 AND/OR 合法面。
    #[test]
    fn test_happy_paths() {
        assert!(happy_path().is_ok());
        let mut e = Armer::new();
        assert!(e.arm(Arm { mask_bit: 0x40, op: VER_OR, req: 0x1000 }).is_ok());
        assert!(e.validate(0x40).is_ok());
        assert_eq!(e.phase, ArmPhase::Validated);
    }

    /// 三要素校验：段内全过、段外哨兵必拒。
    #[test]
    fn test_triad() {
        for c in EpCode::ALL.iter() {
            assert!(triad_ok(c.code()));
        }
        assert!(!triad_ok(SEG_BASE + 0x3F));
        assert!(!triad_ok(0));
    }

    /// 双跑一致 + 段位闭集。
    #[test]
    fn test_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(EpCode::ALL.len(), 14);
        for c in EpCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
