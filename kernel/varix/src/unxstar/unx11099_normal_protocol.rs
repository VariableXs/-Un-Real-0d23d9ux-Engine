//! UNX-F11099 长稳常态化协议（纳入回归清单）（0x1ec60 段）
//!
//! 锚点：UNX-F11099-J1 常态化协议逐项立账（10/10），清单闭合断言（10/10）。
//! 域：长稳收尾——常态化协议逐项立账（频次、门限、回滚位、责任面），逐项
//! 挂入回归清单，清单闭合断言（清单号连续、每项协议齐备、门限非零、
//! 回滚位定值）全过才算常态化成立。
//!
//! 结构：协议立账册（ProtocolBook，协议登记→门限定值→回归挂账→闭合断言）+
//! 回归清单（RegList，清单项序号敏感链式 fold）+ 注入核（门限零值/
//! 重号/清单断裂/未挂账闭合四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1ec60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ec60;
pub const CC_ZERO: u32 = SEG_BASE + 0x01; // 门限零值
pub const CC_DUP: u32 = SEG_BASE + 0x02; // 协议号重号
pub const CC_BREAK: u32 = SEG_BASE + 0x03; // 清单号断裂
pub const CC_UNLISTED: u32 = SEG_BASE + 0x04; // 未挂账先闭合
pub const CC_EMPTY: u32 = SEG_BASE + 0x05; // 协议名空
pub const CC_ROLL: u32 = SEG_BASE + 0x06; // 回滚位非法
pub const CC_OVER: u32 = SEG_BASE + 0x07; // 超定容

/// 协议定容（>10，容量先定后用）。
pub const CAP: usize = 20;
/// 常态化验收件数（判据 10/10）。
pub const NEEDED: usize = 10;
/// 常态化锚（fold 种子）。
pub const NORM_TAG: u64 = 0x1ec60cc;
/// 回滚位定值：0=只报 1=自动回滚。
pub const ROLL_REPORT: u32 = 0;
pub const ROLL_AUTO: u32 = 1;
/// 指纹步进常量。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单条常态化协议：门限 + 回滚位 + 回归挂账位。
#[derive(Clone, Debug, PartialEq)]
pub struct Protocol {
    pub no: u32,
    pub name: String,
    pub freq: u32,
    pub threshold: u32,
    pub rollback: u32,
    pub listed: bool,
}

impl Protocol {
    pub fn legal_rollback(&self) -> bool {
        self.rollback == ROLL_REPORT || self.rollback == ROLL_AUTO
    }

    /// 协议齐备判：名非空、门限非零、回滚位合法、已挂账。
    pub fn complete(&self) -> bool {
        !self.name.is_empty() && self.threshold > 0 && self.legal_rollback() && self.listed
    }

    pub fn render(&self) -> String {
        let r = if self.rollback == ROLL_AUTO { "A" } else { "R" };
        format!("P{}|{}|t{}", self.no, r, self.threshold)
    }
}

/// 回归清单：清单项（协议号序），闭合断言载体。
#[derive(Debug)]
pub struct RegList {
    pub entries: Vec<u32>,
}

impl RegList {
    pub fn new() -> RegList {
        RegList { entries: Vec::new() }
    }

    /// 挂账：协议号按序入清单，重号拒。
    pub fn enlist(&mut self, no: u32) -> Result<(), u32> {
        if self.entries.contains(&no) {
            return Err(CC_DUP);
        }
        self.entries.push(no);
        self.entries.sort();
        Ok(())
    }

    /// 闭合断言：清单号从 1 连续递增无断裂。
    pub fn closed(&self) -> bool {
        !self.entries.is_empty()
            && self.entries[0] == 1
            && {
                let mut k = 1usize;
                while k < self.entries.len() {
                    if self.entries[k] != self.entries[k - 1] + 1 {
                        return false;
                    }
                    k += 1;
                }
                true
            }
    }

    /// 清单指纹：号序敏感链式 fold。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = NORM_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            h = (h ^ self.entries[k] as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 常态化协议立账册：登记→定门限→挂回归清单→闭合断言。
#[derive(Debug)]
pub struct ProtocolBook {
    pub items: Vec<Protocol>,
    pub list: RegList,
}

impl ProtocolBook {
    pub fn new() -> ProtocolBook {
        ProtocolBook { items: Vec::new(), list: RegList::new() }
    }

    /// 协议登记：重号、空名、超容拒。
    pub fn enroll(&mut self, no: u32, name: &str, freq: u32) -> Result<(), u32> {
        if name.is_empty() {
            return Err(CC_EMPTY);
        }
        if self.items.iter().any(|i| i.no == no) {
            return Err(CC_DUP);
        }
        if self.items.len() >= CAP {
            return Err(CC_OVER);
        }
        self.items.push(Protocol {
            no,
            name: name.to_string(),
            freq,
            threshold: 0,
            rollback: ROLL_REPORT,
            listed: false,
        });
        Ok(())
    }

    /// 门限定值：零值拒、冻结（已挂账）拒。
    pub fn set_threshold(&mut self, no: u32, threshold: u32) -> Result<(), u32> {
        if threshold == 0 {
            return Err(CC_ZERO);
        }
        let it = self.find(no).ok_or(CC_UNLISTED)?;
        if it.listed {
            return Err(CC_UNLISTED);
        }
        it.threshold = threshold;
        Ok(())
    }

    /// 回滚位定值：非法值拒、冻结拒。
    pub fn set_rollback(&mut self, no: u32, rollback: u32) -> Result<(), u32> {
        if rollback != ROLL_REPORT && rollback != ROLL_AUTO {
            return Err(CC_ROLL);
        }
        let it = self.find(no).ok_or(CC_UNLISTED)?;
        if it.listed {
            return Err(CC_UNLISTED);
        }
        it.rollback = rollback;
        Ok(())
    }

    /// 回归挂账：门限未定拒、回滚位非法拒；挂账即冻结。
    pub fn enlist(&mut self, no: u32) -> Result<(), u32> {
        let it = self.find(no).ok_or(CC_UNLISTED)?;
        if it.threshold == 0 {
            return Err(CC_ZERO);
        }
        if !it.legal_rollback() {
            return Err(CC_ROLL);
        }
        it.listed = true;
        self.list.enlist(no)
    }

    /// 常态化裁断：NEEDED 项全立账且全挂清单且清单闭合。
    pub fn normalized(&self) -> bool {
        self.items.len() == NEEDED
            && self.items.iter().all(|i| i.complete())
            && self.list.closed()
            && self.list.entries.len() == NEEDED
    }

    /// 未挂账协议清单（按登记序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].listed {
                g.push(self.items[k].no);
            }
            k += 1;
        }
        g
    }

    fn find(&mut self, no: u32) -> Option<&mut Protocol> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                return Some(&mut self.items[k]);
            }
            k += 1;
        }
        None
    }

    /// 册级指纹：协议与清单双链 fold。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = NORM_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            h = (h ^ self.items[k].no as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.items[k].threshold as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.items[k].rollback as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.items[k].listed as u64).wrapping_mul(FP_STEP);
            let bytes = self.items[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h = (h ^ self.list.fold()).wrapping_mul(FP_STEP);
        h
    }
}

/// 协议名样本（确定性，正反两路共用）。
pub fn proto_name(no: u32) -> String {
    format!("长稳常态协议 {}", no)
}

/// 标准回滚位：偶数号自动回滚，奇数号只报。
pub fn sample_rollback(no: u32) -> u32 {
    if no % 2 == 0 { ROLL_AUTO } else { ROLL_REPORT }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 门限零值必拒
    let mut z = ProtocolBook::new();
    let _ = z.enroll(1, "零门限", 1);
    got[0] = z.set_threshold(1, 0).err() == Some(CC_ZERO);
    // 2) 重号必拒
    let mut d = ProtocolBook::new();
    let _ = d.enroll(1, "甲", 1);
    got[1] = d.enroll(1, "乙", 2).err() == Some(CC_DUP);
    // 3) 清单号断裂闭合必假（跳过 3 号直挂 4 号）
    let mut b = ProtocolBook::new();
    let _ = b.enroll(2, "断二", 1);
    let _ = b.enroll(4, "断四", 1);
    let _ = b.set_threshold(2, 5);
    let _ = b.set_threshold(4, 5);
    let _ = b.enlist(2);
    let _ = b.enlist(4);
    got[2] = !b.list.closed();
    // 4) 未挂账闭合必假（全登记但不挂清单）
    let mut u = ProtocolBook::new();
    let mut n = 1u32;
    while n <= NEEDED as u32 {
        let _ = u.enroll(n, &proto_name(n), 1);
        let _ = u.set_threshold(n, 9);
        let _ = u.set_rollback(n, sample_rollback(n));
        n += 1;
    }
    got[3] = !u.normalized() && u.gaps().len() == NEEDED;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11099_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11099");

    // J1 常态化协议逐项立账（10/10）+ 清单闭合断言（10/10）
    let mut book = ProtocolBook::new();
    let mut n = 1u32;
    let mut done = 0usize;
    while n <= NEEDED as u32 {
        let ok_en = book.enroll(n, &proto_name(n), n * 7).is_ok();
        let ok_th = book.set_threshold(n, n * 11).is_ok();
        let ok_rb = book.set_rollback(n, sample_rollback(n)).is_ok();
        let ok_li = book.enlist(n).is_ok();
        if !(ok_en && ok_th && ok_rb && ok_li) {
            break;
        }
        done += 1;
        n += 1;
    }
    s.add(
        "J1-常态化立账十项",
        done == NEEDED && book.normalized(),
        "10 项协议逐项立账并挂入回归清单",
    );

    // J2 清单闭合断言：清单号 1..10 连续且闭合为真
    let j2 = book.list.closed() && book.list.entries.len() == NEEDED;
    s.add("J2-清单闭合", j2, "回归清单号连续无断裂");

    // J3 注入检出：零门限 + 重号
    let inj = inject_four();
    s.add("J3-零门限重号检出", inj[0] && inj[1], "门限零值与协议重号两路必抓");

    // J4 注入检出：清单断裂 + 未挂账闭合
    s.add("J4-断裂未挂账检出", inj[2] && inj[3], "清单断裂与未挂账闭合两路必抓");

    // J5 冻结语义：挂账后改门限必拒
    let j5 = {
        let mut c = ProtocolBook::new();
        let _ = c.enroll(1, "冻结样本", 3);
        let _ = c.set_threshold(1, 7);
        let _ = c.enlist(1);
        c.set_threshold(1, 9).err() == Some(CC_UNLISTED)
    };
    s.add("J5-挂账后冻结", j5, "挂账后改门限必拒");

    // J6 回滚位非法拒
    let j6 = {
        let mut c = ProtocolBook::new();
        let _ = c.enroll(2, "回滚样本", 2);
        c.set_rollback(2, 7).err() == Some(CC_ROLL)
    };
    s.add("J6-回滚位非法拒", j6, "回滚位非 0/1 必拒");

    // J7 未定门限挂账拒
    let j7 = {
        let mut c = ProtocolBook::new();
        let _ = c.enroll(3, "未定门限", 4);
        c.enlist(3).err() == Some(CC_ZERO)
    };
    s.add("J7-未定门限挂账拒", j7, "门限未定先挂账必拒");

    // J8 空名拒
    let j8 = ProtocolBook::new().enroll(1, "", 1).err() == Some(CC_EMPTY);
    s.add("J8-空名拒收", j8, "协议名空必拒");

    // J9 容量边界：第 CAP+1 条登记被拒且原账不损
    let j9 = {
        let mut c = ProtocolBook::new();
        let mut k = 1u32;
        while k <= CAP as u32 {
            if c.enroll(k, "x", 1).is_err() {
                break;
            }
            k += 1;
        }
        c.enroll(999, "over", 1).err() == Some(CC_OVER) && c.items.len() == CAP
    };
    s.add("J9-容量边界", j9, "超出定容必拒且原账不损");

    // J10 渲染面：自动回滚与只报回滚可区分
    let j10 = {
        let mut c = ProtocolBook::new();
        let _ = c.enroll(1, "渲染甲", 1);
        let _ = c.enroll(2, "渲染乙", 1);
        let _ = c.set_threshold(1, 5);
        let _ = c.set_threshold(2, 5);
        let _ = c.set_rollback(1, ROLL_AUTO);
        c.items[0].render() != c.items[1].render()
            && c.items[0].render().contains("A")
            && c.items[1].render().contains("R")
    };
    s.add("J10-渲染分旗", j10, "回滚位两态渲染可区分");

    // J11 缺口枚举：漏挂一项时 gaps 可枚举且 normalized 为假
    let j11 = {
        let mut c = ProtocolBook::new();
        let mut k = 1u32;
        while k <= NEEDED as u32 {
            let _ = c.enroll(k, &proto_name(k), 1);
            let _ = c.set_threshold(k, 3);
            if k < NEEDED as u32 {
                let _ = c.enlist(k);
            }
            k += 1;
        }
        c.gaps().len() == 1 && !c.normalized()
    };
    s.add("J11-缺口枚举", j11, "未挂账项可枚举且常态化为假");

    // J12 未翻位不算常态化：值齐但未挂清单
    let j12 = {
        let mut c = ProtocolBook::new();
        let mut k = 1u32;
        while k <= NEEDED as u32 {
            let _ = c.enroll(k, &proto_name(k), 1);
            let _ = c.set_threshold(k, 4);
            k += 1;
        }
        !c.normalized() && c.gaps().len() == NEEDED
    };
    s.add("J12-未挂账不算常态", j12, "协议齐但未挂清单仍不算常态化");

    // J13 清单指纹稳定：同清单两次 fold 一致且非零
    let j13 = {
        let f1 = book.list.fold();
        f1 != 0 && f1 == book.list.fold()
    };
    s.add("J13-清单指纹稳定", j13, "清单指纹可重算零漂移");

    // J14 动态明细：册指纹与缺口数
    let fp = book.fold();
    let g = book.gaps().len();
    s.add_dyn("J14-常态册明细", g == 0 && fp != 0, &format!("gaps={} fp={:#x}", g, fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11099_normal_flow() {
        let mut l = ProtocolBook::new();
        assert!(l.enroll(1, "a", 1).is_ok());
        assert!(l.set_threshold(1, 5).is_ok());
        assert!(l.set_threshold(1, 0).err() == Some(CC_ZERO));
        assert!(l.set_rollback(1, ROLL_AUTO).is_ok());
        assert!(l.enlist(1).is_ok());
        assert!(l.set_threshold(1, 9).err() == Some(CC_UNLISTED));
        assert!(l.items[0].complete());
        assert!(l.list.closed());
        assert!(l.enroll(1, "dup", 2).err() == Some(CC_DUP));
    }

    #[test]
    fn unx11099_checks_green() {
        let s = run_unx11099_checks();
        assert!(s.all_passed(), "UNX-F11099 判据未全绿");
    }
}
