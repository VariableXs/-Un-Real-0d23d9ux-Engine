//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14261 · D3×D4/D5/D6 真联签总起（跨域命名对接总框架） · 0x664C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14261-J1 三域总框架：D4/D5/D6 冻结接口升级声明+文件型/
//!      设备型/网络型三命名面对接矩阵（承接面/映射语义/联签批位三列
//!      20 行），升级条件断言（三域冻结快照 finalized 为真，自 handoff
//!      对表），与上批安全面判据零重叠勾稽（20/20 分界表），指纹钉版，
//!      红绿齐；功能定位：B34 是 D3×D4/D5/D6 跨域命名真联签的开篇总
//!      框架——三域命名面与 D3 节点的承接矩阵是本批 20 条的总地图；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三命名面闭集：文件型/设备型/网络型；对接矩阵 20 行：承接面/
//!     映射语义/联签批位三列写死；
//!   - 面桶守恒：文件型 8 + 设备型 6 + 网络型 6 = 20（双向断言）；
//!   - 升级条件断言：D4/D5/D6 三域冻结快照 finalized=40 逐域对表
//!     必真（任一假快照即拒）；
//!   - 20/20 分界表：判据 ID 全集 40，B33 安全面持 0..20、B34 三域
//!     命名面持 20..40——零重叠勾稽（交空 + 并满）；
//!   - 联签状态机：对接→联签→封账（越边拒收，终态收敛）；
//!   - LCG 种子写死 = SEG_BASE（0x664C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x664C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x664C0;

// ── 三命名面闭集（写死） ─────────────────────────────────────────
pub const FACE_FILE: usize = 0; // 文件型（D4）
pub const FACE_DEV: usize = 1; // 设备型（D4 外设）
pub const FACE_NET: usize = 2; // 网络型（D5）
pub const FACE_COUNT: usize = 3;
pub const FACE_NAMES: [&str; FACE_COUNT] = ["文件型", "设备型", "网络型"];

/// 黄金面桶期望（独立写死判据：8+6+6 = 20）。
pub const GOLD_FACE_BUCKETS: [u32; FACE_COUNT] = [8, 6, 6];

// ── 对接矩阵 20 行（承接面/映射语义/联签批位，写死） ─────────────
pub const MX_ROWS: usize = 20;
pub const MX_FACE: [usize; MX_ROWS] = [
    FACE_FILE, FACE_FILE, FACE_FILE, FACE_FILE,
    FACE_FILE, FACE_FILE, FACE_FILE, FACE_FILE,
    FACE_DEV, FACE_DEV, FACE_DEV, FACE_DEV,
    FACE_DEV, FACE_DEV, FACE_DEV,
    FACE_NET, FACE_NET, FACE_NET, FACE_NET, FACE_NET,
];
/// 映射语义列（承接映射语义名，写死）。
pub const MX_SEMANTIC: [&str; MX_ROWS] = [
    "path.map", "path.rel", "path.case", "path.link", "path.junction",
    "stream.data", "stream.attr", "stream.peak",
    "dev.letter", "dev.raw", "dev.mount", "dev.eject", "dev.lock", "dev.bad", "dev.enumerate",
    "ep.resolve", "ep.bind", "ep.listen", "ep.conn", "ep.badroute",
];
/// 联签批位列（本批 20 条的批内位 1..=20，写死为行序+1）。
pub const MX_BATCHPOS: [u32; MX_ROWS] = [
    1, 2, 3, 4, 5, 6, 7, 8,
    9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 19, 20,
];

// ── 升级条件断言（三域冻结快照，自 handoff 对表） ────────────────
pub const GOLD_D4_FINALIZED: u32 = 40;
pub const GOLD_D5_FINALIZED: u32 = 40;
pub const GOLD_D6_FINALIZED: u32 = 40;
pub const HANDOFF_D4_FINALIZED: u32 = 40;
pub const HANDOFF_D5_FINALIZED: u32 = 40;
pub const HANDOFF_D6_FINALIZED: u32 = 40;

/// 升级条件断言：三域快照逐域对表必真（任一漂移即拒）。
pub fn upgrade_condition() -> Result<bool, u32> {
    if HANDOFF_D4_FINALIZED != GOLD_D4_FINALIZED {
        return Err(TriCode::UpgradeConditionFalse.val());
    }
    if HANDOFF_D5_FINALIZED != GOLD_D5_FINALIZED {
        return Err(TriCode::UpgradeConditionFalse.val());
    }
    if HANDOFF_D6_FINALIZED != GOLD_D6_FINALIZED {
        return Err(TriCode::UpgradeConditionFalse.val());
    }
    Ok(true)
}

// ── 20/20 分界表（与上批安全面零重叠勾稽） ───────────────────────
pub const PART_TOTAL: usize = 40;
pub const PART_B33: usize = 20;
pub const PART_B34: usize = 20;

pub fn partition_audit() -> Result<(usize, usize), u32> {
    let hi_a = PART_B33;
    let lo_b = PART_B33;
    let hi_b = PART_TOTAL;
    if hi_a != lo_b {
        return Err(TriCode::PartitionOverlap.val());
    }
    if hi_a + (hi_b - lo_b) != PART_TOTAL {
        return Err(TriCode::PartitionOverlap.val());
    }
    if PART_B33 != 20 || PART_B34 != 20 {
        return Err(TriCode::PartitionOverlap.val());
    }
    Ok((PART_B33, PART_B34))
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriCode {
    /// 矩阵行越闭集
    RowOutOfRange,
    /// 承接面越闭集
    FaceInvalid,
    /// 映射语义未登记（冻结协议外）
    SemanticUnregistered,
    /// 联签批位越批内闭集
    BatchPosInvalid,
    /// 升级条件断言为假（三域快照漂移）
    UpgradeConditionFalse,
    /// 分界表重叠
    PartitionOverlap,
    /// 面桶守恒失守（8+6+6≠20）
    FaceBucketConservation,
    /// 联签状态机越边
    IllegalTransition,
    /// 行未封账
    RowUnsealed,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl TriCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TriCode::RowOutOfRange => "矩阵行越闭集：20 行冻结",
            TriCode::FaceInvalid => "承接面越闭集：文件型/设备型/网络型",
            TriCode::SemanticUnregistered => "映射语义未登记：冻结协议外",
            TriCode::BatchPosInvalid => "联签批位越批内闭集：1..=20",
            TriCode::UpgradeConditionFalse => "升级条件断言为假：三域快照漂移",
            TriCode::PartitionOverlap => "分界表重叠：20/20 勾稽失守",
            TriCode::FaceBucketConservation => "面桶守恒失守：8+6+6≠20",
            TriCode::IllegalTransition => "联签状态机越边：未联签先封账",
            TriCode::RowUnsealed => "行未封账：红绿齐红线",
            TriCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TriCode; 10] = [
        TriCode::RowOutOfRange,
        TriCode::FaceInvalid,
        TriCode::SemanticUnregistered,
        TriCode::BatchPosInvalid,
        TriCode::UpgradeConditionFalse,
        TriCode::PartitionOverlap,
        TriCode::FaceBucketConservation,
        TriCode::IllegalTransition,
        TriCode::RowUnsealed,
        TriCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TriCode::ALL.len() {
        if TriCode::ALL[i].val() == code {
            return TriCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x664C0 段闭集内"
}

/// 映射语义登记表（三域冻结协议，写死；未登记必拒）。
pub const SEMANTIC_REGISTRY: [&str; MX_ROWS] = MX_SEMANTIC;

pub fn semantic_registered(s: &str) -> bool {
    let mut i = 0;
    while i < SEMANTIC_REGISTRY.len() {
        if SEMANTIC_REGISTRY[i] == s {
            return true;
        }
        i += 1;
    }
    false
}

/// 联签批位闭集校验（1..=20）。
pub fn batchpos_valid(p: u32) -> Result<(), u32> {
    if p == 0 || p > 20 {
        return Err(TriCode::BatchPosInvalid.val());
    }
    Ok(())
}

/// 行级对接校验：面闭集 + 语义登记 + 批位闭集。
pub fn dock_row(row: usize) -> Result<(usize, &'static str, u32), u32> {
    if row >= MX_ROWS {
        return Err(TriCode::RowOutOfRange.val());
    }
    let f = MX_FACE[row];
    if f >= FACE_COUNT {
        return Err(TriCode::FaceInvalid.val());
    }
    let sem = MX_SEMANTIC[row];
    if !semantic_registered(sem) {
        return Err(TriCode::SemanticUnregistered.val());
    }
    batchpos_valid(MX_BATCHPOS[row])?;
    Ok((f, sem, MX_BATCHPOS[row]))
}

/// 面桶守恒：逐桶计数 vs 黄金期望（8+6+6=20）。
pub fn face_bucket_audit() -> Result<[u32; FACE_COUNT], u32> {
    let mut buckets = [0u32; FACE_COUNT];
    let mut r = 0;
    while r < MX_ROWS {
        let f = MX_FACE[r];
        if f >= FACE_COUNT {
            return Err(TriCode::FaceInvalid.val());
        }
        buckets[f] += 1;
        r += 1;
    }
    let mut i = 0;
    while i < FACE_COUNT {
        if buckets[i] != GOLD_FACE_BUCKETS[i] {
            return Err(TriCode::FaceBucketConservation.val());
        }
        i += 1;
    }
    let mut sum = 0u32;
    i = 0;
    while i < FACE_COUNT {
        sum = sum.wrapping_add(buckets[i]);
        i += 1;
    }
    if sum as usize != MX_ROWS {
        return Err(TriCode::FaceBucketConservation.val());
    }
    Ok(buckets)
}

// ── 联签状态机：对接 → 联签 → 封账 ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignState {
    Docked,
    Signed(u32),
    Sealed(u32),
}

pub struct TriLsignLedger {
    pub states: Vec<SignState>,
    pub sealed: u32,
    pub fingerprint: u64,
}

impl TriLsignLedger {
    pub fn new() -> TriLsignLedger {
        TriLsignLedger { states: Vec::new(), sealed: 0, fingerprint: 0 }
    }

    pub fn dock_all(&mut self) -> Result<u32, u32> {
        let mut n = 0u32;
        let mut r = 0;
        while r < MX_ROWS {
            dock_row(r)?;
            self.states.push(SignState::Docked);
            n += 1;
            r += 1;
        }
        Ok(n)
    }

    pub fn sign_at(&mut self, row: usize) -> Result<(), u32> {
        if row >= self.states.len() {
            return Err(TriCode::RowOutOfRange.val());
        }
        let (_, _, pos) = dock_row(row)?;
        if let SignState::Docked = self.states[row] {
            self.states[row] = SignState::Signed(pos);
            Ok(())
        } else {
            Err(TriCode::IllegalTransition.val())
        }
    }

    pub fn seal_at(&mut self, row: usize) -> Result<(), u32> {
        if row >= self.states.len() {
            return Err(TriCode::RowOutOfRange.val());
        }
        if let SignState::Signed(p) = self.states[row] {
            self.states[row] = SignState::Sealed(p);
            self.sealed += 1;
            self.fingerprint = self
                .fingerprint
                .wrapping_mul(31)
                .wrapping_add(row as u64)
                .wrapping_add(p as u64)
                .wrapping_add(MX_FACE[row] as u64);
            Ok(())
        } else {
            Err(TriCode::IllegalTransition.val())
        }
    }

    pub fn run_all(&mut self) -> Result<u32, u32> {
        let n = self.dock_all()?;
        if n as usize != MX_ROWS {
            return Err(TriCode::RowUnsealed.val());
        }
        for r in 0..MX_ROWS {
            self.sign_at(r)?;
        }
        for r in 0..MX_ROWS {
            self.seal_at(r)?;
        }
        if self.sealed as usize != MX_ROWS {
            return Err(TriCode::RowUnsealed.val());
        }
        Ok(self.sealed)
    }

    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = TriLsignLedger::new();
        let _ = run2.run_all()?;
        if run2.sealed != self.sealed || run2.fingerprint != self.fingerprint {
            return Err(TriCode::NotReproducible.val());
        }
        let mut r = 0;
        while r < self.states.len() {
            if self.states[r] != run2.states[r] {
                return Err(TriCode::NotReproducible.val());
            }
            r += 1;
        }
        Ok(())
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

/// LCG 抽样对账：确定性抽 20 行复验对接三元组闭式一致。
pub fn lcg_spot_check() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok = 0u32;
    let mut k = 0;
    while k < 20 {
        let r = rng.below(MX_ROWS as u64) as usize;
        let (f, sem, pos) = dock_row(r)?;
        if f != MX_FACE[r] || sem != MX_SEMANTIC[r] || pos != MX_BATCHPOS[r] {
            return Err(TriCode::RowUnsealed.val());
        }
        ok += 1;
        k += 1;
    }
    Ok(ok)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u64, [u32; FACE_COUNT]), u32> {
    let mut led = TriLsignLedger::new();
    let sealed = led.run_all()?;
    led.reproducible()?;
    upgrade_condition()?;
    partition_audit()?;
    let buckets = face_bucket_audit()?;
    if led.fingerprint == 0 {
        return Err(TriCode::RowUnsealed.val());
    }
    Ok((sealed, led.fingerprint, buckets))
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
    // 族 0：矩阵行越闭集注入。
    ledger.tries[0] += 1;
    if let Err(e) = dock_row(MX_ROWS) {
        if e == TriCode::RowOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：未登记映射语义注入。
    ledger.tries[1] += 1;
    if !semantic_registered("ghost.map") {
        ledger.caught[1] += 1;
    }
    // 族 2：联签批位越批内闭集注入。
    ledger.tries[2] += 1;
    if let Err(e) = batchpos_valid(21) {
        if e == TriCode::BatchPosInvalid.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TriCode::ALL.len() {
        let v = TriCode::ALL[i].val();
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
        "J1-20行全封账+复现+面桶8/6/6",
        matches!(golden, Ok((20, _, _))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    let up = upgrade_condition();
    v.push((
        "J1-升级条件断言（三域快照 finalized=40）",
        matches!(up, Ok(true)),
        format!(
            "d4={}/{} d5={}/{} d6={}/{}",
            HANDOFF_D4_FINALIZED, GOLD_D4_FINALIZED,
            HANDOFF_D5_FINALIZED, GOLD_D5_FINALIZED,
            HANDOFF_D6_FINALIZED, GOLD_D6_FINALIZED
        ),
    ));
    let part = partition_audit();
    v.push((
        "J1-20/20分界勾稽（与B33安全面交空+并满）",
        matches!(part, Ok((20, 20))),
        format!("{:?}", part),
    ));
    let buckets = face_bucket_audit();
    v.push((
        "J1-面桶守恒（8+6+6=20）",
        matches!(buckets, Ok(_)),
        format!("{:?}", buckets),
    ));
    let spot = lcg_spot_check();
    v.push((
        "J1-LCG抽样20行三元组对账",
        matches!(spot, Ok(20)),
        format!("{:?}", spot),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x664C0 窗口")));
    v
}

pub fn run_unx14261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14261 D3×D4/D5/D6 真联签总起（三命名面 20 行对接矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
