//! VE-F4303 · 一致性×跨域联调（U06 场景应用组）
//! 职责：联调前契约包预交换（两域契约 diff 预演失配即阻断）+ 联调中违例
//! 即时提示（三通道——域本色）+ 快捷豁免通道（频次阈值防滥用收紧）
//! + 联调后违例账本并入处置流冲突仲裁。
//! 段位：0x3020（续表 idx537）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3020;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XdCode {
    /// 预交换失配（联调前阻断）。
    PreSwapFail = SEG_BASE,
    /// 快捷豁免滥用（频次阈值收紧）。
    ExemptAbuse = SEG_BASE + 0x01,
    /// 并入冲突（仲裁）。
    MergeClash = SEG_BASE + 0x02,
}

/// 契约包：(规则, 语义字)。
pub type ContractPack = Vec<(u16, u32)>;

/// 跨域联调器。
pub struct CrossDebug {
    /// 预交换包：域 A / 域 B。
    pub pack_a: ContractPack,
    pub pack_b: ContractPack,
    /// 联调违例账：(规则, 计数)。
    pub ledger: Vec<(u16, u32)>,
    /// 快捷豁免：(who, 次数)。
    pub exempts: Vec<(u8, u32)>,
    pub exempt_cap: u32,
    /// 提示三通道（域本色）。
    pub channels: [bool; 3],
    pub blocked: bool,
}

impl CrossDebug {
    pub fn new(exempt_cap: u32) -> Self {
        CrossDebug { pack_a: Vec::new(), pack_b: Vec::new(), ledger: Vec::new(), exempts: Vec::new(), exempt_cap, channels: [true; 3], blocked: false }
    }

    /// 预交换 diff 预演：同规则语义不同=失配阻断。
    pub fn preswap(&mut self) -> Result<Vec<u16>, XdCode> {
        let mut clashes = Vec::new();
        for (r, va) in &self.pack_a {
            if let Some((_, vb)) = self.pack_b.iter().find(|(rb, _)| rb == r) {
                if va != vb {
                    clashes.push(*r);
                }
            }
        }
        if !clashes.is_empty() {
            self.blocked = true; // 联调前阻断
            return Err(XdCode::PreSwapFail);
        }
        Ok(clashes)
    }

    /// 联调中违例即时提示（三通道）。
    pub fn hit(&mut self, rule: u16) {
        match self.ledger.iter_mut().find(|(r, _)| *r == rule) {
            Some((_, c)) => *c += 1,
            None => self.ledger.push((rule, 1)),
        }
    }

    pub fn channels_ok(&self) -> bool {
        self.channels.iter().all(|c| *c)
    }

    /// 快捷豁免：频次阈值内放行；超阈收紧拒绝。
    pub fn quick_exempt(&mut self, who: u8) -> Result<(), XdCode> {
        match self.exempts.iter_mut().find(|(w, _)| *w == who) {
            Some((_, c)) => {
                *c += 1;
                if *c > self.exempt_cap {
                    return Err(XdCode::ExemptAbuse); // 阈值收紧
                }
                Ok(())
            }
            None => {
                self.exempts.push((who, 1));
                Ok(())
            }
        }
    }

    /// 并入处置流：与主账冲突（同规则已有更高计数）→仲裁取大。
    pub fn merge_into(&mut self, main: &mut Vec<(u16, u32)>) -> Result<usize, XdCode> {
        if self.blocked {
            return Err(XdCode::MergeClash);
        }
        let mut merged = 0;
        for (r, c) in self.ledger.iter() {
            match main.iter_mut().find(|(mr, _)| *mr == *r) {
                Some(mc) => {
                    if mc.1 < *c {
                        mc.1 = *c; // 仲裁取大
                    }
                    merged += 1;
                }
                None => {
                    main.push((*r, *c));
                    merged += 1;
                }
            }
        }
        Ok(merged)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C537-01..12。
pub fn run_ves537_checks() -> CheckSet {
    let mut set = CheckSet::new("ves537");

    let mut xd = CrossDebug::new(2);
    xd.pack_a = alloc::vec![(10, 1), (20, 2)];
    xd.pack_b = alloc::vec![(10, 1), (20, 3)];
    chk(&mut set, "C537-01", xd.preswap() == Err(XdCode::PreSwapFail), "预交换失配（规则20 语义异）");
    chk(&mut set, "C537-02", xd.blocked, "联调前阻断在册");
    xd.pack_b[1].1 = 2;
    chk(&mut set, "C537-03", xd.preswap() == Ok(alloc::vec![]), "修正后预演通过");
    chk(&mut set, "C537-04", !xd.blocked, "阻断解除");

    // 联调违例
    xd.hit(10);
    xd.hit(10);
    xd.hit(30);
    chk(&mut set, "C537-05", xd.ledger == alloc::vec![(10, 2), (30, 1)], "违例账计数");
    chk(&mut set, "C537-06", xd.channels_ok(), "即时提示三通道齐（域本色）");

    // 快捷豁免
    chk(&mut set, "C537-07", xd.quick_exempt(1).is_ok() && xd.quick_exempt(1).is_ok(), "豁免阈值内放行");
    chk(&mut set, "C537-08", xd.quick_exempt(1) == Err(XdCode::ExemptAbuse), "第 3 次超阈收紧拒绝");

    // 并入处置流
    let mut main: Vec<(u16, u32)> = alloc::vec![(10, 1), (40, 5)];
    let n = xd.merge_into(&mut main).unwrap_or(0);
    chk(&mut set, "C537-09", n == 2, "并入两条");
    chk(&mut set, "C537-10", main.iter().find(|(r, _)| *r == 10).map(|(_, c)| *c) == Some(2), "冲突仲裁取大（2>1）");
    chk(&mut set, "C537-11", main.iter().any(|(r, c)| *r == 40 && *c == 5), "主账原有不覆盖");
    chk(&mut set, "C537-12", main.len() == 3, "并入后主账三条");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_debug() {
        let mut xd = CrossDebug::new(1);
        xd.pack_a = alloc::vec![(1, 1)];
        xd.pack_b = alloc::vec![(1, 1)];
        assert!(xd.preswap().is_ok());
        xd.hit(1);
        assert!(xd.quick_exempt(1).is_ok());
        assert!(xd.quick_exempt(1).is_err());
    }
}
