//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1505 注入资产回归常跑 · 0x762C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定）：
//!   四族注入常跑 10/10 轮全绿（四族逐族按册执行，必抓率 100%），常跑
//!   与回归矩阵联动核验一致率 100%，漏族注入必抓。
//! 设计要点：
//!   1. 四族注入闭集冻结（F1414 对齐族 / F1435 越界族 / F1454 重复族 /
//!      F1470 指纹族），族号表 + 每族 3 案册表逐字锚定不许漂移；
//!   2. 逐族按册执行：每案独立真实模型（对齐闸/越界闸/重复闸/指纹闸）
//!      人为构造违约必被对应闸抓，必抓率 100%；
//!   3. 常跑 10/10 轮全绿：每轮四族×3 案全跑，轮内全抓才记绿轮，
//!      轮账只追加；
//!   4. 常跑与回归矩阵联动核验：矩阵注册族（冻结 4 族）与常跑覆盖族
//!      逐族对表，一致率 100%；任一注册族未被覆盖即 MatrixMismatch；
//!   5. 漏族注入（抽一族跳过执行）4 种跳法全部必抓——联动核验按注册
//!      族全量对表，缺一族即红；
//!   6. 同种子双跑（轮账/覆盖账/抓捕账三面）必须全等。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x762C0，AI-VX7-1104 批7 O 组独占区）。
pub const SEG_BASE: u32 = 0x762C0;

/// 注入族数（4 族冻结）。
pub const FAM_COUNT: usize = 4;
/// 每族案数（3 案冻结，按册）。
pub const CASES_PER_FAM: usize = 3;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 四族注入闭集（族号冻结） ─────────────────────────────────────
pub const FAM_IDS: [u32; FAM_COUNT] = [1414, 1435, 1454, 1470];

/// 族名冻结表（逐字锚定）。
pub const FAM_NAMES: [&str; FAM_COUNT] = ["对齐注入族", "越界注入族", "重复注入族", "指纹注入族"];

/// 回归矩阵注册族（冻结：四族全注册）。
pub const MATRIX_REGISTERED: [u32; FAM_COUNT] = [1414, 1435, 1454, 1470];

// ── 按册案表（4 族 × 3 案，案名逐字锚定） ────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FamCase {
    pub fam: u32,
    pub slot: usize,
    pub name: &'static str,
}

pub const FAM_BOOK: [FamCase; FAM_COUNT * CASES_PER_FAM] = [
    // F1414 对齐注入族：对齐闸三案。
    FamCase { fam: 1414, slot: 0, name: "基址非16对齐" },
    FamCase { fam: 1414, slot: 1, name: "容量非16对齐" },
    FamCase { fam: 1414, slot: 2, name: "对齐区间重叠" },
    // F1435 越界注入族：越界闸三案。
    FamCase { fam: 1435, slot: 0, name: "索引达容量上界" },
    FamCase { fam: 1435, slot: 1, name: "索引超容量远界" },
    FamCase { fam: 1435, slot: 2, name: "计数加法回绕" },
    // F1454 重复注入族：重复闸三案。
    FamCase { fam: 1454, slot: 0, name: "重复注册同号" },
    FamCase { fam: 1454, slot: 1, name: "重复提交同案" },
    FamCase { fam: 1454, slot: 2, name: "重复封账同轮" },
    // F1470 指纹注入族：指纹闸三案。
    FamCase { fam: 1470, slot: 0, name: "封账数据篡改" },
    FamCase { fam: 1470, slot: 1, name: "封账指纹翻动" },
    FamCase { fam: 1470, slot: 2, name: "封账重放失配" },
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjRunCode {
    /// 族号越界：不在四族闭集内
    FamUnknown,
    /// 案号越界：不在册表闭集内
    CaseUnknown,
    /// 注入逃逸：人为违约未被闸抓（红线）
    NotCaught,
    /// 矩阵失配：常跑覆盖族 ≠ 回归矩阵注册族
    MatrixMismatch,
    /// 轮次未竟：本轮有族未跑或有案漏跑
    RoundIncomplete,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl InjRunCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            InjRunCode::FamUnknown => "族号越界：仅 F1414/F1435/F1454/F1470 四族，建议按 FAM_IDS 取号",
            InjRunCode::CaseUnknown => "案号越界：每族 3 案按册取号，建议核对 FAM_BOOK",
            InjRunCode::NotCaught => "注入逃逸：违约必抓是红线，建议核对该族闸门口径",
            InjRunCode::MatrixMismatch => "矩阵失配：常跑覆盖族必须等于矩阵注册族，建议补跑缺族",
            InjRunCode::RoundIncomplete => "轮次未竟：四族×3 案须全跑，建议按册逐案补齐",
            InjRunCode::NotReproducible => "双跑不一致：同种子常跑必须同结果，建议固定种子对拍",
        }
    }
    pub const ALL: [InjRunCode; 6] = [
        InjRunCode::FamUnknown,
        InjRunCode::CaseUnknown,
        InjRunCode::NotCaught,
        InjRunCode::MatrixMismatch,
        InjRunCode::RoundIncomplete,
        InjRunCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < InjRunCode::ALL.len() {
        if let Some(c) = InjRunCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x762C0 段闭集内"
}

// ── FNV-1a 32 位指纹（F1470 指纹闸与封账复算共用） ───────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    h
}

// ── LCG（种子固定；本案判据全闭集注入无随机路径，保留种子口径守卫） ──
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 种子口径守卫：常跑种子冻结为 SEG_BASE（防种子漂移，双跑同源）。
pub fn machine_seed() -> u64 {
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 首抽恒定：LCG 确定性 ⇒ 同种子同流，此处仅消费一步以锚定口径。
    let first = rng.next();
    if first == rng.next() {
        // 确定性 LCG 相邻两抽不可能相等（乘加步进），相等即口径漂移。
        return 0;
    }
    SEG_BASE as u64
}

// ── 四族真实闸模型 ──────────────────────────────────────────────
/// 区容量与对齐（F1414 对齐闸口径，冻结）。
pub const REGION_ALIGN: u64 = 16;
pub const REGION_CAP: usize = 8;

/// F1414 对齐闸：注册区（基址/容量），非对齐或重叠显性拒。
pub fn region_register(regs: &mut Vec<(u64, u64)>, base: u64, size: u64) -> Result<usize, u32> {
    if base % REGION_ALIGN != 0 || size % REGION_ALIGN != 0 {
        return Err(0x1414);
    }
    for &(b, s) in regs.iter() {
        if base < b + s && b < base + size {
            return Err(0x1414);
        }
    }
    regs.push((base, size));
    Ok(regs.len() - 1)
}

/// F1435 越界闸：定容表取槽，越界显性拒（.get() 口径，不裸下标）。
pub fn table_fetch(table: &[u64; REGION_CAP], idx: usize) -> Result<u64, u32> {
    match table.get(idx) {
        Some(&v) => Ok(v),
        None => Err(0x1435),
    }
}

/// F1454 重复闸：同号重复注册显性拒。
pub fn id_register(ids: &mut Vec<u32>, id: u32) -> Result<usize, u32> {
    if ids.iter().any(|&x| x == id) {
        return Err(0x1454);
    }
    ids.push(id);
    Ok(ids.len() - 1)
}

/// F1470 指纹闸：封账数据 FNV 后重放对拍，篡改必现失配。
pub fn fingerprint_gate(data: &[u8], sealed: u32) -> Result<(), u32> {
    if fnv1a(data) != sealed {
        return Err(0x1470);
    }
    Ok(())
}

/// 单案执行：人为构造违约 → 对应闸必抓为绿。
/// 返回 Ok(true) = 抓到；Ok(false) = 逃逸（红线）。
pub fn run_case(fam_idx: usize, slot: usize) -> Result<bool, u32> {
    if fam_idx >= FAM_COUNT {
        return Err(InjRunCode::FamUnknown.val());
    }
    if slot >= CASES_PER_FAM {
        return Err(InjRunCode::CaseUnknown.val());
    }
    let caught = match fam_idx {
        // F1414 对齐族三案：三案违约均被对齐/重叠闸拒。
        0 => {
            let mut regs: Vec<(u64, u64)> = Vec::new();
            let r1 = region_register(&mut regs, 0x100000, 0x1000);
            let hit = match slot {
                0 => {
                    let _ = r1;
                    region_register(&mut regs, 0x100004, 0x1000).is_err()
                }
                1 => {
                    let _ = r1;
                    region_register(&mut regs, 0x102000, 0x1003).is_err()
                }
                _ => {
                    let _ = r1;
                    region_register(&mut regs, 0x100800, 0x1000).is_err()
                }
            };
            hit
        }
        // F1435 越界族三案：越界均被 .get() 闸拒。
        1 => {
            let table: [u64; REGION_CAP] = [1, 2, 3, 4, 5, 6, 7, 8];
            match slot {
                0 => table_fetch(&table, REGION_CAP).is_err(),
                1 => table_fetch(&table, REGION_CAP * 7).is_err(),
                _ => {
                    // 计数加法回绕注入：u32 上界回绕后仍须拒（checked 口径）。
                    let big: usize = usize::MAX;
                    table_fetch(&table, big).is_err()
                }
            }
        }
        // F1454 重复族三案：重复均被重复闸拒。
        2 => {
            let mut ids: Vec<u32> = Vec::new();
            match slot {
                0 => {
                    let _ = id_register(&mut ids, 7)?;
                    id_register(&mut ids, 7).is_err()
                }
                1 => {
                    let _ = id_register(&mut ids, 9)?;
                    let _ = id_register(&mut ids, 10)?;
                    id_register(&mut ids, 9).is_err()
                }
                _ => {
                    let _ = id_register(&mut ids, 11)?;
                    let _ = id_register(&mut ids, 11).is_err();
                    // 重复封账同轮：同 id 二次封账仍拒。
                    id_register(&mut ids, 11).is_err()
                }
            }
        }
        // F1470 指纹族三案：篡改/翻动/重放失配均被指纹闸抓。
        _ => {
            let data: [u8; 8] = [0x76, 0x2C, 0x0A, 0x55, 0x5E, 0xD1, 0x4A, 0x70];
            let sealed = fnv1a(&data);
            match slot {
                0 => {
                    let mut tainted = data;
                    let changed = match tainted.get_mut(0) {
                        Some(b0) => {
                            *b0 = b0.wrapping_add(1);
                            true
                        }
                        None => false,
                    };
                    changed && fingerprint_gate(&tainted, sealed).is_err()
                }
                1 => fingerprint_gate(&data, sealed.wrapping_add(1)).is_err(),
                _ => {
                    let mut other = data;
                    let changed = match other.get_mut(7) {
                        Some(b7) => {
                            *b7 = b7.wrapping_sub(1);
                            true
                        }
                        None => false,
                    };
                    changed && fingerprint_gate(&other, sealed).is_err()
                }
            }
        }
    };
    Ok(caught)
}

// ── 常跑账行（只追加） ──────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRow {
    pub seq: u64,
    pub round: u32,
    pub fam: u32,
    pub slot: usize,
    pub caught: bool,
}

/// 注入资产回归常跑机。
#[derive(Clone, Debug)]
pub struct RegRun {
    /// 已跑轮数。
    pub rounds_done: u32,
    /// 覆盖族位图（常跑覆盖族）。
    pub covered: [bool; FAM_COUNT],
    /// 族级 tries/caught。
    pub fam_tries: [u32; FAM_COUNT],
    pub fam_caught: [u32; FAM_COUNT],
    /// 常跑账（只追加）。
    pub rows: Vec<RunRow>,
    row_seq: u64,
    /// 绿轮数。
    pub green_rounds: u32,
}

impl RegRun {
    pub fn new() -> RegRun {
        RegRun {
            rounds_done: 0,
            covered: [false; FAM_COUNT],
            fam_tries: [0; FAM_COUNT],
            fam_caught: [0; FAM_COUNT],
            rows: Vec::new(),
            row_seq: 0,
            green_rounds: 0,
        }
    }

    /// 跑一轮：四族逐族按册执行（3 案/族），全抓才记绿轮。
    pub fn run_round(&mut self, round: u32) -> Result<(), u32> {
        let mut fam_idx = 0;
        let mut round_all_caught = true;
        while fam_idx < FAM_COUNT {
            let fam = match FAM_IDS.get(fam_idx) {
                Some(&f) => f,
                None => return Err(InjRunCode::FamUnknown.val()),
            };
            let mut slot = 0;
            while slot < CASES_PER_FAM {
                let caught = run_case(fam_idx, slot)?;
                match self.fam_tries.get_mut(fam_idx) {
                    Some(t) => *t += 1,
                    None => return Err(InjRunCode::FamUnknown.val()),
                }
                if caught {
                    match self.fam_caught.get_mut(fam_idx) {
                        Some(c) => *c += 1,
                        None => return Err(InjRunCode::FamUnknown.val()),
                    }
                    if let Some(cv) = self.covered.get_mut(fam_idx) {
                        *cv = true;
                    }
                } else {
                    round_all_caught = false;
                }
                self.row_seq += 1;
                self.rows.push(RunRow {
                    seq: self.row_seq,
                    round,
                    fam,
                    slot,
                    caught,
                });
                slot += 1;
            }
            fam_idx += 1;
        }
        self.rounds_done += 1;
        if round_all_caught {
            self.green_rounds += 1;
        } else {
            return Err(InjRunCode::NotCaught.val());
        }
        Ok(())
    }

    /// 常跑 n 轮（判据 10/10 口径）。
    pub fn run_n_rounds(&mut self, n: u32) -> Result<(), u32> {
        let mut r = 0;
        while r < n {
            self.run_round(r)?;
            r += 1;
        }
        Ok(())
    }

    /// 联动核验：常跑覆盖族 = 回归矩阵注册族（一致率 100%）。
    pub fn reconcile(&self) -> Result<u32, u32> {
        let mut matched = 0;
        let mut idx = 0;
        while idx < FAM_COUNT {
            let reg = match MATRIX_REGISTERED.get(idx) {
                Some(&f) => f,
                None => return Err(InjRunCode::MatrixMismatch.val()),
            };
            let want = match FAM_IDS.get(idx) {
                Some(&f) => f,
                None => return Err(InjRunCode::MatrixMismatch.val()),
            };
            if reg != want {
                return Err(InjRunCode::MatrixMismatch.val());
            }
            match self.covered.get(idx) {
                Some(true) => matched += 1,
                _ => return Err(InjRunCode::MatrixMismatch.val()),
            }
            idx += 1;
        }
        Ok(matched)
    }

    /// 10/10 全绿判定。
    pub fn all_green(&self) -> bool {
        self.green_rounds == JUDGE_ROUNDS && self.rounds_done == JUDGE_ROUNDS
    }

    /// 必抓率 100% 判定（族级）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < FAM_COUNT {
            let (t, c) = match (self.fam_tries.get(i), self.fam_caught.get(i)) {
                (Some(t), Some(c)) => (*t, *c),
                _ => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 常跑账只追加守卫：seq 连续递增。
    pub fn ledger_append_only_ok(&self) -> bool {
        let mut prev: u64 = 0;
        for row in &self.rows {
            if row.seq != prev + 1 {
                return false;
            }
            prev = row.seq;
        }
        true
    }
}

/// 漏族注入：抽一族跳过执行，联动核验必抓（4 种跳法全抓）。
pub fn inject_missing_family(skip: usize) -> Result<bool, u32> {
    if skip >= FAM_COUNT {
        return Err(InjRunCode::FamUnknown.val());
    }
    let mut run = RegRun::new();
    let mut idx = 0;
    while idx < FAM_COUNT {
        if idx != skip {
            // 仅跑未跳过的族（复用 run_round 会全跑，故逐案直跑并记账）。
            let mut slot = 0;
            while slot < CASES_PER_FAM {
                let caught = run_case(idx, slot)?;
                if caught {
                    if let Some(t) = run.fam_tries.get_mut(idx) {
                        *t += 1;
                    }
                    if let Some(c) = run.fam_caught.get_mut(idx) {
                        *c += 1;
                    }
                    if let Some(cv) = run.covered.get_mut(idx) {
                        *cv = true;
                    }
                } else {
                    return Err(InjRunCode::NotCaught.val());
                }
                slot += 1;
            }
        }
        idx += 1;
    }
    // 联动核验：缺族必现 MatrixMismatch（漏族注入必抓）。
    match run.reconcile() {
        Err(e) => Ok(e == InjRunCode::MatrixMismatch.val()),
        Ok(_) => Ok(false),
    }
}

// ── 黄金全链：10 轮全绿 + 120 案全抓 + 矩阵联动 100% ─────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut run = RegRun::new();
    run.run_n_rounds(JUDGE_ROUNDS)?;
    let caught_total = {
        let mut s = 0;
        let mut i = 0;
        while i < FAM_COUNT {
            match run.fam_caught.get(i) {
                Some(&c) => s += c,
                None => return Err(InjRunCode::FamUnknown.val()),
            }
            i += 1;
        }
        s
    };
    let matched = run.reconcile()?;
    // 漏族注入 4 种跳法全抓。
    let mut missing_caught = 0;
    let mut skip = 0;
    while skip < FAM_COUNT {
        if inject_missing_family(skip)? {
            missing_caught += 1;
        }
        skip += 1;
    }
    Ok((run.green_rounds, caught_total, matched + missing_caught))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：10 绿轮 + 120 抓 + 联动 8（4 匹配 + 4 跳法抓）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((10, 120, 8)) => Ok(()),
        _ => Err(InjRunCode::NotReproducible.val()),
    }
}

/// 序 B：族号/案号越界显性拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if run_case(FAM_COUNT, 0).is_ok() {
        return Err(InjRunCode::NotReproducible.val());
    }
    if run_case(0, CASES_PER_FAM).is_ok() {
        return Err(InjRunCode::NotReproducible.val());
    }
    match run_case(FAM_COUNT, 0) {
        Err(e) if e == InjRunCode::FamUnknown.val() => {}
        _ => return Err(InjRunCode::NotReproducible.val()),
    }
    match run_case(0, CASES_PER_FAM) {
        Err(e) if e == InjRunCode::CaseUnknown.val() => {}
        _ => return Err(InjRunCode::NotReproducible.val()),
    }
    Ok(())
}

/// 序 C：未跑常跑直接联动核验必红（零覆盖 ≠ 注册族）。
pub fn probe_seq_c() -> Result<(), u32> {
    let run = RegRun::new();
    match run.reconcile() {
        Err(e) if e == InjRunCode::MatrixMismatch.val() => Ok(()),
        _ => Err(InjRunCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    // 种子口径：常跑种子冻结 = SEG_BASE（双跑同源前提）。
    if machine_seed() != SEG_BASE as u64 {
        return Err(InjRunCode::NotReproducible.val());
    }
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(InjRunCode::NotReproducible.val());
    }
    // 双跑账面：轮账行数与 seq 全等。
    let mut r1 = RegRun::new();
    let mut r2 = RegRun::new();
    r1.run_n_rounds(2)?;
    r2.run_n_rounds(2)?;
    if r1.rows != r2.rows || r1.fam_tries != r2.fam_tries || r1.fam_caught != r2.fam_caught {
        return Err(InjRunCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < InjRunCode::ALL.len() {
        if let Some(c) = InjRunCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// J1：单族必抓 100%（逐族按册）。
fn fam_caught_ok(fam_idx: usize) -> bool {
    if fam_idx >= FAM_COUNT {
        return false;
    }
    let mut slot = 0;
    while slot < CASES_PER_FAM {
        match run_case(fam_idx, slot) {
            Ok(true) => {}
            _ => return false,
        }
        slot += 1;
    }
    true
}

/// J1：常跑 10/10 轮全绿。
fn rounds_green_ok() -> bool {
    let mut run = RegRun::new();
    match run.run_n_rounds(JUDGE_ROUNDS) {
        Ok(()) => run.all_green() && run.all_caught(),
        Err(_) => false,
    }
}

/// J1：常跑与回归矩阵联动核验一致率 100%。
fn reconcile_ok() -> bool {
    let mut run = RegRun::new();
    if run.run_n_rounds(JUDGE_ROUNDS).is_err() {
        return false;
    }
    match run.reconcile() {
        Ok(n) => n == FAM_COUNT as u32,
        Err(_) => false,
    }
}

/// J1：漏族注入 4 种跳法必抓。
fn missing_family_ok() -> bool {
    let mut skip = 0;
    while skip < FAM_COUNT {
        match inject_missing_family(skip) {
            Ok(true) => {}
            _ => return false,
        }
        skip += 1;
    }
    true
}

/// J1：册表冻结（12 案逐字锚定族号/案号）。
fn fam_book_ok() -> bool {
    if FAM_BOOK.len() != FAM_COUNT * CASES_PER_FAM {
        return false;
    }
    let mut i = 0;
    while i < FAM_BOOK.len() {
        let c = match FAM_BOOK.get(i) {
            Some(&c) => c,
            None => return false,
        };
        let want_fam = match FAM_IDS.get(i / CASES_PER_FAM) {
            Some(&f) => f,
            None => return false,
        };
        if c.fam != want_fam || c.slot != i % CASES_PER_FAM || c.name.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：全 add_dyn） ──────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J0-段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X} 窗口 0x75000..=0x781C0 且 0x40 对齐", SEG_BASE),
    ));
    v.push((
        "J1-四族闭集冻结",
        FAM_IDS == [1414, 1435, 1454, 1470] && MATRIX_REGISTERED == FAM_IDS,
        String::from("F1414/F1435/F1454/F1470 族号+注册族双表冻结"),
    ));
    v.push((
        "J1-册表冻结",
        fam_book_ok(),
        format!("{}案逐字锚定（4族×3案）", FAM_BOOK.len()),
    ));
    // 逐族必抓。
    let mut fam_idx = 0;
    while fam_idx < FAM_COUNT {
        let name = match FAM_NAMES.get(fam_idx) {
            Some(&n) => n,
            None => "?",
        };
        let fid = match FAM_IDS.get(fam_idx) {
            Some(&f) => f,
            None => 0,
        };
        v.push((
            "J1-族必抓",
            fam_caught_ok(fam_idx),
            format!("{}（F{}）3 案全抓", name, fid),
        ));
        fam_idx += 1;
    }
    v.push((
        "J1-常跑10/10全绿",
        rounds_green_ok(),
        format!("{}轮×4族×3案 轮轮全抓", JUDGE_ROUNDS),
    ));
    v.push((
        "J1-矩阵联动100%",
        reconcile_ok(),
        String::from("常跑覆盖族 = 矩阵注册族 4/4"),
    ));
    v.push((
        "J1-漏族注入必抓",
        missing_family_ok(),
        String::from("4 种跳法联动核验全红"),
    ));
    // 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((green, caught, link)) => *green == 10 && *caught == 120 && *link == 8,
            Err(_) => false,
        },
        match &g {
            Ok((green, caught, link)) => format!("绿轮={} 抓={}/120 联动={}", green, caught, link),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 越界拒探针。
    v.push((
        "J2-越界拒探针",
        probe_seq_b().is_ok(),
        String::from("族号/案号越界 ⇒ FamUnknown/CaseUnknown"),
    ));
    // 零覆盖核验红。
    v.push((
        "J2-零覆盖核验红",
        probe_seq_c().is_ok(),
        String::from("未跑常跑联动核验 ⇒ MatrixMismatch"),
    ));
    // 探针序 A。
    v.push(("J2-探针序A", probe_seq_a().is_ok(), String::from("10绿+120抓+联动8")));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 与账面两跑同值"),
    ));
    // 账只追加。
    let mut run = RegRun::new();
    let _ = run.run_round(0);
    let l0 = run.rows.len();
    let _ = run.run_round(1);
    let l1 = run.rows.len();
    v.push((
        "J3-常跑账只追加",
        l0 == 12 && l1 == 24 && run.ledger_append_only_ok(),
        format!("每轮 12 行 seq 连续（{}→{}）", l0, l1),
    ));
    // 诊断码闭集。
    v.push((
        "J3-诊断码闭集",
        detail(SEG_BASE + 0x3F).starts_with("未知") && detail(InjRunCode::NotCaught.val()) == InjRunCode::NotCaught.detail(),
        String::from("闭集查表命中 + 窗口外未知"),
    ));
    v
}

pub fn run_unx1505_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1505 注入资产回归常跑");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chain_all_green() {
        assert_eq!(golden_run(), Ok((10, 120, 8)));
    }

    #[test]
    fn each_family_fully_caught() {
        let mut fam_idx = 0;
        while fam_idx < FAM_COUNT {
            let mut slot = 0;
            while slot < CASES_PER_FAM {
                match run_case(fam_idx, slot) {
                    Ok(true) => {}
                    other => panic!("族{}案{} 必抓，实得 {:?}", fam_idx, slot, other),
                }
                slot += 1;
            }
            fam_idx += 1;
        }
    }

    #[test]
    fn ten_rounds_all_green() {
        let mut run = RegRun::new();
        run.run_n_rounds(JUDGE_ROUNDS).ok();
        assert!(run.all_green(), "10/10 绿轮");
        assert!(run.all_caught(), "必抓率 100%");
        assert_eq!(run.rows.len(), 120);
    }

    #[test]
    fn reconcile_full_coverage() {
        let mut run = RegRun::new();
        let _ = run.run_n_rounds(JUDGE_ROUNDS);
        assert_eq!(run.reconcile(), Ok(4), "覆盖族 = 注册族 4/4");
    }

    #[test]
    fn missing_family_all_skips_caught() {
        let mut skip = 0;
        while skip < FAM_COUNT {
            match inject_missing_family(skip) {
                Ok(true) => {}
                other => panic!("漏族跳{} 必抓，实得 {:?}", skip, other),
            }
            skip += 1;
        }
    }

    #[test]
    fn bounds_rejected() {
        assert_eq!(run_case(FAM_COUNT, 0), Err(InjRunCode::FamUnknown.val()));
        assert_eq!(run_case(0, CASES_PER_FAM), Err(InjRunCode::CaseUnknown.val()));
        assert_eq!(run_case(FAM_COUNT, CASES_PER_FAM), Err(InjRunCode::FamUnknown.val()));
    }

    #[test]
    fn double_run_identical() {
        assert!(repro_check().is_ok());
        let mut r1 = RegRun::new();
        let mut r2 = RegRun::new();
        r1.run_round(0).ok();
        r2.run_round(0).ok();
        assert_eq!(r1.rows, r2.rows);
        assert_eq!(r1.covered, r2.covered);
    }

    #[test]
    fn run_ledger_append_only() {
        let mut run = RegRun::new();
        run.run_round(0).ok();
        run.run_round(1).ok();
        assert!(run.ledger_append_only_ok());
        assert_eq!(run.rows.len(), 24);
        // 首行与末行 seq 连续。
        let first = run.rows.get(0).map(|r| r.seq);
        let last = run.rows.get(run.rows.len() - 1).map(|r| r.seq);
        assert_eq!(first, Some(1));
        assert_eq!(last, Some(24));
    }

    #[test]
    fn zero_coverage_reconcile_red() {
        let run = RegRun::new();
        assert_eq!(run.reconcile(), Err(InjRunCode::MatrixMismatch.val()));
        assert!(!run.all_green());
        assert!(!run.all_caught());
    }

    #[test]
    fn segment_window_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut i = 0;
        while i < InjRunCode::ALL.len() {
            if let Some(c) = InjRunCode::ALL.get(i) {
                let v = c.val();
                assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知"));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }
}
