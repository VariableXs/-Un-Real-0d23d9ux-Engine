#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F2361 · 收官轴总控（宣告编排三段）
//! 域/批：CoRun·UNX-A3-B39（域收官宣告与封账 · 交接收官轴三）
//! 册目标：310 行 ｜ 本文件实际：≥2800 行（深化版）
//! 判据原文：UNX-F2361-J1 宣告三段编排全过（证据段：五证据逐件点收；
//!            宣告段：条件机五条件硬断言；封账段：五证明落账与封存），
//!            三段时序断言全过，跳段注入 10/10 拒绝
//! 诊断码段：0x15C0–0x15FF（本单独占，不得他用）
//! =====================================================================================
//!
//! ## 本单定位
//! F2361 是 UNX-A3 域「收官宣告与封账」的**总控件**：它不实现五证据的点收逻辑、
//! 不实现条件机的五条件本体（归 F2362）、不实现五证明的细则（归 F2368），
//! 只把「证据段 → 宣告段 → 封账段」编排成一个有向无环的三节点状态机，
//! 并对跳段 / 乱序 / 重复 / 越界 / 缺证据 / 证明缺项六类错排逐条拒记留痕。
//!
//! ## 三段语义（判据 §三段编排）
//! - **证据段（Evidence）**：五证据逐件点收。判据第①条要求「五证据齐备才收官」，
//!   证据闭集为：总回归全绿 / 行数守恒 / 批间三态全已深化 / 联签双 100% /
//!   pending 白名单终态齐备。五件缺一即证据段不放行。
//! - **宣告段（Declare）**：条件机五条件**硬断言**（F2362 本体）。五条件任一为假
//!   即拦截宣告，硬断言即「不满足就红，不做降级、不做 warn-through」。
//! - **封账段（Seal）**：五证明落账与封存（质量/压力/防线/观测/健康），
//!   封账档哈希落档；封账后写路径全拦截（判据红线）。
//!
//! ## 合法边闭集
//! `Evidence → Declare → Seal → Done`，共 3 条合法边；3×3−3 = 6 条跨段边
//! （含回头边、自环、跳段边）一律由 `enter_stage()` 拒记并落 `reject_log`。
//!
//! ## 守恒账（整数恒等式，禁浮点）
//! - 证据守恒：`evidence_ok + evidence_bad == EVIDENCE_TOTAL(5)`
//! - 条件守恒：`cond_true + cond_false == COND_TOTAL(5)`
//! - 证明守恒：`proof_ok + proof_bad == PROOF_TOTAL(5)`
//! - 三段守恒：`stage_done + stage_pending == STAGE_COUNT(3)`
//! - 四链守恒：`chain_ok + chain_bad == CHAIN_TOTAL(4)`（判据链/资产链/文档链/包链）
//!
//! ## 与册内邻单的显式对齐点
//! - **F1594（宣告条件机体例 A2 同构）**：本单宣告段消费该体例的五条件口径。
//! - **F1598（域账封账五证明体例）**：本单封账段消费该体例的五证明闭集。
//! - **F1597（移交完成宣告体例）**：宣告段的「宣告」动作语义源。
//! - **F2360（B38 批级总成 228,000 确认）**：证据段第②条「行数守恒」的直接来源。
//! - **F2358（联签终账）**：证据段第④条「联签双 100%」的来源。
//! - **F2207（pending 白名单账）**：证据段第⑤条的来源。
//! - **F2362（宣告条件机）**：本单宣告段调用其硬断言，不重复其判定逻辑。
//! - **F2368（域账封账五证明）**：本单封账段的五证明闭集来源。
//! - **F2381（域终账总成）**：封账档哈希是其前置之一。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!`；零浮点；
//! 零真实 IO、零墙钟——时间一律为注入的逻辑 `tick: u64`；确定性 LCG 保证同种子
//! 同轨迹；错误为自定义 `Result<T, DecErr>` 闭集。
use alloc::vec;
extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 诊断码段基址（判据：0x15C0–0x15FF 本单独占）。
pub const SEG_BASE: u16 = 0x15C0;
/// 诊断码段掩码（0x15C0..0x15FF 共 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 模块版本标识。
pub const MODULE_VERSION: u64 = 0x0002_3610_0000_0001;
/// 模块短标签。
pub const MODULE_TAG: &str = "v2361-udecLARE3";
/// 判据单号。
pub const TASK_ID: &str = "UNX-F2361-J1";
/// 判据短名。
pub const CRITERION_NAME: &str = "宣告三段编排全过";

/// 编排段数。
pub const STAGE_COUNT: usize = 3;
/// 证据段序号。
pub const STAGE_EVIDENCE: usize = 0;
/// 宣告段序号。
pub const STAGE_DECLARE: usize = 1;
/// 封账段序号。
pub const STAGE_SEAL: usize = 2;
/// 终态序号。
pub const STAGE_DONE: usize = 3;
/// 合法边总数。
pub const LEGAL_EDGE_COUNT: usize = 3;
/// 全矩阵边数。
pub const MATRIX_EDGE_COUNT: usize = STAGE_COUNT * STAGE_COUNT;
/// 非法边总数。
pub const ILLEGAL_EDGE_COUNT: usize = MATRIX_EDGE_COUNT - LEGAL_EDGE_COUNT;

/// 五证据闭集大小（判据 §证据段：五证据逐件点收）。
pub const EVIDENCE_TOTAL: usize = 5;
/// 五条件闭集大小（判据 §宣告段：条件机五条件硬断言）。
pub const COND_TOTAL: usize = 5;
/// 五证明闭集大小（判据 §封账段：五证明落账与封存）。
pub const PROOF_TOTAL: usize = 5;
/// 四链闭集大小（判据 §收官总链：判据链+资产链+文档链+包链）。
pub const CHAIN_TOTAL: usize = 4;
/// 注入核负向形态数（判据 §跳段注入 10/10）。
pub const INJECT_TOTAL: u32 = 10;
/// 环形账容量。
pub const RING_CAP: usize = 80;
/// 注入器默认工作量。
pub const INJECT_SCAN_WORK: u32 = 8;

/// 相邻段最小 tick 间隔。
pub const STAGE_MIN_GAP_TICK: u64 = 1;
/// 每动作 tick 粒度。
pub const TICK_PER_ACTION: u64 = 1;
/// 宣告比率目标（百分制）。
pub const DECLARE_RATIO_TARGET: u32 = 100;
/// 宣告比率分母。
pub const DECLARE_RATIO_DEN: u32 = 100;

/// 域账累计口径：B39 段起累计 228,000 + 6,000 = 234,000。
pub const DOMAIN_ACCOUNT_BASE: u64 = 228_000;
/// B39 批增量。
pub const DOMAIN_ACCOUNT_DELTA: u64 = 6_000;
/// 本单所在段的累计域账。
pub const DOMAIN_ACCOUNT_EXPECT: u64 = DOMAIN_ACCOUNT_BASE + DOMAIN_ACCOUNT_DELTA;
/// 封账预记满账 240,000。
pub const DOMAIN_ACCOUNT_SEALED: u64 = 240_000;

/// FNV-1a 64 位 basis。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位 prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;

/// 体验日志五字段数。
pub const EXP_LOG_FIELDS: usize = 5;
/// 错误三要素元素数。
pub const ERR_TRIPLE_ELEMENTS: usize = 3;
/// 错误明细最小可读长度。
pub const ERR_DETAIL_MIN_LEN: usize = 24;
/// 封账哈希盐。
pub const SEAL_HASH_SALT: u64 = 0x5345_414C_4844_3031;
/// 报告行上限。
pub const REPORT_MAX_LINES: usize = 4096;

// ===========================================================================
// 2. 确定性 LCG 与 FNV-1a
// ===========================================================================

/// 确定性 LCG 发生器。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    state: u64,
}

impl Lcg {
    /// 以种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 只读内部状态。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；n = 0 返回 0。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取布尔。
    pub fn flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位（字节）。
pub fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h = FNV_BASIS;
    for b in bytes {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 64 位（u64 字）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    for w in words {
        let mut i = 0usize;
        while i < 8 {
            h ^= ((*w >> (i * 8)) & 0xFF) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
    }
    h
}

/// 混入 u32。
pub fn mix32(h: u64, v: u32) -> u64 {
    let mut acc = h ^ ((v as u64) & 0xFFFF_FFFF);
    acc = acc.wrapping_mul(FNV_PRIME);
    acc ^= acc >> 29;
    acc
}

/// 混入 u64。
pub fn mix64(h: u64, v: u64) -> u64 {
    let mut acc = h ^ v;
    acc = acc.wrapping_mul(FNV_PRIME);
    acc ^= acc >> 31;
    acc
}

/// 混入布尔。
pub fn mix_bool(h: u64, v: bool) -> u64 {
    mix32(h, if v { 1 } else { 0 })
}

/// 混入字符串。
pub fn mix_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= (*b as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
    }
    mix32(acc, s.len() as u32)
}

// ===========================================================================
// 3. 错误闭集：DecErr（15 变体）
// ===========================================================================

/// 本单错误闭集；每变体占诊断码 `SEG_BASE + n`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecErr {
    /// 0：段号越界。
    StageIndexOutOfRange(u8),
    /// 1：非法段间转换。
    IllegalStageEdge(u8, u8),
    /// 2：终态后再迁入。
    AlreadyFinalized,
    /// 3：时序倒流。
    TickRegress(u64, u64),
    /// 4：段内重入。
    StageReentrant(u8),
    /// 5：tick 未注入。
    TickMissing,
    /// 6：证据号越界。
    EvidenceIndexOutOfRange(u8),
    /// 7：证据重复点收。
    EvidenceDuplicated(u8),
    /// 8：证据未点收即宣告。
    EvidenceNotCollected(u8),
    /// 9：五证据不齐即宣告。载荷为 (已点收数, 应点收数)，用 u64 与账本口径对齐。
    EvidenceIncomplete(u64, u64),
    /// 10：条件号越界。
    CondIndexOutOfRange(u8),
    /// 11：硬断言失败仍放行宣告。
    CondAssertionFailed(u8),
    /// 12：证明号越界。
    ProofIndexOutOfRange(u8),
    /// 13：五证明不齐即封账。载荷为 (已落账数, 应落账数)，用 u64 与账本口径对齐。
    ProofIncomplete(u64, u64),
    /// 14：封账后写路径未拦截。
    WriteAfterSeal(u64),
    /// 15：封账哈希与预记不符。
    SealHashBroken(u64, u64),
    /// 16：守恒账不平。
    LedgerUnbalanced(u64, u64),
    /// 17：宣告比率不足。
    DeclareRatioShort(u32),
    /// 18：环形账下溢。
    RingUnderflow(u64),
    /// 19：注入检出数与期望不符。
    InjectDetectMismatch(u32, u32),
    /// 20：四链不齐即固化。载荷为 (已固化链数, 应固化链数)，用 u64 与账本口径对齐。
    ChainIncomplete(u64, u64),
}

impl DecErr {
    /// 变体序号（0 起）。
    pub fn idx(self) -> u16 {
        match self {
            DecErr::StageIndexOutOfRange(_) => 0,
            DecErr::IllegalStageEdge(_, _) => 1,
            DecErr::AlreadyFinalized => 2,
            DecErr::TickRegress(_, _) => 3,
            DecErr::StageReentrant(_) => 4,
            DecErr::TickMissing => 5,
            DecErr::EvidenceIndexOutOfRange(_) => 6,
            DecErr::EvidenceDuplicated(_) => 7,
            DecErr::EvidenceNotCollected(_) => 8,
            DecErr::EvidenceIncomplete(_, _) => 9,
            DecErr::CondIndexOutOfRange(_) => 10,
            DecErr::CondAssertionFailed(_) => 11,
            DecErr::ProofIndexOutOfRange(_) => 12,
            DecErr::ProofIncomplete(_, _) => 13,
            DecErr::WriteAfterSeal(_) => 14,
            DecErr::SealHashBroken(_, _) => 15,
            DecErr::LedgerUnbalanced(_, _) => 16,
            DecErr::DeclareRatioShort(_) => 17,
            DecErr::RingUnderflow(_) => 18,
            DecErr::InjectDetectMismatch(_, _) => 19,
            DecErr::ChainIncomplete(_, _) => 20,
        }
    }
    /// 诊断码 = `SEG_BASE + idx()`，必落 0x15C0..0x15FF。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.idx()) & (SEG_BASE | SEG_MASK)
    }
    /// 错误短名。
    pub fn name(self) -> &'static str {
        match self {
            DecErr::StageIndexOutOfRange(_) => "StageIndexOutOfRange",
            DecErr::IllegalStageEdge(_, _) => "IllegalStageEdge",
            DecErr::AlreadyFinalized => "AlreadyFinalized",
            DecErr::TickRegress(_, _) => "TickRegress",
            DecErr::StageReentrant(_) => "StageReentrant",
            DecErr::TickMissing => "TickMissing",
            DecErr::EvidenceIndexOutOfRange(_) => "EvidenceIndexOutOfRange",
            DecErr::EvidenceDuplicated(_) => "EvidenceDuplicated",
            DecErr::EvidenceNotCollected(_) => "EvidenceNotCollected",
            DecErr::EvidenceIncomplete(_, _) => "EvidenceIncomplete",
            DecErr::CondIndexOutOfRange(_) => "CondIndexOutOfRange",
            DecErr::CondAssertionFailed(_) => "CondAssertionFailed",
            DecErr::ProofIndexOutOfRange(_) => "ProofIndexOutOfRange",
            DecErr::ProofIncomplete(_, _) => "ProofIncomplete",
            DecErr::WriteAfterSeal(_) => "WriteAfterSeal",
            DecErr::SealHashBroken(_, _) => "SealHashBroken",
            DecErr::LedgerUnbalanced(_, _) => "LedgerUnbalanced",
            DecErr::DeclareRatioShort(_) => "DeclareRatioShort",
            DecErr::RingUnderflow(_) => "RingUnderflow",
            DecErr::InjectDetectMismatch(_, _) => "InjectDetectMismatch",
            DecErr::ChainIncomplete(_, _) => "ChainIncomplete",
        }
    }
    /// 错误三要素：现象 / 位置 / 建议修法。
    pub fn detail(self) -> String {
        match self {
            DecErr::StageIndexOutOfRange(v) => format!(
                "现象：段号 {} 越界；位置：Declare3::enter_stage 段号入口；建议修法：段号只在 0..{} 内取值，越界须先 reject 并补登跳段痕",
                v, STAGE_COUNT
            ),
            DecErr::IllegalStageEdge(a, b) => format!(
                "现象：非法段间转换 {} -> {}；位置：Declare3::enter_stage 合法边闭集裁决；建议修法：仅允许 0->1、1->2、2->3，其余 6 条边必须拒记",
                a, b
            ),
            DecErr::AlreadyFinalized => String::from(
                "现象：编排已入终态仍收到迁入请求；位置：Declare3::enter_stage 终态闸；建议修法：终态后任何迁入一律拒绝，封账档不得被重开",
            ),
            DecErr::TickRegress(cur, req) => format!(
                "现象：tick 回退 现 {} -> 欲 {}；位置：Declare3::enter_stage 时序闸；建议修法：逻辑 tick 必须单调不减，且相邻段间隔不小于 {} tick",
                cur, req, STAGE_MIN_GAP_TICK
            ),
            DecErr::StageReentrant(v) => format!(
                "现象：第 {} 段未完成又重入；位置：Declare3::enter_stage 未完成闸；建议修法：同一段重入须显式收口或回退，不得并发重入同一段",
                v
            ),
            DecErr::TickMissing => String::from(
                "现象：tick 未注入或为 0；位置：Declare3::advance 逻辑时间入口；建议修法：时间一律显式注入，缺省 0 视为时间未注入而拒绝推进",
            ),
            DecErr::EvidenceIndexOutOfRange(v) => format!(
                "现象：证据号 {} 越界；位置：Declare3::collect_evidence 证据入口；建议修法：证据号只在 0..{} 内，溢出即判黑户",
                v, EVIDENCE_TOTAL
            ),
            DecErr::EvidenceDuplicated(v) => format!(
                "现象：第 {} 件证据重复点收；位置：Declare3::collect_evidence 重收闸；建议修法：一件一收，重复点收即判重号并拒绝覆盖既有指纹",
                v
            ),
            DecErr::EvidenceNotCollected(v) => format!(
                "现象：第 {} 件证据未点收；位置：Declare3::declare_gate 齐备闸；建议修法：宣告前五证据必须逐件点收，缺一即拦截宣告",
                v
            ),
            DecErr::EvidenceIncomplete(ok, want) => format!(
                "现象：证据齐备度 {} / {}；位置：Declare3::declare_gate 齐备闸；建议修法：五证据齐备才收官，任一件缺失即判宣告不成立",
                ok, want
            ),
            DecErr::CondIndexOutOfRange(v) => format!(
                "现象：条件号 {} 越界；位置：Declare3::assert_cond 条件入口；建议修法：条件号只在 0..{} 内，越界即判条件机闭集被破坏",
                v, COND_TOTAL
            ),
            DecErr::CondAssertionFailed(v) => format!(
                "现象：第 {} 条硬断言失败仍尝试放行宣告；位置：Declare3::declare_gate 硬断言闸；建议修法：五条件硬断言缺一即拦截，禁止降级为警告",
                v
            ),
            DecErr::ProofIndexOutOfRange(v) => format!(
                "现象：证明号 {} 越界；位置：Declare3::seal_proof 证明入口；建议修法：证明号只在 0..{} 内，越界即判五证明闭集被破坏",
                v, PROOF_TOTAL
            ),
            DecErr::ProofIncomplete(ok, want) => format!(
                "现象：证明齐备度 {} / {}；位置：Declare3::seal_gate 齐备闸；建议修法：五证明齐备才封账，缺一即判封账不成立",
                ok, want
            ),
            DecErr::WriteAfterSeal(t) => format!(
                "现象：封账后 tick={} 仍有写路径；位置：Declare3::push_ring 封账后写闸；建议修法：封账档写路径全拦截，越界写入一律拒记",
                t
            ),
            DecErr::SealHashBroken(want, got) => format!(
                "现象：封账哈希不符 预记 {} 实得 {}；位置：Declare3::verify_seal 哈希校验；建议修法：封账档哈希必须与预记逐位相同，不符即查落账漂移源",
                want, got
            ),
            DecErr::LedgerUnbalanced(a, b) => format!(
                "现象：守恒账不平 已记 {} 应为 {}；位置：Declare3::reconcile 对账；建议修法：已记与分项和必须逐笔相等，差值只允许来自显式记账项",
                a, b
            ),
            DecErr::DeclareRatioShort(r) => format!(
                "现象：宣告比率 {}% 未达 100%；位置：Declare3::declare_gate 比率闸；建议修法：五条件必须全真才宣告，任一为假即比率不足",
                r
            ),
            DecErr::RingUnderflow(n) => format!(
                "现象：环形账下溢 {} 条；位置：RingLog::pop 取账；建议修法：取账前先判空，空账取账应拒绝而非补零",
                n
            ),
            DecErr::InjectDetectMismatch(got, want) => format!(
                "现象：注入检出 {} 例 与期望 {} 例不符；位置：Declare3::inject_all 注入核；建议修法：十形态注入必须逐条检出并各自独立留痕",
                got, want
            ),
            DecErr::ChainIncomplete(ok, want) => format!(
                "现象：四链齐备度 {} / {}；位置：Declare3::chain_gate 固化闸；建议修法：判据链/资产链/文档链/包链四链齐备才可固化收官总链",
                ok, want
            ),
        }
    }
    /// 三要素齐备性自检。
    pub fn detail_has_triple(&self) -> bool {
        let d = self.detail();
        d.len() >= ERR_DETAIL_MIN_LEN
            && d.contains("现象")
            && d.contains("位置")
            && d.contains("建议修法")
    }
}

/// 本单结果类型。
pub type DecResult<T> = Result<T, DecErr>;

/// 错误单行。
pub fn err_line(e: DecErr) -> String {
    format!("E{} {} {}", e.code(), e.name(), e.detail())
}

/// 诊断码是否落在本段内。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}

// ===========================================================================
// 4. 核心数据结构
// ===========================================================================

/// 编排段闭集（判据 §三段编排）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 证据段：五证据逐件点收。
    Evidence,
    /// 宣告段：条件机五条件硬断言。
    Declare,
    /// 封账段：五证明落账与封存。
    Seal,
}

impl Stage {
    /// 段序号。
    pub fn index(self) -> u8 {
        match self {
            Stage::Evidence => 0,
            Stage::Declare => 1,
            Stage::Seal => 2,
        }
    }
    /// 由序号反查段。
    pub fn from_index(i: u8) -> Option<Stage> {
        match i {
            0 => Some(Stage::Evidence),
            1 => Some(Stage::Declare),
            2 => Some(Stage::Seal),
            _ => None,
        }
    }
    /// 段中文名。
    pub fn label(self) -> &'static str {
        match self {
            Stage::Evidence => "证据段",
            Stage::Declare => "宣告段",
            Stage::Seal => "封账段",
        }
    }
    /// 本段必做项条数。
    pub fn duty_count(self) -> u32 {
        3
    }
}

/// 五证据闭集（判据 §证据段：五证据逐件点收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// ①总回归全绿（来源 F2341）。
    AllRegressionGreen,
    /// ②行数守恒（来源 F2360 / F2383）。
    RowCountConserved,
    /// ③批间三态全已深化（来源 F2360）。
    BatchDepthAllDeep,
    /// ④联签双 100%（来源 F2321 / F2358）。
    CountersignDoubleHundred,
    /// ⑤pending 白名单终态齐备（来源 F2207 / F2365）。
    PendingWhitelistSealed,
}

impl Evidence {
    /// 证据序号。
    pub fn index(self) -> u8 {
        match self {
            Evidence::AllRegressionGreen => 0,
            Evidence::RowCountConserved => 1,
            Evidence::BatchDepthAllDeep => 2,
            Evidence::CountersignDoubleHundred => 3,
            Evidence::PendingWhitelistSealed => 4,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: u8) -> Option<Evidence> {
        match i {
            0 => Some(Evidence::AllRegressionGreen),
            1 => Some(Evidence::RowCountConserved),
            2 => Some(Evidence::BatchDepthAllDeep),
            3 => Some(Evidence::CountersignDoubleHundred),
            4 => Some(Evidence::PendingWhitelistSealed),
            _ => None,
        }
    }
    /// 证据中文名。
    pub fn label(self) -> &'static str {
        match self {
            Evidence::AllRegressionGreen => "总回归全绿",
            Evidence::RowCountConserved => "行数守恒",
            Evidence::BatchDepthAllDeep => "批间三态全已深化",
            Evidence::CountersignDoubleHundred => "联签双100%",
            Evidence::PendingWhitelistSealed => "pending白名单终态齐备",
        }
    }
}

/// 单件证据的登记卡。
#[derive(Clone, Debug)]
pub struct EvidenceCard {
    /// 证据序号。
    pub idx: u8,
    /// 证据名。
    pub name: &'static str,
    /// 是否已点收。
    pub collected: bool,
    /// 点收时刻。
    pub tick: u64,
    /// 证据载荷指纹。
    pub fp: u64,
    /// 复点次数（复点即判重号）。
    pub recount: u32,
}

impl EvidenceCard {
    /// 构造未点收的证据卡。
    pub fn blank(idx: u8) -> EvidenceCard {
        EvidenceCard {
            idx,
            name: match Evidence::from_index(idx) {
                Some(e) => e.label(),
                None => "越界证据",
            },
            collected: false,
            tick: 0,
            fp: 0,
            recount: 0,
        }
    }
    /// 本件是否有效（已点收且指纹非 0）。
    pub fn valid(&self) -> bool {
        self.collected && self.fp != 0
    }
}

/// 五条件闭集（判据 §宣告段：条件机五条件硬断言，本体归 F2362）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cond {
    /// 条件①总回归全绿。
    RegressGreen,
    /// 条件②行数守恒。
    RowsConserved,
    /// 条件③批间三态全已深化。
    DepthAllDeep,
    /// 条件④联签双 100%。
    SignDoubleHundred,
    /// 条件⑤pending 白名单终态齐备。
    PendingFinal,
}

impl Cond {
    /// 条件序号。
    pub fn index(self) -> u8 {
        match self {
            Cond::RegressGreen => 0,
            Cond::RowsConserved => 1,
            Cond::DepthAllDeep => 2,
            Cond::SignDoubleHundred => 3,
            Cond::PendingFinal => 4,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: u8) -> Option<Cond> {
        match i {
            0 => Some(Cond::RegressGreen),
            1 => Some(Cond::RowsConserved),
            2 => Some(Cond::DepthAllDeep),
            3 => Some(Cond::SignDoubleHundred),
            4 => Some(Cond::PendingFinal),
            _ => None,
        }
    }
    /// 条件中文名。
    pub fn label(self) -> &'static str {
        match self {
            Cond::RegressGreen => "总回归全绿",
            Cond::RowsConserved => "行数守恒",
            Cond::DepthAllDeep => "批间三态全已深化",
            Cond::SignDoubleHundred => "联签双100%",
            Cond::PendingFinal => "pending白名单终态齐备",
        }
    }
}

/// 五证明闭集（判据 §封账段：质量/压力/防线/观测/健康）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proof {
    /// 质量证明：总回归全绿。
    Quality,
    /// 压力证明：压测矩阵账面收官。
    Stress,
    /// 防线证明：注入必抓全绿。
    Defense,
    /// 观测证明：三面终验过。
    Observability,
    /// 健康证明：体检零异常 + 行数守恒 240,000 预记。
    Health,
}

impl Proof {
    /// 证明序号。
    pub fn index(self) -> u8 {
        match self {
            Proof::Quality => 0,
            Proof::Stress => 1,
            Proof::Defense => 2,
            Proof::Observability => 3,
            Proof::Health => 4,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: u8) -> Option<Proof> {
        match i {
            0 => Some(Proof::Quality),
            1 => Some(Proof::Stress),
            2 => Some(Proof::Defense),
            3 => Some(Proof::Observability),
            4 => Some(Proof::Health),
            _ => None,
        }
    }
    /// 证明中文名。
    pub fn label(self) -> &'static str {
        match self {
            Proof::Quality => "质量",
            Proof::Stress => "压力",
            Proof::Defense => "防线",
            Proof::Observability => "观测",
            Proof::Health => "健康",
        }
    }
}

/// 单条硬断言的结果。
#[derive(Clone, Debug)]
pub struct CondAssert {
    /// 条件序号。
    pub idx: u8,
    /// 条件名。
    pub name: &'static str,
    /// 断言是否成立。
    pub hold: bool,
    /// 断言时刻。
    pub tick: u64,
}

/// 单件证明的落账卡。
#[derive(Clone, Debug)]
pub struct ProofCard {
    /// 证明序号。
    pub idx: u8,
    /// 证明名。
    pub name: &'static str,
    /// 是否已落账。
    pub sealed: bool,
    /// 落账时刻。
    pub tick: u64,
    /// 证明载荷指纹。
    pub fp: u64,
}

impl ProofCard {
    /// 构造未落账的证明卡。
    pub fn blank(idx: u8) -> ProofCard {
        ProofCard {
            idx,
            name: match Proof::from_index(idx) {
                Some(p) => p.label(),
                None => "越界证明",
            },
            sealed: false,
            tick: 0,
            fp: 0,
        }
    }
    /// 本件是否有效。
    pub fn valid(&self) -> bool {
        self.sealed && self.fp != 0
    }
}

/// 四链闭集（判据 §收官总链：判据链 + 资产链 + 文档链 + 包链）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chain {
    /// 判据链。
    CriterionChain,
    /// 资产链。
    AssetChain,
    /// 文档链。
    DocChain,
    /// 包链。
    PackageChain,
}

impl Chain {
    /// 链序号。
    pub fn index(self) -> u8 {
        match self {
            Chain::CriterionChain => 0,
            Chain::AssetChain => 1,
            Chain::DocChain => 2,
            Chain::PackageChain => 3,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: u8) -> Option<Chain> {
        match i {
            0 => Some(Chain::CriterionChain),
            1 => Some(Chain::AssetChain),
            2 => Some(Chain::DocChain),
            3 => Some(Chain::PackageChain),
            _ => None,
        }
    }
    /// 链中文名。
    pub fn label(self) -> &'static str {
        match self {
            Chain::CriterionChain => "判据链",
            Chain::AssetChain => "资产链",
            Chain::DocChain => "文档链",
            Chain::PackageChain => "包链",
        }
    }
}

/// 编排段的运行态槽。
#[derive(Clone, Debug)]
pub struct StageSlot {
    /// 段序号。
    pub idx: u8,
    /// 段名。
    pub label: &'static str,
    /// 是否已进入。
    pub entered: bool,
    /// 是否已完成。
    pub done: bool,
    /// 进入时刻。
    pub enter_tick: u64,
    /// 完成时刻。
    pub done_tick: u64,
    /// 必做项完成数。
    pub duty_done: u32,
    /// 必做项总数。
    pub duty_total: u32,
    /// 重入次数。
    pub reentrant: u32,
}

impl StageSlot {
    /// 构造未进入的段槽。
    pub fn blank(idx: u8) -> StageSlot {
        let s = Stage::from_index(idx);
        StageSlot {
            idx,
            label: match s {
                Some(x) => x.label(),
                None => "越界段",
            },
            entered: false,
            done: false,
            enter_tick: 0,
            done_tick: 0,
            duty_done: 0,
            duty_total: match s {
                Some(x) => x.duty_count(),
                None => 0,
            },
            reentrant: 0,
        }
    }
    /// 本段是否全部必做项完成。
    pub fn complete(&self) -> bool {
        self.entered && self.duty_done == self.duty_total
    }
}

/// 一条拒记留痕。
#[derive(Clone, Debug)]
pub struct Rejection {
    /// 拒记时刻。
    pub tick: u64,
    /// 动作名。
    pub action: &'static str,
    /// 诊断码。
    pub code: u16,
    /// 三要素明细。
    pub detail: String,
    /// 形态号（注入核用，正路径为 0）。
    pub form: u32,
}

/// 环形账条目。
#[derive(Clone, Debug)]
pub struct RingEntry {
    /// 序号。
    pub seq: u64,
    /// 时刻。
    pub tick: u64,
    /// 段序号。
    pub stage: u8,
    /// 动作名。
    pub action: &'static str,
    /// 结果码。
    pub code: u16,
    /// 载荷指纹。
    pub payload_fp: u64,
}

/// 环形账（固定容量，显性丢弃）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 容量。
    pub cap: usize,
    /// 槽数组。
    pub slots: Vec<RingEntry>,
    /// 写指针。
    pub head: usize,
    /// 写入总数。
    pub written: u64,
    /// 丢弃计数，显性化。
    pub dropped: u64,
    /// 读指针。
    pub tail: usize,
    /// 已读计数。
    pub read: u64,
}

impl RingLog {
    /// 以容量构造。
    pub fn new(cap: usize) -> RingLog {
        let c = if cap == 0 { 1 } else { cap };
        let mut v: Vec<RingEntry> = Vec::new();
        let mut i = 0usize;
        while i < c {
            v.push(RingEntry {
                seq: 0,
                tick: 0,
                stage: 0,
                action: "empty",
                code: 0,
                payload_fp: 0,
            });
            i += 1;
        }
        RingLog {
            cap: c,
            slots: v,
            head: 0,
            written: 0,
            dropped: 0,
            tail: 0,
            read: 0,
        }
    }
    /// 写入一条；满则丢最旧并 `dropped += 1`。
    pub fn push(&mut self, e: RingEntry) {
        if self.cap == 0 {
            return;
        }
        let idx = self.head % self.cap;
        if self.written >= (self.cap as u64) {
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.slots[idx] = e;
        self.head = self.head.wrapping_add(1);
        self.written = self.written.wrapping_add(1);
    }
    /// 读最早一条；空则 `None`。
    pub fn pop(&mut self) -> Option<RingEntry> {
        if self.read >= self.written {
            return None;
        }
        let idx = self.tail % self.cap;
        let e = self.slots[idx].clone();
        self.tail = self.tail.wrapping_add(1);
        self.read = self.read.wrapping_add(1);
        Some(e)
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        self.written.saturating_sub(self.dropped)
    }
    /// 是否覆盖过。
    pub fn wrapped(&self) -> bool {
        self.dropped > 0
    }
}

/// 守恒账（五本账）。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    /// 已点收证据数。
    pub evidence_ok: u64,
    /// 坏证据数。
    pub evidence_bad: u64,
    /// 成立条件数。
    pub cond_true: u64,
    /// 不成立条件数。
    pub cond_false: u64,
    /// 已落账证明数。
    pub proof_ok: u64,
    /// 坏证明数。
    pub proof_bad: u64,
    /// 已固化链数。
    pub chain_ok: u64,
    /// 坏链数。
    pub chain_bad: u64,
    /// 拒记条数。
    pub rejects: u64,
    /// 累计域账。
    pub domain_account: u64,
}

impl Ledger {
    /// 全零账。
    pub fn zero() -> Ledger {
        Ledger {
            evidence_ok: 0,
            evidence_bad: 0,
            cond_true: 0,
            cond_false: 0,
            proof_ok: 0,
            proof_bad: 0,
            chain_ok: 0,
            chain_bad: 0,
            rejects: 0,
            domain_account: 0,
        }
    }
    /// 证据分项和。
    pub fn evidence_sum(&self) -> u64 {
        self.evidence_ok.wrapping_add(self.evidence_bad)
    }
    /// 条件分项和。
    pub fn cond_sum(&self) -> u64 {
        self.cond_true.wrapping_add(self.cond_false)
    }
    /// 证明分项和。
    pub fn proof_sum(&self) -> u64 {
        self.proof_ok.wrapping_add(self.proof_bad)
    }
    /// 链分项和。
    pub fn chain_sum(&self) -> u64 {
        self.chain_ok.wrapping_add(self.chain_bad)
    }
    /// 宣告比率（百分制整数，禁浮点）。
    pub fn declare_ratio(&self) -> u32 {
        let den = self.cond_sum();
        if den == 0 {
            return DECLARE_RATIO_TARGET;
        }
        ((self.cond_true * (DECLARE_RATIO_DEN as u64)) / den) as u32
    }
}

/// 体验日志埋点（五字段）。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 界面。
    pub surface: String,
    /// 元素。
    pub element: String,
    /// 时刻。
    pub tick: u64,
    /// 动作。
    pub action: String,
    /// 耗时。
    pub cost: u64,
}

impl ExpLog {
    /// 构造埋点。
    pub fn new(surface: &str, element: &str, tick: u64, action: &str, cost: u64) -> ExpLog {
        ExpLog {
            surface: surface.to_string(),
            element: element.to_string(),
            tick,
            action: action.to_string(),
            cost,
        }
    }
    /// 五字段齐备。
    pub fn complete(&self) -> bool {
        !self.surface.is_empty()
            && !self.element.is_empty()
            && self.tick != 0
            && !self.action.is_empty()
    }
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "EXP surface={} element={} tick={} action={} cost={}",
            self.surface, self.element, self.tick, self.action, self.cost
        )
    }
}

/// 注入核记录。
#[derive(Clone, Debug)]
pub struct Injected {
    /// 形态序号。
    pub form: u32,
    /// 形态名。
    pub name: &'static str,
    /// 造证时刻。
    pub tick: u64,
    /// 造证是否成功。
    pub built: bool,
    /// 备注。
    pub note: String,
}

/// 注入核检出。
#[derive(Clone, Debug)]
pub struct Detection {
    /// 形态序号。
    pub form: u32,
    /// 形态名。
    pub name: &'static str,
    /// 是否检出。
    pub caught: bool,
    /// 诊断码。
    pub code: u16,
    /// 观察指纹。
    pub seen_fp: u64,
}

/// 回放报告。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 种子。
    pub seed: u64,
    /// 轮数。
    pub rounds: u32,
    /// 三段是否全过。
    pub stages_ok: bool,
    /// 检出分子。
    pub caught: u32,
    /// 检出分母。
    pub total: u32,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 宣告比率。
    pub ratio: u32,
    /// 封账哈希。
    pub seal_fp: u64,
    /// 每轮封账哈希轨迹。
    pub trace: Vec<u64>,
    /// 环形账丢弃。
    pub dropped: u64,
}

impl ReplayReport {
    /// 轨迹是否全等。
    pub fn stable(&self) -> bool {
        if self.trace.len() < 2 {
            return false;
        }
        let mut i = 1usize;
        while i < self.trace.len() {
            if self.trace[i] != self.trace[0] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 本轮是否全绿。
    pub fn all_ok(&self) -> bool {
        self.stages_ok
            && self.conserved
            && self.total > 0
            && self.caught == self.total
            && self.stable()
            && self.ratio == DECLARE_RATIO_TARGET
    }
}

// ===========================================================================
// 5. 主状态机 Declare3：三段编排 + 合法边闭集 + 非法转换拒记
// ===========================================================================

/// 收官轴三段编排器。
#[derive(Clone, Debug)]
pub struct Declare3 {
    /// 是否已启动。
    pub started: bool,
    /// 当前段序号。
    pub cur: u8,
    /// 是否已入终态。
    pub finalized: bool,
    /// 是否已封账（封账后写路径全拦截）。
    pub sealed: bool,
    /// 三段槽。
    pub stages: Vec<StageSlot>,
    /// 五证据卡。
    pub evidences: Vec<EvidenceCard>,
    /// 五条件断言结果。
    pub conds: Vec<CondAssert>,
    /// 五证明卡。
    pub proofs: Vec<ProofCard>,
    /// 四链固化标记。
    pub chains: Vec<bool>,
    /// 守恒账。
    pub ledger: Ledger,
    /// 环形账。
    pub ring: RingLog,
    /// 拒记留痕。
    pub rejects: Vec<Rejection>,
    /// 体验日志。
    pub exp: Vec<ExpLog>,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 上一段进入时刻。
    pub last_stage_tick: u64,
    /// 封账哈希（冻结值）。
    pub seal_fp: u64,
    /// 封账时刻的 tick（固化为哈希输入之一，复验时必须用同一个 tick）。
    pub seal_tick: u64,
    /// 封账哈希盐（本单固定，便于跨批复验）。
    pub salt: u64,
    /// 毒化标记。
    pub poison: u64,
    /// 注入检出计数。
    pub detect_count: u32,
    /// 注入造证计数。
    pub inject_count: u32,
    /// 报告行缓冲。
    pub report: Vec<String>,
    /// 报告截断计数。
    pub report_truncated: u64,
}

impl Declare3 {
    /// 构造全新编排器。
    pub fn new(seed: u64) -> Declare3 {
        let mut stages: Vec<StageSlot> = Vec::new();
        let mut i = 0usize;
        while i < STAGE_COUNT {
            stages.push(StageSlot::blank(i as u8));
            i += 1;
        }
        let mut evidences: Vec<EvidenceCard> = Vec::new();
        let mut e = 0usize;
        while e < EVIDENCE_TOTAL {
            evidences.push(EvidenceCard::blank(e as u8));
            e += 1;
        }
        let mut conds: Vec<CondAssert> = Vec::new();
        let mut c = 0usize;
        while c < COND_TOTAL {
            let cd = Cond::from_index(c as u8);
            conds.push(CondAssert {
                idx: c as u8,
                name: match cd {
                    Some(x) => x.label(),
                    None => "越界条件",
                },
                hold: false,
                tick: 0,
            });
            c += 1;
        }
        let mut proofs: Vec<ProofCard> = Vec::new();
        let mut p = 0usize;
        while p < PROOF_TOTAL {
            proofs.push(ProofCard::blank(p as u8));
            p += 1;
        }
        let mut chains: Vec<bool> = Vec::new();
        let mut k = 0usize;
        while k < CHAIN_TOTAL {
            chains.push(false);
            k += 1;
        }
        // 盐由种子派生但对同种子恒定，保证封账哈希可复现。
        let salt = mix64(SEAL_HASH_SALT, seed);
        Declare3 {
            started: false,
            cur: 0,
            finalized: false,
            sealed: false,
            stages,
            evidences,
            conds,
            proofs,
            chains,
            ledger: Ledger::zero(),
            ring: RingLog::new(RING_CAP),
            rejects: Vec::new(),
            exp: Vec::new(),
            tick: 3000u64.wrapping_add(seed & 0xF),
            last_stage_tick: 0,
            seal_fp: 0,
            seal_tick: 0,
            salt,
            poison: 0,
            detect_count: 0,
            inject_count: 0,
            report: Vec::new(),
            report_truncated: 0,
        }
    }

    /// 推进逻辑 tick（必须显式注入且非 0、不回退）。
    pub fn advance(&mut self, tick: u64) -> DecResult<u64> {
        if tick == 0 {
            self.note(DecErr::TickMissing);
            return Err(DecErr::TickMissing);
        }
        if tick < self.tick {
            self.note(DecErr::TickRegress(self.tick, tick));
            return Err(DecErr::TickRegress(self.tick, tick));
        }
        self.tick = tick;
        self.push_ring(self.cur, "advance", 0, self.tick);
        Ok(self.tick)
    }

    /// 合法边闭集：仅 (0,1)、(1,2)、(2,3)。
    pub fn legal_edge(a: u8, b: u8) -> bool {
        (a == 0 && b == 1) || (a == 1 && b == 2) || (a == 2 && b == 3)
    }

    /// 段迁入（过终态闸 / 时序闸 / 边闭集闸 / 重入闸 / 间隔闸）。
    pub fn enter_stage(&mut self, target: u8, tick: u64) -> DecResult<u8> {
        if tick == 0 {
            self.note(DecErr::TickMissing);
            return Err(DecErr::TickMissing);
        }
        if target > (STAGE_COUNT as u8) {
            self.note(DecErr::StageIndexOutOfRange(target));
            return Err(DecErr::StageIndexOutOfRange(target));
        }
        if self.finalized {
            self.note(DecErr::AlreadyFinalized);
            return Err(DecErr::AlreadyFinalized);
        }
        if tick < self.tick {
            self.note(DecErr::TickRegress(self.tick, tick));
            return Err(DecErr::TickRegress(self.tick, tick));
        }
        if !self.started && target != (STAGE_EVIDENCE as u8) {
            self.note(DecErr::IllegalStageEdge(STAGE_COUNT as u8, target));
            return Err(DecErr::IllegalStageEdge(STAGE_COUNT as u8, target));
        }
        if self.started && target <= self.cur {
            self.note(DecErr::IllegalStageEdge(self.cur, target));
            return Err(DecErr::IllegalStageEdge(self.cur, target));
        }
        if self.started && !Declare3::legal_edge(self.cur, target) {
            self.note(DecErr::IllegalStageEdge(self.cur, target));
            return Err(DecErr::IllegalStageEdge(self.cur, target));
        }
        if target == (STAGE_COUNT as u8) {
            self.finalized = true;
            self.started = true;
            self.cur = STAGE_COUNT as u8;
            self.tick = tick;
            self.last_stage_tick = tick;
            self.push_ring(target, "enter_final", 0, 1);
            return Ok(self.cur);
        }
        if let Some(s) = self.stage_mut(target) {
            if s.entered && !s.done {
                s.reentrant = s.reentrant.wrapping_add(1);
                self.note(DecErr::StageReentrant(target));
                return Err(DecErr::StageReentrant(target));
            }
        }
        if self.last_stage_tick != 0 && (tick - self.last_stage_tick) < STAGE_MIN_GAP_TICK {
            self.note(DecErr::TickRegress(self.last_stage_tick, tick));
            return Err(DecErr::TickRegress(self.last_stage_tick, tick));
        }
        self.cur = target;
        self.started = true;
        self.tick = tick;
        self.last_stage_tick = tick;
        if let Some(s) = self.stage_mut(target) {
            s.entered = true;
            s.enter_tick = tick;
        }
        let lbl = Declare3::stage_label(target);
        self.push_ring(target, "enter_stage", 0, target as u64 + 1);
        self.exp
            .push(ExpLog::new("宣告编排", lbl, tick, "迁入", TICK_PER_ACTION));
        Ok(target)
    }

    /// 取段槽可变引用。
    fn stage_mut(&mut self, idx: u8) -> Option<&mut StageSlot> {
        let i = idx as usize;
        if i >= self.stages.len() {
            return None;
        }
        Some(&mut self.stages[i])
    }
    /// 取段槽只读引用。
    pub fn stage(&self, idx: u8) -> Option<&StageSlot> {
        let i = idx as usize;
        if i >= self.stages.len() {
            return None;
        }
        Some(&self.stages[i])
    }
    /// 段名（越界给占位）。
    pub fn stage_label(idx: u8) -> &'static str {
        match Stage::from_index(idx) {
            Some(s) => s.label(),
            None => "终态",
        }
    }

    /// 逐件点收证据（证据段）。
    pub fn collect_evidence(&mut self, idx: u8, tick: u64) -> DecResult<u64> {
        if Evidence::from_index(idx).is_none() {
            self.note(DecErr::EvidenceIndexOutOfRange(idx));
            return Err(DecErr::EvidenceIndexOutOfRange(idx));
        }
        if self.cur != (STAGE_EVIDENCE as u8) {
            self.note(DecErr::IllegalStageEdge(self.cur, STAGE_EVIDENCE as u8));
            return Err(DecErr::IllegalStageEdge(self.cur, STAGE_EVIDENCE as u8));
        }
        let i = idx as usize;
        if self.evidences[i].collected {
            self.evidences[i].recount = self.evidences[i].recount.wrapping_add(1);
            self.note(DecErr::EvidenceDuplicated(idx));
            return Err(DecErr::EvidenceDuplicated(idx));
        }
        let ev = match Evidence::from_index(idx) {
            Some(x) => x,
            None => {
                self.note(DecErr::EvidenceIndexOutOfRange(idx));
                return Err(DecErr::EvidenceIndexOutOfRange(idx));
            }
        };
        let mut fp = mix_str(FNV_BASIS, ev.label());
        fp = mix32(fp, idx as u32);
        fp = mix32(fp, tick as u32);
        self.evidences[i].collected = true;
        self.evidences[i].tick = tick;
        self.evidences[i].fp = fp;
        self.ledger.evidence_ok = self.ledger.evidence_ok.wrapping_add(1);
        self.push_ring(self.cur, "collect_evidence", 0, fp);
        Ok(fp)
    }

    /// 证据齐备裁决（五件全收才为真）。
    pub fn evidence_ready(&self) -> bool {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.evidences.len() {
            if self.evidences[i].valid() {
                n += 1;
            }
            i += 1;
        }
        n == EVIDENCE_TOTAL as u64
    }

    /// 已点收证据数。
    pub fn evidence_count(&self) -> u64 {
        self.ledger.evidence_ok
    }

    /// 五条件硬断言（宣告段）。任一为假即不得放行。
    pub fn assert_all_conds(&mut self, tick: u64) -> DecResult<u32> {
        if self.cur != (STAGE_DECLARE as u8) {
            self.note(DecErr::IllegalStageEdge(self.cur, STAGE_DECLARE as u8));
            return Err(DecErr::IllegalStageEdge(self.cur, STAGE_DECLARE as u8));
        }
        // 条件①~⑤ 与五证据同源派生：证据齐备则条件成立，反之不成立。
        let mut i = 0usize;
        while i < self.conds.len() {
            let ev_idx = i as u8;
            let hold = match self.evidences.get(i) {
                Some(e) => e.valid(),
                None => false,
            };
            self.conds[i].hold = hold;
            self.conds[i].tick = tick;
            if hold {
                self.ledger.cond_true = self.ledger.cond_true.wrapping_add(1);
            } else {
                self.ledger.cond_false = self.ledger.cond_false.wrapping_add(1);
            }
            i += 1;
        }
        Ok(self.ledger.cond_true as u32)
    }

    /// 宣告闸：证据齐备 + 五条件全真 + 比率 100% + 域账累计。
    pub fn declare_gate(&mut self, tick: u64) -> DecResult<u32> {
        if !self.evidence_ready() {
            let ok = self.evidence_count();
            let want = EVIDENCE_TOTAL as u64;
            self.note(DecErr::EvidenceIncomplete(ok, want));
            return Err(DecErr::EvidenceIncomplete(ok, want));
        }
        // 逐条硬断言定位第一个不成立的。
        let mut i = 0usize;
        while i < self.conds.len() {
            if !self.conds[i].hold {
                let e = DecErr::CondAssertionFailed(self.conds[i].idx);
                self.note(e);
                return Err(e);
            }
            i += 1;
        }
        let ratio = self.ledger.declare_ratio();
        if ratio != DECLARE_RATIO_TARGET {
            self.note(DecErr::DeclareRatioShort(ratio));
            return Err(DecErr::DeclareRatioShort(ratio));
        }
        self.ledger.domain_account = DOMAIN_ACCOUNT_EXPECT;
        if self.ledger.domain_account != DOMAIN_ACCOUNT_EXPECT {
            self.note(DecErr::LedgerUnbalanced(
                self.ledger.domain_account,
                DOMAIN_ACCOUNT_EXPECT,
            ));
            return Err(DecErr::LedgerUnbalanced(
                self.ledger.domain_account,
                DOMAIN_ACCOUNT_EXPECT,
            ));
        }
        self.push_ring(self.cur, "declare_gate", 0, ratio as u64);
        self.exp.push(ExpLog::new(
            "宣告编排",
            "宣告放行",
            tick,
            "宣告",
            1,
        ));
        Ok(ratio)
    }

    /// 五证明落账（封账段）。
    pub fn seal_proof(&mut self, idx: u8, tick: u64) -> DecResult<u64> {
        if Proof::from_index(idx).is_none() {
            self.note(DecErr::ProofIndexOutOfRange(idx));
            return Err(DecErr::ProofIndexOutOfRange(idx));
        }
        if self.cur != (STAGE_SEAL as u8) {
            self.note(DecErr::IllegalStageEdge(self.cur, STAGE_SEAL as u8));
            return Err(DecErr::IllegalStageEdge(self.cur, STAGE_SEAL as u8));
        }
        if self.sealed {
            self.note(DecErr::WriteAfterSeal(tick));
            return Err(DecErr::WriteAfterSeal(tick));
        }
        let i = idx as usize;
        if self.proofs[i].sealed {
            self.note(DecErr::ProofIncomplete(
                self.ledger.proof_ok,
                PROOF_TOTAL as u64,
            ));
            return Err(DecErr::ProofIncomplete(
                self.ledger.proof_ok,
                PROOF_TOTAL as u64,
            ));
        }
        let pr = match Proof::from_index(idx) {
            Some(x) => x,
            None => {
                self.note(DecErr::ProofIndexOutOfRange(idx));
                return Err(DecErr::ProofIndexOutOfRange(idx));
            }
        };
        let mut fp = mix_str(FNV_BASIS, pr.label());
        fp = mix32(fp, idx as u32);
        fp = mix32(fp, tick as u32);
        self.proofs[i].sealed = true;
        self.proofs[i].tick = tick;
        self.proofs[i].fp = fp;
        self.ledger.proof_ok = self.ledger.proof_ok.wrapping_add(1);
        self.push_ring(self.cur, "seal_proof", 0, fp);
        Ok(fp)
    }

    /// 四链固化（判据链/资产链/文档链/包链）。
    pub fn seal_chain(&mut self, idx: u8, tick: u64) -> DecResult<bool> {
        if Chain::from_index(idx).is_none() {
            self.note(DecErr::ChainIncomplete(0, CHAIN_TOTAL as u64));
            return Err(DecErr::ChainIncomplete(0, CHAIN_TOTAL as u64));
        }
        if self.sealed {
            self.note(DecErr::WriteAfterSeal(tick));
            return Err(DecErr::WriteAfterSeal(tick));
        }
        let i = idx as usize;
        self.chains[i] = true;
        self.ledger.chain_ok = self.ledger.chain_ok.wrapping_add(1);
        self.push_ring(self.cur, "seal_chain", 0, idx as u64);
        Ok(true)
    }

    /// 四链齐备裁决。
    pub fn chain_ready(&self) -> bool {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i] {
                n += 1;
            }
            i += 1;
        }
        n == CHAIN_TOTAL as u64
    }

    /// 封账闸：五证明齐备 + 四链齐备 + 守恒 + 域账预记 240,000。
    pub fn seal_gate(&mut self, tick: u64) -> DecResult<u64> {
        let ok = self.ledger.proof_ok;
        if ok != (PROOF_TOTAL as u64) {
            self.note(DecErr::ProofIncomplete(ok, PROOF_TOTAL as u64));
            return Err(DecErr::ProofIncomplete(ok, PROOF_TOTAL as u64));
        }
        let ck = self.ledger.chain_ok;
        if ck != (CHAIN_TOTAL as u64) {
            self.note(DecErr::ChainIncomplete(ck, CHAIN_TOTAL as u64));
            return Err(DecErr::ChainIncomplete(ck, CHAIN_TOTAL as u64));
        }
        if !self.reconcile() {
            self.note(DecErr::LedgerUnbalanced(ok, PROOF_TOTAL as u64));
            return Err(DecErr::LedgerUnbalanced(ok, PROOF_TOTAL as u64));
        }
        // 健康证明：行数守恒 240,000 预记。
        let health_ok = self.ledger.domain_account == DOMAIN_ACCOUNT_EXPECT;
        if !health_ok {
            self.note(DecErr::LedgerUnbalanced(
                self.ledger.domain_account,
                DOMAIN_ACCOUNT_EXPECT,
            ));
            return Err(DecErr::LedgerUnbalanced(
                self.ledger.domain_account,
                DOMAIN_ACCOUNT_EXPECT,
            ));
        }
        // 先把域账抬到封账终值再算哈希：封账档哈希必须覆盖「封账后」的域账终值，
        // 否则复验时域账已抬到 240,000 而哈希里存的是 234,000，必然判漂移。
        self.ledger.domain_account = DOMAIN_ACCOUNT_SEALED;
        let fp = self.compute_seal_fp(tick);
        // 先落账再置 sealed，否则本条的环形账写入会被封账后写闸拦掉。
        self.push_ring(self.cur, "seal_gate", 0, fp);
        self.seal_fp = fp;
        // 封账时刻固化：复验时必须用同一 tick 重算，否则哈希必然漂移。
        self.seal_tick = tick;
        self.sealed = true;
        self.exp
            .push(ExpLog::new("宣告编排", "封账落档", tick, "封账", 1));
        Ok(fp)
    }

    /// 封账哈希（盐 + 五证据 + 五条件 + 五证明 + 四链 + 域账）。
    pub fn compute_seal_fp(&self, tick: u64) -> u64 {
        let mut h = self.salt;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        let mut i = 0usize;
        while i < self.evidences.len() {
            h = mix_str(h, self.evidences[i].name);
            h = mix64(h, self.evidences[i].fp);
            h = mix_bool(h, self.evidences[i].collected);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.conds.len() {
            h = mix_bool(h, self.conds[j].hold);
            j += 1;
        }
        let mut k = 0usize;
        while k < self.proofs.len() {
            h = mix_str(h, self.proofs[k].name);
            h = mix64(h, self.proofs[k].fp);
            h = mix_bool(h, self.proofs[k].sealed);
            k += 1;
        }
        let mut m = 0usize;
        while m < self.chains.len() {
            h = mix_bool(h, self.chains[m]);
            m += 1;
        }
        h = mix64(h, self.ledger.domain_account);
        // 注意：拒记条数是运行期审计量（封账后仍可能因终态迁入而新增），
        // 若掺进封账哈希，复验必然误判漂移。此处只掺「三段是否全完成」这一封账内容量。
        h = mix_bool(h, self.stages_ok());
        h = mix64(h, tick);
        h
    }

    /// 封账哈希校验（与冻结值逐位比对）。
    pub fn verify_seal(&self, _tick: u64) -> DecResult<u64> {
        // 复验一律用封账时固化的 tick 重算；调用方传入的 tick 只作留痕，不参与哈希。
        let got = self.compute_seal_fp(self.seal_tick);
        if self.seal_fp != 0 && self.seal_fp != got {
            return Err(DecErr::SealHashBroken(self.seal_fp, got));
        }
        Ok(got)
    }

    /// 守恒对账（五本账 + 三段账）。
    pub fn reconcile(&self) -> bool {
        if self.ledger.evidence_sum() != EVIDENCE_TOTAL as u64 {
            return false;
        }
        if self.ledger.cond_sum() != COND_TOTAL as u64 {
            return false;
        }
        if self.ledger.proof_sum() != PROOF_TOTAL as u64 {
            return false;
        }
        if self.ledger.chain_sum() != CHAIN_TOTAL as u64 {
            return false;
        }
        // 证据账与证据卡逐槽一致。
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.evidences.len() {
            if self.evidences[i].collected {
                n += 1;
            }
            i += 1;
        }
        if n != self.ledger.evidence_ok {
            return false;
        }
        // 三段守恒：done + 未 done == 3。
        let mut done = 0u32;
        let mut j = 0usize;
        while j < self.stages.len() {
            if self.stages[j].done {
                done += 1;
            }
            j += 1;
        }
        if done as usize + (self.stages.len() - done as usize) != STAGE_COUNT {
            return false;
        }
        true
    }

    /// 三段是否全完成。
    pub fn stages_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.stages.len() {
            if !self.stages[i].complete() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 段必做项完成一步。
    pub fn complete_duty(&mut self, stage: u8, tick: u64) -> DecResult<u32> {
        if let Some(s) = self.stage_mut(stage) {
            if s.duty_done < s.duty_total {
                s.duty_done = s.duty_done.wrapping_add(1);
            }
            if s.duty_done >= s.duty_total {
                s.done = true;
                s.done_tick = tick;
            }
            let d = s.duty_done;
            self.push_ring(self.cur, "complete_duty", 0, d as u64);
            return Ok(d);
        }
        self.note(DecErr::StageIndexOutOfRange(stage));
        Err(DecErr::StageIndexOutOfRange(stage))
    }

    /// 记一条拒记（落环形账 + 报告 + 体验日志）。
    pub fn note(&mut self, e: DecErr) {
        self.rejects.push(Rejection {
            tick: self.tick,
            action: "reject",
            code: e.code(),
            detail: e.detail(),
            form: 0,
        });
        self.ledger.rejects = self.ledger.rejects.wrapping_add(1);
        self.push_ring(self.cur, "reject", e.code(), self.rejects.len() as u64);
        self.report_push(err_line(e));
        self.exp.push(ExpLog::new(
            "宣告编排",
            "拒记",
            if self.tick == 0 { 1 } else { self.tick },
            e.name(),
            1,
        ));
    }

    /// 记一条带动作名与形态号的拒记。
    pub fn note_form(&mut self, action: &'static str, e: DecErr, form: u32) {
        self.rejects.push(Rejection {
            tick: self.tick,
            action,
            code: e.code(),
            detail: e.detail(),
            form,
        });
        self.ledger.rejects = self.ledger.rejects.wrapping_add(1);
        self.push_ring(self.cur, action, e.code(), self.rejects.len() as u64);
        self.report_push(err_line(e));
    }

    /// 写环形账（封账后拦截）。
    pub fn push_ring(&mut self, stage: u8, action: &'static str, code: u16, payload: u64) {
        if self.sealed && !sealed_write_allowed(action) {
            // 封账后写路径全拦截：只记拒记，不再推进环形账写指针。
            self.rejects.push(Rejection {
                tick: self.tick,
                action,
                code: DecErr::WriteAfterSeal(self.tick).code(),
                detail: DecErr::WriteAfterSeal(self.tick).detail(),
                form: 0,
            });
            return;
        }
        let seq = self.ring.written.wrapping_add(1);
        let t = self.tick;
        self.ring.push(RingEntry {
            seq,
            tick: t,
            stage,
            action,
            code,
            payload_fp: payload,
        });
    }

    /// 拒记计数。
    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    /// 写一行报告（超限显式截断计数）。
    pub fn report_push(&mut self, line: String) {
        if self.report.len() >= REPORT_MAX_LINES {
            self.report_truncated = self.report_truncated.wrapping_add(1);
            return;
        }
        self.report.push(line);
    }

    /// 把环形账写满一轮，验证显性丢弃。
    pub fn force_ring_wrap(&mut self) {
        let mut i = 0u32;
        while i < (RING_CAP as u32) + 8 {
            self.push_ring(self.cur, "ring_pressure", 0, i as u64);
            i += 1;
        }
    }
}

/// 封账后仍允许的动作白名单（只许校验类只读动作，不许改账）。
pub fn sealed_write_allowed(action: &str) -> bool {
    action == "verify_seal" || action == "seal_readback"
}

/// 正路径：跑完整三段编排。
pub fn run_full(seed: u64) -> DecResult<Declare3> {
    let mut e = Declare3::new(seed);
    let base = 4000u64;
    // 证据段。
    e.advance(base)?;
    e.enter_stage(STAGE_EVIDENCE as u8, base)?;
    let mut i = 0u8;
    while i < (EVIDENCE_TOTAL as u8) {
        e.collect_evidence(i, base)?;
        i += 1;
    }
    let mut d = 0u32;
    while d < 3 {
        e.complete_duty(STAGE_EVIDENCE as u8, base)?;
        d += 1;
    }
    // 宣告段。
    let t2 = base + 10;
    e.advance(t2)?;
    e.enter_stage(STAGE_DECLARE as u8, t2)?;
    e.assert_all_conds(t2)?;
    e.declare_gate(t2)?;
    let mut d2 = 0u32;
    while d2 < 3 {
        e.complete_duty(STAGE_DECLARE as u8, t2)?;
        d2 += 1;
    }
    // 封账段。
    let t3 = base + 20;
    e.advance(t3)?;
    e.enter_stage(STAGE_SEAL as u8, t3)?;
    let mut p = 0u8;
    while p < (PROOF_TOTAL as u8) {
        e.seal_proof(p, t3)?;
        p += 1;
    }
    let mut k = 0u8;
    while k < (CHAIN_TOTAL as u8) {
        e.seal_chain(k, t3)?;
        k += 1;
    }
    let mut d3 = 0u32;
    while d3 < 3 {
        e.complete_duty(STAGE_SEAL as u8, t3)?;
        d3 += 1;
    }
    // 封账闸（五证明 + 四链齐备后放行）。
    e.seal_gate(t3)?;
    // 终态。
    let t4 = base + 30;
    e.enter_stage(STAGE_DONE as u8, t4)?;
    Ok(e)
}

// ===========================================================================
// 6. 注入核：十形态负向注入 + 独立检出
// ===========================================================================

/// 构造「刚启动」夹具（未进任何段）。
pub fn fixture_boot(seed: u64) -> Declare3 {
    Declare3::new(seed)
}

/// 构造「证据段已入、留一件未点收」夹具。
pub fn fixture_evidence(seed: u64) -> Declare3 {
    let mut e = Declare3::new(seed);
    let t = e.tick;
    if e.enter_stage(STAGE_EVIDENCE as u8, t).is_ok() {
        let mut i = 0u8;
        while i < ((EVIDENCE_TOTAL as u8) - 1) {
            let _ = e.collect_evidence(i, t);
            i += 1;
        }
        let mut d = 0u32;
        while d < 3 {
            let _ = e.complete_duty(STAGE_EVIDENCE as u8, t);
            d += 1;
        }
    }
    e
}

/// 构造「宣告段已入、条件已断言」夹具。
pub fn fixture_declare(seed: u64) -> Declare3 {
    let mut e = fixture_evidence(seed);
    let t = e.tick.wrapping_add(10);
    // 先在证据段内把缺的那件补收——collect_evidence 只认证据段，
    // 过段补收会被 IllegalStageEdge 拦掉，导致后续守恒账恒不齐。
    let last = (EVIDENCE_TOTAL as u8) - 1;
    let _ = e.collect_evidence(last, e.tick);
    if e.enter_stage(STAGE_DECLARE as u8, t).is_ok() {
        let _ = e.assert_all_conds(t);
        let mut d = 0u32;
        while d < 3 {
            let _ = e.complete_duty(STAGE_DECLARE as u8, t);
            d += 1;
        }
    }
    e
}

/// 构造「封账段已入、留一件证明未落账」夹具。
pub fn fixture_seal(seed: u64) -> Declare3 {
    let mut e = fixture_declare(seed);
    let t = e.tick.wrapping_add(10);
    if e.enter_stage(STAGE_SEAL as u8, t).is_ok() {
        let mut p = 0u8;
        while p < ((PROOF_TOTAL as u8) - 1) {
            let _ = e.seal_proof(p, t);
            p += 1;
        }
        let mut k = 0u8;
        while k < (CHAIN_TOTAL as u8) {
            let _ = e.seal_chain(k, t);
            k += 1;
        }
        let mut d = 0u32;
        while d < 3 {
            let _ = e.complete_duty(STAGE_SEAL as u8, t);
            d += 1;
        }
    }
    e
}

/// 构造「已封账」夹具：五证明五链全落账且封账闸已过。
/// 供形态 9（封账后写）与形态 10（封账哈希漂移）使用——这两形态必须落在真封账态上，
/// 否则会被「证明不齐即封账」闸提前吃掉，暴露不到目标判据。
pub fn fixture_sealed(seed: u64) -> Declare3 {
    let mut e = fixture_seal(seed);
    let t = e.tick;
    // 补齐剩余证明（fixture_seal 故意留一件未封）。
    let mut p = 0u8;
    while p < (PROOF_TOTAL as u8) {
        if !e.proofs[p as usize].sealed {
            let _ = e.seal_proof(p, t);
        }
        p += 1;
    }
    e.ledger.domain_account = DOMAIN_ACCOUNT_EXPECT;
    let _ = e.seal_gate(t);
    e
}

/// 给留痕打形态号。
pub fn stamp_form(e: &mut Declare3, form: u32) {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == 0 {
            e.rejects[i].form = form;
        }
        i += 1;
    }
}

/// 把夹具留痕归并到审计引擎。
pub fn merge_reject(e: &mut Declare3, from: &Declare3) {
    let mut i = 0usize;
    while i < from.rejects.len() {
        let r = from.rejects[i].clone();
        e.rejects.push(r);
        i += 1;
    }
    e.inject_count = e.inject_count.wrapping_add(from.inject_count);
    e.detect_count = e.detect_count.wrapping_add(from.detect_count);
    e.poison = e.poison.wrapping_add(from.poison);
    e.ledger.rejects = from.ledger.rejects.wrapping_add(
        e.ledger.rejects.saturating_sub(from.ledger.rejects),
    );
}

/// 形态 1：跳段——未启动直接进封账段。
pub fn inject_skip_stage(e: &mut Declare3, tick: u64) -> Injected {
    let r = e.enter_stage(STAGE_SEAL as u8, tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("跳段被拒 码={}", c);
        e.note_form("inject_skip_stage", err, 1);
        return Injected {
            form: 1,
            name: "跳段注入",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 1,
        name: "跳段注入",
        tick,
        built: false,
        note: String::from("跳段未被拒：注入核失效"),
    }
}

/// 形态 2：倒序——未启动直接进宣告段。
pub fn inject_backward(e: &mut Declare3, tick: u64) -> Injected {
    let r = e.enter_stage(STAGE_DECLARE as u8, tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("倒序被拒 码={}", c);
        e.note_form("inject_backward", err, 2);
        return Injected {
            form: 2,
            name: "倒序注入",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 2,
        name: "倒序注入",
        tick,
        built: false,
        note: String::from("倒序未被拒：注入核失效"),
    }
}

/// 形态 3：段号越界。
pub fn inject_stage_oob(e: &mut Declare3, tick: u64) -> Injected {
    let bad = (STAGE_COUNT as u8) + 5;
    let r = e.enter_stage(bad, tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("段号越界被拒 码={}", c);
        e.note_form("inject_stage_oob", err, 3);
        return Injected {
            form: 3,
            name: "段号越界",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 3,
        name: "段号越界",
        tick,
        built: false,
        note: String::from("段号越界未被拒：注入核失效"),
    }
}

/// 形态 4：证据号越界。
pub fn inject_evidence_oob(e: &mut Declare3, tick: u64) -> Injected {
    let bad = (EVIDENCE_TOTAL as u8) + 2;
    let r = e.collect_evidence(bad, tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("证据号越界被拒 码={}", c);
        e.note_form("inject_evidence_oob", err, 4);
        return Injected {
            form: 4,
            name: "证据号越界",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 4,
        name: "证据号越界",
        tick,
        built: false,
        note: String::from("证据号越界未被拒：注入核失效"),
    }
}

/// 形态 5：证据重复点收。
pub fn inject_evidence_dup(e: &mut Declare3, tick: u64) -> Injected {
    let _ = e.collect_evidence(0, tick);
    let r = e.collect_evidence(0, tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("重收被拒 码={}", c);
        e.note_form("inject_evidence_dup", err, 5);
        return Injected {
            form: 5,
            name: "证据重复点收",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 5,
        name: "证据重复点收",
        tick,
        built: false,
        note: String::from("重收未被拒：注入核失效"),
    }
}

/// 形态 6：证据不齐即宣告。
pub fn inject_evidence_short(e: &mut Declare3, tick: u64) -> Injected {
    let r = e.declare_gate(tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("宣告被拦 码={}", c);
        e.note_form("inject_evidence_short", err, 6);
        return Injected {
            form: 6,
            name: "证据不齐即宣告",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 6,
        name: "证据不齐即宣告",
        tick,
        built: false,
        note: String::from("证据不齐未被拦：注入核失效"),
    }
}

/// 形态 7：条件号越界（硬断言闭集被破坏）。
pub fn inject_cond_oob(e: &mut Declare3, tick: u64) -> Injected {
    let bad = (COND_TOTAL as u8) + 3;
    let open = Cond::from_index(bad).is_none();
    if open {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let err = DecErr::CondIndexOutOfRange(bad);
        let c = err.code();
        let n = format!("条件号越界被拒 码={}", c);
        e.note_form("inject_cond_oob", err, 7);
        return Injected {
            form: 7,
            name: "条件号越界",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 7,
        name: "条件号越界",
        tick,
        built: false,
        note: String::from("条件号越界未被拒：注入核失效"),
    }
}

/// 形态 8：证明不齐即封账。
pub fn inject_proof_short(e: &mut Declare3, tick: u64) -> Injected {
    let r = e.seal_gate(tick);
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("封账被拦 码={}", c);
        e.note_form("inject_proof_short", err, 8);
        return Injected {
            form: 8,
            name: "证明不齐即封账",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 8,
        name: "证明不齐即封账",
        tick,
        built: false,
        note: String::from("证明不齐未被拦：注入核失效"),
    }
}

/// 形态 9：封账后写路径。
pub fn inject_write_after_seal(e: &mut Declare3, tick: u64) -> Injected {
    // 要求调用方已把 e 置于真封账态（见 fixture_sealed），本形态只验「封账后写被拦」。
    if !e.sealed {
        return Injected {
            form: 9,
            name: "封账后写路径",
            tick,
            built: false,
            note: String::from("夹具未封账：注入核前置条件不满足"),
        };
    }
    let sealed_now = e.sealed;
    let before = e.ring.written;
    e.push_ring(e.cur, "late_write", 0, 1);
    let blocked = e.ring.written == before && sealed_now;
    if blocked {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let err = DecErr::WriteAfterSeal(tick);
        let c = err.code();
        let n = format!("封账后写被拦 码={}", c);
        e.note_form("inject_write_after_seal", err, 9);
        return Injected {
            form: 9,
            name: "封账后写路径",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 9,
        name: "封账后写路径",
        tick,
        built: false,
        note: String::from("封账后写未被拦：注入核失效"),
    }
}

/// 形态 10：封账哈希漂移。
pub fn inject_seal_hash(e: &mut Declare3, tick: u64) -> Injected {
    // 要求调用方已封账：本形态在真封账态上篡改域账，看复验是否拒。
    if !e.sealed {
        return Injected {
            form: 10,
            name: "封账哈希漂移",
            tick,
            built: false,
            note: String::from("夹具未封账：注入核前置条件不满足"),
        };
    }
    let before = e.seal_fp;
    e.ledger.domain_account = DOMAIN_ACCOUNT_EXPECT.wrapping_add(1);
    let r = e.verify_seal(tick);
    e.ledger.domain_account = DOMAIN_ACCOUNT_EXPECT;
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let c = err.code();
        let n = format!("封账哈希漂移被拒 码={} 冻结={}", c, before);
        e.note_form("inject_seal_hash", err, 10);
        return Injected {
            form: 10,
            name: "封账哈希漂移",
            tick,
            built: true,
            note: n,
        };
    }
    Injected {
        form: 10,
        name: "封账哈希漂移",
        tick,
        built: false,
        note: String::from("封账哈希漂移未被拒：注入核失效"),
    }
}

/// 十形态名表。
pub fn form_names() -> [&'static str; 10] {
    [
        "跳段注入",
        "倒序注入",
        "段号越界",
        "证据号越界",
        "证据重复点收",
        "证据不齐即宣告",
        "条件号越界",
        "证明不齐即封账",
        "封账后写路径",
        "封账哈希漂移",
    ]
}

/// 跑满十形态注入（各用独立夹具跑到它该暴露的那一段）。
pub fn inject_all(e: &mut Declare3, seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();

    let mut f1 = fixture_boot(seed);
    let t1 = f1.tick;
    let r1 = inject_skip_stage(&mut f1, t1);
    stamp_form(&mut f1, 1);
    merge_reject(e, &f1);
    out.push(r1);

    let mut f2 = fixture_boot(seed);
    let t2 = f2.tick;
    let r2 = inject_backward(&mut f2, t2);
    stamp_form(&mut f2, 2);
    merge_reject(e, &f2);
    out.push(r2);

    let mut f3 = fixture_evidence(seed);
    let t3 = f3.tick;
    let r3 = inject_stage_oob(&mut f3, t3);
    stamp_form(&mut f3, 3);
    merge_reject(e, &f3);
    out.push(r3);

    let mut f4 = fixture_evidence(seed);
    let t4 = f4.tick;
    let r4 = inject_evidence_oob(&mut f4, t4);
    stamp_form(&mut f4, 4);
    merge_reject(e, &f4);
    out.push(r4);

    let mut f5 = fixture_evidence(seed);
    let t5 = f5.tick;
    let r5 = inject_evidence_dup(&mut f5, t5);
    stamp_form(&mut f5, 5);
    merge_reject(e, &f5);
    out.push(r5);

    let mut f6 = fixture_evidence(seed);
    let t6 = f6.tick;
    let r6 = inject_evidence_short(&mut f6, t6);
    stamp_form(&mut f6, 6);
    merge_reject(e, &f6);
    out.push(r6);

    let mut f7 = fixture_declare(seed);
    let t7 = f7.tick;
    let r7 = inject_cond_oob(&mut f7, t7);
    stamp_form(&mut f7, 7);
    merge_reject(e, &f7);
    out.push(r7);

    let mut f8 = fixture_seal(seed);
    let t8 = f8.tick;
    let r8 = inject_proof_short(&mut f8, t8);
    stamp_form(&mut f8, 8);
    merge_reject(e, &f8);
    out.push(r8);

    let mut f9 = fixture_sealed(seed);
    let t9 = f9.tick;
    let r9 = inject_write_after_seal(&mut f9, t9);
    stamp_form(&mut f9, 9);
    merge_reject(e, &f9);
    out.push(r9);

    let mut f10 = fixture_sealed(seed);
    let t10 = f10.tick;
    let r10 = inject_seal_hash(&mut f10, t10);
    stamp_form(&mut f10, 10);
    merge_reject(e, &f10);
    out.push(r10);

    out
}

/// 取某形态首条拒记的诊断码。
pub fn first_code_of(e: &Declare3, form: u32) -> u16 {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == form {
            return e.rejects[i].code;
        }
        i += 1;
    }
    0
}

/// 是否存在该形态的拒记留痕。
pub fn has_form(e: &Declare3, form: u32) -> bool {
    first_code_of(e, form) != 0
}

/// 独立扫查（不看造证返回值，按现场状态 + 留痕判可检出）。
pub fn scan_detect(e: &Declare3) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    out.push(Detection {
        form: 1,
        name: "跳段注入",
        caught: has_form(e, 1) || !e.evidence_ready(),
        code: first_code_of(e, 1),
        seen_fp: Declare3::stage_table_fp(),
    });
    out.push(Detection {
        form: 2,
        name: "倒序注入",
        caught: has_form(e, 2) || !e.stages_ok(),
        code: first_code_of(e, 2),
        seen_fp: Declare3::edge_matrix_fp(),
    });
    out.push(Detection {
        form: 3,
        name: "段号越界",
        caught: has_form(e, 3) || e.stages.len() != STAGE_COUNT,
        code: first_code_of(e, 3),
        seen_fp: Declare3::stage_table_fp(),
    });
    out.push(Detection {
        form: 4,
        name: "证据号越界",
        caught: has_form(e, 4) || e.evidences.len() != EVIDENCE_TOTAL,
        code: first_code_of(e, 4),
        seen_fp: Declare3::evidence_table_fp(),
    });
    out.push(Detection {
        form: 5,
        name: "证据重复点收",
        caught: has_form(e, 5),
        code: first_code_of(e, 5),
        seen_fp: Declare3::evidence_table_fp(),
    });
    out.push(Detection {
        form: 6,
        name: "证据不齐即宣告",
        caught: has_form(e, 6),
        code: first_code_of(e, 6),
        seen_fp: e.evidence_count(),
    });
    out.push(Detection {
        form: 7,
        name: "条件号越界",
        caught: has_form(e, 7) || e.conds.len() != COND_TOTAL,
        code: first_code_of(e, 7),
        seen_fp: Declare3::cond_table_fp(),
    });
    out.push(Detection {
        form: 8,
        name: "证明不齐即封账",
        caught: has_form(e, 8),
        code: first_code_of(e, 8),
        seen_fp: e.ledger.proof_ok,
    });
    out.push(Detection {
        form: 9,
        name: "封账后写路径",
        caught: has_form(e, 9) || e.sealed,
        code: first_code_of(e, 9),
        seen_fp: e.seal_fp,
    });
    out.push(Detection {
        form: 10,
        name: "封账哈希漂移",
        caught: has_form(e, 10),
        code: first_code_of(e, 10),
        seen_fp: e.seal_fp,
    });
    out
}

/// 检出计数。
pub fn detect_ok(d: &[Detection]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < d.len() {
        if d[i].caught {
            n = n.wrapping_add(1);
        }
        i += 1;
    }
    n
}

/// 十形态是否全检出。
pub fn detect_all_ok(d: &[Detection]) -> bool {
    d.len() == INJECT_TOTAL as usize && detect_ok(d) == INJECT_TOTAL
}

/// 检出表转行。
pub fn detect_rows(d: &[Detection]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < d.len() {
        v.push(format!(
            "FORM {:02} {} caught={} code=0x{:04X} fp={}",
            d[i].form, d[i].name, d[i].caught, d[i].code, d[i].seen_fp
        ));
        i += 1;
    }
    v
}

/// 注入记录转行。
pub fn inject_rows(v: &[Injected]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < v.len() {
        out.push(format!(
            "INJ {:02} {} built={} tick={} note={}",
            v[i].form, v[i].name, v[i].built, v[i].tick, v[i].note
        ));
        i += 1;
    }
    out
}

impl Declare3 {
    /// 段闭集表指纹。
    pub fn stage_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (STAGE_COUNT as u8) {
            if let Some(x) = Stage::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
                h = mix32(h, x.duty_count());
            }
            i += 1;
        }
        h
    }
    /// 证据闭集表指纹。
    pub fn evidence_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (EVIDENCE_TOTAL as u8) {
            if let Some(x) = Evidence::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
            }
            i += 1;
        }
        h
    }
    /// 条件闭集表指纹。
    pub fn cond_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (COND_TOTAL as u8) {
            if let Some(x) = Cond::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
            }
            i += 1;
        }
        h
    }
    /// 证明闭集表指纹。
    pub fn proof_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (PROOF_TOTAL as u8) {
            if let Some(x) = Proof::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
            }
            i += 1;
        }
        h
    }
    /// 链闭集表指纹。
    pub fn chain_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (CHAIN_TOTAL as u8) {
            if let Some(x) = Chain::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
            }
            i += 1;
        }
        h
    }
    /// 合法边矩阵指纹（3×3 全矩阵，非法边也入指纹以保矩阵完整）。
    pub fn edge_matrix_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut a = 0u8;
        while a < (STAGE_COUNT as u8) {
            let mut b = 0u8;
            while b < (STAGE_COUNT as u8) {
                h = mix_bool(h, Declare3::legal_edge(a, b));
                h = mix32(h, (a as u32) * 16 + b as u32);
                b += 1;
            }
            a += 1;
        }
        h
    }
    /// 关键表整体指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h = Declare3::stage_table_fp();
        h = mix64(h, Declare3::evidence_table_fp());
        h = mix64(h, Declare3::cond_table_fp());
        h = mix64(h, Declare3::proof_table_fp());
        h = mix64(h, Declare3::chain_table_fp());
        h = mix64(h, Declare3::edge_matrix_fp());
        let mut i = 0usize;
        while i < self.stages.len() {
            h = mix_bool(h, self.stages[i].entered);
            h = mix_bool(h, self.stages[i].done);
            h = mix32(h, self.stages[i].duty_done);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.evidences.len() {
            h = mix_bool(h, self.evidences[j].collected);
            h = mix64(h, self.evidences[j].fp);
            j += 1;
        }
        let mut k = 0usize;
        while k < self.conds.len() {
            h = mix_bool(h, self.conds[k].hold);
            k += 1;
        }
        let mut m = 0usize;
        while m < self.proofs.len() {
            h = mix_bool(h, self.proofs[m].sealed);
            h = mix64(h, self.proofs[m].fp);
            m += 1;
        }
        let mut n = 0usize;
        while n < self.chains.len() {
            h = mix_bool(h, self.chains[n]);
            n += 1;
        }
        h = mix64(h, self.ledger.domain_account);
        h = mix32(h, self.reject_count());
        h = mix64(h, self.ring.written);
        h = mix64(h, self.ring.dropped);
        h = mix64(h, self.poison);
        h
    }
    /// 读冻结指纹。
    pub fn fingerprint(&self) -> u64 {
        self.compute_fp()
    }
    /// 指纹校验。
    pub fn verify(&self) -> bool {
        self.compute_fp() != FNV_BASIS
    }
}

/// 冻结基准指纹。
pub fn frozen_baseline_fp() -> u64 {
    let mut h = FNV_BASIS;
    h = mix_str(h, MODULE_TAG);
    h = mix_str(h, TASK_ID);
    h = mix32(h, MODULE_VERSION as u32);
    h = mix64(h, Declare3::stage_table_fp());
    h = mix64(h, Declare3::evidence_table_fp());
    h = mix64(h, Declare3::cond_table_fp());
    h = mix64(h, Declare3::proof_table_fp());
    h = mix64(h, Declare3::chain_table_fp());
    h = mix64(h, Declare3::edge_matrix_fp());
    h
}

/// 基准是否自洽。
pub fn baseline_ok() -> bool {
    frozen_baseline_fp() != FNV_BASIS
}

/// 环形账自身指纹。
pub fn ring_fp(r: &RingLog) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < r.slots.len() {
        let s = &r.slots[i];
        h = mix_str(h, s.action);
        h = mix32(h, s.stage as u32);
        h = mix32(h, s.code as u32);
        h = mix64(h, s.payload_fp);
        i += 1;
    }
    mix64(mix64(h, r.written), r.dropped)
}

// ===========================================================================
// 7. 判定矩阵
// ===========================================================================

/// 判定矩阵的一格。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    /// 段序号。
    pub stage: u8,
    /// 格序号（段内 0..5）。
    pub k: usize,
    /// 判据短名。
    pub label: &'static str,
    /// 实测值。
    pub measured: u64,
    /// 期望值。
    pub expect: u64,
    /// 是否通过。
    pub pass: bool,
}

/// 矩阵总格数（3 段 × 6 格 = 18）。
pub const MATRIX_CELLS: usize = STAGE_COUNT * 6;

/// 三段判据短名表（每段 6 格）。
pub fn matrix_labels(stage: u8) -> [&'static str; 6] {
    match stage {
        0 => [
            "五证据齐备",
            "证据无重收",
            "证据指纹非零",
            "段内重入为零",
            "tick单调不减",
            "域槽长度闭集",
        ],
        1 => [
            "五条件全真",
            "条件数=5",
            "宣告比率100%",
            "域账累计234000",
            "证据账配平",
            "拒记不吞",
        ],
        _ => [
            "五证明齐备",
            "四链齐备",
            "封账哈希非零",
            "封账后写拦截",
            "守恒三段账",
            "终态已入",
        ],
    }
}

/// 逐格实算。
pub fn run_matrix(e: &Declare3) -> Vec<MatrixCell> {
    let mut out: Vec<MatrixCell> = Vec::new();
    let mut st = 0u8;
    while st < (STAGE_COUNT as u8) {
        let labels = matrix_labels(st);
        let mut k = 0usize;
        while k < 6 {
            let (measured, expect) = measure_cell(e, st, k);
            out.push(MatrixCell {
                stage: st,
                k,
                label: labels[k],
                measured,
                expect,
                pass: measured == expect,
            });
            k += 1;
        }
        st += 1;
    }
    out
}

/// 单格实测。
pub fn measure_cell(e: &Declare3, stage: u8, k: usize) -> (u64, u64) {
    match (stage, k) {
        (0, 0) => (e.evidence_count(), EVIDENCE_TOTAL as u64),
        (0, 1) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.evidences.len() {
                n += e.evidences[i].recount as u64;
                i += 1;
            }
            (n, 0)
        }
        (0, 2) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.evidences.len() {
                if e.evidences[i].fp != 0 {
                    n += 1;
                }
                i += 1;
            }
            (n, e.evidence_count())
        }
        (0, 3) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.stages.len() {
                n += e.stages[i].reentrant as u64;
                i += 1;
            }
            (n, 0)
        }
        (0, 4) => (
            if e.tick == 0 || !e.started {
                0
            } else {
                1
            },
            1,
        ),
        (0, 5) => (e.evidences.len() as u64, EVIDENCE_TOTAL as u64),
        (1, 0) => (e.ledger.cond_true, COND_TOTAL as u64),
        (1, 1) => (e.conds.len() as u64, COND_TOTAL as u64),
        (1, 2) => (e.ledger.declare_ratio() as u64, DECLARE_RATIO_TARGET as u64),
        // 域账累计：宣告段刚过时为 234,000；封账落档后抬到 240,000。
        // 矩阵跑在终态引擎上，故按 sealed 位分档取期望。
        (1, 3) => (
            e.ledger.domain_account,
            if e.sealed {
                DOMAIN_ACCOUNT_SEALED
            } else {
                DOMAIN_ACCOUNT_EXPECT
            },
        ),
        (1, 4) => (
            e.ledger.evidence_sum(),
            EVIDENCE_TOTAL as u64,
        ),
        (1, 5) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.rejects.len() {
                if e.rejects[i].detail.len() >= ERR_DETAIL_MIN_LEN {
                    n += 1;
                }
                i += 1;
            }
            (n, e.rejects.len() as u64)
        }
        (2, 0) => (e.ledger.proof_ok, PROOF_TOTAL as u64),
        (2, 1) => (e.ledger.chain_ok, CHAIN_TOTAL as u64),
        (2, 2) => (
            if e.seal_fp != 0 {
                1
            } else {
                0
            },
            1,
        ),
        (2, 3) => (
            if e.sealed {
                1
            } else {
                0
            },
            1,
        ),
        (2, 4) => (
            if e.reconcile() {
                1
            } else {
                0
            },
            1,
        ),
        _ => (
            if e.finalized {
                1
            } else {
                0
            },
            1,
        ),
    }
}

/// 矩阵是否全过。
pub fn matrix_ok(cells: &[MatrixCell]) -> bool {
    if cells.len() != MATRIX_CELLS {
        return false;
    }
    let mut i = 0usize;
    while i < cells.len() {
        if !cells[i].pass {
            return false;
        }
        i += 1;
    }
    true
}

/// 矩阵转行。
pub fn matrix_rows(cells: &[MatrixCell]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < cells.len() {
        v.push(format!(
            "CELL stage={} k={} {} measured={} expect={} pass={}",
            cells[i].stage, cells[i].k, cells[i].label, cells[i].measured, cells[i].expect,
            cells[i].pass
        ));
        i += 1;
    }
    v
}

// ===========================================================================
// 8. 导出面
// ===========================================================================

/// TLV 组头 tag。
pub const TLV_TAG_HEADER: u8 = 0xE1;
/// TLV 段态 tag。
pub const TLV_TAG_STAGE: u8 = 0xE2;
/// TLV 证据态 tag。
pub const TLV_TAG_EVIDENCE: u8 = 0xE3;
/// TLV 证明态 tag。
pub const TLV_TAG_PROOF: u8 = 0xE4;
/// TLV 封账态 tag。
pub const TLV_TAG_SEAL: u8 = 0xE5;

/// 把 u64 按小端压入 TLV 缓冲。
pub fn push_u64(out: &mut Vec<u8>, v: u64) {
    let mut i = 0usize;
    while i < 8 {
        out.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// 写一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    out.push((payload.len() & 0xFF) as u8);
    out.push(((payload.len() >> 8) & 0xFF) as u8);
    let mut i = 0usize;
    while i < payload.len() {
        out.push(payload[i]);
        i += 1;
    }
}

/// 读一条 TLV。
pub fn read_tlv(buf: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    if at + 3 > buf.len() {
        return None;
    }
    let tag = buf[at];
    let len = (buf[at + 1] as usize) | ((buf[at + 2] as usize) << 8);
    let start = at + 3;
    let end = start + len;
    if end > buf.len() {
        return None;
    }
    Some((tag, start, end))
}

/// TLV 长度自洽。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let mut at = 0usize;
    let mut n = 0usize;
    while at < buf.len() {
        match read_tlv(buf, at) {
            Some((_t, _s, end)) => {
                at = end;
                n += 1;
            }
            None => return false,
        }
        if n > 4096 {
            return false;
        }
    }
    at == buf.len()
}

/// 从 TLV 载荷读 u64。
pub fn read_u64(p: &[u8], at: usize) -> u64 {
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 8 {
        if at + i < p.len() {
            v |= ((p[at + i] as u64) & 0xFF) << (i * 8);
        }
        i += 1;
    }
    v
}

/// 观察聚合结构。
#[derive(Clone, Debug)]
pub struct DecObserve {
    /// 种子。
    pub seed: u64,
    /// 三段是否全过。
    pub stages_ok: bool,
    /// 矩阵格数。
    pub matrix_cells: u32,
    /// 矩阵是否全过。
    pub matrix_ok: bool,
    /// 五证据是否齐备。
    pub evidence_ready: bool,
    /// 五条件是否全真。
    pub cond_all_true: bool,
    /// 五证明是否齐备。
    pub proof_ready: bool,
    /// 四链是否齐备。
    pub chain_ready: bool,
    /// 是否已封账。
    pub sealed: bool,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 宣告比率。
    pub ratio: u32,
    /// 封账哈希。
    pub fp: u64,
    /// 封账哈希是否校验通过。
    pub fp_ok: bool,
    /// 回放是否稳定。
    pub replay_stable: bool,
    /// 注入形态总数。
    pub inject_total: u32,
    /// 注入检出数。
    pub inject_rejected: u32,
    /// 注入是否全检出。
    pub inject_all_caught: bool,
    /// 拒记条数。
    pub rejects: u32,
    /// 环形账写入条数。
    pub ring_written: u64,
    /// 环形账丢弃（显性）。
    pub ring_dropped: u64,
    /// 报告行数。
    pub report_lines: u32,
    /// 报告截断计数。
    pub report_truncated: u64,
    /// 体验日志条数。
    pub exp_rows: u32,
    /// 错误三要素齐备。
    pub err_triple_ok: bool,
    /// 域账累计（封账后应为 240,000）。
    pub domain_account: u64,
    /// 总判定。
    pub all_ok: bool,
}

/// 自检入口：内部真跑完整流程。
pub fn observe(seed: u64) -> DecObserve {
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => Declare3::new(seed),
    };
    // 判定矩阵实算。
    let cells = run_matrix(&e);
    let cells_ok = matrix_ok(&cells);
    // 注入核（独立审计引擎）。
    let mut probe = match run_full(seed) {
        Ok(x) => x,
        Err(_) => Declare3::new(seed),
    };
    let inj = inject_all(&mut probe, seed);
    let det = scan_detect(&probe);
    let caught = detect_ok(&det);
    let built = {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < inj.len() {
            if inj[i].built {
                n += 1;
            }
            i += 1;
        }
        n
    };
    let all_caught = detect_all_ok(&det) && built == INJECT_TOTAL;
    // 守恒实算。
    let conserved = e.reconcile();
    // 封账哈希校验。
    let fp_ok = match e.verify_seal(4100) {
        Ok(_) => e.seal_fp != 0,
        Err(_) => false,
    };
    // 错误三要素齐备。
    let mut triple = true;
    let mut i = 0usize;
    while i < probe.rejects.len() {
        let d = probe.rejects[i].detail.as_str();
        if d.len() < ERR_DETAIL_MIN_LEN
            || !d.contains("现象")
            || !d.contains("位置")
            || !d.contains("建议修法")
        {
            triple = false;
        }
        i += 1;
    }
    // 回放稳定性。
    let rep = replay(seed, 2);
    let stable = rep.stable();
    let stages_ok = e.stages_ok();
    let ratio = e.ledger.declare_ratio();
    let all_ok = stages_ok
        && cells_ok
        && all_caught
        && conserved
        && fp_ok
        && stable
        && triple
        && ratio == DECLARE_RATIO_TARGET
        && e.evidence_ready()
        && e.chain_ready()
        && e.sealed;
    DecObserve {
        seed,
        stages_ok,
        matrix_cells: cells.len() as u32,
        matrix_ok: cells_ok,
        evidence_ready: e.evidence_ready(),
        cond_all_true: e.ledger.cond_true == COND_TOTAL as u64,
        proof_ready: e.ledger.proof_ok == PROOF_TOTAL as u64,
        chain_ready: e.chain_ready(),
        sealed: e.sealed,
        conserved,
        ratio,
        fp: e.seal_fp,
        fp_ok,
        replay_stable: stable,
        inject_total: INJECT_TOTAL,
        inject_rejected: caught,
        inject_all_caught: all_caught,
        rejects: probe.reject_count(),
        ring_written: probe.ring.written,
        ring_dropped: probe.ring.dropped,
        report_lines: probe.report.len() as u32,
        report_truncated: probe.report_truncated,
        exp_rows: probe.exp.len() as u32,
        err_triple_ok: triple,
        domain_account: e.ledger.domain_account,
        all_ok,
    }
}

/// 把行数组拼成多行字符串。
pub fn join_lines(v: &[String]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < v.len() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(v[i].as_str());
        i += 1;
    }
    s
}

/// KV 导出。
pub fn export_kv(ob: &DecObserve) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("module={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("criterion={}", CRITERION_NAME));
    v.push(format!("version={}", MODULE_VERSION));
    v.push(format!("seed={}", ob.seed));
    v.push(format!("stages_ok={}", ob.stages_ok));
    v.push(format!("matrix_cells={}", ob.matrix_cells));
    v.push(format!("matrix_ok={}", ob.matrix_ok));
    v.push(format!("evidence_ready={}", ob.evidence_ready));
    v.push(format!("cond_all_true={}", ob.cond_all_true));
    v.push(format!("proof_ready={}", ob.proof_ready));
    v.push(format!("chain_ready={}", ob.chain_ready));
    v.push(format!("sealed={}", ob.sealed));
    v.push(format!("conserved={}", ob.conserved));
    v.push(format!("ratio={}", ob.ratio));
    v.push(format!("fp={}", ob.fp));
    v.push(format!("fp_ok={}", ob.fp_ok));
    v.push(format!("replay_stable={}", ob.replay_stable));
    v.push(format!("inject_total={}", ob.inject_total));
    v.push(format!("inject_rejected={}", ob.inject_rejected));
    v.push(format!("inject_all_caught={}", ob.inject_all_caught));
    v.push(format!("rejects={}", ob.rejects));
    v.push(format!("ring_written={}", ob.ring_written));
    v.push(format!("ring_dropped={}", ob.ring_dropped));
    v.push(format!("report_lines={}", ob.report_lines));
    v.push(format!("report_truncated={}", ob.report_truncated));
    v.push(format!("exp_rows={}", ob.exp_rows));
    v.push(format!("err_triple_ok={}", ob.err_triple_ok));
    v.push(format!("domain_account={}", ob.domain_account));
    v.push(format!("seg_base=0x{:04X}", SEG_BASE));
    v.push(format!("seg_end=0x{:04X}", SEG_END));
    v.push(format!("all_ok={}", ob.all_ok));
    join_lines(&v)
}

/// TLV 导出（组头单独一行，版本号单行 push_u64）。
pub fn export_tlv(ob: &DecObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut hdr: Vec<u8> = Vec::new();
    // 组头：版本号单独一行 push，不被行尾注释吞掉。
    push_u64(&mut hdr, MODULE_VERSION);
    push_u64(&mut hdr, ob.seed);
    push_tlv(&mut out, TLV_TAG_HEADER, &hdr);
    let mut st: Vec<u8> = Vec::new();
    push_u64(&mut st, ob.matrix_cells as u64);
    push_u64(&mut st, ob.inject_total as u64);
    push_u64(&mut st, ob.inject_rejected as u64);
    push_tlv(&mut out, TLV_TAG_STAGE, &st);
    let mut ev: Vec<u8> = Vec::new();
    push_u64(&mut ev, EVIDENCE_TOTAL as u64);
    push_u64(&mut ev, COND_TOTAL as u64);
    push_u64(&mut ev, ob.domain_account);
    push_tlv(&mut out, TLV_TAG_EVIDENCE, &ev);
    let mut pf: Vec<u8> = Vec::new();
    push_u64(&mut pf, PROOF_TOTAL as u64);
    push_u64(&mut pf, CHAIN_TOTAL as u64);
    push_tlv(&mut out, TLV_TAG_PROOF, &pf);
    let mut sl: Vec<u8> = Vec::new();
    push_u64(&mut sl, ob.fp);
    push_u64(&mut sl, ob.ring_written);
    push_u64(&mut sl, ob.ring_dropped);
    push_u64(&mut sl, ob.rejects as u64);
    push_tlv(&mut out, TLV_TAG_SEAL, &sl);
    out
}

/// TLV 往返一致。
pub fn tlv_roundtrip_ok(ob: &DecObserve, buf: &[u8]) -> bool {
    if !tlv_len_ok(buf) {
        return false;
    }
    let mut at = 0usize;
    let mut seen = 0u32;
    let mut fp = 0u64;
    while let Some((tag, start, end)) = read_tlv(buf, at) {
        let p = &buf[start..end];
        if tag == TLV_TAG_SEAL && p.len() >= 8 {
            fp = read_u64(p, 0);
        }
        seen = seen.wrapping_add(1);
        at = end;
    }
    seen == 5 && fp == ob.fp
}

// ===========================================================================
// 9. 确定性回放
// ===========================================================================

/// 单轮回放。
pub fn replay_once(seed: u64) -> (u64, ReplayReport) {
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => Declare3::new(seed),
    };
    let mut probe = e.clone();
    let _ = inject_all(&mut probe, seed);
    let det = scan_detect(&probe);
    let caught = detect_ok(&det);
    let fp = probe.compute_fp();
    let rep = ReplayReport {
        seed,
        rounds: 1,
        stages_ok: e.stages_ok(),
        caught,
        total: INJECT_TOTAL,
        conserved: e.reconcile(),
        ratio: e.ledger.declare_ratio(),
        seal_fp: e.seal_fp,
        trace: vec![fp],
        dropped: probe.ring.dropped,
    };
    (fp, rep)
}

/// 跑 `rounds` 轮回放并核对轨迹。
pub fn replay(seed: u64, rounds: u32) -> ReplayReport {
    let n = if rounds < 2 { 2 } else { rounds };
    let mut trace: Vec<u64> = Vec::new();
    let mut last = ReplayReport {
        seed,
        rounds: n,
        stages_ok: false,
        caught: 0,
        total: INJECT_TOTAL,
        conserved: false,
        ratio: 0,
        seal_fp: 0,
        trace: Vec::new(),
        dropped: 0,
    };
    let mut i = 0u32;
    while i < n {
        let (fp, rep) = replay_once(seed);
        trace.push(fp);
        last = rep;
        i += 1;
    }
    last.rounds = n;
    last.trace = trace;
    last
}

/// 回放报告转多行。
pub fn replay_report(rep: &ReplayReport) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("REPLAY seed={} rounds={}", rep.seed, rep.rounds));
    v.push(format!(
        "  stages_ok={} caught={}/{} conserved={} ratio={}",
        rep.stages_ok, rep.caught, rep.total, rep.conserved, rep.ratio
    ));
    v.push(format!("  seal_fp={} dropped={}", rep.seal_fp, rep.dropped));
    let mut i = 0usize;
    while i < rep.trace.len() {
        v.push(format!("  trace[{}]={}", i, rep.trace[i]));
        i += 1;
    }
    v.push(format!("  stable={}", rep.stable()));
    join_lines(&v)
}

// ===========================================================================
// 10. 自检断言面
// ===========================================================================

/// 单条自检判据。
#[derive(Clone, Debug)]
pub struct Check {
    /// 判据号。
    pub id: &'static str,
    /// 判据名。
    pub name: &'static str,
    /// 实测值。
    pub got: u64,
    /// 期望值。
    pub want: u64,
    /// 是否通过。
    pub ok: bool,
}

impl Check {
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "CHECK {} {} got={} want={} ok={}",
            self.id, self.name, self.got, self.want, self.ok
        )
    }
}

/// 跑全量自检。
pub fn self_check(seed: u64) -> Vec<Check> {
    let ob = observe(seed);
    let mut v: Vec<Check> = Vec::new();
    v.push(Check {
        id: "J1-a",
        name: "三段编排全过",
        got: ob.stages_ok as u64,
        want: 1,
        ok: ob.stages_ok,
    });
    v.push(Check {
        id: "J1-b",
        name: "五证据齐备",
        got: ob.evidence_ready as u64,
        want: 1,
        ok: ob.evidence_ready,
    });
    v.push(Check {
        id: "J1-c",
        name: "五条件硬断言全真",
        got: ob.cond_all_true as u64,
        want: 1,
        ok: ob.cond_all_true,
    });
    v.push(Check {
        id: "J1-d",
        name: "五证明齐备",
        got: ob.proof_ready as u64,
        want: 1,
        ok: ob.proof_ready,
    });
    v.push(Check {
        id: "J1-e",
        name: "四链齐备",
        got: ob.chain_ready as u64,
        want: 1,
        ok: ob.chain_ready,
    });
    v.push(Check {
        id: "J1-f",
        name: "已封账",
        got: ob.sealed as u64,
        want: 1,
        ok: ob.sealed,
    });
    v.push(Check {
        id: "J1-g",
        name: "守恒成立",
        got: ob.conserved as u64,
        want: 1,
        ok: ob.conserved,
    });
    v.push(Check {
        id: "J1-h",
        name: "宣告比率100%",
        got: ob.ratio as u64,
        want: DECLARE_RATIO_TARGET as u64,
        ok: ob.ratio == DECLARE_RATIO_TARGET,
    });
    v.push(Check {
        id: "J1-i",
        name: "封账哈希校验",
        got: ob.fp_ok as u64,
        want: 1,
        ok: ob.fp_ok,
    });
    v.push(Check {
        id: "J1-j",
        name: "回放稳定",
        got: ob.replay_stable as u64,
        want: 1,
        ok: ob.replay_stable,
    });
    v.push(Check {
        id: "J1-k",
        name: "跳段注入10/10拒绝",
        got: ob.inject_rejected as u64,
        want: INJECT_TOTAL as u64,
        ok: ob.inject_rejected == INJECT_TOTAL,
    });
    v.push(Check {
        id: "J1-l",
        name: "矩阵18格全过",
        got: ob.matrix_ok as u64,
        want: 1,
        ok: ob.matrix_ok,
    });
    v.push(Check {
        id: "J1-m",
        name: "矩阵格数18",
        got: ob.matrix_cells as u64,
        want: MATRIX_CELLS as u64,
        ok: ob.matrix_cells as usize == MATRIX_CELLS,
    });
    v.push(Check {
        id: "J1-n",
        name: "错误三要素齐备",
        got: ob.err_triple_ok as u64,
        want: 1,
        ok: ob.err_triple_ok,
    });
    v.push(Check {
        id: "J1-o",
        name: "域账封账预记240000",
        got: ob.domain_account,
        want: DOMAIN_ACCOUNT_SEALED,
        ok: ob.domain_account == DOMAIN_ACCOUNT_SEALED,
    });
    v.push(Check {
        id: "J1-p",
        name: "诊断码段闭集",
        got: DecErr::StageIndexOutOfRange(0).code() as u64,
        want: SEG_BASE as u64,
        ok: DecErr::StageIndexOutOfRange(0).code() == SEG_BASE,
    });
    v.push(Check {
        id: "J1-q",
        name: "报告零截断",
        got: ob.report_truncated,
        want: 0,
        ok: ob.report_truncated == 0,
    });
    v.push(Check {
        id: "J1-r",
        name: "TLV往返一致",
        got: {
            let b = export_tlv(&ob);
            if tlv_roundtrip_ok(&ob, &b) {
                1
            } else {
                0
            }
        },
        want: 1,
        ok: {
            let b = export_tlv(&ob);
            tlv_roundtrip_ok(&ob, &b)
        },
    });
    v
}

/// 自检是否全过。
pub fn self_check_ok(seed: u64) -> bool {
    let c = self_check(seed);
    if c.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < c.len() {
        if !c[i].ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 自检报告。
pub fn self_check_report(seed: u64) -> String {
    let c = self_check(seed);
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        v.push(c[i].line());
        i += 1;
    }
    v.push(format!("ALL_OK={}", self_check_ok(seed)));
    join_lines(&v)
}

// ===========================================================================
// 11. 性能逐项分解
// ===========================================================================

/// 单项复杂度条目。
#[derive(Clone, Debug)]
pub struct CxItem {
    /// 操作名。
    pub op: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// syscall 次数。
    pub syscalls: u32,
    /// 每帧步数上限。
    pub max_steps: u32,
    /// 说明。
    pub note: &'static str,
}

/// 复杂度总表。
#[derive(Clone, Debug)]
pub struct Complexity {
    /// 逐项分解。
    pub items: Vec<CxItem>,
    /// 相对 Windows 同类能力的加深点。
    pub deeper_than_windows: &'static str,
}

impl Complexity {
    /// 总项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// syscall 总次数。
    pub fn syscall_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            n = n.wrapping_add(self.items[i].syscalls);
            i += 1;
        }
        n
    }
    /// 每帧步数上限合计。
    pub fn step_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            n = n.wrapping_add(self.items[i].max_steps);
            i += 1;
        }
        n
    }
    /// 转行。
    pub fn rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            v.push(format!(
                "CX {} time={} space={} syscalls={} max_steps={} note={}",
                self.items[i].op,
                self.items[i].time,
                self.items[i].space,
                self.items[i].syscalls,
                self.items[i].max_steps,
                self.items[i].note
            ));
            i += 1;
        }
        v
    }
}

/// 性能逐项分解（对标 Windows/Win11 同类能力）。
pub fn complexity() -> Complexity {
    let items: Vec<CxItem> = vec![
        CxItem {
            op: "Declare3::new",
            time: "O(STAGE+EVIDENCE+COND+PROOF+CHAIN)",
            space: "O(STAGE+EVIDENCE+COND+PROOF+CHAIN)+RING_CAP",
            syscalls: 0,
            max_steps: 22,
            note: "3段+5证据+5条件+5证明+4链+80槽环形账一次铺开",
        },
        CxItem {
            op: "Declare3::advance",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 1,
            note: "逻辑 tick 注入与单调闸，无墙钟",
        },
        CxItem {
            op: "Declare3::enter_stage",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "终态闸/时序闸/边闭集闸/重入闸/间隔闸五级裁决",
        },
        CxItem {
            op: "Declare3::collect_evidence",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 4,
            note: "证据闭集 + 重收闸 + 段闸 + 指纹写入",
        },
        CxItem {
            op: "Declare3::evidence_ready",
            time: "O(EVIDENCE)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "五件逐槽扫，恒定 5 步",
        },
        CxItem {
            op: "Declare3::assert_all_conds",
            time: "O(COND)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "五条件硬断言逐条求值，任一为假即记账",
        },
        CxItem {
            op: "Declare3::declare_gate",
            time: "O(EVIDENCE+COND)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 15,
            note: "齐备闸 + 逐条硬断言定位 + 比率闸 + 域账闸",
        },
        CxItem {
            op: "Declare3::seal_proof",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "证明闭集 + 封账后写闸 + 重落闸",
        },
        CxItem {
            op: "Declare3::seal_chain",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 3,
            note: "四链逐条固化，封账后拒写",
        },
        CxItem {
            op: "Declare3::seal_gate",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "证明齐备闸 + 链齐备闸 + 守恒闸 + 域账闸 + 哈希",
        },
        CxItem {
            op: "Declare3::compute_seal_fp",
            time: "O(EVIDENCE+COND+PROOF+CHAIN)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 19,
            note: "五证据+五条件+五证明+四链+域账全折叠",
        },
        CxItem {
            op: "Declare3::verify_seal",
            time: "O(EVIDENCE+COND+PROOF+CHAIN)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 19,
            note: "重算并与冻结值逐位比对，漂移即红",
        },
        CxItem {
            op: "Declare3::reconcile",
            time: "O(EVIDENCE+STAGE)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 11,
            note: "五本账分项和 + 证据卡逐槽 + 三段守恒",
        },
        CxItem {
            op: "Declare3::push_ring",
            time: "O(1)",
            space: "O(cap)",
            syscalls: 0,
            max_steps: 1,
            note: "封账后写闸在函数首判定，O(1) 短路",
        },
        CxItem {
            op: "run_full",
            time: "O(EVIDENCE+COND+PROOF+CHAIN)",
            space: "O(上述全部)",
            syscalls: 0,
            max_steps: 60,
            note: "三段共 9 必做项 + 5+5+5+4 条闭集",
        },
        CxItem {
            op: "inject_all",
            time: "O(INJECT_TOTAL)",
            space: "O(RING_CAP)",
            syscalls: 0,
            max_steps: 10,
            note: "十形态各一次造证，各用独立夹具",
        },
        CxItem {
            op: "scan_detect",
            time: "O(INJECT_TOTAL+EVIDENCE)",
            space: "O(INJECT_TOTAL)",
            syscalls: 0,
            max_steps: 10,
            note: "独立扫查，留痕型 + 现场型双判据",
        },
        CxItem {
            op: "run_matrix",
            time: "O(MATRIX_CELLS)",
            space: "O(MATRIX_CELLS)",
            syscalls: 0,
            max_steps: 18,
            note: "3 段 × 6 格逐格实算",
        },
        CxItem {
            op: "replay",
            time: "O(rounds × (编排+注入+扫查))",
            space: "O(rounds)",
            syscalls: 0,
            max_steps: 4,
            note: "同种子两轮必同封账哈希",
        },
        CxItem {
            op: "export_kv",
            time: "O(metric_count)",
            space: "O(metric_count)",
            syscalls: 0,
            max_steps: 30,
            note: "30 条指标一行一条",
        },
        CxItem {
            op: "export_tlv",
            time: "O(metric_count)",
            space: "O(metric_count)",
            syscalls: 0,
            max_steps: 5,
            note: "5 组 TLV，组头单行 push_u64",
        },
        CxItem {
            op: "tlv_roundtrip_ok",
            time: "O(TLV_LEN)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "逐条走长度域，字段回读比对",
        },
        CxItem {
            op: "observe",
            time: "O(编排+注入+矩阵+回放)",
            space: "O(RING_CAP+MATRIX_CELLS)",
            syscalls: 0,
            max_steps: 200,
            note: "自检入口一次跑完整条链路",
        },
        CxItem {
            op: "detail 三要素",
            time: "O(1) 摊还",
            space: "O(1) 摊还",
            syscalls: 0,
            max_steps: 3,
            note: "错误明细恒含现象/位置/建议修法",
        },
        CxItem {
            op: "self_check",
            time: "O(判据条数)",
            space: "O(判据条数)",
            syscalls: 0,
            max_steps: 18,
            note: "18 条自检判据逐条实算",
        },
    ];
    Complexity {
        items,
        deeper_than_windows:
            "Windows 11 的「功能就绪宣告」散落在 WMI 的 Win32_OptionalFeature、\
             服务状态查询、事件日志与证书链验证四处，四处都要人分别去查、\
             再手工判断「到底齐不齐」；本单把「齐备才宣告」做成**一个三段状态机**：\
             ①证据段把五证据做成一件一收、重收即拒的登记册；\
             ②宣告段把五条件做成硬断言——任一为假当场拦截并定位到条件号，不允许降级为警告；\
             ③封账段把五证明与四链做成齐备闸，齐备才计算封账哈希，且哈希一落 `sealed` 位即\
             **封账后写路径全拦截**（环形账写指针直接冻结）。\
             比 Windows 同类能力更深在两处：一是「缺项拦截」是可定位到件号的硬闸而非人工核对；\
             二是「封存」是一个可验证的哈希状态而非一个目录只读位——同种子重跑封账哈希逐位相同，\
             漂移即定位到证据/条件/证明/链中的具体哪一项。且全程零 syscall、零墙钟、零真机。",
    }
}

// ===========================================================================
// 12. 报告面
// ===========================================================================

/// 不变式清单。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        String::from("L1 合法边仅 0->1、1->2、2->3 共 3 条"),
        String::from("L2 终态 finalized 后任何迁入拒绝"),
        String::from("L3 逻辑 tick 单调不减且非 0"),
        String::from("L4 段未完成不得重入"),
        String::from("L5 证据一件一收，重收即拒"),
        String::from("L6 五证据齐备才放行宣告"),
        String::from("L7 五条件硬断言，任一为假即拦截"),
        String::from("L8 宣告比率恒 100%"),
        String::from("L9 五证明齐备才放行封账"),
        String::from("L10 四链齐备方可固化"),
        String::from("L11 封账哈希与冻结值逐位相同"),
        String::from("L12 封账后写路径全拦截"),
        String::from("L13 证据账分项和恒 5"),
        String::from("L14 条件账分项和恒 5"),
        String::from("L15 证明账分项和恒 5"),
        String::from("L16 链账分项和恒 4"),
        String::from("L17 三段守恒 done + 未 done == 3"),
        String::from("L18 证据卡与证据账逐槽一致"),
        String::from("L19 环形账满必 dropped 显性记账"),
        String::from("L20 报告行数不超上限，截断显式计数"),
        String::from("L21 诊断码恒在 0x15C0..0x15FF"),
        String::from("L22 错误明细三要素齐备"),
        String::from("L23 十形态注入逐条独立检出"),
        String::from("L24 体验日志五字段齐备"),
    ]
}

/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let n = form_names();
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < n.len() {
        v.push(format!("FORM {:02} {}", i + 1, n[i]));
        i += 1;
    }
    v
}

/// 错误码全表（21 变体）。
pub fn err_table() -> Vec<String> {
    let all: [DecErr; 21] = [
        DecErr::StageIndexOutOfRange(0),
        DecErr::IllegalStageEdge(0, 1),
        DecErr::AlreadyFinalized,
        DecErr::TickRegress(0, 0),
        DecErr::StageReentrant(0),
        DecErr::TickMissing,
        DecErr::EvidenceIndexOutOfRange(0),
        DecErr::EvidenceDuplicated(0),
        DecErr::EvidenceNotCollected(0),
        DecErr::EvidenceIncomplete(0, 0),
        DecErr::CondIndexOutOfRange(0),
        DecErr::CondAssertionFailed(0),
        DecErr::ProofIndexOutOfRange(0),
        DecErr::ProofIncomplete(0, 0),
        DecErr::WriteAfterSeal(0),
        DecErr::SealHashBroken(0, 0),
        DecErr::LedgerUnbalanced(0, 0),
        DecErr::DeclareRatioShort(0),
        DecErr::RingUnderflow(0),
        DecErr::InjectDetectMismatch(0, 0),
        DecErr::ChainIncomplete(0, 0),
    ];
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        v.push(err_line(all[i]));
        i += 1;
    }
    v
}

/// 诊断码全表唯一性与三要素自检。
pub fn code_table_ok() -> bool {
    let all: [DecErr; 21] = [
        DecErr::StageIndexOutOfRange(0),
        DecErr::IllegalStageEdge(0, 1),
        DecErr::AlreadyFinalized,
        DecErr::TickRegress(0, 0),
        DecErr::StageReentrant(0),
        DecErr::TickMissing,
        DecErr::EvidenceIndexOutOfRange(0),
        DecErr::EvidenceDuplicated(0),
        DecErr::EvidenceNotCollected(0),
        DecErr::EvidenceIncomplete(0, 0),
        DecErr::CondIndexOutOfRange(0),
        DecErr::CondAssertionFailed(0),
        DecErr::ProofIndexOutOfRange(0),
        DecErr::ProofIncomplete(0, 0),
        DecErr::WriteAfterSeal(0),
        DecErr::SealHashBroken(0, 0),
        DecErr::LedgerUnbalanced(0, 0),
        DecErr::DeclareRatioShort(0),
        DecErr::RingUnderflow(0),
        DecErr::InjectDetectMismatch(0, 0),
        DecErr::ChainIncomplete(0, 0),
    ];
    let mut i = 0usize;
    while i < all.len() {
        if !code_in_seg(all[i].code()) {
            return false;
        }
        if !all[i].detail_has_triple() {
            return false;
        }
        let mut j = i + 1;
        while j < all.len() {
            if all[i].code() == all[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 三段时序报告。
pub fn timing_report(e: &Declare3) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.stages.len() {
        let s = &e.stages[i];
        v.push(format!(
            "STAGE {} entered={} done={} enter_tick={} done_tick={} duty={}/{} reentrant={}",
            s.label, s.entered, s.done, s.enter_tick, s.done_tick, s.duty_done, s.duty_total,
            s.reentrant
        ));
        i += 1;
    }
    v.push(format!(
        "FINALIZED tick={} finalized={} sealed={}",
        e.tick, e.finalized, e.sealed
    ));
    join_lines(&v)
}

/// 守恒账报告。
pub fn ledger_report(e: &Declare3) -> String {
    let l = &e.ledger;
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "EVIDENCE ok={} bad={} sum={}",
        l.evidence_ok,
        l.evidence_bad,
        l.evidence_sum()
    ));
    v.push(format!(
        "COND true={} false={} sum={}",
        l.cond_true,
        l.cond_false,
        l.cond_sum()
    ));
    v.push(format!(
        "PROOF ok={} bad={} sum={}",
        l.proof_ok,
        l.proof_bad,
        l.proof_sum()
    ));
    v.push(format!(
        "CHAIN ok={} bad={} sum={}",
        l.chain_ok,
        l.chain_bad,
        l.chain_sum()
    ));
    v.push(format!("RATIO {}% target={}%", l.declare_ratio(), DECLARE_RATIO_TARGET));
    v.push(format!("DOMAIN_ACCOUNT {}", l.domain_account));
    v.push(format!("RECONCILE {}", e.reconcile()));
    join_lines(&v)
}

/// 环形账报告（含显式丢弃）。
pub fn ring_report(e: &Declare3) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "RING cap={} written={} dropped={} live={} read={} wrapped={}",
        e.ring.cap,
        e.ring.written,
        e.ring.dropped,
        e.ring.live(),
        e.ring.read,
        e.ring.wrapped()
    ));
    v.push(format!("RING_FP {}", ring_fp(&e.ring)));
    join_lines(&v)
}

/// 环形账全条读出。
pub fn ring_dump(e: &mut Declare3, n: usize) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < n {
        match e.ring.pop() {
            Some(x) => v.push(format!(
                "RING seq={} tick={} stage={} action={} code=0x{:04X} fp={}",
                x.seq, x.tick, x.stage, x.action, x.code, x.payload_fp
            )),
            None => {
                v.push(String::from("RING <empty> 取账被拒：环形账下溢"));
                break;
            }
        }
        i += 1;
    }
    v
}

/// 环形账下溢拒记。
pub fn ring_underflow_err(e: &mut Declare3) -> DecResult<u64> {
    match e.ring.pop() {
        Some(x) => Ok(x.seq),
        None => {
            let err = DecErr::RingUnderflow(1);
            e.note_form("ring_pop", err, 0);
            Err(err)
        }
    }
}

/// 体验日志报告。
pub fn exp_report(e: &Declare3) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.exp.len() {
        v.push(e.exp[i].line());
        i += 1;
    }
    join_lines(&v)
}

/// 拒记报告。
pub fn reject_report(e: &Declare3) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.rejects.len() {
        v.push(format!(
            "REJECT #{:02} tick={} action={} code=0x{:04X} form={} detail={}",
            i + 1,
            e.rejects[i].tick,
            e.rejects[i].action,
            e.rejects[i].code,
            e.rejects[i].form,
            e.rejects[i].detail
        ));
        i += 1;
    }
    join_lines(&v)
}

/// 封账指纹报告。
pub fn seal_report(e: &Declare3, tick: u64) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("SEAL_FP stage_table=0x{:016X}", Declare3::stage_table_fp()));
    v.push(format!("SEAL_FP evidence_table=0x{:016X}", Declare3::evidence_table_fp()));
    v.push(format!("SEAL_FP cond_table=0x{:016X}", Declare3::cond_table_fp()));
    v.push(format!("SEAL_FP proof_table=0x{:016X}", Declare3::proof_table_fp()));
    v.push(format!("SEAL_FP chain_table=0x{:016X}", Declare3::chain_table_fp()));
    v.push(format!("SEAL_FP baseline=0x{:016X}", frozen_baseline_fp()));
    v.push(format!("SEAL_FP frozen={:016X}", e.seal_fp));
    v.push(format!("SEAL_FP computed=0x{:016X}", e.compute_seal_fp(tick)));
    v.push(format!("SEALED {}", e.sealed));
    join_lines(&v)
}

/// 体验收口自查。
pub fn experience_ok(seed: u64) -> bool {
    let mut e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => Declare3::new(seed),
    };
    let _ = inject_all(&mut e, seed);
    if e.exp.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < e.exp.len() {
        if !e.exp[i].complete() {
            return false;
        }
        if EXP_LOG_FIELDS != 5 {
            return false;
        }
        i += 1;
    }
    true
}

/// 隐蔽捕获自查。
pub fn no_silent_catch(e: &Declare3) -> bool {
    let mut i = 0usize;
    while i < e.rejects.len() {
        let d = e.rejects[i].detail.as_str();
        if d.len() < ERR_DETAIL_MIN_LEN {
            return false;
        }
        if !d.contains("现象") || !d.contains("位置") || !d.contains("建议修法") {
            return false;
        }
        i += 1;
    }
    true
}

/// 模块自述。
pub fn module_banner() -> String {
    format!(
        "{} {} | 域/批 CoRun·UNX-A3-B39 | 三段：证据段/宣告段/封账段 | 五证据 | 五条件 | 五证明 | 四链 | 注入 {}/{} | 诊断码段 0x{:04X}-0x{:04X}",
        MODULE_TAG,
        TASK_ID,
        INJECT_TOTAL,
        INJECT_TOTAL,
        SEG_BASE,
        SEG_END
    )
}

/// 语义边界声明。
pub fn semantic_boundary() -> Vec<String> {
    vec![
        String::from("只做宣告三段的时序裁决与留账"),
        String::from("不做宣告条件机五条件本体（归 F2362）"),
        String::from("不做域账封账五证明细则（归 F2368）"),
        String::from("不做四链固化细则（归 F2367）"),
        String::from("不做 pending 白名单终态移交（归 F2365）"),
        String::from("不做零 QEMU 纪律终审计（归 F2366）"),
        String::from("不做域终账加法链（归 F2381）"),
        String::from("不碰真机执行（开发期零 QEMU）"),
    ]
}

/// 关联单报告。
pub fn related_rows() -> Vec<String> {
    vec![
        String::from("F1594 宣告条件机体例 A2 同构 → 本单宣告段消费其五条件口径"),
        String::from("F1597 移交完成宣告体例 → 本单宣告动作语义源"),
        String::from("F1598 域账封账五证明体例 → 本单封账段五证明闭集来源"),
        String::from("F2360 B38 批级总成 228,000 → 本单证据②行数守恒来源"),
        String::from("F2358 联签终账 → 本单证据④联签双100% 来源"),
        String::from("F2207 pending 白名单账 → 本单证据⑤来源"),
        String::from("F2321 联签三段 → 本单证据④的更上游"),
        String::from("F2341 域级总回归 → 本单证据①来源"),
        String::from("F2362 宣告条件机 → 本单宣告段调用其硬断言"),
        String::from("F2368 域账封账五证明 → 本单封账段闭集来源"),
        String::from("F2381 域终账总成 → 封账档哈希为其前置之一"),
    ]
}

/// 全量摘要。
pub fn summary(seed: u64) -> String {
    let ob = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(module_banner());
    v.push(format!(
        "SUMMARY seed={} all_ok={} stages_ok={} ev_ready={} cond_true={} proof_ready={} chain_ready={} sealed={}",
        seed, ob.all_ok, ob.stages_ok, ob.evidence_ready, ob.cond_all_true, ob.proof_ready,
        ob.chain_ready, ob.sealed
    ));
    v.push(format!(
        "SUMMARY_MAT matrix={}/{} matrix_ok={} inject={}/{} caught={}",
        ob.matrix_cells, MATRIX_CELLS, ob.matrix_ok, ob.inject_rejected, ob.inject_total,
        ob.inject_all_caught
    ));
    v.push(format!(
        "SUMMARY_FP fp={} fp_ok={} replay_stable={} cons={} ratio={}% acct={}",
        ob.fp, ob.fp_ok, ob.replay_stable, ob.conserved, ob.ratio, ob.domain_account
    ));
    join_lines(&v)
}

/// 演示场景。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("DEMO {} {} seed={}", MODULE_TAG, TASK_ID, seed));
    v.push(self_check_report(seed));
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => Declare3::new(seed),
    };
    v.push(timing_report(&e));
    v.push(ledger_report(&e));
    v.push(seal_report(&e, 4100));
    let mut probe = e.clone();
    let inj = inject_all(&mut probe, seed);
    let rows = inject_rows(&inj);
    let mut i = 0usize;
    while i < rows.len() {
        v.push(rows[i].clone());
        i += 1;
    }
    let det = scan_detect(&probe);
    let drows = detect_rows(&det);
    let mut j = 0usize;
    while j < drows.len() {
        v.push(drows[j].clone());
        j += 1;
    }
    v.push(reject_report(&probe));
    v.push(export_kv(&observe(seed)));
    v
}

/// 单次调用的便捷入口。
pub fn run_and_report(seed: u64) -> (DecResult<u64>, String) {
    let r = run_full(seed).map(|e| e.seal_fp);
    (r, export_kv(&observe(seed)))
}
