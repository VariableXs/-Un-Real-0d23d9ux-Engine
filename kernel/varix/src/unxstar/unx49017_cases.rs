//! UNX-F49017 quirk fuzz 面 + UNX-F49018 判据用例集 · 0x71AC0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-D 首槽；卡面 J1 引用按原文保留）：
//!   UNX-F49017 触控板 quirk 表引擎与配置面 fuzz 面（乱参数注入不崩）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49018-J1 ×20 全绿入库；判据锚定 §AI-62 §8；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49018 触控板 quirk 表引擎与配置面判据用例集（20 例入库可复跑）:
//!          ① 纯功能代码 ≥200 行；
//!          ② 判据原文：UNX-F49019-J1 批收口防重留痕零撞号；判据锚定 §6.1；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 说明：F49017 卡面引用 UNX-F49018-J1、F49018 卡面引用 UNX-F49019-J1（板上
//! 自身的 +1 引用）。此处逐字保留，不改号，便于任务板锚定逐字回查。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（fuzz 面）：参数域枚举 → 越界先拒 → 一致性 → 表窗命中 →
//! 边缘降级 → 兜底 Miss → 独立复算 → 落账。
//! 参数域枚举先于一切：四个参数（阈值/增益/力度/滞回）必须各有明确的合法
//! 区间。区间没定义时，「乱参数」与「合法但极端」无法区分，fuzz 只是在测
//! 自己的想象力。
//! 越界先于一致性：阈值 0 / 增益 0 / 力度 >9 / 滞回 >40 属结构越界，必须先
//! 拒；先做一致性（滞回 > 阈值）会让「阈值 0、滞回 0」这种坏参数漏过去。
//! 一致性先于表窗：滞回大于阈值意味着回弹窗口比触发窗口还宽，参数自相矛盾。
//! 自相矛盾的参数即使落进某条表窗，也只是巧合命中，不能当 Hit。
//! 表窗先于边缘降级：Hit 是「已知 good 档」，Degrade 是「能跑但要收承诺」。
//! 顺序反了，贴着表窗边缘的合法参数会被降级吞掉，表窗等于白设。
//! 边缘降级先于兜底：阈值 ≥230、增益 ≤12、滞回/阈值比 >3/10 是三条降级线，
//! 必须先判；先兜底 Miss 会把「该收承诺的档」记成「不在表里」，承诺面失真。
//! 独立复算后于分类：分类用乘法比值，复算用除法比值，二者是两条独立路径。
//! 只有逐轮一致，fuzz 的「没崩」才同时也意味着「没算错」。
//! 落账后于全部判定：账本按四类判词计数，轮数必须等于四类之和。先落账后
//! 判定，账本会把中途异常吃成某一类，fuzz 的零崩就成了统计假象。
//! 闸门次序（20 例用例集）：条数 → 编号升序 → 名称唯一 → 期望值复算 →
//! 判词覆盖 → 摘要复跑。
//! 条数/编号先于名称：20 例是入库门槛，编号必须 1..20 连续；编号断裂时
//! 「名称唯一」只是在给一张残缺表做美容。
//! 名称先于期望值：用例名是复跑时的检索键，重名会让同一名字对应两条期望，
//! 回查时无法定位到底哪条红。
//! 期望值复算先于覆盖：每条期望必须同时被 classify 与 reference 两条路径
//! 确认；期望是抄来的数字时，20 例全绿只是「抄得一致」。
//! 覆盖后于复跑：四种判词都要有用例（Reject/Hit/Degrade/Miss），否则复跑
//! 摘要再稳定，也只是稳定地没测到某一类。
//! 关键差异：本模块不做 quirk 表的匹配链（表引擎在 0x718C0 段），不做四点
//! 埋点（在 0x71A80 段），不做文案走查（在 0x71A40 段）。fuzz 只覆盖参数
//! 分类这一个纯函数域，不注入真实设备事件，也不写盘；20 例用例集只登记
//! 参数/期望/判词，不携带跨批状态，更不声称任务板已写回。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71AC0，VX6 批 B11-D 首槽）。
pub const SEG_BASE: u32 = 0x71AC0;

/// fuzz 轮数（卡面 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 轮数上界（覆盖闸门）。
pub const FUZZ_MAX: u32 = 20_000;
/// 表窗档数。
pub const PROFILES: usize = 4;
/// 判词种类数。
pub const VERDICTS: usize = 4;
/// 用例条数（卡面 ×20）。
pub const CASES: usize = 20;
/// 用例编号起点。
pub const CASE_ID_FIRST: u8 = 1;
/// 期望值复算轮数（摘要复跑三遍）。
pub const REPLAY_PASSES: u32 = 3;
/// 阈值下界。
pub const THRESHOLD_MIN: u32 = 1;
/// 阈值上界。
pub const THRESHOLD_MAX: u32 = 250;
/// 增益上界。
pub const GAIN_MAX: u32 = 200;
/// 力度上界。
pub const FORCE_MAX: u32 = 9;
/// 滞回上界。
pub const HYST_MAX: u32 = 40;
/// 降级阈值线（阈值 ≥ 此值即降级）。
pub const DEG_THRESHOLD: u32 = 230;
/// 降级增益线（增益 ≤ 此值即降级）。
pub const DEG_GAIN: u32 = 12;
/// 表窗阈值容差。
pub const TOL_THRESHOLD: u32 = 8;
/// 表窗增益容差。
pub const TOL_GAIN: u32 = 10;
/// 表窗滞回容差。
pub const TOL_HYST: u32 = 4;
/// 敌意参数路数。
pub const HOSTILE_ROUTES: usize = 5;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QcCode {
    /// 用例/表窗条数不符
    CountBad,
    /// 表窗结构非法（阈值越界/重复）
    ProfileBad,
    /// 参数越界或自相矛盾
    ParamBad,
    /// 判词越出四类或分类不收敛
    VerdictBad,
    /// 用例编号/形状非法
    CaseBad,
    /// 用例编号或名称重复
    DupBad,
    /// 用例未按编号升序
    OrderBad,
    /// fuzz 账本不守恒
    LedgerBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 渲染行不可解析
    RenderBad,
    /// 覆盖轮数为零或越界
    CoverBad,
    /// 判词覆盖缺口（某类判词无用例）
    GapBad,
    /// 诊断码越出本模块段窗
    SegmentBad,
    /// 敌意参数未被拒或不收敛
    HostileBad,
}

impl QcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QcCode::CountBad => "用例条数不是 20 或表窗不是 4 档",
            QcCode::ProfileBad => "表窗阈值越界、区间倒置或档位重复",
            QcCode::ParamBad => "参数越界（阈值/增益/力度/滞回）或滞回大于阈值",
            QcCode::VerdictBad => "判词越出 Hit/Miss/Degrade/Reject 四类",
            QcCode::CaseBad => "用例编号不是 1..20 或名称为空",
            QcCode::DupBad => "用例编号或名称重复",
            QcCode::OrderBad => "用例未按编号升序排列",
            QcCode::LedgerBad => "fuzz 轮数 ≠ 四类判词计数之和",
            QcCode::ReplayBad => "两次独立复算结果不一致",
            QcCode::RenderBad => "渲染行字段缺失或回读不一致",
            QcCode::CoverBad => "覆盖轮数为零或越过上限",
            QcCode::GapBad => "四种判词未全部被用例覆盖",
            QcCode::SegmentBad => "诊断码越出本模块段窗",
            QcCode::HostileBad => "敌意参数未被拒或分类不收敛",
        }
    }

    pub const ALL: [QcCode; 14] = [
        QcCode::CountBad,
        QcCode::ProfileBad,
        QcCode::ParamBad,
        QcCode::VerdictBad,
        QcCode::CaseBad,
        QcCode::DupBad,
        QcCode::OrderBad,
        QcCode::LedgerBad,
        QcCode::ReplayBad,
        QcCode::RenderBad,
        QcCode::CoverBad,
        QcCode::GapBad,
        QcCode::SegmentBad,
        QcCode::HostileBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QcCode::ALL.len() {
        if let Some(c) = QcCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 参数与判词 ────────────────────────────────────────────────────
/// 四个 quirk 参数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QParams {
    pub threshold: u32,
    pub gain: u32,
    pub force: u32,
    pub hysteresis: u32,
}

impl QParams {
    pub const fn new(threshold: u32, gain: u32, force: u32, hysteresis: u32) -> QParams {
        QParams {
            threshold,
            gain,
            force,
            hysteresis,
        }
    }

    pub fn digits(&self) -> u32 {
        self.threshold
            .saturating_add(self.gain)
            .saturating_add(self.force)
            .saturating_add(self.hysteresis)
    }
}

/// 四类判词：命中 / 未命中 / 降级 / 拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Hit,
    Miss,
    Degrade,
    Reject,
}

impl Verdict {
    pub fn index(self) -> u8 {
        match self {
            Verdict::Hit => 0,
            Verdict::Miss => 1,
            Verdict::Degrade => 2,
            Verdict::Reject => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verdict::Hit => "hit",
            Verdict::Miss => "miss",
            Verdict::Degrade => "degrade",
            Verdict::Reject => "reject",
        }
    }

    pub const ALL: [Verdict; VERDICTS] = [
        Verdict::Hit,
        Verdict::Miss,
        Verdict::Degrade,
        Verdict::Reject,
    ];
}

/// 一条表窗：中心值 + 固定容差（容差为模块常量，不随档位变化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Profile {
    pub threshold: u32,
    pub gain: u32,
    pub force: u32,
    pub hysteresis: u32,
}

pub const PROFILE_TABLE: [Profile; PROFILES] = [
    Profile { threshold: 30, gain: 180, force: 2, hysteresis: 2 },
    Profile { threshold: 60, gain: 140, force: 3, hysteresis: 6 },
    Profile { threshold: 120, gain: 80, force: 5, hysteresis: 12 },
    Profile { threshold: 200, gain: 45, force: 7, hysteresis: 20 },
];

pub fn abs_diff(a: u32, b: u32) -> u32 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// 参数是否落在合法区间。
pub fn bounds_ok(p: &QParams) -> bool {
    p.threshold >= THRESHOLD_MIN
        && p.threshold <= THRESHOLD_MAX
        && p.gain >= 1
        && p.gain <= GAIN_MAX
        && p.force <= FORCE_MAX
        && p.hysteresis <= HYST_MAX
}

/// 表窗命中：四项同时进窗（力度必须精确相等）。
pub fn within(prof: &Profile, p: &QParams) -> bool {
    abs_diff(p.threshold, prof.threshold) <= TOL_THRESHOLD
        && abs_diff(p.gain, prof.gain) <= TOL_GAIN
        && p.force == prof.force
        && abs_diff(p.hysteresis, prof.hysteresis) <= TOL_HYST
}

/// 分类（主路径）：越界 → 一致性 → 表窗 → 降级线 → 兜底 Miss。
pub fn classify(p: &QParams) -> Verdict {
    if !bounds_ok(p) || p.hysteresis > p.threshold {
        return Verdict::Reject;
    }
    let mut i = 0usize;
    while i < PROFILE_TABLE.len() {
        if let Some(prof) = PROFILE_TABLE.get(i) {
            if within(prof, p) {
                return Verdict::Hit;
            }
        }
        i += 1;
    }
    if p.threshold >= DEG_THRESHOLD
        || p.gain <= DEG_GAIN
        || p.hysteresis.saturating_mul(10) > p.threshold.saturating_mul(3)
    {
        return Verdict::Degrade;
    }
    Verdict::Miss
}

/// 独立复算（副路径）：同样的语义，用不同写法重算一遍。
/// 差别：区间判定写成显式闭区间链，降级比值写成除法形式，
/// 表窗用「先算差值再比较」的局部变量，便于与主路径交叉核对。
pub fn reference_verdict(p: &QParams) -> Verdict {
    let bounds = p.threshold >= 1
        && p.threshold <= 250
        && p.gain >= 1
        && p.gain <= 200
        && p.force <= 9
        && p.hysteresis <= 40;
    if !bounds || p.hysteresis > p.threshold {
        return Verdict::Reject;
    }
    let mut hit = false;
    let mut i = 0usize;
    while i < PROFILE_TABLE.len() {
        if let Some(prof) = PROFILE_TABLE.get(i) {
            let dt = abs_diff(p.threshold, prof.threshold);
            let dg = abs_diff(p.gain, prof.gain);
            let dh = abs_diff(p.hysteresis, prof.hysteresis);
            if dt <= 8 && dg <= 10 && dh <= 4 && p.force == prof.force {
                hit = true;
            }
        }
        i += 1;
    }
    if hit {
        return Verdict::Hit;
    }
    let ratio = p.hysteresis > p.threshold.saturating_mul(3) / 10;
    if p.threshold >= 230 || p.gain <= 12 || ratio {
        Verdict::Degrade
    } else {
        Verdict::Miss
    }
}

/// 表窗自审：档数 → 阈值/增益/力度/滞回在界 → 档位唯一 → 阈值升序。
pub fn profile_integrity(table: &[Profile]) -> Result<u64, u32> {
    if table.len() != PROFILES {
        return Err(QcCode::CountBad.val());
    }
    let mut fp = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let p = match table.get(i) {
            Some(v) => *v,
            None => return Err(QcCode::ProfileBad.val()),
        };
        if p.threshold < THRESHOLD_MIN || p.threshold > THRESHOLD_MAX {
            return Err(QcCode::ProfileBad.val());
        }
        if p.gain < 1 || p.gain > GAIN_MAX {
            return Err(QcCode::ProfileBad.val());
        }
        if p.force > FORCE_MAX || p.hysteresis > HYST_MAX {
            return Err(QcCode::ProfileBad.val());
        }
        let mut j = 0usize;
        while j < i {
            if let Some(q) = table.get(j) {
                if q.threshold == p.threshold {
                    return Err(QcCode::ProfileBad.val());
                }
                if q.threshold > p.threshold {
                    return Err(QcCode::OrderBad.val());
                }
            }
            j += 1;
        }
        fp = p_core::fold_u32(fp, p.threshold);
        fp = p_core::fold_u32(fp, p.gain);
        fp = p_core::fold_u32(fp, p.force);
        fp = p_core::fold_u32(fp, p.hysteresis);
        i += 1;
    }
    Ok(fp)
}

// ── fuzz 面 ───────────────────────────────────────────────────────
/// fuzz 账：四类判词计数 + 独立复算不一致数 + 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzLedger {
    pub rounds: u32,
    pub hit: u32,
    pub miss: u32,
    pub degrade: u32,
    pub reject: u32,
    pub mismatches: u32,
    pub extreme_rejects: u32,
    pub digest: u64,
}

impl FuzzLedger {
    pub const fn new() -> FuzzLedger {
        FuzzLedger {
            rounds: 0,
            hit: 0,
            miss: 0,
            degrade: 0,
            reject: 0,
            mismatches: 0,
            extreme_rejects: 0,
            digest: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds
            == self
                .hit
                .saturating_add(self.miss)
                .saturating_add(self.degrade)
                .saturating_add(self.reject)
    }

    pub fn per_verdict(&self) -> [u32; VERDICTS] {
        [self.hit, self.miss, self.degrade, self.reject]
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// 边界值表（含 0 与 u32::MAX，专门撞溢出与越界）。
pub const EDGES: [u32; 13] = [0, 1, 2, 9, 10, 12, 40, 41, 200, 201, 230, 250, u32::MAX];

/// 带符号抖动后夹回 u32 全域（不 panic、不溢出）。
pub fn jitter(base: u32, delta: i32) -> u32 {
    let v = base as i64 + delta as i64;
    if v < 0 {
        0
    } else if v > u32::MAX as i64 {
        u32::MAX
    } else {
        v as u32
    }
}

fn fold_params(h: u64, p: &QParams) -> u64 {
    let mut x = p_core::fold_u32(h, p.threshold);
    x = p_core::fold_u32(x, p.gain);
    x = p_core::fold_u32(x, p.force);
    x = p_core::fold_u32(x, p.hysteresis);
    x
}

/// fuzz：四种取参模式，逐轮分类 + 独立复算 + 落账。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzLedger, u32> {
    if rounds == 0 || rounds > FUZZ_MAX {
        return Err(QcCode::CoverBad.val());
    }
    profile_integrity(&PROFILE_TABLE)?;
    let mut rng = p_core::Lcg::new(0x4901_7C01);
    let mut lg = FuzzLedger::new();
    let mut i = 0u32;
    while i < rounds {
        let mode = rng.below(4);
        let p = match mode {
            0 => QParams::new(
                rng.next() as u32,
                rng.next() as u32,
                rng.next() as u32,
                rng.next() as u32,
            ),
            1 => QParams::new(
                THRESHOLD_MIN + rng.below(THRESHOLD_MAX - THRESHOLD_MIN + 1),
                1 + rng.below(GAIN_MAX),
                rng.below(FORCE_MAX + 1),
                rng.below(HYST_MAX + 1),
            ),
            2 => QParams::new(
                match EDGES.get(rng.below(EDGES.len() as u32) as usize) {
                    Some(v) => *v,
                    None => 0,
                },
                match EDGES.get(rng.below(EDGES.len() as u32) as usize) {
                    Some(v) => *v,
                    None => 0,
                },
                match EDGES.get(rng.below(EDGES.len() as u32) as usize) {
                    Some(v) => *v,
                    None => 0,
                },
                match EDGES.get(rng.below(EDGES.len() as u32) as usize) {
                    Some(v) => *v,
                    None => 0,
                },
            ),
            _ => {
                let prof = match PROFILE_TABLE.get(rng.below(PROFILES as u32) as usize) {
                    Some(v) => *v,
                    None => PROFILE_TABLE[0],
                };
                QParams::new(
                    jitter(prof.threshold, rng.signed(12)),
                    jitter(prof.gain, rng.signed(14)),
                    rng.below(FORCE_MAX + 1),
                    jitter(prof.hysteresis, rng.signed(6)),
                )
            }
        };
        let v = classify(&p);
        let rv = reference_verdict(&p);
        if v != rv {
            lg.mismatches = lg.mismatches.saturating_add(1);
        }
        if !bounds_ok(&p) {
            lg.extreme_rejects = lg.extreme_rejects.saturating_add(1);
        }
        match v {
            Verdict::Hit => lg.hit = lg.hit.saturating_add(1),
            Verdict::Miss => lg.miss = lg.miss.saturating_add(1),
            Verdict::Degrade => lg.degrade = lg.degrade.saturating_add(1),
            Verdict::Reject => lg.reject = lg.reject.saturating_add(1),
        }
        lg.rounds = lg.rounds.saturating_add(1);
        lg.digest = p_core::fold_u32(lg.digest, i);
        lg.digest = fold_params(lg.digest, &p);
        lg.digest = p_core::fold_u32(lg.digest, v.index() as u32);
        i = i.saturating_add(1);
    }
    if !lg.conserved() {
        return Err(QcCode::LedgerBad.val());
    }
    if lg.mismatches != 0 {
        return Err(QcCode::ReplayBad.val());
    }
    Ok(lg)
}

/// 有界性探针：每个参数取 0 / 1 / 上界 / u32::MAX 的组合，分类必须收敛。
pub fn bounded_probe() -> Result<u32, u32> {
    let mut seen = [0u32; VERDICTS];
    let probes = [
        QParams::new(0, 0, 0, 0),
        QParams::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        QParams::new(120, 80, 5, 12),
        QParams::new(250, 200, 9, 40),
        QParams::new(1, 1, 0, 1),
        QParams::new(231, 200, 9, 40),
        QParams::new(100, 12, 5, 10),
        QParams::new(145, 95, 5, 10),
        QParams::new(80, 80, 5, 30),
        QParams::new(240, 100, 5, 10),
    ];
    let mut n = 0u32;
    for p in probes.iter() {
        let v = classify(p);
        let rv = reference_verdict(p);
        if v != rv {
            return Err(QcCode::VerdictBad.val());
        }
        if let Some(slot) = seen.get_mut(v.index() as usize) {
            *slot = slot.saturating_add(1);
        }
        n = n.saturating_add(1);
    }
    // 四类都必须至少出现过一次，否则探针覆盖不成立。
    let mut k = 0usize;
    while k < seen.len() {
        if let Some(c) = seen.get(k) {
            if *c == 0 {
                return Err(QcCode::GapBad.val());
            }
        }
        k += 1;
    }
    Ok(n)
}

/// 五路敌意参数：必须全部落 Reject，且两条路径一致。
pub fn hostile_routes() -> Result<u32, u32> {
    let probes = [
        QParams::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        QParams::new(0, 0, 0, 0),
        QParams::new(u32::MAX, 0, 0, 0),
        QParams::new(u32::MAX, u32::MAX, 0, u32::MAX),
        QParams::new(0, u32::MAX, u32::MAX, 0),
    ];
    let mut ok = 0u32;
    for p in probes.iter() {
        if classify(p) == Verdict::Reject && reference_verdict(p) == Verdict::Reject {
            ok = ok.saturating_add(1);
        }
    }
    if ok != HOSTILE_ROUTES as u32 {
        return Err(QcCode::HostileBad.val());
    }
    Ok(ok)
}

// ── 20 例判据用例集 ──────────────────────────────────────────────
/// 一条用例：编号 / 名称 / 参数 / 期望判词。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    pub id: u8,
    pub name: &'static str,
    pub params: QParams,
    pub expected: Verdict,
}

pub const CASE_TABLE: [Case; CASES] = [
    Case { id: 1, name: "reject_zero_threshold", params: QParams::new(0, 100, 5, 10), expected: Verdict::Reject },
    Case { id: 2, name: "reject_threshold_over", params: QParams::new(251, 100, 5, 10), expected: Verdict::Reject },
    Case { id: 3, name: "reject_gain_zero", params: QParams::new(100, 0, 5, 10), expected: Verdict::Reject },
    Case { id: 4, name: "reject_gain_over", params: QParams::new(100, 201, 5, 10), expected: Verdict::Reject },
    Case { id: 5, name: "reject_force_over", params: QParams::new(100, 100, 10, 10), expected: Verdict::Reject },
    Case { id: 6, name: "reject_hyst_over", params: QParams::new(100, 100, 5, 41), expected: Verdict::Reject },
    Case { id: 7, name: "reject_hyst_above_threshold", params: QParams::new(50, 100, 5, 60), expected: Verdict::Reject },
    Case { id: 8, name: "hit_profile_zero_exact", params: QParams::new(120, 80, 5, 12), expected: Verdict::Hit },
    Case { id: 9, name: "hit_profile_zero_inside", params: QParams::new(125, 75, 5, 14), expected: Verdict::Hit },
    Case { id: 10, name: "hit_profile_one_exact", params: QParams::new(60, 140, 3, 6), expected: Verdict::Hit },
    Case { id: 11, name: "hit_profile_one_inside", params: QParams::new(65, 135, 3, 8), expected: Verdict::Hit },
    Case { id: 12, name: "hit_profile_two_exact", params: QParams::new(200, 45, 7, 20), expected: Verdict::Hit },
    Case { id: 13, name: "hit_profile_three_exact", params: QParams::new(30, 180, 2, 2), expected: Verdict::Hit },
    Case { id: 14, name: "hit_profile_three_low_hyst", params: QParams::new(28, 185, 2, 0), expected: Verdict::Hit },
    Case { id: 15, name: "degrade_threshold_line", params: QParams::new(240, 100, 5, 10), expected: Verdict::Degrade },
    Case { id: 16, name: "degrade_gain_line", params: QParams::new(100, 10, 5, 10), expected: Verdict::Degrade },
    Case { id: 17, name: "degrade_hyst_ratio", params: QParams::new(80, 80, 5, 30), expected: Verdict::Degrade },
    Case { id: 18, name: "miss_between_profiles", params: QParams::new(145, 95, 5, 10), expected: Verdict::Miss },
    Case { id: 19, name: "miss_low_threshold", params: QParams::new(10, 100, 4, 1), expected: Verdict::Miss },
    Case { id: 20, name: "miss_near_profile_two", params: QParams::new(220, 60, 8, 8), expected: Verdict::Miss },
];

/// 用例集摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseStats {
    pub total: u32,
    pub per_verdict: [u32; VERDICTS],
    pub digest: u64,
}

impl CaseStats {
    pub fn covers_all(&self) -> bool {
        let mut i = 0usize;
        while i < self.per_verdict.len() {
            if let Some(n) = self.per_verdict.get(i) {
                if *n == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 用例集自审：条数 → 编号 1..20 连续 → 名称非空唯一 → 期望值双路径复算 →
/// 四判词覆盖 → 摘要。
pub fn case_integrity(table: &[Case]) -> Result<CaseStats, u32> {
    if table.len() != CASES {
        return Err(QcCode::CountBad.val());
    }
    let mut per_verdict = [0u32; VERDICTS];
    let mut fp = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let c = match table.get(i) {
            Some(v) => *v,
            None => return Err(QcCode::CaseBad.val()),
        };
        let want_id = CASE_ID_FIRST as u32 + i as u32;
        if c.id as u32 != want_id {
            return Err(QcCode::CaseBad.val());
        }
        if c.name.is_empty() {
            return Err(QcCode::CaseBad.val());
        }
        // 编号/名称唯一 + 升序（编号连续已隐含升序，此处按显式比较再钉一遍）。
        let mut j = 0usize;
        while j < i {
            if let Some(p) = table.get(j) {
                if p.id == c.id || p.name == c.name {
                    return Err(QcCode::DupBad.val());
                }
                if p.id > c.id {
                    return Err(QcCode::OrderBad.val());
                }
            }
            j += 1;
        }
        // 期望值必须被两条独立路径同时确认。
        let a = classify(&c.params);
        let b = reference_verdict(&c.params);
        if a != c.expected || b != c.expected {
            return Err(QcCode::VerdictBad.val());
        }
        if let Some(slot) = per_verdict.get_mut(c.expected.index() as usize) {
            *slot = slot.saturating_add(1);
        }
        fp = p_core::fold_u32(fp, c.id as u32);
        fp = fold_wstr(fp, c.name);
        fp = fold_params(fp, &c.params);
        fp = p_core::fold_u32(fp, c.expected.index() as u32);
        i += 1;
    }
    // 四判词必须全被覆盖。
    let mut k = 0usize;
    while k < per_verdict.len() {
        if let Some(n) = per_verdict.get(k) {
            if *n == 0 {
                return Err(QcCode::GapBad.val());
            }
        }
        k += 1;
    }
    Ok(CaseStats {
        total: table.len() as u32,
        per_verdict,
        digest: fp,
    })
}

pub fn fold_wstr(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = p_core::fold_byte(x, *b);
    }
    x
}

/// 用例集复跑：三遍摘要必须逐位一致。
pub fn replay_cases() -> Result<u64, u32> {
    let stats = case_integrity(&CASE_TABLE)?;
    let mut h = stats.digest;
    let mut pass = 0u32;
    while pass < REPLAY_PASSES {
        let again = case_integrity(&CASE_TABLE)?;
        if again.digest != stats.digest {
            return Err(QcCode::ReplayBad.val());
        }
        h = p_core::fold_u32(h, pass);
        h = p_core::fold_u32(h, (again.digest >> 32) as u32);
        h = p_core::fold_u32(h, again.digest as u32);
        pass = pass.saturating_add(1);
    }
    if h == 0 {
        return Err(QcCode::ReplayBad.val());
    }
    Ok(h)
}

/// 反例必拒：六种坏用例表各按指定码拒绝。
pub fn tamper_rejected() -> bool {
    let mut t = CASE_TABLE.to_vec();
    t[4].id = 9;
    let d1 = case_integrity(&t).err() == Some(QcCode::CaseBad.val());

    let mut t = CASE_TABLE.to_vec();
    t[7].name = CASE_TABLE[8].name;
    let d2 = case_integrity(&t).err() == Some(QcCode::DupBad.val());

    let t = &CASE_TABLE[..CASES - 1];
    let d3 = case_integrity(t).err() == Some(QcCode::CountBad.val());

    // 期望值写错：编号/名称都对，仅判词不符。
    let mut t = CASE_TABLE.to_vec();
    t[0].expected = Verdict::Miss;
    let d4 = case_integrity(&t).err() == Some(QcCode::VerdictBad.val());

    // 全部 20 例都设成 Hit：四判词覆盖出现缺口。
    let mut t = CASE_TABLE.to_vec();
    let mut i = 0usize;
    while i < t.len() {
        if let Some(slot) = t.get_mut(i) {
            slot.params = QParams::new(120, 80, 5, 12);
            slot.expected = Verdict::Hit;
        }
        i += 1;
    }
    let d5 = case_integrity(&t).err() == Some(QcCode::GapBad.val());

    // 参数自相矛盾（滞回 > 阈值）却期望 Hit。
    let mut t = CASE_TABLE.to_vec();
    t[1].params = QParams::new(20, 80, 5, 30);
    t[1].expected = Verdict::Hit;
    let d6 = case_integrity(&t).err() == Some(QcCode::VerdictBad.val());

    d1 && d2 && d3 && d4 && d5 && d6
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < QcCode::ALL.len() {
        if let Some(c) = QcCode::ALL.get(i) {
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

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
/// params=fuzz 轮数 / schedule=20 例 / fails=复算不一致数 / keys=判词种数。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn row_fields(row: &str) -> Option<[&str; RENDER_FIELDS]> {
    let mut out: [&str; RENDER_FIELDS] = [""; RENDER_FIELDS];
    let mut n = 0usize;
    for p in row.split('|') {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = out.get_mut(n) {
            *slot = p;
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    Some(out)
}

pub fn split_kv(part: &str) -> Option<(&str, &str)> {
    let mut it = part.split('=');
    let k = it.next()?;
    let v = it.next()?;
    if it.next().is_some() || k.is_empty() {
        return None;
    }
    Some((k, v))
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

pub fn audit_render(row: &str) -> bool {
    let parts = match row_fields(row) {
        Some(p) => p,
        None => return false,
    };
    let tag = match parts.get(0) {
        Some(v) if !v.is_empty() => *v,
        _ => return false,
    };
    const KEYS: [&str; 4] = ["params", "schedule", "fails", "keys"];
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let (k, v) = match split_kv(p) {
            Some(x) => x,
            None => return false,
        };
        if k != KEYS.get(i - 1).copied().unwrap_or("") {
            return false;
        }
        let num = match parse_u32(v) {
            Some(n) => n,
            None => return false,
        };
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let (dk, dv) = match split_kv(dpart) {
        Some(x) => x,
        None => return false,
    };
    if dk != "digest" {
        return false;
    }
    let digest = match parse_hex16(dv) {
        Some(v) => v,
        None => return false,
    };
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49017 ①：fuzz 万轮零崩（轮数守恒、四类判词都有）。
    let fuzz = fuzz_rounds(FUZZ_ROUNDS);
    let fuzz_ok = match &fuzz {
        Ok(l) => l.rounds == FUZZ_ROUNDS && l.conserved() && l.reject > 0 && l.hit > 0,
        Err(_) => false,
    };
    out.push((
        "F49017/万轮零崩",
        fuzz_ok,
        format!(
            "rounds={:?} per={:?} digest={:016X}",
            fuzz.as_ref().map(|l| l.rounds),
            fuzz.as_ref().map(|l| l.per_verdict()),
            fuzz.as_ref().map(|l| l.digest()).unwrap_or(0)
        ),
    ));

    // F49017 ②：分类有界（四判词都被探针打到，两条路径一致）。
    let bounded = bounded_probe();
    out.push((
        "F49017/分类有界",
        matches!(&bounded, Ok(n) if *n == 10),
        format!("probes={:?} verdicts={}", bounded.as_ref().ok(), VERDICTS),
    ));

    // F49017 ③：独立复算逐轮一致（两遍 fuzz 摘要相同、零 mismatch）。
    let fuzz_b = fuzz_rounds(FUZZ_ROUNDS);
    let recompute_ok = match (&fuzz, &fuzz_b) {
        (Ok(a), Ok(b)) => {
            a.mismatches == 0
                && b.mismatches == 0
                && a.digest() == b.digest()
                && a.digest() != 0
        }
        _ => false,
    };
    out.push((
        "F49017/独立复算",
        recompute_ok,
        format!(
            "mismatch={:?} digest_a={:016X} digest_b={:016X}",
            fuzz.as_ref().map(|l| l.mismatches),
            fuzz.as_ref().map(|l| l.digest()).unwrap_or(0),
            fuzz_b.as_ref().map(|l| l.digest()).unwrap_or(0)
        ),
    ));

    // F49017 ④：敌意参数不崩（五路全拒 + 极端值仍落 Reject）。
    let hostile = hostile_routes();
    let extr_ok = classify(&QParams::new(0, 0, 0, 0)) == Verdict::Reject
        && classify(&QParams::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX)) == Verdict::Reject
        && classify(&QParams::new(250, 200, 9, 40)) != Verdict::Reject;
    out.push((
        "F49017/敌意不崩",
        matches!(&hostile, Ok(n) if *n == HOSTILE_ROUTES as u32) && extr_ok,
        format!("routes={:?} extremes={}", hostile.as_ref().ok(), extr_ok),
    ));

    // F49018 ①：20 例入库（编号 1..20、名称唯一、四判词覆盖）。
    let cases = case_integrity(&CASE_TABLE);
    let cases_ok = match &cases {
        Ok(s) => s.total == CASES as u32 && s.covers_all() && s.digest != 0,
        Err(_) => false,
    };
    out.push((
        "F49018/20例入库",
        cases_ok,
        format!(
            "total={:?} per={:?} digest={:016X}",
            cases.as_ref().map(|s| s.total),
            cases.as_ref().map(|s| s.per_verdict),
            cases.as_ref().map(|s| s.digest).unwrap_or(0)
        ),
    ));

    // F49018 ②：期望值复算（每条都被 classify 与 reference 双确认）。
    let mut recomputed = 0u32;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if let Some(c) = CASE_TABLE.get(i) {
            let a = classify(&c.params);
            let b = reference_verdict(&c.params);
            if a == c.expected && b == c.expected {
                recomputed = recomputed.saturating_add(1);
            }
        }
        i += 1;
    }
    out.push((
        "F49018/期望值复算",
        recomputed == CASES as u32,
        format!("recomputed={}/{}", recomputed, CASES),
    ));

    // F49018 ③：复跑摘要稳定（三遍一致）+ 反例必拒。
    let replay_a = replay_cases();
    let replay_b = replay_cases();
    let replay_ok = match (&replay_a, &replay_b) {
        (Ok(a), Ok(b)) => a == b && *a != 0,
        _ => false,
    };
    out.push((
        "F49018/复跑稳定",
        replay_ok && tamper_rejected(),
        format!(
            "passes={} digest={:016X} tamper={}",
            REPLAY_PASSES,
            replay_a.as_ref().copied().unwrap_or(0),
            tamper_rejected()
        ),
    ));

    // F49018 ④：渲染回读 + 段位守卫。
    let digest = replay_a.as_ref().copied().unwrap_or(0);
    let row = render_row(
        "Q49017",
        FUZZ_ROUNDS,
        CASES as u32,
        0,
        VERDICTS as u32,
        digest,
    );
    let render_ok = audit_render(&row)
        && !audit_render("")
        && !audit_render("Q49017|params=10000|schedule=20|fails=0|keys=4")
        && !audit_render(
            "Q49017|params=10000|schedule=20|fails=0|keys=4|digest=0000000000000001|tail=1",
        );
    out.push((
        "F49018/渲染回读",
        render_ok && segment_guard(),
        format!("row={} audit={} seg={}", row, render_ok, segment_guard()),
    ));

    out
}

pub fn run_unx49017_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49018 quirk fuzz 面与 20 例判据用例集");
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
        assert_eq!(QcCode::CountBad.val(), SEG_BASE + 0x01);
        assert_eq!(QcCode::HostileBad.val(), SEG_BASE + 14);
        assert_eq!(QcCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < QcCode::ALL.len() {
            if let Some(c) = QcCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn profile_table_is_sane() {
        assert!(profile_integrity(&PROFILE_TABLE).unwrap() != 0);
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < PROFILE_TABLE.len() {
            assert!(PROFILE_TABLE[i].threshold >= THRESHOLD_MIN);
            assert!(PROFILE_TABLE[i].threshold <= THRESHOLD_MAX);
            assert!(PROFILE_TABLE[i].gain <= GAIN_MAX);
            assert!(PROFILE_TABLE[i].force <= FORCE_MAX);
            assert!(PROFILE_TABLE[i].hysteresis <= HYST_MAX);
            assert!(PROFILE_TABLE[i].threshold > prev);
            prev = PROFILE_TABLE[i].threshold;
            i += 1;
        }
    }

    #[test]
    fn reject_cases_are_rejected() {
        let mut i = 0usize;
        while i < 7 {
            assert_eq!(CASE_TABLE[i].expected, Verdict::Reject);
            assert_eq!(classify(&CASE_TABLE[i].params), Verdict::Reject);
            assert_eq!(
                reference_verdict(&CASE_TABLE[i].params),
                Verdict::Reject
            );
            i += 1;
        }
    }

    #[test]
    fn hit_cases_are_hits() {
        let mut i = 7usize;
        while i < 14 {
            assert_eq!(CASE_TABLE[i].expected, Verdict::Hit);
            assert_eq!(classify(&CASE_TABLE[i].params), Verdict::Hit);
            assert_eq!(reference_verdict(&CASE_TABLE[i].params), Verdict::Hit);
            i += 1;
        }
    }

    #[test]
    fn degrade_cases_are_degrades() {
        let mut i = 14usize;
        while i < 17 {
            assert_eq!(CASE_TABLE[i].expected, Verdict::Degrade);
            assert_eq!(classify(&CASE_TABLE[i].params), Verdict::Degrade);
            assert_eq!(
                reference_verdict(&CASE_TABLE[i].params),
                Verdict::Degrade
            );
            i += 1;
        }
    }

    #[test]
    fn miss_cases_are_misses() {
        let mut i = 17usize;
        while i < 20 {
            assert_eq!(CASE_TABLE[i].expected, Verdict::Miss);
            assert_eq!(classify(&CASE_TABLE[i].params), Verdict::Miss);
            assert_eq!(reference_verdict(&CASE_TABLE[i].params), Verdict::Miss);
            i += 1;
        }
    }

    #[test]
    fn both_paths_agree_on_every_case() {
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            assert_eq!(
                classify(&CASE_TABLE[i].params),
                reference_verdict(&CASE_TABLE[i].params),
                "case {}",
                CASE_TABLE[i].name
            );
            i += 1;
        }
    }

    #[test]
    fn fuzz_ledger_conserves_all_verdicts() {
        let l = fuzz_rounds(FUZZ_ROUNDS).unwrap();
        assert_eq!(l.rounds, FUZZ_ROUNDS);
        assert!(l.conserved());
        assert_eq!(l.mismatches, 0);
        assert!(l.hit > 0 && l.miss > 0 && l.degrade > 0 && l.reject > 0);
        assert!(fuzz_rounds(0).is_err());
        assert!(fuzz_rounds(FUZZ_MAX + 1).is_err());
    }

    #[test]
    fn fuzz_matches_independent_recompute() {
        let l = fuzz_rounds(2_000).unwrap();
        assert_eq!(l.mismatches, 0);
    }

    #[test]
    fn fuzz_replay_is_stable() {
        let a = fuzz_rounds(2_000).unwrap();
        let b = fuzz_rounds(2_000).unwrap();
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
    }

    #[test]
    fn extremes_are_always_rejected() {
        assert_eq!(
            classify(&QParams::new(0, 0, 0, 0)),
            Verdict::Reject
        );
        assert_eq!(
            classify(&QParams::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX)),
            Verdict::Reject
        );
        assert_eq!(
            classify(&QParams::new(0, u32::MAX, 0, 0)),
            Verdict::Reject
        );
        assert_eq!(
            classify(&QParams::new(100, 100, 5, 101)),
            Verdict::Reject
        );
    }

    #[test]
    fn hostile_routes_all_reject() {
        assert_eq!(hostile_routes().unwrap(), HOSTILE_ROUTES as u32);
        assert_eq!(bounded_probe().unwrap(), 10);
    }

    #[test]
    fn case_registry_is_self_consistent() {
        let s = case_integrity(&CASE_TABLE).unwrap();
        assert_eq!(s.total, 20);
        assert!(s.covers_all());
        assert_ne!(s.digest, 0);
        assert_eq!(s.per_verdict, [7, 3, 3, 7]);
    }

    #[test]
    fn case_names_and_ids_are_unique() {
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            assert_eq!(CASE_TABLE[i].id, i as u8 + 1);
            let mut j = i + 1;
            while j < CASE_TABLE.len() {
                assert_ne!(CASE_TABLE[i].name, CASE_TABLE[j].name);
                assert_ne!(CASE_TABLE[i].id, CASE_TABLE[j].id);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn bad_case_tables_are_rejected() {
        assert!(tamper_rejected());
        let mut t = CASE_TABLE.to_vec();
        t[0].expected = Verdict::Hit;
        assert_eq!(
            case_integrity(&t).err(),
            Some(QcCode::VerdictBad.val())
        );
        let mut t = CASE_TABLE.to_vec();
        t[9].name = "";
        assert_eq!(case_integrity(&t).err(), Some(QcCode::CaseBad.val()));
    }

    #[test]
    fn case_replay_is_stable() {
        let a = replay_cases().unwrap();
        let b = replay_cases().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let row = render_row("Q49017", 10_000, 20, 0, 4, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&row));
        assert!(!audit_render(""));
        assert!(!audit_render("Q49017|params=10000|schedule=20|fails=0|keys=4"));
        assert!(!audit_render(
            "Q49017|params=10000|schedule=20|fails=0|keys=4|digest=000000000000000"
        ));
        assert!(!audit_render(
            "Q49017|params=10000|schedule=20|fails=0|keyz=4|digest=0000000000000001"
        ));
        assert_eq!(parse_u32("10000"), Some(10_000));
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32("100000000000"), None);
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789ABCDEF0"), None);
        assert_eq!(row_fields(&row).map(|f| f.len()), Some(6));
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49017_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(extra_checks().len(), 8);
    }
}
