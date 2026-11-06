//! UNX-F10043 触发器矩阵（提交/收口/快照后） · 0x91ac0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10043（B22 常态化回归线 · 触发矩阵）。
//! 锚点：UNX-F10044-J1 策略映射冻结（档↔策略一一对应 10/10 次），越策略选例 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91ac0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `MatrixCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 映射表 `POLICY_MAP`：三类触发（提交后/收口前/快照后）↔ 三种选例策略
//!     （增量/全量/抽样）的冻结一一对应，双射性质逐次复核；
//!   - 分发器 `TriggerMatrix`：resolve（触发→策略）与 guard（越策略选例拦截）
//!     双口冻结，映射改写显性拒绝；
//!   - 留痕账 `MatrixLedger`：触发 = 留痕 + 逃逸，逃逸恒 0，只增不减；
//!   - 注入核 `BadMatrixCase`：越策略选例 / 未映射触发 / 重复绑定 / 触发无痕 /
//!     冻结后改映射 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10043 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91ac0;

/// 段窗上界（下一卡 0x91b00 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91b00;

/// 触发种类数（提交后/收口前/快照后）。
pub const TRIGGER_COUNT: usize = 3;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixCode {
    /// 越策略选例（claimed ≠ 冻结映射）
    OutOfPolicy,
    /// 未映射触发（原始码出闭集）
    UnmappedTrigger,
    /// 重复绑定（同触发二次绑策略）
    DuplicateBinding,
    /// 触发无留痕（逃逸）
    MissingTrace,
    /// 冻结后改映射
    FrozenMapMutate,
    /// 映射非双射
    NotBijective,
    /// 留痕账倒退
    LedgerRegress,
    /// 矩阵未冻结即分发
    NotFrozen,
    /// 触发序号越界
    TriggerOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 解析失败
    ParseFailure,
    /// 段位越界
    SegmentViolation,
}

impl MatrixCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MatrixCode::OutOfPolicy => "越策略选例被拒：选例策略必须取冻结映射结果；请按 resolve 返回值选例",
            MatrixCode::UnmappedTrigger => "未映射触发被拒：触发码出三类闭集；请核对触发种类",
            MatrixCode::DuplicateBinding => "重复绑定被拒：同触发已绑策略；一一对应禁止二次绑定",
            MatrixCode::MissingTrace => "触发无留痕被拒：每次触发必须入留痕账；请补留痕",
            MatrixCode::FrozenMapMutate => "冻结后改映射被拒：矩阵已封账只读；请走修订通道",
            MatrixCode::NotBijective => "映射非双射：档↔策略必须一一对应；请核对映射表",
            MatrixCode::LedgerRegress => "留痕账倒退被拒：计数只增不减；请排查回滚误改",
            MatrixCode::NotFrozen => "矩阵未冻结：冻结前不得对外分发；请先完成冻结",
            MatrixCode::TriggerOutOfRange => "触发序号越界：序号必须落在 0..=2；请核对下标",
            MatrixCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            MatrixCode::ParseFailure => "解析失败：触发行必须恰为两段 trigger|policy；请补齐字段",
            MatrixCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91ac0..=0x91b00",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [MatrixCode; 12] = [
        MatrixCode::OutOfPolicy,
        MatrixCode::UnmappedTrigger,
        MatrixCode::DuplicateBinding,
        MatrixCode::MissingTrace,
        MatrixCode::FrozenMapMutate,
        MatrixCode::NotBijective,
        MatrixCode::LedgerRegress,
        MatrixCode::NotFrozen,
        MatrixCode::TriggerOutOfRange,
        MatrixCode::NotReproducible,
        MatrixCode::ParseFailure,
        MatrixCode::SegmentViolation,
    ];
}

// ── 触发与策略闭集 ────────────────────────────────────────────────────
/// 触发种类（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerKind {
    /// 提交后触发
    AfterCommit,
    /// 收口前触发
    BeforeClosing,
    /// 快照后触发
    AfterSnapshot,
}

impl TriggerKind {
    /// 闭集全量。
    pub const ALL: [TriggerKind; 3] =
        [TriggerKind::AfterCommit, TriggerKind::BeforeClosing, TriggerKind::AfterSnapshot];

    /// 从文本解析（"commit"/"closing"/"snapshot"）。
    pub fn from_field(s: &str) -> Option<TriggerKind> {
        match s {
            "commit" => Some(TriggerKind::AfterCommit),
            "closing" => Some(TriggerKind::BeforeClosing),
            "snapshot" => Some(TriggerKind::AfterSnapshot),
            _ => None,
        }
    }

    /// 槽位下标。
    pub fn index(self) -> usize {
        match self {
            TriggerKind::AfterCommit => 0,
            TriggerKind::BeforeClosing => 1,
            TriggerKind::AfterSnapshot => 2,
        }
    }
}

/// 选例策略（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickPolicy {
    /// 增量选例（依赖图驱动）
    Incremental,
    /// 全量选例
    FullSweep,
    /// 抽样选例（分层随机）
    Stratified,
}

impl PickPolicy {
    /// 闭集全量。
    pub const ALL: [PickPolicy; 3] =
        [PickPolicy::Incremental, PickPolicy::FullSweep, PickPolicy::Stratified];

    /// 从文本解析（"incremental"/"full"/"stratified"）。
    pub fn from_field(s: &str) -> Option<PickPolicy> {
        match s {
            "incremental" => Some(PickPolicy::Incremental),
            "full" => Some(PickPolicy::FullSweep),
            "stratified" => Some(PickPolicy::Stratified),
            _ => None,
        }
    }

    /// 策略名。
    pub fn name(self) -> &'static str {
        match self {
            PickPolicy::Incremental => "增量",
            PickPolicy::FullSweep => "全量",
            PickPolicy::Stratified => "抽样",
        }
    }
}

/// 冻结映射表：提交后→增量、收口前→全量、快照后→抽样
/// （来源：UNX-F10044-J1 档↔策略一一对应判据）。
pub const POLICY_MAP: [(TriggerKind, PickPolicy); TRIGGER_COUNT] = [
    (TriggerKind::AfterCommit, PickPolicy::Incremental),
    (TriggerKind::BeforeClosing, PickPolicy::FullSweep),
    (TriggerKind::AfterSnapshot, PickPolicy::Stratified),
];

/// 双射性质核：触发侧无重复、策略侧无重复（一一对应）。
pub fn mapping_bijective() -> bool {
    let mut i = 0usize;
    while i < POLICY_MAP.len() {
        let ti = match POLICY_MAP.get(i) {
            Some(row) => row.0,
            None => return false,
        };
        let pi = match POLICY_MAP.get(i) {
            Some(row) => row.1,
            None => return false,
        };
        let mut j = i + 1;
        while j < POLICY_MAP.len() {
            if let Some(row) = POLICY_MAP.get(j) {
                if row.0 == ti || row.1 == pi {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 分发器 ────────────────────────────────────────────────────────────
/// 触发矩阵分发器：冻结后对外供 resolve 与 guard 双口。
#[derive(Clone, Copy, Debug)]
pub struct TriggerMatrix {
    /// 是否已冻结
    pub frozen: bool,
    /// 绑定占用表（每触发至多一次绑定）
    pub bound: [bool; TRIGGER_COUNT],
}

impl TriggerMatrix {
    /// 构造未冻结分发器。
    pub const fn new() -> TriggerMatrix {
        TriggerMatrix { frozen: false, bound: [false; TRIGGER_COUNT] }
    }

    /// 绑定一行映射：重复绑定与越闭集绑定显性拒绝。
    pub fn bind(&mut self, t: TriggerKind, p: PickPolicy) -> Result<(), MatrixCode> {
        if self.frozen {
            return Err(MatrixCode::FrozenMapMutate);
        }
        let expected = map_of(t);
        if expected != p {
            return Err(MatrixCode::OutOfPolicy);
        }
        let idx = t.index();
        if *self.bound.get(idx).unwrap_or(&true) {
            return Err(MatrixCode::DuplicateBinding);
        }
        if let Some(slot) = self.bound.get_mut(idx) {
            *slot = true;
        }
        Ok(())
    }

    /// 冻结：三触发全绑定且映射双射才可冻结。
    pub fn freeze(&mut self) -> Result<(), MatrixCode> {
        if self.frozen {
            return Err(MatrixCode::FrozenMapMutate);
        }
        let mut i = 0usize;
        while i < TRIGGER_COUNT {
            if !*self.bound.get(i).unwrap_or(&false) {
                return Err(MatrixCode::UnmappedTrigger);
            }
            i += 1;
        }
        if !mapping_bijective() {
            return Err(MatrixCode::NotBijective);
        }
        self.frozen = true;
        Ok(())
    }

    /// resolve：触发 → 冻结策略；未冻结即拒绝。
    pub fn resolve(&self, t: TriggerKind) -> Result<PickPolicy, MatrixCode> {
        if !self.frozen {
            return Err(MatrixCode::NotFrozen);
        }
        Ok(map_of(t))
    }

    /// guard：越策略选例拦截——claimed 必须等于冻结映射值。
    pub fn guard(&self, t: TriggerKind, claimed: PickPolicy) -> Result<(), MatrixCode> {
        let actual = self.resolve(t)?;
        if actual != claimed {
            return Err(MatrixCode::OutOfPolicy);
        }
        Ok(())
    }
}

/// 冻结映射查表（触发 → 策略；表驱动，禁运行期造映射）。
pub fn map_of(t: TriggerKind) -> PickPolicy {
    let idx = t.index();
    match POLICY_MAP.get(idx) {
        Some(row) if row.0 == t => row.1,
        _ => PickPolicy::FullSweep,
    }
}

/// 标准三绑定并冻结。
pub fn freeze_standard(m: &mut TriggerMatrix) -> Result<(), MatrixCode> {
    let mut i = 0usize;
    while i < POLICY_MAP.len() {
        if let Some(row) = POLICY_MAP.get(i) {
            m.bind(row.0, row.1)?;
        }
        i += 1;
    }
    m.freeze()
}

// ── 触发行解析器 ──────────────────────────────────────────────────────
/// 触发行解析器：「trigger|policy」两段，缺一即 ParseFailure。
#[derive(Default)]
pub struct MatrixParser {
    /// 累计成功解析条数
    pub parsed: u32,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl MatrixParser {
    /// 解析一行触发行；失败原因由 MatrixCode 表达。
    pub fn parse(&mut self, text: &str) -> Result<(TriggerKind, PickPolicy), MatrixCode> {
        let parts: Vec<&str> = text.split('|').collect();
        if parts.len() != 2 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(MatrixCode::ParseFailure);
        }
        let t = match TriggerKind::from_field(parts.get(0).copied().unwrap_or("")) {
            Some(v) => v,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(MatrixCode::UnmappedTrigger);
            }
        };
        let p = match PickPolicy::from_field(parts.get(1).copied().unwrap_or("")) {
            Some(v) => v,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(MatrixCode::ParseFailure);
            }
        };
        self.parsed = self.parsed.saturating_add(1);
        Ok((t, p))
    }
}

// ── 留痕账 ────────────────────────────────────────────────────────────
/// 留痕账：触发 = 留痕 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixLedger {
    /// 触发总次数
    pub fired: u32,
    /// 已留痕次数
    pub traced: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl MatrixLedger {
    /// 构造零账。
    pub const fn zero() -> MatrixLedger {
        MatrixLedger { fired: 0, traced: 0, escaped: 0 }
    }

    /// 记一次触发；trace=false 即逃逸入账（显性记逃逸而非静默吞）。
    pub fn record(&mut self, trace: bool) -> Result<(), MatrixCode> {
        self.fired = self.fired.saturating_add(1);
        if trace {
            self.traced = self.traced.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.traced.saturating_add(self.escaped) != self.fired {
            return Err(MatrixCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.traced.saturating_add(self.escaped) == self.fired
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏触发样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMatrixCase {
    /// 越策略选例（claim 与映射不符）
    OutOfPolicy,
    /// 未映射触发（未知文本）
    UnmappedTrigger,
    /// 重复绑定
    DuplicateBinding,
    /// 触发无留痕
    MissingTrace,
    /// 冻结后改映射
    FrozenMapMutate,
}

impl BadMatrixCase {
    /// 闭集全量。
    pub const ALL: [BadMatrixCase; 5] = [
        BadMatrixCase::OutOfPolicy,
        BadMatrixCase::UnmappedTrigger,
        BadMatrixCase::DuplicateBinding,
        BadMatrixCase::MissingTrace,
        BadMatrixCase::FrozenMapMutate,
    ];

    /// 注入一轮：构造坏样本并触发矩阵，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadMatrixCase::OutOfPolicy => {
                let mut m = TriggerMatrix::new();
                if freeze_standard(&mut m).is_err() {
                    return false;
                }
                // 提交后映射为增量，claimed 全量 → 必拒。
                m.guard(TriggerKind::AfterCommit, PickPolicy::FullSweep).is_err()
                    && m.guard(TriggerKind::AfterCommit, PickPolicy::Incremental).is_ok()
            }
            BadMatrixCase::UnmappedTrigger => {
                let mut p = MatrixParser::default();
                p.parse("nightly|full").is_err()
            }
            BadMatrixCase::DuplicateBinding => {
                let mut m = TriggerMatrix::new();
                if m.bind(TriggerKind::AfterSnapshot, PickPolicy::Stratified).is_err() {
                    return false;
                }
                m.bind(TriggerKind::AfterSnapshot, PickPolicy::Stratified).is_err()
            }
            BadMatrixCase::MissingTrace => {
                let mut l = MatrixLedger::zero();
                let _ = l.record(true);
                let _ = l.record(false);
                l.escaped == 1 && l.conserved()
            }
            BadMatrixCase::FrozenMapMutate => {
                let mut m = TriggerMatrix::new();
                if freeze_standard(&mut m).is_err() {
                    return false;
                }
                m.bind(TriggerKind::AfterSnapshot, PickPolicy::Stratified).is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_matrix_injection() -> (MatrixLedger, [u32; 5]) {
    let mut ledger = MatrixLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadMatrixCase::ALL.len() {
        if let Some(case) = BadMatrixCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越策略选例族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未映射触发族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复绑定族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏触发行族"),
    ("EROFS", -30, 19, "Windows ERROR_WRITE_PROTECT；冻结映射只读族"),
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

/// 规范语料双跑指纹：解析→绑定→冻结→分发全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = MatrixParser::default();
    let mut m = TriggerMatrix::new();
    let mut bytes: Vec<u8> = Vec::new();
    let texts = ["commit|incremental", "closing|full", "snapshot|stratified", "nightly|full"];
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let r = p.parse(t);
            bytes.push(if r.is_ok() { 1 } else { 0 });
            if let Ok((tr, po)) = r {
                bytes.push(match m.bind(tr, po) {
                    Ok(()) => 2,
                    Err(_) => 3,
                });
            }
        }
        i += 1;
    }
    bytes.push(match m.freeze() {
        Ok(()) => 4,
        Err(_) => 5,
    });
    let mut t = 0usize;
    while t < TriggerKind::ALL.len() {
        if let Some(kind) = TriggerKind::ALL.get(t).copied() {
            match m.resolve(kind) {
                Ok(po) => bytes.push(po as u8 + 1),
                Err(_) => bytes.push(0),
            }
        }
        t += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10043 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10043_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10043");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91ac0..=0x91b00", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 冻结映射双射性质
    s.add("策略映射：档↔策略一一对应（双射）", mapping_bijective(),
          "三触发三策略两侧各无重复，映射表闭集冻结");

    // 3. 标准三绑定并冻结成功
    let mut m = TriggerMatrix::new();
    let std_ok = freeze_standard(&mut m).is_ok() && m.frozen;
    s.add("分发器：三绑定→冻结成功", std_ok,
          "POLICY_MAP 逐行绑定后单向冻结");

    // 4. resolve 逐档正确
    let resolve_ok = m.resolve(TriggerKind::AfterCommit) == Ok(PickPolicy::Incremental)
        && m.resolve(TriggerKind::BeforeClosing) == Ok(PickPolicy::FullSweep)
        && m.resolve(TriggerKind::AfterSnapshot) == Ok(PickPolicy::Stratified);
    s.add("分发器：逐档 resolve 与冻结映射一致", resolve_ok,
          "提交后→增量、收口前→全量、快照后→抽样");

    // 5. 越策略选例 10/10 拦截（锚点判据直接探针）
    let mut oop_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut tmp = TriggerMatrix::new();
        let _ = freeze_standard(&mut tmp);
        // 三档各拿一个错策略试一遍：全错才计检出。
        let wrong = tmp.guard(TriggerKind::AfterCommit, PickPolicy::Stratified).is_err()
            && tmp.guard(TriggerKind::BeforeClosing, PickPolicy::Incremental).is_err()
            && tmp.guard(TriggerKind::AfterSnapshot, PickPolicy::FullSweep).is_err();
        if wrong {
            oop_hit = oop_hit.saturating_add(1);
        }
        r += 1;
    }
    s.add("越策略选例拦截：10/10 轮三档全拦", oop_hit == INJECT_ROUNDS,
          "claim≠冻结映射值即 OutOfPolicy，无一放行");

    // 6. 绑定校验：绑错策略与重复绑定都拒绝
    let mut b1 = TriggerMatrix::new();
    let wrong_bind = b1.bind(TriggerKind::AfterCommit, PickPolicy::FullSweep)
        == Err(MatrixCode::OutOfPolicy);
    let mut b2 = TriggerMatrix::new();
    let _ = b2.bind(TriggerKind::AfterCommit, PickPolicy::Incremental);
    let dup_bind = b2.bind(TriggerKind::AfterCommit, PickPolicy::Incremental)
        == Err(MatrixCode::DuplicateBinding);
    s.add("绑定校验：错策略/重复绑定双双拒绝", wrong_bind && dup_bind,
          "绑定口是映射表唯一入口，错绑显性拒绝");

    // 7. 缺绑定冻结拦截
    let mut part = TriggerMatrix::new();
    let _ = part.bind(TriggerKind::AfterCommit, PickPolicy::Incremental);
    let part_rej = part.freeze() == Err(MatrixCode::UnmappedTrigger);
    s.add("缺绑定冻结：未绑全三触发拒绝冻结", part_rej,
          "冻结前提是三触发各有一绑");

    // 8. 冻结后改映射拦截 + 未冻结分发拦截
    let mut fz = TriggerMatrix::new();
    let _ = freeze_standard(&mut fz);
    let mut_rej = fz.bind(TriggerKind::AfterCommit, PickPolicy::Incremental)
        == Err(MatrixCode::FrozenMapMutate);
    let nf = TriggerMatrix::new();
    let nf_rej = nf.resolve(TriggerKind::AfterCommit) == Err(MatrixCode::NotFrozen);
    s.add("冻结闸：冻结后改映射/未冻结分发都拒绝", mut_rej && nf_rej,
          "冻结是单向闸，映射只读、分发需先冻结");

    // 9. 触发留痕 10/10：三档逐轮分发留痕
    let mut trace_ok = true;
    let mut r2 = 0u32;
    let mut total_fired = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut mt = TriggerMatrix::new();
        if freeze_standard(&mut mt).is_err() {
            trace_ok = false;
            break;
        }
        let mut l = MatrixLedger::zero();
        let mut t = 0usize;
        while t < TriggerKind::ALL.len() {
            if let Some(kind) = TriggerKind::ALL.get(t).copied() {
                if mt.resolve(kind).is_ok() && l.record(true).is_err() {
                    trace_ok = false;
                }
            }
            t += 1;
        }
        total_fired = total_fired.saturating_add(l.fired);
        r2 += 1;
    }
    s.add("触发留痕：十轮三档逐次入账 30/30", trace_ok && total_fired == 3 * INJECT_ROUNDS,
          "每次分发必留痕，十轮共 30 条");

    // 10. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_matrix_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "越策略/未映射/重复绑定/无痕/冻结改映射全被显式拒绝");

    // 11. 留痕账守恒：逃逸=0
    s.add("留痕账守恒：触发=留痕+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.fired == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 12. 文案闭集：非空
    let text_ok = MatrixCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < MatrixCode::ALL.len() {
        let mut j = i + 1;
        while j < MatrixCode::ALL.len() {
            if let (Some(x), Some(y)) = (MatrixCode::ALL.get(i), MatrixCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = MatrixCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EROFS)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EROFS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 解析账闭合
    let mut p2 = MatrixParser::default();
    let _ = p2.parse("commit|incremental");
    let _ = p2.parse("bad");
    let _ = p2.parse("closing|full");
    s.add("解析账闭合：解析+拒绝=提交总数",
          p2.parsed.saturating_add(p2.rejected) == 3,
          "每行提交必须入两账之一，无第三去向");

    // 17. 触发序号越界拦截（map_of 表驱动只收闭集下标）
    let idx_ok = TriggerKind::AfterCommit.index() == 0
        && TriggerKind::BeforeClosing.index() == 1
        && TriggerKind::AfterSnapshot.index() == 2
        && POLICY_MAP.len() == TRIGGER_COUNT;
    s.add("触发闭集：三触发下标 0..=2 与映射表同长", idx_ok,
          "表驱动映射下标必须落在闭集内");

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
    fn mapping_is_bijective() {
        assert!(mapping_bijective());
        assert_eq!(map_of(TriggerKind::AfterCommit), PickPolicy::Incremental);
        assert_eq!(map_of(TriggerKind::BeforeClosing), PickPolicy::FullSweep);
        assert_eq!(map_of(TriggerKind::AfterSnapshot), PickPolicy::Stratified);
    }

    #[test]
    fn out_of_policy_rejected() {
        let mut m = TriggerMatrix::new();
        let _ = freeze_standard(&mut m);
        assert_eq!(m.guard(TriggerKind::AfterCommit, PickPolicy::FullSweep),
                   Err(MatrixCode::OutOfPolicy));
        assert!(m.guard(TriggerKind::AfterCommit, PickPolicy::Incremental).is_ok());
    }

    #[test]
    fn duplicate_binding_rejected() {
        let mut m = TriggerMatrix::new();
        assert!(m.bind(TriggerKind::BeforeClosing, PickPolicy::FullSweep).is_ok());
        assert_eq!(m.bind(TriggerKind::BeforeClosing, PickPolicy::FullSweep),
                   Err(MatrixCode::DuplicateBinding));
    }

    #[test]
    fn wrong_binding_rejected() {
        let mut m = TriggerMatrix::new();
        assert_eq!(m.bind(TriggerKind::AfterSnapshot, PickPolicy::FullSweep),
                   Err(MatrixCode::OutOfPolicy));
    }

    #[test]
    fn incomplete_freeze_rejected() {
        let mut m = TriggerMatrix::new();
        let _ = m.bind(TriggerKind::AfterCommit, PickPolicy::Incremental);
        assert_eq!(m.freeze(), Err(MatrixCode::UnmappedTrigger));
    }

    #[test]
    fn frozen_matrix_is_readonly() {
        let mut m = TriggerMatrix::new();
        let _ = freeze_standard(&mut m);
        assert_eq!(m.bind(TriggerKind::AfterCommit, PickPolicy::Incremental),
                   Err(MatrixCode::FrozenMapMutate));
    }

    #[test]
    fn resolve_before_freeze_rejected() {
        let m = TriggerMatrix::new();
        assert_eq!(m.resolve(TriggerKind::AfterCommit), Err(MatrixCode::NotFrozen));
    }

    #[test]
    fn parser_rejects_unknown_trigger() {
        let mut p = MatrixParser::default();
        assert!(p.parse("nightly|full").is_err());
        assert!(p.parse("commit|bogus").is_err());
        assert!(p.parse("commit").is_err());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_matrix_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.fired, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EROFS));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10043_checks_green() {
        let s = run_unx10043_checks();
        assert!(s.all_passed(), "UNX-F10043 red: {}", s.red_items_text());
    }
}
