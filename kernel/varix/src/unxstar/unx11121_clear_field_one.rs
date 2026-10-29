//! UNX-F11121 清账场搭建（全域清账账与 B01–B20 执行序）（0x1f5a1 段）
//!
//! 清账段一共 20 批，B01 至 B20 按序执行。清账场先立全域清账账，逐批
//! 记录族名与执行序号，全部批次武装后冻结执行序，冻结后不再接受改动。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11121 独占 0x1f5a1。
pub const SEG_BASE: u32 = 0x1f5a1;

/// 批号越出 B01–B20。
pub const CF_RANGE: u32 = SEG_BASE + 0x01;
/// 执行序错乱。
pub const CF_ORDER: u32 = SEG_BASE + 0x02;
/// 批号重复立账。
pub const CF_DUP: u32 = SEG_BASE + 0x03;
/// 族名为空。
pub const CF_EMPTY: u32 = SEG_BASE + 0x04;
/// 尚有批次未武装。
pub const CF_UNARMED: u32 = SEG_BASE + 0x05;
/// 执行序账已冻结。
pub const CF_FROZEN: u32 = SEG_BASE + 0x06;
/// 批次不足。
pub const CF_THIN: u32 = SEG_BASE + 0x07;
/// 容量溢出。
pub const CF_OVER: u32 = SEG_BASE + 0x08;

/// 清账段一批数。
pub const BATCHES: usize = 20;
/// 容量。
pub const CAP: usize = 24;
/// 折叠种子。
const CF_TAG: u64 = 0x0000_1f5_a1a1;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 执行序条目。
#[derive(Clone, Debug)]
pub struct ExecStep {
    /// 批号（1..=20）。
    pub batch: usize,
    /// 族名。
    pub family: String,
    /// 执行序号（等于批号）。
    pub seq: usize,
    /// 是否武装。
    pub armed: bool,
}

impl ExecStep {
    /// 条目渲染，形如 B01|PIPE|1|A。
    pub fn render(&self) -> String {
        let flag = if self.armed { "A" } else { "N" };
        format!("B{:02}|{}|{}|{}", self.batch, self.family, self.seq, flag)
    }

    /// 序号是否与批号一致。
    pub fn ordered(&self) -> bool {
        self.seq == self.batch
    }
}

/// 清账场。
#[derive(Clone, Debug)]
pub struct ClearField {
    steps: Vec<ExecStep>,
    frozen: bool,
    root: u64,
}

impl ClearField {
    /// 建空场。
    pub fn new() -> Self {
        Self { steps: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位批下标。
    fn index_of(&self, batch: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].batch == batch {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 立一批：域、空名、重复、顺序、容量、冻结六道闸。
    pub fn plan(&mut self, batch: usize, family: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(CF_FROZEN);
        }
        if batch == 0 || batch > BATCHES {
            return Err(CF_RANGE);
        }
        if family.is_empty() {
            return Err(CF_EMPTY);
        }
        if self.steps.len() >= CAP {
            return Err(CF_OVER);
        }
        if self.index_of(batch) != usize::MAX {
            return Err(CF_DUP);
        }
        if self.steps.len().wrapping_add(1) != batch {
            return Err(CF_ORDER);
        }
        let label = String::from(family);
        self.steps.push(ExecStep { batch, family: label, seq: batch, armed: false });
        Ok(())
    }

    /// 武装一批。
    pub fn arm(&mut self, batch: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(CF_FROZEN);
        }
        let idx = self.index_of(batch);
        if idx == usize::MAX {
            return Err(CF_RANGE);
        }
        if let Some(st) = self.steps.get_mut(idx) {
            st.armed = true;
            return Ok(());
        }
        Err(CF_RANGE)
    }

    /// 已立账批数。
    pub fn planned(&self) -> usize {
        self.steps.len()
    }

    /// 已武装批数。
    pub fn armed(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].armed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 执行序是否逐条有序。
    pub fn ordered(&self) -> bool {
        if self.steps.len() != BATCHES {
            return false;
        }
        let mut k = 0usize;
        while k < self.steps.len() {
            if !self.steps[k].ordered() || self.steps[k].batch != k.wrapping_add(1) {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 清账场是否可执行。
    pub fn field_ready(&self) -> bool {
        self.planned() == BATCHES && self.armed() == BATCHES && self.ordered()
    }

    /// 冻结执行序：返回折叠根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.root = self.fold();
        self.root
    }

    /// 已冻结否。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = CF_TAG;
        let mut k = 0usize;
        while k < self.steps.len() {
            let st = &self.steps[k];
            h = (h ^ st.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ st.seq as u64).wrapping_mul(FP_STEP);
            let b = if st.armed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 二十批族名，按 B01 至 B20 排列。
pub const FAMILIES: [&str; 20] = [
    "PIPE", "PIPE2", "SIG", "SIG2", "TERM", "TERM2", "LINE", "JOB", "USOCK", "USOCK2",
    "SCM", "MUX", "SYSV", "SYSV2", "POSIX", "DEEP", "DEEP2", "OPT", "EPOLL", "MTYPE",
];

/// 标准清账场：二十批按序立账并全部武装。
pub fn build_field() -> ClearField {
    let mut field = ClearField::new();
    let mut batch = 1usize;
    while batch <= BATCHES {
        let idx = batch.saturating_sub(1);
        let fam = if idx < FAMILIES.len() { FAMILIES[idx] } else { "UNK" };
        let _ = field.plan(batch, fam);
        let _ = field.arm(batch);
        batch = batch.wrapping_add(1);
    }
    field
}

/// 四路对抗：批号越界、跳批立账、族名为空、重复立账。
pub fn inject_four() -> [bool; 4] {
    let mut a_field = ClearField::new();
    let a = a_field.plan(0, "PIPE") == Err(CF_RANGE)
        && a_field.plan(21, "PIPE") == Err(CF_RANGE)
        && a_field.planned() == 0;
    let mut b_field = ClearField::new();
    let b = b_field.plan(3, "SIG") == Err(CF_ORDER) && b_field.planned() == 0;
    let mut c_field = ClearField::new();
    let c = c_field.plan(1, "") == Err(CF_EMPTY) && c_field.planned() == 0;
    let mut d_field = ClearField::new();
    let _ = d_field.plan(1, "PIPE");
    let _ = d_field.plan(2, "PIPE2");
    let d = d_field.plan(2, "PIPE2") == Err(CF_DUP) && d_field.planned() == 2;
    [a, b, c, d]
}

/// F11121 全部检查：清账场全链搭建与执行序冻结。
pub fn run_unx11121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11121 清账场搭建");
    let field = build_field();
    let four = inject_four();

    // J1 二十批立账。
    s.add(
        "J1-全域立账",
        field.planned() == BATCHES && BATCHES == 20,
        "B01 至 B20 共 20 批全部立账",
    );
    // J2 执行序有序。
    s.add(
        "J2-执行序",
        field.ordered(),
        "执行序号与批号逐条一致，二十批严格按序",
    );
    // J3 全批武装。
    s.add(
        "J3-全批武装",
        field.armed() == BATCHES && field.field_ready(),
        "二十批全部武装，清账场可执行",
    );
    // J4 冻结执行序。
    let mut sealed = build_field();
    let root = sealed.freeze();
    s.add(
        "J4-执行序冻结",
        sealed.is_frozen() && sealed.fold() == root && root != CF_TAG,
        "执行序冻结后折叠根稳定且不等于种子",
    );
    // J5 批号闸。
    let mut range_field = ClearField::new();
    s.add(
        "J5-批号闸",
        range_field.plan(0, "PIPE") == Err(CF_RANGE)
            && range_field.plan(21, "PIPE") == Err(CF_RANGE),
        "批号 0 与 21 越出 B01–B20，立账报 CF_RANGE",
    );
    // J6 跳批闸。
    let mut order_field = ClearField::new();
    let _ = order_field.plan(1, "PIPE");
    s.add(
        "J6-跳批闸",
        order_field.plan(3, "SIG") == Err(CF_ORDER) && order_field.planned() == 1,
        "B01 后直接立 B03 报 CF_ORDER",
    );
    // J7 空名闸。
    let mut empty_field = ClearField::new();
    s.add(
        "J7-空名闸",
        empty_field.plan(1, "") == Err(CF_EMPTY) && empty_field.planned() == 0,
        "族名为空时报 CF_EMPTY，不入账",
    );
    // J8 重复立账闸。
    let mut dup_field = ClearField::new();
    let _ = dup_field.plan(1, "PIPE");
    s.add(
        "J8-重账闸",
        dup_field.plan(1, "PIPE") == Err(CF_DUP) && dup_field.planned() == 1,
        "同一批号二次立账报 CF_DUP",
    );
    // J9 冻结拒写。
    let mut frozen_field = build_field();
    let _ = frozen_field.freeze();
    s.add(
        "J9-冻结拒写",
        frozen_field.plan(1, "PIPE") == Err(CF_FROZEN)
            && frozen_field.arm(1) == Err(CF_FROZEN),
        "冻结后立账与武装均报 CF_FROZEN",
    );
    // J10 未武装不可执行。
    let mut half = ClearField::new();
    let mut batch = 1usize;
    while batch <= BATCHES {
        let idx = batch.saturating_sub(1);
        let fam = if idx < FAMILIES.len() { FAMILIES[idx] } else { "UNK" };
        let _ = half.plan(batch, fam);
        if batch < BATCHES {
            let _ = half.arm(batch);
        }
        batch = batch.wrapping_add(1);
    }
    s.add(
        "J10-武装闸",
        half.armed() == BATCHES - 1 && !half.field_ready(),
        "B20 未武装时清账场不可执行",
    );
    // J11 四路对抗。
    s.add(
        "J11-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批号越界、跳批、空名、重账四路全部被拒",
    );
    // J12 族名入账。
    let mut fam_field = ClearField::new();
    let _ = fam_field.plan(1, "PIPE");
    s.add(
        "J12-族名入账",
        fam_field.plan(2, "PIPE2").is_ok() && fam_field.planned() == 2,
        "族名随批入账，PIPE 与 PIPE2 分列两批",
    );
    // J13 条目渲染。
    let step = ExecStep {
        batch: 1,
        family: String::from("PIPE"),
        seq: 1,
        armed: true,
    };
    s.add(
        "J13-条目渲染",
        step.render() == "B01|PIPE|1|A" && step.ordered(),
        "首条执行序渲染为 B01|PIPE|1|A",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账场明细",
        field.field_ready(),
        &format!(
            "立账={} 武装={} 有序={} 冻结={}",
            field.planned(),
            field.armed(),
            field.ordered(),
            field.is_frozen()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11121_field_flow() {
        let mut field = build_field();
        assert_eq!(field.planned(), BATCHES);
        assert_eq!(field.armed(), BATCHES);
        assert!(field.ordered());
        assert!(field.field_ready());
        let root = field.freeze();
        assert_eq!(field.fold(), root);
        assert_eq!(field.plan(1, "PIPE"), Err(CF_FROZEN));
    }

    #[test]
    fn unx11121_checks_green() {
        let s = run_unx11121_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
