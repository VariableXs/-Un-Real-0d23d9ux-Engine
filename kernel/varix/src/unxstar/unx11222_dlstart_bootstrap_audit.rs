//! UNX-F11222 _dl_start 自举段内核消费（0x1f822 段）
//!
//! 锚点：UNX-F11222-J1 ld.so 自举期（重定位前）内核消费清单（mmap/self-open
//! 类）逐项登记，自举期零未登记消费。
//! 域：CoRun·UNX-C5-B02——自举期的白名单是封闭集：重定位之后出现的任何
//! 新消费都不属于自举段，登记动作本身要拒；未登记项一律计数并阻止冻存。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f822;
pub const DB_EMPTY: u32 = SEG_BASE + 0x01; // 消费项名空
pub const DB_UNREG: u32 = SEG_BASE + 0x02; // 未登记消费
pub const DB_PHASE: u32 = SEG_BASE + 0x03; // 重定位后仍按自举期登记
pub const DB_DUP: u32 = SEG_BASE + 0x04; // 同项重录
pub const DB_ORDER: u32 = SEG_BASE + 0x05; // 序错
pub const DB_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const DB_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 自举期白名单（八项）。
pub const WHITELIST: [&str; 8] = ["openat", "read", "mmap", "mprotect", "fstat", "close", "arch_prctl", "brk"];
pub const MIN_ITEMS: usize = 8;
pub const CAP: usize = 16;
pub const DB_TAG: u64 = 0x1f822d5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一条自举期消费。
#[derive(Clone, Debug)]
pub struct Consumption {
    pub op: &'static str,
    pub ord: u32,
    pub bytes: u64,
    pub checked: bool,
}

impl Consumption {
    pub fn in_whitelist(&self) -> bool {
        WHITELIST.iter().any(|w| *w == self.op)
    }
}

/// 自举期消费账。
#[derive(Clone, Debug)]
pub struct BootstrapAudit {
    pub items: Vec<Consumption>,
    pub relocated: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl BootstrapAudit {
    pub fn new() -> BootstrapAudit {
        BootstrapAudit { items: Vec::new(), relocated: false, frozen: false, root: None }
    }

    /// 登记一条自举期消费：空名/非白名单/重定位后/重项/超容五路拒。
    pub fn add(&mut self, op: &'static str, bytes: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(DB_FROZEN);
        }
        if op.is_empty() {
            return Err(DB_EMPTY);
        }
        if !WHITELIST.iter().any(|w| *w == op) {
            return Err(DB_UNREG);
        }
        if self.relocated {
            return Err(DB_PHASE);
        }
        if self.items.iter().any(|x| x.op == op) {
            return Err(DB_DUP);
        }
        if self.items.len() >= CAP {
            return Err(DB_OVER);
        }
        let ord = self.items.len() as u32;
        self.items.push(Consumption { op, ord, bytes, checked: true });
        Ok(())
    }

    /// 重定位完成：此后的消费不再计入自举段。
    pub fn reloc_done(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Err(DB_FROZEN);
        }
        if self.items.len() < MIN_ITEMS {
            return Err(DB_ORDER);
        }
        self.relocated = true;
        Ok(())
    }

    /// 序自证。
    pub fn ordered(&self) -> bool {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].ord != k as u32 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 未登记消费数（白名单外或未核验）。
    pub fn unregistered(&self) -> usize {
        self.items.iter().filter(|x| !x.in_whitelist() || !x.checked).count()
    }

    /// 自举期零未登记。
    pub fn clean(&self) -> bool {
        self.items.len() >= MIN_ITEMS && self.unregistered() == 0 && self.ordered() && self.relocated
    }

    pub fn mmap_bytes(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].op == "mmap" || self.items[k].op == "brk" {
                t = t.saturating_add(self.items[k].bytes);
            }
            k += 1;
        }
        t
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(DB_FROZEN);
        }
        if self.items.len() < MIN_ITEMS {
            return Err(DB_ORDER);
        }
        if !self.relocated {
            return Err(DB_PHASE);
        }
        if self.unregistered() != 0 {
            return Err(DB_UNREG);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("dls {}/{} unreg {}", self.items.len(), MIN_ITEMS, self.unregistered())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = DB_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let b = self.items[k].op.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.items[k].bytes).wrapping_mul(FP_STEP);
            h = (h ^ self.items[k].ord as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.relocated as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 八项消费的字节数（mmap/brk 记映射量，其余记请求长）。
pub const BYTES: [u64; 8] = [4096, 512, 2_097_152, 4096, 256, 0, 0, 65_536];

/// 组一张自举期干净的账。
pub fn build() -> BootstrapAudit {
    let mut a = BootstrapAudit::new();
    let mut k = 0usize;
    while k < WHITELIST.len() {
        let _ = a.add(WHITELIST[k], BYTES[k]);
        k += 1;
    }
    let _ = a.reloc_done();
    a
}

/// 四路对抗：白名单外、重项、重定位后补登、未重定位即冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = BootstrapAudit::new();
    out[0] = a.add("socket", 1) == Err(DB_UNREG);
    let _ = a.add("mmap", 4096);
    out[1] = a.add("mmap", 4096) == Err(DB_DUP);
    let mut b = build();
    out[2] = b.add("futex", 8) == Err(DB_UNREG);
    let mut c = BootstrapAudit::new();
    let mut k = 0usize;
    while k < WHITELIST.len() {
        let _ = c.add(WHITELIST[k], BYTES[k]);
        k += 1;
    }
    out[3] = c.freeze() == Err(DB_PHASE);
    out
}

/// 判据入口。
pub fn run_unx11222_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11222");
    let mut a = build();
    let root = a.freeze().unwrap_or(0);

    s.add("J1-八项齐", a.items.len() == MIN_ITEMS, "自举期消费恰登记八项");
    s.add("J2-白名单内", a.items.iter().all(|x| x.in_whitelist()), "八项全部落在封闭白名单内");
    s.add("J3-序自证", a.ordered(), "消费序 ord 与登记下标逐位一致");
    s.add("J4-零未登记", a.unregistered() == 0, "自举期未登记消费为零");
    s.add("J5-重定位已完", a.relocated, "重定位完成标记已置");
    s.add("J6-mmap 量", a.mmap_bytes() >= 2_162_688, "mmap+brk 映射量不低于二兆一");
    s.add("J7-冻存根", root != 0 && a.root == Some(root), "消费账冻存产出非零根");

    s.add("J8-冻后加拒", a.add("read", 1) == Err(DB_FROZEN), "冻存后新增消费拒为 DB_FROZEN");
    s.add("J9-冻后重定位拒", a.reloc_done() == Err(DB_FROZEN), "冻存后改重定位态拒为 DB_FROZEN");
    s.add("J10-白名单外拒", {
        let mut x = BootstrapAudit::new();
        x.add("ioctl", 4) == Err(DB_UNREG)
    }, "白名单外消费拒为 DB_UNREG");

    let mut d = build();
    s.add("J11-重定位后补登拒", d.add("close", 0) == Err(DB_PHASE), "重定位后按自举期补登拒为 DB_PHASE");
    let mut e = BootstrapAudit::new();
    let _ = e.add("mmap", 4096);
    s.add("J12-不足禁重定位", e.reloc_done() == Err(DB_ORDER), "消费不足八项时不得置重定位完成");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "白名单外/重项/重定位后/未重定位四路全拒");
    s.add_dyn(
        "J14-消费明细",
        a.items.len() == 8 && a.items.iter().all(|x| x.checked),
        &format!("八项全核验，mmap+brk 共 {} 字节，未登记 {}", a.mmap_bytes(), a.unregistered()),
    );
    s.add_dyn(
        "J15-账根",
        root == a.fold(),
        &format!("根 0x{:x}，首项 {}@{}", root, a.items[0].op, a.items[0].ord),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11222_dlstart_bootstrap_flow() {
        let mut a = build();
        assert!(a.clean());
        let r = a.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(a.add("read", 0), Err(DB_FROZEN));
        let mut b = BootstrapAudit::new();
        assert_eq!(b.add("execve", 0), Err(DB_UNREG));
        assert_eq!(b.reloc_done(), Err(DB_ORDER));
    }

    #[test]
    fn unx11222_checks_green() {
        let s = run_unx11222_checks();
        assert!(s.all_passed(), "UNX-F11222 判据未全绿");
    }
}
