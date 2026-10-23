//! VE-F0126 · 服务生命周期与总线对接（VE-A 域 · A06 环境与系统集成组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0126`
//!
//! **职责定位（锚点原文）**：渲染服务生命周期的总线对接（启动/就绪/降级/关闭四态
//! 与 N 域服务总线对接），生命周期钩子的双向注册，服务死亡的通知义务。
//!
//! **铁律**：
//! 1. **四态对接**——启动/就绪/降级/关闭。
//! 2. **双向钩子**——上行下行钩子都可注册。
//! 3. **死亡通知**——服务死亡必须通知。
//! 4. **总线漂移拦截**——版本不符拒绝。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C4（生命周期对接）。
pub const SEG_BASE: u16 = 0xE8C4;

/// 总线漂移。
pub const XC_BUS_DRIFT: u16 = SEG_BASE | 0x01;
/// 钩子失效。
pub const XC_HOOK_DEAD: u16 = SEG_BASE | 0x02;
/// 死亡未通知（红线）。
pub const XC_DEATH_SILENT: u16 = SEG_BASE | 0x03;
/// 状态迁移非法。
pub const XC_BAD_TRANSITION: u16 = SEG_BASE | 0x04;

/// 生命周期四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LifeState {
    /// 启动中。
    Starting,
    /// 就绪。
    Ready,
    /// 降级。
    Degraded,
    /// 关闭。
    Shutdown,
}

impl LifeState {
    /// 合法迁移。
    pub fn can_transition(&self, to: &LifeState) -> bool {
        matches!(
            (self, to),
            (LifeState::Starting, LifeState::Ready)
                | (LifeState::Ready, LifeState::Degraded)
                | (LifeState::Degraded, LifeState::Ready)
                | (LifeState::Ready, LifeState::Shutdown)
                | (LifeState::Degraded, LifeState::Shutdown)
                | (LifeState::Starting, LifeState::Shutdown)
        )
    }
}

/// 钩子方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookDir {
    /// 上行（服务→总线）。
    Up,
    /// 下行（总线→服务）。
    Down,
}

/// 生命周期钩子。
#[derive(Clone, Debug)]
pub struct LifeHook {
    pub id: u32,
    pub dir: HookDir,
    /// 钩子健康。
    pub alive: bool,
}

/// 总线对接器。
pub struct BusAdapter {
    state: LifeState,
    hooks: Vec<LifeHook>,
    next_hook: u32,
    /// 总线协议版本。
    bus_version: u16,
    last_code: u16,
    /// 死亡通知计数。
    death_notices: u32,
    /// 状态迁移次数。
    transitions: u32,
}

impl BusAdapter {
    pub fn new(bus_version: u16) -> Result<Self, u16> {
        if bus_version == 0 {
            return Err(XC_BUS_DRIFT);
        }
        Ok(BusAdapter {
            state: LifeState::Starting,
            hooks: Vec::new(),
            next_hook: 1,
            bus_version,
            last_code: 0,
            death_notices: 0,
            transitions: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 注册双向钩子。
    pub fn register_hook(&mut self, dir: HookDir) -> u32 {
        let id = self.next_hook;
        self.next_hook += 1;
        self.hooks.push(LifeHook {
            id,
            dir,
            alive: true,
        });
        id
    }

    /// 状态迁移（四态合法迁移）。
    pub fn transition(&mut self, to: LifeState) -> Result<(), u16> {
        if !self.state.can_transition(&to) {
            self.last_code = XC_BAD_TRANSITION;
            return Err(XC_BAD_TRANSITION);
        }
        self.state = to;
        self.transitions += 1;
        self.last_code = 0;
        Ok(())
    }

    /// 钩子失效检测。
    pub fn check_hooks(&mut self) -> Result<(), u16> {
        if self.hooks.iter().any(|h| !h.alive) {
            self.last_code = XC_HOOK_DEAD;
            return Err(XC_HOOK_DEAD);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 服务死亡通知义务（红线：必须通知）。
    pub fn notify_death(&mut self, hook_id: u32) -> Result<(), u16> {
        match self.hooks.iter_mut().find(|h| h.id == hook_id) {
            Some(h) => {
                h.alive = false;
                self.death_notices += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_DEATH_SILENT;
                Err(XC_DEATH_SILENT)
            }
        }
    }

    /// 总线版本校验。
    pub fn verify_bus(&mut self, expected: u16) -> Result<(), u16> {
        if self.bus_version != expected {
            self.last_code = XC_BUS_DRIFT;
            return Err(XC_BUS_DRIFT);
        }
        self.last_code = 0;
        Ok(())
    }

    pub fn state(&self) -> LifeState {
        self.state
    }

    pub fn death_notices(&self) -> u32 {
        self.death_notices
    }

    pub fn transitions(&self) -> u32 {
        self.transitions
    }

    pub fn hook_count(&self) -> usize {
        self.hooks.len()
    }
}

/// 自检：四态对接、双向钩子、死亡通知、总线联动。
pub fn run_vea126_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea126_life");

    // C126-01 四态合法迁移
    {
        let mut b = BusAdapter::new(1).unwrap();
        let r = b.transition(LifeState::Ready);
        chk(&mut set, "C126-01 迁移", r.is_ok() && b.state() == LifeState::Ready, "迁移失败");
    }

    // C126-02 非法迁移拒绝
    {
        let mut b = BusAdapter::new(1).unwrap();
        let r = b.transition(LifeState::Shutdown);
        let r2 = b.transition(LifeState::Ready); // Shutdown→Ready 非法
        chk(
            &mut set,
            "C126-02 非法拒",
            r.is_ok() && r2.is_err() && b.last_code() == XC_BAD_TRANSITION,
            "非法被放",
        );
    }

    // C126-03 双向钩子
    {
        let mut b = BusAdapter::new(1).unwrap();
        let u = b.register_hook(HookDir::Up);
        let d = b.register_hook(HookDir::Down);
        chk(&mut set, "C126-03 双向", u != d && b.hook_count() == 2, "双向缺");
    }

    // C126-04 死亡通知
    {
        let mut b = BusAdapter::new(1).unwrap();
        let id = b.register_hook(HookDir::Up);
        let r = b.notify_death(id);
        chk(&mut set, "C126-04 死亡通知", r.is_ok() && b.death_notices() == 1, "未通知");
    }

    // C126-05 死亡未通知红线
    {
        let mut b = BusAdapter::new(1).unwrap();
        let r = b.notify_death(99);
        chk(
            &mut set,
            "C126-05 红线",
            r.is_err() && b.last_code() == XC_DEATH_SILENT,
            "红线被破",
        );
    }

    // C126-06 钩子失效检出
    {
        let mut b = BusAdapter::new(1).unwrap();
        let id = b.register_hook(HookDir::Up);
        let _ = b.notify_death(id);
        let r = b.check_hooks();
        chk(&mut set, "C126-06 失效检出", r.is_err() && b.last_code() == XC_HOOK_DEAD, "失效漏检");
    }

    // C126-07 总线漂移拦截
    {
        let mut b = BusAdapter::new(1).unwrap();
        let r = b.verify_bus(99);
        chk(
            &mut set,
            "C126-07 漂移拦截",
            r.is_err() && b.last_code() == XC_BUS_DRIFT,
            "漂移被放",
        );
    }

    // C126-08 版本零拒绝
    {
        let r = BusAdapter::new(0);
        chk(&mut set, "C126-08 零版本拒", r.is_err(), "零版本被放");
    }

    // C126-09 降级→就绪回迁
    {
        let mut b = BusAdapter::new(1).unwrap();
        let _ = b.transition(LifeState::Ready);
        let _ = b.transition(LifeState::Degraded);
        let r = b.transition(LifeState::Ready);
        chk(&mut set, "C126-09 回迁", r.is_ok() && b.state() == LifeState::Ready, "回迁失败");
    }

    // C126-10 迁移计数
    {
        let mut b = BusAdapter::new(1).unwrap();
        let _ = b.transition(LifeState::Ready);
        let _ = b.transition(LifeState::Shutdown);
        chk(&mut set, "C126-10 计数", b.transitions() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea126_all_green() {
        let s = run_vea126_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
