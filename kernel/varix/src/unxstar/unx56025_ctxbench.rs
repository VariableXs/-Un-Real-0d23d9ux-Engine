//! UNX-F56025 上下文切换延迟基准件（管程/管道乒乓）（0x60300 段）
//!
//! 锚点：UNX-F56025-J1 双进程管道乒乓 10⁶ 次切换采样（折算单次纳秒
//! 落账）；J2 绑核/不绑核两态分账（不许混样）；J3 自迁移干扰标记（乒
//! 乓任一侧跨核迁移即标污染并剔除）+ 双机对照行落账三判据全绿。判据
//! 锚定 §AI-71.2 专题二①扩展族。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·上下文切换面）。
//!
//! 结构：乒乓轮账（每轮折算单次切换纳秒 × CPU ID 落账）+ 绑核/不绑核
//! 两态闭集分账 + 迁移干扰标记核（轮内任一 CPU 异动标污染剔除）+ 剔
//! 除率账 + 双机对照行 + 注入核（越闭集态/零折算/迁移污染未剔/样本不
//! 足/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60300 独占 0x20）。
pub const SEG_BASE: u32 = 0x60300;
pub const CX_MODE_UNK: u32 = SEG_BASE + 0x01; // 越闭集绑核态
pub const CX_NS_ZERO: u32 = SEG_BASE + 0x02; // 零折算（轮空或除零）
pub const CX_POLLUTED: u32 = SEG_BASE + 0x03; // 自迁移污染轮未剔
pub const CX_STATS_MISS: u32 = SEG_BASE + 0x04; // 样本不足无统计
pub const CX_SEALED: u32 = SEG_BASE + 0x05; // 两态账已封账拒写
pub const CX_DUP_CELL: u32 = SEG_BASE + 0x06; // 同态重复建账
pub const CX_SWITCH_ZERO: u32 = SEG_BASE + 0x07; // 切换次数零（除零口径）

/// 绑核态闭集。
pub const MD_PINNED: usize = 0;
pub const MD_FREE: usize = 1;
pub const MD_COUNT: usize = 2;
pub const MD_NAMES: [&str; MD_COUNT] = ["pinned", "free"];

/// 正式切换次数口径（10⁶）。
pub const SWITCH_TARGET: u64 = 1_000_000;

/// 单轮乒乓账：切换次数 × 总耗时 × 双侧 CPU ID。
#[derive(Clone)]
pub struct PingPongRound {
    pub switches: u64,
    pub total_ns: u64,
    pub cpu_a: u32,
    pub cpu_b: u32,
}

impl PingPongRound {
    pub fn new(switches: u64, total_ns: u64, cpu_a: u32, cpu_b: u32) -> Result<Self, u32> {
        if switches == 0 {
            return Err(CX_SWITCH_ZERO);
        }
        if total_ns == 0 {
            return Err(CX_NS_ZERO);
        }
        Ok(PingPongRound { switches, total_ns, cpu_a, cpu_b })
    }

    /// 折算单次切换（纳秒，整数）。
    pub fn per_switch_ns(&self) -> u64 {
        self.total_ns / self.switches
    }

    /// J3：自迁移干扰判定（双侧同核 = 干净；异核 = 污染轮）。
    pub fn polluted(&self) -> bool {
        self.cpu_a != self.cpu_b
    }
}

/// 单态账：轮账 + 污染剔除账 + 分位统计。
pub struct ModeBook {
    pub mode: usize,
    pub pinned_cpu: Option<u32>,
    pub rounds: Vec<PingPongRound>,
    pub dropped: usize,
    pub sealed: bool,
}

impl ModeBook {
    pub fn new(mode: usize) -> Result<Self, u32> {
        if mode >= MD_COUNT {
            return Err(CX_MODE_UNK);
        }
        Ok(ModeBook {
            mode,
            pinned_cpu: None,
            rounds: Vec::new(),
            dropped: 0,
            sealed: false,
        })
    }

    /// 绑核声明（pinned 态必须声明；free 态忽略）。
    pub fn pin(&mut self, cpu: u32) {
        self.pinned_cpu = Some(cpu);
    }

    /// 落一轮：污染轮剔除（pinned 态任一侧离核即剔；free 态双侧异核剔）。
    pub fn record_round(&mut self, r: PingPongRound) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_SEALED);
        }
        let dirty = match self.mode {
            MD_PINNED => match self.pinned_cpu {
                None => return Err(CX_MODE_UNK),
                Some(p) => r.cpu_a != p || r.cpu_b != p,
            },
            _ => r.polluted(),
        };
        if dirty {
            self.dropped += 1;
            return Err(CX_POLLUTED);
        }
        self.rounds.push(r);
        Ok(())
    }

    /// 单次切换折算序列（只取干净轮）。
    pub fn per_switch_series(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for r in self.rounds.iter() {
            out.push(r.per_switch_ns());
        }
        out
    }

    /// p95（干净轮 ≥10 才出）。
    pub fn p95(&self) -> Result<u64, u32> {
        let v = self.per_switch_series();
        if v.len() < 10 {
            return Err(CX_STATS_MISS);
        }
        let mut s = v.clone();
        s.sort();
        let idx = (s.len() * 95 / 100).min(s.len() - 1);
        Ok(s[idx])
    }

    /// 剔除率（万分比）。
    pub fn drop_pbp(&self) -> u32 {
        let total = self.rounds.len() + self.dropped;
        if total == 0 {
            return 0;
        }
        ((self.dropped as u64 * 10_000) / total as u64) as u32
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：两态账（pinned 核 2 / free 态；各 12 干净轮 + 标准折算）。
/// pinned 轮 i：切换 10000 次，总耗时 (300+i)×10000 → 单次 300+i ns。
pub fn standard_modes() -> (ModeBook, ModeBook) {
    let mut pinned = ModeBook::new(MD_PINNED).expect("pinned");
    pinned.pin(2);
    let mut free = ModeBook::new(MD_FREE).expect("free");
    for i in 0..12u64 {
        let r = PingPongRound::new(10_000, (300 + i) * 10_000, 2, 2).expect("round");
        pinned.record_round(r).expect("pinned round");
    }
    for i in 0..12u64 {
        let r = PingPongRound::new(10_000, (400 + i) * 10_000, 2, 2).expect("round");
        free.record_round(r).expect("free round");
    }
    (pinned, free)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集态（mode=9）
    if ModeBook::new(9).err() == Some(CX_MODE_UNK) {
        caught += 1;
    }
    // 2) 切换次数零（除零口径）
    if PingPongRound::new(0, 1_000, 2, 2).err() == Some(CX_SWITCH_ZERO) {
        caught += 1;
    }
    // 3) 迁移污染轮剔除（pinned 态一侧离核）
    let mut b = standard_modes().0;
    let dirty = PingPongRound::new(10_000, 3_000_000, 2, 5).expect("dirty");
    if b.record_round(dirty) == Err(CX_POLLUTED) && b.dropped == 1 {
        caught += 1;
    }
    // 4) 未绑核即采（pinned 态未 pin）
    let mut b2 = ModeBook::new(MD_PINNED).expect("b2");
    let r = PingPongRound::new(10_000, 3_000_000, 2, 2).expect("r");
    if b2.record_round(r) == Err(CX_MODE_UNK) {
        caught += 1;
    }
    // 5) 样本不足拒 p95（9 轮 < 10）
    let mut b3 = ModeBook::new(MD_FREE).expect("b3");
    for _ in 0..9 {
        let r = PingPongRound::new(10_000, 4_000_000, 2, 2).expect("r2");
        b3.record_round(r).expect("ok");
    }
    if b3.p95() == Err(CX_STATS_MISS) {
        caught += 1;
    }
    (caught, 5)
}

/// 对照行：两态 × 双机 p95 × ratio（万分比）。
#[derive(Clone)]
pub struct CxCmpRow {
    pub mode: usize,
    pub varix_p95: u64,
    pub win_p95: u64,
    pub ratio_pbp: u64,
}

/// 逐态对照行（ratio = win/varix 万分比）。
pub fn cx_cmp(v: &ModeBook, w: &ModeBook) -> Result<CxCmpRow, u32> {
    let vp = v.p95()?;
    let wp = w.p95()?;
    if vp == 0 {
        return Err(CX_NS_ZERO);
    }
    Ok(CxCmpRow {
        mode: v.mode,
        varix_p95: vp,
        win_p95: wp,
        ratio_pbp: wp * 10_000 / vp,
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56025_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56025");
    // J1-1 乒乓折算语义（300+i ns 单次，第 0 轮 300）
    let (pinned, free) = standard_modes();
    let series = pinned.per_switch_series();
    let ok = series.len() == 12 && series[0] == 300 && series[11] == 311;
    s.add("56025-J1-乒乓折算语义", ok, if ok { "" } else { "折算漂移" });
    // J1-2 绑核态绑核语义（pinned 核 2 全轮一致）
    let mut all_cpu2 = true;
    for r in pinned.rounds.iter() {
        if r.cpu_a != 2 || r.cpu_b != 2 {
            all_cpu2 = false;
        }
    }
    let ok = all_cpu2 && pinned.pinned_cpu == Some(2);
    s.add("56025-J1-绑核一致", ok, if ok { "" } else { "绑核漂移" });
    // J1-3 两态分账不混（free p95 > pinned p95）
    let pp = pinned.p95().expect("pp");
    let fp = free.p95().expect("fp");
    let ok = pp == 311 && fp == 411 && fp > pp;
    s.add("56025-J1-两态分账", ok, if ok { "" } else { "两态混样" });
    // J2-1 自迁移干扰判定语义（同核净 / 异核污）
    let clean = PingPongRound::new(1_000, 300_000, 2, 2).expect("c");
    let dirty = PingPongRound::new(1_000, 300_000, 2, 3).expect("d");
    let ok = !clean.polluted() && dirty.polluted();
    s.add("56025-J2-迁移判定语义", ok, if ok { "" } else { "判定漂移" });
    // J2-2 污染轮剔除落账（剔除率 1/13 → 769 万分比）
    let mut b = standard_modes().0;
    let dirty2 = PingPongRound::new(10_000, 3_000_000, 2, 9).expect("d2");
    let _ = b.record_round(dirty2);
    let ok = b.dropped == 1 && b.rounds.len() == 12 && b.drop_pbp() == 769;
    s.add("56025-J2-污染剔除落账", ok, if ok { "" } else { "剔除账漂移" });
    // J2-3 剔除后 p95 不受污染轮影响（仍 311）
    let ok = b.p95().expect("p2") == 311;
    s.add("56025-J2-剔除后p95稳", ok, if ok { "" } else { "污染混入统计" });
    // J3-1 双机对照行（同夹具 ratio=10000）
    let (v, w) = standard_modes();
    let row = cx_cmp(&v, &w).expect("row");
    let ok = row.ratio_pbp == 10_000 && row.varix_p95 == 311;
    s.add("56025-J3-对照行语义", ok, if ok { "" } else { "对照行漂移" });
    // J3-2 对照 ratio 敏感（win 单次 ×2 → ratio=20000）
    let mut w2 = ModeBook::new(MD_PINNED).expect("w2");
    w2.pin(2);
    for i in 0..12u64 {
        let r = PingPongRound::new(10_000, (600 + i * 2) * 10_000, 2, 2).expect("r3");
        w2.record_round(r).expect("ok2");
    }
    let row2 = cx_cmp(&standard_modes().0, &w2).expect("row2");
    let ok = row2.ratio_pbp == 20_000;
    s.add("56025-J3-ratio敏感", ok, if ok { "" } else { "ratio 漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56025-J3-注入核必抓", ok, &detail);
    // J1-4 态名闭集同源
    let ok = MD_NAMES[MD_PINNED] == "pinned" && MD_NAMES[MD_FREE] == "free";
    s.add("56025-J1-态名同源", ok, if ok { "" } else { "命名漂移" });
    // J1-5 切换次数口径（10⁶ 目标常量）
    let ok = SWITCH_TARGET == 1_000_000;
    s.add("56025-J1-切换口径", ok, if ok { "" } else { "口径漂移" });
    // J2-4 封账冻结
    let mut b2 = standard_modes().0;
    b2.seal();
    let r4 = PingPongRound::new(10_000, 3_000_000, 2, 2).expect("r4");
    let ok = b2.record_round(r4) == Err(CX_SEALED);
    s.add("56025-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56025_all_checks_green() {
        let set = run_unx56025_checks();
        assert!(set.all_passed(), "UNX-F56025 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56025_per_switch() {
        let r = PingPongRound::new(1_000_000, 300_000_000, 2, 2).expect("r");
        assert_eq!(r.per_switch_ns(), 300);
    }
}
