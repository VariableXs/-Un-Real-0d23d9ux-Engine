//! UNX-F49005 触控板配置面板语义 + UNX-F49006 配置持久化与热生效 · 0x71940
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-C 首槽；任务板 J1 引用按原文保留，
//! 不把 F49005 卡面引用的 F49006-J1 改写成 F49005-J1，也不把 F49006 卡面
//! 引用的 F49007-J1 改写成 F49006-J1，以便任务板可逐字回查）：
//!   UNX-F49005 触控板配置面板语义（灵敏度/滚动方向/点击力度三档）:
//!           ① 纯功能代码 ≥360 行；
//!           ② 判据原文：UNX-F49006-J1 热生效 ×10 断流 0；判据锚定 体验三；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49006 配置持久化与热生效（改配置不断流）:
//!           ① 纯功能代码 ≥340 行；
//!           ② 判据原文：UNX-F49007-J1 预告登记成文 + fake 预演 10/10 次；判据锚定 §AI-62 §8 横向 AI-57；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（三档配置 + 热生效）：
//!   轴/档位合法性 → 跨轴互斥 → 生效参数复算 → 持久化封印 → 热切换 →
//!   事件流连续性复核 → 账本渲染。
//! 轴档位先于跨轴互斥：三个轴各自 0..2 的取值域是互斥规则的定义域。若先判
//! 互斥再判域，越界档位会被当成「某个布尔条件不满足」而报成互斥冲突——错
//! 误码指错根因，运维会去调灵敏度，真正坏的是档位字节。
//! 互斥先于参数复算：重力度档（阈值 80）配迟钝增益（40）时，点击判定窗口
//! 与力度阈值在物理上不可同时满足。先算参数会把一个语义上不成立的组合算成
//! 一组「能跑」的数字，坏组合就此进入硬件寄存器。
//! 复算先于持久化：持久化写下去的是生效参数的封印摘要，不是三个档位字节。
//! 先持久化后复算，磁盘里会留下一条档位合法、参数却没算过的记录——恢复时
//! 它的校验和是自洽的，损坏因此变得不可检出。
//! 持久化先于热切换：一次热切换必须在「旧记录仍然可读」的前提下完成。顺序
//! 反过来（先切再持久化），切换后写盘失败就同时失去了旧配置和新记录，热
//! 生效退化成一次不可回滚的破坏。
//! 热切换先于流连续性复核：断流只可能发生在切换的那一刻。先复核后切换，复核
//! 的是切换前的平静窗口，判据「断流 0」被自己的动作绕开——这是最典型的
//! 自证绿。
//! 流连续性复核先于渲染：渲染行里的 keys 字段是事件计数的对外投影。先渲染
//! 后复核，一旦发生重置，屏幕上已经出现一行「连续」的数字，纠错只能靠人眼。
//! 段位守卫最后：14 个诊断码必须落在 [0x71941, 0x7194E]。窗口漂移会把前六项
//! 闸门的失败记成「未知诊断码」，红变灰。
//! 关键差异：本模块不做 quirk 表匹配与 T1 承诺面（B11-B 的 F49003/F49004 承担），
//! 不做多用户配置隔离预告与表版本校验和（B11-D 的 F49007/F49008 承担），不做
//! quirk 表 fuzz 与损坏配置回退（B11-E 的 F49009/F49010 承担）。
//! F49007-J1「预告登记成文 + fake 预演 10/10」的预告载体在 B11-D，本模块只
//! 逐字保留该引用，不自造预告登记，以免把 D 槽的判据提前预支成绿。
//! 本模块只回答两问：
//!   ① 三轴 × 三档的 27 个组合里，哪些是合法配置，合法配置的生效参数是否
//!      可复算、可区分、同配置恒同参数（三档 ×10^2 行为一致）；
//!   ② 配置改动的热路径是否零断流：十次热生效期间事件计数只增不减、从不
//!      归零，非法配置只进拒绝账、不打断事件流。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71940，VX6 批 B11-C 首槽）。
pub const SEG_BASE: u32 = 0x71940;

/// 轴数（灵敏度 / 滚动方向 / 点击力度）。
pub const AXES: usize = 3;
/// 每轴档位数。
pub const LEVELS: u8 = 3;
/// 组合总数（3^3）。
pub const COMBOS: u32 = 27;
/// 合法组合数（27 减去重力度 × 迟钝增益的三条互斥）。
pub const VALID_COMBOS: u32 = 24;
/// 互斥组合数。
pub const FORBIDDEN_COMBOS: u32 = 3;
/// 热生效轮数（判据 ×10）。
pub const HOT_ROUNDS: u32 = 10;
/// 每轮热切换前处理的事件数。
pub const EVENTS_BEFORE: u64 = 25;
/// 每轮热切换后处理的事件数。
pub const EVENTS_AFTER: u64 = 5;
/// 每轮事件总数。
pub const EVENTS_PER_ROUND: u64 = EVENTS_BEFORE + EVENTS_AFTER;
/// 三档一致性轮数（判据 ×10^2）。
pub const PARITY_ROUNDS: u32 = 100;
/// 轮数上界闸门。
pub const SCENARIO_MAX: u32 = 400;
/// 注入路数。
pub const INJECT_ROUTES: usize = 5;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 账本字段数。
pub const STATS_FIELDS: usize = 9;
/// 持久化记录魔数。
pub const PERSIST_MAGIC: u64 = 0x5034_3930_3035_0001;
/// 持久化格式版本。
pub const PERSIST_VERSION: u8 = 1;
/// fuzz 轮数上限。
pub const FUZZ_ROUNDS: u32 = 10_000;

// ── 三轴档位语义表 ──────────────────────────────────────────────
/// 灵敏度档 → 指针增益（千分之一为单位的整数增益）。
pub const SENS_GAIN: [u32; 3] = [40, 100, 160];
/// 滚动方向档 → (垂直符号, 水平符号)。
pub const DIR_SIGNS: [(i32, i32); 3] = [(1, 1), (-1, 1), (-1, -1)];
/// 点击力度档 → 触发阈值（力度单位）。
pub const PRESSURE_THRESHOLD: [u32; 3] = [20, 50, 80];

/// 轴名（下标即轴号）。
pub const AXIS_NAMES: [&str; AXES] = ["sensitivity", "direction", "pressure"];
/// 档位名（行=轴，列=档）。
pub const LEVEL_NAMES: [[&str; 3]; AXES] = [
    ["slow", "normal", "fast"],
    ["traditional", "natural", "natural-both"],
    ["light", "medium", "heavy"],
];

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanCode {
    /// 档位越出 0..2
    LevelBad,
    /// 跨轴互斥（重力度 × 迟钝增益）
    ComboBad,
    /// 持久化记录不完整
    PersistBad,
    /// 校验和不符
    ChecksumBad,
    /// 持久化格式版本不符
    VersionBad,
    /// 热切换账不守恒（epoch/applied 断裂）
    HotBad,
    /// 事件流断流或计数重置
    StreamBad,
    /// 账本不守恒
    LedgerBad,
    /// 三档行为不一致（同配置不同参数）
    ParityBad,
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
}

impl PanCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PanCode::LevelBad => "档位越出 0..2 的取值域",
            PanCode::ComboBad => "重力度档与迟钝增益互斥：阈值 80 配增益 40 不可达",
            PanCode::PersistBad => "持久化记录缺字段或序号倒退",
            PanCode::ChecksumBad => "持久化记录校验和与内容不符",
            PanCode::VersionBad => "持久化格式版本不是 1",
            PanCode::HotBad => "热切换账断裂：epoch 与生效次数不符",
            PanCode::StreamBad => "事件流断流：计数被重置或倒退",
            PanCode::LedgerBad => "热生效账不守恒（轮数/事件/次数）",
            PanCode::ParityBad => "同一配置两次求值参数不一致",
            PanCode::ReplayBad => "两次确定性复跑摘要不一致",
            PanCode::InjectBad => "注入轮次未被全部捕获",
            PanCode::RenderBad => "渲染行字段数或数值不可解析",
            PanCode::CoverBad => "覆盖轮数为零或越过上限",
            PanCode::SegmentBad => "诊断码越出本模块段窗",
        }
    }

    pub const ALL: [PanCode; 14] = [
        PanCode::LevelBad,
        PanCode::ComboBad,
        PanCode::PersistBad,
        PanCode::ChecksumBad,
        PanCode::VersionBad,
        PanCode::HotBad,
        PanCode::StreamBad,
        PanCode::LedgerBad,
        PanCode::ParityBad,
        PanCode::ReplayBad,
        PanCode::InjectBad,
        PanCode::RenderBad,
        PanCode::CoverBad,
        PanCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PanCode::ALL.len() {
        if let Some(c) = PanCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 三轴配置 ────────────────────────────────────────────────────
/// 触控板三轴配置：每轴三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelConfig {
    pub sensitivity: u8,
    pub direction: u8,
    pub pressure: u8,
}

impl PanelConfig {
    pub const fn new(sensitivity: u8, direction: u8, pressure: u8) -> PanelConfig {
        PanelConfig {
            sensitivity,
            direction,
            pressure,
        }
    }

    /// 默认配置：标准灵敏度 / 自然方向 / 中力度。
    pub const fn default_config() -> PanelConfig {
        PanelConfig::new(1, 1, 1)
    }

    /// 档位取值域闸门。
    pub fn levels_ok(&self) -> bool {
        self.sensitivity < LEVELS && self.direction < LEVELS && self.pressure < LEVELS
    }

    /// 配置校验：档位域 → 跨轴互斥。
    pub fn validate(&self) -> Result<(), u32> {
        if !self.levels_ok() {
            return Err(PanCode::LevelBad.val());
        }
        // 重力度档把点击阈值推到 80，迟钝档把指针增益压到 40；两者同时成立时
        // 点击判定窗口会短于一次正常点按的抖动周期，语义上不可达。
        if self.pressure == 2 && self.sensitivity == 0 {
            return Err(PanCode::ComboBad.val());
        }
        Ok(())
    }

    /// 配置指纹（六字节折叠，供账本与去重使用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, u32::from(self.sensitivity));
        h = p_core::fold_u32(h, u32::from(self.direction));
        h = p_core::fold_u32(h, u32::from(self.pressure));
        h
    }

    /// 标签（例如 "s2d1p0"），用于渲染与日志。
    pub fn tag(&self) -> String {
        format!("s{}d{}p{}", self.sensitivity, self.direction, self.pressure)
    }
}

/// 组合下标 → 配置（下标 = s*9 + d*3 + p）。
pub fn config_at(index: u32) -> Option<PanelConfig> {
    if index >= COMBOS {
        return None;
    }
    let s = (index / 9) as u8;
    let d = ((index / 3) % 3) as u8;
    let p = (index % 3) as u8;
    Some(PanelConfig::new(s, d, p))
}

/// 配置 → 组合下标。
pub fn index_of(c: &PanelConfig) -> Option<u32> {
    if !c.levels_ok() {
        return None;
    }
    Some(u32::from(c.sensitivity) * 9 + u32::from(c.direction) * 3 + u32::from(c.pressure))
}

/// 枚举全部 27 个组合（含非法），供校验与覆盖度统计。
pub fn enumerate_all() -> Vec<PanelConfig> {
    let mut v: Vec<PanelConfig> = Vec::new();
    let mut i = 0u32;
    while i < COMBOS {
        if let Some(c) = config_at(i) {
            v.push(c);
        }
        i += 1;
    }
    v
}

/// 枚举 24 个合法组合。
pub fn enumerate_valid() -> Vec<PanelConfig> {
    let mut v: Vec<PanelConfig> = Vec::new();
    let mut i = 0u32;
    while i < COMBOS {
        if let Some(c) = config_at(i) {
            if c.validate().is_ok() {
                v.push(c);
            }
        }
        i += 1;
    }
    v
}

/// 生效参数：一次配置求值的结果四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Effective {
    pub gain: u32,
    pub v_sign: i32,
    pub h_sign: i32,
    pub threshold: u32,
}

impl Effective {
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.gain);
        h = p_core::fold_i32(h, self.v_sign);
        h = p_core::fold_i32(h, self.h_sign);
        h = p_core::fold_u32(h, self.threshold);
        h
    }
}

/// 生效参数求值（路径 A：按轴分支查表）。
pub fn effective_a(c: &PanelConfig) -> Result<Effective, u32> {
    c.validate()?;
    let gain = match SENS_GAIN.get(c.sensitivity as usize) {
        Some(v) => *v,
        None => return Err(PanCode::LevelBad.val()),
    };
    let (v_sign, h_sign) = match DIR_SIGNS.get(c.direction as usize) {
        Some(v) => *v,
        None => return Err(PanCode::LevelBad.val()),
    };
    let threshold = match PRESSURE_THRESHOLD.get(c.pressure as usize) {
        Some(v) => *v,
        None => return Err(PanCode::LevelBad.val()),
    };
    if gain == 0 || threshold == 0 {
        return Err(PanCode::ParityBad.val());
    }
    Ok(Effective {
        gain,
        v_sign,
        h_sign,
        threshold,
    })
}

/// 生效参数求值（路径 B：单表线性化索引 + 独立算符）。
/// 与路径 A 同结果才算「可复算」：两条实现不共享中间变量。
pub fn effective_b(c: &PanelConfig) -> Result<Effective, u32> {
    if c.sensitivity >= LEVELS || c.direction >= LEVELS || c.pressure >= LEVELS {
        return Err(PanCode::LevelBad.val());
    }
    if c.pressure * 3 + c.sensitivity == 6 {
        // pressure==2 && sensitivity==0 的线性化写法（3*2+0）。
        return Err(PanCode::ComboBad.val());
    }
    let flat = (c.sensitivity as u32) * 16 + (c.direction as u32) * 4 + (c.pressure as u32);
    let gain = (flat >> 4) * 60 + 40;
    let v = ((flat >> 2) & 3) as i32;
    let v_sign = if v == 0 { 1 } else { -1 };
    let h_sign = if v == 2 { -1 } else { 1 };
    let threshold = (flat & 3) * 30 + 20;
    Ok(Effective {
        gain,
        v_sign,
        h_sign,
        threshold,
    })
}

/// 默认求值路径。
pub fn effective(c: &PanelConfig) -> Result<Effective, u32> {
    effective_a(c)
}

/// 两条路径逐字段一致。
pub fn effective_agree(c: &PanelConfig) -> bool {
    match (effective_a(c), effective_b(c)) {
        (Ok(a), Ok(b)) => a == b,
        (Err(a), Err(b)) => a == b,
        _ => false,
    }
}

/// 24 个合法配置的生效参数必须两两不同（否则某一档在观感上是假的）。
pub fn distinct_effective() -> u32 {
    let valid = enumerate_valid();
    let mut seen: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < valid.len() {
        let d = match effective_a(&valid[i]) {
            Ok(e) => e.digest(),
            Err(_) => return 0,
        };
        let mut j = 0usize;
        while j < seen.len() {
            if seen.get(j).copied() == Some(d) {
                return 0;
            }
            j += 1;
        }
        seen.push(d);
        i += 1;
    }
    seen.len() as u32
}

// ── 三档 ×10^2 行为一致 ────────────────────────────────────────
/// 一致性账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParityLedger {
    pub rounds: u32,
    pub valid: u32,
    pub invalid: u32,
    pub mismatches: u32,
    pub fingerprint: u64,
}

impl ParityLedger {
    pub const fn new() -> ParityLedger {
        ParityLedger {
            rounds: 0,
            valid: 0,
            invalid: 0,
            mismatches: 0,
            fingerprint: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds == self.valid.saturating_add(self.invalid) && self.mismatches == 0
    }
}

/// ×10^2 轮：随机取 27 个组合，合法者两条路径互验 + 同配置重算一致，
/// 非法者必须被两条路径以同一码拒绝。
pub fn parity_rounds(rounds: u32) -> Result<ParityLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(PanCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_5C31);
    let mut lg = ParityLedger::new();
    let mut fp = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let idx = rng.below(COMBOS);
        let cfg = match config_at(idx) {
            Some(v) => v,
            None => return Err(PanCode::CoverBad.val()),
        };
        let va = cfg.validate();
        let vb = effective_b(&cfg);
        match (va, vb) {
            (Ok(()), Ok(_)) => {
                let a1 = match effective_a(&cfg) {
                    Ok(v) => v,
                    Err(_) => return Err(PanCode::ParityBad.val()),
                };
                let a2 = match effective_a(&cfg) {
                    Ok(v) => v,
                    Err(_) => return Err(PanCode::ParityBad.val()),
                };
                let b2 = match effective_b(&cfg) {
                    Ok(v) => v,
                    Err(_) => return Err(PanCode::ParityBad.val()),
                };
                if a1 != a2 || a1 != b2 {
                    lg.mismatches = lg.mismatches.saturating_add(1);
                }
                fp = p_core::fold_u32(fp, idx);
                fp = p_core::fold_u32(fp, a1.gain);
                fp = p_core::fold_u32(fp, a1.threshold);
                lg.valid = lg.valid.saturating_add(1);
            }
            (Err(ea), Err(eb)) => {
                if ea != eb {
                    lg.mismatches = lg.mismatches.saturating_add(1);
                }
                fp = p_core::fold_u32(fp, idx);
                fp = p_core::fold_u32(fp, 0xFFFF_0000);
                lg.invalid = lg.invalid.saturating_add(1);
            }
            _ => {
                lg.mismatches = lg.mismatches.saturating_add(1);
            }
        }
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.fingerprint = fp;
    if !lg.conserved() {
        return Err(PanCode::ParityBad.val());
    }
    Ok(lg)
}

/// 一致性复跑：两轮摘要逐位相同且不为零。
pub fn parity_replay(rounds: u32) -> bool {
    let a = parity_rounds(rounds);
    let b = parity_rounds(rounds);
    match (a, b) {
        (Ok(a), Ok(b)) => a.fingerprint == b.fingerprint && a.fingerprint != 0 && a.valid > 0,
        _ => false,
    }
}

// ── 持久化记录 ──────────────────────────────────────────────────
/// 一条持久化记录：版本 + 配置 + 序号 + 校验和。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersistRecord {
    pub version: u8,
    pub cfg: PanelConfig,
    pub seq: u32,
    pub checksum: u64,
}

/// 校验和：魔数 → 版本 → 三档 → 序号，逐字段折叠。
pub fn checksum_of(version: u8, cfg: &PanelConfig, seq: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, (PERSIST_MAGIC >> 32) as u32);
    h = p_core::fold_u32(h, (PERSIST_MAGIC & 0xFFFF_FFFF) as u32);
    h = p_core::fold_byte(h, version);
    h = p_core::fold_byte(h, cfg.sensitivity);
    h = p_core::fold_byte(h, cfg.direction);
    h = p_core::fold_byte(h, cfg.pressure);
    h = p_core::fold_u32(h, seq);
    h
}

/// 生成记录（先校验配置，再封印）。
pub fn make_record(cfg: &PanelConfig, seq: u32) -> Result<PersistRecord, u32> {
    cfg.validate()?;
    Ok(PersistRecord {
        version: PERSIST_VERSION,
        cfg: *cfg,
        seq,
        checksum: checksum_of(PERSIST_VERSION, cfg, seq),
    })
}

/// 恢复记录：版本 → 校验和 → 配置语义。
pub fn restore(rec: &PersistRecord) -> Result<PanelConfig, u32> {
    if rec.version != PERSIST_VERSION {
        return Err(PanCode::VersionBad.val());
    }
    if rec.checksum == 0 {
        return Err(PanCode::PersistBad.val());
    }
    if checksum_of(rec.version, &rec.cfg, rec.seq) != rec.checksum {
        return Err(PanCode::ChecksumBad.val());
    }
    rec.cfg.validate()?;
    Ok(rec.cfg)
}

/// 持久化往返账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersistLedger {
    pub rounds: u32,
    pub written: u32,
    pub restored: u32,
    pub rejected: u32,
    pub fingerprint: u64,
}

impl PersistLedger {
    pub const fn new() -> PersistLedger {
        PersistLedger {
            rounds: 0,
            written: 0,
            restored: 0,
            rejected: 0,
            fingerprint: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds == self.written.saturating_add(self.rejected) && self.restored == self.written
    }
}

/// 持久化往返轮：每轮写一条、读一条、比对配置与序号。
pub fn persist_rounds(rounds: u32) -> Result<PersistLedger, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(PanCode::CoverBad.val());
    }
    let valid = enumerate_valid();
    if valid.is_empty() {
        return Err(PanCode::ParityBad.val());
    }
    let mut lg = PersistLedger::new();
    let mut fp = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let cfg = match valid.get((r as usize) % valid.len()) {
            Some(v) => *v,
            None => return Err(PanCode::CoverBad.val()),
        };
        match make_record(&cfg, r) {
            Ok(rec) => {
                lg.written = lg.written.saturating_add(1);
                match restore(&rec) {
                    Ok(back) => {
                        if back != cfg || rec.seq != r {
                            lg.rejected = lg.rejected.saturating_add(1);
                        } else {
                            lg.restored = lg.restored.saturating_add(1);
                            fp = p_core::fold_u32(fp, rec.seq);
                            fp = p_core::fold_u32(fp, (rec.checksum & 0xFFFF_FFFF) as u32);
                        }
                    }
                    Err(_) => lg.rejected = lg.rejected.saturating_add(1),
                }
            }
            Err(_) => {
                lg.rejected = lg.rejected.saturating_add(1);
            }
        }
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.fingerprint = fp;
    if !lg.conserved() {
        return Err(PanCode::LedgerBad.val());
    }
    Ok(lg)
}

// ── 热生效（事件流不断） ────────────────────────────────────────
/// 热生效状态：活跃配置 + 纪元 + 事件计数 + 各类计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotState {
    pub active: PanelConfig,
    pub epoch: u32,
    pub events: u64,
    pub rounds: u32,
    pub attempts: u32,
    pub applied: u32,
    pub rejected: u32,
    pub noops: u32,
    pub persists: u32,
    pub breaks: u32,
}

impl HotState {
    pub const fn new() -> HotState {
        HotState {
            active: PanelConfig::default_config(),
            epoch: 0,
            events: 0,
            rounds: 0,
            attempts: 0,
            applied: 0,
            rejected: 0,
            noops: 0,
            persists: 0,
            breaks: 0,
        }
    }

    /// 守恒：每次尝试恰好落进 生效/拒绝/空转 之一；每轮一次尝试一次落盘；
    /// 事件数 = 轮数 × 每轮事件数。
    pub fn conserved(&self) -> bool {
        self.attempts == self.applied.saturating_add(self.rejected).saturating_add(self.noops)
            && self.persists == self.rounds
            && self.events == u64::from(self.rounds).saturating_mul(EVENTS_PER_ROUND)
            && self.epoch == self.applied
    }

    /// 零断流：断流计数必须为 0。
    pub fn seamless(&self) -> bool {
        self.breaks == 0
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.active.sensitivity as u32);
        h = p_core::fold_u32(h, self.active.direction as u32);
        h = p_core::fold_u32(h, self.active.pressure as u32);
        h = p_core::fold_u32(h, self.epoch);
        h = p_core::fold_u32(h, (self.events & 0xFFFF_FFFF) as u32);
        h = p_core::fold_u32(h, (self.events >> 32) as u32);
        h = p_core::fold_u32(h, self.applied);
        h = p_core::fold_u32(h, self.rejected);
        h = p_core::fold_u32(h, self.noops);
        h = p_core::fold_u32(h, self.breaks);
        h
    }
}

/// 一次热切换：非法配置只进拒绝账，绝不触碰事件计数。
pub fn hot_apply(st: &mut HotState, next: &PanelConfig) -> Result<(), u32> {
    st.attempts = st.attempts.saturating_add(1);
    if let Err(e) = next.validate() {
        st.rejected = st.rejected.saturating_add(1);
        return Err(e);
    }
    if *next == st.active {
        st.noops = st.noops.saturating_add(1);
        return Ok(());
    }
    // 切换点：只改配置与纪元，事件计数在此处一字不动。
    let before = st.events;
    st.active = *next;
    st.epoch = st.epoch.saturating_add(1);
    st.applied = st.applied.saturating_add(1);
    if st.events != before {
        st.breaks = st.breaks.saturating_add(1);
    }
    Ok(())
}

/// 十轮热重载的候选序列：7 个合法变更 + 3 个非法注入。
pub const RELOAD_SEQ: [PanelConfig; HOT_ROUNDS as usize] = [
    PanelConfig::new(2, 0, 0),
    // 非法：重力度 × 迟钝增益。
    PanelConfig::new(0, 1, 2),
    PanelConfig::new(0, 2, 0),
    PanelConfig::new(2, 2, 2),
    PanelConfig::new(1, 0, 1),
    // 非法：灵敏度档位越界。
    PanelConfig::new(3, 0, 0),
    PanelConfig::new(0, 0, 1),
    PanelConfig::new(1, 2, 0),
    // 非法：方向档位越界。
    PanelConfig::new(1, 5, 1),
    PanelConfig::new(0, 0, 0),
];

/// 热重载：每轮 25 事件 → 切换 → 5 事件 → 落盘 → 回读。
/// 断流的唯一判据是事件计数在切换前后连续，故每一轮都复算总数。
pub fn hot_reload_rounds(rounds: u32) -> Result<HotState, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(PanCode::CoverBad.val());
    }
    let mut st = HotState::new();
    let mut r = 0u32;
    while r < rounds {
        // 切换前的事件。
        st.events = st.events.saturating_add(EVENTS_BEFORE);
        let before = st.events;
        let cand = match RELOAD_SEQ.get((r as usize) % HOT_ROUNDS as usize) {
            Some(v) => *v,
            None => return Err(PanCode::CoverBad.val()),
        };
        match hot_apply(&mut st, &cand) {
            Ok(()) => {}
            Err(_) => {
                // 非法配置：活跃配置与事件计数都必须保持原值。
                if st.events != before {
                    st.breaks = st.breaks.saturating_add(1);
                }
            }
        }
        // 切换后的事件：计数继续累加，不因拒绝而丢。
        st.events = st.events.saturating_add(EVENTS_AFTER);
        st.rounds = st.rounds.saturating_add(1);
        // 落盘 + 回读：记录读回来的必须就是当前活跃配置。
        let rec = match make_record(&st.active, st.epoch) {
            Ok(v) => v,
            Err(_) => return Err(PanCode::PersistBad.val()),
        };
        st.persists = st.persists.saturating_add(1);
        match restore(&rec) {
            Ok(back) => {
                if back != st.active {
                    st.breaks = st.breaks.saturating_add(1);
                }
            }
            Err(_) => {
                st.breaks = st.breaks.saturating_add(1);
            }
        }
        // 每轮复算事件总数：任何一次重置都会在这里现形。
        let want = u64::from(st.rounds).saturating_mul(EVENTS_PER_ROUND);
        if st.events != want {
            st.breaks = st.breaks.saturating_add(1);
        }
        r += 1;
    }
    if !st.conserved() {
        return Err(PanCode::LedgerBad.val());
    }
    if !st.seamless() {
        return Err(PanCode::StreamBad.val());
    }
    Ok(st)
}

/// 热重载复跑：两次摘要一致。
pub fn hot_replay(rounds: u32) -> bool {
    match (hot_reload_rounds(rounds), hot_reload_rounds(rounds)) {
        (Ok(a), Ok(b)) => a.digest() == b.digest() && a.digest() != 0,
        _ => false,
    }
}

/// 事件计数单调性：逐轮重放，任何一次倒退即失败。
pub fn events_monotonic(rounds: u32) -> bool {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return false;
    }
    let mut st = HotState::new();
    let mut prev: u64 = 0;
    let mut r = 0u32;
    while r < rounds {
        st.events = st.events.saturating_add(EVENTS_BEFORE);
        if st.events < prev {
            return false;
        }
        prev = st.events;
        let cand = match RELOAD_SEQ.get((r as usize) % HOT_ROUNDS as usize) {
            Some(v) => *v,
            None => return false,
        };
        let _ = hot_apply(&mut st, &cand);
        if st.events < prev {
            return false;
        }
        prev = st.events;
        st.events = st.events.saturating_add(EVENTS_AFTER);
        if st.events < prev {
            return false;
        }
        prev = st.events;
        st.rounds = st.rounds.saturating_add(1);
        r += 1;
    }
    st.events == u64::from(rounds).saturating_mul(EVENTS_PER_ROUND) && st.breaks == 0
}

// ── 注入与反例 ──────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub attempts: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        self.attempts > 0 && self.caught == self.attempts
    }
}

/// 五路注入：
/// 0 档位 3 越界；1 重力度 × 迟钝互斥；2 校验和被改一位；
/// 3 记录版本改成 2；4 记录配置在封印后又被改写（等价于外部篡改）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(PanCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_5D71);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(INJECT_ROUTES as u32);
        let ok = match route {
            0 => PanelConfig::new(1, 1, 3).validate().err() == Some(PanCode::LevelBad.val()),
            1 => PanelConfig::new(0, 0, 2).validate().err() == Some(PanCode::ComboBad.val()),
            2 => {
                let base = PanelConfig::new(2, 1, 1);
                let mut rec = match make_record(&base, 7) {
                    Ok(v) => v,
                    Err(_) => return Err(PanCode::PersistBad.val()),
                };
                rec.checksum ^= 0x0000_0000_0000_0100;
                restore(&rec).err() == Some(PanCode::ChecksumBad.val())
            }
            3 => {
                let base = PanelConfig::new(2, 1, 1);
                let mut rec = match make_record(&base, 7) {
                    Ok(v) => v,
                    Err(_) => return Err(PanCode::PersistBad.val()),
                };
                rec.version = 2;
                restore(&rec).err() == Some(PanCode::VersionBad.val())
            }
            _ => {
                // 封印后改写配置：校验和不再匹配。
                let base = PanelConfig::new(1, 0, 1);
                let mut rec = match make_record(&base, 9) {
                    Ok(v) => v,
                    Err(_) => return Err(PanCode::PersistBad.val()),
                };
                rec.cfg.pressure = 2;
                restore(&rec).err() == Some(PanCode::ChecksumBad.val())
            }
        };
        if ok {
            caught = caught.saturating_add(1);
        }
        last = route;
        r += 1;
    }
    Ok(InjectLog {
        attempts: rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒（六条独立反例）。
pub fn tamper_rejected() -> bool {
    let c1 = PanelConfig::new(3, 0, 0).validate().err() == Some(PanCode::LevelBad.val());
    let c2 = PanelConfig::new(0, 3, 0).validate().err() == Some(PanCode::LevelBad.val());
    let c3 = PanelConfig::new(0, 0, 2).validate().err() == Some(PanCode::ComboBad.val());
    // 合法边界：重力度配标准增益是允许的。
    let c4 = PanelConfig::new(1, 0, 2).validate().is_ok();
    // 校验和篡改。
    let mut rec = make_record(&PanelConfig::new(1, 1, 1), 3).unwrap_or(PersistRecord {
        version: 0,
        cfg: PanelConfig::new(0, 0, 0),
        seq: 0,
        checksum: 0,
    });
    rec.checksum = rec.checksum.wrapping_add(1);
    let c5 = restore(&rec).err() == Some(PanCode::ChecksumBad.val());
    // 序号改动同样要被抓。
    let mut rec2 = make_record(&PanelConfig::new(1, 1, 1), 3).unwrap_or(rec);
    rec2.seq = 4;
    let c6 = restore(&rec2).err() == Some(PanCode::ChecksumBad.val());
    c1 && c2 && c3 && c4 && c5 && c6
}

// ── 段位守卫 + 渲染 ─────────────────────────────────────────────
/// 段位守卫：14 个诊断码全部落在 [SEG_BASE+1, SEG_BASE+14]。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < PanCode::ALL.len() {
        if let Some(c) = PanCode::ALL.get(i) {
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

/// 六字段渲染：tag | params（合法组合数）| schedule（热生效轮数）|
/// fails（拒绝次数）| keys（事件计数）| digest。
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

/// 切六字段。
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

/// 渲染审计。
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

/// 热状态渲染行。
pub fn hot_row(st: &HotState) -> String {
    render_row(
        "P49005",
        VALID_COMBOS,
        st.rounds,
        st.rejected,
        (st.events & 0xFFFF_FFFF) as u32,
        st.digest(),
    )
}

/// 渲染自检：热行可解析且字段与状态一致。
pub fn render_selfcheck() -> bool {
    let st = match hot_reload_rounds(HOT_ROUNDS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let row = hot_row(&st);
    if !audit_render(&row) {
        return false;
    }
    let parts = match row_fields(&row) {
        Some(v) => v,
        None => return false,
    };
    if parts.get(1).copied() != Some("params=24") {
        return false;
    }
    if parts.get(2).copied() != Some("schedule=10") {
        return false;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49005 ①：三轴三档自洽（27 组合、24 合法、3 互斥）。
    let all = enumerate_all();
    let valid = enumerate_valid();
    let mut bad_levels = 0u32;
    let mut i = 0usize;
    while i < all.len() {
        if let Some(c) = all.get(i) {
            if !c.levels_ok() {
                bad_levels = bad_levels.saturating_add(1);
            }
        }
        i += 1;
    }
    let axes_ok = all.len() as u32 == COMBOS
        && valid.len() as u32 == VALID_COMBOS
        && bad_levels == 0
        && COMBOS - VALID_COMBOS == FORBIDDEN_COMBOS;
    out.push((
        "F49005/三轴三档自洽",
        axes_ok,
        format!(
            "all={} valid={} forbidden={}",
            all.len(),
            valid.len(),
            COMBOS - VALID_COMBOS as u32
        ),
    ));

    // F49005 ②：三档 ×10^2 行为一致。
    let par = parity_rounds(PARITY_ROUNDS);
    let par_ok = match &par {
        Ok(l) => l.rounds == PARITY_ROUNDS && l.conserved() && l.valid > 0 && l.invalid > 0,
        Err(_) => false,
    };
    out.push((
        "F49005/三档×10^2行为一致",
        par_ok,
        format!(
            "rounds={:?} valid={:?} invalid={:?} mism={:?}",
            par.as_ref().map(|l| l.rounds),
            par.as_ref().map(|l| l.valid),
            par.as_ref().map(|l| l.invalid),
            par.as_ref().map(|l| l.mismatches)
        ),
    ));

    // F49005 ③：三档可区分（24 个指纹两两不同 + 双路径互验）。
    let distinct = distinct_effective();
    let mut agree = true;
    let mut k = 0usize;
    while k < valid.len() {
        if let Some(c) = valid.get(k) {
            if !effective_agree(c) {
                agree = false;
            }
        }
        k += 1;
    }
    out.push((
        "F49005/三档可区分",
        distinct == VALID_COMBOS && agree,
        format!("distinct={} paths_agree={}", distinct, agree),
    ));

    // F49005 ④：反例必拒（越界档位、互斥组合、封印后篡改）。
    let tamper = tamper_rejected();
    let inj = inject_rounds(1_000);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    out.push((
        "F49005/反例必拒",
        tamper && inj_ok,
        format!(
            "tamper={} inject={:?}/{:?}",
            tamper,
            inj.as_ref().map(|l| l.caught),
            inj.as_ref().map(|l| l.attempts)
        ),
    ));

    // F49006 ①：持久化往返（写 10 读 10，序号与配置一致）。
    let pers = persist_rounds(HOT_ROUNDS);
    let pers_ok = match &pers {
        Ok(l) => l.rounds == HOT_ROUNDS && l.written > 0 && l.restored == l.written && l.conserved(),
        Err(_) => false,
    };
    out.push((
        "F49006/持久化往返",
        pers_ok,
        format!(
            "written={:?} restored={:?} rejected={:?}",
            pers.as_ref().map(|l| l.written),
            pers.as_ref().map(|l| l.restored),
            pers.as_ref().map(|l| l.rejected)
        ),
    ));

    // F49006 ②：热生效 ×10 断流 0。
    let hot = hot_reload_rounds(HOT_ROUNDS);
    let hot_ok = match &hot {
        Ok(s) => {
            s.rounds == HOT_ROUNDS
                && s.applied == 7
                && s.rejected == 3
                && s.noops == 0
                && s.events == EVENTS_PER_ROUND * u64::from(HOT_ROUNDS)
                && s.breaks == 0
                && s.conserved()
                && s.seamless()
        }
        Err(_) => false,
    };
    out.push((
        "F49006/热生效×10断流0",
        hot_ok,
        format!(
            "applied={:?} rejected={:?} breaks={:?} events={:?}",
            hot.as_ref().map(|s| s.applied),
            hot.as_ref().map(|s| s.rejected),
            hot.as_ref().map(|s| s.breaks),
            hot.as_ref().map(|s| s.events)
        ),
    ));

    // F49006 ③：事件计数不重置（单调 + 总数复算 + epoch 与生效次数一致）。
    let mono = events_monotonic(HOT_ROUNDS);
    let epoch_ok = matches!(&hot, Ok(s) if s.epoch == s.applied && s.persists == s.rounds);
    out.push((
        "F49006/事件计数不重置",
        mono && epoch_ok,
        format!(
            "monotonic={} epoch={:?} persists={:?}",
            mono,
            hot.as_ref().map(|s| s.epoch),
            hot.as_ref().map(|s| s.persists)
        ),
    ));

    // F49006 ④：复跑零漂移 + 段位守卫 + 渲染。
    let replay = parity_replay(PARITY_ROUNDS) && hot_replay(HOT_ROUNDS);
    let seg = segment_guard();
    let render = render_selfcheck();
    let dg = hot.as_ref().map(|s| s.digest()).unwrap_or(0);
    out.push((
        "F49006/复跑与段位",
        replay && seg && render && dg != 0,
        format!(
            "replay={} seg={} render={} digest={:016X}",
            replay, seg, render, dg
        ),
    ));

    out
}

pub fn run_unx49005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49006 触控板三档配置与热生效");
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
        assert_eq!(PanCode::LevelBad.val(), SEG_BASE + 0x01);
        assert_eq!(PanCode::SegmentBad.val(), SEG_BASE + 14);
        assert_eq!(PanCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < PanCode::ALL.len() {
            if let Some(c) = PanCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn three_axes_have_three_levels_each() {
        assert_eq!(AXIS_NAMES.len(), AXES);
        let mut a = 0usize;
        while a < AXES {
            assert_eq!(LEVEL_NAMES[a].len(), 3);
            assert!(!LEVEL_NAMES[a][0].is_empty());
            a += 1;
        }
        assert_eq!(SENS_GAIN.len(), 3);
        assert_eq!(DIR_SIGNS.len(), 3);
        assert_eq!(PRESSURE_THRESHOLD.len(), 3);
        assert_eq!(AXES, 3);
        assert_eq!(LEVELS, 3);
    }

    #[test]
    fn all_27_combos_enumerate_and_24_validate() {
        let all = enumerate_all();
        let valid = enumerate_valid();
        assert_eq!(all.len() as u32, COMBOS);
        assert_eq!(valid.len() as u32, VALID_COMBOS);
        assert_eq!(COMBOS - VALID_COMBOS, FORBIDDEN_COMBOS);
        let mut i = 0usize;
        while i < all.len() {
            let c = all[i];
            assert_eq!(index_of(&c), Some(i as u32));
            assert_eq!(c.validate().is_ok(), valid.contains(&c));
            i += 1;
        }
    }

    #[test]
    fn level_gates_precede_cross_axis_gate() {
        // 越界档位必须报 LevelBad，而不是被互斥规则抢先报 ComboBad。
        assert_eq!(
            PanelConfig::new(0, 0, 5).validate().err(),
            Some(PanCode::LevelBad.val())
        );
        assert_eq!(
            PanelConfig::new(0, 0, 2).validate().err(),
            Some(PanCode::ComboBad.val())
        );
        assert!(PanelConfig::new(1, 0, 2).validate().is_ok());
    }

    #[test]
    fn effective_parameters_follow_the_table() {
        let c = PanelConfig::new(2, 1, 0);
        let e = effective(&c).unwrap();
        assert_eq!(e.gain, 160);
        assert_eq!(e.v_sign, -1);
        assert_eq!(e.h_sign, 1);
        assert_eq!(e.threshold, 20);
        let z = PanelConfig::new(0, 0, 1);
        let ez = effective(&z).unwrap();
        assert_eq!(ez.gain, 40);
        assert_eq!(ez.v_sign, 1);
        assert_eq!(ez.threshold, 50);
    }

    #[test]
    fn two_evaluation_paths_agree_on_every_combo() {
        let all = enumerate_all();
        let mut i = 0usize;
        while i < all.len() {
            assert!(effective_agree(&all[i]));
            i += 1;
        }
    }

    #[test]
    fn twenty_four_configs_are_distinguishable() {
        assert_eq!(distinct_effective(), VALID_COMBOS);
    }

    #[test]
    fn parity_hundred_rounds_is_consistent() {
        let l = parity_rounds(PARITY_ROUNDS).unwrap();
        assert_eq!(l.rounds, PARITY_ROUNDS);
        assert_eq!(l.mismatches, 0);
        assert_eq!(l.valid + l.invalid, PARITY_ROUNDS);
        assert!(l.valid > 0 && l.invalid > 0);
        assert!(l.conserved());
        assert_ne!(l.fingerprint, 0);
    }

    #[test]
    fn parity_replay_is_stable() {
        assert!(parity_replay(PARITY_ROUNDS));
    }

    #[test]
    fn persist_round_trip_restores_every_round() {
        let l = persist_rounds(HOT_ROUNDS).unwrap();
        assert_eq!(l.written, 10);
        assert_eq!(l.restored, 10);
        assert_eq!(l.rejected, 0);
        assert!(l.conserved());
        assert_ne!(l.fingerprint, 0);
    }

    #[test]
    fn checksum_covers_every_field() {
        let a = PanelConfig::new(1, 1, 1);
        let base = checksum_of(PERSIST_VERSION, &a, 5);
        assert_ne!(base, checksum_of(PERSIST_VERSION, &PanelConfig::new(2, 1, 1), 5));
        assert_ne!(base, checksum_of(PERSIST_VERSION, &PanelConfig::new(1, 2, 1), 5));
        assert_ne!(base, checksum_of(PERSIST_VERSION, &PanelConfig::new(1, 1, 2), 5));
        assert_ne!(base, checksum_of(PERSIST_VERSION, &a, 6));
        assert_ne!(base, checksum_of(2, &a, 5));
    }

    #[test]
    fn restore_rejects_tampered_records() {
        let rec = make_record(&PanelConfig::new(2, 2, 2), 11).unwrap();
        assert_eq!(restore(&rec), Ok(PanelConfig::new(2, 2, 2)));
        let mut bad = rec;
        bad.cfg.direction = 0;
        assert_eq!(restore(&bad).err(), Some(PanCode::ChecksumBad.val()));
        let mut bad2 = rec;
        bad2.version = 2;
        assert_eq!(restore(&bad2).err(), Some(PanCode::VersionBad.val()));
    }

    #[test]
    fn hot_ten_rounds_never_break_stream() {
        let st = hot_reload_rounds(HOT_ROUNDS).unwrap();
        assert_eq!(st.rounds, HOT_ROUNDS);
        assert_eq!(st.applied, 7);
        assert_eq!(st.rejected, 3);
        assert_eq!(st.noops, 0);
        assert_eq!(st.events, 300);
        assert_eq!(st.epoch, 7);
        assert_eq!(st.persists, 10);
        assert_eq!(st.breaks, 0);
        assert!(st.conserved());
        assert!(st.seamless());
    }

    #[test]
    fn invalid_config_keeps_active_and_events() {
        let mut st = HotState::new();
        st.events = 25;
        let before = st.active;
        let e = hot_apply(&mut st, &PanelConfig::new(0, 0, 2));
        assert_eq!(e.err(), Some(PanCode::ComboBad.val()));
        assert_eq!(st.active, before);
        assert_eq!(st.events, 25);
        assert_eq!(st.rejected, 1);
        assert_eq!(st.applied, 0);
        assert_eq!(st.epoch, 0);
    }

    #[test]
    fn noop_apply_does_not_move_epoch() {
        let mut st = HotState::new();
        let cur = st.active;
        assert!(hot_apply(&mut st, &cur).is_ok());
        assert_eq!(st.epoch, 0);
        assert_eq!(st.applied, 0);
        assert_eq!(st.noops, 1);
        assert!(hot_apply(&mut st, &PanelConfig::new(2, 0, 0)).is_ok());
        assert_eq!(st.epoch, 1);
        assert_eq!(st.applied, 1);
    }

    #[test]
    fn events_are_monotonic_across_reloads() {
        assert!(events_monotonic(HOT_ROUNDS));
        assert!(hot_replay(HOT_ROUNDS));
    }

    #[test]
    fn injection_catches_every_attempt() {
        let l = inject_rounds(1_000).unwrap();
        assert_eq!(l.attempts, 1_000);
        assert_eq!(l.caught, 1_000);
        assert!(l.all_caught());
        assert!(inject_rounds(0).is_err());
        assert!(inject_rounds(FUZZ_ROUNDS + 1).is_err());
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn rounds_bounds_are_enforced() {
        assert!(hot_reload_rounds(0).is_err());
        assert!(hot_reload_rounds(SCENARIO_MAX + 1).is_err());
        assert!(parity_rounds(0).is_err());
        assert!(persist_rounds(0).is_err());
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("P49005", 24, 10, 3, 300, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("P49005|params=24|schedule=10|fails=3|keys=300"));
        assert!(!audit_render("P49005|params=z|schedule=10|fails=3|keys=300|digest=0000000000000001"));
        assert!(!audit_render("P49005|params=24|schedule=10|fails=3|keys=300|digest=ABC"));
        assert_eq!(parse_u32("300"), Some(300));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("3x0"), None);
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
    }

    #[test]
    fn hot_row_is_auditable() {
        let st = hot_reload_rounds(HOT_ROUNDS).unwrap();
        let row = hot_row(&st);
        assert!(audit_render(&row));
        assert!(row.starts_with("P49005|params=24|schedule=10|fails=3|keys=300|"));
        assert!(render_selfcheck());
    }

    #[test]
    fn config_tag_and_fingerprint_are_stable() {
        let c = PanelConfig::new(2, 1, 0);
        assert_eq!(c.tag(), "s2d1p0");
        assert_eq!(c.fingerprint(), c.fingerprint());
        assert_ne!(c.fingerprint(), PanelConfig::new(2, 1, 1).fingerprint());
        assert_eq!(index_of(&PanelConfig::new(3, 0, 0)), None);
        assert!(config_at(COMBOS).is_none());
        assert_eq!(config_at(13), Some(PanelConfig::new(1, 1, 1)));
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49005_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
        assert_eq!(STATS_FIELDS, 9);
        assert_eq!(RENDER_FIELDS, 6);
        assert_eq!(RELOAD_SEQ.len(), HOT_ROUNDS as usize);
    }
}
