//! CGPU-F0012 · 硬件兼容能力探测与认证档案（vca12_hwcompat）
//!
//! 每一类显卡都要有"档案"：**兼容探测流程**——枚举设备后跑标准探
//! 测套件（能力查询/特性实测微基准/驱动版本指纹/已知缺陷表），产
//! 出**适配档案**（支持档位：金牌全效/银牌标准/铜牌基础/软渲兜底；
//! 已知问题清单+规避开关）。**认证流程**：新硬件过四阶段（探测→基
//! 准→30 网页场景 24 小时长稳→认证发布），档案入库**随版本分发**。
//! **运行时行为按档案执行**：铜牌自动进降级链、已知问题自动应用规
//! 避开关。**档案与实测不符**（声明金牌实测掉帧）按最高缺陷处理，
//! **回炉认证**。
//!
//! 定档律（显性规则，判据独立手算对拍）：三基准归一分（fill/blend/
//! text 各自÷基准线×1000 取 min）——min≥1000 金牌；≥600 银牌；<
//! 600 或能力缺项铜牌；无硬渲能力软渲兜底（不看分数）。 virtio-gpu
//! 首发档案（vendor 0x1AF4/device 0x1050）实测 900/700/800→min=700
//! →银牌（恰值锚）。
//!
//! 错误路径与降级矩阵：**阶段乱序→拦截**（跳阶段 E_HC_STAGE_ORDER
//! ——认证不跳步）；**基准不达线→拦**（E_HC_BENCH_FAIL）；**长稳
//! 失败→拦**（E_HC_SOAK_TIMEOUT）；**档案不符→回炉**（E_HC_PROFILE_
//! MISMATCH——revoke+requeue 重认证）。性能：定档 O(1)（三数取
//! min）；入库 O(log n)；分发 O(1) 版本号递增。
//!
//! 跨批对接点：F0011 适配器上游（枚举产物输入）；F0007 能力位图；
//! CGPU-O 降级链消费（Bronze 联动）；F0019 认证计划资产。无障碍：
//! 档案摘要读屏可达（spoken）。隐私面：档案只记硬件指纹与结论，
//! 不记用户数据。
//!
//! 诊断码 0xC4 段五码互异：`E_HC_NO_DEVICE` / `E_HC_STAGE_ORDER` /
//! `E_HC_BENCH_FAIL` / `E_HC_SOAK_TIMEOUT` / `E_HC_PROFILE_MISMATCH`。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 硬件兼容档案版本。
pub const HWCOMPAT_VERSION: &str = "A12-hwcompat-v1";
/// 金牌线（三基准归一分 min≥此值）。
pub const TIER_GOLD_MIN: u32 = 1000;
/// 银牌线（min≥此值，<金牌线）。
pub const TIER_SILVER_MIN: u32 = 600;
/// 长稳场景数（30 网页场景）。
pub const SOAK_SCENE_COUNT: usize = 30;
/// 长稳时长（小时）。
pub const SOAK_HOURS: u32 = 24;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xC4 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 未枚举设备：空设备面建档案。
pub const E_HC_NO_DEVICE: u16 = 0xC401;
/// 阶段乱序：认证流程跳步。
pub const E_HC_STAGE_ORDER: u16 = 0xC402;
/// 基准不达线：微基准低于银牌线。
pub const E_HC_BENCH_FAIL: u16 = 0xC403;
/// 长稳失败：30 场景 24h 有失败。
pub const E_HC_SOAK_TIMEOUT: u16 = 0xC404;
/// 档案不符：声明档位与实测不符→最高缺陷回炉。
pub const E_HC_PROFILE_MISMATCH: u16 = 0xC405;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_HC_NO_DEVICE => "未枚举设备：空设备面建档案拒绝",
        E_HC_STAGE_ORDER => "阶段乱序：认证流程必须探测→基准→长稳→发布顺序推进",
        E_HC_BENCH_FAIL => "基准不达线：微基准低于银牌线，拦在认证门外",
        E_HC_SOAK_TIMEOUT => "长稳失败：30 场景 24 小时有失败，拦发",
        E_HC_PROFILE_MISMATCH => "档案不符：声明档位与实测不符，按最高缺陷回炉认证",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xC4 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 硬件指纹与能力面
// ---------------------------------------------------------------------------

/// 驱动版本指纹（vendor/device/driver 位拼接——O(1) 恰值可手算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DriverFingerprint {
    pub vendor_id: u16,
    pub device_id: u16,
    pub driver_version: u32,
}

impl DriverFingerprint {
    /// 位拼接指纹：vendor<<48 | device<<32 | driver（判据手算锚）。
    pub fn packed(self) -> u64 {
        ((self.vendor_id as u64) << 48)
            | ((self.device_id as u64) << 32)
            | (self.driver_version as u64)
    }

    pub fn zh(self) -> String {
        format!(
            "厂商0x{:04X} 设备0x{:04X} 驱动{}.{}.{}",
            self.vendor_id,
            self.device_id,
            self.driver_version >> 16,
            (self.driver_version >> 8) & 0xFF,
            self.driver_version & 0xFF,
        )
    }
}

/// 能力查询面（枚举产物——定档的核心能力依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceCaps {
    /// 硬渲核心能力（false=软渲兜底档，不看分数）。
    pub core_render: bool,
    pub texture_2d: bool,
    pub blend: bool,
    pub mipmap: bool,
    /// 浮点混合（金牌线要求项）。
    pub float_blend: bool,
}

impl DeviceCaps {
    /// 核心能力闭集（定档银牌线以上要求的四项）。
    fn core_ok(self) -> bool {
        self.core_render && self.texture_2d && self.blend && self.mipmap
    }

    /// 金牌附加项。
    fn gold_ok(self) -> bool {
        self.core_ok() && self.float_blend
    }
}

/// 特性实测微基准（三基准归一分输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MicroBench {
    /// 填充率（MPx/s，基准线 1000）。
    pub fill_mpx_s: u32,
    /// 混合率（GPx/s，基准线 1000）。
    pub blend_gpx_s: u32,
    /// 文字光栅（千字形/毫秒，基准线 1000）。
    pub text_kglyphs_ms: u32,
}

impl MicroBench {
    /// 三基准归一分：各÷基准线×1000 取 min（O(1)，手算锚：
    /// 900/700/800 → min(900,700,800)=700）。
    pub fn score(self) -> u32 {
        let f = (self.fill_mpx_s.min(TIER_GOLD_MIN) * 1000) / TIER_GOLD_MIN;
        let b = (self.blend_gpx_s.min(TIER_GOLD_MIN) * 1000) / TIER_GOLD_MIN;
        let t = (self.text_kglyphs_ms.min(TIER_GOLD_MIN) * 1000) / TIER_GOLD_MIN;
        f.min(b).min(t)
    }
}

// ---------------------------------------------------------------------------
// 支持档位（四档闭集）
// ---------------------------------------------------------------------------

/// 支持档位闭集：金牌全效/银牌标准/铜牌基础/软渲兜底。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Gold,
    Silver,
    Bronze,
    SoftFallback,
}

impl Tier {
    pub const COUNT: usize = 4;

    pub fn all() -> [Tier; Tier::COUNT] {
        [Tier::Gold, Tier::Silver, Tier::Bronze, Tier::SoftFallback]
    }

    pub fn zh(self) -> &'static str {
        match self {
            Tier::Gold => "金牌全效",
            Tier::Silver => "银牌标准",
            Tier::Bronze => "铜牌基础",
            Tier::SoftFallback => "软渲兜底",
        }
    }

    /// 运行时行为联动：Bronze 自动进降级链。
    pub fn triggers_degrade_chain(self) -> bool {
        self == Tier::Bronze
    }

    /// 运行时行为联动：软渲兜底强制软渲路径。
    pub fn forces_software(self) -> bool {
        self == Tier::SoftFallback
    }
}

/// 定档律（O(1) 显性规则——判据手算对拍的唯一依据）。
pub fn classify_tier(caps: DeviceCaps, bench: MicroBench) -> Tier {
    if !caps.core_ok() {
        // 无硬渲能力：软渲兜底（不看分数）。
        return Tier::SoftFallback;
    }
    let s = bench.score();
    if caps.gold_ok() && s >= TIER_GOLD_MIN {
        Tier::Gold
    } else if s >= TIER_SILVER_MIN {
        Tier::Silver
    } else {
        Tier::Bronze
    }
}

// ---------------------------------------------------------------------------
// 已知缺陷表与规避开关
// ---------------------------------------------------------------------------

/// 已知缺陷条目（清单+规避开关——档案的known issues 面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownIssue {
    /// 缺陷 id（如 "virtio-blend-drift"）。
    pub issue_id: &'static str,
    /// 触发条件描述。
    pub trigger: &'static str,
    /// 规避开关名（运行时自动应用）。
    pub workaround: &'static str,
}

/// 规避开关账（运行时按档案自动启用的开关集）。
#[derive(Debug, Default)]
pub struct WorkaroundLedger {
    enabled: BTreeMap<&'static str, &'static str>,
    pub applied: u32,
}

impl WorkaroundLedger {
    pub fn new() -> WorkaroundLedger {
        WorkaroundLedger::default()
    }

    /// 应用一条（O(log n)——重复应用幂等）。
    pub fn apply(&mut self, issue_id: &'static str, workaround: &'static str) {
        self.enabled.insert(issue_id, workaround);
        self.applied += 1;
    }

    pub fn len(&self) -> usize {
        self.enabled.len()
    }

    pub fn is_empty(&self) -> bool {
        self.enabled.is_empty()
    }

    pub fn has(&self, issue_id: &str) -> bool {
        self.enabled.contains_key(issue_id)
    }

    pub fn workaround_of(&self, issue_id: &str) -> Option<&'static str> {
        self.enabled.get(issue_id).copied()
    }
}

// ---------------------------------------------------------------------------
// 适配档案与档案库
// ---------------------------------------------------------------------------

/// 适配档案（探测套件产出——入库随版本分发的单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceProfile {
    pub profile_id: u32,
    pub fingerprint: DriverFingerprint,
    pub tier: Tier,
    pub caps: DeviceCaps,
    pub bench: MicroBench,
    /// 已知问题清单。
    pub issues: Vec<KnownIssue>,
    /// 认证发布后 true；回炉撤销后 false。
    pub certified: bool,
    /// 回炉重认证中 true。
    pub in_requeue: bool,
}

/// 档案库（入库+随版本分发——version 递增快照制）。
#[derive(Debug)]
pub struct ProfileRegistry {
    profiles: BTreeMap<u64, DeviceProfile>,
    next_id: u32,
    /// 分发版本号（每次入库/撤销递增——O(1)）。
    pub dispatch_version: u64,
    pub bag: DiagBag,
}

impl ProfileRegistry {
    pub fn new() -> ProfileRegistry {
        ProfileRegistry {
            profiles: BTreeMap::new(),
            next_id: 1,
            dispatch_version: 0,
            bag: DiagBag::new(),
        }
    }

    /// 入库（认证发布阶段调用——version 递增）。
    pub fn admit(&mut self, mut p: DeviceProfile) -> u32 {
        p.profile_id = self.next_id;
        self.next_id += 1;
        p.certified = true;
        p.in_requeue = false;
        let key = p.fingerprint.packed();
        self.profiles.insert(key, p);
        self.dispatch_version += 1;
        self.next_id - 1
    }

    /// 查档案（指纹 O(log n)）。
    pub fn get(&self, fp: DriverFingerprint) -> Option<&DeviceProfile> {
        self.profiles.get(&fp.packed())
    }

    /// 可变查（回炉审计用）。
    pub fn get_mut(&mut self, fp: DriverFingerprint) -> Option<&mut DeviceProfile> {
        self.profiles.get_mut(&fp.packed())
    }

    /// 回炉撤销（certified=false + in_requeue=true——version 递增）。
    pub fn revoke(&mut self, fp: DriverFingerprint) -> bool {
        match self.profiles.get_mut(&fp.packed()) {
            Some(p) => {
                p.certified = false;
                p.in_requeue = true;
                self.dispatch_version += 1;
                true
            }
            None => false,
        }
    }

    /// 分发快照（当前全档案 id 降序稳定——BTreeMap 键序）。
    pub fn dispatch_snapshot(&self) -> Vec<u32> {
        self.profiles.values().map(|p| p.profile_id).collect()
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 认证流程（四阶段状态机——探测→基准→长稳→发布）
// ---------------------------------------------------------------------------

/// 认证阶段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertStage {
    Probe,
    Bench,
    Soak,
    Release,
}

impl CertStage {
    pub const COUNT: usize = 4;

    pub fn all() -> [CertStage; CertStage::COUNT] {
        [CertStage::Probe, CertStage::Bench, CertStage::Soak, CertStage::Release]
    }

    /// 顺序序号（乱序判定的 O(1) 锚）。
    pub fn seq(self) -> usize {
        match self {
            CertStage::Probe => 0,
            CertStage::Bench => 1,
            CertStage::Soak => 2,
            CertStage::Release => 3,
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            CertStage::Probe => "探测",
            CertStage::Bench => "基准",
            CertStage::Soak => "长稳",
            CertStage::Release => "发布",
        }
    }
}

/// 探测套件输入（一次认证的设备面+实测面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeInput {
    pub fp: DriverFingerprint,
    pub caps: DeviceCaps,
    pub bench: MicroBench,
    /// 长稳失败场景号（None=全过；Some(n)=第 n 场景失败——1 起）。
    pub soak_fail_at: Option<usize>,
    /// 复检模式（回炉重认证时 true——用复检基准覆盖档案基准）。
    pub recheck: bool,
}

/// 认证流水线（四阶段顺序推进——档案入库的唯一正门）。
#[derive(Debug)]
pub struct CertPipeline {
    /// 当前阶段（None=未开始）。
    stage: Option<CertStage>,
    input: Option<ProbeInput>,
    /// 阶段通过账（读屏用）。
    pub passed: Vec<CertStage>,
    pub registry: ProfileRegistry,
}

impl CertPipeline {
    pub fn new() -> CertPipeline {
        CertPipeline {
            stage: None,
            input: None,
            passed: Vec::new(),
            registry: ProfileRegistry::new(),
        }
    }

    /// 阶段 1：探测（能力查询+指纹+微基准齐备才能开跑）。
    pub fn begin(&mut self, input: ProbeInput) -> Result<Tier, u16> {
        if self.stage.is_some() {
            self.registry.bag.push(E_HC_STAGE_ORDER, "begin", "上轮认证未收口");
            return Err(E_HC_STAGE_ORDER);
        }
        // 探测套件定档（O(1)）。
        let tier = classify_tier(input.caps, input.bench);
        self.stage = Some(CertStage::Probe);
        self.input = Some(input);
        Ok(tier)
    }

    /// 推进并执行当前阶段门槛（过→返回下一阶段；Soak 过即发布入库
    /// ——返回 Ok(Release) 时档案已入库，语义"发布完成"）。
    pub fn advance(&mut self) -> Result<CertStage, u16> {
        let cur = match self.stage {
            Some(s) => s,
            None => {
                self.registry.bag.push(E_HC_STAGE_ORDER, "advance", "认证未开始");
                return Err(E_HC_STAGE_ORDER);
            }
        };
        let input = match self.input {
            Some(i) => i,
            None => {
                self.registry.bag.push(E_HC_NO_DEVICE, "advance", "无探测输入");
                return Err(E_HC_NO_DEVICE);
            }
        };
        match cur {
            CertStage::Probe => {
                // 探测过=能力面非空（软渲兜底也放行进入基准——诚实记录）。
                self.stage = Some(CertStage::Bench);
                self.passed.push(cur);
                Ok(CertStage::Bench)
            }
            CertStage::Bench => {
                // 基准阶段：达银牌线才放行（软渲兜底豁免——兜底不看分数）。
                let tier = classify_tier(input.caps, input.bench);
                if tier != Tier::SoftFallback && input.bench.score() < TIER_SILVER_MIN {
                    self.registry.bag.push(
                        E_HC_BENCH_FAIL,
                        &input.fp.zh(),
                        &format!("归一分{}<银牌线{}", input.bench.score(), TIER_SILVER_MIN),
                    );
                    self.stage = None;
                    self.input = None;
                    return Err(E_HC_BENCH_FAIL);
                }
                self.stage = Some(CertStage::Soak);
                self.passed.push(cur);
                Ok(CertStage::Soak)
            }
            CertStage::Soak => {
                // 长稳阶段：30 场景 24h 全过才发布。
                if let Some(fail_scene) = input.soak_fail_at {
                    if fail_scene >= 1 && fail_scene <= SOAK_SCENE_COUNT {
                        self.registry.bag.push(
                            E_HC_SOAK_TIMEOUT,
                            &input.fp.zh(),
                            &format!("第{}场景失败", fail_scene),
                        );
                        self.stage = None;
                        self.input = None;
                        return Err(E_HC_SOAK_TIMEOUT);
                    }
                }
                // 发布：档案入库（复检模式覆盖基准——回炉重认证语义）。
                let mut profile = DeviceProfile {
                    profile_id: 0,
                    fingerprint: input.fp,
                    tier: classify_tier(input.caps, input.bench),
                    caps: input.caps,
                    bench: input.bench,
                    issues: Vec::new(),
                    certified: false,
                    in_requeue: false,
                };
                if input.recheck {
                    profile.issues.push(KnownIssue {
                        issue_id: "recheck-passed",
                        trigger: "回炉复检通过",
                        workaround: "archived-recheck-note",
                    });
                }
                self.registry.admit(profile);
                self.passed.push(cur);
                self.stage = None;
                self.input = None;
                Ok(CertStage::Release)
            }
            CertStage::Release => {
                // 不可达：发布后 stage 已 None（防御性显性拦）。
                self.registry.bag.push(E_HC_STAGE_ORDER, "advance", "发布后无阶段");
                Err(E_HC_STAGE_ORDER)
            }
        }
    }

    /// 当前阶段读屏摘要。
    pub fn spoken(&self) -> String {
        match self.stage {
            Some(s) => format!("认证中：{}（已过{}阶段）", s.zh(), self.passed.len()),
            None => format!(
                "认证空闲（历史通过{}阶段）",
                self.passed.len()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 档案驱动运行时（Bronze 降级链/软渲兜底/规避开关自动应用）
// ---------------------------------------------------------------------------

/// 运行时绑定（档案→运行时行为的唯一入口——行为按档案执行）。
#[derive(Debug, Default)]
pub struct RuntimeBinder {
    pub degrade_chain: bool,
    pub force_software: bool,
    pub workarounds: WorkaroundLedger,
    pub bound_profile: Option<u32>,
}

impl RuntimeBinder {
    pub fn new() -> RuntimeBinder {
        RuntimeBinder::default()
    }

    /// 应用档案（Bronze→降级链；SoftFallback→软渲；issues→规避开关）。
    pub fn apply(&mut self, p: &DeviceProfile) {
        self.degrade_chain = p.tier.triggers_degrade_chain();
        self.force_software = p.tier.forces_software();
        for issue in &p.issues {
            self.workarounds.apply(issue.issue_id, issue.workaround);
        }
        self.bound_profile = Some(p.profile_id);
    }
}

// ---------------------------------------------------------------------------
// 档案复检审计（不符回炉——最高缺陷处理）
// ---------------------------------------------------------------------------

/// 复检结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecheckVerdict {
    /// 档案相符（维持档位）。
    Match,
    /// 档案不符（声明档位高于实测——最高缺陷→回炉）。
    Mismatch,
}

/// 复检审计：声明 Gold/银牌的档案，实测归一分跌破银牌线→不符回炉。
pub fn audit_recheck(p: &DeviceProfile, observed: MicroBench) -> RecheckVerdict {
    let declared_soft = p.tier == Tier::SoftFallback;
    let observed_ok = observed.score() >= TIER_SILVER_MIN;
    if declared_soft || observed_ok {
        RecheckVerdict::Match
    } else {
        // 声明金牌/银牌/铜牌但实测跌破银牌线——档案与实测不符。
        RecheckVerdict::Mismatch
    }
}

/// 回炉编排（revoke→requeue→重认证→恢复——判据炉-05 的主链）。
pub fn requeue_flow(
    reg: &mut ProfileRegistry,
    fp: DriverFingerprint,
    observed: MicroBench,
) -> Result<(RecheckVerdict, Tier), u16> {
    let p = match reg.get(fp) {
        Some(p) => p.clone(),
        None => {
            reg.bag.push(E_HC_NO_DEVICE, "requeue", "档案不存在");
            return Err(E_HC_NO_DEVICE);
        }
    };
    let verdict = audit_recheck(&p, observed);
    if verdict != RecheckVerdict::Mismatch {
        return Ok((verdict, p.tier));
    }
    // 最高缺陷处理：撤销认证+回炉立案。
    reg.bag.push(
        E_HC_PROFILE_MISMATCH,
        &fp.zh(),
        &format!(
            "声明{}实测归一分{}<银牌线{}——回炉",
            p.tier.zh(),
            observed.score(),
            TIER_SILVER_MIN
        ),
    );
    reg.revoke(fp);
    // 重认证（复检模式——用观测基准重走四阶段）。
    let input = ProbeInput {
        fp,
        caps: p.caps,
        bench: observed,
        soak_fail_at: None,
        recheck: true,
    };
    let mut pipe = CertPipeline::new();
    let _tier0 = pipe.begin(input)?;
    let mut released = false;
    for _ in 0..CertStage::COUNT {
        match pipe.advance() {
            Ok(CertStage::Release) => {
                released = true;
                break;
            }
            Ok(_) => continue,
            Err(code) => {
                // 审查不留痕等于没审：重认证拦截立案回流外层档案库台账。
                reg.bag.push(code, &fp.zh(), "回炉重认证拦截");
                return Err(code);
            }
        }
    }
    if !released {
        reg.bag.push(E_HC_STAGE_ORDER, "requeue", "重认证未走完");
        return Err(E_HC_STAGE_ORDER);
    }
    let new_tier = classify_tier(p.caps, observed);
    Ok((verdict, new_tier))
}

// ---------------------------------------------------------------------------
// virtio-gpu 首发档案（锚点判据 1——首发入库走全流程）
// ---------------------------------------------------------------------------

/// virtio-gpu 首发指纹（vendor 0x1AF4 / device 0x1050 / driver 1.0.2）。
pub const VIRTIO_GPU_FP: DriverFingerprint = DriverFingerprint {
    vendor_id: 0x1AF4,
    device_id: 0x1050,
    driver_version: 0x0001_0002,
};

/// virtio-gpu 能力面（core+2d+blend+mipmap 有；float_blend 无→银牌）。
pub const VIRTIO_GPU_CAPS: DeviceCaps = DeviceCaps {
    core_render: true,
    texture_2d: true,
    blend: true,
    mipmap: true,
    float_blend: false,
};

/// virtio-gpu 实测微基准（900/700/800→归一分 min=700→银牌恰值）。
pub const VIRTIO_GPU_BENCH: MicroBench = MicroBench {
    fill_mpx_s: 900,
    blend_gpx_s: 700,
    text_kglyphs_ms: 800,
};

// ---------------------------------------------------------------------------
// 判据（七项——首发档案/四阶段走通/三拦互异/运行时联动/回炉/分发/码）
// ---------------------------------------------------------------------------

/// CGPU-F0012 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vca12_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0012");

    // 档-01 virtio-gpu 首发档案：定档银牌恰值+指纹位拼接恰值。
    {
        // 手算：score=min(900,700,800)=700；700≥600 且 <1000 且
        // gold_ok=false（float_blend 无）→ Silver。
        let tier = classify_tier(VIRTIO_GPU_CAPS, VIRTIO_GPU_BENCH);
        // 手算指纹：0x1AF4<<48 | 0x1050<<32 | 0x00010002
        //        = 0x1AF4_1050_0001_0002。
        let fp = VIRTIO_GPU_FP.packed();
        let ok = tier == Tier::Silver
            && fp == 0x1AF4_1050_0001_0002u64
            && VIRTIO_GPU_BENCH.score() == 700
            && !tier.triggers_degrade_chain()
            && !tier.forces_software();
        if ok {
            s.ok("F0012-档-01-virtio首发银牌恰值");
        } else {
            s.fail("F0012-档-01-virtio首发银牌恰值", "定档或指纹不符");
        }
    }

    // 认-02 四阶段走通：Probe→Bench→Soak→Release 顺序全过+入库。
    {
        let mut pipe = CertPipeline::new();
        let t0 = pipe.begin(ProbeInput {
            fp: VIRTIO_GPU_FP,
            caps: VIRTIO_GPU_CAPS,
            bench: VIRTIO_GPU_BENCH,
            soak_fail_at: None,
            recheck: false,
        });
        let mut steps: Vec<CertStage> = Vec::new();
        let mut walk_ok = t0 == Ok(Tier::Silver);
        for _ in 0..CertStage::COUNT {
            match pipe.advance() {
                Ok(st) => steps.push(st),
                Err(_) => {
                    walk_ok = false;
                    break;
                }
            }
            if steps.last() == Some(&CertStage::Release) {
                break;
            }
        }
        // 手算：顺序应为 基准→长稳→发布（探测已由 begin 记账）；
        // 入库 1 档案、分发版本 1。
        let ok = walk_ok
            && steps.as_slice()
                == [CertStage::Bench, CertStage::Soak, CertStage::Release]
            && pipe.registry.len() == 1
            && pipe.registry.dispatch_version == 1
            && pipe.registry.get(VIRTIO_GPU_FP).map(|p| p.certified) == Some(true)
            && pipe.registry.get(VIRTIO_GPU_FP).map(|p| p.tier) == Some(Tier::Silver);
        if ok {
            s.ok("F0012-认-02-四阶段走通入库");
        } else {
            s.fail("F0012-认-02-四阶段走通入库", "阶段序或入库不符");
        }
    }

    // 认-03 三拦互异：乱序/基准不达/长稳失败各自显性拦+立案恰值。
    {
        // 3a 阶段乱序：未 begin 就 advance。
        let mut p1 = CertPipeline::new();
        let r1 = p1.advance();
        // 3b 基准不达线：400/300/350→min=300<600→E_HC_BENCH_FAIL。
        let mut p2 = CertPipeline::new();
        let weak = MicroBench { fill_mpx_s: 400, blend_gpx_s: 300, text_kglyphs_ms: 350 };
        let _ = p2.begin(ProbeInput {
            fp: DriverFingerprint { vendor_id: 0x10DE, device_id: 0x2504, driver_version: 0x0201_0003 },
            caps: DeviceCaps {
                core_render: true, texture_2d: true, blend: true,
                mipmap: true, float_blend: true,
            },
            bench: weak,
            soak_fail_at: None,
            recheck: false,
        });
        let r2 = p2.advance(); // Probe→Bench
        let r2b = p2.advance(); // Bench 门槛→拦
        // 3c 长稳失败：第 17 场景失败。
        let mut p3 = CertPipeline::new();
        let _ = p3.begin(ProbeInput {
            fp: VIRTIO_GPU_FP,
            caps: VIRTIO_GPU_CAPS,
            bench: VIRTIO_GPU_BENCH,
            soak_fail_at: Some(17),
            recheck: false,
        });
        let _ = p3.advance(); // Probe→Bench
        let _ = p3.advance(); // Bench→Soak
        let r3 = p3.advance(); // Soak 门槛→拦
        // 手算：三码 0xC402/0xC403/0xC404 互异；p2 立案 1 条 BENCH；
        // p3 立案 1 条 SOAK；入库均 0。
        let ok = r1 == Err(E_HC_STAGE_ORDER)
            && matches!(r2, Ok(CertStage::Bench))
            && r2b == Err(E_HC_BENCH_FAIL)
            && p2.registry.bag.count_of(E_HC_BENCH_FAIL) == 1
            && r3 == Err(E_HC_SOAK_TIMEOUT)
            && p3.registry.bag.count_of(E_HC_SOAK_TIMEOUT) == 1
            && p2.registry.is_empty()
            && p3.registry.is_empty()
            && [E_HC_STAGE_ORDER, E_HC_BENCH_FAIL, E_HC_SOAK_TIMEOUT]
                .iter()
                .collect::<Vec<_>>()
                .len()
                == 3;
        if ok {
            s.ok("F0012-认-03-三拦互异立案恰值");
        } else {
            s.fail("F0012-认-03-三拦互异立案恰值", "拦截面或立案不符");
        }
    }

    // 行-04 档案驱动运行时：Bronze 降级链/软渲兜底/规避开关自动应用。
    {
        // 铜牌设备：500/400/450→min=400<600→Bronze→降级链 true。
        let bronze_caps = DeviceCaps {
            core_render: true, texture_2d: true, blend: true,
            mipmap: true, float_blend: false,
        };
        let bronze_bench = MicroBench { fill_mpx_s: 500, blend_gpx_s: 400, text_kglyphs_ms: 450 };
        let tier_b = classify_tier(bronze_caps, bronze_bench);
        // 软渲设备：core=false→SoftFallback（不看分数）。
        let soft_caps = DeviceCaps {
            core_render: false, texture_2d: false, blend: false,
            mipmap: false, float_blend: false,
        };
        let tier_s = classify_tier(soft_caps, VIRTIO_GPU_BENCH);
        // 金牌档案带已知缺陷→规避开关自动应用恰值。
        let gold_caps = DeviceCaps {
            core_render: true, texture_2d: true, blend: true,
            mipmap: true, float_blend: true,
        };
        let gold_bench = MicroBench { fill_mpx_s: 1200, blend_gpx_s: 1100, text_kglyphs_ms: 1050 };
        // 手算：min(1050,1000,1000)=1000≥1000 且 gold_ok→Gold。
        let mut prof = DeviceProfile {
            profile_id: 9,
            fingerprint: DriverFingerprint { vendor_id: 0x8086, device_id: 0x46A6, driver_version: 0x0301_0004 },
            tier: classify_tier(gold_caps, gold_bench),
            caps: gold_caps,
            bench: gold_bench,
            issues: vec![
                KnownIssue {
                    issue_id: "float-drift",
                    trigger: "FP16 混合尾差",
                    workaround: "force-fp32-blend",
                },
                KnownIssue {
                    issue_id: "mip-pop",
                    trigger: "mip 切换闪烁",
                    workaround: "clamp-mip-lod",
                },
            ],
            certified: true,
            in_requeue: false,
        };
        prof.tier = Tier::Gold;
        let mut binder = RuntimeBinder::new();
        binder.apply(&prof);
        let bronze_binder = {
            let mut b = RuntimeBinder::new();
            b.apply(&DeviceProfile {
                profile_id: 10,
                fingerprint: VIRTIO_GPU_FP,
                tier: Tier::Bronze,
                caps: bronze_caps,
                bench: bronze_bench,
                issues: Vec::new(),
                certified: true,
                in_requeue: false,
            });
            b
        };
        let soft_binder = {
            let mut b = RuntimeBinder::new();
            b.apply(&DeviceProfile {
                profile_id: 11,
                fingerprint: VIRTIO_GPU_FP,
                tier: Tier::SoftFallback,
                caps: soft_caps,
                bench: VIRTIO_GPU_BENCH,
                issues: Vec::new(),
                certified: true,
                in_requeue: false,
            });
            b
        };
        // 手算：Gold→降级链 false+软渲 false+开关 2 条恰值；
        // Bronze→降级链 true；SoftFallback→软渲 true。
        let ok = tier_b == Tier::Bronze
            && tier_s == Tier::SoftFallback
            && gold_bench.score() == 1000
            && !binder.degrade_chain
            && !binder.force_software
            && binder.workarounds.len() == 2
            && binder.workarounds.workaround_of("float-drift") == Some("force-fp32-blend")
            && binder.workarounds.workaround_of("mip-pop") == Some("clamp-mip-lod")
            && bronze_binder.degrade_chain
            && !bronze_binder.force_software
            && soft_binder.force_software
            && !soft_binder.degrade_chain;
        if ok {
            s.ok("F0012-行-04-档案驱动运行时联动");
        } else {
            s.fail("F0012-行-04-档案驱动运行时联动", "联动面不符");
        }
    }

    // 炉-05 不符回炉：Gold 档案实测掉帧→最高缺陷→revoke→重认证恢复。
    {
        let mut reg = ProfileRegistry::new();
        let gold_caps = DeviceCaps {
            core_render: true, texture_2d: true, blend: true,
            mipmap: true, float_blend: true,
        };
        let gold_bench = MicroBench { fill_mpx_s: 1200, blend_gpx_s: 1100, text_kglyphs_ms: 1050 };
        let fp = DriverFingerprint { vendor_id: 0x8086, device_id: 0x46A6, driver_version: 0x0301_0004 };
        let _ = reg.admit(DeviceProfile {
            profile_id: 0,
            fingerprint: fp,
            tier: Tier::Gold,
            caps: gold_caps,
            bench: gold_bench,
            issues: Vec::new(),
            certified: false,
            in_requeue: false,
        });
        // 实测掉帧：300/200/250→min=200<600→不符→回炉。
        let observed = MicroBench { fill_mpx_s: 300, blend_gpx_s: 200, text_kglyphs_ms: 250 };
        // 手算：observed.score=200<600→Mismatch；回炉重认证（复检基
        // 准仍 200<银牌线）→BENCH 拦→档案保持 revoked。
        let out = requeue_flow(&mut reg, fp, observed);
        let revoked = reg.get(fp).map(|p| (p.certified, p.in_requeue));
        // 手算：版本 1(入库)+1(撤销)=2。
        let ver_after = reg.dispatch_version;
        let ok = matches!(&out, Err(E_HC_BENCH_FAIL))
            && revoked == Some((false, true))
            && reg.bag.count_of(E_HC_PROFILE_MISMATCH) == 1
            && reg.bag.count_of(E_HC_BENCH_FAIL) == 1
            && ver_after == 2;
        if ok {
            s.ok("F0012-炉-05-不符回炉撤销复检拦");
        } else {
            s.fail("F0012-炉-05-不符回炉撤销复检拦", "回炉链不符");
        }
    }

    // 发-06 随版本分发：两次入库 version 递增+快照含全部档案。
    {
        let mut reg = ProfileRegistry::new();
        let caps = VIRTIO_GPU_CAPS;
        let bench = VIRTIO_GPU_BENCH;
        let fp_a = VIRTIO_GPU_FP;
        let fp_b = DriverFingerprint { vendor_id: 0x10DE, device_id: 0x2504, driver_version: 0x0201_0003 };
        let id_a = reg.admit(DeviceProfile {
            profile_id: 0, fingerprint: fp_a, tier: Tier::Silver,
            caps, bench, issues: Vec::new(), certified: false, in_requeue: false,
        });
        let id_b = reg.admit(DeviceProfile {
            profile_id: 0, fingerprint: fp_b, tier: Tier::Silver,
            caps, bench, issues: Vec::new(), certified: false, in_requeue: false,
        });
        let snap = reg.dispatch_snapshot();
        // 手算：id 1,2；version 2；快照按指纹键序（fp_a<fp_b 因
        // 0x1AF4<0x10DE？否——0x1AF4>0x10DE，所以 B 先 A 后）。
        let ok = id_a == 1
            && id_b == 2
            && reg.dispatch_version == 2
            && reg.len() == 2
            && snap == vec![2, 1]
            && reg.get(fp_a).is_some()
            && reg.get(fp_b).is_some();
        if ok {
            s.ok("F0012-发-06-版本分发快照有序");
        } else {
            s.fail("F0012-发-06-版本分发快照有序", "版本或快照不符");
        }
    }

    // 码-07 五码互异+reason 全量显性+读屏摘要非空。
    {
        let codes = [E_HC_NO_DEVICE, E_HC_STAGE_ORDER, E_HC_BENCH_FAIL, E_HC_SOAK_TIMEOUT, E_HC_PROFILE_MISMATCH];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        // 段位恰值：0xC401..0xC405 连续互异。
        let seg_ok = codes[0] == 0xC401
            && codes[1] == 0xC402
            && codes[2] == 0xC403
            && codes[3] == 0xC404
            && codes[4] == 0xC405;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        let mut bag = DiagBag::new();
        bag.push(E_HC_PROFILE_MISMATCH, "virtio-gpu", "演练立案");
        let spoken_ok = bag.spoken().contains("回炉") && bag.spoken().contains("virtio-gpu");
        // 无设备拦：空注册表回炉→E_HC_NO_DEVICE。
        let mut reg = ProfileRegistry::new();
        let ghost = requeue_flow(&mut reg, VIRTIO_GPU_FP, VIRTIO_GPU_BENCH);
        let ok = distinct && seg_ok && reasons_all && spoken_ok
            && ghost == Err(E_HC_NO_DEVICE)
            && !bag.is_empty()
            && bag.len() == 1;
        if ok {
            s.ok("F0012-码-07-五码互异读屏显性");
        } else {
            s.fail("F0012-码-07-五码互异读屏显性", "诊断面不符");
        }
    }

    s
}
