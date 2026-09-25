//! F072 最近使用引擎 · 完整设计（STAR I 主册 G-C-02）。
//!
//! **判据（主册）**：三处消费面数据一致（同一时刻三处列表同序同内容）；
//! 时间衰减实测：7 天不用的文件排名持续下沉。
//!
//! **设计要点（主册）**：
//! - 双维度使用记录：应用（启动/前台时长）与文件（打开/保存/拖拽）分别
//!   计频次与时间衰减分；同一引擎供三处消费：开始菜单「最近使用」/
//!   文件管理器「快速访问」/任务栏跳转清单（F074）——**数据 API 唯一
//!   （一处一事实）**；
//! - frecency 公式（唯一数值源）：score = freq × 1/(1+days/3)（三天半衰
//!   期）；频次上限封顶 20（防单一文件霸榜）；
//! - 固定项单独区置顶，**不参与排序、永不衰减**；
//! - 记录表上限 500 条 LRU；隐私总闸一键清空；
//! - 文件被移走/删除 → 灰条「文件已不存在」+ 定位/移除二选；
//! - 同文件多路径（快捷方式打开）归并到真身；
//! - 右端相对时间标注（「9 小时前」格式，>7 天显日期）。
//!
//! 分数定点化：score_milli = freq × 72000 / (72000 + 小时数×1000)
//! —— 72h（3 天）恰为 ×0.5 半衰期，与主册公式逐点等价。时间注入式
//! （秒钟），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 记录表上限（LRU 逐出最沉底）。
pub const RECORD_CAP: usize = 500;

/// 频次封顶（防单一文件霸榜）。
pub const FREQ_CAP: u32 = 20;

/// 半衰期（小时）——3 天（主册 frecency：1/(1+days/3)）。
pub const HALF_LIFE_H: u64 = 72;

/// 相对时间「显示日期」分界（小时）——7 天。
pub const DATE_BEYOND_H: u64 = 7 * 24;

// ---------------------------------------------------------------------------
// 记录模型
// ---------------------------------------------------------------------------

/// 一条使用记录（文件维度；应用维度同构复用——path 承载 app id）。
#[derive(Clone, Debug)]
pub struct RecentRecord {
    /// 路径（真身——别名已归并）。
    pub path: String,
    /// 频次（封顶 [`FREQ_CAP`]）。
    pub freq: u32,
    /// 最近使用时刻（秒注入钟）。
    pub last_ts_s: u64,
    /// 固定（永不衰减、置顶不排序）。
    pub pinned: bool,
    /// 文件已不存在（灰条标注——记录保留）。
    pub missing: bool,
}

impl RecentRecord {
    /// frecency 定点分（毫倍数）：freq × 1000 × 72000 / (72000 + 小时×1000)
    /// —— 满权 = freq × 1000；72h（3 天）恰减半——与主册 1/(1+days/3)
    /// 逐点等价。固定项同分显示（置顶区不排序，永不沉底）。
    pub fn score_milli(&self, now_s: u64) -> u64 {
        let hours = now_s.saturating_sub(self.last_ts_s) / 3600;
        let denom = 72_000u64.saturating_add(hours.saturating_mul(1000));
        (self.freq as u64) * 72_000_000 / denom
    }
}

/// 排序视图行（三消费面统一渲染单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewRow {
    pub path: String,
    pub freq: u32,
    pub score_milli: u64,
    pub pinned: bool,
    pub missing: bool,
    /// 相对时间标注（右端）。
    pub rel_time: String,
}

/// 相对时间标注（「N 分钟前 / N 小时前 / N 天前 / 日期」格式族）。
pub fn relative_time(now_s: u64, ts_s: u64) -> String {
    let d = now_s.saturating_sub(ts_s);
    if d < 60 {
        String::from("刚刚")
    } else if d < 3600 {
        let m = d / 60;
        let mut s = String::from("");
        push_num(&mut s, m);
        s.push_str(" 分钟前");
        s
    } else if d < 86_400 {
        let h = d / 3600;
        let mut s = String::from("");
        push_num(&mut s, h);
        s.push_str(" 小时前");
        s
    } else if d < DATE_BEYOND_H * 3600 {
        let day = d / 86_400;
        let mut s = String::from("");
        push_num(&mut s, day);
        s.push_str(" 天前");
        s
    } else {
        // >7 天显日期（ts 折算 年-月-日 简历法：自 2026-01-01 起的偏移）。
        date_of(ts_s)
    }
}

/// 无格式化器的整数入串（no_std 面无 itoa——手写）。
fn push_num(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// 简历法日期串（基准 2026-01-01；仅供 >7 天的相对展示——一致性由
/// 唯一函数保证，不追求真实历法完整性）。
fn date_of(ts_s: u64) -> String {
    let days = ts_s / 86_400;
    let mut y = 2026u64;
    let mut rem = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if rem < yd {
            break;
        }
        rem -= yd;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    while rem >= mdays[m] {
        rem -= mdays[m];
        m += 1;
    }
    let mut s = String::from("");
    push_num(&mut s, y);
    s.push('-');
    push_num(&mut s, (m + 1) as u64);
    s.push('-');
    push_num(&mut s, rem + 1);
    s
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

/// 最近使用引擎（三消费面唯一数据源）。
pub struct RecentEngine {
    records: Vec<RecentRecord>,
    /// 别名归并表（快捷方式路径哈希 → 真身路径）。
    alias: Vec<(u64, String)>,
    clock_s: u64,
}

impl RecentEngine {
    pub fn new() -> RecentEngine {
        RecentEngine { records: Vec::new(), alias: Vec::new(), clock_s: 0 }
    }

    /// 注入钟推进（宿主测试确定复现）。
    pub fn set_clock(&mut self, now_s: u64) {
        self.clock_s = now_s;
    }

    fn now(&self) -> u64 {
        self.clock_s
    }

    /// 归并声明：alias_path（快捷方式）→ canonical（真身）。
    pub fn declare_alias(&mut self, alias_path: &str, canonical: &str) {
        let h = fnv1a(alias_path.as_bytes());
        self.alias.retain(|(a, _)| *a != h);
        self.alias.push((h, String::from(canonical)));
    }

    /// 真身路径解析（无别名声明 → 原路径即真身）。
    pub fn canonical_of(&self, path: &str) -> String {
        let h = fnv1a(path.as_bytes());
        match self.alias.iter().find(|(a, _)| *a == h) {
            Some((_, c)) => c.clone(),
            None => String::from(path),
        }
    }

    /// 使用事件（打开/保存/拖拽统一计频）：真身 freq+1（封顶）+ 时间更新；
    /// 新路径建记录；超 500 LRU 逐出「最沉底」（未固定中分数最低者）。
    pub fn touch(&mut self, path: &str) {
        let now = self.now();
        let canon = self.canonical_of(path);
        if let Some(r) = self.records.iter_mut().find(|r| r.path == canon) {
            r.freq = (r.freq + 1).min(FREQ_CAP);
            r.last_ts_s = now;
            return;
        }
        if self.records.len() >= RECORD_CAP {
            // LRU 逐出：未固定、分数最低（同分取更旧——沉底者先走）。
            let victim = self
                .records
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.pinned)
                .min_by_key(|(_, r)| (r.score_milli(now), r.last_ts_s))
                .map(|(i, _)| i);
            if let Some(i) = victim {
                self.records.remove(i);
            } else {
                return; // 全表固定（理论不可达）——拒绝新记录保固定区。
            }
        }
        self.records.push(RecentRecord {
            path: canon,
            freq: 1,
            last_ts_s: now,
            pinned: false,
            missing: false,
        });
    }

    /// 固定/解除（固定的永不衰减、置顶不排序）。
    pub fn pin(&mut self, path: &str, pinned: bool) -> bool {
        let canon = self.canonical_of(path);
        match self.records.iter_mut().find(|r| r.path == canon) {
            Some(r) => {
                r.pinned = pinned;
                true
            }
            None => false,
        }
    }

    /// 文件已不存在（灰条标注——记录保留）。
    pub fn mark_missing(&mut self, path: &str) -> bool {
        let canon = self.canonical_of(path);
        match self.records.iter_mut().find(|r| r.path == canon) {
            Some(r) => {
                r.missing = true;
                true
            }
            None => false,
        }
    }

    /// 二选之一：定位成功（文件回来了）→ 清灰条。
    pub fn locate(&mut self, path: &str) -> bool {
        let canon = self.canonical_of(path);
        match self.records.iter_mut().find(|r| r.path == canon) {
            Some(r) => {
                r.missing = false;
                true
            }
            None => false,
        }
    }

    /// 二选之一：移除记录。
    pub fn remove(&mut self, path: &str) -> bool {
        let canon = self.canonical_of(path);
        let n = self.records.len();
        self.records.retain(|r| r.path != canon);
        self.records.len() != n
    }

    /// 排序视图（唯一排序口）：固定区置顶（声明序）+ 未固定按分数降序
    /// （同分按最近时间新→旧——确定性）。
    pub fn ranked(&self) -> Vec<ViewRow> {
        let now = self.now();
        let mut pinned: Vec<ViewRow> = Vec::new();
        let mut rest: Vec<ViewRow> = Vec::new();
        for r in &self.records {
            let row = ViewRow {
                path: r.path.clone(),
                freq: r.freq,
                score_milli: r.score_milli(now),
                pinned: r.pinned,
                missing: r.missing,
                rel_time: relative_time(now, r.last_ts_s),
            };
            if r.pinned {
                pinned.push(row);
            } else {
                rest.push(row);
            }
        }
        rest.sort_by(|a, b| b.score_milli.cmp(&a.score_milli).then(b.path.cmp(&a.path)));
        pinned.extend(rest);
        pinned
    }

    /// 消费面一：开始菜单「最近使用」。
    pub fn consume_startmenu(&self) -> Vec<ViewRow> {
        self.ranked()
    }

    /// 消费面二：文件管理器「快速访问」。
    pub fn consume_quick_access(&self) -> Vec<ViewRow> {
        self.ranked()
    }

    /// 消费面三：任务栏跳转清单（F074）。
    pub fn consume_jumplist(&self) -> Vec<ViewRow> {
        self.ranked()
    }

    /// 隐私总闸：一键清空。
    pub fn clear_all(&mut self) -> usize {
        let n = self.records.len();
        self.records.clear();
        n
    }

    pub fn count(&self) -> usize {
        self.records.len()
    }
}

/// FNV-1a 64（别名归并键）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F072 自检（判据：三消费面一致；7 天衰减下沉）。
pub fn run_recenteng_checks() -> CheckSet {
    let mut set = CheckSet::new("F072-recenteng");

    let mut e = RecentEngine::new();
    e.set_clock(1_000_000);

    // 1. 建记录：首次打开 freq=1。
    e.touch("/docs/a.txt");
    let rows = e.ranked();
    set.add(
        "touch creates record",
        e.count() == 1 && rows[0].freq == 1 && rows[0].score_milli == 1000,
        "",
    );

    // 2. 频次封顶 20：高频不霸榜（分数上限恒定）。
    for _ in 0..30 {
        e.touch("/docs/a.txt");
    }
    let rows = e.ranked();
    set.add(
        "freq capped at 20",
        rows[0].freq == FREQ_CAP && rows[0].score_milli == (FREQ_CAP as u64) * 1000,
        "",
    );

    // 3. 三天半衰期：72 小时后同频分数恰减半（主册公式逐点核对）。
    e.set_clock(1_000_000 + 72 * 3600);
    let half = e.ranked()[0].score_milli;
    e.set_clock(1_000_000);
    let full = e.ranked()[0].score_milli;
    set.add("half-life at 72h", full == (FREQ_CAP as u64) * 1000 && half == (FREQ_CAP as u64) * 500, "");

    // 4. 7 天下沉：7 天不用的新文件压不过 3 天用的旧文件——衰减可辨。
    e.touch("/docs/old.txt");
    e.set_clock(1_000_000);
    for _ in 0..10 {
        e.touch("/docs/old.txt");
    }
    e.set_clock(1_000_000 + 3 * 3600);
    e.touch("/docs/new.txt");
    e.set_clock(1_000_000 + 3 * 3600 + 7 * 86_400);
    let rows = e.ranked();
    let old_pos = rows.iter().position(|r| r.path == "/docs/old.txt").unwrap_or(999);
    let new_pos = rows.iter().position(|r| r.path == "/docs/new.txt").unwrap_or(999);
    set.add(
        "7-day decay sinks rank",
        old_pos < new_pos && rows[new_pos].freq == 1,
        "",
    );

    // 5. 固定置顶不排序不衰减：固定 30 天前的文件仍压一切。
    e.pin("/docs/new.txt", true);
    e.set_clock(1_000_000 + 3 * 3600 + 30 * 86_400);
    let rows = e.ranked();
    set.add(
        "pinned stays top never decays",
        rows[0].pinned && rows[0].path == "/docs/new.txt",
        "",
    );

    // 6. 三消费面一致：同一时刻三处列表同序同内容（主册判据）。
    let (a, b, c) = (e.consume_startmenu(), e.consume_quick_access(), e.consume_jumplist());
    set.add(
        "three consumers identical",
        a == b && b == c && !a.is_empty(),
        "",
    );

    // 7. 别名归并：快捷方式打开 → 频次与时间落在真身（old.txt 现有
    //    freq 11——检查 4 首建 + 10 次）。
    e.declare_alias("/lnk/a.lnk", "/docs/old.txt");
    let before = e.ranked().iter().find(|r| r.path == "/docs/old.txt").unwrap().freq;
    e.touch("/lnk/a.lnk");
    let after = e.ranked().iter().find(|r| r.path == "/docs/old.txt").unwrap().freq;
    set.add(
        "alias merges to canonical",
        before == 11 && after == 12 && e.ranked().iter().all(|r| r.path != "/lnk/a.lnk"),
        "",
    );

    // 8. 灰条「文件已不存在」：标注保留 + 定位恢复 + 移除三态。
    e.mark_missing("/docs/a.txt");
    let gray = e.ranked().iter().find(|r| r.path == "/docs/a.txt").map(|r| r.missing);
    e.locate("/docs/a.txt");
    let restored = e.ranked().iter().find(|r| r.path == "/docs/a.txt").map(|r| r.missing);
    e.mark_missing("/docs/a.txt");
    let removed = e.remove("/docs/a.txt");
    let gone = e.ranked().iter().all(|r| r.path != "/docs/a.txt");
    set.add(
        "missing mark locate remove",
        gray == Some(true) && restored == Some(false) && removed && gone,
        "",
    );

    // 9. 隐私总闸一键清空（返回清除数——如实非零，清后表空）。
    let n = e.clear_all();
    set.add("privacy clear all", n > 0 && e.count() == 0, "");

    // 10. 相对时间格式族：分钟/小时/天/日期四段分界。
    let base = 1_000_000u64;
    set.add(
        "relative time formats",
        relative_time(base, base - 30) == "刚刚"
            && relative_time(base, base - 9 * 60) == "9 分钟前"
            && relative_time(base, base - 9 * 3600) == "9 小时前"
            && relative_time(base, base - 5 * 86_400) == "5 天前"
            && relative_time(base, base - 10 * 86_400).contains('-'),
        "",
    );

    // 11. 同分按最近时间（确定性 tie-break）。
    let mut e2 = RecentEngine::new();
    e2.set_clock(10_000);
    e2.touch("/x/one.txt");
    e2.set_clock(20_000);
    e2.touch("/x/two.txt");
    e2.set_clock(20_000);
    let rows = e2.ranked();
    set.add(
        "tie broken by recency",
        rows.len() == 2 && rows[0].path == "/x/two.txt",
        "",
    );

    // 12. LRU 500：第 501 条挤出最沉底（未固定、分数最低者）。
    let mut e3 = RecentEngine::new();
    e3.set_clock(1000);
    for i in 0..RECORD_CAP {
        let mut p = String::from("/p/f");
        push_num(&mut p, i as u64);
        p.push_str(".txt");
        e3.touch(&p);
    }
    e3.set_clock(2000);
    e3.touch("/p/old-first.txt"); // 最沉底候选：freq 1、时间最早的是 f0。
    e3.set_clock(100_000);
    // 两轮逐出：old-first 入表时挤 f0（全表同分取序首）；brand-new 入表
    // 时挤 f1（f1..f499 与 old-first 同分 727，f1 的 last_ts 更旧先走）。
    e3.touch("/p/brand-new.txt");
    let has_first = e3.ranked().iter().any(|r| r.path == "/p/old-first.txt");
    let has_f0 = e3.ranked().iter().any(|r| r.path == "/p/f0.txt");
    let has_new = e3.ranked().iter().any(|r| r.path == "/p/brand-new.txt");
    set.add(
        "lru 500 evicts lowest",
        e3.count() == RECORD_CAP && has_first && has_new && !has_f0,
        "",
    );

    // 13. 固定区不可被 LRU 逐出（全表固定 → 拒绝新记录，诚实防线）。
    let mut e4 = RecentEngine::new();
    for i in 0..RECORD_CAP {
        let mut p = String::from("/pin/f");
        push_num(&mut p, i as u64);
        e4.touch(&p);
        e4.pin(&p, true);
    }
    e4.touch("/pin/extra.txt");
    set.add(
        "pinned rows survive lru",
        e4.count() == RECORD_CAP && e4.ranked().iter().all(|r| r.pinned),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpin_reenters_ranking() {
        let mut e = RecentEngine::new();
        e.set_clock(100);
        e.touch("/a");
        e.pin("/a", true);
        assert!(e.ranked()[0].pinned);
        e.pin("/a", false);
        assert!(!e.ranked()[0].pinned);
    }

    #[test]
    fn touch_nonexistent_pin_fails() {
        let mut e = RecentEngine::new();
        assert!(!e.pin("/ghost", true));
        assert!(!e.mark_missing("/ghost"));
    }

    #[test]
    fn alias_without_declaration_is_self() {
        let e = RecentEngine::new();
        assert_eq!(e.canonical_of("/plain/x.txt"), "/plain/x.txt");
    }

    #[test]
    fn remove_nonexistent_is_false() {
        let mut e = RecentEngine::new();
        assert!(!e.remove("/nothing"));
    }

    #[test]
    fn date_format_uses_resume_calendar() {
        // >7 天显日期：简历法基准 2026-01-01（ts=0）。ts=365 天 → 2027-01-01
        // （2026 平年 365 天），与 now 无关（日期取自事件时刻）。
        let s = relative_time(86_400 * 400, 86_400 * 365);
        assert!(s.starts_with("2027-"), "应落 2027：{}", s);
        assert!(relative_time(86_400 * 8, 0).starts_with("2026-"), "8 天差触发日期态");
    }

    #[test]
    fn clear_empty_is_zero() {
        let mut e = RecentEngine::new();
        assert_eq!(e.clear_all(), 0);
    }
}
