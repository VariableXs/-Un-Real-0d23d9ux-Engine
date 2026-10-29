//! UNX-F56015 采集时段窗规范件（0x601C0 段）
//!
//! 锚点：UNX-F56015-J1 冷启动窗（上电后 T+0）与热态窗（连续运行
//! T+30min）分离登记（双窗分账不许混样）；J2 窗判定自动标记（按上电
//! 秒数自动归窗，×10 场景全判对）；J3 混样拒收（跨窗样本合并请求必
//! 拒并留痕）三判据全绿。判据锚定 §AI-71.2 专题二。
//! 域：CoRun·UNX-F2-B40（环境冻结域·时段窗面）。
//!
//! 结构：双窗闭集登记册（冷窗 T+0 / 热窗 T+1800s 逐窗落账）+ 自动归
//！窗核（uptime 秒数阈值判定，边界归属显性）+ 分账闸（跨窗合并拒收
//! 留痕）+ 窗样本账（逐窗独立计数）+ 注入核（越闭集窗/混样请求/零上
//! 电时刻/封账改写/边界漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x601C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x601C0;
pub const TW_UNK_WIN: u32 = SEG_BASE + 0x01; // 越闭集窗
pub const TW_MIXED: u32 = SEG_BASE + 0x02; // 跨窗混样请求
pub const TW_UPTIME_MISS: u32 = SEG_BASE + 0x03; // 上电时刻缺（零哨兵）
pub const TW_SEALED: u32 = SEG_BASE + 0x04; // 窗账已封账拒写
pub const TW_BOUNDARY: u32 = SEG_BASE + 0x05; // 边界归属漂移
pub const TW_DUP_WIN: u32 = SEG_BASE + 0x06; // 同窗重复登记
pub const TW_ORDER_BAD: u32 = SEG_BASE + 0x07; // 窗起始序错乱（热窗早于冷窗）

/// 时段窗闭集。
pub const W_COLD: usize = 0;
pub const W_HOT: usize = 1;
pub const W_COUNT: usize = 2;
pub const W_NAMES: [&str; W_COUNT] = ["cold_T0", "hot_T30min"];

/// 热态窗起始（秒）：上电后 30 分钟。
pub const HOT_START_SEC: u32 = 1_800;

/// 自动归窗：uptime < 1800 → 冷窗；uptime >= 1800 → 热窗（边界归热窗）。
pub fn classify_window(uptime_sec: u32) -> usize {
    if uptime_sec < HOT_START_SEC {
        W_COLD
    } else {
        W_HOT
    }
}

/// 单窗账目：窗 × 起始秒 × 说明。
#[derive(Clone)]
pub struct WindowEntry {
    pub win: usize,
    pub start_sec: u32,
    pub desc: String,
}

impl WindowEntry {
    pub fn new(win: usize, start_sec: u32, desc: &str) -> Result<Self, u32> {
        if win >= W_COUNT {
            return Err(TW_UNK_WIN);
        }
        if desc.is_empty() {
            return Err(TW_ORDER_BAD);
        }
        Ok(WindowEntry { win, start_sec, desc: desc.to_string() })
    }
}

/// 时段窗分账册：双窗登记 + 自动标记 + 窗样本计数。
pub struct TimeWindowBook {
    pub wins: [Option<WindowEntry>; W_COUNT],
    pub marks: Vec<(u32, usize)>,
    pub mixed_deny: Vec<String>,
    pub sealed: bool,
}

impl TimeWindowBook {
    pub fn new() -> Self {
        TimeWindowBook { wins: [None, None], marks: Vec::new(), mixed_deny: Vec::new(), sealed: false }
    }

    /// 双窗登记（起始序核：冷窗 0，热窗 ≥1800；同窗重复拒）。
    pub fn register(&mut self, e: WindowEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(TW_SEALED);
        }
        if e.win >= W_COUNT {
            return Err(TW_UNK_WIN);
        }
        if self.wins[e.win].is_some() {
            return Err(TW_DUP_WIN);
        }
        if e.win == W_COLD && e.start_sec != 0 {
            return Err(TW_ORDER_BAD);
        }
        if e.win == W_HOT && e.start_sec < HOT_START_SEC {
            return Err(TW_ORDER_BAD);
        }
        let win = e.win;
        self.wins[win] = Some(e);
        Ok(())
    }

    /// 自动标记：uptime → 窗，落标记账（零哨兵拒）。
    pub fn auto_mark(&mut self, uptime_sec: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(TW_SEALED);
        }
        if uptime_sec == 0 {
            return Err(TW_UPTIME_MISS);
        }
        let w = classify_window(uptime_sec);
        self.marks.push((uptime_sec, w));
        Ok(w)
    }

    /// 混样请求：两窗样本合并必拒并留痕。
    pub fn merge_request(&mut self, wa: usize, wb: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(TW_SEALED);
        }
        if wa >= W_COUNT || wb >= W_COUNT {
            return Err(TW_UNK_WIN);
        }
        if wa != wb {
            self.mixed_deny.push(format!("MIX|DENY|{}+{}", W_NAMES[wa], W_NAMES[wb]));
            return Err(TW_MIXED);
        }
        Ok(())
    }

    /// J2：×10 场景标记核（标记账逐条与判定函数一致）。
    pub fn marks_consistent(&self) -> bool {
        for m in self.marks.iter() {
            if classify_window(m.0) != m.1 {
                return false;
            }
        }
        self.marks.len() >= 10
    }

    /// J1：双窗分账核（两窗都登记且起始序合规）。
    pub fn both_wins_ok(&self) -> bool {
        match (self.wins[W_COLD].as_ref(), self.wins[W_HOT].as_ref()) {
            (Some(c), Some(h)) => {
                c.start_sec == 0 && h.start_sec >= HOT_START_SEC && c.win < h.win
            }
            _ => false,
        }
    }

    /// 混样拒收计数。
    pub fn mixed_count(&self) -> usize {
        self.mixed_deny.len()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：双窗登记 + 10 场景自动标记。
pub fn standard_book() -> TimeWindowBook {
    let mut b = TimeWindowBook::new();
    b.register(WindowEntry::new(W_COLD, 0, "power-on T+0").expect("cold")).expect("reg c");
    b.register(WindowEntry::new(W_HOT, HOT_START_SEC, "steady T+30min").expect("hot"))
        .expect("reg h");
    let scenes = [1u32, 60, 300, 900, 1_799, 1_800, 1_801, 3_600, 7_200, 10_000];
    for t in scenes.iter() {
        b.auto_mark(*t).expect("mark");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集窗（win=9）
    if WindowEntry::new(9, 0, "x").err() == Some(TW_UNK_WIN) {
        caught += 1;
    }
    // 2) 冷窗起始非零（序错乱）
    let mut b = TimeWindowBook::new();
    if b.register(WindowEntry::new(W_COLD, 100, "x").expect("cold bad")) == Err(TW_ORDER_BAD) {
        caught += 1;
    }
    // 3) 零哨兵 uptime
    let mut b2 = standard_book();
    if b2.auto_mark(0) == Err(TW_UPTIME_MISS) {
        caught += 1;
    }
    // 4) 混样请求拒
    let mut b3 = standard_book();
    if b3.merge_request(W_COLD, W_HOT) == Err(TW_MIXED) {
        caught += 1;
    }
    // 5) 封账后改写
    let mut b4 = standard_book();
    b4.seal();
    if b4.auto_mark(100) == Err(TW_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56015");
    // J1-1 双窗分账登记齐
    let b = standard_book();
    let ok = b.both_wins_ok();
    s.add("56015-J1-双窗分账登记", ok, if ok { "" } else { "双窗登记缺" });
    // J1-2 窗名闭集同源
    let ok = W_NAMES[W_COLD] == "cold_T0" && W_NAMES[W_HOT] == "hot_T30min";
    s.add("56015-J1-窗名同源", ok, if ok { "" } else { "窗名漂移" });
    // J1-3 热窗起始语义（30 分钟）
    let ok = HOT_START_SEC == 1_800
        && standard_book().wins[W_HOT].as_ref().expect("h").start_sec == 1_800;
    s.add("56015-J1-热窗起始语义", ok, if ok { "" } else { "起始漂移" });
    // J2-1 ×10 场景自动标记全一致
    let ok = standard_book().marks_consistent() && standard_book().marks.len() == 10;
    s.add("56015-J2-十场景自动标记", ok, if ok { "" } else { "标记漂移" });
    // J2-2 边界归属显性（1799 冷 / 1800 热）
    let ok = classify_window(1_799) == W_COLD && classify_window(1_800) == W_HOT;
    s.add("56015-J2-边界归属显性", ok, if ok { "" } else { "边界漂移" });
    // J2-3 深冷深热判定（60 冷 / 7200 热）
    let ok = classify_window(60) == W_COLD && classify_window(7_200) == W_HOT;
    s.add("56015-J2-深冷深热判定", ok, if ok { "" } else { "判定漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56015-J3-注入核必抓", ok, &detail);
    // J3-2 混样拒收留痕（MIX|DENY 行）
    let mut b2 = standard_book();
    let _ = b2.merge_request(W_HOT, W_COLD);
    let ok = b2.mixed_count() == 1 && b2.mixed_deny[0].contains("MIX|DENY|hot_T30min+cold_T0");
    s.add("56015-J3-混样拒收留痕", ok, if ok { "" } else { "混样未留痕" });
    // J3-3 同窗合并不误拒
    let mut b3 = standard_book();
    let ok = b3.merge_request(W_COLD, W_COLD).is_ok() && b3.mixed_count() == 0;
    s.add("56015-J3-同窗合并放行", ok, if ok { "" } else { "同窗误拒" });
    // J1-4 同窗重复登记拒
    let mut b4 = TimeWindowBook::new();
    let _ = b4.register(WindowEntry::new(W_COLD, 0, "x").expect("c"));
    let ok = b4.register(WindowEntry::new(W_COLD, 0, "y").expect("c2")) == Err(TW_DUP_WIN);
    s.add("56015-J1-重复登记拒", ok, if ok { "" } else { "重复穿透" });
    // J1-5 封账冻结
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.register(WindowEntry::new(W_COLD, 0, "z").expect("s")) == Err(TW_SEALED);
    s.add("56015-J1-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-4 标记账与窗闭集一致（标记值只落 0/1）
    let ok = standard_book().marks.iter().all(|m| m.1 < W_COUNT);
    s.add("56015-J2-标记闭集一致", ok, if ok { "" } else { "标记越集" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56015_all_checks_green() {
        let set = run_unx56015_checks();
        assert!(set.all_passed(), "UNX-F56015 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56015_boundary() {
        assert_eq!(classify_window(1_799), W_COLD);
        assert_eq!(classify_window(1_800), W_HOT);
    }
}
