//! UNX-F11238 glibc 内核依赖行为微差账（0x1f838 段）
//!
//! 锚点：UNX-F11238-J1 微差条目（现象/根因/危害级/处置）四列账落账，
//! 微差零静默。
//! 域：CoRun·UNX-C5-B02——微差不是失败，也不能被静默：每条必须四列齐全，
//! 危害级落 1..=3，处置列写明「接受/修正/上报」之一，缺一即静默。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f838;
pub const MD_EMPTY: u32 = SEG_BASE + 0x01; // 某列为空
pub const MD_SEV: u32 = SEG_BASE + 0x02; // 危害级越界
pub const MD_ACTION: u32 = SEG_BASE + 0x03; // 处置不在白名单
pub const MD_SILENT: u32 = SEG_BASE + 0x04; // 静默条目
pub const MD_DUP: u32 = SEG_BASE + 0x05; // 条目号重复
pub const MD_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const MD_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 危害级上限与处置白名单。
pub const SEV_MAX: u8 = 3;
pub const ACTIONS: [&str; 3] = ["接受", "修正", "上报"];
pub const WANT: usize = 6;
pub const CAP: usize = 12;
pub const MD_TAG: u64 = 0x1f838d9;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一条微差。
#[derive(Clone, Debug)]
pub struct Diff {
    pub id: u32,
    pub phenomenon: &'static str,
    pub root: &'static str,
    pub severity: u8,
    pub action: &'static str,
}

impl Diff {
    pub fn four_columns(&self) -> bool {
        !self.phenomenon.is_empty() && !self.root.is_empty() && self.severity >= 1 && self.severity <= SEV_MAX
    }

    pub fn action_ok(&self) -> bool {
        ACTIONS.iter().any(|a| *a == self.action)
    }

    pub fn silent(&self) -> bool {
        !self.four_columns() || !self.action_ok()
    }
}

/// 微差账。
#[derive(Clone, Debug)]
pub struct MicroLedger {
    pub diffs: Vec<Diff>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MicroLedger {
    pub fn new() -> MicroLedger {
        MicroLedger { diffs: Vec::new(), frozen: false, root: None }
    }

    /// 登账：空列/越级/坏处置/重号四路拒。
    pub fn add(&mut self, d: Diff) -> Result<(), u32> {
        if self.frozen {
            return Err(MD_FROZEN);
        }
        if d.phenomenon.is_empty() || d.root.is_empty() || d.action.is_empty() {
            return Err(MD_EMPTY);
        }
        if d.severity < 1 || d.severity > SEV_MAX {
            return Err(MD_SEV);
        }
        if !d.action_ok() {
            return Err(MD_ACTION);
        }
        if self.diffs.iter().any(|x| x.id == d.id) {
            return Err(MD_DUP);
        }
        if self.diffs.len() >= CAP {
            return Err(MD_OVER);
        }
        self.diffs.push(d);
        Ok(())
    }

    pub fn silent_items(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.diffs.len() {
            if self.diffs[k].silent() {
                out.push(self.diffs[k].id);
            }
            k += 1;
        }
        out
    }

    pub fn accept(&self) -> usize {
        self.diffs.iter().filter(|d| d.action == "接受").count()
    }

    pub fn fix(&self) -> usize {
        self.diffs.iter().filter(|d| d.action == "修正").count()
    }

    pub fn escalate(&self) -> usize {
        self.diffs.iter().filter(|d| d.action == "上报").count()
    }

    pub fn high(&self) -> usize {
        self.diffs.iter().filter(|d| d.severity == 3).count()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(MD_FROZEN);
        }
        if self.diffs.len() != WANT {
            return Err(MD_EMPTY);
        }
        if !self.silent_items().is_empty() {
            return Err(MD_SILENT);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("micro {} accept {} fix {} esc {}", self.diffs.len(), self.accept(), self.fix(), self.escalate())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = MD_TAG;
        let mut k = 0usize;
        while k < self.diffs.len() {
            h = (h ^ self.diffs[k].id as u64).wrapping_mul(FP_STEP);
            let p = self.diffs[k].phenomenon.as_bytes();
            let mut i = 0usize;
            while i < p.len() {
                h = (h ^ p[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.diffs[k].severity as u64).wrapping_mul(FP_STEP);
            let a = self.diffs[k].action.as_bytes();
            i = 0;
            while i < a.len() {
                h = (h ^ a[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 六条微差：现象、根因、级、处置。
pub const ITEMS: [(u32, &str, &str, u8, &str); 6] = [
    (1, "brk 返回值高位置零", "内核地址空间上限不同", 1, "接受"),
    (2, "mmap 提示地址被顺延", "ASLR 档位更保守", 1, "接受"),
    (3, "futex 唤醒晚一拍", "调度器批处理", 2, "修正"),
    (4, "rseq 注册被拒", "未实现该 vDSO 槽", 2, "修正"),
    (5, "clock 抖动偏大", "定时器分辨率粗", 2, "修正"),
    (6, "SIGSEGV 附 core 位", "core dump 默认开启", 3, "上报"),
];

/// 组一张六条四列齐全的微差账。
pub fn build() -> MicroLedger {
    let mut l = MicroLedger::new();
    let mut k = 0usize;
    while k < ITEMS.len() {
        let (id, p, r, s, a) = ITEMS[k];
        let _ = l.add(Diff { id, phenomenon: p, root: r, severity: s, action: a });
        k += 1;
    }
    l
}

/// 四路对抗：空现象、越级、坏处置、重号。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = MicroLedger::new();
    out[0] = a.add(Diff { id: 1, phenomenon: "", root: "r", severity: 1, action: "接受" }) == Err(MD_EMPTY);
    out[1] = a.add(Diff { id: 1, phenomenon: "p", root: "r", severity: 4, action: "接受" }) == Err(MD_SEV);
    out[2] = a.add(Diff { id: 1, phenomenon: "p", root: "r", severity: 1, action: "忽略" }) == Err(MD_ACTION);
    let _ = a.add(Diff { id: 1, phenomenon: "p", root: "r", severity: 1, action: "修正" });
    out[3] = a.add(Diff { id: 1, phenomenon: "q", root: "r", severity: 2, action: "上报" }) == Err(MD_DUP);
    out
}

/// 判据入口。
pub fn run_unx11238_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11238");
    let mut l = build();
    let root = l.freeze().unwrap_or(0);

    s.add("J1-六条微差", l.diffs.len() == WANT, "微差账恰六条");
    s.add("J2-四列齐全", l.diffs.iter().all(|d| d.four_columns()), "现象/根因/危害级/处置四列均有值");
    s.add("J3-危害分布", l.high() == 1, "危害级 3 的条目恰一条");
    s.add("J4-处置白名单", l.diffs.iter().all(|d| d.action_ok()), "处置列全部落在接受/修正/上报");
    s.add("J5-接受两条", l.accept() == 2, "接受处置恰两条");
    s.add("J6-修正三条", l.fix() == 3, "修正处置恰三条");
    s.add("J7-上报一条", l.escalate() == 1, "上报处置恰一条");
    s.add("J8-零静默", l.silent_items().is_empty(), "静默条目清单为空");
    s.add("J9-冻存根", root != 0 && l.root == Some(root), "微差账冻存产出非零根");

    s.add("J10-冻后加拒", l.add(Diff { id: 7, phenomenon: "p", root: "r", severity: 1, action: "接受" }) == Err(MD_FROZEN), "冻存后新增微差拒为 MD_FROZEN");
    s.add("J11-越级拒", {
        let mut x = MicroLedger::new();
        x.add(Diff { id: 1, phenomenon: "p", root: "r", severity: 5, action: "接受" }) == Err(MD_SEV)
    }, "危害级五拒为 MD_SEV");
    s.add("J12-静默禁冻", {
        let mut x = build();
        x.diffs[0].action = "";
        x.freeze() == Err(MD_SILENT)
    }, "处置被清空的账冻存拒为 MD_SILENT");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "空现象/越级/坏处置/重号四路全拒");
    s.add_dyn(
        "J14-处置明细",
        l.accept() + l.fix() + l.escalate() == 6,
        &format!("接受 {} / 修正 {} / 上报 {}，高分 {}", l.accept(), l.fix(), l.escalate(), l.high()),
    );
    s.add_dyn(
        "J15-账根",
        root == l.fold(),
        &format!("根 0x{:x}，首条 {}", root, l.diffs[0].phenomenon),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11238_microdiff_flow() {
        let mut l = build();
        assert!(l.silent_items().is_empty());
        let r = l.freeze().unwrap_or(0);
        assert!(r != 0);
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11238_checks_green() {
        let s = run_unx11238_checks();
        assert!(s.all_passed(), "UNX-F11238 判据未全绿");
    }
}
