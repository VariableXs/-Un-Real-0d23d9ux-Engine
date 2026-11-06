//! UNX-F11620 B21 批 ktest 断言集聚合
//!
//! 镜面：B21 批的判据散在十九个断言包里，各自单跑都绿，合起来却可能因为
//! 包号重排、条数对不上、跳过数超限而整体失真。本单把十九包收成一张
//! ktest 静态面，一条命令跑全，且聚合面必须自洽：
//!
//! 1. 十九包齐全，包号 1..=19 严格递增；
//! 2. 每包内部 `pass + skip == checks`，且 `skip <= 1`；
//! 3. 聚合面 `checks = pass + skip`，`checks` 等于各包条数之和；
//! 4. `green` 只数「全过无跳过」的包，申报必须与实测一致；
//! 5. 申报面五项（条数/通过/跳过/全绿包数）任一不符都显式报错。
//!
//! 语料十四槽：八活（全绿 / 一跳 / 二跳 / 三跳 / 六跳 / 七跳 / 末包跳 /
//! 首末双跳），六死（缺包 / 重号 / 包内算术不平 / 跳过超限 / 条数申报错 /
//! 通过数申报错）。
//!
//! 次序要害：包数先于包内（缺包时「第 N 包」无意义）；包内先于包号（一包
//! 自身的账都平不了，排序再对也白搭）；包号先于跳过限（先确认这是十九个
//! 互异包）；跳过限先于聚合（单包超限会污染总账）；聚合先于申报（实测面
//! 立住了才谈得上对照申报）。
//!
//! 闸门次序：名称与段基址 → 码族与判面 → 包表构造 → 聚合统计 → 逐槽对判 →
//! 语料互异 → 逐槽判面表 → 汇总与渲染 → 红档显式 → 文案逐字 → 槽号与覆盖 →
//! 校验器与账本 → 冻结重放四注入。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfc30;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 断言包数。
pub const MAXB: usize = 19;

/// 单包跳过上限。
pub const SKIP_MAX: u8 = 1;

/// 单包条数上限。
pub const CHECK_MAX: u8 = 8;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 缺包。
pub const KT_ESHORT: u32 = SEG_BASE + 0x01;
/// 包号乱序或重号。
pub const KT_EORDER: u32 = SEG_BASE + 0x02;
/// 包内算术不平。
pub const KT_EINNER: u32 = SEG_BASE + 0x03;
/// 跳过超限。
pub const KT_ESKIP: u32 = SEG_BASE + 0x04;
/// 条数申报错。
pub const KT_ETOTAL: u32 = SEG_BASE + 0x05;
/// 通过数申报错。
pub const KT_ECLAIM: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const KT_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const KT_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const KT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const KT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const KT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const KT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const KT_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 取包。
pub const OP_BUNDLE: u32 = 1;
/// 聚合。
pub const OP_TOTAL: u32 = 2;
/// 全绿。
pub const OP_GREEN: u32 = 3;
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

/// 折一个字符串（字节 + 长度）。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(v) => x = fold_byte(x, *v),
            None => break,
        }
        i += 1;
    }
    fold_word(x, b.len() as u64)
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
// 包
// ---------------------------------------------------------------------------

/// 一包断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bundle {
    /// 包号（1..=19）。
    pub id: u8,
    /// 包名。
    pub name: &'static str,
    /// 条数。
    pub checks: u8,
    /// 通过数。
    pub pass: u8,
    /// 跳过数。
    pub skip: u8,
}

impl Bundle {
    /// 构造。
    pub const fn new(id: u8, name: &'static str, checks: u8, pass: u8, skip: u8) -> Bundle {
        Bundle { id, name, checks, pass, skip }
    }

    /// 包内算术平（pass + skip == checks）。
    pub const fn sum_ok(&self) -> bool {
        (self.pass as u16) + (self.skip as u16) == self.checks as u16
    }

    /// 跳过不超限。
    pub const fn skip_ok(&self) -> bool {
        self.skip <= SKIP_MAX
    }

    /// 全绿（无跳过且全过）。
    pub const fn green(&self) -> bool {
        self.skip == 0 && self.pass == self.checks
    }

    /// 包号有效。
    pub const fn id_ok(&self) -> bool {
        self.id >= 1 && (self.id as usize) <= MAXB
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.id as u64);
        h = fold_str(h, self.name);
        h = fold_word(h, self.checks as u64);
        h = fold_word(h, self.pass as u64);
        fold_word(h, self.skip as u64)
    }
}

/// 十九包聚合面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ktest {
    /// 包表。
    pub bundles: [Bundle; MAXB],
    /// 有效包数。
    pub count: u8,
}

/// 各包条数（B21 静态表）。
pub const CHECKS: [u8; MAXB] = [5, 4, 6, 5, 5, 4, 6, 5, 5, 4, 6, 5, 5, 4, 6, 5, 5, 4, 5];

/// 各包名（B21 静态表）。
pub const NAMES: [&str; MAXB] = [
    "magic-base",
    "class-bits",
    "type-gate",
    "phoff-span",
    "phent-size",
    "phnum-count",
    "seg-bounds",
    "seg-overlap",
    "seg-align",
    "seg-congruence",
    "interp-path",
    "interp-exec",
    "needed-name",
    "needed-cycle",
    "reloc-kind",
    "symbol-scope",
    "stack-limit",
    "brk-rule",
    "entry-anchor",
];

impl Ktest {
    /// 空面。
    pub const fn empty() -> Ktest {
        Ktest { bundles: [Bundle::new(1, "?", 0, 0, 0); MAXB], count: 0 }
    }

    /// 全绿面。
    pub const fn green_all() -> Ktest {
        let mut bs = [Bundle::new(1, "?", 0, 0, 0); MAXB];
        let mut i = 0usize;
        while i < MAXB {
            let c = CHECKS[i];
            bs[i] = Bundle::new((i + 1) as u8, NAMES[i], c, c, 0);
            i += 1;
        }
        Ktest { bundles: bs, count: MAXB as u8 }
    }

    /// 改一包跳过数（条数不变，通过数相应减）。
    pub const fn with_skip(mut k: Ktest, i: usize, n: u8) -> Ktest {
        let b = k.bundles[i];
        let pass = b.checks - n;
        k.bundles[i] = Bundle::new(b.id, b.name, b.checks, pass, n);
        k
    }

    /// 改一包通过数（条数与跳过数不变）。
    pub const fn with_pass(mut k: Ktest, i: usize, pass: u8) -> Ktest {
        let b = k.bundles[i];
        k.bundles[i] = Bundle::new(b.id, b.name, b.checks, pass, b.skip);
        k
    }

    /// 改一包包号。
    pub const fn with_id(mut k: Ktest, i: usize, id: u8) -> Ktest {
        let b = k.bundles[i];
        k.bundles[i] = Bundle::new(id, b.name, b.checks, b.pass, b.skip);
        k
    }

    /// 截短包数。
    pub const fn truncated(mut k: Ktest, n: u8) -> Ktest {
        k.count = n;
        k
    }

    /// 有效包数。
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 取一包。
    pub fn get(&self, i: usize) -> Option<&Bundle> {
        if i >= self.len() {
            return None;
        }
        self.bundles.get(i)
    }

    /// 按包号取包。
    pub fn by_id(&self, id: u8) -> Option<&Bundle> {
        let mut i = 0usize;
        while i < self.len() {
            if let Some(b) = self.bundles.get(i) {
                if b.id == id {
                    return Some(b);
                }
            }
            i += 1;
        }
        None
    }

    /// 包号全部有效。
    pub fn ids_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len() {
            match self.bundles.get(i) {
                Some(b) => {
                    if !b.id_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 包号严格递增。
    pub fn ids_ordered(&self) -> bool {
        let mut prev = 0u8;
        let mut i = 0usize;
        while i < self.len() {
            match self.bundles.get(i) {
                Some(b) => {
                    if b.id <= prev {
                        return false;
                    }
                    prev = b.id;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 包号齐全（1..=19 依次）。
    pub fn ids_complete(&self) -> bool {
        if self.len() != MAXB {
            return false;
        }
        let mut i = 0usize;
        while i < self.len() {
            match self.bundles.get(i) {
                Some(b) => {
                    if b.id as usize != i + 1 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 首包内不平的包号。
    pub fn first_inner(&self) -> Option<u8> {
        let mut i = 0usize;
        while i < self.len() {
            if let Some(b) = self.bundles.get(i) {
                if !b.sum_ok() {
                    return Some(b.id);
                }
            }
            i += 1;
        }
        None
    }

    /// 首跳过超限的包号。
    pub fn first_over_skip(&self) -> Option<u8> {
        let mut i = 0usize;
        while i < self.len() {
            if let Some(b) = self.bundles.get(i) {
                if !b.skip_ok() {
                    return Some(b.id);
                }
            }
            i += 1;
        }
        None
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.count as u64);
        let mut i = 0usize;
        while i < self.len() {
            if let Some(b) = self.bundles.get(i) {
                h = fold_word(h, b.seal());
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 申报面与实测面
// ---------------------------------------------------------------------------

/// 申报面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 总条数。
    pub checks: u32,
    /// 总通过数。
    pub pass: u32,
    /// 总跳过数。
    pub skip: u32,
    /// 全绿包数。
    pub green: u32,
}

impl Claim {
    /// 构造。
    pub const fn new(checks: u32, pass: u32, skip: u32, green: u32) -> Claim {
        Claim { checks, pass, skip, green }
    }

    /// 自洽（checks == pass + skip）。
    pub const fn coherent(&self) -> bool {
        self.checks == self.pass + self.skip
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.checks as u64);
        h = fold_word(h, self.pass as u64);
        h = fold_word(h, self.skip as u64);
        fold_word(h, self.green as u64)
    }
}

/// 实测面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// 总条数。
    pub checks: u32,
    /// 总通过数。
    pub pass: u32,
    /// 总跳过数。
    pub skip: u32,
    /// 全绿包数。
    pub green: u32,
}

impl Summary {
    /// 空。
    pub const fn new() -> Summary {
        Summary { checks: 0, pass: 0, skip: 0, green: 0 }
    }

    /// 由聚合面实测。
    pub fn of(k: &Ktest) -> Summary {
        let mut s = Summary::new();
        let mut i = 0usize;
        while i < k.len() {
            if let Some(b) = k.bundles.get(i) {
                s.checks += b.checks as u32;
                s.pass += b.pass as u32;
                s.skip += b.skip as u32;
                if b.green() {
                    s.green += 1;
                }
            }
            i += 1;
        }
        s
    }

    /// 算术平（checks == pass + skip）。
    pub const fn coherent(&self) -> bool {
        self.checks == self.pass + self.skip
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
    /// 缺包。
    Short,
    /// 包号乱序或重号。
    Order,
    /// 包内算术不平。
    Inner,
    /// 跳过超限。
    Skip,
    /// 条数申报错。
    Total,
    /// 通过数申报错。
    Claim,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Short => "short",
            Fault::Order => "order",
            Fault::Inner => "inner",
            Fault::Skip => "skip",
            Fault::Total => "total",
            Fault::Claim => "claim",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Short => KT_ESHORT,
            Fault::Order => KT_EORDER,
            Fault::Inner => KT_EINNER,
            Fault::Skip => KT_ESKIP,
            Fault::Total => KT_ETOTAL,
            Fault::Claim => KT_ECLAIM,
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
    /// 聚合面。
    pub ktest: Ktest,
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
        ktest: Ktest,
        claim: Claim,
        fault: Fault,
        note: &'static str,
    ) -> Slot {
        Slot { name, ktest, claim, fault, note }
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 期望错误码（通过为 0）。
    pub fn expected(&self) -> u32 {
        judge(self).code()
    }

    /// 实测面。
    pub fn summary(&self) -> Summary {
        Summary::of(&self.ktest)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.ktest.seal());
        fold_word(h, self.claim.seal())
    }
}

/// 对判：包数 → 包内 → 包号 → 跳过限 → 聚合 → 申报。
pub fn judge(s: &Slot) -> Fault {
    if s.ktest.len() != MAXB {
        return Fault::Short;
    }
    if s.ktest.first_inner().is_some() {
        return Fault::Inner;
    }
    if !s.ktest.ids_ok() || !s.ktest.ids_ordered() {
        return Fault::Order;
    }
    if s.ktest.first_over_skip().is_some() {
        return Fault::Skip;
    }
    let got = Summary::of(&s.ktest);
    if !got.coherent() {
        return Fault::Inner;
    }
    if s.claim.checks != got.checks {
        return Fault::Total;
    }
    if s.claim.pass != got.pass || s.claim.skip != got.skip || s.claim.green != got.green {
        return Fault::Claim;
    }
    Fault::Ok
}

/// 实测面或错误码。
pub fn judge_summary(s: &Slot) -> Result<Summary, u32> {
    match s.judge() {
        Fault::Ok => Ok(Summary::of(&s.ktest)),
        f => Err(f.code()),
    }
}

// 静态聚合面构造器。

/// 缺包：只留十八包。
const fn short_rows() -> Ktest {
    Ktest::truncated(Ktest::green_all(), 18)
}

/// 重号：第 9 包改成第 8 号。
const fn dup_ids() -> Ktest {
    Ktest::with_id(Ktest::green_all(), 9, 8)
}

/// 包内不平：第 5 包跳过 1 但通过数不扣。
const fn inner_bad() -> Ktest {
    Ktest::with_pass(Ktest::with_skip(Ktest::green_all(), 5, 1), 5, 4)
}

/// 跳过超限：第 3 包跳 2。
const fn skip_two() -> Ktest {
    Ktest::with_skip(Ktest::green_all(), 3, 2)
}

/// 全绿面。
const fn green_all() -> Ktest {
    Ktest::green_all()
}

/// 十四槽。
pub const PAIRS: [Slot; CORPUS] = [
    Slot::new("green-all", green_all(), Claim::new(94, 94, 0, 19), Fault::Ok, "十九包全绿"),
    Slot::new(
        "one-skip",
        Ktest::with_skip(green_all(), 7, 1),
        Claim::new(94, 93, 1, 18),
        Fault::Ok,
        "一包一跳",
    ),
    Slot::new(
        "two-skip",
        Ktest::with_skip(Ktest::with_skip(green_all(), 2, 1), 10, 1),
        Claim::new(94, 92, 2, 17),
        Fault::Ok,
        "两包各一跳",
    ),
    Slot::new(
        "three-skip",
        Ktest::with_skip(Ktest::with_skip(Ktest::with_skip(green_all(), 0, 1), 6, 1), 18, 1),
        Claim::new(94, 91, 3, 16),
        Fault::Ok,
        "三包各一跳",
    ),
    Slot::new(
        "six-skip",
        Ktest::with_skip(
            Ktest::with_skip(
                Ktest::with_skip(
                    Ktest::with_skip(
                        Ktest::with_skip(Ktest::with_skip(green_all(), 1, 1), 4, 1),
                        7,
                        1,
                    ),
                    11,
                    1,
                ),
                14,
                1,
            ),
            17,
            1,
        ),
        Claim::new(94, 88, 6, 13),
        Fault::Ok,
        "六包各一跳",
    ),
    Slot::new(
        "seven-skip",
        Ktest::with_skip(
            Ktest::with_skip(
                Ktest::with_skip(
                    Ktest::with_skip(
                        Ktest::with_skip(
                            Ktest::with_skip(Ktest::with_skip(green_all(), 3, 1), 5, 1),
                            8,
                            1,
                        ),
                        9,
                        1,
                    ),
                    12,
                    1,
                ),
                15,
                1,
            ),
            16,
            1,
        ),
        Claim::new(94, 87, 7, 12),
        Fault::Ok,
        "七包各一跳",
    ),
    Slot::new(
        "last-skip",
        Ktest::with_skip(green_all(), 18, 1),
        Claim::new(94, 93, 1, 18),
        Fault::Ok,
        "末包一跳",
    ),
    Slot::new(
        "ends-skip",
        Ktest::with_skip(Ktest::with_skip(green_all(), 0, 1), 18, 1),
        Claim::new(94, 92, 2, 17),
        Fault::Ok,
        "首末双跳",
    ),
    Slot::new("short-rows", short_rows(), Claim::new(94, 94, 0, 19), Fault::Short, "缺包死"),
    Slot::new("dup-ids", dup_ids(), Claim::new(94, 94, 0, 19), Fault::Order, "重号死"),
    Slot::new("inner-bad", inner_bad(), Claim::new(94, 94, 0, 19), Fault::Inner, "包内不平死"),
    Slot::new("skip-two", skip_two(), Claim::new(94, 94, 0, 19), Fault::Skip, "跳过超限死"),
    Slot::new("total-bad", green_all(), Claim::new(93, 93, 0, 19), Fault::Total, "条数申报错死"),
    Slot::new(
        "claim-bad",
        Ktest::with_skip(green_all(), 7, 1),
        Claim::new(94, 92, 1, 18),
        Fault::Claim,
        "通过数申报错死",
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
    Fault::Inner,
    Fault::Skip,
    Fault::Total,
    Fault::Claim,
];

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
    /// 通过槽条数累计。
    pub checks: u32,
    /// 通过槽通过数累计。
    pub pass: u32,
    /// 通过槽跳过数累计。
    pub skip: u32,
    /// 通过槽全绿包数累计。
    pub green: u32,
    /// 命中数。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, checks: 0, pass: 0, skip: 0, green: 0, matched: 0 }
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

    /// 聚合面是否自洽。
    pub const fn balanced(&self) -> bool {
        self.checks == self.pass + self.skip
    }

    /// 是否全绿。
    pub const fn green_run(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total && self.balanced()
    }

    /// 闸门。
    pub const fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(KT_ENOBS);
        }
        if self.matched != self.total {
            return Err(KT_EVER);
        }
        if !self.balanced() {
            return Err(KT_EBAL);
        }
        Ok(())
    }

    /// 计一槽。
    pub fn tally(&mut self, s: &Slot) {
        self.total += 1;
        if s.judge() == Fault::Ok {
            self.accept += 1;
            let got = Summary::of(&s.ktest);
            self.checks += got.checks;
            self.pass += got.pass;
            self.skip += got.skip;
            self.green += got.green;
        } else {
            self.reject += 1;
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("ktest corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" checks=");
        s.push_str(&u32_text(self.checks));
        s.push_str(" pass=");
        s.push_str(&u32_text(self.pass));
        s.push_str(" skip=");
        s.push_str(&u32_text(self.skip));
        s.push_str(" green=");
        s.push_str(&u32_text(self.green));
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
            return Err(KT_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < PAIRS.len() {
        let s = match PAIRS.get(i) {
            Some(v) => *v,
            None => return Err(KT_ESLOT),
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
        Err(KT_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// ktest 命令行。
pub fn plan_line(k: &Ktest) -> String {
    let s = Summary::of(k);
    let mut out = String::from("ktest b21 --bundles ");
    out.push_str(&u32_text(k.len() as u32));
    out.push_str(" --checks ");
    out.push_str(&u32_text(s.checks));
    out.push_str(" --green ");
    out.push_str(&u32_text(s.green));
    out.push('\n');
    out
}

/// 单行诊断。
pub fn message(s: &Slot) -> String {
    let mut out = String::from("./prog: ktest ");
    match s.judge() {
        Fault::Ok => {
            let got = Summary::of(&s.ktest);
            let mut o = String::from("ok: ");
            o.push_str(s.name);
            o.push_str(" checks=");
            o.push_str(&u32_text(got.checks));
            o.push_str(" pass=");
            o.push_str(&u32_text(got.pass));
            o.push_str(" skip=");
            o.push_str(&u32_text(got.skip));
            o.push_str(" green=");
            o.push_str(&u32_text(got.green));
            o.push('\n');
            return o;
        }
        Fault::Short => {
            out.push_str(s.name);
            out.push_str(": bundles=");
            out.push_str(&u32_text(s.ktest.len() as u32));
            out.push_str(" want ");
            out.push_str(&u32_text(MAXB as u32));
            out.push('\n');
        }
        Fault::Order => {
            out.push_str(s.name);
            out.push_str(": bundle ids duplicated or out of order\n");
        }
        Fault::Inner => {
            out.push_str(s.name);
            out.push_str(": bundle ");
            let mut id = 0u32;
            let mut sum = 0u32;
            let mut checks = 0u32;
            let mut i = 0usize;
            while i < s.ktest.len() {
                if let Some(b) = s.ktest.bundles.get(i) {
                    if !b.sum_ok() {
                        id = b.id as u32;
                        sum = (b.pass as u32) + (b.skip as u32);
                        checks = b.checks as u32;
                        break;
                    }
                }
                i += 1;
            }
            out.push_str(&u32_text(id));
            out.push_str(" pass+skip=");
            out.push_str(&u32_text(sum));
            out.push_str(" != checks=");
            out.push_str(&u32_text(checks));
            out.push('\n');
        }
        Fault::Skip => {
            out.push_str(s.name);
            out.push_str(": bundle ");
            let mut id = 0u32;
            let mut skip = 0u32;
            let mut i = 0usize;
            while i < s.ktest.len() {
                if let Some(b) = s.ktest.bundles.get(i) {
                    if !b.skip_ok() {
                        id = b.id as u32;
                        skip = b.skip as u32;
                        break;
                    }
                }
                i += 1;
            }
            out.push_str(&u32_text(id));
            out.push_str(" skips ");
            out.push_str(&u32_text(skip));
            out.push('\n');
        }
        Fault::Total => {
            let got = Summary::of(&s.ktest);
            out.push_str(s.name);
            out.push_str(": claim checks=");
            out.push_str(&u32_text(s.claim.checks));
            out.push_str(" actual ");
            out.push_str(&u32_text(got.checks));
            out.push('\n');
        }
        Fault::Claim => {
            let got = Summary::of(&s.ktest);
            out.push_str(s.name);
            out.push_str(": claim ");
            out.push_str(&u32_text(s.claim.pass));
            out.push('/');
            out.push_str(&u32_text(s.claim.skip));
            out.push('/');
            out.push_str(&u32_text(s.claim.green));
            out.push_str(" actual ");
            out.push_str(&u32_text(got.pass));
            out.push('/');
            out.push_str(&u32_text(got.skip));
            out.push('/');
            out.push_str(&u32_text(got.green));
            out.push('\n');
        }
    }
    out
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
            self.last = KT_ESTATE;
            return Err(KT_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        match judge_summary(s) {
            Ok(v) => {
                self.last = 0;
                Ok(v)
            }
            Err(c) => {
                self.last = c;
                Err(c)
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
            return Err(KT_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(KT_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(KT_ESLOT)
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
            return Err(KT_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 752, 730, 22, 130, 929];
        const CODES: [u32; 6] = [
            KT_ESHORT, KT_EORDER, KT_EINNER, KT_ESKIP, KT_ETOTAL, KT_ECLAIM,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KT_EBAL),
            };
            if s.slot as usize != i {
                return Err(KT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KT_EBAL);
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
            return Err(KT_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(KT_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(KT_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(KT_ESEAL);
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
    let _ = l.register(count_cell(3, OP_TOTAL, r.map(|x| x.checks as usize)));

    let _ = l.register(count_cell(4, OP_TOTAL, r.map(|x| x.pass as usize)));
    let _ = l.register(count_cell(5, OP_TOTAL, r.map(|x| x.skip as usize)));
    let _ = l.register(count_cell(6, OP_GREEN, r.map(|x| x.green as usize)));

    let red = run_with(Some((0, KT_EVER)));
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
        Fault::Inner.name(),
        Fault::Skip.name(),
        Fault::Total.name(),
        Fault::Claim.name(),
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
    let mut k = 0usize;
    while k < NAMES.len() {
        match NAMES.get(k) {
            Some(n) => {
                if n.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    SEG_BASE == 0xfc30
        && KT_ESHORT == SEG_BASE + 0x01
        && KT_EORDER == SEG_BASE + 0x02
        && KT_EINNER == SEG_BASE + 0x03
        && KT_ESKIP == SEG_BASE + 0x04
        && KT_ETOTAL == SEG_BASE + 0x05
        && KT_ECLAIM == SEG_BASE + 0x06
        && KT_ESTATE == SEG_BASE + 0x07
        && KT_EVER == SEG_BASE + 0x08
        && KT_EBAL == SEG_BASE + 0x09
        && KT_ESLOT == SEG_BASE + 0x0a
        && KT_EVALUE == SEG_BASE + 0x0b
        && KT_ESEAL == SEG_BASE + 0x0c
        && KT_ENOBS == SEG_BASE + 0x0d
}

/// 码族与判面。
fn codes_ok() -> bool {
    let a = Fault::Ok.code() == 0 && Fault::Short.code() == KT_ESHORT;
    let b = Fault::Order.code() == KT_EORDER
        && Fault::Inner.code() == KT_EINNER
        && Fault::Skip.code() == KT_ESKIP
        && Fault::Total.code() == KT_ETOTAL
        && Fault::Claim.code() == KT_ECLAIM;
    let c = Fault::Ok.errno() == 0 && Fault::Inner.errno() == ENOEXEC && !Fault::Ok.alive();
    let d = Bundle::new(1, "a", 5, 4, 1).sum_ok() && !Bundle::new(1, "a", 5, 5, 1).sum_ok();
    let e = Bundle::new(1, "a", 5, 3, 2).skip_ok() == false;
    let f = Bundle::new(1, "a", 5, 5, 0).green() && !Bundle::new(1, "a", 5, 4, 1).green();
    let g = Bundle::new(19, "a", 1, 1, 0).id_ok() && !Bundle::new(20, "a", 1, 1, 0).id_ok();
    a && b && c && d && e && f && g
}

/// 包表构造。
fn bundle_ok() -> bool {
    let k = Ktest::green_all();
    let a = k.len() == MAXB && k.ids_complete() && k.ids_ordered() && k.ids_ok();
    let b = k.get(0).is_some() && k.get(MAXB).is_none() && !k.is_empty();
    let c = match k.by_id(6) {
        Some(b) => b.name == "phnum-count" && b.checks == 4 && b.green(),
        None => false,
    };
    let d = k.by_id(20).is_none() && k.by_id(0).is_none();
    let e = short_rows().len() == 18 && !short_rows().ids_complete();
    let f = !dup_ids().ids_ordered() && dup_ids().ids_ok();
    let g = inner_bad().first_inner() == Some(6) && Ktest::green_all().first_inner().is_none();
    let h = skip_two().first_over_skip() == Some(4)
        && Ktest::green_all().first_over_skip().is_none();
    let i = Ktest::empty().is_empty() && Ktest::empty().get(0).is_none();
    a && b && c && d && e && f && g && h && i
}

/// 聚合统计。
fn stats_ok() -> bool {
    let k = Ktest::green_all();
    let s = Summary::of(&k);
    let a = s.checks == 94 && s.pass == 94 && s.skip == 0 && s.green == 19;
    let b = s.coherent();
    let c = Summary::of(&Ktest::with_skip(k, 7, 1)) == Summary {
        checks: 94,
        pass: 93,
        skip: 1,
        green: 18,
    };
    let d = Summary::of(&Ktest::with_skip(Ktest::with_skip(k, 0, 1), 18, 1)).skip == 2;
    let e = Claim::new(94, 93, 1, 18).coherent() && !Claim::new(94, 94, 1, 18).coherent();
    let f = k.seal() == Ktest::green_all().seal() && k.seal() != skip_two().seal();
    let g = plan_line(&k).as_str() == "ktest b21 --bundles 19 --checks 94 --green 19\n";
    a && b && c && d && e && f && g
}

/// 逐槽对判。
fn judge_ok() -> bool {
    let a = judge(&PAIRS[0]) == Fault::Ok;
    let b = judge(&PAIRS[8]) == Fault::Short;
    let c = judge(&PAIRS[9]) == Fault::Order;
    let d = judge(&PAIRS[10]) == Fault::Inner;
    let e = judge(&PAIRS[11]) == Fault::Skip;
    let f = judge(&PAIRS[12]) == Fault::Total;
    let g = judge(&PAIRS[13]) == Fault::Claim;
    let h = judge_summary(&PAIRS[0]) == Ok(Summary { checks: 94, pass: 94, skip: 0, green: 19 });
    let i = judge_summary(&PAIRS[12]) == Err(KT_ETOTAL);
    let j = PAIRS[0].expected() == 0 && PAIRS[13].expected() == KT_ECLAIM;
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
            if a.ktest == b.ktest && a.claim == b.claim {
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
    let b = r.checks == 752 && r.pass == 730 && r.skip == 22 && r.green == 130;
    let c = r.rate() == 1000 && r.green_run() && r.gate() == Ok(());
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
        == "ktest corpus=14 accept=8 reject=6 checks=752 pass=730 skip=22 green=130 match=14 rate=1000ppm\n"
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, KT_EVER))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(KT_EVER) && !r.green_run();
    let c = run_with(Some((CORPUS, KT_EVER))).err() == Some(KT_ESLOT);
    a && b && c
}

/// 文案。
fn message_ok() -> bool {
    let a = message(&PAIRS[0]).as_str()
        == "ok: green-all checks=94 pass=94 skip=0 green=19\n";
    let b = message(&PAIRS[4]).as_str()
        == "ok: six-skip checks=94 pass=88 skip=6 green=13\n";
    let c = message(&PAIRS[8]).as_str() == "./prog: ktest short-rows: bundles=18 want 19\n";
    let d = message(&PAIRS[9]).as_str()
        == "./prog: ktest dup-ids: bundle ids duplicated or out of order\n";
    let e = message(&PAIRS[10]).as_str() == "./prog: ktest inner-bad: bundle 6 pass+skip=5 != checks=4\n";
    let f = message(&PAIRS[11]).as_str() == "./prog: ktest skip-two: bundle 4 skips 2\n";
    let g = message(&PAIRS[12]).as_str() == "./prog: ktest total-bad: claim checks=93 actual 94\n";
    let h = message(&PAIRS[13]).as_str()
        == "./prog: ktest claim-bad: claim 92/1/18 actual 93/1/18\n";
    a && b && c && d && e && f && g && h
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.name == "green-all" && s.judge() == Fault::Ok,
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.name == "claim-bad" && s.expected() == KT_ECLAIM,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(KT_ENOBS);
    let e = short(Fault::Inner) == "inner" && short(Fault::Ok) == "ok";
    let f = match slot_at(4) {
        Some(s) => s.summary() == Summary { checks: 94, pass: 88, skip: 6, green: 13 },
        None => false,
    };
    a && b && c && d && e && f
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = c.check(&PAIRS[0]) == Ok(Summary { checks: 94, pass: 94, skip: 0, green: 19 });
    let d = c.check(&PAIRS[0]).err() == Some(KT_ESTATE);
    let mut w = Checker::new();
    let e = w.check(&PAIRS[11]).err() == Some(KT_ESKIP);
    let f = w.last() == KT_ESKIP && w.steps() == 1;
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
    let d = m.register(Sample::blank()).err() == Some(KT_ESEAL) && m.rejects() == 1;
    let e = m.freeze().err() == Some(KT_ESEAL) && m.rejects() == 2;
    let mut n = Ledger::new();
    let f = n.replay().err() == Some(KT_ESTATE);
    let g = n.register(Sample::blank()).is_ok() && n.len() == 1;
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && codes_ok() && bundle_ok();
    out[1] = stats_ok() && judge_ok() && table_ok();
    out[2] = corpus_ok() && render_ok() && reds_ok() && message_ok();
    out[3] = slots_ok() && checker_ok() && ledger_ok() && unique() && notes_ok();
    out
}

/// 判据集合。
pub fn run_unx11620_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11620_ktest_agg");

    s.add(
        "KA01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "KA02-码族与判面",
        {
            codes_ok()
        },
        "static",
    );

    s.add(
        "KA03-包表构造",
        {
            bundle_ok()
        },
        "static",
    );

    s.add(
        "KA04-聚合统计",
        {
            stats_ok()
        },
        "static",
    );

    s.add(
        "KA05-逐槽对判",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "KA06-语料互异",
        {
            unique() && notes_ok()
        },
        "static",
    );

    s.add(
        "KA07-逐槽判面表",
        {
            table_ok()
        },
        "static",
    );

    s.add(
        "KA08-汇总与渲染",
        {
            corpus_ok() && render_ok()
        },
        "static",
    );

    s.add(
        "KA09-红档显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "KA10-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "KA11-槽号与覆盖",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "KA12-校验器与账本",
        {
            checker_ok() && ledger_ok()
        },
        "static",
    );

    s.add(
        "KA13-冻结重放四注入",
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
        panic!("unx11620 test fail")
    }

    #[test]
    fn fault_names() {
        assert_eq!(Fault::Ok.name(), "ok");
        assert_eq!(Fault::Short.name(), "short");
        assert_eq!(Fault::Order.name(), "order");
        assert_eq!(Fault::Inner.name(), "inner");
        assert_eq!(Fault::Skip.name(), "skip");
        assert_eq!(Fault::Total.name(), "total");
        assert_eq!(Fault::Claim.name(), "claim");
    }

    #[test]
    fn fault_codes() {
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Short.code(), KT_ESHORT);
        assert_eq!(Fault::Order.code(), KT_EORDER);
        assert_eq!(Fault::Inner.code(), KT_EINNER);
        assert_eq!(Fault::Skip.code(), KT_ESKIP);
        assert_eq!(Fault::Total.code(), KT_ETOTAL);
        assert_eq!(Fault::Claim.code(), KT_ECLAIM);
        assert_eq!(Fault::Claim.errno(), 8);
        assert!(Fault::Inner.alive() && !Fault::Ok.alive());
    }

    #[test]
    fn bundle_rules() {
        assert!(Bundle::new(1, "x", 5, 4, 1).sum_ok());
        assert!(!Bundle::new(1, "x", 5, 5, 1).sum_ok());
        assert!(!Bundle::new(1, "x", 5, 3, 2).skip_ok());
        assert!(Bundle::new(1, "x", 5, 5, 0).green());
        assert!(!Bundle::new(1, "x", 5, 4, 0).green());
    }

    #[test]
    fn bundle_ids() {
        assert!(Bundle::new(1, "x", 1, 1, 0).id_ok());
        assert!(Bundle::new(19, "x", 1, 1, 0).id_ok());
        assert!(!Bundle::new(0, "x", 1, 1, 0).id_ok());
        assert!(!Bundle::new(20, "x", 1, 1, 0).id_ok());
    }

    #[test]
    fn green_all_shape() {
        let k = Ktest::green_all();
        assert_eq!(k.len(), 19);
        assert!(k.ids_complete());
        assert!(k.ids_ordered());
        assert!(k.ids_ok());
        assert_eq!(Summary::of(&k).green, 19);
    }

    #[test]
    fn checks_table_sum() {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < CHECKS.len() {
            sum += CHECKS[i] as u32;
            i += 1;
        }
        assert_eq!(sum, 94);
        assert_eq!(NAMES.len(), MAXB);
    }

    #[test]
    fn by_id_lookup() {
        let k = Ktest::green_all();
        assert_eq!(k.by_id(1).map(|b| b.name), Some("magic-base"));
        assert_eq!(k.by_id(6).map(|b| b.name), Some("phnum-count"));
        assert_eq!(k.by_id(19).map(|b| b.name), Some("entry-anchor"));
        assert!(k.by_id(0).is_none());
        assert!(k.by_id(20).is_none());
    }

    #[test]
    fn with_skip_math() {
        let k = Ktest::with_skip(Ktest::green_all(), 7, 1);
        assert_eq!(Summary::of(&k), Summary { checks: 94, pass: 93, skip: 1, green: 18 });
        let k2 = Ktest::with_skip(k, 18, 1);
        assert_eq!(Summary::of(&k2), Summary { checks: 94, pass: 92, skip: 2, green: 17 });
    }

    #[test]
    fn inner_detect() {
        assert_eq!(inner_bad().first_inner(), Some(6));
        assert!(Ktest::green_all().first_inner().is_none());
        assert_eq!(skip_two().first_over_skip(), Some(4));
        assert!(Ktest::green_all().first_over_skip().is_none());
    }

    #[test]
    fn id_anomalies() {
        assert!(!dup_ids().ids_ordered());
        assert!(dup_ids().ids_ok());
        assert!(!short_rows().ids_complete());
        assert_eq!(short_rows().len(), 18);
    }

    #[test]
    fn claim_coherent() {
        assert!(Claim::new(94, 93, 1, 18).coherent());
        assert!(!Claim::new(94, 94, 1, 18).coherent());
        assert!(Claim::new(94, 94, 0, 19).coherent());
    }

    #[test]
    fn summary_coherent() {
        assert!(Summary { checks: 94, pass: 93, skip: 1, green: 18 }.coherent());
        assert!(!Summary { checks: 94, pass: 94, skip: 1, green: 18 }.coherent());
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
        assert_eq!(PAIRS[10].judge(), Fault::Inner);
        assert_eq!(PAIRS[11].judge(), Fault::Skip);
        assert_eq!(PAIRS[12].judge(), Fault::Total);
        assert_eq!(PAIRS[13].judge(), Fault::Claim);
    }

    #[test]
    fn judge_summary_values() {
        assert_eq!(judge_summary(&PAIRS[0]), Ok(Summary { checks: 94, pass: 94, skip: 0, green: 19 }));
        assert_eq!(judge_summary(&PAIRS[12]), Err(KT_ETOTAL));
        assert_eq!(judge_summary(&PAIRS[10]), Err(KT_EINNER));
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
        assert_ne!(PAIRS[4].seal(), PAIRS[5].seal());
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
        assert_eq!(r.checks, 752);
        assert_eq!(r.pass, 730);
        assert_eq!(r.skip, 22);
        assert_eq!(r.green, 130);
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
            "ktest corpus=14 accept=8 reject=6 checks=752 pass=730 skip=22 green=130 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn report_gate() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(r.balanced() && r.green_run());
        assert_eq!(r.gate(), Ok(()));
        let mut bad = Report::new();
        bad.total = CORPUS as u32;
        bad.matched = CORPUS as u32;
        bad.checks = 10;
        bad.pass = 5;
        assert!(!bad.balanced());
        assert_eq!(bad.gate(), Err(KT_EBAL));
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, KT_EVER))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(KT_EVER));
        assert_eq!(run_with(Some((14, KT_EVER))).err(), Some(KT_ESLOT));
    }

    #[test]
    fn message_exact() {
        assert_eq!(
            message(&PAIRS[0]).as_str(),
            "ok: green-all checks=94 pass=94 skip=0 green=19\n"
        );
        assert_eq!(
            message(&PAIRS[4]).as_str(),
            "ok: six-skip checks=94 pass=88 skip=6 green=13\n"
        );
        assert_eq!(
            message(&PAIRS[12]).as_str(),
            "./prog: ktest total-bad: claim checks=93 actual 94\n"
        );
    }

    #[test]
    fn message_reject_exact() {
        assert_eq!(
            message(&PAIRS[10]).as_str(),
            "./prog: ktest inner-bad: bundle 6 pass+skip=5 != checks=4\n"
        );
        assert_eq!(
            message(&PAIRS[11]).as_str(),
            "./prog: ktest skip-two: bundle 4 skips 2\n"
        );
        assert_eq!(
            message(&PAIRS[13]).as_str(),
            "./prog: ktest claim-bad: claim 92/1/18 actual 93/1/18\n"
        );
    }

    #[test]
    fn plan_line_exact() {
        assert_eq!(
            plan_line(&Ktest::green_all()).as_str(),
            "ktest b21 --bundles 19 --checks 94 --green 19\n"
        );
        assert_eq!(
            plan_line(&Ktest::with_skip(Ktest::green_all(), 7, 1)).as_str(),
            "ktest b21 --bundles 19 --checks 94 --green 18\n"
        );
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        assert_eq!(
            c.check(&PAIRS[0]),
            Ok(Summary { checks: 94, pass: 94, skip: 0, green: 19 })
        );
        assert_eq!(c.check(&PAIRS[0]).err(), Some(KT_ESTATE));
        assert_eq!(c.steps(), 1);
    }

    #[test]
    fn checker_wrong() {
        let mut c = Checker::new();
        assert_eq!(c.check(&PAIRS[13]).err(), Some(KT_ECLAIM));
        assert_eq!(c.last(), KT_ECLAIM);
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
        assert_eq!(l.register(Sample::blank()).err(), Some(KT_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(KT_ESEAL));
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
        assert_eq!(short(Fault::Total), "total");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(752).as_str(), "752");
        assert_eq!(hex_text(0xfc31).as_str(), "0xfc31");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11620_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
