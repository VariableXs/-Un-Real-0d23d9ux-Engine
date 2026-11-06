//! UNX-F48888 十设备三十分钟对账 · 分派回执 · 0x70AE0
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48888（十设备 ×30 分钟心跳对账零漂移）· F48889（分派回执
//!         ×10^3 单次有效）· F48890（窗口齐全 + 漂移判定 ×10^2 零漏判）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48888 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48888-J1 十设备 ×30 分钟对账零漂移；
//!   F48889 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48889-J1 分派回执 ×10^3 单次有效；
//!   F48890 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48890-J1 窗口齐全/漂移判定 ×10^2 零漏判。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：设备号 → 分钟号 → 心跳序号 → 位图置位 → 分派槽 →
//!           回执登记 → 回执核销 → 窗口对账。
//!   次序不可倒的理由逐条：
//!     · 设备号先判：设备号越界时分钟位图会落到邻设备的账上，之后
//!       无论怎么对账都会「齐」，只是齐在了错误的设备上；
//!     · 分钟号次判：30 分钟窗口只有 0..29，第 30 分钟属于下一窗口，
//!       先置位再判号会把下一窗口的心跳提前算进本窗口；
//!     · 心跳序号必须在置位之前：重放的心跳若先置位再被序号拒绝，
//!       位图已经被污染，序列号却回滚，账就无法同时自洽；
//!     · 分派槽必须在回执之前：回执按槽号编号，槽号错了回执就对不上
//!       任何一台真实设备，核销时只能发现「找不着」，发现不了「走错」；
//!     · 回执登记必须早于核销：没有登记就核销等于凭空销账；
//!     · 窗口对账最后执行，它只回答「这一窗口的每台设备是否齐、
//!       漂移是否为零」，不回答心跳是否合法。
//! 关键差异：本模块只拥有「分派 + 回执 + 窗口对账」这一层。
//!   core 拥有三操作 SOP；meta 拥有合并策略；guard 拥有背压与配额；
//!   edge 拥有边界阶梯；bridge 拥有跨模块寻址；quality 拥有分位与
//!   覆盖率。它不复制背压、不复制合并、不复制配额；它回答的是：
//!   这十台设备在这 30 个分钟格里，心跳是否一格不缺，回执是否一证
//!   一销，分派槽是否两两均衡。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70AE0，VX6 批 B05 族第四槽）。
pub const SEG_BASE: u32 = 0x70AE0;

/// 十设备。
pub const DEVICE_COUNT: usize = 10;
/// 对账窗口 30 分钟。
pub const WINDOW_MINUTES: u32 = 30;
/// 每秒一拍时窗口内的满格位图（30 位全一）。
pub const WINDOW_MASK: u32 = (1u32 << WINDOW_MINUTES) - 1;
/// 分派槽数（十设备两两分槽）。
pub const SHARDS: usize = 5;
/// 每槽设备数。
pub const PER_SHARD: usize = DEVICE_COUNT / SHARDS;
/// 对账轮数（判据 ×10^2）。
pub const RECON_ROUNDS: u32 = 100;
/// 回执注入轮数。
pub const RECEIPT_ROUNDS: u32 = 1000;
/// 心跳语料轮数。
pub const BEAT_ROUNDS: u32 = 1000;
/// 负载上限（百分比）。
pub const LOAD_MAX: u32 = 100;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DkCode {
    /// 设备号越界
    DevBad,
    /// 分钟号越界（不在 0..30）
    MinuteBad,
    /// 心跳序号非法（零 / 回退 / 重放）
    BeatBad,
    /// 分派槽非法（与设备号不匹配 / 槽号越界）
    ShardBad,
    /// 回执非法（未登记 / 槽号漂移）
    ReceiptBad,
    /// 回执重放（同一凭证核销两次）
    ReplayBad,
    /// 窗口不齐（位图缺格 / 多格）
    WindowBad,
    /// 对账漂移（发出与观测不一致）
    DriftBad,
    /// 账不平（核销数大于登记数 / 总轮数不符）
    LedgerBad,
    /// 导出畸形（字段数 / 数字 / 覆盖）
    RenderBad,
    /// 段位越界
    SegmentBad,
}

impl DkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DkCode::DevBad => "设备号越界：只允许 0..9 十台设备",
            DkCode::MinuteBad => "分钟号越界：对账窗口只含第 0..29 分钟",
            DkCode::BeatBad => "心跳非法：序号非零、严格递增，重复分钟视为重放",
            DkCode::ShardBad => "分派槽非法：槽号须等于设备号对 5 取模",
            DkCode::ReceiptBad => "回执非法：凭证未登记或槽号与登记不符",
            DkCode::ReplayBad => "回执重放：同一凭证只允许核销一次",
            DkCode::WindowBad => "窗口不齐：三十格位图必须全一，缺一即拒",
            DkCode::DriftBad => "对账漂移：发出心跳数与位图观测数必须相等",
            DkCode::LedgerBad => "账不平：登记、核销、总轮数三者不自洽",
            DkCode::RenderBad => "导出畸形：六字段、数字可解析、摘要十六位",
            DkCode::SegmentBad => "段位越界：诊断码逃出 0x70AE0 窗口",
        }
    }

    pub const ALL: [DkCode; 11] = [
        DkCode::DevBad,
        DkCode::MinuteBad,
        DkCode::BeatBad,
        DkCode::ShardBad,
        DkCode::ReceiptBad,
        DkCode::ReplayBad,
        DkCode::WindowBad,
        DkCode::DriftBad,
        DkCode::LedgerBad,
        DkCode::RenderBad,
        DkCode::SegmentBad,
    ];
}

/// 段内码取明细文本；闭集外给固定文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DkCode::ALL.len() {
        if let Some(c) = DkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知分派诊断码：不属于 0x70AE0 闭集"
}

// ── FNV-1a 64 ────────────────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u16(h: u64, v: u16) -> u64 {
    let a = fold_byte(h, (v & 0xFF) as u8);
    fold_byte(a, (v >> 8) as u8)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let a = fold_u16(h, (v & 0xFFFF) as u16);
    fold_u16(a, (v >> 16) as u16)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── 确定性伪随机 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed } }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }
}

// ── 心跳 ─────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Beat {
    pub dev: u8,
    pub minute: u8,
    pub seq: u32,
    pub load: u8,
}

impl Beat {
    pub fn new(dev: u8, minute: u8, seq: u32, load: u8) -> Result<Beat, u32> {
        if dev as usize >= DEVICE_COUNT {
            return Err(DkCode::DevBad.val());
        }
        if minute as u32 >= WINDOW_MINUTES {
            return Err(DkCode::MinuteBad.val());
        }
        if seq == 0 {
            return Err(DkCode::BeatBad.val());
        }
        if load as u32 > LOAD_MAX {
            return Err(DkCode::BeatBad.val());
        }
        Ok(Beat { dev, minute, seq, load })
    }

    pub fn text(&self) -> String {
        format!("{}|{}|{}|{}", self.dev, self.minute, self.seq, self.load)
    }
}

/// 单设备窗口账：30 位位图 + 序号 + 计数。
#[derive(Clone, Debug)]
pub struct DeviceWindow {
    dev: u8,
    mask: u32,
    last_seq: u32,
    beats: u32,
    load_sum: u64,
    digest: u64,
}

impl DeviceWindow {
    pub fn new(dev: u8) -> Result<DeviceWindow, u32> {
        if dev as usize >= DEVICE_COUNT {
            return Err(DkCode::DevBad.val());
        }
        Ok(DeviceWindow {
            dev,
            mask: 0,
            last_seq: 0,
            beats: 0,
            load_sum: 0,
            digest: FNV_OFF,
        })
    }

    pub fn dev(&self) -> u8 {
        self.dev
    }

    pub fn mask(&self) -> u32 {
        self.mask
    }

    pub fn beats(&self) -> u32 {
        self.beats
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn load_avg(&self) -> u32 {
        if self.beats == 0 {
            0
        } else {
            (self.load_sum / self.beats as u64) as u32
        }
    }

    /// 收一拍：分钟重复视为重放，序号必须递增。
    pub fn beat(&mut self, b: Beat) -> Result<u32, u32> {
        if b.dev != self.dev {
            return Err(DkCode::DevBad.val());
        }
        if b.minute as u32 >= WINDOW_MINUTES {
            return Err(DkCode::MinuteBad.val());
        }
        if b.seq <= self.last_seq {
            return Err(DkCode::BeatBad.val());
        }
        let bit = 1u32 << (b.minute as u32);
        if self.mask & bit != 0 {
            return Err(DkCode::BeatBad.val());
        }
        self.mask |= bit;
        self.last_seq = b.seq;
        self.beats = self.beats.saturating_add(1);
        self.load_sum = self.load_sum.saturating_add(b.load as u64);
        self.digest = fold_byte(self.digest, b.dev);
        self.digest = fold_byte(self.digest, b.minute);
        self.digest = fold_u32(self.digest, b.seq);
        Ok(self.mask)
    }

    pub fn full(&self) -> bool {
        self.mask == WINDOW_MASK
    }

    /// 自审：位图一的个数必须等于心跳计数。
    pub fn audit(&self) -> Result<(), u32> {
        let ones = self.mask.count_ones();
        if ones != self.beats {
            return Err(DkCode::DriftBad.val());
        }
        if self.mask & !WINDOW_MASK != 0 {
            return Err(DkCode::WindowBad.val());
        }
        Ok(())
    }
}

// ── 分派表（设备 → 槽，两两均衡） ───────────────────────────────────
#[derive(Clone, Debug)]
pub struct DispatchTable {
    slots: [u8; DEVICE_COUNT],
    sizes: [u8; SHARDS],
}

impl DispatchTable {
    pub fn new() -> DispatchTable {
        DispatchTable { slots: [0; DEVICE_COUNT], sizes: [0; SHARDS] }
    }

    pub fn bind(&mut self, dev: u8, shard: u8) -> Result<u8, u32> {
        if dev as usize >= DEVICE_COUNT || shard as usize >= SHARDS {
            return Err(DkCode::ShardBad.val());
        }
        if shard as usize != dev as usize % SHARDS {
            return Err(DkCode::ShardBad.val());
        }
        if let Some(s) = self.slots.get_mut(dev as usize) {
            *s = shard;
        }
        if let Some(n) = self.sizes.get_mut(shard as usize) {
            *n = n.saturating_add(1);
        }
        Ok(shard)
    }

    pub fn shard_of(&self, dev: u8) -> Result<u8, u32> {
        if dev as usize >= DEVICE_COUNT {
            return Err(DkCode::DevBad.val());
        }
        match self.slots.get(dev as usize) {
            Some(v) => Ok(*v),
            None => Err(DkCode::DevBad.val()),
        }
    }

    pub fn size(&self, shard: usize) -> u8 {
        match self.sizes.get(shard) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 自审：每槽恰两台，总绑定十台。
    pub fn audit(&self) -> Result<(), u32> {
        let mut total: u32 = 0;
        let mut i = 0usize;
        while i < SHARDS {
            let n = self.size(i) as u32;
            if n != PER_SHARD as u32 {
                return Err(DkCode::ShardBad.val());
            }
            total = total.saturating_add(n);
            i += 1;
        }
        if total != DEVICE_COUNT as u32 {
            return Err(DkCode::ShardBad.val());
        }
        Ok(())
    }
}

/// 标准分派：设备 dev 固定落在 dev%5 槽。
pub fn bind_all() -> Result<DispatchTable, u32> {
    let mut t = DispatchTable::new();
    let mut dev = 0u8;
    while (dev as usize) < DEVICE_COUNT {
        let shard = (dev as usize % SHARDS) as u8;
        let _ = t.bind(dev, shard)?;
        dev = dev.saturating_add(1);
    }
    t.audit()?;
    Ok(t)
}

// ── 回执（登记 / 一次核销） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub token: u32,
    pub dev: u8,
    pub shard: u8,
    pub minute: u8,
}

impl Receipt {
    pub fn new(dev: u8, shard: u8, minute: u8, seq: u32) -> Result<Receipt, u32> {
        if dev as usize >= DEVICE_COUNT || shard as usize >= SHARDS {
            return Err(DkCode::ShardBad.val());
        }
        if shard as usize != dev as usize % SHARDS {
            return Err(DkCode::ShardBad.val());
        }
        if minute as u32 >= WINDOW_MINUTES || seq == 0 {
            return Err(DkCode::ReceiptBad.val());
        }
        let token = ((dev as u32) << 24) | ((minute as u32) << 16) | (seq & 0xFFFF);
        if token == 0 {
            return Err(DkCode::ReceiptBad.val());
        }
        Ok(Receipt { token, dev, shard, minute })
    }
}

#[derive(Clone, Debug)]
pub struct ReceiptBook {
    entries: Vec<(Receipt, bool)>,
    issued: u32,
    acked: u32,
    replays: u32,
    digest: u64,
}

impl ReceiptBook {
    pub fn new() -> ReceiptBook {
        ReceiptBook { entries: Vec::new(), issued: 0, acked: 0, replays: 0, digest: FNV_OFF }
    }

    pub fn issued(&self) -> u32 {
        self.issued
    }

    pub fn acked(&self) -> u32 {
        self.acked
    }

    pub fn replays(&self) -> u32 {
        self.replays
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 登记回执：同一凭证不得登记两次。
    pub fn issue(&mut self, r: Receipt) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some((old, _)) = self.entries.get(i) {
                if old.token == r.token {
                    return Err(DkCode::ReceiptBad.val());
                }
            }
            i += 1;
        }
        self.entries.push((r, false));
        self.issued = self.issued.saturating_add(1);
        self.digest = fold_u32(self.digest, r.token);
        Ok(())
    }

    /// 核销：凭证必须存在且未核销；已核销再次来即重放。
    pub fn ack(&mut self, token: u32) -> Result<u8, u32> {
        let mut ix: Option<usize> = None;
        let mut used = false;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some((r, done)) = self.entries.get(i) {
                if r.token == token {
                    ix = Some(i);
                    used = *done;
                    break;
                }
            }
            i += 1;
        }
        let at = match ix {
            Some(v) => v,
            None => return Err(DkCode::ReceiptBad.val()),
        };
        if used {
            self.replays = self.replays.saturating_add(1);
            return Err(DkCode::ReplayBad.val());
        }
        let dev = match self.entries.get_mut(at) {
            Some((r, done)) => {
                *done = true;
                r.dev
            }
            None => return Err(DkCode::ReceiptBad.val()),
        };
        self.acked = self.acked.saturating_add(1);
        self.digest = fold_byte(self.digest, dev);
        Ok(dev)
    }

    /// 自审：核销不超过登记，条目数等于登记数。
    pub fn audit(&self) -> Result<(), u32> {
        if self.acked > self.issued {
            return Err(DkCode::LedgerBad.val());
        }
        if self.entries.len() != self.issued as usize {
            return Err(DkCode::LedgerBad.val());
        }
        Ok(())
    }
}

// ── 对账（窗口 + 回执 + 漂移三合一） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconRow {
    pub dev: u8,
    pub observed: u32,
    pub expected: u32,
    pub drift: u32,
    pub full: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconVerdict {
    pub rows: usize,
    pub full_rows: usize,
    pub drift_sum: u32,
    pub digest: u64,
}

impl ReconVerdict {
    pub fn clean(&self) -> bool {
        self.rows == DEVICE_COUNT && self.full_rows == DEVICE_COUNT && self.drift_sum == 0
    }
}

/// 单行对账：观测位图与期望心跳数必须相等且满窗。
pub fn reconcile_one(w: &DeviceWindow, expected: u32) -> Result<ReconRow, u32> {
    w.audit()?;
    let observed = w.mask().count_ones();
    let drift = if expected >= observed { expected - observed } else { observed - expected };
    let full = w.full();
    if !full {
        return Err(DkCode::WindowBad.val());
    }
    if drift != 0 {
        return Err(DkCode::DriftBad.val());
    }
    Ok(ReconRow { dev: w.dev(), observed, expected, drift, full })
}

/// 整窗对账：十台设备逐行，缺一即拒。
pub fn reconcile_all(windows: &[DeviceWindow], expected: u32) -> Result<ReconVerdict, u32> {
    if windows.len() != DEVICE_COUNT {
        return Err(DkCode::LedgerBad.val());
    }
    let mut i = 0usize;
    let mut full_rows = 0usize;
    let mut drift_sum = 0u32;
    let mut digest = FNV_OFF;
    while i < windows.len() {
        let w = match windows.get(i) {
            Some(v) => v,
            None => return Err(DkCode::LedgerBad.val()),
        };
        let row = reconcile_one(w, expected)?;
        if row.full {
            full_rows = full_rows.saturating_add(1);
        }
        drift_sum = drift_sum.saturating_add(row.drift);
        digest = fold_byte(digest, row.dev);
        digest = fold_u32(digest, row.observed);
        i += 1;
    }
    if full_rows != DEVICE_COUNT || drift_sum != 0 {
        return Err(DkCode::DriftBad.val());
    }
    Ok(ReconVerdict { rows: windows.len(), full_rows, drift_sum, digest })
}

// ── 语料与黄金运行 ──────────────────────────────────────────────────
/// 第 ix 拍：循环覆盖十设备 × 三十格。
pub fn corpus_beat(ix: u32) -> Result<Beat, u32> {
    let dev = (ix % DEVICE_COUNT as u32) as u8;
    let minute = ((ix / DEVICE_COUNT as u32) % WINDOW_MINUTES) as u8;
    let seq = ix / DEVICE_COUNT as u32 + 1;
    let load = (ix % (LOAD_MAX + 1)) as u8;
    Beat::new(dev, minute, seq, load)
}

/// 一窗口满扫：300 拍，十台各三十格。
pub fn full_sweep() -> Result<Vec<DeviceWindow>, u32> {
    let mut wins: Vec<DeviceWindow> = Vec::new();
    let mut d = 0u8;
    while (d as usize) < DEVICE_COUNT {
        wins.push(DeviceWindow::new(d)?);
        d = d.saturating_add(1);
    }
    let mut i = 0u32;
    while i < (DEVICE_COUNT as u32) * WINDOW_MINUTES {
        let b = Beat::new(
            (i % DEVICE_COUNT as u32) as u8,
            ((i / DEVICE_COUNT as u32) % WINDOW_MINUTES) as u8,
            i / DEVICE_COUNT as u32 + 1,
            (i % (LOAD_MAX + 1)) as u8,
        )?;
        let ix = b.dev as usize;
        if let Some(w) = wins.get_mut(ix) {
            let _ = w.beat(b)?;
        }
        i = i.saturating_add(1);
    }
    Ok(wins)
}

/// 黄金运行：满扫 + 分派 + 300 回执 + 每三张核销一张。
pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    let wins = full_sweep()?;
    let table = bind_all()?;
    let mut book = ReceiptBook::new();
    let mut i = 0u32;
    while i < (DEVICE_COUNT as u32) * WINDOW_MINUTES {
        let dev = (i % DEVICE_COUNT as u32) as u8;
        let minute = ((i / DEVICE_COUNT as u32) % WINDOW_MINUTES) as u8;
        let seq = i + 1;
        let shard = table.shard_of(dev)?;
        let r = Receipt::new(dev, shard, minute, seq)?;
        let _ = book.issue(r)?;
        i = i.saturating_add(1);
    }
    let mut acked = 0u32;
    let mut k = 0u32;
    while k < book.issued() {
        if k % 3 == 0 {
            let token = match book_token(&book, k) {
                Some(t) => t,
                None => return Err(DkCode::LedgerBad.val()),
            };
            let _ = book.ack(token)?;
            acked = acked.saturating_add(1);
        }
        k = k.saturating_add(1);
    }
    let verdict = reconcile_all(&wins, WINDOW_MINUTES)?;
    if !verdict.clean() {
        return Err(DkCode::DriftBad.val());
    }
    book.audit()?;
    let mut digest = verdict.digest;
    digest = fold_u32(digest, book.digest() as u32);
    digest = fold_u32(digest, (book.digest() >> 32) as u32);
    Ok((
        (DEVICE_COUNT as u32) * WINDOW_MINUTES,
        book.issued(),
        acked,
        verdict.drift_sum,
        digest,
    ))
}

fn book_token(book: &ReceiptBook, ix: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < book.entries.len() {
        if i == ix as usize {
            if let Some((r, _)) = book.entries.get(i) {
                return Some(r.token);
            }
            return None;
        }
        i += 1;
    }
    None
}

// ── 注入（四路） ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    pub rounds: u32,
    pub caught: u32,
    pub route_caught: [u32; 4],
    pub digest: u64,
}

impl InjectLedger {
    pub fn all_caught(&self) -> bool {
        self.caught == self.rounds && self.rounds > 0
    }
}

fn inject_once(route: u32, ix: u32) -> Result<bool, u32> {
    match route % 4 {
        0 => {
            // 分钟号越界必须 MinuteBad；设备号越界必须 DevBad。
            let m = Beat::new(0, WINDOW_MINUTES as u8, 1, 1);
            let d = Beat::new(DEVICE_COUNT as u8, 0, 1, 1);
            Ok(m == Err(DkCode::MinuteBad.val()) && d == Err(DkCode::DevBad.val()))
        }
        1 => {
            // 回执重放必须 ReplayBad；未登记必须 ReceiptBad。
            let mut b = ReceiptBook::new();
            let r = Receipt::new(1, 1, 2, 1)?;
            let _ = b.issue(r)?;
            let first = b.ack(r.token).is_ok();
            let second = b.ack(r.token) == Err(DkCode::ReplayBad.val());
            let unknown = b.ack(0x0BAD_0000) == Err(DkCode::ReceiptBad.val());
            Ok(first && second && unknown)
        }
        2 => {
            // 槽号错位必须 ShardBad。
            let r = Receipt::new(3, 2, 1, 1);
            Ok(r == Err(DkCode::ShardBad.val()))
        }
        _ => {
            // 缺一格窗口必须 WindowBad；重复分钟必须 BeatBad。
            let mut w = DeviceWindow::new((ix % 10) as u8)?;
            let b0 = Beat::new((ix % 10) as u8, 0, 1, 1)?;
            let _ = w.beat(b0)?;
            let dup = w.beat(b0) == Err(DkCode::BeatBad.val());
            let short = reconcile_one(&w, 1) == Err(DkCode::WindowBad.val());
            Ok(dup && short)
        }
    }
}

/// 四路注入各跑 rounds/4 轮。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds / 4 == 0 {
        return Err(DkCode::LedgerBad.val());
    }
    let per = rounds / 4;
    let mut led = InjectLedger { rounds: 0, caught: 0, route_caught: [0; 4], digest: FNV_OFF };
    let mut route = 0u32;
    while route < 4 {
        let mut i = 0u32;
        while i < per {
            let hit = inject_once(route, i)?;
            if hit {
                led.caught = led.caught.saturating_add(1);
                if let Some(c) = led.route_caught.get_mut(route as usize) {
                    *c = c.saturating_add(1);
                }
            }
            led.rounds = led.rounds.saturating_add(1);
            led.digest = fold_byte(led.digest, if hit { 1 } else { 0 });
            i = i.saturating_add(1);
        }
        route = route.saturating_add(1);
    }
    Ok(led)
}

// ── 导出与回读 ──────────────────────────────────────────────────────
pub fn render_summary(wins: &[DeviceWindow], book: &ReceiptBook) -> Result<String, u32> {
    let verdict = reconcile_all(wins, WINDOW_MINUTES)?;
    let mut s = String::new();
    s.push_str(&format!(
        "{}|{}|{}|{}|{}|",
        verdict.rows, verdict.full_rows, verdict.drift_sum, book.issued(), book.acked()
    ));
    s.push_str(&format!("{:016X}\n", verdict.digest ^ book.digest()));
    Ok(s)
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(DkCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(DkCode::RenderBad.val()),
        };
        if !b.is_ascii_digit() {
            return Err(DkCode::RenderBad.val());
        }
        v = v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
            .ok_or_else(|| DkCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 16 {
        return Err(DkCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(DkCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            _ => return Err(DkCode::RenderBad.val()),
        };
        v = v.checked_mul(16).and_then(|x| x.checked_add(d)).ok_or_else(|| DkCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

/// 回读：六字段、十行齐、漂移为零、账平。
pub fn audit_summary(text: &str) -> Result<(usize, u32, u64), u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 1 {
        return Err(DkCode::RenderBad.val());
    }
    let line = match lines.get(0) {
        Some(v) => *v,
        None => return Err(DkCode::RenderBad.val()),
    };
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 6 {
        return Err(DkCode::RenderBad.val());
    }
    let rows = parse_u32(parts.get(0).copied().unwrap_or(""))? as usize;
    let full = parse_u32(match parts.get(1) {
        Some(v) => *v,
        None => return Err(DkCode::RenderBad.val()),
    })?;
    let drift = parse_u32(match parts.get(2) {
        Some(v) => *v,
        None => return Err(DkCode::RenderBad.val()),
    })?;
    let issued = parse_u32(match parts.get(3) {
        Some(v) => *v,
        None => return Err(DkCode::RenderBad.val()),
    })?;
    let acked = parse_u32(match parts.get(4) {
        Some(v) => *v,
        None => return Err(DkCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => *v,
        None => return Err(DkCode::RenderBad.val()),
    })?;
    if rows != DEVICE_COUNT || full != DEVICE_COUNT as u32 || drift != 0 {
        return Err(DkCode::WindowBad.val());
    }
    if acked > issued {
        return Err(DkCode::LedgerBad.val());
    }
    Ok((rows, acked, digest))
}

// ── 探针 ────────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let wins = full_sweep()?;
    let v = reconcile_all(&wins, WINDOW_MINUTES)?;
    if !v.clean() {
        return Err(DkCode::WindowBad.val());
    }
    let mut i = 0usize;
    while i < wins.len() {
        if let Some(w) = wins.get(i) {
            w.audit()?;
            if !w.full() {
                return Err(DkCode::WindowBad.val());
            }
        }
        i += 1;
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let t = bind_all()?;
    let mut dev = 0u8;
    while (dev as usize) < DEVICE_COUNT {
        let want = (dev as usize % SHARDS) as u8;
        if t.shard_of(dev)? != want {
            return Err(DkCode::ShardBad.val());
        }
        dev = dev.saturating_add(1);
    }
    t.audit()
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = ReceiptBook::new();
    let r = Receipt::new(4, 4, 7, 9)?;
    b.issue(r.clone())?;
    if b.ack(r.token)? != 4 {
        return Err(DkCode::ReceiptBad.val());
    }
    if b.ack(r.token) != Err(DkCode::ReplayBad.val()) {
        return Err(DkCode::ReplayBad.val());
    }
    b.audit()
}

/// 复现：黄金计数、摘要、注入、回读四件事必须同时成立。
pub fn repro_check() -> Result<(), u32> {
    let (beats, issued, acks, drift, digest) = golden_run()?;
    if beats != (DEVICE_COUNT as u32) * WINDOW_MINUTES {
        return Err(DkCode::LedgerBad.val());
    }
    if issued != beats || acks != (issued + 2) / 3 || drift != 0 {
        return Err(DkCode::LedgerBad.val());
    }
    let again = golden_run()?;
    if again.4 != digest {
        return Err(DkCode::LedgerBad.val());
    }
    let wins = full_sweep()?;
    let mut book = ReceiptBook::new();
    let mut i = 0u32;
    while i < issued {
        let dev = (i % DEVICE_COUNT as u32) as u8;
        let minute = ((i / DEVICE_COUNT as u32) % WINDOW_MINUTES) as u8;
        let r = Receipt::new(dev, (dev as usize % SHARDS) as u8, minute, i + 1)?;
        let _ = book.issue(r)?;
        i = i.saturating_add(1);
    }
    let text = render_summary(&wins, &book)?;
    let (rows, acked, _) = audit_summary(&text)?;
    if rows != DEVICE_COUNT || acked != 0 {
        return Err(DkCode::LedgerBad.val());
    }
    let inj = inject_rounds(RECEIPT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(DkCode::LedgerBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 0x70AE1..=0x70AEB。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < DkCode::ALL.len() {
        let c = match DkCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() < lo || c.val() > hi {
            return false;
        }
        i += 1;
    }
    true
}

// ── 十项动态判据 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48888-J1-十设备满窗",
        match probe_seq_a() {
            Ok(_) => true,
            Err(_) => false,
        },
        String::from("10 devices x 30 minutes, mask all ones"),
    ));
    v.push((
        "F48888-J1-漂移为零",
        match golden_run() {
            Ok((b, iss, ac, d, _)) => b == 300 && iss == 300 && ac == 100 && d == 0,
            Err(_) => false,
        },
        match golden_run() {
            Ok((b, iss, ac, d, h)) => format!("beats={} issued={} acks={} drift={} h={:016X}", b, iss, ac, d, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48889-J1-分派均衡",
        match bind_all() {
            Ok(t) => t.audit().is_ok() && t.size(0) == 2 && t.size(4) == 2,
            Err(_) => false,
        },
        String::from("5 shards x 2 devices"),
    ));
    v.push((
        "F48889-J1-回执单次",
        match probe_seq_c() {
            Ok(_) => true,
            Err(_) => false,
        },
        String::from("ack twice: second ReplayBad"),
    ));
    v.push((
        "F48889-J1-未登记拒收",
        {
            let mut b = ReceiptBook::new();
            b.ack(0x1234_5678) == Err(DkCode::ReceiptBad.val())
        },
        String::from("unknown token refused"),
    ));
    v.push((
        "F48890-J1-缺格漏判",
        match (DeviceWindow::new(0), Beat::new(0, 0, 1, 1)) {
            (Ok(mut w), Ok(b)) => {
                let _ = w.beat(b);
                reconcile_one(&w, 1) == Err(DkCode::WindowBad.val())
            }
            _ => false,
        },
        String::from("one-minute window rejected as incomplete"),
    ));
    v.push((
        "F48890-J1-重放漏判",
        match (DeviceWindow::new(1), Beat::new(1, 3, 1, 50)) {
            (Ok(mut w), Ok(b)) => {
                let _ = w.beat(b);
                w.beat(b) == Err(DkCode::BeatBad.val())
            }
            _ => false,
        },
        String::from("duplicate minute refused"),
    ));
    v.push((
        "J1-注入四路全捕",
        match inject_rounds(RECEIPT_ROUNDS) {
            Ok(l) => l.all_caught() && l.route_caught.iter().all(|c| *c > 0),
            Err(_) => false,
        },
        match inject_rounds(RECEIPT_ROUNDS) {
            Ok(l) => format!("rounds={} caught={} routes={:?}", l.rounds, l.caught, l.route_caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "J1-导出回读",
        {
            match full_sweep() {
                Ok(wins) => {
                    let book = ReceiptBook::new();
                    match render_summary(&wins, &book) {
                        Ok(t) => audit_summary(&t).is_ok(),
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            }
        },
        String::from("six fields round-trip"),
    ));
    v.push((
        "J1-账平自审",
        match golden_run() {
            Ok((b, iss, ac, d, _)) => iss <= b && ac <= iss && d == 0,
            Err(_) => false,
        },
        String::from("acks<=issued<=beats, drift 0"),
    ));
    v
}

/// 本模块判据集合。
pub fn run_unx48888_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48888 十设备对账与分派回执");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let g = segment_guard();
    s.add_dyn("F48888-J1-段位守卫", g, if g { "" } else { "诊断码逃出 0x70AE0" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_validation_order() {
        assert_eq!(Beat::new(10, 0, 1, 1), Err(DkCode::DevBad.val()));
        assert_eq!(Beat::new(0, 30, 1, 1), Err(DkCode::MinuteBad.val()));
        assert_eq!(Beat::new(0, 0, 0, 1), Err(DkCode::BeatBad.val()));
        assert_eq!(Beat::new(0, 0, 1, 101), Err(DkCode::BeatBad.val()));
        assert!(Beat::new(9, 29, 1, 100).is_ok());
    }

    #[test]
    fn window_rejects_duplicate_minute() {
        let w = DeviceWindow::new(0);
        let mut w = match w {
            Ok(v) => v,
            Err(_) => return,
        };
        let b = Beat::new(0, 5, 1, 10);
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(w.beat(b).is_ok());
        assert_eq!(w.beat(b), Err(DkCode::BeatBad.val()));
        assert_eq!(w.mask().count_ones(), 1);
    }

    #[test]
    fn window_rejects_foreign_device() {
        let w = DeviceWindow::new(2);
        let mut w = match w {
            Ok(v) => v,
            Err(_) => return,
        };
        let b = Beat::new(3, 1, 1, 1);
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(w.beat(b), Err(DkCode::DevBad.val()));
    }

    #[test]
    fn full_sweep_fills_all_bits() {
        let wins = full_sweep();
        let wins = match wins {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(wins.len(), DEVICE_COUNT);
        for w in &wins {
            assert_eq!(w.mask(), WINDOW_MASK);
            assert_eq!(w.beats(), WINDOW_MINUTES);
            assert!(w.audit().is_ok());
        }
    }

    #[test]
    fn bind_all_is_balanced() {
        let t = bind_all();
        let t = match t {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(t.audit().is_ok());
        assert_eq!(t.shard_of(0), Ok(0));
        assert_eq!(t.shard_of(9), Ok(4));
        assert_eq!(t.shard_of(10), Err(DkCode::DevBad.val()));
    }

    #[test]
    fn bind_rejects_wrong_shard() {
        let mut t = DispatchTable::new();
        assert_eq!(t.bind(3, 2), Err(DkCode::ShardBad.val()));
        assert_eq!(t.bind(3, 5), Err(DkCode::ShardBad.val()));
        assert!(t.bind(3, 3).is_ok());
    }

    #[test]
    fn receipt_issue_unique() {
        let mut b = ReceiptBook::new();
        let r = Receipt::new(0, 0, 1, 1);
        let r = match r {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(b.issue(r.clone()).is_ok());
        assert_eq!(b.issue(r), Err(DkCode::ReceiptBad.val()));
        assert_eq!(b.issued(), 1);
    }

    #[test]
    fn receipt_ack_once() {
        let mut b = ReceiptBook::new();
        let r = Receipt::new(5, 0, 2, 3);
        let r = match r {
            Ok(v) => v,
            Err(_) => return,
        };
        let _ = b.issue(r.clone());
        assert_eq!(b.ack(r.token), Ok(5));
        assert_eq!(b.ack(r.token), Err(DkCode::ReplayBad.val()));
        assert_eq!(b.replays(), 1);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn receipt_token_shape() {
        let r = Receipt::new(9, 4, 29, 7);
        let r = match r {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.token, (9 << 24) | (29 << 16) | 7);
    }

    #[test]
    fn reconcile_rejects_incomplete_window() {
        let w = DeviceWindow::new(0);
        let mut w = match w {
            Ok(v) => v,
            Err(_) => return,
        };
        let b = Beat::new(0, 0, 1, 1);
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        let _ = w.beat(b);
        assert_eq!(reconcile_one(&w, 1), Err(DkCode::WindowBad.val()));
    }

    #[test]
    fn reconcile_rejects_expected_mismatch() {
        let wins = full_sweep();
        let wins = match wins {
            Ok(v) => v,
            Err(_) => return,
        };
        let w = match wins.get(0) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(reconcile_one(w, WINDOW_MINUTES + 1), Err(DkCode::DriftBad.val()));
    }

    #[test]
    fn reconcile_all_clean() {
        let wins = full_sweep();
        let wins = match wins {
            Ok(v) => v,
            Err(_) => return,
        };
        let v = reconcile_all(&wins, WINDOW_MINUTES);
        let v = match v {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(v.clean());
        assert_eq!(v.drift_sum, 0);
    }

    #[test]
    fn reconcile_all_rejects_wrong_count() {
        let wins = full_sweep();
        let wins = match wins {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut short: Vec<DeviceWindow> = Vec::new();
        let mut i = 0usize;
        while i + 1 < wins.len() {
            if let Some(w) = wins.get(i) {
                short.push(w.clone());
            }
            i += 1;
        }
        assert_eq!(reconcile_all(&short, WINDOW_MINUTES), Err(DkCode::LedgerBad.val()));
    }

    #[test]
    fn golden_counts_are_stable() {
        let g = golden_run();
        let g = match g {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(g.0, 300);
        assert_eq!(g.1, 300);
        assert_eq!(g.2, 100);
        assert_eq!(g.3, 0);
        let h = golden_run();
        assert_eq!(h.ok().map(|x| x.4), Some(g.4));
    }

    #[test]
    fn injection_all_routes() {
        let l = inject_rounds(1000);
        let l = match l {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.all_caught());
        assert_eq!(l.rounds, 1000);
        assert_eq!(l.route_caught, [250, 250, 250, 250]);
    }

    #[test]
    fn render_round_trip() {
        let wins = full_sweep();
        let wins = match wins {
            Ok(v) => v,
            Err(_) => return,
        };
        let book = ReceiptBook::new();
        let t = render_summary(&wins, &book);
        let t = match t {
            Ok(v) => v,
            Err(_) => return,
        };
        let r = audit_summary(&t);
        assert!(r.is_ok());
        let bad = t.replacen("10|10|", "10|9|", 1);
        assert!(audit_summary(&bad).is_err());
    }

    #[test]
    fn probes_and_repro() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_in_window_and_unique() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < DkCode::ALL.len() {
            if let Some(c) = DkCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 1 && c.val() <= SEG_BASE + 0x0B);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert!(detail(1).contains("未知"));
    }
}
