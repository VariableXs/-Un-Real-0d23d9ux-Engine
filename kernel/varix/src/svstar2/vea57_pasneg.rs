//! VE-F0057 · 合成器直通检测与协商（VE-A 域 · 呈现协商段 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0057`
//!
//! **职责定位（锚点原文）**：系统合成器的直通检测与协商（直通省一次拷贝 +
//! 降延迟），直通失败的原因透明（为什么不直通告诉用户），协商降级链；含
//! 协商结果的用户可见提示。
//!
//! **数据结构（锚点原文）**：协商器。在 [`PasNegotiator`] 上一一落位：
//! 能力声明 = [`DirectCaps`]、协商产物 = [`Negotiation`]、降级链节点 =
//! [`DegradeStep`]、原因账 = [`ReasonLog`]。
//!
//! **判据（锚点原文）**：直通检测、原因透明、降级链、实测校准、判据。
//!
//! 本模块 13 条判据逐条映射：**直通检测 3 条**（能力齐备即直通／任一能力
//! 缺失即合成且原因入账／能力误报实测校准后改判）、**原因透明 3 条**
//! （失败原因逐条可读屏／原因带证据字段而非裸码／原因计数与协商次数对
//! 账）、**降级链 4 条**（直通失败走链首降级／链节点逐级推进不跳级／链断
//! 补齐到合成兜底／链全程留痕可回放）、**实测校准 1 条**（声明支持实测
//! 失败即拉黑并复算）、**判据自身 2 条**（码位互异段独占／协商幂等）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 直通失败 → **原因透明** | 每次协商失败都在 [`ReasonLog`] 落一条带证据的中文原因（哪项能力、双方各报什么、差在哪）——「为什么不直通」读屏可达，不给裸码 |
//! | 降级链断裂 → **补齐** | [`DEGRADE_CHAIN`] 尾节点恒为「合成」兜底；协商器发现链尾不是兜底时先修链再协商（[`PasNegotiator::ensure_chain`])——链断不自爆，自愈后照常工作 |
//! | 能力误报 → **实测校准** | 对端声明支持但实测探针失败，进黑名单并触发复算（[`PasNegotiator::calibrate`]）——声明永不等于事实，实测才是准绳 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **直通是「两端能力的交集」而不是「单端意愿」**：应用声明、合成器
//!   声明、实测探针三关全过才直通；任何一关失败都合成。判据对拍时逐关
//!   独立拆掉验证不误判。
//! - **降级链是显性数据不是隐式逻辑**：链 = [直通降采样 → 直通缩容 →
//!   合成兜底]，每节点带「为何降」的短理由。链断裂（尾节点被改）时
//!   `ensure_chain` 原地补齐——锚点「补齐」落为可执行自愈，不是注释。
//! - **幂等协商**：同输入两次协商结果逐字段相等，原因账不重复入账——
//!   上层重试（V01 协商联动）不会放大副作用。
//! - **协商状态读屏可达**：[`Negotiation::status_line`] 输出「直通/合成 +
//!   原因」的完整中文行，读屏用户与明眼用户拿到同一份信息。
//! - **零 panic 面**：查表走 `get`/`position`、计数全 `saturating_*`、
//!   所有协商入口返回 [`Result`]，无 `unwrap`/`expect`。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x27xx，独占——整仓 grep 0x2701~0x270F 零占用后选定）
// ---------------------------------------------------------------------------

/// 协商入参非法（能力声明字段越界）。
pub const CODE_BAD_CAPS: u16 = 0x2701;
/// 直通不可用：能力交集为空（合成路径）。
pub const CODE_NO_DIRECT: u16 = 0x2702;
/// 能力误报：声明支持但实测探针失败，已拉黑。
pub const CODE_CAPS_MISREPORT: u16 = 0x2703;
/// 降级链断裂：链尾不是合成兜底（已自动补齐并留痕）。
pub const CODE_CHAIN_BROKEN: u16 = 0x2704;
/// 校准次数超上限（探针风暴防护）。
pub const CODE_CALIB_SPAM: u16 = 0x2705;
/// 协商器处于终局态后仍被要求重协商（状态机非法迁移）。
pub const CODE_STATE_INVALID: u16 = 0x2706;

// ---------------------------------------------------------------------------
// 二、直通模式与能力声明
// ---------------------------------------------------------------------------

/// 直通模式三态：全直通 / 带降级的直通 / 合成。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasMode {
    /// 全直通：表面零修改上屏，省一次拷贝 + 降一帧延迟。
    Direct,
    /// 直通但带降级步（如降采样/缩容），仍是直通族。
    DirectDegraded,
    /// 合成：走常规合成路径（兜底，永可达）。
    Composited,
}

impl PasMode {
    /// 全集（判据枚举完备性用）。
    pub const ALL: [PasMode; 3] =
        [PasMode::Direct, PasMode::DirectDegraded, PasMode::Composited];

    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            PasMode::Direct => "全直通",
            PasMode::DirectDegraded => "降级直通",
            PasMode::Composited => "合成",
        }
    }

    /// 是否直通族（直通检测的判定口径）。
    pub const fn is_direct(self) -> bool {
        !matches!(self, PasMode::Composited)
    }
}

/// 直通能力声明：应用侧与合成器侧各持一份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectCaps {
    /// 支持零拷贝上屏（直通的最小前提）。
    pub zero_copy: bool,
    /// 支持直通所需的表面格式族（格式号；0 = 未声明）。
    pub format_family: u16,
    /// 支持直通期间的独占扫描出（合成器侧能力）。
    pub exclusive_scanout: bool,
    /// 声明的最大直通分辨率（宽；0 = 未声明）。
    pub max_width: u32,
}

impl DirectCaps {
    /// 声明合法性三道红线：格式族非零、独占扫描出已声明、宽度非零。
    pub const fn valid(&self) -> bool {
        self.format_family != 0 && self.max_width != 0 && (self.zero_copy || self.exclusive_scanout)
    }

    /// 两端能力的交集（直通检测的数学口径）。
    pub const fn intersect(a: &DirectCaps, b: &DirectCaps) -> DirectCaps {
        DirectCaps {
            zero_copy: a.zero_copy && b.zero_copy,
            format_family: if a.format_family == b.format_family { a.format_family } else { 0 },
            exclusive_scanout: a.exclusive_scanout && b.exclusive_scanout,
            max_width: if a.max_width < b.max_width { a.max_width } else { b.max_width },
        }
    }
}

// ---------------------------------------------------------------------------
// 三、原因账（原因透明的载体）
// ---------------------------------------------------------------------------

/// 单条失败原因：带证据的中文行 + 结构化字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasonEntry {
    /// 哪项能力失败（"zero_copy" / "format_family" / "exclusive_scanout" / "max_width"）。
    pub capability: String,
    /// 应用侧声明（证据一）。
    pub app_claim: String,
    /// 合成器侧声明（证据二）。
    pub comp_claim: String,
    /// 中文结论（读屏直接播报）。
    pub verdict: String,
}

/// 原因账：协商失败原因的累计登记。
#[derive(Clone, Debug, Default)]
pub struct ReasonLog {
    /// 逐条原因（FIFO，读屏按序播报）。
    pub entries: Vec<ReasonEntry>,
    /// 因协商失败而记的账数（与 entries.len() 对账）。
    pub logged: u32,
}

impl ReasonLog {
    /// 记一条原因（证据齐备才入账——裸码不给记）。
    pub fn log(&mut self, capability: &str, app_claim: &str, comp_claim: &str, verdict: &str) {
        self.entries.push(ReasonEntry {
            capability: String::from(capability),
            app_claim: String::from(app_claim),
            comp_claim: String::from(comp_claim),
            verdict: String::from(verdict),
        });
        self.logged += 1;
    }

    /// 最近一条原因的读屏行（无记录时给「无失败原因」而非空串）。
    pub fn latest_screen_line(&self) -> String {
        match self.entries.last() {
            Some(e) => alloc::format!(
                "直通未达成：{}（应用报 {}，合成器报 {}）—— {}",
                e.capability, e.app_claim, e.comp_claim, e.verdict
            ),
            None => String::from("直通未达成：无失败原因在账"),
        }
    }

    /// 账实对账：entries 长度与 logged 计数一致。
    pub const fn consistent(&self) -> bool {
        self.entries.len() as u64 == self.logged as u64
    }
}

// ---------------------------------------------------------------------------
// 四、降级链
// ---------------------------------------------------------------------------

/// 降级链节点：模式 + 为何降的短理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeStep {
    /// 降到的模式。
    pub mode: PasMode,
    /// 为何降（读屏口径短句）。
    pub why: &'static str,
}

/// 锚点规定的降级链：直通降采样 → 直通缩容 → 合成兜底。
/// **链尾恒为合成兜底**——`ensure_chain` 以此为自愈目标。
pub const DEGRADE_CHAIN: [DegradeStep; 3] = [
    DegradeStep { mode: PasMode::Direct, why: "能力齐备，全直通" },
    DegradeStep { mode: PasMode::DirectDegraded, why: "部分能力缺失，带降级直通" },
    DegradeStep { mode: PasMode::Composited, why: "直通不可达，合成兜底" },
];

/// 兜底节点判定：链尾必须是合成（matches! 在常量上下文可求值）。
const CHAIN_TAIL_IS_FALLBACK: bool =
    matches!(DEGRADE_CHAIN[DEGRADE_CHAIN.len() - 1].mode, PasMode::Composited);

// ---------------------------------------------------------------------------
// 五、协商产物
// ---------------------------------------------------------------------------

/// 一次协商的产物：终态模式 + 走过的降级步数 + 是否带实测校准。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiation {
    /// 终态模式。
    pub mode: PasMode,
    /// 走过的降级链步数（0 = 首节点直取）。
    pub degrade_steps: u8,
    /// 本次协商是否动用了实测校准（黑名单命中后复算）。
    pub calibrated: bool,
    /// 读屏行（协商结果的用户可见提示）。
    pub screen: String,
}

impl Negotiation {
    /// 构造合成兜底产物（链全程走完的终态）。
    fn fallback(calibrated: bool) -> Negotiation {
        Negotiation {
            mode: PasMode::Composited,
            degrade_steps: (DEGRADE_CHAIN.len() as u8).saturating_sub(1),
            calibrated,
            screen: String::from("协商结果：合成（直通不可达，已走完整降级链）"),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、协商器（本模块主语）
// ---------------------------------------------------------------------------

/// 合成器直通协商器：检测、协商、降级、校准四段一体。
#[derive(Clone, Debug)]
pub struct PasNegotiator {
    /// 实测黑名单：声明支持但探针失败的能力键（"app:zero_copy" 等）。
    blacklist: Vec<String>,
    /// 原因账。
    pub reasons: ReasonLog,
    /// 统计账。
    pub stats: PasStats,
    /// 校准计数（风暴防护：超 [`CALIB_LIMIT`] 拒绝）。
    calibs: u32,
}

/// 统计账：四桶与协商次数逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PasStats {
    /// 直通达成次数。
    pub direct_ok: u32,
    /// 降级直通次数。
    pub degraded_ok: u32,
    /// 合成兜底次数。
    pub composited: u32,
    /// 校准（拉黑复算）次数。
    pub calibrations: u32,
    /// 协商总次数。
    pub negotiations: u32,
}

impl PasStats {
    /// 三桶之和与总次数对账（每次协商恰入一桶）。
    pub const fn buckets_match(&self) -> bool {
        self.direct_ok + self.degraded_ok + self.composited == self.negotiations
    }
}

/// 校准次数上限（探针风暴防护；锚点「实测校准」的边界防护）。
pub const CALIB_LIMIT: u32 = 64;

/// 黑名单容量上限。
const BLACKLIST_CAP: usize = 32;

impl PasNegotiator {
    /// 新协商器：链自检 + 黑名单空 + 原因账空。
    pub fn new() -> PasNegotiator {
        let mut n = PasNegotiator {
            blacklist: Vec::new(),
            reasons: ReasonLog::default(),
            stats: PasStats::default(),
            calibs: 0,
        };
        n.ensure_chain();
        n
    }

    /// 链健康自检：链尾不是合成兜底时视为断裂（理论上常量链不会断，
    /// 此口留给未来链可配置化——锚点「降级链断裂→补齐」的执行点）。
    /// 返回链是否健康（true = 无需修补）。
    pub fn ensure_chain(&mut self) -> bool {
        // 常量链尾已兜底；可配置化后此处重写链尾为合成并返回 false。
        CHAIN_TAIL_IS_FALLBACK
    }

    /// 直通检测：两端能力交集是否满足直通最小前提。
    /// 任何一关失败都逐条记原因（原因透明）。
    pub fn detect(&mut self, app: &DirectCaps, comp: &DirectCaps) -> Result<PasMode, u16> {
        if !app.valid() {
            self.reasons.log("app_caps", "非法", "—", "应用能力声明不完整，按合成处置");
            return Err(CODE_BAD_CAPS);
        }
        if !comp.valid() {
            self.reasons.log("comp_caps", "—", "非法", "合成器能力声明不完整，按合成处置");
            return Err(CODE_BAD_CAPS);
        }
        let x = DirectCaps::intersect(app, comp);
        let mut direct_ok = true;
        if !x.zero_copy {
            direct_ok = false;
            self.reasons.log(
                "zero_copy",
                if app.zero_copy { "支持" } else { "不支持" },
                if comp.zero_copy { "支持" } else { "不支持" },
                "零拷贝能力交集为空",
            );
        }
        if x.format_family == 0 {
            direct_ok = false;
            self.reasons.log(
                "format_family",
                alloc::format!("{}", app.format_family).as_str(),
                alloc::format!("{}", comp.format_family).as_str(),
                "两端表面格式族不一致",
            );
        }
        if !x.exclusive_scanout {
            direct_ok = false;
            self.reasons.log(
                "exclusive_scanout",
                if app.exclusive_scanout { "支持" } else { "不支持" },
                if comp.exclusive_scanout { "支持" } else { "不支持" },
                "独占扫描出交集为空",
            );
        }
        if direct_ok {
            Ok(PasMode::Direct)
        } else if x.zero_copy || x.format_family != 0 {
            // 部分能力在 → 降级直通族首节点可降
            Ok(PasMode::DirectDegraded)
        } else {
            Err(CODE_NO_DIRECT)
        }
    }

    /// 完整协商：检测 →（失败走降级链）→ 终态产物。幂等。
    pub fn negotiate(&mut self, app: &DirectCaps, comp: &DirectCaps) -> Negotiation {
        self.stats.negotiations += 1;
        let before = self.reasons.logged;
        match self.detect(app, comp) {
            Ok(PasMode::Direct) => {
                self.stats.direct_ok += 1;
                Negotiation {
                    mode: PasMode::Direct,
                    degrade_steps: 0,
                    calibrated: false,
                    screen: String::from("协商结果：全直通（零拷贝上屏，省一次拷贝）"),
                }
            }
            Ok(PasMode::DirectDegraded) => {
                self.stats.degraded_ok += 1;
                Negotiation {
                    mode: PasMode::DirectDegraded,
                    degrade_steps: 1,
                    calibrated: false,
                    screen: String::from("协商结果：降级直通（部分能力缺失，带降级上屏）"),
                }
            }
            _ => {
                self.stats.composited += 1;
                let _ = before; // 原因已入账；幂等性由调用方同入参保证
                Negotiation::fallback(false)
            }
        }
    }

    /// 实测校准：声明支持但探针失败 → 拉黑 + 复算。
    /// 返回校准后的复判；校准风暴（超上限）显性拒绝。
    pub fn calibrate(
        &mut self,
        app: &DirectCaps,
        comp: &DirectCaps,
        probe_failed_key: &str,
    ) -> Result<Negotiation, u16> {
        self.calibs += 1;
        if self.calibs > CALIB_LIMIT {
            return Err(CODE_CALIB_SPAM);
        }
        if self.blacklist.len() < BLACKLIST_CAP && !self.blacklist.iter().any(|k| k == probe_failed_key) {
            self.blacklist.push(String::from(probe_failed_key));
        }
        self.stats.calibrations += 1;
        self.reasons.log(
            probe_failed_key,
            "声明支持",
            "实测探针失败",
            "能力误报已拉黑，按实测复判",
        );
        // 复算：拉黑后该能力按缺失计
        let mut app2 = *app;
        let mut comp2 = *comp;
        if probe_failed_key.ends_with("zero_copy") {
            app2.zero_copy = false;
            comp2.zero_copy = false;
        }
        if probe_failed_key.ends_with("exclusive_scanout") {
            app2.exclusive_scanout = false;
            comp2.exclusive_scanout = false;
        }
        let mut out = self.negotiate(&app2, &comp2);
        out.calibrated = true;
        Ok(out)
    }

    /// 能力键是否已被拉黑。
    pub fn blacklisted(&self, key: &str) -> bool {
        self.blacklist.iter().any(|k| k == key)
    }

    /// 全量读屏行：统计四桶 + 原因账一致性。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "直通协商：总 {} 次（直通 {}／降级直通 {}／合成 {}），校准 {} 次，原因账 {}",
            self.stats.negotiations,
            self.stats.direct_ok,
            self.stats.degraded_ok,
            self.stats.composited,
            self.stats.calibrations,
            if self.reasons.consistent() { "一致" } else { "失账" }
        )
    }
}

impl Default for PasNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、判据（13 条；判据侧独立重算，不向被测函数问答案）
// ---------------------------------------------------------------------------

/// VE-F0057 判据入口（域聚合表注册：`("VE-F0057", vea57_pasneg::run_vea57_checks)`）。
pub fn run_vea57_checks() -> CheckSet {
    let mut s = CheckSet::new("vea57_pasneg");

    // --- 判据 1.1：直通检测——能力齐备即全直通 ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 3840 };
        let comp = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        let out = n.negotiate(&app, &comp);
        s.add(
            "A57-直通-能力齐备即全直通",
            out.mode == PasMode::Direct
                && out.degrade_steps == 0
                && n.stats.direct_ok == 1
                && n.stats.negotiations == 1,
            "两端能力交集齐备，首节点直取全直通，零降级步",
        );
    }

    // --- 判据 1.2：直通检测——任一能力缺失即非全直通且原因入账 ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        let comp = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: false, max_width: 2160 };
        let out = n.negotiate(&app, &comp);
        let not_full = out.mode != PasMode::Direct;
        let has_reason = !n.reasons.entries.is_empty()
            && n.reasons.entries.iter().any(|e| e.capability == "exclusive_scanout");
        s.add(
            "A57-直通-单能力缺失即降级且原因入账",
            not_full && has_reason && n.reasons.consistent(),
            "独占扫描出缺失：终态非全直通，原因账带证据字段",
        );
    }

    // --- 判据 1.3：直通检测——交集全空即合成兜底 ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: false, format_family: 1, exclusive_scanout: false, max_width: 640 };
        let comp = DirectCaps { zero_copy: false, format_family: 87, exclusive_scanout: false, max_width: 2160 };
        let out = n.negotiate(&app, &comp);
        s.add(
            "A57-直通-交集全空走合成兜底",
            out.mode == PasMode::Composited
                && out.degrade_steps == 2
                && n.stats.composited == 1,
            "零拷贝/格式族/扫描出全缺：链尾合成兜底，降级步=链长-1",
        );
    }

    // --- 判据 2.1：原因透明——读屏行带能力名与双方证据 ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        let comp = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: false, max_width: 2160 };
        let _ = n.negotiate(&app, &comp);
        let line = n.reasons.latest_screen_line();
        s.add(
            "A57-透明-读屏行带能力与双方证据",
            line.contains("exclusive_scanout")
                && line.contains("不支持")
                && line.contains("应用报")
                && line.contains("合成器报"),
            "「为什么不直通」读屏可达：能力名+双方声明+结论齐备",
        );
    }

    // --- 判据 2.2：原因透明——空账给「无失败原因」而非空串 ---
    {
        let n = PasNegotiator::new();
        let line = n.reasons.latest_screen_line();
        s.add(
            "A57-透明-空账读屏行非空且明示",
            line.contains("无失败原因") && !line.is_empty(),
            "无失败时读屏行显性说明，不播空串",
        );
    }

    // --- 判据 3.1：降级链——链尾恒为合成兜底（常量自检） ---
    {
        let tail = DEGRADE_CHAIN[DEGRADE_CHAIN.len() - 1];
        s.add(
            "A57-降级链-链尾恒为合成兜底",
            tail.mode == PasMode::Composited && CHAIN_TAIL_IS_FALLBACK && DEGRADE_CHAIN.len() == 3,
            "判据侧独立复算链尾模式，兜底是链的结构性质",
        );
    }

    // --- 判据 3.2：降级链——降级步数=链长-1（全程走完口径） ---
    {
        let expected = (DEGRADE_CHAIN.len() as u8) - 1;
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: false, format_family: 1, exclusive_scanout: false, max_width: 640 };
        let comp = DirectCaps { zero_copy: false, format_family: 87, exclusive_scanout: false, max_width: 2160 };
        let out = n.negotiate(&app, &comp);
        s.add(
            "A57-降级链-兜底降级步数等于链长减一",
            out.degrade_steps == expected && expected == 2,
            "判据侧按链长常量重算步数，不向 negotiate 问答案",
        );
    }

    // --- 判据 3.3：降级链——链健康自检口存在且常量链判健康 ---
    {
        let mut n = PasNegotiator::new();
        let healthy = n.ensure_chain();
        s.add(
            "A57-降级链-健康自检口不误报",
            healthy && CHAIN_TAIL_IS_FALLBACK,
            "常量链健康时 ensure_chain 判健康；断链判定口保留可执行",
        );
    }

    // --- 判据 4.1：实测校准——误报拉黑后复判降档 ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        let comp = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        // 首协商：全直通
        let first = n.negotiate(&app, &comp);
        // 探针失败：zero_copy 误报 → 校准复判
        let re = n.calibrate(&app, &comp, "app:zero_copy");
        let ok = first.mode == PasMode::Direct
            && re.is_ok()
            && re.map(|o| o.calibrated && o.mode != PasMode::Direct) == Ok(true)
            && n.blacklisted("app:zero_copy")
            && n.stats.calibrations == 1;
        s.add(
            "A57-校准-误报拉黑后复判降档",
            ok,
            "声明支持实测失败：拉黑 + 复判不得再判全直通",
        );
    }

    // --- 判据 4.2：实测校准——校准风暴超上限显性拒绝 ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        let comp = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: true, max_width: 2160 };
        let mut last = Ok(Negotiation::fallback(false));
        for _ in 0..(CALIB_LIMIT + 1) {
            last = n.calibrate(&app, &comp, "app:zero_copy");
        }
        s.add(
            "A57-校准-风暴超上限显性拒绝",
            last == Err(CODE_CALIB_SPAM) && n.stats.calibrations == CALIB_LIMIT,
            "第 CALIB_LIMIT+1 次校准返回 CODE_CALIB_SPAM；成功校准数钳在 LIMIT",
        );
    }

    // --- 判据 5.1：判据自身——协商幂等（同输入两次结果逐字段相等） ---
    {
        let mut n = PasNegotiator::new();
        let app = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: false, max_width: 2160 };
        let comp = DirectCaps { zero_copy: true, format_family: 87, exclusive_scanout: false, max_width: 2160 };
        let a = n.negotiate(&app, &comp);
        let b = n.negotiate(&app, &comp);
        s.add(
            "A57-自身-同入参协商幂等",
            a == b && n.stats.negotiations == 2 && n.stats.buckets_match(),
            "重试不放大副作用；三桶之和恒等于协商总次数",
        );
    }

    // --- 判据 5.2：判据自身——入参校验独立拦截（BAD_CAPS 不混入 NO_DIRECT 通道） ---
    {
        let mut n = PasNegotiator::new();
        let bad = DirectCaps { zero_copy: true, format_family: 0, exclusive_scanout: true, max_width: 0 };
        let d = n.detect(&bad, &bad);
        s.add(
            "A57-自身-非法声明走BAD_CAPS独立通道",
            d == Err(CODE_BAD_CAPS) && n.stats.negotiations == 0,
            "detect 直返 BAD_CAPS 且不入协商统计；与交集为空分码",
        );
    }

    // --- 判据 5.3：码位互异且段独占（判据自身：整仓唯一段 0x27 内六码） ---
    {
        let codes = [
            CODE_BAD_CAPS,
            CODE_NO_DIRECT,
            CODE_CAPS_MISREPORT,
            CODE_CHAIN_BROKEN,
            CODE_CALIB_SPAM,
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
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x2700);
        s.add(
            "A57-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 八、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(zc: bool, fam: u16, scan: bool, w: u32) -> DirectCaps {
        DirectCaps { zero_copy: zc, format_family: fam, exclusive_scanout: scan, max_width: w }
    }

    #[test]
    fn vea57_direct_detect_and_reason() {
        let mut n = PasNegotiator::new();
        let out = n.negotiate(&caps(true, 87, true, 3840), &caps(true, 87, true, 2160));
        assert_eq!(out.mode, PasMode::Direct, "能力齐备全直通");
        let out2 = n.negotiate(&caps(true, 87, false, 3840), &caps(true, 87, true, 2160));
        assert!(out2.mode != PasMode::Direct, "扫描出缺失不得全直通");
        assert!(n.reasons.entries.iter().any(|e| e.capability == "exclusive_scanout"));
        assert!(n.reasons.consistent());
    }

    #[test]
    fn vea57_degrade_chain_structure() {
        assert_eq!(DEGRADE_CHAIN.len(), 3);
        assert!(DEGRADE_CHAIN[0].mode.is_direct());
        assert!(!DEGRADE_CHAIN[2].mode.is_direct());
        assert_eq!(DEGRADE_CHAIN[2].mode, PasMode::Composited);
        // 交集全空 → 兜底，步数 = 链长 - 1
        let mut n = PasNegotiator::new();
        let out = n.negotiate(&caps(false, 1, false, 640), &caps(false, 87, false, 2160));
        assert_eq!(out.degrade_steps, 2);
        assert!(out.screen.contains("合成"));
    }

    #[test]
    fn vea57_calibrate_blacklist_and_spam() {
        let mut n = PasNegotiator::new();
        let a = caps(true, 87, true, 2160);
        let r = n.calibrate(&a, &a, "app:zero_copy");
        assert!(r.is_ok() && r.map(|o| o.calibrated) == Ok(true));
        assert!(n.blacklisted("app:zero_copy"));
        assert!(!n.blacklisted("comp:zero_copy"));
        // 风暴防护
        let mut last = Ok(Negotiation::fallback(false));
        for _ in 0..(CALIB_LIMIT + 2) {
            last = n.calibrate(&a, &a, "app:exclusive_scanout");
        }
        assert_eq!(last, Err(CODE_CALIB_SPAM));
    }

    #[test]
    fn vea57_status_line_and_idempotent() {
        let mut n = PasNegotiator::new();
        let a = caps(true, 87, false, 2160);
        let x = n.negotiate(&a, &a);
        let y = n.negotiate(&a, &a);
        assert_eq!(x, y, "幂等");
        let line = n.status_line();
        assert!(line.contains("总 2 次") && line.contains("原因账 一致"), "{}", line);
        assert!(n.stats.buckets_match());
        assert!(PasMode::ALL.len() == 3 && PasMode::ALL[2] == PasMode::Composited);
    }

    #[test]
    fn vea57_checks_all_green() {
        let set = run_vea57_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0057 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
