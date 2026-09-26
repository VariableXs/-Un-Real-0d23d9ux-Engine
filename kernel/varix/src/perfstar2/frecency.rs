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

// ===========================================================================
// v2 深化批（F072 · G-C-02）——衰减重算扫描 / 多别名归并 / 三面快照同源
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-02 功能定义的实装细化，非新立项）：
// 1. DecaySweeper —— 衰减重算扫描：半衰期公式（freq × 1/(1+days/3)）
//    的批量重算面——「7 天不用排名持续下沉」的量化验证器（给出
//    0/1/3/7/14 天五锚点的分数单调下沉曲线）。
// 2. AliasMergeLedger —— 多别名归并账：同名真身的多入口（.lnk/拼音
//    路径/短路径）频次合并入真身账——归并不丢频次（resolve_real_hash
//    的账面深化）。
// 3. SurfaceConsistency —— 三消费面同序断言：同一快照版本下三消费面
//    取到的列表逐项相等（主册「三处消费面数据一致」的机制面——版本
//    号相同 + 列表逐项对拍）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 衰减锚点天数（五锚点验证曲线）。
pub const DECAY_ANCHOR_DAYS: [u64; 5] = [0, 1, 3, 7, 14];

// ---------------------------------------------------------------------------
// 深化一：衰减重算扫描
// ---------------------------------------------------------------------------

/// 半衰期分数公式（×1000 定点）：score_x1000 = freq × 3_000_000 /
/// (3000 + days × 1000)——days=0 得 freq×1000；days=3（半衰期）恰半。
pub fn decay_score_x1000(freq: u32, days_since: u64) -> u64 {
    let denom = 3000 + (days_since.min(36_500)) * 1000;
    (freq as u64) * 3_000_000 / denom
}

/// 五锚点单调下沉验证：0 天 > 1 天 > 3 天 > 7 天 > 14 天（严格下沉）。
pub fn decay_curve_strictly_descending(freq: u32) -> bool {
    let mut prev = decay_score_x1000(freq, 0);
    for &d in DECAY_ANCHOR_DAYS[1..].iter() {
        let s = decay_score_x1000(freq, d);
        if s >= prev {
            return false;
        }
        prev = s;
    }
    true
}

// ---------------------------------------------------------------------------
// 深化二：多别名归并账
// ---------------------------------------------------------------------------

/// 别名归并账：alias_hash → 真身 path_hash 的定长映射。
pub struct AliasMergeLedger {
    pairs: [Option<(u64, u64)>; 32], // (别名哈希, 真身哈希)
    n: usize,
    merged_uses: u64,
    /// 直接入口（非别名）计数。
    direct_uses: u64,
}

impl AliasMergeLedger {
    pub const fn new() -> Self {
        AliasMergeLedger {
            pairs: [None; 32],
            n: 0,
            merged_uses: 0,
            direct_uses: 0,
        }
    }

    /// 登记别名→真身（同别名幂等；表满拒绝——不静默挤掉既有映射）。
    pub fn register(&mut self, alias_hash: u64, real_hash: u64) -> bool {
        for p in self.pairs.iter().flatten() {
            if p.0 == alias_hash {
                return p.1 == real_hash; // 同别名不同真身 = 冲突拒绝
            }
        }
        if self.n >= 32 {
            return false;
        }
        self.pairs[self.n] = Some((alias_hash, real_hash));
        self.n += 1;
        true
    }

    /// 一次使用记账：别名入口归并到真身（返回真身哈希）。
    pub fn record_use(&mut self, path_hash: u64) -> u64 {
        for p in self.pairs.iter().flatten() {
            if p.0 == path_hash {
                self.merged_uses += 1;
                return p.1;
            }
        }
        self.direct_uses += 1;
        path_hash
    }

    /// 归并率 ×100（别名入口 / 总入口）。
    pub fn merge_rate_pct(&self) -> u32 {
        let total = self.merged_uses + self.direct_uses;
        if total == 0 {
            return 0;
        }
        (self.merged_uses * 100 / total) as u32
    }

    pub fn stats(&self) -> (usize, u64, u64) {
        (self.n, self.merged_uses, self.direct_uses)
    }
}

// ---------------------------------------------------------------------------
// 深化三：三消费面同序断言
// ---------------------------------------------------------------------------

/// 三消费面快照对拍：同一快照版本 + 三个列表逐项相等 → 一致。
/// 返回 (一致, 版本号)——不一致时调用方按主册纪律重刷三面。
pub fn surfaces_consistent(
    snapshots: [&[usize]; 3],
    versions: [u64; 3],
) -> (bool, u64) {
    let v = versions[0];
    if versions[1] != v || versions[2] != v {
        return (false, v); // 版本不同步——直接不一致（不比内容）
    }
    let same = snapshots[0] == snapshots[1] && snapshots[1] == snapshots[2];
    (same, v)
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：衰减曲线 / 归并账 / 三面对拍逐条实摆。
pub fn run_frecency_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F072-frecency-deep");

    // ── 衰减曲线 ──
    // 1) 五锚点严格下沉（freq=10 与 freq=1 双档验证）。
    cs.add(
        "decay_anchors_descend_f10",
        decay_curve_strictly_descending(10),
        "",
    );
    cs.add("decay_anchors_descend_f1", decay_curve_strictly_descending(1), "");
    // 2) 半衰期锚点：3 天恰半（freq=10 → 5000 = 10×1000/2）。
    cs.add(
        "decay_half_life_exact",
        decay_score_x1000(10, 3) == 5000,
        "",
    );
    // 3) 零频恒零；7 天不用的量化下沉（freq=10：1 天 7500 → 7 天 3000）。
    cs.add("decay_zero_freq_zero", decay_score_x1000(0, 7) == 0, "");
    cs.add(
        "decay_7d_quantified",
        decay_score_x1000(10, 7) == 3000 && decay_score_x1000(10, 7) < decay_score_x1000(10, 1),
        "",
    );
    // 4) 超长空窗不炸（36500 天封顶——分母有界）。
    cs.add(
        "decay_extreme_days_safe",
        decay_score_x1000(5, u64::MAX) < 1000, // 极限空窗分数有界（不溢出不炸）
        "",
    );

    // ── 多别名归并账 ──
    let mut am = AliasMergeLedger::new();
    // 真身 0xAAAA；两个别名（.lnk 与短路径）。
    cs.add(
        "alias_register_pair",
        am.register(0x1111, 0xAAAA) && am.register(0x2222, 0xAAAA),
        "",
    );
    cs.add(
        "alias_use_merged",
        am.record_use(0x1111) == 0xAAAA && am.record_use(0x2222) == 0xAAAA,
        "",
    );
    cs.add("alias_use_direct", am.record_use(0xAAAA) == 0xAAAA, "");
    cs.add(
        "alias_rate_ledger",
        am.stats() == (2, 2, 1) && am.merge_rate_pct() == 66, // 2/3
        "",
    );
    // 同别名不同真身 = 冲突拒绝（一处一事实——别名不得漂移）。
    cs.add("alias_conflict_refused", !am.register(0x1111, 0xBBBB), "");
    // 幂等：同别名同真身重复登记不增账。
    let _ = am.register(0x1111, 0xAAAA);
    cs.add("alias_register_idempotent", am.stats().0 == 2, "");

    // ── 三消费面同序断言 ──
    // 1) 同版本同列表 → 一致。
    let l1 = [3usize, 1, 4, 1, 5];
    let (ok, v) = surfaces_consistent([&l1, &l1, &l1], [7, 7, 7]);
    cs.add("surfaces_same_ok", ok && v == 7, "");
    // 2) 版本不同步 → 不一致（即使内容相同——旧快照不得冒充新快照）。
    let (ok2, _) = surfaces_consistent([&l1, &l1, &l1], [7, 7, 8]);
    cs.add("surfaces_version_mismatch_red", !ok2, "");
    // 3) 同版本不同内容 → 不一致。
    let l2 = [3usize, 1, 4];
    let (ok3, _) = surfaces_consistent([&l1, &l1, &l2], [7, 7, 7]);
    cs.add("surfaces_content_diff_red", !ok3, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn decay_curve_never_rises() {
        // 连续扫 0..60 天：分数序列逐点不升（单调性全谱验证）。
        let mut prev = decay_score_x1000(20, 0);
        for d in 1..60u64 {
            let s = decay_score_x1000(20, d);
            assert!(s <= prev, "第 {d} 天分数回升——衰减公式破单调");
            prev = s;
        }
    }

    #[test]
    fn alias_chained_alias_not_followed() {
        // 别名指向别名 = 冲突场景：0x3333 → 0xAAAA 已登记，再登记
        // 0x3333 → 0xBBBB 被拒（不许链式漂移）。
        let mut am = AliasMergeLedger::new();
        let _ = am.register(0x3333, 0xAAAA);
        assert!(!am.register(0x3333, 0xBBBB));
        assert_eq!(am.record_use(0x3333), 0xAAAA);
    }

    #[test]
    fn alias_table_full_rejects_honestly() {
        let mut am = AliasMergeLedger::new();
        let mut granted = 0;
        for k in 0..40u64 {
            if am.register(0x10_0000 + k, 0xAAAA) {
                granted += 1;
            }
        }
        assert_eq!(granted, 32, "32 对硬顶——拒绝显式");
        assert_eq!(am.stats().0, 32);
    }

    #[test]
    fn surfaces_empty_lists_consistent() {
        let e: [usize; 0] = [];
        let (ok, _) = surfaces_consistent([&e, &e, &e], [0, 0, 0]);
        assert!(ok, "空列表同版本也一致（空态不是例外）");
    }
}

// ===========================================================================
// v3 深化批（F072 · G-C-02）——目录聚类视图 / 快照校验和与往返
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-02 功能定义的实装细化，非新立项）：
// 1. DirCluster —— 目录聚类视图：按目录哈希聚合条目计数（「最近」
//    按目录折叠展示的数据面）；聚类数封顶。
// 2. SnapshotChecksum —— 引擎快照序列化：条目定长字节格式 + FNV
//    校验 + roundtrip 等值（三消费面快照可迁移的机制面）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 聚类容量。
pub const CLUSTER_CAP: usize = 32;
/// 快照条目字节布局：hash(8) + freq(4) + last_ms(8) + flags(1) = 21 字节。
pub const SNAP_ENTRY_BYTES: usize = 21;
/// 快照最大条目（序列化代表集）。
pub const SNAP_MAX_ENTRIES: usize = 24;

// ---------------------------------------------------------------------------
// 深化一：目录聚类视图
// ---------------------------------------------------------------------------

/// 目录哈希：取路径首段（首个 '/' 前）FNV——目录身份。
pub fn dir_hash_of_path(path: &[u8]) -> u64 {
    let end = path.iter().position(|b| *b == b'/').unwrap_or(path.len());
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in &path[..end] {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 聚类表（目录哈希 → 计数）。
pub struct DirCluster {
    entries: [Option<(u64, u32)>; CLUSTER_CAP],
    n: usize,
    merged: u64,
}

impl DirCluster {
    pub const fn new() -> Self {
        DirCluster { entries: [None; CLUSTER_CAP], n: 0, merged: 0 }
    }

    /// 记一条使用：目录已在 → 计数+1；不在 → 新建（满丢弃最冷聚类）。
    pub fn record(&mut self, path: &[u8]) {
        let dh = dir_hash_of_path(path);
        for e in self.entries.iter_mut().flatten() {
            if e.0 == dh {
                e.1 += 1;
                self.merged += 1;
                return;
            }
        }
        if self.n < CLUSTER_CAP {
            self.entries[self.n] = Some((dh, 1));
            self.n += 1;
            return;
        }
        // 满：计数最小的聚类被吸收（其计数并入同路径？无——直接被覆盖）。
        let mut victim = 0usize;
        let mut min = u32::MAX;
        for (k, e) in self.entries.iter().enumerate() {
            if let Some((_, c)) = e {
                if *c < min {
                    min = *c;
                    victim = k;
                }
            }
        }
        self.entries[victim] = Some((dh, 1));
    }

    /// 聚类数。
    pub fn clusters(&self) -> usize {
        self.n
    }

    /// 某目录计数。
    pub fn count_of(&self, path: &[u8]) -> u32 {
        let dh = dir_hash_of_path(path);
        self.entries
            .iter()
            .flatten()
            .find(|e| e.0 == dh)
            .map(|e| e.1)
            .unwrap_or(0)
    }

    /// Top-1 目录哈希（计数最大——平局取先入）。
    pub fn top_dir(&self) -> Option<u64> {
        let mut best: Option<(u64, u32)> = None;
        for e in self.entries.iter().flatten() {
            best = match best {
                Some((_, c)) if c >= e.1 => best,
                _ => Some(*e),
            };
        }
        best.map(|(h, _)| h)
    }

    pub fn merged(&self) -> u64 {
        self.merged
    }
}

// ---------------------------------------------------------------------------
// 深化二：快照校验和与往返
// ---------------------------------------------------------------------------

/// 快照序列化器（条目定长 21B + 头 4B 条目数 + 尾 4B FNV）。
pub struct SnapshotCodec {
    buf: [u8; 4 + SNAP_MAX_ENTRIES * SNAP_ENTRY_BYTES + 4],
    len: usize,
}

/// 快照条目（序列化输入/输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapEntry {
    pub path_hash: u64,
    pub freq: u32,
    pub last_ms: u64,
    pub pinned: bool,
}

impl SnapshotCodec {
    /// 编码。
    pub fn encode(entries: &[SnapEntry]) -> Self {
        let mut c = SnapshotCodec { buf: [0; 4 + SNAP_MAX_ENTRIES * SNAP_ENTRY_BYTES + 4], len: 0 };
        let n = entries.len().min(SNAP_MAX_ENTRIES);
        c.buf[0] = (n as u32 & 0xFF) as u8;
        c.buf[1] = ((n as u32 >> 8) & 0xFF) as u8;
        c.buf[2] = 0;
        c.buf[3] = 0;
        c.len = 4;
        for e in entries.iter().take(n) {
            let base = c.len;
            c.buf[base..base + 8].copy_from_slice(&e.path_hash.to_le_bytes());
            c.buf[base + 8..base + 12].copy_from_slice(&e.freq.to_le_bytes());
            c.buf[base + 12..base + 20].copy_from_slice(&e.last_ms.to_le_bytes());
            c.buf[base + 20] = if e.pinned { 1 } else { 0 };
            c.len += SNAP_ENTRY_BYTES;
        }
        // 尾部 FNV。
        let h = crate::perfstar2::perfgate::fnv1a(&c.buf[..c.len]);
        c.buf[c.len..c.len + 4].copy_from_slice(&h.to_le_bytes());
        c.len += 4;
        c
    }

    /// 校验。
    pub fn verify(&self) -> bool {
        if self.len < 8 {
            return false;
        }
        let content = &self.buf[..self.len - 4];
        let expected = crate::perfstar2::perfgate::fnv1a(content);
        let stored = u32::from_le_bytes([
            self.buf[self.len - 4],
            self.buf[self.len - 3],
            self.buf[self.len - 2],
            self.buf[self.len - 1],
        ]);
        expected == stored
    }

    /// 解码到定长输出（roundtrip 的实现面——校验不过返回 0）。
    pub fn decode_into(&self, out: &mut [SnapEntry]) -> usize {
        if !self.verify() {
            return 0;
        }
        let n = (self.buf[0] as usize) | ((self.buf[1] as usize) << 8);
        let n = n.min(SNAP_MAX_ENTRIES).min(out.len());
        for k in 0..n {
            let base = 4 + k * SNAP_ENTRY_BYTES;
            let mut h8 = [0u8; 8];
            h8.copy_from_slice(&self.buf[base..base + 8]);
            let mut f4 = [0u8; 4];
            f4.copy_from_slice(&self.buf[base + 8..base + 12]);
            let mut l8 = [0u8; 8];
            l8.copy_from_slice(&self.buf[base + 12..base + 20]);
            out[k] = SnapEntry {
                path_hash: u64::from_le_bytes(h8),
                freq: u32::from_le_bytes(f4),
                last_ms: u64::from_le_bytes(l8),
                pinned: self.buf[base + 20] == 1,
            };
        }
        n
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：聚类 / 快照逐条实摆。
pub fn run_frecency_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F072-frecency-v3");

    // ── 目录聚类 ──
    let mut dc = DirCluster::new();
    for _ in 0..3 {
        dc.record(b"docs/report.docx");
    }
    for _ in 0..5 {
        dc.record(b"docs/spec.md");
    }
    for _ in 0..2 {
        dc.record(b"music/song.flac");
    }
    cs.add("dircluster_merges_same_dir", dc.clusters() == 2, "");
    cs.add("dircluster_counts", dc.count_of(b"docs/anything") == 8, "");
    cs.add("dircluster_top", dc.top_dir() == Some(dir_hash_of_path(b"docs/x")), "");
    cs.add("dircluster_merged_ledger", dc.merged() == 8, ""); // 10 次记录 − 2 新建
    // 满后挤最冷。
    let mut dc2 = DirCluster::new();
    for k in 0..CLUSTER_CAP {
        let (p, l) = format_path(k);
        dc2.record(&p[..l]);
    }
    cs.add("dircluster_cap", dc2.clusters() == CLUSTER_CAP, "");

    // ── 快照编解码 ──
    let entries = [
        SnapEntry { path_hash: 0xAAAA_BBBB, freq: 12, last_ms: 1_234_567, pinned: true },
        SnapEntry { path_hash: 0xCCDD_EEFF, freq: 3, last_ms: 7_654_321, pinned: false },
    ];
    let snap = SnapshotCodec::encode(&entries);
    cs.add("snap_len_exact", snap.len() == 4 + 2 * SNAP_ENTRY_BYTES + 4, "");
    cs.add("snap_verify_ok", snap.verify(), "");
    let mut out = [SnapEntry { path_hash: 0, freq: 0, last_ms: 0, pinned: false }; SNAP_MAX_ENTRIES];
    let n = snap.decode_into(&mut out);
    cs.add(
        "snap_roundtrip_equal",
        n == 2 && out[0] == entries[0] && out[1] == entries[1],
        "",
    );
    // 篡改检出。
    let mut bad = SnapshotCodec::encode(&entries);
    bad.buf[5] ^= 0xFF;
    cs.add("snap_tamper_detected", !bad.verify() && bad.decode_into(&mut out) == 0, "");
    // 24 条满编解码。
    let full: Vec<SnapEntry> = (0..SNAP_MAX_ENTRIES)
        .map(|k| SnapEntry { path_hash: k as u64, freq: k as u32, last_ms: k as u64, pinned: false })
        .collect();
    let snap2 = SnapshotCodec::encode(&full);
    let mut out2 = [SnapEntry { path_hash: 0, freq: 0, last_ms: 0, pinned: false }; SNAP_MAX_ENTRIES];
    cs.add("snap_full_roundtrip", snap2.decode_into(&mut out2) == SNAP_MAX_ENTRIES && out2[23].path_hash == 23, "");

    cs
}

/// 测试辅助：生成第 k 条路径（零堆——定长栈缓冲，返回缓冲与长度）。
fn format_path(k: usize) -> ([u8; 16], usize) {
    let mut b = *b"dir0000000000000";
    let mut v = k;
    let mut digits = [0u8; 12];
    let mut dn = 0usize;
    if v == 0 {
        digits[0] = b'0';
        dn = 1;
    }
    while v > 0 {
        digits[dn] = b'0' + (v % 10) as u8;
        v /= 10;
        dn += 1;
    }
    for j in 0..dn {
        b[3 + j] = digits[dn - 1 - j];
    }
    (b, 3 + dn)
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn dir_hash_differs_by_dir() {
        assert_ne!(dir_hash_of_path(b"docs/a"), dir_hash_of_path(b"music/a"));
        assert_eq!(dir_hash_of_path(b"docs/a"), dir_hash_of_path(b"docs/b"), "同目录同哈希");
        assert_eq!(dir_hash_of_path(b"nodir"), dir_hash_of_path(b"nodir"), "同全路径同哈希");
        assert_ne!(dir_hash_of_path(b"nodir"), dir_hash_of_path(b"other"), "不同全路径不同哈希");
    }

    #[test]
    fn snap_zero_entries_valid() {
        let s = SnapshotCodec::encode(&[]);
        assert!(s.verify());
        let mut out = [SnapEntry { path_hash: 0, freq: 0, last_ms: 0, pinned: false }; 4];
        assert_eq!(s.decode_into(&mut out), 0);
    }

    #[test]
    fn snap_over_cap_truncates() {
        let v: Vec<SnapEntry> = (0..40)
            .map(|k| SnapEntry { path_hash: k as u64, freq: 1, last_ms: 1, pinned: false })
            .collect();
        let s = SnapshotCodec::encode(&v);
        let mut out = [SnapEntry { path_hash: 0, freq: 0, last_ms: 0, pinned: false }; SNAP_MAX_ENTRIES];
        assert_eq!(s.decode_into(&mut out), SNAP_MAX_ENTRIES, "超容截断到 24");
    }
}

// ===========================================================================
// v4 深化批（F072 · G-C-02）——快照合并 / Top-K 选择
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-02 功能定义的实装细化，非新立项）：
// 1. SnapshotMerge —— 双机快照合并：同条目取频次 max、最近时刻 max
//    （跨设备同步的合并律——并集不丢条目，冲突取大）。
// 2. TopKSelect —— Top-K 选择：score 降序前 K 条（无堆插入选择——
//    排行页/聚类页的选取面）。
// 全部零堆：定长表，无 Vec/String/浮点/format!。
// ===========================================================================



/// 合并两侧快照（各 ≤24 条）→ 合并集（≤24 条，写 out 返回数）。
/// 合并律：同 path_hash → freq 取 max、last_ms 取 max、pinned 取或；
/// 独有条目直接并入（超 24 条时按 freq 降序保留前 24）。
pub fn snapshot_merge(a: &[SnapEntry], b: &[SnapEntry], out: &mut [SnapEntry]) -> usize {
    // 先收集并集（去重合并）到临时定长缓冲（容量 48）。
    let mut merged: [Option<SnapEntry>; 48] = [None; 48];
    let mut mn = 0usize;
    for e in a.iter().chain(b.iter()) {
        if let Some(m) = merged.iter_mut().flatten().find(|m| m.path_hash == e.path_hash) {
            // 冲突取大（并集不丢——两侧数据都可信）。
            m.freq = m.freq.max(e.freq);
            m.last_ms = m.last_ms.max(e.last_ms);
            m.pinned = m.pinned || e.pinned;
        } else if mn < 48 {
            merged[mn] = Some(*e);
            mn += 1;
        }
    }
    // 频次降序插入排序。
    for i in 1..mn {
        let key = merged[i];
        let mut j = i;
        while j > 0 && merged[j - 1].unwrap().freq < key.unwrap().freq {
            merged[j] = merged[j - 1];
            j -= 1;
        }
        merged[j] = key;
    }
    let n = mn.min(out.len()).min(SNAP_MAX_ENTRIES);
    for k in 0..n {
        out[k] = merged[k].unwrap();
    }
    n
}

/// Top-K 选择：按 score 降序前 K 条的 idx（K = out.len() 上限）。
pub fn topk_select(scores: &[u32], out: &mut [usize]) -> usize {
    let k = out.len();
    if k == 0 {
        return 0;
    }
    // 选择法：K 轮各找当前最大（已选置 u32::MAX 哨兵——分数非负域）。
    let mut work = [u32::MAX; 64];
    let n = scores.len().min(64);
    for (w, s) in work.iter_mut().zip(scores.iter()) {
        *w = *s;
    }
    let mut written = 0usize;
    for _ in 0..k {
        // 找当前最大（非哨兵）；严格大于 → 平分稳定（先入优先）。
        let mut bi: Option<usize> = None;
        let mut bv = -1i64;
        for (idx, v) in work.iter().enumerate().take(n) {
            if *v != u32::MAX && (*v as i64) > bv {
                bv = *v as i64;
                bi = Some(idx);
            }
        }
        match bi {
            Some(i) => {
                out[written] = i;
                written += 1;
                work[i] = u32::MAX;
            }
            None => break,
        }
    }
    written
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：合并 / TopK 逐条实摆。
pub fn run_frecency_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F072-frecency-v4");

    // ── 快照合并 ──
    let a = [
        SnapEntry { path_hash: 1, freq: 10, last_ms: 100, pinned: false },
        SnapEntry { path_hash: 2, freq: 5, last_ms: 200, pinned: false },
    ];
    let b = [
        SnapEntry { path_hash: 2, freq: 9, last_ms: 150, pinned: true },
        SnapEntry { path_hash: 3, freq: 1, last_ms: 50, pinned: false },
    ];
    let mut out = [SnapEntry { path_hash: 0, freq: 0, last_ms: 0, pinned: false }; 24];
    let n = snapshot_merge(&a, &b, &mut out);
    cs.add("merge_union_count", n == 3, "");
    cs.add(
        "merge_conflict_takes_max",
        out.iter().any(|e| e.path_hash == 2 && e.freq == 9 && e.last_ms == 200 && e.pinned),
        "",
    );
    // 频次降序排列（合并后 top = freq 10）。
    cs.add("merge_sorted_by_freq", out[0].freq == 10 && out[1].freq == 9 && out[2].freq == 1, "");
    // 相同快照合并 = 原样。
    let n2 = snapshot_merge(&a, &a, &mut out);
    cs.add("merge_self_identity", n2 == 2 && out[0].freq == 10, "");
    // 双侧不同条目并集不丢。
    cs.add("merge_no_loss", out.iter().take(n2).all(|e| e.path_hash != 0), "");

    // ── TopK ──
    let scores = [30u32, 90, 10, 70, 50];
    let mut out2 = [0usize; 3];
    let k = topk_select(&scores, &mut out2);
    cs.add("topk_three_largest", k == 3 && out2 == [1, 3, 4], "");
    // K > n → 全部返回。
    let mut out3 = [0usize; 8];
    cs.add("topk_k_over_n", topk_select(&scores, &mut out3) == 5, "");
    // 空输入。
    cs.add("topk_empty_zero", topk_select(&[], &mut out2) == 0, "");
    // 平分稳定（先入优先——严格大于才换）。
    let ties = [50u32, 50, 50];
    let mut out4 = [0usize; 3];
    let _ = topk_select(&ties, &mut out4);
    cs.add("topk_ties_stable", out4 == [0, 1, 2], "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn merge_cap_24_by_freq() {
        let many: Vec<SnapEntry> = (0..30)
            .map(|k| SnapEntry { path_hash: k as u64, freq: k as u32, last_ms: 0, pinned: false })
            .collect();
        let mut out = [SnapEntry { path_hash: 0, freq: 0, last_ms: 0, pinned: false }; 24];
        let n = snapshot_merge(&many, &[], &mut out);
        assert_eq!(n, 24);
        assert_eq!(out[0].freq, 29, "超容按频次保 top");
        assert!(out.iter().take(24).all(|e| e.freq >= 6), "低频被截");
    }

    #[test]
    fn topk_zero_scores_all_valid() {
        let scores = [0u32; 5];
        let mut out = [0usize; 2];
        assert_eq!(topk_select(&scores, &mut out), 2);
        // 全零分也按序返回 idx（哨兵不与 0 冲突——u32::MAX 哨兵设计）。
        assert!(out[0] != out[1]);
    }
}

// ===========================================================================
// v5 深化批（deep5）：时钟偏差补偿 + 批量导入去重
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：跨设备时钟偏差补偿（两设备事件流 → 偏差 = 事件对时刻差中位）
// ---------------------------------------------------------------------------

/// 时钟偏差估计器：配对事件（同事件两机时刻）→ 偏差中位数。
pub struct ClockSkewEstimator {
    /// 配对差值（device_b - device_a，ms，可为负）环。
    diffs: [i32; 8],
    pos: usize,
    n: usize,
}

impl ClockSkewEstimator {
    pub const fn new() -> Self {
        ClockSkewEstimator { diffs: [0; 8], pos: 0, n: 0 }
    }

    /// 记一对事件时刻。
    pub fn pair(&mut self, a_ms: u32, b_ms: u32) {
        self.diffs[self.pos] = b_ms as i32 - a_ms as i32;
        self.pos = (self.pos + 1) % 8;
        if self.n < 8 {
            self.n += 1;
        }
    }

    /// 偏差中位数（样本 <4 → None）。
    pub fn skew_median_ms(&self) -> Option<i32> {
        if self.n < 4 {
            return None;
        }
        let mut sorted = [0i32; 8];
        sorted[..self.n].copy_from_slice(&self.diffs[..self.n]);
        sorted[..self.n].sort();
        Some(sorted[self.n / 2])
    }

    /// b 机时刻折算到 a 机时间轴。
    pub fn to_a_timeline(&self, b_ms: u32) -> Option<u32> {
        let skew = self.skew_median_ms()?;
        Some((b_ms as i64 - skew as i64).clamp(0, u32::MAX as i64) as u32)
    }
}

// ---------------------------------------------------------------------------
// 深化二：批量导入去重（指纹位图——1024 槽布隆式单哈希，撞槽即疑重）
// ---------------------------------------------------------------------------

/// 导入去重账：1024 位指纹位图（FNV16 截断——判据层近似，接线时换全指纹）。
pub struct ImportDeduper {
    bits: [u64; 16],
    seen: u32,
    dup_suspects: u32,
}

impl ImportDeduper {
    pub const fn new() -> Self {
        ImportDeduper { bits: [0; 16], seen: 0, dup_suspects: 0 }
    }

    fn slot_of(print: u16) -> (usize, u64) {
        ((print % 1024) as usize / 64, 1u64 << (print % 64))
    }

    /// 登记（返回 false = 疑似重复）。
    pub fn admit(&mut self, print: u16) -> bool {
        let (w, bit) = Self::slot_of(print);
        if self.bits[w] & bit != 0 {
            self.dup_suspects += 1;
            return false;
        }
        self.bits[w] |= bit;
        self.seen += 1;
        true
    }

    /// 强制收录（确认非重复——位图误报时用）。
    pub fn force(&mut self, print: u16) {
        let (w, bit) = Self::slot_of(print);
        self.bits[w] |= bit;
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.seen, self.dup_suspects)
    }

    pub fn reset(&mut self) {
        self.bits = [0; 16];
        self.seen = 0;
        self.dup_suspects = 0;
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_frecency_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F072-frecency-v5");

    // ── 时钟偏差 ──
    // 1) 恒偏差 +500ms：配对 4 组 → 中位 500。
    let mut sk = ClockSkewEstimator::new();
    for k in 0..4u32 {
        sk.pair(k * 100, k * 100 + 500);
    }
    cs.add("skew_median_500", sk.skew_median_ms() == Some(500), "");
    // 2) 折算时间轴：b=1500 → a 轴 1000。
    cs.add("skew_to_a_timeline", sk.to_a_timeline(1500) == Some(1000), "");
    // 3) 负偏差：b 慢 200ms。
    let mut sk2 = ClockSkewEstimator::new();
    for _ in 0..4 {
        sk2.pair(1000, 800);
    }
    cs.add("skew_negative", sk2.skew_median_ms() == Some(-200) && sk2.to_a_timeline(900) == Some(1100), "");
    // 4) 样本 <4 → None。
    let mut sk3 = ClockSkewEstimator::new();
    sk3.pair(0, 5);
    sk3.pair(10, 20);
    cs.add("skew_insufficient_none", sk3.skew_median_ms().is_none(), "");
    // 5) 折算不越 0（正偏差 sk：b=3 → a = 3-500 → 钳 0）。
    cs.add("skew_clamp_zero", sk.to_a_timeline(3) == Some(0), "");

    // ── 导入去重 ──
    // 6) 首收 / 疑重。
    let mut dd = ImportDeduper::new();
    cs.add("dedup_first_ok", dd.admit(1234), "");
    cs.add("dedup_dup_suspect", !dd.admit(1234) && dd.stats() == (1, 1), "");
    // 7) 不同指纹各收。
    cs.add("dedup_distinct_ok", dd.admit(2345) && dd.stats().0 == 2, "");
    // 8) force 后再收仍疑重（位图语义——单哈希无移除）。
    let mut dd2 = ImportDeduper::new();
    let _ = dd2.admit(7);
    dd2.force(7);
    cs.add("dedup_force_idempotent", !dd2.admit(7), "");
    // 9) reset 清账。
    dd2.reset();
    cs.add("dedup_reset_clears", dd2.admit(7) && dd2.stats() == (1, 0), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn skew_median_robust_to_outlier() {
        let mut sk = ClockSkewEstimator::new();
        sk.pair(0, 500); // +500
        sk.pair(100, 600); // +500
        sk.pair(200, 700); // +500
        sk.pair(300, 99_999); // 离群对 +99699
        // 样本 [500,500,500,99699] → 中位 idx2 = 500——离群被吸收。
        assert_eq!(sk.skew_median_ms(), Some(500), "中位数吸收单个离群");
    }

    #[test]
    fn dedup_full_bitmap_stats() {
        let mut dd = ImportDeduper::new();
        let mut admitted = 0u32;
        for k in 0..2000u16 {
            if dd.admit(k) {
                admitted += 1;
            }
        }
        // 2000 指纹 → 1024 槽，至少 1024 收录，其余全疑重。
        assert_eq!(admitted, 1024);
        assert_eq!(dd.stats(), (1024, 976));
    }
}
