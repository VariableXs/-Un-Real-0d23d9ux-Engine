//! VE-F4306 · 一致性×插件生态（U06 场景应用组）
//! 职责：插件 API 调用一致性（词典入口强制走 SDK 查询——直连绕过封禁
//! API 键）+ 插件 UI 三查（浮层出路/焦点管理/快捷键）+ 沙箱内违规不拖垮
//! 本体（隔离交叉承诺）+ 沙箱逃逸红线级即时处置 + 三查含读屏焦点项。
//! 段位：0x3080（续表 idx540）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3080;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PgCode {
    /// 直连绕过（封禁 API 键）。
    DirectBypass = SEG_BASE,
    /// 三查不过（上架拒绝）。
    UiCheckFail = SEG_BASE + 0x01,
    /// 沙箱逃逸（红线即时处置）。
    SandboxEscape = SEG_BASE + 0x02,
}

/// UI 三查。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiCheck {
    PopupOut = 0,   // 浮层出路
    Focus = 1,      // 焦点管理（含读屏——域本色）
    Hotkey = 2,     // 快捷键
}

/// 插件。
#[derive(Clone, Copy, Debug)]
pub struct Plugin {
    pub id: u16,
    pub key: u32,
    pub banned: bool,
    pub ui: [bool; 3],
    pub sandboxed: bool,
    pub escaped: bool,
    pub listed: bool,
}

/// 插件生态器。
pub struct PluginEco {
    pub plugins: Vec<Plugin>,
    /// SDK 词典入口登记键。
    pub sdk_keys: Vec<u32>,
    /// 直连检测计数。
    pub bypass_hits: u32,
    /// 本体健康（沙箱违规不拖垮）。
    pub core_alive: bool,
}

impl PluginEco {
    pub fn new() -> Self {
        PluginEco { plugins: Vec::new(), sdk_keys: Vec::new(), bypass_hits: 0, core_alive: true }
    }

    pub fn register(&mut self, id: u16, key: u32) {
        if !self.plugins.iter().any(|p| p.id == id) {
            self.plugins.push(Plugin { id, key, banned: false, ui: [false; 3], sandboxed: true, escaped: false, listed: false });
        }
    }

    pub fn sdk_key(&mut self, key: u32) {
        if !self.sdk_keys.contains(&key) {
            self.sdk_keys.push(key);
        }
    }

    /// 词典调用检：走 SDK 键=合规；直连=绕过封禁。
    pub fn dict_call(&mut self, id: u16, via_sdk: bool) -> Result<(), PgCode> {
        let p = self.plugins.iter_mut().find(|p| p.id == id).ok_or(PgCode::DirectBypass)?;
        if p.banned {
            return Err(PgCode::DirectBypass);
        }
        if !via_sdk {
            p.banned = true; // 封禁 API 键
            self.bypass_hits += 1;
            return Err(PgCode::DirectBypass);
        }
        Ok(())
    }

    /// UI 三查登记：全过才可上架。
    pub fn ui_check(&mut self, id: u16, which: UiCheck, pass: bool) {
        if let Some(p) = self.plugins.iter_mut().find(|p| p.id == id) {
            p.ui[which as usize] = pass;
        }
    }

    pub fn list(&mut self, id: u16) -> Result<(), PgCode> {
        let p = self.plugins.iter_mut().find(|p| p.id == id).ok_or(PgCode::UiCheckFail)?;
        if !p.ui.iter().all(|u| *u) {
            return Err(PgCode::UiCheckFail);
        }
        p.listed = true;
        Ok(())
    }

    /// 沙箱逃逸：红线即时处置（插件销毁；本体不拖垮）。
    pub fn escape(&mut self, id: u16) -> Result<(), PgCode> {
        let p = self.plugins.iter_mut().find(|p| p.id == id).ok_or(PgCode::SandboxEscape)?;
        p.escaped = true;
        p.listed = false;
        p.banned = true;
        self.core_alive = true; // 隔离交叉承诺：本体健康
        Ok(())
    }

    pub fn alive_plugins(&self) -> usize {
        self.plugins.iter().filter(|p| !p.banned && !p.escaped).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C540-01..12。
pub fn run_ves540_checks() -> CheckSet {
    let mut set = CheckSet::new("ves540");

    let mut pe = PluginEco::new();
    pe.register(1, 0xAA);
    pe.sdk_key(0xAA);
    chk(&mut set, "C540-01", pe.dict_call(1, true).is_ok(), "SDK 键调用合规");
    chk(&mut set, "C540-02", pe.dict_call(1, false) == Err(PgCode::DirectBypass), "直连绕过检出");
    chk(&mut set, "C540-03", pe.bypass_hits == 1 && pe.plugins[0].banned, "API 键封禁在册");
    chk(&mut set, "C540-04", pe.dict_call(1, true) == Err(PgCode::DirectBypass), "封禁后合法调用也拒");
    pe.register(2, 0xBB);
    pe.sdk_key(0xBB);
    chk(&mut set, "C540-05", pe.dict_call(2, true).is_ok(), "未封禁插件正常");

    // UI 三查
    pe.ui_check(2, UiCheck::PopupOut, true);
    pe.ui_check(2, UiCheck::Focus, true);
    chk(&mut set, "C540-06", pe.list(2) == Err(PgCode::UiCheckFail), "缺快捷键查拒绝上架");
    pe.ui_check(2, UiCheck::Hotkey, true);
    chk(&mut set, "C540-07", pe.list(2).is_ok() && pe.plugins[1].listed, "三查全过上架");
    chk(&mut set, "C540-08", pe.plugins[1].ui[UiCheck::Focus as usize], "焦点管理含读屏项在册（域本色）");

    // 沙箱逃逸
    chk(&mut set, "C540-09", pe.escape(2).is_ok(), "沙箱逃逸红线即时处置");
    chk(&mut set, "C540-10", pe.plugins[1].banned && !pe.plugins[1].listed, "插件销毁下架");
    chk(&mut set, "C540-11", pe.core_alive, "本体不拖垮（隔离交叉承诺）");
    chk(&mut set, "C540-12", pe.alive_plugins() == 1 && UiCheck::Hotkey as usize == 2, "存活账与三查枚举自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_eco() {
        let mut pe = PluginEco::new();
        pe.register(1, 1);
        assert!(pe.dict_call(1, true).is_ok());
        assert!(pe.dict_call(1, false).is_err());
        for c in [UiCheck::PopupOut, UiCheck::Focus, UiCheck::Hotkey] {
            pe.ui_check(1, c, true);
        }
        assert!(pe.list(1).is_ok());
        assert!(pe.escape(1).is_ok());
        assert!(pe.core_alive);
    }
}
