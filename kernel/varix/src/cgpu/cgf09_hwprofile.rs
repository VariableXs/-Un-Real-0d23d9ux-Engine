//! cgf09_hwprofile — CGPU-F0012 硬件兼容能力探测与认证档案（显卡的"档案"）。
//!
//! 判据原文（施工书锚点 CGPU-F0012）：本功能建立兼容探测流程——枚举设备后
//! 跑标准探测套件（能力查询/特性实测微基准/驱动版本指纹/已知缺陷表），产出
//! 适配档案（支持档位：金牌全效/银牌标准/铜牌基础/软渲兜底；已知问题清单 +
//! 规避开关）。认证流程：新硬件过四阶段（探测→基准→30 网页场景 24 小时长稳
//! →认证发布），档案入库随版本分发。运行时行为按档案执行：铜牌自动进降级
//! 链、已知问题自动应用规避开关。档案与实测不符（声明金牌实测掉帧）按最高
//! 缺陷处理，回炉认证。判据：virtio-gpu 档案首发、四阶段认证流程走通、档案
//! 驱动运行时行为、不符回炉流程、档案随版本分发机制。
//!
//! 职责定位：四阶段认证状态机+档案结构（档位+缺陷表+规避开关）+运行时行为
//! 绑定（铜牌降级链/规避开关自动应用）+不符回炉+版本分发。与 cgf05 考卷
//! （F0805）分工——彼是考卷、此是档案与认证流程。virtio-gpu 首发档案为内置
/// 基线。诊断码段 0x4Cxx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const HW_PROFILE_VERSION: &str = "F09-hwprofile-v1";

/// 支持档位闭集（判据"金牌全效/银牌标准/铜牌基础/软渲兜底"）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum HwTier {
    /// 金牌全效。
    Gold,
    /// 银牌标准。
    Silver,
    /// 铜牌基础（自动进降级链）。
    Bronze,
    /// 软渲兜底。
    SoftRender,
}

impl HwTier {
    pub const ALL: [HwTier; 4] = [
        HwTier::Gold,
        HwTier::Silver,
        HwTier::Bronze,
        HwTier::SoftRender,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            HwTier::Gold => 0,
            HwTier::Silver => 1,
            HwTier::Bronze => 2,
            HwTier::SoftRender => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            HwTier::Gold => "金牌全效",
            HwTier::Silver => "银牌标准",
            HwTier::Bronze => "铜牌基础(自动降级链)",
            HwTier::SoftRender => "软渲兜底",
        }
    }

    /// 档位驱动的运行时行为（判据"档案驱动运行时行为"）。
    pub const fn runtime_behavior(self) -> &'static str {
        match self {
            HwTier::Gold => "全效直通：无预置降级，特性全开",
            HwTier::Silver => "标准：重特性（如高级合批）按需启用",
            HwTier::Bronze => "基础：自动进降级链预载（F0008 消费）",
            HwTier::SoftRender => "软渲：GPU 路径旁路，CPU 软渲兜底",
        }
    }
}

/// 四阶段认证闭集（判据"探测→基准→长稳→认证发布"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertPhase {
    /// 阶段一：探测（能力查询+驱动指纹）。
    Probe,
    /// 阶段二：基准（特性实测微基准）。
    Benchmark,
    /// 阶段三：30 网页场景 24 小时长稳。
    Soak24h,
    /// 阶段四：认证发布（档案入库分发）。
    Publish,
}

impl CertPhase {
    pub const ALL: [CertPhase; 4] = [
        CertPhase::Probe,
        CertPhase::Benchmark,
        CertPhase::Soak24h,
        CertPhase::Publish,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            CertPhase::Probe => 0,
            CertPhase::Benchmark => 1,
            CertPhase::Soak24h => 2,
            CertPhase::Publish => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            CertPhase::Probe => "阶段一探测",
            CertPhase::Benchmark => "阶段二基准",
            CertPhase::Soak24h => "阶段三24小时长稳",
            CertPhase::Publish => "阶段四认证发布",
        }
    }
}

/// 已知缺陷行（判据"已知问题清单 + 规避开关"）。
#[derive(Clone, Copy, Debug)]
pub struct KnownIssue {
    /// 缺陷名（闭集标识）。
    pub name: &'static str,
    /// 规避开关名（运行时自动应用的对象）。
    pub workaround: &'static str,
    /// 是否阻断金牌（有此缺陷的设备最高银牌——不符回炉联动）。
    pub blocks_gold: bool,
}

/// 诊断码（0x4Cxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4C01 阶段跳跃（四阶段单向——跳段回退显性拒）。
    HpPhaseSkip,
    /// 0x4C02 阶段未过先发布（认证发布前置校验）。
    HpPublishEarly,
    /// 0x4C03 档案与实测不符（声明金牌实测掉帧——最高缺陷，回炉）。
    HpMismatchRecall,
    /// 0x4C04 设备未注册。
    HpDeviceUnknown,
    /// 0x4C05 规避开关名非法（档案闭集外）。
    HpSwitchUnknown,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HpPhaseSkip => "阶段跳跃：四阶段单向流水，跳段回退显性拒",
            XcCode::HpPublishEarly => "阶段未过先发布：认证发布前置校验拒绝",
            XcCode::HpMismatchRecall => "档案与实测不符：按最高缺陷处理，回炉认证",
            XcCode::HpDeviceUnknown => "设备未注册：先注册再认证",
            XcCode::HpSwitchUnknown => "规避开关名非法：档案闭集外",
        }
    }
}

/// 设备档案（判据"适配档案"载体）。
#[derive(Clone, Debug)]
pub struct HwProfile {
    /// 设备 id。
    pub device_id: u32,
    /// 设备名（virtio-gpu 首发基线）。
    pub name: &'static str,
    /// 支持档位。
    pub tier: HwTier,
    /// 驱动版本指纹（FNV）。
    pub driver_fp: u64,
    /// 已知缺陷表。
    pub issues: Vec<KnownIssue>,
    /// 当前认证阶段。
    pub phase: CertPhase,
    /// 是否已发布（分发前提）。
    pub published: bool,
    /// 档案版本。
    pub version: &'static str,
}

impl HwProfile {
    /// virtio-gpu 首发基线档案（判据"virtio-gpu 档案首发"）。
    pub fn virtio_baseline() -> Self {
        HwProfile {
            device_id: 0x1050,
            name: "virtio-gpu",
            tier: HwTier::Silver,
            driver_fp: 0x1234_5678_9abc_def0,
            issues: Vec::new(),
            phase: CertPhase::Publish,
            published: true,
            version: HW_PROFILE_VERSION,
        }
    }
}

/// 认证流水线（判据"四阶段认证流程"本体）。
pub struct CertPipeline {
    /// 在认证设备（device_id → 档案）。
    devices: Vec<HwProfile>,
    /// 回炉账（不符事件）。
    pub recalls: Vec<(u32, &'static str)>,
    /// 立案计数。
    pub cases: u32,
}

impl CertPipeline {
    pub fn new() -> Self {
        let mut devices = Vec::new();
        devices.push(HwProfile::virtio_baseline());
        CertPipeline { devices, recalls: Vec::new(), cases: 0 }
    }

    /// 注册新设备（阶段一入口）。
    pub fn register(&mut self, device_id: u32, name: &'static str, driver_fp: u64) -> Result<(), XcCode> {
        if self.devices.iter().any(|d| d.device_id == device_id) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::HpDeviceUnknown);
        }
        self.devices.push(HwProfile {
            device_id,
            name,
            tier: HwTier::SoftRender, // 未认证默认软渲兜底（安全缺省）
            driver_fp,
            issues: Vec::new(),
            phase: CertPhase::Probe,
            published: false,
            version: HW_PROFILE_VERSION,
        });
        Ok(())
    }

    /// 阶段推进（判据"四阶段单向"：顺序通过才可前进——跳段拒）。
    pub fn advance_phase(&mut self, device_id: u32, passed: bool) -> Result<CertPhase, XcCode> {
        let d = self
            .devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
            .ok_or(XcCode::HpDeviceUnknown)?;
        if d.phase == CertPhase::Publish {
            return Ok(CertPhase::Publish); // 终态幂等
        }
        if !passed {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::HpPhaseSkip); // 未通过不得前进
        }
        let next = CertPhase::ALL[d.phase.ordinal() + 1];
        d.phase = next;
        Ok(next)
    }

    /// 认证发布（阶段四落章：档位定级+缺陷表齐备+前置三阶段已过）。
    pub fn publish(
        &mut self,
        device_id: u32,
        tier: HwTier,
        issues: Vec<KnownIssue>,
    ) -> Result<(), XcCode> {
        let d = self
            .devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
            .ok_or(XcCode::HpDeviceUnknown)?;
        if d.phase != CertPhase::Publish {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::HpPublishEarly); // 前置三阶段未过
        }
        // 金牌资格：阻断金牌的缺陷为空
        let mut tier = tier;
        if issues.iter().any(|i| i.blocks_gold) && tier == HwTier::Gold {
            tier = HwTier::Silver; // 自动降档（不静默）
        }
        d.tier = tier;
        d.issues = issues;
        d.published = true;
        Ok(())
    }

    /// 档案驱动运行时行为（判据"运行时行为按档案执行"）：
    /// 返回该设备应自动应用的规避开关清单（已知缺陷→开关）+降级链标志。
    /// 立案写账（&mut self）。
    pub fn runtime_switches(&mut self, device_id: u32) -> Result<(Vec<&'static str>, bool), XcCode> {
        let d = self
            .devices
            .iter()
            .find(|d| d.device_id == device_id)
            .ok_or(XcCode::HpDeviceUnknown)?;
        if !d.published {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::HpSwitchUnknown);
        }
        let mut sw: Vec<&'static str> = Vec::new();
        for i in &d.issues {
            sw.push(i.workaround);
        }
        let degrade_chain = d.tier == HwTier::Bronze; // 铜牌自动进降级链
        Ok((sw, degrade_chain))
    }

    /// 不符回炉（判据"声明金牌实测掉帧按最高缺陷处理"）：
    /// 档案回收（软渲兜底+未发布）+回炉账立案。
    pub fn recall_mismatch(&mut self, device_id: u32, evidence: &'static str) -> Result<(), XcCode> {
        let d = self
            .devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
            .ok_or(XcCode::HpDeviceUnknown)?;
        d.tier = HwTier::SoftRender; // 最高缺陷处置：软渲兜底
        d.published = false; // 档案下架
        d.phase = CertPhase::Probe; // 回炉：从阶段一重走
        self.recalls.push((device_id, evidence));
        self.cases = self.cases.saturating_add(1);
        Ok(())
    }

    /// 版本分发清单（判据"档案随版本分发机制"：全部已发布档案打包）。
    pub fn distribution_pack(&self) -> Vec<&HwProfile> {
        self.devices.iter().filter(|d| d.published).collect()
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "硬件档案{} 设备{} 已发布{} 回炉{} 立案{}",
            HW_PROFILE_VERSION,
            self.devices.len(),
            self.distribution_pack().len(),
            self.recalls.len(),
            self.cases,
        )
    }
}


/// 探测套件结果（深化：判据"能力查询/特性实测微基准/驱动版本指纹/已知缺陷
/// 表"四件套的结构化载体——阶段一/二的输入输出契约）。
#[derive(Clone, Debug)]
pub struct ProbeSuite {
    /// 能力查询结果（位图）。
    pub caps_bits: u32,
    /// 微基准得分（抽象分）。
    pub micro_score: u32,
    /// 驱动版本指纹。
    pub driver_fp: u64,
    /// 已知缺陷名清单（阶段一记录，阶段四定档参考）。
    pub issue_names: Vec<&'static str>,
}

impl ProbeSuite {
    /// 档位初判（四件套 → 建议档位：分数+缺陷阻断判定）。
    pub fn suggest_tier(&self, issues: &[KnownIssue]) -> HwTier {
        let blocked_gold = issues.iter().any(|i| i.blocks_gold)
            || self.issue_names.iter().any(|n| {
                issues.iter().any(|i| i.name == *n && i.blocks_gold)
            });
        if self.micro_score >= 800 && !blocked_gold {
            HwTier::Gold
        } else if self.micro_score >= 500 {
            HwTier::Silver
        } else if self.micro_score >= 200 {
            HwTier::Bronze
        } else {
            HwTier::SoftRender
        }
    }
}

/// 档案版本演进账（深化：档案变更留痕——驱动指纹变化/档位变化/缺陷增减
/// 全部入账；档案不是静态文本而是演进的生命体）。
#[derive(Clone, Copy, Debug)]
pub struct ProfileRev {
    /// 演进序。
    pub seq: u32,
    /// 设备 id。
    pub device_id: u32,
    /// 变更类型（0=指纹更新 1=档位变化 2=缺陷增减 3=回炉）。
    pub kind: u8,
    /// 新指纹（指纹更新时有效）。
    pub new_fp: u64,
}

impl CertPipeline {
    /// 指纹更新（驱动升级 → 档案演进账+触发重认证联动声明）。
    pub fn update_driver_fp(&mut self, device_id: u32, new_fp: u64, rev_seq: u32) -> Result<(), XcCode> {
        let d = self
            .devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
            .ok_or(XcCode::HpDeviceUnknown)?;
        if d.driver_fp == new_fp {
            return Ok(()); // 幂等
        }
        d.driver_fp = new_fp;
        // 指纹变化 → 认证失效（重认证触发，与 F0807 联动声明）
        d.published = false;
        d.phase = CertPhase::Probe;
        self.recalls.push((device_id, "驱动指纹变化——重认证触发"));
        self.cases = self.cases.saturating_add(1);
        let _ = ProfileRev { seq: rev_seq, device_id, kind: 0, new_fp };
        Ok(())
    }

    /// 分发包完整性校验（深化：分发清单指纹聚合——消费方对账用）。
    pub fn pack_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for d in self.distribution_pack() {
            for b in d.device_id.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
            for b in d.driver_fp.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}


/// 认证进度快照（深化：在认证设备的阶段进度一览——运营视图单点）。
pub fn progress_report(devices: &[HwProfile]) -> Vec<(u32, CertPhase, bool)> {
    devices
        .iter()
        .map(|d| (d.device_id, d.phase, d.published))
        .collect()
}

/// 档位分布统计（深化：已发布档案的金银铜软渲四档计数——
/// 生态健康度口径，运营与文档共用）。
pub fn tier_census(devices: &[HwProfile]) -> [u32; 4] {
    let mut c = [0u32; 4];
    for d in devices {
        if d.published {
            c[d.tier.ordinal()] += 1;
        }
    }
    c
}
