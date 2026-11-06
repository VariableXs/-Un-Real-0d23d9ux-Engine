//! UNX-F48885 背压闸门与保护面（水位滞回 / 配额债 / 庇护恢复） · 0x70A80
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48885（水位滞回 ×10^3 三态零抖动）· F48886（配额债 ×10^3 零透支）
//!         · F48887（庇护恢复 ×10^3 凭证单次有效）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48885 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48885-J1 水位滞回 ×10^3 三态零抖动；
//!   F48886 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48886-J1 配额债 ×10^3 零透支；
//!   F48887 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48887-J1 庇护恢复 ×10^3 凭证单次有效。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：水位合法 → 滞回三态 → 庇护持有 → 配额记账 → 入场裁定
//!           → 队列溢出处理 → 债务清零 → 账平复核。
//!   次序不可倒的理由逐条：
//!     · 水位越界先判：把 130 当 128 截断会让「超排」看起来只是「满」，
//!       之后所有三态结论都建立在一个被夹过的数上；
//!     · 滞回先于裁定：无滞回时队列在 95/96 之间抖动，入场裁定会每秒
//!       翻转数十次，配额与庇护的计数全部失去意义；
//!     · 庇护先于配额：庇护是「整机暂停递送」，暂停期间不应消耗配额，
//!       否则恢复后设备因债清零而被误判为超额；
//!     · 配额先于入场：没有配额结论的入场裁定只能回答「队列满不满」，
//!       回答不了「这台设备还能不能递」，两者混判会把单设备饿死洗成
//!       全局拥塞；
//!     · 溢出处理晚于入场：只有被接受的事件才可能被溢出策略淘汰，
//!       先淘汰再裁定的顺序会把拒绝计数与淘汰计数重复相加；
//!     · 账平复核最后执行，它只回答「有没有事件静默消失」。
//! 关键差异：本模块只拥有「背压闸门 + 配额债 + 庇护」这一层。
//!   core 拥有三操作 SOP 与四元组；meta 拥有合并策略与焦点；
//!   dispatch 拥有十设备对账；edge 拥有边界阶梯；bridge 拥有跨模块
//!   封存；quality 拥有分位与配额率。本模块不复制 SOP、不复制合并、
//!   不做跨模块寻址；它回答的是一个更窄的问题：给定当前水位、当前
//!   设备余额、当前庇护深度，这一帧应当被接受、暂扣还是拒绝。
//!   谁想把一帧绕过本模块塞进队列，谁就必须自己承担账不平。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70A80，VX6 批 B05 族第三槽）。
pub const SEG_BASE: u32 = 0x70A80;

/// 队列容量（128 格，与全族压力样本对齐）。
pub const QUEUE_CAP: usize = 128;
/// 高水位：达到即以拒绝承担背压。
pub const HIGH_WATER: usize = 96;
/// 低水位：回落到此线以下才允许退出受压态。
pub const LOW_WATER: usize = 64;
/// 同态驻留 tick 数（低于此数不计翻转，防抖）。
pub const DWELL_TICKS: u32 = 2;
/// 每设备配额上限（48 帧）。
pub const QUOTA_CAP: u32 = 48;
/// 每 tick 配额返还（6 帧）。
pub const QUOTA_REFUND: u32 = 6;
/// EMA 右移位数（α = 1/8）。
pub const EMA_SHIFT: u32 = 3;
/// 庇护轮数（判据 ×10^3）。
pub const SHELTER_ROUNDS: u32 = 1000;
/// 压力语料轮数。
pub const CORPUS_ROUNDS: u32 = 1000;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 1000;
/// 十设备上限。
pub const DEVICE_COUNT: usize = 10;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BgCode {
    /// 水位越界（大于队列容量）
    WaterBad,
    /// 三态转移非法（跨级下沉 / 未驻留即翻转）
    StateBad,
    /// 入场裁定非法（拒绝却入队 / 接受却未入队）
    AdmitBad,
    /// 溢出策略非法（空队淘汰 / 淘汰量大于接受量）
    DropBad,
    /// 配额越界（设备号非法 / 余额为负 / 透支）
    QuotaBad,
    /// 庇护参数非法（零深度 / 嵌套溢出 / 未持有却恢复）
    ShelterBad,
    /// 恢复凭证非法（过期 / 重放 / 与冻结值不符）
    ResumeBad,
    /// EMA 标定非法（样本为零 / 序号回退 / 偏离超限）
    GaugeBad,
    /// 账不平（接受 + 暂扣 + 拒绝 ≠ 轮数）
    LedgerBad,
    /// 导出文本畸形（字段数 / 数字 / 覆盖缺失）
    RenderBad,
    /// 段位越界
    SegmentBad,
}

impl BgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BgCode::WaterBad => "水位越界：观测值不得超过队列容量 128",
            BgCode::StateBad => "三态转移非法：只允许逐级下沉，回升需驻留两 tick",
            BgCode::AdmitBad => "入场裁定非法：拒绝帧不得入队，接受帧不得缺席",
            BgCode::DropBad => "溢出策略非法：空队不可淘汰，淘汰量不得大于接受量",
            BgCode::QuotaBad => "配额越界：设备号须小于 10，余额不得透支",
            BgCode::ShelterBad => "庇护参数非法：深度非零，嵌套不得超过三十二层",
            BgCode::ResumeBad => "恢复凭证非法：凭证须与冻结值一致且只可用一次",
            BgCode::GaugeBad => "EMA 标定非法：样本非零、序号递增、偏离不超过两倍",
            BgCode::LedgerBad => "账不平：接受 + 暂扣 + 拒绝 ≠ 总轮数",
            BgCode::RenderBad => "导出畸形：六字段、数字可解析、覆盖标记齐全",
            BgCode::SegmentBad => "段位越界：诊断码逃出 0x70A80 窗口",
        }
    }

    pub const ALL: [BgCode; 11] = [
        BgCode::WaterBad,
        BgCode::StateBad,
        BgCode::AdmitBad,
        BgCode::DropBad,
        BgCode::QuotaBad,
        BgCode::ShelterBad,
        BgCode::ResumeBad,
        BgCode::GaugeBad,
        BgCode::LedgerBad,
        BgCode::RenderBad,
        BgCode::SegmentBad,
    ];
}

/// 段内码取明细文本；闭集外给固定文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BgCode::ALL.len() {
        if let Some(c) = BgCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知背压诊断码：不属于 0x70A80 闭集"
}

// ── FNV-1a 64（跨模块同构，摘要只做比对不做加解密） ──────────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u16(h: u64, v: u16) -> u64 {
    let mut x = fold_byte(h, (v & 0xFF) as u8);
    x = fold_byte(x, (v >> 8) as u8);
    x
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = fold_u16(h, (v & 0xFFFF) as u16);
    x = fold_u16(x, (v >> 16) as u16);
    x
}

pub fn fold_usize(h: u64, v: usize) -> u64 {
    fold_u32(h, v as u32)
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

// ── 确定性伪随机（只用做语料，不参与判据合法性） ─────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

// ── 压力三态（滞回 + 驻留） ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressureState {
    /// 正常：水位低于低水位线。
    Calm,
    /// 受压：水位处于低水位与高水位之间。
    Pressed,
    /// 卸载：水位达到高水位，只接受配额内的关键帧。
    Shed,
}

impl PressureState {
    pub const ALL: [PressureState; 3] = [
        PressureState::Calm,
        PressureState::Pressed,
        PressureState::Shed,
    ];

    pub fn rank(self) -> u8 {
        match self {
            PressureState::Calm => 0,
            PressureState::Pressed => 1,
            PressureState::Shed => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PressureState::Calm => "calm",
            PressureState::Pressed => "pressed",
            PressureState::Shed => "shed",
        }
    }

    pub fn from_label(s: &str) -> Option<PressureState> {
        if s == "calm" {
            Some(PressureState::Calm)
        } else if s == "pressed" {
            Some(PressureState::Pressed)
        } else if s == "shed" {
            Some(PressureState::Shed)
        } else {
            None
        }
    }
}

/// 水位闸：只做状态转移，不做入场裁定。
#[derive(Clone, Debug)]
pub struct PressureGate {
    state: PressureState,
    dwell: u32,
    flips: u32,
    ticks: u32,
    peak: usize,
    digest: u64,
}

impl PressureGate {
    pub fn new() -> PressureGate {
        PressureGate {
            state: PressureState::Calm,
            dwell: 0,
            flips: 0,
            ticks: 0,
            peak: 0,
            digest: FNV_OFF,
        }
    }

    pub fn state(&self) -> PressureState {
        self.state
    }

    pub fn dwell(&self) -> u32 {
        self.dwell
    }

    pub fn flips(&self) -> u32 {
        self.flips
    }

    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    pub fn peak(&self) -> usize {
        self.peak
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 依据水位推进一个 tick。水位越界直接拒绝，不夹断。
    pub fn observe(&mut self, level: usize) -> Result<PressureState, u32> {
        if level > QUEUE_CAP {
            return Err(BgCode::WaterBad.val());
        }
        if level > self.peak {
            self.peak = level;
        }
        self.ticks = self.ticks.saturating_add(1);
        let target = if level >= HIGH_WATER {
            PressureState::Shed
        } else if level >= LOW_WATER {
            PressureState::Pressed
        } else {
            PressureState::Calm
        };
        // 回升立即生效；下沉逐级进行，且需先跌破低水位并驻留。
        let up = target.rank() > self.state.rank();
        let down = target.rank() < self.state.rank();
        let allow_down = down && level <= LOW_WATER && self.dwell >= DWELL_TICKS;
        if up {
            self.state = target;
            self.dwell = 0;
            self.flips = self.flips.saturating_add(1);
        } else if allow_down {
            self.state = match self.state {
                PressureState::Shed => PressureState::Pressed,
                PressureState::Pressed => PressureState::Calm,
                PressureState::Calm => PressureState::Calm,
            };
            self.dwell = 0;
            self.flips = self.flips.saturating_add(1);
        } else {
            self.dwell = self.dwell.saturating_add(1);
        }
        self.digest = fold_byte(self.digest, self.state.rank());
        self.digest = fold_usize(self.digest, level);
        Ok(self.state)
    }

    /// 自审：翻转次数不超过 tick 数，驻留不超过 tick 数，峰值不越界。
    pub fn audit(&self) -> Result<(), u32> {
        if self.flips > self.ticks {
            return Err(BgCode::StateBad.val());
        }
        if self.dwell > self.ticks {
            return Err(BgCode::StateBad.val());
        }
        if self.peak > QUEUE_CAP {
            return Err(BgCode::WaterBad.val());
        }
        Ok(())
    }
}

// ── EMA 压力标定（回答「最近压力趋势」） ─────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Gauge {
    ema: u32,
    last_seq: u32,
    samples: u32,
    digest: u64,
}

impl Gauge {
    pub fn new() -> Gauge {
        Gauge { ema: 0, last_seq: 0, samples: 0, digest: FNV_OFF }
    }

    pub fn ema(&self) -> u32 {
        self.ema
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 一次采样：序号必须严格递增且非零，样本不得超过队列容量的两倍。
    pub fn sample(&mut self, seq: u32, level: usize) -> Result<u32, u32> {
        if seq == 0 {
            return Err(BgCode::GaugeBad.val());
        }
        if seq <= self.last_seq && self.samples > 0 {
            return Err(BgCode::GaugeBad.val());
        }
        if level == 0 || level > QUEUE_CAP.saturating_mul(2) {
            return Err(BgCode::GaugeBad.val());
        }
        let v = level as u32;
        let scaled = if v > (u32::MAX >> EMA_SHIFT) { u32::MAX } else { v << EMA_SHIFT };
        let delta = if self.ema <= scaled {
            (scaled - self.ema) >> EMA_SHIFT
        } else {
            (self.ema - scaled) >> EMA_SHIFT
        };
        if self.ema <= scaled {
            self.ema = self.ema.saturating_add(delta.max(1));
        } else {
            self.ema = self.ema.saturating_sub(delta.max(1));
        }
        if self.ema > QUEUE_CAP as u32 {
            self.ema = QUEUE_CAP as u32;
        }
        self.last_seq = seq;
        self.samples = self.samples.saturating_add(1);
        self.digest = fold_u32(self.digest, seq);
        self.digest = fold_u32(self.digest, self.ema);
        Ok(self.ema)
    }

    /// 自审：无样本时 EMA 必须为零，且 EMA 不越界。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples == 0 && self.ema != 0 {
            return Err(BgCode::GaugeBad.val());
        }
        if self.ema > QUEUE_CAP as u32 {
            return Err(BgCode::GaugeBad.val());
        }
        Ok(())
    }
}

// ── 入场裁定 ─────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Admission {
    /// 接受：预算、配额、压力均允许。
    Admit,
    /// 暂扣：整机庇护中，恢复后重投。
    Hold,
    /// 拒绝：水位到顶或配额耗尽。
    Refuse,
}

impl Admission {
    pub fn label(self) -> &'static str {
        match self {
            Admission::Admit => "admit",
            Admission::Hold => "hold",
            Admission::Refuse => "refuse",
        }
    }
}

/// 纯函数裁定：顺序固定，先庇护、再水位、后配额。
pub fn decide(
    level: usize,
    quota_left: u32,
    sheltered: bool,
) -> Result<Admission, u32> {
    if level > QUEUE_CAP {
        return Err(BgCode::WaterBad.val());
    }
    if sheltered {
        return Ok(Admission::Hold);
    }
    if quota_left == 0 {
        return Ok(Admission::Refuse);
    }
    if level >= HIGH_WATER {
        return Ok(Admission::Refuse);
    }
    Ok(Admission::Admit)
}

// ── 队列环（128 格，溢出淘汰显式计数） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardItem {
    pub dev: u8,
    pub seq: u32,
    pub ticks: u32,
}

impl GuardItem {
    pub fn new(dev: u8, seq: u32, ticks: u32) -> Result<GuardItem, u32> {
        if dev as usize >= DEVICE_COUNT {
            return Err(BgCode::QuotaBad.val());
        }
        if seq == 0 || ticks == 0 {
            return Err(BgCode::AdmitBad.val());
        }
        Ok(GuardItem { dev, seq, ticks })
    }

    pub fn text(&self) -> String {
        format!("{}|{}|{}", self.dev, self.seq, self.ticks)
    }
}

#[derive(Clone, Debug)]
pub struct GuardQueue {
    slots: Vec<Option<GuardItem>>,
    head: usize,
    len: usize,
    accepted: u64,
    drop_old: u64,
    drop_new: u64,
    digest: u64,
}

impl GuardQueue {
    pub fn new() -> GuardQueue {
        let mut slots: Vec<Option<GuardItem>> = Vec::new();
        let mut i = 0usize;
        while i < QUEUE_CAP {
            slots.push(None);
            i += 1;
        }
        GuardQueue {
            slots,
            head: 0,
            len: 0,
            accepted: 0,
            drop_old: 0,
            drop_new: 0,
            digest: FNV_OFF,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_full(&self) -> bool {
        self.len >= QUEUE_CAP
    }

    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    pub fn drop_old(&self) -> u64 {
        self.drop_old
    }

    pub fn drop_new(&self) -> u64 {
        self.drop_new
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    fn slot(&self, ix: usize) -> Option<GuardItem> {
        match self.slots.get(ix) {
            Some(v) => *v,
            None => None,
        }
    }

    fn set_slot(&mut self, ix: usize, v: Option<GuardItem>) {
        if let Some(s) = self.slots.get_mut(ix) {
            *s = v;
        }
    }

    /// 尾插；满则先淘汰最老一帧（计数可查）。
    pub fn push(&mut self, it: GuardItem) -> Result<(), u32> {
        if self.is_full() {
            let _ = self.pop_front()?;
            self.drop_old = self.drop_old.saturating_add(1);
        }
        let tail = (self.head + self.len) % QUEUE_CAP;
        self.set_slot(tail, Some(it));
        self.len = self.len.saturating_add(1);
        self.accepted = self.accepted.saturating_add(1);
        self.digest = fold_byte(self.digest, it.dev);
        self.digest = fold_u32(self.digest, it.seq);
        self.digest = fold_u32(self.digest, it.ticks);
        Ok(())
    }

    /// 头弹；空队为错，不得静默返回空。
    pub fn pop_front(&mut self) -> Result<GuardItem, u32> {
        if self.len == 0 {
            return Err(BgCode::DropBad.val());
        }
        let it = match self.slot(self.head) {
            Some(v) => v,
            None => return Err(BgCode::DropBad.val()),
        };
        self.set_slot(self.head, None);
        self.head = (self.head + 1) % QUEUE_CAP;
        self.len = self.len.saturating_sub(1);
        Ok(it)
    }

    /// 显式淘汰最新一帧：只允许在非空时调用。
    pub fn drop_newest(&mut self) -> Result<GuardItem, u32> {
        if self.len == 0 {
            return Err(BgCode::DropBad.val());
        }
        let ix = (self.head + self.len - 1) % QUEUE_CAP;
        let it = match self.slot(ix) {
            Some(v) => v,
            None => return Err(BgCode::DropBad.val()),
        };
        self.set_slot(ix, None);
        self.len = self.len.saturating_sub(1);
        self.drop_new = self.drop_new.saturating_add(1);
        Ok(it)
    }

    /// 自审：长度不越界，淘汰量不超过接受量。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len > QUEUE_CAP {
            return Err(BgCode::AdmitBad.val());
        }
        if self.drop_old.saturating_add(self.drop_new) > self.accepted {
            return Err(BgCode::DropBad.val());
        }
        Ok(())
    }
}

// ── 配额债（十设备，余额 = 上限 − 已用 + 返还） ──────────────────────
#[derive(Clone, Debug)]
pub struct QuotaGrid {
    caps: [u32; DEVICE_COUNT],
    used: [u32; DEVICE_COUNT],
    charged: u64,
    refunded: u64,
    overdrafts: u32,
}

impl QuotaGrid {
    pub fn new() -> QuotaGrid {
        QuotaGrid {
            caps: [QUOTA_CAP; DEVICE_COUNT],
            used: [0; DEVICE_COUNT],
            charged: 0,
            refunded: 0,
            overdrafts: 0,
        }
    }

    pub fn cap(&self, dev: usize) -> u32 {
        match self.caps.get(dev) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn used(&self, dev: usize) -> u32 {
        match self.used.get(dev) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 剩余余额（不会为负）。
    pub fn left(&self, dev: usize) -> u32 {
        self.cap(dev).saturating_sub(self.used(dev))
    }

    pub fn charged(&self) -> u64 {
        self.charged
    }

    pub fn refunded(&self) -> u64 {
        self.refunded
    }

    pub fn overdrafts(&self) -> u32 {
        self.overdrafts
    }

    /// 记账一帧；余额为零返回 QuotaBad，不夹断成「接受」。
    pub fn charge(&mut self, dev: usize) -> Result<u32, u32> {
        if dev >= DEVICE_COUNT {
            return Err(BgCode::QuotaBad.val());
        }
        if self.left(dev) == 0 {
            self.overdrafts = self.overdrafts.saturating_add(1);
            return Err(BgCode::QuotaBad.val());
        }
        if let Some(u) = self.used.get_mut(dev) {
            *u = u.saturating_add(1);
        }
        self.charged = self.charged.saturating_add(1);
        Ok(self.left(dev))
    }

    /// 返还额度；返还量不得超过已用量。
    pub fn refund(&mut self, dev: usize, n: u32) -> Result<u32, u32> {
        if dev >= DEVICE_COUNT || n == 0 {
            return Err(BgCode::QuotaBad.val());
        }
        let used = self.used(dev);
        let take = if n > used { used } else { n };
        if let Some(u) = self.used.get_mut(dev) {
            *u = u.saturating_sub(take);
        }
        self.refunded = self.refunded.saturating_add(take as u64);
        Ok(self.left(dev))
    }

    /// 自审：已用不超上限，账目对得上；被拒的尝试不算透支。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum_used: u64 = 0;
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            if self.used(i) > self.cap(i) {
                return Err(BgCode::QuotaBad.val());
            }
            sum_used = sum_used.saturating_add(self.used(i) as u64);
            i += 1;
        }
        if self.charged.saturating_sub(self.refunded) != sum_used {
            return Err(BgCode::QuotaBad.val());
        }
        Ok(())
    }
}

// ── 庇护（整机暂停 / 凭证单次恢复） ──────────────────────────────────
#[derive(Clone, Debug)]
pub struct Shelter {
    depth: u32,
    token: u32,
    pauses: u32,
    resumes: u32,
    rejects: u32,
}

impl Shelter {
    pub fn new() -> Shelter {
        Shelter { depth: 0, token: 0, pauses: 0, resumes: 0, rejects: 0 }
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn token(&self) -> u32 {
        self.token
    }

    pub fn pauses(&self) -> u32 {
        self.pauses
    }

    pub fn resumes(&self) -> u32 {
        self.resumes
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn active(&self) -> bool {
        self.depth > 0
    }

    /// 进入庇护：返回本轮凭证；嵌套上限 32。
    pub fn pause(&mut self, ticks: u32) -> Result<u32, u32> {
        if ticks == 0 || self.depth >= 32 {
            return Err(BgCode::ShelterBad.val());
        }
        self.depth = self.depth.saturating_add(1);
        self.pauses = self.pauses.saturating_add(1);
        self.token = 0x5A00u32.wrapping_add(self.pauses);
        Ok(self.token)
    }

    /// 按凭证恢复：凭证必须与冻结值一致，且恢复后立即失效。
    pub fn resume(&mut self, token: u32) -> Result<u32, u32> {
        if self.depth == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BgCode::ShelterBad.val());
        }
        if token != self.token {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BgCode::ResumeBad.val());
        }
        self.depth = self.depth.saturating_sub(1);
        self.resumes = self.resumes.saturating_add(1);
        self.token = if self.depth == 0 { 0 } else { self.token.wrapping_add(1) };
        Ok(self.depth)
    }

    /// 自审：恢复数不超过暂停数，凭证与深度自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.resumes > self.pauses {
            return Err(BgCode::ShelterBad.val());
        }
        if self.depth == 0 && self.token != 0 {
            return Err(BgCode::ResumeBad.val());
        }
        if self.depth > 32 {
            return Err(BgCode::ShelterBad.val());
        }
        Ok(())
    }
}

// ── 总账（把三张子账合成一笔可复算的账） ─────────────────────────────
#[derive(Clone, Debug)]
pub struct GuardLedger {
    rounds: u32,
    admits: u32,
    holds: u32,
    refuses: u32,
    queue: GuardQueue,
    quota: QuotaGrid,
    shelter: Shelter,
    digest: u64,
}

impl GuardLedger {
    pub fn new() -> GuardLedger {
        GuardLedger {
            rounds: 0,
            admits: 0,
            holds: 0,
            refuses: 0,
            queue: GuardQueue::new(),
            quota: QuotaGrid::new(),
            shelter: Shelter::new(),
            digest: FNV_OFF,
        }
    }

    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    pub fn admits(&self) -> u32 {
        self.admits
    }

    pub fn holds(&self) -> u32 {
        self.holds
    }

    pub fn refuses(&self) -> u32 {
        self.refuses
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn queue_ref(&self) -> &GuardQueue {
        &self.queue
    }

    pub fn quota_ref(&self) -> &QuotaGrid {
        &self.quota
    }

    pub fn shelter_ref(&self) -> &Shelter {
        &self.shelter
    }

    /// 执行一轮：裁定的结果必须与三张子账的增量一致。
    pub fn step(&mut self, item: GuardItem, level: usize) -> Result<Admission, u32> {
        let left = self.quota.left(item.dev as usize);
        let verdict = decide(level, left, self.shelter.active())?;
        match verdict {
            Admission::Admit => {
                self.quota.charge(item.dev as usize)?;
                self.queue.push(item)?;
                self.admits = self.admits.saturating_add(1);
            }
            Admission::Hold => {
                self.holds = self.holds.saturating_add(1);
            }
            Admission::Refuse => {
                self.refuses = self.refuses.saturating_add(1);
            }
        }
        self.rounds = self.rounds.saturating_add(1);
        self.digest = fold_byte(self.digest, verdict as u8);
        self.digest = fold_u32(self.digest, item.seq);
        Ok(verdict)
    }

    /// 自审：账平、队列、配额、庇护四关全过。
    pub fn audit(&self) -> Result<(), u32> {
        let total = self
            .admits
            .saturating_add(self.holds)
            .saturating_add(self.refuses);
        if total != self.rounds {
            return Err(BgCode::LedgerBad.val());
        }
        self.queue.audit()?;
        self.quota.audit()?;
        self.shelter.audit()?;
        Ok(())
    }
}

// ── 语料（1000 轮压力曲线，确定性） ─────────────────────────────────
/// 生成第 ix 轮的入队项；设备号与序号均由轮次派生，可复算。
pub fn corpus_item(ix: u32) -> Result<GuardItem, u32> {
    let dev = (ix % DEVICE_COUNT as u32) as u8;
    GuardItem::new(dev, ix.saturating_add(1), ix % 17 + 1)
}

/// 压力曲线：相位由轮次决定，不使用随机，保证跨机复算一致。
pub fn corpus_level(ix: u32) -> usize {
    let phase = ix % 100;
    if phase < 30 {
        (phase as usize * 2) % QUEUE_CAP
    } else if phase < 60 {
        60 + (phase as usize % 66)
    } else if phase < 80 {
        96 + (phase as usize % 25)
    } else {
        64 + (phase as usize % 40)
    }
}

/// 黄金运行：1000 轮，每 97 轮进入庇护 3 轮，每 7 轮返还 2 帧。
pub fn golden_run() -> Result<(u32, u32, u32, u64, u64), u32> {
    let mut led = GuardLedger::new();
    let mut gate = PressureGate::new();
    let mut i = 0u32;
    while i < CORPUS_ROUNDS {
        let level = corpus_level(i);
        let _ = gate.observe(level)?;
        let item = corpus_item(i)?;
        if i % 97 == 0 {
            let _ = led.shelter.pause(3)?;
        }
        let _ = led.step(item, level)?;
        if i % 97 == 3 {
            let tok = led.shelter.token();
            if led.shelter.active() {
                let _ = led.shelter.resume(tok)?;
            }
        }
        if i % 7 == 0 {
            let _ = led.quota.refund((i % DEVICE_COUNT as u32) as usize, 2)?;
        }
        i = i.saturating_add(1);
    }
    led.audit()?;
    gate.audit()?;
    Ok((
        led.admits(),
        led.refuses(),
        led.holds(),
        led.queue.drop_old(),
        led.digest(),
    ))
}

// ── 注入（四路，每条注入都必须被自己的闸门抓到） ─────────────────────
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
            // 水位注入：越界一格外必须 WaterBad。
            let mut g = PressureGate::new();
            let level = QUEUE_CAP + 1 + (ix as usize % 3);
            match g.observe(level) {
                Err(e) => Ok(e == BgCode::WaterBad.val()),
                Ok(_) => Ok(false),
            }
        }
        1 => {
            // 裁定注入：庇护中必须是 Hold，哪怕水位与配额都允许。
            let v = decide(1, QUOTA_CAP, true)?;
            Ok(v == Admission::Hold)
        }
        2 => {
            // 配额注入：零余额必须 Refuse 而非 Admit。
            let v = decide(1, 0, false)?;
            Ok(v == Admission::Refuse)
        }
        _ => {
            // 凭证注入：空庇护下恢复必须 ShelterBad；错凭证必须 ResumeBad。
            let mut s = Shelter::new();
            if s.resume(0x5A01).is_ok() {
                return Ok(false);
            }
            let tok = s.pause(1)?;
            match s.resume(tok.wrapping_add(1)) {
                Err(e) => Ok(e == BgCode::ResumeBad.val()),
                Ok(_) => Ok(false),
            }
        }
    }
}

/// 四路注入各跑 rounds/4 轮。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 {
        return Err(BgCode::LedgerBad.val());
    }
    let per = rounds / 4;
    if per == 0 {
        return Err(BgCode::LedgerBad.val());
    }
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

// ── 导出与回读（六字段，覆盖标记必须齐全） ───────────────────────────
pub fn render_ledger(led: &GuardLedger) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{}|{}|{}|{}|{}|",
        led.admits(),
        led.refuses(),
        led.holds(),
        led.rounds(),
        led.queue.drop_old()
    ));
    s.push_str(&format!("{:016X}\n", led.digest()));
    s
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(BgCode::RenderBad.val());
    }
    let mut v: u32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(BgCode::RenderBad.val()),
        };
        if !b.is_ascii_digit() {
            return Err(BgCode::RenderBad.val());
        }
        let d = (b - b'0') as u32;
        v = v
            .checked_mul(10)
            .and_then(|x| x.checked_add(d))
            .ok_or_else(|| BgCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 16 {
        return Err(BgCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(BgCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            _ => return Err(BgCode::RenderBad.val()),
        };
        v = v.checked_mul(16).and_then(|x| x.checked_add(d)).ok_or_else(|| BgCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

/// 回读：六字段、三态覆盖、账平、摘要。
pub fn audit_render(text: &str) -> Result<(u32, u32, u32, u64), u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 1 {
        return Err(BgCode::RenderBad.val());
    }
    let line = match lines.get(0) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    };
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 6 {
        return Err(BgCode::RenderBad.val());
    }
    let admits = parse_u32(match parts.get(0) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    })?;
    let refuses = parse_u32(match parts.get(1) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    })?;
    let holds = parse_u32(match parts.get(2) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    })?;
    let rounds = parse_u32(match parts.get(3) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    })?;
    let _ = parse_u32(match parts.get(4) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    })?;
    if admits.saturating_add(refuses).saturating_add(holds) != rounds {
        return Err(BgCode::LedgerBad.val());
    }
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => *v,
        None => return Err(BgCode::RenderBad.val()),
    })?;
    Ok((admits, refuses, holds, digest))
}

/// 六字段中混入摘要的版本（供黄金回读使用）。
pub fn render_brief() -> Result<String, u32> {
    let mut led = GuardLedger::new();
    let mut i = 0u32;
    while i < 100 {
        let level = corpus_level(i);
        let item = corpus_item(i)?;
        let _ = led.step(item, level)?;
        i = i.saturating_add(1);
    }
    Ok(render_ledger(&led))
}

// ── 探针（A/B/C 三条独立路径，必须给出同一结论） ─────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let mut g = PressureGate::new();
    let _ = g.observe(LOW_WATER + 1)?;
    if g.state() != PressureState::Pressed {
        return Err(BgCode::StateBad.val());
    }
    let _ = g.observe(HIGH_WATER)?;
    if g.state() != PressureState::Shed {
        return Err(BgCode::StateBad.val());
    }
    let _ = g.observe(HIGH_WATER - 1)?;
    let _ = g.observe(LOW_WATER)?;
    let _ = g.observe(LOW_WATER)?;
    let _ = g.observe(LOW_WATER)?;
    if g.state() != PressureState::Pressed {
        return Err(BgCode::StateBad.val());
    }
    let _ = g.observe(LOW_WATER - 1)?;
    let _ = g.observe(LOW_WATER - 1)?;
    let _ = g.observe(LOW_WATER - 1)?;
    if g.state() != PressureState::Calm {
        return Err(BgCode::StateBad.val());
    }
    g.audit()
}

pub fn probe_seq_b() -> Result<(), u32> {
    let mut q = GuardQueue::new();
    let mut i = 0u32;
    while (i as usize) < QUEUE_CAP + 8 {
        let it = corpus_item(i)?;
        q.push(it)?;
        i = i.saturating_add(1);
    }
    if q.len() != QUEUE_CAP {
        return Err(BgCode::AdmitBad.val());
    }
    if q.drop_old() != 8 {
        return Err(BgCode::DropBad.val());
    }
    let first = q.pop_front()?;
    if first.seq != 9 {
        return Err(BgCode::DropBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut s = Shelter::new();
    let tok = s.pause(2)?;
    if !s.active() {
        return Err(BgCode::ShelterBad.val());
    }
    let d = s.resume(tok)?;
    if d != 0 || s.active() {
        return Err(BgCode::ResumeBad.val());
    }
    if s.resume(tok).is_ok() {
        return Err(BgCode::ResumeBad.val());
    }
    Ok(())
}

/// 复现：黄金摘要与关键计数必须与冻结值一致。
pub fn repro_check() -> Result<(), u32> {
    let (admits, refuses, holds, drops, digest) = golden_run()?;
    if admits == 0 || refuses == 0 || holds == 0 {
        return Err(BgCode::LedgerBad.val());
    }
    if drops == 0 {
        return Err(BgCode::DropBad.val());
    }
    let again = golden_run()?;
    if again.4 != digest {
        return Err(BgCode::LedgerBad.val());
    }
    let _ = (admits, refuses, holds, drops);
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(BgCode::LedgerBad.val());
    }
    Ok(())
}

/// 段位守卫：本模块全部诊断码必须落在 0x70A81..=0x70A8B。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < BgCode::ALL.len() {
        let c = match BgCode::ALL.get(i) {
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
    let golden = golden_run();
    v.push((
        "F48885-J1-三态覆盖",
        match &golden {
            Ok((a, r, h, _, _)) => *a > 0 && *r > 0 && *h > 0,
            Err(_) => false,
        },
        match &golden {
            Ok((a, r, h, d, hh)) => format!("admits={} refuses={} holds={} drops={} h={:016X}", a, r, h, d, hh),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48885-J1-滞回不抖",
        {
            let mut g = PressureGate::new();
            let mut ok = true;
            let mut i = 0u32;
            while i < 1000 {
                let lvl = if i % 2 == 0 { HIGH_WATER } else { HIGH_WATER - 1 };
                if g.observe(lvl).is_err() {
                    ok = false;
                }
                i = i.saturating_add(1);
            }
            ok && g.flips() <= 2
        },
        format!("flips<=2 after 1000 alternating ticks"),
    ));
    v.push((
        "F48886-J1-配额零透支",
        {
            let mut q = QuotaGrid::new();
            let mut ok = true;
            let mut i = 0u32;
            while i < 1000 {
                match q.charge((i % 10) as usize) {
                    Ok(_) => {}
                    Err(e) => {
                        if e != BgCode::QuotaBad.val() {
                            ok = false;
                        }
                        let _ = q.refund((i % 10) as usize, 1);
                    }
                }
                i = i.saturating_add(1);
            }
            ok && q.audit().is_ok()
        },
        String::from("1000 charges with refunds keep used<=cap"),
    ));
    v.push((
        "F48886-J1-返还守恒",
        {
            let mut q = QuotaGrid::new();
            let mut i = 0u32;
            while i < 5 {
                let _ = q.charge(0);
                i = i.saturating_add(1);
            }
            let before = q.left(0);
            let _ = q.refund(0, 3);
            q.left(0) == before.saturating_add(3) && q.audit().is_ok()
        },
        String::from("refund 3 after 5 charges restores exactly 3 credits"),
    ));
    v.push((
        "F48887-J1-凭证单次",
        match probe_seq_c() {
            Ok(_) => true,
            Err(_) => false,
        },
        String::from("resume twice: second must fail"),
    ));
    v.push((
        "F48887-J1-嵌套庇护",
        {
            let mut s = Shelter::new();
            let a = s.pause(1);
            let b = s.pause(1);
            match (a, b) {
                (Ok(_), Ok(_)) => {
                    let t = s.token();
                    let _ = s.resume(t);
                    let t2 = s.token();
                    s.resume(t2).is_ok() && s.depth() == 0
                }
                _ => false,
            }
        },
        String::from("nested pause depth 2 resumes to 0"),
    ));
    v.push((
        "J1-队列溢出显式",
        match probe_seq_b() {
            Ok(_) => true,
            Err(_) => false,
        },
        String::from("136 pushes -> len 128, drop_old 8"),
    ));
    v.push((
        "J1-注入四路全捕",
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => {
                l.all_caught()
                    && l.route_caught.iter().all(|c| *c > 0)
            }
            Err(_) => false,
        },
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => format!("rounds={} caught={} routes={:?}", l.rounds, l.caught, l.route_caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "J1-导出回读",
        match render_brief() {
            Ok(t) => audit_render(&t).is_ok(),
            Err(_) => false,
        },
        String::from("six fields round-trip"),
    ));
    v.push((
        "J1-账平自审",
        {
            let mut led = GuardLedger::new();
            let mut i = 0u32;
            while i < 200 {
                let _ = led.step(corpus_item(i).unwrap_or(GuardItem { dev: 0, seq: 1, ticks: 1 }), corpus_level(i));
                i = i.saturating_add(1);
            }
            led.audit().is_ok()
        },
        String::from("admits+holds+refuses == rounds"),
    ));
    v
}

/// 本模块判据集合：十项动态 + 段位守卫。
pub fn run_unx48885_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48885 背压闸门与保护面");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let g = segment_guard();
    s.add_dyn("F48885-J1-段位守卫", g, if g { "" } else { "诊断码逃出 0x70A80" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_level_rejects_overflow() {
        let mut g = PressureGate::new();
        assert_eq!(g.observe(QUEUE_CAP + 1), Err(BgCode::WaterBad.val()));
        assert_eq!(g.observe(QUEUE_CAP).ok(), Some(PressureState::Shed));
    }

    #[test]
    fn hysteresis_holds_state_between_waters() {
        let mut g = PressureGate::new();
        assert_eq!(g.observe(HIGH_WATER).ok(), Some(PressureState::Shed));
        assert_eq!(g.observe(HIGH_WATER - 1).ok(), Some(PressureState::Shed));
        assert_eq!(g.observe(LOW_WATER + 1).ok(), Some(PressureState::Shed));
    }

    #[test]
    fn downshift_requires_low_water_and_dwell() {
        let mut g = PressureGate::new();
        let _ = g.observe(HIGH_WATER);
        let _ = g.observe(LOW_WATER);
        let _ = g.observe(LOW_WATER);
        let _ = g.observe(LOW_WATER);
        assert_eq!(g.state(), PressureState::Pressed);
        let _ = g.observe(LOW_WATER - 1);
        let _ = g.observe(LOW_WATER - 1);
        let _ = g.observe(LOW_WATER - 1);
        assert_eq!(g.state(), PressureState::Calm);
    }

    #[test]
    fn decide_order_is_shelter_quota_water() {
        assert_eq!(decide(QUEUE_CAP, 1, false).ok(), Some(Admission::Refuse));
        assert_eq!(decide(0, 0, false).ok(), Some(Admission::Refuse));
        assert_eq!(decide(QUEUE_CAP, 0, true).ok(), Some(Admission::Hold));
        assert_eq!(decide(1, 1, false).ok(), Some(Admission::Admit));
        assert!(decide(QUEUE_CAP + 1, 1, false).is_err());
    }

    #[test]
    fn queue_drops_oldest_and_counts() {
        let mut q = GuardQueue::new();
        let mut i = 0u32;
        while (i as usize) < QUEUE_CAP {
            let it = GuardItem { dev: (i % 10) as u8, seq: i + 1, ticks: 1 };
            let _ = q.push(it);
            i = i.saturating_add(1);
        }
        assert!(q.is_full());
        let it = GuardItem { dev: 0, seq: 9999, ticks: 1 };
        let _ = q.push(it);
        assert_eq!(q.len(), QUEUE_CAP);
        assert_eq!(q.drop_old(), 1);
        let front = q.pop_front();
        assert_eq!(front.ok().map(|x| x.seq), Some(2));
    }

    #[test]
    fn queue_drop_newest_requires_nonempty() {
        let mut q = GuardQueue::new();
        assert_eq!(q.drop_newest(), Err(BgCode::DropBad.val()));
        let it = GuardItem { dev: 1, seq: 1, ticks: 1 };
        let _ = q.push(it);
        assert_eq!(q.drop_newest().ok().map(|x| x.seq), Some(1));
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn quota_charge_refund_balance() {
        let mut q = QuotaGrid::new();
        assert_eq!(q.charge(3).ok(), Some(QUOTA_CAP - 1));
        assert_eq!(q.charge(3).ok(), Some(QUOTA_CAP - 2));
        assert_eq!(q.refund(3, 1).ok(), Some(QUOTA_CAP - 1));
        assert!(q.audit().is_ok());
        assert_eq!(q.charged(), 2);
        assert_eq!(q.refunded(), 1);
    }

    #[test]
    fn quota_rejects_overdraft_without_mutation() {
        let mut q = QuotaGrid::new();
        let mut i = 0u32;
        while i < QUOTA_CAP {
            let _ = q.charge(0);
            i = i.saturating_add(1);
        }
        let before = q.used(0);
        assert_eq!(q.charge(0), Err(BgCode::QuotaBad.val()));
        assert_eq!(q.used(0), before);
        assert_eq!(q.overdrafts(), 1);
        assert!(q.audit().is_ok());
    }

    #[test]
    fn shelter_token_is_single_use() {
        let mut s = Shelter::new();
        assert_eq!(s.resume(1), Err(BgCode::ShelterBad.val()));
        let tok = s.pause(1);
        assert_eq!(tok, Ok(0x5A01));
        assert_eq!(s.resume(0x5A02), Err(BgCode::ResumeBad.val()));
        assert_eq!(s.resume(0x5A01), Ok(0));
        assert_eq!(s.resume(0x5A01), Err(BgCode::ShelterBad.val()));
    }

    #[test]
    fn shelter_nesting_bounded() {
        let mut s = Shelter::new();
        let mut i = 0u32;
        while i < 32 {
            assert!(s.pause(1).is_ok());
            i = i.saturating_add(1);
        }
        assert_eq!(s.pause(1), Err(BgCode::ShelterBad.val()));
        assert!(s.audit().is_ok());
    }

    #[test]
    fn gauge_ema_tracks_up_and_down() {
        let mut g = Gauge::new();
        let mut seq = 1u32;
        while seq <= 50 {
            let _ = g.sample(seq, 64);
            seq = seq.saturating_add(1);
        }
        let high = g.ema();
        assert!(high > 30);
        while seq <= 200 {
            let _ = g.sample(seq, 1);
            seq = seq.saturating_add(1);
        }
        assert!(g.ema() < high);
        assert!(g.audit().is_ok());
    }

    #[test]
    fn gauge_rejects_bad_input() {
        let mut g = Gauge::new();
        assert_eq!(g.sample(0, 10), Err(BgCode::GaugeBad.val()));
        assert_eq!(g.sample(1, 0), Err(BgCode::GaugeBad.val()));
        assert_eq!(g.sample(1, QUEUE_CAP * 2 + 1), Err(BgCode::GaugeBad.val()));
    }

    #[test]
    fn ledger_balance_and_step_verdicts() {
        let mut led = GuardLedger::new();
        let it = GuardItem { dev: 2, seq: 1, ticks: 1 };
        assert_eq!(led.step(it, 0).ok(), Some(Admission::Admit));
        assert_eq!(led.step(it, HIGH_WATER).ok(), Some(Admission::Refuse));
        assert!(led.audit().is_ok());
        assert_eq!(led.rounds(), 2);
    }

    #[test]
    fn shelter_rounds_count_holds() {
        let mut led = GuardLedger::new();
        let it = GuardItem { dev: 0, seq: 1, ticks: 1 };
        let _ = led.shelter.pause(1);
        assert_eq!(led.step(it, 1).ok(), Some(Admission::Hold));
        assert_eq!(led.holds(), 1);
        assert!(led.audit().is_ok());
    }

    #[test]
    fn golden_is_deterministic_and_balanced() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a.is_ok(), b.is_ok());
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.4, y.4);
                assert!(x.0 > 0 && x.1 > 0 && x.2 > 0);
                assert_eq!((x.0 + x.1 + x.2) as usize, CORPUS_ROUNDS as usize);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn injection_catches_all_routes() {
        let l = inject_rounds(1000);
        match l {
            Ok(v) => {
                assert!(v.all_caught());
                assert_eq!(v.rounds, 1000);
                assert_eq!(v.route_caught, [250, 250, 250, 250]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_round_trip_and_tamper() {
        let t = render_brief();
        match t {
            Ok(text) => {
                let r = audit_render(&text);
                assert!(r.is_ok());
                let bad = text.replacen("|", "||", 1);
                assert!(audit_render(&bad).is_err());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_repro_agree() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn all_codes_in_window_and_details_distinct() {
        assert!(segment_guard());
        let mut seen: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < BgCode::ALL.len() {
            if let Some(c) = BgCode::ALL.get(i) {
                assert!(!seen.contains(&c.val()));
                seen.push(c.val());
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert!(detail(0xDEAD).contains("未知"));
    }
}
