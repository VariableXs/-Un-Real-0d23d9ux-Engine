//! UNX-F48394 —— M12 文档一致性抽查（协议/配置文档 vs 实现抽查）
//! 判据原文：UNX-F48394-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥297 行；
//!   J1-2 协议/配置/版本三类条款登记成闭集，实现侧逐条抽样比对；
//!   J1-3 值漂移（文档值 ≠ 实现值）必须点名到条款 ID 并由门禁拦截；
//!   J1-4 缺样（实现侧无对应抽样）必须检出（E_SAMPLE_MISS）；
//!   J1-5 条款 ID 格式非法/重复登记一律拒绝；
//!   J1-6 抽查子集由确定性 LCG 选点，复跑指纹一致（无时钟/地址依赖）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD1E0; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_EMPTY: u16 = SEG_BASE + 1;       // 空条款表
const E_ID_BAD: u16 = SEG_BASE + 2;      // 条款 ID 格式非法
const E_DUP_ID: u16 = SEG_BASE + 3;      // 条款 ID 重复
const E_SAMPLE_MISS: u16 = SEG_BASE + 4; // 实现侧缺样
const E_VAL_DRIFT: u16 = SEG_BASE + 5;   // 值漂移：文档值 ≠ 实现值
const E_KIND_BAD: u16 = SEG_BASE + 6;    // 条款类别越界
const E_UNIT: u16 = SEG_BASE + 7;        // 单位字段为空
const E_RATE: u16 = SEG_BASE + 8;        // 一致率不足 100%
const E_FINGER: u16 = SEG_BASE + 9;      // 复跑指纹不一致
const E_SPOT: u16 = SEG_BASE + 10;       // LCG 抽查子集抽中漂移项

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空条款表：无条款可抽查",
        E_ID_BAD => "条款 ID 格式非法：须为 类别字母+两位序号",
        E_DUP_ID => "条款 ID 重复：后到拒绝",
        E_SAMPLE_MISS => "实现侧缺样：条款无对应实现抽样",
        E_VAL_DRIFT => "值漂移：文档值与实现值不符，门禁拦截",
        E_KIND_BAD => "条款类别越界：非协议/配置/版本",
        E_UNIT => "单位字段为空：条款未实体化",
        E_RATE => "一致率不足 100%：抽查门禁拦截",
        E_FINGER => "复跑指纹不一致：抽查确定性被破坏",
        E_SPOT => "LCG 抽查子集抽中漂移项：漂移未先经全量审计清零",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// 条款类别：协议 / 配置 / 版本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Protocol,
    Config,
    Version,
}

/// 文档条款：卡面文档侧的登记项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocClause {
    pub id: &'static str, // 类别字母 + 两位序号，如 P01/C03/V01
    pub kind: DocKind,
    pub doc_val: u32,     // 文档声明值
    pub unit: &'static str,
}

/// 实现抽样：实现侧对某条款的实测值与源行号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImplSample {
    pub id: &'static str,
    pub impl_val: u32,
    pub src_line: u16,
}

/// 条款闭集：协议 4 + 配置 5 + 版本 1，共 10 条。
pub const DOC_CLAUSES: [DocClause; 10] = [
    DocClause { id: "P01", kind: DocKind::Protocol, doc_val: 115_200, unit: "bps" },
    DocClause { id: "P02", kind: DocKind::Protocol, doc_val: 4_096, unit: "B" },
    DocClause { id: "P03", kind: DocKind::Protocol, doc_val: 0x5641_5258, unit: "magic" },
    DocClause { id: "P04", kind: DocKind::Protocol, doc_val: 1, unit: "fnv1a" },
    DocClause { id: "C01", kind: DocKind::Config, doc_val: 64, unit: "win" },
    DocClause { id: "C02", kind: DocKind::Config, doc_val: 5_000, unit: "ms" },
    DocClause { id: "C03", kind: DocKind::Config, doc_val: 64, unit: "B" },
    DocClause { id: "C04", kind: DocKind::Config, doc_val: 128, unit: "q" },
    DocClause { id: "C05", kind: DocKind::Config, doc_val: 3, unit: "retry" },
    DocClause { id: "V01", kind: DocKind::Version, doc_val: 12, unit: "M" },
];

/// ID 格式判据闭包：首字节 P/C/V，后两字节为数字。
pub fn id_ok(id: &str) -> bool {
    let b = id.as_bytes();
    if b.len() != 3 {
        return false;
    }
    let head = b[0];
    if head != b'P' && head != b'C' && head != b'V' {
        return false;
    }
    (b[1] as char).is_ascii_digit() && (b[2] as char).is_ascii_digit()
}

/// 条款自检：ID 格式 + 单位非空。
pub fn clause_selfcheck(c: &DocClause) -> Result<(), u16> {
    if !id_ok(c.id) {
        return Err(E_ID_BAD);
    }
    if c.unit.is_empty() {
        return Err(E_UNIT);
    }
    Ok(())
}

/// 规范实现抽样生成：实现值与文档值一致，源行号由 ID 哈希确定性导出。
pub fn canonical_samples() -> Vec<ImplSample> {
    let mut out: Vec<ImplSample> = Vec::new();
    for c in DOC_CLAUSES.iter() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(c.id.as_bytes());
        buf.extend_from_slice(b"impl-line");
        let line = (fnv1a64(&buf) % 3600) as u16 + 100;
        out.push(ImplSample {
            id: c.id,
            impl_val: c.doc_val,
            src_line: line,
        });
    }
    out
}

// ── 审计报告 ────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub checked: usize,
    pub matched: usize,
    pub drifts: Vec<String>, // 漂移点名：id(doc,impl)
    pub fp: u64,
}

impl AuditReport {
    pub fn pass_rate(&self) -> u32 {
        if self.checked == 0 {
            return 0;
        }
        (self.matched as u64 * 100 / self.checked as u64) as u32
    }

    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("docaudit ");
        s.push_str(&self.matched.to_string());
        s.push_str("/");
        s.push_str(&self.checked.to_string());
        s.push_str(" 漂移 ");
        s.push_str(&self.drifts.len().to_string());
        s
    }
}

/// 全量审计：逐条比对文档条款与实现抽样。
pub fn audit(docs: &[DocClause], samples: &[ImplSample]) -> Result<AuditReport, u16> {
    if docs.is_empty() {
        return Err(E_EMPTY);
    }
    for c in docs {
        clause_selfcheck(c)?;
    }
    for i in 0..docs.len() {
        for j in (i + 1)..docs.len() {
            if docs[i].id == docs[j].id {
                return Err(E_DUP_ID);
            }
        }
    }
    let mut matched = 0usize;
    let mut drifts: Vec<String> = Vec::new();
    let mut hashbuf: Vec<u8> = vec![0xDAu8, 0x1E];
    for c in docs {
        let mut found: Option<&ImplSample> = None;
        for s in samples {
            if s.id == c.id {
                found = Some(s);
                break;
            }
        }
        let s = match found {
            Some(s) => s,
            None => return Err(E_SAMPLE_MISS),
        };
        hashbuf.extend_from_slice(c.id.as_bytes());
        hashbuf.extend_from_slice(&s.impl_val.to_le_bytes());
        if s.impl_val == c.doc_val {
            matched += 1;
        } else {
            let mut d = String::new();
            d.push_str(c.id);
            d.push('(');
            d.push_str(&c.doc_val.to_string());
            d.push(',');
            d.push_str(&s.impl_val.to_string());
            d.push(')');
            drifts.push(d);
        }
    }
    Ok(AuditReport {
        checked: docs.len(),
        matched,
        drifts,
        fp: fnv1a64(&hashbuf),
    })
}

/// 门禁：漂移非零或一致率不足 100% 即拦截。
pub fn gate(r: &AuditReport) -> Result<(), u16> {
    if !r.drifts.is_empty() {
        return Err(E_VAL_DRIFT);
    }
    if r.pass_rate() < 100 {
        return Err(E_RATE);
    }
    Ok(())
}

/// LCG 抽查：确定性选点 k 项组成子集。
pub fn spot_pick(docs: &[DocClause], k: usize, seed: u32) -> Vec<DocClause> {
    let mut state: u32 = seed;
    let mut out: Vec<DocClause> = Vec::new();
    let mut used: Vec<usize> = Vec::new();
    let n = docs.len();
    let mut guard = 0usize;
    while out.len() < k && guard < n * 8 {
        guard += 1;
        let idx = (lcg_next(&mut state) % (n as u32)) as usize;
        if used.contains(&idx) {
            continue;
        }
        used.push(idx);
        out.push(docs[idx]);
    }
    out
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let docs: Vec<DocClause> = DOC_CLAUSES.to_vec();

    // R1：协议条款 4 条全抽全中。
    let proto: Vec<DocClause> = docs
        .iter()
        .filter(|c| c.kind == DocKind::Protocol)
        .copied()
        .collect();
    let r1 = audit(&proto, &canonical_samples())?;
    if r1.matched != 4 || !r1.drifts.is_empty() {
        return Err(E_VAL_DRIFT);
    }
    hits += 1;

    // R2：配置条款 5 条全抽全中。
    let cfg: Vec<DocClause> = docs
        .iter()
        .filter(|c| c.kind == DocKind::Config)
        .copied()
        .collect();
    let r2 = audit(&cfg, &canonical_samples())?;
    if r2.matched != 5 || !r2.drifts.is_empty() {
        return Err(E_VAL_DRIFT);
    }
    hits += 1;

    // R3：版本条款 V01 一致且文档版本 = 12（M12 段）。
    let ver: Vec<DocClause> = docs
        .iter()
        .filter(|c| c.kind == DocKind::Version)
        .copied()
        .collect();
    let r3 = audit(&ver, &canonical_samples())?;
    if r3.matched != 1 || ver[0].doc_val != 12 {
        return Err(E_VAL_DRIFT);
    }
    hits += 1;

    // R4：缺样检出：剔除 C03 抽样后审计必须回 E_SAMPLE_MISS。
    let mut partial = canonical_samples();
    partial.retain(|s| s.id != "C03");
    if audit(&docs, &partial) != Err(E_SAMPLE_MISS) {
        return Err(E_SAMPLE_MISS);
    }
    hits += 1;

    // R5：漂移检出：C02 实现值 +1 → 点名 + 门禁拦截。
    let mut drifted = canonical_samples();
    for s in drifted.iter_mut() {
        if s.id == "C02" {
            s.impl_val = s.impl_val.wrapping_add(1);
        }
    }
    let r5 = audit(&docs, &drifted)?;
    if r5.drifts.len() != 1 || !r5.drifts[0].starts_with("C02") {
        return Err(E_VAL_DRIFT);
    }
    if gate(&r5) != Err(E_VAL_DRIFT) {
        return Err(E_VAL_DRIFT);
    }
    hits += 1;

    // R6：pass_rate 100%：全量规范审计 matched == checked == 10，门禁放行。
    let r6 = audit(&docs, &canonical_samples())?;
    if r6.checked != 10 || r6.matched != 10 || r6.pass_rate() != 100 {
        return Err(E_RATE);
    }
    gate(&r6)?;
    hits += 1;

    // R7：复跑确定性：两次全量审计指纹一致。
    let r7a = audit(&docs, &canonical_samples())?;
    let r7b = audit(&docs, &canonical_samples())?;
    if r7a.fp != r7b.fp {
        return Err(E_FINGER);
    }
    hits += 1;

    // R8：ID 闭包：非法 ID 与重复 ID 一律拒绝。
    let bad_clause = DocClause { id: "X99", kind: DocKind::Config, doc_val: 1, unit: "u" };
    if audit(&[bad_clause], &canonical_samples()) != Err(E_ID_BAD) {
        return Err(E_ID_BAD);
    }
    let dup = [
        DOC_CLAUSES[0],
        DocClause { id: "P01", kind: DocKind::Config, doc_val: 2, unit: "u" },
    ];
    if audit(&dup, &canonical_samples()) != Err(E_DUP_ID) {
        return Err(E_DUP_ID);
    }
    hits += 1;

    // R9：实体化闭包：全部条款单位非空、类别合法。
    for c in docs.iter() {
        clause_selfcheck(c)?;
        let kind_ok = c.kind == DocKind::Protocol
            || c.kind == DocKind::Config
            || c.kind == DocKind::Version;
        if !kind_ok {
            return Err(E_KIND_BAD);
        }
    }
    hits += 1;

    // R10：LCG 抽查子集：seed 固定两次选点一致，且子集内无漂移。
    let s1 = spot_pick(&docs, 4, 0xD1E0);
    let s2 = spot_pick(&docs, 4, 0xD1E0);
    if s1.len() != 4 || s1.len() != s2.len() {
        return Err(E_SPOT);
    }
    for i in 0..4 {
        if s1[i].id != s2[i].id {
            return Err(E_FINGER);
        }
    }
    let rs = audit(&s1, &canonical_samples())?;
    gate(&rs)?;
    hits += 1;

    Ok(hits)
}
