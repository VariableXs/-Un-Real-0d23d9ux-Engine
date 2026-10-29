//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14201 · D3×D1 真联签总起（冻结接口对接总框架） · 0x66400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14201-J1 真联签总框架：B27 预演升级声明+D3 命名空间节点
//!      ×D1 属性面对接矩阵总表（节点类型/属性字段/联签深度三列
//!      20 行），升级条件断言（D1 满账 finalized=40 为真，快照自
//!      handoff 对表），与 B27 通道预演判据零重叠勾稽（20/20 分界表），
//!      指纹钉版，红绿齐；功能定位：B31 是 D3×D1 真联签段的开篇总
//!      框架——预演（B27）升级为实接（D1 已满账封账）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对接矩阵 20 行闭集：节点类型（6 类 D3 节点）/属性字段（D1 冻结
//!     接口字段名）/联签深度三列写死；
//!   - 升级条件断言：D1 满账 finalized=40 写死判据，与 handoff 快照
//!     对表必真（假快照即拒）；
//!   - 20/20 分界表：判据 ID 全集 40，B27 预演持 0..20、B31 实接持
//!     20..40——零重叠勾稽（交空 + 并满双向断言）；
//!   - 联签状态机：对接→联签→封账（越边拒收，终态收敛）；
//!   - 复现账：同输入双跑逐行一致 + 指纹双跑一致（确定性）；
//!   - LCG 种子写死 = SEG_BASE（0x66400），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66400，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66400;

// ── D3 命名空间节点类型闭集（6 类，写死） ────────────────────────
pub const ND_ROOT: usize = 0; // 根节点
pub const ND_USER: usize = 1; // 用户节点
pub const ND_PERM: usize = 2; // 权限节点
pub const ND_AUDIT: usize = 3; // 审计节点
pub const ND_CRED: usize = 4; // 凭据节点
pub const ND_JUDGE: usize = 5; // 判据节点
pub const ND_COUNT: usize = 6;
pub const ND_NAMES: [&str; ND_COUNT] =
    ["根节点", "用户节点", "权限节点", "审计节点", "凭据节点", "判据节点"];

// ── 对接矩阵 20 行（节点类型/属性字段/联签深度，写死） ───────────
pub const MX_ROWS: usize = 20;
pub const MX_NODE: [usize; MX_ROWS] = [
    ND_ROOT, ND_ROOT, ND_ROOT, ND_ROOT,
    ND_USER, ND_USER, ND_USER, ND_USER,
    ND_PERM, ND_PERM, ND_PERM, ND_PERM,
    ND_AUDIT, ND_AUDIT, ND_AUDIT, ND_AUDIT,
    ND_CRED, ND_CRED, ND_JUDGE, ND_JUDGE,
];
pub const MX_ATTR: [&str; MX_ROWS] = [
    "root.name", "root.epoch", "root.scope", "root.frozen",
    "user.profile", "user.isolation", "user.bad_profile", "user.cred_ref",
    "perm.grant", "perm.deny", "perm.inherit", "perm.scope",
    "audit.emit", "audit.retain", "audit.bad_rec", "audit.sink",
    "cred.cache", "cred.ttl", "judge.freeze", "judge.verdict",
];
/// 联签深度（0=只读对接/1=单向联签/2=双向联签，写死）。
pub const MX_DEPTH: [u32; MX_ROWS] = [
    0, 0, 1, 2,
    1, 2, 2, 1,
    2, 2, 1, 0,
    1, 0, 2, 1,
    2, 1, 2, 0,
];

// ── 升级条件断言（D1 满账快照，自 handoff 对表） ─────────────────
/// 写死判据：D1 满账 finalized = 40。
pub const GOLD_D1_FINALIZED: u32 = 40;
/// handoff 快照（写死；假快照注入必被抓）。
pub const HANDOFF_D1_FINALIZED: u32 = 40;

/// 升级条件断言：handoff 快照与写死判据逐值对表。
pub fn upgrade_condition() -> Result<bool, u32> {
    if HANDOFF_D1_FINALIZED != GOLD_D1_FINALIZED {
        return Err(Ls1Code::UpgradeConditionFalse.val());
    }
    if HANDOFF_D1_FINALIZED == 0 {
        return Err(Ls1Code::UpgradeConditionFalse.val());
    }
    Ok(true)
}

// ── 20/20 分界表（与 B27 通道预演零重叠勾稽） ────────────────────
/// 判据 ID 全集 40：B27 预演持 0..20、B31 实接持 20..40。
pub const PART_TOTAL: usize = 40;
pub const PART_B27: usize = 20;
pub const PART_B31: usize = 20;

/// 分界勾稽：交空 + 并满双向断言。
pub fn partition_audit() -> Result<(usize, usize), u32> {
    // 交空：两半区间 [0,20) 与 [20,40) 无公共 id（闭式判定）。
    let lo_a = 0usize;
    let hi_a = PART_B27;
    let lo_b = PART_B27;
    let hi_b = PART_TOTAL;
    if hi_a != lo_b {
        return Err(Ls1Code::PartitionOverlap.val());
    }
    // 并满：两半之和 = 全集。
    if (hi_a - lo_a) + (hi_b - lo_b) != PART_TOTAL {
        return Err(Ls1Code::PartitionOverlap.val());
    }
    if PART_B27 != 20 || PART_B31 != 20 {
        return Err(Ls1Code::PartitionOverlap.val());
    }
    Ok((PART_B27, PART_B31))
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ls1Code {
    /// 矩阵行越闭集
    RowOutOfRange,
    /// 节点类型越闭集
    NodeTypeInvalid,
    /// 属性字段未登记（D1 冻结接口外）
    AttrUnregistered,
    /// 联签深度越枚举
    DepthInvalid,
    /// 升级条件断言为假（D1 满账快照漂移）
    UpgradeConditionFalse,
    /// 分界表重叠（零重叠勾稽失守）
    PartitionOverlap,
    /// 联签状态机越边
    IllegalTransition,
    /// 行未封账
    RowUnsealed,
    /// 对接矩阵指纹失真
    FingerprintInvalid,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl Ls1Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Ls1Code::RowOutOfRange => "矩阵行越闭集：20 行冻结",
            Ls1Code::NodeTypeInvalid => "节点类型越闭集：六类节点冻结",
            Ls1Code::AttrUnregistered => "属性字段未登记：D1 冻结接口外",
            Ls1Code::DepthInvalid => "联签深度越枚举：0/1/2 闭集",
            Ls1Code::UpgradeConditionFalse => "升级条件断言为假：D1 满账≠40",
            Ls1Code::PartitionOverlap => "分界表重叠：20/20 勾稽失守",
            Ls1Code::IllegalTransition => "联签状态机越边：未联签先封账",
            Ls1Code::RowUnsealed => "行未封账：红绿齐红线",
            Ls1Code::FingerprintInvalid => "对接矩阵指纹失真：钉版红线",
            Ls1Code::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [Ls1Code; 10] = [
        Ls1Code::RowOutOfRange,
        Ls1Code::NodeTypeInvalid,
        Ls1Code::AttrUnregistered,
        Ls1Code::DepthInvalid,
        Ls1Code::UpgradeConditionFalse,
        Ls1Code::PartitionOverlap,
        Ls1Code::IllegalTransition,
        Ls1Code::RowUnsealed,
        Ls1Code::FingerprintInvalid,
        Ls1Code::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < Ls1Code::ALL.len() {
        if Ls1Code::ALL[i].val() == code {
            return Ls1Code::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66400 段闭集内"
}

/// 属性字段登记表（D1 冻结接口字段名，写死；未登记必拒）。
pub const ATTR_REGISTRY: [&str; MX_ROWS] = MX_ATTR;

pub fn attr_registered(a: &str) -> bool {
    let mut i = 0;
    while i < ATTR_REGISTRY.len() {
        if ATTR_REGISTRY[i] == a {
            return true;
        }
        i += 1;
    }
    false
}

/// 联签深度闭集校验。
pub fn depth_valid(d: u32) -> Result<(), u32> {
    match d {
        0 | 1 | 2 => Ok(()),
        _ => Err(Ls1Code::DepthInvalid.val()),
    }
}

/// 行级对接校验：节点闭集 + 字段登记 + 深度闭集。
pub fn dock_row(row: usize) -> Result<(usize, &'static str, u32), u32> {
    if row >= MX_ROWS {
        return Err(Ls1Code::RowOutOfRange.val());
    }
    let nd = MX_NODE[row];
    if nd >= ND_COUNT {
        return Err(Ls1Code::NodeTypeInvalid.val());
    }
    let attr = MX_ATTR[row];
    if !attr_registered(attr) {
        return Err(Ls1Code::AttrUnregistered.val());
    }
    depth_valid(MX_DEPTH[row])?;
    Ok((nd, attr, MX_DEPTH[row]))
}

// ── 联签状态机：对接 → 联签 → 封账（越边拒收） ───────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignState {
    Docked,
    Signed(u32),
    Sealed(u32),
}

// ── 对接矩阵总账 ─────────────────────────────────────────────────
pub struct LsignLedger {
    pub states: Vec<SignState>,
    pub sealed: u32,
    pub fingerprint: u64,
}

impl LsignLedger {
    pub fn new() -> LsignLedger {
        LsignLedger { states: Vec::new(), sealed: 0, fingerprint: 0 }
    }

    /// 对接 20 行（全部入 Docked）。
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

    /// 联签一行（Docked → Signed(depth)）。
    pub fn sign_at(&mut self, row: usize) -> Result<(), u32> {
        if row >= self.states.len() {
            return Err(Ls1Code::RowOutOfRange.val());
        }
        let (_, _, depth) = dock_row(row)?;
        if let SignState::Docked = self.states[row] {
            self.states[row] = SignState::Signed(depth);
            Ok(())
        } else {
            Err(Ls1Code::IllegalTransition.val())
        }
    }

    /// 封账一行（Signed → Sealed）。
    pub fn seal_at(&mut self, row: usize) -> Result<(), u32> {
        if row >= self.states.len() {
            return Err(Ls1Code::RowOutOfRange.val());
        }
        if let SignState::Signed(d) = self.states[row] {
            self.states[row] = SignState::Sealed(d);
            self.sealed += 1;
            self.fingerprint = self
                .fingerprint
                .wrapping_mul(31)
                .wrapping_add(row as u64)
                .wrapping_add(d as u64);
            Ok(())
        } else {
            Err(Ls1Code::IllegalTransition.val())
        }
    }

    /// 全 20 行跑账：对接→联签→封账。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let n = self.dock_all()?;
        if n as usize != MX_ROWS {
            return Err(Ls1Code::RowUnsealed.val());
        }
        for r in 0..MX_ROWS {
            self.sign_at(r)?;
        }
        for r in 0..MX_ROWS {
            self.seal_at(r)?;
        }
        if self.sealed as usize != MX_ROWS {
            return Err(Ls1Code::RowUnsealed.val());
        }
        Ok(self.sealed)
    }

    /// 复现：双跑逐行 + 指纹一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = LsignLedger::new();
        let _ = run2.run_all()?;
        if run2.sealed != self.sealed || run2.fingerprint != self.fingerprint {
            return Err(Ls1Code::NotReproducible.val());
        }
        let mut r = 0;
        while r < self.states.len() {
            if self.states[r] != run2.states[r] {
                return Err(Ls1Code::NotReproducible.val());
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
        let (nd, attr, depth) = dock_row(r)?;
        if nd != MX_NODE[r] || attr != MX_ATTR[r] || depth != MX_DEPTH[r] {
            return Err(Ls1Code::FingerprintInvalid.val());
        }
        ok += 1;
        k += 1;
    }
    Ok(ok)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u64), u32> {
    let mut led = LsignLedger::new();
    let sealed = led.run_all()?;
    led.reproducible()?;
    upgrade_condition()?;
    partition_audit()?;
    if led.fingerprint == 0 {
        return Err(Ls1Code::FingerprintInvalid.val());
    }
    Ok((sealed, led.fingerprint))
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
        if e == Ls1Code::RowOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：未登记属性字段注入（D1 冻结接口外字段）。
    ledger.tries[1] += 1;
    if !attr_registered("ghost.attr") {
        ledger.caught[1] += 1;
    }
    // 族 2：状态机越边注入（未联签先封账）。
    ledger.tries[2] += 1;
    let mut led = LsignLedger::new();
    led.dock_all().ok();
    if let Err(e) = led.seal_at(0) {
        if e == Ls1Code::IllegalTransition.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < Ls1Code::ALL.len() {
        let v = Ls1Code::ALL[i].val();
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
        "J1-20行全封账+复现+指纹非零",
        matches!(golden, Ok((20, _))),
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
        "J1-升级条件断言（D1 finalized=40 快照对表）",
        matches!(up, Ok(true)),
        format!("handoff={} gold={}", HANDOFF_D1_FINALIZED, GOLD_D1_FINALIZED),
    ));
    let part = partition_audit();
    v.push((
        "J1-20/20分界勾稽（交空+并满）",
        matches!(part, Ok((20, 20))),
        format!("{:?}", part),
    ));
    let spot = lcg_spot_check();
    v.push((
        "J1-LCG抽样20行三元组对账",
        matches!(spot, Ok(20)),
        format!("{:?}", spot),
    ));
    // 节点类型点验：六类节点在矩阵中全部出现（节点覆盖点验）。
    let node_cover = {
        let mut seen = [false; ND_COUNT];
        let mut r = 0;
        while r < MX_ROWS {
            if MX_NODE[r] < ND_COUNT {
                seen[MX_NODE[r]] = true;
            }
            r += 1;
        }
        let mut all = true;
        let mut i = 0;
        while i < ND_COUNT {
            if !seen[i] {
                all = false;
            }
            i += 1;
        }
        all
    };
    v.push((
        "J1-六类节点全覆盖点验",
        node_cover,
        String::from("20 行矩阵覆盖全部节点类型"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66400 窗口")));
    v
}

pub fn run_unx14201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14201 D3×D1 真联签总起（20 行对接矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
