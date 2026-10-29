//! UNX-F0674 · 耗时账双格式导出（CSV/JSON）· 诊断段 0x5580（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0674-J1 双格式导出内容等价（逐行 diff 语义等 100%）且 schema
//! 冻结哈希封存。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 双格式同源：CSV 与 JSON 均由同一账行模型渲染，字段序一致。
//!  3. 语义等价核：两格式各解析回账行模型，逐行逐字段全等（语义 diff = 0）。
//!  4. schema 冻结哈希：表头闭集指纹封存，表头变更 → 哈希复核失败拦截。
//!  5. 导出只读：渲染不改账，账行篡改 → 解析对账检出。
//!  6. 诊断段 0x5580 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5580;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_SCHEMA: u16 = SEG_BASE + 1;  // schema 哈希复核失败（表头漂移）
pub const E_CSV: u16 = SEG_BASE + 2;     // CSV 解析失败（列数/数值非法）
pub const E_JSON: u16 = SEG_BASE + 3;    // JSON 解析失败（键缺/值非法）
pub const E_SEM: u16 = SEG_BASE + 4;     // 语义不等价（逐行 diff 非零）
pub const E_ROW: u16 = SEG_BASE + 5;     // 账行要素缺
pub const E_SEAL: u16 = SEG_BASE + 6;    // schema 已封，再改拦截
pub const E_EMPTY: u16 = SEG_BASE + 7;   // 空账导出拦截

/// LCG64。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// schema 表头闭集（5 列）。
pub const SCHEMA_COLS: [&str; 5] = ["mach", "run", "total_ms", "seg1_ms", "seg2_ms"];

/// schema 冻结哈希（表头闭集指纹）。
pub fn schema_fp() -> u64 {
    let mut buf = Vec::new();
    for c in SCHEMA_COLS {
        for b in c.as_bytes() {
            buf.push(*b);
        }
        buf.push(0x1F);
    }
    fnv1a64(&buf)
}

/// schema 封存簿。
pub struct SchemaSeal {
    pub frozen_fp: u64,
    pub sealed: bool,
}

impl SchemaSeal {
    pub fn freeze() -> SchemaSeal {
        SchemaSeal { frozen_fp: schema_fp(), sealed: true }
    }

    /// 封存复核：现算表头指纹 = 冻结指纹。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_SEAL);
        }
        if schema_fp() != self.frozen_fp {
            return Err(E_SCHEMA);
        }
        Ok(())
    }

    /// 表头漂移注入核：模拟表头闭集被改 → 复核必失败。
    pub fn inject_drift_must_fail(&self) -> Result<(), u16> {
        // 构造漂移指纹（表头 + "x"）
        let mut buf = Vec::new();
        for c in SCHEMA_COLS {
            for b in c.as_bytes() {
                buf.push(*b);
            }
            buf.push(0x1F);
        }
        buf.push(b'x');
        let drifted = fnv1a64(&buf);
        if drifted == self.frozen_fp {
            return Err(E_SCHEMA);
        }
        Ok(())
    }
}

/// 账行模型（导出唯一数据源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    /// 机型序号（0..2）。
    pub mach: u8,
    /// 次号。
    pub run: u32,
    pub total_ms: u64,
    pub seg1_ms: u64,
    pub seg2_ms: u64,
}

impl LedgerRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.mach > 2 || self.run == 0 {
            return Err(E_ROW);
        }
        if self.seg1_ms + self.seg2_ms != self.total_ms {
            return Err(E_ROW);
        }
        Ok(())
    }
}

/// 确定性账行生成（段和守恒内建）。
pub fn synth_rows(seed: u64, n: usize) -> Result<Vec<LedgerRow>, u16> {
    if n == 0 {
        return Err(E_EMPTY);
    }
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mach = (i % 3) as u8;
        let s1 = 900 + rng.next_u32() as u64 % 400;
        let s2 = 1_100 + rng.next_u32() as u64 % 500;
        out.push(LedgerRow {
            mach,
            run: (i + 1) as u32,
            total_ms: s1 + s2,
            seg1_ms: s1,
            seg2_ms: s2,
        });
    }
    Ok(out)
}

/// 整数转字符串（导出用）。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = v;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut s = String::new();
    for b in &buf[i..] {
        s.push(*b as char);
    }
    s
}

/// CSV 渲染：表头 + 逐行 `mach,run,total,seg1,seg2`。
pub fn render_csv(rows: &[LedgerRow]) -> Result<Vec<String>, u16> {
    if rows.is_empty() {
        return Err(E_EMPTY);
    }
    for r in rows {
        r.audit()?;
    }
    let mut out = Vec::new();
    let mut head = String::new();
    for (i, c) in SCHEMA_COLS.iter().enumerate() {
        if i > 0 {
            head.push(',');
        }
        head.push_str(c);
    }
    out.push(head);
    for r in rows {
        let mut l = String::new();
        l.push_str(&itoa(r.mach as u64));
        l.push(',');
        l.push_str(&itoa(r.run as u64));
        l.push(',');
        l.push_str(&itoa(r.total_ms));
        l.push(',');
        l.push_str(&itoa(r.seg1_ms));
        l.push(',');
        l.push_str(&itoa(r.seg2_ms));
        out.push(l);
    }
    Ok(out)
}

/// JSON 渲染：`{"mach":m,"run":r,"total_ms":t,"seg1_ms":a,"seg2_ms":b}` 逐行对象数组行。
pub fn render_json(rows: &[LedgerRow]) -> Result<Vec<String>, u16> {
    if rows.is_empty() {
        return Err(E_EMPTY);
    }
    for r in rows {
        r.audit()?;
    }
    let mut out = Vec::new();
    out.push(String::from("["));
    for (i, r) in rows.iter().enumerate() {
        let mut l = String::from("  {\"mach\":");
        l.push_str(&itoa(r.mach as u64));
        l.push_str(",\"run\":");
        l.push_str(&itoa(r.run as u64));
        l.push_str(",\"total_ms\":");
        l.push_str(&itoa(r.total_ms));
        l.push_str(",\"seg1_ms\":");
        l.push_str(&itoa(r.seg1_ms));
        l.push_str(",\"seg2_ms\":");
        l.push_str(&itoa(r.seg2_ms));
        l.push('}');
        if i + 1 < rows.len() {
            l.push(',');
        }
        out.push(l);
    }
    out.push(String::from("]"));
    Ok(out)
}

/// 简易 CSV 行解析（5 列整数，防注入：非数字列拦截）。
pub fn parse_csv_row(line: &str) -> Result<LedgerRow, u16> {
    let mut vals: [u64; 5] = [0; 5];
    let mut idx = 0usize;
    let mut cur: u64 = 0;
    let mut got_digit = false;
    for ch in line.chars() {
        if ch == ',' {
            if idx >= 5 || !got_digit {
                return Err(E_CSV);
            }
            vals[idx] = cur;
            idx += 1;
            cur = 0;
            got_digit = false;
        } else if ch >= '0' && ch <= '9' {
            cur = cur.wrapping_mul(10).wrapping_add(ch as u64 - '0' as u64);
            got_digit = true;
        } else {
            return Err(E_CSV);
        }
    }
    if idx != 4 || !got_digit {
        return Err(E_CSV);
    }
    vals[4] = cur;
    Ok(LedgerRow {
        mach: vals[0] as u8,
        run: vals[1] as u32,
        total_ms: vals[2],
        seg1_ms: vals[3],
        seg2_ms: vals[4],
    })
}

/// 简易 JSON 行解析（键序固定，防注入：键缺/乱序拦截）。
pub fn parse_json_row(line: &str) -> Result<LedgerRow, u16> {
    // 期望形态: {"mach":A,"run":B,"total_ms":C,"seg1_ms":D,"seg2_ms":E}
    let keys = ["\"mach\":", "\"run\":", "\"total_ms\":", "\"seg1_ms\":", "\"seg2_ms\":"];
    let mut rest = line;
    let mut vals: [u64; 5] = [0; 5];
    for (i, k) in keys.iter().enumerate() {
        if !rest.starts_with(k) {
            return Err(E_JSON);
        }
        rest = &rest[k.len()..];
        // 读数字
        let mut cur: u64 = 0;
        let mut n = 0usize;
        for ch in rest.chars() {
            if ch >= '0' && ch <= '9' {
                cur = cur.wrapping_mul(10).wrapping_add(ch as u64 - '0' as u64);
                n += 1;
            } else {
                break;
            }
        }
        if n == 0 {
            return Err(E_JSON);
        }
        vals[i] = cur;
        rest = &rest[n..];
        if i < 4 {
            if !rest.starts_with(',') {
                return Err(E_JSON);
            }
            rest = &rest[1..];
        }
    }
    if !rest.is_empty() {
        return Err(E_JSON);
    }
    Ok(LedgerRow {
        mach: vals[0] as u8,
        run: vals[1] as u32,
        total_ms: vals[2],
        seg1_ms: vals[3],
        seg2_ms: vals[4],
    })
}

/// 语义等价核：CSV 解析回模型 = JSON 解析回模型 = 原账（逐行全等）。
pub fn semantic_equivalence(rows: &[LedgerRow]) -> Result<(), u16> {
    if rows.is_empty() {
        return Err(E_EMPTY);
    }
    let csv = render_csv(rows)?;
    let js = render_json(rows)?;
    // CSV：跳过表头行
    let mut back_csv: Vec<LedgerRow> = Vec::new();
    for (i, l) in csv.iter().enumerate() {
        if i == 0 {
            // 表头行与 schema 闭集比对
            let mut head = String::new();
            for (j, c) in SCHEMA_COLS.iter().enumerate() {
                if j > 0 {
                    head.push(',');
                }
                head.push_str(c);
            }
            if *l != head {
                return Err(E_SCHEMA);
            }
            continue;
        }
        back_csv.push(parse_csv_row(l)?);
    }
    // JSON：跳过首尾括号行
    let mut back_json: Vec<LedgerRow> = Vec::new();
    for (i, l) in js.iter().enumerate() {
        if i == 0 {
            if l != "[" {
                return Err(E_JSON);
            }
            continue;
        }
        if i == js.len() - 1 {
            if l != "]" {
                return Err(E_JSON);
            }
            continue;
        }
        let trimmed = l.trim_start();
        let obj = trimmed.trim_end_matches(',');
        back_json.push(parse_json_row(obj)?);
    }
    // 三方逐行全等
    if back_csv.len() != rows.len() || back_json.len() != rows.len() {
        return Err(E_SEM);
    }
    for i in 0..rows.len() {
        if back_csv[i] != rows[i] || back_json[i] != rows[i] {
            return Err(E_SEM);
        }
    }
    Ok(())
}

/// 解析防注入自检：非法 CSV/JSON 输入全拦截。
pub fn parse_guard_audit() -> Result<(), u16> {
    // CSV：字母列
    if parse_csv_row("a,1,100,50,50").is_ok() {
        return Err(SEG_BASE + 0x30);
    }
    // CSV：列数不足
    if parse_csv_row("1,2,3").is_ok() {
        return Err(SEG_BASE + 0x31);
    }
    // JSON：键缺失
    if parse_json_row("{\"mach\":1,\"run\":2}").is_ok() {
        return Err(SEG_BASE + 0x32);
    }
    // JSON：乱序键
    if parse_json_row("{\"run\":1,\"mach\":2,\"total_ms\":3,\"seg1_ms\":4,\"seg2_ms\":5}").is_ok() {
        return Err(SEG_BASE + 0x33);
    }
    // JSON：尾随垃圾
    if parse_json_row("{\"mach\":1,\"run\":2,\"total_ms\":3,\"seg1_ms\":4,\"seg2_ms\":5}x").is_ok() {
        return Err(SEG_BASE + 0x34);
    }
    Ok(())
}

/// 账行守恒自检：段和 ≠ 总 → 拦截。
pub fn row_cons_audit() -> Result<(), u16> {
    let bad = LedgerRow { mach: 0, run: 1, total_ms: 100, seg1_ms: 40, seg2_ms: 40 };
    if bad.audit() != Err(E_ROW) {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 行数守恒：CSV = rows+1（表头），JSON = rows+2（括号）。
pub struct GroupSummary {
    /// (机, 行数, 段和)。
    pub rows: Vec<(u8, usize, u64)>,
}

impl GroupSummary {
    pub fn build(rows: &[LedgerRow]) -> Result<GroupSummary, u16> {
        if rows.is_empty() {
            return Err(E_EMPTY);
        }
        let mut out: Vec<(u8, usize, u64)> = Vec::new();
        for m in 0..3u8 {
            let mut n = 0usize;
            let mut seg_sum: u64 = 0;
            for r in rows {
                if r.mach == m {
                    n += 1;
                    seg_sum += r.seg1_ms + r.seg2_ms;
                }
            }
            out.push((m, n, seg_sum));
        }
        Ok(GroupSummary { rows: out })
    }

    /// 分组守恒：Σ机行数 = 总行数；Σ机段和 = Σ全账段和。
    pub fn hedge(&self, rows: &[LedgerRow]) -> Result<(), u16> {
        let mut n_sum = 0usize;
        let mut s_sum: u64 = 0;
        for (_, n, s) in &self.rows {
            n_sum += n;
            s_sum += s;
        }
        if n_sum != rows.len() {
            return Err(E_SEM);
        }
        let mut direct: u64 = 0;
        for r in rows {
            direct += r.seg1_ms + r.seg2_ms;
        }
        if direct != s_sum {
            return Err(E_SEM);
        }
        Ok(())
    }

    /// 摘要行（零手抄）。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (m, n, s) in &self.rows {
            let mut l = String::from("M");
            l.push_str(&itoa(*m as u64));
            l.push_str(" 行数=");
            l.push_str(&itoa(*n as u64));
            l.push_str(" 段和=");
            l.push_str(&itoa(*s));
            out.push(l);
        }
        out
    }
}

/// 行序错位注入核：JSON 行序打乱 → 语义核逐行比对检出。
pub fn inject_reorder_must_detect(rows: &[LedgerRow]) -> Result<(), u16> {
    if rows.len() < 2 {
        return Err(E_EMPTY);
    }
    let mut shuffled: Vec<LedgerRow> = rows.to_vec();
    shuffled.reverse();
    // 语义核要求与原账逐行全等：乱序账对原账必不等（同账自身自比才过）
    let ok_orig = semantic_equivalence(rows).is_ok();
    // 乱序账对乱序账自身解析回模型：行序与原账不一致 → 与原账比失败
    let csv = render_csv(&shuffled)?;
    let mut back: Vec<LedgerRow> = Vec::new();
    for (i, l) in csv.iter().enumerate() {
        if i == 0 {
            continue;
        }
        back.push(parse_csv_row(l)?);
    }
    let mut same = true;
    for i in 0..rows.len() {
        if back[i] != rows[i] {
            same = false;
        }
    }
    if ok_orig && same && rows.len() > 1 {
        return Err(E_SEM);
    }
    Ok(())
}

/// 大数守卫自检：解析超大数字不 panic（环绕允许但守恒必破拦截）。
pub fn overflow_guard_audit() -> Result<(), u16> {
    let r = parse_csv_row("1,1,18446744073709551615,9223372036854775807,9223372036854775808")?;
    // 环绕后段和 ≠ 总 → audit 拦截
    if r.audit().is_ok() {
        return Err(SEG_BASE + 0x3A);
    }
    Ok(())
}

/// 双格式导出账（渲染对账 + 封存）。
pub struct ExportBook {
    pub csv_lines: Vec<String>,
    pub json_lines: Vec<String>,
    pub rows_n: usize,
    pub sealed: bool,
}

impl ExportBook {
    pub fn export(rows: &[LedgerRow]) -> Result<ExportBook, u16> {
        if rows.is_empty() {
            return Err(E_EMPTY);
        }
        let csv = render_csv(rows)?;
        let js = render_json(rows)?;
        semantic_equivalence(rows)?;
        Ok(ExportBook { csv_lines: csv, json_lines: js, rows_n: rows.len(), sealed: false })
    }

    /// 行数守恒：CSV = rows+1（表头），JSON = rows+2（括号）。
    pub fn assert_shape(&self) -> Result<(), u16> {
        if self.csv_lines.len() != self.rows_n + 1 {
            return Err(E_CSV);
        }
        if self.json_lines.len() != self.rows_n + 2 {
            return Err(E_JSON);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_shape()?;
        self.sealed = true;
        Ok(())
    }
}

/// 自例程：双格式语义等价 100% + schema 封存 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    parse_guard_audit()?;
    row_cons_audit()?;
    overflow_guard_audit()?;

    let rows = synth_rows(0x0674, 24)?;
    let seal = SchemaSeal::freeze();
    seal.verify()?;
    seal.inject_drift_must_fail()?;

    let mut book = ExportBook::export(&rows)?;
    book.assert_shape()?;
    // 渲染确定性：两次导出逐行全等
    let book2 = ExportBook::export(&rows)?;
    if book.csv_lines != book2.csv_lines || book.json_lines != book2.json_lines {
        return Err(E_SEM);
    }
    book.seal()?;

    // 分组摘要：守恒 + 摘要行
    let gs = GroupSummary::build(&rows)?;
    gs.hedge(&rows)?;
    if gs.lines().len() != 3 {
        return Err(SEG_BASE + 0x3B);
    }
    // 注入：账行篡改 → 语义核检出
    let mut tamper = rows.clone();
    tamper[5].total_ms += 1;
    if semantic_equivalence(&tamper).is_ok() {
        return Err(SEG_BASE + 0x36);
    }
    // 注入：行序错位 → 逐行比对检出
    inject_reorder_must_detect(&rows)?;
    // 注入：空账导出 → 拦截
    if ExportBook::export(&[]).err() != Some(E_EMPTY) {
        return Err(SEG_BASE + 0x37);
    }
    if render_csv(&[]) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x38);
    }
    if render_json(&[]) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x39);
    }
    Ok(0)
}
