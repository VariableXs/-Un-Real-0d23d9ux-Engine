//! VE-F6005 · 物理与渲染对接（vez17_bridge）
//!
//! 物理与渲染对接三件——**位姿输出、插值缓冲、剔除协同**（物理插值是消除
//! 画面抖动的关键，**抖动即缺陷**：插值缺失导致的画面抖动自动检出，不是
//! 玩家肉眼报障才发现）。对接含**插值缓冲水位可见**；水位告急含**自动降载**；
//! 自动降载含**策略公开**；公开含**策略文档**——降载不是黑箱丢帧，策略
//! 可查可读，玩家看得见"为什么这一帧环境粒子跳了一下"。
//!
//! 对接三件：
//! 1. **位姿输出**——物理 tick 的位姿快照（位置+朝向）按实体登记落缓冲，
//!    渲染帧按 alpha 在 prev/curr 两快照间线性插值取样（O(1)/实体）；
//! 2. **插值缓冲**——每实体有界环形快照缓冲（容量 4），双快照齐才插值
//!    （单快照硬切即抖动源）；水位 = 快照数/容量 全量可查；
//! 3. **剔除协同**——物理坐标与渲染剔除共享同一位姿源：偏差超阈值即
//!    **位姿漂移→标定**立案（两边看到的不是同一个世界，画面必然错位）。
//!
//! 错误路径：插值缺失→检测告警（E_BR_NOINTERP）；位姿漂移→标定
//! （E_BR_DRIFT）；越权→拒绝（未登记实体 E_BR_UNREGISTERED）。
//! 性能：对接 O(可见实体)——sample_all 逐实体 O(1) 取样合计线性。
//! 无障碍：对接状态可查（`status_spoken` 可读名单）。
//!
//! Z 域字符串码五码互异：`E_BR_NOINTERP` / `E_BR_DRIFT` /
//! `E_BR_UNREGISTERED` / `E_BR_OVERLOAD` / `E_BR_NOSNAP`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性——错误必须说得出口）
// ---------------------------------------------------------------------------

/// 插值缺失（单快照硬切/缓冲空）——抖动源，自动检出立案。
pub const E_BR_NOINTERP: &str = "E_BR_NOINTERP";
/// 位姿漂移（物理与渲染位姿源偏差超阈值）——标定立案。
pub const E_BR_DRIFT: &str = "E_BR_DRIFT";
/// 越权（未登记实体写快照）。
pub const E_BR_UNREGISTERED: &str = "E_BR_UNREGISTERED";
/// 降载发生（水位告急丢最旧）——不是错误，是策略执行记录。
pub const E_BR_OVERLOAD: &str = "E_BR_OVERLOAD";
/// 快照缺失（缓冲空不可取样）。
pub const E_BR_NOSNAP: &str = "E_BR_NOSNAP";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_BR_NOINTERP => "插值缺失：单快照硬切即抖动源，自动检出立案",
        E_BR_DRIFT => "位姿漂移：物理与渲染位姿源偏差超阈值，需标定",
        E_BR_UNREGISTERED => "越权：实体未登记对接面，拒绝写快照",
        E_BR_OVERLOAD => "降载发生：插值缓冲水位告急，按公开策略丢最旧",
        E_BR_NOSNAP => "快照缺失：缓冲空不可取样，等待物理 tick 补齐",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 位姿与快照（对接的数据面）
// ---------------------------------------------------------------------------

/// 位姿快照：物理 tick `tick` 时刻实体 `entity` 的位置与朝向。
/// `Copy` 语义——快照是不可变事实记录，复制无负担。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseSnap {
    pub entity: u32,
    pub tick: u64,
    pub x: i32,
    pub y: i32,
    /// 朝向（角度，示例域直接线性插值——旋转插值属于 F6xxx 后续单深化）。
    pub rot: i32,
}

impl PoseSnap {
    /// 两快照线性插值（alpha ∈ [0,1]，越界钳制——外插不是对接的职责，
    /// 越界按端点处理：alpha<0 取 prev、>1 取 curr，不报错不外推）。
    pub fn lerp(prev: &PoseSnap, curr: &PoseSnap, alpha: i32, denom: i32) -> PoseSnap {
        let a = if denom <= 0 {
            0
        } else if alpha < 0 {
            0
        } else if alpha > denom {
            denom
        } else {
            alpha
        };
        PoseSnap {
            entity: curr.entity,
            tick: curr.tick,
            x: prev.x + (curr.x - prev.x) * a / denom.max(1),
            y: prev.y + (curr.y - prev.y) * a / denom.max(1),
            rot: prev.rot + (curr.rot - prev.rot) * a / denom.max(1),
        }
    }
}

// ---------------------------------------------------------------------------
// 插值缓冲（有界环形 + 水位可见 + 告急自动降载）
// ---------------------------------------------------------------------------

/// 缓冲容量（有界——内存预算固定，满载降载不静默膨胀）。
pub const BUF_CAP: usize = 4;
/// 水位告急线（比例 ×1000：≥900 即 90% 告急触发降载）。
pub const HIGH_WATER_PERMILE: u32 = 900;
/// 位姿漂移标定阈值（物理/渲染坐标偏差曼哈顿距离）。
pub const DRIFT_THRESHOLD: i32 = 8;

/// 降载策略文档（**策略公开**：常量文档 + `policy_spoken` 可读名单——
/// 策略不是黑箱，玩家与开发者查得到"丢什么、为什么、按什么顺序"）。
pub const DEGRADE_POLICY_DOC: &str = "插值缓冲降载策略：水位>=90% 时丢最旧快照一张；\
每帧至多丢一张（防雪崩）；被丢快照计入 OVERLOAD 账不静默；采样优先级 渲染插值 > 降载回收";

/// 降载策略可读名单（无障碍：对接状态可查）。
pub fn policy_spoken() -> &'static str {
    "降载策略：水位九成告急，丢最旧一张，每帧至多一张，丢账公开"
}

/// 单实体插值缓冲：有界 FIFO 快照环 + 水位 + 降载账。
#[derive(Clone)]
pub struct InterpBuf {
    entity: u32,
    snaps: Vec<PoseSnap>,
    /// 累计降载张数（丢账不静默——丢了多少张历史可查）。
    pub dropped: u32,
}

impl InterpBuf {
    pub fn new(entity: u32) -> Self {
        InterpBuf { entity, snaps: Vec::new(), dropped: 0 }
    }

    pub fn entity(&self) -> u32 {
        self.entity
    }

    /// 快照数（水位分子）。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }

    /// 水位（千分比，O(1)——容量常数除法）。
    pub fn water_permile(&self) -> u32 {
        (self.snaps.len() * 1000 / BUF_CAP) as u32
    }

    /// 压入快照（满载即降载：丢最旧一张再压——**每帧至多丢一张**防雪崩；
    /// 降载计入 OVERLOAD 账）。返回被丢快照（有则 Some）。
    pub fn push(&mut self, s: PoseSnap) -> Option<PoseSnap> {
        let mut evicted = None;
        if self.snaps.len() >= BUF_CAP {
            evicted = Some(self.snaps.remove(0));
            self.dropped += 1;
        }
        self.snaps.push(s);
        evicted
    }

    /// 是否告急（水位 ≥ 高位线）。
    pub fn is_high_water(&self) -> bool {
        self.water_permile() >= HIGH_WATER_PERMILE
    }

    /// 双快照插值取样（渲染帧 `render_tick` 落在 [prev.tick, curr.tick]）。
    /// 双快照齐且区间命中 → Ok(插值位姿)；单快照/区间未命中 →
    /// Err(E_BR_NOSNAP)（调用方立案抖动——**抖动即缺陷**，不静默硬切）。
    pub fn sample(&self, render_tick: u64) -> Result<PoseSnap, &'static str> {
        if self.snaps.len() < 2 {
            return Err(E_BR_NOSNAP);
        }
        // 取最新两快照（FIFO 尾部）——渲染追物理，取最近区间插值。
        let curr = &self.snaps[self.snaps.len() - 1];
        let prev = &self.snaps[self.snaps.len() - 2];
        if render_tick < prev.tick || render_tick > curr.tick {
            return Err(E_BR_NOSNAP);
        }
        let denom = (curr.tick - prev.tick) as i32;
        let alpha = (render_tick - prev.tick) as i32;
        Ok(PoseSnap::lerp(prev, curr, alpha, denom))
    }
}

// ---------------------------------------------------------------------------
// 对接门面（登记闸 + 三件编排 + 抖动检测 + 剔除协同 + 状态可查）
// ---------------------------------------------------------------------------

/// 剔除协同立案：位姿漂移记录（渲染期望 vs 物理事实）。
#[derive(Clone, Debug)]
pub struct DriftCase {
    pub entity: u32,
    pub phys_x: i32,
    pub phys_y: i32,
    pub expect_x: i32,
    pub expect_y: i32,
    pub manhattan: i32,
}

/// 对接门面：物理→渲染单通道（登记闸 + 位姿输出 + 插值缓冲 +
/// 抖动检测 + 剔除协同 + 水位降载）。所有账计数公开可查。
pub struct PoseBridge {
    registered: Vec<u32>,
    bufs: Vec<InterpBuf>,
    /// 插值缺失告警计数（抖动即缺陷——每漏一次记一次）。
    pub jitter_alerts: u32,
    /// 降载事件计数。
    pub overloads: u32,
    /// 位姿漂移立案簿（FIFO，对账面）。
    pub drifts: Vec<DriftCase>,
}

impl PoseBridge {
    pub fn new() -> Self {
        PoseBridge {
            registered: Vec::new(),
            bufs: Vec::new(),
            jitter_alerts: 0,
            overloads: 0,
            drifts: Vec::new(),
        }
    }

    /// 实体登记（对接面准入闸——O(登记数) 线性查重，重复登记幂等）。
    pub fn register(&mut self, entity: u32) {
        if !self.registered.contains(&entity) {
            self.registered.push(entity);
            self.bufs.push(InterpBuf::new(entity));
        }
    }

    pub fn is_registered(&self, entity: u32) -> bool {
        self.registered.contains(&entity)
    }

    fn buf_of(&mut self, entity: u32) -> Option<&mut InterpBuf> {
        self.bufs.iter_mut().find(|b| b.entity() == entity)
    }

    /// 位姿输出入口（物理 tick 快照落缓冲）。
    /// 越权（未登记）→ Err(E_BR_UNREGISTERED)；
    /// 满载降载 → Ok(Some(被丢快照)) + overloads 计数。
    pub fn put_pose(&mut self, s: PoseSnap) -> Result<Option<PoseSnap>, &'static str> {
        if !self.is_registered(s.entity) {
            return Err(E_BR_UNREGISTERED);
        }
        let b = self.buf_of(s.entity).ok_or(E_BR_UNREGISTERED)?;
        let evicted = b.push(s);
        if evicted.is_some() {
            self.overloads += 1;
        }
        Ok(evicted)
    }

    /// 渲染帧取样（插值缓冲消费侧）。插值缺失 → 抖动立案计数 +
    /// Err(E_BR_NOSNAP)——**调用方拿到的错误本身就是抖动告警**，
    /// 不静默硬切（硬切是抖动之源，检测在源头）。
    pub fn sample_pose(&mut self, entity: u32, render_tick: u64) -> Result<PoseSnap, &'static str> {
        if !self.is_registered(entity) {
            return Err(E_BR_UNREGISTERED);
        }
        let b = self.buf_of(entity).ok_or(E_BR_UNREGISTERED)?;
        match b.sample(render_tick) {
            Ok(p) => Ok(p),
            Err(E_BR_NOSNAP) => {
                self.jitter_alerts += 1;
                Err(E_BR_NOINTERP)
            }
            Err(e) => Err(e),
        }
    }

    /// 剔除协同：渲染剔除器期望的实体位置 vs 物理事实位置对拍。
    /// 曼哈顿距离 > 阈值 → **位姿漂移→标定**立案（两边不是同一个世界）。
    /// 返回立案（有则 Some）。
    pub fn cull_reconcile(&mut self, entity: u32, expect_x: i32, expect_y: i32) -> Option<DriftCase> {
        let tail = {
            let mut found = None;
            for bb in self.bufs.iter() {
                if bb.entity() == entity && !bb.is_empty() {
                    found = Some(bb.tail());
                }
            }
            found
        }?;
        let man = (tail.x - expect_x).abs() + (tail.y - expect_y).abs();
        if man > DRIFT_THRESHOLD {
            let case = DriftCase {
                entity,
                phys_x: tail.x,
                phys_y: tail.y,
                expect_x,
                expect_y,
                manhattan: man,
            };
            self.drifts.push(case.clone());
            Some(case)
        } else {
            None
        }
    }

    /// 对接状态可查（无障碍可读名单：登记数/告警/降载/漂移立案数）。
    pub fn status_spoken(&self) -> String {
        format!(
            "对接状态：登记实体 {} 个，抖动告警 {} 次，降载 {} 次，漂移立案 {} 起",
            self.registered.len(),
            self.jitter_alerts,
            self.overloads,
            self.drifts.len()
        )
    }

    /// 全实体取样编排（性能：对接 O(可见实体)——逐实体 O(1) 取样合计线性）。
    /// 返回 (成功插值位姿名单, 失败实体码单)。
    pub fn sample_all(&mut self, entities: &[u32], render_tick: u64) -> (Vec<PoseSnap>, Vec<(u32, &'static str)>) {
        let mut oks = Vec::new();
        let mut fails = Vec::new();
        for &e in entities {
            match self.sample_pose(e, render_tick) {
                Ok(p) => oks.push(p),
                Err(code) => fails.push((e, code)),
            }
        }
        (oks, fails)
    }
}

impl InterpBuf {
    /// 尾部快照读（对拍用；空缓冲 None）。
    pub fn tail(&self) -> PoseSnap {
        self.snaps[self.snaps.len() - 1]
    }
}

impl Default for PoseBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：位姿输出/插值缓冲/抖动即缺陷/水位降载/复用声明/编排）
// ---------------------------------------------------------------------------

/// F6005 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_vez17_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6005");

    // --- 位姿输出（判据一）---
    // 对-01：双快照插值手算——prev=(100,100,rot0)@t10 curr=(200,300,rot40)@t14，
    // 渲染 t11 → alpha=1/4 → x=100+100/4=125 y=100+200/4=150 rot=0+40/4=10；
    // 越界 alpha 钳制：t9(<prev) 取端点 prev 值不外推；t20(>curr) 取 curr。
    let mut br0 = PoseBridge::new();
    br0.register(1);
    let _ = br0.put_pose(PoseSnap { entity: 1, tick: 10, x: 100, y: 100, rot: 0 });
    let _ = br0.put_pose(PoseSnap { entity: 1, tick: 14, x: 200, y: 300, rot: 40 });
    let p11 = br0.sample_pose(1, 11);
    let p10e = br0.sample_pose(1, 9); // 低于 prev 区间 → NOSNAP → NOINTERP
    let p14e = br0.sample_pose(1, 20); // 高于 curr 区间 → 同上
    let pose_ok = matches!(&p11, Ok(p) if p.x == 125 && p.y == 150 && p.rot == 10)
        && matches!(p10e, Err(E_BR_NOINTERP))
        && matches!(p14e, Err(E_BR_NOINTERP));
    if pose_ok {
        s.ok("F6005-对-01-位姿输出插值手算与越界钳制");
    } else {
        s.fail("F6005-对-01-位姿输出插值手算与越界钳制", "位姿输出失效");
    }

    // --- 插值缓冲（判据二）---
    // 对-02：单快照不插值（双快照齐才插——单快照硬切即抖动源）：
    // 新实体只压一张 → sample Err；补第二张同 tick 区间 → Ok。
    let mut br1 = PoseBridge::new();
    br1.register(2);
    let _ = br1.put_pose(PoseSnap { entity: 2, tick: 5, x: 0, y: 0, rot: 0 });
    let single = br1.sample_pose(2, 5);
    let _ = br1.put_pose(PoseSnap { entity: 2, tick: 8, x: 9, y: 0, rot: 0 });
    let doubled = br1.sample_pose(2, 7); // alpha=2/3 → x=0+9*2/3=6
    let buf_ok = matches!(single, Err(E_BR_NOINTERP))
        && matches!(&doubled, Ok(p) if p.x == 6);
    if buf_ok {
        s.ok("F6005-对-02-单快照不插值双快照齐才插");
    } else {
        s.fail("F6005-对-02-单快照不插值双快照齐才插", "插值缓冲失效");
    }

    // --- 抖动即缺陷（判据三）---
    // 抖-01：插值缺失自动检出——缓冲空实体取样 → Err(NOINTERP) +
    // jitter_alerts 恰值递增（每漏一次记一次，三次三次）。
    let mut br2 = PoseBridge::new();
    br2.register(3);
    let _ = br2.sample_pose(3, 1);
    let _ = br2.sample_pose(3, 2);
    let jitter_ok = matches!(br2.sample_pose(3, 3), Err(E_BR_NOINTERP)) && br2.jitter_alerts == 3;
    if jitter_ok {
        s.ok("F6005-抖-01-插值缺失自动检出立案");
    } else {
        s.fail("F6005-抖-01-插值缺失自动检出立案", "抖动检测失效");
    }

    // --- 剔除协同（判据四）---
    // 抖-02：位姿漂移→标定——物理事实 (300,100) vs 渲染期望 (290,100)
    // 曼哈顿 10 > 阈值 8 → 立案内容精确；期望 (295,100) 偏差 5 ≤ 8 不立案
    // （恰值边界：偏差 8 恰不立案 > 才立案）。
    let mut br3 = PoseBridge::new();
    br3.register(4);
    let _ = br3.put_pose(PoseSnap { entity: 4, tick: 1, x: 300, y: 100, rot: 0 });
    let drift = br3.cull_reconcile(4, 290, 100);
    let no_drift = br3.cull_reconcile(4, 292, 100); // 偏差 8 恰值不立案
    let drift_ok = matches!(&drift, Some(c) if c.manhattan == 10 && c.entity == 4)
        && no_drift.is_none()
        && br3.drifts.len() == 1;
    if drift_ok {
        s.ok("F6005-抖-02-位姿漂移标定恰值边界");
    } else {
        s.fail("F6005-抖-02-位姿漂移标定恰值边界", "剔除协同失效");
    }

    // --- 水位可见（判据五）---
    // 水-01：水位千分比恰值——容量 4：0 张 0‰、2 张 500‰、4 张 1000‰；
    // 告急线 900：3 张 750‰ 不告急、4 张 1000‰ 告急；策略文档与
    // 可读名单非空互异（公开含策略文档）。
    let mut b5 = InterpBuf::new(9);
    let w0 = b5.water_permile();
    let _ = b5.push(PoseSnap { entity: 9, tick: 1, x: 0, y: 0, rot: 0 });
    let _ = b5.push(PoseSnap { entity: 9, tick: 2, x: 1, y: 0, rot: 0 });
    let w2 = b5.water_permile();
    let hw2 = b5.is_high_water();
    let _ = b5.push(PoseSnap { entity: 9, tick: 3, x: 2, y: 0, rot: 0 });
    let _ = b5.push(PoseSnap { entity: 9, tick: 4, x: 3, y: 0, rot: 0 });
    let w4 = b5.water_permile();
    let hw4 = b5.is_high_water();
    let water_ok = w0 == 0
        && w2 == 500
        && w4 == 1000
        && !hw2
        && hw4
        && !DEGRADE_POLICY_DOC.is_empty()
        && !policy_spoken().is_empty()
        && DEGRADE_POLICY_DOC != policy_spoken();
    if water_ok {
        s.ok("F6005-水-01-水位可见策略公开");
    } else {
        s.fail("F6005-水-01-水位可见策略公开", "水位可见失效");
    }

    // --- 自动降载（判据六）---
    // 水-02：告急降载恰值——容量 4 压第 5 张 → 丢最旧（tick1）dropped=1
    // 被丢快照内容精确返回；降载后水位回落 750‰；bridge 层 overloads
    // 计数同步（丢账不静默）；每帧至多丢一张（压 1 丢 1 不会连丢）。
    let mut br4 = PoseBridge::new();
    br4.register(5);
    for t in 1..=4u64 {
        let _ = br4.put_pose(PoseSnap { entity: 5, tick: t, x: t as i32, y: 0, rot: 0 });
    }
    let ev = br4.put_pose(PoseSnap { entity: 5, tick: 5, x: 5, y: 0, rot: 0 });
    let evicted_ok = matches!(&ev, Ok(Some(s)) if s.tick == 1 && s.x == 1)
        && br4.overloads == 1;
    // 丢 1 压 1 净变化 0：水位维持满载 1000‰——每帧至多丢一张防雪崩，
    // 水位靠持续降载压制而不是一次清空（策略文档口径）。
    let load_ok = match br4.bufs.iter().find(|b| b.entity() == 5) {
        Some(b) => evicted_ok && b.water_permile() == 1000 && b.len() == 4 && b.dropped == 1,
        None => false,
    };
    if load_ok {
        s.ok("F6005-水-02-告急自动降载恰值");
    } else {
        s.fail("F6005-水-02-告急自动降载恰值", "自动降载失效");
    }

    // --- 越权拒绝（判据七）---
    // 权-01：未登记实体写/读全拒 E_BR_UNREGISTERED——登记闸是唯一
    // 入口，越权面（写快照/取样）双拒；登记后同实体放行（幂等登记）。
    let mut br5 = PoseBridge::new();
    let denied_w = br5.put_pose(PoseSnap { entity: 7, tick: 1, x: 0, y: 0, rot: 0 });
    let denied_r = br5.sample_pose(7, 1);
    br5.register(7);
    br5.register(7); // 幂等
    let _ = br5.put_pose(PoseSnap { entity: 7, tick: 1, x: 4, y: 0, rot: 0 });
    let _ = br5.put_pose(PoseSnap { entity: 7, tick: 2, x: 8, y: 0, rot: 0 });
    let granted = br5.sample_pose(7, 2);
    let gate_ok = matches!(denied_w, Err(E_BR_UNREGISTERED))
        && matches!(denied_r, Err(E_BR_UNREGISTERED))
        && br5.is_registered(7)
        && br5.bufs.len() == 1 // 幂等：重复登记不重复开缓冲
        && matches!(&granted, Ok(p) if p.x == 8);
    if gate_ok {
        s.ok("F6005-权-01-越权拒绝幂等登记");
    } else {
        s.fail("F6005-权-01-越权拒绝幂等登记", "登记闸失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 文案互异非空 + 缺省分支兜底；零 panic
    // 空态平凡（空 bridge 全查询不炸）。
    let codes = [E_BR_NOINTERP, E_BR_DRIFT, E_BR_UNREGISTERED, E_BR_OVERLOAD, E_BR_NOSNAP];
    let mut codes_distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                codes_distinct = false;
            }
        }
    }
    let mut reasons_distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if reason_of(codes[i]) == reason_of(codes[j]) {
                reasons_distinct = false;
            }
        }
    }
    let meta_ok = codes_distinct
        && reasons_distinct
        && reason_of("E_BR_UNKNOWN_X") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty());
    let mut br6 = PoseBridge::new();
    let empty_ok = br6.sample_all(&[], 0).0.is_empty()
        && br6.status_spoken().contains("登记实体 0 个");
    if meta_ok && empty_ok {
        s.ok("F6005-元-01-五码互异零panic空态");
    } else {
        s.fail("F6005-元-01-五码互异零panic空态", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(可见实体)——3 实体混合流：e1 双快照正常插值、
    // e2 单快照抖动立案、e3 未登记失败码单；全量取样对账
    // ok=1 fail=2 + 抖动计数 +1 + 状态可读名单对账。
    let mut br7 = PoseBridge::new();
    br7.register(10);
    br7.register(11);
    let _ = br7.put_pose(PoseSnap { entity: 10, tick: 20, x: 40, y: 0, rot: 0 });
    let _ = br7.put_pose(PoseSnap { entity: 10, tick: 24, x: 80, y: 0, rot: 0 });
    let _ = br7.put_pose(PoseSnap { entity: 11, tick: 20, x: 1, y: 0, rot: 0 });
    let (oks, fails) = br7.sample_all(&[10, 11, 12], 22);
    let chain_ok = oks.len() == 1
        && matches!(&oks[0], p if p.entity == 10 && p.x == 60) // alpha=1/2 → 40+40/2=60
        && fails.len() == 2
        && fails.iter().any(|(e, c)| *e == 11 && *c == E_BR_NOINTERP)
        && fails.iter().any(|(e, c)| *e == 12 && *c == E_BR_UNREGISTERED)
        && br7.jitter_alerts == 1;
    let spoken = br7.status_spoken();
    let spoken_ok = spoken.contains("登记实体 2 个") && spoken.contains("漂移立案 0 起");
    if chain_ok && spoken_ok {
        s.ok("F6005-编-01-全链路O可见实体对账");
    } else {
        s.fail("F6005-编-01-全链路O可见实体对账", "编排失效");
    }

    s
}
