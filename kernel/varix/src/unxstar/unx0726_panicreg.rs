//! UNX-F0726 · 必需响应缺失 panic 路径回归 · 诊断段 0x2730
//!
//! ── 验收锚点（原文全抄） ─────────────────────────────────────────────
//! 定位：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//! 行 2316，表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」下（B37 批
//! 「UNX-A1-B37 · Limine 嫁接跟随与升级回归（F721–F740 · 20 条）」）：
//!
//! | UNX-F0726 | 必需响应缺失 panic 路径回归 | 570 | 已深化 | UNX-F0726-J1
//!   panic 路径回归过（必需响应缺失注入 → PANIC 文案含 tag 名可观测 100%）
//!   三重 fault 挂死语义保持 |
//!
//! J 判据拆解（本模块逐条落地的验收口径）：
//!   J1-a 必需 tag 闭集六项：与 F0725 响应面三臂（memory_map/framebuffer/
//!        kernel_address）对齐声明，另加 Limine boot info 必需响应
//!        hhdm/modules/firmware，共六项 const 清单（REQUIRED_TAGS），
//!        tag 码闭集 1..=6，闭集外 E_TAG_UNKNOWN（0x2731）拒收。
//!   J1-b 缺失即 panic 路径：RequiredRespGate 逐项在场校验，任一必需 tag
//!        缺席即错误码化「panic 裁决」（不真触发 panic——内核路径零
//!        panic!/unwrap/expect，裁决全部走 Result<T, PanicRegError>），
//!        E_TAG_MISSING（0x2732）为 panic 路径入口码。
//!   J1-c PANIC 文案三要素 100%：文案必须含缺失 tag 名 + 错误码 + 位置
//!        三要素（J1 原文「PANIC 文案含 tag 名可观测 100%」的落地面）；
//!        任一要素缺失分别 E_MSG_NO_TAG / E_MSG_NO_CODE / E_MSG_NO_SITE
//!        （0x2736/0x2737/0x2738）检出；文案入账（不落盘——账面可查）。
//!   J1-d 三重 fault 挂死语义保持：panic 后不可恢复——panic 路径再次进入
//!        即 double fault，再进即 triple fault → 挂死态（Halt）；挂死态
//!        显性（halted() 谓词 + 阶梯留痕 FaultStep），不再接受任何输入
//!        （present/gate_check/在线注入/文案入账一律 E_HALT_LOCKED
//!        0x2735），且「三重 fault 保持」断言：任何后续调用都保持 Halt
//!        不变（halt_maintain_assertion 逐轮核）。
//!   J1-e 回归可重放：每次注入 → 结论 → 文案指纹入回归账（RegBook），
//!        replay() 以原材料（tag 码 + 裁决序号）确定性重建文案并核对
//!        指纹，不符即 E_REPLAY_MISMATCH（0x273B）。
//!
//! ── 深度子系统清单（本文件全手工定制，零注水） ───────────────────────
//!   S1 RequiredTag 六项必需 tag 闭集：tag 码/名/最低 revision/闸内校验
//!      点位/载荷对齐五列 const 表 + const 期合理性核（码严格升序、名非空、
//!      revision 档位、对齐 2 的幂）。
//!   S2 RequiredRespGate 必需响应闸：六槽 PresentRecord 在场登记 + 逐项
//!      在场校验（缺谁显性）+ 覆盖率 ppm + 重复呈现冲突/越档/越上限三拒。
//!   S3 PanicMessage PANIC 文案：三要素（tag 名 + 错误码 + 位置）确定性
//!      生成 + FNV-1a 64 指纹 + 三要素逐项核验（缺谁显性）。
//!   S4 PanicPathSim panic 路径模拟 + TripleFaultSemantics：fault 阶梯
//!      Normal → Fault1 → Fault2 → Halt 显性建模，每步 FaultStep 留痕；
//!      挂死后任何输入拒收（E_HALT_LOCKED）且挂死保持断言逐轮核。
//!   S5 MessageAccount 文案入账：PANIC 文案内存账（不落盘——账面可查），
//!      入账即验三要素；按序号/tag/指纹三路查询。
//!   S6 RegBook 回归账：每注入一行（注入 tag/检出结论/文案指纹/原材料），
//!      检出率 ppm 核算（10000 ppm = 100%）+ replay 确定性重放核对。
//!   S7 Ledger 事件账（封板制）+ health() 体检单 + 账级 FNV 指纹。
//!   S8 PanicRegAccount 总账 facade：注入面三路（tag 缺失注入（六 tag
//!      轮换——检出率 100% + 文案含 tag 名逐例验证）/ 挂死后输入注入 /
//!      文案无 tag 注入各检出）+ 双跑确定性对拍 + 六节回归报告程序化生成。
//!
//! ── 相邻任务对接 ─────────────────────────────────────────────────────
//!   F0725 响应树兼容账：本闸六 tag 前三项（memory_map/framebuffer/
//!     kernel_address）与其 RespKind 三臂同码对齐（1/2/3）；其冻结表
//!     100% 核对是本闸「在场」的前置，本账 VerifyRejected 类事件是其
//!     响应面子集的 panic 面收口。
//!   F0724 升级触发回归：本账六 tag 轮换回归是其 300+ 断言的 panic 面
//!     子集（检出率 100% 口径同源 PPM_SCALE=10_000）。
//!   F0727 升级前后 A/B 零差异对照：本账双跑对拍（dual_run_check）同构。
//!   F0730 升级回滚预案：本账 triple fault 漂习通道（deliberate_triple_
//!     fault）是其回退路径演练的 panic 面前置。
//!
//! ── 通用纪律 ─────────────────────────────────────────────────────────
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert! 允许）。
//!   可失败 API 全走 Result<T, PanicRegError>；溢出显性 saturating/wrapping。
//!   >64KiB 结构体禁上栈（本文件最大结构为账级 PanicRegAccount，内含
//!   Vec 堆存储，栈上外壳 < 200B；单行 LedgerEvent/RegRow/PanicMessage
//!   均为堆 String/Vec 载体，安全）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引）；
//!   含 Vec 字段的结构体禁 derive Copy。
//!
//! 零浮点、零 unwrap、零 expect、零 panic 路径（#[cfg(test)] 测试域除外）。

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

// ── 诊断码段（0x2730~0x273F 独占 0x10；先验空闲：kernel/varix/src 全树
//    grep "0x273" 于真实 .rs 源零命中。相邻段实况：0x2700=svstar2/ves501、
//    0x2720=svstar2/ves502（SEG_BASE 注明「续表 idx502」）、0x2740=
//    unxstar/unx3181_udomainfinal、0x2760=ves503、0x2780=segs_all 续表、
//    0x27A0=ves504、0x27E0=ves505——任务书预核「0x2720 空闲」与实况不符
//    （ves502 已占），按纪律依次试段：0x2730 段全空闲，故取 0x2730；0x2740
//    段亦被 unx3181 占用，不再后移）──
pub const SEG_BASE: u16 = 0x2730;

const E_SEALED: u16 = SEG_BASE + 0x0; // 0x2730 账已封板仍写
const E_TAG_UNKNOWN: u16 = SEG_BASE + 0x1; // 0x2731 tag 码越六项闭集
const E_TAG_MISSING: u16 = SEG_BASE + 0x2; // 0x2732 必需响应缺失（panic 路径入口）
const E_REVISION_BAD: u16 = SEG_BASE + 0x3; // 0x2733 tag revision 越档
const E_PRESENT_CONFLICT: u16 = SEG_BASE + 0x4; // 0x2734 同 tag 重复呈现参数漂移
const E_HALT_LOCKED: u16 = SEG_BASE + 0x5; // 0x2735 挂死态锁定：任何输入不再受理
const E_MSG_NO_TAG: u16 = SEG_BASE + 0x6; // 0x2736 PANIC 文案缺失 tag 名（三要素破）
const E_MSG_NO_CODE: u16 = SEG_BASE + 0x7; // 0x2737 PANIC 文案缺失错误码（三要素破）
const E_MSG_NO_SITE: u16 = SEG_BASE + 0x8; // 0x2738 PANIC 文案缺失位置（三要素破）
const E_FAULT_SEQ: u16 = SEG_BASE + 0x9; // 0x2739 fault 阶梯跳变非法/挂死语义破
const E_NOT_HALTED: u16 = SEG_BASE + 0xA; // 0x273A 挂死保持断言在非挂死态执行
const E_REPLAY_MISMATCH: u16 = SEG_BASE + 0xB; // 0x273B 回归账重放指纹不符
const E_DUAL_RUN_DRIFT: u16 = SEG_BASE + 0xC; // 0x273C 双跑确定性对拍漂移
const E_RATE_NOT_FULL: u16 = SEG_BASE + 0xD; // 0x273D 检出率不足 100%
const E_CAPACITY: u16 = SEG_BASE + 0xE; // 0x273E 事件账/文案账/回归账容量满
const E_PAYLOAD_BAD: u16 = SEG_BASE + 0xF; // 0x273F 呈现载荷长度/条目数越上限

/// 诊断码 → 中文可操作诊断（独占段 0x2730~0x273F 内解释，段外一律未知）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：再写属误用，请克隆新账或走只读通道",
        E_TAG_UNKNOWN => "tag 码越六项闭集：合法仅 memory_map/framebuffer/kernel_address/hhdm/modules/firmware（1..=6）",
        E_TAG_MISSING => "必需响应缺失：panic 路径入口，请核对引导器六项必需 tag 呈现",
        E_REVISION_BAD => "tag revision 越档：需 ≥ 表定最低 revision 且 ≤ 账面上限 64",
        E_PRESENT_CONFLICT => "重复呈现参数漂移：同 tag 再呈现须与首发参数一致",
        E_HALT_LOCKED => "挂死态锁定：三重 fault 后不再接受任何输入，请重建新模拟器",
        E_MSG_NO_TAG => "PANIC 文案缺失 tag 名：三要素破，可观测性 100% 不成立",
        E_MSG_NO_CODE => "PANIC 文案缺失错误码：三要素破，请补 0x2732 段诊断码",
        E_MSG_NO_SITE => "PANIC 文案缺失位置：三要素破，请补闸内校验点位锚",
        E_FAULT_SEQ => "fault 阶梯跳变非法：只许 Normal→1→2→Halt 单步，或挂死后仍可写",
        E_NOT_HALTED => "挂死保持断言前置破：该断言只在三重 fault 挂死后可执行",
        E_REPLAY_MISMATCH => "回归账重放指纹不符：同原材料重建文案指纹漂移，确定性破",
        E_DUAL_RUN_DRIFT => "双跑确定性对拍漂移：同输入两跑指纹不等，确定性破",
        E_RATE_NOT_FULL => "检出率不足 100%：轮换注入有漏检，六 tag 必须全检出",
        E_CAPACITY => "容量满：事件账 512/文案账 256/回归账 128 封顶，请封板归档",
        E_PAYLOAD_BAD => "呈现载荷越上限：payload_len ≤ 4096 且 entries ≤ 4096",
        _ => "未知诊断码（0x2730 段外或保留位）",
    }
}

// ── 常量（来源逐条注明） ─────────────────────────────────────────────
/// 六项必需 tag 数（J1 原文「memory map/framebuffer/kernel address 等六项
/// ——与 F0725 响应面对齐声明」：F0725 三臂 + Limine 必需响应
/// hhdm/modules/firmware）。
pub const TAG_COUNT: usize = 6;
/// tag revision 账面上限（与 F0725 REV_CAP 同源：Limine 各响应 revision
/// 实际为个位数，账面 64 防漂移）。
pub const REV_CAP: u64 = 64;
/// 呈现载荷长度账面上限（与 F0725 PAYLOAD_CAP 同源：响应头 4 KiB 口径）。
pub const PAYLOAD_CAP: u32 = 4096;
/// 呈现条目数账面上限（与 F0725 COUNT_CAP 同源）。
pub const ENTRY_COUNT_CAP: u32 = 4096;
/// 检出率 ppm 基数：10000 ppm = 100%（J1「可观测 100%」的整数口径，
/// 与 F0725/F0724 PPM_SCALE 同源）。
pub const PPM_SCALE: u64 = 10_000;
/// 六槽全在场位掩码（低六位全 1）。
pub const REQUIRED_MASK_ALL: u8 = 0x3F;
/// 位置锚根（本模块在闸内的路径前缀）。
pub const SITE_ROOT: &str = "unx0726";
/// PANIC 文案横幅关键字（J1 原文「PANIC 文案」字面口径）。
pub const PANIC_BANNER: &str = "PANIC";
/// 位置锚基：F0726 任务号低 24 位口径（0xF0726 << 8 = 0xF07260 起，
/// | tag 码 1..=6 → 0xF07261..0xF07266）。
pub const ANCHOR_BASE: u32 = 0xF072_60;
/// 事件账容量封顶（与 F0725 Ledger::CAP 同源：512 行）。
pub const LEDGER_CAP: usize = 512;
/// 文案账容量封顶（每文案 ≤ 256B 量级 × 256 行 ≈ 64KiB 上界内堆存）。
pub const MSG_CAP: usize = 256;
/// 回归账容量封顶（六 tag 轮换 × 双跑 + 手工注入余量）。
pub const REG_CAP: usize = 128;
/// 挂死保持断言默认轮数（账面口径：5 轮逐核）。
pub const HALT_PROBE_ROUNDS: u32 = 5;
/// fault 阶梯原始码：Normal（未入 panic 路径）。
pub const STAGE_NORMAL: u8 = 0;
/// fault 阶梯原始码：Fault1（首次 panic 裁决）。
pub const STAGE_FAULT1: u8 = 1;
/// fault 阶梯原始码：Fault2（double fault）。
pub const STAGE_FAULT2: u8 = 2;
/// fault 阶梯原始码：Halt（triple fault 挂死态）。
pub const STAGE_HALT: u8 = 3;

// ── S1：RequiredTag 六项必需 tag 闭集（判据 J1-a） ───────────────────
/// 必需 tag 五列 const 行：码/名/最低 revision/闸内校验点位/载荷对齐。
///
/// 前三项与 F0725 RespKind 三臂同码对齐（MemoryMap=1/Framebuffer=2/
/// KernelAddress=3，见 unx0725_resptree.rs RespKind::code）；后三项为
/// Limine Boot Protocol 必需响应（hhdm 高半区直接映射/modules 模块清单/
/// firmware RSDP 固件表，limine.h 必要响应口径）。
#[derive(Clone, Copy, Debug)]
pub struct RequiredTag {
    /// 闭集原始码（1..=6）。
    pub code: u8,
    /// tag 名（PANIC 文案第一要素，可观测载体）。
    pub name: &'static str,
    /// 最低 revision（低于即越档 E_REVISION_BAD）。
    pub min_rev: u64,
    /// 闸内校验点位（PANIC 文案第三要素的 stage 部分）。
    pub site: &'static str,
    /// 呈现载荷对齐（2 的幂；hhdm/modules 指针域 8 字节口径）。
    pub align: u64,
}

/// 必需 tag 1：memory map（与 F0725 RespKind::MemoryMap 同码 1）。
pub const TAG_MEMORY_MAP: RequiredTag = RequiredTag {
    code: 1,
    name: "memory_map",
    min_rev: 1,
    site: "gate/mm",
    align: 8,
};
/// 必需 tag 2：framebuffer（与 F0725 RespKind::Framebuffer 同码 2）。
pub const TAG_FRAMEBUFFER: RequiredTag = RequiredTag {
    code: 2,
    name: "framebuffer",
    min_rev: 1,
    site: "gate/fb",
    align: 8,
};
/// 必需 tag 3：kernel address（与 F0725 RespKind::KernelAddress 同码 3）。
pub const TAG_KERNEL_ADDRESS: RequiredTag = RequiredTag {
    code: 3,
    name: "kernel_address",
    min_rev: 1,
    site: "gate/ka",
    align: 8,
};
/// 必需 tag 4：hhdm（Limine higher-half direct map 响应）。
pub const TAG_HHDM: RequiredTag = RequiredTag {
    code: 4,
    name: "hhdm",
    min_rev: 1,
    site: "gate/hhdm",
    align: 8,
};
/// 必需 tag 5：modules（Limine module 响应——模块清单）。
pub const TAG_MODULES: RequiredTag = RequiredTag {
    code: 5,
    name: "modules",
    min_rev: 1,
    site: "gate/mod",
    align: 8,
};
/// 必需 tag 6：firmware（Limine firmware 响应——RSDP/固件表）。
pub const TAG_FIRMWARE: RequiredTag = RequiredTag {
    code: 6,
    name: "firmware",
    min_rev: 1,
    site: "gate/fw",
    align: 8,
};

/// 六项必需 tag 清单（闭集；行序即 idx 0..=5，码严格 1..=6 升序）。
pub const REQUIRED_TAGS: [RequiredTag; TAG_COUNT] = [
    TAG_MEMORY_MAP,
    TAG_FRAMEBUFFER,
    TAG_KERNEL_ADDRESS,
    TAG_HHDM,
    TAG_MODULES,
    TAG_FIRMWARE,
];

/// 码 → tag（闭集外 None）。const 期 while+索引，零 slice::get。
pub const fn tag_from_code(code: u8) -> Option<RequiredTag> {
    let mut i = 0usize;
    while i < TAG_COUNT {
        if REQUIRED_TAGS[i].code == code {
            return Some(REQUIRED_TAGS[i]);
        }
        i += 1;
    }
    None
}

/// idx（0..=5）→ tag（越界 None）。
pub const fn tag_from_index(idx: usize) -> Option<RequiredTag> {
    if idx < TAG_COUNT {
        Some(REQUIRED_TAGS[idx])
    } else {
        None
    }
}

/// tag → 位置锚（ANCHOR_BASE | code → 0xF07261..0xF07266）。
pub const fn tag_anchor(tag: RequiredTag) -> u32 {
    ANCHOR_BASE | (tag.code as u32)
}

/// tag → 完整位置文本（SITE_ROOT/site#锚，PANIC 文案第三要素）。
pub fn tag_site_text(tag: RequiredTag) -> String {
    format!("{}/{}#0x{:06X}", SITE_ROOT, tag.site, tag_anchor(tag))
}

/// 六项清单 const 期合理性核：码严格升序 1..=6、名非空、点位非空、
/// revision 档 1..=REV_CAP、对齐 2 的幂非零。
pub const fn required_tags_sane() -> bool {
    let mut i = 0usize;
    while i < TAG_COUNT {
        let t = REQUIRED_TAGS[i];
        // 码严格升序（第 i 行码必为 i+1，杜绝重码/乱序）。
        if t.code != (i as u8) + 1 {
            return false;
        }
        // 名与点位非空（文案第一/第三要素的载体不许空串）。
        if t.name.is_empty() {
            return false;
        }
        if t.site.is_empty() {
            return false;
        }
        // revision 档：1 ≤ min_rev ≤ REV_CAP。
        if t.min_rev == 0 || t.min_rev > REV_CAP {
            return false;
        }
        // 对齐：非零 2 的幂。
        if t.align == 0 || (t.align & (t.align - 1)) != 0 {
            return false;
        }
        i += 1;
    }
    true
}

// 编译期封核（零运行期 assert 的口径）。
const _: () = assert!(SEG_BASE == 0x2730);
const _: () = assert!(TAG_COUNT == 6);
const _: () = assert!(REQUIRED_MASK_ALL == 0x3F);
const _: () = assert!(ANCHOR_BASE == 0xF072_60);
const _: () = assert!(LEDGER_CAP == 512);
const _: () = assert!(MSG_CAP == 256);
const _: () = assert!(REG_CAP == 128);
const _: () = assert!(required_tags_sane());

// ── 错误面（可失败 API 全走 Result<T, PanicRegError>） ───────────────
/// panic 路径回归错误面。每变体自带段内诊断码 + 上下文，
/// detail() 出中文可操作诊断（不落 panic 路径——错误码化裁决）。
#[derive(Clone, Debug)]
pub enum PanicRegError {
    /// 账已封板仍写。
    Sealed { code: u16 },
    /// tag 码越六项闭集。
    TagUnknown { code: u16, raw: u8 },
    /// 必需响应缺失（panic 路径入口；missing 为缺席 tag 码升序列表）。
    TagMissing { code: u16, missing: Vec<u8> },
    /// tag revision 越档。
    RevisionBad { code: u16, tag: u8, observed: u64 },
    /// 同 tag 重复呈现参数漂移。
    PresentConflict { code: u16, tag: u8, first_rev: u64, new_rev: u64 },
    /// 挂死态锁定：任何输入不再受理。
    HaltLocked { code: u16, attempts: u32 },
    /// PANIC 文案缺失 tag 名。
    MsgNoTag { code: u16, text: String },
    /// PANIC 文案缺失错误码。
    MsgNoCode { code: u16, text: String },
    /// PANIC 文案缺失位置。
    MsgNoSite { code: u16, text: String },
    /// fault 阶梯跳变非法/挂死语义破。
    FaultSeq { code: u16, from: u8, to: u8 },
    /// 挂死保持断言在非挂死态执行。
    NotHalted { code: u16, stage: u8 },
    /// 回归账重放指纹不符。
    ReplayMismatch { code: u16, seq: u32, stored: u64, rebuilt: u64 },
    /// 双跑确定性对拍漂移。
    DualRunDrift { code: u16, fp_a: u64, fp_b: u64 },
    /// 检出率不足 100%。
    RateNotFull { code: u16, detected: u64, total: u64 },
    /// 容量满（事件账/文案账/回归账各自封顶）。
    Capacity { code: u16, which: &'static str, cap: usize },
    /// 呈现载荷长度/条目数越上限。
    PayloadBad { code: u16, tag: u8, payload_len: u32, entries: u32 },
}

impl PanicRegError {
    /// 诊断码（段内 0x2730~0x273F）。
    pub fn diag_code(&self) -> u16 {
        match self {
            PanicRegError::Sealed { code }
            | PanicRegError::TagUnknown { code, .. }
            | PanicRegError::TagMissing { code, .. }
            | PanicRegError::RevisionBad { code, .. }
            | PanicRegError::PresentConflict { code, .. }
            | PanicRegError::HaltLocked { code, .. }
            | PanicRegError::MsgNoTag { code, .. }
            | PanicRegError::MsgNoCode { code, .. }
            | PanicRegError::MsgNoSite { code, .. }
            | PanicRegError::FaultSeq { code, .. }
            | PanicRegError::NotHalted { code, .. }
            | PanicRegError::ReplayMismatch { code, .. }
            | PanicRegError::DualRunDrift { code, .. }
            | PanicRegError::RateNotFull { code, .. }
            | PanicRegError::Capacity { code, .. }
            | PanicRegError::PayloadBad { code, .. } => *code,
        }
    }

    /// 中文可操作诊断（直接透传段内解释）。
    pub fn detail(&self) -> &'static str {
        diag_detail(self.diag_code())
    }
}

// ── FNV-1a 64（指纹基元，const 期 while+索引） ───────────────────────
/// FNV-1a 64 位哈希（偏移基 0xCBF29CE484222325，素数 0x100000001B3；
/// FNV 参考实现口径，与 F0725 fnv1a64 同源）。确定性对拍与账级指纹共用。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

// ── S2：RequiredRespGate 必需响应闸（判据 J1-a/J1-b） ────────────────
/// 单 tag 在场登记行（全部标量，Copy 安全——零 Vec 字段）。
#[derive(Clone, Copy, Debug)]
pub struct PresentRecord {
    /// 是否已呈现。
    pub present: bool,
    /// 呈现的 revision（首发口径；重复呈现须一致）。
    pub rev: u64,
    /// 呈现载荷长度（字节）。
    pub payload_len: u32,
    /// 呈现条目数。
    pub entries: u32,
}

impl PresentRecord {
    /// 空槽（未呈现）。
    pub const fn absent() -> PresentRecord {
        PresentRecord { present: false, rev: 0, payload_len: 0, entries: 0 }
    }
}

/// 闸校验结论（缺谁显性——数据面，不含错误路径）。
#[derive(Clone, Debug)]
pub struct GateCheck {
    /// 六项是否全在场。
    pub complete: bool,
    /// 在场位掩码（低六位）。
    pub present_mask: u8,
    /// 缺席 tag 码（升序）。
    pub missing_codes: Vec<u8>,
    /// 缺席 tag 名（与码同序——文案第一要素的候选清单）。
    pub missing_names: Vec<String>,
    /// 覆盖率 ppm（在场数 × 10000 / 6）。
    pub coverage_ppm: u64,
}

impl GateCheck {
    /// 缺席项数。
    pub fn missing_len(&self) -> usize {
        self.missing_codes.len()
    }
}

/// 必需响应闸：六槽在场登记 + 逐项在场校验。
///
/// 缺失不在这里报错——闸只给显性结论；「缺失即进入 panic 路径」的
/// 错误码化裁决在账户层（gate_check → PanicPathSim.enter_panic_path）。
#[derive(Clone, Debug)]
pub struct RequiredRespGate {
    slots: [PresentRecord; TAG_COUNT],
    /// 呈现次数（含幂等重呈；账面统计）。
    presents: u32,
    /// 闸检次数。
    checks: u32,
}

impl RequiredRespGate {
    /// 新闸（六槽全空）。
    pub fn new() -> RequiredRespGate {
        RequiredRespGate {
            slots: [
                PresentRecord::absent(),
                PresentRecord::absent(),
                PresentRecord::absent(),
                PresentRecord::absent(),
                PresentRecord::absent(),
                PresentRecord::absent(),
            ],
            presents: 0,
            checks: 0,
        }
    }

    /// 呈现一项必需 tag（在场登记）。
    ///
    /// 三拒：码越闭集（E_TAG_UNKNOWN）/ revision 越档（E_REVISION_BAD，
    /// 含低于表定 min_rev 与高于 REV_CAP）/ 载荷越上限（E_PAYLOAD_BAD）；
    /// 重复呈现参数漂移拒（E_PRESENT_CONFLICT）；同参幂等重呈放行。
    pub fn present(
        &mut self,
        tag_code: u8,
        rev: u64,
        payload_len: u32,
        entries: u32,
    ) -> Result<(), PanicRegError> {
        let tag = match tag_from_code(tag_code) {
            Some(t) => t,
            None => {
                return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: tag_code });
            }
        };
        if rev < tag.min_rev || rev > REV_CAP {
            return Err(PanicRegError::RevisionBad {
                code: E_REVISION_BAD,
                tag: tag_code,
                observed: rev,
            });
        }
        if payload_len > PAYLOAD_CAP || entries > ENTRY_COUNT_CAP {
            return Err(PanicRegError::PayloadBad {
                code: E_PAYLOAD_BAD,
                tag: tag_code,
                payload_len,
                entries,
            });
        }
        let idx = (tag_code - 1) as usize;
        let slot = self.slots[idx];
        if slot.present {
            // 幂等重呈：同参放行（升级复跑口径）；参数漂移即冲突。
            if slot.rev != rev {
                return Err(PanicRegError::PresentConflict {
                    code: E_PRESENT_CONFLICT,
                    tag: tag_code,
                    first_rev: slot.rev,
                    new_rev: rev,
                });
            }
            self.presents = self.presents.saturating_add(1);
            return Ok(());
        }
        self.slots[idx] = PresentRecord { present: true, rev, payload_len, entries };
        self.presents = self.presents.saturating_add(1);
        Ok(())
    }

    /// 注入面专用：把一项 tag 标记回缺席（模拟必需响应缺失注入）。
    /// 码越闭集拒（E_TAG_UNKNOWN）。已在缺席态则幂等。
    pub fn mark_missing(&mut self, tag_code: u8) -> Result<(), PanicRegError> {
        let tag = match tag_from_code(tag_code) {
            Some(t) => t,
            None => {
                return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: tag_code });
            }
        };
        let idx = (tag.code - 1) as usize;
        self.slots[idx] = PresentRecord::absent();
        Ok(())
    }

    /// 注入面专用：强制恢复一项 tag 在场（注入完成即恢复——瞬时扰动口径；
    /// 不走 present 冲突关，参数按注入前快照原样回写）。
    pub fn mark_present(
        &mut self,
        tag_code: u8,
        rev: u64,
        payload_len: u32,
        entries: u32,
    ) -> Result<(), PanicRegError> {
        let tag = match tag_from_code(tag_code) {
            Some(t) => t,
            None => {
                return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: tag_code });
            }
        };
        let idx = (tag.code - 1) as usize;
        self.slots[idx] = PresentRecord { present: true, rev, payload_len, entries };
        Ok(())
    }

    /// 逐项在场校验（缺谁显性；不报错——结论数据面）。
    pub fn check(&mut self) -> GateCheck {
        self.checks = self.checks.saturating_add(1);
        let mut present_mask: u8 = 0;
        let mut present_count: u64 = 0;
        let mut missing_codes: Vec<u8> = Vec::new();
        let mut missing_names: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < TAG_COUNT {
            if self.slots[i].present {
                present_mask |= 1u8 << i;
                present_count += 1;
            } else {
                missing_codes.push(REQUIRED_TAGS[i].code);
                missing_names.push(REQUIRED_TAGS[i].name.to_string());
            }
            i += 1;
        }
        GateCheck {
            complete: missing_codes.is_empty(),
            present_mask,
            missing_codes,
            missing_names,
            coverage_ppm: present_count * PPM_SCALE / (TAG_COUNT as u64),
        }
    }

    /// 只读在场掩码（不推进 checks 计数）。
    pub fn present_mask(&self) -> u8 {
        let mut m: u8 = 0;
        let mut i = 0usize;
        while i < TAG_COUNT {
            if self.slots[i].present {
                m |= 1u8 << i;
            }
            i += 1;
        }
        m
    }

    /// 六项是否全在场（只读）。
    pub fn is_complete(&self) -> bool {
        self.present_mask() == REQUIRED_MASK_ALL
    }

    /// 呈现次数（账面统计）。
    pub fn presents(&self) -> u32 {
        self.presents
    }

    /// 闸检次数（账面统计）。
    pub fn checks(&self) -> u32 {
        self.checks
    }

    /// 单槽只读查询（idx 0..=5；越界 None——不落 panic）。
    pub fn slot(&self, idx: usize) -> Option<PresentRecord> {
        if idx < TAG_COUNT {
            Some(self.slots[idx])
        } else {
            None
        }
    }
}

// ── S3：PanicMessage PANIC 文案（判据 J1-c 三要素 100%） ─────────────
/// 三要素核验结论。
#[derive(Clone, Copy, Debug)]
pub struct ElementCheck {
    /// 文案含缺失 tag 名。
    pub has_tag: bool,
    /// 文案含错误码（0x2732 段十六进制字面）。
    pub has_code: bool,
    /// 文案含位置（SITE_ROOT/site#锚）。
    pub has_site: bool,
}

impl ElementCheck {
    /// 三要素齐。
    pub fn all_present(&self) -> bool {
        self.has_tag && self.has_code && self.has_site
    }
}

/// PANIC 文案（内存对象——不落盘，入账后账面可查）。
#[derive(Clone, Debug)]
pub struct PanicMessage {
    /// 完整文案（含三要素）。
    pub text: String,
    /// 缺失 tag 码。
    pub tag_code: u8,
    /// 缺失 tag 名（第一要素）。
    pub tag_name: String,
    /// 诊断码（第二要素，段内 0x2732）。
    pub diag_code: u16,
    /// 位置文本（第三要素）。
    pub site_text: String,
    /// 裁决序号（第几号 panic 裁决——重放原材料之一）。
    pub ordinal: u32,
    /// 文案指纹（FNV-1a 64 over text）。
    pub fingerprint: u64,
}

/// 确定性生成一份 PANIC 文案（同 tag + 同序号 → 同文案同指纹）。
///
/// 文案三要素固定槽位：tag 名（tag='…'）、错误码（diag=0x…）、
/// 位置（site=…#0x…）；横幅 PANIC[unx-f0726] + 缺失语义句。
pub fn build_panic_message(tag: RequiredTag, ordinal: u32) -> PanicMessage {
    let site_text = tag_site_text(tag);
    let text = format!(
        "{}[unx-f0726]: required response missing: tag='{}' (code {}) diag=0x{:04X} site={} ordinal={}",
        PANIC_BANNER,
        tag.name,
        tag.code,
        E_TAG_MISSING,
        site_text,
        ordinal
    );
    PanicMessage {
        fingerprint: fnv1a64(text.as_bytes()),
        text,
        tag_code: tag.code,
        tag_name: tag.name.to_string(),
        diag_code: E_TAG_MISSING,
        site_text,
        ordinal,
    }
}

/// 三要素逐项核验（文案文本 vs 期望要素；缺谁显性——错误码化检出）。
///
/// 返回 Err 即注入检出：缺 tag 名 E_MSG_NO_TAG / 缺错误码 E_MSG_NO_CODE /
/// 缺位置 E_MSG_NO_SITE（判据 J1-c「文案无 tag 注入检出」的落地面）。
pub fn verify_three_elements(
    text: &str,
    tag_name: &str,
    diag_code: u16,
    site_text: &str,
) -> Result<ElementCheck, PanicRegError> {
    let has_tag = text.contains(tag_name);
    let has_code = text.contains(&format!("0x{:04X}", diag_code));
    let has_site = text.contains(site_text);
    let check = ElementCheck { has_tag, has_code, has_site };
    if !has_tag {
        return Err(PanicRegError::MsgNoTag { code: E_MSG_NO_TAG, text: text.to_string() });
    }
    if !has_code {
        return Err(PanicRegError::MsgNoCode { code: E_MSG_NO_CODE, text: text.to_string() });
    }
    if !has_site {
        return Err(PanicRegError::MsgNoSite { code: E_MSG_NO_SITE, text: text.to_string() });
    }
    Ok(check)
}

// ── S4：PanicPathSim + TripleFaultSemantics（判据 J1-d） ─────────────
/// fault 阶梯（四态闭集；原始码 0..=3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultStage {
    /// 正常态（未入 panic 路径）。
    Normal,
    /// 首次 panic 裁决后。
    Fault1,
    /// double fault（panic 路径二次进入）。
    Fault2,
    /// triple fault 挂死态（不可恢复）。
    Halt,
}

impl FaultStage {
    /// 原始码（Normal=0/Fault1=1/Fault2=2/Halt=3）。
    pub const fn code(&self) -> u8 {
        match self {
            FaultStage::Normal => STAGE_NORMAL,
            FaultStage::Fault1 => STAGE_FAULT1,
            FaultStage::Fault2 => STAGE_FAULT2,
            FaultStage::Halt => STAGE_HALT,
        }
    }

    /// 码 → 阶梯（越闭集 None）。
    pub const fn from_code(c: u8) -> Option<FaultStage> {
        match c {
            STAGE_NORMAL => Some(FaultStage::Normal),
            STAGE_FAULT1 => Some(FaultStage::Fault1),
            STAGE_FAULT2 => Some(FaultStage::Fault2),
            STAGE_HALT => Some(FaultStage::Halt),
            _ => None,
        }
    }

    /// 中文名（报告用）。
    pub const fn cn(&self) -> &'static str {
        match self {
            FaultStage::Normal => "正常",
            FaultStage::Fault1 => "一重 fault",
            FaultStage::Fault2 => "二重 fault",
            FaultStage::Halt => "三重 fault 挂死",
        }
    }

    /// 是否挂死态。
    pub const fn is_halt(&self) -> bool {
        matches!(self, FaultStage::Halt)
    }
}

/// 阶梯转移留痕种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// 首次 panic 裁决（Normal → Fault1）。
    PanicVerdict,
    /// double fault（Fault1 → Fault2）。
    DoubleFault,
    /// triple fault 挂死（Fault2 → Halt）。
    TripleFaultHalt,
    /// 挂死后输入被拒（Halt → Halt，不变式保持）。
    RejectedHalted,
}

impl StepKind {
    /// 事件指纹用原始码。
    pub const fn code(&self) -> u8 {
        match self {
            StepKind::PanicVerdict => 1,
            StepKind::DoubleFault => 2,
            StepKind::TripleFaultHalt => 3,
            StepKind::RejectedHalted => 4,
        }
    }

    /// 中文名（报告用）。
    pub const fn cn(&self) -> &'static str {
        match self {
            StepKind::PanicVerdict => "panic 裁决",
            StepKind::DoubleFault => "二重 fault",
            StepKind::TripleFaultHalt => "三重 fault 挂死",
            StepKind::RejectedHalted => "挂死后输入拒收",
        }
    }
}

/// 单步阶梯留痕。
#[derive(Clone, Debug)]
pub struct FaultStep {
    /// 留痕序号（模拟器内单调）。
    pub seq: u32,
    /// 转移前阶梯码。
    pub from: u8,
    /// 转移后阶梯码。
    pub to: u8,
    /// 转移种类。
    pub kind: StepKind,
    /// 触发 tag 码（拒收步为尝试输入的 tag 码）。
    pub tag_code: u8,
}

/// panic 裁决回执（enter_panic_path 的 Ok 载荷）。
#[derive(Clone, Debug)]
pub struct PanicEntry {
    /// 裁决序号。
    pub ordinal: u32,
    /// 裁决后阶梯。
    pub stage_after: FaultStage,
    /// 本步转移种类。
    pub step_kind: StepKind,
    /// 缺失 tag 码。
    pub tag_code: u8,
    /// 缺失 tag 名。
    pub tag_name: String,
    /// 本裁决文案指纹（文案本体由账户层入账）。
    pub msg_fingerprint: u64,
}

/// panic 路径模拟器（错误码化裁决——不真触发 panic）。
///
/// TripleFaultSemantics 落地：进入即裁决（Fault1），再进 double（Fault2），
/// 再进 triple → Halt；挂死后任何进入一律拒收（E_HALT_LOCKED），
/// 阶梯保持 Halt 不变（「三重 fault 保持」）。
#[derive(Clone, Debug)]
pub struct PanicPathSim {
    stage: FaultStage,
    /// 留痕单调序号。
    step_seq: u32,
    /// panic 裁决计数（Fault1/2/Halt 三类进入各计一次）。
    verdicts: u32,
    /// double fault 计数。
    double_faults: u32,
    /// triple fault 挂死计数（至多 1——挂死后不再接受输入）。
    triple_faults: u32,
    /// 挂死后输入拒收计数（挂死保持证据）。
    halt_maintained: u32,
    trace: Vec<FaultStep>,
}

impl PanicPathSim {
    /// 新模拟器（Normal 态）。
    pub fn new() -> PanicPathSim {
        PanicPathSim {
            stage: FaultStage::Normal,
            step_seq: 0,
            verdicts: 0,
            double_faults: 0,
            triple_faults: 0,
            halt_maintained: 0,
            trace: Vec::new(),
        }
    }

    /// 当前阶梯。
    pub fn stage(&self) -> FaultStage {
        self.stage
    }

    /// 当前阶梯原始码。
    pub fn stage_code(&self) -> u8 {
        self.stage.code()
    }

    /// 是否挂死态。
    pub fn halted(&self) -> bool {
        self.stage.is_halt()
    }

    /// 裁决计数。
    pub fn verdicts(&self) -> u32 {
        self.verdicts
    }

    /// double fault 计数。
    pub fn double_faults(&self) -> u32 {
        self.double_faults
    }

    /// triple fault 挂死计数。
    pub fn triple_faults(&self) -> u32 {
        self.triple_faults
    }

    /// 挂死后输入拒收计数。
    pub fn halt_maintained(&self) -> u32 {
        self.halt_maintained
    }

    /// 账户级挂死拒收记账（in-machine 通道被 check_open_inmachine 拒时
    /// 调用——与 enter_panic_path 的拒收共用 halt_maintained 计数与
    /// RejectedHalted 留痕，tag_code 记 0 哨兵）。
    pub fn note_halted_input(&mut self) {
        if self.stage.is_halt() {
            self.halt_maintained = self.halt_maintained.saturating_add(1);
            self.push_step(FaultStage::Halt, FaultStage::Halt, StepKind::RejectedHalted, 0);
        }
    }

    /// 阶梯留痕（只读切片）。
    pub fn trace(&self) -> &[FaultStep] {
        &self.trace
    }

    fn push_step(&mut self, from: FaultStage, to: FaultStage, kind: StepKind, tag_code: u8) {
        self.step_seq = self.step_seq.saturating_add(1);
        self.trace.push(FaultStep {
            seq: self.step_seq,
            from: from.code(),
            to: to.code(),
            kind,
            tag_code,
        });
    }

    /// 进入 panic 路径一次（错误码化裁决）。
    ///
    /// - Normal → Fault1：首发裁决（StepKind::PanicVerdict）；
    /// - Fault1 → Fault2：double fault；
    /// - Fault2 → Halt：triple fault 挂死；
    /// - Halt → 拒收 E_HALT_LOCKED（挂死后不再接受任何输入；
    ///   halt_maintained 递增——「三重 fault 保持」证据）。
    pub fn enter_panic_path(&mut self, tag: RequiredTag) -> Result<PanicEntry, PanicRegError> {
        let from = self.stage;
        if from.is_halt() {
            // 挂死保持：输入拒收 + 阶梯不变（留痕 RejectedHalted）。
            self.halt_maintained = self.halt_maintained.saturating_add(1);
            self.push_step(from, FaultStage::Halt, StepKind::RejectedHalted, tag.code);
            return Err(PanicRegError::HaltLocked {
                code: E_HALT_LOCKED,
                attempts: self.halt_maintained,
            });
        }
        let ordinal = self.verdicts.saturating_add(1);
        let msg = build_panic_message(tag, ordinal);
        let to = match from {
            FaultStage::Normal => {
                let kind = StepKind::PanicVerdict;
                self.push_step(from, FaultStage::Fault1, kind, tag.code);
                FaultStage::Fault1
            }
            FaultStage::Fault1 => {
                self.double_faults = self.double_faults.saturating_add(1);
                self.push_step(from, FaultStage::Fault2, StepKind::DoubleFault, tag.code);
                FaultStage::Fault2
            }
            FaultStage::Fault2 => {
                self.triple_faults = self.triple_faults.saturating_add(1);
                self.push_step(from, FaultStage::Halt, StepKind::TripleFaultHalt, tag.code);
                FaultStage::Halt
            }
            FaultStage::Halt => {
                // 不可达（is_halt 已拦）；防御性拒收，不落 panic。
                return Err(PanicRegError::FaultSeq {
                    code: E_FAULT_SEQ,
                    from: from.code(),
                    to: STAGE_HALT,
                });
            }
        };
        self.verdicts = self.verdicts.saturating_add(1);
        self.stage = to;
        Ok(PanicEntry {
            ordinal,
            stage_after: to,
            step_kind: match from {
                FaultStage::Normal => StepKind::PanicVerdict,
                FaultStage::Fault1 => StepKind::DoubleFault,
                _ => StepKind::TripleFaultHalt,
            },
            tag_code: tag.code,
            tag_name: tag.name.to_string(),
            msg_fingerprint: msg.fingerprint,
        })
    }

    /// 挂死保持断言（判据 J1-d「任何后续调用都保持 Halt 不变」）。
    ///
    /// 前置：必须已挂死（否则 E_NOT_HALTED——断言只在挂死后可执行）。
    /// 逐轮以一个 tag 尝试再入 panic 路径，每轮必须 E_HALT_LOCKED 且
    /// 阶梯保持 Halt；任何一轮阶梯漂移即 E_FAULT_SEQ（挂死语义破）。
    /// 返回完成的轮数。
    pub fn halt_maintain_assertion(
        &mut self,
        rounds: u32,
        probe_tag: RequiredTag,
    ) -> Result<u32, PanicRegError> {
        if !self.stage.is_halt() {
            return Err(PanicRegError::NotHalted { code: E_NOT_HALTED, stage: self.stage.code() });
        }
        let mut done: u32 = 0;
        while done < rounds {
            match self.enter_panic_path(probe_tag) {
                Err(e) if e.diag_code() == E_HALT_LOCKED => {
                    // 拒收正确——阶梯必须仍 Halt。
                    if !self.stage.is_halt() {
                        return Err(PanicRegError::FaultSeq {
                            code: E_FAULT_SEQ,
                            from: STAGE_HALT,
                            to: self.stage.code(),
                        });
                    }
                }
                Err(e) => return Err(e),
                Ok(_) => {
                    // 挂死后竟然裁决成功——挂死语义破。
                    return Err(PanicRegError::FaultSeq {
                        code: E_FAULT_SEQ,
                        from: STAGE_HALT,
                        to: self.stage.code(),
                    });
                }
            }
            done += 1;
        }
        Ok(done)
    }

    /// 阶梯留痕指纹（FNV over 逐步 seq/from/to/kind/tag——确定性对拍原料）。
    pub fn trace_fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.trace.len() {
            let s = &self.trace[i];
            bytes.extend_from_slice(&s.seq.to_le_bytes());
            bytes.push(s.from);
            bytes.push(s.to);
            bytes.push(s.kind.code());
            bytes.push(s.tag_code);
            i += 1;
        }
        fnv1a64(&bytes)
    }
}

// ── S5：MessageAccount 文案入账（不落盘——账面可查） ─────────────────
/// PANIC 文案内存账：入账即验三要素；按序号/tag 码/指纹三路查询。
///
/// 「不落盘」口径：账本体即内存 Vec，无任何存储后端调用；账面可查 =
/// 三路只读查询 API（J1 原文「文案入账（不落盘——账面可查）」）。
#[derive(Clone, Debug)]
pub struct MessageAccount {
    rows: Vec<PanicMessage>,
    cap: usize,
}

impl MessageAccount {
    /// 新文案账（容量 MSG_CAP=256）。
    pub fn new() -> MessageAccount {
        MessageAccount { rows: Vec::new(), cap: MSG_CAP }
    }

    /// 文案入账：容量关（E_CAPACITY）+ 三要素关（缺谁显性，
    /// MsgNoTag/MsgNoCode/MsgNoSite——文案无 tag 注入在此检出）。
    /// 同参重入账幂等放行（指纹已存在即返回该序号）。
    pub fn enter(&mut self, msg: PanicMessage) -> Result<u32, PanicRegError> {
        // 三要素核验（对入账文案本体，防绕过 builder 的手工文案）。
        match verify_three_elements(&msg.text, &msg.tag_name, msg.diag_code, &msg.site_text) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        // 幂等：同指纹已入账 → 返回既有序号（账面去重）。
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].fingerprint == msg.fingerprint {
                return Ok(i as u32);
            }
            i += 1;
        }
        if self.rows.len() >= self.cap {
            return Err(PanicRegError::Capacity {
                code: E_CAPACITY,
                which: "文案账",
                cap: self.cap,
            });
        }
        self.rows.push(msg);
        Ok((self.rows.len() - 1) as u32)
    }

    /// 按账内序号查询（0 基；越界 None）。
    pub fn by_index(&self, idx: usize) -> Option<&PanicMessage> {
        if idx < self.rows.len() {
            Some(&self.rows[idx])
        } else {
            None
        }
    }

    /// 按 tag 码查询（首条命中；账面可查路之二）。
    pub fn first_by_tag(&self, tag_code: u8) -> Option<&PanicMessage> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].tag_code == tag_code {
                return Some(&self.rows[i]);
            }
            i += 1;
        }
        None
    }

    /// 按指纹查询（账面可查路之三）。
    pub fn by_fingerprint(&self, fp: u64) -> Option<&PanicMessage> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].fingerprint == fp {
                return Some(&self.rows[i]);
            }
            i += 1;
        }
        None
    }

    /// 账内文案数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 全账只读视图。
    pub fn rows(&self) -> &[PanicMessage] {
        &self.rows
    }

    /// 账级指纹（FNV over 逐行指纹——确定性对拍原料）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            bytes.extend_from_slice(&self.rows[i].fingerprint.to_le_bytes());
            i += 1;
        }
        fnv1a64(&bytes)
    }
}

// ── S6：RegBook 回归账（判据 J1-e 回归可重放） ───────────────────────
/// 回归账单行：一次注入 → 结论 → 文案指纹 + 重放原材料。
#[derive(Clone, Debug)]
pub struct RegRow {
    /// 账内序号（0 基）。
    pub seq: u32,
    /// 注入的 tag 码（轮换面 1..=6 或手工注入）。
    pub injected_tag_code: u8,
    /// 检出结论（缺失被 panic 路径裁决且文案三要素齐才算检出）。
    pub detected: bool,
    /// 裁决后阶梯码。
    pub stage_after: u8,
    /// 文案指纹（重放核对目标）。
    pub msg_fingerprint: u64,
    /// 重放原材料：裁决序号（确定性重建文案用）。
    pub verdict_ordinal: u32,
    /// 是否轮换回归行（双跑对拍只核轮换面）。
    pub is_rotation: bool,
    /// 行注（账面口径）。
    pub note: String,
}

/// 重放核对报告。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 核对行数。
    pub rows: usize,
    /// 指纹相符行数。
    pub matched: u32,
    /// 不符行序号（空 = 全符）。
    pub mismatched_seqs: Vec<u32>,
}

impl ReplayReport {
    /// 全符判定。
    pub fn all_matched(&self) -> bool {
        self.mismatched_seqs.is_empty()
    }
}

/// 轮换回归汇总。
#[derive(Clone, Debug)]
pub struct RegSummary {
    /// 注入例数（六 tag 轮换 = 6）。
    pub cases: u64,
    /// 检出例数。
    pub detected: u64,
    /// 检出率 ppm（10000 = 100%）。
    pub rate_ppm: u64,
    /// 各例文案指纹（与账行同序）。
    pub fingerprints: Vec<u64>,
}

/// 回归账：每注入一行，容量 REG_CAP=128 封顶；replay 以原材料
/// （tag 码 + 裁决序号）确定性重建文案核对指纹（判据 J1-e）。
#[derive(Clone, Debug)]
pub struct RegBook {
    rows: Vec<RegRow>,
    cap: usize,
}

impl RegBook {
    /// 新回归账。
    pub fn new() -> RegBook {
        RegBook { rows: Vec::new(), cap: REG_CAP }
    }

    /// 记一行（容量满 E_CAPACITY）。
    pub fn record(
        &mut self,
        injected_tag_code: u8,
        detected: bool,
        stage_after: u8,
        msg_fingerprint: u64,
        verdict_ordinal: u32,
        is_rotation: bool,
        note: &str,
    ) -> Result<u32, PanicRegError> {
        if self.rows.len() >= self.cap {
            return Err(PanicRegError::Capacity {
                code: E_CAPACITY,
                which: "回归账",
                cap: self.cap,
            });
        }
        let seq = self.rows.len() as u32;
        self.rows.push(RegRow {
            seq,
            injected_tag_code,
            detected,
            stage_after,
            msg_fingerprint,
            verdict_ordinal,
            is_rotation,
            note: note.to_string(),
        });
        Ok(seq)
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 全账只读视图。
    pub fn rows(&self) -> &[RegRow] {
        &self.rows
    }

    /// 检出率 ppm（账空时 0——未跑回归不算全检出）。
    pub fn detection_rate_ppm(&self) -> u64 {
        if self.rows.is_empty() {
            return 0;
        }
        let mut detected: u64 = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].detected {
                detected += 1;
            }
            i += 1;
        }
        detected * PPM_SCALE / (self.rows.len() as u64)
    }

    /// 轮换行数（双跑对拍口径）。
    pub fn rotation_len(&self) -> usize {
        let mut n: usize = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].is_rotation {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账级指纹（FNV over 逐行 seq/tag/detected/stage/fp/ordinal——确定性）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            bytes.extend_from_slice(&r.seq.to_le_bytes());
            bytes.push(r.injected_tag_code);
            bytes.push(if r.detected { 1u8 } else { 0u8 });
            bytes.push(r.stage_after);
            bytes.extend_from_slice(&r.msg_fingerprint.to_le_bytes());
            bytes.extend_from_slice(&r.verdict_ordinal.to_le_bytes());
            i += 1;
        }
        fnv1a64(&bytes)
    }

    /// 轮换面指纹（只取轮换行，序序 FNV——与离线双跑对拍口径一致）。
    pub fn rotation_fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.is_rotation {
                bytes.push(r.injected_tag_code);
                bytes.extend_from_slice(&r.msg_fingerprint.to_le_bytes());
            }
            i += 1;
        }
        fnv1a64(&bytes)
    }

    /// 重放核对：逐行以（tag 码 → tag，裁决序号）重建文案，比对指纹。
    ///
    /// - tag 码越闭集 → E_TAG_UNKNOWN（防御：账被外改）；
    /// - 任一行指纹不符 → 该行入 mismatched_seqs，整体 Err(E_REPLAY_MISMATCH)；
    /// - 全符 → Ok（matched == rows）。
    /// 不改账本体（纯核对）。
    pub fn replay(&self) -> Result<ReplayReport, PanicRegError> {
        let mut mismatched: Vec<u32> = Vec::new();
        let mut matched: u32 = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let tag = match tag_from_code(r.injected_tag_code) {
                Some(t) => t,
                None => {
                    return Err(PanicRegError::TagUnknown {
                        code: E_TAG_UNKNOWN,
                        raw: r.injected_tag_code,
                    })
                }
            };
            let rebuilt = build_panic_message(tag, r.verdict_ordinal);
            if rebuilt.fingerprint == r.msg_fingerprint {
                matched += 1;
            } else {
                mismatched.push(r.seq);
            }
            i += 1;
        }
        if mismatched.is_empty() {
            Ok(ReplayReport { rows: self.rows.len(), matched, mismatched_seqs: mismatched })
        } else {
            // 报告细节取首不符行（错误面单行口径）。
            let first = mismatched[0] as usize;
            let stored = self.rows[first].msg_fingerprint;
            let tag = match tag_from_code(self.rows[first].injected_tag_code) {
                Some(t) => t,
                None => TAG_MEMORY_MAP,
            };
            let rebuilt = build_panic_message(tag, self.rows[first].verdict_ordinal);
            Err(PanicRegError::ReplayMismatch {
                code: E_REPLAY_MISMATCH,
                seq: mismatched[0],
                stored,
                rebuilt: rebuilt.fingerprint,
            })
        }
    }
}

// ── S7：Ledger 事件账（封板制）+ health() + 账级指纹 ─────────────────
/// 事件种类（闭集；code() 供指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    /// 单 tag 呈现入闸。
    GatePresented,
    /// 闸检全过（六项全在场）。
    GateCheckPass,
    /// 闸检见缺（进入 panic 路径前置）。
    GateCheckMissing,
    /// panic 裁决落账。
    PanicVerdicted,
    /// double fault。
    DoubleFault,
    /// triple fault 挂死。
    TripleFaultHalt,
    /// 挂死后输入拒收。
    HaltLocked,
    /// 挂死保持断言过。
    HaltMaintained,
    /// 注入检出。
    InjectionDetected,
    /// 注入漏检（不允许出现——health 红灯源）。
    InjectionMissed,
    /// 文案入账。
    MsgEntered,
    /// 文案被拒（三要素破——注入检出的一种）。
    MsgRejected,
    /// 重放核对全符。
    ReplayOk,
    /// 重放不符。
    ReplayMismatch,
    /// 双跑一致。
    DualRunOk,
    /// 双跑漂移。
    DualRunDrift,
    /// 六 tag 轮换回归完成。
    RotationDone,
    /// 封板。
    Sealed,
}

impl EvKind {
    /// 指纹用原始码（闭集 1..=18）。
    pub const fn code(&self) -> u8 {
        match self {
            EvKind::GatePresented => 1,
            EvKind::GateCheckPass => 2,
            EvKind::GateCheckMissing => 3,
            EvKind::PanicVerdicted => 4,
            EvKind::DoubleFault => 5,
            EvKind::TripleFaultHalt => 6,
            EvKind::HaltLocked => 7,
            EvKind::HaltMaintained => 8,
            EvKind::InjectionDetected => 9,
            EvKind::InjectionMissed => 10,
            EvKind::MsgEntered => 11,
            EvKind::MsgRejected => 12,
            EvKind::ReplayOk => 13,
            EvKind::ReplayMismatch => 14,
            EvKind::DualRunOk => 15,
            EvKind::DualRunDrift => 16,
            EvKind::RotationDone => 17,
            EvKind::Sealed => 18,
        }
    }

    /// 中文名（报告用）。
    pub const fn cn(&self) -> &'static str {
        match self {
            EvKind::GatePresented => "tag 呈现",
            EvKind::GateCheckPass => "闸检全过",
            EvKind::GateCheckMissing => "闸检见缺",
            EvKind::PanicVerdicted => "panic 裁决",
            EvKind::DoubleFault => "二重 fault",
            EvKind::TripleFaultHalt => "三重 fault 挂死",
            EvKind::HaltLocked => "挂死拒收",
            EvKind::HaltMaintained => "挂死保持",
            EvKind::InjectionDetected => "注入检出",
            EvKind::InjectionMissed => "注入漏检",
            EvKind::MsgEntered => "文案入账",
            EvKind::MsgRejected => "文案被拒",
            EvKind::ReplayOk => "重放全符",
            EvKind::ReplayMismatch => "重放不符",
            EvKind::DualRunOk => "双跑一致",
            EvKind::DualRunDrift => "双跑漂移",
            EvKind::RotationDone => "轮换回归完成",
            EvKind::Sealed => "封板",
        }
    }
}

/// 事件账单行。
#[derive(Clone, Debug)]
pub struct LedgerEvent {
    /// 事件序号（账内单调）。
    pub seq: u32,
    /// 事件种类。
    pub kind: EvKind,
    /// 关联诊断码（无关联 0）。
    pub code: u16,
    /// 事件注（账面口径）。
    pub note: String,
}

/// 事件账（封板制；容量 LEDGER_CAP=512 封顶）。
#[derive(Clone, Debug)]
pub struct Ledger {
    events: Vec<LedgerEvent>,
    cap: usize,
    sealed: bool,
}

/// Ledger 常量口径（与 F0725 同源 512）。
impl Ledger {
    /// 事件账容量。
    pub const CAP: usize = LEDGER_CAP;

    /// 新账（未封板）。
    pub fn new() -> Ledger {
        Ledger { events: Vec::new(), cap: Ledger::CAP, sealed: false }
    }

    /// 记一事（封板后 E_SEALED；满 E_CAPACITY）。
    pub fn record(&mut self, kind: EvKind, code: u16, note: &str) -> Result<(), PanicRegError> {
        if self.sealed {
            return Err(PanicRegError::Sealed { code: E_SEALED });
        }
        if self.events.len() >= self.cap {
            return Err(PanicRegError::Capacity {
                code: E_CAPACITY,
                which: "事件账",
                cap: self.cap,
            });
        }
        let seq = self.events.len() as u32;
        self.events.push(LedgerEvent { seq, kind, code, note: note.to_string() });
        Ok(())
    }

    /// 事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 是否已封板。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封板（幂等：已封再封 E_SEALED）。
    pub fn seal(&mut self) -> Result<(), PanicRegError> {
        if self.sealed {
            return Err(PanicRegError::Sealed { code: E_SEALED });
        }
        self.sealed = true;
        // 封板事件本体先落（seal 翻转前口径：封板行是最后一行）。
        let seq = self.events.len() as u32;
        if self.events.len() < self.cap {
            self.events.push(LedgerEvent {
                seq,
                kind: EvKind::Sealed,
                code: E_SEALED,
                note: "事件账封板".to_string(),
            });
        }
        Ok(())
    }

    /// 按种类计数。
    pub fn count_kind(&self, kind: EvKind) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].kind == kind {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 全账只读视图。
    pub fn events(&self) -> &[LedgerEvent] {
        &self.events
    }

    /// 账级指纹（FNV over seq 4B LE + kind 1B + code 2B LE + note 字节）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.events.len() {
            let e = &self.events[i];
            bytes.extend_from_slice(&e.seq.to_le_bytes());
            bytes.push(e.kind.code());
            bytes.extend_from_slice(&e.code.to_le_bytes());
            bytes.extend_from_slice(e.note.as_bytes());
            i += 1;
        }
        fnv1a64(&bytes)
    }
}

/// health 体检单（全字段账面派生，零手抄）。
#[derive(Clone, Debug)]
pub struct HealthSummary {
    /// 事件账行数。
    pub events: usize,
    /// tag 呈现次数。
    pub presents: u32,
    /// 闸检全过次数。
    pub gate_passes: u32,
    /// 闸检见缺次数。
    pub missing_detections: u32,
    /// panic 裁决数。
    pub panic_verdicts: u32,
    /// double fault 数。
    pub double_faults: u32,
    /// triple fault 数（≤1）。
    pub triple_faults: u32,
    /// 挂死后拒收数。
    pub halt_maintained: u32,
    /// 注入检出数（三注入面合计）。
    pub injections_detected: u32,
    /// 注入漏检数（必须 0）。
    pub injections_missed: u32,
    /// 文案账行数。
    pub messages: usize,
    /// 回归账行数。
    pub regression_rows: usize,
    /// 检出率 ppm（回归面口径）。
    pub detection_rate_ppm: u64,
    /// 重放不符数（必须 0）。
    pub replay_mismatches: u32,
    /// 双跑漂移数（必须 0）。
    pub dual_run_drifts: u32,
    /// 是否已封板。
    pub sealed: bool,
    /// 是否挂死态。
    pub halted: bool,
    /// 绿灯：无漏检 + 无重放不符 + 无双跑漂移 +（跑了回归则检出率 100%）。
    pub is_green: bool,
}

// ── S8：PanicRegAccount 总账 facade（判据 J1 全链） ──────────────────
/// 单例注入结论（tag 缺失注入/轮换单例共用）。
#[derive(Clone, Debug)]
pub struct InjectionOutcome {
    /// 注入的 tag 码。
    pub tag_code: u8,
    /// 注入的 tag 名。
    pub tag_name: String,
    /// 是否检出（裁决成立 + 文案三要素齐）。
    pub detected: bool,
    /// 裁决后阶梯码。
    pub stage_after: u8,
    /// 文案指纹。
    pub msg_fingerprint: u64,
    /// 裁决序号。
    pub verdict_ordinal: u32,
}

/// 挂死后输入注入结论。
#[derive(Clone, Debug)]
pub struct HaltOutcome {
    /// 尝试输入路数。
    pub attempts: u32,
    /// 全部被拒（E_HALT_LOCKED）。
    pub all_rejected: bool,
    /// 尝试后仍挂死。
    pub still_halted: bool,
    /// 挂死后拒收累计（含本次）。
    pub halt_maintained_total: u32,
}

/// 文案无 tag 注入的缺要素档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingElement {
    /// 缺 tag 名（E_MSG_NO_TAG）。
    Tag,
    /// 缺错误码（E_MSG_NO_CODE）。
    Code,
    /// 缺位置（E_MSG_NO_SITE）。
    Site,
}

impl MissingElement {
    /// 期望检出的诊断码。
    pub const fn want_code(&self) -> u16 {
        match self {
            MissingElement::Tag => E_MSG_NO_TAG,
            MissingElement::Code => E_MSG_NO_CODE,
            MissingElement::Site => E_MSG_NO_SITE,
        }
    }

    /// 中文名。
    pub const fn cn(&self) -> &'static str {
        match self {
            MissingElement::Tag => "tag 名",
            MissingElement::Code => "错误码",
            MissingElement::Site => "位置",
        }
    }
}

/// 文案缺要素注入结论。
#[derive(Clone, Debug)]
pub struct MsgRejectOutcome {
    /// 注入的缺要素档位。
    pub element: MissingElement,
    /// 实际检出诊断码（应 == want_code）。
    pub got_code: u16,
    /// 检出成立。
    pub detected: bool,
}

/// 双跑对拍报告。
#[derive(Clone, Debug)]
pub struct DualRunReport {
    /// 第一跑轮换面指纹。
    pub fp_a: u64,
    /// 第二跑轮换面指纹。
    pub fp_b: u64,
    /// 回归账轮换面指纹（账内行 vs 离线重跑的一致性证据）。
    pub regbook_fp: u64,
    /// 三者一致（fp_a==fp_b；账有轮换行时 regbook_fp 亦须相等）。
    pub consistent: bool,
}

/// panic 路径回归总账：闸 + 模拟器 + 文案账 + 回归账 + 事件账 一体 facade。
///
/// 挂死锁口径（判据 J1-d「不再接受任何输入」）：模拟器挂死后，
/// in-machine 通道（present/gate_check/inject_tag_missing/deliberate_
/// triple_fault/inject_message_without_tag/message_enter）一律
/// E_HALT_LOCKED；离线回归通道（rotation_case/run_rotation_regression/
/// replay/dual_run_check——自带独立模拟器，不向挂死机输入）不受锁。
#[derive(Clone, Debug)]
pub struct PanicRegAccount {
    gate: RequiredRespGate,
    sim: PanicPathSim,
    messages: MessageAccount,
    regbook: RegBook,
    ledger: Ledger,
}

impl PanicRegAccount {
    /// 新总账（Normal 态，六闸空）。
    pub fn new() -> PanicRegAccount {
        PanicRegAccount {
            gate: RequiredRespGate::new(),
            sim: PanicPathSim::new(),
            messages: MessageAccount::new(),
            regbook: RegBook::new(),
            ledger: Ledger::new(),
        }
    }

    // ── 只读视图 ──

    /// 闸只读引用。
    pub fn gate(&self) -> &RequiredRespGate {
        &self.gate
    }

    /// 模拟器只读引用。
    pub fn sim(&self) -> &PanicPathSim {
        &self.sim
    }

    /// 文案账只读引用。
    pub fn messages(&self) -> &MessageAccount {
        &self.messages
    }

    /// 回归账只读引用。
    pub fn regbook(&self) -> &RegBook {
        &self.regbook
    }

    /// 事件账只读引用。
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// 是否挂死。
    pub fn halted(&self) -> bool {
        self.sim.halted()
    }

    /// 是否封板。
    pub fn is_sealed(&self) -> bool {
        self.ledger.is_sealed()
    }

    /// 账开卷核（封板/挂死两关；in-machine 通道共用）。
    /// 挂死拒收时同步记入模拟器 halt_maintained（挂死保持证据统一口径）。
    fn check_open_inmachine(&mut self) -> Result<(), PanicRegError> {
        if self.ledger.is_sealed() {
            return Err(PanicRegError::Sealed { code: E_SEALED });
        }
        if self.sim.halted() {
            self.sim.note_halted_input();
            return Err(PanicRegError::HaltLocked {
                code: E_HALT_LOCKED,
                attempts: self.sim.halt_maintained(),
            });
        }
        Ok(())
    }

    // ── 闸面（in-machine） ──

    /// 呈现一项必需 tag（挂死/封板拒）。
    pub fn present(
        &mut self,
        tag_code: u8,
        rev: u64,
        payload_len: u32,
        entries: u32,
    ) -> Result<(), PanicRegError> {
        self.check_open_inmachine()?;
        match self.gate.present(tag_code, rev, payload_len, entries) {
            Ok(()) => {
                let tag = match tag_from_code(tag_code) {
                    Some(t) => t,
                    None => TAG_MEMORY_MAP,
                };
                self.ledger.record(
                    EvKind::GatePresented,
                    0,
                    &format!("tag='{}' rev={} len={} entries={}", tag.name, rev, payload_len, entries),
                )
            }
            Err(e) => Err(e),
        }
    }

    /// 闸检：六项全在场 → Ok（账面 GateCheckPass）；
    /// 有缺席 → 逐缺席 tag（升序）进入 panic 路径（错误码化裁决 +
    /// 文案入账），最后 Err(TagMissing)（panic 路径入口码）。
    ///
    /// 挂死态下拒收（E_HALT_LOCKED）——挂死后闸检也是「输入」。
    pub fn gate_check(&mut self) -> Result<GateCheck, PanicRegError> {
        self.check_open_inmachine()?;
        let gc = self.gate.check();
        if gc.complete {
            self.ledger.record(
                EvKind::GateCheckPass,
                0,
                &format!("mask=0x{:02X} coverage={}ppm", gc.present_mask, gc.coverage_ppm),
            )?;
            return Ok(gc);
        }
        self.ledger.record(
            EvKind::GateCheckMissing,
            E_TAG_MISSING,
            &format!("missing={} 路", gc.missing_codes.len()),
        )?;
        // 逐缺席 tag 进入 panic 路径（升序；阶梯按进入次数推进）。
        let mut i = 0usize;
        while i < gc.missing_codes.len() {
            let tag = match tag_from_code(gc.missing_codes[i]) {
                Some(t) => t,
                None => {
                    return Err(PanicRegError::TagUnknown {
                        code: E_TAG_UNKNOWN,
                        raw: gc.missing_codes[i],
                    })
                }
            };
            // in-machine 阶梯推进前再核挂死（多次进入会挂死——语义正确）。
            if self.sim.halted() {
                self.ledger.record(
                    EvKind::HaltLocked,
                    E_HALT_LOCKED,
                    "闸检在挂死态被拒收（三重 fault 保持）",
                )?;
                return Err(PanicRegError::HaltLocked {
                    code: E_HALT_LOCKED,
                    attempts: self.sim.halt_maintained().saturating_add(1),
                });
            }
            match self.sim.enter_panic_path(tag) {
                Ok(entry) => {
                    let msg = build_panic_message(tag, entry.ordinal);
                    // 文案入账（三要素核验内建；不落盘——账面可查）。
                    match self.messages.enter(msg.clone()) {
                        Ok(_) => {
                            self.ledger.record(
                                EvKind::MsgEntered,
                                0,
                                &format!("ordinal={} fp=0x{:016X}", entry.ordinal, msg.fingerprint),
                            )?;
                        }
                        Err(e) => return Err(e),
                    }
                    match entry.stage_after.code() {
                        STAGE_FAULT2 => {
                            self.ledger.record(
                                EvKind::DoubleFault,
                                E_FAULT_SEQ,
                                &format!("tag='{}' 二重 fault", tag.name),
                            )?;
                        }
                        STAGE_HALT => {
                            self.ledger.record(
                                EvKind::TripleFaultHalt,
                                E_FAULT_SEQ,
                                &format!("tag='{}' 三重 fault 挂死", tag.name),
                            )?;
                        }
                        _ => {
                            self.ledger.record(
                                EvKind::PanicVerdicted,
                                E_TAG_MISSING,
                                &format!("tag='{}' ordinal={} fp=0x{:016X}",
                                    tag.name, entry.ordinal, entry.msg_fingerprint),
                            )?;
                        }
                    }
                }
                Err(e) => return Err(e),
            }
            i += 1;
        }
        Err(PanicRegError::TagMissing {
            code: E_TAG_MISSING,
            missing: gc.missing_codes.clone(),
        })
    }

    // ── 注入面一：tag 缺失注入（in-machine，阶梯推进） ──

    /// 注入一路 tag 缺失（把在场标记抹掉 → 闸检 → 裁决 → 文案 → 回归账）。
    ///
    /// 检出判据三件齐：裁决成立 + 文案三要素齐（含 tag 名）+ 回归账落行。
    /// 文案含 tag 名逐例验证（J1「PANIC 文案含 tag 名可观测 100%」）。
    pub fn inject_tag_missing(&mut self, tag_code: u8) -> Result<InjectionOutcome, PanicRegError> {
        self.check_open_inmachine()?;
        let tag = match tag_from_code(tag_code) {
            Some(t) => t,
            None => {
                return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: tag_code });
            }
        };
        // 注入前在场快照（注入完成即恢复——瞬时扰动口径）。
        let prev = match self.gate.slot((tag.code - 1) as usize) {
            Some(r) => r,
            None => PresentRecord::absent(),
        };
        self.gate.mark_missing(tag_code)?;
        let detected;
        let mut stage_after = self.sim.stage_code();
        let mut msg_fp: u64 = 0;
        let mut ordinal: u32 = 0;
        let mut tag_name = tag.name.to_string();
        match self.gate_check() {
            Err(e) if e.diag_code() == E_TAG_MISSING => {
                // panic 路径已裁决（gate_check 内完成文案入账与事件账）。
                // 本注入目标 tag 的文案按指纹取回验证三要素。
                let msg = build_panic_message(tag, self.sim.verdicts());
                ordinal = msg.ordinal;
                msg_fp = msg.fingerprint;
                stage_after = self.sim.stage_code();
                let ec = verify_three_elements(&msg.text, &msg.tag_name, msg.diag_code, &msg.site_text);
                detected = matches!(ec, Ok(c) if c.all_present());
                tag_name = msg.tag_name;
                if detected {
                    self.ledger.record(
                        EvKind::InjectionDetected,
                        E_TAG_MISSING,
                        &format!("tag='{}' 缺失注入检出（文案三要素齐）", tag.name),
                    )?;
                } else {
                    self.ledger.record(
                        EvKind::InjectionMissed,
                        E_MSG_NO_TAG,
                        &format!("tag='{}' 缺失注入漏检（文案三要素破）", tag.name),
                    )?;
                }
            }
            Err(e) => return Err(e),
            Ok(_) => {
                // 抹掉后闸检竟然全过——闸语义破（不可达防御）。
                detected = false;
                self.ledger.record(
                    EvKind::InjectionMissed,
                    E_TAG_MISSING,
                    &format!("tag='{}' 缺失注入漏检（闸未显缺席）", tag.name),
                )?;
            }
        }
        // 恢复在场快照（瞬时扰动口径：注入只留阶梯推进与账行，不留闸缺）。
        if prev.present {
            let _ = self.gate.mark_present(tag_code, prev.rev, prev.payload_len, prev.entries);
        }
        self.regbook.record(
            tag_code,
            detected,
            stage_after,
            msg_fp,
            ordinal,
            false,
            &format!("手工缺失注入 tag='{}'", tag_name),
        )?;
        Ok(InjectionOutcome {
            tag_code,
            tag_name,
            detected,
            stage_after,
            msg_fingerprint: msg_fp,
            verdict_ordinal: ordinal,
        })
    }

    // ── 注入面一（离线）：六 tag 轮换回归（每例独立 panic 路径） ──

    /// 轮换单例：独立闸（六项全呈现、仅目标 tag 缺席）+ 独立模拟器
    /// （一裁决定格 Fault1），不推进本账 in-machine 阶梯（离线回归
    /// 通道——不向挂死机输入，判据 J1-e 回归账行来源）。
    ///
    /// 检出 = 裁决成立 + 文案三要素齐（含 tag 名逐例验证）+ 回归账落行。
    pub fn rotation_case(&mut self, tag_code: u8) -> Result<InjectionOutcome, PanicRegError> {
        if self.ledger.is_sealed() {
            return Err(PanicRegError::Sealed { code: E_SEALED });
        }
        let tag = match tag_from_code(tag_code) {
            Some(t) => t,
            None => {
                return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: tag_code });
            }
        };
        // 独立闸：六项全呈现、仅目标缺席。
        let mut gate = RequiredRespGate::new();
        let mut i = 0usize;
        while i < TAG_COUNT {
            let t = match tag_from_index(i) {
                Some(t) => t,
                None => {
                    return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: i as u8 });
                }
            };
            if t.code != tag.code {
                gate.present(t.code, t.min_rev, 64, 1)?;
            }
            i += 1;
        }
        let gc = gate.check();
        if gc.complete {
            // 不可达防御：目标 tag 必在场缺席。
            self.ledger.record(
                EvKind::InjectionMissed,
                E_TAG_MISSING,
                &format!("轮换 tag='{}' 闸未显缺席", tag.name),
            )?;
            return Ok(InjectionOutcome {
                tag_code: tag.code,
                tag_name: tag.name.to_string(),
                detected: false,
                stage_after: STAGE_NORMAL,
                msg_fingerprint: 0,
                verdict_ordinal: 0,
            });
        }
        // 独立模拟器：一裁决定格 Fault1。
        let mut sim = PanicPathSim::new();
        let entry = match sim.enter_panic_path(tag) {
            Ok(e) => e,
            Err(e) => return Err(e),
        };
        let msg = build_panic_message(tag, entry.ordinal);
        // 三要素逐项核（缺谁显性——文案含 tag 名 100% 判据的逐例验证）。
        let detected = match verify_three_elements(&msg.text, &msg.tag_name, msg.diag_code, &msg.site_text) {
            Ok(c) => c.all_present() && entry.stage_after.code() == STAGE_FAULT1,
            Err(_) => false,
        };
        // 文案入本账文案账（账面可查；同参幂等）。
        match self.messages.enter(msg.clone()) {
            Ok(_) => {
                self.ledger.record(
                    EvKind::MsgEntered,
                    0,
                    &format!("轮换 tag='{}' ordinal={} fp=0x{:016X}", tag.name, entry.ordinal, msg.fingerprint),
                )?;
            }
            Err(e) => return Err(e),
        }
        self.regbook.record(
            tag.code,
            detected,
            entry.stage_after.code(),
            msg.fingerprint,
            entry.ordinal,
            true,
            &format!("轮换缺失注入 tag='{}'", tag.name),
        )?;
        if detected {
            self.ledger.record(
                EvKind::InjectionDetected,
                E_TAG_MISSING,
                &format!("轮换 tag='{}' 缺失注入检出（文案三要素齐）", tag.name),
            )?;
        } else {
            self.ledger.record(
                EvKind::InjectionMissed,
                E_MSG_NO_TAG,
                &format!("轮换 tag='{}' 缺失注入漏检", tag.name),
            )?;
        }
        Ok(InjectionOutcome {
            tag_code: tag.code,
            tag_name: msg.tag_name,
            detected,
            stage_after: entry.stage_after.code(),
            msg_fingerprint: msg.fingerprint,
            verdict_ordinal: entry.ordinal,
        })
    }

    /// 六 tag 轮换回归（1..=6 逐例）：检出率不足 100% 即 E_RATE_NOT_FULL。
    /// 返回汇总（cases=6、detected 应=6、rate_ppm 应=10_000）。
    pub fn run_rotation_regression(&mut self) -> Result<RegSummary, PanicRegError> {
        if self.ledger.is_sealed() {
            return Err(PanicRegError::Sealed { code: E_SEALED });
        }
        let mut fingerprints: Vec<u64> = Vec::new();
        let mut detected: u64 = 0;
        let mut code: u8 = 1;
        while code <= TAG_COUNT as u8 {
            let outcome = self.rotation_case(code)?;
            if outcome.detected {
                detected += 1;
            }
            fingerprints.push(outcome.msg_fingerprint);
            code += 1;
        }
        let rate_ppm = detected * PPM_SCALE / (TAG_COUNT as u64);
        self.ledger.record(
            EvKind::RotationDone,
            0,
            &format!("六 tag 轮换：检出 {}/6，率 {}ppm", detected, rate_ppm),
        )?;
        if rate_ppm != PPM_SCALE {
            return Err(PanicRegError::RateNotFull {
                code: E_RATE_NOT_FULL,
                detected,
                total: TAG_COUNT as u64,
            });
        }
        Ok(RegSummary { cases: TAG_COUNT as u64, detected, rate_ppm, fingerprints })
    }

    // ── 三重 fault 漂习通道（in-machine，判据 J1-d 阶梯驱动） ──

    /// 三重 fault 漂习：直接驱动 panic 路径三连入（memory_map →
    /// framebuffer → kernel_address，F0725 三臂序），定格 Halt。
    ///
    /// 文案逐裁决入账；已挂死拒收（E_HALT_LOCKED）。返回定格阶梯码。
    pub fn deliberate_triple_fault(&mut self) -> Result<u8, PanicRegError> {
        self.check_open_inmachine()?;
        let codes: [u8; 3] = [TAG_MEMORY_MAP.code, TAG_FRAMEBUFFER.code, TAG_KERNEL_ADDRESS.code];
        let mut k = 0usize;
        while k < codes.len() {
            let tag = match tag_from_code(codes[k]) {
                Some(t) => t,
                None => {
                    return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: codes[k] });
                }
            };
            match self.sim.enter_panic_path(tag) {
                Ok(entry) => {
                    let msg = build_panic_message(tag, entry.ordinal);
                    match self.messages.enter(msg.clone()) {
                        Ok(_) => {
                            self.ledger.record(
                                EvKind::MsgEntered,
                                0,
                                &format!(
                                    "漂习 ordinal={} stage={} fp=0x{:016X}",
                                    entry.ordinal,
                                    entry.stage_after.code(),
                                    msg.fingerprint
                                ),
                            )?;
                        }
                        Err(e) => return Err(e),
                    }
                    match entry.step_kind {
                        StepKind::PanicVerdict => {
                            self.ledger.record(
                                EvKind::PanicVerdicted,
                                E_TAG_MISSING,
                                &format!("漂习 tag='{}' 一重 fault", tag.name),
                            )?;
                        }
                        StepKind::DoubleFault => {
                            self.ledger.record(
                                EvKind::DoubleFault,
                                E_FAULT_SEQ,
                                &format!("漂习 tag='{}' 二重 fault", tag.name),
                            )?;
                        }
                        StepKind::TripleFaultHalt => {
                            self.ledger.record(
                                EvKind::TripleFaultHalt,
                                E_FAULT_SEQ,
                                &format!("漂习 tag='{}' 三重 fault 挂死", tag.name),
                            )?;
                        }
                        StepKind::RejectedHalted => {
                            return Err(PanicRegError::FaultSeq {
                                code: E_FAULT_SEQ,
                                from: STAGE_HALT,
                                to: STAGE_HALT,
                            });
                        }
                    }
                }
                Err(e) => return Err(e),
            }
            k += 1;
        }
        if !self.sim.halted() {
            // 三连入后必须挂死——阶梯语义破防御。
            return Err(PanicRegError::FaultSeq {
                code: E_FAULT_SEQ,
                from: STAGE_FAULT2,
                to: self.sim.stage_code(),
            });
        }
        Ok(self.sim.stage_code())
    }

    // ── 注入面二：挂死后输入注入 ──

    /// 挂死后输入注入：对挂死机依次尝试四路 in-machine 输入
    /// （present / gate_check / inject_tag_missing / message_enter），
    /// 每路必须 E_HALT_LOCKED；任何一路放行即挂死语义破（E_FAULT_SEQ）。
    /// 前置：未挂死即 E_NOT_HALTED（先走 deliberate_triple_fault）。
    pub fn inject_post_halt_input(&mut self, probe_tag_code: u8) -> Result<HaltOutcome, PanicRegError> {
        if !self.sim.halted() {
            return Err(PanicRegError::NotHalted { code: E_NOT_HALTED, stage: self.sim.stage_code() });
        }
        let attempts: u32 = 4;
        let mut all_rejected = true;
        // 路 1：present。
        match self.present(probe_tag_code, 1, 64, 1) {
            Err(e) if e.diag_code() == E_HALT_LOCKED => {}
            Err(e) => return Err(e),
            Ok(()) => all_rejected = false,
        }
        // 路 2：gate_check。
        match self.gate_check() {
            Err(e) if e.diag_code() == E_HALT_LOCKED => {}
            Err(e) if e.diag_code() == E_TAG_MISSING => all_rejected = false,
            Err(e) => return Err(e),
            Ok(_) => all_rejected = false,
        }
        // 路 3：inject_tag_missing。
        match self.inject_tag_missing(probe_tag_code) {
            Err(e) if e.diag_code() == E_HALT_LOCKED => {}
            Err(e) => return Err(e),
            Ok(_) => all_rejected = false,
        }
        // 路 4：message_enter（手工文案入账通道）。
        let tag = match tag_from_code(probe_tag_code) {
            Some(t) => t,
            None => {
                return Err(PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: probe_tag_code });
            }
        };
        let msg = build_panic_message(tag, 1);
        match self.message_enter(msg) {
            Err(e) if e.diag_code() == E_HALT_LOCKED => {}
            Err(e) => return Err(e),
            Ok(_) => all_rejected = false,
        }
        let still_halted = self.sim.halted();
        if !all_rejected || !still_halted {
            return Err(PanicRegError::FaultSeq {
                code: E_FAULT_SEQ,
                from: STAGE_HALT,
                to: self.sim.stage_code(),
            });
        }
        self.ledger.record(
            EvKind::InjectionDetected,
            E_HALT_LOCKED,
            &format!("挂死后输入注入 {} 路全拒收（三重 fault 保持）", attempts),
        )?;
        Ok(HaltOutcome {
            attempts,
            all_rejected,
            still_halted,
            halt_maintained_total: self.sim.halt_maintained(),
        })
    }

    /// 挂死保持断言（账户层）：模拟器 HALT_PROBE_ROUNDS 轮逐核 +
    /// 一次账户级 present 拒收核。返回完成轮数。
    pub fn halt_maintain_assertion(&mut self) -> Result<u32, PanicRegError> {
        if !self.sim.halted() {
            return Err(PanicRegError::NotHalted { code: E_NOT_HALTED, stage: self.sim.stage_code() });
        }
        let probe_tag = TAG_HHDM;
        let done = match self.sim.halt_maintain_assertion(HALT_PROBE_ROUNDS, probe_tag) {
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        // 账户级再核一路（present 必拒）。
        match self.present(TAG_MODULES.code, 1, 64, 1) {
            Err(e) if e.diag_code() == E_HALT_LOCKED => {}
            Err(e) => return Err(e),
            Ok(()) => {
                return Err(PanicRegError::FaultSeq {
                    code: E_FAULT_SEQ,
                    from: STAGE_HALT,
                    to: self.sim.stage_code(),
                })
            }
        }
        self.ledger.record(
            EvKind::HaltMaintained,
            E_HALT_LOCKED,
            &format!("挂死保持断言 {} 轮 + 账户级 1 路全保持", done),
        )?;
        Ok(done)
    }

    // ── 注入面三：文案无 tag 注入（三档缺要素） ──

    /// 手工文案入账通道（in-machine）：挂死/封板拒；三要素破显性拒。
    pub fn message_enter(&mut self, msg: PanicMessage) -> Result<u32, PanicRegError> {
        self.check_open_inmachine()?;
        match self.messages.enter(msg) {
            Ok(idx) => {
                self.ledger.record(EvKind::MsgEntered, 0, "手工文案入账（三要素齐）")?;
                Ok(idx)
            }
            Err(e) => {
                let kind = match e.diag_code() {
                    E_MSG_NO_TAG | E_MSG_NO_CODE | E_MSG_NO_SITE => EvKind::MsgRejected,
                    _ => return Err(e),
                };
                self.ledger.record(kind, e.diag_code(), "手工文案被拒（三要素破）")?;
                Err(e)
            }
        }
    }

    /// 注入一份缺要素文案（档位 Tag/Code/Site）→ 必须以对应码拒收
    /// （E_MSG_NO_TAG / E_MSG_NO_CODE / E_MSG_NO_SITE——检出即 Err 载回）。
    pub fn inject_message_without_element(
        &mut self,
        element: MissingElement,
    ) -> Result<MsgRejectOutcome, PanicRegError> {
        self.check_open_inmachine()?;
        let tag = TAG_MEMORY_MAP;
        let good = build_panic_message(tag, 1);
        // 构造缺要素文案：按档位挖掉对应要素（其余保持）。
        let text = match element {
            MissingElement::Tag => {
                // 挖 tag 名（换成哨兵串——文案仍含码与位置）。
                good.text.replace(tag.name, "□□□")
            }
            MissingElement::Code => {
                good.text.replace(&format!("0x{:04X}", E_TAG_MISSING), "0x□□□□")
            }
            MissingElement::Site => {
                good.text.replace(&good.site_text, "□site□")
            }
        };
        let crafted = PanicMessage {
            fingerprint: fnv1a64(text.as_bytes()),
            text,
            tag_code: good.tag_code,
            tag_name: good.tag_name.clone(),
            diag_code: good.diag_code,
            site_text: good.site_text.clone(),
            ordinal: good.ordinal,
        };
        let want = element.want_code();
        match self.message_enter(crafted) {
            Err(e) if e.diag_code() == want => {
                let got = e.diag_code();
                self.ledger.record(
                    EvKind::InjectionDetected,
                    want,
                    &format!("文案缺{}注入检出（0x{:04X}）", element.cn(), want),
                )?;
                Ok(MsgRejectOutcome { element, got_code: got, detected: true })
            }
            Err(e) => Err(e),
            Ok(_) => Err(PanicRegError::MsgNoTag {
                code: E_MSG_NO_TAG,
                text: "缺要素文案竟然入账——三要素核验失效".to_string(),
            }),
        }
    }

    // ── 回归面：重放 + 双跑 + 报告 + health ──

    /// 回归账重放核对（判据 J1-e）：全符 ReplayOk 落账；不符 ReplayMismatch
    /// 落账 + Err。
    pub fn replay(&mut self) -> Result<ReplayReport, PanicRegError> {
        match self.regbook.replay() {
            Ok(rep) => {
                self.ledger.record(
                    EvKind::ReplayOk,
                    0,
                    &format!("重放 {} 行全符", rep.rows),
                )?;
                Ok(rep)
            }
            Err(e) => {
                self.ledger.record(
                    EvKind::ReplayMismatch,
                    e.diag_code(),
                    "重放指纹不符（确定性破）",
                )?;
                Err(e)
            }
        }
    }

    /// 双跑确定性对拍（判据同 F0727 A/B 口径）：离线轮换面纯函数跑两次
    /// （fp_a/fp_b 必等），且账内轮换行指纹（regbook_fp）与离线一致
    /// （账有轮换行时）。三者一致 DualRunOk；漂移 DualRunDrift + Err。
    pub fn dual_run_check(&mut self) -> Result<DualRunReport, PanicRegError> {
        let fp_a = rotation_fingerprint();
        let fp_b = rotation_fingerprint();
        let regbook_fp = self.regbook.rotation_fingerprint();
        let rot_len = self.regbook.rotation_len();
        let consistent = fp_a == fp_b && (rot_len == 0 || regbook_fp == fp_a);
        let report = DualRunReport { fp_a, fp_b, regbook_fp, consistent };
        if consistent {
            self.ledger.record(
                EvKind::DualRunOk,
                0,
                &format!("双跑一致 fp=0x{:016X}（账轮换 {} 行绑定）", fp_a, rot_len),
            )?;
            Ok(report)
        } else {
            self.ledger.record(
                EvKind::DualRunDrift,
                E_DUAL_RUN_DRIFT,
                "双跑漂移：离线两跑不等或账行脱钩",
            )?;
            Err(PanicRegError::DualRunDrift { code: E_DUAL_RUN_DRIFT, fp_a, fp_b })
        }
    }

    /// 账级指纹：FNV over（回归账指纹 ++ 文案账指纹 ++ 事件账指纹 ++
    /// 模拟器留痕指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&self.regbook.fingerprint().to_le_bytes());
        bytes.extend_from_slice(&self.messages.fingerprint().to_le_bytes());
        bytes.extend_from_slice(&self.ledger.fingerprint().to_le_bytes());
        bytes.extend_from_slice(&self.sim.trace_fingerprint().to_le_bytes());
        fnv1a64(&bytes)
    }

    /// health 体检单（全字段账面派生，零手抄）。
    ///
    /// 绿灯口径：注入漏检 0 + 重放不符 0 + 双跑漂移 0 +
    /// （回归账非空时检出率 10000ppm）。挂死态本身不扣灯
    /// （三重 fault 是保词语义，不是故障）。
    pub fn health(&self) -> HealthSummary {
        let missed = self.ledger.count_kind(EvKind::InjectionMissed);
        let replay_bad = self.ledger.count_kind(EvKind::ReplayMismatch);
        let drift = self.ledger.count_kind(EvKind::DualRunDrift);
        let rate = self.regbook.detection_rate_ppm();
        let rows = self.regbook.len();
        let is_green =
            missed == 0 && replay_bad == 0 && drift == 0 && (rows == 0 || rate == PPM_SCALE);
        HealthSummary {
            events: self.ledger.len(),
            presents: self.gate.presents(),
            gate_passes: self.ledger.count_kind(EvKind::GateCheckPass),
            missing_detections: self.ledger.count_kind(EvKind::GateCheckMissing),
            panic_verdicts: self.sim.verdicts(),
            double_faults: self.sim.double_faults(),
            triple_faults: self.sim.triple_faults(),
            halt_maintained: self.sim.halt_maintained(),
            injections_detected: self.ledger.count_kind(EvKind::InjectionDetected),
            injections_missed: missed,
            messages: self.messages.len(),
            regression_rows: rows,
            detection_rate_ppm: rate,
            replay_mismatches: replay_bad,
            dual_run_drifts: drift,
            sealed: self.ledger.is_sealed(),
            halted: self.sim.halted(),
            is_green,
        }
    }

    /// 封板（幂等：再封 E_SEALED）。
    pub fn seal(&mut self) -> Result<(), PanicRegError> {
        self.ledger.seal()
    }

    /// 六节回归报告（程序化生成，零手抄——全字段账面派生）：
    /// 一闸口径 / 二轮换检出 / 三文案要素 / 四 fault 阶梯 /
    /// 五回归账重放 / 六挂死保持。
    pub fn regression_report(&self) -> String {
        let h = self.health();
        let mut out: String = String::new();
        out.push_str("== UNX-F0726 必需响应缺失 panic 路径回归报告 ==\n");
        // 一、闸口径。
        out.push_str("[一] 闸口径：");
        out.push_str(&format!(
            "六 tag 闭集 1..=6（{}/{}/{}/{}/{}/{}），呈现 {} 次，闸检 {} 次，在场掩码 0x{:02X}，覆盖 {}ppm\n",
            TAG_MEMORY_MAP.name,
            TAG_FRAMEBUFFER.name,
            TAG_KERNEL_ADDRESS.name,
            TAG_HHDM.name,
            TAG_MODULES.name,
            TAG_FIRMWARE.name,
            h.presents,
            self.gate.checks(),
            self.gate.present_mask(),
            if self.gate.is_complete() { PPM_SCALE } else {
                (self.gate.present_mask().count_ones() as u64) * PPM_SCALE / (TAG_COUNT as u64)
            }
        ));
        // 二、轮换检出。
        out.push_str("[二] 轮换检出：");
        out.push_str(&format!(
            "回归账 {} 行，检出率 {}ppm（{} 检出 / {} 漏检）\n",
            h.regression_rows,
            h.detection_rate_ppm,
            h.injections_detected,
            h.injections_missed
        ));
        // 三、文案要素。
        out.push_str("[三] 文案要素：");
        out.push_str(&format!(
            "文案账 {} 行（不落盘——账面可查），逐行三要素核验入账关内建\n",
            h.messages
        ));
        let mut mi = 0usize;
        while mi < self.messages.len() {
            let m = match self.messages.by_index(mi) {
                Some(m) => m,
                None => break,
            };
            out.push_str(&format!(
                "  #{:03} tag='{}' diag=0x{:04X} site={} ordinal={} fp=0x{:016X}\n",
                mi, m.tag_name, m.diag_code, m.site_text, m.ordinal, m.fingerprint
            ));
            mi += 1;
        }
        // 四、fault 阶梯。
        out.push_str("[四] fault 阶梯：");
        out.push_str(&format!(
            "裁决 {}，二重 {}，三重 {}，当前 {}（{}），留痕 {} 步\n",
            h.panic_verdicts,
            h.double_faults,
            h.triple_faults,
            self.sim.stage().code(),
            self.sim.stage().cn(),
            self.sim.trace().len()
        ));
        let mut ti = 0usize;
        while ti < self.sim.trace().len() {
            let s = &self.sim.trace()[ti];
            out.push_str(&format!(
                "  步#{} {}→{} {}（tag {}）\n",
                s.seq,
                FaultStage::from_code(s.from).map_or("?", |f| f.cn()),
                FaultStage::from_code(s.to).map_or("?", |f| f.cn()),
                s.kind.cn(),
                s.tag_code
            ));
            ti += 1;
        }
        // 五、回归账重放。
        out.push_str("[五] 回归账重放：");
        match self.regbook.replay() {
            Ok(rep) => {
                out.push_str(&format!("{} 行全符（matched={}）\n", rep.rows, rep.matched));
            }
            Err(e) => {
                out.push_str(&format!("不符：{}（0x{:04X}）\n", e.detail(), e.diag_code()));
            }
        }
        // 六、挂死保持。
        out.push_str("[六] 挂死保持：");
        out.push_str(&format!(
            "挂死态={}，挂死后拒收 {} 次，三重 fault 保持断言 {} 事件\n",
            h.halted,
            h.halt_maintained,
            self.ledger.count_kind(EvKind::HaltMaintained)
        ));
        out.push_str(&format!(
            "== 尾注：绿灯={}（事件 {} 行，封板={}）==\n",
            h.is_green,
            h.events,
            h.sealed
        ));
        out
    }
}

/// 离线轮换面纯函数指纹：六 tag 逐例独立闸+独立模拟器一裁决，
/// FNV over（tag 码 ++ 文案指纹）。双跑对拍的确定性真值源。
pub fn rotation_fingerprint() -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut code: u8 = 1;
    while code <= TAG_COUNT as u8 {
        if let Some(tag) = tag_from_code(code) {
            let msg = build_panic_message(tag, 1);
            bytes.push(tag.code);
            bytes.extend_from_slice(&msg.fingerprint.to_le_bytes());
        }
        code += 1;
    }
    fnv1a64(&bytes)
}

// ═════════════════════════════════════════════════════════════════════
// 测试域（#[cfg(test)]——std 通道；内核路径不受影响）
// ═════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;
    // 测试域专用（std 通道；内核路径不受影响）。
    use std::vec;

    /// 测试辅助：Ok 解包（Err 即 panic，仅测试域）。
    fn ok<T>(r: Result<T, PanicRegError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => std::panic::panic_any(format!("expect ok, got 0x{:04X}: {}", e.diag_code(), e.detail())),
        }
    }

    /// 测试辅助：断言诊断码。
    fn assert_code(r: Result<(), PanicRegError>, want: u16) {
        match r {
            Ok(_) => std::panic::panic_any(format!("expect err 0x{:04X}, got ok", want)),
            Err(e) => assert_eq!(e.diag_code(), want, "diag mismatch: {}", e.detail()),
        }
    }

    /// 干净六呈现（rev=1、payload 64B、entries 1——账面下限口径）。
    fn present_all(acc: &mut PanicRegAccount) {
        let mut c: u8 = 1;
        while c <= TAG_COUNT as u8 {
            ok(acc.present(c, 1, 64, 1));
            c += 1;
        }
    }

    // ── T1：诊断码段独占 + 每变体中文可操作诊断 ──
    #[test]
    fn diag_segment_exclusive_and_details() {
        // 段内 16 码全部有中文诊断。
        let mut c: u16 = SEG_BASE;
        while c <= 0x273F {
            let d = diag_detail(c);
            assert!(!d.starts_with("未知"), "0x{:04X} 缺诊断文案", c);
            assert!(d.len() >= 8, "0x{:04X} 诊断过短", c);
            c += 1;
        }
        // 段外归未知（左邻 0x2720 段被 ves502 占，右邻 0x2740 段被
        // unx3181 占——本段两端界外必须未知）。
        assert!(diag_detail(0x272F).starts_with("未知"));
        assert!(diag_detail(0x2740).starts_with("未知"));
        // 每个错误变体 detail 非空且码在段内。
        let samples: Vec<PanicRegError> = vec![
            PanicRegError::Sealed { code: E_SEALED },
            PanicRegError::TagUnknown { code: E_TAG_UNKNOWN, raw: 9 },
            PanicRegError::TagMissing { code: E_TAG_MISSING, missing: vec![6] },
            PanicRegError::RevisionBad { code: E_REVISION_BAD, tag: 1, observed: 0 },
            PanicRegError::PresentConflict { code: E_PRESENT_CONFLICT, tag: 1, first_rev: 1, new_rev: 2 },
            PanicRegError::HaltLocked { code: E_HALT_LOCKED, attempts: 3 },
            PanicRegError::MsgNoTag { code: E_MSG_NO_TAG, text: "x".to_string() },
            PanicRegError::MsgNoCode { code: E_MSG_NO_CODE, text: "x".to_string() },
            PanicRegError::MsgNoSite { code: E_MSG_NO_SITE, text: "x".to_string() },
            PanicRegError::FaultSeq { code: E_FAULT_SEQ, from: 0, to: 3 },
            PanicRegError::NotHalted { code: E_NOT_HALTED, stage: 0 },
            PanicRegError::ReplayMismatch { code: E_REPLAY_MISMATCH, seq: 0, stored: 1, rebuilt: 2 },
            PanicRegError::DualRunDrift { code: E_DUAL_RUN_DRIFT, fp_a: 1, fp_b: 2 },
            PanicRegError::RateNotFull { code: E_RATE_NOT_FULL, detected: 5, total: 6 },
            PanicRegError::Capacity { code: E_CAPACITY, which: "事件账", cap: 512 },
            PanicRegError::PayloadBad { code: E_PAYLOAD_BAD, tag: 1, payload_len: 5000, entries: 0 },
        ];
        assert_eq!(samples.len(), 16, "16 变体闭集");
        let mut i = 0usize;
        while i < samples.len() {
            let e = &samples[i];
            assert!(e.diag_code() >= SEG_BASE && e.diag_code() <= 0x273F);
            assert!(!e.detail().is_empty());
            assert!(!e.detail().starts_with("未知"));
            i += 1;
        }
    }

    // ── T2：六 tag 闭集 + 位置锚口径 ──
    #[test]
    fn tag_closed_set_and_anchors() {
        // 码 1..=6 全中，闭集外全空。
        let mut c: u8 = 1;
        while c <= 6 {
            let t = match tag_from_code(c) {
                Some(t) => t,
                None => std::panic::panic_any("1..=6 必中"),
            };
            assert_eq!(t.code, c);
            assert!(!t.name.is_empty());
            c += 1;
        }
        assert!(tag_from_code(0).is_none());
        assert!(tag_from_code(7).is_none());
        assert!(tag_from_code(255).is_none());
        // idx 闭集。
        assert!(tag_from_index(6).is_none());
        assert_eq!(tag_from_index(0).map(|t| t.code), Some(1));
        // 前三项与 F0725 三臂同码对齐声明。
        assert_eq!(TAG_MEMORY_MAP.code, 1);
        assert_eq!(TAG_FRAMEBUFFER.code, 2);
        assert_eq!(TAG_KERNEL_ADDRESS.code, 3);
        // 位置锚：0xF07261..0xF07266。
        assert_eq!(tag_anchor(TAG_MEMORY_MAP), 0xF07261);
        assert_eq!(tag_anchor(TAG_FIRMWARE), 0xF07266);
        assert_eq!(tag_site_text(TAG_FIRMWARE), "unx0726/gate/fw#0xF07266");
        assert!(tag_site_text(TAG_HHDM).contains("hhdm"));
        // const 期合理性核（编译期已断言，这里运行期复证）。
        assert!(required_tags_sane());
        assert_eq!(REQUIRED_TAGS.len(), TAG_COUNT);
    }

    // ── T3：闸三拒 + 幂等重呈 + 覆盖率 ──
    #[test]
    fn gate_present_rules_and_check() {
        let mut gate = RequiredRespGate::new();
        // 三拒。
        assert_code(gate.present(9, 1, 64, 1).map(|_| ()), E_TAG_UNKNOWN);
        assert_code(gate.present(1, 0, 64, 1).map(|_| ()), E_REVISION_BAD);
        assert_code(gate.present(1, REV_CAP + 1, 64, 1).map(|_| ()), E_REVISION_BAD);
        assert_code(gate.present(1, 1, PAYLOAD_CAP + 1, 0).map(|_| ()), E_PAYLOAD_BAD);
        assert_code(gate.present(1, 1, 0, ENTRY_COUNT_CAP + 1).map(|_| ()), E_PAYLOAD_BAD);
        // 逐项呈现：覆盖率单调升至 100%。
        let mut c: u8 = 1;
        while c <= 6 {
            ok(gate.present(c, 1, 64, 1));
            let gc = gate.check();
            assert_eq!(gc.coverage_ppm, (c as u64) * PPM_SCALE / 6);
            if c < 6 {
                assert!(!gc.complete, "第 {} 项呈现后不应全过", c);
            }
            c += 1;
        }
        let full = gate.check();
        assert!(full.complete);
        assert_eq!(full.present_mask, REQUIRED_MASK_ALL);
        assert_eq!(full.missing_len(), 0);
        assert_eq!(full.coverage_ppm, PPM_SCALE);
        // 幂等重呈放行；参数漂移拒。
        ok(gate.present(2, 1, 64, 1));
        assert_code(gate.present(2, 2, 64, 1).map(|_| ()), E_PRESENT_CONFLICT);
        // 注入面原语：标记回缺席 → 显性缺席。
        ok(gate.mark_missing(4));
        let gc = gate.check();
        assert!(!gc.complete);
        assert_eq!(gc.missing_codes, vec![4u8]);
        assert_eq!(gc.missing_names, vec!["hhdm".to_string()]);
        ok(gate.mark_missing(4)); // 幂等。
        assert_code(gate.mark_missing(0).map(|_| ()), E_TAG_UNKNOWN);
        assert_eq!(gate.presents(), 7); // 6 首发 + 1 幂等
        // 只读视图不推进计数。
        assert!(!gate.is_complete());
        assert!(gate.slot(3).is_some());
        assert!(gate.slot(6).is_none());
    }

    // ── T4：PANIC 文案三要素 + 确定性指纹 ──
    #[test]
    fn panic_message_three_elements() {
        let m = build_panic_message(TAG_HHDM, 3);
        // 横幅 + 三要素逐项在场。
        assert!(m.text.contains(PANIC_BANNER));
        assert!(m.text.contains("hhdm"), "文案必须含 tag 名：{}", m.text);
        assert!(m.text.contains("0x2732"), "文案必须含错误码：{}", m.text);
        assert!(m.text.contains(&tag_site_text(TAG_HHDM)), "文案必须含位置：{}", m.text);
        assert_eq!(m.diag_code, E_TAG_MISSING);
        assert_eq!(m.ordinal, 3);
        // verify 全过（三要素全真）。
        let ec = ok(verify_three_elements(&m.text, &m.tag_name, m.diag_code, &m.site_text));
        assert!(ec.all_present());
        assert!(ec.has_tag && ec.has_code && ec.has_site);
        // 同参确定性。
        let m2 = build_panic_message(TAG_HHDM, 3);
        assert_eq!(m.fingerprint, m2.fingerprint);
        assert_eq!(m.text, m2.text);
        // 异参必异纹。
        let m3 = build_panic_message(TAG_HHDM, 4);
        assert_ne!(m.fingerprint, m3.fingerprint);
        // 三缺逐路检出（文案无 tag/无码/无位置注入的核验面）。
        let t = m.text.replace("hhdm", "□□□□");
        assert_code(
            verify_three_elements(&t, "hhdm", m.diag_code, &m.site_text).map(|_| ()),
            E_MSG_NO_TAG,
        );
        let t2 = m.text.replace("0x2732", "0x□□□□");
        assert_code(
            verify_three_elements(&t2, "hhdm", m.diag_code, &m.site_text).map(|_| ()),
            E_MSG_NO_CODE,
        );
        let t3 = m.text.replace(&m.site_text, "□site□");
        assert_code(
            verify_three_elements(&t3, "hhdm", m.diag_code, &m.site_text).map(|_| ()),
            E_MSG_NO_SITE,
        );
    }

    // ── T5：fault 阶梯 + 三重 fault 挂死 + 挂死后拒收（模拟器直证） ──
    #[test]
    fn fault_ladder_and_halt_lock() {
        let mut sim = PanicPathSim::new();
        assert_eq!(sim.stage_code(), STAGE_NORMAL);
        // 三连入：Fault1 → Fault2 → Halt。
        let e1 = ok(sim.enter_panic_path(TAG_MEMORY_MAP));
        assert_eq!(e1.stage_after.code(), STAGE_FAULT1);
        assert_eq!(e1.step_kind, StepKind::PanicVerdict);
        let e2 = ok(sim.enter_panic_path(TAG_FRAMEBUFFER));
        assert_eq!(e2.stage_after.code(), STAGE_FAULT2);
        assert_eq!(e2.step_kind, StepKind::DoubleFault);
        let e3 = ok(sim.enter_panic_path(TAG_KERNEL_ADDRESS));
        assert_eq!(e3.stage_after.code(), STAGE_HALT);
        assert_eq!(e3.step_kind, StepKind::TripleFaultHalt);
        assert!(sim.halted());
        assert_eq!(sim.verdicts(), 3);
        assert_eq!(sim.double_faults(), 1);
        assert_eq!(sim.triple_faults(), 1);
        // 裁决序号单调。
        assert_eq!((e1.ordinal, e2.ordinal, e3.ordinal), (1, 2, 3));
        // 挂死后任何进入一律拒收，且阶梯保持。
        assert_code(
            match sim.enter_panic_path(TAG_HHDM) {
                Ok(v) => Ok(v),
                Err(e) => Err(e),
            }
            .map(|_| ()),
            E_HALT_LOCKED,
        );
        assert!(sim.halted());
        assert_eq!(sim.halt_maintained(), 1);
        assert_code(
            sim.enter_panic_path(TAG_MODULES).map(|_| ()),
            E_HALT_LOCKED,
        );
        assert_eq!(sim.halt_maintained(), 2);
        // 留痕四步：裁决/二重/三重/拒收。
        assert_eq!(sim.trace().len(), 5); // 3 阶梯 + 2 拒收
        assert_eq!(sim.trace()[0].kind, StepKind::PanicVerdict);
        assert_eq!(sim.trace()[1].kind, StepKind::DoubleFault);
        assert_eq!(sim.trace()[2].kind, StepKind::TripleFaultHalt);
        assert_eq!(sim.trace()[3].kind, StepKind::RejectedHalted);
        assert_eq!(sim.trace()[4].kind, StepKind::RejectedHalted);
        // 留痕指纹确定性：同操作同纹；多两次拒收步必异纹。
        let mut sim2 = PanicPathSim::new();
        ok(sim2.enter_panic_path(TAG_MEMORY_MAP));
        ok(sim2.enter_panic_path(TAG_FRAMEBUFFER));
        ok(sim2.enter_panic_path(TAG_KERNEL_ADDRESS));
        assert_ne!(sim.trace_fingerprint(), sim2.trace_fingerprint()); // sim 多两次拒收步
        let mut sim3 = PanicPathSim::new();
        ok(sim3.enter_panic_path(TAG_MEMORY_MAP));
        ok(sim3.enter_panic_path(TAG_FRAMEBUFFER));
        ok(sim3.enter_panic_path(TAG_KERNEL_ADDRESS));
        assert_eq!(sim2.trace_fingerprint(), sim3.trace_fingerprint());
    }

    // ── T6：闸检缺失 → panic 路径（账户链路：裁决+文案入账+事件账） ──
    #[test]
    fn account_gate_check_missing_enters_panic_path() {
        // 五项呈现（缺 firmware）→ 闸检 Err(TagMissing)，panic 路径裁决一格。
        let mut acc2 = PanicRegAccount::new();
        let mut c: u8 = 1;
        while c <= 5 {
            ok(acc2.present(c, 1, 64, 1));
            c += 1;
        }
        match acc2.gate_check() {
            Ok(_) => std::panic::panic_any("缺 firmware 必入 panic 路径"),
            Err(e) => {
                assert_eq!(e.diag_code(), E_TAG_MISSING);
                match e {
                    PanicRegError::TagMissing { missing, .. } => {
                        assert_eq!(missing, vec![6u8]);
                    }
                    _ => std::panic::panic_any("变体口径"),
                }
            }
        }
        // 一裁决：Fault1；文案一行（firmware）；事件账见缺+裁决+入账。
        assert_eq!(acc2.sim().stage_code(), STAGE_FAULT1);
        assert_eq!(acc2.messages().len(), 1);
        let m = acc2.messages().first_by_tag(6).map(|m| m.text.clone());
        let text = match m {
            Some(t) => t,
            None => std::panic::panic_any("firmware 文案必须在账"),
        };
        assert!(text.contains("firmware"));
        assert!(text.contains("0x2732"));
        assert!(acc2.ledger().count_kind(EvKind::GateCheckMissing) >= 1);
        assert!(acc2.ledger().count_kind(EvKind::PanicVerdicted) >= 1);
        assert!(acc2.ledger().count_kind(EvKind::MsgEntered) >= 1);
    }

    // ── T7：六 tag 轮换回归 100% + 文案含 tag 名逐例验证 ──
    #[test]
    fn rotation_regression_full_detection() {
        let mut acc = PanicRegAccount::new();
        let sum = ok(acc.run_rotation_regression());
        assert_eq!(sum.cases, 6);
        assert_eq!(sum.detected, 6, "六 tag 轮换检出必须 6/6");
        assert_eq!(sum.rate_ppm, PPM_SCALE);
        assert_eq!(sum.fingerprints.len(), 6);
        // 回归账六行全轮换、全检出、全定格 Fault1。
        assert_eq!(acc.regbook().len(), 6);
        let mut i = 0usize;
        while i < acc.regbook().rows().len() {
            let r = &acc.regbook().rows()[i];
            assert!(r.detected);
            assert!(r.is_rotation);
            assert_eq!(r.stage_after, STAGE_FAULT1);
            assert_eq!(r.verdict_ordinal, 1);
            i += 1;
        }
        // 文案账：六 tag 各一行，逐例含 tag 名（J1「可观测 100%」逐例验证）。
        assert_eq!(acc.messages().len(), 6);
        let mut c: u8 = 1;
        while c <= 6 {
            let tag = match tag_from_code(c) {
                Some(t) => t,
                None => std::panic::panic_any("闭集内"),
            };
            let m = match acc.messages().first_by_tag(c) {
                Some(m) => m,
                None => std::panic::panic_any("六 tag 文案必须在账"),
            };
            assert!(m.text.contains(tag.name), "tag={} 文案缺名", tag.name);
            assert!(m.text.contains("0x2732"));
            assert!(m.text.contains(&tag_site_text(tag)));
            c += 1;
        }
        // 离线通道不推阶梯：账户模拟器仍 Normal。
        assert_eq!(acc.sim().stage_code(), STAGE_NORMAL);
        assert!(!acc.halted());
        // health 绿灯（检出率 10000ppm、零漏检）。
        let h = acc.health();
        assert!(h.is_green, "health 应绿：{:?}", h);
        assert_eq!(h.detection_rate_ppm, PPM_SCALE);
        assert_eq!(h.injections_missed, 0);
        assert_eq!(h.injections_detected, 6);
    }

    // ── T8：回归账重放（全符 + 篡改检出） ──
    #[test]
    fn regbook_replay_and_tamper_detection() {
        let mut acc = PanicRegAccount::new();
        ok(acc.run_rotation_regression());
        let rep = ok(acc.replay());
        assert_eq!(rep.rows, 6);
        assert_eq!(rep.matched, 6);
        assert!(rep.all_matched());
        assert_eq!(acc.ledger().count_kind(EvKind::ReplayOk), 1);
        // 篡改一行指纹 → 重放必检出 E_REPLAY_MISMATCH。
        acc.regbook.rows[2].msg_fingerprint = 0xDEAD_BEEF;
        assert_code(acc.replay().map(|_| ()), E_REPLAY_MISMATCH);
        assert_eq!(acc.ledger().count_kind(EvKind::ReplayMismatch), 1);
        // health 转红（重放不符计数）。
        let h = acc.health();
        assert!(!h.is_green);
        assert_eq!(h.replay_mismatches, 1);
    }

    // ── T9：双跑确定性对拍 ──
    #[test]
    fn dual_run_deterministic() {
        // 纯函数双跑必等。
        assert_eq!(rotation_fingerprint(), rotation_fingerprint());
        let mut acc = PanicRegAccount::new();
        // 未跑轮换前：账无轮换行，一致性只看双跑。
        let rep0 = ok(acc.dual_run_check());
        assert!(rep0.consistent);
        assert_eq!(rep0.fp_a, rep0.fp_b);
        // 跑完轮换：账轮换行与离线真值绑定。
        ok(acc.run_rotation_regression());
        let rep = ok(acc.dual_run_check());
        assert!(rep.consistent);
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.regbook_fp, rep.fp_a);
        assert_eq!(acc.ledger().count_kind(EvKind::DualRunOk), 2);
        // 漂移变体口径直证（构造注入——账外手工对拍不等即该错误）。
        let e = PanicRegError::DualRunDrift { code: E_DUAL_RUN_DRIFT, fp_a: 1, fp_b: 2 };
        assert_eq!(e.diag_code(), E_DUAL_RUN_DRIFT);
    }

    // ── T10：三重 fault 漂习（deliberate 通道定格 Halt） ──
    #[test]
    fn deliberate_triple_fault_reaches_halt() {
        let mut acc = PanicRegAccount::new();
        let stage = ok(acc.deliberate_triple_fault());
        assert_eq!(stage, STAGE_HALT);
        assert!(acc.halted());
        assert_eq!(acc.sim().verdicts(), 3);
        assert_eq!(acc.sim().double_faults(), 1);
        assert_eq!(acc.sim().triple_faults(), 1);
        assert!(acc.ledger().count_kind(EvKind::TripleFaultHalt) >= 1);
        assert!(acc.ledger().count_kind(EvKind::DoubleFault) >= 1);
        // 三裁决文案在账（mm 幂等去重与首裁同纹不另立行——至少 fb/ka 新纹）。
        assert!(acc.messages().len() >= 2);
        // 再漂习：挂死拒收。
        assert_code(acc.deliberate_triple_fault().map(|_| ()), E_HALT_LOCKED);
    }

    // ── T11：挂死后输入注入（四路全拒 + 保持） ──
    #[test]
    fn post_halt_input_injection_all_rejected() {
        let mut acc = PanicRegAccount::new();
        // 前置破：未挂死即注入 → E_NOT_HALTED。
        assert_code(acc.inject_post_halt_input(1).map(|_| ()), E_NOT_HALTED);
        ok(acc.deliberate_triple_fault());
        let out = ok(acc.inject_post_halt_input(TAG_HHDM.code));
        assert_eq!(out.attempts, 4);
        assert!(out.all_rejected, "挂死后四路输入必须全拒");
        assert!(out.still_halted, "挂死态必须保持");
        assert!(out.halt_maintained_total >= 4);
        assert!(acc.halted());
        // 再来一轮仍全拒（三重 fault 保持——任何后续调用都保持 Halt）。
        let out2 = ok(acc.inject_post_halt_input(TAG_FIRMWARE.code));
        assert!(out2.all_rejected && out2.still_halted);
        assert!(acc.ledger().count_kind(EvKind::InjectionDetected) >= 2);
    }

    // ── T12：挂死保持断言（逐轮核 + 账户级） ──
    #[test]
    fn halt_maintain_assertion_rounds() {
        let mut acc = PanicRegAccount::new();
        // 前置破：非挂死态执行断言 → E_NOT_HALTED。
        assert_code(acc.halt_maintain_assertion().map(|_| ()), E_NOT_HALTED);
        ok(acc.deliberate_triple_fault());
        let rounds = ok(acc.halt_maintain_assertion());
        assert_eq!(rounds, HALT_PROBE_ROUNDS);
        // 5 轮模拟器 + 1 路账户级 present 拒收。
        assert!(acc.sim().halt_maintained() >= HALT_PROBE_ROUNDS as u32 + 1);
        assert!(acc.halted());
        assert_eq!(acc.sim().stage_code(), STAGE_HALT);
        assert_eq!(acc.ledger().count_kind(EvKind::HaltMaintained), 1);
        // 重复断言仍保持。
        ok(acc.halt_maintain_assertion());
        assert!(acc.halted());
    }

    // ── T13：文案无 tag（无码/无位置）注入三档检出 ──
    #[test]
    fn message_without_element_injections() {
        let mut acc = PanicRegAccount::new();
        let before = acc.messages().len();
        // 三档逐路：检出码逐档对号。
        let o1 = ok(acc.inject_message_without_element(MissingElement::Tag));
        assert!(o1.detected);
        assert_eq!(o1.got_code, E_MSG_NO_TAG);
        let o2 = ok(acc.inject_message_without_element(MissingElement::Code));
        assert!(o2.detected);
        assert_eq!(o2.got_code, E_MSG_NO_CODE);
        let o3 = ok(acc.inject_message_without_element(MissingElement::Site));
        assert!(o3.detected);
        assert_eq!(o3.got_code, E_MSG_NO_SITE);
        // 拒收不入账：文案账行数不变。
        assert_eq!(acc.messages().len(), before);
        assert_eq!(acc.ledger().count_kind(EvKind::MsgRejected), 3);
        assert_eq!(acc.ledger().count_kind(EvKind::InjectionDetected), 3);
        // 完好文案正常入账（对照路）。
        let good = build_panic_message(TAG_MODULES, 1);
        ok(acc.message_enter(good));
        assert_eq!(acc.messages().len(), before + 1);
    }

    // ── T14：封板制（封板后写通道全拒 E_SEALED） ──
    #[test]
    fn seal_blocks_all_writes() {
        let mut acc = PanicRegAccount::new();
        present_all(&mut acc);
        ok(acc.seal());
        assert!(acc.is_sealed());
        assert_code(acc.present(1, 1, 64, 1).map(|_| ()), E_SEALED);
        assert_code(acc.gate_check().map(|_| ()), E_SEALED);
        assert_code(acc.inject_tag_missing(1).map(|_| ()), E_SEALED);
        assert_code(acc.run_rotation_regression().map(|_| ()), E_SEALED);
        assert_code(acc.deliberate_triple_fault().map(|_| ()), E_SEALED);
        assert_code(acc.dual_run_check().map(|_| ()), E_SEALED);
        // 幂等封板再封拒。
        assert_code(acc.seal(), E_SEALED);
        let h = acc.health();
        assert!(h.sealed);
    }

    // ── T15：文案账容量封顶 + 三路查询 ──
    #[test]
    fn message_account_capacity_and_queries() {
        let mut mac = MessageAccount::new();
        assert!(mac.is_empty());
        // 256 行（ordinal 逐行异——指纹互异）。
        let mut i: u32 = 1;
        while i <= MSG_CAP as u32 {
            let m = build_panic_message(TAG_MEMORY_MAP, i);
            ok(mac.enter(m));
            i += 1;
        }
        assert_eq!(mac.len(), MSG_CAP);
        // 第 257 行 → E_CAPACITY。
        let extra = build_panic_message(TAG_MEMORY_MAP, MSG_CAP as u32 + 1);
        assert_code(mac.enter(extra).map(|_| ()), E_CAPACITY);
        // 三路查询。
        assert!(mac.by_index(0).is_some());
        assert!(mac.by_index(MSG_CAP).is_none());
        let hit = mac.first_by_tag(1);
        assert!(hit.is_some());
        let fp9 = build_panic_message(TAG_MEMORY_MAP, 9).fingerprint;
        assert!(mac.by_fingerprint(fp9).is_some());
        assert!(mac.by_fingerprint(0x1234_5678).is_none());
        // 幂等：同纹重入返回既有序号、不加行。
        let again = build_panic_message(TAG_MEMORY_MAP, 9);
        let idx = ok(mac.enter(again));
        assert_eq!(idx, 8);
        assert_eq!(mac.len(), MSG_CAP);
        // 缺要素文案在账关即拒（不靠账户层）。
        let bad_text = "PANIC: no elements here";
        let bad = PanicMessage {
            text: bad_text.to_string(),
            tag_code: 1,
            tag_name: "memory_map".to_string(),
            diag_code: E_TAG_MISSING,
            site_text: tag_site_text(TAG_MEMORY_MAP),
            ordinal: 1,
            fingerprint: fnv1a64(bad_text.as_bytes()),
        };
        assert_code(mac.enter(bad).map(|_| ()), E_MSG_NO_TAG);
        assert_eq!(mac.len(), MSG_CAP);
    }

    // ── T16：手工缺失注入（in-machine，阶梯推进） ──
    #[test]
    fn manual_tag_missing_injection() {
        let mut acc = PanicRegAccount::new();
        present_all(&mut acc);
        let out = ok(acc.inject_tag_missing(TAG_FRAMEBUFFER.code));
        assert!(out.detected, "缺失注入必须检出");
        assert_eq!(out.tag_code, 2);
        assert_eq!(out.tag_name, "framebuffer");
        assert_eq!(out.stage_after, STAGE_FAULT1);
        assert!(out.msg_fingerprint != 0);
        // 回归账一行（非轮换），文案账含 fb 名。
        assert_eq!(acc.regbook().len(), 1);
        assert!(!acc.regbook().rows()[0].is_rotation);
        let m = acc.messages().first_by_tag(2).map(|m| m.text.clone());
        let text = match m {
            Some(t) => t,
            None => std::panic::panic_any("fb 文案必须在账"),
        };
        assert!(text.contains("framebuffer"));
        assert_eq!(acc.sim().stage_code(), STAGE_FAULT1);
        assert_eq!(acc.ledger().count_kind(EvKind::InjectionDetected), 1);
        // 再注入另一 tag：double fault（阶梯推进——panic 后不可恢复语义）。
        let out2 = ok(acc.inject_tag_missing(TAG_HHDM.code));
        assert!(out2.detected);
        assert_eq!(out2.stage_after, STAGE_FAULT2);
        // 第三次：triple fault 挂死。
        let out3 = ok(acc.inject_tag_missing(TAG_MODULES.code));
        assert!(out3.detected);
        assert_eq!(out3.stage_after, STAGE_HALT);
        assert!(acc.halted());
        // 第四次：挂死拒收（inject 通道也是输入）。
        assert_code(acc.inject_tag_missing(TAG_FIRMWARE.code).map(|_| ()), E_HALT_LOCKED);
        assert!(acc.halted());
    }

    // ── T17：六节回归报告（程序化生成，零手抄） ──
    #[test]
    fn regression_report_sections() {
        let mut acc = PanicRegAccount::new();
        ok(acc.run_rotation_regression());
        ok(acc.replay());
        ok(acc.dual_run_check());
        ok(acc.deliberate_triple_fault());
        ok(acc.halt_maintain_assertion());
        ok(acc.inject_post_halt_input(TAG_HHDM.code));
        let rep = acc.regression_report();
        // 六节齐。
        for mark in ["[一]", "[二]", "[三]", "[四]", "[五]", "[六]"] {
            assert!(rep.contains(mark), "报告缺节 {}", mark);
        }
        // 六 tag 名全可观测（文案节逐行列出）。
        for name in [
            TAG_MEMORY_MAP.name,
            TAG_FRAMEBUFFER.name,
            TAG_KERNEL_ADDRESS.name,
            TAG_HHDM.name,
            TAG_MODULES.name,
            TAG_FIRMWARE.name,
        ] {
            assert!(rep.contains(name), "报告缺 tag 名 {}", name);
        }
        // 关键结论行。
        assert!(rep.contains("检出率 10000ppm"));
        assert!(rep.contains("三重 fault 挂死"));
        assert!(rep.contains("行全符"));
        assert!(rep.contains("绿灯=true"));
        // 报告确定性：同账再生成逐字节一致。
        let rep2 = acc.regression_report();
        assert_eq!(rep, rep2);
    }

    // ── T18：账级指纹双跑一致（同操作序列两账对拍） ──
    #[test]
    fn account_fingerprint_dual_run() {
        let build = || -> u64 {
            let mut a = PanicRegAccount::new();
            ok(a.run_rotation_regression());
            ok(a.replay());
            ok(a.dual_run_check());
            ok(a.deliberate_triple_fault());
            ok(a.halt_maintain_assertion());
            ok(a.inject_post_halt_input(TAG_HHDM.code));
            a.fingerprint()
        };
        assert_eq!(build(), build(), "同操作序列两账指纹必须一致");
        // 不同操作序列必异纹。
        let mut b = PanicRegAccount::new();
        ok(b.run_rotation_regression());
        assert_ne!(build(), b.fingerprint());
    }

    // ── T19：health 全字段口径（干净链路 + 挂死链路） ──
    #[test]
    fn health_summary_fields() {
        // 干净链路：轮换+重放+双跑，未挂死，绿灯。
        let mut a = PanicRegAccount::new();
        ok(a.run_rotation_regression());
        ok(a.replay());
        ok(a.dual_run_check());
        let h = a.health();
        assert!(h.is_green);
        assert!(!h.halted);
        assert_eq!(h.regression_rows, 6);
        assert_eq!(h.messages, 6);
        assert_eq!(h.panic_verdicts, 0); // 离线通道不动本账模拟器
        assert_eq!(h.injections_missed, 0);
        assert_eq!(h.replay_mismatches, 0);
        assert_eq!(h.dual_run_drifts, 0);
        // 挂死链路：三重 fault 后绿灯口径（挂死是保词语义，不扣灯）。
        ok(a.deliberate_triple_fault());
        ok(a.halt_maintain_assertion());
        let h2 = a.health();
        assert!(h2.halted);
        assert_eq!(h2.triple_faults, 1);
        assert!(h2.halt_maintained >= HALT_PROBE_ROUNDS + 1);
        assert!(h2.is_green, "挂死不扣灯（语义保持而非故障）：{:?}", h2);
        // 呈现计数入 health。
        let mut c = PanicRegAccount::new();
        present_all(&mut c);
        let h3 = c.health();
        assert_eq!(h3.presents, 6);
        assert_eq!(h3.gate_passes, 0); // present 不触发闸检
    }

    // ── T20：闸检全过链路（六呈现 → Ok + 事件账） ──
    #[test]
    fn gate_check_pass_path() {
        let mut acc = PanicRegAccount::new();
        present_all(&mut acc);
        let gc = ok(acc.gate_check());
        assert!(gc.complete);
        assert_eq!(gc.missing_names.len(), 0);
        assert_eq!(acc.ledger().count_kind(EvKind::GateCheckPass), 1);
        assert_eq!(acc.sim().stage_code(), STAGE_NORMAL); // 全过不入 panic 路径
        assert_eq!(acc.messages().len(), 0);
        // 幂等复跑全过。
        ok(acc.gate_check());
        assert_eq!(acc.ledger().count_kind(EvKind::GateCheckPass), 2);
        let h = acc.health();
        assert!(h.is_green);
    }
}
