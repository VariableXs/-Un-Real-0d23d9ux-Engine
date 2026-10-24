//! ═══════════════════════════════════════════════════════════════════════════
//! UNX-F0306 · 权限位策略引擎（W^X 矩阵）——第二件深化（wxmatrix2）
//! 诊断段：0x09F0 ~ 0x09FF（独占，16 码）
//!
//! 诊断段占用核查记录（grep 全仓结论，2026-10-24，按纪律先复核再落位）：
//!   - 预核段 0x1E50~0x1E5F：已被同任务先件 `unxstar/unx0306_wxmatrix.rs`
//!     （SEG_BASE=0x1E50，头注声明独占 0x1E50~0x1E5F）占用 → 弃用；
//!   - 备选段 0x1F10：落入 svstar2/ves495_s28gov.rs（SEG_BASE=0x1F00）与
//!     svstar2/ves496_s28process.rs（SEG_BASE=0x1F20）之间的连续占用带；
//!   - 备选段 0x1F50 / 0x1F70：均落入 unxstar/unx2782_ubuddymerge.rs 头注
//!     声明的独占段 0x1F40~0x1F7F（SEG_MASK=0x003F）→ 弃用；
//!   - 备选段 0x1FB0：落入 unxstar/unx2801_ubigpoolcap.rs 头注声明的
//!     独占段 0x1F80~0x1FBF → 弃用；
//!   - 近邻补查 0x1E7x：unxstar/unx2702_ulrufield.rs 声明独占
//!     0x1E40~0x1E7F → 弃用（unx0329_apgdt.rs 头注有同样核查在案）；
//!   - 终选段 0x09F0~0x09FF：全域 `grep -rn "0x09F" --include="*.rs"
//!     kernel/`（剔除 .mimosa 基线镜像）字面量零命中；下邻
//!     cgpu/cgb374_respool.rs 独占 0x09C0~0x09CF 与 cgpu/cgb376_feedback.rs
//!     独占 0x09E0~0x09EF 均止于 0x09EF，上邻 cgpu/cgb375_warmup.rs 独占
//!     0x0A00~0x0A0F 自 0x0A00 起——0x09F0~0x09FF 为两段之间唯一无主
//!     16 码洞 → 本模块取 0x09F0。
//!
//! 验收锚点（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md 第 1748 行"编号 | 功能条目 | 行数 | 状态 | 判据"表，
//! 原文全文抄录）：
//!   | UNX-F0306 | 权限位策略引擎（W^X 矩阵） | 300 | 已深化 | UNX-F0306-J1 区域类型→RWX 矩阵全过，W^X 违例（可写可执行）检出 100% |
//!
//! 判据 UNX-F0306-J1 分解（本件对先件 unx0306_wxmatrix.rs 的增量深化面）：
//!   (a) 区域类型→RWX 矩阵全过：8 类区域闭集 × [R,W,X] 三位冻结矩阵，
//!       逐行来源注释在案（ELF gABI 程序头 p_flags / Intel SDM Vol.3A
//!       §4.5 PTE 权限位 / 本仓 F0096 段权限、F0307 NX、F0308 自映射、
//!       F0309 PAT、F0318 trampoline 设计冻结值）；合法子集请求（含空集
//!       哨兵页、行全集）100% 过闸，超集 100% 拒收（PermSuperset）；
//!   (b) W^X 违例（可写∧可执行）检出 100% 红线（双保险）：
//!       其一，矩阵编译期逐行断言"任何类型矩阵行本身不含 W∧X"——
//!       政策源头永不给 W^X 留位；其二，运行期任何请求位组携带 W∧X
//!       （WX、RWX 两种组合 × 全部 8 类型 × 16 试次）一律红案
//!       （WxViolation），并计入 wx_redlines 红线账；
//!   (c) 增量子系统（本件独有，先件未覆盖的真实面）：
//!       - PTE 权限位编解码器：PermRwx ↔ x86-64 PTE（P/RW/NX）往返，
//!         坏 PTE（RW=1 且 NX=0 即 W^X 编码）在解码口 100% 检出；
//!       - 访问探针：按地址+访问类（读/写/取指）合成 #PF 错误码
//!         （SDM Vol.3 §6.15 位定义：P/W-R/U-S/RSVD/I-D），
//!         数据页取指→I/D 置位故障、只读页写→W/R 置位故障；
//!       - W^X 全区扫描器：对已登记区做存量扫描，配合 F0316 式
//!         权限污染钻孔钩子（corrupt_region_perms_for_drill），
//!         污染后的 W^X / 超集区 100% 被扫描立案；
//!       - 引擎快照与状态对拍（diff_snapshots）：双跑确定性在
//!         指纹级与快照级双口径核对；
//!       - 批量变更事务（mprotect 批）：一揽子降权原子应用，
//!         任一笔失败整批回滚（batch_begin/batch_apply）；
//!       - 封账语义（seal）：封账后一切变更红案，只读探账放行；
//!       - 环形账本驱逐计数：裁决/事件账满环后旧迹驱逐留数；
//!       - 审计报告生成器（audit_report）：矩阵/账本/比率/豁免/
//!         健康度一段式可读转储。
//!
//! 纪律：no_std + alloc；本文件只 `use core::…` / `use alloc::…`，零
//!   `use crate::…`（完全自包含）；内核路径零 unwrap/expect/panic!/
//!   运行期 assert!（编译期 const 断言不受限；#[cfg(test)] 测试体不属
//!   内核路径）；可失败全走 Result<T, WxPolicyError>，错误枚举每变体
//!   带 detail(&self) -> &'static str 中文可操作诊断；计数一律
//!   saturating/wrapping 显性；>64KiB 结构禁上栈（本模块最大单块为
//!   堆置 Vec 账本，栈上只留句柄）；const fn 内不使用 slice::get /
//!   Vec 索引 / Ord::min/max / derive-PartialEq 的 == / for 循环
//!   （一律 while+索引或拆参比较），含 Vec 字段的结构不 derive Copy。
//! ═══════════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════════
// §1 · 冻结常量段（每常量注明来源）
// ═══════════════════════════════════════════════════════════════════════════

/// 4K 标准页 = 2^12 字节（x86-64 PT 叶项粒度，Intel SDM Vol.3A §4.2 硬件冻结值；
/// 与本仓 F0305 PAGE_4K、先件 unx0306_wxmatrix.rs PAGE_4K 同值互证）。
pub const PAGE_4K: u64 = 0x1000;

/// 权限位 R（可读）掩码位（本单政策位序：bit0=R，bit1=W，bit2=X）。
pub const PERM_R: u8 = 1 << 0;
/// 权限位 W（可写）掩码位。
pub const PERM_W: u8 = 1 << 1;
/// 权限位 X（可执行/可取指）掩码位。判据原文"可写可执行"中的执行位
/// 即本位；任务口径中的"A"（执行别名记法）与本位同义，本单一律记 X。
pub const PERM_X: u8 = 1 << 2;
/// 合法权限位全集掩码（R|W|X，三位之外皆为非法位）。
pub const PERM_MASK: u8 = PERM_R | PERM_W | PERM_X;

/// x86-64 PTE 存在位 P（Intel SDM Vol.3A §4.5 图 4-11，bit0）。
pub const PTE_P: u64 = 1 << 0;
/// x86-64 PTE 读写位 RW（SDM §4.5，bit1：1=可写，0=只读）。
pub const PTE_RW: u64 = 1 << 1;
/// x86-64 PTE 用户/监督位 US（SDM §4.5，bit2：本单为监督态策略引擎，
/// 编解码时该位不参与政策位，恒监督语义）。
pub const PTE_US: u64 = 1 << 2;
/// x86-64 PTE 执行禁用位 NX（SDM §4.6：EFER.NXE=1 时 bit63=1 禁取指；
/// 与本仓 F0307 EFER.NXE 使能序对接——NXE 开之前 NX 位被硬件忽略，
/// 本单编解码按 NXE=1 终态口径）。
pub const PTE_NX: u64 = 1 << 63;

/// #PF 错误码位 P（SDM Vol.3 §6.15：1=由页级保护 violation 引发，
/// 0=页不存在引发）。
pub const PF_P: u32 = 1 << 0;
/// #PF 错误码位 W/R（§6.15：1=写访问引发，0=读访问引发）。
pub const PF_WR: u32 = 1 << 1;
/// #PF 错误码位 U/S（§6.15：1=用户态引发；本单恒监督态，恒 0）。
pub const PF_US: u32 = 1 << 2;
/// #PF 错误码位 RSVD（§6.15：1=页表保留位写 1 引发）。
pub const PF_RSVD: u32 = 1 << 3;
/// #PF 错误码位 I/D（§6.15：1=取指引发——数据页执行故障的硬件指印，
/// 判据 F0307-J1"数据页执行 #PF"同源）。
pub const PF_ID: u32 = 1 << 4;

/// FNV-1a 64 位偏移基（FNV 参考实现 frostbytes.com/archive/reference/fnv/
/// 与 draft-eastlake-fnv 冻结值 0xcbf29ce484222325）。
pub const FNV_OFFSET_64: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（同上参考冻结值 0x100000001b3）。
pub const FNV_PRIME_64: u64 = 0x0000_0100_0000_01b3;

/// 确定性 LCG 乘子（Knuth MMIX 线性同余冻结值 6364136223846793005，
/// TAOCP Vol.2 §3.3.4；仅用于确定性剧本回放，非密码源）。
pub const LCG_MULT: u64 = 6364136223846793005;
/// 确定性 LCG 增量（Knuth MMIX 冻结值 1442695040888963407）。
pub const LCG_INC: u64 = 1442695040888963407;

/// 区域登记上限：区记录约 48B/条 × 256 ≈ 12KiB 堆账（防区域表膨胀；
/// 与先件 MAX_REGIONS 同值互证）。
pub const MAX_REGIONS: usize = 256;
/// 裁决账环形容量：约 24B/条 × 1024 ≈ 24KiB 堆账（满环驱逐最旧，
/// 驱逐数单独计数，不丢比率统计）。
pub const MAX_DECISIONS: usize = 1024;
/// 变更账环形容量：约 24B/条 × 512 ≈ 12KiB 堆账。
pub const MAX_MPROTECTS: usize = 512;
/// 豁免登记上限：约 32B/条 × 64 ≈ 2KiB 堆账（豁免默认 0 红线，
/// 上限内也要逐条显式登记/撤销）。
pub const MAX_EXEMPTS: usize = 64;
/// 事件账环形容量：约 40B/条 × 2048 ≈ 80KiB 堆账（满环驱逐最旧）。
pub const MAX_EVENTS: usize = 2048;

/// 任务编号（UNX 内存管理域 F03 系）。
pub const MODULE_ID: u16 = 0x0306;
/// 任务标签。
pub const TASK_TAG: &str = "UNX-F0306";

// ═══════════════════════════════════════════════════════════════════════════
// §2 · 诊断码段（独占 0x09F0~0x09FF；0x09F0 为 OK 码，15 个错误变体
//       一码一位恰占满 0x09F1~0x09FF，段满零余量）
// ═══════════════════════════════════════════════════════════════════════════

/// 本模块诊断段基址（0x09F0，独占段 0x09F0~0x09FF；选段过程见头注）。
pub const SEG_BASE: u16 = 0x09F0;
/// 裁决通过码（段内 0 号；账本中 granted=true 行的 code 字段）。
pub const E_OK: u16 = SEG_BASE;

/// UNX-F0306 第二件诊断码闭集（15 个错误变体恰好填满 0x09F1~0x09FF）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WxPolicyError {
    /// 0x09F1 · 请求位含 R/W/X 之外位，或 PTE 解码口径非法（无 P 位）。
    IllegalPermBits,
    /// 0x09F2 · W^X 红线：可写∧可执行同时置位（判据 J1(b) 红案）。
    WxViolation,
    /// 0x09F3 · 请求为类型矩阵行的超集（含存量扫描发现的超集污染）。
    PermSuperset,
    /// 0x09F4 · 区域类型索引越界（u8→闭集换算失败）。
    UnknownRegionType,
    /// 0x09F5 · 区形错误：零长/未 4K 对齐/基址+长度 u64 溢出/物理页号未对齐。
    RegionShape,
    /// 0x09F6 · 与已登记活跃区地址重叠。
    RegionOverlap,
    /// 0x09F7 · 句柄或地址查无此区（探针口径以 fault 结果呈现此码）。
    RegionUnknown,
    /// 0x09F8 · 引擎状态非法：已封账后变更、对已退役区变更等。
    BadState,
    /// 0x09F9 · 区域登记表容量满（MAX_REGIONS）。
    LedgerFull,
    /// 0x09F1A · 变更路径升权（新增任何权限位）被拒——降权自由、升权拒。
    MprotectUpgrade,
    /// 0x09F1B · 豁免登记容量满（MAX_EXEMPTS）。
    ExemptFull,
    /// 0x09F1C · 同物理页豁免重复登记。
    ExemptDuplicate,
    /// 0x09F1D · 未登记豁免的双映射检出（W 别名与 X 别名同页且无豁免在案）。
    ExemptUnregistered,
    /// 0x09F1E · 豁免别名形状脏：W 别名不含 W 或含 X、X 别名不含 X 或含 W。
    ExemptAliasDirty,
    /// 0x09F1F · 撤销的豁免不存在（已撤销或从未登记）。
    ExemptUnknown,
}

impl WxPolicyError {
    /// 错误码（段内一码一位；E_OK=0x09F0 之外的 0x09F1..=0x09FF）。
    #[must_use]
    pub const fn code(&self) -> u16 {
        match *self {
            WxPolicyError::IllegalPermBits => SEG_BASE + 0x01, // 0x09F1
            WxPolicyError::WxViolation => SEG_BASE + 0x02, // 0x09F2
            WxPolicyError::PermSuperset => SEG_BASE + 0x03, // 0x09F3
            WxPolicyError::UnknownRegionType => SEG_BASE + 0x04, // 0x09F4
            WxPolicyError::RegionShape => SEG_BASE + 0x05, // 0x09F5
            WxPolicyError::RegionOverlap => SEG_BASE + 0x06, // 0x09F6
            WxPolicyError::RegionUnknown => SEG_BASE + 0x07, // 0x09F7
            WxPolicyError::BadState => SEG_BASE + 0x08, // 0x09F8
            WxPolicyError::LedgerFull => SEG_BASE + 0x09, // 0x09F9
            WxPolicyError::MprotectUpgrade => SEG_BASE + 0x0A, // 0x09F1A
            WxPolicyError::ExemptFull => SEG_BASE + 0x0B, // 0x09F1B
            WxPolicyError::ExemptDuplicate => SEG_BASE + 0x0C, // 0x09F1C
            WxPolicyError::ExemptUnregistered => SEG_BASE + 0x0D, // 0x09F1D
            WxPolicyError::ExemptAliasDirty => SEG_BASE + 0x0E, // 0x09F1E
            WxPolicyError::ExemptUnknown => SEG_BASE + 0x0F, // 0x09F1F
        }
    }

    /// 错误变体计数（编译期自证 15 变体恰满段）。
    pub const VARIANT_COUNT: usize = 15;

    /// 中文可操作诊断（每变体一句"定位+怎么办"）。
    #[must_use]
    pub const fn detail(&self) -> &'static str {
        match *self {
            WxPolicyError::IllegalPermBits => {
                "请求权限位超出 R/W/X 三位掩码：检查调用方位组装，只允许 \
                 R=bit0/W=bit1/X=bit2；PTE 解码口径要求 P 位置 1。"
            }
            WxPolicyError::WxViolation => {
                "W^X 红线红案：请求同时携带可写与可执行（可写可执行）。\
                 系统不给任何区域类型发放 W^X 组合；写后执行需求请走 \
                 DynCodeFinal 终态 + 显式双映射豁免通道（一 W 一 X 两个\
                 别名分别登记）。"
            }
            WxPolicyError::PermSuperset => {
                "请求权限为该区域类型矩阵行的超集：对照 RWX_MATRIX 行裁剪\
                 请求位；确需更高权限时改用正确类型登记，或走显式豁免。"
            }
            WxPolicyError::UnknownRegionType => {
                "区域类型索引越界：RegionType 闭集共 8 类（索引 0..=7），\
                 检查上游类型编码表与 RegionType::COUNT 是否脱节。"
            }
            WxPolicyError::RegionShape => {
                "区域形状非法：要求 len>0、base 与 len 均 4K 对齐、\
                 base+len 不溢出 u64；MMIO/页表区另需物理页号 4K 对齐。\
                 修整请求参数后重试。"
            }
            WxPolicyError::RegionOverlap => {
                "与已登记活跃区地址重叠：先退役旧区或另择基址；重叠区\
                 需合并权限时须以旧区权限子集登记。"
            }
            WxPolicyError::RegionUnknown => {
                "查无此区：区域句柄不存在或已退役，或探针地址不在任何\
                 活跃区跨度内；核对句柄/地址与登记账。"
            }
            WxPolicyError::BadState => {
                "引擎状态非法：封账（seal）后拒绝一切变更，只读探账/\
                 探针放行；已退役区不可再变更或探账。解封无路径，\
                 需新引擎重建。"
            }
            WxPolicyError::LedgerFull => {
                "区域登记表满（MAX_REGIONS=256）：先退役不活跃区释放表位，\
                 或审计后另建引擎分片。"
            }
            WxPolicyError::MprotectUpgrade => {
                "变更路径升权被拒：mprotect 语义为只降不升（新增任何权限\
                 位即拒）。确需恢复高权限时退役旧区并按矩阵重新登记，\
                 动态代码走豁免双映射。"
            }
            WxPolicyError::ExemptFull => {
                "豁免登记表满（MAX_EXEMPTS=64）：先撤销不再使用的双映射\
                 豁免再登记新豁免。"
            }
            WxPolicyError::ExemptDuplicate => {
                "同物理页豁免重复登记：一页至多一条活跃豁免；如需更换\
                 别名形状，先撤销旧豁免再登记。"
            }
            WxPolicyError::ExemptUnregistered => {
                "未登记豁免的双映射被检出：同物理页 W 别名与 X 别名并存\
                 时必须先 register_exempt 显式登记并留 owner 溯源，\
                 否则按 W^X 攻击面红案。"
            }
            WxPolicyError::ExemptAliasDirty => {
                "豁免别名形状脏：W 别名必须含 W 且不含 X（W 或 RW），\
                 X 别名必须含 X 且不含 W（X 或 RX）；单别名内 W^X 由\
                 WxViolation 红案先行拦截。"
            }
            WxPolicyError::ExemptUnknown => {
                "撤销的豁免不存在：该物理页无活跃豁免（已撤销或从未\
                 登记）；核对豁免账后再撤销。"
            }
        }
    }
}

impl core::fmt::Display for WxPolicyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{:#06x}] {}", self.code(), self.detail())
    }
}

/// 段基 16 对齐自证（编译期：0x09F0 & 0xF == 0，段界不跨界）。
const _: () = assert!(SEG_BASE & 0x000F == 0, "UNX-F0306: SEG_BASE 须 16 码对齐");
/// 15 变体 × 1 码 + 1 OK 码 == 16 码恰满段（编译期）。
const _: () = assert!(
    WxPolicyError::VARIANT_COUNT == 15,
    "UNX-F0306: 错误变体数须恰为 15（段满零余量口径）"
);

// ═══════════════════════════════════════════════════════════════════════════
// §3 · PermRwx 权限位组（u8 新语；const fn 全员位运算，无禁用面）
// ═══════════════════════════════════════════════════════════════════════════

/// R/W/X 权限位组（三位打包于 u8；无 Vec 字段，可安全 derive Copy）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermRwx(pub u8);

impl PermRwx {
    /// 空权限（哨兵页/守护页口径：三位皆 0，合法子集，恒过闸）。
    pub const NONE: PermRwx = PermRwx(0);
    /// 只读 R。
    pub const R: PermRwx = PermRwx(PERM_R);
    /// 只写 W（无读写权限组合中，W 用于豁免 W 别名）。
    pub const W: PermRwx = PermRwx(PERM_W);
    /// 读写 RW。
    pub const RW: PermRwx = PermRwx(PERM_R | PERM_W);
    /// 读执行 RX（代码段标准权）。
    pub const RX: PermRwx = PermRwx(PERM_R | PERM_X);
    /// 只执行 X（豁免 X 别名最小权）。
    pub const X: PermRwx = PermRwx(PERM_X);
    /// 写执行 WX——**W^X 违例组合**（矩阵任何行不含、任何请求红案）。
    pub const WX: PermRwx = PermRwx(PERM_W | PERM_X);
    /// 全开 RWX——**W^X 违例组合**（同上红案）。
    pub const RWX: PermRwx = PermRwx(PERM_R | PERM_W | PERM_X);

    /// 原始位组。
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// 位组合法性（三位掩码内）。
    #[must_use]
    pub const fn is_legal(self) -> bool {
        self.0 & !PERM_MASK == 0
    }

    /// 含 R？
    #[must_use]
    pub const fn is_r(self) -> bool {
        self.0 & PERM_R != 0
    }

    /// 含 W？
    #[must_use]
    pub const fn is_w(self) -> bool {
        self.0 & PERM_W != 0
    }

    /// 含 X？
    #[must_use]
    pub const fn is_x(self) -> bool {
        self.0 & PERM_X != 0
    }

    /// **W^X 红线判式**：可写∧可执行同时置位（判据原文"可写可执行"）。
    #[must_use]
    pub const fn is_wx_violation(self) -> bool {
        self.0 & (PERM_W | PERM_X) == (PERM_W | PERM_X)
    }

    /// 并集。
    #[must_use]
    pub const fn union(self, other: PermRwx) -> PermRwx {
        PermRwx(self.0 | other.0)
    }

    /// 交集。
    #[must_use]
    pub const fn intersect(self, other: PermRwx) -> PermRwx {
        PermRwx(self.0 & other.0)
    }

    /// 子集判式：self ⊆ other。
    #[must_use]
    pub const fn is_subset_of(self, other: PermRwx) -> bool {
        self.0 & !other.0 == 0
    }

    /// self 是否含 other 没有的位（升权判式：新增位检测）。
    #[must_use]
    pub const fn has_extra_bits_of(self, other: PermRwx) -> bool {
        self.0 & !other.0 != 0
    }

    /// 去掉 other 所含位（降权计算：old 剥掉收缴位）。
    #[must_use]
    pub const fn minus(self, other: PermRwx) -> PermRwx {
        PermRwx(self.0 & !other.0)
    }

    /// 访问许可判式（探针用）：op 位在位组内。
    #[must_use]
    pub const fn permits(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    /// 三字母速记（"RW-"/"R-X"…；非法位被掩掉后 8 组合逐一对应；
    /// 复合值以命名常量做模式——or-模式是"二选一"不是位或）。
    #[must_use]
    pub const fn mnem(self) -> &'static str {
        // 复合位值命名常量（模式位匹配用）。
        const M_RW: u8 = PERM_R | PERM_W;
        const M_RX: u8 = PERM_R | PERM_X;
        const M_WX: u8 = PERM_W | PERM_X;
        match self.0 & PERM_MASK {
            0 => "---",
            PERM_R => "R--",
            PERM_W => "-W-",
            M_RW => "RW-",
            PERM_X => "--X",
            M_RX => "R-X",
            M_WX => "-WX",
            _ => "RWX",
        }
    }

    /// 编码为 x86-64 PTE 权限位（P 恒置；W→RW 位；无 X→NX 位；
    /// US 位不参与，监督态口径）。const fn 位运算，无禁用面。
    #[must_use]
    pub const fn to_pte_flags(self) -> u64 {
        let mut v: u64 = PTE_P;
        if self.0 & PERM_W != 0 {
            v |= PTE_RW;
        }
        if self.0 & PERM_X == 0 {
            v |= PTE_NX;
        }
        v
    }

    /// 从 x86-64 PTE 权限位解码（NXE=1 终态口径）。
    /// 坏口径三案全检出：无 P 位→IllegalPermBits；W∧可执行
    /// （RW=1 且 NX=0）→WxViolation 红案；本方法不产生其他错误。
    #[must_use]
    pub fn from_pte_flags(flags: u64) -> Result<PermRwx, WxPolicyError> {
        if flags & PTE_P == 0 {
            return Err(WxPolicyError::IllegalPermBits);
        }
        let w = flags & PTE_RW != 0;
        let x = flags & PTE_NX == 0;
        if w && x {
            return Err(WxPolicyError::WxViolation);
        }
        let mut b: u8 = 0;
        if w {
            b |= PERM_W;
        }
        if x {
            b |= PERM_X;
        }
        // PTE 口径无"显式 R=0"：硬件读权由 P 位隐含，R 位恒折算置 1
        // （SDM §4.5：P=1 即可读——只读页 RW=0）。纯 W/X 别名属政策层
        // 豁免形状，不经 PTE 解码口产生。
        b |= PERM_R;
        Ok(PermRwx(b))
    }

    /// 全部 8 种位组合（测试与矩阵对拍枚举用；含 2 个违例组合）。
    #[must_use]
    pub const fn all_combos() -> [PermRwx; 8] {
        [
            PermRwx::NONE,
            PermRwx::R,
            PermRwx::W,
            PermRwx::RW,
            PermRwx::X,
            PermRwx::RX,
            PermRwx::WX,
            PermRwx::RWX,
        ]
    }
}

impl core::fmt::Display for PermRwx {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.mnem())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4 · RegionType 区域类型闭集 + 8×3 RWX 冻结矩阵（逐行来源注释）
// ═══════════════════════════════════════════════════════════════════════════

/// 区域类型闭集（8 类；判据 J1(a)"区域类型→RWX 矩阵"的行轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionType {
    /// [0] 内核代码段 .text。
    KernelCode,
    /// [1] 内核只读数据 .rodata。
    KernelRoData,
    /// [2] 内核可写数据 .data/.bss。
    KernelData,
    /// [3] 任务栈（每任务栈区）。
    TaskStack,
    /// [4] 任务堆。
    TaskHeap,
    /// [5] 设备 MMIO 寄存器窗口。
    DeviceMmio,
    /// [6] 页表自身（自映射窗口可写的页表页）。
    PageTableSelf,
    /// [7] 动态代码终态（模块/trampoline 落定后的执行面）。
    DynCodeFinal,
}

impl RegionType {
    /// 闭集计数（矩阵行数；编译期断言与 RWX_MATRIX 行数互锁）。
    pub const COUNT: usize = 8;

    /// 行索引（0..=7）。
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            RegionType::KernelCode => 0,
            RegionType::KernelRoData => 1,
            RegionType::KernelData => 2,
            RegionType::TaskStack => 3,
            RegionType::TaskHeap => 4,
            RegionType::DeviceMmio => 5,
            RegionType::PageTableSelf => 6,
            RegionType::DynCodeFinal => 7,
        }
    }

    /// 索引→类型（越界 None；调用方显式处理，闭集不 panic）。
    #[must_use]
    pub const fn from_index(idx: u8) -> Option<RegionType> {
        match idx {
            0 => Some(RegionType::KernelCode),
            1 => Some(RegionType::KernelRoData),
            2 => Some(RegionType::KernelData),
            3 => Some(RegionType::TaskStack),
            4 => Some(RegionType::TaskHeap),
            5 => Some(RegionType::DeviceMmio),
            6 => Some(RegionType::PageTableSelf),
            7 => Some(RegionType::DynCodeFinal),
            _ => None,
        }
    }

    /// 索引→类型（失败走 Result，带诊断码 UnknownRegionType）。
    #[must_use]
    pub fn try_from_index(idx: u8) -> Result<RegionType, WxPolicyError> {
        match RegionType::from_index(idx) {
            Some(t) => Ok(t),
            None => Err(WxPolicyError::UnknownRegionType),
        }
    }

    /// 全类型清单（对拍/演练枚举用）。
    #[must_use]
    pub const fn all() -> [RegionType; RegionType::COUNT] {
        [
            RegionType::KernelCode,
            RegionType::KernelRoData,
            RegionType::KernelData,
            RegionType::TaskStack,
            RegionType::TaskHeap,
            RegionType::DeviceMmio,
            RegionType::PageTableSelf,
            RegionType::DynCodeFinal,
        ]
    }

    /// 中文类型名（审计报告用）。
    #[must_use]
    pub const fn zh(self) -> &'static str {
        match self {
            RegionType::KernelCode => "内核代码段",
            RegionType::KernelRoData => "内核只读数据",
            RegionType::KernelData => "内核可写数据",
            RegionType::TaskStack => "任务栈",
            RegionType::TaskHeap => "任务堆",
            RegionType::DeviceMmio => "设备MMIO",
            RegionType::PageTableSelf => "页表自身",
            RegionType::DynCodeFinal => "动态代码终态",
        }
    }

    /// 该类型的矩阵行（与 RWX_MATRIX 双源互证；const fn 纯 match，
    /// 不索引数组，规避 const fn 禁索引纪律）。
    #[must_use]
    pub const fn row_mask(self) -> PermRwx {
        match self {
            RegionType::KernelCode => PB_KERNEL_CODE,
            RegionType::KernelRoData => PB_KERNEL_RODATA,
            RegionType::KernelData => PB_KERNEL_DATA,
            RegionType::TaskStack => PB_TASK_STACK,
            RegionType::TaskHeap => PB_TASK_HEAP,
            RegionType::DeviceMmio => PB_DEVICE_MMIO,
            RegionType::PageTableSelf => PB_PAGE_TABLE_SELF,
            RegionType::DynCodeFinal => PB_DYN_CODE_FINAL,
        }
    }
}

impl core::fmt::Display for RegionType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.zh())
    }
}

// ── 8×3 RWX 冻结矩阵（行=类型索引，列=[R,W,X]，1=允许）───────────────────
// 逐行来源（判据 J1(a) 的"每行来源注释"，全行抄录）：
//  [0] KernelCode    [1,0,1] ELF PT_LOAD .text p_flags=PF_R|PF_X（ELF gABI
//                     程序头章）；本仓 F0096 段权限冻结、F0098 KASLR 只移
//                     基址不改权限、F0303 高半区落位同权；
//  [1] KernelRoData  [1,0,0] .rodata p_flags=PF_R；F0084 重定位修补完成后
//                     应写保护（修补期 RW 是临时窗口，终态行恒只读）；
//  [2] KernelData    [1,1,0] .data/.bss p_flags=PF_R|PF_W；数据面 NX 由
//                     F0307 EFER.NXE 使能后硬件保证；
//  [3] TaskStack     [1,1,0] 栈 PF_R|PF_W + 硬性 NX——栈不可执行是 W^X
//                     政策的组成部分（x86-64 栈段无 PF_X；SDM §6.15
//                     栈执行故障以 I/D 位报出）；
//  [4] TaskHeap      [1,1,0] 堆 PF_R|PF_W + NX（防堆喷跳转执行；JIT 堆
//                     执行需求一律改走 [7] DynCodeFinal+豁免双映射）；
//  [5] DeviceMmio    [1,1,0] MMIO 寄存器窗口 R+W 永不可执行；缓存属性
//                     UC/WC 属 F0309 PAT 槽位非权限位；粒度恒 4K
//                     （F0305 DeviceMmio 仅 4K 判据）；
//  [6] PageTableSelf [1,1,0] F0308 自映射窗口读写页表项；页表数据永不
//                     可执行——页表执行=自指攻击面（W^X 第 3 类红线）；
//  [7] DynCodeFinal  [1,0,1] 模块/trampoline/低 1MB 恒等映射（F0318）
//                     落定后与 .text 同权（R+X）；写入期经独立 W 别名
//                     （KernelData 面 RW-NX）+ 显式双映射豁免，终态行
//                     本身不含 W。
/// 8×3 RWX 冻结矩阵（审计数据源；与 RegionType::row_mask 双源互证，
/// verify_matrix_dual_source() 逐行对拍）。
pub const RWX_MATRIX: [[u8; 3]; RegionType::COUNT] = [
    [1, 0, 1], // [0] KernelCode    r-x（来源见上）
    [1, 0, 0], // [1] KernelRoData  r--
    [1, 1, 0], // [2] KernelData    rw-
    [1, 1, 0], // [3] TaskStack     rw-
    [1, 1, 0], // [4] TaskHeap      rw-
    [1, 1, 0], // [5] DeviceMmio    rw-
    [1, 1, 0], // [6] PageTableSelf rw-
    [1, 0, 1], // [7] DynCodeFinal  r-x
];

/// KernelCode 行常量（RWX_MATRIX 行折算；const 初始化器中的数组字面量
/// 索引属编译期求值，不受 const fn 禁索引纪律约束）。
pub const PB_KERNEL_CODE: PermRwx =
    PermRwx(RWX_MATRIX[0][0] | (RWX_MATRIX[0][1] << 1) | (RWX_MATRIX[0][2] << 2));
/// KernelRoData 行常量。
pub const PB_KERNEL_RODATA: PermRwx =
    PermRwx(RWX_MATRIX[1][0] | (RWX_MATRIX[1][1] << 1) | (RWX_MATRIX[1][2] << 2));
/// KernelData 行常量。
pub const PB_KERNEL_DATA: PermRwx =
    PermRwx(RWX_MATRIX[2][0] | (RWX_MATRIX[2][1] << 1) | (RWX_MATRIX[2][2] << 2));
/// TaskStack 行常量。
pub const PB_TASK_STACK: PermRwx =
    PermRwx(RWX_MATRIX[3][0] | (RWX_MATRIX[3][1] << 1) | (RWX_MATRIX[3][2] << 2));
/// TaskHeap 行常量。
pub const PB_TASK_HEAP: PermRwx =
    PermRwx(RWX_MATRIX[4][0] | (RWX_MATRIX[4][1] << 1) | (RWX_MATRIX[4][2] << 2));
/// DeviceMmio 行常量。
pub const PB_DEVICE_MMIO: PermRwx =
    PermRwx(RWX_MATRIX[5][0] | (RWX_MATRIX[5][1] << 1) | (RWX_MATRIX[5][2] << 2));
/// PageTableSelf 行常量。
pub const PB_PAGE_TABLE_SELF: PermRwx =
    PermRwx(RWX_MATRIX[6][0] | (RWX_MATRIX[6][1] << 1) | (RWX_MATRIX[6][2] << 2));
/// DynCodeFinal 行常量。
pub const PB_DYN_CODE_FINAL: PermRwx =
    PermRwx(RWX_MATRIX[7][0] | (RWX_MATRIX[7][1] << 1) | (RWX_MATRIX[7][2] << 2));

// ── 编译期逐行断言（判据 J1(b) 第一保险："任何类型矩阵行本身不含
//    可写∧可执行"——矩阵源头零 W^X 行；比较原始 u8 字段，u8 的 ==
//    是内建 const 运算，不经 derive-PartialEq 路径）──────────────────────
const _: () = assert!(
    !(RWX_MATRIX[0][1] == 1 && RWX_MATRIX[0][2] == 1),
    "UNX-F0306: KernelCode 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[1][1] == 1 && RWX_MATRIX[1][2] == 1),
    "UNX-F0306: KernelRoData 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[2][1] == 1 && RWX_MATRIX[2][2] == 1),
    "UNX-F0306: KernelData 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[3][1] == 1 && RWX_MATRIX[3][2] == 1),
    "UNX-F0306: TaskStack 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[4][1] == 1 && RWX_MATRIX[4][2] == 1),
    "UNX-F0306: TaskHeap 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[5][1] == 1 && RWX_MATRIX[5][2] == 1),
    "UNX-F0306: DeviceMmio 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[6][1] == 1 && RWX_MATRIX[6][2] == 1),
    "UNX-F0306: PageTableSelf 行含 W^X，矩阵红线破裂"
);
const _: () = assert!(
    !(RWX_MATRIX[7][1] == 1 && RWX_MATRIX[7][2] == 1),
    "UNX-F0306: DynCodeFinal 行含 W^X，矩阵红线破裂"
);

// ── 矩阵形状自锁（行数/列语义/行权合计——多一重防手改漂移）────────────────
/// 八行 R 位合计（const 折算）：全行可读（每类区域至少可读）。
pub const MATRIX_R_SUM: u8 = RWX_MATRIX[0][0]
    + RWX_MATRIX[1][0]
    + RWX_MATRIX[2][0]
    + RWX_MATRIX[3][0]
    + RWX_MATRIX[4][0]
    + RWX_MATRIX[5][0]
    + RWX_MATRIX[6][0]
    + RWX_MATRIX[7][0];
/// 八行 W 位合计：恰 5 行可写（KernelData/TaskStack/TaskHeap/
/// DeviceMmio/PageTableSelf）。
pub const MATRIX_W_SUM: u8 = RWX_MATRIX[0][1]
    + RWX_MATRIX[1][1]
    + RWX_MATRIX[2][1]
    + RWX_MATRIX[3][1]
    + RWX_MATRIX[4][1]
    + RWX_MATRIX[5][1]
    + RWX_MATRIX[6][1]
    + RWX_MATRIX[7][1];
/// 八行 X 位合计：恰 2 行可执行（KernelCode/DynCodeFinal）。
pub const MATRIX_X_SUM: u8 = RWX_MATRIX[0][2]
    + RWX_MATRIX[1][2]
    + RWX_MATRIX[2][2]
    + RWX_MATRIX[3][2]
    + RWX_MATRIX[4][2]
    + RWX_MATRIX[5][2]
    + RWX_MATRIX[6][2]
    + RWX_MATRIX[7][2];

const _: () = assert!(
    MATRIX_R_SUM == 8,
    "UNX-F0306: 矩阵 R 位合计须为 8（全行可读）"
);
const _: () = assert!(
    MATRIX_W_SUM == 5,
    "UNX-F0306: 矩阵 W 位合计须为 5（恰 5 行可写）"
);
const _: () = assert!(
    MATRIX_X_SUM == 2,
    "UNX-F0306: 矩阵 X 位合计须为 2（恰 2 行可执行）"
);
const _: () = assert!(
    RegionType::COUNT == RWX_MATRIX.len(),
    "UNX-F0306: 类型闭集数与矩阵行数须互锁为 8"
);

/// 双源对拍（纯函数，无引擎依赖）：RegionType::row_mask 逐行折回
/// 三元组与 RWX_MATRIX 原表比对，返回失配行数（0=双源一致）。
/// 判据 J1(a) 的"矩阵全过"另由全子集闸测试覆盖；本函数锁的是
/// "match 源"与"表源"不脱节。
#[must_use]
pub fn verify_matrix_dual_source() -> usize {
    let mut mismatch: usize = 0;
    let all = RegionType::all();
    let mut i: usize = 0;
    while i < RegionType::COUNT {
        let row = all[i].row_mask();
        let folded = RWX_MATRIX[i][0] | (RWX_MATRIX[i][1] << 1) | (RWX_MATRIX[i][2] << 2);
        if row.bits() != folded {
            mismatch += 1;
        }
        i += 1;
    }
    mismatch
}

// ═══════════════════════════════════════════════════════════════════════════
// §5 · 账本记录类型（POD 小记录；无 Vec 字段者可 Copy）
// ═══════════════════════════════════════════════════════════════════════════

/// 区域生命周期状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionState {
    /// 活跃（当前映射在册）。
    Active,
    /// 已退役（映射已撤；保留账迹，不可再变更/探账为 fault）。
    Retired,
}

/// 区域登记记录（约 48B POD；堆置 Vec 管理，禁上栈大数组）。
#[derive(Clone, Copy, Debug)]
pub struct RegionRecord {
    /// 区域句柄（引擎内单调发放，u32 饱和计数）。
    pub id: u32,
    /// 区域类型（闭集索引）。
    pub kind: RegionType,
    /// 基址（4K 对齐）。
    pub base: u64,
    /// 长度（4K 对齐，>0）。
    pub len: u64,
    /// 当前生效权限（恒 ⊆ 该类型矩阵行——除非经污染钻孔钩子）。
    pub perms: PermRwx,
    /// 生命周期状态。
    pub state: RegionState,
    /// 登记裁决码（成功行恒 E_OK=0x09F0）。
    pub grant_code: u16,
    /// 登记时事件序号。
    pub born_seq: u64,
    /// 退役时事件序号（未退役为 0）。
    pub retired_seq: u64,
}

impl RegionRecord {
    /// 区末地址（base+len，构造时已保证不溢出；防御性再走 checked）。
    #[must_use]
    pub const fn end(self) -> u64 {
        self.base.wrapping_add(self.len)
    }

    /// 地址是否落入本区跨度（半开区间 [base, base+len)）。
    #[must_use]
    pub const fn contains(self, addr: u64) -> bool {
        addr >= self.base && addr < self.base.wrapping_add(self.len)
    }

    /// 是否与另一活跃区重叠（区间相交判式，防溢出用比较式）。
    #[must_use]
    pub const fn overlaps(self, other_base: u64, other_len: u64) -> bool {
        let a0 = self.base;
        let a1 = self.base.wrapping_add(self.len);
        let b0 = other_base;
        let b1 = other_base.wrapping_add(other_len);
        a0 < b1 && b0 < a1
    }

    /// 权限对类型矩阵行的存量合规性（扫描器口径）：
    /// Ok(())=合规；Err(WxViolation)=存量 W^X；Err(PermSuperset)=超集污染。
    #[must_use]
    pub const fn perms_audit(self) -> Result<(), WxPolicyError> {
        if self.perms.is_wx_violation() {
            Err(WxPolicyError::WxViolation)
        } else if !self.perms.is_subset_of(self.kind.row_mask()) {
            Err(WxPolicyError::PermSuperset)
        } else {
            Ok(())
        }
    }
}

/// 裁决账行（每次裁决留痕：类型/请求位/结论/码）。
#[derive(Clone, Copy, Debug)]
pub struct DecisionRecord {
    /// 全局事件序号。
    pub seq: u64,
    /// 被裁决区域类型。
    pub kind: RegionType,
    /// 请求权限位组。
    pub req: PermRwx,
    /// 结论（true=过闸）。
    pub granted: bool,
    /// 结算码（过闸 E_OK / 拒收对应错误码）。
    pub code: u16,
}

/// 变更账行（mprotect 语义留痕：旧权/新权/结论/码）。
#[derive(Clone, Copy, Debug)]
pub struct MprotectRecord {
    /// 全局事件序号。
    pub seq: u64,
    /// 目标区域句柄。
    pub region: u32,
    /// 变更前权限。
    pub old: PermRwx,
    /// 请求变更后权限。
    pub new: PermRwx,
    /// 结论（true=已应用）。
    pub granted: bool,
    /// 结算码。
    pub code: u16,
}

/// 豁免登记记录（双映射豁免账行；默认 0 条是红线基态）。
#[derive(Clone, Copy, Debug)]
pub struct ExemptRecord {
    /// 被双映射的物理页基址（4K 对齐）。
    pub phys: u64,
    /// 豁免 owner 溯源标签（调用方自定，如任务 id/模块 id）。
    pub owner: u64,
    /// W 别名权限（W 或 RW，不含 X）。
    pub w_alias: PermRwx,
    /// X 别名权限（X 或 RX，不含 W）。
    pub x_alias: PermRwx,
    /// 登记时事件序号。
    pub grant_seq: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// §6 · 引擎事件账（枚举 12 变体 + FNV 摘要；全 POD 可 Copy）
// ═══════════════════════════════════════════════════════════════════════════

/// 引擎事件（事件账行；字段以 u8/u16/u32/u64 POD 打平便于确定性摘要）。
#[derive(Clone, Copy, Debug)]
pub enum EngineEvent {
    /// 区域登记完成（granted=true 时 region 为句柄；false 时 0xFFFF）。
    Register {
        seq: u64,
        region: u32,
        kind: u8,
        base: u64,
        len: u64,
        granted: bool,
        code: u16,
    },
    /// 裁决（过闸/拒收均留痕）。
    Decision {
        seq: u64,
        kind: u8,
        req: u8,
        granted: bool,
        code: u16,
    },
    /// 权限变更。
    Mprotect {
        seq: u64,
        region: u32,
        old: u8,
        new: u8,
        granted: bool,
        code: u16,
    },
    /// 区域退役。
    Retire { seq: u64, region: u32 },
    /// 豁免登记。
    ExemptGrant { seq: u64, phys: u64, owner: u64 },
    /// 双映射检查（ok=有豁免在案放行）。
    ExemptUse { seq: u64, phys: u64, ok: bool },
    /// 豁免撤销。
    ExemptRevoke { seq: u64, phys: u64 },
    /// 访问探针。
    Probe {
        seq: u64,
        addr: u64,
        op: u8,
        allowed: bool,
        pf: u32,
    },
    /// 全区扫描。
    Sweep { seq: u64, scanned: u16, violations: u16 },
    /// 演练注入检出。
    Drill { seq: u64, kind: u8, detected: bool, code: u16 },
    /// 权限污染钻孔（F0316 式故障注入钩子）。
    Corrupt { seq: u64, region: u32, bad: u8 },
    /// 封账。
    Seal { seq: u64 },
}

impl EngineEvent {
    /// FNV 混合步（h ^ v 再乘素数；确定性摘要的基本步）。
    #[must_use]
    pub const fn fnv_mix(h: u64, v: u64) -> u64 {
        (h ^ v).wrapping_mul(FNV_PRIME_64)
    }

    /// 事件确定性摘要（同状态同摘要；FNV-1a 折叠）。
    /// 判别 tag 用逐臂字面量混入（带字段枚举不可 as-cast，
    /// const fn 内亦不走 derive-PartialEq 路径）。
    #[must_use]
    pub const fn digest(&self) -> u64 {
        let mut h: u64 = FNV_OFFSET_64;
        match *self {
            EngineEvent::Register {
                seq,
                region,
                kind,
                base,
                len,
                granted,
                code,
            } => {
                h = EngineEvent::fnv_mix(h, 0x5245_4753_u64); // "REGS"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, region as u64);
                h = EngineEvent::fnv_mix(h, kind as u64);
                h = EngineEvent::fnv_mix(h, base);
                h = EngineEvent::fnv_mix(h, len);
                h = EngineEvent::fnv_mix(h, granted as u64);
                h = EngineEvent::fnv_mix(h, code as u64);
            }
            EngineEvent::Decision {
                seq,
                kind,
                req,
                granted,
                code,
            } => {
                h = EngineEvent::fnv_mix(h, 0x4443_4953_u64); // "DCIS"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, kind as u64);
                h = EngineEvent::fnv_mix(h, req as u64);
                h = EngineEvent::fnv_mix(h, granted as u64);
                h = EngineEvent::fnv_mix(h, code as u64);
            }
            EngineEvent::Mprotect {
                seq,
                region,
                old,
                new,
                granted,
                code,
            } => {
                h = EngineEvent::fnv_mix(h, 0x4D50_5254_u64); // "MPRT"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, region as u64);
                h = EngineEvent::fnv_mix(h, old as u64);
                h = EngineEvent::fnv_mix(h, new as u64);
                h = EngineEvent::fnv_mix(h, granted as u64);
                h = EngineEvent::fnv_mix(h, code as u64);
            }
            EngineEvent::Retire { seq, region } => {
                h = EngineEvent::fnv_mix(h, 0x5245_5449_u64); // "RETI"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, region as u64);
            }
            EngineEvent::ExemptGrant { seq, phys, owner } => {
                h = EngineEvent::fnv_mix(h, 0x4547_5254_u64); // "EGRT"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, phys);
                h = EngineEvent::fnv_mix(h, owner);
            }
            EngineEvent::ExemptUse { seq, phys, ok } => {
                h = EngineEvent::fnv_mix(h, 0x4555_5345_u64); // "EUSE"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, phys);
                h = EngineEvent::fnv_mix(h, ok as u64);
            }
            EngineEvent::ExemptRevoke { seq, phys } => {
                h = EngineEvent::fnv_mix(h, 0x4552_564B_u64); // "ERVK"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, phys);
            }
            EngineEvent::Probe {
                seq,
                addr,
                op,
                allowed,
                pf,
            } => {
                h = EngineEvent::fnv_mix(h, 0x5052_4245_u64); // "PRBE"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, addr);
                h = EngineEvent::fnv_mix(h, op as u64);
                h = EngineEvent::fnv_mix(h, allowed as u64);
                h = EngineEvent::fnv_mix(h, pf as u64);
            }
            EngineEvent::Sweep { seq, scanned, violations } => {
                h = EngineEvent::fnv_mix(h, 0x5357_5050_u64); // "SWPP"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, scanned as u64);
                h = EngineEvent::fnv_mix(h, violations as u64);
            }
            EngineEvent::Drill {
                seq,
                kind,
                detected,
                code,
            } => {
                h = EngineEvent::fnv_mix(h, 0x4452_4C4C_u64); // "DRLL"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, kind as u64);
                h = EngineEvent::fnv_mix(h, detected as u64);
                h = EngineEvent::fnv_mix(h, code as u64);
            }
            EngineEvent::Corrupt { seq, region, bad } => {
                h = EngineEvent::fnv_mix(h, 0x4352_5054_u64); // "CRPT"
                h = EngineEvent::fnv_mix(h, seq);
                h = EngineEvent::fnv_mix(h, region as u64);
                h = EngineEvent::fnv_mix(h, bad as u64);
            }
            EngineEvent::Seal { seq } => {
                h = EngineEvent::fnv_mix(h, 0x5345_414C_u64); // "SEAL"
                h = EngineEvent::fnv_mix(h, seq);
            }
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7 · Ring<T> 环形账本（满环驱逐最旧；驱逐由调用方计数）
// ═══════════════════════════════════════════════════════════════════════════

/// 定容环形账本（堆置 Vec 缓冲 + 头游标；push 满环驱逐最旧并返回 true）。
/// cap 为编译期常量口径（构造方传入本模块 MAX_* 常量，恒 >0）。
#[derive(Debug)]
struct Ring<T> {
    buf: Vec<T>,
    head: usize,
    len: usize,
    cap: usize,
}

impl<T> Ring<T> {
    /// 新建定容环（cap>0 由调用方常量保证；本模块恒传 MAX_* 常量）。
    fn with_cap(cap: usize) -> Ring<T> {
        Ring {
            buf: Vec::with_capacity(cap),
            head: 0,
            len: 0,
            cap,
        }
    }

    /// 追加一行；返回是否驱逐了最旧一行（容量审计用）。
    fn push(&mut self, item: T) -> bool {
        if self.len < self.cap {
            self.buf.push(item);
            self.len += 1;
            false
        } else {
            // 满环：覆写最旧位（head 恒 < cap == buf.len()，索引安全；
            // 运行期 Vec 索引不受 const fn 纪律约束）。
            self.buf[self.head] = item;
            self.head = (self.head + 1) % self.cap;
            true
        }
    }

    /// 有序取行（i=0 为最旧在场行）；越界 None。
    fn at(&self, i: usize) -> Option<&T> {
        if i >= self.len {
            None
        } else {
            Some(&self.buf[(self.head + i) % self.cap])
        }
    }

    /// 最新一行。
    fn last(&self) -> Option<&T> {
        if self.len == 0 {
            None
        } else {
            self.at(self.len - 1)
        }
    }

    /// 在场行数。
    fn count(&self) -> usize {
        self.len
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8 · 引擎统计（全 u64 饱和计数 + 每类型分桶 + 基点比率）
// ═══════════════════════════════════════════════════════════════════════════

/// 引擎计数器族（判据"拒收率统计"口径：总比 + 分类型比；
/// derive PartialEq 供快照对拍相等比较——非 const fn 路径）。
#[derive(Clone, Debug, PartialEq)]
pub struct EngineStats {
    /// 区域登记成功数。
    pub regions_registered: u64,
    /// 区域退役数。
    pub regions_retired: u64,
    /// 裁决总数（过+拒）。
    pub decisions_total: u64,
    /// 裁决过闸数。
    pub decisions_pass: u64,
    /// 裁决拒收数。
    pub decisions_reject: u64,
    /// 裁决账满环驱逐数。
    pub decision_evictions: u64,
    /// 每类型裁决总数（索引=RegionType::index）。
    pub per_kind_total: [u64; RegionType::COUNT],
    /// 每类型裁决拒收数。
    pub per_kind_reject: [u64; RegionType::COUNT],
    /// 变更请求总数。
    pub mprotect_total: u64,
    /// 变更应用成功数。
    pub mprotect_ok: u64,
    /// 变更升权拒收数。
    pub mprotect_upgrade_rejected: u64,
    /// W^X 红线立案数（拒收 + 存量检出 + 污染钻孔注入）。
    pub wx_redlines: u64,
    /// 豁免登记数。
    pub exempt_granted: u64,
    /// 豁免撤销数。
    pub exempt_revoked: u64,
    /// 双映射检查总数。
    pub dualmap_checks: u64,
    /// 双映射放行数（豁免在案）。
    pub dualmap_passed: u64,
    /// 探针总数。
    pub probes_total: u64,
    /// 探针故障数。
    pub probe_faults: u64,
    /// 扫描次数。
    pub sweeps_total: u64,
    /// 扫描累计立案数。
    pub sweep_violations: u64,
    /// 权限污染钻孔次数。
    pub corrupt_drills: u64,
    /// 演练（三注入）轮数。
    pub drills_run: u64,
    /// 事件账满环驱逐数。
    pub event_evictions: u64,
}

impl EngineStats {
    /// 全零统计（引擎构造口径）。
    #[must_use]
    pub fn zeros() -> EngineStats {
        EngineStats {
            regions_registered: 0,
            regions_retired: 0,
            decisions_total: 0,
            decisions_pass: 0,
            decisions_reject: 0,
            decision_evictions: 0,
            per_kind_total: [0; RegionType::COUNT],
            per_kind_reject: [0; RegionType::COUNT],
            mprotect_total: 0,
            mprotect_ok: 0,
            mprotect_upgrade_rejected: 0,
            wx_redlines: 0,
            exempt_granted: 0,
            exempt_revoked: 0,
            dualmap_checks: 0,
            dualmap_passed: 0,
            probes_total: 0,
            probe_faults: 0,
            sweeps_total: 0,
            sweep_violations: 0,
            corrupt_drills: 0,
            drills_run: 0,
            event_evictions: 0,
        }
    }

    /// 总拒收率（万基点 bp；0 裁决时定义为 0；上界 10000 封顶防溢出）。
    #[must_use]
    pub fn reject_rate_bp(&self) -> u16 {
        if self.decisions_total == 0 {
            0
        } else {
            let bp = self
                .decisions_reject
                .saturating_mul(10_000)
                .checked_div(self.decisions_total);
            match bp {
                Some(v) if v <= 10_000 => v as u16,
                _ => 10_000,
            }
        }
    }

    /// 分类型拒收率（万基点；该类型 0 裁决时为 0）。
    #[must_use]
    pub fn kind_reject_rate_bp(&self, kind: RegionType) -> u16 {
        let i = kind.index() as usize;
        let total = self.per_kind_total[i];
        if total == 0 {
            0
        } else {
            let bp = self.per_kind_reject[i]
                .saturating_mul(10_000)
                .checked_div(total);
            match bp {
                Some(v) if v <= 10_000 => v as u16,
                _ => 10_000,
            }
        }
    }

    /// 活跃豁免数（登记-撤销）。
    #[must_use]
    pub fn exempt_active(&self) -> u64 {
        self.exempt_granted
            .saturating_sub(self.exempt_revoked)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §9 · 探针/扫描/演练/快照/健康报告载荷类型
// ═══════════════════════════════════════════════════════════════════════════

/// 访问类别（探针轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessOp {
    /// 读访问。
    Read,
    /// 写访问。
    Write,
    /// 取指访问。
    Exec,
}

impl AccessOp {
    /// 探针事件轴编号（0/1/2，事件账 digest 用）。
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            AccessOp::Read => 0,
            AccessOp::Write => 1,
            AccessOp::Exec => 2,
        }
    }

    /// 对应权限位（R/W/X）。
    #[must_use]
    pub const fn perm_bit(self) -> u8 {
        match self {
            AccessOp::Read => PERM_R,
            AccessOp::Write => PERM_W,
            AccessOp::Exec => PERM_X,
        }
    }

    /// 中文动作名。
    #[must_use]
    pub const fn zh(self) -> &'static str {
        match self {
            AccessOp::Read => "读",
            AccessOp::Write => "写",
            AccessOp::Exec => "取指",
        }
    }
}

/// 访问探针结果（探针不失败——fault 是正常结果口径）。
#[derive(Clone, Copy, Debug)]
pub struct ProbeOutcome {
    /// 命中区域句柄（无映射为 None）。
    pub region: Option<u32>,
    /// 访问类别。
    pub op: AccessOp,
    /// 是否放行。
    pub allowed: bool,
    /// 合成的 #PF 错误码（放行为 0；故障按 SDM §6.15 位定义）。
    pub pf: u32,
    /// 结算诊断码（放行 E_OK；无区 RegionUnknown；越权 PermSuperset）。
    pub diag: u16,
}

/// 扫描立案行。
#[derive(Clone, Copy, Debug)]
pub struct SweepViolation {
    /// 违例区域句柄。
    pub region: u32,
    /// 区域类型。
    pub kind: RegionType,
    /// 污染后的权限位组。
    pub perms: PermRwx,
    /// 立案码（WxViolation 或 PermSuperset）。
    pub code: u16,
}

/// 全区扫描报告。
#[derive(Clone, Debug)]
pub struct SweepReport {
    /// 扫描的活跃区域数。
    pub scanned: u16,
    /// 立案清单。
    pub violations: Vec<SweepViolation>,
    /// 是否零立案。
    pub clean: bool,
}

/// 三注入演练报告（W^X 违例注入/越权升级注入/未登记豁免注入）。
#[derive(Clone, Copy, Debug)]
pub struct DrillReport {
    /// W^X 注入试次（8 类型 × 2 违例组合 = 16）。
    pub wx_attempts: u32,
    /// W^X 注入检出数（红线要求 = 试次数，100%）。
    pub wx_detected: u32,
    /// W^X 注入结算码（应恒 WxViolation=0x09F2）。
    pub wx_code: u16,
    /// 升权注入是否检出（应恒 true，码 MprotectUpgrade）。
    pub upgrade_detected: bool,
    /// 升权注入结算码。
    pub upgrade_code: u16,
    /// 未登记豁免注入是否检出（应恒 true，码 ExemptUnregistered）。
    pub dualmap_detected: bool,
    /// 未登记豁免注入结算码。
    pub dualmap_code: u16,
    /// 三注入全检出（演练判据）。
    pub all_detected: bool,
}

/// 双跑对拍报告（指纹级 + 快照级双口径）。
#[derive(Clone, Debug)]
pub struct DoubleRunReport {
    /// 剧本种子。
    pub seed: u64,
    /// 第一跑指纹。
    pub fp_a: u64,
    /// 第二跑指纹。
    pub fp_b: u64,
    /// 指纹一致。
    pub fingerprints_equal: bool,
    /// 快照差异数。
    pub snapshot_diff_count: u32,
    /// 快照差异头部样本（至多 8 条，防报告膨胀）。
    pub snapshot_diff_head: Vec<String>,
    /// 双口径一致（判据"双跑确定性对拍"）。
    pub consistent: bool,
}

/// 引擎快照（状态级对拍口径；derive PartialEq 供测试断言，
/// 非内核路径亦只作相等比较用）。
#[derive(Clone, Debug, PartialEq)]
pub struct EngineSnapshot {
    /// 快照时全局序号。
    pub seq: u64,
    /// 状态指纹。
    pub fingerprint: u64,
    /// 区域总数。
    pub regions_total: usize,
    /// 活跃区域数。
    pub regions_active: usize,
    /// 裁决账在场行数。
    pub decisions_len: usize,
    /// 变更账在场行数。
    pub mprotects_len: usize,
    /// 事件账在场行数。
    pub events_len: usize,
    /// 活跃豁免数。
    pub exempt_active: usize,
    /// 统计族副本。
    pub stats: EngineStats,
}

/// 健康报告（health() 口径；豁免默认 0 红线在此结算）。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 报告时全局序号。
    pub seq: u64,
    /// 状态指纹。
    pub fingerprint: u64,
    /// 矩阵运行期复核通过（8 行零 W^X；恒 true，防手改 const 面的
    /// 纵深复核）。
    pub matrix_clean: bool,
    /// 活跃豁免数。
    pub exempt_active: u64,
    /// 豁免默认 0 红线是否守住的基态（活跃豁免 == 0）。
    pub exempt_zero_default: bool,
    /// 总拒收率（bp）。
    pub reject_rate_bp: u16,
    /// 裁决总数。
    pub decisions_total: u64,
    /// W^X 红线累计立案数。
    pub wx_redlines: u64,
    /// 是否已封账。
    pub sealed: bool,
    /// 健康判据（矩阵净 && 豁免零基态）。
    pub verdict_ok: bool,
    /// 附注清单（豁免非零/高拒收率/账本驱逐等提示）。
    pub notes: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// §10 · DetLcg 确定性剧本源（Knuth MMIX；无时钟无随机源依赖）
// ═══════════════════════════════════════════════════════════════════════════

/// 确定性线性同余发生器（剧本回放专用；非密码源，来源 TAOCP Vol.2）。
#[derive(Clone, Debug)]
pub struct DetLcg {
    state: u64,
}

impl DetLcg {
    /// 播种。
    #[must_use]
    pub const fn new(seed: u64) -> DetLcg {
        DetLcg { state: seed }
    }

    /// 下一原值（wrapping 显性，不 panic 不失败）。
    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_MULT)
            .wrapping_add(LCG_INC);
        // splitmix 风格散射，改善低位质量（常数 0xff51afd7ed558ccd /
        // 0xc4ceb9fe1a85ec53 来自 MurmurHash3 fmix64 冻结值）。
        let z = self.state ^ (self.state >> 33);
        let z = z.wrapping_mul(0xff51_afd7_ed55_8ccd);
        let z = z ^ (z >> 33);
        z
    }

    /// 有界取值（mod 非零界；界恒为非零常量）。
    #[must_use]
    pub fn next_below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §11 · WxPolicyEngine 策略引擎主体
// ═══════════════════════════════════════════════════════════════════════════

/// W^X 权限位策略引擎（区域申请→矩阵裁决；变更路径；豁免通道；
/// 账本/统计/扫描/演练/快照/封账全口径）。
///
/// 结构全为堆置 Vec 账本 + POD 游标，栈上只留句柄（<200B），
/// 符合 >64KiB 禁上栈纪律。
#[derive(Debug)]
pub struct WxPolicyEngine {
    /// 区域登记表（含退役账迹；上限 MAX_REGIONS）。
    regions: Vec<RegionRecord>,
    /// 下一区域句柄（u32 饱和；到顶后拒绝新登记）。
    next_region_id: u32,
    /// 裁决账（环形）。
    decisions: Ring<DecisionRecord>,
    /// 变更账（环形）。
    mprotects: Ring<MprotectRecord>,
    /// 豁免登记表（上限 MAX_EXEMPTS；撤销即移除，账迹在事件账+统计）。
    exempts: Vec<ExemptRecord>,
    /// 事件账（环形）。
    events: Ring<EngineEvent>,
    /// 统计族。
    stats: EngineStats,
    /// 封账标志。
    sealed: bool,
    /// 全局事件序号（从 1 起，0 保留"无事件"）。
    seq: u64,
    /// 批量变更暂存（batch_begin 后有效；含 Vec 字段的结构按纪律
    /// 不 derive Copy，仅 Clone）。
    batch: Option<Vec<(u32, PermRwx)>>,
}

impl WxPolicyEngine {
    /// 构造引擎（账本容量取本模块 MAX_* 常量；堆分配在构造期一次完成）。
    #[must_use]
    pub fn new() -> WxPolicyEngine {
        WxPolicyEngine {
            regions: Vec::with_capacity(MAX_REGIONS),
            next_region_id: 0,
            decisions: Ring::with_cap(MAX_DECISIONS),
            mprotects: Ring::with_cap(MAX_MPROTECTS),
            exempts: Vec::with_capacity(MAX_EXEMPTS),
            events: Ring::with_cap(MAX_EVENTS),
            stats: EngineStats::zeros(),
            sealed: false,
            seq: 0,
            batch: None,
        }
    }

    // ── 内部账务助手 ─────────────────────────────────────────────────────

    /// 取下一全局序号（饱和；饱和后序号停顶——账迹仍可追加，
    /// 双跑对拍以同序同状态为准）。
    fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq = self.seq.saturating_add(1);
        s
    }

    /// 事件入账（满环驱逐计数）。
    fn push_event(&mut self, ev: EngineEvent) {
        if self.events.push(ev) {
            self.stats.event_evictions = self.stats.event_evictions.saturating_add(1);
        }
    }

    /// 裁决入账 + 统计（含每类型分桶与红线计数）。
    fn record_decision(&mut self, kind: RegionType, req: PermRwx, granted: bool, code: u16) {
        let seq = self.next_seq();
        let rec = DecisionRecord {
            seq,
            kind,
            req,
            granted,
            code,
        };
        if self.decisions.push(rec) {
            self.stats.decision_evictions = self.stats.decision_evictions.saturating_add(1);
        }
        self.stats.decisions_total = self.stats.decisions_total.saturating_add(1);
        if granted {
            self.stats.decisions_pass = self.stats.decisions_pass.saturating_add(1);
        } else {
            self.stats.decisions_reject = self.stats.decisions_reject.saturating_add(1);
        }
        let i = kind.index() as usize;
        self.stats.per_kind_total[i] = self.stats.per_kind_total[i].saturating_add(1);
        if !granted {
            self.stats.per_kind_reject[i] = self.stats.per_kind_reject[i].saturating_add(1);
        }
        if code == WxPolicyError::WxViolation.code() {
            self.stats.wx_redlines = self.stats.wx_redlines.saturating_add(1);
        }
        self.push_event(EngineEvent::Decision {
            seq,
            kind: kind.index(),
            req: req.bits(),
            granted,
            code,
        });
    }

    /// 矩阵闸（位法→W^X 红线→子集判；三查一体，拒收即入裁决账）。
    /// 判据 J1：请求 ⊆ 行则过，超集拒；W^X 一律红案（优先于超集
    /// 结算码，保证 100% 检出口径落在 WxViolation 码上）。
    fn matrix_gate(&mut self, kind: RegionType, req: PermRwx) -> Result<(), WxPolicyError> {
        if !req.is_legal() {
            self.record_decision(kind, req, false, WxPolicyError::IllegalPermBits.code());
            return Err(WxPolicyError::IllegalPermBits);
        }
        if req.is_wx_violation() {
            self.record_decision(kind, req, false, WxPolicyError::WxViolation.code());
            return Err(WxPolicyError::WxViolation);
        }
        if !req.is_subset_of(kind.row_mask()) {
            self.record_decision(kind, req, false, WxPolicyError::PermSuperset.code());
            return Err(WxPolicyError::PermSuperset);
        }
        self.record_decision(kind, req, true, E_OK);
        Ok(())
    }

    /// 活跃区重叠检查（命中返回首个重叠区句柄）。
    fn find_overlap(&self, base: u64, len: u64) -> Option<u32> {
        let mut i: usize = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            if r.state == RegionState::Active && r.overlaps(base, len) {
                return Some(r.id);
            }
            i += 1;
        }
        None
    }

    /// 按句柄找区域下标（活跃优先口径由调用方定；此处查原始记录）。
    fn region_pos(&self, id: u32) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.regions.len() {
            if self.regions[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    // ── 区域登记面（判据 J1(a) 主路径）───────────────────────────────────

    /// 区域登记（类型 + 基址 + 长度 + 请求权限 → 矩阵裁决）。
    ///
    /// 闸序：封账 → 句柄余量 → 形状（4K 对齐/非零/不溢出）→
    /// 活跃重叠 → 矩阵闸（位法→W^X→子集）。全过则入区域表并返回句柄。
    pub fn register_region(
        &mut self,
        kind: RegionType,
        base: u64,
        len: u64,
        req: PermRwx,
    ) -> Result<u32, WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        if self.regions.len() >= MAX_REGIONS {
            return Err(WxPolicyError::LedgerFull);
        }
        if self.next_region_id == u32::MAX {
            return Err(WxPolicyError::LedgerFull);
        }
        // 形状闸：非零、4K 对齐、base+len 不溢出 u64。
        if len == 0
            || base % PAGE_4K != 0
            || len % PAGE_4K != 0
            || base.checked_add(len).is_none()
        {
            return Err(WxPolicyError::RegionShape);
        }
        // MMIO/页表区另需物理口径对齐说明：本单 base 即映射基址，
        // 4K 对齐已覆盖；大页粒度属 F0305 不在本闸。
        if self.find_overlap(base, len).is_some() {
            return Err(WxPolicyError::RegionOverlap);
        }
        // 矩阵闸（拒收已入裁决账）。
        self.matrix_gate(kind, req)?;
        let seq = self.next_seq();
        let id = self.next_region_id;
        self.next_region_id = self.next_region_id.saturating_add(1);
        self.regions.push(RegionRecord {
            id,
            kind,
            base,
            len,
            perms: req,
            state: RegionState::Active,
            grant_code: E_OK,
            born_seq: seq,
            retired_seq: 0,
        });
        self.stats.regions_registered = self.stats.regions_registered.saturating_add(1);
        self.push_event(EngineEvent::Register {
            seq,
            region: id,
            kind: kind.index(),
            base,
            len,
            granted: true,
            code: E_OK,
        });
        Ok(id)
    }

    /// 纯裁决（不登记区域，只过闸留痕；查询/演练面）。
    pub fn adjudicate(&mut self, kind: RegionType, req: PermRwx) -> Result<PermRwx, WxPolicyError> {
        if !self.sealed {
            self.matrix_gate(kind, req)?;
        } else {
            // 封账后纯裁决仍可算（只读语义），但不再留新账——
            // 以临时副本结算，避免污染冻结账本。
            if !req.is_legal() {
                return Err(WxPolicyError::IllegalPermBits);
            }
            if req.is_wx_violation() {
                return Err(WxPolicyError::WxViolation);
            }
            if !req.is_subset_of(kind.row_mask()) {
                return Err(WxPolicyError::PermSuperset);
            }
        }
        Ok(req)
    }

    /// 按句柄取区域记录。
    #[must_use]
    pub fn region(&self, id: u32) -> Option<&RegionRecord> {
        match self.region_pos(id) {
            Some(i) => Some(&self.regions[i]),
            None => None,
        }
    }

    /// 按地址取活跃区域（线性扫，≤MAX_REGIONS）。
    #[must_use]
    pub fn region_by_addr(&self, addr: u64) -> Option<&RegionRecord> {
        let mut i: usize = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            if r.state == RegionState::Active && r.contains(addr) {
                return Some(&self.regions[i]);
            }
            i += 1;
        }
        None
    }

    /// 区域表快照引用（审计读口径）。
    #[must_use]
    pub fn regions(&self) -> &[RegionRecord] {
        &self.regions
    }

    /// 退役区域（撤销映射语义；账迹保留，句柄不复用）。
    pub fn retire_region(&mut self, id: u32) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        match self.region_pos(id) {
            None => Err(WxPolicyError::RegionUnknown),
            Some(i) => {
                if self.regions[i].state == RegionState::Retired {
                    return Err(WxPolicyError::BadState);
                }
                let seq = self.next_seq();
                self.regions[i].state = RegionState::Retired;
                self.regions[i].retired_seq = seq;
                self.stats.regions_retired = self.stats.regions_retired.saturating_add(1);
                self.push_event(EngineEvent::Retire { seq, region: id });
                Ok(())
            }
        }
    }

    // ── 变更路径（mprotect 语义：类型不变改权限；降权自由升权拒）─────────

    /// 权限变更（mprotect 语义）。
    ///
    /// 闸序：封账 → 句柄 → 活跃态 → 矩阵闸（新权限仍须 ⊆ 该类型行，
    /// W^X 同红线）→ 升权判（new 含 old 没有的位即拒）。应用后旧权返回。
    pub fn mprotect(&mut self, id: u32, new_perms: PermRwx) -> Result<PermRwx, WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        let pos = match self.region_pos(id) {
            None => return Err(WxPolicyError::RegionUnknown),
            Some(p) => p,
        };
        if self.regions[pos].state != RegionState::Active {
            return Err(WxPolicyError::BadState);
        }
        let kind = self.regions[pos].kind;
        let old = self.regions[pos].perms;
        // 矩阵闸先行（拒收入裁决账）。
        self.matrix_gate(kind, new_perms)?;
        // 升权判：new ⊄ old 即拒（降权/等权放行）。
        if new_perms.has_extra_bits_of(old) {
            let code = WxPolicyError::MprotectUpgrade.code();
            let seq = self.next_seq();
            let rec = MprotectRecord {
                seq,
                region: id,
                old,
                new: new_perms,
                granted: false,
                code,
            };
            // 变更账驱逐不另设计数（容量审计以事件账/裁决账为口径）。
            let _ = self.mprotects.push(rec);
            self.stats.mprotect_total = self.stats.mprotect_total.saturating_add(1);
            self.stats.mprotect_upgrade_rejected =
                self.stats.mprotect_upgrade_rejected.saturating_add(1);
            self.push_event(EngineEvent::Mprotect {
                seq,
                region: id,
                old: old.bits(),
                new: new_perms.bits(),
                granted: false,
                code,
            });
            return Err(WxPolicyError::MprotectUpgrade);
        }
        let seq = self.next_seq();
        self.regions[pos].perms = new_perms;
        let rec = MprotectRecord {
            seq,
            region: id,
            old,
            new: new_perms,
            granted: true,
            code: E_OK,
        };
        let _ = self.mprotects.push(rec);
        self.stats.mprotect_total = self.stats.mprotect_total.saturating_add(1);
        self.stats.mprotect_ok = self.stats.mprotect_ok.saturating_add(1);
        self.push_event(EngineEvent::Mprotect {
            seq,
            region: id,
            old: old.bits(),
            new: new_perms.bits(),
            granted: true,
            code: E_OK,
        });
        Ok(old)
    }

    /// 批量变更开批（后续以 batch_stage 暂存；batch_apply 原子应用，
    /// 任一笔失败整批回滚——回滚=已应用各笔恢复旧权并逆转账）。
    pub fn batch_begin(&mut self) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        if self.batch.is_some() {
            return Err(WxPolicyError::BadState);
        }
        self.batch = Some(Vec::new());
        Ok(())
    }

    /// 批内暂存一笔降权（不立即应用；升权/非法位在 apply 时统一结算）。
    pub fn batch_stage(&mut self, id: u32, new_perms: PermRwx) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        match self.batch.as_mut() {
            None => Err(WxPolicyError::BadState),
            Some(v) => {
                v.push((id, new_perms));
                Ok(())
            }
        }
    }

    /// 批量回滚专用恢复器：直接把区域权限恢复为 rollback_to。
    ///
    /// 恢复的权限必然 ⊆ 该类型矩阵行（它曾是经全闸应用过的合法权限），
    /// 故直接落位不破坏矩阵不变量；变更账/事件账仍双向留痕
    /// （from→to 记录，granted=true，审计可回放批的正反两向）。
    /// 仅供 batch_apply 失败回滚调用，不对生产路径开放。
    fn restore_perms(&mut self, id: u32, from: PermRwx, rollback_to: PermRwx) {
        if let Some(p) = self.region_pos(id) {
            self.regions[p].perms = rollback_to;
        }
        let seq = self.next_seq();
        let rec = MprotectRecord {
            seq,
            region: id,
            old: from,
            new: rollback_to,
            granted: true,
            code: E_OK,
        };
        let _ = self.mprotects.push(rec);
        self.stats.mprotect_total = self.stats.mprotect_total.saturating_add(1);
        self.stats.mprotect_ok = self.stats.mprotect_ok.saturating_add(1);
        self.push_event(EngineEvent::Mprotect {
            seq,
            region: id,
            old: from.bits(),
            new: rollback_to.bits(),
            granted: true,
            code: E_OK,
        });
    }

    /// 批量应用（原子语义：逐笔走 mprotect 全闸；首笔失败即中止，
    /// 已应用各笔逐笔回滚恢复旧权；返回应用成功的净笔数 0）。
    /// 任意失败返回 Err（回滚后引擎回到开批前权限状态，变更账保留
    /// 正反双向留痕——审计可回放）。
    pub fn batch_apply(&mut self) -> Result<usize, WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        let staged = match self.batch.take() {
            None => return Err(WxPolicyError::BadState),
            Some(v) => v,
        };
        let mut applied: Vec<(u32, PermRwx)> = Vec::new();
        let mut failure: Option<WxPolicyError> = None;
        let mut k: usize = 0;
        while k < staged.len() {
            let (id, np) = staged[k];
            match self.mprotect(id, np) {
                Ok(old) => {
                    applied.push((id, old));
                }
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
            k += 1;
        }
        match failure {
            None => Ok(applied.len()),
            Some(e) => {
                // 回滚：逆序恢复旧权（restore_perms 直落位——恢复目标
                // 是曾经合法的旧权，矩阵不变量不受破坏）。
                let mut j: usize = applied.len();
                while j > 0 {
                    j -= 1;
                    let (id, old) = applied[j];
                    let from = match self.region_pos(id) {
                        Some(p) => self.regions[p].perms,
                        None => old,
                    };
                    self.restore_perms(id, from, old);
                }
                Err(e)
            }
        }
    }

    /// 弃批（未应用的暂存清空）。
    pub fn batch_abort(&mut self) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        self.batch = None;
        Ok(())
    }

    // ── 例外通道（显式双映射豁免；默认 0 红线）──────────────────────────

    /// 豁免别名形状闸：单别名先过 W^X 红线，再过形状判
    /// （W 别名含 W 不含 X；X 别名含 X 不含 W；R 位两者皆可携带）。
    fn alias_gate(w: PermRwx, x: PermRwx) -> Result<(), WxPolicyError> {
        if !w.is_legal() || !x.is_legal() {
            return Err(WxPolicyError::IllegalPermBits);
        }
        if w.is_wx_violation() || x.is_wx_violation() {
            return Err(WxPolicyError::WxViolation);
        }
        if !w.is_w() || w.is_x() || !x.is_x() || x.is_w() {
            return Err(WxPolicyError::ExemptAliasDirty);
        }
        Ok(())
    }

    /// 登记双映射豁免（同物理页一 W 一 X 两个别名；每条豁免留 owner
    /// 溯源；一页至多一条活跃豁免）。
    pub fn register_exempt(
        &mut self,
        phys: u64,
        owner: u64,
        w_alias: PermRwx,
        x_alias: PermRwx,
    ) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        if phys % PAGE_4K != 0 {
            return Err(WxPolicyError::RegionShape);
        }
        // 别名闸（W^X 红线在此路径同样计数——alias_gate 不经 matrix_gate）。
        if let Err(e) = WxPolicyEngine::alias_gate(w_alias, x_alias) {
            if e == WxPolicyError::WxViolation {
                self.stats.wx_redlines = self.stats.wx_redlines.saturating_add(1);
            }
            return Err(e);
        }
        if self.exempts.len() >= MAX_EXEMPTS {
            return Err(WxPolicyError::ExemptFull);
        }
        let mut i: usize = 0;
        while i < self.exempts.len() {
            if self.exempts[i].phys == phys {
                return Err(WxPolicyError::ExemptDuplicate);
            }
            i += 1;
        }
        let seq = self.next_seq();
        self.exempts.push(ExemptRecord {
            phys,
            owner,
            w_alias,
            x_alias,
            grant_seq: seq,
        });
        self.stats.exempt_granted = self.stats.exempt_granted.saturating_add(1);
        self.push_event(EngineEvent::ExemptGrant { seq, phys, owner });
        Ok(())
    }

    /// 撤销豁免（恢复默认 0 红线基态方向的通道；撤销不存在者红案）。
    pub fn revoke_exempt(&mut self, phys: u64) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        let mut pos: Option<usize> = None;
        let mut i: usize = 0;
        while i < self.exempts.len() {
            if self.exempts[i].phys == phys {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        match pos {
            None => Err(WxPolicyError::ExemptUnknown),
            Some(p) => {
                let seq = self.next_seq();
                self.exempts.remove(p);
                self.stats.exempt_revoked = self.stats.exempt_revoked.saturating_add(1);
                self.push_event(EngineEvent::ExemptRevoke { seq, phys });
                Ok(())
            }
        }
    }

    /// 双映射检查（未登记豁免的双映射检出面）。
    ///
    /// 同物理页 W 别名与 X 别名并存时：别名形状先过闸（单别名绝不
    /// W^X），然后必须有活跃豁免在案才放行，否则按 W^X 攻击面红案
    /// （ExemptUnregistered）。
    pub fn check_dual_map(
        &mut self,
        phys: u64,
        w_alias: PermRwx,
        x_alias: PermRwx,
    ) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        if phys % PAGE_4K != 0 {
            return Err(WxPolicyError::RegionShape);
        }
        // 别名闸（W^X 红线计数同 register_exempt 口径）。
        if let Err(e) = WxPolicyEngine::alias_gate(w_alias, x_alias) {
            if e == WxPolicyError::WxViolation {
                self.stats.wx_redlines = self.stats.wx_redlines.saturating_add(1);
            }
            return Err(e);
        }
        self.stats.dualmap_checks = self.stats.dualmap_checks.saturating_add(1);
        let mut found = false;
        let mut i: usize = 0;
        while i < self.exempts.len() {
            if self.exempts[i].phys == phys {
                found = true;
                break;
            }
            i += 1;
        }
        let seq = self.next_seq();
        if found {
            self.stats.dualmap_passed = self.stats.dualmap_passed.saturating_add(1);
            self.push_event(EngineEvent::ExemptUse { seq, phys, ok: true });
            Ok(())
        } else {
            self.push_event(EngineEvent::ExemptUse { seq, phys, ok: false });
            Err(WxPolicyError::ExemptUnregistered)
        }
    }

    /// 豁免表只读视图。
    #[must_use]
    pub fn exempts(&self) -> &[ExemptRecord] {
        &self.exempts
    }

    // ── 访问探针（#PF 错误码合成；SDM §6.15 位定义）─────────────────────

    /// 访问探针：按地址+访问类合成放行/故障结论。
    ///
    /// 探针不返回 Result——fault 是正常结论口径（探针的意义就是把
    /// "会以 #PF 收场"的访问翻译成错误码位指）。封账后只读放行
    /// （不追加事件账）。
    pub fn probe(&mut self, addr: u64, op: AccessOp) -> ProbeOutcome {
        let hit = self.region_by_addr(addr);
        let outcome = match hit {
            None => ProbeOutcome {
                region: None,
                op,
                allowed: false,
                pf: pf_wr_op(op) | pf_id_op(op),
                diag: WxPolicyError::RegionUnknown.code(),
            },
            Some(r) => {
                if r.perms.permits(op.perm_bit()) {
                    ProbeOutcome {
                        region: Some(r.id),
                        op,
                        allowed: true,
                        pf: 0,
                        diag: E_OK,
                    }
                } else {
                    ProbeOutcome {
                        region: Some(r.id),
                        op,
                        allowed: false,
                        pf: PF_P | pf_wr_op(op) | pf_id_op(op),
                        diag: WxPolicyError::PermSuperset.code(),
                    }
                }
            }
        };
        if !self.sealed {
            let seq = self.next_seq();
            self.stats.probes_total = self.stats.probes_total.saturating_add(1);
            if !outcome.allowed {
                self.stats.probe_faults = self.stats.probe_faults.saturating_add(1);
            }
            self.push_event(EngineEvent::Probe {
                seq,
                addr,
                op: op.tag(),
                allowed: outcome.allowed,
                pf: outcome.pf,
            });
        }
        outcome
    }

    // ── 存量扫描（W^X 全区扫描器）────────────────────────────────────────

    /// 全区扫描：逐活跃区做存量权限审计（W^X 检出 / 超集检出）。
    ///
    /// 正常路径下区域权限恒合规（登记与变更都过矩阵闸），本扫描的
    /// 检出面是污染钻孔（corrupt_region_perms_for_drill）注入的坏权
    /// 限——F0316 式"坏项检出 100%"在本单的落点。
    pub fn sweep_wx(&mut self) -> SweepReport {
        let mut violations: Vec<SweepViolation> = Vec::new();
        let mut scanned: u16 = 0;
        let mut i: usize = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            if r.state == RegionState::Active {
                scanned = scanned.saturating_add(1);
                if let Err(e) = r.perms_audit() {
                    violations.push(SweepViolation {
                        region: r.id,
                        kind: r.kind,
                        perms: r.perms,
                        code: e.code(),
                    });
                }
            }
            i += 1;
        }
        let clean = violations.is_empty();
        let seq = self.next_seq();
        self.stats.sweeps_total = self.stats.sweeps_total.saturating_add(1);
        self.stats.sweep_violations = self
            .stats
            .sweep_violations
            .saturating_add(violations.len() as u64);
        for v in &violations {
            if v.code == WxPolicyError::WxViolation.code() {
                self.stats.wx_redlines = self.stats.wx_redlines.saturating_add(1);
            }
        }
        self.push_event(EngineEvent::Sweep {
            seq,
            scanned,
            violations: violations.len() as u16,
        });
        SweepReport {
            scanned,
            violations,
            clean,
        }
    }

    /// 权限污染钻孔（F0316 式故障注入钩子，演练专用）：
    /// 直接把区域权限改写为任意位组（绕过矩阵闸），供 sweep_wx /
    /// health 检出。注入 W^X 组合即计入红线账。
    pub fn corrupt_region_perms_for_drill(
        &mut self,
        id: u32,
        bad: PermRwx,
    ) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        let pos = match self.region_pos(id) {
            None => return Err(WxPolicyError::RegionUnknown),
            Some(p) => p,
        };
        self.regions[pos].perms = bad;
        self.stats.corrupt_drills = self.stats.corrupt_drills.saturating_add(1);
        if bad.is_wx_violation() {
            self.stats.wx_redlines = self.stats.wx_redlines.saturating_add(1);
        }
        let seq = self.next_seq();
        self.push_event(EngineEvent::Corrupt {
            seq,
            region: id,
            bad: bad.bits(),
        });
        Ok(())
    }

    // ── 封账 ─────────────────────────────────────────────────────────────

    /// 封账（冻结一切变更通道：登记/变更/退役/豁免/钻孔；探针与
    /// 快照/健康/审计等只读面继续放行）。
    pub fn seal(&mut self) -> Result<(), WxPolicyError> {
        if self.sealed {
            return Err(WxPolicyError::BadState);
        }
        let seq = self.next_seq();
        self.sealed = true;
        self.batch = None;
        self.push_event(EngineEvent::Seal { seq });
        Ok(())
    }

    /// 是否已封账。
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    // ── 账本只读视图 ─────────────────────────────────────────────────────

    /// 裁决账在场行数。
    #[must_use]
    pub fn decision_len(&self) -> usize {
        self.decisions.count()
    }

    /// 裁决账按序取行（i=0 最旧在场）。
    #[must_use]
    pub fn decision_at(&self, i: usize) -> Option<&DecisionRecord> {
        self.decisions.at(i)
    }

    /// 裁决账最新一行。
    #[must_use]
    pub fn decision_last(&self) -> Option<&DecisionRecord> {
        self.decisions.last()
    }

    /// 变更账在场行数。
    #[must_use]
    pub fn mprotect_len(&self) -> usize {
        self.mprotects.count()
    }

    /// 变更账按序取行。
    #[must_use]
    pub fn mprotect_at(&self, i: usize) -> Option<&MprotectRecord> {
        self.mprotects.at(i)
    }

    /// 事件账在场行数。
    #[must_use]
    pub fn event_len(&self) -> usize {
        self.events.count()
    }

    /// 事件账按序取行。
    #[must_use]
    pub fn event_at(&self, i: usize) -> Option<&EngineEvent> {
        self.events.at(i)
    }

    /// 统计族只读视图。
    #[must_use]
    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }

    /// 全局序号。
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.seq
    }

    // ── 指纹 / 快照 / 健康 / 审计 ────────────────────────────────────────

    /// 状态指纹（FNV-1a 折叠全状态：区域表+豁免表+三账在场行+统计族；
    /// 同状态同指纹——双跑对拍以完整状态为口径，比单事件流更严）。
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_OFFSET_64;
        h = EngineEvent::fnv_mix(h, self.seq);
        h = EngineEvent::fnv_mix(h, MODULE_ID as u64);
        // 区域表折叠。
        let mut i: usize = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            h = EngineEvent::fnv_mix(h, r.id as u64);
            h = EngineEvent::fnv_mix(h, r.kind.index() as u64);
            h = EngineEvent::fnv_mix(h, r.base);
            h = EngineEvent::fnv_mix(h, r.len);
            h = EngineEvent::fnv_mix(h, r.perms.bits() as u64);
            h = EngineEvent::fnv_mix(h, match r.state {
                RegionState::Active => 1,
                RegionState::Retired => 2,
            });
            i += 1;
        }
        // 豁免表折叠。
        let mut j: usize = 0;
        while j < self.exempts.len() {
            let e = self.exempts[j];
            h = EngineEvent::fnv_mix(h, e.phys);
            h = EngineEvent::fnv_mix(h, e.owner);
            h = EngineEvent::fnv_mix(h, e.w_alias.bits() as u64);
            h = EngineEvent::fnv_mix(h, e.x_alias.bits() as u64);
            h = EngineEvent::fnv_mix(h, e.grant_seq);
            j += 1;
        }
        // 裁决账折叠。
        let mut d: usize = 0;
        while d < self.decisions.count() {
            if let Some(rec) = self.decisions.at(d) {
                h = EngineEvent::fnv_mix(h, rec.seq);
                h = EngineEvent::fnv_mix(h, rec.kind.index() as u64);
                h = EngineEvent::fnv_mix(h, rec.req.bits() as u64);
                h = EngineEvent::fnv_mix(h, rec.granted as u64);
                h = EngineEvent::fnv_mix(h, rec.code as u64);
            }
            d += 1;
        }
        // 变更账折叠。
        let mut m: usize = 0;
        while m < self.mprotects.count() {
            if let Some(rec) = self.mprotects.at(m) {
                h = EngineEvent::fnv_mix(h, rec.seq ^ 0x4d50_5254_4f43_5458);
                h = EngineEvent::fnv_mix(h, rec.region as u64);
                h = EngineEvent::fnv_mix(h, rec.old.bits() as u64);
                h = EngineEvent::fnv_mix(h, rec.new.bits() as u64);
                h = EngineEvent::fnv_mix(h, rec.granted as u64);
                h = EngineEvent::fnv_mix(h, rec.code as u64);
            }
            m += 1;
        }
        // 事件账折叠（digest 已含判别 tag）。
        let mut e: usize = 0;
        while e < self.events.count() {
            if let Some(ev) = self.events.at(e) {
                h = EngineEvent::fnv_mix(h, ev.digest());
            }
            e += 1;
        }
        // 统计族折叠（计数器逐个混合；数组分元素混合）。
        h = EngineEvent::fnv_mix(h, self.stats.regions_registered);
        h = EngineEvent::fnv_mix(h, self.stats.regions_retired);
        h = EngineEvent::fnv_mix(h, self.stats.decisions_total);
        h = EngineEvent::fnv_mix(h, self.stats.decisions_pass);
        h = EngineEvent::fnv_mix(h, self.stats.decisions_reject);
        h = EngineEvent::fnv_mix(h, self.stats.decision_evictions);
        let mut k: usize = 0;
        while k < RegionType::COUNT {
            h = EngineEvent::fnv_mix(h, self.stats.per_kind_total[k]);
            h = EngineEvent::fnv_mix(h, self.stats.per_kind_reject[k]);
            k += 1;
        }
        h = EngineEvent::fnv_mix(h, self.stats.mprotect_total);
        h = EngineEvent::fnv_mix(h, self.stats.mprotect_ok);
        h = EngineEvent::fnv_mix(h, self.stats.mprotect_upgrade_rejected);
        h = EngineEvent::fnv_mix(h, self.stats.wx_redlines);
        h = EngineEvent::fnv_mix(h, self.stats.exempt_granted);
        h = EngineEvent::fnv_mix(h, self.stats.exempt_revoked);
        h = EngineEvent::fnv_mix(h, self.stats.dualmap_checks);
        h = EngineEvent::fnv_mix(h, self.stats.dualmap_passed);
        h = EngineEvent::fnv_mix(h, self.stats.probes_total);
        h = EngineEvent::fnv_mix(h, self.stats.probe_faults);
        h = EngineEvent::fnv_mix(h, self.stats.sweeps_total);
        h = EngineEvent::fnv_mix(h, self.stats.sweep_violations);
        h = EngineEvent::fnv_mix(h, self.stats.corrupt_drills);
        h = EngineEvent::fnv_mix(h, self.stats.drills_run);
        h = EngineEvent::fnv_mix(h, self.stats.event_evictions);
        h
    }

    /// 引擎快照（状态级对拍口径）。
    #[must_use]
    pub fn snapshot(&self) -> EngineSnapshot {
        let active = self
            .regions
            .iter()
            .filter(|r| r.state == RegionState::Active)
            .count();
        EngineSnapshot {
            seq: self.seq,
            fingerprint: self.fingerprint(),
            regions_total: self.regions.len(),
            regions_active: active,
            decisions_len: self.decisions.count(),
            mprotects_len: self.mprotects.count(),
            events_len: self.events.count(),
            exempt_active: self.exempts.len(),
            stats: self.stats.clone(),
        }
    }

    /// 健康报告（豁免默认 0 红线在此结算；矩阵运行期纵深复核）。
    #[must_use]
    pub fn health(&self) -> HealthReport {
        // 纵深复核矩阵（const 已断言；此处运行期再扫，防二进制被
        // 补丁手改的极端面）。
        let mut matrix_clean = true;
        let mut i: usize = 0;
        while i < RegionType::COUNT {
            if RWX_MATRIX[i][1] == 1 && RWX_MATRIX[i][2] == 1 {
                matrix_clean = false;
            }
            i += 1;
        }
        let exempt_active = self.stats.exempt_active();
        let exempt_zero_default = exempt_active == 0;
        let verdict_ok = matrix_clean && exempt_zero_default;
        let mut notes: Vec<String> = Vec::new();
        if !exempt_zero_default {
            notes.push(format!(
                "豁免非零（{} 条活跃）：W^X 默认 0 红线处于开口期，\
                 须逐条审计 owner 溯源并限时撤销。",
                exempt_active
            ));
        }
        if self.stats.reject_rate_bp() > 5_000 {
            notes.push(format!(
                "总拒收率 {}bp 超 5000bp：检查请求方位组装与矩阵口径\
                 是否脱节。",
                self.stats.reject_rate_bp()
            ));
        }
        if self.stats.decision_evictions > 0 {
            notes.push(format!(
                "裁决账满环驱逐 {} 次：细查需提前 snapshot 留档。",
                self.stats.decision_evictions
            ));
        }
        if self.stats.wx_redlines > 0 {
            notes.push(format!(
                "W^X 红线累计立案 {} 次（含演练注入属正常留痕）：\
                 生产路径立案须逐条溯源。",
                self.stats.wx_redlines
            ));
        }
        HealthReport {
            seq: self.seq,
            fingerprint: self.fingerprint(),
            matrix_clean,
            exempt_active,
            exempt_zero_default,
            reject_rate_bp: self.stats.reject_rate_bp(),
            decisions_total: self.stats.decisions_total,
            wx_redlines: self.stats.wx_redlines,
            sealed: self.sealed,
            verdict_ok,
            notes,
        }
    }

    /// 一段式审计报告（矩阵/区域/账本/比率/豁免/健康可读转储）。
    #[must_use]
    pub fn audit_report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "== {} 权限位策略引擎审计（模块 0x{:04X}，诊断段 {:#06x}~{:#06x}）==\n",
            TASK_TAG,
            MODULE_ID,
            SEG_BASE,
            SEG_BASE + 0x0F
        ));
        s.push_str("-- 8×3 RWX 冻结矩阵（行=类型 [R,W,X]）--\n");
        let all = RegionType::all();
        let mut i: usize = 0;
        while i < all.len() {
            let row = all[i].row_mask();
            s.push_str(&format!(
                "  [{}] {:<8} {} {}\n",
                i,
                row.mnem(),
                all[i].zh(),
                if row.is_wx_violation() { "!!W^X!!" } else { "" }
            ));
            i += 1;
        }
        s.push_str(&format!(
            "双源对拍失配行数：{}\n",
            verify_matrix_dual_source()
        ));
        s.push_str("-- 区域登记 --\n");
        let mut a: usize = 0;
        while a < self.regions.len() {
            let r = self.regions[a];
            let state_zh = if r.state == RegionState::Active {
                "活跃"
            } else {
                "退役"
            };
            s.push_str(&format!(
                "  #{} {} [{:#x}+{:#x}] {} ({}) 登记序号 {}\n",
                r.id,
                r.kind.zh(),
                r.base,
                r.len,
                r.perms.mnem(),
                state_zh,
                r.born_seq
            ));
            a += 1;
        }
        s.push_str(&format!(
            "-- 裁决账 --在场 {} 行（驱逐 {} 次）总 {} 过 {} 拒 {} 率 {}bp\n",
            self.decisions.count(),
            self.stats.decision_evictions,
            self.stats.decisions_total,
            self.stats.decisions_pass,
            self.stats.decisions_reject,
            self.stats.reject_rate_bp()
        ));
        let mut ki: usize = 0;
        while ki < RegionType::COUNT {
            let t = self.stats.per_kind_total[ki];
            if t > 0 {
                let kind = match RegionType::from_index(ki as u8) {
                    Some(k) => k,
                    None => continue,
                };
                s.push_str(&format!(
                    "    {} 总 {} 拒 {} 率 {}bp\n",
                    kind.zh(),
                    t,
                    self.stats.per_kind_reject[ki],
                    self.stats.kind_reject_rate_bp(kind)
                ));
            }
            ki += 1;
        }
        s.push_str(&format!(
            "-- 变更账 --在场 {} 行：请求 {} 成功 {} 升权拒 {}\n",
            self.mprotects.count(),
            self.stats.mprotect_total,
            self.stats.mprotect_ok,
            self.stats.mprotect_upgrade_rejected
        ));
        s.push_str(&format!(
            "-- 豁免账 --活跃 {} 条（登记 {} 撤销 {}）：\n",
            self.exempts.len(),
            self.stats.exempt_granted,
            self.stats.exempt_revoked
        ));
        let mut x: usize = 0;
        while x < self.exempts.len() {
            let e = self.exempts[x];
            s.push_str(&format!(
                "    物理页 {:#x} owner {:#x} W别名 {} X别名 {}\n",
                e.phys,
                e.owner,
                e.w_alias.mnem(),
                e.x_alias.mnem()
            ));
            x += 1;
        }
        let h = self.health();
        s.push_str(&format!(
            "-- 健康 -- 矩阵净={} 豁免零基态={} 拒收率={}bp W^X立案={} 封账={} \
             判定={} 指纹={:#018x}\n",
            h.matrix_clean,
            h.exempt_zero_default,
            h.reject_rate_bp,
            h.wx_redlines,
            h.sealed,
            if h.verdict_ok { "健康" } else { "开口待审" },
            h.fingerprint
        ));
        for n in &h.notes {
            s.push_str(&format!("    注：{}\n", n));
        }
        s
    }

    // ── 三注入演练面（W^X 违例/越权升级/未登记豁免各检出）────────────────

    /// 三注入演练（判据"注入面"三案）：
    ///   1. W^X 违例注入：8 类型 × {WX, RWX} 共 16 试次，全部应拒于
    ///      WxViolation（100% 检出）；
    ///   2. 越权升级注入：登记一块 KernelData 演练区，降权到 R 后尝试
    ///      升回 RW，应拒于 MprotectUpgrade；
    ///   3. 未登记豁免注入：对无豁免在案的演练物理页做 W+X 双映射
    ///      检查，应拒于 ExemptUnregistered。
    /// 演练区/演练页用保留演练地址带（0xDE00_0000_0000 起），与生产
    /// 登记带隔离；演练自身也全留痕（决策/事件账）。
    pub fn drill_injections(&mut self) -> DrillReport {
        const DRILL_BASE: u64 = 0xDE00_0000_0000;
        const DRILL_PAGE: u64 = 0x0000_5D00_0000_0000;
        self.stats.drills_run = self.stats.drills_run.saturating_add(1);

        // 案 1：W^X 违例注入（16 试次全检出红线）。
        let mut wx_attempts: u32 = 0;
        let mut wx_detected: u32 = 0;
        let mut wx_code: u16 = E_OK;
        let bad_combos = [PermRwx::WX, PermRwx::RWX];
        let mut bi: usize = 0;
        while bi < bad_combos.len() {
            let mut ti: usize = 0;
            let all = RegionType::all();
            while ti < all.len() {
                wx_attempts += 1;
                let kind = all[ti];
                let detected = match self.adjudicate(kind, bad_combos[bi]) {
                    Err(e) => {
                        let det = e == WxPolicyError::WxViolation;
                        if det {
                            wx_detected += 1;
                        }
                        wx_code = e.code();
                        det
                    }
                    Ok(_) => {
                        wx_code = E_OK; // 过闸=红线漏检（测试断言面）
                        false
                    }
                };
                let seq = self.next_seq();
                self.push_event(EngineEvent::Drill {
                    seq,
                    kind: 1,
                    detected,
                    code: wx_code,
                });
                ti += 1;
            }
            bi += 1;
        }

        // 案 2：越权升级注入。
        let mut upgrade_detected = false;
        let mut upgrade_code: u16 = E_OK;
        let reg = self.register_region(
            RegionType::KernelData,
            DRILL_BASE,
            PAGE_4K * 2,
            PermRwx::RW,
        );
        match reg {
            Ok(id) => {
                if self.mprotect(id, PermRwx::R).is_ok() {
                    match self.mprotect(id, PermRwx::RW) {
                        Err(e) => {
                            upgrade_detected = e == WxPolicyError::MprotectUpgrade;
                            upgrade_code = e.code();
                        }
                        Ok(_) => {
                            upgrade_detected = false;
                            upgrade_code = E_OK;
                        }
                    }
                }
                let _ = self.retire_region(id);
            }
            Err(e) => {
                upgrade_code = e.code();
            }
        }
        let seq2 = self.next_seq();
        self.push_event(EngineEvent::Drill {
            seq: seq2,
            kind: 2,
            detected: upgrade_detected,
            code: upgrade_code,
        });

        // 案 3：未登记豁免注入。
        let mut dualmap_detected = false;
        let mut dualmap_code: u16 = E_OK;
        match self.check_dual_map(DRILL_PAGE, PermRwx::W, PermRwx::X) {
            Err(e) => {
                dualmap_detected = e == WxPolicyError::ExemptUnregistered;
                dualmap_code = e.code();
            }
            Ok(()) => {}
        }
        let seq3 = self.next_seq();
        self.push_event(EngineEvent::Drill {
            seq: seq3,
            kind: 3,
            detected: dualmap_detected,
            code: dualmap_code,
        });

        let all_detected =
            wx_detected == wx_attempts && upgrade_detected && dualmap_detected;
        DrillReport {
            wx_attempts,
            wx_detected,
            wx_code,
            upgrade_detected,
            upgrade_code,
            dualmap_detected,
            dualmap_code,
            all_detected,
        }
    }
}

/// #PF 位合成助手：写访问置 W/R 位；取指置 I/D 位（读不置）。
const fn pf_wr_op(op: AccessOp) -> u32 {
    match op {
        AccessOp::Write => PF_WR,
        _ => 0,
    }
}

/// #PF 位合成助手：取指置 I/D 位。
const fn pf_id_op(op: AccessOp) -> u32 {
    match op {
        AccessOp::Exec => PF_ID,
        _ => 0,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §12 · 快照对拍 + 确定性剧本 + 双跑对拍（自由函数面）
// ═══════════════════════════════════════════════════════════════════════════

/// 快照对拍（字段级差异清单；空=两快照状态一致）。
#[must_use]
pub fn diff_snapshots(a: &EngineSnapshot, b: &EngineSnapshot) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if a.seq != b.seq {
        out.push(format!("seq: {} != {}", a.seq, b.seq));
    }
    if a.fingerprint != b.fingerprint {
        out.push(format!(
            "fingerprint: {:#018x} != {:#018x}",
            a.fingerprint, b.fingerprint
        ));
    }
    if a.regions_total != b.regions_total {
        out.push(format!(
            "regions_total: {} != {}",
            a.regions_total, b.regions_total
        ));
    }
    if a.regions_active != b.regions_active {
        out.push(format!(
            "regions_active: {} != {}",
            a.regions_active, b.regions_active
        ));
    }
    if a.decisions_len != b.decisions_len {
        out.push(format!(
            "decisions_len: {} != {}",
            a.decisions_len, b.decisions_len
        ));
    }
    if a.mprotects_len != b.mprotects_len {
        out.push(format!(
            "mprotects_len: {} != {}",
            a.mprotects_len, b.mprotects_len
        ));
    }
    if a.events_len != b.events_len {
        out.push(format!("events_len: {} != {}", a.events_len, b.events_len));
    }
    if a.exempt_active != b.exempt_active {
        out.push(format!(
            "exempt_active: {} != {}",
            a.exempt_active, b.exempt_active
        ));
    }
    if a.stats.decisions_total != b.stats.decisions_total {
        out.push(format!(
            "stats.decisions_total: {} != {}",
            a.stats.decisions_total, b.stats.decisions_total
        ));
    }
    if a.stats.decisions_reject != b.stats.decisions_reject {
        out.push(format!(
            "stats.decisions_reject: {} != {}",
            a.stats.decisions_reject, b.stats.decisions_reject
        ));
    }
    if a.stats.wx_redlines != b.stats.wx_redlines {
        out.push(format!(
            "stats.wx_redlines: {} != {}",
            a.stats.wx_redlines, b.stats.wx_redlines
        ));
    }
    if a.stats.mprotect_total != b.stats.mprotect_total {
        out.push(format!(
            "stats.mprotect_total: {} != {}",
            a.stats.mprotect_total, b.stats.mprotect_total
        ));
    }
    if a.stats.probe_faults != b.stats.probe_faults {
        out.push(format!(
            "stats.probe_faults: {} != {}",
            a.stats.probe_faults, b.stats.probe_faults
        ));
    }
    out
}

/// 确定性剧本（双跑对拍的操作序列；种子驱动，无时钟无随机源）。
///
/// 剧本带：0x0011_0000_0000 起为生产演练带（固定三区 +
/// LCG 驱动 16 区），0xDE00_0000_0000 为 drill_injections 演练带，
/// 0x0000_5D00_0000_0000 为豁免演练物理页——三带互不重叠。
#[must_use]
pub fn scripted_scenario(engine: &mut WxPolicyEngine, seed: u64) -> u64 {
    const BAND: u64 = 0x0011_0000_0000;
    // 固定三区（保证后续句柄确定：0/1/2）。
    let r0 = engine.register_region(RegionType::KernelData, BAND, PAGE_4K * 4, PermRwx::RW);
    let _r1 = engine.register_region(
        RegionType::KernelCode,
        BAND + 0x10_0000,
        PAGE_4K * 8,
        PermRwx::RX,
    );
    let _r2 = engine.register_region(
        RegionType::KernelRoData,
        BAND + 0x20_0000,
        PAGE_4K * 2,
        PermRwx::R,
    );
    // 变更路径：降权自由、升权拒。
    if let Ok(id) = r0 {
        let _ = engine.mprotect(id, PermRwx::R);
        let _ = engine.mprotect(id, PermRwx::RW); // 升权→拒（留痕）
    }
    // LCG 驱动 16 区（每第 4 区故意请求超集/违例组合，拒收留痕）。
    let mut lcg = DetLcg::new(seed ^ 0x5758_4d41_5452_4958);
    let mut i: u64 = 0;
    while i < 16 {
        let idx = lcg.next_below(8) as u8;
        let kind = match RegionType::try_from_index(idx) {
            Ok(k) => k,
            Err(_) => RegionType::KernelData,
        };
        let row = kind.row_mask();
        let req = if i % 4 == 3 {
            // 违例组合轮换：WX / RWX 交替（拒收留痕，determinstic）。
            if (i / 4) % 2 == 0 {
                PermRwx::WX
            } else {
                PermRwx::RWX
            }
        } else if i % 4 == 1 {
            row.union(PermRwx::R) // 行∪R=行（恒合法，走子集全过）
        } else {
            PermRwx(row.bits() & (lcg.next_below(8) as u8))
        };
        let base = BAND + 0x100_0000 + i * 0x2_0000;
        let len = PAGE_4K * (1 + lcg.next_below(4));
        let reg = engine.register_region(kind, base, len, req);
        if let Ok(id) = reg {
            if i % 4 == 0 {
                // 降权演练（R 剥离或全收）。
                let _ = engine.mprotect(id, PermRwx::R.intersect(row));
            }
        }
        i += 1;
    }
    // 探针三发：数据区写（过）/数据区取指（故障 I-D）/野地址（故障 P=0）。
    let _p1 = engine.probe(BAND + 0x800, AccessOp::Write);
    let _p2 = engine.probe(BAND + 0x800, AccessOp::Exec);
    let _p3 = engine.probe(0x99AB_0000_0000_0000, AccessOp::Read);
    // 豁免通道演练：登记→双映射检查过→撤销→再查（未登记红案）。
    const EX_PHYS: u64 = 0x0000_5D00_0000_0000;
    let _ = engine.register_exempt(EX_PHYS, 0xC0DE_0001, PermRwx::RW, PermRwx::RX);
    let _ = engine.check_dual_map(EX_PHYS, PermRwx::W, PermRwx::X);
    let _ = engine.revoke_exempt(EX_PHYS);
    let _ = engine.check_dual_map(EX_PHYS, PermRwx::W, PermRwx::X); // 红案
    // 批量事务演练：两笔降权 + 一笔对不存在句柄（整批回滚）。
    if r0.is_ok() {
        let _ = engine.batch_begin();
        let _ = engine.batch_stage(r0.unwrap_or(u32::MAX), PermRwx::NONE);
        let _ = engine.batch_stage(0xF7F7_F7F7, PermRwx::NONE); // 不存在句柄
        let _ = engine.batch_apply(); // 失败→回滚
    }
    // 三注入 + 扫描 + 污染钻孔一轮。
    let _drill = engine.drill_injections();
    let sweep = engine.sweep_wx();
    if !sweep.clean {
        // 剧本内不应有存量污染（未钻孔）；此分支只为确定性收口。
        let _ = engine.sweep_wx();
    }
    if let Ok(id) = r0 {
        if engine.region(id).is_some() {
            let _ = engine.corrupt_region_perms_for_drill(id, PermRwx::RWX);
            let _ = engine.sweep_wx(); // 检出污染并立案
        }
    }
    engine.fingerprint()
}

/// 双跑对拍（指纹级 + 快照级双口径；判据"双跑确定性"）。
#[must_use]
pub fn double_run_check(seed: u64) -> DoubleRunReport {
    let mut a = WxPolicyEngine::new();
    let fp_a = scripted_scenario(&mut a, seed);
    let mut b = WxPolicyEngine::new();
    let fp_b = scripted_scenario(&mut b, seed);
    let sa = a.snapshot();
    let sb = b.snapshot();
    let diff = diff_snapshots(&sa, &sb);
    let mut head: Vec<String> = Vec::new();
    let mut hi: usize = 0;
    while hi < diff.len() && hi < 8 {
        head.push(diff[hi].clone());
        hi += 1;
    }
    let fingerprints_equal = fp_a == fp_b;
    let snapshot_diff_count = diff.len() as u32;
    DoubleRunReport {
        seed,
        fp_a,
        fp_b,
        fingerprints_equal,
        snapshot_diff_count,
        snapshot_diff_head: head,
        consistent: fingerprints_equal && diff.is_empty(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §13 · 测试（#[cfg(test)] 体不属内核路径；隔离探针 vx1024b_probe_unx0306b
//       以 #[path] 挂载本文件，cargo test 全绿为验收口径）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── 判据 J1(a)：区域类型→RWX 矩阵全过 ────────────────────────────────

    /// 全 8 类型：行全集请求、空集请求、行内每个单/双位子集全过闸。
    #[test]
    fn test_matrix_all_types_pass() {
        let mut e = WxPolicyEngine::new();
        for kind in RegionType::all() {
            let row = kind.row_mask();
            // 行全集过。
            assert_eq!(e.adjudicate(kind, row), Ok(row), "{} 行全集应过", kind.zh());
            // 空集（哨兵页）过。
            assert_eq!(e.adjudicate(kind, PermRwx::NONE), Ok(PermRwx::NONE));
            // 8 种组合里凡行子集者全过。
            for c in PermRwx::all_combos() {
                if c.is_subset_of(row) && !c.is_wx_violation() {
                    assert_eq!(
                        e.adjudicate(kind, c),
                        Ok(c),
                        "{} 子集 {:?} 应过",
                        kind.zh(),
                        c
                    );
                }
            }
        }
        assert_eq!(e.stats().decisions_reject, 0, "矩阵全过轮不应有拒收");
    }

    /// 判据 J1(b) 双保险之一（编译期已断言；此处运行期复核）：
    /// 任何类型矩阵行本身不含 W∧X。
    #[test]
    fn test_matrix_rows_never_wx() {
        for kind in RegionType::all() {
            assert!(
                !kind.row_mask().is_wx_violation(),
                "{} 矩阵行含 W^X",
                kind.zh()
            );
        }
        // 形状自锁常量复核。
        assert_eq!(MATRIX_R_SUM, 8);
        assert_eq!(MATRIX_W_SUM, 5);
        assert_eq!(MATRIX_X_SUM, 2);
        // 双源对拍零失配。
        assert_eq!(verify_matrix_dual_source(), 0);
    }

    // ── 判据 J1(b)：W^X 违例 100% 检出 ───────────────────────────────────

    /// 8 类型 × {WX, RWX} = 16 试次全部红案（码恒 WxViolation），
    /// 且 wx_redlines 同步计数。
    #[test]
    fn test_wx_violation_100pct() {
        let mut e = WxPolicyEngine::new();
        let bad = [PermRwx::WX, PermRwx::RWX];
        let mut attempts = 0u32;
        for kind in RegionType::all() {
            for b in bad {
                attempts += 1;
                match e.adjudicate(kind, b) {
                    Err(err) => assert_eq!(
                        err,
                        WxPolicyError::WxViolation,
                        "{} {:?} 应红案 WxViolation",
                        kind.zh(),
                        b
                    ),
                    Ok(_) => panic!("{} {:?} 竟过闸", kind.zh(), b),
                }
            }
        }
        assert_eq!(attempts, 16);
        assert_eq!(e.stats().wx_redlines, 16, "红线账应计 16 次");
        assert_eq!(e.stats().decisions_reject, 16);
        // 登记路径同样红案。
        let reg = e.register_region(
            RegionType::TaskHeap,
            0x0020_0000,
            PAGE_4K,
            PermRwx::RWX,
        );
        assert_eq!(reg, Err(WxPolicyError::WxViolation));
        assert_eq!(e.stats().wx_redlines, 17);
        // 每次红案都留痕于裁决账（最新一行码核对）。
        let last = e.decision_last().cloned();
        assert!(last.is_some());
        let last = last.unwrap_or(DecisionRecord {
            seq: 0,
            kind: RegionType::KernelCode,
            req: PermRwx::NONE,
            granted: true,
            code: E_OK,
        });
        assert_eq!(last.code, WxPolicyError::WxViolation.code());
        assert!(!last.granted);
    }

    /// 非法位（超三位掩码）检出 + PTE 解码坏口径检出。
    #[test]
    fn test_illegal_bits_and_pte() {
        let mut e = WxPolicyEngine::new();
        let bad = PermRwx(0b1000);
        assert_eq!(
            e.adjudicate(RegionType::KernelData, bad),
            Err(WxPolicyError::IllegalPermBits)
        );
        // PTE 编解码往返：合法权（非 W^X）round-trip。
        for c in PermRwx::all_combos() {
            if c.is_wx_violation() || !c.is_legal() {
                continue;
            }
            let flags = c.to_pte_flags();
            let back = PermRwx::from_pte_flags(flags);
            assert!(back.is_ok(), "{:?} PTE 往返失败", c);
            let back = back.unwrap_or(PermRwx::NONE);
            // 解码口径 R 位恒置（P=1 即可读）：期待折算 = c ∪ R。
            assert_eq!(back, c.union(PermRwx::R), "{:?} PTE 往返漂移", c);
        }
        // W^X 的 PTE 编码（RW=1 且 NX=0）在解码口红案。
        let bad_pte = PTE_P | PTE_RW; // 可写且无 NX=可执行
        assert_eq!(
            PermRwx::from_pte_flags(bad_pte),
            Err(WxPolicyError::WxViolation)
        );
        // 无 P 位→IllegalPermBits。
        assert_eq!(
            PermRwx::from_pte_flags(PTE_RW | PTE_NX),
            Err(WxPolicyError::IllegalPermBits)
        );
        assert_eq!(PermRwx::from_pte_flags(0), Err(WxPolicyError::IllegalPermBits));
    }

    // ── 变更路径（mprotect：降权自由升权拒）──────────────────────────────

    #[test]
    fn test_mprotect_path() {
        let mut e = WxPolicyEngine::new();
        let id = e
            .register_region(RegionType::KernelData, 0x0010_0000, PAGE_4K * 2, PermRwx::RW)
            .unwrap_or(u32::MAX);
        assert_ne!(id, u32::MAX);
        // 等权变更（no-op）放行。
        assert_eq!(e.mprotect(id, PermRwx::RW), Ok(PermRwx::RW));
        // 降权 RW→R 放行。
        assert_eq!(e.mprotect(id, PermRwx::R), Ok(PermRwx::RW));
        assert_eq!(
            e.region(id).unwrap_or(&tests::PLACEHOLDER).perms,
            PermRwx::R
        );
        // 升权 R→RW 拒（MprotectUpgrade）。
        assert_eq!(
            e.mprotect(id, PermRwx::RW),
            Err(WxPolicyError::MprotectUpgrade)
        );
        // 权限不变仍为 R。
        assert_eq!(
            e.region(id).unwrap_or(&tests::PLACEHOLDER).perms,
            PermRwx::R
        );
        // 升权到 W（新增 W 位）→ 升权拒。
        assert_eq!(
            e.mprotect(id, PermRwx::W),
            Err(WxPolicyError::MprotectUpgrade)
        );
        // 降权到空集（守护页化）放行。
        assert_eq!(e.mprotect(id, PermRwx::NONE), Ok(PermRwx::R));
        // 变更账五行留痕（两过一等权、两升权拒、一守护页化）。
        assert_eq!(e.mprotect_len(), 5);
        let m1 = e.mprotect_at(3).cloned().unwrap_or(MprotectRecord {
            seq: 0,
            region: 0,
            old: PermRwx::NONE,
            new: PermRwx::NONE,
            granted: true,
            code: E_OK,
        });
        assert!(!m1.granted);
        assert_eq!(m1.code, WxPolicyError::MprotectUpgrade.code());
        // 新权限若超类型行（RoData 想要 W）在矩阵闸即拒。
        let rid = e
            .register_region(RegionType::KernelRoData, 0x0030_0000, PAGE_4K, PermRwx::R)
            .unwrap_or(u32::MAX);
        assert_eq!(
            e.mprotect(rid, PermRwx::RW),
            Err(WxPolicyError::PermSuperset)
        );
        // 未知句柄/已退役区。
        assert_eq!(
            e.mprotect(0xFEFE_FEFE, PermRwx::R),
            Err(WxPolicyError::RegionUnknown)
        );
        assert!(e.retire_region(id).is_ok());
        assert_eq!(
            e.mprotect(id, PermRwx::NONE),
            Err(WxPolicyError::BadState)
        );
    }

    /// 批量变更事务：失败整批回滚。
    #[test]
    fn test_batch_transaction_rollback() {
        let mut e = WxPolicyEngine::new();
        let a = e
            .register_region(RegionType::TaskHeap, 0x0040_0000, PAGE_4K, PermRwx::RW)
            .unwrap_or(u32::MAX);
        let b = e
            .register_region(RegionType::TaskStack, 0x0050_0000, PAGE_4K, PermRwx::RW)
            .unwrap_or(u32::MAX);
        assert!(e.batch_begin().is_ok());
        assert!(e.batch_stage(a, PermRwx::R).is_ok());
        assert!(e.batch_stage(0xE0E0_E0E0, PermRwx::NONE).is_ok()); // 坏句柄
        let applied = e.batch_apply();
        assert!(applied.is_err(), "坏句柄应致整批失败");
        // 回滚后 a 区权限恢复 RW。
        assert_eq!(
            e.region(a).unwrap_or(&tests::PLACEHOLDER).perms,
            PermRwx::RW,
            "回滚须恢复旧权"
        );
        assert_eq!(
            e.region(b).unwrap_or(&tests::PLACEHOLDER).perms,
            PermRwx::RW
        );
        // 全好批正常应用。
        assert!(e.batch_begin().is_ok());
        assert!(e.batch_stage(a, PermRwx::R).is_ok());
        assert!(e.batch_stage(b, PermRwx::R).is_ok());
        assert_eq!(e.batch_apply().ok(), Some(2));
        assert_eq!(
            e.region(a).unwrap_or(&tests::PLACEHOLDER).perms,
            PermRwx::R
        );
        // 未开批即 apply 红案。
        assert_eq!(e.batch_apply(), Err(WxPolicyError::BadState));
    }

    // ── 例外通道（显式双映射豁免；默认 0 红线）───────────────────────────

    #[test]
    fn test_exempt_channel() {
        let mut e = WxPolicyEngine::new();
        // 基态：豁免默认 0，health 红线口径健康。
        let h0 = e.health();
        assert_eq!(h0.exempt_active, 0);
        assert!(h0.exempt_zero_default);
        assert!(h0.verdict_ok);
        // 未登记豁免的双映射 → 红案。
        assert_eq!(
            e.check_dual_map(0x7000, PermRwx::W, PermRwx::X),
            Err(WxPolicyError::ExemptUnregistered)
        );
        // 登记豁免（W 别名 RW、X 别名 RX）→ 放行。
        assert!(e.register_exempt(0x7000, 0xABCD, PermRwx::RW, PermRwx::RX).is_ok());
        assert!(e.check_dual_map(0x7000, PermRwx::W, PermRwx::X).is_ok());
        let h1 = e.health();
        assert_eq!(h1.exempt_active, 1);
        assert!(!h1.exempt_zero_default, "开口期 health 应转待审");
        assert!(!h1.verdict_ok);
        assert_eq!(h1.notes.len(), 1, "豁免非零应有附注");
        // 重复登记拒。
        assert_eq!(
            e.register_exempt(0x7000, 0xFFFF, PermRwx::W, PermRwx::X),
            Err(WxPolicyError::ExemptDuplicate)
        );
        // 别名形状脏：W 别名不含 W / 含 X；X 别名含 W。
        assert_eq!(
            e.register_exempt(0x8000, 1, PermRwx::R, PermRwx::X),
            Err(WxPolicyError::ExemptAliasDirty)
        );
        assert_eq!(
            e.register_exempt(0x8000, 1, PermRwx::RW, PermRwx::R),
            Err(WxPolicyError::ExemptAliasDirty)
        );
        assert_eq!(
            e.register_exempt(0x8000, 1, PermRwx::RW, PermRwx::RW),
            Err(WxPolicyError::ExemptAliasDirty)
        );
        // 单别名 W^X → 红线优先于形状脏。
        assert_eq!(
            e.register_exempt(0x8000, 1, PermRwx::RWX, PermRwx::X),
            Err(WxPolicyError::WxViolation)
        );
        assert_eq!(
            e.register_exempt(0x8000, 1, PermRwx::W, PermRwx::RWX),
            Err(WxPolicyError::WxViolation)
        );
        // 物理页未 4K 对齐。
        assert_eq!(
            e.register_exempt(0x7001, 1, PermRwx::W, PermRwx::X),
            Err(WxPolicyError::RegionShape)
        );
        // 撤销 → 恢复零基态；再撤销红案；撤销后再登记可复用。
        assert!(e.revoke_exempt(0x7000).is_ok());
        assert_eq!(e.revoke_exempt(0x7000), Err(WxPolicyError::ExemptUnknown));
        let h2 = e.health();
        assert_eq!(h2.exempt_active, 0);
        assert!(h2.verdict_ok);
        assert!(e.register_exempt(0x7000, 0xABCE, PermRwx::W, PermRwx::RX).is_ok());
        assert_eq!(e.exempts().len(), 1);
        assert_eq!(e.exempts()[0].owner, 0xABCE, "撤销后重登记留新 owner");
    }

    /// 豁免容量上限。
    #[test]
    fn test_exempt_capacity() {
        let mut e = WxPolicyEngine::new();
        let mut i: u64 = 0;
        while i < MAX_EXEMPTS as u64 {
            let phys = 0x10_0000 + i * PAGE_4K;
            let r = e.register_exempt(phys, i, PermRwx::W, PermRwx::X);
            assert!(r.is_ok(), "第 {} 条豁免应可登记", i);
            i += 1;
        }
        assert_eq!(
            e.register_exempt(0x10_0000 + MAX_EXEMPTS as u64 * PAGE_4K, 9, PermRwx::W, PermRwx::X),
            Err(WxPolicyError::ExemptFull)
        );
        // 撤一条腾位。
        assert!(e.revoke_exempt(0x10_0000).is_ok());
        assert!(e.register_exempt(0x10_0000 + MAX_EXEMPTS as u64 * PAGE_4K, 9, PermRwx::W, PermRwx::X).is_ok());
    }

    // ── 区域形状/重叠/容量 ────────────────────────────────────────────────

    #[test]
    fn test_region_shape_overlap_capacity() {
        let mut e = WxPolicyEngine::new();
        // 零长。
        assert_eq!(
            e.register_region(RegionType::KernelData, 0x1000, 0, PermRwx::RW),
            Err(WxPolicyError::RegionShape)
        );
        // 未对齐 base / len。
        assert_eq!(
            e.register_region(RegionType::KernelData, 0x1001, PAGE_4K, PermRwx::RW),
            Err(WxPolicyError::RegionShape)
        );
        assert_eq!(
            e.register_region(RegionType::KernelData, 0x1000, 0x800, PermRwx::RW),
            Err(WxPolicyError::RegionShape)
        );
        // base+len 溢出。
        assert_eq!(
            e.register_region(RegionType::KernelData, u64::MAX - 0x1000, PAGE_4K * 2, PermRwx::RW),
            Err(WxPolicyError::RegionShape)
        );
        // 合法首区 + 重叠拒。
        assert!(e
            .register_region(RegionType::KernelData, 0x0010_0000, PAGE_4K * 4, PermRwx::RW)
            .is_ok());
        assert_eq!(
            e.register_region(RegionType::TaskHeap, 0x0010_0000 + PAGE_4K * 3, PAGE_4K * 2, PermRwx::RW),
            Err(WxPolicyError::RegionOverlap)
        );
        // 退役后地址可复用。
        assert!(e.retire_region(0).is_ok());
        assert!(e
            .register_region(RegionType::TaskHeap, 0x0010_0000, PAGE_4K * 4, PermRwx::RW)
            .is_ok());
        // 容量：填满 MAX_REGIONS 后 LedgerFull（退役区不腾表位——账迹保留；
        // 此前表内已有 2 条（1 退役 + 1 复用登记），再登记 MAX_REGIONS-2
        // 条恰满 256）。
        let mut i: u64 = 0;
        while i < MAX_REGIONS as u64 - 2 {
            let base = 0x0100_0000_0000 + i * 0x10_000;
            assert!(e.register_region(RegionType::TaskHeap, base, PAGE_4K, PermRwx::RW).is_ok());
            i += 1;
        }
        assert_eq!(
            e.register_region(RegionType::TaskHeap, 0xF000_0000_0000_0000, PAGE_4K, PermRwx::RW),
            Err(WxPolicyError::LedgerFull)
        );
        // 未知句柄退役红案。
        assert_eq!(e.retire_region(0x9999), Err(WxPolicyError::RegionUnknown));
    }

    // ── 访问探针（#PF 错误码合成）────────────────────────────────────────

    #[test]
    fn test_probe_pf_synthesis() {
        let mut e = WxPolicyEngine::new();
        let id = e
            .register_region(RegionType::KernelData, 0x0010_0000, PAGE_4K, PermRwx::RW)
            .unwrap_or(u32::MAX);
        // 数据区写：过。
        let w = e.probe(0x0010_0800, AccessOp::Write);
        assert!(w.allowed);
        assert_eq!(w.pf, 0);
        assert_eq!(w.region, Some(id));
        // 数据区取指：故障 P|I/D（数据页执行 #PF——F0307-J1 同源指印）。
        let x = e.probe(0x0010_0800, AccessOp::Exec);
        assert!(!x.allowed);
        assert_eq!(x.pf, PF_P | PF_ID);
        // 野地址：故障 P=0（页不存在口径）。
        let wild = e.probe(0x7777_0000_0000, AccessOp::Read);
        assert!(!wild.allowed);
        assert_eq!(wild.pf & PF_P, 0);
        assert_eq!(wild.region, None);
        assert_eq!(wild.diag, WxPolicyError::RegionUnknown.code());
        // 只读区写：故障 P|W/R。
        let rid = e
            .register_region(RegionType::KernelRoData, 0x0030_0000, PAGE_4K, PermRwx::R)
            .unwrap_or(u32::MAX);
        let ro_w = e.probe(0x0030_0000, AccessOp::Write);
        assert!(!ro_w.allowed);
        assert_eq!(ro_w.pf, PF_P | PF_WR);
        // 退役区探针：故障（无活跃映射口径）。
        assert!(e.retire_region(rid).is_ok());
        let t = e.probe(0x0030_0000, AccessOp::Read);
        assert!(!t.allowed);
        assert_eq!(t.region, None);
        // 统计对账：5 探针 4 故障。
        assert_eq!(e.stats().probes_total, 5);
        assert_eq!(e.stats().probe_faults, 4);
    }

    // ── 存量扫描 + 污染钻孔（F0316 式检出 100%）─────────────────────────

    #[test]
    fn test_sweep_detects_corruption() {
        let mut e = WxPolicyEngine::new();
        let a = e
            .register_region(RegionType::TaskHeap, 0x0010_0000, PAGE_4K, PermRwx::RW)
            .unwrap_or(u32::MAX);
        let b = e
            .register_region(RegionType::KernelCode, 0x0020_0000, PAGE_4K, PermRwx::RX)
            .unwrap_or(u32::MAX);
        let clean = e.sweep_wx();
        assert!(clean.clean);
        assert_eq!(clean.scanned, 2);
        // 污染 a 为 RWX（W^X 存量违例）。
        assert!(e.corrupt_region_perms_for_drill(a, PermRwx::RWX).is_ok());
        // 污染 b 为 RW（代码段超集污染）。
        assert!(e.corrupt_region_perms_for_drill(b, PermRwx::RW).is_ok());
        let bad = e.sweep_wx();
        assert!(!bad.clean);
        assert_eq!(bad.violations.len(), 2);
        assert_eq!(bad.violations[0].code, WxPolicyError::WxViolation.code());
        assert_eq!(bad.violations[1].code, WxPolicyError::PermSuperset.code());
        // 红线账：RWX 注入 1（钻孔）+ 扫描立案 1 = 2。
        assert_eq!(e.stats().wx_redlines, 2);
        assert_eq!(e.stats().sweep_violations, 2);
        // 降权修复合规（RWX→RW 是降权方向，恒可过闸）。
        assert!(e.mprotect(a, PermRwx::RW).is_ok());
        // b 清污：RW→RX 含新增 X → 升权拒（变更路径只降不升），
        // 代码段污染只能退役。
        assert_eq!(
            e.mprotect(b, PermRwx::RX),
            Err(WxPolicyError::MprotectUpgrade)
        );
        let _ = e.retire_region(b);
        let re = e.sweep_wx();
        assert!(re.clean, "修复合规后扫描应复净");
    }

    // ── 三注入演练 ────────────────────────────────────────────────────────

    #[test]
    fn test_drill_injections() {
        let mut e = WxPolicyEngine::new();
        let d = e.drill_injections();
        assert_eq!(d.wx_attempts, 16);
        assert_eq!(d.wx_detected, 16, "W^X 注入 100% 检出");
        assert_eq!(d.wx_code, WxPolicyError::WxViolation.code());
        assert!(d.upgrade_detected, "升权注入检出");
        assert_eq!(d.upgrade_code, WxPolicyError::MprotectUpgrade.code());
        assert!(d.dualmap_detected, "未登记豁免注入检出");
        assert_eq!(d.dualmap_code, WxPolicyError::ExemptUnregistered.code());
        assert!(d.all_detected);
        assert_eq!(e.stats().drills_run, 1);
        // 演练区收尾退役（drill 内部已退役），豁免零基态不被污染。
        let h = e.health();
        assert_eq!(h.exempt_active, 0);
    }

    // ── 双跑确定性对拍（指纹 + 快照双口径）───────────────────────────────

    #[test]
    fn test_double_run_determinism() {
        for seed in [1u64, 42, 0xDEAD_BEEF, u64::MAX] {
            let rep = double_run_check(seed);
            assert!(rep.fingerprints_equal, "种子 {} 指纹漂移", seed);
            assert_eq!(rep.snapshot_diff_count, 0, "种子 {} 快照差异", seed);
            assert!(rep.consistent);
        }
        // 同种子两轮对拍自身一致；不同种子指纹应不同（区分度）。
        let r1 = double_run_check(7);
        let r2 = double_run_check(8);
        assert_ne!(r1.fp_a, r2.fp_a, "不同种子指纹应不同");
        // LCG 确定性：同种子同序列。
        let mut a = DetLcg::new(99);
        let mut b = DetLcg::new(99);
        let mut i = 0;
        while i < 8 {
            assert_eq!(a.next_u64(), b.next_u64());
            i += 1;
        }
    }

    // ── 环形账本驱逐 + 比率统计 ──────────────────────────────────────────

    #[test]
    fn test_ring_eviction_and_rates() {
        let mut e = WxPolicyEngine::new();
        // 1030 次裁决：环容量 1024，驱逐 6。
        let mut i = 0;
        while i < 1030 {
            if i % 2 == 0 {
                let _ = e.adjudicate(RegionType::KernelData, PermRwx::RW);
            } else {
                let _ = e.adjudicate(RegionType::KernelRoData, PermRwx::RW); // 拒
            }
            i += 1;
        }
        assert_eq!(e.decision_len(), MAX_DECISIONS);
        assert_eq!(e.stats().decision_evictions, 6);
        assert_eq!(e.stats().decisions_total, 1030);
        assert_eq!(e.stats().decisions_reject, 515);
        // 拒收率恰 5000bp（515/1030 = 1/2 整——偶奇交替口径）。
        assert_eq!(e.stats().reject_rate_bp(), 5000);
        // 分类型率：RoData 100%。
        assert_eq!(
            e.stats().kind_reject_rate_bp(RegionType::KernelRoData),
            10_000
        );
        assert_eq!(e.stats().kind_reject_rate_bp(RegionType::KernelData), 0);
        // 最旧在场行是第 7 次（i=6）。
        let oldest = e.decision_at(0).cloned().unwrap_or(DecisionRecord {
            seq: 0,
            kind: RegionType::KernelCode,
            req: PermRwx::NONE,
            granted: true,
            code: E_OK,
        });
        assert_eq!(oldest.kind, RegionType::KernelData);
    }

    // ── 快照/指纹/审计报告 ────────────────────────────────────────────────

    #[test]
    fn test_snapshot_fingerprint_audit() {
        let mut e = WxPolicyEngine::new();
        let _ = e.register_region(RegionType::KernelData, 0x0010_0000, PAGE_4K, PermRwx::RW);
        let s1 = e.snapshot();
        let _ = e.register_region(RegionType::KernelCode, 0x0020_0000, PAGE_4K, PermRwx::RX);
        let s2 = e.snapshot();
        let diff = diff_snapshots(&s1, &s2);
        assert!(!diff.is_empty(), "状态推进后快照应有差异");
        assert!(diff.len() >= 2); // fingerprint + regions_total 至少两处
        assert!(diff_snapshots(&s1, &s1).is_empty());
        // 同状态指纹稳定。
        assert_eq!(e.fingerprint(), e.fingerprint());
        // 审计报告要素齐备。
        let rep = e.audit_report();
        assert!(rep.contains(TASK_TAG));
        assert!(rep.contains("RW-"));
        assert!(rep.contains("R-X"));
        assert!(rep.contains("裁决账"));
        assert!(rep.contains("豁免账"));
        assert!(rep.contains("指纹"));
    }

    // ── 封账语义 ──────────────────────────────────────────────────────────

    #[test]
    fn test_seal_semantics() {
        let mut e = WxPolicyEngine::new();
        let id = e
            .register_region(RegionType::TaskStack, 0x0010_0000, PAGE_4K, PermRwx::RW)
            .unwrap_or(u32::MAX);
        assert!(e.seal().is_ok());
        assert!(e.is_sealed());
        assert_eq!(e.seal(), Err(WxPolicyError::BadState));
        // 一切变更红案。
        assert_eq!(
            e.register_region(RegionType::TaskHeap, 0x0040_0000, PAGE_4K, PermRwx::RW),
            Err(WxPolicyError::BadState)
        );
        assert_eq!(
            e.mprotect(id, PermRwx::R),
            Err(WxPolicyError::BadState)
        );
        assert_eq!(e.retire_region(id), Err(WxPolicyError::BadState));
        assert_eq!(
            e.register_exempt(0x7000, 1, PermRwx::W, PermRwx::X),
            Err(WxPolicyError::BadState)
        );
        assert_eq!(
            e.check_dual_map(0x7000, PermRwx::W, PermRwx::X),
            Err(WxPolicyError::BadState)
        );
        assert_eq!(
            e.corrupt_region_perms_for_drill(id, PermRwx::RWX),
            Err(WxPolicyError::BadState)
        );
        assert_eq!(e.batch_begin(), Err(WxPolicyError::BadState));
        // 只读面放行：探针（不留账）/裁决（不记账）/快照/健康/审计。
        let before = e.event_len();
        let dl_before = e.decision_len();
        let p = e.probe(0x0010_0000, AccessOp::Read);
        assert!(p.allowed);
        assert_eq!(e.event_len(), before, "封账后探针不追加事件账");
        assert_eq!(e.adjudicate(RegionType::KernelData, PermRwx::RW), Ok(PermRwx::RW));
        assert_eq!(e.adjudicate(RegionType::KernelData, PermRwx::RWX), Err(WxPolicyError::WxViolation));
        assert_eq!(e.decision_len(), dl_before, "封账后纯裁决不记账");
        let _ = e.snapshot();
        let _ = e.health();
        let _ = e.audit_report();
    }

    // ── 诊断码闭集完整性 ─────────────────────────────────────────────────

    #[test]
    fn test_error_code_closure() {
        let all = [
            WxPolicyError::IllegalPermBits,
            WxPolicyError::WxViolation,
            WxPolicyError::PermSuperset,
            WxPolicyError::UnknownRegionType,
            WxPolicyError::RegionShape,
            WxPolicyError::RegionOverlap,
            WxPolicyError::RegionUnknown,
            WxPolicyError::BadState,
            WxPolicyError::LedgerFull,
            WxPolicyError::MprotectUpgrade,
            WxPolicyError::ExemptFull,
            WxPolicyError::ExemptDuplicate,
            WxPolicyError::ExemptUnregistered,
            WxPolicyError::ExemptAliasDirty,
            WxPolicyError::ExemptUnknown,
        ];
        assert_eq!(all.len(), 15);
        let mut seen: Vec<u16> = Vec::new();
        for e in all {
            let c = e.code();
            assert!(c > SEG_BASE && c <= SEG_BASE + 0x0F, "码 {:#06x} 出段", c);
            assert!(!seen.contains(&c), "码 {:#06x} 重复", c);
            seen.push(c);
            assert!(!e.detail().is_empty());
            // Display 含码与诊断。
            let s = format!("{}", e);
            assert!(s.contains(&format!("{:#06x}", c)));
        }
        // 段内 15 错误码 + 1 OK 码恰满 16。
        assert_eq!(seen.len(), 16 - 1);
        assert_eq!(E_OK, SEG_BASE);
    }

    /// 占位区域记录（测试里替代 unwrap 的兜底值；断言失败时给出可读错）。
    static PLACEHOLDER: RegionRecord = RegionRecord {
        id: u32::MAX,
        kind: RegionType::KernelCode,
        base: 0,
        len: 0,
        perms: PermRwx::NONE,
        state: RegionState::Retired,
        grant_code: E_OK,
        born_seq: 0,
        retired_seq: 0,
    };

    // ── PermRwx 代数性质 ──────────────────────────────────────────────────

    #[test]
    fn test_permrwx_algebra() {
        // 子集自反 / 交换幺半群性质抽查。
        for a in PermRwx::all_combos() {
            assert!(a.is_subset_of(a));
            assert_eq!(a.union(a), a);
            assert_eq!(a.intersect(a), a);
            assert!(a.union(PermRwx::NONE) == a);
            // 违例组合恰两种。
            if a == PermRwx::WX || a == PermRwx::RWX {
                assert!(a.is_wx_violation());
            } else if a.is_legal() {
                assert!(!a.is_wx_violation());
            }
        }
        // minus/has_extra 语义。
        assert!(PermRwx::RW.has_extra_bits_of(PermRwx::R));
        assert!(!PermRwx::R.has_extra_bits_of(PermRwx::RW));
        assert_eq!(PermRwx::RW.minus(PermRwx::W), PermRwx::R);
        // mnem 速记。
        assert_eq!(PermRwx::RX.mnem(), "R-X");
        assert_eq!(PermRwx::NONE.mnem(), "---");
        assert_eq!(PermRwx::RWX.mnem(), "RWX");
        // 事件摘要确定性。
        let e1 = EngineEvent::Seal { seq: 7 };
        let e2 = EngineEvent::Seal { seq: 7 };
        assert_eq!(e1.digest(), e2.digest());
        let e3 = EngineEvent::Seal { seq: 8 };
        assert_ne!(e1.digest(), e3.digest());
    }

    // ── RegionRecord 区间代数 ─────────────────────────────────────────────

    #[test]
    fn test_region_interval_math() {
        let r = RegionRecord {
            id: 0,
            kind: RegionType::KernelData,
            base: 0x1000,
            len: 0x3000,
            perms: PermRwx::RW,
            state: RegionState::Active,
            grant_code: E_OK,
            born_seq: 1,
            retired_seq: 0,
        };
        assert!(r.contains(0x1000));
        assert!(r.contains(0x3FFF));
        assert!(!r.contains(0x4000), "半开区间上界不含");
        assert!(!r.contains(0x0FFF));
        assert!(r.overlaps(0x3000, 0x1000));
        assert!(!r.overlaps(0x4000, 0x1000));
        // 溢出防御：end 用 wrapping 不 panic。
        let wild = RegionRecord {
            id: 1,
            kind: RegionType::KernelData,
            base: u64::MAX - 0x1000 + 1,
            len: 0x2000,
            perms: PermRwx::RW,
            state: RegionState::Active,
            grant_code: E_OK,
            born_seq: 2,
            retired_seq: 0,
        };
        let _ = wild.end();
        // 存量审计口径。
        assert!(r.perms_audit().is_ok());
    }

    // ── 剧本烟囱（判据总口径一遍过）──────────────────────────────────────

    #[test]
    fn test_scripted_scenario_smoke() {
        let mut e = WxPolicyEngine::new();
        let fp = scripted_scenario(&mut e, 0x1234_5678);
        assert_ne!(fp, 0);
        // 剧本含拒收（违例组合/升权/未登记豁免）与放行两类。
        assert!(e.stats().decisions_total > 20);
        assert!(e.stats().decisions_reject > 0);
        assert!(e.stats().wx_redlines > 0);
        assert!(e.stats().sweep_violations > 0, "钻孔后扫描应立案");
        // 剧本收尾豁免零基态（撤销收口）。
        let h = e.health();
        assert_eq!(h.exempt_active, 0);
        assert!(h.matrix_clean);
    }
}
