//! ═══════════════════════════════════════════════════════════════════════════
//! UNX-F0306 · 权限位策略引擎（W^X 矩阵）
//! 诊断段：0x1E50 ~ 0x1E5F（独占）
//!
//! 诊断段占用核查记录（grep 全仓结论，2026-10-24）：
//!   - `grep -rn "0x1E5" kernel/`：仅命中 4 处 u64 魔数（reghive.rs 哈希种子
//!     0xCBFF_1E5A、unx2562.rs 种子 0x2C1E5545…、unx0435.rs 哨兵 0xDEAD_1E50、
//!     unx685.rs LCG 种子 0x68B1_056A_1E5E_C0DE），均非 u16 诊断码段；
//!   - `grep -n "1E5" varix/src/segs_all.txt`：零命中；
//!   - unxstar 内 SEG_BASE 清单：…0x1E40 / 0x1E80…，0x1E50 无主；
//!   - 结论：0x1E50~0x1E5F 全仓无 u16 诊断码占用 → 本模块取 0x1E50。
//!
//! 验收锚点（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//!           Unxreal.md 第 1748 行，原文全文抄录）：
//!   | UNX-F0306 | 权限位策略引擎（W^X 矩阵） | 300 | 已深化 | UNX-F0306-J1 区域类型→RWX 矩阵全过，W^X 违例（可写可执行）检出 100% |
//!
//! 判据 UNX-F0306-J1 分解：
//!   (a) 区域类型→RWX 矩阵全过：RegionType 八类闭集（内核代码段/内核只读数据/
//!       内核可写数据/任务栈/任务堆/设备 MMIO/页表自身/动态代码终态）逐类有
//!       冻结的 RWX 矩阵行；矩阵审计（表↔const 双源对拍 + 行内不含 W+X +
//!       行必含 R）8/8 全过；合法子集请求（请求位 ⊆ 矩阵行）逐类放行；
//!   (b) W^X 违例（可写可执行，任务文"W+A 同时"之 A 即执行位 X）检出 100%：
//!       编译期逐行断言矩阵行不含 W+X；运行时任何请求含 W+X 一律红案
//!       （优先级最高，先于矩阵子集判定）；W^X 红线全扫描 8 类 × {W+X,
//!       R+W+X} = 16 案 16 拒；注入面复测 100%；
//!   (c) 裁决账：每次裁决留痕（类型/请求位/结论/码/W^X 红线位）+ 拒收率
//!       （千分比）统计 + 分类型直方图；
//!   (d) 权限变更路径（mprotect 语义）：类型不变改权限，变更同样过矩阵；
//!       降权（新 ⊆ 旧）自由放行，升权（新增任何位）一律拒收；
//!   (e) 例外通道（显式双映射豁免）：同物理页双映射（一 W 别名 R+W、
//!       一 X 别名 R+X）需显式登记 + 豁免账 + 撤销；豁免默认 0 为红线
//!       （引擎出厂 active=0 且留痕）；
//!   (f) 事件账 + health() + FNV-1a 指纹 + 双跑确定性对拍 + 注入面
//!       （W^X 违例注入/越权升级注入/未登记豁免注入各 100% 检出）。
//!
//! 纪律：no_std+alloc；零 unwrap/expect/panic!/运行期 assert!（编译期
//!       const assert 除外）；可失败全走 Result<T, WxMatrixError>；溢出
//!       显性 saturating/wrapping；>64KB 结构全走堆；const fn 内禁
//!       slice::get / Vec 索引 / Ord::min/max / derive-PartialEq 的 ==
//!       / for 循环（一律 while+索引）。
//! ═══════════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════════
// 常量段（来源注释：x86-64 硬件冻结值 + 本仓 F0301~F0318 系列设计冻结值）
// ═══════════════════════════════════════════════════════════════════════════

/// 4K 标准页 = 2^12 字节（x86-64 PT 叶项粒度，硬件冻结值；与 F0305 同源）。
pub const PAGE_4K: u64 = 0x1000;
/// 4K 对齐掩码（低 12 位清零；x86-64 页粒度冻结值）。
pub const PAGE_4K_MASK: u64 = 0x0FFF;

/// 本模块诊断段基址（0x1E50，独占段 0x1E50~0x1E5F）。
pub const SEG_BASE: u16 = 0x1E50;
/// 任务编号（UNX-F0306）。
pub const MODULE_ID: u16 = 0x0306;
/// 任务标签。
pub const TASK_TAG: &str = "UNX-F0306";

// ── x86-64 PTE 权限位（来源：Intel SDM Vol.3A §4.5 页表项格式；EFER.NXE
//    位来源同卷 §4.5.1，本仓 F0307 NXE 使能序列消费方）────────────────────
/// PTE Present 位（bit 0）。
pub const PTE_P: u64 = 1 << 0;
/// PTE Read/Write 位（bit 1）：置 1 = 可写，清 0 = 只读。
pub const PTE_RW: u64 = 1 << 1;
/// PTE User/Supervisor 位（bit 2）：置 1 = 用户态可访，清 0 = 仅内核态。
pub const PTE_US: u64 = 1 << 2;
/// PTE NX 位（bit 63）：置 1 = 不可执行（EFER.NXE=1 前提下生效，见 F0307）。
pub const PTE_NX: u64 = 1 << 63;

// ── 账面容量冻结值（堆账上限，防无界增长；越限拒绝服务而非 OOM）──────────
/// 区登记上限：RegionRec 约 48B × 256 ≈ 12KB 堆账。
pub const MAX_REGIONS: usize = 256;
/// 裁决账上限：DecisionRec 约 40B × 1024 ≈ 40KB 堆账，越限走丢包计数。
pub const MAX_DECISIONS: usize = 1024;
/// 变更账上限：MprotectRec 约 32B × 512 = 16KB 堆账。
pub const MAX_MPROTECTS: usize = 512;
/// 豁免账上限：ExemptRec 约 56B × 64 ≈ 3.5KB 堆账（豁免本就应稀有）。
pub const MAX_EXEMPTS: usize = 64;
/// 事件账上限：EventRec 约 32B × 1024 = 32KB 堆账，越限走丢包计数。
pub const MAX_EVENTS: usize = 1024;
/// 单次访问探针流上限（防 O(n) 走查失控，超出拒绝整批）。
pub const MAX_ACCESS_PROBES: usize = 4096;

/// W^X 红线全扫描案数 = 8 类 × 2 违例形状（W+X、R+W+X）。
pub const WX_SWEEP_CASES: u32 = 16;

// ═══════════════════════════════════════════════════════════════════════════
// 诊断码段（独占 0x1E50~0x1E5F；0x1E50 为 OK 码，0x1E51~0x1E5F 为错误码）
// ═══════════════════════════════════════════════════════════════════════════

/// 事件级 OK 码：正常留痕（非错误）。
pub const E_OK: u16 = SEG_BASE; // 0x1E50

/// UNX-F0306 诊断码闭集（15 个错误变体恰好填满 0x1E51~0x1E5F）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WxMatrixError {
    /// 请求位为空或含 RWX 闭集（bit0..2）之外的位。
    IllegalPermBits, // 0x1E51
    /// W^X 红线：可写+可执行同时置位（可写可执行），一律拒收。
    WxViolation,     // 0x1E52
    /// 请求权限是矩阵行超集（请求位超出该类型冻结行）。
    PermSuperset,    // 0x1E53
    /// 区域类型索引越界（from_index 收到 ≥8 的值）。
    UnknownRegionType, // 0x1E54
    /// 区形状非法：零长/未 4K 对齐/基址+长度回绕/与在册区重叠。
    RegionShape,     // 0x1E55
    /// 区域/地址不在册：查无该 region_id，或访问地址不落入任何在册区与豁免别名。
    RegionUnknown,   // 0x1E56
    /// 状态机误用：封板后仍登记区/登记豁免（裁决与变更路径不受封板影响）。
    BadState,        // 0x1E57
    /// 账面超限：区/裁决/变更/豁免账越过冻结上限，请分批或调高上限常量。
    LedgerFull,      // 0x1E58
    /// 豁免账满：在册豁免条目已达 MAX_EXEMPTS，请先撤销再登记。
    ExemptFull,      // 0x1E59
    /// 豁免条目不存在（撤销/查询的 exempt_id 查无）。
    ExemptUnknown,   // 0x1E5A
    /// 豁免形状非法：别名未 4K 对齐/两别名同址/别名落在在册区内/
    /// 物理页已有在态豁免/未登记别名被放行。
    ExemptIllegal,   // 0x1E5B
    /// 豁免状态非法：对已撤销条目重复撤销。
    ExemptState,     // 0x1E5C
    /// 升权拒收：mprotect 新权限超出旧权限（新增任何位），降权自由升权拒。
    UpgradeDenied,   // 0x1E5D
    /// 双跑确定性对拍漂移：同脚本两跑指纹不同。
    DeterminismDrift, // 0x1E5E
    /// 内部算术溢出（checked 链断裂，属防御性兜底）。
    InternalOverflow, // 0x1E5F
}

impl WxMatrixError {
    /// 诊断码（段内唯一，0x1E51~0x1E5F；const 以便编译期闭集断言复用）。
    pub const fn code(&self) -> u16 {
        match self {
            WxMatrixError::IllegalPermBits => SEG_BASE + 0x01,
            WxMatrixError::WxViolation => SEG_BASE + 0x02,
            WxMatrixError::PermSuperset => SEG_BASE + 0x03,
            WxMatrixError::UnknownRegionType => SEG_BASE + 0x04,
            WxMatrixError::RegionShape => SEG_BASE + 0x05,
            WxMatrixError::RegionUnknown => SEG_BASE + 0x06,
            WxMatrixError::BadState => SEG_BASE + 0x07,
            WxMatrixError::LedgerFull => SEG_BASE + 0x08,
            WxMatrixError::ExemptFull => SEG_BASE + 0x09,
            WxMatrixError::ExemptUnknown => SEG_BASE + 0x0A,
            WxMatrixError::ExemptIllegal => SEG_BASE + 0x0B,
            WxMatrixError::ExemptState => SEG_BASE + 0x0C,
            WxMatrixError::UpgradeDenied => SEG_BASE + 0x0D,
            WxMatrixError::DeterminismDrift => SEG_BASE + 0x0E,
            WxMatrixError::InternalOverflow => SEG_BASE + 0x0F,
        }
    }

    /// 中文可操作诊断（每变体固定一句，指明"哪里错了+怎么修"）。
    pub fn detail(&self) -> &'static str {
        match self {
            WxMatrixError::IllegalPermBits =>
                "权限位非法：请求为空或含 R/W/X（bit0..2）之外的位，请核对请求位的非法置位",
            WxMatrixError::WxViolation =>
                "W^X 红线拒收：可写+可执行同时置位（可写可执行）一律红案；如确需双面访问请走显式双映射豁免通道登记",
            WxMatrixError::PermSuperset =>
                "矩阵超集拒收：请求权限超出该区域类型的冻结 RWX 矩阵行，请比对 8×3 矩阵表该行后收敛请求",
            WxMatrixError::UnknownRegionType =>
                "区域类型未知：类型索引越界（闭集共 8 类），请核对 from_index 入参",
            WxMatrixError::RegionShape =>
                "区形状非法：区长为零/未 4K 对齐/基址+长度回绕/与在册区重叠，请核对区表的基址、长度与重叠关系",
            WxMatrixError::RegionUnknown =>
                "目标不在册：查无该 region_id，或访问地址不落入任何在册区域与豁免别名——未登记访问一律拒收",
            WxMatrixError::BadState =>
                "状态机误用：账已封板，登记区/登记豁免被拒；裁决与 mprotect 变更路径不受封板影响",
            WxMatrixError::LedgerFull =>
                "账面超限：区/裁决/变更账越过冻结上限，请分批提交或调高上限常量",
            WxMatrixError::ExemptFull =>
                "豁免账满：在册豁免条目已达上限，请先撤销不再使用的双映射豁免再登记新条目",
            WxMatrixError::ExemptUnknown =>
                "豁免条目不存在：exempt_id 查无，请核对豁免账在册清单",
            WxMatrixError::ExemptIllegal =>
                "豁免形状非法：别名未 4K 对齐/两别名同址/别名落在在册区内/物理页已有在态豁免/未登记别名被放行，请核对双映射的物理页与两个别名地址",
            WxMatrixError::ExemptState =>
                "豁免状态非法：该条目已撤销，不可重复撤销；如需再豁免请重新登记",
            WxMatrixError::UpgradeDenied =>
                "升权拒收：mprotect 新权限含旧权限没有的位；降权自由、升权一律拒，请重新登记区域或走豁免通道",
            WxMatrixError::DeterminismDrift =>
                "双跑漂移：同脚本两跑指纹不同，请检查是否有未冻结的迭代序或随机源",
            WxMatrixError::InternalOverflow =>
                "内部算术溢出：checked 链断裂属防御性兜底，请携带事件账上报",
        }
    }
}

impl core::fmt::Display for WxMatrixError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "UNX-F0306 0x{:04X}: {}", self.code(), self.detail())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PermBits：RWX 权限位闭集（newtype u8，bit0=R / bit1=W / bit2=X）
// ═══════════════════════════════════════════════════════════════════════════

/// 权限位掩码闭集（R=bit0，W=bit1，X=bit2；任务文"W+A"之 A 即执行位 X）。
pub const PERM_MASK: u8 = 0b111;
/// R 位（读）。
pub const PERM_R: u8 = 1 << 0;
/// W 位（写）。
pub const PERM_W: u8 = 1 << 1;
/// X 位（执行/取指）。
pub const PERM_X: u8 = 1 << 2;

/// RWX 权限位集（u8 newtype；const fn 一律走位运算，不用 derive 的 ==）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermBits(pub u8);

impl PermBits {
    /// 空权限（无任何位）。
    pub const NONE: PermBits = PermBits(0);
    /// 只读（R）。
    pub const RO: PermBits = PermBits(PERM_R);
    /// 读写（R+W），数据面冻结形状。
    pub const RW: PermBits = PermBits(PERM_R | PERM_W);
    /// 读执行（R+X），代码面冻结形状。
    pub const RX: PermBits = PermBits(PERM_R | PERM_X);

    /// 构造：bits 必须非零且 ⊆ PERM_MASK，否则 None。
    pub const fn new(bits: u8) -> Option<PermBits> {
        if bits & !PERM_MASK != 0 {
            None
        } else if bits == 0 {
            None
        } else {
            Some(PermBits(bits))
        }
    }

    /// 原始位。
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// 含 R 位。
    pub const fn has_r(self) -> bool {
        self.0 & PERM_R != 0
    }

    /// 含 W 位。
    pub const fn has_w(self) -> bool {
        self.0 & PERM_W != 0
    }

    /// 含 X 位。
    pub const fn has_x(self) -> bool {
        self.0 & PERM_X != 0
    }

    /// 是否为空权限。
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// W^X 违例判定：W 与 X 同时置位（可写可执行）。
    pub const fn is_wx_violation(self) -> bool {
        self.0 & (PERM_W | PERM_X) == (PERM_W | PERM_X)
    }

    /// 子集判定：self ⊆ other（self 的每一位 other 都有）。
    pub const fn is_subset_of(self, other: PermBits) -> bool {
        self.0 & other.0 == self.0
    }

    /// 并集。
    pub const fn union(self, other: PermBits) -> PermBits {
        PermBits(self.0 | other.0)
    }

    /// 差集（从 self 去掉 other 有的位）。
    pub const fn minus(self, other: PermBits) -> PermBits {
        PermBits(self.0 & !other.0)
    }

    /// 剥离执行位（W 别名派生用：任何形状去掉 X 后必满足 W^X）。
    pub const fn strip_exec(self) -> PermBits {
        PermBits(self.0 & !PERM_X)
    }

    /// 剥离写位（X 别名派生用）。
    pub const fn strip_write(self) -> PermBits {
        PermBits(self.0 & !PERM_W)
    }

    /// 三字母助记串（"rwx"/"r-x"/…），8 种形状闭集 match。
    pub const fn rwx_str(self) -> &'static str {
        match self.0 & PERM_MASK {
            0b000 => "---",
            0b001 => "r--",
            0b010 => "-w-",
            0b011 => "rw-",
            0b100 => "--x",
            0b101 => "r-x",
            0b110 => "rw x!", // 非法形状（W+X），仅诊断展示用
            _ => "rwx!",      // 非法形状（R+W+X），仅诊断展示用
        }
    }

    /// 折算 x86-64 PTE 权限位：P 恒置；W 位→PTE_RW；X 位缺→PTE_NX；
    /// user 置位→PTE_US（内核类型清 0）。永不产出 W+X 编码（入参为违例
    /// 形状时 NX 与 RW 同置属硬件不可达态，调用方须先过 W^X 红线）。
    pub const fn to_pte_bits(self, user: bool) -> u64 {
        let mut p = PTE_P;
        if self.0 & PERM_W != 0 {
            p |= PTE_RW;
        }
        if self.0 & PERM_X == 0 {
            p |= PTE_NX;
        }
        if user {
            p |= PTE_US;
        }
        p
    }
}

impl core::fmt::Display for PermBits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.rwx_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// RegionType：八类区域闭集 + 8×3 RWX 冻结矩阵
// ═══════════════════════════════════════════════════════════════════════════

/// 区域类型闭集（8 类；矩阵行来源逐行注释于 RWX_MATRIX 处）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionType {
    /// 内核代码段（.text）。
    KernelCode,
    /// 内核只读数据（.rodata）。
    KernelRoData,
    /// 内核可写数据（.data/.bss）。
    KernelData,
    /// 任务栈（用户态）。
    TaskStack,
    /// 任务堆（用户态）。
    TaskHeap,
    /// 设备 MMIO 寄存器窗口。
    DeviceMmio,
    /// 页表自身（经 F0308 自映射窗口访问）。
    PageTableSelf,
    /// 动态代码终态（模块/trampoline 落定后；写入期走独立 W 别名+豁免）。
    DynCode,
}

impl RegionType {
    /// 闭集基数。
    pub const COUNT: usize = 8;

    /// 类型索引（0..7；const fn 用 match，不用数组索引）。
    pub const fn index(self) -> usize {
        match self {
            RegionType::KernelCode => 0,
            RegionType::KernelRoData => 1,
            RegionType::KernelData => 2,
            RegionType::TaskStack => 3,
            RegionType::TaskHeap => 4,
            RegionType::DeviceMmio => 5,
            RegionType::PageTableSelf => 6,
            RegionType::DynCode => 7,
        }
    }

    /// 由索引构造（越界 None）。
    pub const fn from_index(i: u8) -> Option<RegionType> {
        match i {
            0 => Some(RegionType::KernelCode),
            1 => Some(RegionType::KernelRoData),
            2 => Some(RegionType::KernelData),
            3 => Some(RegionType::TaskStack),
            4 => Some(RegionType::TaskHeap),
            5 => Some(RegionType::DeviceMmio),
            6 => Some(RegionType::PageTableSelf),
            7 => Some(RegionType::DynCode),
            _ => None,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            RegionType::KernelCode => "内核代码段",
            RegionType::KernelRoData => "内核只读数据",
            RegionType::KernelData => "内核可写数据",
            RegionType::TaskStack => "任务栈",
            RegionType::TaskHeap => "任务堆",
            RegionType::DeviceMmio => "设备MMIO",
            RegionType::PageTableSelf => "页表自身",
            RegionType::DynCode => "动态代码终态",
        }
    }

    /// 该类型的冻结矩阵行（match 返回 PermBits 常量，与 RWX_MATRIX 表
    /// 双源互证：编译期各自断言，运行期 audit_matrix 对拍）。
    pub const fn matrix_row(self) -> PermBits {
        match self {
            RegionType::KernelCode => PB_KERNEL_CODE,
            RegionType::KernelRoData => PB_KERNEL_RODATA,
            RegionType::KernelData => PB_KERNEL_DATA,
            RegionType::TaskStack => PB_TASK_STACK,
            RegionType::TaskHeap => PB_TASK_HEAP,
            RegionType::DeviceMmio => PB_DEVICE_MMIO,
            RegionType::PageTableSelf => PB_PAGE_TABLE_SELF,
            RegionType::DynCode => PB_DYN_CODE,
        }
    }

    /// 用户态可达（US 位）：仅任务栈/任务堆/动态代码属用户面
    /// （来源：F0301 四级流水用户区划分 + F0303 高半区内核区划分）。
    pub const fn is_user(self) -> bool {
        match self {
            RegionType::TaskStack | RegionType::TaskHeap | RegionType::DynCode => true,
            _ => false,
        }
    }

    /// NX 硬性要求（矩阵行无 X 位即数据面，F0307 EFER.NXE 生效后必置 NX）。
    pub const fn nx_required(self) -> bool {
        !self.matrix_row().has_x()
    }

    /// 是否豁免通道常客（动态代码写入期典型走双映射豁免）。
    pub const fn exempt_friendly(self) -> bool {
        match self {
            RegionType::DynCode => true,
            _ => false,
        }
    }
}

// ── 8×3 RWX 冻结矩阵（行=类型，列=[R,W,X]，1=允许）─────────────────────────
// 逐行来源（判据 J1(a) 的"每行来源注释"）：
//  [0] 内核代码段   [1,0,1] ELF PT_LOAD .text 段标志 PF_R|PF_X（ELF gABI
//                    §程序头 p_flags），F0096 段加载落位，KASLR（F0098）
//                    只移基址不改权限；
//  [1] 内核只读数据 [1,0,0] .rodata 段 PF_R；F0085 BSS 清零/F0084 重定位
//                    修完后应写保护，恒只读；
//  [2] 内核可写数据 [1,1,0] .data/.bss 段 PF_R|PF_W；数据面 NX（F0307）；
//  [3] 任务栈       [1,1,0] 栈段 PF_R|PF_W + NX 硬性（栈不可执行是
//                    W^X 政策的组成部分，x86-64 栈无 PF_X）；
//  [4] 任务堆       [1,1,0] 堆 PF_R|PF_W + NX（防堆喷跳转执行）；
//  [5] 设备 MMIO    [1,1,0] MMIO 寄存器窗口 R+W（UC 缓存属性走 F0309
//                    PAT，与执行权无关），永不可执行；
//  [6] 页表自身     [1,1,0] F0308 自映射窗口读写页表项，页表数据永不
//                    可执行（自指执行是第 3 类 W^X 攻击面）；
//  [7] 动态代码终态 [1,0,1] 模块/trampoline 落定后与 .text 同权（R+X）；
//                    写入期权限经独立 W 别名（KernelData 面）+ 显式
//                    双映射豁免通道，终态行本身不含 W。
/// 8×3 RWX 矩阵（审计数据源之一；与 RegionType::matrix_row 双源互证）。
pub const RWX_MATRIX: [[u8; 3]; RegionType::COUNT] = [
    [1, 0, 1], // [0] KernelCode   r-x（来源见上）
    [1, 0, 0], // [1] KernelRoData r--
    [1, 1, 0], // [2] KernelData   rw-
    [1, 1, 0], // [3] TaskStack    rw-
    [1, 1, 0], // [4] TaskHeap     rw-
    [1, 1, 0], // [5] DeviceMmio   rw-
    [1, 1, 0], // [6] PageTableSelf rw-
    [1, 0, 1], // [7] DynCode      r-x
];

/// 各类型矩阵行的 PermBits 常量（由 RWX_MATRIX 行折算；const 初始化中
/// 的数组字面量索引属编译期求值，不受 const fn 禁索引纪律约束）。
pub const PB_KERNEL_CODE: PermBits =
    PermBits(RWX_MATRIX[0][0] | (RWX_MATRIX[0][1] << 1) | (RWX_MATRIX[0][2] << 2));
/// KernelRoData 行常量。
pub const PB_KERNEL_RODATA: PermBits =
    PermBits(RWX_MATRIX[1][0] | (RWX_MATRIX[1][1] << 1) | (RWX_MATRIX[1][2] << 2));
/// KernelData 行常量。
pub const PB_KERNEL_DATA: PermBits =
    PermBits(RWX_MATRIX[2][0] | (RWX_MATRIX[2][1] << 1) | (RWX_MATRIX[2][2] << 2));
/// TaskStack 行常量。
pub const PB_TASK_STACK: PermBits =
    PermBits(RWX_MATRIX[3][0] | (RWX_MATRIX[3][1] << 1) | (RWX_MATRIX[3][2] << 2));
/// TaskHeap 行常量。
pub const PB_TASK_HEAP: PermBits =
    PermBits(RWX_MATRIX[4][0] | (RWX_MATRIX[4][1] << 1) | (RWX_MATRIX[4][2] << 2));
/// DeviceMmio 行常量。
pub const PB_DEVICE_MMIO: PermBits =
    PermBits(RWX_MATRIX[5][0] | (RWX_MATRIX[5][1] << 1) | (RWX_MATRIX[5][2] << 2));
/// PageTableSelf 行常量。
pub const PB_PAGE_TABLE_SELF: PermBits =
    PermBits(RWX_MATRIX[6][0] | (RWX_MATRIX[6][1] << 1) | (RWX_MATRIX[6][2] << 2));
/// DynCode 行常量。
pub const PB_DYN_CODE: PermBits =
    PermBits(RWX_MATRIX[7][0] | (RWX_MATRIX[7][1] << 1) | (RWX_MATRIX[7][2] << 2));

// ── 编译期逐行断言（判据 J1(b)"任何类型矩阵行本身不含 W+A"红线）──────────
// 注意：PermBits derive 了 PartialEq，其 == 非 const，故此处一律比较 .0
// 原始 u8 字段（u8 的 == 是内建运算，const 可用）。
const _: () = assert!(
    !(RWX_MATRIX[0][1] == 1 && RWX_MATRIX[0][2] == 1),
    "UNX-F0306: KernelCode 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[1][1] == 1 && RWX_MATRIX[1][2] == 1),
    "UNX-F0306: KernelRoData 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[2][1] == 1 && RWX_MATRIX[2][2] == 1),
    "UNX-F0306: KernelData 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[3][1] == 1 && RWX_MATRIX[3][2] == 1),
    "UNX-F0306: TaskStack 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[4][1] == 1 && RWX_MATRIX[4][2] == 1),
    "UNX-F0306: TaskHeap 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[5][1] == 1 && RWX_MATRIX[5][2] == 1),
    "UNX-F0306: DeviceMmio 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[6][1] == 1 && RWX_MATRIX[6][2] == 1),
    "UNX-F0306: PageTableSelf 行含 W+X，W^X 红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[7][1] == 1 && RWX_MATRIX[7][2] == 1),
    "UNX-F0306: DynCode 行含 W+X，W^X 红线破裂"
);
// ── 编译期逐行断言：行必含 R（无可读性才有意义的最小行）───────────────────
const _: () = assert!(RWX_MATRIX[0][0] == 1, "UNX-F0306: KernelCode 行缺 R");
const _: () = assert!(RWX_MATRIX[1][0] == 1, "UNX-F0306: KernelRoData 行缺 R");
const _: () = assert!(RWX_MATRIX[2][0] == 1, "UNX-F0306: KernelData 行缺 R");
const _: () = assert!(RWX_MATRIX[3][0] == 1, "UNX-F0306: TaskStack 行缺 R");
const _: () = assert!(RWX_MATRIX[4][0] == 1, "UNX-F0306: TaskHeap 行缺 R");
const _: () = assert!(RWX_MATRIX[5][0] == 1, "UNX-F0306: DeviceMmio 行缺 R");
const _: () = assert!(RWX_MATRIX[6][0] == 1, "UNX-F0306: PageTableSelf 行缺 R");
const _: () = assert!(RWX_MATRIX[7][0] == 1, "UNX-F0306: DynCode 行缺 R");
// ── 编译期断言：行形状恰为冻结值（防表被手改漂移）─────────────────────────
const _: () = assert!(PB_KERNEL_CODE.0 == 0b101, "UNX-F0306: KernelCode 行形状漂移");
const _: () = assert!(PB_KERNEL_RODATA.0 == 0b001, "UNX-F0306: KernelRoData 行形状漂移");
const _: () = assert!(PB_KERNEL_DATA.0 == 0b011, "UNX-F0306: KernelData 行形状漂移");
const _: () = assert!(PB_TASK_STACK.0 == 0b011, "UNX-F0306: TaskStack 行形状漂移");
const _: () = assert!(PB_TASK_HEAP.0 == 0b011, "UNX-F0306: TaskHeap 行形状漂移");
const _: () = assert!(PB_DEVICE_MMIO.0 == 0b011, "UNX-F0306: DeviceMmio 行形状漂移");
const _: () = assert!(PB_PAGE_TABLE_SELF.0 == 0b011, "UNX-F0306: PageTableSelf 行形状漂移");
const _: () = assert!(PB_DYN_CODE.0 == 0b101, "UNX-F0306: DynCode 行形状漂移");

/// 双映射豁免的 W 别名冻结形状（数据面：R+W，永不含 X）。
pub const EXEMPT_W_ALIAS: PermBits = PermBits::RW;
/// 双映射豁免的 X 别名冻结形状（取指面：R+X，永不含 W）。
pub const EXEMPT_X_ALIAS: PermBits = PermBits::RX;

// ── 编译期断言：豁免别名形状各自满足 W^X 且两别名互补不重叠────────────────
const _: () = assert!(
    !EXEMPT_W_ALIAS.is_wx_violation(),
    "UNX-F0306: 豁免 W 别名形状违例"
);
const _: () = assert!(
    !EXEMPT_X_ALIAS.is_wx_violation(),
    "UNX-F0306: 豁免 X 别名形状违例"
);

// ═══════════════════════════════════════════════════════════════════════════
// FNV-1a 64 指纹（标准参数，来源：FNV-1a 64 规范；与 F0305 同源）
// ═══════════════════════════════════════════════════════════════════════════

/// FNV-1a 64 偏移基（规范冻结值）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（规范冻结值）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 单字节折叠。
pub const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// u64 折叠（小端逐字节，while+索引实现确定性）。
pub fn fnv_u64(h: u64, v: u64) -> u64 {
    let bytes = v.to_le_bytes();
    let mut out = h;
    let mut i = 0usize;
    while i < 8 {
        out = fnv_byte(out, bytes[i]);
        i += 1;
    }
    out
}

/// u32 折叠（低 4 字节小端）。
pub fn fnv_u32(h: u64, v: u32) -> u64 {
    fnv_u64(h, v as u64 & 0xFFFF_FFFF)
}

/// 字节串折叠（while+索引，不用迭代器以保迭代序显式冻结）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h = fnv_byte(h, data[i]);
        i += 1;
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════════
// 记录体：裁决账 / 变更账 / 豁免账 / 事件账 / 区登记
// ═══════════════════════════════════════════════════════════════════════════

/// 裁决结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 放行。
    Allow,
    /// 拒收。
    Deny,
}

impl Verdict {
    /// 折算指纹用位。
    pub const fn bit(self) -> u8 {
        match self {
            Verdict::Allow => 1,
            Verdict::Deny => 0,
        }
    }
}

/// 无挂靠区的裁决 region_id 哨兵（纯裁决不落任何区）。
pub const NO_REGION: u32 = u32::MAX;

/// 裁决账记录（判据 J1(c)：类型/请求位/结论/码/W^X 红线位）。
#[derive(Clone, Copy, Debug)]
pub struct DecisionRec {
    /// 全局序号。
    pub seq: u64,
    /// 挂靠区（NO_REGION=纯裁决）。
    pub region_id: u32,
    /// 区域类型。
    pub rtype: RegionType,
    /// 请求权限位。
    pub requested: PermBits,
    /// 实授权限（拒收时为 NONE）。
    pub granted: PermBits,
    /// 结论。
    pub verdict: Verdict,
    /// 诊断码（放行 E_OK / 拒收错误码）。
    pub code: u16,
    /// 是否 W^X 红线案。
    pub wx_redline: bool,
}

/// 变更账记录（mprotect 语义，判据 J1(d)）。
#[derive(Clone, Copy, Debug)]
pub struct MprotectRec {
    /// 全局序号。
    pub seq: u64,
    /// 目标区。
    pub region_id: u32,
    /// 区类型（不变性由引擎保证）。
    pub rtype: RegionType,
    /// 变更前权限。
    pub old_perms: PermBits,
    /// 请求新权限。
    pub new_perms: PermBits,
    /// 结论。
    pub verdict: Verdict,
    /// 诊断码。
    pub code: u16,
}

/// 豁免条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExemptState {
    /// 在态（豁免生效中）。
    Active,
    /// 已撤销。
    Revoked,
}

impl ExemptState {
    /// 折算指纹用位。
    pub const fn bit(self) -> u8 {
        match self {
            ExemptState::Active => 1,
            ExemptState::Revoked => 0,
        }
    }
}

/// 双映射豁免账记录（判据 J1(e)）。
#[derive(Clone, Copy, Debug)]
pub struct ExemptRec {
    /// 豁免条目号。
    pub exempt_id: u32,
    /// 被双映射的物理页基址（4K 对齐）。
    pub phys: u64,
    /// W 别名虚地址（数据面 R+W）。
    pub va_w: u64,
    /// X 别名虚地址（取指面 R+X）。
    pub va_x: u64,
    /// W 别名冻结形状。
    pub w_alias: PermBits,
    /// X 别名冻结形状。
    pub x_alias: PermBits,
    /// 在态/已撤销。
    pub state: ExemptState,
    /// 登记序号。
    pub grant_seq: u64,
    /// 撤销序号（未撤销为 0）。
    pub revoke_seq: u64,
}

/// 事件种类（事件账用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 引擎出厂（含 boot seed）。
    Boot,
    /// 区登记成功。
    RegionAdd,
    /// 一次裁决。
    Decision,
    /// 一次权限变更。
    Mprotect,
    /// 豁免登记。
    ExemptGrant,
    /// 豁免撤销。
    ExemptRevoke,
    /// 访问探针拒收（放行不留事件省账）。
    AccessDeny,
    /// 注入面动作。
    Injection,
    /// 封板。
    Seal,
    /// 账面丢包（越限计数）。
    Overflow,
}

impl EventKind {
    /// 折算指纹用索引。
    pub const fn idx(self) -> u8 {
        match self {
            EventKind::Boot => 0,
            EventKind::RegionAdd => 1,
            EventKind::Decision => 2,
            EventKind::Mprotect => 3,
            EventKind::ExemptGrant => 4,
            EventKind::ExemptRevoke => 5,
            EventKind::AccessDeny => 6,
            EventKind::Injection => 7,
            EventKind::Seal => 8,
            EventKind::Overflow => 9,
        }
    }
}

/// 事件账记录。
#[derive(Clone, Copy, Debug)]
pub struct EventRec {
    /// 全局序号。
    pub seq: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 关联诊断码。
    pub code: u16,
    /// 载荷 a（语境随 kind 而异）。
    pub a: u64,
    /// 载荷 b。
    pub b: u64,
}

/// 在册区记录。
#[derive(Clone, Copy, Debug)]
pub struct RegionRec {
    /// 区号。
    pub region_id: u32,
    /// 区类型（登记后冻结）。
    pub rtype: RegionType,
    /// 基址（4K 对齐）。
    pub base: u64,
    /// 长度（4K 整数倍）。
    pub len: u64,
    /// 当前实授权限（登记实授=请求；mprotect 只降不升）。
    pub perms: PermBits,
    /// mprotect 世代计数（每次成功降权 +1）。
    pub gen: u32,
    /// 登记序号。
    pub grant_seq: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// 统计账（拒收率千分比 + 分类型直方图）
// ═══════════════════════════════════════════════════════════════════════════

/// 引擎统计快照（全 u64 计数器 + [u64;8] 分类型直方图，Copy）。
#[derive(Clone, Copy, Debug)]
pub struct WxStats {
    /// 裁决总数。
    pub decisions_total: u64,
    /// 放行数。
    pub allows: u64,
    /// 拒收数。
    pub denies: u64,
    /// W^X 红线案数（denies 子集）。
    pub wx_redlines: u64,
    /// 裁决账丢包数（账满后裁决仍出结论，仅不留痕）。
    pub decisions_dropped: u64,
    /// 变更尝试总数。
    pub mprotect_total: u64,
    /// 成功降权数。
    pub mprotect_downgrades: u64,
    /// 变更拒收数（升权/矩阵拒/W^X 拒）。
    pub mprotect_denied: u64,
    /// 成功登记区数。
    pub regions_registered: u64,
    /// 豁免登记成功数。
    pub exempt_grants: u64,
    /// 豁免撤销成功数。
    pub exempt_revokes: u64,
    /// 豁免形状/状态拒收数。
    pub exempt_rejected: u64,
    /// 出厂豁免基线为零（红线：豁免默认 0）。
    pub exempt_boot_zero: bool,
    /// 访问探针总数。
    pub access_probes: u64,
    /// 访问拒收数。
    pub access_denies: u64,
    /// 其中未落在册面（unmapped）数。
    pub access_unmapped: u64,
    /// 事件账丢包数。
    pub events_dropped: u64,
    /// 分类型裁决直方图（下标=RegionType::index）。
    pub per_type_decisions: [u64; 8],
    /// 分类型拒收直方图。
    pub per_type_denies: [u64; 8],
}

impl WxStats {
    /// 全零快照（boot_zero 由引擎出厂时置真）。
    pub const fn zeroed() -> WxStats {
        WxStats {
            decisions_total: 0,
            allows: 0,
            denies: 0,
            wx_redlines: 0,
            decisions_dropped: 0,
            mprotect_total: 0,
            mprotect_downgrades: 0,
            mprotect_denied: 0,
            regions_registered: 0,
            exempt_grants: 0,
            exempt_revokes: 0,
            exempt_rejected: 0,
            exempt_boot_zero: false,
            access_probes: 0,
            access_denies: 0,
            access_unmapped: 0,
            events_dropped: 0,
            per_type_decisions: [0; 8],
            per_type_denies: [0; 8],
        }
    }

    /// 拒收率（千分比；denies/total，saturating 乘法防溢出，total=0 时 0）。
    pub const fn reject_rate_per_mille(&self) -> u64 {
        if self.decisions_total == 0 {
            0
        } else {
            self.denies.saturating_mul(1000) / self.decisions_total
        }
    }

    /// 豁免在态估计值（grants - revokes，saturating）。
    pub const fn exempt_active_estimate(&self) -> u64 {
        self.exempt_grants.saturating_sub(self.exempt_revokes)
    }

    /// 放行率（千分比；1000 - 拒收率，显式重算而非减法漂移）。
    pub const fn allow_rate_per_mille(&self) -> u64 {
        if self.decisions_total == 0 {
            0
        } else {
            self.allows.saturating_mul(1000) / self.decisions_total
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 矩阵审计（判据 J1(a)：区域类型→RWX 矩阵全过）
// ═══════════════════════════════════════════════════════════════════════════

/// 单行审计结论。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRowAudit {
    /// 区域类型。
    pub rtype: RegionType,
    /// 表源行（RWX_MATRIX 原始三元组）。
    pub rwx: [u8; 3],
    /// const 源行（matrix_row 折算）。
    pub row: PermBits,
    /// 行内无 W+X。
    pub no_wx: bool,
    /// 行内含 R。
    pub has_r: bool,
    /// 表源与 const 源一致（双源对拍）。
    pub matches_const: bool,
    /// 行本行全过。
    pub row_ok: bool,
    /// 该行折算 PTE 编码（user 位随类型；防 W+X 编码出现的纵深检查输入）。
    pub pte: u64,
}

/// 全矩阵审计结论。
#[derive(Clone, Copy, Debug)]
pub struct MatrixAudit {
    /// 8 行逐行结论。
    pub rows: [MatrixRowAudit; 8],
    /// 全过位。
    pub all_ok: bool,
    /// 矩阵指纹（FNV over RWX_MATRIX 字节，跨实例恒定）。
    pub fingerprint: u64,
}

/// 矩阵审计（免费函数：表源 RWX_MATRIX ↔ const 源 matrix_row 双源对拍 +
/// 逐行 W^X 红线 + 逐行含 R + PTE 编码；while+索引遍历）。
pub fn audit_matrix() -> MatrixAudit {
    let mut rows: [MatrixRowAudit; 8] = [
        row_audit_placeholder(0);
        8
    ];
    let mut all_ok = true;
    let mut i = 0usize;
    while i < 8 {
        // i < 8 且 from_index 闭集覆盖 0..=7，此处 rt 必为 Some；
        // 仍走 match 防御（None 时该行记失败）。
        let rt = match RegionType::from_index(i as u8) {
            Some(t) => t,
            None => {
                all_ok = false;
                i += 1;
                continue;
            }
        };
        let rwx = RWX_MATRIX[i];
        let row = rt.matrix_row();
        let table_bits = PermBits(rwx[0] | (rwx[1] << 1) | (rwx[2] << 2));
        // 双源对拍：比较原始 u8（derive == 在运行期可用，但保惯用位比较）。
        let matches_const = table_bits.0 == row.0;
        let no_wx = !(row.has_w() && row.has_x());
        let has_r = row.has_r();
        let row_ok = matches_const && no_wx && has_r;
        if !row_ok {
            all_ok = false;
        }
        rows[i] = MatrixRowAudit {
            rtype: rt,
            rwx,
            row,
            no_wx,
            has_r,
            matches_const,
            row_ok,
            pte: row.to_pte_bits(rt.is_user()),
        };
        i += 1;
    }
    // 矩阵指纹：24 字节逐字节折叠（迭代序冻结）。
    let mut fp = FNV_OFFSET;
    let mut r = 0usize;
    while r < 8 {
        let mut c = 0usize;
        while c < 3 {
            fp = fnv_byte(fp, RWX_MATRIX[r][c]);
            c += 1;
        }
        r += 1;
    }
    MatrixAudit {
        rows,
        all_ok,
        fingerprint: fp,
    }
}

/// 占位行（audit_matrix 内部初始化用；type 取 KernelCode 占位，逐行覆写）。
fn row_audit_placeholder(_i: usize) -> MatrixRowAudit {
    MatrixRowAudit {
        rtype: RegionType::KernelCode,
        rwx: [0, 0, 0],
        row: PermBits::NONE,
        no_wx: false,
        has_r: false,
        matches_const: false,
        row_ok: false,
        pte: 0,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 纯裁决核（const fn：编译期即可裁决；判据 J1(b) 红线的编译期证明）
// ═══════════════════════════════════════════════════════════════════════════

/// 纯裁决核（无账面副作用；const fn 可用于编译期断言）：
/// 1) 权限位闭集校验（非零且 ⊆ PERM_MASK）→ IllegalPermBits；
/// 2) W^X 红线（W+X 同时）→ WxViolation（优先级最高，先于矩阵判定）；
/// 3) 矩阵子集判定（请求 ⊆ 冻结行）→ 超集即 PermSuperset；
/// 4) 放行，实授=请求（不补齐到整行）。
pub const fn adjudicate_const(rtype: RegionType, req: PermBits) -> Result<PermBits, WxMatrixError> {
    if req.0 & !PERM_MASK != 0 || req.0 == 0 {
        return Err(WxMatrixError::IllegalPermBits);
    }
    if req.0 & (PERM_W | PERM_X) == (PERM_W | PERM_X) {
        return Err(WxMatrixError::WxViolation);
    }
    let row = rtype.matrix_row();
    if req.0 & row.0 != req.0 {
        return Err(WxMatrixError::PermSuperset);
    }
    Ok(req)
}

// 编译期红线证明（判据 J1(b) 的编译期侧）：八类 × 违例形状全拒。
const CT_WX_KCODE: Result<PermBits, WxMatrixError> =
    adjudicate_const(RegionType::KernelCode, PermBits(0b110));
const _: () = assert!(matches!(CT_WX_KCODE, Err(WxMatrixError::WxViolation)));
const CT_WX_STACK: Result<PermBits, WxMatrixError> =
    adjudicate_const(RegionType::TaskStack, PermBits(0b111));
const _: () = assert!(matches!(CT_WX_STACK, Err(WxMatrixError::WxViolation)));
const CT_OK_CODE: Result<PermBits, WxMatrixError> =
    adjudicate_const(RegionType::KernelCode, PermBits(0b101));
const _: () = assert!(matches!(CT_OK_CODE, Ok(_)));
const CT_SUPERSET_RO: Result<PermBits, WxMatrixError> =
    adjudicate_const(RegionType::KernelRoData, PermBits(0b011));
const _: () = assert!(matches!(CT_SUPERSET_RO, Err(WxMatrixError::PermSuperset)));

// ═══════════════════════════════════════════════════════════════════════════
// 访问裁决（访问探针：区域/豁免别名/未映射三源归一）
// ═══════════════════════════════════════════════════════════════════════════

/// 访问来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessSource {
    /// 命中在册区（载荷=region_id）。
    Region(u32),
    /// 命中豁免 W 别名（载荷=exempt_id）。
    ExemptW(u32),
    /// 命中豁免 X 别名（载荷=exempt_id）。
    ExemptX(u32),
    /// 未落在任何在册面（红线：未登记访问一律拒）。
    Unmapped,
}

/// 单次访问裁决结论。
#[derive(Clone, Copy, Debug)]
pub struct AccessVerdict {
    /// 探针虚地址。
    pub va: u64,
    /// 需求权限位。
    pub need: PermBits,
    /// 命中面实授权限（未映射为 NONE）。
    pub effective: PermBits,
    /// 放行位。
    pub allowed: bool,
    /// 诊断码（放行 E_OK）。
    pub code: u16,
    /// 来源。
    pub source: AccessSource,
}

/// 访问探针流汇总。
#[derive(Clone, Copy, Debug)]
pub struct AccessSummary {
    /// 探针总数。
    pub probes: u64,
    /// 放行数。
    pub allows: u64,
    /// 拒收数。
    pub denies: u64,
    /// 未映射拒收数（子集）。
    pub unmapped: u64,
    /// 其中 W^X 红线拒收数（子集）。
    pub wx_blocked: u64,
    /// 首个拒收码（全放行为 E_OK）。
    pub first_fail_code: u16,
}

// ═══════════════════════════════════════════════════════════════════════════
// 注入面（判据 J1(f)：三类注入各 100% 检出）
// ═══════════════════════════════════════════════════════════════════════════

/// 注入种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionKind {
    /// W^X 违例注入（W+X / R+W+X 请求）。
    WxViolation,
    /// 越权升级注入（mprotect 升权）。
    IllegalUpgrade,
    /// 未登记豁免注入（裸别名访问）。
    UnregisteredExempt,
}

impl InjectionKind {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            InjectionKind::WxViolation => "W^X违例注入",
            InjectionKind::IllegalUpgrade => "越权升级注入",
            InjectionKind::UnregisteredExempt => "未登记豁免注入",
        }
    }
}

/// 注入报告（检出数=拒收数；all_detected 要求案案拒收且码对号）。
#[derive(Clone, Copy, Debug)]
pub struct InjectionReport {
    /// 注入种类。
    pub kind: InjectionKind,
    /// 注入案数。
    pub attempts: u32,
    /// 检出（按预期码拒收）案数。
    pub detected: u32,
    /// 全检出位。
    pub all_detected: bool,
    /// 首案诊断码。
    pub first_code: u16,
}

// ═══════════════════════════════════════════════════════════════════════════
// 策略脚本（双跑确定性对拍用；迭代序=脚本序，显式冻结）
// ═══════════════════════════════════════════════════════════════════════════

/// 单条策略操作。
#[derive(Clone, Copy, Debug)]
pub enum PolicyOp {
    /// 登记区（类型/基址/长度/请求权限）。
    Register {
        /// 区域类型。
        rtype: RegionType,
        /// 基址。
        base: u64,
        /// 长度。
        len: u64,
        /// 请求权限。
        req: PermBits,
    },
    /// 纯裁决（类型/请求权限）。
    Adjudicate {
        /// 区域类型。
        rtype: RegionType,
        /// 请求权限。
        req: PermBits,
    },
    /// 权限变更（区号/新权限）。
    Mprotect {
        /// 区号。
        region: u32,
        /// 新权限。
        perms: PermBits,
    },
    /// 登记双映射豁免（物理页/W 别名/X 别名）。
    GrantExempt {
        /// 物理页基址。
        phys: u64,
        /// W 别名。
        va_w: u64,
        /// X 别名。
        va_x: u64,
    },
    /// 撤销豁免（条目号）。
    RevokeExempt {
        /// 条目号。
        exempt: u32,
    },
    /// 访问探针（虚地址/需求权限）。
    Access {
        /// 虚地址。
        va: u64,
        /// 需求权限。
        need: PermBits,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// 健康报告
// ═══════════════════════════════════════════════════════════════════════════

/// 引擎健康报告（health() 产物）。
#[derive(Clone, Copy, Debug)]
pub struct HealthReport {
    /// 矩阵审计全过（判据 J1(a)）。
    pub matrix_ok: bool,
    /// W^X 泄漏检查过（无任何放行案/在册区/豁免别名含 W+X）。
    pub wx_leak_ok: bool,
    /// 豁免一致性红线过（active == grants - revokes）。
    pub exempt_redline_ok: bool,
    /// 出厂豁免基线为零（豁免默认 0 红线）。
    pub exempt_baseline_zero: bool,
    /// 统计快照。
    pub stats: WxStats,
    /// 全账 FNV 指纹。
    pub fingerprint: u64,
    /// 引擎当前序号。
    pub seq: u64,
    /// 四项红线全过。
    pub all_ok: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// WxPolicyEngine：策略引擎主体
// ═══════════════════════════════════════════════════════════════════════════

/// 权限位策略引擎（W^X 矩阵裁决 + 裁决账 + 变更账 + 豁免账 + 事件账）。
///
/// 状态机：出厂（Open）→ 可登记区/豁免、可裁决、可变更；seal() 后
/// （Sealed）登记类操作拒收（BadState），裁决/变更/访问照常——策略
/// 引擎的运行期职责不因封板停止。
pub struct WxPolicyEngine {
    /// 全局序号源（wrapping 推进）。
    seq: u64,
    /// 出厂种子（折入指纹；双跑对拍同种子同指纹）。
    boot_seed: u64,
    /// 封板位。
    sealed: bool,
    /// 区登记账（按登记序 append，region_id 单调）。
    regions: Vec<RegionRec>,
    /// 下一区号。
    next_region_id: u32,
    /// 裁决账。
    decisions: Vec<DecisionRec>,
    /// 变更账。
    mprotects: Vec<MprotectRec>,
    /// 豁免账。
    exempts: Vec<ExemptRec>,
    /// 下一豁免条目号。
    next_exempt_id: u32,
    /// 事件账。
    events: Vec<EventRec>,
    /// 统计账。
    stats: WxStats,
}

impl WxPolicyEngine {
    /// 出厂（boot seed 折入 Boot 事件与指纹；豁免基线为零红线留痕）。
    pub fn new_seeded(seed: u64) -> WxPolicyEngine {
        let mut eng = WxPolicyEngine {
            seq: 0,
            boot_seed: seed,
            sealed: false,
            regions: Vec::new(),
            next_region_id: 1,
            decisions: Vec::new(),
            mprotects: Vec::new(),
            exempts: Vec::new(),
            next_exempt_id: 1,
            events: Vec::new(),
            stats: WxStats::zeroed(),
        };
        // 豁免默认 0 红线：出厂在册豁免恒为 0，统计账留真。
        eng.stats.exempt_boot_zero = true;
        eng.push_event(EventKind::Boot, E_OK, seed, 0);
        eng
    }

    /// 出厂（seed=0）。
    pub fn new() -> WxPolicyEngine {
        WxPolicyEngine::new_seeded(0)
    }

    /// 序号推进（wrapping 显式）。
    fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// 事件留痕（账满走丢包计数，不失败）。
    fn push_event(&mut self, kind: EventKind, code: u16, a: u64, b: u64) {
        if self.events.len() >= MAX_EVENTS {
            self.stats.events_dropped = self.stats.events_dropped.saturating_add(1);
            // 丢包也占一个序号，保证序号空间与账面规模一致。
            let _ = self.next_seq();
            return;
        }
        let seq = self.next_seq();
        self.events.push(EventRec {
            seq,
            kind,
            code,
            a,
            b,
        });
    }

    /// 裁决留痕（账满走丢包计数；结论照常返回）。
    fn record_decision(
        &mut self,
        region_id: u32,
        rtype: RegionType,
        requested: PermBits,
        granted: PermBits,
        verdict: Verdict,
        code: u16,
        wx_redline: bool,
    ) {
        let seq = self.next_seq();
        if self.decisions.len() >= MAX_DECISIONS {
            self.stats.decisions_dropped = self.stats.decisions_dropped.saturating_add(1);
        } else {
            self.decisions.push(DecisionRec {
                seq,
                region_id,
                rtype,
                requested,
                granted,
                verdict,
                code,
                wx_redline,
            });
        }
        // 统计口径不受账满影响。
        self.stats.decisions_total = self.stats.decisions_total.saturating_add(1);
        let ti = rtype.index();
        self.stats.per_type_decisions[ti] = self.stats.per_type_decisions[ti].saturating_add(1);
        match verdict {
            Verdict::Allow => {
                self.stats.allows = self.stats.allows.saturating_add(1);
            }
            Verdict::Deny => {
                self.stats.denies = self.stats.denies.saturating_add(1);
                self.stats.per_type_denies[ti] = self.stats.per_type_denies[ti].saturating_add(1);
                if wx_redline {
                    self.stats.wx_redlines = self.stats.wx_redlines.saturating_add(1);
                }
            }
        }
        self.push_event(EventKind::Decision, code, requested.0 as u64, rtype.index() as u64);
    }

    /// 裁决（带账面留痕）：矩阵裁决 + 裁决账 + 拒收率统计。
    ///
    /// 判据 J1(b)：请求含 W+X 一律红案（WxViolation，优先于矩阵判定）。
    pub fn adjudicate(
        &mut self,
        rtype: RegionType,
        req: PermBits,
    ) -> Result<PermBits, WxMatrixError> {
        match adjudicate_const(rtype, req) {
            Ok(granted) => {
                self.record_decision(NO_REGION, rtype, req, granted, Verdict::Allow, E_OK, false);
                Ok(granted)
            }
            Err(e) => {
                let wx = e.code() == WxMatrixError::WxViolation.code();
                self.record_decision(NO_REGION, rtype, req, PermBits::NONE, Verdict::Deny, e.code(), wx);
                Err(e)
            }
        }
    }

    /// 区形状校验（零长/对齐/回绕/重叠 → RegionShape）。
    fn validate_region_shape(&self, base: u64, len: u64) -> Result<(), WxMatrixError> {
        if len == 0 {
            return Err(WxMatrixError::RegionShape);
        }
        if base & PAGE_4K_MASK != 0 || len & PAGE_4K_MASK != 0 {
            return Err(WxMatrixError::RegionShape);
        }
        match base.checked_add(len) {
            Some(_) => {}
            None => return Err(WxMatrixError::RegionShape),
        }
        // 重叠检查：区间相交（经典 [a1,a2)∩[b1,b2)≠∅ ⟺ a1<b2 && b1<a2）。
        let end = base + len;
        let mut i = 0usize;
        while i < self.regions.len() {
            let r = self.regions[i];
            let rend = r.base.saturating_add(r.len);
            if base < rend && r.base < end {
                return Err(WxMatrixError::RegionShape);
            }
            i += 1;
        }
        Ok(())
    }

    /// 在册区之上找下一个空闲基址（注入面/脚本用；4K 对齐且抬高至少
    /// 0x10_0000=1MB，避开低 1MB trampoline 区，来源 F0318 低 1MB 预置）。
    pub fn next_free_base(&self) -> u64 {
        let mut max_end: u64 = 0x10_0000; // 1MB：低 1MB 恒等映射预留（F0318）
        let mut i = 0usize;
        while i < self.regions.len() {
            let r = self.regions[i];
            let end = r.base.saturating_add(r.len);
            if end > max_end {
                max_end = end;
            }
            i += 1;
        }
        // 已对齐到 4K（所有在册区端点均 4K 倍数；防御性再对齐一次）。
        (max_end + PAGE_4K_MASK) & !PAGE_4K_MASK
    }

    /// 登记区（类型+请求权限 → 矩阵裁决 → 实授=请求）：
    /// 拒收时裁决账已留痕（Deny 案），不产生区记录。
    pub fn register_region(
        &mut self,
        rtype: RegionType,
        base: u64,
        len: u64,
        req: PermBits,
    ) -> Result<u32, WxMatrixError> {
        if self.sealed {
            return Err(WxMatrixError::BadState);
        }
        self.validate_region_shape(base, len)?;
        if self.regions.len() >= MAX_REGIONS {
            self.push_event(EventKind::Overflow, WxMatrixError::LedgerFull.code(), 0, 1);
            return Err(WxMatrixError::LedgerFull);
        }
        let granted = match self.adjudicate(rtype, req) {
            Ok(g) => g,
            Err(e) => return Err(e), // 裁决账已在 adjudicate 内留痕
        };
        let region_id = self.next_region_id;
        self.next_region_id = self.next_region_id.wrapping_add(1);
        let seq = self.next_seq();
        self.regions.push(RegionRec {
            region_id,
            rtype,
            base,
            len,
            perms: granted,
            gen: 0,
            grant_seq: seq,
        });
        self.stats.regions_registered = self.stats.regions_registered.saturating_add(1);
        self.push_event(EventKind::RegionAdd, E_OK, base, len);
        Ok(region_id)
    }

    /// 区号查区。
    pub fn region(&self, region_id: u32) -> Option<RegionRec> {
        let mut i = 0usize;
        while i < self.regions.len() {
            if self.regions[i].region_id == region_id {
                return Some(self.regions[i]);
            }
            i += 1;
        }
        None
    }

    /// 区账只读视图。
    pub fn regions(&self) -> &[RegionRec] {
        &self.regions
    }

    /// 裁决账只读视图。
    pub fn decisions(&self) -> &[DecisionRec] {
        &self.decisions
    }

    /// 变更账只读视图。
    pub fn mprotects(&self) -> &[MprotectRec] {
        &self.mprotects
    }

    /// 豁免账只读视图。
    pub fn exemptions(&self) -> &[ExemptRec] {
        &self.exempts
    }

    /// 事件账只读视图。
    pub fn events(&self) -> &[EventRec] {
        &self.events
    }

    /// 统计快照。
    pub fn stats(&self) -> WxStats {
        self.stats
    }

    /// 虚地址落区查找（返回区记录；无命中 None）。
    pub fn region_of_va(&self, va: u64) -> Option<RegionRec> {
        let mut i = 0usize;
        while i < self.regions.len() {
            let r = self.regions[i];
            let end = r.base.saturating_add(r.len);
            if va >= r.base && va < end {
                return Some(r);
            }
            i += 1;
        }
        None
    }

    /// 权限变更（mprotect 语义，判据 J1(d)）：类型不变；变更同样过矩阵
    /// （W^X 红线优先）；降权（新 ⊆ 旧）自由放行，升权（新增任何位）一律
    /// 拒收（UpgradeDenied）。封板不影响本路径。
    pub fn mprotect(
        &mut self,
        region_id: u32,
        new_perms: PermBits,
    ) -> Result<PermBits, WxMatrixError> {
        // 取区（复制后立即释放借用，再走可变路径）。
        let rec = match self.region(region_id) {
            Some(r) => r,
            None => return Err(WxMatrixError::RegionUnknown),
        };
        let rtype = rec.rtype;
        let old_perms = rec.perms;
        // 第一步：矩阵裁决（含 W^X 红线；裁决账留痕）。
        let granted = match self.adjudicate(rtype, new_perms) {
            Ok(g) => g,
            Err(e) => {
                self.stats.mprotect_total = self.stats.mprotect_total.saturating_add(1);
                self.stats.mprotect_denied = self.stats.mprotect_denied.saturating_add(1);
                self.record_mprotect(region_id, rtype, old_perms, new_perms, Verdict::Deny, e.code());
                return Err(e);
            }
        };
        self.stats.mprotect_total = self.stats.mprotect_total.saturating_add(1);
        // 第二步：升降权判定（新 ⊆ 旧 → 降权/等权放行；否则升权拒）。
        if !granted.is_subset_of(old_perms) {
            self.stats.mprotect_denied = self.stats.mprotect_denied.saturating_add(1);
            self.record_mprotect(region_id, rtype, old_perms, new_perms, Verdict::Deny, WxMatrixError::UpgradeDenied.code());
            return Err(WxMatrixError::UpgradeDenied);
        }
        // 第三步：落账（世代 +1，wrapping）。
        let mut i = 0usize;
        while i < self.regions.len() {
            if self.regions[i].region_id == region_id {
                self.regions[i].perms = granted;
                self.regions[i].gen = self.regions[i].gen.wrapping_add(1);
                break;
            }
            i += 1;
        }
        self.stats.mprotect_downgrades = self.stats.mprotect_downgrades.saturating_add(1);
        self.record_mprotect(region_id, rtype, old_perms, new_perms, Verdict::Allow, E_OK);
        Ok(granted)
    }

    /// 变更留痕（账满丢包计数）。
    fn record_mprotect(
        &mut self,
        region_id: u32,
        rtype: RegionType,
        old_perms: PermBits,
        new_perms: PermBits,
        verdict: Verdict,
        code: u16,
    ) {
        if self.mprotects.len() >= MAX_MPROTECTS {
            // 变更账无独立丢包计数器，折入事件账 Overflow 口径。
            self.push_event(EventKind::Overflow, WxMatrixError::LedgerFull.code(), region_id as u64, 2);
            return;
        }
        let seq = self.next_seq();
        self.mprotects.push(MprotectRec {
            seq,
            region_id,
            rtype,
            old_perms,
            new_perms,
            verdict,
            code,
        });
        self.push_event(EventKind::Mprotect, code, region_id as u64, new_perms.0 as u64);
    }

    /// 登记双映射豁免（判据 J1(e)）：同一物理页显式登记 W 别名（R+W）与
    /// X 别名（R+X）；任一别名单独满足 W^X；两别名不落在任何在册区；
    /// 同一物理页同时至多一条在态豁免。封板后拒收。
    pub fn grant_exemption(
        &mut self,
        phys: u64,
        va_w: u64,
        va_x: u64,
    ) -> Result<u32, WxMatrixError> {
        if self.sealed {
            return Err(WxMatrixError::BadState);
        }
        // 形状校验：三址 4K 对齐 + 两别名不同址。
        if phys & PAGE_4K_MASK != 0 || va_w & PAGE_4K_MASK != 0 || va_x & PAGE_4K_MASK != 0 {
            self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
            return Err(WxMatrixError::ExemptIllegal);
        }
        if va_w == va_x {
            self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
            return Err(WxMatrixError::ExemptIllegal);
        }
        // 别名不得落在在册区（避免与区权限歧义）。
        if self.region_of_va(va_w).is_some() || self.region_of_va(va_x).is_some() {
            self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
            return Err(WxMatrixError::ExemptIllegal);
        }
        // 同物理页在态唯一。
        let mut i = 0usize;
        while i < self.exempts.len() {
            let e = self.exempts[i];
            if e.phys == phys && e.state == ExemptState::Active {
                self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
                return Err(WxMatrixError::ExemptIllegal);
            }
            // 两别名也不得与其他豁免条目别名撞车（在态）。
            if e.state == ExemptState::Active && (e.va_w == va_w || e.va_w == va_x || e.va_x == va_w || e.va_x == va_x) {
                self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
                return Err(WxMatrixError::ExemptIllegal);
            }
            i += 1;
        }
        if self.exempts.len() >= MAX_EXEMPTS {
            self.push_event(EventKind::Overflow, WxMatrixError::ExemptFull.code(), phys, 3);
            return Err(WxMatrixError::ExemptFull);
        }
        let exempt_id = self.next_exempt_id;
        self.next_exempt_id = self.next_exempt_id.wrapping_add(1);
        let seq = self.next_seq();
        self.exempts.push(ExemptRec {
            exempt_id,
            phys,
            va_w,
            va_x,
            w_alias: EXEMPT_W_ALIAS,
            x_alias: EXEMPT_X_ALIAS,
            state: ExemptState::Active,
            grant_seq: seq,
            revoke_seq: 0,
        });
        self.stats.exempt_grants = self.stats.exempt_grants.saturating_add(1);
        self.push_event(EventKind::ExemptGrant, E_OK, phys, va_w ^ va_x);
        Ok(exempt_id)
    }

    /// 撤销豁免（在态条目 → Revoked；重复撤销/查无 → ExemptState/
    /// ExemptUnknown）。封板后拒收。
    pub fn revoke_exemption(&mut self, exempt_id: u32) -> Result<(), WxMatrixError> {
        if self.sealed {
            return Err(WxMatrixError::BadState);
        }
        let mut found = false;
        let mut i = 0usize;
        while i < self.exempts.len() {
            if self.exempts[i].exempt_id == exempt_id {
                found = true;
                if self.exempts[i].state != ExemptState::Active {
                    self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
                    return Err(WxMatrixError::ExemptState);
                }
                let seq = self.next_seq();
                self.exempts[i].state = ExemptState::Revoked;
                self.exempts[i].revoke_seq = seq;
                break;
            }
            i += 1;
        }
        if !found {
            self.stats.exempt_rejected = self.stats.exempt_rejected.saturating_add(1);
            return Err(WxMatrixError::ExemptUnknown);
        }
        self.stats.exempt_revokes = self.stats.exempt_revokes.saturating_add(1);
        self.push_event(EventKind::ExemptRevoke, E_OK, exempt_id as u64, 0);
        Ok(())
    }

    /// 豁免条目查询。
    pub fn exemption(&self, exempt_id: u32) -> Option<ExemptRec> {
        let mut i = 0usize;
        while i < self.exempts.len() {
            if self.exempts[i].exempt_id == exempt_id {
                return Some(self.exempts[i]);
            }
            i += 1;
        }
        None
    }

    /// 在态豁免计数（线性对账）。
    pub fn exempt_active_count(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.exempts.len() {
            if self.exempts[i].state == ExemptState::Active {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 访问源解析（三源归一：在册区 → 豁免别名 → 未映射；返回命中面的
    /// 实授权限与来源，未命中为 (NONE, Unmapped)）。while+索引遍历。
    fn resolve_access_source(&self, va: u64) -> (PermBits, AccessSource) {
        if let Some(r) = self.region_of_va(va) {
            return (r.perms, AccessSource::Region(r.region_id));
        }
        let mut i = 0usize;
        while i < self.exempts.len() {
            let e = self.exempts[i];
            if e.state == ExemptState::Active {
                if va >= e.va_w && va < e.va_w.saturating_add(PAGE_4K) {
                    return (e.w_alias, AccessSource::ExemptW(e.exempt_id));
                }
                if va >= e.va_x && va < e.va_x.saturating_add(PAGE_4K) {
                    return (e.x_alias, AccessSource::ExemptX(e.exempt_id));
                }
            }
            i += 1;
        }
        (PermBits::NONE, AccessSource::Unmapped)
    }

    /// 访问裁决（三源归一）：需求位非法 → IllegalPermBits；先解析命中面，
    /// 再按"W^X 红线 > 未映射 > 矩阵子集"的码优先级裁决——需求含 W+X →
    /// WxViolation（不论命中面，与 adjudicate_const 同口径）；未落任何面
    /// → RegionUnknown（未登记访问一律拒，豁免默认 0 红线的运行期投影）；
    /// 命中面 need ⊄ effective → PermSuperset。拒收留事件账。
    pub fn check_access(&mut self, va: u64, need: PermBits) -> AccessVerdict {
        self.stats.access_probes = self.stats.access_probes.saturating_add(1);
        // 需求位闭集校验（先于源解析：非法需求无处落面）。
        if need.0 & !PERM_MASK != 0 || need.0 == 0 {
            self.stats.access_denies = self.stats.access_denies.saturating_add(1);
            let code = WxMatrixError::IllegalPermBits.code();
            self.push_event(EventKind::AccessDeny, code, va, need.0 as u64);
            return AccessVerdict {
                va,
                need,
                effective: PermBits::NONE,
                allowed: false,
                code,
                source: AccessSource::Unmapped,
            };
        }
        // 源解析（真实命中面进结论，供审计对账）。
        let (effective, source) = self.resolve_access_source(va);
        // 码优先级：W^X 红线 > 未映射 > 子集判定。
        let (allowed, code) = if need.is_wx_violation() {
            (false, WxMatrixError::WxViolation.code())
        } else if source == AccessSource::Unmapped {
            (false, WxMatrixError::RegionUnknown.code())
        } else if need.is_subset_of(effective) {
            (true, E_OK)
        } else {
            (false, WxMatrixError::PermSuperset.code())
        };
        if !allowed {
            self.stats.access_denies = self.stats.access_denies.saturating_add(1);
            if code == WxMatrixError::RegionUnknown.code() {
                self.stats.access_unmapped = self.stats.access_unmapped.saturating_add(1);
            }
            self.push_event(EventKind::AccessDeny, code, va, need.0 as u64);
        }
        AccessVerdict {
            va,
            need,
            effective,
            allowed,
            code,
            source,
        }
    }

    /// 访问探针流（批量；越 MAX_ACCESS_PROBES 拒整批）。
    pub fn probe_stream(&mut self, probes: &[(u64, PermBits)]) -> Result<AccessSummary, WxMatrixError> {
        if probes.len() > MAX_ACCESS_PROBES {
            return Err(WxMatrixError::LedgerFull);
        }
        let mut sum = AccessSummary {
            probes: 0,
            allows: 0,
            denies: 0,
            unmapped: 0,
            wx_blocked: 0,
            first_fail_code: E_OK,
        };
        let mut i = 0usize;
        while i < probes.len() {
            let (va, need) = probes[i];
            let v = self.check_access(va, need);
            sum.probes = sum.probes.saturating_add(1);
            if v.allowed {
                sum.allows = sum.allows.saturating_add(1);
            } else {
                sum.denies = sum.denies.saturating_add(1);
                if sum.first_fail_code == E_OK {
                    sum.first_fail_code = v.code;
                }
                if v.code == WxMatrixError::WxViolation.code() {
                    sum.wx_blocked = sum.wx_blocked.saturating_add(1);
                }
                if v.source == AccessSource::Unmapped {
                    sum.unmapped = sum.unmapped.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok(sum)
    }

    /// W^X 红线全扫描（判据 J1(b)：8 类 × {W+X, R+W+X} = 16 案全拒）。
    /// 返回（案数，检出数）；每案均走带账裁决。
    pub fn wx_redline_sweep(&mut self) -> (u32, u32) {
        let patterns = [PermBits(PERM_W | PERM_X), PermBits(PERM_R | PERM_W | PERM_X)];
        let mut attempts = 0u32;
        let mut detected = 0u32;
        let mut t = 0u8;
        while t < 8 {
            let rt = match RegionType::from_index(t) {
                Some(x) => x,
                None => {
                    t += 1;
                    continue;
                }
            };
            let mut j = 0usize;
            while j < patterns.len() {
                attempts += 1;
                match self.adjudicate(rt, patterns[j]) {
                    Err(e) => {
                        if e.code() == WxMatrixError::WxViolation.code() {
                            detected += 1;
                        }
                    }
                    Ok(_) => {
                        // W^X 请求被放行属红线破裂；账面已留 Allow 案，
                        // health() 的 wx_leak_ok 会兜底翻红。
                    }
                }
                j += 1;
            }
            t += 1;
        }
        (attempts, detected)
    }

    /// 封板（登记类操作自此拒收；裁决/变更/访问照常）。
    pub fn seal(&mut self) {
        if !self.sealed {
            self.sealed = true;
            self.push_event(EventKind::Seal, E_OK, self.regions.len() as u64, self.exempts.len() as u64);
        }
    }

    /// 封板位。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 出厂种子。
    pub fn boot_seed(&self) -> u64 {
        self.boot_seed
    }

    // ── 注入面 ─────────────────────────────────────────────────────────────

    /// 注入一：W^X 违例注入——16 案（8 类 × 2 形状）逐案要求 WxViolation
    /// 拒收；任何一案放行或错码即红线破裂（返回 Err(WxViolation)）。
    pub fn inject_wx_violation(&mut self) -> Result<InjectionReport, WxMatrixError> {
        let (attempts, detected) = self.wx_redline_sweep();
        let first_code = WxMatrixError::WxViolation.code();
        let all = attempts == WX_SWEEP_CASES && detected == attempts;
        self.push_event(
            EventKind::Injection,
            if all { E_OK } else { WxMatrixError::WxViolation.code() },
            InjectionKind::WxViolation as u64,
            detected as u64,
        );
        if !all {
            return Err(WxMatrixError::WxViolation);
        }
        Ok(InjectionReport {
            kind: InjectionKind::WxViolation,
            attempts,
            detected,
            all_detected: all,
            first_code,
        })
    }

    /// 注入二：越权升级注入——登记后尝试 mprotect 升权（含 W+X 目标），
    /// 逐案要求拒收（UpgradeDenied / WxViolation / PermSuperset 对号）。
    pub fn inject_illegal_upgrade(&mut self) -> Result<InjectionReport, WxMatrixError> {
        let base = self.next_free_base();
        let mut attempts = 0u32;
        let mut detected = 0u32;
        let mut first_code = E_OK;
        // 案 1：任务堆登记 R（矩阵行 rw- 的合法子集）后直接申请升权 RW
        // （矩阵可过、旧权不可过 → UpgradeDenied）。
        let rid = match self.register_region(RegionType::TaskHeap, base, PAGE_4K, PermBits::RO) {
            Ok(id) => id,
            Err(e) => return Err(e),
        };
        attempts += 1;
        match self.mprotect(rid, PermBits::RW) {
            Err(e) if e.code() == WxMatrixError::UpgradeDenied.code() => {
                detected += 1;
                if first_code == E_OK {
                    first_code = e.code();
                }
            }
            Err(e) => {
                if first_code == E_OK {
                    first_code = e.code();
                }
            }
            Ok(_) => {}
        }
        // 案 2：栈区 RW → R（降权成功）后 R → RW（升权回弹必拒）。
        let sid = match self.register_region(RegionType::TaskStack, base + PAGE_4K * 4, PAGE_4K * 4, PermBits::RW) {
            Ok(id) => id,
            Err(e) => return Err(e),
        };
        if let Ok(_) = self.mprotect(sid, PermBits::RO) {
            attempts += 1;
            match self.mprotect(sid, PermBits::RW) {
                Err(e) if e.code() == WxMatrixError::UpgradeDenied.code() => {
                    detected += 1;
                    if first_code == E_OK {
                        first_code = e.code();
                    }
                }
                Err(e) => {
                    if first_code == E_OK {
                        first_code = e.code();
                    }
                }
                Ok(_) => {}
            }
        }
        // 案 3：动态代码区 RX → RWX（W+X 目标，红线案）。
        let did = match self.register_region(RegionType::DynCode, base + PAGE_4K * 16, PAGE_4K, PermBits::RX) {
            Ok(id) => id,
            Err(e) => return Err(e),
        };
        attempts += 1;
        match self.mprotect(did, PermBits(PERM_R | PERM_W | PERM_X)) {
            Err(e) if e.code() == WxMatrixError::WxViolation.code() => {
                detected += 1;
                if first_code == E_OK {
                    first_code = e.code();
                }
            }
            Err(e) => {
                if first_code == E_OK {
                    first_code = e.code();
                }
            }
            Ok(_) => {}
        }
        // 案 4：数据区 RW → RX（矩阵外，PermSuperset）。
        let kid = match self.register_region(RegionType::KernelData, base + PAGE_4K * 32, PAGE_4K, PermBits::RW) {
            Ok(id) => id,
            Err(e) => return Err(e),
        };
        attempts += 1;
        match self.mprotect(kid, PermBits::RX) {
            Err(e) if e.code() == WxMatrixError::PermSuperset.code() => {
                detected += 1;
                if first_code == E_OK {
                    first_code = e.code();
                }
            }
            Err(e) => {
                if first_code == E_OK {
                    first_code = e.code();
                }
            }
            Ok(_) => {}
        }
        let all = attempts > 0 && detected == attempts;
        self.push_event(
            EventKind::Injection,
            if all { E_OK } else { WxMatrixError::UpgradeDenied.code() },
            InjectionKind::IllegalUpgrade as u64,
            ((attempts as u64) << 32) | detected as u64,
        );
        if !all {
            return Err(WxMatrixError::UpgradeDenied);
        }
        Ok(InjectionReport {
            kind: InjectionKind::IllegalUpgrade,
            attempts,
            detected,
            all_detected: all,
            first_code,
        })
    }

    /// 注入三：未登记豁免注入——在无任何豁免登记的地址上做裸别名访问
    /// （W 面/X 面各一案）+ W+X 需求一案；要求全部拒收且在态豁免数
    /// 保持不变（无隐式豁免物化）。任何放行即 Err(ExemptIllegal)。
    pub fn inject_unregistered_exempt(&mut self) -> Result<InjectionReport, WxMatrixError> {
        let before_active = self.exempt_active_count();
        let base = self.next_free_base();
        let va_w_probe = base;                       // 裸 W 别名候选
        let va_x_probe = base.saturating_add(PAGE_4K * 8); // 裸 X 别名候选
        let mut attempts = 0u32;
        let mut detected = 0u32;
        let mut first_code = E_OK;
        // 案 1：裸 W 面写访问（无登记）→ 必须 RegionUnknown 拒。
        attempts += 1;
        let v1 = self.check_access(va_w_probe, PermBits::RW);
        if !v1.allowed && v1.code == WxMatrixError::RegionUnknown.code() {
            detected += 1;
            if first_code == E_OK {
                first_code = v1.code;
            }
        } else if first_code == E_OK {
            first_code = v1.code;
        }
        // 案 2：裸 X 面取指访问（无登记）→ 必须 RegionUnknown 拒。
        attempts += 1;
        let v2 = self.check_access(va_x_probe, PermBits::RX);
        if !v2.allowed && v2.code == WxMatrixError::RegionUnknown.code() {
            detected += 1;
            if first_code == E_OK {
                first_code = v2.code;
            }
        } else if first_code == E_OK {
            first_code = v2.code;
        }
        // 案 3：未登记地址上的 W+X 需求 → 必须 WxViolation 红案。
        attempts += 1;
        let v3 = self.check_access(va_x_probe, PermBits(PERM_W | PERM_X));
        if !v3.allowed && v3.code == WxMatrixError::WxViolation.code() {
            detected += 1;
            if first_code == E_OK {
                first_code = v3.code;
            }
        } else if first_code == E_OK {
            first_code = v3.code;
        }
        // 无隐式豁免物化：在态计数必须不变。
        let after_active = self.exempt_active_count();
        let stable = before_active == after_active;
        let all = attempts > 0 && detected == attempts && stable;
        self.push_event(
            EventKind::Injection,
            if all { E_OK } else { WxMatrixError::ExemptIllegal.code() },
            InjectionKind::UnregisteredExempt as u64,
            detected as u64,
        );
        if !all {
            return Err(WxMatrixError::ExemptIllegal);
        }
        Ok(InjectionReport {
            kind: InjectionKind::UnregisteredExempt,
            attempts,
            detected,
            all_detected: all,
            first_code,
        })
    }

    // ── 指纹与健康 ─────────────────────────────────────────────────────────

    /// 全账 FNV-1a 指纹（折叠序冻结：boot → 区账 → 裁决账 → 变更账 →
    /// 豁免账 → 事件账 → 统计账 → 矩阵；全部 while+索引遍历）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_u64(h, MODULE_ID as u64);
        h = fnv_u64(h, self.seq);
        h = fnv_u64(h, self.boot_seed);
        h = fnv_u64(h, if self.sealed { 1 } else { 0 });
        // 区账。
        let mut i = 0usize;
        while i < self.regions.len() {
            let r = self.regions[i];
            h = fnv_u32(h, r.region_id);
            h = fnv_byte(h, r.rtype.index() as u8);
            h = fnv_u64(h, r.base);
            h = fnv_u64(h, r.len);
            h = fnv_byte(h, r.perms.0);
            h = fnv_u32(h, r.gen);
            h = fnv_u64(h, r.grant_seq);
            i += 1;
        }
        // 裁决账。
        let mut i = 0usize;
        while i < self.decisions.len() {
            let d = self.decisions[i];
            h = fnv_u64(h, d.seq);
            h = fnv_u32(h, d.region_id);
            h = fnv_byte(h, d.rtype.index() as u8);
            h = fnv_byte(h, d.requested.0);
            h = fnv_byte(h, d.granted.0);
            h = fnv_byte(h, d.verdict.bit());
            h = fnv_u32(h, d.code as u32);
            h = fnv_byte(h, if d.wx_redline { 1 } else { 0 });
            i += 1;
        }
        // 变更账。
        let mut i = 0usize;
        while i < self.mprotects.len() {
            let m = self.mprotects[i];
            h = fnv_u64(h, m.seq);
            h = fnv_u32(h, m.region_id);
            h = fnv_byte(h, m.rtype.index() as u8);
            h = fnv_byte(h, m.old_perms.0);
            h = fnv_byte(h, m.new_perms.0);
            h = fnv_byte(h, m.verdict.bit());
            h = fnv_u32(h, m.code as u32);
            i += 1;
        }
        // 豁免账。
        let mut i = 0usize;
        while i < self.exempts.len() {
            let e = self.exempts[i];
            h = fnv_u32(h, e.exempt_id);
            h = fnv_u64(h, e.phys);
            h = fnv_u64(h, e.va_w);
            h = fnv_u64(h, e.va_x);
            h = fnv_byte(h, e.w_alias.0);
            h = fnv_byte(h, e.x_alias.0);
            h = fnv_byte(h, e.state.bit());
            h = fnv_u64(h, e.grant_seq);
            h = fnv_u64(h, e.revoke_seq);
            i += 1;
        }
        // 事件账。
        let mut i = 0usize;
        while i < self.events.len() {
            let e = self.events[i];
            h = fnv_u64(h, e.seq);
            h = fnv_byte(h, e.kind.idx());
            h = fnv_u32(h, e.code as u32);
            h = fnv_u64(h, e.a);
            h = fnv_u64(h, e.b);
            i += 1;
        }
        // 统计账（含丢包计数，账满不影响确定性）。
        h = fnv_u64(h, self.stats.decisions_total);
        h = fnv_u64(h, self.stats.allows);
        h = fnv_u64(h, self.stats.denies);
        h = fnv_u64(h, self.stats.wx_redlines);
        h = fnv_u64(h, self.stats.decisions_dropped);
        h = fnv_u64(h, self.stats.mprotect_total);
        h = fnv_u64(h, self.stats.mprotect_downgrades);
        h = fnv_u64(h, self.stats.mprotect_denied);
        h = fnv_u64(h, self.stats.regions_registered);
        h = fnv_u64(h, self.stats.exempt_grants);
        h = fnv_u64(h, self.stats.exempt_revokes);
        h = fnv_u64(h, self.stats.exempt_rejected);
        h = fnv_byte(h, if self.stats.exempt_boot_zero { 1 } else { 0 });
        h = fnv_u64(h, self.stats.access_probes);
        h = fnv_u64(h, self.stats.access_denies);
        h = fnv_u64(h, self.stats.access_unmapped);
        h = fnv_u64(h, self.stats.events_dropped);
        let mut k = 0usize;
        while k < 8 {
            h = fnv_u64(h, self.stats.per_type_decisions[k]);
            h = fnv_u64(h, self.stats.per_type_denies[k]);
            k += 1;
        }
        // 矩阵（跨实例恒定项，折入防"同账异矩阵"漂移）。
        h = fnv_u64(h, audit_matrix().fingerprint);
        h
    }

    /// W^X 泄漏检查：裁决账无 W+X 放行案、区账无 W+X 实授、豁免别名
    /// 无 W+X 形状（豁免别名是冻结常量，此处仍逐条复核）。
    fn wx_leak_check(&self) -> bool {
        let mut i = 0usize;
        while i < self.decisions.len() {
            let d = self.decisions[i];
            if d.verdict == Verdict::Allow && d.granted.is_wx_violation() {
                return false;
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.regions.len() {
            if self.regions[i].perms.is_wx_violation() {
                return false;
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.exempts.len() {
            let e = self.exempts[i];
            if e.w_alias.is_wx_violation() || e.x_alias.is_wx_violation() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 健康报告（矩阵全过 + W^X 无泄漏 + 豁免一致 + 基线为零 + 指纹）。
    pub fn health(&self) -> HealthReport {
        let matrix_ok = audit_matrix().all_ok;
        let wx_leak_ok = self.wx_leak_check();
        let active = self.exempt_active_count();
        let est = self.stats.exempt_active_estimate();
        let exempt_redline_ok = active == est;
        let exempt_baseline_zero = self.stats.exempt_boot_zero && self.stats.exempt_active_estimate() == active;
        let stats = self.stats;
        let fingerprint = self.fingerprint();
        let seq = self.seq;
        let all_ok = matrix_ok && wx_leak_ok && exempt_redline_ok && exempt_baseline_zero;
        HealthReport {
            matrix_ok,
            wx_leak_ok,
            exempt_redline_ok,
            exempt_baseline_zero,
            stats,
            fingerprint,
            seq,
            all_ok,
        }
    }

    /// 人读摘要（诊断命令消费；F0313 转储视图口径）。
    pub fn format_summary(&self) -> String {
        let h = self.health();
        format!(
            "[UNX-F0306 W^X矩阵] seq={} 区={} 裁决={} 放行={} 拒={} 拒收率={}.{}% \
             红线案={} 变更(降/拒)={}/{} 豁免(登/撤/在)={}/{}/{} 访问(总/拒/未映射)={}/{}/{} \
             丢包(裁决/事件)={}/{} 矩阵过={} 泄漏={} 指纹=0x{:016X}",
            h.seq,
            self.regions.len(),
            h.stats.decisions_total,
            h.stats.allows,
            h.stats.denies,
            h.stats.reject_rate_per_mille() / 10,
            h.stats.reject_rate_per_mille() % 10,
            h.stats.wx_redlines,
            h.stats.mprotect_downgrades,
            h.stats.mprotect_denied,
            h.stats.exempt_grants,
            h.stats.exempt_revokes,
            self.exempt_active_count(),
            h.stats.access_probes,
            h.stats.access_denies,
            h.stats.access_unmapped,
            h.stats.decisions_dropped,
            h.stats.events_dropped,
            if h.matrix_ok { "Y" } else { "N" },
            if h.wx_leak_ok { "无" } else { "有" },
            h.fingerprint,
        )
    }

    // ── 双跑确定性对拍 ─────────────────────────────────────────────────────

    /// 执行策略脚本（顺序=脚本序；首错即停，Err 原样上抛）。
    pub fn run_script(seed: u64, ops: &[PolicyOp]) -> Result<WxPolicyEngine, WxMatrixError> {
        let mut eng = WxPolicyEngine::new_seeded(seed);
        let mut i = 0usize;
        while i < ops.len() {
            match &ops[i] {
                PolicyOp::Register {
                    rtype,
                    base,
                    len,
                    req,
                } => {
                    eng.register_region(*rtype, *base, *len, *req)?;
                }
                PolicyOp::Adjudicate { rtype, req } => {
                    eng.adjudicate(*rtype, *req)?;
                }
                PolicyOp::Mprotect { region, perms } => {
                    eng.mprotect(*region, *perms)?;
                }
                PolicyOp::GrantExempt { phys, va_w, va_x } => {
                    eng.grant_exemption(*phys, *va_w, *va_x)?;
                }
                PolicyOp::RevokeExempt { exempt } => {
                    eng.revoke_exemption(*exempt)?;
                }
                PolicyOp::Access { va, need } => {
                    // 访问拒收是合法剧本步（不问号上抛——脚本语义关心的是
                    // 账面演化，拒收同样确定性演化）。
                    let _ = eng.check_access(*va, *need);
                }
            }
            i += 1;
        }
        Ok(eng)
    }

    /// 双跑确定性对拍：同 seed 同脚本跑两遍，比对指纹（含失败路径：两跑
    /// 同错码折叠为 0x8000..|code 后比对）。漂移返回 DeterminismDrift。
    pub fn dual_run_determinism(seed: u64, ops: &[PolicyOp]) -> Result<u64, WxMatrixError> {
        let fp_a = match WxPolicyEngine::run_script(seed, ops) {
            Ok(e) => e.fingerprint(),
            Err(e) => 0x8000_0000_0000_0000u64 | (e.code() as u64),
        };
        let fp_b = match WxPolicyEngine::run_script(seed, ops) {
            Ok(e) => e.fingerprint(),
            Err(e) => 0x8000_0000_0000_0000u64 | (e.code() as u64),
        };
        if fp_a != fp_b {
            return Err(WxMatrixError::DeterminismDrift);
        }
        Ok(fp_a)
    }
}

impl Default for WxPolicyEngine {
    fn default() -> Self {
        WxPolicyEngine::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 测试（探针壳 cargo test 消费；测试内运行期 assert! 属测试断言惯例）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec;

    // ── J1(a)：区域类型→RWX 矩阵全过 ─────────────────────────────────────

    #[test]
    fn test_matrix_full_pass() {
        let audit = audit_matrix();
        assert!(audit.all_ok, "矩阵审计必须 8/8 全过");
        assert_eq!(audit.rows.len(), 8);
        let mut i = 0usize;
        while i < 8 {
            let r = audit.rows[i];
            assert!(r.no_wx, "{} 行不得含 W+X", r.rtype.name());
            assert!(r.has_r, "{} 行必须含 R", r.rtype.name());
            assert!(r.matches_const, "{} 行双源对拍一致", r.rtype.name());
            // PTE 编码纵深：行含 X → NX 清零；行无 X → NX 置位；行无 W → RW 清零。
            let expect_nx = !r.row.has_x();
            let nx_set = r.pte & PTE_NX != 0;
            assert_eq!(nx_set, expect_nx, "{} 行 NX 编码", r.rtype.name());
            let rw_set = r.pte & PTE_RW != 0;
            assert_eq!(rw_set, r.row.has_w(), "{} 行 RW 编码", r.rtype.name());
            assert!(r.pte & PTE_P != 0, "{} 行 P 位恒置", r.rtype.name());
            i += 1;
        }
        // 矩阵指纹跨调用恒定。
        assert_eq!(audit.fingerprint, audit_matrix().fingerprint);
    }

    #[test]
    fn test_const_row_asserts_runtime_mirror() {
        // 编译期逐行断言的运行期镜像（同口径复核）。
        let mut i = 0usize;
        while i < 8 {
            let rt = RegionType::from_index(i as u8);
            assert!(rt.is_some());
            let rt = rt.unwrap_or(RegionType::KernelCode);
            let row = rt.matrix_row();
            assert!(!row.is_wx_violation());
            assert!(row.has_r());
            assert!(!row.is_empty());
            i += 1;
        }
        // 表源镜像。
        let mut i = 0usize;
        while i < 8 {
            assert!(RWX_MATRIX[i][0] == 1);
            assert!(!(RWX_MATRIX[i][1] == 1 && RWX_MATRIX[i][2] == 1));
            i += 1;
        }
    }

    #[test]
    fn test_adjudicate_all_types_pass() {
        // J1(a) 后半：合法子集请求逐类放行（请求 ⊆ 行 → Ok(请求)）。
        let mut eng = WxPolicyEngine::new();
        // 每类的全部合法子集（手工展开，非空且 ⊆ 行）。
        let cases: [(RegionType, PermBits); 14] = [
            (RegionType::KernelCode, PermBits::RX),
            (RegionType::KernelCode, PermBits::RO),
            (RegionType::KernelCode, PermBits(PERM_X)),
            (RegionType::KernelRoData, PermBits::RO),
            (RegionType::KernelData, PermBits::RW),
            (RegionType::KernelData, PermBits::RO),
            (RegionType::KernelData, PermBits(PERM_W)),
            (RegionType::TaskStack, PermBits::RW),
            (RegionType::TaskHeap, PermBits::RO),
            (RegionType::DeviceMmio, PermBits::RW),
            (RegionType::PageTableSelf, PermBits::RO),
            (RegionType::DynCode, PermBits::RX),
            (RegionType::DynCode, PermBits::RO),
            (RegionType::TaskStack, PermBits(PERM_W)),
        ];
        let mut i = 0usize;
        while i < cases.len() {
            let (t, req) = cases[i];
            let got = eng.adjudicate(t, req);
            assert!(
                got.is_ok(),
                "合法子集请求被误拒：{} {}",
                t.name(),
                req.rwx_str()
            );
            let g = got.unwrap_or(PermBits::NONE);
            assert_eq!(g.0, req.0, "实授必须恰等于请求");
            i += 1;
        }
        // 全部留痕为 Allow。
        assert_eq!(eng.stats().allows, cases.len() as u64);
        assert_eq!(eng.stats().denies, 0);
    }

    // ── J1(b)：W^X 违例 100% 检出 ────────────────────────────────────────

    #[test]
    fn test_wx_violation_100pct() {
        let mut eng = WxPolicyEngine::new();
        let (attempts, detected) = eng.wx_redline_sweep();
        assert_eq!(attempts, WX_SWEEP_CASES, "全扫描案数 8×2=16");
        assert_eq!(detected, attempts, "W^X 检出必须 100%");
        // 裁决账红线位全标。
        let mut redlines = 0u64;
        let ds = eng.decisions();
        let mut i = 0usize;
        while i < ds.len() {
            if ds[i].wx_redline {
                redlines += 1;
            }
            i += 1;
        }
        assert_eq!(redlines, WX_SWEEP_CASES as u64);
        assert_eq!(eng.stats().wx_redlines, WX_SWEEP_CASES as u64);
        // 拒收率 1000‰。
        assert_eq!(eng.stats().reject_rate_per_mille(), 1000);
        // health 无泄漏。
        assert!(eng.health().wx_leak_ok);
    }

    #[test]
    fn test_wx_precedence_over_matrix() {
        let mut eng = WxPolicyEngine::new();
        // 对每类：W+X 请求必须报 WxViolation（而非 PermSuperset），
        // 即使该类矩阵行本就无 W（如 RoData）。
        let mut i = 0u8;
        while i < 8 {
            let t = RegionType::from_index(i).unwrap_or(RegionType::KernelCode);
            match eng.adjudicate(t, PermBits(PERM_W | PERM_X)) {
                Err(e) => assert_eq!(e, WxMatrixError::WxViolation),
                Ok(_) => assert!(false, "W^X 请求被放行：{}", t.name()),
            }
            i += 1;
        }
    }

    // ── 权限位闭集校验 ────────────────────────────────────────────────────

    #[test]
    fn test_illegal_perm_bits() {
        let mut eng = WxPolicyEngine::new();
        // 空位。
        let e1 = eng.adjudicate(RegionType::KernelData, PermBits::NONE);
        assert!(matches!(e1, Err(WxMatrixError::IllegalPermBits)));
        // 闭集外位（bit3）。
        let e2 = eng.adjudicate(RegionType::KernelData, PermBits(0b1000));
        assert!(matches!(e2, Err(WxMatrixError::IllegalPermBits)));
        // PermBits::new 同口径。
        assert!(PermBits::new(0).is_none());
        assert!(PermBits::new(0b1111).is_none());
        assert!(PermBits::new(0b011).is_some());
    }

    #[test]
    fn test_superset_denied() {
        let mut eng = WxPolicyEngine::new();
        // 只读数据请求 RW → 超集拒。
        let e1 = eng.adjudicate(RegionType::KernelRoData, PermBits::RW);
        assert!(matches!(e1, Err(WxMatrixError::PermSuperset)));
        // 可写数据请求 RX → 超集拒（非 W^X：无 W+X 并存）。
        let e2 = eng.adjudicate(RegionType::KernelData, PermBits::RX);
        assert!(matches!(e2, Err(WxMatrixError::PermSuperset)));
        // 页表自身请求 R+X → 超集拒。
        let e3 = eng.adjudicate(RegionType::PageTableSelf, PermBits::RX);
        assert!(matches!(e3, Err(WxMatrixError::PermSuperset)));
        // 编译期镜像。
        const CT: Result<PermBits, WxMatrixError> =
            adjudicate_const(RegionType::PageTableSelf, PermBits(0b101));
        const _: () = assert!(matches!(CT, Err(WxMatrixError::PermSuperset)));
    }

    // ── 区登记与裁决账 ────────────────────────────────────────────────────

    #[test]
    fn test_register_region_and_stats() {
        let mut eng = WxPolicyEngine::new();
        let id1 = eng
            .register_region(RegionType::KernelCode, 0x1_0000, 0x2000, PermBits::RX)
            .unwrap_or(0);
        assert!(id1 > 0);
        // 形状非法族。
        assert!(matches!(
            eng.register_region(RegionType::KernelData, 0x1_000, 0, PermBits::RW),
            Err(WxMatrixError::RegionShape)
        )); // 零长
        assert!(matches!(
            eng.register_region(RegionType::KernelData, 0x1_001, 0x1000, PermBits::RW),
            Err(WxMatrixError::RegionShape)
        )); // 未对齐
        assert!(matches!(
            eng.register_region(RegionType::KernelData, 0xFFFF_FFFF_FFFF_F000, 0x3000, PermBits::RW),
            Err(WxMatrixError::RegionShape)
        )); // 回绕
        assert!(matches!(
            eng.register_region(RegionType::KernelData, 0x1_0000, 0x1000, PermBits::RW),
            Err(WxMatrixError::RegionShape)
        )); // 与 id1 重叠
        // 矩阵拒绝的登记：裁决账留 Deny 案。
        assert!(matches!(
            eng.register_region(RegionType::KernelRoData, 0x2_0000, 0x1000, PermBits::RW),
            Err(WxMatrixError::PermSuperset)
        ));
        let st = eng.stats();
        assert_eq!(st.regions_registered, 1);
        // 形状案先于裁决（不走裁决账）：1 放行 + 1 矩阵拒 = 2 案。
        assert_eq!(st.decisions_total, 2);
        assert_eq!(st.allows, 1);
        assert_eq!(st.denies, 1);
        assert_eq!(eng.regions().len(), 1);
        // region 查询。
        let r = eng.region(id1).unwrap_or(RegionRec {
            region_id: 0,
            rtype: RegionType::KernelCode,
            base: 0,
            len: 0,
            perms: PermBits::NONE,
            gen: 0,
            grant_seq: 0,
        });
        assert_eq!(r.perms.0, PermBits::RX.0);
        assert_eq!(r.gen, 0);
    }

    #[test]
    fn test_ledger_full_regions() {
        let mut eng = WxPolicyEngine::new();
        let mut ok = 0u32;
        let mut last_code = E_OK;
        let mut i = 0u64;
        while i < (MAX_REGIONS as u64) + 8 {
            let base = 0x10_0000u64 + i * 0x10_000;
            match eng.register_region(RegionType::TaskHeap, base, 0x1000, PermBits::RW) {
                Ok(_) => ok += 1,
                Err(e) => {
                    last_code = e.code();
                    break;
                }
            }
            i += 1;
        }
        assert_eq!(ok as usize, MAX_REGIONS, "恰好登记到上限");
        assert_eq!(eng.regions().len(), MAX_REGIONS);
        assert_eq!(last_code, WxMatrixError::LedgerFull.code());
    }

    // ── mprotect 变更路径 ─────────────────────────────────────────────────

    #[test]
    fn test_mprotect_downgrade_free_upgrade_denied() {
        let mut eng = WxPolicyEngine::new();
        let sid = eng
            .register_region(RegionType::TaskStack, 0x10_0000, 0x8_000, PermBits::RW)
            .unwrap_or(0);
        // 降权：RW → R。
        let g1 = eng.mprotect(sid, PermBits::RO);
        assert!(g1.is_ok());
        assert_eq!(g1.unwrap_or(PermBits::NONE).0, PermBits::RO.0);
        // 再降：R → R（等权，仍属 ⊆ 旧，放行）。
        assert!(eng.mprotect(sid, PermBits::RO).is_ok());
        // 升权：R → RW 必拒。
        assert!(matches!(
            eng.mprotect(sid, PermBits::RW),
            Err(WxMatrixError::UpgradeDenied)
        ));
        // 升权目标纵使在矩阵行内也拒（冻结实授只降不升）。
        assert!(matches!(
            eng.mprotect(sid, PermBits(PERM_W)),
            Err(WxMatrixError::UpgradeDenied)
        ));
        // 变更到 W+X：红线案（先于升降权判定）。
        assert!(matches!(
            eng.mprotect(sid, PermBits(PERM_W | PERM_X)),
            Err(WxMatrixError::WxViolation)
        ));
        // 变更到矩阵外：PermSuperset。
        assert!(matches!(
            eng.mprotect(sid, PermBits::RX),
            Err(WxMatrixError::PermSuperset)
        ));
        // 世代计数：2 次成功降权。
        let r = eng.region(sid).unwrap_or(RegionRec {
            region_id: 0,
            rtype: RegionType::TaskStack,
            base: 0,
            len: 0,
            perms: PermBits::NONE,
            gen: 0,
            grant_seq: 0,
        });
        assert_eq!(r.gen, 2);
        // 变更账口径。
        let st = eng.stats();
        assert_eq!(st.mprotect_total, 6);
        assert_eq!(st.mprotect_downgrades, 2);
        assert_eq!(st.mprotect_denied, 4);
        // 未知区。
        assert!(matches!(
            eng.mprotect(0x7FFF, PermBits::RO),
            Err(WxMatrixError::RegionUnknown)
        ));
    }

    #[test]
    fn test_mprotect_type_frozen() {
        let mut eng = WxPolicyEngine::new();
        let did = eng
            .register_region(RegionType::DynCode, 0x10_0000, 0x1000, PermBits::RX)
            .unwrap_or(0);
        // 降权到 RO 后类型不变（仍 DynCode），再尝试 RW 变更：
        // DynCode 行无 W → PermSuperset（类型不变性经矩阵行体现）。
        assert!(eng.mprotect(did, PermBits::RO).is_ok());
        assert!(matches!(
            eng.mprotect(did, PermBits::RW),
            Err(WxMatrixError::PermSuperset)
        ));
        let r = eng.region(did).unwrap_or(RegionRec {
            region_id: 0,
            rtype: RegionType::KernelCode,
            base: 0,
            len: 0,
            perms: PermBits::NONE,
            gen: 0,
            grant_seq: 0,
        });
        assert_eq!(r.rtype, RegionType::DynCode);
    }

    // ── 豁免通道 ─────────────────────────────────────────────────────────

    #[test]
    fn test_exempt_channel_lifecycle() {
        let mut eng = WxPolicyEngine::new();
        // 出厂基线：在态 0。
        assert_eq!(eng.exempt_active_count(), 0);
        let phys = 0x2_0000_0000u64;
        let va_w = 0x7F_0000_0000u64;
        let va_x = 0x7F_0001_0000u64;
        let eid = eng.grant_exemption(phys, va_w, va_x).unwrap_or(0);
        assert!(eid > 0);
        assert_eq!(eng.exempt_active_count(), 1);
        // W 别名访问：写需求放行，取指拒。
        let vw = eng.check_access(va_w + 0x123, PermBits::RW);
        assert!(vw.allowed);
        assert!(matches!(vw.source, AccessSource::ExemptW(id) if id == eid));
        let vx_on_w = eng.check_access(va_w, PermBits::RX);
        assert!(!vx_on_w.allowed);
        assert_eq!(vx_on_w.code, WxMatrixError::PermSuperset.code());
        // X 别名访问：取指放行，写拒。
        let vx = eng.check_access(va_x + 0x40, PermBits::RX);
        assert!(vx.allowed);
        assert!(matches!(vx.source, AccessSource::ExemptX(id) if id == eid));
        let vw_on_x = eng.check_access(va_x, PermBits::RW);
        assert!(!vw_on_x.allowed);
        assert_eq!(vw_on_x.code, WxMatrixError::PermSuperset.code());
        // 任一别名上的 W+X 需求：红案。
        let vwx = eng.check_access(va_w, PermBits(PERM_W | PERM_X));
        assert!(!vwx.allowed);
        assert_eq!(vwx.code, WxMatrixError::WxViolation.code());
        // 同物理页重复登记拒。
        assert!(matches!(
            eng.grant_exemption(phys, va_w + 0x10_000, va_x + 0x10_000),
            Err(WxMatrixError::ExemptIllegal)
        ));
        // 别名撞车拒。
        assert!(matches!(
            eng.grant_exemption(phys + 0x1000, va_w, va_x + 0x20_000),
            Err(WxMatrixError::ExemptIllegal)
        ));
        // 未对齐/同址拒。
        assert!(matches!(
            eng.grant_exemption(phys + 0x123, va_w + 0x30_000, va_x + 0x30_000),
            Err(WxMatrixError::ExemptIllegal)
        ));
        assert!(matches!(
            eng.grant_exemption(phys + 0x2000, va_w + 0x40_000, va_w + 0x40_000),
            Err(WxMatrixError::ExemptIllegal)
        ));
        // 撤销后别名失效（未映射拒）。
        assert!(eng.revoke_exemption(eid).is_ok());
        assert_eq!(eng.exempt_active_count(), 0);
        let v_after = eng.check_access(va_w, PermBits::RW);
        assert!(!v_after.allowed);
        assert_eq!(v_after.code, WxMatrixError::RegionUnknown.code());
        // 重复撤销拒。
        assert!(matches!(
            eng.revoke_exemption(eid),
            Err(WxMatrixError::ExemptState)
        ));
        // 查无条目拒。
        assert!(matches!(
            eng.revoke_exemption(0x6FFF),
            Err(WxMatrixError::ExemptUnknown)
        ));
        // 撤销后同物理页可再登记。
        let eid2 = eng.grant_exemption(phys, va_w, va_x).unwrap_or(0);
        assert!(eid2 > eid);
        assert_eq!(eng.exempt_active_count(), 1);
        // 统计口径（4 形状拒 + 1 重复撤销 + 1 查无 = 6 拒收）。
        let st = eng.stats();
        assert_eq!(st.exempt_grants, 2);
        assert_eq!(st.exempt_revokes, 1);
        assert_eq!(st.exempt_rejected, 6);
        // health 豁免一致。
        assert!(eng.health().exempt_redline_ok);
    }

    #[test]
    fn test_exempt_alias_not_in_region() {
        let mut eng = WxPolicyEngine::new();
        let base = eng.next_free_base();
        let rid = eng
            .register_region(RegionType::TaskHeap, base, 0x10_000, PermBits::RW)
            .unwrap_or(0);
        assert!(rid > 0);
        // 别名落在在册区内 → 拒。
        assert!(matches!(
            eng.grant_exemption(0x3_0000_0000, base + 0x1_000, base + 0x2_000),
            Err(WxMatrixError::ExemptIllegal)
        ));
        // 区外别名可登记。
        assert!(eng
            .grant_exemption(0x3_0000_0000, base + 0x10_000, base + 0x20_000)
            .is_ok());
    }

    #[test]
    fn test_exempt_default_zero_redline() {
        // 红线：豁免默认 0——出厂引擎在态豁免恒 0 且 health 报真。
        let eng = WxPolicyEngine::new_seeded(42);
        assert_eq!(eng.exempt_active_count(), 0);
        let h = eng.health();
        assert!(h.exempt_baseline_zero);
        assert!(h.exempt_redline_ok);
        assert!(h.all_ok);
        assert!(h.matrix_ok);
        assert!(h.wx_leak_ok);
    }

    // ── 访问裁决与探针流 ─────────────────────────────────────────────────

    #[test]
    fn test_access_region_source() {
        let mut eng = WxPolicyEngine::new();
        let cid = eng
            .register_region(RegionType::KernelCode, 0x10_0000, 0x4_000, PermBits::RX)
            .unwrap_or(0);
        let did = eng
            .register_region(RegionType::KernelData, 0x14_0000, 0x4_000, PermBits::RW)
            .unwrap_or(0);
        assert!(cid > 0 && did > 0);
        // 代码区取指放行。
        let v1 = eng.check_access(0x10_0004, PermBits::RX);
        assert!(v1.allowed);
        assert!(matches!(v1.source, AccessSource::Region(id) if id == cid));
        // 代码区写拒（PermSuperset）。
        let v2 = eng.check_access(0x10_0004, PermBits::RW);
        assert!(!v2.allowed);
        assert_eq!(v2.code, WxMatrixError::PermSuperset.code());
        // 数据区写放行。
        let v3 = eng.check_access(0x14_0008, PermBits::RW);
        assert!(v3.allowed);
        // 越界一页 → 未映射。
        let v4 = eng.check_access(0x18_0000, PermBits::RO);
        assert!(!v4.allowed);
        assert_eq!(v4.code, WxMatrixError::RegionUnknown.code());
        // 非法需求位。
        let v5 = eng.check_access(0x10_0000, PermBits(0b1000));
        assert!(!v5.allowed);
        assert_eq!(v5.code, WxMatrixError::IllegalPermBits.code());
        // 探针流汇总。
        let probes: Vec<(u64, PermBits)> = vec![
            (0x10_0004, PermBits::RX),
            (0x10_0004, PermBits::RW),
            (0x14_0008, PermBits::RW),
            (0x18_0000, PermBits::RO),
            (0x10_0004, PermBits(PERM_W | PERM_X)),
        ];
        let sum = eng.probe_stream(&probes).unwrap_or(AccessSummary {
            probes: 0,
            allows: 0,
            denies: 0,
            unmapped: 0,
            wx_blocked: 0,
            first_fail_code: E_OK,
        });
        assert_eq!(sum.probes, 5);
        assert_eq!(sum.allows, 2);
        assert_eq!(sum.denies, 3);
        assert_eq!(sum.unmapped, 1);
        assert_eq!(sum.wx_blocked, 1);
        assert_eq!(sum.first_fail_code, WxMatrixError::PermSuperset.code());
        // 探针流超限拒整批。
        let big: Vec<(u64, PermBits)> = vec![(0u64, PermBits::RO); MAX_ACCESS_PROBES + 1];
        assert!(matches!(
            eng.probe_stream(&big),
            Err(WxMatrixError::LedgerFull)
        ));
    }

    // ── 封板状态机 ────────────────────────────────────────────────────────

    #[test]
    fn test_seal_state_machine() {
        let mut eng = WxPolicyEngine::new();
        let rid = eng
            .register_region(RegionType::KernelData, 0x10_0000, 0x1000, PermBits::RW)
            .unwrap_or(0);
        assert!(rid > 0);
        eng.seal();
        assert!(eng.is_sealed());
        // 登记类拒收。
        assert!(matches!(
            eng.register_region(RegionType::TaskHeap, 0x20_0000, 0x1000, PermBits::RW),
            Err(WxMatrixError::BadState)
        ));
        assert!(matches!(
            eng.grant_exemption(0x4_0000_0000, 0x50_0000_0000, 0x50_0001_0000),
            Err(WxMatrixError::BadState)
        ));
        assert!(matches!(
            eng.revoke_exemption(1),
            Err(WxMatrixError::BadState)
        ));
        // 裁决/变更/访问照常。
        assert!(eng.adjudicate(RegionType::KernelCode, PermBits::RX).is_ok());
        assert!(eng.mprotect(rid, PermBits::RO).is_ok());
        let v = eng.check_access(0x10_0000, PermBits::RO);
        assert!(v.allowed);
        // 重复封板幂等。
        eng.seal();
        assert!(eng.is_sealed());
    }

    // ── 注入面 ────────────────────────────────────────────────────────────

    #[test]
    fn test_inject_wx_violation() {
        let mut eng = WxPolicyEngine::new();
        let rep = eng.inject_wx_violation().unwrap_or(InjectionReport {
            kind: InjectionKind::WxViolation,
            attempts: 0,
            detected: 0,
            all_detected: false,
            first_code: E_OK,
        });
        assert!(rep.all_detected);
        assert_eq!(rep.attempts, WX_SWEEP_CASES);
        assert_eq!(rep.detected, rep.attempts);
        assert_eq!(rep.first_code, WxMatrixError::WxViolation.code());
        // 重复注入仍 100%（账面演化确定性）。
        let rep2 = eng.inject_wx_violation().unwrap_or(InjectionReport {
            kind: InjectionKind::WxViolation,
            attempts: 0,
            detected: 0,
            all_detected: false,
            first_code: E_OK,
        });
        assert!(rep2.all_detected);
    }

    #[test]
    fn test_inject_illegal_upgrade() {
        let mut eng = WxPolicyEngine::new();
        let rep = eng.inject_illegal_upgrade().unwrap_or(InjectionReport {
            kind: InjectionKind::IllegalUpgrade,
            attempts: 0,
            detected: 0,
            all_detected: false,
            first_code: E_OK,
        });
        assert!(rep.all_detected);
        assert_eq!(rep.attempts, 4);
        assert_eq!(rep.detected, 4);
        assert_eq!(rep.first_code, WxMatrixError::UpgradeDenied.code());
        // health 仍全绿（升权拒收是账面正常演化）。
        let h = eng.health();
        assert!(h.all_ok);
        assert!(h.wx_leak_ok);
    }

    #[test]
    fn test_inject_unregistered_exempt() {
        let mut eng = WxPolicyEngine::new();
        let rep = eng.inject_unregistered_exempt().unwrap_or(InjectionReport {
            kind: InjectionKind::UnregisteredExempt,
            attempts: 0,
            detected: 0,
            all_detected: false,
            first_code: E_OK,
        });
        assert!(rep.all_detected);
        assert_eq!(rep.attempts, 3);
        assert_eq!(rep.detected, 3);
        assert_eq!(eng.exempt_active_count(), 0, "无隐式豁免物化");
        // 在已有豁免的引擎上注入仍过（在态计数稳定口径）。
        let phys = 0x5_0000_0000u64;
        let va_w = 0x60_0000_0000u64;
        let va_x = 0x60_0001_0000u64;
        assert!(eng.grant_exemption(phys, va_w, va_x).is_ok());
        let rep2 = eng.inject_unregistered_exempt().unwrap_or(InjectionReport {
            kind: InjectionKind::UnregisteredExempt,
            attempts: 0,
            detected: 0,
            all_detected: false,
            first_code: E_OK,
        });
        assert!(rep2.all_detected);
        assert_eq!(eng.exempt_active_count(), 1);
    }

    // ── 双跑确定性与指纹 ─────────────────────────────────────────────────

    #[test]
    fn test_dual_run_determinism_ok_path() {
        let ops: Vec<PolicyOp> = vec![
            PolicyOp::Register {
                rtype: RegionType::KernelCode,
                base: 0x10_0000,
                len: 0x4_000,
                req: PermBits::RX,
            },
            PolicyOp::Register {
                rtype: RegionType::TaskStack,
                base: 0x20_0000,
                len: 0x8_000,
                req: PermBits::RW,
            },
            PolicyOp::Adjudicate {
                rtype: RegionType::DeviceMmio,
                req: PermBits::RW,
            },
            PolicyOp::Mprotect {
                region: 2,
                perms: PermBits::RO,
            },
            PolicyOp::GrantExempt {
                phys: 0x7_0000_0000,
                va_w: 0x80_0000_0000,
                va_x: 0x80_0001_0000,
            },
            PolicyOp::Access {
                va: 0x80_0000_1000 - 1,
                need: PermBits::RW,
            },
            PolicyOp::Access {
                va: 0x10_0008,
                need: PermBits::RX,
            },
            PolicyOp::RevokeExempt { exempt: 1 },
        ];
        let fp = WxPolicyEngine::dual_run_determinism(7, &ops);
        assert!(fp.is_ok());
        let f = fp.unwrap_or(0);
        assert!(f != 0);
        // 三跑再对拍（指纹恒定）。
        let fp3 = WxPolicyEngine::dual_run_determinism(7, &ops).unwrap_or(0);
        assert_eq!(f, fp3);
        // 脚本产物逐项验证。
        let eng = WxPolicyEngine::run_script(7, &ops).unwrap_or_else(|_| WxPolicyEngine::new());
        assert_eq!(eng.regions().len(), 2);
        assert_eq!(eng.exempt_active_count(), 0);
        assert_eq!(eng.exemptions().len(), 1);
        assert_eq!(eng.mprotects().len(), 1);
        assert!(eng.health().all_ok);
    }

    #[test]
    fn test_dual_run_determinism_fail_path() {
        // 含必败步的脚本：两跑同错码 → 折叠指纹相等（Ok 返回）。
        let ops: Vec<PolicyOp> = vec![
            PolicyOp::Register {
                rtype: RegionType::TaskHeap,
                base: 0x10_0000,
                len: 0x1000,
                req: PermBits::RW,
            },
            PolicyOp::Adjudicate {
                rtype: RegionType::TaskHeap,
                req: PermBits(PERM_W | PERM_X), // 红线必拒
            },
        ];
        let fp = WxPolicyEngine::dual_run_determinism(3, &ops);
        assert!(fp.is_ok());
        let f = fp.unwrap_or(0);
        assert!(f & 0x8000_0000_0000_0000 != 0, "失败路径折叠指纹带高位标");
        assert_eq!((f & 0xFFFF) as u16, WxMatrixError::WxViolation.code());
    }

    #[test]
    fn test_fingerprint_sensitivity() {
        // 同序同案 → 同指纹；异 seed / 异操作序 → 异指纹。
        let ops_a: Vec<PolicyOp> = vec![
            PolicyOp::Register {
                rtype: RegionType::KernelData,
                base: 0x10_0000,
                len: 0x1000,
                req: PermBits::RW,
            },
            PolicyOp::Adjudicate {
                rtype: RegionType::KernelCode,
                req: PermBits::RX,
            },
        ];
        let f1 = WxPolicyEngine::dual_run_determinism(1, &ops_a).unwrap_or(0);
        let f2 = WxPolicyEngine::dual_run_determinism(2, &ops_a).unwrap_or(0);
        assert!(f1 != f2, "boot seed 必须折入指纹");
        // 交换两步顺序 → 指纹变。
        let ops_b: Vec<PolicyOp> = vec![
            PolicyOp::Adjudicate {
                rtype: RegionType::KernelCode,
                req: PermBits::RX,
            },
            PolicyOp::Register {
                rtype: RegionType::KernelData,
                base: 0x10_0000,
                len: 0x1000,
                req: PermBits::RW,
            },
        ];
        let f3 = WxPolicyEngine::dual_run_determinism(1, &ops_b).unwrap_or(0);
        assert!(f1 != f3, "操作序必须折入指纹");
    }

    // ── PermBits / PTE / 显示 ────────────────────────────────────────────

    #[test]
    fn test_perm_bits_ops() {
        assert!(PermBits::RW.is_subset_of(PermBits::RW));
        assert!(PermBits::RO.is_subset_of(PermBits::RW));
        assert!(!PermBits::RW.is_subset_of(PermBits::RO));
        assert_eq!(PermBits::RW.union(PermBits::RX).0, 0b111);
        assert_eq!(PermBits(PERM_R | PERM_W | PERM_X).strip_exec().0, 0b011);
        assert_eq!(PermBits(0b111).strip_write().0, 0b101);
        assert!(PermBits(0b110).is_wx_violation());
        assert!(PermBits(0b111).is_wx_violation());
        assert!(!PermBits(0b011).is_wx_violation());
        assert!(!PermBits(0b101).is_wx_violation());
        assert_eq!(PermBits::RO.rwx_str(), "r--");
        assert_eq!(PermBits::RW.rwx_str(), "rw-");
        assert_eq!(PermBits::RX.rwx_str(), "r-x");
        assert_eq!(PermBits(0b110).rwx_str(), "rw x!");
    }

    #[test]
    fn test_pte_encoding() {
        // W+X 形状在 PTE 编码上不可达有效态（NX 与 RW 同置是硬件矛盾态，
        // 调用方必须先过红线；此处验证编码位的机械行为供审计）。
        let pte_rx = PermBits::RX.to_pte_bits(true);
        assert!(pte_rx & PTE_P != 0);
        assert!(pte_rx & PTE_RW == 0, "RX 无 W 位");
        assert!(pte_rx & PTE_NX == 0, "RX 不置 NX");
        assert!(pte_rx & PTE_US != 0, "user=1 置 US");
        let pte_rw_k = PermBits::RW.to_pte_bits(false);
        assert!(pte_rw_k & PTE_RW != 0);
        assert!(pte_rw_k & PTE_NX != 0, "数据面必置 NX");
        assert!(pte_rw_k & PTE_US == 0, "内核面清 US");
        // 矩阵行 PTE 编码闭环（user 位随类型）。
        let audit = audit_matrix();
        let mut i = 0usize;
        while i < 8 {
            let r = audit.rows[i];
            let us_set = r.pte & PTE_US != 0;
            assert_eq!(us_set, r.rtype.is_user(), "{} US 位", r.rtype.name());
            i += 1;
        }
    }

    // ── 错误码闭集 ────────────────────────────────────────────────────────

    #[test]
    fn test_error_code_closure() {
        // 15 个错误变体码恰填 0x1E51~0x1E5F，无重复。
        let codes = [
            WxMatrixError::IllegalPermBits.code(),
            WxMatrixError::WxViolation.code(),
            WxMatrixError::PermSuperset.code(),
            WxMatrixError::UnknownRegionType.code(),
            WxMatrixError::RegionShape.code(),
            WxMatrixError::RegionUnknown.code(),
            WxMatrixError::BadState.code(),
            WxMatrixError::LedgerFull.code(),
            WxMatrixError::ExemptFull.code(),
            WxMatrixError::ExemptUnknown.code(),
            WxMatrixError::ExemptIllegal.code(),
            WxMatrixError::ExemptState.code(),
            WxMatrixError::UpgradeDenied.code(),
            WxMatrixError::DeterminismDrift.code(),
            WxMatrixError::InternalOverflow.code(),
        ];
        assert_eq!(codes.len(), 15);
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u16, "码必须连续独占");
            // 中文诊断非空且含可操作动作提示。
            let d = detail_of(codes[i]);
            assert!(d.len() > 8, "诊断必须可读");
            i += 1;
        }
        assert_eq!(E_OK, SEG_BASE);
    }

    /// 码→诊断（闭集反查辅助）。
    fn detail_of(code: u16) -> &'static str {
        let all = [
            WxMatrixError::IllegalPermBits,
            WxMatrixError::WxViolation,
            WxMatrixError::PermSuperset,
            WxMatrixError::UnknownRegionType,
            WxMatrixError::RegionShape,
            WxMatrixError::RegionUnknown,
            WxMatrixError::BadState,
            WxMatrixError::LedgerFull,
            WxMatrixError::ExemptFull,
            WxMatrixError::ExemptUnknown,
            WxMatrixError::ExemptIllegal,
            WxMatrixError::ExemptState,
            WxMatrixError::UpgradeDenied,
            WxMatrixError::DeterminismDrift,
            WxMatrixError::InternalOverflow,
        ];
        let mut i = 0usize;
        while i < all.len() {
            if all[i].code() == code {
                return all[i].detail();
            }
            i += 1;
        }
        ""
    }

    #[test]
    fn test_display_impls() {
        let e = WxMatrixError::WxViolation;
        let s = format!("{}", e);
        assert!(s.contains("UNX-F0306"));
        assert!(s.contains("1E52"));
        let p = format!("{}", PermBits::RX);
        assert_eq!(p, "r-x");
    }

    // ── 综合健康与摘要 ────────────────────────────────────────────────────

    #[test]
    fn test_health_and_summary() {
        let mut eng = WxPolicyEngine::new_seeded(0xC0DE);
        // 全子系统各走一遍。
        assert!(eng
            .register_region(RegionType::KernelCode, 0x10_0000, 0x4_000, PermBits::RX)
            .is_ok());
        assert!(eng
            .register_region(RegionType::TaskHeap, 0x20_0000, 0x10_000, PermBits::RW)
            .is_ok());
        assert!(eng.adjudicate(RegionType::PageTableSelf, PermBits::RW).is_ok());
        let (a, d) = eng.wx_redline_sweep();
        assert_eq!((a, d), (WX_SWEEP_CASES, WX_SWEEP_CASES));
        assert!(eng.inject_illegal_upgrade().is_ok());
        assert!(eng.inject_unregistered_exempt().is_ok());
        let rid = eng.region_of_va(0x20_1000).map(|r| r.region_id).unwrap_or(0);
        assert!(rid > 0);
        assert!(eng.mprotect(rid, PermBits::RO).is_ok());
        let h = eng.health();
        assert!(h.all_ok);
        assert!(h.matrix_ok);
        assert!(h.wx_leak_ok);
        assert!(h.exempt_redline_ok);
        assert!(h.exempt_baseline_zero);
        assert!(h.stats.wx_redlines >= WX_SWEEP_CASES as u64);
        assert!(h.fingerprint != 0);
        // 摘要含关键口径。
        let s = eng.format_summary();
        assert!(s.contains("UNX-F0306"));
        assert!(s.contains("拒收率"));
        assert!(s.contains("指纹=0x"));
        // 引擎序号单调且折入指纹。
        assert!(h.seq > 0);
        let fp1 = eng.fingerprint();
        assert!(eng.adjudicate(RegionType::DeviceMmio, PermBits::RW).is_ok());
        let fp2 = eng.fingerprint();
        assert!(fp1 != fp2, "账面演化必须改变指纹");
    }

    #[test]
    fn test_next_free_base_and_region_of_va() {
        let mut eng = WxPolicyEngine::new();
        let b0 = eng.next_free_base();
        assert_eq!(b0, 0x10_0000, "空引擎空闲基址=1MB（低 1MB 预留）");
        assert!(eng
            .register_region(RegionType::KernelData, b0, 0x3_000, PermBits::RW)
            .is_ok());
        let b1 = eng.next_free_base();
        assert_eq!(b1, b0 + 0x3_000, "空闲基址随最高区端点推进");
        // 区内/边界/界外。
        assert!(eng.region_of_va(b0).is_some());
        assert!(eng.region_of_va(b0 + 0x2FFF).is_some());
        assert!(eng.region_of_va(b0 + 0x3_000).is_none(), "端点开区间");
    }

    #[test]
    fn test_decisions_ledger_drop_accounting() {
        // 裁决账满后：结论照常、账不留痕、丢包计数递增、指纹仍确定。
        let mut eng = WxPolicyEngine::new();
        // 先灌满裁决账（纯裁决，每案一条）。
        let mut i = 0u64;
        while i < MAX_DECISIONS as u64 {
            let req = if i % 2 == 0 {
                PermBits::RX
            } else {
                PermBits::RW
            };
            let t = if i % 2 == 0 {
                RegionType::KernelCode
            } else {
                RegionType::KernelData
            };
            let _ = eng.adjudicate(t, req);
            i += 1;
        }
        assert_eq!(eng.decisions().len(), MAX_DECISIONS);
        assert_eq!(eng.stats().decisions_dropped, 0);
        // 超限一案：仍出结论。
        let extra = eng.adjudicate(RegionType::TaskStack, PermBits::RW);
        assert!(extra.is_ok());
        assert_eq!(eng.decisions().len(), MAX_DECISIONS, "账不增");
        assert_eq!(eng.stats().decisions_dropped, 1);
        assert_eq!(eng.stats().decisions_total, MAX_DECISIONS as u64 + 1);
        // 双跑对拍仍确定（丢包计数折入指纹）。
        let ops: Vec<PolicyOp> = vec![];
        let fp = WxPolicyEngine::dual_run_determinism(9, &ops).unwrap_or(0);
        let eng2 = WxPolicyEngine::new_seeded(9);
        assert_eq!(fp, eng2.fingerprint());
    }
}
