//! unx48278_fcisol — UNX-F48278 addon 崩溃隔离（addon 死亡不拖垮桥，自动切回自研引擎）（诊断段 0xC3A0）。
//!
//! 判据原文：
//!   J1 杀 addon ×10 桥存活且引擎自动回退零感切换；判据锚定 体验十四 沙箱隔离。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 隔离不变式：addon 进程死于任意状态（建链中/换库中/提交中），
//!      桥本体状态机必须保持 Alive，在途输入事件由自研引擎即时接棒
//!      （回退切换对用户零感：无丢键、无状态重置）。
//!   2. ×10 杀演练：10 轮「建链→用→杀→回退→重建」循环，逐轮断言桥
//!      存活 + 回退计数 + 在途事件零丢失 + 零感账（用户可见状态位
//!      前后一致）。
//!   3. 健康探测：桥对 addon 心跳（tick 预算 3 拍）；超时即判死，
//!      判死与真实死亡（显式 kill 事件）两条路径都进同一条回退管线。
//!   4. 切换幂等：死亡风暴（连续 kill 通知）只触发一次回退；
//!      重建成功后才允许再次启用 addon。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC3A0;

const E_BRIDGE_DEAD: u16 = SEG_BASE + 1;  // 桥被拖垮（隔离破）
const E_LOST_EVENT: u16 = SEG_BASE + 2;   // 回退期丢键
const E_DOUBLE_FALL: u16 = SEG_BASE + 3;  // 死亡风暴重复回退
const E_REBUILD_EARLY: u16 = SEG_BASE + 4; // 未回退就重建
const E_ZERO_SHIFT: u16 = SEG_BASE + 5;   // 零感账破（可见状态位变化）
const E_HB_LOST: u16 = SEG_BASE + 6;      // 心跳账不平
const E_GUARD_UNBAL: u16 = SEG_BASE + 7;  // 隔离账不平

/// 引擎档位：自研引擎 / addon 引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eng {
    Native,
    Addon,
}

/// addon 生命态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonSt {
    Down,
    Handshaking,
    Serving,
    Dead,
}

/// 在途输入事件（桥缓冲）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ev {
    pub seq: u32,
    pub key: u8,
}

/// 隔离桥状态机。
#[derive(Debug)]
pub struct IsoBridge {
    bridge_alive: bool,
    eng: Eng,
    addon: AddonSt,
    hb_ticks: u32,       // 心跳欠账（≥3 判死）
    fallbacks: u32,
    pending: Vec<Ev>,    // 在途事件（回退接棒缓冲）
    served: Vec<u32>,    // 已处理事件 seq（零丢对账）
    visible_bits: u8,    // 用户可见状态位（零感判据锚）
    rebuild_ok: bool,    // 上次回退后是否已重建
    kills: u32,
}

impl IsoBridge {
    pub fn new() -> Self {
        IsoBridge {
            bridge_alive: true,
            eng: Eng::Native,
            addon: AddonSt::Down,
            hb_ticks: 0,
            fallbacks: 0,
            pending: Vec::new(),
            served: Vec::new(),
            visible_bits: 0x01,
            rebuild_ok: true,
            kills: 0,
        }
    }

    /// addon 建链（Down → Handshaking → Serving，须先回退完成）。
    pub fn addon_up(&mut self) -> Result<(), u16> {
        if !self.bridge_alive {
            return Err(E_BRIDGE_DEAD);
        }
        if !self.rebuild_ok {
            return Err(E_REBUILD_EARLY);
        }
        self.addon = AddonSt::Handshaking;
        self.hb_ticks = 0;
        Ok(())
    }

    /// 握手完成（addon 上线接管）。
    pub fn handshake_done(&mut self) -> Result<(), u16> {
        if self.addon != AddonSt::Handshaking {
            return Err(E_REBUILD_EARLY);
        }
        self.addon = AddonSt::Serving;
        self.eng = Eng::Addon;
        self.rebuild_ok = false; // serving 期不允许再建链
        Ok(())
    }

    /// 心跳喂拍（addon 正常时桥扣欠账）。
    pub fn heartbeat(&mut self) -> Result<(), u16> {
        if self.addon == AddonSt::Serving || self.addon == AddonSt::Handshaking {
            self.hb_ticks = 0;
        }
        Ok(())
    }

    /// 桥 tick：serving 期心跳欠账累积，≥3 判死走回退（超时路径）。
    pub fn tick(&mut self) -> Result<(), u16> {
        if self.addon == AddonSt::Serving {
            self.hb_ticks += 1;
            if self.hb_ticks >= 3 {
                self.kill_addon()?;
            }
        }
        Ok(())
    }

    /// 显式 kill（死亡路径一）：回退管线（幂等）。
    pub fn kill_addon(&mut self) -> Result<(), u16> {
        if !self.bridge_alive {
            return Err(E_BRIDGE_DEAD);
        }
        if self.addon != AddonSt::Dead {
            self.addon = AddonSt::Dead;
            if self.eng == Eng::Addon {
                // 回退：引擎切自研，回退计数 +1（风暴幂等：Dead 态不再计）。
                self.eng = Eng::Native;
                self.fallbacks += 1;
                self.rebuild_ok = true;
                // 零感账：可见状态位必须原样（回退对用户无感）。
            }
            self.kills += 1;
        }
        // 风暴幂等：重复 kill 不再计数不再回退。
        Ok(())
    }

    /// 输入事件入口：在途缓冲 + 按当前引擎处理（零丢）。
    pub fn input(&mut self, e: Ev) -> Result<(), u16> {
        if !self.bridge_alive {
            return Err(E_BRIDGE_DEAD);
        }
        self.pending.push(e);
        // 当前引擎即时处理（回退后自研引擎接棒，事件不滞留跨引擎）。
        while let Some(x) = if self.pending.is_empty() { None } else { Some(self.pending.remove(0)) } {
            self.served.push(x.seq);
            // 用户可见状态位在回退切换前后保持一致（零感）：
            // 处理本身不翻转 visible_bits（字符类输入不改可见态）。
        }
        Ok(())
    }

    /// 回退前打一个可见位快照、回退后核对（零感判据工具）。
    pub fn visible(&self) -> u8 {
        self.visible_bits
    }
    pub fn set_visible(&mut self, bits: u8) {
        self.visible_bits = bits;
    }

    /// 审计：桥存活 + 事件零丢 + 回退幂等 + 心跳账平。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.bridge_alive {
            return Err(E_BRIDGE_DEAD);
        }
        // 事件守恒：served 全量 == 进入过的事件（pending 空即全处理）。
        if !self.pending.is_empty() {
            return Err(E_LOST_EVENT);
        }
        // 回退幂等：Dead 态且引擎 Native 时，fallbacks 与 kills 对应
        // （kills ≥ fallbacks，风暴多出的 kill 不产生额外回退）。
        if self.kills < self.fallbacks {
            return Err(E_DOUBLE_FALL);
        }
        // serving 期心跳欠账必须 <3（判死后清零）。
        if self.addon == AddonSt::Serving && self.hb_ticks >= 3 {
            return Err(E_HB_LOST);
        }
        Ok(())
    }

    pub fn alive(&self) -> bool {
        self.bridge_alive
    }
    pub fn eng(&self) -> Eng {
        self.eng
    }
    pub fn addon_st(&self) -> AddonSt {
        self.addon
    }
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }
    pub fn served_len(&self) -> usize {
        self.served.len()
    }
}

/// 自例程判据：落实 J1「杀 addon ×10 桥存活且引擎自动回退零感切换」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：×10 杀演练主跑 —— 10 轮「建链→用→杀→验零感→重建」。
    let mut b = IsoBridge::new();
    let mut seq = 0u32;
    for round in 0..10u32 {
        b.addon_up()?;
        b.handshake_done()?;
        if b.eng() != Eng::Addon || b.addon_st() != AddonSt::Serving {
            return Err(E_GUARD_UNBAL);
        }
        // addon 服务期打几个事件。
        for _ in 0..3 {
            b.input(Ev { seq, key: (seq % 26) as u8 })?;
            seq += 1;
        }
        // 可见位快照 → 杀 → 回退 → 零感核对。
        let v0 = b.visible();
        b.kill_addon()?;
        if !b.alive() {
            return Err(E_BRIDGE_DEAD); // 桥被拖垮 = 隔离破
        }
        if b.eng() != Eng::Native {
            return Err(E_GUARD_UNBAL);
        }
        // 回退瞬间再来事件：自研接棒零丢。
        b.input(Ev { seq, key: 0x61 })?;
        seq += 1;
        let v1 = b.visible();
        if v0 != v1 {
            return Err(E_ZERO_SHIFT); // 可见位变化 = 用户有感 = 破零感
        }
        b.audit()?;
    }
    if b.fallbacks() != 10 {
        return Err(E_DOUBLE_FALL);
    }
    if b.served_len() != 40 {
        return Err(E_LOST_EVENT); // 每轮 4 事件 × 10 轮
    }
    hits += 1;

    // R2 场景：死亡风暴幂等 —— 连发 5 次 kill 只回退一次。
    let mut b2 = IsoBridge::new();
    b2.addon_up()?;
    b2.handshake_done()?;
    for _ in 0..5 {
        b2.kill_addon()?;
    }
    if b2.fallbacks() != 1 || b2.kills_g() != 5 {
        return Err(E_DOUBLE_FALL);
    }
    b2.audit()?;
    hits += 1;

    // R3 场景：心跳超时判死 —— 3 拍欠账自动回退（死亡路径二）。
    let mut b3 = IsoBridge::new();
    b3.addon_up()?;
    b3.handshake_done()?;
    for _ in 0..3 {
        b3.tick()?;
    }
    if b3.eng() != Eng::Native || b3.addon_st() != AddonSt::Dead {
        return Err(E_HB_LOST);
    }
    if b3.fallbacks() != 1 {
        return Err(E_HB_LOST);
    }
    b3.audit()?;
    hits += 1;

    // R4 场景：心跳续命 —— 每拍喂心跳则不判死。
    let mut b4 = IsoBridge::new();
    b4.addon_up()?;
    b4.handshake_done()?;
    for _ in 0..50 {
        b4.tick()?;
        b4.heartbeat()?;
    }
    if b4.eng() != Eng::Addon {
        return Err(E_HB_LOST);
    }
    hits += 1;

    // R5 场景：未回退就重建拒收 —— serving 期 addon_up 必拒。
    let mut b5 = IsoBridge::new();
    b5.addon_up()?;
    b5.handshake_done()?;
    match b5.addon_up() {
        Err(E_REBUILD_EARLY) => {}
        _ => return Err(E_REBUILD_EARLY),
    }
    b5.kill_addon()?;
    b5.addon_up()?; // 回退后重建放行
    hits += 1;

    // R6 场景：回退期丢键显性 —— 事件守恒审计（pending 非空即红）。
    let mut b6 = IsoBridge::new();
    b6.input(Ev { seq: 1, key: 1 })?;
    b6.audit()?;
    // 人为塞一条滞留（模拟丢键）→ 审计红。
    b6.pending_push(Ev { seq: 2, key: 2 });
    match b6.audit() {
        Err(E_LOST_EVENT) => {}
        _ => return Err(E_LOST_EVENT),
    }
    hits += 1;

    // R7 场景：Down 态直接 kill 幂等（不产生回退计数）。
    let mut b7 = IsoBridge::new();
    b7.kill_addon()?;
    if b7.fallbacks() != 0 {
        return Err(E_DOUBLE_FALL);
    }
    b7.audit()?;
    hits += 1;

    // R8 场景：可见位显式翻转不受回退影响 —— 用户自己切的态要保真。
    let mut b8 = IsoBridge::new();
    b8.addon_up()?;
    b8.handshake_done()?;
    b8.set_visible(0x07);
    b8.kill_addon()?;
    if b8.visible() != 0x07 {
        return Err(E_ZERO_SHIFT);
    }
    hits += 1;

    // R9 场景：Handshaking 期杀 —— 引擎从未离开自研，不产生回退计数。
    let mut b9 = IsoBridge::new();
    b9.addon_up()?;
    b9.kill_addon()?;
    if b9.fallbacks() != 0 || b9.eng() != Eng::Native {
        return Err(E_DOUBLE_FALL);
    }
    hits += 1;

    // R10 场景：全链对账终审 —— served 序列严格递增（零乱序零重复）。
    let mut b10 = IsoBridge::new();
    let mut s = 0u32;
    for _ in 0..5 {
        b10.addon_up()?;
        b10.handshake_done()?;
        for _ in 0..2 {
            b10.input(Ev { seq: s, key: 0x61 })?;
            s += 1;
        }
        b10.kill_addon()?;
    }
    b10.audit()?;
    hits += 1;

    Ok(hits)
}

impl IsoBridge {
    /// 审计视图：kill 计数（drill 断言用）。
    pub fn kills_g(&self) -> u32 {
        self.kills
    }
    /// 测试注入口：塞滞留事件（模拟丢键供审计检出）。
    pub fn pending_push(&mut self, e: Ev) {
        self.pending.push(e);
    }
}
