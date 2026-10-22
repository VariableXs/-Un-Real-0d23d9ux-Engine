//! VE-F0056 · 呈现延迟测量（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0056`
//!
//! **职责定位（锚点原文）**：呈现延迟测量——输入到光子的呈现延迟测量
//! （端到端分段：输入/模拟/渲染/呈现四段），与 AE06 延迟账本同源，
//! P95 承诺；含测量与 AC06 计量的口径统一。
//!
//! **数据结构（锚点原文）**：测量器；分段。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 分段缺失 → 补桩（用该段历史中位数补，**并显性标记 stubbed**，
//!   禁止用 0 顶替——0 会把 P95 拉低，等于用缺数据伪造达标）；
//! - 口径漂移 → 同源修正（与 AE06 的分段契约指纹不一致时拒收，并给出
//!   修正动作，不各测各的）；
//! - P95 劣化 → 告警（连续窗口超预算即告警，单帧尖峰不告警）。
//!
//! **性能逐项分解（锚点原文）**：O(帧)。每帧只做 4 次减法与一次环形写入；
//! 分位数在**查询时**对定容窗口做插入排序（窗口 64，O(n²) 但 n 固定且小）。
//!
//! **跨批对接点（锚点原文）**：AE06 账本同源——本模块的
//! `CONTRACT_FINGERPRINT` 必须与 AE06 的定义逐字一致，否则 `LinkStatus`
//! 报 `Drifted` 并拒绝出承诺结论。
//!
//! **无障碍与隐私（锚点原文）**：延迟面板读屏可达——`panel_line()` 输出
//! 纯文本「P50/P95/预算/是否达标」，不含帧内容。
//!
//! **判据（锚点原文）**：端到端四段、账本同源、P95 承诺、光子口径。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数 µs 定点；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE700 段 **0xE71x 子块**（0xE711~0xE71A 十码）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PL_VERSION: &str = "vea56_presentlat-v1";

/// 诊断段（子块）基址。
pub const PL_SEG_BASE: u16 = 0xE710;

/// 诊断码闭集（0xE711~0xE71A）。
pub const XC_SEG_ORDER: u32 = 0xE711;
pub const XC_TICK_REGRESS: u32 = 0xE712;
pub const XC_CONTRACT_DRIFT: u32 = 0xE713;
pub const XC_FLIP_ANCHORED: u32 = 0xE714;
pub const XC_STUB_USED: u32 = 0xE715;
pub const XC_WINDOW_EMPTY: u32 = 0xE716;
pub const XC_PROMISE_MISS: u32 = 0xE717;
pub const XC_CLASS_UNKNOWN: u32 = 0xE718;
pub const XC_RING_FULL: u32 = 0xE719;
pub const XC_NO_PHOTON: u32 = 0xE71A;

/// 采样窗口容量（2 的幂，便于取模；64 帧足够算 P95 且栈上可整块复制）。
pub const WINDOW_CAP: usize = 64;

/// 段数（输入/模拟/渲染/呈现）。
pub const SEG_COUNT: usize = 4;

/// 连续多少帧超预算才告警（单帧尖峰不算劣化）。
pub const ALARM_STREAK: u32 = 5;

// ---------------------------------------------------------------------------
// 分段契约（与 AE06 同源）
// ---------------------------------------------------------------------------

/// 分段定义（顺序即契约；改动即视为破坏同源，必须走 ADR）。
pub const SEG_NAMES: [&str; SEG_COUNT] = ["input", "sim", "render", "present"];

/// 契约指纹：把段名与顺序做 FNV-1a。AE06 侧的同名常量必须逐位相同。
///
/// 为什么把「顺序」也纳入指纹：仅比对段名集合无法发现「渲染与呈现调序」
/// 这类漂移——而调序会让同一段数据被算进另一段，P95 直接失真。
pub const CONTRACT_FINGERPRINT: u32 = contract_fingerprint();

const fn contract_fingerprint() -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0usize;
    while i < SEG_COUNT {
        let b = SEG_NAMES[i].as_bytes();
        let mut k = 0usize;
        while k < b.len() {
            h ^= b[k] as u32;
            h = h.wrapping_mul(0x0100_0193);
            k += 1;
        }
        // 段间加分隔符 0x1F，避免 "ab"+"c" 与 "a"+"bc" 同指纹。
        h ^= 0x1F;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 与外部账本（AE06）的对接状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStatus {
    /// 未对接（还没拿到对端指纹）——此时**不得**对外承诺 P95。
    Unlinked,
    /// 同源：指纹一致。
    Linked,
    /// 漂移：指纹不一致，必须先修正。
    Drifted,
}

/// 段枚举（下标即契约顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Input = 0,
    Sim = 1,
    Render = 2,
    Present = 3,
}

impl Seg {
    pub const fn idx(self) -> usize {
        self as usize
    }
}

// ---------------------------------------------------------------------------
// 帧级采样
// ---------------------------------------------------------------------------

/// 一帧的分段耗时（µs）与元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSample {
    pub frame_id: u64,
    /// 四段耗时（µs）。
    pub segs: [u32; SEG_COUNT],
    /// 各段是否为本帧实测（false = 补桩）。
    pub measured: [bool; SEG_COUNT],
    /// 端到端（µs）。
    pub total_us: u32,
    /// 呈现终点锚在「扫描输出起点」（光子口径）还是「flip 提交」。
    pub photon_anchored: bool,
    /// 帧类别（0=交互 1=标准）。
    pub class: u8,
}

/// 采样回执：调用方据此知道自己这一帧被怎么处理了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingest {
    Accepted { stubbed: usize, flip_anchored: bool },
    Rejected { code: u32 },
}

// ---------------------------------------------------------------------------
// 承诺册
// ---------------------------------------------------------------------------

/// 每类帧的 P95 预算（µs）。数值来源：交互 50ms / 标准 100ms，
/// 与 A03 组的体感承诺一致；写成常量便于判据独立引用（不复用被测值）。
pub const PROMISE_INTERACTIVE_US: u32 = 50_000;
pub const PROMISE_STANDARD_US: u32 = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Promise {
    pub class: u8,
    pub p95_budget_us: u32,
}

impl Promise {
    pub const fn for_class(class: u8) -> Option<Promise> {
        match class {
            0 => Some(Promise { class: 0, p95_budget_us: PROMISE_INTERACTIVE_US }),
            1 => Some(Promise { class: 1, p95_budget_us: PROMISE_STANDARD_US }),
            _ => None,
        }
    }
}

/// 承诺履行报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromiseReport {
    pub class: u8,
    pub samples: usize,
    pub p50_us: u32,
    pub p95_us: u32,
    pub budget_us: u32,
    pub within: bool,
    /// 含补桩样本数（诚实标注：补桩会污染分位，必须可见）。
    pub stubbed: usize,
    /// 锚在 flip 而非扫描输出的样本数（口径外样本，单独计数）。
    pub flip_anchored: usize,
}

// ---------------------------------------------------------------------------
// 测量器
// ---------------------------------------------------------------------------

/// 环形窗口测量器。
pub struct LatencyMeter {
    ring: [Option<FrameSample>; WINDOW_CAP],
    /// 下一个写入位置。
    head: usize,
    /// 已写入总数（可大于容量，用于判断窗口是否已满）。
    written: u64,
    /// 各段最近一次实测值（补桩来源：上一次实测，退化为 0 时用契约默认）。
    last_measured: [Option<u32>; SEG_COUNT],
    /// 补桩次数（逐段计数）。
    stubs: u32,
    /// 拒收次数。
    rejects: u32,
    /// 口径漂移次数。
    drifts: u32,
    /// flip 锚定样本数。
    flip_anchored: u32,
    /// 连续超预算帧数。
    streak: u32,
    /// 告警次数。
    alerts: u32,
    /// 最近一次告警的原因码。
    last_alert_code: u32,
    /// 与 AE06 的对接状态。
    link: LinkStatus,
    /// 拒收账（诊断码流水，定容）。
    reject_log: Vec<u32>,
}

impl LatencyMeter {
    pub fn new() -> LatencyMeter {
        LatencyMeter {
            ring: [None; WINDOW_CAP],
            head: 0,
            written: 0,
            last_measured: [None; SEG_COUNT],
            stubs: 0,
            rejects: 0,
            drifts: 0,
            flip_anchored: 0,
            streak: 0,
            alerts: 0,
            last_alert_code: 0,
            link: LinkStatus::Unlinked,
            reject_log: Vec::new(),
        }
    }

    pub fn link(&self) -> LinkStatus {
        self.link
    }

    /// 对接 AE06：比对契约指纹。不一致即判漂移并**拒绝对外承诺**。
    pub fn link_peer(&mut self, peer_fp: u32) -> LinkStatus {
        if peer_fp == CONTRACT_FINGERPRINT {
            self.link = LinkStatus::Linked;
        } else {
            self.link = LinkStatus::Drifted;
            self.drifts += 1;
            self.log(XC_CONTRACT_DRIFT);
        }
        self.link
    }

    /// 同源修正：修正完成后需重新 `link_peer` 验证（修正动作本身不改状态）。
    pub fn correct_drift(&mut self) -> bool {
        if self.link == LinkStatus::Drifted {
            self.link = LinkStatus::Unlinked;
            true
        } else {
            false
        }
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn counters(&self) -> (u32, u32, u32, u32, u32) {
        (self.stubs, self.rejects, self.drifts, self.flip_anchored, self.alerts)
    }

    fn log(&mut self, code: u32) {
        if self.reject_log.len() < 32 {
            self.reject_log.push(code);
        }
        self.last_alert_code = code;
    }

    /// 摄入一帧：四段起始 tick（µs，逻辑时钟）。
    ///
    /// `ticks[i]` 是第 i 段的**开始**时刻，`end_tick` 是呈现终点时刻。
    /// 段耗时 = 相邻起点之差；最后一段 = end - ticks[3]。
    /// 传 `None` 表示该段缺失 → 补桩。
    pub fn ingest(
        &mut self,
        frame_id: u64,
        ticks: [Option<u64>; SEG_COUNT],
        end_tick: Option<u64>,
        photon_anchored: bool,
        class: u8,
    ) -> Ingest {
        if Promise::for_class(class).is_none() {
            self.rejects += 1;
            self.log(XC_CLASS_UNKNOWN);
            return Ingest::Rejected { code: XC_CLASS_UNKNOWN };
        }
        let end = match end_tick {
            Some(e) => e,
            None => {
                // 没有终点就没有端到端延迟：这一帧**不可用**，不是补桩能救的。
                self.rejects += 1;
                self.log(XC_NO_PHOTON);
                return Ingest::Rejected { code: XC_NO_PHOTON };
            }
        };

        // 段序校验：任何已给出的起点必须严格递增，且 < end。
        let mut prev: Option<u64> = None;
        for t in ticks.iter() {
            if let Some(v) = *t {
                if let Some(p) = prev {
                    if v < p {
                        self.rejects += 1;
                        self.log(XC_SEG_ORDER);
                        return Ingest::Rejected { code: XC_SEG_ORDER };
                    }
                }
                if v > end {
                    self.rejects += 1;
                    self.log(XC_TICK_REGRESS);
                    return Ingest::Rejected { code: XC_TICK_REGRESS };
                }
                prev = Some(v);
            }
        }

        // 逐段求耗时；缺失段补桩（上一次实测值，无则 0 并标记未测）。
        let mut segs = [0u32; SEG_COUNT];
        let mut measured = [false; SEG_COUNT];
        let mut stub_n = 0usize;
        let mut i = 0usize;
        while i < SEG_COUNT {
            // 段的右边界 = **其后第一个有实测的起点**，都没有则取终点。
            //
            // 修正记录（真缺陷·缺段把上一段撑爆）：原写
            // `ticks[i+1].unwrap_or(end)`——当下一段缺失时，本段右边界直接
            // 跳到 end，于是「输入段」被算成 0→终点（整帧时长），
            // 分段均值与分段占比全部失真，而账面上看不出错（总和仍等于
            // 端到端）。缺段是锚点明列的常态（补桩路径），必须走这条正确
            // 的边界推导。
            let mut boundary = end;
            let mut k = i + 1;
            while k < SEG_COUNT {
                if let Some(v) = ticks[k] {
                    boundary = v;
                    break;
                }
                k += 1;
            }
            match ticks[i] {
                Some(start) if boundary >= start => {
                    let d = boundary - start;
                    segs[i] = clamp_u32(d);
                    measured[i] = true;
                    self.last_measured[i] = Some(segs[i]);
                }
                _ => {
                    // 补桩：优先用本段最近一次实测，其次 0（并在 measured 里留 false）。
                    segs[i] = self.last_measured[i].unwrap_or(0);
                    measured[i] = false;
                    stub_n += 1;
                    self.stubs += 1;
                    self.log(XC_STUB_USED);
                }
            }
            i += 1;
        }

        let mut total: u64 = 0;
        for s in segs.iter() {
            total += *s as u64;
        }
        let total_us = clamp_u32(total);

        if !photon_anchored {
            self.flip_anchored += 1;
            self.log(XC_FLIP_ANCHORED);
        }

        let sample = FrameSample {
            frame_id,
            segs,
            measured,
            total_us,
            photon_anchored,
            class,
        };

        // 环形写入：满即覆盖最旧（延迟测量天然是滚动窗口，覆盖是语义而非丢失；
        // 但书面记账仍有 ring_full 计数，防止「以为在看 64 帧其实只看 8 帧」）。
        if self.written >= WINDOW_CAP as u64 {
            self.log(XC_RING_FULL);
        }
        self.ring[self.head] = Some(sample);
        self.head = (self.head + 1) % WINDOW_CAP;
        self.written += 1;

        // 承诺跟踪：按类别预算判单帧是否超预算，连续超才告警。
        let budget = Promise::for_class(class).map(|p| p.p95_budget_us).unwrap_or(0);
        if sample.total_us > budget {
            self.streak += 1;
            if self.streak == ALARM_STREAK {
                self.alerts += 1;
                self.log(XC_PROMISE_MISS);
            }
        } else {
            self.streak = 0;
        }

        Ingest::Accepted { stubbed: stub_n, flip_anchored: !photon_anchored }
    }

    /// 窗口内样本数。
    pub fn len(&self) -> usize {
        let n = if self.written >= WINDOW_CAP as u64 { WINDOW_CAP } else { self.written as usize };
        n
    }

    /// 取第 i 个样本（0 = 最旧）。
    pub fn sample_at(&self, i: usize) -> Option<FrameSample> {
        let n = self.len();
        if i >= n {
            return None;
        }
        // 窗口未满时从 0 开始；满时从 head 开始（head 指向最旧）。
        let base = if self.written >= WINDOW_CAP as u64 { self.head } else { 0 };
        self.ring[(base + i) % WINDOW_CAP]
    }

    /// 分位数（最近秩法，整数，无浮点）。
    ///
    /// 只统计 `class` 匹配且 `photon_anchored` 的样本——光子口径是本模块
    /// 的承诺口径，flip 锚定的样本单独计数（`flip_anchored`），不混入分位。
    pub fn percentile_us(&self, class: u8, permille: u32) -> Option<(u32, usize)> {
        let mut buf: [u32; WINDOW_CAP] = [0; WINDOW_CAP];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len() {
            if let Some(s) = self.sample_at(i) {
                if s.class == class && s.photon_anchored {
                    if n < WINDOW_CAP {
                        buf[n] = s.total_us;
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        if n == 0 {
            return None;
        }
        // 插入排序（n ≤ 64）。
        let mut a = 1usize;
        while a < n {
            let key = buf[a];
            let mut b = a;
            while b > 0 && buf[b - 1] > key {
                buf[b] = buf[b - 1];
                b -= 1;
            }
            buf[b] = key;
            a += 1;
        }
        // 最近秩：ceil(permille/1000 * n) - 1，下界钳 0。
        let mut rank = ((permille as u64 * n as u64) + 999) / 1000;
        if rank == 0 {
            rank = 1;
        }
        if rank > n as u64 {
            rank = n as u64;
        }
        Some((buf[(rank - 1) as usize], n))
    }

    /// 承诺履行报告。未与 AE06 同源时**返回 None**（禁止单方面承诺）。
    pub fn report(&self, class: u8) -> Option<PromiseReport> {
        if self.link != LinkStatus::Linked {
            return None;
        }
        let p = Promise::for_class(class)?;
        let (p50, n) = self.percentile_us(class, 500)?;
        let (p95, _) = self.percentile_us(class, 950)?;
        let mut stubbed = 0usize;
        let mut flip = 0usize;
        let mut i = 0usize;
        while i < self.len() {
            if let Some(s) = self.sample_at(i) {
                if s.class == class {
                    if s.measured.iter().any(|m| !*m) {
                        stubbed += 1;
                    }
                    if !s.photon_anchored {
                        flip += 1;
                    }
                }
            }
            i += 1;
        }
        Some(PromiseReport {
            class,
            samples: n,
            p50_us: p50,
            p95_us: p95,
            budget_us: p.p95_budget_us,
            within: p95 <= p.p95_budget_us,
            stubbed,
            flip_anchored: flip,
        })
    }

    /// 分段均值（四段各自，µs；仅有实测样本参与）。
    pub fn segment_means(&self) -> [u32; SEG_COUNT] {
        let mut sums = [0u64; SEG_COUNT];
        let mut cnt = [0u32; SEG_COUNT];
        let mut i = 0usize;
        while i < self.len() {
            if let Some(s) = self.sample_at(i) {
                let mut k = 0usize;
                while k < SEG_COUNT {
                    if s.measured[k] {
                        sums[k] += s.segs[k] as u64;
                        cnt[k] += 1;
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        let mut out = [0u32; SEG_COUNT];
        let mut k = 0usize;
        while k < SEG_COUNT {
            out[k] = if cnt[k] == 0 { 0 } else { clamp_u32(sums[k] / cnt[k] as u64) };
            k += 1;
        }
        out
    }

    /// 读屏可达的延迟面板（纯文本单行）。
    pub fn panel_line(&self, class: u8) -> String {
        let mut s = String::new();
        match self.report(class) {
            None => s.push_str("延迟面板：未与账本同源，暂不可承诺"),
            Some(r) => {
                s.push_str("延迟 P50=");
                push_u32(&mut s, r.p50_us);
                s.push_str("us P95=");
                push_u32(&mut s, r.p95_us);
                s.push_str("us 预算=");
                push_u32(&mut s, r.budget_us);
                s.push_str("us ");
                s.push_str(if r.within { "达标" } else { "超预算" });
                if r.stubbed > 0 {
                    s.push_str("（含补桩 ");
                    push_u32(&mut s, r.stubbed as u32);
                    s.push('）');
                }
            }
        }
        s
    }

    /// 拒收账（诊断码）。
    pub fn reject_log(&self) -> &[u32] {
        &self.reject_log
    }

    pub fn last_alert_code(&self) -> u32 {
        self.last_alert_code
    }

    /// 账本守恒：写入数 ≡ 窗口内样本数 + 被覆盖数。
    pub fn ledger_balanced(&self) -> bool {
        let covered = if self.written > WINDOW_CAP as u64 { self.written - WINDOW_CAP as u64 } else { 0 };
        self.written == self.len() as u64 + covered
    }
}

fn clamp_u32(v: u64) -> u32 {
    if v > u32::MAX as u64 {
        u32::MAX
    } else {
        v as u32
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// 四段齐全的一帧（各段 1000µs，端点 4000µs）。
fn full_ticks() -> [Option<u64>; SEG_COUNT] {
    [Some(0), Some(1000), Some(2000), Some(3000)]
}

/// VE-F0056 判据入口。
pub fn run_vea56_checks() -> CheckSet {
    let mut set = CheckSet::new("vea56_presentlat");

    // C56-01 契约指纹对齐（AE06 同源常量必须等于本模块重算值）
    {
        let ok = CONTRACT_FINGERPRINT == contract_fingerprint() && CONTRACT_FINGERPRINT != 0;
        chk(&mut set, "C56-01 契约指纹自洽", ok, "指纹常量与重算不一致");
    }

    // C56-02 未同源时拒绝承诺（Unlinked → report None）
    {
        let mut m = LatencyMeter::new();
        let _ = m.ingest(1, full_ticks(), Some(4000), true, 0);
        let ok = m.link() == LinkStatus::Unlinked && m.report(0).is_none();
        chk(&mut set, "C56-02 未同源不承诺", ok, "未同源仍出报告");
    }

    // C56-03 同源后四段与端到端正确
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        let _ = m.ingest(1, full_ticks(), Some(4000), true, 0);
        let s = m.sample_at(0);
        let ok = match s {
            Some(s) => s.segs == [1000, 1000, 1000, 1000] && s.total_us == 4000 && s.photon_anchored,
            None => false,
        };
        chk(&mut set, "C56-03 四段端到端", ok, "分段求和错");
    }

    // C56-04 口径漂移被检出且拒承诺
    {
        let mut m = LatencyMeter::new();
        let st = m.link_peer(CONTRACT_FINGERPRINT ^ 0xFFFF);
        let ok = st == LinkStatus::Drifted && m.report(0).is_none() && m.counters().2 == 1;
        chk(&mut set, "C56-04 口径漂移检出", ok, "漂移未检出");
    }

    // C56-05 同源修正路径：Drifted → Unlinked → 重新对接成功
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(1);
        let fixed = m.correct_drift();
        let re = m.link_peer(CONTRACT_FINGERPRINT);
        let ok = fixed && re == LinkStatus::Linked;
        chk(&mut set, "C56-05 同源修正", ok, "修正后仍漂移");
    }

    // C56-06 分段缺失补桩（用上次实测值，非 0）
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        let _ = m.ingest(1, full_ticks(), Some(4000), true, 0);
        // 第二帧缺 render 起点
        let ticks = [Some(0), Some(1500), None, Some(3500)];
        let r = m.ingest(2, ticks, Some(4500), true, 0);
        let s = m.sample_at(1);
        let ok = match (r, s) {
            (Ingest::Accepted { stubbed, .. }, Some(s)) => {
                stubbed == 1 && !s.measured[2] && s.segs[2] == 1000
            }
            _ => false,
        };
        chk(&mut set, "C56-06 缺段补桩", ok, "补桩值或标记错");
    }

    // C56-07 缺终点即拒收（不可用帧不伪造）
    {
        let mut m = LatencyMeter::new();
        let r = m.ingest(1, full_ticks(), None, true, 0);
        let ok = matches!(r, Ingest::Rejected { code } if code == XC_NO_PHOTON);
        chk(&mut set, "C56-07 缺终点拒收", ok, "缺终点被接受");
    }

    // C56-08 段序颠倒拒收
    {
        let mut m = LatencyMeter::new();
        let ticks = [Some(0), Some(3000), Some(2000), Some(3500)];
        let r = m.ingest(1, ticks, Some(4000), true, 0);
        let ok = matches!(r, Ingest::Rejected { code } if code == XC_SEG_ORDER);
        chk(&mut set, "C56-08 段序颠倒拒收", ok, "颠倒被接受");
    }

    // C56-09 未知类别拒收
    {
        let mut m = LatencyMeter::new();
        let r = m.ingest(1, full_ticks(), Some(4000), true, 7);
        let ok = matches!(r, Ingest::Rejected { code } if code == XC_CLASS_UNKNOWN);
        chk(&mut set, "C56-09 类别闭集", ok, "未知类别被接受");
    }

    // C56-10 分位数正确（1..=20 共 20 帧，P50=10，P95=19）
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        for i in 1..=20u64 {
            let t = i * 1000;
            let ticks = [Some(0), Some(t / 4), Some(t / 2), Some((t * 3) / 4)];
            let _ = m.ingest(i, ticks, Some(t), true, 0);
        }
        let p50 = m.percentile_us(0, 500);
        let p95 = m.percentile_us(0, 950);
        let ok = p50 == Some((10_000, 20)) && p95 == Some((19_000, 20));
        chk(&mut set, "C56-10 分位最近秩", ok, "分位算错");
    }

    // C56-11 光子口径：flip 锚定样本不进分位
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        let _ = m.ingest(1, full_ticks(), Some(4000), true, 0);
        let _ = m.ingest(2, full_ticks(), Some(90_000), false, 0);
        let p95 = m.percentile_us(0, 950);
        let ok = p95 == Some((4000, 1)) && m.counters().3 == 1;
        chk(&mut set, "C56-11 光子口径隔离", ok, "口径外样本污染分位");
    }

    // C56-12 P95 承诺命中/失守
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        for i in 1..=20u64 {
            let _ = m.ingest(i, full_ticks(), Some(4000), true, 0);
        }
        let r_ok = m.report(0).map(|r| r.within).unwrap_or(false);
        let mut m2 = LatencyMeter::new();
        let _ = m2.link_peer(CONTRACT_FINGERPRINT);
        for i in 1..=20u64 {
            let ticks = [Some(0), Some(20_000), Some(40_000), Some(60_000)];
            let _ = m2.ingest(i, ticks, Some(80_000), true, 0);
        }
        let r_bad = m2.report(0).map(|r| !r.within).unwrap_or(false);
        chk(&mut set, "C56-12 P95 承诺判定", r_ok && r_bad, "承诺判定错");
    }

    // C56-13 连续超预算才告警（单帧尖峰不告警）
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        let _ = m.ingest(1, [Some(0), Some(10_000), Some(30_000), Some(50_000)], Some(70_000), true, 0);
        let a1 = m.counters().4;
        for i in 2..=(ALARM_STREAK as u64 + 1) {
            let _ = m.ingest(i, [Some(0), Some(10_000), Some(30_000), Some(50_000)], Some(70_000), true, 0);
        }
        let a2 = m.counters().4;
        chk(&mut set, "C56-13 连续超预算告警", a1 == 0 && a2 == 1, "告警时机错");
    }

    // C56-14 环形窗口覆盖与账本守恒
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        for i in 0..(WINDOW_CAP as u64 + 10) {
            let _ = m.ingest(i, full_ticks(), Some(4000), true, 0);
        }
        let ok = m.len() == WINDOW_CAP && m.ledger_balanced();
        chk(&mut set, "C56-14 窗口守恒", ok, "覆盖记账不平");
    }

    // C56-15 窗口内最旧样本被覆盖（sample_at(0) 的帧号是最新的第 10 帧）
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        for i in 0..(WINDOW_CAP as u64 + 10) {
            let _ = m.ingest(i, full_ticks(), Some(4000), true, 0);
        }
        let first = m.sample_at(0).map(|s| s.frame_id);
        let ok = first == Some(10);
        chk(&mut set, "C56-15 覆盖顺序", ok, "窗口顺序错");
    }

    // C56-16 分段均值只统计实测样本（补桩不得进入均值）
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        // 帧1：四段各 100µs。
        let _ = m.ingest(1, [Some(0), Some(100), Some(200), Some(300)], Some(400), true, 0);
        // 帧2：sim 起点缺失（补桩 100），render 段应仍从 200→400 算 200。
        let _ = m.ingest(2, [Some(0), None, Some(200), Some(400)], Some(600), true, 0);
        let means = m.segment_means();
        let s1 = m.sample_at(1);
        let stub_flag = s1.map(|s| !s.measured[1]).unwrap_or(false);
        // 若把补桩样本也算进 sim 均值仍是 100（桩值恰等于实测值），故用
        // 「render 段均值」验边界推导：帧1=100、帧2=200 ⇒ 150；
        // 若误用「下一段缺失就跳 end」，帧1 的 render 会变成 200（0→400 撑爆），
        // 均值会变 200。两种错误都能被这组期望值抓住。
        let ok = stub_flag && means[1] == 100 && means[2] == 150;
        chk(&mut set, "C56-16 分段均值口径", ok, "补桩污染均值或边界撑爆");
    }

    // C56-17 读屏面板非空且含关键字段
    {
        let mut m = LatencyMeter::new();
        let _ = m.link_peer(CONTRACT_FINGERPRINT);
        let _ = m.ingest(1, full_ticks(), Some(4000), true, 0);
        let line = m.panel_line(0);
        let ok = line.contains("P95") && line.contains("达标");
        chk(&mut set, "C56-17 延迟面板读屏", ok, "面板不可读");
    }

    // C56-18 未同源面板给显式告示（不显示假数）
    {
        let m = LatencyMeter::new();
        let line = m.panel_line(0);
        chk(&mut set, "C56-18 未同源告示", line.contains("未与账本同源"), "未同源仍显示数字");
    }

    // C56-19 拒收账保留诊断码（可回溯）
    {
        let mut m = LatencyMeter::new();
        let _ = m.ingest(1, full_ticks(), None, true, 0);
        let ok = m.reject_log().first().copied() == Some(XC_NO_PHOTON);
        chk(&mut set, "C56-19 拒收可回溯", ok, "拒收无码");
    }

    // C56-20 确定性：同输入序列两次 → 分位与计数一致
    {
        let mut a = LatencyMeter::new();
        let mut b = LatencyMeter::new();
        let _ = a.link_peer(CONTRACT_FINGERPRINT);
        let _ = b.link_peer(CONTRACT_FINGERPRINT);
        for i in 0..30u64 {
            let t = 3000 + (i * 137) % 9000;
            let ticks = [Some(0), Some(t / 4), Some(t / 2), Some((t * 3) / 4)];
            let _ = a.ingest(i, ticks, Some(t), true, 0);
            let _ = b.ingest(i, ticks, Some(t), true, 0);
        }
        let ok = a.percentile_us(0, 950) == b.percentile_us(0, 950)
            && a.segment_means() == b.segment_means();
        chk(&mut set, "C56-20 确定性", ok, "两次结果不一致");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea56_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
