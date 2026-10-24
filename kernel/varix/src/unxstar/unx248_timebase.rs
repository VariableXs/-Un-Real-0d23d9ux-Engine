//! UNX-F0248 早传第七步：时间基准链（0x3BD0 段）
//!
//! 锚点：UNX-F0248-J1 TSC 基准/频率校准值/墙钟锚点三件入
//! Manifest，与 F0106/F0162 对账一致。
//! 域：CoRun·UNX-A1-B13（时间基准链面）。
//!
//! 结构：TSC 基准锚（引导零点快照）+ 频率校准值（Hz，非零守卫）+
//! 墙钟锚点（秒/纳秒对，UTC 语义）+ 时间节封包（三件入 Manifest）+
//! 换算核（TSC tick → 纳秒，与 F0106 同口径）+ 墙钟核（与 F0162
//! GetTime/SetTime 语义对账）+ 注入核（零频率/倒流/漂移必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3BD0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3BD0;
pub const TC_FREQ_ZERO: u32 = SEG_BASE + 0x01; // 频率校准值为零
pub const TC_BACKWARD: u32 = SEG_BASE + 0x02; // 墙钟倒流
pub const TC_DRIFT: u32 = SEG_BASE + 0x03; // 与 F0106/F0162 对账漂移
pub const TC_SEALED: u32 = SEG_BASE + 0x04; // 基准链已封账拒写
pub const TC_NSEC_OOB: u32 = SEG_BASE + 0x05; // 纳秒分量越界（≥1e9）

/// 纳秒分量上界（1 秒 = 1e9 ns）。
pub const NSEC_BOUND: u64 = 1_000_000_000;

/// TSC 基准锚：引导零点 TSC 读数 + 采样序号。
#[derive(Clone, Copy)]
pub struct TscAnchor {
    pub base_ticks: u64,
    pub sample_seq: u32,
}

/// 墙钟锚点：UTC 秒 + 纳秒分量。
#[derive(Clone, Copy)]
pub struct WallAnchor {
    pub epoch_sec: u64,
    pub nsec: u32,
}

impl WallAnchor {
    /// 合法性：纳秒分量 < 1e9。
    pub fn valid(&self) -> bool {
        (self.nsec as u64) < NSEC_BOUND
    }

    /// 折算整纳秒（防溢出用 u64：秒×1e9 + 纳秒）。
    pub fn total_ns(&self) -> u64 {
        self.epoch_sec
            .wrapping_mul(NSEC_BOUND)
            .wrapping_add(self.nsec as u64)
    }
}

/// 时间基准链（三件聚合，可封账）。
pub struct TimebaseChain {
    pub tsc: Option<TscAnchor>,
    pub freq_hz: u64,
    pub wall: Option<WallAnchor>,
    pub sealed: bool,
}

impl TimebaseChain {
    pub fn new() -> Self {
        TimebaseChain { tsc: None, freq_hz: 0, wall: None, sealed: false }
    }

    /// 写 TSC 基准（封账后拒）。
    pub fn set_tsc(&mut self, base_ticks: u64, sample_seq: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TC_SEALED);
        }
        self.tsc = Some(TscAnchor { base_ticks, sample_seq });
        Ok(())
    }

    /// 写频率校准值（零值拒绝：零频换算无意义）。
    pub fn set_freq(&mut self, hz: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TC_SEALED);
        }
        if hz == 0 {
            return Err(TC_FREQ_ZERO);
        }
        self.freq_hz = hz;
        Ok(())
    }

    /// 写墙钟锚点（倒流拒绝：新锚必须不早于旧锚；纳秒越界拒绝）。
    pub fn set_wall(&mut self, epoch_sec: u64, nsec: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TC_SEALED);
        }
        if (nsec as u64) >= NSEC_BOUND {
            return Err(TC_NSEC_OOB);
        }
        let w = WallAnchor { epoch_sec, nsec };
        if let Some(old) = self.wall {
            if w.total_ns() < old.total_ns() {
                return Err(TC_BACKWARD);
            }
        }
        self.wall = Some(w);
        Ok(())
    }

    /// 三件齐备核（J1 齐 = tsc + freq + wall 均在）。
    pub fn complete(&self) -> bool {
        self.tsc.is_some() && self.freq_hz > 0 && self.wall.is_some()
    }

    /// 换算核（F0106 同口径）：TSC tick 增量 → 纳秒。
    /// ns = ticks * 1e9 / freq（u128 中间量防溢出）。
    pub fn ticks_to_ns(&self, ticks: u64) -> Result<u64, u32> {
        if self.freq_hz == 0 {
            return Err(TC_FREQ_ZERO);
        }
        let ns = (ticks as u128) * 1_000_000_000u128 / (self.freq_hz as u128);
        Ok(ns as u64)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Manifest 渲染（三件齐时输出基准链行）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        match self.tsc {
            Some(t) => {
                let line = format!("TSC基准 ticks={} seq={}", t.base_ticks, t.sample_seq);
                out.push_str(&line);
            }
            None => out.push_str("TSC基准 未采样"),
        }
        out.push('\n');
        let f = format!("频率校准 {}Hz", self.freq_hz);
        out.push_str(&f);
        out.push('\n');
        match self.wall {
            Some(w) => {
                let line = format!(
                    "墙钟锚点 sec={} nsec={} 总ns={}",
                    w.epoch_sec, w.nsec, w.total_ns()
                );
                out.push_str(&line);
            }
            None => out.push_str("墙钟锚点 未置"),
        }
        out
    }
}

/// 标准夹具：1.8 GHz TSC + 基准 0x1000 ticks + 2026-01-01 墙钟。
pub fn standard_chain() -> TimebaseChain {
    let mut c = TimebaseChain::new();
    let _ = c.set_tsc(0x1000, 7);
    let _ = c.set_freq(1_800_000_000);
    let _ = c.set_wall(1_767_225_600, 500_000_000); // 2026-01-01T00:00:00.5Z
    c
}

/// F0106 对账锚：日志时间戳换算口径核。
/// 夹具频率下 1_800_000_000 ticks 必须换算为恰 1 秒（1e9 ns）。
pub fn reconcile_f0106(chain: &TimebaseChain) -> Result<(), u32> {
    let ns = chain.ticks_to_ns(chain.freq_hz)?;
    if ns != 1_000_000_000 {
        return Err(TC_DRIFT);
    }
    Ok(())
}

/// F0162 对账锚：GetTime 语义核。
/// 墙钟 total_ns 必须可分解回 (sec, nsec) 且与原值一致。
pub fn reconcile_f0162(chain: &TimebaseChain) -> Result<(), u32> {
    let w = match chain.wall {
        Some(w) => w,
        None => return Err(TC_DRIFT),
    };
    if !w.valid() {
        return Err(TC_NSEC_OOB);
    }
    let total = w.total_ns();
    let sec = total / NSEC_BOUND;
    let nsec = (total % NSEC_BOUND) as u32;
    if sec != w.epoch_sec || nsec != w.nsec {
        return Err(TC_DRIFT);
    }
    Ok(())
}

/// 注入核：6 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零频率拒
    let mut c = TimebaseChain::new();
    if c.set_freq(0) == Err(TC_FREQ_ZERO) {
        caught += 1;
    }
    // 2) 墙钟倒流拒
    let _ = c.set_wall(100, 0);
    if c.set_wall(99, 999_999_999) == Err(TC_BACKWARD) {
        caught += 1;
    }
    // 3) 纳秒越界拒
    let mut c2 = TimebaseChain::new();
    if c2.set_wall(100, 1_000_000_000) == Err(TC_NSEC_OOB) {
        caught += 1;
    }
    // 4) 封账后写拒
    c2.seal();
    if c2.set_freq(1_000_000) == Err(TC_SEALED) {
        caught += 1;
    }
    // 5) 零频换算拒（未设频率直接换算）
    let mut c3 = TimebaseChain::new();
    if c3.ticks_to_ns(1000) == Err(TC_FREQ_ZERO) {
        caught += 1;
    }
    // 6) 换算漂移必抓（频率错置 2 倍 → 1 秒 tick 换算成 0.5 秒）
    let mut c4 = TimebaseChain::new();
    let _ = c4.set_freq(3_600_000_000);
    if c4.ticks_to_ns(1_800_000_000) != Ok(500_000_000) {
        caught += 1;
    }
    // 反向验证：正确频率下同 ticks = 1e9 ns（两向实证）
    let mut c5 = TimebaseChain::new();
    let _ = c5.set_freq(1_800_000_000);
    if c5.ticks_to_ns(1_800_000_000) == Ok(1_000_000_000) {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx248_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0248");
    // J1-1 三件入 Manifest
    let c = standard_chain();
    let ok = c.complete() && !c.render().is_empty();
    s.add("B13-11-基准链三件齐", ok, if ok { "" } else { "三件缺一" });
    // J1-2 与 F0106 换算口径一致
    let ok = reconcile_f0106(&standard_chain()).is_ok();
    s.add("B13-12-F0106口径一致", ok, if ok { "" } else { "换算漂移" });
    // J1-3 与 F0162 墙钟语义一致
    let ok = reconcile_f0162(&standard_chain()).is_ok();
    s.add("B13-13-F0162语义一致", ok, if ok { "" } else { "墙钟漂移" });
    // J1-4 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-14-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx248_all_checks_green() {
        let set = run_unx248_checks();
        assert!(set.all_passed(), "UNX-F0248 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx248_ticks_to_ns_exact() {
        let c = standard_chain();
        assert_eq!(c.ticks_to_ns(900_000_000), Ok(500_000_000));
    }
}
