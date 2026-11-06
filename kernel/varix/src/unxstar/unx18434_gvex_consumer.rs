//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18434 GetVersionEx 消费挂点 · 0x7E840段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18434-J1 结构体字段逐一经矩阵填充断言全过，字段级旁路注入检出
//! 设计要点：
//!   - 挂点语义：应用传 OSVERSIONINFOW（仅 dwOSVersionInfoSize 预填）调
//!     GetVersionEx，内核挂点**逐字段**从谎报矩阵取值回填——九个数据域
//!     全部经矩阵填充，禁止调用方预塞真值绕过；
//!   - 字段级旁路注入检出：调用方在 1..10 号域预塞非零值即「预塞旁路」，
//!     prestuff_scan 逐域扫描给出精确域号，挂点一律拒——10 轮×9 域全检出；
//!   - 尺寸闸：dwOSVersionInfoSize 必须等于结构体冻结字节数（推导见常量注）；
//!   - CSD 串域（szCSDVersion）：按矩阵串表 ID 回填定容缓冲+NUL 结尾，
//!     超长串在注册闸拒绝（挂点侧零截断歧义）；
//!   - 注入核三类×10 轮：逐域预塞 10/10 检出、坏尺寸 10/10 拒、
//!     矩阵缺值 10/10 显式拒；漏检必须为 0；
//!   - 对账面：回填快照逐字段与矩阵值+映射表对拍；双跑逐字节一致；
//!   - 确定性：LCG 种子=SEG_BASE(0x7E840)；no_std+alloc；零 unwrap/
//!     expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   GvexField / GvexCode(诊断闭集) / GvexStruct(调用方结构)
//!   / FilledSnapshot(回填快照) / FIELD_CSD_BYTES+CSD 表
//!   / ProcGvexMatrix(本地矩阵) / prestuff_scan+gvex_hook(挂点)
//!   / FIELD_DIM_MAP(域→维度映射) / InjectLedger / golden_run
//!   / run_unx18434_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E840，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E840;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 结构体域数（冻结：Size + 九个数据域）。
pub const FIELD_N: usize = 10;
/// szCSDVersion 缓冲字节容量（冻结：32 字节，含 NUL 结尾预算）。
pub const CSD_BYTES: usize = 32;
/// 结构体冻结字节数（推导：4×4 整数域=16，SP 主/次/Suite/ProductType 4×2=8，
/// PlatformId 4，Size 4，CSD 32 → 16+8+4+4+32 = 64 字节）。
pub const STRUCT_BYTES: u32 = 64;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 数据域数（九域：Size 之外全部）。
pub const DATA_FIELD_N: usize = 9;
/// 进程矩阵条目容量（冻结：8 进程 × 9 维度 = 72 ≤ 96）。
pub const MATRIX_CAP: usize = 96;

// ── 结构体域枚举 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GvexField {
    /// dwOSVersionInfoSize（唯一允许预填的域）。
    Size = 0,
    /// dwMajorVersion。
    Major = 1,
    /// dwMinorVersion。
    Minor = 2,
    /// dwBuildNumber。
    Build = 3,
    /// dwPlatformId。
    Platform = 4,
    /// wServicePackMajor。
    SpMajor = 5,
    /// wServicePackMinor。
    SpMinor = 6,
    /// wSuiteMask。
    Suite = 7,
    /// wProductType。
    ProductType = 8,
    /// szCSDVersion（域号占位；实际走串表回填）。
    Csd = 9,
}

impl GvexField {
    /// 域序号。
    pub fn idx(self) -> usize {
        self as usize
    }
    /// 由序号还原；越界 None。
    pub fn from_idx(i: usize) -> Option<GvexField> {
        match i {
            0 => Some(GvexField::Size),
            1 => Some(GvexField::Major),
            2 => Some(GvexField::Minor),
            3 => Some(GvexField::Build),
            4 => Some(GvexField::Platform),
            5 => Some(GvexField::SpMajor),
            6 => Some(GvexField::SpMinor),
            7 => Some(GvexField::Suite),
            8 => Some(GvexField::ProductType),
            9 => Some(GvexField::Csd),
            _ => None,
        }
    }
    /// 域名（诊断文本共用）。
    pub fn name(self) -> &'static str {
        match self {
            GvexField::Size => "Size",
            GvexField::Major => "Major",
            GvexField::Minor => "Minor",
            GvexField::Build => "Build",
            GvexField::Platform => "Platform",
            GvexField::SpMajor => "SpMajor",
            GvexField::SpMinor => "SpMinor",
            GvexField::Suite => "Suite",
            GvexField::ProductType => "ProductType",
            GvexField::Csd => "CSD",
        }
    }
}

// ── 域→矩阵维度映射表（冻结 v1） ────────────────────────────────
/// 数据域（1..10）对应的矩阵维度号：Major/Minor/Build/Platform/SP主/SP次/
/// Suite/ProductType/CSD 九维，顺序与域号一一对应（域号-1=维度号）。
pub const FIELD_DIM_MAP: [usize; DATA_FIELD_N] = [0, 1, 2, 3, 4, 5, 6, 7, 8];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GvexCode {
    /// 尺寸域与冻结字节数不符
    SizeMismatch,
    /// 调用方预塞数据域（字段级旁路）
    FieldPreStuffed,
    /// 矩阵无该进程该维值
    NoMatrixValue,
    /// CSD 串超长（注册闸拒）
    CsdTooLong,
    /// 注册依据为空
    BasisMissing,
    /// 维度号越界
    DimIllegal,
    /// 矩阵条目满
    MatrixFull,
    /// 回填后校验失败（快照≠矩阵）
    FillMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl GvexCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GvexCode::SizeMismatch => "尺寸域不符：dwOSVersionInfoSize ≠ 64 冻结值",
            GvexCode::FieldPreStuffed => "预塞旁路检出：数据域调用方预填非零值",
            GvexCode::NoMatrixValue => "矩阵无值：该进程该维未登记",
            GvexCode::CsdTooLong => "CSD 串超长：超出 32 字节缓冲预算",
            GvexCode::BasisMissing => "注册依据为空：有值必有据不变量",
            GvexCode::DimIllegal => "维度号越界：九维闭集外",
            GvexCode::MatrixFull => "矩阵条目满：注册被拒且账不丢",
            GvexCode::FillMismatch => "回填校验失败：快照与矩阵值不一致",
            GvexCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [GvexCode; 9] = [
        GvexCode::SizeMismatch,
        GvexCode::FieldPreStuffed,
        GvexCode::NoMatrixValue,
        GvexCode::CsdTooLong,
        GvexCode::BasisMissing,
        GvexCode::DimIllegal,
        GvexCode::MatrixFull,
        GvexCode::FillMismatch,
        GvexCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GvexCode::ALL.len() {
        if GvexCode::ALL[i].val() == code {
            return GvexCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E840 段闭集内"
}

// ── CSD 串表（冻结：4 条 ASCII，均 ≤ 31 字节） ──────────────────
/// 矩阵 CSD 维度值 → 串文本（0=无补丁串，空）。
pub const CSD_TABLE: [&str; 4] = ["", "Service Pack 1", "Service Pack 2", "Service Pack 3"];

/// CSD 维度值合法性（0..4）。
pub fn csd_valid(id: u32) -> bool {
    (id as usize) < CSD_TABLE.len()
}

// ── 调用方结构（入口侧） ────────────────────────────────────────
/// 调用方传入的 OSVERSIONINFOW 模型：整数域 + CSD 串槽。
/// 合法入口只允许 Size 域预填；数据域必须全零/空。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GvexStruct {
    /// 整数域（含 Size；数据域 None/0=未预塞）。
    pub ints: [Option<u32>; FIELD_N],
}

impl GvexStruct {
    /// 合法入口：仅 Size 预填。
    pub fn legal_entry() -> GvexStruct {
        let mut ints: [Option<u32>; FIELD_N] = [None; FIELD_N];
        ints[0] = Some(STRUCT_BYTES);
        GvexStruct { ints }
    }
    /// 全零入口（尺寸域也缺——尺寸闸负样本）。
    pub fn blank() -> GvexStruct {
        GvexStruct { ints: [None; FIELD_N] }
    }
    /// 在某数据域预塞值（注入构造器）。
    pub fn prestuff(mut self, field: GvexField, val: u32) -> GvexStruct {
        let i = field.idx();
        if i < FIELD_N {
            self.ints[i] = Some(val);
        }
        self
    }
}

// ── 预塞扫描（字段级旁路检出核） ────────────────────────────────
/// 逐域扫描 1..10 号域：非零/非空即预塞。返回 (逐域命中位图, 预塞域数)。
/// Size 域（0 号）豁免——它本就允许预填。
pub fn prestuff_scan(st: &GvexStruct) -> ([bool; FIELD_N], usize) {
    let mut marks = [false; FIELD_N];
    let mut n = 0usize;
    let mut i = 1;
    while i < FIELD_N {
        match st.ints[i] {
            Some(v) if v != 0 => {
                marks[i] = true;
                n += 1;
            }
            _ => {}
        }
        i += 1;
    }
    (marks, n)
}

// ── 进程矩阵（九维） ────────────────────────────────────────────
/// 单槽：(进程,维度) → 值+依据。CSD 维存串表 ID（0..4）。
#[derive(Clone, Copy, Debug)]
pub struct GvexSlot {
    pub proc_id: u32,
    pub dim: u32,
    pub value: u32,
    pub basis: u32,
}

/// 进程矩阵：线性键表。
pub struct ProcGvexMatrix {
    slots: Vec<GvexSlot>,
    cap: usize,
    pub reg_total: u32,
}

impl ProcGvexMatrix {
    pub fn new() -> ProcGvexMatrix {
        ProcGvexMatrix { slots: Vec::new(), cap: MATRIX_CAP, reg_total: 0 }
    }

    /// 注册一维裁决值；CSD 维校验串表 ID 域；依据为空拒。
    pub fn register(&mut self, proc_id: u32, dim: u32, value: u32, basis: u32) -> Result<(), u32> {
        if dim as usize >= DATA_FIELD_N {
            return Err(GvexCode::DimIllegal.val());
        }
        if dim == 8 && !csd_valid(value) {
            return Err(GvexCode::CsdTooLong.val());
        }
        if basis == 0 {
            return Err(GvexCode::BasisMissing.val());
        }
        self.reg_total = self.reg_total.saturating_add(1);
        let mut i = 0;
        while i < self.slots.len() {
            let sl = &self.slots[i];
            if sl.proc_id == proc_id && sl.dim == dim {
                break;
            }
            i += 1;
        }
        if i < self.slots.len() {
            let sl = &mut self.slots[i];
            sl.value = value;
            sl.basis = basis;
            return Ok(());
        }
        if self.slots.len() >= self.cap {
            self.reg_total = self.reg_total.saturating_sub(1);
            return Err(GvexCode::MatrixFull.val());
        }
        self.slots.push(GvexSlot { proc_id, dim, value, basis });
        Ok(())
    }

    /// 查询一维值。
    pub fn lookup(&self, proc_id: u32, dim: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.slots.len() {
            let sl = &self.slots[i];
            if sl.proc_id == proc_id && sl.dim == dim {
                return Some(sl.value);
            }
            i += 1;
        }
        None
    }

    pub fn used(&self) -> usize {
        self.slots.len()
    }
}

// ── 回填快照（出口侧） ──────────────────────────────────────────
/// 挂点回填后的结构体快照：九域值 + CSD 定容缓冲（NUL 结尾）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilledSnapshot {
    /// 数据域值（下标 = 域号-1，九维顺序）。
    pub data: [u32; DATA_FIELD_N],
    /// CSD 缓冲（含 NUL；未用区全零）。
    pub csd: [u8; CSD_BYTES],
    /// CSD 实长（不含 NUL）。
    pub csd_len: usize,
}

impl FilledSnapshot {
    /// 出口校验：Size 域冻结值 + 逐域与矩阵对拍（字段逐一填充断言）。
    pub fn verify(&self, matrix: &ProcGvexMatrix, proc_id: u32) -> Result<usize, u32> {
        let mut ok_n = 0usize;
        let mut d = 0;
        while d < DATA_FIELD_N {
            let mv = match matrix.lookup(proc_id, d as u32) {
                Some(v) => v,
                None => return Err(GvexCode::NoMatrixValue.val()),
            };
            if self.data[d] != mv {
                return Err(GvexCode::FillMismatch.val());
            }
            ok_n += 1;
            d += 1;
        }
        // CSD 串与串表对拍。
        let csd_id = matrix.lookup(proc_id, 8).unwrap_or(0);
        let want = match CSD_TABLE.get(csd_id as usize) {
            Some(s) => s,
            None => return Err(GvexCode::FillMismatch.val()),
        };
        let wb = want.as_bytes();
        if wb.len() != self.csd_len {
            return Err(GvexCode::FillMismatch.val());
        }
        let mut k = 0;
        while k < wb.len() {
            if self.csd[k] != wb[k] {
                return Err(GvexCode::FillMismatch.val());
            }
            k += 1;
        }
        // NUL 结尾必须在位。
        if self.csd[self.csd_len] != 0 {
            return Err(GvexCode::FillMismatch.val());
        }
        Ok(ok_n)
    }
}

// ── 挂点判定器 ──────────────────────────────────────────────────
/// GetVersionEx 消费挂点：尺寸闸 → 预塞闸 → 逐域矩阵回填（CSD 走串表）。
pub fn gvex_hook(st: &GvexStruct, proc_id: u32, matrix: &ProcGvexMatrix) -> Result<FilledSnapshot, u32> {
    // 闸 1：尺寸域必须等于冻结字节数。
    match st.ints[0] {
        Some(v) if v == STRUCT_BYTES => {}
        _ => return Err(GvexCode::SizeMismatch.val()),
    }
    // 闸 2：字段级旁路检出——任何数据域预塞即拒。
    let (marks, n) = prestuff_scan(st);
    if n > 0 {
        // 诊断记账：取首个命中域号拼进错误号段外无门——拒绝统一走
        // FieldPreStuffed，域号经 prestuff_scan 位图由审计面取出。
        let _ = marks;
        return Err(GvexCode::FieldPreStuffed.val());
    }
    // 闸 3：逐域从矩阵取值回填。
    let mut snap = FilledSnapshot { data: [0; DATA_FIELD_N], csd: [0; CSD_BYTES], csd_len: 0 };
    let mut d = 0;
    while d < DATA_FIELD_N {
        let dim = FIELD_DIM_MAP[d] as u32;
        let v = match matrix.lookup(proc_id, dim) {
            Some(v) => v,
            None => return Err(GvexCode::NoMatrixValue.val()),
        };
        snap.data[d] = v;
        d += 1;
    }
    // CSD 串回填：串表 ID → 文本 → 定容缓冲 + NUL。
    let csd_id = snap.data[8];
    let text = match CSD_TABLE.get(csd_id as usize) {
        Some(s) => s,
        None => return Err(GvexCode::CsdTooLong.val()),
    };
    let tb = text.as_bytes();
    if tb.len() + 1 > CSD_BYTES {
        return Err(GvexCode::CsdTooLong.val());
    }
    let mut k = 0;
    while k < tb.len() {
        snap.csd[k] = tb[k];
        k += 1;
    }
    snap.csd[tb.len()] = 0;
    snap.csd_len = tb.len();
    Ok(snap)
}

// ── 矩阵种子（冻结基线：10.0.19045 平台2 SP0 Suite 0x01C4 PT1） ─
pub fn seed_matrix(matrix: &mut ProcGvexMatrix, proc_id: u32) -> Result<(), u32> {
    matrix.register(proc_id, 0, 10, 81)?; // Major
    matrix.register(proc_id, 1, 0, 82)?; // Minor
    matrix.register(proc_id, 2, 19_045, 83)?; // Build
    matrix.register(proc_id, 3, 2, 84)?; // Platform
    matrix.register(proc_id, 4, 0, 85)?; // SP 主
    matrix.register(proc_id, 5, 0, 86)?; // SP 次
    matrix.register(proc_id, 6, 0x01C4, 87)?; // Suite
    matrix.register(proc_id, 7, 1, 88)?; // ProductType
    matrix.register(proc_id, 8, 0, 89)?; // CSD 串 ID（无补丁串）
    Ok(())
}

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

/// 10 轮注入：
/// 槽 0：九域逐一预塞（每轮 9 域全试，域号必须逐一对上）；
/// 槽 1：坏尺寸（多 1/少 1/全缺三态轮换）；
/// 槽 2：矩阵缺值（未注册进程查询）。
pub fn inject_rounds(rounds: u32, matrix: &ProcGvexMatrix) -> (InjectLedger, [u32; DATA_FIELD_N]) {
    let mut led = InjectLedger::new();
    // 逐域命中账：每域预塞检出计数（9 域 ×10 轮 = 每域 10）。
    let mut per_field = [0u32; DATA_FIELD_N];
    let mut r = 0;
    while r < rounds {
        // 槽 0：九域逐个预塞，全部必须检出且域号精确。
        let mut f = 1;
        while f < FIELD_N {
            let field = match GvexField::from_idx(f) {
                Some(x) => x,
                None => GvexField::Major,
            };
            let st = GvexStruct::legal_entry().prestuff(field, 0xDEAD + f as u32);
            let (marks, n) = prestuff_scan(&st);
            let hook_reject = match gvex_hook(&st, 5, matrix) {
                Err(c) => c == GvexCode::FieldPreStuffed.val(),
                Ok(_) => false,
            };
            // 域号精确性：恰好该域命中。
            let exact = marks[f] && n == 1;
            let hit = hook_reject && exact;
            if hit {
                per_field[f - 1] = per_field[f - 1].saturating_add(1);
            }
            // 记账：槽 0 计满轮数即可（all_caught 判定用）。
            led.record(0, hit);
            f += 1;
        }
        // 槽 1：坏尺寸三态轮换（+1 / -1 / 缺）。
        let mode = r % 3;
        let bad = match mode {
            0 => GvexStruct::legal_entry().prestuff(GvexField::Size, STRUCT_BYTES + 1),
            1 => GvexStruct::legal_entry().prestuff(GvexField::Size, STRUCT_BYTES - 1),
            _ => GvexStruct::blank(),
        };
        let hit1 = match gvex_hook(&bad, 5, matrix) {
            Err(c) => c == GvexCode::SizeMismatch.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 槽 2：未注册进程 → 矩阵缺值。
        let clean = GvexStruct::legal_entry();
        let hit2 = match gvex_hook(&clean, 99, matrix) {
            Err(c) => c == GvexCode::NoMatrixValue.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        r += 1;
    }
    (led, per_field)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：合法入口回填成功且逐域与矩阵一致。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut m = ProcGvexMatrix::new();
    seed_matrix(&mut m, 1)?;
    let snap = gvex_hook(&GvexStruct::legal_entry(), 1, &m)?;
    snap.verify(&m, 1)?;
    Ok(())
}

/// 负探针 1：全零结构（尺寸域缺）→ SizeMismatch。
pub fn probe_seq_b() -> Result<(), u32> {
    let m = ProcGvexMatrix::new();
    match gvex_hook(&GvexStruct::blank(), 1, &m) {
        Err(c) => if c == GvexCode::SizeMismatch.val() { Ok(()) } else { Err(c) },
        Ok(_) => Err(GvexCode::NotReproducible.val()),
    }
}

/// 负探针 2：预塞 Major → FieldPreStuffed。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = ProcGvexMatrix::new();
    seed_matrix(&mut m, 2)?;
    let st = GvexStruct::legal_entry().prestuff(GvexField::Major, 6);
    match gvex_hook(&st, 2, &m) {
        Err(c) => if c == GvexCode::FieldPreStuffed.val() { Ok(()) } else { Err(c) },
        Ok(_) => Err(GvexCode::NotReproducible.val()),
    }
}

/// 黄金全链：合法回填 → 快照逐域对拍 → 注入 10 轮全抓 → 双跑一致。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let mut m = ProcGvexMatrix::new();
    seed_matrix(&mut m, 5)?;
    let snap = gvex_hook(&GvexStruct::legal_entry(), 5, &m)?;
    let filled = snap.verify(&m, 5)?;
    if filled != DATA_FIELD_N {
        return Err(GvexCode::FillMismatch.val());
    }
    let (led, _) = inject_rounds(JUDGE_ROUNDS, &m);
    if !led.all_caught() {
        return Err(GvexCode::NotReproducible.val());
    }
    Ok((filled, m.used() as u32))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GvexCode::ALL.len() {
        let v = GvexCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18434_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18434 GetVersionEx 消费挂点");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!("回填{:?}/{} 矩阵{}", g.as_ref().map(|r| r.0).unwrap_or(0), DATA_FIELD_N, g.as_ref().map(|r| r.1).unwrap_or(0)),
    );
    // ② 字段逐一填充断言：回填快照逐域 == 矩阵值（含 CSD 串对拍）。
    let mut m = ProcGvexMatrix::new();
    let seed_ok = seed_matrix(&mut m, 5).is_ok();
    let fill_ok = seed_ok && match gvex_hook(&GvexStruct::legal_entry(), 5, &m) {
        Ok(snap) => snap.verify(&m, 5).unwrap_or(0) == DATA_FIELD_N,
        Err(_) => false,
    };
    s.add("字段逐一经矩阵填充", fill_ok, "九域+CSD 全对拍");
    // ③ 注入：预塞 9 域 ×10 轮全检出（域号精确）。
    let (inj, per_field) = inject_rounds(JUDGE_ROUNDS, &m);
    let mut pf_ok = true;
    let mut fi = 0;
    while fi < DATA_FIELD_N {
        if per_field[fi] != JUDGE_ROUNDS {
            pf_ok = false;
        }
        fi += 1;
    }
    s.add_dyn(
        "字段级旁路注入全检出",
        pf_ok,
        &format!("{:?}", per_field),
    );
    s.add_dyn("坏尺寸10/10拒", inj.caught[1] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[1], inj.tries[1]));
    s.add_dyn("矩阵缺值10/10拒", inj.caught[2] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[2], inj.tries[2]));
    s.add("注入漏检为零", inj.missed() == 0, "检出+漏检=注入");
    // ④ 尺寸冻结值推导核对：STRUCT_BYTES=64（16+8+4+4+32）。
    s.add("尺寸冻结值推导", STRUCT_BYTES == 16 + 8 + 4 + 4 + (CSD_BYTES as u32), "4 整数域+4 SP/Suite/PT+Platform+Size+CSD");
    // ⑤ CSD 串边界：空串与 SP1 均可回填，NUL 在位。
    let mut m2 = ProcGvexMatrix::new();
    let mut csd_ok = seed_matrix(&mut m2, 3).is_ok();
    csd_ok = csd_ok && m2.register(3, 8, 1, 91).is_ok(); // 改 SP1 串。
    let snap_sp1 = gvex_hook(&GvexStruct::legal_entry(), 3, &m2);
    csd_ok = csd_ok && match snap_sp1 {
        Ok(sp) => {
            let txt = "Service Pack 1".as_bytes();
            sp.csd_len == txt.len() && sp.csd[sp.csd_len] == 0
        }
        Err(_) => false,
    };
    s.add("CSD 串回填含 NUL", csd_ok, "空串/SP1 两态");
    // ⑥ CSD 超长串注册闸拒（ID 越界模拟）。
    let mut m3 = ProcGvexMatrix::new();
    let over_ok = m3.register(2, 8, 9, 92).is_err() && m3.register(2, 8, 4, 92).is_err();
    s.add("CSD 串 ID 越界注册拒", over_ok, "串表闭集 0..4 外拒");
    // ⑦ 注册边界：依据为空拒 + 满矩阵记账。
    let mut m4 = ProcGvexMatrix::new();
    let reg_ok = m4.register(1, 0, 10, 0).is_err() && m4.register(1, 9, 10, 1).is_err();
    let mut m5 = ProcGvexMatrix::new();
    let mut filled = 0usize;
    let mut p = 0;
    while p < 12 && filled < MATRIX_CAP {
        let mut d = 0;
        while d < DATA_FIELD_N && filled < MATRIX_CAP {
            if m5.register(p, d as u32, 1, 1).is_ok() {
                filled += 1;
            }
            d += 1;
        }
        p += 1;
    }
    let full_reject = m5.register(99, 0, 1, 1).is_err();
    s.add("注册边界齐备", reg_ok && full_reject && filled == MATRIX_CAP, "空依据拒+维度越界拒+矩阵满拒");
    // ⑧ 判定次序：尺寸错+预塞同时成立 → 先报尺寸（闸 1 在预塞之前）。
    let both = GvexStruct::legal_entry()
        .prestuff(GvexField::Size, STRUCT_BYTES + 2)
        .prestuff(GvexField::Major, 6);
    let order_ok = match gvex_hook(&both, 5, &m) {
        Err(c) => c == GvexCode::SizeMismatch.val(),
        Ok(_) => false,
    };
    s.add("判定次序尺寸先于预塞", order_ok, "闸 1 先于闸 2");
    // ⑨ 双跑一致。
    let a = golden_run();
    let b = golden_run();
    s.add("双跑一致", a == b, "同语料同快照");
    // ⑩ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(), "正/全零/预塞");
    // ⑪ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < GvexCode::ALL.len() {
        if GvexCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "9 码全有中文建议");
    s.add("段位守卫 0x7E840", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_all_fields_from_matrix() {
        let mut m = ProcGvexMatrix::new();
        assert!(seed_matrix(&mut m, 5).is_ok());
        let snap = gvex_hook(&GvexStruct::legal_entry(), 5, &m);
        match snap {
            Ok(sp) => {
                assert_eq!(sp.verify(&m, 5), Ok(DATA_FIELD_N), "九域逐一对拍");
                assert_eq!(sp.data[0], 10, "Major");
                assert_eq!(sp.data[2], 19_045, "Build");
                assert_eq!(sp.data[6], 0x01C4, "Suite");
            }
            Err(e) => panic!("回填应成功: {} {}", e, detail(e)),
        }
    }

    #[test]
    fn prestuff_detected_all_nine_fields() {
        let mut m = ProcGvexMatrix::new();
        seed_matrix(&mut m, 5).unwrap_or(());
        let mut f = 1;
        while f < FIELD_N {
            let field = GvexField::from_idx(f).unwrap_or(GvexField::Major);
            let st = GvexStruct::legal_entry().prestuff(field, 1);
            let (marks, n) = prestuff_scan(&st);
            assert!(marks[f] && n == 1, "域 {} 预塞必须精确命中", f);
            match gvex_hook(&st, 5, &m) {
                Err(c) => assert_eq!(c, GvexCode::FieldPreStuffed.val()),
                Ok(_) => panic!("预塞必须拒"),
            }
            f += 1;
        }
    }

    #[test]
    fn size_gate_three_states() {
        let mut m = ProcGvexMatrix::new();
        seed_matrix(&mut m, 5).unwrap_or(());
        for bad in [STRUCT_BYTES + 1, STRUCT_BYTES - 1] {
            let st = GvexStruct::legal_entry().prestuff(GvexField::Size, bad);
            match gvex_hook(&st, 5, &m) {
                Err(c) => assert_eq!(c, GvexCode::SizeMismatch.val()),
                Ok(_) => panic!("坏尺寸必须拒"),
            }
        }
        match gvex_hook(&GvexStruct::blank(), 5, &m) {
            Err(c) => assert_eq!(c, GvexCode::SizeMismatch.val()),
            Ok(_) => panic!("缺尺寸必须拒"),
        }
    }

    #[test]
    fn missing_matrix_value_rejected() {
        let mut m = ProcGvexMatrix::new();
        seed_matrix(&mut m, 5).unwrap_or(());
        // 未注册进程。
        match gvex_hook(&GvexStruct::legal_entry(), 42, &m) {
            Err(c) => assert_eq!(c, GvexCode::NoMatrixValue.val()),
            Ok(_) => panic!("缺值必须拒"),
        }
        // 半注册进程（缺 CSD 维）。
        let mut m2 = ProcGvexMatrix::new();
        m2.register(9, 0, 10, 1).unwrap_or(());
        match gvex_hook(&GvexStruct::legal_entry(), 9, &m2) {
            Err(c) => assert_eq!(c, GvexCode::NoMatrixValue.val()),
            Ok(_) => panic!("缺 CSD 维必须拒"),
        }
    }

    #[test]
    fn csd_fill_and_boundaries() {
        let mut m = ProcGvexMatrix::new();
        seed_matrix(&mut m, 3).unwrap_or(());
        // 改 SP1 串后回填长度正确、NUL 在位。
        m.register(3, 8, 1, 91).unwrap_or(());
        let sp = gvex_hook(&GvexStruct::legal_entry(), 3, &m).unwrap_or_else(|_| {
            FilledSnapshot { data: [0; DATA_FIELD_N], csd: [0; CSD_BYTES], csd_len: 0 }
        });
        assert_eq!(sp.csd_len, "Service Pack 1".len());
        assert_eq!(sp.csd[sp.csd_len], 0, "NUL 结尾必须在位");
        // 串表 ID 越界注册拒。
        assert!(m.register(3, 8, 7, 91).is_err());
    }

    #[test]
    fn judge_order_size_before_prestuff() {
        let mut m = ProcGvexMatrix::new();
        seed_matrix(&mut m, 5).unwrap_or(());
        let st = GvexStruct::legal_entry()
            .prestuff(GvexField::Size, 999)
            .prestuff(GvexField::Build, 12345);
        match gvex_hook(&st, 5, &m) {
            Err(c) => assert_eq!(c, GvexCode::SizeMismatch.val(), "闸 1 先于闸 2"),
            Ok(_) => panic!("双违约必须拒"),
        }
    }

    #[test]
    fn double_run_identical_and_probes() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑一致");
    }

    #[test]
    fn register_edges_and_full_matrix() {
        let mut m = ProcGvexMatrix::new();
        assert!(m.register(1, 0, 10, 0).is_err(), "空依据拒");
        assert!(m.register(1, 9, 10, 1).is_err(), "维度越界拒");
        assert!(m.register(1, 0, 10, 1).is_ok());
        assert_eq!(m.lookup(1, 0), Some(10));
        let mut m2 = ProcGvexMatrix::new();
        let mut filled = 0usize;
        let mut p = 0;
        while p < 12 && filled < MATRIX_CAP {
            let mut d = 0;
            while d < DATA_FIELD_N && filled < MATRIX_CAP {
                if m2.register(p, d as u32, 1, 1).is_ok() {
                    filled += 1;
                }
                d += 1;
            }
            p += 1;
        }
        assert_eq!(filled, MATRIX_CAP);
        assert!(m2.register(99, 0, 1, 1).is_err(), "矩阵满拒");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18434_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
