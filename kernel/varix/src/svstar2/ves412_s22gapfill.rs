//! ves412_s22gapfill — VE-F4172 · 缺失与薄弱项补齐
//! 自查缺口与薄弱项补齐：六维资产汇总交叉扫描缺口清单；补齐分级（P0 阻断收口必修/P1 终审前必修/
//! P2 登记缓办）；补齐回执制；P0 未消→阻断收口（硬门复述）；补齐失败→复议或降级声明；回执缺→催办；
//! 缺口清单含多语言无障碍专项列；语料补齐走脱敏通道。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0E80;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfCode {
    Ok = 0,
    P0Open = SEG_BASE + 0x01,     // 0x0E81
    ReceiptMissing = SEG_BASE + 0x02, // 0x0E82
    Refused = SEG_BASE + 0x03,    // 0x0E83
}

/// 补齐分级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority { P2Defer = 0, P1BeforeFinal = 1, P0Blocker = 2 }

impl Priority {
    pub fn name(self) -> &'static str {
        match self { Priority::P2Defer => "P2-defer", Priority::P1BeforeFinal => "P1-before-final",
            Priority::P0Blocker => "P0-blocker" }
    }
}

/// 缺口条目：六维来源 + 无障碍专项列标记 + 脱敏通道 + 分级 + 补齐态 + 回执。
#[derive(Clone, Copy)]
pub struct GapItem {
    pub dim: &'static str,       // 来源六维（api/test/perf/security/a11y/doc）
    pub desc: &'static str,
    pub a11y_col: bool,          // 多语言无障碍专项列（域本色）
    pub sanitized_channel: bool, // 语料补齐走脱敏通道
    pub prio: Priority,
    pub fixed: bool,
    pub receipt: bool,
}

impl GapItem {
    pub fn open_p0(&self) -> bool { self.prio == Priority::P0Blocker && !self.fixed }
    pub fn needs_receipt(&self) -> bool { self.fixed && !self.receipt }
}

/// 交叉扫描缺口清单。
pub struct GapList {
    pub items: Vec<GapItem>,
    pub urges: Vec<usize>,
    pub appeals: Vec<&'static str>, // 复议/降级声明留痕
}

impl GapList {
    pub fn new() -> Self { GapList { items: Vec::new(), urges: Vec::new(), appeals: Vec::new() } }

    pub fn add(&mut self, g: GapItem) {
        match self.items.iter().position(|x| x.desc == g.desc) {
            Some(i) => self.items[i] = g,
            None => self.items.push(g),
        }
    }

    /// P0 硬门：任一 P0 未消 → 阻断收口。
    pub fn p0_open(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, x)| x.open_p0()).map(|(i, _)| i).collect()
    }
    pub fn close_allowed(&self) -> bool { self.p0_open().is_empty() }

    /// 补齐登记。
    pub fn mark_fixed(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.fixed = true; true }
            None => false,
        }
    }

    /// 回执缺催办（幂等）。
    pub fn urge_receipts(&mut self) -> Vec<usize> {
        for (i, x) in self.items.iter().enumerate() {
            if x.needs_receipt() && !self.urges.contains(&i) { self.urges.push(i); }
        }
        self.urges.clone()
    }

    pub fn give_receipt(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.receipt = true; true }
            None => false,
        }
    }

    /// 补齐失败 → 复议或降级声明（P0 降级须留痕）。
    pub fn demote(&mut self, idx: usize, to: Priority, reason: &'static str) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => {
                x.prio = to;
                self.appeals.push(reason);
                true
            }
            None => false,
        }
    }

    /// 分级排序核验：P0 > P1 > P2。
    pub fn order_ok(&self) -> bool { Priority::P2Defer < Priority::P1BeforeFinal && Priority::P1BeforeFinal < Priority::P0Blocker }

    /// 无障碍专项列在清单（域本色）。
    pub fn a11y_col_present(&self) -> bool { self.items.iter().any(|x| x.a11y_col) }

    /// 脱敏通道核验：语料类缺口必须走脱敏通道。
    pub fn corpus_sanitized(&self) -> bool {
        self.items.iter().filter(|x| x.dim == "test").all(|x| x.sanitized_channel)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves412_checks() -> CheckSet {
    let mut set = CheckSet::new("ves412");
    let mut gl = GapList::new();

    // C412-01 三级缺口建单。
    gl.add(GapItem { dim: "api", desc: "a11y 文本接口缺参考", a11y_col: true, sanitized_channel: false, prio: Priority::P0Blocker, fixed: false, receipt: false });
    gl.add(GapItem { dim: "perf", desc: "切换基准缺 RTL 档", a11y_col: false, sanitized_channel: false, prio: Priority::P1BeforeFinal, fixed: false, receipt: false });
    gl.add(GapItem { dim: "doc", desc: "格式速查表缺附录", a11y_col: false, sanitized_channel: false, prio: Priority::P2Defer, fixed: false, receipt: false });
    chk(&mut set, "C412-01", gl.items.len() == 3 && gl.order_ok(), "三级三单");

    // C412-02 P0 硬门阻断。
    chk(&mut set, "C412-02", gl.p0_open() == vec![0] && !gl.close_allowed(), "P0 阻断收口");

    // C412-03 补齐后硬门放行。
    gl.mark_fixed(0);
    chk(&mut set, "C412-03", gl.p0_open().is_empty() && gl.close_allowed(), "P0 消解放行");

    // C412-04 回执缺催办。
    let u1 = gl.urge_receipts();
    chk(&mut set, "C412-04", u1 == vec![0], "催办一条");

    // C412-05 回执登记后催办清。
    gl.give_receipt(0);
    chk(&mut set, "C412-05", gl.urge_receipts().is_empty(), "回执清催办");

    // C412-06 补齐失败降级声明（P0 → P1 留痕）。
    let mut gl2 = GapList::new();
    gl2.add(GapItem { dim: "test", desc: "语料受限级未加密", a11y_col: false, sanitized_channel: true, prio: Priority::P0Blocker, fixed: false, receipt: false });
    chk(&mut set, "C412-06", gl2.demote(0, Priority::P1BeforeFinal, "补齐失败降级声明")
        && gl2.appeals.len() == 1 && gl2.close_allowed(), "降级留痕放行");

    // C412-07 无障碍专项列在清单（域本色）。
    chk(&mut set, "C412-07", gl.a11y_col_present(), "专项列在册");

    // C412-08 语料脱敏通道核验。
    chk(&mut set, "C412-08", gl2.corpus_sanitized(), "语料走脱敏通道");

    // C412-09 未脱敏语料缺口检出。
    let mut gl3 = GapList::new();
    gl3.add(GapItem { dim: "test", desc: "语料补样", a11y_col: false, sanitized_channel: false, prio: Priority::P1BeforeFinal, fixed: false, receipt: false });
    chk(&mut set, "C412-09", !gl3.corpus_sanitized(), "未脱敏检出");

    // C412-10 分级名单单源。
    chk(&mut set, "C412-10", Priority::P0Blocker.name() == "P0-blocker"
        && Priority::P2Defer.name() == "P2-defer", "分级单源");

    // C412-11 催办幂等。
    gl2.mark_fixed(0);
    let a = gl2.urge_receipts();
    let b = gl2.urge_receipts();
    chk(&mut set, "C412-11", a == b && a == vec![0], "催办幂等");

    // C412-12 空清单放行（空集全称）。
    let empty = GapList::new();
    chk(&mut set, "C412-12", empty.close_allowed() && !empty.a11y_col_present(), "空清单放行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves412_smoke() {
        let set = run_ves412_checks();
        assert!(set.failed_count() == 0, "ves412 red: {}", set.red_items_text());
    }
}
