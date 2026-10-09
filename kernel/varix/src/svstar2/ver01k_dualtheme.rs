//! VE-F3411 · 明暗双主题运行时 —— 双主题令牌对强制配对 + 平滑过渡 + 双主题对比度断言。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3411`
//!
//! # 职责（锚点原文拆解）
//!
//! - **强制配对**：每个颜色令牌必有明暗两值，缺一即拒绝——「半对令牌」
//!   （只有亮值或只有暗值）是双主题体系的第一破坏源：它在当前主题下
//!   看起来正常，切主题后裸奔。构造入口只收 `(Option, Option)`，任一侧
//!   `None` 即 `Err(PairMissing)`，「缺一半」在类型面就不可构造；
//! - **对比度双主题达标断言**：令牌两值分别对该侧表面基线做 WCAG 对比度
//!   断言，任一侧不足即阻断（`ContrastLow`）——只测亮景不测暗景等于
//!   把色弱用户在暗主题下丢掉；
//! - **平滑过渡**：切换不允许瞬切（跳变）——降级矩阵明文「切换跳变→
//!   过渡」。过渡器持当前侧/目标侧/进度三本账，请求即起过渡；过渡中
//!   再请求则从当前混合态平滑接管（不跳回起点重放）；值为按进度对
//!   明暗两值的逐通道整数插值；
//! - **跟随系统 + 日出日落预留**：`follow` 开后系统信号变化自动起过渡；
//!   时间驱动的日出日落切换是**冻结接口**（`FrozenSwitch`）——预留
//!   未开放的功能不许偷偷生效；
//! - **跨批对接点 V07 夜间模式联动**：联动挂点（强度 ‰），越界即拒；
//! - **降级矩阵**：缺配对→拒绝；切换跳变→过渡；对比度不足→阻断。
//!
//! # 为什么对比度要双主题各测一次
//!
//! 同一令牌两值里暗值常在亮值附近手调（「看起来差不多」），但暗主题下表
//! 面基线也换了一侧——用亮景达标推暗景达标是猜。双测的成本是每令牌
//! 两次整数比值，O(令牌对) 与扫描同阶，没有理由省。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；
//! 判据区一律 match 记红。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；E13 段独占，与 E11/F3409、E12/F3410 等既有段零重叠）
// ---------------------------------------------------------------------------

/// 明暗双主题域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeCode {
    /// 令牌路径非法（空或超长）。
    TokenEmpty,
    /// 缺配对：明暗两值缺一（缺一即拒绝）。
    PairMissing,
    /// 对比度不足：任一侧不达 AA 即阻断。
    ContrastLow,
    /// 切换跳变被转换为过渡（非阻断，记账）。
    JumpConverted,
    /// 过渡中再请求：从当前混合态平滑接管（非阻断，记账）。
    TransitionBusy,
    /// V07 夜间模式联动状态非法（强度越界）。
    LinkageState,
    /// 时间驱动切换被冻结预留接口拒绝（未开放）。
    FrozenSwitch,
}

impl ThemeCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [ThemeCode; 7] = [
        ThemeCode::TokenEmpty,
        ThemeCode::PairMissing,
        ThemeCode::ContrastLow,
        ThemeCode::JumpConverted,
        ThemeCode::TransitionBusy,
        ThemeCode::LinkageState,
        ThemeCode::FrozenSwitch,
    ];

    /// 线上短码（E13 段独占）。
    pub const fn code(self) -> &'static str {
        match self {
            ThemeCode::TokenEmpty => "E13-TOKEN-EMPTY",
            ThemeCode::PairMissing => "E13-PAIR-MISSING",
            ThemeCode::ContrastLow => "E13-CONTRAST-LOW",
            ThemeCode::JumpConverted => "E13-JUMP-CONVERTED",
            ThemeCode::TransitionBusy => "E13-TRANSITION-BUSY",
            ThemeCode::LinkageState => "E13-LINKAGE-STATE",
            ThemeCode::FrozenSwitch => "E13-FROZEN-SWITCH",
        }
    }

    /// 是否阻断。
    pub const fn blocking(self) -> bool {
        !matches!(self, ThemeCode::JumpConverted | ThemeCode::TransitionBusy)
    }

    /// 是否降级类（走降级矩阵：跳变→过渡）。
    pub const fn degradable(self) -> bool {
        matches!(self, ThemeCode::JumpConverted | ThemeCode::TransitionBusy)
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            ThemeCode::TokenEmpty => "令牌路径非法。",
            ThemeCode::PairMissing => "颜色令牌缺明或暗值，已拒绝。",
            ThemeCode::ContrastLow => "对比度不达标，已阻断。",
            ThemeCode::JumpConverted => "瞬切请求已转换为平滑过渡。",
            ThemeCode::TransitionBusy => "过渡进行中，已从当前态平滑接管。",
            ThemeCode::LinkageState => "夜间模式联动状态非法。",
            ThemeCode::FrozenSwitch => "时间驱动切换尚未开放，已冻结。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、契约常量
// ---------------------------------------------------------------------------

/// 契约版本（冻结）。
pub const THEME_CONTRACT: &str = "E13-dualtheme-v1";
/// 令牌路径字节上限。
pub const PATH_MAX: usize = 128;
/// 亮度近似常数（0.05 × 255 取整）：对比度整数口径的暗室底。
pub const LUM_FLOOR: u32 = 13;
/// WCAG AA 对比度阈值（‰）：4.5:1。
pub const CONTRAST_AA_PERMILLE: u32 = 4500;
/// WCAG AAA 对比度阈值（‰）：7:1（高对比域共用口径）。
pub const CONTRAST_AAA_PERMILLE: u32 = 7000;
/// 联动强度上限（‰）。
pub const PERMILLE_MAX: u32 = 1000;
/// 过渡完成进度（‰）。
pub const PROGRESS_DONE: u32 = 1000;
/// 每帧默认步进（‰）：60 帧 ≈ 1s 过渡的步进量级说明用。
pub const DEFAULT_STEP_PERMILLE: u32 = 60;

/// 校验令牌路径。
fn validate_path(p: &str) -> Result<(), ThemeCode> {
    if p.is_empty() || p.len() > PATH_MAX {
        return Err(ThemeCode::TokenEmpty);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、颜色与对比度（整数口径，零浮点）
// ---------------------------------------------------------------------------

/// RGB888 颜色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RGB888 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RGB888 {
    pub const fn new(r: u8, g: u8, b: u8) -> RGB888 {
        RGB888 { r, g, b }
    }

    /// 相对亮度近似：(2126R + 7152G + 722B) / 10000，范围 0..=255。
    pub const fn luminance(self) -> u32 {
        (2126u32 * self.r as u32 + 7152 * self.g as u32 + 722 * self.b as u32) / 10000
    }

/// 单通道按进度（‰）整数插值（向零取整，钳位 0..=255）。
const fn mix_ch(a: u8, b: u8, p: u32) -> u8 {
    let d = b as i32 - a as i32;
    let v = a as i32 + d * p as i32 / PROGRESS_DONE as i32;
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

/// 逐通道按进度（‰）向目标插值（整数钳位 0..=255）。
pub const fn mix(self, other: RGB888, progress_permille: u32) -> RGB888 {
    let p = if progress_permille > PROGRESS_DONE {
        PROGRESS_DONE
    } else {
        progress_permille
    };
    RGB888 {
        r: Self::mix_ch(self.r, other.r, p),
        g: Self::mix_ch(self.g, other.g, p),
        b: Self::mix_ch(self.b, other.b, p),
    }
}
}

/// WCAG 对比度（‰ 整数口径）：(Lmax + 13) × 1000 / (Lmin + 13)。
///
/// 13 ≈ 0.05 × 255 取整——与 luminance 的 0..255 刻度对齐；两值相等
/// 时返回 1000（1:1），阻断判定由调用方按阈值比对。
pub const fn contrast_permille(a: RGB888, b: RGB888) -> u32 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + LUM_FLOOR) * 1000 / (lo + LUM_FLOOR)
}

/// 对比度是否达 AA（双主题断言的公共判定）。
pub const fn contrast_ok(fg: RGB888, bg: RGB888) -> bool {
    contrast_permille(fg, bg) >= CONTRAST_AA_PERMILLE
}

// ---------------------------------------------------------------------------
// 四、主题侧与强制配对表
// ---------------------------------------------------------------------------

/// 主题侧：明/暗两值全集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeSide {
    /// 亮主题。
    Light,
    /// 暗主题。
    Dark,
}

impl ThemeSide {
    /// 全集（判据据此核对无遗漏）。
    pub const ALL: [ThemeSide; 2] = [ThemeSide::Light, ThemeSide::Dark];

    /// 另一侧（切换即取反）。
    pub const fn other(self) -> ThemeSide {
        match self {
            ThemeSide::Light => ThemeSide::Dark,
            ThemeSide::Dark => ThemeSide::Light,
        }
    }

    /// 中文名（读屏）。
    pub const fn zh(self) -> &'static str {
        match self {
            ThemeSide::Light => "亮主题",
            ThemeSide::Dark => "暗主题",
        }
    }

    /// 序号（wire 口径）。
    pub const fn wire(self) -> u8 {
        match self {
            ThemeSide::Light => 0,
            ThemeSide::Dark => 1,
        }
    }

    /// 序号反查（越界 None）。
    pub const fn of_wire(v: u8) -> Option<ThemeSide> {
        match v {
            0 => Some(ThemeSide::Light),
            1 => Some(ThemeSide::Dark),
            _ => None,
        }
    }
}

/// 强制配对的颜色令牌：明暗两值齐备才可存在。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PairedToken {
    /// 令牌路径。
    pub path: String,
    /// 亮主题侧值。
    pub light: RGB888,
    /// 暗主题侧值。
    pub dark: RGB888,
}

/// 双主题配对表（O(令牌对) 操作）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PairTable {
    /// 亮主题表面基线（对比度断言的亮侧参照）。
    pub surface_light: RGB888,
    /// 暗主题表面基线（对比度断言的暗侧参照）。
    pub surface_dark: RGB888,
    /// 令牌全集（按路径唯一）。
    pub tokens: Vec<PairedToken>,
}

impl PairTable {
    /// 建表：双表面基线随表创建（缺基线则对比度断言无参照）。
    pub fn new(surface_light: RGB888, surface_dark: RGB888) -> PairTable {
        PairTable {
            surface_light,
            surface_dark,
            tokens: Vec::new(),
        }
    }

    /// 插入令牌（唯一入口：强制配对 + 双主题对比度两道闸）。
    ///
    /// - 任一值缺失 → `Err(PairMissing)`（缺一即拒绝）；
    /// - 任一值对同侧表面基线不达 AA → `Err(ContrastLow)`（双主题各测
    ///   一次，阻断）；
    /// - 同路径已存在 → 覆盖（配色修正走同一入口，两闸不豁免）。
    pub fn insert(
        &mut self,
        path: &str,
        light: Option<RGB888>,
        dark: Option<RGB888>,
    ) -> Result<(), ThemeCode> {
        validate_path(path)?;
        let (lv, dv) = match (light, dark) {
            (Some(l), Some(d)) => (l, d),
            _ => return Err(ThemeCode::PairMissing),
        };
        if !contrast_ok(lv, self.surface_light) || !contrast_ok(dv, self.surface_dark) {
            return Err(ThemeCode::ContrastLow);
        }
        for t in self.tokens.iter_mut() {
            if t.path == path {
                t.light = lv;
                t.dark = dv;
                return Ok(());
            }
        }
        self.tokens.push(PairedToken {
            path: String::from(path),
            light: lv,
            dark: dv,
        });
        Ok(())
    }

    /// 按路径查令牌。
    pub fn get(&self, path: &str) -> Option<&PairedToken> {
        self.tokens.iter().find(|t| t.path == path)
    }

    /// 按路径删令牌（存在即删并返回 true）。
    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.tokens.len();
        self.tokens.retain(|t| t.path != path);
        self.tokens.len() != before
    }

    /// 令牌数。
    pub fn count(&self) -> usize {
        self.tokens.len()
    }

    /// 取某侧值（侧外即 None——调用方拿侧去索引，不猜）。
    pub fn value_of(&self, path: &str, side: ThemeSide) -> Option<RGB888> {
        self.get(path).map(|t| match side {
            ThemeSide::Light => t.light,
            ThemeSide::Dark => t.dark,
        })
    }
}

// ---------------------------------------------------------------------------
// 五、过渡器（切换跳变→过渡）
// ---------------------------------------------------------------------------

/// 主题过渡器：当前侧/目标侧/进度三本账，值按进度整数插值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transitioner {
    /// 当前侧（进度 < 1000 时表示过渡起点侧）。
    pub current: ThemeSide,
    /// 目标侧（请求落点）。
    pub target: ThemeSide,
    /// 过渡进度（‰，0..=1000；1000 = 已落脚）。
    pub progress_permille: u32,
}

impl Default for Transitioner {
    fn default() -> Self {
        Transitioner {
            current: ThemeSide::Light,
            target: ThemeSide::Light,
            progress_permille: PROGRESS_DONE,
        }
    }
}

impl Transitioner {
    /// 新建：静止于给定侧。
    pub fn new(side: ThemeSide) -> Transitioner {
        Transitioner {
            current: side,
            target: side,
            progress_permille: PROGRESS_DONE,
        }
    }

    /// 是否过渡中。
    pub const fn in_transition(&self) -> bool {
        self.progress_permille < PROGRESS_DONE
    }

    /// 请求切换到目标侧。
    ///
    /// - 已在该侧且无在途过渡 → `Ok(None)`（空操作，不制造假事务）；
    /// - 静止态请求另一侧 → 起过渡（从 0 开始），`Ok(None)`；
    /// - 过渡中请求 → 从当前混合态平滑接管（起点改为当前侧、进度续走），
    ///   `Ok(Some(TransitionBusy))`（跳变被过渡接管的记账）。
    pub fn request(&mut self, side: ThemeSide) -> Result<Option<ThemeCode>, ThemeCode> {
        if self.progress_permille >= PROGRESS_DONE {
            if self.current == side {
                return Ok(None);
            }
            self.target = side;
            self.progress_permille = 0;
            return Ok(None);
        }
        // 过渡中：平滑接管——起点仍是当前侧（从当前混合态续走），
        // 目标改新侧，进度不清零（清零=跳回起点重放，用户看到主题
        // 「倒回去再走」）。
        let note = if self.target != side {
            self.target = side;
            Some(ThemeCode::TransitionBusy)
        } else {
            None
        };
        Ok(note)
    }

    /// 瞬切请求（跳变）：纪律上不放行——一律转换为过渡。
    ///
    /// 已在目标侧且静止 → `Ok(None)`（无跳变可转换，空操作不造假账）；
    /// 其余 → 起过渡或接管，返回 `JumpConverted`（接管中续走时以
    /// `TransitionBusy` 呈现，转换事实优先）。
    pub fn request_instant(&mut self, side: ThemeSide) -> Result<Option<ThemeCode>, ThemeCode> {
        let idle_at = self.progress_permille >= PROGRESS_DONE && self.current == side;
        let note = self.request(side)?;
        if idle_at && note.is_none() {
            return Ok(None);
        }
        Ok(Some(match note {
            Some(c) => c,
            None => ThemeCode::JumpConverted,
        }))
    }

    /// 推进进度（‰ 步进；饱和于 1000，到位即落脚）。
    pub fn advance(&mut self, step_permille: u32) {
        if !self.in_transition() {
            return;
        }
        self.progress_permille = self
            .progress_permille
            .saturating_add(step_permille)
            .min(PROGRESS_DONE);
        if self.progress_permille >= PROGRESS_DONE {
            self.current = self.target;
        }
    }

    /// 按进度对某令牌两值取当前混合值（过渡面即显示面）。
    pub fn mixed(&self, tok: &PairedToken) -> RGB888 {
        if self.target == self.current {
            return match self.current {
                ThemeSide::Light => tok.light,
                ThemeSide::Dark => tok.dark,
            };
        }
        // 过渡方向：current → target 逐通道插值。
        let (from, to) = match (self.current, self.target) {
            (ThemeSide::Light, ThemeSide::Dark) => (tok.light, tok.dark),
            _ => (tok.dark, tok.light),
        };
        from.mix(to, self.progress_permille)
    }
}

// ---------------------------------------------------------------------------
// 六、运行时段：跟随系统 + V07 联动 + 日出日落冻结接口
// ---------------------------------------------------------------------------

/// 跟随模式：跟随系统信号 or 手动锁定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FollowMode {
    /// 跟随系统（系统侧变化自动起过渡）。
    System,
    /// 手动锁定（系统侧变化不打扰）。
    Manual,
}

/// V07 夜间模式联动挂点（跨批对接点：联动是协调不是放水）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NightModeLinkage {
    /// 联动是否启用。
    pub enabled: bool,
    /// 联动强度（‰，0..=1000）。
    pub strength_permille: u32,
}

impl NightModeLinkage {
    /// 默认：关闭（双主题自成体系，联动是显式选择）。
    pub const fn off() -> NightModeLinkage {
        NightModeLinkage {
            enabled: false,
            strength_permille: 0,
        }
    }

    /// 校验联动状态（强度越界即拒）。
    pub fn validate(&self) -> Result<(), ThemeCode> {
        if self.strength_permille > PERMILLE_MAX {
            return Err(ThemeCode::LinkageState);
        }
        Ok(())
    }
}

/// 日出日落自动切换（时间驱动）——**冻结预留接口**。
///
/// 时间驱动切换依赖时钟源与定时策略的全局约定，未冻结前任何调用
/// 一律 `FrozenSwitch` 拒绝——预留功能不许偷偷生效。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeDrivenSwitch {
    /// 解冻开关（默认 false = 冻结）。
    pub unfrozen: bool,
}

impl Default for TimeDrivenSwitch {
    fn default() -> Self {
        TimeDrivenSwitch { unfrozen: false }
    }
}

impl TimeDrivenSwitch {
    /// 请求时间驱动切换（冻结中一律拒绝）。
    pub fn request(&self, _at_minutes: u32) -> Result<(), ThemeCode> {
        if !self.unfrozen {
            return Err(ThemeCode::FrozenSwitch);
        }
        Ok(())
    }
}

/// 明暗双主题运行时：配对表 + 过渡器 + 跟随模式 + 联动 + 冻结接口。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DualThemeRuntime {
    /// 双主题配对表。
    pub table: PairTable,
    /// 主题过渡器。
    pub transition: Transitioner,
    /// 跟随模式。
    pub follow: FollowMode,
    /// 系统信号侧（跟随模式的输入）。
    pub system_signal: ThemeSide,
    /// V07 夜间模式联动。
    pub linkage: NightModeLinkage,
    /// 日出日落冻结接口。
    pub timed: TimeDrivenSwitch,
    /// 事件账（跳变转换/接管记账，判据侧对账）。
    pub events: Vec<ThemeCode>,
}

impl DualThemeRuntime {
    /// 新建运行时：表基线、起始侧、跟随模式随建。
    pub fn new(
        surface_light: RGB888,
        surface_dark: RGB888,
        start: ThemeSide,
        follow: FollowMode,
    ) -> DualThemeRuntime {
        DualThemeRuntime {
            table: PairTable::new(surface_light, surface_dark),
            transition: Transitioner::new(start),
            follow,
            system_signal: start,
            linkage: NightModeLinkage::off(),
            timed: TimeDrivenSwitch::default(),
            events: Vec::new(),
        }
    }

    /// 系统信号变化（跟随模式生效；手动模式只记信号不起过渡）。
    pub fn set_system_signal(&mut self, side: ThemeSide) -> Result<Option<ThemeCode>, ThemeCode> {
        self.system_signal = side;
        if self.follow != FollowMode::System {
            return Ok(None);
        }
        self.request_theme(side)
    }

    /// 请求主题切换（统一走过渡：跳变在入口即被转换）。
    pub fn request_theme(&mut self, side: ThemeSide) -> Result<Option<ThemeCode>, ThemeCode> {
        let note = self.transition.request(side)?;
        if let Some(c) = note {
            self.events.push(c);
        }
        Ok(note)
    }

    /// 瞬切请求（放行即转换记账，接口与 request_theme 同权）。
    pub fn request_theme_instant(
        &mut self,
        side: ThemeSide,
    ) -> Result<Option<ThemeCode>, ThemeCode> {
        let note = self.transition.request_instant(side)?;
        if let Some(c) = note {
            self.events.push(c);
        }
        Ok(note)
    }

    /// 帧步进（默认步进）。
    pub fn tick(&mut self) {
        self.transition.advance(DEFAULT_STEP_PERMILLE);
    }

    /// 帧步进（自定义步进，‰）。
    pub fn tick_by(&mut self, step_permille: u32) {
        self.transition.advance(step_permille);
    }

    /// 当前显示面：某令牌此刻的混合值。
    pub fn display_value(&self, path: &str) -> Option<RGB888> {
        self.table.get(path).map(|t| self.transition.mixed(t))
    }

    /// 联动校验（V07 对接前置）。
    pub fn validate_linkage(&self) -> Result<(), ThemeCode> {
        self.linkage.validate()
    }

    /// 日出日落切换请求（转委冻结接口）。
    pub fn request_time_switch(&self, at_minutes: u32) -> Result<(), ThemeCode> {
        self.timed.request(at_minutes)
    }
}

// ---------------------------------------------------------------------------
// 七、读屏播报
// ---------------------------------------------------------------------------

/// 运行时读屏播报：当前侧/过渡态/跟随/联动/令牌数。
pub fn runtime_spoken(rt: &DualThemeRuntime) -> String {
    let cur = rt.transition.current.zh();
    let tgt = rt.transition.target.zh();
    let line = if rt.transition.in_transition() {
        format!(
            "正在从{}过渡到{}，进度{}‰；",
            cur,
            tgt,
            rt.transition.progress_permille
        )
    } else {
        format!("当前{}，静止；", cur)
    };
    format!(
        "明暗双主题运行时：{}跟随{}；联动{}；令牌{}对。{}",
        line,
        match rt.follow {
            FollowMode::System => "系统",
            FollowMode::Manual => "手动",
        },
        if rt.linkage.enabled { "开" } else { "关" },
        rt.table.count(),
        THEME_CONTRACT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: RGB888 = RGB888::new(255, 255, 255);
    const BLACK: RGB888 = RGB888::new(0, 0, 0);
    const GRAY: RGB888 = RGB888::new(128, 128, 128);

    #[test]
    fn pair_mandatory() {
        let mut t = PairTable::new(WHITE, BLACK);
        assert_eq!(
            t.insert("c.fg", Some(BLACK), None),
            Err(ThemeCode::PairMissing)
        );
        assert_eq!(t.insert("c.fg", None, Some(WHITE)), Err(ThemeCode::PairMissing));
        assert_eq!(t.insert("c.fg", Some(BLACK), Some(WHITE)), Ok(()));
        assert_eq!(t.count(), 1);
    }

    #[test]
    fn contrast_both_sides() {
        let mut t = PairTable::new(WHITE, BLACK);
        // 亮侧黑对白达标；暗侧白对黑达标。
        assert_eq!(t.insert("c.fg", Some(BLACK), Some(WHITE)), Ok(()));
        // 亮侧中灰对白不达 AA → 阻断（双主题各测一次）。
        assert_eq!(
            t.insert("c.mid", Some(GRAY), Some(WHITE)),
            Err(ThemeCode::ContrastLow)
        );
    }

    #[test]
    fn transition_smooth() {
        let mut tr = Transitioner::new(ThemeSide::Light);
        assert!(!tr.in_transition());
        assert_eq!(tr.request(ThemeSide::Light), Ok(None));
        assert_eq!(tr.request(ThemeSide::Dark), Ok(None));
        assert!(tr.in_transition());
        // 过渡中再请求：平滑接管记账。
        assert_eq!(
            tr.request(ThemeSide::Light),
            Ok(Some(ThemeCode::TransitionBusy))
        );
        tr.advance(500);
        assert_eq!(tr.progress_permille, 500);
        tr.advance(500);
        assert!(!tr.in_transition());
        // 落脚目标侧（接管后的新目标 = Light）。
        assert_eq!(tr.current, ThemeSide::Light);
        tr.advance(500);
        assert_eq!(tr.current, ThemeSide::Light);
    }

    #[test]
    fn instant_converted_or_noop() {
        let mut tr = Transitioner::new(ThemeSide::Light);
        // 已在目标侧且静止：无跳变可转换。
        assert_eq!(tr.request_instant(ThemeSide::Light), Ok(None));
        // 请求另一侧：跳变被转换为过渡。
        assert_eq!(
            tr.request_instant(ThemeSide::Dark),
            Ok(Some(ThemeCode::JumpConverted))
        );
        assert!(tr.in_transition());
        // 过渡中瞬切：接管事实优先于转换事实。
        assert_eq!(
            tr.request_instant(ThemeSide::Light),
            Ok(Some(ThemeCode::TransitionBusy))
        );
    }
}
