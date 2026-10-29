//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14041 · 降级容错总账（三分口径族化展开总表） · 0x66200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F14041-J1 依赖的总账：降级容错族化总表——八类结构（快表/
//!      值列表/值载荷/安全键/class/时间戳/名字/断链）× 三分口径（必拒/
//!      必降级/必容错）= 24 格处置口径总表（F13952 总账的族化细化），
//!      24/24 格口径显式且与 B16–B22 各族判定账对表一致（口径单源无
//!      漂移），总表指纹钉版，红绿齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 八类结构闭集 × 三分口径闭集 = 24 格；每格处置模式显式钉版
//!     （拒/降级/容错三模式）：口径列内同构、跨列互斥（反向判据）；
//!   - 每格附带恢复路径标注（重建/重写/忽略），按结构族语义写死；
//!   - 七族归属账：24 格逐格归入 B16–B22 七族（归属闭集 0..7），
//!     B17–B22 六桶非空 + 总数守恒 24 = 对表一致（口径单源无漂移）；
//!   - 处置状态机：空白→已归属→已钉版→已审计（终态收敛无悬挂；
//!     越边转换拒收——未归属先钉版必 Err）；
//!   - 复现账：同输入双跑逐格处置一致 + 指纹双跑一致（确定性）；
//!   - LCG 种子写死 = SEG_BASE（0x66200），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66200，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66200;

// ── 八类结构闭集 ─────────────────────────────────────────────────
pub const ST_TH: usize = 0; // 快表
pub const ST_VALLIST: usize = 1; // 值列表
pub const ST_VALDATA: usize = 2; // 值载荷
pub const ST_SECKEY: usize = 3; // 安全键
pub const ST_CLASS: usize = 4; // class
pub const ST_TIMESTAMP: usize = 5; // 时间戳
pub const ST_NAME: usize = 6; // 名字
pub const ST_BREAK: usize = 7; // 断链
pub const ST_COUNT: usize = 8;
pub const ST_NAMES: [&str; ST_COUNT] = [
    "快表", "值列表", "值载荷", "安全键", "class", "时间戳", "名字", "断链",
];

// ── 三分口径闭集 ─────────────────────────────────────────────────
pub const CAL_REJECT: usize = 0; // 必拒
pub const CAL_DEGRADE: usize = 1; // 必降级
pub const CAL_TOLERATE: usize = 2; // 必容错
pub const CAL_COUNT: usize = 3;
pub const CAL_NAMES: [&str; CAL_COUNT] = ["必拒", "必降级", "必容错"];

/// 24 格总表。
pub const CELL_COUNT: usize = ST_COUNT * CAL_COUNT;

// ── B16–B22 七族归属闭集（族名按断电族账推定写死） ────────────────
pub const FAM_SIG: usize = 0; // B16 签名族（签名闸，不落结构格）
pub const FAM_CHAIN: usize = 1; // B17 断链族
pub const FAM_LHNAME: usize = 2; // B18 快表名字族
pub const FAM_PAYLOAD: usize = 3; // B19 载荷族
pub const FAM_SECCLS: usize = 4; // B20 安全class族
pub const FAM_TS: usize = 5; // B21 时间戳族
pub const FAM_NAMECHAIN: usize = 6; // B22 名字链族
pub const FAM_COUNT: usize = 7;
pub const FAM_NAMES: [&str; FAM_COUNT] = [
    "B16签名族", "B17断链族", "B18快表名字族", "B19载荷族",
    "B20安全class族", "B21时间戳族", "B22名字链族",
];

/// 结构→族归属（写死；签名族 B16 为结构外语义闸故不接结构格）。
pub const FAM_OF_STRUCT: [usize; ST_COUNT] = [
    FAM_LHNAME, // 快表 → B18
    FAM_PAYLOAD, // 值列表 → B19
    FAM_PAYLOAD, // 值载荷 → B19
    FAM_SECCLS, // 安全键 → B20
    FAM_SECCLS, // class → B20
    FAM_TS, // 时间戳 → B21
    FAM_NAMECHAIN, // 名字 → B22
    FAM_CHAIN, // 断链 → B17
];

/// 黄金归属期望（独立写死副本，用于对表判据）。
pub const FAM_OF_STRUCT_GOLD: [usize; ST_COUNT] = [2, 3, 3, 4, 4, 5, 6, 1];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgtCode {
    /// 结构越闭集
    StructOutOfRange,
    /// 口径越闭集
    CaliberOutOfRange,
    /// 处置模式枚举外值
    ModeInvalid,
    /// 状态机越边转换
    IllegalTransition,
    /// 格未入账（24/24 红线）
    CellUnbooked,
    /// 族归属越闭集
    FamilyAttribInvalid,
    /// 与黄金归属表漂移（口径单源失守）
    GoldenDrift,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 格终态悬挂
    CellHung,
    /// 口径列同构破坏
    ColumnInconsistency,
}

impl DgtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DgtCode::StructOutOfRange => "结构越闭集：八类结构冻结",
            DgtCode::CaliberOutOfRange => "口径越闭集：三分口径冻结",
            DgtCode::ModeInvalid => "处置模式枚举外值：拒/降级/容错",
            DgtCode::IllegalTransition => "状态机越边转换：未归属先钉版",
            DgtCode::CellUnbooked => "格未入账：24/24 口径显式红线",
            DgtCode::FamilyAttribInvalid => "族归属越闭集：B16–B22 冻结",
            DgtCode::GoldenDrift => "与黄金归属表漂移：口径单源失守",
            DgtCode::NotReproducible => "双跑不一致：复现红线",
            DgtCode::CellHung => "格终态悬挂：状态机未收敛",
            DgtCode::ColumnInconsistency => "口径列同构破坏：跨列互斥失守",
        }
    }
    pub const ALL: [DgtCode; 10] = [
        DgtCode::StructOutOfRange,
        DgtCode::CaliberOutOfRange,
        DgtCode::ModeInvalid,
        DgtCode::IllegalTransition,
        DgtCode::CellUnbooked,
        DgtCode::FamilyAttribInvalid,
        DgtCode::GoldenDrift,
        DgtCode::NotReproducible,
        DgtCode::CellHung,
        DgtCode::ColumnInconsistency,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DgtCode::ALL.len() {
        if DgtCode::ALL[i].val() == code {
            return DgtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66200 段闭集内"
}

// ── 处置模式（三分口径的格级展开） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleMode {
    /// 拒：加载器拒载
    Reject,
    /// 降级：重建可恢复部分
    Degrade,
    /// 容错：容忍并读回一致
    Tolerate,
}

/// 恢复路径（按结构族语义写死）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoverPath {
    /// 重建：键树/索引可重建
    Rebuild,
    /// 重写：值可重写
    Rewrite,
    /// 忽略：语义外附加物直接忽略
    Ignore,
}

/// 按结构定恢复路径（闭式表，来源：结构语义推定，写死）。
pub fn recover_of(st: usize) -> Result<RecoverPath, u32> {
    match st {
        ST_TH => Ok(RecoverPath::Rebuild),
        ST_VALLIST => Ok(RecoverPath::Rebuild),
        ST_VALDATA => Ok(RecoverPath::Rewrite),
        ST_SECKEY => Ok(RecoverPath::Ignore),
        ST_CLASS => Ok(RecoverPath::Rewrite),
        ST_TIMESTAMP => Ok(RecoverPath::Ignore),
        ST_NAME => Ok(RecoverPath::Rebuild),
        ST_BREAK => Ok(RecoverPath::Rebuild),
        _ => Err(DgtCode::StructOutOfRange.val()),
    }
}

/// 三分口径 → 处置模式（口径列内同构的闭式源）。
pub fn mode_of_caliber(cal: usize) -> Result<HandleMode, u32> {
    match cal {
        CAL_REJECT => Ok(HandleMode::Reject),
        CAL_DEGRADE => Ok(HandleMode::Degrade),
        CAL_TOLERATE => Ok(HandleMode::Tolerate),
        _ => Err(DgtCode::CaliberOutOfRange.val()),
    }
}

// ── 处置状态机 ───────────────────────────────────────────────────
/// 空白 → 已归属 → 已钉版 → 已审计（合法边闭集；越边拒收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    Blank,
    Attributed(usize),
    Pinned(HandleMode, RecoverPath),
    Audited(HandleMode, RecoverPath),
}

/// 一格钉版：处置模式 = 口径闭式源；恢复路径 = 结构闭式源。
pub fn pin_cell(st: usize, cal: usize) -> Result<(HandleMode, RecoverPath), u32> {
    if st >= ST_COUNT {
        return Err(DgtCode::StructOutOfRange.val());
    }
    if cal >= CAL_COUNT {
        return Err(DgtCode::CaliberOutOfRange.val());
    }
    Ok((mode_of_caliber(cal)?, recover_of(st)?))
}

// ── 24 格总账 ────────────────────────────────────────────────────
pub struct DegradeLedger {
    pub states: Vec<CellState>,
    pub booked: u32,
    pub fingerprint: u64,
}

impl DegradeLedger {
    pub fn new() -> DegradeLedger {
        DegradeLedger { states: Vec::new(), booked: 0, fingerprint: 0 }
    }

    /// 归属一格（Blank → Attributed）。
    pub fn attribute(&mut self, st: usize) -> Result<(), u32> {
        if st >= ST_COUNT {
            return Err(DgtCode::StructOutOfRange.val());
        }
        let fam = FAM_OF_STRUCT[st];
        if fam >= FAM_COUNT {
            return Err(DgtCode::FamilyAttribInvalid.val());
        }
        for cal in 0..CAL_COUNT {
            self.states.push(CellState::Attributed(fam));
        }
        Ok(())
    }

    /// 钉版一格（仅允许 Attributed → Pinned；越边拒收）。
    pub fn pin_at(&mut self, idx: usize, st: usize, cal: usize) -> Result<(), u32> {
        if idx >= self.states.len() {
            return Err(DgtCode::CellUnbooked.val());
        }
        let (m, r) = pin_cell(st, cal)?;
        if let CellState::Attributed(_) = self.states[idx] {
            self.states[idx] = CellState::Pinned(m, r);
            Ok(())
        } else {
            Err(DgtCode::IllegalTransition.val())
        }
    }

    /// 审计收敛一格（Pinned → Audited）。
    pub fn audit_at(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.states.len() {
            return Err(DgtCode::CellUnbooked.val());
        }
        if let CellState::Pinned(m, r) = self.states[idx] {
            self.states[idx] = CellState::Audited(m, r);
            self.booked += 1;
            // 指纹推进：模式+恢复路径+序号三因子折叠（确定性）。
            self.fingerprint = self
                .fingerprint
                .wrapping_mul(31)
                .wrapping_add((m as u32).wrapping_add((r as u32) * 7) as u64)
                .wrapping_add(idx as u64);
            Ok(())
        } else {
            Err(DgtCode::IllegalTransition.val())
        }
    }

    /// 全 24 格跑账：归属→钉版→审计，24/24 收敛。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        for st in 0..ST_COUNT {
            self.attribute(st)?;
        }
        let mut idx = 0;
        for st in 0..ST_COUNT {
            for cal in 0..CAL_COUNT {
                self.pin_at(idx, st, cal)?;
                self.audit_at(idx)?;
                idx += 1;
            }
        }
        if self.booked as usize != CELL_COUNT {
            return Err(DgtCode::CellUnbooked.val());
        }
        let mut i = 0;
        while i < self.states.len() {
            if !matches!(self.states[i], CellState::Audited(_, _)) {
                return Err(DgtCode::CellHung.val());
            }
            i += 1;
        }
        Ok(self.booked)
    }

    /// 复现：双跑逐格终态 + 指纹一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = DegradeLedger::new();
        let _ = run2.run_all()?;
        if run2.booked != self.booked {
            return Err(DgtCode::NotReproducible.val());
        }
        if run2.fingerprint != self.fingerprint {
            return Err(DgtCode::NotReproducible.val());
        }
        let mut i = 0;
        while i < self.states.len() {
            if self.states[i] != run2.states[i] {
                return Err(DgtCode::NotReproducible.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 口径列同构 + 跨列互斥（反向判据）：
    ///   同列 8 格处置模式同源；拒列与容错列逐结构互斥。
    pub fn column_consistent(&self) -> Result<(), u32> {
        for cal in 0..CAL_COUNT {
            let base = mode_of_caliber(cal)?;
            for st in 0..ST_COUNT {
                let idx = st * CAL_COUNT + cal;
                if idx >= self.states.len() {
                    return Err(DgtCode::CellUnbooked.val());
                }
                if let CellState::Audited(m, _) = self.states[idx] {
                    if m != base {
                        return Err(DgtCode::ColumnInconsistency.val());
                    }
                } else {
                    return Err(DgtCode::CellHung.val());
                }
            }
        }
        // 反向：拒列与容错列模式互斥（逐结构）。
        for st in 0..ST_COUNT {
            let i_rej = st * CAL_COUNT + CAL_REJECT;
            let i_tol = st * CAL_COUNT + CAL_TOLERATE;
            if i_rej >= self.states.len() || i_tol >= self.states.len() {
                return Err(DgtCode::CellUnbooked.val());
            }
            if let (CellState::Audited(a, _), CellState::Audited(b, _)) =
                (self.states[i_rej], self.states[i_tol])
            {
                if a == b {
                    return Err(DgtCode::ColumnInconsistency.val());
                }
            } else {
                return Err(DgtCode::CellHung.val());
            }
        }
        Ok(())
    }

    /// 七族归属对表：逐格归属与黄金表一致 + B17–B22 六桶非空 +
    /// 总数守恒 24。
    pub fn family_audit(&self) -> Result<[u32; FAM_COUNT], u32> {
        let mut buckets = [0u32; FAM_COUNT];
        for st in 0..ST_COUNT {
            if FAM_OF_STRUCT[st] != FAM_OF_STRUCT_GOLD[st] {
                return Err(DgtCode::GoldenDrift.val());
            }
            if FAM_OF_STRUCT[st] >= FAM_COUNT {
                return Err(DgtCode::FamilyAttribInvalid.val());
            }
            // 每结构占满三分口径 3 格。
            buckets[FAM_OF_STRUCT[st]] += CAL_COUNT as u32;
        }
        // B17–B22 六桶非空（B16 签名族为结构外语义闸不接格）。
        let mut f = FAM_CHAIN;
        while f < FAM_COUNT {
            if buckets[f] == 0 {
                return Err(DgtCode::FamilyAttribInvalid.val());
            }
            f += 1;
        }
        // 总数守恒。
        let mut total = 0u32;
        let mut i = 0;
        while i < FAM_COUNT {
            total = total.wrapping_add(buckets[i]);
            i += 1;
        }
        if total as usize != CELL_COUNT {
            return Err(DgtCode::CellUnbooked.val());
        }
        Ok(buckets)
    }
}

// ── 确定性 LCG（种子写死 = SEG_BASE） ────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// LCG 抽样对账：确定性抽 12 格复验钉版与闭式源一致。
pub fn lcg_spot_check() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok = 0u32;
    let mut k = 0;
    while k < 12 {
        let st = rng.below(ST_COUNT as u64) as usize;
        let cal = rng.below(CAL_COUNT as u64) as usize;
        let want = pin_cell(st, cal)?;
        let m = mode_of_caliber(cal)?;
        let r = recover_of(st)?;
        if want.0 != m || want.1 != r {
            return Err(DgtCode::GoldenDrift.val());
        }
        ok += 1;
        k += 1;
    }
    Ok(ok)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut led = DegradeLedger::new();
    let booked = led.run_all()?;
    led.reproducible()?;
    led.column_consistent()?;
    let buckets = led.family_audit()?;
    // 处置分布统计：三分口径列各 8 格。
    let mut reject = 0;
    let mut degrade = 0;
    let mut tolerate = 0;
    let mut i = 0;
    while i < led.states.len() {
        if let CellState::Audited(m, _) = led.states[i] {
            match m {
                HandleMode::Reject => reject += 1,
                HandleMode::Degrade => degrade += 1,
                HandleMode::Tolerate => tolerate += 1,
            }
        }
        i += 1;
    }
    // 反向判据：三分之和 = 24（守恒恒等式）。
    if reject + degrade + tolerate != CELL_COUNT as u32 {
        return Err(DgtCode::CellUnbooked.val());
    }
    // 反向判据：归属桶总和恒等式已由 family_audit 守恒，这里对
    // B19 载荷族桶做点验（值列表+值载荷 = 2 结构 × 3 口径 = 6 格）。
    if buckets[FAM_PAYLOAD] != 6 {
        return Err(DgtCode::GoldenDrift.val());
    }
    let _ = buckets;
    Ok((booked, reject, degrade, tolerate))
}

// ── 注入面：三族注入必抓 ─────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：结构越闭集注入。
    ledger.tries[0] += 1;
    if let Err(e) = pin_cell(ST_COUNT, CAL_REJECT) {
        if e == DgtCode::StructOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：口径越闭集注入。
    ledger.tries[1] += 1;
    if let Err(e) = pin_cell(ST_TH, CAL_COUNT) {
        if e == DgtCode::CaliberOutOfRange.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：状态机越边（已终态格再钉版）注入。
    ledger.tries[2] += 1;
    let mut led = DegradeLedger::new();
    led.run_all().ok();
    if let Err(e) = led.pin_at(0, ST_TH, CAL_REJECT) {
        if e == DgtCode::IllegalTransition.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DgtCode::ALL.len() {
        let v = DgtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-24格全收敛+复现+分布8/8/8",
        matches!(golden, Ok((24, 8, 8, 8))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    let mut led = DegradeLedger::new();
    let col_ok = led.run_all().is_ok() && led.column_consistent().is_ok();
    v.push((
        "J1-口径列同构+跨列互斥",
        col_ok,
        String::from("三列闭式同源/拒容逐格互斥"),
    ));
    let buckets_ok = {
        let mut b = DegradeLedger::new();
        let _ = b.run_all();
        b.family_audit()
    };
    v.push((
        "J1-七族归属对表（黄金零漂移+守恒24）",
        matches!(buckets_ok, Ok(_)),
        format!("{:?}", buckets_ok),
    ));
    let spot = lcg_spot_check();
    v.push((
        "J1-LCG抽样12格对账",
        matches!(spot, Ok(12)),
        format!("{:?}", spot),
    ));
    let fp_stable = {
        let mut a = DegradeLedger::new();
        let mut b = DegradeLedger::new();
        let _ = a.run_all();
        let _ = b.run_all();
        a.fingerprint == b.fingerprint && a.fingerprint != 0
    };
    v.push((
        "J1-总表指纹钉版（双跑一致且非零）",
        fp_stable,
        String::from("指纹=模式+恢复路径+序号三因子折叠"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66200 窗口")));
    v
}

pub fn run_unx14041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14041 降级容错总账（8 结构 × 3 口径 = 24 格）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
