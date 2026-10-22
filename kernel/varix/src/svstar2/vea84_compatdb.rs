//! VE-F0084 · 应用兼容档案查询服务（VE-A 域 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0084`
//!
//! **职责定位（锚点原文）**：应用渲染兼容档案的**服务化查询**（按应用查历史
//! 兼容结论），档案数据来源**透明**（实测 + 用户反馈双源），档案**置信度标注**。
//!
//! **判据（锚点原文）**：档案服务、双源仲裁、置信标注、缓存兜底、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 数据冲突 → **双源仲裁** | 实测与反馈给出不同结论时，[`arbitrate`] 取**实测**并把 `conflicted` 置真、置信度压到 [`CONFLICT_SCORE`]。取反馈会让「一个人随手点了差评」覆盖整批实测；取平均则两边都不对 |
//! | 置信虚标 → **实测校准** | 置信度由**来源构成**算出来，不是调用方填的：实测 [`MEASURED_SCORE`] > 反馈 [`FEEDBACK_SCORE`] > 冲突 [`CONFLICT_SCORE`]，双源一致 [`AGREED_SCORE`]，且上限 [`MAX_SCORE`] 永远小于 100——「永远可能有新驱动」 |
//! | 查询超时 → **缓存** | [`CompatDb::query_before`] 在超过 deadline 时退回缓存（**即使已过期**），并把 `stale` 标出来。过期数据配「不新鲜」标记，好过没有数据配「查不到」 |
//!
//! **性能逐项分解（锚点原文）**：O(查询)。档案按应用指纹线性查找，上限 32；
//! 缓存 16 槽线性，命中即返回。无分配（除建档案时的 `Vec` 增长）。
//!
//! **跨批对接点**：A83 名单消费——档案结论可与名单裁决交叉校验，
//! 但本模块**不读名单**（各自单一职责）。
//!
//! **无障碍与隐私（锚点原文）**：档案读屏可达（[`Answer::a11y_line`]）——
//! 只报结论、置信度与来源数；应用以**指纹**标识，不含应用名、路径、窗口标题。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **应用用指纹不用名字**：名字是用户内容，指纹是哈希。查询服务把名字
//!   写进日志，等于把「用户装了什么」记下来。
//! - **置信度是算出来的**：调用方填置信度必然虚标；来源构成是客观事实。
//! - **冲突不是「取最新」**：最新一条反馈可能刚被刷，实测样本更稳。
//! - **零 panic 面**：无 `unwrap`、索引经 `get`/`min`。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA7FD/E 与 0xA80D/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 档案表已满。
pub const E_PROFILE_FULL: u16 = 0xA7FD;
/// 观测记录非法（时刻为零，或结论写成 Unknown——Unknown 是「没档案」，
/// 不是一条可登记的观测）。
pub const E_BAD_OBSERVATION: u16 = 0xA7FE;

// ---------------------------------------------------------------------------
// 二、容量与分值
// ---------------------------------------------------------------------------

pub const MAX_PROFILES: usize = 32;
pub const MAX_OBS: usize = 6;
pub const CACHE_SLOTS: usize = 16;
/// 缓存有效期（秒/tick）。
pub const CACHE_TTL: u64 = 300;

/// 双源一致时的置信度。
pub const AGREED_SCORE: u8 = 95;
/// 仅实测。
pub const MEASURED_SCORE: u8 = 70;
/// 仅反馈。
pub const FEEDBACK_SCORE: u8 = 40;
/// 双源冲突（实测优先，但不确定）。
pub const CONFLICT_SCORE: u8 = 55;
/// 置信度硬上限。永不取 100。
pub const MAX_SCORE: u8 = 95;

// ---------------------------------------------------------------------------
// 三、来源与结论
// ---------------------------------------------------------------------------

/// 数据来源。两者的可信度不对称——这是整个模块的核心假设。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// 本机实测。
    Measured,
    /// 用户反馈。
    Feedback,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Measured => "measured",
            Source::Feedback => "feedback",
        }
    }

    /// 可信权重：实测 3、反馈 1。
    pub fn weight(self) -> u8 {
        match self {
            Source::Measured => 3,
            Source::Feedback => 1,
        }
    }
}

/// 兼容结论。四值封闭：没有「大概能用」这种中间态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compat {
    /// 实测/反馈均无问题。
    Green,
    /// 有已知问题但可用。
    Yellow,
    /// 已知不可用。
    Red,
    /// 无档案。
    Unknown,
}

impl Compat {
    pub fn as_str(self) -> &'static str {
        match self {
            Compat::Green => "green",
            Compat::Yellow => "yellow",
            Compat::Red => "red",
            Compat::Unknown => "unknown",
        }
    }

    pub fn known(self) -> bool {
        !matches!(self, Compat::Unknown)
    }
}

/// 一条观测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Observation {
    pub source: Source,
    pub verdict: Compat,
    pub at: u64,
}

impl Observation {
    pub fn valid(&self) -> bool {
        self.at > 0 && self.verdict.known()
    }
}

// ---------------------------------------------------------------------------
// 四、档案
// ---------------------------------------------------------------------------

/// 一个应用的档案。观测按时间追加，满则挤掉最旧。
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    /// 应用指纹（哈希），不含名字。
    pub key: u32,
    obs: [Option<Observation>; MAX_OBS],
    n: usize,
}

impl Profile {
    pub const fn new(key: u32) -> Profile {
        Profile {
            key,
            obs: [None; MAX_OBS],
            n: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn observations(&self) -> &[Option<Observation>; MAX_OBS] {
        &self.obs
    }

    /// 追加观测；满则整体左移一格（丢最旧）。
    pub fn push(&mut self, o: Observation) {
        if self.n < MAX_OBS {
            self.obs[self.n] = Some(o);
            self.n += 1;
            return;
        }
        for i in 1..MAX_OBS {
            self.obs[i - 1] = self.obs[i];
        }
        self.obs[MAX_OBS - 1] = Some(o);
    }

    /// 某来源的最新一条。
    pub fn latest(&self, source: Source) -> Option<Observation> {
        let mut best: Option<Observation> = None;
        for slot in self.obs.iter().flatten() {
            if slot.source != source {
                continue;
            }
            match best {
                Some(b) if b.at >= slot.at => {}
                _ => best = Some(*slot),
            }
        }
        best
    }
}

/// 双源仲裁结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Judgement {
    pub verdict: Compat,
    pub score: u8,
    pub conflicted: bool,
    /// 参与仲裁的来源数（0/1/2）。
    pub sources: u8,
}

/// 按来源构成算结论与置信度。
///
/// 实测与反馈都给结论且不一致时取实测——同时把 `conflicted` 标出来，
/// 让上层知道「这份档案内部有分歧」。
pub fn arbitrate(p: &Profile) -> Judgement {
    let m = p.latest(Source::Measured);
    let f = p.latest(Source::Feedback);
    match (m, f) {
        (Some(m), Some(f)) => {
            if m.verdict == f.verdict {
                Judgement {
                    verdict: m.verdict,
                    score: AGREED_SCORE,
                    conflicted: false,
                    sources: 2,
                }
            } else {
                Judgement {
                    verdict: m.verdict,
                    score: CONFLICT_SCORE,
                    conflicted: true,
                    sources: 2,
                }
            }
        }
        (Some(m), None) => Judgement {
            verdict: m.verdict,
            score: MEASURED_SCORE,
            conflicted: false,
            sources: 1,
        },
        (None, Some(f)) => Judgement {
            verdict: f.verdict,
            score: FEEDBACK_SCORE,
            conflicted: false,
            sources: 1,
        },
        (None, None) => Judgement {
            verdict: Compat::Unknown,
            score: 0,
            conflicted: false,
            sources: 0,
        },
    }
}

// ---------------------------------------------------------------------------
// 五、缓存
// ---------------------------------------------------------------------------

/// 一个缓存槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheSlot {
    pub key: u32,
    pub verdict: Compat,
    pub score: u8,
    pub at: u64,
    pub valid: bool,
}

impl CacheSlot {
    pub const fn empty() -> CacheSlot {
        CacheSlot {
            key: 0,
            verdict: Compat::Unknown,
            score: 0,
            at: 0,
            valid: false,
        }
    }

    pub fn fresh(&self, now: u64) -> bool {
        self.valid && now.saturating_sub(self.at) <= CACHE_TTL
    }
}

/// 一次查询的答案。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Answer {
    pub verdict: Compat,
    pub score: u8,
    pub conflicted: bool,
    pub from_cache: bool,
    /// 缓存已过期但仍被使用（超时兜底）。
    pub stale: bool,
}

impl Answer {
    pub fn unknown() -> Answer {
        Answer {
            verdict: Compat::Unknown,
            score: 0,
            conflicted: false,
            from_cache: false,
            stale: false,
        }
    }

    /// 读屏一行。
    pub fn a11y_line(&self) -> String {
        format!(
            "兼容结论 {}，置信度 {}，{}",
            self.verdict.as_str(),
            self.score,
            if self.from_cache {
                if self.stale {
                    "来自过期缓存"
                } else {
                    "来自缓存"
                }
            } else {
                "来自档案"
            }
        )
    }
}

// ---------------------------------------------------------------------------
// 六、服务
// ---------------------------------------------------------------------------

/// 档案服务。
pub struct CompatDb {
    profiles: Vec<Profile>,
    cache: [CacheSlot; CACHE_SLOTS],
    cache_head: usize,
    queries: u32,
    hits: u32,
    misses: u32,
    stale_served: u32,
    conflicts: u32,
}

impl CompatDb {
    pub fn new() -> CompatDb {
        CompatDb {
            profiles: Vec::new(),
            cache: [CacheSlot::empty(); CACHE_SLOTS],
            cache_head: 0,
            queries: 0,
            hits: 0,
            misses: 0,
            stale_served: 0,
            conflicts: 0,
        }
    }

    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    pub fn stats(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.queries,
            self.hits,
            self.misses,
            self.stale_served,
            self.conflicts,
        )
    }

    /// 登记一条观测。档案不存在则新建；表满拒绝。
    pub fn observe(&mut self, key: u32, o: Observation) -> Result<(), u16> {
        if !o.valid() {
            return Err(E_BAD_OBSERVATION);
        }
        if let Some(p) = self.profiles.iter_mut().find(|p| p.key == key) {
            p.push(o);
            return Ok(());
        }
        if self.profiles.len() >= MAX_PROFILES {
            return Err(E_PROFILE_FULL);
        }
        let mut p = Profile::new(key);
        p.push(o);
        self.profiles.push(p);
        Ok(())
    }

    pub fn profile(&self, key: u32) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.key == key)
    }

    fn cache_get(&self, key: u32, now: u64) -> Option<CacheSlot> {
        self.cache
            .iter()
            .copied()
            .find(|s| s.valid && s.key == key && s.fresh(now))
    }

    fn cache_any(&self, key: u32) -> Option<CacheSlot> {
        self.cache.iter().copied().find(|s| s.valid && s.key == key)
    }

    fn cache_put(&mut self, key: u32, j: Judgement, now: u64) {
        let slot = CacheSlot {
            key,
            verdict: j.verdict,
            score: j.score,
            at: now,
            valid: true,
        };
        if let Some(s) = self.cache.iter_mut().find(|s| s.valid && s.key == key) {
            *s = slot;
            return;
        }
        self.cache[self.cache_head] = slot;
        self.cache_head = (self.cache_head + 1) % CACHE_SLOTS;
    }

    /// 查询。缓存优先，未过期即返回。
    pub fn query(&mut self, key: u32, now: u64) -> Answer {
        self.queries = self.queries.saturating_add(1);
        if let Some(s) = self.cache_get(key, now) {
            self.hits = self.hits.saturating_add(1);
            return Answer {
                verdict: s.verdict,
                score: s.score,
                conflicted: false,
                from_cache: true,
                stale: false,
            };
        }
        self.misses = self.misses.saturating_add(1);
        match self.profile(key) {
            Some(p) => {
                let j = arbitrate(p);
                if j.conflicted {
                    self.conflicts = self.conflicts.saturating_add(1);
                }
                self.cache_put(key, j, now);
                Answer {
                    verdict: j.verdict,
                    score: j.score,
                    conflicted: j.conflicted,
                    from_cache: false,
                    stale: false,
                }
            }
            None => Answer::unknown(),
        }
    }

    /// 带截止时刻的查询：超时则退回缓存（**过期也退**），并标 stale。
    ///
    /// `deadline == 0` 表示不限时，等价于 [`CompatDb::query`]。`deadline`
    /// 是**绝对时刻**（与 `now` 同一时基），不是「还能等多久」——两者混用
    /// 会让调用方传进来的 300 被当成某个远古时刻，每次查询都走兜底。
    pub fn query_before(&mut self, key: u32, now: u64, deadline: u64) -> Answer {
        if deadline != 0 && now > deadline {
            self.queries = self.queries.saturating_add(1);
            if let Some(s) = self.cache_any(key) {
                self.stale_served = self.stale_served.saturating_add(1);
                return Answer {
                    verdict: s.verdict,
                    score: s.score,
                    conflicted: false,
                    from_cache: true,
                    stale: !s.fresh(now),
                };
            }
            // 没有缓存可用：仍然走正常路径，把 Unknown 如实交出去。
            return self.query(key, now);
        }
        self.query(key, now)
    }

    /// 读屏。
    pub fn a11y_lines(&self) -> Vec<String> {
        let (q, h, m, st, cf) = self.stats();
        let mut out = Vec::new();
        out.push(format!(
            "兼容档案 {} 份 / {} profiles",
            self.profiles.len(),
            self.profiles.len()
        ));
        out.push(format!(
            "查询 {} 次，命中 {}，未命中 {} / {} queries, {} hits, {} misses",
            q, h, m, q, h, m
        ));
        out.push(format!(
            "过期兜底 {} 次，冲突档案 {} 次 / {} stale, {} conflicts",
            st, cf, st, cf
        ));
        out
    }
}

impl Default for CompatDb {
    fn default() -> CompatDb {
        CompatDb::new()
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

fn obs(source: Source, v: Compat, at: u64) -> Observation {
    Observation {
        source,
        verdict: v,
        at,
    }
}

/// VE-F0084 的十四条判据。
pub fn run_vea84_checks() -> CheckSet {
    let mut c = CheckSet::new("vea84");

    // 1. 无档案返回 Unknown。
    let mut db = CompatDb::new();
    if db.query(0xAA, 1).verdict == Compat::Unknown {
        c.ok("A84-查询-无档案即未知");
    } else {
        c.fail("A84-查询-无档案即未知", "unknown profile lied");
    }

    // 2. 仅实测：结论取实测，置信度 70。
    let _ = db.observe(0xAA, obs(Source::Measured, Compat::Green, 10));
    let a = db.query(0xAA, 11);
    if a.verdict == Compat::Green && a.score == MEASURED_SCORE && !a.conflicted {
        c.ok("A84-仲裁-仅实测");
    } else {
        c.fail("A84-仲裁-仅实测", "measured-only wrong");
    }

    // 3. 仅反馈：置信度低于实测。
    let _ = db.observe(0xBB, obs(Source::Feedback, Compat::Yellow, 10));
    let b = db.query(0xBB, 11);
    if b.verdict == Compat::Yellow && b.score == FEEDBACK_SCORE && b.score < MEASURED_SCORE {
        c.ok("A84-仲裁-仅反馈低于实测");
    } else {
        c.fail("A84-仲裁-仅反馈低于实测", "feedback weighting wrong");
    }

    // 4. 双源一致：置信度最高。
    let _ = db.observe(0xCC, obs(Source::Measured, Compat::Green, 10));
    let _ = db.observe(0xCC, obs(Source::Feedback, Compat::Green, 20));
    let c1 = db.query(0xCC, 21);
    if c1.verdict == Compat::Green && c1.score == AGREED_SCORE && !c1.conflicted {
        c.ok("A84-仲裁-双源一致最高");
    } else {
        c.fail("A84-仲裁-双源一致最高", "agreement scoring wrong");
    }

    // 5. 双源冲突：取实测 + 标冲突。
    let _ = db.observe(0xDD, obs(Source::Measured, Compat::Red, 10));
    let _ = db.observe(0xDD, obs(Source::Feedback, Compat::Green, 20));
    let d = db.query(0xDD, 21);
    if d.verdict == Compat::Red && d.conflicted && d.score == CONFLICT_SCORE {
        c.ok("A84-仲裁-冲突取实测");
    } else {
        c.fail("A84-仲裁-冲突取实测", "conflict resolution wrong");
    }

    // 6. 置信度上限永不为 100。
    if AGREED_SCORE <= MAX_SCORE && MAX_SCORE < 100 && CONFLICT_SCORE < AGREED_SCORE {
        c.ok("A84-置信-上限小于百");
    } else {
        c.fail("A84-置信-上限小于百", "confidence ceiling wrong");
    }

    // 7. 缓存命中：同 key 短时二次查询走缓存。
    let a2 = db.query(0xCC, 22);
    if a2.from_cache {
        c.ok("A84-缓存-短时命中");
    } else {
        c.fail("A84-缓存-短时命中", "cache miss on second query");
    }

    // 8. 缓存过期后重新查档案。
    let a3 = db.query(0xCC, 22 + CACHE_TTL + 1);
    if !a3.from_cache {
        c.ok("A84-缓存-过期即重查");
    } else {
        c.fail("A84-缓存-过期即重查", "stale cache reused silently");
    }

    // 9. 超时兜底：过期缓存配 stale 标记。
    let mut db2 = CompatDb::new();
    let _ = db2.observe(0x11, obs(Source::Measured, Compat::Yellow, 1));
    let _ = db2.query(0x11, 2);
    let late = db2.query_before(0x11, 10_000, 5);
    if late.from_cache && late.stale {
        c.ok("A84-缓存-超时用过期并标注");
    } else {
        c.fail("A84-缓存-超时用过期并标注", "deadline fallback wrong");
    }

    // 10. 超时且无缓存：如实返回 Unknown，不伪造。
    let none = db2.query_before(0x22, 20_000, 5);
    if none.verdict == Compat::Unknown && !none.from_cache {
        c.ok("A84-缓存-无缓存不伪造");
    } else {
        c.fail("A84-缓存-无缓存不伪造", "fabricated answer on deadline");
    }

    // 11. 非法观测即拒。
    if db.observe(0xEE, obs(Source::Measured, Compat::Green, 0)) == Err(E_BAD_OBSERVATION)
        && db.observe(0xEE, obs(Source::Measured, Compat::Unknown, 5)) == Err(E_BAD_OBSERVATION)
    {
        c.ok("A84-观测-非法即拒");
    } else {
        c.fail("A84-观测-非法即拒", "invalid observation accepted");
    }

    // 12. 档案满即拒（新 key）。
    let mut full = CompatDb::new();
    for i in 0..MAX_PROFILES {
        let _ = full.observe(0x1000 + i as u32, obs(Source::Measured, Compat::Green, 1));
    }
    if full.observe(0x9999, obs(Source::Measured, Compat::Green, 1)) == Err(E_PROFILE_FULL) {
        c.ok("A84-档案-满即拒");
    } else {
        c.fail("A84-档案-满即拒", "profile overflow accepted");
    }

    // 13. 观测环形：满后丢最旧，只留 MAX_OBS 条。
    let mut ring = Profile::new(1);
    for i in 0..(MAX_OBS as u64 + 2) {
        ring.push(obs(Source::Measured, Compat::Green, i + 1));
    }
    let first = ring.observations()[0].map(|o| o.at);
    if ring.len() == MAX_OBS && first == Some(3) {
        c.ok("A84-档案-观测环形保新");
    } else {
        c.fail("A84-档案-观测环形保新", "observation ring wrong");
    }

    // 14. 读屏三行且含档案数。
    let lines = db.a11y_lines();
    if lines.len() == 3 && lines[0].contains(&db.profile_count().to_string()) {
        c.ok("A84-读屏-状态可播");
    } else {
        c.fail("A84-读屏-状态可播", "a11y incomplete");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_weights_are_asymmetric() {
        assert!(Source::Measured.weight() > Source::Feedback.weight());
    }

    #[test]
    fn empty_profile_is_unknown() {
        let p = Profile::new(1);
        let j = arbitrate(&p);
        assert_eq!(j.verdict, Compat::Unknown);
        assert_eq!(j.sources, 0);
    }

    #[test]
    fn latest_picks_newest_per_source() {
        let mut p = Profile::new(1);
        p.push(obs(Source::Feedback, Compat::Yellow, 5));
        p.push(obs(Source::Feedback, Compat::Green, 9));
        assert_eq!(p.latest(Source::Feedback).map(|o| o.at), Some(9));
    }

    #[test]
    fn ring_keeps_newest() {
        let mut p = Profile::new(1);
        for i in 0..(MAX_OBS as u64 + 1) {
            p.push(obs(Source::Measured, Compat::Green, i + 1));
        }
        assert_eq!(p.len(), MAX_OBS);
        assert_eq!(p.observations()[0].map(|o| o.at), Some(2));
    }

    #[test]
    fn cache_slot_freshness() {
        let s = CacheSlot {
            key: 1,
            verdict: Compat::Green,
            score: 90,
            at: 100,
            valid: true,
        };
        assert!(s.fresh(100));
        assert!(s.fresh(100 + CACHE_TTL));
        assert!(!s.fresh(100 + CACHE_TTL + 1));
    }

    #[test]
    fn answer_line_mentions_source() {
        let a = Answer {
            verdict: Compat::Yellow,
            score: 70,
            conflicted: false,
            from_cache: true,
            stale: true,
        };
        assert!(a.a11y_line().contains("过期"));
    }

    #[test]
    fn query_counts_move() {
        let mut db = CompatDb::new();
        let _ = db.observe(7, obs(Source::Measured, Compat::Green, 1));
        let _ = db.query(7, 2);
        let _ = db.query(7, 3);
        let (q, h, m, _, _) = db.stats();
        assert_eq!(q, 2);
        assert_eq!(h, 1);
        assert_eq!(m, 1);
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea84_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea84 red: {}", cs.red_items_text());
        assert!(p >= 14);
    }
}
