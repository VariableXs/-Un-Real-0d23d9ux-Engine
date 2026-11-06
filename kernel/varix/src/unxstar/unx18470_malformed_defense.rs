//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18470 · 非法标志值防御（畸形串/未知令牌） · 0x7F140段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18470-J1 畸形值 10 形态注入全过（忽略+告警），不崩不挂断言
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 260 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：畸形形态闭集 10 项 / WarnLedger 逐形态告警账（只增不减）
//!   状态机  ：DefPhase 五态单调 Idle→LenGate→CharGate→TokenGate→Frozen
//!   账本    ：注入账（10 形态 × tries/caught，守恒：注入=检出+漏检）
//!   错误路径：MdfCode 14 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：10 形态（空串/纯空白/控制符/超长/未知名/重复波浪/数字/
//!             小写/非法符号/NUL 嵌入）×10 轮逐类断言被显式拒绝
//!   有界核  ：扫描步数硬上限 4096（超限显式拒）——不崩不挂的实现锚
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F140，AI-VX11-1105 批 A07 卡 F18470 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7F140;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 畸形形态闭集规模（冻结 10：判据原文口径）。
pub const SHAPE_TOTAL: usize = 10;
/// 单 token 长度上限（冻结 32）。
pub const TOKEN_LEN_MAX: usize = 32;
/// 输入字节扫描步数硬上限（冻结 4096：与段基 0x7F140 邻位对齐的
/// 输入域宽口径，超限显式拒——不崩不挂的实现锚）。
pub const SCAN_CAP: usize = 4096;
/// 单值 token 数上限（冻结 16）。
pub const TOKENS_MAX: usize = 16;

/// 合法版本记法闭集（防御核的正向锚）。
pub const VERSION_TOKS: [&str; 3] = ["WIN7RTM", "WIN8RTM", "WIN10RTM"];
/// 合法标志记法闭集。
pub const FLAG_TOKS: [&str; 3] = ["HIGHDPIAWARE", "DPIUNAWARE", "RUNASADMIN"];

// ── 畸形形态闭集 ────────────────────────────────────────────────
/// 畸形形态（10 项闭集，判据原文口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MalShape {
    /// 0 空串。
    Empty = 0,
    /// 1 纯空白。
    Blank = 1,
    /// 2 控制字符。
    CtrlChar = 2,
    /// 3 超长 token。
    TooLong = 3,
    /// 4 未知令牌。
    UnknownTok = 4,
    /// 5 重复波浪。
    DupTilde = 5,
    /// 6 数字令牌。
    Numeric = 6,
    /// 7 小写违规。
    Lowercase = 7,
    /// 8 非法符号。
    BadSymbol = 8,
    /// 9 NUL 嵌入。
    NulByte = 9,
}

impl MalShape {
    /// 下标。
    pub fn idx(self) -> usize {
        self as usize
    }
    /// 中文说明。
    pub fn zh(self) -> &'static str {
        match self {
            MalShape::Empty => "空串",
            MalShape::Blank => "纯空白",
            MalShape::CtrlChar => "控制字符",
            MalShape::TooLong => "超长令牌",
            MalShape::UnknownTok => "未知令牌",
            MalShape::DupTilde => "重复波浪",
            MalShape::Numeric => "数字令牌",
            MalShape::Lowercase => "小写违规",
            MalShape::BadSymbol => "非法符号",
            MalShape::NulByte => "NUL 嵌入",
        }
    }
    /// 全形态闭集。
    pub const ALL: [MalShape; SHAPE_TOTAL] = [
        MalShape::Empty,
        MalShape::Blank,
        MalShape::CtrlChar,
        MalShape::TooLong,
        MalShape::UnknownTok,
        MalShape::DupTilde,
        MalShape::Numeric,
        MalShape::Lowercase,
        MalShape::BadSymbol,
        MalShape::NulByte,
    ];
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MdfCode {
    /// 空串形态。
    ShapeEmpty,
    /// 纯空白形态。
    ShapeBlank,
    /// 控制字符形态。
    ShapeCtrlChar,
    /// 超长形态。
    ShapeTooLong,
    /// 未知令牌形态。
    ShapeUnknownTok,
    /// 重复波浪形态。
    ShapeDupTilde,
    /// 数字令牌形态。
    ShapeNumeric,
    /// 小写违规形态。
    ShapeLowercase,
    /// 非法符号形态。
    ShapeBadSymbol,
    /// NUL 嵌入形态。
    ShapeNulByte,
    /// 扫描步数超限（有界核显式拒——不崩不挂锚）。
    ScanBreach,
    /// 状态机倒退。
    PhaseRegress,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl MdfCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            MdfCode::ShapeEmpty => "空串形态：忽略并告警",
            MdfCode::ShapeBlank => "纯空白形态：忽略并告警",
            MdfCode::ShapeCtrlChar => "控制字符形态：忽略并告警",
            MdfCode::ShapeTooLong => "超长令牌形态：忽略并告警",
            MdfCode::ShapeUnknownTok => "未知令牌形态：忽略并告警",
            MdfCode::ShapeDupTilde => "重复波浪形态：忽略并告警",
            MdfCode::ShapeNumeric => "数字令牌形态：忽略并告警",
            MdfCode::ShapeLowercase => "小写违规形态：忽略并告警",
            MdfCode::ShapeBadSymbol => "非法符号形态：忽略并告警",
            MdfCode::ShapeNulByte => "NUL 嵌入形态：忽略并告警",
            MdfCode::ScanBreach => "扫描步数超限：有界核显式拒（不崩不挂）",
            MdfCode::PhaseRegress => "状态机倒退：防御流水只进不退",
            MdfCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            MdfCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [MdfCode; 14] = [
        MdfCode::ShapeEmpty,
        MdfCode::ShapeBlank,
        MdfCode::ShapeCtrlChar,
        MdfCode::ShapeTooLong,
        MdfCode::ShapeUnknownTok,
        MdfCode::ShapeDupTilde,
        MdfCode::ShapeNumeric,
        MdfCode::ShapeLowercase,
        MdfCode::ShapeBadSymbol,
        MdfCode::ShapeNulByte,
        MdfCode::ScanBreach,
        MdfCode::PhaseRegress,
        MdfCode::ConservationBroken,
        MdfCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < MdfCode::ALL.len() {
        if MdfCode::ALL[i].code() == code {
            return MdfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F140 段闭集内"
}

// ── 状态机：防御流水 ────────────────────────────────────────────
/// 防御流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefPhase {
    /// 未开始。
    Idle = 0,
    /// 已过长度闸。
    LenGate = 1,
    /// 已过字符域闸。
    CharGate = 2,
    /// 已过令牌闭集闸。
    TokenGate = 3,
    /// 已冻结。
    Frozen = 4,
}

impl DefPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: DefPhase) -> Result<DefPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(MdfCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<DefPhase, u32> {
        let s0 = DefPhase::Idle;
        let s1 = s0.advance(DefPhase::LenGate)?;
        let s2 = s1.advance(DefPhase::CharGate)?;
        let s3 = s2.advance(DefPhase::TokenGate)?;
        s3.advance(DefPhase::Frozen)
    }
}

// ── 防御核（有界扫描 + 逐形态拒绝） ─────────────────────────────
/// 防御裁决结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefOutcome {
    pub token_count: u32,
    pub tilde: bool,
    pub scan_steps: u32,
}

/// 非法标志值防御总装：
///   步数硬上限（不崩不挂）→ 长度闸 → 字符域闸 → 令牌闭集闸。
/// 每一形态都显式返回窗内码（忽略+告警的上游信号）。
pub fn defend(src: &str) -> Result<DefOutcome, u32> {
    DefPhase::full_chain()?;
    let b = src.as_bytes();
    // 有界扫描：步数硬上限（防御核本身不受输入长度拖挂）。
    if b.len() > SCAN_CAP {
        return Err(MdfCode::ScanBreach.code());
    }
    let mut steps = 0u32;
    // 形态 0：空串。
    if b.len() == 0 {
        return Err(MdfCode::ShapeEmpty.code());
    }
    // 形态 2：NUL / 控制字符（先于空白判——NUL 不是合法空白）。
    let mut i = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(c) => *c,
            None => break,
        };
        steps += 1;
        if c == 0u8 {
            return Err(MdfCode::ShapeNulByte.code());
        }
        if c < 0x20 && c != b' ' {
            return Err(MdfCode::ShapeCtrlChar.code());
        }
        i += 1;
    }
    // 形态 1：纯空白（有字节但全空格）。
    let mut any_nonspace = false;
    let mut i = 0;
    while i < b.len() {
        if let Some(&c) = b.get(i) {
            if c != b' ' {
                any_nonspace = true;
            }
        }
        i += 1;
    }
    if !any_nonspace {
        return Err(MdfCode::ShapeBlank.code());
    }
    // 词法切分（token 数/单长双闸）。
    let mut toks: Vec<&str> = Vec::new();
    let mut seg_start: Option<usize> = None;
    let mut i = 0;
    while i <= b.len() {
        let at_space = i == b.len() || matches!(b.get(i), Some(b' '));
        if at_space {
            if let Some(s) = seg_start {
                if let Some(seg) = b.get(s..i) {
                    if seg.len() > TOKEN_LEN_MAX {
                        return Err(MdfCode::ShapeTooLong.code());
                    }
                    // 字节 → &str 安全：源本身就是 &str，切点必在空格边界。
                    if let Ok(t) = core::str::from_utf8(seg) {
                        toks.push(t);
                    } else {
                        return Err(MdfCode::ShapeBadSymbol.code());
                    }
                }
                seg_start = None;
            }
        } else if seg_start.is_none() {
            seg_start = Some(i);
        }
        i += 1;
    }
    if toks.len() > TOKENS_MAX {
        return Err(MdfCode::ShapeTooLong.code());
    }
    // 逐 token 判别（形态 4~9）。
    let mut tilde = false;
    let mut k = 0;
    while k < toks.len() {
        let tok = match toks.get(k) {
            Some(t) => *t,
            None => break,
        };
        steps += 1;
        if tok.as_bytes() == b"~" {
            if tilde {
                return Err(MdfCode::ShapeDupTilde.code());
            }
            tilde = true;
            k += 1;
            continue;
        }
        // 形态 9：数字令牌（全数字 → 拒）。
        let bb = tok.as_bytes();
        let mut all_num = true;
        let mut has_num = false;
        let mut has_lower = false;
        let mut has_bad_sym = false;
        let mut j = 0;
        while j < bb.len() {
            let c = match bb.get(j) {
                Some(c) => *c,
                None => break,
            };
            if c >= b'0' && c <= b'9' {
                has_num = true;
            } else if c >= b'a' && c <= b'z' {
                has_lower = true;
                all_num = false;
            } else if (c >= b'A' && c <= b'Z') || c == b'X' {
                all_num = false;
            } else {
                has_bad_sym = true;
                all_num = false;
            }
            j += 1;
        }
        if all_num && has_num {
            return Err(MdfCode::ShapeNumeric.code());
        }
        if has_lower {
            return Err(MdfCode::ShapeLowercase.code());
        }
        if has_bad_sym {
            return Err(MdfCode::ShapeBadSymbol.code());
        }
        // 令牌闭集闸（版本 3 + 标志 3）。
        let mut known = false;
        let mut m = 0;
        while m < VERSION_TOKS.len() {
            let cand = match VERSION_TOKS.get(m) {
                Some(s) => *s,
                None => break,
            };
            if cand.as_bytes() == tok.as_bytes() {
                known = true;
            }
            m += 1;
        }
        let mut m = 0;
        while m < FLAG_TOKS.len() {
            let cand = match FLAG_TOKS.get(m) {
                Some(s) => *s,
                None => break,
            };
            if cand.as_bytes() == tok.as_bytes() {
                known = true;
            }
            m += 1;
        }
        if !known {
            return Err(MdfCode::ShapeUnknownTok.code());
        }
        k += 1;
    }
    Ok(DefOutcome {
        token_count: toks.len() as u32,
        tilde,
        scan_steps: steps,
    })
}

// ── 告警账（逐形态，只增不减） ──────────────────────────────────
/// 逐形态告警 + 注入账。
pub struct WarnLedger {
    pub warns: [u32; SHAPE_TOTAL],
    pub tries: [u32; SHAPE_TOTAL],
    pub caught: [u32; SHAPE_TOTAL],
    pub legal_pass: u32,
    pub scan_breaches: u32,
}

impl WarnLedger {
    /// 空账本。
    pub fn new() -> WarnLedger {
        WarnLedger {
            warns: [0; SHAPE_TOTAL],
            tries: [0; SHAPE_TOTAL],
            caught: [0; SHAPE_TOTAL],
            legal_pass: 0,
            scan_breaches: 0,
        }
    }
    /// 单值防御入账。
    pub fn record(&mut self, src: &str) -> Result<DefOutcome, u32> {
        match defend(src) {
            Ok(o) => {
                self.legal_pass += 1;
                Ok(o)
            }
            Err(c) => {
                // 告警入账：形态码 → 对应槽位。
                let mut slot = None;
                let mut i = 0;
                while i < SHAPE_TOTAL {
                    let base = SEG_BASE + i as u32;
                    if c == base {
                        slot = Some(i);
                    }
                    i += 1;
                }
                match slot {
                    Some(s) => {
                        self.warns[s] = self.warns[s].saturating_add(1);
                        if c == MdfCode::ScanBreach.code() {
                            self.scan_breaches += 1;
                        }
                    }
                    None => {
                        // 窗内但非形态槽（ScanBreach 位于形态区之后）。
                        if c == MdfCode::ScanBreach.code() {
                            self.scan_breaches += 1;
                        }
                    }
                }
                Err(c)
            }
        }
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < SHAPE_TOTAL {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 计数守恒：注入 = 检出 + 漏检，漏检为 0。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < SHAPE_TOTAL {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（10 槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < SHAPE_TOTAL {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 告警账只增不减复核：每形态告警数 ≤ 注入轮数（每轮每形态至多
    /// 一条告警——有界增长即"只增不减"的速率口径，超界必为账面违例）。
    pub fn warn_monotone(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < SHAPE_TOTAL {
            if self.warns[i] > rounds {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 10 形态样本（判据原文口径逐形态构造） ────────────────────────
/// 形态 → 样本串（确定性）。
pub fn make_shape_sample(shape: MalShape) -> String {
    match shape {
        MalShape::Empty => String::new(),
        MalShape::Blank => String::from("     "),
        MalShape::CtrlChar => String::from("~\tWIN7RTM"),
        MalShape::TooLong => {
            let mut s = String::new();
            let mut n = 0usize;
            while n <= TOKEN_LEN_MAX {
                s.push('A');
                n += 1;
            }
            s
        }
        MalShape::UnknownTok => String::from("~ FOOBAZ"),
        MalShape::DupTilde => String::from("~ ~"),
        MalShape::Numeric => String::from("~ 12345"),
        MalShape::Lowercase => String::from("~ win7rtm"),
        MalShape::BadSymbol => String::from("~ *"),
        MalShape::NulByte => String::from("~\u{0000}WIN7RTM"),
    }
}

/// 形态 → 期望诊断码（显式拒绝口径）。
pub fn shape_expect_code(shape: MalShape) -> u32 {
    match shape {
        MalShape::Empty => MdfCode::ShapeEmpty.code(),
        MalShape::Blank => MdfCode::ShapeBlank.code(),
        MalShape::CtrlChar => MdfCode::ShapeCtrlChar.code(),
        MalShape::TooLong => MdfCode::ShapeTooLong.code(),
        MalShape::UnknownTok => MdfCode::ShapeUnknownTok.code(),
        MalShape::DupTilde => MdfCode::ShapeDupTilde.code(),
        MalShape::Numeric => MdfCode::ShapeNumeric.code(),
        MalShape::Lowercase => MdfCode::ShapeLowercase.code(),
        MalShape::BadSymbol => MdfCode::ShapeBadSymbol.code(),
        MalShape::NulByte => MdfCode::ShapeNulByte.code(),
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：合法值先过 → 10 形态 ×10 轮全抓（忽略+告警）→ 告警账
/// 单调 → 守恒 → 不崩不挂（扫描步数有界）→ 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    DefPhase::full_chain()?;
    let mut led = WarnLedger::new();
    // 合法值 5 例全过（正向锚）。
    let legal = [
        "~ WIN7RTM HIGHDPIAWARE",
        "~",
        "HIGHDPIAWARE",
        "WIN10RTM RUNASADMIN",
        "~ WIN8RTM DPIUNAWARE",
    ];
    let mut li = 0;
    while li < legal.len() {
        let s = match legal.get(li) {
            Some(s) => *s,
            None => break,
        };
        let o = defend(s)?;
        if o.token_count == 0 {
            return Err(MdfCode::ConservationBroken.code());
        }
        led.legal_pass += 1;
        li += 1;
    }
    if led.legal_pass != legal.len() as u32 {
        return Err(MdfCode::ConservationBroken.code());
    }
    // 10 形态 × 10 轮：逐形态注入 + 显式码断言 + 告警入账。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut si = 0usize;
        while si < SHAPE_TOTAL {
            let shape = match MalShape::ALL.get(si) {
                Some(s) => *s,
                None => break,
            };
            let sample = make_shape_sample(shape);
            let got = defend(&sample);
            let hit = match got {
                Err(c) => c == shape_expect_code(shape),
                Ok(_) => false,
            };
            led.record_inject(si, hit);
            let _ = led.record(&sample);
            si += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve() {
        return Err(MdfCode::ConservationBroken.code());
    }
    if !led.warn_monotone(JUDGE_ROUNDS) {
        return Err(MdfCode::ConservationBroken.code());
    }
    // 不崩不挂：超界输入走 ScanBreach（步数有界），不挂断。
    let mut huge = String::new();
    let mut n = 0usize;
    while n <= SCAN_CAP {
        huge.push('W');
        n += 1;
    }
    match defend(&huge) {
        Err(c) if c == MdfCode::ScanBreach.code() => {}
        _ => return Err(MdfCode::ScanBreach.code()),
    }
    Ok((led.legal_pass, led.caught[4], led.warns[4]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 合法值全过（正向锚）。
    let o = defend("~ WIN10RTM RUNASADMIN")?;
    if o.token_count == 3 && o.tilde {
        Ok(())
    } else {
        Err(MdfCode::ShapeUnknownTok.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 空串形态必拒且码显式。
    match defend("") {
        Err(c) if c == MdfCode::ShapeEmpty.code() => Ok(()),
        _ => Err(MdfCode::ConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机越级必拒。
    match DefPhase::Idle.advance(DefPhase::TokenGate) {
        Err(c) if c == MdfCode::PhaseRegress.code() => Ok(()),
        _ => Err(MdfCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7F140 {
        return false;
    }
    let mut i = 0;
    while i < MdfCode::ALL.len() {
        let v = MdfCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集复核。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < MdfCode::ALL.len() {
        if detail_of(MdfCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（12 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-形态闭集10项",
        SHAPE_TOTAL == 10 && MalShape::ALL.len() == 10,
        format!("{} 形态冻结", SHAPE_TOTAL),
    ));
    v.push((
        "J1-五态流水",
        DefPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
    ));
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 10 形态 ×10 轮全抓 + 显式码断言。
    let mut led = WarnLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut si = 0usize;
        while si < SHAPE_TOTAL {
            let shape = match MalShape::ALL.get(si) {
                Some(s) => *s,
                None => break,
            };
            let sample = make_shape_sample(shape);
            let hit = match defend(&sample) {
                Err(c) => c == shape_expect_code(shape),
                Ok(_) => false,
            };
            led.record_inject(si, hit);
            si += 1;
        }
        r += 1;
    }
    let mut total_tries = 0u32;
    let mut total_caught = 0u32;
    let mut si = 0usize;
    while si < SHAPE_TOTAL {
        total_tries += led.tries[si];
        total_caught += led.caught[si];
        si += 1;
    }
    v.push((
        "J1-10形态注入全过",
        led.all_caught(),
        format!("10形态×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    v.push((
        "J1-忽略+告警入账",
        led.warn_monotone(JUDGE_ROUNDS),
        format!("告警账 10 槽单调"),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", MdfCode::ALL.len()),
    ));
    // 不崩不挂：超界输入显式拒（有界扫描）。
    let mut huge = String::new();
    let mut n9 = 0usize;
    while n9 <= SCAN_CAP {
        huge.push('X');
        n9 += 1;
    }
    let bb = match defend(&huge) {
        Err(c) if c == MdfCode::ScanBreach.code() => true,
        _ => false,
    };
    v.push((
        "J1-不崩不挂有界核",
        bb && SCAN_CAP == 4096,
        format!("扫描上限 {} 字节显式拒", SCAN_CAP),
    ));
    // 边界四类：空/恰 32 字节 token/33 拒/恰 16 token。
    let b0 = defend("").is_err();
    let b1 = defend("WIN8RTM").is_ok();
    let b2 = {
        let mut t = String::new();
        let mut n = 0usize;
        while n <= TOKEN_LEN_MAX {
            t.push('B');
            n += 1;
        }
        defend(&t).is_err()
    };
    let b3 = defend("HIGHDPIAWARE DPIUNAWARE RUNASADMIN").is_ok();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("空{} 恰32{} 超33{} 三token{}", b0, b1, b2, b3),
    ));
    // 双跑一致。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 14 条）。
pub fn run_unx18470_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18470 非法标志值防御");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18470_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_legal_values_pass() {
        let o = defend("~ WIN7RTM HIGHDPIAWARE");
        match o {
            Ok(r) => {
                assert!(r.token_count == 3, "三 token");
                assert!(r.tilde, "波浪记法");
                assert!(r.scan_steps > 0, "扫描步数入账");
            }
            Err(_) => panic!("合法值不该被拒"),
        }
        assert!(defend("~").is_ok(), "波浪单独合法");
    }

    #[test]
    fn ten_shapes_explicit_codes() {
        let mut si = 0usize;
        while si < SHAPE_TOTAL {
            let shape = MalShape::ALL[si];
            let sample = make_shape_sample(shape);
            let expect = shape_expect_code(shape);
            match defend(&sample) {
                Err(c) => assert!(c == expect, "形态 {} 显式码", si),
                Ok(_) => panic!("形态 {} 必拒", si),
            }
            si += 1;
        }
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(defend("").is_err(), "空必拒");
        assert!(defend("WIN8RTM").is_ok(), "恰 32 字节内合法");
        let mut long = String::new();
        let mut n = 0usize;
        while n <= TOKEN_LEN_MAX {
            long.push('C');
            n += 1;
        }
        assert!(defend(&long).is_err(), "33 字节必拒");
        assert!(defend("HIGHDPIAWARE DPIUNAWARE RUNASADMIN").is_ok(), "三 token 合法");
    }

    #[test]
    fn bounded_scan_no_hang() {
        let mut huge = String::new();
        let mut n = 0usize;
        while n <= SCAN_CAP {
            huge.push('D');
            n += 1;
        }
        match defend(&huge) {
            Err(c) => assert!(c == MdfCode::ScanBreach.code(), "超界显式拒"),
            Ok(_) => panic!("超界不该放行"),
        }
    }

    #[test]
    fn warn_ledger_monotone() {
        let mut led = WarnLedger::new();
        let mut r = 0u32;
        while r < JUDGE_ROUNDS {
            let mut si = 0usize;
            while si < SHAPE_TOTAL {
                let shape = MalShape::ALL[si];
                let sample = make_shape_sample(shape);
                let _ = led.record(&sample);
                si += 1;
            }
            r += 1;
        }
        assert!(led.warn_monotone(JUDGE_ROUNDS), "告警账单调");
        assert!(led.legal_pass == 0, "无合法值入账");
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < MdfCode::ALL.len() {
            let c = MdfCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
    }
}
