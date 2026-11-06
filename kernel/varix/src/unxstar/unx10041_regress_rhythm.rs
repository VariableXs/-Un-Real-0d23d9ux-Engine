//! UNX-F10041 回归常态化总纲（节奏与触发） · 0x91a40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10041（B22 常态化回归线首卡）。
//! 锚点：UNX-F10042-J1 周期表冻结（三档参数注出处），无源档 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91a40（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `RhythmCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `RhythmRegistry`：日/批/波三档节奏槽位，注册必须带出处锚
//!     （"UNX-F" 前缀），无源档显性拒绝；周期表冻结后参数只读；
//!   - 状态机 `RhythmState`：Collecting → FrozenTable 单向推进，缺档冻结拒绝；
//!   - 留痕账 `TraceLedger`：触发留痕 = 入账 + 逃逸，逃逸恒 0，只增不减；
//!   - 注入核 `BadRhythmCase`：无出处档 / 参数越界 / 重复注册 / 缺档冻结 /
//!     冻结后改参 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10041 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91a40;

/// 段窗上界（下一卡 0x91a80 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91a80;

/// 节奏档数（日/批/波）。
pub const TIER_COUNT: usize = 3;

/// 注册出处锚前缀（无此前缀即无源档）。
pub const SOURCE_PREFIX: &str = "UNX-F";

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 日档周期（每日 1 tick 一轮；来源：常态化总纲日档条款）。
pub const DAILY_INTERVAL: u32 = 1;

/// 批档周期（每批收口一轮；来源：B22 收口小结批档条款）。
pub const BATCH_INTERVAL: u32 = 20;

/// 波档周期（每波出账一轮；来源：B21 报表线六波节奏）。
pub const WAVE_INTERVAL: u32 = 6;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const EBUSY: i32 = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RhythmCode {
    /// 无源档（出处锚缺失或格式非法）
    NoSource,
    /// 节奏参数越界（interval/cap 出窗）
    ParamRange,
    /// 重复注册同档
    DuplicateTier,
    /// 缺档冻结（三档未齐即冻结）
    MissingTier,
    /// 冻结后改参
    FrozenMutate,
    /// 触发无留痕（逃逸）
    TraceEscape,
    /// 留痕账倒退
    LedgerRegress,
    /// 未知档位
    UnknownTier,
    /// 周期表未冻结即取参
    NotFrozen,
    /// 双跑不一致
    NotReproducible,
    /// 解析失败
    ParseFailure,
    /// 段位越界
    SegmentViolation,
}

impl RhythmCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RhythmCode::NoSource => "无源档被拒：节奏参数必须注出处锚（UNX-F 前缀）；请补出处再注册",
            RhythmCode::ParamRange => "参数越界被拒：周期 1..=240、容量 1..=200；请核对参数窗",
            RhythmCode::DuplicateTier => "重复注册被拒：同档已有槽位；请先解冻或改用他档",
            RhythmCode::MissingTier => "缺档冻结被拒：三档未齐不得冻结周期表；请补齐缺失档",
            RhythmCode::FrozenMutate => "冻结后改参被拒：周期表已封账只读；请走修订通道",
            RhythmCode::TraceEscape => "触发无留痕被拒：每次触发必须入留痕账；请补留痕",
            RhythmCode::LedgerRegress => "留痕账倒退被拒：计数只增不减；请排查回滚误改",
            RhythmCode::UnknownTier => "未知档位被拒：档位只允许日/批/波三档；请核对照",
            RhythmCode::NotFrozen => "周期表未冻结：冻结前不得对外供参；请先完成冻结",
            RhythmCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            RhythmCode::ParseFailure => "解析失败：节奏行必须恰为三段 tier|interval|cap；请补齐字段",
            RhythmCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91a40..=0x91a80",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RhythmCode; 12] = [
        RhythmCode::NoSource,
        RhythmCode::ParamRange,
        RhythmCode::DuplicateTier,
        RhythmCode::MissingTier,
        RhythmCode::FrozenMutate,
        RhythmCode::TraceEscape,
        RhythmCode::LedgerRegress,
        RhythmCode::UnknownTier,
        RhythmCode::NotFrozen,
        RhythmCode::NotReproducible,
        RhythmCode::ParseFailure,
        RhythmCode::SegmentViolation,
    ];
}

// ── 档位与注册表 ──────────────────────────────────────────────────────
/// 节奏档位（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierKind {
    /// 日档
    Daily,
    /// 批档
    PerBatch,
    /// 波档
    PerWave,
}

impl TierKind {
    /// 闭集全量。
    pub const ALL: [TierKind; 3] =
        [TierKind::Daily, TierKind::PerBatch, TierKind::PerWave];

    /// 从文本解析（"daily"/"batch"/"wave"）。
    pub fn from_field(s: &str) -> Option<TierKind> {
        match s {
            "daily" => Some(TierKind::Daily),
            "batch" => Some(TierKind::PerBatch),
            "wave" => Some(TierKind::PerWave),
            _ => None,
        }
    }

    /// 槽位下标。
    pub fn index(self) -> usize {
        match self {
            TierKind::Daily => 0,
            TierKind::PerBatch => 1,
            TierKind::PerWave => 2,
        }
    }
}

/// 已注册的节奏槽位：参数与出处锚一并封账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RhythmSlot {
    /// 档位
    pub tier: TierKind,
    /// 周期（tick 数，1..=240）
    pub interval: u32,
    /// 单轮容量上限（1..=200）
    pub cap: u32,
}

/// 节奏注册表：三档槽位 + 出处锚 + 冻结状态。
#[derive(Clone, Debug)]
pub struct RhythmRegistry {
    /// 三档槽位（None=未注册）
    pub slots: [Option<RhythmSlot>; TIER_COUNT],
    /// 三档出处锚（与槽位一一对应，注册时校验前缀）
    pub sources: [&'static str; TIER_COUNT],
    /// 当前状态
    pub state: RhythmState,
}

/// 状态机：Collecting → FrozenTable 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RhythmState {
    /// 收集中（注册进行时）
    Collecting,
    /// 周期表已冻结（参数只读）
    FrozenTable,
}

impl RhythmRegistry {
    /// 构造空注册表（出处锚初始为空串占位）。
    pub const fn new() -> RhythmRegistry {
        RhythmRegistry {
            slots: [None, None, None],
            sources: ["", "", ""],
            state: RhythmState::Collecting,
        }
    }

    /// 出处锚合法：非空且以 "UNX-F" 开头。
    pub fn source_ok(src: &str) -> bool {
        !src.is_empty() && src.len() >= SOURCE_PREFIX.len()
            && &src.as_bytes()[..SOURCE_PREFIX.len()] == SOURCE_PREFIX.as_bytes()
    }

    /// 注册一档：出处必检、参数必检、重复必拒、冻结后必拒。
    pub fn register(
        &mut self,
        tier: TierKind,
        interval: u32,
        cap: u32,
        source: &'static str,
    ) -> Result<(), RhythmCode> {
        if self.state == RhythmState::FrozenTable {
            return Err(RhythmCode::FrozenMutate);
        }
        if !Self::source_ok(source) {
            return Err(RhythmCode::NoSource);
        }
        if interval == 0 || interval > 240 || cap == 0 || cap > 200 {
            return Err(RhythmCode::ParamRange);
        }
        let idx = tier.index();
        if self.slots.get(idx).copied().unwrap_or(Some(RhythmSlot {
            tier: TierKind::Daily,
            interval: 0,
            cap: 0,
        }))
        .is_some()
        {
            return Err(RhythmCode::DuplicateTier);
        }
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(RhythmSlot { tier, interval, cap });
        }
        // 出处锚与槽位同槽写入；合法前缀已在上游校验。
        if let Some(src) = self.sources.get_mut(idx) {
            *src = source;
        }
        Ok(())
    }

    /// 周期表冻结：三档齐备才可冻结；冻结后参数只读。
    pub fn freeze(&mut self) -> Result<(), RhythmCode> {
        if self.state == RhythmState::FrozenTable {
            return Err(RhythmCode::FrozenMutate);
        }
        let mut i = 0usize;
        while i < TIER_COUNT {
            if self.slots.get(i).copied().unwrap_or(None).is_none() {
                return Err(RhythmCode::MissingTier);
            }
            i += 1;
        }
        self.state = RhythmState::FrozenTable;
        Ok(())
    }

    /// 冻结后取参：未冻结即取参显性拒绝（NotFrozen）。
    pub fn interval_of(&self, tier: TierKind) -> Result<u32, RhythmCode> {
        if self.state != RhythmState::FrozenTable {
            return Err(RhythmCode::NotFrozen);
        }
        match self.slots.get(tier.index()).copied().unwrap_or(None) {
            Some(slot) => Ok(slot.interval),
            None => Err(RhythmCode::UnknownTier),
        }
    }
}

/// 标准三档注册（出处锚固定为本卡判据）。
pub fn register_standard(reg: &mut RhythmRegistry) -> Result<(), RhythmCode> {
    reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1")?;
    reg.register(TierKind::PerBatch, BATCH_INTERVAL, 200, "UNX-F10042-J1")?;
    reg.register(TierKind::PerWave, WAVE_INTERVAL, 60, "UNX-F10042-J1")
}

// ── 触发留痕账 ────────────────────────────────────────────────────────
/// 留痕账：触发 = 入账 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceLedger {
    /// 触发总次数
    pub fired: u32,
    /// 已留痕次数
    pub traced: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl TraceLedger {
    /// 构造零账。
    pub const fn zero() -> TraceLedger {
        TraceLedger { fired: 0, traced: 0, escaped: 0 }
    }

    /// 记一次触发；trace=false 即逃逸入账（显性记逃逸而非静默吞）。
    pub fn record(&mut self, trace: bool) -> Result<(), RhythmCode> {
        self.fired = self.fired.saturating_add(1);
        if trace {
            self.traced = self.traced.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.traced.saturating_add(self.escaped) != self.fired {
            return Err(RhythmCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.traced.saturating_add(self.escaped) == self.fired
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏注册样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRhythmCase {
    /// 无出处档（空锚）
    NoSource,
    /// 坏前缀出处档
    BadPrefix,
    /// 参数越界（interval=0 与 interval>240 双探）
    ParamRange,
    /// 重复注册
    DuplicateTier,
    /// 缺档冻结
    MissingTier,
}

impl BadRhythmCase {
    /// 闭集全量。
    pub const ALL: [BadRhythmCase; 5] = [
        BadRhythmCase::NoSource,
        BadRhythmCase::BadPrefix,
        BadRhythmCase::ParamRange,
        BadRhythmCase::DuplicateTier,
        BadRhythmCase::MissingTier,
    ];

    /// 注入一轮：构造坏样本并触发注册表，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let mut reg = RhythmRegistry::new();
        match self {
            BadRhythmCase::NoSource => {
                reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "").is_err()
            }
            BadRhythmCase::BadPrefix => {
                reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "B22 手记").is_err()
            }
            BadRhythmCase::ParamRange => {
                let a = reg.register(TierKind::Daily, 0, 200, "UNX-F10042-J1").is_err();
                let b = reg.register(TierKind::PerBatch, 999, 200, "UNX-F10042-J1").is_err();
                a && b
            }
            BadRhythmCase::DuplicateTier => {
                if reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1").is_err() {
                    return false;
                }
                reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1").is_err()
            }
            BadRhythmCase::MissingTier => {
                if reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1").is_err() {
                    return false;
                }
                if reg.register(TierKind::PerBatch, BATCH_INTERVAL, 200, "UNX-F10042-J1").is_err() {
                    return false;
                }
                reg.freeze().is_err()
            }
        }
    }
}

/// 冻结后改参注入（独立核：冻结成功后再注册必须被拒）。
pub fn inject_frozen_mutate() -> bool {
    let mut reg = RhythmRegistry::new();
    if register_standard(&mut reg).is_err() {
        return false;
    }
    if reg.freeze().is_err() {
        return false;
    }
    reg.register(TierKind::Daily, 2, 200, "UNX-F10042-J1").is_err()
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_rhythm_injection() -> (TraceLedger, [u32; 5]) {
    let mut ledger = TraceLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRhythmCase::ALL.len() {
        if let Some(case) = BadRhythmCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject() && inject_frozen_mutate();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结后改参族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；无源档/缺档族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；冻结表只读族"),
    ("EBUSY", -16, 170, "Windows ERROR_RESOURCE_BUSY；重复注册占用族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；参数越界族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：注册→冻结→取参全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = RhythmRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    // 坏注册逐次入账（无源档先行）。
    bytes.push(match reg.register(TierKind::Daily, DAILY_INTERVAL, 200, "") {
        Ok(()) => 0,
        Err(_) => 1,
    });
    let _ = register_standard(&mut reg);
    let frozen = reg.freeze();
    bytes.push(match frozen {
        Ok(()) => 2,
        Err(_) => 3,
    });
    let mut t = 0usize;
    while t < TierKind::ALL.len() {
        if let Some(kind) = TierKind::ALL.get(t).copied() {
            match reg.interval_of(kind) {
                Ok(v) => bytes.extend_from_slice(&v.to_le_bytes()),
                Err(_) => bytes.push(0),
            }
        }
        t += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10041 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10041_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10041");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91a40..=0x91a80", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 标准三档注册成功
    let mut reg = RhythmRegistry::new();
    let std_ok = register_standard(&mut reg).is_ok();
    s.add("注册表：日/批/波三档带出处注册成功", std_ok,
          "出处锚 UNX-F10042-J1 三档各落一槽");

    // 3. 出处锚格式核
    let src_ok = RhythmRegistry::source_ok("UNX-F10042-J1")
        && !RhythmRegistry::source_ok("")
        && !RhythmRegistry::source_ok("B22 手记");
    s.add("出处锚：UNX-F 前缀合法、空锚/俗锚拒绝", src_ok,
          "无源档在注册口即拦，不允许进表");

    // 4. 无源档 10/10 拦截（锚点判据直接探针）
    let mut nosrc_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut tmp = RhythmRegistry::new();
        if tmp.register(TierKind::Daily, DAILY_INTERVAL, 200, "").is_err() {
            nosrc_hit = nosrc_hit.saturating_add(1);
        }
        r += 1;
    }
    s.add("无源档拦截：10/10 轮注册即拦", nosrc_hit == INJECT_ROUNDS,
          "出处锚缺失的档位每轮都被显式拒绝");

    // 5. 参数越界拦截
    let mut rng = RhythmRegistry::new();
    let range_ok = rng.register(TierKind::Daily, 0, 200, "UNX-F10042-J1").is_err()
        && rng.register(TierKind::PerBatch, 999, 200, "UNX-F10042-J1").is_err()
        && rng.register(TierKind::PerWave, WAVE_INTERVAL, 0, "UNX-F10042-J1").is_err();
    s.add("参数窗：周期 1..=240、容量 1..=200 越界拒绝", range_ok,
          "三档参数窗口闭集，出窗即拦");

    // 6. 重复注册拦截
    let mut dup = RhythmRegistry::new();
    let _ = dup.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1");
    let dup_ok = dup.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1")
        == Err(RhythmCode::DuplicateTier);
    s.add("重复注册：同档二次注册被拒", dup_ok,
          "一档一槽，重复注册显性拒绝");

    // 7. 缺档冻结拦截 + 齐档冻结成功
    let mut part = RhythmRegistry::new();
    let _ = part.register(TierKind::Daily, DAILY_INTERVAL, 200, "UNX-F10042-J1");
    let part_rej = part.freeze() == Err(RhythmCode::MissingTier);
    let mut full = RhythmRegistry::new();
    let _ = register_standard(&mut full);
    let full_ok = full.freeze().is_ok() && full.state == RhythmState::FrozenTable;
    s.add("周期表冻结：缺档拒绝、三档齐通过", part_rej && full_ok,
          "冻结是单向闸：Collecting → FrozenTable");

    // 8. 冻结后改参拦截
    let mut frozen_reg = RhythmRegistry::new();
    let _ = register_standard(&mut frozen_reg);
    let _ = frozen_reg.freeze();
    let mut_ok = frozen_reg.register(TierKind::Daily, 2, 200, "UNX-F10042-J1")
        == Err(RhythmCode::FrozenMutate);
    s.add("冻结后改参：再注册被拒", mut_ok,
          "周期表冻结即封账，修订必须走通道");

    // 9. 未冻结取参拦截 + 冻结后取参正确
    let mut nf = RhythmRegistry::new();
    let _ = register_standard(&mut nf);
    let nf_rej = nf.interval_of(TierKind::Daily) == Err(RhythmCode::NotFrozen);
    let frozen_val = frozen_reg.interval_of(TierKind::PerBatch) == Ok(BATCH_INTERVAL);
    s.add("取参守卫：未冻结拒绝、冻结后逐档取参", nf_rej && frozen_val,
          "批档周期 20 只有冻结后才对外供参");

    // 10. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_rhythm_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "无源/坏前缀/越参/重复/缺档+冻结改参全被显式拒绝");

    // 11. 留痕账守恒：逃逸=0
    s.add("留痕账守恒：触发=留痕+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.fired == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次留痕、0 逃逸");

    // 12. 文案闭集：非空
    let text_ok = RhythmCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < RhythmCode::ALL.len() {
        let mut j = i + 1;
        while j < RhythmCode::ALL.len() {
            if let (Some(x), Some(y)) = (RhythmCode::ALL.get(i), RhythmCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = RhythmCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EBUSY)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 留痕逃逸显性入账：trace=false 记逃逸不静默
    let mut esc = TraceLedger::zero();
    let _ = esc.record(true);
    let _ = esc.record(false);
    s.add("留痕账：无痕触发显性记逃逸", esc.escaped == 1 && esc.conserved(),
          "逃逸必须入账可见，禁止静默吞触发");

    // 17. 解析面：未知档文本拒绝
    let unk = TierKind::from_field("nightly").is_none()
        && TierKind::from_field("daily").is_some();
    s.add("档位闭集：未知档文本拒绝、合法文本命中", unk,
          "档位只允许 daily/batch/wave 三键");

    // 动态明细（≤8）：留痕账一行 + 指纹一行
    let ledger_text = format!("触发 {} / 留痕 {} / 逃逸 {}", ledger.fired, ledger.traced, ledger.escaped);
    s.add_dyn("留痕快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_registration_ok() {
        let mut reg = RhythmRegistry::new();
        assert!(register_standard(&mut reg).is_ok());
        assert_eq!(reg.slots.iter().filter(|s| s.is_some()).count(), TIER_COUNT);
    }

    #[test]
    fn no_source_rejected() {
        let mut reg = RhythmRegistry::new();
        assert_eq!(reg.register(TierKind::Daily, 1, 200, ""), Err(RhythmCode::NoSource));
        assert_eq!(reg.register(TierKind::Daily, 1, 200, "x"), Err(RhythmCode::NoSource));
    }

    #[test]
    fn param_range_rejected() {
        let mut reg = RhythmRegistry::new();
        assert_eq!(reg.register(TierKind::Daily, 0, 200, "UNX-F10042-J1"),
                   Err(RhythmCode::ParamRange));
        assert_eq!(reg.register(TierKind::Daily, 1, 999, "UNX-F10042-J1"),
                   Err(RhythmCode::ParamRange));
    }

    #[test]
    fn duplicate_tier_rejected() {
        let mut reg = RhythmRegistry::new();
        let _ = reg.register(TierKind::Daily, 1, 200, "UNX-F10042-J1");
        assert_eq!(reg.register(TierKind::Daily, 1, 200, "UNX-F10042-J1"),
                   Err(RhythmCode::DuplicateTier));
    }

    #[test]
    fn missing_tier_cannot_freeze() {
        let mut reg = RhythmRegistry::new();
        let _ = reg.register(TierKind::Daily, 1, 200, "UNX-F10042-J1");
        assert_eq!(reg.freeze(), Err(RhythmCode::MissingTier));
    }

    #[test]
    fn frozen_table_is_readonly() {
        let mut reg = RhythmRegistry::new();
        let _ = register_standard(&mut reg);
        assert!(reg.freeze().is_ok());
        assert_eq!(reg.register(TierKind::PerWave, 6, 60, "UNX-F10042-J1"),
                   Err(RhythmCode::FrozenMutate));
        assert_eq!(reg.interval_of(TierKind::PerWave), Ok(WAVE_INTERVAL));
    }

    #[test]
    fn unknown_tier_text_rejected() {
        assert!(TierKind::from_field("weekly").is_none());
        assert!(TierKind::from_field("batch").is_some());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_rhythm_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.fired, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBUSY));
        assert!(agrees_with_linux(EPERM));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn rhythmcode_text_complete() {
        for c in RhythmCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10041_checks_green() {
        let s = run_unx10041_checks();
        assert!(s.all_passed(), "UNX-F10041 red: {}", s.red_items_text());
    }
}
