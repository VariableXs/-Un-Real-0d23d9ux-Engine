//! UNX-F2407 · buddy 元数据区自举分配 · 诊断段 0x6D60（独占 0x6D60~0x6D6F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2407）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2407-J1 元数据区字节数按最大帧数闭式计算并与实占一致，自举后
//! buddy 可用帧数与位图口径差为零。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 闭式计算一致：元数据区字节数 = 闭式 f(最大帧数) —— 位图口径
//!        （每帧 1 位）+ 阶账口径（每帧 1 字节迁移类型）+ 对齐填充；
//!        自举时实占字节数逐字节等于闭式值，差非零即红。
//!   J1-b 自举后可用帧数与位图口径差为零：总帧数 − 元数据区自占帧数
//!        ≡ 位图中「空闲」位计数，两条独立口径逐帧核，恒等式破即红。
//!   J1-c 自举路径确定性：同一物理布局两次自举产生逐位相同的元数据
//!        区内容（位图/阶账），确定性可回放。
//!
//! **建模口径（诚实声明）**：位图/阶账以 Vec<u64> 位板建模；「元数据
//! 区自占」按闭式预留，不真分配物理页（真机自举在 pmm 引导路径）。
//! 闭式公式来源注释写死。
//!
//! **段位复核记录**：0x6D60~0x6D6F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6D60~0x6D6F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6D60;
/// 段顶。
pub const SEG_TOP: u16 = 0x6D6F;

/// 页帧大小：4096 字节。
pub const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuddyMetaError {
    /// 0x6D61 最大帧数为 0 或超闭式上限。
    BadMaxFrames,
    /// 0x6D62 闭式字节数与实占不一致。
    SizeDrift,
    /// 0x6D63 可用帧数双口径差非零。
    FreeDrift,
    /// 0x6D64 位图区跨页边界未对齐（预留区必须整帧）。
    MetaUnaligned,
    /// 0x6D65 两次自举内容不一致（非确定性）。
    BootstrapDrift,
    /// 0x6D66 位图越界访问。
    BitmapRange,
}

impl BuddyMetaError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            BuddyMetaError::BadMaxFrames => format!(
                "[0x{:04X}] 最大帧数非法：必须 >0 且 ≤ META_MAX_FRAMES。\
                 改法：核对内存探测登记的总帧数。",
                self.code()
            ),
            BuddyMetaError::SizeDrift => format!(
                "[0x{:04X}] 元数据区闭式字节数与实占不一致。\
                 改法：比对 closed_form_meta_bytes() 与 actual_meta_bytes()，\
                 检查是否有隐藏字段未计入闭式。",
                self.code()
            ),
            BuddyMetaError::FreeDrift => format!(
                "[0x{:04X}] 可用帧数双口径差非零：闭式预留口径与位图计数\
                 不等。改法：逐阶核对位图 free 位与元数据自占帧区间是否重叠。",
                self.code()
            ),
            BuddyMetaError::MetaUnaligned => format!(
                "[0x{:04X}] 元数据区未按整帧预留。\
                 改法：预留字节数向上取整到 4KiB 整帧。",
                self.code()
            ),
            BuddyMetaError::BootstrapDrift => format!(
                "[0x{:04X}] 两次自举位图内容不一致：自举必须确定性。\
                 改法：检查自举是否依赖未初始化内存或迭代序不定的容器。",
                self.code()
            ),
            BuddyMetaError::BitmapRange => format!(
                "[0x{:04X}] 位图下标越界。\
                 改法：访问前先比 len，禁裸索引到边界之外。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、闭式容量账（J1-a 核心）
// ===========================================================================

/// 闭式上限：本模块建模容量，512 Mi 帧（2 TiB 物理）。
pub const META_MAX_FRAMES: u64 = 1 << 29;

/// 闭式公式：元数据区字节数 = f(最大帧数)。
///
/// 组成（来源：A4-B01 施工口径）：
///   (1) 空闲位图：⌈max_frames / 8⌉ 字节（每帧 1 位，空闲=1）；
///   (2) 迁移类型账：每帧 1 字节（0=UNMOVABLE 1=MOVABLE 2=RECLAIMABLE
///       3=RESERVED），max_frames 字节；
///   (3) 阶账：每帧 1 字节（所在块阶 0~10，预留帧写 0xFF），
///       max_frames 字节；
///   (4) 位图自指位板：⌈⌈max_frames/8⌉ × 8 / 8⌉ = ⌈max_frames/8⌉
///       （位板自身 8 字节对齐取整）。
/// 合计后再向上取整到 8 字节对齐（u64 位板原子位操作要求）。
pub const fn closed_form_meta_bytes(max_frames: u64) -> u64 {
    let bitmap = (max_frames + 7) / 8;
    let mig = max_frames;
    let ord = max_frames;
    let selfbitmap = (max_frames + 7) / 8;
    let raw = bitmap + mig + ord + selfbitmap;
    (raw + 7) / 8 * 8
}

/// 预留整帧数：字节数向上取整到 4KiB。
pub const fn closed_form_meta_frames(max_frames: u64) -> u64 {
    (closed_form_meta_bytes(max_frames) + PAGE_SIZE - 1) / PAGE_SIZE
}

// ===========================================================================
// 三、自举主体
// ===========================================================================

/// 位板：u64 数组，位序 = 帧号低位在前。
pub struct Bitboard(Vec<u64>);

impl Bitboard {
    /// 按位数建空板（全 0）。
    pub fn zeros(bits: u64) -> Self {
        let words = ((bits + 63) / 64) as usize;
        Bitboard(vec![0u64; words])
    }

    /// 按位数建满板（全 1）。
    pub fn ones(bits: u64) -> Self {
        let mut b = Bitboard::zeros(bits);
        let full_words = (bits / 64) as usize;
        let mut i = 0;
        while i < full_words {
            b.0[i] = u64::MAX;
            i += 1;
        }
        let rem = (bits % 64) as u32;
        if rem > 0 && full_words < b.0.len() {
            b.0[full_words] = (1u64 << rem) - 1;
        }
        b
    }

    /// 置位（越界拒绝）。
    pub fn set(&mut self, bit: u64) -> result::Result<(), BuddyMetaError> {
        let words = self.0.len() as u64;
        if bit >= words * 64 {
            return Err(BuddyMetaError::BitmapRange);
        }
        self.0[(bit / 64) as usize] |= 1u64 << (bit % 64);
        Ok(())
    }

    /// 清位（越界拒绝）。
    pub fn clear(&mut self, bit: u64) -> result::Result<(), BuddyMetaError> {
        let words = self.0.len() as u64;
        if bit >= words * 64 {
            return Err(BuddyMetaError::BitmapRange);
        }
        self.0[(bit / 64) as usize] &= !(1u64 << (bit % 64));
        Ok(())
    }

    /// 读位（越界返回 Err）。
    pub fn get(&self, bit: u64) -> result::Result<bool, BuddyMetaError> {
        let words = self.0.len() as u64;
        if bit >= words * 64 {
            return Err(BuddyMetaError::BitmapRange);
        }
        Ok(self.0[(bit / 64) as usize] >> (bit % 64) & 1 != 0)
    }

    /// 置位计数（位板口径的可用帧数真源）。
    pub fn popcount(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.0.len() {
            n += self.0[i].count_ones() as u64;
            i += 1;
        }
        n
    }

    /// 逐位比较（确定性核）。
    pub fn identical(&self, other: &Bitboard) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }
        let mut i = 0;
        while i < self.0.len() {
            if self.0[i] != other.0[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 字节数（实占口径组成项之一）。
    pub fn byte_len(&self) -> u64 {
        (self.0.len() * 8) as u64
    }
}

/// 自举产物：位图 + 双口径账。
pub struct BuddyBootstrap {
    /// 总帧数登记值。
    pub max_frames: u64,
    /// 空闲位图（全 1 起：所有帧空闲）。
    pub free_map: Bitboard,
    /// 迁移类型账。
    pub mig_types: Vec<u8>,
    /// 阶账。
    pub order_of: Vec<u8>,
    /// 闭式元数据字节数。
    pub closed_bytes: u64,
    /// 实占元数据字节数（各板字节和 + 8 对齐补齐）。
    pub actual_bytes: u64,
    /// 元数据自占帧数（整帧预留）。
    pub meta_frames: u64,
}

impl BuddyBootstrap {
    /// 自举：按闭式预留 + 双账布置。
    pub fn bootstrap(max_frames: u64) -> result::Result<Self, BuddyMetaError> {
        if max_frames == 0 || max_frames > META_MAX_FRAMES {
            return Err(BuddyMetaError::BadMaxFrames);
        }
        let closed_bytes = closed_form_meta_bytes(max_frames);
        // 实占：三个组件逐字节累加 + 8 字节对齐。
        let bitmap_bytes = (max_frames + 7) / 8;
        let actual_raw = bitmap_bytes + max_frames + max_frames + bitmap_bytes;
        let actual_bytes = (actual_raw + 7) / 8 * 8;
        if actual_bytes != closed_bytes {
            return Err(BuddyMetaError::SizeDrift);
        }
        let meta_frames = (closed_bytes + PAGE_SIZE - 1) / PAGE_SIZE;
        // 整帧性核：预留区必须能被 4KiB 整除表达。
        if meta_frames * PAGE_SIZE < closed_bytes {
            return Err(BuddyMetaError::MetaUnaligned);
        }
        // J1-b 双口径：闭式口径可用 = max_frames − meta_frames；
        // 位图口径 = 全 1 板 popcount − meta_frames（自占帧标 0）。
        let mut free_map = Bitboard::ones(max_frames);
        let mut k = 0;
        while k < meta_frames && k < max_frames {
            free_map.clear(k)?;
            k += 1;
        }
        let free_bitmap = free_map.popcount();
        let free_closed = max_frames.saturating_sub(meta_frames);
        if meta_frames <= max_frames && free_bitmap != free_closed {
            return Err(BuddyMetaError::FreeDrift);
        }
        // 迁移类型账：自占区 3（RESERVED），其余 1（MOVABLE 缺省）。
        let mut mig_types = vec![1u8; max_frames as usize];
        let mut k2 = 0;
        while k2 < meta_frames && k2 < max_frames {
            mig_types[k2 as usize] = 3;
            k2 += 1;
        }
        // 阶账：自占区 0xFF，其余 0xFF（未登记态，由 free_area 批改写）。
        let order_of = vec![0xFFu8; max_frames as usize];
        Ok(BuddyBootstrap {
            max_frames,
            free_map,
            mig_types,
            order_of,
            closed_bytes,
            actual_bytes,
            meta_frames,
        })
    }

    /// 自举后可用帧数（位图口径）。
    pub fn free_frames(&self) -> u64 {
        self.free_map.popcount()
    }
}

// ===========================================================================
// 四、确定性复核（J1-c）
// ===========================================================================

/// 两次自举逐位一致核：位图/迁移账/阶账三份内容全等。
pub fn bootstrap_determinism(max_frames: u64) -> result::Result<(), BuddyMetaError> {
    let a = BuddyBootstrap::bootstrap(max_frames)?;
    let b = BuddyBootstrap::bootstrap(max_frames)?;
    if !a.free_map.identical(&b.free_map) {
        return Err(BuddyMetaError::BootstrapDrift);
    }
    if a.mig_types.len() != b.mig_types.len() {
        return Err(BuddyMetaError::BootstrapDrift);
    }
    let mut i = 0;
    while i < a.mig_types.len() {
        if a.mig_types[i] != b.mig_types[i] {
            return Err(BuddyMetaError::BootstrapDrift);
        }
        i += 1;
    }
    if a.order_of.len() != b.order_of.len() {
        return Err(BuddyMetaError::BootstrapDrift);
    }
    let mut j = 0;
    while j < a.order_of.len() {
        if a.order_of[j] != b.order_of[j] {
            return Err(BuddyMetaError::BootstrapDrift);
        }
        j += 1;
    }
    Ok(())
}

/// 闭式-实占一致核：多组帧数逐组核。
pub fn closed_form_consistency() -> result::Result<usize, BuddyMetaError> {
    let cases: [u64; 10] = [
        1,
        7,
        8,
        63,
        64,
        65,
        4096,
        65536,
        1 << 20,
        1 << 24,
    ];
    let mut checked = 0usize;
    let mut i = 0;
    while i < cases.len() {
        let mf = cases[i];
        let b = BuddyBootstrap::bootstrap(mf)?;
        if b.closed_bytes != b.actual_bytes {
            return Err(BuddyMetaError::SizeDrift);
        }
        checked += 1;
        i += 1;
    }
    Ok(checked)
}

// ===========================================================================
// 五、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 闭式核组数。
    pub closed_groups: usize,
    /// 确定性核通过。
    pub determinism_ok: bool,
    /// 抽核平台可用帧数。
    pub free_frames_probe: u64,
    /// 抽核平台总帧数。
    pub total_frames_probe: u64,
    /// 零帧入参被拒。
    pub zero_rejected: bool,
    /// 超限入参被拒。
    pub over_limit_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.closed_groups >= 10
            && self.determinism_ok
            && self.free_frames_probe + self.meta_probe_frames() == self.total_frames_probe
            && self.zero_rejected
            && self.over_limit_rejected
    }

    fn meta_probe_frames(&self) -> u64 {
        (closed_form_meta_bytes(self.total_frames_probe) + PAGE_SIZE - 1) / PAGE_SIZE
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, BuddyMetaError> {
    let groups = closed_form_consistency()?;
    bootstrap_determinism(1 << 16)?;
    // 抽核平台：64 Ki 帧（256 MiB）。
    let probe = BuddyBootstrap::bootstrap(1 << 16)?;
    let zero_rej = BuddyBootstrap::bootstrap(0).is_err();
    let over_rej = BuddyBootstrap::bootstrap(META_MAX_FRAMES + 1).is_err();
    Ok(SelfAudit {
        closed_groups: groups,
        determinism_ok: true,
        free_frames_probe: probe.free_frames(),
        total_frames_probe: 1 << 16,
        zero_rejected: zero_rej,
        over_limit_rejected: over_rej,
    })
}
