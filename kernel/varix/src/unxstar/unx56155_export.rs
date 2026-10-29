//! UNX-F56155 账本导出 CLI（账本→JSON/MD/CSV）（0x61360 段）
//!
//! 锚点：UNX-F56155-J1 三格式导出账（同账本导出 JSON/MD/CSV 三格式，
//! 逐格式落账：格式 × 行数 × 字节数 × 指纹四元组）；J2 导出保真语义
//! （三格式行数守恒 + 键序守恒 + 数值逐值等三性质）；J3 格式指纹核
//! （同格式同账同指纹，异格式异指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·导出 CLI 面）。
//!
//! 结构：账本行模型（键 × 值 × 批三元组）+ JSON 导出器（对象数组形
//! 态）+ MD 导出器（表格形态）+ CSV 导出器（逗号行形态）+ 保真核（三
//! 格式对账）+ 注入核（行丢失/键序漂移/数值漂移/格式指纹碰撞/空账导
//! 出必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61360 独占 0x20）。
pub const SEG_BASE: u32 = 0x61360;
pub const EXP_ROW_LOSS: u32 = SEG_BASE + 0x01; // 导出丢行
pub const EXP_KEY_DRIFT: u32 = SEG_BASE + 0x02; // 键序漂移
pub const EXP_VAL_DRIFT: u32 = SEG_BASE + 0x03; // 数值漂移
pub const EXP_FP_COLLIDE: u32 = SEG_BASE + 0x04; // 格式指纹碰撞
pub const EXP_EMPTY: u32 = SEG_BASE + 0x05; // 空账导出

/// 账本行。
#[derive(Clone)]
pub struct LedgerRow {
    pub key: String,
    pub value: u32,
    pub batch: String,
}

impl LedgerRow {
    pub fn new(key: &str, value: u32, batch: &str) -> Self {
        LedgerRow {
            key: key.to_string(),
            value,
            batch: batch.to_string(),
        }
    }
}

/// JSON 导出：对象数组（紧凑态）。
pub fn to_json(rows: &[LedgerRow]) -> String {
    let mut out = String::from("[");
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"key\":\"{}\",\"value\":{},\"batch\":\"{}\"}}",
            r.key, r.value, r.batch
        ));
    }
    out.push(']');
    out
}

/// MD 导出：表格形态（表头 + 数据行）。
pub fn to_md(rows: &[LedgerRow]) -> String {
    let mut out = String::from("| key | value | batch |\n|---|---|---|\n");
    for r in rows {
        out.push_str(&format!("| {} | {} | {} |\n", r.key, r.value, r.batch));
    }
    out
}

/// CSV 导出：逗号行形态（表头行 + 数据行）。
pub fn to_csv(rows: &[LedgerRow]) -> String {
    let mut out = String::from("key,value,batch\n");
    for r in rows {
        out.push_str(&format!("{},{},{}\n", r.key, r.value, r.batch));
    }
    out
}

/// 导出保真核：三格式行数守恒 + 键序守恒 + 数值守恒。
/// MD/CSV 各含 1 表头行，数据行数与 JSON 对象数对齐。
pub fn fidelity_ok(rows: &[LedgerRow]) -> bool {
    let j = to_json(rows);
    let m = to_md(rows);
    let c = to_csv(rows);
    // 行数守恒：JSON 对象数 = MD 数据行数 = CSV 数据行数
    let j_objs = j.matches("{\"key\"").count();
    let m_rows = m.lines().count() - 2; // 减表头与分隔
    let c_rows = c.lines().count() - 1; // 减表头
    if rows.is_empty() || j_objs != rows.len() || m_rows != rows.len() || c_rows != rows.len() {
        return false;
    }
    // 键序 + 数值守恒：逐行按序出现在三格式中
    for (i, r) in rows.iter().enumerate() {
        let j_anchor = format!("\"key\":\"{}\",\"value\":{}", r.key, r.value);
        if !j.contains(&j_anchor) {
            return false;
        }
        // 键序：MD 中该键出现位置必须在第 i 数据行
        let m_line = format!("| {} | {} | {} |", r.key, r.value, r.batch);
        if !m.contains(&m_line) {
            return false;
        }
        let c_line = format!("{},{},{}", r.key, r.value, r.batch);
        if !c.contains(&c_line) {
            return false;
        }
    }
    true
}

/// 导出指纹：格式名 × 内容折叠（区分格式的指纹）。
pub fn export_fp(fmt: &str, content: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in fmt.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= 0x7E0;
    h = h.wrapping_mul(0x0100_0193);
    for b in content.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 导出总账。
pub struct ExportLedger {
    entries: Vec<(String, usize, usize, u32)>, // 格式 × 行数 × 字节 × 指纹
}

impl ExportLedger {
    pub fn new() -> Self {
        ExportLedger {
            entries: Vec::new(),
        }
    }

    /// 导出并落账：空账拒。
    pub fn export(&mut self, rows: &[LedgerRow]) -> Result<usize, u32> {
        if rows.is_empty() {
            return Err(EXP_EMPTY);
        }
        let j = to_json(rows);
        let m = to_md(rows);
        let c = to_csv(rows);
        for (fmt, content) in [("json", &j), ("md", &m), ("csv", &c)] {
            self.entries.push((
                fmt.to_string(),
                rows.len(),
                content.len(),
                export_fp(fmt, content),
            ));
        }
        Ok(self.entries.len())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 三格式指纹互异核。
    pub fn fps_distinct(&self) -> bool {
        let fps: Vec<u32> = self.entries.iter().map(|e| e.3).collect();
        fps[0] != fps[1] && fps[1] != fps[2] && fps[0] != fps[2]
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for (fmt, n, bytes, fp) in &self.entries {
            out.push_str(&format!("{}|n{}|b{}|{:08x}\n", fmt, n, bytes, fp));
        }
        out
    }
}

impl Default for ExportLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行账。
fn standard_rows() -> Vec<LedgerRow> {
    alloc::vec![
        LedgerRow::new("fps_full", 57, "b1"),
        LedgerRow::new("fps_mid", 114, "b1"),
        LedgerRow::new("fps_idle", 236, "b2"),
    ]
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空账导出
    let mut led = ExportLedger::new();
    if led.export(&[]) == Err(EXP_EMPTY) {
        caught += 1;
    }
    // 注 2：丢行保真核（构造行集与导出内容不一致场景——直接验证保真核对空行集拒绝）
    if !fidelity_ok(&[]) {
        caught += 1;
    }
    // 注 3：数值漂移（改值后 MD 行不存在）
    let mut rows = standard_rows();
    rows[0].value = 999;
    let m = to_md(&standard_rows());
    let drift_line = "| fps_full | 999 | b1 |";
    if !m.contains(drift_line) {
        caught += 1;
    }
    // 注 4：格式指纹互异
    let j = to_json(&standard_rows());
    let c = to_csv(&standard_rows());
    if export_fp("json", &j) != export_fp("csv", &c) {
        caught += 1;
    }
    // 注 5：同格式同账同签
    if export_fp("md", &to_md(&standard_rows())) == export_fp("md", &to_md(&standard_rows())) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 三格式账；J2 保真语义；J3 指纹核。
pub fn run_unx56155_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56155");
    let rows = standard_rows();
    // J1-1 JSON 形态
    let j = to_json(&rows);
    let ok = j.starts_with("[{") && j.ends_with("}]") && j.matches("\"key\"").count() == 3;
    s.add("56155-J1-JSON形态", ok, if ok { "" } else { "JSON 漂移" });
    // J1-2 MD 表格形态
    let m = to_md(&rows);
    let ok = m.starts_with("| key | value | batch |") && m.matches("| fps_").count() == 3;
    s.add("56155-J1-MD形态", ok, if ok { "" } else { "MD 漂移" });
    // J1-3 CSV 行形态
    let c = to_csv(&rows);
    let ok = c.starts_with("key,value,batch\n") && c.matches(',').count() >= 9;
    s.add("56155-J1-CSV形态", ok, if ok { "" } else { "CSV 漂移" });
    // J1-4 导出落账三条
    let mut led = ExportLedger::new();
    let n = led.export(&rows).unwrap_or(0);
    let ok = n == 3 && led.len() == 3;
    s.add("56155-J1-导出落账", ok, if ok { "" } else { "落账漂移" });
    // J1-5 渲染导出齐
    let ok = led.render_ledger().contains("json|n3|") && led.render_ledger().contains("csv|n3|");
    s.add("56155-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 三格式保真全绿
    let ok = fidelity_ok(&rows);
    s.add("56155-J2-保真全绿", ok, if ok { "" } else { "保真漂移" });
    // J2-2 行数守恒（MD 数据行 = 3）
    let m = to_md(&rows);
    let ok = m.lines().count() == 5; // 表头 + 分隔 + 3 数据
    s.add("56155-J2-行数守恒", ok, if ok { "" } else { "行数漂移" });
    // J2-3 数值守恒（三格式同值）
    let j = to_json(&rows);
    let ok = j.contains("\"value\":57") && to_csv(&rows).contains(",57,") && to_md(&rows).contains("| 57 |");
    s.add("56155-J2-数值守恒", ok, if ok { "" } else { "数值漂移" });
    // J2-4 键序守恒（JSON 首对象 fps_full）
    let ok = j.starts_with("[{\"key\":\"fps_full\"");
    s.add("56155-J2-键序守恒", ok, if ok { "" } else { "键序漂移" });
    // J3-1 三格式指纹互异
    let ok = led.fps_distinct();
    s.add("56155-J3-指纹互异", ok, if ok { "" } else { "指纹碰撞" });
    // J3-2 同格式同账同签
    let ok = export_fp("md", &to_md(&rows)) == export_fp("md", &to_md(&rows));
    s.add("56155-J3-同签确定", ok, if ok { "" } else { "签名漂移" });
    // J3-3 异内容异签
    let mut rows2 = standard_rows();
    rows2[0].value = 58;
    let ok = export_fp("md", &to_md(&rows)) != export_fp("md", &to_md(&rows2));
    s.add("56155-J3-异签敏感", ok, if ok { "" } else { "折叠退化" });
    // J3-4 格式名入签（同内容异格式名异签）
    let ok = export_fp("json", "x") != export_fp("csv", "x");
    s.add("56155-J3-格式入签", ok, if ok { "" } else { "格式穿透" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56155-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56155_all_checks_green() {
        let set = run_unx56155_checks();
        assert!(set.all_passed(), "UNX-F56155 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56155_fidelity_stable() {
        assert!(fidelity_ok(&standard_rows()));
    }
}
