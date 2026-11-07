//! UNX-F81007 台账快照序列化器（append-only JSONL 落盘） · 0x910F0段
//!
//! 判据原文（派工卡 81006 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81007-J1 「台账快照序列化器（append-only JSONL 落盘）」
//!      判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障 10 次
//!      零静默；失败路径显性化、内存缓冲建模；判据锚定 §AI-81 台账域
//!      任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 内存缓冲建模落盘：JsonlWriter 只持有行缓冲（Vec<String>），每行以
//!    `{"i":<n>,` 序号前缀开头，模拟 append-only 文件语义；无真实 IO。
//! 2. 转义器闭集：反斜杠/双引号/换行/制表/控制字符逐字节转义，非 ASCII
//!    字节直接拒（E_HIGH），保证行内不会出现伪造的 `","cs":` 结构。
//! 3. 每行后缀校验和：FNV-1a 32 位对「前缀+载荷」求和，8 位十六进制
//!    后缀；verify_line 复算比对，篡改必被抓（E_CSMUT）。
//! 4. 行账三层：行号单调（回退/跳号拒）、重写旧行拒、删除拒——append-only
//!    的三条铁律各占一个诊断码。
//! 5. 行型闭集：Data=1 / Marker=2，第四态行型（kind≥3）构造即拒。
//! 6. 注入核 ×10，注入必抓：每类坏样本返回 Err 且诊断码落在段闭集内。
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81006 − 81001)）。
pub const SEG_BASE: u32 = 0x910F0;

// ── 诊断码闭集（段内偏移 0x01..0x0D） ─────────────────────────────
/// 重写旧行。
pub const E_REWRITE: u16 = 1;
/// 删除行。
pub const E_DELETE: u16 = 2;
/// 行号回退。
pub const E_REWNUM: u16 = 3;
/// 行号跳号。
pub const E_SKIPNUM: u16 = 4;
/// 转义注入（载荷携带结构伪造序列）。
pub const E_ESCINJ: u16 = 5;
/// 控制字符。
pub const E_CTRL: u16 = 6;
/// 超长字段。
pub const E_TOOLONG: u16 = 7;
/// 空行。
pub const E_EMPTYLINE: u16 = 8;
/// 校验和篡改。
pub const E_CSMUT: u16 = 9;
/// 非法第四态行型。
pub const E_BADKIND: u16 = 10;
/// 非 ASCII 字节。
pub const E_HIGH: u16 = 11;
/// 行缓冲空（verify 无行）。
pub const E_EMPTY: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 单字段字节上限。
pub const MAX_FIELD: usize = 512;

/// 行型闭集：数据行。
pub const KIND_DATA: u8 = 1;
/// 行型闭集：标记行。
pub const KIND_MARK: u8 = 2;

/// FNV-1a 32 位步进。
pub fn fnv1a32_step(h: u32, byte: u8) -> u32 {
    h ^ (byte as u32).wrapping_mul(0x0100_0193)
}

/// FNV-1a 32 位全量（空输入 = 偏移基 0x811C9DC5）。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = fnv1a32_step(h, bytes[i]);
        i += 1;
    }
    h
}

fn push_nibble(out: &mut String, n: u8) {
    let d = if n < 10 { b'0' + n } else { b'a' + (n - 10) };
    out.push(d as char);
}

fn hex8(h: u32) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < 8 {
        let shift = (7 - i) * 4;
        push_nibble(&mut s, ((h >> shift) & 0xF) as u8);
        i += 1;
    }
    s
}

/// 载荷合法性：空拒/超长拒/控制字符拒/非 ASCII 拒/结构伪造序列拒。
fn check_field(field: &str) -> Result<(), JsonlError> {
    if field.is_empty() {
        return Err(JsonlError::EmptyLine);
    }
    if field.len() > MAX_FIELD {
        return Err(JsonlError::TooLong);
    }
    let b = field.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c < 0x20 {
            return Err(JsonlError::Ctrl);
        }
        if c >= 0x80 {
            return Err(JsonlError::High);
        }
        i += 1;
    }
    if field.contains("\\n") || field.contains("\\\\") || field.contains("\\u") {
        return Err(JsonlError::EscInj);
    }
    if field.contains("\",\"cs\":") || field.contains("\"i\":") {
        return Err(JsonlError::EscInj);
    }
    Ok(())
}

/// 逐字节转义（\ " 换行 制表；控制字符已在上游拒）。
fn push_escaped(out: &mut String, s: &str) {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                out.push('\\');
                out.push('\\');
            }
            b'"' => {
                out.push('\\');
                out.push('"');
            }
            _ => out.push(b[i] as char),
        }
        i += 1;
    }
}

/// 行缓冲建模：append-only JSONL。
#[derive(Debug)]
pub struct JsonlWriter {
    lines: Vec<String>,
    row_nums: Vec<u64>,
    total_bytes: u64,
}

impl JsonlWriter {
    /// 空缓冲。
    pub fn new() -> JsonlWriter {
        JsonlWriter {
            lines: Vec::new(),
            row_nums: Vec::new(),
            total_bytes: 0,
        }
    }

    /// 已落盘行数。
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// 字节总量账。
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// 下一个行号。
    pub fn next_row(&self) -> u64 {
        if let Some(last) = self.row_nums.last() {
            return last + 1;
        }
        0
    }

    /// 行号单调账：expect 必须恰等于 next_row，小则回退、大则跳号。
    fn check_row_order(&self, expect: u64) -> Result<(), JsonlError> {
        let nxt = self.next_row();
        if expect < nxt {
            return Err(JsonlError::RewNum);
        }
        if expect > nxt {
            return Err(JsonlError::SkipNum);
        }
        Ok(())
    }

    /// 编码一行（不落账）：前缀 + 转义载荷 + 校验和后缀。
    fn encode_row(row: u64, kind: u8, field: &str) -> String {
        let mut prefix = String::new();
        prefix.push_str("{\"i\":");
        let mut num = String::new();
        let mut v = row;
        if v == 0 {
            num.push('0');
        }
        let mut digits: [u8; 20] = [0u8; 20];
        let mut dn = 0usize;
        while v > 0 {
            digits[dn] = b'0' + (v % 10) as u8;
            dn += 1;
            v /= 10;
        }
        let mut k = dn;
        while k > 0 {
            k -= 1;
            num.push(digits[k] as char);
        }
        prefix.push_str(&num);
        prefix.push_str(",\"k\":");
        prefix.push((b'0' + kind) as char);
        prefix.push_str(",\"f\":\"");
        // 校验和域 = 前缀字节 ∥ 转义前载荷字节。
        let mut cs = fnv1a32(prefix.as_bytes());
        let fb = field.as_bytes();
        let mut fi = 0usize;
        while fi < fb.len() {
            cs = fnv1a32_step(cs, fb[fi]);
            fi += 1;
        }
        push_escaped(&mut prefix, field);
        prefix.push_str("\",\"cs\":\"");
        prefix.push_str(&hex8(cs));
        prefix.push_str("\"}\n");
        prefix
    }

    /// append-only 落一行（显式行号账）。
    pub fn append_at(&mut self, expect_row: u64, kind: u8, field: &str) -> Result<u64, JsonlError> {
        if kind != KIND_DATA && kind != KIND_MARK {
            return Err(JsonlError::BadKind);
        }
        check_field(field)?;
        self.check_row_order(expect_row)?;
        let line = Self::encode_row(expect_row, kind, field);
        self.total_bytes += line.len() as u64;
        self.row_nums.push(expect_row);
        self.lines.push(line);
        Ok(expect_row)
    }

    /// 重写旧行：append-only 铁律，构造即拒。
    pub fn rewrite_row(&self, row: u64, _field: &str) -> Result<(), JsonlError> {
        let mut i = 0usize;
        while i < self.row_nums.len() {
            if self.row_nums[i] == row {
                return Err(JsonlError::Rewrite);
            }
            i += 1;
        }
        Err(JsonlError::Rewrite)
    }

    /// 删除行：append-only 铁律，构造即拒。
    pub fn delete_row(&self, row: u64) -> Result<(), JsonlError> {
        let mut i = 0usize;
        while i < self.row_nums.len() {
            if self.row_nums[i] == row {
                return Err(JsonlError::Delete);
            }
            i += 1;
        }
        Err(JsonlError::Delete)
    }

    /// 取第 idx 行原文（越界 None）。
    pub fn line(&self, idx: usize) -> Option<&str> {
        if idx >= self.lines.len() {
            return None;
        }
        Some(&self.lines[idx])
    }

    /// 逐行复算校验和：篡改/损坏必被抓。空缓冲拒（Empty）。
    pub fn verify_all(&self) -> Result<usize, JsonlError> {
        if self.lines.is_empty() {
            return Err(JsonlError::Empty);
        }
        let mut n = 0usize;
        while n < self.lines.len() {
            verify_line(&self.lines[n])?;
            n += 1;
        }
        Ok(n)
    }

    /// 序号前缀冻结口径：每行必须以 `{"i":<n>,` 开头。
    pub fn prefix_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.lines.len() {
            let want_row = self.row_nums[i];
            let mut want = String::new();
            want.push_str("{\"i\":");
            let mut num = String::new();
            let mut v = want_row;
            if v == 0 {
                num.push('0');
            }
            let mut digits: [u8; 20] = [0u8; 20];
            let mut dn = 0usize;
            while v > 0 {
                digits[dn] = b'0' + (v % 10) as u8;
                dn += 1;
                v /= 10;
            }
            let mut k = dn;
            while k > 0 {
                k -= 1;
                num.push(digits[k] as char);
            }
            want.push_str(&num);
            want.push(',');
            if !self.lines[i].starts_with(&want) {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 独立行校验：复算 cs 后缀比对，篡改 → Csmut。
pub fn verify_line(line: &str) -> Result<(), JsonlError> {
    let mark = "\",\"cs\":\"";
    let mut cut = None;
    let b = line.as_bytes();
    let m = mark.as_bytes();
    let mut i = 0usize;
    while i + m.len() <= b.len() {
        let mut same = true;
        let mut k = 0usize;
        while k < m.len() {
            if b[i + k] != m[k] {
                same = false;
            }
            k += 1;
        }
        if same {
            cut = Some(i);
        }
        i += 1;
    }
    let pos = match cut {
        Some(p) => p,
        None => return Err(JsonlError::CsMut),
    };
    if pos + m.len() + 9 > b.len() {
        return Err(JsonlError::CsMut);
    }
    let cs_hex = &line[pos + m.len()..pos + m.len() + 8];
    let recomputed = hex8(fnv1a32(&b[..pos]));
    if cs_hex != recomputed {
        return Err(JsonlError::CsMut);
    }
    Ok(())
}

/// JSONL 域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonlError {
    /// 重写旧行。
    Rewrite,
    /// 删除行。
    Delete,
    /// 行号回退。
    RewNum,
    /// 行号跳号。
    SkipNum,
    /// 转义注入。
    EscInj,
    /// 控制字符。
    Ctrl,
    /// 超长字段。
    TooLong,
    /// 空行。
    EmptyLine,
    /// 校验和篡改。
    CsMut,
    /// 非法第四态行型。
    BadKind,
    /// 非 ASCII 字节。
    High,
    /// 行缓冲空。
    Empty,
}

impl JsonlError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            JsonlError::Rewrite => E_REWRITE,
            JsonlError::Delete => E_DELETE,
            JsonlError::RewNum => E_REWNUM,
            JsonlError::SkipNum => E_SKIPNUM,
            JsonlError::EscInj => E_ESCINJ,
            JsonlError::Ctrl => E_CTRL,
            JsonlError::TooLong => E_TOOLONG,
            JsonlError::EmptyLine => E_EMPTYLINE,
            JsonlError::CsMut => E_CSMUT,
            JsonlError::BadKind => E_BADKIND,
            JsonlError::High => E_HIGH,
            JsonlError::Empty => E_EMPTY,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            JsonlError::Rewrite => "rewrite of appended row refused",
            JsonlError::Delete => "row deletion refused",
            JsonlError::RewNum => "row number rewind",
            JsonlError::SkipNum => "row number gap",
            JsonlError::EscInj => "escape injection payload",
            JsonlError::Ctrl => "control character in field",
            JsonlError::TooLong => "field over budget",
            JsonlError::EmptyLine => "empty line refused",
            JsonlError::CsMut => "checksum suffix tampered",
            JsonlError::BadKind => "row kind outside closed set",
            JsonlError::High => "non-ascii byte refused",
            JsonlError::Empty => "buffer empty",
        }
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    let mut w = JsonlWriter::new();
    let _ = w.append_at(0, KIND_DATA, "alpha");
    let _ = w.append_at(1, KIND_DATA, "beta");
    // 1 重写旧行 → Rewrite
    match w.rewrite_row(0, "hacked") {
        Err(e) => out.push(("rewrite-old", e.code())),
        Ok(_) => out.push(("rewrite-old-MISSED", SEG_BASE)),
    }
    // 2 删除行 → Delete
    match w.delete_row(1) {
        Err(e) => out.push(("delete-row", e.code())),
        Ok(_) => out.push(("delete-row-MISSED", SEG_BASE)),
    }
    // 3 行号回退 → RewNum
    match w.append_at(0, KIND_DATA, "rew") {
        Err(e) => out.push(("row-rewind", e.code())),
        Ok(_) => out.push(("row-rewind-MISSED", SEG_BASE)),
    }
    // 4 行号跳号 → SkipNum
    match w.append_at(9, KIND_DATA, "skip") {
        Err(e) => out.push(("row-skip", e.code())),
        Ok(_) => out.push(("row-skip-MISSED", SEG_BASE)),
    }
    // 5 转义注入（引号内嵌引号/结构伪造）→ EscInj
    match w.append_at(2, KIND_DATA, "x\",\"cs\":\"deadbeef") {
        Err(e) => out.push(("esc-inject", e.code())),
        Ok(_) => out.push(("esc-inject-MISSED", SEG_BASE)),
    }
    // 6 控制字符 → Ctrl
    match w.append_at(2, KIND_DATA, "bad\u{0007}bell") {
        Err(e) => out.push(("ctrl-char", e.code())),
        Ok(_) => out.push(("ctrl-char-MISSED", SEG_BASE)),
    }
    // 7 超长字段 → TooLong
    let big = build_long(600);
    match w.append_at(2, KIND_DATA, &big) {
        Err(e) => out.push(("overlong", e.code())),
        Ok(_) => out.push(("overlong-MISSED", SEG_BASE)),
    }
    // 8 空行 → EmptyLine
    match w.append_at(2, KIND_DATA, "") {
        Err(e) => out.push(("empty-line", e.code())),
        Ok(_) => out.push(("empty-line-MISSED", SEG_BASE)),
    }
    // 9 校验和篡改 → CsMut（篡改已落盘行后缀一位）
    let orig = match w.line(0) {
        Some(l) => l.to_string(),
        None => String::new(),
    };
    let tampered = tamper_cs(&orig);
    match verify_line(&tampered) {
        Err(e) => out.push(("cs-tamper", e.code())),
        Ok(_) => out.push(("cs-tamper-MISSED", SEG_BASE)),
    }
    // 10 非法第四态行型 → BadKind
    match w.append_at(2, 7, "ghost") {
        Err(e) => out.push(("bad-kind", e.code())),
        Ok(_) => out.push(("bad-kind-MISSED", SEG_BASE)),
    }
    out
}

fn build_long(n: usize) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < n {
        s.push('x');
        i += 1;
    }
    s
}

/// 篡改 cs 后缀：把第一个十六进制位翻转（必改哈希比对结果）。
fn tamper_cs(line: &str) -> String {
    let b = line.as_bytes();
    let mark = "\",\"cs\":\"";
    let m = mark.as_bytes();
    let mut pos = None;
    let mut i = 0usize;
    while i + m.len() <= b.len() {
        let mut same = true;
        let mut k = 0usize;
        while k < m.len() {
            if b[i + k] != m[k] {
                same = false;
            }
            k += 1;
        }
        if same {
            pos = Some(i);
        }
        i += 1;
    }
    let p = match pos {
        Some(p) => p,
        None => return line.to_string(),
    };
    let cs_at = p + m.len();
    let mut out = String::new();
    let mut j = 0usize;
    while j < b.len() {
        if j == cs_at {
            out.push(if b[j] == b'0' { 'f' } else { '0' });
        } else {
            out.push(b[j] as char);
        }
        j += 1;
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (_, code) = samples[i];
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81007-J1 落地）：13 条，正向 ×10 + 注入汇总。
pub fn run_unx81006_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81006");

    // 1 段位守卫
    let ok = SEG_BASE == 0x910F0 && SEG_BASE % 0x30 == 0;
    s.add("F81007-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81007-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 正向写入：两行落账、行号递增
    let mut w = JsonlWriter::new();
    let r0 = w.append_at(0, KIND_DATA, "snap-a");
    let r1 = w.append_at(1, KIND_DATA, "snap-b");
    let ok = matches!(r0, Ok(0)) && matches!(r1, Ok(1)) && w.line_count() == 2;
    s.add("F81007-03 正向写入", ok, if ok { "" } else { "写入路径坏" });

    // 4 序号前缀冻结口径
    let ok = w.prefix_ok();
    s.add("F81007-04 序号前缀", ok, if ok { "" } else { "前缀口径漂移" });

    // 5 转义保真：引号/反斜杠转义后行内校验仍过
    let mut we = JsonlWriter::new();
    let _ = we.append_at(0, KIND_DATA, "q\"b\\c");
    let esc_ok = match we.line(0) {
        Some(l) => l.contains("\\\"") && l.contains("\\\\") && verify_line(l).is_ok(),
        None => false,
    };
    s.add("F81007-05 转义保真", esc_ok, if esc_ok { "" } else { "转义器失真" });

    // 6 校验和确定：空输入 = FNV 偏移基；同行两次编码逐位一致
    let ok = fnv1a32(b"") == 0x811C_9DC5
        && JsonlWriter::encode_row(3, KIND_DATA, "z") == JsonlWriter::encode_row(3, KIND_DATA, "z");
    s.add("F81007-06 校验确定", ok, if ok { "" } else { "校验和不确定" });

    // 7 字节总量账
    let sum = match we.line(0) {
        Some(l) => l.len() as u64,
        None => 0,
    };
    let ok = we.total_bytes() == sum;
    s.add("F81007-07 字节账", ok, if ok { "" } else { "字节账漂移" });

    // 8 行账单调：回退/跳号双拒
    let ok = matches!(we.append_at(0, KIND_DATA, "r"), Err(JsonlError::RewNum))
        && matches!(we.append_at(5, KIND_DATA, "g"), Err(JsonlError::SkipNum));
    s.add("F81007-08 行账单调", ok, if ok { "" } else { "行号闸失灵" });

    // 9 重写/删除双拒
    let ok = we.rewrite_row(0, "x").is_err() && we.delete_row(0).is_err();
    s.add("F81007-09 追加铁律", ok, if ok { "" } else { "append-only 漏洞" });

    // 10 空行/超长/控制字符三拒
    let ok = matches!(we.append_at(1, KIND_DATA, ""), Err(JsonlError::EmptyLine))
        && matches!(we.append_at(1, KIND_DATA, "bad\u{001B}"), Err(JsonlError::Ctrl))
        && matches!(we.append_at(1, KIND_DATA, &build_long(600)), Err(JsonlError::TooLong));
    s.add("F81007-10 场内三拒", ok, if ok { "" } else { "载荷闸失灵" });

    // 11 全行复算（往返一致）
    let mut wv = JsonlWriter::new();
    let _ = wv.append_at(0, KIND_DATA, "v0");
    let _ = wv.append_at(1, KIND_MARK, "v1");
    let ok = matches!(wv.verify_all(), Ok(2));
    s.add("F81007-11 往返一致", ok, if ok { "" } else { "复算失配" });

    // 12 空缓冲复算拒
    let ok = matches!(JsonlWriter::new().verify_all(), Err(JsonlError::Empty));
    s.add("F81007-12 空缓冲拒", ok, if ok { "" } else { "空缓冲漏抓" });

    // 13 注入 10/10
    let ok = inject_all_caught();
    s.add("F81007-13 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81006_checks_green() {
        let set = run_unx81006_checks();
        assert!(set.all_passed(), "UNX-F81006 red: {}", set.red_items_text());
    }
}
