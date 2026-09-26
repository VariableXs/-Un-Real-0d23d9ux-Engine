//! F044 预取指纹 v2 · 深化件（AI-K1 深化批次三 · G-B-04）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【数据与存储】「指纹文件按应用存 `cache/prefetch/<hash>.pf`（页位图+访问序），**上限 8MB/应用**，**LRU 驱逐**；格式自定开放（F126）」 | [`PfHeader`] 开放格式头 + [`PfStore`] 8MB/应用配额账 + LRU 驱逐 |
//! | 2 | 【状态与异常】「程序更新（**版本变化**）→ 指纹失效重建；**指纹损坏 → 弃用走无预取路径（正确但慢，不报错）**；U 盘换机 → 指纹随 DATA 分区走」 | [`PfValidity`] 三态裁定（有效/版本失效/损坏）+ 弃用不报错语义 |
//! | 3 | 【设计细节】「预读 IO 按指纹序**批量 8 页一组**（对齐 BOT 顺序读优势）」 | [`PrefetchPlanner`] 八页成批 + 尾批不补齐（不伪造） |
//! | 4 | 【设计细节】「**预读优先级永远低于前台 IO**（F057 分级）」 | [`PfPriority`] 优先级常量与比较面（一处一事实） |
//! | 5 | 【设计细节】「指纹生成在**应用退出后 30 秒**后台完成（不抢退出体验）」 | [`GenSchedule`] 退出后延时 + 到期执行 + 取消（应用又启动了就取消） |
//! | 6 | 【验收判据】「**指纹命中率 >70%** 实测」 | [`HitRateMeter`] 命中率千分账 |

use crate::checks::CheckSet;
use crate::perfstar::frameledger_ext::crc32;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 指纹文件魔数（`VXPF`）。
pub const PF_MAGIC: u32 = 0x4650_4656;
/// 格式版本（变更走 ADR；F126 开放格式宪法要求版本化）。
pub const PF_VERSION: u16 = 1;
/// 头部字节数：魔数 4 + 版本 2 + 保留 2 + 应用版本 8 + 页数 4 + 位图字数 4 +
/// 访问序长度 4 + 位图 CRC 4 + 访问序 CRC 4 + 头 CRC 4 = 40。
pub const PF_HEADER_BYTES: usize = 40;
/// 单应用指纹上限 8MB（主册【数据与存储】）。
pub const PF_MAX_BYTES_PER_APP: usize = 8 * 1024 * 1024;
/// 批量 8 页一组（主册【设计细节】）。
pub const PF_BATCH_PAGES: usize = 8;
/// 指纹生成延时 30 秒（主册【设计细节】「应用退出后 30 秒后台完成」）。
pub const GEN_DELAY_MS: u64 = 30_000;
/// 命中率红线 70%（千分 = 700）。
pub const HIT_REDLINE_PERMILLE: u32 = 700;
/// 指纹库可容纳应用数（定长环；超了按 LRU 驱逐最久未用）。
pub const PF_STORE_APPS: usize = 32;

// ---------------------------------------------------------------------------
// 1. 指纹文件格式（开放格式 F126：有魔数、有版本、有 CRC、字段自解释）
// ---------------------------------------------------------------------------

/// 指纹文件头（定长 40B，落 `cache/prefetch/<hash>.pf`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PfHeader {
    pub app_ver: u64,
    /// 该应用被追踪的页数。
    pub page_count: u32,
    /// 位图字数（u64 字）。
    pub bitmap_words: u32,
    /// 访问序条目数。
    pub order_len: u32,
    /// 位图区 CRC32。
    pub bitmap_crc: u32,
    /// 访问序区 CRC32。
    pub order_crc: u32,
}

/// 头部读写结果（错误显式化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfIo {
    Ok,
    ShortBuffer(usize),
    BadMagic,
    BadVersion(u16),
    BadCrc,
}

impl PfIo {
    pub fn is_ok(&self) -> bool {
        matches!(self, PfIo::Ok)
    }
}

impl PfHeader {
    /// 负载字节数（位图 + 访问序；访问序条目按 u32 页号计）。
    pub fn payload_bytes(&self) -> usize {
        (self.bitmap_words as usize) * 8 + (self.order_len as usize) * 4
    }
    /// 文件总字节数（头 + 负载）。
    pub fn total_bytes(&self) -> usize {
        PF_HEADER_BYTES + self.payload_bytes()
    }
    /// 是否超出单应用 8MB 上限（超了就不写——不写坏文件）。
    pub fn within_budget(&self) -> bool {
        self.total_bytes() <= PF_MAX_BYTES_PER_APP
    }

    pub fn encode(&self, out: &mut [u8]) -> PfIo {
        if out.len() < PF_HEADER_BYTES {
            return PfIo::ShortBuffer(PF_HEADER_BYTES);
        }
        let mut p = 0usize;
        out[p..p + 4].copy_from_slice(&PF_MAGIC.to_le_bytes());
        p += 4;
        out[p..p + 2].copy_from_slice(&PF_VERSION.to_le_bytes());
        p += 2;
        out[p..p + 2].copy_from_slice(&0u16.to_le_bytes());
        p += 2;
        out[p..p + 8].copy_from_slice(&self.app_ver.to_le_bytes());
        p += 8;
        out[p..p + 4].copy_from_slice(&self.page_count.to_le_bytes());
        p += 4;
        out[p..p + 4].copy_from_slice(&self.bitmap_words.to_le_bytes());
        p += 4;
        out[p..p + 4].copy_from_slice(&self.order_len.to_le_bytes());
        p += 4;
        out[p..p + 4].copy_from_slice(&self.bitmap_crc.to_le_bytes());
        p += 4;
        out[p..p + 4].copy_from_slice(&self.order_crc.to_le_bytes());
        p += 4;
        let h = crc32(&out[..p]);
        out[p..p + 4].copy_from_slice(&h.to_le_bytes());
        PfIo::Ok
    }

    pub fn decode(buf: &[u8]) -> (PfIo, Option<PfHeader>) {
        if buf.len() < PF_HEADER_BYTES {
            return (PfIo::ShortBuffer(PF_HEADER_BYTES), None);
        }
        let mut p = 0usize;
        let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        p += 4;
        if magic != PF_MAGIC {
            return (PfIo::BadMagic, None);
        }
        let ver = u16::from_le_bytes([buf[p], buf[p + 1]]);
        p += 2;
        if ver != PF_VERSION {
            return (PfIo::BadVersion(ver), None);
        }
        p += 2; // reserved
        let body_end = PF_HEADER_BYTES - 4;
        let want = u32::from_le_bytes([buf[body_end], buf[body_end + 1], buf[body_end + 2], buf[body_end + 3]]);
        if crc32(&buf[..body_end]) != want {
            return (PfIo::BadCrc, None);
        }
        let app_ver = u64::from_le_bytes([
            buf[p], buf[p + 1], buf[p + 2], buf[p + 3], buf[p + 4], buf[p + 5], buf[p + 6], buf[p + 7],
        ]);
        p += 8;
        let page_count = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        p += 4;
        let bitmap_words = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        p += 4;
        let order_len = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        p += 4;
        let bitmap_crc = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        p += 4;
        let order_crc = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        (
            PfIo::Ok,
            Some(PfHeader { app_ver, page_count, bitmap_words, order_len, bitmap_crc, order_crc }),
        )
    }
}

// ---------------------------------------------------------------------------
// 2. 有效性裁定（状态与异常三态）
// ---------------------------------------------------------------------------

/// 指纹有效性（主册【状态与异常】三条逐条对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfValidity {
    /// 版本一致且 CRC 通过 → 可用。
    Valid,
    /// 程序更新（版本变化）→ 失效重建（不是错误，是预期内的失效）。
    VersionStale { stored: u64, current: u64 },
    /// 指纹损坏 → 弃用走无预取路径（正确但慢，不报错）。
    Corrupt,
}

impl PfValidity {
    /// 是否可以使用指纹预读。
    pub fn usable(&self) -> bool {
        matches!(self, PfValidity::Valid)
    }
    /// 是否属于「不报错」的静默降级（主册原文：正确但慢，不报错）。
    pub fn silent_degrade(&self) -> bool {
        !self.usable()
    }
    /// 裁定：头部读回结果 + 版本比对。
    pub fn judge(io: PfIo, stored_ver: u64, current_ver: u64) -> PfValidity {
        match io {
            PfIo::Ok => {
                if stored_ver == current_ver {
                    PfValidity::Valid
                } else {
                    PfValidity::VersionStale { stored: stored_ver, current: current_ver }
                }
            }
            // 头 CRC 坏/魔数坏/版本不支持：一律按损坏处理 → 弃用不报错
            PfIo::BadCrc | PfIo::BadMagic | PfIo::BadVersion(_) | PfIo::ShortBuffer(_) => PfValidity::Corrupt,
        }
    }
}

// ---------------------------------------------------------------------------
// 3. 指纹库（8MB/应用 + LRU 驱逐 + DATA 分区随身）
// ---------------------------------------------------------------------------

/// 一条指纹登记项。
#[derive(Clone, Copy, Debug)]
pub struct PfEntry {
    /// 应用哈希（文件名 `<hash>.pf` 的来源）。
    pub app_hash: u64,
    pub bytes: usize,
    pub last_used_ms: u64,
}

/// 指纹库：定长表 + LRU 驱逐 + 配额账。
pub struct PfStore {
    entries: [Option<PfEntry>; PF_STORE_APPS],
    n: usize,
    /// 因超配额被拒的写入次数（零静默：被拒要能被问到）。
    pub rejected_over_budget: u64,
    /// LRU 驱逐次数。
    pub evictions: u64,
}

impl PfStore {
    pub const fn new() -> Self {
        PfStore { entries: [None; PF_STORE_APPS], n: 0, rejected_over_budget: 0, evictions: 0 }
    }

    fn find(&self, hash: u64) -> Option<usize> {
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                if e.app_hash == hash {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 登记/更新一条指纹。`bytes` 超 8MB → 拒绝并记录（不写坏文件）。
    pub fn put(&mut self, hash: u64, bytes: usize, now_ms: u64) -> bool {
        if bytes > PF_MAX_BYTES_PER_APP {
            self.rejected_over_budget += 1;
            return false;
        }
        if let Some(i) = self.find(hash) {
            if let Some(e) = self.entries[i].as_mut() {
                e.bytes = bytes;
                e.last_used_ms = now_ms;
            }
            return true;
        }
        if self.n >= PF_STORE_APPS {
            // LRU 驱逐最久未用者
            let mut victim = 0usize;
            let mut oldest = u64::MAX;
            for i in 0..self.n {
                if let Some(e) = self.entries[i] {
                    if e.last_used_ms < oldest {
                        oldest = e.last_used_ms;
                        victim = i;
                    }
                }
            }
            self.entries[victim] = None;
            self.evictions += 1;
            // 紧凑化：把被驱逐位之后的元素前移（定长表，零堆）
            for i in victim..self.n - 1 {
                self.entries[i] = self.entries[i + 1];
            }
            self.entries[self.n - 1] = None;
            self.n -= 1;
        }
        self.entries[self.n] = Some(PfEntry { app_hash: hash, bytes, last_used_ms: now_ms });
        self.n += 1;
        true
    }

    pub fn get(&self, hash: u64) -> Option<PfEntry> {
        self.find(hash).and_then(|i| self.entries[i])
    }

    /// 库内总字节（诊断面展示用）。
    pub fn total_bytes(&self) -> usize {
        self.entries.iter().filter_map(|e| e.map(|v| v.bytes)).sum()
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 4. 预读计划（八页成批，尾批不补齐）
// ---------------------------------------------------------------------------

/// 预读批次（8 页一组，对齐 BOT 顺序读优势）。
#[derive(Clone, Copy, Debug)]
pub struct PfBatch {
    pub pages: [u32; PF_BATCH_PAGES],
    pub len: usize,
    /// 是否为本轮最后一批（尾批可能不满 8 页——不补齐，不伪造）。
    pub last: bool,
}

/// 预读计划器：按访问序把热页切成八页批。
pub struct PrefetchPlanner {
    pub pos: usize,
}

impl PrefetchPlanner {
    pub const fn new() -> Self {
        PrefetchPlanner { pos: 0 }
    }
    /// 取下一批。`order` 为按热度排序的页号序列。
    pub fn next_batch(&mut self, order: &[u32]) -> Option<PfBatch> {
        if self.pos >= order.len() {
            return None;
        }
        let mut pages = [0u32; PF_BATCH_PAGES];
        let mut len = 0usize;
        while len < PF_BATCH_PAGES && self.pos < order.len() {
            pages[len] = order[self.pos];
            len += 1;
            self.pos += 1;
        }
        Some(PfBatch { pages, len, last: self.pos >= order.len() })
    }
    /// 剩余批数（进度面用：诚实预估，尾批计入）。
    pub fn remaining_batches(&self, order: &[u32]) -> usize {
        let left = order.len().saturating_sub(self.pos);
        (left + PF_BATCH_PAGES - 1) / PF_BATCH_PAGES
    }
}

/// 预读优先级（主册「预读优先级永远低于前台 IO」）。
pub struct PfPriority;

impl PfPriority {
    /// 前台交互级（F057 三级队列的第一级）。
    pub const FG: u8 = 0;
    /// 后台任务级。
    pub const BG: u8 = 1;
    /// 批量级。
    pub const BATCH: u8 = 2;
    /// 预读所属级：**永远低于前台**——预读落在后台级。
    pub const PREFETCH: u8 = Self::BG;
    /// 比较：数值越小优先级越高。预读不得高于前台。
    pub fn prefetch_below_foreground() -> bool {
        Self::PREFETCH > Self::FG
    }
}

// ---------------------------------------------------------------------------
// 5. 生成调度（退出后 30 秒后台，不抢退出体验）
// ---------------------------------------------------------------------------

/// 指纹生成调度：退出后延时生成；应用又启动了就取消（不打断正在用的应用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenState {
    Idle,
    /// 已排期，`at_ms` 为到期时刻。
    Scheduled { at_ms: u64 },
    /// 正在生成。
    Running,
}

pub struct GenSchedule {
    pub state: GenState,
    /// 到期却因前台忙而推迟的次数（不抢体验的代价记账）。
    pub postponed: u32,
    /// 被取消次数（应用又启动了）。
    pub cancelled: u32,
    /// 完成次数。
    pub done: u32,
}

impl GenSchedule {
    pub const fn new() -> Self {
        GenSchedule { state: GenState::Idle, postponed: 0, cancelled: 0, done: 0 }
    }
    /// 应用退出：排期到 exit_ms + 30s。
    pub fn on_exit(&mut self, exit_ms: u64) {
        self.state = GenState::Scheduled { at_ms: exit_ms.saturating_add(GEN_DELAY_MS) };
    }
    /// 应用又启动：取消（用户正在用，别抢）。
    pub fn on_launch(&mut self) {
        if !matches!(self.state, GenState::Idle) {
            self.cancelled += 1;
        }
        self.state = GenState::Idle;
    }
    /// 心跳：`busy` = 前台是否繁忙（繁忙则推迟，不抢）。
    pub fn tick(&mut self, now_ms: u64, busy: bool) -> bool {
        match self.state {
            GenState::Scheduled { at_ms } => {
                if now_ms < at_ms {
                    return false;
                }
                if busy {
                    self.postponed += 1;
                    // 推迟 5 秒再试（不无限推迟：postponed 计数可查）
                    self.state = GenState::Scheduled { at_ms: now_ms.saturating_add(5_000) };
                    return false;
                }
                self.state = GenState::Running;
                true
            }
            _ => false,
        }
    }
    /// 生成完成。
    pub fn finish(&mut self) {
        if matches!(self.state, GenState::Running) {
            self.done += 1;
            self.state = GenState::Idle;
        }
    }
}

// ---------------------------------------------------------------------------
// 6. 命中率账（判据 >70%）
// ---------------------------------------------------------------------------

/// 命中率账：预读的页里有多少真的被用到（预取的聪明在于只预取真的）。
#[derive(Clone, Copy, Debug)]
pub struct HitRateMeter {
    pub prefetched: u32,
    pub hit: u32,
}

impl HitRateMeter {
    pub const fn new() -> Self {
        HitRateMeter { prefetched: 0, hit: 0 }
    }
    pub fn feed(&mut self, prefetched: u32, hit: u32) {
        self.prefetched = self.prefetched.saturating_add(prefetched);
        self.hit = self.hit.saturating_add(hit.min(prefetched));
    }
    pub fn permille(&self) -> u32 {
        if self.prefetched == 0 {
            return 0;
        }
        ((self.hit as u64 * 1000) / self.prefetched as u64) as u32
    }
    pub fn passes(&self) -> bool {
        self.prefetched > 0 && self.permille() > HIT_REDLINE_PERMILLE
    }
    /// 浪费的预读页数（预读了却没用到——调参依据，入账本）。
    pub fn wasted(&self) -> u32 {
        self.prefetched.saturating_sub(self.hit)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F044-prefetch2-ext");
    // 1) 开放格式头 round-trip（F126 要求格式自解释且可校验）。
    let h = PfHeader { app_ver: 0xDEAD_BEEF, page_count: 4_096, bitmap_words: 64, order_len: 512, bitmap_crc: 1, order_crc: 2 };
    let mut buf = [0u8; PF_HEADER_BYTES];
    let enc = h.encode(&mut buf);
    let (dec, got) = PfHeader::decode(&buf);
    cs.add("pf_header_roundtrip", enc.is_ok() && dec.is_ok() && got == Some(h), "");
    // 2) 头 CRC 坏 → Corrupt（弃用不报错，不产出半对的文件）。
    let mut bad = buf;
    bad[10] ^= 0x40;
    cs.add("pf_bad_crc_is_corrupt", matches!(PfHeader::decode(&bad).0, PfIo::BadCrc), "");
    // 3) 未来版本 → BadVersion 带版本号（不静默丢弃）。
    let mut bv = buf;
    bv[4] = 7;
    cs.add("pf_bad_version_reported", matches!(PfHeader::decode(&bv).0, PfIo::BadVersion(7)), "");
    // 4) 8MB/应用配额：超出即拒（不写坏文件）。
    cs.add(
        "pf_budget_8mb_per_app",
        PF_MAX_BYTES_PER_APP == 8 * 1024 * 1024
            && h.within_budget()
            && !PfHeader { bitmap_words: 8 * 1024 * 1024 / 8, ..h }.within_budget(),
        "",
    );
    // 5) 版本失效 ≠ 损坏（前者预期内重建，后者弃用——两种处置不同）。
    let v_valid = PfValidity::judge(PfIo::Ok, 5, 5);
    let v_stale = PfValidity::judge(PfIo::Ok, 5, 6);
    let v_corrupt = PfValidity::judge(PfIo::BadCrc, 5, 5);
    cs.add(
        "pf_validity_three_states",
        v_valid.usable()
            && matches!(v_stale, PfValidity::VersionStale { stored: 5, current: 6 })
            && v_corrupt == PfValidity::Corrupt
            && v_stale.silent_degrade()
            && v_corrupt.silent_degrade(),
        "",
    );
    // 6) LRU 驱逐 + 配额拒绝计数。
    let mut st = PfStore::new();
    for i in 0..PF_STORE_APPS {
        st.put(i as u64, 1_024, 1_000 + i as u64);
    }
    let before = st.len();
    st.put(999, 1_024, 9_999_999); // 超容量 → 驱逐最久未用（i=0, last_used=1000）
    cs.add("pf_store_lru_evicts", before == PF_STORE_APPS && st.len() == PF_STORE_APPS && st.evictions == 1 && st.get(0).is_none(), "");
    // 7) 超 8MB 单应用写入被拒（零静默：拒绝次数可查）。
    let mut st2 = PfStore::new();
    let ok = st2.put(1, PF_MAX_BYTES_PER_APP + 1, 0);
    cs.add("pf_store_rejects_over_budget", !ok && st2.rejected_over_budget == 1, "");
    // 8) 八页成批 + 尾批不补齐（不伪造页）。
    let order: [u32; 20] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19];
    let mut pl = PrefetchPlanner::new();
    let b1 = pl.next_batch(&order).unwrap();
    let b2 = pl.next_batch(&order).unwrap();
    let b3 = pl.next_batch(&order).unwrap();
    cs.add(
        "pf_batch_eight_and_tail",
        b1.len == 8 && !b1.last && b2.len == 8 && !b2.last && b3.len == 4 && b3.last && pl.next_batch(&order).is_none(),
        "",
    );
    // 9) 剩余批数预估（尾批计入，20 页 → 3 批）。
    let mut pl2 = PrefetchPlanner::new();
    cs.add("pf_remaining_batches", pl2.remaining_batches(&order) == 3, "");
    pl2.next_batch(&order);
    cs.add("pf_remaining_decreases", pl2.remaining_batches(&order) == 2, "");
    // 10) 预读优先级永远低于前台（F057 分级联动）。
    cs.add("pf_priority_below_fg", PfPriority::prefetch_below_foreground() && PfPriority::PREFETCH == PfPriority::BG, "");
    // 11) 生成调度：退出后 30s 到期；前台忙则推迟（不抢退出/使用体验）。
    let mut g = GenSchedule::new();
    g.on_exit(1_000);
    cs.add("gen_not_due_before_30s", !g.tick(1_000 + 29_999, false), "");
    let due = g.tick(1_000 + 30_000, true); // 到期但前台忙 → 推迟
    cs.add("gen_postponed_when_busy", !due && g.postponed == 1, "");
    let due2 = g.tick(1_000 + 30_000 + 5_000, false);
    g.finish();
    cs.add("gen_runs_when_idle", due2 && g.done == 1 && matches!(g.state, GenState::Idle), "");
    // 12) 应用又启动 → 取消（用户正在用，别抢）。
    let mut g2 = GenSchedule::new();
    g2.on_exit(0);
    g2.on_launch();
    cs.add("gen_cancelled_on_relaunch", g2.cancelled == 1 && matches!(g2.state, GenState::Idle), "");
    // 13) 命中率账：>70% 达标；零预读不算达标（不粉饰）。
    let mut m = HitRateMeter::new();
    m.feed(100, 80);
    cs.add("hit_rate_passes", m.passes() && m.permille() == 800 && m.wasted() == 20, "");
    let mut m2 = HitRateMeter::new();
    m2.feed(100, 60);
    cs.add("hit_rate_below_redline_fails", !m2.passes(), "");
    let m3 = HitRateMeter::new();
    cs.add("hit_rate_zero_samples_not_pass", !m3.passes(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_payload_and_budget_math_is_consistent() {
        let h = PfHeader { app_ver: 1, page_count: 512, bitmap_words: 8, order_len: 512, bitmap_crc: 0, order_crc: 0 };
        assert_eq!(h.payload_bytes(), 8 * 8 + 512 * 4);
        assert_eq!(h.total_bytes(), PF_HEADER_BYTES + h.payload_bytes());
        assert!(h.within_budget());
    }

    #[test]
    fn store_total_bytes_sums_entries() {
        let mut s = PfStore::new();
        s.put(1, 1_000, 0);
        s.put(2, 2_000, 1);
        assert_eq!(s.total_bytes(), 3_000);
        // 更新同一应用不新增条目
        s.put(1, 5_000, 2);
        assert_eq!(s.len(), 2);
        assert_eq!(s.total_bytes(), 7_000);
    }

    #[test]
    fn planner_tail_batch_can_be_exactly_eight() {
        let order: [u32; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1];
        let mut p = PrefetchPlanner::new();
        let a = p.next_batch(&order).unwrap();
        let b = p.next_batch(&order).unwrap();
        assert_eq!(a.len, 8);
        assert_eq!(b.len, 8);
        assert!(b.last, "恰好整除时末批满 8 也是最后一批");
    }

    #[test]
    fn gen_schedule_idle_never_fires() {
        let mut g = GenSchedule::new();
        assert!(!g.tick(1_000_000, false));
        assert_eq!(g.done, 0);
    }

    #[test]
    fn hit_rate_clamps_hit_to_prefetched() {
        let mut m = HitRateMeter::new();
        m.feed(10, 999); // 命中数超过预读数 = 数据错乱，钳制而不是算出 99900‰
        assert_eq!(m.permille(), 1000);
        assert_eq!(m.wasted(), 0);
    }
}
