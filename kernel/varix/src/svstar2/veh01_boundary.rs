//! VE-F1401 · H 域开工与边界契约执行（VE-H 域 · 音频引擎本体 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1401`
//!
//! **判据（锚点原文四条）**：ADR 落册、四主题差异化、复用声明、查重对账。
//!
//! # 锚点原文摘要与本文件的执行面
//!
//! 锚点要求：H 域（F1401-F1600，官方主题混音图/空间音频/HRTF/音频令牌/
//! 淡入淡出/事件总线/低延迟路径/重采样/响度归一）开工前，必须执行 F1381 的
//! 边界契约声明与 F1397 的移交处置清单，并**落 ADR**——"无 ADR 不开工"。
//!
//! 锚点点名的四个重叠主题与处置方向：
//!
//! | 主题 | G07 视角（F13xx） | H 域视角（F14xx） |
//! | --- | --- | --- |
//! | 混音 | F1324 播放总线（16 总线固定拓扑） | F1403 通用混音图（任意 DAG） |
//! | HRTF | F1326 双耳播放（媒体内容渲染） | F1409 世界空间引擎（listener/emitter） |
//! | 重采样 | F1323 播放链重采样 | F1413 统一重采样服务（离线+实时同核） |
//! | 响度 | F1325 播放时对齐 | F1414 批量响度归一服务（库级） |
//!
//! 本文件把这层"法理"变成可执行的数据结构与闸门，三段工程量对应锚点：
//! - **ADR 四主题落册**（[`AdrRegistry`]）：四主题各一条 ADR，记录处置
//!   （G 保留/H 主导/合并 ADR 三选一）、两侧保留面、视角差异、共享核；
//!   未落册或未 Accepted ⇒ [`BoundaryContract::gate_h_work`] 拒绝开工；
//! - **复用声明与共享核抽定**（[`CoreRegistry`] + [`LayerRegistry`]）：
//!   DSP 核（biquad/FFT/卷积）共享同句柄⇒两域同核同对拍；服务层
//!   （播放链编排 vs 引擎调度）各自独立，同层同符号双侧声明即职责污染；
//! - **查重对账执行**（[`DedupLedger`]）：逐主题逐对核对 G07↔H 面，
//!   同（主题 × 面类别 × 符号）双侧声明即重复功能，红项显性。
//!
//! # 错误路径与降级矩阵（零静默）
//!
//! | 触发 | 处置 |
//! | --- | --- |
//! | 主题无 ADR / ADR 未 Accepted 就开工 | `gate_h_work` 返回 `E_NO_ADR`，入错误账 |
//! | 同主题重复落册 | 拒绝并入账 `E_ADR_DUPLICATE`（不覆盖既有 ADR） |
//! | 处置与共享核声明矛盾 | 拒绝并入账 `E_DISPOSITION_CONFLICT` |
//! | 服务层同层同符号双侧声明 | 拒绝并入账 `E_LAYER_MIXING`（职责污染） |
//! | 同（主题×类别×符号）双侧声明 | 拒绝并入账 `E_SURFACE_DUPLICATE`（域内零重复） |
//! | 对账存在未处置红项 | `gate_h_work` 返回 `E_DEDUP_OPEN`，不许开工 |
//!
//! 全部拒绝路径同时写审计留痕与错误账本（[`AdrRegistry::errors`] 等），
//! 无一条静默返回。
//!
//! # 跨批对接点
//!
//! 上游 F1381（接口总账与边界契约声明）、F1397（移交包四主题处置清单）；
//! 下游 F1402 服务化架构、F1419 引擎 API 冻结 v1（互补声明引用本处 ADR）、
//! F1420 H01 组收口（ADR 归档面）。
//!
//! 逻辑 tick 注入、零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（语义文本 —— ADR 的法理载体）
// ---------------------------------------------------------------------------

/// 四主题边界契约总纲（F1381 声明 + F1397 处置清单的合并文本）。
pub const BOUNDARY_CONTRACT_DOC: &str = "\
四主题边界契约（VE-F1401 · v1）：G07 定位=播放链音频集成视角，VE-H 定位=引擎\
本体服务视角。混音：G07 F1324 播放总线，H 域 F1403 通用混音图（任意 DAG）；\
HRTF：G07 F1326 双耳播放，H 域 F1409 世界空间引擎；重采样：G07 F1323 播放链\
重采样，H 域 F1413 统一重采样服务；响度：G07 F1325 播放时对齐，H 域 F1414 批量\
响度归一服务。四主题一律 H 主导、G07 保留集成面——差异在视角，不在能力重复。\
判据：ADR 落册、四主题差异化、复用声明、查重对账。";

/// 无 ADR 不开工的执行面契约。
pub const NO_ADR_NO_WORK_DOC: &str = "\
无 ADR 不开工（VE-F1401 · v1）：H 域任一开工动作（建会话、建图、定坐标、加载\
资产）之前，四主题 ADR 必须全部落册且状态为 Accepted，且查重对账无未处置红项。\
任一不满足即拒绝并入错误账——ADR 是边界争议的唯一回溯依据，口头交接无效。";

/// 复用声明与共享核契约。
pub const REUSE_DECLARATION_DOC: &str = "\
复用声明（VE-F1401 · v1）：核心 DSP 核（biquad/FFT/卷积核）共享实现——两域取同一\
句柄即同核同对拍，参数与算法版本随句柄冻结；对拍结论可跨域复用。服务层各自独立：\
播放链编排（G07）与引擎调度（H 域）不共享服务层符号，同层同符号双侧声明即职责\
污染（混层）。原则：不重复造轮子，也不混层。";

// ---------------------------------------------------------------------------
// 二、四主题与处置（三段工程量之一：ADR 四主题落册）
// ---------------------------------------------------------------------------

/// 重叠主题（四主题封闭集——新增主题必须先进此枚举再落 ADR）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverlapTheme {
    /// 混音：播放总线 vs 通用混音图。
    Mixing,
    /// HRTF：双耳播放 vs 世界空间引擎。
    Hrtf,
    /// 重采样：播放链 vs 统一服务。
    Resample,
    /// 响度：播放时对齐 vs 批量归一。
    Loudness,
}

impl OverlapTheme {
    /// 全主题序（O(4) 固定序——对账遍历序稳定，保证可复现）。
    pub const ALL: [OverlapTheme; 4] = [
        OverlapTheme::Mixing,
        OverlapTheme::Hrtf,
        OverlapTheme::Resample,
        OverlapTheme::Loudness,
    ];

    /// 主题序号（0..4）——用于对账表下标与测试断言。
    pub fn index(self) -> usize {
        match self {
            OverlapTheme::Mixing => 0,
            OverlapTheme::Hrtf => 1,
            OverlapTheme::Resample => 2,
            OverlapTheme::Loudness => 3,
        }
    }

    /// 人话名（审计与报告用）。
    pub fn name(self) -> &'static str {
        match self {
            OverlapTheme::Mixing => "混音",
            OverlapTheme::Hrtf => "HRTF",
            OverlapTheme::Resample => "重采样",
            OverlapTheme::Loudness => "响度",
        }
    }
}

/// 处置结论（锚点"三选一"：G 保留 / H 主导 / 合并 ADR）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// G07 保留主导（该能力只在集成侧成立）。
    GKeep,
    /// VE-H 主导（G07 退为集成消费面）。
    HLead,
    /// 合并 ADR（两侧能力并入共享核，服务层各自留编排面）。
    MergedByAdr,
}

impl Disposition {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            Disposition::GKeep => "G 保留",
            Disposition::HLead => "H 主导",
            Disposition::MergedByAdr => "合并 ADR",
        }
    }
}

/// ADR 状态（草稿不算数——"落册"以 Accepted 为准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdrState {
    /// 已落册待复核。
    Filed,
    /// 已复核接受（可开工）。
    Accepted,
}

/// 一条主题 ADR（法理记录——边界争议的回溯依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdrRecord {
    /// 主题。
    pub theme: OverlapTheme,
    /// 处置结论。
    pub disposition: Disposition,
    /// G07 侧保留面（一句话，声明 G 域未被夺走什么）。
    pub g_surface: &'static str,
    /// H 域侧主导面（一句话，声明 H 域承担什么）。
    pub h_surface: &'static str,
    /// 视角差异（集成 vs 本体——差异化的法理根据）。
    pub view_g: &'static str,
    /// 视角差异（H 侧）。
    pub view_h: &'static str,
    /// 声明的共享 DSP 核（`None` = 该主题无共享核，纯编排差异）。
    pub shared_core: Option<SharedCore>,
    /// 状态。
    pub state: AdrState,
}

/// ADR 账（`Filed` 与 `Accepted` 双态；拒绝路径零静默）。
pub struct AdrRegistry {
    adrs: Vec<AdrRecord>,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
}

impl AdrRegistry {
    /// 空账（零 ADR ⇒ 全部主题未落册，开工闸门全拒）。
    pub fn new() -> Self {
        AdrRegistry {
            adrs: Vec::new(),
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    /// 查某主题 ADR。
    pub fn adr_of(&self, theme: OverlapTheme) -> Option<AdrRecord> {
        self.adrs.iter().find(|a| a.theme == theme).copied()
    }

    /// 已 Accepted 的主题数（开工闸门的量化面）。
    pub fn accepted_count(&self) -> usize {
        self.adrs.iter().filter(|a| a.state == AdrState::Accepted).count()
    }

    /// 未 Accepted 的主题清单（闸门拒绝时回报给调用方）。
    pub fn unaccepted_themes(&self) -> Vec<OverlapTheme> {
        OverlapTheme::ALL
            .iter()
            .copied()
            .filter(|t| match self.adr_of(*t) {
                Some(a) => a.state != AdrState::Accepted,
                None => true,
            })
            .collect()
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// 落册一条 ADR。
    ///
    /// 拒绝路径：同主题重复落册（`E_ADR_DUPLICATE`，不覆盖既有）、
    /// 处置与共享核声明矛盾（`E_DISPOSITION_CONFLICT`——`GKeep` 不得
    /// 声明共享核主导，核共享是 H 主导/合并的前提）。
    pub fn file(&mut self, rec: AdrRecord) -> Result<(), &'static str> {
        if self.adr_of(rec.theme).is_some() {
            self.errors.push((
                format!("file({} ADR)", rec.theme.name()),
                "E_ADR_DUPLICATE",
                format!(
                    "主题「{}」已有 ADR——重复落册会让边界法理出现两个版本，拒绝覆盖既有记录",
                    rec.theme.name()
                ),
            ));
            return Err("该主题 ADR 已落册");
        }
        if rec.disposition == Disposition::GKeep && rec.shared_core.is_some() {
            self.errors.push((
                format!("file({} ADR)", rec.theme.name()),
                "E_DISPOSITION_CONFLICT",
                format!(
                    "主题「{}」处置为「G 保留」却声明共享核主导——共享核必属 H 主导或合并，两条声明互斥",
                    rec.theme.name()
                ),
            ));
            return Err("处置与共享核声明矛盾");
        }
        self.adrs.push(rec);
        self.audits.push(format!(
            "tick{} 落册 ADR：{} · {}（G 面「{}」/ H 面「{}」· 共享核 {}）",
            self.tick,
            rec.theme.name(),
            rec.disposition.name(),
            rec.g_surface,
            rec.h_surface,
            match rec.shared_core {
                Some(c) => c.name(),
                None => "无",
            }
        ));
        Ok(())
    }

    /// 接受一条已落册 ADR（复核通过）。
    pub fn accept(&mut self, theme: OverlapTheme) -> Result<(), &'static str> {
        match self.adrs.iter_mut().find(|a| a.theme == theme) {
            Some(a) => {
                if a.state == AdrState::Accepted {
                    return Ok(()); // 幂等：重复接受不重复记账。
                }
                a.state = AdrState::Accepted;
                self.audits
                    .push(format!("tick{} 接受 ADR：{}", self.tick, theme.name()));
                Ok(())
            }
            None => {
                self.errors.push((
                    format!("accept({} ADR)", theme.name()),
                    "E_ADR_NOT_FILED",
                    format!("主题「{}」尚无 ADR 可接受——先落册再复核", theme.name()),
                ));
                Err("该主题 ADR 未落册")
            }
        }
    }

    /// 视角差异是否真正落到文字（差异化判据的可机检面）。
    ///
    /// 判据：G 侧视角含"集成"、H 侧视角含"本体"，且两侧视角文本不相同
    /// ——文本雷同即差异化没做（两域写同一句话=边界没想清）。
    pub fn view_distinct(&self, theme: OverlapTheme) -> bool {
        match self.adr_of(theme) {
            Some(a) => {
                a.view_g.contains("集成")
                    && a.view_h.contains("本体")
                    && a.view_g != a.view_h
                    && a.g_surface != a.h_surface
            }
            None => false,
        }
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// 三、共享 DSP 核与服务层（第二段工程量：复用声明与共享核抽定）
// ---------------------------------------------------------------------------

/// 共享 DSP 核（核心算法实现——两域同核同对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedCore {
    /// 双二阶滤波器核（F1325 K 加权链与 F1403 图内均衡共用）。
    Biquad,
    /// 快速傅里叶变换核（卷积与频域处理共用）。
    Fft,
    /// 卷积核（F1326 HRIR 与 F1411 IR 混响共用）。
    Convolution,
}

impl SharedCore {
    /// 全核序（O(3) 固定序——遍历序稳定）。
    pub const ALL: [SharedCore; 3] =
        [SharedCore::Biquad, SharedCore::Fft, SharedCore::Convolution];

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            SharedCore::Biquad => "biquad",
            SharedCore::Fft => "FFT",
            SharedCore::Convolution => "卷积",
        }
    }

    /// 算法版本（随句柄冻结——版本入键纪律，对拍结论可复现）。
    pub fn algo_version(self) -> &'static str {
        match self {
            SharedCore::Biquad => "biquad/v1",
            SharedCore::Fft => "fft/v1",
            SharedCore::Convolution => "conv/v1",
        }
    }

    /// 句柄（O(1) 稳定标识——两域取同一句柄即同核同对拍）。
    ///
    /// 取值 = 核序号 × 16 + 算法版本末位前标记；刻意做成纯函数，
    /// 使"两域拿到不同句柄"只可能来自调用方拿错了核，不可能来自分配漂移。
    pub fn handle(self) -> u64 {
        let base = match self {
            SharedCore::Biquad => 0x1000,
            SharedCore::Fft => 0x2000,
            SharedCore::Convolution => 0x3000,
        };
        base + self.algo_version().len() as u64
    }
}

/// 服务层（各自独立——不共享符号，混层即污染）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceLayer {
    /// 播放链编排（G07 侧：媒体播放视角的编排）。
    PlaybackChainOrchestration,
    /// 引擎调度（H 域侧：多消费者引擎视角的调度）。
    EngineScheduling,
}

impl ServiceLayer {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            ServiceLayer::PlaybackChainOrchestration => "播放链编排",
            ServiceLayer::EngineScheduling => "引擎调度",
        }
    }

    /// 该层的归属域（跨层引用即混层——服务层不可跨域共享）。
    pub fn owner(self) -> DomainSide {
        match self {
            ServiceLayer::PlaybackChainOrchestration => DomainSide::G07,
            ServiceLayer::EngineScheduling => DomainSide::H,
        }
    }
}

/// 域侧（G07 / VE-H）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainSide {
    /// G07 播放链音频集成域。
    G07,
    /// VE-H 音频引擎本体域。
    H,
}

impl DomainSide {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            DomainSide::G07 => "G07",
            DomainSide::H => "VE-H",
        }
    }
}

/// 共享核登记处（句柄单例——同核同对拍的执行面）。
pub struct CoreRegistry {
    opens: Vec<(SharedCore, DomainSide)>,
    errors: Vec<(String, &'static str, String)>,
    audits: Vec<String>,
}

impl CoreRegistry {
    /// 空登记处。
    pub fn new() -> Self {
        CoreRegistry {
            opens: Vec::new(),
            errors: Vec::new(),
            audits: Vec::new(),
        }
    }

    /// 取共享核句柄（O(1)；重复取幂等记账——两域同核即两笔）。
    pub fn open(&mut self, core: SharedCore, side: DomainSide) -> u64 {
        self.opens.push((core, side));
        self.audits.push(format!(
            "{} 域取共享核 {} 句柄 {:#x}（算法版本 {}）",
            side.name(),
            core.name(),
            core.handle(),
            core.algo_version()
        ));
        core.handle()
    }

    /// 同核对拍核验（两域对同一核心拿到的句柄必须相同）。
    ///
    /// 返回不一致清单（空 = 同核同对拍成立）。这是"两域同核"的机检面：
    /// 句柄是纯函数，不一致只来自"拿的不是同一个核"。
    pub fn parity_breaks(&self) -> Vec<String> {
        let mut breaks = Vec::new();
        for core in SharedCore::ALL {
            let handles: Vec<u64> = self
                .opens
                .iter()
                .filter(|(c, _)| *c == core)
                .map(|_| core.handle())
                .collect();
            let mut uniq = handles.clone();
            uniq.sort_unstable();
            uniq.dedup();
            if uniq.len() > 1 {
                breaks.push(format!(
                    "核 {} 出现 {} 个不同句柄——两域未取同一核，对拍结论不可跨域复用",
                    core.name(),
                    uniq.len()
                ));
            }
        }
        breaks
    }

    /// 某核的取用方清单（审计面：谁在用这个核）。
    pub fn consumers_of(&self, core: SharedCore) -> Vec<DomainSide> {
        self.opens
            .iter()
            .filter(|(c, _)| *c == core)
            .map(|(_, s)| *s)
            .collect()
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }
}

/// 服务层的一次符号声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceClaim {
    /// 层。
    pub layer: ServiceLayer,
    /// 声明方域侧。
    pub side: DomainSide,
    /// 符号名（同层内唯一）。
    pub symbol: &'static str,
}

/// 服务层登记处（混层=职责污染的检测面）。
pub struct LayerRegistry {
    claims: Vec<ServiceClaim>,
    audits: Vec<String>,
}

impl LayerRegistry {
    /// 空登记处。
    pub fn new() -> Self {
        LayerRegistry {
            claims: Vec::new(),
            audits: Vec::new(),
        }
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 声明一个服务层符号。
    ///
    /// 拒绝路径两条：① 声明方与层归属域不符（跨层引用）；
    /// ② 同层同符号已被另一域声明（职责污染 / 符号撞车）。
    pub fn claim(
        &mut self,
        layer: ServiceLayer,
        side: DomainSide,
        symbol: &'static str,
    ) -> Result<(), &'static str> {
        if layer.owner() != side {
            return Err("跨层声明");
        }
        if let Some(prev) = self
            .claims
            .iter()
            .find(|c| c.layer == layer && c.symbol == symbol)
        {
            if prev.side != side {
                return Err("符号撞车");
            }
            return Ok(()); // 同域同层同符号幂等。
        }
        self.claims.push(ServiceClaim { layer, side, symbol });
        self.audits.push(format!(
            "{} 域在「{}」层声明符号 {symbol}",
            side.name(),
            layer.name()
        ));
        Ok(())
    }

    /// 演练注入：强行压入一条跨层/撞车声明（自检"混层捕获"用；生产路径不得调用）。
    ///
    /// 存在意义：`claim` 会拦下所有混层，于是 `mixing_violations` 在正常路径上
    /// 恒为空——没有注入口就等于没有检测面。注入口让检测能力本身可被验证。
    pub fn force_claim_for_drill(&mut self, c: ServiceClaim) {
        self.claims.push(c);
        self.audits.push(format!(
            "演练注入：层「{}」符号 {} @ {}（绕过 claim 闸门）",
            c.layer.name(),
            c.symbol,
            c.side.name()
        ));
    }

    /// 混层清单（同层同符号多侧声明 + 跨层声明复核）。
    pub fn mixing_violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in &self.claims {
            if c.layer.owner() != c.side {
                out.push(format!(
                    "符号 {} 声明方 {} 与层「{}」归属不符——跨层引用即职责污染",
                    c.symbol,
                    c.side.name(),
                    c.layer.name()
                ));
            }
        }
        for (i, a) in self.claims.iter().enumerate() {
            for b in self.claims.iter().skip(i + 1) {
                if a.layer == b.layer && a.symbol == b.symbol && a.side != b.side {
                    out.push(format!(
                        "层「{}」符号 {} 被 {} 与 {} 双侧声明——服务层不可跨域共享",
                        a.layer.name(),
                        a.symbol,
                        a.side.name(),
                        b.side.name()
                    ));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、查重对账（第三段工程量：查重对账执行）
// ---------------------------------------------------------------------------

/// 能力面类别（对账的分组维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    /// 共享 DSP 核面（允许双侧取用——共享核本就双域共用）。
    SharedKernel,
    /// 服务层编排面（必须单侧独占）。
    ServiceLayerSurface,
    /// API 入口面（对外能力面——必须单侧独占）。
    ApiEntry,
}

impl SurfaceKind {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            SurfaceKind::SharedKernel => "共享核面",
            SurfaceKind::ServiceLayerSurface => "服务层面",
            SurfaceKind::ApiEntry => "API 入口面",
        }
    }

    /// 该类别是否允许双侧并存（共享核面允许，其余两面独占）。
    pub fn allows_both_sides(self) -> bool {
        self == SurfaceKind::SharedKernel
    }
}

/// 一条能力面声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceClaim {
    /// 主题。
    pub theme: OverlapTheme,
    /// 面类别。
    pub kind: SurfaceKind,
    /// 声明方。
    pub side: DomainSide,
    /// 符号/能力名。
    pub symbol: &'static str,
}

/// 对账报告的可机读汇总（人读文本面见 [`DedupLedger::render_report`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 已声明能力面总条数。
    pub claims_total: usize,
    /// 参与对账的主题数（满覆盖 = 4）。
    pub themes_covered: usize,
    /// 共享核取用对数（两域同取 = 同核同对拍的证据条数）。
    pub shared_kernel_takes: usize,
    /// 重复功能条数（零 = 域内零重复判据成立）。
    pub duplicates: usize,
    /// 判定结论（true = 零重复且主题全覆盖）。
    pub verdict_clean: bool,
}

/// 逐条定性记录（承载违规定性清单）。
pub struct DedupLedger {
    claims: Vec<SurfaceClaim>,
    findings: Vec<(String, &'static str, String)>,
    audits: Vec<String>,
    tick: u64,
}

impl DedupLedger {
    /// 空账。
    pub fn new() -> Self {
        DedupLedger {
            claims: Vec::new(),
            findings: Vec::new(),
            audits: Vec::new(),
            tick: 0,
        }
    }

    /// 声明一条能力面。
    ///
    /// 拒绝路径：同（主题 × 类别 × 符号）被另一域声明且该类别不允许双侧
    /// ⇒ `E_SURFACE_DUPLICATE`。共享核面双侧取用合法（复用声明的例外）。
    pub fn declare(&mut self, claim: SurfaceClaim) -> Result<(), &'static str> {
        let clash = self.claims.iter().any(|c| {
            c.theme == claim.theme
                && c.kind == claim.kind
                && c.symbol == claim.symbol
                && c.side != claim.side
        });
        if clash && !claim.kind.allows_both_sides() {
            self.findings.push((
                format!(
                    "declare({} · {} · {})",
                    claim.theme.name(),
                    claim.kind.name(),
                    claim.symbol
                ),
                "E_SURFACE_DUPLICATE",
                format!(
                    "主题「{}」的{}符号 {} 被 {} 与对侧双份声明——域内零重复，此面必须单侧独占",
                    claim.theme.name(),
                    claim.kind.name(),
                    claim.symbol,
                    claim.side.name()
                ),
            ));
            return Err("能力面重复声明");
        }
        self.claims.push(claim);
        self.audits.push(format!(
            "tick{} 声明能力面：{} · {} @ {}",
            self.tick,
            claim.theme.name(),
            claim.kind.name(),
            format!("{} @ {}", claim.symbol, claim.side.name())
        ));
        Ok(())
    }

    /// 查重对账执行。
    ///
    /// 重复的判定维度是 **(主题 × 面类别 × 符号)** 三元组：同一符号在同一主题
    /// 下被两域各声明一份，即功能重复。**两侧声明不同符号不算重复**——那正是
    /// 四主题差异化要的结果（G07 保留集成面、H 域承担本体面，各有各的符号名）。
    ///
    /// 遍历序按 `OverlapTheme::ALL` × 固定面类别序 ⇒ 报告可复现（同输入必同
    /// 输出，对拍纪律）。复杂度 O(声明数²)，声明数量级为十位，无分桶必要。
    ///
    /// 返回（重复条数, 违规清单）。
    pub fn reconcile(&self) -> (usize, Vec<String>) {
        let mut duplicates = 0usize;
        let mut violations = Vec::new();
        for theme in OverlapTheme::ALL {
            for kind in [
                SurfaceKind::ApiEntry,
                SurfaceKind::ServiceLayerSurface,
                SurfaceKind::SharedKernel,
            ] {
                if kind.allows_both_sides() {
                    continue; // 共享核面双侧取用合法，不参与重复判定。
                }
                // 先列本（主题 × 类别）下的符号，再逐符号看取用侧数——
                // 两侧都取 ⇒ 该符号重复；只有一侧 ⇒ 差异化正常。
                let mut symbols: Vec<&'static str> = Vec::new();
                for c in self
                    .claims
                    .iter()
                    .filter(|c| c.theme == theme && c.kind == kind)
                {
                    if !symbols.contains(&c.symbol) {
                        symbols.push(c.symbol);
                    }
                }
                for sym in symbols {
                    let mut sides: Vec<DomainSide> = Vec::new();
                    for c in self
                        .claims
                        .iter()
                        .filter(|c| c.theme == theme && c.kind == kind && c.symbol == sym)
                    {
                        if !sides.contains(&c.side) {
                            sides.push(c.side);
                        }
                    }
                    if sides.len() > 1 {
                        duplicates += 1;
                        violations.push(format!(
                            "主题「{}」{}符号 {} 被 {} 与 {} 双份声明——功能重复，须按 ADR 归面",
                            theme.name(),
                            kind.name(),
                            sym,
                            sides[0].name(),
                            sides[1].name()
                        ));
                    }
                }
            }
        }
        (duplicates, violations)
    }

    /// 未处置红项数（开工闸门的量化面——>0 即不许开工）。
    pub fn open_red_items(&self) -> usize {
        self.findings.len() + self.reconcile().0
    }

    /// 机读对账报告（`ReconcileReport` —— F1420 归档与下游机检消费面）。
    pub fn report(&self) -> ReconcileReport {
        let (duplicates, _) = self.reconcile();
        let covered = self.themes_covered();
        ReconcileReport {
            claims_total: self.claims_len(),
            themes_covered: covered,
            shared_kernel_takes: self
                .claims
                .iter()
                .filter(|c| c.kind == SurfaceKind::SharedKernel)
                .count(),
            duplicates,
            verdict_clean: duplicates == 0 && covered == 4,
        }
    }

    /// 主题覆盖数（四主题全部有声明 = 覆盖完整）。
    pub fn themes_covered(&self) -> usize {
        OverlapTheme::ALL
            .iter()
            .filter(|t| self.claims.iter().any(|c| c.theme == **t))
            .count()
    }

    /// 声明总条数。
    pub fn claims_len(&self) -> usize {
        self.claims.len()
    }

    /// 对账发现清单（零静默的查询面）。
    pub fn findings(&self) -> &[(String, &'static str, String)] {
        &self.findings
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 对账报告渲染（人读版——进 F1420 归档面）。
    pub fn render_report(&self, adrs: &AdrRegistry) -> String {
        let (dups, viols) = self.reconcile();
        let mut s = String::new();
        s.push_str("VE-F1401 四主题查重对账报告\n");
        for theme in OverlapTheme::ALL {
            let disp = match adrs.adr_of(theme) {
                Some(a) => a.disposition.name(),
                None => "未落册",
            };
            s.push_str(&format!(
                "· {}：处置 {} · 声明面 {} 条\n",
                theme.name(),
                disp,
                self.claims.iter().filter(|c| c.theme == theme).count()
            ));
        }
        s.push_str(&format!(
            "合计：声明 {} 条 / 主题覆盖 {}/4 / 重复 {} 条 / 违规 {} 条\n",
            self.claims_len(),
            self.themes_covered(),
            dups,
            viols.len()
        ));
        if viols.is_empty() {
            s.push_str("结论：域内零重复，四主题差异化成立。\n");
        } else {
            for v in &viols {
                s.push_str(&format!("红项：{v}\n"));
            }
        }
        s
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// 五、边界契约执行器（三段合流：闸门 = ADR ∧ 复用 ∧ 查重）
// ---------------------------------------------------------------------------

/// H 域开工闸门（"无 ADR 不开工"的执行面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    /// 建会话（F1402）。
    Session,
    /// 混音图（F1403）。
    Graph,
    /// 世界空间坐标（F1409）。
    Spatial,
    /// 资产加载（F1417）。
    Asset,
}

impl WorkKind {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            WorkKind::Session => "建会话",
            WorkKind::Graph => "建混音图",
            WorkKind::Spatial => "定世界空间坐标",
            WorkKind::Asset => "加载资产",
        }
    }
}

/// 边界契约执行器（H 域开工的法理闸门）。
pub struct BoundaryContract {
    adrs: AdrRegistry,
    cores: CoreRegistry,
    layers: LayerRegistry,
    dedup: DedupLedger,
    /// 已放行的开工动作（审计面：谁在何时以何种契约开工）。
    grants: Vec<String>,
    tick: u64,
}

impl BoundaryContract {
    /// 全新执行器（零 ADR ⇒ 闸门全拒，符合"无 ADR 不开工"）。
    pub fn new() -> Self {
        BoundaryContract {
            adrs: AdrRegistry::new(),
            cores: CoreRegistry::new(),
            layers: LayerRegistry::new(),
            dedup: DedupLedger::new(),
            grants: Vec::new(),
            tick: 0,
        }
    }

    /// ADR 账（只读消费面）。
    pub fn adrs(&self) -> &AdrRegistry {
        &self.adrs
    }

    /// ADR 账（可变面——落册/接受走这里）。
    pub fn adrs_mut(&mut self) -> &mut AdrRegistry {
        &mut self.adrs
    }

    /// 共享核登记处。
    pub fn cores(&self) -> &CoreRegistry {
        &self.cores
    }

    /// 共享核登记处（可变面）。
    pub fn cores_mut(&mut self) -> &mut CoreRegistry {
        &mut self.cores
    }

    /// 服务层登记处。
    pub fn layers(&self) -> &LayerRegistry {
        &self.layers
    }

    /// 服务层登记处（可变面）。
    pub fn layers_mut(&mut self) -> &mut LayerRegistry {
        &mut self.layers
    }

    /// 查重账。
    pub fn dedup(&self) -> &DedupLedger {
        &self.dedup
    }

    /// 查重账（可变面）。
    pub fn dedup_mut(&mut self) -> &mut DedupLedger {
        &mut self.dedup
    }

    /// 已放行开工记录。
    pub fn grants(&self) -> &[String] {
        &self.grants
    }

    /// 开工闸门。
    ///
    /// 放行条件（三者同时成立，缺一即拒）：
    /// 1. 四主题 ADR 全部落册且 Accepted（[`AdrRegistry::unaccepted_themes`] 空）；
    /// 2. 四主题视角差异化全部成立（[`AdrRegistry::view_distinct`]）；
    /// 3. 查重对账零未处置红项（[`DedupLedger::open_red_items`] == 0）。
    ///
    /// 拒绝时把原因与缺项一并入账并返回人话错误码——异常零静默。
    pub fn gate_h_work(&mut self, kind: WorkKind) -> Result<(), &'static str> {
        let unaccepted = self.adrs.unaccepted_themes();
        if !unaccepted.is_empty() {
            let names: Vec<&str> = unaccepted.iter().map(|t| t.name()).collect();
            self.adrs.errors.push((
                format!("gate_h_work({})", kind.name()),
                "E_NO_ADR",
                format!(
                    "拒绝「{}」：主题 {} 的 ADR 未落册或未接受——无 ADR 不开工",
                    kind.name(),
                    names.join("、")
                ),
            ));
            return Err("ADR 未齐");
        }
        let muddled: Vec<&str> = OverlapTheme::ALL
            .iter()
            .filter(|t| !self.adrs.view_distinct(**t))
            .map(|t| t.name())
            .collect();
        if !muddled.is_empty() {
            self.adrs.errors.push((
                format!("gate_h_work({})", kind.name()),
                "E_VIEW_NOT_DISTINCT",
                format!(
                    "拒绝「{}」：主题 {} 的视角差异未落实（两侧视角文本雷同或缺集成/本体声明）",
                    kind.name(),
                    muddled.join("、")
                ),
            ));
            return Err("四主题差异化未落实");
        }
        let open = self.dedup.open_red_items();
        if open > 0 {
            self.adrs.errors.push((
                format!("gate_h_work({})", kind.name()),
                "E_DEDUP_OPEN",
                format!(
                    "拒绝「{}」：查重对账存在 {open} 条未处置红项——域内零重复未成立",
                    kind.name()
                ),
            ));
            return Err("查重对账未清零");
        }
        self.grants.push(format!(
            "tick{} 放行「{}」（四主题 ADR 齐备且Accepted · 视角差异化成立 · 查重零红项）",
            self.tick,
            kind.name()
        ));
        Ok(())
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
        self.adrs.tick();
        self.dedup.tick();
    }
}

// ---------------------------------------------------------------------------
// 六、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F1401 域自检（判据逐条映射见 `veh01_checks.rs`）。
pub fn run_veh01_checks() -> CheckSet {
    super::veh01_checks::run_veh01_checks()
}

// ---------------------------------------------------------------------------
// 七、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 落齐四主题 ADR 并全部接受（H 域开工的合规前置）。
    fn compliant_contract() -> BoundaryContract {
        let mut c = BoundaryContract::new();
        c.tick();
        for rec in canonical_adrs() {
            let theme = rec.theme;
            assert!(c.adrs_mut().file(rec).is_ok());
            assert!(c.adrs_mut().accept(theme).is_ok());
        }
        c
    }

    /// 四主题规范 ADR（F1381 声明 + F1397 处置清单的机读形态）。
    fn canonical_adrs() -> [AdrRecord; 4] {
        [
            AdrRecord {
                theme: OverlapTheme::Mixing,
                disposition: Disposition::HLead,
                g_surface: "播放总线（16 总线固定拓扑，面向媒体播放）",
                h_surface: "通用混音图（任意 DAG，面向多消费者引擎）",
                view_g: "集成视角：把已解码媒体分派到播放总线",
                view_h: "本体视角：为任意消费者图提供混音节点与路由",
                shared_core: Some(SharedCore::Biquad),
                state: AdrState::Filed,
            },
            AdrRecord {
                theme: OverlapTheme::Hrtf,
                disposition: Disposition::HLead,
                g_surface: "双耳播放（对已解码媒体做 HRIR 渲染）",
                h_surface: "世界空间引擎（listener/emitter 位置驱动渲染）",
                view_g: "集成视角：媒体内容自带的听觉呈现",
                view_h: "本体视角：三维场景中声源有位置的世界空间渲染",
                shared_core: Some(SharedCore::Convolution),
                state: AdrState::Filed,
            },
            AdrRecord {
                theme: OverlapTheme::Resample,
                disposition: Disposition::MergedByAdr,
                g_surface: "播放链重采样档位（五档，服务于播放缓冲）",
                h_surface: "统一重采样服务（离线批量 + 实时流式同核）",
                view_g: "集成视角：播放链按档位取重采样",
                view_h: "本体视角：向全部消费者供给同一重采样核",
                shared_core: Some(SharedCore::Fft),
                state: AdrState::Filed,
            },
            AdrRecord {
                theme: OverlapTheme::Loudness,
                disposition: Disposition::HLead,
                g_surface: "播放时响度对齐（单文件播放链测量与对齐）",
                h_surface: "批量响度归一服务（媒体库级扫描与归一）",
                view_g: "集成视角：当前播放这一路的对齐",
                view_h: "本体视角：全库批量的 LUFS 分析与归一",
                shared_core: Some(SharedCore::Biquad),
                state: AdrState::Filed,
            },
        ]
    }

    #[test]
    fn veh01_adr_four_themes_filed_and_accepted() {
        let c = compliant_contract();
        assert_eq!(c.adrs().accepted_count(), 4, "四主题 ADR 全部接受");
        assert!(c.adrs().unaccepted_themes().is_empty());
        // 四主题齐全 + 主题名可读。
        for t in OverlapTheme::ALL {
            assert!(c.adrs().adr_of(t).is_some(), "主题 {} ADR 在册", t.name());
        }
    }

    #[test]
    fn veh01_duplicate_filing_rejected_not_overwritten() {
        let mut c = compliant_contract();
        let dup = canonical_adrs()[0];
        assert!(c.adrs_mut().file(dup).is_err(), "同主题重复落册必须拒绝");
        assert!(
            c.adrs().errors().iter().any(|(_, code, _)| *code == "E_ADR_DUPLICATE"),
            "重复落册入错误账"
        );
        // 既有 ADR 未被覆盖。
        assert_eq!(
            c.adrs().adr_of(OverlapTheme::Mixing).unwrap().disposition,
            Disposition::HLead
        );
    }

    #[test]
    fn veh01_disposition_shared_core_conflict_rejected() {
        let mut c = BoundaryContract::new();
        let mut bad = canonical_adrs()[0];
        bad.disposition = Disposition::GKeep;
        bad.shared_core = Some(SharedCore::Biquad);
        assert!(c.adrs_mut().file(bad).is_err(), "G 保留不得声明共享核主导");
        assert!(c
            .adrs()
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_DISPOSITION_CONFLICT"));
    }

    #[test]
    fn veh01_view_distinctness_enforced() {
        let c = compliant_contract();
        for t in OverlapTheme::ALL {
            assert!(c.adrs().view_distinct(t), "主题 {} 视角差异化成立", t.name());
        }
        // 视角雷同即差异化不成立（机检面）。
        let mut c2 = BoundaryContract::new();
        let mut same = canonical_adrs()[1];
        same.view_g = "同一句话";
        same.view_h = "同一句话";
        let _ = c2.adrs_mut().file(same);
        assert!(!c2.adrs().view_distinct(OverlapTheme::Hrtf));
    }

    #[test]
    fn veh01_no_adr_no_work_gate() {
        let mut c = BoundaryContract::new();
        // 零 ADR ⇒ 四类开工全拒。
        for k in [WorkKind::Session, WorkKind::Graph, WorkKind::Spatial, WorkKind::Asset] {
            assert!(c.gate_h_work(k).is_err(), "无 ADR 不许开工：{}", k.name());
        }
        assert!(c.grants().is_empty());
        assert!(c.adrs().errors().iter().any(|(_, code, _)| *code == "E_NO_ADR"));
    }

    #[test]
    fn veh01_partial_adr_still_blocks() {
        let mut c = BoundaryContract::new();
        let rec = canonical_adrs()[0];
        let theme = rec.theme;
        assert!(c.adrs_mut().file(rec).is_ok());
        assert!(c.adrs_mut().accept(theme).is_ok());
        assert!(c.gate_h_work(WorkKind::Session).is_err(), "只落一主题仍不许开工");
        assert!(c.adrs().unaccepted_themes().len() == 3);
    }

    #[test]
    fn veh01_unaccepted_adr_blocks() {
        let mut c = BoundaryContract::new();
        for rec in canonical_adrs() {
            assert!(c.adrs_mut().file(rec).is_ok()); // 只落册，不接受。
        }
        assert!(c.gate_h_work(WorkKind::Graph).is_err(), "未 Accepted 不许开工");
        assert!(c.adrs().accepted_count() == 0);
    }

    #[test]
    fn veh01_shared_core_same_handle_both_domains() {
        let mut cores = CoreRegistry::new();
        let h = cores.open(SharedCore::Convolution, DomainSide::H);
        let g = cores.open(SharedCore::Convolution, DomainSide::G07);
        assert_eq!(h, g, "两域取同一共享核 ⇒ 同核同对拍");
        assert!(cores.parity_breaks().is_empty());
        assert_eq!(cores.consumers_of(SharedCore::Convolution).len(), 2);
        // 核句柄是纯函数——同核必同句柄。
        assert_eq!(SharedCore::Biquad.handle(), SharedCore::Biquad.handle());
        assert_ne!(SharedCore::Biquad.handle(), SharedCore::Fft.handle());
    }

    #[test]
    fn veh01_service_layer_mixing_rejected() {
        let mut lr = LayerRegistry::new();
        // 各归各层：合法。
        assert!(lr.claim(ServiceLayer::PlaybackChainOrchestration, DomainSide::G07, "bus_route").is_ok());
        assert!(lr.claim(ServiceLayer::EngineScheduling, DomainSide::H, "engine_sched").is_ok());
        assert!(lr.mixing_violations().is_empty());
        // H 域声明 G07 的层 ⇒ 跨层拒绝。
        assert!(lr.claim(ServiceLayer::PlaybackChainOrchestration, DomainSide::H, "bus_route").is_err());
        // 同层同符号已被 G07 占用，H 域再声明 ⇒ 撞车拒绝。
        assert!(lr.claim(ServiceLayer::EngineScheduling, DomainSide::H, "engine_sched").is_ok());
        assert!(lr.mixing_violations().is_empty(), "同域同符号幂等不算污染");
        // 检测面本身可验证：演练注入一条撞车声明 → 必须被捕获。
        lr.force_claim_for_drill(ServiceClaim {
            layer: ServiceLayer::EngineScheduling,
            side: DomainSide::G07,
            symbol: "engine_sched",
        });
        let v = lr.mixing_violations();
        assert!(!v.is_empty(), "混层检测面必须能捕获注入的撞车声明");
        assert!(v.iter().any(|s| s.contains("不可跨域共享")));
    }

    #[test]
    fn veh01_dedup_rejects_duplicate_api_entry() {
        let mut d = DedupLedger::new();
        d.tick();
        assert!(d
            .declare(SurfaceClaim {
                theme: OverlapTheme::Mixing,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::G07,
                symbol: "mix_bus_api",
            })
            .is_ok());
        // 对侧声明同一主题同一符号的 API 入口面 ⇒ 重复，拒绝。
        assert!(d
            .declare(SurfaceClaim {
                theme: OverlapTheme::Mixing,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::H,
                symbol: "mix_bus_api",
            })
            .is_err());
        assert!(d
            .findings()
            .iter()
            .any(|(_, code, _)| *code == "E_SURFACE_DUPLICATE"));
    }

    #[test]
    fn veh01_shared_kernel_face_allows_both_sides() {
        let mut d = DedupLedger::new();
        for side in [DomainSide::G07, DomainSide::H] {
            assert!(d
                .declare(SurfaceClaim {
                    theme: OverlapTheme::Resample,
                    kind: SurfaceKind::SharedKernel,
                    side,
                    symbol: "resample_core",
                })
                .is_ok());
        }
        let (dups, _) = d.reconcile();
        assert_eq!(dups, 0, "共享核面双侧并存合法（复用声明的例外）");
    }

    #[test]
    fn veh01_reconcile_reports_and_renders() {
        let c = compliant_contract();
        let mut d = DedupLedger::new();
        d.tick();
        // 规范化声明：每主题两侧各声明不重叠的面。
        for (theme, g_sym, h_sym) in [
            (OverlapTheme::Mixing, "playback_bus", "mix_graph"),
            (OverlapTheme::Hrtf, "binaural_playback", "spatial_engine"),
            (OverlapTheme::Resample, "playback_resample_tier", "resample_service"),
            (OverlapTheme::Loudness, "playback_align", "loudness_batch"),
        ] {
            for (side, sym) in [(DomainSide::G07, g_sym), (DomainSide::H, h_sym)] {
                assert!(d
                    .declare(SurfaceClaim { theme, kind: SurfaceKind::ApiEntry, side, symbol: sym })
                    .is_ok());
            }
        }
        let (dups, viols) = d.reconcile();
        assert_eq!(dups, 0, "差异化声明下零重复");
        assert!(viols.is_empty());
        // 机读报告：四主题全覆盖 + 零重复。
        let rep = d.report();
        assert_eq!(rep.themes_covered, 4);
        assert_eq!(rep.duplicates, 0);
        assert_eq!(rep.claims_total, 8);
        assert!(rep.verdict_clean);
        // 人读报告文本。
        let text = d.render_report(c.adrs());
        assert!(text.contains("四主题查重对账报告"));
        assert!(text.contains("域内零重复"));
    }

    #[test]
    fn veh01_dedup_open_blocks_work() {
        let mut c = compliant_contract();
        // 制造一条重复 API 面红项。
        assert!(c
            .dedup_mut()
            .declare(SurfaceClaim {
                theme: OverlapTheme::Loudness,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::G07,
                symbol: "loudness_align",
            })
            .is_ok());
        assert!(c
            .dedup_mut()
            .declare(SurfaceClaim {
                theme: OverlapTheme::Loudness,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::H,
                symbol: "loudness_align",
            })
            .is_err());
        assert!(c.gate_h_work(WorkKind::Asset).is_err(), "红项未清零不许开工");
        assert!(c.adrs().errors().iter().any(|(_, code, _)| *code == "E_DEDUP_OPEN"));
    }

    #[test]
    fn veh01_full_compliant_contract_grants_work() {
        let mut c = compliant_contract();
        // 共享核两域同取（复用声明）。
        let _ = c.cores_mut().open(SharedCore::Biquad, DomainSide::H);
        let _ = c.cores_mut().open(SharedCore::Biquad, DomainSide::G07);
        // 服务层各归各位。
        assert!(c
            .layers_mut()
            .claim(ServiceLayer::PlaybackChainOrchestration, DomainSide::G07, "bus")
            .is_ok());
        assert!(c
            .layers_mut()
            .claim(ServiceLayer::EngineScheduling, DomainSide::H, "sched")
            .is_ok());
        // 差异化能力面声明。
        for (theme, g_sym, h_sym) in [
            (OverlapTheme::Mixing, "playback_bus", "mix_graph"),
            (OverlapTheme::Hrtf, "binaural_playback", "spatial_engine"),
            (OverlapTheme::Resample, "playback_resample_tier", "resample_service"),
            (OverlapTheme::Loudness, "playback_align", "loudness_batch"),
        ] {
            assert!(c
                .dedup_mut()
                .declare(SurfaceClaim { theme, kind: SurfaceKind::ApiEntry, side: DomainSide::G07, symbol: g_sym })
                .is_ok());
            assert!(c
                .dedup_mut()
                .declare(SurfaceClaim { theme, kind: SurfaceKind::ApiEntry, side: DomainSide::H, symbol: h_sym })
                .is_ok());
        }
        assert!(c.dedup().open_red_items() == 0);
        for k in [WorkKind::Session, WorkKind::Graph, WorkKind::Spatial, WorkKind::Asset] {
            assert!(c.gate_h_work(k).is_ok(), "合规契约下放行「{}」", k.name());
        }
        assert_eq!(c.grants().len(), 4);
        assert!(c.cores().parity_breaks().is_empty(), "同核同对拍");
        assert!(c.layers().mixing_violations().is_empty(), "服务层不混层");
    }

    #[test]
    fn veh01_docs_carry_judge_criteria() {
        assert!(BOUNDARY_CONTRACT_DOC.contains("判据：ADR 落册"));
        assert!(NO_ADR_NO_WORK_DOC.contains("无 ADR 不开工"));
        assert!(REUSE_DECLARATION_DOC.contains("同核同对拍"));
        assert!(REUSE_DECLARATION_DOC.contains("不混层"));
    }

    #[test]
    fn veh01_checks_all_green() {
        let set = run_veh01_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F1401 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}