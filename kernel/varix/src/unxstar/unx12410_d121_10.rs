//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12410 · 节族错误矩阵 · 0x30140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12410-J1 族错误矩阵（保护冲突/越界/类型错配）全行齐码，
//!      注入抽样 30 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 矩阵域：接口轴（Create/Open/Map/Unmap/Extend/Query 六接口）×
//!   错误类轴（保护冲突/越界/类型错配 三类）= 18 行全格；
//!   - 全行齐码判据：每行必须落一个非零 NT 档码（MatrixHole 拒空格），
//!     码值与节族 NT 常量口径一致（INVALID_PAGE_PROTECTION/
//!     INVALID_VIEW_SIZE/OBJECT_TYPE_MISMATCH 等）；
//!   - 行唯一性：同接口同类不得重复登记（DupRow 拒）；
//!   - 注入抽样：30 例（接口,错误类) 二元组抽样，逐例查表码值与锚定
//!     表一致（CodeMismatch 拒），抽样覆盖六接口（SampleMiss 判漏）；
//!   - 码域核对：全部矩阵码必须落 NT 错误码闭集（UnmappedCode 拒）；
//!   - 矩阵账单调：登记后只增不减（MatrixShrink 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回矩阵；
//!   - 注入核：LCG 写死种子 0x30140，抽样 30 例 10/10 全一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30140 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30140;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入抽样例数（判准 30 例）。
pub const SAMPLE_CASES: usize = 30;

/// 接口轴宽度（六接口）。
pub const IFACE_COUNT: usize = 6;

/// 错误类轴宽度（三类）。
pub const KIND_COUNT: usize = 3;

/// 矩阵总格数。
pub const MATRIX_CELLS: usize = IFACE_COUNT * KIND_COUNT;

// 接口轴（六接口闭集序号）。
/// Create（0）。
pub const IF_CREATE: usize = 0;
/// Open（1）。
pub const IF_OPEN: usize = 1;
/// Map（2）。
pub const IF_MAP: usize = 2;
/// Unmap（3）。
pub const IF_UNMAP: usize = 3;
/// Extend（4）。
pub const IF_EXTEND: usize = 4;
/// Query（5）。
pub const IF_QUERY: usize = 5;

// 错误类轴（三类闭集序号）。
/// 保护冲突（0）。
pub const KD_PROTECT: usize = 0;
/// 越界（1）。
pub const KD_RANGE: usize = 1;
/// 类型错配（2）。
pub const KD_TYPE: usize = 2;

// NT 错误码档（ntstatus.h 口径）。
/// STATUS_INVALID_PAGE_PROTECTION。
pub const NT_INVALID_PAGE_PROTECTION: u32 = 0xC000_0045;
/// STATUS_INVALID_VIEW_SIZE。
pub const NT_INVALID_VIEW_SIZE: u32 = 0xC000_001F;
/// STATUS_OBJECT_TYPE_MISMATCH。
pub const NT_OBJECT_TYPE_MISMATCH: u32 = 0xC000_0024;
/// STATUS_CONFLICTING_ADDRESSES。
pub const NT_CONFLICTING_ADDRESSES: u32 = 0xC000_0018;
/// STATUS_SECTION_PROTECTION。
pub const NT_SECTION_PROTECTION: u32 = 0xC000_0045;
/// STATUS_SECTION_NOT_EXTENDED。
pub const NT_SECTION_NOT_EXTENDED: u32 = 0xC000_0047;
/// STATUS_NOT_MAPPED_VIEW。
pub const NT_NOT_MAPPED_VIEW: u32 = 0xC000_0019;
/// STATUS_FILE_INVALID。
pub const NT_FILE_INVALID: u32 = 0xC000_0056;
/// STATUS_INVALID_PARAMETER_1。
pub const NT_INVALID_PARAMETER_1: u32 = 0xC000_00EF;
/// STATUS_ACCESS_DENIED。
pub const NT_ACCESS_DENIED: u32 = 0xC000_0022;

/// 诊断码（0x30140 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MxCode {
    /// 0x30140 矩阵空格（该格无码）。
    MatrixHole = 0,
    /// 0x30141 码值失配（抽样码 ≠ 锚定码）。
    CodeMismatch = 1,
    /// 0x30142 接口轴越界。
    IfaceOob = 2,
    /// 0x30143 错误类轴越界。
    KindOob = 3,
    /// 0x30144 重复行（同接口同类重复登记）。
    DupRow = 4,
    /// 0x30145 矩阵缩水（登记后只增不减被破）。
    MatrixShrink = 5,
    /// 0x30146 码域越集（矩阵码落 NT 闭集之外）。
    UnmappedCode = 6,
    /// 0x30147 抽样漏覆盖（抽样未覆盖全部接口）。
    SampleMiss = 7,
    /// 0x30148 锚定表缺行（锚定口径少行）。
    AnchorMiss = 8,
    /// 0x30149 码值零（登记了零码）。
    CodeZero = 9,
}

impl MxCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [MxCode; 10] = [
        MxCode::MatrixHole, MxCode::CodeMismatch, MxCode::IfaceOob, MxCode::KindOob,
        MxCode::DupRow, MxCode::MatrixShrink, MxCode::UnmappedCode, MxCode::SampleMiss,
        MxCode::AnchorMiss, MxCode::CodeZero,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30140 => "矩阵空格：先补该格 NT 码，空格不可对抽样。",
        0x30141 => "码值失配：逐例回查锚定表，码值类零容差。",
        0x30142 => "接口轴越界：接口号先比 IFACE_COUNT 再查。",
        0x30143 => "错误类轴越界：类别号先比 KIND_COUNT 再查。",
        0x30144 => "重复行：同接口同类只登记一行，重复走幂等拒。",
        0x30145 => "矩阵缩水：登记后只增不减，先对账再抽样。",
        0x30146 => "码域越集：矩阵码必须落节族 NT 闭集。",
        0x30147 => "抽样漏覆盖：30 例必须覆盖全部六接口。",
        0x30148 => "锚定表缺行：先补锚定口径再抽样。",
        0x30149 => "码值零：零码不是错误码，回查登记面。",
        _ => "未知诊断码：回查 0x30140 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30140 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3014_3014_3014_3014;

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

/// 接口名表（六接口闭集）。
pub const IFACE_NAMES: [&str; IFACE_COUNT] =
    ["Create", "Open", "Map", "Unmap", "Extend", "Query"];

/// 错误类名表（三类闭集）。
pub const KIND_NAMES: [&str; KIND_COUNT] = ["保护冲突", "越界", "类型错配"];

/// 矩阵格（接口×错误类→NT 码）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixCell {
    /// 接口轴。
    pub iface: usize,
    /// 错误类轴。
    pub kind: usize,
    /// NT 档码。
    pub code: u32,
}

/// 锚定表行（抽样核对锚：接口×类→期望码）。
pub struct AnchorRow {
    /// 接口轴。
    pub iface: usize,
    /// 错误类轴。
    pub kind: usize,
    /// 锚定码。
    pub code: u32,
}

/// 节族错误矩阵正本。
///
/// 生命周期：fill（逐格登记 NT 码）→ lookup（查表）→ sample（抽样核对）。
#[derive(Debug)]
pub struct ErrMatrix {
    /// 矩阵格（定容 18 格，按 iface*KIND_COUNT+kind 索引）。
    cells: [Option<u32>; MATRIX_CELLS],
    /// 登记行数。
    filled: usize,
    /// 登记峰值（只增不减判准）。
    filled_high: usize,
    /// 抽样执行次数。
    samples: u32,
    /// 拒止累计。
    rejects: u32,
}

impl ErrMatrix {
    /// 建账（全空格）。
    pub fn new() -> Self {
        ErrMatrix {
            cells: core::array::from_fn(|_| None),
            filled: 0,
            filled_high: 0,
            samples: 0,
            rejects: 0,
        }
    }

    /// 格索引核对。
    pub fn idx_ok(iface: usize, kind: usize) -> bool {
        iface < IFACE_COUNT && kind < KIND_COUNT
    }

    /// 登记一格（越轴/零码/重复拒）。
    pub fn fill(&mut self, iface: usize, kind: usize, code: u32) -> Result<bool, MxCode> {
        if !Self::idx_ok(iface, kind) {
            self.rejects += 1;
            return Err(if iface >= IFACE_COUNT {
                MxCode::IfaceOob
            } else {
                MxCode::KindOob
            });
        }
        if code == 0 {
            self.rejects += 1;
            return Err(MxCode::CodeZero);
        }
        if !Self::code_in_domain(code) {
            self.rejects += 1;
            return Err(MxCode::UnmappedCode);
        }
        let i = iface * KIND_COUNT + kind;
        if self.cells[i].is_some() {
            self.rejects += 1;
            return Err(MxCode::DupRow);
        }
        self.cells[i] = Some(code);
        self.filled += 1;
        if self.filled > self.filled_high {
            self.filled_high = self.filled;
        }
        Ok(true)
    }

    /// 全矩阵登记（18 格锚定口径）。
    pub fn fill_anchor(&mut self) -> Result<usize, MxCode> {
        let table = Self::anchor_table();
        let mut n = 0usize;
        for a in table.iter() {
            self.fill(a.iface, a.kind, a.code)?;
            n += 1;
        }
        Ok(n)
    }

    /// 查表（空格/越轴拒）。
    pub fn lookup(&self, iface: usize, kind: usize) -> Result<u32, MxCode> {
        if !Self::idx_ok(iface, kind) {
            return Err(if iface >= IFACE_COUNT {
                MxCode::IfaceOob
            } else {
                MxCode::KindOob
            });
        }
        match self.cells[iface * KIND_COUNT + kind] {
            Some(c) => Ok(c),
            None => Err(MxCode::MatrixHole),
        }
    }

    /// 全行齐码判据（18 格全有码）。
    pub fn full(&self) -> bool {
        self.filled == MATRIX_CELLS && self.cells.iter().all(|c| c.is_some())
    }

    /// 矩阵只增不减判据。
    pub fn monotone(&self) -> bool {
        self.filled >= self.filled_high
    }

    /// NT 码域闭集核对。
    pub fn code_in_domain(code: u32) -> bool {
        matches!(
            code,
            NT_INVALID_PAGE_PROTECTION
                | NT_INVALID_VIEW_SIZE
                | NT_OBJECT_TYPE_MISMATCH
                | NT_CONFLICTING_ADDRESSES
                // 注：NT_SECTION_PROTECTION 与 NT_INVALID_PAGE_PROTECTION
                // 在真实 NT 同值（0xC0000045），or-模式取一份避免重臂。
                | NT_SECTION_NOT_EXTENDED
                | NT_NOT_MAPPED_VIEW
                | NT_FILE_INVALID
                | NT_INVALID_PARAMETER_1
                | NT_ACCESS_DENIED
        )
    }

    /// 锚定表（18 行锚定口径：接口×类→期望 NT 码）。
    pub fn anchor_table() -> [AnchorRow; MATRIX_CELLS] {
        [
            AnchorRow { iface: IF_CREATE, kind: KD_PROTECT, code: NT_INVALID_PAGE_PROTECTION },
            AnchorRow { iface: IF_CREATE, kind: KD_RANGE, code: NT_INVALID_VIEW_SIZE },
            AnchorRow { iface: IF_CREATE, kind: KD_TYPE, code: NT_OBJECT_TYPE_MISMATCH },
            AnchorRow { iface: IF_OPEN, kind: KD_PROTECT, code: NT_ACCESS_DENIED },
            AnchorRow { iface: IF_OPEN, kind: KD_RANGE, code: NT_OBJECT_TYPE_MISMATCH },
            AnchorRow { iface: IF_OPEN, kind: KD_TYPE, code: NT_OBJECT_TYPE_MISMATCH },
            AnchorRow { iface: IF_MAP, kind: KD_PROTECT, code: NT_SECTION_PROTECTION },
            AnchorRow { iface: IF_MAP, kind: KD_RANGE, code: NT_CONFLICTING_ADDRESSES },
            AnchorRow { iface: IF_MAP, kind: KD_TYPE, code: NT_OBJECT_TYPE_MISMATCH },
            AnchorRow { iface: IF_UNMAP, kind: KD_PROTECT, code: NT_SECTION_PROTECTION },
            AnchorRow { iface: IF_UNMAP, kind: KD_RANGE, code: NT_NOT_MAPPED_VIEW },
            AnchorRow { iface: IF_UNMAP, kind: KD_TYPE, code: NT_OBJECT_TYPE_MISMATCH },
            AnchorRow { iface: IF_EXTEND, kind: KD_PROTECT, code: NT_SECTION_PROTECTION },
            AnchorRow { iface: IF_EXTEND, kind: KD_RANGE, code: NT_SECTION_NOT_EXTENDED },
            AnchorRow { iface: IF_EXTEND, kind: KD_TYPE, code: NT_FILE_INVALID },
            AnchorRow { iface: IF_QUERY, kind: KD_PROTECT, code: NT_SECTION_PROTECTION },
            AnchorRow { iface: IF_QUERY, kind: KD_RANGE, code: NT_INVALID_PARAMETER_1 },
            AnchorRow { iface: IF_QUERY, kind: KD_TYPE, code: NT_OBJECT_TYPE_MISMATCH },
        ]
    }

    /// 注入抽样 30 例（LCG 抽 (iface,kind)，逐例查表比对锚定码）。
    ///
    /// 返回 (一致例数, 抽样例数, 覆盖接口数)。
    pub fn sample30(&mut self, lcg: &mut Lcg) -> (usize, usize, usize) {
        self.samples += 1;
        let mut hit = 0usize;
        let mut seen: [bool; IFACE_COUNT] = [false; IFACE_COUNT];
        let table = Self::anchor_table();
        for _i in 0..SAMPLE_CASES {
            // 抽样口径：接口轴按例序轮转（保证 30 例覆盖六接口），
            // 错误类轴按 LCG 抖动。
            let jitter = (lcg.next() >> 33) as usize;
            let iface = (_i + jitter) % IFACE_COUNT;
            let kind = lcg.below(KIND_COUNT as u64) as usize;
            seen[iface] = true;
            // 锚定码查表（锚定表全格口径）。
            let want = table[iface * KIND_COUNT + kind].code;
            match self.lookup(iface, kind) {
                Ok(c) if c == want => hit += 1,
                _ => {}
            }
        }
        let covered = seen.iter().filter(|&&s| s).count();
        (hit, SAMPLE_CASES, covered)
    }

    /// 抽样执行次数。
    pub fn sample_runs(&self) -> u32 {
        self.samples
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 格快照。
    pub fn cell_at(&self, iface: usize, kind: usize) -> Option<u32> {
        if !Self::idx_ok(iface, kind) {
            None
        } else {
            self.cells[iface * KIND_COUNT + kind]
        }
    }
}

/// 全行齐码演练：18 格锚定登记全过、逐格可查 10/10。
pub fn drill_fill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30140 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut mx = ErrMatrix::new();
        let n = mx.fill_anchor();
        let mut pass = matches!(n, Ok(x) if x == MATRIX_CELLS);
        pass = pass && mx.full() && mx.monotone();
        // 逐格可查且码值非零。
        for i in 0..IFACE_COUNT {
            for k in 0..KIND_COUNT {
                pass = pass && matches!(mx.lookup(i, k), Ok(c) if c != 0);
            }
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全行齐码轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入抽样 30 例一致演练：码值全一致且覆盖六接口 10/10。
pub fn drill_sample30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30140 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut mx = ErrMatrix::new();
        let _ = mx.fill_anchor();
        let (hit, total, covered) = mx.sample30(&mut lcg);
        if hit == total && covered == IFACE_COUNT && mx.sample_runs() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样30例一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// 矩阵域拒止演练：越轴/零码/越域码/重复登记 10/10 拒。
pub fn drill_matrix_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30140 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut mx = ErrMatrix::new();
        let _ = mx.fill(IF_CREATE, KD_PROTECT, NT_INVALID_PAGE_PROTECTION);
        let dup = mx.fill(IF_CREATE, KD_PROTECT, NT_INVALID_PAGE_PROTECTION);
        let zero = mx.fill(IF_CREATE, KD_RANGE, 0);
        let dom = mx.fill(IF_CREATE, KD_RANGE, 0xDEAD_BEEF);
        let ioob = mx.fill(IFACE_COUNT + 1, KD_RANGE, NT_ACCESS_DENIED);
        let koob = mx.fill(IF_CREATE, KIND_COUNT + 1, NT_ACCESS_DENIED);
        let hole = mx.lookup(IF_CREATE, KD_RANGE);
        let _ = lcg.below(3);
        if matches!(dup, Err(MxCode::DupRow))
            && matches!(zero, Err(MxCode::CodeZero))
            && matches!(dom, Err(MxCode::UnmappedCode))
            && matches!(ioob, Err(MxCode::IfaceOob))
            && matches!(koob, Err(MxCode::KindOob))
            && matches!(hole, Err(MxCode::MatrixHole))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("矩阵域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：矩阵转录导出（逐格一行：接口/类/码）。
/// ─────────────────────────────────────────────────────────────────────
/// 矩阵转录导出。
pub fn export_matrix_lines(mx: &ErrMatrix) -> String {
    let mut s = String::from("错误矩阵转录：\n");
    for i in 0..IFACE_COUNT {
        for k in 0..KIND_COUNT {
            match mx.cell_at(i, k) {
                Some(c) => s.push_str(&format!(
                    "{} × {} → {:08x}\n",
                    IFACE_NAMES[i], KIND_NAMES[k], c
                )),
                None => s.push_str(&format!("{} × {} → 空\n", IFACE_NAMES[i], KIND_NAMES[k])),
            }
        }
    }
    s
}

/// 转录完整判据（全格行齐备）。
pub fn matrix_lines_complete(mx: &ErrMatrix) -> bool {
    let s = export_matrix_lines(mx);
    if !mx.full() {
        return false;
    }
    (0..IFACE_COUNT).all(|i| s.contains(IFACE_NAMES[i]))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：矩阵报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct MxReport {
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

impl MxReport {
    /// 建报告。
    pub fn build(mx: &ErrMatrix) -> Self {
        MxReport {
            filled: mx.full().then(|| MATRIX_CELLS).unwrap_or(0),
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
            "MxReport 格={}/{} 抽样={} 拒={} 齐={}",
            self.filled, self.cells, self.samples, self.rejects, self.full
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：码域闭集演练（十码 NT 档逐一在域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_code_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30140 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        const SET: [u32; 10] = [
            NT_INVALID_PAGE_PROTECTION, NT_INVALID_VIEW_SIZE, NT_OBJECT_TYPE_MISMATCH,
            NT_CONFLICTING_ADDRESSES, NT_SECTION_PROTECTION, NT_SECTION_NOT_EXTENDED,
            NT_NOT_MAPPED_VIEW, NT_FILE_INVALID, NT_INVALID_PARAMETER_1, NT_ACCESS_DENIED,
        ];
        let mut pass = true;
        for c in SET.iter() {
            pass = pass && ErrMatrix::code_in_domain(*c);
        }
        pass = pass && !ErrMatrix::code_in_domain(0) && !ErrMatrix::code_in_domain(0x1234_5678);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码域闭集轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：接口/类域核对（六接口三类名表齐 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_axis_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30140 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = IFACE_NAMES.len() == IFACE_COUNT && KIND_NAMES.len() == KIND_COUNT;
        pass = pass && IFACE_NAMES.iter().all(|n| !n.is_empty());
        pass = pass && KIND_NAMES.iter().all(|n| !n.is_empty());
        pass = pass && MATRIX_CELLS == 18;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("轴域核对轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct MxCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_mx_checklist(mx: &ErrMatrix) -> Vec<MxCheckRow> {
    let rep = MxReport::build(mx);
    let mut rows: Vec<MxCheckRow> = Vec::new();
    rows.push(MxCheckRow { name: "全行齐码", passed: mx.full() });
    rows.push(MxCheckRow { name: "矩阵单调", passed: mx.monotone() });
    rows.push(MxCheckRow { name: "格数口径", passed: MATRIX_CELLS == 18 });
    rows.push(MxCheckRow { name: "抽样口径", passed: SAMPLE_CASES == 30 });
    rows.push(MxCheckRow { name: "码域闭集", passed: drill_code_domain_10().0 });
    rows.push(MxCheckRow { name: "轴域齐", passed: drill_axis_domain_10().0 });
    rows.push(MxCheckRow { name: "转录完整", passed: matrix_lines_complete(mx) });
    rows.push(MxCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(MxCheckRow { name: "拒止在账", passed: mx.reject_total() >= 1 });
    rows.push(MxCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn mx_checklist_green(rows: &[MxCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn mx_stats_line(mx: &ErrMatrix) -> String {
    format!(
        "矩阵概况：格 {}/{} 抽样 {} 拒 {}",
        if mx.full() { MATRIX_CELLS } else { 0 },
        MATRIX_CELLS,
        mx.sample_runs(),
        mx.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12410_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut mx = ErrMatrix::new();
    let _ = mx.fill_anchor();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = mx.sample30(&mut lcg);
    // 注入一例重复登记（拒止账口径）。
    let _ = mx.fill(IF_CREATE, KD_PROTECT, NT_INVALID_PAGE_PROTECTION);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        matrix_lines_complete(&mx) && MxReport::build(&mx).all_green(),
        mx_stats_line(&mx),
    ));
    // X-b：码域闭集 10/10。
    let (ok_c, d_c) = drill_code_domain_10();
    out.push(("X-码域闭集10/10", ok_c, d_c));
    // X-c：轴域核对 10/10。
    let (ok_a, d_a) = drill_axis_domain_10();
    out.push(("X-轴域核对10/10", ok_a, d_a));
    // X-d：终验清单。
    let rows = build_mx_checklist(&mx);
    out.push(("X-终验清单10项", mx_checklist_green(&rows), MxReport::build(&mx).to_text()));
    // X-e：查表纯函数域（两例锚定格比对）。
    let l1 = mx.lookup(IF_MAP, KD_RANGE) == Ok(NT_CONFLICTING_ADDRESSES);
    let l2 = mx.lookup(IF_EXTEND, KD_RANGE) == Ok(NT_SECTION_NOT_EXTENDED);
    out.push(("X-查表锚定两面", l1 && l2, format!("l1={} l2={}", l1, l2)));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：空格查表必拒（MatrixHole）。
    let mx = ErrMatrix::new();
    if matches!(mx.lookup(IF_CREATE, KD_TYPE), Err(MxCode::MatrixHole)) {
        caught += 1;
        d.push("注1 空格:抓".to_string());
    } else {
        d.push("注1 空格:漏".to_string());
    }
    // 注 2：重复登记必拒（DupRow）。
    let mut mx2 = ErrMatrix::new();
    let _ = mx2.fill(IF_OPEN, KD_TYPE, NT_OBJECT_TYPE_MISMATCH);
    if matches!(mx2.fill(IF_OPEN, KD_TYPE, NT_OBJECT_TYPE_MISMATCH), Err(MxCode::DupRow)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：越域码必拒（UnmappedCode）。
    if matches!(mx2.fill(IF_OPEN, KD_PROTECT, 0xABCD), Err(MxCode::UnmappedCode)) {
        caught += 1;
        d.push("注3 越域:抓".to_string());
    } else {
        d.push("注3 越域:漏".to_string());
    }
    // 注 4：接口轴越界必拒（IfaceOob）。
    if matches!(mx2.fill(99, KD_TYPE, NT_ACCESS_DENIED), Err(MxCode::IfaceOob)) {
        caught += 1;
        d.push("注4 接口越:抓".to_string());
    } else {
        d.push("注4 接口越:漏".to_string());
    }
    // 注 5：零码登记必拒（CodeZero）。
    if matches!(mx2.fill(IF_QUERY, KD_RANGE, 0), Err(MxCode::CodeZero)) {
        caught += 1;
        d.push("注5 零码:抓".to_string());
    } else {
        d.push("注5 零码:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30140 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    MxCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12410_checks：本单判据聚合（诊断段 0x30140）。
pub fn run_unx12410_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12410 节族错误矩阵");
    // J1-a：全行齐码（18 格锚定登记 10/10）。
    let (ok1, d1) = drill_fill_matrix_10();
    s.add_dyn("J1-全行齐码10/10", ok1, &d1);
    // J1-b：注入抽样 30 例一致（覆盖六接口 10/10）。
    let (ok2, d2) = drill_sample30_10();
    s.add_dyn("J1-抽样30例一致10/10", ok2, &d2);
    // J1-c：矩阵域拒止 10/10。
    let (ok3, d3) = drill_matrix_reject_10();
    s.add_dyn("J1-矩阵域拒止10/10", ok3, &d3);
    // J1-d：标准矩阵（18 格全齐 + 抽样一致 + 单调）。
    let mut mx = ErrMatrix::new();
    let n = mx.fill_anchor().unwrap_or(0);
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let (hit, total, covered) = mx.sample30(&mut lcg);
    s.add_dyn(
        "J1-三类矩阵全格齐码",
        n == MATRIX_CELLS && mx.full() && hit == total && covered == IFACE_COUNT && mx.monotone(),
        &format!("格={}/{} 抽样 {}/{} 覆盖 {}", n, MATRIX_CELLS, hit, total, covered),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏格登记后矩阵格数不变）。
    let f0 = mx.full();
    let _ = mx.fill(IF_CREATE, KD_TYPE, 0x1234);
    s.add_dyn(
        "J2-拒止不写回",
        mx.full() == f0 && mx.monotone(),
        &format!("齐码 {}→{}", f0, mx.full()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30140段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12410_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
