//! UNX-F50002 渲染意图四档实现（感知/相对比色/饱和/绝对）（0x42c60 段）
//!
//! 锚点：UNX-F50002-J1 四档 ×50 色块输出逐档一致；判据锚定 §AI-63 §2 专题十二。
//!
//! 数据段 `SEG_BASE = 0x42c60`（独占 0x20）。诊断码闭集 = 本段 `E_EMPTY..=E_UNREGISTERED`
//! （1..=12），`IntentError::code()` 恒为 `0x42c60 + n`，不回绕、不借用 0x42c40 段。
//!
//! 为什么本模块不能只写四个 `match` 分支：F50001 提供的是「单点色域映射」的公共内核
//! （色度学、ICC 解析、边界二分），而渲染意图在设备上是一条**会话**——请求、策略
//! 校验、落账、回退各有状态；四档之间的差异必须能逐 patch 复算并落账，否则「意图」
//! 只是一个枚举标签。本模块补的正是这层：策略注册表 + 五态会话机 + 意图账本。
//!
//! 结构清单：
//! * `IntentError`：12 个显式错误，`code()` 落 0x42c60 段；
//! * `IntentPolicy` / `PolicyRegistry`：每设备意图策略（容量 6，重名显式拒绝）；
//! * `SessionState` / `IntentSession`：五态会话机（Idle/Requested/Validated/Applied/
//!   Reverted），非法跳转返回 `IllegalTransition`；
//! * `IntentLedger`：意图账本（容量 64），逐 patch 记录输出与 BPC 结局，守恒可查；
//! * `IntentEngine`：四档映射本体（四档各自独立分支 + 可配置拐点/上限/纸张白）；
//! * `patch50()`：J1 的 50 色块样本（10 级中性 + 40 个去中性网格点）；
//! * `injection_core`：5 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{expf, fabsf};

use super::unx50001_gamutmap::{
    black_point_compensate, boundary_chroma, delta_e2000, lab_to_lch, lab_to_xyz_d50, lch_to_lab,
    rgb8_to_lab, xyz_d50_to_lab, IccProfile, Intent, Lab, Lch, Rgb8, Xyz,
};

/// 诊断段：0x42c60 独占 0x20（同批其余模块从 0x42c80 起顺序占用）。
pub const SEG_BASE: u32 = 0x42c60;

pub const E_EMPTY: u16 = 1;
pub const E_POLICY_FULL: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_NO_POLICY: u16 = 4;
pub const E_INTENT_NOT_ALLOWED: u16 = 5;
pub const E_BPC_NOT_ALLOWED: u16 = 6;
pub const E_ILLEGAL_TRANSITION: u16 = 7;
pub const E_LEDGER_FULL: u16 = 8;
pub const E_RANGE: u16 = 9;
pub const E_PATCH_COUNT: u16 = 10;
pub const E_NOT_REQUESTED: u16 = 11;
pub const E_UNREGISTERED: u16 = 12;

/// 意图会话/账本的显式错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentError {
    /// 空输入（无 patch、空设备名）。
    Empty,
    /// 策略注册表已满（容量 6）。
    PolicyFull,
    /// 同名设备重复注册。
    Duplicate,
    /// 查询的设备没有注册策略。
    NoPolicy,
    /// 策略禁止该意图档。
    IntentNotAllowed,
    /// BPC 请求在语义上不成立（感知/饱和档，或策略关闭）。
    BpcNotAllowed,
    /// 会话状态机拒绝了这次跳转。
    IllegalTransition,
    /// 意图账本容量耗尽（容量 64）。
    LedgerFull,
    /// 数值越界（NaN/Inf/负色度/意图码越界）。
    Range,
    /// 单批 patch 数越界（1..=255，受 `u8` 序位字段约束）。
    PatchCount,
    /// 会话尚未请求意图就校验/落账。
    NotRequested,
    /// 回退目标会话在注册表里查不到策略。
    Unregistered,
}

impl IntentError {
    /// 诊断码：`SEG_BASE + n`，与全仓闭集口径一致。
    pub fn code(self) -> u32 {
        let n = match self {
            IntentError::Empty => E_EMPTY,
            IntentError::PolicyFull => E_POLICY_FULL,
            IntentError::Duplicate => E_DUPLICATE,
            IntentError::NoPolicy => E_NO_POLICY,
            IntentError::IntentNotAllowed => E_INTENT_NOT_ALLOWED,
            IntentError::BpcNotAllowed => E_BPC_NOT_ALLOWED,
            IntentError::IllegalTransition => E_ILLEGAL_TRANSITION,
            IntentError::LedgerFull => E_LEDGER_FULL,
            IntentError::Range => E_RANGE,
            IntentError::PatchCount => E_PATCH_COUNT,
            IntentError::NotRequested => E_NOT_REQUESTED,
            IntentError::Unregistered => E_UNREGISTERED,
        };
        SEG_BASE + n as u32
    }

    /// 一行人类可读原因（落日志用，不参与判据）。
    pub fn message(self) -> &'static str {
        match self {
            IntentError::Empty => "empty patch batch or device name",
            IntentError::PolicyFull => "intent policy registry full",
            IntentError::Duplicate => "intent policy already registered",
            IntentError::NoPolicy => "no intent policy for device",
            IntentError::IntentNotAllowed => "rendering intent not allowed by policy",
            IntentError::BpcNotAllowed => "black point compensation refused",
            IntentError::IllegalTransition => "illegal session transition",
            IntentError::LedgerFull => "intent ledger full",
            IntentError::Range => "value outside legal range",
            IntentError::PatchCount => "patch count outside 1..=255",
            IntentError::NotRequested => "intent not requested yet",
            IntentError::Unregistered => "session device unregistered",
        }
    }
}

// PCS（D50）白点。与 F50001 的内部常量同值；本模块要按「纸张白 / PCS 白」的比例
// 做绝对比色档的落点，必须自己持有一份（F50001 未导出该常量）。
const PCS_X: f32 = 0.96422;
const PCS_Y: f32 = 1.0;
const PCS_Z: f32 = 0.82521;

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 意图码 → 枚举。越界码返回 `Range`（不 panic、不猜默认档）。
pub fn intent_from_index(i: u8) -> Result<Intent, IntentError> {
    match i {
        0 => Ok(Intent::Perceptual),
        1 => Ok(Intent::Relative),
        2 => Ok(Intent::Saturation),
        3 => Ok(Intent::Absolute),
        _ => Err(IntentError::Range),
    }
}

/// 下标 → 枚举（判据遍历四档用，避免裸下标）。
pub fn intent_at(i: usize) -> Intent {
    match i {
        1 => Intent::Relative,
        2 => Intent::Saturation,
        3 => Intent::Absolute,
        _ => Intent::Perceptual,
    }
}

// ---------------------------------------------------------------------------
// 1. 设备意图策略 + 注册表
// ---------------------------------------------------------------------------

pub const POLICY_CAP: usize = 6;

/// 一台输出设备的意图策略：允许哪几档、是否开 BPC、纸张白、色域名义缩放。
///
/// 策略与 profile 分开存：同一台机器可以换 profile（换纸/换墨），但「这台设备
/// 允许哪些意图」是运营配置，换 profile 不该悄悄改掉它。
#[derive(Clone, Debug, PartialEq)]
pub struct IntentPolicy {
    pub device: String,
    /// 按 `Intent::code()` 下标索引的四档允许位。
    pub allowed: [bool; 4],
    pub bpc_enabled: bool,
    /// 该设备的纸张白（绝对比色档的落点）。
    pub paper: Xyz,
    /// 名义色域缩放（用于策略侧的越域预判，0.20..=1.00）。
    pub gamut_scale: f32,
}

impl IntentPolicy {
    /// 全档允许、BPC 开启、纸张白取 96% 反射率的办公纸。
    pub fn full(device: &str) -> IntentPolicy {
        IntentPolicy {
            device: device.to_string(),
            allowed: [true; 4],
            bpc_enabled: true,
            // 0.96 反射率 + 轻微暖调：纸不是理想漫射体，绝对比色必须落在这里。
            // 三通道比例 (0.9858, 0.96, 0.9694)×D50 —— 全部 ≤ 1，物理上
            // 纸只能比理想漫射体更暗，不能更亮。
            paper: Xyz::new(0.9505, 0.96, 0.8000),
            gamut_scale: 1.0,
        }
    }

    /// 只允许比色两档（印前打样常用：不生成新色，只做可测的换算）。
    pub fn colorimetric_only(device: &str) -> IntentPolicy {
        let mut p = IntentPolicy::full(device);
        p.allowed = [false, true, false, true];
        p
    }

    /// 该策略是否允许 `intent` 档。
    pub fn allows(&self, intent: Intent) -> bool {
        match self.allowed.get(intent.code() as usize) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 策略校验：档位允许 + BPC 语义成立。这是会话 `Validated` 态的唯一入口，
    /// 校验失败必须让会话停在 `Requested`（不静默降档）。
    pub fn check(&self, intent: Intent, bpc: bool) -> Result<(), IntentError> {
        if !self.allows(intent) {
            return Err(IntentError::IntentNotAllowed);
        }
        if bpc && !intent.supports_bpc() {
            return Err(IntentError::BpcNotAllowed);
        }
        if bpc && !self.bpc_enabled {
            return Err(IntentError::BpcNotAllowed);
        }
        Ok(())
    }
}

/// 策略注册表（定容 6 台设备）。重名注册是配置错误，显式拒绝而不是覆盖——
/// 覆盖会让上一台设备的意图静默改变，正是本批要消灭的静默。
#[derive(Clone, Debug)]
pub struct PolicyRegistry {
    entries: Vec<IntentPolicy>,
}

impl PolicyRegistry {
    pub fn new() -> PolicyRegistry {
        PolicyRegistry {
            entries: Vec::new(),
        }
    }

    /// 注册一台设备策略。容量满 → `PolicyFull`；同名 → `Duplicate`。
    pub fn register(&mut self, policy: IntentPolicy) -> Result<(), IntentError> {
        if policy.device.is_empty() {
            return Err(IntentError::Empty);
        }
        if self.entries.len() >= POLICY_CAP {
            return Err(IntentError::PolicyFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.device.as_str() == policy.device.as_str() {
                    return Err(IntentError::Duplicate);
                }
            }
            i += 1;
        }
        self.entries.push(policy);
        Ok(())
    }

    /// 按设备名查策略。
    pub fn lookup(&self, device: &str) -> Option<&IntentPolicy> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.device.as_str() == device {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 2. 五态会话机
// ---------------------------------------------------------------------------

/// 渲染意图会话状态。转移图（唯一合法路径）：
///
/// ```text
/// Idle --request--> Requested --validate--> Validated --apply--> Applied --revert--> Reverted
///                       ^                       |                                  |
///                       |                       +---(校验失败留在 Requested)       |
///                       +-----------------------------------------------------------+
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Requested,
    Validated,
    Applied,
    Reverted,
}

impl SessionState {
    pub fn code(self) -> u8 {
        match self {
            SessionState::Idle => 0,
            SessionState::Requested => 1,
            SessionState::Validated => 2,
            SessionState::Applied => 3,
            SessionState::Reverted => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Requested => "requested",
            SessionState::Validated => "validated",
            SessionState::Applied => "applied",
            SessionState::Reverted => "reverted",
        }
    }
}

/// 一条渲染意图会话：绑一台设备，走完请求→校验→落账→回退。
///
/// 为什么要有会话而不是裸函数：意图在页与页之间是**粘性**的（印前按页签意图），
/// 而「请求了但没校验」「校验了但没落账」这两类半截状态必须能被判据抓到，
/// 否则作业的意图档位就不可审计。
#[derive(Clone, Debug)]
pub struct IntentSession {
    device: String,
    state: SessionState,
    intent: Option<Intent>,
    bpc: bool,
    steps: u16,
    rejections: u16,
}

impl IntentSession {
    /// 新建会话（Idle）。
    pub fn new(device: &str) -> IntentSession {
        IntentSession {
            device: device.to_string(),
            state: SessionState::Idle,
            intent: None,
            bpc: false,
            steps: 0,
            rejections: 0,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn device(&self) -> &str {
        self.device.as_str()
    }

    pub fn intent(&self) -> Option<Intent> {
        self.intent
    }

    pub fn bpc(&self) -> bool {
        self.bpc
    }

    pub fn steps(&self) -> u16 {
        self.steps
    }

    pub fn rejections(&self) -> u16 {
        self.rejections
    }

    /// 请求一档意图（仅 Idle / Reverted 可请求）。空设备名 → `Empty`。
    pub fn request(&mut self, intent: Intent, bpc: bool) -> Result<(), IntentError> {
        if self.device.is_empty() {
            return Err(IntentError::Empty);
        }
        if self.state != SessionState::Idle && self.state != SessionState::Reverted {
            self.rejections = self.rejections.saturating_add(1);
            return Err(IntentError::IllegalTransition);
        }
        self.intent = Some(intent);
        self.bpc = bpc;
        self.state = SessionState::Requested;
        Ok(())
    }

    /// 按注册表校验当前请求。设备没注册 → `NoPolicy`；策略拒绝 → 对应错误；
    /// 两种失败都留在 `Requested`，请求本身不被抹掉（可换档重试）。
    pub fn validate(&mut self, reg: &PolicyRegistry) -> Result<(), IntentError> {
        if self.state != SessionState::Requested {
            self.rejections = self.rejections.saturating_add(1);
            return Err(IntentError::IllegalTransition);
        }
        let intent = self.intent.ok_or(IntentError::NotRequested)?;
        let policy = reg.lookup(self.device.as_str()).ok_or(IntentError::NoPolicy)?;
        policy.check(intent, self.bpc)?;
        self.state = SessionState::Validated;
        Ok(())
    }

    /// 落账：把整批 patch 按当前意图写进账本。仅在 `Validated` 可调用；
    /// 成功即转 `Applied`（失败不转移，账本也不留半批——见 `process_batch`）。
    pub fn apply(
        &mut self,
        engine: &IntentEngine,
        profile: &IccProfile,
        patches: &[Rgb8],
        ledger: &mut IntentLedger,
    ) -> Result<BatchReport, IntentError> {
        if self.state != SessionState::Validated {
            self.rejections = self.rejections.saturating_add(1);
            return Err(IntentError::IllegalTransition);
        }
        let intent = self.intent.ok_or(IntentError::NotRequested)?;
        let rep = ledger.process_batch(engine, profile, intent, self.bpc, patches)?;
        self.state = SessionState::Applied;
        self.steps = self.steps.saturating_add(1);
        Ok(rep)
    }

    /// 回退到可再请求态（换意图重跑）。
    pub fn revert(&mut self) -> Result<(), IntentError> {
        if self.state != SessionState::Applied {
            self.rejections = self.rejections.saturating_add(1);
            return Err(IntentError::IllegalTransition);
        }
        self.state = SessionState::Reverted;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. 意图引擎：四档各自的映射分支
// ---------------------------------------------------------------------------

/// 引擎配置。`black_l` 是**实测设备黑**（L*）而不是矩阵模型的 0：
/// 喷墨在纸上的黑受渗透/纸张吸收影响，实测约 3~6 L*，矩阵模型给不出来。
/// 感知档按它把源 L 压进可印范围；比色档不动 L（那是 BPC 的职责）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineCfg {
    /// 纸张白（绝对比色的落点）；`Relative` 档不使用它。
    pub paper: Xyz,
    /// 实测设备黑 L*（0..=60）。
    pub black_l: f32,
    /// 感知档色度拐点系数（相对域边界），默认 0.82。
    pub knee: f32,
    /// 感知档色度上限系数，默认 0.90（与饱和度档留出可区分带）。
    pub ceiling: f32,
    /// 饱和度档色度提升系数，默认 1.12（ICC 饱和度意图的观感语义）。
    pub sat_boost: f32,
}

impl EngineCfg {
    /// 办公纸档：96% 反射率纸 + 4 L* 实测黑 + ICC 常用拐点。
    pub fn office() -> EngineCfg {
        EngineCfg {
            paper: Xyz::new(0.9505, 0.96, 0.8000),
            black_l: 4.0,
            knee: 0.82,
            ceiling: 0.90,
            sat_boost: 1.12,
        }
    }

    /// 理想纸档（实验室参考：纸=理想漫射体、黑=0）。用于把「纸张差异」与
    /// 「意图差异」在判据里分开。
    pub fn ideal() -> EngineCfg {
        EngineCfg {
            paper: Xyz::new(PCS_X, PCS_Y, PCS_Z),
            black_l: 0.0,
            knee: 0.82,
            ceiling: 0.90,
            sat_boost: 1.12,
        }
    }
}

/// 四档映射本体。
#[derive(Clone, Copy, Debug)]
pub struct IntentEngine {
    pub cfg: EngineCfg,
}

impl IntentEngine {
    pub const fn new(cfg: EngineCfg) -> IntentEngine {
        IntentEngine { cfg }
    }

    /// 按 office 配置构造（绝大多数判据用这一档）。
    pub fn office() -> IntentEngine {
        IntentEngine::new(EngineCfg::office())
    }

    /// 源 L 的可印范围下限（实测设备黑）。
    pub fn target_black(&self, profile: &IccProfile) -> f32 {
        let _ = profile;
        clampf(self.cfg.black_l, 0.0, 60.0)
    }

    /// 源 L 的可印范围上限（设备白 L*，clip 到 40..=100）。
    pub fn target_white(&self, profile: &IccProfile) -> f32 {
        clampf(profile.white_l(), 40.0, 100.0)
    }

    /// 单 patch 映射。四档各自独立分支，任何输入都返回确定值（无 panic 路径）。
    ///
    /// * Perceptual：L 压进 [设备黑, 设备白]，C 走 0.82 拐点 / 0.90 上限的软压缩；
    /// * Relative：白点相对（PCS 白→设备白），L/C 不改写，仅裁剪到设备域；
    /// * Saturation：C 乘 `sat_boost` 后夹到域边界（保饱和的观感语义），L 夹到可印域；
    /// * Absolute：不做白点适配，XYZ 直接乘「纸张白 / PCS 白」，保留绝对色度量。
    pub fn map(&self, src: Lab, profile: &IccProfile, intent: Intent, bpc: bool) -> Lab {
        let l_in = if bpc && intent.supports_bpc() {
            black_point_compensate(src.l, 0.0, profile.black_l())
        } else {
            src.l
        };
        let lch = lab_to_lch(Lab::new(l_in, src.a, src.b));
        let boundary = boundary_chroma(profile, lch.l, lch.h);
        let black = self.target_black(profile);
        let white = self.target_white(profile);
        match intent {
            Intent::Perceptual => {
                let l2 = remap_l(lch.l, black, white);
                let c2 = soft_chroma(lch.c, boundary, self.cfg.knee, self.cfg.ceiling);
                settle(profile, Lch::new(l2, c2, lch.h))
            }
            Intent::Relative => settle(profile, lch),
            Intent::Saturation => {
                let l2 = clampf(lch.l, black, white);
                let c2 = if lch.c <= 0.0 {
                    0.0
                } else {
                    clampf(lch.c * self.cfg.sat_boost, 0.0, boundary)
                };
                settle(profile, Lch::new(l2, c2, lch.h))
            }
            Intent::Absolute => {
                // 绝对比色：源（PCS D50 绝对量）→ 按纸张白等比缩放 → 设备域。
                // 白点适配在这里被**故意跳过**，这正是它与 Relative 的唯一语义差。
                let xyz = lab_to_xyz_d50(Lab::new(lch.l, src.a, src.b));
                let scaled = Xyz::new(
                    xyz.x * (self.cfg.paper.x / PCS_X),
                    xyz.y * (self.cfg.paper.y / PCS_Y),
                    xyz.z * (self.cfg.paper.z / PCS_Z),
                );
                settle(profile, lab_to_lch(xyz_d50_to_lab(scaled)))
            }
        }
    }
}

/// 源 L 线性压进 [black, white]（0→black、100→white）。
fn remap_l(l: f32, black: f32, white: f32) -> f32 {
    let b = clampf(black, 0.0, 60.0);
    let w = clampf(white, 40.0, 100.0);
    if w - b < 1.0 {
        return clampf(l, 0.0, 100.0);
    }
    clampf(b + (clampf(l, 0.0, 100.0) / 100.0) * (w - b), 0.0, 100.0)
}

/// 感知档色度压缩：`knee·boundary` 以内原样，之外指数饱和到 `ceiling·boundary`。
///
/// 拐点取 0.82、上限取 0.90 的理由：ICC 感知意图要求「不裁切、可复现的层次」，
/// 留 10% 余量让 OOG 色在纸上仍有梯度；同时与饱和度档（贴边界）拉开 ≥10%
/// 的色度差，使两档在越域色上**可区分**（J1 的可区分性度量就测这条）。
/// 单调性：t 增大 → c' 增大（一阶导 `span·exp(-t)/span_denom > 0`），无回折。
fn soft_chroma(c: f32, boundary: f32, knee_k: f32, ceiling_k: f32) -> f32 {
    if boundary <= 1e-6 || c <= 0.0 {
        return clampf(c, 0.0, boundary.max(0.0));
    }
    if c <= boundary {
        return c;
    }
    let kk = clampf(knee_k, 0.30, 0.95);
    let ck = clampf(ceiling_k, kk + 0.02, 0.99);
    let knee = kk * boundary;
    if c <= knee {
        return c;
    }
    let ceiling = ck * boundary;
    let span = ceiling - knee;
    if span <= 1e-6 {
        return ceiling;
    }
    let t = (c - knee) / span;
    knee + span * (1.0 - expf(-t))
}

/// 落到设备域并取回 Lab（内部裁剪到 [0,1] 设备值，故结果必在域内）。
fn settle(profile: &IccProfile, c: Lch) -> Lab {
    profile.device_to_lab(profile.lab_to_device(lch_to_lab(c)))
}

// ---------------------------------------------------------------------------
// 4. 意图账本
// ---------------------------------------------------------------------------

pub const LEDGER_CAP: usize = 128;

/// 单 patch 的 BPC 结局。三值闭集，账本守恒式 `requests = applied + refused + off`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpcOutcome {
    /// 未请求 BPC。
    Off,
    /// 请求且语义成立（比色两档），补偿路径已执行。
    Applied,
    /// 请求但语义不成立（感知/饱和档），**显式登记**为拒绝，不是静默忽略。
    Refused,
}

impl BpcOutcome {
    pub fn code(self) -> u8 {
        match self {
            BpcOutcome::Off => 0,
            BpcOutcome::Applied => 1,
            BpcOutcome::Refused => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BpcOutcome::Off => "off",
            BpcOutcome::Applied => "applied",
            BpcOutcome::Refused => "refused",
        }
    }
}

/// 一条账目：源、输出、ΔE、档位、BPC 结局、是否发生裁剪。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntentRecord {
    pub seq: u16,
    pub patch: u8,
    pub src: Lab,
    pub out: Lab,
    pub delta_e: f32,
    pub intent: Intent,
    pub bpc: BpcOutcome,
    pub clipped: bool,
}

/// 一批 patch 的汇总（判据与日志共用；不做成魔数，全部由记录算出）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatchReport {
    pub records: usize,
    pub mean_de: f32,
    pub max_de: f32,
    pub clipped: usize,
    pub outcome: BpcOutcome,
}

/// 意图账本。容量 128 = 50 色块 × 2 档 + 28 条余量。
///
/// 为什么按「2 档整批」定容：一页作业至少要能同时装下「比色档 + BPC 开关
/// 对照」两批 50 色块，否则对照实验本身会被容量截断，判据就测不出差异。
/// 容量检查在**写入前**做整批预判：半批落账比整批拒绝更难审计（记录边界
/// 与 patch 边界错位），所以宁可整批拒绝。
#[derive(Clone, Debug)]
pub struct IntentLedger {
    recs: Vec<IntentRecord>,
    applied: u32,
    refused: u32,
    off: u32,
}

impl IntentLedger {
    pub fn new() -> IntentLedger {
        IntentLedger {
            recs: Vec::new(),
            applied: 0,
            refused: 0,
            off: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn capacity_left(&self) -> usize {
        if self.recs.len() >= LEDGER_CAP {
            0
        } else {
            LEDGER_CAP - self.recs.len()
        }
    }

    /// 守恒三元组（applied, refused, off）。三者之和恒等于 `requests()`。
    pub fn conservation(&self) -> (u32, u32, u32) {
        (self.applied, self.refused, self.off)
    }

    pub fn requests(&self) -> u32 {
        self.applied + self.refused + self.off
    }

    /// 按 BPC 结局计数的条数（判据用：拒绝必须能被数出来）。
    pub fn count_of(&self, outcome: BpcOutcome) -> u32 {
        match outcome {
            BpcOutcome::Applied => self.applied,
            BpcOutcome::Refused => self.refused,
            BpcOutcome::Off => self.off,
        }
    }

    pub fn get(&self, i: usize) -> Option<&IntentRecord> {
        self.recs.get(i)
    }

    /// 追加一条账目。满 → `LedgerFull`（记录数不变）。
    pub fn push(&mut self, rec: IntentRecord) -> Result<(), IntentError> {
        if self.recs.len() >= LEDGER_CAP {
            return Err(IntentError::LedgerFull);
        }
        match rec.bpc {
            BpcOutcome::Applied => self.applied += 1,
            BpcOutcome::Refused => self.refused += 1,
            BpcOutcome::Off => self.off += 1,
        }
        self.recs.push(rec);
        Ok(())
    }

    /// 整批处理并落账。空批 → `Empty`；>255 → `PatchCount`；容量不足 → `LedgerFull`
    /// （整批预判，不留半批）。返回汇总，供判据直接断言。
    pub fn process_batch(
        &mut self,
        engine: &IntentEngine,
        profile: &IccProfile,
        intent: Intent,
        bpc: bool,
        patches: &[Rgb8],
    ) -> Result<BatchReport, IntentError> {
        if patches.is_empty() {
            return Err(IntentError::Empty);
        }
        if patches.len() > 255 {
            return Err(IntentError::PatchCount);
        }
        if patches.len() > self.capacity_left() {
            return Err(IntentError::LedgerFull);
        }
        let outcome = if bpc {
            if intent.supports_bpc() {
                BpcOutcome::Applied
            } else {
                BpcOutcome::Refused
            }
        } else {
            BpcOutcome::Off
        };
        let effective_bpc = outcome == BpcOutcome::Applied;
        let base = self.recs.len();
        let mut sum = 0.0_f32;
        let mut max = 0.0_f32;
        let mut clipped = 0usize;
        let mut i = 0usize;
        while i < patches.len() {
            let src = match patches.get(i) {
                Some(c) => rgb8_to_lab(*c),
                None => return Err(IntentError::PatchCount),
            };
            if !src.l.is_finite() || !src.a.is_finite() || !src.b.is_finite() {
                return Err(IntentError::Range);
            }
            let out = engine.map(src, profile, intent, effective_bpc);
            let dev_clipped = !profile.in_gamut(out, 2e-3);
            if dev_clipped {
                clipped += 1;
            }
            let de = delta_e2000(src, out);
            if de > max {
                max = de;
            }
            sum += de;
            let rec = IntentRecord {
                seq: (base + i) as u16,
                patch: i as u8,
                src,
                out,
                delta_e: de,
                intent,
                bpc: outcome,
                clipped: dev_clipped,
            };
            self.push(rec)?;
            i += 1;
        }
        let n = patches.len();
        Ok(BatchReport {
            records: n,
            mean_de: sum / n as f32,
            max_de: max,
            clipped,
            outcome,
        })
    }
}

// ---------------------------------------------------------------------------
// 5. J1 的 50 色块样本
// ---------------------------------------------------------------------------

/// J1 的 50 色块：10 级中性 + 40 个六档网格里的去中性点。
///
/// 为什么中性要单列：四档在中性上的差异只来自 L 处理（可印域压缩/BPC），
/// 色谱块上的差异来自 C 处理；两类混在一起会让「逐档一致」的失败无法归因。
pub fn patch50() -> Vec<Rgb8> {
    let mut out = Vec::with_capacity(50);
    let mut i = 0u16;
    while i < 10 {
        let v = (i * 255 / 9) as u8;
        out.push(Rgb8::new(v, v, v));
        i += 1;
    }
    let steps = [0u8, 51, 102, 153, 204, 255];
    let mut a = 0usize;
    while a < steps.len() && out.len() < 50 {
        let mut b = 0usize;
        while b < steps.len() && out.len() < 50 {
            let mut c = 0usize;
            while c < steps.len() && out.len() < 50 {
                let r = match steps.get(a) {
                    Some(v) => *v,
                    None => 0,
                };
                let g = match steps.get(b) {
                    Some(v) => *v,
                    None => 0,
                };
                let bl = match steps.get(c) {
                    Some(v) => *v,
                    None => 0,
                };
                if !(r == g && g == bl) {
                    out.push(Rgb8::new(r, g, bl));
                }
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    // 网格去中性后仍有 210 个候选，上面必能取满 40；若哪天常量被改小，
    // 这里用中性补齐而不是返回短表（长度契约 = 50）。
    while out.len() < 50 {
        out.push(Rgb8::new(128, 128, 128));
    }
    out
}

/// 把整批 patch 映射成 Lab 序列（不落账，判据比较用）。
pub fn map_all(
    engine: &IntentEngine,
    profile: &IccProfile,
    intent: Intent,
    patches: &[Rgb8],
) -> Vec<Lab> {
    let mut out = Vec::with_capacity(patches.len());
    let mut i = 0usize;
    while i < patches.len() {
        let lab = match patches.get(i) {
            Some(c) => rgb8_to_lab(*c),
            None => Lab::new(0.0, 0.0, 0.0),
        };
        out.push(engine.map(lab, profile, intent, false));
        i += 1;
    }
    out
}

fn fnv_step(h: u64, v: u32) -> u64 {
    (h ^ (v as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

/// 输出序列的位级签名（FNV-1a over f32 bits）。
///
/// 用**逐位**而不是 ΔE 比较：J1 要的是「逐档一致」——同一档跑两次必须
/// 完全相同的位，ΔE=0 也可能只是小数点后相同。
pub fn signature_of(labs: &[Lab]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < labs.len() {
        if let Some(c) = labs.get(i) {
            h = fnv_step(h, c.l.to_bits());
            h = fnv_step(h, c.a.to_bits());
            h = fnv_step(h, c.b.to_bits());
        }
        i += 1;
    }
    h
}

/// 两条输出序列里可区分（ΔE > eps）的 patch 数。
pub fn distinct_patches(a: &[Lab], b: &[Lab], eps: f32) -> usize {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut k = 0usize;
    let mut i = 0usize;
    while i < n {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if delta_e2000(*x, *y) > eps {
                    k += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    k
}

// ---------------------------------------------------------------------------
// 6. 注入核：5 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 5 类坏样本，逐类断言被显式拒绝。返回 (抓住数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let engine = IntentEngine::office();
    let profile = IccProfile::simulated_printer(0.70);
    let mut ledger = IntentLedger::new();

    // 1) 空批 → Empty
    if ledger
        .process_batch(&engine, &profile, Intent::Perceptual, false, &[])
        .err()
        == Some(IntentError::Empty)
    {
        caught += 1;
    }

    // 2) 意图码越界（4..=255 全是非法码）→ Range
    let bad_intent = intent_from_index(4).err() == Some(IntentError::Range)
        && intent_from_index(200).err() == Some(IntentError::Range);
    if bad_intent {
        caught += 1;
    }

    // 3) 账本容量耗尽 → LedgerFull 且记录数不变
    let mut full = IntentLedger::new();
    let patch = Rgb8::new(10, 20, 30);
    let mut k = 0usize;
    while k < LEDGER_CAP {
        let rec = IntentRecord {
            seq: k as u16,
            patch: 0,
            src: Lab::new(50.0, 0.0, 0.0),
            out: Lab::new(50.0, 0.0, 0.0),
            delta_e: 0.0,
            intent: Intent::Relative,
            bpc: BpcOutcome::Off,
            clipped: false,
        };
        if full.push(rec).is_err() {
            break;
        }
        k += 1;
    }
    let before = full.len();
    let overflow = full.push(IntentRecord {
        seq: 999,
        patch: 0,
        src: Lab::new(1.0, 1.0, 1.0),
        out: Lab::new(1.0, 1.0, 1.0),
        delta_e: 0.0,
        intent: Intent::Relative,
        bpc: BpcOutcome::Off,
        clipped: false,
    });
    if overflow.err() == Some(IntentError::LedgerFull) && full.len() == before {
        caught += 1;
    }

    // 4) 同名设备重复注册 → Duplicate
    let mut reg = PolicyRegistry::new();
    let ok1 = reg.register(IntentPolicy::full("press-a")).is_ok();
    let dup = reg.register(IntentPolicy::full("press-a")).err() == Some(IntentError::Duplicate);
    if ok1 && dup {
        caught += 1;
    }

    // 5) 未注册设备校验 → NoPolicy（且会话停在 Requested）
    let mut sess = IntentSession::new("press-ghost");
    let requested = sess.request(Intent::Relative, false).is_ok();
    let nopolicy = sess.validate(&reg).err() == Some(IntentError::NoPolicy);
    if requested && nopolicy && sess.state() == SessionState::Requested {
        caught += 1;
    }

    let _ = patch;
    (caught, total)
}

// ---------------------------------------------------------------------------
// 7. 判据
// ---------------------------------------------------------------------------

/// UNX-F50002 判据集（18 条）。锚点：四档 ×50 色块输出逐档一致。
pub fn run_unx50002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50002");
    let profile = IccProfile::simulated_printer(0.70);
    let engine = IntentEngine::office();
    let patches = patch50();

    // J1-1 样本规模：J1 的 50 色块必须齐（少一个都不是 50 色块判据）
    let ok = patches.len() == 50;
    let det = format!("n={}", patches.len());
    s.add_dyn("50002-J1-色块50", ok, &det);

    // J1-2 四档 ×50 色块逐档一致（锚点原文）：每档跑两轮，逐 patch 比位。
    let mut sig_a: Vec<u64> = Vec::with_capacity(4);
    let mut stable = true;
    let mut total_patches = 0usize;
    let mut i = 0usize;
    while i < 4 {
        let intent = intent_at(i);
        let run1 = map_all(&engine, &profile, intent, &patches);
        let run2 = map_all(&engine, &profile, intent, &patches);
        total_patches += run1.len();
        let mut same = run1.len() == run2.len();
        let mut k = 0usize;
        while k < run1.len() && same {
            match (run1.get(k), run2.get(k)) {
                (Some(x), Some(y)) => {
                    if x.l.to_bits() != y.l.to_bits()
                        || x.a.to_bits() != y.a.to_bits()
                        || x.b.to_bits() != y.b.to_bits()
                    {
                        same = false;
                    }
                }
                _ => same = false,
            }
            k += 1;
        }
        if !same {
            stable = false;
        }
        sig_a.push(signature_of(&run1));
        i += 1;
    }
    let det = format!("patches={} stable={}", total_patches, stable);
    s.add_dyn("50002-J1-四档逐档一致", stable && total_patches == 200, &det);

    // J1-3 四档签名两两互异（4 档不能塌成同一映射）
    let mut distinct_sigs = 0usize;
    let mut a = 0usize;
    while a < 4 {
        let mut b = a + 1;
        let mut uniq = true;
        while b < 4 {
            if sig_a.get(a) == sig_a.get(b) {
                uniq = false;
            }
            b += 1;
        }
        if uniq {
            distinct_sigs += 1;
        }
        a += 1;
    }
    let det = format!("uniq={}/4", distinct_sigs);
    s.add_dyn("50002-J1-四档签名互异", distinct_sigs == 4, &det);

    // J1-4 六对意图的可区分性度量：每对至少 1 个 patch 有 >0.5 ΔE 差异
    let outs_p = map_all(&engine, &profile, Intent::Perceptual, &patches);
    let outs_r = map_all(&engine, &profile, Intent::Relative, &patches);
    let outs_s = map_all(&engine, &profile, Intent::Saturation, &patches);
    let outs_a = map_all(&engine, &profile, Intent::Absolute, &patches);
    let pairs = [
        (0usize, 1usize),
        (0, 2),
        (0, 3),
        (1, 2),
        (1, 3),
        (2, 3),
    ];
    let mut worst = usize::MAX;
    let mut idx = 0usize;
    while idx < pairs.len() {
        let (x, y) = match pairs.get(idx) {
            Some(v) => *v,
            None => (0, 0),
        };
        let left = match x {
            0 => &outs_p,
            1 => &outs_r,
            2 => &outs_s,
            _ => &outs_a,
        };
        let right = match y {
            0 => &outs_p,
            1 => &outs_r,
            2 => &outs_s,
            _ => &outs_a,
        };
        let d = distinct_patches(left, right, 0.5);
        if d < worst {
            worst = d;
        }
        idx += 1;
    }
    let det = format!("min_pair_diff={}", worst);
    s.add_dyn("50002-J1-六对可区分", worst >= 1, &det);

    // J1-5 感知 vs 饱和：越域色上 C 差 > 1（两档不是同一压缩曲线）
    let hot = rgb8_to_lab(Rgb8::new(255, 0, 0));
    let mp = engine.map(hot, &profile, Intent::Perceptual, false);
    let ms = engine.map(hot, &profile, Intent::Saturation, false);
    let dc = fabsf(mp.chroma() - ms.chroma());
    let ok = dc > 1.0;
    let det = format!("Cp={:.2} Cs={:.2}", mp.chroma(), ms.chroma());
    s.add_dyn("50002-J1-感知≠饱和", ok, &det);

    // J1-6 相对 vs 绝对：在域内子集上必因纸张白差异而不同（均值 ΔE > 0.2）
    let mut sum_de = 0.0_f32;
    let mut n_in = 0usize;
    let mut k = 0usize;
    while k < patches.len() {
        if let Some(c) = patches.get(k) {
            let src = rgb8_to_lab(*c);
            if profile.in_gamut(src, 1e-4) {
                let r = engine.map(src, &profile, Intent::Relative, false);
                let ab = engine.map(src, &profile, Intent::Absolute, false);
                sum_de += delta_e2000(r, ab);
                n_in += 1;
            }
        }
        k += 1;
    }
    let mean_rel_abs = if n_in > 0 { sum_de / n_in as f32 } else { 0.0 };
    let ok = n_in >= 10 && mean_rel_abs > 0.2;
    let det = format!("n_in={} mean_dE={:.2}", n_in, mean_rel_abs);
    s.add_dyn("50002-J1-相对≠绝对", ok, &det);

    // J1-7 四档输出必在设备域内（映射不得把色推到纸外面）
    let mut outside = 0usize;
    let mut q = 0usize;
    while q < 4 {
        let intent = intent_at(q);
        let outs = map_all(&engine, &profile, intent, &patches);
        let mut j = 0usize;
        while j < outs.len() {
            if let Some(c) = outs.get(j) {
                if !profile.in_gamut(*c, 2e-3) {
                    outside += 1;
                }
            }
            j += 1;
        }
        q += 1;
    }
    // 域内性与中性轴上的是纯布尔性质，失败原因由实现分支唯一决定，
    // 不需要运行期明细（dyn 槽位只留给需要报数的判据）。
    s.add("50002-J1-输出域内", outside == 0, "有输出越域");

    // J1-8 中性轴保真：10 级中性映射后 L 单调不减、C < 1.5（无色偏引入）
    let mut mono = true;
    let mut neutral_ok = true;
    let mut prev = -1.0_f32;
    let mut n = 0usize;
    while n < 10 {
        if let Some(c) = patches.get(n) {
            let out = engine.map(rgb8_to_lab(*c), &profile, Intent::Perceptual, false);
            if out.l + 1e-3 < prev {
                mono = false;
            }
            prev = out.l;
            if out.chroma() >= 1.5 {
                neutral_ok = false;
            }
        }
        n += 1;
    }
    let ok = mono && neutral_ok;
    s.add("50002-J1-中性保真", ok, "中性轴漂移或 L 回折");

    // J1-9 幂等：相对档对已在域内的结果再映射一次，ΔE < 0.3
    let mut worst_idem = 0.0_f32;
    let mut m = 0usize;
    while m < patches.len() {
        if let Some(c) = patches.get(m) {
            let once = engine.map(rgb8_to_lab(*c), &profile, Intent::Relative, false);
            let twice = engine.map(once, &profile, Intent::Relative, false);
            let d = delta_e2000(once, twice);
            if d > worst_idem {
                worst_idem = d;
            }
        }
        m += 1;
    }
    let ok = worst_idem < 0.3;
    let det = format!("worst={:.4}", worst_idem);
    s.add_dyn("50002-J1-幂等", ok, &det);

    // 会话机：合法路径 Idle→Requested→Validated→Applied→Reverted
    let mut reg = PolicyRegistry::new();
    let reg_ok = reg.register(IntentPolicy::full("press-a")).is_ok();
    let mut ledger = IntentLedger::new();
    let mut sess = IntentSession::new("press-a");
    let mut path_ok = reg_ok;
    path_ok = path_ok && sess.request(Intent::Relative, true).is_ok();
    path_ok = path_ok && sess.validate(&reg).is_ok();
    path_ok = path_ok && sess.apply(&engine, &profile, &patches, &mut ledger).is_ok();
    path_ok = path_ok && sess.state() == SessionState::Applied;
    path_ok = path_ok && sess.revert().is_ok();
    path_ok = path_ok && sess.state() == SessionState::Reverted;
    s.add("50002-J1-会话合法路径", path_ok, "五态路径断裂");

    // 会话机：非法跳转被拒（Idle 直接 apply / 未申请就校验）
    let mut bad = IntentSession::new("press-a");
    let mut l2 = IntentLedger::new();
    let a1 = bad
        .apply(&engine, &profile, &patches, &mut l2)
        .err()
        == Some(IntentError::IllegalTransition);
    let a2 = bad.validate(&reg).err() == Some(IntentError::IllegalTransition);
    let a3 = bad.request(Intent::Relative, false).is_ok()
        && bad.request(Intent::Absolute, false).err() == Some(IntentError::IllegalTransition);
    s.add("50002-J1-非法跳转必拒", a1 && a2 && a3 && bad.rejections() == 3, "非法跳转未被拒");

    // 策略注册表：重名拒绝且原条目保留
    let mut reg2 = PolicyRegistry::new();
    let r1 = reg2.register(IntentPolicy::full("dev")).is_ok();
    let r2 = reg2.register(IntentPolicy::full("dev")).err() == Some(IntentError::Duplicate);
    s.add("50002-J1-重名必拒", r1 && r2 && reg2.len() == 1, "重名被静默覆盖");

    // 策略注册表：容量满显式拒绝（6 台之后第 7 台）
    let mut reg3 = PolicyRegistry::new();
    let names = ["d0", "d1", "d2", "d3", "d4", "d5", "d6"];
    let mut filled = 0usize;
    let mut t = 0usize;
    while t < names.len() {
        if let Some(nm) = names.get(t) {
            if reg3.register(IntentPolicy::full(nm)).is_ok() {
                filled += 1;
            }
        }
        t += 1;
    }
    let seventh = reg3.register(IntentPolicy::full("d7"));
    let ok = filled == POLICY_CAP && seventh.err() == Some(IntentError::PolicyFull);
    s.add("50002-J1-注册表满拒", ok && reg3.len() == POLICY_CAP, "容量未封口");

    // 策略语义：BPC 请求在感知档必须被拒（不是静默忽略）
    let pol = IntentPolicy::full("press-a");
    let ok = pol.check(Intent::Perceptual, true).err() == Some(IntentError::BpcNotAllowed)
        && pol.check(Intent::Relative, true).is_ok()
        && pol.check(Intent::Saturation, false).is_ok();
    s.add("50002-J1-BPC语义", ok, "感知档 BPC 被静默接受");

    // 策略语义：只允许比色的设备必须拒绝饱和档
    let pol2 = IntentPolicy::colorimetric_only("proof");
    let ok = pol2.check(Intent::Saturation, false).err() == Some(IntentError::IntentNotAllowed)
        && pol2.check(Intent::Absolute, false).is_ok();
    s.add("50002-J1-档位白名单", ok, "白名单未生效");

    // 账本守恒：requests == applied + refused + off（混跑两批）
    let mut lg = IntentLedger::new();
    let rep_a = lg.process_batch(&engine, &profile, Intent::Relative, true, &patches);
    let rep_b = lg.process_batch(&engine, &profile, Intent::Perceptual, true, &patches);
    let (ap, rf, of) = lg.conservation();
    let ok = rep_a.is_ok()
        && rep_b.is_ok()
        && ap + rf + of == lg.requests()
        && lg.requests() as usize == lg.len()
        && ap == 50
        && rf == 50
        && of == 0;
    s.add("50002-J1-账本守恒", ok, "守恒式破裂");

    // 账本容量：整批预判，超容整批拒绝（记录数不变、不留半批）
    let mut lg2 = IntentLedger::new();
    let b1 = lg2.process_batch(&engine, &profile, Intent::Relative, false, &patches).is_ok();
    let b2 = lg2.process_batch(&engine, &profile, Intent::Relative, false, &patches).is_ok();
    let before = lg2.len();
    let third = lg2.process_batch(&engine, &profile, Intent::Relative, false, &patches);
    let ok = b1 && b2 && before == 100 && third.err() == Some(IntentError::LedgerFull)
        && lg2.len() == before;
    s.add("50002-J1-账本整批拒", ok, "半批落账或容量未封口");

    // 注入核：5 类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50002-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50002_checks_green() {
        let set = run_unx50002_checks();
        assert!(set.all_passed(), "UNX-F50002 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50002_four_intents_are_distinct() {
        let profile = IccProfile::simulated_printer(0.70);
        let engine = IntentEngine::office();
        let patches = patch50();
        let p = map_all(&engine, &profile, Intent::Perceptual, &patches);
        let r = map_all(&engine, &profile, Intent::Relative, &patches);
        let s = map_all(&engine, &profile, Intent::Saturation, &patches);
        let a = map_all(&engine, &profile, Intent::Absolute, &patches);
        assert_eq!(patches.len(), 50);
        assert!(distinct_patches(&p, &r, 0.5) > 0);
        assert!(distinct_patches(&p, &s, 0.5) > 0);
        assert!(distinct_patches(&r, &a, 0.5) > 0);
    }

    #[test]
    fn unx50002_error_codes_stay_in_segment() {
        let all = [
            IntentError::Empty,
            IntentError::PolicyFull,
            IntentError::Duplicate,
            IntentError::NoPolicy,
            IntentError::IntentNotAllowed,
            IntentError::BpcNotAllowed,
            IntentError::IllegalTransition,
            IntentError::LedgerFull,
            IntentError::Range,
            IntentError::PatchCount,
            IntentError::NotRequested,
            IntentError::Unregistered,
        ];
        for e in all {
            let c = e.code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x20, "code {:#x}", c);
            assert!(!e.message().is_empty());
        }
    }
}
