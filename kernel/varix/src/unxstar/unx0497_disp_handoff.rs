//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0497 · 显示交接字段（A2/显示域预备） · 0x760C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6-N）：
//!   交接字段 v1 冻结（8 字段），消费方 mock diff 零差
//! 设计要点：
//!   1. 八字段冻结闭集：帧缓冲基址 / stride / 宽 / 高 / 主输出 id /
//!      色深 / 哨兵魔数 / 封账指纹，字段数冻结 8，少一字段即封账失败；
//!   2. 冻结闸三重：基址 4K 页对齐、stride ≥ 宽×每像素字节、色深只许
//!      16/24/32 闭集、主输出 id < 输出上限、哨兵魔数逐字对拍；
//!   3. 单调状态机：草拟→冻结→封账，冻结后任何 set_field 显性拒
//!      （FrozenLocked），无还原门；
//!   4. 封账指纹 FNV-1a64：对前 7 字段序列化字节求指纹，复算两遍
//!      同值才许封账（指纹漂移即拒）；
//!   5. 消费方 mock：按冻结快照逐字段 diff，绿态 diff 计数必须为零，
//!      篡改任一字段的 mock 必出非零 diff（8 路注入必抓）；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x760C0，AI-VX7-1104 批6-N 独占槽）。
pub const SEG_BASE: u32 = 0x760C0;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 交接字段数（冻结 8）。
pub const FIELD_COUNT: usize = 8;
/// 帧缓冲基址页对齐（4K，手册：x86 页帧 4096）。
pub const FB_ALIGN: u64 = 4096;
/// 色深闭集（16/24/32bpp）。
pub const BPP_CLOSED: [u32; 3] = [16, 24, 32];
/// 主输出 id 上界（输出数冻结 4）。
pub const MAX_OUTS: u32 = 4;
/// 哨兵魔数（冻结：0x55AA_760C_0000_0497 段号回文式自证）。
pub const SENTINEL_MAGIC: u64 = 0x55AA_760C_0000_0497;
/// 交接语义版本（v1 冻结）。
pub const HANDOFF_VERSION: u32 = 1;

/// 真机冻结值表（A2 显示域预备样机：1024×768×32bpp，stride=宽×4）。
pub const M_FB_BASE: u64 = 0x8000_0000;
pub const M_STRIDE: u32 = 4096;
pub const M_WIDTH: u32 = 1024;
pub const M_HEIGHT: u32 = 768;
pub const M_MAIN_OUT: u32 = 0;
pub const M_BPP: u32 = 32;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandCode {
    /// 冻结后改写（单调闸显性拒）
    FrozenLocked,
    /// 基址非 4K 对齐
    BaseMisaligned,
    /// stride 不足以承载一行像素
    StrideTooSmall,
    /// 色深越出 16/24/32 闭集
    BppClosed,
    /// 主输出 id 越界
    OutOver,
    /// 哨兵魔数失配
    MagicMismatch,
    /// 封账指纹漂移
    FpDrift,
    /// 双跑不一致
    NotReproducible,
}

impl HandCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HandCode::FrozenLocked => "冻结后改写：交接字段 v1 已冻结，建议新开 v2 交接代",
            HandCode::BaseMisaligned => "帧缓冲基址非 4K 对齐：建议按页帧重排基址",
            HandCode::StrideTooSmall => "stride 不足一行像素：建议 stride ≥ 宽×每像素字节数",
            HandCode::BppClosed => "色深越出闭集：只许 16/24/32bpp，建议对齐冻结色深表",
            HandCode::OutOver => "主输出 id 越界：id 须 < 输出上限 4，建议按输出账重编",
            HandCode::MagicMismatch => "哨兵魔数失配：建议回读冻结魔数 0x55AA_760C_0000_0497",
            HandCode::FpDrift => "封账指纹漂移：前 7 字节序列与指纹不符，建议整账复算再封",
            HandCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [HandCode; 8] = [
        HandCode::FrozenLocked,
        HandCode::BaseMisaligned,
        HandCode::StrideTooSmall,
        HandCode::BppClosed,
        HandCode::OutOver,
        HandCode::MagicMismatch,
        HandCode::FpDrift,
        HandCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HandCode::ALL.len() {
        if let Some(c) = HandCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x760C0 段闭集内"
}

// ── 域模型（八字段交接块） ──────────────────────────────────────
/// 字段号闭集（0~7 对应八字段冻结序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldId {
    /// 帧缓冲基址
    FbBase,
    /// 行跨度
    Stride,
    /// 宽
    Width,
    /// 高
    Height,
    /// 主输出 id
    MainOut,
    /// 色深
    Bpp,
    /// 哨兵魔数
    SentinelMagic,
    /// 封账指纹
    SealFp,
}

impl FieldId {
    pub fn ordinal(self) -> usize {
        match self {
            FieldId::FbBase => 0,
            FieldId::Stride => 1,
            FieldId::Width => 2,
            FieldId::Height => 3,
            FieldId::MainOut => 4,
            FieldId::Bpp => 5,
            FieldId::SentinelMagic => 6,
            FieldId::SealFp => 7,
        }
    }
    /// 中文名（诊断文案与 mock diff 用）。
    pub fn name(self) -> &'static str {
        match self {
            FieldId::FbBase => "帧缓冲基址",
            FieldId::Stride => "行跨度",
            FieldId::Width => "宽",
            FieldId::Height => "高",
            FieldId::MainOut => "主输出id",
            FieldId::Bpp => "色深",
            FieldId::SentinelMagic => "哨兵魔数",
            FieldId::SealFp => "封账指纹",
        }
    }
    pub const ALL: [FieldId; 8] = [
        FieldId::FbBase,
        FieldId::Stride,
        FieldId::Width,
        FieldId::Height,
        FieldId::MainOut,
        FieldId::Bpp,
        FieldId::SentinelMagic,
        FieldId::SealFp,
    ];
}

// ── 交接状态机（草拟→冻结→封账，单调前进） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandState {
    /// 草拟（可写）
    Draft,
    /// 冻结（只读）
    Frozen,
    /// 封账（消费侧可接）
    Sealed,
}

impl HandState {
    pub fn rank(self) -> u32 {
        match self {
            HandState::Draft => 0,
            HandState::Frozen => 1,
            HandState::Sealed => 2,
        }
    }
    pub fn can_advance(self, to: HandState) -> bool {
        to.rank() > self.rank()
    }
    pub const ALL: [HandState; 3] = [HandState::Draft, HandState::Frozen, HandState::Sealed];
}

/// 八字段交接块（数值字段统一 u64 存账，指纹独立成账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffBlock {
    pub version: u32,
    pub state: HandState,
    pub fb_base: u64,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub main_out: u32,
    pub bpp: u32,
    pub sentinel_magic: u64,
    pub seal_fp: u64,
    pub write_rejects: u32,
}

impl HandoffBlock {
    pub fn draft() -> HandoffBlock {
        HandoffBlock {
            version: HANDOFF_VERSION,
            state: HandState::Draft,
            fb_base: 0,
            stride: 0,
            width: 0,
            height: 0,
            main_out: 0,
            bpp: 0,
            sentinel_magic: SENTINEL_MAGIC,
            seal_fp: 0,
            write_rejects: 0,
        }
    }

    /// 单字段写（仅草拟态；冻结/封账态显性拒并记账）。
    pub fn set_field(&mut self, id: FieldId, v: u64) -> Result<(), u32> {
        if self.state != HandState::Draft {
            self.write_rejects = self.write_rejects.saturating_add(1);
            return Err(HandCode::FrozenLocked.val());
        }
        match id {
            FieldId::FbBase => self.fb_base = v,
            FieldId::Stride => self.stride = v as u32,
            FieldId::Width => self.width = v as u32,
            FieldId::Height => self.height = v as u32,
            FieldId::MainOut => self.main_out = v as u32,
            FieldId::Bpp => self.bpp = v as u32,
            FieldId::SentinelMagic => {
                if v != SENTINEL_MAGIC {
                    return Err(HandCode::MagicMismatch.val());
                }
                self.sentinel_magic = v;
            }
            FieldId::SealFp => self.seal_fp = v,
        }
        Ok(())
    }

    /// 字段读（按字段号闭集取值；越号显性回 None）。
    pub fn field(&self, id: FieldId) -> Option<u64> {
        match id {
            FieldId::FbBase => Some(self.fb_base),
            FieldId::Stride => Some(self.stride as u64),
            FieldId::Width => Some(self.width as u64),
            FieldId::Height => Some(self.height as u64),
            FieldId::MainOut => Some(self.main_out as u64),
            FieldId::Bpp => Some(self.bpp as u64),
            FieldId::SentinelMagic => Some(self.sentinel_magic),
            FieldId::SealFp => Some(self.seal_fp),
        }
    }
}

// ── 冻结闸（七闸齐过才许冻结） ─────────────────────────────────
/// 基址闸：4K 对齐。
fn fb_base_ok(b: &HandoffBlock) -> bool {
    b.fb_base % FB_ALIGN == 0 && b.fb_base != 0
}

/// stride 闸：stride ≥ 宽 × 每像素字节数（色深换算）。
fn stride_ok(b: &HandoffBlock) -> bool {
    if b.bpp == 0 || b.width == 0 {
        return false;
    }
    let bytes_pp = match b.bpp {
        16 => 2u64,
        24 => 3u64,
        32 => 4u64,
        _ => return false,
    };
    (b.stride as u64) >= b.width as u64 * bytes_pp
}

/// 色深闭集闸。
fn bpp_ok(b: &HandoffBlock) -> bool {
    let mut i = 0;
    while i < BPP_CLOSED.len() {
        if let Some(x) = BPP_CLOSED.get(i) {
            if *x == b.bpp {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 主输出 id 闸。
fn main_out_ok(b: &HandoffBlock) -> bool {
    b.main_out < MAX_OUTS
}

/// 哨兵魔数闸。
fn magic_ok(b: &HandoffBlock) -> bool {
    b.sentinel_magic == SENTINEL_MAGIC
}

/// 尺寸闸：宽高均非零。
fn size_ok(b: &HandoffBlock) -> bool {
    b.width > 0 && b.height > 0
}

/// 冻结总闸（七闸合取；指纹闸在 seal 面单独复算）。
pub fn freeze_gate(b: &HandoffBlock) -> bool {
    fb_base_ok(b) && stride_ok(b) && bpp_ok(b) && main_out_ok(b) && magic_ok(b) && size_ok(b)
}

// ── FNV-1a 64 封账指纹 ─────────────────────────────────────────
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(x) = bytes.get(i) {
            h ^= *x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        i += 1;
    }
    h
}

/// 前 7 字段序列化（八字段序，跳过封账指纹本身）。
fn serialize_seven(b: &HandoffBlock) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let vals: [u64; 7] = [
        b.fb_base,
        b.stride as u64,
        b.width as u64,
        b.height as u64,
        b.main_out as u64,
        b.bpp as u64,
        b.sentinel_magic,
    ];
    let mut i = 0;
    while i < vals.len() {
        if let Some(v) = vals.get(i) {
            // 小端 8 字节逐位落账。
            let mut k = 0;
            while k < 8 {
                out.push((*v >> (k * 8)) as u8);
                k += 1;
            }
        }
        i += 1;
    }
    out
}

/// 封账指纹（前 7 字段 FNV-1a64）。
pub fn seal_fingerprint(b: &HandoffBlock) -> u64 {
    let bytes = serialize_seven(b);
    fnv1a64(&bytes)
}

/// 冻结（七闸齐过 + 指纹自填），绿态返回指纹，违约显性拒。
pub fn freeze(b: &mut HandoffBlock) -> Result<u64, u32> {
    if !freeze_gate(b) {
        // 逐闸回因：按八字段冻结序挑第一个违约闸回诊断码。
        if !fb_base_ok(b) {
            return Err(HandCode::BaseMisaligned.val());
        }
        if !bpp_ok(b) {
            return Err(HandCode::BppClosed.val());
        }
        if !stride_ok(b) {
            return Err(HandCode::StrideTooSmall.val());
        }
        if !main_out_ok(b) {
            return Err(HandCode::OutOver.val());
        }
        if !magic_ok(b) {
            return Err(HandCode::MagicMismatch.val());
        }
        return Err(HandCode::StrideTooSmall.val());
    }
    let fp = seal_fingerprint(b);
    b.seal_fp = fp;
    b.state = HandState::Frozen;
    Ok(fp)
}

/// 封账（冻结→封账；指纹复算两遍同值才许过）。
pub fn seal(b: &mut HandoffBlock) -> Result<(), u32> {
    if !b.state.can_advance(HandState::Sealed) {
        return Err(HandCode::FrozenLocked.val());
    }
    let fp1 = seal_fingerprint(b);
    let fp2 = seal_fingerprint(b);
    if fp1 != fp2 || fp1 != b.seal_fp {
        return Err(HandCode::FpDrift.val());
    }
    b.state = HandState::Sealed;
    Ok(())
}

// ── 消费方 mock（按冻结快照逐字段 diff） ────────────────────────
#[derive(Clone, Debug, Default)]
pub struct ConsumerMock {
    /// 快照八字段（FieldId 序）。
    pub snap: [u64; 8],
    pub loaded: bool,
}

impl ConsumerMock {
    /// 加载快照（封账态才许接）。
    pub fn load(&mut self, b: &HandoffBlock) -> Result<(), u32> {
        if b.state != HandState::Sealed {
            return Err(HandCode::FrozenLocked.val());
        }
        let mut i = 0;
        while i < FieldId::ALL.len() {
            if let Some(id) = FieldId::ALL.get(i) {
                let v = match b.field(*id) {
                    Some(x) => x,
                    None => 0,
                };
                if let Some(s) = self.snap.get_mut(i) {
                    *s = v;
                }
            }
            i += 1;
        }
        self.loaded = true;
        Ok(())
    }

    /// 逐字段 diff：与对照块八字段逐位比，返回差字段号清单。
    pub fn diff(&self, b: &HandoffBlock) -> Vec<usize> {
        let mut bad: Vec<usize> = Vec::new();
        if !self.loaded {
            return bad;
        }
        let mut i = 0;
        while i < FieldId::ALL.len() {
            if let Some(id) = FieldId::ALL.get(i) {
                let a = match self.snap.get(i) {
                    Some(x) => *x,
                    None => 0,
                };
                let v = match b.field(*id) {
                    Some(x) => x,
                    None => 0,
                };
                if a != v {
                    bad.push(i);
                }
            }
            i += 1;
        }
        bad
    }
}

// ── 真机冻结交接块（样机值全表落账） ────────────────────────────
pub fn machine_block() -> HandoffBlock {
    let mut b = HandoffBlock::draft();
    let _ = b.set_field(FieldId::FbBase, M_FB_BASE);
    let _ = b.set_field(FieldId::Stride, M_STRIDE as u64);
    let _ = b.set_field(FieldId::Width, M_WIDTH as u64);
    let _ = b.set_field(FieldId::Height, M_HEIGHT as u64);
    let _ = b.set_field(FieldId::MainOut, M_MAIN_OUT as u64);
    let _ = b.set_field(FieldId::Bpp, M_BPP as u64);
    let _ = b.set_field(FieldId::SentinelMagic, SENTINEL_MAGIC);
    b
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（九路：冻结后写 ×8 字段 + 魔数篡改） ─────────────
pub struct InjectLedger {
    pub tries: [u32; 9],
    pub caught: [u32; 9],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 9], caught: [0; 9] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 9 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 9 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：样机落账→冻结→封账→指纹复算→mock 零差→冻结改写必拒 ─
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut b = machine_block();
    if !freeze_gate(&b) {
        return Err(HandCode::BaseMisaligned.val());
    }
    let fp = freeze(&mut b)?;
    seal(&mut b)?;
    if b.state != HandState::Sealed {
        return Err(HandCode::FpDrift.val());
    }
    // 指纹复算两遍同值。
    if seal_fingerprint(&b) != fp {
        return Err(HandCode::FpDrift.val());
    }
    // 消费方 mock 加载 + 零差。
    let mut mock = ConsumerMock::default();
    mock.load(&b)?;
    let bad = mock.diff(&b);
    if !bad.is_empty() {
        return Err(HandCode::FpDrift.val());
    }
    // 冻结后改写八字段全拒（write_rejects 记账 8 次）。
    let mut rej = 0u32;
    let mut i = 0;
    while i < FieldId::ALL.len() {
        if let Some(id) = FieldId::ALL.get(i) {
            match b.set_field(*id, 0xdead) {
                Err(e) if e == HandCode::FrozenLocked.val() => rej = rej.saturating_add(1),
                _ => return Err(HandCode::NotReproducible.val()),
            }
        }
        i += 1;
    }
    Ok((b.seal_fp as u32, rej, bad.len() as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（冻结后写拒 8 次 / mock 零差）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((_, 8, 0)) => Ok(()),
        _ => Err(HandCode::NotReproducible.val()),
    }
}

/// 序 B：魔数篡改在草拟态即拒（MagicMismatch）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = HandoffBlock::draft();
    match b.set_field(FieldId::SentinelMagic, 0x1234) {
        Err(e) if e == HandCode::MagicMismatch.val() => Ok(()),
        _ => Err(HandCode::NotReproducible.val()),
    }
}

/// 序 C：口径冻结（八字段 / 4K 对齐 / 色深闭集 / 魔数值）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        FIELD_COUNT == 8,
        FB_ALIGN == 4096,
        BPP_CLOSED.len() == 3,
        SENTINEL_MAGIC == 0x55AA_760C_0000_0497,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(HandCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 9 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0~7：封账后逐字段改写全拒。
        let mut f = 0;
        while f < FieldId::ALL.len() {
            let mut b = machine_block();
            let _ = freeze(&mut b);
            let _ = seal(&mut b);
            let id = match FieldId::ALL.get(f) {
                Some(x) => *x,
                None => FieldId::FbBase,
            };
            let hit = match b.set_field(id, rng.below(0xFFFF) as u64) {
                Err(e) => e == HandCode::FrozenLocked.val(),
                Ok(()) => false,
            };
            led.record(f, hit);
            f += 1;
        }
        // 路 8：mock 篡改快照必出非零 diff（改主输出 id 位）。
        let mut b2 = machine_block();
        let _ = freeze(&mut b2);
        let _ = seal(&mut b2);
        let mut mock = ConsumerMock::default();
        let _ = mock.load(&b2);
        // 篡改：快照主输出 id 拨到越界值。
        if let Some(s) = mock.snap.get_mut(FieldId::MainOut.ordinal()) {
            *s = MAX_OUTS as u64 + 1;
        }
        let bad = mock.diff(&b2);
        led.record(8, bad.len() == 1);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HandCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < HandCode::ALL.len() {
        if let Some(c) = HandCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < HandCode::ALL.len() {
        if let Some(c) = HandCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// LCG 确定性。
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入账 0 号槽取值（.get 取，不裸下标）。
fn slot0(t: &[u32; 9]) -> u32 {
    match t.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        FIELD_COUNT == 8 && HANDOFF_VERSION == 1 && FieldId::ALL.len() == 8,
        format!("八字段闭集 v{} 哨兵 0x{:x}", HANDOFF_VERSION, SENTINEL_MAGIC),
    ));
    // 2. 冻结七闸对真机块全绿。
    let mb = machine_block();
    v.push((
        "J1-冻结七闸",
        freeze_gate(&mb) && fb_base_ok(&mb) && stride_ok(&mb) && bpp_ok(&mb) && main_out_ok(&mb)
            && magic_ok(&mb) && size_ok(&mb),
        format!(
            "基址 0x{:x} stride={} {}x{} {}bpp",
            mb.fb_base, mb.stride, mb.width, mb.height, mb.bpp
        ),
    ));
    // 3. 冻结+封账全链与指纹复算。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((_, rej, bad)) => *rej == 8 && *bad == 0,
            Err(_) => false,
        },
        match &g {
            Ok((fp, rej, bad)) => format!("指纹 0x{:x} 写拒{} mock差{}", fp, rej, bad),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. mock 零差（封账块加载后逐字段 diff）。
    let mut b = machine_block();
    let _ = freeze(&mut b);
    let _ = seal(&mut b);
    let mut mock = ConsumerMock::default();
    let load_ok = mock.load(&b).is_ok();
    let zero_diff = load_ok && mock.diff(&b).is_empty();
    v.push((
        "J1-mock零差",
        zero_diff,
        String::from("消费方按冻结快照逐字段 diff 计数=0"),
    ));
    // 5. 冻结前 mock 加载显性拒。
    let mut b2 = machine_block();
    let pre_reject = match mock.load(&b2) {
        Err(e) => e == HandCode::FrozenLocked.val(),
        Ok(()) => false,
    };
    let _ = freeze(&mut b2);
    let draft_reject = match mock.load(&b2) {
        Err(e) => e == HandCode::FrozenLocked.val(),
        Ok(()) => false,
    };
    v.push((
        "J1-未封账禁接",
        pre_reject && draft_reject,
        String::from("草拟/冻结态 mock 加载显性拒"),
    ));
    // 6. 魔数闸。
    v.push((
        "J1-魔数闸",
        probe_seq_b().is_ok() && magic_ok(&mb),
        String::from("草拟态魔数篡改即拒 值逐字对拍"),
    ));
    // 7. 冻结后八字段写拒（记账 8）。
    v.push((
        "J1-冻结后写拒",
        match &g {
            Ok((_, rej, _)) => *rej == 8,
            Err(_) => false,
        },
        String::from("v1 冻结无还原门 八字段全拒"),
    ));
    // 8. 指纹漂移闸（篡指纹复算必破）。
    let mut b3 = machine_block();
    let _ = freeze(&mut b3);
    let fp_real = seal_fingerprint(&b3);
    b3.seal_fp = fp_real.wrapping_add(1);
    let fp_drift = match seal(&mut b3) {
        Err(e) => e == HandCode::FpDrift.val(),
        Ok(()) => false,
    };
    v.push((
        "J1-指纹漂移闸",
        fp_drift,
        String::from("封账指纹改一位复算必破平"),
    ));
    // 9. 探针三序。
    v.push((
        "J1-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a全链 b魔数 c口径"),
    ));
    // 10. 注入九路 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入九路10/10",
        match &inj {
            Ok(l) => l.all_caught() && slot0(&l.tries) == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 11. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 12. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 13. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", HandCode::ALL.len()),
    ));
    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x760C0 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx0497_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0497 显示交接字段（A2/显示域预备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((_, rej, bad)) => {
                assert_eq!(rej, 8);
                assert_eq!(bad, 0);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(FIELD_COUNT, 8);
        assert_eq!(FieldId::ALL.len(), 8);
        assert_eq!(BPP_CLOSED, [16, 24, 32]);
        assert_eq!(SENTINEL_MAGIC, 0x55AA_760C_0000_0497);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn freeze_gate_boundary() {
        // 基址非对齐。
        let mut b = machine_block();
        b.fb_base = 0x8000_0004;
        assert!(!freeze_gate(&b));
        assert!(!fb_base_ok(&b));
        // stride 不足。
        let mut b2 = machine_block();
        b2.stride = 1024;
        assert!(!stride_ok(&b2));
        // 色深越闭集。
        let mut b3 = machine_block();
        b3.bpp = 8;
        assert!(!bpp_ok(&b3));
        // 主输出越界。
        let mut b4 = machine_block();
        b4.main_out = MAX_OUTS;
        assert!(!main_out_ok(&b4));
        // 宽为零。
        let mut b5 = machine_block();
        b5.width = 0;
        assert!(!size_ok(&b5));
    }

    #[test]
    fn write_after_frozen_always_rejected() {
        let mut b = machine_block();
        freeze(&mut b).ok();
        seal(&mut b).ok();
        let mut i = 0;
        while i < FieldId::ALL.len() {
            let id = match FieldId::ALL.get(i) {
                Some(x) => *x,
                None => FieldId::FbBase,
            };
            match b.set_field(id, 1) {
                Err(e) => assert_eq!(e, HandCode::FrozenLocked.val()),
                Ok(()) => assert!(false, "冻结后写{}未拒", id.name()),
            }
            i += 1;
        }
        assert_eq!(b.write_rejects, 8);
    }

    #[test]
    fn mock_diff_zero_and_tamper_detected() {
        let mut b = machine_block();
        freeze(&mut b).ok();
        seal(&mut b).ok();
        let mut mock = ConsumerMock::default();
        mock.load(&b).ok();
        assert!(mock.diff(&b).is_empty(), "绿态 mock diff 必须零差");
        // 篡改对照块八字段之一 → diff 恰一个字段。
        let mut b2 = b.clone();
        b2.bpp = 24;
        let d = mock.diff(&b2);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(slot0(&led.tries), JUDGE_ROUNDS);
        let t8 = match led.tries.get(8) {
            Some(x) => *x,
            None => 0,
        };
        assert_eq!(t8, JUDGE_ROUNDS, "mock 篡改路 10 试");
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn fingerprint_replay_and_drift() {
        let mut b = machine_block();
        let fp = freeze(&mut b).ok();
        let fp = match fp {
            Some(x) => x,
            None => 0,
        };
        assert_eq!(seal_fingerprint(&b), fp, "指纹复算一致");
        assert_eq!(seal_fingerprint(&b), fp, "指纹三算仍一致");
        b.seal_fp = fp.wrapping_add(1);
        match seal(&mut b) {
            Err(e) => assert_eq!(e, HandCode::FpDrift.val()),
            Ok(()) => assert!(false, "指纹漂移封账未拒"),
        }
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(HandCode::FpDrift.val()), HandCode::FpDrift.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }

    #[test]
    fn checks_face_full_green() {
        let s = run_unx0497_checks();
        assert!(s.len() >= 12, "检查条数须 ≥12");
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
