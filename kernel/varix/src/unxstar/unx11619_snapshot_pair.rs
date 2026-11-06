//! UNX-F11619 坏 ELF 判据双机快照验证（S1 / M7）
//!
//! 镜面：判据单机跑绿不算数——同一批坏 ELF 分别在 S1 与 M7 两台机器上跑，
//! 各拍一张十八格快照，逐格对拍。本单把「两机是否同判」收成可复算的面：
//!
//! 1. 每格是一条第 N 号判据（magic/class/phoff/…/entry）在两机上的三态
//!    （pass/fail/skip），三态相等即同判；
//! 2. 十八格必须齐全、编号必须 1..=18 且严格递增；缺格、重号、越界都拒；
//! 3. 申报面 `agreements + conflicts` 必须恰为十八——算术不平即拒；
//! 4. 申报的同判数与冲突数必须与实测相等，冲突格号必须逐个对上；
//! 5. 冲突必须能渲染成规范化文案 `./prog: ktest-conflict: …`，逐格可指认。
//!
//! 语料十四槽：八活（全同通过 / 全同失败 / 混合同判 / 双冲突 / 末格冲突 /
//! 全跳过 / 三冲突 / 翻转冲突），六死（缺格 / 重号 / 申报算术不平 / 申报数
//! 不符 / 冲突格号错 / 判据号越界）。
//!
//! 次序要害：格数先于编号（不满十八格就谈不上逐格）；编号先于算术（越界或
//! 乱序的编号让「第 N 格」失去意义）；算术先于计数（申报面自相矛盾时，比较
//! 两个数毫无意义）；计数先于格号（总数对上了才轮到逐格点名）。
//!
//! 闸门次序：名称与段基址 → 码族与判面 → 快照构造 → 快照统计 → 逐槽对判 →
//! 语料互异 → 逐槽判面表 → 汇总与渲染 → 红档显式 → 文案逐字 → 槽号与覆盖 →
//! 校验器与账本 → 冻结重放四注入。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfc20;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 判据格数。
pub const LINES: usize = 18;

/// 申报冲突格号上限。
pub const MAX_CONF: usize = 4;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 缺格。
pub const VS_ESHORT: u32 = SEG_BASE + 0x01;
/// 编号乱序或重号。
pub const VS_EORDER: u32 = SEG_BASE + 0x02;
/// 申报算术不平。
pub const VS_EMATH: u32 = SEG_BASE + 0x03;
/// 申报数不符。
pub const VS_ECOUNT: u32 = SEG_BASE + 0x04;
/// 冲突格号不符。
pub const VS_ECONF: u32 = SEG_BASE + 0x05;
/// 判据号越界。
pub const VS_EID: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const VS_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const VS_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const VS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const VS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const VS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const VS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const VS_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 取格。
pub const OP_ROW: u32 = 1;
/// 同判。
pub const OP_AGREE: u32 = 2;
/// 冲突。
pub const OP_CONFLICT: u32 = 3;
/// 文案。
pub const OP_TEXT: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;
/// 对判。
pub const OP_JUDGE: u32 = 6;
/// 封存。
pub const OP_SEAL: u32 = 7;

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
// 三态与格
// ---------------------------------------------------------------------------

/// 单机判据结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 跳过。
    Skip,
}

impl Level {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Level::Pass => "pass",
            Level::Fail => "fail",
            Level::Skip => "skip",
        }
    }

    /// 数值。
    pub const fn code(&self) -> u8 {
        match self {
            Level::Pass => 0,
            Level::Fail => 1,
            Level::Skip => 2,
        }
    }

    /// 数值反查。
    pub const fn from_code(c: u8) -> Option<Level> {
        match c {
            0 => Some(Level::Pass),
            1 => Some(Level::Fail),
            2 => Some(Level::Skip),
            _ => None,
        }
    }

    /// 是否确判（非跳过）。
    pub const fn decided(&self) -> bool {
        !matches!(self, Level::Skip)
    }
}

/// 一格判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 判据号（1..=18）。
    pub id: u8,
    /// S1 结论。
    pub s1: Level,
    /// M7 结论。
    pub m7: Level,
}

impl Row {
    /// 构造。
    pub const fn new(id: u8, s1: Level, m7: Level) -> Row {
        Row { id, s1, m7 }
    }

    /// 两机同判。
    pub const fn agree(&self) -> bool {
        matches!((self.s1, self.m7), (Level::Pass, Level::Pass))
            || matches!((self.s1, self.m7), (Level::Fail, Level::Fail))
            || matches!((self.s1, self.m7), (Level::Skip, Level::Skip))
    }

    /// 编号有效。
    pub const fn id_ok(&self) -> bool {
        self.id >= 1 && (self.id as usize) <= LINES
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.id as u64);
        h = fold_word(h, self.s1.code() as u64);
        fold_word(h, self.m7.code() as u64)
    }
}

/// 十八格快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// 格表。
    pub rows: [Row; LINES],
    /// 有效格数。
    pub count: u8,
}

impl Snapshot {
    /// 空格。
    pub const fn empty() -> Snapshot {
        Snapshot { rows: [Row::new(1, Level::Pass, Level::Pass); LINES], count: 0 }
    }

    /// 全同快照。
    pub const fn filled(level: Level) -> Snapshot {
        let mut rows = [Row::new(1, Level::Pass, Level::Pass); LINES];
        let mut i = 0usize;
        while i < LINES {
            rows[i] = Row::new((i + 1) as u8, level, level);
            i += 1;
        }
        Snapshot { rows, count: LINES as u8 }
    }

    /// 改一格（保留原编号）。
    pub const fn set(mut s: Snapshot, i: usize, s1: Level, m7: Level) -> Snapshot {
        let id = s.rows[i].id;
        s.rows[i] = Row::new(id, s1, m7);
        s
    }

    /// 改一格编号（保留两机结论）。
    pub const fn with_id(mut s: Snapshot, i: usize, id: u8) -> Snapshot {
        let s1 = s.rows[i].s1;
        let m7 = s.rows[i].m7;
        s.rows[i] = Row::new(id, s1, m7);
        s
    }

    /// 截短格数。
    pub const fn truncated(mut s: Snapshot, n: u8) -> Snapshot {
        s.count = n;
        s
    }

    /// 有效格数。
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 取一格。
    pub fn get(&self, i: usize) -> Option<&Row> {
        if i >= self.len() {
            return None;
        }
        self.rows.get(i)
    }

    /// 同判格数。
    pub fn agreements(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len() {
            if let Some(r) = self.rows.get(i) {
                if r.agree() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 冲突格数。
    pub fn conflicts(&self) -> u32 {
        self.len() as u32 - self.agreements()
    }

    /// 首个冲突格号。
    pub fn first_conflict(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len() {
            if let Some(r) = self.rows.get(i) {
                if !r.agree() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 冲突格号（依次）。
    pub fn conflict_at(&self, k: usize) -> Option<usize> {
        let mut seen = 0usize;
        let mut i = 0usize;
        while i < self.len() {
            if let Some(r) = self.rows.get(i) {
                if !r.agree() {
                    if seen == k {
                        return Some(i);
                    }
                    seen += 1;
                }
            }
            i += 1;
        }
        None
    }

    /// 编号全部有效。
    pub fn ids_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if !r.id_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 编号严格递增。
    pub fn ids_ordered(&self) -> bool {
        let mut prev = 0u8;
        let mut i = 0usize;
        while i < self.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.id <= prev {
                        return false;
                    }
                    prev = r.id;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 编号齐全（1..=18 全覆盖）。
    pub fn ids_complete(&self) -> bool {
        if self.len() != LINES {
            return false;
        }
        let mut i = 0usize;
        while i < self.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.id as usize != i + 1 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.count as u64);
        let mut i = 0usize;
        while i < self.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.seal());
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 申报面
// ---------------------------------------------------------------------------

/// 申报摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报同判数。
    pub agreements: u32,
    /// 申报冲突数。
    pub conflicts: u32,
    /// 申报冲突格号。
    pub idx: [u8; MAX_CONF],
    /// 申报冲突格数。
    pub nidx: u8,
}

impl Claim {
    /// 全同申报。
    pub const fn plain(a: u32) -> Claim {
        Claim { agreements: a, conflicts: 0, idx: [0; MAX_CONF], nidx: 0 }
    }

    /// 带冲突格号的申报。
    pub const fn split(a: u32, c: u32, idx: [u8; MAX_CONF], n: u8) -> Claim {
        Claim { agreements: a, conflicts: c, idx, nidx: n }
    }

    /// 取一个申报格号。
    pub const fn index(&self, i: usize) -> Option<u8> {
        if i >= self.nidx as usize {
            return None;
        }
        Some(self.idx[i])
    }

    /// 申报和。
    pub const fn sum(&self) -> u32 {
        self.agreements + self.conflicts
    }

    /// 申报面自洽（和 = 18，格号个数 = 冲突数）。
    pub const fn coherent(&self) -> bool {
        self.sum() == LINES as u32 && self.conflicts == self.nidx as u32
    }

    /// 冲突格号逐个对上。
    pub fn indices_match(&self, s: &Snapshot) -> bool {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < s.len() {
            if let Some(r) = s.rows.get(i) {
                if !r.agree() {
                    let want = match self.index(k) {
                        Some(v) => v,
                        None => return false,
                    };
                    if want as usize != i {
                        return false;
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        k == self.nidx as usize
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.agreements as u64);
        h = fold_word(h, self.conflicts as u64);
        h = fold_word(h, self.nidx as u64);
        let mut i = 0usize;
        while i < self.nidx as usize {
            h = fold_word(h, self.idx[i] as u64);
            i += 1;
        }
        h
    }
}

/// 实测摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// 同判数。
    pub agreements: u32,
    /// 冲突数。
    pub conflicts: u32,
}

impl Summary {
    /// 空。
    pub const fn new() -> Summary {
        Summary { agreements: 0, conflicts: 0 }
    }

    /// 由快照实测。
    pub fn of(s: &Snapshot) -> Summary {
        Summary { agreements: s.agreements(), conflicts: s.conflicts() }
    }

    /// 和。
    pub const fn sum(&self) -> u32 {
        self.agreements + self.conflicts
    }

    /// 是否满十八。
    pub const fn full(&self) -> bool {
        self.sum() == LINES as u32
    }
}

// ---------------------------------------------------------------------------
// 判面
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 缺格。
    Short,
    /// 编号乱序或重号。
    Order,
    /// 申报算术不平。
    Math,
    /// 申报数不符。
    Count,
    /// 冲突格号不符。
    Conf,
    /// 判据号越界。
    Id,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Short => "short",
            Fault::Order => "order",
            Fault::Math => "math",
            Fault::Count => "count",
            Fault::Conf => "conflict",
            Fault::Id => "id",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Short => VS_ESHORT,
            Fault::Order => VS_EORDER,
            Fault::Math => VS_EMATH,
            Fault::Count => VS_ECOUNT,
            Fault::Conf => VS_ECONF,
            Fault::Id => VS_EID,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            _ => ENOEXEC,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 槽名。
    pub name: &'static str,
    /// 快照。
    pub snap: Snapshot,
    /// 申报面。
    pub claim: Claim,
    /// 期望判面。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(
        name: &'static str,
        snap: Snapshot,
        claim: Claim,
        fault: Fault,
        note: &'static str,
    ) -> Slot {
        Slot { name, snap, claim, fault, note }
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 期望错误码（通过为 0）。
    pub fn expected(&self) -> u32 {
        judge(self).code()
    }

    /// 实测摘要。
    pub fn summary(&self) -> Summary {
        Summary::of(&self.snap)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.snap.seal());
        fold_word(h, self.claim.seal())
    }
}

/// 对判：格数 → 编号有效 → 编号有序 → 算术 → 计数 → 格号。
pub fn judge(s: &Slot) -> Fault {
    if s.snap.len() != LINES {
        return Fault::Short;
    }
    if !s.snap.ids_ok() {
        return Fault::Id;
    }
    if !s.snap.ids_ordered() {
        return Fault::Order;
    }
    if !s.claim.coherent() {
        return Fault::Math;
    }
    let got = Summary::of(&s.snap);
    if got.agreements != s.claim.agreements || got.conflicts != s.claim.conflicts {
        return Fault::Count;
    }
    if !s.claim.indices_match(&s.snap) {
        return Fault::Conf;
    }
    Fault::Ok
}

/// 首冲突格号或错误码。
pub fn judge_first(s: &Slot) -> Result<Option<usize>, u32> {
    match s.judge() {
        Fault::Ok => Ok(s.snap.first_conflict()),
        f => Err(f.code()),
    }
}

// 快照构造器（编译期折叠）。

/// 混合：前十二格全同通过，后六格全同失败。
const fn mixed() -> Snapshot {
    let mut s = Snapshot::filled(Level::Pass);
    let mut i = 12usize;
    while i < LINES {
        s = Snapshot::set(s, i, Level::Fail, Level::Fail);
        i += 1;
    }
    s
}

/// 全跳过。
const fn skip_all() -> Snapshot {
    Snapshot::filled(Level::Skip)
}

/// 双冲突：第 4 格与第 11 格。
const fn two_conflicts() -> Snapshot {
    let mut s = Snapshot::filled(Level::Pass);
    s = Snapshot::set(s, 4, Level::Pass, Level::Fail);
    s = Snapshot::set(s, 11, Level::Fail, Level::Skip);
    s
}

/// 末格冲突。
const fn late_conflict() -> Snapshot {
    Snapshot::set(Snapshot::filled(Level::Pass), 17, Level::Skip, Level::Fail)
}

/// 三冲突：第 0、8、17 格。
const fn three_conflicts() -> Snapshot {
    let mut s = Snapshot::filled(Level::Fail);
    s = Snapshot::set(s, 0, Level::Pass, Level::Skip);
    s = Snapshot::set(s, 8, Level::Skip, Level::Pass);
    s = Snapshot::set(s, 17, Level::Pass, Level::Fail);
    s
}

/// 翻转冲突：第 2 格与第 9 格。
const fn flip_conflicts() -> Snapshot {
    let mut s = Snapshot::filled(Level::Fail);
    s = Snapshot::set(s, 2, Level::Pass, Level::Skip);
    s = Snapshot::set(s, 9, Level::Fail, Level::Skip);
    s
}

/// 重号：第 10 格改成第 9 号。
const fn dup_id() -> Snapshot {
    Snapshot::with_id(Snapshot::filled(Level::Fail), 10, 9)
}

/// 越界：末格改成第 19 号（顺序仍递增，仅编号越界）。
const fn id_high() -> Snapshot {
    Snapshot::with_id(Snapshot::filled(Level::Pass), 17, 19)
}

/// 缺格：只留十七格。
const fn short_rows() -> Snapshot {
    Snapshot::truncated(Snapshot::filled(Level::Pass), 17)
}

/// 十四槽。
pub const PAIRS: [Slot; CORPUS] = [
    Slot::new(
        "all-pass",
        Snapshot::filled(Level::Pass),
        Claim::plain(18),
        Fault::Ok,
        "十八格全同通过",
    ),
    Slot::new(
        "all-fail",
        Snapshot::filled(Level::Fail),
        Claim::plain(18),
        Fault::Ok,
        "十八格全同失败",
    ),
    Slot::new("mixed-agree", mixed(), Claim::plain(18), Fault::Ok, "混合结论全同判"),
    Slot::new(
        "two-conflict",
        two_conflicts(),
        Claim::split(16, 2, [4, 11, 0, 0], 2),
        Fault::Ok,
        "双冲突逐格点名",
    ),
    Slot::new(
        "late-conflict",
        late_conflict(),
        Claim::split(17, 1, [17, 0, 0, 0], 1),
        Fault::Ok,
        "末格冲突",
    ),
    Slot::new("all-skip", skip_all(), Claim::plain(18), Fault::Ok, "十八格全跳过"),
    Slot::new(
        "three-conflict",
        three_conflicts(),
        Claim::split(15, 3, [0, 8, 17, 0], 3),
        Fault::Ok,
        "三冲突",
    ),
    Slot::new(
        "flip-conflict",
        flip_conflicts(),
        Claim::split(16, 2, [2, 9, 0, 0], 2),
        Fault::Ok,
        "两机翻转冲突",
    ),
    Slot::new(
        "short-rows",
        short_rows(),
        Claim::plain(17),
        Fault::Short,
        "缺一死",
    ),
    Slot::new(
        "dup-ids",
        dup_id(),
        Claim::plain(18),
        Fault::Order,
        "重号死",
    ),
    Slot::new(
        "claim-math",
        Snapshot::filled(Level::Pass),
        Claim::split(17, 2, [4, 11, 0, 0], 2),
        Fault::Math,
        "算术不平死",
    ),
    Slot::new(
        "claim-count",
        Snapshot::filled(Level::Pass),
        Claim::split(17, 1, [17, 0, 0, 0], 1),
        Fault::Count,
        "计数不符死",
    ),
    Slot::new(
        "claim-index",
        two_conflicts(),
        Claim::split(16, 2, [3, 11, 0, 0], 2),
        Fault::Conf,
        "格号错死",
    ),
    Slot::new(
        "id-range",
        id_high(),
        Claim::plain(18),
        Fault::Id,
        "编号越界死",
    ),
];

/// 逐槽期望判面。
pub const EXPECTED: [Fault; CORPUS] = [
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Short,
    Fault::Order,
    Fault::Math,
    Fault::Count,
    Fault::Conf,
    Fault::Id,
];

/// 判据名表。
pub const CRITERIA: [&str; LINES] = [
    "magic",
    "class",
    "type",
    "phoff",
    "phent",
    "phnum",
    "seg-count",
    "seg-memsz",
    "seg-filesz",
    "seg-align",
    "seg-congruence",
    "interp",
    "needed",
    "reloc",
    "symbol",
    "stack",
    "brk",
    "entry",
];

/// 判据名。
pub fn criterion_name(id: u8) -> &'static str {
    if id < 1 || id as usize > LINES {
        return "?";
    }
    match CRITERIA.get((id - 1) as usize) {
        Some(v) => *v,
        None => "?",
    }
}

/// 取一槽。
pub fn slot_at(i: usize) -> Option<&'static Slot> {
    PAIRS.get(i)
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总数。
    pub total: u32,
    /// 通过数。
    pub accept: u32,
    /// 拒收数。
    pub reject: u32,
    /// 同判格累计（仅通过槽）。
    pub agree: u32,
    /// 冲突格累计（仅通过槽）。
    pub conflict: u32,
    /// 判据格数。
    pub criteria: u32,
    /// 零冲突通过槽数。
    pub clean: u32,
    /// 命中数。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, agree: 0, conflict: 0, criteria: 0, clean: 0, matched: 0 }
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

    /// 通过槽的同判与冲突是否合计十八格。
    pub const fn balanced(&self) -> bool {
        self.agree + self.conflict == self.accept * LINES as u32
    }

    /// 是否全绿。
    pub const fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total && self.balanced()
    }

    /// 闸门。
    pub const fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(VS_ENOBS);
        }
        if self.matched != self.total {
            return Err(VS_EVER);
        }
        if !self.balanced() {
            return Err(VS_EBAL);
        }
        Ok(())
    }

    /// 计一槽。
    pub fn tally(&mut self, s: &Slot) {
        self.total += 1;
        if s.judge() == Fault::Ok {
            self.accept += 1;
            let sum = Summary::of(&s.snap);
            self.agree += sum.agreements;
            self.conflict += sum.conflicts;
            if sum.conflicts == 0 {
                self.clean += 1;
            }
        } else {
            self.reject += 1;
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("snapshot corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" agree=");
        s.push_str(&u32_text(self.agree));
        s.push_str(" conflict=");
        s.push_str(&u32_text(self.conflict));
        s.push_str(" criteria=");
        s.push_str(&u32_text(self.criteria));
        s.push_str(" clean=");
        s.push_str(&u32_text(self.clean));
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
            return Err(VS_ESLOT);
        }
    }
    let mut r = Report::new();
    r.criteria = LINES as u32;
    let mut i = 0usize;
    while i < PAIRS.len() {
        let s = match PAIRS.get(i) {
            Some(v) => *v,
            None => return Err(VS_ESLOT),
        };
        r.tally(&s);
        let overridden = match over {
            Some((k, _)) => k == i,
            None => false,
        };
        if s.judge() == EXPECTED[i] && !overridden {
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
        Err(VS_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 冲突行（规范化）。
pub fn conflict_line(s: &Snapshot, i: usize) -> String {
    let mut out = String::from("./prog: ktest-conflict: ");
    match s.get(i) {
        Some(r) => {
            out.push_str(criterion_name(r.id));
            out.push_str(" S1=");
            out.push_str(r.s1.name());
            out.push_str(" M7=");
            out.push_str(r.m7.name());
            out.push('\n');
        }
        None => {
            out.push_str("row ");
            out.push_str(&u32_text(i as u32));
            out.push_str(" out of range\n");
        }
    }
    out
}

/// 单行诊断。
pub fn message(s: &Slot) -> String {
    match s.judge() {
        Fault::Ok => {
            let sum = Summary::of(&s.snap);
            let mut out = String::from("ok: ");
            out.push_str(s.name);
            out.push_str(" agree=");
            out.push_str(&u32_text(sum.agreements));
            out.push_str(" conflict=");
            out.push_str(&u32_text(sum.conflicts));
            out.push('\n');
            out
        }
        Fault::Short => {
            let mut out = String::from("./prog: snapshot ");
            out.push_str(s.name);
            out.push_str(": rows=");
            out.push_str(&u32_text(s.snap.len() as u32));
            out.push_str(" want ");
            out.push_str(&u32_text(LINES as u32));
            out.push('\n');
            out
        }
        Fault::Order => {
            let mut out = String::from("./prog: snapshot ");
            out.push_str(s.name);
            out.push_str(": criterion ids duplicated or out of order\n");
            out
        }
        Fault::Math => {
            let mut out = String::from("./prog: snapshot ");
            out.push_str(s.name);
            out.push_str(": claim sum ");
            out.push_str(&u32_text(s.claim.sum()));
            out.push_str(" want ");
            out.push_str(&u32_text(LINES as u32));
            out.push('\n');
            out
        }
        Fault::Count => {
            let got = Summary::of(&s.snap);
            let mut out = String::from("./prog: snapshot ");
            out.push_str(s.name);
            out.push_str(": claim ");
            out.push_str(&u32_text(s.claim.agreements));
            out.push('/');
            out.push_str(&u32_text(s.claim.conflicts));
            out.push_str(" actual ");
            out.push_str(&u32_text(got.agreements));
            out.push('/');
            out.push_str(&u32_text(got.conflicts));
            out.push('\n');
            out
        }
        Fault::Conf => {
            let mut out = String::from("./prog: snapshot ");
            out.push_str(s.name);
            out.push_str(": conflict index mismatch\n");
            out
        }
        Fault::Id => {
            let mut out = String::from("./prog: snapshot ");
            out.push_str(s.name);
            out.push_str(": criterion id out of range\n");
            out
        }
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
    pub fn check(&mut self, s: &Slot) -> Result<Summary, u32> {
        if self.used {
            self.last = VS_ESTATE;
            return Err(VS_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        match s.judge() {
            Fault::Ok => {
                self.last = 0;
                Ok(Summary::of(&s.snap))
            }
            f => {
                self.last = f.code();
                Err(f.code())
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
            return Err(VS_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(VS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(VS_ESLOT)
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
            return Err(VS_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 136, 8, 18, 4, 929];
        const CODES: [u32; 6] = [
            VS_ESHORT, VS_EORDER, VS_EMATH, VS_ECOUNT, VS_ECONF, VS_EID,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VS_EBAL),
            };
            if s.slot as usize != i {
                return Err(VS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VS_EVALUE);
                        }
                    }
                    None => return Err(VS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VS_EVALUE);
                        }
                    }
                    None => return Err(VS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(VS_EBAL);
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
            return Err(VS_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(VS_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(VS_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(VS_ESEAL);
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
    let _ = l.register(count_cell(3, OP_AGREE, r.map(|x| x.agree as usize)));

    let _ = l.register(count_cell(4, OP_CONFLICT, r.map(|x| x.conflict as usize)));
    let _ = l.register(count_cell(5, OP_ROW, r.map(|x| x.criteria as usize)));
    let _ = l.register(count_cell(6, OP_ROW, r.map(|x| x.clean as usize)));

    let red = run_with(Some((0, VS_EVER)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_JUDGE, PAIRS[8].expected()));
    let _ = l.register(err_cell(9, OP_JUDGE, PAIRS[9].expected()));
    let _ = l.register(err_cell(10, OP_JUDGE, PAIRS[10].expected()));
    let _ = l.register(err_cell(11, OP_JUDGE, PAIRS[11].expected()));
    let _ = l.register(err_cell(12, OP_JUDGE, PAIRS[12].expected()));
    let _ = l.register(err_cell(13, OP_JUDGE, PAIRS[13].expected()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let fnames = [
        Fault::Ok.name(),
        Fault::Short.name(),
        Fault::Order.name(),
        Fault::Math.name(),
        Fault::Count.name(),
        Fault::Conf.name(),
        Fault::Id.name(),
    ];
    let mut i = 0usize;
    while i < fnames.len() {
        if fnames[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < fnames.len() {
            if fnames[i] == fnames[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let lnames = [Level::Pass.name(), Level::Fail.name(), Level::Skip.name()];
    let mut k = 0usize;
    while k < lnames.len() {
        if lnames[k].is_empty() {
            return false;
        }
        let mut m = k + 1;
        while m < lnames.len() {
            if lnames[k] == lnames[m] {
                return false;
            }
            m += 1;
        }
        k += 1;
    }
    SEG_BASE == 0xfc20
        && VS_ESHORT == SEG_BASE + 0x01
        && VS_EORDER == SEG_BASE + 0x02
        && VS_EMATH == SEG_BASE + 0x03
        && VS_ECOUNT == SEG_BASE + 0x04
        && VS_ECONF == SEG_BASE + 0x05
        && VS_EID == SEG_BASE + 0x06
        && VS_ESTATE == SEG_BASE + 0x07
        && VS_EVER == SEG_BASE + 0x08
        && VS_EBAL == SEG_BASE + 0x09
        && VS_ESLOT == SEG_BASE + 0x0a
        && VS_EVALUE == SEG_BASE + 0x0b
        && VS_ESEAL == SEG_BASE + 0x0c
        && VS_ENOBS == SEG_BASE + 0x0d
}

/// 码族与判面。
fn codes_ok() -> bool {
    let a = Level::Pass.code() == 0 && Level::Fail.code() == 1 && Level::Skip.code() == 2;
    let b = Level::from_code(0) == Some(Level::Pass)
        && Level::from_code(1) == Some(Level::Fail)
        && Level::from_code(2) == Some(Level::Skip)
        && Level::from_code(3).is_none();
    let c = Level::Pass.decided() && Level::Fail.decided() && !Level::Skip.decided();
    let d = Fault::Ok.code() == 0 && Fault::Short.code() == VS_ESHORT;
    let e = Fault::Order.code() == VS_EORDER
        && Fault::Math.code() == VS_EMATH
        && Fault::Count.code() == VS_ECOUNT
        && Fault::Conf.code() == VS_ECONF
        && Fault::Id.code() == VS_EID;
    let f = Fault::Ok.errno() == 0 && Fault::Count.errno() == ENOEXEC && !Fault::Ok.alive();
    let g = Row::new(1, Level::Pass, Level::Pass).id_ok()
        && Row::new(18, Level::Fail, Level::Skip).id_ok()
        && !Row::new(0, Level::Pass, Level::Pass).id_ok()
        && !Row::new(19, Level::Pass, Level::Pass).id_ok();
    let h = Row::new(3, Level::Skip, Level::Skip).agree()
        && !Row::new(3, Level::Pass, Level::Fail).agree();
    a && b && c && d && e && f && g && h
}

/// 快照构造。
fn snapshot_ok() -> bool {
    let a = Snapshot::filled(Level::Pass).len() == LINES;
    let b = Snapshot::filled(Level::Fail).agreements() == 18;
    let c = Snapshot::filled(Level::Skip).ids_complete();
    let d = mixed().ids_complete() && mixed().agreements() == 18;
    let e = two_conflicts().conflicts() == 2
        && two_conflicts().conflict_at(0) == Some(4)
        && two_conflicts().conflict_at(1) == Some(11)
        && two_conflicts().conflict_at(2).is_none();
    let f = late_conflict().first_conflict() == Some(17);
    let g = three_conflicts().conflicts() == 3 && flip_conflicts().conflicts() == 2;
    let h = !short_rows().ids_complete() && short_rows().len() == 17;
    let i = !dup_id().ids_ordered() && dup_id().ids_ok();
    let j = !id_high().ids_ok() && id_high().ids_ordered();
    let k = Snapshot::empty().is_empty() && Snapshot::empty().get(0).is_none();
    a && b && c && d && e && f && g && h && i && j && k
}

/// 快照统计与指纹。
fn stats_ok() -> bool {
    let s = two_conflicts();
    let a = s.agreements() == 16 && s.conflicts() == 2;
    let b = Summary::of(&s) == Summary { agreements: 16, conflicts: 2 };
    let c = Summary::of(&s).sum() == 18 && Summary::of(&s).full();
    let d = Summary { agreements: 17, conflicts: 2 }.full() == false;
    let e = s.seal() == two_conflicts().seal() && s.seal() != three_conflicts().seal();
    let f = Claim::split(16, 2, [4, 11, 0, 0], 2).indices_match(&s);
    let g = !Claim::split(16, 2, [3, 11, 0, 0], 2).indices_match(&s);
    let h = !Claim::split(16, 2, [4, 11, 0, 0], 1).indices_match(&s);
    let i = Claim::plain(18).coherent() && !Claim::plain(17).coherent();
    let j = !Claim::split(17, 2, [4, 11, 0, 0], 2).coherent();
    let k = Claim::plain(18).index(0).is_none() && Claim::split(16, 2, [4, 11, 0, 0], 2).index(0) == Some(4);
    a && b && c && d && e && f && g && h && i && j && k
}

/// 逐槽对判。
fn judge_ok() -> bool {
    let a = judge(&PAIRS[0]) == Fault::Ok;
    let b = judge(&PAIRS[8]) == Fault::Short;
    let c = judge(&PAIRS[9]) == Fault::Order;
    let d = judge(&PAIRS[10]) == Fault::Math;
    let e = judge(&PAIRS[11]) == Fault::Count;
    let f = judge(&PAIRS[12]) == Fault::Conf;
    let g = judge(&PAIRS[13]) == Fault::Id;
    let h = judge_first(&PAIRS[3]) == Ok(Some(4)) && judge_first(&PAIRS[0]) == Ok(None);
    let i = judge_first(&PAIRS[9]) == Err(VS_EORDER);
    let j = PAIRS[0].expected() == 0 && PAIRS[13].expected() == VS_EID;
    a && b && c && d && e && f && g && h && i && j
}

/// 语料互异。
fn unique() -> bool {
    let mut i = 0usize;
    while i < PAIRS.len() {
        let a = match PAIRS.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < PAIRS.len() {
            let b = match PAIRS.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.name == b.name {
                return false;
            }
            if a.snap == b.snap && a.claim == b.claim {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注齐全。
fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < PAIRS.len() {
        match PAIRS.get(i) {
            Some(s) => {
                if s.name.is_empty() || s.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐槽判面表。
fn table_ok() -> bool {
    let mut i = 0usize;
    while i < PAIRS.len() {
        let s = match PAIRS.get(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match EXPECTED.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.judge() != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 汇总。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.agree == 136 && r.conflict == 8 && r.criteria == 18 && r.clean == 4;
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
        == "snapshot corpus=14 accept=8 reject=6 agree=136 conflict=8 criteria=18 clean=4 match=14 rate=1000ppm\n"
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, VS_EVER))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(VS_EVER) && !r.green();
    let c = run_with(Some((CORPUS, VS_EVER))).err() == Some(VS_ESLOT);
    a && b && c
}

/// 文案。
fn message_ok() -> bool {
    let a = message(&PAIRS[0]).as_str() == "ok: all-pass agree=18 conflict=0\n";
    let b = message(&PAIRS[3]).as_str() == "ok: two-conflict agree=16 conflict=2\n";
    let c = message(&PAIRS[8]).as_str() == "./prog: snapshot short-rows: rows=17 want 18\n";
    let d = message(&PAIRS[9]).as_str()
        == "./prog: snapshot dup-ids: criterion ids duplicated or out of order\n";
    let e = message(&PAIRS[10]).as_str() == "./prog: snapshot claim-math: claim sum 19 want 18\n";
    let f = message(&PAIRS[11]).as_str()
        == "./prog: snapshot claim-count: claim 17/1 actual 18/0\n";
    let g = message(&PAIRS[12]).as_str()
        == "./prog: snapshot claim-index: conflict index mismatch\n";
    let h = message(&PAIRS[13]).as_str() == "./prog: snapshot id-range: criterion id out of range\n";
    let i = conflict_line(&two_conflicts(), 4).as_str()
        == "./prog: ktest-conflict: phent S1=pass M7=fail\n";
    let j = conflict_line(&two_conflicts(), 11).as_str()
        == "./prog: ktest-conflict: interp S1=fail M7=skip\n";
    let k = conflict_line(&two_conflicts(), 18).as_str()
        == "./prog: ktest-conflict: row 18 out of range\n";
    a && b && c && d && e && f && g && h && i && j && k
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.name == "all-pass" && s.judge() == Fault::Ok,
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.name == "id-range" && s.expected() == VS_EID,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(VS_ENOBS);
    let e = short(Fault::Math) == "math" && short(Fault::Ok) == "ok";
    let f = match slot_at(4) {
        Some(s) => s.name == "late-conflict" && s.summary().conflicts == 1,
        None => false,
    };
    a && b && c && d && e && f
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = c.check(&PAIRS[0]) == Ok(Summary { agreements: 18, conflicts: 0 }) && c.last() == 0;
    let d = c.check(&PAIRS[0]).err() == Some(VS_ESTATE);
    let mut w = Checker::new();
    let e = w.check(&PAIRS[10]).err() == Some(VS_EMATH);
    let f = w.last() == VS_EMATH && w.steps() == 1;
    a && b && d && e && f
}

/// 账本。
fn ledger_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit() == Ok(()) && !l.is_empty() && l.sample(SAMPLES).is_none();
    let mut m = build_ledger();
    let c = match m.freeze() {
        Ok(r) => r != 0 && m.root() == r && m.is_frozen() && m.replay() == Ok(r),
        Err(_) => false,
    };
    let d = m.register(Sample::blank()).err() == Some(VS_ESEAL) && m.rejects() == 1;
    let e = m.freeze().err() == Some(VS_ESEAL) && m.rejects() == 2;
    let mut n = Ledger::new();
    let f = n.replay().err() == Some(VS_ESTATE);
    let g = n.register(Sample::blank()).is_ok() && n.len() == 1;
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && codes_ok() && snapshot_ok();
    out[1] = stats_ok() && judge_ok() && table_ok();
    out[2] = corpus_ok() && render_ok() && reds_ok() && message_ok();
    out[3] = slots_ok() && checker_ok() && ledger_ok() && unique() && notes_ok();
    out
}

/// 判据集合。
pub fn run_unx11619_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11619_snapshot_pair");

    s.add(
        "VS01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "VS02-码族与判面",
        {
            codes_ok()
        },
        "static",
    );

    s.add(
        "VS03-快照构造",
        {
            snapshot_ok()
        },
        "static",
    );

    s.add(
        "VS04-快照统计与指纹",
        {
            stats_ok()
        },
        "static",
    );

    s.add(
        "VS05-逐槽对判",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "VS06-语料互异",
        {
            unique() && notes_ok()
        },
        "static",
    );

    s.add(
        "VS07-逐槽判面表",
        {
            table_ok()
        },
        "static",
    );

    s.add(
        "VS08-汇总与渲染",
        {
            corpus_ok() && render_ok()
        },
        "static",
    );

    s.add(
        "VS09-红档显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "VS10-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "VS11-槽号与覆盖",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "VS12-校验器与账本",
        {
            checker_ok() && ledger_ok()
        },
        "static",
    );

    s.add(
        "VS13-冻结重放四注入",
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
        panic!("unx11619 test fail")
    }

    #[test]
    fn level_table() {
        assert_eq!(Level::Pass.name(), "pass");
        assert_eq!(Level::Fail.name(), "fail");
        assert_eq!(Level::Skip.name(), "skip");
        assert_eq!(Level::Pass.code(), 0);
        assert_eq!(Level::Skip.code(), 2);
        assert!(Level::Pass.decided() && !Level::Skip.decided());
    }

    #[test]
    fn level_roundtrip() {
        assert_eq!(Level::from_code(0), Some(Level::Pass));
        assert_eq!(Level::from_code(1), Some(Level::Fail));
        assert_eq!(Level::from_code(2), Some(Level::Skip));
        assert_eq!(Level::from_code(3), None);
    }

    #[test]
    fn row_agree() {
        assert!(Row::new(1, Level::Pass, Level::Pass).agree());
        assert!(Row::new(1, Level::Fail, Level::Fail).agree());
        assert!(Row::new(1, Level::Skip, Level::Skip).agree());
        assert!(!Row::new(1, Level::Pass, Level::Fail).agree());
        assert!(!Row::new(1, Level::Fail, Level::Skip).agree());
    }

    #[test]
    fn row_ids() {
        assert!(Row::new(1, Level::Pass, Level::Pass).id_ok());
        assert!(Row::new(18, Level::Pass, Level::Pass).id_ok());
        assert!(!Row::new(0, Level::Pass, Level::Pass).id_ok());
        assert!(!Row::new(19, Level::Pass, Level::Pass).id_ok());
    }

    #[test]
    fn filled_snapshot() {
        let s = Snapshot::filled(Level::Pass);
        assert_eq!(s.len(), 18);
        assert_eq!(s.agreements(), 18);
        assert_eq!(s.conflicts(), 0);
        assert!(s.ids_complete());
        assert!(!s.is_empty());
    }

    #[test]
    fn mixed_snapshot() {
        let s = mixed();
        assert_eq!(s.agreements(), 18);
        assert_eq!(s.conflicts(), 0);
        assert_eq!(s.rows[12].s1, Level::Fail);
        assert_eq!(s.rows[11].s1, Level::Pass);
    }

    #[test]
    fn conflict_snapshot() {
        let s = two_conflicts();
        assert_eq!(s.conflicts(), 2);
        assert_eq!(s.first_conflict(), Some(4));
        assert_eq!(s.conflict_at(0), Some(4));
        assert_eq!(s.conflict_at(2), None);
    }

    #[test]
    fn short_snapshot() {
        let s = short_rows();
        assert_eq!(s.len(), 17);
        assert!(!s.ids_complete());
        assert_eq!(s.agreements(), 17);
    }

    #[test]
    fn id_anomalies() {
        assert!(!dup_id().ids_ordered());
        assert!(dup_id().ids_ok());
        assert!(!id_high().ids_ok());
        assert!(id_high().ids_ordered());
    }

    #[test]
    fn summary_full() {
        let s = Summary::of(&two_conflicts());
        assert_eq!(s, Summary { agreements: 16, conflicts: 2 });
        assert_eq!(s.sum(), 18);
        assert!(s.full());
        assert!(!Summary { agreements: 17, conflicts: 2 }.full());
    }

    #[test]
    fn claim_coherent() {
        assert!(Claim::plain(18).coherent());
        assert!(!Claim::plain(17).coherent());
        assert!(!Claim::split(17, 2, [4, 11, 0, 0], 2).coherent());
        assert!(Claim::split(16, 2, [4, 11, 0, 0], 2).coherent());
    }

    #[test]
    fn claim_indices() {
        let s = two_conflicts();
        assert!(Claim::split(16, 2, [4, 11, 0, 0], 2).indices_match(&s));
        assert!(!Claim::split(16, 2, [3, 11, 0, 0], 2).indices_match(&s));
        assert!(!Claim::split(16, 2, [4, 11, 0, 0], 1).indices_match(&s));
        assert!(Claim::plain(18).indices_match(&Snapshot::filled(Level::Pass)));
    }

    #[test]
    fn judge_accept_slots() {
        let mut i = 0usize;
        while i < 8 {
            assert_eq!(PAIRS[i].judge(), Fault::Ok);
            assert_eq!(PAIRS[i].expected(), 0);
            i += 1;
        }
    }

    #[test]
    fn judge_reject_slots() {
        assert_eq!(PAIRS[8].judge(), Fault::Short);
        assert_eq!(PAIRS[9].judge(), Fault::Order);
        assert_eq!(PAIRS[10].judge(), Fault::Math);
        assert_eq!(PAIRS[11].judge(), Fault::Count);
        assert_eq!(PAIRS[12].judge(), Fault::Conf);
        assert_eq!(PAIRS[13].judge(), Fault::Id);
    }

    #[test]
    fn judge_first_conflict() {
        assert_eq!(judge_first(&PAIRS[3]), Ok(Some(4)));
        assert_eq!(judge_first(&PAIRS[0]), Ok(None));
        assert_eq!(judge_first(&PAIRS[13]), Err(VS_EID));
    }

    #[test]
    fn pairs_unique() {
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(PAIRS.len(), CORPUS);
    }

    #[test]
    fn slot_seal() {
        assert_eq!(PAIRS[0].seal(), PAIRS[0].seal());
        assert_ne!(PAIRS[0].seal(), PAIRS[1].seal());
        assert_ne!(PAIRS[3].seal(), PAIRS[12].seal());
    }

    #[test]
    fn criterion_names() {
        assert_eq!(criterion_name(1), "magic");
        assert_eq!(criterion_name(5), "phent");
        assert_eq!(criterion_name(18), "entry");
        assert_eq!(criterion_name(0), "?");
        assert_eq!(criterion_name(19), "?");
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
        assert_eq!(r.agree, 136);
        assert_eq!(r.conflict, 8);
        assert_eq!(r.criteria, 18);
        assert_eq!(r.clean, 4);
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
            "snapshot corpus=14 accept=8 reject=6 agree=136 conflict=8 criteria=18 clean=4 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn report_gate() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(r.balanced() && r.green());
        assert_eq!(r.gate(), Ok(()));
        let mut bad = Report::new();
        bad.total = CORPUS as u32;
        bad.matched = CORPUS as u32;
        bad.accept = 8;
        bad.agree = 100;
        assert!(!bad.balanced());
        assert_eq!(bad.gate(), Err(VS_EBAL));
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, VS_EVER))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(VS_EVER));
        assert_eq!(run_with(Some((14, VS_EVER))).err(), Some(VS_ESLOT));
    }

    #[test]
    fn message_exact() {
        assert_eq!(message(&PAIRS[0]).as_str(), "ok: all-pass agree=18 conflict=0\n");
        assert_eq!(message(&PAIRS[3]).as_str(), "ok: two-conflict agree=16 conflict=2\n");
        assert_eq!(
            message(&PAIRS[10]).as_str(),
            "./prog: snapshot claim-math: claim sum 19 want 18\n"
        );
    }

    #[test]
    fn message_reject_exact() {
        assert_eq!(
            message(&PAIRS[8]).as_str(),
            "./prog: snapshot short-rows: rows=17 want 18\n"
        );
        assert_eq!(
            message(&PAIRS[11]).as_str(),
            "./prog: snapshot claim-count: claim 17/1 actual 18/0\n"
        );
        assert_eq!(
            message(&PAIRS[13]).as_str(),
            "./prog: snapshot id-range: criterion id out of range\n"
        );
    }

    #[test]
    fn conflict_line_exact() {
        assert_eq!(
            conflict_line(&two_conflicts(), 4).as_str(),
            "./prog: ktest-conflict: phent S1=pass M7=fail\n"
        );
        assert_eq!(
            conflict_line(&two_conflicts(), 11).as_str(),
            "./prog: ktest-conflict: interp S1=fail M7=skip\n"
        );
        assert_eq!(
            conflict_line(&Snapshot::filled(Level::Pass), 18).as_str(),
            "./prog: ktest-conflict: row 18 out of range\n"
        );
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        assert_eq!(c.check(&PAIRS[0]), Ok(Summary { agreements: 18, conflicts: 0 }));
        assert_eq!(c.check(&PAIRS[0]).err(), Some(VS_ESTATE));
        assert_eq!(c.steps(), 1);
    }

    #[test]
    fn checker_wrong() {
        let mut c = Checker::new();
        assert_eq!(c.check(&PAIRS[12]).err(), Some(VS_ECONF));
        assert_eq!(c.last(), VS_ECONF);
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
        assert_eq!(l.register(Sample::blank()).err(), Some(VS_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(VS_ESEAL));
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
        assert_eq!(short(Fault::Short), "short");
        assert_eq!(short(Fault::Ok), "ok");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(136).as_str(), "136");
        assert_eq!(hex_text(0xfc21).as_str(), "0xfc21");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11619_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
