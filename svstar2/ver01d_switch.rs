//! VE-F3404 · 主题切换事务（原子换肤）（VE-E 域 · 主题与个性化引擎 · 令牌运行时组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3404`
//!
//! **判据（锚点原文）**：原子保证、预演干跑、快照回滚、悬空兜底、判据。
//!
//! **职责定位（锚点原文）**：原子换肤（要么全换要么不换，半新半旧不存在），
//! 切换预演（新主题求值干跑），失败回滚到旧主题快照；含切换中截图一致性断言
//! （任意时刻截图不出现半新半旧）。
//!
//! **数据结构（锚点原文）**：事务管理器 [`Switchboard`]；快照回滚 [`Snapshot`]。
//!
//! ## 一、原子性是怎么保证的（不是"我们保证"，是结构上没有那个窗口）
//!
//! 换肤的朴素写法是**就地改写**生效表：拿到当前主题的值表，逐个令牌写成新主题
//! 的值。那样在改写的第 k 步与第 k+1 步之间，界面读到的是"前 k 个令牌是新的、
//! 其余还是旧的"——**半新半旧**。这不是概率问题，是那个写法必然产生的中间态。
//!
//! 本模块的做法是**双缓冲 + 单指针翻转**：
//!
//! - 生效面是 [`Switchboard::slots`] 两个槽加一个 [`Switchboard::active`] 游标；
//! - **生效槽的表一旦发布就再不被写**（[`verify_atomic`] 把这条当不变式查）；
//! - 提交 = 把整张新表**一次性**装进非生效槽，然后把游标翻过去（一次赋值）。
//!
//! 于是"读到半新半旧"这件事在结构上**没有对应的时机**：读侧要么拿到翻转前的
//! 整张旧表，要么拿到翻转后的整张新表。锚点那句"任意时刻截图不出现半新半旧"
//! 因此不是一句承诺，而是"提交路径上不存在逐令牌写生效表这个操作"。
//!
//! ## 二、截图一致性断言怎么落地（`FrameProbe` / `FrameSample::mixed_with`）
//!
//! 断言要有鉴别力，就得能说出"半新半旧"长什么样。本模块把一帧建模成**在若干
//! 探针路径上取到的值**（[`FrameSample`]），并给出判定 [`MixedVerdict`]：
//!
//! - `AllOld`：每个**会变的**探针都取到旧值；
//! - `AllNew`：每个会变的探针都取到新值；
//! - `Mixed`：有的探针新、有的探针旧 —— **这就是半新半旧**；
//! - `Foreign`：某个探针取到的值既不是旧值也不是新值 —— 比半新半旧更坏，
//!   是脏数据（读到了不属于任何一张表的东西），单列一档不与半新半旧混为一谈。
//!
//! 判定只看**会变的探针**（旧值≠新值）。不变的探针两边同值，把它算进来只会
//! 让判定永远得不出结论——那就是**采样留洞**：断言看着在跑，实际测不到任何东西。
//! 所以 [`FrameProbe::new`] 拒绝"零个会变探针"的探针集（[`SwitchCode::ProbeBlind`]），
//! 这是从 W009·VE-F1004 那个坑（语料在判定区间内无采样点，放宽阈值仍全绿）
//! 直接搬来的闸门。
//!
//! ## 三、判据的双向验证（为什么这个断言不是恒真）
//!
//! 一个只会说"没问题的"断言没有价值。所以本模块**同时实现一个故意非原子的
//! 参考发布器** [`Switchboard::publish_in_place_for_test`]：它逐令牌写生效槽，
//! 并在**每写一个令牌之后**取一帧，把整条时间线交出来。自检据此断言两件事：
//!
//! 1. 真实提交路径产出的每一帧都判为 `AllOld`（事务在途）或 `AllNew`（提交后）；
//! 2. **非原子参考发布器**产出的时间线里**至少有一帧**判为 `Mixed`。
//!
//! 第 2 条是鉴别力的证据：判定若对非原子实现也放行，那它就是恒真的空断言。
//! 只验第 1 条等于没验——这是"补判据后必须双向验证"的纪律。
//!
//! ## 四、错误路径与降级矩阵（锚点原文逐条）
//!
//! - **预演失败 → 取消切换**。[`Switchboard::begin`] 只**建暂存表**，一个字都不
//!   写生效面；[`Switchboard::preview`] 是只读报告。预演判否（[`PreviewVerdict::Rejected`]）
//!   或上游级联报错，一律 [`Switchboard::abort`]，生效面逐字节不变。
//! - **中途崩溃 → 快照恢复**。[`Journal`] 只增不改，是**仲裁者**：恢复时以账本
//!   为准而不是以生效面的现状为准——现状恰好是崩溃现场，不能当证据。账本最后
//!   一条停在 `Committing`/`Staged`/`Previewed`，[`Switchboard::recover`] 就用该条
//!   携带的快照回滚。
//! - **事务悬空 → 超时回滚**。[`Switchboard::sweep`] 按逻辑 tick 判龄，超
//!   [`TXN_DANGLE_TICKS`] 的在途事务强制回滚（[`SwitchKind::DanglingRollback`]），
//!   并把兜底次数显性化。tick 回拨时 [`SwitchCode::ClockRewind`] 拒绝推进——
//!   年龄算错会把刚开的��务当成悬空回滚掉。
//!
//! ## 五、性能逐项分解（锚点原文 O(令牌数)）
//!
//! - 预演：O(令牌数) —— 一次建表 + 一次有序归并差分，不是"两遍哈希表"；
//! - 提交：O(1) —— 装表在事务开始时已完成，提交只做一次游标翻转；
//! - 回滚：O(令牌数) —— 从快照重建一张整表（快照是值拷贝，不是引用）；
//! - 悬空扫描：O(1) —— 至多一个在途事务。
//!
//! ## 六、跨批对接点（锚点原文 E02 换肤引擎消费）
//!
//! 交出三件：稳定错误码 [`SwitchCode::wire`]、只读预演报告 [`PreviewReport`]、
//! 事务终局报告 [`SwitchReport`]。E02 直接消费，不自己算差分、不自己回滚。
//! 覆盖优先级仲裁归 F3406、类型校验归 F3405、增量流水线归 F3408，本模块一概
//! 不做。
//!
//! ## 七、无障碍与隐私（锚点原文 切换状态读屏播报）
//!
//! [`Switchboard::spoken`] 给出与画面等信息的纯文本替述：当前主题与世代、令牌
//! 条数、在途事务与其阶段的中文名、提交/回滚/兜底计数。阶段名走
//! [`JournalState::zh`]，**朗读顺序即字段顺序**，不依赖颜色与缩进。播报内容不含
//! 令牌值正文（值可能含用户自定义字符串）。
//!
//! **零墙钟、零 IO、无随机源**，时间一律由调用方注入逻辑 tick，回归可复现。

use super::ver01b_parser::{Site, TokenDag, TokenSet};
use super::ver01c_cascade::CascadeEngine;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 事务管理器版本。换肤语义变更走版本号。
pub const SWITCH_VERSION: &str = "E01-switch-v1";

/// 主题标识字节上限。主题 id 是短标识，不是正文；超长即拒绝而不是截断
/// （截断后两个不同主题会同名，那比拒绝更坏）。
pub const MAX_THEME_ID_LEN: usize = 64;

/// 单张生效表的令牌条数上限。**必须与 [`super::ver01c_cascade::MAX_GRAPH_NODES`]
/// 同值**：级联引擎放行 4096 个节点而换肤拒收 4095 条，就是"解析成功、级联成功、
/// 换肤失败"的空转。
pub const MAX_SKIN_VALUES: usize = 4096;

/// 单个令牌值的字节上限。**必须与 [`super::ver01c_cascade::MAX_VALUE_LEN`] 同值**，
/// 理由同上：两级闸不同值，总有一级的承诺是假的。
pub const MAX_VALUE_BYTES: usize = 4096;

/// 事务悬空判龄阈值（逻辑 tick）。在途事务超过这个 tick 数未被推进即视为悬空。
///
/// 取 900 而不是更小：一次真实的换肤要建表 + 级联 + 差分，让它在正常路径上
/// 绝不被误判为悬空；但也不能大到"卡住了也看不出来"。
pub const TXN_DANGLE_TICKS: u64 = 900;

/// 崩溃恢复的账本回看窗口（逻辑 tick）。比悬空阈值宽，给"崩溃前刚开的事务"
/// 留出被扫到的余地。
pub const RECOVER_WINDOW_TICKS: u64 = TXN_DANGLE_TICKS * 5;

/// 账本容量。满则 [`SwitchCode::JournalFull`] 拒绝写入——宁可显式失败，也不让
/// "崩溃恢复有据可查"在溢出时悄悄失效（与 F3401 账本同一纪律）。
pub const JOURNAL_CAP: usize = 256;

/// 单次预演的差分条目上限。超了说明这次换肤的形状异常，应当拆分而不是硬算。
pub const PREVIEW_DIFF_CAP: usize = 1024;

/// 探针数上限。探针是断言的采样点，不是全表 dump。
pub const MAX_PROBES: usize = 64;

/// 探针数下限。**少于两个探针判不出"半新半旧"**（一个探针只有新或旧两种取值，
/// 混色无从谈起），故 [`FrameProbe::new`] 拒绝单探针。
pub const MIN_PROBES: usize = 2;

/// 主题切换的阶段数（读屏播报"已完成 k/N 阶段"的分母）。
pub const STAGES: u8 = 4;

/// 发布过程观察样本上限。一次发布观察两三个点，但反复发布会累积，上限到了
/// 丢弃新样本并计数（[`Switchboard::probe_dropped`]），不静默截断。
pub const MAX_PROBE_FRAMES: usize = 64;

// ---------------------------------------------------------------------------
// 二、诊断（三要素缺一不可）
// ---------------------------------------------------------------------------

/// 换肤侧诊断码。与 F3402 的 `DiagCode`、F3403 的 `CascadeCode` 是**三套**：
/// 各自模块自持，不共用枚举，免得一处加码牵连另两处的穷举匹配。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchCode {
    /// 诊断三要素里有空段（构造期拦截）。
    IncompleteDiag,
    /// 尚无生效主题（首启之前）。
    NoLiveTheme,
    /// 已有事务在途（换肤互斥）。
    TxnBusy,
    /// 事务不在可操作状态。
    TxnNotOpen,
    /// 主题标识非法（空或超长）。
    ThemeIdInvalid,
    /// 表未建全（未排序、有重复、缺值或值超长）——**拒发**。
    SkinIncomplete,
    /// 表条数超上限。
    SkinTooLarge,
    /// 探针集非法（越界、重复、过少、过多，或**零个会变探针**）。
    ProbeInvalid,
    /// 事务悬空（已被兜底回滚）。
    Dangling,
    /// 账本已满。
    JournalFull,
    /// 账本结构损坏。
    JournalCorrupt,
    /// 上游（解析/级联）拒绝，本模块不代为解释为"切换成功"。
    UpstreamRejected,
    /// 时钟回拨（注入的 tick 小于已推进到的 tick）。
    ClockRewind,
}

impl SwitchCode {
    /// 稳定短码，进诊断台账与读屏播报。**枚举判别值不是线上编码值**——
    /// 自检断言 `wire()` 与判别值解耦。
    pub fn wire(self) -> &'static str {
        match self {
            SwitchCode::IncompleteDiag => "E04_DIAG_INCOMPLETE",
            SwitchCode::NoLiveTheme => "E04_NO_LIVE_THEME",
            SwitchCode::TxnBusy => "E04_TXN_BUSY",
            SwitchCode::TxnNotOpen => "E04_TXN_NOT_OPEN",
            SwitchCode::ThemeIdInvalid => "E04_THEME_ID_INVALID",
            SwitchCode::SkinIncomplete => "E04_SKIN_INCOMPLETE",
            SwitchCode::SkinTooLarge => "E04_SKIN_TOO_LARGE",
            SwitchCode::ProbeInvalid => "E04_PROBE_INVALID",
            SwitchCode::Dangling => "E04_TXN_DANGLING",
            SwitchCode::JournalFull => "E04_JOURNAL_FULL",
            SwitchCode::JournalCorrupt => "E04_JOURNAL_CORRUPT",
            SwitchCode::UpstreamRejected => "E04_UPSTREAM_REJECTED",
            SwitchCode::ClockRewind => "E04_CLOCK_REWIND",
        }
    }

    /// 该码是否代表**阻断**。
    ///
    /// 唯一非阻断的是 [`SwitchCode::Dangling`]：悬空不是"拒绝"，是**兜底处置**
    /// ——它已经触发了一次回滚。若把它算作阻断，读者会以为"什么都没发生"，
    /// 而实际上生效面已经被人改回旧主题了。
    pub fn blocking(self) -> bool {
        !matches!(self, SwitchCode::Dangling)
    }

    /// 该码是否为**兜底类**（已触发回滚，需在台账上单独计数）。
    pub fn is_fallback(self) -> bool {
        matches!(self, SwitchCode::Dangling)
    }
}

/// 换肤侧诊断。**三要素缺一不可**——"报错但没给出路"比不报更坏。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchDiag {
    /// 诊断码。
    pub code: SwitchCode,
    /// 位置（读屏第一句就念它）。
    pub site: Site,
    /// 现象：发生了什么。
    pub what: String,
    /// 原因：为什么发生。
    pub why: String,
    /// 处置：读者下一步该做什么。
    pub fix: String,
}

impl SwitchDiag {
    /// 构造诊断。**任一要素为空则降级为 [`SwitchCode::IncompleteDiag`]**。
    pub fn new(
        code: SwitchCode,
        site: Site,
        what: &str,
        why: &str,
        fix: &str,
    ) -> SwitchDiag {
        if what.is_empty() || why.is_empty() || fix.is_empty() {
            let which = if what.is_empty() {
                "现象段为空：调用方没说明发生了什么"
            } else if why.is_empty() {
                "原因段为空：调用方没说明为什么发生"
            } else {
                "处置段为空：调用方没说明读者下一步该做什么"
            };
            return SwitchDiag {
                code: SwitchCode::IncompleteDiag,
                site,
                what: "诊断三要素不完整".to_string(),
                why: which.to_string(),
                fix: "补齐缺失的那一段后重试；换肤事务不接受残缺诊断".to_string(),
            };
        }
        SwitchDiag {
            code,
            site,
            what: what.to_string(),
            why: why.to_string(),
            fix: fix.to_string(),
        }
    }

    /// 三要素是否齐备。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.fix.is_empty()
    }

    /// 读屏播报文本：**先位置、再现象、后原因与处置**，字段顺序即朗读顺序。
    pub fn spoken(&self) -> String {
        format!(
            "{}第 {} 行第 {} 列（字节 {}）：{}。原因：{}。处置：{}",
            self.code.wire(),
            self.site.line,
            self.site.col,
            self.site.byte,
            self.what,
            self.why,
            self.fix
        )
    }
}

/// 上游级联诊断 → 换肤诊断。**三要素由上游原文承接**，不另编一套说法——
/// 换肤层没参与那次求值，没资格替它解释。
pub fn from_upstream(site: Site, d: &super::ver01c_cascade::CascadeDiag) -> SwitchDiag {
    SwitchDiag::new(
        SwitchCode::UpstreamRejected,
        site,
        &format!("预演被上游级联拒绝：{}", d.what),
        &format!("上游原因：{}", d.why),
        &format!("按上游处置执行后再重试换肤：{}", d.fix),
    )
}

// ---------------------------------------------------------------------------
// 三、生效表（一经发布即只读）
// ---------------------------------------------------------------------------

/// 一张已建全的主题值表。**`build` 成功即整表可用**，不存在"建到一半"的状态：
/// 增量建表会让"半新半旧"从一个纪律问题变成一个数据结构问题。
///
/// `values` 恒按路径**字节序升序**且路径唯一，故查值走二分（[`Self::get`]）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SkinTable {
    /// 主题标识。
    pub theme: String,
    /// 发布世代。`0` = 未发布（只在暂存区里待着）。
    pub epoch: u64,
    /// 令牌值表，按路径字节序升序、路径唯一。
    pub values: Vec<(String, String)>,
}

impl SkinTable {
    /// 建表：校验 → 排序 → 查重 → 定长闸。**建表失败就没有半成品**。
    ///
    /// 校验项（任一不过即 [`SwitchCode::SkinIncomplete`] / [`SwitchCode::SkinTooLarge`]）：
    /// 条数上限、主题 id 非空且不超长、值非空且不超长、路径非空、路径无重复。
    pub fn build(theme: &str, mut pairs: Vec<(String, String)>) -> Result<SkinTable, SwitchDiag> {
        check_theme_id(theme)?;
        if pairs.len() > MAX_SKIN_VALUES {
            return Err(SwitchDiag::new(
                SwitchCode::SkinTooLarge,
                Site::start(),
                &format!(
                    "主题 {:?} 有 {} 条令牌，超过上限 {}",
                    theme,
                    pairs.len(),
                    MAX_SKIN_VALUES
                ),
                "换肤表的容量必须与级联图的节点上限一致，否则两级闸互相打脸",
                &format!(
                    "把令牌集拆成主题包分批加载，或把上限从 {} 调到至少 {}",
                    MAX_SKIN_VALUES,
                    pairs.len() + 1
                ),
            ));
        }
        for (i, (path, value)) in pairs.iter().enumerate() {
            if path.is_empty() {
                return Err(SwitchDiag::new(
                    SwitchCode::SkinIncomplete,
                    Site::start(),
                    &format!("主题 {:?} 的第 {} 条令牌路径为空", theme, i),
                    "空路径无法被界面引用，也无法被探针采样，等于一条不可用的记录",
                    "给这条令牌一个点分路径；令牌路径不可为空",
                ));
            }
            if value.is_empty() {
                return Err(SwitchDiag::new(
                    SwitchCode::SkinIncomplete,
                    Site::start(),
                    &format!("主题 {:?} 的令牌 {:?} 求值为空串", theme, path),
                    "空串是求值失败的样子，不是合法的令牌值；把它发出去等于把半新半旧写进界面",
                    "修好该令牌的引用链；求值失败应当走上游的降级兜底而不是产出空串",
                ));
            }
            if value.len() > MAX_VALUE_BYTES {
                return Err(SwitchDiag::new(
                    SwitchCode::SkinIncomplete,
                    Site::start(),
                    &format!(
                        "主题 {:?} 的令牌 {:?} 值长 {} 字节，超过上限 {}",
                        theme,
                        path,
                        value.len(),
                        MAX_VALUE_BYTES
                    ),
                    "嵌引用会指数膨胀，没有闸就是内存耗尽",
                    "把引用拆成多级中间令牌",
                ));
            }
        }
        pairs.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        for i in 1..pairs.len() {
            if pairs[i].0 == pairs[i - 1].0 {
                return Err(SwitchDiag::new(
                    SwitchCode::SkinIncomplete,
                    Site::start(),
                    &format!(
                        "主题 {:?} 的令牌路径 {:?} 出现两次",
                        theme, pairs[i].0
                    ),
                    "同路径两条值，发出去时界面读到哪一条取决于排序实现，不是取决于作者意图",
                    "合并成一条；主题表内路径必须唯一",
                ));
            }
        }
        Ok(SkinTable {
            theme: theme.to_string(),
            epoch: 0,
            values: pairs,
        })
    }

    /// 从已求值的级联引擎取表（**预演路径**：新主题先求值再决定要不要换）。
    pub fn from_engine(theme: &str, engine: &CascadeEngine) -> Result<SkinTable, SwitchDiag> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for n in 0..engine.graph.nodes {
            let path = match engine.graph.path_of(n as u32) {
                Some(p) => p.to_string(),
                None => {
                    return Err(SwitchDiag::new(
                        SwitchCode::UpstreamRejected,
                        Site::start(),
                        &format!("预演取表时节点号 {} 在依赖图里没有路径", n),
                        "图与令牌表不同源，说明这份引擎不是与本次预演配套构建的",
                        "从源文件重走一遍解析与建图",
                    ));
                }
            };
            let value = match engine.values.get(n) {
                Some(v) => v.clone(),
                None => {
                    return Err(SwitchDiag::new(
                        SwitchCode::UpstreamRejected,
                        Site::start(),
                        &format!("预演取表时节点 {:?} 没有求值结果", path),
                        "级联引擎交出的求值数组与图不同长度",
                        "重建引擎后再预演",
                    ));
                }
            };
            pairs.push((path, value));
        }
        SkinTable::build(theme, pairs)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 按路径取值（二分，保持 `values` 的字节序不变式）。
    pub fn get(&self, path: &str) -> Option<&str> {
        self.values
            .binary_search_by(|e| e.0.as_bytes().cmp(path.as_bytes()))
            .ok()
            .map(|i| self.values[i].1.as_str())
    }

    /// 结构完整性自检（**逐项复算，不"自己说自己对"**）。
    ///
    /// 查四样：有序性、唯一性、值长、路径非空。这四样各自都能被独立伪造，
    /// 逐项查才有意义；只查"有序"则一张两行的重复表能过。
    pub fn verify(&self) -> Result<(), SwitchDiag> {
        check_theme_id(&self.theme)?;
        if self.values.len() > MAX_SKIN_VALUES {
            return Err(SwitchDiag::new(
                SwitchCode::SkinTooLarge,
                Site::start(),
                &format!(
                    "主题 {:?} 的表有 {} 条，超过上限 {}",
                    self.theme,
                    self.values.len(),
                    MAX_SKIN_VALUES
                ),
                "容量承诺失效",
                "拆包或调高上限",
            ));
        }
        for i in 0..self.values.len() {
            let (path, value) = &self.values[i];
            if path.is_empty() || value.is_empty() || value.len() > MAX_VALUE_BYTES {
                return Err(SwitchDiag::new(
                    SwitchCode::SkinIncomplete,
                    Site::start(),
                    &format!("主题 {:?} 的第 {} 条记录不完整", self.theme, i),
                    "生效表里出现了空路径、空值或超长值",
                    "重建这张表；未建全的表不得发布",
                ));
            }
            if i > 0 {
                let prev = &self.values[i - 1].0;
                if path.as_bytes() <= prev.as_bytes() {
                    let why = if path == prev {
                        "路径重复，二分查找的结果取决于命中哪一条"
                    } else {
                        "字节序不是升序，二分查找会漏掉本该命中的路径"
                    };
                    return Err(SwitchDiag::new(
                        SwitchCode::SkinIncomplete,
                        Site::start(),
                        &format!(
                            "主题 {:?} 的第 {} 条记录 {:?} 未严格大于前一条 {:?}",
                            self.theme, i, path, prev
                        ),
                        why,
                        "按 [`SkinTable::build`] 重建：它会排序并拒绝重复",
                    ));
                }
            }
        }
        Ok(())
    }

    /// 两表形状是否一致（同条数、同路径序）。差分的前提。
    pub fn same_shape(&self, other: &SkinTable) -> bool {
        if self.values.len() != other.values.len() {
            return false;
        }
        self.values
            .iter()
            .zip(other.values.iter())
            .all(|(a, b)| a.0 == b.0)
    }
}

/// 主题标识闸（空 / 超长）。
fn check_theme_id(theme: &str) -> Result<(), SwitchDiag> {
    if theme.trim().is_empty() {
        return Err(SwitchDiag::new(
            SwitchCode::ThemeIdInvalid,
            Site::start(),
            "主题标识为空",
            "空标识会让两套不同的值表在账本与快照里长得一样，回滚时无从分辨",
            "给主题一个非空标识（短名即可，不是正文）",
        ));
    }
    if theme.len() > MAX_THEME_ID_LEN {
        return Err(SwitchDiag::new(
            SwitchCode::ThemeIdInvalid,
            Site::start(),
            &format!(
                "主题标识长 {} 字节，超过上限 {}",
                theme.len(),
                MAX_THEME_ID_LEN
            ),
            "标识超长时截断会让两个不同主题同名，那比直接拒绝更难排查",
            &format!(
                "把标识压到 {} 字节以内；标识是短名不是正文",
                MAX_THEME_ID_LEN
            ),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、帧探针与半新半旧判定（截图一致性断言）
// ---------------------------------------------------------------------------

/// 帧探针：一帧里被采样的令牌路径集合。**"一帧"就是这个集合上的取值向量**，
/// 不引入真实像素——像素是 E02 的事，本单只保证"读到的主题是一致的"。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrameProbe {
    /// 探针路径，按字节序升序。
    pub paths: Vec<String>,
}

impl FrameProbe {
    /// 建探针。**闸门在此**（[`SwitchCode::ProbeInvalid`]）：
    /// - 探针数须在 `[MIN_PROBES, MAX_PROBES]`；
    /// - 每条路径非空、无重复；
    /// - 每条路径必须**存在于待比较的两张表里**（否则采不到值，判定无从谈起）；
    /// - 与 `old` 比较时**至少要有一个会变的探针**（`old[p] != new[p]`）——
    ///   全不变时判定恒为 `AllOld`，断言就成了测不到任何东西的空断言。
    pub fn new(paths: &[&str], old: &SkinTable, new: &SkinTable) -> Result<FrameProbe, SwitchDiag> {
        if paths.len() < MIN_PROBES || paths.len() > MAX_PROBES {
            return Err(SwitchDiag::new(
                SwitchCode::ProbeInvalid,
                Site::start(),
                &format!(
                    "探针数 {} 越界，应在 [{}, {}]",
                    paths.len(),
                    MIN_PROBES,
                    MAX_PROBES
                ),
                "一个探针判不出半新半旧（它只有新或旧两种取值），过密的探针则是全表 dump 而非采样",
                &format!("把探针数调到 {} 到 {} 之间", MIN_PROBES, MAX_PROBES),
            ));
        }
        let mut sorted: Vec<String> = Vec::with_capacity(paths.len());
        for p in paths {
            if p.is_empty() {
                return Err(SwitchDiag::new(
                    SwitchCode::ProbeInvalid,
                    Site::start(),
                    "探针路径为空",
                    "空路径在表里取不到值，采样点落空会让这一路判定永远走不到",
                    "换成一个真实存在的令牌路径",
                ));
            }
            if old.get(p).is_none() || new.get(p).is_none() {
                return Err(SwitchDiag::new(
                    SwitchCode::ProbeInvalid,
                    Site::start(),
                    &format!("探针路径 {:?} 不同时存在于新旧两张表", p),
                    "采样点落空时判定无从谈起；若两表形状本就不同，那是换肤的形状问题而不是探针问题",
                    "改用两表共有的路径；换肤前先确认两表形状一致",
                ));
            }
            sorted.push(p.to_string());
        }
        sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        for i in 1..sorted.len() {
            if sorted[i] == sorted[i - 1] {
                return Err(SwitchDiag::new(
                    SwitchCode::ProbeInvalid,
                    Site::start(),
                    &format!("探针路径 {:?} 重复", sorted[i]),
                    "重复探针会把同一个采样点计两次，混色判定被按权重放大",
                    "去重；每个探针路径只出现一次",
                ));
            }
        }
        let changing = sorted
            .iter()
            .filter(|p| old.get(p.as_str()) != new.get(p.as_str()))
            .count();
        if changing == 0 {
            return Err(SwitchDiag::new(
                SwitchCode::ProbeInvalid,
                Site::start(),
                &format!("这 {} 条探针路径在新旧两表里取值全相同", sorted.len()),
                "没有会变的探针，混色判定恒为全旧——断言在跑但测不到任何东西（采样留洞）",
                "至少选一条在新主题里取值不同的路径作探针",
            ));
        }
        Ok(FrameProbe { paths: sorted })
    }

    /// 会变的探针数（判定实际依据的采样点数）。
    pub fn changing_count(&self, old: &SkinTable, new: &SkinTable) -> usize {
        self.paths
            .iter()
            .filter(|p| old.get(p.as_str()) != new.get(p.as_str()))
            .count()
    }

    /// 在给定表上取一帧。
    pub fn sample(&self, table: &SkinTable) -> Result<FrameSample, SwitchDiag> {
        let mut seen: Vec<(String, String)> = Vec::with_capacity(self.paths.len());
        for p in self.paths.iter() {
            match table.get(p.as_str()) {
                Some(v) => seen.push((p.clone(), v.to_string())),
                None => {
                    return Err(SwitchDiag::new(
                        SwitchCode::ProbeInvalid,
                        Site::start(),
                        &format!("对主题 {:?} 采样时探针 {:?} 取不到值", table.theme, p),
                        "探针在建表时校验过存在，这里取不到说明表被换掉了或被改坏了",
                        "重新建表并重建探针",
                    ));
                }
            }
        }
        Ok(FrameSample {
            theme: table.theme.clone(),
            epoch: table.epoch,
            seen,
        })
    }
}

/// 一帧的采样结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrameSample {
    /// 取样所依据的表的主题标识。
    pub theme: String,
    /// 取样所依据的表的世代。
    pub epoch: u64,
    /// 采样值（路径 → 值），与探针同序。
    pub seen: Vec<(String, String)>,
}

/// 半新半旧判定结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MixedVerdict {
    /// 每个会变的探针都取到旧值 —— 整帧旧。
    AllOld,
    /// 每个会变的探针都取到新值 —— 整帧新。
    AllNew,
    /// 有的探针新、有的探针旧 —— **半新半旧**。
    ///
    /// **两侧清单分开存**（不拼成一条）：拼接后"清单非空"这条弱门禁就看不出
    /// 分布对不对——把新侧只记一条、或把同一条记两遍，拼出来的清单照样非空。
    /// 分开存才能断"旧侧恰好 2 条、新侧恰好 1 条"这种**精确到数**的判据。
    Mixed {
        /// 走旧侧的探针路径。
        stale: Vec<String>,
        /// 走新侧的探针路径。
        fresh: Vec<String>,
    },
    /// 某个探针取到的值既不是旧值也不是新值 —— 脏数据（附路径清单）。
    Foreign(Vec<String>),
}

impl MixedVerdict {
    /// 是否为"出现了半新半旧"。
    pub fn is_mixed(&self) -> bool {
        matches!(self, MixedVerdict::Mixed { .. })
    }

    /// 是否为"读到了不属于任何一张表的值"。
    pub fn is_foreign(&self) -> bool {
        matches!(self, MixedVerdict::Foreign(_))
    }

    /// 是否整帧一致（要么全旧要么全新，且没有脏数据）。
    pub fn is_coherent(&self) -> bool {
        matches!(self, MixedVerdict::AllOld | MixedVerdict::AllNew)
    }

    /// 问题路径数（混色为两侧合计；一致时为 0）。
    pub fn offender_count(&self) -> usize {
        match self {
            MixedVerdict::Mixed { stale, fresh } => stale.len() + fresh.len(),
            MixedVerdict::Foreign(v) => v.len(),
            _ => 0,
        }
    }

    /// 走旧侧的探针数（只有混色时非零）。
    pub fn stale_count(&self) -> usize {
        match self {
            MixedVerdict::Mixed { stale, .. } => stale.len(),
            _ => 0,
        }
    }

    /// 走新侧的探针数（只有混色时非零）。
    pub fn fresh_count(&self) -> usize {
        match self {
            MixedVerdict::Mixed { fresh, .. } => fresh.len(),
            _ => 0,
        }
    }

    /// 短标签，进台账。
    pub fn label(&self) -> &'static str {
        match self {
            MixedVerdict::AllOld => "全旧",
            MixedVerdict::AllNew => "全新",
            MixedVerdict::Mixed { .. } => "半新半旧",
            MixedVerdict::Foreign(_) => "脏数据",
        }
    }
}

impl FrameSample {
    /// 本帧相对 `old` / `new` 的判定。
    ///
    /// **只看会变的探针**：不变探针两边同值，算进来只会稀释判定、让混色看不出来。
    /// 判定走**三档**而不是两档：`Foreign`（既非旧也非新）与 `Mixed`（有新有旧）
    /// 是两种不同的坏——后者是换肤写坏了表，前者是读到了表之外的东西，处置不同。
    pub fn mixed_with(&self, old: &SkinTable, new: &SkinTable) -> MixedVerdict {
        let mut stale: Vec<String> = Vec::new();
        let mut fresh: Vec<String> = Vec::new();
        let mut alien: Vec<String> = Vec::new();
        for (path, value) in self.seen.iter() {
            let o = old.get(path.as_str());
            let n = new.get(path.as_str());
            match (o, n) {
                (Some(o), Some(n)) if o == n => {
                    // 不变探针：两表同值，采到谁都一致，不参与判定。
                }
                (Some(o), Some(_)) => {
                    if value == o {
                        stale.push(path.clone());
                    } else if value == n.unwrap_or("") {
                        fresh.push(path.clone());
                    } else {
                        alien.push(path.clone());
                    }
                }
                _ => {
                    alien.push(path.clone());
                }
            }
        }
        if !alien.is_empty() {
            return MixedVerdict::Foreign(alien);
        }
        if !stale.is_empty() && !fresh.is_empty() {
            return MixedVerdict::Mixed { stale, fresh };
        }
        if !fresh.is_empty() {
            MixedVerdict::AllNew
        } else {
            MixedVerdict::AllOld
        }
    }

    /// 读屏单行（不含令牌值正文——值可能含用户自定义字符串）。
    pub fn spoken(&self) -> String {
        format!(
            "一帧取自主题 {}（世代 {}），共采样 {} 个探针点",
            self.theme,
            self.epoch,
            self.seen.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 五、快照（值拷贝，不是引用）
// ---------------------------------------------------------------------------

/// 旧主题快照。**值拷贝**：回滚必须能在原表已被覆盖之后仍然还原，
/// 留个引用等于把"能不能回滚"押在"那张表别被改"上——而那正是崩溃现场。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 被快照的主题标识。
    pub theme: String,
    /// 被快照时的世代。
    pub epoch: u64,
    /// 取快照时的逻辑 tick。
    pub tick: u64,
    /// 令牌值表（与 [`SkinTable::values`] 同序的值拷贝）。
    pub values: Vec<(String, String)>,
}

impl Snapshot {
    /// 取快照。
    pub fn capture(table: &SkinTable, tick: u64) -> Snapshot {
        Snapshot {
            theme: table.theme.clone(),
            epoch: table.epoch,
            tick,
            values: table.values.clone(),
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 还原成一张可发布的整表（回滚路径必经）。
    pub fn restore(&self) -> Result<SkinTable, SwitchDiag> {
        SkinTable::build(&self.theme, self.values.clone())
    }

    /// 读屏单行。
    pub fn spoken(&self) -> String {
        format!(
            "快照取自主题 {}（世代 {}），共 {} 条令牌，tick {}",
            self.theme, self.epoch, self.values.len(), self.tick
        )
    }
}

// ---------------------------------------------------------------------------
// 六、账本（只增不改，崩溃恢复的仲裁者）
// ---------------------------------------------------------------------------

/// 事务状态。**账本只记这些状态，不记"生效面现状"**——现状是崩溃现场，不能当证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JournalState {
    /// 无事务。
    Idle,
    /// 已预演，暂存表就绪。
    Previewed,
    /// 已确认暂存（可提交）。
    Staged,
    /// 提交途中（**崩溃恢复只看这个**）。
    Committing,
    /// 已提交。
    Committed,
    /// 已回滚。
    RolledBack,
    /// 已取消（预演判否）。
    Aborted,
    /// 悬空兜底回滚。
    DanglingRolledBack,
}

impl JournalState {
    /// 中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            JournalState::Idle => "空闲",
            JournalState::Previewed => "已预演",
            JournalState::Staged => "已确认暂存",
            JournalState::Committing => "提交中",
            JournalState::Committed => "已提交",
            JournalState::RolledBack => "已回滚",
            JournalState::Aborted => "已取消",
            JournalState::DanglingRolledBack => "悬空兜底回滚",
        }
    }

    /// 该状态是否为**在途**（可被兜底回滚，也可被崩溃恢复接手）。
    pub fn in_flight(self) -> bool {
        matches!(
            self,
            JournalState::Previewed | JournalState::Staged | JournalState::Committing
        )
    }

    /// 该状态是否为**终局**（不可再推进）。
    pub fn terminal(self) -> bool {
        !matches!(self, JournalState::Idle) && !self.in_flight()
    }

    /// 短码，进诊断台账。
    pub fn code(self) -> &'static str {
        match self {
            JournalState::Idle => "IDLE",
            JournalState::Previewed => "PREVIEWED",
            JournalState::Staged => "STAGED",
            JournalState::Committing => "COMMITTING",
            JournalState::Committed => "COMMITTED",
            JournalState::RolledBack => "ROLLED_BACK",
            JournalState::Aborted => "ABORTED",
            JournalState::DanglingRolledBack => "DANGLING_ROLLED_BACK",
        }
    }
}

/// 账本的一条。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JournalEntry {
    /// 落账序号（**严格单调递增**，账本唯一的主键）。
    ///
    /// **事务号与落账序号是两回事**：一个事务要落多条账（预演 → 暂存 → 提交中 →
    /// 终局），它们**共用同一个事务号**，所以主键不能是事务号——否则账本会因
    /// "事务号重复"而自我判损坏。故主键取落账序号，事务号另设**不下降**的不变式
    /// （同一事务的多条账相等，不同事务必须递增）。
    pub seq: u64,
    /// 事务号（同一事务的多条账共用；不同事务严格递增）。
    pub txn: u64,
    /// 状态。
    pub state: JournalState,
    /// 目标主题标识。
    pub theme: String,
    /// 事务打开时的逻辑 tick。
    pub tick: u64,
    /// 提交前的世代。
    pub from_epoch: u64,
    /// 事务携带的回滚快照（**恢复的唯一凭据**）。
    pub snapshot: Snapshot,
}

/// 只增不改的换肤账本。满则 [`SwitchCode::JournalFull`] 拒绝——让"崩溃恢复有据"
/// 在溢出时悄悄失效，比拒绝更坏。
#[derive(Clone, Debug, Default)]
pub struct Journal {
    entries: Vec<JournalEntry>,
    next_seq: u64,
    dropped: u64,
    rejected: u64,
}

impl Journal {
    /// 空账本（`next_seq` 从 1 起，0 留作"无落账"哨兵）。
    pub fn new() -> Self {
        Journal {
            entries: Vec::new(),
            next_seq: 1,
            dropped: 0,
            rejected: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 因满而**未落账**的条数（显性化，不静默丢）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 因内容非法而**被拒**的条数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 只读遍历。
    pub fn entries(&self) -> &[JournalEntry] {
        self.entries.as_slice()
    }

    /// 按事务号查**最后一条**该事务的账（同事务多阶段，取最新的那条）。
    pub fn entry(&self, txn: u64) -> Option<&JournalEntry> {
        self.entries.iter().rev().find(|e| e.txn == txn)
    }

    /// 最后一条。
    pub fn last(&self) -> Option<&JournalEntry> {
        self.entries.last()
    }

    /// 追加一条。**账满即拒**（[`SwitchCode::JournalFull`]），不覆盖旧条。
    ///
    /// 落账序号由账本自己分配（[`JournalEntry::seq`] 传 0 即可）——序号是账本的
    /// 主键，让调用方填就等于让调用方能造出倒退的序号。
    pub fn append(&mut self, mut e: JournalEntry) -> Result<(), SwitchDiag> {
        if e.theme.trim().is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SwitchDiag::new(
                SwitchCode::JournalCorrupt,
                Site::start(),
                "账本拒收：条目缺主题标识",
                "没有主题标识的条目在崩溃恢复时无法分辨该回滚到哪一套值",
                "补上主题标识后重试",
            ));
        }
        if self.entries.len() >= JOURNAL_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(SwitchDiag::new(
                SwitchCode::JournalFull,
                Site::start(),
                &format!("换肤账本已满（{} 条），本条未落账", self.entries.len()),
                "账满后若继续覆盖旧条，崩溃恢复就只能凭现状猜，那等于没有恢复",
                &format!(
                    "先归档旧账本（保留终局条目）再换肤，或把上限从 {} 调高",
                    JOURNAL_CAP
                ),
            ));
        }
        e.seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(e);
        Ok(())
    }

/// 结构自检（**两条不变式分开查**）：
/// - 落账序号**严格递增**（账本主键，不得倒退）；
/// - 事务号**不下降**（同一事务可多条账，故允许相等；不同事务必须严格递增，
///   否则崩溃恢复会挑错快照）。
///
/// 合成一条查会漏：只查事务号严格递增，则任何多阶段事务都自我判损坏。
pub fn verify(&self) -> Result<(), SwitchDiag> {
        for i in 1..self.entries.len() {
            if self.entries[i].seq <= self.entries[i - 1].seq {
                return Err(SwitchDiag::new(
                    SwitchCode::JournalCorrupt,
                    Site::start(),
                    &format!(
                        "账本第 {} 条的落账序号 {} 未大于前一条 {}",
                        i,
                        self.entries[i].seq,
                        self.entries[i - 1].seq
                    ),
                    "落账序号是账本主键；倒退会让末条失去意义，崩溃恢复将读到过期条目",
                    "重建账本；落账序号必须由单调计数器产生",
                ));
            }
            if self.entries[i].txn < self.entries[i - 1].txn {
                return Err(SwitchDiag::new(
                    SwitchCode::JournalCorrupt,
                    Site::start(),
                    &format!(
                        "账本第 {} 条的事务号 {} 小于前一条 {}",
                        i,
                        self.entries[i].txn,
                        self.entries[i - 1].txn
                    ),
                    "事务号乱序会让崩溃恢复挑错快照，回滚到不相干的主题",
                    "重建账本；事务号必须由单调计数器产生",
                ));
            }
        }
        Ok(())
    }

    /// 崩溃恢复的仲裁：**每个事务的最新一条里，取事务号最大的在途条目**。
    ///
    /// **必须逐事务取最新，不能从整本账里挑任意在途条目**：账本是"一个事务多
    /// 条账"（预演 → 暂存 → 提交中 → 终局），早期那条 `Previewed` 在该事务
    /// 已落终局之后**仍然在账里**。若那样挑，恢复过一次之后再扫就会把同一条
    /// （或更早那条）重新接手——表现为"撤销了用户的正常操作"。
    ///
    /// 取"最新"而非"末条"：末条是全局最后一条，恰好没有在途事务时它一定不是
    /// 在途态，找不到待办；但若某个老事务停在在途、而新事务已终局，末条判空就
    /// 漏了。逐事务取最新则两者都对。
    pub fn pending(&self) -> Option<&JournalEntry> {
        // 每个事务的最新条目（落账序号最大者）。
        let mut latest: Vec<&JournalEntry> = Vec::new();
        for e in self.entries.iter() {
            match latest.iter_mut().find(|x| x.txn == e.txn) {
                Some(slot) => {
                    if e.seq > slot.seq {
                        *slot = e;
                    }
                }
                None => latest.push(e),
            }
        }
        latest
            .into_iter()
            .filter(|e| e.state.in_flight())
            .max_by_key(|e| e.txn)
    }
}

// ---------------------------------------------------------------------------
// 七、预演与报告
// ---------------------------------------------------------------------------

/// 预演结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PreviewVerdict {
    /// 预演通过（可以继续走到提交）。
    Clean,
    /// 预演判否（必须取消切换）。
    Rejected(SwitchCode),
}

/// 预演报告（**只读产物**：报告生成过程不写生效面一个字节）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PreviewReport {
    /// 事务号。
    pub txn: u64,
    /// 原主题标识。
    pub from_theme: String,
    /// 目标主题标识。
    pub to_theme: String,
    /// 值会变的路径（字节序升序）。
    pub changed: Vec<String>,
    /// 新表新增的路径。
    pub added: Vec<String>,
    /// 新表缺失的路径。
    pub removed: Vec<String>,
    /// 结论。
    pub verdict: PreviewVerdict,
}

impl PreviewReport {
    /// 会变的路径数。
    pub fn changed_len(&self) -> usize {
        self.changed.len()
    }

    /// 形态变化总数（新增 + 缺失）。**非零意味着这次换肤会改界面结构**，
    /// 值得在读屏里单独说一句（只改值与增删令牌对用户不是一回事）。
    pub fn shape_delta(&self) -> usize {
        self.added.len() + self.removed.len()
    }

    /// 是否通过。
    pub fn clean(&self) -> bool {
        matches!(self.verdict, PreviewVerdict::Clean)
    }

    /// 读屏播报。
    pub fn spoken(&self) -> String {
        let verdict = match &self.verdict {
            PreviewVerdict::Clean => "通过（可继续切换）",
            PreviewVerdict::Rejected(c) => {
                let _ = c;
                "不通过（须取消切换）"
            }
        };
        format!(
            "预演：从主题 {} 换到 {}，{} 条令牌值会变，{} 条新增，{} 条缺失，结论{}",
            self.from_theme,
            self.to_theme,
            self.changed.len(),
            self.added.len(),
            self.removed.len(),
            verdict
        )
    }
}

/// 换肤事务的终局类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchKind {
    /// 首装（此前没有生效主题）。
    Mount,
    /// 正常提交。
    Commit,
    /// 主动回滚。
    Rollback,
    /// 预演判否后取消。
    Abort,
    /// 悬空兜底回滚。
    DanglingRollback,
    /// 崩溃恢复回滚。
    Recovered,
}

impl SwitchKind {
    /// 中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            SwitchKind::Mount => "首装",
            SwitchKind::Commit => "提交",
            SwitchKind::Rollback => "回滚",
            SwitchKind::Abort => "取消",
            SwitchKind::DanglingRollback => "悬空兜底回滚",
            SwitchKind::Recovered => "崩溃恢复回滚",
        }
    }

    /// 是否为回滚类（台账上要与正常提交分开计数）。
    pub fn is_rollback(self) -> bool {
        matches!(
            self,
            SwitchKind::Rollback
                | SwitchKind::DanglingRollback
                | SwitchKind::Recovered
        )
    }
}

/// 事务终局报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchReport {
    /// 事务号。
    pub txn: u64,
    /// 终局类别。
    pub kind: SwitchKind,
    /// 切换后生效的主题标识。
    pub theme: String,
    /// 切换后的世代。
    pub epoch: u64,
    /// 本次改动的令牌条数。
    pub changed: usize,
}

impl SwitchReport {
    /// 读屏播报。
    pub fn spoken(&self) -> String {
        format!(
            "换肤{}完成：主题 {}，世代 {}，本单改动 {} 条令牌",
            self.kind.zh(),
            self.theme,
            self.epoch,
            self.changed
        )
    }
}

// ---------------------------------------------------------------------------
// 八、事务管理器
// ---------------------------------------------------------------------------

/// **在途事务**（`Previewed` / `Staged` / `Committing`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchTxn {
    /// 事务号。
    pub id: u64,
    /// 目标主题标识。
    pub theme: String,
    /// 打开时的逻辑 tick。
    pub opened_tick: u64,
    /// 回滚凭据（打开事务时立刻取的快照）。
    pub snapshot: Snapshot,
    /// 暂存表（**只在非生效槽里待命**，不参与生效）。
    pub staged: SkinTable,
    /// 预演报告。
    pub preview: PreviewReport,
    /// 当前状态。
    pub state: JournalState,
    /// 已完成阶段数（0..=STAGES，读屏播报用）。
    pub stage: u8,
}

/// **已提交但仍可回滚的事务**（`Committed`）。
///
/// **为什么与在途事务分开存**：提交之后事务就"结束"了，但它携带的快照**仍是
/// 唯一能把界面退回旧主题的凭据**。若提交时把它一并丢弃，"失败回滚到旧主题
/// 快照"就只剩崩溃恢复那一条路——而崩溃恢复处理的是"提交途中崩溃"，不是
/// "提交成功但用户反悔"。两者是不同需求，不能用一条路径顶替。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Undone {
    /// 事务号。
    pub id: u64,
    /// 目标主题标识（提交上去的那一套）。
    pub theme: String,
    /// 提交时的逻辑 tick。
    pub tick: u64,
    /// 回滚凭据：提交前的整表快照。
    pub snapshot: Snapshot,
}

/// **发布观察点**（过程可见性）。
///
/// **为什么需要它**：终态检查抓不到"中间态"。一个就地逐令牌改写的发布器，
/// 跑完之后生效表仍然是一张**完整的新表**——终态完全正确，可它在中途确实
/// 让界面读了半新半旧。所以要判住原子性，就必须能在**发布的中间**取样，
/// 而不能只在发布**之后**取样。
///
/// 回调签名给的是**当前生效槽此刻的内容**（可能被改坏的那一份）。这是"向被测
/// 方问现场"的唯一入口，也是让半新半旧有处可寻的唯一办法。
///
/// **用裸函数指针而不是 `&mut dyn FnMut`**：板子要能 `Clone`（自检里多处复制
/// 板子做隔离场景），而 `&mut dyn` 不可克隆。函数指针可克隆，且它只读现场做
/// 自己的事，样本另存在板子自带的 [`Switchboard::probe_frames`] 里——**不依赖
/// 任何静态存储**（内核是 `no_std`，`thread_local!` 不可用；用静态可变量则要
/// `unsafe`，而内核的纪律是零 `unsafe`）。
pub type PublishProbe = fn(&SkinTable);

/// 换肤事务管理器：双缓冲 + 单指针翻转 + 快照回滚 + 悬空兜底。
///
/// **原子性的结构保证**：`commit` 的全部写操作都落在**非生效槽**上，最后一步
/// 是一次游标赋值。生效槽的表在发布之后不再被任何路径写入
/// （[`Self::verify_atomic`] 把这条当不变式查）。因此"读到半新半旧"没有对应时机。
#[derive(Clone, Debug)]
pub struct Switchboard {
    /// 两个值表槽（互为新旧）。
    slots: [Option<SkinTable>; 2],
    /// 生效槽游标。**这就是那个"单指针"**。
    active: usize,
    /// 当前世代。每次发布 +1。
    epoch: u64,
    /// 在途事务（换肤互斥：至多一个）。
    txn: Option<SwitchTxn>,
    /// 已提交但仍可回滚的事务（**至多一个**：再提交一次就把更早的那次挤掉，
    /// 界面只可能退回上一次切换之前，不做多级撤销栈）。
    undone: Option<Undone>,
    /// 发布过程观察点（`None` = 不观察，正常路径不付这个成本）。
    /// **仅供自检注入**：见 [`PublishProbe`] 的说明。
    pub probe: Option<PublishProbe>,
    /// 观察期间抓到的生效槽现场（**每次 [`Self::observe`] 追加一份**）。
    ///
    /// **有上限**：发布一次只观察两三个点，但反复发布会让它无限长。上限到了
    /// 就**丢弃新样本并计数**（[`Self::probe_dropped`]），不静默截断——让样本数
    /// 与实际观察次数对不上，比少抓几帧更坏。
    pub probe_frames: Vec<SkinTable>,
    /// 因样本上限而丢弃的观察次数。
    pub probe_dropped: usize,
    /// 账本。
    pub journal: Journal,
    /// 最近一次提交的见证对（旧表 + 新表）——截图断言要对它判。
    witness: Option<(SkinTable, SkinTable)>,
    /// 下一个事务号。
    next_txn: u64,
    /// 已推进到的逻辑 tick。
    clock: u64,
    /// 提交次数。
    pub commits: u64,
    /// 回滚次数（含兜底与恢复）。
    pub rollbacks: u64,
    /// 悬空兜底次数。
    pub dangling_rollbacks: u64,
    /// 因时钟回拨被拒的推进次数。
    pub clock_rewinds: u64,
    /// 账本拒收次数。
    pub journal_rejects: u64,
    /// 预演判否次数。
    pub previews_rejected: u64,
    /// 最近一次差分**真实走过的归并步数**（比较次数）。
    ///
    /// 供判据与独立解析式对账。这是工作量计数，不是"n × 常数"的算术——
    /// 判据侧另有独立重算的参考值，两者必须相等。
    pub last_diff_steps: usize,
    /// `preview` 观察窗：被 [`Self::preview`] 记下的事务号。
    ///
    /// **本字段只应由只读的 [`Self::preview`] 写入，且写入即缺陷**：预演是干跑，
    /// 干跑不得留下任何痕迹可供后续路径观察到。留这个字段是为了让"预演到底
    /// 有没有副作用"**可被外部检查**——判据侧读它就能断"预演后除账本与生效面
    /// 外，别的状态也没被碰过"，而不是靠"调用没报错"推断没副作用。
    /// 恒为 `None` 是唯一合法取值。
    pub preview_side_effect: core::cell::Cell<Option<u64>>,
}

impl Switchboard {
    /// 新建（**无生效主题**：首装之前 [`Self::active`] 为 `None`）。
    pub fn new() -> Self {
        Switchboard {
            slots: [None, None],
            active: 0,
            epoch: 0,
            txn: None,
            undone: None,
            probe: None,
            probe_frames: Vec::new(),
            probe_dropped: 0,
            journal: Journal::new(),
            witness: None,
            next_txn: 1,
            clock: 0,
            commits: 0,
            rollbacks: 0,
            dangling_rollbacks: 0,
            clock_rewinds: 0,
            journal_rejects: 0,
            previews_rejected: 0,
            last_diff_steps: 0,
            preview_side_effect: core::cell::Cell::new(None),
        }
    }

    /// 换肤引擎版本。
    pub fn version(&self) -> &'static str {
        SWITCH_VERSION
    }

    /// **仅供自检注入损坏**：把某张表直接塞进生效槽，用于验证
    /// [`Self::verify_atomic`] 能不能抓到"生效面是两表拼接"。
    /// 正常使用路径不该拿到它，故名字里带 `for_test` 明示。
    ///
    /// **保持世代不变**（沿用当前生效表的 `epoch`）：否则注入的表会因为世代
    /// 对不上而**先被世代检查拦下**，判据就变成在验"世代检查"而不是"拼接
    /// 检测"——缺陷类型错位，判据就白写了。要造世代不符的表用
    /// [`Self::slots_mut_for_test`]。
    pub fn force_active_for_test(&mut self, mut table: SkinTable) {
        table.epoch = self.epoch;
        self.slots[self.active] = Some(table);
    }

    /// 当前生效槽游标（**仅供自检**）。
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// 生效槽的可变引用（**仅供自检注入任意损坏**，包括世代不符的表）。
    ///
    /// 与 [`Self::force_active_for_test`] 的区别：那个入口替你保持世代一致，
    /// 这个不干预——需要构造"世代与表不同源"这类损坏时用它。
    pub fn slots_mut_for_test(&mut self, idx: usize) -> &mut Option<SkinTable> {
        &mut self.slots[idx]
    }

    /// 当前生效表。
    pub fn active(&self) -> Option<&SkinTable> {
        self.slots[self.active].as_ref()
    }

    /// 当前生效主题标识。
    pub fn theme(&self) -> Option<&str> {
        self.active().map(|t| t.theme.as_str())
    }

    /// 当前世代（尚无生效主题时为 0）。
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 令牌条数（尚无生效主题时为 0）。
    pub fn token_count(&self) -> usize {
        self.active().map(|t| t.len()).unwrap_or(0)
    }

    /// 按路径取生效值。
    pub fn value_of(&self, path: &str) -> Option<&str> {
        self.active().and_then(|t| t.get(path))
    }

    /// 在途事务（只读）。
    pub fn txn(&self) -> Option<&SwitchTxn> {
        self.txn.as_ref()
    }

    /// 已提交但仍可回滚的事务（只读）。
    pub fn undone(&self) -> Option<&Undone> {
        self.undone.as_ref()
    }

    /// 上一笔提交是否仍可回滚。
    pub fn can_undo(&self) -> bool {
        self.undone.is_some()
    }

    /// 是否换肤互斥已被占。
    pub fn busy(&self) -> bool {
        self.txn.is_some()
    }

    /// 已推进到的逻辑 tick。
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// 最近一次提交的见证对（旧表, 新表）。截图断言要对它判。
    pub fn witness(&self) -> Option<(&SkinTable, &SkinTable)> {
        self.witness
            .as_ref().map(|(o, n)| (o, n))
    }

    /// 推进逻辑时钟。**回拨即拒**（[`SwitchCode::ClockRewind`]）：年龄算错会把
    /// 刚打开的事务当成悬空回滚掉，而回滚掉一个正在正常推进的事务是真实的数据损失。
    pub fn advance_to(&mut self, tick: u64) -> Result<(), SwitchDiag> {
        if tick < self.clock {
            self.clock_rewinds = self.clock_rewinds.saturating_add(1);
            return Err(SwitchDiag::new(
                SwitchCode::ClockRewind,
                Site::start(),
                &format!("注入的 tick {} 小于已推进到的 {}", tick, self.clock),
                "按回拨后的 tick 判事务年龄，会把刚打开的事务误判成悬空并回滚掉",
                &format!("注入不小于 {} 的 tick；单调时钟自己会走，不要往回拨", self.clock),
            ));
        }
        self.clock = tick;
        Ok(())
    }

    // ---- 首装 ------------------------------------------------------------

    /// 首装：把一张整表装为生效主题（此前必须没有生效主题）。
    pub fn mount(&mut self, table: SkinTable, tick: u64) -> Result<SwitchReport, SwitchDiag> {
        if self.active().is_some() {
            return Err(SwitchDiag::new(
                SwitchCode::TxnBusy,
                Site::start(),
                "首装失败：已有生效主题",
                "首装是「无主题可用」时的一次性入口；对已有主题应当走事务换肤，否则绕过快照与账本",
                "改用 begin/commit 走事务",
            ));
        }
        if self.txn.is_some() {
            return Err(SwitchDiag::new(
                SwitchCode::TxnBusy,
                Site::start(),
                "首装失败：已有事务在途",
                "换肤互斥：同一时刻至多一个在途事务",
                "先结束当前事务（提交、回滚或取消）",
            ));
        }
        self.advance_to(tick)?;
        table.verify()?;
        let txn_id = self.next_txn;
        self.next_txn = self.next_txn.saturating_add(1);
        let empty = Snapshot {
            theme: table.theme.clone(),
            epoch: 0,
            tick,
            values: Vec::new(),
        };
        self.push_journal(txn_id, JournalState::Committing, &table.theme, tick, empty.clone())?;
        let epoch = self.epoch + 1;
        let mut staged = table;
        staged.epoch = epoch;
        let changed = staged.len();
        let report = SwitchReport {
            txn: txn_id,
            kind: SwitchKind::Mount,
            theme: staged.theme.clone(),
            epoch,
            changed,
        };
        self.publish(staged, empty, txn_id, JournalState::Committed)?;
        Ok(report)
    }

    // ---- 事务三段：预演 → 暂存 → 提交 -------------------------------------

    /// 开事务：**只建暂存表与快照，生效面一个字节都不动**（预演干跑的落点）。
    ///
    /// 上游（解析/级联）若拒绝，**在这里就拒**（[`SwitchCode::UpstreamRejected`]），
    /// 账本不落条、生效面不变——这就是降级矩阵第一格"预演失败 → 取消切换"。
    pub fn begin_from_tokens(
        &mut self,
        theme: &str,
        ts: &TokenSet,
        dag: &TokenDag,
        tick: u64,
    ) -> Result<u64, SwitchDiag> {
        check_theme_id(theme)?;
        let engine = match CascadeEngine::new(ts.clone(), dag.clone()) {
            Ok(e) => e,
            Err(d) => {
                self.previews_rejected = self.previews_rejected.saturating_add(1);
                return Err(from_upstream(Site::start(), &d));
            }
        };
        let staged = match SkinTable::from_engine(theme, &engine) {
            Ok(t) => t,
            Err(d) => {
                self.previews_rejected = self.previews_rejected.saturating_add(1);
                return Err(d);
            }
        };
        self.begin(theme, staged, tick)
    }

    /// 开事务（表已建好的路径）。返回值是事务号。
    pub fn begin(&mut self, theme: &str, staged: SkinTable, tick: u64) -> Result<u64, SwitchDiag> {
        check_theme_id(theme)?;
        self.advance_to(tick)?;
        let live = match self.active() {
            Some(t) => t.clone(),
            None => {
                return Err(SwitchDiag::new(
                    SwitchCode::NoLiveTheme,
                    Site::start(),
                    "开事务失败：尚无生效主题",
                    "没有旧主题就没有可回滚的快照，而没有快照的换肤一旦失败只能靠用户重进程序",
                    "先用 mount 首装一套主题",
                ));
            }
        };
        if self.txn.is_some() {
            return Err(SwitchDiag::new(
                SwitchCode::TxnBusy,
                Site::start(),
                "开事务失败：已有事务在途",
                "换肤互斥：两个事务同时改生效面，快照与见证会互相覆盖，回滚将无从判断回哪一套",
                "先结束当前事务",
            ));
        }
        staged.verify()?;
        let snapshot = Snapshot::capture(&live, tick);
        let (report, steps) = self.diff(&live, &staged);
        self.last_diff_steps = steps;
        let id = self.next_txn;
        self.next_txn = self.next_txn.saturating_add(1);
        self.push_journal(id, JournalState::Previewed, theme, tick, snapshot.clone())?;
        self.txn = Some(SwitchTxn {
            id,
            theme: theme.to_string(),
            opened_tick: tick,
            snapshot,
            staged,
            preview: report,
            state: JournalState::Previewed,
            stage: 1,
        });
        Ok(id)
    }

    /// 预演报告（**只读**：不改任何状态，包括生效面与账本）。
    pub fn preview(&self, txn: u64) -> Result<PreviewReport, SwitchDiag> {
        let t = self.require_txn(txn, &[JournalState::Previewed, JournalState::Staged])?;
        let mut r = t.preview.clone();
        // 报告里的事务号由管理器填，不接受事务对象里那个占位 0。
        r.txn = txn;
        Ok(r)
    }

    /// 确认暂存（[`JournalState::Previewed`] → [`JournalState::Staged`]）。
    pub fn stage(&mut self, txn: u64, tick: u64) -> Result<&PreviewReport, SwitchDiag> {
        self.advance_to(tick)?;
        let t = self.require_txn(txn, &[JournalState::Previewed])?;
        let snap = t.snapshot.clone();
        let theme = t.theme.clone();
        self.push_journal(txn, JournalState::Staged, &theme, tick, snap)?;
        match self.txn.as_mut() {
            Some(t) => {
                t.state = JournalState::Staged;
                t.stage = 2;
                Ok(&t.preview)
            }
            None => Err(SwitchDiag::new(
                SwitchCode::TxnNotOpen,
                Site::start(),
                "暂存后事务不见了",
                "内部不一致：刚校验过的事务在推进阶段丢失",
                "重新开事务",
            )),
        }
    }

    /// 提交：**整表装进非生效槽，然后一次游标翻转**。
    ///
    /// 生效槽在本次调用中**只被读**。这是原子性的全部实现。
    pub fn commit(&mut self, txn: u64, tick: u64) -> Result<SwitchReport, SwitchDiag> {
        self.advance_to(tick)?;
        let t = match self.require_txn(txn, &[JournalState::Previewed, JournalState::Staged]) {
            Ok(t) => t.clone(),
            Err(d) => return Err(d),
        };
        self.push_journal(txn, JournalState::Committing, &t.theme, tick, t.snapshot.clone())?;
        let changed = t.preview.changed_len() + t.preview.shape_delta();
        let report = SwitchReport {
            txn,
            kind: SwitchKind::Commit,
            theme: t.staged.theme.clone(),
            epoch: self.epoch + 1,
            changed,
        };
        // 见证对由 publish 统一记录（那里才知道发布后的世代）。
        // 记下可回滚：提交不是终点，界面退回旧主题的凭据就是这张快照。
        self.undone = Some(Undone {
            id: txn,
            theme: t.staged.theme.clone(),
            tick,
            snapshot: t.snapshot.clone(),
        });
        self.publish(t.staged, t.snapshot, txn, JournalState::Committed)?;
        self.commits = self.commits.saturating_add(1);
        Ok(report)
    }

    /// 取消（预演判否或调用方改主意）：**丢暂存表，不碰生效面**。
    pub fn abort(&mut self, txn: u64, tick: u64) -> Result<SwitchReport, SwitchDiag> {
        self.advance_to(tick)?;
        let t = match self.require_txn(txn, &[JournalState::Previewed, JournalState::Staged]) {
            Ok(t) => t.clone(),
            Err(d) => return Err(d),
        };
        self.previews_rejected = self.previews_rejected.saturating_add(1);
        self.push_journal(txn, JournalState::Aborted, &t.theme, tick, t.snapshot.clone())?;
        let theme = self.theme().unwrap_or("").to_string();
        let epoch = self.epoch;
        self.txn = None;
        Ok(SwitchReport {
            txn,
            kind: SwitchKind::Abort,
            theme,
            epoch,
            changed: 0,
        })
    }

    /// 主动回滚：**在途事务**回滚，或**上一笔已提交事务**撤销。
    ///
    /// 两条路径合在一个入口是有意的：调用方（E02）面对的是同一个用户意图
    /// ——"退回换之前的界面"，不该关心那笔切换走到哪一步了。
    pub fn rollback(&mut self, txn: u64, tick: u64) -> Result<SwitchReport, SwitchDiag> {
        // 先看在途：悬空/未提交的路径走事务回滚。
        if let Some(t) = self.txn.as_ref() {
            if t.id == txn {
                return self.do_rollback(txn, tick, SwitchKind::Rollback);
            }
        }
        // 再看已提交但仍可回滚的那笔。
        if let Some(u) = self.undone.clone() {
            if u.id == txn {
                return self.undo(txn, tick);
            }
        }
        Err(SwitchDiag::new(
            SwitchCode::TxnNotOpen,
            Site::start(),
            &format!("回滚失败：事务 {} 既不在途，也不是可回滚的已提交事务", txn),
            "事务要么还没提交（在途），要么已经回滚过/已被下一次提交挤掉——两者都不是可回滚态",
            "先读 spoken() 确认 can_undo 与当前事务号",
        ))
    }

    /// 撤销一笔**已提交**的换肤（退回它提交前的快照）。
    fn undo(&mut self, txn: u64, tick: u64) -> Result<SwitchReport, SwitchDiag> {
        if self.txn.is_some() {
            return Err(SwitchDiag::new(
                SwitchCode::TxnBusy,
                Site::start(),
                "撤销失败：已有事务在途",
                "在途事务与撤销会争抢同一份见证对，两个都做会让快照判定失据",
                "先结束当前事务",
            ));
        }
        self.advance_to(tick)?;
        // **不用 expect**：调用方 [`Self::rollback`] 已比对过事务号，但那层
        // 保证在本函数里是「约定」而不是「类型」。一旦将来新增调用点绕过
        // 那层，这里就会 panic ——而生产路径上一次 panic 就是内核里的
        // 停机。宁可显式报 TxnNotOpen（同一诊断码，调用方已有处理分支）。
        let u = match self.undone.clone() {
            Some(u) => u,
            None => {
                return Err(SwitchDiag::new(
                    SwitchCode::TxnNotOpen,
                    Site::start(),
                    &format!("撤销失败：事务 {} 已不在可回滚集合里", txn),
                    "可回滚凭据在进入本函数后消失了——两次提交之间只有一次撤销机会",
                    "读 spoken() 确认 can_undo 与当前可回滚事务号",
                ))
            }
        };
        let restored = u.snapshot.restore()?;
        let theme = restored.theme.clone();
        let changed = match self.active() {
            Some(live) => {
                let (d, steps) = self.diff(live, &restored);
                self.last_diff_steps = steps;
                d.changed_len() + d.shape_delta()
            }
            None => restored.len(),
        };
        self.push_journal(
            txn,
            JournalState::RolledBack,
            &u.theme,
            tick,
            u.snapshot.clone(),
        )?;
        // 见证对由 publish 统一记录。
        self.publish(restored, u.snapshot.clone(), txn, JournalState::RolledBack)?;
        // 撤销后这一笔不再可回滚：再按一次回滚键会误撤销到更早的主题。
        self.undone = None;
        self.rollbacks = self.rollbacks.saturating_add(1);
        Ok(SwitchReport {
            txn,
            kind: SwitchKind::Rollback,
            theme,
            epoch: self.epoch,
            changed,
        })
    }

    fn do_rollback(
        &mut self,
        txn: u64,
        tick: u64,
        kind: SwitchKind,
    ) -> Result<SwitchReport, SwitchDiag> {
        self.advance_to(tick)?;
        let t = match self.require_txn(
            txn,
            &[
                JournalState::Previewed,
                JournalState::Staged,
                JournalState::Committing,
            ],
        ) {
            Ok(t) => t.clone(),
            Err(d) => return Err(d),
        };
        let end_state = match kind {
            SwitchKind::DanglingRollback => JournalState::DanglingRolledBack,
            _ => JournalState::RolledBack,
        };
        // 先落终局记录：账本绝不能停在 Committing 上——那正是崩溃恢复会误判
        // "还在提交中"的状态。
        self.push_journal(txn, end_state, &t.theme, tick, t.snapshot.clone())?;
        let restored = t.snapshot.restore()?;
        // 改动条数 = 恢复前生效面与快照之间的差分（三分类合计），如实计数。
        let changed = match self.active() {
            Some(live) => {
                let (d, steps) = self.diff(live, &restored);
                self.last_diff_steps = steps;
                d.changed_len() + d.shape_delta()
            }
            None => restored.len(),
        };
        let report = SwitchReport {
            txn,
            kind,
            theme: restored.theme.clone(),
            epoch: self.epoch + 1,
            changed,
        };
        // 见证对由 publish 统一记录。
        self.publish(restored, t.snapshot, txn, end_state)?;
        self.rollbacks = self.rollbacks.saturating_add(1);
        if kind == SwitchKind::DanglingRollback {
            self.dangling_rollbacks = self.dangling_rollbacks.saturating_add(1);
        }
        Ok(report)
    }

    /// 悬空兜底：在途事务判龄超 [`TXN_DANGLE_TICKS`] 即强制回滚。
    ///
    /// 返回 `None` = 无事务或未到龄。**至多一个在途事务，故本函数 O(1)。**
    pub fn sweep(&mut self, tick: u64) -> Result<Option<SwitchReport>, SwitchDiag> {
        self.advance_to(tick)?;
        let (id, opened) = match self.txn.as_ref() {
            None => return Ok(None),
            Some(t) => (t.id, t.opened_tick),
        };
        // saturating_sub：tick 回拨已在 advance_to 被拒，这里仍不靠减法判龄。
        let age = tick.saturating_sub(opened);
        if age < TXN_DANGLE_TICKS {
            return Ok(None);
        }
        Ok(Some(self.do_rollback(id, tick, SwitchKind::DanglingRollback)?))
    }

    /// 在途事务的年龄（tick）。
    pub fn txn_age(&self, tick: u64) -> Option<u64> {
        self.txn.as_ref().map(|t| tick.saturating_sub(t.opened_tick))
    }

    /// 崩溃恢复：**以账本为准**，把最后一条在途事务回滚掉。
    ///
    /// 不看生效面现状——现状恰恰是崩溃现场，把它当证据就是"自己说自己对"。
    pub fn recover(&mut self, tick: u64) -> Result<Option<SwitchReport>, SwitchDiag> {
        self.journal.verify()?;
        let pending = match self.journal.pending() {
            None => return Ok(None),
            Some(p) => p.clone(),
        };
        if tick.saturating_sub(pending.tick) > RECOVER_WINDOW_TICKS {
            return Ok(None);
        }
        // 账本说有在途事务而内存里没有（事务对象本身丢了）：只能按账本快照强制恢复。
        match self.txn.as_ref() {
            Some(t) if t.id == pending.txn => {
                Ok(Some(self.do_rollback(pending.txn, tick, SwitchKind::Recovered)?))
            }
            _ => self.recover_from_journal(&pending, tick),
        }
    }

    fn recover_from_journal(
        &mut self,
        pending: &JournalEntry,
        tick: u64,
    ) -> Result<Option<SwitchReport>, SwitchDiag> {
        let restored = pending.snapshot.restore()?;
        if !pending.snapshot.is_empty() {
            let theme = restored.theme.clone();
            self.push_journal(
                pending.txn,
                JournalState::RolledBack,
                &pending.theme,
                tick,
                pending.snapshot.clone(),
            )?;
            self.publish(
                restored,
                pending.snapshot.clone(),
                pending.txn,
                JournalState::RolledBack,
            )?;
            self.rollbacks = self.rollbacks.saturating_add(1);
            return Ok(Some(SwitchReport {
                txn: pending.txn,
                kind: SwitchKind::Recovered,
                theme,
                epoch: self.epoch,
                changed: 0,
            }));
        }
        // 快照为空 = 这条是首装事务，恢复等于撤掉首装：把生效面清回"无主题"。
        self.slots[self.active] = None;
        self.epoch = 0;
        self.witness = None;
        self.txn = None;
        self.push_journal(
            pending.txn,
            JournalState::RolledBack,
            &pending.theme,
            tick,
            pending.snapshot.clone(),
        )?;
        self.rollbacks = self.rollbacks.saturating_add(1);
        Ok(Some(SwitchReport {
            txn: pending.txn,
            kind: SwitchKind::Recovered,
            theme: pending.theme.clone(),
            epoch: 0,
            changed: 0,
        }))
    }

    // ---- 内部：发布与不变式 -----------------------------------------------

    /// 发布一张整表：**写非生效槽 → 一次游标翻转**。
    ///
    /// 生效槽在这个函数里**只被读一次**（算 `target` 的补数），全部写操作落在
    /// 非生效槽上，最后一步是 `self.active = target`。这是原子性的全部实现，
    /// 也就意味着"读到半新半旧"在结构上没有对应时机。
    fn publish(
        &mut self,
        mut staged: SkinTable,
        snapshot: Snapshot,
        txn: u64,
        end_state: JournalState,
    ) -> Result<(), SwitchDiag> {
        staged.verify()?;
        let epoch = self.epoch.saturating_add(1);
        staged.epoch = epoch;
        let target = 1 - self.active;
        let theme = staged.theme.clone();
        // 发布前的生效表（见证对的旧侧）——此刻它还是完整的旧值。
        let before = self.slots[self.active].clone();
        let after = staged.clone();
        // 写非生效槽。
        self.slots[target] = Some(staged);
        // 观察点·写后翻前：此刻生效槽**应当仍是完整的旧表**。就地改写的实现
        // 会在这个时刻已经把一部分令牌换成了新值——这里就是它露馅的地方。
        self.observe();
        // 单指针翻转：原子性的唯一一次写生效面的动作。
        self.active = target;
        // 观察点·翻后：此刻生效槽应当是完整的新表。
        self.observe();
        self.epoch = epoch;
        self.txn = None;
        // 见证对在这里统一记：两张表都带上了刚推进的世代。
        self.record_witness(before, after, epoch);
        self.push_journal(txn, end_state, &theme, self.clock, snapshot)
    }

    /// 触发一次发布观察。**无观察点时零成本**（一次分支）。
    ///
    /// 观察做两件事：把生效槽**此刻**的内容存进 [`Self::probe_frames`]，再调
    /// 回调。前者是判据要的证据，后者给调用方一个自己动手的位置。
    fn observe(&mut self) {
        if self.probe.is_none() {
            return;
        }
        let snapshot = match self.slots[self.active].clone() {
            Some(t) => t,
            None => return,
        };
        if self.probe_frames.len() >= MAX_PROBE_FRAMES {
            self.probe_dropped = self.probe_dropped.saturating_add(1);
            return;
        }
        self.probe_frames.push(snapshot.clone());
        if let Some(p) = self.probe {
            p(&snapshot);
        }
    }

    /// 装一个发布观察点（**仅供自检**：验证半新半旧有没有处可寻）。
    pub fn set_probe(&mut self, p: Option<PublishProbe>) {
        self.probe = p;
        self.probe_frames.clear();
        self.probe_dropped = 0;
    }

    /// 取走观察样本（判据侧消费后清空，便于下一段场景复用）。
    pub fn take_probe_frames(&mut self) -> Vec<SkinTable> {
        let out = self.probe_frames.clone();
        self.probe_frames.clear();
        out
    }

    /// **记见证对**：把"本次发布前"与"发布后"的两张表**按发布后的世代对齐**
    /// 存起来，供截图断言与 [`Self::verify_atomic`] 判。
    ///
    /// **为什么必须在这里做、且要对齐世代**：见证对是**发布后**才完整的信息
    /// （要两张表都在、都带着同一个世代），而调用方在调 [`Self::publish`] 之前
    /// 手上只有一张。若各调用方自己拼，拼出来的两张表 epoch 都是 0（建表态），
    /// 与生效表（epoch 已推进）逐字段不等，`verify_atomic` 会把**每一次正常提交**
    /// 都判成"两表拼接"——判据指向了一个永远红的错误方向。
    fn record_witness(&mut self, before: Option<SkinTable>, after: SkinTable, epoch: u64) {
        let mut after = after;
        after.epoch = epoch;
        if let Some(mut b) = before {
            // 旧表也要对齐到**发布前**的世代：它本来就是那时的生效表，epoch 天然正确；
            // 若它来自快照还原（如回滚路径），也已是当时的世代。此处只做兜底钳制，
            // 不让它比生效世代新——那会让"生效表等于见证的一侧"永远不成立。
            if b.epoch > epoch {
                b.epoch = epoch;
            }
            self.witness = Some((b, after));
        }
    }

    /// 账本落条（**满了直接拒绝，不静默丢**）。
    fn push_journal(
        &mut self,
        txn: u64,
        state: JournalState,
        theme: &str,
        tick: u64,
        snapshot: Snapshot,
    ) -> Result<(), SwitchDiag> {
        let r = self.journal.append(JournalEntry {
            seq: 0,
            txn,
            state,
            theme: theme.to_string(),
            tick,
            from_epoch: self.epoch,
            snapshot,
        });
        if r.is_err() {
            self.journal_rejects = self.journal_rejects.saturating_add(1);
        }
        r
    }

    /// 取事务并校验状态。
    fn require_txn(
        &self,
        txn: u64,
        allowed: &[JournalState],
    ) -> Result<&SwitchTxn, SwitchDiag> {
        let t = match self.txn.as_ref() {
            Some(t) if t.id == txn => t,
            _ => {
                return Err(SwitchDiag::new(
                    SwitchCode::TxnNotOpen,
                    Site::start(),
                    &format!("事务 {} 不在途", txn),
                    "当前没有这个在途事务；操作一个不存在的在途事务会让报告与实际生效面脱节",
                    "读账本确认事务号与状态",
                ));
            }
        };
        if !allowed.contains(&t.state) {
            return Err(SwitchDiag::new(
                SwitchCode::TxnNotOpen,
                Site::start(),
                &format!(
                    "事务 {} 处于{}，不接受本操作",
                    txn,
                    t.state.zh()
                ),
                "状态机拒绝在当前阶段推进：终局事务不可复活，在途事务不可跳过阶段",
                &format!(
                    "按状态机推进：预演 → 暂存 → 提交；或取消 / 回滚（当前 {}）",
                    t.state.zh()
                ),
            ));
        }
        Ok(t)
    }

    /// **有序归并差分**（O(n+m)，不是"两遍哈希表"）。
    ///
    /// 三分类如实报出：值变的（`changed`）、新表新增的（`added`）、新表缺失的
    /// （`removed`）。**形状变化不与值变化混为一谈**——只改值与增删令牌对用户
    /// 界面不是一回事。
    ///
    /// 同时把**真实归并步数**记进 [`Self::last_diff_steps`]：那是被测方自己走过的
    /// 工作量，供判据与独立解析式对账（防止"差分退化成两遍全表扫描"这种劣化
    /// 在功能不变的情况下混过去）。
    fn diff(&self, old: &SkinTable, new: &SkinTable) -> (PreviewReport, usize) {
        let mut changed: Vec<String> = Vec::new();
        let mut added: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        let (mut i, mut j) = (0usize, 0usize);
        let mut steps = 0usize;
        while i < old.values.len() && j < new.values.len() {
            if changed.len() + added.len() + removed.len() >= PREVIEW_DIFF_CAP {
                break;
            }
            steps += 1;
            let a = &old.values[i].0;
            let b = &new.values[j].0;
            match a.as_bytes().cmp(b.as_bytes()) {
                core::cmp::Ordering::Less => {
                    removed.push(a.clone());
                    i += 1;
                }
                core::cmp::Ordering::Greater => {
                    added.push(b.clone());
                    j += 1;
                }
                core::cmp::Ordering::Equal => {
                    if old.values[i].1 != new.values[j].1 {
                        changed.push(a.clone());
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        while i < old.values.len() && changed.len() + added.len() + removed.len() < PREVIEW_DIFF_CAP
        {
            steps += 1;
            removed.push(old.values[i].0.clone());
            i += 1;
        }
        while j < new.values.len() && changed.len() + added.len() + removed.len() < PREVIEW_DIFF_CAP
        {
            steps += 1;
            added.push(new.values[j].0.clone());
            j += 1;
        }
        (
            PreviewReport {
                txn: 0,
                from_theme: old.theme.clone(),
                to_theme: new.theme.clone(),
                changed,
                added,
                removed,
                verdict: PreviewVerdict::Clean,
            },
            steps,
        )
    }

    // ---- 断言侧：截图一致性与不变式 ---------------------------------------

    /// 当前生效面上取一帧。
    pub fn screenshot(&self, probe: &FrameProbe) -> Result<FrameSample, SwitchDiag> {
        let live = match self.active() {
            Some(t) => t,
            None => {
                return Err(SwitchDiag::new(
                    SwitchCode::NoLiveTheme,
                    Site::start(),
                    "截图失败：尚无生效主题",
                    "没有生效面就没有可采样的表",
                    "先首装一套主题",
                ));
            }
        };
        probe.sample(live)
    }

    /// **截图一致性断言**：当前这一帧相对最近一次提交的见证对是否半新半旧。
    ///
    /// 返回 `Ok(None)` = 尚无见证对（还没提交过，断言无从谈起）。
    pub fn judge_frame(
        &self,
        probe: &FrameProbe,
    ) -> Result<Option<MixedVerdict>, SwitchDiag> {
        let (old, new) = match self.witness() {
            None => return Ok(None),
            Some((o, n)) => (o.clone(), n.clone()),
        };
        let frame = self.screenshot(probe)?;
        Ok(Some(frame.mixed_with(&old, &new)))
    }

    /// 事务在途时的期望判定：在途期间生效面**必须整帧全旧**。
    ///
    /// 这是"半新半旧不存在"最直接的表述——不是"提交后会一致"，而是**提交发生
    /// 之前连一个字节都不能变**。
    pub fn expect_in_flight(&self, probe: &FrameProbe) -> Result<bool, SwitchDiag> {
        match self.judge_frame(probe)? {
            None => Ok(true),
            Some(v) => Ok(v == MixedVerdict::AllOld),
        }
    }

    /// 提交后的期望判定：生效面必须整帧全新。
    pub fn expect_committed(&self, probe: &FrameProbe) -> Result<bool, SwitchDiag> {
        match self.judge_frame(probe)? {
            None => Ok(true),
            Some(v) => Ok(v == MixedVerdict::AllNew),
        }
    }

    /// 原子性不变式自检：
    /// - 生效槽的表必须结构完整；
    /// - 生效表的世代必须等于当前世代（表与游标同源）；
    /// - 有见证对时，**当前生效表必须与见证对的一侧逐字节相等**——
    ///   这条才是"生效面不是两表拼起来的"的直接证据。
    pub fn verify_atomic(&self) -> Result<(), SwitchDiag> {
        let live = match self.active() {
            Some(t) => t,
            None => return Ok(()),
        };
        live.verify()?;
        if live.epoch != self.epoch {
            return Err(SwitchDiag::new(
                SwitchCode::JournalCorrupt,
                Site::start(),
                &format!(
                    "生效表世代 {} 与管理器世代 {} 不一致",
                    live.epoch, self.epoch
                ),
                "表与游标不同源，说明生效面被绕过发布路径改过",
                "从最近一次快照重建生效面",
            ));
        }
        if let Some((old, new)) = self.witness() {
            if live != old && live != new {
                let same_shape = live.same_shape(new);
                let mut offenders: Vec<String> = Vec::new();
                if same_shape {
                    for (idx, (path, value)) in live.values.iter().enumerate() {
                        if new.values[idx].1 != *value {
                            offenders.push(path.clone());
                        }
                    }
                }
                return Err(SwitchDiag::new(
                    SwitchCode::SkinIncomplete,
                    Site::start(),
                    &format!(
                        "生效表既不等于旧表也不等于新表（形状{}，差异令牌 {} 条）",
                        if same_shape { "一致" } else { "不一致" },
                        offenders.len()
                    ),
                    "生效面同时含有两套值 —— 这正是半新半旧，且已落到生效槽上",
                    &format!(
                        "从快照恢复；若形状不一致说明两张表令牌数不同，先核对主题形状。差异路径：{:?}",
                        offenders
                    ),
                ));
            }
        }
        Ok(())
    }

    /// 读屏替述（无障碍：切换状态要能念）。
    pub fn spoken(&self) -> String {
        let head = match self.active() {
            Some(t) => format!(
                "当前主题 {}，世代 {}，生效令牌 {} 条",
                t.theme,
                self.epoch,
                t.len()
            ),
            None => "当前没有生效主题".to_string(),
        };
        let txn = match self.txn.as_ref() {
            Some(t) => format!(
                "；事务 {} 处于{}（已完成 {}/{} 阶段，目标主题 {}）",
                t.id,
                t.state.zh(),
                t.stage,
                STAGES,
                t.theme
            ),
            None => "；当前无在途事务".to_string(),
        };
        let undo = match self.undone.as_ref() {
            Some(u) => format!("；上一笔事务 {}（主题 {}）仍可回滚", u.id, u.theme),
            None => "；当前无可回滚的已提交事务".to_string(),
        };
        format!(
            "{}{}{}；累计提交 {} 次，回滚 {} 次（其中悬空兜底 {} 次），预演判否 {} 次，账本拒收 {} 次",
            head,
            txn,
            undo,
            self.commits,
            self.rollbacks,
            self.dangling_rollbacks,
            self.previews_rejected,
            self.journal_rejects
        )
    }
}

impl Default for Switchboard {
    fn default() -> Self {
        Switchboard::new()
    }
}

// ---------------------------------------------------------------------------
// 九、故意非原子的参考发布器（判据鉴别力的证据，不是产品路径）
// ---------------------------------------------------------------------------

/// **非原子参考发布器**：逐令牌改写生效槽，每改一个令牌后取一帧，返回整条
/// 时间线。
///
/// 存在的唯一理由是**证明 [`MixedVerdict`] 有鉴别力**：如果半新半旧判定对这条
/// 路径也放行，那判定就是恒真的空断言。它逐令牌写生效槽——正是本模块头注里
/// 说的"朴素写法"，被隔离在 `_for_test` 命名下，不与 [`Switchboard::commit`]
/// 混用。
pub fn publish_in_place_for_test(
    board: &mut Switchboard,
    txn: u64,
    probe: &FrameProbe,
) -> Result<Vec<FrameSample>, SwitchDiag> {
    publish_in_place_ordered_for_test(board, txn, probe, PublishOrder::Forward)
}

/// 参考发布器的写入顺序。
///
/// **为什么要可逆序**（判据鉴别力的第二根支柱）：正序写入时，最后一个混色帧的
/// **旧侧恒只有 1 条成员**（只有"最后被写的那条会变探针"还没换新）。于是
/// "旧侧只记第一条"这种变异与正确实现**外部表现完全相同**——判据抓不到。
/// 逆序写入把分布翻过来：旧侧有多条成员、新侧只剩 1 条，两侧的对称漏洞同时暴露。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PublishOrder {
    /// 按目标表的字节序写入（正序）。
    Forward,
    /// 按字节序**倒序**写入（逆序）。
    Reverse,
}

/// 非原子参考发布器，可选写入顺序。
///
/// 正序与逆序共用同一份改写逻辑——两份实现各写一遍的话，改了一处忘了另一处，
/// 判据就会出现"某条路径根本没被覆盖"的留洞。
pub fn publish_in_place_ordered_for_test(
    board: &mut Switchboard,
    txn: u64,
    probe: &FrameProbe,
    order: PublishOrder,
) -> Result<Vec<FrameSample>, SwitchDiag> {
    let staged = match board.txn.as_ref() {
        Some(t) if t.id == txn => t.staged.clone(),
        _ => {
            return Err(SwitchDiag::new(
                SwitchCode::TxnNotOpen,
                Site::start(),
                &format!("事务 {} 不在途", txn),
                "参考发布器需要一个在途事务的暂存表",
                "先开事务",
            ));
        }
    };
    let target = staged.clone();
    let mut frames: Vec<FrameSample> = Vec::new();
    let snapshot = board.txn.as_ref().map(|t| t.snapshot.clone());
    let snap = match snapshot {
        Some(s) => s,
        None => {
            return Err(SwitchDiag::new(
                SwitchCode::TxnNotOpen,
                Site::start(),
                "参考发布器取不到快照",
                "在途事务必带快照",
                "先开事务",
            ));
        }
    };
    // ---- 朴素写法：就地逐令牌改写生效槽。每写一个就取一帧。 ----
    let mut live = board.slots[board.active]
        .clone()
        .unwrap_or_else(|| target.clone());
    // 写入次序：正序取字节序，逆序取其反序。表本身始终维持字节序（`get` 靠
    // 二分查找，乱序会让取样失真），只有**遍历次序**变。
    let mut write_order: Vec<usize> = (0..target.values.len()).collect();
    if order == PublishOrder::Reverse {
        write_order.reverse();
    }
    for i in write_order {
        let (path, value) = (&target.values[i].0, &target.values[i].1);
        match live
            .values
            .iter_mut()
            .find(|e| e.0.as_str() == path.as_str())
        {
            Some(slot) => slot.1 = value.clone(),
            None => live.values.push((path.clone(), value.clone())),
        }
        live.values.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        board.slots[board.active] = Some(live.clone());
        frames.push(probe.sample(&live)?);
    }
    let restored = snap.restore()?;
    board.slots[board.active] = Some(restored);
    board.txn = None;
    Ok(frames)
}

// ---------------------------------------------------------------------------
// 十、VE-F3404 域自检入口（判据逐条映射见 `ver01d_checks.rs`）
// ---------------------------------------------------------------------------

/// VE-F3404 域自检入口。
pub fn run_ver01d_checks() -> crate::checks::CheckSet {
    super::ver01d_checks::run_ver01d_checks()
}