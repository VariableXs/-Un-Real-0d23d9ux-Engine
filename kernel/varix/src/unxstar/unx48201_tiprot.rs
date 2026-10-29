//! unx48201_tiprot — UNX-F48201 text-input-v2/v3 双协议兼容层总成（诊断段 0xBA00）。
//!
//! 判据原文：
//!   J1 双协议 ×10 转换保真、版本协商正确；判据锚定 §AI-61 §2 专题三类型三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 内核内只建模"统一事件"（UnifiedEvent 七变体：Enable/Disable/
//!      Commit/Preedit/ContentType/CursorRect/PanelHint）；两协议各
//!      有独立线格式——v2 opcode 0x10 起、事件先发 opcode 后发参数
//!      数组；v3 opcode 0x30 起、参数打包进单 u32 载荷带小标头。
//!      转换器三段式：Unified → 协议帧 → Unified 回解，回解结果与
//!      原事件同构（same_event 全变体逐一比对）即"转换保真"。
//!   2. 版本协商账：manager 支持掩码（bit0=V2, bit1=V3）∩ client
//!      请求掩码 = 公共集；取最高版（V3 优先）。未知位、零交集、
//!      请求未宣告版本三条拒收路径独立成码。
//!   3. Preedit 事件带文本载荷（u32 码位序）+ 光标位；两协议编码
//!      光标口径不同（v2 绝对位、v3 锚+偏移），回解后统一为绝对位
//!      再比对——口径差异必须在转换层吸收，不许漏到比对层。
//!   4. 线格式自防御：未知 opcode、参数截断（帧参数数不足）、载荷
//!      越界（光标位 > 文本长）三类畸形帧必须 E_MALFORMED，解码层
//!      不崩、不产半截事件。
//!   5. ×10 保真判据：10 条确定性事件（LCG 造码位文本，覆盖 7 变体）
//!      走 v2 与 v3 两路往返各 10 次，20 项回解逐一同构；另加协商
//!      矩阵 ×8 组合断言全对。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBA00;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NO_COMMON: u16 = SEG_BASE + 1; // 版本协商零交集
const E_NOT_ADVERTISED: u16 = SEG_BASE + 2; // 请求未宣告版本
const E_MASK_BAD: u16 = SEG_BASE + 3; // 掩码未知位
const E_MALFORMED: u16 = SEG_BASE + 4; // 畸形帧
const E_FIDELITY: u16 = SEG_BASE + 5; // 往返回解不同构
const E_CASE_MISS: u16 = SEG_BASE + 6; // 保真用例不足 ×10
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

// ── 协议版本与掩码口径 ─────────────────────────────────────
pub const BIT_V2: u32 = 0x1;
pub const BIT_V3: u32 = 0x2;
pub const MASK_KNOWN: u32 = BIT_V2 | BIT_V3;

/// 协议版本闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtoVer {
    V2,
    V3,
}

/// 统一事件（纪律 1：内核唯一建模面）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnifiedEvent {
    Enable,
    Disable,
    Commit(Vec<u32>),
    /// 组合串 + 绝对光标位。
    Preedit { text: Vec<u32>, cursor: u32 },
    ContentType { hint: u32, purpose: u32 },
    CursorRect { x: i32, y: i32, w: i32, h: i32 },
    PanelHint { behavior: u32 },
}

/// v2 线帧：opcode + 参数数组。
#[derive(Clone, Debug)]
pub struct V2Frame {
    pub opcode: u16,
    pub args: Vec<u32>,
}

/// v3 线帧：opcode + 打包载荷（单 u32 承小整数、长载荷仍入数组）。
#[derive(Clone, Debug)]
pub struct V3Frame {
    pub opcode: u16,
    pub head: u32, // 小标头：类型 tag + 长度
    pub args: Vec<u32>,
}

// v2 opcode 表。
pub const V2_OP_ENABLE: u16 = 0x10;
pub const V2_OP_DISABLE: u16 = 0x11;
pub const V2_OP_COMMIT: u16 = 0x12;
pub const V2_OP_PREEDIT: u16 = 0x13;
pub const V2_OP_CONTENT: u16 = 0x14;
pub const V2_OP_CURSOR: u16 = 0x15;
pub const V2_OP_PANEL: u16 = 0x16;
// v3 opcode 表（独立段，不与 v2 复用）。
pub const V3_OP_ENABLE: u16 = 0x30;
pub const V3_OP_DISABLE: u16 = 0x31;
pub const V3_OP_COMMIT: u16 = 0x32;
pub const V3_OP_PREEDIT: u16 = 0x33;
pub const V3_OP_CONTENT: u16 = 0x34;
pub const V3_OP_CURSOR: u16 = 0x35;
pub const V3_OP_PANEL: u16 = 0x36;

/// 同构比对（纪律 1）：全变体逐一。
pub fn same_event(a: &UnifiedEvent, b: &UnifiedEvent) -> bool {
    match (a, b) {
        (UnifiedEvent::Enable, UnifiedEvent::Enable) => true,
        (UnifiedEvent::Disable, UnifiedEvent::Disable) => true,
        (UnifiedEvent::Commit(x), UnifiedEvent::Commit(y)) => vec_eq(x, y),
        (
            UnifiedEvent::Preedit { text: x, cursor: cx },
            UnifiedEvent::Preedit { text: y, cursor: cy },
        ) => vec_eq(x, y) && cx == cy,
        (UnifiedEvent::ContentType { hint: h1, purpose: p1 }, UnifiedEvent::ContentType { hint: h2, purpose: p2 }) => {
            h1 == h2 && p1 == p2
        }
        (
            UnifiedEvent::CursorRect { x: x1, y: y1, w: w1, h: h1 },
            UnifiedEvent::CursorRect { x: x2, y: y2, w: w2, h: h2 },
        ) => x1 == x2 && y1 == y2 && w1 == w2 && h1 == h2,
        (UnifiedEvent::PanelHint { behavior: b1 }, UnifiedEvent::PanelHint { behavior: b2 }) => {
            b1 == b2
        }
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

/// v2 编码：事件 → 帧（opcode 前置，参数平铺；文本先长后码位，
/// 光标绝对位直接入参——纪律 3 的 v2 口径）。
pub fn encode_v2(ev: &UnifiedEvent) -> Result<V2Frame, u16> {
    match ev {
        UnifiedEvent::Enable => Ok(V2Frame { opcode: V2_OP_ENABLE, args: Vec::new() }),
        UnifiedEvent::Disable => Ok(V2Frame { opcode: V2_OP_DISABLE, args: Vec::new() }),
        UnifiedEvent::Commit(text) => {
            let mut args: Vec<u32> = Vec::new();
            args.push(text.len() as u32);
            let mut i = 0usize;
            while i < text.len() {
                args.push(text[i]);
                i += 1;
            }
            Ok(V2Frame { opcode: V2_OP_COMMIT, args })
        }
        UnifiedEvent::Preedit { text, cursor } => {
            let mut args: Vec<u32> = Vec::new();
            args.push(text.len() as u32);
            args.push(*cursor);
            let mut i = 0usize;
            while i < text.len() {
                args.push(text[i]);
                i += 1;
            }
            Ok(V2Frame { opcode: V2_OP_PREEDIT, args })
        }
        UnifiedEvent::ContentType { hint, purpose } => {
            let mut args: Vec<u32> = Vec::new();
            args.push(*hint);
            args.push(*purpose);
            Ok(V2Frame { opcode: V2_OP_CONTENT, args })
        }
        UnifiedEvent::CursorRect { x, y, w, h } => {
            let mut args: Vec<u32> = Vec::new();
            args.push(*x as u32);
            args.push(*y as u32);
            args.push(*w as u32);
            args.push(*h as u32);
            Ok(V2Frame { opcode: V2_OP_CURSOR, args })
        }
        UnifiedEvent::PanelHint { behavior } => {
            let mut args: Vec<u32> = Vec::new();
            args.push(*behavior);
            Ok(V2Frame { opcode: V2_OP_PANEL, args })
        }
    }
}

/// v2 解码：帧 → 事件（畸形帧三类拒收，纪律 4）。
pub fn decode_v2(f: &V2Frame) -> Result<UnifiedEvent, u16> {
    match f.opcode {
        V2_OP_ENABLE => {
            if !f.args.is_empty() {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::Enable)
        }
        V2_OP_DISABLE => Ok(UnifiedEvent::Disable),
        V2_OP_COMMIT => {
            if f.args.len() < 1 {
                return Err(E_MALFORMED);
            }
            let n = f.args[0] as usize;
            if f.args.len() != n + 1 {
                return Err(E_MALFORMED);
            }
            let mut text: Vec<u32> = Vec::new();
            let mut i = 1usize;
            while i <= n {
                text.push(f.args[i]);
                i += 1;
            }
            Ok(UnifiedEvent::Commit(text))
        }
        V2_OP_PREEDIT => {
            if f.args.len() < 2 {
                return Err(E_MALFORMED);
            }
            let n = f.args[0] as usize;
            let cursor = f.args[1];
            if f.args.len() != n + 2 {
                return Err(E_MALFORMED);
            }
            if cursor > n as u32 {
                return Err(E_MALFORMED); // 光标位 > 文本长：载荷越界
            }
            let mut text: Vec<u32> = Vec::new();
            let mut i = 2usize;
            while i <= n + 1 {
                text.push(f.args[i]);
                i += 1;
            }
            Ok(UnifiedEvent::Preedit { text, cursor })
        }
        V2_OP_CONTENT => {
            if f.args.len() != 2 {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::ContentType { hint: f.args[0], purpose: f.args[1] })
        }
        V2_OP_CURSOR => {
            if f.args.len() != 4 {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::CursorRect {
                x: f.args[0] as i32,
                y: f.args[1] as i32,
                w: f.args[2] as i32,
                h: f.args[3] as i32,
            })
        }
        V2_OP_PANEL => {
            if f.args.len() != 1 {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::PanelHint { behavior: f.args[0] })
        }
        _ => Err(E_MALFORMED),
    }
}

/// v3 编码：opcode + head(tag|len<<8) + 参数数组（纪律 1/3）。
/// v3 光标口径：锚（文本长）+ 偏移（绝对位）双参，回解取偏移为绝对位。
pub fn encode_v3(ev: &UnifiedEvent) -> Result<V3Frame, u16> {
    let (op, tag, args) = match ev {
        UnifiedEvent::Enable => (V3_OP_ENABLE, 0u32, Vec::new()),
        UnifiedEvent::Disable => (V3_OP_DISABLE, 0u32, Vec::new()),
        UnifiedEvent::Commit(text) => {
            let mut a: Vec<u32> = Vec::new();
            a.push(text.len() as u32);
            let mut i = 0usize;
            while i < text.len() {
                a.push(text[i]);
                i += 1;
            }
            (V3_OP_COMMIT, 1u32, a)
        }
        UnifiedEvent::Preedit { text, cursor } => {
            let mut a: Vec<u32> = Vec::new();
            a.push(text.len() as u32);
            a.push(text.len() as u32); // v3 锚：文本长
            a.push(*cursor); // v3 偏移：绝对位
            let mut i = 0usize;
            while i < text.len() {
                a.push(text[i]);
                i += 1;
            }
            (V3_OP_PREEDIT, 2u32, a)
        }
        UnifiedEvent::ContentType { hint, purpose } => {
            let mut a: Vec<u32> = Vec::new();
            a.push(*hint);
            a.push(*purpose);
            (V3_OP_CONTENT, 3u32, a)
        }
        UnifiedEvent::CursorRect { x, y, w, h } => {
            let mut a: Vec<u32> = Vec::new();
            a.push(*x as u32);
            a.push(*y as u32);
            a.push(*w as u32);
            a.push(*h as u32);
            (V3_OP_CURSOR, 4u32, a)
        }
        UnifiedEvent::PanelHint { behavior } => {
            let mut a: Vec<u32> = Vec::new();
            a.push(*behavior);
            (V3_OP_PANEL, 5u32, a)
        }
    };
    let head = tag | ((args.len() as u32) << 8);
    Ok(V3Frame { opcode: op, head, args })
}

/// v3 解码：head 长度字段与 args 实长核对（畸形三类拒收，纪律 4）。
pub fn decode_v3(f: &V3Frame) -> Result<UnifiedEvent, u16> {
    let want_len = (f.head >> 8) as usize;
    if want_len != f.args.len() {
        return Err(E_MALFORMED);
    }
    match f.opcode {
        V3_OP_ENABLE => {
            if f.args.is_empty() {
                Ok(UnifiedEvent::Enable)
            } else {
                Err(E_MALFORMED)
            }
        }
        V3_OP_DISABLE => Ok(UnifiedEvent::Disable),
        V3_OP_COMMIT => {
            if f.args.len() < 1 {
                return Err(E_MALFORMED);
            }
            let n = f.args[0] as usize;
            if f.args.len() != n + 1 {
                return Err(E_MALFORMED);
            }
            let mut text: Vec<u32> = Vec::new();
            let mut i = 1usize;
            while i <= n {
                text.push(f.args[i]);
                i += 1;
            }
            Ok(UnifiedEvent::Commit(text))
        }
        V3_OP_PREEDIT => {
            if f.args.len() < 3 {
                return Err(E_MALFORMED);
            }
            let n = f.args[0] as usize;
            let anchor = f.args[1];
            let off = f.args[2];
            if f.args.len() != n + 3 {
                return Err(E_MALFORMED);
            }
            if off > n as u32 || anchor != n as u32 {
                return Err(E_MALFORMED); // 偏移越界 / 锚不符
            }
            let mut text: Vec<u32> = Vec::new();
            let mut i = 3usize;
            while i <= n + 2 {
                text.push(f.args[i]);
                i += 1;
            }
            // v3 口径吸收：锚+偏移 → 统一绝对位（纪律 3）。
            Ok(UnifiedEvent::Preedit { text, cursor: off })
        }
        V3_OP_CONTENT => {
            if f.args.len() != 2 {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::ContentType { hint: f.args[0], purpose: f.args[1] })
        }
        V3_OP_CURSOR => {
            if f.args.len() != 4 {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::CursorRect {
                x: f.args[0] as i32,
                y: f.args[1] as i32,
                w: f.args[2] as i32,
                h: f.args[3] as i32,
            })
        }
        V3_OP_PANEL => {
            if f.args.len() != 1 {
                return Err(E_MALFORMED);
            }
            Ok(UnifiedEvent::PanelHint { behavior: f.args[0] })
        }
        _ => Err(E_MALFORMED),
    }
}

/// 版本协商（纪律 2）：取公共最高版。
pub fn negotiate(manager_mask: u32, client_mask: u32) -> Result<ProtoVer, u16> {
    if manager_mask & !MASK_KNOWN != 0 || client_mask & !MASK_KNOWN != 0 {
        return Err(E_MASK_BAD);
    }
    if client_mask & !manager_mask != 0 {
        return Err(E_NOT_ADVERTISED);
    }
    let common = manager_mask & client_mask;
    if common & BIT_V3 != 0 {
        return Ok(ProtoVer::V3);
    }
    if common & BIT_V2 != 0 {
        return Ok(ProtoVer::V2);
    }
    Err(E_NO_COMMON)
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
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（双协议 ×10 转换保真、版本协商正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xBA00_0010);

    // R1 ×10 保真用例构造：覆盖 7 变体（Enable/Disable/Commit×2/
    //    Preedit×3/Content/Cursor/Panel），码位文本由 LCG 造。
    let mut cases: Vec<UnifiedEvent> = Vec::new();
    cases.push(UnifiedEvent::Enable);
    cases.push(UnifiedEvent::Disable);
    cases.push(UnifiedEvent::ContentType { hint: 2, purpose: 1 });
    cases.push(UnifiedEvent::CursorRect { x: -8, y: 40, w: 3, h: 19 });
    cases.push(UnifiedEvent::PanelHint { behavior: 1 });
    // Commit ×2：长短两串。
    let mut t1: Vec<u32> = Vec::new();
    let mut i = 0u32;
    while i < 6 {
        t1.push(0x4E00 + (rng.next() % 3000));
        i += 1;
    }
    cases.push(UnifiedEvent::Commit(t1));
    let mut t2: Vec<u32> = Vec::new();
    t2.push(0x20 + (rng.next() % 90));
    cases.push(UnifiedEvent::Commit(t2));
    // Preedit ×3：光标 0 / 中位 / 末位。
    let mut t3: Vec<u32> = Vec::new();
    let mut i = 0u32;
    while i < 9 {
        t3.push(0x4E00 + (rng.next() % 3000));
        i += 1;
    }
    cases.push(UnifiedEvent::Preedit { text: t3.clone(), cursor: 0 });
    cases.push(UnifiedEvent::Preedit { text: t3.clone(), cursor: 4 });
    cases.push(UnifiedEvent::Preedit { text: t3, cursor: 9 });
    if cases.len() < 10 {
        return Err(E_CASE_MISS);
    }
    hits += 1;

    // R2 v2 往返保真 ×10：encode → decode 同构，一条不同构即废。
    let mut i = 0usize;
    while i < cases.len() {
        let f = encode_v2(&cases[i])?;
        let back = decode_v2(&f)?;
        if !same_event(&back, &cases[i]) {
            return Err(E_FIDELITY);
        }
        i += 1;
    }
    hits += 1;

    // R3 v3 往返保真 ×10：含锚+偏移口径吸收。
    let mut i = 0usize;
    while i < cases.len() {
        let f = encode_v3(&cases[i])?;
        let back = decode_v3(&f)?;
        if !same_event(&back, &cases[i]) {
            return Err(E_FIDELITY);
        }
        i += 1;
    }
    hits += 1;

    // R4 协议交叉：v2 编 → v3 编 → 两帧回解同构（两路殊途同归）。
    let mut i = 0usize;
    while i < cases.len() {
        let f2 = encode_v2(&cases[i])?;
        let f3 = encode_v3(&cases[i])?;
        if !same_event(&decode_v2(&f2)?, &decode_v3(&f3)?) {
            return Err(E_FIDELITY);
        }
        i += 1;
    }
    hits += 1;

    // R5 畸形帧三探针（纪律 4）：未知 opcode / 参数截断 / 光标越界。
    let bad = V2Frame { opcode: 0x7F, args: Vec::new() };
    if decode_v2(&bad).is_ok() {
        return Err(E_MALFORMED);
    }
    let trunc = V2Frame { opcode: V2_OP_COMMIT, args: { let mut a = Vec::new(); a.push(5u32); a } };
    if decode_v2(&trunc).is_ok() {
        return Err(E_MALFORMED);
    }
    let over = V2Frame {
        opcode: V2_OP_PREEDIT,
        args: { let mut a = Vec::new(); a.push(2u32); a.push(3u32); a.push(0x4E00); a.push(0x4E01); a },
    };
    if decode_v2(&over).is_ok() {
        return Err(E_MALFORMED); // 光标 3 > 文本长 2
    }
    let bad3 = V3Frame { opcode: 0x77, head: 0, args: Vec::new() };
    if decode_v3(&bad3).is_ok() {
        return Err(E_MALFORMED);
    }
    let trunc3 = V3Frame { opcode: V3_OP_PREEDIT, head: 2 | (4 << 8), args: { let mut a = Vec::new(); a.push(2u32); a } };
    if decode_v3(&trunc3).is_ok() {
        return Err(E_MALFORMED); // head 长度 4 ≠ args 实长 1
    }
    hits += 1;

    // R6 协商矩阵 ×8：全 4 组合语义 + 未知位 + 未宣告 + 零交集三拒收。
    if negotiate(0x3, 0x3)? != ProtoVer::V3 {
        return Err(E_FIDELITY);
    }
    if negotiate(0x3, 0x1)? != ProtoVer::V2 {
        return Err(E_FIDELITY);
    }
    if negotiate(0x1, 0x1)? != ProtoVer::V2 {
        return Err(E_FIDELITY);
    }
    if negotiate(0x2, 0x2)? != ProtoVer::V3 {
        return Err(E_FIDELITY);
    }
    if negotiate(0x1, 0x2).is_ok() {
        return Err(E_NO_COMMON); // 零交集
    }
    if negotiate(0x3, 0x4).is_ok() {
        return Err(E_MASK_BAD); // 未知位
    }
    if negotiate(0x1, 0x3).is_ok() {
        return Err(E_NOT_ADVERTISED); // 请求未宣告的 V3
    }
    hits += 1;

    // R7 Preedit 口径专项：v3 锚不符（锚≠文本长）必拒（口径吸收不吞错）。
    let ev = UnifiedEvent::Preedit { text: { let mut t = Vec::new(); t.push(0x4E00u32); t.push(0x4E01); t }, cursor: 1 };
    let f = encode_v3(&ev)?;
    let mut bad_anchor = V3Frame { opcode: f.opcode, head: f.head, args: Vec::new() };
    let mut i = 0usize;
    while i < f.args.len() {
        bad_anchor.args.push(f.args[i]);
        i += 1;
    }
    bad_anchor.args[1] = 7; // 锚改成 7 ≠ 文本长 2
    if decode_v3(&bad_anchor).is_ok() {
        return Err(E_MALFORMED);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x0203)?;
    hits += 1;

    Ok(hits)
}
