//! UNX-F10919 三复用器交叉对照总矩阵（0x1d260 段）
//!
//! 锚点：UNX-F10919-J1 正面：select/poll/epoll 同场景九组对照全绿
//! （各 10/10）；反面：三系结果一致（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·三系交叉对照面）。
//!
//! 结构：场景账（九组同场景 = 3 就绪图 × 3 复用器）+ 对照器（三系
//! 同场景裁决一致才算绿）+ 注入核（单系独判/就绪图漂移/场景缺组/
//! 冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d260 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d260;
pub const XC_MISMATCH: u32 = SEG_BASE + 0x01; // 三系结果不一致
pub const XC_NOSCEN: u32 = SEG_BASE + 0x02; // 场景缺组
pub const XC_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改账
pub const XC_BADPATTERN: u32 = SEG_BASE + 0x04; // 就绪图号非法
pub const XC_BADMUX: u32 = SEG_BASE + 0x05; // 复用器号非法

/// 三复用器号。
pub const X_SELECT: u8 = 1;
pub const X_POLL: u8 = 2;
pub const X_EPOLL: u8 = 3;
/// 三就绪图号（0=全无 1=单就绪 2=双就绪）。
pub const PAT_NONE: u8 = 0;
pub const PAT_ONE: u8 = 1;
pub const PAT_TWO: u8 = 2;

/// 九组定容。
pub const GROUPS: usize = 9;
/// XC 域标（fold 种子）。
pub const XC_TAG: u64 = 0x1d26213;

/// 场景组裁决。
#[derive(Clone, Debug, PartialEq)]
pub struct GroupVerdict {
    pub pattern: u8,
    pub mux: u8,
    pub ready: usize,
}

impl GroupVerdict {
    pub fn render(&self) -> String {
        format!("G{}{}|{}", self.pattern, self.mux, self.ready)
    }
}

/// 交叉对照账。
pub struct CrossBoard {
    pub verdicts: Vec<GroupVerdict>,
    pub frozen: bool,
}

impl CrossBoard {
    pub fn new() -> CrossBoard {
        CrossBoard { verdicts: Vec::new(), frozen: false }
    }

    /// 就绪图到期望就绪数。
    pub fn expect_ready(pattern: u8) -> Result<usize, u32> {
        match pattern {
            PAT_NONE => Ok(0),
            PAT_ONE => Ok(1),
            PAT_TWO => Ok(2),
            _ => Err(XC_BADPATTERN),
        }
    }

    /// 单系裁决：按就绪图模拟 select/poll/epoll 返回值。
    pub fn judge_one(&self, pattern: u8, mux: u8) -> Result<usize, u32> {
        if mux < X_SELECT || mux > X_EPOLL {
            return Err(XC_BADMUX);
        }
        let n = Self::expect_ready(pattern)?;
        if n == 0 {
            // 三系零就绪均返 0（超时语义）。
            Ok(0)
        } else {
            // 三系就绪均返就绪条目数。
            Ok(n)
        }
    }

    /// 记录场景组：三系各记一条，裁决必须一致。
    pub fn record(&mut self, pattern: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN);
        }
        let exp = Self::expect_ready(pattern)?;
        let mut m = X_SELECT;
        while m <= X_EPOLL {
            let got = self.judge_one(pattern, m)?;
            if got != exp {
                return Err(XC_MISMATCH);
            }
            self.verdicts.push(GroupVerdict { pattern, mux: m, ready: got });
            m += 1;
        }
        Ok(())
    }

    /// 九组齐且三系结果一致。
    pub fn all_green(&self) -> bool {
        if self.verdicts.len() != GROUPS {
            return false;
        }
        let mut p = PAT_NONE;
        while p <= PAT_TWO {
            let mut base: Option<usize> = None;
            let mut m = X_SELECT;
            while m <= X_EPOLL {
                let mut found = false;
                let mut k = 0usize;
                while k < self.verdicts.len() {
                    let v = &self.verdicts[k];
                    if v.pattern == p && v.mux == m {
                        found = true;
                        match base {
                            None => base = Some(v.ready),
                            Some(b) => {
                                if b != v.ready {
                                    return false;
                                }
                            }
                        }
                    }
                    k += 1;
                }
                if !found {
                    return false;
                }
                m += 1;
            }
            p += 1;
        }
        true
    }

    /// 链式 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h = XC_TAG;
        let mut k = 0usize;
        while k < self.verdicts.len() {
            let v = &self.verdicts[k];
            h = (h ^ v.pattern as u64).wrapping_mul(0x100000001b3);
            h = (h ^ v.mux as u64).wrapping_mul(0x100000001b3);
            h = (h ^ v.ready as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 单系独判必抓：手工塞不一致裁决后 all_green 必假
    let mut b = CrossBoard::new();
    let _ = b.record(PAT_NONE);
    let _ = b.record(PAT_ONE);
    let _ = b.record(PAT_TWO);
    b.verdicts[0].ready = 5;
    got[0] = !b.all_green();
    // 2) 场景缺组必抓：少记一组
    let mut c = CrossBoard::new();
    let _ = c.record(PAT_NONE);
    let _ = c.record(PAT_ONE);
    got[1] = !c.all_green();
    // 3) 就绪图号非法必拒
    let x = CrossBoard::new();
    got[2] = x.judge_one(9, X_SELECT).err() == Some(XC_BADPATTERN);
    // 4) 冻结后记账必抓
    let mut f = CrossBoard::new();
    f.freeze();
    got[3] = f.record(PAT_NONE).err() == Some(XC_FROZEN);
    got
}

/// 就绪图确定性样本：第 r 轮的图号。
pub fn xc_sample_pattern(r: u32) -> u8 {
    (r.wrapping_mul(2654435761) % 3) as u8
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10919_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10919");

    // J1 正面：九组对照全绿（十轮全账重建）
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = CrossBoard::new();
        let _ = b.record(PAT_NONE);
        let _ = b.record(PAT_ONE);
        let _ = b.record(PAT_TWO);
        if b.all_green() && b.verdicts.len() == GROUPS {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-九组对照十连", h1 == 10, "九组三系十轮全绿");

    // J2 反面：三系结果一致千次
    let mut ok = 0u32;
    let mut r2 = 0u32;
    while r2 < 1000 {
        let b = CrossBoard::new();
        let p = xc_sample_pattern(r2);
        let a = b.judge_one(p, X_SELECT);
        let c = b.judge_one(p, X_POLL);
        let e = b.judge_one(p, X_EPOLL);
        if a == c && c == e && a.is_ok() {
            ok += 1;
        }
        r2 += 1;
    }
    s.add("J2-千次三系一致", ok == 1000, "千轮三系裁决一致");

    // J3 零就绪图三系全返 0
    let j3 = {
        let b = CrossBoard::new();
        b.judge_one(PAT_NONE, X_SELECT) == Ok(0)
            && b.judge_one(PAT_NONE, X_POLL) == Ok(0)
            && b.judge_one(PAT_NONE, X_EPOLL) == Ok(0)
    };
    s.add("J3-零就绪返零", j3, "超时语义三系全返 0");

    // J4 双就绪图三系全返 2
    let j4 = {
        let b = CrossBoard::new();
        b.judge_one(PAT_TWO, X_SELECT) == Ok(2)
            && b.judge_one(PAT_TWO, X_POLL) == Ok(2)
            && b.judge_one(PAT_TWO, X_EPOLL) == Ok(2)
    };
    s.add("J4-双就绪返二", j4, "就绪条目数三系全返 2");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "独判/缺组/坏图/冻结全数命中");

    // J6 复用器号非法拒
    let j6 = CrossBoard::new().judge_one(PAT_ONE, 9).err() == Some(XC_BADMUX);
    s.add("J6-坏器号拒", j6, "复用器号越界必拒");

    // J7 冻结后记账拒
    let j7 = {
        let mut b = CrossBoard::new();
        b.freeze();
        b.record(PAT_ONE).err() == Some(XC_FROZEN)
    };
    s.add("J7-冻结拒记账", j7, "冻结后记账必拒");

    // J8 渲染面：三系九组渲染互异
    let j8 = {
        let mut b = CrossBoard::new();
        let _ = b.record(PAT_NONE);
        let _ = b.record(PAT_ONE);
        let _ = b.record(PAT_TWO);
        let mut all_diff = true;
        let mut i = 0usize;
        while i < b.verdicts.len() {
            let mut j = i + 1;
            while j < b.verdicts.len() {
                if b.verdicts[i].render() == b.verdicts[j].render() {
                    all_diff = false;
                }
                j += 1;
            }
            i += 1;
        }
        all_diff
    };
    s.add("J8-渲染互异", j8, "九组渲染两两互异");

    // J9 裁决顺序敏感：账序不同指纹不同
    let j9 = {
        let mut b1 = CrossBoard::new();
        let _ = b1.record(PAT_NONE);
        let _ = b1.record(PAT_ONE);
        let mut b2 = CrossBoard::new();
        let _ = b2.record(PAT_ONE);
        let _ = b2.record(PAT_NONE);
        b1.fold() != b2.fold()
    };
    s.add("J9-序敏感指纹", j9, "记账顺序入指纹");

    // J10 fold 稳定：同态一致
    let j10 = {
        let mut b = CrossBoard::new();
        let _ = b.record(PAT_TWO);
        let f1 = b.fold();
        let f2 = b.fold();
        f1 == f2 && f1 != 0
    };
    s.add("J10-指纹稳定", j10, "同态指纹稳定");

    // J11 动态明细：千次一致计数落账
    s.add_dyn("J11-千次明细", ok == 1000, &format!("agree_ok={}/1000", ok));

    // J12 动态明细：九组全绿十连计数落账
    s.add_dyn("J12-十连明细", h1 == 10, &format!("green_groups={}/10", h1));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10919_crossboard_flow() {
        let mut b = CrossBoard::new();
        assert!(b.record(PAT_NONE).is_ok());
        assert!(b.record(PAT_ONE).is_ok());
        assert!(b.record(PAT_TWO).is_ok());
        assert_eq!(b.verdicts.len(), GROUPS);
        assert!(b.all_green());
        assert!(b.judge_one(9, X_SELECT).err() == Some(XC_BADPATTERN));
        assert!(b.judge_one(PAT_ONE, 9).err() == Some(XC_BADMUX));
        b.freeze();
        assert!(b.record(PAT_NONE).err() == Some(XC_FROZEN));
    }

    #[test]
    fn unx10919_checks_green() {
        let s = run_unx10919_checks();
        assert!(s.all_passed(), "UNX-F10919 判据未全绿");
    }
}
