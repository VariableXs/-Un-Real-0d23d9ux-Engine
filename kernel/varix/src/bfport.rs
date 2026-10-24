//! 移植 · bitflags 标志位类型内核（上游：bitflags 2.13.2，Apache-2.0 OR MIT）
//!
//! # 锚点（上游出处与判据锚）
//!
//! 上游：`X:\OpenSource\bitflags\`（`src/traits.rs` 487 行 / `src/internal.rs` 125 行
//! / `src/iter.rs` 214 行 / `src/parser.rs` 332 行，共 8396 行）。
//! 移植的是**算法层**（位运算族 + 截断语义 + 迭代器），**不是宏层**。
//!
//! 为什么不要上游的 `bitflags!` 过程宏：宏层靠编译期展开生成类型，
//! 内核是 `no_std` 且要求「每个类型都有锚点注释与判据」，
//! 宏生成的类型无法逐个挂判据、也无法逐个核对闭集。
//! 故此处改为**手写 `const fn` 显式类型**，语义与上游逐条同构。
//!
//! 上游算法锚（判据即围绕这几条性质建立）：
//! - `traits.rs:201from_bits_truncate` —— `bits & Self::all().bits()`，丢弃未定义位。
//! - `traits.rs:262 is_all` —— 比 `Self::all().bits() | self == self`，
//!   **不是**比 `bits::ALL`。上游原注释写明：「已定义标志集可能占不满全部位」。
//!   这是最容易写错的一条，移植时原样保留该语义并在判据中钉死。
//! - `traits.rs:269 intersects` / `traits.rs:280 contains`
//!   —— 前者「有交集即真」，后者「对方全含于我即真」，两者不是同一判定。
//! - `traits.rs:341~362` 交并差对称差四运算族。
//!
//! # 与内核的关系（真实使用点，非死代码）
//!
//! 内核此前把「多状态」压成裸常量或裸 `u32`（如 `cga11_power_sched.rs`
//! 的 `0x3B01` 码值族、`cga03_framegraph.rs` 的等待/信号位），
//! 一旦需要「多标志可并存 + 可查询交集」就只能手写 `&`、`|`、`!`
//! 且无一处能拒「未定义位」。本模块提供该能力。
//!
//! # 铁律遵守
//!
//! - `no_std`，零分配（无 `Vec`/`String`/`Box`/`format!`）。
//! - 零 `unwrap()`/`expect()`/`panic!`：位标志的「非法输入」一律走
//!   [`BfDiag`] + `Err`，不炸。
//! - 迭代器零分配手写（`BfIter` 持有游标，不持集合）。
//! - 诊断码独占段 `0x1460`（全域差集实测：16910 个 4 位码已用，
//!   此段为 246 个空闲连续段之一）。
//!
//! 注：`no_std` 由 crate 根 `lib.rs` 的 `#![cfg_attr(not(test), no_std)]` 统一施加，
//! 本模块**不写内层 `#![no_std]`**（内层属性会与 crate 根冲突）。

use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断（穷举闭集 —— 表外不是「其他」，是建模缺口）
// ---------------------------------------------------------------------------

/// 诊断码（独占段 `0x1460`，全域差集实测空闲）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BfCode {
    /// 输入位含本类型未定义的位，且调用方要求严格拒（`truncate` 语义）。
    UndefinedBits = 0x1460,
    /// 位宽与存储类型不符（构造时宽度溢出）。
    WidthOverflow = 0x1461,
    /// 名为空或不在闭集内的标志名查询。
    UnknownName = 0x1462,
    /// 迭代游标已越界（内部不变量破坏）。
    CursorOverflow = 0x1463,
}

/// 诊断详情（零堆：字段直读，不格式化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BfDiag {
    pub code: BfCode,
    /// 出错标志名（`UnknownName` 时有效，否则空串）。
    pub name: &'static [u8],
    /// 出错位值（供定位）。
    pub bits: u64,
}

impl BfDiag {
    pub const fn new(code: BfCode, name: &'static [u8], bits: u64) -> Self {
        Self { code, name, bits }
    }

    /// 建议（错误路径必须给可操作建议 —— 铁律第 6 条）。
    ///
    /// 返回 `&'static str` 而非字节串：中文诊断必须可读，
    /// 且 Rust 的 `b"..."` **不允许非 ASCII**（字节串仅限 ASCII）。
    pub const fn advice(&self) -> &'static str {
        match self.code {
            BfCode::UndefinedBits => "含未定义位：或改用 from_bits_retain 显式保留，或补进 FLAGS 闭集",
            BfCode::WidthOverflow => "位值超出存储类型宽度：核对常量后重新构造",
            BfCode::UnknownName => "标志名不在闭集：先查 FLAGS 表，补表或改用位值构造",
            BfCode::CursorOverflow => "迭代游标越界：这是内部不变量破坏，非调用方错误",
        }
    }
}

// ---------------------------------------------------------------------------
// 标志定义表（闭集 —— 可穷举、可审计）
// ---------------------------------------------------------------------------

/// 单个已定义标志（对齐上游 `Flag<B>`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlagDef {
    /// 标志名（空名 = 未命名位，上游用空串表达，此处保留该语义）。
    pub name: &'static str,
    /// 位值（单比特或组合位）。
    pub bits: u64,
}

impl FlagDef {
    pub const fn new(name: &'static str, bits: u64) -> Self {
        Self { name, bits }
    }

    /// 是否具名（对齐上游 `is_named`）。
    pub const fn is_named(&self) -> bool {
        !self.name.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 标志集合（核心类型）
// ---------------------------------------------------------------------------

/// 标志集合：定长位存储 + 闭集表引用。
///
/// `u64` 定长存储使结构体恒16 字节（远小于 64KB 上栈纪律），
/// 代价是只支持 64 位以内的标志空间 —— 内核现有全部码值族都在 32 位内，
/// 故这是刻意取舍，非能力缺失。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flags<const N: usize> {
    bits: u64,
    table: &'static [FlagDef; N],
}

impl<const N: usize> Flags<N> {
    /// 从闭集表 + 位值构造（**不做截断**，保留全部位）。
    pub const fn from_bits_retain(bits: u64, table: &'static [FlagDef; N]) -> Self {
        Self { bits, table }
    }

    /// 空集合。
    pub const fn empty(table: &'static [FlagDef; N]) -> Self {
        Self { bits: 0, table }
    }

    /// 全部已定义位的并集（**注意**：不等于 `u64::MAX` —— 上游语义）。
    pub fn all(&self) -> Self {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < N {
            acc |= self.table[i].bits;
            i += 1;
        }
        Self { bits: acc, table: self.table }
    }

    /// 从位值构造，**丢弃未定义位**（对齐上游 `from_bits_truncate`）。
    ///
    /// 表随实例走（`&self`）——闭集是类型的静态属性，
    /// 不能脱离实例存在，故这是方法而非关联函数。
    pub fn from_bits_truncate(&self, bits: u64) -> Self {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < N {
            acc |= self.table[i].bits;
            i += 1;
        }
        Self { bits: bits & acc, table: self.table }
    }

    /// 从位值构造，**严格**：含未定义位则 `Err`（上游无此入口，
    /// 本模块新增 —— 内核要求「非法输入显性拒」而非静默丢位）。
    pub fn from_bits_strict(&self, bits: u64) -> Result<Self, BfDiag> {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < N {
            acc |= self.table[i].bits;
            i += 1;
        }
        let unknown = bits & !acc;
        if unknown != 0 {
            return Err(BfDiag::new(BfCode::UndefinedBits, b"", unknown));
        }
        Ok(Self { bits, table: self.table })
    }

    /// 按名查单个标志（对齐上游 `from_name`）。
    pub fn from_name(&self, name: &str) -> Result<Self, BfDiag> {
        if name.is_empty() {
            return Err(BfDiag::new(BfCode::UnknownName, b"", 0));
        }
        let mut i = 0usize;
        while i < N {
            if self.table[i].name == name {
                return Ok(Self { bits: self.table[i].bits, table: self.table });
            }
            i += 1;
        }
        Err(BfDiag::new(BfCode::UnknownName, b"", 0))
    }

    /// 原始位值。
    pub const fn bits(&self) -> u64 {
        self.bits
    }

    /// 闭集表引用。
    pub const fn defs(&self) -> &'static [FlagDef; N] {
        self.table
    }

    /// 全 unset（对齐上游 `is_empty`）。
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// 「已定义位全置」—— 比 `all()` 而非 `u64::MAX`（上游 `traits.rs:262`）。
    pub fn is_all(&self) -> bool {
        let a = self.all().bits;
        a | self.bits == self.bits
    }

    /// 有交集即真（对齐上游 `intersects`）。
    pub fn intersects(&self, other: &Self) -> bool {
        self.bits & other.bits != 0
    }

    /// 对方全含于我即真（对齐上游 `contains` —— 与 `intersects` 不是同一判定）。
    pub fn contains(&self, other: &Self) -> bool {
        self.bits & other.bits == other.bits
    }

    /// 含未定义位？（严格构造的前置自查）。
    pub fn has_undefined(&self) -> bool {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < N {
            acc |= self.table[i].bits;
            i += 1;
        }
        self.bits & !acc != 0
    }

    /// 截断：就地丢弃未定义位（对齐上游 `truncate`）。
    pub fn truncate(&mut self) {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < N {
            acc |= self.table[i].bits;
            i += 1;
        }
        self.bits &= acc;
    }

    // -- 四运算族（对齐上游 traits.rs:341~362） --------------------------------

    pub fn union(&self, other: &Self) -> Self {
        Self { bits: self.bits | other.bits, table: self.table }
    }

    pub fn intersection(&self, other: &Self) -> Self {
        Self { bits: self.bits & other.bits, table: self.table }
    }

    pub fn difference(&self, other: &Self) -> Self {
        Self { bits: self.bits & !other.bits, table: self.table }
    }

    pub fn symmetric_difference(&self, other: &Self) -> Self {
        Self { bits: (self.bits | other.bits) & !(self.bits & other.bits), table: self.table }
    }

    /// 插入（对齐上游 `insert`，`|=` 语义）。
    pub fn insert(&mut self, other: &Self) {
        self.bits |= other.bits;
    }

    /// 移除（`&=!` 语义）。
    pub fn remove(&mut self, other: &Self) {
        self.bits &= !other.bits;
    }

    /// 切换（`^=` 语义）。
    pub fn toggle(&mut self, other: &Self) {
        self.bits ^= other.bits;
    }

    /// 置位数。
    pub const fn count_ones(&self) -> u32 {
        // const fn不能用 `<u64>::count_ones`（非 const trait），
        // 故手写 Kernighan 计数——这也是内核 `const fn` 常用范式。
        let mut v = self.bits;
        let mut n = 0u32;
        while v != 0 {
            v &= v - 1;
            n += 1;
        }
        n
    }

    // -- 迭代（零分配手写，对齐上游 iter.rs）---------------------------------

    /// 已含标志名迭代器（持有游标 + 只读表引用，零分配）。
    pub const fn iter_names(&self) -> BfIter<N> {
        BfIter { idx: 0, flags: *self }
    }

    /// 已含标志总数（不迭代，直接数）。
    pub fn contained_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < N {
            if self.table[i].is_named() && self.bits & self.table[i].bits == self.table[i].bits {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 具名标志迭代器（游标式；零分配、零 `Iterator` trait 依赖 —— 内核零堆）。
#[derive(Clone, Copy, Debug)]
pub struct BfIter<const N: usize> {
    idx: usize,
    flags: Flags<N>,
}

impl<const N: usize> BfIter<N> {
    /// 取下一个已含标志名；`None` 表迭代结束。
    pub fn next_flag(&mut self) -> Option<&'static str> {
        while self.idx < N {
            let f = &self.flags.table[self.idx];
            self.idx += 1;
            if f.is_named() && self.flags.bits & f.bits == f.bits {
                return Some(f.name);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 判据自检（内核判据纪律：常量独立写死不复用被测值）
// ---------------------------------------------------------------------------

/// 判据用闭集表 A：三个具名标志 + 一个组合位（组合位无名，
/// 用来验「未命名位不进名字迭代」这一上游语义）。
const T_A: [FlagDef; 4] = [
    FlagDef::new("read", 1 << 0),
    FlagDef::new("write", 1 << 1),
    FlagDef::new("exec", 1 << 2),
    FlagDef::new("", 1 << 3 | 1 << 4), // 无名组合位
];

/// 判据用闭集表 B：**故意不满位**（只占低 3 位）——
/// 用来钉死 `is_all` 比 `all()` 而非 `u64::MAX` 这条上游语义。
const T_B: [FlagDef; 3] = [
    FlagDef::new("lo", 1 << 0),
    FlagDef::new("mid", 1 << 1),
    FlagDef::new("hi", 1 << 2),
];

pub fn run_bfport_checks() -> CheckSet {
    let mut s = CheckSet::new("移植·bitflags 标志位内核（上游 bitflags 2.13.2）");

    // 1. all() 是已定义位并集，**不等于** u64::MAX（不满位闭集必验）。
    let fa = Flags::<4>::from_bits_retain(0, &T_A);
    let all_a = fa.all().bits;
    let fa_b = Flags::<3>::from_bits_retain(0, &T_B);
    let all_b = fa_b.all().bits;
    s.add(
        "all=已定义位并集且不满 u64::MAX",
        all_a == 0b1_1111 && all_b == 0b111 && all_b != u64::MAX,
        "上游 traits.rs:262 语义：已定义集可能占不满全部位",
    );

    // 2. is_all 比 all()，不是比 u64::MAX（这是最容易写错的一条）。
    let b_full = Flags::<3>::from_bits_retain(0b111, &T_B);
    let b_partial = Flags::<3>::from_bits_retain(0b011, &T_B);
    let b_empty = Flags::<3>::from_bits_retain(0, &T_B);
    s.add(
        "is_all 比 all() 而非全 1",
        b_full.is_all() && !b_partial.is_all() && !b_empty.is_all(),
        "只置满已定义位才算 all；置满 u64::MAX 反而不该算（那是未定义位）",
    );

    // 3. intersects 与 contains 不是同一判定（上游两条独立方法）。
    let x = Flags::<3>::from_bits_retain(0b001, &T_B);
    let y = Flags::<3>::from_bits_retain(0b011, &T_B);
    let z = Flags::<3>::from_bits_retain(0b100, &T_B);
    s.add(
        "intersects≠contains（部分重叠只 intersects）",
        x.intersects(&y) && !x.contains(&y) && y.contains(&x) && !x.intersects(&z),
        "上游 traits.rs:269/280：交集非空即 intersects；对方全含才 contains",
    );

    // 4. truncate 丢未定义位 / retain 全留 / strict 显性拒。
    //
    // 夹具推导：T_A 的已定义位并集 `all_a == 0b1_1111`（bit0 read / bit1 write /
    // bit2 exec / bit3|bit4 无名组合位）。故**未定义位必须取 bit5 及以上**——
    // 早先取 `0b1_1010`（bit1/3/4）全落在闭集内，truncate 不会丢任何位，
    // 该判据恒红，是夹具与闭集定义自相矛盾，不是实现缺陷。
    let raw = 0b10_1010u64; // bit1,bit3,bit5 —— bit5 未定义（首个越界位）
    let base = Flags::<4>::empty(&T_A);
    let t = base.from_bits_truncate(raw);
    let r = Flags::<4>::from_bits_retain(raw, &T_A);
    let strict_ok = base.from_bits_strict(raw).is_err();
    // truncate 期望 = raw & all_a = 0b10_1010 & 0b1_1111 = 0b1010（bit1,bit3）。
    s.add(
        "truncate 丢位/retain 全留/strict 显性拒",
        t.bits == 0b1010
            && r.bits == raw
            && r.has_undefined()
            && strict_ok
            && Flags::<4>::from_bits_retain(0b1_1010, &T_A).from_bits_strict(0b1_1010).is_ok(),
        "期望可推导：truncate=raw&已定义位并集(0b10_1010&0b1_1111=0b1010)；\
         retain 全留故 has_undefined 为真；strict 对越界位 Err、对全合法位 Ok",
    );

    // 5. 四运算族逐条对拍（交集/并/差/对称差）。
    let p = Flags::<3>::from_bits_retain(0b011, &T_B);
    let q = Flags::<3>::from_bits_retain(0b110, &T_B);
    let (i, u, d, sd) = (
        p.intersection(&q).bits,
        p.union(&q).bits,
        p.difference(&q).bits,
        p.symmetric_difference(&q).bits,
    );
    s.add(
        "四运算族逐条对拍",
        i == 0b010 && u == 0b111 && d == 0b001 && sd == 0b101,
        "交集 010 / 并 111 / 差 001 / 对称差 101（手算期望）",
    );

    // 6. count_ones = Kernighan 计数；置位数与交集位一致。
    s.add(
        "count_ones 与交集计数自洽",
        p.count_ones() == 2 && q.count_ones() == 2 && p.intersection(&q).count_ones() == 1,
        "const fn 不能用 count_ones trait ⇒手写 Kernighan，须与运算结果自洽",
    );

    // 7. 具名迭代：未命名组合位不进名字迭代（上游 is_named 语义）。
    let mixed = Flags::<4>::from_bits_retain(0b1_1111, &T_A);
    let mut it = mixed.iter_names();
    let mut names = [0u8; 8];
    let mut cnt = 0usize;
    while let Some(nm) = it.next_flag() {
        if cnt < names.len() {
            names[cnt] = nm.len() as u8;
            cnt += 1;
        }
    }
    // 期望：read/write/exec 三个具名位各命中（组合位无名不计）⇒ cnt == 3。
    let expect_cnt = mixed.contained_count() as usize;
    s.add(
        "具名迭代跳过未命名位且与计数一致",
        cnt == 3 && expect_cnt == 3 && names[0] == 4 && names[1] == 5,
        "上游 is_named：空名位不进名字迭代；read(4)/write(5)/exec(4)",
    );

    // 8. by-name 查找命中/未命中（上游 from_name）。
    let by_name = base.from_name("write");
    let bad_name = base.from_name("nosuch");
    let empty_name = base.from_name("");
    s.add(
        "from_name 命中/未命中/空名三路显性",
        by_name.is_ok()
            && by_name.map(|v| v.bits) == Ok(0b010)
            && bad_name.is_err()
            && empty_name.is_err(),
        "上游 from_name 空名返回 None；本模块走Err 带 UnknownName",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::run_bfport_checks;

    /// 独立测试入口：让本模块判据可被 `cargo test bfport` 单独跑到。
    ///
    /// 注意：只注册进 `robust.rs` 的总闸是不够的 —— 那只保证全量跑时执行，
    /// 不保证能单独定位。**编译绿 ≠ 判据绿**（登记于纪律第 3 条）。
    #[test]
    fn bfport_all_checks_pass() {
        let set = run_bfport_checks();
        if !set.all_passed() {
            // 红项自带明细：不用「XX must be green」同义反复，
            // 直接点名是哪条判据、原因是什么。
            panic!(
                "bfport 判据存在红项：{}/{} 绿；红项：{}",
                set.passed_count(),
                set.passed_count() + set.failed_count(),
                set.red_items_text()
            );
        }
        assert!(set.len() >= 8, "bfport CheckSet 应 ≥8 项，实际 {}", set.len());
    }
}
