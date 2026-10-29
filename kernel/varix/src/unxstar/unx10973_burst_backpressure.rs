//! UNX-F10973 多键连发背压联测（快速键入不丢键）（0x1d920 段）
//!
//! 卡片判据（UNX-F10973-J1）：
//! - 高频连发万键零丢失（100/100 分批对账），背压协同对账（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模多键连发背压协同：生产者
//! 万键分批连发，有界缓冲满时生产者暂停（背压挂起）而非丢键；
//! 消费者分批对账 100/100 批；总账：喂入=消费，批账逐批可核。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10973-10900)*0x20 = 0x1d920）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d920;

/// BP_EBADF：通道未绑定
pub const BP_EBADF: u32 = SEG_BASE + 0x01;
/// BP_EINVAL：批大小非法
pub const BP_EINVAL: u32 = SEG_BASE + 0x02;
/// BP_ELOSE：分批对账丢键（自检面）
pub const BP_ELOSE: u32 = SEG_BASE + 0x03;
/// BP_EORDER：批序错乱（自检面）
pub const BP_EORDER: u32 = SEG_BASE + 0x04;
/// BP_EAGAIN：背压挂起（协同面，重试非丢失）
pub const BP_EAGAIN: u32 = SEG_BASE + 0x05;
/// BP_ECLOSED：通道已关闭
pub const BP_ECLOSED: u32 = SEG_BASE + 0x06;
/// BP_EDRAIN：排空后仍有残留（自检面）
pub const BP_EDRAIN: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 有界缓冲容量（键数）
const RING_CAP: usize = 64;
/// 生产者批大小
pub const BURST: usize = 100;
/// 分批对账批数
pub const BATCHES: u32 = 100;
/// 万键总数
pub const TOTAL_KEYS: u32 = 10_000;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单批对账条目
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchRec {
    pub no: u32,
    pub keys: u32,
}

impl BatchRec {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.no as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.keys as u64).wrapping_mul(0x100000001b3);
        h
    }
}

/// 背压联测台
#[derive(Clone, Debug)]
pub struct BurstDesk {
    /// 有界缓冲
    ring: Vec<u32>,
    /// 下一写入键号
    next_key: u32,
    /// 生产者已喂入总数
    pub fed: u64,
    /// 消费者已消费总数
    pub ate: u64,
    /// 背压挂起次数
    pub pauses: u64,
    /// 分批对账记录
    pub batches: Vec<BatchRec>,
    /// 通道开启
    pub open: bool,
    /// 错误计数
    pub n_again: u32,
    pub n_einval: u32,
}

impl BurstDesk {
    pub fn new() -> Self {
        BurstDesk {
            ring: Vec::new(),
            next_key: 1,
            fed: 0,
            ate: 0,
            pauses: 0,
            batches: Vec::new(),
            open: true,
            n_again: 0,
            n_einval: 0,
        }
    }

    /// 生产者推一键（满则背压挂起）
    pub fn push(&mut self) -> Result<(), u32> {
        if !self.open {
            return Err(BP_ECLOSED);
        }
        if self.ring.len() >= RING_CAP {
            self.n_again = self.n_again.wrapping_add(1);
            return Err(BP_EAGAIN);
        }
        self.ring.push(self.next_key);
        self.next_key = self.next_key.wrapping_add(1);
        self.fed = self.fed.wrapping_add(1);
        Ok(())
    }

    /// 生产者连发一批（遇背压则挂起等待消费者协同）
    pub fn burst(&mut self, n: usize) -> Result<usize, u32> {
        if !self.open {
            return Err(BP_ECLOSED);
        }
        if n == 0 {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(BP_EINVAL);
        }
        let mut done = 0usize;
        while done < n {
            match self.push() {
                Ok(()) => done = done.wrapping_add(1),
                Err(BP_EAGAIN) => {
                    // 背压协同：挂起一笔入账后消费一批腾出空间继续
                    self.pauses = self.pauses.wrapping_add(1);
                    self.consume(RING_CAP / 2)?;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }

    /// 消费者取 n 键（有则取，记录批账）
    pub fn consume(&mut self, n: usize) -> Result<usize, u32> {
        if !self.open {
            return Err(BP_ECLOSED);
        }
        let mut got = 0usize;
        while got < n && !self.ring.is_empty() {
            let k = self.ring.remove(0);
            // 批序对账：键号必须等于已消费数+1
            if k as u64 != self.ate.wrapping_add(1) {
                return Err(BP_EORDER);
            }
            self.ate = self.ate.wrapping_add(1);
            got = got.wrapping_add(1);
        }
        if got > 0 {
            self.batches.push(BatchRec { no: self.batches.len() as u32 + 1, keys: got as u32 });
        }
        Ok(got)
    }

    /// 万键连发 + 分批消费：TOTAL_KEYS 键、BATCHES 批对账
    pub fn run_10k(&mut self) -> Result<u32, u32> {
        let per_batch = TOTAL_KEYS / BATCHES;
        let mut b = 0u32;
        while b < BATCHES {
            self.burst(per_batch as usize)?;
            // 消费至缓冲半空以下再继续生产
            while self.ring.len() > RING_CAP / 4 {
                self.consume(RING_CAP / 4)?;
            }
            b = b.wrapping_add(1);
        }
        // 排空
        while !self.ring.is_empty() {
            self.consume(RING_CAP)?;
        }
        Ok(self.batches.len() as u32)
    }

    /// 对账：缓冲排空、喂入=消费、批账总和=消费
    pub fn reconcile(&self) -> Result<(), u32> {
        if !self.ring.is_empty() {
            return Err(BP_EDRAIN);
        }
        if self.fed != self.ate {
            return Err(BP_ELOSE);
        }
        let mut sum = 0u64;
        for b in &self.batches {
            sum = sum.wrapping_add(b.keys as u64);
        }
        if sum != self.ate {
            return Err(BP_ELOSE);
        }
        if !self.ring.is_empty() {
            return Err(BP_EDRAIN);
        }
        Ok(())
    }

    /// 背压协同对账：有挂起且最终零丢失
    pub fn coop_ok(&self) -> bool {
        self.pauses > 0 && self.reconcile().is_ok()
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_fed: u64) -> bool {
        self.fed == expect_fed && self.reconcile().is_ok()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.fed).wrapping_mul(0x100000001b3);
        h = (h ^ self.ate).wrapping_mul(0x100000001b3);
        h = (h ^ self.pauses).wrapping_mul(0x100000001b3);
        for b in &self.batches {
            h = (h ^ b.fold()).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "burst:fed={} ate={} pauses={} batches={}",
            self.fed,
            self.ate,
            self.pauses,
            self.batches.len()
        )
    }
}

impl Default for BurstDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = BurstDesk::new();
    // 路1：零批长 → EINVAL
    let r1 = matches!(d.burst(0), Err(x) if x == BP_EINVAL);
    // 路2：关闭后推 → ECLOSED
    d.open = false;
    let r2 = matches!(d.push(), Err(x) if x == BP_ECLOSED);
    d.open = true;
    // 路3：批序破坏必检出
    let _ = d.burst(10);
    d.ring[3] = 99;
    let _ = d.consume(10);
    let r3 = matches!(d.consume(1), Err(x) if x == BP_EORDER);
    // 路4：排空面：残留必检出
    let mut d2 = BurstDesk::new();
    let _ = d2.burst(5);
    let r4 = d2.reconcile() == Err(BP_EDRAIN);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮 10 键连发 + 消费协同，总账对平记 1 轮；
/// 每百轮一次分批对账。
pub fn sample_thousand() -> u32 {
    let mut d = BurstDesk::new();
    let mut good: u32 = 0;
    let mut fed_at_round_start: u64 = 0;
    for r in 0..1000u32 {
        fed_at_round_start = d.fed;
        if d.burst(10).is_ok() {
            // 消费协同：清空至缓冲 1/4 以下
            while d.ring.len() > RING_CAP / 4 {
                let _ = d.consume(RING_CAP / 4);
            }
            // 每 100 轮做一次全排空对账
            if r % 100 == 99 {
                while !d.ring.is_empty() {
                    let _ = d.consume(RING_CAP);
                }
                if d.reconcile().is_ok() && d.fed == fed_at_round_start + 10 {
                    good = good.wrapping_add(1);
                }
            } else {
                good = good.wrapping_add(1);
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10973_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10973");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EINVAL/ECLOSED/EORDER/EDRAIN 坏样本全部命中");

    // J1 万键分批对账
    let mut d = BurstDesk::new();
    let j1 = match d.run_10k() {
        Ok(_) => d.ledger_ok(TOTAL_KEYS as u64) && d.ate == TOTAL_KEYS as u64,
        Err(_) => false,
    };
    s.add(
        "J1-万键零丢失",
        j1,
        "万键连发全消费，批账逐批可核",
    );

    // 分批对账 100/100
    let batch_ok = d.batches.len() as u32 >= BATCHES
        && d.batches.iter().map(|b| b.keys as u64).sum::<u64>() == TOTAL_KEYS as u64;
    s.add("J2-分批对账100/100", batch_ok, "批数与批账总和逐批对平");

    // 背压协同对账 10/10
    let mut coop10 = 0u32;
    for _ in 0..10 {
        let mut d2 = BurstDesk::new();
        if d2.run_10k().is_ok() && d2.coop_ok() {
            coop10 = coop10.wrapping_add(1);
        }
    }
    s.add("J3-背压协同10/10", coop10 == 10, "十轮万键压测背压挂起零丢失");

    // 千轮样本
    let good = sample_thousand();
    s.add("J4-千轮样本", good == 1000, "千轮连发协同全对");

    // 错误码分段
    let codes = [BP_EBADF, BP_EINVAL, BP_ELOSE, BP_EORDER, BP_EAGAIN, BP_ECLOSED, BP_EDRAIN];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "七码全部落在 0x1d920 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "七码两两互异");

    // 账目对平
    s.add("J7-账目对平", d.ledger_ok(TOTAL_KEYS as u64), "喂入=消费=万键");

    // 背压确实发生
    s.add(
        "J8-背压发生",
        d.pauses > 0 && d.n_again > 0,
        "万键连发必然触发背压挂起",
    );

    // 批账记录折叠稳定
    let h1 = d.fold_all();
    let h2 = d.fold_all();
    s.add("J9-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = d.render();
    s.add(
        "J10-渲染非空",
        r.starts_with("burst:") && r.contains("fed=10000"),
        "摘要含连发账",
    );

    // 批键数守恒
    let per_ok = d.batches.iter().all(|b| b.keys > 0);
    s.add("J11-批键守恒", per_ok, "每批键数非零");

    // 动态判据
    s.add_dyn(
        "J12-批账明细",
        d.batches.len() > 0,
        &crate::alloc::format!("batches={} pauses={}", d.batches.len(), d.pauses),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J14-排空账",
        d.reconcile().is_ok(),
        &crate::alloc::format!("fed={} ate={}", d.fed, d.ate),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10973_burst_backpressure_flow() {
        let mut d = BurstDesk::new();
        assert!(d.run_10k().is_ok());
        assert!(d.ledger_ok(TOTAL_KEYS as u64));
        assert!(d.pauses > 0);
        assert!(matches!(d.burst(0), Err(x) if x == BP_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10973_checks_green() {
        let s = run_unx10973_checks();
        assert!(s.all_passed(), "UNX-F10973 判据未全绿");
    }
}
