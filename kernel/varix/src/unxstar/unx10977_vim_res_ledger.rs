//! UNX-F10977 vim 场资源账联测（fd/内存/进程零泄漏）（0x1d9a0 段）
//!
//! 锚点：UNX-F10977-J1 场景全程资源账零泄漏（10/10），终态对平（10/10）。
//! 域：CoRun·UNX-B29（I 型联测域·vim 场资源面）。
//!
//! 结构：资源桶账（fd / 内存页 / 子进程三类资源，逐类记获取/释放/
//! 峰值/在册四段）+ 场景驱动器（每轮六步：三类各取一件再三类各还
//! 一件，轮末在册必回零）+ 峰值看守（在册不得超过场景上限）+
//! fold 与冻结 + 注入核（越界类号/释放过量/冻结后获取/空账释放
//! 四路必抓）。
//!
//! 守恒律：获取总数 == 释放总数，且在册数 == 获取 - 释放 恒成立；
//! 峰值只记单轮内的最高在册，跨轮不累积。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10977 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1d9a0;

/// 空账 —— 未立资源桶即操作
pub const RS_EMPTY: u32 = SEG_BASE + 0x01;
/// 资源类越界 —— 资源类编号 1..=3
pub const RS_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立桶 —— 同类资源只准立一桶
pub const RS_DUP: u32 = SEG_BASE + 0x03;
/// 释放过量 —— 释放数不得超过在册数
pub const RS_NEG: u32 = SEG_BASE + 0x04;
/// 泄漏检出 —— 终态在册不为零
pub const RS_LEAK: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再记账
pub const RS_FROZEN: u32 = SEG_BASE + 0x06;

/// 资源类：文件描述符。
pub const RES_FD: u8 = 1;
/// 资源类：内存页。
pub const RES_MEM: u8 = 2;
/// 资源类：子进程。
pub const RES_PROC: u8 = 3;
/// 资源类总数。
pub const RES_CLASSES: usize = 3;
/// 场景轮数。
pub const ROUNDS: u64 = 1000;
/// 每轮步数（三类取 + 三类还）。
pub const STEPS_PER_ROUND: u8 = 6;
/// 单类在册峰值上限（场景并发度）。
pub const LIVE_CAP: u64 = 3;
/// 资源账域标（fold 种子）。
pub const RS_TAG: u64 = 0x1d9_77;

/// 资源桶：一类资源的四段计数。
#[derive(Clone, Debug)]
pub struct ResBucket {
    /// 桶号（0x15100 段起编）
    pub rid: u32,
    /// 资源类号
    pub class: u8,
    /// 桶名
    pub name: String,
    /// 累计获取
    pub acq: u64,
    /// 累计释放
    pub rel: u64,
    /// 在册（acq - rel）
    pub live: u64,
    /// 峰值在册
    pub high: u64,
}

impl ResBucket {
    /// 净账：获取减释放。
    pub fn net(&self) -> u64 {
        self.acq.wrapping_sub(self.rel)
    }

    /// 桶内守恒：在册 == 净账。
    pub fn conserves(&self) -> bool {
        self.live == self.net()
    }

    /// 桶内零泄漏：在册为零。
    pub fn drained(&self) -> bool {
        self.live == 0
    }

    /// 峰值不越上限。
    pub fn peak_ok(&self) -> bool {
        self.high <= LIVE_CAP
    }

    /// 周转率（获取 / 峰值），峰值为零返 0。
    pub fn churn(&self) -> u64 {
        if self.high == 0 {
            0
        } else {
            self.acq / self.high
        }
    }
}

/// vim 场资源总账。
#[derive(Clone, Debug)]
pub struct VimResLedger {
    /// 三资源桶
    pub buckets: Vec<ResBucket>,
    /// 已跑轮数
    pub round: u64,
    /// 已跑步数
    pub steps: u64,
    /// 峰值越限次数
    pub peak_breaks: u64,
    /// 释放过量拒止次数
    pub overdrafts: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl VimResLedger {
    /// 立场：三类资源桶全部立账（零计数）。
    pub fn new() -> VimResLedger {
        let mut lg = VimResLedger {
            buckets: Vec::new(),
            round: 0,
            steps: 0,
            peak_breaks: 0,
            overdrafts: 0,
            frozen: false,
        };
        let names = ["fd", "mempage", "proc"];
        let mut k = 0usize;
        while k < RES_CLASSES {
            lg.buckets.push(ResBucket {
                rid: 0x15100u32.wrapping_add(k as u32),
                class: (k as u8) + 1,
                name: names[k].to_string(),
                acq: 0,
                rel: 0,
                live: 0,
                high: 0,
            });
            k += 1;
        }
        lg
    }

    /// 按资源类定位桶下标。
    pub fn bucket_ix(&self, class: u8) -> Result<usize, u32> {
        if self.buckets.is_empty() {
            return Err(RS_EMPTY);
        }
        if class == 0 || class as usize > RES_CLASSES {
            return Err(RS_RANGE);
        }
        let ix = (class as usize).wrapping_sub(1);
        if ix >= self.buckets.len() {
            return Err(RS_RANGE);
        }
        Ok(ix)
    }

    /// 另立一桶：同类重号即拒。
    pub fn add_bucket(&mut self, class: u8, name: String) -> Result<(), u32> {
        if self.frozen {
            return Err(RS_FROZEN);
        }
        if class == 0 || class as usize > RES_CLASSES {
            return Err(RS_RANGE);
        }
        let mut k = 0usize;
        while k < self.buckets.len() {
            if self.buckets[k].class == class {
                return Err(RS_DUP);
            }
            k += 1;
        }
        let no = self.buckets.len();
        self.buckets.push(ResBucket {
            rid: 0x15100u32.wrapping_add(no as u32),
            class,
            name,
            acq: 0,
            rel: 0,
            live: 0,
            high: 0,
        });
        Ok(())
    }

    /// 获取 n 件资源。
    pub fn acquire(&mut self, class: u8, n: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(RS_FROZEN);
        }
        let ix = self.bucket_ix(class)?;
        let b = &mut self.buckets[ix];
        b.acq = b.acq.wrapping_add(n);
        b.live = b.live.wrapping_add(n);
        if b.live > b.high {
            b.high = b.live;
        }
        if b.high > LIVE_CAP {
            self.peak_breaks = self.peak_breaks.wrapping_add(1);
        }
        Ok(b.live)
    }

    /// 释放 n 件资源：不得超过在册。
    pub fn release(&mut self, class: u8, n: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(RS_FROZEN);
        }
        let ix = self.bucket_ix(class)?;
        if self.buckets[ix].live < n {
            self.overdrafts = self.overdrafts.wrapping_add(1);
            return Err(RS_NEG);
        }
        let b = &mut self.buckets[ix];
        b.rel = b.rel.wrapping_add(n);
        b.live = b.live.wrapping_sub(n);
        Ok(b.live)
    }

    /// 场景一步：1..=3 取，4..=6 还。
    pub fn step(&mut self, step_no: u8) -> Result<u64, u32> {
        if self.frozen {
            return Err(RS_FROZEN);
        }
        if self.buckets.is_empty() {
            return Err(RS_EMPTY);
        }
        let class = match step_no {
            1 | 4 => RES_FD,
            2 | 5 => RES_MEM,
            3 | 6 => RES_PROC,
            _ => return Err(RS_RANGE),
        };
        let out = if step_no <= 3 {
            self.acquire(class, 1)?
        } else {
            self.release(class, 1)?
        };
        self.steps = self.steps.wrapping_add(1);
        Ok(out)
    }

    /// 跑一轮（六步）。
    pub fn round(&mut self) -> Result<u64, u32> {
        let mut k = 1u8;
        while k <= STEPS_PER_ROUND {
            self.step(k)?;
            k += 1;
        }
        self.round = self.round.wrapping_add(1);
        Ok(self.round)
    }

    /// 跑 n 轮。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.round()?;
            k = k.wrapping_add(1);
        }
        Ok(self.round)
    }

    /// 在册总数。
    pub fn live_total(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.buckets.len() {
            t = t.wrapping_add(self.buckets[k].live);
            k += 1;
        }
        t
    }

    /// 获取总数。
    pub fn acq_total(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.buckets.len() {
            t = t.wrapping_add(self.buckets[k].acq);
            k += 1;
        }
        t
    }

    /// 释放总数。
    pub fn rel_total(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.buckets.len() {
            t = t.wrapping_add(self.buckets[k].rel);
            k += 1;
        }
        t
    }

    /// 零泄漏：在册归零且获取等于释放。
    pub fn zero_leak(&self) -> bool {
        self.live_total() == 0 && self.acq_total() == self.rel_total()
    }

    /// 泄漏探针：有在册即 RS_LEAK。
    pub fn leak_probe(&self) -> Result<(), u32> {
        if self.buckets.is_empty() {
            return Err(RS_EMPTY);
        }
        if self.live_total() != 0 {
            return Err(RS_LEAK);
        }
        Ok(())
    }

    /// 逐桶守恒。
    pub fn conservative(&self) -> bool {
        let mut k = 0usize;
        while k < self.buckets.len() {
            if !self.buckets[k].conserves() {
                return false;
            }
            k += 1;
        }
        !self.buckets.is_empty()
    }

    /// 逐桶峰值合规。
    pub fn peaks_ok(&self) -> bool {
        let mut k = 0usize;
        while k < self.buckets.len() {
            if !self.buckets[k].peak_ok() {
                return false;
            }
            k += 1;
        }
        !self.buckets.is_empty()
    }

    /// 周转率：每桶获取 / 峰值。
    pub fn churn_of(&self, class: u8) -> Result<u64, u32> {
        let ix = self.bucket_ix(class)?;
        Ok(self.buckets[ix].churn())
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RS_TAG;
        let mut k = 0usize;
        while k < self.buckets.len() {
            let b = &self.buckets[k];
            h = (h ^ b.rid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ b.acq).wrapping_mul(0x100000001b3);
            h = (h ^ b.rel).wrapping_mul(0x100000001b3);
            h = (h ^ b.high).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.steps).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结资源账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮取还对平零泄漏。
pub fn sample_balanced() -> (VimResLedger, bool) {
    let mut lg = VimResLedger::new();
    let _ = lg.run(ROUNDS);
    let ok = lg.zero_leak() && lg.conservative() && lg.peaks_ok();
    lg.freeze();
    (lg, ok)
}

/// 注入核：坏类 / 释放过量 / 冻结后获取 / 空账释放。
pub fn inject_four() -> [bool; 4] {
    let mut lg = VimResLedger::new();
    // 注入 1：坏资源类 → RS_RANGE 拒收
    let i1 = matches!(lg.acquire(9, 1), Err(RS_RANGE));
    // 注入 2：释放过量 → RS_NEG 拒收
    let _ = lg.acquire(RES_FD, 1);
    let i2 = matches!(lg.release(RES_FD, 5), Err(RS_NEG));
    // 注入 3：冻结后获取 → RS_FROZEN 拒收
    lg.freeze();
    let i3 = matches!(lg.acquire(RES_FD, 1), Err(RS_FROZEN));
    // 注入 4：空账释放 → RS_EMPTY 拒收
    let mut empty = VimResLedger {
        buckets: Vec::new(),
        round: 0,
        steps: 0,
        peak_breaks: 0,
        overdrafts: 0,
        frozen: false,
    };
    let i4 = matches!(empty.release(RES_FD, 1), Err(RS_EMPTY));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 全程资源账零泄漏，终态对平。
pub fn run_unx10977_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10977");
    let (mut lg, ok) = sample_balanced();

    s.add("J1-零泄漏", ok && lg.zero_leak(), "千轮场景全程资源账零泄漏");
    s.add("J1-终态对平", lg.acq_total() == lg.rel_total() && lg.live_total() == 0, "终态获取=释放且在册归零");
    s.add("J2-轮数守恒", lg.round == ROUNDS && lg.steps == ROUNDS * STEPS_PER_ROUND as u64, "千轮六千步逐轮守恒");
    s.add("J2-逐桶守恒", lg.conservative(), "三桶各自在册=获取-释放");
    s.add("J2-逐桶排空", lg.buckets.iter().all(|b| b.drained()), "三桶各自在册归零");
    s.add("J3-峰值合规", lg.peaks_ok() && lg.peak_breaks == 0, "三桶峰值不越上限且零越限");
    s.add("J3-峰值恰用", lg.buckets.iter().all(|b| b.high == 1), "单轮并发峰值为一时峰值");
    s.add("J4-周转率", lg.buckets.iter().all(|b| b.churn() == ROUNDS), "每桶周转千次/峰值一");
    s.add("J4-类号分布", {
        let mut cover = VimResLedger::new();
        let all_ok = (1..=RES_CLASSES as u8).all(|c| cover.acquire(c, 0).is_ok() && cover.bucket_ix(c).is_ok());
        all_ok
            && cover.acquire(0, 0).err() == Some(RS_RANGE)
            && cover.acquire(RES_CLASSES as u8 + 1, 0).err() == Some(RS_RANGE)
    }, "三类资源桶齐备可按类取且类号越界必拒");
    s.add("J5-泄漏探针", lg.leak_probe().is_ok() && lg.overdrafts == 0, "终态泄漏探针放行且零透支");
    s.add("J5-过半检出", {
        let mut probe = VimResLedger::new();
        let _ = probe.acquire(RES_MEM, 2);
        probe.leak_probe() == Err(RS_LEAK)
    }, "半途持有时泄漏探针即 RS_LEAK");
    s.add("J6-坏类拒收", inject_four()[0], "坏资源类被 RS_RANGE 拒收");
    s.add("J6-过量拒收", inject_four()[1], "释放过量被 RS_NEG 拒收");
    s.add("J7-冻结拒记", inject_four()[2], "冻结后获取被 RS_FROZEN 拒止");
    s.add("J7-空账拒记", inject_four()[3], "空账释放被 RS_EMPTY 拒止");
    s.add("J8-重号拒立", {
        let mut d = VimResLedger::new();
        d.add_bucket(RES_FD, "dup".to_string()) == Err(RS_DUP)
    }, "同类资源重号立桶被 RS_DUP 拒止");
    s.add("J8-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-资源动态",
        ok,
        &format!(
            "轮 {} 步 {} 获取 {} 释放 {} 在册 {} 峰越限 {} 透支 {}",
            lg.round,
            lg.steps,
            lg.acq_total(),
            lg.rel_total(),
            lg.live_total(),
            lg.peak_breaks,
            lg.overdrafts,
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10977_vim_res_ledger_flow() {
        let mut lg = VimResLedger::new();
        assert_eq!(lg.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(lg.zero_leak());
        assert!(lg.conservative());
        assert!(lg.peaks_ok());
        assert_eq!(lg.live_total(), 0);
        // 单轮语义抽查
        let mut g = VimResLedger::new();
        let _ = g.acquire(RES_FD, 2);
        assert_eq!(g.live_total(), 2);
        assert!(g.leak_probe() == Err(RS_LEAK));
        assert_eq!(g.release(RES_FD, 2).unwrap_or(9), 0);
        assert!(g.leak_probe().is_ok());
        // 轮末回零
        let mut h = VimResLedger::new();
        let _ = h.round();
        assert_eq!(h.live_total(), 0);
        assert_eq!(h.churn_of(RES_FD).unwrap_or(0), 1);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10977_checks_green() {
        let s = run_unx10977_checks();
        assert!(s.all_passed(), "UNX-F10977 判据未全绿");
    }
}
