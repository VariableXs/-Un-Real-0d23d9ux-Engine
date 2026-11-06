//! UNX-F49009 quirk 表 fuzz + UNX-F49010 配置面错误路径 · 0x719C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-E 首槽；任务板 J1 引用按原文保留，
//! 不把 F49009 卡面引用的 F49010-J1 改写成 F49009-J1，也不把 F49010 卡面
//! 引用的 F49011-J1 改写成 F49010-J1，以便任务板可逐字回查）：
//!   UNX-F49009 quirk 表 fuzz（乱表项/乱固件版本注入不崩）:
//!           ① 纯功能代码 ≥300 行；
//!           ② 判据原文：UNX-F49010-J1 损坏注入 ×10 回退告警全过；判据锚定 体验九；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49010 配置面错误路径（损坏配置回退默认+告警可查）:
//!           ① 纯功能代码 ≥280 行；
//!           ② 判据原文：UNX-F49011-J1 三态 ×10^2 计数一致；判据锚定 体验十三；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（表 fuzz + 配置错误路径）：
//!   分类判据定序 → 乱表项分类 → 独立复算 → 固件窗口查询 → 配置 blob 解码 →
//!   语义兜底 → 回退落账 → 告警可查 → 账本渲染。
//! 分类判据定序先于分类：「厂商 0 / 机型 0 / 区间倒置 / 参数越界 / 标志越界」
//! 五条规则必须有唯一的先后。同一个乱表项若同时踩三条规则，先报哪条决定了
//! 计数器落在哪个格；不定序的话 10^4 轮的分类合计对不上行数，fuzz 的守恒式
//! 本身就是坏的。
//! 乱表项分类先于独立复算：分类是主路径，复算是旁证。先复算再分类，复算
//! 失败时主路径还没跑过，账本里没有分母，只能记成「未知」——fuzz 最怕的就
//! 是把失败记成未知。
//! 独立复算先于固件窗口查询：表项本身不成立时，拿它的固件区间去查窗口只是
//! 把坏数据传播到第二组计数。先确认「这一项是不是可接受的表项」，再问「这个
//! 固件版本落在窗口哪一格」。
//! 窗口查询先于 blob 解码：两组 fuzz 共用一本账、一个 LCG 种子，先跑完表侧
//! 再进配置侧，失败的归属才唯一。反序会让配置侧的红污染表侧的计数，10^4 的
//! 分母变得不可解释。
//! blob 解码先于语义兜底：魔数/版本/长度/校验和证明「这段字节是它自称的那
//! 段字节」，轴档位与跨轴互斥证明「这段字节的语义成立」。先语义后结构，
//! 一个随机字节串可能碰巧是合法三档，被当成配置接受。
//! 语义兜底先于回退落账：解码失败必须先在默认配置上落地，再记账。顺序反过
//! 来（先记「已回退」再取默认），一旦默认值本身被改，账本会声称回退过而
//! 实际拿到的是半截配置。
//! 回退落账先于告警可查：告警是账本的投影，不是账本的替代。先写告警再落账，
//! 告警可以指向一条不存在的回退；回退先落，告警才有可查的序号与码值。
//! 告警可查先于渲染：渲染行只带聚合数字，具体是哪一种损坏必须能按码检索。
//! 只有聚合没有检索，十次注入全捕获也只是十个布尔，运维拿到行却查不到因。
//! 段位守卫最后：14 个诊断码必须落在 [0x719C1, 0x719CE]。窗口漂移会把前八项
//! 闸门的失败记成「未知诊断码」，红变灰。
//! 关键差异：本模块不做 quirk 匹配与 T1 承诺面（B11-B 的 F49003/F49004 承担），
//! 不做触控板三档面板与热生效（B11-C 的 F49005/F49006 承担），不做多用户隔离
//! 预告与表版本裁决（B11-D 的 F49007/F49008 承担）。F49011-J1「三态 ×10^2
//! 计数一致」的命中/未命中/降级三态账在 B11-F，本模块只逐字保留该引用；
//! 本模块自己的三格是「窗口命中 / 窗口间隙 / 越界」，与 B11-F 的三态账不是
//! 同一本账，不作替代。
//! 本模块只回答两问：
//!   ① 10^4 轮乱表项 + 乱固件版本注入下，是否零崩溃、每个输入都落进一个
//!      有名字的格子、且分类合计与轮数严格守恒；
//!   ② 配置 blob 被损坏时，是否 **先结构后语义** 地拒绝、回退到默认配置、
//!      并把这一种损坏以可检索的告警码落进账本（十种损坏逐种检出）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x719C0，VX6 批 B11-E 首槽）。
pub const SEG_BASE: u32 = 0x719C0;

/// fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 轮数上界闸门。
pub const SCENARIO_MAX: u32 = 40_000;
/// 表项分类数。
pub const CLASSES: usize = 6;
/// 固件窗口数。
pub const WINDOWS: usize = 6;
/// 损坏注入路数（判据 ×10）。
pub const CORRUPTION_ROUTES: usize = 10;
/// 告警码数。
pub const WARN_CODES: usize = 8;
/// 轴数。
pub const AXES: usize = 3;
/// 每轴档位数。
pub const LEVELS: u8 = 3;
/// 合法配置组合数（27 − 3 条互斥）。
pub const VALID_CONFIGS: u32 = 24;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 账本字段数。
pub const STATS_FIELDS: usize = 12;

/// 表项厂商上限。
pub const VENDOR_MAX: u16 = 0xFFFF;
/// 表项参数上限。
pub const PARAM_MAX: u16 = 0x00FF;
/// 表项标志合法掩码。
pub const FLAGS_MASK: u8 = 0x0F;
/// 固件版本下界。
pub const FW_MIN: u32 = 0x0000_0100;
/// 固件版本上界。
pub const FW_MAX: u32 = 0x00FF_FFFF;
/// 配置 blob 魔数（"F490"）。
pub const MAGIC: u32 = 0x4634_3930;
/// 配置 blob 版本。
pub const BLOB_VERSION: u8 = 1;
/// 配置主体长度。
pub const BODY_LEN: u8 = 3;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    /// fuzz 账不守恒（分类合计 ≠ 轮数）
    FuzzBad,
    /// 固件窗口查询三格计数不守恒
    LookupBad,
    /// 配置 blob 结构不成立（魔数/版本/长度）
    BlobBad,
    /// blob 校验和不符
    ChecksumBad,
    /// 轴档位越界
    AxisBad,
    /// 跨轴互斥（重力度 × 迟钝增益）
    ComboBad,
    /// 告警账缺失或不可按码检索
    WarnBad,
    /// 账本不守恒
    LedgerBad,
    /// 复跑摘要不一致
    ReplayBad,
    /// 注入未全捕获
    InjectBad,
    /// 渲染不可解析
    RenderBad,
    /// 覆盖轮数越界
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// 主分类与独立复算不一致
    DriftBad,
}

impl FzCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            FzCode::FuzzBad => "fuzz 分类合计与轮数不等，或有输入没落进任何一格",
            FzCode::LookupBad => "窗口命中/间隙/越界三格合计与查询数不等",
            FzCode::BlobBad => "配置 blob 的魔数/版本/长度与其自称不符",
            FzCode::ChecksumBad => "配置 blob 校验和与内容不符",
            FzCode::AxisBad => "轴档位越出 0..2 的取值域",
            FzCode::ComboBad => "重力度档与迟钝增益互斥（阈值 80 配增益 40）",
            FzCode::WarnBad => "告警账缺失，或告警码无法按码检索",
            FzCode::LedgerBad => "账本不守恒（接受/拒绝/回退计数断裂）",
            FzCode::ReplayBad => "两次确定性复跑摘要不一致",
            FzCode::InjectBad => "损坏注入轮次未被全部捕获",
            FzCode::RenderBad => "渲染行字段数或数值不可解析",
            FzCode::CoverBad => "覆盖轮数为零或越过上限",
            FzCode::SegmentBad => "诊断码越出本模块段窗",
            FzCode::DriftBad => "主分类与独立复算给出的判定不一致",
        }
    }

    pub const ALL: [FzCode; 14] = [
        FzCode::FuzzBad,
        FzCode::LookupBad,
        FzCode::BlobBad,
        FzCode::ChecksumBad,
        FzCode::AxisBad,
        FzCode::ComboBad,
        FzCode::WarnBad,
        FzCode::LedgerBad,
        FzCode::ReplayBad,
        FzCode::InjectBad,
        FzCode::RenderBad,
        FzCode::CoverBad,
        FzCode::SegmentBad,
        FzCode::DriftBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FzCode::ALL.len() {
        if let Some(c) = FzCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 表项 fuzz ───────────────────────────────────────────────────
/// 一条被 fuzz 的表项（三段式：机型 / 固件区间 / 参数覆盖）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzEntry {
    pub vendor: u16,
    pub model: u16,
    pub fw_lo: u32,
    pub fw_hi: u32,
    pub param: u16,
    pub flags: u8,
}

/// 表项分类：六个有名字的格子，任何输入必落其一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzClass {
    /// 可接受表项
    Accepted,
    /// 厂商为 0
    VendorZero,
    /// 机型为 0
    ModelZero,
    /// 固件区间倒置或越界
    RangeBad,
    /// 参数为 0 或越界
    ParamBad,
    /// 标志位越出合法掩码
    FlagsBad,
}

impl FuzzClass {
    pub fn index(self) -> usize {
        match self {
            FuzzClass::Accepted => 0,
            FuzzClass::VendorZero => 1,
            FuzzClass::ModelZero => 2,
            FuzzClass::RangeBad => 3,
            FuzzClass::ParamBad => 4,
            FuzzClass::FlagsBad => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FuzzClass::Accepted => "accepted",
            FuzzClass::VendorZero => "vendor-zero",
            FuzzClass::ModelZero => "model-zero",
            FuzzClass::RangeBad => "range-bad",
            FuzzClass::ParamBad => "param-bad",
            FuzzClass::FlagsBad => "flags-bad",
        }
    }

    pub const ALL: [FuzzClass; CLASSES] = [
        FuzzClass::Accepted,
        FuzzClass::VendorZero,
        FuzzClass::ModelZero,
        FuzzClass::RangeBad,
        FuzzClass::ParamBad,
        FuzzClass::FlagsBad,
    ];
}

/// 主分类：判定次序被本节固定（厂商 → 机型 → 区间 → 参数 → 标志）。
pub fn classify(e: &FuzzEntry) -> FuzzClass {
    if e.vendor == 0 {
        return FuzzClass::VendorZero;
    }
    if e.model == 0 {
        return FuzzClass::ModelZero;
    }
    if e.fw_lo < FW_MIN || e.fw_lo > e.fw_hi || e.fw_hi > FW_MAX {
        return FuzzClass::RangeBad;
    }
    if e.param == 0 || e.param > PARAM_MAX {
        return FuzzClass::ParamBad;
    }
    if e.flags & !FLAGS_MASK != 0 {
        return FuzzClass::FlagsBad;
    }
    FuzzClass::Accepted
}

/// 独立复算：写法与主分类不同（布尔合取而非短路链），只回答「是否可接受」。
pub fn accept_recheck(e: &FuzzEntry) -> bool {
    let vendor_ok = !(e.vendor == 0);
    let model_ok = !(e.model == 0);
    let range_ok = !(e.fw_lo < FW_MIN || e.fw_lo > e.fw_hi || e.fw_hi > FW_MAX);
    let param_ok = !(e.param == 0 || e.param > PARAM_MAX);
    let flags_ok = !(e.flags & !FLAGS_MASK != 0);
    vendor_ok && model_ok && range_ok && param_ok && flags_ok
}

/// 高位取模：LCG 的低位周期极短（bit0 恒翻转），分类抽样必须走高位取模，
/// 否则「厂商为 0」这一格在 10^4 轮里可能一次都抽不到，覆盖断言假红。
fn roll(rng: &mut p_core::Lcg, n: u32) -> u32 {
    if n == 0 {
        0
    } else {
        ((rng.next() >> 24) % u64::from(n)) as u32
    }
}

/// 一条乱表项的生成器：刻意让五条规则都有可观的比例被踩到。
pub fn fuzz_entry(rng: &mut p_core::Lcg) -> FuzzEntry {
    let vendor = if roll(rng, 8) == 0 {
        0
    } else {
        (roll(rng, 0x2000) as u16) | 1
    };
    let model = if roll(rng, 10) == 0 {
        0
    } else {
        roll(rng, 0x2000) as u16
    };
    let lo_raw = roll(rng, 0x0002_0000);
    let fw_lo = if roll(rng, 12) == 0 {
        lo_raw % FW_MIN
    } else {
        lo_raw
    };
    let fw_hi = match roll(rng, 6) {
        // 区间倒置。
        0 => fw_lo.wrapping_sub(roll(rng, 0x100) + 1),
        // 上界越界。
        1 => FW_MAX.saturating_add(roll(rng, 0x1000)),
        // 正常闭区间。
        _ => fw_lo.saturating_add(roll(rng, 0x8000)),
    };
    let param = if roll(rng, 9) == 0 {
        0
    } else if roll(rng, 11) == 0 {
        PARAM_MAX.saturating_add(roll(rng, 0x100) as u16)
    } else {
        (roll(rng, 0x100) as u16) | 1
    };
    let flags = if roll(rng, 7) == 0 {
        (roll(rng, 0x100) as u8) | 0x10
    } else {
        (roll(rng, 0x10)) as u8
    };
    FuzzEntry {
        vendor,
        model,
        fw_lo,
        fw_hi,
        param,
        flags,
    }
}

// ── 固件窗口查询（三格：命中 / 间隙 / 越界） ─────────────────────
/// 六个固件窗口（闭区间）。
pub const FW_WINDOWS: [(u32, u32); WINDOWS] = [
    (0x0001_0000, 0x0001_0FFF),
    (0x0002_0000, 0x0002_0FFF),
    (0x0003_0000, 0x0003_4FFF),
    (0x0005_0000, 0x0005_FFFF),
    (0x0008_0000, 0x0008_FFFF),
    (0x0010_0000, 0x0010_01FF),
];

/// 查询结果：命中窗口下标 / 早于全部窗口 / 晚于全部窗口 / 落在窗口间隙。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lookup {
    Hit(u8),
    TooOld,
    TooNew,
    Gap,
}

impl Lookup {
    pub fn slot(self) -> usize {
        match self {
            Lookup::Hit(_) => 0,
            Lookup::TooOld => 1,
            Lookup::TooNew => 2,
            Lookup::Gap => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lookup::Hit(_) => "hit",
            Lookup::TooOld => "too-old",
            Lookup::TooNew => "too-new",
            Lookup::Gap => "gap",
        }
    }
}

/// 固件版本查询：窗口按升序固化，故「早/晚/间隙」三格可判。
pub fn lookup(fw: u32) -> Lookup {
    let mut i = 0usize;
    while i < FW_WINDOWS.len() {
        if let Some((lo, hi)) = FW_WINDOWS.get(i) {
            if fw >= *lo && fw <= *hi {
                return Lookup::Hit(i as u8);
            }
        }
        i += 1;
    }
    let first = match FW_WINDOWS.first() {
        Some(v) => *v,
        None => return Lookup::TooOld,
    };
    let last = match FW_WINDOWS.get(FW_WINDOWS.len().saturating_sub(1)) {
        Some(v) => *v,
        None => return Lookup::TooNew,
    };
    if fw < first.0 {
        return Lookup::TooOld;
    }
    if fw > last.1 {
        return Lookup::TooNew;
    }
    Lookup::Gap
}

/// 窗口表自审：下界升序、每窗 lo ≤ hi、窗口之间不重叠。
pub fn windows_ok() -> bool {
    let mut i = 0usize;
    while i < FW_WINDOWS.len() {
        let a = match FW_WINDOWS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.0 > a.1 {
            return false;
        }
        if i > 0 {
            if let Some(p) = FW_WINDOWS.get(i - 1) {
                if a.0 <= p.1 {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

// ── fuzz 账 ─────────────────────────────────────────────────────
/// fuzz 账：表侧六格 + 窗口侧四格 + 独立复算漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzLedger {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub classes: [u32; CLASSES],
    pub hits: u32,
    pub gaps: u32,
    pub too_old: u32,
    pub too_new: u32,
    pub drift: u32,
    pub digest: u64,
}

impl FuzzLedger {
    pub const fn new() -> FuzzLedger {
        FuzzLedger {
            rounds: 0,
            accepted: 0,
            rejected: 0,
            classes: [0; CLASSES],
            hits: 0,
            gaps: 0,
            too_old: 0,
            too_new: 0,
            drift: 0,
            digest: 0,
        }
    }

    /// 守恒：接受 + 拒绝 = 轮数；六个分类合计 = 轮数；窗口四格合计 = 轮数。
    pub fn conserved(&self) -> bool {
        let mut cls = 0u32;
        let mut i = 0usize;
        while i < self.classes.len() {
            if let Some(v) = self.classes.get(i) {
                cls = cls.saturating_add(*v);
            }
            i += 1;
        }
        let windows = self
            .hits
            .saturating_add(self.gaps)
            .saturating_add(self.too_old)
            .saturating_add(self.too_new);
        self.accepted.saturating_add(self.rejected) == self.rounds
            && cls == self.rounds
            && windows == self.rounds
            && self.drift == 0
    }

    /// 覆盖：六格与窗口四格都被踩到过（否则 fuzz 只是十次重复）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < self.classes.len() {
            match self.classes.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        self.hits > 0 && self.gaps > 0 && self.too_old > 0 && self.too_new > 0
    }
}

/// 10^4 轮 fuzz：乱表项 + 乱固件版本，全部落格、零崩溃、独立复算。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzLedger, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(FzCode::CoverBad.val());
    }
    if !windows_ok() {
        return Err(FzCode::LookupBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_9E01);
    let mut lg = FuzzLedger::new();
    let mut digest = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let e = fuzz_entry(&mut rng);
        let cls = classify(&e);
        if let Some(slot) = lg.classes.get_mut(cls.index()) {
            *slot = slot.saturating_add(1);
        } else {
            return Err(FzCode::FuzzBad.val());
        }
        if cls == FuzzClass::Accepted {
            lg.accepted = lg.accepted.saturating_add(1);
        } else {
            lg.rejected = lg.rejected.saturating_add(1);
        }
        // 独立复算：主分类说可接受 ⇔ 复算说可接受。
        if accept_recheck(&e) != (cls == FuzzClass::Accepted) {
            lg.drift = lg.drift.saturating_add(1);
        }
        // 乱固件版本：从表项区间附近取值再打散到全域。
        let fw = e
            .fw_lo
            .wrapping_add(e.fw_hi)
            .wrapping_add(rng.below(0x0020_0000));
        match lookup(fw) {
            Lookup::Hit(_) => lg.hits = lg.hits.saturating_add(1),
            Lookup::Gap => lg.gaps = lg.gaps.saturating_add(1),
            Lookup::TooOld => lg.too_old = lg.too_old.saturating_add(1),
            Lookup::TooNew => lg.too_new = lg.too_new.saturating_add(1),
        }
        digest = p_core::fold_u32(digest, u32::from(e.vendor));
        digest = p_core::fold_u32(digest, u32::from(e.model));
        digest = p_core::fold_u32(digest, e.fw_lo);
        digest = p_core::fold_u32(digest, e.fw_hi);
        digest = p_core::fold_u32(digest, u32::from(e.param));
        digest = p_core::fold_byte(digest, e.flags);
        digest = p_core::fold_u32(digest, cls.index() as u32);
        digest = p_core::fold_u32(digest, lookup(fw).slot() as u32);
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.digest = digest;
    if !lg.conserved() {
        return Err(FzCode::FuzzBad.val());
    }
    Ok(lg)
}

/// fuzz 复跑：两轮摘要逐位相同。
pub fn fuzz_replay(rounds: u32) -> bool {
    match (fuzz_rounds(rounds), fuzz_rounds(rounds)) {
        (Ok(a), Ok(b)) => a.digest == b.digest && a.digest != 0,
        _ => false,
    }
}

/// 分类次序反例：同一表项同时踩多条规则时，必须先报厂商、再报机型、再报区间。
pub fn class_precedence_ok() -> bool {
    let e = FuzzEntry {
        vendor: 0,
        model: 0,
        fw_lo: 0,
        fw_hi: 0,
        param: 0,
        flags: 0xF0,
    };
    if classify(&e) != FuzzClass::VendorZero {
        return false;
    }
    let e2 = FuzzEntry {
        vendor: 1,
        model: 0,
        fw_lo: 0,
        fw_hi: 0,
        param: 0,
        flags: 0xF0,
    };
    if classify(&e2) != FuzzClass::ModelZero {
        return false;
    }
    let e3 = FuzzEntry {
        vendor: 1,
        model: 1,
        fw_lo: 9,
        fw_hi: 3,
        param: 0,
        flags: 0xF0,
    };
    if classify(&e3) != FuzzClass::RangeBad {
        return false;
    }
    let e4 = FuzzEntry {
        vendor: 1,
        model: 1,
        fw_lo: FW_MIN,
        fw_hi: FW_MIN,
        param: 0,
        flags: 0xF0,
    };
    if classify(&e4) != FuzzClass::ParamBad {
        return false;
    }
    let e5 = FuzzEntry {
        vendor: 1,
        model: 1,
        fw_lo: FW_MIN,
        fw_hi: FW_MIN,
        param: 1,
        flags: 0xF0,
    };
    if classify(&e5) != FuzzClass::FlagsBad {
        return false;
    }
    let e6 = FuzzEntry {
        vendor: 1,
        model: 1,
        fw_lo: FW_MIN,
        fw_hi: FW_MIN,
        param: 1,
        flags: 0x0F,
    };
    classify(&e6) == FuzzClass::Accepted
}

// ── 配置 blob（结构 → 语义 → 回退） ─────────────────────────────
/// 一份三轴配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cfg {
    pub sens: u8,
    pub dir: u8,
    pub press: u8,
}

/// 出厂默认：标准灵敏度 / 自然方向 / 中力度。
pub const DEFAULT_CFG: Cfg = Cfg {
    sens: 1,
    dir: 1,
    press: 1,
};

/// 告警码：八种可检索的损坏类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WarnCode {
    /// blob 魔数不符
    Magic,
    /// blob 版本不符
    Version,
    /// blob 长度不符
    Length,
    /// blob 校验和不符
    Checksum,
    /// blob 被截断（长度小于主体）
    Truncated,
    /// blob 全零
    Empty,
    /// 轴档位越界
    Axis,
    /// 跨轴互斥
    Combo,
}

impl WarnCode {
    pub fn index(self) -> usize {
        match self {
            WarnCode::Magic => 0,
            WarnCode::Version => 1,
            WarnCode::Length => 2,
            WarnCode::Checksum => 3,
            WarnCode::Truncated => 4,
            WarnCode::Empty => 5,
            WarnCode::Axis => 6,
            WarnCode::Combo => 7,
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            WarnCode::Magic => "配置 blob 魔数不符，按出厂默认回退",
            WarnCode::Version => "配置 blob 版本不符，按出厂默认回退",
            WarnCode::Length => "配置 blob 长度不符，按出厂默认回退",
            WarnCode::Checksum => "配置 blob 校验和不符，按出厂默认回退",
            WarnCode::Truncated => "配置 blob 被截断，按出厂默认回退",
            WarnCode::Empty => "配置 blob 全零，按出厂默认回退",
            WarnCode::Axis => "轴档位越界，按出厂默认回退",
            WarnCode::Combo => "重力度 × 迟钝增益互斥，按出厂默认回退",
        }
    }

    pub const ALL: [WarnCode; WARN_CODES] = [
        WarnCode::Magic,
        WarnCode::Version,
        WarnCode::Length,
        WarnCode::Checksum,
        WarnCode::Truncated,
        WarnCode::Empty,
        WarnCode::Axis,
        WarnCode::Combo,
    ];
}

/// 告警码的自由查询（越界码返回「未知告警码」）。
pub fn warn_detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < WarnCode::ALL.len() {
        if let Some(c) = WarnCode::ALL.get(i) {
            if c.index() as u32 == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知告警码"
}

impl Cfg {
    pub const fn new(sens: u8, dir: u8, press: u8) -> Cfg {
        Cfg { sens, dir, press }
    }

    /// 语义闸门：档位域 → 跨轴互斥。
    pub fn validate(&self) -> Result<(), WarnCode> {
        if self.sens >= LEVELS || self.dir >= LEVELS || self.press >= LEVELS {
            return Err(WarnCode::Axis);
        }
        if self.press == 2 && self.sens == 0 {
            return Err(WarnCode::Combo);
        }
        Ok(())
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_byte(h, self.sens);
        h = p_core::fold_byte(h, self.dir);
        h = p_core::fold_byte(h, self.press);
        h
    }
}

/// 配置 blob：魔数 + 版本 + 长度 + 三字节主体 + 校验和。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfigBlob {
    pub magic: u32,
    pub version: u8,
    pub len: u8,
    pub body: [u8; 3],
    pub checksum: u32,
}

/// blob 校验和：折叠魔数、版本、长度、主体三字节（不含校验和自身）。
pub fn blob_checksum(b: &ConfigBlob) -> u32 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, b.magic);
    h = p_core::fold_byte(h, b.version);
    h = p_core::fold_byte(h, b.len);
    let mut i = 0usize;
    while i < b.body.len() {
        if let Some(v) = b.body.get(i) {
            h = p_core::fold_byte(h, *v);
        }
        i += 1;
    }
    (h & 0xFFFF_FFFF) as u32
}

/// 编码：先校验语义，再封印。
pub fn encode(cfg: &Cfg) -> Result<ConfigBlob, WarnCode> {
    cfg.validate()?;
    let mut b = ConfigBlob {
        magic: MAGIC,
        version: BLOB_VERSION,
        len: BODY_LEN,
        body: [cfg.sens, cfg.dir, cfg.press],
        checksum: 0,
    };
    b.checksum = blob_checksum(&b);
    Ok(b)
}

/// 解码：全零 → 魔数 → 版本 → 长度 → 校验和 → 轴档位 → 跨轴互斥。
pub fn decode(b: &ConfigBlob) -> Result<Cfg, WarnCode> {
    if b.magic == 0 && b.version == 0 && b.len == 0 && b.checksum == 0 {
        let mut all_zero = true;
        let mut i = 0usize;
        while i < b.body.len() {
            if let Some(v) = b.body.get(i) {
                if *v != 0 {
                    all_zero = false;
                }
            }
            i += 1;
        }
        if all_zero {
            return Err(WarnCode::Empty);
        }
    }
    if b.magic != MAGIC {
        return Err(WarnCode::Magic);
    }
    if b.version != BLOB_VERSION {
        return Err(WarnCode::Version);
    }
    if b.len < BODY_LEN {
        return Err(WarnCode::Truncated);
    }
    if b.len != BODY_LEN {
        return Err(WarnCode::Length);
    }
    if blob_checksum(b) != b.checksum || b.checksum == 0 {
        return Err(WarnCode::Checksum);
    }
    let s = match b.body.get(0) {
        Some(v) => *v,
        None => return Err(WarnCode::Truncated),
    };
    let d = match b.body.get(1) {
        Some(v) => *v,
        None => return Err(WarnCode::Truncated),
    };
    let p = match b.body.get(2) {
        Some(v) => *v,
        None => return Err(WarnCode::Truncated),
    };
    let cfg = Cfg::new(s, d, p);
    cfg.validate()?;
    Ok(cfg)
}

/// 加载动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadAction {
    /// 配置成立，采纳
    Kept,
    /// 配置损坏，回退默认
    RolledBack,
}

/// 一次加载的结论：拿到的配置 + 动作 + 告警（回退时必有）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadOutcome {
    pub cfg: Cfg,
    pub action: LoadAction,
    pub warn: Option<WarnCode>,
}

/// 加载：解码失败即回退默认，并把告警带出，绝不半采纳。
pub fn load(b: &ConfigBlob) -> LoadOutcome {
    match decode(b) {
        Ok(cfg) => LoadOutcome {
            cfg,
            action: LoadAction::Kept,
            warn: None,
        },
        Err(w) => LoadOutcome {
            cfg: DEFAULT_CFG,
            action: LoadAction::RolledBack,
            warn: Some(w),
        },
    }
}

/// 合法配置枚举（24 条）。
pub fn valid_configs() -> Vec<Cfg> {
    let mut v: Vec<Cfg> = Vec::new();
    let mut s = 0u8;
    while s < LEVELS {
        let mut d = 0u8;
        while d < LEVELS {
            let mut p = 0u8;
            while p < LEVELS {
                let c = Cfg::new(s, d, p);
                if c.validate().is_ok() {
                    v.push(c);
                }
                p += 1;
            }
            d += 1;
        }
        s += 1;
    }
    v
}

// ── 损坏注入与告警账 ────────────────────────────────────────────
/// 一条告警记录：序号 + 码 + 是否已回退 + 回退后的配置指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WarnRecord {
    pub seq: u32,
    pub code: WarnCode,
    pub rolled_back: bool,
    pub cfg_digest: u64,
}

/// 告警账：可按下标与按码两种方式检索。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WarnLedger {
    pub attempts: u32,
    pub caught: u32,
    pub records: Vec<WarnRecord>,
}

impl WarnLedger {
    pub fn new() -> WarnLedger {
        WarnLedger {
            attempts: 0,
            caught: 0,
            records: Vec::new(),
        }
    }

    pub fn all_caught(&self) -> bool {
        self.attempts > 0 && self.caught == self.attempts
    }

    /// 按告警码检索：同类损坏出现几次。
    pub fn find(&self, code: WarnCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.code == code {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 已出现过的告警码位图（低位起）。
    pub fn codes_seen(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                m |= 1u32 << (r.code.index() as u32);
            }
            i += 1;
        }
        m
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.attempts);
        h = p_core::fold_u32(h, self.caught);
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                h = p_core::fold_u32(h, r.seq);
                h = p_core::fold_u32(h, r.code.index() as u32);
                h = p_core::fold_u32(h, u32::from(r.rolled_back));
            }
            i += 1;
        }
        h
    }
}

/// 重新封印（模拟「改完内容再补校验和」的损坏路径：校验和防手滑不防恶意）。
fn resign(b: &ConfigBlob) -> ConfigBlob {
    let mut out = *b;
    out.checksum = blob_checksum(&out);
    out
}

/// 十种损坏：结构四路、截断/全零两路、语义两路、校验和两路。
pub fn corrupt(b: &ConfigBlob, route: u32) -> ConfigBlob {
    let mut out = *b;
    match route {
        0 => out.magic ^= 1,
        1 => out.version = 2,
        2 => out.len = BODY_LEN + 1,
        3 => out.checksum ^= 1,
        4 => {
            if let Some(slot) = out.body.get_mut(0) {
                *slot = 3;
            }
            out = resign(&out);
        }
        5 => {
            if let Some(slot) = out.body.get_mut(1) {
                *slot = 7;
            }
            out = resign(&out);
        }
        6 => {
            if let Some(slot) = out.body.get_mut(2) {
                *slot = 4;
            }
            out = resign(&out);
        }
        7 => {
            // 重力度 × 迟钝增益：档位都合法，语义不成立。
            out.body = [0, 1, 2];
            out = resign(&out);
        }
        8 => {
            out.len = BODY_LEN - 1;
            out = resign(&out);
        }
        _ => {
            out.magic = 0;
            out.version = 0;
            out.len = 0;
            out.body = [0, 0, 0];
            out.checksum = 0;
        }
    }
    out
}

/// 十轮损坏注入：每种都必须回退到默认并留下可查告警。
pub fn corruption_rounds(rounds: u32) -> Result<WarnLedger, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(FzCode::CoverBad.val());
    }
    let valid = valid_configs();
    if valid.len() as u32 != VALID_CONFIGS {
        return Err(FzCode::LedgerBad.val());
    }
    let mut lg = WarnLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let cfg = match valid.get((r as usize) % valid.len()) {
            Some(v) => *v,
            None => return Err(FzCode::CoverBad.val()),
        };
        let good = match encode(&cfg) {
            Ok(v) => v,
            Err(_) => return Err(FzCode::BlobBad.val()),
        };
        // 干净 blob 必须原样采纳（回退不能误伤好配置）。
        let clean = load(&good);
        if clean.action != LoadAction::Kept || clean.cfg != cfg || clean.warn.is_some() {
            return Err(FzCode::BlobBad.val());
        }
        let bad = corrupt(&good, r % CORRUPTION_ROUTES as u32);
        let out = load(&bad);
        let ok = out.action == LoadAction::RolledBack
            && out.cfg == DEFAULT_CFG
            && out.warn.is_some();
        if ok {
            lg.caught = lg.caught.saturating_add(1);
        }
        if let Some(w) = out.warn {
            lg.records.push(WarnRecord {
                seq: r,
                code: w,
                rolled_back: out.action == LoadAction::RolledBack,
                cfg_digest: out.cfg.digest(),
            });
        }
        lg.attempts = lg.attempts.saturating_add(1);
        r += 1;
    }
    if lg.records.len() != lg.attempts as usize {
        return Err(FzCode::WarnBad.val());
    }
    Ok(lg)
}

/// 反例必拒：六条独立反例（结构、语义、告警各二）。
pub fn tamper_rejected() -> bool {
    let good = match encode(&Cfg::new(2, 0, 1)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    // 1) 魔数不符。
    let mut b = good;
    b.magic = 0x1111_2222;
    let c1 = decode(&b).err() == Some(WarnCode::Magic);
    // 2) 版本不符。
    let mut b = good;
    b.version = 9;
    let c2 = decode(&b).err() == Some(WarnCode::Version);
    // 3) 主体越长（长度字段与主体不符）。
    let mut b = good;
    b.len = 4;
    let c3 = decode(&b).err() == Some(WarnCode::Length);
    // 4) 校验和改一位。
    let mut b = good;
    b.checksum ^= 0x10;
    let c4 = decode(&b).err() == Some(WarnCode::Checksum);
    // 5) 轴档位越界（补校验和也拦不住语义闸门）。
    let mut b = good;
    b.body[0] = 3;
    b = resign(&b);
    let c5 = decode(&b).err() == Some(WarnCode::Axis);
    // 6) 互斥组合（补校验和也拦不住语义闸门）。
    let mut b = good;
    b.body = [0, 2, 2];
    b = resign(&b);
    let c6 = decode(&b).err() == Some(WarnCode::Combo);
    c1 && c2 && c3 && c4 && c5 && c6
}

// ── 段位守卫 + 渲染 ─────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < FzCode::ALL.len() {
        if let Some(c) = FzCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 六字段渲染：tag | params（接受表项数）| schedule（fuzz 轮数）|
/// fails（拒绝表项数）| keys（窗口查询数）| digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn row_fields(row: &str) -> Option<[&str; RENDER_FIELDS]> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let mut out: [&str; RENDER_FIELDS] = [""; RENDER_FIELDS];
    let mut i = 0usize;
    while i < RENDER_FIELDS {
        match parts.get(i).and_then(|x| *x) {
            Some(v) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = v;
                }
            }
            None => return None,
        }
        i += 1;
    }
    Some(out)
}

pub fn audit_render(row: &str) -> bool {
    let parts = match row_fields(row) {
        Some(v) => v,
        None => return false,
    };
    let tag = match parts.get(0) {
        Some(v) if !v.is_empty() => *v,
        _ => return false,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if sp.next().is_some() {
            return false;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    if sp.next().is_some() {
        return false;
    }
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

/// fuzz 账本渲染。
pub fn ledger_row(lg: &FuzzLedger) -> String {
    render_row(
        "F49009",
        lg.accepted,
        lg.rounds,
        lg.rejected,
        lg.rounds,
        lg.digest,
    )
}

/// 渲染自检：行可解析、字段可复算。
pub fn render_selfcheck() -> bool {
    let lg = match fuzz_rounds(1_000) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let row = ledger_row(&lg);
    if !audit_render(&row) {
        return false;
    }
    let parts = match row_fields(&row) {
        Some(v) => v,
        None => return false,
    };
    if parts.get(2).copied() != Some("schedule=1000") {
        return false;
    }
    if parts.get(4).copied() != Some("keys=1000") {
        return false;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49009 ①：fuzz ×10^4 零崩（分类合计守恒 + 六格全覆盖 + 独立复算零漂移）。
    let fz = fuzz_rounds(FUZZ_ROUNDS);
    let fz_ok = match &fz {
        Ok(l) => {
            l.rounds == FUZZ_ROUNDS
                && l.conserved()
                && l.covered()
                && l.drift == 0
                && l.accepted > 0
                && l.rejected > 0
                && l.digest != 0
        }
        Err(_) => false,
    };
    out.push((
        "F49009/fuzz×10^4零崩",
        fz_ok,
        format!(
            "rounds={:?} acc={:?} rej={:?} drift={:?}",
            fz.as_ref().map(|l| l.rounds),
            fz.as_ref().map(|l| l.accepted),
            fz.as_ref().map(|l| l.rejected),
            fz.as_ref().map(|l| l.drift)
        ),
    ));

    // F49009 ②：乱固件版本三格可算（窗口守恒 + 三格非零 + 边界逐点）。
    let lookup_ok = match &fz {
        Ok(l) => {
            l.hits > 0 && l.gaps > 0 && l.too_old > 0 && l.too_new > 0
        }
        Err(_) => false,
    };
    let boundary = lookup(FW_MIN - 1) == Lookup::TooOld
        && lookup(FW_WINDOWS[0].0) == Lookup::Hit(0)
        && lookup(FW_WINDOWS[0].1) == Lookup::Hit(0)
        && lookup(FW_WINDOWS[0].1 + 1) == Lookup::Gap
        && lookup(FW_WINDOWS[WINDOWS - 1].1 + 1) == Lookup::TooNew
        && windows_ok();
    out.push((
        "F49009/固件窗口三格",
        lookup_ok && boundary,
        format!(
            "hit={:?} gap={:?} old={:?} new={:?} boundary={}",
            fz.as_ref().map(|l| l.hits),
            fz.as_ref().map(|l| l.gaps),
            fz.as_ref().map(|l| l.too_old),
            fz.as_ref().map(|l| l.too_new),
            boundary
        ),
    ));

    // F49009 ③：分类次序 + 独立复算（同输入同格子）。
    let prec = class_precedence_ok();
    let mut recheck_ok = true;
    let mut rng = p_core::Lcg::new(0x4900_9E77);
    let mut r = 0u32;
    while r < 2_000 {
        let e = fuzz_entry(&mut rng);
        if accept_recheck(&e) != (classify(&e) == FuzzClass::Accepted) {
            recheck_ok = false;
        }
        r += 1;
    }
    out.push((
        "F49009/分类次序复算",
        prec && recheck_ok,
        format!("precedence={} recheck={}", prec, recheck_ok),
    ));

    // F49009 ④：复跑一致 + 段位 + 渲染。
    let replay = fuzz_replay(1_000);
    let seg = segment_guard();
    let render = render_selfcheck();
    let dg = fz.as_ref().map(|l| l.digest).unwrap_or(0);
    out.push((
        "F49009/复跑与段位",
        replay && seg && render && dg != 0,
        format!("replay={} seg={} render={} digest={:016X}", replay, seg, render, dg),
    ));

    // F49010 ①：损坏注入 ×10 回退告警全过。
    let warn = corruption_rounds(CORRUPTION_ROUTES as u32);
    let warn_ok = match &warn {
        Ok(w) => {
            w.attempts == CORRUPTION_ROUTES as u32
                && w.all_caught()
                && w.records.len() == w.attempts as usize
                && w.records.iter().all(|r| r.rolled_back && r.cfg_digest == DEFAULT_CFG.digest())
        }
        Err(_) => false,
    };
    out.push((
        "F49010/损坏×10回退",
        warn_ok,
        format!(
            "attempts={:?} caught={:?} records={:?}",
            warn.as_ref().map(|w| w.attempts),
            warn.as_ref().map(|w| w.caught),
            warn.as_ref().map(|w| w.records.len())
        ),
    ));

    // F49010 ②：告警可查（八种码全部出现、按码计数可检索）。
    let query_ok = match &warn {
        Ok(w) => {
            let mut all = 0u32;
            let mut i = 0usize;
            while i < WarnCode::ALL.len() {
                if let Some(c) = WarnCode::ALL.get(i) {
                    if w.find(*c) == 0 {
                        all = all.saturating_add(1);
                    }
                    if warn_detail(c.index() as u32).is_empty() {
                        all = all.saturating_add(1);
                    }
                }
                i += 1;
            }
            all == 0 && w.codes_seen() == 0xFF
        }
        Err(_) => false,
    };
    let q_digest = warn.as_ref().map(|w| w.digest()).unwrap_or(0);
    out.push((
        "F49010/告警可查",
        query_ok && q_digest != 0,
        format!("codes={:02X} digest={:016X}", warn.as_ref().map(|w| w.codes_seen()).unwrap_or(0), q_digest),
    ));

    // F49010 ③：干净配置不回退（24 条合法配置逐条往返）。
    let valid = valid_configs();
    let mut roundtrip = valid.len() as u32 == VALID_CONFIGS;
    let mut i = 0usize;
    while i < valid.len() {
        if let Some(c) = valid.get(i) {
            match encode(c) {
                Ok(b) => {
                    let out = load(&b);
                    if out.action != LoadAction::Kept || out.cfg != *c || out.warn.is_some() {
                        roundtrip = false;
                    }
                }
                Err(_) => roundtrip = false,
            }
        }
        i += 1;
    }
    out.push((
        "F49010/干净配置不回退",
        roundtrip && tamper_rejected(),
        format!("valid={} roundtrip={} tamper={}", valid.len(), roundtrip, tamper_rejected()),
    ));

    // F49010 ④：回退复跑一致（告警账摘要稳定）。
    let again = corruption_rounds(CORRUPTION_ROUTES as u32);
    let replay2 = match (&warn, &again) {
        (Ok(a), Ok(b)) => a.digest() == b.digest() && a.digest() != 0,
        _ => false,
    };
    out.push((
        "F49010/回退复跑一致",
        replay2 && render && seg,
        format!("replay={} render={} seg={}", replay2, render, seg),
    ));

    out
}

pub fn run_unx49009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49010 quirk 表 fuzz 与配置错误路径");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(FzCode::FuzzBad.val(), SEG_BASE + 0x01);
        assert_eq!(FzCode::DriftBad.val(), SEG_BASE + 14);
        assert_eq!(FzCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < FzCode::ALL.len() {
            if let Some(c) = FzCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn fuzz_classes_are_total_and_named() {
        assert_eq!(FuzzClass::ALL.len(), CLASSES);
        let mut i = 0usize;
        while i < FuzzClass::ALL.len() {
            assert_eq!(FuzzClass::ALL[i].index(), i);
            assert!(!FuzzClass::ALL[i].name().is_empty());
            i += 1;
        }
    }

    #[test]
    fn class_precedence_is_fixed() {
        assert!(class_precedence_ok());
    }

    #[test]
    fn ten_thousand_rounds_conserve_and_cover() {
        let l = fuzz_rounds(FUZZ_ROUNDS).unwrap();
        assert_eq!(l.rounds, 10_000);
        assert_eq!(l.accepted + l.rejected, 10_000);
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < l.classes.len() {
            assert!(l.classes[i] > 0);
            sum += l.classes[i];
            i += 1;
        }
        assert_eq!(sum, 10_000);
        assert_eq!(l.hits + l.gaps + l.too_old + l.too_new, 10_000);
        assert_eq!(l.drift, 0);
        assert!(l.conserved());
        assert!(l.covered());
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn fuzz_is_replayable() {
        assert!(fuzz_replay(1_000));
        let a = fuzz_rounds(500).unwrap();
        let b = fuzz_rounds(500).unwrap();
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.classes, b.classes);
    }

    #[test]
    fn windows_are_sorted_and_disjoint() {
        assert!(windows_ok());
        assert_eq!(FW_WINDOWS.len(), WINDOWS);
    }

    #[test]
    fn lookup_boundaries_are_exact() {
        assert_eq!(lookup(FW_MIN - 1), Lookup::TooOld);
        assert_eq!(lookup(0), Lookup::TooOld);
        assert_eq!(lookup(FW_WINDOWS[0].0), Lookup::Hit(0));
        assert_eq!(lookup(FW_WINDOWS[0].1), Lookup::Hit(0));
        assert_eq!(lookup(FW_WINDOWS[0].1 + 1), Lookup::Gap);
        assert_eq!(lookup(FW_WINDOWS[WINDOWS - 1].0), Lookup::Hit(5));
        assert_eq!(lookup(FW_WINDOWS[WINDOWS - 1].1), Lookup::Hit(5));
        assert_eq!(lookup(FW_WINDOWS[WINDOWS - 1].1 + 1), Lookup::TooNew);
    }

    #[test]
    fn lookup_names_are_stable() {
        assert_eq!(Lookup::Hit(2).name(), "hit");
        assert_eq!(Lookup::Gap.name(), "gap");
        assert_eq!(Lookup::TooOld.name(), "too-old");
        assert_eq!(Lookup::TooNew.name(), "too-new");
        assert_eq!(Lookup::Hit(0).slot(), 0);
        assert_eq!(Lookup::Gap.slot(), 3);
    }

    #[test]
    fn fuzz_handles_extreme_inputs_without_panic() {
        let extremes = [
            FuzzEntry { vendor: 0, model: 0, fw_lo: 0, fw_hi: 0, param: 0, flags: 0 },
            FuzzEntry {
                vendor: 0xFFFF,
                model: 0xFFFF,
                fw_lo: u32::MAX,
                fw_hi: u32::MAX,
                param: 0xFFFF,
                flags: 0xFF,
            },
            FuzzEntry {
                vendor: 1,
                model: 1,
                fw_lo: u32::MAX,
                fw_hi: 0,
                param: 1,
                flags: 0x0F,
            },
        ];
        let mut i = 0usize;
        while i < extremes.len() {
            let c = classify(&extremes[i]);
            assert_eq!(c, c);
            assert_eq!(accept_recheck(&extremes[i]), c == FuzzClass::Accepted);
            i += 1;
        }
        assert_eq!(classify(&extremes[0]), FuzzClass::VendorZero);
        assert_eq!(classify(&extremes[1]), FuzzClass::RangeBad);
        assert_eq!(classify(&extremes[2]), FuzzClass::RangeBad);
        assert_eq!(lookup(u32::MAX), Lookup::TooNew);
    }

    #[test]
    fn every_valid_config_round_trips() {
        let v = valid_configs();
        assert_eq!(v.len() as u32, VALID_CONFIGS);
        let mut i = 0usize;
        while i < v.len() {
            let b = encode(&v[i]).unwrap();
            assert_eq!(decode(&b), Ok(v[i]));
            let out = load(&b);
            assert_eq!(out.action, LoadAction::Kept);
            assert_eq!(out.cfg, v[i]);
            assert!(out.warn.is_none());
            i += 1;
        }
    }

    #[test]
    fn blob_structure_errors_are_distinct() {
        let good = encode(&Cfg::new(2, 0, 1)).unwrap();
        let mut b = good;
        b.magic ^= 1;
        assert_eq!(decode(&b).err(), Some(WarnCode::Magic));
        let mut b = good;
        b.version = 2;
        assert_eq!(decode(&b).err(), Some(WarnCode::Version));
        let mut b = good;
        b.len = 4;
        assert_eq!(decode(&b).err(), Some(WarnCode::Length));
        let mut b = good;
        b.len = 2;
        assert_eq!(decode(&b).err(), Some(WarnCode::Truncated));
        let mut b = good;
        b.checksum ^= 4;
        assert_eq!(decode(&b).err(), Some(WarnCode::Checksum));
        let zero = ConfigBlob {
            magic: 0,
            version: 0,
            len: 0,
            body: [0, 0, 0],
            checksum: 0,
        };
        assert_eq!(decode(&zero).err(), Some(WarnCode::Empty));
    }

    #[test]
    fn semantic_gates_survive_rechecksum() {
        let good = encode(&Cfg::new(1, 1, 1)).unwrap();
        let mut b = good;
        b.body[0] = 3;
        let signed = resign(&b);
        assert_eq!(decode(&signed).err(), Some(WarnCode::Axis));
        let mut b2 = good;
        b2.body = [0, 0, 2];
        let signed2 = resign(&b2);
        assert_eq!(decode(&signed2).err(), Some(WarnCode::Combo));
    }

    #[test]
    fn rollback_always_lands_on_default() {
        let good = encode(&Cfg::new(2, 2, 2)).unwrap();
        let bad = corrupt(&good, 7);
        let out = load(&bad);
        assert_eq!(out.action, LoadAction::RolledBack);
        assert_eq!(out.cfg, DEFAULT_CFG);
        assert_eq!(out.warn, Some(WarnCode::Combo));
        assert_eq!(out.cfg.digest(), DEFAULT_CFG.digest());
    }

    #[test]
    fn ten_corruptions_are_all_caught() {
        let w = corruption_rounds(CORRUPTION_ROUTES as u32).unwrap();
        assert_eq!(w.attempts, 10);
        assert_eq!(w.caught, 10);
        assert!(w.all_caught());
        assert_eq!(w.records.len(), 10);
        let mut i = 0usize;
        while i < w.records.len() {
            assert!(w.records[i].rolled_back);
            assert_eq!(w.records[i].cfg_digest, DEFAULT_CFG.digest());
            i += 1;
        }
    }

    #[test]
    fn warnings_are_queryable_by_code() {
        let w = corruption_rounds(CORRUPTION_ROUTES as u32).unwrap();
        assert_eq!(w.codes_seen(), 0xFF);
        let mut total = 0usize;
        let mut i = 0usize;
        while i < WarnCode::ALL.len() {
            let c = WarnCode::ALL[i];
            let n = w.find(c);
            assert!(n > 0, "{} should appear", c.detail());
            assert!(!warn_detail(c.index() as u32).is_empty());
            total += n;
            i += 1;
        }
        assert_eq!(total, 10);
        assert_eq!(warn_detail(99), "未知告警码");
        assert_ne!(w.digest(), 0);
    }

    #[test]
    fn corruption_replay_is_stable() {
        let a = corruption_rounds(CORRUPTION_ROUTES as u32).unwrap();
        let b = corruption_rounds(CORRUPTION_ROUTES as u32).unwrap();
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn rounds_bounds_are_enforced() {
        assert!(fuzz_rounds(0).is_err());
        assert!(fuzz_rounds(SCENARIO_MAX + 1).is_err());
        assert!(corruption_rounds(0).is_err());
        assert!(corruption_rounds(SCENARIO_MAX + 1).is_err());
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("F49009", 4_000, 10_000, 6_000, 10_000, 0x0011_2233_4455_6677);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("F49009|params=4000|schedule=10000|fails=6000|keys=10000"));
        assert!(!audit_render("F49009|params=x|schedule=10000|fails=6000|keys=10000|digest=0000000000000001"));
        assert!(!audit_render("F49009|params=4000|schedule=10000|fails=6000|keys=10000|digest=0x11"));
        assert_eq!(parse_u32("10000"), Some(10_000));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1 0"), None);
        assert_eq!(parse_hex16("0011223344556677"), Some(0x0011_2233_4455_6677));
    }

    #[test]
    fn ledger_row_is_auditable() {
        let l = fuzz_rounds(1_000).unwrap();
        let row = ledger_row(&l);
        assert!(audit_render(&row));
        assert!(row.starts_with("F49009|params="));
        assert!(row.contains("|schedule=1000|"));
        assert!(row.contains("|keys=1000|"));
        assert!(render_selfcheck());
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49009_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
        assert_eq!(STATS_FIELDS, 12);
        assert_eq!(RENDER_FIELDS, 6);
    }

    #[test]
    fn warn_codes_are_ordered_and_total() {
        assert_eq!(WarnCode::ALL.len(), WARN_CODES);
        let mut i = 0usize;
        while i < WarnCode::ALL.len() {
            assert_eq!(WarnCode::ALL[i].index(), i);
            assert!(!WarnCode::ALL[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(WARN_CODES, 8);
    }
}
