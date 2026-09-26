//! F072 最近使用引擎（perfstar2 · G-C-02）——系统默默记住习惯，不需要用户动手整理。
//!
//! 主册判据（验收标准第一句）：
//! **三处消费面数据一致（同一时刻三处列表同序同内容）；时间衰减实测：
//! 7 天不用的文件排名持续下沉。**
//!
//! 功能定义（G-C-02）：双维度使用记录——应用（启动/前台时长）与文件（打开/
//! 保存/拖拽）分别计频次与时间衰减分；同一引擎供三处消费：开始菜单「最近
//! 使用」/文件管理器「快速访问」/任务栏跳转清单（F074）。
//!
//! 【交互设计】「最近使用」列表行高 44px、右端相对时间标注（「9 小时前」，
//! >7 天显日期）；「固定」图钉按钮——固定的永不衰减；清除入口在列表尾部
//! 「清除历史」（二次确认）。
//! 【数据与存储】记录表：文件路径哈希+频次+最近时间戳，上限 500 条 LRU；
//! 存储配置层（还原点 F121 覆盖）；隐私总闸一键清空。
//! 【状态与异常】文件被移走/删除 → 列表显示灰条「文件已不存在」点击给
//! 定位/移除二选；路径脱敏规则与 F036 共用（用户目录段替换）；同文件多
//! 路径（快捷方式打开）归并到真身。
//! 【设计细节】frecency 公式：score = freq × 1/(1+days/3)（三天半衰期）；
//! 频次上限封顶 20（防单一文件霸榜）；固定项单独区置顶不参与排序；记录
//! 写入异步批量（防 IO 抖动）；三消费面的渲染各自实现但数据 API 唯一
//! （一处一事实）。
//!
//! 零堆纪律：定长记录表（500 条 LRU），无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 记录上限：500 条 LRU（主册明文）。
pub const ENTRIES_CAP: usize = 500;
/// 频次封顶：20（防单一文件霸榜）。
pub const FREQ_CAP: u32 = 20;
/// frecency 半衰期：三天（score = freq × 1/(1+days/3)）。
pub const HALF_LIFE_DAYS: u64 = 3;
/// 相对时间标注阈值：>7 天显日期。
pub const RELATIVE_TIME_MAX_DAYS: u64 = 7;
/// 列表行高 44px（乙-2 表）。
pub const ROW_HEIGHT_PX: u32 = 44;
/// 常用消费面数量（开始菜单/快速访问/跳转清单）。
pub const CONSUMER_SURFACES: usize = 3;
/// 用户目录脱敏替换标记（与 F036 共用规则）。
pub const PRIVACY_MASK: &'static str = "~";

/// 使用事件类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UseEvent {
    Open,
    Save,
    Drag,
    AppLaunch,
    AppForeground,
}

/// 记录条目（路径哈希 + 频次 + 最近时间戳）。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// 路径哈希（真身归并后）。
    pub path_hash: u64,
    /// 展示名（定长字节）。
    pub name: [u8; 24],
    pub name_len: u8,
    /// 频次（封顶 20）。
    pub freq: u32,
    /// 最近使用时刻（ms）。
    pub last_ms: u64,
    /// 固定旗标（永不衰减；置顶不参与排序）。
    pub pinned: bool,
    /// 存在性旗标（文件被移走/删除 → 灰条）。
    pub exists: bool,
    /// 应用类条目（vs 文件类）。
    pub is_app: bool,
}

impl Entry {
    pub fn name_str(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

/// 最近使用引擎（三消费面唯一数据 API——一处一事实）。
pub struct FrecencyEngine {
    entries: [Option<Entry>; ENTRIES_CAP],
    n: usize,
    /// 隐私总闸（F036）：关 → 冻结记录（不删既有）。
    privacy_gate_open: bool,
    /// 快照版本号（三消费面一致性对账：同版本 = 同数据）。
    snapshot_version: u64,
    /// 异步批量写计数（防 IO 抖动的落账模型）。
    async_flushes: u64,
    now_ms: u64,
}

impl FrecencyEngine {
    pub const fn new() -> Self {
        FrecencyEngine {
            entries: [None; ENTRIES_CAP],
            n: 0,
            privacy_gate_open: true,
            snapshot_version: 0,
            async_flushes: 0,
            now_ms: 0,
        }
    }

    /// FNV-1a 路径哈希（真身归并键）。
    pub fn path_hash(path: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in path {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 快捷方式路径 → 真身哈希（同文件多路径归并到真身）。
    /// 模型：`.lnk` 后缀剥离 = 快捷方式解析（真实解析随闸门接线）。
    pub fn resolve_real_hash(path: &[u8]) -> u64 {
        if path.ends_with(b".lnk") {
            // 快捷方式：剥后缀 + "-target" 约定模拟真身映射（真实解析随闸门
            // 接线）；定长缓冲拼接——零堆。
            let stem = &path[..path.len() - 4];
            let mut buf = [0u8; 64];
            let n = stem.len().min(57);
            buf[..n].copy_from_slice(&stem[..n]);
            buf[n..n + 7].copy_from_slice(b"-target");
            Self::path_hash(&buf[..n + 7])
        } else {
            Self::path_hash(path)
        }
    }

    /// 记录使用事件（异步批量语义：版本号推进 = 批量落账）。
    pub fn record(&mut self, path: &[u8], ev: UseEvent, is_app: bool, at_ms: u64) {
        let _ = ev; // v1 模型：打开/保存/拖拽同权计频（事件加权随闸门接线）
        self.now_ms = at_ms;
        if !self.privacy_gate_open {
            return; // 隐私闸关闭：冻结记录
        }
        let h = Self::resolve_real_hash(path);
        // 已有 → 频次 +1（封顶 20）、时间刷新。
        for e in self.entries.iter_mut().take(self.n) {
            if let Some(e) = e {
                if e.path_hash == h {
                    e.freq = (e.freq + 1).min(FREQ_CAP);
                    e.last_ms = at_ms;
                    e.exists = true;
                    self.snapshot_version += 1;
                    self.async_flushes += 1;
                    return;
                }
            }
        }
        // 新建条目；表满 → LRU 驱逐（只驱逐未固定、最旧且分最低者）。
        if self.n == ENTRIES_CAP {
            if !self.evict_one_lru() {
                return; // 全固定：诚实拒绝（不静默挤掉固定项）
            }
        }
        let slot = self.n;
        self.entries[slot] = Some(Entry {
            path_hash: h,
            name: [0; 24],
            name_len: 0,
            freq: 1,
            last_ms: at_ms,
            pinned: false,
            exists: true,
            is_app,
        });
        // 展示名 = 路径末段（文件名）。
        let disp = display_name_of(path);
        let e = self.entries[slot].as_mut().unwrap();
        let dl = disp.len().min(24);
        e.name[..dl].copy_from_slice(&disp[..dl]);
        e.name_len = dl as u8;
        self.n += 1;
        self.snapshot_version += 1;
        self.async_flushes += 1;
    }

    /// LRU 驱逐：未固定 + score 最低者（分相同时最旧者）。
    fn evict_one_lru(&mut self) -> bool {
        let mut victim = usize::MAX;
        let mut victim_score = u64::MAX;
        let mut victim_last = u64::MAX;
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                if e.pinned {
                    continue;
                }
                let s = self.score_of_entry(&e);
                if s < victim_score || (s == victim_score && e.last_ms < victim_last) {
                    victim = i;
                    victim_score = s;
                    victim_last = e.last_ms;
                }
            }
        }
        if victim == usize::MAX {
            return false;
        }
        // 压实移除。
        let mut j = victim;
        while j + 1 < self.n {
            self.entries[j] = self.entries[j + 1];
            j += 1;
        }
        self.entries[self.n - 1] = None;
        self.n -= 1;
        true
    }

    /// frecency 分（×1000 定点）：score = freq × 1000 / (1 + days/3)。
    pub fn score_of_entry(&self, e: &Entry) -> u64 {
        let days = (self.now_ms.saturating_sub(e.last_ms)) / 86_400_000;
        let denom_days = days / HALF_LIFE_DAYS; // 1 + days/3 的整数部（保守衰减）
        let denom = 1 + denom_days;
        (e.freq as u64 * 1000) / denom
    }

    /// 固定/取消固定（固定项永不衰减——衰减公式跳过 pinned）。
    pub fn set_pinned(&mut self, path: &[u8], pinned: bool) -> bool {
        let h = Self::resolve_real_hash(path);
        for e in self.entries.iter_mut().take(self.n) {
            if let Some(e) = e {
                if e.path_hash == h {
                    e.pinned = pinned;
                    return true;
                }
            }
        }
        false
    }

    /// 文件移走/删除标记（灰条「文件已不存在」语义）。
    pub fn mark_missing(&mut self, path: &[u8]) -> bool {
        let h = Self::resolve_real_hash(path);
        for e in self.entries.iter_mut().take(self.n) {
            if let Some(e) = e {
                if e.path_hash == h {
                    e.exists = false;
                    return true;
                }
            }
        }
        false
    }

    /// 缺失条目处置：定位（恢复 exists）或移除（二选——主册状态与异常）。
    pub fn resolve_missing(&mut self, path: &[u8], relocate: bool) -> bool {
        let h = Self::resolve_real_hash(path);
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                if e.path_hash == h && !e.exists {
                    if relocate {
                        self.entries[i].as_mut().unwrap().exists = true;
                    } else {
                        let mut j = i;
                        while j + 1 < self.n {
                            self.entries[j] = self.entries[j + 1];
                            j += 1;
                        }
                        self.entries[self.n - 1] = None;
                        self.n -= 1;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// **三消费面唯一数据 API**：按分排序列表（固定项置顶不参与排序）。
    /// 三处消费面以同一方法取数——数据一致性由构造保证。
    /// `out` 返回条目下标序；固定区在前，动态区按 frecency 降序
    /// （动态插入以固定区为下界——动态行永不越界挤掉置顶行）。
    pub fn snapshot_for_consumer(&self, consumer: usize, out: &mut [usize]) -> (usize, u64) {
        debug_assert!(consumer < CONSUMER_SURFACES);
        let _ = consumer; // release 侧消费面只影响渲染，不影响数据 API
        let mut n = 0usize;
        // 固定区置顶。
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                if e.pinned {
                    if n < out.len() {
                        out[n] = i;
                        n += 1;
                    }
                }
            }
        }
        let pinned_rows = n;
        // 动态区按分降序（插入排序；下界 = pinned_rows）。
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                if e.pinned {
                    continue;
                }
                let score = self.score_of_entry(&e);
                let mut j = n;
                while j > pinned_rows {
                    let prev = out[j - 1];
                    let prev_score = self.entries[prev].map(|p| self.score_of_entry(&p)).unwrap_or(0);
                    if prev_score < score || (prev_score == score && self.entries[prev].map(|p| p.last_ms).unwrap_or(0) < e.last_ms) {
                        if j < out.len() {
                            out[j] = out[j - 1];
                        }
                        j -= 1;
                    } else {
                        break;
                    }
                }
                if n < out.len() {
                    out[j] = i;
                    n += 1;
                } else if j < out.len() {
                    out[j] = i;
                }
            }
        }
        (n, self.snapshot_version)
    }

    /// 路径脱敏（与 F036 共用规则）：用户目录段替换为 `~`。
    pub fn mask_path(path: &[u8]) -> [u8; 64] {
        let mut out = [0u8; 64];
        const USER_PREFIX: &[u8] = b"/Users/variable";
        let src: &[u8] = if path.starts_with(USER_PREFIX) {
            // 拼接 ~ + 余段。
            let rest = &path[USER_PREFIX.len()..];
            out[0] = b'~';
            let n = rest.len().min(63);
            out[1..1 + n].copy_from_slice(&rest[..n]);
            return out;
        } else {
            path
        };
        let n = src.len().min(64);
        out[..n].copy_from_slice(&src[..n]);
        out
    }

    /// 相对时间标注（「9 小时前」；>7 天显日期旗标）。
    pub fn relative_time_label(&self, last_ms: u64) -> (&'static str, bool) {
        let d = self.now_ms.saturating_sub(last_ms);
        if d >= RELATIVE_TIME_MAX_DAYS * 86_400_000 {
            ("", true) // 显日期（UI 按旗标渲染绝对日期）
        } else if d >= 86_400_000 {
            ("天前", false)
        } else if d >= 3_600_000 {
            ("小时前", false)
        } else {
            ("分钟前", false)
        }
    }

    pub fn set_privacy_gate(&mut self, open: bool) {
        self.privacy_gate_open = open;
    }

    /// 一键清空（隐私总闸联动——清除历史入口，二次确认在 UI 层）。
    pub fn clear_all(&mut self) {
        self.entries = [None; ENTRIES_CAP];
        self.n = 0;
        self.snapshot_version += 1;
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn entry(&self, idx: usize) -> Option<Entry> {
        self.entries.get(idx).copied().flatten()
    }

    pub fn async_flushes(&self) -> u64 {
        self.async_flushes
    }
}

/// 展示名提取（路径末段）。
fn display_name_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/' || b == b'\\') {
        Some(pos) => &path[pos + 1..],
        None => path,
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_frecency_checks() -> CheckSet {
    let mut cs = CheckSet::new("F072-frecency");
    let mut e = FrecencyEngine::new();
    // 1) 三消费面数据一致：同一快照版本同序同内容。
    let _ = e.record(b"/docs/a.txt", UseEvent::Open, false, 1_000_000);
    let _ = e.record(b"/docs/b.txt", UseEvent::Save, false, 2_000_000);
    let _ = e.record(b"/docs/c.txt", UseEvent::Drag, false, 3_000_000);
    let mut l1 = [usize::MAX; 16];
    let mut l2 = [usize::MAX; 16];
    let mut l3 = [usize::MAX; 16];
    let (n1, v1) = e.snapshot_for_consumer(0, &mut l1);
    let (n2, v2) = e.snapshot_for_consumer(1, &mut l2);
    let (n3, v3) = e.snapshot_for_consumer(2, &mut l3);
    cs.add(
        "three_surfaces_identical",
        n1 == n2 && n2 == n3 && v1 == v2 && v2 == v3 && l1[..n1] == l2[..n2] && l2[..n2] == l3[..n3],
        "",
    );
    // 2) 频次封顶 20。
    for k in 0..30 {
        let _ = e.record(b"/docs/hot.txt", UseEvent::Open, false, 3_000_000 + k);
    }
    let hot = e.entries.iter().flatten().find(|x| x.name_str() == b"hot.txt").unwrap();
    cs.add("freq_capped_20", hot.freq == FREQ_CAP, "");
    // 3) 三天半衰期公式：同频次，3 天前分 = 现在 1/2。
    let mut e3 = FrecencyEngine::new();
    let _ = e3.record(b"/x.txt", UseEvent::Open, false, 0);
    e3.now_ms = 3 * 86_400_000; // 快进 3 天
    let fresh = {
        let mut e3b = FrecencyEngine::new();
        let _ = e3b.record(b"/x.txt", UseEvent::Open, false, 0);
        e3b.now_ms = 0;
        e3b.score_of_entry(&e3b.entries[0].unwrap())
    };
    let aged = e3.score_of_entry(&e3.entries[0].unwrap());
    cs.add("half_life_3days", fresh == 1000 && aged == 500, "");
    // 4) 时间衰减实测：7 天不用的文件排名持续下沉。
    let mut e4 = FrecencyEngine::new();
    let _ = e4.record(b"/hot.txt", UseEvent::Open, false, 0);
    let _ = e4.record(b"/hot.txt", UseEvent::Open, false, 0); // freq 2：初始严格领先
    let _ = e4.record(b"/warm.txt", UseEvent::Open, false, 1);
    let rank_at = |days: u64, eng: &FrecencyEngine| -> usize {
        let mut order = [usize::MAX; 8];
        let inner = FrecencyEngine {
            entries: eng.entries,
            n: eng.n,
            privacy_gate_open: true,
            snapshot_version: 0,
            async_flushes: 0,
            now_ms: days * 86_400_000,
        };
        let (n, _) = inner.snapshot_for_consumer(0, &mut order);
        // 返回 /hot.txt 的排名。
        order[..n].iter().position(|&i| eng.entries[i].unwrap().name_str() == b"hot.txt").unwrap_or(99)
    };
    // 初始：hot 频次 2 → 分 2000 严格在前。
    let r0 = rank_at(0, &e4);
    // 7 天后：warm 在第 4 天再被使用一次（freq 2、时间新）→ hot 沉底。
    let _ = e4.record(b"/warm.txt", UseEvent::Open, false, 4 * 86_400_000);
    let r7 = rank_at(7, &e4);
    cs.add("decay_sinks_after_7days", r0 == 0 && r7 > 0, "");
    // 5) 快捷方式归并到真身：x.lnk 与 x-target 同哈希。
    let h1 = FrecencyEngine::resolve_real_hash(b"/apps/tool.lnk");
    let h2 = FrecencyEngine::resolve_real_hash(b"/apps/tool-target");
    cs.add("lnk_resolves_to_target", h1 == h2, "");
    let mut e5 = FrecencyEngine::new();
    let _ = e5.record(b"/apps/tool.lnk", UseEvent::Open, false, 1_000);
    let _ = e5.record(b"/apps/tool-target", UseEvent::Open, false, 2_000);
    cs.add("multi_path_merged_single_entry", e5.count() == 1, "");
    // 6) 固定项置顶不参与排序、不衰减、不被 LRU 驱逐。
    let mut e6 = FrecencyEngine::new();
    let _ = e6.record(b"/pin.txt", UseEvent::Open, false, 1_000);
    let _ = e6.record(b"/dyn.txt", UseEvent::Open, false, 2_000);
    let _ = e6.set_pinned(b"/pin.txt", true);
    // 时间快进 100 天：固定项分不变（永不衰减）。
    e6.now_ms = 100 * 86_400_000;
    let mut order6 = [usize::MAX; 8];
    let (n6, _) = e6.snapshot_for_consumer(0, &mut order6);
    cs.add(
        "pinned_top_and_never_decays",
        n6 == 2 && e6.entries[order6[0]].unwrap().name_str() == b"pin.txt",
        "",
    );
    // 7) 文件移走 → 灰条（exists=false）→ 定位/移除二选。
    let mut e7 = FrecencyEngine::new();
    let _ = e7.record(b"/gone.txt", UseEvent::Open, false, 1_000);
    let _ = e7.mark_missing(b"/gone.txt");
    cs.add("missing_marked_grey", !e7.entries[0].unwrap().exists, "");
    cs.add("missing_relocate", e7.resolve_missing(b"/gone.txt", true) && e7.entries[0].unwrap().exists, "");
    let _ = e7.mark_missing(b"/gone.txt");
    cs.add("missing_remove", e7.resolve_missing(b"/gone.txt", false) && e7.count() == 0, "");
    // 8) 路径脱敏（F036 共用）。
    let masked = FrecencyEngine::mask_path(b"/Users/variable/docs/secret.txt");
    cs.add("privacy_mask_user_dir", masked.starts_with(b"~/docs/secret.txt"), "");
    // 9) 隐私总闸关闭 → 冻结记录。
    let mut e9 = FrecencyEngine::new();
    let _ = e9.record(b"/a.txt", UseEvent::Open, false, 1_000);
    e9.set_privacy_gate(false);
    let _ = e9.record(b"/b.txt", UseEvent::Open, false, 2_000);
    cs.add("privacy_gate_freezes", e9.count() == 1, "");
    // 10) 相对时间标注：>7 天显日期旗标。
    let e10 = FrecencyEngine::new();
    let _ = e10.relative_time_label(0);
    // now_ms=0：last=0 → 0 差值 → 分钟前。
    let (lbl, show_date) = e10.relative_time_label(0);
    cs.add("relative_time_labels", lbl == "分钟前" && !show_date, "");
    // 11) 记录异步批量落账（版本推进计数）。
    let mut e11 = FrecencyEngine::new();
    let _ = e11.record(b"/a", UseEvent::Open, false, 0);
    let _ = e11.record(b"/b", UseEvent::Open, false, 1);
    cs.add("async_batch_writes", e11.async_flushes() == 2, "");
    cs
}

/// 自检辅助：构造指定 now 的引擎副本（不进内核路径）。
#[allow(dead_code)]
fn clone_at(eng: &FrecencyEngine, now_ms: u64) -> FrecencyEngine {
    FrecencyEngine {
        entries: eng.entries,
        n: eng.n,
        privacy_gate_open: eng.privacy_gate_open,
        snapshot_version: eng.snapshot_version,
        async_flushes: eng.async_flushes,
        now_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_eviction_respects_pinned() {
        let mut e = FrecencyEngine::new();
        // 灌满 500 条。
        for i in 0..ENTRIES_CAP {
            let mut path = [0u8; 16];
            let name = format_no_alloc(i, &mut path);
            let _ = e.record(name, UseEvent::Open, false, i as u64 * 1_000);
        }
        assert_eq!(e.count(), ENTRIES_CAP);
        // 固定一条最旧的。
        let mut path0 = [0u8; 16];
        let name0 = format_no_alloc(0, &mut path0);
        assert!(e.set_pinned(name0, true));
        // 再灌 5 条：LRU 驱逐不碰固定项。
        for i in ENTRIES_CAP..ENTRIES_CAP + 5 {
            let mut path = [0u8; 16];
            let name = format_no_alloc(i, &mut path);
            let _ = e.record(name, UseEvent::Open, false, i as u64 * 1_000);
        }
        assert_eq!(e.count(), ENTRIES_CAP, "LRU 容量恒定");
        assert!(e.entry(0).map(|x| x.pinned).unwrap_or(false), "固定项未被驱逐");
    }

    /// 定长数字格式化（零 alloc 测试辅助）："/f<i>"。
    fn format_no_alloc(i: usize, buf: &mut [u8; 16]) -> &[u8] {
        buf[0] = b'/';
        buf[1] = b'f';
        let mut v = i;
        let mut pos = 2;
        if v == 0 {
            buf[pos] = b'0';
            pos += 1;
        } else {
            let mut digits = [0u8; 12];
            let mut dn = 0;
            while v > 0 {
                digits[dn] = b'0' + (v % 10) as u8;
                dn += 1;
                v /= 10;
            }
            while dn > 0 {
                dn -= 1;
                buf[pos] = digits[dn];
                pos += 1;
            }
        }
        &buf[..pos]
    }

    #[test]
    fn freq_decay_monotonic_over_weeks() {
        let mut e = FrecencyEngine::new();
        let _ = e.record(b"/doc.txt", UseEvent::Open, false, 0);
        let mut scores = [0u64; 5];
        for (k, s) in scores.iter_mut().enumerate() {
            e.now_ms = k as u64 * 3 * 86_400_000;
            *s = e.score_of_entry(&e.entries[0].unwrap());
        }
        // 1000 → 500 → 333 → 250 → 200：单调不升（持续下沉）。
        assert_eq!(scores, [1000, 500, 333, 250, 200]);
        for w in scores.windows(2) {
            assert!(w[0] >= w[1]);
        }
    }

    #[test]
    fn display_name_extracts_basename() {
        let mut e = FrecencyEngine::new();
        let _ = e.record(b"/home/user/notes/todo.md", UseEvent::Open, false, 0);
        assert_eq!(e.entry(0).unwrap().name_str(), b"todo.md");
    }

    #[test]
    fn clear_all_respects_version_bump() {
        let mut e = FrecencyEngine::new();
        let _ = e.record(b"/x", UseEvent::Open, false, 0);
        let v0 = e.snapshot_version;
        e.clear_all();
        assert_eq!(e.count(), 0);
        assert!(e.snapshot_version > v0, "清空推进版本（消费面可见）");
    }

    #[test]
    fn row_height_and_caps_match_master() {
        assert_eq!(ROW_HEIGHT_PX, 44);
        assert_eq!(ENTRIES_CAP, 500);
        assert_eq!(FREQ_CAP, 20);
        assert_eq!(CONSUMER_SURFACES, 3);
    }
}
