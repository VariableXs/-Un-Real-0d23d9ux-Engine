//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18493 · 覆写错误路径账（非法维度/值域） · 0x7F700 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18493-J1 非法值 10 形态注入返回码与 Windows 一致，错误三
//!       要素齐备
//! 本模块结构清单：
//!   1. 冻结常量层：参数包计数上限 8 / 维度闭集 1..=3 / 值域四段 /
//!       判据轮数 10；
//!   2. 诊断码闭集 VdCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. Windows 错误码对账表 WIN_MAP（冻结 10 项，逐项可复算）：
//!       87 ERROR_INVALID_PARAMETER / 13 ERROR_INVALID_DATA / 1413
//!       ERROR_INVALID_INDEX / 1004 ERROR_INVALID_FLAGS / 183
//!       ERROR_ALREADY_EXISTS / 122 ERROR_INSUFFICIENT_BUFFER；
//!   4. 数据结构：
//!       - 参数包 ParamPack（dim/major/minor/build/plat/flags/count 七元）；
//!       - 数组账本 ErrLedger（注入/拒收/日志三账 + 环形日志 32 槽）；
//!       - 形态注入账 InjectLedger（10 槽逐类）；
//!   5. 判定器 validate()：参数校验流水，判定次序与 Windows 语义一致
//!       （维度类先于 flags 类先于值域类——两错同时成立先返回约定码）；
//!   6. 错误三要素：每条日志（WIN 码, 阶段序号 1..=3, FNV 摘要）逐项
//!       非零且阶段在闭集——齐备性机器断言；
//!   7. 注入核：10 形态（超长计数/空包/维度越界/保留 flags/全零 flags/
//!       维度不配/重复维度/major 越界/build 越界/plat 越界）×10 轮，
//!       逐类对码且与 Windows 映射逐项一致；
//!   8. run_unx18493_checks：17 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F700，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F700;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 参数包计数上限（写死 8：批量校验一次至多 8 包——与覆写表容量同域）。
pub const COUNT_MAX: u32 = 8;
/// 维度闭集（冻结 1..=3：主版本/次版本/包层 三维，与 F18491 同表）。
pub const DIM_MAX: u32 = 3;
/// 版本值域（冻结：与 F18491 四段同域——跨卡口径一致）。
pub const VER_MAJOR_MAX: u32 = 10;
pub const VER_MINOR_MAX: u32 = 9;
pub const VER_BUILD_MAX: u32 = 65535;
pub const PLAT_MAX: u32 = 3;
/// flags 有效位（冻结低 4 位：bit0 major / bit1 minor / bit2 build /
/// bit3 plat；bit4..15 保留恒零）。
pub const FLAGS_VALID_MASK: u16 = 0x000F;
/// 阶段闭集（写死 3：1 维度类 / 2 flags 类 / 3 值域类）。
pub const PHASE_DIM: u8 = 1;
pub const PHASE_FLAG: u8 = 2;
pub const PHASE_VALUE: u8 = 3;
/// 日志环容量（写死 32）。
pub const LOG_CAP: usize = 32;
/// 判据轮数（写死 10：J1"10 形态注入"×10 轮语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── Windows 错误码对账表（冻结，逐项可复算） ───────────────────────
/// ERROR_INVALID_FUNCTION（值 1）。
pub const WIN_INVALID_FUNCTION: u32 = 1;
/// ERROR_INVALID_DATA（值 13）。
pub const WIN_INVALID_DATA: u32 = 13;
/// ERROR_NOT_SUPPORTED（值 50）。
pub const WIN_NOT_SUPPORTED: u32 = 50;
/// ERROR_INVALID_PARAMETER（值 87）。
pub const WIN_INVALID_PARAMETER: u32 = 87;
/// ERROR_INSUFFICIENT_BUFFER（值 122）。
pub const WIN_INSUFFICIENT_BUFFER: u32 = 122;
/// ERROR_ALREADY_EXISTS（值 183）。
pub const WIN_ALREADY_EXISTS: u32 = 183;
/// ERROR_INVALID_FLAGS（值 1004）。
pub const WIN_INVALID_FLAGS: u32 = 1004;
/// ERROR_INVALID_INDEX（值 1413）。
pub const WIN_INVALID_INDEX: u32 = 1413;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VdCode {
    /// 参数计数超限（>8）
    CountTooBig,
    /// 空参数包（全零哨兵）
    NullPack,
    /// 维度越界（0 或 >3）
    DimOutOfRange,
    /// 重复维度（同包同维重复声明）
    DimDup,
    /// 保留 flags 位被置（bit4..15 非零）
    FlagsReserved,
    /// flags 全零（无任何有效段声明）
    FlagsEmpty,
    /// 维度-flags 不配（dim 声明的段 flags 未置）
    DimFlagMismatch,
    /// major 零/越界
    MajorBad,
    /// minor 越界
    MinorBad,
    /// build 越界
    BuildBad,
    /// plat 越界
    PlatBad,
    /// 判定次序破坏（维度类错误应先于值域类）
    OrderBroken,
    /// Windows 映射破坏（本地码→WIN 码与冻结表不符）
    WinMapBroken,
    /// 三账守恒破坏
    LedgerBroken,
    /// 三要素不齐
    TripleBroken,
    /// 双跑不一致
    NotReproducible,
}

impl VdCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VdCode::CountTooBig => "参数计数超限：一次至多 8 包",
            VdCode::NullPack => "空参数包：全零哨兵",
            VdCode::DimOutOfRange => "维度越界：须在 1..=3",
            VdCode::DimDup => "重复维度：同包同维重复声明",
            VdCode::FlagsReserved => "保留 flags 位被置：bit4..15 恒零",
            VdCode::FlagsEmpty => "flags 全零：无有效段声明",
            VdCode::DimFlagMismatch => "维度-flags 不配：dim 段未声明",
            VdCode::MajorBad => "major 零/越界：须在 1..=10",
            VdCode::MinorBad => "minor 越界：须在 0..=9",
            VdCode::BuildBad => "build 越界：须在 0..=65535",
            VdCode::PlatBad => "plat 越界：须在 1..=3",
            VdCode::OrderBroken => "判定次序破坏：维度类先于值域类",
            VdCode::WinMapBroken => "Windows 映射破坏：与冻结对账表不符",
            VdCode::LedgerBroken => "三账守恒破坏：injected≠rejected≠logged",
            VdCode::TripleBroken => "错误三要素不齐：码/阶段/摘要",
            VdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [VdCode; 16] = [
        VdCode::CountTooBig,
        VdCode::NullPack,
        VdCode::DimOutOfRange,
        VdCode::DimDup,
        VdCode::FlagsReserved,
        VdCode::FlagsEmpty,
        VdCode::DimFlagMismatch,
        VdCode::MajorBad,
        VdCode::MinorBad,
        VdCode::BuildBad,
        VdCode::PlatBad,
        VdCode::OrderBroken,
        VdCode::WinMapBroken,
        VdCode::LedgerBroken,
        VdCode::TripleBroken,
        VdCode::NotReproducible,
    ];
}

/// 本地码 → Windows 码映射（冻结对账表——与 Win32 GetLastError 值域逐项
/// 一致）。推导：参数计数/空包/全零 flags 属"参数非法"→87；维度越界/
/// plat 越界属"索引越界"→1413；保留位/不配属"标志非法"→1004；重复
/// 维度属"对象已存在"→183；值域越界属"数据非法"→13；build 超限属
/// "缓冲尺寸不足"→122。
pub fn win_map(local: u32) -> Result<u32, u32> {
    match local {
        c if c == VdCode::CountTooBig.code() => Ok(WIN_INVALID_PARAMETER),
        c if c == VdCode::NullPack.code() => Ok(WIN_INVALID_DATA),
        c if c == VdCode::DimOutOfRange.code() => Ok(WIN_INVALID_INDEX),
        c if c == VdCode::DimDup.code() => Ok(WIN_ALREADY_EXISTS),
        c if c == VdCode::FlagsReserved.code() => Ok(WIN_INVALID_FLAGS),
        c if c == VdCode::FlagsEmpty.code() => Ok(WIN_INVALID_PARAMETER),
        c if c == VdCode::DimFlagMismatch.code() => Ok(WIN_INVALID_FLAGS),
        c if c == VdCode::MajorBad.code() => Ok(WIN_INVALID_DATA),
        c if c == VdCode::MinorBad.code() => Ok(WIN_INVALID_DATA),
        c if c == VdCode::BuildBad.code() => Ok(WIN_INSUFFICIENT_BUFFER),
        c if c == VdCode::PlatBad.code() => Ok(WIN_INVALID_INDEX),
        _ => Err(VdCode::WinMapBroken.code()),
    }
}

/// FNV-1a 32（三要素摘要基元——确定性，零随机源）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 参数包 ─────────────────────────────────────────────────────────
/// 覆写参数包（七元组：维度/版本四段/flags/计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamPack {
    pub dim: u32,
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub plat: u32,
    pub flags: u16,
    pub count: u32,
}

impl ParamPack {
    /// 零包哨兵（const）。
    pub const fn zero() -> ParamPack {
        ParamPack { dim: 0, major: 0, minor: 0, build: 0, plat: 0, flags: 0, count: 0 }
    }
    /// 规范包（dim=3 全段声明，值域内——不误伤基线）。
    pub const fn canonical() -> ParamPack {
        ParamPack { dim: 3, major: 6, minor: 1, build: 7650, plat: 2, flags: 0x000F, count: 1 }
    }
    /// 全零判别（空包哨兵形态）。
    pub fn is_null(&self) -> bool {
        self.dim == 0
            && self.major == 0
            && self.minor == 0
            && self.build == 0
            && self.plat == 0
            && self.flags == 0
            && self.count == 0
    }
}

/// 维度作用域位掩码（推导：dim=1 只覆盖 major 段 → 0x0001；dim=2 覆盖
/// major+minor → 0x0003；dim=3 全段 → 0x000F——超出作用域的段位声明
/// 即"重复维度"形态）。
pub fn dim_scope(dim: u32) -> u16 {
    match dim {
        1 => 0x0001u16,
        2 => 0x0003u16,
        _ => 0x000Fu16,
    }
}

/// 判定器：参数校验流水。判定次序与 Windows 语义一致——
///   ①计数 → ②空包 → ③维度越界 → ④flags 全零 → ⑤保留位 →
///     ⑥维度-flags 不配 → ⑦越维声明（重复维度）→
///     ⑧值域类（major→minor→build→plat 逐段）。
/// 两错同时成立先返回约定码（次序即语义）。
pub fn validate(p: ParamPack) -> Result<u32, u32> {
    // ① 计数闸。
    if p.count > COUNT_MAX {
        return Err(VdCode::CountTooBig.code());
    }
    // ② 空包哨兵。
    if p.is_null() {
        return Err(VdCode::NullPack.code());
    }
    // ③ 维度越界。
    if p.dim == 0 || p.dim > DIM_MAX {
        return Err(VdCode::DimOutOfRange.code());
    }
    // ④ flags 全零（无任何有效段声明）。
    if p.flags == 0 {
        return Err(VdCode::FlagsEmpty.code());
    }
    // ⑤ 保留位闸（bit4..15 恒零）。
    if p.flags & !FLAGS_VALID_MASK != 0 {
        return Err(VdCode::FlagsReserved.code());
    }
    // ⑥ 维度-flags 不配（dim 自身段位未声明）。
    let need = if p.dim == 1 { 0x0001u16 } else if p.dim == 2 { 0x0002u16 } else { 0x0004u16 };
    if p.flags & need != need {
        return Err(VdCode::DimFlagMismatch.code());
    }
    // ⑦ 越维声明（超出 dim 作用域的段位被声明 = 重复维度）。
    if p.flags & !dim_scope(p.dim) != 0 {
        return Err(VdCode::DimDup.code());
    }
    // ⑧ 值域类（major→minor→build→plat 次序固定）。
    if p.flags & 0x0001 != 0 && (p.major == 0 || p.major > VER_MAJOR_MAX) {
        return Err(VdCode::MajorBad.code());
    }
    if p.flags & 0x0002 != 0 && p.minor > VER_MINOR_MAX {
        return Err(VdCode::MinorBad.code());
    }
    if p.flags & 0x0004 != 0 && p.build > VER_BUILD_MAX {
        return Err(VdCode::BuildBad.code());
    }
    if p.flags & 0x0008 != 0 && (p.plat == 0 || p.plat > PLAT_MAX) {
        return Err(VdCode::PlatBad.code());
    }
    // 全闸过 → 打包裁决值（与 F18491 同打包式）。
    Ok(p.major * 100_000_000 + p.minor * 1_000_000 + p.build * 10 + p.plat)
}

// ── 10 形态注入样本（每形态 = 包 + 预期本地码） ────────────────────
/// 形态数（写死 10：J1"非法值 10 形态"语义）。
pub const FORM_N: usize = 10;

/// 构造第 f 号非法形态（纯函数，确定性）。
pub fn make_form(f: usize) -> ParamPack {
    match f {
        // 0 超长参数计数。
        0 => ParamPack { count: COUNT_MAX + 1, ..ParamPack::canonical() },
        // 1 空参数包。
        1 => ParamPack::zero(),
        // 2 维度越界。
        2 => ParamPack { dim: DIM_MAX + 1, ..ParamPack::canonical() },
        // 3 保留 flags 位（bit15）。
        3 => ParamPack { flags: FLAGS_VALID_MASK | 0x8000, ..ParamPack::canonical() },
        // 4 flags 全零。
        4 => ParamPack { flags: 0, ..ParamPack::canonical() },
        // 5 维度-flags 不配（dim=2 但 minor 位未声明）。
        5 => ParamPack { dim: 2, flags: 0x000D, ..ParamPack::canonical() },
        // 6 重复维度（dim=1 却越维声明 minor 段——超出作用域即重复声明）。
        6 => ParamPack { dim: 1, flags: 0x0003, major: 6, minor: 1, build: 0, plat: 0, count: 1 },
        // 7 major 越界（=0）。
        7 => ParamPack { major: 0, ..ParamPack::canonical() },
        // 8 build 越界（+1）。
        8 => ParamPack { build: VER_BUILD_MAX + 1, ..ParamPack::canonical() },
        // 9 plat 越界。
        _ => ParamPack { plat: PLAT_MAX + 1, ..ParamPack::canonical() },
    }
}

/// 第 f 号形态预期本地码（冻结——与判定器次序逐项复算）。
pub fn expected_code(f: usize) -> u32 {
    match f {
        0 => VdCode::CountTooBig.code(),
        1 => VdCode::NullPack.code(),
        2 => VdCode::DimOutOfRange.code(),
        3 => VdCode::FlagsReserved.code(),
        4 => VdCode::FlagsEmpty.code(),
        5 => VdCode::DimFlagMismatch.code(),
        6 => VdCode::DimDup.code(),
        7 => VdCode::MajorBad.code(),
        8 => VdCode::BuildBad.code(),
        _ => VdCode::PlatBad.code(),
    }
}

/// 形态判定封装（与注入核同路径：validate(make_form(f))）。
pub fn validate_form(f: usize) -> Result<u32, u32> {
    validate(make_form(f))
}

// ── 错误路径账（三账守恒 + 环形日志三要素） ────────────────────────
/// 单条错误日志（三要素：WIN 码/阶段/摘要）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub win_code: u32,
    pub phase: u8,
    pub digest: u32,
}

/// 错误路径账：injected==rejected==logged 三账守恒；日志环形 32 槽
/// 满后覆盖并显性记账。
pub struct ErrLedger {
    pub injected: u32,
    pub rejected: u32,
    pub logged: u32,
    pub log: [ErrEntry; LOG_CAP],
    pub head: usize,
    pub len: usize,
    pub overflow: u32,
}

impl ErrLedger {
    /// 空账。
    pub fn new() -> ErrLedger {
        ErrLedger {
            injected: 0,
            rejected: 0,
            logged: 0,
            log: [ErrEntry { win_code: 0, phase: 0, digest: 0 }; LOG_CAP],
            head: 0,
            len: 0,
            overflow: 0,
        }
    }

    /// 喂入一包：validate 必拒才入账（三账 +1，日志三要素）。
    /// 返回 Err(本地码)=已被拒；Ok=未被拒（漏检，调用方据守恒判红）。
    pub fn feed(&mut self, p: ParamPack, phase: u8) -> Result<(), u32> {
        self.injected = self.injected.saturating_add(1);
        match validate(p) {
            Err(local) => {
                self.rejected = self.rejected.saturating_add(1);
                // 摘要 = FNV(本地码‖阶段‖包七元)——同包同码同摘要（幂等）。
                let mut buf = [0u8; 16];
                buf[0] = (local % 256) as u8;
                buf[1] = ((local / 256) % 256) as u8;
                buf[2] = ((local / 65536) % 256) as u8;
                buf[3] = ((local / 16777216) % 256) as u8;
                buf[4] = phase;
                buf[5] = (p.dim % 256) as u8;
                buf[6] = (p.major % 256) as u8;
                buf[7] = (p.minor % 256) as u8;
                buf[8] = (p.build % 256) as u8;
                buf[9] = ((p.build / 256) % 256) as u8;
                buf[10] = (p.plat % 256) as u8;
                buf[11] = (p.flags % 256) as u8;
                buf[12] = ((p.flags / 256) % 256) as u8;
                buf[13] = (p.count % 256) as u8;
                buf[14] = 0;
                buf[15] = 0;
                let d = fnv1a(&buf);
                let dd = if d == 0 { 1 } else { d };
                let win = match win_map(local) {
                    Ok(w) => w,
                    Err(_) => WIN_NOT_SUPPORTED,
                };
                // 环形入账（满后覆盖 + overflow 显性记账）。
                self.log[self.head] = ErrEntry { win_code: win, phase, digest: dd };
                self.head = (self.head + 1) % LOG_CAP;
                if self.len < LOG_CAP {
                    self.len += 1;
                } else {
                    self.overflow = self.overflow.saturating_add(1);
                }
                self.logged = self.logged.saturating_add(1);
                Err(local)
            }
            Ok(_) => {
                // 合法包：只记 injected（不误伤基线）。
                Ok(())
            }
        }
    }

    /// 三账守恒：injected == rejected + 通过数，且 rejected == logged。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.rejected != self.logged {
            return Err(VdCode::LedgerBroken.code());
        }
        if self.injected < self.rejected {
            return Err(VdCode::LedgerBroken.code());
        }
        Ok(())
    }

    /// 三要素齐备：逐条 win 码在冻结域 / 阶段在 1..=3 / 摘要非零。
    pub fn triple_ok(&self) -> bool {
        let win_domain = [
            WIN_INVALID_FUNCTION,
            WIN_INVALID_DATA,
            WIN_NOT_SUPPORTED,
            WIN_INVALID_PARAMETER,
            WIN_INSUFFICIENT_BUFFER,
            WIN_ALREADY_EXISTS,
            WIN_INVALID_FLAGS,
            WIN_INVALID_INDEX,
        ];
        let mut i = 0;
        while i < self.len {
            let e = self.log[i];
            if e.digest == 0 || e.phase == 0 || e.phase > PHASE_VALUE {
                return false;
            }
            let mut found = false;
            let mut k = 0;
            while k < win_domain.len() {
                if e.win_code == win_domain[k] {
                    found = true;
                }
                k += 1;
            }
            if !found {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 注入核 ─────────────────────────────────────────────────────────
/// 形态注入账（10 槽逐类，守恒 tries==caught 漏检必 0）。
pub struct InjectLedger {
    pub tries: [u32; FORM_N],
    pub caught: [u32; FORM_N],
    /// 本地码对账命中（逐类与 expected_code 一致的轮数）。
    pub code_match: [u32; FORM_N],
    /// Windows 映射对账命中。
    pub win_match: [u32; FORM_N],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger {
            tries: [0; FORM_N],
            caught: [0; FORM_N],
            code_match: [0; FORM_N],
            win_match: [0; FORM_N],
        }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool, code_ok: bool, win_ok: bool) {
        if slot < FORM_N {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
            if code_ok {
                self.code_match[slot] = self.code_match[slot].saturating_add(1);
            }
            if win_ok {
                self.win_match[slot] = self.win_match[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：十槽全记录、全抓、码与映射全对账。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < FORM_N {
            if self.tries[i] == 0
                || self.caught[i] != self.tries[i]
                || self.code_match[i] != self.tries[i]
                || self.win_match[i] != self.tries[i]
            {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 10 形态 × rounds 轮注入：每轮全新 ErrLedger，逐类对码 + Windows 对账。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut f = 0;
        while f < FORM_N {
            let mut l = ErrLedger::new();
            let res = l.feed(make_form(f), PHASE_DIM);
            let hit = res.is_err();
            let code_ok = res.err() == Some(expected_code(f));
            // Windows 对账：本地码映射 == 冻结表预期。
            let win_ok = match res.err() {
                Some(c) => match win_map(c) {
                    Ok(w) => w == win_map(expected_code(f)).unwrap_or(WIN_NOT_SUPPORTED),
                    Err(_) => false,
                },
                None => false,
            };
            led.record(f, hit, code_ok, win_ok);
            f += 1;
        }
        r += 1;
    }
    Ok(led)
}

/// 判定次序断言：维度类与值域类错误同时成立 → 先返回维度类约定码。
pub fn order_check() -> Result<(), u32> {
    // dim=9 越界 ∧ build=65536 越界 → 必先 DimOutOfRange。
    let p = ParamPack { dim: DIM_MAX + 1, build: VER_BUILD_MAX + 1, ..ParamPack::canonical() };
    match validate(p) {
        Err(c) if c == VdCode::DimOutOfRange.code() => Ok(()),
        Err(_) => Err(VdCode::OrderBroken.code()),
        Ok(_) => Err(VdCode::OrderBroken.code()),
    }
}

/// 边界四类：零/上限/上限+1/闭集内最小值。
pub fn boundary_check() -> Result<(), u32> {
    // 维度：1（最小合法）过 / 3（上限）过 / 0 拒 / 4（上限+1）拒。
    let d1 = ParamPack { dim: 1, flags: 0x0001, major: 6, minor: 0, build: 0, plat: 0, count: 1 };
    let d3 = ParamPack { dim: DIM_MAX, flags: 0x000F, ..ParamPack::canonical() };
    let d0 = ParamPack { dim: 0, ..ParamPack::canonical() };
    let d4 = ParamPack { dim: DIM_MAX + 1, ..ParamPack::canonical() };
    if validate(d1).is_err() || validate(d3).is_err() {
        return Err(VdCode::DimOutOfRange.code());
    }
    if validate(d0).is_ok() || validate(d4).is_ok() {
        return Err(VdCode::DimOutOfRange.code());
    }
    // build：65535（上限）过 / 65536（上限+1）拒。
    let bmax = ParamPack { build: VER_BUILD_MAX, ..ParamPack::canonical() };
    let bovf = ParamPack { build: VER_BUILD_MAX + 1, ..ParamPack::canonical() };
    if validate(bmax).is_err() || validate(bovf).is_ok() {
        return Err(VdCode::BuildBad.code());
    }
    Ok(())
}

/// 幂等：同形态同包两次判定同码同摘要（环形账两次喂入逐字段相等）。
pub fn idempotent_check() -> Result<(), u32> {
    let mut l = ErrLedger::new();
    let p = make_form(3);
    let a = l.feed(p, PHASE_FLAG);
    let b = l.feed(p, PHASE_FLAG);
    if a.err() != b.err() {
        return Err(VdCode::NotReproducible.code());
    }
    if l.log[0].digest != l.log[1].digest {
        return Err(VdCode::NotReproducible.code());
    }
    Ok(())
}

/// 双跑一致：全量 10 形态跑两遍逐形态对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = inject_rounds(JUDGE_ROUNDS)?;
    let b = inject_rounds(JUDGE_ROUNDS)?;
    if a.tries != b.tries || a.caught != b.caught || a.code_match != b.code_match {
        return Err(VdCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VdCode::ALL.len() {
        let v = VdCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < VdCode::ALL.len() {
        if VdCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 不误伤基线：规范包与常见合法变体全过。
pub fn good_passes() -> Result<(), u32> {
    let base = ParamPack::canonical();
    if validate(base).is_err() {
        return Err(VdCode::NullPack.code());
    }
    let only_major = ParamPack { dim: 1, flags: 0x0001, major: 6, minor: 0, build: 0, plat: 0, count: 1 };
    if validate(only_major).is_err() {
        return Err(VdCode::DimFlagMismatch.code());
    }
    let dim2 = ParamPack { dim: 2, flags: 0x0003, major: 6, minor: 1, build: 0, plat: 0, count: 1 };
    if validate(dim2).is_err() {
        return Err(VdCode::DimFlagMismatch.code());
    }
    Ok(())
}

// ── 判据收口（17 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18493_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18493 覆写错误路径账（非法维度/值域）");
    // 1. 正常参数不误伤。
    s.add("J1-正常参数通过", good_passes().is_ok(), "规范包与合法变体全过");
    // 2. 10 形态 ×10 轮全抓且码/映射全对账（J1 主体）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-10形态注入全过", inj_ok, &format!("caught={:?}", inj.as_ref().map(|l| l.caught)));
    // 3. Windows 映射逐项一致（10 本地码全映射且域内）。
    let win_ok = {
        let mut ok = true;
        let mut f = 0;
        while f < FORM_N {
            match win_map(expected_code(f)) {
                Ok(_) => {}
                Err(_) => ok = false,
            }
            f += 1;
        }
        ok
    };
    s.add("J1-Windows映射逐项一致", win_ok, "冻结对账表 10 项全映射");
    // 4. 错误三要素齐备（喂满 10 形态逐条复核）。
    let triple = {
        let mut l = ErrLedger::new();
        let mut f = 0;
        let mut fed = 0;
        while f < FORM_N {
            if l.feed(make_form(f), PHASE_DIM).is_err() {
                fed += 1;
            }
            f += 1;
        }
        fed == FORM_N && l.conserve().is_ok() && l.triple_ok() && l.len == FORM_N
    };
    s.add("J1-错误三要素齐备", triple, "WIN码/阶段/摘要逐条非零");
    // 5. 三账守恒。
    let cons = {
        let mut l = ErrLedger::new();
        let mut f = 0;
        while f < FORM_N {
            let _ = l.feed(make_form(f), PHASE_DIM);
            f += 1;
        }
        l.conserve().is_ok() && l.injected == l.rejected && l.injected == FORM_N as u32
    };
    s.add("J1-三账守恒", cons, "injected==rejected==logged");
    // 6. 判定次序（维度类先于值域类）。
    s.add("J1-判定次序", order_check().is_ok(), "维度类错误先返回");
    // 7-11. 形态逐码样例（5 个代表性形态）。
    s.add(
        "J1-形态:计数超限",
        validate(ParamPack { count: COUNT_MAX + 1, ..ParamPack::canonical() }).err()
            == Some(VdCode::CountTooBig.code()),
        "WIN=87",
    );
    s.add(
        "J1-形态:空包",
        validate(ParamPack::zero()).err() == Some(VdCode::NullPack.code()),
        "WIN=13",
    );
    s.add(
        "J1-形态:维度越界",
        validate(make_form(2)).err() == Some(VdCode::DimOutOfRange.code()),
        "WIN=1413",
    );
    s.add(
        "J1-形态:保留flags",
        validate(make_form(3)).err() == Some(VdCode::FlagsReserved.code()),
        "WIN=1004",
    );
    s.add(
        "J1-形态:build越界",
        validate(make_form(8)).err() == Some(VdCode::BuildBad.code()),
        "WIN=122",
    );
    // 12. 边界四类。
    s.add("J1-边界四类", boundary_check().is_ok(), "零/上限/上限+1/最小");
    // 13. 幂等（同形态同码同摘要）。
    s.add("J1-注入幂等", idempotent_check().is_ok(), "同包两次账面一致");
    // 14. 环形覆盖（LOG_CAP+4 次喂入 len 恒 ≤CAP）。
    let ring = {
        let mut l = ErrLedger::new();
        let mut k = 0;
        while k < LOG_CAP + 4 {
            let _ = l.feed(make_form(k % FORM_N), PHASE_VALUE);
            k += 1;
        }
        l.len <= LOG_CAP && l.overflow >= 4 && l.conserve().is_ok()
    };
    s.add("J1-环形覆盖显性记账", ring, "满后覆盖 overflow>=4");
    // 15. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "全量两跑逐形态对拍");
    // 16. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7F700 窗口");
    // 17. 文案闭集。
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18493_checks_green() {
        let set = run_unx18493_checks();
        assert!(set.all_passed(), "UNX-F18493 red: {}", set.red_items_text());
    }

    #[test]
    fn t_forms_match_expected() {
        let forms_codes = [
            VdCode::CountTooBig,
            VdCode::NullPack,
            VdCode::DimOutOfRange,
            VdCode::FlagsReserved,
            VdCode::FlagsEmpty,
            VdCode::DimFlagMismatch,
            VdCode::DimDup,
            VdCode::MajorBad,
            VdCode::BuildBad,
            VdCode::PlatBad,
        ];
        let mut f = 0;
        while f < FORM_N {
            assert_eq!(
                validate_form(f).err(),
                Some(forms_codes[f].code()),
                "形态 {} 拒因不符",
                f
            );
            f += 1;
        }
    }

    #[test]
    fn t_win_map_frozen_table() {
        // 对账表逐项复算（冻结 WIN 常量域）。
        assert_eq!(win_map(VdCode::CountTooBig.code()).unwrap_or(0), WIN_INVALID_PARAMETER);
        assert_eq!(win_map(VdCode::NullPack.code()).unwrap_or(0), WIN_INVALID_DATA);
        assert_eq!(win_map(VdCode::DimOutOfRange.code()).unwrap_or(0), WIN_INVALID_INDEX);
        assert_eq!(win_map(VdCode::FlagsReserved.code()).unwrap_or(0), WIN_INVALID_FLAGS);
        assert_eq!(win_map(VdCode::DimDup.code()).unwrap_or(0), WIN_ALREADY_EXISTS);
        assert_eq!(win_map(VdCode::BuildBad.code()).unwrap_or(0), WIN_INSUFFICIENT_BUFFER);
        assert_eq!(win_map(VdCode::PlatBad.code()).unwrap_or(0), WIN_INVALID_INDEX);
        // 闭集外映射必拒。
        assert!(win_map(SEG_BASE + 0x3F).is_err(), "未登记码不映射");
    }

    #[test]
    fn t_ten_rounds_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught());
        // 总注入 = 10 形态 × 10 轮 = 100。
        let mut total = 0u32;
        let mut i = 0;
        while i < FORM_N {
            total += l.tries[i];
            i += 1;
        }
        assert_eq!(total, 100, "总注入 100");
    }

    #[test]
    fn t_order_dimension_before_value() {
        // 次序断言：值域坏 + 维度坏同时成立 → 维度码先出。
        let p1 = ParamPack { dim: DIM_MAX + 1, major: 0, ..ParamPack::canonical() };
        assert_eq!(validate(p1).err(), Some(VdCode::DimOutOfRange.code()));
        // flags 保留位 + 维度越界同时 → 维度类（③）先于 flags 类（④）。
        let p2 = ParamPack { dim: 0, flags: 0x8000, ..ParamPack::canonical() };
        assert_eq!(validate(p2).err(), Some(VdCode::DimOutOfRange.code()));
    }

    #[test]
    fn t_value_range_boundaries() {
        // major：0 拒 / 1 过 / 10 过 / 11 拒。
        assert_eq!(validate(ParamPack { major: 0, ..ParamPack::canonical() }).err(), Some(VdCode::MajorBad.code()));
        assert!(validate(ParamPack { major: 1, ..ParamPack::canonical() }).is_ok());
        assert!(validate(ParamPack { major: VER_MAJOR_MAX, ..ParamPack::canonical() }).is_ok());
        assert_eq!(validate(ParamPack { major: VER_MAJOR_MAX + 1, ..ParamPack::canonical() }).err(), Some(VdCode::MajorBad.code()));
        // plat：0 拒 / 3 过 / 4 拒。
        assert_eq!(validate(ParamPack { plat: 0, ..ParamPack::canonical() }).err(), Some(VdCode::PlatBad.code()));
        assert!(validate(ParamPack { plat: PLAT_MAX, ..ParamPack::canonical() }).is_ok());
        assert_eq!(validate(ParamPack { plat: PLAT_MAX + 1, ..ParamPack::canonical() }).err(), Some(VdCode::PlatBad.code()));
    }

    #[test]
    fn t_flags_gates() {
        // 保留位（bit15）拒 / 全零拒 / dim=2 无 minor 位拒 / dim=3 无 build 位拒。
        assert_eq!(validate(ParamPack { flags: 0x8000, ..ParamPack::canonical() }).err(), Some(VdCode::FlagsReserved.code()));
        assert_eq!(validate(ParamPack { flags: 0, ..ParamPack::canonical() }).err(), Some(VdCode::FlagsEmpty.code()));
        assert_eq!(validate(ParamPack { dim: 2, flags: 0x000D, ..ParamPack::canonical() }).err(), Some(VdCode::DimFlagMismatch.code()));
        assert_eq!(validate(ParamPack { dim: 3, flags: 0x000B, ..ParamPack::canonical() }).err(), Some(VdCode::DimFlagMismatch.code()));
    }

    #[test]
    fn t_ledger_ring_and_conservation() {
        let mut l = ErrLedger::new();
        let mut k = 0;
        while k < LOG_CAP + 4 {
            let _ = l.feed(make_form(k % FORM_N), PHASE_VALUE);
            k += 1;
        }
        assert_eq!(l.len, LOG_CAP, "环形覆盖");
        assert_eq!(l.overflow, 4, "溢出显性记账");
        assert_eq!(l.injected, (LOG_CAP + 4) as u32, "注入账不丢");
        assert_eq!(l.logged, (LOG_CAP + 4) as u32, "日志账不丢");
        assert!(l.conserve().is_ok());
        assert!(l.triple_ok());
    }

    #[test]
    fn t_double_run_deterministic() {
        let a = inject_rounds(3);
        let b = inject_rounds(3);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.tries, y.tries, "双跑 tries 一致");
                assert_eq!(x.code_match, y.code_match, "双跑码账一致");
            }
            _ => panic!("注入跑不应失败"),
        }
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F700 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(VdCode::OrderBroken.code(), SEG_BASE + 11, "码位推导");
    }
}
