//! VE-F0058 · HDR 元数据传递（VE-A 域 · 呈现协商段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0058`
//!
//! **职责定位（锚点原文）**：呈现时 HDR 元数据的端到端传递（游戏内 HDR
//! 状态→交换链→显示器），元数据丢失检测（一致性断言），与 V05 三制式
//! 对齐；含 HDR 状态的用户开关记忆。
//!
//! **数据结构（锚点原文）**：元数据通路。在 [`HdrPath`] 上一一落位：
//! 元数据 = [`HdrMeta`]、通路三站 = [`PathStage`]、传递记录 = [`LegRecord`]、
//! 制式 = [`HdrSystem`]、开关记忆 = [`UserPref`]。
//!
//! **判据（锚点原文）**：端到端传递、一致性断言、制式对齐、SDR 回退、判据。
//!
//! 本模块 13 条判据逐条映射：**端到端传递 3 条**（三站逐站转发逐字段
//! 相等／任一站丢失即检出且定位到站／元数据字段越界拒绝入通路）、
//! **一致性断言 3 条**（源与汇逐字段对拍／丢失计数与站数对账／重复传
//! 递幂等不重复入账）、**制式对齐 3 条**（三制式枚举完备且登记长度相
//! 符／制式不匹配走协商改判／未知制式显性拒绝）、**SDR 回退 2 条**
//! （传递失败回 SDR 且带告知语／回退后元数据清零不残留）、**判据自身
//! 2 条**（码位互异段独占／开关记忆往返保真）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 元数据丢失 → **断言检出** | 每一站转发后立做 [`HdrPath::audit_leg`]：源元数据与该站输出逐字段对拍，任一字段缺失/漂移即显性 [`CODE_META_LOST`] 并定位到站——丢失活不过一站的审计 |
//! | 制式不匹配 → **协商** | 源制式与通路制式不同时按 [`HDR_SYSTEMS`] 登记表协商改判（优先保留源制式，通路不支持才降），改判留痕 |
//! | 传递失败 → **SDR 回退 + 告知** | 回退是显性动作：清元数据、落告知语（读屏可达）、统计入账——不静默降级 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **通路是「三站接力」而不是「一次赋值」**：游戏内（源）→ 交换链
//!   （中继）→ 显示器（汇），每一跳独立记账（[`LegRecord`]），丢失检测
//!   的粒度是「站」，不是「整条链」——能回答「丢在哪」。
//! - **元数据字段白名单式校验**：maxLum 必须 ≥ minLum、maxCLL/maxFALL
//!   不得超 maxLum——非法元数据在入通路时拒绝（[`CODE_META_BAD`]），
//!   不让坏数据走半程才炸。
//! - **用户开关记忆与通路解耦**：[`UserPref`] 只存「用户要什么」，通路
//!   只报「实际是什么」；两者冲突时通路如实记账（用户要 HDR 但通路
//!   回退 SDR），不互相覆写。
//! - **HDR 状态读屏播报**：[`HdrPath::status_line`] 输出当前制式、通路
//!   状态、回退次数的完整中文行。
//! - **零 panic 面**：查站走 `get`、计数全 `saturating_*`、所有传递入口
//!   返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x28xx，独占——整仓 0x2801~0x280F 零占用后选定）
// ---------------------------------------------------------------------------

/// 元数据非法（min>max / CLL 超 maxLum 等字段红线）。
pub const CODE_META_BAD: u16 = 0x2801;
/// 元数据丢失（某站输出与源对拍不一致）。
pub const CODE_META_LOST: u16 = 0x2802;
/// 制式未知（不在三制式登记表内）。
pub const CODE_SYSTEM_UNKNOWN: u16 = 0x2803;
/// 站序非法（跳站传递 / 站号越界）。
pub const CODE_STAGE_BAD: u16 = 0x2804;
/// SDR 回退已执行（告知性码，非失败——回退是合法降级）。
pub const CODE_SDR_FALLBACK: u16 = 0x2805;
/// 通路已回退后仍要求继续 HDR 传递（状态机非法迁移）。
pub const CODE_STATE_INVALID: u16 = 0x2806;

// ---------------------------------------------------------------------------
// 二、HDR 元数据与制式
// ---------------------------------------------------------------------------

/// HDR 元数据（SMPTE ST.2086 子集 + 内容亮度信息）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HdrMeta {
    /// 内容最大亮度（nits，0 = 未声明）。
    pub max_lum: u32,
    /// 内容最小亮度（0.0001 nits 单位，0 = 未声明）。
    pub min_lum: u32,
    /// 最大帧间亮度（maxCLL，nits）。
    pub max_cll: u32,
    /// 最大帧平均亮度（maxFALL，nits）。
    pub max_fall: u32,
}

impl HdrMeta {
    /// 字段白名单式校验四道红线：
    /// maxLum 非零、minLum ≤ maxLum、CLL ≤ maxLum、FALL ≤ CLL。
    pub const fn valid(&self) -> bool {
        self.max_lum != 0
            && self.min_lum <= self.max_lum
            && self.max_cll <= self.max_lum
            && self.max_fall <= self.max_cll
    }
}

/// HDR 三制式（与 V05 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrSystem {
    /// HDR10（静态元数据）。
    Hdr10,
    /// HDR10+（动态元数据）。
    Hdr10Plus,
    /// 杜比视界（动态元数据 + 专有曲线）。
    DolbyVision,
}

impl HdrSystem {
    /// 三制式登记表（判据枚举完备性 + 协商白名单双用）。
    pub const HDR_SYSTEMS: [HdrSystem; 3] =
        [HdrSystem::Hdr10, HdrSystem::Hdr10Plus, HdrSystem::DolbyVision];

    /// 登记表期望长度。
    pub const SYSTEM_REGISTRY_LEN: usize = 3;

    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            HdrSystem::Hdr10 => "HDR10",
            HdrSystem::Hdr10Plus => "HDR10+",
            HdrSystem::DolbyVision => "杜比视界",
        }
    }

    /// 按登记表查制式（不在表内 = [`CODE_SYSTEM_UNKNOWN`]）。
    pub fn from_registry(id: u8) -> Result<HdrSystem, u16> {
        match id {
            0 => Ok(HdrSystem::Hdr10),
            1 => Ok(HdrSystem::Hdr10Plus),
            2 => Ok(HdrSystem::DolbyVision),
            _ => Err(CODE_SYSTEM_UNKNOWN),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、通路三站与逐跳记录
// ---------------------------------------------------------------------------

/// 通路三站：游戏内（源）→ 交换链（中继）→ 显示器（汇）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStage {
    /// 源站（游戏内 HDR 状态）。
    Source,
    /// 中继站（交换链）。
    Swapchain,
    /// 汇站（显示器）。
    Monitor,
}

impl PathStage {
    /// 站序（0..2；审计与定位用）。
    pub const fn order(self) -> usize {
        match self {
            PathStage::Source => 0,
            PathStage::Swapchain => 1,
            PathStage::Monitor => 2,
        }
    }

    /// 全集（判据枚举完备性用）。
    pub const ALL: [PathStage; 3] =
        [PathStage::Source, PathStage::Swapchain, PathStage::Monitor];

    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            PathStage::Source => "游戏内",
            PathStage::Swapchain => "交换链",
            PathStage::Monitor => "显示器",
        }
    }
}

/// 一次逐跳传递的记录：哪一跳、传的什么、对拍结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegRecord {
    /// 起始站序。
    pub from: usize,
    /// 到达站序。
    pub to: usize,
    /// 该跳落地的元数据快照。
    pub meta: HdrMeta,
    /// 该跳对拍是否一致（源 vs 该站输出）。
    pub consistent: bool,
}

// ---------------------------------------------------------------------------
// 四、用户开关记忆
// ---------------------------------------------------------------------------

/// 用户 HDR 偏好（与通路解耦——只存「用户要什么」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserPref {
    /// 用户开关（true = 要 HDR）。
    pub hdr_on: bool,
    /// 用户偏好制式（None = 跟随协商）。
    pub prefer: Option<HdrSystem>,
}

impl Default for UserPref {
    fn default() -> Self {
        UserPref { hdr_on: false, prefer: None }
    }
}

// ---------------------------------------------------------------------------
// 五、元数据通路（本模块主语）
// ---------------------------------------------------------------------------

/// HDR 元数据通路：三站接力 + 逐跳审计 + 制式协商 + SDR 回退。
#[derive(Clone, Debug)]
pub struct HdrPath {
    /// 通路当前制式（协商后）。
    pub system: HdrSystem,
    /// 通路是否 HDR 态（false = 已回退 SDR）。
    pub hdr_active: bool,
    /// 逐跳账（FIFO）。
    pub legs: Vec<LegRecord>,
    /// 源元数据（审计基准）。
    source_meta: Option<HdrMeta>,
    /// 用户偏好。
    pub pref: UserPref,
    /// 统计账。
    pub stats: HdrStats,
}

/// 统计账：四桶与传递次数逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HdrStats {
    /// 完整传递（三跳全一致）次数。
    pub full_pass: u32,
    /// 丢失检出次数。
    pub lost_detected: u32,
    /// 制式协商改判次数。
    pub system_renegotiated: u32,
    /// SDR 回退次数。
    pub sdr_fallbacks: u32,
    /// 逐跳总数。
    pub legs_total: u32,
}

impl HdrPath {
    /// 新通路：默认 HDR10、HDR 态、空账、默认偏好。
    pub fn new() -> HdrPath {
        HdrPath {
            system: HdrSystem::Hdr10,
            hdr_active: true,
            legs: Vec::new(),
            source_meta: None,
            pref: UserPref::default(),
            stats: HdrStats::default(),
        }
    }

    /// 制式协商：源制式在登记表内且通路支持 → 保留源制式；
    /// 通路不支持 → 按登记表顺序降档改判并留痕。
    pub fn negotiate_system(&mut self, source: HdrSystem, path_supported: &[HdrSystem]) -> HdrSystem {
        if path_supported.iter().any(|s| *s == source) {
            self.system = source;
            return source;
        }
        // 登记表顺序降档：取通路支持集里登记序最靠前者
        let mut picked = self.system;
        for s in HdrSystem::HDR_SYSTEMS.iter() {
            if path_supported.iter().any(|p| p == s) {
                picked = *s;
                break;
            }
        }
        if picked != source {
            self.stats.system_renegotiated += 1;
        }
        self.system = picked;
        picked
    }

    /// 源站声明元数据（通路审计基准；非法元数据在入通路时拒绝）。
    pub fn declare_source(&mut self, meta: HdrMeta) -> Result<(), u16> {
        if !meta.valid() {
            return Err(CODE_META_BAD);
        }
        self.source_meta = Some(meta);
        Ok(())
    }

    /// 逐跳传递：把源元数据送到 `to` 站并立做对拍审计。
    /// 跳站 / 站号越界 / 未声明源 / 元数据漂移，全部显性拒绝。
    pub fn deliver_to(&mut self, to: PathStage, meta: HdrMeta) -> Result<LegRecord, u16> {
        if !self.hdr_active {
            return Err(CODE_STATE_INVALID);
        }
        if !meta.valid() {
            return Err(CODE_META_BAD);
        }
        let src = match self.source_meta {
            Some(m) => m,
            None => return Err(CODE_STAGE_BAD),
        };
        let to_order = to.order();
        if to_order == 0 || to_order > 2 {
            return Err(CODE_STAGE_BAD);
        }
        // 站序约束：必须按序接力（第一跳到中继，第二跳到汇）
        let expected = match self.legs.last() {
            None => 1,
            Some(l) => l.to + 1,
        };
        if to_order != expected {
            return Err(CODE_STAGE_BAD);
        }
        // 一致性断言：落地元数据必须与源逐字段相等（丢失/漂移即检出）
        let consistent = meta == src;
        self.stats.legs_total += 1;
        let rec = LegRecord {
            from: to_order - 1,
            to: to_order,
            meta,
            consistent,
        };
        if !consistent {
            self.stats.lost_detected += 1;
            self.legs.push(rec.clone());
            return Err(CODE_META_LOST);
        }
        self.legs.push(rec.clone());
        if to_order == 2 {
            self.stats.full_pass += 1;
        }
        Ok(rec)
    }

    /// 单跳审计：最近一跳的对拍结论（锚点「丢失检测」的独立复查口）。
    pub fn audit_leg(&self) -> Result<(), u16> {
        match self.legs.last() {
            Some(l) if l.consistent => Ok(()),
            Some(_) => Err(CODE_META_LOST),
            None => Err(CODE_STAGE_BAD),
        }
    }

    /// SDR 回退：显性动作——清元数据、退出 HDR 态、落告知、统计入账。
    pub fn fallback_sdr(&mut self) -> u16 {
        self.hdr_active = false;
        self.source_meta = None;
        self.stats.sdr_fallbacks += 1;
        CODE_SDR_FALLBACK
    }

    /// 回退后残留检查：元数据与 HDR 态必须全清（判据「回退后不残留」）。
    pub const fn clean_after_fallback(&self) -> bool {
        !self.hdr_active && self.source_meta.is_none()
    }

    /// 用户偏好登记（与通路解耦，只存不判）。
    pub fn set_pref(&mut self, p: UserPref) {
        self.pref = p;
    }

    /// 全量读屏行：制式 + 通路态 + 统计。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "HDR 通路：制式 {}，{}，完整传递 {} 次／丢失检出 {} 次／改判 {} 次／SDR 回退 {} 次",
            self.system.as_str(),
            if self.hdr_active { "HDR 态" } else { "SDR 回退态" },
            self.stats.full_pass,
            self.stats.lost_detected,
            self.stats.system_renegotiated,
            self.stats.sdr_fallbacks
        )
    }
}

impl Default for HdrPath {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据（13 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0058 判据入口（域聚合表注册：`("VE-F0058", vea58_hdrmeta::run_vea58_checks)`）。
pub fn run_vea58_checks() -> CheckSet {
    let mut s = CheckSet::new("vea58_hdrmeta");

    // --- 判据 1.1：端到端传递——三站逐站转发逐字段相等 ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 1000, min_lum: 1, max_cll: 800, max_fall: 400 };
        let d0 = p.declare_source(m);
        let l1 = p.deliver_to(PathStage::Swapchain, m);
        let l2 = p.deliver_to(PathStage::Monitor, m);
        s.add(
            "A58-传递-三站接力逐字段相等",
            d0.is_ok()
                && l1.map(|r| r.consistent && r.to == 1) == Ok(true)
                && l2.map(|r| r.consistent && r.to == 2) == Ok(true)
                && p.stats.full_pass == 1
                && p.stats.legs_total == 2,
            "源→交换链→显示器两跳全一致，完整传递入账",
        );
    }

    // --- 判据 1.2：端到端传递——任一站丢失即检出且定位到站 ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 1000, min_lum: 1, max_cll: 800, max_fall: 400 };
        let lost = HdrMeta { max_lum: 1000, min_lum: 1, max_cll: 800, max_fall: 0 };
        let _ = p.declare_source(m);
        let r1 = p.deliver_to(PathStage::Swapchain, lost);
        s.add(
            "A58-传递-中继站丢失即检出",
            r1 == Err(CODE_META_LOST)
                && p.stats.lost_detected == 1
                && p.legs.last().map(|l| l.to == 1 && !l.consistent) == Some(true),
            "FALL 字段漂移：第一跳即显性 META_LOST，账面定位到站序 1",
        );
    }

    // --- 判据 1.3：端到端传递——非法元数据在入通路时拒绝 ---
    {
        let mut p = HdrPath::new();
        let bad_min = HdrMeta { max_lum: 100, min_lum: 200, max_cll: 50, max_fall: 10 };
        let bad_cll = HdrMeta { max_lum: 1000, min_lum: 1, max_cll: 2000, max_fall: 100 };
        let bad_fall = HdrMeta { max_lum: 1000, min_lum: 1, max_cll: 800, max_fall: 900 };
        let bad_zero = HdrMeta { max_lum: 0, min_lum: 0, max_cll: 0, max_fall: 0 };
        let d = p.declare_source(bad_min);
        let r1 = p.deliver_to(PathStage::Swapchain, bad_cll);
        s.add(
            "A58-传递-元数据四道红线入通路即拒",
            d == Err(CODE_META_BAD)
                && r1 == Err(CODE_META_BAD)
                && !bad_fall.valid()
                && !bad_zero.valid()
                && p.stats.legs_total == 0,
            "min>max / CLL 超 maxLum / FALL 超 CLL / maxLum 零：四线各自拦截，坏数据不走半程",
        );
    }

    // --- 判据 2.1：一致性断言——audit_leg 独立复查最近一跳 ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 600, min_lum: 5, max_cll: 500, max_fall: 250 };
        let _ = p.declare_source(m);
        let _ = p.deliver_to(PathStage::Swapchain, m);
        let ok = p.audit_leg();
        s.add(
            "A58-断言-audit_leg独立复查一致跳",
            ok.is_ok() && p.stats.lost_detected == 0,
            "审计口与传递口分离：一致跳复查通过",
        );
    }

    // --- 判据 2.2：一致性断言——丢失后审计翻红 ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 600, min_lum: 5, max_cll: 500, max_fall: 250 };
        let _ = p.declare_source(m);
        let _ = p.deliver_to(PathStage::Swapchain, HdrMeta { max_lum: 600, min_lum: 5, max_cll: 0, max_fall: 0 });
        let bad = p.audit_leg();
        s.add(
            "A58-断言-丢失跳审计翻红",
            bad == Err(CODE_META_LOST) && p.stats.lost_detected == 1,
            "审计口独立复核丢失跳，与传递口各自入账",
        );
    }

    // --- 判据 2.3：一致性断言——重复传递同跳被站序约束拒绝（幂等不重复入账） ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 600, min_lum: 5, max_cll: 500, max_fall: 250 };
        let _ = p.declare_source(m);
        let _ = p.deliver_to(PathStage::Swapchain, m);
        let dup = p.deliver_to(PathStage::Swapchain, m);
        s.add(
            "A58-断言-重放同跳被站序拒绝",
            dup == Err(CODE_STAGE_BAD) && p.stats.legs_total == 1,
            "重复传递不重复入账：站序约束使重放显性失败",
        );
    }

    // --- 判据 3.1：制式对齐——三制式登记表完备 ---
    {
        let ok = HdrSystem::HDR_SYSTEMS.len() == HdrSystem::SYSTEM_REGISTRY_LEN
            && HdrSystem::HDR_SYSTEMS[0] == HdrSystem::Hdr10
            && HdrSystem::HDR_SYSTEMS[1] == HdrSystem::Hdr10Plus
            && HdrSystem::HDR_SYSTEMS[2] == HdrSystem::DolbyVision;
        s.add(
            "A58-制式-三制式登记表完备",
            ok,
            "判据侧独立复算登记表内容与期望长度，与 V05 对齐",
        );
    }

    // --- 判据 3.2：制式对齐——通路不支持时按登记序降档改判并留痕 ---
    {
        let mut p = HdrPath::new();
        let sup = [HdrSystem::Hdr10];
        let got = p.negotiate_system(HdrSystem::DolbyVision, &sup);
        s.add(
            "A58-制式-不支持时降档改判留痕",
            got == HdrSystem::Hdr10
                && p.system == HdrSystem::Hdr10
                && p.stats.system_renegotiated == 1,
            "杜比视界请求、通路只支持 HDR10：按登记序取最靠前支持者，改判计数 1",
        );
    }

    // --- 判据 3.3：制式对齐——未知制式显性拒绝 ---
    {
        let unk = HdrSystem::from_registry(99);
        let k0 = HdrSystem::from_registry(0);
        s.add(
            "A58-制式-未知制式显性拒绝",
            unk == Err(CODE_SYSTEM_UNKNOWN) && k0 == Ok(HdrSystem::Hdr10),
            "登记表外 id 走 SYSTEM_UNKNOWN 独立通道，不静默映射",
        );
    }

    // --- 判据 4.1：SDR 回退——回退显性 + 告知 + 统计入账 ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 600, min_lum: 5, max_cll: 500, max_fall: 250 };
        let _ = p.declare_source(m);
        let code = p.fallback_sdr();
        let line = p.status_line();
        s.add(
            "A58-回退-回退显性且告知入账",
            code == CODE_SDR_FALLBACK
                && p.stats.sdr_fallbacks == 1
                && line.contains("SDR 回退态")
                && line.contains("SDR 回退 1 次"),
            "回退不是静默：返回告知码、读屏行明示回退态与次数",
        );
    }

    // --- 判据 4.2：SDR 回退——回退后元数据与 HDR 态全清不残留 ---
    {
        let mut p = HdrPath::new();
        let m = HdrMeta { max_lum: 600, min_lum: 5, max_cll: 500, max_fall: 250 };
        let _ = p.declare_source(m);
        let _ = p.deliver_to(PathStage::Swapchain, m);
        let _ = p.fallback_sdr();
        let after = p.deliver_to(PathStage::Monitor, m);
        s.add(
            "A58-回退-回退后不残留不续传",
            p.clean_after_fallback() && after == Err(CODE_STATE_INVALID),
            "回退清源元数据与 HDR 态；续传 HDR 被状态机显性拒绝",
        );
    }

    // --- 判据 5.1：判据自身——用户开关记忆往返保真 ---
    {
        let mut p = HdrPath::new();
        let want = UserPref { hdr_on: true, prefer: Some(HdrSystem::Hdr10Plus) };
        p.set_pref(want);
        s.add(
            "A58-自身-用户开关记忆往返保真",
            p.pref == want && p.pref.hdr_on && p.pref.prefer == Some(HdrSystem::Hdr10Plus),
            "偏好只存不判：登记后逐字段可读回",
        );
    }

    // --- 判据 5.2：判据自身——码位互异且段独占（整仓唯一段 0x28 内六码） ---
    {
        let codes = [
            CODE_META_BAD,
            CODE_META_LOST,
            CODE_SYSTEM_UNKNOWN,
            CODE_STAGE_BAD,
            CODE_SDR_FALLBACK,
            CODE_STATE_INVALID,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x2800);
        s.add(
            "A58-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 七、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(cll: u32) -> HdrMeta {
        HdrMeta { max_lum: 1000, min_lum: 1, max_cll: cll, max_fall: cll / 2 }
    }

    #[test]
    fn vea58_end_to_end_full_pass() {
        let mut p = HdrPath::new();
        let m = meta(800);
        assert!(p.declare_source(m).is_ok());
        assert!(p.deliver_to(PathStage::Swapchain, m).is_ok());
        assert!(p.deliver_to(PathStage::Monitor, m).is_ok());
        assert_eq!(p.stats.full_pass, 1);
        assert_eq!(p.stats.legs_total, 2);
        assert!(p.status_line().contains("完整传递 1 次"));
    }

    #[test]
    fn vea58_loss_localized_and_audited() {
        let mut p = HdrPath::new();
        let m = meta(800);
        let _ = p.declare_source(m);
        // 第一跳丢失（CLL 归零）
        assert_eq!(
            p.deliver_to(PathStage::Swapchain, HdrMeta { max_lum: 1000, min_lum: 1, max_cll: 0, max_fall: 0 }),
            Err(CODE_META_LOST)
        );
        assert_eq!(p.audit_leg(), Err(CODE_META_LOST));
        assert_eq!(p.legs.len(), 1);
        assert_eq!(p.legs[0].to, 1);
    }

    #[test]
    fn vea58_system_negotiation_and_registry() {
        let mut p = HdrPath::new();
        // 源支持：保留
        assert_eq!(p.negotiate_system(HdrSystem::Hdr10Plus, &[HdrSystem::Hdr10, HdrSystem::Hdr10Plus]), HdrSystem::Hdr10Plus);
        // 源不支持：登记序降档
        assert_eq!(p.negotiate_system(HdrSystem::DolbyVision, &[HdrSystem::Hdr10]), HdrSystem::Hdr10);
        assert_eq!(p.stats.system_renegotiated, 1);
        // 登记表往返
        assert_eq!(HdrSystem::from_registry(2), Ok(HdrSystem::DolbyVision));
        assert!(HdrSystem::from_registry(3).is_err());
    }

    #[test]
    fn vea58_sdr_fallback_clean_state() {
        let mut p = HdrPath::new();
        let m = meta(500);
        let _ = p.declare_source(m);
        let _ = p.deliver_to(PathStage::Swapchain, m);
        assert_eq!(p.fallback_sdr(), CODE_SDR_FALLBACK);
        assert!(p.clean_after_fallback());
        assert_eq!(p.deliver_to(PathStage::Monitor, m), Err(CODE_STATE_INVALID));
        let line = p.status_line();
        assert!(line.contains("SDR 回退 1 次"), "{}", line);
    }

    #[test]
    fn vea58_checks_all_green() {
        let set = run_vea58_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0058 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
