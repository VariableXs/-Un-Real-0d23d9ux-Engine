//! UNX-F11617 execve 错误码族对照矩阵（ENOEXEC/EACCES/ENOENT/ENOMEM）
//!
//! 镜面：`execve` 的失败不是一句「打不开」——内核把触发条件折进四个
//! errno 族：找不到件是 ENOENT，权限不许是 EACCES，形不成映像/解释器
//! 坏了是 ENOEXEC，装不下是 ENOMEM。本单把「触发件 ↔ 码」做成一张
//! 可逐槽复核的矩阵。
//!
//! 触发件共十四种，各自归属唯一族：
//!
//! 1. 缺失族（ENOENT=2）：件不存在、解释器不存在、解释器路径断链；
//! 2. 权限族（EACCES=13）：无 X 位、目标是目录、挂载点 noexec；
//! 3. 格式族（ENOEXEC=8）：魔数坏、脚本无 shebang、空件、解释器格式坏；
//! 4. 内存族（ENOMEM=12）：地址空间映射失败、参数区装不下、栈增长失败、
//!    锁定内存不足。
//!
//! 每一槽给一个「声称码」。声称与族一致记活，错记拒收；拒收理由里必须
//! 同时写出应然码与实称码，矩阵按族统计「声称正确数 / 总触发数」。
//!
//! 语料十四槽，八活六死——缺失三、权限三、格式四、内存四。
//!
//! 闸门次序：名称 → 码族常数 → 触发映射 → 判面 → 矩阵统计 → 语料 →
//! 汇总 → 红单 → 文案 → 校验器 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfc00;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 族数。
pub const FAMILIES: usize = 4;

/// ENOENT。
pub const ERR_ENOENT: i32 = 2;

/// EACCES。
pub const ERR_EACCES: i32 = 13;

/// ENOEXEC。
pub const ERR_ENOEXEC: i32 = 8;

/// ENOMEM。
pub const ERR_ENOMEM: i32 = 12;

/// EINVAL。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 缺失族。
pub const EG_MISSING: u32 = SEG_BASE + 0x01;
/// 权限族。
pub const EG_PERM: u32 = SEG_BASE + 0x02;
/// 格式族。
pub const EG_FORMAT: u32 = SEG_BASE + 0x03;
/// 内存族。
pub const EG_MEMORY: u32 = SEG_BASE + 0x04;
/// 声称错。
pub const EG_WRONG: u32 = SEG_BASE + 0x05;
/// 矩阵不平。
pub const EG_MATRIX: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const EG_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const EG_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const EG_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const EG_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const EG_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const EG_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const EG_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 映射。
pub const OP_MAP: u32 = 1;
/// 判面。
pub const OP_JUDGE: u32 = 2;
/// 矩阵。
pub const OP_MATRIX: u32 = 3;
/// 文案。
pub const OP_TEXT: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;

// ---------------------------------------------------------------------------
// 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折一个字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字。
pub const fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 十六进制文本（带 `0x`，小写）。
pub fn hex_text(v: u64) -> String {
    let mut buf = [0u8; 16];
    let mut n = v;
    let mut i = 16usize;
    if n == 0 {
        return String::from("0x0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        let d = (n & 0xf) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        n >>= 4;
    }
    let mut s = String::from("0x");
    while i < 16 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 错误码族
// ---------------------------------------------------------------------------

/// 四族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 缺失。
    Missing,
    /// 权限。
    Perm,
    /// 格式。
    Format,
    /// 内存。
    Memory,
}

impl Family {
    /// 族名。
    pub const fn name(&self) -> &'static str {
        match self {
            Family::Missing => "enoent",
            Family::Perm => "eacces",
            Family::Format => "enoexec",
            Family::Memory => "enomem",
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Family::Missing => ERR_ENOENT,
            Family::Perm => ERR_EACCES,
            Family::Format => ERR_ENOEXEC,
            Family::Memory => ERR_ENOMEM,
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Family::Missing => EG_MISSING,
            Family::Perm => EG_PERM,
            Family::Format => EG_FORMAT,
            Family::Memory => EG_MEMORY,
        }
    }

    /// 序号。
    pub const fn index(&self) -> usize {
        match self {
            Family::Missing => 0,
            Family::Perm => 1,
            Family::Format => 2,
            Family::Memory => 3,
        }
    }

    /// 由序号取族。
    pub const fn of(i: usize) -> Option<Family> {
        match i {
            0 => Some(Family::Missing),
            1 => Some(Family::Perm),
            2 => Some(Family::Format),
            3 => Some(Family::Memory),
            _ => None,
        }
    }

    /// 从 errno 反查。
    pub const fn from_errno(e: i32) -> Option<Family> {
        match e {
            ERR_ENOENT => Some(Family::Missing),
            ERR_EACCES => Some(Family::Perm),
            ERR_ENOEXEC => Some(Family::Format),
            ERR_ENOMEM => Some(Family::Memory),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 触发件
// ---------------------------------------------------------------------------

/// 十四种触发件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// 件不存在。
    NoFile,
    /// 解释器不存在。
    InterpMissing,
    /// shebang 指向断链。
    ShebangMissing,
    /// 无 X 位。
    NoXbit,
    /// 目标是目录。
    DirTarget,
    /// 挂载点 noexec。
    NoexecMount,
    /// 魔数坏。
    BadMagic,
    /// 脚本无 shebang。
    BadScript,
    /// 空件。
    EmptyFile,
    /// 地址空间映射失败。
    OomMap,
    /// 参数区装不下。
    OomArgv,
    /// 栈增长失败。
    StackGrow,
    /// 解释器格式坏。
    BadInterp,
    /// 锁定内存不足。
    LockedMem,
}

impl Trigger {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Trigger::NoFile => "no-file",
            Trigger::InterpMissing => "interp-missing",
            Trigger::ShebangMissing => "shebang-missing",
            Trigger::NoXbit => "no-xbit",
            Trigger::DirTarget => "dir-target",
            Trigger::NoexecMount => "noexec-mount",
            Trigger::BadMagic => "bad-magic",
            Trigger::BadScript => "bad-script",
            Trigger::EmptyFile => "empty-file",
            Trigger::OomMap => "oom-map",
            Trigger::OomArgv => "oom-argv",
            Trigger::StackGrow => "stack-grow",
            Trigger::BadInterp => "bad-interp",
            Trigger::LockedMem => "locked-mem",
        }
    }

    /// 应然族。
    pub const fn family(&self) -> Family {
        match self {
            Trigger::NoFile | Trigger::InterpMissing | Trigger::ShebangMissing => Family::Missing,
            Trigger::NoXbit | Trigger::DirTarget | Trigger::NoexecMount => Family::Perm,
            Trigger::BadMagic | Trigger::BadScript | Trigger::EmptyFile | Trigger::BadInterp => {
                Family::Format
            }
            Trigger::OomMap | Trigger::OomArgv | Trigger::StackGrow | Trigger::LockedMem => {
                Family::Memory
            }
        }
    }

    /// 应然 errno。
    pub const fn errno(&self) -> i32 {
        self.family().errno()
    }

    /// 应然码。
    pub const fn code(&self) -> u32 {
        self.family().code()
    }
}

/// 十四触发全表。
pub const TRIGGERS: [Trigger; 14] = [
    Trigger::NoFile,
    Trigger::InterpMissing,
    Trigger::ShebangMissing,
    Trigger::NoXbit,
    Trigger::DirTarget,
    Trigger::NoexecMount,
    Trigger::BadMagic,
    Trigger::BadScript,
    Trigger::EmptyFile,
    Trigger::OomMap,
    Trigger::OomArgv,
    Trigger::StackGrow,
    Trigger::BadInterp,
    Trigger::LockedMem,
];

// ---------------------------------------------------------------------------
// 判面
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 声称对。
    Ok,
    /// 声称错。
    Wrong,
    /// 矩阵不平。
    Matrix,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Wrong => "wrong",
            Fault::Matrix => "matrix",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Wrong => EG_WRONG,
            Fault::Matrix => EG_MATRIX,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            Fault::Wrong => ERR_EINVAL,
            Fault::Matrix => ERR_EINVAL,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 一槽：触发件 + 声称族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 触发件。
    pub trigger: Trigger,
    /// 声称族。
    pub claim: Family,
}

impl Case {
    /// 构造。
    pub const fn new(trigger: Trigger, claim: Family) -> Case {
        Case { trigger, claim }
    }

    /// 判面。
    pub const fn judge(&self) -> Fault {
        if self.claim.errno() == self.trigger.errno() {
            Fault::Ok
        } else {
            Fault::Wrong
        }
    }

    /// 应然码。
    pub const fn expected(&self) -> u32 {
        self.trigger.code()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.trigger as u64);
        h = fold_word(h, self.claim.index() as u64);
        h
    }
}

/// 判一槽。
pub fn judge(c: &Case) -> Fault {
    c.judge()
}

/// 判一槽并给出应然 errno。
pub fn judge_errno(c: &Case) -> Result<i32, i32> {
    match c.judge() {
        Fault::Ok => Ok(c.trigger.errno()),
        _ => Err(c.trigger.errno()),
    }
}

// ---------------------------------------------------------------------------
// 矩阵
// ---------------------------------------------------------------------------

/// 一族一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 总触发数。
    pub total: u32,
    /// 声称正确数。
    pub good: u32,
}

impl Row {
    /// 空行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 计一槽。
    pub fn tally(&mut self, ok: bool) {
        self.total += 1;
        if ok {
            self.good += 1;
        }
    }

    /// 是否全对。
    pub const fn perfect(&self) -> bool {
        self.total == self.good
    }
}

/// 四族矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行表。
    pub rows: [Row; FAMILIES],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); FAMILIES] }
    }

    /// 计一槽。
    pub fn tally(&mut self, c: &Case) {
        let i = c.trigger.family().index();
        let ok = c.judge() == Fault::Ok;
        if let Some(r) = self.rows.get_mut(i) {
            r.tally(ok);
        }
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 总格数。
    pub fn total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FAMILIES {
            if let Some(r) = self.rows.get(i) {
                n += r.total;
            }
            i += 1;
        }
        n
    }

    /// 正确格数。
    pub fn good(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FAMILIES {
            if let Some(r) = self.rows.get(i) {
                n += r.good;
            }
            i += 1;
        }
        n
    }

    /// 是否平衡：总格 14、每族非空。
    pub const fn balanced(&self) -> bool {
        let a = self.rows[0].total > 0;
        let b = self.rows[1].total > 0;
        let c = self.rows[2].total > 0;
        let d = self.rows[3].total > 0;
        a && b && c && d
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < FAMILIES {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.total as u64);
                h = fold_word(h, r.good as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一槽登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 案例。
    pub case: Case,
    /// 期望判面。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(case: Case, fault: Fault, note: &'static str) -> Slot {
        Slot { case, fault, note }
    }
}

/// 十四槽语料：八活六死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Case::new(Trigger::NoFile, Family::Missing), Fault::Ok, "missing-file"),
    Slot::new(Case::new(Trigger::InterpMissing, Family::Missing), Fault::Ok, "interp-missing"),
    Slot::new(Case::new(Trigger::ShebangMissing, Family::Missing), Fault::Ok, "shebang-dangling"),
    Slot::new(Case::new(Trigger::NoXbit, Family::Perm), Fault::Ok, "no-xbit"),
    Slot::new(Case::new(Trigger::DirTarget, Family::Perm), Fault::Ok, "directory-target"),
    Slot::new(Case::new(Trigger::NoexecMount, Family::Perm), Fault::Ok, "noexec-mount"),
    Slot::new(Case::new(Trigger::BadMagic, Family::Format), Fault::Ok, "bad-magic"),
    Slot::new(Case::new(Trigger::OomMap, Family::Memory), Fault::Ok, "oom-map"),
    Slot::new(Case::new(Trigger::BadScript, Family::Perm), Fault::Wrong, "script-claimed-perm"),
    Slot::new(Case::new(Trigger::EmptyFile, Family::Memory), Fault::Wrong, "empty-claimed-memory"),
    Slot::new(Case::new(Trigger::OomArgv, Family::Missing), Fault::Wrong, "argv-claimed-missing"),
    Slot::new(Case::new(Trigger::StackGrow, Family::Format), Fault::Wrong, "stack-claimed-format"),
    Slot::new(Case::new(Trigger::BadInterp, Family::Missing), Fault::Wrong, "interp-claimed-missing"),
    Slot::new(Case::new(Trigger::LockedMem, Family::Perm), Fault::Wrong, "locked-claimed-perm"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料互异。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            let b = match CASES.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.note == b.note || (a.case.trigger == b.case.trigger && a.case.claim == b.case.claim)
            {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(v) => {
                if v.note.is_empty() || v.case.trigger.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐槽对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let s = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.case.judge() != s.fault {
            return false;
        }
        if s.fault == Fault::Ok && s.case.expected() != s.case.claim.code() {
            return false;
        }
        i += 1;
    }
    true
}

/// 语料矩阵。
pub fn matrix() -> Matrix {
    let mut m = Matrix::blank();
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(s) = CASES.get(i) {
            m.tally(&s.case);
        }
        i += 1;
    }
    m
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总数。
    pub total: u32,
    /// 声称对。
    pub accept: u32,
    /// 声称错。
    pub reject: u32,
    /// 缺失族。
    pub missing: u32,
    /// 权限族。
    pub perm: u32,
    /// 格式族。
    pub format: u32,
    /// 内存族。
    pub memory: u32,
    /// 命中数。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, missing: 0, perm: 0, format: 0, memory: 0, matched: 0 }
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 命中率（千分比，四舍五入）。
    pub const fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub const fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub const fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(EG_ENOBS);
        }
        if self.matched != self.total {
            return Err(EG_EVER);
        }
        Ok(())
    }

    /// 计一槽。
    pub fn tally(&mut self, c: &Case) {
        self.total += 1;
        match c.trigger.family() {
            Family::Missing => self.missing += 1,
            Family::Perm => self.perm += 1,
            Family::Format => self.format += 1,
            Family::Memory => self.memory += 1,
        }
        if c.judge() == Fault::Ok {
            self.accept += 1;
        } else {
            self.reject += 1;
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("errcode corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" enoent=");
        s.push_str(&u32_text(self.missing));
        s.push_str(" eacces=");
        s.push_str(&u32_text(self.perm));
        s.push_str(" enoexec=");
        s.push_str(&u32_text(self.format));
        s.push_str(" enomem=");
        s.push_str(&u32_text(self.memory));
        s.push_str(" match=");
        s.push_str(&u32_text(self.matched));
        s.push_str(" rate=");
        s.push_str(&u32_text(self.rate()));
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带红档覆盖的全跑。
pub fn run_with(over: Option<(usize, u32)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(EG_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(EG_ESLOT),
        };
        r.tally(&slot.case);
        let overridden = match over {
            Some((k, _)) => k == i,
            None => false,
        };
        if slot.case.judge() == slot.fault && !overridden {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 覆盖率闸门。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(EG_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 单行诊断。
pub fn message(c: &Case) -> String {
    match c.judge() {
        Fault::Ok => {
            let mut s = String::from("ok: ");
            s.push_str(c.trigger.name());
            s.push_str(" -> ");
            let f = c.trigger.family();
            s.push_str(f.name());
            s.push('(');
            s.push_str(&i32_text(f.errno()));
            s.push_str(")\n");
            s
        }
        _ => {
            let mut s = String::from("./prog: exec ");
            s.push_str(c.trigger.name());
            s.push_str(" expects ");
            let f = c.trigger.family();
            s.push_str(f.name());
            s.push('(');
            s.push_str(&i32_text(f.errno()));
            s.push_str("), claimed ");
            s.push_str(c.claim.name());
            s.push('(');
            s.push_str(&i32_text(c.claim.errno()));
            s.push_str(")\n");
            s
        }
    }
}

/// 有符号十进制文本。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(&u32_text(v.unsigned_abs()));
        s
    } else {
        u32_text(v as u32)
    }
}

/// 短名。
pub fn short(f: Fault) -> &'static str {
    f.name()
}

// ---------------------------------------------------------------------------
// 校验器（单次）
// ---------------------------------------------------------------------------

/// 单次校验器。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    used: bool,
    last: u32,
    steps: u32,
}

impl Checker {
    /// 新建。
    pub const fn new() -> Checker {
        Checker { used: false, last: 0, steps: 0 }
    }

    /// 是否已用。
    pub const fn bound(&self) -> bool {
        self.used
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 末码。
    pub const fn last(&self) -> u32 {
        self.last
    }

    /// 判一槽。
    pub fn check(&mut self, c: &Case) -> Result<i32, u32> {
        if self.used {
            self.last = EG_ESTATE;
            return Err(EG_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        match c.judge() {
            Fault::Ok => {
                self.last = 0;
                Ok(c.trigger.errno())
            }
            _ => {
                self.last = c.expected();
                Err(c.expected())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u32,
    /// 操作码。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 数值。
    pub value: u32,
}

impl Sample {
    /// 空格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 新账。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EG_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(EG_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(EG_ESLOT)
        }
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<&Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i)
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EG_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 3, 3, 4, 4, 929];
        const CODES: [u32; 6] = [
            EG_FORMAT, EG_FORMAT, EG_MEMORY, EG_MEMORY, EG_FORMAT, EG_MEMORY,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EG_EBAL),
            };
            if s.slot as usize != i {
                return Err(EG_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EG_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EG_EVALUE);
                        }
                    }
                    None => return Err(EG_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EG_EVALUE);
                        }
                    }
                    None => return Err(EG_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EG_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EG_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(EG_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(EG_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(EG_ESEAL);
        }
        Ok(self.root)
    }

    /// 根。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u32, op: u32, v: Result<usize, u32>) -> Sample {
    match v {
        Ok(n) => Sample { slot, op, ok: true, code: 0, value: n as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 错误格。
fn err_cell(slot: u32, op: u32, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 造账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total as usize)));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept as usize)));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject as usize)));
    let _ = l.register(count_cell(3, OP_MAP, r.map(|x| x.missing as usize)));

    let _ = l.register(count_cell(4, OP_MAP, r.map(|x| x.perm as usize)));
    let _ = l.register(count_cell(5, OP_MATRIX, r.map(|x| x.format as usize)));
    let _ = l.register(count_cell(6, OP_MATRIX, r.map(|x| x.memory as usize)));

    let red = run_with(Some((0, EG_WRONG)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_JUDGE, CASES[8].case.expected()));
    let _ = l.register(err_cell(9, OP_JUDGE, CASES[9].case.expected()));
    let _ = l.register(err_cell(10, OP_JUDGE, CASES[10].case.expected()));
    let _ = l.register(err_cell(11, OP_JUDGE, CASES[11].case.expected()));
    let _ = l.register(err_cell(12, OP_JUDGE, CASES[12].case.expected()));
    let _ = l.register(err_cell(13, OP_JUDGE, CASES[13].case.expected()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let names = [Fault::Ok.name(), Fault::Wrong.name(), Fault::Matrix.name()];
    let mut i = 0usize;
    while i < names.len() {
        if names[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let fam = [
        Family::Missing.name(),
        Family::Perm.name(),
        Family::Format.name(),
        Family::Memory.name(),
    ];
    let mut k = 0usize;
    while k < fam.len() {
        if fam[k].is_empty() {
            return false;
        }
        let mut m = k + 1;
        while m < fam.len() {
            if fam[k] == fam[m] {
                return false;
            }
            m += 1;
        }
        k += 1;
    }
    EG_MISSING == SEG_BASE + 0x01
        && EG_PERM == SEG_BASE + 0x02
        && EG_FORMAT == SEG_BASE + 0x03
        && EG_MEMORY == SEG_BASE + 0x04
        && EG_WRONG == SEG_BASE + 0x05
        && EG_MATRIX == SEG_BASE + 0x06
        && EG_ESTATE == SEG_BASE + 0x07
        && EG_EVER == SEG_BASE + 0x08
        && EG_EBAL == SEG_BASE + 0x09
        && EG_ESLOT == SEG_BASE + 0x0a
        && EG_EVALUE == SEG_BASE + 0x0b
        && EG_ESEAL == SEG_BASE + 0x0c
        && EG_ENOBS == SEG_BASE + 0x0d
}

/// 码族常数。
fn codes_ok() -> bool {
    let a = Family::Missing.errno() == ERR_ENOENT && Family::Perm.errno() == ERR_EACCES;
    let b = Family::Format.errno() == ERR_ENOEXEC && Family::Memory.errno() == ERR_ENOMEM;
    let c = Family::from_errno(2) == Some(Family::Missing)
        && Family::from_errno(13) == Some(Family::Perm)
        && Family::from_errno(8) == Some(Family::Format)
        && Family::from_errno(12) == Some(Family::Memory);
    let d = Family::from_errno(22).is_none() && Family::from_errno(0).is_none();
    let e = Family::of(0) == Some(Family::Missing) && Family::of(3) == Some(Family::Memory);
    let f = Family::of(4).is_none();
    let g = Family::Missing.code() == EG_MISSING && Family::Memory.code() == EG_MEMORY;
    let h = Family::Missing.index() == 0 && Family::Memory.index() == 3;
    a && b && c && d && e && f && g && h
}

/// 触发映射。
fn map_ok() -> bool {
    let a = TRIGGERS.len() == 14;
    let b = Trigger::NoFile.errno() == 2 && Trigger::InterpMissing.errno() == 2;
    let c = Trigger::NoXbit.errno() == 13 && Trigger::DirTarget.errno() == 13;
    let d = Trigger::BadMagic.errno() == 8 && Trigger::BadScript.errno() == 8;
    let e = Trigger::OomMap.errno() == 12 && Trigger::LockedMem.errno() == 12;
    let f = Trigger::NoFile.family() == Family::Missing && Trigger::NoXbit.family() == Family::Perm;
    let g = Trigger::EmptyFile.family() == Family::Format && Trigger::StackGrow.family() == Family::Memory;
    let mut names = [""; 14];
    let mut i = 0usize;
    while i < TRIGGERS.len() {
        match TRIGGERS.get(i) {
            Some(t) => {
                let n = t.name();
                if n.is_empty() {
                    return false;
                }
                let mut j = 0usize;
                while j < i {
                    if names[j] == n {
                        return false;
                    }
                    j += 1;
                }
                names[i] = n;
            }
            None => return false,
        }
        i += 1;
    }
    a && b && c && d && e && f && g
}

/// 判面。
fn judge_ok() -> bool {
    let good = Case::new(Trigger::BadMagic, Family::Format);
    let bad = Case::new(Trigger::BadMagic, Family::Perm);
    let a = good.judge() == Fault::Ok && bad.judge() == Fault::Wrong;
    let b = judge(&good) == Fault::Ok && judge(&bad) == Fault::Wrong;
    let c = judge_errno(&good) == Ok(8) && judge_errno(&bad) == Err(8);
    let d = Fault::Wrong.code() == EG_WRONG && Fault::Wrong.errno() == ERR_EINVAL;
    let e = good.expected() == EG_FORMAT && bad.expected() == EG_FORMAT;
    let f = good.seal() == Case::new(Trigger::BadMagic, Family::Format).seal();
    let g = good.seal() != bad.seal();
    a && b && c && d && e && f && g
}

/// 矩阵统计。
fn matrix_ok() -> bool {
    let m = matrix();
    let a = m.total() == 14 && m.good() == 8;
    let b = m.balanced();
    let c = m.row(0) == Some(Row { total: 3, good: 3 });
    let d = m.row(1) == Some(Row { total: 3, good: 3 });
    let e = m.row(2) == Some(Row { total: 4, good: 1 });
    let f = m.row(3) == Some(Row { total: 4, good: 1 });
    let g = m.row(4).is_none() && m.rows[0].perfect() && !m.rows[2].perfect();
    let h = m.seal() == matrix().seal() && m.seal() != Matrix::blank().seal();
    a && b && c && d && e && f && g && h
}

/// 语料。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.missing == 3 && r.perm == 3 && r.format == 4 && r.memory == 4;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = table_ok() && unique() && notes_ok();
    a && b && c && d
}

/// 渲染。
fn render_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    r.render().as_str()
        == "errcode corpus=14 accept=8 reject=6 enoent=3 eacces=3 enoexec=4 enomem=4 match=14 rate=1000ppm\n"
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, EG_WRONG))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(EG_EVER) && !r.green();
    let c = run_with(Some((CORPUS, EG_WRONG))).err() == Some(EG_ESLOT);
    a && b && c
}

/// 文案。
fn message_ok() -> bool {
    let a = message(&Case::new(Trigger::NoFile, Family::Missing)).as_str()
        == "ok: no-file -> enoent(2)\n";
    let b = message(&Case::new(Trigger::NoXbit, Family::Perm)).as_str()
        == "ok: no-xbit -> eacces(13)\n";
    let c = message(&Case::new(Trigger::OomMap, Family::Memory)).as_str()
        == "ok: oom-map -> enomem(12)\n";
    let d = message(&Case::new(Trigger::BadMagic, Family::Format)).as_str()
        == "ok: bad-magic -> enoexec(8)\n";
    let e = message(&Case::new(Trigger::BadScript, Family::Perm)).as_str()
        == "./prog: exec bad-script expects enoexec(8), claimed eacces(13)\n";
    let f = message(&Case::new(Trigger::EmptyFile, Family::Memory)).as_str()
        == "./prog: exec empty-file expects enoexec(8), claimed enomem(12)\n";
    let g = i32_text(-5).as_str() == "-5" && i32_text(2).as_str() == "2";
    a && b && c && d && e && f && g
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.case.trigger == Trigger::NoFile && s.fault == Fault::Ok,
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.case.trigger == Trigger::LockedMem && s.fault == Fault::Wrong,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(EG_ENOBS);
    let e = short(Fault::Wrong) == "wrong" && short(Fault::Ok) == "ok";
    let f = Trigger::LockedMem.code() == EG_MEMORY && Fault::Matrix.alive();
    a && b && c && d && e && f
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = match c.check(&Case::new(Trigger::BadMagic, Family::Format)) {
        Ok(e) => e == 8 && c.last() == 0,
        Err(_) => false,
    };
    let d = c.check(&Case::new(Trigger::BadMagic, Family::Format)).err() == Some(EG_ESTATE);
    let mut w = Checker::new();
    let e = w.check(&Case::new(Trigger::BadMagic, Family::Perm)).err() == Some(EG_FORMAT);
    let f = w.last() == EG_FORMAT && w.steps() == 1;
    a && b && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && codes_ok() && map_ok();
    out[1] = judge_ok() && matrix_ok() && corpus_ok();
    out[2] = render_ok() && reds_ok() && message_ok() && slots_ok();
    out[3] = checker_ok()
        && build_ledger().audit() == Ok(())
        && {
            let mut l = build_ledger();
            l.freeze().is_ok() && l.replay().is_ok()
        };
    out
}

/// 判据集合。
pub fn run_unx11617_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11617_err_family");

    s.add(
        "EF01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "EF02-码族常数",
        {
            codes_ok()
        },
        "static",
    );

    s.add(
        "EF03-触发映射",
        {
            map_ok()
        },
        "static",
    );

    s.add(
        "EF04-判面",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "EF05-矩阵统计",
        {
            matrix_ok()
        },
        "static",
    );

    s.add(
        "EF06-语料互异",
        {
            unique() && notes_ok()
        },
        "static",
    );

    s.add(
        "EF07-逐槽对判",
        {
            table_ok()
        },
        "static",
    );

    s.add(
        "EF08-汇总与渲染",
        {
            corpus_ok() && render_ok()
        },
        "static",
    );

    s.add(
        "EF09-红档显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "EF10-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "EF11-槽号与覆盖",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "EF12-校验器与账本",
        {
            checker_ok() && build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "EF13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unx11617 test fail")
    }

    #[test]
    fn family_errno_table() {
        assert_eq!(Family::Missing.errno(), 2);
        assert_eq!(Family::Perm.errno(), 13);
        assert_eq!(Family::Format.errno(), 8);
        assert_eq!(Family::Memory.errno(), 12);
    }

    #[test]
    fn family_roundtrip() {
        assert_eq!(Family::from_errno(2), Some(Family::Missing));
        assert_eq!(Family::from_errno(13), Some(Family::Perm));
        assert_eq!(Family::from_errno(8), Some(Family::Format));
        assert_eq!(Family::from_errno(12), Some(Family::Memory));
        assert_eq!(Family::from_errno(1), None);
        assert_eq!(Family::of(0), Some(Family::Missing));
        assert_eq!(Family::of(4), None);
    }

    #[test]
    fn trigger_table() {
        assert_eq!(TRIGGERS.len(), 14);
        assert_eq!(Trigger::NoFile.errno(), 2);
        assert_eq!(Trigger::DirTarget.errno(), 13);
        assert_eq!(Trigger::BadInterp.errno(), 8);
        assert_eq!(Trigger::LockedMem.errno(), 12);
    }

    #[test]
    fn trigger_const_family() {
        assert_eq!(Trigger::NoFile.family(), Family::Missing);
        assert_eq!(Trigger::NoexecMount.family(), Family::Perm);
        assert_eq!(Trigger::EmptyFile.family(), Family::Format);
        assert_eq!(Trigger::OomArgv.family(), Family::Memory);
    }

    #[test]
    fn judge_good_bad() {
        assert_eq!(Case::new(Trigger::NoFile, Family::Missing).judge(), Fault::Ok);
        assert_eq!(Case::new(Trigger::NoFile, Family::Perm).judge(), Fault::Wrong);
        assert_eq!(judge(&Case::new(Trigger::OomMap, Family::Memory)), Fault::Ok);
    }

    #[test]
    fn judge_errno_values() {
        assert_eq!(judge_errno(&Case::new(Trigger::NoFile, Family::Missing)), Ok(2));
        assert_eq!(judge_errno(&Case::new(Trigger::NoFile, Family::Perm)), Err(2));
    }

    #[test]
    fn matrix_rows() {
        let m = matrix();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(2), Some(Row { total: 4, good: 1 }));
    }

    #[test]
    fn matrix_seal() {
        assert_eq!(matrix().seal(), matrix().seal());
        assert_ne!(matrix().seal(), Matrix::blank().seal());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.missing, 3);
        assert_eq!(r.perm, 3);
        assert_eq!(r.format, 4);
        assert_eq!(r.memory, 4);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "errcode corpus=14 accept=8 reject=6 enoent=3 eacces=3 enoexec=4 enomem=4 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, EG_WRONG))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(EG_EVER));
        assert_eq!(run_with(Some((14, EG_WRONG))).err(), Some(EG_ESLOT));
    }

    #[test]
    fn message_exact() {
        assert_eq!(
            message(&Case::new(Trigger::NoFile, Family::Missing)).as_str(),
            "ok: no-file -> enoent(2)\n"
        );
        assert_eq!(
            message(&Case::new(Trigger::BadScript, Family::Perm)).as_str(),
            "./prog: exec bad-script expects enoexec(8), claimed eacces(13)\n"
        );
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        assert_eq!(c.check(&Case::new(Trigger::BadMagic, Family::Format)), Ok(8));
        assert_eq!(
            c.check(&Case::new(Trigger::BadMagic, Family::Format)).err(),
            Some(EG_ESTATE)
        );
        assert_eq!(c.steps(), 1);
    }

    #[test]
    fn checker_wrong() {
        let mut c = Checker::new();
        assert_eq!(
            c.check(&Case::new(Trigger::EmptyFile, Family::Memory)).err(),
            Some(EG_FORMAT)
        );
        assert_eq!(c.last(), EG_FORMAT);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(EG_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(EG_ESEAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn slot_bounds() {
        assert!(slot_at(0).is_some());
        assert!(slot_at(CORPUS - 1).is_some());
        assert!(slot_at(CORPUS).is_none());
        assert_eq!(short(Fault::Ok), "ok");
        assert_eq!(short(Fault::Matrix), "matrix");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(13).as_str(), "0xd");
        assert_eq!(i32_text(-22).as_str(), "-22");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11617_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
