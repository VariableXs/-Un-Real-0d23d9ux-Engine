//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18433 VerifyVersionInfo 消费挂点 · 0x7E800段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18433-J1 VerifyVersionInfo 判定走矩阵输出断言全过，直连系统值旁路注入 10/10 拒绝
//! 设计要点：
//!   - 挂点语义：应用以「声称值+类型掩码+条件掩码」调 VerifyVersionInfo，
//!     内核挂点**强制**用谎报矩阵裁决值（而非系统真值）逐域比较后给出
//!     布尔判定——判定走矩阵输出，不允许直连系统值旁路；
//!   - 路由闭集：MatrixRoute（合法）｜DirectSystem（旁路注入，一律拒）；
//!   - 条件掩码状态机：VER_SET_CONDITION 逐域登记 6 种比较算子
//!     （Equal/NotEqual/Greater/GreaterEqual/Less/LessEqual，3 位编码），
//!     重复登记/域越界/算子越界均为非法迁移；
//!   - 判定次序（结构错误先于策略错误，与 EBADF 先于 EFAULT 同理）：
//!     掩码空 → 算子越界 → 掩码错配 → 旁路拒 → 矩阵无值 → 逐域裁决；
//!   - 注入核三类 ×10 轮：直连旁路 10/10 拒、算子越界 10/10 拒、
//!     掩码错配 10/10 拒；漏检必须为 0；
//!   - 对账面：10 例合法判定逐例与手算期望对拍（矩阵值×算子真值表）；
//!   - 确定性：LCG 种子=SEG_BASE(0x7E800)；no_std+alloc；零 unwrap/expect/
//!     panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   VviField / CondOp / Route / VviCode(诊断闭集) / ConditionMaskBuilder
//!   / VviRequest / MatrixSlot+ProcMatrix(本地矩阵) / vvi_hook(挂点判定器)
//!   / 对账语料 10 例 / InjectLedger / golden_run / run_unx18433_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E800，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E800;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 结构体域数（冻结：Major/Minor/Build/Platform/SP主/SP次/Suite/ProductType）。
pub const FIELD_N: usize = 8;
/// 比较算子数（冻结：6 种，3 位编码，编码 6/7 越界）。
pub const OP_N: u32 = 6;
/// 每域算子编码位宽（冻结：3 位，2^3=8 ≥ 6 种算子）。
pub const OP_BITS: u32 = 3;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 对账语料例数（冻结：10 例合法判定逐例对拍）。
pub const RECON_CASES: usize = 10;
/// 进程矩阵条目容量（冻结：8 进程 × 8 域 = 64 ≤ 96）。
pub const MATRIX_CAP: usize = 96;

// ── 结构体域枚举 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VviField {
    /// dwMajorVersion。
    Major = 0,
    /// dwMinorVersion。
    Minor = 1,
    /// dwBuildNumber。
    Build = 2,
    /// dwPlatformId。
    Platform = 3,
    /// wServicePackMajor。
    SpMajor = 4,
    /// wServicePackMinor。
    SpMinor = 5,
    /// wSuiteMask。
    Suite = 6,
    /// wProductType。
    ProductType = 7,
}

impl VviField {
    /// 域序号（类型掩码位号）。
    pub fn bit(self) -> u32 {
        self as u32
    }
    /// 由域序号还原；越界 None。
    pub fn from_bit(b: u32) -> Option<VviField> {
        match b {
            0 => Some(VviField::Major),
            1 => Some(VviField::Minor),
            2 => Some(VviField::Build),
            3 => Some(VviField::Platform),
            4 => Some(VviField::SpMajor),
            5 => Some(VviField::SpMinor),
            6 => Some(VviField::Suite),
            7 => Some(VviField::ProductType),
            _ => None,
        }
    }
    /// 域名（诊断文本共用）。
    pub fn name(self) -> &'static str {
        match self {
            VviField::Major => "Major",
            VviField::Minor => "Minor",
            VviField::Build => "Build",
            VviField::Platform => "Platform",
            VviField::SpMajor => "SpMajor",
            VviField::SpMinor => "SpMinor",
            VviField::Suite => "Suite",
            VviField::ProductType => "ProductType",
        }
    }
}

// ── 比较算子枚举（VER_* 六族） ──────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CondOp {
    /// VER_EQUAL。
    Equal = 0,
    /// VER_NOT_EQUAL。
    NotEqual = 1,
    /// VER_GREATER。
    Greater = 2,
    /// VER_GREATER_EQUAL。
    GreaterEqual = 3,
    /// VER_LESS。
    Less = 4,
    /// VER_LESS_EQUAL。
    LessEqual = 5,
}

impl CondOp {
    /// 3 位编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 由编码还原；6/7 越界返回 None（注入面）。
    pub fn from_code(c: u32) -> Option<CondOp> {
        match c {
            0 => Some(CondOp::Equal),
            1 => Some(CondOp::NotEqual),
            2 => Some(CondOp::Greater),
            3 => Some(CondOp::GreaterEqual),
            4 => Some(CondOp::Less),
            5 => Some(CondOp::LessEqual),
            _ => None,
        }
    }
    /// 真值表：系统侧（矩阵裁决值）OP 要求侧（声称值）——
    /// Windows 语义：VerifyVersionInfo 判「系统版本满足要求条件」，
    /// 即 actual OP claimed（非对易算子的方向以此为准）。
    pub fn eval(self, actual: u32, claimed: u32) -> bool {
        match self {
            CondOp::Equal => actual == claimed,
            CondOp::NotEqual => actual != claimed,
            CondOp::Greater => actual > claimed,
            CondOp::GreaterEqual => actual >= claimed,
            CondOp::Less => actual < claimed,
            CondOp::LessEqual => actual <= claimed,
        }
    }
}

// ── 路由闭集 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// 合法：判定走谎报矩阵输出。
    MatrixRoute = 0,
    /// 旁路注入：直连系统值——挂点一律拒绝。
    DirectSystem = 1,
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VviCode {
    /// 类型掩码为空（无参与域）
    TypeMaskEmpty,
    /// 算子编码越界（6/7）
    OpIllegal,
    /// 条件掩码与类型掩码错配
    MaskMismatch,
    /// 直连系统值旁路（一律拒）
    BypassRefused,
    /// 矩阵无该进程该域裁决值
    NoMatrixValue,
    /// 参与域缺声称值
    ClaimMissing,
    /// 条件登记重复（同域二次 set）
    CondDup,
    /// 登记域越界（≥8）
    FieldIllegal,
    /// 矩阵条目满
    MatrixFull,
    /// 注册依据为空
    BasisMissing,
    /// 双跑不一致
    NotReproducible,
}

impl VviCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VviCode::TypeMaskEmpty => "类型掩码空：无参与域不构成判定",
            VviCode::OpIllegal => "算子编码越界：3 位槽只容 0..6",
            VviCode::MaskMismatch => "掩码错配：条件掩码登记域不在类型掩码内",
            VviCode::BypassRefused => "旁路拒绝：直连系统值不走矩阵不许判定",
            VviCode::NoMatrixValue => "矩阵无值：该进程该域未登记裁决值",
            VviCode::ClaimMissing => "参与域缺声称值：请求结构不完整",
            VviCode::CondDup => "条件重复登记：同域二次 set 为非法迁移",
            VviCode::FieldIllegal => "登记域越界：域序 ≥8",
            VviCode::MatrixFull => "矩阵条目满：注册被拒且账不丢",
            VviCode::BasisMissing => "注册依据为空：有值必有据不变量",
            VviCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [VviCode; 11] = [
        VviCode::TypeMaskEmpty,
        VviCode::OpIllegal,
        VviCode::MaskMismatch,
        VviCode::BypassRefused,
        VviCode::NoMatrixValue,
        VviCode::ClaimMissing,
        VviCode::CondDup,
        VviCode::FieldIllegal,
        VviCode::MatrixFull,
        VviCode::BasisMissing,
        VviCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VviCode::ALL.len() {
        if VviCode::ALL[i].val() == code {
            return VviCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E800 段闭集内"
}

// ── 条件掩码状态机（VER_SET_CONDITION 语义） ────────────────────
/// 逐域登记比较算子；重复登记/域越界为非法迁移。
pub struct ConditionMaskBuilder {
    /// 类型掩码（bit i = 域 i 参与）。
    pub type_mask: u32,
    /// 条件掩码（每域 3 位算子编码，仅类型掩码置位的域有效）。
    pub cond_mask: u64,
    /// 登记过的域账（重复登记检出用）。
    seen: [bool; FIELD_N],
}

impl ConditionMaskBuilder {
    /// 新构造器（全空）。
    pub fn new() -> ConditionMaskBuilder {
        ConditionMaskBuilder { type_mask: 0, cond_mask: 0, seen: [false; FIELD_N] }
    }

    /// VER_SET_CONDITION(field, op)：登记一域条件。
    pub fn set_condition(&mut self, field: VviField, op: CondOp) -> Result<(), u32> {
        let b = field.bit();
        if b as usize >= FIELD_N {
            return Err(VviCode::FieldIllegal.val());
        }
        if self.seen[b as usize] {
            return Err(VviCode::CondDup.val());
        }
        self.seen[b as usize] = true;
        self.type_mask |= 1u32 << b;
        let shift = b * OP_BITS;
        self.cond_mask |= (op.code() as u64) << shift;
        Ok(())
    }

    /// 取某域的算子编码（0=未登记）。
    pub fn op_code_of(&self, field_bit: u32) -> u32 {
        if field_bit as usize >= FIELD_N {
            return 0;
        }
        if self.type_mask & (1u32 << field_bit) == 0 {
            return 0;
        }
        let shift = field_bit * OP_BITS;
        ((self.cond_mask >> shift) & 0b111) as u32
    }
}

// ── 本地进程矩阵（裁决值来源） ──────────────────────────────────
/// 单槽：(进程,域) → 裁决值+依据。有值必有据不变量。
#[derive(Clone, Copy, Debug)]
pub struct MatrixSlot {
    pub proc_id: u32,
    pub field_bit: u32,
    pub value: u32,
    pub basis: u32,
}

/// 进程矩阵：线性键表（64 键规模）。
pub struct ProcMatrix {
    slots: Vec<MatrixSlot>,
    cap: usize,
    pub reg_total: u32,
}

impl ProcMatrix {
    pub fn new() -> ProcMatrix {
        ProcMatrix { slots: Vec::new(), cap: MATRIX_CAP, reg_total: 0 }
    }

    /// 注册裁决值（依据为空拒；同键更新记账）。
    pub fn register(&mut self, proc_id: u32, field_bit: u32, value: u32, basis: u32) -> Result<(), u32> {
        if VviField::from_bit(field_bit).is_none() {
            return Err(VviCode::FieldIllegal.val());
        }
        if basis == 0 {
            return Err(VviCode::BasisMissing.val());
        }
        self.reg_total = self.reg_total.saturating_add(1);
        let mut i = 0;
        while i < self.slots.len() {
            let s = &self.slots[i];
            if s.proc_id == proc_id && s.field_bit == field_bit {
                break;
            }
            i += 1;
        }
        if i < self.slots.len() {
            let s = &mut self.slots[i];
            s.value = value;
            s.basis = basis;
            return Ok(());
        }
        if self.slots.len() >= self.cap {
            self.reg_total = self.reg_total.saturating_sub(1);
            return Err(VviCode::MatrixFull.val());
        }
        self.slots.push(MatrixSlot { proc_id, field_bit, value, basis });
        Ok(())
    }

    /// 查询裁决值。
    pub fn lookup(&self, proc_id: u32, field_bit: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.slots.len() {
            let s = &self.slots[i];
            if s.proc_id == proc_id && s.field_bit == field_bit {
                return Some(s.value);
            }
            i += 1;
        }
        None
    }

    pub fn used(&self) -> usize {
        self.slots.len()
    }
}

// ── 挂点请求与判定器 ────────────────────────────────────────────
/// VerifyVersionInfo 挂点请求：声称值 + 双掩码 + 路由。
pub struct VviRequest {
    /// 进程键。
    pub proc_id: u32,
    /// 逐域声称值（None=未声称）。
    pub claimed: [Option<u32>; FIELD_N],
    pub type_mask: u32,
    pub cond_mask: u64,
    pub route: Route,
}

/// 挂点判定：按冻结判定次序逐闸，过闸后逐域用矩阵值求布尔。
/// 全参与域真 → Ok(true)；任一域假 → Ok(false)；结构/策略/取值违约 → Err(码)。
pub fn vvi_hook(req: &VviRequest, matrix: &ProcMatrix) -> Result<bool, u32> {
    // 闸 1：类型掩码空。
    if req.type_mask == 0 {
        return Err(VviCode::TypeMaskEmpty.val());
    }
    // 闸 2：算子编码越界（按类型掩码逐域取 3 位槽）。
    let mut b = 0usize;
    while b < FIELD_N {
        if req.type_mask & (1u32 << b) != 0 {
            let c = ((req.cond_mask >> ((b as u32) * OP_BITS)) & 0b111) as u32;
            if CondOp::from_code(c).is_none() {
                return Err(VviCode::OpIllegal.val());
            }
        }
        b += 1;
    }
    // 闸 3：掩码错配——条件掩码高位置了类型掩码没有的域（登记域越出参与集）。
    let mut bb = 0usize;
    while bb < FIELD_N {
        // 8 域 × 3 位 = 24 位，逐 3 位槽扫（域闭集内）。
        let c = ((req.cond_mask >> ((bb as u32) * OP_BITS)) & 0b111) as u32;
        if c != 0 && (req.type_mask & (1u32 << bb)) == 0 {
            return Err(VviCode::MaskMismatch.val());
        }
        bb += 1;
    }
    // 闸 4：旁路拒绝（策略错误在结构错误之后）。
    if req.route == Route::DirectSystem {
        return Err(VviCode::BypassRefused.val());
    }
    // 闸 5：逐域取值+缺声称值检查，然后逐域真值表。
    let mut b2 = 0usize;
    while b2 < FIELD_N {
        if req.type_mask & (1u32 << b2) != 0 {
            let claimed = match req.claimed[b2] {
                Some(v) => v,
                None => return Err(VviCode::ClaimMissing.val()),
            };
            let actual = match matrix.lookup(req.proc_id, b2 as u32) {
                Some(v) => v,
                None => return Err(VviCode::NoMatrixValue.val()),
            };
            let c = ((req.cond_mask >> ((b2 as u32) * OP_BITS)) & 0b111) as u32;
            let op = match CondOp::from_code(c) {
                Some(o) => o,
                None => return Err(VviCode::OpIllegal.val()),
            };
            if !op.eval(actual, claimed) {
                return Ok(false);
            }
        }
        b2 += 1;
    }
    Ok(true)
}

// ── 对账语料（10 例合法判定 + 手算期望） ────────────────────────
/// 语料进程的矩阵注册（全部 8 域齐备）。
pub fn seed_matrix(matrix: &mut ProcMatrix, proc_id: u32) -> Result<(), u32> {
    // 冻结基线：10.0.19045 平台 2 SP 0.0 Suite 0x01C4 ProductType 1。
    matrix.register(proc_id, 0, 10, 71)?;
    matrix.register(proc_id, 1, 0, 72)?;
    matrix.register(proc_id, 2, 19_045, 73)?;
    matrix.register(proc_id, 3, 2, 74)?;
    matrix.register(proc_id, 4, 0, 75)?;
    matrix.register(proc_id, 5, 0, 76)?;
    matrix.register(proc_id, 6, 0x01C4, 77)?;
    matrix.register(proc_id, 7, 1, 78)?;
    Ok(())
}

/// 单例构造：按 (域, 算子, 声称值) 列表装配请求。
pub fn make_case(proc_id: u32, conds: &[(VviField, CondOp, u32)], route: Route) -> Result<VviRequest, u32> {
    let mut b = ConditionMaskBuilder::new();
    let mut claimed = [None; FIELD_N];
    let mut i = 0;
    while i < conds.len() {
        let (f, op, v) = conds[i];
        b.set_condition(f, op)?;
        claimed[f.bit() as usize] = Some(v);
        i += 1;
    }
    Ok(VviRequest {
        proc_id,
        claimed,
        type_mask: b.type_mask,
        cond_mask: b.cond_mask,
        route,
    })
}

/// 10 例对账语料：(请求, 手算期望)。
/// 期望全部按「矩阵值 × 算子真值表」手算落账。
pub fn make_recon_cases() -> [(VviRequest, bool); RECON_CASES] {
    let p = 7u32;
    // 0：Major Equal 10（矩阵 10）→ 真。
    let c0 = make_case(p, &[(VviField::Major, CondOp::Equal, 10)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 1：Major GreaterEqual 10（10>=10）→ 真。
    let c1 = make_case(p, &[(VviField::Major, CondOp::GreaterEqual, 10)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 2：Minor Greater 0（0>0 假）→ 假。
    let c2 = make_case(p, &[(VviField::Minor, CondOp::Greater, 0)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 3：Build LessEqual 20000（19045<=20000）→ 真。
    let c3 = make_case(p, &[(VviField::Build, CondOp::LessEqual, 20_000)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 4：Build Less 19045（19045<19045 假）→ 假。
    let c4 = make_case(p, &[(VviField::Build, CondOp::Less, 19_045)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 5：Platform NotEqual 3（2!=3）→ 真。
    let c5 = make_case(p, &[(VviField::Platform, CondOp::NotEqual, 3)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 6：ProductType Equal 1（矩阵 1）→ 真。
    let c6 = make_case(p, &[(VviField::ProductType, CondOp::Equal, 1)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 7：Suite Equal 0x01C4 → 真。
    let c7 = make_case(p, &[(VviField::Suite, CondOp::Equal, 0x01C4)], Route::MatrixRoute).unwrap_or(VVI_NIL);
    // 8：双域 AND：Major Equal 10 ∧ SpMajor Equal 0 → 真。
    let c8 = make_case(
        p,
        &[(VviField::Major, CondOp::Equal, 10), (VviField::SpMajor, CondOp::Equal, 0)],
        Route::MatrixRoute,
    )
    .unwrap_or(VVI_NIL);
    // 9：双域 AND 一真一假：Major Equal 10 ∧ Build Equal 1 → 假。
    let c9 = make_case(
        p,
        &[(VviField::Major, CondOp::Equal, 10), (VviField::Build, CondOp::Equal, 1)],
        Route::MatrixRoute,
    )
    .unwrap_or(VVI_NIL);
    [(c0, true), (c1, true), (c2, false), (c3, true), (c4, false), (c5, true), (c6, true), (c7, true), (c8, true), (c9, false)]
}

/// 全空请求哨兵（语料装配兜底；正常路径不触达）。
pub const VVI_NIL: VviRequest = VviRequest {
    proc_id: 0,
    claimed: [None; FIELD_N],
    type_mask: 0,
    cond_mask: 0,
    route: Route::MatrixRoute,
};

// ── 注入核（三类 ×10 轮必拒） ───────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽0 直连旁路 / 槽1 算子越界 / 槽2 掩码错配。
pub fn inject_rounds(rounds: u32, matrix: &ProcMatrix) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：合法条件 + DirectSystem 路由 → 必拒 BypassRefused。
        let mut req0 = match make_case(7, &[(VviField::Major, CondOp::Equal, 10)], Route::DirectSystem) {
            Ok(x) => x,
            Err(_) => VVI_NIL,
        };
        // 路由面单独覆盖（保持参数合法）。
        let hit0 = match vvi_hook(&req0, matrix) {
            Err(c) => c == VviCode::BypassRefused.val(),
            Ok(_) => false,
        };
        led.record(0, hit0);
        // 槽 1：类型掩码合法但算子槽写 7（越界编码）→ 必拒 OpIllegal。
        req0.route = Route::MatrixRoute;
        req0.type_mask = 1u32 << 0;
        req0.cond_mask = 0b111u64; // 槽 0 写满 111=7，越界。
        let hit1 = match vvi_hook(&req0, matrix) {
            Err(c) => c == VviCode::OpIllegal.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 槽 2：条件掩码登记域 1 但类型掩码只有域 0 → 必拒 MaskMismatch。
        req0.type_mask = 1u32 << 0;
        req0.cond_mask = 0b010u64 << (1 * OP_BITS); // 域 1 槽有编码。
        let hit2 = match vvi_hook(&req0, matrix) {
            Err(c) => c == VviCode::MaskMismatch.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        r += 1;
    }
    led
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：单域等值判定真。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut m = ProcMatrix::new();
    seed_matrix(&mut m, 3)?;
    let req = make_case(3, &[(VviField::Major, CondOp::Equal, 10)], Route::MatrixRoute)
        .unwrap_or(VVI_NIL);
    match vvi_hook(&req, &m) {
        Ok(true) => Ok(()),
        _ => Err(VviCode::NotReproducible.val()),
    }
}

/// 负探针 1：类型掩码空 → TypeMaskEmpty（次序闸 1）。
pub fn probe_seq_b() -> Result<(), u32> {
    let m = ProcMatrix::new();
    match vvi_hook(&VVI_NIL, &m) {
        Err(c) => if c == VviCode::TypeMaskEmpty.val() { Ok(()) } else { Err(c) },
        Ok(_) => Err(VviCode::NotReproducible.val()),
    }
}

/// 负探针 2：旁路路由 → BypassRefused。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = ProcMatrix::new();
    seed_matrix(&mut m, 4)?;
    let req = make_case(4, &[(VviField::Major, CondOp::Equal, 10)], Route::DirectSystem)
        .unwrap_or(VVI_NIL);
    match vvi_hook(&req, &m) {
        Err(c) => if c == VviCode::BypassRefused.val() { Ok(()) } else { Err(c) },
        Ok(_) => Err(VviCode::NotReproducible.val()),
    }
}

/// 黄金全链：10 例对账全过 + 注入 30/30 拒 + 双跑一致。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let mut m = ProcMatrix::new();
    seed_matrix(&mut m, 7)?;
    let cases = make_recon_cases();
    let mut pass = 0usize;
    let mut i = 0;
    while i < cases.len() {
        let (req, expect) = &cases[i];
        let got = vvi_hook(req, &m)?;
        if got != *expect {
            return Err(VviCode::NotReproducible.val());
        }
        pass += 1;
        i += 1;
    }
    let led = inject_rounds(JUDGE_ROUNDS, &m);
    if !led.all_caught() {
        return Err(VviCode::NotReproducible.val());
    }
    Ok((pass, m.used() as u32))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VviCode::ALL.len() {
        let v = VviCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18433_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18433 VerifyVersionInfo 消费挂点");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!("对账{:?}/{} 矩阵{}", g.as_ref().map(|r| r.0).unwrap_or(0), RECON_CASES, g.as_ref().map(|r| r.1).unwrap_or(0)),
    );
    // ② 10 例对账逐例期望一致（判定走矩阵输出）。
    let mut m = ProcMatrix::new();
    let seed_ok = seed_matrix(&mut m, 7).is_ok();
    let cases = make_recon_cases();
    let mut recon_ok = seed_ok;
    let mut i = 0;
    while i < cases.len() && recon_ok {
        match vvi_hook(&cases[i].0, &m) {
            Ok(v) => recon_ok = v == cases[i].1,
            Err(_) => recon_ok = false,
        }
        i += 1;
    }
    s.add("对账10例逐例一致", recon_ok, "矩阵值×算子真值表");
    // ③ 注入三类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS, &m);
    s.add_dyn("旁路注入10/10拒", inj.caught[0] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[0], inj.tries[0]));
    s.add_dyn("算子越界10/10拒", inj.caught[1] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[1], inj.tries[1]));
    s.add_dyn("掩码错配10/10拒", inj.caught[2] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[2], inj.tries[2]));
    s.add("注入漏检为零", inj.missed() == 0, "检出+漏检=注入");
    // ④ 判定次序：旁路+算子越界同时成立 → 先报算子越界（结构先于策略）。
    let mut order_req = match make_case(7, &[(VviField::Major, CondOp::Equal, 10)], Route::DirectSystem) {
        Ok(x) => x,
        Err(_) => VVI_NIL,
    };
    order_req.type_mask = 1;
    order_req.cond_mask = 0b111u64;
    let order_hit = match vvi_hook(&order_req, &m) {
        Err(c) => c == VviCode::OpIllegal.val(),
        Ok(_) => false,
    };
    s.add("判定次序结构先于策略", order_hit, "越界先于旁路");
    // ⑤ 状态机：同域二次登记非法迁移。
    let mut b2 = ConditionMaskBuilder::new();
    let sm1 = b2.set_condition(VviField::Major, CondOp::Equal).is_ok();
    let sm2 = b2.set_condition(VviField::Major, CondOp::Less).is_err();
    s.add("登记重复必拒", sm1 && sm2, "同域二次 set 拒");
    // ⑥ 边界：全 8 域参与判定真（满掩码）。
    let full = make_case(
        7,
        &[
            (VviField::Major, CondOp::Equal, 10),
            (VviField::Minor, CondOp::Equal, 0),
            (VviField::Build, CondOp::Equal, 19_045),
            (VviField::Platform, CondOp::Equal, 2),
            (VviField::SpMajor, CondOp::Equal, 0),
            (VviField::SpMinor, CondOp::Equal, 0),
            (VviField::Suite, CondOp::Equal, 0x01C4),
            (VviField::ProductType, CondOp::Equal, 1),
        ],
        Route::MatrixRoute,
    );
    let full_ok = match full {
        Ok(r) => match vvi_hook(&r, &m) {
            Ok(v) => v,
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("满掩码8域AND判定", full_ok, "全域参与逐域真");
    // ⑦ 边界：空掩码拒（闸 1）。
    let empty_ok = match vvi_hook(&VVI_NIL, &m) {
        Err(c) => c == VviCode::TypeMaskEmpty.val(),
        Ok(_) => false,
    };
    s.add("空掩码边界拒", empty_ok, "无参与域不构成判定");
    // ⑧ 取值边界：矩阵无值域 → NoMatrixValue。
    let unreg = make_case(9, &[(VviField::Major, CondOp::Equal, 10)], Route::MatrixRoute);
    let unreg_ok = match unreg {
        Ok(r) => match vvi_hook(&r, &m) {
            Err(c) => c == VviCode::NoMatrixValue.val(),
            Ok(_) => false,
        },
        Err(_) => false,
    };
    s.add("矩阵无值显式拒", unreg_ok, "未登记进程域不给判定");
    // ⑨ 注册边界：依据为空拒 + 矩阵满记账（96 槽填满后第 97 键被拒）。
    let mut m2 = ProcMatrix::new();
    let reg_ok = m2.register(1, 0, 10, 0).is_err();
    let mut m3 = ProcMatrix::new();
    let mut filled = 0usize;
    let mut p = 0;
    while p < 12 && filled < MATRIX_CAP {
        let mut f = 0;
        while f < FIELD_N && filled < MATRIX_CAP {
            if m3.register(p, f as u32, 1, 1).is_ok() {
                filled += 1;
            }
            f += 1;
        }
        p += 1;
    }
    let full_reject = m3.register(99, 0, 1, 1).is_err();
    s.add("注册边界齐备", reg_ok && full_reject && filled == MATRIX_CAP, "空依据拒+矩阵满拒");
    // ⑩ 双跑一致。
    let a = golden_run();
    let b = golden_run();
    s.add("双跑一致", a == b, "同语料同判定");
    // ⑪ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(), "正/空掩码/旁路");
    // ⑫ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < VviCode::ALL.len() {
        if VviCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "11 码全有中文建议");
    s.add("段位守卫 0x7E800", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recon_ten_cases_match_expectation() {
        let mut m = ProcMatrix::new();
        assert!(seed_matrix(&mut m, 7).is_ok());
        for (req, expect) in make_recon_cases().iter() {
            let got = vvi_hook(req, &m);
            assert_eq!(got, Ok(*expect), "对账例偏离手算期望");
        }
    }

    #[test]
    fn bypass_rejected_ten_rounds() {
        let mut m = ProcMatrix::new();
        seed_matrix(&mut m, 7).unwrap_or(());
        let inj = inject_rounds(10, &m);
        assert_eq!(inj.caught[0], 10, "旁路必须 10/10 拒");
        assert!(inj.all_caught());
    }

    #[test]
    fn judge_order_structure_before_policy() {
        let mut m = ProcMatrix::new();
        seed_matrix(&mut m, 7).unwrap_or(());
        let mut req = VVI_NIL;
        req.route = Route::DirectSystem;
        // 类型掩码空 + 旁路：先报掩码空（闸 1 在旁路之前）。
        match vvi_hook(&req, &m) {
            Err(c) => assert_eq!(c, VviCode::TypeMaskEmpty.val()),
            Ok(_) => panic!("空掩码必须拒"),
        }
        // 掩码合法 + 算子越界 + 旁路：先报算子越界。
        req.type_mask = 1;
        req.cond_mask = 0b111;
        match vvi_hook(&req, &m) {
            Err(c) => assert_eq!(c, VviCode::OpIllegal.val()),
            Ok(_) => panic!("算子越界必须拒"),
        }
    }

    #[test]
    fn condition_mask_state_machine() {
        let mut b = ConditionMaskBuilder::new();
        assert!(b.set_condition(VviField::Major, CondOp::GreaterEqual).is_ok());
        assert!(b.set_condition(VviField::Build, CondOp::Less).is_ok());
        assert!(b.set_condition(VviField::Major, CondOp::Equal).is_err(), "重复登记必拒");
        assert_eq!(b.op_code_of(0), CondOp::GreaterEqual.code());
        assert_eq!(b.op_code_of(2), CondOp::Less.code());
        assert_eq!(b.op_code_of(3), 0, "未登记域编码为 0");
    }

    #[test]
    fn full_mask_and_boundary() {
        let mut m = ProcMatrix::new();
        seed_matrix(&mut m, 7).unwrap_or(());
        let full = make_case(
            7,
            &[
                (VviField::Major, CondOp::Equal, 10),
                (VviField::Minor, CondOp::Equal, 0),
                (VviField::Build, CondOp::Equal, 19_045),
                (VviField::Platform, CondOp::Equal, 2),
                (VviField::SpMajor, CondOp::Equal, 0),
                (VviField::SpMinor, CondOp::Equal, 0),
                (VviField::Suite, CondOp::Equal, 0x01C4),
                (VviField::ProductType, CondOp::Equal, 1),
            ],
            Route::MatrixRoute,
        )
        .unwrap_or(VVI_NIL);
        assert_eq!(vvi_hook(&full, &m), Ok(true), "满掩码全真");
        assert_eq!(vvi_hook(&VVI_NIL, &m), Err(VviCode::TypeMaskEmpty.val()));
    }

    #[test]
    fn probes_all_sides() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn double_run_identical() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }

    #[test]
    fn matrix_register_edges() {
        let mut m = ProcMatrix::new();
        assert!(m.register(1, 0, 10, 0).is_err(), "空依据拒");
        assert!(m.register(1, 9, 10, 1).is_err(), "域越界拒");
        assert!(m.register(1, 0, 10, 1).is_ok());
        assert_eq!(m.lookup(1, 0), Some(10));
        assert_eq!(m.lookup(2, 0), None);
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18433_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
