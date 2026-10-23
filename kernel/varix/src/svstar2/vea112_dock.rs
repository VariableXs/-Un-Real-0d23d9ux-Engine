//! VE-F0112 · 外接 GPU 坞热迁移（VE-A 域 · A05 多 GPU 与异构组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0112`
//!
//! **职责定位（锚点原文）**：外接 GPU 坞热迁移——外接坞插拔的热迁移（资源迁入/
//! 迁出坞卡），迁移窗口（插拔瞬间的保护），迁移的用户告知。
//!
//! **铁律**：
//! 1. **热迁移**——插拔不中断渲染，资源自动迁入/迁出。
//! 2. **迁移窗口**——插拔瞬间保护期，禁止高危操作。
//! 3. **用户告知**——迁移事件三要素告知。
//! 4. **失败回退**——迁移失败留在原卡降级。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B6（坞热迁移）。
pub const SEG_BASE: u16 = 0xE8B6;

/// 迁移窗口冲突。
pub const XC_WINDOW_CONFLICT: u16 = SEG_BASE | 0x01;
/// 坞卡不可用。
pub const XC_DOCK_UNAVAILABLE: u16 = SEG_BASE | 0x02;
/// 迁移失败。
pub const XC_MIGRATE_FAIL: u16 = SEG_BASE | 0x03;
/// 告知失败。
pub const XC_NOTIFY_FAIL: u16 = SEG_BASE | 0x04;

/// 坞事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockEvent {
    /// 插入。
    Attach,
    /// 拔出。
    Detach,
}

/// 迁移窗口（保护期）。
#[derive(Clone, Copy, Debug)]
pub struct MigWindow {
    pub open_tick: u64,
    pub close_tick: u64,
}

impl MigWindow {
    /// 是否在窗口内。
    pub fn contains(&self, tick: u64) -> bool {
        tick >= self.open_tick && tick < self.close_tick
    }
}

/// 迁移项。
#[derive(Clone, Copy, Debug)]
pub struct MigratingRes {
    pub id: u32,
    pub bytes: u64,
    /// 0=待迁 1=迁入 2=迁出 3=失败回退。
    pub state: u8,
}

/// 用户告知（三要素）。
#[derive(Clone, Debug)]
pub struct DockNotice {
    pub what: u16,
    pub why: u16,
    pub howto: u16,
    pub tick: u64,
}

/// 坞热迁移管理器。
pub struct DockMigrator {
    /// 坞卡在线。
    dock_online: bool,
    /// 迁移窗口。
    window: Option<MigWindow>,
    resources: Vec<MigratingRes>,
    notices: Vec<DockNotice>,
    last_code: u16,
    /// 成功迁入。
    migrated_in: u32,
    /// 成功迁出。
    migrated_out: u32,
    /// 回退。
    fell_back: u32,
    /// 窗口冲突拒绝数。
    window_blocks: u32,
}

impl DockMigrator {
    pub fn new() -> Self {
        DockMigrator {
            dock_online: false,
            window: None,
            resources: Vec::new(),
            notices: Vec::new(),
            last_code: 0,
            migrated_in: 0,
            migrated_out: 0,
            fell_back: 0,
            window_blocks: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn register_res(&mut self, id: u32, bytes: u64) {
        self.resources.retain(|r| r.id != id);
        self.resources.push(MigratingRes {
            id,
            bytes,
            state: 0,
        });
    }

    /// 坞事件：开迁移窗口。
    pub fn on_dock_event(&mut self, ev: DockEvent, tick: u64) -> Result<(), u16> {
        self.dock_online = ev == DockEvent::Attach;
        self.window = Some(MigWindow {
            open_tick: tick,
            close_tick: tick + 10,
        });
        // 三要素告知。
        let (what, why, howto) = match ev {
            DockEvent::Attach => (0x10, 0x20, 0x30),
            DockEvent::Detach => (0x11, 0x21, 0x31),
        };
        self.notices.push(DockNotice {
            what,
            why,
            howto,
            tick,
        });
        self.last_code = 0;
        Ok(())
    }

    /// 迁移资源（窗口内）。
    pub fn migrate_res(&mut self, id: u32, tick: u64) -> Result<u8, u16> {
        // 窗口外禁止高危迁移。
        match &self.window {
            Some(w) if w.contains(tick) => {}
            _ => {
                self.window_blocks += 1;
                self.last_code = XC_WINDOW_CONFLICT;
                return Err(XC_WINDOW_CONFLICT);
            }
        }
        if !self.dock_online {
            // 迁出方向。
            match self.resources.iter_mut().find(|r| r.id == id) {
                Some(r) => {
                    r.state = 2;
                    self.migrated_out += 1;
                    self.last_code = 0;
                    Ok(2)
                }
                None => {
                    self.last_code = XC_MIGRATE_FAIL;
                    Err(XC_MIGRATE_FAIL)
                }
            }
        } else {
            match self.resources.iter_mut().find(|r| r.id == id) {
                Some(r) => {
                    r.state = 1;
                    self.migrated_in += 1;
                    self.last_code = 0;
                    Ok(1)
                }
                None => {
                    self.last_code = XC_MIGRATE_FAIL;
                    Err(XC_MIGRATE_FAIL)
                }
            }
        }
    }

    /// 迁移失败回退。
    pub fn fallback(&mut self, id: u32) -> Result<(), u16> {
        match self.resources.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                r.state = 3;
                self.fell_back += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_MIGRATE_FAIL;
                Err(XC_MIGRATE_FAIL)
            }
        }
    }

    pub fn notices(&self) -> &[DockNotice] {
        &self.notices
    }

    pub fn migrated_in(&self) -> u32 {
        self.migrated_in
    }

    pub fn migrated_out(&self) -> u32 {
        self.migrated_out
    }

    pub fn fell_back(&self) -> u32 {
        self.fell_back
    }

    pub fn window_blocks(&self) -> u32 {
        self.window_blocks
    }

    pub fn is_dock_online(&self) -> bool {
        self.dock_online
    }

    pub fn res_state(&self, id: u32) -> Option<u8> {
        self.resources.iter().find(|r| r.id == id).map(|r| r.state)
    }
}

impl Default for DockMigrator {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：热迁移、迁移窗口、用户告知、失败回退。
pub fn run_vea112_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea112_dock");

    // C112-01 插入热迁移
    {
        let mut d = DockMigrator::new();
        d.register_res(1, 100);
        let _ = d.on_dock_event(DockEvent::Attach, 10);
        let r = d.migrate_res(1, 12);
        chk(&mut set, "C112-01 热迁入", r == Ok(1) && d.migrated_in() == 1, "迁入失败");
    }

    // C112-02 拔出迁出
    {
        let mut d = DockMigrator::new();
        d.register_res(1, 100);
        let _ = d.on_dock_event(DockEvent::Detach, 10);
        let r = d.migrate_res(1, 12);
        chk(&mut set, "C112-02 迁出", r == Ok(2) && d.migrated_out() == 1, "迁出失败");
    }

    // C112-03 窗口外拒绝
    {
        let mut d = DockMigrator::new();
        d.register_res(1, 100);
        let _ = d.on_dock_event(DockEvent::Attach, 10);
        let r = d.migrate_res(1, 99); // 窗口外
        chk(
            &mut set,
            "C112-03 窗口保护",
            r.is_err() && d.window_blocks() == 1,
            "窗口外被放",
        );
    }

    // C112-04 三要素告知
    {
        let mut d = DockMigrator::new();
        let _ = d.on_dock_event(DockEvent::Attach, 1);
        let n = &d.notices()[0];
        let ok = n.what != 0 && n.why != 0 && n.howto != 0;
        chk(&mut set, "C112-04 三要素", ok, "告知不全");
    }

    // C112-05 失败回退
    {
        let mut d = DockMigrator::new();
        d.register_res(1, 100);
        let r = d.fallback(1);
        chk(&mut set, "C112-05 回退", r.is_ok() && d.fell_back() == 1, "回退失败");
    }

    // C112-06 未知资源拒绝
    {
        let mut d = DockMigrator::new();
        let r = d.fallback(99);
        chk(&mut set, "C112-06 未知拒", r.is_err(), "未知被放");
    }

    // C112-07 坞状态可查
    {
        let mut d = DockMigrator::new();
        let _ = d.on_dock_event(DockEvent::Attach, 1);
        chk(&mut set, "C112-07 坞状态", d.is_dock_online(), "状态错");
    }

    // C112-08 窗口时效
    {
        let mut d = DockMigrator::new();
        d.register_res(1, 10);
        let _ = d.on_dock_event(DockEvent::Attach, 100);
        let r = d.migrate_res(1, 115); // 窗口 close=110
        chk(&mut set, "C112-08 窗口时效", r.is_err(), "过期窗口被用");
    }

    // C112-09 状态可查
    {
        let mut d = DockMigrator::new();
        d.register_res(5, 10);
        let _ = d.on_dock_event(DockEvent::Attach, 1);
        let _ = d.migrate_res(5, 2);
        chk(&mut set, "C112-09 状态查询", d.res_state(5) == Some(1), "状态不可查");
    }

    // C112-10 重复注册覆盖
    {
        let mut d = DockMigrator::new();
        d.register_res(1, 10);
        d.register_res(1, 20);
        chk(&mut set, "C112-10 重复覆盖", d.res_state(1) == Some(0), "未覆盖");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea112_all_green() {
        let s = run_vea112_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
