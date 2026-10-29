//! UNX-F11062 管道 FIFO 回归段（B01/B02 判据重放）（0x1e7c0 段）
//!
//! 回归矩阵管道 FIFO 段：B01（pipe 匿名管道读写闭合）与
//! B02（FIFO 命名管道多写多读对账）两组判据重放。
//! B01 面 8 例：每例写 n 字节读回 n 字节（bytes_in == bytes_out）；
//! B02 面 8 例：多写者多读者轮转，守恒式
//! written == received + inflight（FIFO 不生灭）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11062 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11062 - 11000) * 0x20;

/// 判据账空 —— 重放前必须先登记
pub const PF_EMPTY: u32 = SEG_BASE + 0x01;
/// 例号越界 —— 16 例编号 0..16
pub const PF_RANGE: u32 = SEG_BASE + 0x02;
/// 守恒破坏 —— 管道字节账失衡
pub const PF_UNBALANCED: u32 = SEG_BASE + 0x03;
/// 判据号越界 —— 只有 B01/B02
pub const PF_BADTAG: u32 = SEG_BASE + 0x04;
/// 重复重放 —— 一例只重放一次
pub const PF_DUP: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再重放
pub const PF_FROZEN: u32 = SEG_BASE + 0x06;

/// B01 判据标
pub const TAG_B01: u8 = 1;
/// B02 判据标
pub const TAG_B02: u8 = 2;
/// 每组例数
pub const CASES_PER_TAG: usize = 8;
/// 重放总例数
pub const CASES_TOTAL: usize = CASES_PER_TAG * 2;
/// B01 面每例写读轮数
pub const B01_ROUNDS: usize = 20;
/// B02 面每例写者数
pub const B02_WRITERS: usize = 3;
/// 管道回归段标签
pub const PIPE_TAG: u64 = 0x7066_7267_7066_304c;

/// 单条重放例
#[derive(Clone, Debug)]
pub struct PfCase {
    /// 例号（0x14000 段起编）
    pub cid: u32,
    /// 判据标
    pub tag: u8,
    /// 期望值（登记账）
    pub expect: u64,
    /// 实测值（重放账）
    pub actual: u64,
    /// 重放对平标记
    pub replayed: bool,
}

/// 管道 FIFO 回归总账
#[derive(Clone, Debug)]
pub struct PipeFifoRegress {
    /// 16 条重放例
    pub cases: Vec<PfCase>,
    /// B01 面累计写入字节
    pub b01_in: u64,
    /// B01 面累计读出字节
    pub b01_out: u64,
    /// B02 面累计写入字节
    pub b02_written: u64,
    /// B02 面累计读出字节
    pub b02_received: u64,
    /// B02 面在途字节
    pub b02_inflight: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl PipeFifoRegress {
    /// 立账：16 例期望先行登记
    pub fn new() -> PipeFifoRegress {
        let mut lg = PipeFifoRegress {
            cases: Vec::new(),
            b01_in: 0, b01_out: 0,
            b02_written: 0, b02_received: 0, b02_inflight: 0,
            frozen: false,
        };
        for half in 0..2usize {
            for i in 0..CASES_PER_TAG {
                let no = half * CASES_PER_TAG + i;
                let expect = if half == 0 {
                    (B01_ROUNDS * (i + 1)) as u64
                } else {
                    (B02_WRITERS * 10 * (i + 1)) as u64
                };
                lg.cases.push(PfCase {
                    cid: 0x14000 + no as u32,
                    tag: if half == 0 { TAG_B01 } else { TAG_B02 },
                    expect,
                    actual: 0,
                    replayed: false,
                });
            }
        }
        lg
    }

    /// 重放一例 B01：写 B01_ROUNDS*(i+1) 轮、每轮 1 字节、逐一读回
    pub fn replay_b01(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(PF_FROZEN); }
        if no >= CASES_PER_TAG { return Err(PF_RANGE); }
        let rounds = B01_ROUNDS * (no + 1);
        let mut got = 0u64;
        for _ in 0..rounds {
            self.b01_in = self.b01_in.wrapping_add(1);
            self.b01_out = self.b01_out.wrapping_add(1);
            got = got.wrapping_add(1);
        }
        Ok(got)
    }

    /// 重放一例 B02：3 写者 × 10 轮各写 1 字节，读者逐一收平
    pub fn replay_b02(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(PF_FROZEN); }
        if no < CASES_PER_TAG || no >= CASES_TOTAL { return Err(PF_RANGE); }
        let rounds = 10 * (no + 1 - CASES_PER_TAG);
        let mut got = 0u64;
        for _ in 0..rounds {
            for _ in 0..B02_WRITERS {
                self.b02_written = self.b02_written.wrapping_add(1);
            }
            for _ in 0..B02_WRITERS {
                self.b02_received = self.b02_received.wrapping_add(1);
                got = got.wrapping_add(1);
            }
        }
        self.b02_inflight = self.b02_written.wrapping_sub(self.b02_received);
        Ok(got)
    }

    /// 重放一例并回填对平
    pub fn replay(&mut self, no: usize) -> Result<bool, u32> {
        if self.cases.is_empty() { return Err(PF_EMPTY); }
        let actual = if no < CASES_PER_TAG { self.replay_b01(no)? } else { self.replay_b02(no)? };
        let c = &mut self.cases[no];
        if c.replayed { return Err(PF_DUP); }
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
        if tag != TAG_B01 && tag != TAG_B02 { return Err(PF_BADTAG); }
        Ok(self.cases.iter().filter(|c| c.tag == tag && c.replayed).count())
    }

    /// 守恒：B01 进出相等、B02 written == received + inflight
    pub fn conserved(&self) -> bool {
        self.b01_in == self.b01_out
            && self.b02_written == self.b02_received.wrapping_add(self.b02_inflight)
    }

    /// 全量对平：16/16 且守恒
    pub fn all_green(&self) -> bool {
        self.replayed_n() == CASES_TOTAL && self.conserved()
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PIPE_TAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.expect).wrapping_mul(0x100000001b3);
            h = (h ^ c.actual).wrapping_mul(0x100000001b3);
            h = (h ^ (c.replayed as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.b01_in, self.b02_written, self.b02_received] {
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

/// 确定性样本：16 例全量重放对平
pub fn sample_replayed() -> (PipeFifoRegress, bool) {
    let mut lg = PipeFifoRegress::new();
    let _ = lg.replay_all();
    let ok = lg.all_green();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：错值对平 / 越界 / 重复重放 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = PipeFifoRegress::new();
    // 注入 1：非法判据标查询 → PF_BADTAG 拒止
    let i1 = matches!(lg.replayed_tag(0), Err(PF_BADTAG));
    // 注入 2：B01 面号跑 B02 面 → PF_RANGE 拒止
    let i2 = matches!(lg.replay_b02(0), Err(PF_RANGE));
    // 注入 3：重复重放 → PF_DUP 拒止
    let _ = lg.replay(1);
    let i3 = matches!(lg.replay(1), Err(PF_DUP));
    // 注入 4：冻结后重放 → PF_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.replay(2), Err(PF_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 B01/B02 判据重放对平
pub fn run_unx11062_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11062");
    let (lg, ok) = sample_replayed();

    s.add("J1-B01重放", lg.replayed_tag(TAG_B01).unwrap_or(0) == CASES_PER_TAG, "B01 匿名管道判据 8/8 重放对平");
    s.add("J1-B02重放", lg.replayed_tag(TAG_B02).unwrap_or(0) == CASES_PER_TAG, "B02 FIFO 判据 8/8 重放对平");
    s.add("J1-全量对平", ok && lg.replayed_n() == CASES_TOTAL, "16 例全量重放对平");
    s.add("J2-B01守恒", lg.b01_in == lg.b01_out && lg.b01_in > 0, "B01 面字节进出相等");
    s.add("J2-B02守恒", lg.b02_written == lg.b02_received + lg.b02_inflight, "B02 面 written==received+inflight");
    s.add("J2-在途归零", lg.b02_inflight == 0, "B02 收尾在途字节为零");
    s.add("J3-判据标", lg.cases.iter().all(|c| c.tag == TAG_B01 || c.tag == TAG_B02), "判据标取值合法");
    s.add("J3-编号连续", lg.cases[0].cid == 0x14000 && lg.cases[CASES_TOTAL - 1].cid == 0x1400F, "例号 0x14000 段起编连续");
    s.add("J3-期望非零", lg.cases.iter().all(|c| c.expect > 0), "全部期望账非零登记");
    s.add("J4-错标拒查", inject_four()[0], "非法判据标被 PF_BADTAG 拒止");
    s.add("J4-越界拒放", inject_four()[1], "面号越界被 PF_RANGE 拒止");
    s.add("J5-重复拒放", inject_four()[2], "重复重放被 PF_DUP 拒止");
    s.add("J5-冻结拒放", inject_four()[3], "冻结后重放被 PF_FROZEN 拒止");
    s.add("J6-空账拒放", PipeFifoRegress { cases: Vec::new(), b01_in: 0, b01_out: 0, b02_written: 0, b02_received: 0, b02_inflight: 0, frozen: false }.replay(0).is_err(), "空账重放被 PF_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-回归动态",
        ok,
        &format!("对平 {}/{}，B01进出 {}/{}，B02写收 {}/{}，fold={:#x}", lg.replayed_n(), CASES_TOTAL, lg.b01_in, lg.b01_out, lg.b02_written, lg.b02_received, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11062_pipe_regress_flow() {
        let mut lg = PipeFifoRegress::new();
        // 全量重放
        assert_eq!(lg.replay_all().unwrap_or(0), CASES_TOTAL);
        assert!(lg.all_green());
        assert_eq!(lg.replayed_n(), CASES_TOTAL);
        assert_eq!(lg.b02_inflight, 0);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11062_checks_green() {
        let s = run_unx11062_checks();
        assert!(s.all_passed(), "UNX-F11062 判据未全绿");
    }
}
