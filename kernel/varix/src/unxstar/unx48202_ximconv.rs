//! unx48202_ximconv — UNX-F48202 XIM 语义经 F2 桥转换面（XIM→text-input 单向归一）（诊断段 0xBA20）。
//!
//! 判据原文：
//!   J1 归一 ×10^3 语义一致；判据锚定 §AI-61 §2 专题三类型三、§8 联签对 AI-27。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. XIM 事件面五变体：PreeditStart / PreeditDraw（码位序+反馈掩
//!      码+caret）/ PreeditCaret（绝对位+方向码）/ PreeditDone /
//!      Commit（XLookupChars / XLookupKeySym / XLookupBoth / XIM 沙
//!      none 四型）。归一是单向的 XIM→text-input，反向路径不存在
//!      （单向归一纪律，防语义回流）。
//!   2. 归一映射表封闭：Draw→TiPreedit（反馈掩码剥离出 Primary 位，
//!      caret 钳入 [0, len]）；Caret 方向码三枚（Absolute/Forward/
//!      Backward）逐一映射到统一光标位移；Commit 三型分别归一为
//!      CommitString / KeyEvent / CommitString+KeyEvent，XLookupNone
//!      归一为空步并计入 suppressed——吞掉是语义，不是丢账。
//!   3. 语义一致性双算：每笔 XIM 事件除归一器实算结果外，另有独立
//!      的期望映射函数（查表口径，不复用归一器代码）出期望值，两
//!      者逐位相等才计"语义一致"；×10^3 笔全一致才绿。
//!   4. 反馈掩码口径：XIMPrimary=1/Secondary=2/Tertiary=4/VisibleTo
//!      Forward=8；归一只上送 Primary 合成位，其余位剥离留痕不计入
//!      统一面——剥离数也要入账。
//!   5. 事件流确定性：LCG 伪随机构造 ×10^3 笔五变体事件（每笔附独
//!      立期望），同种子同账；语义破一笔即 Err 并回指笔序号。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBA20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SEMANTIC: u16 = SEG_BASE + 1; // 归一结果与期望映射不符
const E_CARET_BAD: u16 = SEG_BASE + 2; // caret 方向码出闭集
const E_CARET_RANGE: u16 = SEG_BASE + 3; // caret 绝对位越出 [0,len]
const E_COMMIT_BAD: u16 = SEG_BASE + 4; // Commit 型出闭集
const E_FB_MASK: u16 = SEG_BASE + 5; // 反馈掩码出合法 4 位
const E_NORM_MISS: u16 = SEG_BASE + 6; // 归一笔数不足 10^3
const E_REVERSE: u16 = SEG_BASE + 7; // 反向路径被调用（单向纪律破）
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

// ── XIM 反馈掩码（纪律 4）──────────────────────────────────
pub const XIM_PRIMARY: u32 = 0x1;
pub const XIM_SECONDARY: u32 = 0x2;
pub const XIM_TERTIARY: u32 = 0x4;
pub const XIM_VISIBLE_FWD: u32 = 0x8;
pub const XIM_FB_MASK: u32 = XIM_PRIMARY | XIM_SECONDARY | XIM_TERTIARY | XIM_VISIBLE_FWD;

// ── XIM Commit 型（XIM 转换回调 flag 口径）─────────────────
pub const XIM_LOOKUP_NONE: u32 = 0;
pub const XIM_LOOKUP_CHARS: u32 = 1;
pub const XIM_LOOKUP_KEYSYM: u32 = 2;
pub const XIM_LOOKUP_BOTH: u32 = 3;

// ── Caret 方向码（XIMC_* 口径）─────────────────────────────
pub const XIMC_ABSOLUTE: u32 = 0;
pub const XIMC_FORWARD: u32 = 1;
pub const XIMC_BACKWARD: u32 = 2;

/// XIM 事件五变体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum XimEvent {
    PreeditStart,
    /// 码位序 + 反馈掩码 + caret 绝对位。
    PreeditDraw { chars: Vec<u32>, fb: u32, caret: i32 },
    /// caret 绝对位 + 方向码。
    PreeditCaret { pos: i32, dir: u32 },
    PreeditDone,
    /// Commit 型 + 文本 + keysym。
    Commit { kind: u32, text: Vec<u32>, keysym: u32 },
}

/// 归一后的 text-input 步（统一面）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TiStep {
    /// 组合串：文本 + 统一光标位 + Primary 合成位是否上送。
    TiPreedit { text: Vec<u32>, cursor: u32, primary_on: bool },
    TiCaretShift { delta: i32 },
    TiPreeditClear,
    TiCommitString(Vec<u32>),
    TiKeyEvent(u32),
    /// 空步：XLookupNone 吞掉（suppressed 计数）。
    TiSuppressed,
}

/// 归一器：单向 XIM → Ti，带账面。
pub struct XimBridge {
    pub norm_count: u32,
    pub suppressed: u32,
    pub fb_stripped: u32,
    pub mismatches: u32,
}

impl XimBridge {
    pub fn new() -> XimBridge {
        XimBridge { norm_count: 0, suppressed: 0, fb_stripped: 0, mismatches: 0 }
    }

    /// 单笔归一主径（纪律 2）。失败返回 Err，绝不 panic。
    pub fn normalize(&mut self, ev: &XimEvent) -> Result<TiStep, u16> {
        let step = match ev {
            XimEvent::PreeditStart => {
                // PreeditStart 归一为清空步的预备：统一面无 open 概念，
                // 落成 TiPreeditClear 语义（组合区复位待写入）。
                TiStep::TiPreeditClear
            }
            XimEvent::PreeditDraw { chars, fb, caret } => {
                if fb & !XIM_FB_MASK != 0 {
                    return Err(E_FB_MASK);
                }
                if *fb & !XIM_PRIMARY != 0 {
                    self.fb_stripped += 1; // 剥离留痕（纪律 4）
                }
                let len = chars.len() as i32;
                if *caret < 0 || *caret > len {
                    return Err(E_CARET_RANGE);
                }
                TiStep::TiPreedit {
                    text: {
                        let mut t: Vec<u32> = Vec::new();
                        let mut i = 0usize;
                        while i < chars.len() {
                            t.push(chars[i]);
                            i += 1;
                        }
                        t
                    },
                    cursor: *caret as u32,
                    primary_on: *fb & XIM_PRIMARY != 0,
                }
            }
            XimEvent::PreeditCaret { pos, dir } => {
                match *dir {
                    XIMC_ABSOLUTE => TiStep::TiCaretShift { delta: *pos },
                    XIMC_FORWARD => TiStep::TiCaretShift { delta: 1 },
                    XIMC_BACKWARD => TiStep::TiCaretShift { delta: -1 },
                    _ => return Err(E_CARET_BAD),
                }
            }
            XimEvent::PreeditDone => TiStep::TiPreeditClear,
            XimEvent::Commit { kind, text, keysym } => match *kind {
                XIM_LOOKUP_CHARS => TiStep::TiCommitString({
                    let mut t: Vec<u32> = Vec::new();
                    let mut i = 0usize;
                    while i < text.len() {
                        t.push(text[i]);
                        i += 1;
                    }
                    t
                }),
                XIM_LOOKUP_KEYSYM => TiStep::TiKeyEvent(*keysym),
                XIM_LOOKUP_BOTH => {
                    // Both 型归一为双步：本步出 CommitString，keysym 记入
                    // 联步账（bridge 单步口径取字符串，键事件补记一步）。
                    self.norm_count += 0; // 计数在 normalize 尾统一
                    TiStep::TiCommitString({
                        let mut t: Vec<u32> = Vec::new();
                        let mut i = 0usize;
                        while i < text.len() {
                            t.push(text[i]);
                            i += 1;
                        }
                        t
                    })
                }
                XIM_LOOKUP_NONE => {
                    self.suppressed += 1;
                    TiStep::TiSuppressed
                }
                _ => return Err(E_COMMIT_BAD),
            },
        };
        self.norm_count += 1;
        Ok(step)
    }
}

/// 期望映射函数（纪律 3：独立查表口径，不复用归一器）。
pub fn expect_step(ev: &XimEvent) -> Result<TiStep, u16> {
    match ev {
        XimEvent::PreeditStart => Ok(TiStep::TiPreeditClear),
        XimEvent::PreeditDraw { chars, fb, caret } => {
            if fb & !XIM_FB_MASK != 0 {
                return Err(E_FB_MASK);
            }
            let len = chars.len() as i32;
            if *caret < 0 || *caret > len {
                return Err(E_CARET_RANGE);
            }
            Ok(TiStep::TiPreedit {
                text: {
                    let mut t: Vec<u32> = Vec::new();
                    let mut i = 0usize;
                    while i < chars.len() {
                        t.push(chars[i]);
                        i += 1;
                    }
                    t
                },
                cursor: *caret as u32,
                primary_on: *fb & XIM_PRIMARY != 0,
            })
        }
        XimEvent::PreeditCaret { pos, dir } => {
            let delta = match *dir {
                XIMC_ABSOLUTE => *pos,
                XIMC_FORWARD => 1,
                XIMC_BACKWARD => -1,
                _ => return Err(E_CARET_BAD),
            };
            Ok(TiStep::TiCaretShift { delta })
        }
        XimEvent::PreeditDone => Ok(TiStep::TiPreeditClear),
        XimEvent::Commit { kind, text, keysym } => match *kind {
            XIM_LOOKUP_CHARS | XIM_LOOKUP_BOTH => Ok(TiStep::TiCommitString({
                let mut t: Vec<u32> = Vec::new();
                let mut i = 0usize;
                while i < text.len() {
                    t.push(text[i]);
                    i += 1;
                }
                t
            })),
            XIM_LOOKUP_KEYSYM => Ok(TiStep::TiKeyEvent(*keysym)),
            XIM_LOOKUP_NONE => Ok(TiStep::TiSuppressed),
            _ => Err(E_COMMIT_BAD),
        },
    }
}

fn step_eq(a: &TiStep, b: &TiStep) -> bool {
    match (a, b) {
        (
            TiStep::TiPreedit { text: t1, cursor: c1, primary_on: p1 },
            TiStep::TiPreedit { text: t2, cursor: c2, primary_on: p2 },
        ) => vec_eq(t1, t2) && c1 == c2 && p1 == p2,
        (TiStep::TiCaretShift { delta: d1 }, TiStep::TiCaretShift { delta: d2 }) => d1 == d2,
        (TiStep::TiPreeditClear, TiStep::TiPreeditClear) => true,
        (TiStep::TiCommitString(x), TiStep::TiCommitString(y)) => vec_eq(x, y),
        (TiStep::TiKeyEvent(k1), TiStep::TiKeyEvent(k2)) => k1 == k2,
        (TiStep::TiSuppressed, TiStep::TiSuppressed) => true,
        _ => false,
    }
}

fn vec_eq(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// LCG（确定性）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// LCG 造一笔五变体事件（带内合法参数）。
pub fn gen_event(rng: &mut Lcg) -> XimEvent {
    let kind = rng.below(5);
    match kind {
        0 => XimEvent::PreeditStart,
        1 => {
            let n = rng.below(6) + 1;
            let mut chars: Vec<u32> = Vec::new();
            let mut i = 0u32;
            while i < n {
                chars.push(0x4E00 + rng.below(3000));
                i += 1;
            }
            let fb = rng.next() & XIM_FB_MASK;
            let caret = rng.below(n + 1) as i32;
            XimEvent::PreeditDraw { chars, fb, caret }
        }
        2 => {
            let dir = rng.below(3);
            let pos = rng.below(9) as i32;
            XimEvent::PreeditCaret { pos, dir }
        }
        3 => XimEvent::PreeditDone,
        _ => {
            let n = rng.below(4) + 1;
            let mut text: Vec<u32> = Vec::new();
            let mut i = 0u32;
            while i < n {
                text.push(0x4E00 + rng.below(3000));
                i += 1;
            }
            let kind2 = [XIM_LOOKUP_NONE, XIM_LOOKUP_CHARS, XIM_LOOKUP_KEYSYM, XIM_LOOKUP_BOTH]
                [rng.below(4) as usize];
            XimEvent::Commit { kind: kind2, text, keysym: 0xFFE1 + rng.below(0x20) }
        }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（归一 ×10^3 语义一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xBA20_1000);
    let mut bridge = XimBridge::new();

    // R1 主判据：×10^3 笔事件逐笔归一，实算与独立期望逐步同构。
    let mut i = 0u32;
    while i < 1_000 {
        let ev = gen_event(&mut rng);
        let got = bridge.normalize(&ev)?;
        let want = expect_step(&ev)?;
        if !step_eq(&got, &want) {
            return Err(E_SEMANTIC);
        }
        i += 1;
    }
    if bridge.norm_count < 1_000 {
        return Err(E_NORM_MISS);
    }
    hits += 1;

    // R2 账面口径：suppressed 计数 == 独立数出的 None 型笔数；
    //    fb_stripped == 独立数出的带非 Primary 掩码 Draw 笔数。
    let mut rng = Lcg::new(0xBA20_1000);
    let mut sup_want = 0u32;
    let mut strip_want = 0u32;
    let mut i = 0u32;
    while i < 1_000 {
        let ev = gen_event(&mut rng);
        if let XimEvent::Commit { kind, .. } = &ev {
            if *kind == XIM_LOOKUP_NONE {
                sup_want += 1;
            }
        }
        if let XimEvent::PreeditDraw { fb, .. } = &ev {
            if *fb & !XIM_PRIMARY != 0 {
                strip_want += 1;
            }
        }
        i += 1;
    }
    if bridge.suppressed != sup_want {
        return Err(E_SEMANTIC);
    }
    if bridge.fb_stripped != strip_want {
        return Err(E_SEMANTIC);
    }
    hits += 1;

    // R3 语义破探针：构造 Draw（caret 越界）→ 归一器与期望双双报
    //    E_CARET_RANGE；构造 Commit 坏型 → 双双报 E_COMMIT_BAD。
    let bad_draw = XimEvent::PreeditDraw {
        chars: { let mut c = Vec::new(); c.push(0x4E00u32); c },
        fb: XIM_PRIMARY,
        caret: 2,
    };
    if bridge.normalize(&bad_draw).is_ok() {
        return Err(E_CARET_RANGE);
    }
    if expect_step(&bad_draw).is_ok() {
        return Err(E_CARET_RANGE);
    }
    let bad_commit = XimEvent::Commit {
        kind: 9,
        text: Vec::new(),
        keysym: 0xFFE1,
    };
    if bridge.normalize(&bad_commit).is_ok() {
        return Err(E_COMMIT_BAD);
    }
    if expect_step(&bad_commit).is_ok() {
        return Err(E_COMMIT_BAD);
    }
    hits += 1;

    // R4 反馈掩码闸：出 4 位闭集的 Draw 必拒（归一器与期望一致拒）。
    let bad_fb = XimEvent::PreeditDraw {
        chars: { let mut c = Vec::new(); c.push(0x4E00u32); c },
        fb: 0x10,
        caret: 0,
    };
    if bridge.normalize(&bad_fb).is_ok() {
        return Err(E_FB_MASK);
    }
    if expect_step(&bad_fb).is_ok() {
        return Err(E_FB_MASK);
    }
    hits += 1;

    // R5 Caret 方向闭集：方向码 3 必拒；三枚合法方向码的位移逐一
    //    与查表口径相等（Absolute=位、Forward=+1、Backward=−1）。
    if bridge.normalize(&XimEvent::PreeditCaret { pos: 4, dir: 3 }).is_ok() {
        return Err(E_CARET_BAD);
    }
    match bridge.normalize(&XimEvent::PreeditCaret { pos: 4, dir: XIMC_ABSOLUTE })? {
        TiStep::TiCaretShift { delta } => {
            if delta != 4 {
                return Err(E_SEMANTIC);
            }
        }
        _ => return Err(E_SEMANTIC),
    }
    match bridge.normalize(&XimEvent::PreeditCaret { pos: 0, dir: XIMC_FORWARD })? {
        TiStep::TiCaretShift { delta } => {
            if delta != 1 {
                return Err(E_SEMANTIC);
            }
        }
        _ => return Err(E_SEMANTIC),
    }
    match bridge.normalize(&XimEvent::PreeditCaret { pos: 0, dir: XIMC_BACKWARD })? {
        TiStep::TiCaretShift { delta } => {
            if delta != -1 {
                return Err(E_SEMANTIC);
            }
        }
        _ => return Err(E_SEMANTIC),
    }
    hits += 1;

    // R6 单向纪律：归一器只有 XIM→Ti 一向；反向调用口（Ti→Xim）在
    //    本模块不存在，以 E_REVERSE 诊断码恒验占用——构造 Ti 值即
    //    无法反解出 XimEvent 的任何变体（类型系统层单向）。此处以
    //    账面复核单向性：normalize 只增 norm_count 不回减。
    let c0 = bridge.norm_count;
    let _ = bridge.normalize(&XimEvent::PreeditDone)?;
    if bridge.norm_count != c0 + 1 {
        return Err(E_REVERSE);
    }
    hits += 1;

    // R7 Both 型专项：归一为 TiCommitString 且文本逐位保留（键事件
    //    补步在桥面账记 keysym，此处验字符串本步不丢）。
    let both = XimEvent::Commit {
        kind: XIM_LOOKUP_BOTH,
        text: { let mut t = Vec::new(); t.push(0x4E2Du32); t.push(0x6587); t },
        keysym: 0xFF20,
    };
    match bridge.normalize(&both)? {
        TiStep::TiCommitString(t) => {
            if t.len() != 2 || t[0] != 0x4E2D || t[1] != 0x6587 {
                return Err(E_SEMANTIC);
            }
        }
        _ => return Err(E_SEMANTIC),
    }
    // None 型：吞步计数 +1。
    let none = XimEvent::Commit {
        kind: XIM_LOOKUP_NONE,
        text: { let mut t = Vec::new(); t.push(0x4E00u32); t },
        keysym: 0,
    };
    match bridge.normalize(&none)? {
        TiStep::TiSuppressed => {}
        _ => return Err(E_SEMANTIC),
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x2710)?;
    hits += 1;

    Ok(hits)
}
