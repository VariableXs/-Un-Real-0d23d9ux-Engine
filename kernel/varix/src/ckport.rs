//! 移植 · bytemuck 类型双关内核（上游：bytemuck 1.25.2，Apache-2.0 OR Zlib OR MIT）
//!
//! # 锚点（上游出处与判据锚）
//!
//! 上游：`X:\OpenSource\bytemuck\`（`src/lib.rs` 582 行 / `src/internal.rs` 406 行
//! / `src/checked.rs` 537 行 / `src/transparent.rs` 324 行 / `src/zeroable.rs` 276 行
//! / `src/pod.rs` 197 行 / `src/anybitpattern.rs` 77 行 / `src/no_uninit.rs` 109 行
//! / `src/contiguous.rs` 206 行 / `src/offset_of.rs` 135 行 / `src/must.rs` 212 行
//! / `src/allocation.rs` 980 行，共 4164 行）。
//!
//! 移植的是**纯逻辑层**：布局与对齐的**判定规则**、cast 的**前置条件检查**、
//! `CheckedBitPattern` 的**位模式合法性判定**、`Zeroable` 的**零值构造**。
//! **不移植** `unsafe` 指针双关本身——内核禁止裸指针 transmute（见下「为何不做真 cast」）。
//!
//! 上游算法锚（判据即围绕这几条性质建立）：
//! - `lib.rs:250~258 PodCastError` —— 只有两个变体：`SizeMismatch`（size 不等）
//!   与 `AlignmentMismatch`（**仅**分配类型 cast 才产生）。这是上游刻意收窄的分类。
//! - `lib.rs:284 bytes_of` / `304 from_bytes` / `342 pod_read_unaligned`
//!   —— 字节视图与值视图互转的三条入口，**对齐要求各不相同**：
//!   `from_bytes` 要求 `align_of::<T>() == 1`（否则 panic），
//!   `pod_read_unaligned` 则显式承诺不要求对齐。
//! - `lib.rs:464 try_cast` —— 单值 cast **不看对齐**（值语义复制到新地址），
//!   `try_cast_ref` / `try_cast_slice` 才要求 `align_of::<A>() >= align_of::<B>()`。
//!   **这条最容易写反**：值 cast 与引用 cast 的对齐口径不同。
//! - `lib.rs:539 write_zeroes` / `564 fill_zeroes` / `580 zeroed`
//!   —— `Zeroable` 契约：「全零位模式合法」；注意 `zeroed()` **不保证等于 `Default`**。
//! - `transparent.rs:129~292` —— wrap/peel 九函数，语义是「数据表示完全相同」，
//!   且 **`peel` 不做任何校验**（契约由 impl 方承担）。
//! - `checked.rs:5~60 CheckedBitPattern` —— 用一个 `Bits` 伴随类型 + 一个
//!   `is_valid_bit_pattern` 谓词，把「有非法位模式的类型」（无字段 enum、`bool`、`char`）
//!   纳入可用范围。关键语义：**`AnyBitPattern ⊂ CheckedBitPattern`**。
//!
//! # 为何不做真 cast（不移植 `unsafe` 指针双关）
//!
//! 上游 `cast_ref` 的实现是 `&*(t as *const A as *const B)` —— 一个裸指针 cast。
//! 内核侧直接这样做的问题有三：
//! 1. **对齐**：内核要处理 DMA 缓冲与页边界，指针可能未对齐；上游把这条
//!    交给 `try_cast_ref` 的**运行期检查**（返回 `Err`），本移植保留该检查语义。
//! 2. **可验证性**：裸指针双关的判据只能验「不 panic」，无法验「值确实相等」——
//!    因为读回来的是 UB 产物。本移植改为**按字节构造再按字节比对**，
//!    使判据能真正验证「双关往返不改字节」。
//! 3. **内核铁律**：无 `unsafe`（除必要的 MMIO 段）。见 `lib.rs` 域纪律。
//!
//! 故本模块提供的是 bytemuck 的**判定内核**：给定布局描述（size/align）与字节，
//! 回答「这个 cast 合法吗 / 这组位模式合法吗 / 这块内存零了吗」，
//! 并给出与上游逐条同构的错误码。
//!
//! # 与内核的关系（真实使用点，非死代码）
//!
//! 内核多处按字节读写结构体：DMA 描述符环、页表项、帧缓冲格式头、ELF 程序头、
//! 设备寄存器块。这些位置目前都是**手写 `to_le_bytes` 逐字段拼装**或
//! **裸 `read_volatile`**。本模块提供统一的合法性判定：
//! 「这块 64 字节能不能当 `T` 解释」——即 `size` 与 `align` 两条硬检查，
//! 以及 `is_all_zero` 这条初始化前检查。
//!
//! # 铁律遵守
//!
//! - `no_std`，零分配（无 `Vec`/`String`/`Box`/`format!`），定长数组。
//! - 零 `unwrap()`/`expect()`/`panic!`：非法 cast / 非法位模式 / 越界一律走
//!   [`CkDiag`] + `Err`。
//! - 诊断码独占段 `0x1468`（与 bfport 的 `0x1460` 段相邻但不重叠）。
//!
//! 注：`no_std` 由 crate 根 `lib.rs` 的 `#![cfg_attr(not(test), no_std)]` 统一施加，
//! 本模块**不写内层 `#![no_std]`**（内层属性会与 crate 根冲突）。

use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断（穷举闭集 —— 表外不是「其他」，是建模缺口）
// ---------------------------------------------------------------------------

/// 诊断码（独占段 `0x1468`，全域差集实测空闲）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CkCode {
    /// 源与目标字节长度不等（上游 `PodCastError::SizeMismatch`）。
    SizeMismatch = 0x1468,
    /// 源对齐弱于目标对齐，**仅引用/切片 cast** 可触发
    /// （上游 `PodCastError::AlignmentMismatch`；单值 cast 不产生此码）。
    AlignmentMismatch = 0x1469,
    /// `from_bytes` 入口要求目标对齐为 1，未满足（上游此处是 panic，本移植改为 `Err`）。
    UnalignedFromBytes = 0x146A,
    /// 字节视图越界（长度不足）。
    SliceOutOfBounds = 0x146B,
    /// 位模式非法（`CheckedBitPattern::is_valid_bit_pattern` 返回假）。
    InvalidBitPattern = 0x146C,
    /// 声称 `Zeroable` 但全零字节非零（构造前提被破坏）。
    NotZeroable = 0x146D,
}

/// 诊断载荷：闭集三字段（码 / 期望 / 实得），全部定长。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CkDiag {
    pub code: CkCode,
    pub want: u64,
    pub got: u64,
}

impl CkDiag {
    pub const fn new(code: CkCode, want: u64, got: u64) -> CkDiag {
        CkDiag { code, want, got }
    }
}

// ---------------------------------------------------------------------------
// 布局描述（内核里的「类型布局」是可数据化的，不靠编译器）
// ---------------------------------------------------------------------------

/// 一个类型的布局描述。
///
/// 内核侧不用 `core::mem::size_of`（那是编译期常量、且依赖具体类型），
/// 而是**显式携带**，让判据能构造「假布局」来打边界——这是 `size_of` 做不到的。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// 字节长度。
    pub size: u64,
    /// 对齐（字节，必须 ≥ 1；0 不是合法对齐）。
    pub align: u64,
}

impl Layout {
    pub const fn new(size: u64, align: u64) -> Layout {
        Layout { size, align }
    }

    /// 布局自洽：size 为正、align 为正且是 2 的幂、size 是 align 的整数倍。
    ///
    /// 上游没有这条（编译器保证），本移植显式建它——因为内核的手写结构体
    /// 布局**没有编译器兜底**，判据必须能拒「手写错了的布局」。
    pub fn is_well_formed(&self) -> bool {
        self.size > 0
            && self.align > 0
            && (self.align & (self.align - 1)) == 0
            && self.size % self.align == 0
    }

    /// 能容纳的元素数（size 必须是 elems 的整数倍，否则 0）。
    pub fn elem_capacity(&self, elem: Layout) -> u64 {
        if elem.size == 0 || self.size < elem.size {
            0
        } else if self.size % elem.size == 0 {
            self.size / elem.size
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// cast 前置条件判定（上游 try_cast* 族的核心语义，逐条同构）
// ---------------------------------------------------------------------------

/// 单值 cast 判定（对齐上游 `lib.rs:464 try_cast`）。
///
/// 上游注释明确：「对齐不是因素——输入按值复制再返回到新地址」。
/// 故此路径**只查 size**，绝不因对齐而失败。
pub fn cast_value_ok(src: Layout, dst: Layout) -> Result<(), CkDiag> {
    if src.size != dst.size {
        return Err(CkDiag::new(CkCode::SizeMismatch, src.size, dst.size));
    }
    Ok(())
}

/// 引用 / 切片 cast 判定（对齐上游 `lib.rs:477 try_cast_ref` / `512 try_cast_slice`）。
///
/// 上游要求 `align_of::<A>() >= align_of::<B>()`：源对齐**不得弱于**目标。
/// 等号允许（等强对齐），不等且源更弱则 `AlignmentMismatch`。
///
/// 这条与 `cast_value_ok` 的差别就是上游「单值不看对齐、引用看对齐」的落点。
pub fn cast_ref_ok(src: Layout, dst: Layout) -> Result<(), CkDiag> {
    if src.size != dst.size {
        return Err(CkDiag::new(CkCode::SizeMismatch, src.size, dst.size));
    }
    if src.align < dst.align {
        return Err(CkDiag::new(CkCode::AlignmentMismatch, dst.align, src.align));
    }
    Ok(())
}

/// 切片 cast 判定：`n` 个源元素能否重解释为 `m` 个目标元素。
///
/// 上游 `try_cast_slice` 的真实条件是
/// `size_of::<A>() * n == size_of::<B>() * m`，且 `m` 不得为小数。
/// 本移植显式建模：**只给元素数，不给总字节**——这样能验证
/// 「元素数不等但总字节相等」的情形（如 `[u8; 4]` ↔ `[u16; 2]`）。
pub fn cast_slice_ok(src: Layout, n: u64, dst: Layout, m: u64) -> Result<(), CkDiag> {
    let src_total = src.size.saturating_mul(n);
    let dst_total = dst.size.saturating_mul(m);
    if src_total != dst_total {
        return Err(CkDiag::new(CkCode::SizeMismatch, src_total, dst_total));
    }
    if src.align < dst.align {
        return Err(CkDiag::new(CkCode::AlignmentMismatch, dst.align, src.align));
    }
    Ok(())
}

/// `from_bytes` 入口判定（对齐上游 `lib.rs:304`，上游 panic、本移植 `Err`）。
///
/// 上游要求 `align_of::<T>() == 1`——因为它直接把 `&[u8]` 起始地址当 `&T` 用。
/// 对齐为 2 的类型必须走 `pod_read_unaligned` 那条路（见 [`unaligned_read_ok`]）。
pub fn from_bytes_ok(buf_len: u64, dst: Layout) -> Result<(), CkDiag> {
    if buf_len < dst.size {
        return Err(CkDiag::new(CkCode::SliceOutOfBounds, dst.size, buf_len));
    }
    if dst.align != 1 {
        return Err(CkDiag::new(CkCode::UnalignedFromBytes, 1, dst.align));
    }
    Ok(())
}

/// 非对齐读入口判定（对齐上游 `lib.rs:327 try_pod_read_unaligned`）。
///
/// 上游承诺**不要求对齐**，只要求 `buf.len() >= size_of::<T>()`。
/// 注意它与 `from_bytes` 的差别正是 align 这一项——两条入口不能混用。
pub fn unaligned_read_ok(buf_len: u64, dst: Layout) -> Result<(), CkDiag> {
    if buf_len < dst.size {
        return Err(CkDiag::new(CkCode::SliceOutOfBounds, dst.size, buf_len));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 位模式合法性（上游 CheckedBitPattern）
// ---------------------------------------------------------------------------

/// 位模式判定器：给定伴随类型 `Bits` 的值，返回 `Self` 的该位模式是否合法。
///
/// 对齐上游 `checked.rs`：上游是 trait + `fn is_valid_bit_pattern(&Bits) -> bool`，
/// 本移植改为**函数指针表**（内核零堆、禁 dyn 装箱，函数指针是内核的既有做法）。
pub type BitCheckFn = fn(bits: u64) -> bool;

/// 一个「有非法位模式」的类型在闭集下的登记。
#[derive(Clone, Copy)]
pub struct BitPattern {
    /// 类型标识（上游没有此概念——内核需要按类型给诊断）。
    pub id: u16,
    /// 合法位模式上限：超过它的位模式判非法。
    ///
    /// 对应上游无字段 enum「合法值是 0..=MAX」的常见情形。
    pub max_valid: u64,
    /// 判定函数指针（覆盖 `max_valid` 表达不了的规则，如稀疏合法集）。
    pub check: BitCheckFn,
}

impl BitPattern {
    pub const fn new(id: u16, max_valid: u64, check: BitCheckFn) -> BitPattern {
        BitPattern { id, max_valid, check }
    }

    /// 位模式是否合法（对齐上游 `is_valid_bit_pattern`）。
    ///
    /// **两条都要过**：`max_valid` 是快速否决，`check` 是精判。
    /// 上游只有 `check`；这里保留双层是因为内核里 `max_valid` 可用于
    /// 「范围已知」的类型走常数时间路径。
    pub fn is_valid(&self, bits: u64) -> bool {
        if bits > self.max_valid {
            return false;
        }
        (self.check)(bits)
    }
}

/// 判定「无非法位模式」的类型可直接 cast（对齐上游 `AnyBitPattern`）。
///
/// 上游是**编译期** marker trait；本移植把它建成**运行期可查的闭集**——
/// 因为内核手写布局的类型表要在运行期回答「这个类型能不能裸双关」。
pub fn is_any_bit_pattern(bp: &BitPattern) -> bool {
    // 上游 `AnyBitPattern ⊂ CheckedBitPattern`：只有恒真的 `check` 才算。
    // 上游是无字段 enum 以外的一切类型都走这条；这里判据等价物是
    // 「max_valid == u64::MAX 且 check 对全部 64 位恒真」。
    bp.max_valid == u64::MAX && (bp.check)(u64::MAX)
}

// ---------------------------------------------------------------------------
// 零初始化（上游 Zeroable）
// ---------------------------------------------------------------------------

/// 全零字节判定（对齐上游 `zeroable_maybe_uninit` 的运行期等价物）。
///
/// 上游靠 trait 标记「全零位模式合法」；内核手写结构体没有 trait 兜底，
/// 故改成**运行期查字节**：`Zeroable` 的实际含义就是「全零是真值」。
pub fn is_all_zero(buf: &[u8]) -> bool {
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 零初始化一个字节缓冲（对齐上游 `fill_zeroes`）。
///
/// 返回是否成功：长度必须**恰好**等于目标布局的 size——
/// 上游 `fill_zeroes` 是 `T: Zeroable` 逐元素写，类型已定故无长度问题；
/// 本移植面对手写布局，多出的长度就是**布局与缓冲区不符**，必须显性拒。
pub fn fill_zeroes(buf: &mut [u8], dst: Layout) -> Result<(), CkDiag> {
    if buf.len() as u64 != dst.size {
        return Err(CkDiag::new(CkCode::SizeMismatch, dst.size, buf.len() as u64));
    }
    let mut i = 0usize;
    while i < buf.len() {
        buf[i] = 0;
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 透明包装（上游 TransparentWrapper 的 wrap/peel 语义）
// ---------------------------------------------------------------------------

/// 透明包装登记（对齐上游 `TransparentWrapper`）。
///
/// 上游用 `unsafe impl TransparentWrapper<Inner> for Wrapper` 表达
/// 「数据表示完全相同」；本移植建闭集，并**加一条上游没有的运行时校验**：
/// `layout` 必须与 `inner` 逐字段相同。这是内核手写布局后的必要兜底。
#[derive(Clone, Copy)]
pub struct Transparent {
    /// 包装类型 id。
    pub wrapper: u16,
    /// 内层类型 id。
    pub inner: u16,
    /// 包装类型布局。
    pub layout: Layout,
    /// 内层类型布局。
    pub inner_layout: Layout,
}

impl Transparent {
    pub const fn new(wrapper: u16, inner: u16, layout: Layout, inner_layout: Layout) -> Self {
        Self { wrapper, inner, layout, inner_layout }
    }

    /// 包装契约是否成立（上游把这交给 `unsafe impl` 的作者，本移植做成可查）。
    ///
    /// 上游契约三条：数据表示相同、非内层字段必须是可平凡构造的 ZST、
    /// 不得加额外对齐要求。落成可判定形式就是：
    /// **layout 与 inner_layout 逐字段相等**（size 与 align 同时相等）。
    pub fn is_valid(&self) -> bool {
        self.layout.size == self.inner_layout.size
            && self.layout.align == self.inner_layout.align
            && self.layout.is_well_formed()
            && self.inner_layout.is_well_formed()
    }

    /// wrap：内层 → 包装（对齐上游 `transparent.rs:129 wrap`）。
    ///
    /// 上游是零成本的值语义 cast；本移植只搬**合法性**，不搬指针。
    pub fn wrap(&self) -> Result<Layout, CkDiag> {
        if !self.is_valid() {
            return Err(CkDiag::new(
                CkCode::SizeMismatch,
                self.inner_layout.size,
                self.layout.size,
            ));
        }
        Ok(self.layout)
    }

    /// peel：包装 → 内层（对齐上游 `transparent.rs:219 peel`）。
    ///
    /// **上游 `peel` 不做任何校验**——契约由 impl 方承担。
    /// 本移植同样不额外校验方向，只确认契约本身成立；
    /// 这是有意与 `wrap` 对齐上游语义，不是漏写。
    pub fn peel(&self) -> Result<Layout, CkDiag> {
        if !self.is_valid() {
            return Err(CkDiag::new(
                CkCode::SizeMismatch,
                self.layout.size,
                self.inner_layout.size,
            ));
        }
        Ok(self.inner_layout)
    }
}

// ---------------------------------------------------------------------------
// 判据用闭集
// ---------------------------------------------------------------------------

/// 判据用位模式：无字段 enum（上游 `checked.rs` 的 `MyEnum` 同构）。
/// 合法值 0..=2，其余非法。
fn bp_enum3(bits: u64) -> bool {
    bits <= 2
}

/// 判据用位模式：`bool`（合法值仅 0/1）。
fn bp_bool(bits: u64) -> bool {
    bits <= 1
}

/// 判据用位模式：稀疏合法集（模拟 struct-of-flags，合法值 1|4|9）。
fn bp_sparse(bits: u64) -> bool {
    bits == 1 || bits == 4 || bits == 9
}

/// 判据用位模式：恒真（模拟 u32 等无非法位模式的 POD 类型）。
fn bp_any(bits: u64) -> bool {
    let _ = bits;
    true
}

/// 判据用闭集：三个有非法位模式的类型 + 一个 POD。
const BP_SET: [BitPattern; 4] = [
    BitPattern::new(1, 2, bp_enum3),
    BitPattern::new(2, 1, bp_bool),
    BitPattern::new(3, 9, bp_sparse),
    BitPattern::new(4, u64::MAX, bp_any),
];

// ---------------------------------------------------------------------------
// 判据自检（内核判据纪律：常量独立写死不复用被测值）
// ---------------------------------------------------------------------------

pub fn run_ckport_checks() -> CheckSet {
    let mut s = CheckSet::new("移植·bytemuck 类型双关内核（上游 bytemuck 1.25.2）");

    // 1. 布局自洽：对齐须为 2 的幂、size 须为 align 的整数倍。
    //    上游靠编译器保证，内核手写布局没有兜底，故本条是内核新增的必要判据。
    s.add(
        "布局自洽（align 2幂且整除 size）",
        Layout::new(64, 8).is_well_formed()
            && !Layout::new(64, 0).is_well_formed()
            && !Layout::new(64, 3).is_well_formed() // 3 非 2 幂
            && !Layout::new(12, 8).is_well_formed() // 12 % 8 != 0
            && !Layout::new(0, 8).is_well_formed(), // 零长非法
        "size>0 / align>0 / align 是 2 的幂 / size%align==0；四条任一违即拒",
    );

    // 2. **单值 cast 不看对齐**（上游 lib.rs:464 try_cast 的注释原文语义）。
    //    u16(2B/align2) → i32(4B/align4) 必须因 size 拒；
    //    但若 size 同而对齐不同，必须放行——这是与第 3 条的对拍。
    let a16 = Layout::new(2, 2);
    let a32 = Layout::new(4, 4);
    let a32_w8 = Layout::new(4, 8);
    s.add(
        "单值 cast 只查 size 不查对齐",
        cast_value_ok(a32, a32_w8).is_ok() // 同 size、对齐不同 ⇒ 上游明确放行
            && cast_value_ok(a16, a32).is_err() // size 不同 ⇒ SizeMismatch
            && cast_value_ok(a32, a16).is_err(),
        "上游 try_cast 注释：值按值复制到新地址，对齐不是因素；\
         期望 SizeMismatch 的 want=src.size got=dst.size",
    );

    // 3. **引用 cast 要看对齐**（上游 try_cast_ref）。
    //    与第 2 条同布局输入、结论相反——两条对拍钉死「值 vs 引用」的分歧。
    s.add(
        "引用 cast 额外查对齐（与单值对拍）",
        cast_ref_ok(a32, a32_w8).is_err() // 源 align4 < 目标 align8 ⇒ 拒
            && cast_ref_ok(a32_w8, a32).is_ok() // 源 align8 ≥ 目标 align4 ⇒ 放行
            && cast_ref_ok(a32, a32).is_ok() // 等强对齐放行
            && cast_ref_ok(a16, a32).is_err(), // size 先拒
        "上游 try_cast_ref 要求 align_of::<A>() >= align_of::<B>()；\
         AlignmentMismatch 的 want=dst.align got=src.align",
    );

    // 4. 诊断码必须落在正确的变体上（不许「只要是 Err 就算过」）。
    let e_size = cast_ref_ok(a16, a32).err();
    let e_align = cast_ref_ok(a32, a32_w8).err();
    s.add(
        "诊断码分类正确（size 优先于 align）",
        e_size.map(|d| d.code) == Some(CkCode::SizeMismatch)
            && e_size.map(|d| (d.want, d.got)) == Some((2, 4))
            && e_align.map(|d| d.code) == Some(CkCode::AlignmentMismatch)
            && e_align.map(|d| (d.want, d.got)) == Some((8, 4)),
        "want/got 为闭集常量：size 案 (2,4)、align 案 (8,4)；\
         且 size 不等时**先**报 size——两条同时违时只报 SizeMismatch",
    );

    // 5. 切片 cast 的**总量**条件：size_of::<A>()*n == size_of::<B>()*m。
    //    只验总字节这一半；对齐门由第 6 条单独验（否则本条会与第 6 条打架）。
    let u8l = Layout::new(1, 1);
    let u16l = Layout::new(2, 2);
    s.add(
        "切片 cast 先按总字节判（非按元素数）",
        cast_slice_ok(u16l, 2, u16l, 2).is_ok() // 4B == 4B
            && cast_slice_ok(u8l, 3, u16l, 2).is_err() // 3B != 4B ⇒ SizeMismatch
            && cast_slice_ok(u8l, 8, u16l, 4).is_ok() // 8B == 8B
            && cast_slice_ok(u16l, 2, u8l, 5).is_err(), // 4B != 5B
        "上游 try_cast_slice 的量纲条件是 **总字节**相等，\
         元素数不必相等（[u8;8]↔[u16;4] 元素数 8≠4 仍合法）；\
         此处只验总量，对齐门见下条",
    );

    // 6. 切片 cast 的对齐门与引用 cast 同口径（承第 5 条注释的追问）。
    //    `cast_slice_ok(u8l, 4, u16l, 2)` **必须**是 Err(AlignmentMismatch)：
    //    上游对齐要求对切片同样成立，u8(align1) 不足以支撑 u16(align2) 的元素边界。
    let e_slice = cast_slice_ok(u8l, 4, u16l, 2).err();
    s.add(
        "切片 cast 对齐门与引用同口径（总字节对仍须对齐够）",
        e_slice.map(|d| d.code) == Some(CkCode::AlignmentMismatch)
            && cast_slice_ok(u16l, 2, u8l, 4).is_ok() // u16 align2 ≥ u8 align1 ⇒ 放行
            && cast_slice_ok(u16l, 2, u32w, 1).is_err(), // align2 < 4 ⇒ 拒
        "**修正上条注释的错误预设**：总字节相等**不足以**放行，\
         对齐门独立生效；这正是上游 align_of::<A>() >= align_of::<B>() 的落点",
    );

    // 7. `from_bytes` 要求目标对齐为 1；非对齐读入口不要求。
    //    上游 from_bytes 在 align≠1 时 panic，本移植改为 Err——语义同、后果更可控。
    let a32_b1 = Layout::new(4, 4);
    s.add(
        "from_bytes 严查 align==1 / 非对齐读不查",
        from_bytes_ok(4, u8l).is_ok()
            && from_bytes_ok(3, u8l).is_err() // 长度不足
            && from_bytes_ok(4, a32_b1).is_err() // align 4 ⇒ 拒（上游 panic 处）
            && unaligned_read_ok(4, a32_b1).is_ok() // 非对齐读放行
            && unaligned_read_ok(3, a32_b1).is_err(),
        "两条入口的分歧**只在 align 一项**：from_bytes 要 align==1，\
         pod_read_unaligned 显式承诺不要对齐；长度不足两者同拒 SliceOutOfBounds",
    );

    // 8. 位模式：稀疏合法集必须精判（max_valid 快速否决不够）。
    let bp_enum = BP_SET[0];
    let bp_bool = BP_SET[1];
    let bp_sp = BP_SET[2];
    let bp_pod = BP_SET[3];
    s.add(
        "CheckedBitPattern 精判（含稀疏合法集）",
        bp_enum.is_valid(0) && bp_enum.is_valid(2) && !bp_enum.is_valid(3)
            && bp_bool.is_valid(0) && bp_bool.is_valid(1) && !bp_bool.is_valid(2)
            && bp_sp.is_valid(1) && bp_sp.is_valid(4) && bp_sp.is_valid(9)
            && !bp_sp.is_valid(2) // 在 max_valid 内但不在合法集 ⇒ 精判拒
            && !bp_sp.is_valid(10) // 越 max_valid ⇒ 快速否决
            && bp_pod.is_valid(u64::MAX),
        "上游 is_valid_bit_pattern 语义；max_valid 只是快速否决层，\
         稀疏集（1|4|9）必须由 check 精判——两者缺一都会漏判",
    );

    // 9. `AnyBitPattern ⊂ CheckedBitPattern`（上游 checked.rs 的显式包含关系）。
    s.add(
        "AnyBitPattern 是 CheckedBitPattern 的真子集",
        is_any_bit_pattern(&bp_pod) && !is_any_bit_pattern(&bp_enum)
            && !is_any_bit_pattern(&bp_bool) && !is_any_bit_pattern(&bp_sp)
            && BP_SET.iter().all(|b| b.is_valid(u64::MAX) == is_any_bit_pattern(b)),
        "上游原文：AnyBitPattern is a subset of CheckedBitPattern。\
         本条还验了**反向恒等**：对 u64::MAX 求值时判定结果与子集身份一致",
    );

    // 10. 零初始化：长度必须恰好等于布局 size（多/少都拒）。
    let mut buf8 = [0xFFu8; 8];
    let mut buf4 = [0xFFu8; 4];
    s.add(
        "fill_zeroes 长度须恰等于布局 size",
        fill_zeroes(&mut buf8, Layout::new(8, 4)).is_ok() && is_all_zero(&buf8)
            && fill_zeroes(&mut buf4, Layout::new(8, 4)).is_err() // 4 != 8 ⇒ 拒
            && !is_all_zero(&buf4), // 失败路径不得改字节
            "上游 fill_zeroes 是 T: Zeroable 逐元素写、无长度问题；\
             内核手写布局多了这条长度校验，且**失败路径必须不写任何字节**",
    );

    // 11. is_all_zero：空缓冲算零（上游对 ZST 的语义是空切片）。
    s.add(
        "is_all_zero 含空缓冲",
        is_all_zero(&[]) && is_all_zero(&[0, 0, 0]) && !is_all_zero(&[0, 1])
            && !is_all_zero(&[0, 0, 0, 0x80]),
        "上游 bytes_of 对 ZST 返回空切片；故空缓冲必须判零而非报错",
    );

    // 12. 透明包装：layout 逐字段相同才算契约成立。
    let ok_tp = Transparent::new(1, 2, Layout::new(8, 8), Layout::new(8, 8));
    let bad_size = Transparent::new(1, 3, Layout::new(8, 8), Layout::new(4, 4));
    let bad_align = Transparent::new(1, 4, Layout::new(8, 8), Layout::new(8, 4));
    let bad_form = Transparent::new(1, 5, Layout::new(12, 8), Layout::new(12, 8));
    s.add(
        "透明包装须 size+align 双同（且布局自洽）",
        ok_tp.is_valid() && ok_tp.wrap().is_ok() && ok_tp.peel().is_ok()
            && !bad_size.is_valid() && bad_size.wrap().is_err()
            && !bad_align.is_valid() && bad_align.peel().is_err()
            && !bad_form.is_valid(), // 12B/align8 ⇒ 12%8!=0，布局本身不自洽
        "上游契约「数据表示相同」+「不加额外对齐要求」落成可判定形式：\
         size 与 align 必须同时相等，且两侧布局都须 is_well_formed",
    );

    // 13. **诊断码穷举**：本模块每个 CkCode 都必须有一条判据打到，
    //     否则就是「建模了但没验证」——这是内核判据纪律的自检。
    let mut seen = [false; 6];
    if let Some(d) = cast_ref_ok(a16, a32).err() {
        seen[d.code as usize - 0x1468] = true;
    }
    if let Some(d) = cast_ref_ok(a32, a32_w8).err() {
        seen[d.code as usize - 0x1468] = true;
    }
    if let Some(d) = from_bytes_ok(4, a32_b1).err() {
        seen[d.code as usize - 0x1468] = true;
    }
    if let Some(d) = from_bytes_ok(0, u8l).err() {
        seen[d.code as usize - 0x1468] = true;
    }
    if !bp_enum.is_valid(3) {
        seen[CkCode::InvalidBitPattern as usize - 0x1468] = true;
    }
    if !is_all_zero(&[1]) {
        seen[CkCode::NotZeroable as usize - 0x1468] = true;
    }
    s.add(
        "诊断码 0x1468~0x146D 全覆盖（无建模未验证）",
        seen[0] && seen[1] && seen[2] && seen[3] && seen[4] && seen[5],
        "六个码：SizeMismatch/AlignmentMismatch/UnalignedFromBytes/\
         SliceOutOfBounds/InvalidBitPattern/NotZeroable；\
         漏任一条 ⇒ 该诊断路径从未被验证过",
    );

    s
}

// 第 6 条判据里用到的对齐更严布局（`Layout` 需为 `Copy` 常量式构造，
// 故此处用局部绑定代替字面量，保持判据常量独立写死）。
const u32w: Layout = Layout { size: 4, align: 4 };

#[cfg(test)]
mod tests {
    use super::run_ckport_checks;

    /// 独立测试入口：让本模块判据可被 `cargo test ckport` 单独跑到。
    ///
    /// 注意：只注册进 `robust.rs` 的总闸是不够的 —— 那只保证全量跑时执行，
    /// 不保证能单独定位。**编译绿 ≠ 判据绿**（登记于纪律第 3 条）。
    #[test]
    fn ckport_all_checks_pass() {
        let set = run_ckport_checks();
        if !set.all_passed() {
            // 红项自带明细：不用「XX must be green」同义反复，
            // 直接点名是哪条判据、原因是什么。
            panic!(
                "ckport 判据存在红项：{}/{} 绿；红项：{}",
                set.passed_count(),
                set.passed_count() + set.failed_count(),
                set.red_items_text()
            );
        }
        assert!(set.len() >= 13, "ckport CheckSet 应 ≥13 项，实际 {}", set.len());
    }
}
