//! UNX-F10232 归因总表（五列齐备与逐条账对平，缺列检出）（0x11020 段）
//!
//! 锚点：UNX-F10232-J1 总表达成（五列齐备与逐条账对平
//! 10/10 次），缺列 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：归因总表（用例/症状/归类/工单/处置五列）+ 逐条账 + 列齐备
//! 核 + 对平核（行数=账数且逐行字段非空）+ 缺列检出 + 折叠指纹 +
//! 注入核（缺列/重行/空字段/封表改写/行账失衡）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x11020;
/// 诊断码：缺列
pub const ERR_COLUMN_MISSING: u32 = SEG_BASE + 0x01;
/// 诊断码：重行
pub const ERR_DUP_ROW: u32 = SEG_BASE + 0x02;
/// 诊断码：空字段
pub const ERR_EMPTY_FIELD: u32 = SEG_BASE + 0x03;
/// 诊断码：封表改写
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：行账失衡
pub const ERR_ROW_IMBALANCE: u32 = SEG_BASE + 0x05;

/// 五列总数
pub const COLUMN_TOTAL: usize = 5;

/// 归因行（五列）
#[derive(Clone, PartialEq, Eq)]
pub struct AttribRow {
    /// 列一：用例
    pub case: String,
    /// 列二：症状
    pub symptom: String,
    /// 列三：归类
    pub class: String,
    /// 列四：工单号
    pub ticket: String,
    /// 列五：处置
    pub action: String,
}

impl AttribRow {
    /// 五列齐备（全部非空）
    pub fn columns_complete(&self) -> bool {
        !self.case.is_empty() && !self.symptom.is_empty() && !self.class.is_empty()
            && !self.ticket.is_empty() && !self.action.is_empty()
    }
    /// 缺列计数
    pub fn missing_columns(&self) -> usize {
        COLUMN_TOTAL - [
            !self.case.is_empty(),
            !self.symptom.is_empty(),
            !self.class.is_empty(),
            !self.ticket.is_empty(),
            !self.action.is_empty(),
        ].iter().filter(|&&x| x).count()
    }
}

/// 归因总表
pub struct AttribMaster {
    /// 逐条行
    pub rows: Vec<AttribRow>,
    /// 行账计数（登记应行数）
    pub expected: usize,
    /// 封表
    pub frozen: bool,
}

impl AttribMaster {
    /// 空表
    pub fn new() -> Self {
        AttribMaster { rows: Vec::new(), expected: 0, frozen: false }
    }

    /// 登记应行数（执行账移交）
    pub fn set_expected(&mut self, n: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        self.expected = n;
        Ok(())
    }

    /// 添行
    pub fn add_row(&mut self, case: &str, symptom: &str, class: &str, ticket: &str, action: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let row = AttribRow {
            case: case.to_string(),
            symptom: symptom.to_string(),
            class: class.to_string(),
            ticket: ticket.to_string(),
            action: action.to_string(),
        };
        if row.missing_columns() > 0 {
            return Err(ERR_COLUMN_MISSING);
        }
        if self.rows.iter().any(|r| r.case == case) {
            return Err(ERR_DUP_ROW);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 对平核：行数=应行数且全行五列齐备
    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.len() != self.expected {
            return Err(ERR_ROW_IMBALANCE);
        }
        if self.rows.iter().any(|r| r.missing_columns() > 0) {
            return Err(ERR_EMPTY_FIELD);
        }
        Ok(())
    }

    /// 缺列行清单
    pub fn incomplete_rows(&self) -> Vec<String> {
        self.rows.iter().filter(|r| r.missing_columns() > 0).map(|r| r.case.clone()).collect()
    }

    /// 按用例回查
    pub fn row_of(&self, case: &str) -> Option<&AttribRow> {
        self.rows.iter().find(|r| r.case == case)
    }

    /// 按归类计数
    pub fn class_count(&self, class: &str) -> usize {
        self.rows.iter().filter(|r| r.class == class).count()
    }

    /// 处置清单
    pub fn action_list(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.action.clone()).collect()
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, r) in self.rows.iter().enumerate() {
            acc = acc.wrapping_mul(6911).wrapping_add(((r.case.len() + r.action.len()) as u32) + i as u32);
            for b in r.class.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        acc = acc.wrapping_add(self.expected as u32);
        acc
    }

    /// 诊断面：行数
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 诊断面：应行数回读
    pub fn expected_count(&self) -> usize {
        self.expected
    }

    /// 诊断面：工单列清单
    pub fn ticket_list(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.ticket.clone()).collect()
    }

    /// 诊断面：症状列回读
    pub fn symptom_of(&self, case: &str) -> Option<&String> {
        self.rows.iter().find(|r| r.case == case).map(|r| &r.symptom)
    }

    /// 诊断面：表摘要
    pub fn table_summary(&self) -> String {
        format!(
            "rows={} expected={} frozen={}",
            self.rows.len(),
            self.expected,
            self.frozen
        )
    }

    /// 诊断面：归类面谱（三分类计数）
    pub fn class_spectrum(&self) -> (usize, usize, usize) {
        (
            self.class_count("KERNEL_BUG"),
            self.class_count("ENV_LIMITED"),
            self.class_count("EXPECT_GAP"),
        )
    }

    /// 诊断面：全行齐备复核
    pub fn all_rows_complete(&self) -> bool {
        self.rows.iter().all(|r| r.columns_complete())
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 缺列拒绝
    let mut m1 = AttribMaster::new();
    out[0] = m1.add_row("c", "", "KERNEL_BUG", "T1", "fix") == Err(ERR_COLUMN_MISSING);
    // 2: 重行拒绝
    let mut m2 = AttribMaster::new();
    let _ = m2.add_row("c", "s", "KERNEL_BUG", "T1", "fix");
    out[1] = m2.add_row("c", "s2", "ENV_LIMITED", "T2", "tag") == Err(ERR_DUP_ROW);
    // 3: 行账失衡检出
    let mut m3 = AttribMaster::new();
    let _ = m3.set_expected(3);
    let _ = m3.add_row("c", "s", "KERNEL_BUG", "T1", "fix");
    out[2] = m3.audit() == Err(ERR_ROW_IMBALANCE);
    // 4: 封表改写拒绝
    let mut m4 = AttribMaster::new();
    let _ = m4.add_row("c", "s", "KERNEL_BUG", "T1", "fix");
    let _ = m4.seal();
    out[3] = m4.add_row("d", "s", "KERNEL_BUG", "T2", "fix") == Err(ERR_FROZEN)
        && m4.set_expected(9) == Err(ERR_FROZEN);
    // 5: 空字段检出（伪造行）
    let mut m5 = AttribMaster::new();
    let _ = m5.set_expected(1);
    let _ = m5.add_row("c", "s", "KERNEL_BUG", "T1", "fix");
    m5.rows[0].ticket = String::new();
    out[4] = m5.audit() == Err(ERR_EMPTY_FIELD) && m5.incomplete_rows() == vec!["c".to_string()];
    out
}

/// 判据函数
pub fn run_unx10232_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10232");
    let mut m = AttribMaster::new();
    let _ = m.set_expected(3);
    let a1 = m.add_row("ut-net-flaky", "三跑同纹 0x5150", "KERNEL_BUG", "BUG-UNX-401", "开单修复");
    let a2 = m.add_row("ut-pty-miss", "实机从口缺失", "ENV_LIMITED", "ENV-202", "挂补测标记");
    let a3 = m.add_row("ut-wide-gap", "缓冲深度与规格差 32", "EXPECT_GAP", "ARB-9", "仲裁后改期望");
    s.add("三行齐列入账", a1.is_ok() && a2.is_ok() && a3.is_ok(), "三条五列齐备行入账");
    s.add("行账对平", m.audit().is_ok(), "行数=应行数且全行齐列");
    s.add("缺列清单空", m.incomplete_rows().is_empty(), "无缺列行");
    s.add("回查命中", m.row_of("ut-pty-miss").map(|r| r.ticket.as_str()) == Some("ENV-202"), "按用例回查工单列");
    s.add("归类计数", m.class_count("KERNEL_BUG") == 1 && m.class_count("ENV_LIMITED") == 1 && m.class_count("EXPECT_GAP") == 1, "三类各一行");
    s.add("处置清单", m.action_list().len() == 3, "三条处置动作");
    s.add("缺列拒绝", m.add_row("x", "", "", "", "") == Err(ERR_COLUMN_MISSING), "四缺列行被拒");
    s.add("重行拒绝", m.add_row("ut-net-flaky", "s", "KERNEL_BUG", "T", "a") == Err(ERR_DUP_ROW), "同用例重行被拒");
    s.add("行数回读", m.row_count() == 3 && m.expected_count() == 3, "行数与应行数一致");
    s.add("工单列清单", m.ticket_list() == vec!["BUG-UNX-401".to_string(), "ENV-202".to_string(), "ARB-9".to_string()], "工单列按行序");
    s.add("症状列回读", m.symptom_of("ut-wide-gap").map(|s| s.contains("32")) == Some(true), "症状列可回读");
    s.add("表摘要", m.table_summary().contains("rows=3") && m.table_summary().contains("frozen=false"), "摘要含行数与封表态");
    s.add("归类面谱", m.class_spectrum() == (1, 1, 1), "三分类各一行");
    s.add("全行齐备", m.all_rows_complete(), "全行五列复核");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = m.seal();
    s.add("封表拒绝", m.add_row("late", "s", "KERNEL_BUG", "T", "a") == Err(ERR_FROZEN), "封表后添行被拒");
    s.add("封表指纹非零", fp != 0, "封表折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_column_row_audit() {
        let mut m = AttribMaster::new();
        let _ = m.set_expected(1);
        let _ = m.add_row("a", "sym", "cls", "tk", "act");
        assert!(m.audit().is_ok());
        assert_eq!(m.row_of("a").unwrap().missing_columns(), 0);
        assert!(m.incomplete_rows().is_empty());
    }

    #[test]
    fn forged_empty_field_detected() {
        let mut m = AttribMaster::new();
        let _ = m.set_expected(1);
        let _ = m.add_row("a", "sym", "cls", "tk", "act");
        m.rows[0].action = String::new();
        assert_eq!(m.audit(), Err(ERR_EMPTY_FIELD));
        assert_eq!(m.incomplete_rows(), vec!["a".to_string()]);
    }

    #[test]
    fn class_counting_and_actions() {
        let mut m = AttribMaster::new();
        let _ = m.set_expected(2);
        let _ = m.add_row("c1", "s1", "KERNEL_BUG", "T1", "fix");
        let _ = m.add_row("c2", "s2", "KERNEL_BUG", "T2", "retest");
        assert_eq!(m.class_count("KERNEL_BUG"), 2);
        assert_eq!(m.class_count("ENV_LIMITED"), 0);
        assert_eq!(m.action_list(), vec!["fix".to_string(), "retest".to_string()]);
        assert!(m.audit().is_ok());
    }

    #[test]
    fn expected_resets_blocked_when_frozen() {
        let mut m = AttribMaster::new();
        let _ = m.set_expected(1);
        let _ = m.add_row("a", "s", "cls", "t", "act");
        let _ = m.seal();
        assert_eq!(m.set_expected(5), Err(ERR_FROZEN));
        assert!(m.row_of("a").is_some());
        assert!(m.row_of("b").is_none());
    }

    #[test]
    fn spectrum_after_multi_class() {
        let mut m = AttribMaster::new();
        let _ = m.set_expected(3);
        let _ = m.add_row("a", "s", "KERNEL_BUG", "t1", "act");
        let _ = m.add_row("b", "s", "KERNEL_BUG", "t2", "act");
        let _ = m.add_row("c", "s", "EXPECT_GAP", "t3", "act");
        assert_eq!(m.class_spectrum(), (2, 0, 1));
        assert_eq!(m.class_count("ENV_LIMITED"), 0);
        assert!(m.all_rows_complete());
        assert_eq!(m.ticket_list().len(), 3);
    }

    #[test]
    fn row_of_missing_is_none() {
        let m = AttribMaster::new();
        assert!(m.row_of("anything").is_none());
        assert_eq!(m.row_count(), 0);
        assert_eq!(m.expected_count(), 0);
        assert!(m.table_summary().contains("rows=0"));
        assert!(m.table_summary().contains("frozen=false"));
    }
}
