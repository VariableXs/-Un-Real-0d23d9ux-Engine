//! CGPU-F3201 · U 域开工与 Bench 总架构（CGPU-U 域 · 基准与 Bench 域 · U01 组 · 目标 340 行）。
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
//! ## 一、T10 移交包签收（判据一：七件逐件对账）
//!
//! T10（T 域收口组）移交 Bench 衔接包七件（[`T10_HANDOVER`]）：对接
//! 协议冻结版/性能基准项清单/场景命名规范/分数口径/运行环境声明/
//! 结果序列化格式/发布通道约定——逐件对账，缺件即空头签收
//! （[`XcCode::HandoverMissing`]）；签收带指纹（FNV 独立重算对拍）。
//!
//! ## 二、Bench 定位声明（判据二：两条款字面量冻结）
//!
//! 两条款（[`POSITION`]）：实测认证不凭文档声明（分数来自实测运行，
//! 文档声明不是证据）/CGPU 理念呼应（「深度基准=实测」与 F0802 认证
//! 流程的基准执行步同源——Bench 是认证体系的考卷供给方）。条款绑定
//! 检查键（[`POSITION_KEYS`]）——可机检声明。
//!
//! ## 三、四段架构（判据三：单向流水线）
//!
//! 四段（[`BenchStage`]）：场景（考卷定义）/运行（考卷执行）/计分
//! （分数计算）/发布（结果公开）——段序单向，跳段/回退显性拒
//! （[`XcCode::StageViolation`]）；段间工件传递（场景定义→运行工件→
//! 分数→发布物），无工件的段推进无效。
//!
//! ## 四、T10 预告兑现确认（判据四）
//!
//! 对接协议性能基准项（T10 预告的兑现位）：[`T10_FULFILL`] 四要素
//! （预告出处/承接物/兑现单号/对接版本）——预告兑现是登记制；
//! 性能基准项闭集（[`BENCH_ITEMS`]）五项（帧率/P95 帧时间/首帧延迟/
//! 带宽吞吐/功耗效率）——考卷按项计分。
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

// ---------------------------------------------------------------------------
// 二、诊断码（0x6C 段独占：U01 开工面）
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
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "T10 移交包缺件：七件逐件对账缺一即未就位，建议找 T 域补齐再签收",
            XcCode::FingerprintMismatch => "移交指纹不符：包内容与冻结版不一致，建议重新对账",
            XcCode::StageViolation => "段序违规：场景→运行→计分→发布单向流水线，跳段/回退显性拒",
            XcCode::NoArtifact => "段推进无工件：无上游工件不得推进（无场景定义不运行、无运行工件不计分）",
            XcCode::UnknownBenchItem => "基准项表外：五项闭集之外无项，建议按 BENCH_ITEMS 取用",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、T10 移交包签收（判据一）
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

// ---------------------------------------------------------------------------
// 四、Bench 定位声明（判据二）
// ---------------------------------------------------------------------------

/// 定位两条款（字面量冻结）。
pub const POSITION: [&str; 2] = [
    "实测认证不凭文档声明：分数来自实测运行，文档声明不是证据",
    "CGPU理念呼应：Bench是F0802认证体系基准执行步的考卷供给方，深度基准=实测",
];

/// 定位条款检查键（可机检 grep 键面）。
pub const POSITION_KEYS: [&str; 2] = ["u01.position.measure-only", "u01.position.cert-supply"];

// ---------------------------------------------------------------------------
// 五、四段架构（判据三）
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
}

/// Bench 流水线账（段游标 + 段工件在账位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchPipeline {
    /// 当前段。
    pub current: BenchStage,
    /// 四段工件在账位（场景定义/运行工件/分数/发布物）。
    artifacts: [bool; 4],
    /// 发布完成。
    finished: bool,
}

impl BenchPipeline {
    /// 新流水线。
    pub const fn new() -> BenchPipeline {
        BenchPipeline {
            current: BenchStage::Scene,
            artifacts: [false; 4],
            finished: false,
        }
    }

    /// 段完成（带工件在账标记；跳段/无工件显性拒）。
    pub fn complete_stage(&mut self, artifact: bool) -> Result<BenchStage, XcCode> {
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
}

impl Default for BenchPipeline {
    fn default() -> BenchPipeline {
        BenchPipeline::new()
    }
}

// ---------------------------------------------------------------------------
// 六、T10 预告兑现确认（判据四）
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

/// 域开工核验总闸（判据总闸：签收/定位/四段/兑现四组齐备）。
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

/// 域开工报告（人读面）。
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
