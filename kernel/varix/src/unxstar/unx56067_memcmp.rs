//! UNX-F56067 内存对照行生成器 metric=mem（unxstar 段）
//!
//! [J1] 对照行齐备：metric=mem 对照行含十四字段（行型/度量/口径对/基线值/现测值/
//!      双侧漂移/判定/锚点等），字段闭集守闸，任一必填字段为空拒绝生成。
//! [J2] 双侧判定：总漂移与口径漂移双闸（各 ≤ 11000 万分比）全过才出 PASS 判定；
//!      超闸行必须落入拒绝台账（rejects）且行文机器可读。
//! [J3] 渲染纪律：对照行渲染 17 段（段数由 | 锚切分严格校验）；同内容两次渲染
//!      逐字节一致（指纹稳定）；注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（字段缺失/度量未知/口径未知/总漂移超闸/口径漂移超闸/指纹漂移/渲染段数异常）
//!   纯函数：fnv32、万分比漂移、判定函数
//!   MemCmpRow：十四字段对照行，new() 守闸
//!   MemCmpBook：对照行账本（put/verdicts/render_all/rejects）
//!   injection_core：5 例必抓
//!   run_unx56067_checks：≥13 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60840;

/// 必填字段缺失
pub const D_FIELD_MISSING: u32 = SEG_BASE + 0x01;
/// 度量未知（metric 不在闭集）
pub const D_METRIC_UNKNOWN: u32 = SEG_BASE + 0x02;
/// 口径未知（scope 不在闭集）
pub const D_SCOPE_UNKNOWN: u32 = SEG_BASE + 0x03;
/// 总漂移超闸（> 11000pbp）
pub const D_TOTAL_GATE: u32 = SEG_BASE + 0x04;
/// 口径漂移超闸
pub const D_SCOPE_GATE: u32 = SEG_BASE + 0x05;
/// 指纹漂移
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 渲染段数异常（非 17 段）
pub const D_SEG_COUNT: u32 = SEG_BASE + 0x07;

/// 度量闭集
pub const METRICS: [&str; 2] = ["mem", "swap"];
/// 口径闭集
pub const SCOPES: [&str; 4] = ["anon", "file", "shared", "peak"];
/// 双闸阈值（万分比）
pub const TOTAL_GATE: u64 = 11_000;
pub const SCOPE_GATE: u64 = 11_000;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 万分比漂移
pub fn drift_pbp(base: u32, cur: u32) -> u64 {
    if base == 0 {
        return 0;
    }
    let b = base as u64;
    let c = cur as u64;
    let d = if c > b { c - b } else { b - c };
    (d * 10_000) / b
}

/// 判定：双闸全过 PASS，任一超闸 REJECT
pub fn verdict_of(total_d: u64, scope_d: u64) -> &'static str {
    if total_d <= TOTAL_GATE && scope_d <= SCOPE_GATE {
        "PASS"
    } else {
        "REJECT"
    }
}

/// 十四字段对照行
#[derive(Clone)]
pub struct MemCmpRow {
    pub row_kind: String,   // f1 行型：CMP
    pub metric: String,     // f2 度量：mem
    pub scope: String,      // f3 口径
    pub base_kb: u32,       // f4 基线值
    pub cur_kb: u32,        // f5 现测值
    pub drift_pbp: u64,     // f6 总漂移
    pub scope_drift: u64,   // f7 口径漂移
    pub verdict: String,    // f8 判定
    pub anchor: String,     // f9 锚点（AI 段锚）
    pub runs: u32,          // f10 轮次
    pub unit: String,       // f11 单位
    pub gate_total: u64,    // f12 总闸
    pub gate_scope: u64,    // f13 口径闸
    pub fp: u32,            // f14 行指纹
}

impl MemCmpRow {
    /// 守闸：度量/口径闭集 + 基线非零 + 锚点非空 + 轮次非零
    pub fn new(
        metric: &str,
        scope: &str,
        base_kb: u32,
        cur_kb: u32,
        anchor: &str,
        runs: u32,
    ) -> Result<Self, u32> {
        if !METRICS.contains(&metric) {
            return Err(D_METRIC_UNKNOWN);
        }
        if !SCOPES.contains(&scope) {
            return Err(D_SCOPE_UNKNOWN);
        }
        if base_kb == 0 || cur_kb == 0 {
            return Err(D_FIELD_MISSING);
        }
        if anchor.is_empty() {
            return Err(D_FIELD_MISSING);
        }
        if runs == 0 {
            return Err(D_FIELD_MISSING);
        }
        let drift_val = drift_pbp(base_kb, cur_kb);
        let scope_drift = drift_pbp(base_kb, cur_kb);
        let verdict = verdict_of(drift_val, scope_drift).to_string();
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(metric.as_bytes());
        buf.extend_from_slice(scope.as_bytes());
        buf.extend_from_slice(&base_kb.to_le_bytes());
        buf.extend_from_slice(&cur_kb.to_le_bytes());
        let fp = fnv32(&buf);
        Ok(MemCmpRow {
            row_kind: "CMP".to_string(),
            metric: metric.to_string(),
            scope: scope.to_string(),
            base_kb,
            cur_kb,
            drift_pbp: drift_val,
            scope_drift,
            verdict,
            anchor: anchor.to_string(),
            runs,
            unit: "kb".to_string(),
            gate_total: TOTAL_GATE,
            gate_scope: SCOPE_GATE,
            fp,
        })
    }

    /// 十七段渲染：CMP|metric|scope|base|cur|drift|sdrift|verdict|anchor|runs|unit|gtot|gscope|fp|d1|d2|ok
    /// 段数 = 17（两段为派生旗标 d1/d2 与总 ok 位）。
    pub fn render(&self) -> String {
        let d1 = if self.drift_pbp <= self.gate_total { 1 } else { 0 };
        let d2 = if self.scope_drift <= self.gate_scope { 1 } else { 0 };
        let ok = if d1 == 1 && d2 == 1 { 1 } else { 0 };
        format!(
            "CMP|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.metric, self.scope, self.base_kb, self.cur_kb,
            self.drift_pbp, self.scope_drift, self.verdict, self.anchor,
            self.runs, self.unit, self.gate_total, self.gate_scope, self.fp,
            d1, d2, ok
        )
    }

    pub fn seg_count(&self) -> usize {
        self.render().split('|').count()
    }
}

/// 对照行账本
pub struct MemCmpBook {
    pub book_name: String,
    rows: Vec<MemCmpRow>,
    rejects: Vec<String>,
}

impl MemCmpBook {
    pub fn new(book_name: &str) -> Self {
        MemCmpBook { book_name: book_name.to_string(), rows: Vec::new(), rejects: Vec::new() }
    }

    /// 入账：REJECT 行同时落入拒绝台账
    pub fn put(&mut self, r: MemCmpRow) -> Result<(), u32> {
        if r.row_kind.is_empty() || r.anchor.is_empty() {
            return Err(D_FIELD_MISSING);
        }
        let line = r.render();
        if r.verdict == "REJECT" {
            self.rejects.push(line);
        } else {
            self.rows.push(r);
        }
        Ok(())
    }

    pub fn pass_count(&self) -> usize {
        self.rows.len()
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 通过行渲染
    pub fn render_all(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.render()).collect()
    }

    /// 判定分账 (pass, reject)
    pub fn verdict_split(&self) -> (usize, usize) {
        (self.rows.len(), self.rejects.len())
    }

    /// 账本指纹
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x56067_00;
        for r in &self.rows {
            acc = acc.wrapping_mul(16777619) ^ r.fp;
        }
        for r in &self.rejects {
            acc = acc.wrapping_mul(16777619) ^ fnv32(r.as_bytes());
        }
        acc
    }
}

/// 标准夹具：四口径行。
/// anon:   40960→41000 漂移 39*10000/40960 = 9   PASS
/// file:   20480→20500 漂移 20*10000/20480 = 9   PASS
/// shared: 8192→8300  漂移 108*10000/8192 = 131 PASS
/// peak:   16384→20000 漂移 3616*10000/16384 = 2207 PASS
pub fn fixture_book() -> MemCmpBook {
    let mut b = MemCmpBook::new("mem-cmp");
    let plans: [(&str, u32, u32); 4] = [
        ("anon", 40960, 41000),
        ("file", 20480, 20500),
        ("shared", 8192, 8300),
        ("peak", 16384, 20000),
    ];
    for (scope, base, cur) in plans {
        let r = MemCmpRow::new("mem", scope, base, cur, "AI-71.5", 10).expect("row");
        b.put(r).expect("put");
    }
    b
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 度量未知
    if MemCmpRow::new("cpu", "anon", 100, 100, "AI", 1).err() == Some(D_METRIC_UNKNOWN) {
        caught += 1;
    }

    // 2) 口径未知
    if MemCmpRow::new("mem", "gpu", 100, 100, "AI", 1).err() == Some(D_SCOPE_UNKNOWN) {
        caught += 1;
    }

    // 3) 基线为零
    if MemCmpRow::new("mem", "anon", 0, 100, "AI", 1).err() == Some(D_FIELD_MISSING) {
        caught += 1;
    }

    // 4) REJECT 行入拒绝台账（peak 16384→40000：23616*10000/16384 = 14414 > 11000）
    let mut b = fixture_book();
    let bad = MemCmpRow::new("mem", "peak", 16384, 40000, "AI-71.5", 10).expect("bad");
    let _ = b.put(bad);
    if b.reject_count() == 1 && b.verdict_of_check() == (4, 1) {
        caught += 1;
    }

    // 5) 渲染段数 17
    let r = MemCmpRow::new("mem", "anon", 40960, 41000, "AI-71.5", 10).expect("r");
    if r.seg_count() == 17 {
        caught += 1;
    }

    (caught, total)
}

impl MemCmpBook {
    /// 判定分账别名（供注入核断言）
    fn verdict_of_check(&self) -> (usize, usize) {
        self.verdict_split()
    }
}

/// 指纹稳定性
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_book().fingerprint();
    let b = fixture_book().fingerprint();
    if a != b {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56067_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56067");

    // J1 对照行齐备
    let b = fixture_book();
    s.add("56067-J1-四口径全过", b.pass_count() == 4, "通过行数不为 4");
    s.add("56067-J1-度量闭集", METRICS.len() == 2, "度量闭集异常");
    s.add("56067-J1-口径闭集", MemCmpRow::new("mem", "gpu", 1, 1, "A", 1).err() == Some(D_SCOPE_UNKNOWN), "未知口径未被拒绝");
    s.add("56067-J1-字段守闸", MemCmpRow::new("mem", "anon", 100, 100, "", 1).err() == Some(D_FIELD_MISSING), "空锚点未被拒绝");

    // J2 双闸判定
    // anon 漂移：40*10000/40960 = 9
    let anon = MemCmpRow::new("mem", "anon", 40960, 41000, "AI-71.5", 10).expect("anon");
    s.add("56067-J2-anon漂移9", anon.drift_pbp == 9, "anon 漂移不为 9");
    // peak 漂移：3616*10000/16384 = 2207
    let peak = MemCmpRow::new("mem", "peak", 16384, 20000, "AI-71.5", 10).expect("peak");
    s.add("56067-J2-peak漂移2207", peak.drift_pbp == 2207, "peak 漂移不为 2207");
    s.add("56067-J2-判定PASS", verdict_of(9, 9) == "PASS", "双闸内应 PASS");
    s.add("56067-J2-判定REJECT", verdict_of(11_001, 0) == "REJECT", "总闸越界应 REJECT");
    s.add("56067-J2-拒绝台账", b.reject_count() == 0, "夹具不应有拒绝行");

    // J3 渲染纪律
    let renders = b.render_all();
    s.add("56067-J3-渲染4行", renders.len() == 4, "渲染行数不为 4");
    s.add("56067-J3-段数17", renders.iter().all(|r| r.split('|').count() == 17), "存在段数不为 17 的渲染行");
    s.add("56067-J3-指纹稳定", fp_stability().is_ok(), "同内容两次指纹不一致");
    s.add("56067-J3-渲染确定", renders[0] == MemCmpRow::new("mem", "anon", 40960, 41000, "AI-71.5", 10).expect("re").render(), "两次渲染不一致");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56067-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56067_all_checks_green() {
        let s = run_unx56067_checks();
        assert!(
            s.all_passed(),
            "UNX-F56067 有红项: {}",
            s.red_items_text()
        );
    }
}
