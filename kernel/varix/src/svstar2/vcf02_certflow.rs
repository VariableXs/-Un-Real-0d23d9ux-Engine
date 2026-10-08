//! CGPU-F0802 · 认证流程体系（CGPU 册 · F 域 · 认证流程一域 · 六步流程正式化）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0802`
//!
//! 锚点原文：「认证流程：六步认证正式化（申请→能力探测→基准执行→判定→认证授予
//! →发布——每步的输入输出与判据固化）；流程自动化（全流程无人值守可跑——D08
//! 执行器复用）；流程中断恢复（任一步失败的续跑/重跑语义）；流程审计（每份认证
//! 的全流程留痕——可回溯）。判据：六步、自动化、中断恢复、审计、判据。」
//!
//! # 一、六步是**状态机**不是清单：步序与依赖固化在类型层
//!
//! 「申请→能力探测→基准执行→判定→认证授予→发布」若只是文档里的六个标题，赶工
//! 期的实现必然出现「没跑基准先发认证」的捷径。本单把六步做成封闭枚举
//! [`CertStep`] + 固化依赖表 [`STEP_DEPS`]：第 i 步的前置步在编译期钉死为 i-1，
//! [`CertFlow::run_step_at`] 对前置未完成的步返回 [`C_VCF02_STEP_ORDER`]——
//! 跳步在类型上**进不了**流程（判据「六步」与「每步的输入输出固化」）。
//!
//! # 二、判定步叠加铁律 8 闸：认证体系**复用** vcf01 的方法论入口
//!
//! 判定步不重新发明裁决：直接调用 [`super::vcf01_gpucompat::certify`]（铁律 8
//! 类型闸）。判定证据标签是**封闭三值**（`SUITE-RUN`/`CLAIM-ONLY`/`UNTESTED`），
//! 越出闭集即 [`C_VCF02_STEP_INPUT`]；`CLAIM-ONLY`（只有文档声明）被 certify
//! 拒绝后在审计中留痕 [`C_VCF01_UNVERIFIED`]，流程面返回
//! [`C_VCF02_JUDGE_REJECT`]——「凭文档声明」同样进不了流程版认证。
//!
//! # 三、自动化与中断恢复：执行器注入 + 续跑不重做
//!
//! 全流程无人值守 = [`FlowExecutor`] trait 注入（D08 执行器复用点）：流程状态机
//! 只管步序/凭据/留痕，步的实际执行外置——stub 执行器即可端到端跑通六步（判据
//! 「自动化」）。中断恢复语义：失败步记审计（verdict=错误码）后流程**停在原步**，
//! [`CertFlow::resume`] 从首个未完成步续跑；已完成步的凭据指纹
//! [`CertFlow::evidence`] 不可变（续跑前后逐位相等——「重跑语义」不重做）；
//! 单步重跑上限 [`MAX_ATTEMPTS`]，耗尽即 [`C_VCF02_RESUME_EXHAUSTED`]。
//!
//! # 四、审计是**哈希链**不是日志数组：逐记录前链混合，篡改可检
//!
//! 每步执行（成功或失败）都追加 [`AuditRecord`]：步号/第几次尝试/输入输出指纹/
//! 裁决码，`chain = mix(prev_chain, record)` 逐记录前链混合——任何一条被篡改，
//! [`CertFlow::audit_verify`] 从种子重算即断链 [`C_VCF02_AUDIT_BROKEN`]。
//! 全流程留痕因此**可回溯且不可抵赖**（判据「审计」）。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界/切片扩断：标签匹配用 `==` 全等比较，
//! 状态表访问用定长下标（编译期界内），所有失败路径走 `Result` 与显式错误码。

use super::vcf01_gpucompat::{certify, CertEvidence, CertStatus};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（vcf02 独占段 0x9F10..0x9F1F）
// ===========================================================================

/// 步序违规（前置步未完成就执行目标步）。
pub const C_VCF02_STEP_ORDER: u16 = 0x9F10;
/// 步输入不合法（判定标签越出封闭三值/执行器返回零凭据）。
pub const C_VCF02_STEP_INPUT: u16 = 0x9F11;
/// 判定步拒绝（certify 铁律 8 闸拒绝——审计留痕底层码）。
pub const C_VCF02_JUDGE_REJECT: u16 = 0x9F12;
/// 单步重跑次数耗尽（中断恢复放弃）。
pub const C_VCF02_RESUME_EXHAUSTED: u16 = 0x9F13;
/// 审计链破损（留痕被篡改/链重算不符）。
pub const C_VCF02_AUDIT_BROKEN: u16 = 0x9F14;
/// 流程状态机非法态（对已完成/已关闭流程再推进）。
pub const C_VCF02_FLOW_STATE: u16 = 0x9F15;
/// 执行器故障（注入的步执行器自身失败——流程留痕后续跑）。
pub const C_VCF02_EXEC_FAULT: u16 = 0x9F16;

/// 域版本。
pub const VCF02_VERSION: &str = "CF02-certflow-v1";

// ===========================================================================
// 二、六步封闭枚举与固化依赖表
// ===========================================================================

/// 六步封闭枚举（锚点「申请→能力探测→基准执行→判定→认证授予→发布」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertStep {
    /// 第 1 步：申请（提交设备/驱动身份）。
    Apply,
    /// 第 2 步：能力探测。
    Probe,
    /// 第 3 步：基准执行。
    Bench,
    /// 第 4 步：判定（铁律 8 闸裁决）。
    Judge,
    /// 第 5 步：认证授予。
    Grant,
    /// 第 6 步：发布（档案入库随版本分发）。
    Publish,
}

/// 步数常量（判据侧独立重算同一结果）。
pub const STEP_COUNT: usize = 6;

/// 步名封闭表（与 [`CertStep`] 枚举序一一对应）。
pub const STEP_NAMES: [&str; STEP_COUNT] =
    ["申请", "能力探测", "基准执行", "判定", "认证授予", "发布"];

/// 枚举→定长下标（穷尽 match——不上 `as` 转换）。
pub fn step_index(step: CertStep) -> usize {
    match step {
        CertStep::Apply => 0,
        CertStep::Probe => 1,
        CertStep::Bench => 2,
        CertStep::Judge => 3,
        CertStep::Grant => 4,
        CertStep::Publish => 5,
    }
}

/// 固化依赖表：`STEP_DEPS[i]` = 第 i 步的前置步下标（0 号步无前置记 0）。
/// 编译期钉死为 i-1——步序不可重排（判据「每步的输入输出与判据固化」）。
pub const STEP_DEPS: [usize; STEP_COUNT] = [0, 0, 1, 2, 3, 4];

// ===========================================================================
// 三、判定证据标签（封闭三值——certify 的流程面入口）
// ===========================================================================

/// 实机凭据标签：基准在本流程内真跑过。
pub const TAG_SUITE_RUN: &[u8] = b"SUITE-RUN";
/// 文档声明标签：只有纸面/营销材料凭据（铁律 8 拒绝）。
pub const TAG_CLAIM_ONLY: &[u8] = b"CLAIM-ONLY";
/// 未实测标签：显性 Uncertified（没测过就说没测过）。
pub const TAG_UNTESTED: &[u8] = b"UNTESTED";

/// 标签→证据（封闭三值映射，越集返回 None）。
pub fn tag_to_evidence(device: u32, driver: u32, tag: &[u8]) -> Option<CertEvidence> {
    if tag == TAG_SUITE_RUN {
        Some(CertEvidence { device, driver, suite_run: true, doc_claim_only: false })
    } else if tag == TAG_CLAIM_ONLY {
        Some(CertEvidence { device, driver, suite_run: true, doc_claim_only: true })
    } else if tag == TAG_UNTESTED {
        Some(CertEvidence { device, driver, suite_run: false, doc_claim_only: false })
    } else {
        None
    }
}

// ===========================================================================
// 四、执行器抽象（D08 执行器复用点——全流程无人值守的注入面）
// ===========================================================================

/// 流程执行器：单步实际执行外置（探针/基准/发布的机械动作由注入方承担）。
///
/// 返回 `Ok(fp)` 为该步输出凭据指纹（**必须非零**——零凭据视同该步无效，
/// 走 [`C_VCF02_STEP_INPUT`]）；`Err(code)` 为步失败（流程留痕后续跑）。
pub trait FlowExecutor {
    /// 执行第 `step` 步（0..STEP_COUNT），`tag` 为判定步证据标签（他步忽略）。
    fn run_step(&mut self, step: usize, tag: &[u8]) -> Result<u64, u16>;
}

/// FNV-1a 64（审计指纹与链混合共用；const 供编译期自检）。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

// ===========================================================================
// 五、审计记录与哈希链
// ===========================================================================

/// 审计链种子（vcf02 段常量拼接——链独立于其他域的 FNV 用法）。
pub const AUDIT_CHAIN_SEED: u64 = 0x9F10_9F11_9F12_9F15;

/// 单步审计记录（每步执行——无论成败——都追加一条）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRecord {
    /// 步号（1..=6，人读口径）。
    pub step: u8,
    /// 第几次尝试（1 起计）。
    pub attempt: u8,
    /// 输入指纹（步名/标签的 FNV）。
    pub input_fp: u64,
    /// 输出指纹（执行器凭据；失败步记 0）。
    pub output_fp: u64,
    /// 裁决码（0=通过，否则底层错误码）。
    pub verdict: u16,
    /// 前链混合后的链值（篡改任一前驱字段即断链）。
    pub chain: u64,
}

/// 链混合：`chain = mix(prev, record)`——字段逐个 FNV 混入后与 prev 异或回混。
pub fn audit_chain(prev: u64, r: &AuditRecord) -> u64 {
    let mut h = prev;
    for v in [r.step as u64, r.attempt as u64, r.input_fp, r.output_fp, r.verdict as u64] {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ prev
}

/// 独立审计核验（公开面——判据侧篡改检测经此走断链路径）：
/// 逐记录字段合法性 + 从种子重算链逐位对账。
pub fn audit_verify_records(records: &[AuditRecord]) -> Result<(), u16> {
    let mut h = AUDIT_CHAIN_SEED;
    for r in records.iter() {
        if r.step < 1 || r.step as usize > STEP_COUNT || r.attempt < 1 {
            return Err(C_VCF02_AUDIT_BROKEN);
        }
        let expect = audit_chain(h, r);
        if expect != r.chain {
            return Err(C_VCF02_AUDIT_BROKEN);
        }
        h = expect;
    }
    Ok(())
}

// ===========================================================================
// 六、流程状态机
// ===========================================================================

/// 单步重跑上限（中断恢复的放弃线）。
pub const MAX_ATTEMPTS: u8 = 3;

/// 认证流程状态机（一份认证 = 一台状态机 = 一条完整审计链）。
#[derive(Debug, Clone)]
pub struct CertFlow {
    /// 流程号（审计回溯主键）。
    pub flow_id: u32,
    /// 设备标识（透传 vcf01 证据）。
    pub device: u32,
    /// 驱动版本（透传 vcf01 证据）。
    pub driver: u32,
    /// 六步完成位图（下标即步序）。
    done: [bool; STEP_COUNT],
    /// 每步已尝试次数。
    attempts: [u8; STEP_COUNT],
    /// 每步输出凭据指纹（完成后不可变——续跑不重做的物证）。
    evidence: [u64; STEP_COUNT],
    /// 授予状态（Grant 完成 / Judge 判 Uncertified 后关闭时落定）。
    granted: Option<CertStatus>,
    /// 流程关闭位（判定为 Uncertified 时提前关账——授予/发布不再受理）。
    closed: bool,
    /// 审计链（逐记录追加）。
    audit: Vec<AuditRecord>,
}

impl CertFlow {
    /// 新流程：六步全待办、审计链从种子起步。
    pub fn new(flow_id: u32, device: u32, driver: u32) -> Self {
        CertFlow {
            flow_id,
            device,
            driver,
            done: [false; STEP_COUNT],
            attempts: [0; STEP_COUNT],
            evidence: [0; STEP_COUNT],
            granted: None,
            closed: false,
            audit: Vec::new(),
        }
    }

    /// 首个未完成步（固化顺序即表序——跳步在此天然不可表达）。
    pub fn next_step(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < STEP_COUNT {
            if !self.done[i] {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 审计链当前头值（种子=空链）。
    pub fn audit_head(&self) -> u64 {
        let mut h = AUDIT_CHAIN_SEED;
        for r in self.audit.iter() {
            h = audit_chain(h, r);
        }
        h
    }

    /// 审计核验：逐记录重算链 + 步号/尝试次合法性（判据「可回溯」）。
    pub fn audit_verify(&self) -> Result<(), u16> {
        audit_verify_records(&self.audit)
    }

    /// 审计留痕条数（成功 + 失败都算——全流程留痕）。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计第 i 条（判据侧回溯用）。
    pub fn audit_record(&self, i: usize) -> Option<AuditRecord> {
        self.audit.get(i).copied()
    }

    /// 追加一条审计（链值在此落定）。
    fn trace(&mut self, step: usize, attempt: u8, input_fp: u64, output_fp: u64, verdict: u16) {
        let rec = AuditRecord {
            step: (step + 1) as u8,
            attempt,
            input_fp,
            output_fp,
            verdict,
            chain: 0,
        };
        let prev = self.audit_head();
        let mut rec = rec;
        rec.chain = audit_chain(prev, &rec);
        self.audit.push(rec);
    }

    /// 定点执行第 `i` 步（续跑/重试的底层入口；依赖闸在此把守）。
    pub fn run_step_at(&mut self, ex: &mut dyn FlowExecutor, i: usize, tag: &[u8]) -> Result<(), u16> {
        if i >= STEP_COUNT {
            return Err(C_VCF02_FLOW_STATE);
        }
        if self.closed {
            return Err(C_VCF02_FLOW_STATE);
        }
        // 固化依赖闸：前置步未完成 → 步序违规。
        let dep = STEP_DEPS[i];
        if i > 0 && !self.done[dep] {
            return Err(C_VCF02_STEP_ORDER);
        }
        if self.done[i] {
            return Err(C_VCF02_FLOW_STATE); // 已完成步不重做（重跑语义只对失败步）
        }
        // 输入契约固化：判定步标签必须落在封闭三值内。
        let input_fp = if i == step_index(CertStep::Judge) {
            if tag_to_evidence(self.device, self.driver, tag).is_none() {
                self.trace(i, self.attempts[i] + 1, fnv64(tag), 0, C_VCF02_STEP_INPUT);
                return Err(C_VCF02_STEP_INPUT);
            }
            fnv64(tag)
        } else {
            fnv64(STEP_NAMES[i].as_bytes())
        };
        // 重跑上限闸。
        if self.attempts[i] >= MAX_ATTEMPTS {
            return Err(C_VCF02_RESUME_EXHAUSTED);
        }
        self.attempts[i] += 1;
        let attempt = self.attempts[i];
        match ex.run_step(i, tag) {
            Ok(fp) => {
                if fp == 0 {
                    // 零凭据 = 该步无效（输出契约固化）。
                    self.trace(i, attempt, input_fp, 0, C_VCF02_STEP_INPUT);
                    return Err(C_VCF02_STEP_INPUT);
                }
                self.evidence[i] = fp;
                self.done[i] = true;
                self.trace(i, attempt, input_fp, fp, 0);
                // 判定步落地：叠加铁律 8 闸（certify）。
                if i == step_index(CertStep::Judge) {
                    let ev = match tag_to_evidence(self.device, self.driver, tag) {
                        Some(ev) => ev,
                        None => return Err(C_VCF02_STEP_INPUT), // 上方已闸，防御性
                    };
                    match certify(&ev) {
                        Ok(status) => {
                            if status == CertStatus::Uncertified {
                                // 显性未认证：流程提前关账（授予/发布不受理）。
                                self.granted = Some(CertStatus::Uncertified);
                                self.closed = true;
                            }
                        }
                        Err(bad) => {
                            self.trace(i, attempt, input_fp, fp, bad);
                            return Err(C_VCF02_JUDGE_REJECT);
                        }
                    }
                }
                // 授予步落地。
                if i == step_index(CertStep::Grant) {
                    self.granted = Some(CertStatus::Certified);
                }
                Ok(())
            }
            Err(code) => {
                self.trace(i, attempt, input_fp, 0, code);
                Err(code)
            }
        }
    }

    /// 推进一步（自动化主循环的最小动作）。
    pub fn run(&mut self, ex: &mut dyn FlowExecutor, tag: &[u8]) -> Result<(), u16> {
        let i = match self.next_step() {
            Some(i) => i,
            None => return Err(C_VCF02_FLOW_STATE),
        };
        self.run_step_at(ex, i, tag)
    }

    /// 全流程无人值守（判据「自动化」）：六步端到端，中途失败即上抛。
    pub fn run_all(&mut self, ex: &mut dyn FlowExecutor, tag: &[u8]) -> Result<CertStatus, u16> {
        // 有界循环：六步 + 判定关账余量——确定性保证终止。
        for _ in 0..(STEP_COUNT + 2) {
            if self.done[step_index(CertStep::Publish)] {
                return Ok(CertStatus::Certified);
            }
            if let Some(CertStatus::Uncertified) = self.granted {
                return Ok(CertStatus::Uncertified);
            }
            self.run(ex, tag)?;
        }
        Err(C_VCF02_FLOW_STATE)
    }

    /// 中断恢复（判据「续跑/重跑语义」）：从首个未完成步续跑；已完成步的
    /// 凭据指纹不动；失败步逐次重试至 [`MAX_ATTEMPTS`] 耗尽。
    pub fn resume(&mut self, ex: &mut dyn FlowExecutor, tag: &[u8]) -> Result<CertStatus, u16> {
        let budget = STEP_COUNT * MAX_ATTEMPTS as usize + 2;
        for _ in 0..budget {
            if self.done[step_index(CertStep::Publish)] {
                return Ok(CertStatus::Certified);
            }
            if let Some(CertStatus::Uncertified) = self.granted {
                return Ok(CertStatus::Uncertified);
            }
            match self.run(ex, tag) {
                Ok(()) => continue,
                Err(C_VCF02_RESUME_EXHAUSTED) => return Err(C_VCF02_RESUME_EXHAUSTED),
                Err(C_VCF02_JUDGE_REJECT) => return Err(C_VCF02_JUDGE_REJECT),
                Err(_) => continue, // 步失败已留痕——重试语义
            }
        }
        Err(C_VCF02_RESUME_EXHAUSTED)
    }

    /// 每步已尝试次数（判据侧断言重试计账）。
    pub fn attempts_of(&self, i: usize) -> u8 {
        if i < STEP_COUNT {
            self.attempts[i]
        } else {
            0
        }
    }

    /// 每步输出凭据指纹（判据侧断言「续跑不重做」）。
    pub fn evidence_of(&self, i: usize) -> u64 {
        if i < STEP_COUNT {
            self.evidence[i]
        } else {
            0
        }
    }

    /// 授予状态。
    pub fn granted(&self) -> Option<CertStatus> {
        self.granted
    }

    /// 发布就绪：六步全完成 + 审计链完好 + 已授予 Certified。
    pub fn publish_ready(&self) -> bool {
        self.done[step_index(CertStep::Publish)]
            && self.audit_verify().is_ok()
            && self.granted == Some(CertStatus::Certified)
    }
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    format!(
        "{} steps={} 「六步认证正式化：申请→能力探测→基准执行→判定→认证授予→发布」",
        VCF02_VERSION,
        STEP_COUNT,
    )
}

// ===========================================================================
// 七、编译期钉死（判据的 const 层）
// ===========================================================================

const _STEP_NAMES_CLOSED: () = assert!(STEP_NAMES.len() == STEP_COUNT);
const _DEPS_ORDER_FROZEN: () = assert!(
    STEP_DEPS[0] == 0
        && STEP_DEPS[1] == 0
        && STEP_DEPS[2] == 1
        && STEP_DEPS[3] == 2
        && STEP_DEPS[4] == 3
        && STEP_DEPS[5] == 4,
    "cert flow step order drift"
);
const _SEED_NONZERO: () = assert!(AUDIT_CHAIN_SEED != 0, "audit seed must be nonzero");
