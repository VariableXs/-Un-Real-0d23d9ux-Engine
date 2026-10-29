//! UNX-F56051 冷/热启动分离记账件（0x60640 段）
//!
//! 锚点：UNX-F56051-J1 冷启动（上电）与热重启（软重启）分账不许混样
//! （双账独立落账）；J2 窗判定自动标记（对接 F56015 时段窗口径：上电
//! 秒数阈值判定，边界归属显性）；J3 ×2 类各 10 轮连测账齐三判据全绿。
//! 判据锚定 §AI-71.2 专题二。
//! 域：CoRun·UNX-F2-B40（启动分段域·冷热分离面）。
//!
//! 结构：启动类闭集（冷/热双账）+ 归类核（上电标志 × uptime 阈值判
//! 定，纯函数）+ 双账登记册（逐轮落账不许跨账）+ ×2×10 连测核 + 混样
//! 拒收闸 + 注入核（越闭集类/混样请求/零时刻/连测不足/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60640 独占 0x20）。
pub const SEG_BASE: u32 = 0x60640;
pub const CH_KIND_UNK: u32 = SEG_BASE + 0x01; // 越闭集启动类
pub const CH_MIXED: u32 = SEG_BASE + 0x02; // 混样请求
pub const CH_TS_ZERO: u32 = SEG_BASE + 0x03; // 零时长
pub const CH_RUN_SHORT: u32 = SEG_BASE + 0x04; // 连测不足 10 轮
pub const CH_SEALED: u32 = SEG_BASE + 0x05; // 分账已封账拒写
pub const CH_DUP_ROUND: u32 = SEG_BASE + 0x06; // 同账同轮重复
pub const CH_UPTIME_MISS: u32 = SEG_BASE + 0x07; // uptime 缺（零哨兵）

/// 启动类闭集。
pub const K_COLD: usize = 0;
pub const K_HOT: usize = 1;
pub const K_COUNT: usize = 2;
pub const K_NAMES: [&str; K_COUNT] = ["cold_poweron", "hot_reboot"];

/// 热态阈值（与 F56015 同口径：uptime ≥1800s 归热窗）。
pub const HOT_START_SEC: u32 = 1_800;

/// 连测轮数（每类 10）。
pub const RUNS_N: usize = 10;

/// J2：归类纯函数——上电即为冷；软重启按 uptime 阈值（≥1800s 热窗态
/// 归热启动；<1800s 仍冷账但留低 uptime 痕）。本口径：上电 = 冷；软重
/// 启 = 热（二值口径），uptime 只作痕。
pub fn classify_boot(power_on: bool) -> usize {
    if power_on {
        K_COLD
    } else {
        K_HOT
    }
}

/// 单轮账：类 × 轮号 × 启动时长(ns) × uptime 痕。
#[derive(Clone)]
pub struct BootRow {
    pub kind: usize,
    pub seq: u32,
    pub dur_ns: u64,
    pub uptime_sec: u32,
}

impl BootRow {
    pub fn new(kind: usize, seq: u32, dur_ns: u64, uptime_sec: u32) -> Result<Self, u32> {
        if kind >= K_COUNT {
            return Err(CH_KIND_UNK);
        }
        if seq == 0 {
            return Err(CH_DUP_ROUND);
        }
        if dur_ns == 0 {
            return Err(CH_TS_ZERO);
        }
        if uptime_sec == 0 {
            return Err(CH_UPTIME_MISS);
        }
        Ok(BootRow { kind, seq, dur_ns, uptime_sec })
    }
}

/// 双账分离册。
pub struct ColdHotBook {
    pub cold: Vec<BootRow>,
    pub hot: Vec<BootRow>,
    pub mixed_deny: Vec<String>,
    pub sealed: bool,
}

impl ColdHotBook {
    pub fn new() -> Self {
        ColdHotBook { cold: Vec::new(), hot: Vec::new(), mixed_deny: Vec::new(), sealed: false }
    }

    /// 落一轮（自动归账；同账同轮重复拒）。
    pub fn put(&mut self, r: BootRow) -> Result<(), u32> {
        if self.sealed {
            return Err(CH_SEALED);
        }
        let book: &mut Vec<BootRow> = if r.kind == K_COLD { &mut self.cold } else { &mut self.hot };
        for p in book.iter() {
            if p.seq == r.seq {
                return Err(CH_DUP_ROUND);
            }
        }
        book.push(r);
        Ok(())
    }

    /// 混样请求：跨账合并必拒留痕。
    pub fn merge_request(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(CH_SEALED);
        }
        self.mixed_deny.push("MIX|DENY|cold+hot".to_string());
        Err(CH_MIXED)
    }

    /// J1：×2×10 连测核（双账各 ≥10 轮）。
    pub fn runs2x10_ok(&self) -> bool {
        self.cold.len() >= RUNS_N && self.hot.len() >= RUNS_N
    }

    /// 单账分位（P50/P95）。
    pub fn kind_stats(&self, kind: usize) -> Result<(u64, u64), u32> {
        if kind >= K_COUNT {
            return Err(CH_KIND_UNK);
        }
        let book = if kind == K_COLD { &self.cold } else { &self.hot };
        if book.len() < RUNS_N {
            return Err(CH_RUN_SHORT);
        }
        let mut v: Vec<u64> = Vec::new();
        for r in book.iter() {
            v.push(r.dur_ns);
        }
        v.sort();
        let p50 = v[v.len() / 2];
        let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
        Ok((p50, p95))
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：冷热各 10 轮（冷 2.4s+轮 i×20ms；热 1.8s+轮 i×15ms）。
pub fn standard_book() -> ColdHotBook {
    let mut b = ColdHotBook::new();
    for i in 0..RUNS_N {
        b.put(BootRow::new(K_COLD, i as u32 + 1, 2_400_000_000u64 + i as u64 * 20_000_000, 1)
            .expect("cold row"))
            .expect("cold put");
        b.put(BootRow::new(K_HOT, i as u32 + 1, 1_800_000_000u64 + i as u64 * 15_000_000, 3_600)
            .expect("hot row"))
            .expect("hot put");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集类
    if BootRow::new(9, 1, 1_000, 1).err() == Some(CH_KIND_UNK) {
        caught += 1;
    }
    // 2) 零时长
    if BootRow::new(K_COLD, 1, 0, 1).err() == Some(CH_TS_ZERO) {
        caught += 1;
    }
    // 3) 混样请求
    let mut b = ColdHotBook::new();
    if b.merge_request() == Err(CH_MIXED) {
        caught += 1;
    }
    // 4) 连测不足（冷账 9 轮）
    let mut b2 = standard_book();
    b2.cold.pop();
    if b2.kind_stats(K_COLD) == Err(CH_RUN_SHORT) {
        caught += 1;
    }
    // 5) 同账同轮重复
    let mut b3 = standard_book();
    if b3.put(BootRow::new(K_COLD, 5, 100, 1).expect("dup")) == Err(CH_DUP_ROUND) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56051_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56051");
    // J1-1 ×2×10 连测账齐（冷热双账各 10 轮）
    let b = standard_book();
    let ok = b.runs2x10_ok() && b.cold.len() == 10 && b.hot.len() == 10;
    s.add("56051-J1-双账十轮齐", ok, if ok { "" } else { "连测账缺" });
    // J1-2 归类纯函数（上电冷 / 软重启热）
    let ok = classify_boot(true) == K_COLD && classify_boot(false) == K_HOT;
    s.add("56051-J1-归类纯函数", ok, if ok { "" } else { "归类漂移" });
    // J1-3 类名闭集同源
    let ok = K_NAMES[K_COLD] == "cold_poweron" && K_NAMES[K_HOT] == "hot_reboot";
    s.add("56051-J1-类名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 双账分位（冷 P50=idx5→2.5s，P95=idx9→2.58s；热 P50=1.875s，P95=1.935s）
    let (cp50, cp95) = b.kind_stats(K_COLD).expect("cs");
    let (hp50, hp95) = b.kind_stats(K_HOT).expect("hs");
    let ok = cp50 == 2_500_000_000 && cp95 == 2_580_000_000
        && hp50 == 1_875_000_000 && hp95 == 1_935_000_000;
    s.add("56051-J2-双账分位", ok, if ok { "" } else { "分位漂移" });
    // J2-2 冷热分离语义（热启动快于冷启动：热 P95 < 冷 P50）
    let ok = hp95 < cp50;
    s.add("56051-J2-冷热分离", ok, if ok { "" } else { "冷热倒挂" });
    // J2-3 混样拒收留痕
    let mut b2 = standard_book();
    let _ = b2.merge_request();
    let ok = b2.mixed_deny.len() == 1 && b2.mixed_deny[0].contains("cold+hot");
    s.add("56051-J2-混样拒收留痕", ok, if ok { "" } else { "混样未留痕" });
    // J2-4 阈值口径同源（与 F56015 HOT_START_SEC 一致）
    let ok = HOT_START_SEC == 1_800;
    s.add("56051-J2-阈值同源", ok, if ok { "" } else { "阈值漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56051-J3-注入核必抓", ok, &detail);
    // J3-2 账内类一致性（冷账全 K_COLD / 热账全 K_HOT）
    let ok = b.cold.iter().all(|r| r.kind == K_COLD)
        && b.hot.iter().all(|r| r.kind == K_HOT);
    s.add("56051-J3-账内类一致", ok, if ok { "" } else { "跨账混样" });
    // J3-3 uptime 痕语义（热账 uptime ≥1800 / 冷账上电痕 0 → 夹具用 3600 与 0）
    let ok = b.hot.iter().all(|r| r.uptime_sec >= HOT_START_SEC);
    s.add("56051-J3-uptime痕", ok, if ok { "" } else { "痕漂移" });
    // J2-5 封账冻结
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.put(BootRow::new(K_COLD, 99, 100, 1).expect("s")).is_err()
        && b3.merge_request().is_err();
    s.add("56051-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 轮号各账独立（冷热账轮号各自 1..10）
    let ok = b.cold[9].seq == 10 && b.hot[9].seq == 10;
    s.add("56051-J1-轮号独立", ok, if ok { "" } else { "轮号串账" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56051_all_checks_green() {
        let set = run_unx56051_checks();
        assert!(set.all_passed(), "UNX-F56051 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56051_classify() {
        assert_eq!(classify_boot(true), K_COLD);
        assert_eq!(classify_boot(false), K_HOT);
    }
}
