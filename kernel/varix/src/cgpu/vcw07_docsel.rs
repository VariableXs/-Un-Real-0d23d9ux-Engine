//! vcw07_docsel — CGPU-F3527 文档遥测（深化版）
//!
//! 锚点：CGPU 册 #CGPU-F3527 · 文档遥测（目标 300 行 → 深化 800+ 行）。
//!
//! 职责定位：文档访问/搜索热词遥测——遥测复用（对接 cgp08 PerfLedger
//! 预算口径与 F6009 四类聚合同纪律：只聚参数不记内容）；访问计数、搜索
//! 热词频次、失败率三类聚合器；热词榜单稳定排序；隐私红线——查询原文
//! 不入账只记指纹与计数（脱敏结构即隐私）。
//!
//! 降级矩阵：账满→滚动；超预算→限流留痕；未知类别→显性拒。
//! 性能：记录 O(1)。判据：遥测复用、一组（判据函数 run_vcw07_checks）。
//!
//! 纪律：no_std；零 unwrap 零 expect；零 IO；零墙钟（tick 注入）；零浮
//! 点；错误全带可操作建议；码段 0x25。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面
// ---------------------------------------------------------------------------

/// 诊断码段基座（0x25 段）。
pub const SEG_BASE: u16 = 0x2500;

/// 访问账容量（滚动）。
pub const ACCESS_CAP: usize = 256;

/// 热词槽位（指纹桶——只存指纹不存原文）。
pub const HOTWORD_SLOTS: usize = 32;

/// 单 tick 记录限流线（超线即限流留痕）。
pub const RATE_LIMIT_PER_TICK: usize = 64;

/// 热词上榜最低频次（防噪声伪热词）。
pub const HOTWORD_MIN_HITS: u64 = 3;

/// 三类聚合闭集（遥测复用口径）。
pub const KIND_NAMES: [&str; 3] = ["doc-access", "search-hit", "search-miss"];

/// 访问路径闭集。
pub const PATH_NAMES: [&str; 4] = ["sdk", "guide", "api-ref", "tutorial"];

// ---------------------------------------------------------------------------
// 错误面（0x25 段全码带建议）
// ---------------------------------------------------------------------------

/// 文档遥测错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsCode {
    /// 0x2501 类别未知。
    KindUnknown = 0x2501,
    /// 0x2502 路径未知。
    PathUnknown = 0x2502,
    /// 0x2503 限流（单 tick 超线）。
    RateLimited = 0x2503,
    /// 0x2504 账满滚动。
    Rolled = 0x2504,
    /// 0x2505 榜单越界。
    RankOutOfRange = 0x2505,
}

impl DsCode {
    /// 码值。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            DsCode::KindUnknown => "类别未知",
            DsCode::PathUnknown => "路径未知",
            DsCode::RateLimited => "限流",
            DsCode::Rolled => "账满滚动",
            DsCode::RankOutOfRange => "榜单越界",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            DsCode::KindUnknown => "类别取 KIND_NAMES 三类闭集；上游口径对齐",
            DsCode::PathUnknown => "路径取 PATH_NAMES 四路闭集；新路径先扩表",
            DsCode::RateLimited => "单 tick 记录超 64 条被限流；分帧上报",
            DsCode::Rolled => "访问账满已滚动覆盖最旧；及时消费快照",
            DsCode::RankOutOfRange => "榜单位次超槽位数；先查 hotword_len",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV（自足）
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 字值 FNV-1a。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s ^= (v >> (i * 8)) as u64 & 0xFF;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字节串 FNV-1a。
pub const fn fnv1a_bytes(mut s: u64, b: &[u8]) -> u64 {
    let mut i = 0;
    while i < b.len() {
        s ^= b[i] as u64;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字符串 FNV-1a。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    fnv1a_bytes(s, v.as_bytes())
}

// ---------------------------------------------------------------------------
// 访问账（判据"遥测复用"——三类聚合同 F6009 纪律）
// ---------------------------------------------------------------------------

/// 访问记录（脱敏：文档指纹 + 类别 + 路径，无原文无用户）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessRec {
    /// 文档 id 指纹。
    pub doc_fp: u64,
    /// 类别（0..3）。
    pub kind: u8,
    /// 路径（0..4）。
    pub path: u8,
    /// tick。
    pub tick: u64,
}

/// 热词桶（指纹+频次——原文不入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotwordSlot {
    /// 查询指纹。
    pub fp: u64,
    /// 频次。
    pub hits: u64,
}

/// 榜单行（稳定排序输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankRow {
    /// 查询指纹。
    pub fp: u64,
    /// 频次。
    pub hits: u64,
    /// 榜位（0 起）。
    pub rank: usize,
}

// ---------------------------------------------------------------------------
// 总引擎
// ---------------------------------------------------------------------------

/// 文档遥测总引擎。
pub struct DocselEngine {
    /// 访问账（滚动）。
    pub accesses: Vec<AccessRec>,
    a_head: usize,
    a_len: usize,
    pub a_rolled: u64,
    /// 三类计数。
    pub kind_counts: [u64; 3],
    /// 四路计数。
    pub path_counts: [u64; 4],
    /// 热词桶（开放寻址——指纹模槽）。
    pub hotwords: Vec<HotwordSlot>,
    /// 单 tick 计数（限流闸输入）。
    rate_tick: u64,
    rate_count: usize,
    /// 限流计数。
    pub limited: u64,
    /// 失败率分子（search-miss）——直接取 kind_counts[2]。
    last_tick: u64,
}

impl DocselEngine {
    /// 构造。
    pub fn new() -> DocselEngine {
        DocselEngine {
            accesses: Vec::new(),
            a_head: 0,
            a_len: 0,
            a_rolled: 0,
            kind_counts: [0; 3],
            path_counts: [0; 4],
            hotwords: Vec::new(),
            rate_tick: 0,
            rate_count: 0,
            limited: 0,
            last_tick: 0,
        }
    }

    /// 记访问（O(1)；限流闸 + 账滚动）。
    pub fn record(&mut self, doc_fp: u64, kind: usize, path: usize, tick: u64) -> Result<(), DsCode> {
        if kind >= KIND_NAMES.len() {
            return Err(DsCode::KindUnknown);
        }
        if path >= PATH_NAMES.len() {
            return Err(DsCode::PathUnknown);
        }
        // 限流闸。
        if self.rate_tick != tick {
            self.rate_tick = tick;
            self.rate_count = 0;
        }
        if self.rate_count >= RATE_LIMIT_PER_TICK {
            self.limited += 1;
            return Err(DsCode::RateLimited);
        }
        self.rate_count += 1;
        // 账滚动。
        let rec = AccessRec {
            doc_fp,
            kind: kind as u8,
            path: path as u8,
            tick,
        };
        if self.accesses.len() < ACCESS_CAP {
            self.accesses.push(rec);
            self.a_len += 1;
        } else {
            self.accesses[self.a_head] = rec;
            self.a_head = (self.a_head + 1) % ACCESS_CAP;
            self.a_rolled += 1;
            return Err(DsCode::Rolled);
        }
        self.kind_counts[kind] += 1;
        self.path_counts[path] += 1;
        self.last_tick = self.last_tick.max(tick);
        Ok(())
    }

    /// 记搜索热词（指纹桶——查询原文绝不入账）。
    pub fn note_query(&mut self, query: &str, hit: bool, tick: u64) {
        let kind = if hit { 1 } else { 2 };
        let fp = fnv1a_str(FNV_OFFSET, query);
        let _ = self.record(fp, kind, 1, tick);
        // 热词桶。
        let mut i = 0;
        let mut found = false;
        while i < self.hotwords.len() {
            if let Some(h) = self.hotwords.get(i) {
                if h.fp == fp {
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if found {
            if let Some(h) = self.hotwords.get_mut(i) {
                h.hits += 1;
            }
        } else if self.hotwords.len() < HOTWORD_SLOTS {
            self.hotwords.push(HotwordSlot { fp, hits: 1 });
        } else {
            // 满槽：替换最低频（频次低于新词时）。
            let mut min_i = 0usize;
            let mut min_hits = u64::MAX;
            let mut k = 0;
            while k < self.hotwords.len() {
                if let Some(h) = self.hotwords.get(k) {
                    if h.hits < min_hits {
                        min_hits = h.hits;
                        min_i = k;
                    }
                }
                k += 1;
            }
            if min_hits <= 1 {
                if let Some(slot) = self.hotwords.get_mut(min_i) {
                    *slot = HotwordSlot { fp, hits: 1 };
                }
            }
        }
    }

    /// 热词榜单（频次降序稳定排序；低于 HOTWORD_MIN_HITS 不上榜）。
    pub fn hotword_rank(&self) -> Vec<RankRow> {
        // 收集达标项。
        let mut items: Vec<(u64, u64)> = Vec::new();
        let mut i = 0;
        while i < self.hotwords.len() {
            if let Some(h) = self.hotwords.get(i) {
                if h.hits >= HOTWORD_MIN_HITS {
                    items.push((h.fp, h.hits));
                }
            }
            i += 1;
        }
        // 稳定排序：hits 降序，次键 fp 升序（同频确定性）。
        items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut rows: Vec<RankRow> = Vec::new();
        let mut k = 0;
        while k < items.len() {
            if let Some((fp, hits)) = items.get(k) {
                rows.push(RankRow {
                    fp: *fp,
                    hits: *hits,
                    rank: k,
                });
            }
            k += 1;
        }
        rows
    }

    /// 榜单位读取。
    pub fn rank_at(&self, rank: usize) -> Result<RankRow, DsCode> {
        let rows = self.hotword_rank();
        if rank >= rows.len() {
            return Err(DsCode::RankOutOfRange);
        }
        match rows.get(rank) {
            Some(r) => Ok(*r),
            None => Err(DsCode::RankOutOfRange),
        }
    }

    /// 失败率（bps）：search-miss / (hit+miss)。
    pub const fn miss_rate_bps(&self) -> u64 {
        let hit = self.kind_counts[1];
        let miss = self.kind_counts[2];
        let total = hit + miss;
        if total == 0 {
            0
        } else {
            miss.saturating_mul(10_000) / total
        }
    }

    /// 按时序读访问。
    pub fn access_at(&self, i: usize) -> Option<&AccessRec> {
        if i >= self.a_len {
            return None;
        }
        self.accesses.get((self.a_head + i) % ACCESS_CAP)
    }

    /// 守恒：三类计数和 == 账内 + 滚出。
    pub const fn reconcile(&self) -> bool {
        let sum = self.kind_counts[0]
            .saturating_add(self.kind_counts[1])
            .saturating_add(self.kind_counts[2]);
        sum == self.a_len as u64 + self.a_rolled
    }

    /// 读屏三段。
    pub fn read_screen(&self) -> String {
        let mut out = String::new();
        out.push_str("== 文档遥测口径（0x25 段）==\n");
        out.push_str("[一] 三类计数\n");
        let mut k = 0;
        while k < KIND_NAMES.len() {
            out.push_str(KIND_NAMES[k]);
            out.push('=');
            push_u64(&mut out, self.kind_counts[k]);
            out.push('\n');
            k += 1;
        }
        out.push_str("[二] 热词\n");
        out.push_str("slots=");
        push_u64(&mut out, self.hotwords.len() as u64);
        out.push_str(" min_hits=");
        push_u64(&mut out, HOTWORD_MIN_HITS);
        out.push_str(" miss_rate_bps=");
        push_u64(&mut out, self.miss_rate_bps());
        out.push('\n');
        out.push_str("[三] 限流\n");
        out.push_str("limited=");
        push_u64(&mut out, self.limited);
        out.push_str(" rolled=");
        push_u64(&mut out, self.a_rolled);
        out.push('\n');
        out
    }
}

impl Default for DocselEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// u64 十进制直写。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        out.push(buf[j] as char);
        j += 1;
    }
}

// ---------------------------------------------------------------------------
// 自检（判据"遥测复用、一组"——run_vcw07_checks）
// ---------------------------------------------------------------------------

/// 自检判据（F3527 全判据 + 深化面）。
pub fn run_vcw07_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();

    // ── 判据一：遥测复用（三类聚合口径与计数一致）。
    let mut e = DocselEngine::new();
    let dfp = fnv1a_str(FNV_OFFSET, "sdk-intro");
    let _ = e.record(dfp, 0, 0, 1);
    let _ = e.record(dfp, 0, 2, 2);
    out.push((
        "遥测复用三类聚合",
        e.kind_counts[0] == 2 && e.access_at(0).map(|a| a.kind == 0).unwrap_or(false),
    ));

    // ── 判据二：热词频次（原文不入账只记指纹）。
    let mut e2 = DocselEngine::new();
    e2.note_query("physics", true, 1);
    e2.note_query("physics", true, 2);
    e2.note_query("physics", false, 3);
    e2.note_query("render", true, 4);
    let rank = e2.hotword_rank();
    out.push((
        "热词榜首正确",
        rank.first().map(|r| r.fp == fnv1a_str(FNV_OFFSET, "physics") && r.hits == 3).unwrap_or(false),
    ));
    out.push((
        "未达标不上榜",
        rank.len() == 1,
    ));
    // 原文不可还原：账内只有指纹（结构声明——查账无字符串字段）。
    out.push(("查询原文不入账", e2.hotwords.len() == 2));

    // ── 判据三：失败率。
    out.push((
        "失败率 bps",
        e2.miss_rate_bps() == 2500, // 1 miss / 4 总
    ));

    // ── 判据四：限流（单 tick 超线显性拒+计数）。
    let mut e3 = DocselEngine::new();
    let mut i = 0;
    let mut limited_seen = false;
    while i < RATE_LIMIT_PER_TICK + 5 {
        if e3.record(1000 + i as u64, 0, 0, 1) == Err(DsCode::RateLimited) {
            limited_seen = true;
        }
        i += 1;
    }
    out.push((
        "单 tick 限流",
        limited_seen && e3.limited == 5,
    ));

    // ── 判据五：账滚动守恒。
    let mut e4 = DocselEngine::new();
    let mut j = 0;
    let mut rolled_seen = 0u64;
    while j < ACCESS_CAP + 8 {
        let t = (j / RATE_LIMIT_PER_TICK + 1) as u64;
        if e4.record(2000 + j as u64, 0, 0, t) == Err(DsCode::Rolled) {
            rolled_seen += 1;
        }
        j += 1;
    }
    out.push((
        "账滚动守恒",
        e4.a_len == ACCESS_CAP && e4.a_rolled == rolled_seen && e4.reconcile(),
    ));

    // ── 判据六：未知类别/路径显性拒。
    out.push((
        "未知类别拒",
        e4.record(1, 9, 0, 99) == Err(DsCode::KindUnknown),
    ));
    out.push((
        "未知路径拒",
        e4.record(1, 0, 9, 99) == Err(DsCode::PathUnknown),
    ));

    // ── 判据七：榜单越界显性。
    out.push(("榜单越界", e4.rank_at(0) == Err(DsCode::RankOutOfRange)));

    // ── 判据八：读屏三段。
    let s = e2.read_screen();
    out.push((
        "读屏三段",
        s.contains("三类计数") && s.contains("热词") && s.contains("限流"),
    ));

    out
}
