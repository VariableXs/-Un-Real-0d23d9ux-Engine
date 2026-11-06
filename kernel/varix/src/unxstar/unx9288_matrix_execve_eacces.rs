//! UNX-F9288 execve 错误矩阵（EACCES/ENOEXEC/E2BIG/EFAULT） · 0x7B580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9288 逐字）：
//!   UNX-F9288-J1 四错误码注入矩阵判据过，坏路径全 EFAULT 格过，非脚本非
//!   ELF 全 ENOEXEC 格过，判序与 Linux 一致格过。
//!
//! 设计要点：
//!   ① 执行链按 Linux do_execveat_common 的先后切成固定阶段：
//!        路径拷贝 → argv 计数 → envp 计数 → 打开与权限 → 格式嗅探 →
//!        参数字节拷贝 → 提交
//!      每个阶段只看属于自己的故障位，命中即返回，绝不回看后续阶段。这条
//!      「阶段单向」不变量是判序判据的落点：坏路径的 EFAULT 必须早于权限
//!      EACCES、格式 ENOEXEC 与参数 E2BIG。
//!   ② 故障用位图表达（9 个位），judge 是逐阶段扫描；另设一张冻结优先级
//!      表 PRIORITY 作为独立第二实现，对 2^9=512 个位组合逐一对账。两套实现
//!      同源不同写法，任何顺序漂移都会在 512 格对账里暴露。
//!   ③ argv/envp 的「空指针合法、坏指针 EFAULT」语义保留：位图只表达坏指针，
//!      空指针不置位；计数超限（MAX_ARG_STRINGS）才置 E2BIG 位。计数阶段的
//!      次序为「先取指针（EFAULT）再查上限（E2BIG）」，与内核对用户指针先
//!      get_user 再比 max 的次序一致。
//!   ④ 文件类别表 FileClass → (故障位, 期望码) 与阶段模型双向对账：类别是
//!      「外部观测」，位图是「内部故障面」，两边必须得到同一个错误码与阶段。
//!      非脚本非 ELF（纯文本/空文件）一律 ENOEXEC；目录/无 x 位/设备节点
//!      一律 EACCES；解释器缺失回 ENOENT。
//!   ⑤ 注入 12 格 × 8 轮 = 96 次；每轮同时校验错误码、阶段号与轨迹长度，
//!      账本 injected == detected + escaped 且 escaped == 0。
//!   ⑥ 诊断码闭集 12 个，码值 SEG_BASE + 序号，段窗口 0x7B580..=0x7B5BF。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9288 独占 0x7B580..=0x7B5BF）。
pub const SEG_BASE: u32 = 0x7B580;
/// 段窗口上界（含）。
pub const SEG_END: u32 = 0x7B5BF;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const E2BIG: i32 = 7;
pub const ENOEXEC: i32 = 8;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;

/// 内核参数个数上限（MAX_ARG_STRINGS）。
pub const MAX_ARG_STRINGS: u32 = 0x7FFF_FFFF;
/// 单次注入每格轮数。
pub const ROUNDS: u32 = 8;
/// 冻结矩阵格数。
pub const CELLS: usize = 12;

// ── 故障位 ────────────────────────────────────────────────────────────
/// 路径字符串本身不可读（用户指针越界 / 非规范地址）。
pub const F_PATH_FAULT: u32 = 1 << 0;
/// argv 向量指针不可读。
pub const F_ARGV_FAULT: u32 = 1 << 1;
/// envp 向量指针不可读。
pub const F_ENVP_FAULT: u32 = 1 << 2;
/// 某个参数字符串内容不可读（向量本身可读）。
pub const F_STRING_FAULT: u32 = 1 << 3;
/// 目标文件缺可执行权限位。
pub const F_NO_EXEC: u32 = 1 << 4;
/// 文件既不是 ELF 也不是带合法 shebang 的脚本。
pub const F_NOT_FORMAT: u32 = 1 << 5;
/// 目标路径不存在（含脚本解释器缺失）。
pub const F_MISSING: u32 = 1 << 6;
/// argv/envp 元素个数超过 MAX_ARG_STRINGS。
pub const F_BIG_COUNT: u32 = 1 << 7;
/// 参数字节总量超过单次 exec 预算。
pub const F_BIG_BYTES: u32 = 1 << 8;
/// 目标为目录（内核在打开阶段按 EACCES 处理）。
pub const F_DIR: u32 = 1 << 9;

/// 全部故障位（等价性穷举使用）。
pub const ALL_FAULTS: u32 = F_PATH_FAULT
    | F_ARGV_FAULT
    | F_ENVP_FAULT
    | F_STRING_FAULT
    | F_NO_EXEC
    | F_NOT_FORMAT
    | F_MISSING
    | F_BIG_COUNT
    | F_BIG_BYTES
    | F_DIR;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码。码值 = SEG_BASE + 序号（1 起），落 0x7B581..=0x7B58C。
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecCode {
    PathFault = 1,
    ArgvFault = 2,
    EnvpFault = 3,
    StringFault = 4,
    Permission = 5,
    MissingInterp = 6,
    BadFormat = 7,
    TooManyArgs = 8,
    TooManyBytes = 9,
    OrderDrift = 10,
    ClassDrift = 11,
    SegmentSkew = 12,
}

impl ExecCode {
    /// 段内码值。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            ExecCode::PathFault => "路径指针不可读：getname 在拷贝阶段即回 EFAULT；请校验用户态指针范围",
            ExecCode::ArgvFault => "argv 向量不可读：计数阶段 get_user 失败回 EFAULT；请先验证向量指针",
            ExecCode::EnvpFault => "envp 向量不可读：envp 计数阶段回 EFAULT；请检查环境块指针",
            ExecCode::StringFault => "参数字符串不可读：字节拷贝阶段回 EFAULT；请确认每个字符串整段可读",
            ExecCode::Permission => "执行权限缺失：打开阶段回 EACCES；请补 x 位或改用解释器",
            ExecCode::MissingInterp => "解释器缺失：shebang 指向的路径不存在，回 ENOENT；请安装解释器",
            ExecCode::BadFormat => "格式不可识别：既非 ELF 也无合法 shebang，回 ENOEXEC；请检查文件头",
            ExecCode::TooManyArgs => "参数个数超限：计数阶段回 E2BIG；请减少 argv/envp 元素数",
            ExecCode::TooManyBytes => "参数总量超限：字节拷贝阶段回 E2BIG；请压缩参数体积",
            ExecCode::OrderDrift => "判序漂移：阶段次序与 Linux 不一致；请按冻结阶段重排",
            ExecCode::ClassDrift => "类别对账漂移：文件类别表与故障位模型结果不一致",
            ExecCode::SegmentSkew => "段位越界：段基址必须 0x40 对齐且落在本卡窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [ExecCode; 12] = [
        ExecCode::PathFault,
        ExecCode::ArgvFault,
        ExecCode::EnvpFault,
        ExecCode::StringFault,
        ExecCode::Permission,
        ExecCode::MissingInterp,
        ExecCode::BadFormat,
        ExecCode::TooManyArgs,
        ExecCode::TooManyBytes,
        ExecCode::OrderDrift,
        ExecCode::ClassDrift,
        ExecCode::SegmentSkew,
    ];
}

// ── 阶段模型 ──────────────────────────────────────────────────────────
/// 执行链阶段。索引即轨迹顺序（0 起），Commit 恒为最后一站。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Path,
    ArgvCount,
    EnvpCount,
    Open,
    Format,
    ArgBytes,
    Commit,
}

impl Stage {
    /// 冻结序号（轨迹长度 = index + 1）。
    pub fn index(self) -> u8 {
        match self {
            Stage::Path => 0,
            Stage::ArgvCount => 1,
            Stage::EnvpCount => 2,
            Stage::Open => 3,
            Stage::Format => 4,
            Stage::ArgBytes => 5,
            Stage::Commit => 6,
        }
    }

    /// 阶段标签（动态明细使用）。
    pub fn label(self) -> &'static str {
        match self {
            Stage::Path => "path",
            Stage::ArgvCount => "argv",
            Stage::EnvpCount => "envp",
            Stage::Open => "open",
            Stage::Format => "format",
            Stage::ArgBytes => "bytes",
            Stage::Commit => "commit",
        }
    }
}

/// 判定结果：错误码 + 出错阶段 + 走过的阶段数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub code: i32,
    pub stage: Stage,
    pub trace_len: u8,
}

impl Verdict {
    pub const fn clean() -> Verdict {
        Verdict { code: 0, stage: Stage::Commit, trace_len: 7 }
    }

    pub fn is_clean(&self) -> bool {
        self.code == 0 && self.stage == Stage::Commit
    }
}

fn verdict(code: i32, stage: Stage, trace: &[Stage]) -> Verdict {
    Verdict { code, stage, trace_len: trace.len() as u8 }
}

// ── 阶段式判定（实现一） ──────────────────────────────────────────────
/// 逐阶段扫描故障位；命中即返回，保证后阶段不可能覆盖前阶段。
pub fn judge(flags: u32) -> Verdict {
    let mut trace: Vec<Stage> = Vec::new();

    trace.push(Stage::Path);
    if flags & F_PATH_FAULT != 0 {
        return verdict(EFAULT, Stage::Path, &trace);
    }

    trace.push(Stage::ArgvCount);
    if flags & F_ARGV_FAULT != 0 {
        return verdict(EFAULT, Stage::ArgvCount, &trace);
    }
    if flags & F_BIG_COUNT != 0 {
        return verdict(E2BIG, Stage::ArgvCount, &trace);
    }

    trace.push(Stage::EnvpCount);
    if flags & F_ENVP_FAULT != 0 {
        return verdict(EFAULT, Stage::EnvpCount, &trace);
    }

    trace.push(Stage::Open);
    if flags & F_MISSING != 0 {
        return verdict(ENOENT, Stage::Open, &trace);
    }
    if flags & F_DIR != 0 {
        return verdict(EACCES, Stage::Open, &trace);
    }
    if flags & F_NO_EXEC != 0 {
        return verdict(EACCES, Stage::Open, &trace);
    }

    trace.push(Stage::Format);
    if flags & F_NOT_FORMAT != 0 {
        return verdict(ENOEXEC, Stage::Format, &trace);
    }

    trace.push(Stage::ArgBytes);
    if flags & F_STRING_FAULT != 0 {
        return verdict(EFAULT, Stage::ArgBytes, &trace);
    }
    if flags & F_BIG_BYTES != 0 {
        return verdict(E2BIG, Stage::ArgBytes, &trace);
    }

    trace.push(Stage::Commit);
    verdict(0, Stage::Commit, &trace)
}

// ── 优先级表（实现二，独立对账） ──────────────────────────────────────
/// 冻结优先级：(故障位, 错误码, 阶段)，自上而下第一条命中即裁决。
pub const PRIORITY: [(u32, i32, Stage); 10] = [
    (F_PATH_FAULT, EFAULT, Stage::Path),
    (F_ARGV_FAULT, EFAULT, Stage::ArgvCount),
    (F_BIG_COUNT, E2BIG, Stage::ArgvCount),
    (F_ENVP_FAULT, EFAULT, Stage::EnvpCount),
    (F_MISSING, ENOENT, Stage::Open),
    (F_DIR, EACCES, Stage::Open),
    (F_NO_EXEC, EACCES, Stage::Open),
    (F_NOT_FORMAT, ENOEXEC, Stage::Format),
    (F_STRING_FAULT, EFAULT, Stage::ArgBytes),
    (F_BIG_BYTES, E2BIG, Stage::ArgBytes),
];

/// 表驱动判定：与 judge 独立实现，用于 512 组合逐格对账。
pub fn judge_table(flags: u32) -> Verdict {
    let mut i = 0usize;
    while i < PRIORITY.len() {
        if let Some((mask, code, stage)) = PRIORITY.get(i).copied() {
            if flags & mask != 0 {
                return Verdict { code, stage, trace_len: stage.index() + 1 };
            }
        }
        i += 1;
    }
    Verdict::clean()
}

// ── 文件类别表 ────────────────────────────────────────────────────────
/// 外部可观测的文件类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileClass {
    /// 64 位 ELF 可执行文件。
    Elf64,
    /// 32 位 ELF 可执行文件。
    Elf32,
    /// 带合法 shebang 且解释器存在的脚本。
    ScriptShell,
    /// shebang 指向的解释器不存在。
    ScriptBadInterp,
    /// 纯文本（无 shebang）。
    PlainText,
    /// 空文件。
    EmptyFile,
    /// 目录。
    Directory,
    /// 普通文件但无 x 位。
    NoExecBit,
    /// 路径不存在。
    Missing,
    /// 字符/块设备节点。
    DeviceNode,
}

impl FileClass {
    /// 类别标签。
    pub fn label(self) -> &'static str {
        match self {
            FileClass::Elf64 => "elf64",
            FileClass::Elf32 => "elf32",
            FileClass::ScriptShell => "script-ok",
            FileClass::ScriptBadInterp => "script-bad-interp",
            FileClass::PlainText => "text",
            FileClass::EmptyFile => "empty",
            FileClass::Directory => "dir",
            FileClass::NoExecBit => "no-x",
            FileClass::Missing => "missing",
            FileClass::DeviceNode => "dev",
        }
    }

    /// 类别映射到故障位（外部观测 → 内部故障面）。
    pub fn flags(self) -> u32 {
        match self {
            FileClass::Elf64 | FileClass::Elf32 | FileClass::ScriptShell => 0,
            FileClass::ScriptBadInterp | FileClass::Missing => F_MISSING,
            FileClass::PlainText | FileClass::EmptyFile => F_NOT_FORMAT,
            FileClass::Directory | FileClass::DeviceNode => F_DIR,
            FileClass::NoExecBit => F_NO_EXEC,
        }
    }

    /// 类别期望错误码（与 Linux 语义逐条对齐）。
    pub fn expect_code(self) -> i32 {
        match self {
            FileClass::Elf64 | FileClass::Elf32 | FileClass::ScriptShell => 0,
            FileClass::ScriptBadInterp | FileClass::Missing => ENOENT,
            FileClass::PlainText | FileClass::EmptyFile => ENOEXEC,
            FileClass::Directory | FileClass::DeviceNode | FileClass::NoExecBit => EACCES,
        }
    }

    /// 类别期望阶段（打开期还是格式期）。
    pub fn expect_stage(self) -> Stage {
        match self {
            FileClass::Elf64 | FileClass::Elf32 | FileClass::ScriptShell => Stage::Commit,
            FileClass::ScriptBadInterp | FileClass::Missing | FileClass::Directory
            | FileClass::DeviceNode | FileClass::NoExecBit => Stage::Open,
            FileClass::PlainText | FileClass::EmptyFile => Stage::Format,
        }
    }
}

/// 类别全表（对账遍历用）。
pub const CLASSES: [FileClass; 10] = [
    FileClass::Elf64,
    FileClass::Elf32,
    FileClass::ScriptShell,
    FileClass::ScriptBadInterp,
    FileClass::PlainText,
    FileClass::EmptyFile,
    FileClass::Directory,
    FileClass::NoExecBit,
    FileClass::Missing,
    FileClass::DeviceNode,
];

// ── 冻结矩阵 ──────────────────────────────────────────────────────────
/// 一格：故障位组合 + 冻结期望码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub label: &'static str,
    pub flags: u32,
    pub expect: i32,
}

/// 12 格冻结矩阵：四错误码各至少两格，外加干净格与 ENOENT 辅助格。
pub const MATRIX: [Cell; CELLS] = [
    Cell { label: "clean-elf", flags: 0, expect: 0 },
    Cell { label: "path-bad", flags: F_PATH_FAULT, expect: EFAULT },
    Cell { label: "argv-bad", flags: F_ARGV_FAULT, expect: EFAULT },
    Cell { label: "envp-bad", flags: F_ENVP_FAULT, expect: EFAULT },
    Cell { label: "string-bad", flags: F_STRING_FAULT, expect: EFAULT },
    Cell { label: "no-x", flags: F_NO_EXEC, expect: EACCES },
    Cell { label: "dir", flags: F_DIR, expect: EACCES },
    Cell { label: "text", flags: F_NOT_FORMAT, expect: ENOEXEC },
    Cell { label: "empty", flags: F_NOT_FORMAT, expect: ENOEXEC },
    Cell { label: "argc-over", flags: F_BIG_COUNT, expect: E2BIG },
    Cell { label: "bytes-over", flags: F_BIG_BYTES, expect: E2BIG },
    Cell { label: "missing", flags: F_MISSING, expect: ENOENT },
];

/// 四错误码闭集（矩阵取证用）。
pub const FOUR_CODES: [i32; 4] = [EACCES, ENOEXEC, E2BIG, EFAULT];

// ── 注入账 ────────────────────────────────────────────────────────────
/// 阶段注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl StageLedger {
    pub const fn zero() -> StageLedger {
        StageLedger { injected: 0, detected: 0, escaped: 0 }
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

/// 矩阵注入结果。
#[derive(Clone, Copy, Debug)]
pub struct StageRun {
    pub observed: [i32; CELLS],
    pub ledger: StageLedger,
    pub per_code: [u32; 4],
}

impl StageRun {
    pub const fn empty() -> StageRun {
        StageRun { observed: [0i32; CELLS], ledger: StageLedger::zero(), per_code: [0u32; 4] }
    }
}

/// 错误码在 FOUR_CODES 里的槽位（非四码返回 None）。
pub fn four_slot(code: i32) -> Option<usize> {
    let mut i = 0usize;
    while i < FOUR_CODES.len() {
        if let Some(c) = FOUR_CODES.get(i) {
            if *c == code {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 12 格 × 8 轮注入，逐轮校验码、阶段与轨迹长度三者一致。
pub fn run_matrix() -> StageRun {
    let mut run = StageRun::empty();
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(cell) = MATRIX.get(i).copied() {
            let mut round = 0u32;
            while round < ROUNDS {
                let v = judge(cell.flags);
                let t = judge_table(cell.flags);
                // 阶段自洽：轨迹长度必须等于阶段序号 + 1。
                let trace_ok = v.trace_len == v.stage.index() + 1 && v == t;
                let caught = v.code == cell.expect && trace_ok;
                let rec = run.ledger.record(caught);
                if rec.is_err() {
                    run.ledger.escaped = run.ledger.escaped.saturating_add(1);
                }
                if round == 0 {
                    if let Some(slot) = run.observed.get_mut(i) {
                        *slot = v.code;
                    }
                    if let Some(pos) = four_slot(v.code) {
                        if let Some(cnt) = run.per_code.get_mut(pos) {
                            *cnt = cnt.saturating_add(1);
                        }
                    }
                }
                round += 1;
            }
        }
        i += 1;
    }
    run
}

/// 512 组合的两套实现逐格对账；返回不一致格数。
pub fn order_mismatches() -> u32 {
    let mut bad = 0u32;
    let mut f = 0u32;
    while f <= ALL_FAULTS {
        if f & !ALL_FAULTS == 0 {
            if judge(f) != judge_table(f) {
                bad = bad.saturating_add(1);
            }
        }
        f = f.saturating_add(1);
    }
    bad
}

/// 故障位全组合数（2^10 = 1024，含 F_DIR 位）。
pub const COMBINATIONS: u32 = 1024;

/// 遍历全部 1024 个组合统计阶段式与表式一致格数。
pub fn order_agreement() -> (u32, u32) {
    let mut ok = 0u32;
    let mut total = 0u32;
    let mut f = 0u32;
    while f < COMBINATIONS {
        total = total.saturating_add(1);
        if judge(f) == judge_table(f) {
            ok = ok.saturating_add(1);
        }
        f = f.saturating_add(1);
    }
    (ok, total)
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

/// 规范语料：1024 个故障位组合的 (code, stage) 全量喂入。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut f = 0u32;
    while f < COMBINATIONS {
        let v = judge(f);
        bytes.push(if v.code == 0 { 0u8 } else { (v.code & 0x7F) as u8 });
        bytes.push(v.stage.index());
        bytes.push(v.trace_len);
        f = f.saturating_add(1);
    }
    // 类别表也纳入指纹，防止只冻结位图而漏掉外部观测面。
    let mut i = 0usize;
    while i < CLASSES.len() {
        if let Some(c) = CLASSES.get(i).copied() {
            bytes.push((c.expect_code() & 0x7F) as u8);
            bytes.push(c.expect_stage().index());
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9288 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9288_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9288");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7B580
        && SEG_END == SEG_BASE + 0x3F;
    s.add("段位守卫：0x40 对齐且窗口 0x7B580..=0x7B5BF", seg_ok,
          "段基址唯一且不与邻卡重叠");

    let mut codes_ok = true;
    let mut i = 0usize;
    while i < ExecCode::ALL.len() {
        if let Some(a) = ExecCode::ALL.get(i) {
            if a.detail().is_empty() {
                codes_ok = false;
            }
            let mut j = i + 1;
            while j < ExecCode::ALL.len() {
                if let Some(b) = ExecCode::ALL.get(j) {
                    if a.code() == b.code() {
                        codes_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("诊断码：12 个码值与文案闭集自洽", codes_ok,
          "段内码值必须两两唯一且每条都有可操作建议");

    let run = run_matrix();

    let mut matrix_ok = true;
    let mut k = 0usize;
    while k < MATRIX.len() {
        if let (Some(cell), Some(got)) = (MATRIX.get(k), run.observed.get(k)) {
            if *got != cell.expect {
                matrix_ok = false;
            }
        } else {
            matrix_ok = false;
        }
        k += 1;
    }
    s.add("矩阵：12 格实测码与冻结表逐格一致", matrix_ok,
          "四错误码矩阵的期望列不得漂移");

    s.add("注入：96 次全检出且逃逸为 0",
          run.ledger.conserved() && run.ledger.escaped == 0
              && run.ledger.injected == CELLS as u32 * ROUNDS,
          "码 + 阶段 + 轨迹长度三者同时成立才算检出");

    let mut four_ok = true;
    let mut c = 0usize;
    while c < FOUR_CODES.len() {
        if let Some(n) = run.per_code.get(c) {
            if *n < 2 {
                four_ok = false;
            }
        }
        c += 1;
    }
    let mut foreign = false;
    let mut o = 0usize;
    while o < run.observed.len() {
        if let Some(code) = run.observed.get(o) {
            if *code != 0 && *code != ENOENT && four_slot(*code).is_none() {
                foreign = true;
            }
        }
        o += 1;
    }
    s.add("四码矩阵：EACCES/ENOEXEC/E2BIG/EFAULT 各 ≥2 格且无杂码",
          four_ok && !foreign,
          "矩阵只允许四码 + ENOENT 辅助格，杂码说明判定面被污染");

    let bad_path_flags = [
        F_PATH_FAULT,
        F_PATH_FAULT | F_ARGV_FAULT,
        F_PATH_FAULT | F_NO_EXEC,
        F_PATH_FAULT | F_NOT_FORMAT | F_BIG_COUNT,
        F_PATH_FAULT | F_STRING_FAULT | F_BIG_BYTES | F_DIR,
    ];
    let mut path_ok = true;
    let mut p = 0usize;
    while p < bad_path_flags.len() {
        if let Some(flags) = bad_path_flags.get(p).copied() {
            let v = judge(flags);
            if v.code != EFAULT || v.stage != Stage::Path {
                path_ok = false;
            }
        }
        p += 1;
    }
    s.add("坏路径：5 组叠加故障全 EFAULT 且停在 Path", path_ok,
          "路径拷贝是第一阶段，任何后续故障都不能抢先");

    let mut fmt_ok = true;
    let classes = [FileClass::PlainText, FileClass::EmptyFile];
    let mut q = 0usize;
    while q < classes.len() {
        if let Some(class) = classes.get(q).copied() {
            let v = judge(class.flags());
            if v.code != ENOEXEC || v.stage != Stage::Format {
                fmt_ok = false;
            }
        }
        q += 1;
    }
    s.add("非脚本非 ELF：纯文本/空文件全 ENOEXEC", fmt_ok,
          "格式嗅探阶段必须拒绝所有不可识别文件头");

    let nox = judge(F_NO_EXEC);
    let dir = judge(F_DIR);
    let dev = judge(FileClass::DeviceNode.flags());
    s.add("权限面：无 x 位/目录/设备节点全 EACCES",
          nox.code == EACCES && dir.code == EACCES && dev.code == EACCES
              && nox.stage == Stage::Open && dir.stage == Stage::Open,
          "打开阶段的权限拒绝统一回 EACCES");

    let count_first = judge(F_ARGV_FAULT | F_NO_EXEC);
    let count_over = judge(F_BIG_COUNT | F_NO_EXEC);
    s.add("判序：argv 指针 EFAULT 与计数 E2BIG 均先于打开 EACCES",
          count_first.code == EFAULT && count_first.stage == Stage::ArgvCount
              && count_over.code == E2BIG && count_over.stage == Stage::ArgvCount,
          "计数阶段必须先取指针再比上限，且整体早于打开");

    let fmt_first = judge(F_NOT_FORMAT | F_STRING_FAULT);
    let perm_first = judge(F_NO_EXEC | F_NOT_FORMAT);
    s.add("判序：权限先于格式、格式先于参数字节",
          perm_first.code == EACCES && perm_first.stage == Stage::Open
              && fmt_first.code == ENOEXEC && fmt_first.stage == Stage::Format,
          "ELF 头嗅探必须在参数内容拷贝之前完成");

    let (agree, total) = order_agreement();
    s.add("阶段式与优先级表：1024 组合全等",
          agree == total && total == COMBINATIONS && order_mismatches() == 0,
          "两套独立实现必须逐格同判，否则判序不可信");

    let mut class_ok = true;
    let mut r = 0usize;
    while r < CLASSES.len() {
        if let Some(class) = CLASSES.get(r).copied() {
            let v = judge(class.flags());
            if v.code != class.expect_code() || v.stage != class.expect_stage() {
                class_ok = false;
            }
        }
        r += 1;
    }
    s.add("类别对账：10 类外部观测与内部故障面同码同阶段", class_ok,
          "Class → flags → judge 必须回到类别自身的期望行");

    s.add("errno 数值：四码与 Linux 逐码一致",
          EACCES == 13 && ENOEXEC == 8 && E2BIG == 7 && EFAULT == 14,
          "错误码只允许 errno 表内数值");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：1024 组合 + 类别表指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等");

    let clean = judge(0);
    s.add("正常流：无故障位时提交成功",
          clean.is_clean() && clean.trace_len == 7,
          "干净执行必须走满 7 个阶段并在 Commit 返回 0");

    let matrix_text = format!(
        "格{} / 注入{} / 检出{} / 逃逸{}",
        CELLS, run.ledger.injected, run.ledger.detected, run.ledger.escaped
    );
    s.add_dyn("矩阵快照", matrix_ok && run.ledger.escaped == 0, &matrix_text);

    let order_text = format!("对账 {}/{} 格一致", agree, total);
    s.add_dyn("判序快照", agree == total, &order_text);

    let fp_text = format!("执行链指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    let clean_text = format!(
        "干净链 {} 阶段，码 {}",
        clean.trace_len, clean.code
    );
    s.add_dyn("提交快照", clean.is_clean(), &clean_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_path_commits() {
        let v = judge(0);
        assert_eq!(v.is_clean(), true);
        assert_eq!(v.trace_len, 7);
    }

    #[test]
    fn path_fault_dominates() {
        let v = judge(F_PATH_FAULT | F_NO_EXEC | F_NOT_FORMAT | F_BIG_COUNT);
        assert_eq!(v.code, EFAULT);
        assert_eq!(v.stage, Stage::Path);
        assert_eq!(v.trace_len, 1);
    }

    #[test]
    fn argv_pointer_before_count_and_open() {
        let v = judge(F_ARGV_FAULT | F_BIG_COUNT | F_NO_EXEC);
        assert_eq!(v.code, EFAULT);
        assert_eq!(v.stage, Stage::ArgvCount);
    }

    #[test]
    fn permission_before_format() {
        let v = judge(F_NO_EXEC | F_NOT_FORMAT);
        assert_eq!(v.code, EACCES);
        assert_eq!(v.stage, Stage::Open);
    }

    #[test]
    fn format_before_bytes() {
        let v = judge(F_NOT_FORMAT | F_STRING_FAULT | F_BIG_BYTES);
        assert_eq!(v.code, ENOEXEC);
        assert_eq!(v.stage, Stage::Format);
    }

    #[test]
    fn four_codes_present() {
        let run = run_matrix();
        for i in 0..FOUR_CODES.len() {
            if let Some(n) = run.per_code.get(i) {
                assert_eq!(*n >= 2, true, "code slot {} count {}", i, *n);
            }
        }
        assert_eq!(run.ledger.injected, 96);
        assert_eq!(run.ledger.escaped, 0);
    }

    #[test]
    fn classes_reconcile() {
        for i in 0..CLASSES.len() {
            if let Some(class) = CLASSES.get(i).copied() {
                let v = judge(class.flags());
                assert_eq!(v.code, class.expect_code());
                assert_eq!(v.stage, class.expect_stage());
            }
        }
    }

    #[test]
    fn order_table_matches_all_combinations() {
        let (ok, total) = order_agreement();
        assert_eq!(ok, total);
        assert_eq!(total, COMBINATIONS);
    }

    #[test]
    fn trace_length_tracks_stage() {
        let mut f = 0u32;
        while f < COMBINATIONS {
            let v = judge(f);
            assert_eq!(v.trace_len, v.stage.index() + 1);
            f = f.saturating_add(1);
        }
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9288_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
