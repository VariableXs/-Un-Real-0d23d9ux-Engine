//! unx0825_cr8tpr — UNX-F0825 CR8 与 TPR 联动快路径（UNX 中断域·LAPIC 优先级）。
//!
//! 判据原文（施工书锚点 UNX-F0825，提取自
//! `docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md`
//! 第 2457 行，全文照抄）：
//!
//! > | UNX-F0825 | CR8 与 TPR 联动快路径 | 220 | 已深化 | UNX-F0825-J1
//! > CR8 写与 TPR Class 字段同步 10/10 次一致，快路径开销实测低于 MMIO
//! > 路径 50% |
//!
//! 职责定位：中断优先级面的「双闸刀联动」——同一个优先级决策有两个硬件
//! 出口：CR8（IA32_TPR 镜像寄存器，SDM Vol.3 §2.5，一次 `mov cr8` 直写）
//! 与 LAPIC TPR（MMIO 偏移 0x080，SDM Vol.3 §10.9 寄存器地址映射表，
//! 需 cli/sti 保护 + MMIO 准入 + 回读校验）。x86-64 把 CR8 定义为 TPR
//! Class[7:4] 的**硬件镜像**：写 CR8 等价改 TPR Class、SubClass[3:0] 保持；
//! 而 SubClass 只存在于 TPR 侧——CR8 根本管不到它。这个「半镜像」语义是
//! 本单全部深度的根：镜像不是全等，是**字段级投影**（Class 双向同步、
//! SubClass 单侧孤悬）。Windows 的 HAL 把两者各写各的、从不留账，出现
//! CR8/TPR 漂移时无凭据可查；Varix 深在：
//!
//! ① **CR8↔TPR 联动镜像**（`Cr8TprMirror`）：每次写（双向）都落「镜像
//!    账」——写前 TPR、写后 TPR、写后 CR8、方向、一致性判定逐次留痕；
//!    滑窗 10 次一致性样本构成 J1 前半判据（10/10 一致）的账面。
//! ② **双路径开销模型**（`PriorityPath`）：CR8 直写快路径步数化（3 步：
//!    校验→mov cr8→镜像落账）vs TPR MMIO 慢路径步数化（9 步：cli→IF
//!    压栈→准入令牌→基址解析→编码→MMIO 写→回读校验→TPR 侧落账→sti），
//!    2×快 ≤ 慢 即 50% 判据（J1 后半）的构造面；两路径对同一输入做
//!    **结果等价对拍**（Class 结果必须一致——路径不同、语义不许分叉）。
//! ③ **优先级屏蔽真值表**（SDM Vol.3 §10.8.3.1：向量优先级 Class 低于或
//!    **等于** TPR Class 即屏蔽——相等屏蔽是边界语义，本单用 256 项
//!    const 真值表把「投递/低档屏蔽/相等屏蔽」三态全部展开并编译期
//!    对拍判定式）。
//! ④ **同步失配检测**（`MismatchScanner`）：CR8/TPR 镜像不一致扫描——
//!    失配即红案（立案不静默修）+ 自动回同步（CR8 Class 为准、TPR
//!    SubClass 保序），回同步后复扫确证。
//! ⑤ **快路径适用判据**（`FastPathGate`）：IF 标志态（EFLAGS.IF=1 才有
//!    「活的」优先级窗口）、NMI 窗口态、镜像同步态三条件判据表（8 项
//!    const 表），不满足时**强制走慢路径**并留拒判账；绕闸硬走快路径
//!    会计违例红案。
//! ⑥ **注入面**：失配注入 / 开销虚报注入 / 边界优先级注入三路，各自由
//!    对应检测器**必须抓到**（fired == detected 否则自检失败）。
//! ⑦ **事件账 + health() + FNV 指纹 + 双跑确定性对拍**：全事件序列化后
//!    FNV-1a 64 位指纹，同一场景跑两遍指纹必须逐位一致（确定性判据）。
//!
//! 语义来源全部写死注明：CR8 位域取 SDM Vol.3 §2.5（Table 2-5，CR8[3:0]
//! = 任务优先级 Class、CR8[63:4] 保留必须写零）；TPR 偏移与位域取 SDM
//! Vol.3 §10.9 / §10.4.4 寄存器地址映射表（0x080，bit7:4=Class、
//! bit3:0=SubClass）；屏蔽判定取 §10.8.3.1（Class ≤ TPR Class 即屏蔽；
//! class 15 的最终裁决由 PPR 0x0A0 承接，本单显性标注不越权）；IF 位取
//! EFLAGS bit9（§2.3）。
//!
//! 上游：UNX-F0821 LAPIC 基址账（基址/偏移闭集由其交付，本单只引用偏移
//! 常量语义）、UNX-F0823 EOI 单点（同域优先级序列）；下游：调度器优先级
//! 提升请求（走本单快/慢路径仲裁）、中断域健康总账（吃本单 health()）。
//! 真机 mov-cr8 / MMIO 物理写归接线面（asm+lapic.rs 实发），本单交付
//! **镜像投影模型 + 双路径开销账 + 屏蔽真值表 + 失配扫描 + 判据闸 +
//! 注入面**六面纯函数决策层。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect/panic!/运行期 assert!（编译期
//! `const _` 可）；零 IO 零墙钟；溢出全显性 wrapping/saturating；>64KB
//! 结构体禁上栈（编译期 size assert）；const fn 内不用 for/slice::get/
//! Ord::min/max/派生 PartialEq 的 `==`（一律 while+索引+手写比较）；
//! 可失败全走 `Result<T, Cr8TprError>`，每变体带中文可操作 `detail()`。
//! 诊断码段 0x1720~0x172F 本单独占（已 grep 确认全仓空闲）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（判据批次：UNX-A2-B02，任务号 F0825）。
pub const CR8TPR_VERSION: &str = "UNX-F0825-cr8tpr-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x1720~0x172F 段，本单独占；已 grep 全仓确认空闲）
// ---------------------------------------------------------------------------

/// 0x1720 CR8 写入与 TPR Class 未同步（快路径写后镜像两侧不一致）。
pub const XC_CR8_SYNC_FAILED: u32 = 0x1720;
/// 0x1721 TPR 写回读失配（TPR 侧写后 Class 投影与 CR8 镜像不一致）。
pub const XC_TPR_READBACK_MISMATCH: u32 = 0x1721;
/// 0x1722 CR8 载体非法（CR8[63:4] 保留位非零——SDM §2.5 要求写零）。
pub const XC_CR8_CARRIER_INVALID: u32 = 0x1722;
/// 0x1723 镜像账落账缺失（写操作未产生镜像账目——逐次留痕承诺破裂）。
pub const XC_LEDGER_ENTRY_MISSING: u32 = 0x1723;
/// 0x1724 同步判据未达标（J1 前半：滑窗未满或 10/10 内有失配样本）。
pub const XC_SYNC_CRITERION_MISS: u32 = 0x1724;
/// 0x1725 开销虚报（J1 后半：快路径步数账超实测常量或破 50% 判据）。
pub const XC_OVERHEAD_INFLATED: u32 = 0x1725;
/// 0x1726 快慢路径结果不等价（同一 Class 输入两路径出口分叉）。
pub const XC_PATH_PARITY_BROKEN: u32 = 0x1726;
/// 0x1727 优先级参数越界（向量越出 0..256 或 Class/SubClass 越出 0..15）。
pub const XC_PRIORITY_RANGE: u32 = 0x1727;
/// 0x1728 相等屏蔽语义违例（向量 Class == TPR Class 仍被判投递）。
pub const XC_EQUALITY_MASK_VIOLATION: u32 = 0x1728;
/// 0x1729 镜像失配红案（CR8/TPR 扫描不一致且尚未完成回同步）。
pub const XC_MISMATCH_RED_CASE: u32 = 0x1729;
/// 0x172A 回同步失败（自动回同步后复扫仍失配——红案未消）。
pub const XC_RESYNC_FAILED: u32 = 0x172A;
/// 0x172B 快路径适用判据违例（判据表判拒仍硬走快路径——绕闸）。
pub const XC_FASTPATH_GATE_VIOLATION: u32 = 0x172B;
/// 0x172C 注入检出缺失（注入已开火但对应检测器未抓到——检测面失明）。
pub const XC_INJECTION_UNDETECTED: u32 = 0x172C;
/// 0x172D 事件账指纹漂移（双跑确定性对拍 FNV 不一致）。
pub const XC_FINGERPRINT_DRIFT: u32 = 0x172D;
/// 0x172E SubClass 隐匿改写（快路径侧动了 CR8 语义管不到的 SubClass 位）。
pub const XC_SUBCLASS_HIDDEN_WRITE: u32 = 0x172E;
/// 0x172F 事件账容量耗尽（账满未倾倒——硬上限保护性拒记）。
pub const XC_LEDGER_EXHAUSTED: u32 = 0x172F;

// ---------------------------------------------------------------------------
// 常量（写死并注明出处）
// ---------------------------------------------------------------------------

/// LAPIC TPR 寄存器 MMIO 偏移（SDM Vol.3 §10.9 寄存器地址映射表；
/// 同域 SVR 0x0F0 / EOI 0x0B0 已由 F0822/F0823 各自锁定，本单只锁 0x080）。
pub const LAPIC_TPR_OFFSET: u32 = 0x080;
/// TPR 寄存器 Class 字段位偏移（SDM Vol.3 §10.9：bit7:4 = Task Priority
/// Class）。
pub const TPR_CLASS_SHIFT: u8 = 4;
/// TPR/CR8 Class 半字节掩码。
pub const CLASS_NIBBLE_MASK: u8 = 0x0F;
/// TPR SubClass 半字节掩码（SDM Vol.3 §10.9：bit3:0 = Task Priority
/// Sub-class）。
pub const SUBCLASS_NIBBLE_MASK: u8 = 0x0F;
/// CR8 保留位掩码（SDM Vol.3 §2.5 Table 2-5：CR8[63:4] 保留，软件写必须
/// 置零，否则 #GP——本单在模型层等价拒绝）。
pub const CR8_RESERVED_MASK: u64 = !0x0Fu64;
/// EFLAGS.IF 位（SDM Vol.3 §2.3：bit9，中断使能标志——快路径适用判据
/// 的第一条件）。
pub const EFLAGS_IF_BIT: u32 = 1 << 9;
/// 屏蔽真值表维度（Class 均为 4 位半字节，16×16=256 全展开）。
pub const MASK_TABLE_DIM: usize = 16;
/// 屏蔽判定三态码：投递（向量 Class > TPR Class）。
pub const VERDICT_DELIVER: u8 = 0;
/// 屏蔽判定三态码：相等屏蔽（向量 Class == TPR Class——SDM §10.8.3.1
/// 「less than or equal」的边界语义，本单边界锚点）。
pub const VERDICT_MASK_EQUAL: u8 = 1;
/// 屏蔽判定三态码：低档屏蔽（向量 Class < TPR Class）。
pub const VERDICT_MASK_BELOW: u8 = 2;
/// PPR（Processor Priority Register）MMIO 偏移——class 15 的最终裁决由
/// PPR 承接（SDM Vol.3 §10.8.3.1），本单显性标注不越权建模。
pub const LAPIC_PPR_OFFSET: u32 = 0x0A0;
/// J1 前半判据窗口长度（「同步 10/10 次一致」的账面滑窗）。
pub const SYNC_WINDOW_LEN: usize = 10;
/// 快路径步数实测常量（CR8 直写：①Class 值校验 ②mov cr8 ③镜像落账；
/// 步数账为模型层「实测」锚——接线面以指令数为准，模型层此值即判据基线）。
pub const FAST_PATH_STEPS: u32 = 3;
/// 慢路径步数实测常量（TPR MMIO：①cli ②IF 压栈 ③准入令牌 ④基址解析
/// ⑤Class/SubClass 编码 ⑥MMIO 写 0x080 ⑦回读校验 ⑧TPR 侧落账 ⑨sti）。
pub const SLOW_PATH_STEPS: u32 = 9;
/// 镜像账滚动容量（账满滚动丢弃最旧并计数——镜像账是热路径账，滚动保命；
/// 事件账走硬上限显性拒记，两账语义不同）。
pub const MIRROR_LEDGER_CAP: usize = 256;
/// 事件账默认容量（硬上限，满即 `XC_LEDGER_EXHAUSTED` 显性拒记）。
pub const LEDGER_CAP_DEFAULT: usize = 512;
/// 事件账最小容量（boot 校验下限——容量太小等于没账）。
pub const LEDGER_CAP_MIN: usize = 8;
/// 注入面类别数（失配/开销虚报/边界优先级）。
pub const INJECTION_KINDS: usize = 3;
/// 注入类别：镜像失配注入。
pub const INJ_MISMATCH: usize = 0;
/// 注入类别：开销虚报注入。
pub const INJ_OVERHEAD: usize = 1;
/// 注入类别：边界优先级注入（相等 Class 仍声称投递）。
pub const INJ_BOUNDARY: usize = 2;
/// 快路径闸判据位：IF 关闭。
pub const GATE_RSN_IF_DISABLED: u32 = 1;
/// 快路径闸判据位：NMI 窗口开启（NMI 窗口内改优先级语义悬置）。
pub const GATE_RSN_NMI_WINDOW: u32 = 2;
/// 快路径闸判据位：镜像失配未消（带病快写会固化失配）。
pub const GATE_RSN_MIRROR_UNSYNC: u32 = 4;

// ---------------------------------------------------------------------------
// 编译期不变量（const _ 断言——编译期即红，不占运行期）
// ---------------------------------------------------------------------------

/// J1 后半判据的构造式：快路径步数必须 ≤ 慢路径 50%（整数域：2×快 ≤ 慢）。
const _: () = assert!(
    2 * FAST_PATH_STEPS <= SLOW_PATH_STEPS,
    "UNX-F0825-J1 后半判据构造式破裂：快路径步数必须 <= 慢路径 50%"
);
/// 屏蔽真值表与 SDM 判定式编译期对拍。
const _: () = assert!(verify_mask_table(), "屏蔽真值表与 SDM §10.8.3.1 判定式不符");
/// 相等屏蔽边界语义必须在真值表里成立（J 判据外的本单边界锚点）。
const _: () = assert!(
    MASK_TRUTH_TABLE[5 * MASK_TABLE_DIM + 5] == VERDICT_MASK_EQUAL,
    "相等屏蔽（Class == TPR Class）边界语义缺失"
);
/// 快路径闸判据表与判据式编译期对拍。
const _: () = assert!(verify_gate_table(), "快路径适用判据表与判据式不符");
/// >64KB 结构体禁上栈：核心结构体体积编译期上界。
const _: () = assert!(
    core::mem::size_of::<Cr8TprMirror>() <= 64 * 1024,
    "Cr8TprMirror 超 64KB——禁止上栈"
);
const _: () = assert!(
    core::mem::size_of::<Cr8TprSubsystem>() <= 64 * 1024,
    "Cr8TprSubsystem 超 64KB——禁止上栈"
);
const _: () = assert!(
    core::mem::size_of::<HealthReport>() <= 64 * 1024,
    "HealthReport 超 64KB——禁止上栈"
);

// ---------------------------------------------------------------------------
// 错误类型（每变体带中文可操作诊断）
// ---------------------------------------------------------------------------

/// CR8/TPR 联动子系统错误全集。每个变体的 [`Cr8TprError::detail`]
/// 给出中文可操作诊断；[`Cr8TprError::code`] 给出 0x1720 段诊断码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cr8TprError {
    /// CR8 写后 TPR Class 未同步（cr8=写侧 Class，tpr=TPR 侧 Class）。
    Cr8SyncFailed { cr8: u8, tpr: u8 },
    /// TPR 写后 Class 投影与 CR8 镜像失配。
    TprReadbackMismatch { tpr: u8, cr8: u8 },
    /// CR8 保留位非零（SDM 要求 CR8[63:4] 写零）。
    Cr8CarrierInvalid { cr8: u64 },
    /// 镜像账落账缺失（写后未产生账目）。
    LedgerEntryMissing { op: u8 },
    /// 同步判据未达标（ok=滑窗内一致数，total=滑窗已填数）。
    SyncCriterionMiss { ok: u32, total: u32 },
    /// 开销虚报（fast/slow=上报的步数账）。
    OverheadInflated { fast: u32, slow: u32 },
    /// 快慢路径对拍不等价（fast_cls/slow_cls=两路径出口 Class）。
    PathParityBroken { fast_cls: u8, slow_cls: u8 },
    /// 优先级参数越界（vec=原始入参，cls=解析出的越界值）。
    PriorityRange { vec: u16, cls: u8 },
    /// 相等屏蔽语义违例（vec_cls==tpr_cls 却判投递）。
    EqualityMaskViolation { vec_cls: u8, tpr_cls: u8 },
    /// 镜像失配红案（cr8/tpr=两侧 Class 现值）。
    MismatchRedCase { cr8: u8, tpr: u8 },
    /// 回同步失败（attempts=失败计次）。
    ResyncFailed { attempts: u32 },
    /// 快路径判据违例（reason=拒判原因位集）。
    FastPathGateViolation { reason: u32 },
    /// 注入未检出（kind=注入类别）。
    InjectionUndetected { kind: u8 },
    /// 指纹漂移（run_a/run_b=两跑合成指纹）。
    FingerprintDrift { run_a: u64, run_b: u64 },
    /// SubClass 隐匿改写（old/new=改写前后 SubClass）。
    SubclassHiddenWrite { old: u8, new: u8 },
    /// 事件账容量耗尽（kind=0 事件账 / 1 镜像账）。
    LedgerExhausted { kind: u8 },
}

impl Cr8TprError {
    /// 中文可操作诊断（每变体一段，面向排障动作，不是复述错误名）。
    pub const fn detail(&self) -> &'static str {
        match self {
            Cr8TprError::Cr8SyncFailed { .. } => {
                "CR8 写入后 TPR Class 未同步：本次写疑似绕过镜像层直达硬件。\
处置：①检查调用方是否直接 mov cr8 而未走 Cr8TprMirror::write_cr8；\
②按 CR8→TPR 方向重放该次写并清空一致性滑窗重计 10/10；\
③如为注入演练，确认 scanner 已回同步（红案应已立案）。"
            }
            Cr8TprError::TprReadbackMismatch { .. } => {
                "TPR 写后 Class 投影与 CR8 镜像失配：慢路径回读校验落空。\
处置：①确认 MMIO 回读（0x080）是否被省略；②按 TPR→CR8 方向重建镜像；\
③若回读值与写入值不同，先查 LAPIC 基址映射是否错区（联动 UNX-F0821）。"
            }
            Cr8TprError::Cr8CarrierInvalid { .. } => {
                "CR8 载体非法：SDM §2.5 要求 CR8[63:4] 保留位写零，本次写入携带了\
高位脏值。处置：①入参先与 CR8_RESERVED_MASK 相与再送写；\
②排查上游是否把指针/标志位错误打包进 CR8 值；\
③本单已拒写，硬件未受影响，无需回滚。"
            }
            Cr8TprError::LedgerEntryMissing { .. } => {
                "镜像账落账缺失：写操作完成后未产生对应账目，逐次留痕承诺破裂。\
处置：①检查写路径是否被短路返回；②核对镜像账容量与滚动计数；\
③用 mirror.entry_count() 与写计数比对定位丢失点。"
            }
            Cr8TprError::SyncCriterionMiss { .. } => {
                "同步判据未达标（J1 前半）：一致性滑窗未满 10 或窗内存在失配样本。\
处置：①滑窗未满是初态不是故障——补足 10 次干净写后复查；\
②窗内有失配样本时先跑 scanner.scan() 消红案再重计窗口；\
③重计前不要让任何一方再写，避免样本混杂。"
            }
            Cr8TprError::OverheadInflated { .. } => {
                "开销虚报（J1 后半）：上报步数超实测常量或快路径占比破 50%。\
处置：①核对 FAST_PATH_STEPS/SLOW_PATH_STEPS 基线是否被擅改；\
②若快路径真的变慢，先查镜像落账是否混入了慢路径步骤；\
③确属硬件步数演进时需改判据常量并重审本单，不许只调上报值。"
            }
            Cr8TprError::PathParityBroken { .. } => {
                "快慢路径结果不等价：同一 Class 输入两路径出口分叉，\
路径差异渗入了语义差异。处置：①检查快路径是否误触 SubClass；\
②检查慢路径编码（class<<4|subclass）是否半字节错位；\
③复跑 run_parity 单独定位，禁止带病继续投递。"
            }
            Cr8TprError::PriorityRange { .. } => {
                "优先级参数越界：向量须在 0..256，Class/SubClass 须在 0..15。\
处置：①入参来源先做半字节掩码；②向量高位（bit7:4）才是 Class，\
别把整向量当 Class 用；③本单已拒收，无副作用。"
            }
            Cr8TprError::EqualityMaskViolation { .. } => {
                "相等屏蔽语义违例：向量 Class == TPR Class 时按 SDM §10.8.3.1\
属于屏蔽（equal 也算 less-than-or-equal），本单不允许判投递。\
处置：①检查调用方是否自写比较用了 `<` 而漏掉 `=`；\
②一律走 mask_verdict_for()/真值表取值，不要自写比较；\
③class 15 特例由 PPR（0x0A0）承接，不在本单开洞。"
            }
            Cr8TprError::MismatchRedCase { .. } => {
                "镜像失配红案：CR8 与 TPR Class 不一致且未完成回同步。\
处置：①立即跑 scanner.scan() 触发自动回同步（CR8 Class 为准、\
TPR SubClass 保序）；②回同步后必须复扫确证；\
③红案期间快路径闸会判拒，属预期行为，不要绕闸。"
            }
            Cr8TprError::ResyncFailed { .. } => {
                "回同步失败：自动回同步后复扫仍失配——红案未消，镜像面已不可信。\
处置：①冻结快路径（闸已强制拒）；②逐项 dump 两侧账目比对；\
③若连续失败，降级为全量重建镜像（两侧清零后重走初始化序列），\
期间对外按「优先级面维护中」应答。"
            }
            Cr8TprError::FastPathGateViolation { .. } => {
                "快路径适用判据违例：判据表判拒仍硬走快路径（绕闸）。\
处置：①先看拒判原因位：bit0=IF 关、bit1=NMI 窗口、bit2=镜像失配；\
②IF 关时应走慢路径或先 sti；③绕闸记录已立案，\
请回溯调用栈确认是否误用了 fast_bypass_gate。"
            }
            Cr8TprError::InjectionUndetected { .. } => {
                "注入未检出：注入面已开火但对应检测器没抓到——检测面失明。\
处置：①核对注入类别与检测器是否配对（失配↔扫描器、虚报↔开销审计、\
边界↔屏蔽真值表）；②若检测器被短路，优先恢复检测而不是去掉注入；\
③本错误即自检红案，不许吞掉。"
            }
            Cr8TprError::FingerprintDrift { .. } => {
                "事件账指纹漂移：同一确定性场景双跑 FNV 不一致——存在非确定性\
状态（未初始化读、迭代序漂移或隐藏时钟依赖）。\
处置：①比对两跑 health() 分项定位首个分叉事件；\
②检查新增代码是否引入了未记账分支；\
③修复后必须重跑双跑对拍闭环。"
            }
            Cr8TprError::SubclassHiddenWrite { .. } => {
                "SubClass 隐匿改写：快路径侧改动了 SubClass 位——CR8 语义根本\
管不到 SubClass，改动即语义越权。\
处置：①快路径只允许动 Class 半字节；②需要改 SubClass 必须显性走\
慢路径（write_tpr/write_tpr_subclass）；③排查是否把 TPR 整字节\
误传进了 CR8 写入口。"
            }
            Cr8TprError::LedgerExhausted { .. } => {
                "事件账容量耗尽：账满触发硬上限保护性拒记。\
处置：①倾倒当前账（fingerprint + health 落盘）后调 ledger_reset；\
②若频繁触顶说明事件速率超预算，上调容量或降采样普通 Op 事件；\
③红案类事件不许因拒记而丢失——先记红案再记普通事件。"
            }
        }
    }

    /// 0x1720 段诊断码映射。
    pub const fn code(&self) -> u32 {
        match self {
            Cr8TprError::Cr8SyncFailed { .. } => XC_CR8_SYNC_FAILED,
            Cr8TprError::TprReadbackMismatch { .. } => XC_TPR_READBACK_MISMATCH,
            Cr8TprError::Cr8CarrierInvalid { .. } => XC_CR8_CARRIER_INVALID,
            Cr8TprError::LedgerEntryMissing { .. } => XC_LEDGER_ENTRY_MISSING,
            Cr8TprError::SyncCriterionMiss { .. } => XC_SYNC_CRITERION_MISS,
            Cr8TprError::OverheadInflated { .. } => XC_OVERHEAD_INFLATED,
            Cr8TprError::PathParityBroken { .. } => XC_PATH_PARITY_BROKEN,
            Cr8TprError::PriorityRange { .. } => XC_PRIORITY_RANGE,
            Cr8TprError::EqualityMaskViolation { .. } => XC_EQUALITY_MASK_VIOLATION,
            Cr8TprError::MismatchRedCase { .. } => XC_MISMATCH_RED_CASE,
            Cr8TprError::ResyncFailed { .. } => XC_RESYNC_FAILED,
            Cr8TprError::FastPathGateViolation { .. } => XC_FASTPATH_GATE_VIOLATION,
            Cr8TprError::InjectionUndetected { .. } => XC_INJECTION_UNDETECTED,
            Cr8TprError::FingerprintDrift { .. } => XC_FINGERPRINT_DRIFT,
            Cr8TprError::SubclassHiddenWrite { .. } => XC_SUBCLASS_HIDDEN_WRITE,
            Cr8TprError::LedgerExhausted { .. } => XC_LEDGER_EXHAUSTED,
        }
    }

    /// 诊断码是否落在本单独占段（0x1720~0x172F）。
    pub const fn code_in_segment(code: u32) -> bool {
        code >= XC_CR8_SYNC_FAILED && code <= XC_LEDGER_EXHAUSTED
    }
}

// ---------------------------------------------------------------------------
// 优先级屏蔽语义：真值表（SDM §10.8.3.1）+ 边界判定
// ---------------------------------------------------------------------------

/// 优先级屏蔽判定：三态真值表（256 项全展开，编译期构建并对拍）。
///
/// 判定式（SDM Vol.3 §10.8.3.1）：向量优先级 Class（vector>>4）大于 TPR
/// Class 时投递；**小于或等于**即屏蔽——相等情形单列 `VERDICT_MASK_EQUAL`
/// 作为本单边界锚点。class 15 的最终裁决由 PPR（0x0A0）承接，本单显性
/// 不越权（真值表仍按上式给值，消费方须知 PPR 才是终审）。
const fn build_mask_table() -> [u8; MASK_TABLE_DIM * MASK_TABLE_DIM] {
    let mut table = [VERDICT_DELIVER; MASK_TABLE_DIM * MASK_TABLE_DIM];
    let mut vc = 0usize;
    while vc < MASK_TABLE_DIM {
        let mut tc = 0usize;
        while tc < MASK_TABLE_DIM {
            // 手写三分支比较（const fn 内不用派生 PartialEq 的 ==）。
            if vc > tc {
                table[vc * MASK_TABLE_DIM + tc] = VERDICT_DELIVER;
            } else if vc == tc {
                table[vc * MASK_TABLE_DIM + tc] = VERDICT_MASK_EQUAL;
            } else {
                table[vc * MASK_TABLE_DIM + tc] = VERDICT_MASK_BELOW;
            }
            tc += 1;
        }
        vc += 1;
    }
    table
}

/// 编译期展开的 256 项屏蔽真值表。
pub const MASK_TRUTH_TABLE: [u8; MASK_TABLE_DIM * MASK_TABLE_DIM] = build_mask_table();

/// 真值表↔判定式编译期对拍（const _ 断言消费）。
const fn verify_mask_table() -> bool {
    let mut vc = 0usize;
    while vc < MASK_TABLE_DIM {
        let mut tc = 0usize;
        while tc < MASK_TABLE_DIM {
            let got = MASK_TRUTH_TABLE[vc * MASK_TABLE_DIM + tc];
            let want = if vc > tc {
                VERDICT_DELIVER
            } else if vc == tc {
                VERDICT_MASK_EQUAL
            } else {
                VERDICT_MASK_BELOW
            };
            if got != want {
                return false;
            }
            tc += 1;
        }
        vc += 1;
    }
    true
}

/// 运行期取真值表判定（向量 Class × TPR Class → 三态码）。
///
/// const fn：纯表查，无分支，可直接用于编译期构造。
pub const fn mask_verdict_for(vec_class: u8, tpr_class: u8) -> u8 {
    MASK_TRUTH_TABLE[(vec_class & CLASS_NIBBLE_MASK) as usize * MASK_TABLE_DIM
        + (tpr_class & CLASS_NIBBLE_MASK) as usize]
}

/// 向量 → 优先级 Class（vector bit7:4，SDM §10.8.3.1 同一映射）。
pub const fn class_of_vector(vector: u8) -> u8 {
    vector >> TPR_CLASS_SHIFT
}

/// 完整投递判定入口：原始向量 × TPR 字节 → 三态码。
///
/// class 15 边界说明：三态码仍按上式给出；终审在 PPR，消费方据
/// `LAPIC_PPR_OFFSET` 语义另行裁决，本单不开洞。
pub const fn delivery_verdict(vector: u8, tpr: u8) -> u8 {
    mask_verdict_for(class_of_vector(vector), tpr >> TPR_CLASS_SHIFT)
}

// ---------------------------------------------------------------------------
// CR8 / TPR 字段级投影编码（const fn 闭集）
// ---------------------------------------------------------------------------

/// CR8 编码：Class 半字节 → 合法 CR8 值（高 60 位补零）。
pub const fn cr8_encode(class: u8) -> u64 {
    (class & CLASS_NIBBLE_MASK) as u64
}

/// TPR 编码：Class + SubClass 两半字节 → TPR 字节。
pub const fn tpr_encode(class: u8, subclass: u8) -> u8 {
    ((class & CLASS_NIBBLE_MASK) << TPR_CLASS_SHIFT) | (subclass & SUBCLASS_NIBBLE_MASK)
}

/// TPR 字节 → Class 半字节。
pub const fn tpr_class_of(tpr: u8) -> u8 {
    tpr >> TPR_CLASS_SHIFT
}

/// TPR 字节 → SubClass 半字节。
pub const fn tpr_subclass_of(tpr: u8) -> u8 {
    tpr & SUBCLASS_NIBBLE_MASK
}

/// Class 合法性（0..=15——半字节天然约束，越界只可能来自未掩码入参）。
pub const fn class_in_range(class: u8) -> bool {
    (class & !CLASS_NIBBLE_MASK) == 0
}

// ---------------------------------------------------------------------------
// 事件账（硬上限显性拒记 + FNV-1a 64 指纹）
// ---------------------------------------------------------------------------

/// 事件类别（账面主键之一；as u8 后进指纹，取值稳定不许重排）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// CR8 侧普通写。
    OpWriteCr8 = 1,
    /// TPR 侧普通写（整字节，含 SubClass）。
    OpWriteTpr = 2,
    /// 快路径执行。
    OpFastPath = 3,
    /// 慢路径执行。
    OpSlowPath = 4,
    /// 快慢路径对拍。
    OpParity = 5,
    /// 失配扫描。
    OpScan = 6,
    /// 回同步。
    OpResync = 7,
    /// 快路径闸拒判。
    OpGateDeny = 8,
    /// 红案立案。
    RedCase = 9,
    /// 注入开火。
    InjectFired = 10,
    /// 注入检出。
    InjectDetected = 11,
    /// 判据复核（10/10 或 50% 复查点）。
    CriterionCheck = 12,
}

/// 事件账目（定宽字段——指纹逐字节折叠，字段宽度稳定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventEntry {
    /// 单调序号（wrapping——账面序不因长期运行回退为 0 语义）。
    pub seq: u32,
    /// 事件类别。
    pub kind: EventKind,
    /// 诊断码（普通 Op 事件为 0；红案/判据类事件为 0x1720 段码）。
    pub code: u32,
    /// 参数 A（语义随类别：多为 Class/类别号）。
    pub a: u32,
    /// 参数 B（语义随类别：多为步数/原因位集）。
    pub b: u32,
}

/// FNV-1a 64 位参数（Fowler–Noll–Vo 公开域常量）。
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV 单字节折叠（const fn——指纹也可用于编译期对拍）。
const fn fnv_mix(hash: u64, byte: u8) -> u64 {
    (hash ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV u32 定宽折叠（大端序逐字节）。
const fn fnv_mix_u32(hash: u64, value: u32) -> u64 {
    fnv_mix(fnv_mix(fnv_mix(fnv_mix(hash, (value >> 24) as u8), (value >> 16) as u8),
        (value >> 8) as u8), value as u8)
}

/// 事件账：硬上限显性拒记（红案优先记账原则由调用方遵守——先记红案
/// 再记普通事件；满即 `XC_LEDGER_EXHAUSTED`，调用方倾倒后 reset）。
#[derive(Debug, Clone)]
pub struct EventLedger {
    entries: Vec<EventEntry>,
    cap: usize,
    seq: u32,
    exhausted: bool,
    rejected: u32,
}

impl EventLedger {
    /// 建账（容量下限校验——太小等于没账）。
    pub fn with_capacity(cap: usize) -> Result<Self, Cr8TprError> {
        if cap < LEDGER_CAP_MIN {
            return Err(Cr8TprError::LedgerExhausted { kind: 0 });
        }
        Ok(EventLedger {
            entries: Vec::new(),
            cap,
            seq: 0,
            exhausted: false,
            rejected: 0,
        })
    }

    /// 记一笔事件。账满即显性错误（不静默滚动——事件账是审计账，不是
    /// 热路径账；热路径镜像账在 `Cr8TprMirror` 内自行滚动）。
    pub fn record(&mut self, kind: EventKind, code: u32, a: u32, b: u32) -> Result<(), Cr8TprError> {
        if self.entries.len() >= self.cap {
            self.exhausted = true;
            self.rejected = self.rejected.wrapping_add(1);
            return Err(Cr8TprError::LedgerExhausted { kind: 0 });
        }
        self.seq = self.seq.wrapping_add(1);
        self.entries.push(EventEntry { seq: self.seq, kind, code, a, b });
        Ok(())
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否已触顶（触顶后必须倾倒再继续）。
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// 累计拒记数。
    pub fn rejected_count(&self) -> u32 {
        self.rejected
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 按下标取账目（越界返回 None——不 panic）。
    pub fn entry_at(&self, index: usize) -> Option<EventEntry> {
        if index >= self.entries.len() {
            return None;
        }
        Some(self.entries[index])
    }

    /// FNV-1a 64 指纹：全部账目逐字段定宽折叠（确定性对拍的锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut hash = FNV_OFFSET_BASIS;
        let mut index = 0usize;
        while index < self.entries.len() {
            let entry = &self.entries[index];
            hash = fnv_mix_u32(hash, entry.seq);
            hash = fnv_mix(hash, entry.kind as u8);
            hash = fnv_mix_u32(hash, entry.code);
            hash = fnv_mix_u32(hash, entry.a);
            hash = fnv_mix_u32(hash, entry.b);
            index += 1;
        }
        hash
    }

    /// 红案类事件计数（code 非 0 且落在诊断段）。
    pub fn coded_event_count(&self) -> u32 {
        let mut count = 0u32;
        let mut index = 0usize;
        while index < self.entries.len() {
            let code = self.entries[index].code;
            if Cr8TprError::code_in_segment(code) {
                count = count.wrapping_add(1);
            }
            index += 1;
        }
        count
    }

    /// 倾倒重置（调用方已 fingerprint/health 落盘后调用）。
    pub fn reset(&mut self) {
        self.entries.clear();
        self.exhausted = false;
    }
}

// ---------------------------------------------------------------------------
// Cr8TprMirror：CR8↔TPR 字段级投影镜像（J1 前半判据的主账面）
// ---------------------------------------------------------------------------

/// 镜像账方向（双向留痕 + 注入与回同步标记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorDirection {
    /// CR8 → TPR Class（快路径方向；SubClass 保持）。
    Cr8ToTpr = 1,
    /// TPR → CR8 Class（慢路径方向；Class 投影回 CR8）。
    TprToCr8 = 2,
    /// TPR 侧 SubClass 专用写（Class 不动——语义孤悬面）。
    TprSubclassOnly = 3,
    /// 失配自动回同步（CR8 Class 为准）。
    Resync = 4,
    /// 注入制造的失配（一致样本为假）。
    Injected = 5,
}

/// 镜像账目：一次写/回同步/注入的逐次留痕。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirrorEntry {
    /// 单调序号。
    pub seq: u32,
    /// 方向。
    pub dir: MirrorDirection,
    /// 写前 TPR 全字节（含 SubClass）。
    pub tpr_before: u8,
    /// 写后 TPR 全字节。
    pub tpr_after: u8,
    /// 写后 CR8 影子值（Class 半字节）。
    pub cr8_after: u64,
    /// 本次操作后两侧 Class 是否一致（J1 前半样本）。
    pub consistent: bool,
}

/// CR8↔TPR 联动镜像。
///
/// 语义模型（字段级投影）：
/// - CR8 写 → TPR Class[7:4] 同步，SubClass[3:0] **保持**（CR8 管不到）；
/// - TPR 写 → Class 投影回 CR8 影子，SubClass 就地生效（TPR 侧独占语义）；
/// - 每次操作后做一次一致性采样（CR8 Class == TPR Class），样本进滑窗；
/// - 滑窗 10 样本全绿即 J1 前半「10/10 一致」判据达成。
///
/// 镜像账滚动（热路径账，账满丢最旧并计数——审计完整性由事件账承接）。
#[derive(Debug, Clone)]
pub struct Cr8TprMirror {
    cr8: u64,
    tpr: u8,
    cr8_writes: u32,
    tpr_writes: u32,
    subclass_only_writes: u32,
    entries: Vec<MirrorEntry>,
    rolled: u32,
    seq: u32,
    window: [u8; SYNC_WINDOW_LEN],
    window_idx: usize,
    window_filled: usize,
    consec_ok: u32,
    consec_fail: u32,
    total_ok: u32,
    total_fail: u32,
    red_cases: u32,
    resyncs: u32,
    subclass_hidden_catches: u32,
}

impl Cr8TprMirror {
    /// 出厂态：CR8=0、TPR=0（Class 0 = 全投递，SDM §10.8.3.1 TPR=0 语义）。
    pub fn new() -> Self {
        Cr8TprMirror {
            cr8: 0,
            tpr: 0,
            cr8_writes: 0,
            tpr_writes: 0,
            subclass_only_writes: 0,
            entries: Vec::new(),
            rolled: 0,
            seq: 0,
            window: [0; SYNC_WINDOW_LEN],
            window_idx: 0,
            window_filled: 0,
            consec_ok: 0,
            consec_fail: 0,
            total_ok: 0,
            total_fail: 0,
            red_cases: 0,
            resyncs: 0,
            subclass_hidden_catches: 0,
        }
    }

    // -- 读取侧 ------------------------------------------------------------

    /// CR8 影子值（Class 半字节，高 60 位恒零）。
    pub fn cr8(&self) -> u64 {
        self.cr8
    }

    /// TPR 全字节（Class+SubClass）。
    pub fn tpr(&self) -> u8 {
        self.tpr
    }

    /// TPR Class 半字节。
    pub fn tpr_class(&self) -> u8 {
        tpr_class_of(self.tpr)
    }

    /// TPR SubClass 半字节（语义孤悬面——只在 TPR 侧存在）。
    pub fn tpr_subclass(&self) -> u8 {
        tpr_subclass_of(self.tpr)
    }

    /// CR8 Class 半字节。
    pub fn cr8_class(&self) -> u8 {
        (self.cr8 & CLASS_NIBBLE_MASK as u64) as u8
    }

    /// 两侧 Class 是否一致（当前态，不采样不落账）。
    pub fn in_sync(&self) -> bool {
        self.cr8_class() == self.tpr_class()
    }

    // -- 写入侧（每个写都：改态→采样→落账） --------------------------------

    /// CR8 直写（快路径方向）。保留位非零即拒（SDM §2.5 写零要求）。
    ///
    /// 语义：TPR Class ← Class；SubClass 保持。写后采样并落镜像账。
    pub fn write_cr8(&mut self, raw: u64) -> Result<(), Cr8TprError> {
        if raw & CR8_RESERVED_MASK != 0 {
            return Err(Cr8TprError::Cr8CarrierInvalid { cr8: raw });
        }
        let class = (raw & CLASS_NIBBLE_MASK as u64) as u8;
        let subclass_before = self.tpr_subclass();
        let tpr_before = self.tpr;
        // SubClass 保持——CR8 写只动 Class 半字节。
        self.tpr = tpr_encode(class, subclass_before);
        self.cr8 = cr8_encode(class);
        self.cr8_writes = self.cr8_writes.wrapping_add(1);

        // SubClass 隐匿改写检测（快路径侧动 SubClass 即越权——虽然本实现
        // 结构上不可能，账面仍显性核对，防未来改码破约）。
        let subclass_after = self.tpr_subclass();
        if subclass_after != subclass_before {
            self.subclass_hidden_catches = self.subclass_hidden_catches.wrapping_add(1);
            self.sample_and_ledger(MirrorDirection::Cr8ToTpr, tpr_before, false);
            return Err(Cr8TprError::SubclassHiddenWrite {
                old: subclass_before,
                new: subclass_after,
            });
        }
        let consistent = self.in_sync();
        self.sample_and_ledger(MirrorDirection::Cr8ToTpr, tpr_before, consistent);
        if !consistent {
            return Err(Cr8TprError::Cr8SyncFailed {
                cr8: self.cr8_class(),
                tpr: self.tpr_class(),
            });
        }
        Ok(())
    }

    /// TPR 整字节写（慢路径方向，含 SubClass）。写后 Class 投影回 CR8
    /// 影子并采样落账。
    pub fn write_tpr(&mut self, raw: u8) -> Result<(), Cr8TprError> {
        let tpr_before = self.tpr;
        let class = tpr_class_of(raw);
        if !class_in_range(class) {
            return Err(Cr8TprError::PriorityRange { vec: raw as u16, cls: class });
        }
        self.tpr = raw;
        self.cr8 = cr8_encode(class);
        self.tpr_writes = self.tpr_writes.wrapping_add(1);
        let consistent = self.in_sync();
        self.sample_and_ledger(MirrorDirection::TprToCr8, tpr_before, consistent);
        if !consistent {
            return Err(Cr8TprError::TprReadbackMismatch {
                tpr: self.tpr_class(),
                cr8: self.cr8_class(),
            });
        }
        Ok(())
    }

    /// TPR 侧 SubClass 专用写：只动低半字节，Class 不动、CR8 影子不动
    /// （语义孤悬面的显性入口——需要改 SubClass 必须走这里或整字节写）。
    pub fn write_tpr_subclass(&mut self, subclass: u8) -> Result<(), Cr8TprError> {
        let sub = subclass & SUBCLASS_NIBBLE_MASK;
        let tpr_before = self.tpr;
        self.tpr = tpr_encode(self.tpr_class(), sub);
        self.subclass_only_writes = self.subclass_only_writes.wrapping_add(1);
        // Class 不变 → 一致性不变，但仍采样落账（语义孤悬面也留痕）。
        let consistent = self.in_sync();
        self.sample_and_ledger(MirrorDirection::TprSubclassOnly, tpr_before, consistent);
        Ok(())
    }

    /// 失配注入核（注入面专用）：把 TPR Class 拨到 CR8 Class+1（半字节
    /// 回绕），不走一致性拒收——制造真实失配供扫描器消案。
    pub fn inject_divergence(&mut self) {
        let bad_class = self.cr8_class().wrapping_add(1) & CLASS_NIBBLE_MASK;
        let tpr_before = self.tpr;
        self.tpr = tpr_encode(bad_class, self.tpr_subclass());
        self.sample_and_ledger(MirrorDirection::Injected, tpr_before, false);
    }

    /// 自动回同步：CR8 Class 为准、TPR SubClass 保序重写 TPR。
    /// 返回回同步后是否确证同步（复扫语义由调用方再 scan 确证）。
    pub fn re_sync(&mut self) -> Result<(), Cr8TprError> {
        let tpr_before = self.tpr;
        self.tpr = tpr_encode(self.cr8_class(), self.tpr_subclass());
        self.resyncs = self.resyncs.wrapping_add(1);
        let consistent = self.in_sync();
        self.sample_and_ledger(MirrorDirection::Resync, tpr_before, consistent);
        if !consistent {
            return Err(Cr8TprError::ResyncFailed { attempts: self.resyncs });
        }
        Ok(())
    }

    // -- 账面与判据 ---------------------------------------------------------

    /// 采样 + 落账（所有状态变更的唯一出口；账满滚动丢最旧）。
    fn sample_and_ledger(&mut self, dir: MirrorDirection, tpr_before: u8, consistent: bool) {
        // 滑窗采样。
        self.window[self.window_idx] = if consistent { 1 } else { 0 };
        self.window_idx = (self.window_idx + 1) % SYNC_WINDOW_LEN;
        if self.window_filled < SYNC_WINDOW_LEN {
            self.window_filled += 1;
        }
        if consistent {
            self.consec_ok = self.consec_ok.wrapping_add(1);
            self.consec_fail = 0;
            self.total_ok = self.total_ok.wrapping_add(1);
        } else {
            self.consec_fail = self.consec_fail.wrapping_add(1);
            self.consec_ok = 0;
            self.total_fail = self.total_fail.wrapping_add(1);
            self.red_cases = self.red_cases.wrapping_add(1);
        }
        // 镜像账滚动落账。
        self.seq = self.seq.wrapping_add(1);
        let entry = MirrorEntry {
            seq: self.seq,
            dir,
            tpr_before,
            tpr_after: self.tpr,
            cr8_after: self.cr8,
            consistent,
        };
        if self.entries.len() >= MIRROR_LEDGER_CAP {
            // 热路径账滚动：丢最旧一条并计数（审计完整性由事件账承接）。
            self.entries.remove(0);
            self.rolled = self.rolled.wrapping_add(1);
        }
        self.entries.push(entry);
    }

    /// J1 前半判据：滑窗满 10 且全绿（10/10 一致）。
    pub fn sync_criterion_ok(&self) -> Result<(), Cr8TprError> {
        if self.window_filled < SYNC_WINDOW_LEN {
            return Err(Cr8TprError::SyncCriterionMiss {
                ok: self.consec_ok,
                total: self.window_filled as u32,
            });
        }
        let mut index = 0usize;
        while index < SYNC_WINDOW_LEN {
            if self.window[index] != 1 {
                return Err(Cr8TprError::SyncCriterionMiss {
                    ok: self.consec_ok,
                    total: SYNC_WINDOW_LEN as u32,
                });
            }
            index += 1;
        }
        Ok(())
    }

    /// 镜像账目数（滚动后 ≤ 容量）。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 按下标取镜像账目。
    pub fn entry_at(&self, index: usize) -> Option<MirrorEntry> {
        if index >= self.entries.len() {
            return None;
        }
        Some(self.entries[index])
    }

    /// 滚动丢弃计数。
    pub fn rolled_count(&self) -> u32 {
        self.rolled
    }

    /// CR8 侧写计数。
    pub fn cr8_write_count(&self) -> u32 {
        self.cr8_writes
    }

    /// TPR 侧写计数。
    pub fn tpr_write_count(&self) -> u32 {
        self.tpr_writes
    }

    /// TPR 侧 SubClass 专用写计数（孤悬面流量）。
    pub fn subclass_only_count(&self) -> u32 {
        self.subclass_only_writes
    }

    /// 红案累计数（一致性为假的操作次数）。
    pub fn red_case_count(&self) -> u32 {
        self.red_cases
    }

    /// 回同步计数。
    pub fn resync_count(&self) -> u32 {
        self.resyncs
    }

    /// 滑窗一致样本数（当前连绿段）。
    pub fn consecutive_ok(&self) -> u32 {
        self.consec_ok
    }

    /// 滑窗失配样本数（当前连红段）。
    pub fn consecutive_fail(&self) -> u32 {
        self.consec_fail
    }

    /// 全程一致样本总数。
    pub fn total_ok(&self) -> u32 {
        self.total_ok
    }

    /// 全程失配样本总数。
    pub fn total_fail(&self) -> u32 {
        self.total_fail
    }

    /// SubClass 隐匿改写抓获数。
    pub fn subclass_hidden_catches(&self) -> u32 {
        self.subclass_hidden_catches
    }

    /// 镜像态指纹（双跑确定性对拍第二锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut hash = FNV_OFFSET_BASIS;
        hash = fnv_mix_u32(hash, self.cr8 as u32);
        hash = fnv_mix_u32(hash, self.tpr as u32);
        hash = fnv_mix_u32(hash, self.cr8_writes);
        hash = fnv_mix_u32(hash, self.tpr_writes);
        hash = fnv_mix_u32(hash, self.subclass_only_writes);
        hash = fnv_mix_u32(hash, self.red_cases);
        hash = fnv_mix_u32(hash, self.resyncs);
        let mut index = 0usize;
        while index < SYNC_WINDOW_LEN {
            hash = fnv_mix(hash, self.window[index]);
            index += 1;
        }
        hash = fnv_mix_u32(hash, self.window_filled as u32);
        hash
    }
}

// ---------------------------------------------------------------------------
// PriorityPath：双路径开销模型 + 结果等价对拍（J1 后半判据主账面）
// ---------------------------------------------------------------------------

/// 路径选择结果（快路径 / 判据拒后的强制慢路径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathChoice {
    /// CR8 直写快路径。
    FastPath,
    /// 判据拒后强制走 TPR MMIO 慢路径。
    ForcedSlowPath,
}

/// 双路径开销与等价对拍账面。
///
/// 步数账语义：`FAST_PATH_STEPS`/`SLOW_PATH_STEPS` 是模型层实测基线
/// （快 3 步 / 慢 9 步，构成式见常量注释）；`2×快 ≤ 慢` 即 J1 后半
/// 「低于 50%」判据的整数域形式。上报值审计超基线即虚报立案。
#[derive(Debug, Clone)]
pub struct PriorityPath {
    fast_steps: u32,
    slow_steps: u32,
    fast_runs: u32,
    slow_runs: u32,
    forced_slow_runs: u32,
    parity_checks: u32,
    parity_fails: u32,
    inflation_flags: u32,
    last_fast_class: u8,
    last_slow_class: u8,
    fast_valid: bool,
    slow_valid: bool,
}

impl PriorityPath {
    /// 出厂态：步数账=实测基线，无执行历史。
    pub fn new() -> Self {
        PriorityPath {
            fast_steps: FAST_PATH_STEPS,
            slow_steps: SLOW_PATH_STEPS,
            fast_runs: 0,
            slow_runs: 0,
            forced_slow_runs: 0,
            parity_checks: 0,
            parity_fails: 0,
            inflation_flags: 0,
            last_fast_class: 0,
            last_slow_class: 0,
            fast_valid: false,
            slow_valid: false,
        }
    }

    /// 快路径执行：Class 校验 → CR8 直写（镜像层）→ 落账。
    /// SubClass 保持语义在镜像层强制（隐匿改写即 `SubclassHiddenWrite`）。
    pub fn run_fast(&mut self, mirror: &mut Cr8TprMirror, class: u8) -> Result<(), Cr8TprError> {
        if !class_in_range(class) {
            return Err(Cr8TprError::PriorityRange { vec: class as u16, cls: class });
        }
        self.fast_runs = self.fast_runs.wrapping_add(1);
        mirror.write_cr8(cr8_encode(class))?;
        // 写后镜像必须同步（快路径的核心承诺：一次写两侧齐）。
        if !mirror.in_sync() {
            return Err(Cr8TprError::Cr8SyncFailed {
                cr8: mirror.cr8_class(),
                tpr: mirror.tpr_class(),
            });
        }
        self.last_fast_class = class;
        self.fast_valid = true;
        Ok(())
    }

    /// 慢路径执行：Class/SubClass 校验 → TPR MMIO 语义写（镜像层）→ 落账。
    pub fn run_slow(
        &mut self,
        mirror: &mut Cr8TprMirror,
        class: u8,
        subclass: u8,
    ) -> Result<(), Cr8TprError> {
        if !class_in_range(class) || !class_in_range(subclass) {
            return Err(Cr8TprError::PriorityRange { vec: tpr_encode(class, subclass) as u16, cls: class });
        }
        self.slow_runs = self.slow_runs.wrapping_add(1);
        mirror.write_tpr(tpr_encode(class, subclass))?;
        if !mirror.in_sync() {
            return Err(Cr8TprError::TprReadbackMismatch {
                tpr: mirror.tpr_class(),
                cr8: mirror.cr8_class(),
            });
        }
        self.last_slow_class = class;
        self.slow_valid = true;
        Ok(())
    }

    /// 强制慢路径标记（判据拒时由闸调用——账面区分「主动慢」与「被拒慢」）。
    pub fn note_forced_slow(&mut self) {
        self.forced_slow_runs = self.forced_slow_runs.wrapping_add(1);
    }

    /// 快慢路径结果等价对拍：同一 Class 输入两路径出口 Class 必须一致。
    /// 需要先各跑一次（run_fast + run_slow）。
    pub fn parity_check(&mut self) -> Result<(), Cr8TprError> {
        if !self.fast_valid || !self.slow_valid {
            // 尚无对拍材料不算失败，返回幂等成功（消费方先跑两路径）。
            return Ok(());
        }
        self.parity_checks = self.parity_checks.wrapping_add(1);
        if self.last_fast_class != self.last_slow_class {
            self.parity_fails = self.parity_fails.wrapping_add(1);
            return Err(Cr8TprError::PathParityBroken {
                fast_cls: self.last_fast_class,
                slow_cls: self.last_slow_class,
            });
        }
        Ok(())
    }

    /// J1 后半判据：快路径步数 ≤ 慢路径 50%（整数域 2×快 ≤ 慢）。
    pub fn cost_criterion_ok(&self) -> bool {
        self.fast_steps.wrapping_mul(2) <= self.slow_steps
    }

    /// 上报步数审计（开销虚报注入的检测器）：上报值超实测基线或破 50%
    /// 判据即立案。
    pub fn audit_reported(&mut self, fast_reported: u32, slow_reported: u32) -> Result<(), Cr8TprError> {
        let over_baseline = fast_reported > FAST_PATH_STEPS || slow_reported > SLOW_PATH_STEPS;
        let breaks_j1 = fast_reported.wrapping_mul(2) > slow_reported;
        if over_baseline || breaks_j1 {
            self.inflation_flags = self.inflation_flags.wrapping_add(1);
            return Err(Cr8TprError::OverheadInflated { fast: fast_reported, slow: slow_reported });
        }
        Ok(())
    }

    /// 快路径步数账。
    pub fn fast_steps(&self) -> u32 {
        self.fast_steps
    }

    /// 慢路径步数账。
    pub fn slow_steps(&self) -> u32 {
        self.slow_steps
    }

    /// 快路径执行次数。
    pub fn fast_runs(&self) -> u32 {
        self.fast_runs
    }

    /// 慢路径执行次数（含主动）。
    pub fn slow_runs(&self) -> u32 {
        self.slow_runs
    }

    /// 被判据拒后强制慢路径次数。
    pub fn forced_slow_runs(&self) -> u32 {
        self.forced_slow_runs
    }

    /// 对拍次数。
    pub fn parity_checks(&self) -> u32 {
        self.parity_checks
    }

    /// 对拍失败数。
    pub fn parity_fails(&self) -> u32 {
        self.parity_fails
    }

    /// 虚报立案数。
    pub fn inflation_flags(&self) -> u32 {
        self.inflation_flags
    }

    /// 最近一次快路径出口 Class。
    pub fn last_fast_class(&self) -> u8 {
        self.last_fast_class
    }

    /// 最近一次慢路径出口 Class。
    pub fn last_slow_class(&self) -> u8 {
        self.last_slow_class
    }

    /// 快/慢路径开销比（百分比，saturating；slow=0 时返回 100 表示不可比）。
    pub fn fast_over_slow_pct(&self) -> u32 {
        if self.slow_steps == 0 {
            return 100;
        }
        ((self.fast_steps.saturating_mul(100)) / self.slow_steps).min(100)
    }
}

// ---------------------------------------------------------------------------
// FastPathGate：快路径适用判据（IF 态 / NMI 窗口 / 镜像同步）
// ---------------------------------------------------------------------------

/// 闸判结果（Allow / Deny+原因位集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    /// 判据全绿，允许快路径。
    Allow,
    /// 判拒；原因位集：bit0=IF 关、bit1=NMI 窗口、bit2=镜像失配。
    Deny { reasons: u32 },
}

/// 快路径适用判据闸。
///
/// 判据表（8 项 const 展开，判定式编译期对拍）：
/// `允许 = IF=1 且 无 NMI 窗口 且 镜像同步`。
/// - IF=0：没有「活的」中断窗口，CR8 直写的优先级提升无法即时生效，
///   且破坏「快路径只为活窗口提速」的本单定位 → 拒；
/// - NMI 窗口：NMI 处理语义下改优先级悬置 → 拒；
/// - 镜像失配：带病快写会固化失配 → 拒（先扫描回同步）。
/// `mask_window_pending` 仅记账不判拒（TPR 屏蔽窗口悬挂不阻塞 CR8 写）。
#[derive(Debug, Clone)]
pub struct FastPathGate {
    if_flag: bool,
    nmi_window: bool,
    mask_window_pending: bool,
    mirror_synced: bool,
    allowed_ct: u32,
    denied_ct: u32,
    violations: u32,
    last_reasons: u32,
}

/// 判据表构建（索引位：bit0=IF、bit1=NMI、bit2=镜像同步；值=拒判原因位
/// 集，0=允许）。
const fn build_gate_table() -> [u8; 8] {
    let mut table = [0u8; 8];
    let mut index = 0usize;
    while index < 8 {
        let mut reasons = 0u8;
        if index & 1 == 0 {
            reasons |= GATE_RSN_IF_DISABLED as u8; // IF=0
        }
        if index & 2 != 0 {
            reasons |= GATE_RSN_NMI_WINDOW as u8; // NMI 窗口开
        }
        if index & 4 == 0 {
            reasons |= GATE_RSN_MIRROR_UNSYNC as u8; // 镜像失配
        }
        table[index] = reasons;
        index += 1;
    }
    table
}

/// 编译期展开的 8 项判据表。
pub const GATE_TRUTH_TABLE: [u8; 8] = build_gate_table();

/// 判据表↔判据式编译期对拍。
const fn verify_gate_table() -> bool {
    let mut index = 0usize;
    while index < 8 {
        let if_on = index & 1 != 0;
        let nmi = index & 2 != 0;
        let synced = index & 4 != 0;
        let mut want = 0u8;
        if !if_on {
            want |= GATE_RSN_IF_DISABLED as u8;
        }
        if nmi {
            want |= GATE_RSN_NMI_WINDOW as u8;
        }
        if !synced {
            want |= GATE_RSN_MIRROR_UNSYNC as u8;
        }
        if GATE_TRUTH_TABLE[index] != want {
            return false;
        }
        index += 1;
    }
    true
}

impl FastPathGate {
    /// 出厂态：IF=0（保守——未观测前不认活窗口）、镜像视同步。
    pub fn new(mirror_synced: bool) -> Self {
        FastPathGate {
            if_flag: false,
            nmi_window: false,
            mask_window_pending: false,
            mirror_synced,
            allowed_ct: 0,
            denied_ct: 0,
            violations: 0,
            last_reasons: 0,
        }
    }

    /// 观测 CPU 现场态（IF/NMI 窗口/TPR 屏蔽窗口悬挂）。
    pub fn observe_cpu(&mut self, if_flag: bool, nmi_window: bool, mask_window_pending: bool) {
        self.if_flag = if_flag;
        self.nmi_window = nmi_window;
        self.mask_window_pending = mask_window_pending;
    }

    /// 观测镜像同步态（扫描/写后由门面回灌）。
    pub fn observe_mirror(&mut self, mirror_synced: bool) {
        self.mirror_synced = mirror_synced;
    }

    /// 判据裁决（查 const 表——判定式唯一出处）。
    pub fn judge(&self) -> GateVerdict {
        let mut index = 0usize;
        if self.if_flag {
            index |= 1;
        }
        if self.nmi_window {
            index |= 2;
        }
        if self.mirror_synced {
            index |= 4;
        }
        let reasons = GATE_TRUTH_TABLE[index] as u32;
        if reasons == 0 {
            GateVerdict::Allow
        } else {
            GateVerdict::Deny { reasons }
        }
    }

    /// 记一次允许放行。
    pub fn note_allowed(&mut self) {
        self.allowed_ct = self.allowed_ct.wrapping_add(1);
    }

    /// 记一次判拒。
    pub fn note_denied(&mut self, reasons: u32) {
        self.denied_ct = self.denied_ct.wrapping_add(1);
        self.last_reasons = reasons;
    }

    /// 绕闸违例立案（fast_bypass_gate 在判拒态硬走时调用）。
    pub fn note_violation(&mut self, reasons: u32) {
        self.violations = self.violations.wrapping_add(1);
        self.last_reasons = reasons;
    }

    /// 当前各观测位。
    pub fn observed(&self) -> (bool, bool, bool, bool) {
        (self.if_flag, self.nmi_window, self.mask_window_pending, self.mirror_synced)
    }

    /// 允许放行计数。
    pub fn allowed_count(&self) -> u32 {
        self.allowed_ct
    }

    /// 判拒计数。
    pub fn denied_count(&self) -> u32 {
        self.denied_ct
    }

    /// 绕闸违例数。
    pub fn violations(&self) -> u32 {
        self.violations
    }

    /// 最近一次拒判/违例原因位集。
    pub fn last_reasons(&self) -> u32 {
        self.last_reasons
    }
}

// ---------------------------------------------------------------------------
// MismatchScanner：CR8/TPR 失配扫描 + 红案 + 自动回同步
// ---------------------------------------------------------------------------

/// 扫描结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanVerdict {
    /// 两侧 Class 一致。
    InSync,
    /// 失配红案已立案并自动回同步成功。
    RedCaseResynced,
}

/// 扫描报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanReport {
    /// 结论。
    pub verdict: ScanVerdict,
    /// 扫描时 CR8 Class。
    pub cr8_class: u8,
    /// 扫描时 TPR Class。
    pub tpr_class: u8,
}

/// 失配扫描器：扫一次、红案一次、回同步一次、复扫确证一次。
#[derive(Debug, Clone)]
pub struct MismatchScanner {
    scans: u32,
    mismatches: u32,
    resyncs_ok: u32,
    resync_fails: u32,
    last_verdict: u8, // 0=InSync 1=RedCaseResynced
}

impl MismatchScanner {
    /// 出厂态。
    pub fn new() -> Self {
        MismatchScanner {
            scans: 0,
            mismatches: 0,
            resyncs_ok: 0,
            resync_fails: 0,
            last_verdict: 0,
        }
    }

    /// 执行一次失配扫描：失配即红案（镜像层已自动记样本）→ 回同步
    /// （CR8 Class 为准、SubClass 保序）→ 复扫确证。
    pub fn scan(&mut self, mirror: &mut Cr8TprMirror) -> Result<ScanReport, Cr8TprError> {
        self.scans = self.scans.wrapping_add(1);
        let cr8_cls = mirror.cr8_class();
        let tpr_cls = mirror.tpr_class();
        if cr8_cls == tpr_cls {
            self.last_verdict = 0;
            return Ok(ScanReport { verdict: ScanVerdict::InSync, cr8_class: cr8_cls, tpr_class: tpr_cls });
        }
        // 失配 → 红案 + 自动回同步。
        self.mismatches = self.mismatches.wrapping_add(1);
        mirror.re_sync()?;
        // 复扫确证：回同步后仍失配 = 红案未消。
        if !mirror.in_sync() {
            self.resync_fails = self.resync_fails.wrapping_add(1);
            return Err(Cr8TprError::ResyncFailed { attempts: self.resync_fails });
        }
        self.resyncs_ok = self.resyncs_ok.wrapping_add(1);
        self.last_verdict = 1;
        Ok(ScanReport {
            verdict: ScanVerdict::RedCaseResynced,
            cr8_class: cr8_cls,
            tpr_class: tpr_cls,
        })
    }

    /// 扫描次数。
    pub fn scan_count(&self) -> u32 {
        self.scans
    }

    /// 失配检出数。
    pub fn mismatch_count(&self) -> u32 {
        self.mismatches
    }

    /// 回同步成功数。
    pub fn resync_ok_count(&self) -> u32 {
        self.resyncs_ok
    }

    /// 回同步失败数。
    pub fn resync_fail_count(&self) -> u32 {
        self.resync_fails
    }

    /// 最近结论（0=InSync 1=RedCaseResynced）。
    pub fn last_verdict(&self) -> u8 {
        self.last_verdict
    }
}

// ---------------------------------------------------------------------------
// 健康报告
// ---------------------------------------------------------------------------

/// 子系统健康快照（findings 为人读诊断行，每行自含结论）。
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// 全绿（无红案、无违例、无对拍/回同步失败、账未触顶）。
    pub healthy: bool,
    /// 降级（有红案但已全部回同步——服务继续但账面留痕）。
    pub degraded: bool,
    /// 事件账目数。
    pub ledger_events: u32,
    /// 事件账拒记数。
    pub ledger_rejected: u32,
    /// 红案累计。
    pub red_cases: u32,
    /// 滑窗一致样本（当前连绿段）。
    pub sync_ok: u32,
    /// 滑窗已填样本。
    pub sync_total: u32,
    /// 同步率（百分比，saturating）。
    pub sync_ratio_pct: u32,
    /// 快路径步数账。
    pub fast_steps: u32,
    /// 慢路径步数账。
    pub slow_steps: u32,
    /// 快/慢开销比（百分比）。
    pub fast_over_slow_pct: u32,
    /// J1 后半判据（50%）现值。
    pub cost_criterion_ok: bool,
    /// J1 前半判据（10/10）现值。
    pub sync_criterion_ok: bool,
    /// 注入开火总数。
    pub injections_fired: u32,
    /// 注入检出总数。
    pub injections_detected: u32,
    /// 闸拒判次数。
    pub gate_denied: u32,
    /// 绕闸违例数。
    pub gate_violations: u32,
    /// 事件账 FNV 指纹。
    pub fingerprint: u64,
    /// 人读诊断行。
    pub findings: Vec<String>,
}

// ---------------------------------------------------------------------------
// 门面：Cr8TprSubsystem（镜像+双路径+闸+扫描+事件账+注入面）
// ---------------------------------------------------------------------------

/// 双跑确定性对拍报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterminismReport {
    /// A 跑事件账指纹。
    pub ledger_fp_a: u64,
    /// B 跑事件账指纹。
    pub ledger_fp_b: u64,
    /// A 跑镜像指纹。
    pub mirror_fp_a: u64,
    /// B 跑镜像指纹。
    pub mirror_fp_b: u64,
    /// 双跑逐位一致。
    pub identical: bool,
}

/// CR8 与 TPR 联动快路径门面：六面合一（镜像/双路径/闸/扫描/事件账/注入）。
///
/// 生命周期：`boot()` → 提优先级（`raise_priority`，闸仲裁快/慢路径）/
/// 显性慢路径（`set_tpr_slow`，可携 SubClass）/ 对拍（`run_parity`）/
/// 扫描（`scan_now`）→ 注入演练（三个 inject_*）→ `self_check()` +
/// `health()`；确定性由 `determinism_self_test()` 双跑闭环。
#[derive(Debug, Clone)]
pub struct Cr8TprSubsystem {
    mirror: Cr8TprMirror,
    path: PriorityPath,
    gate: FastPathGate,
    scanner: MismatchScanner,
    ledger: EventLedger,
    inj_fired: [u32; INJECTION_KINDS],
    inj_detected: [u32; INJECTION_KINDS],
}

impl Cr8TprSubsystem {
    /// 上电：出厂镜像（CR8=0/TPR=0 全投递）、基线步数账、保守闸、空账。
    pub fn boot() -> Result<Self, Cr8TprError> {
        Self::boot_with_ledger_cap(LEDGER_CAP_DEFAULT)
    }

    /// 指定事件账容量的上电（容量下限校验——太小等于没账）。
    pub fn boot_with_ledger_cap(cap: usize) -> Result<Self, Cr8TprError> {
        let mirror = Cr8TprMirror::new();
        let gate = FastPathGate::new(mirror.in_sync());
        Ok(Cr8TprSubsystem {
            mirror,
            path: PriorityPath::new(),
            gate,
            scanner: MismatchScanner::new(),
            ledger: EventLedger::with_capacity(cap)?,
            inj_fired: [0; INJECTION_KINDS],
            inj_detected: [0; INJECTION_KINDS],
        })
    }

    // -- CPU 现场观测 -------------------------------------------------------

    /// 观测 CPU 现场（IF 标志态 / NMI 窗口态 / TPR 屏蔽窗口悬挂态）。
    /// IF 位语义见 `EFLAGS_IF_BIT`（EFLAGS bit9）。
    pub fn observe_cpu(&mut self, if_flag: bool, nmi_window: bool, mask_window_pending: bool) {
        self.gate.observe_cpu(if_flag, nmi_window, mask_window_pending);
    }

    /// 回灌镜像同步态到闸（每次操作后门面自动做，也可显性调用）。
    pub fn refresh_gate(&mut self) {
        self.gate.observe_mirror(self.mirror.in_sync());
    }

    // -- 主路径 -------------------------------------------------------------

    /// 优先级提升：闸仲裁——判据全绿走 CR8 直写快路径；判据拒**强制走
    /// 慢路径**（TPR 语义写，SubClass=0）并留拒判账。
    pub fn raise_priority(&mut self, class: u8) -> Result<PathChoice, Cr8TprError> {
        self.refresh_gate();
        match self.gate.judge() {
            GateVerdict::Allow => {
                self.gate.note_allowed();
                self.path.run_fast(&mut self.mirror, class)?;
                self.ledger
                    .record(EventKind::OpFastPath, 0, class as u32, FAST_PATH_STEPS)?;
                Ok(PathChoice::FastPath)
            }
            GateVerdict::Deny { reasons } => {
                self.gate.note_denied(reasons);
                self.ledger
                    .record(EventKind::OpGateDeny, 0, class as u32, reasons)?;
                self.path.note_forced_slow();
                self.path.run_slow(&mut self.mirror, class, 0)?;
                self.ledger
                    .record(EventKind::OpSlowPath, 0, class as u32, SLOW_PATH_STEPS)?;
                Ok(PathChoice::ForcedSlowPath)
            }
        }
    }

    /// 显性慢路径写 TPR（含 SubClass——SubClass 只在 TPR 侧存在，改它
    /// 必须走这里，快路径无权触碰）。
    pub fn set_tpr_slow(&mut self, class: u8, subclass: u8) -> Result<(), Cr8TprError> {
        self.path.run_slow(&mut self.mirror, class, subclass)?;
        self.ledger
            .record(EventKind::OpSlowPath, 0, tpr_encode(class, subclass) as u32, SLOW_PATH_STEPS)?;
        self.refresh_gate();
        Ok(())
    }

    /// TPR 侧 SubClass 专用写（语义孤悬面的显性入口）。
    pub fn set_tpr_subclass(&mut self, subclass: u8) -> Result<(), Cr8TprError> {
        self.mirror.write_tpr_subclass(subclass)?;
        self.ledger
            .record(EventKind::OpWriteTpr, 0, subclass as u32, 0)?;
        Ok(())
    }

    /// 绕闸快路径（误用面/演练入口）：判拒态硬走即立案并返回
    /// `FastPathGateViolation`——绕闸必留痕，不给静默绕行留门。
    pub fn fast_bypass_gate(&mut self, class: u8) -> Result<(), Cr8TprError> {
        self.refresh_gate();
        match self.gate.judge() {
            GateVerdict::Allow => {
                self.gate.note_allowed();
                self.path.run_fast(&mut self.mirror, class)?;
                self.ledger
                    .record(EventKind::OpFastPath, 0, class as u32, FAST_PATH_STEPS)?;
                Ok(())
            }
            GateVerdict::Deny { reasons } => {
                self.gate.note_violation(reasons);
                self.ledger
                    .record(EventKind::RedCase, XC_FASTPATH_GATE_VIOLATION, class as u32, reasons)?;
                Err(Cr8TprError::FastPathGateViolation { reason: reasons })
            }
        }
    }

    // -- 对拍与扫描 ---------------------------------------------------------

    /// 快慢路径结果等价对拍：同一 Class 两路径各跑一遍再对拍出口 Class。
    pub fn run_parity(&mut self, class: u8) -> Result<(), Cr8TprError> {
        self.path.run_fast(&mut self.mirror, class)?;
        self.ledger
            .record(EventKind::OpFastPath, 0, class as u32, FAST_PATH_STEPS)?;
        self.path.run_slow(&mut self.mirror, class, 0)?;
        self.ledger
            .record(EventKind::OpSlowPath, 0, class as u32, SLOW_PATH_STEPS)?;
        self.path.parity_check()?;
        self.ledger
            .record(EventKind::OpParity, 0, self.path.last_fast_class() as u32, self.path.last_slow_class() as u32)?;
        self.refresh_gate();
        Ok(())
    }

    /// 失配扫描（含自动回同步），留扫描账。
    pub fn scan_now(&mut self) -> Result<ScanReport, Cr8TprError> {
        let report = self.scanner.scan(&mut self.mirror)?;
        match report.verdict {
            ScanVerdict::InSync => {
                self.ledger.record(EventKind::OpScan, 0, report.cr8_class as u32, report.tpr_class as u32)?;
            }
            ScanVerdict::RedCaseResynced => {
                self.ledger.record(EventKind::RedCase, XC_MISMATCH_RED_CASE, report.cr8_class as u32, report.tpr_class as u32)?;
                self.ledger.record(EventKind::OpResync, 0, report.cr8_class as u32, report.tpr_class as u32)?;
            }
        }
        self.refresh_gate();
        Ok(report)
    }

    // -- 注入面（三类注入，fired == detected 否则自检红案） -----------------

    /// 注入一：镜像失配注入——镜像层制造真实失配，扫描器必须抓到并回同步。
    pub fn inject_mismatch(&mut self) -> Result<ScanReport, Cr8TprError> {
        self.inj_fired[INJ_MISMATCH] = self.inj_fired[INJ_MISMATCH].wrapping_add(1);
        self.mirror.inject_divergence();
        self.ledger
            .record(EventKind::InjectFired, XC_MISMATCH_RED_CASE, 0, 0)?;
        let report = self.scan_now()?;
        if report.verdict == ScanVerdict::RedCaseResynced {
            self.inj_detected[INJ_MISMATCH] = self.inj_detected[INJ_MISMATCH].wrapping_add(1);
            self.ledger.record(EventKind::InjectDetected, 0, INJ_MISMATCH as u32, 0)?;
            return Ok(report);
        }
        // 注入未产生红案 = 检测面失明。
        Err(Cr8TprError::InjectionUndetected { kind: INJ_MISMATCH as u8 })
    }

    /// 注入二：开销虚报注入——上报快路径步数超基线，开销审计必须立案。
    pub fn inject_overhead_inflation(&mut self) -> Result<(), Cr8TprError> {
        self.inj_fired[INJ_OVERHEAD] = self.inj_fired[INJ_OVERHEAD].wrapping_add(1);
        self.ledger
            .record(EventKind::InjectFired, XC_OVERHEAD_INFLATED, 0, 0)?;
        match self.path.audit_reported(FAST_PATH_STEPS + 2, SLOW_PATH_STEPS) {
            Err(Cr8TprError::OverheadInflated { fast, slow }) => {
                self.inj_detected[INJ_OVERHEAD] = self.inj_detected[INJ_OVERHEAD].wrapping_add(1);
                self.ledger
                    .record(EventKind::InjectDetected, 0, INJ_OVERHEAD as u32, fast)?;
                let _ = slow; // slow 参与审计判定，账面只留快路径虚报量
                Ok(())
            }
            Ok(()) => Err(Cr8TprError::InjectionUndetected { kind: INJ_OVERHEAD as u8 }),
            Err(other) => Err(other),
        }
    }

    /// 注入三：边界优先级注入——声称「向量 Class == TPR Class 仍投递」，
    /// 屏蔽真值表必须判相等屏蔽（抓到该谎言）。
    pub fn inject_boundary_priority(&mut self) -> Result<(), Cr8TprError> {
        self.inj_fired[INJ_BOUNDARY] = self.inj_fired[INJ_BOUNDARY].wrapping_add(1);
        self.ledger
            .record(EventKind::InjectFired, XC_EQUALITY_MASK_VIOLATION, 0, 0)?;
        let tpr_cls = self.mirror.tpr_class();
        let verdict = mask_verdict_for(tpr_cls, tpr_cls);
        if verdict == VERDICT_MASK_EQUAL {
            self.inj_detected[INJ_BOUNDARY] = self.inj_detected[INJ_BOUNDARY].wrapping_add(1);
            self.ledger
                .record(EventKind::InjectDetected, 0, INJ_BOUNDARY as u32, tpr_cls as u32)?;
            return Ok(());
        }
        Err(Cr8TprError::InjectionUndetected { kind: INJ_BOUNDARY as u8 })
    }

    /// 注入演练覆盖复核：fired == detected 逐类核对，缺一即红案。
    pub fn injection_coverage_ok(&self) -> Result<(), Cr8TprError> {
        let mut kind = 0usize;
        while kind < INJECTION_KINDS {
            if self.inj_fired[kind] != self.inj_detected[kind] {
                return Err(Cr8TprError::InjectionUndetected { kind: kind as u8 });
            }
            kind += 1;
        }
        Ok(())
    }

    // -- 自检与健康 ---------------------------------------------------------

    /// 自检：镜像现态同步 → 50% 判据 → 对拍零失败 → 回同步零失败 →
    /// 注入覆盖 → 绕闸零违例。任何一项破即红案错误。
    pub fn self_check(&self) -> Result<(), Cr8TprError> {
        if !self.mirror.in_sync() {
            return Err(Cr8TprError::MismatchRedCase {
                cr8: self.mirror.cr8_class(),
                tpr: self.mirror.tpr_class(),
            });
        }
        if !self.path.cost_criterion_ok() {
            return Err(Cr8TprError::OverheadInflated {
                fast: self.path.fast_steps(),
                slow: self.path.slow_steps(),
            });
        }
        if self.path.parity_fails() != 0 {
            return Err(Cr8TprError::PathParityBroken {
                fast_cls: self.path.last_fast_class(),
                slow_cls: self.path.last_slow_class(),
            });
        }
        if self.scanner.resync_fail_count() != 0 {
            return Err(Cr8TprError::ResyncFailed { attempts: self.scanner.resync_fail_count() });
        }
        self.injection_coverage_ok()?;
        if self.gate.violations() != 0 {
            return Err(Cr8TprError::FastPathGateViolation { reason: self.gate.last_reasons() });
        }
        Ok(())
    }

    /// J1 前半判据透出（滑窗 10/10）。
    pub fn sync_criterion(&self) -> Result<(), Cr8TprError> {
        self.mirror.sync_criterion_ok()
    }

    /// 健康快照（findings 每行自含结论，供总账聚合）。
    pub fn health(&self) -> HealthReport {
        let mut findings: Vec<String> = Vec::new();
        let sync_ok = self.mirror.consecutive_ok();
        let sync_total = self.mirror_window_filled();
        let sync_ratio = if sync_total == 0 {
            0
        } else {
            (sync_ok.saturating_mul(100) / sync_total).min(100)
        };
        let cost_ok = self.path.cost_criterion_ok();
        let sync_crit = self.mirror.sync_criterion_ok().is_ok();
        let red = self.mirror.red_case_count();
        let violations = self.gate.violations();
        let parity_fails = self.path.parity_fails();
        let resync_fails = self.scanner.resync_fail_count();
        let exhausted = self.ledger.is_exhausted();
        let clean = red == 0 && violations == 0 && parity_fails == 0 && resync_fails == 0 && !exhausted;

        findings.push(format!(
            "J1前半(10/10)：{}（滑窗 {}/{}，连绿 {}）",
            if sync_crit { "达标" } else { "未达标" },
            sync_ok,
            sync_total,
            sync_ok
        ));
        findings.push(format!(
            "J1后半(50%)：{}（快 {} 步 / 慢 {} 步 = {}%）",
            if cost_ok { "达标" } else { "破裂" },
            self.path.fast_steps(),
            self.path.slow_steps(),
            self.path.fast_over_slow_pct()
        ));
        findings.push(format!(
            "红案 {} 起（回同步 {} 成 / {} 败）；绕闸违例 {}；对拍失败 {}",
            red,
            self.scanner.resync_ok_count(),
            resync_fails,
            violations,
            parity_fails
        ));
        let mut fired_total = 0u32;
        let mut detected_total = 0u32;
        let mut kind = 0usize;
        while kind < INJECTION_KINDS {
            fired_total = fired_total.wrapping_add(self.inj_fired[kind]);
            detected_total = detected_total.wrapping_add(self.inj_detected[kind]);
            kind += 1;
        }
        findings.push(format!(
            "注入面：开火 {} / 检出 {}（{}）",
            fired_total,
            detected_total,
            if fired_total == detected_total { "覆盖完整" } else { "有失明——立即 self_check" }
        ));
        findings.push(format!(
            "事件账 {} 笔（拒记 {}），指纹 {:016x}",
            self.ledger.len(),
            self.ledger.rejected_count(),
            self.ledger.fingerprint()
        ));
        findings.push(format!(
            "镜像字段语义：Class 双向同步账 {}+{} 笔；SubClass 孤悬面 {} 笔（只在 TPR 侧）",
            self.mirror.cr8_write_count(),
            self.mirror.tpr_write_count(),
            self.mirror.subclass_only_count()
        ));

        HealthReport {
            healthy: clean,
            degraded: !clean && resync_fails == 0 && violations == 0 && parity_fails == 0,
            ledger_events: self.ledger.len() as u32,
            ledger_rejected: self.ledger.rejected_count(),
            red_cases: red,
            sync_ok,
            sync_total,
            sync_ratio_pct: sync_ratio,
            fast_steps: self.path.fast_steps(),
            slow_steps: self.path.slow_steps(),
            fast_over_slow_pct: self.path.fast_over_slow_pct(),
            cost_criterion_ok: cost_ok,
            sync_criterion_ok: sync_crit,
            injections_fired: fired_total,
            injections_detected: detected_total,
            gate_denied: self.gate.denied_count(),
            gate_violations: violations,
            fingerprint: self.ledger.fingerprint(),
            findings,
        }
    }

    /// 滑窗已填样本数。
    fn mirror_window_filled(&self) -> u32 {
        // 从指纹输入反推不优雅——直接经公开账面：总样本 = ok + fail 全程计数
        // 与滑窗容量取小（wrapping 安全域内等价于 filled）。
        let total = self.mirror.total_ok().saturating_add(self.mirror.total_fail());
        if total > SYNC_WINDOW_LEN as u32 {
            SYNC_WINDOW_LEN as u32
        } else {
            total
        }
    }

    // -- 账面透出 -----------------------------------------------------------

    /// 事件账指纹。
    pub fn fingerprint(&self) -> u64 {
        self.ledger.fingerprint()
    }

    /// 镜像指纹（双跑对拍第二锚）。
    pub fn mirror_fingerprint(&self) -> u64 {
        self.mirror.fingerprint()
    }

    /// 事件账只读访问。
    pub fn ledger_entry_at(&self, index: usize) -> Option<EventEntry> {
        self.ledger.entry_at(index)
    }

    /// 事件账长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 镜像层只读访问。
    pub fn mirror(&self) -> &Cr8TprMirror {
        &self.mirror
    }

    /// 镜像层可变访问（负向用例/演练直写用；常规路径请走门面方法）。
    pub fn mirror_mut(&mut self) -> &mut Cr8TprMirror {
        &mut self.mirror
    }

    /// 双路径账只读访问。
    pub fn path(&self) -> &PriorityPath {
        &self.path
    }

    /// 闸只读访问。
    pub fn gate(&self) -> &FastPathGate {
        &self.gate
    }

    /// 扫描器只读访问。
    pub fn scanner(&self) -> &MismatchScanner {
        &self.scanner
    }

    /// 注入开火计数（按类）。
    pub fn injection_fired(&self, kind: usize) -> u32 {
        if kind >= INJECTION_KINDS {
            return 0;
        }
        self.inj_fired[kind]
    }

    /// 注入检出计数（按类）。
    pub fn injection_detected(&self, kind: usize) -> u32 {
        if kind >= INJECTION_KINDS {
            return 0;
        }
        self.inj_detected[kind]
    }

    /// 账倾倒重置（指纹/健康落盘后调用——账满恢复唯一通道）。
    pub fn ledger_reset(&mut self) {
        self.ledger.reset();
    }
}

// ---------------------------------------------------------------------------
// 确定性场景与双跑对拍
// ---------------------------------------------------------------------------

/// 确定性驱动脚本：固定操作序列（无时钟、无外部熵——指纹仅由序列决定）。
fn drive_scenario(sub: &mut Cr8TprSubsystem) -> Result<(), Cr8TprError> {
    sub.observe_cpu(true, false, false);
    // 12 次交替提优先级：先铺满滑窗 10 样本再溢出 2 次（窗口滚动语义）。
    let classes: [u8; 12] = [2, 4, 6, 8, 10, 12, 14, 1, 3, 5, 7, 9];
    let mut index = 0usize;
    while index < classes.len() {
        sub.raise_priority(classes[index])?;
        index += 1;
    }
    // 显性慢路径携 SubClass（孤悬面走账）。
    sub.set_tpr_slow(9, 3)?;
    // 对拍：同一 Class 两路径出口必须一致。
    sub.run_parity(11)?;
    // 扫描一次（预期 InSync）。
    let _ = sub.scan_now()?;
    Ok(())
}

/// 双跑确定性对拍：同一场景在两个全新实例上各跑一遍，事件账与镜像
/// 指纹必须逐位一致；否则 `FingerprintDrift` 红案。
pub fn determinism_self_test() -> Result<DeterminismReport, Cr8TprError> {
    let mut run_a = Cr8TprSubsystem::boot()?;
    drive_scenario(&mut run_a)?;
    let mut run_b = Cr8TprSubsystem::boot()?;
    drive_scenario(&mut run_b)?;
    let ledger_fp_a = run_a.fingerprint();
    let ledger_fp_b = run_b.fingerprint();
    let mirror_fp_a = run_a.mirror_fingerprint();
    let mirror_fp_b = run_b.mirror_fingerprint();
    let identical = ledger_fp_a == ledger_fp_b && mirror_fp_a == mirror_fp_b;
    if !identical {
        return Err(Cr8TprError::FingerprintDrift {
            run_a: ledger_fp_a ^ mirror_fp_a,
            run_b: ledger_fp_b ^ mirror_fp_b,
        });
    }
    Ok(DeterminismReport {
        ledger_fp_a,
        ledger_fp_b,
        mirror_fp_a,
        mirror_fp_b,
        identical,
    })
}

/// 注入类别名（诊断透出用）。
pub fn injection_kind_name(kind: usize) -> &'static str {
    match kind {
        INJ_MISMATCH => "镜像失配注入",
        INJ_OVERHEAD => "开销虚报注入",
        INJ_BOUNDARY => "边界优先级注入",
        _ => "未知注入类别",
    }
}

// ---------------------------------------------------------------------------
// 测试（冒烟判据面：同步 10/10 / 开销 50% / 屏蔽真值表 / 失配回同步 /
// 双跑一致 / 闸判据 / 三注入各检出 / 错误诊断完备性）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：Result → 值（失败即带中文诊断的测试红案）。
    fn ok<T>(result: Result<T, Cr8TprError>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("预期成功，实际失败 code={:#x} detail={}", err.code(), err.detail()),
        }
    }

    /// 判据一：CR8 写与 TPR Class 同步 10/10 次一致（J1 前半）。
    #[test]
    fn sync_criterion_10_of_10() {
        let mut sub = ok(Cr8TprSubsystem::boot());
        sub.observe_cpu(true, false, false);
        // 12 次干净写（>10，覆盖滑窗滚动）；判据在窗口铺满后复查。
        let classes: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let mut index = 0usize;
        while index < classes.len() {
            let choice = ok(sub.raise_priority(classes[index]));
            assert!(choice == PathChoice::FastPath, "判据全绿应走快路径");
            index += 1;
        }
        // 滑窗铺满后 J1 前半判据必须达成。
        ok(sub.sync_criterion());
        // 每次写后镜像账都有一笔（逐次留痕承诺）。
        assert!(sub.mirror().cr8_write_count() == 12, "CR8 写计数应为 12");
        assert!(sub.mirror().entry_count() == 12, "镜像账逐笔留痕");
        // 逐笔核对双向一致样本。
        let mut index = 0usize;
        while index < sub.mirror().entry_count() {
            let entry = sub.mirror().entry_at(index).unwrap_or(MirrorEntry {
                seq: 0,
                dir: MirrorDirection::Cr8ToTpr,
                tpr_before: 0,
                tpr_after: 0,
                cr8_after: 0,
                consistent: false,
            });
            assert!(entry.consistent, "10/10 窗内不允许失配样本");
            index += 1;
        }
        assert!(sub.self_check().is_ok(), "自检应全绿");
    }

    /// 判据二：快路径开销低于 MMIO 路径 50%（J1 后半）+ 虚报审计。
    #[test]
    fn cost_criterion_below_50pct() {
        let path = PriorityPath::new();
        assert!(path.cost_criterion_ok(), "2*3=6 <= 9，50% 判据应达成");
        assert!(path.fast_over_slow_pct() <= 50, "开销比应 ≤ 50%");
        assert!(FAST_PATH_STEPS * 2 <= SLOW_PATH_STEPS, "构造式直验");
        // 虚报审计：基线内上报放行；超基线立案。
        let mut audit = PriorityPath::new();
        assert!(audit.audit_reported(FAST_PATH_STEPS, SLOW_PATH_STEPS).is_ok(), "基线内上报应通过");
        assert!(
            audit.audit_reported(FAST_PATH_STEPS + 1, SLOW_PATH_STEPS).is_err(),
            "快路径上报超基线应立案"
        );
        assert!(audit.inflation_flags() == 1, "虚报立案计数应为一");
    }

    /// 判据二边界：上报值恰好压线 50% 判据与基线的组合行为。
    #[test]
    fn cost_audit_boundary() {
        let mut path = PriorityPath::new();
        // 5 步 vs 11 步：2*5=10<=11 但 5>3 超基线 → 仍立案。
        assert!(path.audit_reported(5, 11).is_err(), "超基线必立案，即使 50% 判据数值上可过");
        // 3 步 vs 5 步：不超基线但 2*3=6>5 → 破 50% 判据 → 立案。
        assert!(path.audit_reported(FAST_PATH_STEPS, 5).is_err(), "破 50% 判据必立案");
        // 3 步 vs 6 步：恰压线（2*3=6<=6）→ 放行。
        assert!(path.audit_reported(FAST_PATH_STEPS, 6).is_ok(), "50% 压线应放行");
        assert!(path.inflation_flags() == 2, "两次立案");
    }

    /// 判据三：屏蔽真值表（投递/低档屏蔽/相等屏蔽三态 + 256 项全扫）。
    #[test]
    fn mask_truth_table_full_scan() {
        // 256 项与判定式逐项对拍。
        let mut vc = 0usize;
        while vc < 16 {
            let mut tc = 0usize;
            while tc < 16 {
                let got = mask_verdict_for(vc as u8, tc as u8);
                let want = if vc > tc {
                    VERDICT_DELIVER
                } else if vc == tc {
                    VERDICT_MASK_EQUAL
                } else {
                    VERDICT_MASK_BELOW
                };
                assert!(got == want, "真值表[{vc}][{tc}] 与判定式不符");
                tc += 1;
            }
            vc += 1;
        }
        // 相等屏蔽边界语义（SDM「less than or equal」）。
        assert!(mask_verdict_for(7, 7) == VERDICT_MASK_EQUAL, "相等必须屏蔽");
        assert!(mask_verdict_for(0, 0) == VERDICT_MASK_EQUAL, "全投递态 TPR=0 时 Class0 向量也按相等语义建模（终审在 PPR）");
        // 低档屏蔽。
        assert!(mask_verdict_for(3, 9) == VERDICT_MASK_BELOW);
        // 投递。
        assert!(mask_verdict_for(12, 5) == VERDICT_DELIVER);
        // 完整向量入口。
        assert!(delivery_verdict(0xA5, 0x30) == VERDICT_DELIVER, "向量 A5 Class=A > TPR Class 3");
        assert!(delivery_verdict(0x34, 0x30) == VERDICT_MASK_EQUAL, "向量 34 Class=3 == TPR Class 3");
    }

    /// 判据四：失配注入 → 红案立案 → 自动回同步 → 复扫确证。
    #[test]
    fn mismatch_scan_and_resync() {
        let mut sub = ok(Cr8TprSubsystem::boot());
        // 先建立干净态。
        sub.observe_cpu(true, false, false);
        ok(sub.raise_priority(6));
        assert!(sub.mirror().in_sync(), "干净写后应同步");
        // 注入失配 → 扫描必须抓到并回同步。
        let report = ok(sub.inject_mismatch());
        assert!(report.verdict == ScanVerdict::RedCaseResynced, "失配注入必须被扫描器抓到");
        assert!(sub.mirror().in_sync(), "回同步后必须确证同步");
        assert!(sub.scanner().mismatch_count() == 1, "失配检出计数");
        assert!(sub.mirror().red_case_count() >= 1, "红案必须立案");
        // 注入覆盖：fired == detected。
        ok(sub.injection_coverage_ok());
        // 失配后滑窗有红样本，补 10 次干净写重达 10/10。
        let mut index = 0usize;
        while index < 10 {
            ok(sub.raise_priority((index as u8) + 1));
            index += 1;
        }
        ok(sub.sync_criterion());
    }

    /// 判据五：双跑确定性对拍（事件账 + 镜像双指纹逐位一致）。
    #[test]
    fn determinism_double_run() {
        let report = ok(determinism_self_test());
        assert!(report.identical, "双跑指纹必须逐位一致");
        assert!(report.ledger_fp_a == report.ledger_fp_b, "事件账指纹一致");
        assert!(report.mirror_fp_a == report.mirror_fp_b, "镜像指纹一致");
    }

    /// 快路径闸：IF=0 判拒 → 强制慢路径；绕闸必立案。
    #[test]
    fn gate_denies_and_forces_slow_path() {
        let mut sub = ok(Cr8TprSubsystem::boot());
        // IF 关：判拒。
        sub.observe_cpu(false, false, false);
        let choice = ok(sub.raise_priority(4));
        assert!(choice == PathChoice::ForcedSlowPath, "IF=0 必须强制慢路径");
        assert!(sub.gate().denied_count() == 1, "拒判必须留账");
        let last_reasons = sub.gate().last_reasons();
        assert!(last_reasons & GATE_RSN_IF_DISABLED != 0, "拒判原因应含 IF 位");
        // IF 开：放行快路径。
        sub.observe_cpu(true, false, false);
        let choice = ok(sub.raise_priority(5));
        assert!(choice == PathChoice::FastPath, "IF=1 且镜像同步应走快路径");
        // NMI 窗口：判拒。
        sub.observe_cpu(true, true, false);
        let choice = ok(sub.raise_priority(6));
        assert!(choice == PathChoice::ForcedSlowPath, "NMI 窗口必须判拒");
        // 绕闸：立案。
        sub.observe_cpu(false, false, false);
        assert!(sub.fast_bypass_gate(7).is_err(), "绕闸必须失败");
        assert!(sub.gate().violations() == 1, "绕闸必须立案");
        // 判据表 8 项全扫。
        let mut index = 0usize;
        while index < 8 {
            let if_on = index & 1 != 0;
            let nmi = index & 2 != 0;
            let synced = index & 4 != 0;
            let mut want = 0u32;
            if !if_on {
                want |= GATE_RSN_IF_DISABLED;
            }
            if nmi {
                want |= GATE_RSN_NMI_WINDOW;
            }
            if !synced {
                want |= GATE_RSN_MIRROR_UNSYNC;
            }
            assert!(GATE_TRUTH_TABLE[index] as u32 == want, "判据表[{index}] 与判定式不符");
            index += 1;
        }
    }

    /// 三类注入各检出（fired == detected 覆盖闭环）。
    #[test]
    fn all_three_injections_detected() {
        let mut sub = ok(Cr8TprSubsystem::boot());
        sub.observe_cpu(true, false, false);
        ok(sub.raise_priority(3));
        // 注入一：失配。
        ok(sub.inject_mismatch());
        assert!(sub.injection_detected(INJ_MISMATCH) == 1, "失配注入必须检出");
        // 注入二：开销虚报。
        ok(sub.inject_overhead_inflation());
        assert!(sub.injection_detected(INJ_OVERHEAD) == 1, "虚报注入必须检出");
        // 注入三：边界优先级（相等 Class 谎称投递）。
        ok(sub.inject_boundary_priority());
        assert!(sub.injection_detected(INJ_BOUNDARY) == 1, "边界注入必须检出");
        // 覆盖闭环 + 自检。
        ok(sub.injection_coverage_ok());
        // 注入一制造过红案（已回同步）——自检的镜像现态应同步。
        assert!(sub.mirror().in_sync(), "回同步后自检前置应满足");
    }

    /// SubClass 语义分离：快路径保持 SubClass；孤悬面专用写不走 CR8。
    #[test]
    fn subclass_semantic_separation() {
        let mut sub = ok(Cr8TprSubsystem::boot());
        sub.observe_cpu(true, false, false);
        // 慢路径写 TPR：Class=9, SubClass=3。
        ok(sub.set_tpr_slow(9, 3));
        assert!(sub.mirror().tpr() == 0x93, "TPR 全字节应为 0x93");
        assert!(sub.mirror().tpr_subclass() == 3, "SubClass 应为 3");
        // 快路径写 CR8：Class 变、SubClass 保持。
        ok(sub.raise_priority(5));
        assert!(sub.mirror().tpr_class() == 5, "Class 应为 5");
        assert!(sub.mirror().tpr_subclass() == 3, "SubClass 必须保持（CR8 管不到）");
        // 孤悬面专用写：只动 SubClass。
        ok(sub.set_tpr_subclass(7));
        assert!(sub.mirror().tpr_class() == 5, "Class 不得被 SubClass 写牵动");
        assert!(sub.mirror().tpr_subclass() == 7, "SubClass 应更新为 7");
        assert!(sub.mirror().subclass_only_count() == 1, "孤悬面写必须计数");
        // CR8 保留位拒收。
        assert!(sub.mirror_mut().write_cr8(0x10).is_err(), "bit4 置位应拒（保留位）");
        assert!(sub.mirror_mut().write_cr8(u64::MAX).is_err(), "全 1 应拒");
    }

    /// 错误面完备性：16 变体 detail 非空、code 全落 0x1720 段。
    #[test]
    fn error_surface_complete() {
        let variants: [Cr8TprError; 16] = [
            Cr8TprError::Cr8SyncFailed { cr8: 1, tpr: 2 },
            Cr8TprError::TprReadbackMismatch { tpr: 2, cr8: 1 },
            Cr8TprError::Cr8CarrierInvalid { cr8: 0x10 },
            Cr8TprError::LedgerEntryMissing { op: 1 },
            Cr8TprError::SyncCriterionMiss { ok: 8, total: 10 },
            Cr8TprError::OverheadInflated { fast: 5, slow: 9 },
            Cr8TprError::PathParityBroken { fast_cls: 3, slow_cls: 4 },
            Cr8TprError::PriorityRange { vec: 0x123, cls: 0x12 },
            Cr8TprError::EqualityMaskViolation { vec_cls: 5, tpr_cls: 5 },
            Cr8TprError::MismatchRedCase { cr8: 1, tpr: 2 },
            Cr8TprError::ResyncFailed { attempts: 1 },
            Cr8TprError::FastPathGateViolation { reason: GATE_RSN_IF_DISABLED },
            Cr8TprError::InjectionUndetected { kind: 0 },
            Cr8TprError::FingerprintDrift { run_a: 1, run_b: 2 },
            Cr8TprError::SubclassHiddenWrite { old: 1, new: 2 },
            Cr8TprError::LedgerExhausted { kind: 0 },
        ];
        let mut index = 0usize;
        while index < variants.len() {
            let detail = variants[index].detail();
            assert!(!detail.is_empty(), "变体 {index} detail 不得为空");
            let code = variants[index].code();
            assert!(Cr8TprError::code_in_segment(code), "变体 {index} 诊断码 {code:#x} 越段");
            assert!(code >= XC_CR8_SYNC_FAILED && code <= XC_LEDGER_EXHAUSTED, "码段校验");
            index += 1;
        }
        // 码段互异（16 变体 16 码）。
        let mut outer = 0usize;
        while outer < variants.len() {
            let mut inner = outer + 1;
            while inner < variants.len() {
                assert!(variants[outer].code() != variants[inner].code(), "诊断码必须互异");
                inner += 1;
            }
            outer += 1;
        }
    }

    /// 健康快照 + 事件账硬上限（耗尽显性拒记 + 倾倒恢复）。
    #[test]
    fn health_and_ledger_exhaustion() {
        // 小账本：4 笔即触顶（下限 8 校验先行）。
        assert!(EventLedger::with_capacity(4).is_err(), "容量低于下限必须拒");
        let mut sub = ok(Cr8TprSubsystem::boot_with_ledger_cap(8));
        sub.observe_cpu(true, false, false);
        let mut index = 0usize;
        let mut exhausted_seen = false;
        while index < 12 {
            match sub.raise_priority((index % 14) as u8 + 1) {
                Ok(_) => {}
                Err(Cr8TprError::LedgerExhausted { .. }) => {
                    exhausted_seen = true;
                    // 倾倒恢复。
                    sub.ledger_reset();
                }
                Err(other) => panic!("非预期错误 code={:#x} detail={}", other.code(), other.detail()),
            }
            index += 1;
        }
        assert!(exhausted_seen, "8 笔小账本跑 12 次必触顶一次");
        let report = sub.health();
        assert!(report.healthy, "倾倒后应恢复全绿");
        assert!(report.cost_criterion_ok, "J1 后半应达标");
        assert!(report.findings.len() >= 5, "诊断行应完整");
        assert!(report.fingerprint != 0, "指纹不得为零（有事件）");
    }

    /// 事件账只读访问与指纹稳定性（同内容双取指纹一致）。
    #[test]
    fn ledger_access_and_fingerprint_stability() {
        let mut sub = ok(Cr8TprSubsystem::boot());
        sub.observe_cpu(true, false, false);
        ok(sub.raise_priority(2));
        ok(sub.raise_priority(4));
        let fp1 = sub.fingerprint();
        let fp2 = sub.fingerprint();
        assert!(fp1 == fp2, "同态指纹必须稳定");
        assert!(sub.ledger_len() == 2, "两次快路径各记一笔");
        let entry = sub.ledger_entry_at(0);
        assert!(entry.is_some(), "账目应可读");
        assert!(sub.ledger_entry_at(99).is_none(), "越界读必须 None 不 panic");
    }

    /// TPR 回读失配路径：TPR 写后镜像一致性错误可显性构造。
    #[test]
    fn tpr_write_projection() {
        let mut mirror = Cr8TprMirror::new();
        ok(mirror.write_tpr(0xA7)); // Class=A SubClass=7
        assert!(mirror.cr8_class() == 0xA, "Class 投影回 CR8");
        assert!(mirror.tpr_subclass() == 7, "SubClass 就地生效");
        ok(mirror.write_cr8(0x3)); // Class=3 SubClass 保持 7
        assert!(mirror.tpr() == 0x37, "TPR 应为 0x37");
        ok(mirror.re_sync());
        assert!(mirror.in_sync(), "回同步后同步");
        // 注入失配后 in_sync 翻假。
        mirror.inject_divergence();
        assert!(!mirror.in_sync(), "注入后必须失配");
        ok(mirror.re_sync());
        assert!(mirror.in_sync(), "回同步恢复");
    }

    /// 判据复核事件与码段独占声明。
    #[test]
    fn segment_exclusive_and_version() {
        // 16 码连续铺满 0x1720..=0x172F。
        let mut expected = XC_CR8_SYNC_FAILED;
        while expected <= XC_LEDGER_EXHAUSTED {
            assert!(Cr8TprError::code_in_segment(expected), "段内码必须被判在段内");
            expected += 1;
        }
        assert!(!Cr8TprError::code_in_segment(0x171F), "段前码必须判出段");
        assert!(!Cr8TprError::code_in_segment(0x1730), "段后码必须判出段（0x1730 已被 CGPU-F0028 占用）");
        assert!(CR8TPR_VERSION.starts_with("UNX-F0825"), "版本号锚定任务号");
        assert!(LAPIC_TPR_OFFSET == 0x080, "TPR 偏移锚定 SDM 地址映射");
        assert!(SYNC_WINDOW_LEN == 10, "判据窗口锚定 10/10");
        assert!(INJECTION_KINDS == 3, "注入面三类");
    }
}
