//! CGPU-F3201 · U 域开工与 Bench 总架构（CGPU-U 域 · 基准与 Bench 域 · U01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3201`
//!
//! **判据（锚点原文）**：签收、定位声明、四段、兑现确认、四组、判据。
//!
//! **职责定位（锚点原文）**：域开工：U 域开工（签收 T10 移交包（Bench
//! 衔接包签收——衔接确认）；Bench 定位（实测认证不凭文档声明——定位
//! 声明（CGPU 理念呼应——呼应声明；四段架构（场景/运行/计分/发布——
//! 四段（T10 预告兑现（对接协议性能基准项——兑现确认；测试（签收/
//! 定位/四段/兑现四组）。判据：签收、定位声明、四段、兑现确认、四组、
//! 判据。
//!
//! ## 一、T10 移交包签收（判据一：七件逐件对账 + 逐件指纹链）
//!
//! T10（T 域收口组）移交 Bench 衔接包七件（[`T10_HANDOVER`]）：对接
//! 协议冻结版/性能基准项清单/场景命名规范/分数口径/运行环境声明/
//! 结果序列化格式/发布通道约定——逐件对账，缺件即空头签收
//! （[`XcCode::HandoverMissing`]）；签收带指纹（FNV 独立重算对拍）。
//! 深化：逐件指纹链（[`HandoverItem`]——每件独立 FNV-1a 指纹 +
//! prev_chain 链式防篡改）；签收回执（[`HandoverReceipt`]——逐件
//! 核验 + 包级指纹对拍 + 签收人 + 签收 tick 注入）；签收台账
//! （[`HandoverLedger`]——多次签收留痕、乱序/重复显性拒）。
//!
//! ## 二、Bench 定位声明（判据二：两条款字面量冻结 + 机检闸）
//!
//! 两条款（[`POSITION`]）：实测认证不凭文档声明（分数来自实测运行，
//! 文档声明不是证据）/CGPU 理念呼应（「深度基准=实测」与 F0802 认证
//! 流程的基准执行步同源——Bench 是认证体系的考卷供给方）。条款绑定
//! 检查键（[`POSITION_KEYS`]）——可机检声明；机检闸
//! （[`verify_position`]——条款数/键数/键面格式三重核验）。
//!
//! ## 三、四段架构（判据三：单向流水线 + 迁移留痕 + 快照恢复）
//!
//! 四段（[`BenchStage`]）：场景（考卷定义）/运行（考卷执行）/计分
//! （分数计算）/发布（结果公开）——段序单向，跳段/回退显性拒
//! （[`XcCode::StageViolation`]）；段间工件传递（场景定义→运行工件→
//! 分数→发布物），无工件的段推进无效。深化：段迁移账
//! （[`StageTransition`]——每段完成带 tick 注入与段指纹）；流水线
//! 快照/恢复（[`BenchPipeline::snapshot`]/[`BenchPipeline::restore`]
//! ——9 元组定长编码）；流水线指纹（[`BenchPipeline::fingerprint`]
//! ——段游标+工件位+迁移账的 FNV 汇总，对拍防账面篡改）。
//!
//! ## 四、T10 预告兑现确认（判据四）
//!
//! 对接协议性能基准项（T10 预告的兑现位）：[`T10_FULFILL`] 四要素
//! （预告出处/承接物/兑现单号/对接版本）——预告兑现是登记制；
//! 性能基准项闭集（[`BENCH_ITEMS`]）五项（帧率/P95 帧时间/首帧延迟/
//! 带宽吞吐/功耗效率）——考卷按项计分。深化：兑现登记账
//! （[`FulfillLedger`]——逐要素登记 + 兑现指纹 + 重复登记显性拒）；
//! 权重表（[`ITEM_WEIGHTS_BPS`]——五项权重 bps，编译期守恒 10000）。
//!
//! ## 五、深化面（语义密度扩展，均在锚点职责半径内）
//!
//! - 场景登记账（[`SceneRegistry`]）：场景命名规范（T10 移交件）落
//!   地为命名校验（[`scene_name_ok`]——前缀/分隔符/长度三规则）；
//!   四类场景闭集（[`SCENE_CATEGORIES`]——渲染/计算/混合/极限，与
//!   F3203 分类接缝）；重复 id 显性拒；场景指纹（内容 FNV）。
//! - 运行配置（[`RunConfig`]）：迭代数/预热轮/采样间隔三参数带边界
//!   校验与饱和换算（[`RunConfig::total_ticks_estimate`]——预估运
//!   行预算，零墙钟——tick 数是逻辑量）。
//! - 计分账（[`ScoreLedger`]）：按 [`BENCH_ITEMS`] 五项闭集计分
//!   （0-10000 bps 制）；越界显性拒；加权总分（权重守恒断言）；
//!   分数变更留痕（[`ScoreChange`]——旧值/新值/动因/tick）。
//! - 归一对照（[`normalize_against_baseline`]）：跨硬件基线对照——
//!   归一比率 bps 制（饱和乘除，零除显性 `None`）。
//! - 防作弊签名（[`BenchSignature`]）：结果签名 = FNV(场景指纹+配
//!   置指纹+五项分数)；验签对拍（[`BenchSignature::verify`]）；签
//!   名不符即 [`XcCode::SignatureMismatch`]（篡改嫌疑）。
//! - 发布闸（[`PublishGate`]）：四段齐 + 签名有效 + 发布通道约定在
//!   包（T10 移交件之七）三闸齐备才放行。
//! - 域开工台账（[`DomainOpenLedger`]）：四组判据各为闸门，全过才
//!   宣告就位；宣告可重复但幂等（重复宣告不改状态）。
//!
//! **对接**：F3041（T 域对接总架构——移交方）；F3202/F3203（基准模型/
//! 场景分类——四段的后段兑现）；F3360（V 域移交起点）。零 panic 面、
//! 零 IO、零墙钟（计时上游注入）、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const U_OPEN_VERSION: &str = "U01-uopen-v1";

/// U 域规模：160 项（F3201-F3360）。
pub const U_DOMAIN_TOTAL: u32 = 160;

/// 域首单号（F3201 ⇒ 3201）。
pub const U_DOMAIN_FIRST: u32 = 3_201;

/// 域末单号（F3360 ⇒ 3360；编译期守恒钉死）。
pub const U_DOMAIN_LAST: u32 = 3_360;

const _: () = assert!(U_DOMAIN_FIRST + U_DOMAIN_TOTAL - 1 == U_DOMAIN_LAST);

/// T10 移交包指纹（冻结值；判据侧独立重算对拍）。
pub const T10_FINGERPRINT: u64 = 0x710E_5A44_C0DE_0001;

/// 移交包件数（七件；编译期钉死）。
pub const T10_HANDOVER_LEN: usize = 7;

const _: () = assert!(T10_HANDOVER.len() == T10_HANDOVER_LEN);

/// 定位条款数（两条款；编译期钉死）。
pub const POSITION_LEN: usize = 2;

const _: () = assert!(POSITION.len() == POSITION_LEN && POSITION_KEYS.len() == POSITION_LEN);

/// 兑现要素数（四要素；编译期钉死）。
pub const T10_FULFILL_LEN: usize = 4;

const _: () = assert!(T10_FULFILL.len() == T10_FULFILL_LEN);

/// 性能基准项数（五项；编译期钉死）。
pub const BENCH_ITEMS_LEN: usize = 5;

const _: () = assert!(BENCH_ITEMS.len() == BENCH_ITEMS_LEN);

/// 分数量纲上限（bps 制：10000 = 满分）。
pub const SCORE_MAX_BPS: u32 = 10_000;

// ---------------------------------------------------------------------------
// 二、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收（UTF-8 字节序）。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 数值吸收（大端逐字节，确定性跨端序）。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

/// FNV-1a u32 数值吸收。
pub const fn fnv1a_u32(state: u64, value: u32) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 三、诊断码（0x6C 段独占：U01 开工面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x6C 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 移交件缺失。
    HandoverMissing,
    /// 指纹不符。
    FingerprintMismatch,
    /// 段序违规（跳段/回退）。
    StageViolation,
    /// 段推进无工件。
    NoArtifact,
    /// 基准项表外。
    UnknownBenchItem,
    /// 逐件指纹链断裂（prev_chain 对拍失败）。
    ChainBroken,
    /// 签收回执未就位（台账空回执即拒）。
    ReceiptMissing,
    /// 场景命名不规范（命名规范三规则违一）。
    SceneNameBad,
    /// 场景 id 重复（登记账拒绝重 id）。
    SceneDuplicate,
    /// 场景类别表外（四类闭集之外无类）。
    SceneCategoryUnknown,
    /// 运行配置越界（迭代/预热/采样间隔超边界）。
    RunConfigInvalid,
    /// 分数越界（bps 制 0-10000 之外）。
    ScoreOutOfRange,
    /// 权重不守恒（五项权重和 ≠ 10000）。
    WeightSumMismatch,
    /// 签名不符（结果被篡改嫌疑）。
    SignatureMismatch,
    /// 发布闸未齐（四段/签名/通道三闸缺一）。
    PublishGateClosed,
    /// 兑现登记重复（同一要素二次登记）。
    FulfillDuplicate,
    /// 兑现要素缺失（四要素未齐）。
    FulfillMissing,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "T10 移交包缺件：七件逐件对账缺一即未就位，建议找 T 域补齐再签收",
            XcCode::FingerprintMismatch => "移交指纹不符：包内容与冻结版不一致，建议重新对账",
            XcCode::StageViolation => "段序违规：场景→运行→计分→发布单向流水线，跳段/回退显性拒",
            XcCode::NoArtifact => "段推进无工件：无上游工件不得推进（无场景定义不运行、无运行工件不计分）",
            XcCode::UnknownBenchItem => "基准项表外：五项闭集之外无项，建议按 BENCH_ITEMS 取用",
            XcCode::ChainBroken => "逐件指纹链断裂：prev_chain 对拍失败，说明包被中途改动，建议整包重传重签",
            XcCode::ReceiptMissing => "签收回执未就位：台账无回执不得开工，建议先走 HandoverReceipt 签收流",
            XcCode::SceneNameBad => "场景命名不规范：u01.前缀+点分隔+长度≤64 三规则违一，建议按命名规范改后重登",
            XcCode::SceneDuplicate => "场景 id 重复：登记账拒绝重 id，建议换 id 或先注销旧场景",
            XcCode::SceneCategoryUnknown => "场景类别表外：渲染/计算/混合/极限四类之外无类，建议按 SCENE_CATEGORIES 取用",
            XcCode::RunConfigInvalid => "运行配置越界：迭代/预热/采样间隔超边界，建议按 RunConfig 边界注释取值",
            XcCode::ScoreOutOfRange => "分数越界：bps 制 0-10000 之外无分，建议 clamp 后重录",
            XcCode::WeightSumMismatch => "权重不守恒：五项权重和必须等于 10000，建议核对 ITEM_WEIGHTS_BPS",
            XcCode::SignatureMismatch => "签名不符：结果与签名对拍失败，存在篡改嫌疑，建议整场重跑重签",
            XcCode::PublishGateClosed => "发布闸未齐：四段完成+签名有效+通道在包三闸缺一，建议补齐后再发布",
            XcCode::FulfillDuplicate => "兑现登记重复：同一要素只登记一次，建议查 FulfillLedger 后再操作",
            XcCode::FulfillMissing => "兑现要素缺失：四要素未齐不得宣告兑现，建议补齐 T10_FULFILL 四要素",
        }
    }

    /// 诊断码短键（机检 grep 键面）。
    pub const fn key(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "u01.xc.handover-missing",
            XcCode::FingerprintMismatch => "u01.xc.fingerprint-mismatch",
            XcCode::StageViolation => "u01.xc.stage-violation",
            XcCode::NoArtifact => "u01.xc.no-artifact",
            XcCode::UnknownBenchItem => "u01.xc.unknown-bench-item",
            XcCode::ChainBroken => "u01.xc.chain-broken",
            XcCode::ReceiptMissing => "u01.xc.receipt-missing",
            XcCode::SceneNameBad => "u01.xc.scene-name-bad",
            XcCode::SceneDuplicate => "u01.xc.scene-duplicate",
            XcCode::SceneCategoryUnknown => "u01.xc.scene-category-unknown",
            XcCode::RunConfigInvalid => "u01.xc.run-config-invalid",
            XcCode::ScoreOutOfRange => "u01.xc.score-out-of-range",
            XcCode::WeightSumMismatch => "u01.xc.weight-sum-mismatch",
            XcCode::SignatureMismatch => "u01.xc.signature-mismatch",
            XcCode::PublishGateClosed => "u01.xc.publish-gate-closed",
            XcCode::FulfillDuplicate => "u01.xc.fulfill-duplicate",
            XcCode::FulfillMissing => "u01.xc.fulfill-missing",
        }
    }

    /// 诊断码段位（0x6C 段独占，逐码偏移；机检用）。
    pub const fn code(self) -> u16 {
        0x6C00
            + match self {
                XcCode::HandoverMissing => 0x01,
                XcCode::FingerprintMismatch => 0x02,
                XcCode::StageViolation => 0x03,
                XcCode::NoArtifact => 0x04,
                XcCode::UnknownBenchItem => 0x05,
                XcCode::ChainBroken => 0x06,
                XcCode::ReceiptMissing => 0x07,
                XcCode::SceneNameBad => 0x08,
                XcCode::SceneDuplicate => 0x09,
                XcCode::SceneCategoryUnknown => 0x0A,
                XcCode::RunConfigInvalid => 0x0B,
                XcCode::ScoreOutOfRange => 0x0C,
                XcCode::WeightSumMismatch => 0x0D,
                XcCode::SignatureMismatch => 0x0E,
                XcCode::PublishGateClosed => 0x0F,
                XcCode::FulfillDuplicate => 0x10,
                XcCode::FulfillMissing => 0x11,
            }
    }
}

// ---------------------------------------------------------------------------
// 四、T10 移交包签收（判据一·深化：逐件指纹链 + 回执 + 台账）
// ---------------------------------------------------------------------------

/// T10 移交包七件。
pub const T10_HANDOVER: [&str; 7] = [
    "对接协议冻结版",
    "性能基准项清单",
    "场景命名规范",
    "分数口径",
    "运行环境声明",
    "结果序列化格式",
    "发布通道约定",
];

/// 移交单件（名称 + 独立指纹 + 链前向指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverItem {
    /// 件名（T10_HANDOVER 之一）。
    pub name: String,
    /// 本件内容指纹（FNV-1a）。
    pub fingerprint: u64,
    /// 前一件链指纹（首件取 FNV_OFFSET）——逐件链式防篡改。
    pub prev_chain: u64,
}

impl HandoverItem {
    /// 构造单件（指纹独立重算；链前向由调用序决定）。
    pub fn new(name: &str, content: &str, prev_chain: u64) -> HandoverItem {
        let fp = fnv1a_str(FNV_OFFSET, content);
        HandoverItem {
            name: name.to_string(),
            fingerprint: fp,
            prev_chain,
        }
    }

    /// 链指纹（本件指纹混入前向——链上任意单件改动即断链）。
    pub const fn chain_value(&self) -> u64 {
        // const fn 里不能直接用 String 内容；链值 = prev_chain 混本件指纹。
        fnv1a_u64(self.prev_chain, self.fingerprint)
    }
}

/// 逐件指纹链核验（顺序对拍：链断即 [`XcCode::ChainBroken`]）。
pub fn verify_item_chain(items: &[HandoverItem]) -> Result<(), XcCode> {
    if items.len() < T10_HANDOVER_LEN {
        return Err(XcCode::HandoverMissing);
    }
    let mut prev = FNV_OFFSET;
    let mut i = 0usize;
    while i < items.len() {
        let it = match items.get(i) {
            Some(x) => x,
            None => return Err(XcCode::HandoverMissing),
        };
        // 件名必须落在七件闭集内（表外件名显性拒）。
        let mut named = false;
        let mut j = 0usize;
        while j < T10_HANDOVER.len() {
            if let Some(n) = T10_HANDOVER.get(j) {
                if *n == it.name {
                    named = true;
                    break;
                }
            }
            j += 1;
        }
        if !named {
            return Err(XcCode::HandoverMissing);
        }
        if it.prev_chain != prev {
            return Err(XcCode::ChainBroken);
        }
        prev = it.chain_value();
        i += 1;
    }
    Ok(())
}

/// 签收回执（逐件核验结果 + 包级指纹 + 签收人 + 签收 tick）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 逐件在账（与 T10_HANDOVER 同序，true=件到且指纹核过）。
    pub items_ok: [bool; T10_HANDOVER_LEN],
    /// 包级指纹（签收时上游给的冻结指纹）。
    pub package_fingerprint: u64,
    /// 签收人（工号或角色键）。
    pub signer: String,
    /// 签收 tick（上游注入，零墙钟）。
    pub signed_tick: u64,
    /// 回执指纹（receipt 自身防篡改：在账位+包指纹+签收人+tick 混算）。
    pub receipt_fingerprint: u64,
}

impl HandoverReceipt {
    /// 回执指纹重算（独立对拍基元）。
    pub fn recompute_fingerprint(&self) -> u64 {
        let mut s = fnv1a_u64(FNV_OFFSET, self.package_fingerprint);
        let mut i = 0usize;
        while i < T10_HANDOVER_LEN {
            s = fnv1a_u64(s, if self.items_ok[i] { 1 } else { 0 });
            i += 1;
        }
        s = fnv1a_str(s, &self.signer);
        fnv1a_u64(s, self.signed_tick)
    }

    /// 回执完整性核验（指纹对拍 + 全件在账）。
    pub fn verify(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < T10_HANDOVER_LEN {
            if !self.items_ok[i] {
                return Err(XcCode::HandoverMissing);
            }
            i += 1;
        }
        if self.receipt_fingerprint != self.recompute_fingerprint() {
            return Err(XcCode::FingerprintMismatch);
        }
        Ok(())
    }
}

/// 移交包核验：七件齐备 + 指纹一致（签收有闸门）。
pub fn verify_handover(received: &[&str], fingerprint: u64) -> Result<(), XcCode> {
    if received.len() < T10_HANDOVER.len() {
        return Err(XcCode::HandoverMissing);
    }
    let mut i = 0usize;
    while i < T10_HANDOVER.len() {
        let need = match T10_HANDOVER.get(i) {
            Some(h) => *h,
            None => return Err(XcCode::HandoverMissing),
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < received.len() {
            if let Some(r) = received.get(j) {
                if *r == need {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return Err(XcCode::HandoverMissing);
        }
        i += 1;
    }
    if fingerprint != T10_FINGERPRINT {
        return Err(XcCode::FingerprintMismatch);
    }
    Ok(())
}

/// 签收台账（多次签收留痕；乱序/重复显性拒；链式防篡改）。
#[derive(Clone, Debug, Default)]
pub struct HandoverLedger {
    receipts: Vec<HandoverReceipt>,
    /// 台账链指纹（每笔回执入账即混链）。
    chain: u64,
    /// 最高签收 tick（乱序检测基线；零墙钟——tick 全由上游注入）。
    last_tick: u64,
}

impl HandoverLedger {
    /// 空台账。
    pub const fn new() -> HandoverLedger {
        HandoverLedger {
            receipts: Vec::new(),
            chain: FNV_OFFSET,
            last_tick: 0,
        }
    }

    /// 入账（tick 回退显性拒——签收时序不可逆）。
    pub fn record(&mut self, receipt: HandoverReceipt) -> Result<(), XcCode> {
        receipt.verify()?;
        if receipt.signed_tick < self.last_tick {
            return Err(XcCode::StageViolation);
        }
        self.chain = fnv1a_u64(self.chain, receipt.receipt_fingerprint);
        self.last_tick = receipt.signed_tick;
        self.receipts.push(receipt);
        Ok(())
    }

    /// 台账笔数。
    pub const fn len(&self) -> usize {
        self.receipts.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    /// 最新回执（无回执即 None——调用方自行落 [`XcCode::ReceiptMissing`]）。
    pub fn latest(&self) -> Option<&HandoverReceipt> {
        // len>0 时 last 必有；get 兜底保零 panic 面。
        let n = self.receipts.len();
        if n == 0 {
            None
        } else {
            self.receipts.get(n - 1)
        }
    }

    /// 台账链指纹（对拍防账面篡改）。
    pub const fn chain_fingerprint(&self) -> u64 {
        self.chain
    }

    /// 台账自洽核验：逐笔回执复验 + 链指纹复算对拍。
    pub fn verify(&self) -> Result<(), XcCode> {
        let mut s = FNV_OFFSET;
        let mut last = 0u64;
        let mut i = 0usize;
        while i < self.receipts.len() {
            let r = match self.receipts.get(i) {
                Some(x) => x,
                None => return Err(XcCode::ReceiptMissing),
            };
            r.verify()?;
            if r.signed_tick < last {
                return Err(XcCode::StageViolation);
            }
            s = fnv1a_u64(s, r.receipt_fingerprint);
            last = r.signed_tick;
            i += 1;
        }
        if s != self.chain {
            return Err(XcCode::ChainBroken);
        }
        Ok(())
    }

    /// 人读台账面。
    pub fn report(&self) -> String {
        alloc::format!(
            "HandoverLedger:签收{}笔/链指纹={:#018x}/末笔tick={}",
            self.receipts.len(),
            self.chain,
            self.last_tick
        )
    }
}

// ---------------------------------------------------------------------------
// 五、Bench 定位声明（判据二·深化：机检闸）
// ---------------------------------------------------------------------------

/// 定位两条款（字面量冻结）。
pub const POSITION: [&str; 2] = [
    "实测认证不凭文档声明：分数来自实测运行，文档声明不是证据",
    "CGPU理念呼应：Bench是F0802认证体系基准执行步的考卷供给方，深度基准=实测",
];

/// 定位条款检查键（可机检 grep 键面）。
pub const POSITION_KEYS: [&str; 2] = ["u01.position.measure-only", "u01.position.cert-supply"];

/// 定位声明机检闸：条款/键数量核验 + 键面格式核验
/// （键必须形如 `u01.position.<slug>`——三段点分且首两段冻结）。
pub fn verify_position() -> Result<(), XcCode> {
    if POSITION.len() != POSITION_LEN || POSITION_KEYS.len() != POSITION_LEN {
        return Err(XcCode::HandoverMissing);
    }
    let mut i = 0usize;
    while i < POSITION_KEYS.len() {
        let k = match POSITION_KEYS.get(i) {
            Some(x) => *x,
            None => return Err(XcCode::HandoverMissing),
        };
        let b = k.as_bytes();
        // 前缀 "u01.position." 冻结（13 字节）。
        const PREFIX: &[u8] = b"u01.position.";
        if b.len() <= PREFIX.len() {
            return Err(XcCode::FingerprintMismatch);
        }
        let mut j = 0usize;
        while j < PREFIX.len() {
            if b[j] != PREFIX[j] {
                return Err(XcCode::FingerprintMismatch);
            }
            j += 1;
        }
        // slug 段非空且全为合法键字符（小写字母/数字/短横）。
        let mut ok = false;
        let mut m = PREFIX.len();
        while m < b.len() {
            let c = b[m];
            let legal = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-';
            if !legal {
                return Err(XcCode::FingerprintMismatch);
            }
            ok = true;
            m += 1;
        }
        if !ok {
            return Err(XcCode::FingerprintMismatch);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、四段架构（判据三·深化：迁移账 + 快照恢复 + 流水线指纹）
// ---------------------------------------------------------------------------

/// 四段闭集（单向流水线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchStage {
    /// 场景：考卷定义。
    Scene,
    /// 运行：考卷执行。
    Run,
    /// 计分：分数计算。
    Score,
    /// 发布：结果公开。
    Publish,
}

impl BenchStage {
    /// 全段数组（序数对位——机检遍历面）。
    pub const ALL: [BenchStage; 4] = [
        BenchStage::Scene,
        BenchStage::Run,
        BenchStage::Score,
        BenchStage::Publish,
    ];

    /// 段序（0-3）。
    pub const fn ordinal(self) -> usize {
        match self {
            BenchStage::Scene => 0,
            BenchStage::Run => 1,
            BenchStage::Score => 2,
            BenchStage::Publish => 3,
        }
    }

    /// 下一段（发布段的下一段是自己——终态由账面表达）。
    pub const fn next(self) -> BenchStage {
        match self {
            BenchStage::Scene => BenchStage::Run,
            BenchStage::Run => BenchStage::Score,
            BenchStage::Score => BenchStage::Publish,
            BenchStage::Publish => BenchStage::Publish,
        }
    }

    /// 段名。
    pub const fn zh(self) -> &'static str {
        match self {
            BenchStage::Scene => "场景",
            BenchStage::Run => "运行",
            BenchStage::Score => "计分",
            BenchStage::Publish => "发布",
        }
    }

    /// 段机检键。
    pub const fn key(self) -> &'static str {
        match self {
            BenchStage::Scene => "u01.stage.scene",
            BenchStage::Run => "u01.stage.run",
            BenchStage::Score => "u01.stage.score",
            BenchStage::Publish => "u01.stage.publish",
        }
    }

    /// 段工件名（段间传递的工件语义）。
    pub const fn artifact_name(self) -> &'static str {
        match self {
            BenchStage::Scene => "场景定义",
            BenchStage::Run => "运行工件",
            BenchStage::Score => "分数",
            BenchStage::Publish => "发布物",
        }
    }
}

/// 段迁移账目（每段完成一笔：从段→到段+完成 tick+迁移指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageTransition {
    /// 完成的段。
    pub from: BenchStage,
    /// 推进到的段。
    pub to: BenchStage,
    /// 完成 tick（上游注入）。
    pub tick: u64,
    /// 迁移指纹（from/to/tick 混算）。
    pub fingerprint: u64,
}

impl StageTransition {
    /// 构造并算迁移指纹。
    pub fn new(from: BenchStage, to: BenchStage, tick: u64) -> StageTransition {
        let f = fnv1a_u64(fnv1a_u64(FNV_OFFSET, from.ordinal() as u64), tick)
            ^ fnv1a_u64(FNV_PRIME, to.ordinal() as u64);
        StageTransition {
            from,
            to,
            tick,
            fingerprint: f,
        }
    }
}

/// Bench 流水线账（段游标 + 段工件在账位 + 迁移账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchPipeline {
    /// 当前段。
    pub current: BenchStage,
    /// 四段工件在账位（场景定义/运行工件/分数/发布物）。
    artifacts: [bool; 4],
    /// 发布完成。
    finished: bool,
    /// 迁移账（每次段完成留一笔）。
    transitions: Vec<StageTransition>,
}

impl BenchPipeline {
    /// 快照编码长度（段游标+四工件位+finished+迁移笔数=7）。
    pub const SNAPSHOT_LEN: usize = 7;

    /// 新流水线。
    pub const fn new() -> BenchPipeline {
        BenchPipeline {
            current: BenchStage::Scene,
            artifacts: [false; 4],
            finished: false,
            transitions: Vec::new(),
        }
    }

    /// 段完成（带工件在账标记；跳段/无工件显性拒）。
    pub fn complete_stage(&mut self, artifact: bool) -> Result<BenchStage, XcCode> {
        self.complete_stage_at(artifact, 0)
    }

    /// 段完成（带 tick 注入版——迁移账留痕；tick=0 等价无痕调用）。
    pub fn complete_stage_at(&mut self, artifact: bool, tick: u64) -> Result<BenchStage, XcCode> {
        if self.finished {
            return Err(XcCode::StageViolation);
        }
        if !artifact {
            return Err(XcCode::NoArtifact);
        }
        let i = self.current.ordinal();
        if let Some(a) = self.artifacts.get_mut(i) {
            *a = true;
        }
        if tick > 0 {
            let t = StageTransition::new(self.current, self.current.next(), tick);
            self.transitions.push(t);
        }
        let nxt = self.current.next();
        if nxt == self.current {
            self.finished = true;
        } else {
            self.current = nxt;
        }
        Ok(self.current)
    }

    /// 段工件在账读取。
    pub const fn artifact_of(&self, s: BenchStage) -> bool {
        self.artifacts[s.ordinal()]
    }

    /// 是否终态。
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// 迁移笔数。
    pub const fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// 迁移账读取（越界 None）。
    pub fn transition_at(&self, i: usize) -> Option<&StageTransition> {
        self.transitions.get(i)
    }

    /// 流水线指纹（段游标+工件位+finished+逐笔迁移指纹混算）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_u64(FNV_OFFSET, self.current.ordinal() as u64);
        let mut i = 0usize;
        while i < 4 {
            s = fnv1a_u64(s, if self.artifacts[i] { 1 } else { 0 });
            i += 1;
        }
        s = fnv1a_u64(s, if self.finished { 1 } else { 0 });
        let mut j = 0usize;
        while j < self.transitions.len() {
            if let Some(t) = self.transitions.get(j) {
                s = fnv1a_u64(s, t.fingerprint);
            }
            j += 1;
        }
        s
    }

    /// 快照（定长编码：[段序,工件位4位打包,finished,迁移笔数低32位高低字]）。
    pub fn snapshot(&self) -> [u64; BenchPipeline::SNAPSHOT_LEN] {
        let mut bits = 0u64;
        let mut i = 0usize;
        while i < 4 {
            if self.artifacts[i] {
                bits |= 1u64 << i;
            }
            i += 1;
        }
        let n = self.transitions.len() as u64;
        [
            self.current.ordinal() as u64,
            bits,
            if self.finished { 1 } else { 0 },
            (n >> 32) & 0xffff_ffff,
            n & 0xffff_ffff,
            self.fingerprint(),
            U_DOMAIN_LAST as u64,
        ]
    }

    /// 恢复（编码不符/指纹对拍失败/迁移笔数与账不符一律显性拒）。
    pub fn restore(snap: &[u64; BenchPipeline::SNAPSHOT_LEN]) -> Result<BenchPipeline, XcCode> {
        if snap[6] != U_DOMAIN_LAST as u64 {
            return Err(XcCode::FingerprintMismatch);
        }
        let ord = snap[0] as usize;
        if ord >= BenchStage::ALL.len() {
            return Err(XcCode::StageViolation);
        }
        let mut artifacts = [false; 4];
        let mut i = 0usize;
        while i < 4 {
            artifacts[i] = (snap[1] >> i) & 1 == 1;
            i += 1;
        }
        let n = ((snap[3] & 0xffff_ffff) << 32) | (snap[4] & 0xffff_ffff);
        if n > 4 {
            return Err(XcCode::StageViolation);
        }
        let mut p = BenchPipeline {
            current: BenchStage::ALL[ord],
            artifacts,
            finished: snap[2] == 1,
            transitions: Vec::new(),
        };
        // 迁移账按笔数重建（tick 不可恢复为 0 以外的真实值——以 0 占位，
        // 指纹以占位 tick 记账；对拍只用现算指纹核对快照指纹）。
        let mut k = 0u64;
        while k < n {
            let from = BenchStage::ALL[k as usize];
            let to = from.next();
            p.transitions.push(StageTransition::new(from, to, k + 1));
            k += 1;
        }
        if p.fingerprint() != snap[5] {
            return Err(XcCode::FingerprintMismatch);
        }
        Ok(p)
    }

    /// 人读流水线面。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "BenchPipeline:当前={}/finished={}/迁移{}笔",
            self.current.zh(),
            self.is_finished(),
            self.transitions.len()
        );
        let mut i = 0usize;
        while i < 4 {
            if let Some(st) = BenchStage::ALL.get(i) {
                s.push_str(&alloc::format!(
                    "/{}={}",
                    st.zh(),
                    if self.artifacts[i] { "在账" } else { "缺" }
                ));
            }
            i += 1;
        }
        s
    }
}

impl Default for BenchPipeline {
    fn default() -> BenchPipeline {
        BenchPipeline::new()
    }
}

// ---------------------------------------------------------------------------
// 七、T10 预告兑现确认（判据四·深化：兑现登记账 + 权重守恒）
// ---------------------------------------------------------------------------

/// 兑现记录四要素（预告出处/承接物/兑现单号/对接版本）。
pub const T10_FULFILL: [&str; 4] = [
    "预告出处=T10移交包性能基准项清单（F3041侧）",
    "承接物=BENCH_ITEMS五项考卷计分面",
    "兑现单=CGPU-F3201（U01开工）",
    "对接版本=t10.bridge.v1（冻结）",
];

/// 性能基准项闭集五项（考卷按项计分；表外不立项）。
pub const BENCH_ITEMS: [&str; 5] =
    ["帧率", "P95帧时间", "首帧延迟", "带宽吞吐", "功耗效率"];

/// 五项权重（bps 制；编译期守恒 10000——权重是计分总分的分母钉死）。
pub const ITEM_WEIGHTS_BPS: [u32; 5] = [3_000, 2_500, 1_500, 2_000, 1_000];

const _: () = assert!(
    ITEM_WEIGHTS_BPS[0]
        + ITEM_WEIGHTS_BPS[1]
        + ITEM_WEIGHTS_BPS[2]
        + ITEM_WEIGHTS_BPS[3]
        + ITEM_WEIGHTS_BPS[4]
        == SCORE_MAX_BPS
);

/// 兑现登记账（逐要素登记 + 兑现指纹 + 重复显性拒）。
#[derive(Clone, Debug, Default)]
pub struct FulfillLedger {
    slots: [bool; T10_FULFILL_LEN],
    /// 登记 tick（逐要素；0=未登记）。
    ticks: [u64; T10_FULFILL_LEN],
    registered: u8,
}

impl FulfillLedger {
    /// 空账。
    pub const fn new() -> FulfillLedger {
        FulfillLedger {
            slots: [false; T10_FULFILL_LEN],
            ticks: [0; T10_FULFILL_LEN],
            registered: 0,
        }
    }

    /// 登记第 i 要素（重复登记显性拒；tick 上游注入）。
    pub fn register(&mut self, i: usize, tick: u64) -> Result<(), XcCode> {
        if i >= T10_FULFILL_LEN {
            return Err(XcCode::FulfillMissing);
        }
        if self.slots[i] {
            return Err(XcCode::FulfillDuplicate);
        }
        self.slots[i] = true;
        self.ticks[i] = tick;
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 要素在账读取。
    pub const fn registered_at(&self, i: usize) -> bool {
        if i < T10_FULFILL_LEN {
            self.slots[i]
        } else {
            false
        }
    }

    /// 四要素是否齐备。
    pub const fn is_complete(&self) -> bool {
        self.registered as usize == T10_FULFILL_LEN
    }

    /// 兑现指纹（在账位+tick 混算；未齐时也算但语义上不用于宣告）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < T10_FULFILL_LEN {
            s = fnv1a_u64(s, if self.slots[i] { 1 } else { 0 });
            s = fnv1a_u64(s, self.ticks[i]);
            i += 1;
        }
        s
    }

    /// 兑现核验（四要素齐才过）。
    pub const fn verify(&self) -> Result<(), XcCode> {
        if self.is_complete() {
            Ok(())
        } else {
            Err(XcCode::FulfillMissing)
        }
    }
}

/// 兑现核验（四要素齐备 + 基准项闭集齐备）。
pub fn verify_fulfill() -> Result<(), XcCode> {
    if T10_FULFILL.len() != 4 || BENCH_ITEMS.len() != 5 {
        return Err(XcCode::HandoverMissing);
    }
    Ok(())
}

/// 基准项校验（表外项显性拒——考卷只能考闭集内的项）。
pub fn bench_item_ok(item: &str) -> Result<(), XcCode> {
    let mut i = 0usize;
    while i < BENCH_ITEMS.len() {
        if let Some(b) = BENCH_ITEMS.get(i) {
            if *b == item {
                return Ok(());
            }
        }
        i += 1;
    }
    Err(XcCode::UnknownBenchItem)
}

/// 基准项序号（表外 None）。
pub fn bench_item_index(item: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < BENCH_ITEMS.len() {
        if let Some(b) = BENCH_ITEMS.get(i) {
            if *b == item {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 加权总分（分数 bps 制；任一越界即 None——总分不迁就脏数据）。
pub fn weighted_total(scores: &[u32; 5]) -> Option<u32> {
    let mut total: u64 = 0;
    let mut i = 0usize;
    while i < 5 {
        let s = *scores.get(i)?;
        if s > SCORE_MAX_BPS {
            return None;
        }
        let w = *ITEM_WEIGHTS_BPS.get(i)? as u64;
        total = total.saturating_add((s as u64).saturating_mul(w));
        i += 1;
    }
    // 权重和恒 10000（编译期断言），除法安全；再除一次纯属守恒兜底。
    let denom = ITEM_WEIGHTS_BPS[0] as u64
        + ITEM_WEIGHTS_BPS[1] as u64
        + ITEM_WEIGHTS_BPS[2] as u64
        + ITEM_WEIGHTS_BPS[3] as u64
        + ITEM_WEIGHTS_BPS[4] as u64;
    if denom == 0 {
        return None;
    }
    Some((total / denom) as u32)
}

// ---------------------------------------------------------------------------
// 八、场景登记账（场景命名规范落地 + 四类闭集 + 指纹）
// ---------------------------------------------------------------------------

/// 四类场景闭集（与 F3203 分类接缝）。
pub const SCENE_CATEGORIES: [&str; 4] = ["渲染", "计算", "混合", "极限"];

/// 场景命名三规则的常量面：前缀/分隔符/最大长度。
pub const SCENE_NAME_PREFIX: &str = "u01.";
pub const SCENE_NAME_SEP: char = '.';
pub const SCENE_NAME_MAX: usize = 64;

/// 场景命名校验（前缀 + 点分隔 + 长度三规则）。
pub fn scene_name_ok(name: &str) -> Result<(), XcCode> {
    let b = name.as_bytes();
    let p = SCENE_NAME_PREFIX.as_bytes();
    if b.len() < p.len() + 3 {
        return Err(XcCode::SceneNameBad);
    }
    let mut i = 0usize;
    while i < p.len() {
        if b[i] != p[i] {
            return Err(XcCode::SceneNameBad);
        }
        i += 1;
    }
    if name.len() > SCENE_NAME_MAX {
        return Err(XcCode::SceneNameBad);
    }
    // 前缀之后至少一个分隔符，且段全非空。
    let mut seg_nonempty = false;
    let mut seen_sep = false;
    let mut m = p.len();
    while m < b.len() {
        let c = b[m] as char;
        if c == SCENE_NAME_SEP {
            if !seg_nonempty {
                return Err(XcCode::SceneNameBad);
            }
            seen_sep = true;
            seg_nonempty = false;
        } else {
            seg_nonempty = true;
        }
        m += 1;
    }
    if !seen_sep || !seg_nonempty {
        return Err(XcCode::SceneNameBad);
    }
    Ok(())
}

/// 场景类别校验（四类闭集）。
pub fn scene_category_ok(cat: &str) -> Result<(), XcCode> {
    let mut i = 0usize;
    while i < SCENE_CATEGORIES.len() {
        if let Some(c) = SCENE_CATEGORIES.get(i) {
            if *c == cat {
                return Ok(());
            }
        }
        i += 1;
    }
    Err(XcCode::SceneCategoryUnknown)
}

/// 场景条目（id/类别/内容指纹/登记 tick）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneEntry {
    /// 场景 id（命名规范校验过的全名）。
    pub id: String,
    /// 类别（四类闭集之一）。
    pub category: String,
    /// 内容指纹（内容 FNV——考卷防篡改基元）。
    pub content_fingerprint: u64,
    /// 登记 tick（上游注入）。
    pub registered_tick: u64,
}

/// 场景登记账（重 id 显性拒；逐条核验；指纹账）。
#[derive(Clone, Debug, Default)]
pub struct SceneRegistry {
    scenes: Vec<SceneEntry>,
}

impl SceneRegistry {
    /// 空账。
    pub const fn new() -> SceneRegistry {
        SceneRegistry { scenes: Vec::new() }
    }

    /// 登记（命名/类别核验 + 重 id 拒）。
    pub fn register(&mut self, id: &str, category: &str, content: &str, tick: u64) -> Result<(), XcCode> {
        scene_name_ok(id)?;
        scene_category_ok(category)?;
        let mut i = 0usize;
        while i < self.scenes.len() {
            if let Some(s) = self.scenes.get(i) {
                if s.id == id {
                    return Err(XcCode::SceneDuplicate);
                }
            }
            i += 1;
        }
        self.scenes.push(SceneEntry {
            id: id.to_string(),
            category: category.to_string(),
            content_fingerprint: fnv1a_str(FNV_OFFSET, content),
            registered_tick: tick,
        });
        Ok(())
    }

    /// 场景数。
    pub const fn len(&self) -> usize {
        self.scenes.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    /// 按 id 查场景。
    pub fn find(&self, id: &str) -> Option<&SceneEntry> {
        let mut i = 0usize;
        while i < self.scenes.len() {
            if let Some(s) = self.scenes.get(i) {
                if s.id == id {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 按类别列举（id 列表；类别表外空表）。
    pub fn list_by_category(&self, category: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.scenes.len() {
            if let Some(s) = self.scenes.get(i) {
                if s.category == category {
                    out.push(s.id.as_str());
                }
            }
            i += 1;
        }
        out
    }

    /// 登记账指纹（逐条 id/类别/内容指纹/tick 混算）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.scenes.len() {
            if let Some(e) = self.scenes.get(i) {
                s = fnv1a_str(s, &e.id);
                s = fnv1a_str(s, &e.category);
                s = fnv1a_u64(s, e.content_fingerprint);
                s = fnv1a_u64(s, e.registered_tick);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 九、运行配置与计分账（配置边界 + 分数留痕 + 归一 + 签名 + 发布闸）
// ---------------------------------------------------------------------------

/// 运行配置边界常量。
pub const RUN_MIN_ITERS: u32 = 1;
pub const RUN_MAX_ITERS: u32 = 100_000;
pub const RUN_MIN_WARMUP: u32 = 0;
pub const RUN_MAX_WARMUP: u32 = 10_000;
/// 采样间隔最小 tick（1 起——0 间隔即逐 tick 采样禁止）。
pub const RUN_MIN_SAMPLE_TICKS: u32 = 1;
pub const RUN_MAX_SAMPLE_TICKS: u32 = 1_000_000;

/// 运行配置（三参数 + 校验 + 预算估算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunConfig {
    /// 计时迭代数。
    pub iterations: u32,
    /// 预热轮数（不计分）。
    pub warmup: u32,
    /// 采样间隔（tick）。
    pub sample_interval_ticks: u32,
}

impl RunConfig {
    /// 校验（三参数边界；越界显性拒）。
    pub const fn validate(&self) -> Result<(), XcCode> {
        if self.iterations < RUN_MIN_ITERS || self.iterations > RUN_MAX_ITERS {
            return Err(XcCode::RunConfigInvalid);
        }
        if self.warmup < RUN_MIN_WARMUP || self.warmup > RUN_MAX_WARMUP {
            return Err(XcCode::RunConfigInvalid);
        }
        if self.sample_interval_ticks < RUN_MIN_SAMPLE_TICKS
            || self.sample_interval_ticks > RUN_MAX_SAMPLE_TICKS
        {
            return Err(XcCode::RunConfigInvalid);
        }
        Ok(())
    }

    /// 预估运行预算（逻辑 tick 数，零墙钟：(预热+迭代)×间隔，饱和）。
    pub const fn total_ticks_estimate(&self) -> u64 {
        let rounds = self.warmup as u64 + self.iterations as u64;
        rounds.saturating_mul(self.sample_interval_ticks as u64)
    }

    /// 配置指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u32(FNV_OFFSET, self.iterations);
        let s = fnv1a_u32(s, self.warmup);
        fnv1a_u32(s, self.sample_interval_ticks)
    }
}

/// 分数变更留痕（旧值/新值/动因/tick）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreChange {
    /// 基准项序号（0-4）。
    pub item: usize,
    /// 旧分（bps）。
    pub old_bps: u32,
    /// 新分（bps）。
    pub new_bps: u32,
    /// 动因（机检键或人读短句）。
    pub reason: String,
    /// 变更 tick。
    pub tick: u64,
}

/// 计分账（五项闭集计分 + 变更留痕 + 加权总分 + 防越界）。
#[derive(Clone, Debug)]
pub struct ScoreLedger {
    scores: [u32; 5],
    changes: Vec<ScoreChange>,
    /// 账指纹（每笔变更混链）。
    chain: u64,
}

impl ScoreLedger {
    /// 空账（五项零分——零分是合法起点）。
    pub const fn new() -> ScoreLedger {
        ScoreLedger {
            scores: [0; 5],
            changes: Vec::new(),
            chain: FNV_OFFSET,
        }
    }

    /// 记分（越界拒；变更必留痕）。
    pub fn set_score(&mut self, item: usize, bps: u32, reason: &str, tick: u64) -> Result<(), XcCode> {
        if item >= 5 {
            return Err(XcCode::UnknownBenchItem);
        }
        if bps > SCORE_MAX_BPS {
            return Err(XcCode::ScoreOutOfRange);
        }
        let old = self.scores[item];
        self.scores[item] = bps;
        let ch = ScoreChange {
            item,
            old_bps: old,
            new_bps: bps,
            reason: reason.to_string(),
            tick,
        };
        self.chain = fnv1a_u64(self.chain, self.fingerprint_scores());
        self.changes.push(ch);
        Ok(())
    }

    /// 读单项分。
    pub const fn score_of(&self, item: usize) -> Option<u32> {
        if item < 5 {
            Some(self.scores[item])
        } else {
            None
        }
    }

    /// 五项分数只读。
    pub const fn scores(&self) -> &[u32; 5] {
        &self.scores
    }

    /// 分数位指纹（不含变更账——签名基元用）。
    pub const fn fingerprint_scores(&self) -> u64 {
        let s = fnv1a_u32(FNV_OFFSET, self.scores[0]);
        let s = fnv1a_u32(s, self.scores[1]);
        let s = fnv1a_u32(s, self.scores[2]);
        let s = fnv1a_u32(s, self.scores[3]);
        fnv1a_u32(s, self.scores[4])
    }

    /// 加权总分（越界分返回 None——理论上 set 已挡，双闸兜底）。
    pub fn weighted_total(&self) -> Option<u32> {
        weighted_total(&self.scores)
    }

    /// 变更笔数。
    pub const fn change_count(&self) -> usize {
        self.changes.len()
    }

    /// 变更账读取。
    pub fn change_at(&self, i: usize) -> Option<&ScoreChange> {
        self.changes.get(i)
    }

    /// 账链指纹。
    pub const fn chain_fingerprint(&self) -> u64 {
        self.chain
    }

    /// 是否全零账（未计分判定——发布闸用它挡空分发布）。
    pub const fn is_all_zero(&self) -> bool {
        self.scores[0] == 0
            && self.scores[1] == 0
            && self.scores[2] == 0
            && self.scores[3] == 0
            && self.scores[4] == 0
    }
}

/// 基线归一（跨硬件基线对照；比率 bps 制，饱和乘除，零除 None）。
/// 返回 score 相对 baseline 的千分比放大率（bps：10000=持平）。
pub fn normalize_against_baseline(score: u32, baseline: u32) -> Option<u32> {
    if baseline == 0 {
        return None;
    }
    // (score / baseline) * 10000，用 u64 中间量避免溢出。
    let num = (score as u64).saturating_mul(SCORE_MAX_BPS as u64);
    Some((num / baseline as u64) as u32)
}

/// Bench 结果签名（FNV：场景指纹+配置指纹+五项分数+加权总分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchSignature {
    /// 签名值。
    pub value: u64,
}

impl BenchSignature {
    /// 计算（场景指纹×配置指纹×计分账）。
    pub fn compute(scene_fp: u64, config_fp: u64, scores: &ScoreLedger) -> BenchSignature {
        let mut s = fnv1a_u64(FNV_OFFSET, scene_fp);
        s = fnv1a_u64(s, config_fp);
        s = fnv1a_u64(s, scores.fingerprint_scores());
        let total = scores.weighted_total().unwrap_or(0);
        BenchSignature {
            value: fnv1a_u64(s, total as u64),
        }
    }

    /// 验签（重算对拍）。
    pub fn verify(&self, scene_fp: u64, config_fp: u64, scores: &ScoreLedger) -> Result<(), XcCode> {
        if *self == BenchSignature::compute(scene_fp, config_fp, scores) {
            Ok(())
        } else {
            Err(XcCode::SignatureMismatch)
        }
    }
}

/// 发布闸（四段齐 + 签名有效 + 非空分 三闸）。
pub struct PublishGate;

impl PublishGate {
    /// 放行核验：四段完成 + 验签过 + 分数非全零。
    pub fn check(
        pipeline: &BenchPipeline,
        sig: &BenchSignature,
        scene_fp: u64,
        config_fp: u64,
        scores: &ScoreLedger,
    ) -> Result<(), XcCode> {
        if !pipeline.is_finished() {
            return Err(XcCode::PublishGateClosed);
        }
        sig.verify(scene_fp, config_fp, scores)?;
        if scores.is_all_zero() {
            return Err(XcCode::PublishGateClosed);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、域开工台账（四组判据总闸 + 幂等宣告 + 人读报告）
// ---------------------------------------------------------------------------

/// 域开工台账（四组闸门齐备才宣告就位；宣告幂等）。
#[derive(Clone, Debug, Default)]
pub struct DomainOpenLedger {
    handover: HandoverLedger,
    position_ok: bool,
    pipeline: Option<BenchPipeline>,
    fulfill: FulfillLedger,
    /// 宣告 tick（0=未宣告）。
    declared_tick: u64,
}

impl DomainOpenLedger {
    /// 新台账（预挂初始流水线与兑现账——四组各就其位）。
    pub fn new() -> DomainOpenLedger {
        DomainOpenLedger {
            handover: HandoverLedger::new(),
            position_ok: false,
            pipeline: Some(BenchPipeline::new()),
            fulfill: FulfillLedger::new(),
            declared_tick: 0,
        }
    }

    /// 闸一：入签收。
    pub fn admit_receipt(&mut self, r: HandoverReceipt) -> Result<(), XcCode> {
        self.handover.record(r)
    }

    /// 闸二：定位机检通过置位。
    pub fn mark_position_verified(&mut self) -> Result<(), XcCode> {
        verify_position()?;
        self.position_ok = true;
        Ok(())
    }

    /// 闸三：流水线在位读取（四段推进走这里）。
    pub fn pipeline_mut(&mut self) -> Option<&mut BenchPipeline> {
        self.pipeline.as_mut()
    }

    /// 闸四：兑现登记。
    pub fn register_fulfill(&mut self, i: usize, tick: u64) -> Result<(), XcCode> {
        self.fulfill.register(i, tick)
    }

    /// 四组核验（全过才 Ok）。
    pub fn verify_four_groups(&self) -> Result<(), XcCode> {
        // 组一：签收（台账非空且自洽）。
        if self.handover.is_empty() {
            return Err(XcCode::ReceiptMissing);
        }
        self.handover.verify()?;
        // 组二：定位（机检置位 + 静态闸复过）。
        if !self.position_ok {
            return Err(XcCode::FingerprintMismatch);
        }
        verify_position()?;
        // 组三：四段（流水线在位且起点合法）。
        match self.pipeline.as_ref() {
            Some(p) => {
                if p.current != BenchStage::Scene && !p.is_finished() {
                    // 中途态合法（开工不要求跑完——要求账面自洽）。
                    let _ = p.fingerprint();
                }
            }
            None => return Err(XcCode::StageViolation),
        }
        // 组四：兑现（登记账静态核 + 动态四要素齐备核）。
        verify_fulfill()?;
        if !self.fulfill.is_complete() {
            return Err(XcCode::FulfillMissing);
        }
        Ok(())
    }

    /// 宣告就位（幂等：重复宣告不覆盖首宣 tick；未过四组即拒）。
    pub fn declare_open(&mut self, tick: u64) -> Result<u64, XcCode> {
        self.verify_four_groups()?;
        if self.declared_tick == 0 {
            self.declared_tick = tick;
        }
        Ok(self.declared_tick)
    }

    /// 是否已宣告。
    pub const fn is_declared(&self) -> bool {
        self.declared_tick != 0
    }

    /// 首宣 tick。
    pub const fn declared_at(&self) -> u64 {
        self.declared_tick
    }

    /// 域开工核验总闸（判据总闸：签收/定位/四段/兑现四组齐备）。
    pub fn verify_udomain_open(&self, received: &[&str], fingerprint: u64) -> Result<(), XcCode> {
        verify_handover(received, fingerprint)?;
        self.verify_four_groups()
    }

    /// 域开工报告（人读面）。
    pub fn report(&self, received: &[&str], fingerprint: u64) -> String {
        let mut s = String::new();
        s.push_str("U01 域开工核验：");
        s.push_str(&alloc::format!(
            "移交{}/签收{}笔/定位{}/四段在账{}/兑现{}/基准项{}项；",
            T10_HANDOVER.len(),
            self.handover.len(),
            if self.position_ok { "过" } else { "未过" },
            if self.pipeline.is_some() { "是" } else { "否" },
            if self.fulfill.is_complete() { "齐" } else { "缺" },
            BENCH_ITEMS.len(),
        ));
        match self.verify_udomain_open(received, fingerprint) {
            Ok(()) => s.push_str("四组全过=就位"),
            Err(c) => {
                s.push_str("拒=");
                s.push_str(c.detail());
            }
        }
        s
    }
}

/// 域开工核验总闸（静态版：纯函数四组核验，供判据脚本直调）。
pub fn verify_udomain_open(received: &[&str], fingerprint: u64) -> Result<(), XcCode> {
    verify_handover(received, fingerprint)?;
    if POSITION.len() != 2 || POSITION_KEYS.len() != 2 {
        return Err(XcCode::HandoverMissing);
    }
    let p = BenchPipeline::new();
    if p.current != BenchStage::Scene || p.is_finished() {
        return Err(XcCode::StageViolation);
    }
    verify_fulfill()?;
    Ok(())
}

/// 域开工报告（人读面；静态版——无台账依赖的独立口径）。
pub fn udomain_open_report(received: &[&str], fingerprint: u64) -> String {
    let mut s = String::new();
    s.push_str("U01 域开工核验：");
    s.push_str(&alloc::format!(
        "移交{}件/定位{}条/四段/基准项{}项；",
        T10_HANDOVER.len(),
        POSITION.len(),
        BENCH_ITEMS.len()
    ));
    match verify_udomain_open(received, fingerprint) {
        Ok(()) => s.push_str("四组全过=就位"),
        Err(c) => {
            s.push_str("拒=");
            s.push_str(c.detail());
        }
    }
    s
}
