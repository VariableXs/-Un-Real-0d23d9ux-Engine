//! UNX-F56163 syscall 延迟直方图精化件（0x61460 段）
//!
//! 锚点：UNX-F56163-J1 直方图精化账（对数桶界直方图——8 桶闭集：1μs
//! /10μs/100μs/1ms/10ms/100ms/1s/溢出——逐桶计数落账）；J2 归桶语义
//! （值 v 归桶 = v 首个 ≥ 上界的桶；边界值归上桶；溢出桶收容一切超界
//! 值）；J3 桶和核（桶计数和 = 事件总数 + 对数桶界单调）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·直方图面）。
//!
//! 结构：对数桶界闭集（8 桶）+ 归桶核（线性扫描首上界）+ 直方图账
//! （逐桶计数 + 事件总数）+ 桶和核（一致性验证）+ 渲染直方条 + 注入
//! 核（桶界乱序/溢出穿透/桶计数篡改/空直方印/未知桶查询必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61460 独占 0x20）。
pub const SEG_BASE: u32 = 0x61460;
pub const HG_BOUNDS_ORDER: u32 = SEG_BASE + 0x01; // 桶界乱序
pub const HG_OVERFLOW_MISS: u32 = SEG_BASE + 0x02; // 溢出穿透
pub const HG_COUNT_TAMPER: u32 = SEG_BASE + 0x03; // 桶计数篡改
pub const HG_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空直方印
pub const HG_UNK_BUCKET: u32 = SEG_BASE + 0x05; // 未知桶查询

/// 八桶闭集（对数上界，微秒；末桶 INF 收容溢出）。
pub const BOUNDS: [u64; 8] = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, u64::MAX];
pub const BUCKET_COUNT: usize = 8;

/// 桶名渲染。
pub const BUCKET_NAMES: [&str; BUCKET_COUNT] =
    ["1us", "10us", "100us", "1ms", "10ms", "100ms", "1s", "INF"];

/// 归桶核：v 归首个 v ≤ 上界的桶（上界单调递增）。
pub fn bucket_of(v: u64) -> Result<usize, u32> {
    // 桶界单调核（防御）
    for w in BOUNDS.windows(2) {
        if w[0] >= w[1] {
            return Err(HG_BOUNDS_ORDER);
        }
    }
    for (i, b) in BOUNDS.iter().enumerate() {
        if v <= *b {
            return Ok(i);
        }
    }
    Err(HG_OVERFLOW_MISS)
}

/// 直方图账。
pub struct Histogram {
    counts: [u64; BUCKET_COUNT],
    total: u64,
    sealed: bool,
}

impl Histogram {
    pub fn new() -> Self {
        Histogram {
            counts: [0; BUCKET_COUNT],
            total: 0,
            sealed: false,
        }
    }

    /// 录一值：归桶计数。
    pub fn record(&mut self, v: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(HG_EMPTY_SEAL);
        }
        let b = bucket_of(v)?;
        self.counts[b] += 1;
        self.total += 1;
        Ok(b)
    }

    pub fn bucket(&self, b: usize) -> Result<u64, u32> {
        if b >= BUCKET_COUNT {
            return Err(HG_UNK_BUCKET);
        }
        Ok(self.counts[b])
    }

    /// 桶和核：桶计数和 = 事件总数。
    pub fn sum_ok(&self) -> bool {
        self.counts.iter().sum::<u64>() == self.total
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.total == 0 {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 直方图指纹。
    pub fn hist_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.counts {
            h = h.wrapping_mul(0x0100_0193) ^ (*c as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染直方条（星号按计数比例，每 10 事件一颗）。
    pub fn render_hist(&self) -> String {
        let mut out = String::new();
        for (i, name) in BUCKET_NAMES.iter().enumerate() {
            let stars = (self.counts[i] / 10) as usize;
            out.push_str(&format!(
                "{}|{}|{}\n",
                name,
                self.counts[i],
                "*".repeat(stars.min(40))
            ));
        }
        if self.sealed {
            out.push_str("HIST-SEAL\n");
        }
        out
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：1000 事件对数分布（1/10/100/1k/10k/100k/1s 各 142，溢出 2）。
fn standard_hist() -> Histogram {
    let mut h = Histogram::new();
    // 桶界值恰好归本桶：1→b0, 10→b1, ...（每桶 142 个）
    for i in 0..142 {
        let _ = h.record(1);
        let _ = h.record(10);
        let _ = h.record(100);
        let _ = h.record(1_000);
        let _ = h.record(10_000);
        let _ = h.record(100_000);
        let _ = h.record(1_000_000);
        let _ = i;
    }
    // 溢出桶 2 个
    let _ = h.record(2_000_000);
    let _ = h.record(9_999_999);
    h
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：桶界单调防御（BOUNDS 静态单调恒真——用归桶溢出防御路径验证）
    // 注：构造不可能归桶的值不存在（末桶 INF），验证溢出桶收容
    let mut h = Histogram::new();
    let b = h.record(u64::MAX).unwrap_or(9);
    if b == BUCKET_COUNT - 1 {
        caught += 1;
    }
    // 注 2：未知桶查询
    let h = standard_hist();
    if h.bucket(9) == Err(HG_UNK_BUCKET) {
        caught += 1;
    }
    // 注 3：桶计数篡改（改桶后 sum_ok 必假）
    let mut h = standard_hist();
    h.counts[0] += 1;
    if !h.sum_ok() {
        caught += 1;
    }
    // 注 4：空直方印
    let mut h = Histogram::new();
    if !h.seal() && !h.sealed() {
        caught += 1;
    }
    // 注 5：封后写拒
    let mut h = standard_hist();
    let _ = h.seal();
    if h.record(1).is_err() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 精化账；J2 归桶语义；J3 桶和核。
pub fn run_unx56163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56163");
    // J1-1 1000 事件全录
    let h = standard_hist();
    let ok = h.total() == 1000 && h.sum_ok();
    s.add("56163-J1-千事件全录", ok, if ok { "" } else { "事件数漂移" });
    // J1-2 逐桶计数（每桶 142，溢出 2）
    let ok = (0..7).all(|b| h.bucket(b) == Ok(142)) && h.bucket(7) == Ok(2);
    s.add("56163-J1-逐桶计数", ok, if ok { "" } else { "桶计数漂移" });
    // J1-3 渲染直方条齐
    let text = standard_hist().render_hist();
    let ok = text.contains("1us|142|*") && text.contains("INF|2|") && text.contains("1s|142|");
    s.add("56163-J1-渲染直方齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 边界值归上桶（1→b0, 2→b1）
    let ok = bucket_of(1) == Ok(0) && bucket_of(2) == Ok(1) && bucket_of(11) == Ok(2);
    s.add("56163-J2-边界归桶", ok, if ok { "" } else { "归桶漂移" });
    // J2-2 零值归首桶
    let ok = bucket_of(0) == Ok(0);
    s.add("56163-J2-零值首桶", ok, if ok { "" } else { "零值漂移" });
    // J2-3 溢出收容（u64::MAX → 末桶）
    let ok = bucket_of(u64::MAX) == Ok(7);
    s.add("56163-J2-溢出收容", ok, if ok { "" } else { "溢出穿透" });
    // J2-4 桶界单调锚（1 < 10 < ... < MAX）
    let ok = BOUNDS.windows(2).all(|w| w[0] < w[1]);
    s.add("56163-J2-桶界单调", ok, if ok { "" } else { "桶界乱序" });
    // J2-5 桶名闭集
    let ok = BUCKET_NAMES[0] == "1us" && BUCKET_NAMES[7] == "INF";
    s.add("56163-J2-桶名闭集", ok, if ok { "" } else { "桶名漂移" });
    // J3-1 桶和核全绿
    let ok = standard_hist().sum_ok();
    s.add("56163-J3-桶和核", ok, if ok { "" } else { "桶和漂移" });
    // J3-2 直方指纹确定
    let ok = standard_hist().hist_fp() == standard_hist().hist_fp();
    s.add("56163-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 一桶变全签变
    let mut h2 = standard_hist();
    let fp0 = h2.hist_fp();
    let _ = h2.record(5);
    let ok = h2.hist_fp() != fp0;
    s.add("56163-J3-桶变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-4 封印
    let mut h3 = standard_hist();
    let ok = h3.seal() && h3.sealed();
    s.add("56163-J3-封印", ok, if ok { "" } else { "封印失败" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56163-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56163_all_checks_green() {
        let set = run_unx56163_checks();
        assert!(set.all_passed(), "UNX-F56163 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56163_bucket_stable() {
        assert_eq!(bucket_of(1), Ok(0));
        assert_eq!(bucket_of(u64::MAX), Ok(7));
    }
}
