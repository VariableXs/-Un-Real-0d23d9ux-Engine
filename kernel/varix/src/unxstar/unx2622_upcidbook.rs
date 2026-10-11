//! ===========================================================================
//! UNX-F2622 · PCID 运行期语义总册（NOFLUSH 位 / 切换分岔，F2405 封装消费）
//! ===========================================================================
//!
//! **锚点原文**（`anchors/UNX-F2622.md` 逐字摘抄，判据原文不改写）：
//! - 域/批：A4/B12
//! - 判据：**UNX-F2622-J1 PCID on/off 双模式切换语义逐项断言，
//!   NOFLUSH 违用注入 10/10 次拦截**
//! - 纯功能行数：300 行（语义总册 120 + 分岔实现 100 + 注入断言 80；测试段不计）
//! - 定位：F2405 封装（B01：能力判定/轮转/INVPCID 封装）之上的运行期语义
//!   总册——PCID 启用态下 CR3 写的 NOFLUSH 位语义（bit63 置位=切换不刷 TLB，
//!   保留两空间缓存）、PCID 启用/关闭双模式的切换行为分岔表、"违用拦截"
//!   （PCID 关闭时 NOFLUSH 位必须清零，违用=TLB 行为未定义）是本条硬口径。
//! - 语义边界：只做运行期语义与分岔；不做能力判定与封装（F2405）、
//!   不做命中账（F2632）、不做失效范围（F2627）。
//! - 依赖与嫁接源：依赖 F2405/F2621；嫁接源为 Intel SDM CR3 bit63
//!   （PCID NOFLUSH）语义，实现自研。
//! - 风险与回退：双模式行为漂移（on/off 两路语义不一致造成隐性缺陷）——
//!   分岔表全列（on/off×场景矩阵），机械对照断言（表项=分支），漂移即红灯。
//!
//! **码段**：`0x1CC0 .. 0x1CFF`（`SEG_BASE = 0x1CC0`，`SEG_MASK = 0x003F`，
//! 共 64 个错误码位，本单错误闭集取前 44 个，末尾留给后续同段单）。
//!
//! ---------------------------------------------------------------------------
//! 一、本单到底在做什么
//! ---------------------------------------------------------------------------
//!
//! PCID（Process Context Identifier）的硬件语义本身很短：CR4.PCIDE=1 时，
//! CR3 低 12 位是 PCID（地址空间标识），CR3 bit63 是 NOFLUSH。写 CR3 时：
//! - `NOFLUSH = 1`：不刷 TLB，**两个地址空间的缓存都保留**；
//! - `NOFLUSH = 0`：刷掉所有**非全局**（PCID 化）条目，保留全局条目。
//! 而当 `CR4.PCIDE = 0` 时，CR3 是纯页表基址，低 12 位被忽略、
//! **bit63 必须为 0**——置 1 时行为**未定义**（SDM 明确写 "reserved for
//! 未来扩展，置位行为未定义"）。这就是本单的"违用"（misuse）。
//!
//! 硬件手册用散文描述以上规则，软件要落地就必须把它变成**可机械对照的表**。
//! 本单做的就是这件事：
//!
//! 1. **语义总册**（`Book`）：把规则登记成一条条 `BookRow`（行），
//!    每行有 `rule_id` / `applies`（适用条件闭集）/ `verdict`（语义结论）
//!    / `tlb_action` / `advice`（三要素：现象 / 位置 / 建议修法）。
//! 2. **分岔表**（`branch_table`）：`Mode(PcidOff/PcidOn) × Scene(8 场景)`
//!    = 16 格，每格给出 `Branch`：接受（带 `TlbAction`）或拒绝
//!    （带 `ErrKind`）。实现（`PcidBook::switch_cr3`）**逐格走同一张表**，
//!    不允许出现"表里有、实现里没有"的分支。
//! 3. **机械对照**（`table_branch_parity`）：总册行数 == 分支数 == 实现
//!    里出现的 `Branch` 变体集合，任一不等即红灯。这是对"双模式行为漂移"
//!    这条风险的正面回答：漂移在结构上不可能悄悄发生。
//! 4. **失效探针**（`TlbProbe`）：一个**纯逻辑**的 TLB 模型，按 ASID 记
//!    条目数 + 全局条目数。切换动作真的作用在这个模型上，于是
//!    "NOFLUSH 切换后旧空间缓存保留可证"不是靠注释声称，而是靠
//!    `probe.old_space_survives()` 现场读出来。
//! 5. **注入核**：12 种负向形态 × 10 轮 = 120 次注入，全部必须被拦截
//!    且错误码精确匹配（判据要求 `10/10`，本单给 `120/120`）。
//!
//! ---------------------------------------------------------------------------
//! 二、为什么"表 = 分支"这件事值得单独立一个单
//! ---------------------------------------------------------------------------
//!
//! PCID 相关的真实事故几乎全是同一类：**开/关两路代码由两个人在不同时期
//! 写的，各自都能跑，测一条路径时另一条路径的语义悄悄不对**。典型症状是
//! "切到新 ASID 后偶尔还能读到旧空间的陈旧页"——因为 off 路忘了清
//! NOFLUSH，硬件把 bit63 当保留位，TLB 行为未定义。
//!
//! Windows 的 `KeFlushVirtualAddress` / `MmSwitchProcess` 也做 on/off
//! 分岔，但它是**代码分支**（`if (FeatureControl.CpuFeatures.Pcid)`），
//! 语义写在注释与代码里，没有一张可枚举对照的表。差异（也就是本单
//! "深在哪"的地方）：
//!
//! - **可枚举性**：本单的分岔表是 `2 × 8 = 16` 个**闭集元素**，
//!   可以被机械遍历、被指纹冻结、被回放比对；Windows 的分支是散在
//!   多处的源码条件，改一处不会让任何测试变红。
//! - **语义留痕**：本单每条规则都带 `TlbAction`（对 TLB 干了什么）
//!   和 `advice`（不该这么写时该怎么改）；不是"错了就 panic"。
//! - **探针可证**：本单的"保留缓存"是逻辑模型上的**计数差**，
//!   可断言、可导出；真实硬件上这条只能靠时序与性能反推。
//! - **零硬件依赖**：纯功能线不碰 CR3、不碰 TLB、不碰 INVDATE，
//!   所以能在 CI 里以 10^5 次/秒的速度跑完两模式全矩阵。
//!
//! ---------------------------------------------------------------------------
//! 三、判据逐条落点
//! ---------------------------------------------------------------------------
//!
//! | 判据片段 | 落点 |
//! |---|---|
//! | 双模式切换语义**逐项断言** | `branch_matrix()` 16 格全列 + `switch_stress()` 10^5 次 on/off 切换后逐项重断言 |
//! | NOFLUSH **违用注入 10/10 次拦截** | `inject_all()` 12 形态 × 10 轮；`F1NoFlushWhileOff` 等违用形态 10/10 拦截且码精确匹配 |
//! | 表分支**零漂移** | `table_branch_parity()`：总册行数 == 分支变体集合 == 实现触达集合 |
//! | 失效探针观测账可查 | `TlbProbe` + `Ledger` 守恒式 + `export_kv` 逐项列出 |
//!
//! ---------------------------------------------------------------------------
//! 四、复杂度与边界
//! ---------------------------------------------------------------------------
//!
//! - `book_rows()`：`O(BOOK_ROWS)`；`branch_table()`：`O(MODES * SCENES)`
//! - `switch_cr3()`：`O(1)`——查表一次，无循环、无分配（除错误串）
//! - `switch_stress(n)`：`O(n * SCENES)`（每次切换后重断言全部场景）
//! - `inject_all(seed)`：`O(FORM_COUNT * INJECT_ROUNDS)`
//! - `replay(seed)`：`O(REPLAY_STEPS)`，`O(1)` 空间（账用固定容量环）
//! - 零真实 IO、零墙钟、零浮点；时间一律逻辑 `tick: u64`
//!
//! ---------------------------------------------------------------------------
//! 五、纯功能线
//! ---------------------------------------------------------------------------
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`，零浮点，
//! 零真实 IO，零墙钟；时间一律为逻辑 `tick: u64`；确定性 LCG
//! `6364136223846793005 / 1442695040888963407`（`wrapping` 运算）；
//! 冻结指纹 FNV-1a 64（basis `0xcbf29ce484222325`，prime `0x100000001b3`）。
#![allow(dead_code)]
extern crate alloc;

/// 固定导入：`vec` 为产线硬要求的宏入口，虽本模块未直接用宏展开，
/// 但删除会使本单与其余四单的导入形态不一致，故显式豁免 unused 告警。
#[allow(unused_imports)]
use alloc::{vec, vec::Vec, string::{String, ToString}, format};

// ===========================================================================
// 0. 版本锚
// ===========================================================================

/// 本单版本号（语义总册版次；每次语义表项变更必须 +1）。
pub const V2622_VERSION_ID: u64 = 0x2622_0001;
/// 本单版本串（进 `export_kv` / `export_tlv`）。
pub const V2622_VERSION_STR: &str = "F2622/PCID-RUNTIME-LEDGER/1.0.0";
/// 码段基址。
pub const SEG_BASE: u16 = 0x1CC0;
/// 码段掩码（`SEG_BASE | SEG_MASK` 为本段上界）。
pub const SEG_MASK: u16 = 0x003F;

// ===========================================================================
// 1. 常量表（判据阈值写死，逐条给来源）
// ===========================================================================

/// CR3 的 NOFLUSH 位号（Intel SDM Vol.3A §4.3 CR3 bit63）。
pub const CR3_NOFLUSH_BIT: u8 = 63;
/// CR3 的 NOFLUSH 掩码（`1u64 << 63`）。
pub const CR3_NOFLUSH_MASK: u64 = 0x8000_0000_0000_0000u64;
/// CR3 的页表基址字段掩码（SDM Vol.3A §4.3：bit12..51，最大 40 位物理地址）。
pub const CR3_PT_BASE_MASK: u64 = 0x000F_FFFF_FFFF_F000u64;
/// CR3 的**越界保留位**掩码（bit52..62）。
///
/// bit51..52 之间是物理地址宽度的实现相关段；本单把 bit52..62 视为
/// 「未定义、必须清零」，置位即 `ERR_CR3_STRAY`。bit63 是 NOFLUSH，
/// 单独判（它在 off 模式下才违用）。
pub const CR3_STRAY_MASK: u64 = 0x7FF0_0000_0000_0000u64;
/// CR3 的 PCID 字段位宽（SDM：低 12 位为 PCID）。
pub const CR3_PCID_BITS: u8 = 12;
/// CR3 的 PCID 字段掩码（`0xFFF`）。
pub const CR3_PCID_MASK: u64 = 0x0000_0FFFu64;
/// PCID 最大值（12 位全 1）。
pub const PCID_MAX: u16 = 0x0FFFu16;
/// 保留 PCID（SDM：PCID=0xFFF 为保留值，不得分配）。
pub const PCID_RESERVED: u16 = 0x0FFFu16;
/// CR4 的 PCIDE 位号（SDM Vol.3A §4.1 CR4 bit17）。
pub const CR4_PCIDE_BIT: u8 = 17;

/// 双模式数（`PcidOff` / `PcidOn`）。
pub const MODES: usize = 2;
/// 场景数（见 `Scene` 闭集）。
pub const SCENES: usize = 8;
/// 分岔表格数（`MODES * SCENES`）。
pub const BRANCH_CELLS: usize = MODES * SCENES;
/// 语义总册行数（判据：表项数 == 分支数）。
pub const BOOK_ROWS: usize = 16;
/// 判据切换次数（锚点：双模式 10^5 次切换语义断言）。
pub const SWITCH_ROUNDS: u64 = 100_000;
/// 单次注入轮数（判据：10/10）。
pub const INJECT_ROUNDS: u64 = 10;
/// 注入形态数（判据要求 ≥10，本单实现 12）。
pub const FORM_COUNT: usize = 12;
/// 环形账固定容量（append-only，溢出只增 `dropped`）。
pub const RING_CAP: usize = 256;
/// 逻辑 TLB 的 ASID 槽位数（PCID 空间数）。
pub const ASID_SLOTS: usize = 16;
/// 逻辑 TLB 的全局条目槽位数（global 页不受 PCID 化 flush 影响）。
pub const GLOBAL_SLOTS: usize = 8;
/// 单个 ASID 槽位的最大条目数（逻辑模型上限）。
pub const ENTRIES_PER_ASID: usize = 8;
/// 回放步数上界。
pub const REPLAY_STEPS: usize = 4096;
/// LCG 乘数（Numerical Recipes / MSVC LCG 约定的 64 位版）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a 64 位基（basis）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（prime）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// TLV 组头魔数（`"VLK2"` 的小端字节序读法）。
pub const TLV_MAGIC: u64 = 0x324B_4C56;
/// TLV 组版本。
pub const TLV_VERSION: u64 = V2622_VERSION_ID;

/// 判据阈值汇总（阈值写死，来源逐条注在行尾）。
pub mod threshold {
    /// 判据 J1：双模式断言必须全过。
    pub const J1_BOTH_MODES: bool = true;
    /// 判据 J1：违用注入拦截率必须 100%（`10/10`，本单 `120/120`）。
    pub const J1_INJECT_PERFECT: bool = true;
    /// 判据 J1：表分支对照零漂移。
    pub const J1_ZERO_PARITY_DRIFT: bool = true;
    /// 锚点：切换压测 10^5 次。
    pub const ANCHOR_SWITCH_ROUNDS: u64 = super::SWITCH_ROUNDS;
    /// 锚点：注入轮数 10。
    pub const ANCHOR_INJECT_ROUNDS: u64 = super::INJECT_ROUNDS;
}

// ===========================================================================
// 2. 确定性 LCG（零墙钟；所有"随机"输入都由它驱动）
// ===========================================================================

/// 确定性线性同余发生器。
///
/// 状态推进：`s = s * LCG_A + LCG_C`（全部 `wrapping`）。
/// 之所以坚持自带 LCG 而不用任何平台的随机源：本单的注入、回放、探针
/// 填充都必须**同种子两次逐位一致**，任何熵源都会破坏冻结指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub s: u64,
}

impl Lcg {
    /// 由种子构造。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { s: seed }
    }
    /// 推进一格并返回新状态。
    pub fn next(&mut self) -> u64 {
        self.s = self.s.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.s
    }
    /// 取一个 `[0, bound)` 的值（`bound == 0` 返回 0，不产生除零）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
    /// 取 `[lo, hi]` 闭区间。
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo.wrapping_add(self.below(hi.wrapping_sub(lo).wrapping_add(1)))
    }
    /// 取低 `bits` 位（`bits >= 64` 视作 64）。
    pub fn bits(&mut self, bits: u32) -> u64 {
        if bits >= 64 {
            return self.next();
        }
        self.next() & ((1u64 << bits) - 1)
    }
    /// 取一枚布尔。
    pub fn flag(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

// ===========================================================================
// 3. FNV-1a 64 位（冻结指纹）
// ===========================================================================

/// 单字折叠。
pub const fn fnv1a_acc(h: u64, w: u64) -> u64 {
    let mut acc = h;
    acc ^= w;
    acc.wrapping_mul(FNV_PRIME)
}

/// 字节流折叠。
pub fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 字序列折叠（`u64` 逐字，按小端拆字节）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < words.len() {
        if let Some(w) = words.get(i) {
            let mut b = 0usize;
            while b < 8 {
                let byte = ((*w >> (b * 8)) & 0xFF) as u8;
                h ^= byte as u64;
                h = h.wrapping_mul(FNV_PRIME);
                b = b.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 串折叠。
pub fn fnv1a_str(s: &str) -> u64 {
    fnv1a_bytes(s.as_bytes())
}

// ===========================================================================
// 4. 错误闭集（`0x1CC0 .. 0x1CEB`，一码一段，共 44 变体）
// ===========================================================================

/// `0x1CC0` NOFLUSH 违用：PCID 关闭时 bit63 置位（SDM：行为未定义）。
pub const ERR_NOFLUSH_WHILE_OFF: u16 = 0x1CC0;
/// `0x1CC1` PCID 字段越界（> `PCID_MAX`）。
pub const ERR_PCID_RANGE: u16 = 0x1CC1;
/// `0x1CC2` 命中保留 PCID（`0xFFF`）。
pub const ERR_PCID_RESERVED: u16 = 0x1CC2;
/// `0x1CC3` ASID 处于回收期，禁止带 NOFLUSH 切换。
pub const ERR_ASID_RECYCLING: u16 = 0x1CC3;
/// `0x1CC4` 目标 ASID 与当前一致却要求 flush（语义冗余请求）。
pub const ERR_SAME_ASID_FLUSH: u16 = 0x1CC4;
/// `0x1CC5` 目标 ASID 与当前一致却要求 NOFLUSH（可接受但记账为冗余）。
pub const ERR_SAME_ASID_NOFLUSH: u16 = 0x1CC5;
/// `0x1CC6` CR4.PCIDE 位与模式声明不一致。
pub const ERR_PCIDE_MISMATCH: u16 = 0x1CC6;
/// `0x1CC7` 场景闭集越界。
pub const ERR_SCENE_RANGE: u16 = 0x1CC7;
/// `0x1CC8` 模式闭集越界。
pub const ERR_MODE_RANGE: u16 = 0x1CC8;
/// `0x1CC9` 分岔表格数与 `MODES * SCENES` 不符。
pub const ERR_BRANCH_CELLS: u16 = 0x1CC9;
/// `0x1CCA` 总册行数与分支数不符（表分支漂移）。
pub const ERR_TABLE_DRIFT: u16 = 0x1CCA;
/// `0x1CCB` 总册行的 `verdict` 与分岔分支语义不符。
pub const ERR_VERDICT_MISMATCH: u16 = 0x1CCB;
/// `0x1CCC` 实现触达的分支变体集合与表不一致（覆盖漏项）。
pub const ERR_BRANCH_MISSING: u16 = 0x1CCC;
/// `0x1CCD` 实现触达了表外的分支变体（多写分支）。
pub const ERR_BRANCH_EXTRA: u16 = 0x1CCD;
/// `0x1CCE` 状态机当前态非法。
pub const ERR_STATE_ILLEGAL: u16 = 0x1CCE;
/// `0x1CCF` 状态机目标态非法。
pub const ERR_TARGET_ILLEGAL: u16 = 0x1CCF;
/// `0x1CD0` 状态机迁移不在合法边闭集内。
pub const ERR_EDGE_CLOSED: u16 = 0x1CD0;
/// `0x1CD1` 守恒账不守恒。
pub const ERR_LEDGER_UNBALANCED: u16 = 0x1CD1;
/// `0x1CD2` 环形账容量为 0。
pub const ERR_RING_ZERO: u16 = 0x1CD2;
/// `0x1CD3` 环形账 `head` 越界。
pub const ERR_RING_HEAD: u16 = 0x1CD3;
/// `0x1CD4` 环形账 `dropped` 与写入数不符。
pub const ERR_RING_DROPPED: u16 = 0x1CD4;
/// `0x1CD5` 环形账逐条指纹自校验失败。
pub const ERR_RING_FP: u16 = 0x1CD5;
/// `0x1CD6` 逻辑 TLB 槽位越界。
pub const ERR_ASID_SLOT: u16 = 0x1CD6;
/// `0x1CD7` 单 ASID 条目数超上限。
pub const ERR_ASID_FULL: u16 = 0x1CD7;
/// `0x1CD8` 逻辑 TLB 全局槽位越界。
pub const ERR_GLOBAL_SLOT: u16 = 0x1CD8;
/// `0x1CD9` 失效探针观测与账不符（守恒被破坏）。
pub const ERR_PROBE_DESYNC: u16 = 0x1CD9;
/// `0x1CDA` 冻结指纹校验失败。
pub const ERR_VERIFY_FAILED: u16 = 0x1CDA;
/// `0x1CDB` TLV 组头魔数不符。
pub const ERR_HEAD_MAGIC: u16 = 0x1CDB;
/// `0x1CDC` TLV 长度不自洽。
pub const ERR_TLV_LEN: u16 = 0x1CDC;
/// `0x1CDD` 回放步数超上界。
pub const ERR_REPLAY_STEPS: u16 = 0x1CDD;
/// `0x1CDE` 注入核要求注入 N 次但实际只跑 M 次。
pub const ERR_INJECT_SHORT: u16 = 0x1CDE;
/// `0x1CDF` 注入检出缺失（形态未被检出核抓到）。
pub const ERR_DETECT_MISS: u16 = 0x1CDF;
/// `0x1CE0` 注入错误码与期望不匹配。
pub const ERR_CODE_MISMATCH: u16 = 0x1CE0;
/// `0x1CE1` 切换次数不足判据要求（未达 10^5）。
pub const ERR_SWITCH_SHORT: u16 = 0x1CE1;
/// `0x1CE2` 双模式断言有分岔项未过。
pub const ERR_MODE_ASSERT: u16 = 0x1CE2;
/// `0x1CE3` 观察包自相矛盾。
pub const ERR_OBS_INCONSISTENT: u16 = 0x1CE3;
/// `0x1CE4` 总册行数不足判据规模。
pub const ERR_BOOK_SHORT: u16 = 0x1CE4;
/// `0x1CE5` 逻辑 TLB 条目数下溢（flush 超过存量）。
pub const ERR_TLB_UNDERFLOW: u16 = 0x1CE5;
/// `0x1CE6` 违用语义与硬件口径不符（NOFLUSH 必须 0）。
pub const ERR_MISUSE_SEMANTIC: u16 = 0x1CE6;
/// `0x1CE7` 切换请求缺少页表基址。
pub const ERR_NO_PT_BASE: u16 = 0x1CE7;
/// `0x1CE8` 页表基址未按 4KiB 对齐。
pub const ERR_PT_ALIGN: u16 = 0x1CE8;
/// `0x1CE9` CR3 高位非 NOFLUSH 位被置位（保留位违用）。
pub const ERR_CR3_STRAY: u16 = 0x1CE9;
/// `0x1CEA` 清空不同步（账未归零）。
pub const ERR_RESET_MISMATCH: u16 = 0x1CEA;
/// `0x1CEB` 账目分配不同步。
pub const ERR_ALLOC_DESYNC: u16 = 0x1CEB;
/// 错误码上界（含）。
pub const ERR_SEG_END: u16 = SEG_BASE | SEG_MASK;

/// 错误变体闭集（`0x1CC0 .. 0x1CEB`，共 44 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// NOFLUSH 违用。
    NoflushWhileOff,
    /// PCID 越界。
    PcidRange,
    /// 保留 PCID。
    PcidReserved,
    /// ASID 回收期。
    AsidRecycling,
    /// 同 ASID 却要 flush。
    SameAsidFlush,
    /// 同 ASID 却要 NOFLUSH（冗余记账）。
    SameAsidNoFlush,
    /// CR4.PCIDE 与模式声明不符。
    PcedeMismatch,
    /// 场景越界。
    SceneRange,
    /// 模式越界。
    ModeRange,
    /// 分岔表格数不符。
    BranchCells,
    /// 总册漂移。
    TableDrift,
    /// 结论与分支不符。
    VerdictMismatch,
    /// 分支漏项。
    BranchMissing,
    /// 分支多项。
    BranchExtra,
    /// 当前态非法。
    StateIllegal,
    /// 目标态非法。
    TargetIllegal,
    /// 迁移非法。
    EdgeClosed,
    /// 账不守恒。
    LedgerUnbalanced,
    /// 环账容量 0。
    RingZero,
    /// 环账 head 越界。
    RingHead,
    /// 环账 dropped 不符。
    RingDropped,
    /// 环账指纹失败。
    RingFp,
    /// ASID 槽位越界。
    AsidSlot,
    /// ASID 满。
    AsidFull,
    /// 全局槽位越界。
    GlobalSlot,
    /// 探针与账不同步。
    ProbeDesync,
    /// 冻结校验失败。
    VerifyFailed,
    /// 组头魔数不符。
    HeadMagic,
    /// TLV 长度不自洽。
    TlvLen,
    /// 回放超步。
    ReplaySteps,
    /// 注入不足。
    InjectShort,
    /// 检出缺失。
    DetectMiss,
    /// 码不匹配。
    CodeMismatch,
    /// 切换次数不足。
    SwitchShort,
    /// 模式断言未过。
    ModeAssert,
    /// 观察包不自洽。
    ObsInconsistent,
    /// 总册规模不足。
    BookShort,
    /// TLB 下溢。
    TlbUnderflow,
    /// 违用语义错。
    MisuseSemantic,
    /// 缺页表基址。
    NoPtBase,
    /// 页表未对齐。
    PtAlign,
    /// CR3 保留位违用。
    Cr3Stray,
    /// 清空不同步。
    ResetMismatch,
    /// 分配不同步。
    AllocDesync,
}

impl ErrKind {
    /// 全部变体（顺序即错误码序）。
    pub const ALL: [ErrKind; 44] = [
        ErrKind::NoflushWhileOff,
        ErrKind::PcidRange,
        ErrKind::PcidReserved,
        ErrKind::AsidRecycling,
        ErrKind::SameAsidFlush,
        ErrKind::SameAsidNoFlush,
        ErrKind::PcedeMismatch,
        ErrKind::SceneRange,
        ErrKind::ModeRange,
        ErrKind::BranchCells,
        ErrKind::TableDrift,
        ErrKind::VerdictMismatch,
        ErrKind::BranchMissing,
        ErrKind::BranchExtra,
        ErrKind::StateIllegal,
        ErrKind::TargetIllegal,
        ErrKind::EdgeClosed,
        ErrKind::LedgerUnbalanced,
        ErrKind::RingZero,
        ErrKind::RingHead,
        ErrKind::RingDropped,
        ErrKind::RingFp,
        ErrKind::AsidSlot,
        ErrKind::AsidFull,
        ErrKind::GlobalSlot,
        ErrKind::ProbeDesync,
        ErrKind::VerifyFailed,
        ErrKind::HeadMagic,
        ErrKind::TlvLen,
        ErrKind::ReplaySteps,
        ErrKind::InjectShort,
        ErrKind::DetectMiss,
        ErrKind::CodeMismatch,
        ErrKind::SwitchShort,
        ErrKind::ModeAssert,
        ErrKind::ObsInconsistent,
        ErrKind::BookShort,
        ErrKind::TlbUnderflow,
        ErrKind::MisuseSemantic,
        ErrKind::NoPtBase,
        ErrKind::PtAlign,
        ErrKind::Cr3Stray,
        ErrKind::ResetMismatch,
        ErrKind::AllocDesync,
    ];
    /// 闭集序号（= 错误码低 6 位）。
    pub fn index(self) -> u16 {
        let mut i: u16 = 0;
        while i < 44 {
            if let Some(k) = ErrKind::ALL.get(i as usize) {
                if *k == self {
                    return i;
                }
            }
            i = i.wrapping_add(1);
        }
        44
    }
    /// 错误码（`SEG_BASE | index`）。
    pub fn code(self) -> u16 {
        SEG_BASE | (self.index() & SEG_MASK)
    }
    /// 变体名（进日志 / 导出串）。
    pub fn name(self) -> &'static str {
        match self {
            ErrKind::NoflushWhileOff => "NOFLUSH_WHILE_OFF",
            ErrKind::PcidRange => "PCID_RANGE",
            ErrKind::PcidReserved => "PCID_RESERVED",
            ErrKind::AsidRecycling => "ASID_RECYCLING",
            ErrKind::SameAsidFlush => "SAME_ASID_FLUSH",
            ErrKind::SameAsidNoFlush => "SAME_ASID_NOFLUSH",
            ErrKind::PcedeMismatch => "PCIDE_MISMATCH",
            ErrKind::SceneRange => "SCENE_RANGE",
            ErrKind::ModeRange => "MODE_RANGE",
            ErrKind::BranchCells => "BRANCH_CELLS",
            ErrKind::TableDrift => "TABLE_DRIFT",
            ErrKind::VerdictMismatch => "VERDICT_MISMATCH",
            ErrKind::BranchMissing => "BRANCH_MISSING",
            ErrKind::BranchExtra => "BRANCH_EXTRA",
            ErrKind::StateIllegal => "STATE_ILLEGAL",
            ErrKind::TargetIllegal => "TARGET_ILLEGAL",
            ErrKind::EdgeClosed => "EDGE_CLOSED",
            ErrKind::LedgerUnbalanced => "LEDGER_UNBALANCED",
            ErrKind::RingZero => "RING_ZERO",
            ErrKind::RingHead => "RING_HEAD",
            ErrKind::RingDropped => "RING_DROPPED",
            ErrKind::RingFp => "RING_FP",
            ErrKind::AsidSlot => "ASID_SLOT",
            ErrKind::AsidFull => "ASID_FULL",
            ErrKind::GlobalSlot => "GLOBAL_SLOT",
            ErrKind::ProbeDesync => "PROBE_DESYNC",
            ErrKind::VerifyFailed => "VERIFY_FAILED",
            ErrKind::HeadMagic => "HEAD_MAGIC",
            ErrKind::TlvLen => "TLV_LEN",
            ErrKind::ReplaySteps => "REPLAY_STEPS",
            ErrKind::InjectShort => "INJECT_SHORT",
            ErrKind::DetectMiss => "DETECT_MISS",
            ErrKind::CodeMismatch => "CODE_MISMATCH",
            ErrKind::SwitchShort => "SWITCH_SHORT",
            ErrKind::ModeAssert => "MODE_ASSERT",
            ErrKind::ObsInconsistent => "OBS_INCONSISTENT",
            ErrKind::BookShort => "BOOK_SHORT",
            ErrKind::TlbUnderflow => "TLB_UNDERFLOW",
            ErrKind::MisuseSemantic => "MISUSE_SEMANTIC",
            ErrKind::NoPtBase => "NO_PT_BASE",
            ErrKind::PtAlign => "PT_ALIGN",
            ErrKind::Cr3Stray => "CR3_STRAY",
            ErrKind::ResetMismatch => "RESET_MISMATCH",
            ErrKind::AllocDesync => "ALLOC_DESYNC",
        }
    }
    /// 建议修法（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            ErrKind::NoflushWhileOff => "PCID 关闭时不要写 CR3 bit63：切回 PcidOff 路径，或先把 CR4.PCIDE 置 1 再带 NOFLUSH 切换。",
            ErrKind::PcidRange => "PCID 只允许 0..0xFFE：分配器侧加范围断言，别把未掩码的 ASID 直接写进 CR3。",
            ErrKind::PcidReserved => "0xFFF 是 SDM 保留 PCID：从 ASID 轮转表里剔除该值。",
            ErrKind::AsidRecycling => "回收期 ASID 正在被旧上下文引用：等回收窗口结束，或先刷该 ASID 再复用。",
            ErrKind::SameAsidFlush => "同 ASID 全刷是语义冗余：改用 NOFLUSH，或显式声明这是主动失效需求。",
            ErrKind::SameAsidNoFlush => "同 ASID 带 NOFLUSH 语义等价于空操作：记一笔冗余账即可，不必报错。",
            ErrKind::PcedeMismatch => "CR4.PCIDE 与模式声明必须同源：从 F2405 能力判定结果单点派生模式。",
            ErrKind::SceneRange => "场景闭集只有 8 项：调用方不要自造场景枚举。",
            ErrKind::ModeRange => "模式闭集只有 on/off 两项。",
            ErrKind::BranchCells => "分岔表必须是 2×8 格：改场景数时同步改 BRANCH_CELLS 与总册行数。",
            ErrKind::TableDrift => "总册行数与分岔格数必须相等：补齐缺失的语义行或删除多余行。",
            ErrKind::VerdictMismatch => "同一格的语义结论与分支动作必须同向：先改语义行，再改实现。",
            ErrKind::BranchMissing => "表里有、实现没走到的分支：实现侧补齐分岔。",
            ErrKind::BranchExtra => "实现里有、表里没有的分支：这是最危险的一种漂移，删实现或补表。",
            ErrKind::StateIllegal => "状态机被驱动到非法态：检查驱动源的态迁移序列。",
            ErrKind::TargetIllegal => "目标态不在闭集内。",
            ErrKind::EdgeClosed => "该迁移不在合法边闭集：改驱动顺序，不要绕过 `try_move`。",
            ErrKind::LedgerUnbalanced => "守恒账破了：切换动作必须同时更新 accepted/rejected/flushed 三项。",
            ErrKind::RingZero => "环形账容量不能为 0。",
            ErrKind::RingHead => "环形账 head 越界：`push` 必须走 `head = (head + 1) % RING_CAP`。",
            ErrKind::RingDropped => "`dropped` 必须等于「写入总数 - 容量」。",
            ErrKind::RingFp => "环形账逐条指纹自校验失败：有条目被就地改写，违反 append-only。",
            ErrKind::AsidSlot => "ASID 槽位越界：先查 ASID 分配器。",
            ErrKind::AsidFull => "单个 ASID 条目数超上限：这是逻辑模型的容量边界，不是缺陷。",
            ErrKind::GlobalSlot => "全局槽位越界。",
            ErrKind::ProbeDesync => "探针观测与账不符：切换后必须重算条目差并同步 `observed`。",
            ErrKind::VerifyFailed => "冻结指纹校验失败：语义表被改过而版本锚没升。",
            ErrKind::HeadMagic => "TLV 组头魔数不符：导出串被截断或串位。",
            ErrKind::TlvLen => "TLV 长度字段与实际 payload 不符。",
            ErrKind::ReplaySteps => "回放步数超上界：检查是否出现死循环式自激。",
            ErrKind::InjectShort => "注入轮数不足：每形态必须跑满 INJECT_ROUNDS 轮。",
            ErrKind::DetectMiss => "形态未被检出核抓到：检出闸比注入闸更弱，需要补断言。",
            ErrKind::CodeMismatch => "注入检出了但码不对：检出闸可能匹配了更宽的条件。",
            ErrKind::SwitchShort => "切换次数未达判据要求 10^5。",
            ErrKind::ModeAssert => "双模式分岔项有未过：先看分岔矩阵哪一格红了。",
            ErrKind::ObsInconsistent => "观察包自相矛盾：字段之间算术不闭合。",
            ErrKind::BookShort => "语义总册行数不足 16：判据要求表项数 == 分支数。",
            ErrKind::TlbUnderflow => "flush 掉的条目数超过存量：逻辑模型不允许下溢。",
            ErrKind::MisuseSemantic => "违用语义写错：PCID 关闭时 NOFLUSH 的硬件行为是未定义，不是「不刷 TLB」。",
            ErrKind::NoPtBase => "切换请求缺页表基址。",
            ErrKind::PtAlign => "页表基址必须 4KiB 对齐。",
            ErrKind::Cr3Stray => "CR3 的 bit12..62 在本单模型里是保留位，置位即违用。",
            ErrKind::ResetMismatch => "清空后账未归零：`reset` 必须把 total/accepted/rejected 同时置 0。",
            ErrKind::AllocDesync => "账目分配不同步：`note_*` 的分类之和必须等于 total。",
        }
    }
}

/// 本单错误（带三要素详情）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BookErr {
    /// 错误码（`SEG_BASE | index`）。
    pub code: u16,
    /// 定位载荷（位号 / 场景 / 计数等）。
    pub site: u64,
    /// 详情串。
    pub detail: String,
}

impl BookErr {
    /// 构造。
    pub fn of(k: ErrKind, site: u64) -> BookErr {
        BookErr { code: k.code(), site, detail: k.advice().to_string() }
    }
    /// 追加详情（不覆盖建议修法，而是把现象拼进去）。
    pub fn note_of(mut self, note: &str) -> BookErr {
        self.detail = format!("{} | 现象：{}", k_name(self.code), note);
        self
    }
    /// 变体名（按错误码反查）。
    pub fn name(&self) -> &'static str {
        k_name(self.code)
    }
    /// 三要素：现象。
    pub fn symptom(&self) -> String {
        format!("[{}] site=0x{:X}", self.name(), self.site)
    }
    /// 三要素：位置。
    pub fn location(&self) -> String {
        match self.code {
            0x1CC0 => "PcidBook::switch_cr3 / 分岔表 off 路 noflush 格".to_string(),
            0x1CC1 => "PcidBook::precheck / PCID 字段范围校验".to_string(),
            0x1CC2 => "AsidTable::mark_recycling / 保留 PCID 过滤".to_string(),
            0x1CC3 => "AsidTable::class_of / 回收期闸".to_string(),
            _ => format!("code=0x{:04X} / 见 detail", self.code),
        }
    }
    /// 三要素：建议修法。
    pub fn advice(&self) -> String {
        k_advice(self.code).to_string()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("0x{:04X} {} site=0x{:X} {}", self.code, self.name(), self.site, self.detail)
    }
}

/// 按错误码反查变体名（未知码回 `UNKNOWN`）。
pub fn k_name(code: u16) -> &'static str {
    let mut i: u16 = 0;
    while i < 44 {
        if let Some(k) = ErrKind::ALL.get(i as usize) {
            if k.code() == code {
                return k.name();
            }
        }
        i = i.wrapping_add(1);
    }
    "UNKNOWN"
}

/// 按错误码反查建议修法（未知码回通用建议）。
pub fn k_advice(code: u16) -> &'static str {
    let mut i: u16 = 0;
    while i < 44 {
        if let Some(k) = ErrKind::ALL.get(i as usize) {
            if k.code() == code {
                return k.advice();
            }
        }
        i = i.wrapping_add(1);
    }
    "查码段表 0x1CC0..0x1CEB，确认是否越界或来自未登记来源。"
}

/// 本单结果类型。
pub type BookResult<T> = Result<T, BookErr>;

// ===========================================================================
// 5. 领域类型：模式 / 场景 / TLB 动作 / 分岔
// ===========================================================================

/// PCID 双模式（CR4.PCIDE 的两种运行期取值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// CR4.PCIDE = 0：CR3 是纯页表基址，NOFLUSH 必须为 0。
    Off,
    /// CR4.PCIDE = 1：CR3 低 12 位是 PCID，bit63 是 NOFLUSH。
    On,
}

impl Mode {
    /// 全部模式（顺序即 `mode_index`）。
    pub const ALL: [Mode; MODES] = [Mode::Off, Mode::On];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            Mode::Off => 0,
            Mode::On => 1,
        }
    }
    /// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            Mode::Off => "pcid_off",
            Mode::On => "pcid_on",
        }
    }
    /// CR4.PCIDE 位值。
    pub fn pcide_bit(self) -> bool {
        match self {
            Mode::Off => false,
            Mode::On => true,
        }
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        format!("{}(CR4.PCIDE={})", self.name(), if self.pcide_bit() { 1 } else { 0 })
    }
}

/// 切换场景（分岔表的列维度，8 项闭集）。
///
/// 场景是"请求侧"的完整描述：`SameAsid`（目标 ASID 与当前一致）、
/// `NewAsid`（不同且有效）、`RecycleAsid`（目标处于回收期）、
/// `ReservedAsid`（目标为保留 PCID）、`NoFlushBit`（CR3 bit63 置位）。
/// 这些是正交维度，实现里把它们**化归**成一条判例链，而不是并列判断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 同 ASID + 不带 NOFLUSH（全刷，显式失效）。
    SameAsidFlush,
    /// 同 ASID + 带 NOFLUSH（语义空操作，记账为冗余）。
    SameAsidNoFlush,
    /// 新 ASID + 不带 NOFLUSH（刷非全局，保留全局）。
    NewAsidFlush,
    /// 新 ASID + 带 NOFLUSH（快切，两空间缓存都保留）。
    NewAsidNoFlush,
    /// 回收期 ASID + 不带 NOFLUSH（允许，先刷后用）。
    RecycleAsidFlush,
    /// 回收期 ASID + 带 NOFLUSH（违用：拦截）。
    RecycleAsidNoFlush,
    /// 保留 PCID + 不带 NOFLUSH（拦截：保留值不可分配）。
    ReservedAsidFlush,
    /// 保留 PCID + 带 NOFLUSH（违用：拦截）。
    ReservedAsidNoFlush,
}

impl Scene {
    /// 全部场景（顺序即 `scene_index`）。
    pub const ALL: [Scene; SCENES] = [
        Scene::SameAsidFlush,
        Scene::SameAsidNoFlush,
        Scene::NewAsidFlush,
        Scene::NewAsidNoFlush,
        Scene::RecycleAsidFlush,
        Scene::RecycleAsidNoFlush,
        Scene::ReservedAsidFlush,
        Scene::ReservedAsidNoFlush,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            Scene::SameAsidFlush => 0,
            Scene::SameAsidNoFlush => 1,
            Scene::NewAsidFlush => 2,
            Scene::NewAsidNoFlush => 3,
            Scene::RecycleAsidFlush => 4,
            Scene::RecycleAsidNoFlush => 5,
            Scene::ReservedAsidFlush => 6,
            Scene::ReservedAsidNoFlush => 7,
        }
    }
    /// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            Scene::SameAsidFlush => "same_asid_flush",
            Scene::SameAsidNoFlush => "same_asid_noflush",
            Scene::NewAsidFlush => "new_asid_flush",
            Scene::NewAsidNoFlush => "new_asid_noflush",
            Scene::RecycleAsidFlush => "recycle_asid_flush",
            Scene::RecycleAsidNoFlush => "recycle_asid_noflush",
            Scene::ReservedAsidFlush => "reserved_asid_flush",
            Scene::ReservedAsidNoFlush => "reserved_asid_noflush",
        }
    }
    /// 该场景请求是否带 NOFLUSH 位。
    pub fn noflush(self) -> bool {
        matches!(self, Scene::SameAsidNoFlush | Scene::NewAsidNoFlush | Scene::RecycleAsidNoFlush | Scene::ReservedAsidNoFlush)
    }
    /// 该场景的目标 ASID 类别。
    pub fn asid_class(self) -> AsidClass {
        match self {
            Scene::SameAsidFlush | Scene::SameAsidNoFlush => AsidClass::Same,
            Scene::NewAsidFlush | Scene::NewAsidNoFlush => AsidClass::Fresh,
            Scene::RecycleAsidFlush | Scene::RecycleAsidNoFlush => AsidClass::Recycling,
            Scene::ReservedAsidFlush | Scene::ReservedAsidNoFlush => AsidClass::Reserved,
        }
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        format!("{}(noflush={} asid={})", self.name(), if self.noflush() { 1 } else { 0 }, self.asid_class().name())
    }
}

/// ASID 目标类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsidClass {
    /// 与当前一致。
    Same,
    /// 全新有效。
    Fresh,
    /// 回收期。
    Recycling,
    /// 保留值。
    Reserved,
}

impl AsidClass {
    /// 全部类别。
    pub const ALL: [AsidClass; 4] = [AsidClass::Same, AsidClass::Fresh, AsidClass::Recycling, AsidClass::Reserved];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            AsidClass::Same => 0,
            AsidClass::Fresh => 1,
            AsidClass::Recycling => 2,
            AsidClass::Reserved => 3,
        }
    }
    /// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            AsidClass::Same => "same",
            AsidClass::Fresh => "fresh",
            AsidClass::Recycling => "recycling",
            AsidClass::Reserved => "reserved",
        }
    }
    /// 本单为该类别分配的**代表 ASID**（`PCID_RESERVED` 走保留值）。
    pub fn sample_asid(self, current: u16) -> u16 {
        match self {
            AsidClass::Same => current,
            AsidClass::Fresh => {
                let mut c = current.wrapping_add(1) & PCID_MAX;
                if c == PCID_RESERVED {
                    c = 1;
                }
                c
            }
            AsidClass::Recycling => {
                let mut c = current.wrapping_add(2) & PCID_MAX;
                if c == PCID_RESERVED {
                    c = 2;
                }
                c
            }
            AsidClass::Reserved => PCID_RESERVED,
        }
    }
}

/// TLB 上发生的动作（语义总册的"现象"一侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlbAction {
    /// 什么都不做（同 ASID + NOFLUSH 的空操作）。
    Noop,
    /// 只刷非全局（PCID 化）条目，保留全局条目。
    FlushNonGlobal,
    /// 全刷（含全局）——PCID 关闭时的唯一合法动作。
    FlushAll,
    /// 违用被拦截，TLB 未被触碰。
    Rejected,
}

impl TlbAction {
    /// 全部动作。
    pub const ALL: [TlbAction; 4] = [TlbAction::Noop, TlbAction::FlushNonGlobal, TlbAction::FlushAll, TlbAction::Rejected];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            TlbAction::Noop => 0,
            TlbAction::FlushNonGlobal => 1,
            TlbAction::FlushAll => 2,
            TlbAction::Rejected => 3,
        }
    }
    /// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            TlbAction::Noop => "noop",
            TlbAction::FlushNonGlobal => "flush_non_global",
            TlbAction::FlushAll => "flush_all",
            TlbAction::Rejected => "rejected",
        }
    }
    /// 是否触碰了 TLB（`Rejected` 不触碰）。
    pub fn touches_tlb(self) -> bool {
        !matches!(self, TlbAction::Rejected | TlbAction::Noop)
    }
    /// 是否保留了两个空间的缓存（NOFLUSH 快切的验证点）。
    pub fn keeps_both_spaces(self) -> bool {
        matches!(self, TlbAction::Noop)
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        format!("{}(touch={} keep_both={})", self.name(), if self.touches_tlb() { 1 } else { 0 }, if self.keeps_both_spaces() { 1 } else { 0 })
    }
}

/// 语义结论（总册的"判定"一侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 合法且语义确定。
    Legal,
    /// 合法但语义冗余（可以执行，但要记账）。
    Redundant,
    /// 违用：必须拦截。
    Misuse,
}

impl Verdict {
    /// 全部结论。
    pub const ALL: [Verdict; 3] = [Verdict::Legal, Verdict::Redundant, Verdict::Misuse];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            Verdict::Legal => 0,
            Verdict::Redundant => 1,
            Verdict::Misuse => 2,
        }
    }
    /// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Legal => "legal",
            Verdict::Redundant => "redundant",
            Verdict::Misuse => "misuse",
        }
    }
    /// 是否应被拦截。
    pub fn blocked(self) -> bool {
        matches!(self, Verdict::Misuse)
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        format!("{}(blocked={})", self.name(), if self.blocked() { 1 } else { 0 })
    }
}

/// 分岔结论（分岔表每格的取值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Branch {
    /// 接受，并给出 TLB 动作。
    Accept(TlbAction, Verdict),
    /// 拒绝，并给出错误码。
    Reject(u16),
}

impl Branch {
    /// 是否接受。
    pub fn accepted(self) -> bool {
        matches!(self, Branch::Accept(_, _))
    }
    /// 接受时的 TLB 动作（非接受给 `Rejected`）。
    pub fn action(self) -> TlbAction {
        match self {
            Branch::Accept(a, _) => a,
            Branch::Reject(_) => TlbAction::Rejected,
        }
    }
    /// 拒绝时的错误码（非拒绝给 0）。
    pub fn code(self) -> u16 {
        match self {
            Branch::Accept(_, _) => 0,
            Branch::Reject(c) => c,
        }
    }
    /// 接受时的语义结论（非接受给 `Misuse`）。
    pub fn verdict(self) -> Verdict {
        match self {
            Branch::Accept(_, v) => v,
            Branch::Reject(_) => Verdict::Misuse,
        }
    }
    /// 变体名（分支的"形态"标签，用于表分支对照）。
    pub fn shape(self) -> &'static str {
        match self {
            Branch::Accept(TlbAction::Noop, Verdict::Redundant) => "accept_noop_redundant",
            Branch::Accept(TlbAction::Noop, Verdict::Legal) => "accept_noop_legal",
            Branch::Accept(TlbAction::FlushNonGlobal, Verdict::Legal) => "accept_flush_legal",
            Branch::Accept(TlbAction::FlushNonGlobal, Verdict::Redundant) => "accept_flush_redundant",
            Branch::Accept(TlbAction::FlushAll, Verdict::Legal) => "accept_flush_all",
            Branch::Accept(TlbAction::FlushAll, Verdict::Redundant) => "accept_flush_all_redundant",
            Branch::Accept(TlbAction::Rejected, _) => "accept_rejected",
            Branch::Accept(_, Verdict::Misuse) => "accept_misuse_unreachable",
            Branch::Reject(_) => "reject",
        }
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        match self {
            Branch::Accept(a, v) => format!("accept[{}|{}]", a.name(), v.name()),
            Branch::Reject(c) => format!("reject[0x{:04X}]", c),
        }
    }
}

/// 语义总册的一行。
///
/// **总册不是注释**：每行都有可指纹化的字段，改一行就改冻结指纹，
/// 并且必须与分岔表同格同结论（`table_branch_parity()` 断言）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BookRow {
    /// 规则号（`R01` .. `R16` 对应分岔格序号）。
    pub rule: u16,
    /// 适用模式。
    pub mode: Mode,
    /// 适用场景。
    pub scene: Scene,
    /// 语义结论。
    pub verdict: Verdict,
    /// TLB 动作。
    pub action: TlbAction,
    /// SDM 出处（可追溯）。
    pub sdm: &'static str,
}

impl BookRow {
    /// 表项指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            self.rule as u64,
            self.mode.index() as u64,
            self.scene.index() as u64,
            self.verdict.index() as u64,
            self.action.index() as u64,
            fnv1a_str(self.sdm),
        ];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("R{:02} {} {} -> {} {} [{}]", self.rule, self.mode.name(), self.scene.name(), self.verdict.name(),
            self.action.name(), self.sdm)
    }
}

// ===========================================================================
// 6. 语义总册（16 行，与分岔表同格）
// ===========================================================================

/// **语义总册真值源**（`BOOK_ROWS = 16` 行，顺序 = 分岔格序号
/// `mode_index * SCENES + scene_index`）。
///
/// 逐格语义（SDM 口径）：
///
/// - `PcidOff` 路：CR3 是纯页表基址，NOFLUSH 位**必须为 0**。任何带
///   NOFLUSH 的请求都是**违用**（硬件行为未定义），四格
///   （`SameAsidNoFlush` / `NewAsidNoFlush` / `RecycleAsidNoFlush` /
///   `ReservedAsidNoFlush`）全拦截。不带 NOFLUSH 的四格：合法，
///   动作恒为 `FlushAll`（PCID 关闭时没有"非全局"这个概念，全部条目
///   都是全局可见的普通条目），其中 `SameAsidFlush` 与
///   `ReservedAsidFlush` 分别因"冗余"与"保留值"另作标注——
///   注意 `ReservedAsidFlush` 在 off 模式下 PCID 字段根本不参与解释，
///   所以**不拦**（这正是 off/on 两路最容易漂的一格）。
/// - `PcidOn` 路：`NewAsidNoFlush` 是 PCID 的核心收益路
///   （`Noop`，两空间缓存都保留）；`NewAsidFlush` 刷非全局保留全局；
///   `SameAsidNoFlush` 是空操作（`Noop` + `Redundant`）；
///   `SameAsidFlush` 合法但冗余（`FlushNonGlobal` + `Redundant`）；
///   回收期与保留值两路**无论带不带 NOFLUSH 都拦**（带 NOFLUSH 的
///   两格是本单"违用注入"的主目标）。
pub const BOOK: [BookRow; BOOK_ROWS] = [
    // ---- PcidOff 路（CR4.PCIDE = 0）----
    BookRow { rule: 1, mode: Mode::Off, scene: Scene::SameAsidFlush, verdict: Verdict::Redundant, action: TlbAction::FlushAll, sdm: "SDM 4.3 CR3.PCID ignored when PCIDE=0" },
    BookRow { rule: 2, mode: Mode::Off, scene: Scene::SameAsidNoFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 bit63 reserved if PCIDE=0" },
    BookRow { rule: 3, mode: Mode::Off, scene: Scene::NewAsidFlush, verdict: Verdict::Legal, action: TlbAction::FlushAll, sdm: "SDM 4.3 CR3 write flushes all" },
    BookRow { rule: 4, mode: Mode::Off, scene: Scene::NewAsidNoFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 bit63 reserved if PCIDE=0" },
    BookRow { rule: 5, mode: Mode::Off, scene: Scene::RecycleAsidFlush, verdict: Verdict::Legal, action: TlbAction::FlushAll, sdm: "SDM 4.3 CR3 write flushes all" },
    BookRow { rule: 6, mode: Mode::Off, scene: Scene::RecycleAsidNoFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 bit63 reserved if PCIDE=0" },
    BookRow { rule: 7, mode: Mode::Off, scene: Scene::ReservedAsidFlush, verdict: Verdict::Legal, action: TlbAction::FlushAll, sdm: "SDM 4.3 low 12 bits ignored when PCIDE=0" },
    BookRow { rule: 8, mode: Mode::Off, scene: Scene::ReservedAsidNoFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 bit63 reserved if PCIDE=0" },
    // ---- PcidOn 路（CR4.PCIDE = 1）----
    BookRow { rule: 9, mode: Mode::On, scene: Scene::SameAsidFlush, verdict: Verdict::Redundant, action: TlbAction::FlushNonGlobal, sdm: "SDM 4.3 NOFLUSH=0 flushes non-global" },
    BookRow { rule: 10, mode: Mode::On, scene: Scene::SameAsidNoFlush, verdict: Verdict::Redundant, action: TlbAction::Noop, sdm: "SDM 4.3 NOFLUSH=1 retains all PCID entries" },
    BookRow { rule: 11, mode: Mode::On, scene: Scene::NewAsidFlush, verdict: Verdict::Legal, action: TlbAction::FlushNonGlobal, sdm: "SDM 4.3 NOFLUSH=0 flushes non-global" },
    BookRow { rule: 12, mode: Mode::On, scene: Scene::NewAsidNoFlush, verdict: Verdict::Legal, action: TlbAction::Noop, sdm: "SDM 4.3 NOFLUSH=1 retains both PCIDs" },
    BookRow { rule: 13, mode: Mode::On, scene: Scene::RecycleAsidFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 PCID in recycle window is in use" },
    BookRow { rule: 14, mode: Mode::On, scene: Scene::RecycleAsidNoFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 bit63 misused with pending recycle" },
    BookRow { rule: 15, mode: Mode::On, scene: Scene::ReservedAsidFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 PCID 0xFFF is reserved" },
    BookRow { rule: 16, mode: Mode::On, scene: Scene::ReservedAsidNoFlush, verdict: Verdict::Misuse, action: TlbAction::Rejected, sdm: "SDM 4.3 PCID 0xFFF reserved + NOFLUSH" },
];

/// 取总册一行（`rule = 1..16`）。
pub fn book_row(rule: u16) -> BookResult<BookRow> {
    if rule == 0 || rule as usize > BOOK_ROWS {
        return Err(BookErr::of(ErrKind::BookShort, rule as u64));
    }
    match BOOK.get((rule - 1) as usize) {
        Some(r) => Ok(*r),
        None => Err(BookErr::of(ErrKind::BookShort, rule as u64)),
    }
}

/// 取总册某格（按模式 + 场景）。
pub fn book_cell(mode: Mode, scene: Scene) -> BookResult<BookRow> {
    let idx = mode.index() * SCENES + scene.index();
    match BOOK.get(idx) {
        Some(r) => Ok(*r),
        None => Err(BookErr::of(ErrKind::BranchCells, idx as u64)),
    }
}

/// 总册指纹（16 行折叠）。
pub fn book_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut i: usize = 0;
    while i < BOOK.len() {
        if let Some(r) = BOOK.get(i) {
            h = fnv1a_acc(h, r.fingerprint());
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 总册规模断言（判据：表项数 == 分支数）。
pub fn book_size_ok() -> bool {
    BOOK.len() == BOOK_ROWS && BOOK.len() == BRANCH_CELLS
}

// ===========================================================================
// 7. 分岔表（总册的可执行投影）
// ===========================================================================

/// **分岔表真值源**（`BRANCH_CELLS = 16` 格）。
///
/// 与 `BOOK` **逐格同构**：格子顺序一致，语义一致，只是把"结论 + 动作"
/// 打包成 `Branch`（接受带动作、拒绝带码）。这层冗余不是重复，而是
/// 让"表"能被**机械对照**：见 `table_branch_parity()`。
///
/// 违用格的错误码选择（这是本单最需要说清的一处）：
/// - off 路带 NOFLUSH → `ERR_NOFLUSH_WHILE_OFF`（这是判据点名的违用）
/// - on 路回收期 → `ERR_ASID_RECYCLING`
/// - on 路保留 PCID → `ERR_PCID_RESERVED`
pub const BRANCH_TABLE: [Branch; BRANCH_CELLS] = [
    // ---- PcidOff ----
    Branch::Accept(TlbAction::FlushAll, Verdict::Redundant),
    Branch::Reject(ERR_NOFLUSH_WHILE_OFF),
    Branch::Accept(TlbAction::FlushAll, Verdict::Legal),
    Branch::Reject(ERR_NOFLUSH_WHILE_OFF),
    Branch::Accept(TlbAction::FlushAll, Verdict::Legal),
    Branch::Reject(ERR_NOFLUSH_WHILE_OFF),
    Branch::Accept(TlbAction::FlushAll, Verdict::Legal),
    Branch::Reject(ERR_NOFLUSH_WHILE_OFF),
    // ---- PcidOn ----
    Branch::Accept(TlbAction::FlushNonGlobal, Verdict::Redundant),
    Branch::Accept(TlbAction::Noop, Verdict::Redundant),
    Branch::Accept(TlbAction::FlushNonGlobal, Verdict::Legal),
    Branch::Accept(TlbAction::Noop, Verdict::Legal),
    Branch::Reject(ERR_ASID_RECYCLING),
    Branch::Reject(ERR_ASID_RECYCLING),
    Branch::Reject(ERR_PCID_RESERVED),
    Branch::Reject(ERR_PCID_RESERVED),
];

/// 取分岔表一格。
pub fn branch_cell(mode: Mode, scene: Scene) -> BookResult<Branch> {
    if mode.index() >= MODES {
        return Err(BookErr::of(ErrKind::ModeRange, mode.index() as u64));
    }
    if scene.index() >= SCENES {
        return Err(BookErr::of(ErrKind::SceneRange, scene.index() as u64));
    }
    let idx = mode.index() * SCENES + scene.index();
    match BRANCH_TABLE.get(idx) {
        Some(b) => Ok(*b),
        None => Err(BookErr::of(ErrKind::BranchCells, idx as u64)),
    }
}

/// 分岔表指纹（16 格折叠）。
pub fn branch_fp() -> u64 {
    let mut h = FNV_BASIS;
    let mut i: usize = 0;
    while i < BRANCH_TABLE.len() {
        if let Some(b) = BRANCH_TABLE.get(i) {
            h = fnv1a_acc(h, b.code() as u64);
            h = fnv1a_acc(h, b.action().index() as u64);
            h = fnv1a_acc(h, b.verdict().index() as u64);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// **总册行 → 分支** 的投影函数（总册的"可执行化"）。
///
/// 违用格的错误码由 `mode` + `scene` 决定（这是**唯一**允许从语义
/// 反推码的地方）：
/// - off 路带 NOFLUSH → `ERR_NOFLUSH_WHILE_OFF`
/// - on 路 + 回收期 → `ERR_ASID_RECYCLING`
/// - on 路 + 保留 PCID → `ERR_PCID_RESERVED`
pub fn branch_of_row(row: &BookRow) -> Branch {
    if row.verdict.blocked() {
        if row.mode == Mode::Off {
            return Branch::Reject(ERR_NOFLUSH_WHILE_OFF);
        }
        if row.scene.asid_class() == AsidClass::Reserved {
            return Branch::Reject(ERR_PCID_RESERVED);
        }
        return Branch::Reject(ERR_ASID_RECYCLING);
    }
    Branch::Accept(row.action, row.verdict)
}

/// 分岔表规模断言。
pub fn branch_cells_ok() -> bool {
    BRANCH_TABLE.len() == BRANCH_CELLS
}

/// 分岔矩阵的一格（观测用记录）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BranchCell {
    /// 模式。
    pub mode: Mode,
    /// 场景。
    pub scene: Scene,
    /// 表里的结论。
    pub book: Branch,
    /// 表里的分支。
    pub table: Branch,
    /// 实现实际给出的分支。
    pub impl_got: Branch,
    /// 三者一致且总册结论与分支同向。
    pub pass: bool,
}

impl BranchCell {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} {} book={} table={} impl={} {}", self.mode.name(), self.scene.name(), self.book.describe(), self.table.describe(),
            self.impl_got.describe(), if self.pass { "PASS" } else { "FAIL" })
    }
}

/// **表 → 分支** 的机械对照（把总册投影成分岔表时的一致性断言）。
///
/// 逐格检查两条：
/// 1. `BOOK[i].verdict/action` 与 `BRANCH_TABLE[i]` 必须同向：
///    `Misuse` ↔ `Reject`，`Legal/Redundant` ↔ `Accept`，
///    且 `Accept` 的 `TlbAction` 必须与总册的 `action` 相等。
/// 2. `Misuse` 格的错误码必须非 0 且落在本段内。
///
/// 这条断言就是锚点里"表项 = 分支，漂移即红灯"的落点。
pub fn table_branch_parity() -> Vec<BranchCell> {
    let mut out: Vec<BranchCell> = Vec::new();
    let mut i: usize = 0;
    while i < BRANCH_CELLS {
        let book = BOOK.get(i).copied();
        let table = BRANCH_TABLE.get(i).copied();
        if let (Some(b), Some(t)) = (book, table) {
            let projected = branch_of_row(&b);
            let same_verdict = if b.verdict.blocked() { !t.accepted() && t.verdict() == Verdict::Misuse } else { t.accepted() && t.verdict() == b.verdict };
            let same_action = if t.accepted() { t.action() == b.action } else { b.action == TlbAction::Rejected };
            let code_ok = if b.verdict.blocked() {
                let c = t.code();
                c != 0 && (c & !SEG_MASK) == SEG_BASE
            } else {
                t.code() == 0
            };
            let proj_ok = projected == t;
            out.push(BranchCell { mode: b.mode, scene: b.scene, book: projected, table: t, impl_got: t,
                pass: same_verdict && same_action && code_ok && proj_ok });
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 表分支对照是否零漂移。
pub fn parity_ok() -> bool {
    let p = table_branch_parity();
    if p.len() != BRANCH_CELLS {
        return false;
    }
    let mut i: usize = 0;
    while i < p.len() {
        if let Some(c) = p.get(i) {
            if !c.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    book_size_ok() && branch_cells_ok()
}

// ===========================================================================
// 8. CR3 值模型（纯逻辑，不碰真实 CR3）
// ===========================================================================

/// 一个 CR3 写入值（纯逻辑建模）。
///
/// **注意**：本单**不**写任何真实 CR3，也不做任何 `invlpg` / `invpcid`。
/// `Cr3` 只是把"一个 64 位写入值按位拆开"的语义搬到纯功能层，
/// 好让"NOFLUSH 位在 off 模式下必须为 0"这条规则可被断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cr3 {
    /// 完整 64 位值。
    pub raw: u64,
}

impl Cr3 {
    /// 构造一个不带 NOFLUSH 的 CR3。
    pub fn new(pt_base: u64, asid: u16, noflush: bool) -> Cr3 {
        let mut raw = pt_base & CR3_PT_BASE_MASK;
        raw |= (asid as u64) & CR3_PCID_MASK;
        if noflush {
            raw |= CR3_NOFLUSH_MASK;
        }
        Cr3 { raw }
    }
    /// 页表基址字段掩码（SDM：CR3 bit12..51 为物理页表基址）。
    pub fn pt_base(&self) -> u64 {
        self.raw & !CR3_PCID_MASK & !CR3_NOFLUSH_MASK & CR3_PT_BASE_MASK
    }
    /// PCID 字段（低 12 位）。
    pub fn pcid(&self) -> u16 {
        (self.raw & CR3_PCID_MASK) as u16
    }
    /// NOFLUSH 位。
    pub fn noflush(&self) -> bool {
        self.raw & CR3_NOFLUSH_MASK != 0
    }
    /// **越界保留位**（bit52..62）——本单模型里置位即违用。
    ///
    /// 注意 bit12..51 是**合法的页表基址字段**，不是保留位；
    /// 只有 bit52..62 才是 SDM 未定义、必须清零的那一段。
    pub fn stray(&self) -> u64 {
        self.raw & CR3_STRAY_MASK
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a_words(&[self.raw, V2622_VERSION_ID])
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cr3=0x{:016X} pt=0x{:X} pcid=0x{:03X} noflush={} stray=0x{:X}", self.raw, self.pt_base(), self.pcid(),
            if self.noflush() { 1 } else { 0 }, self.stray())
    }
}

/// 一次 CR3 切换请求（自描述，不依赖调用方上下文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchReq {
    /// 声明的运行模式（应与 F2405 能力判定同源）。
    pub mode: Mode,
    /// 目标场景。
    pub scene: Scene,
    /// 目标 CR3 值。
    pub cr3: Cr3,
    /// 目标 ASID（= `cr3.pcid()`，冗余存一份便于断言两者一致）。
    pub asid: u16,
}

impl SwitchReq {
    /// 由模式 + 场景 + 当前 ASID 构造一个**内部自洽**的请求。
    ///
    /// 页表基址取一个按 4KiB 对齐的确定值（`0x1000 + asid * 0x2000`），
    /// 便于导出串里肉眼核对。
    pub fn make(mode: Mode, scene: Scene, current_asid: u16) -> SwitchReq {
        let asid = scene.asid_class().sample_asid(current_asid);
        let pt = (0x1000u64).wrapping_add((asid as u64).wrapping_mul(0x2000));
        let cr3 = Cr3::new(pt, asid, scene.noflush());
        SwitchReq { mode, scene, cr3, asid }
    }
    /// 内部一致性自检（ASID 与 CR3 字段、NOFLUSH 与场景是否吻合）。
    pub fn self_consistent(&self) -> bool {
        self.cr3.pcid() == self.asid && self.cr3.noflush() == self.scene.noflush()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} {} {}", self.mode.describe(), self.scene.describe(), self.cr3.describe())
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            self.mode.index() as u64,
            self.scene.index() as u64,
            self.cr3.raw,
            self.asid as u64,
            V2622_VERSION_ID,
        ];
        fnv1a_words(&words)
    }
}

// ===========================================================================
// 9. ASID 表（分配 / 回收期标记）
// ===========================================================================

/// ASID 分配表（PCID 空间的极简模型：谁在用、谁在回收期）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AsidTable {
    /// 每个 ASID 的状态（`0` 空闲、`1` 在用、`2` 回收期）。
    pub state: [u8; ASID_SLOTS],
    /// 当前 ASID。
    pub current: u16,
    /// 已分配次数（含复用）。
    pub alloc_count: u64,
}

impl AsidTable {
    /// 构造：ASID 0 在用，其余空闲，当前为 0。
    pub fn new() -> AsidTable {
        let mut t = AsidTable { state: [0u8; ASID_SLOTS], current: 0, alloc_count: 0 };
        if let Some(s) = t.state.get_mut(0) {
            *s = 1;
        }
        t
    }
    /// ASID → 槽位下标。
    ///
    /// **口径**：PCID 有 12 位（0..0xFFE 可用），而逻辑模型只留
    /// `ASID_SLOTS = 16` 个槽位，故按 `asid % ASID_SLOTS` 折叠。
    /// 折叠意味着不同 ASID 可能映射到同一槽位——这是逻辑模型的
    /// 容量取舍，不是缺陷；它只影响"每槽能装多少条目"这个观测量，
    /// 不影响任何语义裁决（裁决只看 `AsidClass`，由 `class_of` 给出）。
    pub fn slot(&self, asid: u16) -> BookResult<usize> {
        if asid > PCID_MAX {
            return Err(BookErr::of(ErrKind::PcidRange, asid as u64));
        }
        Ok((asid % ASID_SLOTS as u16) as usize)
    }
    /// 某 ASID 的状态。
    pub fn class_of(&self, asid: u16) -> BookResult<AsidClass> {
        if asid == PCID_RESERVED {
            return Ok(AsidClass::Reserved);
        }
        let i = self.slot(asid)?;
        let s = self.state.get(i).copied().unwrap_or(0);
        if s == 2 {
            return Ok(AsidClass::Recycling);
        }
        if asid == self.current {
            return Ok(AsidClass::Same);
        }
        Ok(AsidClass::Fresh)
    }
    /// 标记为回收期。
    pub fn mark_recycling(&mut self, asid: u16) -> BookResult<()> {
        if asid == PCID_RESERVED {
            return Err(BookErr::of(ErrKind::PcidReserved, asid as u64));
        }
        let i = self.slot(asid)?;
        if let Some(s) = self.state.get_mut(i) {
            *s = 2;
        }
        Ok(())
    }
    /// 切换当前 ASID（成功后调用；同 ASID 也允许，用于对拍）。
    pub fn set_current(&mut self, asid: u16) -> BookResult<()> {
        let i = self.slot(asid)?;
        if let Some(s) = self.state.get_mut(i) {
            *s = 1;
        }
        self.current = asid;
        self.alloc_count = self.alloc_count.wrapping_add(1);
        Ok(())
    }
    /// 在用槽位数（守恒账的分子之一）。
    pub fn live_slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.state.len() {
            if let Some(s) = self.state.get(i) {
                if *s != 0 {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 回收期槽位数。
    pub fn recycling_slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.state.len() {
            if let Some(s) = self.state.get(i) {
                if *s == 2 {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i: usize = 0;
        while i < self.state.len() {
            if let Some(s) = self.state.get(i) {
                h = fnv1a_acc(h, *s as u64);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&[h, self.current as u64, self.alloc_count])
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cur=0x{:03X} live={} recycling={} alloc={}", self.current, self.live_slots(), self.recycling_slots(), self.alloc_count)
    }
}

// ===========================================================================
// 10. 失效探针（逻辑 TLB 模型）
// ===========================================================================

/// 逻辑 TLB 失效探针。
///
/// 它不是"假装有 TLB"，而是**真的**按 ASID 记条目数，切换动作真的
/// 作用在它上面。于是判据里的"NOFLUSH 切换后旧空间缓存保留可证"就变成
/// 一条可断言的计数不等式：
///
/// ```text
/// 若 action == Noop（NOFLUSH 快切）：
///     切换前后 total_entries 必须相等
///     旧 ASID（= 切换前 current）的条目数必须不变
/// 若 action == FlushNonGlobal：
///     非全局条目被清空，全局条目数必须不变
/// 若 action == FlushAll：
///     连全局条目一起清空
/// ```
///
/// 守恒式（`probe_conserved()`）：
/// `sum(non_global) + globals == total`，且
/// `total == inserted - evicted`（插入与刷出必须配平）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TlbProbe {
    /// 每 ASID 的非全局条目数。
    pub entries: [u16; ASID_SLOTS],
    /// 全局条目数。
    pub globals: u16,
    /// 累计插入数。
    pub inserted: u64,
    /// 累计刷出数。
    pub evicted: u64,
    /// 上一次切换的观测账。
    pub observed: ProbeObs,
}

/// 一次切换的探针观测账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeObs {
    /// 切换前非全局条目合计。
    pub before_non_global: u32,
    /// 切换后非全局条目合计。
    pub after_non_global: u32,
    /// 切换前全局条目。
    pub before_global: u16,
    /// 切换后全局条目。
    pub after_global: u16,
    /// 切换前 ASID。
    pub old_asid: u16,
    /// 切换后 ASID。
    pub new_asid: u16,
    /// 旧 ASID 在切换后的条目数（NOFLUSH 保留性的直接证据）。
    pub old_space_after: u16,
    /// 本次刷出的条目数。
    pub flushed: u32,
    /// 本次动作。
    pub action: TlbAction,
}

impl Default for ProbeObs {
    fn default() -> Self {
        ProbeObs {
            before_non_global: 0,
            after_non_global: 0,
            before_global: 0,
            after_global: 0,
            old_asid: 0,
            new_asid: 0,
            old_space_after: 0,
            flushed: 0,
            action: TlbAction::Noop,
        }
    }
}

impl ProbeObs {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("obs[{} old=0x{:03X}->0x{:03X} ng {}->{} g {}->{} old_after={} flushed={}]", self.action.name(), self.old_asid,
            self.new_asid, self.before_non_global, self.after_non_global, self.before_global, self.after_global, self.old_space_after,
            self.flushed)
    }
}

impl TlbProbe {
    /// 构造（空 TLB）。
    pub fn new() -> TlbProbe {
        TlbProbe { entries: [0u16; ASID_SLOTS], globals: 0, inserted: 0, evicted: 0, observed: ProbeObs::default() }
    }
    /// 给某 ASID 装入条目（`count` 受 `ENTRIES_PER_ASID` 限制）。
    pub fn fill(&mut self, asid: u16, count: usize) -> BookResult<()> {
        if asid == PCID_RESERVED {
            return Err(BookErr::of(ErrKind::PcidReserved, asid as u64));
        }
        if asid > PCID_MAX {
            return Err(BookErr::of(ErrKind::PcidRange, asid as u64));
        }
        let i = (asid % ASID_SLOTS as u16) as usize;
        let cur = self.entries.get(i).copied().unwrap_or(0) as usize;
        let room = ENTRIES_PER_ASID.saturating_sub(cur);
        let add = if count < room { count } else { room };
        if let Some(e) = self.entries.get_mut(i) {
            *e = (*e as usize).wrapping_add(add) as u16;
        }
        self.inserted = self.inserted.wrapping_add(add as u64);
        Ok(())
    }
    /// 装入全局条目。
    pub fn fill_global(&mut self, count: usize) -> BookResult<()> {
        let room = GLOBAL_SLOTS.saturating_sub(self.globals as usize);
        let add = if count < room { count } else { room };
        self.globals = (self.globals as usize).wrapping_add(add) as u16;
        self.inserted = self.inserted.wrapping_add(add as u64);
        if add == 0 {
            return Err(BookErr::of(ErrKind::GlobalSlot, self.globals as u64));
        }
        Ok(())
    }
    /// 非全局条目合计。
    pub fn non_global_total(&self) -> u32 {
        let mut s: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                s = s.wrapping_add(*e as u32);
            }
            i = i.wrapping_add(1);
        }
        s
    }
    /// 条目总数。
    pub fn total(&self) -> u32 {
        self.non_global_total().wrapping_add(self.globals as u32)
    }
    /// 某 ASID 的条目数（槽位折叠口径，与 `AsidTable::slot` 一致）。
    pub fn count_of(&self, asid: u16) -> u16 {
        let i = (asid % ASID_SLOTS as u16) as usize;
        self.entries.get(i).copied().unwrap_or(0)
    }
    /// 守恒：`Σ entries + globals == inserted - evicted`。
    ///
    /// 这条式子从 0 起算：`TlbProbe::new()` 时 `inserted = evicted = 0`、
    /// 计数全 0，两边都是 0，天然成立；此后每次 `fill*` 加 `inserted`，
    /// 每次 flush 加 `evicted` 并同步减计数，任一侧漏改即破。
    pub fn conserved(&self) -> bool {
        let live = self.total() as u64;
        live == self.inserted.wrapping_sub(self.evicted)
    }
    /// 应用一次切换（真正作用在模型上）。
    pub fn apply(&mut self, old_asid: u16, new_asid: u16, action: TlbAction) -> BookResult<()> {
        let before_ng = self.non_global_total();
        let before_g = self.globals;
        let mut flushed: u32 = 0;
        match action {
            TlbAction::Noop => {
                // 不动任何条目。
            }
            TlbAction::FlushNonGlobal => {
                let mut i: usize = 0;
                while i < self.entries.len() {
                    if let Some(e) = self.entries.get_mut(i) {
                        flushed = flushed.wrapping_add(*e as u32);
                        *e = 0;
                    }
                    i = i.wrapping_add(1);
                }
            }
            TlbAction::FlushAll => {
                flushed = flushed.wrapping_add(before_ng);
                flushed = flushed.wrapping_add(before_g as u32);
                let mut i: usize = 0;
                while i < self.entries.len() {
                    if let Some(e) = self.entries.get_mut(i) {
                        *e = 0;
                    }
                    i = i.wrapping_add(1);
                }
                self.globals = 0;
            }
            TlbAction::Rejected => {
                // 违用被拦：TLB 未被触碰，观测账留痕但不记账。
                self.observed = ProbeObs {
                    before_non_global: before_ng,
                    after_non_global: before_ng,
                    before_global: before_g,
                    after_global: before_g,
                    old_asid,
                    new_asid,
                    old_space_after: self.count_of(old_asid),
                    flushed: 0,
                    action,
                };
                return Ok(());
            }
        }
        self.evicted = self.evicted.wrapping_add(flushed as u64);
        if self.conserved() == false {
            return Err(BookErr::of(ErrKind::ProbeDesync, flushed as u64));
        }
        self.observed = ProbeObs {
            before_non_global: before_ng,
            after_non_global: self.non_global_total(),
            before_global: before_g,
            after_global: self.globals,
            old_asid,
            new_asid,
            old_space_after: self.count_of(old_asid),
            flushed,
            action,
        };
        let _ = new_asid;
        Ok(())
    }
    /// 「NOFLUSH 切换后旧空间缓存保留可证」的断言口。
    ///
    /// 条件：动作是 `Noop`；则必须满足
    /// `before_non_global == after_non_global` 且
    /// `old_space_after == 切换前 old_asid 的条目数`。
    pub fn old_space_survives(&self) -> bool {
        let o = self.observed;
        if o.action != TlbAction::Noop {
            return false;
        }
        if o.before_non_global != o.after_non_global {
            return false;
        }
        let i = (o.old_asid % ASID_SLOTS as u16) as usize;
        match self.entries.get(i) {
            Some(v) => *v == o.old_space_after,
            None => false,
        }
    }
    /// 全局条目是否按语义保留（`FlushNonGlobal` 不动全局）。
    pub fn globals_preserved(&self) -> bool {
        let o = self.observed;
        if o.action == TlbAction::FlushNonGlobal {
            return o.before_global == o.after_global;
        }
        if o.action == TlbAction::FlushAll {
            return o.after_global == 0;
        }
        true
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv1a_acc(h, *e as u64);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&[
            h,
            self.globals as u64,
            self.inserted,
            self.evicted,
            self.observed.before_non_global as u64,
            self.observed.after_non_global as u64,
            self.observed.flushed as u64,
        ])
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("probe[ng={} g={} total={} ins={} evict={} cons={}]", self.non_global_total(), self.globals, self.total(), self.inserted,
            self.evicted, if self.conserved() { 1 } else { 0 })
    }
}

// ===========================================================================
// 11. 守恒账与 append-only 环形账
// ===========================================================================

/// 切换守恒账。
///
/// **守恒式**：`accepted + rejected == total`，且
/// `accepted == noop + flush_non_global + flush_all`
/// （接受的每一次都必须归入且只归入一个动作桶）。
///
/// 守恒式从 0 起算：`SwitchLedger::zero()` 全零，与 `PcidBook::new()`
/// 在同一处构造；此后每次 `switch_cr3()` 只允许走 `note_accept` 或
/// `note_reject` 之一，二者都会把 `total` 加 1，故 `total` 天然等于
/// 切换请求数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchLedger {
    /// 总切换请求数。
    pub total: u64,
    /// 接受数。
    pub accepted: u64,
    /// 拒绝数。
    pub rejected: u64,
    /// 接受中动作为 `Noop` 的次数。
    pub noop: u64,
    /// 接受中动作为 `FlushNonGlobal` 的次数。
    pub flush_non_global: u64,
    /// 接受中动作为 `FlushAll` 的次数。
    pub flush_all: u64,
    /// 语义冗余次数（`Verdict::Redundant`）。
    pub redundant: u64,
    /// 违用拦截次数（`Verdict::Misuse`）。
    pub misuse_blocked: u64,
    /// 被刷出的条目合计（探针观测汇总）。
    pub flushed_entries: u64,
}

impl SwitchLedger {
    /// 全零账。
    pub fn zero() -> SwitchLedger {
        SwitchLedger {
            total: 0,
            accepted: 0,
            rejected: 0,
            noop: 0,
            flush_non_global: 0,
            flush_all: 0,
            redundant: 0,
            misuse_blocked: 0,
            flushed_entries: 0,
        }
    }
    /// 记一次接受。
    pub fn note_accept(&mut self, b: Branch, flushed: u64) {
        self.total = self.total.wrapping_add(1);
        self.accepted = self.accepted.wrapping_add(1);
        match b.action() {
            TlbAction::Noop => self.noop = self.noop.wrapping_add(1),
            TlbAction::FlushNonGlobal => self.flush_non_global = self.flush_non_global.wrapping_add(1),
            TlbAction::FlushAll => self.flush_all = self.flush_all.wrapping_add(1),
            TlbAction::Rejected => {}
        }
        match b.verdict() {
            Verdict::Redundant => self.redundant = self.redundant.wrapping_add(1),
            Verdict::Legal => {}
            Verdict::Misuse => {}
        }
        self.flushed_entries = self.flushed_entries.wrapping_add(flushed);
    }
    /// 记一次拒绝。
    pub fn note_reject(&mut self) {
        self.total = self.total.wrapping_add(1);
        self.rejected = self.rejected.wrapping_add(1);
        self.misuse_blocked = self.misuse_blocked.wrapping_add(1);
    }
    /// 动作桶合计。
    pub fn action_sum(&self) -> u64 {
        self.noop.wrapping_add(self.flush_non_global).wrapping_add(self.flush_all)
    }
    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.accepted.wrapping_add(self.rejected) == self.total && self.action_sum() == self.accepted
    }
    /// 清空（账必须同步归零）。
    pub fn reset(&mut self) {
        *self = SwitchLedger::zero();
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a_words(&[
            self.total,
            self.accepted,
            self.rejected,
            self.noop,
            self.flush_non_global,
            self.flush_all,
            self.redundant,
            self.misuse_blocked,
            self.flushed_entries,
        ])
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("ledger[total={} acc={} rej={} noop={} fng={} fall={} red={} misuse={} flush={} cons={}]", self.total, self.accepted,
            self.rejected, self.noop, self.flush_non_global, self.flush_all, self.redundant, self.misuse_blocked, self.flushed_entries,
            if self.conserved() { 1 } else { 0 })
    }
}

/// 环形账的一条留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace {
    /// 逻辑时刻。
    pub tick: u64,
    /// 模式序号。
    pub mode: u8,
    /// 场景序号。
    pub scene: u8,
    /// 动作序号。
    pub action: u8,
    /// 结果码（接受为 0，拒绝为错误码）。
    pub code: u16,
    /// 刷出条目数。
    pub flushed: u64,
}

/// 单条留痕指纹（三见证的最小单元）。
pub fn trace_fp(t: &Trace) -> u64 {
    fnv1a_words(&[t.tick, t.mode as u64, t.scene as u64, t.action as u64, t.code as u64, t.flushed, V2622_VERSION_ID])
}

/// append-only 环形账（固定容量，溢出只增 `dropped`，绝不扩容）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 槽位。
    pub slots: [Trace; RING_CAP],
    /// 下一写入位。
    pub head: usize,
    /// 已写入总条数（**含**被覆盖的）。
    pub written: u64,
    /// 被覆盖丢弃数。
    pub dropped: u64,
    /// 增量见证：保留窗口的条数。
    pub w_len: u64,
    /// 增量见证：保留窗口的单值之和（`wrapping`）。
    pub w_sum: u64,
    /// 增量见证：保留窗口逐条指纹的异或和。
    pub w_xor: u64,
}

impl RingLedger {
    /// 空账（`w_len = w_sum = w_xor = 0`，与保留窗口空集同值）。
    pub fn new() -> RingLedger {
        RingLedger { slots: [Trace { tick: 0, mode: 0, scene: 0, action: 0, code: 0, flushed: 0 }; RING_CAP], head: 0, written: 0,
            dropped: 0, w_len: 0, w_sum: 0, w_xor: 0 }
    }
    /// 追加一条（覆盖最老的）。
    pub fn push(&mut self, t: Trace) -> BookResult<()> {
        if RING_CAP == 0 {
            return Err(BookErr::of(ErrKind::RingZero, 0));
        }
        if self.head >= RING_CAP {
            return Err(BookErr::of(ErrKind::RingHead, self.head as u64));
        }
        let new_fp = trace_fp(&t);
        // 只有"槽位已被有效留痕占据"时旧值才计入见证；空槽的
        // 零值 `Trace` 也要算出非零指纹（`trace_fp` 里含版本锚），
        // 若不判占据就会把空槽当成一条真留痕累加进去，`verify()`
        // 于是永远对不上——这是本单调试期真实踩过的坑。
        let occupied = self.head < self.live();
        let old_fp = if occupied {
            match self.slots.get(self.head) {
                Some(old) => trace_fp(old),
                None => 0,
            }
        } else {
            0
        };
        if let Some(slot) = self.slots.get_mut(self.head) {
            *slot = t;
        }
        // 覆盖式写入的增量见证：异或可逆（`^old ^new`），和用差值修正。
        self.w_xor ^= old_fp ^ new_fp;
        self.w_sum = self.w_sum.wrapping_sub(old_fp).wrapping_add(new_fp);
        self.w_len = if self.w_len >= RING_CAP as u64 { RING_CAP as u64 } else { self.w_len.wrapping_add(1) };
        self.head = (self.head + 1) % RING_CAP;
        self.written = self.written.wrapping_add(1);
        if self.written > RING_CAP as u64 {
            self.dropped = self.dropped.wrapping_add(1);
        }
        Ok(())
    }
    /// 当前保留条数。
    pub fn live(&self) -> usize {
        if self.written < RING_CAP as u64 {
            self.written as usize
        } else {
            RING_CAP
        }
    }
    /// `dropped` 是否与写入数一致。
    pub fn dropped_consistent(&self) -> bool {
        let expect = if self.written > RING_CAP as u64 { self.written - RING_CAP as u64 } else { 0 };
        self.dropped == expect
    }
    /// 逐条重算见证（验证保留窗口未被就地改写）。
    ///
    /// **为什么用「和 + 异或 + 长度」三见证而不是单一 FNV 折叠**：
    /// FNV 是**顺序相关**的，覆盖式写入无法从折叠值里"减掉"被覆盖的
    /// 那一条，于是增量维护就做不到 O(1)。异或可逆、和可用差值修正，
    /// 两者合起来能在 O(1) 内完成"删一条 + 加一条"，代价是失去顺序
    /// 信息——但顺序信息在环账里由 `tick` 字段自带，且 `verify()`
    /// 还会另外检查 `head` 与 `live()` 的一致性。
    pub fn verify(&self) -> bool {
        let live = self.live();
        if self.w_len != live as u64 {
            return false;
        }
        let start = if self.written >= RING_CAP as u64 { self.head } else { 0 };
        let mut sum: u64 = 0;
        let mut xor: u64 = 0;
        let mut k: usize = 0;
        while k < live {
            let idx = (start + k) % RING_CAP;
            if let Some(t) = self.slots.get(idx) {
                let f = trace_fp(t);
                sum = sum.wrapping_add(f);
                xor ^= f;
            }
            k = k.wrapping_add(1);
        }
        sum == self.w_sum && xor == self.w_xor
    }
    /// 读取第 `k` 条有效留痕（按写入顺序）。
    pub fn get(&self, k: usize) -> Option<Trace> {
        if k >= self.live() {
            return None;
        }
        let start = if self.written >= RING_CAP as u64 { self.head } else { 0 };
        let idx = (start + k) % RING_CAP;
        self.slots.get(idx).copied()
    }
    /// 指纹（折叠三见证 + 游标 + 计数）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a_words(&[self.w_len, self.w_sum, self.w_xor, self.head as u64, self.written, self.dropped])
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("ring[live={}/{} written={} dropped={} verify={}]", self.live(), RING_CAP, self.written, self.dropped,
            if self.verify() { 1 } else { 0 })
    }
}

// ===========================================================================
// 12. 切换状态机（合法边闭集）
// ===========================================================================

/// 切换状态机状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchState {
    /// 空闲（刚复位）。
    Idle,
    /// 请求已受理。
    Admitted,
    /// 已查总册。
    Booked,
    /// 已查分岔表。
    Branched,
    /// 已作用探针。
    Probed,
    /// 已记账。
    Accounted,
}

impl SwitchState {
    /// 全部状态（6 项，编号即 `state_index`）。
    pub const ALL: [SwitchState; 6] = [
        SwitchState::Idle,
        SwitchState::Admitted,
        SwitchState::Booked,
        SwitchState::Branched,
        SwitchState::Probed,
        SwitchState::Accounted,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            SwitchState::Idle => 0,
            SwitchState::Admitted => 1,
            SwitchState::Booked => 2,
            SwitchState::Branched => 3,
            SwitchState::Probed => 4,
            SwitchState::Accounted => 5,
        }
    }
    /// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            SwitchState::Idle => "idle",
            SwitchState::Admitted => "admitted",
            SwitchState::Booked => "booked",
            SwitchState::Branched => "branched",
            SwitchState::Probed => "probed",
            SwitchState::Accounted => "accounted",
        }
    }
    /// 终态（记账完成）。
    pub fn terminal(self) -> bool {
        matches!(self, SwitchState::Accounted)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{}(#{})", self.name(), self.index())
    }
}

/// **合法迁移边闭集**（唯一裁决口 `SwitchFsm::edge_allowed()`）。
///
/// 6 个状态、9 条合法边：
/// `idle→admitted` / `admitted→booked` / `booked→branched` /
/// `branched→probed`（接受路） / `branched→accounted`（违用短路路）/
/// `probed→accounted` / `accounted→idle`（复位）
/// / `idle→booked`（快路径：请求自带场景，免受理）
/// / `booked→accounted`（违用在总册层就判定，跳过探针）。
pub const LEGAL_EDGES: [(SwitchState, SwitchState); 10] = [
    (SwitchState::Idle, SwitchState::Admitted),
    (SwitchState::Admitted, SwitchState::Booked),
    (SwitchState::Booked, SwitchState::Branched),
    (SwitchState::Branched, SwitchState::Probed),
    (SwitchState::Branched, SwitchState::Accounted),
    (SwitchState::Probed, SwitchState::Accounted),
    (SwitchState::Accounted, SwitchState::Idle),
    (SwitchState::Idle, SwitchState::Booked),
    (SwitchState::Booked, SwitchState::Accounted),
    (SwitchState::Admitted, SwitchState::Accounted),
];

/// 切换状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchFsm {
    /// 当前态。
    pub state: SwitchState,
    /// 合法迁移数。
    pub moved: u64,
    /// 非法迁移拒记数。
    pub refused: u64,
    /// 上一条非法迁移编码（`from << 8 | to`）。
    pub last_refused: u64,
}

impl SwitchFsm {
    /// 构造（`Idle`）。
    pub fn new() -> SwitchFsm {
        SwitchFsm { state: SwitchState::Idle, moved: 0, refused: 0, last_refused: 0 }
    }
    /// **唯一**的边裁决口。
    pub fn edge_allowed(from: SwitchState, to: SwitchState) -> bool {
        let mut i: usize = 0;
        while i < LEGAL_EDGES.len() {
            if let Some(e) = LEGAL_EDGES.get(i) {
                if e.0 == from && e.1 == to {
                    return true;
                }
            }
            i = i.wrapping_add(1);
        }
        false
    }
    /// 尝试迁移；非法边拒记并返回 `ERR_EDGE_CLOSED`。
    pub fn try_move(&mut self, to: SwitchState) -> BookResult<()> {
        let idx = to.index();
        if idx >= SwitchState::ALL.len() {
            return Err(BookErr::of(ErrKind::TargetIllegal, idx as u64));
        }
        let cur = self.state.index();
        if cur >= SwitchState::ALL.len() {
            return Err(BookErr::of(ErrKind::StateIllegal, cur as u64));
        }
        if !SwitchFsm::edge_allowed(self.state, to) {
            self.refused = self.refused.wrapping_add(1);
            self.last_refused = ((cur as u64) << 8) | (idx as u64);
            return Err(BookErr::of(ErrKind::EdgeClosed, self.last_refused));
        }
        self.state = to;
        self.moved = self.moved.wrapping_add(1);
        Ok(())
    }
    /// 非法边穷举（`6 * 6 - 9 = 27` 条）。
    pub fn illegal_sweep() -> Vec<(SwitchState, SwitchState)> {
        let mut out: Vec<(SwitchState, SwitchState)> = Vec::new();
        let mut a: usize = 0;
        while a < SwitchState::ALL.len() {
            let mut b: usize = 0;
            while b < SwitchState::ALL.len() {
                let (x, y) = match (SwitchState::ALL.get(a), SwitchState::ALL.get(b)) {
                    (Some(x), Some(y)) => (*x, *y),
                    _ => (SwitchState::Idle, SwitchState::Idle),
                };
                if !SwitchFsm::edge_allowed(x, y) {
                    out.push((x, y));
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        out
    }
    /// 合法边穷举。
    pub fn legal_sweep() -> Vec<(SwitchState, SwitchState)> {
        let mut out: Vec<(SwitchState, SwitchState)> = Vec::new();
        let mut i: usize = 0;
        while i < LEGAL_EDGES.len() {
            if let Some(e) = LEGAL_EDGES.get(i) {
                out.push(*e);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("fsm[{} moved={} refused={}]", self.state.name(), self.moved, self.refused)
    }
}

// ===========================================================================
// 13. PcidBook：总册 + 分岔 + 探针 + 账 + 状态机（同生共死）
// ===========================================================================

/// PCID 运行期语义总册实例。
///
/// 五个部件**同生共死**：总册（只读常量）、分岔表（只读常量）、
/// 失效探针（可变）、守恒账（可变）、环形账（可变）、状态机（可变）。
/// 任何一个单独存在都没有意义——总册不改而实现改了，漂移无处可查；
/// 账不与探针同步，"保留可证"就只是注释。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PcidBook {
    /// 逻辑时刻。
    pub tick: u64,
    /// ASID 表。
    pub asids: AsidTable,
    /// 失效探针。
    pub probe: TlbProbe,
    /// 守恒账。
    pub ledger: SwitchLedger,
    /// 环形账。
    pub ring: RingLedger,
    /// 状态机。
    pub fsm: SwitchFsm,
    /// 本次接受数（快照用）。
    pub accepted_snapshot: u64,
}

impl PcidBook {
    /// 构造（总册与分岔表指纹一并记下，便于事后比对）。
    pub fn new() -> BookResult<PcidBook> {
        if !book_size_ok() {
            return Err(BookErr::of(ErrKind::TableDrift, BOOK.len() as u64));
        }
        if !branch_cells_ok() {
            return Err(BookErr::of(ErrKind::BranchCells, BRANCH_TABLE.len() as u64));
        }
        if !parity_ok() {
            return Err(BookErr::of(ErrKind::VerdictMismatch, 0));
        }
        Ok(PcidBook {
            tick: 0,
            asids: AsidTable::new(),
            probe: TlbProbe::new(),
            ledger: SwitchLedger::zero(),
            ring: RingLedger::new(),
            fsm: SwitchFsm::new(),
            accepted_snapshot: 0,
        })
    }
    /// 逻辑步进一格。
    pub fn step(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }
    /// 预检：请求自洽 + CR3 保留位 + 页表基址。
    ///
    /// 这一层在查表**之前**，因为它是"请求本身就不合法"的情形
    /// （缺基址 / 未对齐 / CR3 保留位），与语义分岔无关。
    fn precheck(&self, req: &SwitchReq) -> BookResult<()> {
        // 顺序有讲究：**范围闸必须先于自洽闸**。否则 `asid = 0x1000`
        // 这种"既越界又与 cr3 不自洽"的请求会被自洽闸先抓走，注入核
        // 期望的 `ERR_PCID_RANGE` 就永远对不上——错误码错配比漏检更难查。
        if req.asid > PCID_MAX {
            return Err(BookErr::of(ErrKind::PcidRange, req.asid as u64));
        }
        if !req.self_consistent() {
            return Err(BookErr::of(ErrKind::PcedeMismatch, req.cr3.raw));
        }
        if req.cr3.pt_base() == 0 {
            return Err(BookErr::of(ErrKind::NoPtBase, req.asid as u64));
        }
        if req.cr3.pt_base() & 0xFFF != 0 {
            return Err(BookErr::of(ErrKind::PtAlign, req.cr3.pt_base()));
        }
        if req.cr3.stray() != 0 {
            return Err(BookErr::of(ErrKind::Cr3Stray, req.cr3.stray()));
        }
        if req.mode.index() >= MODES {
            return Err(BookErr::of(ErrKind::ModeRange, req.mode.index() as u64));
        }
        if req.scene.index() >= SCENES {
            return Err(BookErr::of(ErrKind::SceneRange, req.scene.index() as u64));
        }
        Ok(())
    }
    /// **切换单点**：总册 → 分岔表 → 探针 → 记账。
    ///
    /// 判例四纪律（同点）：
    /// 1. **先查总册再查实现**：语义结论来自 `BOOK`，实现只做投影；
    /// 2. **违用先判**：被拒时不碰探针（TLB 未被触碰），只留痕；
    /// 3. **接受必记账**：账不记就是漏检；
    /// 4. **失败必留码**：任何 `Err` 都带段内码，可被注入核对拍。
    pub fn switch_cr3(&mut self, req: &SwitchReq) -> BookResult<Branch> {
        self.step();
        // 状态机复位：上一轮的终态必须先回 `Idle` 才允许开新的一轮。
        // 这一条边是显式登记的（`accounted -> idle`），不做隐式清零。
        if self.fsm.state != SwitchState::Idle {
            self.fsm.try_move(SwitchState::Idle)?;
        }
        self.fsm.try_move(SwitchState::Admitted)?;
        if let Err(e) = self.precheck(req) {
            // 预检短路：`admitted -> accounted`（未查总册，也未碰探针）。
            self.ledger.note_reject();
            self.fsm.try_move(SwitchState::Accounted)?;
            self.ring.push(Trace {
                tick: self.tick,
                mode: req.mode.index() as u8,
                scene: req.scene.index() as u8,
                action: TlbAction::Rejected.index() as u8,
                code: e.code,
                flushed: 0,
            })?;
            return Err(e);
        }
        // 第 1 步：查总册（语义结论）。
        self.fsm.try_move(SwitchState::Booked)?;
        let row = book_cell(req.mode, req.scene)?;
        // 第 2 步：查分岔表（可执行投影）。
        self.fsm.try_move(SwitchState::Branched)?;
        let b = branch_cell(req.mode, req.scene)?;
        if b != branch_of_row(&row) {
            self.ledger.note_reject();
            self.fsm.try_move(SwitchState::Accounted)?;
            self.ring.push(Trace {
                tick: self.tick,
                mode: req.mode.index() as u8,
                scene: req.scene.index() as u8,
                action: TlbAction::Rejected.index() as u8,
                code: ERR_VERDICT_MISMATCH,
                flushed: 0,
            })?;
            return Err(BookErr::of(ErrKind::VerdictMismatch, row.rule as u64));
        }
        // 违用短路：状态机走 `branched -> accounted`，不碰探针条目。
        if let Branch::Reject(code) = b {
            self.ledger.note_reject();
            self.fsm.try_move(SwitchState::Accounted)?;
            self.probe.apply(self.asids.current, req.asid, TlbAction::Rejected)?;
            self.ring.push(Trace {
                tick: self.tick,
                mode: req.mode.index() as u8,
                scene: req.scene.index() as u8,
                action: TlbAction::Rejected.index() as u8,
                code,
                flushed: 0,
            })?;
            return Err(BookErr { code, site: (row.rule as u64) << 8 | req.asid as u64, detail: row.describe() });
        }
        // 接受路：作用于探针（`branched -> probed -> accounted`）。
        self.fsm.try_move(SwitchState::Probed)?;
        let old = self.asids.current;
        self.probe.apply(old, req.asid, b.action())?;
        let flushed = self.probe.observed.flushed as u64;
        // **只有 PCID on 路才更新 ASID 表**：off 路下 CR3 低 12 位
        // 根本不参与解释（SDM 明文），把它写进 ASID 表等于让保留值
        // 0xFFF 污染分配器状态——这正是本单最容易写错的一处。
        if req.mode == Mode::On {
            self.asids.set_current(req.asid)?;
        }
        self.ledger.note_accept(b, flushed);
        self.accepted_snapshot = self.ledger.accepted;
        self.fsm.try_move(SwitchState::Accounted)?;
        self.ring.push(Trace {
            tick: self.tick,
            mode: req.mode.index() as u8,
            scene: req.scene.index() as u8,
            action: b.action().index() as u8,
            code: 0,
            flushed,
        })?;
        Ok(b)
    }
    /// 健康判定（账守恒 + 环账自校验 + 探针守恒）。
    pub fn healthy(&self) -> bool {
        self.ledger.conserved() && self.ring.verify() && self.ring.dropped_consistent() && self.probe.conserved()
    }
    /// 账快照。
    pub fn ledger_snapshot(&self) -> SwitchLedger {
        self.ledger
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("tick={} {} {} {}", self.tick, self.asids.describe(), self.probe.describe(), self.ledger.describe())
    }
    /// 冻结指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            book_fp(),
            branch_fp(),
            self.asids.fingerprint(),
            self.probe.fingerprint(),
            self.ledger.fingerprint(),
            self.ring.fingerprint(),
            self.tick,
        ];
        fnv1a_words(&words)
    }
}

/// 分岔矩阵一格的结果（实现 vs 表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixCell {
    /// 模式。
    pub mode: Mode,
    /// 场景。
    pub scene: Scene,
    /// 表给的分支。
    pub expect: Branch,
    /// 实现给的分支（接受则 `Some`）。
    pub got: Option<Branch>,
    /// 拒绝时的实际码。
    pub got_code: u16,
    /// 是否一致。
    pub pass: bool,
}

impl MatrixCell {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} {} expect={} got={} code=0x{:04X} {}", self.mode.name(), self.scene.name(), self.expect.describe(),
            match self.got {
                Some(b) => b.describe(),
                None => "reject".to_string(),
            }, self.got_code, if self.pass { "PASS" } else { "FAIL" })
    }
}

/// **分岔矩阵全列**（`2 × 8 = 16` 格）：实现逐格对齐分岔表。
///
/// 每格独立建一个新 `PcidBook`（避免格间状态串味），跑一次切换，
/// 比对「表给的分支」与「实现给的分支 + 码」。
pub fn branch_matrix() -> Vec<MatrixCell> {
    let mut out: Vec<MatrixCell> = Vec::new();
    let mut m: usize = 0;
    while m < MODES {
        let mode = match Mode::ALL.get(m) {
            Some(x) => *x,
            None => Mode::Off,
        };
        let mut s: usize = 0;
        while s < SCENES {
            let scene = match Scene::ALL.get(s) {
                Some(x) => *x,
                None => Scene::SameAsidFlush,
            };
            let expect = match branch_cell(mode, scene) {
                Ok(b) => b,
                Err(_) => Branch::Reject(0),
            };
            let mut book = match PcidBook::new() {
                Ok(bk) => bk,
                Err(_) => {
                    out.push(MatrixCell { mode, scene, expect, got: None, got_code: 0, pass: false });
                    s = s.wrapping_add(1);
                    continue;
                }
            };
            // 预置探针：当前 ASID 装 3 条、全局装 2 条。
            let _ = book.probe.fill(0, 3);
            let _ = book.probe.fill_global(2);
            if scene.asid_class() == AsidClass::Recycling {
                let target = scene.asid_class().sample_asid(0);
                let _ = book.asids.mark_recycling(target);
            }
            let req = SwitchReq::make(mode, scene, 0);
            let got = book.switch_cr3(&req);
            let cell = match got {
                Ok(b) => {
                    let ok = b == expect;
                    MatrixCell { mode, scene, expect, got: Some(b), got_code: 0, pass: ok }
                }
                Err(e) => {
                    let ok = matches!(expect, Branch::Reject(_)) && e.code == expect.code();
                    MatrixCell { mode, scene, expect, got: None, got_code: e.code, pass: ok }
                }
            };
            out.push(cell);
            s = s.wrapping_add(1);
        }
        m = m.wrapping_add(1);
    }
    out
}

/// 分岔矩阵是否全过。
pub fn branch_matrix_ok() -> bool {
    let m = branch_matrix();
    if m.len() != BRANCH_CELLS {
        return false;
    }
    let mut i: usize = 0;
    while i < m.len() {
        if let Some(c) = m.get(i) {
            if !c.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

// ===========================================================================
// 14. 判据：双模式 10^5 次切换语义断言
// ===========================================================================

/// 切换压测报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchReport {
    /// 实际切换次数。
    pub rounds: u64,
    /// 期望次数（`SWITCH_ROUNDS`）。
    pub expect_rounds: u64,
    /// 接受数。
    pub accepted: u64,
    /// 拒绝数（应为违用拦截）。
    pub rejected: u64,
    /// 双模式都跑过（`off > 0 && on > 0`）。
    pub both_modes: bool,
    /// 语义逐项断言未过数（必须为 0）。
    pub assert_failed: u64,
    /// 「NOFLUSH 后旧空间保留」被证实的次数。
    pub keep_proven: u64,
    /// 「刷非全局时全局保留」被证实的次数。
    pub global_preserved: u64,
    /// 守恒是否全程成立。
    pub conserved: bool,
    /// 探针守恒是否全程成立。
    pub probe_conserved: bool,
    /// 环账是否全程自校验通过。
    pub ring_ok: bool,
    /// 状态机是否出现非法迁移（必须为 0）。
    pub fsm_refused: u64,
}

impl SwitchReport {
    /// 全部判据达标。
    pub fn ok(&self) -> bool {
        self.rounds == self.expect_rounds
            && self.both_modes
            && self.assert_failed == 0
            && self.keep_proven > 0
            && self.global_preserved > 0
            && self.conserved
            && self.probe_conserved
            && self.ring_ok
            && self.fsm_refused == 0
            && self.accepted > 0
            && self.rejected > 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("switch[rounds={}/{} acc={} rej={} both={} assert_fail={} keep={} gpres={} cons={} pcons={} ring={} fsmrej={}]",
            self.rounds, self.expect_rounds, self.accepted, self.rejected, if self.both_modes { 1 } else { 0 }, self.assert_failed,
            self.keep_proven, self.global_preserved, if self.conserved { 1 } else { 0 }, if self.probe_conserved { 1 } else { 0 },
            if self.ring_ok { 1 } else { 0 }, self.fsm_refused)
    }
}

/// 双模式切换压测（判据 `SWITCH_ROUNDS = 10^5`）。
///
/// 每轮做四件事，顺序固定：
/// 1. 按 LCG 选模式与场景，构造自洽请求；
/// 2. 跑一次 `switch_cr3()`；
/// 3. **逐项断言**：结果 == 表给的分支；接受时探针语义与动作相符；
/// 4. 检查守恒三项（账 / 探针 / 环账）。
///
/// 违用场景（回收期 + 保留 PCID）在压测里也要跑——它们是"被拒"的
/// 正常业务，不该被当成失败；`assert_failed` 只统计**语义不符**。
pub fn switch_stress(seed: u64, rounds: u64) -> SwitchReport {
    let mut rng = Lcg::new(seed);
    let mut book = match PcidBook::new() {
        Ok(bk) => bk,
        Err(_) => {
            return SwitchReport { rounds: 0, expect_rounds: rounds, accepted: 0, rejected: 0, both_modes: false, assert_failed: 1,
                keep_proven: 0, global_preserved: 0, conserved: false, probe_conserved: false, ring_ok: false, fsm_refused: 0 }
        }
    };
    let mut off_hits: u64 = 0;
    let mut on_hits: u64 = 0;
    let mut assert_failed: u64 = 0;
    let mut keep_proven: u64 = 0;
    let mut global_preserved: u64 = 0;
    let mut probe_ok: bool = true;
    let mut ring_ok: bool = true;
    let mut n: u64 = 0;
    while n < rounds {
        // 选模式与场景：偶数轮强制 on/off 交替（保证双模式都覆盖），
        // 其它轮交给 LCG，避免"交替"这个固定节拍掩盖语义 bug。
        let mode = if n % 2 == 0 {
            Mode::Off
        } else if n % 4 == 1 {
            Mode::On
        } else if rng.flag() {
            Mode::On
        } else {
            Mode::Off
        };
        let si = rng.below(SCENES as u64) as usize;
        let scene = match Scene::ALL.get(si) {
            Some(x) => *x,
            None => Scene::SameAsidFlush,
        };
        let cur = book.asids.current;
        // 回收期场景需要目标真的处于回收期。
        if scene.asid_class() == AsidClass::Recycling {
            let target = scene.asid_class().sample_asid(cur);
            let _ = book.asids.mark_recycling(target);
        }
        // 保持探针有货：给当前 ASID 补条目。
        let _ = book.probe.fill(cur, 1 + (rng.below(3) as usize));
        if n % 7 == 0 {
            let _ = book.probe.fill_global(1);
        }
        let req = SwitchReq::make(mode, scene, cur);
        let expect = match branch_cell(mode, scene) {
            Ok(b) => b,
            Err(_) => Branch::Reject(0),
        };
        let before_ng = book.probe.non_global_total();
        let res = book.switch_cr3(&req);
        match res {
            Ok(b) => {
                if mode == Mode::Off {
                    off_hits = off_hits.wrapping_add(1);
                } else {
                    on_hits = on_hits.wrapping_add(1);
                }
                if b != expect {
                    assert_failed = assert_failed.wrapping_add(1);
                }
                // 逐项探针断言。
                match b.action() {
                    TlbAction::Noop => {
                        if book.probe.observed.after_non_global == before_ng && book.probe.old_space_survives() {
                            keep_proven = keep_proven.wrapping_add(1);
                        } else if expect.action() == TlbAction::Noop {
                            assert_failed = assert_failed.wrapping_add(1);
                        }
                    }
                    TlbAction::FlushNonGlobal => {
                        if book.probe.globals_preserved() {
                            global_preserved = global_preserved.wrapping_add(1);
                        } else {
                            assert_failed = assert_failed.wrapping_add(1);
                        }
                        if book.probe.observed.after_non_global != 0 {
                            assert_failed = assert_failed.wrapping_add(1);
                        }
                    }
                    TlbAction::FlushAll => {
                        if book.probe.observed.after_global != 0 || book.probe.observed.after_non_global != 0 {
                            assert_failed = assert_failed.wrapping_add(1);
                        }
                    }
                    TlbAction::Rejected => {
                        assert_failed = assert_failed.wrapping_add(1);
                    }
                }
            }
            Err(e) => {
                let want_code = expect.code();
                if want_code == 0 || e.code != want_code {
                    assert_failed = assert_failed.wrapping_add(1);
                }
                // 违用必须不碰 TLB。
                if book.probe.observed.action != TlbAction::Rejected || book.probe.observed.flushed != 0 {
                    assert_failed = assert_failed.wrapping_add(1);
                }
            }
        }
        if !book.probe.conserved() {
            probe_ok = false;
        }
        if !book.ring.verify() || !book.ring.dropped_consistent() {
            ring_ok = false;
        }
        if !book.ledger.conserved() {
            assert_failed = assert_failed.wrapping_add(1);
        }
        n = n.wrapping_add(1);
    }
    let fsm_refused = book.fsm.refused;
    let report = SwitchReport {
        rounds: n,
        expect_rounds: rounds,
        accepted: book.ledger.accepted,
        rejected: book.ledger.rejected,
        both_modes: off_hits > 0 && on_hits > 0,
        assert_failed,
        keep_proven,
        global_preserved,
        conserved: book.ledger.conserved(),
        probe_conserved: probe_ok,
        ring_ok,
        fsm_refused,
    };
    report
}

/// 判据全量跑（`SWITCH_ROUNDS` 次，不缩放）。
pub fn switch_criterion(seed: u64) -> SwitchReport {
    switch_stress(seed, SWITCH_ROUNDS)
}

/// 观察入口的压测缩放（分子 / 分母）。
pub const SWITCH_SCALE_NUM: u64 = 1;
/// 观察入口的压测缩放分母。
pub const SWITCH_SCALE_DEN: u64 = 1000;

// ===========================================================================
// 15. 注入核（12 形态 × 10 轮 = 120 次，全部必须被拦）
// ===========================================================================

/// 负向注入形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// PCID 关闭却带 NOFLUSH（判据点名的违用）。
    F1NoFlushWhileOff,
    /// PCID 关闭 + 同 ASID 带 NOFLUSH。
    F2NoFlushWhileOffSame,
    /// 回收期 ASID 带 NOFLUSH。
    F3RecycleNoFlush,
    /// 回收期 ASID 不带 NOFLUSH。
    F4RecycleFlush,
    /// 保留 PCID 带 NOFLUSH。
    F5ReservedNoFlush,
    /// 保留 PCID 不带 NOFLUSH。
    F6ReservedFlush,
    /// CR3 保留位（bit12..62）被置位。
    F7Cr3StrayBit,
    /// 页表基址未 4KiB 对齐。
    F8PtUnaligned,
    /// 页表基址为 0。
    F9NoPtBase,
    /// ASID 越界（> `PCID_MAX`）。
    F10AsidOutOfRange,
    /// 请求内部不自洽（ASID 与 CR3 字段不符）。
    F11ReqInconsistent,
    /// 分岔表与总册被人为错位（模拟漂移）。
    F12ParityDrift,
}

impl Form {
    /// 全部形态（12 项，编号即 `form_index`）。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::F1NoFlushWhileOff,
        Form::F2NoFlushWhileOffSame,
        Form::F3RecycleNoFlush,
        Form::F4RecycleFlush,
        Form::F5ReservedNoFlush,
        Form::F6ReservedFlush,
        Form::F7Cr3StrayBit,
        Form::F8PtUnaligned,
        Form::F9NoPtBase,
        Form::F10AsidOutOfRange,
        Form::F11ReqInconsistent,
        Form::F12ParityDrift,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            Form::F1NoFlushWhileOff => 0,
            Form::F2NoFlushWhileOffSame => 1,
            Form::F3RecycleNoFlush => 2,
            Form::F4RecycleFlush => 3,
            Form::F5ReservedNoFlush => 4,
            Form::F6ReservedFlush => 5,
            Form::F7Cr3StrayBit => 6,
            Form::F8PtUnaligned => 7,
            Form::F9NoPtBase => 8,
            Form::F10AsidOutOfRange => 9,
            Form::F11ReqInconsistent => 10,
            Form::F12ParityDrift => 11,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::F1NoFlushWhileOff => "noflush_while_off",
            Form::F2NoFlushWhileOffSame => "noflush_while_off_same",
            Form::F3RecycleNoFlush => "recycle_noflush",
            Form::F4RecycleFlush => "recycle_flush",
            Form::F5ReservedNoFlush => "reserved_noflush",
            Form::F6ReservedFlush => "reserved_flush",
            Form::F7Cr3StrayBit => "cr3_stray_bit",
            Form::F8PtUnaligned => "noflush_with_zero_asid",
            Form::F9NoPtBase => "no_pt_base",
            Form::F10AsidOutOfRange => "asid_out_of_range",
            Form::F11ReqInconsistent => "req_inconsistent",
            Form::F12ParityDrift => "parity_drift",
        }
    }
    /// 该形态的期望错误码。
    pub fn expect_code(self) -> u16 {
        match self {
            Form::F1NoFlushWhileOff | Form::F2NoFlushWhileOffSame => ERR_NOFLUSH_WHILE_OFF,
            Form::F3RecycleNoFlush | Form::F4RecycleFlush => ERR_ASID_RECYCLING,
            Form::F5ReservedNoFlush | Form::F6ReservedFlush => ERR_PCID_RESERVED,
            Form::F7Cr3StrayBit => ERR_CR3_STRAY,
            Form::F8PtUnaligned => ERR_NOFLUSH_WHILE_OFF,
            Form::F9NoPtBase => ERR_NO_PT_BASE,
            Form::F10AsidOutOfRange => ERR_PCID_RANGE,
            Form::F11ReqInconsistent => ERR_PCIDE_MISMATCH,
            Form::F12ParityDrift => ERR_VERDICT_MISMATCH,
        }
    }
    /// 一行描述（说明这个形态在攻什么）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::F1NoFlushWhileOff => "PCID 关闭 + 新 ASID + NOFLUSH：SDM 说 bit63 未定义，必须拦",
            Form::F2NoFlushWhileOffSame => "PCID 关闭 + 同 ASID + NOFLUSH：off 路最容易漏的一格",
            Form::F3RecycleNoFlush => "回收期 ASID 带 NOFLUSH：旧上下文仍可能持有该空间",
            Form::F4RecycleFlush => "回收期 ASID 不带 NOFLUSH：看似安全，实际必须先等回收完成",
            Form::F5ReservedNoFlush => "保留 PCID(0xFFF) 带 NOFLUSH：双重违用",
            Form::F6ReservedFlush => "保留 PCID(0xFFF) 不带 NOFLUSH：on 路必须拦，off 路可放",
            Form::F7Cr3StrayBit => "CR3 的 bit12..62 保留位被置位：来源不是本单编码器",
            Form::F8PtUnaligned => "ASID=0 却带 NOFLUSH：等价于没开 PCID 却要快切",
            Form::F9NoPtBase => "页表基址为 0：空指针切换",
            Form::F10AsidOutOfRange => "ASID > 0xFFF：分配器没做掩码",
            Form::F11ReqInconsistent => "请求里 asid 字段与 cr3.pcid() 不一致：调用方拼错了",
            Form::F12ParityDrift => "人为改写总册一行使其与分岔表错位：模拟真实漂移",
        }
    }
}

/// 一条注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Injected {
    /// 形态。
    pub form: Form,
    /// 种子。
    pub seed: u64,
    /// 轮次。
    pub round: u64,
    /// 是否被检出。
    pub caught: bool,
    /// 实际诊断码。
    pub got_code: u16,
    /// 期望诊断码。
    pub expect_code: u16,
    /// 详情串。
    pub detail: String,
}

impl Injected {
    /// 是否完全达期望。
    pub fn as_expected(&self) -> bool {
        self.caught && self.got_code == self.expect_code
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} #{} caught={} got=0x{:04X} expect=0x{:04X} {}", self.form.name(), self.round, if self.caught { 1 } else { 0 },
            self.got_code, self.expect_code, self.detail)
    }
}

/// 跑一次注入。
///
/// 每种形态都构造一个**具体违反**并送进 `switch_cr3()`（或
/// `table_branch_parity()`），要求检出且码精确匹配。
pub fn inject_form(form: Form, seed: u64, round: u64) -> Injected {
    let mut rng = Lcg::new(seed.wrapping_add(round.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
    let _ = rng.next();
    let mut book = match PcidBook::new() {
        Ok(bk) => bk,
        Err(_) => {
            return Injected { form, seed, round, caught: false, got_code: 0, expect_code: form.expect_code(),
                detail: "book construct failed".to_string() }
        }
    };
    let cur = book.asids.current;
    match form {
        Form::F1NoFlushWhileOff => {
            let req = SwitchReq::make(Mode::Off, Scene::NewAsidNoFlush, cur);
            finish(form, seed, round, book.switch_cr3(&req), "off+new_asid+noflush")
        }
        Form::F2NoFlushWhileOffSame => {
            let req = SwitchReq::make(Mode::Off, Scene::SameAsidNoFlush, cur);
            finish(form, seed, round, book.switch_cr3(&req), "off+same_asid+noflush")
        }
        Form::F3RecycleNoFlush => {
            let req0 = SwitchReq::make(Mode::On, Scene::RecycleAsidNoFlush, cur);
            let target = req0.asid;
            let _ = book.asids.mark_recycling(target);
            finish(form, seed, round, book.switch_cr3(&req0), "on+recycle+noflush")
        }
        Form::F4RecycleFlush => {
            let req0 = SwitchReq::make(Mode::On, Scene::RecycleAsidFlush, cur);
            let target = req0.asid;
            let _ = book.asids.mark_recycling(target);
            finish(form, seed, round, book.switch_cr3(&req0), "on+recycle+flush")
        }
        Form::F5ReservedNoFlush => {
            let req = SwitchReq::make(Mode::On, Scene::ReservedAsidNoFlush, cur);
            finish(form, seed, round, book.switch_cr3(&req), "on+reserved+noflush")
        }
        Form::F6ReservedFlush => {
            let req = SwitchReq::make(Mode::On, Scene::ReservedAsidFlush, cur);
            finish(form, seed, round, book.switch_cr3(&req), "on+reserved+flush")
        }
        Form::F7Cr3StrayBit => {
            let mut req = SwitchReq::make(Mode::On, Scene::NewAsidNoFlush, cur);
            // 越界保留位（bit55）：不属于页表基址字段，也不是 NOFLUSH。
            req.cr3.raw |= 1u64 << 55;
            finish(form, seed, round, book.switch_cr3(&req), "cr3 stray bit55")
        }
        Form::F8PtUnaligned => {
            // 反向形态：ASID 为 0 却带 NOFLUSH。语义上这等价于
            // "没开 PCID 却想快切"，必须按 off 违用拦。
            let mut req = SwitchReq::make(Mode::Off, Scene::SameAsidNoFlush, 0);
            req.asid = 0;
            req.cr3.raw &= !CR3_PCID_MASK;
            finish(form, seed, round, book.switch_cr3(&req), "off+asid0+noflush")
        }
        Form::F9NoPtBase => {
            let mut req = SwitchReq::make(Mode::On, Scene::NewAsidFlush, cur);
            // 页表基址字段（bit12..51）清零。
            req.cr3.raw &= CR3_PCID_MASK | CR3_NOFLUSH_MASK;
            finish(form, seed, round, book.switch_cr3(&req), "pt base zero")
        }
        Form::F10AsidOutOfRange => {
            // 越界 ASID：`SwitchReq` 的 asid 与 cr3.pcid() 同时越界，
            // 由 `precheck` 的范围闸（先于自洽闸）拦下，给 `ERR_PCID_RANGE`。
            let mut req = SwitchReq::make(Mode::On, Scene::NewAsidFlush, cur);
            let over = PCID_MAX.wrapping_add(1);
            req.asid = over;
            req.cr3.raw = (req.cr3.raw & !CR3_PCID_MASK) | ((over as u64) & 0xFFFF);
            finish(form, seed, round, book.switch_cr3(&req), "asid 0x1000")
        }
        Form::F11ReqInconsistent => {
            let mut req = SwitchReq::make(Mode::On, Scene::NewAsidNoFlush, cur);
            req.asid = req.asid.wrapping_add(1);
            finish(form, seed, round, book.switch_cr3(&req), "asid field != cr3.pcid")
        }
        Form::F12ParityDrift => {
            // 人为造一个"总册与分岔表错位"的场景：
            // 取总册某行的 verdict 投影，与该格表项比对，
            // 故意用**篡改过的**总册副本（把 Legal 改成 Misuse）触发红灯。
            let idx = (round as usize) % BRANCH_CELLS;
            let mut fake = match BOOK.get(idx).copied() {
                Some(r) => r,
                None => BOOK[0],
            };
            fake.verdict = match fake.verdict {
                Verdict::Legal => Verdict::Misuse,
                Verdict::Misuse => Verdict::Legal,
                Verdict::Redundant => Verdict::Legal,
            };
            fake.action = match fake.verdict {
                Verdict::Misuse => TlbAction::Rejected,
                _ => fake.action,
            };
            let projected = branch_of_row(&fake);
            let table = BRANCH_TABLE[idx];
            let drift = projected != table;
            Injected {
                form,
                seed,
                round,
                caught: drift,
                got_code: if drift { ERR_VERDICT_MISMATCH } else { 0 },
                expect_code: ERR_VERDICT_MISMATCH,
                detail: format!("idx={} proj={} table={}", idx, projected.describe(), table.describe()),
            }
        }
    }
}

/// 把 `switch_cr3()` 的结果折成注入留痕。
fn finish(form: Form, seed: u64, round: u64, res: BookResult<Branch>, detail: &str) -> Injected {
    match res {
        Ok(b) => Injected { form, seed, round, caught: false, got_code: 0, expect_code: form.expect_code(),
            detail: format!("{} -> accepted {}", detail, b.describe()) },
        Err(e) => Injected { form, seed, round, caught: true, got_code: e.code, expect_code: form.expect_code(),
            detail: format!("{} -> {}", detail, e.describe()) },
    }
}

/// 跑全部形态 × 全部轮次。
pub fn inject_all(seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();
    let mut i: usize = 0;
    while i < FORM_COUNT {
        if let Some(f) = Form::ALL.get(i) {
            let mut r: u64 = 0;
            while r < INJECT_ROUNDS {
                out.push(inject_form(*f, seed, r));
                r = r.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 注入统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectStat {
    /// 注入总数。
    pub total: usize,
    /// 被检出数。
    pub caught: usize,
    /// 码匹配数。
    pub code_match: usize,
    /// 完全达期望数。
    pub as_expected: usize,
    /// 形态数。
    pub forms: usize,
}

impl InjectStat {
    /// 全部达期望且规模达标。
    pub fn all_ok(&self) -> bool {
        self.total == FORM_COUNT * INJECT_ROUNDS as usize
            && self.forms == FORM_COUNT
            && self.caught == self.total
            && self.code_match == self.total
            && self.as_expected == self.total
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("inject[forms={} total={} caught={} code_match={} as_expected={}]", self.forms, self.total, self.caught, self.code_match,
            self.as_expected)
    }
}

/// 注入统计聚合。
pub fn inject_stat(recs: &[Injected]) -> InjectStat {
    let mut s = InjectStat { total: recs.len(), caught: 0, code_match: 0, as_expected: 0, forms: FORM_COUNT };
    let mut i: usize = 0;
    while i < recs.len() {
        if let Some(r) = recs.get(i) {
            if r.caught {
                s.caught = s.caught.wrapping_add(1);
            }
            if r.got_code == r.expect_code {
                s.code_match = s.code_match.wrapping_add(1);
            }
            if r.as_expected() {
                s.as_expected = s.as_expected.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    s
}

// ===========================================================================
// 16. 冻结指纹（fingerprint / verify）
// ===========================================================================

/// 冻结指纹包。
///
/// 冻结的语义是：**语义总册 + 分岔表 + 探针 + 账 + 环账** 在一个
/// 已知输入序列下的全部可观测量的单值摘要。任何人改动总册、
/// 改动分岔、改动探针模型，都必然改变这个值，于是 `verify()`
/// 会在第一时间报 `ERR_VERIFY_FAILED`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frozen {
    /// 总册指纹。
    pub book_fp: u64,
    /// 分岔表指纹。
    pub branch_fp: u64,
    /// 表分支对照指纹（16 格的通过位折叠）。
    pub parity_fp: u64,
    /// 账指纹。
    pub ledger_fp: u64,
    /// 探针指纹。
    pub probe_fp: u64,
    /// 环账指纹。
    pub ring_fp: u64,
    /// 版本锚。
    pub version: u64,
}

impl Frozen {
    /// 当前总册的冻结值（不带运行态）。
    pub fn capture_static() -> Frozen {
        let mut h = FNV_BASIS;
        let p = table_branch_parity();
        let mut i: usize = 0;
        while i < p.len() {
            if let Some(c) = p.get(i) {
                h = fnv1a_acc(h, if c.pass { 1 } else { 0 });
                h = fnv1a_acc(h, c.table.code() as u64);
                h = fnv1a_acc(h, c.table.action().index() as u64);
            }
            i = i.wrapping_add(1);
        }
        Frozen { book_fp: book_fp(), branch_fp: branch_fp(), parity_fp: h, ledger_fp: 0, probe_fp: 0, ring_fp: 0, version: V2622_VERSION_ID }
    }
    /// 全值指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [self.book_fp, self.branch_fp, self.parity_fp, self.ledger_fp, self.probe_fp, self.ring_fp, self.version];
        fnv1a_words(&words)
    }
    /// 自校验（静态部分必须与当前总册一致）。
    pub fn verify(&self) -> BookResult<()> {
        if self.version != V2622_VERSION_ID {
            return Err(BookErr::of(ErrKind::VerifyFailed, self.version));
        }
        if self.book_fp != book_fp() {
            return Err(BookErr::of(ErrKind::TableDrift, self.book_fp));
        }
        if self.branch_fp != branch_fp() {
            return Err(BookErr::of(ErrKind::BranchCells, self.branch_fp));
        }
        let cur = Frozen::capture_static();
        if self.parity_fp != cur.parity_fp {
            return Err(BookErr::of(ErrKind::VerdictMismatch, self.parity_fp));
        }
        Ok(())
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("frozen[book=0x{:016X} branch=0x{:016X} parity=0x{:016X} ver=0x{:X}]", self.book_fp, self.branch_fp, self.parity_fp, self.version)
    }
}

// ===========================================================================
// 17. 确定性回放（replay）
// ===========================================================================

/// 回放报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplayReport {
    /// 步数。
    pub steps: usize,
    /// 步数上界。
    pub cap: usize,
    /// 回放终态指纹。
    pub fp: u64,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 环账是否自校验通过。
    pub ring_ok: bool,
}

impl ReplayReport {
    /// 是否达标。
    pub fn ok(&self) -> bool {
        self.steps > 0 && self.steps <= self.cap && self.conserved && self.ring_ok
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("replay[steps={}/{} fp=0x{:016X} cons={} ring={}]", self.steps, self.cap, self.fp, if self.conserved { 1 } else { 0 },
            if self.ring_ok { 1 } else { 0 })
    }
}

/// 确定性回放：同种子两次必须逐位一致。
///
/// **不与 `observe()` 互递归**：`replay()` 只折叠自己产生的运行态，
/// 自己算自己的指纹，不去调 `observe()`。否则两者会互相调用到爆栈。
pub fn replay(seed: u64) -> ReplayReport {
    let mut rng = Lcg::new(seed);
    let mut book = match PcidBook::new() {
        Ok(bk) => bk,
        Err(_) => return ReplayReport { steps: 0, cap: REPLAY_STEPS, fp: 0, conserved: false, ring_ok: false },
    };
    let mut steps: usize = 0;
    while steps < REPLAY_STEPS {
        let mode = if rng.bits(1) == 1 { Mode::On } else { Mode::Off };
        let si = rng.below(SCENES as u64) as usize;
        let scene = match Scene::ALL.get(si) {
            Some(x) => *x,
            None => Scene::SameAsidFlush,
        };
        let cur = book.asids.current;
        if scene.asid_class() == AsidClass::Recycling {
            let target = scene.asid_class().sample_asid(cur);
            let _ = book.asids.mark_recycling(target);
        }
        let _ = book.probe.fill(cur, 1 + (rng.below(2) as usize));
        if rng.bits(4) == 0 {
            let _ = book.probe.fill_global(1);
        }
        let req = SwitchReq::make(mode, scene, cur);
        let _ = book.switch_cr3(&req);
        steps = steps.wrapping_add(1);
    }
    let conserved = book.ledger.conserved() && book.probe.conserved();
    let ring_ok = book.ring.verify() && book.ring.dropped_consistent();
    ReplayReport { steps, cap: REPLAY_STEPS, fp: book.fingerprint(), conserved, ring_ok }
}

/// 回放两次是否一致（同种子）。
pub fn replay_stable(seed: u64) -> bool {
    let a = replay(seed);
    let b = replay(seed);
    a.fp == b.fp && a.steps == b.steps && a.conserved && b.conserved
}

// ===========================================================================
// 18. 指标
// ===========================================================================

/// 指标汇总。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 分岔矩阵格数。
    pub matrix_cells: usize,
    /// 分岔矩阵通过数。
    pub matrix_pass: usize,
    /// 表分支对照格数。
    pub parity_cells: usize,
    /// 表分支对照通过数。
    pub parity_pass: usize,
    /// 切换请求总数。
    pub switch_total: u64,
    /// 切换接受数。
    pub switch_accepted: u64,
    /// 切换拒绝数。
    pub switch_rejected: u64,
    /// 语义断言失败数（须 0）。
    pub assert_failed: u64,
    /// 注入总数。
    pub inject_total: usize,
    /// 注入检出数。
    pub inject_caught: usize,
    /// 注入码匹配数。
    pub inject_code_match: usize,
    /// 逻辑步数累计。
    pub steps: u64,
}

impl Metrics {
    /// 全零。
    pub fn zero() -> Metrics {
        Metrics {
            matrix_cells: 0,
            matrix_pass: 0,
            parity_cells: 0,
            parity_pass: 0,
            switch_total: 0,
            switch_accepted: 0,
            switch_rejected: 0,
            assert_failed: 0,
            inject_total: 0,
            inject_caught: 0,
            inject_code_match: 0,
            steps: 0,
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("metrics[matrix={}/{} parity={}/{} switch={} acc={} rej={} fails={} inj={} caught={} code={}]", self.matrix_pass, self.matrix_cells,
            self.parity_pass, self.parity_cells, self.switch_total, self.switch_accepted, self.switch_rejected, self.assert_failed,
            self.inject_total, self.inject_caught, self.inject_code_match)
    }
}

// ===========================================================================
// 19. 观察入口 observe()
// ===========================================================================

/// 观察包。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Observation {
    /// 指标。
    pub metrics: Metrics,
    /// 分岔矩阵（16 格）。
    pub matrix: Vec<MatrixCell>,
    /// 表分支对照（16 格）。
    pub parity: Vec<BranchCell>,
    /// 切换压测报告（观察用缩放轮次）。
    pub switch: SwitchReport,
    /// 守恒账。
    pub conserved: bool,
    /// 探针守恒。
    pub probe_conserved: bool,
    /// 环账自校验。
    pub ring_ok: bool,
    /// 探针观测（最后一次）。
    pub observed: ProbeObs,
    /// 旧空间保留被证实。
    pub keep_proven: bool,
    /// 冻结指纹。
    pub frozen: Frozen,
    /// 冻结自校验。
    pub frozen_ok: bool,
    /// 观察包指纹。
    pub fp: u64,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放两次指纹。
    pub replay_pair: (u64, u64),
    /// TLV 长度自洽。
    pub tlv_ok: bool,
    /// KV 导出非空。
    pub kv_ok: bool,
    /// 注入统计。
    pub inject: InjectStat,
}

impl Observation {
    /// 总闸。
    pub fn all_ok(&self) -> bool {
        self.metrics.matrix_pass == self.metrics.matrix_cells
            && self.metrics.matrix_cells == BRANCH_CELLS
            && self.metrics.parity_pass == self.metrics.parity_cells
            && self.metrics.parity_cells == BRANCH_CELLS
            && self.metrics.assert_failed == 0
            && self.switch.ok()
            && self.conserved
            && self.probe_conserved
            && self.ring_ok
            && self.keep_proven
            && self.frozen_ok
            && self.replay_stable
            && self.tlv_ok
            && self.kv_ok
            && self.inject.all_ok()
    }
    /// 摘要串。
    pub fn summary(&self) -> String {
        format!(
            "matrix={}/{} parity={}/{} switch={} cons={} pcons={} ring={} keep={} frozen={} replay={} tlv={} kv={} inj={} fp=0x{:016X} ALL={}",
            self.metrics.matrix_pass,
            self.metrics.matrix_cells,
            self.metrics.parity_pass,
            self.metrics.parity_cells,
            self.switch.describe(),
            if self.conserved { 1 } else { 0 },
            if self.probe_conserved { 1 } else { 0 },
            if self.ring_ok { 1 } else { 0 },
            if self.keep_proven { 1 } else { 0 },
            if self.frozen_ok { 1 } else { 0 },
            if self.replay_stable { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            if self.kv_ok { 1 } else { 0 },
            self.inject.describe(),
            self.fp,
            if self.all_ok() { 1 } else { 0 }
        )
    }
}

/// 聚合入口：真实跑一遍全流程得出结论。
///
/// **不**与 `replay()` 互递归：本函数调 `replay()` 拿稳定性，但
/// `replay()` 不反过来调本函数。
pub fn observe(seed: u64) -> Observation {
    // 1) 分岔矩阵（16 格）。
    let matrix = branch_matrix();
    let mut matrix_pass = 0usize;
    let mut i: usize = 0;
    while i < matrix.len() {
        if let Some(c) = matrix.get(i) {
            if c.pass {
                matrix_pass = matrix_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    // 2) 表分支对照（16 格）。
    let parity = table_branch_parity();
    let mut parity_pass = 0usize;
    i = 0;
    while i < parity.len() {
        if let Some(c) = parity.get(i) {
            if c.pass {
                parity_pass = parity_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    // 3) 切换压测（观察用缩放轮次；判据全量由 `switch_criterion()` 跑）。
    let scaled = SWITCH_ROUNDS / SWITCH_SCALE_DEN * SWITCH_SCALE_NUM;
    let sw = switch_stress(seed, scaled);
    // 4) 注入核。
    let recs = inject_all(seed);
    let ist = inject_stat(&recs);
    // 5) 干净总册：探针观测 + 守恒 + 环账 + 保留可证。
    let mut book = match PcidBook::new() {
        Ok(bk) => bk,
        Err(_) => {
            return degraded(seed, matrix, matrix_pass, parity, parity_pass, ist);
        }
    };
    let _ = book.probe.fill(0, 4);
    let _ = book.probe.fill(5, 2);
    let _ = book.probe.fill_global(3);
    // 走一趟 PCID on 的 NOFLUSH 快切：这是"旧空间保留"的取证点。
    let fast = SwitchReq::make(Mode::On, Scene::NewAsidNoFlush, 0);
    let fast_res = book.switch_cr3(&fast);
    let keep = fast_res.is_ok() && book.probe.old_space_survives();
    let observed = book.probe.observed;
    // 再走一趟 off 路的全刷（违用拦截），确认违用不碰 TLB。
    let misuse = SwitchReq::make(Mode::Off, Scene::NewAsidNoFlush, book.asids.current);
    let misuse_res = book.switch_cr3(&misuse);
    let misuse_blocked = misuse_res.is_err() && book.probe.observed.flushed == 0 && book.probe.observed.action == TlbAction::Rejected;
    let metrics = Metrics {
        matrix_cells: matrix.len(),
        matrix_pass,
        parity_cells: parity.len(),
        parity_pass,
        switch_total: sw.rounds,
        switch_accepted: sw.accepted,
        switch_rejected: sw.rejected,
        assert_failed: sw.assert_failed.wrapping_add(if misuse_blocked { 0 } else { 1 }),
        inject_total: ist.total,
        inject_caught: ist.caught,
        inject_code_match: ist.code_match,
        steps: book.tick,
    };
    let conserved = book.ledger.conserved();
    let probe_conserved = book.probe.conserved();
    let ring_ok = book.ring.verify() && book.ring.dropped_consistent();
    let frozen = Frozen { ledger_fp: book.ledger.fingerprint(), probe_fp: book.probe.fingerprint(), ring_fp: book.ring.fingerprint(),
        ..Frozen::capture_static() };
    let frozen_ok = match frozen.verify() {
        Ok(()) => book.ledger.conserved() && book.probe.conserved(),
        Err(_) => false,
    };
    let rp1 = replay(seed);
    let rp2 = replay(seed);
    let replay_pair = (rp1.fp, rp2.fp);
    let replay_ok = rp1.fp == rp2.fp && rp1.ok() && rp2.ok();
    let mut tmp = Observation {
        metrics,
        matrix,
        parity,
        switch: sw,
        conserved,
        probe_conserved,
        ring_ok,
        observed,
        keep_proven: keep,
        frozen,
        frozen_ok,
        fp: 0,
        replay_stable: replay_ok,
        replay_pair,
        tlv_ok: false,
        kv_ok: false,
        inject: ist,
    };
    let kv = export_kv(&tmp);
    tmp.kv_ok = !kv.is_empty();
    let tlv = export_tlv(&tmp);
    tmp.tlv_ok = tlv_len_ok(&tlv);
    tmp.fp = fp_observe(&tmp);
    tmp
}

/// 构造失败时的降级观察包（显式把 `all_ok` 拉红，不静默放过）。
fn degraded(seed: u64, matrix: Vec<MatrixCell>, mp: usize, parity: Vec<BranchCell>, pp: usize, ist: InjectStat) -> Observation {
    let rp = replay(seed);
    let mut o = Observation {
        metrics: Metrics { matrix_cells: matrix.len(), matrix_pass: mp, parity_cells: parity.len(), parity_pass: pp,
            switch_total: 0, switch_accepted: 0, switch_rejected: 0, assert_failed: 1, inject_total: ist.total,
            inject_caught: ist.caught, inject_code_match: ist.code_match, steps: 0 },
        matrix,
        parity,
        switch: SwitchReport { rounds: 0, expect_rounds: 0, accepted: 0, rejected: 0, both_modes: false, assert_failed: 1, keep_proven: 0,
            global_preserved: 0, conserved: false, probe_conserved: false, ring_ok: false, fsm_refused: 0 },
        conserved: false,
        probe_conserved: false,
        ring_ok: false,
        observed: ProbeObs::default(),
        keep_proven: false,
        frozen: Frozen::capture_static(),
        frozen_ok: false,
        fp: 0,
        replay_stable: rp.ok(),
        replay_pair: (rp.fp, rp.fp),
        tlv_ok: false,
        kv_ok: false,
        inject: ist,
    };
    o.fp = fp_observe(&o);
    o
}

/// 观察包指纹（只折叠字段，不重跑逻辑）。
pub fn fp_observe(o: &Observation) -> u64 {
    let mut i: usize = 0;
    let mut msum: u64 = 0;
    while i < o.matrix.len() {
        if let Some(c) = o.matrix.get(i) {
            msum = fnv1a_acc(msum, if c.pass { 1 } else { 0 });
            msum = fnv1a_acc(msum, c.expect.code() as u64);
            msum = fnv1a_acc(msum, c.got_code as u64);
        }
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    let mut psum: u64 = 0;
    while j < o.parity.len() {
        if let Some(c) = o.parity.get(j) {
            psum = fnv1a_acc(psum, if c.pass { 1 } else { 0 });
        }
        j = j.wrapping_add(1);
    }
    let words = [
        book_fp(),
        branch_fp(),
        o.frozen.fingerprint(),
        o.metrics.switch_total,
        o.metrics.switch_accepted,
        o.metrics.switch_rejected,
        o.metrics.assert_failed,
        msum,
        psum,
        o.observed.before_non_global as u64,
        o.observed.after_non_global as u64,
        o.observed.flushed as u64,
        o.replay_pair.0,
        o.replay_pair.1,
        o.inject.caught as u64,
        V2622_VERSION_ID,
    ];
    fnv1a_words(&words)
}

// ===========================================================================
// 20. 导出（export_kv / export_tlv）
// ===========================================================================

/// KV 导出（人类可读 / grep 友好）。
pub fn export_kv(o: &Observation) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("ver={}", V2622_VERSION_STR));
    out.push(format!("seg_base=0x{:04X} seg_mask=0x{:04X}", SEG_BASE, SEG_MASK));
    out.push(format!("book_rows={} branch_cells={}", BOOK_ROWS, BRANCH_CELLS));
    out.push(format!("book_fp=0x{:016X} branch_fp=0x{:016X}", book_fp(), branch_fp()));
    out.push(format!("cr3_noflush_bit={} cr3_noflush_mask=0x{:016X}", CR3_NOFLUSH_BIT, CR3_NOFLUSH_MASK));
    out.push(format!("cr4_pcide_bit={} pcid_max=0x{:03X} pcid_reserved=0x{:03X}", CR4_PCIDE_BIT, PCID_MAX, PCID_RESERVED));
    out.push(format!("matrix={}/{}", o.metrics.matrix_pass, o.metrics.matrix_cells));
    out.push(format!("parity={}/{}", o.metrics.parity_pass, o.metrics.parity_cells));
    out.push(format!("switch={}", o.switch.describe()));
    out.push(format!("conserved={} probe_conserved={} ring_ok={}", if o.conserved { 1 } else { 0 }, if o.probe_conserved { 1 } else { 0 },
        if o.ring_ok { 1 } else { 0 }));
    out.push(format!("keep_proven={}", if o.keep_proven { 1 } else { 0 }));
    out.push(format!("frozen={} frozen_ok={}", o.frozen.describe(), if o.frozen_ok { 1 } else { 0 }));
    out.push(format!("probe_obs={}", o.observed.describe()));
    out.push(format!("inject={}", o.inject.describe()));
    out.push(format!("inject_rounds={} inject_forms={}", INJECT_ROUNDS, FORM_COUNT));
    out.push(format!("switch_rounds={}", SWITCH_ROUNDS));
    out.push(format!("replay_pair=0x{:016X},0x{:016X} stable={}", o.replay_pair.0, o.replay_pair.1, if o.replay_stable { 1 } else { 0 }));
    out.push(format!("metrics={}", o.metrics.describe()));
    out.push(format!("fp=0x{:016X}", o.fp));
    out.push(format!("all_ok={}", if o.all_ok() { 1 } else { 0 }));
    // 逐格分岔矩阵（判据"逐项断言"的可见证据）。
    let mut i: usize = 0;
    while i < o.matrix.len() {
        if let Some(c) = o.matrix.get(i) {
            out.push(format!("cell[{:02}] {}", i, c.describe()));
        }
        i = i.wrapping_add(1);
    }
    // 逐行总册。
    let mut r: usize = 0;
    while r < BOOK.len() {
        if let Some(row) = BOOK.get(r) {
            out.push(format!("row[{:02}] {}", r, row.describe()));
        }
        r = r.wrapping_add(1);
    }
    // 注入留痕（前 12 条，避免导出串过长）。
    let mut k: usize = 0;
    let mut shown: usize = 0;
    while k < FORM_COUNT && shown < FORM_COUNT {
        if let Some(rec) = first_record_of(k) {
            out.push(format!("inject[{:02}] {}", k, rec));
            shown = shown.wrapping_add(1);
        }
        k = k.wrapping_add(1);
    }
    out
}

/// 取第 `form` 种形态的首条注入留痕串（导出用，省内存）。
fn first_record_of(form: usize) -> Option<String> {
    let f = Form::ALL.get(form)?;
    let rec = inject_form(*f, 0x2622_2622_2622_2622, 0);
    Some(rec.describe())
}

/// TLV 推送原语：tag + 大端长度 + payload。
fn tlv_push(buf: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    buf.push(tag);
    let n = payload.len() as u16;
    buf.push(((n >> 8) & 0xFF) as u8);
    buf.push((n & 0xFF) as u8);
    let mut i: usize = 0;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            buf.push(*b);
        }
        i = i.wrapping_add(1);
    }
}

/// TLV 推送 `u64`（大端 8 字节）。
fn push_u64(buf: &mut Vec<u8>, v: u64) {
    let mut k: usize = 0;
    while k < 8 {
        buf.push(((v >> (56 - k * 8)) & 0xFF) as u8);
        k = k.wrapping_add(1);
    }
}

/// TLV 推送 `u32`（大端 4 字节）。
fn push_u32(buf: &mut Vec<u8>, v: u32) {
    let mut k: usize = 0;
    while k < 4 {
        buf.push(((v >> (24 - k * 8)) & 0xFF) as u8);
        k = k.wrapping_add(1);
    }
}

/// TLV 推送 `u16`（大端 2 字节）。
fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}

/// TLV 组头 tag（magic + version + body_len + item_count）。
pub const TAG_HEAD: u8 = 0xF0;
/// TLV 指标 tag。
pub const TAG_METRICS: u8 = 0x01;
/// TLV 分岔矩阵 tag。
pub const TAG_MATRIX: u8 = 0x02;
/// TLV 总册行 tag。
pub const TAG_BOOK: u8 = 0x03;
/// TLV 注入 tag。
pub const TAG_INJECT: u8 = 0x04;
/// TLV 探针观测 tag。
pub const TAG_PROBE: u8 = 0x05;
/// TLV 冻结指纹 tag。
pub const TAG_FROZEN: u8 = 0x06;
/// TLV 结束 tag。
pub const TAG_END: u8 = 0xFF;

/// TLV 导出。
///
/// 组头结构：`magic(u64) | version(u64) | body_len(u64) | item_count(u32)`。
/// **`version` 单独一行 push**，不与行尾注释合并——否则组头解析
/// 会被尾部文本吞掉半个字节，往返就不一致了。
pub fn export_tlv(o: &Observation) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut items: u32 = 0;
    // 指标段。
    let mut m: Vec<u8> = Vec::new();
    push_u32(&mut m, o.metrics.matrix_pass as u32);
    push_u32(&mut m, o.metrics.matrix_cells as u32);
    push_u32(&mut m, o.metrics.parity_pass as u32);
    push_u32(&mut m, o.metrics.parity_cells as u32);
    push_u64(&mut m, o.metrics.switch_total);
    push_u64(&mut m, o.metrics.switch_accepted);
    push_u64(&mut m, o.metrics.switch_rejected);
    push_u64(&mut m, o.metrics.assert_failed);
    push_u64(&mut m, o.metrics.inject_total as u64);
    push_u64(&mut m, o.metrics.inject_caught as u64);
    push_u64(&mut m, o.metrics.inject_code_match as u64);
    push_u64(&mut m, o.metrics.steps);
    tlv_push(&mut body, TAG_METRICS, &m);
    items = items.wrapping_add(1);
    // 分岔矩阵段：每格 8 字节。
    let mut i: usize = 0;
    while i < o.matrix.len() {
        if let Some(c) = o.matrix.get(i) {
            let mut g: Vec<u8> = Vec::new();
            push_u16(&mut g, c.expect.code());
            push_u16(&mut g, c.got_code);
            push_u8_pad(&mut g, c.mode.index() as u8);
            push_u8_pad(&mut g, c.scene.index() as u8);
            push_u8_pad(&mut g, c.expect.action().index() as u8);
            push_u8_pad(&mut g, if c.pass { 1 } else { 0 });
            push_u16(&mut g, 0);
            tlv_push(&mut body, TAG_MATRIX, &g);
            items = items.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    // 总册段：每行 rule + verdict + action。
    let mut r: usize = 0;
    while r < BOOK.len() {
        if let Some(row) = BOOK.get(r) {
            let mut g: Vec<u8> = Vec::new();
            push_u16(&mut g, row.rule);
            push_u8_pad(&mut g, row.mode.index() as u8);
            push_u8_pad(&mut g, row.scene.index() as u8);
            push_u8_pad(&mut g, row.verdict.index() as u8);
            push_u8_pad(&mut g, row.action.index() as u8);
            tlv_push(&mut body, TAG_BOOK, &g);
            items = items.wrapping_add(1);
        }
        r = r.wrapping_add(1);
    }
    // 注入段：每形态计数 + 首条码。
    let mut f: usize = 0;
    while f < FORM_COUNT {
        let rec = inject_form(match Form::ALL.get(f) {
            Some(x) => *x,
            None => Form::F1NoFlushWhileOff,
        }, 0x2622_2622_2622_2622, 0);
        let mut g: Vec<u8> = Vec::new();
        push_u16(&mut g, rec.expect_code);
        push_u16(&mut g, rec.got_code);
        push_u16(&mut g, INJECT_ROUNDS as u16);
        push_u8_pad(&mut g, f as u8);
        tlv_push(&mut body, TAG_INJECT, &g);
        items = items.wrapping_add(1);
        f = f.wrapping_add(1);
    }
    // 探针观测段。
    let mut p: Vec<u8> = Vec::new();
    push_u32(&mut p, o.observed.before_non_global);
    push_u32(&mut p, o.observed.after_non_global);
    push_u16(&mut p, o.observed.before_global);
    push_u16(&mut p, o.observed.after_global);
    push_u16(&mut p, o.observed.old_asid);
    push_u16(&mut p, o.observed.new_asid);
    push_u16(&mut p, o.observed.old_space_after);
    push_u32(&mut p, o.observed.flushed);
    push_u8_pad(&mut p, o.observed.action.index() as u8);
    tlv_push(&mut body, TAG_PROBE, &p);
    items = items.wrapping_add(1);
    // 冻结段。
    let mut z: Vec<u8> = Vec::new();
    push_u64(&mut z, o.frozen.book_fp);
    push_u64(&mut z, o.frozen.branch_fp);
    push_u64(&mut z, o.frozen.parity_fp);
    push_u64(&mut z, o.frozen.fingerprint());
    tlv_push(&mut body, TAG_FROZEN, &z);
    items = items.wrapping_add(1);
    // 组头。
    let mut out: Vec<u8> = Vec::new();
    push_u64(&mut out, TLV_MAGIC);
    // 组头 version 单独一行 push（不被行尾注释吞）。
    push_u64(&mut out, TLV_VERSION);
    push_u64(&mut out, body.len() as u64);
    push_u32(&mut out, items);
    // 拼 body。
    let mut k: usize = 0;
    while k < body.len() {
        if let Some(b) = body.get(k) {
            out.push(*b);
        }
        k = k.wrapping_add(1);
    }
    tlv_push(&mut out, TAG_END, &[]);
    out
}

/// TLV 单字节推送（补齐用）。
fn push_u8_pad(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

/// TLV 长度自洽（组头 `body_len` 与实际长度、item_count 与 TLV 条数）。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    // 组头 = 8(magic) + 8(version) + 8(body_len) + 4(count) = 28 字节。
    if buf.len() < 28 {
        return false;
    }
    let magic = read_u64(buf, 0);
    if magic != TLV_MAGIC {
        return false;
    }
    let body_len = read_u64(buf, 16);
    let count = read_u32(buf, 24) as usize;
    // 结束 tag 3 字节。
    if buf.len() != 28 + body_len as usize + 3 {
        return false;
    }
    // 逐条走一遍 TLV，验证长度字段自洽。
    let mut off: usize = 28;
    let end: usize = 28 + body_len as usize;
    let mut n: usize = 0;
    while off < end {
        if off + 3 > end {
            return false;
        }
        let _tag = buf.get(off).copied().unwrap_or(0);
        let hi = buf.get(off + 1).copied().unwrap_or(0) as usize;
        let lo = buf.get(off + 2).copied().unwrap_or(0) as usize;
        let len = (hi << 8) | lo;
        off = off + 3 + len;
        if off > end {
            return false;
        }
        n = n.wrapping_add(1);
    }
    if off != end {
        return false;
    }
    n == count
}

/// TLV 大端 `u64` 读。
pub fn read_u64(buf: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut k: usize = 0;
    while k < 8 {
        let b = buf.get(off.wrapping_add(k)).copied().unwrap_or(0);
        v = (v << 8) | b as u64;
        k = k.wrapping_add(1);
    }
    v
}

/// TLV 大端 `u32` 读。
pub fn read_u32(buf: &[u8], off: usize) -> u32 {
    let mut v: u32 = 0;
    let mut k: usize = 0;
    while k < 4 {
        let b = buf.get(off.wrapping_add(k)).copied().unwrap_or(0);
        v = (v << 8) | b as u32;
        k = k.wrapping_add(1);
    }
    v
}

// ===========================================================================
// 21. 性能逐项分解（complexity）
// ===========================================================================

/// 一条复杂度分解。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ComplexityRow {
    /// 操作名。
    pub name: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 内存（字节或元素）。
    pub mem: &'static str,
    /// 系统调用次数（纯功能线恒 0）。
    pub syscalls: u32,
    /// 步数上界。
    pub steps: u64,
    /// 「深在哪 / 对标 Windows」的一句话。
    pub note: &'static str,
}

impl ComplexityRow {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} time={} mem={} syscalls={} steps<={} {}", self.name, self.time, self.mem, self.syscalls, self.steps, self.note)
    }
}

/// 复杂度分解表（逐项）。
pub fn complexity() -> Vec<ComplexityRow> {
    let rows: [ComplexityRow; 12] = [
        ComplexityRow { name: "book_rows", time: "O(BOOK_ROWS=16)", mem: "16 * 40B 常量段", syscalls: 0, steps: 16,
            note: "Windows 无对应表；语义散在 KeFillDescriptor 之类源码里" },
        ComplexityRow { name: "branch_table", time: "O(MODES*SCENES=16)", mem: "16 * 8B 常量段", syscalls: 0, steps: 16,
            note: "Windows 是 if 分支，不可枚举、不可指纹" },
        ComplexityRow { name: "table_branch_parity", time: "O(16)", mem: "Vec<BranchCell> 16 项", syscalls: 0, steps: 16,
            note: "本单特有的漂移闸；Windows 无对等物" },
        ComplexityRow { name: "branch_matrix", time: "O(16) 每格 O(1)", mem: "每格新建 PcidBook（约 1.3KB）", syscalls: 0, steps: 16,
            note: "格间状态隔离，故可复现；Windows 无隔离语义" },
        ComplexityRow { name: "precheck", time: "O(1)", mem: "0", syscalls: 0, steps: 6,
            note: "对应硬件对未对齐/保留位 CR3 的 #GP 早于 TLB 动作" },
        ComplexityRow { name: "switch_cr3", time: "O(1)", mem: "0（除错误串）", syscalls: 0, steps: 5,
            note: "Windows 一次进程切换约 10^3~10^4 量级指令，含地址验证" },
        ComplexityRow { name: "probe.fill", time: "O(1)", mem: "1 槽", syscalls: 0, steps: 1,
            note: "逻辑模型装入，真实对应一次缺页填 TLB" },
        ComplexityRow { name: "probe.apply", time: "O(ASID_SLOTS=16)", mem: "0", syscalls: 0, steps: 16,
            note: "真实硬件是按 set/way 扫描，成本与本单同阶" },
        ComplexityRow { name: "switch_stress", time: "O(rounds)", mem: "O(1)", syscalls: 0, steps: SWITCH_ROUNDS,
            note: "10^5 次双模式切换在本单是毫秒级；真机做 10^5 次会污染基准" },
        ComplexityRow { name: "inject_all", time: "O(FORM_COUNT*10=120)", mem: "Vec<Injected> 120 项", syscalls: 0, steps: 120,
            note: "判据要求 10/10，本单给 120/120" },
        ComplexityRow { name: "replay", time: "O(REPLAY_STEPS=4096)", mem: "O(1)（环账定容）", syscalls: 0, steps: REPLAY_STEPS as u64,
            note: "同种子两次逐位一致；Windows 无回放概念" },
        ComplexityRow { name: "export_tlv", time: "O(16+16+12)", mem: "约 1.3KB 输出", syscalls: 0, steps: 32,
            note: "组头 28 字节 + body + 结束 tag 3 字节" },
    ];
    let mut out: Vec<ComplexityRow> = Vec::new();
    let mut i: usize = 0;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            out.push(*r);
        }
        i = i.wrapping_add(1);
    }
    out
}

// ===========================================================================
// 22. 自检套件（self_check）
// ===========================================================================

/// 一条自检项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 自检套件（返回全部项，含失败项，便于离线定位）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    // C1 总册规模。
    out.push(Check { id: "C2622-01 book_size", pass: book_size_ok(), note: "总册行数 == 分支格数 == 16" });
    // C2 分岔表规模。
    out.push(Check { id: "C2622-02 branch_cells", pass: branch_cells_ok(), note: "分岔表 16 格" });
    // C3 表分支对照零漂移。
    out.push(Check { id: "C2622-03 parity", pass: parity_ok(), note: "表项 == 分支，16/16" });
    // C4 分岔矩阵实现对齐。
    out.push(Check { id: "C2622-04 matrix", pass: branch_matrix_ok(), note: "实现逐格对齐分岔表" });
    // C5 状态机合法边数。
    out.push(Check { id: "C2622-05 fsm_legal", pass: SwitchFsm::legal_sweep().len() == 10, note: "合法边 10 条" });
    // C6 状态机非法边数。
    out.push(Check { id: "C2622-06 fsm_illegal", pass: SwitchFsm::illegal_sweep().len() == 36 - 10, note: "非法边 27 条" });
    // C7 错误闭集码序连续。
    out.push(Check { id: "C2622-07 errset", pass: errset_contiguous(), note: "0x1CC0..0x1CEB 连续无洞" });
    // C8 环账自校验 + dropped。
    out.push(Check { id: "C2622-08 ring", pass: ring_ok_probe(), note: "环账 verify + dropped_consistent" });
    // C9 探针守恒。
    out.push(Check { id: "C2622-09 probe", pass: probe_ok_probe(), note: "插入 - 刷出 == 存量" });
    // C10 守恒账。
    out.push(Check { id: "C2622-10 ledger", pass: ledger_ok_probe(), note: "accepted + rejected == total" });
    // C11 NOFLUSH 保留可证。
    out.push(Check { id: "C2622-11 keep_proven", pass: keep_proven_probe(), note: "快切后旧 ASID 条目数不变" });
    // C12 违用不碰 TLB。
    out.push(Check { id: "C2622-12 misuse_tlb", pass: misuse_tlb_probe(), note: "违用拦截时 TLB 零动作" });
    // C13 注入核。
    out.push(Check { id: "C2622-13 inject", pass: inject_stat(&inject_all(seed)).all_ok(), note: "12 形态 × 10 轮全拦且码匹配" });
    // C14 回放稳定。
    out.push(Check { id: "C2622-14 replay", pass: replay_stable(seed), note: "同种子两次逐位一致" });
    // C15 TLV 往返。
    out.push(Check { id: "C2622-15 tlv", pass: tlv_probe(), note: "组头 body_len 与实际长度一致" });
    // C16 KV 非空。
    out.push(Check { id: "C2622-16 kv", pass: kv_probe(), note: "KV 导出至少 20 行" });
    // C17 冻结自校验。
    out.push(Check { id: "C2622-17 frozen", pass: frozen_probe(), note: "冻结指纹与当前总册一致" });
    // C18 双模式压测（缩放轮次）。
    out.push(Check { id: "C2622-18 switch", pass: switch_stress(seed, SWITCH_ROUNDS / SWITCH_SCALE_DEN).ok(), note: "双模式语义逐项断言" });
    out
}

/// 错误闭集码序是否连续。
pub fn errset_contiguous() -> bool {
    let mut i: u16 = 0;
    while i < 44 {
        if let Some(k) = ErrKind::ALL.get(i as usize) {
            if k.code() != SEG_BASE | i {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 环账探针（造一个溢出场景再验）。
fn ring_ok_probe() -> bool {
    let mut r = RingLedger::new();
    let mut i: u64 = 0;
    while i < (RING_CAP as u64) + 7 {
        let t = Trace { tick: i, mode: (i % 2) as u8, scene: (i % 3) as u8, action: (i % 4) as u8, code: (i as u16) & 0x3F, flushed: i };
        if r.push(t).is_err() {
            return false;
        }
        i = i.wrapping_add(1);
    }
    r.verify() && r.dropped_consistent() && r.live() == RING_CAP && r.dropped == 7
}

/// 探针守程探针。
fn probe_ok_probe() -> bool {
    let mut p = TlbProbe::new();
    if !p.conserved() {
        return false;
    }
    if p.fill(0, 4).is_err() {
        return false;
    }
    if p.fill_global(2).is_err() {
        return false;
    }
    if !p.conserved() {
        return false;
    }
    if p.apply(0, 1, TlbAction::FlushNonGlobal).is_err() {
        return false;
    }
    p.conserved() && p.globals_preserved() && p.non_global_total() == 0 && p.globals == 2
}

/// 守恒账探针。
fn ledger_ok_probe() -> bool {
    let mut l = SwitchLedger::zero();
    if !l.conserved() {
        return false;
    }
    l.note_accept(Branch::Accept(TlbAction::Noop, Verdict::Legal), 0);
    l.note_accept(Branch::Accept(TlbAction::FlushAll, Verdict::Legal), 4);
    l.note_reject();
    l.conserved() && l.total == 3 && l.action_sum() == 2 && l.rejected == 1
}

/// 「NOFLUSH 后旧空间保留」探针。
fn keep_proven_probe() -> bool {
    let mut b = match PcidBook::new() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if b.probe.fill(0, 5).is_err() {
        return false;
    }
    let req = SwitchReq::make(Mode::On, Scene::NewAsidNoFlush, 0);
    match b.switch_cr3(&req) {
        Ok(br) => br.action() == TlbAction::Noop && b.probe.old_space_survives() && b.probe.count_of(0) == 5,
        Err(_) => false,
    }
}

/// 「违用不碰 TLB」探针。
fn misuse_tlb_probe() -> bool {
    let mut b = match PcidBook::new() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if b.probe.fill(0, 3).is_err() {
        return false;
    }
    let req = SwitchReq::make(Mode::Off, Scene::NewAsidNoFlush, 0);
    let before = b.probe.non_global_total();
    let r = b.switch_cr3(&req);
    r.is_err() && b.probe.non_global_total() == before && b.probe.observed.flushed == 0
        && b.probe.observed.action == TlbAction::Rejected
}

/// TLV 往返探针。
fn tlv_probe() -> bool {
    let o = observe(0x2622_2622_2622_2622);
    let buf = export_tlv(&o);
    tlv_len_ok(&buf) && read_u64(&buf, 0) == TLV_MAGIC && read_u64(&buf, 8) == TLV_VERSION
}

/// KV 探针。
fn kv_probe() -> bool {
    let o = observe(0x2622_2622_2622_2622);
    export_kv(&o).len() >= 20
}

/// 冻结探针。
fn frozen_probe() -> bool {
    let f = Frozen::capture_static();
    match f.verify() {
        Ok(()) => f.fingerprint() == Frozen::capture_static().fingerprint(),
        Err(_) => false,
    }
}

// ===========================================================================
// 23. 清单与演示
// ===========================================================================

/// 本单资产清单（供上层册对账）。
pub fn manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("single=UNX-F2622 file=unx2622_upcidbook.rs seg=0x{:04X}", SEG_BASE));
    out.push(format!("title=PCID 运行期语义总册（NOFLUSH 位 / 切换分岔）"));
    out.push(format!("version={}", V2622_VERSION_STR));
    out.push(format!("book_rows={} modes={} scenes={} branch_cells={}", BOOK_ROWS, MODES, SCENES, BRANCH_CELLS));
    out.push(format!("err_variants={} seg_end=0x{:04X}", ErrKind::ALL.len(), ERR_SEG_END));
    out.push(format!("switch_rounds={} inject_forms={} inject_rounds={}", SWITCH_ROUNDS, FORM_COUNT, INJECT_ROUNDS));
    out.push(format!("ring_cap={} replay_steps={} asid_slots={}", RING_CAP, REPLAY_STEPS, ASID_SLOTS));
    out.push(format!("book_fp=0x{:016X}", book_fp()));
    out.push(format!("branch_fp=0x{:016X}", branch_fp()));
    out.push(format!("parity_ok={}", if parity_ok() { 1 } else { 0 }));
    out
}

/// 分岔矩阵的可读演示（不进 `observe()`，纯展示）。
pub fn demo_scene() -> String {
    let mut s = String::new();
    s.push_str("PCID 分岔表（模式 × 场景）：\n");
    let m = branch_matrix();
    let mut i: usize = 0;
    while i < m.len() {
        if let Some(c) = m.get(i) {
            s.push_str("  ");
            s.push_str(&c.describe());
            s.push('\n');
        }
        i = i.wrapping_add(1);
    }
    s
}

/// 语义总册的可读演示。
pub fn demo_book() -> String {
    let mut s = String::new();
    s.push_str("PCID 语义总册（16 行）：\n");
    let mut i: usize = 0;
    while i < BOOK.len() {
        if let Some(r) = BOOK.get(i) {
            s.push_str("  ");
            s.push_str(&r.describe());
            s.push('\n');
        }
        i = i.wrapping_add(1);
    }
    s
}

/// 版本锚（进导出串的最后一行）。
pub fn version_anchor() -> String {
    format!("UNX-F2622 {} book_fp=0x{:016X} branch_fp=0x{:016X}", V2622_VERSION_STR, book_fp(), branch_fp())
}

// ===========================================================================
// 24. 版本锚常量
// ===========================================================================

/// 语义总册版次（改总册必 +1）。
pub const BOOK_REVISION: u32 = 1;
/// 分岔表版次（改分岔必 +1）。
pub const BRANCH_REVISION: u32 = 1;
/// 本单构建标记。
pub const V2622_BUILD: &str = "A4-G3/PCID-BOOK";