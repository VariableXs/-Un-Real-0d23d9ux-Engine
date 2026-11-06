//! UNX-F11587 busybox ls 与 coreutils ls 差异矩阵锚
//!
//! busybox 的 `ls` 不是 GNU coreutils `ls` 的裁剪副本，而是一份**独立实现**：
//! 选项拼写相同的地方，语义常常不同；选项根本不存在的地方，脚本会静默退化。
//! 本模块把这种“同形不同义”固化成一张**差异矩阵**：行是选项（短选项与长选项
//! 各占一行），列是四条可观测行为——
//!
//! - 默认列格式（单列 / 多列 / 长格式的选择与列宽折算）；
//! - 时间样式（mtime/ctime/atime 的呈现规则与“近六个月”判据）；
//! - 着色（是否识别 `--color` 及 `LS_COLORS` 环境）；
//! - inode / 块数显示（`-i`、`-s` 的数字列宽与对齐）。
//!
//! 每个单元格取三值之一：`SAME`（语义一致）、`DIVERGES`（语义分歧）、
//! `MISSING`（busybox 无此选项，整行四个格子都记 MISSING——选项不存在时，
//! 讨论某一列的行为没有意义，这是矩阵的**整行缺席**约定，不是四份独立缺失）。
//!
//! 行裁决由格子聚合：只要有一个 MISSING 就判“缺失”，否则只要有一个
//! DIVERGES 就判“分歧”，全 SAME 才判“一致”。三档互斥，聚合函数必须先扫
//! 完整行再给结论，不允许短路——短路会让“既缺失又分歧”的行随机落到某一档。
//!
//! 矩阵之外还有三层运行期判据：
//!
//! 1. **选项次序**：`-1` 与 `-l`、`-C` 与 `-x`、`-c` 与 `-u`、`-S` 与 `-t`
//!    四组互斥；busybox 不做“后者覆盖前者”，而是直接拒绝（`EORDER`）。
//! 2. **终端态**：显式 `-C` 在非终端上必须拒绝（`ETTY`），隐式默认列格式
//!    则允许退化为单列。
//! 3. **着色策略**：`never/auto/always` 三态，未知策略值拒绝（`ECOLOR`）；
//!    `auto` 只在“是终端且 TERM 不是 dumb”时开启。
//!
//! 列布局按“最长名字 + 2 空格”折算单元格宽度，列数向下取整且至少为 1；
//! 行数向上取整；再反算每行列数，最后用 `cols * rows >= entries` 复核，
//! 复核失败说明宽度参数不可用（`EWIDTH`）。这条复核是为了拦住
//! “窄屏 + 长名字 + 大目录”时列数归零导致的除零。
//!
//! 闸门次序：`cell`：列号 → 行号 → 单元格值；`row_verdict`：行号 → 逐格扫描
//! （先缺席后分歧）；`resolve`：未知选项 → 重复选项 → 互斥次序 → 终端态 →
//! 格式归并；`render_name`：空名 → 类型 → 引号策略；`layout`：宽度为零 →
//! 列数兜底 → 行数 → 乘积复核。
//!
//! 账本登记十四格：八格成功（两格单元格、一格行裁决、一格格式归并、一格
//! 布局、一格引号渲染、一格覆盖率），六格失败（列号越界、行号未知、次序
//! 冲突、终端态拒绝、宽度为零、颜色策略非法）。冻结时把十四格连同拒绝计数
//! 折进 FNV-1a 票号，重放要求重新构建的账本票号逐位相同。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfa00;

/// 选项不在矩阵内（未知选项）。
pub const LS_EOPT: u32 = SEG_BASE + 0x01;
/// 列号越界或单元格取值非法。
pub const LS_ECOL: u32 = SEG_BASE + 0x02;
/// 行号未知或名字查不到行。
pub const LS_EROW: u32 = SEG_BASE + 0x03;
/// 互斥选项同时出现（次序无法归并）。
pub const LS_EORDER: u32 = SEG_BASE + 0x04;
/// 选项组合在语义上不可能（重复、空名、未知类型）。
pub const LS_EMODE: u32 = SEG_BASE + 0x05;
/// 终端态不允许该选项。
pub const LS_ETTY: u32 = SEG_BASE + 0x06;
/// 颜色策略非法。
pub const LS_ECOLOR: u32 = SEG_BASE + 0x07;
/// 布局宽度为零或列数复核失败。
pub const LS_EWIDTH: u32 = SEG_BASE + 0x08;
/// 账本形状不符（14/8/6）。
pub const LS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或重复登记。
pub const LS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与登记值不符。
pub const LS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放票号不一致。
pub const LS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 矩阵行数（26 个选项）。
pub const ROWS: usize = 26;
/// 矩阵列数（四条行为）。
pub const COLS: usize = 4;
/// 矩阵单元格总数。
pub const CELLS: usize = ROWS * COLS;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 行为列：默认列格式。
pub const COL_COLUMNS: u8 = 0;
/// 行为列：时间样式。
pub const COL_TIME: u8 = 1;
/// 行为列：着色。
pub const COL_COLOR: u8 = 2;
/// 行为列：inode / 块数。
pub const COL_INODE: u8 = 3;

/// 单元格：语义一致。
pub const SAME: u8 = 0;
/// 单元格：语义分歧。
pub const DIVERGES: u8 = 1;
/// 单元格：busybox 无此选项。
pub const MISSING: u8 = 2;

/// 行裁决：一致。
pub const V_SAME: u8 = 0;
/// 行裁决：分歧。
pub const V_DIV: u8 = 1;
/// 行裁决：缺失。
pub const V_MISS: u8 = 2;
/// 行裁决：非法（内部）。
pub const V_BAD: u8 = 3;

/// 列格式：单列。
pub const FORMAT_ONE: u8 = 0;
/// 列格式：长格式。
pub const FORMAT_LONG: u8 = 1;
/// 列格式：纵列优先。
pub const FORMAT_COLUMNS: u8 = 2;
/// 列格式：横排优先。
pub const FORMAT_ACROSS: u8 = 3;

/// 颜色策略：从不。
pub const COLOR_NEVER: u8 = 0;
/// 颜色策略：自动。
pub const COLOR_AUTO: u8 = 1;
/// 颜色策略：总是。
pub const COLOR_ALWAYS: u8 = 2;

/// 引号策略：原样。
pub const QUOTE_LITERAL: u8 = 0;
/// 引号策略：shell 转义。
pub const QUOTE_SHELL: u8 = 1;
/// 引号策略：C 风格转义。
pub const QUOTE_C: u8 = 2;

/// 文件类型：普通文件。
pub const KIND_REG: u8 = 0;
/// 文件类型：目录。
pub const KIND_DIR: u8 = 1;
/// 文件类型：可执行。
pub const KIND_EXE: u8 = 2;
/// 文件类型：符号链接。
pub const KIND_LINK: u8 = 3;
/// 文件类型：FIFO。
pub const KIND_FIFO: u8 = 4;
/// 文件类型：套接字。
pub const KIND_SOCK: u8 = 5;

/// 操作码：查单元格。
pub const OP_CELL: u8 = 1;
/// 操作码：行裁决。
pub const OP_VERDICT: u8 = 2;
/// 操作码：选项归并。
pub const OP_RESOLVE: u8 = 3;
/// 操作码：列布局。
pub const OP_LAYOUT: u8 = 4;
/// 操作码：名字渲染。
pub const OP_RENDER: u8 = 5;
/// 操作码：覆盖率。
pub const OP_COVER: u8 = 6;
/// 操作码：颜色策略。
pub const OP_COLOR: u8 = 7;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（每字节一次乘法，共八次）。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 十六进制字符表（渲染 C 风格转义用）。
const HEX: [u8; 16] = *b"0123456789abcdef";

/// 选项短字母（长选项行为 0）。
pub const SHORT: [u8; ROWS] = [
    b'1', b'a', b'A', b'C', b'F', b'G', b'H', b'R', b'S', b'c', b'd', b'i', b'l', b'n', b'p',
    b'q', b'r', b's', b't', b'u', b'x', b'h', 0, 0, 0, 0,
];

/// 选项规范名（短选项为字母，长选项带 `--` 前缀）。
pub const NAMES: [&str; ROWS] = [
    "one-per-line",
    "all",
    "almost-all",
    "columns",
    "classify",
    "no-group",
    "dereference",
    "recursive",
    "sort-size",
    "time-ctime",
    "directory",
    "inode",
    "long",
    "numeric-uid",
    "classify-dir",
    "hide-control",
    "reverse",
    "size",
    "sort-time",
    "time-atime",
    "across",
    "human",
    "--color",
    "--time-style",
    "--full-time",
    "--quoting-style",
];

/// 差异矩阵：26 行 × 4 列。
///
/// 行的顺序与 `SHORT` / `NAMES` 严格对应；列的顺序与 `COL_*` 严格对应。
/// 任何一处错位都会让“行裁决”与“定向核对”互相矛盾，因此账本第一格就
/// 抽查 `c` 行的时间列与 `G` 行的颜色列。
pub const MATRIX: [[u8; COLS]; ROWS] = [
    [SAME, SAME, SAME, SAME],             // 1
    [SAME, SAME, SAME, SAME],             // a
    [SAME, SAME, SAME, SAME],             // A
    [DIVERGES, SAME, SAME, SAME],         // C：列宽折算算法不同
    [SAME, SAME, SAME, SAME],             // F
    [MISSING, MISSING, MISSING, MISSING], // G：busybox 无 -G
    [DIVERGES, SAME, SAME, SAME],         // H：命令行符号链接解引用不同
    [SAME, SAME, SAME, SAME],             // R
    [SAME, SAME, SAME, SAME],             // S
    [SAME, DIVERGES, SAME, SAME],         // c：ctime 呈现规则不同
    [SAME, SAME, SAME, SAME],             // d
    [SAME, SAME, SAME, SAME],             // i
    [SAME, DIVERGES, SAME, SAME],         // l：长格式时间阈值不同
    [SAME, DIVERGES, SAME, SAME],         // n：数字 uid/gid 列宽不同
    [SAME, SAME, SAME, SAME],             // p
    [SAME, SAME, SAME, SAME],             // q
    [SAME, SAME, SAME, SAME],             // r
    [SAME, SAME, SAME, SAME],             // s
    [SAME, SAME, SAME, SAME],             // t
    [SAME, DIVERGES, SAME, SAME],         // u：atime 呈现规则不同
    [DIVERGES, SAME, SAME, SAME],         // x：横排顺序不同
    [SAME, SAME, SAME, SAME],             // h
    [MISSING, MISSING, MISSING, MISSING], // --color
    [MISSING, MISSING, MISSING, MISSING], // --time-style
    [SAME, DIVERGES, SAME, SAME],         // --full-time
    [MISSING, MISSING, MISSING, MISSING], // --quoting-style
];

/// 一列的布局折算结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// 列数（至少 1）。
    pub cols: u32,
    /// 行数（向上取整）。
    pub rows: u32,
    /// 反算出的每行列数。
    pub per_line: u32,
}

impl Layout {
    /// 空格子。
    pub const fn empty() -> Layout {
        Layout { cols: 0, rows: 0, per_line: 0 }
    }

    /// 容量是否覆盖条目数。
    pub fn covers(&self, entries: u32) -> bool {
        match self.cols.checked_mul(self.rows) {
            Some(cap) => cap >= entries,
            None => false,
        }
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本（定容十四格）。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格：槽位必须等于当前长度，冻结后拒收。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(LS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账：形状、槽序、成功值与失败码逐格复核。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(LS_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 2, 2, 0, 20, 8, 76];
        const CODES: [u32; 6] =
            [LS_ECOL, LS_EROW, LS_EORDER, LS_ETTY, LS_EWIDTH, LS_ECOLOR];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LS_EBAL),
            };
            if s.slot as usize != i {
                return Err(LS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LS_EVALUE);
                        }
                    }
                    None => return Err(LS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LS_EVALUE);
                        }
                    }
                    None => return Err(LS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LS_EBAL);
        }
        Ok(())
    }

    /// 票号：十四格五字段 + 拒绝计数。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结：只允许一次，重复冻结计入拒绝。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(LS_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建账本并要求票号一致。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(LS_ESEAL)
        }
    }

    /// 冻结票号。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 行号 → 短字母（长选项为 0）。
pub fn short_at(row: u8) -> Result<u8, u32> {
    match SHORT.get(row as usize) {
        Some(v) => Ok(*v),
        None => Err(LS_EROW),
    }
}

/// 行号 → 规范名。
pub fn name_at(row: u8) -> Result<&'static str, u32> {
    match NAMES.get(row as usize) {
        Some(v) => Ok(v),
        None => Err(LS_EROW),
    }
}

/// 规范名 → 行号（线性扫描，26 行足够小）。
pub fn row_of_name(name: &str) -> Result<u8, u32> {
    let mut i = 0usize;
    while i < ROWS {
        if let Some(n) = NAMES.get(i) {
            if *n == name {
                return Ok(i as u8);
            }
        }
        i += 1;
    }
    Err(LS_EROW)
}

/// 短字母 → 行号。
pub fn row_of_short(letter: u8) -> Result<u8, u32> {
    if letter == 0 {
        return Err(LS_EROW);
    }
    let mut i = 0usize;
    while i < ROWS {
        if let Some(s) = SHORT.get(i) {
            if *s == letter {
                return Ok(i as u8);
            }
        }
        i += 1;
    }
    Err(LS_EROW)
}

/// 查一个单元格：先验列号，再验行号，最后验取值。
pub fn cell(row: u8, col: u8) -> Result<u8, u32> {
    if col as usize >= COLS {
        return Err(LS_ECOL);
    }
    let r = match MATRIX.get(row as usize) {
        Some(v) => v,
        None => return Err(LS_EROW),
    };
    match r.get(col as usize) {
        Some(v) => {
            if *v > MISSING {
                Err(LS_ECOL)
            } else {
                Ok(*v)
            }
        }
        None => Err(LS_ECOL),
    }
}

/// 行裁决：整行扫描，先缺席后分歧。
pub fn row_verdict(row: u8) -> Result<u8, u32> {
    let r = match MATRIX.get(row as usize) {
        Some(v) => v,
        None => return Err(LS_EROW),
    };
    let mut missing = false;
    let mut diverges = false;
    let mut i = 0usize;
    while i < COLS {
        match r.get(i) {
            Some(v) => {
                if *v == MISSING {
                    missing = true;
                } else if *v == DIVERGES {
                    diverges = true;
                } else if *v != SAME {
                    return Err(LS_ECOL);
                }
            }
            None => return Err(LS_ECOL),
        }
        i += 1;
    }
    if missing {
        Ok(V_MISS)
    } else if diverges {
        Ok(V_DIV)
    } else {
        Ok(V_SAME)
    }
}

/// 按裁决统计行数。
pub fn rows_with_verdict(want: u8) -> Result<u32, u32> {
    if want > V_MISS {
        return Err(LS_EORDER);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        if row_verdict(i as u8)? == want {
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 按取值统计单元格数。
pub fn cells_with(kind: u8) -> Result<u32, u32> {
    if kind > MISSING {
        return Err(LS_ECOL);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        let r = match MATRIX.get(i) {
            Some(v) => v,
            None => return Err(LS_EROW),
        };
        let mut j = 0usize;
        while j < COLS {
            if let Some(v) = r.get(j) {
                if *v == kind {
                    n += 1;
                } else if *v > MISSING {
                    return Err(LS_ECOL);
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 某一列里非 SAME 的单元格数。
pub fn column_gap(col: u8) -> Result<u32, u32> {
    if col as usize >= COLS {
        return Err(LS_ECOL);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        if cell(i as u8, col)? != SAME {
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 覆盖率：SAME 单元格占全矩阵的百分比（向下取整）。
pub fn coverage_pct() -> Result<u32, u32> {
    let same = cells_with(SAME)?;
    Ok(same.saturating_mul(100) / CELLS as u32)
}

/// 保真率：非 MISSING 单元格占全矩阵的百分比（向下取整）。
pub fn fidelity_pct() -> Result<u32, u32> {
    let missing = cells_with(MISSING)?;
    let present = (CELLS as u32).saturating_sub(missing);
    Ok(present.saturating_mul(100) / CELLS as u32)
}

/// 选项名是否互不重复（矩阵行的身份判据）。
pub fn names_unique() -> bool {
    let mut i = 0usize;
    while i < ROWS {
        let a = match NAMES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < ROWS {
            if let Some(b) = NAMES.get(j) {
                if a == *b {
                    return false;
                }
            } else {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 裁决档位的中文标签。
pub fn verdict_label(v: u8) -> Result<&'static str, u32> {
    match v {
        V_SAME => Ok("一致"),
        V_DIV => Ok("分歧"),
        V_MISS => Ok("缺失"),
        _ => Err(LS_EORDER),
    }
}

/// 选项归并：校验未知、重复、互斥、终端态，返回最终列格式。
pub fn resolve(opts: &[u8], tty: bool) -> Result<u8, u32> {
    let mut seen = [false; ROWS];
    let mut i = 0usize;
    while i < opts.len() {
        let id = match opts.get(i) {
            Some(v) => *v,
            None => return Err(LS_EOPT),
        };
        if id as usize >= ROWS {
            return Err(LS_EOPT);
        }
        if let Some(slot) = seen.get_mut(id as usize) {
            if *slot {
                return Err(LS_EMODE);
            }
            *slot = true;
        }
        i += 1;
    }
    // 互斥组：(one,long) (columns,across) (ctime,atime) (sort-size,sort-time)。
    if seen[0] && seen[12] {
        return Err(LS_EORDER);
    }
    if seen[3] && seen[20] {
        return Err(LS_EORDER);
    }
    if seen[9] && seen[19] {
        return Err(LS_EORDER);
    }
    if seen[8] && seen[18] {
        return Err(LS_EORDER);
    }
    if seen[3] && !tty {
        return Err(LS_ETTY);
    }
    if seen[0] {
        return Ok(FORMAT_ONE);
    }
    if seen[12] {
        return Ok(FORMAT_LONG);
    }
    if seen[20] {
        return Ok(FORMAT_ACROSS);
    }
    Ok(FORMAT_COLUMNS)
}

/// 颜色策略：三态 + 环境护栏。
pub fn resolve_color(policy: u8, tty: bool, dumb: bool) -> Result<bool, u32> {
    match policy {
        COLOR_NEVER => Ok(false),
        COLOR_ALWAYS => Ok(true),
        COLOR_AUTO => Ok(tty && !dumb),
        _ => Err(LS_ECOLOR),
    }
}

/// 列布局折算：单元格宽 = 最长名 + 2；列数向下取整且至少 1。
pub fn layout(entries: u32, name_max: u32, screen_cols: u32) -> Result<Layout, u32> {
    if screen_cols == 0 || name_max == 0 {
        return Err(LS_EWIDTH);
    }
    if entries == 0 {
        return Ok(Layout { cols: 1, rows: 0, per_line: 0 });
    }
    let cell_w = name_max.saturating_add(2);
    let mut cols = screen_cols / cell_w;
    if cols == 0 {
        cols = 1;
    }
    let rows = entries.saturating_add(cols - 1) / cols;
    let per_line = if rows == 0 { 0 } else { entries.saturating_add(rows - 1) / rows };
    let out = Layout { cols, rows, per_line };
    if !out.covers(entries) {
        return Err(LS_EWIDTH);
    }
    Ok(out)
}

/// 文件类型 → classify 后缀（无后缀返回 0）。
pub fn classify_suffix(kind: u8) -> Result<char, u32> {
    match kind {
        KIND_REG => Ok('\0'),
        KIND_DIR => Ok('/'),
        KIND_EXE => Ok('*'),
        KIND_LINK => Ok('@'),
        KIND_FIFO => Ok('|'),
        KIND_SOCK => Ok('='),
        _ => Err(LS_EMODE),
    }
}

/// 名字是否必须加引号（含空白、引号、反斜杠或控制字节）。
fn needs_quote(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b <= 0x20 || *b == b'\'' || *b == b'"' || *b == b'\\' || *b >= 0x7f {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 名字渲染：原样 / shell 单引号 / C 风格转义，最后按类型追加 classify 后缀。
pub fn render_name(name: &str, kind: u8, quote: u8, classify: bool) -> Result<String, u32> {
    if name.is_empty() {
        return Err(LS_EROW);
    }
    let suffix = classify_suffix(kind)?;
    let mut out = String::new();
    if quote == QUOTE_LITERAL {
        out.push_str(name);
    } else if quote == QUOTE_SHELL {
        if needs_quote(name) {
            out.push('\'');
            let bytes = name.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() {
                match bytes.get(i) {
                    Some(b) => {
                        if *b == b'\'' {
                            out.push_str("'\\''");
                        } else {
                            out.push(*b as char);
                        }
                    }
                    None => return Err(LS_EVALUE),
                }
                i += 1;
            }
            out.push('\'');
        } else {
            out.push_str(name);
        }
    } else if quote == QUOTE_C {
        let bytes = name.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes.get(i) {
                Some(b) => {
                    let c = *b;
                    if c == b'\\' || c == b'"' {
                        out.push('\\');
                        out.push(c as char);
                    } else if c < 0x20 || c >= 0x7f {
                        out.push('\\');
                        out.push('x');
                        let hi = (c >> 4) & 0x0f;
                        let lo = c & 0x0f;
                        if let Some(ch) = HEX.get(hi as usize) {
                            out.push(*ch as char);
                        }
                        if let Some(ch) = HEX.get(lo as usize) {
                            out.push(*ch as char);
                        }
                    } else {
                        out.push(c as char);
                    }
                }
                None => return Err(LS_EVALUE),
            }
            i += 1;
        }
    } else {
        return Err(LS_EMODE);
    }
    if classify {
        if suffix != '\0' {
            out.push(suffix);
        }
    }
    Ok(out)
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_CELL, cell(3, COL_COLUMNS).map(|v| v as u32)));
    let _ = l.register(word_cell(1, OP_CELL, cell(9, COL_TIME).map(|v| v as u32)));
    let _ = l.register(word_cell(2, OP_CELL, cell(22, COL_COLOR).map(|v| v as u32)));
    let _ = l.register(word_cell(3, OP_VERDICT, row_verdict(5).map(|v| v as u32)));
    let _ = l.register(word_cell(4, OP_RESOLVE, resolve(&[0u8], true).map(|v| v as u32)));
    let _ = l.register(word_cell(
        5,
        OP_LAYOUT,
        layout(100, 14, 80).map(|lay| lay.rows),
    ));
    let _ = l.register(word_cell(
        6,
        OP_RENDER,
        render_name("sp ace", KIND_REG, QUOTE_SHELL, false).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(7, OP_COVER, coverage_pct()));

    let _ = l.register(word_cell(8, OP_CELL, cell(0, 9).map(|v| v as u32)));
    let _ = l.register(word_cell(9, OP_CELL, cell(99, COL_TIME).map(|v| v as u32)));
    let _ = l.register(word_cell(
        10,
        OP_RESOLVE,
        resolve(&[0u8, 12u8], true).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        11,
        OP_RESOLVE,
        resolve(&[3u8], false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(12, OP_LAYOUT, layout(10, 14, 0).map(|v| v.rows)));
    let _ = l.register(word_cell(
        13,
        OP_COLOR,
        resolve_color(9, true, false).map(|v| v as u32),
    ));

    l
}

/// 四路注入：矩阵闸门 / 单元格与裁决 / 选项归并 / 呈现折算。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = ROWS == 26
        && COLS == 4
        && CELLS == 104
        && names_unique()
        && cells_with(SAME) == Ok(80)
        && cells_with(DIVERGES) == Ok(8)
        && cells_with(MISSING) == Ok(16)
        && rows_with_verdict(V_SAME) == Ok(14)
        && rows_with_verdict(V_DIV) == Ok(8)
        && rows_with_verdict(V_MISS) == Ok(4);

    out[1] = cell(3, COL_COLUMNS) == Ok(DIVERGES)
        && cell(9, COL_TIME) == Ok(DIVERGES)
        && cell(22, COL_COLOR) == Ok(MISSING)
        && cell(0, COL_INODE) == Ok(SAME)
        && row_verdict(5) == Ok(V_MISS)
        && row_verdict(3) == Ok(V_DIV)
        && row_verdict(7) == Ok(V_SAME)
        && row_verdict(24) == Ok(V_DIV)
        && row_verdict(26) == Err(LS_EROW)
        && column_gap(0) == Ok(7)
        && column_gap(1) == Ok(9)
        && column_gap(2) == Ok(4)
        && column_gap(3) == Ok(4)
        && column_gap(4) == Err(LS_ECOL);

    let fmt_one = resolve(&[0u8], true) == Ok(FORMAT_ONE);
    let fmt_long = resolve(&[12u8], true) == Ok(FORMAT_LONG);
    let fmt_across = resolve(&[20u8], true) == Ok(FORMAT_ACROSS);
    let fmt_def = resolve(&[], false) == Ok(FORMAT_COLUMNS);
    let ord1 = resolve(&[0u8, 12u8], true) == Err(LS_EORDER);
    let ord2 = resolve(&[3u8, 20u8], true) == Err(LS_EORDER);
    let ord3 = resolve(&[9u8, 19u8], true) == Err(LS_EORDER);
    let ord4 = resolve(&[8u8, 18u8], true) == Err(LS_EORDER);
    let unknown = resolve(&[200u8], true) == Err(LS_EOPT);
    let dup = resolve(&[0u8, 0u8], true) == Err(LS_EMODE);
    let tty = resolve(&[3u8], false) == Err(LS_ETTY) && resolve(&[3u8], true) == Ok(FORMAT_COLUMNS);
    out[2] = fmt_one && fmt_long && fmt_across && fmt_def && ord1 && ord2 && ord3 && ord4
        && unknown
        && dup
        && tty;

    let lay = layout(100, 14, 80);
    let wide = layout(3, 4, 200);
    let zero = layout(10, 14, 0) == Err(LS_EWIDTH);
    let color = resolve_color(COLOR_ALWAYS, false, true) == Ok(true)
        && resolve_color(COLOR_AUTO, true, false) == Ok(true)
        && resolve_color(COLOR_AUTO, true, true) == Ok(false)
        && resolve_color(COLOR_AUTO, false, false) == Ok(false)
        && resolve_color(7, true, false) == Err(LS_ECOLOR);
    let quoted = match render_name("a'b", KIND_DIR, QUOTE_SHELL, true) {
        Ok(s) => s.as_str() == "'a'\\''b'/",
        Err(_) => false,
    };
    let cstyle = match render_name("a\tb", KIND_REG, QUOTE_C, false) {
        Ok(s) => s.as_str() == "a\\x09b",
        Err(_) => false,
    };
    let suffix = classify_suffix(KIND_SOCK) == Ok('=')
        && classify_suffix(KIND_FIFO) == Ok('|')
        && classify_suffix(KIND_REG) == Ok('\0')
        && classify_suffix(9) == Err(LS_EMODE);
    out[3] = lay == Ok(Layout { cols: 5, rows: 20, per_line: 5 })
        && lay.map(|v| v.covers(100)) == Ok(true)
        && wide == Ok(Layout { cols: 33, rows: 1, per_line: 3 })
        && zero
        && color
        && quoted
        && cstyle
        && suffix
        && render_name("", KIND_REG, QUOTE_LITERAL, false) == Err(LS_EROW);

    out
}

/// 十三项判据。
pub fn run_unx11587_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11587.busybox_ls");
    let l = build_ledger();

    s.add(
        "LS1-差异矩阵形状",
        ROWS == 26
            && COLS == 4
            && CELLS == 104
            && MATRIX.len() == ROWS
            && names_unique()
            && l.sample(100).is_none(),
        "static",
    );

    s.add(
        "LS2-分歧缺失三档计数",
        cells_with(SAME) == Ok(80)
            && cells_with(DIVERGES) == Ok(8)
            && cells_with(MISSING) == Ok(16)
            && rows_with_verdict(V_SAME) == Ok(14)
            && rows_with_verdict(V_DIV) == Ok(8)
            && rows_with_verdict(V_MISS) == Ok(4),
        "static",
    );

    s.add(
        "LS3-覆盖率与保真率",
        coverage_pct() == Ok(76)
            && fidelity_pct() == Ok(84)
            && coverage_pct() < fidelity_pct()
            && cells_with(9) == Err(LS_ECOL),
        "static",
    );

    s.add(
        "LS4-单元格定向核对",
        cell(3, COL_COLUMNS) == Ok(DIVERGES)
            && cell(9, COL_TIME) == Ok(DIVERGES)
            && cell(22, COL_COLOR) == Ok(MISSING)
            && cell(0, COL_INODE) == Ok(SAME)
            && cell(0, 4) == Err(LS_ECOL)
            && cell(26, 0) == Err(LS_EROW),
        "static",
    );

    s.add(
        "LS5-行裁决三档聚合",
        row_verdict(5) == Ok(V_MISS)
            && row_verdict(22) == Ok(V_MISS)
            && row_verdict(3) == Ok(V_DIV)
            && row_verdict(24) == Ok(V_DIV)
            && row_verdict(7) == Ok(V_SAME)
            && verdict_label(V_MISS) == Ok("缺失")
            && verdict_label(9) == Err(LS_EORDER),
        "static",
    );

    s.add(
        "LS6-默认列格式次序",
        resolve(&[0u8], true) == Ok(FORMAT_ONE)
            && resolve(&[12u8], true) == Ok(FORMAT_LONG)
            && resolve(&[20u8], true) == Ok(FORMAT_ACROSS)
            && resolve(&[], true) == Ok(FORMAT_COLUMNS)
            && resolve(&[11u8, 16u8], true) == Ok(FORMAT_COLUMNS),
        "static",
    );

    s.add(
        "LS7-互斥选项判次序冲突",
        resolve(&[0u8, 12u8], true) == Err(LS_EORDER)
            && resolve(&[3u8, 20u8], true) == Err(LS_EORDER)
            && resolve(&[9u8, 19u8], true) == Err(LS_EORDER)
            && resolve(&[8u8, 18u8], true) == Err(LS_EORDER),
        "static",
    );

    s.add(
        "LS8-未知与重复选项",
        resolve(&[200u8], true) == Err(LS_EOPT)
            && resolve(&[0u8, 0u8], true) == Err(LS_EMODE)
            && resolve(&[255u8], true) == Err(LS_EOPT)
            && row_of_short(b'G') == Ok(5)
            && row_of_short(0) == Err(LS_EROW)
            && row_of_short(b'z') == Err(LS_EROW),
        "static",
    );

    s.add(
        "LS9-终端态闸门",
        resolve(&[3u8], false) == Err(LS_ETTY)
            && resolve(&[3u8], true) == Ok(FORMAT_COLUMNS)
            && resolve(&[0u8], false) == Ok(FORMAT_ONE),
        "static",
    );

    s.add(
        "LS10-颜色策略三态",
        resolve_color(COLOR_NEVER, true, false) == Ok(false)
            && resolve_color(COLOR_ALWAYS, false, true) == Ok(true)
            && resolve_color(COLOR_AUTO, true, false) == Ok(true)
            && resolve_color(COLOR_AUTO, true, true) == Ok(false)
            && resolve_color(7, true, false) == Err(LS_ECOLOR),
        "static",
    );

    s.add(
        "LS11-列布局折算",
        layout(100, 14, 80) == Ok(Layout { cols: 5, rows: 20, per_line: 5 })
            && layout(3, 4, 200) == Ok(Layout { cols: 33, rows: 1, per_line: 3 })
            && layout(0, 8, 80) == Ok(Layout { cols: 1, rows: 0, per_line: 0 })
            && layout(100, 14, 80).map(|v| v.covers(100)) == Ok(true),
        "static",
    );

    s.add(
        "LS12-宽度与名字护栏",
        layout(10, 14, 0) == Err(LS_EWIDTH)
            && layout(10, 0, 80) == Err(LS_EWIDTH)
            && render_name("", KIND_REG, QUOTE_LITERAL, false) == Err(LS_EROW)
            && render_name("d", 9, QUOTE_LITERAL, false) == Err(LS_EMODE)
            && render_name("d", KIND_REG, 9, false) == Err(LS_EMODE)
            && classify_suffix(KIND_DIR) == Ok('/'),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let one = f.freeze();
    let two = f.freeze();
    let inj = inject_four();
    s.add(
        "LS13-冻结重放四注入",
        one.is_ok()
            && one == rep
            && two.is_err()
            && f.is_frozen()
            && f.root() != 0
            && f.rejects() == 1
            && inj[0]
            && inj[1]
            && inj[2]
            && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(3).map(|s| s.value), Some(2));
        assert_eq!(l.sample(5).map(|s| s.value), Some(20));
        assert_eq!(l.sample(0).map(|s| s.code), Some(0));
    }

    #[test]
    fn matrix_cells_tally() {
        assert_eq!(cells_with(SAME), Ok(80));
        assert_eq!(cells_with(DIVERGES), Ok(8));
        assert_eq!(cells_with(MISSING), Ok(16));
        assert_eq!(coverage_pct(), Ok(76));
        assert_eq!(fidelity_pct(), Ok(84));
    }

    #[test]
    fn row_verdicts() {
        assert_eq!(row_verdict(0), Ok(V_SAME));
        assert_eq!(row_verdict(3), Ok(V_DIV));
        assert_eq!(row_verdict(5), Ok(V_MISS));
        assert_eq!(row_verdict(25), Ok(V_MISS));
        assert_eq!(row_verdict(99), Err(LS_EROW));
        assert_eq!(rows_with_verdict(V_MISS), Ok(4));
    }

    #[test]
    fn cell_bounds() {
        assert_eq!(cell(0, 4), Err(LS_ECOL));
        assert_eq!(cell(26, 0), Err(LS_EROW));
        assert_eq!(cell(11, COL_INODE), Ok(SAME));
        assert_eq!(cell(13, COL_TIME), Ok(DIVERGES));
    }

    #[test]
    fn columns_gap_profile() {
        assert_eq!(column_gap(0), Ok(7));
        assert_eq!(column_gap(1), Ok(9));
        assert_eq!(column_gap(2), Ok(4));
        assert_eq!(column_gap(3), Ok(4));
        assert_eq!(column_gap(4), Err(LS_ECOL));
    }

    #[test]
    fn lookup_by_name_and_short() {
        assert_eq!(row_of_name("no-group"), Ok(5));
        assert_eq!(row_of_name("--color"), Ok(22));
        assert_eq!(row_of_name("--nope"), Err(LS_EROW));
        assert_eq!(row_of_short(b'l'), Ok(12));
        assert_eq!(row_of_short(b'z'), Err(LS_EROW));
        assert_eq!(name_at(24), Ok("--full-time"));
        assert_eq!(short_at(22), Ok(0));
        assert_eq!(name_at(26), Err(LS_EROW));
    }

    #[test]
    fn format_precedence() {
        assert_eq!(resolve(&[0], true), Ok(FORMAT_ONE));
        assert_eq!(resolve(&[12], true), Ok(FORMAT_LONG));
        assert_eq!(resolve(&[20], true), Ok(FORMAT_ACROSS));
        assert_eq!(resolve(&[3], true), Ok(FORMAT_COLUMNS));
        assert_eq!(resolve(&[], false), Ok(FORMAT_COLUMNS));
        assert_eq!(resolve(&[10], true), Ok(FORMAT_COLUMNS));
    }

    #[test]
    fn order_conflicts() {
        assert_eq!(resolve(&[0, 12], true), Err(LS_EORDER));
        assert_eq!(resolve(&[3, 20], true), Err(LS_EORDER));
        assert_eq!(resolve(&[9, 19], true), Err(LS_EORDER));
        assert_eq!(resolve(&[8, 18], true), Err(LS_EORDER));
    }

    #[test]
    fn unknown_and_duplicate_options() {
        assert_eq!(resolve(&[200], true), Err(LS_EOPT));
        assert_eq!(resolve(&[0, 0], true), Err(LS_EMODE));
        assert_eq!(resolve(&[7, 7], true), Err(LS_EMODE));
    }

    #[test]
    fn tty_gate() {
        assert_eq!(resolve(&[3], false), Err(LS_ETTY));
        assert_eq!(resolve(&[3], true), Ok(FORMAT_COLUMNS));
        assert_eq!(resolve(&[0], false), Ok(FORMAT_ONE));
    }

    #[test]
    fn color_policy() {
        assert_eq!(resolve_color(COLOR_NEVER, true, false), Ok(false));
        assert_eq!(resolve_color(COLOR_ALWAYS, false, true), Ok(true));
        assert_eq!(resolve_color(COLOR_AUTO, true, false), Ok(true));
        assert_eq!(resolve_color(COLOR_AUTO, true, true), Ok(false));
        assert_eq!(resolve_color(COLOR_AUTO, false, false), Ok(false));
        assert_eq!(resolve_color(2, false, false), Ok(true));
        assert_eq!(resolve_color(5, true, false), Err(LS_ECOLOR));
    }

    #[test]
    fn layout_rows() {
        assert_eq!(layout(100, 14, 80), Ok(Layout { cols: 5, rows: 20, per_line: 5 }));
        assert_eq!(layout(1, 8, 80), Ok(Layout { cols: 8, rows: 1, per_line: 1 }));
        assert_eq!(layout(3, 4, 200), Ok(Layout { cols: 33, rows: 1, per_line: 3 }));
        assert_eq!(layout(0, 8, 80), Ok(Layout { cols: 1, rows: 0, per_line: 0 }));
        assert_eq!(layout(10, 200, 80), Ok(Layout { cols: 1, rows: 10, per_line: 1 }));
    }

    #[test]
    fn layout_rejects_zero_width() {
        assert_eq!(layout(10, 14, 0), Err(LS_EWIDTH));
        assert_eq!(layout(10, 0, 80), Err(LS_EWIDTH));
    }

    #[test]
    fn naming_and_quoting() {
        assert_eq!(render_name("plain", KIND_REG, QUOTE_LITERAL, false), Ok(String::from("plain")));
        assert_eq!(render_name("sp ace", KIND_REG, QUOTE_SHELL, false), Ok(String::from("'sp ace'")));
        assert_eq!(render_name("a'b", KIND_DIR, QUOTE_SHELL, true), Ok(String::from("'a'\\''b'/")));
        assert_eq!(render_name("a\tb", KIND_REG, QUOTE_C, false), Ok(String::from("a\\x09b")));
        assert_eq!(render_name("plain", KIND_EXE, QUOTE_LITERAL, true), Ok(String::from("plain*")));
        assert_eq!(render_name("", KIND_REG, QUOTE_LITERAL, false), Err(LS_EROW));
        assert_eq!(render_name("x", 8, QUOTE_LITERAL, false), Err(LS_EMODE));
        assert_eq!(classify_suffix(KIND_SOCK), Ok('='));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(LS_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.root(), root);
    }

    #[test]
    fn ledger_rejects_bad_slots() {
        let mut l = Ledger::new();
        let s = Sample { slot: 3, op: OP_CELL, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(s), Err(LS_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_empty());
        let good = Sample { slot: 0, op: OP_CELL, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.register(good), Err(LS_ESLOT));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn injections() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
        assert!(names_unique());
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11587_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
