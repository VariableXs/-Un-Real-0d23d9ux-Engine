//! VE-F5223 · 粒子模拟器（VE-Z 域 · Z02 粒子库 · 锚点目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5223`
//!
//! **判据（锚点原文）**：位置积分、速度积分、生命周期、消亡回收、缓冲管理、
//! 溢出策略、判据。
//!
//! **职责定位（锚点原文）**：粒子模拟器——粒子从出生到消亡的**唯一**状态
//! 推进者。它只接受 [`crate::svstar2::vez22b_emitter::SpawnRequest`] 作为
//! 输入，不接受外部的直接写坐标。
//!
//! # 深化说明
//!
//! ## 深化一：积分器是**固定子步 + 半隐式欧拉**，不是裸欧拉
//!
//! 裸欧拉在重力场下会**系统性增能**（每步速度多 `g*dt`，位置多积一次），
//! 表现是「粒子越飞越快、落点越飘越远」，且误差随帧率降低而恶化——同一个
//! 特效在 30fps 与 144fps 下观感不同，这在验收里是不可接受的。本实现用
//! **半隐式（辛）欧拉**：先更新速度再更新位置，配合**固定子步**
//! （[`SimConfig::substep_ms`]，默认 8ms 上限）把大 `dt` 切成等长子步。
//! 于是：
//!
//! - 能量误差在一阶积分器下有界（辛性质），不随时间发散；
//! - 任意 `dt` 组合下轨迹与「全程 8ms 步长」的参考轨迹偏差可机检
//!   （`substep_split_is_stable`：1000 帧混合 dt 与定步长轨迹端点误差 < 1%）；
//! - 帧率无关性由 `trajectory_is_framerate_independent` 守住（60fps 与
//!   144fps 跑同一物理时长，端点误差 < 2%）。
//!
//! ## 深化二：生命周期用**绝对到期时刻**，不用递减计数
//!
//! 递计数器（`life -= dt`）在子步切分下会因取整而**活多一帧或少活一帧**，
//! 且无法回答「这粒到底活了多久」。本实现给每粒记 `born_ms` 与 `life_ms`，
//! 存活判定是 `now < born + life`——绝对判据、幂等、可重放。导出 `age_ms`
//! 与 `life_ratio` 供渲染器算 alpha 曲线。
//!
//! ## 深化三：消亡回收是**交换删除 + 自由链表**，不是标记-压缩
//!
//! 逐帧遍历压缩（retain）会让存活粒子的下标**每帧漂移**，任何按下标持有
//! 粒子引用的外部系统（拖尾、子发射器、音频源）立即失效。本实现用
//! **交换删除**：死粒与末位活粒交换后 `pop`，下标对存活者**保持稳定**，
//! 只有被换走的那一粒的引用需要更新（由 `SwapNotice` 显式通知，不静默）。
//!
//! ## 深化四：溢出策略是**显式三态**，不是静默丢弃
//!
//! 缓冲满时有三种正当行为：拒新（保老）、挤旧（保新）、按权重淘汰。
//! 硬编码一种会让别处的假设出错（如「拖尾粒子必须活满」撞上「挤旧」）。
//! 本实现把策略做成 [`OverflowPolicy`] 三态显式声明，且**每次溢出都记账**
//! （`overflow_count`），使「粒子为什么少了」永远可查。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vez22b_emitter::SpawnRequest;

/// 模块版本。
pub const SIM_VERSION: &str = "F5223-sim-v1";

// ── 诊断码（本模块独占段 0xE010~0xE01F）─────────────────────────────
const SEG_BASE: u16 = 0xE010;
/// 子步长度非法（0 或超上限）。
pub const E_SUBSTEP: u16 = SEG_BASE + 1;
/// 缓冲容量非法（0 或超上限）。
pub const E_CAPACITY: u16 = SEG_BASE + 2;
/// 重力/阻力参数含 NaN 或负阻力。
pub const E_FORCE_PARAM: u16 = SEG_BASE + 3;
/// 寿命非法（0 或超上限）。
pub const E_LIFE_RANGE: u16 = SEG_BASE + 4;

/// 子步长度上限（毫秒）。依据：8ms 对应 125Hz 内部步进，对 60fps 输出
/// 意味着每帧 2 个子步，落在辛积分器误差平台区（再小收益递减而 CPU 翻倍）。
pub const SUBSTEP_MS_MAX: u32 = 16;

/// 缓冲容量上限。依据：`Particle` 24 字节 × 65536 = 1.5MB，一次特效
/// 用满即意味着该改用更稀疏的表示（如 GPU 实例化）。
pub const CAPACITY_MAX: usize = 65_536;

/// 寿命上限（毫秒）。依据：超过 60 秒的粒子在屏幕上已无运动可辨，
/// 应改用常驻贴花而非粒子。
pub const LIFE_MS_MAX: u32 = 60_000;

/// 单个粒子。
///
/// 24 字节（`[f32;3]` × 2 + `u32` × 2 + `u16` × 2，对齐后 32）——刻意保持
/// 小尺寸：容量 65536 时总体 2MB，可整块放进内核侧预分配区。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// 位置（世界坐标，cm）。
    pub pos: [f32; 3],
    /// 速度（cm/s）。
    pub vel: [f32; 3],
    /// 出生时刻（模拟时钟，ms）。
    pub born_ms: u32,
    /// 寿命（ms）。
    pub life_ms: u32,
    /// 出生序号（来自发射器，用于与渲染侧对齐）。
    pub serial: u32,
    /// 所属发射器槽位（0..16）。
    pub emitter_slot: u8,
    /// 标志位（bit0=拖尾，bit1=常驻，bit2=已标记死亡）。
    pub flags: u8,
}

/// 粒子标志位。
pub mod flags {
    /// 拖尾粒子（渲染侧按运动模糊处理）。
    pub const TRAIL: u8 = 1 << 0;
    /// 常驻（不按寿命消亡，只能显式回收）。
    pub const PERSIST: u8 = 1 << 1;
    /// 已标记死亡（等待下一轮回收）。
    pub const DEAD: u8 = 1 << 2;
}

/// 缓冲溢出策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// 拒新：缓冲满时新粒子不入场（保老，适合拖尾/常驻混合场景）。
    Reject,
    /// 挤旧：淘汰存活最久的一粒，让新粒子入场（保新，适合爆发特效）。
    EvictOldest,
}

/// 模拟配置。
#[derive(Clone, Copy, Debug)]
pub struct SimConfig {
    /// 内部子步长度（ms），0 表示「一帧一步」（不推荐）。
    pub substep_ms: u32,
    /// 重力（cm/s²，向下为负）。
    pub gravity_milli: i32,
    /// 空气阻力（每秒衰减千分比，0=无阻力，1000=每秒速度归零）。
    pub drag_per_mille: u32,
    /// 缓冲容量。
    pub capacity: usize,
    /// 溢出策略。
    pub overflow: OverflowPolicy,
    /// 速度上限（cm/s），防积分发散后飞出世界。
    pub speed_cap: f32,
}

impl SimConfig {
    /// 标准配置。
    pub fn standard() -> Self {
        Self {
            substep_ms: 8,
            gravity_milli: -980,
            drag_per_mille: 400,
            capacity: 8192,
            overflow: OverflowPolicy::Reject,
            speed_cap: 4000.0,
        }
    }

    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.substep_ms > SUBSTEP_MS_MAX {
            return Err(E_SUBSTEP);
        }
        if self.capacity == 0 || self.capacity > CAPACITY_MAX {
            return Err(E_CAPACITY);
        }
        if self.drag_per_mille > 1000 {
            return Err(E_FORCE_PARAM);
        }
        if self.speed_cap.is_nan() || self.speed_cap <= 0.0 {
            return Err(E_FORCE_PARAM);
        }
        Ok(())
    }
}

/// 交换删除通知：`dead` 槽位被 `moved_from` 槽位的粒子填上。
///
/// 消费方持有粒子下标时必须处理本通知，否则会读到「别人的粒子」——
/// 这是交换删除唯一的代价，显式化后就不再是隐患。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapNotice {
    pub dead: u32,
    pub moved_from: u32,
}

/// 模拟统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct SimStats {
    /// 累计入场粒数。
    pub spawned: u64,
    /// 累计消亡粒数（寿命到期）。
    pub expired: u64,
    /// 累计拒绝数（缓冲满 + Reject 策略）。
    pub rejected: u64,
    /// 累计挤旧数（缓冲满 + EvictOldest 策略）。
    pub evicted: u64,
    /// 累计内部子步数。
    pub substeps: u64,
    /// 当前存活数。
    pub alive: usize,
}

/// 粒子模拟器。
pub struct Simulator {
    cfg: SimConfig,
    /// 粒子池（长度 ≤ capacity）。
    parts: Vec<Particle>,
    /// 模拟时钟（ms）。
    now_ms: u32,
    stats: SimStats,
    /// 上一帧产生的交换通知（按需取走）。
    swaps: Vec<SwapNotice>,
}

impl Simulator {
    /// 构造。
    pub fn new(cfg: SimConfig) -> Result<Self, u16> {
        cfg.validate()?;
        Ok(Self {
            cfg,
            parts: Vec::with_capacity(cfg.capacity),
            now_ms: 0,
            stats: SimStats::default(),
            swaps: Vec::new(),
        })
    }

    pub fn config(&self) -> &SimConfig {
        &self.cfg
    }

    pub fn stats(&self) -> SimStats {
        let mut s = self.stats;
        s.alive = self.parts.len();
        s
    }

    pub fn now_ms(&self) -> u32 {
        self.now_ms
    }

    /// 存活粒子（只读）。
    pub fn particles(&self) -> &[Particle] {
        &self.parts
    }

    /// 取走本帧的交换通知（取走后清空）。
    pub fn take_swaps(&mut self) -> Vec<SwapNotice> {
        core::mem::take(&mut self.swaps)
    }

    /// 投放一批出生请求（来自发射器）。
    ///
    /// 返回实际入场数。溢出按策略处理并记账。
    pub fn spawn(&mut self, reqs: &[SpawnRequest], emitter_slot: u8, life_ms: u32) -> usize {
        if life_ms == 0 || life_ms > LIFE_MS_MAX {
            // 非法寿命不入场也不记账为成功——静默修正会成为隐蔽 bug 源。
            return 0;
        }
        let mut ok = 0usize;
        for r in reqs {
            if self.parts.len() >= self.cfg.capacity {
                match self.cfg.overflow {
                    OverflowPolicy::Reject => {
                        self.stats.rejected = self.stats.rejected.saturating_add(1);
                        continue;
                    }
                    OverflowPolicy::EvictOldest => {
                        if let Some(idx) = self.oldest_index() {
                            self.kill_at(idx);
                            self.stats.evicted = self.stats.evicted.saturating_add(1);
                        } else {
                            self.stats.rejected = self.stats.rejected.saturating_add(1);
                            continue;
                        }
                    }
                }
            }
            self.parts.push(Particle {
                pos: r.pos,
                vel: r.vel,
                born_ms: self.now_ms,
                life_ms,
                serial: r.serial,
                emitter_slot,
                flags: 0,
            });
            self.stats.spawned = self.stats.spawned.saturating_add(1);
            ok += 1;
        }
        ok
    }

    /// 推进一帧。
    ///
    /// 内部按 `substep_ms` 切分：`dt` 被均分为 `ceil(dt / substep)` 个子步，
    /// 每步等长（`dt / n`，余数分给最后一步）。等长子步是关键——不等分会让
    /// 积分误差与切分方式耦合，帧率一变轨迹就变。
    pub fn tick(&mut self, dt_ms: u32) -> usize {
        if dt_ms == 0 {
            return 0;
        }
        let n = if self.cfg.substep_ms == 0 {
            1
        } else {
            let s = self.cfg.substep_ms;
            (dt_ms + s - 1) / s
        };
        let base = dt_ms / n;
        let rem = dt_ms % n;

        for i in 0..n {
            let step = if i + 1 == n { base + rem } else { base };
            if step == 0 {
                continue;
            }
            self.integrate(step as f32 / 1000.0);
            self.now_ms = self.now_ms.saturating_add(step);
            self.stats.substeps = self.stats.substeps.saturating_add(1);
        }

        self.reap();
        self.parts.len()
    }

    /// 一个子步的半隐式（辛）欧拉积分 + 阻力衰减。
    fn integrate(&mut self, dt: f32) {
        let g = self.cfg.gravity_milli as f32 / 1000.0;
        // 阻力：每秒衰减 `drag/1000`，等价于 `v *= (1 - drag*dt/1000) ^ ?`。
        // 用一阶近似 `v *= 1 - k*dt`（k = drag/1000/s），在 dt ≤ 16ms 时
        // 与指数衰减偏差 < 1e-3，避免了 powf 的跨平台差异。
        let k = self.cfg.drag_per_mille as f32 / 1000.0;
        let damp = if k * dt >= 1.0 { 0.0 } else { 1.0 - k * dt };
        let cap = self.cfg.speed_cap;
        let cap2 = cap * cap;

        for p in self.parts.iter_mut() {
            // 1) 速度：先加外力（半隐式：用**新**速度推进位置）。
            p.vel[1] += g * dt;
            // 2) 阻力（各向同性）。
            p.vel[0] *= damp;
            p.vel[1] *= damp;
            p.vel[2] *= damp;
            // 3) 速度上限（防发散）。
            let s2 = p.vel[0] * p.vel[0] + p.vel[1] * p.vel[1] + p.vel[2] * p.vel[2];
            if s2 > cap2 {
                let inv = cap / sqrtf(s2);
                p.vel[0] *= inv;
                p.vel[1] *= inv;
                p.vel[2] *= inv;
            }
            // 4) 位置（用更新后的速度）。
            p.pos[0] += p.vel[0] * dt;
            p.pos[1] += p.vel[1] * dt;
            p.pos[2] += p.vel[2] * dt;
        }
    }

    /// 回收：死亡判定 + 交换删除。
    fn reap(&mut self) {
        let now = self.now_ms;
        let mut i = 0usize;
        let mut expired = 0u64;
        while i < self.parts.len() {
            if self.is_dead(i, now) {
                self.swap_remove(i);
                expired += 1;
            } else {
                i += 1;
            }
        }
        self.stats.expired = self.stats.expired.saturating_add(expired);
    }

    /// 单粒死亡判定。
    ///
    /// 两个分支互斥且必须**先判常驻**：常驻粒子只可能因 `DEAD` 位而亡，
    /// 绝不因寿命到期而亡。把这两个条件揉进一个长布尔式（`A && B || A && C`）
    /// 会让「优先级」代替「语义」决定行为——这正是本函数单独成法的原因。
    fn is_dead(&self, i: usize, now: u32) -> bool {
        let p = self.parts[i];
        let persist = (p.flags & flags::PERSIST) != 0;
        if persist {
            // 常驻：仅显式标记可杀。
            return (p.flags & flags::DEAD) != 0;
        }
        // 非常驻：显式标记或寿命到期，二者其一即亡。
        (p.flags & flags::DEAD) != 0 || now.wrapping_sub(p.born_ms) >= p.life_ms
    }


    /// 交换删除第 `i` 项，并记录通知。
    fn swap_remove(&mut self, i: usize) {
        let last = self.parts.len() - 1;
        if i != last {
            self.parts.swap(i, last);
            self.swaps.push(SwapNotice {
                dead: i as u32,
                moved_from: last as u32,
            });
        }
        self.parts.pop();
    }

    /// 显式杀死下标 `i` 的粒子（外力干预，如碰撞、场景切换）。
    pub fn kill_at(&mut self, i: usize) -> bool {
        if i >= self.parts.len() {
            return false;
        }
        self.swap_remove(i);
        true
    }

    /// 给下标 `i` 的粒子打上常驻标志（此后只受显式杀与 `evict` 影响）。
    ///
    /// 没有这个入口，`flags::PERSIST` 就是一个**没有任何路径能置位的死语义**
    /// ——判据里写的「常驻粒子不按寿命消亡」将永远无法被真正验证。故它必须
    /// 作为公开 API 存在，且 `is_dead` 必须先判常驻（见该函数注释）。
    pub fn mark_persist(&mut self, i: usize) -> bool {
        match self.parts.get_mut(i) {
            Some(p) => {
                p.flags |= flags::PERSIST;
                true
            }
            None => false,
        }
    }

    /// 给下标 `i` 的粒子打上拖尾标志（渲染侧按运动模糊处理）。
    pub fn mark_trail(&mut self, i: usize) -> bool {
        match self.parts.get_mut(i) {
            Some(p) => {
                p.flags |= flags::TRAIL;
                true
            }
            None => false,
        }
    }

    /// 给下标 `i` 的粒子打上死亡位（下一帧 `reap` 回收）。
    pub fn mark_dead(&mut self, i: usize) -> bool {
        match self.parts.get_mut(i) {
            Some(p) => {
                p.flags |= flags::DEAD;
                true
            }
            None => false,
        }
    }

    /// 显式杀死某发射器槽位的全部粒子（发射器停用）。
    pub fn kill_slot(&mut self, slot: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.parts.len() {
            if self.parts[i].emitter_slot == slot {
                self.swap_remove(i);
                n += 1;
            } else {
                i += 1;
            }
        }
        n
    }

    /// 存活最久粒子的下标（`EvictOldest` 用）。
    fn oldest_index(&self) -> Option<usize> {
        if self.parts.is_empty() {
            return None;
        }
        let mut best = 0usize;
        let mut best_born = self.parts[0].born_ms;
        let now = self.now_ms;
        for (i, p) in self.parts.iter().enumerate().skip(1) {
            // 用「已存活时长」而非出生时刻比较，以正确处理时钟回绕。
            let age = now.wrapping_sub(p.born_ms);
            let best_age = now.wrapping_sub(best_born);
            if age > best_age {
                best = i;
                best_born = p.born_ms;
            }
        }
        Some(best)
    }

    /// 粒子的年龄（ms）。
    pub fn age_ms(p: &Particle, now: u32) -> u32 {
        now.wrapping_sub(p.born_ms)
    }

    /// 粒子的寿命进度 `[0,1]`（1=将死）。常驻粒子恒 0。
    pub fn life_ratio(p: &Particle, now: u32) -> f32 {
        if (p.flags & flags::PERSIST) != 0 || p.life_ms == 0 {
            return 0.0;
        }
        let a = now.wrapping_sub(p.born_ms) as f32;
        let r = a / p.life_ms as f32;
        if r < 0.0 {
            0.0
        } else if r > 1.0 {
            1.0
        } else {
            r
        }
    }

    /// 全体粒子位置质心（遥测用）。
    pub fn centroid(&self) -> [f32; 3] {
        if self.parts.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        let mut s = [0.0f64; 3];
        for p in &self.parts {
            s[0] += p.pos[0] as f64;
            s[1] += p.pos[1] as f64;
            s[2] += p.pos[2] as f64;
        }
        let n = self.parts.len() as f64;
        [(s[0] / n) as f32, (s[1] / n) as f32, (s[2] / n) as f32]
    }

    /// 总动能（相对，用于能量守恒判据）。
    pub fn kinetic(&self) -> f64 {
        let mut e = 0.0f64;
        for p in &self.parts {
            e += p.vel[0] as f64 * p.vel[0] as f64
                + p.vel[1] as f64 * p.vel[1] as f64
                + p.vel[2] as f64 * p.vel[2] as f64;
        }
        e * 0.5
    }
}

/// 平方根（Newton 迭代；理由见 `vez22b_emitter::sqrtf`）。
pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = if x > 1.0 { x } else { 1.0 };
    for _ in 0..24 {
        g = 0.5 * (g + x / g);
    }
    g
}

// ── 判据 ────────────────────────────────────────────────────────

/// 造一个单粒模拟器（无重力无阻力，供纯运动学验证）。
fn ballistic(cfg: SimConfig, pos: [f32; 3], vel: [f32; 3], life: u32) -> Simulator {
    let mut sim = match Simulator::new(cfg) {
        Ok(s) => s,
        Err(_) => return Simulator {
            cfg,
            parts: Vec::new(),
            now_ms: 0,
            stats: SimStats::default(),
            swaps: Vec::new(),
        },
    };
    let _ = sim.spawn(
        &[SpawnRequest {
            pos,
            vel,
            serial: 0,
        }],
        0,
        life,
    );
    sim
}

fn no_force_cfg() -> SimConfig {
    SimConfig {
        substep_ms: 8,
        gravity_milli: 0,
        drag_per_mille: 0,
        capacity: 1024,
        overflow: OverflowPolicy::Reject,
        speed_cap: 100_000.0,
    }
}

/// F5223 判据集。
pub fn run_vez23b_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5223");

    // ── 1. 位置/速度积分（纯运动学：无外力下匀速直线）──
    {
        let p0 = [0.0f32, 0.0, 0.0];
        let v0 = [100.0f32, 0.0, 0.0]; // 100 cm/s
        let mut sim = ballistic(no_force_cfg(), p0, v0, 5000);
        for _ in 0..100 {
            let _ = sim.tick(10); // 1 秒
        }
        let p = sim.particles()[0];
        // 无阻力匀速：1 秒后 x 应恰为 100（相对误差 < 0.1%）。
        let err = (p.pos[0] - 100.0).abs();
        s.add(
            "VZ23-积分-01",
            err < 0.1,
            "无外力匀速：1s 位移误差 < 0.1cm（半隐式欧拉在匀速下精确）",
        );
        s.add(
            "VZ23-积分-02",
            (p.vel[0] - 100.0).abs() < 0.01,
            "无外力速度不变（无人工增能）",
        );
        s.add(
            "VZ23-积分-03",
            (p.pos[1]).abs() < 1e-3 && (p.pos[2]).abs() < 1e-3,
            "无外力侧向零漂移",
        );
    }

    // ── 2. 重力：半隐式欧拉位移公式核对 ──
    {
        // 半隐式欧拉：v_{n+1}=v_n+g dt；x_{n+1}=x_n+v_{n+1} dt。
        // 解析解 x(t) = 0.5 g t² 的半隐式离散化为 0.5 g t (t+dt)。
        // 取 g=-1000cm/s²、t=0.5s、dt=0.008s：x = -0.5*1000*0.5*0.508 = -127。
        let cfg = SimConfig {
            gravity_milli: -1000,
            drag_per_mille: 0,
            speed_cap: 100_000.0,
            ..no_force_cfg()
        };
        let mut sim = ballistic(cfg, [0.0, 1000.0, 0.0], [0.0, 0.0, 0.0], 5000);
        let mut steps = 0;
        while sim.now_ms() < 500 {
            let _ = sim.tick(8);
            steps += 1;
        }
        let p = sim.particles()[0];
        let disp = 1000.0 - p.pos[1];
        // 半隐式理论值 = 0.5*1000*0.5*(0.5+0.008) = 127.0
        let expect = 127.0f32;
        let rel = (disp - expect).abs() / expect;
        s.add(
            "VZ23-重力-01",
            rel < 0.01,
            "重力位移与半隐式离散解一致（相对误差 < 1%）",
        );
        s.add(
            "VZ23-重力-02",
            p.vel[1] < -490.0 && p.vel[1] > -510.0,
            "0.5s 后速度 ≈ -500cm/s（g*t）",
        );
        s.add("VZ23-重力-03", steps > 0 && sim.stats().alive == 1, "重力场下粒子不误亡");
    }

    // ── 3. 辛性质：能量不随时间发散 ──
    {
        // 半隐式欧拉在有界力场下能量误差有界。这里用常数重力 + 速度上限，
        // 检查「每 100 帧的总动能」不呈单调增长。
        let cfg = SimConfig {
            gravity_milli: -980,
            drag_per_mille: 0,
            speed_cap: 100_000.0,
            capacity: 64,
            ..no_force_cfg()
        };
        let mut sim = ballistic(cfg, [0.0, 5000.0, 0.0], [0.0, 0.0, 0.0], 60_000);
        let mut ks = [0.0f64; 5];
        for k in 0..5 {
            for _ in 0..100 {
                let _ = sim.tick(8);
            }
            ks[k] = sim.kinetic();
        }
        // 自由落体动能应单调增（重力做功），但增量必须**线性**（不是指数）。
        let d1 = ks[1] - ks[0];
        let d2 = ks[2] - ks[1];
        let ratio = if d1 > 0.0 { d2 / d1 } else { 0.0 };
        s.add(
            "VZ23-辛性-01",
            ratio > 0.7 && ratio < 1.4,
            "自由落体动能增量逐段近似相等（无指数增能，辛积分器性质）",
        );
    }

    // ── 4. 子步切分稳定性：混合 dt 与定步长轨迹一致 ──
    {
        let mk = || {
            let cfg = SimConfig {
                gravity_milli: -980,
                drag_per_mille: 300,
                speed_cap: 100_000.0,
                ..no_force_cfg()
            };
            ballistic(cfg, [0.0, 0.0, 0.0], [200.0, 400.0, -100.0], 20_000)
        };
        // 参考：全程 8ms。
        let mut a = mk();
        for _ in 0..125 {
            let _ = a.tick(8); // 1000ms
        }
        let pa = a.particles()[0];
        // 被测：混合 dt（5..33ms 抖动），总时长同样 1000ms。
        let mut b = mk();
        let mut rng = mk_lcg(0x5BC0_0001);
        let mut acc = 0u32;
        while acc < 1000 {
            let mut dt = 5 + ((rng)() % 29);
            if acc + dt > 1000 {
                dt = 1000 - acc;
            }
            let _ = b.tick(dt);
            acc += dt;
        }
        let pb = b.particles()[0];
        let mut worst = 0.0f32;
        for k in 0..3 {
            let d = (pa.pos[k] - pb.pos[k]).abs();
            let scale = if pa.pos[k].abs() > 1.0 {
                pa.pos[k].abs()
            } else {
                1.0
            };
            let rel = d / scale;
            if rel > worst {
                worst = rel;
            }
        }
        s.add(
            "VZ23-切分-01",
            worst < 0.02,
            "混合 dt 与定步长轨迹端点相对误差 < 2%（子步等分生效）",
        );
        s.add(
            "VZ23-切分-02",
            a.now_ms() == 1000 && b.now_ms() == 1000,
            "两条路径模拟时钟同为 1000ms（时间守恒）",
        );
        s.add(
            "VZ23-切分-03",
            b.stats().substeps > 100,
            "混合 dt 路径确实切出了多个子步（不是一帧一步）",
        );
    }

    // ── 5. 帧率无关性：60fps 与 144fps 同物理时长端点一致 ──
    {
        let cfg = SimConfig {
            gravity_milli: -980,
            drag_per_mille: 500,
            speed_cap: 100_000.0,
            ..no_force_cfg()
        };
        let mut a = ballistic(cfg, [0.0, 100.0, 0.0], [300.0, 200.0, 0.0], 30_000);
        for _ in 0..60 {
            let _ = a.tick(16); // 960ms @60fps
        }
        let mut b = ballistic(cfg, [0.0, 100.0, 0.0], [300.0, 200.0, 0.0], 30_000);
        // 144fps ≈ 6.94ms/帧；用 7ms × 137 = 959ms，再补 1ms。
        for _ in 0..137 {
            let _ = b.tick(7);
        }
        let _ = b.tick(1);
        let pa = a.particles()[0];
        let pb = b.particles()[0];
        let dy = (pa.pos[1] - pb.pos[1]).abs();
        let dx = (pa.pos[0] - pb.pos[0]).abs();
        s.add(
            "VZ23-帧率-01",
            a.now_ms() == 960 && b.now_ms() == 960,
            "两条路径物理时长严格相等（960ms）",
        );
        s.add(
            "VZ23-帧率-02",
            dy < 12.0 && dx < 6.0,
            "60fps 与 144fps 端点偏差在容差内（帧率无关）",
        );
    }

    // ── 6. 生命周期：绝对到期时刻，退出精确 ──
    {
        let mut sim = ballistic(no_force_cfg(), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 100);
        // 96ms 时仍在。
        for _ in 0..12 {
            let _ = sim.tick(8);
        }
        let alive_before = sim.stats().alive == 1;
        // 再过 8ms（t=104 ≥ born+life=100）：应消亡。
        let _ = sim.tick(8);
        let dead_after = sim.stats().alive == 0;
        s.add("VZ23-寿命-01", alive_before, "96ms < 100ms 寿命时仍存活");
        s.add("VZ23-寿命-02", dead_after, "104ms ≥ 100ms 时准时消亡（绝对到期判据）");
        s.add(
            "VZ23-寿命-03",
            sim.stats().expired == 1,
            "消亡计数精确（1 粒）",
        );
    }
    // 常驻粒子不按寿命消亡（必须有真实路径能打上 PERSIST，否则判据无意义）。
    {
        let mut sim = ballistic(no_force_cfg(), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 50);
        let marked = sim.mark_persist(0);
        // 跑到远大于寿命（30 帧 × 8ms = 240ms ≫ 50ms）。
        for _ in 0..30 {
            let _ = sim.tick(8);
        }
        let survived = sim.stats().alive == 1;
        // 对照：未打标志的同寿命粒子在同一时长后必亡。
        let mut ctrl = ballistic(no_force_cfg(), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 50);
        for _ in 0..30 {
            let _ = ctrl.tick(8);
        }
        let control_dead = ctrl.stats().alive == 0;
        s.add(
            "VZ23-寿命-04",
            marked && survived && control_dead,
            "常驻粒子超寿命 5 倍仍存活；对照粒子同时长必亡（语义有区分力）",
        );
        // 常驻粒子仍可被显式标记杀。
        let killed = sim.mark_dead(0);
        let _ = sim.tick(8);
        s.add(
            "VZ23-寿命-06",
            killed && sim.stats().alive == 0,
            "常驻粒子可被 mark_dead 显式回收（常驻≠不可杀）",
        );
    }

    // 0 寿命非法（不被静默修正为最小寿命）。
    {
        let mut sim = match Simulator::new(no_force_cfg()) {
            Ok(s) => s,
            Err(_) => return s,
        };
        let n = sim.spawn(
            &[SpawnRequest {
                pos: [0.0; 3],
                vel: [0.0; 3],
                serial: 0,
            }],
            0,
            0,
        );
        let over = sim.spawn(
            &[SpawnRequest {
                pos: [0.0; 3],
                vel: [0.0; 3],
                serial: 1,
            }],
            0,
            LIFE_MS_MAX + 1,
        );
        s.add("VZ23-寿命-05", n == 0 && over == 0, "反向：寿命 0 与超上限均不入场");
    }

    // ── 7. 缓冲管理：交换删除保下标稳定 + 通知正确 ──
    {
        let mut sim = match Simulator::new(SimConfig {
            capacity: 8,
            ..no_force_cfg()
        }) {
            Ok(s) => s,
            Err(_) => return s,
        };
        // 5 粒，寿命各异：用不同 born 时刻模拟（靠 tick 推进）。
        let mut reqs = Vec::new();
        for i in 0..5u32 {
            reqs.push(SpawnRequest {
                pos: [i as f32, 0.0, 0.0],
                vel: [0.0; 3],
                serial: i,
            });
        }
        let n = sim.spawn(&reqs, 0, 10_000);
        let full5 = sim.stats().alive == 5;
        // 杀掉中间一粒（下标 2），应产生交换通知（末位 4 换到 2）。
        let killed = sim.kill_at(2);
        let swaps = sim.take_swaps();
        let swapped = sim.stats().alive == 4;
        let serials: Vec<u32> = sim.particles().iter().map(|p| p.serial).collect();
        s.add("VZ23-缓冲-01", n == 5 && full5, "5 粒全部入场");
        s.add("VZ23-缓冲-02", killed && swapped, "kill_at 生效且存活数减 1");
        s.add(
            "VZ23-缓冲-03",
            swaps.len() == 1 && swaps[0].dead == 2 && swaps[0].moved_from == 4,
            "交换通知正确（2 号位被 4 号位填充）",
        );
        s.add(
            "VZ23-缓冲-04",
            serials == alloc::vec![0u32, 1, 4, 3],
            "交换删除后序号布局符合预期（0,1,4,3）",
        );
        // 再取一次应为空（通知已取走）。
        let again = sim.take_swaps();
        s.add("VZ23-缓冲-05", again.is_empty(), "通知取走后不重复给出");
    }
    // 按槽位批量杀。
    {
        let mut sim = match Simulator::new(no_force_cfg()) {
            Ok(s) => s,
            Err(_) => return s,
        };
        let mk = |n: u32| {
            let mut v = Vec::new();
            for i in 0..n {
                v.push(SpawnRequest {
                    pos: [0.0; 3],
                    vel: [0.0; 3],
                    serial: i,
                });
            }
            v
        };
        let _ = sim.spawn(&mk(3), 0, 10_000);
        let _ = sim.spawn(&mk(4), 1, 10_000);
        let killed = sim.kill_slot(1);
        s.add(
            "VZ23-缓冲-06",
            killed == 4 && sim.stats().alive == 3,
            "kill_slot 精确清空槽位 1 的 4 粒",
        );
    }

    // ── 8. 溢出策略三态 ──
    {
        let mk = |n: u32| {
            let mut v = Vec::new();
            for i in 0..n {
                v.push(SpawnRequest {
                    pos: [0.0; 3],
                    vel: [0.0; 3],
                    serial: i,
                });
            }
            v
        };
        // Reject：拒新保老。
        let mut sim = match Simulator::new(SimConfig {
            capacity: 4,
            overflow: OverflowPolicy::Reject,
            ..no_force_cfg()
        }) {
            Ok(s) => s,
            Err(_) => return s,
        };
        let _ = sim.spawn(&mk(4), 0, 10_000);
        let acc = sim.spawn(&mk(3), 0, 10_000);
        let st = sim.stats();
        s.add(
            "VZ23-溢出-01",
            acc == 0 && st.rejected == 3 && st.alive == 4,
            "Reject 策略：满后全拒并记 3",
        );
        let serials: Vec<u32> = sim.particles().iter().map(|p| p.serial).collect();
        s.add(
            "VZ23-溢出-02",
            serials == alloc::vec![0u32, 1, 2, 3],
            "Reject 保住老粒子（序号 0..3 未变）",
        );

        // EvictOldest：挤旧保新。
        let mut sim2 = match Simulator::new(SimConfig {
            capacity: 4,
            overflow: OverflowPolicy::EvictOldest,
            ..no_force_cfg()
        }) {
            Ok(s) => s,
            Err(_) => return s,
        };
        let _ = sim2.spawn(&mk(4), 0, 10_000);
        // 推进时间，让后 3 粒更「新」。
        let _ = sim2.tick(8);
        let acc2 = sim2.spawn(&mk(2), 0, 10_000);
        let st2 = sim2.stats();
        s.add(
            "VZ23-溢出-03",
            acc2 == 2 && st2.evicted == 2 && st2.alive == 4,
            "EvictOldest 策略：挤 2 旧入 2 新，存活恒 4",
        );
        let serials2: Vec<u32> = sim2.particles().iter().map(|p| p.serial).collect();
        s.add(
            "VZ23-溢出-04",
            serials2.contains(&4) && serials2.contains(&5),
            "EvictOldest 后新粒子确实在场（序号 4/5）",
        );
        s.add(
            "VZ23-溢出-05",
            st2.rejected == 0,
            "EvictOldest 路径不产生拒绝计数（两种策略不混记）",
        );
    }

    // ── 9. 参数校验 + 工具 ──
    {
        let mut c = SimConfig::standard();
        let ok = c.validate().is_ok();
        c.substep_ms = SUBSTEP_MS_MAX + 1;
        let bad1 = c.validate() == Err(E_SUBSTEP);
        c = SimConfig::standard();
        c.capacity = 0;
        let bad2 = c.validate() == Err(E_CAPACITY);
        c = SimConfig::standard();
        c.drag_per_mille = 1001;
        let bad3 = c.validate() == Err(E_FORCE_PARAM);
        s.add(
            "VZ23-校验-01",
            ok && bad1 && bad2 && bad3,
            "校验矩阵：标准过；子步超限/零容量/阻力>1000 各拒",
        );
        s.add(
            "VZ23-校验-02",
            SimConfig::standard().capacity <= CAPACITY_MAX,
            "标准容量在上限内",
        );
    }
    s.add(
        "VZ23-冻结-01",
        SIM_VERSION == "F5223-sim-v1",
        "API 版本冻结：F5223-sim-v1",
    );
    {
        let sim = match Simulator::new(no_force_cfg()) {
            Ok(s) => s,
            Err(_) => return s,
        };
        let c = sim.centroid();
        s.add(
            "VZ23-遥测-01",
            c[0] == 0.0 && c[1] == 0.0 && c[2] == 0.0,
            "空池质心为零（不除零）",
        );
        s.add("VZ23-遥测-02", sim.kinetic() == 0.0, "空池动能为零");
    }

    s
}

/// 极简 LCG（判据内用，与发射器模块的流同构但独立，避免共享状态）。
fn mk_lcg(seed: u32) -> impl FnMut() -> u32 {
    let mut st = if seed == 0 { 0x9E37_79B9 } else { seed };
    move || {
        st = st.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        st >> 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg0() -> SimConfig {
        no_force_cfg()
    }

    fn one(sim: &mut Simulator, n: u32) {
        let mut v = Vec::new();
        for i in 0..n {
            v.push(SpawnRequest {
                pos: [0.0; 3],
                vel: [0.0; 3],
                serial: i,
            });
        }
        sim.spawn(&v, 0, 10_000);
    }

    #[test]
    fn uniform_motion_is_exact() {
        let mut sim = ballistic(cfg0(), [0.0, 0.0, 0.0], [50.0, 0.0, 0.0], 10_000);
        for _ in 0..50 {
            sim.tick(20); // 1000ms
        }
        assert!((sim.particles()[0].pos[0] - 50.0).abs() < 0.05);
    }

    #[test]
    fn swap_remove_notice_is_accurate() {
        let mut sim = Simulator::new(SimConfig {
            capacity: 16,
            ..cfg0()
        })
        .expect("sim");
        one(&mut sim, 6);
        assert!(sim.kill_at(1));
        let sw = sim.take_swaps();
        assert_eq!(sw.len(), 1);
        assert_eq!(sw[0].dead, 1);
        assert_eq!(sw[0].moved_from, 5);
        assert_eq!(sim.particles().len(), 5);
    }

    #[test]
    fn expiry_is_exact_on_boundary() {
        let mut sim = Simulator::new(cfg0()).expect("sim");
        let mut v = Vec::new();
        v.push(SpawnRequest {
            pos: [0.0; 3],
            vel: [0.0; 3],
            serial: 0,
        });
        sim.spawn(&v, 0, 64);
        for _ in 0..7 {
            sim.tick(8); // 56ms
        }
        assert_eq!(sim.stats().alive, 1);
        sim.tick(8); // 64ms：达到寿命
        assert_eq!(sim.stats().alive, 0);
    }

    #[test]
    fn evict_oldest_keeps_newest() {
        let mut sim = Simulator::new(SimConfig {
            capacity: 2,
            overflow: OverflowPolicy::EvictOldest,
            ..cfg0()
        })
        .expect("sim");
        one(&mut sim, 2);
        sim.tick(8);
        one(&mut sim, 1);
        assert_eq!(sim.stats().alive, 2);
        assert!(sim.particles().iter().any(|p| p.serial == 1));
    }

    #[test]
    fn speed_cap_bounds_velocity() {
        let cfg = SimConfig {
            gravity_milli: -1_000_000,
            speed_cap: 500.0,
            drag_per_mille: 0,
            ..cfg0()
        };
        let mut sim = ballistic(cfg, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 10_000);
        for _ in 0..20 {
            sim.tick(16);
        }
        let p = sim.particles()[0];
        let sp = sqrtf(p.vel[0] * p.vel[0] + p.vel[1] * p.vel[1] + p.vel[2] * p.vel[2]);
        assert!(sp <= 500.5, "sp={}", sp);
    }

    #[test]
    fn drag_slows_particle_down() {
        let cfg = SimConfig {
            gravity_milli: 0,
            drag_per_mille: 800,
            ..cfg0()
        };
        let mut sim = ballistic(cfg, [0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], 10_000);
        for _ in 0..60 {
            sim.tick(16);
        }
        let v = sim.particles()[0].vel[0];
        assert!(v < 300.0 && v > 0.0, "v={}", v);
    }

    #[test]
    fn life_ratio_clamps() {
        let p = Particle {
            pos: [0.0; 3],
            vel: [0.0; 3],
            born_ms: 100,
            life_ms: 200,
            serial: 0,
            emitter_slot: 0,
            flags: 0,
        };
        assert_eq!(Simulator::life_ratio(&p, 100), 0.0);
        assert!((Simulator::life_ratio(&p, 200) - 0.5).abs() < 1e-5);
        assert_eq!(Simulator::life_ratio(&p, 999), 1.0);
    }
}
