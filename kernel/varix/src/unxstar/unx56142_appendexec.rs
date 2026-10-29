//! UNX-F56142 账本只追加不改写执行器（禁账后改判基座）（0x611C0 段）
//!
//! 锚点：UNX-F56142-J1 只追加执行语义（追加成功 + 改写必拒 + 删除无
//! 接口三性质）；J2 追加序账（行号连续 + 内容指纹链逐行锁定）；J3 撤
//! 销语义替代（改判走追加更实行，旧行保留可追溯）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·只追加基座面）。
//!
//! 结构：只追加执行器（append 单一写入口）+ 内容指纹链（逐行 FNV 与
//! 前行折叠，篡改任意行后续链全断）+ 更实行核（supersede 追加更实行
//! 并锁定旧行）+ 链验证核（全链重算一致）+ 注入核（改写尝试/链篡改/
//! 越行号/更实行再改/空行追加必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x611C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x611C0;
pub const APP_MUTATE: u32 = SEG_BASE + 0x01; // 改写尝试（只追加语义被破）
pub const APP_CHAIN_BREAK: u32 = SEG_BASE + 0x02; // 指纹链断裂（篡改捕获）
pub const APP_ROW_RANGE: u32 = SEG_BASE + 0x03; // 越行号引用
pub const APP_SUP_REWRITE: u32 = SEG_BASE + 0x04; // 更实行再改
pub const APP_EMPTY_ROW: u32 = SEG_BASE + 0x05; // 空行追加

/// 行内容指纹：FNV-1a（与前行折叠成链）。
fn row_fp(prev: u32, content: &str) -> u32 {
    let mut h = prev ^ 0x0100_0193;
    for b in content.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 账本行。
#[derive(Clone)]
pub struct AppendRow {
    pub row_no: u32,
    pub content: String,
    pub fp: u32,
    pub superseded_by: Option<u32>,
}

/// 只追加执行器。
pub struct AppendExecutor {
    rows: Vec<AppendRow>,
    chain_head: u32,
}

impl AppendExecutor {
    pub fn new() -> Self {
        AppendExecutor {
            rows: Vec::new(),
            chain_head: 0x811C_9DC5,
        }
    }

    /// 追加一行：唯一写入口。空内容拒。
    pub fn append(&mut self, content: &str) -> Result<u32, u32> {
        if content.is_empty() {
            return Err(APP_EMPTY_ROW);
        }
        let no = self.rows.len() as u32 + 1;
        let fp = row_fp(self.chain_head, content);
        self.rows.push(AppendRow {
            row_no: no,
            content: content.to_string(),
            fp,
            superseded_by: None,
        });
        self.chain_head = fp;
        Ok(no)
    }

    /// 更实行：锁定旧行（superseded_by 置位）+ 追加更实行。
    /// 旧行必须存在且未被更过；更实行内容带 "SUPERSEDES:r{old}" 前缀由调用方约定。
    pub fn supersede(&mut self, old_no: u32, new_content: &str) -> Result<u32, u32> {
        if old_no == 0 || old_no as usize > self.rows.len() {
            return Err(APP_ROW_RANGE);
        }
        let idx = old_no as usize - 1;
        if self.rows[idx].superseded_by.is_some() {
            return Err(APP_SUP_REWRITE);
        }
        if new_content.is_empty() {
            return Err(APP_EMPTY_ROW);
        }
        let no = self.append(&format!("SUPERSEDES:r{}|{}", old_no, new_content))?;
        self.rows[idx].superseded_by = Some(no);
        Ok(no)
    }

    /// 改写尝试核：执行器无任何原位改写接口（类型级保证的账面表达——
    /// content 字段仅在本模块构造时写入；此处验证已存行内容不可达改动）。
    pub fn row_content(&self, no: u32) -> Option<&str> {
        if no == 0 || no as usize > self.rows.len() {
            return None;
        }
        Some(&self.rows[no as usize - 1].content)
    }

    pub fn row(&self, no: u32) -> Option<&AppendRow> {
        if no == 0 || no as usize > self.rows.len() {
            return None;
        }
        Some(&self.rows[no as usize - 1])
    }

    /// 链验证核：全链重算（从种子逐行折叠）与存量指纹一致。
    pub fn chain_ok(&self) -> bool {
        let mut h = 0x811C_9DC5;
        for r in &self.rows {
            h = row_fp(h, &r.content);
            if h != r.fp {
                return false;
            }
        }
        true
    }

    /// 更实链核：被更行全部锁定且指向存在的更实行。
    pub fn supersede_chain_ok(&self) -> bool {
        self.rows.iter().all(|r| match r.superseded_by {
            None => true,
            Some(t) => t > r.row_no && t as usize <= self.rows.len(),
        })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 渲染导出：逐行 + 锁定标记。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&format!(
                "r{}|{}|{:08x}|{}",
                r.row_no,
                r.content,
                r.fp,
                if r.superseded_by.is_some() { "LOCKED" } else { "LIVE" }
            ));
            out.push('\n');
        }
        out
    }
}

impl Default for AppendExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行账（第 2 行被第 4 行更实）。
fn standard_executor() -> AppendExecutor {
    let mut ex = AppendExecutor::new();
    let _ = ex.append("full_load|60|57|9500");
    let r2 = ex.append("mid_load|120|114|9500").unwrap_or(0);
    let _ = ex.append("idle|240|236|9833");
    let _ = ex.supersede(r2, "mid_load|120|116|9667");
    ex
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空行追加
    let mut ex = AppendExecutor::new();
    if ex.append("") == Err(APP_EMPTY_ROW) {
        caught += 1;
    }
    // 注 2：越行号引用
    let mut ex = AppendExecutor::new();
    if ex.supersede(9, "x") == Err(APP_ROW_RANGE) {
        caught += 1;
    }
    // 注 3：更实行再改
    let mut ex = standard_executor();
    if ex.supersede(2, "again") == Err(APP_SUP_REWRITE) {
        caught += 1;
    }
    // 注 4：链篡改捕获（手工破链后 chain_ok 必假）
    let mut ex = AppendExecutor::new();
    let _ = ex.append("a");
    let _ = ex.append("b");
    // 模拟篡改：直接改第 1 行内容（绕过接口的内存级攻击面）
    ex.rows[0].content = "tampered".to_string();
    if !ex.chain_ok() {
        caught += 1;
    }
    // 注 5：更实行指向核（手工破坏指向后 supersede_chain_ok 必假）
    let mut ex = standard_executor();
    ex.rows[1].superseded_by = Some(99);
    if !ex.supersede_chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 只追加语义；J2 追加序账；J3 更实链。
pub fn run_unx56142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56142");
    // J1-1 四行落账（3 原始 + 1 更实）
    let ex = standard_executor();
    let ok = ex.len() == 4;
    s.add("56142-J1-四行落账", ok, if ok { "" } else { "行数漂移" });
    // J1-2 链全绿
    let ok = ex.chain_ok() && ex.supersede_chain_ok();
    s.add("56142-J1-链全绿", ok, if ok { "" } else { "指纹链断裂" });
    // J1-3 渲染导出齐
    let text = standard_executor().render_ledger();
    let ok = text.contains("SUPERSEDES:r2|") && text.contains("LOCKED") && text.contains("LIVE");
    s.add("56142-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 行号连续
    let ex = standard_executor();
    let ok = ex.row(1).map(|r| r.row_no == 1).unwrap_or(false)
        && ex.row(4).map(|r| r.row_no == 4).unwrap_or(false);
    s.add("56142-J2-行号连续", ok, if ok { "" } else { "行号漂移" });
    // J2-2 指纹链逐行锁定：同内容同链位同指纹
    let mut a = AppendExecutor::new();
    let _ = a.append("seed|row");
    let mut b = AppendExecutor::new();
    let _ = b.append("seed|row");
    let ok = a.row(1).map(|r| r.fp).unwrap_or(0) == b.row(1).map(|r| r.fp).unwrap_or(1);
    s.add("56142-J2-同位同签", ok, if ok { "" } else { "链签名漂移" });
    // J2-3 链位敏感：同内容异链位异指纹
    let mut c = AppendExecutor::new();
    let _ = c.append("other");
    let _ = c.append("seed|row");
    let ok = a.row(1).map(|r| r.fp).unwrap_or(0) != c.row(2).map(|r| r.fp).unwrap_or(0);
    s.add("56142-J2-链位敏感", ok, if ok { "" } else { "链退化" });
    // J2-4 空行拒
    let mut d = AppendExecutor::new();
    let ok = d.append("") == Err(APP_EMPTY_ROW) && d.len() == 0;
    s.add("56142-J2-空行拒", ok, if ok { "" } else { "空行穿透" });
    // J3-1 更实行锁定旧行
    let ex = standard_executor();
    let ok = ex.row(2).map(|r| r.superseded_by == Some(4)).unwrap_or(false);
    s.add("56142-J3-旧行锁定", ok, if ok { "" } else { "锁定面漂移" });
    // J3-2 更实行前缀语义
    let ok = ex.row_content(4).map(|c| c.starts_with("SUPERSEDES:r2|")).unwrap_or(false);
    s.add("56142-J3-更实前缀", ok, if ok { "" } else { "前缀语义漂移" });
    // J3-3 更实行也可再被更（未锁定行可继续更实）
    let mut ex2 = standard_executor();
    let ok = ex2.supersede(4, "mid_load|120|117|9750").is_ok();
    s.add("56142-J3-更实行可续更", ok, if ok { "" } else { "续更面漂移" });
    // J3-4 旧行内容保留可追溯
    let ok = ex.row_content(2) == Some("mid_load|120|114|9500");
    s.add("56142-J3-旧行可溯", ok, if ok { "" } else { "追溯面漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56142-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56142_all_checks_green() {
        let set = run_unx56142_checks();
        assert!(set.all_passed(), "UNX-F56142 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56142_chain_stable() {
        assert!(standard_executor().chain_ok());
    }
}
