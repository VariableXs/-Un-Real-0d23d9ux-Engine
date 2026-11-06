//! UNX-F9290 零参数 execve 边界矩阵 · 0x7B600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9290 逐字）：
//!   UNX-F9290-J1 空 argv 正判据（argv[0] 可空）判据过，空 envp 正判据
//!   格过，坏指针优先判序格过。
//!
//! 设计要点：
//!   ① 指针三态是核心：Null（指针本身为空）、Empty（指针有效但首元素为
//!      NULL）、Normal（有效链表）、Fault（指针不可读）。内核 count() 对
//!      argv == NULL 直接返回 0，因此 Null 与 Empty 在参数语义上都等价于
//!      「零参数」，但它们在指针校验面上必须分开取证，防止把空指针误判成
//!      坏指针。
//!   ② 空 argv 是正判据而不是错误：argc = 0 时 execve 仍应成功装载程序；
//!      空 envp 同理。argv[0] 为空串（""）时 argc = 1，空串是一个真实参数，
//!      不能与「首元素 NULL」混为一谈。
//!   ③ 判序冻结为：路径指针 → argv 指针 → envp 指针 → 打开权限 → 格式嗅探
//!      → 提交。两处坏指针同时出现时，argv 的 EFAULT 必须先于 envp；路径坏
//!      指针必须先于一切。zone 字段记录停在哪一站，probes 记录走了几站，
//!      两者与 zone.index 严格对应。
//!   ④ 4×4 指针类别网格（argv × envp）共 16 格：只有 Fault 参与判定，任何
//!      含 Fault 的格回 EFAULT，其余格全部成功；成功格的 argc 由 argv 形态
//!      决定（Null/Empty 为 0，Normal 为物理条目数）。
//!   ⑤ 注入 16 格 × 8 轮 = 128 次；每轮校验码、zone、probes 与 argc 四元组，
//!      账本 injected == detected + escaped 且 escaped == 0。
//!   ⑥ 诊断码闭集 12 个，段窗口 0x7B600..=0x7B63F。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9290 独占 0x7B600..=0x7B63F）。
pub const SEG_BASE: u32 = 0x7B600;
/// 段窗口上界（含）。
pub const SEG_END: u32 = 0x7B63F;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ENOEXEC: i32 = 8;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;

/// 网格边长（argv 4 类 × envp 4 类）。
pub const GRID_SIDE: usize = 4;
/// 网格总格数。
pub const GRID_CELLS: usize = 16;
/// 每格注入轮数。
pub const ROUNDS: u32 = 8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码。码值 = SEG_BASE + 序号（1 起），落 0x7B601..=0x7B60C。
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameCode {
    PathFault = 1,
    ArgvFault = 2,
    EnvpFault = 3,
    PathNull = 4,
    EmptyArgvRejected = 5,
    EmptyEnvpRejected = 6,
    ArgcDrift = 7,
    OrderDrift = 8,
    GridDrift = 9,
    LedgerLeak = 10,
    DoubleRun = 11,
    SegmentSkew = 12,
}

impl FrameCode {
    /// 段内码值。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            FrameCode::PathFault => "路径指针不可读：最先判，回 EFAULT；请检查用户态路径地址",
            FrameCode::ArgvFault => "argv 指针不可读：计数阶段回 EFAULT；空指针与坏指针必须区分",
            FrameCode::EnvpFault => "envp 指针不可读：argv 之后判，回 EFAULT；请校验环境块地址",
            FrameCode::PathNull => "路径为空指针：execve 的 path 不允许为 NULL，回 EFAULT",
            FrameCode::EmptyArgvRejected => "空 argv 被误拒：NULL/首元素 NULL 都是合法的零参数形态",
            FrameCode::EmptyEnvpRejected => "空 envp 被误拒：envp 允许为空，子进程得到空环境",
            FrameCode::ArgcDrift => "argc 漂移：空串 argv[0] 必须计 1，首元素 NULL 必须计 0",
            FrameCode::OrderDrift => "判序漂移：路径 → argv → envp 的指针判序被对调",
            FrameCode::GridDrift => "网格漂移：16 格实测与冻结表不符；请核对 Fault 行列",
            FrameCode::LedgerLeak => "注入账不守恒：detected+escaped 与 injected 不等",
            FrameCode::DoubleRun => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            FrameCode::SegmentSkew => "段位越界：段基址必须 0x40 对齐且落在本卡窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [FrameCode; 12] = [
        FrameCode::PathFault,
        FrameCode::ArgvFault,
        FrameCode::EnvpFault,
        FrameCode::PathNull,
        FrameCode::EmptyArgvRejected,
        FrameCode::EmptyEnvpRejected,
        FrameCode::ArgcDrift,
        FrameCode::OrderDrift,
        FrameCode::GridDrift,
        FrameCode::LedgerLeak,
        FrameCode::DoubleRun,
        FrameCode::SegmentSkew,
    ];
}

// ── 指针三态与阶段 ────────────────────────────────────────────────────
/// 用户指针类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 指针本身为 NULL（合法语义：空向量）。
    Null,
    /// 指针有效，但首元素就是 NULL（合法语义：空向量）。
    Empty,
    /// 指针有效且至少有一个条目。
    Normal,
    /// 指针不可读（越界/非规范地址）。
    Fault,
}

impl PtrClass {
    /// 冻结编码（指纹使用）。
    pub fn tag(self) -> u8 {
        match self {
            PtrClass::Null => 0,
            PtrClass::Empty => 1,
            PtrClass::Normal => 2,
            PtrClass::Fault => 3,
        }
    }

    /// 是否坏指针。
    pub fn is_fault(self) -> bool {
        matches!(self, PtrClass::Fault)
    }
}

/// 执行链站点（与 probes 一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Path,
    Argv,
    Envp,
    Open,
    Format,
    Commit,
}

impl Zone {
    /// 冻结序号：probes = index + 1。
    pub fn index(self) -> u8 {
        match self {
            Zone::Path => 0,
            Zone::Argv => 1,
            Zone::Envp => 2,
            Zone::Open => 3,
            Zone::Format => 4,
            Zone::Commit => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Zone::Path => "path",
            Zone::Argv => "argv",
            Zone::Envp => "envp",
            Zone::Open => "open",
            Zone::Format => "format",
            Zone::Commit => "commit",
        }
    }
}

/// 一次判定的完整结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecOutcome {
    pub code: i32,
    pub zone: Zone,
    pub argc: u32,
    pub envc: u32,
    pub probes: u8,
}

impl ExecOutcome {
    pub fn launched(&self) -> bool {
        self.code == 0 && self.zone == Zone::Commit
    }
}

fn outcome(code: i32, zone: Zone, argc: u32, envc: u32) -> ExecOutcome {
    ExecOutcome { code, zone, argc, envc, probes: zone.index() + 1 }
}

// ── 帧模型 ────────────────────────────────────────────────────────────
/// 一次 execve 的用户态输入帧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub path: PtrClass,
    pub argv: PtrClass,
    pub envp: PtrClass,
    /// argv 有效时的物理条目数（Normal 用；Null/Empty 忽略）。
    pub argv_entries: u32,
    /// envp 有效时的物理条目数。
    pub env_entries: u32,
    /// argv[0] 是否为空串（真实参数，计 1 条）。
    pub argv0_empty_string: bool,
    /// 目标文件是否具备执行权限。
    pub file_exec: bool,
    /// 目标是否 ELF 或合法脚本。
    pub is_exec_format: bool,
}

impl Frame {
    /// 干净帧：ELF、可执行、空 argv/envp。
    pub const fn clean() -> Frame {
        Frame {
            path: PtrClass::Normal,
            argv: PtrClass::Null,
            envp: PtrClass::Null,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: true,
            is_exec_format: true,
        }
    }
}

/// argc 语义：Null/Empty 为 0；Normal 为物理条目数（空串已计入条目）。
pub fn argc_of(f: &Frame) -> u32 {
    match f.argv {
        PtrClass::Null | PtrClass::Empty | PtrClass::Fault => 0,
        PtrClass::Normal => f.argv_entries,
    }
}

/// envc 语义同上。
pub fn envc_of(f: &Frame) -> u32 {
    match f.envp {
        PtrClass::Null | PtrClass::Empty | PtrClass::Fault => 0,
        PtrClass::Normal => f.env_entries,
    }
}

/// 执行判定：指针判序在前，权限/格式在后。
pub fn judge(f: &Frame) -> ExecOutcome {
    // ① 路径指针：NULL 与 Fault 都是 EFAULT，且先于 argv。
    if f.path == PtrClass::Fault || f.path == PtrClass::Null || f.path == PtrClass::Empty {
        return outcome(EFAULT, Zone::Path, 0, 0);
    }
    // ② argv 指针：只有 Fault 被拒；Null/Empty 是合法空向量。
    if f.argv.is_fault() {
        return outcome(EFAULT, Zone::Argv, 0, 0);
    }
    // ③ envp 指针：argv 之后判。
    if f.envp.is_fault() {
        return outcome(EFAULT, Zone::Envp, 0, 0);
    }
    // ④ 打开权限。
    if !f.file_exec {
        return outcome(EACCES, Zone::Open, 0, 0);
    }
    // ⑤ 格式嗅探。
    if !f.is_exec_format {
        return outcome(ENOEXEC, Zone::Format, 0, 0);
    }
    // ⑥ 提交：argc/envc 按三态语义结算。
    outcome(0, Zone::Commit, argc_of(f), envc_of(f))
}

// ── 4×4 网格 ──────────────────────────────────────────────────────────
/// 一格：argv 类别 + envp 类别 + 期望码 + 期望 argc。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridCell {
    pub argv: PtrClass,
    pub envp: PtrClass,
    pub expect: i32,
    pub argc: u32,
}

/// 行顺序：Null / Empty / Normal / Fault。
pub const ROW_CLASSES: [PtrClass; GRID_SIDE] = [
    PtrClass::Null,
    PtrClass::Empty,
    PtrClass::Normal,
    PtrClass::Fault,
];

/// 列顺序：Null / Empty / Normal / Fault。
pub const COL_CLASSES: [PtrClass; GRID_SIDE] = [
    PtrClass::Null,
    PtrClass::Empty,
    PtrClass::Normal,
    PtrClass::Fault,
];

/// 16 格冻结网格。Normal 行 argv_entries=3，Normal 列 env_entries=2。
pub const GRID: [[GridCell; GRID_SIDE]; GRID_SIDE] = [
    [
        GridCell { argv: PtrClass::Null, envp: PtrClass::Null, expect: 0, argc: 0 },
        GridCell { argv: PtrClass::Null, envp: PtrClass::Empty, expect: 0, argc: 0 },
        GridCell { argv: PtrClass::Null, envp: PtrClass::Normal, expect: 0, argc: 0 },
        GridCell { argv: PtrClass::Null, envp: PtrClass::Fault, expect: EFAULT, argc: 0 },
    ],
    [
        GridCell { argv: PtrClass::Empty, envp: PtrClass::Null, expect: 0, argc: 0 },
        GridCell { argv: PtrClass::Empty, envp: PtrClass::Empty, expect: 0, argc: 0 },
        GridCell { argv: PtrClass::Empty, envp: PtrClass::Normal, expect: 0, argc: 0 },
        GridCell { argv: PtrClass::Empty, envp: PtrClass::Fault, expect: EFAULT, argc: 0 },
    ],
    [
        GridCell { argv: PtrClass::Normal, envp: PtrClass::Null, expect: 0, argc: 3 },
        GridCell { argv: PtrClass::Normal, envp: PtrClass::Empty, expect: 0, argc: 3 },
        GridCell { argv: PtrClass::Normal, envp: PtrClass::Normal, expect: 0, argc: 3 },
        GridCell { argv: PtrClass::Normal, envp: PtrClass::Fault, expect: EFAULT, argc: 0 },
    ],
    [
        GridCell { argv: PtrClass::Fault, envp: PtrClass::Null, expect: EFAULT, argc: 0 },
        GridCell { argv: PtrClass::Fault, envp: PtrClass::Empty, expect: EFAULT, argc: 0 },
        GridCell { argv: PtrClass::Fault, envp: PtrClass::Normal, expect: EFAULT, argc: 0 },
        GridCell { argv: PtrClass::Fault, envp: PtrClass::Fault, expect: EFAULT, argc: 0 },
    ],
];

/// 由格构造帧（路径/权限/格式均为正常值）。
pub fn frame_of(cell: &GridCell) -> Frame {
    Frame {
        path: PtrClass::Normal,
        argv: cell.argv,
        envp: cell.envp,
        argv_entries: if cell.argv == PtrClass::Normal { 3 } else { 0 },
        env_entries: if cell.envp == PtrClass::Normal { 2 } else { 0 },
        argv0_empty_string: false,
        file_exec: true,
        is_exec_format: true,
    }
}

/// 取格（越界返回 None，禁止裸索引）。
pub fn cell_at(row: usize, col: usize) -> Option<GridCell> {
    match GRID.get(row) {
        Some(line) => line.get(col).copied(),
        None => None,
    }
}

// ── 判序哨兵 ──────────────────────────────────────────────────────────
/// 一组判序哨兵：同时叠加多种故障，验证停在最靠前的站点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderCase {
    pub label: &'static str,
    pub frame: Frame,
    pub code: i32,
    pub zone: Zone,
}

/// 8 个判序哨兵。
pub const ORDER_CASES: [OrderCase; 8] = [
    OrderCase {
        label: "path-vs-all",
        frame: Frame {
            path: PtrClass::Fault,
            argv: PtrClass::Fault,
            envp: PtrClass::Fault,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: false,
            is_exec_format: false,
        },
        code: EFAULT,
        zone: Zone::Path,
    },
    OrderCase {
        label: "argv-vs-envp",
        frame: Frame {
            path: PtrClass::Normal,
            argv: PtrClass::Fault,
            envp: PtrClass::Fault,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: false,
            is_exec_format: false,
        },
        code: EFAULT,
        zone: Zone::Argv,
    },
    OrderCase {
        label: "envp-vs-open",
        frame: Frame {
            path: PtrClass::Normal,
            argv: PtrClass::Null,
            envp: PtrClass::Fault,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: false,
            is_exec_format: false,
        },
        code: EFAULT,
        zone: Zone::Envp,
    },
    OrderCase {
        label: "open-vs-format",
        frame: Frame {
            path: PtrClass::Normal,
            argv: PtrClass::Null,
            envp: PtrClass::Null,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: false,
            is_exec_format: false,
        },
        code: EACCES,
        zone: Zone::Open,
    },
    OrderCase {
        label: "format-only",
        frame: Frame {
            path: PtrClass::Normal,
            argv: PtrClass::Null,
            envp: PtrClass::Null,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: true,
            is_exec_format: false,
        },
        code: ENOEXEC,
        zone: Zone::Format,
    },
    OrderCase {
        label: "clean",
        frame: Frame::clean(),
        code: 0,
        zone: Zone::Commit,
    },
    OrderCase {
        label: "path-null",
        frame: Frame {
            path: PtrClass::Null,
            argv: PtrClass::Null,
            envp: PtrClass::Null,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: true,
            is_exec_format: true,
        },
        code: EFAULT,
        zone: Zone::Path,
    },
    OrderCase {
        label: "envp-vs-format",
        frame: Frame {
            path: PtrClass::Normal,
            argv: PtrClass::Empty,
            envp: PtrClass::Fault,
            argv_entries: 0,
            env_entries: 0,
            argv0_empty_string: false,
            file_exec: true,
            is_exec_format: false,
        },
        code: EFAULT,
        zone: Zone::Envp,
    },
];

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EPERM);
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 网格注入结果。
#[derive(Clone, Copy, Debug)]
pub struct GridRun {
    pub observed: [i32; GRID_CELLS],
    pub ledger: InjectLedger,
    pub fault_cells: u32,
    pub empty_ok: u32,
}

impl GridRun {
    pub const fn empty() -> GridRun {
        GridRun { observed: [0i32; GRID_CELLS], ledger: InjectLedger::zero(), fault_cells: 0, empty_ok: 0 }
    }
}

/// 16 格 × 8 轮注入：码 + zone + probes + argc 四元组同时校验。
pub fn run_grid() -> GridRun {
    let mut run = GridRun::empty();
    let mut row = 0usize;
    while row < GRID_SIDE {
        let mut col = 0usize;
        while col < GRID_SIDE {
            if let Some(cell) = cell_at(row, col) {
                let index = row.saturating_mul(GRID_SIDE).saturating_add(col);
                let f = frame_of(&cell);
                let mut round = 0u32;
                while round < ROUNDS {
                    let out = judge(&f);
                    let zone_ok = out.zone.index() + 1 == out.probes;
                    let argc_ok = if cell.expect == 0 { out.argc == cell.argc } else { out.argc == 0 };
                    let caught = out.code == cell.expect && zone_ok && argc_ok;
                    let rec = run.ledger.record(caught);
                    if rec.is_err() {
                        run.ledger.escaped = run.ledger.escaped.saturating_add(1);
                    }
                    if round == 0 {
                        if let Some(slot) = run.observed.get_mut(index) {
                            *slot = out.code;
                        }
                        if cell.expect == EFAULT {
                            run.fault_cells = run.fault_cells.saturating_add(1);
                        }
                        let empty_pair = (cell.argv == PtrClass::Null || cell.argv == PtrClass::Empty)
                            && (cell.envp == PtrClass::Null || cell.envp == PtrClass::Empty);
                        if empty_pair && out.launched() {
                            run.empty_ok = run.empty_ok.saturating_add(1);
                        }
                    }
                    round += 1;
                }
            }
            col += 1;
        }
        row += 1;
    }
    run
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料：16 格结果 + argc 语义哨兵。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut row = 0usize;
    while row < GRID_SIDE {
        let mut col = 0usize;
        while col < GRID_SIDE {
            if let Some(cell) = cell_at(row, col) {
                let out = judge(&frame_of(&cell));
                bytes.push(if out.code == 0 { 0 } else { (out.code & 0x7F) as u8 });
                bytes.push(out.zone.index());
                bytes.push(cell.argv.tag());
                bytes.push(cell.envp.tag());
                bytes.push((out.argc & 0xFF) as u8);
            }
            col += 1;
        }
        row += 1;
    }
    // argv[0] 空串：Normal + 1 条，argc 必须为 1。
    let mut one_empty = Frame::clean();
    one_empty.argv = PtrClass::Normal;
    one_empty.argv_entries = 1;
    one_empty.argv0_empty_string = true;
    let out_one = judge(&one_empty);
    bytes.push((out_one.argc & 0xFF) as u8);
    // 空 argv + 空 envp：argc/envc 都必须为 0。
    let out_empty = judge(&Frame::clean());
    bytes.push((out_empty.argc & 0xFF) as u8);
    bytes.push((out_empty.envc & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9290 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9290_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9290");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7B600
        && SEG_END == SEG_BASE + 0x3F;
    s.add("段位守卫：0x40 对齐且窗口 0x7B600..=0x7B63F", seg_ok,
          "段基址唯一且不与邻卡重叠");

    let mut code_ok = true;
    let mut i = 0usize;
    while i < FrameCode::ALL.len() {
        if let Some(a) = FrameCode::ALL.get(i) {
            if a.detail().is_empty() {
                code_ok = false;
            }
            let mut j = i + 1;
            while j < FrameCode::ALL.len() {
                if let Some(b) = FrameCode::ALL.get(j) {
                    if a.code() == b.code() {
                        code_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("诊断码：12 个码值与文案闭集自洽", code_ok,
          "段内码值两两唯一且每条可操作");

    let run = run_grid();

    let mut grid_ok = true;
    let mut row = 0usize;
    while row < GRID_SIDE {
        let mut col = 0usize;
        while col < GRID_SIDE {
            let index = row.saturating_mul(GRID_SIDE).saturating_add(col);
            match (cell_at(row, col), run.observed.get(index)) {
                (Some(cell), Some(got)) => {
                    if *got != cell.expect {
                        grid_ok = false;
                    }
                }
                _ => grid_ok = false,
            }
            col += 1;
        }
        row += 1;
    }
    s.add("网格：16 格实测码与冻结表逐格一致", grid_ok,
          "Fault 行列回 EFAULT，其余格成功");

    s.add("注入：128 次全检出且逃逸为 0",
          run.ledger.conserved() && run.ledger.escaped == 0
              && run.ledger.injected == GRID_CELLS as u32 * ROUNDS,
          "码 + zone + probes + argc 四元组同时成立才算检出");

    s.add("Fault 格：含坏指针的 7 格全 EFAULT",
          run.fault_cells == 7,
          "Fault 行 4 格 + Fault 列 3 格，去重后恰为 7 格");

    s.add("空参数正判据：Null/Empty 组合 4 格全部装载成功",
          run.empty_ok == 4,
          "空 argv 与空 envp 是合法形态，不得被误拒");

    let empty = judge(&Frame::clean());
    s.add("空 argv + 空 envp：成功且 argc=0 / envc=0",
          empty.launched() && empty.argc == 0 && empty.envc == 0
              && empty.probes == 6,
          "零参数是正判据，提交站必须走满 6 站");

    let mut one_empty = Frame::clean();
    one_empty.argv = PtrClass::Normal;
    one_empty.argv_entries = 1;
    one_empty.argv0_empty_string = true;
    let one = judge(&one_empty);
    s.add("argv[0] 空串：计 1 条参数且装载成功",
          one.launched() && one.argc == 1,
          "空串是真实参数，不能与首元素 NULL 混同");

    let mut normal = Frame::clean();
    normal.argv = PtrClass::Normal;
    normal.argv_entries = 4;
    normal.envp = PtrClass::Normal;
    normal.env_entries = 2;
    let n = judge(&normal);
    s.add("常规参数：argc=4 / envc=2 如实结算",
          n.launched() && n.argc == 4 && n.envc == 2,
          "物理条目数必须原样回填，不得截断");

    let mut path_fault = Frame::clean();
    path_fault.path = PtrClass::Fault;
    path_fault.argv = PtrClass::Fault;
    path_fault.envp = PtrClass::Fault;
    path_fault.file_exec = false;
    path_fault.is_exec_format = false;
    let pf = judge(&path_fault);
    s.add("判序一：路径坏指针先于 argv/envp/权限/格式",
          pf.code == EFAULT && pf.zone == Zone::Path && pf.probes == 1,
          "路径指针在第一站即裁决，后续故障不得抢先");

    let mut argv_fault = Frame::clean();
    argv_fault.argv = PtrClass::Fault;
    argv_fault.envp = PtrClass::Fault;
    argv_fault.file_exec = false;
    argv_fault.is_exec_format = false;
    let af = judge(&argv_fault);
    s.add("判序二：argv 坏指针先于 envp 与打开阶段",
          af.code == EFAULT && af.zone == Zone::Argv && af.probes == 2,
          "argv 计数阶段在 envp 之前，且都早于打开");

    let mut env_fault = Frame::clean();
    env_fault.envp = PtrClass::Fault;
    env_fault.file_exec = false;
    env_fault.is_exec_format = false;
    let ef = judge(&env_fault);
    s.add("判序三：envp 坏指针先于权限与格式",
          ef.code == EFAULT && ef.zone == Zone::Envp && ef.probes == 3,
          "envp 指针校验完成后才允许看文件属性");

    let mut order_ok = true;
    let mut k = 0usize;
    while k < ORDER_CASES.len() {
        if let Some(case) = ORDER_CASES.get(k).copied() {
            let out = judge(&case.frame);
            if out.code != case.code || out.zone != case.zone {
                order_ok = false;
            }
        }
        k += 1;
    }
    s.add("判序哨兵：8 组叠加故障与冻结站点一致", order_ok,
          "哨兵覆盖坏路径/坏 argv/坏 envp/权限/格式/干净六种停站");

    let mut probes_ok = true;
    let mut r = 0usize;
    while r < GRID_SIDE {
        let mut c = 0usize;
        while c < GRID_SIDE {
            if let Some(cell) = cell_at(r, c) {
                let out = judge(&frame_of(&cell));
                if out.probes != out.zone.index() + 1 {
                    probes_ok = false;
                }
            }
            c += 1;
        }
        r += 1;
    }
    s.add("站点自洽：probes 恒等于 zone.index()+1", probes_ok,
          "轨迹长度与停站必须一一对应，否则判序不可复算");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：16 格 + argc 哨兵指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等");

    let grid_text = format!(
        "格{} / Fault {} / 空参成功 {} / 逃逸 {}",
        GRID_CELLS, run.fault_cells, run.empty_ok, run.ledger.escaped
    );
    s.add_dyn("网格快照", grid_ok && run.fault_cells == 7, &grid_text);

    let zero_text = format!("空参 argc={} envc={} probes={}", empty.argc, empty.envc, empty.probes);
    s.add_dyn("空参快照", empty.launched() && empty.argc == 0, &zero_text);

    let fp_text = format!("帧指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_argv_and_envp_launch() {
        let out = judge(&Frame::clean());
        assert_eq!(out.launched(), true);
        assert_eq!(out.argc, 0);
        assert_eq!(out.envc, 0);
        assert_eq!(out.probes, 6);
    }

    #[test]
    fn null_and_empty_vectors_equivalent() {
        let mut a = Frame::clean();
        a.argv = PtrClass::Empty;
        let mut b = Frame::clean();
        b.argv = PtrClass::Null;
        assert_eq!(judge(&a).argc, 0);
        assert_eq!(judge(&b).argc, 0);
        assert_eq!(judge(&a).launched() && judge(&b).launched(), true);
    }

    #[test]
    fn empty_string_is_one_arg() {
        let mut f = Frame::clean();
        f.argv = PtrClass::Normal;
        f.argv_entries = 1;
        f.argv0_empty_string = true;
        let out = judge(&f);
        assert_eq!(out.argc, 1);
        assert_eq!(out.launched(), true);
    }

    #[test]
    fn bad_argv_pointer_is_efault() {
        let mut f = Frame::clean();
        f.argv = PtrClass::Fault;
        let out = judge(&f);
        assert_eq!(out.code, EFAULT);
        assert_eq!(out.zone, Zone::Argv);
    }

    #[test]
    fn path_fault_wins_all() {
        let mut f = Frame::clean();
        f.path = PtrClass::Fault;
        f.argv = PtrClass::Fault;
        f.envp = PtrClass::Fault;
        f.file_exec = false;
        let out = judge(&f);
        assert_eq!(out.code, EFAULT);
        assert_eq!(out.zone, Zone::Path);
        assert_eq!(out.probes, 1);
    }

    #[test]
    fn argv_fault_precedes_envp() {
        let mut f = Frame::clean();
        f.argv = PtrClass::Fault;
        f.envp = PtrClass::Fault;
        f.is_exec_format = false;
        let out = judge(&f);
        assert_eq!(out.zone, Zone::Argv);
    }

    #[test]
    fn envp_fault_precedes_open() {
        let mut f = Frame::clean();
        f.envp = PtrClass::Fault;
        f.file_exec = false;
        f.is_exec_format = false;
        let out = judge(&f);
        assert_eq!(out.zone, Zone::Envp);
        assert_eq!(out.code, EFAULT);
    }

    #[test]
    fn grid_matches_frozen() {
        for row in 0..GRID_SIDE {
            for col in 0..GRID_SIDE {
                if let Some(cell) = cell_at(row, col) {
                    let out = judge(&frame_of(&cell));
                    assert_eq!(out.code, cell.expect);
                    assert_eq!(out.probes, out.zone.index() + 1);
                }
            }
        }
    }

    #[test]
    fn injected_grid_conserved() {
        let run = run_grid();
        assert_eq!(run.ledger.injected, 128);
        assert_eq!(run.ledger.escaped, 0);
        assert_eq!(run.fault_cells, 7);
        assert_eq!(run.empty_ok, 4);
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9290_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
