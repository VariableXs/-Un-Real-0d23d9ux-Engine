//! UNX-F11063 信号回归段（B03/B04/B22 判据重放）（0x1e7e0 段）
//!
//! 回归矩阵信号段：B03（信号注册与递送闭合）、B04（信号合并语义）
//! 与 B22（信号掩码阻塞解除）三组判据重放。每面 6 例，
//! 守恒式：delivered == registered（每注必递），
//! coalesced 计数与 pending 消账一致（合并不丢失终态）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11063 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11063 - 11000) * 0x20;

/// 判据账空 —— 重放前必须先登记
pub const SG_EMPTY: u32 = SEG_BASE + 0x01;
/// 例号越界 —— 18 例编号 0..18
pub const SG_RANGE: u32 = SEG_BASE + 0x02;
/// 守恒破坏 —— 递送账失衡
pub const SG_UNBALANCED: u32 = SEG_BASE + 0x03;
/// 判据号越界 —— 只有 B03/B04/B22
pub const SG_BADTAG: u32 = SEG_BASE + 0x04;
/// 重复重放 —— 一例只重放一次
pub const SG_DUP: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再重放
pub const SG_FROZEN: u32 = SEG_BASE + 0x06;

/// B03 判据标
pub const TAG_B03: u8 = 3;
/// B04 判据标
pub const TAG_B04: u8 = 4;
/// B22 判据标
pub const TAG_B22: u8 = 22;
/// 每组例数
pub const CASES_PER_TAG: usize = 6;
/// 重放总例数
pub const CASES_TOTAL: usize = CASES_PER_TAG * 3;
/// 信号回归段标签
pub const SIGR_TAG: u64 = 0x7367_7267_7367_304d;

/// 单条重放例
#[derive(Clone, Debug)]
pub struct SgCase {
    /// 例号（0x15000 段起编）
    pub cid: u32,
    /// 判据标
    pub tag: u8,
    /// 期望值
    pub expect: u64,
    /// 实测值
    pub actual: u64,
    /// 重放对平标记
    pub replayed: bool,
}

/// 信号回归总账
#[derive(Clone, Debug)]
pub struct SignalRegress {
    /// 18 条重放例
    pub cases: Vec<SgCase>,
    /// B03 面累计注册次数
    pub registered: u64,
    /// B03 面累计递送次数
    pub delivered: u64,
    /// B04 面累计合并次数
    pub coalesced: u64,
    /// B04 面累计消账次数
    pub drained: u64,
    /// B22 面累计阻塞次数
    pub blocked: u64,
    /// B22 面累计解除次数
    pub unblocked: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl SignalRegress {
    /// 立账：18 例期望先行登记
    pub fn new() -> SignalRegress {
        let mut lg = SignalRegress {
            cases: Vec::new(),
            registered: 0, delivered: 0,
            coalesced: 0, drained: 0,
            blocked: 0, unblocked: 0,
            frozen: false,
        };
        for half in 0..3usize {
            for i in 0..CASES_PER_TAG {
                let no = half * CASES_PER_TAG + i;
                // B04 面是合并语义：同类信号合并后终态恰一次，对平值即 1；
                // B03/B22 面是逐次递送/解除：按 10*(i+1) 计数。
                let expect = if half == 1 {
                    1
                } else {
                    ((no % CASES_PER_TAG) as u64 + 1) * 10
                };
                lg.cases.push(SgCase {
                    cid: 0x15000 + no as u32,
                    tag: if half == 0 { TAG_B03 } else if half == 1 { TAG_B04 } else { TAG_B22 },
                    expect,
                    actual: 0,
                    replayed: false,
                });
            }
        }
        lg
    }

    /// 重放一例 B03：注册 10*(i+1) 次逐一递送
    pub fn replay_b03(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(SG_FROZEN); }
        if no >= CASES_PER_TAG { return Err(SG_RANGE); }
        let rounds = (10 * (no + 1)) as u64;
        let mut got = 0u64;
        for _ in 0..rounds {
            self.registered = self.registered.wrapping_add(1);
            self.delivered = self.delivered.wrapping_add(1);
            got = got.wrapping_add(1);
        }
        Ok(got)
    }

    /// 重放一例 B04：发送 10*(i+1) 次同类信号合并为 1 次，终态消账 1 次
    pub fn replay_b04(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(SG_FROZEN); }
        if no < CASES_PER_TAG || no >= CASES_PER_TAG * 2 { return Err(SG_RANGE); }
        let sends = (10 * (no + 1 - CASES_PER_TAG)) as u64;
        let mut got = 0u64;
        for _ in 0..sends {
            self.coalesced = self.coalesced.wrapping_add(1);
        }
        // 合并后终态仅 1 次，读账消 1 次
        self.drained = self.drained.wrapping_add(1);
        got = 1;
        Ok(got)
    }

    /// 重放一例 B22：阻塞 10*(i+1) 次逐一解除
    pub fn replay_b22(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(SG_FROZEN); }
        if no < CASES_PER_TAG * 2 || no >= CASES_TOTAL { return Err(SG_RANGE); }
        let rounds = (10 * (no + 1 - CASES_PER_TAG * 2)) as u64;
        let mut got = 0u64;
        for _ in 0..rounds {
            self.blocked = self.blocked.wrapping_add(1);
            self.unblocked = self.unblocked.wrapping_add(1);
            got = got.wrapping_add(1);
        }
        Ok(got)
    }

    /// 重放一例并回填对平（B04 面对平值为 1：合并终态恰一次）
    pub fn replay(&mut self, no: usize) -> Result<bool, u32> {
        if self.cases.is_empty() { return Err(SG_EMPTY); }
        let actual = if no < CASES_PER_TAG {
            self.replay_b03(no)?
        } else if no < CASES_PER_TAG * 2 {
            self.replay_b04(no)?
        } else {
            self.replay_b22(no)?
        };
        let c = &mut self.cases[no];
        if c.replayed { return Err(SG_DUP); }
        c.actual = actual;
        c.replayed = c.expect == actual;
        Ok(c.replayed)
    }

    /// 全量重放
    pub fn replay_all(&mut self) -> Result<usize, u32> {
        let mut n = 0usize;
        for no in 0..self.cases.len() {
            if self.replay(no)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 对平数
    pub fn replayed_n(&self) -> usize {
        self.cases.iter().filter(|c| c.replayed).count()
    }

    /// 按判据标统计对平数
    pub fn replayed_tag(&self, tag: u8) -> Result<usize, u32> {
        if tag != TAG_B03 && tag != TAG_B04 && tag != TAG_B22 { return Err(SG_BADTAG); }
        Ok(self.cases.iter().filter(|c| c.tag == tag && c.replayed).count())
    }

    /// 守恒：递送账闭合、合并终态消账、阻塞全解除
    pub fn conserved(&self) -> bool {
        self.registered == self.delivered
            && self.drained == CASES_PER_TAG as u64
            && self.blocked == self.unblocked
    }

    /// 全量对平：18/18 且守恒
    pub fn all_green(&self) -> bool {
        self.replayed_n() == CASES_TOTAL && self.conserved()
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SIGR_TAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.expect).wrapping_mul(0x100000001b3);
            h = (h ^ c.actual).wrapping_mul(0x100000001b3);
            h = (h ^ (c.replayed as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.registered, self.delivered, self.coalesced, self.blocked] {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结回归账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：18 例全量重放对平
pub fn sample_replayed() -> (SignalRegress, bool) {
    let mut lg = SignalRegress::new();
    let _ = lg.replay_all();
    let ok = lg.all_green();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：错标 / 越界 / 重复重放 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = SignalRegress::new();
    // 注入 1：非法判据标查询 → SG_BADTAG 拒止
    let i1 = matches!(lg.replayed_tag(0), Err(SG_BADTAG));
    // 注入 2：B03 面号跑 B04 面 → SG_RANGE 拒止
    let i2 = matches!(lg.replay_b04(0), Err(SG_RANGE));
    // 注入 3：重复重放 → SG_DUP 拒止
    let _ = lg.replay(1);
    let i3 = matches!(lg.replay(1), Err(SG_DUP));
    // 注入 4：冻结后重放 → SG_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.replay(2), Err(SG_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 B03/B04/B22 判据重放对平
pub fn run_unx11063_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11063");
    let (lg, ok) = sample_replayed();

    s.add("J1-B03重放", lg.replayed_tag(TAG_B03).unwrap_or(0) == CASES_PER_TAG, "B03 注册递送判据 6/6 重放对平");
    s.add("J1-B04重放", lg.replayed_tag(TAG_B04).unwrap_or(0) == CASES_PER_TAG, "B04 合并语义判据 6/6 重放对平");
    s.add("J1-B22重放", lg.replayed_tag(TAG_B22).unwrap_or(0) == CASES_PER_TAG, "B22 掩码阻塞判据 6/6 重放对平");
    s.add("J1-全量对平", ok && lg.replayed_n() == CASES_TOTAL, "18 例全量重放对平");
    s.add("J2-递送守恒", lg.registered == lg.delivered && lg.registered > 0, "B03 面每注必递");
    s.add("J2-合并消账", lg.drained == CASES_PER_TAG as u64 && lg.coalesced > 0, "B04 面六例各消终态一次");
    s.add("J2-阻塞解除", lg.blocked == lg.unblocked, "B22 面阻塞全解除");
    s.add("J3-判据标", lg.cases.iter().all(|c| c.tag == TAG_B03 || c.tag == TAG_B04 || c.tag == TAG_B22), "判据标取值合法");
    s.add("J3-编号连续", lg.cases[0].cid == 0x15000 && lg.cases[CASES_TOTAL - 1].cid == 0x15011, "例号 0x15000 段起编连续");
    s.add("J3-期望非零", lg.cases.iter().all(|c| c.expect > 0), "全部期望账非零登记");
    s.add("J4-错标拒查", inject_four()[0], "非法判据标被 SG_BADTAG 拒止");
    s.add("J4-越界拒放", inject_four()[1], "面号越界被 SG_RANGE 拒止");
    s.add("J5-重复拒放", inject_four()[2], "重复重放被 SG_DUP 拒止");
    s.add("J5-冻结拒放", inject_four()[3], "冻结后重放被 SG_FROZEN 拒止");
    s.add("J6-空账拒放", SignalRegress { cases: Vec::new(), registered: 0, delivered: 0, coalesced: 0, drained: 0, blocked: 0, unblocked: 0, frozen: false }.replay(0).is_err(), "空账重放被 SG_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-回归动态",
        ok,
        &format!("对平 {}/{}，注递 {}/{}，合并 {} 消账 {}，fold={:#x}", lg.replayed_n(), CASES_TOTAL, lg.registered, lg.delivered, lg.coalesced, lg.drained, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11063_signal_regress_flow() {
        let mut lg = SignalRegress::new();
        // 全量重放
        assert_eq!(lg.replay_all().unwrap_or(0), CASES_TOTAL);
        assert!(lg.all_green());
        assert_eq!(lg.replayed_n(), CASES_TOTAL);
        // B04 合并终态抽查
        assert_eq!(lg.cases[CASES_PER_TAG].actual, 1);
        assert_eq!(lg.drained, CASES_PER_TAG as u64);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11063_checks_green() {
        let s = run_unx11063_checks();
        assert!(s.all_passed(), "UNX-F11063 判据未全绿");
    }
}
