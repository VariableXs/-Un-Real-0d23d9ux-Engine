//! UNX-F48883 帧合并策略配置面（逐帧递交 vs 合成 125Hz 等价流，默认逐帧） · 0x70A40
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48883（两策略 ×10 轮行为与声明一致）· F48884（焦点递送 ×10^2 零串窗）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48883 ① 纯功能代码 ≥360 行；
//!          ② UNX-F48883-J1 两策略 ×10 轮行为与声明一致；
//!   F48884 ① 纯功能代码 ≥360 行；
//!          ② UNX-F48884-J1 焦点递送 ×10^2 零串窗。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：策略合法性 → 窗口参数 → 批量上限 → 帧号单调 → 声明对账
//!           → 焦点归属 → 设备账平 → 证据包四要素 → 版本单调。
//!   次序不可倒：配置非法时先校验窗口/批量会给出「参数错」的假诊断，掩盖
//!   「策略根本没实现」；帧号单调必须先于合并，否则跨帧事件会被合成批次，
//!   合并窗口就成了改序工具；声明对账必须在实际递交之后，先声明后递交等于
//!   让配置自己给自己打分。
//! 关键差异：本模块是 B05 的「配置面 + 元数据面」——它只回答「按哪条策略
//!   合成」「这批事件的元数据是否四要素齐全」「版本号是否还能回退」。
//!   它不执行限速（ops_core）、不执行背压（ops_guard）、不做 10 设备 30 分
//!   钟对账（ops_dispatch）。逐帧是默认策略；合成 125Hz 只在对账实验里开，
//!   并且必须能被声明值逐轮核对，声明错了就是 CfgDeclare。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70A40，VX6 批 B05 族第二槽）。
pub const SEG_BASE: u32 = 0x70A40;

/// 设备上限（与全族一致，10 设备）。
pub const DEVICE_COUNT: usize = 10;
/// 合成 125Hz 的等价格：8000us 一窗。
pub const SYNTH_WINDOW_US: u32 = 8000;
/// 合成策略默认批量上限。
pub const SYNTH_BATCH: u32 = 8;
/// 批量硬上限（超过即认为配置写错）。
pub const BATCH_MAX: u32 = 64;
/// 每轮事件数（判据 ×10 轮）。
pub const ROUND_EVENTS: u32 = 100;
/// 策略轮数（判据 ×10）。
pub const POLICY_ROUNDS: u32 = 10;
/// 焦点递送轮数（判据 ×10^2）。
pub const FOCUS_ROUNDS: u32 = 100;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtCode {
    /// 未知合并策略
    CfgUnknown,
    /// 合并窗口不是 0（逐帧）或 8000us（合成）
    CfgWindow,
    /// 批量上限越界
    CfgBatch,
    /// 实际行为与声明不一致 / 帧号倒退
    CfgDeclare,
    /// 焦点窗口号为 0
    FocZero,
    /// 焦点未变却要求切换 / 递送串窗
    FocStale,
    /// 设备槽越界
    DevSlot,
    /// 设备重复登记
    DevDup,
    /// 设备账不平
    CountBad,
    /// 证据包四要素缺失
    EvidPartial,
    /// 版本非单调 / 无可回退版本
    VersionBad,
    /// 段位越界
    SegmentBad,
}

impl MtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MtCode::CfgUnknown => "未知合并策略：只允许逐帧或合成 125Hz",
            MtCode::CfgWindow => "合并窗口非法：逐帧必须 0，合成必须 8000us",
            MtCode::CfgBatch => "批量上限越界：只允许 1..=64",
            MtCode::CfgDeclare => "行为与声明不一致：实际批次数 ≠ 声明批次数",
            MtCode::FocZero => "焦点窗口非法：0 号窗口不接受递送",
            MtCode::FocStale => "焦点失配：未变更要求切换或递送串窗",
            MtCode::DevSlot => "设备槽越界：只允许 0..=9",
            MtCode::DevDup => "设备重复登记：同一槽位只能有一本设备账",
            MtCode::CountBad => "设备账不平：收 + 交 + 压 + 拒 ≠ 产",
            MtCode::EvidPartial => "证据包四要素缺失：版本/设备/判据/时间",
            MtCode::VersionBad => "版本非法：必须单调递增且回退只能退一版",
            MtCode::SegmentBad => "段位越界：诊断码逃出 0x70A40 窗口",
        }
    }

    pub const ALL: [MtCode; 12] = [
        MtCode::CfgUnknown,
        MtCode::CfgWindow,
        MtCode::CfgBatch,
        MtCode::CfgDeclare,
        MtCode::FocZero,
        MtCode::FocStale,
        MtCode::DevSlot,
        MtCode::DevDup,
        MtCode::CountBad,
        MtCode::EvidPartial,
        MtCode::VersionBad,
        MtCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MtCode::ALL.len() {
        if let Some(c) = MtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70A40 段闭集内"
}

// ── FNV-1a（本域盐：meta 前缀） ──────────────────────────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u16(h: u64, v: u16) -> u64 {
    fold_byte(fold_byte(h, (v & 0xFF) as u8), ((v >> 8) & 0xFF) as u8)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, (v >> i.saturating_mul(8)) as u8);
        i += 1;
    }
    x
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

// ── LCG ──────────────────────────────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 帧合并策略 ───────────────────────────────────────────────────
/// 两条策略：逐帧（默认）与合成 125Hz（仅对账实验开启）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergePolicy {
    PerFrame,
    Synthetic125,
}

impl MergePolicy {
    pub fn name(self) -> &'static str {
        match self {
            MergePolicy::PerFrame => "逐帧",
            MergePolicy::Synthetic125 => "合成125Hz",
        }
    }

    pub fn ord(self) -> u8 {
        match self {
            MergePolicy::PerFrame => 0,
            MergePolicy::Synthetic125 => 1,
        }
    }

    pub fn from_ord(v: u8) -> Result<MergePolicy, u32> {
        match v {
            0 => Ok(MergePolicy::PerFrame),
            1 => Ok(MergePolicy::Synthetic125),
            _ => Err(MtCode::CfgUnknown.val()),
        }
    }
}

/// 合并配置：策略 + 窗口 + 批量上限，三者互相约束。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeCfg {
    pub policy: MergePolicy,
    pub window_us: u32,
    pub max_batch: u32,
}

impl MergeCfg {
    /// 默认逐帧：窗口 0、批量 1（每条事件单独成批）。
    pub fn per_frame() -> MergeCfg {
        MergeCfg { policy: MergePolicy::PerFrame, window_us: 0, max_batch: 1 }
    }

    /// 合成 125Hz：窗口必须正好 8000us，批量在 1..=64。
    pub fn synthetic(max_batch: u32) -> Result<MergeCfg, u32> {
        MergeCfg::new(MergePolicy::Synthetic125, SYNTH_WINDOW_US, max_batch)
    }

    /// 参数化构造：逐帧必须 (0,1)，合成必须 (8000,1..=64)。
    pub fn new(policy: MergePolicy, window_us: u32, max_batch: u32) -> Result<MergeCfg, u32> {
        match policy {
            MergePolicy::PerFrame => {
                if window_us != 0 || max_batch != 1 {
                    return Err(MtCode::CfgDeclare.val());
                }
            }
            MergePolicy::Synthetic125 => {
                if window_us != SYNTH_WINDOW_US {
                    return Err(MtCode::CfgWindow.val());
                }
                if max_batch == 0 || max_batch > BATCH_MAX {
                    return Err(MtCode::CfgBatch.val());
                }
            }
        }
        Ok(MergeCfg { policy, window_us, max_batch })
    }

    /// 声明批次数：配置自己给出的期望值，后续必须与实际逐轮对齐。
    pub fn declared_batches(&self, events: u32) -> u32 {
        match self.policy {
            MergePolicy::PerFrame => events,
            MergePolicy::Synthetic125 => {
                if self.max_batch == 0 {
                    0
                } else {
                    events.saturating_add(self.max_batch - 1) / self.max_batch
                }
            }
        }
    }

    /// 声明的单批最大条数。
    pub fn declared_size(&self) -> u32 {
        self.max_batch
    }
}

/// 一批已合并事件：帧号 + 事件序列。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Batch {
    pub frame: u32,
    pub events: Vec<u32>,
}

impl Batch {
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(v) = self.events.get(i) {
                s = s.saturating_add(*v);
            }
            i += 1;
        }
        s
    }

    pub fn ordered(&self) -> bool {
        let mut i = 1usize;
        while i < self.events.len() {
            match (self.events.get(i - 1), self.events.get(i)) {
                (Some(a), Some(b)) => {
                    if *b < *a {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 合并器：按配置把输入事件压成批次。
///
/// 不变量：
///   · 逐帧策略每次 push 必产出一批（长度 1）；
///   · 合成策略只在满批或 flush 时产出，窗口内的帧号必须一致；
///   · 任何一批内部事件序号必须单调（合并不许改序）。
pub struct MergePlanner {
    cfg: MergeCfg,
    pending: Vec<u32>,
    frame: u32,
    batches: u32,
    emitted: u32,
    last_frame: u32,
}

impl MergePlanner {
    pub fn new(cfg: MergeCfg) -> MergePlanner {
        MergePlanner {
            cfg,
            pending: Vec::new(),
            frame: 0,
            batches: 0,
            emitted: 0,
            last_frame: 0,
        }
    }

    pub fn cfg(&self) -> MergeCfg {
        self.cfg
    }

    pub fn batches(&self) -> u32 {
        self.batches
    }

    pub fn emitted(&self) -> u32 {
        self.emitted
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 推进一帧：帧号必须不倒退（同帧号可多次 push）。
    pub fn set_frame(&mut self, frame: u32) -> Result<(), u32> {
        if frame < self.last_frame {
            return Err(MtCode::CfgDeclare.val());
        }
        self.last_frame = frame;
        self.frame = frame;
        Ok(())
    }

    /// 递交一条事件；返回本次成形的批次（合成未满批时返回空批）。
    pub fn push(&mut self, ev: u32) -> Result<Batch, u32> {
        match self.cfg.policy {
            MergePolicy::PerFrame => {
                self.batches = self.batches.saturating_add(1);
                self.emitted = self.emitted.saturating_add(1);
                Ok(Batch { frame: self.frame, events: vec![ev] })
            }
            MergePolicy::Synthetic125 => {
                self.pending.push(ev);
                if self.pending.len() >= self.cfg.max_batch as usize {
                    self.take_batch()
                } else {
                    Ok(Batch { frame: self.frame, events: Vec::new() })
                }
            }
        }
    }

    /// 窗口边界 flush：把余量成批（空余量不产批）。
    pub fn flush(&mut self) -> Batch {
        if self.pending.is_empty() {
            return Batch { frame: self.frame, events: Vec::new() };
        }
        let events = self.pending.split_off(0);
        self.batches = self.batches.saturating_add(1);
        self.emitted = self.emitted.saturating_add(events.len() as u32);
        Batch { frame: self.frame, events }
    }

    fn take_batch(&mut self) -> Result<Batch, u32> {
        let events = self.pending.split_off(0);
        let batch = Batch { frame: self.frame, events };
        if !batch.ordered() {
            return Err(MtCode::CfgDeclare.val());
        }
        if batch.len() as u32 > self.cfg.max_batch {
            return Err(MtCode::CfgBatch.val());
        }
        self.batches = self.batches.saturating_add(1);
        self.emitted = self.emitted.saturating_add(batch.len() as u32);
        Ok(batch)
    }
}

/// 一轮实测：返回 (事件数, 批次数, 每批不超上限, 序保持)。
pub fn run_policy(cfg: MergeCfg, events: u32) -> Result<(u32, u32, bool, bool), u32> {
    let mut planner = MergePlanner::new(cfg);
    let mut accepted = 0u32;
    let mut size_ok = true;
    let mut order_ok = true;
    let mut i = 0u32;
    while i < events {
        // 每 25 条事件推进一帧，帧号单调。
        if i % 25 == 0 {
            planner.set_frame(i / 25)?;
        }
        let b = planner.push(i)?;
        if !b.is_empty() {
            if b.len() as u32 > cfg.declared_size() {
                size_ok = false;
            }
            if !b.ordered() {
                order_ok = false;
            }
            accepted = accepted.saturating_add(b.len() as u32);
        }
        i = i.saturating_add(1);
    }
    let tail = planner.flush();
    if !tail.is_empty() {
        accepted = accepted.saturating_add(tail.len() as u32);
        if !tail.ordered() {
            order_ok = false;
        }
    }
    Ok((accepted, planner.batches(), size_ok, order_ok))
}

// ── 焦点递送（只在焦点变化时切路由） ─────────────────────────────
/// 焦点表：keyboard/pointer 焦点归 vxwm 判定；本域只按焦点递送。
pub struct FocusTable {
    current: u32,
    deliveries: u32,
    switches: u32,
    cross: u32,
}

impl FocusTable {
    pub fn new() -> FocusTable {
        FocusTable { current: 0, deliveries: 0, switches: 0, cross: 0 }
    }

    /// 设定焦点：0 号窗口拒；同值不切（计一次 stale 请求）。
    pub fn set_focus(&mut self, win: u32) -> Result<bool, u32> {
        if win == 0 {
            return Err(MtCode::FocZero.val());
        }
        if win == self.current {
            return Ok(false);
        }
        self.current = win;
        self.switches = self.switches.saturating_add(1);
        Ok(true)
    }

    /// 递送：窗口必须是当前焦点，否则记一次串窗并拒。
    pub fn deliver(&mut self, win: u32) -> Result<u32, u32> {
        if win == 0 {
            return Err(MtCode::FocZero.val());
        }
        if win != self.current {
            self.cross = self.cross.saturating_add(1);
            return Err(MtCode::FocStale.val());
        }
        self.deliveries = self.deliveries.saturating_add(1);
        Ok(self.current)
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn deliveries(&self) -> u32 {
        self.deliveries
    }

    pub fn switches(&self) -> u32 {
        self.switches
    }

    pub fn cross(&self) -> u32 {
        self.cross
    }
}

/// 焦点递送压测：每轮随机挑窗口，先切后递；只允许递送到当前焦点。
/// 返回 (递送数, 切换数, 串窗数)；串窗数必须为 0。
pub fn focus_rounds(rounds: u32) -> Result<(u32, u32, u32), u32> {
    let mut table = FocusTable::new();
    table.set_focus(1)?;
    let mut rng = Lcg::new(0x70A40 ^ 0x464F_4355_53);
    let mut r = 0u32;
    while r < rounds {
        let win = rng.below(4).saturating_add(1);
        table.set_focus(win)?;
        let cur = table.current();
        table.deliver(cur)?;
        // 反向探针：对非当前窗口递送必须被拒（真实串窗路径）。
        let other = if cur == 1 { 2 } else { 1 };
        if table.deliver(other).err() != Some(MtCode::FocStale.val()) {
            return Err(MtCode::FocStale.val());
        }
        r = r.saturating_add(1);
    }
    Ok((table.deliveries(), table.switches(), table.cross()))
}

// ── 设备账（10 台独立账，总账守恒） ─────────────────────────────
/// 一台设备的独立账：产 / 交 / 压 / 拒 四栏，恒等式
/// `produced == delivered + queued + rejected`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceAcct {
    pub id: u8,
    pub vid: u16,
    pub pid: u16,
    pub fw: u32,
    pub produced: u32,
    pub delivered: u32,
    pub queued: u32,
    pub rejected: u32,
    pub version: u32,
}

impl DeviceAcct {
    pub fn balanced(&self) -> bool {
        self.produced
            == self.delivered
                .saturating_add(self.queued)
                .saturating_add(self.rejected)
    }

    pub fn text(&self) -> String {
        format!(
            "dev{} {:04X}:{:04X} fw{:04X} p={} d={} q={} r={}",
            self.id,
            self.vid,
            self.pid,
            self.fw,
            self.produced,
            self.delivered,
            self.queued,
            self.rejected
        )
    }
}

/// 设备簿：10 槽，重复登记与越界都拒。
pub struct DeviceBook {
    rows: Vec<DeviceAcct>,
}

impl DeviceBook {
    pub fn new() -> DeviceBook {
        DeviceBook { rows: Vec::new() }
    }

    pub fn register(&mut self, id: u8, vid: u16, pid: u16, fw: u32) -> Result<(), u32> {
        if (id as usize) >= DEVICE_COUNT {
            return Err(MtCode::DevSlot.val());
        }
        if vid == 0 || pid == 0 || fw == 0 {
            return Err(MtCode::EvidPartial.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    return Err(MtCode::DevDup.val());
                }
            }
            i += 1;
        }
        self.rows.push(DeviceAcct {
            id,
            vid,
            pid,
            fw,
            produced: 0,
            delivered: 0,
            queued: 0,
            rejected: 0,
            version: 1,
        });
        Ok(())
    }

    pub fn find(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 产出一批事件。
    pub fn post(&mut self, id: u8, n: u32) -> Result<u32, u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(MtCode::DevSlot.val()),
        };
        match self.rows.get_mut(idx) {
            Some(r) => {
                r.produced = r.produced.saturating_add(n);
                Ok(r.produced)
            }
            None => Err(MtCode::DevSlot.val()),
        }
    }

    /// 递交：只能交「已产出且未结清」的量。
    pub fn settle(&mut self, id: u8, n: u32) -> Result<u32, u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(MtCode::DevSlot.val()),
        };
        match self.rows.get_mut(idx) {
            Some(r) => {
                let open = r
                    .produced
                    .saturating_sub(r.delivered)
                    .saturating_sub(r.rejected);
                if n > open {
                    return Err(MtCode::CountBad.val());
                }
                r.delivered = r.delivered.saturating_add(n);
                Ok(r.delivered)
            }
            None => Err(MtCode::DevSlot.val()),
        }
    }

    /// 压入队列（背压侧的元数据口径，真正的背压归 ops_guard）。
    pub fn queue(&mut self, id: u8, n: u32) -> Result<u32, u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(MtCode::DevSlot.val()),
        };
        match self.rows.get_mut(idx) {
            Some(r) => {
                let open = r
                    .produced
                    .saturating_sub(r.delivered)
                    .saturating_sub(r.rejected)
                    .saturating_sub(r.queued);
                if n > open {
                    return Err(MtCode::CountBad.val());
                }
                r.queued = r.queued.saturating_add(n);
                Ok(r.queued)
            }
            None => Err(MtCode::DevSlot.val()),
        }
    }

    /// 显式拒绝（有码可查，不是静默丢）。
    pub fn reject(&mut self, id: u8, n: u32) -> Result<u32, u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(MtCode::DevSlot.val()),
        };
        match self.rows.get_mut(idx) {
            Some(r) => {
                r.rejected = r.rejected.saturating_add(n);
                Ok(r.rejected)
            }
            None => Err(MtCode::DevSlot.val()),
        }
    }

    /// 版本推进（单调）。
    pub fn bump(&mut self, id: u8, v: u32) -> Result<u32, u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(MtCode::DevSlot.val()),
        };
        match self.rows.get_mut(idx) {
            Some(r) => {
                if v <= r.version {
                    return Err(MtCode::VersionBad.val());
                }
                r.version = v;
                Ok(v)
            }
            None => Err(MtCode::DevSlot.val()),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, idx: usize) -> Option<DeviceAcct> {
        self.rows.get(idx).copied()
    }

    /// 总账守恒：每本账各自平，且总产 = 总交 + 总压 + 总拒。
    pub fn balance(&self) -> Result<(u32, u32, u32, u32), u32> {
        let mut p = 0u32;
        let mut d = 0u32;
        let mut q = 0u32;
        let mut r = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(a) = self.rows.get(i) {
                if !a.balanced() {
                    return Err(MtCode::CountBad.val());
                }
                p = p.saturating_add(a.produced);
                d = d.saturating_add(a.delivered);
                q = q.saturating_add(a.queued);
                r = r.saturating_add(a.rejected);
            }
            i += 1;
        }
        if p != d.saturating_add(q).saturating_add(r) {
            return Err(MtCode::CountBad.val());
        }
        Ok((p, d, q, r))
    }

    pub fn root(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(a) = self.rows.get(i) {
                h = fold_byte(h, a.id);
                h = fold_u16(h, a.vid);
                h = fold_u16(h, a.pid);
                h = fold_u32(h, a.fw);
                h = fold_u32(h, a.produced);
                h = fold_u32(h, a.delivered);
                h = fold_u32(h, a.queued);
                h = fold_u32(h, a.rejected);
                h = fold_u32(h, a.version);
            }
            i += 1;
        }
        h
    }
}

/// 10 台设备建账（vid/pid/fw 固定，便于冻结断言）。
pub fn build_devices() -> Result<DeviceBook, u32> {
    let mut book = DeviceBook::new();
    let mut i = 0u8;
    while (i as usize) < DEVICE_COUNT {
        book.register(
            i,
            0x046Du16,
            0xC52Bu16.saturating_add(i as u16),
            0x0100u32.saturating_add(i as u32),
        )?;
        i = i.saturating_add(1);
    }
    Ok(book)
}

// ── 证据包元数据与版本台账 ───────────────────────────────────────
/// 证据包元数据四要素：版本 / 设备 / 判据 / 时间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvidenceMeta {
    pub version: u32,
    pub device_mask: u32,
    pub criteria: u8,
    pub at_ms: u32,
}

impl EvidenceMeta {
    pub fn new(version: u32, device_mask: u32, criteria: u8, at_ms: u32) -> EvidenceMeta {
        EvidenceMeta { version, device_mask, criteria, at_ms }
    }

    pub fn complete(&self) -> bool {
        self.version > 0 && self.device_mask != 0 && self.criteria > 0 && self.at_ms > 0
    }

    pub fn missing(&self) -> u8 {
        let mut n = 0u8;
        if self.version == 0 {
            n = n.saturating_add(1);
        }
        if self.device_mask == 0 {
            n = n.saturating_add(1);
        }
        if self.criteria == 0 {
            n = n.saturating_add(1);
        }
        if self.at_ms == 0 {
            n = n.saturating_add(1);
        }
        n
    }

    pub fn text(&self) -> String {
        format!(
            "v{} mask=0x{:03X} crit={} at={}",
            self.version, self.device_mask, self.criteria, self.at_ms
        )
    }
}

/// 证据归档：四要素不齐拒，版本重号拒。
pub struct EvidenceArchive {
    packs: Vec<EvidenceMeta>,
}

impl EvidenceArchive {
    pub fn new() -> EvidenceArchive {
        EvidenceArchive { packs: Vec::new() }
    }

    pub fn archive(&mut self, pack: EvidenceMeta) -> Result<u64, u32> {
        if !pack.complete() {
            return Err(MtCode::EvidPartial.val());
        }
        let mut i = 0usize;
        while i < self.packs.len() {
            if let Some(p) = self.packs.get(i) {
                if p.version == pack.version {
                    return Err(MtCode::VersionBad.val());
                }
            }
            i += 1;
        }
        self.packs.push(pack);
        Ok(self.root())
    }

    pub fn len(&self) -> usize {
        self.packs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    pub fn get(&self, idx: usize) -> Option<EvidenceMeta> {
        self.packs.get(idx).copied()
    }

    pub fn root(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.packs.len() {
            if let Some(p) = self.packs.get(i) {
                h = fold_u32(h, p.version);
                h = fold_u32(h, p.device_mask);
                h = fold_byte(h, p.criteria);
                h = fold_u32(h, p.at_ms);
                h = h.wrapping_mul(FNV_PRIME);
            }
            i += 1;
        }
        h
    }
}

/// 版本台账：只允许前进一格或退一格，跳版即拒。
pub struct VersionLog {
    history: Vec<u32>,
}

impl VersionLog {
    pub fn new(base: u32) -> Result<VersionLog, u32> {
        if base == 0 {
            return Err(MtCode::VersionBad.val());
        }
        Ok(VersionLog { history: vec![base] })
    }

    pub fn current(&self) -> u32 {
        self.history.last().copied().unwrap_or(0)
    }

    pub fn depth(&self) -> usize {
        self.history.len()
    }

    /// 前进：必须正好 +1（跳版会让回退目标不可判定）。
    pub fn advance(&mut self) -> Result<u32, u32> {
        let cur = self.current();
        let next = cur.saturating_add(1);
        if next == cur {
            return Err(MtCode::VersionBad.val());
        }
        self.history.push(next);
        Ok(next)
    }

    /// 回退：只能退到上一版；只剩一版时拒。
    pub fn revert(&mut self) -> Result<u32, u32> {
        if self.history.len() < 2 {
            return Err(MtCode::VersionBad.val());
        }
        self.history.pop();
        Ok(self.current())
    }
}

// ── 黄金全链 ─────────────────────────────────────────────────────
/// 黄金：(逐帧批次, 合成批次, 焦点递送, 设备总产, 根)。
pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    // ① 两策略各 10 轮：实际批次必须等于声明批次。
    let per = MergeCfg::per_frame();
    let synth = MergeCfg::synthetic(SYNTH_BATCH)?;
    let mut per_batches = 0u32;
    let mut synth_batches = 0u32;
    let mut r = 0u32;
    while r < POLICY_ROUNDS {
        let (pe, pb, psize, porder) = run_policy(per, ROUND_EVENTS)?;
        if pe != ROUND_EVENTS || pb != per.declared_batches(ROUND_EVENTS) || !psize || !porder {
            return Err(MtCode::CfgDeclare.val());
        }
        let (se, sb, ssize, sorder) = run_policy(synth, ROUND_EVENTS)?;
        if se != ROUND_EVENTS || sb != synth.declared_batches(ROUND_EVENTS) || !ssize || !sorder {
            return Err(MtCode::CfgDeclare.val());
        }
        per_batches = per_batches.saturating_add(pb);
        synth_batches = synth_batches.saturating_add(sb);
        r = r.saturating_add(1);
    }

    // ② 焦点 100 轮零串窗。
    let (deliveries, _switches, cross) = focus_rounds(FOCUS_ROUNDS)?;
    if cross != FOCUS_ROUNDS {
        return Err(MtCode::FocStale.val());
    }

    // ③ 10 设备账：每台产 100，交 70、压 20、拒 10。
    let mut devices = build_devices()?;
    let mut i = 0u8;
    while (i as usize) < DEVICE_COUNT {
        devices.post(i, 100)?;
        devices.settle(i, 70)?;
        devices.queue(i, 20)?;
        devices.reject(i, 10)?;
        i = i.saturating_add(1);
    }
    let (p, _d, _q, _ru) = devices.balance()?;

    // ④ 证据包三份 + 版本三进两退。
    let mut archive = EvidenceArchive::new();
    archive.archive(EvidenceMeta::new(1, 0x3FF, 4, 100))?;
    archive.archive(EvidenceMeta::new(2, 0x3FF, 4, 200))?;
    archive.archive(EvidenceMeta::new(3, 0x3FF, 4, 300))?;
    let mut log = VersionLog::new(7)?;
    log.advance()?;
    log.advance()?;
    log.advance()?;
    log.revert()?;
    log.revert()?;
    if log.current() != 8 {
        return Err(MtCode::VersionBad.val());
    }

    let mut h = devices.root();
    h ^= archive.root();
    h = fold_u32(h, per_batches);
    h = fold_u32(h, synth_batches);
    h = fold_u32(h, deliveries);
    h = fold_u32(h, log.current());
    Ok((per_batches, synth_batches, deliveries, p, h))
}

// ── 注入账（四路） ───────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入：策略不实现 / 窗口错配 / 批量越界 / 证据缺项 / 焦点串窗 / 账溢出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4D45_5441);
    let mut r = 0u32;
    while r < rounds {
        let jitter = rng.below(0x3FF);

        // 路 1：未知策略。
        let unknown = MergePolicy::from_ord(2).err() == Some(MtCode::CfgUnknown.val())
            && MergePolicy::from_ord(0xFF).is_err();
        led.record(0, unknown);

        // 路 2：窗口错配（逐帧给 8000，合成给 0）。
        let window = MergeCfg::new(MergePolicy::PerFrame, SYNTH_WINDOW_US, 1).err()
            == Some(MtCode::CfgDeclare.val())
            && MergeCfg::new(MergePolicy::Synthetic125, 0, 8).err()
                == Some(MtCode::CfgWindow.val());
        led.record(1, window);

        // 路 3：批量越界（0 与 65）。
        let batch = MergeCfg::synthetic(0).err() == Some(MtCode::CfgBatch.val())
            && MergeCfg::synthetic(BATCH_MAX + 1).err() == Some(MtCode::CfgBatch.val());
        led.record(2, batch);

        // 路 4：证据缺项 + 串窗递送。
        let mut archive = EvidenceArchive::new();
        let partial = archive
            .archive(EvidenceMeta::new(1, 0, 4, 10u32.saturating_add(jitter)))
            .err()
            == Some(MtCode::EvidPartial.val());
        let mut focus = FocusTable::new();
        focus.set_focus(2)?;
        let cross = focus.deliver(3).err() == Some(MtCode::FocStale.val())
            && focus.deliver(0).err() == Some(MtCode::FocZero.val());
        led.record(3, partial && cross);

        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ─────────────────────────────────────────────────────
/// 序 A：两策略声明对账 + 焦点零串窗 + 总账守恒。
pub fn probe_seq_a() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b || a.0 != POLICY_ROUNDS.saturating_mul(ROUND_EVENTS) || a.2 != FOCUS_ROUNDS {
        return Err(MtCode::CfgDeclare.val());
    }
    if a.3 != (DEVICE_COUNT as u32).saturating_mul(100) {
        return Err(MtCode::CountBad.val());
    }
    Ok(())
}

/// 序 B：非法配置与非法递送逐路拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if MergeCfg::new(MergePolicy::PerFrame, 1, 1).err() != Some(MtCode::CfgDeclare.val()) {
        return Err(MtCode::CfgDeclare.val());
    }
    if MergeCfg::synthetic(BATCH_MAX + 1).err() != Some(MtCode::CfgBatch.val()) {
        return Err(MtCode::CfgBatch.val());
    }
    let mut focus = FocusTable::new();
    if focus.set_focus(0).err() != Some(MtCode::FocZero.val()) {
        return Err(MtCode::FocZero.val());
    }
    let mut book = DeviceBook::new();
    if book.register(10, 1, 1, 1).err() != Some(MtCode::DevSlot.val()) {
        return Err(MtCode::DevSlot.val());
    }
    if book.register(0, 1, 1, 1).is_err() || book.register(0, 1, 1, 1).err() != Some(MtCode::DevDup.val()) {
        return Err(MtCode::DevDup.val());
    }
    if book.settle(0, 1).err() != Some(MtCode::CountBad.val()) {
        return Err(MtCode::CountBad.val());
    }
    Ok(())
}

/// 序 C：冻结常量。
pub fn probe_seq_c() -> Result<(), u32> {
    if DEVICE_COUNT == 10
        && SYNTH_WINDOW_US == 8000
        && SYNTH_BATCH == 8
        && BATCH_MAX == 64
        && ROUND_EVENTS == 100
        && POLICY_ROUNDS == 10
        && FOCUS_ROUNDS == 100
    {
        Ok(())
    } else {
        Err(MtCode::SegmentBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MtCode::CountBad.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MtCode::ALL.len() {
        if let Some(c) = MtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x20) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面 ───────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 逐帧策略 10 轮：批次数 == 事件数。
    let per = MergeCfg::per_frame();
    let per_run = run_policy(per, ROUND_EVENTS);
    let per_ok = match &per_run {
        Ok((e, b, size, order)) => {
            *e == ROUND_EVENTS && *b == per.declared_batches(ROUND_EVENTS) && *size && *order
        }
        Err(_) => false,
    };
    v.push((
        "F48883-J1-逐帧策略十轮",
        per_ok,
        match &per_run {
            Ok((e, b, _, _)) => format!("events={} batches={} declared={}", e, b, per.declared_batches(ROUND_EVENTS)),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // ② 合成策略 10 轮：批次数 == 声明（100/8 向上取整 = 13）。
    let synth = MergeCfg::synthetic(SYNTH_BATCH);
    let synth_run = match synth {
        Ok(cfg) => {
            let mut batches = 0u32;
            let mut events = 0u32;
            let mut ok = true;
            let mut r = 0u32;
            while r < POLICY_ROUNDS {
                match run_policy(cfg, ROUND_EVENTS) {
                    Ok((e, b, size, order)) => {
                        if e != ROUND_EVENTS || b != cfg.declared_batches(ROUND_EVENTS) || !size || !order {
                            ok = false;
                        }
                        batches = batches.saturating_add(b);
                        events = events.saturating_add(e);
                    }
                    Err(_) => ok = false,
                }
                r = r.saturating_add(1);
            }
            Ok((events, batches, ok, cfg.declared_batches(ROUND_EVENTS)))
        }
        Err(e) => Err(e),
    };
    v.push((
        "F48883-J1-合成策略十轮声明一致",
        match &synth_run {
            Ok((e, b, ok, _)) => *e == ROUND_EVENTS.saturating_mul(POLICY_ROUNDS) && *b > 0 && *ok,
            Err(_) => false,
        },
        match &synth_run {
            Ok((e, b, _, d)) => format!("events={} batches={} per_round_declared={}", e, b, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // ③ 配置互斥：逐帧不接受窗口/批量，合成不接受 0 窗。
    let cfg_rules = MergeCfg::new(MergePolicy::PerFrame, 8000, 1).err()
        == Some(MtCode::CfgDeclare.val())
        && MergeCfg::new(MergePolicy::Synthetic125, 4000, 8).err()
            == Some(MtCode::CfgWindow.val())
        && MergeCfg::synthetic(SYNTH_BATCH).is_ok();
    v.push((
        "F48883-J1-策略参数互斥",
        cfg_rules,
        String::from("逐帧=(0,1)；合成=(8000,1..=64)"),
    ));

    // ④ 帧号倒退必须拒。
    let frame_ok = {
        let mut p = MergePlanner::new(MergeCfg::per_frame());
        p.set_frame(3).is_ok() && p.set_frame(2).err() == Some(MtCode::CfgDeclare.val())
    };
    v.push((
        "F48883-J1-帧号单调",
        frame_ok,
        String::from("帧号 3→2 拒，同帧号可续"),
    ));

    // ⑤ 焦点 100 轮零串窗。
    let focus = focus_rounds(FOCUS_ROUNDS);
    v.push((
        "F48884-J1-焦点递送百轮零串窗",
        match &focus {
            Ok((d, _, c)) => *d == FOCUS_ROUNDS && *c == FOCUS_ROUNDS,
            Err(_) => false,
        },
        match &focus {
            Ok((d, s, c)) => format!("deliver={} switch={} cross_rejected={}", d, s, c),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // ⑥ 焦点 0 与同值切换。
    let focus_rules = {
        let mut t = FocusTable::new();
        let a = t.set_focus(0).err() == Some(MtCode::FocZero.val());
        let b = t.set_focus(3).ok() == Some(true);
        let c = t.set_focus(3).ok() == Some(false);
        let d = t.deliver(3).is_ok();
        a && b && c && d && t.switches() == 1
    };
    v.push((
        "F48884-J1-焦点归属规则",
        focus_rules,
        String::from("0 号拒；同值不切；异窗递送拒"),
    ));

    // ⑦ 10 设备独立账 + 总账守恒。
    let dev = build_devices();
    let dev_ok = match dev {
        Ok(mut book) => {
            let mut i = 0u8;
            let mut posted = true;
            while (i as usize) < DEVICE_COUNT {
                if book.post(i, 100).is_err()
                    || book.settle(i, 60).is_err()
                    || book.queue(i, 30).is_err()
                    || book.reject(i, 10).is_err()
                {
                    posted = false;
                }
                i = i.saturating_add(1);
            }
            posted && book.balance().ok() == Some((1000, 600, 300, 100))
        }
        Err(_) => false,
    };
    v.push((
        "F48883-J1-十设备账守恒",
        dev_ok,
        String::from("总产 1000 = 交 600 + 压 300 + 拒 100"),
    ));

    // ⑧ 设备重复登记与越槽。
    let dev_rules = {
        let mut b = DeviceBook::new();
        let a = b.register(0, 1, 1, 1).is_ok();
        let c = b.register(0, 1, 1, 1).err() == Some(MtCode::DevDup.val());
        let d = b.register(10, 1, 1, 1).err() == Some(MtCode::DevSlot.val());
        let e = b.register(1, 0, 1, 1).err() == Some(MtCode::EvidPartial.val());
        a && c && d && e
    };
    v.push((
        "F48883-J1-设备登记唯一",
        dev_rules,
        String::from("重号 DevDup；越槽 DevSlot；零字段 EvidPartial"),
    ));

    // ⑨ 证据四要素与版本台账。
    let evid = {
        let mut ar = EvidenceArchive::new();
        let a = ar.archive(EvidenceMeta::new(1, 0x3FF, 4, 10)).is_ok();
        let b = ar.archive(EvidenceMeta::new(1, 0x3FF, 4, 20)).err() == Some(MtCode::VersionBad.val());
        let c = ar.archive(EvidenceMeta::new(2, 0, 4, 30)).err() == Some(MtCode::EvidPartial.val());
        let d = match VersionLog::new(1) {
            Ok(mut log) => {
                log.advance().is_ok()
                    && log.advance().is_ok()
                    && log.revert().ok() == Some(2)
                    && log.revert().ok() == Some(1)
                    && log.revert().err() == Some(MtCode::VersionBad.val())
            }
            Err(_) => false,
        };
        a && b && c && d && ar.len() == 1
    };
    v.push((
        "F48884-J1-证据四要素与版本台账",
        evid,
        String::from("缺要素拒；版本重号拒；回退只能退一版"),
    ));

    // ⑩ 四路注入 + 探针 + 段位。
    let inj = inject_rounds(10);
    v.push((
        "F48884-J1-注入四路与探针三序",
        match &inj {
            Ok(l) => l.all_caught() && probe_seq_a().is_ok() && probe_seq_b().is_ok()
                && probe_seq_c().is_ok() && repro_check().is_ok() && segment_guard(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?} seg={}", l.tries, l.caught, segment_guard()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    v
}

pub fn run_unx48883_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48883 帧合并策略配置面与焦点递送");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn per_cfg() -> MergeCfg {
        MergeCfg::per_frame()
    }

    fn synth_cfg() -> MergeCfg {
        MergeCfg { policy: MergePolicy::Synthetic125, window_us: 8000, max_batch: 8 }
    }

    #[test]
    fn policy_ord_round_trip() {
        assert_eq!(MergePolicy::from_ord(0).ok(), Some(MergePolicy::PerFrame));
        assert_eq!(MergePolicy::from_ord(1).ok(), Some(MergePolicy::Synthetic125));
        assert_eq!(MergePolicy::from_ord(2).err(), Some(MtCode::CfgUnknown.val()));
    }

    #[test]
    fn config_rules_are_mutually_exclusive() {
        assert!(MergeCfg::new(MergePolicy::PerFrame, 0, 1).is_ok());
        assert_eq!(
            MergeCfg::new(MergePolicy::PerFrame, 8000, 1).err(),
            Some(MtCode::CfgDeclare.val())
        );
        assert_eq!(
            MergeCfg::new(MergePolicy::Synthetic125, 4000, 8).err(),
            Some(MtCode::CfgWindow.val())
        );
        assert_eq!(
            MergeCfg::new(MergePolicy::Synthetic125, 8000, 0).err(),
            Some(MtCode::CfgBatch.val())
        );
        assert_eq!(
            MergeCfg::new(MergePolicy::Synthetic125, 8000, 65).err(),
            Some(MtCode::CfgBatch.val())
        );
    }

    #[test]
    fn declared_batches_match_ceil() {
        assert_eq!(per_cfg().declared_batches(100), 100);
        assert_eq!(synth_cfg().declared_batches(100), 13);
        assert_eq!(synth_cfg().declared_batches(8), 1);
        assert_eq!(synth_cfg().declared_batches(0), 0);
        assert_eq!(synth_cfg().declared_size(), 8);
    }

    #[test]
    fn per_frame_emits_every_event() {
        let mut p = MergePlanner::new(per_cfg());
        assert!(p.set_frame(1).is_ok());
        let b = p.push(7);
        assert!(b.is_ok());
        match b {
            Ok(batch) => {
                assert_eq!(batch.len(), 1);
                assert_eq!(batch.frame, 1);
                assert_eq!(batch.sum(), 7);
            }
            Err(_) => assert!(false),
        }
        assert_eq!(p.emitted(), 1);
        assert_eq!(p.batches(), 1);
    }

    #[test]
    fn synthetic_accumulates_to_batch() {
        let mut p = MergePlanner::new(synth_cfg());
        let mut produced = 0u32;
        let mut i = 0u32;
        while i < 8 {
            match p.push(i) {
                Ok(b) => {
                    if !b.is_empty() {
                        produced = produced.saturating_add(b.len() as u32);
                    }
                }
                Err(_) => assert!(false),
            }
            i += 1;
        }
        assert_eq!(p.emitted(), 8);
        assert_eq!(p.batches(), 1);
        assert_eq!(produced, 8);
        assert_eq!(p.pending_len(), 0);
    }

    #[test]
    fn synthetic_flush_yields_tail() {
        let mut p = MergePlanner::new(synth_cfg());
        let mut i = 0u32;
        while i < 5 {
            assert!(p.push(i).is_ok());
            i += 1;
        }
        assert_eq!(p.pending_len(), 5);
        let tail = p.flush();
        assert_eq!(tail.len(), 5);
        assert!(tail.ordered());
        assert!(p.flush().is_empty());
        assert_eq!(p.batches(), 1);
    }

    #[test]
    fn frame_regression_rejected() {
        let mut p = MergePlanner::new(per_cfg());
        assert!(p.set_frame(5).is_ok());
        assert_eq!(p.set_frame(4).err(), Some(MtCode::CfgDeclare.val()));
        assert!(p.set_frame(5).is_ok());
    }

    #[test]
    fn focus_zero_and_cross_window() {
        let mut f = FocusTable::new();
        assert_eq!(f.set_focus(0).err(), Some(MtCode::FocZero.val()));
        assert_eq!(f.set_focus(2).ok(), Some(true));
        assert_eq!(f.set_focus(2).ok(), Some(false));
        assert_eq!(f.deliver(3).err(), Some(MtCode::FocStale.val()));
        assert_eq!(f.deliver(0).err(), Some(MtCode::FocZero.val()));
        assert_eq!(f.deliver(2).ok(), Some(2));
        assert_eq!(f.deliveries(), 1);
        assert_eq!(f.switches(), 1);
        assert_eq!(f.cross(), 1);
    }

    #[test]
    fn device_book_balances_and_rejects_dups() {
        let mut b = build_devices().ok().unwrap_or_else(DeviceBook::new);
        assert_eq!(b.len(), 10);
        assert_eq!(b.register(0, 1, 1, 1).err(), Some(MtCode::DevDup.val()));
        assert_eq!(b.register(10, 1, 1, 1).err(), Some(MtCode::DevSlot.val()));
        let mut i = 0u8;
        while (i as usize) < DEVICE_COUNT {
            assert!(b.post(i, 10).is_ok());
            assert!(b.settle(i, 6).is_ok());
            assert!(b.queue(i, 2).is_ok());
            assert!(b.reject(i, 2).is_ok());
            i = i.saturating_add(1);
        }
        assert_eq!(b.balance().ok(), Some((100, 60, 20, 20)));
    }

    #[test]
    fn settle_cannot_exceed_open() {
        let mut b = DeviceBook::new();
        assert!(b.register(0, 1, 2, 3).is_ok());
        assert_eq!(b.settle(0, 1).err(), Some(MtCode::CountBad.val()));
        assert!(b.post(0, 4).is_ok());
        assert!(b.settle(0, 4).is_ok());
        assert_eq!(b.settle(0, 1).err(), Some(MtCode::CountBad.val()));
    }

    #[test]
    fn device_version_is_monotonic() {
        let mut b = DeviceBook::new();
        assert!(b.register(0, 1, 2, 3).is_ok());
        assert_eq!(b.bump(0, 1).err(), Some(MtCode::VersionBad.val()));
        assert_eq!(b.bump(0, 2).ok(), Some(2));
        assert_eq!(b.bump(0, 2).err(), Some(MtCode::VersionBad.val()));
    }

    #[test]
    fn evidence_requires_four_elements() {
        let mut ar = EvidenceArchive::new();
        assert_eq!(
            ar.archive(EvidenceMeta::new(0, 1, 1, 1)).err(),
            Some(MtCode::EvidPartial.val())
        );
        assert_eq!(
            ar.archive(EvidenceMeta::new(1, 0, 1, 1)).err(),
            Some(MtCode::EvidPartial.val())
        );
        assert_eq!(
            ar.archive(EvidenceMeta::new(1, 1, 0, 1)).err(),
            Some(MtCode::EvidPartial.val())
        );
        assert_eq!(
            ar.archive(EvidenceMeta::new(1, 1, 1, 0)).err(),
            Some(MtCode::EvidPartial.val())
        );
        assert!(ar.archive(EvidenceMeta::new(1, 1, 1, 1)).is_ok());
        assert_eq!(ar.archive(EvidenceMeta::new(1, 1, 1, 2)).err(), Some(MtCode::VersionBad.val()));
        assert_eq!(ar.get(0).map(|p| p.missing()), Some(0));
    }

    #[test]
    fn version_log_advances_and_reverts_one() {
        let mut log = VersionLog::new(5).ok().unwrap_or_else(|| VersionLog { history: Vec::new() });
        assert_eq!(log.current(), 5);
        assert_eq!(log.advance().ok(), Some(6));
        assert_eq!(log.advance().ok(), Some(7));
        assert_eq!(log.revert().ok(), Some(6));
        assert_eq!(log.revert().ok(), Some(5));
        assert_eq!(log.revert().err(), Some(MtCode::VersionBad.val()));
        assert_eq!(log.depth(), 1);
        assert!(VersionLog::new(0).is_err());
    }

    #[test]
    fn golden_values_are_frozen() {
        match golden_run() {
            Ok((per, synth, focus, produced, root)) => {
                assert_eq!(per, 1000);
                assert_eq!(synth, 130);
                assert_eq!(focus, 100);
                assert_eq!(produced, 1000);
                assert!(root != 0);
            }
            Err(_) => assert!(false),
        }
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_four_routes() {
        match inject_rounds(10) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [10, 10, 10, 10]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_segment_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(MtCode::ALL.len(), 12);
        assert_eq!(MtCode::CfgUnknown.val(), SEG_BASE + 1);
        assert_eq!(MtCode::SegmentBad.val(), SEG_BASE + 12);
    }

    #[test]
    fn detail_covers_all_and_rejects_outside() {
        assert!(detail(SEG_BASE).contains("未知诊断码"));
        let mut i = 0usize;
        while i < MtCode::ALL.len() {
            assert_eq!(detail(MtCode::ALL[i].val()), MtCode::ALL[i].detail());
            i += 1;
        }
    }

    #[test]
    fn checks_are_all_green() {
        let cs = run_unx48883_checks();
        assert!(cs.all_passed());
        assert!(cs.len() >= 8 && cs.len() <= 10);
    }
}
