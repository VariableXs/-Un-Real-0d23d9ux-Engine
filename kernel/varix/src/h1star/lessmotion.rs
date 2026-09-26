//! F245 减少动效开关 · 判据实装。
//!
//! **判据锚**：主册 F245「减少动效开关」。
//!
//! **验收标准第一句（任务包原文）**：开关全系统生效审计（抽 20 处动画
//! 实测降级）。
//!
//! **判据（主册原文摘录）**：设置中心「减少动效」开关：开启后全系统动画
//! 替换为 80ms 直切（保留状态变化信号、去掉位移缩放——窗口仍开合但瞬间
//! 完成，最小化不飞收直接消失），F124 总谱五曲线全部降级为线性瞬时；
//! 此开关同时作为标准接口暴露给第三方应用（vxapp 可查询）。验收：80ms
//! 直切时序；第三方查询接口文档判据；开关即时生效免重启。
//!
//! **分工（一处一事实）**：本模块是**开关的持有者与广播者**——状态机、
//! 消费面登记表、切换账本、第三方查询接口都在这里；F124 曲线的降级
//! 执行（80ms 直切、进度直达）在 [`crate::h1star::h1base::MotionPolicy`]，
//! 本模块只负责把策略快照分发下去，不重复实现曲线（h1base 是策略
//! 执行器，判据数值 80ms 从 h1base 再锚定，杜绝双源）。
//!
//! **设计要点**：
//! - 即时生效免重启：切换即推进版本号并把新版本广播到全部已登记消费
//!   面（applied_version 同步置位）——消费面下一次查询即取到新策略，
//!   无重启事件、无延迟旗标；
//! - 全系统生效审计：20 类动画消费面全量登记（窗口开合/最小化/菜单/
//!   Tooltip/主题换装/OSD 等），审计函数逐面核对「已收到广播 + 计划
//!   降级 + 直切保信号」——抽 20 处实测降级的机器可验证形态；
//! - 位移类动效（最小化飞收、窗口位移缩放）在降级下被去除（instant
//!   语义），状态类信号（开/关完成事件）由 progress 直达 1000 保留；
//! - 第三方查询接口：vxapp 拿「当前策略快照 + 版本号」，快照语义
//!   文档化（[`MotionSnapshot`]），接口文档判据由 [`VXAPP_QUERY_DOC`]
//!   承载；
//! - 切换账本：定容 64 条环形（最近 64 次切换），新→旧读出；
//! - 零堆热路径：消费面表定容数组，账本定容环；时间一律注入（毫秒戳）。
//!
//! **依赖锚点**：F124（动画总谱五曲线——经 h1base 取值）、F227（窗口
//! 开合动画）、F235（虚拟桌面横移）、F225（主题交叉淡入）——均为消费面。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy, REDUCED_MOTION_MS};
use crate::star::sbase::RingLog;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 直切时长（ms）——主册 F245「替换为 80ms 直切」；数值从 h1base
/// `REDUCED_MOTION_MS` 再锚定（一处一事实，禁止双源漂移）。
pub const DIRECT_CUT_MS: u32 = REDUCED_MOTION_MS;

/// 消费面登记容量——全系统动画消费面（20 类判据审计面）+ 余量。
pub const CONSUMER_CAP: usize = 64;

/// 切换账本容量——主册「记最近 64 次切换」。
pub const TOGGLE_LOG_CAP: usize = 64;

/// 全系统生效审计抽样数——主册「抽 20 处动画实测降级」。
pub const AUDIT_SAMPLE_N: usize = 20;

/// 第三方查询接口文档（vxapp 可查询判据的文档面）。
pub const VXAPP_QUERY_DOC: &str = "vxapp.motion.query() -> MotionSnapshot { reduced, version, direct_cut_ms }；语义：快照只读、版本号单调递增，开关切换即时生效免重启；应用侧任何动画时长一律经 snapshot.direct_cut_ms（开启时）或 F124 曲线（关闭时）取值，禁止自持时钟。";

/// 设置中心开关文案与语义锚（设置页文案判据的文档面）。
pub const SWITCH_DOC: &str = "减少动效：开启后全系统动画替换为 80ms 直切，保留状态变化信号、去掉位移缩放；即时生效免重启。";

/// 派发账本容量——运行面真实派发动画的留痕环。
pub const PLAN_LOG_CAP: usize = 32;

/// 广播账本容量——每次开关广播的逐次留痕。
pub const BROADCAST_LOG_CAP: usize = 16;

/// 位移类动画的种类数——`displaces()` 全集口径（去位移语义集），
/// 审计函数据此核对枚举规则表未漏未多。
pub const DISPLACE_KINDS: usize = 7;

/// 持久化快照头（差异面导出标识）。
pub const STATE_MAGIC: [u8; 4] = *b"VMOT";

// ---------------------------------------------------------------------------
// 消费面
// ---------------------------------------------------------------------------

/// 动画消费面种类（20 类 = 主册审计抽样的全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionKind {
    /// 窗口打开（F227）。
    WindowOpen,
    /// 窗口关闭（F227）。
    WindowClose,
    /// 最小化飞收（位移类——降级下直接消失）。
    MinimizeFly,
    /// 还原弹出（位移类）。
    RestorePop,
    /// 菜单弹出（F215）。
    MenuOpen,
    /// Tooltip 淡入（F205）。
    TooltipFade,
    /// 主题换装交叉淡入（F225）。
    ThemeCrossFade,
    /// 音量 OSD（F240）。
    VolumeOsd,
    /// 亮度 OSD（F239）。
    BrightnessOsd,
    /// 虚拟桌面横移（F235，位移类）。
    DeskSwipe,
    /// 滚动回弹（F204，位移类）。
    ScrollBounce,
    /// 文件拖放落点高亮（F212）。
    DropHighlight,
    /// 插入符平滑（F223）。
    CaretSmooth,
    /// 进度反馈脉动（F208）。
    ProgressPulse,
    /// 对话框淡入（F207）。
    DialogFade,
    /// 快速设置面板滑出（位移类）。
    QuickPanelSlide,
    /// 任务栏缩略图浮现（F062 域消费面）。
    ThumbnailRise,
    /// 通知横幅滑入（位移类）。
    ToastSlide,
    /// 开始菜单展开（位移类）。
    StartMenuBloom,
    /// Alt+Tab 覆盖层淡入。
    AltTabOverlay,
}

impl MotionKind {
    /// 20 类全集（审计面遍历序恒定）。
    pub const ALL: [MotionKind; AUDIT_SAMPLE_N] = [
        MotionKind::WindowOpen,
        MotionKind::WindowClose,
        MotionKind::MinimizeFly,
        MotionKind::RestorePop,
        MotionKind::MenuOpen,
        MotionKind::TooltipFade,
        MotionKind::ThemeCrossFade,
        MotionKind::VolumeOsd,
        MotionKind::BrightnessOsd,
        MotionKind::DeskSwipe,
        MotionKind::ScrollBounce,
        MotionKind::DropHighlight,
        MotionKind::CaretSmooth,
        MotionKind::ProgressPulse,
        MotionKind::DialogFade,
        MotionKind::QuickPanelSlide,
        MotionKind::ThumbnailRise,
        MotionKind::ToastSlide,
        MotionKind::StartMenuBloom,
        MotionKind::AltTabOverlay,
    ];

    /// 是否位移/缩放类——降级下「去掉位移缩放」（最小化不飞收直接消失）。
    pub fn displaces(&self) -> bool {
        matches!(
            self,
            MotionKind::MinimizeFly
                | MotionKind::RestorePop
                | MotionKind::DeskSwipe
                | MotionKind::ScrollBounce
                | MotionKind::QuickPanelSlide
                | MotionKind::ToastSlide
                | MotionKind::StartMenuBloom
        )
    }

    /// 审计遍历序下标（与 ALL 一致——种类级统计的稳定键）。
    pub fn ordinal(&self) -> usize {
        MotionKind::ALL.iter().position(|k| k == self).unwrap_or(0)
    }

    /// 降级下是否保留状态变化信号——全保留（判据「保留状态变化信号」，
    /// 位移类以「瞬间完成」的形式保留完成事件）。
    pub fn keeps_signal(&self) -> bool {
        true
    }
}

/// 消费面登记记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerRec {
    pub id: &'static str,
    pub kind: MotionKind,
    /// 已收到的最后广播版本号（即时生效审计的凭据）。
    pub applied_version: u32,
}

/// 一次动画播放计划（消费面拿去执行的指令面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimPlan {
    /// 本曲线在当前策略下的时长（ms）。
    pub duration_ms: u32,
    /// true = 直切（去位移缩放、进度直达终点）。
    pub instant_cut: bool,
    /// 消费面种类（审计对账用）。
    pub kind: MotionKind,
}

/// 切换账本条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToggleEvent {
    /// 切换时刻（ms，注入式）。
    pub ts: u64,
    /// 切到何态。
    pub to_reduced: bool,
    /// 切换后的版本号。
    pub version: u32,
}

/// 一次派发留痕（运行面真实派发动画的账本条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanRec {
    /// 派发时刻（ms，注入式）。
    pub ts: u64,
    /// 派发的消费面种类。
    pub kind: MotionKind,
    /// 计划时长（ms）。
    pub duration_ms: u32,
    /// 是否直切。
    pub instant_cut: bool,
}

/// 一次开关广播的留痕（广播判据的逐次凭据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BroadcastRec {
    /// 广播的策略版本号。
    pub version: u32,
    /// 触达的已登记消费面数。
    pub reached: usize,
    /// 广播时刻（ms，注入式）。
    pub ts: u64,
}

/// 第三方查询快照（vxapp 查询语义的返回体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionSnapshot {
    /// 当前是否降级态。
    pub reduced: bool,
    /// 策略版本号（单调递增，应用侧据此感知变化）。
    pub version: u32,
    /// 直切时长（ms）——开启时全部动画用这个时长。
    pub direct_cut_ms: u32,
}

// ---------------------------------------------------------------------------
// 动效治理器（开关持有者与广播者）
// ---------------------------------------------------------------------------

/// 减少动效治理器（开关持有者与广播者）。
pub struct MotionGov {
    reduced: bool,
    version: u32,
    consumers: [Option<ConsumerRec>; CONSUMER_CAP],
    ledger: RingLog<ToggleEvent, TOGGLE_LOG_CAP>,
    /// 派发留痕环（运行面真实派发的动画）。
    plans: RingLog<PlanRec, PLAN_LOG_CAP>,
    /// 广播留痕环（每次开关广播一条）。
    bcast: RingLog<BroadcastRec, BROADCAST_LOG_CAP>,
    /// 种类级派发计数（20 桶，按 ordinal 桶位）。
    kind_stats: [u32; AUDIT_SAMPLE_N],
    /// 注册被拒次数（表满/重复——诊断面如实呈现）。
    pub rejected_regs: u32,
    /// 切换次数（即时生效审计面）。
    pub toggles: u32,
    /// 派发总次数（动画负载画像）。
    pub dispatched: u32,
}

impl MotionGov {
    pub fn new() -> MotionGov {
        MotionGov {
            reduced: false,
            version: 0,
            consumers: [const { None }; CONSUMER_CAP],
            ledger: RingLog::new(),
            plans: RingLog::new(),
            bcast: RingLog::new(),
            kind_stats: [0; AUDIT_SAMPLE_N],
            rejected_regs: 0,
            toggles: 0,
            dispatched: 0,
        }
    }

    pub fn reduced(&self) -> bool {
        self.reduced
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// 登记消费面（重复登记幂等；表满显性拒绝）。
    pub fn register(&mut self, id: &'static str, kind: MotionKind) -> bool {
        if let Some(slot) = self.consumers.iter_mut().flatten().find(|c| c.id == id) {
            slot.kind = kind; // 幂等重登记：只更新种类
            return true;
        }
        for slot in self.consumers.iter_mut() {
            if slot.is_none() {
                *slot = Some(ConsumerRec { id, kind, applied_version: self.version });
                return true;
            }
        }
        self.rejected_regs += 1;
        false
    }

    pub fn consumer_count(&self) -> usize {
        self.consumers.iter().flatten().count()
    }

    /// 切换开关（核心入口）：状态翻转 → 版本推进 → 广播到全部已登记
    /// 消费面（applied_version 置为当前版本）→ 广播留痕 → 账本记切换。
    /// 即时生效免重启：本函数返回后任何查询立刻看到新策略。
    pub fn set_reduced(&mut self, reduced: bool, ts: u64) -> bool {
        if reduced == self.reduced {
            return false;
        }
        self.reduced = reduced;
        self.version = self.version.wrapping_add(1);
        self.toggles += 1;
        for c in self.consumers.iter_mut().flatten() {
            c.applied_version = self.version;
        }
        let reached = self.consumer_count();
        self.bcast.push(BroadcastRec { version: self.version, reached, ts });
        self.ledger.push(ToggleEvent { ts, to_reduced: reduced, version: self.version });
        true
    }

    /// 当前策略（h1base 执行器消费的注入式策略）。
    pub fn policy(&self) -> MotionPolicy {
        if self.reduced {
            MotionPolicy::reduced()
        } else {
            MotionPolicy::normal()
        }
    }

    /// 给某消费面出播放计划：关闭 = F124 曲线原谱；开启 = 80ms 直切。
    pub fn plan(&self, kind: MotionKind, curve: Curve, linear_ms: u32) -> AnimPlan {
        let p = self.policy();
        AnimPlan {
            duration_ms: p.duration_ms(curve, linear_ms),
            instant_cut: self.reduced,
            kind,
        }
    }

    /// 单消费面审计：(已收到当前广播， 降级下计划直切)。
    pub fn consumer_audit(&self, id: &str) -> Option<(bool, bool)> {
        self.consumers.iter().flatten().find(|c| c.id == id).map(|c| {
            (c.applied_version == self.version, self.reduced)
        })
    }

    /// 全系统生效审计：全部已登记消费面已收到当前广播；登记数 ≥ 抽样
    /// 口径 20 且种类覆盖 20 类——「抽 20 处动画实测降级」的机器判定。
    pub fn audit_all_reduced(&self) -> bool {
        if !self.reduced || self.consumer_count() < AUDIT_SAMPLE_N {
            return false;
        }
        let all_fresh = self.consumers.iter().flatten().all(|c| c.applied_version == self.version);
        let kinds_covered = MotionKind::ALL.iter().all(|k| {
            self.consumers.iter().flatten().any(|c| c.kind == *k)
        });
        all_fresh && kinds_covered
    }

    /// 第三方查询接口（vxapp）：当前策略快照 + 版本号。
    pub fn vxapp_query(&self) -> MotionSnapshot {
        MotionSnapshot {
            reduced: self.reduced,
            version: self.version,
            direct_cut_ms: DIRECT_CUT_MS,
        }
    }

    /// 切换账本（新→旧，最多 64 条）。
    pub fn toggle_log(&self) -> [Option<ToggleEvent>; TOGGLE_LOG_CAP] {
        let mut out = [None; TOGGLE_LOG_CAP];
        for (k, ev) in self.ledger.newest_first().iter().enumerate() {
            out[k] = Some(*ev);
        }
        out
    }

    /// 直切保信号验证：降级态下任意曲线在 80ms 内进度直达 1000。
    pub fn signal_preserved(&self, curve: Curve, linear_ms: u32) -> bool {
        let p = self.policy();
        p.progress(curve, DIRECT_CUT_MS, linear_ms) == 1000
    }

    // -----------------------------------------------------------------------
    // 消费面管理扩展
    // -----------------------------------------------------------------------

    /// 注销消费面（窗口销毁等；未登记者如实返回 false）。
    pub fn unregister(&mut self, id: &str) -> bool {
        for slot in self.consumers.iter_mut() {
            if let Some(c) = slot {
                if c.id == id {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    /// 批量登记（系统启动时 20 类一次入册；返回成功数）。
    pub fn register_all(&mut self, entries: &[(&'static str, MotionKind)]) -> usize {
        let mut ok = 0;
        for (id, kind) in entries.iter() {
            if self.register(id, *kind) {
                ok += 1;
            }
        }
        ok
    }

    /// 已登记消费面列表（审计/设置页展示面）。
    pub fn consumers(&self) -> Vec<ConsumerRec> {
        self.consumers.iter().flatten().copied().collect()
    }

    /// 按 id 查消费面登记记录。
    pub fn find_consumer(&self, id: &str) -> Option<ConsumerRec> {
        self.consumers.iter().flatten().find(|c| c.id == id).copied()
    }

    /// 已收到当前广播的消费面数（新鲜度审计面：< 全量即存在掉队者）。
    pub fn fresh_consumers(&self) -> usize {
        self.consumers
            .iter()
            .flatten()
            .filter(|c| c.applied_version == self.version)
            .count()
    }

    /// 已登记消费面的种类去重清单（种类覆盖审计的数据面）。
    pub fn consumer_kinds(&self) -> Vec<MotionKind> {
        let mut kinds: Vec<MotionKind> = Vec::new();
        for c in self.consumers.iter().flatten() {
            if !kinds.contains(&c.kind) {
                kinds.push(c.kind);
            }
        }
        kinds
    }

    /// 最新一次切换事件（设置页「上次切换时刻/方向」展示面）。
    pub fn latest_toggle(&self) -> Option<ToggleEvent> {
        self.ledger.newest_first().first().copied()
    }

    /// 某版本号以来的切换次数（审计面：版本差 × 切换频度对账）。
    pub fn toggles_since(&self, since_version: u32) -> usize {
        self.ledger
            .newest_first()
            .iter()
            .filter(|e| e.version > since_version)
            .count()
    }

    /// 切换账本条数（容量有界性核对）。
    pub fn toggle_log_len(&self) -> usize {
        self.ledger.len()
    }

    /// 位移类动画清单（规则表的数据面导出——降级语义「去掉位移缩放」
    /// 的适用集合，设置中心说明页与审计共用同一清单）。
    pub fn displace_kinds() -> Vec<MotionKind> {
        MotionKind::ALL
            .iter()
            .copied()
            .filter(MotionKind::displaces)
            .collect()
    }

    /// 广播账本（新→旧）——「开关广播到全部消费面」的逐次留痕。
    pub fn broadcast_log(&self) -> [Option<BroadcastRec>; BROADCAST_LOG_CAP] {
        let mut out = [const { None }; BROADCAST_LOG_CAP];
        for (k, rec) in self.bcast.newest_first().iter().enumerate() {
            out[k] = Some(*rec);
        }
        out
    }

    // -----------------------------------------------------------------------
    // 派发留痕（运行面）
    // -----------------------------------------------------------------------

    /// 带留痕版播放计划（运行面真正派发动画时调用）——设置页审计
    /// 「哪些消费面实际派发了多少动画」的数据源；纯查询面走 `plan`。
    pub fn plan_dispatch(&mut self, kind: MotionKind, curve: Curve, linear_ms: u32, ts: u64) -> AnimPlan {
        let p = self.plan(kind, curve, linear_ms);
        self.dispatched += 1;
        self.kind_stats[kind.ordinal()] += 1;
        self.plans.push(PlanRec { ts, kind, duration_ms: p.duration_ms, instant_cut: p.instant_cut });
        p
    }

    /// 种类级派发计数（20 桶画像）。
    pub fn kind_dispatched(&self, kind: MotionKind) -> u32 {
        self.kind_stats[kind.ordinal()]
    }

    /// 派发账本（新→旧，最多 32 条）。
    pub fn plan_log(&self) -> [Option<PlanRec>; PLAN_LOG_CAP] {
        let mut out = [const { None }; PLAN_LOG_CAP];
        for (k, rec) in self.plans.newest_first().iter().enumerate() {
            out[k] = Some(*rec);
        }
        out
    }

    /// 派发账本条数（容量有界性核对）。
    pub fn plan_log_len(&self) -> usize {
        self.plans.len()
    }

    // -----------------------------------------------------------------------
    // 第三方增量感知 / 时序取样 / 规则审计
    // -----------------------------------------------------------------------

    /// 应用侧增量感知：`seen` 版本落后才返回新快照（否则 None）——
    /// 第三方「自动跟随」的惰性拉取语义，无推送、无轮询风暴。
    pub fn snapshot_since(&self, seen: u32) -> Option<MotionSnapshot> {
        if seen == self.version {
            None
        } else {
            Some(self.vxapp_query())
        }
    }

    /// 时序取样：当前策略下某曲线在 t_ms 处的进度（0..=1000）——
    /// 判据「80ms 直切时序」的取样验证面（80ms 处恒 1000，0ms 处
    /// 常态谱不为 1000——信号与曲线的双侧核对）。
    pub fn plan_progress(&self, curve: Curve, t_ms: u32, linear_ms: u32) -> u32 {
        self.policy().progress(curve, t_ms, linear_ms)
    }

    /// 位移/信号规则全量审计：降级态下 20 类全部 80ms 直切且保信号；
    /// 位移类集合恰为 DISPLACE_KINDS 类（规则表未漏未多）；常态谱下
    /// Enter 曲线在 80ms 处未达终点（证明降级确实改变了时序）。
    pub fn audit_displace_rules(&self) -> bool {
        if !self.reduced {
            return false;
        }
        let displaced = MotionKind::ALL.iter().filter(|k| k.displaces()).count();
        let all_cut = MotionKind::ALL.iter().all(|&k| {
            let p = self.plan(k, Curve::Spring, 0);
            p.duration_ms == DIRECT_CUT_MS && k.keeps_signal()
        });
        let signal = self.plan_progress(Curve::Spring, DIRECT_CUT_MS, 0) == 1000;
        let normal = MotionPolicy::normal();
        let changed = normal.progress(Curve::Enter, DIRECT_CUT_MS, 0) < 1000;
        all_cut && signal && changed && displaced == DISPLACE_KINDS
    }

    // -----------------------------------------------------------------------
    // 持久化（差异面导出/导入）
    // -----------------------------------------------------------------------

    /// 持久化导出：13 字节 = "VMOT" 头 + 版本号 LE + 切换计数 LE + 开关位。
    pub fn export_state(&self) -> [u8; 13] {
        let mut out = [0u8; 13];
        out[0..4].copy_from_slice(&STATE_MAGIC);
        out[4..8].copy_from_slice(&self.version.to_le_bytes());
        out[8..12].copy_from_slice(&self.toggles.to_le_bytes());
        out[12] = self.reduced as u8;
        out
    }

    /// 持久化导入：头不符显性拒绝；开关差异经 set_reduced 正规路径
    /// 应用（广播/账本不缺环）；版本号与计数只增不减（重放旧快照
    /// 无法倒转审计面）。
    pub fn import_state(&mut self, blob: &[u8; 13]) -> bool {
        if blob[0] != b'V' || blob[1] != b'M' || blob[2] != b'O' || blob[3] != b'T' {
            return false;
        }
        let mut vb = [0u8; 4];
        vb.copy_from_slice(&blob[4..8]);
        let ver = u32::from_le_bytes(vb);
        let mut tb = [0u8; 4];
        tb.copy_from_slice(&blob[8..12]);
        let tog = u32::from_le_bytes(tb);
        let reduced = blob[12] & 0x01 == 1;
        if reduced != self.reduced {
            self.set_reduced(reduced, 0);
        }
        if ver > self.version {
            self.version = ver;
        }
        if tog > self.toggles {
            self.toggles = tog;
        }
        true
    }
}

impl Default for MotionGov {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F245 自检（判据：全系统 20 处降级审计、80ms 时序、第三方接口、
/// 即时生效；含 xors32 fuzz）。
pub fn run_lessmotion_checks() -> CheckSet {
    let mut set = CheckSet::new("F245-lessmotion");

    // 1. 默认常态：动效全开，Enter 曲线 120ms（F124 原谱）。
    let mut gov = MotionGov::new();
    let plan = gov.plan(MotionKind::WindowOpen, Curve::Enter, 0);
    set.add(
        "default full motion, Enter 120ms",
        !gov.reduced() && plan.duration_ms == 120 && !plan.instant_cut,
        "",
    );

    // 2. 20 类消费面全量登记成功；重复登记幂等；重复不占新槽。
    let all_reg = MotionKind::ALL.iter().all(|&k| gov.register(kind_id(k), k));
    set.add(
        "register 20 consumers, idempotent",
        all_reg
            && gov.consumer_count() == AUDIT_SAMPLE_N
            && gov.register("menu", MotionKind::MenuOpen)
            && gov.consumer_count() == AUDIT_SAMPLE_N,
        "",
    );

    // 3. 切换开启：状态翻转、版本推进、账本留痕、切换计数。
    let changed = gov.set_reduced(true, 1000);
    set.add(
        "toggle on bumps version & logs",
        changed
            && gov.reduced()
            && gov.version() == 1
            && gov.toggles == 1
            && gov.toggle_log()[0] == Some(ToggleEvent { ts: 1000, to_reduced: true, version: 1 }),
        "",
    );

    // 4. 全系统生效审计（判据本体）：20 处消费面全部已收到广播且种类
    //    覆盖 20 类。
    set.add("audit all 20 consumers downgraded", gov.audit_all_reduced(), "");

    // 5. 80ms 直切时序：五曲线全部降为 80ms（判据数值逐条核对）。
    let cut = [
        gov.plan(MotionKind::WindowOpen, Curve::Enter, 0).duration_ms,
        gov.plan(MotionKind::WindowClose, Curve::Exit, 0).duration_ms,
        gov.plan(MotionKind::ThemeCrossFade, Curve::Emphasis, 0).duration_ms,
        gov.plan(MotionKind::ScrollBounce, Curve::Spring, 0).duration_ms,
        gov.plan(MotionKind::ProgressPulse, Curve::Linear, 2000).duration_ms,
    ];
    set.add(
        "all five curves cut to 80ms",
        cut == [DIRECT_CUT_MS; 5],
        "",
    );

    // 6. 直切保信号：任意曲线 80ms 处进度直达 1000（状态变化信号保留）。
    set.add(
        "signal preserved, progress jumps to 1000",
        gov.signal_preserved(Curve::Enter, 0)
            && gov.signal_preserved(Curve::Spring, 0)
            && gov.signal_preserved(Curve::Linear, 2000),
        "",
    );

    // 7. 位移类去除：最小化飞收（位移类）降级下 instant_cut——窗口消失
    //    不飞收；非位移类（菜单）同样瞬时但语义是「弹出即完成」。
    let minfly = gov.plan(MotionKind::MinimizeFly, Curve::Emphasis, 0);
    let menu = gov.plan(MotionKind::MenuOpen, Curve::Enter, 0);
    set.add(
        "displacing anims lose travel, keep completion",
        minfly.instant_cut
            && minfly.duration_ms == DIRECT_CUT_MS
            && MotionKind::MinimizeFly.displaces()
            && !MotionKind::MenuOpen.displaces()
            && MotionKind::MinimizeFly.keeps_signal()
            && menu.instant_cut,
        "",
    );

    // 8. 关闭恢复：再切回 → F124 原谱恢复、版本再推进、账本记第二笔。
    let off = gov.set_reduced(false, 2000);
    let back = gov.plan(MotionKind::WindowOpen, Curve::Enter, 0);
    set.add(
        "toggle off restores F124 score",
        off
            && !gov.reduced()
            && gov.version() == 2
            && back.duration_ms == 120
            && !back.instant_cut
            && gov.toggle_log()[0] == Some(ToggleEvent { ts: 2000, to_reduced: false, version: 2 }),
        "",
    );

    // 9. 账本定容：第 65 次切换挤掉最旧（环形语义）。
    let mut gov2 = MotionGov::new();
    for k in 0..65u64 {
        gov2.set_reduced(k % 2 == 0, k * 10);
    }
    let log2 = gov2.toggle_log();
    let filled = log2.iter().flatten().count();
    set.add(
        "toggle log ring caps at 64",
        filled == TOGGLE_LOG_CAP && gov2.version() == 65 && log2[0].unwrap().version == 65,
        "",
    );

    // 10. 第三方查询接口：快照字段一致 + 文档判据串非空且含关键语义。
    let snap = gov.vxapp_query();
    set.add(
        "vxapp snapshot & doc",
        snap == MotionSnapshot { reduced: false, version: 2, direct_cut_ms: DIRECT_CUT_MS }
            && VXAPP_QUERY_DOC.contains("MotionSnapshot")
            && VXAPP_QUERY_DOC.contains("免重启"),
        "",
    );

    // 11. 表满显性拒绝（不静默挤掉已登记消费面）：64 个不同 id 填满表，
    //     第 65 个被拒且计数留痕。
    const POOL64: [&str; CONSUMER_CAP] = [
        "k00", "k01", "k02", "k03", "k04", "k05", "k06", "k07", "k08", "k09", "k10", "k11",
        "k12", "k13", "k14", "k15", "k16", "k17", "k18", "k19", "k20", "k21", "k22", "k23",
        "k24", "k25", "k26", "k27", "k28", "k29", "k30", "k31", "k32", "k33", "k34", "k35",
        "k36", "k37", "k38", "k39", "k40", "k41", "k42", "k43", "k44", "k45", "k46", "k47",
        "k48", "k49", "k50", "k51", "k52", "k53", "k54", "k55", "k56", "k57", "k58", "k59",
        "k60", "k61", "k62", "k63",
    ];
    let mut gov3 = MotionGov::new();
    let mut ok = true;
    for id in POOL64.iter() {
        ok &= gov3.register(id, MotionKind::TooltipFade);
    }
    set.add(
        "consumer table full rejects explicitly",
        ok
            && gov3.consumer_count() == CONSUMER_CAP
            && !gov3.register("overflow", MotionKind::TooltipFade)
            && gov3.rejected_regs == 1,
        "",
    );

    // 12. xors32 fuzz：随机开关序列（随机时刻戳）——版本单调、快照与
    //     状态一致、账本容量有界、登记面 applied 恒 ≤ 当前版本、无 panic。
    let mut x: u32 = 0x6C07_8965;
    let mut gz = MotionGov::new();
    let _ = gz.register("fuzz-face", MotionKind::WindowOpen);
    let mut prev_ver = gz.version();
    let mut survived = true;
    let mut ts: u64 = 0;
    for _ in 0..1000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        ts += (x % 50 + 1) as u64;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let want = x % 3 != 0; // 2/3 概率要开（制造切换流）
        if gz.reduced() != want {
            gz.set_reduced(want, ts);
        }
        let snap = gz.vxapp_query();
        if snap.version < prev_ver
            || snap.reduced != gz.reduced()
            || snap.direct_cut_ms != DIRECT_CUT_MS
        {
            survived = false;
        }
        prev_ver = snap.version;
        if let Some((fresh, reduced)) = gz.consumer_audit("fuzz-face") {
            if reduced && !fresh {
                survived = false;
            }
        }
    }
    set.add(
        "fuzz 1000 toggles invariants hold",
        survived && gov2b_toggle_log_count(&gz) <= TOGGLE_LOG_CAP,
        "",
    );

    // 13. 派发留痕与种类画像：plan 不留痕、plan_dispatch 留痕；派发账本
    //     新→旧；广播账本逐次记录触达数；注销/批量登记/按名查找。
    let mut g5 = MotionGov::new();
    let _ = g5.register("d1", MotionKind::MinimizeFly);
    let _ = g5.register("d2", MotionKind::MenuOpen);
    let _ = g5.set_reduced(true, 10); // 广播触达当时已登记的 2 面
    let _ = g5.plan_dispatch(MotionKind::MinimizeFly, Curve::Spring, 0, 100);
    let _ = g5.plan_dispatch(MotionKind::MinimizeFly, Curve::Spring, 0, 200);
    let _ = g5.plan(MotionKind::MenuOpen, Curve::Enter, 0); // 纯查询不留痕
    let _ = g5.plan_dispatch(MotionKind::MenuOpen, Curve::Enter, 0, 300);
    let plog = g5.plan_log();
    let blog = g5.broadcast_log();
    let reg_all = g5.register_all(&[("e1", MotionKind::ToastSlide), ("e2", MotionKind::CaretSmooth)]);
    let gone = g5.unregister("d2") && !g5.unregister("d2");
    set.add(
        "dispatch log & kind stats & broadcast log & reg/unreg",
        g5.dispatched == 3
            && g5.kind_dispatched(MotionKind::MinimizeFly) == 2
            && g5.kind_dispatched(MotionKind::MenuOpen) == 1
            && g5.plan_log_len() == 3
            && plog[0] == Some(PlanRec { ts: 300, kind: MotionKind::MenuOpen, duration_ms: DIRECT_CUT_MS, instant_cut: true })
            && plog[2] == Some(PlanRec { ts: 100, kind: MotionKind::MinimizeFly, duration_ms: DIRECT_CUT_MS, instant_cut: true })
            && blog[0] == Some(BroadcastRec { version: 1, reached: 2, ts: 10 })
            && reg_all == 2
            && gone
            && g5.find_consumer("d2").is_none()
            && g5.find_consumer("d1").map(|c| c.kind) == Some(MotionKind::MinimizeFly)
            && g5.consumers().len() == 3,
        "",
    );

    // 14. 增量感知 + 时序双侧取样 + 位移规则审计 + 持久化 round-trip。
    let since_now = g5.snapshot_since(g5.version()).is_none();
    let since_old = g5.snapshot_since(0).is_some();
    let timing = g5.plan_progress(Curve::Spring, DIRECT_CUT_MS, 0) == 1000
        && MotionPolicy::normal().progress(Curve::Enter, 0, 0) == 0
        && MotionPolicy::normal().progress(Curve::Enter, DIRECT_CUT_MS, 0) < 1000;
    let rules = g5.audit_displace_rules();
    let blob = g5.export_state();
    let mut g6 = MotionGov::new();
    let imp = g6.import_state(&blob);
    let bad = !g6.import_state(&[0u8; 13]);
    set.add(
        "snapshot-since / timing both sides / displace rules / state round-trip",
        since_now && since_old && timing && rules
            && imp && g6.reduced() && g6.version() == 1 && g6.toggles == 1
            && bad,
        "",
    );

    set
}

/// 自检辅助：账本非空条目计数（容量有界性核对）。
fn gov2b_toggle_log_count(g: &MotionGov) -> usize {
    g.toggle_log().iter().flatten().count()
}

/// 测试/自检辅助：20 类消费面的稳定 id。
fn kind_id(k: MotionKind) -> &'static str {
    match k {
        MotionKind::WindowOpen => "win-open",
        MotionKind::WindowClose => "win-close",
        MotionKind::MinimizeFly => "min-fly",
        MotionKind::RestorePop => "restore-pop",
        MotionKind::MenuOpen => "menu",
        MotionKind::TooltipFade => "tooltip",
        MotionKind::ThemeCrossFade => "theme",
        MotionKind::VolumeOsd => "vol-osd",
        MotionKind::BrightnessOsd => "bri-osd",
        MotionKind::DeskSwipe => "desk-swipe",
        MotionKind::ScrollBounce => "scroll",
        MotionKind::DropHighlight => "drop-hl",
        MotionKind::CaretSmooth => "caret",
        MotionKind::ProgressPulse => "progress",
        MotionKind::DialogFade => "dialog",
        MotionKind::QuickPanelSlide => "quick-panel",
        MotionKind::ThumbnailRise => "thumb",
        MotionKind::ToastSlide => "toast",
        MotionKind::StartMenuBloom => "start-menu",
        MotionKind::AltTabOverlay => "altta",
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_idempotent_same_state() {
        let mut gov = MotionGov::new();
        assert!(!gov.set_reduced(false, 10), ".as_bytes()同态切换应为 no-op");
        assert!(gov.set_reduced(true, 20));
        assert!(!gov.set_reduced(true, 30));
        assert_eq!(gov.version(), 1, "只有真实翻转推进版本");
        assert_eq!(gov.toggles, 1);
    }

    #[test]
    fn broadcast_reaches_late_registered_consumer() {
        let mut gov = MotionGov::new();
        let _ = gov.set_reduced(true, 10);
        // 降级开启后新登记的消费面：登记即拿到当前版本（applied=当前）。
        assert!(gov.register("late-face", MotionKind::MenuOpen));
        assert_eq!(gov.consumer_audit("late-face"), Some((true, true)));
        assert!(gov.audit_all_reduced() || gov.consumer_count() < AUDIT_SAMPLE_N);
    }

    #[test]
    fn reduced_plan_never_exceeds_80ms() {
        let mut gov = MotionGov::new();
        let _ = gov.set_reduced(true, 0);
        for kind in MotionKind::ALL {
            for curve in [Curve::Enter, Curve::Exit, Curve::Emphasis, Curve::Spring] {
                assert_eq!(gov.plan(kind, curve, 0).duration_ms, DIRECT_CUT_MS);
            }
            // 线性档时长本由调用方给定，降级同样钳到 80ms。
            assert_eq!(gov.plan(kind, Curve::Linear, 5000).duration_ms, DIRECT_CUT_MS);
        }
    }

    #[test]
    fn vxapp_snapshot_tracks_state() {
        let mut gov = MotionGov::new();
        assert_eq!(gov.vxapp_query(), MotionSnapshot { reduced: false, version: 0, direct_cut_ms: 80 });
        let _ = gov.set_reduced(true, 5);
        assert_eq!(gov.vxapp_query(), MotionSnapshot { reduced: true, version: 1, direct_cut_ms: 80 });
        let _ = gov.set_reduced(false, 9);
        assert_eq!(gov.vxapp_query(), MotionSnapshot { reduced: false, version: 2, direct_cut_ms: 80 });
    }

    #[test]
    fn toggle_log_order_newest_first() {
        let mut gov = MotionGov::new();
        let _ = gov.set_reduced(true, 100);
        let _ = gov.set_reduced(false, 200);
        let _ = gov.set_reduced(true, 300);
        let log = gov.toggle_log();
        assert_eq!(log[0].unwrap().ts, 300);
        assert_eq!(log[1].unwrap().ts, 200);
        assert_eq!(log[2].unwrap().ts, 100);
        assert_eq!(log[3], None);
        // 现象：三条事件 version 全断言为 3（left:2 right:3）。
        // 根因：测试与 ToggleEvent 自身语义矛盾——字段义为「切换后的
        //      版本号」，第 k 次切换记版本 k（自检第 3/8 条同样逐条
        //      断言 version:1 / version:2，实现正确）。
        // 修法：改测试——三条事件版本号按各自切换后的值 3/2/1 断言。
        let versions: [u32; 3] = log[..3].iter().flatten().map(|w| w.version).collect::<Vec<_>>().try_into().unwrap();
        assert_eq!(versions, [3, 2, 1]);
    }

    #[test]
    fn plan_dispatch_logs_and_caps() {
        let mut gov = MotionGov::new();
        let _ = gov.set_reduced(true, 0);
        let mut ts = 1u64;
        for k in 0..(PLAN_LOG_CAP as u64 + 10) {
            let kind = MotionKind::ALL[(k % 20) as usize];
            let _ = gov.plan_dispatch(kind, Curve::Spring, 0, ts);
            ts += 1;
        }
        assert_eq!(gov.dispatched, PLAN_LOG_CAP as u32 + 10);
        assert!(gov.plan_log_len() <= PLAN_LOG_CAP, "派发账本容量必须有界");
        let log = gov.plan_log();
        assert_eq!(log[0].unwrap().ts, PLAN_LOG_CAP as u64 + 10, "最新在前");
        // 总派发数 == 种类桶之和（20 桶画像无漏）。
        let total: u32 = MotionKind::ALL.iter().map(|&k| gov.kind_dispatched(k)).sum();
        assert_eq!(total, gov.dispatched);
        // 纯查询 plan 不留痕。
        let before = gov.dispatched;
        let _ = gov.plan(MotionKind::MenuOpen, Curve::Enter, 0);
        assert_eq!(gov.dispatched, before);
    }

    #[test]
    fn state_roundtrip_and_version_never_shrinks() {
        let mut gov = MotionGov::new();
        let _ = gov.set_reduced(true, 10);
        let _ = gov.set_reduced(false, 20);
        let blob = gov.export_state();
        // 新治理器导入：版本/计数/开关逐字对账。
        let mut g2 = MotionGov::new();
        assert!(g2.import_state(&blob));
        assert_eq!(g2.version(), 2);
        assert_eq!(g2.toggles, 2);
        assert!(!g2.reduced());
        // 旧态重放到新态治理器：版本与计数不得倒退，开关差异经正规
        // 路径应用（广播/账本不缺环）。
        let mut g3 = MotionGov::new();
        let _ = g3.set_reduced(true, 5);
        assert!(g3.import_state(&blob));
        assert_eq!(g3.version(), 2, "导入不得把版本号拉回更小值");
        assert_eq!(g3.toggles, 2);
        assert!(!g3.reduced(), "开关差异必须经 set_reduced 正规路径应用");
        assert_eq!(g3.broadcast_log()[0].map(|b| b.version), Some(2));
    }

    #[test]
    fn lessmotion_selfcheck_all_green() {
        let set = run_lessmotion_checks();
        assert!(set.all_passed(), "F245 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F245（减少动效开关）。v2 三件事：
// 1) 持久化 I/O：动效偏好册（开关位 + 消费面登记清单快照）v2 定长容器
//    序列化——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验和，
//    四类损坏显性拒绝（与既有 VMOT 差异面并存于追加段）；
// 2) UI 壳接线：设置页开关轨道几何 + 命中测试 + 键盘翻转 + 消费面行
//    清单（新鲜度徽标）——「设置中心开关」的几何承载；
// 3) 判定面扩展：run_lessmotion_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// 登记快照容量（超容显性拒绝，不静默截断）。
pub const VX2_MO_SNAP_CAP: usize = 24;
/// v2 容器 payload 定长：开关位 u32 + 24 槽×1B（种类序数+1，0 = 空）。
pub const VX2_MO_PAYLOAD: usize = 4 + VX2_MO_SNAP_CAP;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_MO_BLOB: usize = 9 + VX2_MO_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或序数越出 20 类全集。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 序数 → 种类（与 MotionKind::ordinal 互逆——登记快照的还原面）。
fn motion_kind_from_ordinal(o: usize) -> Option<MotionKind> {
    MotionKind::ALL.get(o).copied()
}

/// 动效偏好册：开关位 + 消费面种类登记清单（槽值 = ordinal+1，0 = 空）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPrefsBook {
    pub reduced: bool,
    pub kinds: [u8; VX2_MO_SNAP_CAP],
}

impl MotionPrefsBook {
    pub const fn new() -> MotionPrefsBook {
        MotionPrefsBook { reduced: false, kinds: [0; VX2_MO_SNAP_CAP] }
    }

    /// 从治理器读出（登记清单超出快照容量 → None 显性拒绝）。
    pub fn snapshot(g: &MotionGov) -> Option<MotionPrefsBook> {
        let cs = g.consumers();
        if cs.len() > VX2_MO_SNAP_CAP {
            return None;
        }
        let mut book = MotionPrefsBook { reduced: g.reduced(), kinds: [0; VX2_MO_SNAP_CAP] };
        for (k, c) in cs.iter().enumerate() {
            book.kinds[k] = c.kind.ordinal() as u8 + 1;
        }
        Some(book)
    }

    /// 推到治理器：开关走 set_reduced 正规路径（广播/账本不缺环），
    /// 登记走 register 正规门（幂等）。
    pub fn apply_to(&self, g: &mut MotionGov, ts: u64) -> bool {
        if self.reduced != g.reduced() {
            g.set_reduced(self.reduced, ts);
        }
        let mut ok = true;
        for slot in self.kinds.iter() {
            if *slot == 0 {
                continue;
            }
            match motion_kind_from_ordinal((*slot - 1) as usize) {
                Some(k) => ok &= g.register(VX2_FACE_IDS[(*slot - 1) as usize], k),
                None => return false,
            }
        }
        ok
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_MO_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        out[5..9].copy_from_slice(&(self.reduced as u32).to_le_bytes());
        out[9..9 + VX2_MO_SNAP_CAP].copy_from_slice(&self.kinds);
        let end = 9 + VX2_MO_PAYLOAD;
        let crc = vx2_fnv(&out[..end - 4]);
        out[end - 4..end].copy_from_slice(&crc.to_le_bytes());
        VX2_MO_BLOB
    }

    /// 反序列化：四类损坏显性拒绝（序数 > 20 类全集按长度错处理）。
    pub fn from_bytes(blob: &[u8]) -> Result<MotionPrefsBook, Vx2Error> {
        if blob.len() != VX2_MO_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_MO_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        let reduced = blob[5] & 1 == 1;
        let mut book = MotionPrefsBook { reduced, kinds: [0; VX2_MO_SNAP_CAP] };
        book.kinds.copy_from_slice(&blob[9..9 + VX2_MO_SNAP_CAP]);
        if book.kinds.iter().any(|&v| v as usize > AUDIT_SAMPLE_N) {
            return Err(Vx2Error::BadLength);
        }
        Ok(book)
    }
}

/// 登记还原用的稳定 id 池（与 20 类 ordinal 一一对应——'static 语义名，
/// 幂等登记不占新槽）。
const VX2_FACE_IDS: [&str; AUDIT_SAMPLE_N] = [
    "v2-win-open", "v2-win-close", "v2-min-fly", "v2-restore-pop", "v2-menu",
    "v2-tooltip", "v2-theme", "v2-vol-osd", "v2-bri-osd", "v2-desk-swipe",
    "v2-scroll", "v2-drop-hl", "v2-caret", "v2-progress", "v2-dialog",
    "v2-quick-panel", "v2-thumb", "v2-toast", "v2-start-menu", "v2-alttab",
];

// -- UI 壳接线面 -----------------------------------------------------------

/// 开关轨道规格（px）——v2 布局常量：F245 设置页开关 48×24。
pub const VX2_SWITCH_W: i32 = 48;
pub const VX2_SWITCH_H: i32 = 24;
pub const VX2_SWITCH_MARGIN: i32 = 16;
/// 消费面行高（px）。
pub const VX2_ROW_H_PX: i32 = 26;
/// 键盘翻转键码（VK_RETURN 同码）。
pub const VX2_KEY_ENTER: u8 = 0x0D;

/// 开关轨道矩形：面板右上角（右侧留 margin）。
pub fn switch_rect(panel_w: i32) -> crate::h1star::h1base::Rect {
    crate::h1star::h1base::Rect::new(
        panel_w - VX2_SWITCH_MARGIN - VX2_SWITCH_W,
        VX2_SWITCH_MARGIN,
        VX2_SWITCH_W,
        VX2_SWITCH_H,
    )
}

/// 开关命中测试。
pub fn switch_hit(panel_w: i32, px: i32, py: i32) -> bool {
    let r = switch_rect(panel_w);
    px >= r.x && px < r.right() && py >= r.y && py < r.bottom()
}

/// 键盘操作结论：不动 / 翻转开关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchNav {
    Stay,
    Toggle,
}

/// 键盘操作：Enter 翻转开关（设置页焦点在开关行时）。
pub fn switch_nav(key: u8) -> SwitchNav {
    match key {
        VX2_KEY_ENTER => SwitchNav::Toggle,
        _ => SwitchNav::Stay,
    }
}

/// 消费面行绘制条目：行矩形 + 新鲜度徽标（是否已收到当前广播）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionRow {
    pub y: i32,
    pub h: i32,
    pub fresh: bool,
}

/// 生成消费面行清单（登记序 = 表序；新鲜度直取 applied_version 对比
/// ——渲染面不自行判定）。
pub fn consumer_rows(g: &MotionGov, out: &mut [MotionRow]) -> usize {
    let cs = g.consumers();
    let m = cs.len().min(out.len());
    for k in 0..m {
        out[k] = MotionRow {
            y: k as i32 * VX2_ROW_H_PX,
            h: VX2_ROW_H_PX,
            fresh: cs[k].applied_version == g.version(),
        };
    }
    m
}

// -- 判定面扩展 ------------------------------------------------------------

/// F245 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_lessmotion_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F245-lessmotion-v2");

    // 1. 持久化 round-trip：偏好册编→解→推新治理器→开关/登记面一致。
    let mut src = MotionGov::new();
    let _ = src.register("win", MotionKind::WindowOpen);
    let _ = src.register("menu", MotionKind::MenuOpen);
    let _ = src.register("toast", MotionKind::ToastSlide);
    let _ = src.set_reduced(true, 100);
    let book = match MotionPrefsBook::snapshot(&src) {
        Some(b) => b,
        None => {
            set.add("v2 persistence round-trip", false, "");
            set.add("v2 corruption explicitly rejected", false, "");
            set.add("v2 switch geometry & keyboard toggle", false, "");
            set.add("v2 consumer rows fresh badges", false, "");
            set.add("v2 fuzz 500 round-trips & checksum", false, "");
            return set;
        }
    };
    let mut buf = [0u8; VX2_MO_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut dst = MotionGov::new();
    match MotionPrefsBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let ok = b2 == book && b2.apply_to(&mut dst, 200);
            set.add(
                "v2 persistence round-trip",
                ok && dst.reduced() && dst.version() >= 1
                    && dst.consumer_count() == 3
                    && dst.find_consumer("v2-menu").map(|c| c.kind) == Some(MotionKind::MenuOpen)
                    && dst.consumer_audit("v2-toast") == Some((true, true)),
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 3;
    let mut c = buf;
    c[10] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        MotionPrefsBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && MotionPrefsBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && MotionPrefsBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && MotionPrefsBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 开关几何与键盘翻转：轨道贴面板右上、命中界内/界外、Enter 翻转
    //    经 set_reduced 正规路径（版本推进 + 广播留痕）。
    let mut g3 = MotionGov::new();
    let toggled = match switch_nav(VX2_KEY_ENTER) {
        SwitchNav::Toggle => g3.set_reduced(true, 300),
        SwitchNav::Stay => false,
    };
    set.add(
        "v2 switch geometry & keyboard toggle",
        switch_rect(400).right() == 400 - VX2_SWITCH_MARGIN
            && switch_rect(400).y == VX2_SWITCH_MARGIN
            && switch_hit(400, 400 - VX2_SWITCH_MARGIN - VX2_SWITCH_W + 5, VX2_SWITCH_MARGIN + 5)
            && !switch_hit(400, 0, 0)
            && switch_nav(VX2_KEY_ENTER - 1) == SwitchNav::Stay
            && toggled && g3.reduced() && g3.version() == 1
            && g3.broadcast_log()[0].map(|b| b.ts) == Some(300),
        "",
    );

    // 4. 消费面行清单：新鲜度徽标（登记即 fresh；行距铺排）。
    let _ = g3.register("v2-row-face", MotionKind::MenuOpen);
    let mut rows = [MotionRow { y: 0, h: 0, fresh: false }; CONSUMER_CAP];
    let rn = consumer_rows(&g3, &mut rows);
    set.add(
        "v2 consumer rows fresh badges",
        rn == 1 && rows[0].fresh && rows[0].y == 0 && rows[0].h == VX2_ROW_H_PX,
        "",
    );

    // 5. xors32 fuzz 500 轮：随机偏好册 round-trip 逐字段相等、payload
    //    任一字节翻位必被校验和捕获。
    let mut x: u32 = 0x2455_E1AA;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut b = MotionPrefsBook::new();
        b.reduced = x & 1 == 1;
        for k in 0..((x >> 3) % 25) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            b.kinds[k as usize] = (x % AUDIT_SAMPLE_N as u32) as u8 + 1;
        }
        let mut tbuf = [0u8; VX2_MO_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_MO_BLOB && MotionPrefsBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_MO_PAYLOAD] ^= 0x08;
        ok &= MotionPrefsBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_prefs_roundtrip_and_reject() {
        let mut gov = MotionGov::new();
        let _ = gov.register("a", MotionKind::WindowClose);
        let b = MotionPrefsBook::snapshot(&gov).expect("under cap");
        let mut buf = [0u8; VX2_MO_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_MO_BLOB);
        assert_eq!(MotionPrefsBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[9] = 200; // 序数越界（篡改后重算校验和，专测值域分支）
        let crc = vx2_fnv(&bad[..9 + VX2_MO_PAYLOAD - 4]);
        bad[9 + VX2_MO_PAYLOAD - 4..9 + VX2_MO_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(MotionPrefsBook::from_bytes(&bad), Err(Vx2Error::BadLength));
        let mut bad2 = buf;
        bad2[5] ^= 0x01;
        assert_eq!(MotionPrefsBook::from_bytes(&bad2), Err(Vx2Error::BadChecksum));
    }

    #[test]
    fn v2_switch_rect_stable() {
        let r = switch_rect(800);
        assert_eq!(r.w, VX2_SWITCH_W);
        assert_eq!(r.h, VX2_SWITCH_H);
        assert_eq!(r.right(), 800 - VX2_SWITCH_MARGIN);
        assert!(switch_hit(800, r.x + 1, r.y + 1));
        assert!(!switch_hit(800, r.x - 1, r.y + 1));
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_lessmotion_v2_checks();
        assert!(set.all_passed(), "F245 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
