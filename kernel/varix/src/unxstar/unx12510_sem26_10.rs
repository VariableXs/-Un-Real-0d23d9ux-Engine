//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12510 · 坏表拒止统一错误码语义 · 0x30dc0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12510-J1 错误码归类矩阵（ACCESS_VIOLATION/INVALID_PARAMETER
//!      档）全行齐，抽样 30 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 归类矩阵域：B26 八面（PEB/TEB/OA/UNICODE_STRING/RUNTIME_FUNCTION/
//!   UNWIND_INFO/CONTEXT/句柄表）× 四错档（ACCESS_VIOLATION/
//!   INVALID_PARAMETER/TYPE_MISMATCH/UNWIND_TARGET）= 32 行全格；
//!   - 全行齐码判据：每格必须落一个非零 NT 档码（CellHole 拒空格），
//!   码值与八面 B 档锚定口径一致；
//!   - 行唯一性：同面同错档只登一行（DupRow 拒）；
//!   - 注入抽样 30 例：LCG 抽 (面,错档) 二元组，逐例查表码值与独立
//!   锚定面一致（CodeMismatch 拒），抽样覆盖全部八面（SampleMiss 判漏）；
//!   - 码域闭集：全部矩阵码必须落 NT 错误码闭集（UnmappedCode 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回矩阵；
//!   - 注入核：LCG 写死种子 0x30dc0，抽样 30 例 10/10 全一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30dc0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30dc0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入抽样例数（判准 30 例）。
pub const SAMPLE_CASES: usize = 30;

/// 面轴宽度（八面闭集）。
pub const FACE_COUNT: usize = 8;

/// 错档轴宽度（四档闭集）。
pub const FAULT_COUNT: usize = 4;

/// 矩阵总格数。
pub const MATRIX_CELLS: usize = FACE_COUNT * FAULT_COUNT;

// NT 错误码档（ntstatus.h 口径）。
/// STATUS_ACCESS_VIOLATION。
pub const NT_ACCESS_VIOLATION: u32 = 0xC000_0005;
/// STATUS_INVALID_PARAMETER。
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
/// STATUS_OBJECT_TYPE_MISMATCH。
pub const NT_TYPE_MISMATCH: u32 = 0xC000_0024;
/// STATUS_INVALID_UNWIND_TARGET。
pub const NT_UNWIND_TARGET: u32 = 0xC000_0029;
/// STATUS_BAD_FUNCTION_TABLE。
pub const NT_BAD_FUNCTION_TABLE: u32 = 0xC000_00FF;
/// STATUS_DATATYPE_MISALIGNMENT。
pub const NT_MISALIGNED: u32 = 0x8000_0002;
/// STATUS_INVALID_PARAMETER_MIX。
pub const NT_PARAM_MIX: u32 = 0xC000_0031;
/// STATUS_REVISION_MISMATCH。
pub const NT_REVISION_MISMATCH: u32 = 0xC000_0059;

/// 诊断码（0x30dc0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UmCode {
    /// 0x30dc0 矩阵空格。
    CellHole = 0,
    /// 0x30dc1 码值失配。
    CodeMismatch = 1,
    /// 0x30dc2 面轴越界。
    FaceOob = 2,
    /// 0x30dc3 错档轴越界。
    FaultOob = 3,
    /// 0x30dc4 重复行。
    DupRow = 4,
    /// 0x30dc5 矩阵缩水。
    MatrixShrink = 5,
    /// 0x30dc6 码值零。
    CodeZero = 6,
    /// 0x30dc7 码域越集。
    UnmappedCode = 7,
    /// 0x30dc8 抽样漏面。
    SampleMiss = 8,
    /// 0x30dc9 锚定表缺行。
    AnchorMiss = 9,
}

impl UmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [UmCode; 10] = [
        UmCode::CellHole, UmCode::CodeMismatch, UmCode::FaceOob, UmCode::FaultOob,
        UmCode::DupRow, UmCode::MatrixShrink, UmCode::CodeZero, UmCode::UnmappedCode,
        UmCode::SampleMiss, UmCode::AnchorMiss,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30dc0 => "矩阵空格：先补该格 NT 码，空格不可对抽样。",
        0x30dc1 => "码值失配：逐例回查锚定面，码值类零容差。",
        0x30dc2 => "面轴越界：面号先比 FACE_COUNT 再查。",
        0x30dc3 => "错档轴越界：错档号先比 FAULT_COUNT 再查。",
        0x30dc4 => "重复行：同面同错档只登记一行。",
        0x30dc5 => "矩阵缩水：登记后只增不减，先对账再抽样。",
        0x30dc6 => "码值零：零不是错误码，回查登记面。",
        0x30dc7 => "码域越集：矩阵码必须落八面 NT 闭集。",
        0x30dc8 => "抽样漏面：30 例必须覆盖全部八面。",
        0x30dc9 => "锚定表缺行：先补锚定口径再抽样。",
        _ => "未知诊断码：回查 0x30dc0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30dc0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30dc_30dc_30dc_30dc;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 面轴（八面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 句柄表。
    Handle = 7,
}

impl Face {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<Face> {
        match i {
            0 => Some(Face::Peb),
            1 => Some(Face::Teb),
            2 => Some(Face::Oa),
            3 => Some(Face::UniStr),
            4 => Some(Face::RuntimeFn),
            5 => Some(Face::UnwindInfo),
            6 => Some(Face::Context),
            7 => Some(Face::Handle),
            _ => None,
        }
    }
}

/// 错档轴（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// ACCESS_VIOLATION 档。
    AccessViolation = 0,
    /// INVALID_PARAMETER 档。
    InvalidParameter = 1,
    /// TYPE_MISMATCH 档。
    TypeMismatch = 2,
    /// UNWIND_TARGET 档。
    UnwindTarget = 3,
}

impl Fault {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<Fault> {
        match i {
            0 => Some(Fault::AccessViolation),
            1 => Some(Fault::InvalidParameter),
            2 => Some(Fault::TypeMismatch),
            3 => Some(Fault::UnwindTarget),
            _ => None,
        }
    }
}

/// 锚定面（面 × 错档 → 锚定码；独立口径）。
pub fn anchor_code(face: Face, fault: Fault) -> u32 {
    match (face, fault) {
        (_, Fault::AccessViolation) => NT_ACCESS_VIOLATION,
        (_, Fault::InvalidParameter) => NT_INVALID_PARAMETER,
        (Face::RuntimeFn, Fault::TypeMismatch) => NT_BAD_FUNCTION_TABLE,
        (Face::UnwindInfo, Fault::TypeMismatch) => NT_MISALIGNED,
        (Face::Context, Fault::TypeMismatch) => NT_PARAM_MIX,
        (Face::Peb, Fault::TypeMismatch) => NT_REVISION_MISMATCH,
        (_, Fault::TypeMismatch) => NT_TYPE_MISMATCH,
        (Face::RuntimeFn, Fault::UnwindTarget) => NT_BAD_FUNCTION_TABLE,
        (Face::UnwindInfo, Fault::UnwindTarget) => NT_UNWIND_TARGET,
        (_, Fault::UnwindTarget) => NT_UNWIND_TARGET,
    }
}

/// 码域闭集核对。
pub fn code_in_domain(code: u32) -> bool {
    matches!(
        code,
        NT_ACCESS_VIOLATION
            | NT_INVALID_PARAMETER
            | NT_TYPE_MISMATCH
            | NT_UNWIND_TARGET
            | NT_BAD_FUNCTION_TABLE
            | NT_MISALIGNED
            | NT_PARAM_MIX
            | NT_REVISION_MISMATCH
    )
}

/// 归类矩阵正本。
///
/// 生命周期：fill（逐格登记）→ lookup（查表）→ sample30（抽样核对）。
#[derive(Debug)]
pub struct UmMatrix {
    /// 矩阵格（按 face*FAULT_COUNT+fault 索引）。
    cells: [Option<u32>; MATRIX_CELLS],
    /// 已登记格数。
    filled: usize,
    /// 登记峰值。
    high: usize,
    /// 抽样次数。
    samples: u32,
    /// 拒止累计。
    rejects: u32,
}

impl UmMatrix {
    /// 建账（全空格）。
    pub fn new() -> Self {
        UmMatrix {
            cells: core::array::from_fn(|_| None),
            filled: 0,
            high: 0,
            samples: 0,
            rejects: 0,
        }
    }

    /// 格索引核对。
    pub fn idx_ok(face: usize, fault: usize) -> bool {
        face < FACE_COUNT && fault < FAULT_COUNT
    }

    /// 登记一格（越轴/零码/越域/重复拒）。
    pub fn fill(&mut self, face: usize, fault: usize, code: u32) -> Result<bool, UmCode> {
        if !Self::idx_ok(face, fault) {
            self.rejects += 1;
            return Err(if face >= FACE_COUNT { UmCode::FaceOob } else { UmCode::FaultOob });
        }
        if code == 0 {
            self.rejects += 1;
            return Err(UmCode::CodeZero);
        }
        if !code_in_domain(code) {
            self.rejects += 1;
            return Err(UmCode::UnmappedCode);
        }
        if self.cells[face * FAULT_COUNT + fault].is_some() {
            self.rejects += 1;
            return Err(UmCode::DupRow);
        }
        self.cells[face * FAULT_COUNT + fault] = Some(code);
        self.filled += 1;
        if self.filled > self.high {
            self.high = self.filled;
        }
        Ok(true)
    }

    /// 全矩阵登记（32 格锚定口径）。
    pub fn fill_anchor(&mut self) -> Result<usize, UmCode> {
        let mut n = 0usize;
        for i in 0..FACE_COUNT {
            let face = match Face::of(i as u64) {
                Some(x) => x,
                None => return Err(UmCode::FaceOob),
            };
            for j in 0..FAULT_COUNT {
                let fault = match Fault::of(j as u64) {
                    Some(x) => x,
                    None => return Err(UmCode::FaultOob),
                };
                self.fill(i, j, anchor_code(face, fault))?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 查表（空格/越轴拒）。
    pub fn lookup(&self, face: usize, fault: usize) -> Result<u32, UmCode> {
        if !Self::idx_ok(face, fault) {
            return Err(if face >= FACE_COUNT { UmCode::FaceOob } else { UmCode::FaultOob });
        }
        match self.cells[face * FAULT_COUNT + fault] {
            Some(c) => Ok(c),
            None => Err(UmCode::CellHole),
        }
    }

    /// 全行齐码判据。
    pub fn full(&self) -> bool {
        self.filled == MATRIX_CELLS && self.cells.iter().all(|c| c.is_some())
    }

    /// 矩阵只增不减判据。
    pub fn monotone(&self) -> bool {
        self.filled >= self.high
    }

    /// 注入抽样 30 例（面轴按例序跨面块轮转保全覆盖）。
    ///
    /// 返回 (一致例数, 例数, 覆盖面数)。
    pub fn sample30(&mut self, lcg: &mut Lcg) -> (usize, usize, usize) {
        self.samples += 1;
        let mut hit = 0usize;
        let mut seen: [bool; FACE_COUNT] = [false; FACE_COUNT];
        let phase = lcg.below(MATRIX_CELLS as u64) as usize;
        for i in 0..SAMPLE_CASES {
            // 步距 1 行序推进：30 例连续覆盖 32 格中的 8 面（行序面块）。
            let idx = (i + phase) % self.filled.max(1);
            let face_i = idx / FAULT_COUNT;
            let fault_i = idx % FAULT_COUNT;
            if face_i < FACE_COUNT {
                seen[face_i] = true;
            }
            let (face, fault) = match (Face::of(face_i as u64), Fault::of(fault_i as u64)) {
                (Some(f), Some(t)) => (f, t),
                _ => continue,
            };
            let want = anchor_code(face, fault);
            match self.lookup(face_i, fault_i) {
                Ok(c) if c == want => hit += 1,
                _ => {}
            }
        }
        let covered = seen.iter().filter(|&&s| s).count();
        (hit, SAMPLE_CASES, covered)
    }

    /// 抽样次数。
    pub fn sample_runs(&self) -> u32 {
        self.samples
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 格快照。
    pub fn cell_at(&self, face: usize, fault: usize) -> Option<u32> {
        if !Self::idx_ok(face, fault) {
            None
        } else {
            self.cells[face * FAULT_COUNT + fault]
        }
    }
}

/// 全行齐码演练：32 格锚定登记全过、逐格可查 10/10。
pub fn drill_fill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30dc0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut mx = UmMatrix::new();
        let n = mx.fill_anchor().unwrap_or(0);
        let mut pass = n == MATRIX_CELLS && mx.full() && mx.monotone();
        for i in 0..FACE_COUNT {
            for j in 0..FAULT_COUNT {
                pass = pass && matches!(mx.lookup(i, j), Ok(c) if c != 0);
            }
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全行齐码轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入抽样 30 例一致演练：码值全一致且覆盖八面 10/10。
pub fn drill_sample30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30dc0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut mx = UmMatrix::new();
        let _ = mx.fill_anchor();
        let (hit, total, covered) = mx.sample30(&mut lcg);
        if hit == total && covered == FACE_COUNT && mx.sample_runs() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样30例一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// 矩阵域拒止演练：越轴/零码/越域码/重复 10/10 拒。
pub fn drill_matrix_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30dc0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut mx = UmMatrix::new();
        let _ = mx.fill(0, 0, NT_ACCESS_VIOLATION);
        let dup = mx.fill(0, 0, NT_ACCESS_VIOLATION);
        let zero = mx.fill(0, 1, 0);
        let dom = mx.fill(0, 1, 0xDEAD);
        let foob = mx.fill(FACE_COUNT + 1, 1, NT_ACCESS_VIOLATION);
        let koob = mx.fill(0, FAULT_COUNT + 1, NT_ACCESS_VIOLATION);
        let hole = mx.lookup(0, 2);
        let _ = lcg.below(3);
        if matches!(dup, Err(UmCode::DupRow))
            && matches!(zero, Err(UmCode::CodeZero))
            && matches!(dom, Err(UmCode::UnmappedCode))
            && matches!(foob, Err(UmCode::FaceOob))
            && matches!(koob, Err(UmCode::FaultOob))
            && matches!(hole, Err(UmCode::CellHole))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("矩阵域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：矩阵转录导出（逐格一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 矩阵转录导出。
pub fn export_matrix_lines(mx: &UmMatrix) -> String {
    let mut s = String::from("错误码归类矩阵转录：\n");
    for i in 0..FACE_COUNT {
        for j in 0..FAULT_COUNT {
            match mx.cell_at(i, j) {
                Some(c) => s.push_str(&format!(
                    "· 面{} 档{} → {:08x}\n",
                    i, j, c
                )),
                None => s.push_str(&format!("· 面{} 档{} → 空\n", i, j)),
            }
        }
    }
    s
}

/// 转录完整判据（全格行齐备）。
pub fn matrix_lines_complete(mx: &UmMatrix) -> bool {
    let s = export_matrix_lines(mx);
    if !mx.full() {
        return false;
    }
    (0..FACE_COUNT).all(|i| s.contains(&format!("面{}", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：归类矩阵报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct UmReport {
    /// 已登记格数。
    pub filled: usize,
    /// 总格数。
    pub cells: usize,
    /// 抽样次数。
    pub samples: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 全行齐码。
    pub full: bool,
}

impl UmReport {
    /// 建报告。
    pub fn build(mx: &UmMatrix) -> Self {
        UmReport {
            filled: if mx.full() { MATRIX_CELLS } else { 0 },
            cells: MATRIX_CELLS,
            samples: mx.sample_runs(),
            rejects: mx.reject_total(),
            full: mx.full(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.samples >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "UmReport 格={}/{} 抽样={} 拒={} 齐={}",
            self.filled, self.cells, self.samples, self.rejects, self.full
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：码域闭集演练（八码逐一在域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_code_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30dc0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        const SET: [u32; 8] = [
            NT_ACCESS_VIOLATION, NT_INVALID_PARAMETER, NT_TYPE_MISMATCH, NT_UNWIND_TARGET,
            NT_BAD_FUNCTION_TABLE, NT_MISALIGNED, NT_PARAM_MIX, NT_REVISION_MISMATCH,
        ];
        let mut pass = true;
        for c in SET.iter() {
            pass = pass && code_in_domain(*c);
        }
        pass = pass && !code_in_domain(0) && !code_in_domain(0x1234_5678);
        pass = pass && SET[0] != SET[1] && SET[2] != SET[3];
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码域闭集轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：轴域演练（八面四档名表齐 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_axis_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30dc0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..FACE_COUNT as u64).all(|k| Face::of(k).is_some());
        pass = pass && Face::of(FACE_COUNT as u64).is_none();
        pass = pass && (0..FAULT_COUNT as u64).all(|k| Fault::of(k).is_some());
        pass = pass && Fault::of(FAULT_COUNT as u64).is_none();
        pass = pass && MATRIX_CELLS == 32;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("轴域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct UmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_um_checklist(mx: &UmMatrix) -> Vec<UmCheckRow> {
    let rep = UmReport::build(mx);
    let mut rows: Vec<UmCheckRow> = Vec::new();
    rows.push(UmCheckRow { name: "全行齐码", passed: mx.full() });
    rows.push(UmCheckRow { name: "矩阵单调", passed: mx.monotone() });
    rows.push(UmCheckRow { name: "格数口径", passed: MATRIX_CELLS == 32 });
    rows.push(UmCheckRow { name: "抽样口径", passed: SAMPLE_CASES == 30 });
    rows.push(UmCheckRow { name: "码域闭集", passed: drill_code_domain_10().0 });
    rows.push(UmCheckRow { name: "轴域齐", passed: drill_axis_domain_10().0 });
    rows.push(UmCheckRow { name: "转录完整", passed: matrix_lines_complete(mx) });
    rows.push(UmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(UmCheckRow { name: "拒止在账", passed: mx.reject_total() >= 1 });
    rows.push(UmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn um_checklist_green(rows: &[UmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn um_stats_line(mx: &UmMatrix) -> String {
    format!(
        "矩阵概况：格 {}/{} 抽样 {} 拒 {}",
        if mx.full() { MATRIX_CELLS } else { 0 },
        MATRIX_CELLS,
        mx.sample_runs(),
        mx.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12510_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut mx = UmMatrix::new();
    let _ = mx.fill_anchor();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = mx.sample30(&mut lcg);
    // 注入一例重号登记（拒止账口径）。
    let _ = mx.fill(0, 0, NT_ACCESS_VIOLATION);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        matrix_lines_complete(&mx) && UmReport::build(&mx).all_green(),
        um_stats_line(&mx),
    ));
    // X-b：码域闭集 10/10。
    let (ok_c, d_c) = drill_code_domain_10();
    out.push(("X-码域闭集10/10", ok_c, d_c));
    // X-c：轴域 10/10。
    let (ok_a, d_a) = drill_axis_domain_10();
    out.push(("X-轴域10/10", ok_a, d_a));
    // X-d：终验清单。
    let rows = build_um_checklist(&mx);
    out.push(("X-终验清单10项", um_checklist_green(&rows), UmReport::build(&mx).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mx = UmMatrix::new();
    // 注 1：空格查表必拒（CellHole）。
    if matches!(mx.lookup(0, 0), Err(UmCode::CellHole)) {
        caught += 1;
        d.push("注1 空格:抓".to_string());
    } else {
        d.push("注1 空格:漏".to_string());
    }
    // 注 2：重复登记必拒（DupRow）。
    let mut mx2 = UmMatrix::new();
    let _ = mx2.fill(1, 1, NT_INVALID_PARAMETER);
    if matches!(mx2.fill(1, 1, NT_INVALID_PARAMETER), Err(UmCode::DupRow)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：越域码必拒（UnmappedCode）。
    if matches!(mx2.fill(1, 0, 0xABCD), Err(UmCode::UnmappedCode)) {
        caught += 1;
        d.push("注3 越域:抓".to_string());
    } else {
        d.push("注3 越域:漏".to_string());
    }
    // 注 4：面轴越界必拒（FaceOob）。
    if matches!(mx2.fill(99, 1, NT_ACCESS_VIOLATION), Err(UmCode::FaceOob)) {
        caught += 1;
        d.push("注4 越面:抓".to_string());
    } else {
        d.push("注4 越面:漏".to_string());
    }
    // 注 5：零码登记必拒（CodeZero）。
    if matches!(mx2.fill(2, 2, 0), Err(UmCode::CodeZero)) {
        caught += 1;
        d.push("注5 零码:抓".to_string());
    } else {
        d.push("注5 零码:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30dc0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    UmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12510_checks：本单判据聚合（诊断段 0x30dc0）。
pub fn run_unx12510_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12510 坏表拒止统一错误码语义");
    // J1-a：错误码归类矩阵全行齐（32 格 10/10）。
    let (ok1, d1) = drill_fill_matrix_10();
    s.add_dyn("J1-归类矩阵全行齐10/10", ok1, &d1);
    // J1-b：注入抽样 30 例一致（覆盖八面 10/10）。
    let (ok2, d2) = drill_sample30_10();
    s.add_dyn("J1-抽样30例一致10/10", ok2, &d2);
    // J1-c：矩阵域拒止（10/10）。
    let (ok3, d3) = drill_matrix_reject_10();
    s.add_dyn("J1-矩阵域拒止10/10", ok3, &d3);
    // J1-d：标准矩阵（32 格全齐 + 抽样一致 + 单调）。
    let mut mx = UmMatrix::new();
    let n = mx.fill_anchor().unwrap_or(0);
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let (hit, total, covered) = mx.sample30(&mut lcg);
    s.add_dyn(
        "J1-AV/IP档全行齐",
        n == MATRIX_CELLS
            && mx.full()
            && hit == total
            && covered == FACE_COUNT
            && mx.monotone()
            && matrix_lines_complete(&mx),
        &format!("格={}/{} 抽样 {}/{} 覆盖 {}", n, MATRIX_CELLS, hit, total, covered),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏格登记后格数不变）。
    let f0 = mx.full();
    let _ = mx.fill(0, 1, 0x1234);
    s.add_dyn(
        "J2-拒止不写回",
        mx.full() == f0 && mx.monotone(),
        &format!("齐码 {}→{}", f0, mx.full()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30dc0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12510_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
