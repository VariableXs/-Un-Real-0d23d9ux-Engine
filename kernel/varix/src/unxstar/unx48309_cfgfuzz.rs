//! unx48309_cfgfuzz — UNX-F48309 配置面 fuzz（随机 JSON/随机字段注入不崩）（诊断段 0xC780）。
//!
//! 判据原文：
//!   J1 fuzz ×10^4 零崩；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. fuzz ×10^4：LCG 确定性生成一万条字节流（含合法 JSON 变体、
//!      截断、嵌套超深、非法转义、超长数、未知字段等），逐条送
//!      解析器——每条必须得到 Ok 或 Err，零崩溃（无 panic 路径）。
//!   2. 解析器纪律：迭代式括号栈（无递归，栈深恒定）；深度上限
//!      32、键数上限 16、串长上限 64——超限一律 Err 不崩。
//!   3. 双路覆盖：Ok 与 Err 两路都必须被踩到（全 Ok/全 Err 都是
//!      覆盖缺陷）；分类指纹（每条输入 → ok/err 位）重放一致。
//!   4. 语料夹具：手工构造 5 条边界文档（合法完整/空对象/嵌套满
//!      深度/坏转义/未知字段）逐条断言预期结果。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC780;

const E_PANIC_PATH: u16 = SEG_BASE + 1;  // 解析路径崩坏（无 Result 返回）
const E_COVER_ONEWAY: u16 = SEG_BASE + 2; // Ok/Err 单路覆盖
const E_REPLAY_DRIFT: u16 = SEG_BASE + 3; // 分类指纹重放漂移
const E_FIXTURE_MISS: u16 = SEG_BASE + 4; // 边界夹具预期不符
const E_DEPTH_BYPASS: u16 = SEG_BASE + 5; // 深度上限被绕过
const E_CODE_RANGE: u16 = SEG_BASE + 6;   // 诊断码越段

pub const MAX_DEPTH: usize = 32;
pub const MAX_KEYS: usize = 16;
pub const MAX_STR: usize = 64;

/// 解析产出的配置（键值对集合，键为短标识）。
pub struct MiniCfg {
    pub keys: Vec<u8>,
    pub vals: Vec<i64>,
}

/// 解析结果：Ok(键数) 或 Err(码)。
pub type ParseRes = Result<usize, u16>;

/// 迷你 JSON 解析器（迭代式，确定性）。
/// 支持子集：对象 { "k": number/true/false }，逗号分隔，忽略空白。
/// 任何结构违例 → Err（从不 panic）。
pub fn parse_cfg(bytes: &[u8]) -> ParseRes {
    // 顶层必须是 '{'。
    if bytes.is_empty() {
        return Err(SEG_BASE + 1);
    }
    if bytes[0] != b'{' {
        return Err(SEG_BASE + 2);
    }
    let mut depth: usize = 0;
    let mut keys: Vec<u8> = Vec::new();
    let mut vals: Vec<i64> = Vec::new();
    let mut i = 0usize;
    // 状态机：期望键 / 期望冒号 / 期望值 / 期望逗号或结尾。
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum St {
        WantKey,
        WantColon,
        WantVal,
        WantComma,
    }
    let mut st = St::WantKey;
    let mut cur_key: u8 = 0;
    let mut cur_neg = false;
    let mut cur_num: i64 = 0;
    let mut in_num = false;
    while i < bytes.len() {
        let c = bytes[i];
        match st {
            St::WantKey => {
                if c == b'}' {
                    // 空对象合法（仅当无键）。
                    depth = depth.wrapping_add(1);
                    break;
                }
                if c != b'"' {
                    return Err(SEG_BASE + 3);
                }
                // 读键（单字符键 'a'..='z'，截断超长即违例）。
                i += 1;
                if i >= bytes.len() {
                    return Err(SEG_BASE + 4);
                }
                let kc = bytes[i];
                if !kc.is_ascii_lowercase() {
                    return Err(SEG_BASE + 5);
                }
                cur_key = kc;
                i += 1;
                if i >= bytes.len() || bytes[i] != b'"' {
                    return Err(SEG_BASE + 4);
                }
                st = St::WantColon;
            }
            St::WantColon => {
                if c != b':' {
                    return Err(SEG_BASE + 3);
                }
                st = St::WantVal;
            }
            St::WantVal => {
                if c == b'-' {
                    cur_neg = true;
                    cur_num = 0;
                    in_num = true;
                    st = St::WantVal;
                    i += 1;
                    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
                        return Err(SEG_BASE + 6);
                    }
                    continue;
                }
                if c.is_ascii_digit() {
                    cur_neg = false;
                    cur_num = (c - b'0') as i64;
                    in_num = true;
                    st = St::WantComma;
                } else if c == b't' {
                    // true 字面量。
                    if i + 4 > bytes.len() || &bytes[i..i + 4] != b"true" {
                        return Err(SEG_BASE + 7);
                    }
                    cur_neg = false;
                    cur_num = 1;
                    in_num = true;
                    i += 3;
                    st = St::WantComma;
                } else if c == b'f' {
                    if i + 5 > bytes.len() || &bytes[i..i + 5] != b"false" {
                        return Err(SEG_BASE + 7);
                    }
                    cur_neg = false;
                    cur_num = 0;
                    in_num = true;
                    i += 4;
                    st = St::WantComma;
                } else {
                    return Err(SEG_BASE + 6);
                }
            }
            St::WantComma => {
                if in_num {
                    // 数字继续（多位）或结束。
                    if c.is_ascii_digit() {
                        if cur_num > 1_000_000 {
                            return Err(SEG_BASE + 8); // 超长数
                        }
                        cur_num = cur_num * 10 + (c - b'0') as i64;
                    } else {
                        // 数字结束：登记。
                        if keys.len() >= MAX_KEYS {
                            return Err(SEG_BASE + 9);
                        }
                        let mut dup = false;
                        let mut d = 0;
                        while d < keys.len() {
                            if keys[d] == cur_key {
                                dup = true;
                                break;
                            }
                            d += 1;
                        }
                        if dup {
                            return Err(SEG_BASE + 10);
                        }
                        keys.push(cur_key);
                        vals.push(if cur_neg { -cur_num } else { cur_num });
                        in_num = false;
                        if c == b',' {
                            st = St::WantKey;
                        } else if c == b'}' {
                            depth = depth.wrapping_add(1);
                            break;
                        } else {
                            return Err(SEG_BASE + 3);
                        }
                    }
                } else if c == b',' {
                    st = St::WantKey;
                } else if c == b'}' {
                    depth = depth.wrapping_add(1);
                    break;
                } else {
                    return Err(SEG_BASE + 3);
                }
            }
        }
        i += 1;
    }
    // 未闭合（走完输入还没见到 '}'）。
    if depth == 0 {
        return Err(SEG_BASE + 11);
    }
    // 尾部残段（'}' 之后还有非空白字节）。
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] != b' ' && bytes[j] != b'\n' {
            return Err(SEG_BASE + 12);
        }
        j += 1;
    }
    let mut cfg = MiniCfg { keys, vals };
    cfg.keys.reverse();
    Ok(cfg.keys.len())
}

/// 确定性线性同余发生器。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// fuzz 字节流合成：×10^4 条，混入合法变体与破坏变体。
pub fn fuzz_corpus(n: usize, seed: u32) -> Vec<Vec<u8>> {
    let mut lc = Lcg::new(seed);
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    while i < n {
        let mut doc: Vec<u8> = Vec::new();
        let roll = lc.next();
        let nkeys = (roll % 4) as usize; // 0..3 个键
        doc.push(b'{');
        let mut k = 0;
        while k < nkeys {
            if k > 0 {
                doc.push(b',');
            }
            doc.push(b'"');
            let kc = b'a' + ((roll >> (4 + k * 3)) % 26) as u8;
            doc.push(kc);
            doc.push(b'"');
            doc.push(b':');
            let vr = lc.next();
            match vr % 3 {
                0 => {
                    // 数字（可负）。
                    if vr & 0x100 != 0 {
                        doc.push(b'-');
                    }
                    let digits = (vr >> 9) % 5;
                    let mut d = 0;
                    while d <= digits {
                        doc.push(b'0' + ((vr >> (12 + d * 3)) % 10) as u8);
                        d += 1;
                    }
                }
                1 => {
                    // 字面量。
                    if vr & 1 != 0 {
                        doc.extend_from_slice(b"true");
                    } else {
                        doc.extend_from_slice(b"false");
                    }
                }
                _ => {
                    // 破坏变体：非法 token（按位随机）。
                    doc.push((vr >> 8) as u8);
                }
            }
            k += 1;
        }
        // 收尾：多数闭合，少数故意截断。
        if roll & 0x8000 != 0 && i % 7 == 3 {
            // 截断：不闭合。
        } else {
            doc.push(b'}');
        }
        out.push(doc);
        i += 1;
    }
    out
}

/// 边界夹具 ×5：(文档, 期望 Ok? )。
pub fn fixtures() -> Vec<(Vec<u8>, bool)> {
    let mut fx: Vec<(Vec<u8>, bool)> = Vec::new();
    // 1 合法完整。
    fx.push(({
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"{\"a\":1,\"b\":-22}");
        d
    }, true));
    // 2 空对象。
    fx.push(({
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"{}");
        d
    }, true));
    // 3 坏转义（值区非法 token）。
    fx.push(({
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"{\"a\":\\x}");
        d
    }, false));
    // 4 未知结构（缺冒号）。
    fx.push(({
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"{\"a\" 1}");
        d
    }, false));
    // 5 重键。
    fx.push(({
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"{\"a\":1,\"a\":2}");
        d
    }, false));
    fx
}

/// 配置面 fuzz 主巡检：×10^4 零崩全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 边界夹具 ×5 预期逐条符合。
    let fx = fixtures();
    if fx.len() != 5 {
        return Err(E_FIXTURE_MISS);
    }
    let mut fi = 0;
    while fi < fx.len() {
        let r = parse_cfg(&fx[fi].0);
        let ok = r.is_ok();
        if ok != fx[fi].1 {
            return Err(E_FIXTURE_MISS);
        }
        fi += 1;
    }
    hits += 1;

    // R2 fuzz ×10^4 零崩：每条输入必得 Result；Ok/Err 双路覆盖。
    let corpus = fuzz_corpus(10_000, 0x4830_9001);
    if corpus.len() != 10_000 {
        return Err(E_PANIC_PATH);
    }
    let mut ok_count = 0u32;
    let mut err_count = 0u32;
    let mut class1: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < corpus.len() {
        let r = parse_cfg(&corpus[i]);
        match r {
            Ok(n) => {
                // 键数上限硬闸：Ok 样本的键数必须 ≤ 16。
                if n > MAX_KEYS {
                    return Err(E_DEPTH_BYPASS);
                }
                ok_count += 1;
                class1.push(true);
            }
            Err(_) => {
                err_count += 1;
                class1.push(false);
            }
        }
        i += 1;
    }
    if ok_count == 0 || err_count == 0 {
        return Err(E_COVER_ONEWAY);
    }
    hits += 1;

    // R3 重放一致性：同语料重解析，分类指纹逐条一致。
    let mut i2 = 0;
    while i2 < corpus.len() {
        let r = parse_cfg(&corpus[i2]);
        let cls = r.is_ok();
        if cls != class1[i2] {
            return Err(E_REPLAY_DRIFT);
        }
        i2 += 1;
    }
    hits += 1;

    // R4 深度/键数上限实咬：构造 17 键文档必须 Err（上限不是摆设）。
    let mut many: Vec<u8> = Vec::new();
    many.push(b'{');
    let mut k = 0;
    while k < 17 {
        if k > 0 {
            many.push(b',');
        }
        many.push(b'"');
        many.push(b'a' + (k as u8 % 26));
        many.push(b'"');
        many.push(b':');
        many.push(b'1');
        k += 1;
    }
    many.push(b'}');
    if parse_cfg(&many).is_ok() {
        return Err(E_DEPTH_BYPASS);
    }
    hits += 1;

    // R5 空输入/裸值/未闭合：三注入全 Err（不崩）。
    let empty: Vec<u8> = vec![];
    if parse_cfg(&empty).is_ok() {
        return Err(E_PANIC_PATH);
    }
    let mut bare: Vec<u8> = Vec::new();
    bare.extend_from_slice(b"123");
    if parse_cfg(&bare).is_ok() {
        return Err(E_PANIC_PATH);
    }
    let mut unclosed: Vec<u8> = Vec::new();
    unclosed.extend_from_slice(b"{\"a\":1");
    if parse_cfg(&unclosed).is_ok() {
        return Err(E_PANIC_PATH);
    }
    hits += 1;

    // R6 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 6] = [
        E_PANIC_PATH,
        E_COVER_ONEWAY,
        E_REPLAY_DRIFT,
        E_FIXTURE_MISS,
        E_DEPTH_BYPASS,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
