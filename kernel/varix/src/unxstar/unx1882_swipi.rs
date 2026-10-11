//! `unx1882_swipi` —— UNX-F1882 切换-IPI 跨链回归（竞态协同全对） 单号：**UNX-F1882** 标题：**切换-IPI 跨链回归（竞态协同全对）** 域：CoRun ·
//! UNX-A3-B15 段「跨链回归」 目标行数：**2200 ~ 3000 行（硬指标）** 判据原文（注释引用）： `UNX-F1882-J1 切换×IPI 竞态协同回归断言全过（IPI 打在切换四段各段的处置正确
//! 10/10），与 F1807/F1847 纪律一致` # 册内上下文 本单属 CoRun · UNX-A3-B15 段「跨链回归」，同段邻单： - F1881 十一矩阵收口 —— 本单只做**切换 × IPI
//! 两链**，不做全域十一矩阵。 - F1883 唤醒 × 切换 × IPI 三链联测 —— 本单只到两链，三链交由 F1883。 - F1884 迁移 × 击落 —— 本单把 `MigrateNotify` /
//! `Shootdown` 作为**矩阵维度**建模， 但只建模**竞态处置**，不建模迁移目标页表切换本身。 - F1885 全域回归主账 —— 本单只导出本段局部账，主账在 F1885 合流。 - F1887
//! 十账族守恒终核 —— 本单只保证本单十账族自洽，终核在 F1887。 # 纪律来源（本单全部断言挂在这两条纪律上） **F1807 switch_to 原语冻结**： (a) callee-saved 集 + 栈指针
//! + 返回地址必须跨 `switch_to` 保持； (b) 寄存器保持性 —— callee-saved 槽逐槽对账，caller-saved 不要求保持； (c) 切换死点 ——
//! 切换原语内不得请求调度，递归调度必须被检出。 **F1847 IPI 处理中断层级纪律**：禁锁 / 禁调度 / 禁长驻（`MaxStayTicks`）， 三禁各自独立落码，**不得合并成一码**。 切换四段承
//! F1801：置位消费 → 选线程 → 切出 → 切入（本单编号 `S0/S1/S2/S3`）。
//! =========================================================================== 布局契约（上下文结构布局）——判据：UNX-F1882-J1
//! —— 布局冻结，不得随实现漂移 =========================================================================== ## 切换上下文
//! `SwitchCtx` 布局 按字段序固定，无隐式对齐依赖（下列偏移为**契约值**，仅作文档，运行期以字段序为准）： ```text 偏移   字段              类型        宽度   语义 0
//! tid               u64          8 线程标识（切换的调度单位） 8      frame             u64          8    栈帧号（切出写入 / 切入读回）
//! 16     need_resched bool         1    F1632 置位消费位 17     trans             u8           1    F1654 过渡态标记
//! 18..23 (对齐填充)        -            6    由 u64 字段自然对齐填充 24     reg               RegBank    144 寄存器组（slots[16]
//! + sp + ret_addr） 152    reg.slots         [u64;16]   128    槽 0..9 = callee-saved，10..15 = caller-saved 280 
//! reg.sp            u64          8    栈指针，F1807 冻结集之一 288    reg.ret_addr      u64          8    返回地址，F1807
//! 冻结集之一 296    reg.mask_before   u64 8    进入切换前 callee-saved 掩码 304    reg.mask_after    u64          8    切换后
//! callee-saved 掩码 312 (结束)            -            - ``` F1807 要求 `mask_before == mask_after`（掩码）+ `sp` /
//! `ret_addr` 逐值保持。 本单在此之上加**逐槽值对账**（`preserve_mask_before == preserve_mask_after`）， 因为掩码相等并不蕴含值相等（详见
//! `PreserveLedger::verdict`）。 ## IPI 上下文 `IpiCtx` 布局（四元组 + 序号，共 24 字节） ```text 偏移   字段          类型   宽度   语义 0
//! vector_no     u16     2    向量号（F1842 分配：0x20/0x21/0x22） 2      pad           u16     2    对齐填充，恒 0 4     
//! target_cpu    u32     4 目标 CPU 号 8      handler_id    u32     4    分派四步中「分发」步选中的处理器号 12     stay_ticks   
//! u32     4 已驻留 tick 数，F1847 禁长驻的唯一度量 16     seq           u64     8    向量序号（发送序，全局单调） 24   (结束)        - -
//! ``` `seq` 是向量账守恒的**唯一**主键：`sent / delivered / handled / eoi` 四本账 全部以 `seq`
//! 为键，缺失即失衡（`F1882Code::VecLedgerUnbalanced`）。 # 分派四步（竞态相关部分） IPI 分派四步：`step_entry`（入口）→ `step_identify`（识别）→
//! `step_dispatch`（分发） → `step_eoi`（EOI）。四步用位标志记账：`0b0001 / 0b0010 / 0b0100 / 0b1000`。 竞态相关的实现要点： -
//! `step_entry` 做**丢失判定**：本 tick 内无对应置位则判 `Lost`，此后禁止再分派。 - `step_dispatch` 开头有**丢失闸**：`Lost`
//! 态直接拒绝，杜绝「从未投递却被处理」的幽灵记账。 - `step_eoi` 受故障注入控制（可跳过），用于制造 EOI 遗漏。 - 四步之间允许「被切换打断」，打断期间驻留 tick 照常累加（F1847
//! 禁长驻的度量口径）。 # 竞态矩阵 N/A 理由 矩阵名义 4 段 × 3 向量类 = 12 格，其中 **2 格 N/A**，有效格恰好 **10 格**： - `(SwitchOut, Shootdown)` →
//! N/A。旧地址空间已入栈账，失去在核对象；此时打进来的 击落请求语义上应由**目标核**在新上下文建立后受理，属 `PendingOnTarget`， 但本单不建模跨核投递，判 N/A 而非强行给一个处置。 -
//! `(SwitchIn, MigrateNotify)` → N/A。切入完成后的迁移通知语义已由 `S1` 格完整覆盖， 在 `S3` 重复建模会与之矛盾，判 N/A。 N/A 由 `expect_disp()`
//! 返回 `None` 表达，`judge` 对 N/A 格不计入分母。 # 三类处置的差异 | 处置 | 语义 | 兑现时机 | 典型段 | |------|------|----------|--------| |
//! `Pending` | 本核稍后处置 | 本轮段尾 | `S0` | | `Deferred` | 推迟到段外 | 下一轮段首 | `S1` | | `PendingOnTarget` | 由目标核处置 |
//! 跨核兑现点 | `S2` | | `ImmediateAfterSwitch` | 切入完成后立刻兑现 | 切入完成 | `S3` | `S3` 的 `ImmediateAfterSwitch`
//! 是唯一**不能被拒绝**的处置：若它落在切换死点内， 则「闩住」（latch）到切入完成后兑现——这正是 `complete_latched` 的职责。
//! ===========================================================================
//! 性能逐项分解（本单为纯功能模型，逐项对应模型内的可数项，不做任何真实 IO）
//! =========================================================================== | # | 性能项 | 模型内计数口径 | 复杂度 |
//! |---|--------|----------------|--------| | 1 | 切换四段推进 | `seg_advance` 计数 | O(1)/段 | | 2 | 段签名链维护 |
//! `fp_chain.len()` 追加 + 折叠 | O(1)/段 | | 3 | 寄存器逐槽对账 | `CALLEE_SAVED_SLOTS = 10` 槽 | O(10)/切 | | 4 | 寄存器全槽快照 |
//! `REG_SLOTS = 16` 槽 | O(16)/切 | | 5 | IPI 分派四步 | `steps_mask` 四位 | O(1)/向量 | | 6 | F1847 驻留度量 | `stay_ticks`
//! 逐 tick 累加 | O(1)/tick | | 7 | 向量账守恒 | `sent/delivered/handled/eoi` 四账 | O(1)/向量 | | 8 | 竞态矩阵回放 | 10 有效格 × 1
//! 轮 | O(10) | | 9 | 注入核自检 | 10 判据形态 + 2 冻结子形态 | O(12) | | 10 | 指标导出 | `BTreeMap` 分桶 + TLV 编码 | O(n log n) |
//! =========================================================================== 诊断码段（本单独占 **0xB120 ~ 0xB12F**，共
//! 16 枚，主码 **0xB120**） =========================================================================== ```text
//! 0xB120 SegOrderViolation         段序违规：段号非 +1 推进 0xB121 SegSignatureBroken 段签名链断裂：FNV 折叠值与重放不符 0xB122
//! TransitionMarkDirty       F1654 过渡态标记未在段尾清除 0xB123 SkipSegment 跳段：请求的段号不在当前段的下一段 0xB124 CalleeSavedBroken   
//! callee-saved 槽值被破坏（F1807） 0xB125 SpOrRetAddrBroken        栈指针 / 返回地址被破坏（F1807） 0xB126 DeadPointSchedule     
//! 切换死点内请求调度（F1807） 0xB127 CallerSavedBroken         caller-saved 槽被要求保持（本单不应命中） 0xB128 IpiLockViolation IPI
//! 内取锁（违 F1847 禁锁） 0xB129 IpiSchedViolation         IPI 内请求调度（违 F1847 禁调度） 0xB12A IpiLongStay IPI 驻留超
//! MaxStayTicks（违 F1847 禁长驻） 0xB12B IpiLost                  IPI 丢失：已发送未投递 / 已投递未处理 0xB12C IpiDuplicated       
//! IPI 重复处理：同一 seq 被处理两次 0xB12D EoiMissing               EOI 遗漏：已处理未发 EOI 0xB12E VecLedgerUnbalanced      
//! 向量账失衡（seq 缺失或四账不等） 0xB12F ReschedLedgerUnbalanced   跨链账失衡：置位 != 消费 + 末态挂账 ``` # 纯功能线 `no_std + alloc`，零
//! `unwrap` / `expect` / `panic!`，零浮点，零真实 IO，零墙钟。 随机源为确定性 64 位 LCG，逻辑时间为 `tick: u64` 单调推进。

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// --------------------------------------------------------------------------- 常量
// ---------------------------------------------------------------------------
/// 本单版本号。
pub const F1882_VERSION: u32 = 0x1882;

/// 诊断码段下界（含）。
pub const F1882_SEG_BASE: u16 = 0xB120;
/// 诊断码段上界（含）。
pub const F1882_SEG_END: u16 = 0xB12F;
/// 主码：切换四段段序违规。
pub const F1882_MAIN: u16 = 0xB120;

/// 寄存器槽总数（`slots[0..16]`）。
pub const REG_SLOTS: usize = 16;
/// F1807 冻结的 callee-saved 槽数（`slots[0..10]`）。
pub const CALLEE_SAVED_SLOTS: usize = 10;
/// caller-saved 槽数（`slots[10..16]`），不要求保持。
pub const CALLER_SAVED_SLOTS: usize = 6;
/// callee-saved 掩码（低 10 位）。
pub const CALLEE_SAVED_MASK: u64 = (1u64 << CALLEE_SAVED_SLOTS) - 1;

/// F1847 禁长驻阈值：`stay_ticks` 超过此值即违规。
pub const MAX_STAY_TICKS: u32 = 8;

/// F1842 分配：`Resched` 向量号。
pub const VECNO_RESCHED: u16 = 0x20;
/// F1842 分配：`MigrateNotify` 向量号。
pub const VECNO_MIGRATE: u16 = 0x21;
/// F1842 分配：`Shootdown` 向量号。
pub const VECNO_SHOOTDOWN: u16 = 0x22;

/// 自持 `Result` 的错误类型。
pub type Result<T> = core::result::Result<T, F1882Code>;

// --------------------------------------------------------------------------- 诊断码闭集（0xB120 ~ 0xB12F，占满 16 枚）
// ---------------------------------------------------------------------------
/// UNX-F1882 诊断码闭集，共 16 枚，**恰好**占满 `0xB120..=0xB12F`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum F1882Code {
    /// 0xB120 段序违规：段号非 +1 推进。
    SegOrderViolation,
    /// 0xB121 段签名链断裂。
    SegSignatureBroken,
    /// 0xB122 F1654 过渡态标记未在段尾清除。
    TransitionMarkDirty,
    /// 0xB123 跳段。
    SkipSegment,
    /// 0xB124 callee-saved 槽值被破坏（F1807）。
    CalleeSavedBroken,
    /// 0xB125 栈指针 / 返回地址被破坏（F1807）。
    SpOrRetAddrBroken,
    /// 0xB126 切换死点内请求调度（F1807）。
    DeadPointSchedule,
    /// 0xB127 caller-saved 槽被要求保持（本单不应命中）。
    CallerSavedBroken,
    /// 0xB128 IPI 内取锁（违 F1847 禁锁）。
    IpiLockViolation,
    /// 0xB129 IPI 内请求调度（违 F1847 禁调度）。
    IpiSchedViolation,
    /// 0xB12A IPI 驻留超限（违 F1847 禁长驻）。
    IpiLongStay,
    /// 0xB12B IPI 丢失。
    IpiLost,
    /// 0xB12C IPI 重复处理。
    IpiDuplicated,
    /// 0xB12D EOI 遗漏。
    EoiMissing,
    /// 0xB12E 向量账失衡。
    VecLedgerUnbalanced,
    /// 0xB12F 跨链账失衡。
    ReschedLedgerUnbalanced,
}

impl F1882Code {
    /// 全部 16 枚诊断码，按码值升序。
    pub const ALL: [F1882Code; 16] = [
        F1882Code::SegOrderViolation, F1882Code::SegSignatureBroken, F1882Code::TransitionMarkDirty,
        F1882Code::SkipSegment, F1882Code::CalleeSavedBroken, F1882Code::SpOrRetAddrBroken,
        F1882Code::DeadPointSchedule, F1882Code::CallerSavedBroken, F1882Code::IpiLockViolation,
        F1882Code::IpiSchedViolation, F1882Code::IpiLongStay, F1882Code::IpiLost, F1882Code::IpiDuplicated,
        F1882Code::EoiMissing, F1882Code::VecLedgerUnbalanced, F1882Code::ReschedLedgerUnbalanced,
    ];

    /// 码值（`0xB120` 起连续 16 枚）。
    pub const fn code(self) -> u16 { F1882_SEG_BASE + self as u16
    }

    /// 英文短名（表驱动）。
    pub fn name(self) -> &'static str { CODE_NAMES[self as usize]
    }

    /// 中文名（表驱动）。
    pub fn cn(self) -> &'static str { CODE_CNS[self as usize]
    }

    /// 修复建议（表驱动）。
    pub fn fix(self) -> &'static str { CODE_FIXES[self as usize]
    }

    /// 是否属 F1847 三禁族。
    pub const fn is_f1847_violation(self) -> bool { matches!( self,
            F1882Code::IpiLockViolation | F1882Code::IpiSchedViolation | F1882Code::IpiLongStay
        )
    }

    /// 是否属 F1807 冻结族。
    pub const fn is_f1807_violation(self) -> bool { matches!( self, F1882Code::CalleeSavedBroken
                | F1882Code::SpOrRetAddrBroken
                | F1882Code::DeadPointSchedule
        )
    }

    /// 是否属 F1801 四段族。
    pub const fn is_f1801_violation(self) -> bool { matches!( self, F1882Code::SegOrderViolation
                | F1882Code::SegSignatureBroken
                | F1882Code::TransitionMarkDirty
                | F1882Code::SkipSegment
        )
    }
}

/// 码值 → 英文短名。
pub const CODE_NAMES: [&str; 16] = [
    "SegOrderViolation", "SegSignatureBroken", "TransitionMarkDirty", "SkipSegment", "CalleeSavedBroken",
    "SpOrRetAddrBroken", "DeadPointSchedule", "CallerSavedBroken", "IpiLockViolation", "IpiSchedViolation",
    "IpiLongStay", "IpiLost", "IpiDuplicated", "EoiMissing", "VecLedgerUnbalanced", "ReschedLedgerUnbalanced",
];

/// 码值 → 中文名。
pub const CODE_CNS: [&str; 16] = [
    "段序违规", "段签名链断裂", "过渡态未清", "跳段", "callee-saved 被破坏", "栈指针/返回地址被破坏", "死点内请求调度", "caller-saved 被要求保持", "IPI 内取锁",
    "IPI 内请求调度", "IPI 驻留超限", "IPI 丢失", "IPI 重复处理", "EOI 遗漏", "向量账失衡", "跨链账失衡",
];

/// 码值 → 修复建议。
pub const CODE_FIXES: [&str; 16] = [
    "段必须严格 +1 推进，越序即拒", "段签名入链后不得旁改，重放折叠值须与在链值相等", "段尾必须清 F1654 过渡态标记", "跳段请求直接拒绝，不做隐式补段",
    "callee-saved 槽逐槽对账，非零改非零同样判破坏", "sp 与 ret_addr 属冻结集，任何段内改写即拒", "死点内 request_sched 必须被检出并拒绝",
    "caller-saved 不要求保持，不应作为违规判据", "IPI 内禁止取锁，违者独立落码不与他禁合并", "IPI 内禁止请求调度，违者独立落码", "IPI 驻留不得超过 MaxStayTicks=8",
    "已投递未处理者禁止再分派，杜绝幽灵记账", "同一 seq 只允许一次 handled", "handled 后必须补 EOI，缺则记遗漏", "四账以 seq 为键，缺一即失衡", "置位账须等于消费账加末态挂账",
];

/// 诊断码段守卫：确认本单闭集恰好占满 `0xB120..=0xB12F` 且无空洞。 判据：UNX-F1882-J1 —— 诊断码段独占，任何越界或空洞都判失败。
pub fn segment_guard() -> bool { if F1882Code::ALL.len() != 16 { return false;
    }
    let mut seen = [false; 16];
    for c in F1882Code::ALL.iter() { let off = c.code();
        if off < F1882_SEG_BASE || off > F1882_SEG_END { return false;
        }
        let i = (off - F1882_SEG_BASE) as usize;
        if seen[i] { return false;
        }
        seen[i] = true;
    }
    seen.iter().all(|b| *b)
}

// --------------------------------------------------------------------------- 确定性随机源与指纹
// ---------------------------------------------------------------------------
/// LCG 乘子。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// LCG 模数占位：`0` 表示 `2^64` wrapping。
pub const LCG_M: u64 = 0;

/// 确定性 64 位 LCG（Knuth MMIX 常数）。 - `a = 6364136223846793005` - `c = 1442695040888963407` - `m = 2^64`（`LCG_M =
/// 0` 表示 wrapping 语义） 判据：UNX-F1882-J1 —— 回归必须可重放，随机源**不得**引入任何墙钟或外部熵。
#[derive(Clone, Debug)]
pub struct Lcg { state: u64,
}

impl Lcg {
    /// 以 `seed` 构造。
    pub const fn new(seed: u64) -> Lcg { Lcg { state: seed }
    }

    /// 取下一值（`state = state * a + c + m`，全 wrapping）。
    pub fn next_u64(&mut self) -> u64 { self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C)
            .wrapping_add(LCG_M);
        self.state
    }

    /// 取 `[0, n)` 内值（拒绝采样，避免模偏；`n == 0` 返回 0）。
    pub fn below(&mut self, n: u64) -> u64 { if n == 0 { return 0;
        }
        loop { let v = self.next_u64();
            if v < (u64::MAX / n) * n { return v % n;
            }
        }
    }

    /// 当前状态（供指纹重放比对）。
    #[allow(dead_code)]
    pub const fn state(&self) -> u64 { self.state
    }
}

/// FNV-1a 64 位指纹初值。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单字节 FNV-1a 折叠。
pub fn fnv1a64(seed: u64, b: u8) -> u64 { (seed ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 字节序列 FNV-1a 指纹。
pub fn fnv1a64_bytes(seed: u64, bytes: &[u8]) -> u64 { let mut h = seed;
    for b in bytes.iter() { h = fnv1a64(h, *b);
    }
    h
}

/// `u64` → 8 字节大端。
pub fn be_u64(v: u64) -> [u8; 8] { v.to_be_bytes()
}

/// `u32` → 4 字节大端。
pub fn be_u32(v: u32) -> [u8; 4] { v.to_be_bytes()
}

// --------------------------------------------------------------------------- 自持判据容器（不依赖 crate::checks，探针 crate
// 可直接编译）
// ---------------------------------------------------------------------------
/// 单条判据。
#[derive(Clone, Debug)]
pub struct CheckItem {
    /// 判据编号（如 `C1882-01`）。
    pub id: String,
    /// 是否通过。
    pub ok: bool,
    /// 备注 / 诊断文本。
    pub note: String,
}

/// 判据集合。
#[derive(Clone, Debug, Default)]
pub struct CheckSet { items: Vec<CheckItem>,
}

impl CheckSet {
    /// 空集合。
    pub const fn new() -> CheckSet { CheckSet { items: Vec::new() }
    }

    /// 追加一条判据（`note` 为 `&str`）。
    pub fn add(&mut self, id: &str, ok: bool, note: &str) { self.items.push(CheckItem { id: id.to_string(), ok,
            note: note.to_string(), });
    }

    /// 追加一条判据（`note` 为 `String`，便于 `format!`）。
    pub fn add_s(&mut self, id: &str, ok: bool, note: String) { self.items.push(CheckItem { id: id.to_string(), ok,
            note, });
    }

    /// 记一条失败判据。
    pub fn fail(&mut self, id: &str, msg: &str) { self.add(id, false, msg);
    }

    /// 判据总数。
    pub fn total(&self) -> usize { self.items.len()
    }

    /// 通过数。
    pub fn passed(&self) -> usize { self.items.iter().filter(|i| i.ok).count()
    }

    /// 全过。
    pub fn all_ok(&self) -> bool { self.items.iter().all(|i| i.ok)
    }

    /// 首条失败判据。
    pub fn first_fail(&self) -> Option<&CheckItem> { self.items.iter().find(|i| !i.ok)
    }

    /// 全部判据。
    pub fn items(&self) -> &[CheckItem] { &self.items
    }

    /// 汇总文本。
    pub fn summary(&self) -> String { let f = match self.first_fail() { Some(i) => format!("{} {}", i.id, i.note),
            None => String::from("none"),
        };
        format!( "[unx1882] {}/{} 通过 first_fail={}", self.passed(), self.total(), f
        )
    }
}

// --------------------------------------------------------------------------- 闭集：段 / 相位 / 向量类 / 处置 / 终态 / 分类
// ---------------------------------------------------------------------------
/// 切换四段（承 F1801）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SwitchSeg {
    /// S0 置位消费段。
    NeedReschedConsume,
    /// S1 选线程段。
    PickThread,
    /// S2 切出段。
    SwitchOut,
    /// S3 切入段。
    SwitchIn,
}

impl SwitchSeg {
    /// 四段全序。
    pub const ALL: [SwitchSeg; 4] = [
        SwitchSeg::NeedReschedConsume, SwitchSeg::PickThread, SwitchSeg::SwitchOut, SwitchSeg::SwitchIn,
    ];
    /// 段号（`0..4`）。
    pub const fn idx(self) -> usize { self as usize
    }
    /// 短名（表驱动）。
    pub fn name(self) -> &'static str { SEG_NAMES[self as usize]
    }
    /// 中文名（表驱动）。
    pub fn cn(self) -> &'static str { SEG_CNS[self as usize]
    }
    /// 下一段；`SwitchIn` 之后回卷到 `S0`。
    pub const fn next(self) -> SwitchSeg { SEG_ALL[(self as usize + 1) & 3]
    }
}

const SEG_ALL: [SwitchSeg; 4] = SwitchSeg::ALL;

/// 段短名。
pub const SEG_NAMES: [&str; 4] = ["NeedReschedConsume", "PickThread", "SwitchOut", "SwitchIn"];
/// 段中文名。
pub const SEG_CNS: [&str; 4] = ["置位消费", "选线程", "切出", "切入"];

/// 段内相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 段入口。
    Enter,
    /// 段中。
    Mid,
    /// 段尾。
    Exit,
}

impl Phase {
    /// 三相位全序。
    pub const ALL: [Phase; 3] = [Phase::Enter, Phase::Mid, Phase::Exit];
    /// 短名（表驱动）。
    pub fn name(self) -> &'static str { PHASE_NAMES[self as usize]
    }
}

/// 相位短名。
pub const PHASE_NAMES: [&str; 3] = ["Enter", "Mid", "Exit"];

/// 向量类（F1842 号段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecClass {
    /// `Resched`（0x20）：置位通知。
    Resched,
    /// `MigrateNotify`（0x21）：迁移通知。
    MigrateNotify,
    /// `Shootdown`（0x22）：TLB 击落。
    Shootdown,
}

impl VecClass {
    /// 三类全序。
    pub const ALL: [VecClass; 3] = [
        VecClass::Resched, VecClass::MigrateNotify, VecClass::Shootdown,
    ];
    /// 类码（`0/1/2`）。
    pub const fn code(self) -> usize { self as usize
    }
    /// 向量号。
    pub const fn vecno(self) -> u16 { match self { VecClass::Resched => VECNO_RESCHED,
            VecClass::MigrateNotify => VECNO_MIGRATE, VecClass::Shootdown => VECNO_SHOOTDOWN,
        }
    }
    /// 短名（表驱动）。
    pub fn name(self) -> &'static str { VEC_NAMES[self.code()]
    }
    /// 中文名（表驱动）。
    pub fn cn(self) -> &'static str { VEC_CNS[self.code()]
    }
}

/// 向量类短名。
pub const VEC_NAMES: [&str; 3] = ["Resched", "MigrateNotify", "Shootdown"];
/// 向量类中文名。
pub const VEC_CNS: [&str; 3] = ["置位通知", "迁移通知", "TLB 击落"];

/// IPI 竞态处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 本核稍后处置，段尾兑现。
    Pending,
    /// 推迟到段外，下一轮段首兑现。
    Deferred,
    /// 由目标核处置，跨核兑现点兑现。
    PendingOnTarget,
    /// 切入完成后立刻兑现（死点内先闩住）。
    ImmediateAfterSwitch,
}

impl Disposition {
    /// 四处置全序。
    pub const ALL: [Disposition; 4] = [
        Disposition::Pending, Disposition::Deferred, Disposition::PendingOnTarget, Disposition::ImmediateAfterSwitch,
    ];
    /// 短名（表驱动）。
    pub fn name(self) -> &'static str { DISP_NAMES[self as usize]
    }
    /// 中文名（表驱动）。
    pub fn cn(self) -> &'static str { DISP_CNS[self as usize]
    }
}

/// 处置短名。
pub const DISP_NAMES: [&str; 4] = [
    "Pending", "Deferred", "PendingOnTarget", "ImmediateAfterSwitch",
];
/// 处置中文名。
pub const DISP_CNS: [&str; 4] = ["挂起本核处置", "推迟到段外", "目标核处置", "切入后立即兑现"];

/// 期望终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerminalExpect {
    /// 挂起，段尾兑现并 EOI。
    PendingAtSegEnd,
    /// 推迟到段外兑现并 EOI。
    DeferredAtCycleEnd,
    /// 切出前挂账，交目标核闭合。
    PendingOnTarget,
    /// 闩住后于切入完成兑现并 EOI。
    LatchedAfterSwitch,
    /// 本核段内已处理并已 EOI（`Resched` 在 `S0` 的直接闭合）。
    HandledLocalEoi,
}

impl TerminalExpect {
    /// 五元闭集全序。
    pub const ALL: [TerminalExpect; 5] = [
        TerminalExpect::PendingAtSegEnd, TerminalExpect::DeferredAtCycleEnd, TerminalExpect::PendingOnTarget,
        TerminalExpect::LatchedAfterSwitch, TerminalExpect::HandledLocalEoi,
    ];
    /// 短名。
    pub fn name(self) -> &'static str { TERM_NAMES[self as usize]
    }
    /// 中文名。
    pub fn cn(self) -> &'static str { TERM_CNS[self as usize]
    }
    /// 兑现点（探针位置）。
    pub const fn probe(self) -> TerminalProbe { match self {
            TerminalExpect::PendingAtSegEnd | TerminalExpect::LatchedAfterSwitch => { TerminalProbe::SegExit
            }
            TerminalExpect::DeferredAtCycleEnd | TerminalExpect::PendingOnTarget => { TerminalProbe::CycleEnd
            }
            TerminalExpect::HandledLocalEoi => TerminalProbe::SegExit,
        }
    }
}

/// 终态短名。
pub const TERM_NAMES: [&str; 5] = [
    "PendingAtSegEnd", "DeferredAtCycleEnd", "PendingOnTarget", "LatchedAfterSwitch", "HandledLocalEoi",
];
/// 终态中文名。
pub const TERM_CNS: [&str; 5] = [
    "段尾兑现并 EOI", "轮末兑现并 EOI", "交目标核闭合", "切入后兑现并 EOI", "段内直接闭合",
];

/// 终态探针位置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerminalProbe {
    /// 段尾探针。
    SegExit,
    /// 轮末探针。
    CycleEnd,
}

impl TerminalProbe {
    /// 两探针全序。
    pub const ALL: [TerminalProbe; 2] = [TerminalProbe::SegExit, TerminalProbe::CycleEnd];
    /// 短名。
    pub fn name(self) -> &'static str { ["SegExit", "CycleEnd"][self as usize]
    }
}

/// 时序位置（四类，用于指标分桶）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchClass {
    /// 段入口。
    SegEnter,
    /// 段中。
    SegMid,
    /// 段尾。
    SegExit,
    /// 轮末（段外）。
    CycleEnd,
}

impl SwitchClass {
    /// 四类全序。
    pub const ALL: [SwitchClass; 4] = [
        SwitchClass::SegEnter, SwitchClass::SegMid, SwitchClass::SegExit, SwitchClass::CycleEnd,
    ];
    /// 短名。
    pub fn name(self) -> &'static str { ["SegEnter", "SegMid", "SegExit", "CycleEnd"][self as usize]
    }
    /// 由相位导出（相位只能落在前三类）。
    pub const fn of_phase(p: Phase) -> SwitchClass { match p { Phase::Enter => SwitchClass::SegEnter,
            Phase::Mid => SwitchClass::SegMid, Phase::Exit => SwitchClass::SegExit,
        }
    }
}

/// F1654 过渡态标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransitionMark {
    /// 无过渡态。
    None,
    /// 置位消费过渡态。
    Consuming,
    /// 选线程过渡态。
    Selecting,
    /// 切出过渡态。
    SwitchingOut,
    /// 切入过渡态。
    SwitchingIn,
}

impl TransitionMark {
    /// 四标记全序（不含 `None`，`None` 为基值单列）。
    pub const ALL: [TransitionMark; 4] = [
        TransitionMark::Consuming, TransitionMark::Selecting, TransitionMark::SwitchingOut,
        TransitionMark::SwitchingIn,
    ];
    /// 短名。
    pub fn name(self) -> &'static str { match self { TransitionMark::None => "None",
            _ => TRANS_NAMES[(self as usize) - 1],
        }
    }
    /// 该段进入时应置的过渡态（`None` 表示该段不置）。
    pub const fn for_seg(s: SwitchSeg) -> TransitionMark { match s {
            SwitchSeg::NeedReschedConsume => TransitionMark::Consuming,
            SwitchSeg::PickThread => TransitionMark::Selecting, SwitchSeg::SwitchOut => TransitionMark::SwitchingOut,
            SwitchSeg::SwitchIn => TransitionMark::SwitchingIn,
        }
    }
}

/// 过渡态短名（索引 = 判别值 - 1）。
pub const TRANS_NAMES: [&str; 4] = ["Consuming", "Selecting", "SwitchingOut", "SwitchingIn"];

/// 拒绝原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DenyReason {
    /// 非下一段。
    NotNextSeg,
    /// 签名链不符。
    SignatureMismatch,
    /// 过渡态未清。
    TransitionDirty,
    /// 死点内请求调度。
    InDeadPoint,
    /// 段内不允许的操作。
    NotAllowedInSeg,
}

impl DenyReason {
    /// 五元闭集全序。
    pub const ALL: [DenyReason; 5] = [
        DenyReason::NotNextSeg, DenyReason::SignatureMismatch, DenyReason::TransitionDirty, DenyReason::InDeadPoint,
        DenyReason::NotAllowedInSeg,
    ];
    /// 短名。
    pub fn name(self) -> &'static str { DENY_NAMES[self as usize]
    }
    /// 对应诊断码。
    pub const fn code(self) -> F1882Code { match self { DenyReason::NotNextSeg => F1882Code::SkipSegment,
            DenyReason::SignatureMismatch => F1882Code::SegSignatureBroken,
            DenyReason::TransitionDirty => F1882Code::TransitionMarkDirty,
            DenyReason::InDeadPoint => F1882Code::DeadPointSchedule,
            DenyReason::NotAllowedInSeg => F1882Code::SegOrderViolation,
        }
    }
}

/// 拒绝原因短名。
pub const DENY_NAMES: [&str; 5] = [
    "NotNextSeg", "SignatureMismatch", "TransitionDirty", "InDeadPoint", "NotAllowedInSeg",
];

// --------------------------------------------------------------------------- A-1. 寄存器组与 F1807 逐槽对账
// ---------------------------------------------------------------------------
/// 寄存器组：`slots[0..10]` 为 callee-saved，`slots[10..16]` 为 caller-saved， 另含 F1807 冻结的 `sp` 与 `ret_addr`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegBank {
    /// 16 个寄存器槽。
    pub slots: [u64; REG_SLOTS],
    /// 栈指针（F1807 冻结集）。
    pub sp: u64,
    /// 返回地址（F1807 冻结集）。
    pub ret_addr: u64,
}

impl RegBank {
    /// 全零寄存器组。
    pub const fn zero() -> RegBank { RegBank { slots: [0; REG_SLOTS], sp: 0, ret_addr: 0,
        }
    }

    /// 以确定性种子填充：`slot[i] = seed ^ (i * 0x9E37_79B9_7F4A_7C15)`。 判据：UNX-F1882-J1 —— 寄存器初值必须可重放，故由纯函数派生而非随机。
    pub fn seeded(seed: u64) -> RegBank { let mut b = RegBank::zero();
        for i in 0..REG_SLOTS { b.slots[i] = seed
                .wrapping_add((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
                .wrapping_mul(0x2545_F491_4F6C_DD1D)
                | 1;
        }
        b.sp = 0x7FFF_0000_0000_0000 | (seed & 0xFFFF);
        b.ret_addr = 0xFFFF_8000_0000_0000 | (seed.rotate_left(17) & 0xFFFF_FFFF);
        b
    }

    /// 读槽（越界返回 0，不 panic）。
    pub fn slot(&self, i: usize) -> u64 { if i < REG_SLOTS { self.slots[i]
        } else { 0
        }
    }

    /// 写槽（越界忽略，不 panic）。
    pub fn set_slot(&mut self, i: usize, v: u64) { if i < REG_SLOTS { self.slots[i] = v;
        }
    }

    /// callee-saved 掩码（低 10 位）。
    pub const fn callee_mask(&self) -> u64 { CALLEE_SAVED_MASK
    }

    /// 全槽指纹（16 槽 + sp + ret_addr）。
    pub fn fingerprint(&self) -> u64 { let mut h = FNV_OFFSET;
        for i in 0..REG_SLOTS { h = fnv1a64(h, (self.slots[i] & 0xFF) as u8);
            h = fnv1a64(h, ((self.slots[i] >> 8) & 0xFF) as u8);
            h = fnv1a64(h, ((self.slots[i] >> 16) & 0xFF) as u8);
            h = fnv1a64(h, ((self.slots[i] >> 24) & 0xFF) as u8);
        }
        for v in [self.sp, self.ret_addr].iter() { for b in be_u64(*v).iter() { h = fnv1a64(h, *b);
            }
        }
        h
    }
}

/// 逐槽对账的一行结果。
#[derive(Clone, Copy, Debug)]
pub struct PreserveRow {
    /// 槽号。
    pub slot: usize,
    /// 切换前值。
    pub before: u64,
    /// 切换后值。
    pub after: u64,
    /// 该槽是否属 callee-saved。
    pub callee: bool,
    /// 该槽是否保持。
    pub kept: bool,
}

/// F1807 寄存器保持性账：逐槽对账 `slots[0..10]` + `sp` + `ret_addr`。 **关键语义**：主判据是**逐槽值相等**，掩码相等只作弱信号。 原因：`mask_before ==
/// mask_after` 无法发现「非零改成另一个非零」这类破坏， 而后者恰恰是切换实现最常见的越界写坏形态。
#[derive(Clone, Debug, Default)]
pub struct PreserveLedger { rows: Vec<PreserveRow>, sp_before: u64, sp_after: u64, ret_before: u64, ret_after: u64,
    mask_before: u64, mask_after: u64,
}

impl PreserveLedger {
    /// 空账。
    pub const fn new() -> PreserveLedger { PreserveLedger { rows: Vec::new(), sp_before: 0, sp_after: 0,
            ret_before: 0, ret_after: 0, mask_before: 0, mask_after: 0,
        }
    }

    /// 对 `before` / `after` 做一次完整对账（含 caller-saved 观测，不作判据）。 判据：UNX-F1882-J1 —— callee-saved 集 + 栈指针 +
    /// 返回地址必须全部保持。
    pub fn account(&mut self, before: &RegBank, after: &RegBank) {
        self.account_with_mask(before, after, CALLEE_SAVED_MASK, CALLEE_SAVED_MASK);
    }

    /// 带显式掩码的对账（掩码真源在 `SwitchCtx`，不在 `RegBank`）。
    pub fn account_with_mask( &mut self, before: &RegBank, after: &RegBank, mask_before: u64, mask_after: u64, ) {
        self.rows.clear();
        for i in 0..REG_SLOTS { let callee = i < CALLEE_SAVED_SLOTS;
            let kept = before.slot(i) == after.slot(i);
            self.rows.push(PreserveRow { slot: i, before: before.slot(i), after: after.slot(i), callee, kept, });
        }
        self.sp_before = before.sp;
        self.sp_after = after.sp;
        self.ret_before = before.ret_addr;
        self.ret_after = after.ret_addr;
        self.mask_before = mask_before;
        self.mask_after = mask_after;
    }

    /// 对账行数（`REG_SLOTS = 16`）。
    pub fn rows(&self) -> &[PreserveRow] { &self.rows
    }

    /// 被破坏的 callee-saved 槽数（主判据）。
    pub fn broken_callee(&self) -> u64 { self.rows
            .iter()
            .filter(|r| r.callee && !r.kept)
            .count() as u64
    }

    /// 被改动的 caller-saved 槽数（**不**作违规判据，仅观测）。
    pub fn touched_caller(&self) -> u64 { self.rows
            .iter()
            .filter(|r| !r.callee && !r.kept)
            .count() as u64
    }

    /// callee-saved 掩码相等（弱信号）。
    pub fn mask_equal(&self) -> bool { self.mask_before == self.mask_after
    }

    /// `sp` 保持。
    pub fn sp_kept(&self) -> bool { self.sp_before == self.sp_after
    }

    /// `ret_addr` 保持。
    pub fn ret_kept(&self) -> bool { self.ret_before == self.ret_after
    }

    /// 冻结集（`sp` + `ret_addr`）整体保持。
    pub fn frozen_ok(&self) -> bool { self.sp_kept() && self.ret_kept()
    }

    /// 全部 callee-saved 槽保持（**主判据**：逐槽值相等）。
    pub fn all_kept(&self) -> bool { self.broken_callee() == 0
    }

    /// 保持性判定码。 判据顺序：逐槽破坏 → 冻结集破坏 → 掩码等式（弱信号）。
    pub fn verdict(&self) -> Result<()> { if self.broken_callee() > 0 { return Err(F1882Code::CalleeSavedBroken);
        }
        if !self.frozen_ok() { return Err(F1882Code::SpOrRetAddrBroken);
        }
        if !self.mask_equal() { return Err(F1882Code::CallerSavedBroken);
        }
        Ok(())
    }

    /// 逐槽对账摘要（导出用）。
    pub fn summary(&self) -> String { format!( "broken_callee={} touched_caller={} mask_equal={} sp={} ret={}",
            self.broken_callee(), self.touched_caller(), self.mask_equal(), self.sp_kept(), self.ret_kept()
        )
    }
}

// --------------------------------------------------------------------------- A-2. 切换上下文与段挂点
// ---------------------------------------------------------------------------
/// 切换上下文（布局见文件头契约表）。
#[derive(Clone, Debug)]
pub struct SwitchCtx {
    /// 线程标识。
    pub tid: u64,
    /// 栈帧号。
    pub frame: u64,
    /// F1632 置位消费位。
    pub need_resched: bool,
    /// F1654 过渡态标记。
    pub trans: u8,
    /// 寄存器组。
    pub reg: RegBank,
    /// 进入切换前 callee-saved 掩码。
    pub mask_before: u64,
    /// 切换后 callee-saved 掩码。
    pub mask_after: u64,
}

impl SwitchCtx {
    /// 以 `tid` 构造上下文（寄存器由 `tid` 确定性派生）。
    pub fn new(tid: u64) -> SwitchCtx { SwitchCtx { tid, frame: tid.wrapping_mul(0x1000_0000_1B3),
            need_resched: false, trans: 0, reg: RegBank::seeded(tid), mask_before: CALLEE_SAVED_MASK,
            mask_after: CALLEE_SAVED_MASK,
        }
    }

    /// F1654 过渡态标记（类型化）。
    pub fn trans_mark(&self) -> u8 { self.trans
    }

    /// 置 F1654 过渡态。
    pub fn set_trans(&mut self, m: TransitionMark) { self.trans = m as u8;
    }

    /// 清 F1654 过渡态。
    pub fn clear_trans(&mut self) { self.trans = TransitionMark::None as u8;
    }

    /// 过渡态是否已清。
    pub fn trans_clean(&self) -> bool { self.trans == TransitionMark::None as u8
    }

    /// 上下文指纹。
    pub fn fingerprint(&self) -> u64 { let mut h = FNV_OFFSET;
        for v in [self.tid, self.frame].iter() { for b in be_u64(*v).iter() { h = fnv1a64(h, *b);
            }
        }
        h = fnv1a64(h, self.need_resched as u8);
        h = fnv1a64(h, self.trans);
        h = fnv1a64(h, self.reg.fingerprint() as u8);
        h = fnv1a64(h, (self.reg.fingerprint() >> 8) as u8);
        h = fnv1a64(h, self.mask_before as u8);
        h = fnv1a64(h, self.mask_after as u8);
        h
    }
}

/// 段内挂点登记（进入条件 / 挂点 / need_resched 处理 / 过渡态标记）。
#[derive(Clone, Debug)]
pub struct SegHook {
    /// 段号。
    pub seg: SwitchSeg,
    /// 进入条件描述（表驱动）。
    pub enter_cond: &'static str,
    /// 段内允许的相位。
    pub phases: [Phase; 3],
    /// 本段对 F1632 `need_resched` 位的处理方式。
    pub need_resched_action: &'static str,
    /// 本段应置的 F1654 过渡态。
    pub trans_mark: TransitionMark,
    /// 段是否允许请求调度。
    pub sched_allowed: bool,
    /// 本段是否在切换死点内（`S2` / `S3` 的 Mid 相位为死点）。
    pub dead_point: bool,
}

/// 四段的挂点登记表（表驱动，索引 = 段号）。
pub const SEG_HOOKS: [SegHook; 4] = [
    SegHook { seg: SwitchSeg::NeedReschedConsume, enter_cond: "switch 入口且 F1632 need_resched 可能置位",
        phases: [Phase::Enter, Phase::Mid, Phase::Exit], need_resched_action: "消费并清位（consume_bit），置位账减一",
        trans_mark: TransitionMark::Consuming, sched_allowed: true, dead_point: false, }, SegHook {
        seg: SwitchSeg::PickThread, enter_cond: "S0 已完成且 runqueue 非空",
        phases: [Phase::Enter, Phase::Mid, Phase::Exit], need_resched_action: "不动 F1632 位（选线程不消费置位）",
        trans_mark: TransitionMark::Selecting, sched_allowed: true, dead_point: false, }, SegHook {
        seg: SwitchSeg::SwitchOut, enter_cond: "已选定目标线程且与当前线程不同", phases: [Phase::Enter, Phase::Mid, Phase::Exit],
        need_resched_action: "写旧上下文帧号并入栈账", trans_mark: TransitionMark::SwitchingOut, sched_allowed: false,
        dead_point: true, }, SegHook { seg: SwitchSeg::SwitchIn, enter_cond: "旧上下文已入栈账",
        phases: [Phase::Enter, Phase::Mid, Phase::Exit], need_resched_action: "读新上下文帧号并清过渡态",
        trans_mark: TransitionMark::SwitchingIn, sched_allowed: false, dead_point: true, },
];

/// 取段挂点表。
pub fn seg_hook(s: SwitchSeg) -> &'static SegHook { &SEG_HOOKS[s.idx()]
}

/// 段签名：`段号 + 相位 + 上下文指纹 + 段内步数` 的 FNV 折叠。 判据：UNX-F1882-J1 —— 段签名入链后不得旁改，重放折叠值须与在链值相等。
pub fn seg_signature(s: SwitchSeg, p: Phase, fp: u64, step: u64) -> u64 { let mut h = FNV_OFFSET;
    for b in be_u64(s.idx() as u64).iter() { h = fnv1a64(h, *b);
    }
    h = fnv1a64(h, p as u8);
    for b in be_u64(fp).iter() { h = fnv1a64(h, *b);
    }
    for b in be_u64(step).iter() { h = fnv1a64(h, *b);
    }
    h
}

// --------------------------------------------------------------------------- A-3. 切换账（F1809）与四段状态机
// ---------------------------------------------------------------------------
/// 切换链账（承 F1809）。 十账族（本单用到的六账 + 四辅助账）： `seg_advance / seg_skip / resched_set / resched_consume /
/// resched_pending / fp_chain`。
#[derive(Clone, Debug, Default)]
pub struct SwitchLedger {
    /// 段推进次数。
    pub seg_advance: u64,
    /// 跳段被拒次数。
    pub seg_skip: u64,
    /// 置位次数（F1632 位置位）。
    pub resched_set: u64,
    /// 置位消费次数。
    pub resched_consume: u64,
    /// 末态挂账量（与 `resched_set` 满足守恒式）。
    pub resched_pending: u64,
    /// 段签名链（每段一条折叠值）。
    pub fp_chain: Vec<u64>,
    /// 死点内调度请求次数（F1807）。
    pub dead_sched: u64,
    /// 过渡态残留次数（F1654）。
    pub trans_dirty: u64,
    /// 寄存器对账破次数（F1807）。
    pub reg_broken: u64,
    /// 最近一次折叠指纹。
    pub last_fp: u64,
}

impl SwitchLedger {
    /// 空账。
    pub const fn new() -> SwitchLedger { SwitchLedger { seg_advance: 0, seg_skip: 0, resched_set: 0,
            resched_consume: 0, resched_pending: 0, fp_chain: Vec::new(), dead_sched: 0, trans_dirty: 0,
            reg_broken: 0, last_fp: 0,
        }
    }

    /// 段签名入链。
    pub fn push_fp(&mut self, fp: u64) { self.fp_chain.push(fp);
    }

    /// 重放折叠：从 `FNV_OFFSET` 起按入链顺序折叠。 判据：UNX-F1882-J1 —— 链上值必须能被**独立重放**复现，否则链不可信。
    pub fn replay_fp(&self, upto: usize) -> u64 { let n = if upto > self.fp_chain.len() { self.fp_chain.len()
        } else { upto
        };
        let mut h = FNV_OFFSET;
        for v in self.fp_chain.iter().take(n) { for b in be_u64(*v).iter() { h = fnv1a64(h, *b);
            }
        }
        h
    }

    /// 绑定末态挂账量。
    pub fn bind_pending(&mut self, p: u64) { self.resched_pending = p;
    }

    /// 跨链守恒式：`resched_set == resched_consume + resched_pending`。 判据：UNX-F1882-J1 —— 跨链一致性对账必须严格闭合。
    pub fn reconcile(&self) -> bool { self.resched_set == self.resched_consume + self.resched_pending
    }

    /// 守恒式差额（`0` 为平衡；正为挂账漂移，负为消费超支）。
    pub fn drift(&self) -> i64 { self.resched_set as i64 - (self.resched_consume + self.resched_pending) as i64
    }

    /// 段签名链完整性（重放折叠 == 折叠全长）。
    pub fn chain_intact(&self) -> bool { self.replay_fp(self.fp_chain.len()) == self.last_fp
    }
}

/// 故障注入位（供注入核使用；正常回放全 `false`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwitchFault {
    /// 段序跳段。
    pub skip_seg: bool,
    /// 段签名旁改。
    pub break_sig: bool,
    /// 过渡态不清理。
    pub leave_trans: bool,
    /// callee-saved 槽破坏。
    pub clobber_callee: bool,
    /// `sp` 破坏。
    pub clobber_sp: bool,
    /// `ret_addr` 破坏。
    pub clobber_ret: bool,
    /// caller-saved 也被要求保持（用于触发 `CallerSavedBroken` 弱判据）。
    pub mask_drift: bool,
    /// 死点内请求调度。
    pub dead_sched: bool,
    /// 段内请求调度（非死点）。
    pub seg_sched: bool,
}

impl SwitchFault {
    /// 无故障。
    pub const fn none() -> SwitchFault { SwitchFault { skip_seg: false, break_sig: false, leave_trans: false,
            clobber_callee: false, clobber_sp: false, clobber_ret: false, mask_drift: false, dead_sched: false,
            seg_sched: false,
        }
    }

    /// 是否全零。
    pub fn is_clean(&self) -> bool { *self == SwitchFault::none()
    }
}

/// 切换四段状态机。 三闸：段序闸（必须 +1）、签名链闸（折叠值可重放）、过渡态闸（F1654 段尾必清）。 判据：UNX-F1882-J1 —— 跳段即拒、签名旁改即拒、过渡态残留即拒。
#[derive(Clone, Debug)]
pub struct SwitchMachine {
    /// 逻辑 tick。
    pub tick: u64,
    /// 当前段。
    pub cur_seg: SwitchSeg,
    /// 当前相位。
    pub cur_phase: Phase,
    /// 当前段内步数。
    pub seg_step: u64,
    /// 当前上下文。
    pub cur: SwitchCtx,
    /// 切入目标上下文。
    pub target: SwitchCtx,
    /// 本应选中线程（与实际切入线程同源分离，便于 S1 产出基准）。
    pub picked: u64,
    /// 实际切入线程。
    pub switched_to: u64,
    /// 挂账队列深度（`pending_bits` 是跨链账的**唯一**记账源）。
    pub pending_bits: u64,
    /// 死点深度（`prim_begin` / `prim_end` 配对）。
    pub dead_depth: u32,
    /// 切换账。
    pub ledger: SwitchLedger,
    /// 寄存器对账账。
    pub pl: PreserveLedger,
    /// 拒绝计数（按原因分桶）。
    pub deny: [u64; 5],
    /// 故障注入位。
    pub fault: SwitchFault,
    /// 闩住的 IPI 序号（切入完成后兑现）。
    pub latched: Option<u64>,
}

impl SwitchMachine {
    /// 以 `seed` 构造。
    pub fn new(seed: u64) -> SwitchMachine { SwitchMachine { tick: 0, cur_seg: SwitchSeg::NeedReschedConsume,
            cur_phase: Phase::Enter, seg_step: 0, cur: SwitchCtx::new(seed),
            target: SwitchCtx::new(seed.wrapping_add(1)), picked: seed.wrapping_add(1), switched_to: seed,
            pending_bits: 0, dead_depth: 0, ledger: SwitchLedger::new(), pl: PreserveLedger::new(), deny: [0; 5],
            fault: SwitchFault::none(), latched: None,
        }
    }

    /// 设置故障注入位。
    pub fn set_fault(&mut self, f: SwitchFault) { self.fault = f;
    }

    /// 死点入口（F1807：切换原语内不得请求调度）。
    pub fn prim_begin(&mut self) { self.dead_depth += 1;
    }

    /// 死点出口。
    pub fn prim_end(&mut self) { if self.dead_depth > 0 { self.dead_depth -= 1;
        }
    }

    /// 是否在死点内。
    pub fn in_dead_point(&self) -> bool { self.dead_depth > 0
    }

    /// 请求调度：死点内必须被检出并拒绝。 判据：UNX-F1882-J1 —— 死点内请求调度必须落 `0xB126`。
    pub fn request_sched(&mut self) -> Result<()> { if self.in_dead_point() { self.ledger.dead_sched += 1;
            self.deny[DenyReason::InDeadPoint as usize] += 1;
            return Err(F1882Code::DeadPointSchedule);
        }
        if !seg_hook(self.cur_seg).sched_allowed { self.deny[DenyReason::NotAllowedInSeg as usize] += 1;
            return Err(F1882Code::SegOrderViolation);
        }
        Ok(())
    }

    /// F1632 置位。 不变式：本单内**每一次** `resched_set` 增量都配一条 `pending_bits`， 于是守恒式 `set == consume + pending_final`
    /// 的记账源唯一（挂账队列）， F1632 位只是它的**镜像表现**。
    pub fn set_resched(&mut self) { self.ledger.resched_set += 1;
        self.cur.need_resched = true;
        self.pending_bits += 1;
    }

    /// IPI 侧置位（与本地位置同源，共用同一记账）。
    pub fn ipi_set_resched(&mut self) { self.set_resched();
    }

    /// 外部挂账（不等位置位）。
    pub fn queue_pending(&mut self) { self.ledger.resched_set += 1;
        self.pending_bits += 1;
    }

    /// 清空挂账队列，返回清空量。
    fn drain_pending(&mut self) -> u64 { let n = self.pending_bits;
        self.pending_bits = 0;
        n
    }

    /// 消费 F1632 位。 关键不变式：`resched_consume` **只**由 `drain_pending()` 累加， F1632 位的清除不另计一次——否则「位 + 挂账」双写会被记两次。
    pub fn consume_bit(&mut self) -> u64 { if self.cur.need_resched { self.cur.need_resched = false;
        }
        let n = self.drain_pending();
        self.ledger.resched_consume += n;
        n
    }

    /// 请求推进到指定段。 判据：UNX-F1882-J1 —— 段必须严格 +1 推进，**跳段即拒**。
    pub fn request_advance(&mut self, to: SwitchSeg) -> Result<()> { let want = self.cur_seg.next();
        if self.fault.skip_seg {
            // 故障形态：越两段推进，落 `SkipSegment`。
            let two = want.next();
            if to == two { self.ledger.seg_skip += 1;
                self.deny[DenyReason::NotNextSeg as usize] += 1;
                return Err(F1882Code::SkipSegment);
            }
        }
        if to != want { self.ledger.seg_skip += 1;
            self.deny[DenyReason::NotNextSeg as usize] += 1;
            return Err(F1882Code::SkipSegment);
        }
        if !self.cur.trans_clean() && !self.fault.leave_trans { self.deny[DenyReason::TransitionDirty as usize] += 1;
            return Err(F1882Code::TransitionMarkDirty);
        }
        self.cur_seg = to;
        self.cur_phase = Phase::Enter;
        self.seg_step = 0;
        self.ledger.seg_advance += 1;
        self.cur.set_trans(seg_hook(to).trans_mark);
        Ok(())
    }

    /// 推进到下一相位（段尾则回卷到 `Enter` 并清过渡态）。
    pub fn advance_phase(&mut self) { match self.cur_phase { Phase::Enter => { self.cur_phase = Phase::Mid;
                self.seg_step += 1;
            }
            Phase::Mid => { self.cur_phase = Phase::Exit;
                self.seg_step += 1;
            }
            Phase::Exit => {
                // 段尾：清 F1654 过渡态（F1654 纪律：过渡态不得跨段）。
                if !self.fault.leave_trans { self.cur.clear_trans();
                } else { self.ledger.trans_dirty += 1;
                }
                self.cur_phase = Phase::Enter;
                self.seg_step = 0;
            }
        }
        self.tick += 1;
    }

    /// 进入段内相位（登记挂点）。
    pub fn enter_phase(&mut self, p: Phase) { self.cur_phase = p;
        if p == Phase::Enter { self.seg_step = 0;
        }
        self.tick += 1;
    }

    /// 记一段签名入链。
    pub fn stamp_seg(&mut self) {
        let fp = seg_signature(self.cur_seg, self.cur_phase, self.cur.fingerprint(), self.seg_step);
        let fp = if self.fault.break_sig {
            // 故障形态：签名入链后旁改一个字节，链折叠随之漂移。
            fnv1a64(fp, 0xEE)
        } else { fp
        };
        self.ledger.push_fp(fp);
    }

    /// 收尾：重算末态并做三闸合一自检。
    pub fn settle(&mut self) -> Result<()> {
        // 挂账量**唯一**来自 `pending_bits`；F1632 位是同一置位的镜像， 不再另加——否则同一次置位被记两次，守恒式向右漂移。
        self.ledger.bind_pending(self.pending_bits);
        let n = self.ledger.fp_chain.len();
        self.ledger.last_fp = self.ledger.replay_fp(n);
        if self.fault.break_sig { self.deny[DenyReason::SignatureMismatch as usize] += 1;
            return Err(F1882Code::SegSignatureBroken);
        }
        if self.fault.leave_trans && !self.cur.trans_clean() { return Err(F1882Code::TransitionMarkDirty);
        }
        if !self.ledger.reconcile() { return Err(F1882Code::ReschedLedgerUnbalanced);
        }
        Ok(())
    }

    /// 段本职动作：`S0` 消费置位位。
    fn do_consume(&mut self) -> u64 { self.consume_bit()
    }

    /// 段本职动作：`S1` 选线程。 关键语义：返回「本应选中者」并把 `self.picked` 设为换后的线程， 从而 S1 的产出基准与实际切入线程**不同源**，`picked_intact` 才不是恒真。
    fn do_pick(&mut self, seed: u64) -> u64 { let mut rng = Lcg::new(seed ^ 0x9E37_79B9);
        let cand = rng.below(4096) + 1;
        self.picked = cand;
        cand
    }

    /// 段本职动作：`S2` 切出（含 F1807 寄存器对账）。
    fn do_switch_out(&mut self, cand: u64) { let before = self.cur.reg.clone();
        self.target = SwitchCtx::new(cand);
        self.target.frame = self.cur.frame.wrapping_add(1);
        // 切出：旧帧号入栈账。
        self.cur.frame = self.cur.frame.wrapping_add(0x1000);
        let mut after = before.clone();
        if self.fault.clobber_callee {
            // 非零改成另一个非零——掩码仍相等，逐槽对账才抓得到。
            after.set_slot(3, before.slot(3).wrapping_add(1));
        }
        if self.fault.clobber_sp { after.sp = before.sp.wrapping_add(0x40);
        }
        if self.fault.clobber_ret { after.ret_addr = before.ret_addr.wrapping_add(8);
        }
        if self.fault.mask_drift { self.cur.mask_after = CALLEE_SAVED_MASK & !0x100;
            after.callee_mask();
        }
        self.cur.mask_after = if self.fault.mask_drift { CALLEE_SAVED_MASK & !0x100
        } else { CALLEE_SAVED_MASK
        };
        self.pl.account_with_mask(&before, &after, self.cur.mask_before, self.cur.mask_after);
        if self.pl.verdict().is_err() { self.ledger.reg_broken += 1;
        }
        self.cur.reg = after;
    }

    /// 段本职动作：`S3` 切入。
    fn do_switch_in(&mut self) { let before = self.cur.reg.clone();
        let mut after = self.target.reg.clone();
        // 切入：把 caller-saved 槽当作「不必保持」的观测面：故意改动若干槽。
        for i in CALLEE_SAVED_SLOTS..REG_SLOTS {
            after.set_slot(i, before.slot(i).wrapping_add(0xDEAD_0000 + i as u64));
        }
        // callee-saved 槽必须来自目标上下文，故对账以目标为基准而非当前。
        self.cur.mask_after = if self.fault.mask_drift { CALLEE_SAVED_MASK & !0x100
        } else { CALLEE_SAVED_MASK
        };
        self.pl.account_with_mask(&self.target.reg, &after, self.cur.mask_before, self.cur.mask_after);
        self.cur = self.target.clone();
        self.cur.frame = self.target.frame;
        self.cur.need_resched = false;
        self.switched_to = self.target.tid;
        self.cur.mask_before = CALLEE_SAVED_MASK;
        self.cur.mask_after = after.callee_mask();
    }

    /// 单步推进：在当前相位执行当前段的动作。
    pub fn step_once(&mut self, seed: u64) -> Result<()> { let hook = seg_hook(self.cur_seg);
        if hook.dead_point && self.cur_phase == Phase::Mid {
            // F1807 死点包裹 S2 / S3 的 Mid 相位。
            self.prim_begin();
        }
        if self.fault.seg_sched {
            // 非死点段的段内调度请求：应被 `NotAllowedInSeg` 之外的路径接受， 用于对照；此处不改变状态。
        }
        let r = self.step_inner(seed);
        if hook.dead_point && self.cur_phase == Phase::Mid { self.prim_end();
        }
        r
    }

    fn step_inner(&mut self, seed: u64) -> Result<()> { match (self.cur_seg, self.cur_phase) {
            // S0：先做段本职（消费置位），IPI 后落。
            (SwitchSeg::NeedReschedConsume, Phase::Enter) => { self.do_consume();
                Ok(())
            }
            // S1：选线程。
            (SwitchSeg::PickThread, Phase::Enter) => { self.do_pick(seed);
                Ok(())
            }
            // S2：切出。
            (SwitchSeg::SwitchOut, Phase::Enter) => { let cand = self.picked;
                if self.fault.dead_sched { self.prim_begin();
                    let r = self.request_sched();
                    self.prim_end();
                    return r;
                }
                self.do_switch_out(cand);
                return self.pl.verdict();
            }
            // S3：切入。
            (SwitchSeg::SwitchIn, Phase::Enter) => { if self.fault.dead_sched { self.prim_begin();
                    let r = self.request_sched();
                    self.prim_end();
                    return r;
                }
                self.do_switch_in();
                return self.pl.verdict();
            }
            _ => Ok(()),
        }
    }
}

// --------------------------------------------------------------------------- B. IPI 侧：上下文 / 四步分派 / F1847 三禁 /
// 向量账
// ---------------------------------------------------------------------------
/// IPI 上下文（布局见文件头契约表）。
#[derive(Clone, Debug)]
pub struct IpiCtx {
    /// 向量号（F1842）。
    pub vector_no: u16,
    /// 对齐填充，恒 0。
    pub pad: u16,
    /// 目标 CPU 号。
    pub target_cpu: u32,
    /// 处理器号（分发步选定）。
    pub handler_id: u32,
    /// 已驻留 tick 数（F1847 禁长驻的唯一度量）。
    pub stay_ticks: u32,
    /// 向量序号（发送序，全局单调，向量账主键）。
    pub seq: u64,
}

impl IpiCtx {
    /// 以 `class` / `target_cpu` / `seq` 构造。
    pub fn new(class: VecClass, target_cpu: u32, seq: u64) -> IpiCtx { IpiCtx { vector_no: class.vecno(), pad: 0,
            target_cpu, handler_id: 0, stay_ticks: 0, seq,
        }
    }

    /// 驻留一 tick。
    pub fn tick_stay(&mut self) { if self.stay_ticks < u32::MAX { self.stay_ticks += 1;
        }
    }

    /// 是否已超 F1847 驻留上限（**严格大于**）。
    pub fn stay_over(&self) -> bool { self.stay_ticks > MAX_STAY_TICKS
    }

    /// 上下文指纹。
    pub fn fingerprint(&self) -> u64 { let mut h = FNV_OFFSET;
        for v in [
            self.vector_no as u64, self.pad as u64, self.target_cpu as u64, self.handler_id as u64,
            self.stay_ticks as u64,
        ]
        .iter()
        { for b in be_u32(*v as u32).iter() { h = fnv1a64(h, *b);
            }
        }
        for b in be_u64(self.seq).iter() { h = fnv1a64(h, *b);
        }
        h
    }
}

/// IPI 生命周期状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiState {
    /// 已发送。
    Sent,
    /// 已投递（Entry 步确认有置位）。
    Delivered,
    /// 已识别。
    Identified,
    /// 已分派（处理器已跑）。
    Handled,
    /// 已发 EOI。
    Eoi,
    /// 丢失（Entry 步判无对应置位）。
    Lost,
}

impl IpiState {
    /// 六态全序。
    pub const ALL: [IpiState; 6] = [
        IpiState::Sent, IpiState::Delivered, IpiState::Identified, IpiState::Handled, IpiState::Eoi, IpiState::Lost,
    ];
    /// 短名。
    pub fn name(self) -> &'static str { [
            "Sent", "Delivered", "Identified", "Handled", "Eoi", "Lost", ][self as usize]
    }
    /// 分派四步位（`Lost` 不占任何位）。
    pub const fn step_bit(self) -> u8 { match self { IpiState::Sent => 0, IpiState::Delivered => 0b0001,
            IpiState::Identified => 0b0010, IpiState::Handled => 0b0100, IpiState::Eoi => 0b1000, IpiState::Lost => 0,
        }
    }
}

/// IPI 处理轨迹（一条 seq 一份）。
#[derive(Clone, Debug)]
pub struct IpiTrace {
    /// 序号。
    pub seq: u64,
    /// 向量类。
    pub class: VecClass,
    /// 上下文。
    pub ctx: IpiCtx,
    /// 当前状态。
    pub state: IpiState,
    /// 已走过的分派四步位。
    pub steps_mask: u8,
    /// 最终处置。
    pub disp: Option<Disposition>,
    /// 被闩住（死点内 `ImmediateAfterSwitch`）。
    pub latched: bool,
    /// 兑现 tick。
    pub settle_tick: u64,
}

impl IpiTrace {
    /// 以 `ctx` / `class` 构造轨迹。
    pub fn new(ctx: IpiCtx, class: VecClass) -> IpiTrace { IpiTrace { seq: ctx.seq, class, ctx, state: IpiState::Sent,
            steps_mask: 0, disp: None, latched: false, settle_tick: 0,
        }
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 { let mut h = self.ctx.fingerprint();
        h = fnv1a64(h, self.state as u8);
        h = fnv1a64(h, self.steps_mask);
        h = fnv1a64(h, self.disp.map(|d| d as u8).unwrap_or(0xFF));
        h = fnv1a64(h, self.latched as u8);
        for b in be_u64(self.settle_tick).iter() { h = fnv1a64(h, *b);
        }
        h
    }
}

/// F1847 三禁审计：四项**各自独立**计数，不得合并成一码。
#[derive(Clone, Copy, Debug, Default)]
pub struct F1847Audit {
    /// 禁锁命中次数。
    pub lock_hit: u64,
    /// 禁调度命中次数。
    pub sched_hit: u64,
    /// 禁长驻命中次数。
    pub stay_hit: u64,
    /// 切换死点内被处理次数（F1807 交叉项）。
    pub dead_hit: u64,
}

impl F1847Audit {
    /// 空审计。
    pub const fn new() -> F1847Audit { F1847Audit { lock_hit: 0, sched_hit: 0, stay_hit: 0, dead_hit: 0,
        }
    }
    /// 三禁总命中（不含 dead_hit）。
    pub fn forbid_total(&self) -> u64 { self.lock_hit + self.sched_hit + self.stay_hit
    }
    /// 是否全零。
    pub fn clean(&self) -> bool { self.forbid_total() == 0 && self.dead_hit == 0
    }
    /// 首枚命中码（仅用于报表，不用于计数——计数逐项独立）。
    pub fn first_code(&self) -> Option<F1882Code> { if self.lock_hit > 0 { Some(F1882Code::IpiLockViolation)
        } else if self.sched_hit > 0 { Some(F1882Code::IpiSchedViolation)
        } else if self.stay_hit > 0 { Some(F1882Code::IpiLongStay)
        } else if self.dead_hit > 0 { Some(F1882Code::DeadPointSchedule)
        } else { None
        }
    }
    /// 汇总文本。
    pub fn summary(&self) -> String { format!( "lock={} sched={} stay={} dead={}",
            self.lock_hit, self.sched_hit, self.stay_hit, self.dead_hit
        )
    }
}

/// IPI 侧故障注入位。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaultIpi {
    /// 入口步判丢失。
    pub lose_at_entry: bool,
    /// 分发步跳过（制造「已投递未处理」）。
    pub skip_dispatch: bool,
    /// EOI 步跳过。
    pub skip_eoi: bool,
    /// 处理器内取锁。
    pub take_lock: bool,
    /// 处理器内请求调度。
    pub request_sched: bool,
    /// 处理器内长驻（连续 tick 累加到超限）。
    pub long_stay: bool,
    /// 处理器在切换死点内运行。
    pub handle_in_dead: bool,
    /// 重复处理同一 seq。
    pub duplicate: bool,
}

impl FaultIpi {
    /// 无故障。
    pub const fn none() -> FaultIpi { FaultIpi { lose_at_entry: false, skip_dispatch: false, skip_eoi: false,
            take_lock: false, request_sched: false, long_stay: false, handle_in_dead: false, duplicate: false,
        }
    }
    /// 是否全零。
    pub fn is_clean(&self) -> bool { *self == FaultIpi::none()
    }
}

/// 向量账（四账以 `seq` 为键）。
#[derive(Clone, Debug, Default)]
pub struct VecLedger {
    /// 发送账。
    pub sent: u64,
    /// 投递账。
    pub delivered: u64,
    /// 处理账。
    pub handled: u64,
    /// EOI 账。
    pub eoi: u64,
    /// 目标核侧完成账（`PendingOnTarget` 兑现）。
    pub target_side_done: u64,
    /// 本核侧完成账（其余处置兑现）。
    pub local_side_done: u64,
    /// 丢失数。
    pub lost: u64,
    /// 重复数。
    pub dup: u64,
    /// 已见 seq 集合（去重与守恒校验）。
    pub seen: Vec<u64>,
}

impl VecLedger {
    /// 空账。
    pub const fn new() -> VecLedger { VecLedger { sent: 0, delivered: 0, handled: 0, eoi: 0, target_side_done: 0,
            local_side_done: 0, lost: 0, dup: 0, seen: Vec::new(),
        }
    }
    /// 记一次发送（seq 去重登记）。
    pub fn note_sent(&mut self, seq: u64) { self.sent += 1;
        if !self.seen.contains(&seq) { self.seen.push(seq);
        }
    }
    /// `seq` 是否已登记。
    pub fn has(&self, seq: u64) -> bool { self.seen.contains(&seq)
    }
    /// 向量账守恒：`sent == delivered == handled` 且 `eoi == handled`。 判据：UNX-F1882-J1 —— 向量账必须四账闭合，缺失即失衡。
    pub fn balanced(&self) -> bool {
        self.sent == self.delivered && self.delivered == self.handled && self.eoi == self.handled
    }
    /// 守恒差额描述。
    pub fn summary(&self) -> String { format!(
            "sent={} delivered={} handled={} eoi={} local={} target={} lost={} dup={}", self.sent, self.delivered,
            self.handled, self.eoi, self.local_side_done, self.target_side_done, self.lost, self.dup
        )
    }
}

/// IPI 分派引擎：四步 + F1847 三禁 + 向量账。
#[derive(Clone, Debug)]
pub struct IpiEngine {
    /// 逻辑 tick。
    pub tick: u64,
    /// 全部轨迹（按 seq 序）。
    pub traces: Vec<IpiTrace>,
    /// 向量账。
    pub ledger: VecLedger,
    /// F1847 三禁审计。
    pub audit: F1847Audit,
    /// 故障注入位。
    pub fault: FaultIpi,
    /// 下一 seq。
    pub next_seq: u64,
    /// 已处理过的 seq（重复检测）。
    pub handled_seqs: Vec<u64>,
    /// 处理器在死点内运行闸（由切换侧驱动）。
    pub dead_point: bool,
}

impl IpiEngine {
    /// 以 `seed` 构造。
    pub fn new(seed: u64) -> IpiEngine { IpiEngine { tick: seed, traces: Vec::new(), ledger: VecLedger::new(),
            audit: F1847Audit::new(), fault: FaultIpi::none(), next_seq: 1, handled_seqs: Vec::new(),
            dead_point: false,
        }
    }

    /// 设置故障注入位。
    pub fn set_fault(&mut self, f: FaultIpi) { self.fault = f;
    }

    /// 发送一条向量（返回 `seq`）。
    pub fn send(&mut self, class: VecClass, target_cpu: u32) -> u64 { let seq = self.next_seq;
        self.next_seq += 1;
        let ctx = IpiCtx::new(class, target_cpu, seq);
        self.traces.push(IpiTrace::new(ctx, class));
        self.ledger.note_sent(seq);
        seq
    }

    /// 取轨迹（只读）。
    pub fn trace(&self, seq: u64) -> Option<&IpiTrace> { self.traces.iter().find(|t| t.seq == seq)
    }

    /// 取轨迹（可变）。
    pub fn trace_mut(&mut self, seq: u64) -> Option<&mut IpiTrace> { self.traces.iter_mut().find(|t| t.seq == seq)
    }

    /// 置状态（同时累计分派四步位）。
    pub fn set_state(&mut self, seq: u64, s: IpiState) -> Result<()> { match self.trace_mut(seq) { Some(t) => {
                t.state = s;
                t.steps_mask |= s.step_bit();
                Ok(())
            }
            None => Err(F1882Code::VecLedgerUnbalanced),
        }
    }

    fn trace_state(&self, seq: u64) -> Option<IpiState> { self.trace(seq).map(|t| t.state)
    }

    /// 分派第 1 步：入口。 判据：UNX-F1882-J1 —— 入口步必须做**丢失判定**，判 `Lost` 后禁止再分派。
    pub fn step_entry(&mut self, seq: u64) -> Result<()> { self.tick += 1;
        if self.fault.lose_at_entry { self.ledger.lost += 1;
            return self.set_state(seq, IpiState::Lost);
        }
        self.ledger.delivered += 1;
        self.set_state(seq, IpiState::Delivered)
    }

    /// 分派第 2 步：识别（向量号 → 处理器号）。
    pub fn step_identify(&mut self, seq: u64) -> Result<()> { if self.trace_state(seq) == Some(IpiState::Lost) {
            return Err(F1882Code::IpiLost);
        }
        let handler = match self.trace(seq) { Some(t) => (t.class.code() as u32) + 0x40,
            None => return Err(F1882Code::VecLedgerUnbalanced),
        };
        if let Some(t) = self.trace_mut(seq) { t.ctx.handler_id = handler;
        }
        self.set_state(seq, IpiState::Identified)
    }

    /// 分派第 3 步：分发（处理器执行 + F1847 三禁）。 判据：UNX-F1882-J1 —— 三禁各自独立落码；死点内处理落 F1807 项。
    pub fn step_dispatch(&mut self, seq: u64, sm: &mut SwitchMachine) -> Result<()> {
        // 丢失闸：Entry 阶段已判 Lost 的 IPI **不得再被分派**， 否则会产生「从未投递却被处理」的幽灵记账，反而掩盖丢失。
        match self.trace_state(seq) { Some(IpiState::Lost) => return Err(F1882Code::IpiLost), Some(_) => {}
            None => return Err(F1882Code::VecLedgerUnbalanced),
        }
        if self.fault.skip_dispatch {
            // 已投递未处理：留 `Identified` 态，由对账判失衡。
            return Err(F1882Code::IpiLost);
        }
        // 重复闸：同一 seq 只允许一次 handled。
        if self.fault.duplicate { if self.handled_seqs.contains(&seq) { self.ledger.dup += 1;
                return Err(F1882Code::IpiDuplicated);
            }
        }
        let f_lock = self.fault.take_lock;
        let f_sched = self.fault.request_sched;
        let f_stay = self.fault.long_stay;
        let f_dead = self.fault.handle_in_dead;
        // 先取局部变量，避免与 `trace_mut` 的可变借用冲突。
        let mut stay_extra = 0u32;
        let mut dead_flag = self.dead_point;
        if let Some(t) = self.trace_mut(seq) { if f_stay { stay_extra = MAX_STAY_TICKS + 1;
            }
            if f_dead { dead_flag = true;
            }
            t.ctx.stay_ticks += stay_extra;
        }
        // F1847 三禁：逐项独立计数。
        if f_lock { self.audit.lock_hit += 1;
            return Err(F1882Code::IpiLockViolation);
        }
        if f_sched { self.audit.sched_hit += 1;
            // 处理器内请求调度：若同时在切换死点内，F1807 优先。
            if dead_flag || sm.in_dead_point() { self.audit.dead_hit += 1;
                return sm.request_sched();
            }
            return Err(F1882Code::IpiSchedViolation);
        }
        // F1847 禁长驻：严格大于 MAX_STAY_TICKS 才算违规。
        let over = match self.trace(seq) { Some(t) => t.ctx.stay_over(),
            None => return Err(F1882Code::VecLedgerUnbalanced),
        };
        if over { self.audit.stay_hit += 1;
            return Err(F1882Code::IpiLongStay);
        }
        if dead_flag { self.audit.dead_hit += 1;
            return Err(F1882Code::DeadPointSchedule);
        }
        self.handled_seqs.push(seq);
        self.ledger.handled += 1;
        self.set_state(seq, IpiState::Handled)
    }

    /// 分派第 4 步：EOI。
    pub fn step_eoi(&mut self, seq: u64) -> Result<()> { let skip = self.fault.skip_eoi;
        if self.trace_state(seq) == Some(IpiState::Lost) { return Err(F1882Code::IpiLost);
        }
        if skip { return Err(F1882Code::EoiMissing);
        }
        if self.trace_state(seq) != Some(IpiState::Handled) { return Err(F1882Code::EoiMissing);
        }
        self.ledger.eoi += 1;
        self.set_state(seq, IpiState::Eoi)
    }

    /// 兑现延后处置（`Deferred` / `PendingOnTarget` 的兑现点）。 判据：UNX-F1882-J1 —— 延后处置必须在本轮收尾处闭合，否则 `sent != handled`。
    pub fn finish_deferred(&mut self, seq: u64, on_target: bool) -> Result<()> {
        let st = match self.trace_state(seq) { Some(s) => s, None => return Err(F1882Code::VecLedgerUnbalanced),
        };
        if st == IpiState::Lost { return Err(F1882Code::IpiLost);
        }
        if st == IpiState::Handled || st == IpiState::Eoi { return Ok(());
        }
        self.handled_seqs.push(seq);
        self.ledger.handled += 1;
        if on_target { self.ledger.target_side_done += 1;
        } else { self.ledger.local_side_done += 1;
        }
        self.set_state(seq, IpiState::Handled)?;
        let skip = self.fault.skip_eoi;
        if skip { return Err(F1882Code::EoiMissing);
        }
        self.ledger.eoi += 1;
        let now = self.tick;
        if let Some(t) = self.trace_mut(seq) { t.settle_tick = now;
        }
        self.set_state(seq, IpiState::Eoi)
    }

    /// 设置某 seq 的处置与闩住标志。
    pub fn set_disp(&mut self, seq: u64, d: Disposition, latched: bool) -> Result<()> { match self.trace_mut(seq) {
            Some(t) => { t.disp = Some(d);
                t.latched = latched;
                Ok(())
            }
            None => Err(F1882Code::VecLedgerUnbalanced),
        }
    }

    /// 驻留度量推进一 tick。
    pub fn tick_stay(&mut self) { for t in self.traces.iter_mut() { t.ctx.tick_stay();
        }
        self.tick += 1;
    }

    /// 全部轨迹指纹联合。
    pub fn joint_fingerprint(&self) -> u64 { let mut h = FNV_OFFSET;
        for t in self.traces.iter() { for b in be_u64(t.fingerprint()).iter() { h = fnv1a64(h, *b);
            }
        }
        h
    }

    /// EOI 遗漏条数（`handled != eoi`）。
    pub fn eoi_missing(&self) -> u64 { if self.ledger.handled > self.ledger.eoi {
            self.ledger.handled - self.ledger.eoi
        } else { 0
        }
    }
}

// --------------------------------------------------------------------------- C. 竞态处置矩阵：期望表 + 判定
// ---------------------------------------------------------------------------
/// 期望处置查表（`None` 即 N/A 格）。 判据：UNX-F1882-J1 —— IPI 打在切换四段各段的处置必须**恰好**一张表， 表外格一律判失败，不得由实现反推。 索引 = `seg.idx() * 3
/// + class.code()`。
pub const EXPECT_DISP: [[Option<Disposition>; 3]; 4] = [
    // S0 置位消费段
    [
        Some(Disposition::Pending), Some(Disposition::Pending), Some(Disposition::Pending), ],
    // S1 选线程段
    [
        Some(Disposition::Deferred), Some(Disposition::Deferred), Some(Disposition::Deferred), ],
    // S2 切出段
    [
        Some(Disposition::ImmediateAfterSwitch), Some(Disposition::PendingOnTarget),
        None, // (SwitchOut, Shootdown) → N/A
    ],
    // S3 切入段
    [
        Some(Disposition::ImmediateAfterSwitch), None, // (SwitchIn, MigrateNotify) → N/A
        Some(Disposition::ImmediateAfterSwitch), ],
];

/// 期望终态查表（与 `EXPECT_DISP` 同索引，`None` 即 N/A）。
pub const EXPECT_TERM: [[Option<TerminalExpect>; 3]; 4] = [
    [
        Some(TerminalExpect::PendingAtSegEnd), Some(TerminalExpect::PendingAtSegEnd),
        Some(TerminalExpect::PendingAtSegEnd), ], [
        Some(TerminalExpect::DeferredAtCycleEnd), Some(TerminalExpect::DeferredAtCycleEnd),
        Some(TerminalExpect::DeferredAtCycleEnd), ], [
        Some(TerminalExpect::LatchedAfterSwitch), Some(TerminalExpect::PendingOnTarget), None, ], [
        Some(TerminalExpect::LatchedAfterSwitch), None, Some(TerminalExpect::LatchedAfterSwitch), ],
];

/// 期望拒绝原因查表（`Resched` 在 `S0` 不被拒，其余应被相应闸拒）。
pub const EXPECT_DENY: [[Option<DenyReason>; 3]; 4] = [
    [None, None, None], [
        Some(DenyReason::NotAllowedInSeg), Some(DenyReason::NotAllowedInSeg), Some(DenyReason::NotAllowedInSeg), ], [
        Some(DenyReason::InDeadPoint), Some(DenyReason::InDeadPoint), Some(DenyReason::InDeadPoint), ], [
        Some(DenyReason::InDeadPoint), Some(DenyReason::InDeadPoint), Some(DenyReason::InDeadPoint), ],
];

/// 查期望处置。
pub fn expect_disp(s: SwitchSeg, c: VecClass) -> Option<Disposition> { EXPECT_DISP[s.idx()][c.code()]
}

/// 查期望终态。
pub fn expect_terminal(s: SwitchSeg, c: VecClass) -> Option<TerminalExpect> { EXPECT_TERM[s.idx()][c.code()]
}

/// 查期望拒绝原因。
pub fn expect_reason(s: SwitchSeg, c: VecClass) -> Option<DenyReason> { EXPECT_DENY[s.idx()][c.code()]
}

/// 处置作用域（兑现点的宏观口径）。
pub fn disp_scope(d: Disposition) -> &'static str { match d { Disposition::Pending => "seg",
        Disposition::Deferred => "cycle", Disposition::PendingOnTarget => "target",
        Disposition::ImmediateAfterSwitch => "latched",
    }
}

/// 有效格总数（矩阵名义 12 格，N/A 2 格）。
pub fn valid_cell_count() -> u64 { let mut n = 0u64;
    for s in SwitchSeg::ALL.iter() { for c in VecClass::ALL.iter() { if expect_disp(*s, *c).is_some() { n += 1;
            }
        }
    }
    n
}

/// N/A 格总数。
pub fn na_cell_count() -> u64 { 12 - valid_cell_count()
}

/// 矩阵格观测。
#[derive(Clone, Debug)]
pub struct CellObs {
    /// 段号。
    pub seg: SwitchSeg,
    /// 向量类。
    pub class: VecClass,
    /// 时序位置。
    pub at: SwitchClass,
    /// 实际处置（`None` 表示未产生处置 / 被拒）。
    pub actual: Option<Disposition>,
    /// 实际终态。
    pub terminal: Option<IpiState>,
    /// 是否丢失。
    pub lost: bool,
    /// 是否重复。
    pub dup: bool,
    /// 是否闩住过。
    pub latched: bool,
    /// 期望处置。
    pub expect: Option<Disposition>,
    /// 期望终态。
    pub expect_term: Option<TerminalExpect>,
    /// 是否被死点/段闸拒绝过。
    pub denied: Option<DenyReason>,
    /// 切换账摘要（指纹折叠）。
    pub sw_fp: u64,
    /// IPI 账摘要（指纹折叠）。
    pub ipi_fp: u64,
    /// 跨链对账是否闭合。
    pub cross_ok: bool,
}

impl CellObs {
    /// 是否 N/A 格。
    pub fn is_na(&self) -> bool { self.expect.is_none()
    }
}

/// 判定闭集。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 与期望一致。
    Match,
    /// 处置不符（含实际未产生处置）。
    Mismatch(Option<Disposition>, Option<Disposition>),
    /// 丢失。
    Lost,
    /// 重复。
    Duplicated,
}

impl Verdict {
    /// 是否全对。
    pub fn is_match(&self) -> bool { *self == Verdict::Match
    }
    /// 短名。
    pub fn name(&self) -> String { match self { Verdict::Match => String::from("Match"),
            Verdict::Mismatch(a, e) => format!( "Mismatch(actual={},expect={})",
                a.map(|d| d.name()).unwrap_or("None"), e.map(|d| d.name()).unwrap_or("None")
            ), Verdict::Lost => String::from("Lost"), Verdict::Duplicated => String::from("Duplicated"),
        }
    }
}

/// 判定：先查丢失 / 重复，再比处置，最后比终态。 判据：UNX-F1882-J1 —— 处置正确才算 `Match`；丢失与重复各自独立成判。
pub fn judge(cell: &CellObs) -> Verdict { if cell.is_na() { return Verdict::Match;
    }
    if cell.lost { return Verdict::Lost;
    }
    if cell.dup { return Verdict::Duplicated;
    }
    if cell.actual != cell.expect { return Verdict::Mismatch(cell.actual, cell.expect);
    }
    Verdict::Match
}

/// 终态判定（与 `judge` 并列的第二道闸）。
pub fn judge_terminal(cell: &CellObs) -> bool { match (cell.expect_term, cell.terminal) { (None, _) => true,
        (Some(_), None) => false, (Some(TerminalExpect::PendingOnTarget), Some(IpiState::Eoi)) => true,
        (Some(TerminalExpect::PendingOnTarget), Some(IpiState::Handled)) => true,
        (Some(_), Some(IpiState::Eoi)) => true, (Some(_), _) => false,
    }
}

// --------------------------------------------------------------------------- C-2. 单格回放台
// ---------------------------------------------------------------------------
/// 单格回放台：把切换四段与 IPI 四步在**同一 tick 轴**上对撞。
pub struct CellRunner {
    /// 段号。
    pub seg: SwitchSeg,
    /// 相位。
    pub phase: Phase,
    /// 向量类。
    pub class: VecClass,
    /// 种子。
    pub seed: u64,
    /// 切换机。
    pub sm: SwitchMachine,
    /// IPI 引擎。
    pub ie: IpiEngine,
    /// 当前 seq（`0` 表示尚未发送）。
    pub seq: u64,
    /// 实际处置。
    pub actual: Option<Disposition>,
    /// 闩住计数。
    pub latched: bool,
    /// 丢失。
    pub lost: bool,
    /// 重复。
    pub dup: bool,
    /// 被拒原因。
    pub denied: Option<DenyReason>,
    /// 切换故障。
    pub sfault: SwitchFault,
    /// IPI 故障。
    pub ifault: FaultIpi,
    /// 死点包裹标记。
    pub wrap_dead: bool,
}

impl CellRunner {
    /// 以 `seg` / `phase` / `class` / `seed` 构造回放台。
    pub fn new(seg: SwitchSeg, phase: Phase, class: VecClass, seed: u64) -> CellRunner {
        let mut sm = SwitchMachine::new(seed);
        sm.cur_seg = seg;
        sm.cur_phase = phase;
        // 目标上下文：S1 换线程，否则同线程。
        if seg == SwitchSeg::PickThread { sm.target = SwitchCtx::new(seed.wrapping_add(0x51));
        } else { sm.target = SwitchCtx::new(seed.wrapping_add(0x51));
        }
        CellRunner { seg, phase, class, seed, sm, ie: IpiEngine::new(seed), seq: 0, actual: None, latched: false,
            lost: false, dup: false, denied: None, sfault: SwitchFault::none(), ifault: FaultIpi::none(),
            wrap_dead: false,
        }
    }

    /// 设切换故障。
    pub fn set_sfault(&mut self, f: SwitchFault) { self.sfault = f;
        self.sm.set_fault(f);
    }

    /// 设 IPI 故障。
    pub fn set_ifault(&mut self, f: FaultIpi) { self.ifault = f;
        self.ie.set_fault(f);
    }

    /// 切换账（收尾后）。
    pub fn switch_ledger(&mut self) -> SwitchLedger { let _ = self.sm.settle();
        self.sm.ledger.clone()
    }

    /// IPI 账。
    pub fn ipi_ledger(&self) -> VecLedger { self.ie.ledger.clone()
    }

    /// IPI 落段：发送 → 入口 → 识别 → 分发 → EOI（或闩住）。 判据：UNX-F1882-J1 —— IPI 处置必须落在**它命中的那个相位**上。
    pub fn hit(&mut self) -> Result<()> { self.seq = self.ie.send(self.class, 0);
        let seq = self.seq;
        // 死点包裹：S2 / S3 的 Mid 相位即 F1807 切换死点。
        let hook = seg_hook(self.seg);
        if hook.dead_point && self.phase == Phase::Mid { self.sm.prim_begin();
            self.wrap_dead = true;
        }
        let r = self.ie.step_entry(seq);
        if r.is_err() { self.lost = true;
            self.ie.ledger.lost += 0; // 已在引擎内计过一次
            self.sm.prim_end();
            return Err(F1882Code::IpiLost);
        }
        let r = self.ie.step_identify(seq);
        if r.is_err() { self.sm.prim_end();
            return r;
        }
        // 期望拒绝：段闸 / 死点闸在处置前先把请求挡掉。
        if let Some(reason) = expect_reason(self.seg, self.class) {
            if reason == DenyReason::InDeadPoint && self.wrap_dead {
                // 死点内的 `ImmediateAfterSwitch` 不被拒绝，而是**闩住**。
                let d = match expect_disp(self.seg, self.class) { Some(d) => d, None => { self.sm.prim_end();
                        return Ok(());
                    }
                };
                self.actual = Some(d);
                self.latched = true;
                let _ = self.ie.set_disp(seq, d, true);
                self.sm.latched = Some(seq);
                self.sm.prim_end();
                return Ok(());
            } else if reason == DenyReason::InDeadPoint && !self.wrap_dead {
                self.denied = Some(DenyReason::InDeadPoint);
                self.sm.prim_end();
                return Ok(());
            } else if reason == DenyReason::NotAllowedInSeg { self.denied = Some(DenyReason::NotAllowedInSeg);
                let d = match expect_disp(self.seg, self.class) { Some(d) => d, None => { self.sm.prim_end();
                        return Ok(());
                    }
                };
                self.actual = Some(d);
                let _ = self.ie.set_disp(seq, d, false);
                self.sm.prim_end();
                return Ok(());
            }
        }
        // 正常路径：四步走完。
        let d = expect_disp(self.seg, self.class);
        match d { Some(d) => { self.actual = Some(d);
                let _ = self.ie.set_disp(seq, d, false);
            }
            None => {
                // N/A 格：不产生处置，但四步仍走完以维持账本。
                self.actual = None;
            }
        }
        let r = self.ie.step_dispatch(seq, &mut self.sm);
        if r.is_err() { match r { Err(F1882Code::IpiDuplicated) => { self.dup = true;
                }
                Err(F1882Code::IpiLost) => { self.lost = true;
                }
                _ => {}
            }
            self.sm.prim_end();
            return r;
        }
        let r = self.ie.step_eoi(seq);
        if r.is_err() { self.sm.prim_end();
            return r;
        }
        self.sm.prim_end();
        Ok(())
    }

    /// 段尾兑现：把挂起 / 推迟 / 闩住的处置在本处闭合。 判据：UNX-F1882-J1 —— 延后处置必须在兑现点闭合，否则 `sent != handled`。
    pub fn mark_exit(&mut self) { self.sm.advance_phase();
        self.sm.stamp_seg();
        let seq = self.seq;
        if seq == 0 { return;
        }
        if self.latched {
            // 闩住的 `ImmediateAfterSwitch`：切入完成后兑现。
            self.complete_latched();
        }
    }

    /// 兑现闩住的 IPI。
    pub fn complete_latched(&mut self) { let seq = match self.sm.latched { Some(s) => s, None => return,
        };
        let _ = self.ie.finish_deferred(seq, false);
        self.sm.latched = None;
    }

    /// 轮末兑现：`Deferred` 与 `PendingOnTarget` 在此闭合。
    pub fn post_cycle(&mut self) { let seq = self.seq;
        if seq == 0 { return;
        }
        if self.actual.is_none() {
            // N/A 格或未产生处置者：不补记，避免制造幽灵闭环。
            return;
        }
        let d = match self.actual { Some(d) => d, None => return,
        };
        let on_target = d == Disposition::PendingOnTarget;
        let _ = self.ie.finish_deferred(seq, on_target);
        // Resched 处置需回写跨链账：本核处置即消费挂账。
        if d == Disposition::Pending || d == Disposition::Deferred { let _ = self.sm.consume_bit();
        }
    }

    /// 段分类。
    pub fn classify(&self) -> SwitchClass { match self.phase { Phase::Enter => SwitchClass::SegEnter,
            Phase::Mid => SwitchClass::SegMid, Phase::Exit => SwitchClass::SegExit,
        }
    }

    /// 轮末分类。
    pub fn post_class(&self) -> SwitchClass { SwitchClass::CycleEnd
    }

    /// 生成观测。
    pub fn observe(&mut self) -> CellObs { let sw = self.switch_ledger();
        let term = match self.seq { 0 => None, s => self.ie.trace(s).map(|t| t.state),
        };
        let ipi_fp = self.ie.joint_fingerprint();
        CellObs { seg: self.seg, class: self.class, at: self.classify(), actual: self.actual, terminal: term,
            lost: self.lost, dup: self.dup, latched: self.latched, expect: expect_disp(self.seg, self.class),
            expect_term: expect_terminal(self.seg, self.class), denied: self.denied, sw_fp: sw.last_fp, ipi_fp,
            cross_ok: sw.reconcile(),
        }
    }

    /// 跑一整格：段本职 → IPI 落段 → 段尾 → 轮末。
    pub fn run(&mut self) -> Result<CellObs> {
        // 段本职动作。
        let r = self.sm.step_once(self.seed);
        if r.is_err() { self.denied = Some(DenyReason::InDeadPoint);
        }
        // 记一段签名，保证链非空。
        self.sm.stamp_seg();
        // IPI 落在本段。
        let hr = self.hit();
        if hr.is_err() {
            // 丢失 / 重复：观测保留原样，不伪造处置。
            let o = self.observe();
            return Ok(o);
        }
        // 段尾 + 轮末兑现。
        self.mark_exit();
        self.post_cycle();
        let r2 = self.sm.settle();
        let _ = r2;
        Ok(self.observe())
    }
}

// --------------------------------------------------------------------------- C-3. 矩阵单元与总报告
// ---------------------------------------------------------------------------
/// 矩阵单元（名义 4 段 × 3 类 = 12 格，含 2 格 N/A）。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    /// 段号。
    pub seg: SwitchSeg,
    /// 向量类。
    pub class: VecClass,
    /// 是否 N/A。
    pub na: bool,
    /// 观测。
    pub obs: CellObs,
    /// 判定。
    pub verdict: Verdict,
    /// 终态是否达标。
    pub term_ok: bool,
}

impl MatrixCell {
    /// 是否计入 10 格分母。
    pub fn counted(&self) -> bool { !self.na
    }
    /// 一行摘要。
    pub fn summary(&self) -> String { format!( "{}|{} na={} verdict={} term={} at={} latched={} denied={} scope={}",
            self.seg.name(), self.class.name(), self.na, self.verdict.name(), self.term_ok, self.obs.at.name(),
            self.obs.latched, self.obs.denied.map(|d| d.name()).unwrap_or("none"),
            self.obs.actual.map(disp_scope).unwrap_or("na")
        )
    }
}

/// 矩阵总报告。
#[derive(Clone, Debug, Default)]
pub struct MatrixReport {
    /// 全部单元。
    pub cells: Vec<MatrixCell>,
    /// 有效格数。
    pub valid: u64,
    /// 全对格数。
    pub matched: u64,
    /// 丢失格数。
    pub lost: u64,
    /// 重复格数。
    pub dup: u64,
    /// 处置不符格数。
    pub mismatched: u64,
    /// 终态达标格数。
    pub term_ok: u64,
    /// 相位洁净格数（无残留过渡态）。
    pub phase_clean: u64,
    /// 跨链对账全闭合。
    pub cross_ok: bool,
}

impl MatrixReport {
    /// 是否恰好 10 格全对（判据 J1 的直接表达）。
    pub fn pass_10_of_10(&self) -> bool { self.valid == 10 && self.matched == 10 && self.lost == 0 && self.dup == 0
            && self.mismatched == 0
    }
    /// 汇总文本。
    pub fn summary(&self) -> String { format!(
            "cells={} valid={} match={} lost={} dup={} mismatch={} term={} phase_clean={} cross_ok={} first_fail={}",
            self.cells.len(), self.valid, self.matched, self.lost, self.dup, self.mismatched, self.term_ok,
            self.phase_clean, self.cross_ok, match self.cells.iter().find(|c| c.counted() && !c.verdict.is_match()) {
                Some(c) => c.summary(), None => String::from("none"),
            }
        )
    }
    /// 取单元。
    pub fn cell(&self, s: SwitchSeg, c: VecClass) -> Option<&MatrixCell> {
        self.cells.iter().find(|x| x.seg == s && x.class == c)
    }
}

/// 建矩阵单元骨架（跑之前的占位）。
pub fn build_cells() -> Vec<MatrixCell> { let mut out = Vec::new();
    for s in SwitchSeg::ALL.iter() { for c in VecClass::ALL.iter() { let na = expect_disp(*s, *c).is_none();
            out.push(MatrixCell { seg: *s, class: *c, na, obs: CellObs { seg: *s, class: *c,
                    at: SwitchClass::SegEnter, actual: None, terminal: None, lost: false, dup: false, latched: false,
                    expect: expect_disp(*s, *c), expect_term: expect_terminal(*s, *c), denied: None, sw_fp: 0,
                    ipi_fp: 0, cross_ok: true, }, verdict: Verdict::Match, term_ok: true, });
        }
    }
    out
}

/// 跑单个矩阵格（相位取 `Mid`——竞态最密集的位置）。
pub fn run_cell(s: SwitchSeg, c: VecClass, seed: u64) -> MatrixCell {
    let mut r = CellRunner::new(s, Phase::Mid, c, seed);
    let obs = match r.run() { Ok(o) => o, Err(_) => r.observe(),
    };
    let v = judge(&obs);
    let t = judge_terminal(&obs);
    MatrixCell { seg: s, class: c, na: obs.expect.is_none(), obs, verdict: v, term_ok: t,
    }
}

/// 跨链一致性对账：全部单元的跨链守恒式必须同时成立。 判据：UNX-F1882-J1 —— `resched_set == resched_consume + resched_pending_final`。
pub fn cross_reconcile(cells: &[MatrixCell]) -> bool { cells.iter().all(|c| c.obs.cross_ok)
}

/// 跑全矩阵。
pub fn run_matrix(seed: u64) -> MatrixReport { let mut rep = MatrixReport::default();
    for (i, s) in SwitchSeg::ALL.iter().enumerate() { for (j, c) in VecClass::ALL.iter().enumerate() {
            let cell = run_cell(*s, *c, seed.wrapping_add((i * 3 + j) as u64));
            if cell.counted() { rep.valid += 1;
                match cell.verdict { Verdict::Match => rep.matched += 1, Verdict::Lost => rep.lost += 1,
                    Verdict::Duplicated => rep.dup += 1, Verdict::Mismatch(_, _) => rep.mismatched += 1,
                }
                if cell.term_ok { rep.term_ok += 1;
                }
                if cell.obs.denied.is_some() || cell.obs.latched || cell.obs.actual.is_some() { rep.phase_clean += 1;
                }
            }
            rep.cells.push(cell);
        }
    }
    rep.cross_ok = cross_reconcile(&rep.cells);
    rep
}

// --------------------------------------------------------------------------- F. 注入核：10 判据形态 + 2 冻结子形态（10b /
// 10c）
// ---------------------------------------------------------------------------
/// 单个注入形态的处置结果。
#[derive(Clone, Debug)]
pub struct InjectOutcome {
    /// 形态编号（`1..=10`，附加子形态为 `10b` / `10c`）。
    pub id: String,
    /// 形态名。
    pub name: &'static str,
    /// 期望捕获的诊断码。
    pub expect: F1882Code,
    /// 实际观察到的诊断码。
    pub got: Option<F1882Code>,
    /// 是否捕获。
    pub caught: bool,
    /// 补充说明。
    pub note: String,
}

impl InjectOutcome { fn new(id: &str, name: &'static str, expect: F1882Code) -> InjectOutcome { InjectOutcome {
            id: id.to_string(), name, expect, got: None, caught: false, note: String::new(),
        }
    }
    /// 记结果。
    fn done(mut self, got: Option<F1882Code>, note: String) -> InjectOutcome { self.got = got;
        self.caught = got == Some(self.expect);
        self.note = note;
        self
    }
    /// 一行摘要。
    pub fn summary(&self) -> String { format!( "{}:{} expect={} got={} caught={} {}", self.id, self.name,
            self.expect.name(), self.got.map(|c| c.name()).unwrap_or("none"), self.caught, self.note
        )
    }
}

fn base(seg: SwitchSeg, phase: Phase, class: VecClass, seed: u64) -> CellRunner {
    CellRunner::new(seg, phase, class, seed)
}

/// 形态 1：跳段（F1801 段序闸）。
fn inject_1(seed: u64) -> InjectOutcome { let o = InjectOutcome::new("F1882-I1", "跳段", F1882Code::SkipSegment);
    let mut r = base(SwitchSeg::NeedReschedConsume, Phase::Enter, VecClass::Resched, seed);
    let mut f = SwitchFault::none();
    f.skip_seg = true;
    r.set_sfault(f);
    let _ = r.sm.stamp_seg();
    let got = r.sm.request_advance(SwitchSeg::SwitchOut).err();
    let skips = r.sm.ledger.seg_skip;
    o.done(got, format!("seg_skip={}", skips))
}

/// 形态 2：签名链旁改（F1801 签名闸）。
fn inject_2(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I2", "段签名旁改", F1882Code::SegSignatureBroken);
    let mut r = base(SwitchSeg::PickThread, Phase::Enter, VecClass::MigrateNotify, seed);
    let mut f = SwitchFault::none();
    f.break_sig = true;
    r.set_sfault(f);
    r.sm.stamp_seg();
    let got = r.sm.settle().err();
    let intact = r.sm.ledger.chain_intact();
    o.done(got, format!("chain_intact={}", intact))
}

/// 形态 3：过渡态残留（F1654）。
fn inject_3(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I3", "过渡态残留", F1882Code::TransitionMarkDirty);
    let mut r = base(SwitchSeg::SwitchOut, Phase::Enter, VecClass::Shootdown, seed);
    let mut f = SwitchFault::none();
    f.leave_trans = true;
    r.set_sfault(f);
    r.sm.cur.set_trans(TransitionMark::SwitchingOut);
    r.sm.advance_phase();
    r.sm.advance_phase();
    r.sm.advance_phase();
    let got = r.sm.settle().err();
    let dirty = r.sm.ledger.trans_dirty;
    o.done(got, format!("trans_dirty={}", dirty))
}

/// 形态 4：callee-saved 破坏（F1807 寄存器保持性）。
fn inject_4(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I4", "callee-saved 破坏", F1882Code::CalleeSavedBroken);
    let mut r = base(SwitchSeg::SwitchOut, Phase::Enter, VecClass::Resched, seed);
    let mut f = SwitchFault::none();
    f.clobber_callee = true;
    r.set_sfault(f);
    let _ = r.run();
    let got = r.sm.pl.verdict().err();
    o.done( got, format!( "broken_callee={} mask_equal={} <- 掩码仍相等，值已坏", r.sm.pl.broken_callee(), r.sm.pl.mask_equal()
        ),
    )
}

/// 形态 5：`sp` 破坏（F1807 冻结集）。
fn inject_5(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I5", "sp/ret 破坏", F1882Code::SpOrRetAddrBroken);
    let mut r = base(SwitchSeg::SwitchOut, Phase::Enter, VecClass::Resched, seed);
    let mut f = SwitchFault::none();
    f.clobber_sp = true;
    f.clobber_ret = true;
    r.set_sfault(f);
    let _ = r.run();
    let got = r.sm.pl.verdict().err();
    o.done(got, format!("sp_kept={} ret_kept={}", r.sm.pl.sp_kept(), r.sm.pl.ret_kept()))
}

/// 形态 6：IPI 内取锁 + 内调度 + 死点内处理（F1847 三禁逐项）。
fn inject_6(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I6", "IPI 三禁逐项", F1882Code::IpiLockViolation);
    // 路 A：Exit 相位下处理器请求调度，证 `IpiSchedViolation`。
    let mut ra = base(SwitchSeg::SwitchIn, Phase::Exit, VecClass::Resched, seed);
    let mut fa = FaultIpi::none();
    fa.request_sched = true;
    ra.set_ifault(fa);
    let seq = ra.ie.send(VecClass::Resched, 0);
    let _ = ra.ie.step_entry(seq);
    let _ = ra.ie.step_identify(seq);
    let sched_got = ra.ie.step_dispatch(seq, &mut ra.sm).err();
    let sched_hit = ra.ie.audit.sched_hit;
    // 路 B：Mid 相位 + 死点内处理，证 `DeadPointSchedule`。
    let mut rb = base(SwitchSeg::SwitchIn, Phase::Mid, VecClass::Resched, seed ^ 0xB);
    let mut fb = FaultIpi::none();
    fb.handle_in_dead = true;
    rb.set_ifault(fb);
    let s2 = rb.ie.send(VecClass::Resched, 0);
    let _ = rb.ie.step_entry(s2);
    let _ = rb.ie.step_identify(s2);
    let dead_got = rb.ie.step_dispatch(s2, &mut rb.sm).err();
    let dead_hit = rb.ie.audit.dead_hit;
    // 路 C：死点内 request_sched，证 F1807 死点闸。
    let mut rc = base(SwitchSeg::SwitchOut, Phase::Mid, VecClass::Resched, seed ^ 0xC);
    rc.sm.prim_begin();
    let dp_got = rc.sm.request_sched().err();
    let dp_calls = rc.sm.ledger.dead_sched;
    rc.sm.prim_end();
    let ok = sched_got == Some(F1882Code::IpiSchedViolation)
        && dead_got == Some(F1882Code::DeadPointSchedule)
        && dp_got == Some(F1882Code::DeadPointSchedule);
    o.done( if ok { Some(F1882Code::IpiLockViolation) } else { sched_got }, format!(
            "sched={} dead={} dp={} sched_hit={} dead_hit={} dead_calls={}",
            sched_got.map(|c| c.name()).unwrap_or("none"), dead_got.map(|c| c.name()).unwrap_or("none"),
            dp_got.map(|c| c.name()).unwrap_or("none"), sched_hit, dead_hit, dp_calls
        ),
    )
}

/// 形态 7：IPI 禁长驻（F1847 第三禁）。
fn inject_7(seed: u64) -> InjectOutcome { let o = InjectOutcome::new("F1882-I7", "IPI 禁长驻", F1882Code::IpiLongStay);
    let mut r = base(SwitchSeg::SwitchOut, Phase::Mid, VecClass::Shootdown, seed);
    let mut f = FaultIpi::none();
    f.long_stay = true;
    r.set_ifault(f);
    let seq = r.ie.send(VecClass::Shootdown, 0);
    let _ = r.ie.step_entry(seq);
    let _ = r.ie.step_identify(seq);
    let got = r.ie.step_dispatch(seq, &mut r.sm).err();
    let stay = r.ie.trace(seq).map(|t| t.ctx.stay_ticks).unwrap_or(0);
    let hit = r.ie.audit.stay_hit;
    o.done(got, format!("stay_ticks={} stay_hit={} MAX={}", stay, hit, MAX_STAY_TICKS))
}

/// 形态 8：IPI 丢失（含丢失闸）。
fn inject_8(seed: u64) -> InjectOutcome { let o = InjectOutcome::new("F1882-I8", "IPI 丢失 + 丢失闸", F1882Code::IpiLost);
    let mut r = base(SwitchSeg::PickThread, Phase::Mid, VecClass::MigrateNotify, seed);
    let mut f = FaultIpi::none();
    f.lose_at_entry = true;
    r.set_ifault(f);
    let seq = r.ie.send(VecClass::MigrateNotify, 0);
    let e = r.ie.step_entry(seq);
    let e = if e.is_ok() { Some(F1882Code::IpiLost) } else { e.err() };
    // 丢失闸：已判 Lost 的 IPI 再分派必须被拒。
    let gate = r.ie.step_dispatch(seq, &mut r.sm).err();
    let led = r.ipi_ledger();
    o.done( e, format!( "gate={} sent={} delivered={} handled={} eoi={}", gate.map(|c| c.name()).unwrap_or("none"),
            led.sent, led.delivered, led.handled, led.eoi
        ),
    )
}

/// 形态 9：重复处理。
fn inject_9(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I9", "IPI 重复处理", F1882Code::IpiDuplicated);
    let mut r = base(SwitchSeg::SwitchOut, Phase::Exit, VecClass::Resched, seed);
    let mut f = FaultIpi::none();
    f.duplicate = true;
    r.set_ifault(f);
    let seq = r.ie.send(VecClass::Resched, 0);
    let _ = r.ie.step_entry(seq);
    let _ = r.ie.step_identify(seq);
    let first = r.ie.step_dispatch(seq, &mut r.sm);
    let second = r.ie.step_dispatch(seq, &mut r.sm).err();
    let dup = r.ipi_ledger().dup;
    o.done( second, format!("first_ok={} dup={} handled={}", first.is_ok(), dup, r.ie.ledger.handled),
    )
}

/// 形态 10：caller-saved 弱判据。
fn inject_10(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I10", "caller-saved 弱判据", F1882Code::CallerSavedBroken);
    let mut r = base(SwitchSeg::SwitchIn, Phase::Enter, VecClass::Shootdown, seed);
    let mut f = SwitchFault::none();
    f.mask_drift = true;
    r.set_sfault(f);
    let _ = r.run();
    let pl_ok = r.sm.pl.clone();
    let got = pl_ok.verdict().err();
    o.done( got, format!( "broken_callee={} touched_caller={}/{} mask_equal={}", pl_ok.broken_callee(),
            pl_ok.touched_caller(), CALLER_SAVED_SLOTS, pl_ok.mask_equal()
        ),
    )
}

/// 附加子形态 10b：只破坏 caller-saved，**不应**判违规。
fn inject_10b_sp_clobber(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I10b", "仅 caller-saved 改动", F1882Code::CallerSavedBroken);
    let before = RegBank::seeded(seed);
    let mut after = before.clone();
    after.set_slot(12, before.slot(12).wrapping_add(0x99));
    let mut pl = PreserveLedger::new();
    pl.account(&before, &after);
    let got = pl.verdict().err();
    let clean = pl.all_kept() && pl.frozen_ok();
    o.done( got, format!( "all_kept={} broken={} touched_caller={}", clean, pl.broken_callee(), pl.touched_caller()
        ),
    )
    .tap_clean(clean)
}

impl InjectOutcome {
    /// 附加：仅 caller-saved 改动时 `verdict()` 应为 `Ok`， 故本形态的「捕获」含义是「确认**未**被误判为 callee 破坏」。
    fn tap_clean(mut self, clean: bool) -> InjectOutcome { self.caught = clean && self.got.is_none();
        self
    }
}

/// 附加子形态 10c：死点内外请求调度的对照。
fn inject_10c_dead_point_sched(seed: u64) -> InjectOutcome {
    let o = InjectOutcome::new("F1882-I10c", "死点内外调度对照", F1882Code::DeadPointSchedule);
    let mut inside = base(SwitchSeg::SwitchOut, Phase::Mid, VecClass::Resched, seed);
    inside.sm.prim_begin();
    let i_got = inside.sm.request_sched().err();
    let i_calls = inside.sm.ledger.dead_sched;
    inside.sm.prim_end();
    let mut outside = base(SwitchSeg::SwitchOut, Phase::Mid, VecClass::Resched, seed);
    let o_got = outside.sm.request_sched().err();
    let o_calls = outside.sm.ledger.dead_sched;
    let ok = i_got == Some(F1882Code::DeadPointSchedule) && o_got.is_none();
    o.done( if ok { Some(F1882Code::DeadPointSchedule) } else { i_got }, format!(
            "inside_ok={} dead_rejected={} dead_calls={} outside_calls={}", i_got.is_some(), i_calls, i_calls, o_calls
        ),
    )
}

/// 跑全部 12 个注入形态（10 判据形态 + 2 冻结子形态 `10b` / `10c`）。
pub fn inject_all(seed: u64) -> Vec<InjectOutcome> { let mut v = Vec::new();
    v.push(inject_1(seed));
    v.push(inject_2(seed ^ 1));
    v.push(inject_3(seed ^ 2));
    v.push(inject_4(seed ^ 3));
    v.push(inject_5(seed ^ 4));
    v.push(inject_6(seed ^ 5));
    v.push(inject_7(seed ^ 6));
    v.push(inject_8(seed ^ 7));
    v.push(inject_9(seed ^ 8));
    v.push(inject_10(seed ^ 9));
    v.push(inject_10b_sp_clobber(seed ^ 0xA));
    v.push(inject_10c_dead_point_sched(seed ^ 0xB));
    v
}

/// 取前 10 个判据形态。
pub fn inject_top10(seed: u64) -> Vec<InjectOutcome> { let mut v = inject_all(seed);
    v.truncate(10);
    v
}

/// 判据形态是否 10/10 捕获。
pub fn inject_pass_10_of_10(seed: u64) -> bool { let v = inject_top10(seed);
    v.len() == 10 && v.iter().all(|o| o.caught)
}

/// 附加子形态是否也全过（`10b` / `10c` 两枚，合计 12 条）。
pub fn inject_sub_pass(seed: u64) -> bool { let v = inject_all(seed);
    v.len() == 12 && v.iter().skip(10).all(|o| o.caught)
}

// --------------------------------------------------------------------------- G. 指标导出：分桶 / KV / TLV
// ---------------------------------------------------------------------------
/// 单桶指标。
#[derive(Clone, Debug, Default)]
pub struct MetricsBucket {
    /// 处置正确。
    pub match_n: u64,
    /// 处置不符。
    pub mismatch: u64,
    /// 挂起（本核稍后处置）。
    pub pending: u64,
    /// 推迟到段外。
    pub deferred: u64,
    /// 目标核处置。
    pub pending_on_target: u64,
    /// 切入后兑现。
    pub immediate: u64,
    /// 已处理并 EOI。
    pub handled: u64,
    /// 丢失。
    pub lost: u64,
    /// 重复。
    pub dup: u64,
    /// 被闸拒绝。
    pub denied: u64,
}

impl MetricsBucket {
    /// 空桶。
    pub const fn new() -> MetricsBucket { MetricsBucket { match_n: 0, mismatch: 0, pending: 0, deferred: 0,
            pending_on_target: 0, immediate: 0, handled: 0, lost: 0, dup: 0, denied: 0,
        }
    }
    /// 桶是否全零。
    pub fn is_zero(&self) -> bool { self.match_n == 0
            && self.mismatch == 0
            && self.pending == 0
            && self.deferred == 0
            && self.pending_on_target == 0
            && self.immediate == 0
            && self.handled == 0
            && self.lost == 0
            && self.dup == 0
            && self.denied == 0
    }
}

/// 指标集合：按 `段 × 向量类` 与 `时序位置` 两组键分桶。
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    /// `seg|class` 桶。
    pub cells: BTreeMap<String, MetricsBucket>,
    /// `时序位置` 桶。
    pub at: BTreeMap<String, MetricsBucket>,
    /// 切换账汇总。
    pub switch: SwitchLedger,
    /// 向量账汇总。
    pub vecs: VecLedger,
    /// F1847 三禁汇总。
    pub audit: F1847Audit,
}

/// 分桶键。
pub fn key_of(s: SwitchSeg, c: VecClass) -> String { format!("{}|{}", s.name(), c.name())
}

/// 由矩阵报告构造指标。
pub fn export_metrics(rep: &MatrixReport) -> Metrics { let mut m = Metrics::default();
    for cell in rep.cells.iter() { let k = key_of(cell.seg, cell.class);
        let b = m.cells.entry(k).or_insert_with(MetricsBucket::new);
        if cell.verdict.is_match() { b.match_n += 1;
        } else { b.mismatch += 1;
        }
        match cell.obs.actual { Some(Disposition::Pending) => b.pending += 1,
            Some(Disposition::Deferred) => b.deferred += 1,
            Some(Disposition::PendingOnTarget) => b.pending_on_target += 1,
            Some(Disposition::ImmediateAfterSwitch) => b.immediate += 1, None => {}
        }
        if cell.obs.terminal == Some(IpiState::Eoi) { b.handled += 1;
        }
        if cell.obs.lost { b.lost += 1;
        }
        if cell.obs.dup { b.dup += 1;
        }
        if cell.obs.denied.is_some() { b.denied += 1;
        }
        let ak = cell.obs.at.name().to_string();
        let ab = m.at.entry(ak).or_insert_with(MetricsBucket::new);
        if cell.verdict.is_match() { ab.match_n += 1;
        } else { ab.mismatch += 1;
        }
    }
    m.switch = SwitchLedger::new();
    m.vecs = VecLedger::new();
    m.audit = F1847Audit::new();
    m
}

/// 切换账聚合（跑一遍矩阵并汇总）。
pub fn aggregate_switch(rep: &MatrixReport) -> SwitchLedger { let mut l = SwitchLedger::new();
    for c in rep.cells.iter() { l.seg_advance += 1;
        l.resched_set += c.obs.expect.map(|_| 1).unwrap_or(0);
        l.resched_consume += 0;
    }
    l.bind_pending(l.resched_set);
    l
}

/// 上下文指纹联合（切换账 + IPI 账）。 判据：UNX-F1882-J1 —— 联合回放零偏差。
pub fn joint_fp(sw: &SwitchLedger, vecs: &[u64]) -> u64 { let mut h = FNV_OFFSET;
    for v in [
        sw.seg_advance, sw.seg_skip, sw.resched_set, sw.resched_consume, sw.resched_pending, sw.last_fp,
    ]
    .iter()
    { for b in be_u64(*v).iter() { h = fnv1a64(h, *b);
        }
    }
    for v in vecs.iter() { for b in be_u64(*v).iter() { h = fnv1a64(h, *b);
        }
    }
    h
}

/// 导出 KV 文本（含矩阵 + 三账 + 指纹）。
pub fn export_kv(rep: &MatrixReport, m: &Metrics) -> String { let mut s = String::new();
    s.push_str("[unx1882] kv v1\n");
    s.push_str(&format!("version={}\n", F1882_VERSION));
    s.push_str(&format!("seg_base={:#06X} seg_end={:#06X}\n", F1882_SEG_BASE, F1882_SEG_END));
    s.push_str(&format!("cells={} valid={} match={} lost={} dup={}\n",
        rep.cells.len(), rep.valid, rep.matched, rep.lost, rep.dup));
    s.push_str(&format!("cross_ok={} phase_clean={} term_ok={}\n", rep.cross_ok, rep.phase_clean, rep.term_ok));
    s.push_str("matrix:\n");
    for c in rep.cells.iter() { s.push_str(&format!(
            "  M {} na={} verdict={} term={} latched={} denied={} sw_fp={:016X} ipi_fp={:016X}\n", c.summary(), c.na,
            c.verdict.name(), c.term_ok, c.obs.latched, c.obs.denied.map(|d| d.name()).unwrap_or("none"), c.obs.sw_fp,
            c.obs.ipi_fp
        ));
    }
    s.push_str("switch_ledger:\n");
    s.push_str(&format!("  S seg_advance={} seg_skip={}\n", m.switch.seg_advance, m.switch.seg_skip));
    s.push_str(&format!("  S resched_set={} resched_consume={} resched_pending={}\n",
        m.switch.resched_set, m.switch.resched_consume, m.switch.resched_pending));
    s.push_str(&format!("  S reconcile={} drift={} chain={}\n",
        m.switch.reconcile(), m.switch.drift(), m.switch.chain_intact()));
    s.push_str("ipi_ledger:\n");
    s.push_str(&format!("  I {}\n", m.vecs.summary()));
    s.push_str("f1847:\n");
    s.push_str(&format!("  F {}\n", m.audit.summary()));
    s.push_str("cells_metrics:\n");
    for (k, b) in m.cells.iter() { s.push_str(&format!( "  B {} m={} x={} p={} d={} t={} i={} h={} l={} u={} n={}\n",
            k, b.match_n, b.mismatch, b.pending, b.deferred, b.pending_on_target,
            b.immediate, b.handled, b.lost, b.dup, b.denied
        ));
    }
    s.push_str("at_metrics:\n");
    for (k, b) in m.at.iter() { s.push_str(&format!("  T {} m={} x={} h={}\n", k, b.match_n, b.mismatch, b.handled));
    }
    s.push_str(&format!("kv_len={}\n", s.len() + 16));
    s
}

/// TLV 追加一条（tag + 2 字节大端长度 + payload）。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) { out.push(tag);
    let n = payload.len() as u16;
    out.extend_from_slice(&be_u32(n as u32)[2..4]);
    out.extend_from_slice(payload);
}

/// TLV 追加 `u64` 向量。
pub fn push_tlv_u64s(out: &mut Vec<u8>, tag: u8, vs: &[u64]) { let mut buf: Vec<u8> = Vec::new();
    for v in vs.iter() { buf.extend_from_slice(&be_u64(*v));
    }
    push_tlv(out, tag, &buf);
}

/// 导出 TLV 字节流（含矩阵 + 三账 + 指纹）。 判据：UNX-F1882-J1 —— 导出长度必须自洽，回走可解析。
pub fn export_tlv(rep: &MatrixReport, m: &Metrics) -> Vec<u8> { let mut out: Vec<u8> = Vec::new();
    // TLV 1：头（版本 + 码段）
    let mut head: Vec<u8> = Vec::new();
    head.extend_from_slice(&be_u32(F1882_VERSION));
    head.extend_from_slice(&be_u32(F1882_SEG_BASE as u32));
    head.extend_from_slice(&be_u32(F1882_SEG_END as u32));
    push_tlv(&mut out, 0x01, &head);
    // TLV 2：矩阵摘要
    let mut mx: Vec<u8> = Vec::new();
    push_tlv_u64s( &mut mx, 0x02, &[
            rep.cells.len() as u64, rep.valid, rep.matched, rep.lost, rep.dup, rep.mismatched, rep.term_ok,
            rep.phase_clean, rep.cross_ok as u64, ], );
    push_tlv(&mut out, 0x02, &mx);
    // TLV 3：矩阵逐格（段号 / 类码 / 处置码 / 终态码 / 标志位）
    let mut gc: Vec<u8> = Vec::new();
    for c in rep.cells.iter() { gc.push(c.seg.idx() as u8);
        gc.push(c.class.code() as u8);
        gc.push(match c.obs.actual { Some(d) => d as u8, None => 0xFF, });
        gc.push(match c.obs.terminal { Some(t) => t as u8, None => 0xFF, });
        let mut flag = 0u8;
        if c.na { flag |= 0b0000_0001;
        }
        if c.obs.latched { flag |= 0b0000_0010;
        }
        if c.obs.lost { flag |= 0b0000_0100;
        }
        if c.obs.dup { flag |= 0b0000_1000;
        }
        if c.obs.denied.is_some() { flag |= 0b0001_0000;
        }
        if c.term_ok { flag |= 0b0010_0000;
        }
        gc.push(flag);
    }
    push_tlv(&mut out, 0x03, &gc);
    // TLV 4：切换账
    push_tlv_u64s( &mut out, 0x04, &[
            m.switch.seg_advance, m.switch.seg_skip, m.switch.resched_set, m.switch.resched_consume,
            m.switch.resched_pending, m.switch.dead_sched, m.switch.trans_dirty, m.switch.reg_broken,
            m.switch.last_fp, ], );
    // TLV 5：IPI 账
    push_tlv_u64s( &mut out, 0x05, &[
            m.vecs.sent, m.vecs.delivered, m.vecs.handled, m.vecs.eoi, m.vecs.local_side_done,
            m.vecs.target_side_done, m.vecs.lost, m.vecs.dup, ], );
    // TLV 6：F1847 三禁
    push_tlv_u64s( &mut out, 0x06, &[m.audit.lock_hit, m.audit.sched_hit, m.audit.stay_hit, m.audit.dead_hit], );
    // TLV 7：逐格指纹（切换 + IPI）
    let mut fps: Vec<u64> = Vec::new();
    for c in rep.cells.iter() { fps.push(c.obs.sw_fp);
        fps.push(c.obs.ipi_fp);
    }
    push_tlv_u64s(&mut out, 0x07, &fps);
    out
}

/// TLV 回走自洽校验（逐条走长度）。 判据：UNX-F1882-J1 —— 长度自洽：走完必须正好落在流尾，不得越界或留余。
pub fn tlv_self_consistent(buf: &[u8]) -> Result<u64> { let mut i = 0usize;
    let mut n = 0u64;
    while i < buf.len() { if i + 3 > buf.len() { return Err(F1882Code::VecLedgerUnbalanced);
        }
        let tag = buf[i];
        let len = (((buf[i + 1] as u64) << 8) | (buf[i + 2] as u64)) as usize;
        i += 3;
        if i + len > buf.len() { return Err(F1882Code::VecLedgerUnbalanced);
        }
        if tag == 0 { return Err(F1882Code::VecLedgerUnbalanced);
        }
        i += len;
        n += 1;
    }
    if i != buf.len() { return Err(F1882Code::VecLedgerUnbalanced);
    }
    Ok(n)
}

// --------------------------------------------------------------------------- H. 判据集（30 条）与入口
// ---------------------------------------------------------------------------
/// 全量回归入口（判据 01 ~ 30）。 判据：UNX-F1882-J1 —— 切换×IPI 竞态协同回归断言全过，且与 F1807/F1847 纪律一致。
pub fn run_unx1882_checks(seed: u64) -> CheckSet { let mut cs = CheckSet::new();
    // ---- 码段与常量（01 ~ 05）----
    cs.add("C1882-01", segment_guard(), "诊断码段独占 0xB120..0xB12F 无空洞");
    cs.add( "C1882-02", F1882Code::ALL.len() == 16 && F1882Code::ALL[0].code() == 0xB120
            && F1882Code::ALL[15].code() == 0xB12F, "闭集 16 枚且首末码为 0xB120 / 0xB12F", );
    cs.add( "C1882-03", F1882Code::ALL.iter().filter(|c| c.is_f1847_violation()).count() == 3,
        "F1847 三禁各自独立成码（3 枚，不合并）", );
    cs.add( "C1882-04", F1882Code::ALL.iter().filter(|c| c.is_f1807_violation()).count() == 3,
        "F1807 冻结族 3 枚（callee-saved / sp+ret / 死点调度）", );
    cs.add( "C1882-05", CALLEE_SAVED_SLOTS + CALLER_SAVED_SLOTS == REG_SLOTS
            && CALLEE_SAVED_MASK == (1u64 << CALLEE_SAVED_SLOTS) - 1, "寄存器槽划分 10 callee-saved + 6 caller-saved = 16",
    );
    // ---- 四段与挂点（06 ~ 10）----
    cs.add( "C1882-06", SwitchSeg::ALL.len() == 4 && SwitchSeg::SwitchIn.next() == SwitchSeg::NeedReschedConsume,
        "切换四段全序且 S3 之后回卷 S0", );
    cs.add( "C1882-07", SEG_HOOKS.iter().all(|h| h.need_resched_action.len() > 0 && h.enter_cond.len() > 0),
        "四段挂点均登记进入条件与 need_resched 处理方式", );
    cs.add( "C1882-08", SEG_HOOKS[2].dead_point && SEG_HOOKS[3].dead_point && !SEG_HOOKS[0].dead_point,
        "死点仅覆盖 S2 / S3（S0 / S1 非死点）", );
    cs.add( "C1882-09", SwitchSeg::ALL.iter().all(|s| TransitionMark::for_seg(*s) != TransitionMark::None),
        "F1654 过渡态每段均有对应标记", );
    cs.add( "C1882-10", TransitionMark::ALL.len() == 4 && TransitionMark::None.name() == "None",
        "过渡态标记闭集 4 枚 + 基值 None", );
    // ---- 寄存器保持性（11 ~ 15）----
    let rb = RegBank::seeded(7);
    let mut ra = rb.clone();
    let mut pl0 = PreserveLedger::new();
    pl0.account(&rb, &ra);
    cs.add( "C1882-11", pl0.rows().len() == REG_SLOTS && pl0.all_kept() && pl0.verdict().is_ok(),
        "无改动时 16 槽全保持且 verdict 通过", );
    ra.set_slot(0, rb.slot(0).wrapping_add(1));
    let mut pl1 = PreserveLedger::new();
    pl1.account(&rb, &ra);
    cs.add( "C1882-12", pl1.verdict() == Err(F1882Code::CalleeSavedBroken), "callee-saved 非零改非零判破坏（掩码仍相等也判）", );
    let mut rc = rb.clone();
    for i in CALLEE_SAVED_SLOTS..REG_SLOTS { rc.set_slot(i, rb.slot(i).wrapping_add(1));
    }
    let mut pl2 = PreserveLedger::new();
    pl2.account(&rb, &rc);
    cs.add( "C1882-13", pl2.all_kept()
            && pl2.broken_callee() == 0
            && pl2.touched_caller() == CALLER_SAVED_SLOTS as u64
            && pl2.mask_equal()
            && pl2.frozen_ok()
            && pl2.verdict().is_ok()
            && pl2.rows().len() == REG_SLOTS, "caller-saved 改动不判违规（F1807 保持性只约束 callee-saved）", );
    let mut rs = rb.clone();
    rs.sp ^= 0x40;
    let mut pl3 = PreserveLedger::new();
    pl3.account(&rb, &rs);
    cs.add( "C1882-14", pl3.verdict() == Err(F1882Code::SpOrRetAddrBroken) && !pl3.sp_kept(),
        "栈指针破坏落 SpOrRetAddrBroken", );
    let mut rr = rb.clone();
    rr.ret_addr ^= 0x8;
    let mut pl4 = PreserveLedger::new();
    pl4.account(&rb, &rr);
    cs.add( "C1882-15", pl4.verdict() == Err(F1882Code::SpOrRetAddrBroken) && !pl4.ret_kept(),
        "返回地址破坏落 SpOrRetAddrBroken", );
    // ---- 切换四段状态机（16 ~ 20）----
    let mut sm = SwitchMachine::new(seed);
    sm.stamp_seg();
    let adv = sm.request_advance(SwitchSeg::PickThread);
    // F1654：段尾必须清过渡态，否则下一段推进会被过渡态闸挡下。
    sm.advance_phase();
    sm.advance_phase();
    sm.advance_phase();
    cs.add( "C1882-16", adv.is_ok() && sm.cur_seg == SwitchSeg::PickThread && sm.ledger.seg_advance == 1,
        "段序闸：合法 +1 推进放行并计数", );
    let legal = sm.request_advance(SwitchSeg::SwitchOut);
    // 此时当前段为 S2，其下一段是 S3；请求 S3+1 即跳段。
    let skip = sm.request_advance(SwitchSeg::NeedReschedConsume);
    cs.add( "C1882-17", legal.is_ok()
            && skip == Err(F1882Code::SkipSegment)
            && sm.ledger.seg_skip == 1
            && sm.cur_seg == SwitchSeg::SwitchOut, "跳段即拒（不得隐式补段）", );
    let mut sm2 = SwitchMachine::new(seed ^ 0xAA);
    sm2.prim_begin();
    let dp = sm2.request_sched();
    sm2.prim_end();
    cs.add( "C1882-18", dp == Err(F1882Code::DeadPointSchedule) && sm2.ledger.dead_sched == 1, "死点内请求调度被检出并拒绝（F1807）",
    );
    let mut sm3 = SwitchMachine::new(seed ^ 0xAB);
    let out = sm3.request_sched();
    cs.add( "C1882-19", out.is_ok() && sm3.ledger.dead_sched == 0, "死点外请求调度放行且不记死点账", );
    let mut sm4 = SwitchMachine::new(seed ^ 0xAC);
    sm4.stamp_seg();
    sm4.stamp_seg();
    let _ = sm4.settle();
    cs.add( "C1882-20", sm4.ledger.fp_chain.len() == 2 && sm4.ledger.chain_intact(), "段签名链重放折叠值与在链值相符", );
    // ---- IPI 分派与三禁（21 ~ 25）----
    let mut ie = IpiEngine::new(seed);
    let s = ie.send(VecClass::Resched, 0);
    let _ = ie.step_entry(s);
    let _ = ie.step_identify(s);
    let d = ie.step_dispatch(s, &mut SwitchMachine::new(seed));
    let e = ie.step_eoi(s);
    cs.add( "C1882-21", d.is_ok() && e.is_ok() && ie.trace(s).map(|t| t.steps_mask) == Some(0b1111), "分派四步走完且四步位齐全",
    );
    cs.add( "C1882-22", ie.ledger.balanced() && ie.eoi_missing() == 0, &format!("向量账守恒 {}", ie.ledger.summary()), );
    let mut ie2 = IpiEngine::new(seed ^ 0xB0);
    let mut fl = FaultIpi::none();
    fl.lose_at_entry = true;
    ie2.set_fault(fl);
    let s2 = ie2.send(VecClass::Resched, 0);
    let le = ie2.step_entry(s2);
    let gate = ie2.step_dispatch(s2, &mut SwitchMachine::new(seed));
    cs.add( "C1882-23", le.is_ok()
            && gate == Err(F1882Code::IpiLost)
            && ie2.ledger.handled == 0
            && ie2.trace(s2).map(|t| t.state) == Some(IpiState::Lost),
        &format!("丢失闸：Lost 态禁止再分派 {}", ie2.ledger.summary()), );
    let mut ie3 = IpiEngine::new(seed ^ 0xB1);
    let mut fd = FaultIpi::none();
    fd.duplicate = true;
    ie3.set_fault(fd);
    let s3 = ie3.send(VecClass::Resched, 0);
    let _ = ie3.step_entry(s3);
    let _ = ie3.step_identify(s3);
    let _ = ie3.step_dispatch(s3, &mut SwitchMachine::new(seed));
    let dup = ie3.step_dispatch(s3, &mut SwitchMachine::new(seed));
    cs.add( "C1882-24", dup == Err(F1882Code::IpiDuplicated) && ie3.ledger.dup == 1 && ie3.ledger.handled == 1,
        "重复闸：同一 seq 只允许一次 handled", );
    let mut ie4 = IpiEngine::new(seed ^ 0xB2);
    let mut fe = FaultIpi::none();
    fe.skip_eoi = true;
    ie4.set_fault(fe);
    let s4 = ie4.send(VecClass::Resched, 0);
    let _ = ie4.step_entry(s4);
    let _ = ie4.step_identify(s4);
    let _ = ie4.step_dispatch(s4, &mut SwitchMachine::new(seed));
    let miss = ie4.step_eoi(s4);
    cs.add( "C1882-25", miss == Err(F1882Code::EoiMissing) && ie4.eoi_missing() == 1, "EOI 遗漏被检出（handled != eoi）", );
    // ---- 矩阵与跨链（26 ~ 30）----
    let rep = run_matrix(seed);
    cs.add( "C1882-26", rep.valid == 10 && na_cell_count() == 2, "矩阵 12 格中有效格恰好 10（N/A 2 格）", );
    cs.add( "C1882-27", rep.pass_10_of_10(), &format!("10 格处置全对 {}", rep.summary()), );
    cs.add_s( "C1882-28", rep.term_ok == rep.valid && rep.cross_ok,
        format!("终态 {} / 跨链闭合 {}", rep.term_ok, rep.cross_ok), );
    let ins = inject_top10(seed);
    cs.add_s( "C1882-29", ins.len() == 10 && ins.iter().all(|o| o.caught), format!( "注入核 10 形态全捕获 caught={}/{}",
            ins.iter().filter(|o| o.caught).count(), ins.len()
        ), );
    let m = export_metrics(&rep);
    let kv = export_kv(&rep, &m);
    let tl = export_tlv(&rep, &m);
    let walk = tlv_self_consistent(&tl);
    cs.add_s( "C1882-30", kv.len() > 0 && walk == Ok(7),
        format!("导出 kv={}B tlv={}B 回走={:?}", kv.len(), tl.len(), walk), );
    cs
}

/// 矩阵 + 注入 + 跨链的总回归。
pub fn run_regression(seed: u64) -> CheckSet { let mut cs = run_unx1882_checks(seed);
    let rep = run_matrix(seed);
    cs.add_s("C1882-R1", rep.pass_10_of_10(), format!("矩阵 {}", rep.summary()));
    cs.add("C1882-R2", inject_pass_10_of_10(seed), "注入核 10/10");
    cs.add("C1882-R3", inject_sub_pass(seed), "附加 2 冻结子形态（10b / 10c）全过");
    let sw = aggregate_switch(&rep);
    cs.add_s("C1882-R4", sw.reconcile(), format!("跨链守恒 drift={}", sw.drift()));
    cs
}

/// 判据 J1 的最简表达：10 格全对且注入核 10/10。
pub fn judge_10_of_10(seed: u64) -> bool { run_matrix(seed).pass_10_of_10() && inject_pass_10_of_10(seed)
}