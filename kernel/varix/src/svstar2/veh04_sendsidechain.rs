//! VE-F1404 · 发送插入与侧链（VE-H 域 · 音频引擎 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1404`
//!
//! **规格原文**：Send 发送（辅助总线共享效果实例——混响/延迟多路共享省算力，
//! Send 电平逐路可调）；Insert 插入（节点级串联效果链，处理顺序=链序；
//! Send（并行共享）与 Insert（串行独占）的选型指南）；侧链 Sidechain（一路
//! 信号触发另一路处理——ducking/去咝声经典机制，侧链输入→受控节点的参数
//! 调制，F1426 ducking 引擎的信号级底层）；效果尾音保护（节点移除后尾音
//! 自然衰减不戛然而止——尾音缓冲管理）；自动化接口（参数包络挂接——复用
//! F1324 预留求值器）。判据：Send 共享、Insert、侧链、尾音保护、自动化、判据。
//!
//! **设计要点**：
//! - Send 是"并行共享"：N 路发送挂同一个效果实例，每路电平独立可调——
//!   省的是 N-1 份效果算力，账面算出来不是喊出来；
//! - Insert 是"串行独占"：效果链串入信号路径，处理顺序 = 链序——两条
//!   语义的选型指南表驱动，不靠口口相传；
//! - 侧链是"一路触发另一路"：触发源包络 → 受控参数调制（ducking：人声
//!   压音乐；去咝声：齿音压 EQ），attack/release 一阶平滑（F1329 同核），
//!   压制量封顶（过压制是缺陷不是力度）；
//! - 尾音保护：移除的效果节点进尾音期，按 RT60 衰减到 -60dB 才真正释放——
//!   尾音硬切 = 可闻突变 = 缺陷；
//! - 自动化：参数包络挂接（节点, 参数) → 分段线性求值（F1324 预留求值器
//!   同核），末点保持语义显性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、Send：共享效果实例 + 逐路电平
// ---------------------------------------------------------------------------

/// 共享效果实例（混响/延迟等多消费者共享）。
#[derive(Clone, Debug, PartialEq)]
pub struct EffectInstance {
    pub id: u32,
    pub kind: &'static str,
    /// 尾音参数：RT60 秒（能量衰减 60dB 所需时间）。
    pub rt60_s: f64,
}

/// 一路 Send：源 → 共享效果，电平独立。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendPath {
    pub source_node: u32,
    pub effect_id: u32,
    /// 发送电平（0..1000 千分比——整数运算避免浮点账）。
    pub level_permille: u32,
}

/// Send 总线：效果实例登记 + 发送路径表。
#[derive(Debug)]
pub struct SendBus {
    pub effects: Vec<EffectInstance>,
    pub paths: Vec<SendPath>,
}

impl SendBus {
    pub fn new() -> SendBus {
        SendBus {
            effects: Vec::new(),
            paths: Vec::new(),
        }
    }

    pub fn register_effect(&mut self, id: u32, kind: &'static str, rt60_s: f64) {
        if !self.effects.iter().any(|e| e.id == id) {
            self.effects.push(EffectInstance { id, kind, rt60_s });
        }
    }

    /// 加一路 Send：电平钳制到 0..1000，目标实例必须已登记。
    pub fn add_send(
        &mut self,
        source_node: u32,
        effect_id: u32,
        level_permille: u32,
    ) -> Result<(), String> {
        if !self.effects.iter().any(|e| e.id == effect_id) {
            return Err(format!("效果实例 {} 未登记——Send 只能挂共享实例", effect_id));
        }
        let level = level_permille.min(1000);
        // 同源同效果只允许一路（重复 Send = 信号加倍）
        if self
            .paths
            .iter()
            .any(|p| p.source_node == source_node && p.effect_id == effect_id)
        {
            return Err(format!("节点 {} 对效果 {} 已有 Send", source_node, effect_id));
        }
        self.paths.push(SendPath {
            source_node,
            effect_id,
            level_permille: level,
        });
        Ok(())
    }

    /// 某效果实例的消费者数。
    pub fn consumers_of(&self, effect_id: u32) -> usize {
        self.paths
            .iter()
            .filter(|p| p.effect_id == effect_id)
            .count()
    }

    /// 共享收益（对照"每源独立实例"）：节省的实例份数百分比。
    pub fn sharing_savings_pct(&self, effect_id: u32) -> u64 {
        let n = self.consumers_of(effect_id);
        if n <= 1 {
            return 0;
        }
        ((n - 1) as u64 * 100) / n as u64
    }

    /// 某路 Send 的实际增益（千分比 → 幅值，供渲染层）。
    pub fn send_gain(&self, source_node: u32, effect_id: u32) -> f64 {
        self.paths
            .iter()
            .find(|p| p.source_node == source_node && p.effect_id == effect_id)
            .map(|p| p.level_permille as f64 / 1000.0)
            .unwrap_or(0.0)
    }
}

// ---------------------------------------------------------------------------
// 二、Insert：串行独占效果链（处理顺序 = 链序）
// ---------------------------------------------------------------------------

/// 插入链：挂在节点上的有序效果序列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertChain {
    pub node: u32,
    /// 链序即处理顺序（链头先处理）。
    pub effects: Vec<u32>,
}

impl InsertChain {
    pub fn new(node: u32) -> InsertChain {
        InsertChain {
            node,
            effects: Vec::new(),
        }
    }

    /// 追加效果（链尾）。
    pub fn append(&mut self, effect_id: u32) -> Result<(), String> {
        if self.effects.contains(&effect_id) {
            return Err(format!("效果 {} 已在节点 {} 的插入链中", effect_id, self.node));
        }
        self.effects.push(effect_id);
        Ok(())
    }

    /// 在指定位置插入（越界钳到链尾）。
    pub fn insert_at(&mut self, pos: usize, effect_id: u32) -> Result<(), String> {
        if self.effects.contains(&effect_id) {
            return Err(format!("效果 {} 已在节点 {} 的插入链中", effect_id, self.node));
        }
        let pos = pos.min(self.effects.len());
        self.effects.insert(pos, effect_id);
        Ok(())
    }

    /// 处理顺序 = 链序（返回处理序的效果号切片）。
    pub fn processing_order(&self) -> &[u32] {
        &self.effects
    }
}

/// Send / Insert 选型指南（表驱动：语义差异决定场景）。
pub struct SelectionGuide;

impl SelectionGuide {
    /// 选型裁决：并行共享效果（多源共用一处空间）用 Send；
    /// 单源专属串联塑形用 Insert。返回人话指南。
    pub fn advise(parallel_shared: bool, exclusive_shaping: bool) -> &'static str {
        match (parallel_shared, exclusive_shaping) {
            (true, _) => "选 Send：多路共享一个效果实例（并行），逐路电平可调，省算力",
            (_, true) => "选 Insert：效果链串入本节点信号路径（串行独占），处理顺序=链序",
            _ => "两者皆可：按风格意图选——空间类共享选 Send，塑形类独占选 Insert",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、侧链：一路信号触发另一路的参数调制
// ---------------------------------------------------------------------------

/// 侧链路由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SidechainRoute {
    /// 触发源节点（如人声轨）。
    pub trigger_node: u32,
    /// 受控节点（如音乐总线）。
    pub target_node: u32,
    /// 被调制的参数名（"gain"=ducking、"eq_highshelf"=去咝声）。
    pub param: &'static str,
    /// 最大压制量（dB，负值语义用绝对值记账）。
    pub max_reduction_db: u32,
    /// attack 步数（触发响应快）。
    pub attack_steps: u32,
    /// release 步数（恢复慢）。
    pub release_steps: u32,
}

/// 一阶平滑状态机（F1329 参数平滑同核）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoother {
    pub value: f64,
    pub attack_coef: f64,
    pub release_coef: f64,
}

impl Smoother {
    /// 系数 = 1 - exp(-1/steps)：attack 越小越快。
    pub fn new(attack_steps: u32, release_steps: u32) -> Smoother {
        Smoother {
            value: 0.0,
            attack_coef: 1.0 - (-1.0 / attack_steps.max(1) as f64).exp(),
            release_coef: 1.0 - (-1.0 / release_steps.max(1) as f64).exp(),
        }
    }

    /// 单步推进：目标升高走 attack、回落走 release（快起慢放）。
    pub fn step(&mut self, target: f64) -> f64 {
        let coef = if target > self.value {
            self.attack_coef
        } else {
            self.release_coef
        };
        self.value += (target - self.value) * coef;
        self.value
    }
}

/// ducking 调制曲线：触发包络 → 受控参数的压制曲线（dB）。
///
/// 触发电平按 0..1 归一；输出 = -max_reduction × 平滑后的触发电平。
pub fn ducking_curve(
    trigger: &[f64],
    max_reduction_db: f64,
    attack_steps: u32,
    release_steps: u32,
) -> Vec<f64> {
    let mut sm = Smoother::new(attack_steps, release_steps);
    trigger
        .iter()
        .map(|t| {
            let env = sm.step(t.clamp(0.0, 1.0));
            -max_reduction_db * env
        })
        .collect()
}

/// 侧链账目校验：路由合法（触发 ≠ 受控、参数登记、压制量封顶）。
pub fn validate_sidechain(route: &SidechainRoute) -> Result<(), String> {
    if route.trigger_node == route.target_node {
        return Err("侧链触发与受控是同一节点——自激环".to_string());
    }
    if route.param != "gain" && route.param != "eq_highshelf" {
        return Err(format!("参数 {} 未在侧链可调集登记", route.param));
    }
    if route.max_reduction_db > 60 {
        return Err("压制量超过 60dB 封顶——过压制是缺陷不是力度".to_string());
    }
    if route.release_steps < route.attack_steps {
        return Err("release 短于 attack——快放慢起会产生泵吸抖动".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、尾音保护：移除的效果节点衰减到 -60dB 才释放
// ---------------------------------------------------------------------------

/// 尾音期节点状态。
#[derive(Clone, Debug, PartialEq)]
pub struct TailNode {
    pub effect_id: u32,
    pub rt60_s: f64,
    pub sample_rate: u32,
    /// 已经历的尾音 tick 数。
    pub elapsed_ticks: u64,
}

impl TailNode {
    /// RT60 → 每 tick 能量衰减系数：10^(-3 / (rt60·rate))（-60dB 全程）。
    pub fn decay_per_tick(&self) -> f64 {
        let total_ticks = self.tail_total_ticks();
        if total_ticks == 0 {
            return 0.0;
        }
        0.001f64.powf(1.0 / total_ticks as f64)
    }

    /// -60dB 所需总 tick 数 = rt60 · rate。
    pub fn tail_total_ticks(&self) -> u64 {
        (self.rt60_s * self.sample_rate as f64) as u64
    }

    /// 当前能量级（dB，相对满幅）。
    pub fn level_db(&self) -> f64 {
        let total = self.tail_total_ticks() as f64;
        if total <= 0.0 {
            return -60.0;
        }
        let ratio = self.elapsed_ticks as f64 / total;
        -60.0 * ratio.min(1.0)
    }

    /// 尾音是否自然结束。
    pub fn finished(&self) -> bool {
        self.elapsed_ticks >= self.tail_total_ticks()
    }
}

/// 尾音缓冲池：节点移除 → 入池续渲染 → 自然结束才释放。
#[derive(Debug, Default)]
pub struct TailPool {
    pub tails: Vec<TailNode>,
    pub released_total: u64,
    /// 硬切拦截账（在尾音未完时重复移除/直接释放 = 缺陷，被拦即留痕）。
    pub hard_cut_blocked: u64,
}

impl TailPool {
    pub fn new() -> TailPool {
        TailPool::default()
    }

    /// 节点移除：入尾音池（有 RT60 的效果才入；干声类无尾音直接走）。
    pub fn retire(&mut self, effect_id: u32, rt60_s: f64, sample_rate: u32) {
        if rt60_s <= 0.0 {
            self.released_total += 1;
            return;
        }
        // 已在尾音期的再移除 = 试图硬切，拦截留痕
        if self.tails.iter().any(|t| t.effect_id == effect_id) {
            self.hard_cut_blocked += 1;
            return;
        }
        self.tails.push(TailNode {
            effect_id,
            rt60_s,
            sample_rate,
            elapsed_ticks: 0,
        });
    }

    /// 推进 n 个 tick：尾音能量自然衰减，到 -60dB 释放。
    pub fn advance(&mut self, ticks: u64) -> usize {
        for t in self.tails.iter_mut() {
            t.elapsed_ticks = (t.elapsed_ticks + ticks).min(t.tail_total_ticks());
        }
        let before = self.tails.len();
        let done: Vec<u32> = self
            .tails
            .iter()
            .filter(|t| t.finished())
            .map(|t| t.effect_id)
            .collect();
        self.tails.retain(|t| !t.finished());
        self.released_total += done.len() as u64;
        before - self.tails.len()
    }

    /// 指定效果是否仍在尾音期（期间重挂同一实例必须复用尾音或拒绝重建）。
    pub fn still_tail(&self, effect_id: u32) -> bool {
        self.tails.iter().any(|t| t.effect_id == effect_id)
    }
}

// ---------------------------------------------------------------------------
// 五、自动化：参数包络挂接（F1324 预留求值器同核：分段线性 + 末点保持）
// ---------------------------------------------------------------------------

/// 包络锚点（逻辑 tick, 值）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvelopePoint {
    pub tick: u64,
    pub value: f64,
}

/// 参数包络。
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub node: u32,
    pub param: &'static str,
    pub points: Vec<EnvelopePoint>,
}

impl Envelope {
    pub fn new(node: u32, param: &'static str, points: Vec<EnvelopePoint>) -> Envelope {
        let mut p = points;
        p.sort_by_key(|pt| pt.tick);
        Envelope { node, param, points: p }
    }

    /// 求值：分段线性 + 末点保持（F1324 同核语义）。
    pub fn eval(&self, tick: u64) -> f64 {
        if self.points.is_empty() {
            return 0.0;
        }
        if tick <= self.points[0].tick {
            return self.points[0].value;
        }
        let last = self.points.last().unwrap();
        if tick >= last.tick {
            return last.value;
        }
        for w in self.points.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if tick >= a.tick && tick <= b.tick {
                let span = (b.tick - a.tick) as f64;
                if span == 0.0 {
                    return b.value;
                }
                let t = (tick - a.tick) as f64 / span;
                return a.value + (b.value - a.value) * t;
            }
        }
        last.value
    }
}

/// 自动化机架：挂接/摘除/求值。
#[derive(Debug, Default)]
pub struct AutomationRack {
    pub envelopes: Vec<Envelope>,
}

impl AutomationRack {
    pub fn new() -> AutomationRack {
        AutomationRack::default()
    }

    pub fn attach(&mut self, env: Envelope) {
        self.envelopes
            .retain(|e| !(e.node == env.node && e.param == env.param));
        self.envelopes.push(env);
    }

    pub fn detach(&mut self, node: u32, param: &str) -> bool {
        let before = self.envelopes.len();
        self.envelopes.retain(|e| !(e.node == node && e.param == param));
        self.envelopes.len() != before
    }

    /// 求值：挂了包络走包络；没挂返回 None（渲染层用静态值）。
    pub fn eval(&self, node: u32, param: &str, tick: u64) -> Option<f64> {
        self.envelopes
            .iter()
            .find(|e| e.node == node && e.param == param)
            .map(|e| e.eval(tick))
    }
}

// ---------------------------------------------------------------------------
// 六、读屏摘要
// ---------------------------------------------------------------------------

/// 服务级摘要（Send/Insert/侧链/尾音/自动化五面）。
pub struct RoutingSummary;

impl RoutingSummary {
    pub fn render(
        send_effects: usize,
        send_paths: usize,
        insert_chains: usize,
        sidechains: usize,
        tails: usize,
        automations: usize,
    ) -> String {
        format!(
            "音频路由：共享效果实例 {} 个（Send 路数 {}）、插入链 {} 条、侧链 {} 条、尾音中 {} 个、自动化 {} 条",
            send_effects, send_paths, insert_chains, sidechains, tails, automations
        )
    }
}
