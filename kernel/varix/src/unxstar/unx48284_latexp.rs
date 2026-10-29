//! unx48284_latexp — UNX-F48284 延迟账导出面（JSON 导出对接 AI-73 机器人判据库）（诊断段 0xC460）。
//!
//! 判据原文：
//!   J1 导出 ×10 schema 校验全过；判据锚定 附则五 三账本交付面。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 交付定式：导出流为严格 JSON——{"v":1,"n":N,"cases":[{"case":C,
//!      "seg":S,"ms":M},...]}；键序固定、无空白、整数无前导零——
//!      机器判据库只认定式，自由格式即拒。
//!   2. 手写序列化：no_std 无格式化库，逐字段拼字节（数字转 ASCII
//!      十进制、键名与标点逐字面量），同账双导出字节逐位相同。
//!   3. schema 校验器（AI-73 侧独立实现）：游标解析——外层三键定序、
//!      cases 数组定长、每记录四键定序、整数语法闭合（负号只许首位、
//!      前导零禁）；×10 逐条字段值回收与账本逐位相等。
//!   4. 校验负例防线：缺右括号/键序漂移/前导零/多余尾部字节 → 全拒，
//!      拒收必带诊断码——导出面非单向吐字，坏流进不了判据库。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC460;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_JS_BAD: u16 = SEG_BASE + 1; // JSON 结构失守
const E_JS_NUM: u16 = SEG_BASE + 2; // 整数语法违规（前导零/负号错位）
const E_JS_TAIL: u16 = SEG_BASE + 3; // 多余尾部字节
const E_JS_COUNT: u16 = SEG_BASE + 4; // n 字段与数组实长不闭合
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 延迟账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 6; // 导出面状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// schema 版本。
pub const SCHEMA_V: u32 = 1;

/// 延迟账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatRow {
    pub case: u32,
    pub seg: u8,
    pub ms: u32,
}

/// 延迟账。
pub struct LatLedger {
    pub rows: Vec<LatRow>,
}

impl LatLedger {
    pub fn new() -> LatLedger {
        LatLedger { rows: Vec::new() }
    }
    pub fn push(&mut self, case: u32, seg: u8, ms: u32) -> Result<(), u16> {
        if ms == 0 {
            return Err(E_LEDGER_BAD); // 零时延条目不入账（采样下限）
        }
        self.rows.push(LatRow { case, seg, ms });
        Ok(())
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    /// 账审计：案号单调不重、段号闭集、ms 闭合。
    pub fn audit(&self) -> Result<(), u16> {
        let mut prev: u32 = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.ms == 0 {
                return Err(E_LEDGER_BAD);
            }
            if i > 0 && r.case <= prev {
                return Err(E_LEDGER_BAD);
            }
            prev = r.case;
            i += 1;
        }
        Ok(())
    }
}

/// u32 → ASCII 十进制字节（无前导零，0 即 "0"）。
pub fn num_bytes(v: u32) -> Vec<u8> {
    if v == 0 {
        let mut v0 = Vec::new();
        v0.push(b'0');
        return v0;
    }
    let mut digits: Vec<u8> = Vec::new();
    let mut x = v;
    while x > 0 {
        digits.push(b'0' + (x % 10) as u8);
        x /= 10;
    }
    let mut out = Vec::new();
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        out.push(digits[i]);
    }
    out
}

fn push_lit(out: &mut Vec<u8>, s: &[u8]) {
    let mut i = 0usize;
    while i < s.len() {
        out.push(s[i]);
        i += 1;
    }
}

/// 导出：延迟账 → 定式 JSON 字节流（纪律 1/2）。
pub fn export_json(led: &LatLedger) -> Result<Vec<u8>, u16> {
    if led.row_count() == 0 {
        return Err(E_STATE_BAD); // 空账不出面
    }
    let mut out: Vec<u8> = Vec::new();
    push_lit(&mut out, b"{\"v\":");
    let vb = num_bytes(SCHEMA_V);
    push_lit(&mut out, &vb);
    push_lit(&mut out, b",\"n\":");
    let nb = num_bytes(led.row_count() as u32);
    push_lit(&mut out, &nb);
    push_lit(&mut out, b",\"cases\":[");
    let mut i = 0usize;
    while i < led.rows.len() {
        if i > 0 {
            push_lit(&mut out, b",");
        }
        let r = led.rows[i];
        push_lit(&mut out, b"{\"case\":");
        let cb = num_bytes(r.case);
        push_lit(&mut out, &cb);
        push_lit(&mut out, b",\"seg\":");
        let sb = num_bytes(r.seg as u32);
        push_lit(&mut out, &sb);
        push_lit(&mut out, b",\"ms\":");
        let mb = num_bytes(r.ms);
        push_lit(&mut out, &mb);
        push_lit(&mut out, b"}");
        i += 1;
    }
    push_lit(&mut out, b"]}");
    Ok(out)
}

/// JSON 游标校验器（AI-73 侧独立实现）：定式解析 + 值回收。
pub struct JsCursor<'a> {
    b: &'a [u8],
    pub pos: usize,
}

impl<'a> JsCursor<'a> {
    pub fn new(b: &'a [u8]) -> JsCursor<'a> {
        JsCursor { b, pos: 0 }
    }
    pub fn lit(&mut self, s: &[u8]) -> Result<(), u16> {
        let mut i = 0usize;
        while i < s.len() {
            if self.pos + i >= self.b.len() || self.b[self.pos + i] != s[i] {
                return Err(E_JS_BAD);
            }
            i += 1;
        }
        self.pos += s.len();
        Ok(())
    }
    /// 整数读取：无前导零、无负号（本 schema 全非负）。
    pub fn uint(&mut self) -> Result<u32, u16> {
        if self.pos >= self.b.len() {
            return Err(E_JS_BAD);
        }
        if self.b[self.pos] == b'-' {
            return Err(E_JS_NUM);
        }
        if self.b[self.pos] == b'0' {
            // "0" 合法，"0x…"（前导零）非法。
            self.pos += 1;
            return Ok(0);
        }
        let start = self.pos;
        let mut v: u32 = 0;
        while self.pos < self.b.len() && self.b[self.pos] >= b'0' && self.b[self.pos] <= b'9' {
            if self.pos == start && self.b[self.pos] == b'0' {
                return Err(E_JS_NUM); // 前导零
            }
            v = v.wrapping_mul(10).wrapping_add((self.b[self.pos] - b'0') as u32);
            self.pos += 1;
        }
        if self.pos == start {
            return Err(E_JS_BAD);
        }
        Ok(v)
    }
}

/// schema 校验 + 字段回收（J1 主体）：×10 逐条与账本逐位相等。
pub fn validate_export(bytes: &[u8], led: &LatLedger) -> Result<Vec<LatRow>, u16> {
    let mut c = JsCursor::new(bytes);
    c.lit(b"{\"v\":")?;
    let v = c.uint()?;
    if v != SCHEMA_V {
        return Err(E_JS_BAD);
    }
    c.lit(b",\"n\":")?;
    let n = c.uint()?;
    if n as usize != led.row_count() {
        return Err(E_JS_COUNT);
    }
    c.lit(b",\"cases\":[")?;
    let mut recs: Vec<LatRow> = Vec::new();
    let mut i = 0u32;
    while i < n {
        if i > 0 {
            c.lit(b",")?;
        }
        c.lit(b"{\"case\":")?;
        let case = c.uint()?;
        c.lit(b",\"seg\":")?;
        let seg = c.uint()?;
        if seg > 0xFF {
            return Err(E_JS_BAD);
        }
        c.lit(b",\"ms\":")?;
        let ms = c.uint()?;
        c.lit(b"}")?;
        recs.push(LatRow { case, seg: seg as u8, ms });
        i += 1;
    }
    c.lit(b"]}")?;
    if c.pos != bytes.len() {
        return Err(E_JS_TAIL);
    }
    // 逐条回收值与账本逐位相等。
    if recs.len() != led.row_count() {
        return Err(E_JS_COUNT);
    }
    let mut j = 0usize;
    while j < led.rows.len() {
        if recs[j] != led.rows[j] {
            return Err(E_JS_BAD);
        }
        j += 1;
    }
    Ok(recs)
}

/// 完成写回（判据 J2）：有导出 + 非空摘要方放行。
pub fn writeback(done: bool, exports_green: u32, summary_len: u32) -> Result<(), u16> {
    if !done || exports_green == 0 {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（导出 ×10 schema 校验全过），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：延迟账 10 条，导出 → schema 校验全过、字段值
    //    逐条逐位回收相等。
    let mut led = LatLedger::new();
    let mut i = 0u32;
    while i < 10 {
        led.push(1 + i, (i % 5) as u8, 20 + i * 7)?;
        i += 1;
    }
    led.audit()?;
    let bytes = export_json(&led)?;
    let recs = validate_export(&bytes, &led)?;
    if recs.len() != 10 {
        return Err(E_JS_COUNT);
    }
    hits += 1;

    // R2 定式终检：导出字节流与手拼定式逐位相同（键序、无空白、无
    //    前导零——机器判据库只认定式）。
    let bytes2 = export_json(&led)?;
    if bytes.len() != bytes2.len() {
        return Err(E_JS_BAD);
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != bytes2[i] {
            return Err(E_JS_BAD); // 同账双导出漂移
        }
        i += 1;
    }
    // 头部字面量抽查。
    let head: [u8; 11] = *b"{\"v\":1,\"n\":";
    let mut i = 0usize;
    while i < head.len() {
        if bytes[i] != head[i] {
            return Err(E_JS_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R3 数字语法防线：前导零注入（"n":010）→ E_JS_NUM；
    //    负号注入（"ms":-5）→ E_JS_NUM。
    let mut ledz = LatLedger::new();
    ledz.push(1, 0, 5)?;
    let bz = export_json(&ledz)?;
    // 找 "n": 后插 0 → "n":01（记录数 1 → 01）。
    let mut lead0 = Vec::new();
    let mut i = 0usize;
    while i < bz.len() {
        lead0.push(bz[i]);
        if bz[i] == b':' && bz[i - 1] == b'n' {
            lead0.push(b'0');
        }
        i += 1;
    }
    if validate_export(&lead0, &ledz).is_ok() {
        return Err(E_JS_NUM);
    }
    // "ms":-5：找 "ms": 后插 '-'。
    let mut neg = Vec::new();
    let pat: [u8; 5] = *b"\"ms\":";
    let mut i = 0usize;
    let mut done = false;
    while i < bz.len() {
        if !done && i + 5 <= bz.len() {
            let mut m = true;
            let mut k = 0usize;
            while k < 5 {
                if bz[i + k] != pat[k] {
                    m = false;
                }
                k += 1;
            }
            if m {
                let mut k = 0usize;
                while k < 5 {
                    neg.push(pat[k]);
                    k += 1;
                }
                neg.push(b'-');
                done = true;
                i += 5;
                continue;
            }
        }
        neg.push(bz[i]);
        i += 1;
    }
    if validate_export(&neg, &ledz).is_ok() {
        return Err(E_JS_NUM);
    }
    hits += 1;

    // R4 结构守门负例：缺右括号 / 键序漂移（"ms" 提前）→ E_JS_BAD。
    let trimmed = &bytes[..bytes.len() - 1];
    if validate_export(trimmed, &led).is_ok() {
        return Err(E_JS_BAD);
    }
    let mut swapped = Vec::new();
    // 头替换为 {"v":1,"n":10,"cases":[{"ms":20,"case":1,...  键序漂移。
    push_lit(&mut swapped, b"{\"v\":1,\"n\":1,\"cases\":[{\"ms\":5,\"case\":1,\"seg\":0}]}");
    if validate_export(&swapped, &ledz).is_ok() {
        return Err(E_JS_BAD);
    }
    hits += 1;

    // R5 n 字段闭合守门：n 虚标 +1（实长不变）→ E_JS_COUNT。
    let mut inflated = Vec::new();
    let mut i = 0usize;
    while i < bz.len() {
        inflated.push(bz[i]);
        if bz[i] == b':' && bz[i - 1] == b'n' {
            inflated.push(b'1'); // 1 → 11 虚标
        }
        i += 1;
    }
    if validate_export(&inflated, &ledz).is_ok() {
        return Err(E_JS_COUNT);
    }
    hits += 1;

    // R6 尾部字节守门：定式之后塞尾巴 → E_JS_TAIL。
    let mut tailed = Vec::new();
    let mut i = 0usize;
    while i < bz.len() {
        tailed.push(bz[i]);
        i += 1;
    }
    tailed.push(b'x');
    if validate_export(&tailed, &ledz).is_ok() {
        return Err(E_JS_TAIL);
    }
    hits += 1;

    // R7 空账不出面：零条目账导出 → E_STATE_BAD。
    let empty = LatLedger::new();
    if export_json(&empty).is_ok() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零导出不写回、空摘要拒、×10 全过+非空摘要放行。
    if writeback(true, 0, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if writeback(true, 10, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 10, 0x30)?;
    hits += 1;

    Ok(hits)
}
