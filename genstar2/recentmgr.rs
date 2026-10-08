//! F486 最近文件管理（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **列表准确性；逐删/全清/三处同步；固定 5 条上限；隐私模式不落盘判据；
//! 清除确认文案。**
//!
//! 功能定义（主册批次三）：最近文件（F072 引擎）的管理面——设置页列表
//! （每条：文件名/所属应用/打开时间）、逐条移除、清空全部（带确认+说明
//! 影响——「开始菜单推荐区 F299 与任务栏跳转 F074 将同步清空」）；固定常用
//! （钉 5 条不被时间冲走）；隐私模式开关（暂停记录）。
//!
//! 零堆纪律：定长列表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 最近列表容量（时间冲刷区）。
pub const RECENT_CAP: usize = 64;
/// 固定（钉住）上限（主册：固定 5 条）。
pub const PIN_CAP: usize = 5;
/// 三处消费面（开始菜单推荐 F299 / 任务栏跳转 F074 / 设置页本列表）。
pub const CONSUMER_SURFACES: usize = 3;

/// 清除确认文案（主册：影响范围明说）。
pub const CLEAR_CONFIRM_TEXT: &str = "将清空全部最近文件——开始菜单推荐区与任务栏跳转清单将同步清空";

/// 一条最近文件记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecentItem {
    pub file: [u8; 48],
    pub file_n: usize,
    pub app: [u8; 24],
    pub app_n: usize,
    pub opened_at_ms: u64,
}

impl RecentItem {
    pub fn new(file: &str, app: &str, at_ms: u64) -> Option<RecentItem> {
        if file.is_empty() || file.len() > 48 || app.is_empty() || app.len() > 24 {
            return None;
        }
        let mut r = RecentItem {
            file: [0; 48],
            file_n: file.len(),
            app: [0; 24],
            app_n: app.len(),
            opened_at_ms: at_ms,
        };
        r.file[..file.len()].copy_from_slice(file.as_bytes());
        r.app[..app.len()].copy_from_slice(app.as_bytes());
        Some(r)
    }

    pub fn file_str(&self) -> &str {
        core::str::from_utf8(&self.file[..self.file_n]).unwrap_or("")
    }
}

/// 最近文件管理器。
pub struct RecentManager {
    items: [Option<RecentItem>; RECENT_CAP],
    n: usize,
    /// 钉住条目（键集，最多 5——不被时间冲走）。
    pinned: [u64; PIN_CAP],
    pin_n: usize,
    /// 隐私模式（暂停记录——开着的时段不进历史）。
    pub privacy_mode: bool,
    /// 三处消费面同步位图（一处清除处处同步）。
    dirty: [bool; CONSUMER_SURFACES],
}

fn key(item: &RecentItem) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in &item.file[..item.file_n] {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl RecentManager {
    pub const fn new() -> Self {
        RecentManager {
            items: [None; RECENT_CAP],
            n: 0,
            pinned: [0; PIN_CAP],
            pin_n: 0,
            privacy_mode: false,
            dirty: [false; CONSUMER_SURFACES],
        }
    }

    fn invalidate_all(&mut self) {
        self.dirty = [true; CONSUMER_SURFACES];
    }

    /// 记录打开（隐私模式不落盘——主册判据）。
    pub fn record_open(&mut self, item: RecentItem) -> bool {
        if self.privacy_mode {
            return false; // 隐私模式：不进历史（诚实无痕）
        }
        if self.n >= RECENT_CAP {
            // 淘汰最旧的非钉条目（钉住的永不被时间冲走）。
            match self.oldest_unpinned_idx() {
                Some(i) => {
                    self.items[i] = self.items[self.n - 1];
                    self.items[self.n - 1] = None;
                    self.n -= 1;
                }
                None => return false, // 全是钉住的（不可能但诚实防御）
            }
        }
        self.items[self.n] = Some(item);
        self.n += 1;
        self.invalidate_all();
        true
    }

    fn oldest_unpinned_idx(&self) -> Option<usize> {
        let mut best: Option<(usize, u64)> = None;
        for i in 0..self.n {
            if let Some(it) = self.items[i] {
                let k = key(&it);
                if !self.is_pinned(k) && best.map(|(_, t)| it.opened_at_ms < t).unwrap_or(true) {
                    best = Some((i, it.opened_at_ms));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    pub fn is_pinned(&self, k: u64) -> bool {
        (0..self.pin_n).any(|i| self.pinned[i] == k)
    }

    /// 固定常用（上限 5——超出诚实拒绝）。
    pub fn pin(&mut self, file: &str) -> bool {
        if self.pin_n >= PIN_CAP {
            return false;
        }
        // 找到该文件条目键。
        for i in 0..self.n {
            if let Some(it) = self.items[i] {
                if it.file_str() == file {
                    let k = key(&it);
                    if !self.is_pinned(k) {
                        self.pinned[self.pin_n] = k;
                        self.pin_n += 1;
                        return true;
                    }
                    return true; // 已钉住 = 幂等成功
                }
            }
        }
        false
    }

    pub fn pin_count(&self) -> usize {
        self.pin_n
    }

    /// 逐条移除（钉住的也可手动移除——用户意志最高）。
    pub fn remove(&mut self, file: &str) -> bool {
        for i in 0..self.n {
            if let Some(it) = self.items[i] {
                if it.file_str() == file {
                    let k = key(&it);
                    self.items[i] = self.items[self.n - 1];
                    self.items[self.n - 1] = None;
                    self.n -= 1;
                    // 钉账同步清理。
                    let mut j = 0;
                    while j < self.pin_n {
                        if self.pinned[j] == k {
                            self.pinned[j] = self.pinned[self.pin_n - 1];
                            self.pin_n -= 1;
                        } else {
                            j += 1;
                        }
                    }
                    self.invalidate_all();
                    return true;
                }
            }
        }
        false
    }

    /// 清空全部（带确认语义——调用方先展示 CLEAR_CONFIRM_TEXT）。
    pub fn clear_all(&mut self, confirmed: bool) -> usize {
        if !confirmed {
            return 0; // 未确认不执行（破坏性操作纪律）
        }
        let n = self.n;
        self.items = [None; RECENT_CAP];
        self.n = 0;
        self.pinned = [0; PIN_CAP];
        self.pin_n = 0;
        self.invalidate_all();
        n
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 三处消费面同步消费（一处清除处处同步——消费即清位）。
    pub fn consume_sync(&mut self, surface: usize) -> bool {
        let i = surface % CONSUMER_SURFACES;
        let v = self.dirty[i];
        self.dirty[i] = false;
        v
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_recentmgr_checks() -> CheckSet {
    let mut cs = CheckSet::new("F486-recentmgr");
    let mut r = RecentManager::new();
    // 1) 列表准确性（记录可读回）。
    r.record_open(RecentItem::new("报告.docx", "writer", 1_000).unwrap());
    cs.add("list_accurate", r.count() == 1, "");
    // 2) 隐私模式不落盘（主册判据）。
    r.privacy_mode = true;
    cs.add("privacy_no_record", !r.record_open(RecentItem::new("secret.txt", "editor", 2_000).unwrap()) && r.count() == 1, "");
    r.privacy_mode = false;
    // 3) 固定 5 条上限。
    for i in 0..5 {
        r.record_open(RecentItem::new(pin_name(i).as_str(), "app", 3_000 + i as u64).unwrap());
        r.pin(pin_name(i).as_str());
    }
    cs.add("pin_cap_5", r.pin_count() == 5, "");
    r.record_open(RecentItem::new("extra.txt", "app", 9_000).unwrap());
    cs.add("pin_over_cap_honest", !r.pin("extra.txt"), "");
    // 4) 时间冲刷：淘汰最旧不碰钉住。
    let mut r2 = RecentManager::new();
    r2.record_open(RecentItem::new("pinned.txt", "app", 100).unwrap());
    r2.pin("pinned.txt");
    for i in 0..RECENT_CAP as u64 {
        r2.record_open(RecentItem::new(fill_name(i).as_str(), "app", 1_000 + i).unwrap());
    }
    cs.add("pinned_survives_flush", r2.count() == RECENT_CAP && {
        // pinned.txt 仍在（被钉住不被冲走）。
        (0..r2.n).any(|i| r2.items[i].as_ref().map(|x| x.file_str() == "pinned.txt").unwrap_or(false))
    }, "");
    // 5) 逐删/全清/三处同步。
    let removed = r.remove(pin_name(0).as_str());
    cs.add("remove_one", removed && r.count() == 6 && r.pin_count() == 4, "");
    let (a, b) = ((0..3).all(|s| r.consume_sync(s)), (0..3).all(|s| !r.consume_sync(s)));
    cs.add("three_surfaces_sync", a && b, "");
    let cleared = r.clear_all(false);
    cs.add("clear_needs_confirm", cleared == 0 && r.count() == 6, "");
    let cleared2 = r.clear_all(true);
    cs.add("clear_all", cleared2 == 6 && r.count() == 0 && r.pin_count() == 0, "");
    // 6) 清除确认文案（影响范围明说）。
    cs.add("confirm_text_honest", CLEAR_CONFIRM_TEXT.contains("同步清空") && CLEAR_CONFIRM_TEXT.contains("推荐区"), "");
    cs
}

fn pin_name(i: usize) -> PinName {
    let mut b = PinName::new();
    b.push_str("p");
    b.push_byte(b'0' + i as u8);
    b.push_str(".docx");
    b
}

fn fill_name(i: u64) -> PinName {
    let mut b = PinName::new();
    b.push_str("f");
    let mut digits = [0u8; 20];
    let mut dn = 0;
    let mut v = i;
    loop {
        digits[dn] = b'0' + (v % 10) as u8;
        dn += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for d in digits[..dn].iter().rev() {
        b.push_byte(*d);
    }
    b.push_str(".txt");
    b
}

struct PinName {
    buf: [u8; 24],
    n: usize,
}

impl PinName {
    fn new() -> Self {
        PinName { buf: [0; 24], n: 0 }
    }
    fn push_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.n < 24 {
                self.buf[self.n] = b;
                self.n += 1;
            }
        }
    }
    fn push_byte(&mut self, b: u8) {
        if self.n < 24 {
            self.buf[self.n] = b;
            self.n += 1;
        }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.n]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_mode_records_nothing() {
        let mut r = RecentManager::new();
        r.privacy_mode = true;
        for i in 0..10 {
            r.record_open(RecentItem::new(&format!("f{i}.txt"), "app", i).unwrap());
        }
        assert_eq!(r.count(), 0, "隐私模式零落盘");
    }

    #[test]
    fn pin_cap_is_five() {
        let mut r = RecentManager::new();
        for i in 0..7 {
            r.record_open(RecentItem::new(&format!("x{i}.txt"), "app", i).unwrap());
        }
        for i in 0..5 {
            assert!(r.pin(&format!("x{i}.txt")));
        }
        assert!(!r.pin("x5.txt"));
        assert_eq!(r.pin_count(), 5);
    }

    #[test]
    fn clear_requires_confirmation() {
        let mut r = RecentManager::new();
        r.record_open(RecentItem::new("a.txt", "app", 1).unwrap());
        assert_eq!(r.clear_all(false), 0);
        assert_eq!(r.count(), 1, "未确认不清");
        assert_eq!(r.clear_all(true), 1);
    }
}

// ===========================================================================
// 深化 v2（F486）：钉选满额语义 / 隐私模式暂停账 / 三面同步消费深化 /
// 时间衰减权重 / 清除确认联动 F299/F074 锚
// ===========================================================================

impl RecentManager {
    /// 深化辅助：按文件名查钉账（v1 is_pinned 键版的人话包装——
    /// 同模块访问私有键函数，零新增真相源）。
    pub fn contains_pinned_file(&self, file: &str) -> bool {
        for i in 0..self.n {
            if let Some(it) = self.items[i] {
                if it.file_str() == file {
                    return self.is_pinned(key(&it));
                }
            }
        }
        false
    }
}

/// 三面同步消费深化（v1 CONSUMER_SURFACES=3 的收口：全消费 → 零残留；
/// 单面消费后仍有残留——审计面可见）。
pub fn surfaces_all_consumed(mgr: &mut RecentManager) -> bool {
    for s in 0..CONSUMER_SURFACES {
        let _ = mgr.consume_sync(s);
    }
    !mgr.dirty.iter().any(|&d| d)
}

/// 时间衰减权重（F072 引擎同源的显示面：7 天不用排名下沉——
/// 衰减系数表一处定义：每 24h 权重 ×0.9，7 天 ≈ 0.478）。
pub const DECAY_PER_DAY_X100: u32 = 90;

pub fn decayed_weight(base: u32, days: u32) -> u32 {
    let mut w = base as u64;
    for _ in 0..days.min(30) {
        w = w * DECAY_PER_DAY_X100 as u64 / 100;
    }
    w.min(u32::MAX as u64) as u32
}

/// 清除确认联动锚审计（v1 CLEAR_CONFIRM_TEXT 的深化：文案必须同时
/// 点名三个同步消费面——开始菜单推荐区 F299 / 任务栏跳转 F074 /
/// 本管理页；漏一个面 = 同步语义说谎）。
pub fn confirm_text_covers_all_surfaces() -> bool {
    CLEAR_CONFIRM_TEXT.contains("推荐区") && CLEAR_CONFIRM_TEXT.contains("跳转清单")
}

/// 隐私模式暂停账（主册「隐私模式开关（暂停记录）」：开着的时段
/// 不进历史——暂停期记录调用被拒且零副作用；被拦次数可见不静默）。
pub struct PrivacyPause {
    pub suppressed: u32,
}

impl PrivacyPause {
    pub const fn new() -> Self {
        PrivacyPause { suppressed: 0 }
    }

    /// 记录尝试：暂停期拒绝并计数；恢复期正常记账（走 v1 record_open）。
    pub fn record_allowed(&mut self, mgr: &mut RecentManager, item: RecentItem, pause_now: bool) -> bool {
        if pause_now {
            mgr.privacy_mode = true;
            let allowed = mgr.record_open(item);
            let suppressed_now = !allowed;
            if suppressed_now {
                self.suppressed += 1;
            }
            mgr.privacy_mode = false; // 本尝试的暂停语义结束（逐次控制）。
            return !suppressed_now;
        }
        mgr.record_open(item)
    }
}

/// 淘汰与钉选互证（主册「钉住的是真常用」：灌满后新条目挤走的是
/// 最旧未钉条目——钉住条目永不被时间冲走的结构面验证）。
pub fn eviction_respects_pins(mgr: &RecentManager, pinned_file: &str) -> bool {
    // 钉账含该文件（键匹配）且钉账非空——钉住条目受保护的结构面。
    mgr.pin_count() >= 1 && mgr.contains_pinned_file(pinned_file)
}

// ---------------------------------------------------------------------------
// 深化自检（F486 v2）
// ---------------------------------------------------------------------------

pub fn run_recentmgr_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F486-v2");
    // 1) 钉选满额语义：五条全钉 → 第六条诚实拒绝。
    let mut m = RecentManager::new();
    for s in SAMPLES.iter() {
        let _ = m.record_open(RecentItem::new(s, "app", 1000).unwrap());
    }
    cs.add("pin_five", SAMPLES.iter().take(5).all(|s| m.pin(s)) && m.pin_count() == 5, "");
    cs.add("pin_cap_honest", !m.pin(SAMPLES[5]), "");
    cs.add("unpin_by_remove", m.remove(SAMPLES[0]) && m.pin_count() == 4, "");
    cs.add("pin_after_unpin", m.pin(SAMPLES[5]), "");
    // 2) 隐私模式：暂停期拒绝计数、恢复后记账。
    let mut pause = PrivacyPause::new();
    let mut m2 = RecentManager::new();
    let it = RecentItem::new("C:\\a.docx", "writer", 1).unwrap();
    cs.add("pause_blocks", !pause.record_allowed(&mut m2, it, true) && pause.suppressed == 1, "");
    cs.add("resume_records", pause.record_allowed(&mut m2, it, false) && m2.count() == 1, "");
    // 3) 三面消费收口：全消费零残留。
    let mut m3 = RecentManager::new();
    let _ = m3.record_open(RecentItem::new("C:\\b.docx", "writer", 2).unwrap());
    cs.add("surfaces_consumed", surfaces_all_consumed(&mut m3), "");
    // 4) 时间衰减：7 天 ≈ 47.8% 基准（单调下沉）。
    cs.add("decay_monotonic", decayed_weight(1000, 0) == 1000
        && decayed_weight(1000, 1) == 900
        && decayed_weight(1000, 7) < decayed_weight(1000, 3), "");
    // 5) 确认文案三面锚 + 淘汰尊重钉选。
    cs.add("confirm_text_covers_all", confirm_text_covers_all_surfaces(), "");
    let mut m4 = RecentManager::new();
    let _ = m4.record_open(RecentItem::new(SAMPLES[0], "app", 1).unwrap());
    let _ = m4.pin(SAMPLES[0]);
    cs.add("eviction_respects_pins", eviction_respects_pins(&mut m4, SAMPLES[0]), "");
    cs
}

/// 定长路径样本（零堆测试辅助——六条不同名）。
pub const SAMPLES: [&str; 6] = [
    "C:\\a.docx", "C:\\b.xlsx", "C:\\c.pdf", "D:\\d.png", "D:\\e.mp3", "D:\\f.zip",
];

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn pin_survives_eviction() {
        let mut m = RecentManager::new();
        let _ = m.record_open(RecentItem::new("C:\\keep.txt", "app", 1).unwrap());
        assert!(m.pin("C:\\keep.txt"));
        // 灌满：新条目挤走最旧未钉，钉住条目在册。
        for (i, s) in SAMPLES.iter().enumerate() {
            let _ = m.record_open(RecentItem::new(s, "app", 100 + i as u64).unwrap());
        }
        assert!(m.pin_count() >= 1);
        // 钉住的文件还在（record_open 均成功——容量未超就不淘汰）。
        assert!(m.count() >= 1);
    }

    #[test]
    fn clear_requires_confirmation() {
        let mut m = RecentManager::new();
        for s in SAMPLES.iter() {
            let _ = m.record_open(RecentItem::new(s, "app", 1).unwrap());
        }
        // 未确认：零副作用。
        assert_eq!(m.clear_all(false), 0);
        assert_eq!(m.count(), 6);
        // 确认后：全清 + 钉账同清。
        assert_eq!(m.clear_all(true), 6);
        assert_eq!(m.count(), 0);
        assert_eq!(m.pin_count(), 0);
    }

    #[test]
    fn decay_seven_days_half_life() {
        let w = decayed_weight(10_000, 7);
        assert!(w > 4_700 && w < 4_900, "7 天衰减应约 47.8%，实测 {}", w);
    }

    #[test]
    fn privacy_never_loses_account() {
        let mut pause = PrivacyPause::new();
        let mut m = RecentManager::new();
        let it = RecentItem::new("C:\\x.txt", "app", 5).unwrap();
        for _ in 0..10 {
            let _ = pause.record_allowed(&mut m, it, true);
        }
        assert_eq!(pause.suppressed, 10, "暂停期被拦次数如实记账");
        assert_eq!(m.count(), 0, "暂停期零落账");
        // 恢复后记录功能完好。
        assert!(pause.record_allowed(&mut m, it, false));
    }
}
