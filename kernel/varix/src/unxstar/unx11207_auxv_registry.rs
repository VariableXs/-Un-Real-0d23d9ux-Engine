//! UNX-F11207 glibc 内核行为依赖清单登记（auxv 项族）（0x1f807 段）
//!
//! 锚点：UNX-F11207-J1 glibc 启动期消费的 auxv 键位逐项登记（AT_PHDR/
//! AT_ENTRY/AT_PAGESZ/AT_RANDOM 等），与 C1 auxv 冻结接口逐项对账零缺项。
//! 域：CoRun·UNX-C5-B01——登记册按 C1 冻结键位表逐项落条目，键号与消费
//! 面双列；对账只比键号集合，缺项与多项都显性列出。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f807;
pub const AX_MISS: u32 = SEG_BASE + 0x01; // 对账缺项
pub const AX_EXTRA: u32 = SEG_BASE + 0x02; // 非冻结键位
pub const AX_DUP: u32 = SEG_BASE + 0x03; // 键号重录
pub const AX_ZERO: u32 = SEG_BASE + 0x04; // 键号零值
pub const AX_NOCONS: u32 = SEG_BASE + 0x05; // 消费面空
pub const AX_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后登记
pub const AX_OVER: u32 = SEG_BASE + 0x07; // 超容

/// C1 冻结 auxv 键位表（12 项，与 C1 交付逐字对齐）。
pub const C1_KEYS: [u32; 12] = [3, 6, 7, 8, 9, 11, 15, 16, 17, 23, 25, 31];
/// 登记容量。
pub const CAP: usize = 16;
pub const AX_TAG: u64 = 0x1f807c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单条 auxv 登记：键号、符号名、消费面、供给态。
#[derive(Clone, Debug)]
pub struct AuxItem {
    pub key: u32,
    pub name: &'static str,
    pub consumer: &'static str,
    pub supplied: bool,
}

impl AuxItem {
    pub fn complete(&self) -> bool {
        self.key != 0 && !self.name.is_empty() && !self.consumer.is_empty()
    }

    pub fn render(&self) -> String {
        format!("AT_{}={}|{}", self.name, self.key, if self.supplied { "y" } else { "n" })
    }
}

/// auxv 登记册。
#[derive(Clone, Debug)]
pub struct AuxReg {
    pub items: Vec<AuxItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl AuxReg {
    pub fn new() -> AuxReg {
        AuxReg { items: Vec::new(), frozen: false, root: None }
    }

    /// 登记：冻结/键零/消费面空/重号/超容五路拒。
    pub fn enroll(&mut self, key: u32, name: &'static str, consumer: &'static str, supplied: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(AX_FROZEN);
        }
        if key == 0 {
            return Err(AX_ZERO);
        }
        if consumer.is_empty() || name.is_empty() {
            return Err(AX_NOCONS);
        }
        if self.items.iter().any(|i| i.key == key) {
            return Err(AX_DUP);
        }
        if self.items.len() >= CAP {
            return Err(AX_OVER);
        }
        self.items.push(AuxItem { key, name, consumer, supplied });
        Ok(())
    }

    pub fn has(&self, key: u32) -> bool {
        self.items.iter().any(|i| i.key == key)
    }

    /// 与 C1 冻结表对账：返回缺项键号（升序）。
    pub fn missing_vs(&self, frozen_keys: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < frozen_keys.len() {
            if !self.has(frozen_keys[k]) {
                out.push(frozen_keys[k]);
            }
            k += 1;
        }
        out.sort();
        out
    }

    /// 非冻结键位（多项）。
    pub fn extras_vs(&self, frozen_keys: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !frozen_keys.contains(&self.items[k].key) {
                out.push(self.items[k].key);
            }
            k += 1;
        }
        out
    }

    /// 全供给判。
    pub fn all_supplied(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.supplied)
    }

    /// 消费面键号清单（去重前按登记序）。
    pub fn consumers(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            out.push(self.items[k].consumer);
            k += 1;
        }
        out
    }

    /// 对账零缺项零多项。
    pub fn reconciled(&self, frozen_keys: &[u32]) -> bool {
        self.missing_vs(frozen_keys).is_empty()
            && self.extras_vs(frozen_keys).is_empty()
            && self.items.len() == frozen_keys.len()
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = AX_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            h = (h ^ self.items[k].key as u64).wrapping_mul(FP_STEP);
            let nb = self.items[k].name.as_bytes();
            let mut b = 0usize;
            while b < nb.len() {
                h = (h ^ nb[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            let cb = self.items[k].consumer.as_bytes();
            let mut c = 0usize;
            while c < cb.len() {
                h = (h ^ cb[c] as u64).wrapping_mul(FP_STEP);
                c += 1;
            }
            h = (h ^ self.items[k].supplied as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 建册：按 C1 冻结表 12 键逐项登记。
pub fn build_reg() -> (AuxReg, bool) {
    let rows: [(u32, &'static str, &'static str); 12] = [
        (3, "PHDR", "ld.so 程序头遍历"),
        (6, "PAGESZ", "malloc 页粒度"),
        (7, "BASE", "解释器装载基址"),
        (8, "FLAGS", "执行标志"),
        (9, "ENTRY", "入口地址"),
        (11, "UID", "凭据缓存"),
        (15, "PLATFORM", "cpu 平台串"),
        (16, "HWCAP", "ifunc 选择"),
        (17, "CLKTCK", "时钟节拍"),
        (23, "SECURE", "安全执行态"),
        (25, "RANDOM", "栈保护随机数"),
        (31, "EXECFN", "程序名回显"),
    ];
    let mut r = AuxReg::new();
    let mut k = 0usize;
    while k < rows.len() {
        let supplied = r.enroll(rows[k].0, rows[k].1, rows[k].2, true).is_ok();
        if !supplied {
            return (r, false);
        }
        k += 1;
    }
    let ok = r.reconciled(&C1_KEYS) && r.all_supplied();
    (r, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 缺一项即对账非零缺项。
    let mut a = AuxReg::new();
    let mut k = 0usize;
    while k + 1 < C1_KEYS.len() {
        let key = C1_KEYS[k];
        let _ = a.enroll(key, "x", "c", true);
        k += 1;
    }
    got[0] = a.missing_vs(&C1_KEYS) == vec![31] && !a.reconciled(&C1_KEYS);
    // 2) 键号零值拒。
    let mut b = AuxReg::new();
    got[1] = b.enroll(0, "ZERO", "c", true).err() == Some(AX_ZERO);
    // 3) 非冻结键位显性列为多项。
    let mut c = AuxReg::new();
    let _ = c.enroll(3, "PHDR", "c", true);
    let _ = c.enroll(99, "EXTRA", "c", true);
    got[2] = c.extras_vs(&C1_KEYS) == vec![99];
    // 4) 消费面空与重号两路拒。
    let mut d = AuxReg::new();
    let _ = d.enroll(9, "ENTRY", "入口", true);
    got[3] = d.enroll(10, "X", "", true).err() == Some(AX_NOCONS)
        && d.enroll(9, "ENTRY2", "入口2", true).err() == Some(AX_DUP);
    got
}

/// 批内判据。
pub fn run_unx11207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11207");
    let (mut r, built) = build_reg();

    s.add("J1-十二键登记", built && r.items.len() == C1_KEYS.len(), "C1 冻结表 12 键逐项登记");
    s.add("J2-对账零缺项", r.missing_vs(&C1_KEYS).is_empty(), "与 C1 冻结接口逐项对账无缺");
    s.add("J3-对账零多项", r.extras_vs(&C1_KEYS).is_empty(), "无越界键位混入");
    s.add("J4-四键必在", r.has(3) && r.has(9) && r.has(6) && r.has(25),
        "AT_PHDR/AT_ENTRY/AT_PAGESZ/AT_RANDOM 四键齐备");
    s.add("J5-全供给", r.all_supplied(), "12 键全部由内核供给");
    s.add("J6-消费面非空", r.consumers().iter().all(|c| !c.is_empty()),
        "每条登记均指明 glibc 消费面");

    s.add("J7-条目完整", r.items.iter().all(|i| i.complete()), "键号/名/消费面三列完整");
    s.add("J8-键序可查", r.items[0].key == 3 && r.items[11].key == 31, "登记序首尾键号正确");
    s.add("J9-缺项显性", {
        let mut a = AuxReg::new();
        let _ = a.enroll(3, "PHDR", "c", true);
        a.missing_vs(&C1_KEYS).len() == 11
    }, "仅登记一键时缺项数为 11");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "缺项/零键/多项/空消费面重号四路必抓");

    let root = r.freeze();
    s.add("J11-冻结根", r.fold() == root && root != 0, "冻结后指纹可重算");
    s.add("J12-冻结拒登", r.enroll(40, "NEW", "c", true).err() == Some(AX_FROZEN),
        "冻结后新键登记必拒");

    let fp = r.fold();
    s.add_dyn("J13-对账明细", r.reconciled(&C1_KEYS) && fp == root,
        &format!("items={} fp={:#x}", r.items.len(), fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11207_auxv_flow() {
        let (mut r, built) = build_reg();
        assert!(built);
        assert!(r.reconciled(&C1_KEYS));
        assert!(r.has(25));
        assert_eq!(r.items.len(), 12);
        assert_eq!(r.enroll(6, "DUP", "c", true), Err(AX_DUP));
        let root = r.freeze();
        assert_eq!(r.fold(), root);
        assert_eq!(r.enroll(40, "X", "c", true), Err(AX_FROZEN));
        assert!(r.items[10].render().contains("AT_RANDOM=25"));
    }

    #[test]
    fn unx11207_checks_green() {
        let s = run_unx11207_checks();
        assert!(s.all_passed(), "UNX-F11207 判据未全绿");
    }
}
