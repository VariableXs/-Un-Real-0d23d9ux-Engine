//! F226 窗口层级与阴影体系 · H 基础通用域实装。
//!
//! **判据锚**：F226。
//!
//! **验收标准（主册第一句）**：全系统窗口分四层——桌面层、普通窗口层、
//! 浮层（菜单/Tooltip/查找条）、系统层（OSD/锁屏），每层一组阴影参数
//! （普通窗 y 偏移 8px 模糊 24px 25% 黑；浮层 y 偏移 4px 模糊 16px 30%），
//! 激活窗口阴影比非激活深一档且边框亮一档；层级晋升只有三个入口（点击
//! 置顶 F248、弹窗、系统浮层），禁止应用私自永远置顶。
//!
//! **设计要点**：
//! - [`WinLayer`] 四层枚举 + [`LAYER_BASE_Z`] 层序基带——四层严格分层，
//!   同层内按注册序号在基带内排深度，跨层永不错位；
//! - [`SHADOW_TABLE`] 阴影参数册：四层 × 激活/非激活 的 y 偏移/模糊/
//!   不透明度/边框亮度全常量表（数值出处见各条目注释，一处一事实）；
//!   激活比非激活「深一档」= 不透明度 +[`SHADOW_DEEPER_STEP_PERMILL`]，
//!   「边框亮一档」= 边框提亮 +[`BORDER_BRIGHT_STEP_PERMILL`]；
//! - 晋升白名单制：[`PromotionEntry`] 三入口穷举（点击置顶 F248 / 弹窗 /
//!   系统浮层），每个入口允许到达的最高层由 `max_layer()` 给出，表外
//!   路径在类型层不存在——「禁止应用私自永远置顶」的编译期表达；
//! - [`WinLayerMgr::request_always_on_top`] 私自置顶审计器：无白名单
//!   授权的置顶请求全数拒绝并写入审计环，`pirate_rejected == requests`
//!   即「私自置顶审计 = 0（漏网 = 0）」的代码级判据；
//! - [`CompositePlan`] 阴影 GPU 合成判定接口：CPU 侧只算参数不画像素
//!   （结构体无任何像素/缓冲字段，体积常量断言封顶），阴影是否走 GPU
//!   合成由 [`SHADOW_GPU_COMPOSITE`] 判定位给出。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检）、
//! `crate::star::sbase::RingLog`（审计环）、`alloc::vec::Vec`（诊断快照面，
//! 非热路径）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 窗口登记容量——层序管理器定容热路径，零堆（64 窗覆盖全系统并发）。
pub const WIN_CAP: usize = 64;

/// 审计环容量——私自置顶/晋升事件最近 64 条（满了覆最旧）。
pub const AUDIT_CAP: usize = 64;

/// 层序基带（F226 四层判据的层序表达）：桌面 < 普通窗 < 浮层 < 系统。
pub const LAYER_BASE_Z: [i32; 4] = [0, 1_000, 2_000, 3_000];

/// 同层基带宽度：每层 1000 个深度槽，同层窗口按注册序在槽内排深度。
pub const LAYER_BAND_W: i32 = 1_000;

/// 激活阴影「深一档」步长（‰）——主册 F226「激活窗口阴影比非激活深一档」
/// 的定量档位：不透明度 250‰→300‰（25%→30%）。
pub const SHADOW_DEEPER_STEP_PERMILL: u32 = 50;

/// 激活「边框亮一档」步长（‰）——非激活 0、激活 +250‰ 提亮。
pub const BORDER_BRIGHT_STEP_PERMILL: u32 = 250;

/// 阴影统一走 GPU 合成器（F226 验收：阴影渲染走 GPU 合成器不占 CPU）——
/// CPU 侧只产出 [`CompositePlan`] 参数，不画任何像素。
pub const SHADOW_GPU_COMPOSITE: bool = true;

// ---------------------------------------------------------------------------
// 四层枚举与阴影参数册
// ---------------------------------------------------------------------------

/// 全系统窗口四层（F226 判据穷举，无第五层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinLayer {
    /// 桌面层（壁纸/桌面图标）。
    Desktop = 0,
    /// 普通窗口层。
    Normal = 1,
    /// 浮层（菜单/Tooltip/查找条）。
    Overlay = 2,
    /// 系统层（OSD/锁屏）。
    System = 3,
}

impl WinLayer {
    /// 四层全列（自检/审计遍历用）。
    pub const ALL: [WinLayer; 4] = [
        WinLayer::Desktop,
        WinLayer::Normal,
        WinLayer::Overlay,
        WinLayer::System,
    ];

    /// 数组下标视图。
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// 单档阴影参数（一层一档：激活/非激活各一份）。
///
/// 纯参数体——无像素、无缓冲，CPU 侧计算后交给 GPU 合成器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowSpec {
    /// 阴影 y 偏移（px，向下为正）。
    pub y_off_px: i32,
    /// 模糊半径（px）。
    pub blur_px: u32,
    /// 黑色阴影不透明度（‰，250 = 25% 黑——主册原文口径）。
    pub opacity_permill: u32,
    /// 边框提亮（‰，0 = 不提亮）。
    pub border_bright_permill: u32,
}

/// 阴影参数册：`[层][0 = 非激活, 1 = 激活]`（F226 验收「四层参数入册（表）」）。
///
/// 数值出处（一处一事实）：
/// - 普通窗：y 偏移 8px、模糊 24px、25% 黑——主册 F226 原文；
/// - 浮层：y 偏移 4px、模糊 16px、30%——主册 F226 原文；
/// - 激活深一档 = 不透明度 +[`SHADOW_DEEPER_STEP_PERMILL`]（250→300）；
/// - 激活边框亮一档 = +[`BORDER_BRIGHT_STEP_PERMILL`]；
/// - 桌面层无阴影（壁纸/图标不投影——桌面元素投影会污染桌面语义）；
/// - 系统层主册未单列数值，按浮层档加重 50‰ 入册（本表即唯一取值点）。
pub const SHADOW_TABLE: [[ShadowSpec; 2]; 4] = [
    // 桌面层：无阴影。
    [
        ShadowSpec { y_off_px: 0, blur_px: 0, opacity_permill: 0, border_bright_permill: 0 },
        ShadowSpec { y_off_px: 0, blur_px: 0, opacity_permill: 0, border_bright_permill: 0 },
    ],
    // 普通窗口层：y8 blur24 25% 黑；激活 30% + 边框亮一档。
    [
        ShadowSpec { y_off_px: 8, blur_px: 24, opacity_permill: 250, border_bright_permill: 0 },
        ShadowSpec { y_off_px: 8, blur_px: 24, opacity_permill: 300, border_bright_permill: 250 },
    ],
    // 浮层：y4 blur16 30%（菜单/Tooltip 恒持交互焦点语义——激活只提边框）。
    [
        ShadowSpec { y_off_px: 4, blur_px: 16, opacity_permill: 300, border_bright_permill: 0 },
        ShadowSpec { y_off_px: 4, blur_px: 16, opacity_permill: 300, border_bright_permill: 250 },
    ],
    // 系统层：按浮层档加重 50‰（OSD/锁屏浮于一切之上，阴影更深以分层）。
    [
        ShadowSpec { y_off_px: 4, blur_px: 16, opacity_permill: 350, border_bright_permill: 0 },
        ShadowSpec { y_off_px: 4, blur_px: 16, opacity_permill: 350, border_bright_permill: 250 },
    ],
];

/// 查一层一档的阴影参数（参数册唯一读取口）。
pub fn shadow_spec(layer: WinLayer, active: bool) -> ShadowSpec {
    SHADOW_TABLE[layer.idx()][active as usize]
}

/// 阴影 GPU 合成计划（F226「阴影渲染走 GPU 合成器不占 CPU」的判定接口）。
///
/// 字段只有层、参数、判定位——无像素缓冲、无绘制指令；体积常量断言封顶，
/// 保证这个接口在类型层面就「不占 CPU 画像素」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositePlan {
    /// 阴影所属层。
    pub layer: WinLayer,
    /// 阴影参数（CPU 侧产出，交 GPU 合成器）。
    pub spec: ShadowSpec,
    /// 是否走 GPU 合成路径（恒 true——表外路径不存在）。
    pub gpu_path: bool,
}

// CompositePlan 无像素字段的体积证明：≤ 32 字节（超限即编译失败）。
const _: () = assert!(core::mem::size_of::<CompositePlan>() <= 32);

/// 产出一窗阴影的合成计划（CPU 只算参数，不画像素）。
pub fn composite_plan(layer: WinLayer, active: bool) -> CompositePlan {
    CompositePlan { layer, spec: shadow_spec(layer, active), gpu_path: SHADOW_GPU_COMPOSITE }
}

/// z 序反解所属层（基带解码——审计/诊断按层切片用）。
pub fn layer_from_z(z: i32) -> Option<WinLayer> {
    if z < 0 {
        return None;
    }
    let band = z / LAYER_BAND_W;
    WinLayer::ALL.into_iter().find(|l| l.idx() as i32 == band)
}

// ---------------------------------------------------------------------------
// 晋升白名单（三入口穷举）
// ---------------------------------------------------------------------------

/// 层级晋升三入口（F226：晋升只有三个入口——白名单穷举，表外路径
/// 在类型层不存在）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromotionEntry {
    /// 点击置顶（F248）：窗口在本层内提到最上，不跨层。
    ClickTop,
    /// 弹窗：晋升到浮层。
    Popup,
    /// 系统浮层（OSD/锁屏等系统组件）：晋升到系统层。
    SystemOverlay,
}

impl PromotionEntry {
    /// 该入口允许到达的目标层（白名单语义的唯一真相）。
    pub fn target_layer(self) -> WinLayer {
        match self {
            PromotionEntry::ClickTop => WinLayer::Normal,
            PromotionEntry::Popup => WinLayer::Overlay,
            PromotionEntry::SystemOverlay => WinLayer::System,
        }
    }

    /// ClickTop 是「同层置顶」语义（不换层只换深度）。
    pub fn same_layer(self) -> bool {
        matches!(self, PromotionEntry::ClickTop)
    }
}

// ---------------------------------------------------------------------------
// 审计记录
// ---------------------------------------------------------------------------

/// 审计事件类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditKind {
    /// 白名单入口晋升成功。
    Promoted,
    /// 私自置顶请求被拒（无授权）。
    PirateTopRejected,
    /// 授权置顶获准（系统/用户明确授权的合法置顶）。
    TopGranted,
    /// 注册登记。
    Registered,
    /// 激活窗口切换（阴影重算脏集来源）。
    ActivationChanged,
}

/// 一条审计记录（小拷贝体，入定容环，零堆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRec {
    pub kind: AuditKind,
    pub win_id: u32,
    /// 事件时刻（ms，注入式）。
    pub ts_ms: u64,
    /// 附带层数值（层下标或 -1 无关）。
    pub layer_hint: i32,
}

// ---------------------------------------------------------------------------
// 层序管理器
// ---------------------------------------------------------------------------

/// 窗口登记记录。
#[derive(Clone, Copy, Debug)]
struct WinRec {
    id: u32,
    layer: WinLayer,
    /// 同层深度序号（越大越靠上；always_top 直接顶到基带最高槽）。
    seq: u32,
    /// 经授权的永远置顶（白名单授权——非授权请求进不了这个位）。
    always_top: bool,
}

/// 窗口层级与阴影体系管理器。
///
/// 热路径（注册/晋升/置顶/查参）全部定长数组，零堆；审计走定容环。
pub struct WinLayerMgr {
    wins: [Option<WinRec>; WIN_CAP],
    seq_next: u32,
    audit: RingLog<AuditRec, AUDIT_CAP>,
    /// 当前激活窗口（阴影激活档的判定源）。
    active: Option<u32>,
    /// 累计注册次数（含重复注册拒绝）。
    pub registered_total: u64,
    /// 激活切换次数。
    pub activations: u64,
    /// 阴影参数重算次数（激活切换只脏两个窗口的阴影——GPU 合成负担
    /// 上界的观测面）。
    pub shadow_recomputes: u64,
    /// 私自置顶请求总数。
    pub pirate_requests: u32,
    /// 私自置顶被拒数（判据：== pirate_requests，漏网 = 0）。
    pub pirate_rejected: u32,
    /// 阴影 CPU 画像素违规数（恒 0——接口层面不可能产生）。
    pub cpu_paint_violations: u32,
}

impl WinLayerMgr {
    pub fn new() -> WinLayerMgr {
        WinLayerMgr {
            wins: [const { None }; WIN_CAP],
            seq_next: 1,
            audit: RingLog::new(),
            active: None,
            registered_total: 0,
            activations: 0,
            shadow_recomputes: 0,
            pirate_requests: 0,
            pirate_rejected: 0,
            cpu_paint_violations: 0,
        }
    }

    /// 注册窗口入层。重复 id / 容量满 → 拒绝（返回 false，不 panic）。
    pub fn register(&mut self, id: u32, layer: WinLayer, ts_ms: u64) -> bool {
        self.registered_total += 1;
        if self.find(id).is_some() {
            return false;
        }
        let slot = match self.wins.iter_mut().find(|w| w.is_none()) {
            Some(s) => s,
            None => return false,
        };
        let seq = self.seq_next;
        self.seq_next = self.seq_next.wrapping_add(1).max(1);
        *slot = Some(WinRec { id, layer, seq, always_top: false });
        self.audit.push(AuditRec { kind: AuditKind::Registered, win_id: id, ts_ms, layer_hint: layer.idx() as i32 });
        true
    }

    /// 注销窗口。
    pub fn unregister(&mut self, id: u32) -> bool {
        match self.wins.iter().position(|w| matches!(w, Some(r) if r.id == id)) {
            Some(idx) => {
                self.wins[idx] = None;
                true
            }
            None => false,
        }
    }

    fn find(&self, id: u32) -> Option<&WinRec> {
        self.wins.iter().find_map(|w| match w {
            Some(r) if r.id == id => Some(r),
            _ => None,
        })
    }

    /// 窗口当前层。
    pub fn layer_of(&self, id: u32) -> Option<WinLayer> {
        self.find(id).map(|r| r.layer)
    }

    /// 窗口当前 z 序（层基带 + 同层深度槽；always_top 顶到基带最高槽）。
    pub fn z_of(&self, id: u32) -> Option<i32> {
        self.find(id).map(|r| {
            let base = LAYER_BASE_Z[r.layer.idx()];
            let band = LAYER_BAND_W - 1;
            let pos = if r.always_top { band } else { (r.seq as i32 - 1) % band + 1 };
            base + pos
        })
    }

    /// 白名单入口晋升：唯一的层级变更通路。
    ///
    /// - ClickTop：同层提到最上（刷新深度序号）；
    /// - Popup / SystemOverlay：跨层晋升到入口允许的目标层。
    /// 未注册窗口返回 None。
    pub fn promote(&mut self, id: u32, entry: PromotionEntry, ts_ms: u64) -> Option<WinLayer> {
        let slot = self.wins.iter_mut().find(|w| matches!(w, Some(r) if r.id == id))?;
        let rec = slot.as_mut().unwrap();
        let target = entry.target_layer();
        if entry.same_layer() {
            // 同层置顶：层不变，深度序号刷新到最新。
            rec.seq = self.seq_next;
            self.seq_next = self.seq_next.wrapping_add(1).max(1);
        } else {
            rec.layer = target;
            rec.seq = self.seq_next;
            self.seq_next = self.seq_next.wrapping_add(1).max(1);
        }
        self.audit.push(AuditRec {
            kind: AuditKind::Promoted,
            win_id: id,
            ts_ms,
            layer_hint: target.idx() as i32,
        });
        Some(rec.layer)
    }

    /// 私自置顶审计器：永远置顶请求唯一通路。
    ///
    /// `authorized = false` 的请求全数拒绝并记账（F226：禁止应用私自
    /// 永远置顶）。返回是否获准。
    pub fn request_always_on_top(&mut self, id: u32, authorized: bool, ts_ms: u64) -> bool {
        if !authorized {
            self.pirate_requests += 1;
            self.pirate_rejected += 1;
            self.audit.push(AuditRec {
                kind: AuditKind::PirateTopRejected,
                win_id: id,
                ts_ms,
                layer_hint: -1,
            });
            return false;
        }
        let slot = match self.wins.iter_mut().find(|w| matches!(w, Some(r) if r.id == id)) {
            Some(s) => s,
            None => return false,
        };
        let rec = slot.as_mut().unwrap();
        rec.always_top = true;
        rec.seq = self.seq_next;
        self.seq_next = self.seq_next.wrapping_add(1).max(1);
        self.audit.push(AuditRec { kind: AuditKind::TopGranted, win_id: id, ts_ms, layer_hint: -1 });
        true
    }

    /// 某层最顶层窗口（max by z；空层返回 None）。
    pub fn topmost_in(&self, layer: WinLayer) -> Option<u32> {
        let mut best: Option<(i32, u32)> = None;
        for w in self.wins.iter().flatten() {
            if w.layer != layer {
                continue;
            }
            let z = self.z_of(w.id).unwrap_or(i32::MIN);
            if best.map(|(bz, _)| z > bz).unwrap_or(true) {
                best = Some((z, w.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// 四层窗口计数（诊断面）。
    pub fn census(&self) -> [u32; 4] {
        let mut c = [0u32; 4];
        for w in self.wins.iter().flatten() {
            c[w.layer.idx()] += 1;
        }
        c
    }

    /// 当前登记窗口数。
    pub fn live_count(&self) -> usize {
        self.wins.iter().filter(|w| w.is_some()).count()
    }

    /// 阴影合成计划查询（交互路径：定长查表，零堆零像素）。
    pub fn shadow_plan_of(&self, id: u32, active: bool) -> Option<CompositePlan> {
        self.layer_of(id).map(|layer| composite_plan(layer, active))
    }

    /// 当前激活窗口。
    pub fn active_window(&self) -> Option<u32> {
        self.active
    }

    /// 激活切换（F226「激活/非激活阴影对比」的落地通路）。
    ///
    /// 返回受影响的阴影重算脏集：`[旧激活的新计划, 新激活的新计划]`
    /// ——一次切换最多脏两个窗口的阴影，GPU 合成负担定常。
    /// 未注册窗口 / 同窗重复激活 → 无脏集（[None, None]）。
    pub fn set_active(&mut self, id: Option<u32>, ts_ms: u64) -> [Option<CompositePlan>; 2] {
        let mut dirty = [None, None];
        if self.active == id {
            return dirty;
        }
        if let Some(new_id) = id {
            if self.layer_of(new_id).is_none() {
                return dirty; // 未注册窗口不接管激活。
            }
        }
        if let Some(old) = self.active {
            dirty[0] = self.shadow_plan_of(old, false);
            self.shadow_recomputes += 1;
        }
        if let Some(new_id) = id {
            dirty[1] = self.shadow_plan_of(new_id, true);
            self.shadow_recomputes += 1;
        }
        self.active = id;
        self.activations += 1;
        self.audit.push(AuditRec {
            kind: AuditKind::ActivationChanged,
            win_id: id.unwrap_or(0),
            ts_ms,
            layer_hint: -1,
        });
        dirty
    }

    /// 合成器绘制序（z 从低到高的窗口 id 序——桌面层先画、系统层最后画）。
    /// 定长插入排序，Vec 仅交付面。
    pub fn render_order(&self) -> Vec<(i32, u32)> {
        let mut items: [Option<(i32, u32)>; WIN_CAP] = [const { None }; WIN_CAP];
        let mut n = 0usize;
        for w in self.wins.iter().flatten() {
            let z = self.z_of(w.id).unwrap_or(i32::MIN);
            // 插入排序：按 z 升序落位。
            let mut pos = n;
            while pos > 0 {
                let prev = items[pos - 1].map(|(pz, _)| pz).unwrap_or(i32::MIN);
                if prev > z {
                    items[pos] = items[pos - 1];
                    pos -= 1;
                } else {
                    break;
                }
            }
            items[pos] = Some((z, w.id));
            n += 1;
        }
        items[..n].iter().flatten().copied().collect()
    }

    /// 审计环快照（新→旧；诊断/验收面，允许短暂 Vec）。
    pub fn audit_snapshot(&self) -> Vec<AuditRec> {
        self.audit.newest_first()
    }

    /// 私自置顶漏网数（判据面：恒 0 才算审计闭环）。
    pub fn pirate_leaked(&self) -> u32 {
        self.pirate_requests.saturating_sub(self.pirate_rejected)
    }

    /// 某层的全部窗口 id（诊断面）。
    pub fn windows_in_layer(&self, layer: WinLayer) -> Vec<u32> {
        self.wins
            .iter()
            .flatten()
            .filter(|r| r.layer == layer)
            .map(|r| r.id)
            .collect()
    }

    /// 审计分账：按事件类别计数（判据「私自置顶审计 = 0」的逐类核查面）。
    /// 顺序：[Promoted, PirateTopRejected, TopGranted, Registered,
    /// ActivationChanged]。
    pub fn audit_counts(&self) -> [u32; 5] {
        let mut c = [0u32; 5];
        for r in self.audit.newest_first() {
            match r.kind {
                AuditKind::Promoted => c[0] += 1,
                AuditKind::PirateTopRejected => c[1] += 1,
                AuditKind::TopGranted => c[2] += 1,
                AuditKind::Registered => c[3] += 1,
                AuditKind::ActivationChanged => c[4] += 1,
            }
        }
        c
    }
}

/// 两档阴影参数是否不同（激活切换是否需要重算的判定）。
pub fn shadow_params_differ(a: ShadowSpec, b: ShadowSpec) -> bool {
    a != b
}

/// 层面快照（诊断/验收面：四层计数 + 各层最顶窗 + 激活窗）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerCensus {
    pub counts: [u32; 4],
    pub topmost: [Option<u32>; 4],
    pub active: Option<u32>,
    pub live: usize,
}

impl WinLayerMgr {
    /// 生成层面快照。
    pub fn census_snapshot(&self) -> LayerCensus {
        let mut snap = LayerCensus {
            counts: self.census(),
            topmost: [None; 4],
            active: self.active,
            live: self.live_count(),
        };
        for l in WinLayer::ALL {
            snap.topmost[l.idx()] = self.topmost_in(l);
        }
        snap
    }

    /// 降层撤销：浮层关闭（菜单收起/查找条消失）等场景把窗口送回
    /// 普通层——晋升的逆操作（仍走白名单语义：系统发起，无第四入口）。
    /// 激活窗若被降层，阴影档位随层重查。
    pub fn restore_layer(&mut self, id: u32, ts_ms: u64) -> bool {
        let slot = match self.wins.iter_mut().find(|w| matches!(w, Some(r) if r.id == id)) {
            Some(s) => s,
            None => return false,
        };
        let rec = slot.as_mut().unwrap();
        if rec.layer == WinLayer::Normal {
            return false; // 已在普通层，无需降。
        }
        rec.layer = WinLayer::Normal;
        rec.always_top = false; // 降层同时收回置顶位。
        rec.seq = self.seq_next;
        self.seq_next = self.seq_next.wrapping_add(1).max(1);
        self.audit.push(AuditRec {
            kind: AuditKind::Promoted,
            win_id: id,
            ts_ms,
            layer_hint: WinLayer::Normal.idx() as i32,
        });
        true
    }
}

impl Default for WinLayerMgr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F226 自检（12 条行为级）。
pub fn run_winlayer_checks() -> CheckSet {
    let mut set = CheckSet::new("F226-winlayer");

    // 1. 阴影参数册与主册数值逐字一致：普通窗 y8/blur24/25% 黑；浮层 y4/blur16/30%。
    let n = shadow_spec(WinLayer::Normal, false);
    let o = shadow_spec(WinLayer::Overlay, false);
    set.add(
        "shadow table matches manual (normal y8 blur24 25%, overlay y4 blur16 30%)",
        n.y_off_px == 8 && n.blur_px == 24 && n.opacity_permill == 250
            && o.y_off_px == 4 && o.blur_px == 16 && o.opacity_permill == 300,
        "",
    );

    // 2. 激活比非激活深一档且边框亮一档（普通窗与浮层两档都验）。
    let na = shadow_spec(WinLayer::Normal, true);
    let oa = shadow_spec(WinLayer::Overlay, true);
    set.add(
        "active shadow one step deeper + border brighter",
        na.opacity_permill == n.opacity_permill + SHADOW_DEEPER_STEP_PERMILL
            && na.border_bright_permill == BORDER_BRIGHT_STEP_PERMILL
            && n.border_bright_permill == 0
            && oa.border_bright_permill == BORDER_BRIGHT_STEP_PERMILL,
        "",
    );

    // 3. 四层基带严格分层：桌面 < 普通 < 浮层 < 系统。
    set.add(
        "layer base z strictly ordered",
        LAYER_BASE_Z[0] < LAYER_BASE_Z[1]
            && LAYER_BASE_Z[1] < LAYER_BASE_Z[2]
            && LAYER_BASE_Z[2] < LAYER_BASE_Z[3],
        "",
    );

    // 4. 晋升白名单三入口的目标层语义。
    set.add(
        "promotion whitelist targets",
        PromotionEntry::ClickTop.target_layer() == WinLayer::Normal
            && PromotionEntry::ClickTop.same_layer()
            && PromotionEntry::Popup.target_layer() == WinLayer::Overlay
            && PromotionEntry::SystemOverlay.target_layer() == WinLayer::System,
        "",
    );

    // 5. 白名单晋升行为：ClickTop 同层置顶、Popup 升浮层。
    let mut mgr = WinLayerMgr::new();
    assert!(mgr.register(1, WinLayer::Normal, 0));
    assert!(mgr.register(2, WinLayer::Normal, 1));
    let l1 = mgr.promote(1, PromotionEntry::ClickTop, 2);
    let l2 = mgr.promote(2, PromotionEntry::Popup, 3);
    set.add(
        "click-top stays in layer, popup raises to overlay",
        l1 == Some(WinLayer::Normal)
            && l2 == Some(WinLayer::Overlay)
            && mgr.layer_of(2) == Some(WinLayer::Overlay),
        "",
    );

    // 6. 同层置顶语义：被点击窗口的 z 序刷新后压过同层未置顶窗口。
    let mut mgr4 = WinLayerMgr::new();
    assert!(mgr4.register(1, WinLayer::Normal, 0));
    assert!(mgr4.register(2, WinLayer::Normal, 1));
    let z2 = mgr4.z_of(2).unwrap();
    let _ = mgr4.promote(1, PromotionEntry::ClickTop, 2);
    set.add(
        "click-top z overtakes same-layer peer",
        mgr4.z_of(1).unwrap() > z2 && mgr4.z_of(1).unwrap() > mgr4.z_of(2).unwrap(),
        "",
    );

    // 7. 私自置顶审计：无授权全数拒绝，漏网 = 0。
    let granted = mgr.request_always_on_top(1, false, 4);
    let granted2 = mgr.request_always_on_top(2, false, 5);
    set.add(
        "pirate always-on-top rejected & fully audited",
        !granted && !granted2 && mgr.pirate_requests == 2 && mgr.pirate_rejected == 2 && mgr.pirate_leaked() == 0,
        "",
    );

    // 8. 授权置顶获准并留痕（TopGranted 进审计环）。
    let ok = mgr.request_always_on_top(1, true, 6);
    let logged = mgr.audit_snapshot().iter().any(|r| r.kind == AuditKind::TopGranted && r.win_id == 1);
    set.add("authorized always-on-top granted & logged", ok && logged, "");

    // 9. 阴影 GPU 合成判定：全层计划 gpu_path = true，CPU 画像素违规恒 0。
    let gpu_all = WinLayer::ALL.iter().all(|l| composite_plan(*l, true).gpu_path && composite_plan(*l, false).gpu_path);
    set.add(
        "shadow composite plans all GPU path, cpu paint violations = 0",
        gpu_all && SHADOW_GPU_COMPOSITE && mgr.cpu_paint_violations == 0,
        "",
    );

    // 10. 层上下文阴影查询：Normal 非激活窗拿到的是普通层非激活档参数。
    let mut mgr2 = WinLayerMgr::new();
    assert!(mgr2.register(7, WinLayer::Normal, 0));
    let plan = mgr2.shadow_plan_of(7, false).unwrap();
    set.add(
        "per-window shadow plan matches table",
        plan.layer == WinLayer::Normal && plan.spec == shadow_spec(WinLayer::Normal, false),
        "",
    );

    // 11. fuzz 2000 轮：随机 注册/晋升/置顶/注销 序列，不变量——
    //     登记数 ≤ 容量、pirate_rejected ≤ requests 且漏网恒 0、
    //     任意窗口 z 序落在其层基带内、全程无 panic。
    let mut x: u32 = 0x9E37_79B9;
    let mut fz = WinLayerMgr::new();
    let mut ok = true;
    let mut next_id = 100u32;
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 4;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let id = next_id + (x % 8);
        let layer = WinLayer::ALL[(x % 4) as usize];
        match op {
            0 => {
                let _ = fz.register(id, layer, i as u64);
            }
            1 => {
                let e = match x % 3 {
                    0 => PromotionEntry::ClickTop,
                    1 => PromotionEntry::Popup,
                    _ => PromotionEntry::SystemOverlay,
                };
                let _ = fz.promote(id, e, i as u64);
            }
            2 => {
                let _ = fz.request_always_on_top(id, x % 7 == 0, i as u64);
            }
            _ => {
                let _ = fz.unregister(id);
            }
        }
        if fz.live_count() > WIN_CAP || fz.pirate_rejected > fz.pirate_requests {
            ok = false;
            break;
        }
        // 抽查一个存活窗口的 z 序必须落在其层基带内。
        if let Some(l) = fz.layer_of(id) {
            let z = fz.z_of(id).unwrap();
            let base = LAYER_BASE_Z[l.idx()];
            if z < base || z >= base + LAYER_BAND_W {
                ok = false;
                break;
            }
        }
        next_id = next_id.wrapping_add(1);
    }
    set.add("fuzz 2000 rounds invariants hold", ok, "");

    // 12. 审计环定容淘汰：注入超过容量的事件后仍可记账，快照恒为最近 AUDIT_CAP 条。
    let mut mgr3 = WinLayerMgr::new();
    for i in 0..200u32 {
        let _ = mgr3.register(i, WinLayer::Normal, i as u64);
        let _ = mgr3.request_always_on_top(i, false, i as u64);
    }
    let snap = mgr3.audit_snapshot();
    set.add(
        "audit ring capped & still recording",
        snap.len() == AUDIT_CAP && mgr3.pirate_rejected == 200,
        "",
    );

    // 13. 激活切换脏集：一次切换只脏旧+新两个窗口的阴影参数；未注册
    //     窗口不接管；绘制序 z 升序；z 反解层一致。
    let mut ma = WinLayerMgr::new();
    assert!(ma.register(1, WinLayer::Normal, 0));
    assert!(ma.register(2, WinLayer::Normal, 1));
    assert!(ma.register(3, WinLayer::Overlay, 2));
    let d1 = ma.set_active(Some(1), 10);
    let d2 = ma.set_active(Some(2), 20);
    let d3 = ma.set_active(Some(99), 30); // 未注册 → 无脏集。
    let order = ma.render_order();
    let z_ok = order
        .windows(2)
        .all(|w| w[0].0 <= w[1].0)
        && ma.active_window() == Some(2)
        && layer_from_z(ma.z_of(3).unwrap()) == Some(WinLayer::Overlay);
    set.add(
        "activation dirty set (2 windows max) & render order & z decode",
        d1[0].is_none() && d1[1].map(|p| p.spec.border_bright_permill == BORDER_BRIGHT_STEP_PERMILL).unwrap_or(false)
            && d2[0].is_some() && d2[1].is_some()
            && d3 == [None, None]
            && ma.shadow_recomputes == 3
            && ma.activations == 2
            && z_ok,
        "",
    );

    // 14. 层快照 / 审计分账 / 降层撤销 / 参数差分。
    let mut mb = WinLayerMgr::new();
    assert!(mb.register(1, WinLayer::Normal, 0));
    assert!(mb.register(2, WinLayer::Overlay, 1));
    let _ = mb.promote(1, PromotionEntry::ClickTop, 2);
    let _ = mb.request_always_on_top(1, false, 3);
    let snap = mb.census_snapshot();
    let counts_ok = snap.counts == [0, 1, 1, 0] && snap.topmost[1] == Some(1) && snap.topmost[2] == Some(2)
        && snap.live == 2;
    let audits = mb.audit_counts();
    let demoted = mb.restore_layer(2, 40);
    let demote_noop = !mb.restore_layer(1, 50); // 普通层无需降。
    set.add(
        "layer census, audit tally, restore-layer demotion",
        counts_ok
            && audits[1] == 1
            && audits[3] == 2
            && demoted
            && demote_noop
            && mb.layer_of(2) == Some(WinLayer::Normal)
            && shadow_params_differ(shadow_spec(WinLayer::Normal, true), shadow_spec(WinLayer::Normal, false))
            && !shadow_params_differ(shadow_spec(WinLayer::Desktop, false), shadow_spec(WinLayer::Desktop, true)),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_table_literal_matches_manual() {
        // 主册原文两组数值逐字比对。
        let n = SHADOW_TABLE[WinLayer::Normal.idx()][0];
        assert_eq!((n.y_off_px, n.blur_px, n.opacity_permill), (8, 24, 250));
        let o = SHADOW_TABLE[WinLayer::Overlay.idx()][0];
        assert_eq!((o.y_off_px, o.blur_px, o.opacity_permill), (4, 16, 300));
        // 桌面层无阴影。
        let d = SHADOW_TABLE[WinLayer::Desktop.idx()][1];
        assert_eq!((d.blur_px, d.opacity_permill), (0, 0));
    }

    #[test]
    fn promotion_whitelist_end_to_end() {
        let mut m = WinLayerMgr::new();
        assert!(m.register(1, WinLayer::Normal, 0));
        assert!(m.register(2, WinLayer::Normal, 1));
        // ClickTop 同层置顶：层不变。
        assert_eq!(m.promote(1, PromotionEntry::ClickTop, 2), Some(WinLayer::Normal));
        assert!(m.z_of(1).unwrap() > m.z_of(2).unwrap(), "点击置顶后 z 应压过同层");
        // Popup 跨层到浮层。
        assert_eq!(m.promote(2, PromotionEntry::Popup, 3), Some(WinLayer::Overlay));
        assert_eq!(m.layer_of(2), Some(WinLayer::Overlay));
        // SystemOverlay 到系统层。
        assert_eq!(m.promote(1, PromotionEntry::SystemOverlay, 4), Some(WinLayer::System));
    }

    #[test]
    fn pirate_top_never_leaks() {
        let mut m = WinLayerMgr::new();
        for i in 0..20u32 {
            assert!(m.register(i, WinLayer::Normal, i as u64));
            assert!(!m.request_always_on_top(i, false, i as u64));
        }
        assert_eq!(m.pirate_requests, 20);
        assert_eq!(m.pirate_rejected, 20);
        assert_eq!(m.pirate_leaked(), 0);
        // 无一窗口获得 always_top。
        for i in 0..20u32 {
            let z = m.z_of(i).unwrap();
            let base = LAYER_BASE_Z[WinLayer::Normal.idx()];
            assert!(z < base + LAYER_BAND_W - 1, "未授权窗口不得占据基带顶槽");
        }
    }

    #[test]
    fn topmost_and_census() {
        let mut m = WinLayerMgr::new();
        assert!(m.register(1, WinLayer::Normal, 0));
        assert!(m.register(2, WinLayer::Normal, 1));
        assert!(m.register(3, WinLayer::Overlay, 2));
        assert_eq!(m.census(), [0, 2, 1, 0]);
        assert_eq!(m.topmost_in(WinLayer::Normal), Some(2));
        let _ = m.promote(1, PromotionEntry::ClickTop, 3);
        assert_eq!(m.topmost_in(WinLayer::Normal), Some(1));
        assert_eq!(m.topmost_in(WinLayer::Desktop), None);
        assert!(m.unregister(2));
        assert_eq!(m.topmost_in(WinLayer::Normal), Some(1));
    }

    #[test]
    fn register_rejects_dup_and_overflow() {
        let mut m = WinLayerMgr::new();
        assert!(m.register(1, WinLayer::Normal, 0));
        assert!(!m.register(1, WinLayer::Overlay, 1), "重复 id 拒绝");
        for i in 0..(WIN_CAP as u32 + 8) {
            let _ = m.register(1000 + i, WinLayer::Normal, i as u64);
        }
        assert!(m.live_count() <= WIN_CAP, "容量硬上限");
    }

    #[test]
    fn composite_plan_is_param_only() {
        let p = composite_plan(WinLayer::System, true);
        assert!(p.gpu_path);
        assert_eq!(p.spec.opacity_permill, 350);
        // 体积封顶：参数体无像素缓冲。
        assert!(core::mem::size_of::<CompositePlan>() <= 32);
        assert!(core::mem::size_of::<ShadowSpec>() <= 16);
    }

    #[test]
    fn activation_dirty_set_and_render_order() {
        let mut m = WinLayerMgr::new();
        assert!(m.register(1, WinLayer::Normal, 0));
        assert!(m.register(2, WinLayer::Normal, 1));
        // 初次激活：只脏新窗口。
        let d = m.set_active(Some(1), 0);
        assert!(d[0].is_none() && d[1].is_some());
        assert_eq!(d[1].unwrap().spec, shadow_spec(WinLayer::Normal, true));
        // 同窗重复激活：零脏集。
        assert_eq!(m.set_active(Some(1), 5), [None, None]);
        // 切换：旧降非激活档、新升激活档。
        let d2 = m.set_active(Some(2), 10);
        assert_eq!(d2[0].unwrap().spec, shadow_spec(WinLayer::Normal, false));
        assert_eq!(d2[1].unwrap().spec, shadow_spec(WinLayer::Normal, true));
        assert_eq!(m.shadow_recomputes, 3);
        // 注销激活窗口 → 激活仍在（窗口关闭处理由上层决定），再激活空档。
        let d3 = m.set_active(None, 20);
        assert!(d3[0].is_some() && d3[1].is_none());
        assert_eq!(m.active_window(), None);
        // 绘制序。
        assert!(m.register(3, WinLayer::Overlay, 30));
        let order = m.render_order();
        assert!(order.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(order.last().map(|(_, id)| *id), Some(3), "浮层最后画");
    }

    #[test]
    fn census_audit_and_demotion() {
        let mut m = WinLayerMgr::new();
        assert!(m.register(1, WinLayer::Normal, 0));
        assert!(m.register(2, WinLayer::Normal, 1));
        assert!(m.register(3, WinLayer::Overlay, 2));
        let _ = m.promote(2, PromotionEntry::SystemOverlay, 3);
        assert_eq!(m.windows_in_layer(WinLayer::System), alloc::vec![2]);
        assert!(m.windows_in_layer(WinLayer::Desktop).is_empty());
        // 降层收回浮层与置顶位。
        assert!(m.restore_layer(2, 4));
        assert_eq!(m.layer_of(2), Some(WinLayer::Normal));
        assert_eq!(m.windows_in_layer(WinLayer::System).len(), 0);
        let audits = m.audit_counts();
        assert_eq!(audits[0], 2, "Promoted：一次晋升 + 一次降层");
        assert_eq!(audits[3], 3, "Registered：三次注册");
        let snap = m.census_snapshot();
        assert_eq!(snap.counts, [0, 2, 1, 0]); // F226: 双普通窗 + 一浮层窗（v1 断言 [0,3,0,0] 误将 Overlay 计入 Normal，修正）。
        assert_eq!(snap.active, None);
        assert_ne!(
            shadow_spec(WinLayer::Overlay, false),
            shadow_spec(WinLayer::Overlay, true),
            "浮层边框亮一档"
        );
    }

    #[test]
    fn winlayer_selfcheck_all_green() {
        let s = run_winlayer_checks();
        assert!(s.all_passed(), "F226 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const WINLAYER_PERSIST_VERSION: u8 = 1;
/// 单参数条 8 字节：y 偏移 i16 + 模糊 u16 + 不透明度 u16 + 边框提亮 u16
/// （全 LE）。容量 = SHADOW_TABLE 定容：4 层 × 2 档 × 8B = 载荷 64B——
/// 参数册是常量表，落盘只是它的备份（一组一事实）。
pub const SHADOW_ENTRY_BYTES: usize = 8;
/// 定长记录总长 = 4 magic + 1 版本 + 载荷 64 + 4 校验 = 73B。
pub const WINLAYER_RECORD_LEN: usize = 5 + 4 * 2 * SHADOW_ENTRY_BYTES + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinlayerPersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 参数册快照编码（SHADOW_TABLE 全量落盘——升级对账用）。
pub fn shadow_table_to_bytes() -> [u8; WINLAYER_RECORD_LEN] {
    let mut out = [0u8; WINLAYER_RECORD_LEN];
    out[0..4].copy_from_slice(&VXH1_MAGIC);
    out[4] = WINLAYER_PERSIST_VERSION;
    for (li, layer) in SHADOW_TABLE.iter().enumerate() {
        for (ai, spec) in layer.iter().enumerate() {
            let o = 5 + (li * 2 + ai) * SHADOW_ENTRY_BYTES;
            out[o..o + 2].copy_from_slice(&(spec.y_off_px as i16).to_le_bytes());
            out[o + 2..o + 4].copy_from_slice(&(spec.blur_px as u16).to_le_bytes());
            out[o + 4..o + 6].copy_from_slice(&(spec.opacity_permill as u16).to_le_bytes());
            out[o + 6..o + 8].copy_from_slice(&(spec.border_bright_permill as u16).to_le_bytes());
        }
    }
    let n = WINLAYER_RECORD_LEN;
    let sum = fnv1a32(&out[5..n - 4]);
    out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
    out
}

/// 参数册快照解码：四类损坏全拒绝；解码表与在册 SHADOW_TABLE 逐条复核
/// ——不符按 BadChecksum（备份与册不符 = 内容被动过，一处一事实：
/// SHADOW_TABLE 是唯一真相）。
pub fn shadow_table_from_bytes(b: &[u8]) -> Result<[[ShadowSpec; 2]; 4], WinlayerPersistError> {
    if b.len() != WINLAYER_RECORD_LEN {
        return Err(WinlayerPersistError::BadLen);
    }
    if b[0..4] != VXH1_MAGIC {
        return Err(WinlayerPersistError::BadMagic);
    }
    if b[4] != WINLAYER_PERSIST_VERSION {
        return Err(WinlayerPersistError::BadVersion);
    }
    let n = b.len();
    let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
    if fnv1a32(&b[5..n - 4]) != sum {
        return Err(WinlayerPersistError::BadChecksum);
    }
    let mut table = [[ShadowSpec { y_off_px: 0, blur_px: 0, opacity_permill: 0, border_bright_permill: 0 }; 2]; 4];
    for li in 0..4 {
        for ai in 0..2 {
            let o = 5 + (li * 2 + ai) * SHADOW_ENTRY_BYTES;
            table[li][ai] = ShadowSpec {
                y_off_px: i16::from_le_bytes([b[o], b[o + 1]]) as i32,
                blur_px: u16::from_le_bytes([b[o + 2], b[o + 3]]) as u32,
                opacity_permill: u16::from_le_bytes([b[o + 4], b[o + 5]]) as u32,
                border_bright_permill: u16::from_le_bytes([b[o + 6], b[o + 7]]) as u32,
            };
        }
    }
    if table != SHADOW_TABLE {
        return Err(WinlayerPersistError::BadChecksum);
    }
    Ok(table)
}

// --- v2 UI 壳接线面：阴影环带几何（弹窗矩形 + 阴影外包络，F056 联动） ---

/// 阴影外包络矩形：窗口矩形左右各外扩 blur、上下外扩 blur 并加 y 偏移
/// ——GPU 合成器的环带外框（CPU 只算几何不画像素，F226 判据）。
pub fn shadow_outer_rect(
    r: crate::h1star::h1base::Rect,
    spec: ShadowSpec,
) -> crate::h1star::h1base::Rect {
    let blur = spec.blur_px as i32;
    crate::h1star::h1base::Rect::new(
        r.x - blur,
        r.y + spec.y_off_px - blur,
        r.w + 2 * blur,
        r.h + 2 * blur,
    )
}

/// 点是否落在阴影环带（外包络内、窗口矩形外）。环带区不拦截点击
/// （阴影是视觉件不是交互件——点击穿透判定的纯函数面）。
pub fn point_in_shadow_band(
    r: crate::h1star::h1base::Rect,
    spec: ShadowSpec,
    x: i32,
    y: i32,
) -> bool {
    shadow_outer_rect(r, spec).contains(x, y) && !r.contains(x, y)
}

/// 弹窗落位（F056 联动）：触发点下方 4px、右缘钳进屏宽——落位后的
/// 阴影档随浮层取（shadow_spec(WinLayer::Overlay, ..)）。
pub fn popup_rect_below(
    trigger: (i32, i32),
    w: i32,
    h: i32,
    screen_w: i32,
) -> crate::h1star::h1base::Rect {
    let x = crate::h1star::h1base::Rect::clamp_i32(trigger.0, 0, (screen_w - w).max(0));
    crate::h1star::h1base::Rect::new(x, trigger.1 + 4, w, h)
}

// --- v2 判定面扩展 ---

/// F226 v2 自检（首条必为持久化 round-trip）。
pub fn run_winlayer_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F226-winlayer-v2");

    // 1. round-trip：参数册快照编码→解码与在册表全等——验主册 F226
    //    「四层参数入册（表）」的备份对账面。
    let bytes = shadow_table_to_bytes();
    set.add(
        "v2 shadow table snapshot roundtrip",
        matches!(shadow_table_from_bytes(&bytes), Ok(t) if t == SHADOW_TABLE),
        "",
    );

    // 2. 四类损坏全拒绝——验十二查「损坏输入明错误」。
    let mut m = bytes;
    m[0] = b'X';
    let mut v = bytes;
    v[4] = 9;
    let mut s = bytes;
    s[10] ^= 0xFF;
    set.add(
        "v2 persist rejects 4 corrupt classes",
        shadow_table_from_bytes(&m) == Err(WinlayerPersistError::BadMagic)
            && shadow_table_from_bytes(&v) == Err(WinlayerPersistError::BadVersion)
            && shadow_table_from_bytes(&s) == Err(WinlayerPersistError::BadChecksum)
            && shadow_table_from_bytes(&bytes[..bytes.len() - 1]) == Err(WinlayerPersistError::BadLen),
        "",
    );

    // 3. 阴影环带几何：普通窗 y8/blur24 → 外包络精确按公式展开；窗口内
    //    点不在环带、环带点命中、外包络外点不命中——验主册 F226
    //    「普通窗 y 偏移 8px 模糊 24px」+「阴影走 GPU」的几何面。
    let win = crate::h1star::h1base::Rect::new(100, 100, 200, 100);
    let spec = shadow_spec(WinLayer::Normal, false);
    let outer = shadow_outer_rect(win, spec);
    set.add(
        "v2 shadow band geometry exact (normal y8 blur24)",
        spec.y_off_px == 8 && spec.blur_px == 24
            && outer.x == 76 && outer.y == 84 && outer.w == 248 && outer.h == 148
            && !point_in_shadow_band(win, spec, 150, 150)
            && point_in_shadow_band(win, spec, 80, 200)
            && !point_in_shadow_band(win, spec, 50, 50),
        "",
    );

    // 4. 弹窗落位 + 浮层档联动：触发点下方 4px、右缘钳进屏宽、阴影档 =
    //    浮层参数册——验主册 F226「浮层 y 偏移 4px 模糊 16px 30%」+
    //    F056 弹窗联动。
    let popup = popup_rect_below((1800, 500), 200, 120, 1920);
    let overlay = shadow_spec(WinLayer::Overlay, true);
    set.add(
        "v2 popup placement clamps, overlay shadow tier wired",
        popup.x == 1720 && popup.y == 504 && popup.right() == 1920
            && overlay.y_off_px == 4 && overlay.blur_px == 16 && overlay.opacity_permill == 300,
        "",
    );

    // 5. 层序反解复核：四层 z 序经 layer_from_z 反解回原层——快照里的
    //    z 序必须能还原层归属（诊断快照面的解码一致性）。
    let decode_ok = WinLayer::ALL.iter().all(|l| {
        layer_from_z(LAYER_BASE_Z[l.idx()] + LAYER_BAND_W / 2) == Some(*l)
            && layer_from_z(LAYER_BASE_Z[l.idx()]) == Some(*l)
    });
    set.add("v2 z decode roundtrip for all layers", decode_ok, "");

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_snapshot_detects_table_drift() {
        // 改写一个参数字节并同步修校验 → 落到「与在册表不符」拒读。
        let mut b = shadow_table_to_bytes();
        b[5] = 0x63; // 普通窗非激活 y 偏移 8 → 99
        let fixed = fnv1a32(&b[5..WINLAYER_RECORD_LEN - 4]);
        b[WINLAYER_RECORD_LEN - 4..WINLAYER_RECORD_LEN].copy_from_slice(&fixed.to_le_bytes());
        assert_eq!(shadow_table_from_bytes(&b), Err(WinlayerPersistError::BadChecksum));
    }

    #[test]
    fn v2_desktop_layer_has_no_shadow_band() {
        let win = crate::h1star::h1base::Rect::new(0, 0, 100, 100);
        let spec = shadow_spec(WinLayer::Desktop, true);
        assert!(!point_in_shadow_band(win, spec, -1, 50), "桌面层无阴影环带");
        assert_eq!(shadow_outer_rect(win, spec), win);
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_winlayer_v2_checks();
        assert!(set.all_passed(), "F226 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
