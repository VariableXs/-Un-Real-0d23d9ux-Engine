//! UNX-F48801 统一输入事件层总成 · 0x70000
//! 覆盖卡：UNX-F48801（input_event 十族 type 全语义）、UNX-F48802（EVIOCGBIT 能力位图读取面）
//!
//! ── 闸门次序 ──
//! 本模块把「事件进入事件层」拆成五道不可换序的闸：
//!   ① 族闸：`kind` 必须落在十族闭集内（EV_SYN/KEY/REL/ABS/MSC/SW/LED/SND/REP/FF）。
//!      若放到位图闸之后判，未知族（例如伪造的 0x99）会先被当成「设备未声明该位」
//!      报成能力缺失——这是**错误定责**：坏的是事件族号，不是设备能力。
//!   ② 能力闸：设备 evbit 未声明该族 → 拒。此闸必须早于码闸，因为 evbit 是
//!      「这台设备有没有这一类」，位图是「这一类里具体哪几个码」；先判族位才能
//!      把「整族不存在」与「族在但码不在」区分开，否则一律报 UnknownCode，
//!      排障时无法判定是设备接错还是驱动映射错。
//!   ③ 码闸：该族 code 位图未置位 → 拒。EV_SYN 是唯一没有码位图的族，
//!      其合法码是闭集 SYN_REPORT/SYN_CONFIG/SYN_DROPPED/SYN_MT_REPORT(≤0x0F)，
//!      所以 SYN 走「上限闭集」而不是「位图置位」——这条分支必须在码闸内显式
//!      分开，否则 SYN 永远会因位图全零被判 UnknownCode。
//!   ④ 处理位闸：十族分发表逐族 `handled` 位必须为真，缺一位即拒。
//!      此闸放在码闸之后：一个「族对、码对、但本层还没接处理位」的事件必须
//!      报 DispatchUnhandled 而不是静默吞掉，这是本层唯一会「显性拒收」的位置。
//!   ⑤ 缓存闸：能力位图只允许在设备枚举时读一次（EVIOCGBIT 语义），
//!      之后一律从节点缓存取。第二次 read 必须报 CacheDoubleRead；缓存未命中
//!      报 CacheMiss。此闸不参与单事件路由，只在枚举/查询面上生效，
//!      但仍写在本模块：位图的生命周期属于能力面，不属于元数据表。
//!
//! ── 关键差异 ──
//! 与 unx48803_input_meta 的边界：本模块**只管「位图怎么说」**，
//!   不持有设备路径/名称/vid-pid/总线拓扑五元组——五元组注册查询归 meta。
//!   本模块的 `CapCache` 以 `dev: u16` 数字句柄做键，句柄到路径的映射由 meta
//!   负责；把路径塞进本模块会让位图面与节点面互相持有，注销时序无法收敛。
//! 与 unx48805_input_guard 的边界：本模块**判死**的是「族/码/能力/处理位」，
//!   属于**静态合法性**；超时合成、时间戳单调、4096 溢出属于**运行期时序**，
//!   一律归 guard。本模块对任何事件都不做时间判断（`ts_us` 只参与指纹，不参与判决）。
//! 与 unx48808_input_dispatch 的边界：本模块给出**分发表内容**（十族 × 处理位），
//!   dispatch 模块给出**分发执行**（三主族路由计数、per-CPU 环、批量出队）。
//!   本模块的 `dispatch()` 是「一次性判定 + 路由标签」，不缓冲、不排队、不重试。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址（B01 族首段）。
pub const SEG_BASE: u32 = 0x70000;

// ── 十族 type 号（Linux input_event.type 语义等价，冻结常量）──────
/// 同步事件族。
pub const EV_SYN: u16 = 0x00;
/// 按键事件族。
pub const EV_KEY: u16 = 0x01;
/// 相对位移事件族。
pub const EV_REL: u16 = 0x02;
/// 绝对坐标事件族。
pub const EV_ABS: u16 = 0x03;
/// 杂项事件族（扫描码/时间戳等）。
pub const EV_MSC: u16 = 0x04;
/// 开关事件族（ lid/tablet 等）。
pub const EV_SW: u16 = 0x05;
/// LED 事件族。
pub const EV_LED: u16 = 0x11;
/// 声音事件族。
pub const EV_SND: u16 = 0x12;
/// 自动重复事件族。
pub const EV_REP: u16 = 0x14;
/// 力反馈事件族。
pub const EV_FF: u16 = 0x15;

// ── 同步子码（EV_SYN 的码闭集，冻结）──────────────────────────
/// 帧结束（本层唯一的提交点）。
pub const SYN_REPORT: u16 = 0;
/// 设备配置已变（需重读位图）。
pub const SYN_CONFIG: u16 = 1;
/// 驱动侧丢了事件（帧必须作废）。
pub const SYN_DROPPED: u16 = 3;
/// 多点触控逐槽上报分隔符。
pub const SYN_MT_REPORT: u16 = 2;

/// 单族码位图的最大码位扫描上限（KEY/FF 的 0x2FF 是十族里最宽的一族）。
pub const CODE_SCAN_MAX: u16 = 0x2FF;
/// 每族码位图字数（64 位 × 12 = 768 位，刚好覆盖 KEY 族最宽码 0x2FF）。
/// 统一按最宽族定容，避免「KEY 位图只存 128 位、0x110 以上的键静默丢失」
/// 这类按族裁剪容量造成的假通过。
pub const CODE_WORDS: usize = 12;
/// 十族数量。
pub const FAMILY_COUNT: usize = 10;
/// 能力转储的定长字节数：4 字节 evbit + 10 族 × 2 字 × 8 字节。
pub const DUMP_LEN: usize = 4 + FAMILY_COUNT * CODE_WORDS * 8;
/// 判据轮数（卡面「×10」的硬口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 样本设备台数（卡面「×10 设备」的硬口径）。
pub const SAMPLE_DEVICES: usize = 10;

// ── 诊断码闭集（12 条，SEG_BASE+1 起，≤0x1F 窗口）──────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InCode {
    /// 事件族不在十族闭集内。
    UnknownType,
    /// 码不在该族合法闭集 / 位图未置位。
    UnknownCode,
    /// evbit 出现十族以外的位（保留位置位）。
    BadEvBit,
    /// 能力转储长度不符定长。
    BadDumpLen,
    /// 族内保留码位被置（越该族 max_code）。
    ReservedBitSet,
    /// 缓存未命中（设备未枚举就查位图）。
    CacheMiss,
    /// 二次读取位图（违反一次读取纪律）。
    CacheDoubleRead,
    /// 设备能力位图未声明该族。
    CapabilityDenied,
    /// 分发表句柄数/槽位错乱。
    TableSkew,
    /// 十族处理位缺失。
    DispatchUnhandled,
    /// 黄金指纹漂移。
    DigestMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl InCode {
    /// 段内绝对码值（SEG_BASE+1 起，避开段首）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 逐条中文文案（排障时直接可读，不做二次解释）。
    pub fn detail(self) -> &'static str {
        match self {
            InCode::UnknownType => "未知事件族：不在十族闭集",
            InCode::UnknownCode => "未知码：该族位图未置位",
            InCode::BadEvBit => "evbit 保留位置位：十族以外",
            InCode::BadDumpLen => "能力转储长度不符定长",
            InCode::ReservedBitSet => "族内保留码位被置：越 max_code",
            InCode::CacheMiss => "能力缓存未命中：设备未枚举",
            InCode::CacheDoubleRead => "位图二次读取：违反一次读取纪律",
            InCode::CapabilityDenied => "设备能力未声明该族",
            InCode::TableSkew => "分发表句柄错乱：十族槽位不符",
            InCode::DispatchUnhandled => "十族处理位缺失",
            InCode::DigestMismatch => "黄金指纹漂移",
            InCode::NotReproducible => "双跑不一致",
        }
    }

    /// 闭集全表（顺序即 `val()` 的枚举序）。
    pub const ALL: [InCode; 12] = [
        InCode::UnknownType,
        InCode::UnknownCode,
        InCode::BadEvBit,
        InCode::BadDumpLen,
        InCode::ReservedBitSet,
        InCode::CacheMiss,
        InCode::CacheDoubleRead,
        InCode::CapabilityDenied,
        InCode::TableSkew,
        InCode::DispatchUnhandled,
        InCode::DigestMismatch,
        InCode::NotReproducible,
    ];
}

/// 码值 → 中文文案；未知码返回闭集外说明（不静默、不借用邻居文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < InCode::ALL.len() {
        if let Some(c) = InCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70001..0x7000C 闭集内"
}

// ── 基础设施：FNV-1a 64（本域按「位图指纹」语义命名）──────────
/// 位图指纹初值。
pub fn print_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一个字节（F48802 位图逐字节取证用）。
pub fn print_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入一个字（按小端 8 字节）。
pub fn print_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 8 {
        acc = print_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 折入一段设备名（ASCII/UTF-8 逐字节）。
pub fn print_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = print_byte(acc, *b);
        }
        i += 1;
    }
    acc
}

/// 位图取位：越界视为未置位（不 panic、不猜测）。
pub fn bit_get(words: &[u64], code: u16) -> bool {
    let w = (code as usize) / 64;
    let b = (code as usize) % 64;
    match words.get(w) {
        Some(v) => ((*v >> b) & 1) == 1,
        None => false,
    }
}

/// 位图置位：越界拒置并返回 false（调用方负责记账）。
pub fn bit_set(words: &mut [u64], code: u16) -> bool {
    let w = (code as usize) / 64;
    let b = (code as usize) % 64;
    match words.get_mut(w) {
        Some(v) => {
            *v |= 1u64 << b;
            true
        }
        None => false,
    }
}

// ── 十族枚举 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvFamily {
    /// EV_SYN 同步族。
    Syn,
    /// EV_KEY 按键族（三主族之一）。
    Key,
    /// EV_REL 相对位移族（三主族之一）。
    Rel,
    /// EV_ABS 绝对坐标族（三主族之一）。
    Abs,
    /// EV_MSC 杂项族。
    Msc,
    /// EV_SW 开关族。
    Sw,
    /// EV_LED 指示灯族。
    Led,
    /// EV_SND 声音族。
    Snd,
    /// EV_REP 自动重复族。
    Rep,
    /// EV_FF 力反馈族。
    Ff,
}

impl EvFamily {
    /// 该族的 type 号。
    pub fn code(self) -> u16 {
        match self {
            EvFamily::Syn => EV_SYN,
            EvFamily::Key => EV_KEY,
            EvFamily::Rel => EV_REL,
            EvFamily::Abs => EV_ABS,
            EvFamily::Msc => EV_MSC,
            EvFamily::Sw => EV_SW,
            EvFamily::Led => EV_LED,
            EvFamily::Snd => EV_SND,
            EvFamily::Rep => EV_REP,
            EvFamily::Ff => EV_FF,
        }
    }

    /// 位图槽位（0..9，与 `ALL` 顺序一致，也是 evbit 的位号）。
    pub fn slot(self) -> usize {
        match self {
            EvFamily::Syn => 0,
            EvFamily::Key => 1,
            EvFamily::Rel => 2,
            EvFamily::Abs => 3,
            EvFamily::Msc => 4,
            EvFamily::Sw => 5,
            EvFamily::Led => 6,
            EvFamily::Snd => 7,
            EvFamily::Rep => 8,
            EvFamily::Ff => 9,
        }
    }

    /// 中文族名（报告与错误三要素共用）。
    pub fn name(self) -> &'static str {
        match self {
            EvFamily::Syn => "EV_SYN 同步",
            EvFamily::Key => "EV_KEY 按键",
            EvFamily::Rel => "EV_REL 相对",
            EvFamily::Abs => "EV_ABS 绝对",
            EvFamily::Msc => "EV_MSC 杂项",
            EvFamily::Sw => "EV_SW 开关",
            EvFamily::Led => "EV_LED 指示灯",
            EvFamily::Snd => "EV_SND 声音",
            EvFamily::Rep => "EV_REP 重复",
            EvFamily::Ff => "EV_FF 力反馈",
        }
    }

    /// 三主族标记（KEY/REL/ABS 优先落地）。
    pub fn primary(self) -> bool {
        matches!(self, EvFamily::Key | EvFamily::Rel | EvFamily::Abs)
    }

    /// 该族合法码上限（含）。
    pub fn max_code(self) -> u16 {
        match self {
            EvFamily::Syn => 0x0F,
            EvFamily::Key => 0x2FF,
            EvFamily::Rel => 0x0F,
            EvFamily::Abs => 0x3F,
            EvFamily::Msc => 0x07,
            EvFamily::Sw => 0x10,
            EvFamily::Led => 0x0F,
            EvFamily::Snd => 0x07,
            EvFamily::Rep => 0x01,
            EvFamily::Ff => 0x7F,
        }
    }

    /// 该族码位图的字节数（EVIOCGBIT 的 size 参数）。
    pub fn bitmap_bytes(self) -> u32 {
        (self.max_code() as u32) / 8 + 1
    }

    /// 该族的处理位（1 << slot）。
    pub fn handle_bit(self) -> u32 {
        1u32 << self.slot()
    }

    /// 十族全表（顺序即槽位）。
    pub const ALL: [EvFamily; FAMILY_COUNT] = [
        EvFamily::Syn,
        EvFamily::Key,
        EvFamily::Rel,
        EvFamily::Abs,
        EvFamily::Msc,
        EvFamily::Sw,
        EvFamily::Led,
        EvFamily::Snd,
        EvFamily::Rep,
        EvFamily::Ff,
    ];

    /// type 号 → 族；十族以外返回 None（**不**回退到任意默认族）。
    pub fn from_code(c: u16) -> Option<EvFamily> {
        match c {
            EV_SYN => Some(EvFamily::Syn),
            EV_KEY => Some(EvFamily::Key),
            EV_REL => Some(EvFamily::Rel),
            EV_ABS => Some(EvFamily::Abs),
            EV_MSC => Some(EvFamily::Msc),
            EV_SW => Some(EvFamily::Sw),
            EV_LED => Some(EvFamily::Led),
            EV_SND => Some(EvFamily::Snd),
            EV_REP => Some(EvFamily::Rep),
            EV_FF => Some(EvFamily::Ff),
            _ => None,
        }
    }

    /// 槽位 → 族（越界返回 None）。
    pub fn from_slot(slot: usize) -> Option<EvFamily> {
        let mut i = 0;
        while i < EvFamily::ALL.len() {
            if let Some(f) = EvFamily::ALL.get(i) {
                if f.slot() == slot {
                    return Some(*f);
                }
            }
            i += 1;
        }
        None
    }
}

/// 十族 evbit 全置的掩码（0x3FF：槽位 0..9）。
pub const EV_MASK_ALL: u32 = (1u32 << FAMILY_COUNT) - 1;

// ── 十族分发表（F48801 的 J1 核心）────────────────────────────
/// 单族表项：族、处理位、接线数（每族接到下游的消费者数量）、说明。
#[derive(Clone, Copy, Debug)]
pub struct FamilyDesc {
    /// 族。
    pub fam: EvFamily,
    /// 处理位是否已接（十族全 true 是本层「全语义」的硬要求）。
    pub handled: bool,
    /// 下游消费者数（KEY→win32k 键队列/IME；REL→指针；ABS→MT；余类推）。
    pub consumers: u8,
    /// 该族的语义备注（逐族不同，防止表被复制粘贴成同构）。
    pub note: &'static str,
}

/// 十族分发表：顺序即槽位，缺一即 `TableSkew`。
pub const DISPATCH_TABLE: [FamilyDesc; FAMILY_COUNT] = [
    FamilyDesc { fam: EvFamily::Syn, handled: true, consumers: 1, note: "帧提交点：SYN_REPORT 才让整帧可见" },
    FamilyDesc { fam: EvFamily::Key, handled: true, consumers: 3, note: "键队列/IME/热键三消费，value=0 释放" },
    FamilyDesc { fam: EvFamily::Rel, handled: true, consumers: 2, note: "指针位移与滚轮两路，值可正可负" },
    FamilyDesc { fam: EvFamily::Abs, handled: true, consumers: 2, note: "绝对坐标与 MT 槽，先报位置再报触" },
    FamilyDesc { fam: EvFamily::Msc, handled: true, consumers: 1, note: "扫描码/原始码取证，不参与语义判决" },
    FamilyDesc { fam: EvFamily::Sw, handled: true, consumers: 1, note: "lid/tablet 开关，边沿触发不重报" },
    FamilyDesc { fam: EvFamily::Led, handled: true, consumers: 1, note: "灯态回写，与锁定键双向同步" },
    FamilyDesc { fam: EvFamily::Snd, handled: true, consumers: 1, note: "蜂鸣/音调，仅 PC 喇叭设备声明" },
    FamilyDesc { fam: EvFamily::Rep, handled: true, consumers: 1, note: "重复延迟/周期两码，设备级参数" },
    FamilyDesc { fam: EvFamily::Ff, handled: true, consumers: 1, note: "力反馈效果上传，手柄专用" },
];

/// 查族表项：槽位越界或族不符返回 None。
pub fn desc_of(fam: EvFamily) -> Option<&'static FamilyDesc> {
    let mut i = 0;
    while i < DISPATCH_TABLE.len() {
        if let Some(d) = DISPATCH_TABLE.get(i) {
            if d.fam == fam {
                return Some(d);
            }
        }
        i += 1;
    }
    None
}

/// 表结构自检：槽位 0..9 逐位对齐、族号无重复、十位全 handled。
pub fn table_check() -> Result<u32, u32> {
    let mut bits = 0u32;
    let mut i = 0usize;
    while i < DISPATCH_TABLE.len() {
        let d = match DISPATCH_TABLE.get(i) {
            Some(d) => d,
            None => return Err(InCode::TableSkew.val()),
        };
        if d.fam.slot() != i {
            return Err(InCode::TableSkew.val());
        }
        if !d.handled {
            return Err(InCode::DispatchUnhandled.val());
        }
        if bits & d.fam.handle_bit() != 0 {
            return Err(InCode::TableSkew.val());
        }
        bits |= d.fam.handle_bit();
        i += 1;
    }
    if bits != EV_MASK_ALL {
        return Err(InCode::TableSkew.val());
    }
    Ok(bits)
}

// ── 事件记录（input_event{timestamp,type,code,value} 等价）─────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// 时间戳（微秒，本层只用于取证与指纹，不参与合法性判决）。
    pub ts_us: u64,
    /// type 号。
    pub kind: u16,
    /// code 号。
    pub code: u16,
    /// 值（按键 0/1/2；相对位移有符号；绝对坐标按轴量纲）。
    pub value: i32,
}

impl InputEvent {
    /// 构造（不做校验：校验属于 `dispatch`，构造保持零成本）。
    pub const fn new(ts_us: u64, kind: u16, code: u16, value: i32) -> InputEvent {
        InputEvent { ts_us, kind, code, value }
    }

    /// 取族（十族以外 → UnknownType）。
    pub fn family(&self) -> Result<EvFamily, u32> {
        match EvFamily::from_code(self.kind) {
            Some(f) => Ok(f),
            None => Err(InCode::UnknownType.val()),
        }
    }

    /// 是否帧结束（唯一的提交点）。
    pub fn is_frame_end(&self) -> bool {
        self.kind == EV_SYN && self.code == SYN_REPORT
    }

    /// 是否帧作废点（驱动丢事件）。
    pub fn is_frame_break(&self) -> bool {
        self.kind == EV_SYN && self.code == SYN_DROPPED
    }

    /// 是否 MT 逐槽分隔符。
    pub fn is_mt_sep(&self) -> bool {
        self.kind == EV_SYN && self.code == SYN_MT_REPORT
    }

    /// 事件指纹（ts/kind/code/value 四字段）。
    pub fn digest(&self) -> u64 {
        let mut h = print_init();
        h = print_word(h, self.ts_us);
        h = print_word(h, self.kind as u64);
        h = print_word(h, self.code as u64);
        h = print_word(h, self.value as u32 as u64);
        h
    }
}

// ── 能力位图（F48802 核心）─────────────────────────────────────
/// 设备能力位图：evbit 用 10 位掩码，码位图按族槽位定容 2 字。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CapBits {
    /// evbit（位号 = 族槽位）。
    pub ev: u32,
    /// 每族码位图（[槽位][字]）。
    pub codes: [[u64; CODE_WORDS]; FAMILY_COUNT],
}

impl CapBits {
    /// 全零位图（无任何能力）。
    pub fn empty() -> CapBits {
        CapBits { ev: 0, codes: [[0u64; CODE_WORDS]; FAMILY_COUNT] }
    }

    /// 置族位（槽位从族取，不信任外部数字）。
    pub fn set_ev(&mut self, fam: EvFamily) {
        self.ev |= fam.handle_bit();
    }

    /// 查族位。
    pub fn has_ev(&self, fam: EvFamily) -> bool {
        (self.ev & fam.handle_bit()) != 0
    }

    /// 置码位；越该族 max_code 返回 false 并且**不置位**（保留位必须干净）。
    pub fn set_code(&mut self, fam: EvFamily, code: u16) -> bool {
        if code > fam.max_code() {
            return false;
        }
        match self.codes.get_mut(fam.slot()) {
            Some(words) => bit_set(words, code),
            None => false,
        }
    }

    /// 查码位（越族上限一律 false，不依赖底层字长）。
    pub fn has_code(&self, fam: EvFamily, code: u16) -> bool {
        if code > fam.max_code() {
            return false;
        }
        match self.codes.get(fam.slot()) {
            Some(words) => bit_get(words, code),
            None => false,
        }
    }

    /// 裸置位（仅供负向探针构造「保留位置位」的坏转储；正常路径走 `set_code`）。
    pub fn force_code(&mut self, fam: EvFamily, code: u16) -> bool {
        match self.codes.get_mut(fam.slot()) {
            Some(words) => bit_set(words, code),
            None => false,
        }
    }

    /// 解码定长转储：4 字节 evbit + 10 族 × 16 字节码位图。
    pub fn decode(bytes: &[u8]) -> Result<CapBits, u32> {
        if bytes.len() != DUMP_LEN {
            return Err(InCode::BadDumpLen.val());
        }
        let ev = read_u32(bytes, 0)?;
        if ev & !EV_MASK_ALL != 0 {
            return Err(InCode::BadEvBit.val());
        }
        let mut caps = CapBits::empty();
        caps.ev = ev;
        let mut slot = 0usize;
        while slot < FAMILY_COUNT {
            let base = 4 + slot * (CODE_WORDS * 8);
            let mut words = [0u64; CODE_WORDS];
            let mut k = 0usize;
            while k < CODE_WORDS {
                let w = read_u64(bytes, base + k * 8)?;
                match words.get_mut(k) {
                    Some(v) => *v = w,
                    None => return Err(InCode::BadDumpLen.val()),
                }
                k += 1;
            }
            match caps.codes.get_mut(slot) {
                Some(dst) => {
                    *dst = words;
                }
                None => return Err(InCode::TableSkew.val()),
            }
            slot += 1;
        }
        // 保留位审计：每族超过 max_code 的位必须为零。
        let mut fi = 0usize;
        while fi < FAMILY_COUNT {
            if let Some(fam) = EvFamily::from_slot(fi) {
                let mut c = fam.max_code().saturating_add(1);
                while c <= CODE_SCAN_MAX {
                    let raw = match caps.codes.get(fi) {
                        Some(words) => bit_get(words, c),
                        None => return Err(InCode::TableSkew.val()),
                    };
                    if raw {
                        return Err(InCode::ReservedBitSet.val());
                    }
                    c = c.saturating_add(1);
                }
            }
            fi += 1;
        }
        Ok(caps)
    }

    /// 编码回定长转储（用于双跑对拍与缓存放行）。
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DUMP_LEN);
        let evb = self.ev.to_le_bytes();
        let mut i = 0usize;
        while i < evb.len() {
            if let Some(b) = evb.get(i) {
                out.push(*b);
            }
            i += 1;
        }
        let mut slot = 0usize;
        while slot < FAMILY_COUNT {
            let mut words = [0u64; CODE_WORDS];
            if let Some(src) = self.codes.get(slot) {
                words = *src;
            }
            let mut k = 0usize;
            while k < CODE_WORDS {
                let w = match words.get(k) {
                    Some(v) => *v,
                    None => 0u64,
                };
                let b = w.to_le_bytes();
                let mut j = 0usize;
                while j < 8 {
                    if let Some(v) = b.get(j) {
                        out.push(*v);
                    }
                    j += 1;
                }
                k += 1;
            }
            slot += 1;
        }
        out
    }

    /// 位图指纹（解码后逐字节，双跑对拍用）。
    pub fn digest(&self) -> u64 {
        let bytes = self.encode();
        let mut h = print_init();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                h = print_byte(h, *b);
            }
            i += 1;
        }
        h
    }

    /// 定长度（供探针断言）。
    pub fn len(&self) -> usize {
        DUMP_LEN
    }

    /// 族内已置码数（报告用，不依赖位图字长）。
    pub fn count_codes(&self, fam: EvFamily) -> u32 {
        let mut n = 0u32;
        let mut c = 0u16;
        while c <= fam.max_code() {
            if self.has_code(fam, c) {
                n += 1;
            }
            c = c.saturating_add(1);
        }
        n
    }
}

/// 小端读 u32（越界 → BadDumpLen，不 panic）。
fn read_u32(bytes: &[u8], off: usize) -> Result<u32, u32> {
    let b0 = match bytes.get(off) {
        Some(v) => *v,
        None => return Err(InCode::BadDumpLen.val()),
    };
    let b1 = match bytes.get(off + 1) {
        Some(v) => *v,
        None => return Err(InCode::BadDumpLen.val()),
    };
    let b2 = match bytes.get(off + 2) {
        Some(v) => *v,
        None => return Err(InCode::BadDumpLen.val()),
    };
    let b3 = match bytes.get(off + 3) {
        Some(v) => *v,
        None => return Err(InCode::BadDumpLen.val()),
    };
    Ok(u32::from_le_bytes([b0, b1, b2, b3]))
}

/// 小端读 u64（越界 → BadDumpLen，不 panic）。
fn read_u64(bytes: &[u8], off: usize) -> Result<u64, u32> {
    let lo = read_u32(bytes, off)?;
    let hi = read_u32(bytes, off + 4)?;
    Ok(((hi as u64) << 32) | lo as u64)
}

// ── EVIOCGBIT 语义等价的 ioctl 面（F48802）────────────────────
/// `_IOC_READ` 方向位。
pub const IOC_READ: u32 = 2;
/// input 子系统 ioctl 幻数 'E'。
pub const IOC_EV_MAGIC: u32 = 0x45;
/// EVIOCGBIT 的 nr 基数。
pub const EVIOCGBIT_BASE: u32 = 0x20;

/// 合成 EVIOCGBIT(type) 的 ioctl 号：dir<<30 | size<<16 | 'E'<<8 | (0x20+type)。
pub fn eviocgbit_ioctl(fam: EvFamily) -> u32 {
    (IOC_READ << 30)
        | ((fam.bitmap_bytes() & 0x3FFF) << 16)
        | (IOC_EV_MAGIC << 8)
        | (EVIOCGBIT_BASE + fam.code() as u32)
}

/// 从 ioctl 号反解 nr（族号校验的逆运算）。
pub fn eviocgbit_nr(ioctl: u32) -> u32 {
    ioctl & 0xFF
}

/// 从 ioctl 号反解 size。
pub fn eviocgbit_size(ioctl: u32) -> u32 {
    (ioctl >> 16) & 0x3FFF
}

/// EVIOCGBIT 编码自检：十族 nr/size 必须与族表逐位一致。
pub fn eviocgbit_selfcheck() -> Result<(), u32> {
    let mut i = 0usize;
    while i < FAMILY_COUNT {
        let fam = match EvFamily::from_slot(i) {
            Some(f) => f,
            None => return Err(InCode::TableSkew.val()),
        };
        let io = eviocgbit_ioctl(fam);
        if eviocgbit_nr(io) != EVIOCGBIT_BASE + fam.code() as u32 {
            return Err(InCode::TableSkew.val());
        }
        if eviocgbit_size(io) != fam.bitmap_bytes() {
            return Err(InCode::TableSkew.val());
        }
        if (io >> 8) & 0xFF != IOC_EV_MAGIC {
            return Err(InCode::TableSkew.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 样本设备画像 ×10（位图「与实物逐位一致」的实物侧账）────────
/// 单台设备的能力画像（按实物键位表手写，不是「全位全置」的占位）。
#[derive(Clone, Debug)]
pub struct SpecDevice {
    /// 设备数字句柄（元数据表另有路径，见 unx48803）。
    pub id: u16,
    /// 实物名（用于指纹与报告）。
    pub name: &'static str,
    /// evbit 掩码。
    pub ev: u32,
    /// EV_KEY 键码表（逐个列出）。
    pub keys: Vec<u16>,
    /// EV_REL 轴码表。
    pub rels: Vec<u16>,
    /// EV_ABS 轴码表。
    pub abss: Vec<u16>,
    /// EV_MSC 码表。
    pub mscs: Vec<u16>,
    /// EV_SW 码表。
    pub sws: Vec<u16>,
    /// EV_LED 码表。
    pub leds: Vec<u16>,
    /// EV_SND 码表。
    pub snds: Vec<u16>,
    /// EV_REP 码表。
    pub reps: Vec<u16>,
    /// EV_FF 码表。
    pub ffs: Vec<u16>,
}

impl SpecDevice {
    /// 该画像是否声明某族。
    pub fn has_ev(&self, fam: EvFamily) -> bool {
        (self.ev & fam.handle_bit()) != 0
    }

    /// 该画像在某族是否含某码（实物侧的唯一权威）。
    pub fn has(&self, fam: EvFamily, code: u16) -> bool {
        match fam {
            EvFamily::Syn => self.has_ev(fam) && code <= fam.max_code(),
            EvFamily::Key => self.keys.contains(&code),
            EvFamily::Rel => self.rels.contains(&code),
            EvFamily::Abs => self.abss.contains(&code),
            EvFamily::Msc => self.mscs.contains(&code),
            EvFamily::Sw => self.sws.contains(&code),
            EvFamily::Led => self.leds.contains(&code),
            EvFamily::Snd => self.snds.contains(&code),
            EvFamily::Rep => self.reps.contains(&code),
            EvFamily::Ff => self.ffs.contains(&code),
        }
    }

    /// 画像 → 位图（族位置位 + 逐码置位，越界码在此处即被 `set_code` 拒）。
    pub fn to_caps(&self) -> CapBits {
        let mut c = CapBits::empty();
        let mut fi = 0usize;
        while fi < FAMILY_COUNT {
            if let Some(fam) = EvFamily::from_slot(fi) {
                if self.has_ev(fam) {
                    c.set_ev(fam);
                }
            }
            fi += 1;
        }
        let mut i = 0usize;
        while i < self.keys.len() {
            if let Some(code) = self.keys.get(i) {
                c.set_code(EvFamily::Key, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.rels.len() {
            if let Some(code) = self.rels.get(i) {
                c.set_code(EvFamily::Rel, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.abss.len() {
            if let Some(code) = self.abss.get(i) {
                c.set_code(EvFamily::Abs, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.mscs.len() {
            if let Some(code) = self.mscs.get(i) {
                c.set_code(EvFamily::Msc, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.sws.len() {
            if let Some(code) = self.sws.get(i) {
                c.set_code(EvFamily::Sw, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.leds.len() {
            if let Some(code) = self.leds.get(i) {
                c.set_code(EvFamily::Led, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.snds.len() {
            if let Some(code) = self.snds.get(i) {
                c.set_code(EvFamily::Snd, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.reps.len() {
            if let Some(code) = self.reps.get(i) {
                c.set_code(EvFamily::Rep, *code);
            }
            i += 1;
        }
        i = 0;
        while i < self.ffs.len() {
            if let Some(code) = self.ffs.get(i) {
                c.set_code(EvFamily::Ff, *code);
            }
            i += 1;
        }
        c
    }

    /// 画像 → 定长转储（枚举时写入节点元数据的那一坨字节）。
    pub fn dump(&self) -> Vec<u8> {
        self.to_caps().encode()
    }

    /// 逐位核对：解码结果的每一位都必须等于实物侧 `has()`。
    ///
    /// 这是「位图读取 ×10 设备与实物逐位一致」的判决核心：
    /// 只要有一台设备的任一位（多置或少置）不符即返回 DigestMismatch，
    /// 不做「抽样通过」的降级。
    pub fn verify(&self, caps: &CapBits) -> Result<u32, u32> {
        if caps.ev != self.ev {
            return Err(InCode::DigestMismatch.val());
        }
        let mut fi = 0usize;
        let mut checked = 0u32;
        while fi < FAMILY_COUNT {
            let fam = match EvFamily::from_slot(fi) {
                Some(f) => f,
                None => return Err(InCode::TableSkew.val()),
            };
            // EV_SYN 是唯一没有码位图的族：它的合法码由闭集上限判，
            // 位图里不该有、也不可能有 SYN 的码位——此族只核族位，跳过码位。
            if fam != EvFamily::Syn {
                let mut code = 0u16;
                while code <= fam.max_code() {
                    let want = self.has(fam, code);
                    let got = caps.has_code(fam, code);
                    if want != got {
                        return Err(InCode::DigestMismatch.val());
                    }
                    checked += 1;
                    code = code.saturating_add(1);
                }
            }
            fi += 1;
        }
        Ok(checked)
    }
}

/// 十台样本设备画像（逐台手写；代表桌面/移动/游戏/遥控/手柄五类实物）。
pub fn device_specs() -> Vec<SpecDevice> {
    let mut v: Vec<SpecDevice> = Vec::new();

    // ① 104 键 AT 键盘：键盘三件套（KEY/MSC/LED/REP/SYN），无指针轴。
    v.push(SpecDevice {
        id: 0x1001,
        name: "atkbd-104",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Msc.handle_bit()
            | EvFamily::Led.handle_bit()
            | EvFamily::Rep.handle_bit(),
        keys: (0x01u16..=0x58).collect(),
        rels: vec![],
        abss: vec![],
        mscs: vec![0x04],
        sws: vec![],
        leds: vec![0x00, 0x01, 0x02],
        snds: vec![],
        reps: vec![0x00, 0x01],
        ffs: vec![],
    });

    // ② 87 键紧凑键盘：比 104 少 F13..F24 与部分编辑键。
    v.push(SpecDevice {
        id: 0x1002,
        name: "atkbd-87",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Msc.handle_bit()
            | EvFamily::Led.handle_bit()
            | EvFamily::Rep.handle_bit(),
        keys: (0x01u16..=0x53).collect(),
        rels: vec![],
        abss: vec![],
        mscs: vec![0x04],
        sws: vec![],
        leds: vec![0x00, 0x01, 0x02],
        snds: vec![],
        reps: vec![0x00, 0x01],
        ffs: vec![],
    });

    // ③ PS/2 三键鼠标：左右中 + 侧键，REL_X/Y/WHEEL。
    v.push(SpecDevice {
        id: 0x1003,
        name: "ps2-mouse",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Rel.handle_bit(),
        keys: vec![0x110, 0x111, 0x112, 0x113, 0x114],
        rels: vec![0x00, 0x01, 0x08],
        abss: vec![],
        mscs: vec![],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![],
        ffs: vec![],
    });

    // ④ 触控板：按键 + REL（滚轮轴）+ ABS（压力/MT 槽）+ MSC 时间戳。
    v.push(SpecDevice {
        id: 0x1004,
        name: "synaptics-touchpad",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Rel.handle_bit()
            | EvFamily::Abs.handle_bit()
            | EvFamily::Msc.handle_bit(),
        keys: vec![0x110, 0x111, 0x113, 0x145, 0x14A],
        rels: vec![0x00, 0x01, 0x06, 0x08],
        abss: vec![0x00, 0x01, 0x18, 0x2F, 0x30, 0x31, 0x32, 0x35, 0x36, 0x39, 0x3A, 0x3C, 0x3D, 0x3E, 0x3F],
        mscs: vec![0x05],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![],
        ffs: vec![],
    });

    // ⑤ 触摸屏：单点/多点轴齐全，无 REL（绝对设备不报相对位移）。
    v.push(SpecDevice {
        id: 0x1005,
        name: "elan-touchscreen",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Abs.handle_bit()
            | EvFamily::Msc.handle_bit(),
        keys: vec![0x14A],
        rels: vec![],
        abss: vec![0x00, 0x01, 0x18, 0x2F, 0x30, 0x35, 0x36, 0x39, 0x3A, 0x3B],
        mscs: vec![0x05],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![],
        ffs: vec![],
    });

    // ⑥ Xbox 手柄：按键 + 双摇杆绝对轴 + 扳机 + 力反馈。
    v.push(SpecDevice {
        id: 0x1006,
        name: "xbox-gamepad",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Abs.handle_bit()
            | EvFamily::Ff.handle_bit(),
        keys: (0x130u16..=0x13F).chain(0x220u16..=0x223).collect(),
        rels: vec![],
        abss: vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x10, 0x11],
        mscs: vec![],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![],
        ffs: (0x50u16..=0x5Fu16).collect(),
    });

    // ⑦ HDMI-CEC 遥控：只有按键 + 自动重复，无指针轴。
    v.push(SpecDevice {
        id: 0x1007,
        name: "cec-remote",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Rep.handle_bit(),
        keys: (0x160u16..=0x16Fu16).collect(),
        rels: vec![],
        abss: vec![],
        mscs: vec![],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![0x00, 0x01],
        ffs: vec![],
    });

    // ⑧ 电源键：单键单族，最容易被「全位置位」的假实现污染。
    v.push(SpecDevice {
        id: 0x1008,
        name: "pwrbutton",
        ev: EvFamily::Syn.handle_bit() | EvFamily::Key.handle_bit(),
        keys: vec![0x74],
        rels: vec![],
        abss: vec![],
        mscs: vec![],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![],
        ffs: vec![],
    });

    // ⑨ 数位板：笔尖键 + 绝对坐标 + 距离/倾斜，MSC 扫描码取证。
    v.push(SpecDevice {
        id: 0x1009,
        name: "wacom-tablet",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Abs.handle_bit()
            | EvFamily::Msc.handle_bit(),
        keys: vec![0x110, 0x14A],
        rels: vec![],
        abss: vec![0x00, 0x01, 0x18, 0x26, 0x27],
        mscs: vec![0x04],
        sws: vec![],
        leds: vec![],
        snds: vec![],
        reps: vec![],
        ffs: vec![],
    });

    // ⑩ 热键组合设备：键盘区 + 多媒体键 + 蜂鸣器 + LED 全色。
    v.push(SpecDevice {
        id: 0x100A,
        name: "hotkeys-combo",
        ev: EvFamily::Syn.handle_bit()
            | EvFamily::Key.handle_bit()
            | EvFamily::Led.handle_bit()
            | EvFamily::Snd.handle_bit()
            | EvFamily::Rep.handle_bit(),
        keys: (0x01u16..=0x53)
            .chain(0xA0u16..=0xAF)
            .chain(0xE0u16..=0xE7)
            .collect(),
        rels: vec![],
        abss: vec![],
        mscs: vec![],
        sws: vec![],
        leds: vec![0x00, 0x01, 0x02],
        snds: vec![0x02, 0x03],
        reps: vec![0x00, 0x01],
        ffs: vec![],
    });

    v
}

// ── 能力缓存（枚举时一次读取）─────────────────────────────────
/// 单条缓存项。
#[derive(Clone, Debug)]
pub struct CapEntry {
    /// 设备句柄。
    pub dev: u16,
    /// 解码后的位图。
    pub caps: CapBits,
    /// 转储指纹（审计：同设备重枚举必然同指纹）。
    pub stamp: u64,
}

/// 能力缓存：一次读取 + 多次命中（EVIOCGBIT 的缓存语义）。
pub struct CapCache {
    entries: Vec<CapEntry>,
    reads: u32,
    hits: u32,
}

impl CapCache {
    /// 空缓存。
    pub fn new() -> CapCache {
        CapCache { entries: Vec::new(), reads: 0, hits: 0 }
    }

    /// 枚举时**唯一**允许的读取入口；对同一设备第二次调用即拒。
    pub fn read_once(&mut self, dev: u16, dump: &[u8]) -> Result<u64, u32> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.dev == dev {
                    return Err(InCode::CacheDoubleRead.val());
                }
            }
            i += 1;
        }
        let caps = CapBits::decode(dump)?;
        let stamp = caps.digest();
        self.reads += 1;
        self.entries.push(CapEntry { dev, caps, stamp });
        Ok(stamp)
    }

    /// 查缓存（只计命中，不触发重读）。
    pub fn cached(&mut self, dev: u16) -> Result<&CapBits, u32> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.dev == dev,
                None => false,
            };
            if hit {
                self.hits += 1;
                let e = match self.entries.get(i) {
                    Some(e) => e,
                    None => return Err(InCode::CacheMiss.val()),
                };
                return Ok(&e.caps);
            }
            i += 1;
        }
        Err(InCode::CacheMiss.val())
    }

    /// 累计真实读取次数（判据：== 设备台数，不多读）。
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// 累计命中次数。
    pub fn hits(&self) -> u32 {
        self.hits
    }

    /// 缓存项数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 缓存是否空（clippy 对称性要求，也是探针入口）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 注销时清项（热插拔路径用；读取计数不回退，账是单调的）。
    pub fn evict(&mut self, dev: u16) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.dev == dev,
                None => false,
            };
            if hit {
                self.entries.remove(i);
                return true;
            }
            i += 1;
        }
        false
    }
}

// ── 路由判决（十族分派 + 能力闸 + 码闸）───────────────────────
/// 一次判定的结果标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Route {
    /// 命中的族。
    pub fam: EvFamily,
    /// 处理位。
    pub bit: u32,
    /// 是否三主族。
    pub primary: bool,
    /// 下游消费者数（来自族表）。
    pub consumers: u8,
}

/// 单事件判定：族 → 能力 → 码 → 处理位（顺序见文件头闸门次序）。
pub fn dispatch(ev: &InputEvent, caps: &CapBits) -> Result<Route, u32> {
    let fam = ev.family()?;
    if !caps.has_ev(fam) {
        return Err(InCode::CapabilityDenied.val());
    }
    if fam == EvFamily::Syn {
        if ev.code > fam.max_code() {
            return Err(InCode::UnknownCode.val());
        }
    } else if !caps.has_code(fam, ev.code) {
        return Err(InCode::UnknownCode.val());
    }
    let d = match desc_of(fam) {
        Some(d) => d,
        None => return Err(InCode::TableSkew.val()),
    };
    if !d.handled {
        return Err(InCode::DispatchUnhandled.val());
    }
    Ok(Route { fam, bit: fam.handle_bit(), primary: fam.primary(), consumers: d.consumers })
}

/// 全能力设备（位图全族全码置位）：用于十族代表事件的路由取证。
pub fn all_caps() -> CapBits {
    let mut c = CapBits::empty();
    let mut fi = 0usize;
    while fi < FAMILY_COUNT {
        if let Some(fam) = EvFamily::from_slot(fi) {
            c.set_ev(fam);
            let mut code = 0u16;
            while code <= fam.max_code() {
                c.set_code(fam, code);
                code = code.saturating_add(1);
            }
        }
        fi += 1;
    }
    c
}

/// 十族代表事件（每族一条，覆盖 KEY 按下/REL 位移/ABS 坐标/…/FF 上传）。
pub fn representative_events() -> Vec<InputEvent> {
    vec![
        InputEvent::new(1_000, EV_SYN, SYN_REPORT, 0),
        InputEvent::new(1_010, EV_KEY, 0x1E, 1),
        InputEvent::new(1_020, EV_REL, 0x00, -3),
        InputEvent::new(1_030, EV_ABS, 0x00, 1_024),
        InputEvent::new(1_040, EV_MSC, 0x04, 0x70016),
        InputEvent::new(1_050, EV_SW, 0x00, 1),
        InputEvent::new(1_060, EV_LED, 0x00, 1),
        InputEvent::new(1_070, EV_SND, 0x02, 440),
        InputEvent::new(1_080, EV_REP, 0x00, 250),
        InputEvent::new(1_090, EV_FF, 0x50, 0x7FFF),
    ]
}

// ── 黄金全链 ──────────────────────────────────────────────────
/// 黄金链输出账面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenOut {
    /// 覆盖设备台数。
    pub devices: u32,
    /// 十族处理位掩码。
    pub handled: u32,
    /// 逐位核对过的 (族,码) 对数。
    pub bits_checked: u32,
    /// 位图指纹链。
    pub digest: u64,
    /// 实际读取次数。
    pub reads: u32,
    /// 缓存命中次数。
    pub hits: u32,
}

/// 黄金全链：十台枚举 → 逐位核对 → 十族代表路由 → 指纹链。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let specs = device_specs();
    if specs.len() != SAMPLE_DEVICES {
        return Err(InCode::TableSkew.val());
    }
    let handled = table_check()?;
    if eviocgbit_selfcheck().is_err() {
        return Err(InCode::TableSkew.val());
    }
    let mut cache = CapCache::new();
    let mut digest = print_init();
    let mut bits_checked = 0u32;
    let mut i = 0usize;
    while i < specs.len() {
        let spec = match specs.get(i) {
            Some(s) => s,
            None => return Err(InCode::TableSkew.val()),
        };
        let dump = spec.dump();
        let stamp = cache.read_once(spec.id, &dump)?;
        if stamp != spec.to_caps().digest() {
            return Err(InCode::DigestMismatch.val());
        }
        let caps = cache.cached(spec.id)?;
        bits_checked += spec.verify(caps)?;
        digest = print_word(digest, stamp);
        digest = print_str(digest, spec.name);
        i += 1;
    }
    if cache.reads() != SAMPLE_DEVICES as u32 {
        return Err(InCode::CacheDoubleRead.val());
    }
    // 十族代表事件：全能力设备上逐族路由，位必须是本族位。
    let caps = all_caps();
    let evs = representative_events();
    let mut seen: u32 = 0;
    let mut k = 0usize;
    while k < evs.len() {
        let ev = match evs.get(k) {
            Some(e) => e,
            None => return Err(InCode::TableSkew.val()),
        };
        let r = dispatch(ev, &caps)?;
        if r.bit != r.fam.handle_bit() {
            return Err(InCode::TableSkew.val());
        }
        seen |= r.bit;
        digest = print_word(digest, r.bit as u64);
        k += 1;
    }
    if seen != EV_MASK_ALL {
        return Err(InCode::DispatchUnhandled.val());
    }
    Ok(GoldenOut {
        devices: SAMPLE_DEVICES as u32,
        handled,
        bits_checked,
        digest,
        reads: cache.reads(),
        hits: cache.hits(),
    })
}

// ── 注入账（10 轮 × 4 槽）──────────────────────────────────────
/// 注入账：每槽 tries/caught 双计数。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 每槽注入次数。
    pub tries: [u32; 4],
    /// 每槽按预期被拒次数。
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }

    /// 记一笔（槽位越界拒记，返回是否记账）。
    pub fn record(&mut self, slot: usize, caught: bool) -> bool {
        let ok = match self.tries.get_mut(slot) {
            Some(t) => {
                *t += 1;
                true
            }
            None => false,
        };
        if ok && caught {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
        ok
    }

    /// 四槽是否「注入即拒」全中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || t != c {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入：未知族 / 能力未声明 / 转储截短 / 保留位置位 四槽 × rounds。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let specs = device_specs();
    let keyboard = match specs.first() {
        Some(s) => s,
        None => return Err(InCode::TableSkew.val()),
    };
    let mouse = match specs.get(2) {
        Some(s) => s,
        None => return Err(InCode::TableSkew.val()),
    };
    let kbd_caps = keyboard.to_caps();
    let mouse_caps = mouse.to_caps();
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：伪造族号 0x99，必须 UnknownType。
        let bogus = InputEvent::new(1, 0x99, 0x01, 1);
        led.record(0, bogus.family().is_err());

        // 槽 1：鼠标无 EV_ABS，ABS_X 必须 CapabilityDenied。
        let abs_ev = InputEvent::new(2, EV_ABS, 0x00, 128);
        let denied = match dispatch(&abs_ev, &mouse_caps) {
            Err(c) => c == InCode::CapabilityDenied.val(),
            Ok(_) => false,
        };
        led.record(1, denied);

        // 槽 2：转储截短一字节，必须 BadDumpLen。
        let mut short = keyboard.dump();
        short.pop();
        let bad_len = match CapBits::decode(&short) {
            Err(c) => c == InCode::BadDumpLen.val(),
            Ok(_) => false,
        };
        led.record(2, bad_len);

        // 槽 3：REL 保留码 0x10 置位，必须 ReservedBitSet。
        let mut dirty = CapBits::empty();
        dirty.set_ev(EvFamily::Rel);
        dirty.force_code(EvFamily::Rel, 0x10);
        let reserved = match CapBits::decode(&dirty.encode()) {
            Err(c) => c == InCode::ReservedBitSet.val(),
            Ok(_) => false,
        };
        led.record(3, reserved);

        // 键盘位图必须始终可解（正向对照，防止「全拒」实现假绿）。
        if kbd_caps.count_codes(EvFamily::Key) == 0 {
            return Err(InCode::DigestMismatch.val());
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────
/// 探针 A：键盘位图解码后 A/左 Ctrl 必须置位，且无 REL 族。
pub fn probe_seq_a() -> Result<(), u32> {
    let specs = device_specs();
    let kb = match specs.first() {
        Some(s) => s,
        None => return Err(InCode::TableSkew.val()),
    };
    let caps = CapBits::decode(&kb.dump())?;
    if !caps.has_code(EvFamily::Key, 0x1E) {
        return Err(InCode::DigestMismatch.val());
    }
    if !caps.has_code(EvFamily::Key, 0x1D) {
        return Err(InCode::DigestMismatch.val());
    }
    if caps.has_ev(EvFamily::Rel) {
        return Err(InCode::DigestMismatch.val());
    }
    Ok(())
}

/// 探针 B：截短转储必须拒（长度闸）。
pub fn probe_seq_b() -> Result<(), u32> {
    let specs = device_specs();
    let kb = match specs.first() {
        Some(s) => s,
        None => return Err(InCode::TableSkew.val()),
    };
    let mut d = kb.dump();
    d.pop();
    match CapBits::decode(&d) {
        Err(c) if c == InCode::BadDumpLen.val() => Ok(()),
        _ => Err(InCode::BadDumpLen.val()),
    }
}

/// 探针 C：未知族与缓存二次读取必须拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let caps = all_caps();
    let bogus = InputEvent::new(0, 0x7F, 0x00, 0);
    match dispatch(&bogus, &caps) {
        Err(c) if c == InCode::UnknownType.val() => {}
        _ => return Err(InCode::UnknownType.val()),
    }
    let mut cache = CapCache::new();
    let specs = device_specs();
    let kb = match specs.first() {
        Some(s) => s,
        None => return Err(InCode::TableSkew.val()),
    };
    cache.read_once(kb.id, &kb.dump())?;
    match cache.read_once(kb.id, &kb.dump()) {
        Err(c) if c == InCode::CacheDoubleRead.val() => Ok(()),
        _ => Err(InCode::CacheDoubleRead.val()),
    }
}

/// 双跑一致性：黄金链两跑同值。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(InCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE+1, SEG_BASE+0x20)。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < InCode::ALL.len() {
        match InCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v <= SEG_BASE || v >= SEG_BASE + 0x20 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据收束 ──────────────────────────────────────────────────
/// 十族分发表逐族处理位（F48801-J1 的可判形态：10 位全置）。
pub fn family_bits() -> u32 {
    let mut bits = 0u32;
    let mut i = 0usize;
    while i < DISPATCH_TABLE.len() {
        if let Some(d) = DISPATCH_TABLE.get(i) {
            if d.handled {
                bits |= d.fam.handle_bit();
            }
        }
        i += 1;
    }
    bits
}

/// 十族代表事件逐族路由（返回按族序的命中位）。
pub fn route_sweep() -> Result<u32, u32> {
    let caps = all_caps();
    let evs = representative_events();
    let mut bits = 0u32;
    let mut i = 0usize;
    while i < evs.len() {
        if let Some(ev) = evs.get(i) {
            let r = dispatch(ev, &caps)?;
            bits |= r.bit;
        }
        i += 1;
    }
    Ok(bits)
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48801-J1：十族分发表 ×10 逐族处理位。
    let bits = family_bits();
    v.push((
        "F48801-J1-十族处理位10/10",
        bits == EV_MASK_ALL && table_check().is_ok(),
        format!("bits=0x{:03X} 应=0x{:03X}", bits, EV_MASK_ALL),
    ));

    // F48801-J1：十族代表事件逐族路由零串族。
    let sweep = route_sweep();
    let sweep_ok = match &sweep {
        Ok(b) => *b == EV_MASK_ALL,
        Err(_) => false,
    };
    v.push((
        "F48801-J1-十族代表路由零串族",
        sweep_ok,
        format!("sweep={:?}", sweep),
    ));

    // F48801-J1：未知族拒绝（不放行、不猜默认族）。
    let caps = all_caps();
    let bogus = InputEvent::new(0, 0x99, 0x00, 0);
    v.push((
        "F48801-J1-未知族拒绝",
        dispatch(&bogus, &caps).is_err() && probe_seq_c().is_ok(),
        String::from("0x99 → UnknownType"),
    ));

    // F48802-J1：位图读取 ×10 设备与实物逐位一致。
    let specs = device_specs();
    let mut exact = true;
    let mut checked = 0u32;
    let mut i = 0usize;
    while i < specs.len() {
        if let Some(s) = specs.get(i) {
            match CapBits::decode(&s.dump()) {
                Ok(c) => match s.verify(&c) {
                    Ok(n) => checked += n,
                    Err(_) => exact = false,
                },
                Err(_) => exact = false,
            }
        }
        i += 1;
    }
    v.push((
        "F48802-J1-位图10设备逐位一致",
        exact && specs.len() == SAMPLE_DEVICES,
        format!("台数={} 位数={}", specs.len(), checked),
    ));

    // F48802-J1：一次读取 + 缓存命中（读 10 次、命中 10 次，不多读）。
    let mut cache = CapCache::new();
    let mut reads_ok = true;
    let mut j = 0usize;
    while j < specs.len() {
        if let Some(s) = specs.get(j) {
            if cache.read_once(s.id, &s.dump()).is_err() {
                reads_ok = false;
            }
            if cache.cached(s.id).is_err() {
                reads_ok = false;
            }
        }
        j += 1;
    }
    let double = match specs.first() {
        Some(s) => cache.read_once(s.id, &s.dump()).is_err(),
        None => false,
    };
    v.push((
        "F48802-J1-一次读取缓存命中",
        reads_ok && double && cache.reads() == SAMPLE_DEVICES as u32,
        format!("reads={} hits={} 二次读拒={}", cache.reads(), cache.hits(), double),
    ));

    // F48802-J1：保留位/截短转储拒绝。
    let mut dirty = CapBits::empty();
    dirty.set_ev(EvFamily::Rel);
    dirty.force_code(EvFamily::Rel, 0x10);
    let reserved_ok = match CapBits::decode(&dirty.encode()) {
        Err(c) => c == InCode::ReservedBitSet.val(),
        Ok(_) => false,
    };
    v.push((
        "F48802-J1-保留位与截短双拒",
        reserved_ok && probe_seq_b().is_ok(),
        format!("reserved={} short={}", reserved_ok, probe_seq_b().is_ok()),
    ));

    // F48802-J1：EVIOCGBIT 编码十族自洽。
    v.push((
        "F48802-J1-EVIOCGBIT编码自洽",
        eviocgbit_selfcheck().is_ok(),
        format!("KEY io=0x{:08X}", eviocgbit_ioctl(EvFamily::Key)),
    ));

    // F48801-J2：注入 10 轮 × 4 槽全中。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F48801-J2-注入10轮4槽全中",
        led_ok,
        format!(
            "tries={:?} caught={:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));

    // F48801-J3：双跑一致。
    v.push((
        "F48801-J3-双跑一致",
        repro_check().is_ok(),
        format!("golden={:?}", golden_run().map(|g| g.digest)),
    ));

    // F48801-J4：段位守卫。
    v.push((
        "F48801-J4-段位守卫",
        segment_guard() && detail(InCode::CacheMiss.val()) == InCode::CacheMiss.detail(),
        String::from("0x70001..0x7000C"),
    ));

    v
}

pub fn run_unx48801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48801 统一输入事件层总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_families_are_closed_and_unique() {
        assert_eq!(EvFamily::ALL.len(), FAMILY_COUNT);
        let mut seen = 0u32;
        for f in EvFamily::ALL.iter() {
            assert_eq!(EvFamily::from_code(f.code()), Some(*f));
            assert_eq!(EvFamily::from_slot(f.slot()), Some(*f));
            assert_eq!(seen & f.handle_bit(), 0, "槽位重复");
            seen |= f.handle_bit();
        }
        assert_eq!(seen, EV_MASK_ALL);
        assert_eq!(EvFamily::from_code(0x06), None);
        assert_eq!(EvFamily::from_slot(10), None);
    }

    #[test]
    fn three_primary_families_exact() {
        let primary: Vec<u16> = EvFamily::ALL
            .iter()
            .filter(|f| f.primary())
            .map(|f| f.code())
            .collect();
        assert_eq!(primary, vec![EV_KEY, EV_REL, EV_ABS]);
    }

    #[test]
    fn dispatch_table_ten_handled_bits() {
        assert_eq!(table_check().ok(), Some(EV_MASK_ALL));
        assert_eq!(family_bits(), EV_MASK_ALL);
        for d in DISPATCH_TABLE.iter() {
            assert!(d.handled);
            assert!(d.consumers >= 1);
            assert!(!d.note.is_empty());
        }
    }

    #[test]
    fn representative_route_no_cross_family() {
        let caps = all_caps();
        for ev in representative_events().iter() {
            let r = dispatch(ev, &caps).expect("代表事件必须可路由");
            assert_eq!(r.fam.code(), ev.kind);
            assert_eq!(r.bit, r.fam.handle_bit());
            assert_eq!(r.primary, matches!(ev.kind, EV_KEY | EV_REL | EV_ABS));
        }
    }

    #[test]
    fn unknown_type_rejected_before_capability() {
        let caps = CapBits::empty();
        let ev = InputEvent::new(0, 0x42, 0, 0);
        assert_eq!(dispatch(&ev, &caps).err(), Some(InCode::UnknownType.val()));
    }

    #[test]
    fn capability_gate_precedes_code_gate() {
        let specs = device_specs();
        let mouse = specs.get(2).expect("鼠标画像");
        let caps = mouse.to_caps();
        let abs = InputEvent::new(0, EV_ABS, 0x00, 0);
        assert_eq!(dispatch(&abs, &caps).err(), Some(InCode::CapabilityDenied.val()));
        let rel_missing = InputEvent::new(0, EV_REL, 0x0F, 0);
        assert_eq!(dispatch(&rel_missing, &caps).err(), Some(InCode::UnknownCode.val()));
    }

    #[test]
    fn syn_uses_closed_set_not_bitmap() {
        let caps = CapBits::empty();
        let syn = InputEvent::new(0, EV_SYN, SYN_REPORT, 0);
        // 未声明 EV_SYN → 能力拒。
        assert_eq!(dispatch(&syn, &caps).err(), Some(InCode::CapabilityDenied.val()));
        let caps = all_caps();
        assert!(dispatch(&syn, &caps).is_ok());
        let bad = InputEvent::new(0, EV_SYN, 0x10, 0);
        assert_eq!(dispatch(&bad, &caps).err(), Some(InCode::UnknownCode.val()));
    }

    #[test]
    fn dump_is_fixed_length_and_roundtrips() {
        for s in device_specs().iter() {
            let d = s.dump();
            assert_eq!(d.len(), DUMP_LEN);
            let c = CapBits::decode(&d).expect("位图必须可解");
            assert_eq!(c.digest(), s.to_caps().digest());
            assert_eq!(c.encode(), d);
        }
        let mut short = device_specs().first().expect("首台").dump();
        short.pop();
        assert_eq!(CapBits::decode(&short).err(), Some(InCode::BadDumpLen.val()));
    }

    #[test]
    fn eviocgbit_encoding_ten_families() {
        assert!(eviocgbit_selfcheck().is_ok());
        let io = eviocgbit_ioctl(EvFamily::Key);
        assert_eq!(eviocgbit_nr(io), 0x21);
        assert_eq!((io >> 8) & 0xFF, IOC_EV_MAGIC);
        assert_eq!(eviocgbit_size(io), 96);
        assert_eq!(eviocgbit_ioctl(EvFamily::Rel) & 0xFF, 0x22);
    }

    #[test]
    fn golden_run_is_stable_and_complete() {
        let a = golden_run().expect("黄金链必须通过");
        let b = golden_run().expect("黄金链二跑");
        assert_eq!(a, b);
        assert_eq!(a.devices, SAMPLE_DEVICES as u32);
        assert_eq!(a.handled, EV_MASK_ALL);
        assert_eq!(a.reads, SAMPLE_DEVICES as u32);
        assert_eq!(a.hits, SAMPLE_DEVICES as u32);
        assert!(a.bits_checked > 0);
    }

    #[test]
    fn inject_four_slots_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).expect("注入必须可跑");
        assert!(led.all_caught());
        assert_eq!(led.tries, [JUDGE_ROUNDS; 4]);
        assert_eq!(led.caught, [JUDGE_ROUNDS; 4]);
    }

    #[test]
    fn cache_evict_and_miss() {
        let specs = device_specs();
        let kb = specs.first().expect("键盘画像");
        let mut cache = CapCache::new();
        assert_eq!(cache.cached(kb.id).err(), Some(InCode::CacheMiss.val()));
        assert!(cache.read_once(kb.id, &kb.dump()).is_ok());
        assert!(cache.evict(kb.id));
        assert!(cache.is_empty());
        assert_eq!(cache.reads(), 1);
    }

    #[test]
    fn codes_are_segment_closed() {
        assert!(segment_guard());
        let mut seen = 0u32;
        for c in InCode::ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE && v < SEG_BASE + 0x20);
            assert_eq!(detail(v), c.detail());
            seen += 1;
            assert!(!c.detail().is_empty());
        }
        assert_eq!(seen, InCode::ALL.len() as u32);
        assert_eq!(detail(0x7000F), "未知诊断码：不在 0x70001..0x7000C 闭集内");
    }

    #[test]
    fn per_family_code_counts_match_specs() {
        let specs = device_specs();
        let pad = specs.get(3).expect("触控板");
        let caps = pad.to_caps();
        assert_eq!(caps.count_codes(EvFamily::Key), pad.keys.len() as u32);
        assert_eq!(caps.count_codes(EvFamily::Abs), pad.abss.len() as u32);
        assert_eq!(caps.count_codes(EvFamily::Ff), 0);
    }
}
