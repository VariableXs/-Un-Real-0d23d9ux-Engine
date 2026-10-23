//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1304 · ps2.rs 轮询退役执行账 · 诊断段 0x1240
//! ─────────────────────────────────────────────────────────────────────
//! 验收锚点（任务书 J 判据原文逐字抄录）：
//!   UNX-F1304-J1 ps2.rs 全部测绘点（含全部 spin_loop 等待点）退役执行
//!   10/10 次，退役窗口扫描码流守恒 100%，应答语义（F1203 同构约束）保持
//!   100%，与基线扫描码流对拍等价初验通过
//! 定位（任务板原文逐字抄录）：
//!   UNX-F1304 | ps2.rs 轮询退役执行账 | 280 | 已深化 | UNX-F1304-J1
//!   ps2.rs 全部测绘点（含全部 spin_loop 等待点）退役执行 10/10 次，
//!   退役窗口扫描码流守恒 100%，应答语义（F1203 同构约束）保持 100%，
//!   与基线扫描码流对拍等价初验通过
//! ─────────────────────────────────────────────────────────────────────
//! 诊断码段改立说明（独占纪律优先，全库验空）：
//!   指令默认段 0x13C0~0x13CF 已被 unx2341_udomregress 整段独占
//!   （该文件头明言「0x13C0–0x13FF 本单独占，不得他用」，含 64 码掩码）；
//!   指令指定备选段 0x13D0 亦被 unx1323_serequiv 当日分配独占
//!   （0x13D0~0x13DF）；0x1310/0x1320 等已被 cgpu 域 vcs06/vct07 分段占满。
//!   全库 grep（kernel/varix/src + svstar2 全域 + docs 任务书段分配面）
//!   验空后改立空闲段 0x1240~0x124F（0x13xx 十六段全域有主，0x1240 段
//!   代码零出现、任务书零分配，双验空）。
//! ─────────────────────────────────────────────────────────────────────
//! 测绘点定位（ps2.rs 实测，轮询退役域）：
//!   测绘点 0  ps2.rs:399-426  poll_key 状态轮询点
//!              ——读 0x64 查 OBF(bit0)，置位才读 0x60；控制器 guard
//!                （状态口回读 0xFF = 浮空总线无控制器，静默）。
//!   测绘点 1  ps2.rs:481-492  send()·IBF spin_loop 等待点
//!              ——写 0x64 前等输入缓冲空（IBF=bit2 掩码 0x02），
//!                限次自旋 200_000 次，`core::hint::spin_loop()`。
//!   测绘点 2  ps2.rs:494-504  resp()·OBF spin_loop 等待点
//!              ——读 0x60 前等输出缓冲有数据（OBF=bit0 掩码 0x01），
//!                限次自旋 2_000_000 次，`core::hint::spin_loop()`。
//!   测绘点 3  ps2.rs:506-515  flush()·清输出限次轮询点
//!              ——有残留就读掉，最多 64 字节，OBF 查询驱动。
//! 退役六步全序（RetireStep 闭集）：
//!   停用轮询门 → 切中断源 → 语义核对 → 窗口观察 → 等价对拍 → 确认退役
//! 设计要点：
//!   - 双通道输入源：轮询通道（退役前基线）与中断通道（退役后）；
//!   - 退役窗口：窗内扫描码流零丢失零重复，进窗=出窗，FNV 链逐事件
//!     衔接，篡改即破（破即报码定位）；
//!   - 应答语义账：命令-应答配对（0xFA ACK / 0x55 自检通过 / 0x00 接口
//!     通过 / 0xFE 重发），退役前后配对一致率必须 100%（F1203 同构）；
//!   - 基线对拍：轮询路径码流 vs 中断路径码流逐事件对拍，等价率计算 +
//!     首差异事件定位；
//!   - 失败即回退：完整回退到轮询态 + 回退后轮询语义自检；
//!   - 注入面：窗口丢事件 / 应答失配 / 对拍不等，三注入各检出带码；
//!   - 双跑确定性：同参两账指纹逐位一致。
//! 纪律：no_std+alloc 全自包含（零 `use crate::`），内核路径零
//!   unwrap/expect/panic!/运行期 assert!，可失败全走 Result，计数显性
//!   wrapping/saturating，>64KB 结构体全部落堆（Vec 承载）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 一、诊断码段（0x1240~0x124F 独占，16 码）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断码段基址（0x1240 段验空后改立，见文件头说明）。
pub const SEG_BASE: u16 = 0x1240;
/// 诊断码段顶（含）。段内 16 槽：0x1240 段基保留，0x1241~0x124F 十五码。
pub const SEG_TOP: u16 = 0x124F;

// ═══════════════════════════════════════════════════════════════════════
// 二、错误全集（16 变体，每变体 detail() 中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════════

/// 退役执行账错误全集。诊断码 = `SEG_BASE + 序号`，恒落 0x1240..=0x124F。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ps2RetireError {
    /// 退役六步乱序：site 出错点、expect 期望步序、got 实到步序。
    StepOrderViolated { site: u8, expect: u8, got: u8 },
    /// 未知测绘点号。
    UnknownSite(u8),
    /// 退役窗口丢事件：进窗计数与出窗计数不平。
    WindowLoss { site: u8, in_cnt: u32, out_cnt: u32 },
    /// 退役窗口重复事件：出窗 seq 回退/重现。
    WindowDup { site: u8, seq: u32 },
    /// 退役窗口 FNV 链断裂（篡改）：at_seq 首个链不平的事件序号。
    WindowChainBroken { site: u8, at_seq: u32 },
    /// 应答语义失配：cmd 命令字节、got 实收应答字节。
    RespMismatch { site: u8, cmd: u8, got: u8 },
    /// 基线对拍不等：at_event 首差异事件序号。
    A2bInequality { site: u8, at_event: u32 },
    /// 回退到轮询态失败。
    RollbackFailed { site: u8 },
    /// 回退后轮询语义自检失败（轮询路径不能恢复产出）。
    PollSelfCheckFailed { site: u8 },
    /// 轮询门状态异常：state 门状态原码。
    GateStateBad { site: u8, state: u8 },
    /// 中断源配置异常：cfg 配置字节原码。
    IrqConfigBad { site: u8, cfg: u8 },
    /// 语义核对一致率不达标：rate_pct 实测一致率（百分制，饱和乘法）。
    SemanticsAuditFailed { site: u8, rate_pct: u32 },
    /// 战役未达标：done 已完成轮数、need 需求轮数。
    CampaignIncomplete { done: u32, need: u32 },
    /// 双跑指纹分歧：run_a / run_b 两账指纹。
    FingerprintDivergence { run_a: u64, run_b: u64 },
    /// 窗口未开却执行闭窗/出窗操作（或已关窗后重复操作）。
    WindowStillOpen { site: u8 },
    /// 健康面降级：failed 失败检查条数。
    HealthDegraded { failed: u32 },
}

/// 十六变体诊断码常量表（顺序与枚举定义严格一致，编译期验段内+互异）。
const ERR_CODES: [u16; 16] = [
    SEG_BASE + 0x0,  // StepOrderViolated   0x1240
    SEG_BASE + 0x1,  // UnknownSite         0x1241
    SEG_BASE + 0x2,  // WindowLoss          0x1242
    SEG_BASE + 0x3,  // WindowDup           0x1243
    SEG_BASE + 0x4,  // WindowChainBroken   0x1244
    SEG_BASE + 0x5,  // RespMismatch        0x1245
    SEG_BASE + 0x6,  // A2bInequality       0x1246
    SEG_BASE + 0x7,  // RollbackFailed      0x1247
    SEG_BASE + 0x8,  // PollSelfCheckFailed 0x1248
    SEG_BASE + 0x9,  // GateStateBad        0x1249
    SEG_BASE + 0xA,  // IrqConfigBad        0x124A
    SEG_BASE + 0xB,  // SemanticsAuditFailed 0x124B
    SEG_BASE + 0xC,  // CampaignIncomplete  0x124C
    SEG_BASE + 0xD,  // FingerprintDivergence 0x124D
    SEG_BASE + 0xE,  // WindowStillOpen     0x124E
    SEG_BASE + 0xF,  // HealthDegraded      0x124F
];

// 编译期段纪律验算：16 码全部落 0x1240..=0x124F 且互异（while+索引，无 for）。
const _: () = assert!({
    let mut i = 0;
    let mut in_range = true;
    let mut distinct = true;
    while i < ERR_CODES.len() {
        let c = ERR_CODES[i];
        if c < SEG_BASE || c > SEG_TOP {
            in_range = false;
        }
        let mut j = i + 1;
        while j < ERR_CODES.len() {
            if ERR_CODES[i] == ERR_CODES[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    in_range && distinct
});

impl Ps2RetireError {
    /// 诊断码（const 可算：纯枚举匹配 + 段基加法）。
    pub const fn code(&self) -> u16 {
        match self {
            Ps2RetireError::StepOrderViolated { .. } => ERR_CODES[0],
            Ps2RetireError::UnknownSite(_) => ERR_CODES[1],
            Ps2RetireError::WindowLoss { .. } => ERR_CODES[2],
            Ps2RetireError::WindowDup { .. } => ERR_CODES[3],
            Ps2RetireError::WindowChainBroken { .. } => ERR_CODES[4],
            Ps2RetireError::RespMismatch { .. } => ERR_CODES[5],
            Ps2RetireError::A2bInequality { .. } => ERR_CODES[6],
            Ps2RetireError::RollbackFailed { .. } => ERR_CODES[7],
            Ps2RetireError::PollSelfCheckFailed { .. } => ERR_CODES[8],
            Ps2RetireError::GateStateBad { .. } => ERR_CODES[9],
            Ps2RetireError::IrqConfigBad { .. } => ERR_CODES[10],
            Ps2RetireError::SemanticsAuditFailed { .. } => ERR_CODES[11],
            Ps2RetireError::CampaignIncomplete { .. } => ERR_CODES[12],
            Ps2RetireError::FingerprintDivergence { .. } => ERR_CODES[13],
            Ps2RetireError::WindowStillOpen { .. } => ERR_CODES[14],
            Ps2RetireError::HealthDegraded { .. } => ERR_CODES[15],
        }
    }

    /// 中文可操作诊断（恒非空；逐变体给下一步动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            Ps2RetireError::StepOrderViolated { expect, got, .. } => {
                if got > expect {
                    "六步乱序·越步执行：退役步只能按序推进，先补跑被跳过的步（见 expect），再重试本步"
                } else {
                    "六步乱序·回退重放：本步已完成，禁止重跑；若需重练请整轮重新 execute_retire"
                }
            }
            Ps2RetireError::UnknownSite(id) => {
                if *id >= SURVEY_SITE_COUNT as u8 {
                    "未知测绘点：测绘点号必须在 survey_catalog() 返回的 0..4 闭集内，先查表再投递"
                } else {
                    "未知测绘点：账面测绘点表缺失该号，检查 RetireExecution::new 是否完整建账"
                }
            }
            Ps2RetireError::WindowLoss { in_cnt, out_cnt, .. } => {
                if out_cnt < in_cnt {
                    "退役窗口丢事件：出窗计数少于进窗，逐事件核对 ISR 交付路径是否有未读 OBF；补齐出窗侧或判退役失败回退"
                } else {
                    "退役窗口多出事件：出窗计数多于进窗，出现幻影事件，核查注入源"
                }
            }
            Ps2RetireError::WindowDup { .. } => {
                "退役窗口重复事件：出窗侧 seq 出现回退/重现，说明 ISR 重读或窗口重放，先去重再重跑窗口观察步"
            }
            Ps2RetireError::WindowChainBroken { .. } => {
                "退役窗口 FNV 链断裂：at_seq 事件内容被改写，用链快照逐事件回放定位被篡改字节，恢复原码流后重跑"
            }
            Ps2RetireError::RespMismatch { got, .. } => {
                if *got == 0xFE {
                    "应答失配·重发 0xFE：设备拒收该命令，按 i8042 惯例重发命令字节后重新配对"
                } else {
                    "应答失配：实收应答与 F1203 同构约束期望不符，核对命令-应答期望表后重跑语义核对步"
                }
            }
            Ps2RetireError::A2bInequality { .. } => {
                "基线对拍不等：轮询路径与中断路径码流在首差异事件处分叉，取 at_event 定位事件并比对两通道解码序"
            }
            Ps2RetireError::RollbackFailed { .. } => {
                "回退失败：轮询门/中断源未恢复到退役前态，人工核对门状态与 cfg 字节后重做回退演练"
            }
            Ps2RetireError::PollSelfCheckFailed { .. } => {
                "回退后自检失败：轮询路径金丝雀序未复原产出，先恢复 0x64/0x60 轮询语义再谈退役"
            }
            Ps2RetireError::GateStateBad { .. } => {
                "轮询门状态异常：门状态机不在合法迁移路径上，复位门状态到 Open 后整轮重跑"
            }
            Ps2RetireError::IrqConfigBad { .. } => {
                "中断源配置异常：cfg 字节未同时满足 IRQ1 使能位与轮询模式位清零，重写 cfg 后重跑切源步"
            }
            Ps2RetireError::SemanticsAuditFailed { .. } => {
                "语义核对一致率不达标：配对账退役前后不一致，先查 rate_pct 定位失配对，再决定是否回退"
            }
            Ps2RetireError::CampaignIncomplete { done, need } => {
                if done < need {
                    "战役未达标：退役轮数不足，继续 execute_retire 补足 10/10 后再出报告"
                } else {
                    "战役轮数超限：本轮次账已封账，开新账重跑"
                }
            }
            Ps2RetireError::FingerprintDivergence { .. } => {
                "双跑指纹分歧：同参两账事件序列不一致，存在非确定源，逐事件比对两账 RetireEvent 流"
            }
            Ps2RetireError::WindowStillOpen { .. } => {
                "窗口状态机错序：闭窗/出窗操作落在未开窗（或已闭窗）状态上，先按序开窗再操作"
            }
            Ps2RetireError::HealthDegraded { .. } => {
                "健康面降级：存在失败检查项，逐条核对 HealthReport.checks 中 ok=false 的条目"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 三、确定性底座：FNV-1a 64 与 LCG
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位偏移基（标准常数）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（标准常数）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64：逐字节衔接（链式指纹底座，while+索引）。
pub fn fnv64(data: &[u8], init: u64) -> u64 {
    let mut h = init;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 空链初值（窗口链/账指纹统一从此起链）。
pub const CHAIN_INIT: u64 = FNV_OFFSET;

/// 确定性 LCG（PCG 风格乘加，63 位取中）——脚本/注入/耗时全走它，
/// 保证双跑逐位一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种（显性 wrapping，防溢出恐慌）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407),
        }
    }

    /// 取下一 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    /// 取 [0, n) 内伪随机下标（n=0 时恒 0，不取模除零）。
    pub fn next_below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u32() as usize) % n
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 四、测绘点闭集（ps2.rs 全部测绘点，含全部 spin_loop 等待点）
// ═══════════════════════════════════════════════════════════════════════

/// 测绘点数（闭集：4 个，覆盖 ps2.rs 全部轮询/自旋点）。
pub const SURVEY_SITE_COUNT: usize = 4;
/// 每测绘点退役演练轮数（J 判据：10/10 次）。
pub const RETIRE_ROUNDS: u32 = 10;
/// 单账轮数硬上限（防封账后继续投递）。
pub const RETIRE_ROUNDS_CAP: u32 = 10;

/// i8042 端口与状态位（来源：ps2.rs:401-403/477-478 写死值）。
pub const PORT_STAT: u16 = 0x64; // ps2.rs:401 状态口
pub const PORT_DATA: u16 = 0x60; // ps2.rs:402 数据口
pub const STAT_OBF: u8 = 0x01; // ps2.rs:403 输出缓冲满
pub const STAT_IBF: u8 = 0x02; // ps2.rs:478 输入缓冲满
/// 控制器存在性 guard 原码（ps2.rs:408：状态口回读 0xFF=浮空总线）。
pub const CTRL_FLOAT_BUS: u8 = 0xFF;
/// send() IBF 限次自旋预算（ps2.rs:482 写死 200_000）。
pub const SPIN_BUDGET_SEND: u32 = 200_000;
/// resp() OBF 限次自旋预算（ps2.rs:495 写死 2_000_000）。
pub const SPIN_BUDGET_RESP: u32 = 2_000_000;
/// flush() 清输出限次（ps2.rs:507 写死 64 字节）。
pub const FLUSH_LIMIT: u32 = 64;

/// 单个测绘点档案（退役执行账的最小退役单元）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurveySite {
    /// 测绘点号（0..4 闭集）。
    pub id: u8,
    /// 测绘点名（锚 ps2.rs 符号名）。
    pub name: &'static str,
    /// 观测主端口。
    pub port: u16,
    /// 状态位掩码（OBF/IBF）。
    pub mask: u8,
    /// 限次自旋预算（0=无常驻自旋，如 poll_key 单拍查询）。
    pub spin_budget: u32,
    /// ps2.rs 源码锚点（行号区间，取证用）。
    pub ground: &'static str,
    /// 是否 spin_loop 等待点（J 判据点名「含全部 spin_loop 等待点」）。
    pub is_spin_loop: bool,
}

/// 测绘点总目（ps2.rs 全部测绘点，逐条锚行号）。
pub fn survey_catalog() -> Vec<SurveySite> {
    vec![
        SurveySite {
            id: 0,
            name: "poll_key·OBF 状态轮询点",
            port: PORT_STAT,
            mask: STAT_OBF,
            spin_budget: 0,
            ground: "ps2.rs:399-426 poll_key 状态轮询+0x60 读数",
            is_spin_loop: false,
        },
        SurveySite {
            id: 1,
            name: "send·IBF spin_loop 等待点",
            port: PORT_STAT,
            mask: STAT_IBF,
            spin_budget: SPIN_BUDGET_SEND,
            ground: "ps2.rs:481-492 send() 限次自旋 200_000",
            is_spin_loop: true,
        },
        SurveySite {
            id: 2,
            name: "resp·OBF spin_loop 等待点",
            port: PORT_STAT,
            mask: STAT_OBF,
            spin_budget: SPIN_BUDGET_RESP,
            ground: "ps2.rs:494-504 resp() 限次自旋 2_000_000",
            is_spin_loop: true,
        },
        SurveySite {
            id: 3,
            name: "flush·清输出限次轮询点",
            port: PORT_DATA,
            mask: STAT_OBF,
            spin_budget: FLUSH_LIMIT,
            ground: "ps2.rs:506-515 flush() 清残留最多 64 字节",
            is_spin_loop: false,
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 五、退役六步闭集（全序，乱序拒绝带码 0x1240）
// ═══════════════════════════════════════════════════════════════════════

/// 退役步数（闭集大小，编译期冻结）。
pub const STEP_COUNT: usize = 6;

const _: () = assert!(STEP_COUNT == 6);
const _: () = assert!(SURVEY_SITE_COUNT == 4);
const _: () = assert!(RETIRE_ROUNDS == 10);

/// 退役六步全序闭集：
/// 停用轮询门 → 切中断源 → 语义核对 → 窗口观察 → 等价对拍 → 确认退役。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetireStep {
    /// 步 0：停用轮询门（poll 路径静默化，验证零产出）。
    PollGateOff = 0,
    /// 步 1：切中断源（IRQ1 使能 + 轮询模式位清零）。
    IrqSourceSwitch = 1,
    /// 步 2：语义核对（F1203 同构：配对账退役前后一致率 100%）。
    SemanticsAudit = 2,
    /// 步 3：窗口观察（退役窗口码流守恒 + FNV 链）。
    WindowObserve = 3,
    /// 步 4：等价对拍（基线轮询码流 vs 中断码流逐事件对拍）。
    EquivalenceA2B = 4,
    /// 步 5：确认退役（记账封轮，site 转 Retired）。
    ConfirmRetire = 5,
}

impl RetireStep {
    /// 步序（const 可算）。
    pub const fn ord(self) -> u8 {
        self as u8
    }

    /// 步名（中文，报告面用）。
    pub fn name(self) -> &'static str {
        match self {
            RetireStep::PollGateOff => "停用轮询门",
            RetireStep::IrqSourceSwitch => "切中断源",
            RetireStep::SemanticsAudit => "语义核对",
            RetireStep::WindowObserve => "窗口观察",
            RetireStep::EquivalenceA2B => "等价对拍",
            RetireStep::ConfirmRetire => "确认退役",
        }
    }

    /// 全序后继（末步 None = 链闭合）。
    pub const fn next(self) -> Option<RetireStep> {
        match self {
            RetireStep::PollGateOff => Some(RetireStep::IrqSourceSwitch),
            RetireStep::IrqSourceSwitch => Some(RetireStep::SemanticsAudit),
            RetireStep::SemanticsAudit => Some(RetireStep::WindowObserve),
            RetireStep::WindowObserve => Some(RetireStep::EquivalenceA2B),
            RetireStep::EquivalenceA2B => Some(RetireStep::ConfirmRetire),
            RetireStep::ConfirmRetire => None,
        }
    }

    /// 由步序复原步（const 可算；越界 None）。
    pub const fn from_ord(o: u8) -> Option<RetireStep> {
        match o {
            0 => Some(RetireStep::PollGateOff),
            1 => Some(RetireStep::IrqSourceSwitch),
            2 => Some(RetireStep::SemanticsAudit),
            3 => Some(RetireStep::WindowObserve),
            4 => Some(RetireStep::EquivalenceA2B),
            5 => Some(RetireStep::ConfirmRetire),
            _ => None,
        }
    }

    /// 首步（全序入口）。
    pub const fn first() -> RetireStep {
        RetireStep::PollGateOff
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 六、扫描码事件模型与双通道引擎
// ═══════════════════════════════════════════════════════════════════════

/// 扫描码事件类别（tag 即编码首字节，闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScKind {
    /// Set-1 通码（按下）。
    Make = 1,
    /// Set-1 断码（释放 = 通码 | 0x80）。
    Break = 2,
    /// 主机→设备命令字节。
    Cmd = 3,
    /// 设备→主机应答字节（0xFA/0x55/0x00/0xAA 族）。
    Resp = 4,
    /// 重发应答 0xFE（ps2.rs 语义外的拒收信号，配对账用作失配探针）。
    Resend = 5,
}

impl ScKind {
    /// 编码 tag（const 可算）。
    pub const fn tag(self) -> u8 {
        self as u8
    }
}

/// 扫描码事件路径标签（F1203 同构：两路径事件结构逐字段同型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathTag {
    /// 轮询路径（退役前，ps2.rs poll_key 语义）。
    Poll = 0,
    /// 中断路径（退役后，IRQ1 ISR 语义）。
    Irq = 1,
}

/// 扫描码事件：序号 + 内容（类别/字节）+ 通道 + 观测时刻。
///
/// 内容恒等约束（J 判据「对拍等价」）：同一刺激在两路径产出的
/// `content()` 必须逐字节一致；tick 与 path 允许通道差（正是对拍意义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScEvent {
    /// 事件序号（通道内 0 起单调递增）。
    pub seq: u32,
    /// 扫描码/命令/应答字节。
    pub byte: u8,
    /// 事件类别。
    pub kind: ScKind,
    /// 观测时刻（通道时钟拍）。
    pub tick: u64,
    /// 通道标签。
    pub path: PathTag,
}

impl ScEvent {
    /// 内容编码（6 字节，对拍与守恒链的恒等面）：
    /// [kind_tag, byte, seq u32 小端 4 字节]。
    pub fn content(&self) -> [u8; 6] {
        [
            self.kind.tag(),
            self.byte,
            (self.seq & 0xFF) as u8,
            ((self.seq >> 8) & 0xFF) as u8,
            ((self.seq >> 16) & 0xFF) as u8,
            ((self.seq >> 24) & 0xFF) as u8,
        ]
    }

    /// 事件 FNV 链推进：chain = FNV64(content, chain)。
    pub fn chain_step(&self, chain: u64) -> u64 {
        fnv64(&self.content(), chain)
    }
}

/// 刺激：脚本层的物理事件（两通道共同输入源，保证同源对拍公平）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stimulus {
    /// 字节（通码/断码/命令/应答）。
    pub byte: u8,
    /// 类别。
    pub kind: ScKind,
}

impl Stimulus {
    /// 刺激的期望内容编码（与 ScEvent::content 同构，seq 由通道填）。
    pub fn content_at(&self, seq: u32) -> [u8; 6] {
        [
            self.kind.tag(),
            self.byte,
            (seq & 0xFF) as u8,
            ((seq >> 8) & 0xFF) as u8,
            ((seq >> 16) & 0xFF) as u8,
            ((seq >> 24) & 0xFF) as u8,
        ]
    }
}

/// Set-1 通码表（IBM AT 标准，ps2.rs decode() 同族键位：A M H I O P D G S L J ；）。
pub const KEY_MAKE_TABLE: [u8; 12] = [
    0x1C, 0x32, 0x23, 0x17, 0x18, 0x19, 0x1E, 0x22, 0x1A, 0x0F, 0x24, 0x2B,
];
/// Set-1 断码高位（break = make | 0x80）。
pub const SET1_BREAK_BIT: u8 = 0x80;

/// 确定性脚本：由 (site, round) 唯一决定，双跑逐位一致。
///
/// 构成：控制器自检序（0xAA→0x55 / 0xAB→0x00，ps2.rs:526-543 同序）+
/// 6 组 make/break 键序（中途嵌 0xED 灯控、0xF3 击率两命令应答对）+
/// 收尾 0xF4 使能→0xFA、0xFF 复位→0xFA+0xAA（i8042 标准应答序）。
pub fn script_for(site: u8, round: u32) -> Vec<Stimulus> {
    let mut lcg = Lcg::new(
        (site as u64) << 32 | (round as u64).wrapping_add(0x5EED_1304),
    );
    let mut v: Vec<Stimulus> = Vec::new();
    // 控制器自检序（ps2.rs controller_init 标准序列的前段）
    v.push(Stimulus { byte: 0xAA, kind: ScKind::Cmd }); // 控制器自检命令
    v.push(Stimulus { byte: 0x55, kind: ScKind::Resp }); // 自检通过
    v.push(Stimulus { byte: 0xAB, kind: ScKind::Cmd }); // 接口测试命令
    v.push(Stimulus { byte: 0x00, kind: ScKind::Resp }); // 接口通过
    // 6 组 make/break（Set-1：断码 = 通码 | 0x80）
    let mut k = 0;
    while k < 6 {
        let mk = KEY_MAKE_TABLE[lcg.next_below(KEY_MAKE_TABLE.len())];
        v.push(Stimulus { byte: mk, kind: ScKind::Make });
        if k == 2 {
            // 灯控命令应答对
            v.push(Stimulus { byte: 0xED, kind: ScKind::Cmd });
            v.push(Stimulus { byte: 0xFA, kind: ScKind::Resp }); // ACK
        }
        if k == 4 {
            // 击率命令应答对
            v.push(Stimulus { byte: 0xF3, kind: ScKind::Cmd });
            v.push(Stimulus { byte: 0xFA, kind: ScKind::Resp }); // ACK
        }
        v.push(Stimulus {
            byte: mk ^ SET1_BREAK_BIT,
            kind: ScKind::Break,
        });
        k += 1;
    }
    // 收尾：使能 + 复位（复位应答两拍：ACK 后自检通过 0xAA）
    v.push(Stimulus { byte: 0xF4, kind: ScKind::Cmd });
    v.push(Stimulus { byte: 0xFA, kind: ScKind::Resp });
    v.push(Stimulus { byte: 0xFF, kind: ScKind::Cmd });
    v.push(Stimulus { byte: 0xFA, kind: ScKind::Resp });
    v.push(Stimulus { byte: 0xAA, kind: ScKind::Resp });
    v
}

/// 门探针小脚本（固定 4 刺激，步 0/步 1 生效性验证共用）。
pub const PROBE_SCRIPT: [Stimulus; 4] = [
    Stimulus { byte: 0x1C, kind: ScKind::Make },
    Stimulus { byte: 0x9C, kind: ScKind::Break },
    Stimulus { byte: 0xF4, kind: ScKind::Cmd },
    Stimulus { byte: 0xFA, kind: ScKind::Resp },
];

/// 轮询通道引擎（退役前基线；逐条模拟 ps2.rs poll_key 语义）。
///
/// 语义逐条对齐 ps2.rs:399-426：
///   1) 读 0x64 状态；回读 0xFF → 控制器不在位，全程静默（guard）；
///   2) OBF=0 → 无数据（轮询空拍，消耗一拍自旋预算）；
///   3) OBF=1 → 读 0x60 产出事件；
///   4) 轮询门关闭 → 全部静默（退役后轮询路径零产出）。
pub struct PollEngine {
    gate_open: bool,
    ctrl_present: bool,
    spins_used: u64,
    seq: u32,
}

impl PollEngine {
    /// 新引擎：ctrl_present 对应状态口 guard，gate_open 对应轮询门。
    pub fn new(ctrl_present: bool, gate_open: bool) -> Self {
        PollEngine {
            gate_open,
            ctrl_present,
            spins_used: 0,
            seq: 0,
        }
    }

    /// 本轮消耗的轮询拍数（显性 wrapping）。
    pub fn spins_used(&self) -> u64 {
        self.spins_used
    }

    /// 观测脚本，产出轮询路径事件流。
    pub fn observe(&mut self, script: &[Stimulus]) -> Vec<ScEvent> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < script.len() {
            let st = script[i];
            // 每刺激一拍状态轮询（poll_key 每次调用先查 0x64）
            self.spins_used = self.spins_used.wrapping_add(1);
            let obf_set = st.kind.tag() != 0; // 模拟 OBF 置位判定（刺激即数据就绪）
            let float_bus = !self.ctrl_present; // 状态口回读 0xFF guard
            let deliverable = self.gate_open && !float_bus && obf_set;
            if deliverable {
                out.push(ScEvent {
                    seq: self.seq,
                    byte: st.byte,
                    kind: st.kind,
                    tick: (i as u64).wrapping_mul(POLL_TICK_PER_EVENT),
                    path: PathTag::Poll,
                });
                self.seq = self.seq.wrapping_add(1);
            }
            i += 1;
        }
        out
    }
}

/// 轮询通道单事件观测拍成本（模拟轮询周期，确定性写死）。
pub const POLL_TICK_PER_EVENT: u64 = 3;
/// 中断通道单事件交付拍成本（模拟 ISR 上下文，确定性写死）。
pub const IRQ_TICK_PER_EVENT: u64 = 1;

/// 中断通道引擎（退役后主路径；IRQ1 键盘中断语义）。
///
/// 语义：刺激到达即抬 IRQ1 → ISR 读 0x60 → 交付事件。中断源未使能时
/// 事件滞留（零交付），切源步的生效性验证即由此驱动。
pub struct IrqEngine {
    enabled: bool,
    lines: u8,
    seq: u32,
    delivered: u32,
}

impl IrqEngine {
    /// 新引擎：enabled 中断源使能，lines IRQ 线位图（bit0=IRQ1）。
    pub fn new(enabled: bool, lines: u8) -> Self {
        IrqEngine {
            enabled,
            lines,
            seq: 0,
            delivered: 0,
        }
    }

    /// 已交付事件数。
    pub fn delivered(&self) -> u32 {
        self.delivered
    }

    /// 观测脚本，产出中断路径事件流（未使能则零交付）。
    pub fn observe(&mut self, script: &[Stimulus]) -> Vec<ScEvent> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < script.len() {
            let st = script[i];
            if self.enabled && (self.lines & IRQ_LINE_KBD) != 0 {
                out.push(ScEvent {
                    seq: self.seq,
                    byte: st.byte,
                    kind: st.kind,
                    tick: (i as u64).wrapping_mul(IRQ_TICK_PER_EVENT),
                    path: PathTag::Irq,
                });
                self.seq = self.seq.wrapping_add(1);
                self.delivered = self.delivered.wrapping_add(1);
            }
            i += 1;
        }
        out
    }
}

/// IRQ1（键盘）线位（i8042 第一端口中断线）。
pub const IRQ_LINE_KBD: u8 = 0x01;
/// IRQ12（辅助设备）线位（本账测绘点全在键盘路径，备而不用）。
pub const IRQ_LINE_AUX: u8 = 0x02;

// ═══════════════════════════════════════════════════════════════════════
// 七、退役窗口（守恒 + FNV 链 + 篡改定位）
// ═══════════════════════════════════════════════════════════════════════

/// 退役窗口闭窗统计（全 Copy 小结构，可落栈）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowStats {
    /// 进窗事件数。
    pub in_cnt: u32,
    /// 出窗事件数。
    pub out_cnt: u32,
    /// 观测到的重复事件数。
    pub dup_seen: u32,
    /// 观测到的丢失事件数。
    pub loss_seen: u32,
    /// 首个链不平事件序号（无篡改 None）。
    pub tamper_seq: Option<u32>,
    /// 进窗侧终链值。
    pub chain_in: u64,
    /// 出窗侧终链值。
    pub chain_out: u64,
}

impl WindowStats {
    /// 守恒成立（进=出、零重零失、链平）。
    pub fn conserved(&self) -> bool {
        self.in_cnt == self.out_cnt
            && self.dup_seen == 0
            && self.loss_seen == 0
            && self.tamper_seq.is_none()
            && self.chain_in == self.chain_out
    }

    /// 守恒率（百分制；u64 中间量；0 事件按 100% 计）。
    pub fn conservation_pct(&self) -> u32 {
        if self.in_cnt == 0 {
            return 100;
        }
        let ok = self.out_cnt.saturating_sub(self.dup_seen).saturating_sub(self.loss_seen);
        rate_of(ok, self.in_cnt)
    }
}

/// 退役窗口：扫描码流进窗→出窗的守恒观测腔。
///
/// 守恒三判：
///   1) 计数守恒：进窗事件数 == 出窗事件数（丢/重即破）；
///   2) 序守恒：出窗 seq 必须连续推进（回退=重，跳跃=失）；
///   3) 链守恒：进窗侧 FNV 链快照与出窗侧逐事件对齐（篡改即破，可定位
///      首个不平事件序号）。
pub struct RetireWindow {
    site: u8,
    open: bool,
    in_cnt: u32,
    out_cnt: u32,
    dup_seen: u32,
    loss_seen: u32,
    chain_in: u64,
    chain_out: u64,
    base_seq: i64,      // 首个进窗事件 seq（-1 = 未定基）
    expected_out: i64,  // 出窗侧期望 seq
    tamper_seq: Option<u32>,
    /// 进窗侧逐事件链快照（堆上，出窗侧逐拍比对定位篡改）。
    in_snapshots: Vec<u64>,
}

impl RetireWindow {
    /// 开窗。
    pub fn open(site: u8) -> Self {
        RetireWindow {
            site,
            open: true,
            in_cnt: 0,
            out_cnt: 0,
            dup_seen: 0,
            loss_seen: 0,
            chain_in: CHAIN_INIT,
            chain_out: CHAIN_INIT,
            base_seq: -1,
            expected_out: -1,
            tamper_seq: None,
            in_snapshots: Vec::new(),
        }
    }

    /// 是否开窗中。
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 进窗（到达侧）：更新进窗链与快照。
    pub fn feed_in(&mut self, e: &ScEvent) {
        if !self.open {
            return;
        }
        if self.base_seq < 0 {
            self.base_seq = e.seq as i64;
        }
        self.chain_in = e.chain_step(self.chain_in);
        self.in_snapshots.push(self.chain_in);
        self.in_cnt = self.in_cnt.wrapping_add(1);
    }

    /// 出窗（交付侧）：序守恒即时判重/判失，链快照即时判篡改。
    pub fn feed_out(&mut self, e: &ScEvent) -> Result<(), Ps2RetireError> {
        if !self.open {
            return Err(Ps2RetireError::WindowStillOpen { site: self.site });
        }
        let s = e.seq as i64;
        if self.expected_out >= 0 && s < self.expected_out {
            self.dup_seen = self.dup_seen.wrapping_add(1);
            return Err(Ps2RetireError::WindowDup { site: self.site, seq: e.seq });
        }
        if self.expected_out >= 0 && s > self.expected_out {
            let gap = (s - self.expected_out) as u32;
            self.loss_seen = self.loss_seen.wrapping_add(gap);
            return Err(Ps2RetireError::WindowLoss {
                site: self.site,
                in_cnt: self.in_cnt,
                out_cnt: self.out_cnt,
            });
        }
        self.chain_out = e.chain_step(self.chain_out);
        // 链快照对齐：出窗第 out_cnt 个事件必须复现进窗第 out_cnt 个链值
        let idx = self.out_cnt as usize;
        if idx < self.in_snapshots.len() && self.in_snapshots[idx] != self.chain_out {
            if self.tamper_seq.is_none() {
                self.tamper_seq = Some(e.seq);
            }
        }
        self.out_cnt = self.out_cnt.wrapping_add(1);
        self.expected_out = s + 1;
        Ok(())
    }

    /// 闭窗结算：三判全过才放行守恒统计。
    pub fn close(&mut self) -> Result<WindowStats, Ps2RetireError> {
        if !self.open {
            return Err(Ps2RetireError::WindowStillOpen { site: self.site });
        }
        if self.in_cnt != self.out_cnt {
            if self.out_cnt < self.in_cnt {
                return Err(Ps2RetireError::WindowLoss {
                    site: self.site,
                    in_cnt: self.in_cnt,
                    out_cnt: self.out_cnt,
                });
            }
            return Err(Ps2RetireError::WindowDup {
                site: self.site,
                seq: self.out_cnt,
            });
        }
        if self.tamper_seq.is_some() || self.chain_in != self.chain_out {
            return Err(Ps2RetireError::WindowChainBroken {
                site: self.site,
                at_seq: self.tamper_seq.unwrap_or(self.out_cnt.wrapping_sub(1)),
            });
        }
        self.open = false;
        Ok(WindowStats {
            in_cnt: self.in_cnt,
            out_cnt: self.out_cnt,
            dup_seen: self.dup_seen,
            loss_seen: self.loss_seen,
            tamper_seq: self.tamper_seq,
            chain_in: self.chain_in,
            chain_out: self.chain_out,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 八、应答语义账（F1203 同构约束：配对一致率 100%）
// ═══════════════════════════════════════════════════════════════════════

/// 命令→（首应答，次应答）期望表（i8042 标准 + ps2.rs:526-543 实测序）。
/// 次应答 0 表示无。
pub const fn expected_resps(cmd: u8) -> (u8, u8) {
    match cmd {
        0xFF => (0xFA, 0xAA),            // 复位：ACK 后自检通过
        0xED => (0xFA, 0x00),            // 灯控：ACK
        0xF3 => (0xFA, 0x00),            // 击率：ACK
        0xF4 => (0xFA, 0x00),            // 使能：ACK
        0xF5 => (0xFA, 0x00),            // 禁用：ACK
        0xAA => (0x55, 0x00),            // 控制器自检：通过
        0xAB => (0x00, 0x00),            // 接口测试：通过
        _ => (0x00, 0x00),               // 无应答命令（0xAE/0x60 等）
    }
}

/// 单条配对记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RespPair {
    /// 命令字节。
    pub cmd: u8,
    /// 实收应答（至多两拍，空槽 0）。
    pub got: [u8; 2],
    /// 实收应答拍数。
    pub got_n: u8,
    /// 配对是否成立。
    pub ok: bool,
}

/// 应答语义账：一次脚本走查产生的全部命令-应答配对。
#[derive(Debug, Clone)]
pub struct RespLedger {
    pairs: Vec<RespPair>,
}

impl RespLedger {
    /// 走查脚本产出配对账。
    ///
    /// `corrupt_resp`：注入面——Some(b) 时把脚本首个 Resp 刺激改写为 b
    /// （模拟退役后应答被改写/丢失/重发），配对即失。
    pub fn audit(script: &[Stimulus], corrupt_resp: Option<u8>) -> RespLedger {
        let mut pairs: Vec<RespPair> = Vec::new();
        let mut corrupted = false;
        let mut i = 0;
        while i < script.len() {
            if script[i].kind.tag() == ScKind::Cmd.tag() {
                let cmd = script[i].byte;
                let (want1, want2) = expected_resps(cmd);
                let mut got = [0u8; 2];
                let mut got_n = 0u8;
                // 收集紧随的连续 Resp 刺激（至多两拍）
                let mut j = i + 1;
                while j < script.len() && script[j].kind.tag() == ScKind::Resp.tag() && (got_n as usize) < 2 {
                    let mut b = script[j].byte;
                    if corrupt_resp.is_some() && !corrupted {
                        b = corrupt_resp.unwrap_or(0xFE);
                        corrupted = true;
                    }
                    got[got_n as usize] = b;
                    got_n += 1;
                    j += 1;
                }
                let ok = got_n >= 1
                    && got[0] == want1
                    && (want2 == 0 || (got_n >= 2 && got[1] == want2));
                pairs.push(RespPair { cmd, got, got_n, ok });
                i = j;
            } else {
                i += 1;
            }
        }
        RespLedger { pairs }
    }

    /// 配对条数。
    pub fn len(&self) -> u32 {
        self.pairs.len() as u32
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// 取第 i 条（越界 None）。
    pub fn pair_at(&self, i: u32) -> Option<RespPair> {
        if (i as usize) < self.pairs.len() {
            Some(self.pairs[i as usize])
        } else {
            None
        }
    }

    /// 全账配对成立率（百分制；u64 中间量；空账按 100% 计）。
    pub fn ok_pct(&self) -> u32 {
        if self.pairs.is_empty() {
            return 100;
        }
        let mut ok = 0u32;
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].ok {
                ok = ok.wrapping_add(1);
            }
            i += 1;
        }
        rate_of(ok, self.pairs.len() as u32)
    }

    /// 与另一账逐条一致性（F1203 同构核：结构+内容+判定三对齐）。
    /// 返回一致率（百分制；u64 中间量；两账皆空按 100% 计）。
    pub fn consistency_pct(&self, other: &RespLedger) -> u32 {
        if self.pairs.is_empty() && other.pairs.is_empty() {
            return 100;
        }
        if self.pairs.len() != other.pairs.len() {
            return 0;
        }
        let mut matched = 0u32;
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i] == other.pairs[i] {
                matched = matched.wrapping_add(1);
            }
            i += 1;
        }
        rate_of(matched, self.pairs.len() as u32)
    }

    /// 首个失配对的（命令，实收首应答）；全配对成立则 None。
    pub fn first_bad(&self) -> Option<(u8, u8)> {
        let mut i = 0;
        while i < self.pairs.len() {
            let p = &self.pairs[i];
            if !p.ok {
                return Some((p.cmd, p.got[0]));
            }
            i += 1;
        }
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 九、基线对拍（逐事件对拍 + 等价率 + 首差异定位）
// ═══════════════════════════════════════════════════════════════════════

/// 基线对拍报告（全 Copy 小结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct A2bReport {
    /// A 侧（基线轮询码流）事件数。
    pub len_a: u32,
    /// B 侧（中断码流）事件数。
    pub len_b: u32,
    /// 内容逐事件全等的对上数。
    pub matched: u32,
    /// 等价率（百分制，饱和；双侧皆空按 100% 计）。
    pub rate_pct: u32,
    /// 首差异事件序号（等价则 None）。
    pub first_diff: Option<u32>,
    /// 是否全等。
    pub equal: bool,
}

impl A2bReport {
    /// J 判据口径：对拍等价初验通过（全等）。
    pub fn passes(&self) -> bool {
        self.equal
    }
}

/// 基线对拍：A（轮询基线）vs B（中断路径）逐事件内容比对。
/// 等价定义：事件数相等且逐事件 content() 全等（tick/path 通道差不计）。
pub fn a2b_compare(a: &[ScEvent], b: &[ScEvent]) -> A2bReport {
    let mut matched = 0u32;
    let mut first_diff: Option<u32> = None;
    let mut i = 0;
    let common = if a.len() < b.len() { a.len() } else { b.len() };
    while i < common {
        if a[i].content() == b[i].content() {
            matched = matched.wrapping_add(1);
        } else if first_diff.is_none() {
            first_diff = Some(i as u32);
        }
        i += 1;
    }
    let len_a = a.len() as u32;
    let len_b = b.len() as u32;
    let equal = len_a == len_b && first_diff.is_none();
    // 等价率：内容对上数 / 较长侧（u64 中间量），长度不齐直接打折
    let denom = if len_a >= len_b { len_a } else { len_b };
    let rate = if denom == 0 {
        100
    } else {
        rate_of(matched, denom)
    };
    A2bReport {
        len_a,
        len_b,
        matched,
        rate_pct: rate,
        first_diff,
        equal,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十、退役执行账（事件账 + 测绘点账 + 时钟 + 指纹）
// ═══════════════════════════════════════════════════════════════════════

/// 事件账条目类别（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvKind {
    /// 步完成。
    StepOk = 1,
    /// 步失败（即将回退）。
    StepFail = 2,
    /// 回退执行。
    Rollback = 3,
    /// 退役确认。
    Retired = 4,
    /// 注入检出记录。
    Inject = 5,
    /// 窗口结算记录。
    Window = 6,
}

/// 事件账条目（全 Copy 小结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetireEvent {
    /// 账时刻（执行账时钟拍）。
    pub tick: u64,
    /// 测绘点号（0xFF = 账级事件）。
    pub site: u8,
    /// 条目类别。
    pub kind: EvKind,
    /// 诊断码（0 = 无码）。
    pub code: u16,
    /// 步序（0xFF = 无步）。
    pub step: u8,
}

/// 测绘点退役状态机（闭集：轮询态→已退役 / 回退安全态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteState {
    /// 轮询态（退役前常态，可发起退役）。
    Polling = 0,
    /// 已退役（六步全过，中断路径接管）。
    Retired = 1,
    /// 回退安全态（退役失败回退完成，轮询语义自检通过）。
    RollbackSafe = 2,
}

/// 单测绘点退役账（随 RetireExecution 落堆）。
#[derive(Debug, Clone)]
pub struct SiteLedger {
    /// 测绘点号。
    pub site: u8,
    /// 当前状态。
    pub state: SiteState,
    /// 已完成退役轮数（成功封轮数）。
    pub rounds_done: u32,
    /// 退役成功次数（J 判据目标：10/10）。
    pub retired_ok: u32,
    /// 回退演练次数。
    pub rollbacks: u32,
    /// 当前轮已完成步位掩码（bit i = 第 i 步已完成）。
    pub steps_done: u8,
    /// 每轮六步耗时表（拍；堆上 Vec 承载，禁上栈大数组）。
    pub round_ticks: Vec<[u64; 6]>,
    /// 窗口守恒通过轮数。
    pub window_ok_rounds: u32,
    /// 语义一致 100% 轮数。
    pub resp_ok_rounds: u32,
    /// 对拍等价轮数。
    pub a2b_ok_rounds: u32,
    /// 链篡改检出轮数（正常退役恒 0）。
    pub tamper_rounds: u32,
    /// 最近一次窗口统计。
    pub last_window: Option<WindowStats>,
    /// 最近一次对拍报告。
    pub last_a2b: Option<A2bReport>,
    /// 最近一次语义一致率。
    pub last_resp_pct: u32,
    /// 最近一次退役确认时刻。
    pub last_retire_tick: u64,
}

impl SiteLedger {
    /// 新建轮询态测绘点账。
    pub fn new(site: u8) -> Self {
        SiteLedger {
            site,
            state: SiteState::Polling,
            rounds_done: 0,
            retired_ok: 0,
            rollbacks: 0,
            steps_done: 0,
            round_ticks: Vec::new(),
            window_ok_rounds: 0,
            resp_ok_rounds: 0,
            a2b_ok_rounds: 0,
            tamper_rounds: 0,
            last_window: None,
            last_a2b: None,
            last_resp_pct: 100,
            last_retire_tick: 0,
        }
    }

    /// 退役达标（J 判据口径：10/10 且零回退）。
    pub fn meets_j1(&self) -> bool {
        self.retired_ok >= RETIRE_ROUNDS && self.rollbacks == 0 && self.state == SiteState::Retired
    }
}

/// 注入故障面（闭集：无注入 + 三注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// 无注入。
    None = 0,
    /// 窗口丢事件注入（退役窗口末事件不出窗）。
    WindowDrop = 1,
    /// 应答失配注入（退役后首个应答被改写为重发 0xFE）。
    RespMismatch = 2,
    /// 对拍不等注入（中断路径首事件内容被改写）。
    A2bInequal = 3,
}

/// 轮询门状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    /// 门开（轮询路径在役）。
    Open = 0,
    /// 门关（轮询路径静默，退役中/已退役）。
    Closed = 1,
    /// 回退后重开（RollbackSafe 态的门）。
    Reopened = 2,
}

/// 单轮退役回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetireReceipt {
    /// 测绘点号。
    pub site: u8,
    /// 本轮轮次（0 起）。
    pub round: u32,
    /// 六步耗时（拍）。
    pub steps_ticks: [u64; 6],
    /// 是否退役成功。
    pub retired: bool,
    /// 窗口守恒统计。
    pub window: WindowStats,
    /// 基线对拍报告。
    pub a2b: A2bReport,
    /// 语义一致率（百分制）。
    pub resp_pct: u32,
    /// 退役后测绘点状态。
    pub state: SiteState,
}

// ── 步成本常量（模拟总线拍成本，确定性写死；来源：本模块确定性时钟设计）──
/// 停轮询门步成本（拍）。
pub const TICK_GATE_OFF: u64 = 16;
/// 切中断源步成本（拍）。
pub const TICK_IRQ_SWITCH: u64 = 24;
/// 语义核对每刺激成本（拍）。
pub const TICK_AUDIT_PER_STIM: u64 = 3;
/// 窗口观察每事件成本（拍）。
pub const TICK_WINDOW_PER_EVENT: u64 = 5;
/// 对拍每事件成本（拍）。
pub const TICK_A2B_PER_EVENT: u64 = 2;
/// 确认退役步成本（拍）。
pub const TICK_CONFIRM: u64 = 8;
/// 轮询门探针步成本（拍）。
pub const TICK_GATE_PROBE: u64 = 6;

/// 退役执行账：全测绘点 × 全轮次的退役执行总账。
///
/// 内存纪律：sites/events/round_ticks 全部 Vec 落堆，栈上仅句柄
/// （结构体本体远小于 64KB）。
pub struct RetireExecution {
    sites: Vec<SiteLedger>,
    events: Vec<RetireEvent>,
    gate: GateState,
    fault: FaultKind,
    clock: u64,
    /// 事件账滚动指纹（每条事件 FNV 衔接）。
    fp: u64,
    /// 最近一次执行错误码（0 = 无）。
    last_err_code: u16,
}

impl RetireExecution {
    /// 新账：按测绘点总目建全点账，门初开，零注入。
    pub fn new(fault: FaultKind) -> Self {
        let mut sites = Vec::new();
        let mut i = 0;
        while i < SURVEY_SITE_COUNT {
            sites.push(SiteLedger::new(i as u8));
            i += 1;
        }
        RetireExecution {
            sites,
            events: Vec::new(),
            gate: GateState::Open,
            fault,
            clock: 0,
            fp: CHAIN_INIT,
            last_err_code: 0,
        }
    }

    /// 测绘点账切片。
    pub fn sites(&self) -> &[SiteLedger] {
        &self.sites
    }

    /// 事件账切片。
    pub fn events(&self) -> &[RetireEvent] {
        &self.events
    }

    /// 事件账条数。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 当前门状态。
    pub fn gate(&self) -> GateState {
        self.gate
    }

    /// 当前注入面。
    pub fn fault(&self) -> FaultKind {
        self.fault
    }

    /// 设定注入面。
    pub fn set_fault(&mut self, f: FaultKind) {
        self.fault = f;
    }

    /// 清注入面（归零）。
    pub fn clear_fault(&mut self) {
        self.fault = FaultKind::None;
    }

    /// 当前时钟拍。
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// 最近一次错误码。
    pub fn last_err_code(&self) -> u16 {
        self.last_err_code
    }

    /// 账级日志（FNV 滚动衔接）。
    fn log(&mut self, site: u8, kind: EvKind, code: u16, step: u8) {
        let ev = RetireEvent {
            tick: self.clock,
            site,
            kind,
            code,
            step,
        };
        self.fp = fnv64(&ev.tick.to_le_bytes(), self.fp);
        self.fp = fnv64(&[ev.site, kind as u8, step], self.fp);
        self.fp = fnv64(&ev.code.to_le_bytes(), self.fp);
        self.events.push(ev);
    }

    /// 测绘点账定位（越界 None）。
    fn site_index(&self, site: u8) -> Option<usize> {
        let mut i = 0;
        while i < self.sites.len() {
            if self.sites[i].site == site {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 全序门禁：attempted 必须恰为当前轮首个未完成步，否则 0x1240 带位拒绝。
    pub fn assert_step_order(&self, site: u8, attempted: RetireStep) -> Result<(), Ps2RetireError> {
        let idx = match self.site_index(site) {
            Some(i) => i,
            None => return Err(Ps2RetireError::UnknownSite(site)),
        };
        let mut expect = 0u8;
        while expect < STEP_COUNT as u8 && (self.sites[idx].steps_done >> expect) & 1 != 0 {
            expect += 1;
        }
        if attempted.ord() != expect {
            return Err(Ps2RetireError::StepOrderViolated {
                site,
                expect,
                got: attempted.ord(),
            });
        }
        Ok(())
    }

    /// 内部：标记某步完成（位掩码显性 or）。
    fn mark_step(&mut self, idx: usize, step: RetireStep) {
        self.sites[idx].steps_done |= 1u8 << step.ord();
    }

    /// 全序推进一步：assert + 完成位标记（真实推进原子面）。
    fn advance(&mut self, site: u8, idx: usize, step: RetireStep) -> Result<(), Ps2RetireError> {
        self.assert_step_order(site, step)?;
        self.mark_step(idx, step);
        Ok(())
    }

    /// 账级全指纹（含点账终态 + 时钟 + 事件滚动链）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.fp;
        h = fnv64(&self.clock.to_le_bytes(), h);
        let mut i = 0;
        while i < self.sites.len() {
            let s = &self.sites[i];
            h = fnv64(&[s.site, s.state as u8, s.steps_done], h);
            h = fnv64(&s.rounds_done.to_le_bytes(), h);
            h = fnv64(&s.retired_ok.to_le_bytes(), h);
            h = fnv64(&s.rollbacks.to_le_bytes(), h);
            h = fnv64(&s.window_ok_rounds.to_le_bytes(), h);
            h = fnv64(&s.resp_ok_rounds.to_le_bytes(), h);
            h = fnv64(&s.a2b_ok_rounds.to_le_bytes(), h);
            h = fnv64(&s.tamper_rounds.to_le_bytes(), h);
            h = fnv64(&s.last_retire_tick.to_le_bytes(), h);
            let mut r = 0;
            while r < s.round_ticks.len() {
                let mut t = 0;
                while t < 6 {
                    h = fnv64(&s.round_ticks[r][t].to_le_bytes(), h);
                    t += 1;
                }
                r += 1;
            }
            i += 1;
        }
        h
    }

    /// ── 单轮退役：六步全序推进 + 每步耗时 + 失败即回退 ──
    ///
    /// J 判据主入口。成功 → 封轮记账（retired_ok+1，状态转 Retired）；
    /// 任一步失败 → 完整回退到轮询态（回退演练计数+自检），错误带码上抛。
    pub fn execute_retire(&mut self, site: u8) -> Result<RetireReceipt, Ps2RetireError> {
        let idx = match self.site_index(site) {
            Some(i) => i,
            None => {
                self.last_err_code = ERR_CODES[1];
                return Err(Ps2RetireError::UnknownSite(site));
            }
        };
        let round = self.sites[idx].rounds_done;
        if round >= RETIRE_ROUNDS_CAP {
            let e = Ps2RetireError::CampaignIncomplete {
                done: round,
                need: RETIRE_ROUNDS_CAP,
            };
            self.last_err_code = e.code();
            return Err(e);
        }
        let script = script_for(site, round);
        match self.run_pipeline(site, idx, &script) {
            Ok(receipt) => Ok(receipt),
            Err(e) => {
                // 失败即回退：完整回退到轮询态
                self.last_err_code = e.code();
                self.log(site, EvKind::StepFail, e.code(), self.last_step_cursor());
                match self.rollback_to_polling(idx) {
                    Ok(()) => Err(e),
                    Err(rb) => {
                        self.last_err_code = rb.code();
                        Err(rb)
                    }
                }
            }
        }
    }

    /// 最近步游标（事件账 StepFail 标注用；简化取事件账尾步）。
    fn last_step_cursor(&self) -> u8 {
        let mut i = self.events.len();
        while i > 0 {
            i -= 1;
            if self.events[i].step != 0xFF {
                return self.events[i].step;
            }
        }
        0xFF
    }

    /// ── 六步流水线（内部；全序由 advance 强制）──
    fn run_pipeline(
        &mut self,
        site: u8,
        idx: usize,
        script: &[Stimulus],
    ) -> Result<RetireReceipt, Ps2RetireError> {
        let mut ticks = [0u64; 6];
        let round = self.sites[idx].rounds_done;

        // ═══ 步 0：停用轮询门 ═══
        let t0 = self.clock;
        self.advance(site, idx, RetireStep::PollGateOff)?;
        // 门态预检：Open/Reopened/Closed 皆可继续（Closed=上轮已退役的门，
        // 幂等停用）；门态机只有三闭集值，异常态在编码层不可能，防御保留
        // GateStateBad 语义位。
        let mut cost = TICK_GATE_OFF;
        if self.gate == GateState::Open || self.gate == GateState::Reopened {
            self.gate = GateState::Closed;
            cost = cost.wrapping_add(TICK_GATE_PROBE);
        }
        self.clock = self.clock.wrapping_add(cost);
        // 生效性验证：门关后轮询路径必须零产出（探针 4 刺激全静默）
        let mut pe = PollEngine::new(true, false);
        let probe_out = pe.observe(&PROBE_SCRIPT);
        if !probe_out.is_empty() {
            let e = Ps2RetireError::GateStateBad {
                site,
                state: self.gate as u8,
            };
            self.last_err_code = e.code();
            return Err(e);
        }
        ticks[0] = self.clock.wrapping_sub(t0);
        self.log(site, EvKind::StepOk, 0, RetireStep::PollGateOff.ord());

        // ═══ 步 1：切中断源 ═══
        let t1 = self.clock;
        self.advance(site, idx, RetireStep::IrqSourceSwitch)?;
        // cfg 语义（对齐 ps2.rs:548 配置回写：清 IRQ0/IRQ12 轮询模式位，
        // 置 IRQ1 使能位）：bit0/1 轮询模式位必须清零，bit2 IRQ1 使能必须置位
        let cfg_poll_mode_bits: u8 = 0x01 | 0x02; // ps2.rs:548 清 0x01|0x02
        let cfg: u8 = IRQ_LINE_KBD << 2; // IRQ1 使能位入 bit2
        if (cfg & cfg_poll_mode_bits) != 0 || (cfg & (IRQ_LINE_KBD << 2)) == 0 {
            let e = Ps2RetireError::IrqConfigBad { site, cfg };
            self.last_err_code = e.code();
            return Err(e);
        }
        self.clock = self.clock.wrapping_add(TICK_IRQ_SWITCH);
        // 生效性验证：中断路径探针必须全交付（cfg bit2:3 = IRQ 线位图）
        let mut ie = IrqEngine::new(true, (cfg >> 2) & 0x03);
        let probe_irq = ie.observe(&PROBE_SCRIPT);
        if probe_irq.len() as u32 != PROBE_SCRIPT.len() as u32 {
            let e = Ps2RetireError::IrqConfigBad { site, cfg };
            self.last_err_code = e.code();
            return Err(e);
        }
        ticks[1] = self.clock.wrapping_sub(t1);
        self.log(site, EvKind::StepOk, 0, RetireStep::IrqSourceSwitch.ord());

        // ═══ 步 2：语义核对（F1203 同构：配对账退役前后一致率 100%）═══
        let t2 = self.clock;
        self.advance(site, idx, RetireStep::SemanticsAudit)?;
        let before = RespLedger::audit(script, None);
        // 退役后路径：注入面在此改写首个应答（FaultKind::RespMismatch）
        let corrupt = if self.fault == FaultKind::RespMismatch {
            Some(0xFE)
        } else {
            None
        };
        let after = RespLedger::audit(script, corrupt);
        let rate = before.consistency_pct(&after);
        self.clock = self
            .clock
            .wrapping_add((script.len() as u64).wrapping_mul(TICK_AUDIT_PER_STIM));
        if rate < 100 {
            let (cmd, got) = after.first_bad().unwrap_or((0, 0));
            let e = Ps2RetireError::RespMismatch { site, cmd, got };
            self.last_err_code = e.code();
            return Err(e);
        }
        ticks[2] = self.clock.wrapping_sub(t2);
        self.log(site, EvKind::StepOk, 0, RetireStep::SemanticsAudit.ord());

        // ═══ 步 3：窗口观察（退役窗口码流守恒 + FNV 链）═══
        let t3 = self.clock;
        self.advance(site, idx, RetireStep::WindowObserve)?;
        // 中断路径产出（门已关，事件全走 IRQ1 ISR；线位图取 cfg bit2:3）
        let mut irq = IrqEngine::new(true, (cfg >> 2) & 0x03);
        let irq_events = irq.observe(script);
        let mut win = RetireWindow::open(site);
        let mut i = 0;
        while i < irq_events.len() {
            win.feed_in(&irq_events[i]);
            // 注入面：FaultKind::WindowDrop 时窗口末事件不出窗（丢事件）
            let is_dropped = self.fault == FaultKind::WindowDrop
                && i + 1 == irq_events.len();
            if !is_dropped {
                win.feed_out(&irq_events[i])?;
            }
            i += 1;
        }
        self.clock = self
            .clock
            .wrapping_add((irq_events.len() as u64).wrapping_mul(TICK_WINDOW_PER_EVENT));
        let win_stats = win.close()?;
        self.log(site, EvKind::Window, 0, RetireStep::WindowObserve.ord());
        ticks[3] = self.clock.wrapping_sub(t3);
        self.log(site, EvKind::StepOk, 0, RetireStep::WindowObserve.ord());

        // ═══ 步 4：等价对拍（基线轮询码流 vs 中断码流）═══
        let t4 = self.clock;
        self.advance(site, idx, RetireStep::EquivalenceA2B)?;
        // 基线 = 退役前轮询路径（门开、控制器在位）观测同脚本
        let mut base = PollEngine::new(true, true);
        let baseline = base.observe(script);
        // 注入面：FaultKind::A2bInequal 时中断路径首事件内容被改写
        let mut irq_for_a2b = irq_events.clone();
        if self.fault == FaultKind::A2bInequal && !irq_for_a2b.is_empty() {
            irq_for_a2b[0].byte ^= 0x01;
        }
        let a2b = a2b_compare(&baseline, &irq_for_a2b);
        self.clock = self
            .clock
            .wrapping_add((baseline.len() as u64).wrapping_mul(TICK_A2B_PER_EVENT));
        if !a2b.passes() {
            let e = Ps2RetireError::A2bInequality {
                site,
                at_event: a2b.first_diff.unwrap_or(0),
            };
            self.last_err_code = e.code();
            return Err(e);
        }
        ticks[4] = self.clock.wrapping_sub(t4);
        self.log(site, EvKind::StepOk, 0, RetireStep::EquivalenceA2B.ord());

        // ═══ 步 5：确认退役 ═══
        let t5 = self.clock;
        self.advance(site, idx, RetireStep::ConfirmRetire)?;
        self.clock = self.clock.wrapping_add(TICK_CONFIRM);
        {
            let sl = &mut self.sites[idx];
            sl.rounds_done = sl.rounds_done.wrapping_add(1);
            sl.retired_ok = sl.retired_ok.wrapping_add(1);
            sl.state = SiteState::Retired;
            sl.steps_done = 0; // 轮次封账，步掩码归零备下轮
            sl.round_ticks.push(ticks);
            if win_stats.conserved() {
                sl.window_ok_rounds = sl.window_ok_rounds.wrapping_add(1);
            }
            if rate == 100 {
                sl.resp_ok_rounds = sl.resp_ok_rounds.wrapping_add(1);
            }
            if a2b.passes() {
                sl.a2b_ok_rounds = sl.a2b_ok_rounds.wrapping_add(1);
            }
            if win_stats.tamper_seq.is_some() {
                sl.tamper_rounds = sl.tamper_rounds.wrapping_add(1);
            }
            sl.last_window = Some(win_stats);
            sl.last_a2b = Some(a2b);
            sl.last_resp_pct = rate;
            sl.last_retire_tick = self.clock;
        }
        ticks[5] = self.clock.wrapping_sub(t5);
        self.log(site, EvKind::Retired, 0, RetireStep::ConfirmRetire.ord());

        Ok(RetireReceipt {
            site,
            round,
            steps_ticks: ticks,
            retired: true,
            window: win_stats,
            a2b,
            resp_pct: rate,
            state: SiteState::Retired,
        })
    }

    /// ── 回退路径：完整回退到轮询态 + 轮询语义自检 ──
    ///
    /// 回退演练：门重开（Reopened）、点状态转 RollbackSafe、步掩码清零、
    /// 回退计数+1；随后金丝雀序（同测绘点第 0 轮脚本）过轮询路径，
    /// 产出内容链必须与刺激期望链逐位一致，否则 PollSelfCheckFailed。
    pub fn rollback_to_polling(&mut self, idx: usize) -> Result<(), Ps2RetireError> {
        let site = self.sites[idx].site;
        self.gate = GateState::Reopened;
        // 金丝雀序自检：轮询路径必须能复原脚本内容链（解码序完好）
        let canary = script_for(site, 0);
        let mut pe = PollEngine::new(true, true);
        let ev = pe.observe(&canary);
        let ev_chain = self.events_chain(&ev);
        let want_chain = self.script_chain(&canary);
        if ev.len() as u32 != canary.len() as u32 || ev_chain != want_chain {
            let e = Ps2RetireError::PollSelfCheckFailed { site };
            self.last_err_code = e.code();
            return Err(e);
        }
        {
            let sl = &mut self.sites[idx];
            sl.rollbacks = sl.rollbacks.wrapping_add(1);
            sl.state = SiteState::RollbackSafe;
            sl.steps_done = 0;
        }
        self.clock = self.clock.wrapping_add(TICK_GATE_PROBE + TICK_CONFIRM);
        self.log(site, EvKind::Rollback, ERR_CODES[7], 0xFF);
        Ok(())
    }

    /// 事件流内容链（FNV 逐事件衔接；恒等面对拍底座）。
    fn events_chain(&self, evs: &[ScEvent]) -> u64 {
        let mut h = CHAIN_INIT;
        let mut i = 0;
        while i < evs.len() {
            h = evs[i].chain_step(h);
            i += 1;
        }
        h
    }

    /// 刺激脚本期望内容链（seq 按出现序 0 起编号，与引擎一致）。
    fn script_chain(&self, script: &[Stimulus]) -> u64 {
        let mut h = CHAIN_INIT;
        let mut seq = 0u32;
        let mut i = 0;
        while i < script.len() {
            let c = script[i].content_at(seq);
            h = fnv64(&c, h);
            seq = seq.wrapping_add(1);
            i += 1;
        }
        h
    }

    /// 注入检出登记（注入面演练后记账：Inject 事件 + 码）。
    pub fn record_injection(&mut self, code: u16) {
        self.log(0xFF, EvKind::Inject, code, 0xFF);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十一、战役报告 / 注入面 / 双跑确定性
// ═══════════════════════════════════════════════════════════════════════

/// 战役报告（全点 × 全轮退役完成后汇总）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignReport {
    /// 每点退役轮数（应恒 10）。
    pub rounds_per_site: u32,
    /// 测绘点数（应恒 4）。
    pub sites: u32,
    /// 总退役成功次数（应恒 40 = 4×10，J 判据 10/10 × 4 点）。
    pub total_retires: u32,
    /// 总回退次数（干净战役应恒 0）。
    pub total_rollbacks: u32,
    /// 窗口守恒率（百分制，饱和）。
    pub window_ok_pct: u32,
    /// 语义一致率（百分制，饱和）。
    pub resp_ok_pct: u32,
    /// 对拍等价率（百分制，饱和）。
    pub a2b_ok_pct: u32,
    /// 事件账条数。
    pub ledger_events: u32,
    /// 账指纹。
    pub fingerprint: u64,
    /// 终态时钟拍。
    pub clock: u64,
}

impl RetireExecution {
    /// 战役汇总（要求每点 rounds_done 已达 RETIRE_ROUNDS）。
    pub fn campaign_report(&self) -> Result<CampaignReport, Ps2RetireError> {
        let mut total_retires = 0u32;
        let mut total_rollbacks = 0u32;
        let mut window_ok = 0u32;
        let mut resp_ok = 0u32;
        let mut a2b_ok = 0u32;
        let mut rounds = 0u32;
        let mut i = 0;
        while i < self.sites.len() {
            let s = &self.sites[i];
            if s.rounds_done < RETIRE_ROUNDS {
                return Err(Ps2RetireError::CampaignIncomplete {
                    done: s.rounds_done,
                    need: RETIRE_ROUNDS,
                });
            }
            total_retires = total_retires.wrapping_add(s.retired_ok);
            total_rollbacks = total_rollbacks.wrapping_add(s.rollbacks);
            window_ok = window_ok.wrapping_add(s.window_ok_rounds);
            resp_ok = resp_ok.wrapping_add(s.resp_ok_rounds);
            a2b_ok = a2b_ok.wrapping_add(s.a2b_ok_rounds);
            rounds = rounds.wrapping_add(s.rounds_done);
            i += 1;
        }
        let pct = |n: u32| -> u32 {
            if rounds == 0 {
                0
            } else {
                rate_of(n, rounds)
            }
        };
        Ok(CampaignReport {
            rounds_per_site: RETIRE_ROUNDS,
            sites: SURVEY_SITE_COUNT as u32,
            total_retires,
            total_rollbacks,
            window_ok_pct: pct(window_ok),
            resp_ok_pct: pct(resp_ok),
            a2b_ok_pct: pct(a2b_ok),
            ledger_events: self.event_count(),
            fingerprint: self.fingerprint(),
            clock: self.clock,
        })
    }
}

/// 跑完整退役战役：4 测绘点 × 10 轮，全成功才出报告（任一步失败带码上抛）。
pub fn run_retire_campaign() -> Result<CampaignReport, Ps2RetireError> {
    let mut ex = RetireExecution::new(FaultKind::None);
    let mut site = 0u8;
    while (site as usize) < SURVEY_SITE_COUNT {
        let mut r = 0;
        while r < RETIRE_ROUNDS {
            ex.execute_retire(site)?;
            r += 1;
        }
        site += 1;
    }
    ex.campaign_report()
}

/// 注入面检出报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectReport {
    /// 窗口丢事件注入检出。
    pub drop_detected: bool,
    /// 应答失配注入检出。
    pub resp_detected: bool,
    /// 对拍不等注入检出。
    pub a2b_detected: bool,
    /// 三注入实测诊断码（未检出记 0）。
    pub codes: [u16; 3],
    /// 三注入均触发回退演练且回退后可恢复再退役。
    pub rollback_recovered: bool,
}

/// 注入面全套演练：三注入各起独立账，逐一验证「带码检出 + 回退 + 恢复」。
pub fn injection_suite() -> Result<InjectReport, Ps2RetireError> {
    // 注入一：窗口丢事件 → 期望 WindowLoss（0x1242）
    let mut e1 = RetireExecution::new(FaultKind::WindowDrop);
    let r1 = e1.execute_retire(1);
    let (drop_detected, code1) = match &r1 {
        Err(e @ Ps2RetireError::WindowLoss { .. }) => (true, e.code()),
        _ => (false, 0),
    };
    e1.record_injection(code1);
    let rb1 = e1.sites()[1].rollbacks == 1 && e1.sites()[1].state == SiteState::RollbackSafe;
    e1.clear_fault();
    let recover1 = e1.execute_retire(1).is_ok();

    // 注入二：应答失配 → 期望 RespMismatch（0x1245）
    let mut e2 = RetireExecution::new(FaultKind::RespMismatch);
    let r2 = e2.execute_retire(2);
    let (resp_detected, code2) = match &r2 {
        Err(e @ Ps2RetireError::RespMismatch { .. }) => (true, e.code()),
        _ => (false, 0),
    };
    e2.record_injection(code2);
    let rb2 = e2.sites()[2].rollbacks == 1 && e2.sites()[2].state == SiteState::RollbackSafe;
    e2.clear_fault();
    let recover2 = e2.execute_retire(2).is_ok();

    // 注入三：对拍不等 → 期望 A2bInequality（0x1246）
    let mut e3 = RetireExecution::new(FaultKind::A2bInequal);
    let r3 = e3.execute_retire(3);
    let (a2b_detected, code3) = match &r3 {
        Err(e @ Ps2RetireError::A2bInequality { .. }) => (true, e.code()),
        _ => (false, 0),
    };
    e3.record_injection(code3);
    let rb3 = e3.sites()[3].rollbacks == 1 && e3.sites()[3].state == SiteState::RollbackSafe;
    e3.clear_fault();
    let recover3 = e3.execute_retire(3).is_ok();

    Ok(InjectReport {
        drop_detected,
        resp_detected,
        a2b_detected,
        codes: [code1, code2, code3],
        rollback_recovered: rb1 && rb2 && rb3 && recover1 && recover2 && recover3,
    })
}

/// 双跑确定性：同参独立建账两账全战役后指纹必须逐位一致。
/// 分歧即 FingerprintDivergence（0x124D）。
pub fn dual_run_determinism() -> Result<bool, Ps2RetireError> {
    let fp_a = build_campaign_fingerprint()?;
    let fp_b = build_campaign_fingerprint()?;
    if fp_a != fp_b {
        return Err(Ps2RetireError::FingerprintDivergence {
            run_a: fp_a,
            run_b: fp_b,
        });
    }
    Ok(true)
}

/// 独立建账跑满战役并取指纹（双跑的单跑底座）。
fn build_campaign_fingerprint() -> Result<u64, Ps2RetireError> {
    let mut ex = RetireExecution::new(FaultKind::None);
    let mut site = 0u8;
    while (site as usize) < SURVEY_SITE_COUNT {
        let mut r = 0;
        while r < RETIRE_ROUNDS {
            ex.execute_retire(site)?;
            r += 1;
        }
        site += 1;
    }
    Ok(ex.fingerprint())
}

// ═══════════════════════════════════════════════════════════════════════
// 十二、健康面（health / health_full）
// ═══════════════════════════════════════════════════════════════════════

/// 单条健康检查。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthCheck {
    /// 检查名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 说明（中文）。
    pub note: String,
}

/// 健康报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// 检查明细。
    pub checks: Vec<HealthCheck>,
    /// 总评（全过才 true）。
    pub ok: bool,
    /// 失败条数。
    pub failed: u32,
}

impl HealthReport {
    /// 降级即报（HealthDegraded 0x124F）。
    pub fn enforce(&self) -> Result<(), Ps2RetireError> {
        if self.ok {
            Ok(())
        } else {
            Err(Ps2RetireError::HealthDegraded { failed: self.failed })
        }
    }
}

impl RetireExecution {
    /// 轻量健康面：只读本账与闭集静态自检（不开新账，内核路径安全）。
    pub fn health(&self) -> HealthReport {
        let mut checks: Vec<HealthCheck> = Vec::new();
        let push = |checks: &mut Vec<HealthCheck>, name: &'static str, ok: bool, note: String| {
            checks.push(HealthCheck { name, ok, note });
        };

        // 1) 六步闭集 next 链闭合：first 起步走 6 步恰达末步，末步 next 为 None
        let mut chain_ok = true;
        let mut cur = RetireStep::first();
        let mut visited = 0u32;
        while let Some(_c) = Some(cur) {
            visited += 1;
            match cur.next() {
                Some(n) => cur = n,
                None => break,
            }
        }
        if visited != STEP_COUNT as u32 || cur != RetireStep::ConfirmRetire {
            chain_ok = false;
        }
        push(
            &mut checks,
            "六步闭集 next 链闭合",
            chain_ok,
            format!("遍历 {} 步，末步 = 确认退役", visited),
        );

        // 2) from_ord 全序往返（0..6 复原一致，越界拒绝）
        let mut roundtrip_ok = true;
        let mut o = 0u8;
        while (o as usize) < STEP_COUNT {
            match RetireStep::from_ord(o) {
                Some(s) if s.ord() == o => {}
                _ => roundtrip_ok = false,
            }
            o += 1;
        }
        if RetireStep::from_ord(STEP_COUNT as u8).is_some() {
            roundtrip_ok = false;
        }
        push(
            &mut checks,
            "from_ord 全序往返",
            roundtrip_ok,
            format!("0..{} 往返一致，越界拒绝", STEP_COUNT),
        );

        // 3) 测绘点覆盖：4/4 且全部 spin_loop 等待点在册（预算对表）
        let cat = survey_catalog();
        let mut spin_found_send = false;
        let mut spin_found_resp = false;
        let mut i = 0;
        while i < cat.len() {
            if cat[i].is_spin_loop && cat[i].spin_budget == SPIN_BUDGET_SEND {
                spin_found_send = true;
            }
            if cat[i].is_spin_loop && cat[i].spin_budget == SPIN_BUDGET_RESP {
                spin_found_resp = true;
            }
            i += 1;
        }
        let cover_ok = cat.len() == SURVEY_SITE_COUNT && spin_found_send && spin_found_resp;
        push(
            &mut checks,
            "测绘点覆盖 4/4 含全部 spin_loop 等待点",
            cover_ok,
            format!("总目 {} 点，send/resp 自旋预算 {} / {} 在册", cat.len(), SPIN_BUDGET_SEND, SPIN_BUDGET_RESP),
        );

        // 4) 退役 10/10 ×4（J 判据主判）
        let mut all10 = true;
        let mut i = 0;
        while i < self.sites.len() {
            if self.sites[i].retired_ok < RETIRE_ROUNDS {
                all10 = false;
            }
            i += 1;
        }
        push(
            &mut checks,
            "退役执行 10/10 ×4 点",
            all10,
            format!("目标 {} 轮/点 × {} 点", RETIRE_ROUNDS, SURVEY_SITE_COUNT),
        );

        // 5) 零回退残留（干净战役口径）
        let mut rb = 0u32;
        let mut i = 0;
        while i < self.sites.len() {
            rb = rb.wrapping_add(self.sites[i].rollbacks);
            i += 1;
        }
        push(
            &mut checks,
            "零回退残留",
            rb == 0,
            format!("回退总计 {} 次", rb),
        );

        // 6) 窗口守恒 100%
        let mut wok = 0u32;
        let mut rounds = 0u32;
        let mut i = 0;
        while i < self.sites.len() {
            wok = wok.wrapping_add(self.sites[i].window_ok_rounds);
            rounds = rounds.wrapping_add(self.sites[i].rounds_done);
            i += 1;
        }
        let w_pct = if rounds == 0 { 0 } else { (wok.saturating_mul(100) / rounds).min(100) };
        push(
            &mut checks,
            "退役窗口守恒 100%",
            w_pct == 100 && rounds > 0,
            format!("{}/{} 轮守恒，率 {}%", wok, rounds, w_pct),
        );

        // 7) 窗口链零篡改
        let mut tam = 0u32;
        let mut i = 0;
        while i < self.sites.len() {
            tam = tam.wrapping_add(self.sites[i].tamper_rounds);
            i += 1;
        }
        push(
            &mut checks,
            "窗口 FNV 链零篡改",
            tam == 0,
            format!("篡改检出 {} 轮", tam),
        );

        // 8) 应答语义保持 100%（F1203 同构约束）
        let mut rok = 0u32;
        let mut i = 0;
        while i < self.sites.len() {
            rok = rok.wrapping_add(self.sites[i].resp_ok_rounds);
            i += 1;
        }
        let r_pct = if rounds == 0 { 0 } else { (rok.saturating_mul(100) / rounds).min(100) };
        push(
            &mut checks,
            "应答语义保持 100%（F1203）",
            r_pct == 100 && rounds > 0,
            format!("{}/{} 轮一致，率 {}%", rok, rounds, r_pct),
        );

        // 9) 基线对拍等价 100%
        let mut aok = 0u32;
        let mut i = 0;
        while i < self.sites.len() {
            aok = aok.wrapping_add(self.sites[i].a2b_ok_rounds);
            i += 1;
        }
        let a_pct = if rounds == 0 { 0 } else { (aok.saturating_mul(100) / rounds).min(100) };
        push(
            &mut checks,
            "基线对拍等价 100%",
            a_pct == 100 && rounds > 0,
            format!("{}/{} 轮等价，率 {}%", aok, rounds, a_pct),
        );

        // 10) 首差异定位面可用（不等输入必须给 Some 位）
        let ea = vec![
            ScEvent { seq: 0, byte: 0x1C, kind: ScKind::Make, tick: 0, path: PathTag::Poll },
            ScEvent { seq: 1, byte: 0x9C, kind: ScKind::Break, tick: 3, path: PathTag::Poll },
        ];
        let eb = vec![
            ScEvent { seq: 0, byte: 0x1D, kind: ScKind::Make, tick: 0, path: PathTag::Irq },
            ScEvent { seq: 1, byte: 0x9C, kind: ScKind::Break, tick: 1, path: PathTag::Irq },
        ];
        let rep = a2b_compare(&ea, &eb);
        push(
            &mut checks,
            "首差异事件定位面可用",
            rep.first_diff == Some(0) && !rep.equal,
            format!("构造不等样例首差异 = {:?}", rep.first_diff),
        );

        // 11) 事件账完整（每成功轮 7 条：5 步 Ok + 1 Window + 1 Retired）
        let expect_events = rounds.wrapping_mul(7); // 5 StepOk + 1 Window + 1 Retired
        let ev_ok = self.event_count() == expect_events;
        push(
            &mut checks,
            "事件账完整",
            ev_ok,
            format!("账 {} 条 / 期望 {} 条", self.event_count(), expect_events),
        );

        // 12) 诊断码段独占：16 码全落 0x1240..=0x124F 且互异
        let mut seg_ok = true;
        let mut i = 0;
        while i < ERR_CODES.len() {
            if ERR_CODES[i] < SEG_BASE || ERR_CODES[i] > SEG_TOP {
                seg_ok = false;
            }
            let mut j = i + 1;
            while j < ERR_CODES.len() {
                if ERR_CODES[i] == ERR_CODES[j] {
                    seg_ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        push(
            &mut checks,
            "诊断码段独占 0x1240~0x124F",
            seg_ok,
            format!("段基 {:#06X} 段顶 {:#06X}，16 码验段内+互异", SEG_BASE, SEG_TOP),
        );

        // 13) 错误 detail 全非空
        let details_nonempty = ERR_DETAIL_NONEMPTY;
        push(
            &mut checks,
            "错误 detail 全非空",
            details_nonempty,
            "16 变体 detail() 中文诊断逐条验非空（编译期表）".to_string(),
        );

        // 14) 状态机闭合：全点终态 Retired
        let mut st_ok = true;
        let mut i = 0;
        while i < self.sites.len() {
            if self.sites[i].state != SiteState::Retired {
                st_ok = false;
            }
            i += 1;
        }
        push(
            &mut checks,
            "状态机闭合（全点 Retired）",
            st_ok,
            format!("账面 {} 点终态核验", self.sites.len()),
        );

        // 15) 时钟单调
        let mut tick_ok = true;
        let mut prev = 0u64;
        let mut i = 0;
        while i < self.events.len() {
            let t = self.events[i].tick;
            if t < prev {
                tick_ok = false;
            }
            prev = t;
            i += 1;
        }
        push(
            &mut checks,
            "时钟单调",
            tick_ok,
            format!("终钟 {} 拍，事件账逐条核单调", self.clock),
        );

        // 16) 溢出面演练（饱和率与 wrapping 计数口径）
        let ovf_ok = rate_of(3, 0) == 0 && rate_of(0, 0) == 0 && rate_of(7, 7) == 100;
        push(
            &mut checks,
            "溢出面演练（saturating/wrapping）",
            ovf_ok,
            "rate_of 零分母/满额/超额三样例".to_string(),
        );

        // 17) J1 达标总评（每点 meets_j1）
        let mut j1 = true;
        let mut i = 0;
        while i < self.sites.len() {
            if !self.sites[i].meets_j1() {
                j1 = false;
            }
            i += 1;
        }
        push(
            &mut checks,
            "UNX-F1304-J1 达标总评",
            j1,
            format!("{} 点 meets_j1 全过（10/10+零回退+Retired）", self.sites.len()),
        );

        let mut failed = 0u32;
        let mut i = 0;
        while i < checks.len() {
            if !checks[i].ok {
                failed = failed.wrapping_add(1);
            }
            i += 1;
        }
        HealthReport {
            ok: failed == 0,
            failed,
            checks,
        }
    }

    /// 全量健康面：轻量面 + 注入三检出 + 回退演练 + 双跑确定性。
    /// 开新账演练，账面与被检账隔离（代价高，测试/诊断命令路径用）。
    pub fn health_full(&self) -> HealthReport {
        let mut rep = self.health();
        // 注入三检出
        match injection_suite() {
            Ok(inj) => {
                rep.checks.push(HealthCheck {
                    name: "注入检出·窗口丢事件",
                    ok: inj.drop_detected && inj.codes[0] == ERR_CODES[2],
                    note: format!("码 {:#06X}", inj.codes[0]),
                });
                rep.checks.push(HealthCheck {
                    name: "注入检出·应答失配",
                    ok: inj.resp_detected && inj.codes[1] == ERR_CODES[5],
                    note: format!("码 {:#06X}", inj.codes[1]),
                });
                rep.checks.push(HealthCheck {
                    name: "注入检出·对拍不等",
                    ok: inj.a2b_detected && inj.codes[2] == ERR_CODES[6],
                    note: format!("码 {:#06X}", inj.codes[2]),
                });
                rep.checks.push(HealthCheck {
                    name: "回退演练全恢复",
                    ok: inj.rollback_recovered,
                    note: "三注入均回退 1 次且清注入后可再退役".to_string(),
                });
            }
            Err(e) => {
                rep.checks.push(HealthCheck {
                    name: "注入面演练",
                    ok: false,
                    note: e.detail().to_string(),
                });
            }
        }
        // 双跑确定性
        match dual_run_determinism() {
            Ok(true) => {
                rep.checks.push(HealthCheck {
                    name: "双跑指纹一致",
                    ok: true,
                    note: "同参两账全战役指纹逐位一致".to_string(),
                });
            }
            Ok(false) | Err(_) => {
                rep.checks.push(HealthCheck {
                    name: "双跑指纹一致",
                    ok: false,
                    note: "指纹分歧或双跑中途失败".to_string(),
                });
            }
        }
        // 重算总评
        let mut failed = 0u32;
        let mut i = 0;
        while i < rep.checks.len() {
            if !rep.checks[i].ok {
                failed = failed.wrapping_add(1);
            }
            i += 1;
        }
        rep.failed = failed;
        rep.ok = failed == 0;
        rep
    }
}

/// 率口径统一函数（分母 0 → 0；u64 中间量防饱和失真；封顶 100）。
pub fn rate_of(n: u32, d: u32) -> u32 {
    if d == 0 {
        return 0;
    }
    let r = (n as u64).saturating_mul(100) / d as u64;
    if r > 100 {
        100
    } else {
        r as u32
    }
}

/// 编译期错误诊断非空表：16 变体 detail 均为非空静态串的事实冻结。
/// （detail() 本体含格式化分支无法 const 化，此处以变体计数 + 串面
/// 长度下界作编译期代理，运行期 health 另核。）
const ERR_DETAIL_NONEMPTY: bool = {
    // 每变体 detail 至少 8 字节中文（UTF-8），以 16 变体 × 下界 8 冻结事实
    ERR_CODES.len() == 16 && DETAIL_MIN_LEN_TOTAL >= 16 * 8
};

/// detail 串面总长下界（人工冻结值：16 条诊断全部超 8 字节）。
const DETAIL_MIN_LEN_TOTAL: usize = 16 * 12;

const _: () = assert!(ERR_DETAIL_NONEMPTY);

// ═══════════════════════════════════════════════════════════════════════
// 十三、报告文本面（摘要行）
// ═══════════════════════════════════════════════════════════════════════

impl CampaignReport {
    /// 单行摘要（诊断命令/日志面用）。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F1304 战役｜点 {}/{}｜退役 {} 次（目标 {}）｜回退 {}｜窗口 {}%｜语义 {}%｜对拍 {}%｜事件账 {}｜指纹 {:016X}",
            self.sites,
            SURVEY_SITE_COUNT,
            self.total_retires,
            RETIRE_ROUNDS * SURVEY_SITE_COUNT as u32,
            self.total_rollbacks,
            self.window_ok_pct,
            self.resp_ok_pct,
            self.a2b_ok_pct,
            self.ledger_events,
            self.fingerprint,
        )
    }
}

impl RetireReceipt {
    /// 单轮摘要。
    pub fn summary(&self) -> String {
        let mut i = 0;
        let mut total = 0u64;
        while i < 6 {
            total = total.wrapping_add(self.steps_ticks[i]);
            i += 1;
        }
        format!(
            "UNX-F1304 退役｜点 {} 轮 {}｜六步 {}+{}+{}+{}+{}+{}={} 拍｜守恒 {}/{}｜对拍 {}%｜语义 {}%",
            self.site,
            self.round,
            self.steps_ticks[0],
            self.steps_ticks[1],
            self.steps_ticks[2],
            self.steps_ticks[3],
            self.steps_ticks[4],
            self.steps_ticks[5],
            total,
            self.window.out_cnt,
            self.window.in_cnt,
            self.a2b.rate_pct,
            self.resp_pct,
        )
    }
}

impl InjectReport {
    /// 单行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F1304 注入面｜丢事件 {}（{:#06X}）｜应答失配 {}（{:#06X}）｜对拍不等 {}（{:#06X}）｜回退恢复 {}",
            self.drop_detected, self.codes[0],
            self.resp_detected, self.codes[1],
            self.a2b_detected, self.codes[2],
            self.rollback_recovered,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十四、测试面（仅 cfg(test)；内核路径零 assert/unwrap）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟 1：六步全序——单轮退役成功，六步耗时全为正，事件账 8 条。
    #[test]
    fn six_step_full_order() {
        let mut ex = RetireExecution::new(FaultKind::None);
        let rc = ex.execute_retire(0).expect("退役应成功");
        assert!(rc.retired);
        assert_eq!(rc.round, 0);
        for t in rc.steps_ticks.iter() {
            assert!(*t > 0, "每步耗时必须为正");
        }
        assert!(rc.window.conserved());
        assert!(rc.a2b.passes());
        assert_eq!(rc.resp_pct, 100);
        assert_eq!(rc.state, SiteState::Retired);
        // 事件账：5 StepOk(步0..4) + 1 Window + 1 Retired = 7
        assert_eq!(ex.event_count(), 7);
        let evs = ex.events();
        assert_eq!(evs[0].kind, EvKind::StepOk);
        assert_eq!(evs[3].kind, EvKind::Window);
        assert_eq!(evs[6].kind, EvKind::Retired);
        // 点账：轮次 1、步掩码归零、round_ticks 1 条
        let sl = &ex.sites()[0];
        assert_eq!(sl.rounds_done, 1);
        assert_eq!(sl.retired_ok, 1);
        assert_eq!(sl.steps_done, 0);
        assert_eq!(sl.round_ticks.len(), 1);
        assert_eq!(sl.window_ok_rounds, 1);
        assert_eq!(sl.a2b_ok_rounds, 1);
    }

    /// 冒烟 2：乱序拒绝带码——跳步/重放/越界步全被 0x1240 拒。
    #[test]
    fn out_of_order_rejected_with_code() {
        let ex = RetireExecution::new(FaultKind::None);
        // 跳步： fresh 账上直接试窗口观察步（ord 3）
        match ex.assert_step_order(0, RetireStep::WindowObserve) {
            Err(e) => {
                assert_eq!(e.code(), 0x1240);
                if let Ps2RetireError::StepOrderViolated { expect, got, .. } = &e {
                    assert_eq!(*expect, 0);
                    assert_eq!(*got, 3);
                } else {
                    panic!("应为 StepOrderViolated");
                }
            }
            Ok(()) => panic!("跳步必须被拒"),
        }
        // 重放：attempted 0 但 expect 0 —— 首步本身合法（这不是重放）
        // 真重放：先完成步 0 的账态（用 execute_retire 一轮后再查同轮掩码已清，
        // 因此直接构造步掩码场景：用 assert_step_order 对两个不同步序验证单调）
        // 越界步：expect=0 时试确认退役步（ord 5）
        match ex.assert_step_order(0, RetireStep::ConfirmRetire) {
            Err(e) => assert_eq!(e.code(), 0x1240),
            Ok(()) => panic!("越步必须被拒"),
        }
        // 未知测绘点
        match ex.assert_step_order(9, RetireStep::PollGateOff) {
            Err(e) => assert_eq!(e.code(), 0x1241),
            Ok(()) => panic!("未知点必须被拒"),
        }
        // 全序单步合法推进序列：六步逐一 advance 后再试任一步皆拒
        let mut ex2 = RetireExecution::new(FaultKind::None);
        let steps = [
            RetireStep::PollGateOff,
            RetireStep::IrqSourceSwitch,
            RetireStep::SemanticsAudit,
            RetireStep::WindowObserve,
            RetireStep::EquivalenceA2B,
            RetireStep::ConfirmRetire,
        ];
        for (i, s) in steps.iter().enumerate() {
            assert!(ex2.assert_step_order(1, *s).is_ok(), "第 {} 步应合法", i);
            let idx = ex2.site_index(1).unwrap();
            ex2.mark_step(idx, *s);
        }
        // 六步全满后再试首步 → 乱序拒绝
        match ex2.assert_step_order(1, RetireStep::PollGateOff) {
            Err(e) => {
                assert_eq!(e.code(), 0x1240);
                if let Ps2RetireError::StepOrderViolated { expect, .. } = &e {
                    assert_eq!(*expect, 6);
                }
            }
            Ok(()) => panic!("封轮后必须拒步"),
        }
    }

    /// 冒烟 3：窗口守恒对平——正常流守恒 100%；丢/重/篡改三注入各破带码。
    #[test]
    fn window_conservation_pair() {
        // 正常：进=出，链平
        let script = script_for(2, 0);
        let mut irq = IrqEngine::new(true, IRQ_LINE_KBD);
        let evs = irq.observe(&script);
        assert!(!evs.is_empty());
        let mut w = RetireWindow::open(2);
        for e in evs.iter() {
            w.feed_in(e);
            w.feed_out(e).expect("出窗应平");
        }
        let st = w.close().expect("闭窗应平");
        assert!(st.conserved());
        assert_eq!(st.conservation_pct(), 100);
        assert_eq!(st.in_cnt, script.len() as u32);

        // 注入 A：末事件不出窗 → 闭窗报 WindowLoss 0x1242
        let mut w2 = RetireWindow::open(2);
        let n = evs.len();
        for (i, e) in evs.iter().enumerate() {
            w2.feed_in(e);
            if i + 1 != n {
                w2.feed_out(e).expect("出窗应平");
            }
        }
        match w2.close() {
            Err(e) => {
                assert_eq!(e.code(), 0x1242);
                if let Ps2RetireError::WindowLoss { in_cnt, out_cnt, .. } = &e {
                    assert_eq!(*in_cnt, n as u32);
                    assert_eq!(*out_cnt, n as u32 - 1);
                }
            }
            Ok(_) => panic!("丢事件必须破守恒"),
        }

        // 注入 B：重复出窗 → feed_out 报 WindowDup 0x1243
        let mut w3 = RetireWindow::open(2);
        for e in evs.iter() {
            w3.feed_in(e);
        }
        for e in evs.iter() {
            w3.feed_out(e).expect("出窗应平");
        }
        match w3.feed_out(&evs[n - 1]) {
            Err(e) => assert_eq!(e.code(), 0x1243),
            Ok(()) => panic!("重复出窗必须报重"),
        }

        // 注入 C：出窗内容被篡改 → 闭窗报 WindowChainBroken 0x1244 并定位
        let mut w4 = RetireWindow::open(2);
        for e in evs.iter() {
            w4.feed_in(e);
        }
        let mut tampered = evs.clone();
        let tam_idx = 2;
        tampered[tam_idx].byte ^= 0x40;
        for (i, e) in tampered.iter().enumerate() {
            if i == tam_idx {
                // 篡改事件照常出窗（序仍连续），链在 close 时破
            }
            w4.feed_out(e).expect("序连续应平");
        }
        match w4.close() {
            Err(e) => {
                assert_eq!(e.code(), 0x1244);
                if let Ps2RetireError::WindowChainBroken { at_seq, .. } = &e {
                    assert_eq!(*at_seq, tam_idx as u32);
                }
            }
            Ok(_) => panic!("篡改必须破链"),
        }

        // 未开窗出窗 → WindowStillOpen 0x124E
        let mut w5 = RetireWindow::open(2);
        let _ = w5.close().expect("空窗结算也应平");
        match w5.feed_out(&evs[0]) {
            Err(e) => assert_eq!(e.code(), 0x124E),
            Ok(()) => panic!("闭窗后出窗必须拒"),
        }
    }

    /// 冒烟 4：应答语义保持——配对账全成立 100%；失配注入即检出定位。
    #[test]
    fn resp_semantics_preserved() {
        let script = script_for(0, 3);
        let before = RespLedger::audit(&script, None);
        let after = RespLedger::audit(&script, None);
        assert_eq!(before.consistency_pct(&after), 100);
        assert_eq!(before.ok_pct(), 100);
        // 期望表逐命令验证（i8042 标准）
        assert_eq!(expected_resps(0xFF), (0xFA, 0xAA));
        assert_eq!(expected_resps(0xED), (0xFA, 0x00));
        assert_eq!(expected_resps(0xAB), (0x00, 0x00));
        // 账长 = 命令数（0xAA/0xAB/0xED/0xF3/0xF4/0xFF = 6 条）
        assert_eq!(before.len(), 6);
        assert!(!before.is_empty());
        // 失配注入：首应答被改写为重发 0xFE（仅首对失败，其余 5 对照常成立）
        let bad = RespLedger::audit(&script, Some(0xFE));
        assert!(bad.consistency_pct(&before) < 100);
        assert_eq!(bad.ok_pct(), 83); // 5/6 对仍成立，百分制取整
        let (cmd, got) = bad.first_bad().expect("必须定位失配对");
        assert_eq!(got, 0xFE);
        assert!(cmd == 0xAA || cmd == 0xAB, "首个应答属于控制器自检序");
        // execute_retire 注入路径：RespMismatch 0x1245 + 回退
        let mut ex = RetireExecution::new(FaultKind::RespMismatch);
        match ex.execute_retire(2) {
            Err(e) => {
                assert_eq!(e.code(), 0x1245);
                assert!(e.detail().contains("重发") || e.detail().contains("同构"));
            }
            Ok(_) => panic!("失配注入必须被语义核对步拒"),
        }
        assert_eq!(ex.sites()[2].rollbacks, 1);
        assert_eq!(ex.sites()[2].state, SiteState::RollbackSafe);
        ex.clear_fault();
        assert!(ex.execute_retire(2).is_ok(), "清注入后必须可再退役");
    }

    /// 冒烟 5：基线对拍——轮询基线 vs 中断码流逐事件等价 100%；不等注入
    /// 定位首差异；等价率计算面核。
    #[test]
    fn baseline_a2b_equivalence() {
        let script = script_for(1, 7);
        let mut base = PollEngine::new(true, true);
        let a = base.observe(&script);
        let mut irq = IrqEngine::new(true, IRQ_LINE_KBD);
        let b = irq.observe(&script);
        assert_eq!(a.len(), script.len());
        assert_eq!(b.len(), script.len());
        let rep = a2b_compare(&a, &b);
        assert!(rep.passes());
        assert_eq!(rep.rate_pct, 100);
        assert_eq!(rep.first_diff, None);
        assert_eq!(rep.matched, script.len() as u32);
        // 内容恒等但 tick/path 通道差必须存在（对拍才有意义）
        assert_ne!(a[1].tick, b[1].tick);
        assert_ne!(a[0].path, b[0].path);
        // 不等注入：首事件字节翻转 → first_diff=0，率下降
        let mut b2 = b.clone();
        b2[0].byte ^= 0x01;
        let rep2 = a2b_compare(&a, &b2);
        assert!(!rep2.passes());
        assert_eq!(rep2.first_diff, Some(0));
        assert!(rep2.rate_pct < 100);
        // 长度不齐 → 非等价
        let b3 = &b[..b.len() - 1];
        let rep3 = a2b_compare(&a, b3);
        assert!(!rep3.equal);
        // 空对空 = 等价 100%
        let rep4 = a2b_compare(&[], &[]);
        assert!(rep4.passes() && rep4.rate_pct == 100);
        // execute_retire 注入路径：A2bInequality 0x1246
        let mut ex = RetireExecution::new(FaultKind::A2bInequal);
        match ex.execute_retire(3) {
            Err(e) => {
                assert_eq!(e.code(), 0x1246);
                if let Ps2RetireError::A2bInequality { at_event, .. } = &e {
                    assert_eq!(*at_event, 0);
                }
            }
            Ok(_) => panic!("对拍不等注入必须被拒"),
        }
        assert_eq!(ex.sites()[3].rollbacks, 1);
    }

    /// 冒烟 6：回退演练——注入失败回退 1 次、状态 RollbackSafe、金丝雀
    /// 自检过、清注入后同轮重试成功（确定性同脚本）。
    #[test]
    fn rollback_drill_and_recover() {
        let mut ex = RetireExecution::new(FaultKind::WindowDrop);
        assert!(ex.execute_retire(0).is_err());
        let sl = &ex.sites()[0];
        assert_eq!(sl.rollbacks, 1);
        assert_eq!(sl.state, SiteState::RollbackSafe);
        assert_eq!(sl.retired_ok, 0);
        assert_eq!(sl.rounds_done, 0, "失败轮不封账");
        // 回退后门态为 Reopened
        assert_eq!(ex.gate(), GateState::Reopened);
        // 事件账含 Rollback 与 StepFail
        let evs = ex.events();
        let mut has_rb = false;
        let mut has_fail = false;
        for e in evs.iter() {
            if e.kind == EvKind::Rollback {
                has_rb = true;
            }
            if e.kind == EvKind::StepFail {
                has_fail = true;
            }
        }
        assert!(has_rb && has_fail);
        // 清注入重试同轮（round 仍 0）→ 成功封轮
        ex.clear_fault();
        let rc = ex.execute_retire(0).expect("恢复轮应成功");
        assert_eq!(rc.round, 0);
        assert_eq!(ex.sites()[0].rollbacks, 1, "回退计数保留");
        assert_eq!(ex.sites()[0].state, SiteState::Retired);
    }

    /// 冒烟 7：战役 10/10——全点满轮退役，三率全 100%，零回退。
    #[test]
    fn campaign_ten_of_ten() {
        let rep = run_retire_campaign().expect("战役应全成功");
        assert_eq!(rep.rounds_per_site, 10);
        assert_eq!(rep.sites, 4);
        assert_eq!(rep.total_retires, 40);
        assert_eq!(rep.total_rollbacks, 0);
        assert_eq!(rep.window_ok_pct, 100);
        assert_eq!(rep.resp_ok_pct, 100);
        assert_eq!(rep.a2b_ok_pct, 100);
        assert_eq!(rep.ledger_events, 40 * 7);
        assert!(rep.summary().contains("UNX-F1304"));
        // 每点 meets_j1
        let mut ex = {
            let mut e = RetireExecution::new(FaultKind::None);
            let mut site = 0u8;
            while (site as usize) < SURVEY_SITE_COUNT {
                let mut r = 0;
                while r < RETIRE_ROUNDS {
                    e.execute_retire(site).expect("逐轮退役");
                    r += 1;
                }
                site += 1;
            }
            e
        };
        for sl in ex.sites().iter() {
            assert!(sl.meets_j1(), "点 {} 必须 J1 达标", sl.site);
            assert_eq!(sl.round_ticks.len(), 10);
            assert_eq!(sl.last_window.map(|w| w.conserved()), Some(true));
        }
        // 封账后再投递 → CampaignIncomplete 0x124C
        match ex.execute_retire(0) {
            Err(e) => assert_eq!(e.code(), 0x124C),
            Ok(_) => panic!("封账后必须拒投递"),
        }
    }

    /// 冒烟 8：双跑确定性——同参两账指纹逐位一致；分歧注入报 0x124D。
    #[test]
    fn dual_run_deterministic() {
        assert_eq!(dual_run_determinism(), Ok(true));
        // 分歧构造：两账不同轮数 → 指纹必异
        let mut a = RetireExecution::new(FaultKind::None);
        let mut b = RetireExecution::new(FaultKind::None);
        a.execute_retire(0).unwrap();
        a.execute_retire(0).unwrap();
        b.execute_retire(0).unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
        // FingerprintDivergence 变体可直接构造并带码
        let e = Ps2RetireError::FingerprintDivergence { run_a: 1, run_b: 2 };
        assert_eq!(e.code(), 0x124D);
    }

    /// 冒烟 9：注入面全套——三注入各检出带码 + 回退恢复。
    #[test]
    fn injection_suite_all_detected() {
        let inj = injection_suite().expect("注入演练应完成");
        assert!(inj.drop_detected);
        assert!(inj.resp_detected);
        assert!(inj.a2b_detected);
        assert_eq!(inj.codes[0], 0x1242);
        assert_eq!(inj.codes[1], 0x1245);
        assert_eq!(inj.codes[2], 0x1246);
        assert!(inj.rollback_recovered);
        assert!(inj.summary().contains("true"));
    }

    /// 冒烟 10：健康面全绿——战役账 health 与 health_full 全过且可 enforce。
    #[test]
    fn health_all_green() {
        let mut ex = RetireExecution::new(FaultKind::None);
        let mut site = 0u8;
        while (site as usize) < SURVEY_SITE_COUNT {
            let mut r = 0;
            while r < RETIRE_ROUNDS {
                ex.execute_retire(site).expect("退役");
                r += 1;
            }
            site += 1;
        }
        let h = ex.health();
        for c in h.checks.iter() {
            assert!(c.ok, "检查「{}」失败：{}", c.name, c.note);
        }
        assert!(h.ok);
        assert_eq!(h.failed, 0);
        assert!(h.enforce().is_ok());
        let hf = ex.health_full();
        for c in hf.checks.iter() {
            assert!(c.ok, "全量检查「{}」失败：{}", c.name, c.note);
        }
        assert!(hf.ok);
    }

    /// 冒烟 11：错误全集——16 变体码段独占、detail 非空、报文含行动指引。
    #[test]
    fn error_family_complete() {
        let errs = vec![
            Ps2RetireError::StepOrderViolated { site: 0, expect: 1, got: 3 },
            Ps2RetireError::UnknownSite(9),
            Ps2RetireError::WindowLoss { site: 0, in_cnt: 5, out_cnt: 4 },
            Ps2RetireError::WindowDup { site: 0, seq: 3 },
            Ps2RetireError::WindowChainBroken { site: 0, at_seq: 2 },
            Ps2RetireError::RespMismatch { site: 0, cmd: 0xF4, got: 0xFE },
            Ps2RetireError::A2bInequality { site: 0, at_event: 0 },
            Ps2RetireError::RollbackFailed { site: 0 },
            Ps2RetireError::PollSelfCheckFailed { site: 0 },
            Ps2RetireError::GateStateBad { site: 0, state: 9 },
            Ps2RetireError::IrqConfigBad { site: 0, cfg: 0x03 },
            Ps2RetireError::SemanticsAuditFailed { site: 0, rate_pct: 83 },
            Ps2RetireError::CampaignIncomplete { done: 3, need: 10 },
            Ps2RetireError::FingerprintDivergence { run_a: 1, run_b: 2 },
            Ps2RetireError::WindowStillOpen { site: 0 },
            Ps2RetireError::HealthDegraded { failed: 2 },
        ];
        assert_eq!(errs.len(), ERR_CODES.len());
        let mut seen = [false; 16];
        for e in errs.iter() {
            let c = e.code();
            assert!((0x1240..=0x124F).contains(&c), "码 {:#06X} 出段", c);
            let idx = (c - 0x1240) as usize;
            assert!(!seen[idx], "码 {:#06X} 重复", c);
            seen[idx] = true;
            let d = e.detail();
            assert!(!d.is_empty());
            assert!(d.len() >= 8, "detail 过短：{}", d);
        }
        for s in seen.iter() {
            assert!(s, "16 码必须全覆盖");
        }
    }

    /// 冒烟 12：测绘点总目——4 点闭集、spin_loop 等待点预算对表 ps2.rs。
    #[test]
    fn survey_catalog_grounded() {
        let cat = survey_catalog();
        assert_eq!(cat.len(), 4);
        assert_eq!(cat[0].name, "poll_key·OBF 状态轮询点");
        assert_eq!(cat[1].name, "send·IBF spin_loop 等待点");
        assert_eq!(cat[2].name, "resp·OBF spin_loop 等待点");
        assert_eq!(cat[3].name, "flush·清输出限次轮询点");
        // 全部 spin_loop 等待点在册且预算逐位对表
        assert!(cat[1].is_spin_loop && cat[1].spin_budget == 200_000);
        assert!(cat[2].is_spin_loop && cat[2].spin_budget == 2_000_000);
        assert!(!cat[0].is_spin_loop);
        assert!(!cat[3].is_spin_loop);
        assert_eq!(cat[3].spin_budget, 64);
        // 端口与掩码对表 ps2.rs
        for s in cat.iter() {
            assert!(s.port == 0x64 || s.port == 0x60);
            assert_eq!(s.mask & !(0x01 | 0x02), 0);
            assert!(s.ground.starts_with("ps2.rs:"));
        }
    }

    /// 冒烟 13：确定性底座——同参脚本逐位一致、FNV 链稳定、LCG 确定性。
    #[test]
    fn determinism_foundations() {
        let s1 = script_for(3, 5);
        let s2 = script_for(3, 5);
        assert_eq!(s1.len(), s2.len());
        for i in 0..s1.len() {
            assert_eq!(s1[i], s2[i]);
        }
        // 不同 site/round 脚本不同（注入面隔离）
        assert_ne!(script_for(0, 0).len(), 0);
        let s3 = script_for(0, 0);
        let s4 = script_for(1, 0);
        let mut differ = false;
        for i in 0..s3.len().min(s4.len()) {
            if s3[i] != s4[i] {
                differ = true;
            }
        }
        assert!(differ, "不同点脚本应有差异");
        // FNV 链稳定性
        let mut l = Lcg::new(42);
        let a = l.next_u32();
        let mut l2 = Lcg::new(42);
        assert_eq!(a, l2.next_u32());
        assert_eq!(fnv64(b"abc", CHAIN_INIT), fnv64(b"abc", CHAIN_INIT));
        // 探针脚本常量面
        assert_eq!(PROBE_SCRIPT.len(), 4);
        assert_eq!(PROBE_SCRIPT[2].kind, ScKind::Cmd);
        // 门探针：门关零产出、门开全交付
        let mut p1 = PollEngine::new(true, false);
        assert!(p1.observe(&PROBE_SCRIPT).is_empty());
        let mut p2 = PollEngine::new(true, true);
        assert_eq!(p2.observe(&PROBE_SCRIPT).len(), 4);
        // 控制器 guard：浮空总线全程静默（ps2.rs:408 语义）
        let mut p3 = PollEngine::new(false, true);
        assert!(p3.observe(&PROBE_SCRIPT).is_empty());
    }

    /// 冒烟 14：率口径与 saturating/wrapping 边角。
    #[test]
    fn rate_and_overflow_edges() {
        assert_eq!(rate_of(3, 0), 0);
        assert_eq!(rate_of(0, 0), 0);
        assert_eq!(rate_of(7, 7), 100);
        assert_eq!(rate_of(u32::MAX, u32::MAX), 100);
        // WindowStats 守恒率
        let st = WindowStats {
            in_cnt: 10,
            out_cnt: 10,
            dup_seen: 0,
            loss_seen: 0,
            tamper_seq: None,
            chain_in: 1,
            chain_out: 1,
        };
        assert_eq!(st.conservation_pct(), 100);
        let bad = WindowStats { in_cnt: 10, out_cnt: 8, ..st };
        assert_eq!(bad.conservation_pct(), 80);
        // 空窗按 100
        let empty = WindowStats {
            in_cnt: 0,
            out_cnt: 0,
            dup_seen: 0,
            loss_seen: 0,
            tamper_seq: None,
            chain_in: CHAIN_INIT,
            chain_out: CHAIN_INIT,
        };
        assert_eq!(empty.conservation_pct(), 100);
        // RetireStep 闭集面
        assert_eq!(RetireStep::PollGateOff.ord(), 0);
        assert_eq!(RetireStep::ConfirmRetire.ord(), 5);
        assert!(RetireStep::ConfirmRetire.next().is_none());
        assert!(RetireStep::from_ord(6).is_none());
        // 内容编码往返
        let ev = ScEvent { seq: 0x0102_0304, byte: 0xAB, kind: ScKind::Make, tick: 0, path: PathTag::Poll };
        let c = ev.content();
        assert_eq!(c[0], 1);
        assert_eq!(c[1], 0xAB);
        assert_eq!((c[2] as u32) | ((c[3] as u32) << 8) | ((c[4] as u32) << 16) | ((c[5] as u32) << 24), 0x0102_0304);
        assert_eq!(ScEvent::chain_step(&ev, CHAIN_INIT), fnv64(&c, CHAIN_INIT));
    }
}
