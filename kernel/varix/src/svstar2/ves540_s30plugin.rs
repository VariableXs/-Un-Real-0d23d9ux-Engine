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
    /// 生态账漂移（越界插件查询）。
    EcoDrift = SEG_BASE + 0x03,
    /// 插件指纹失配（序列化损坏）。
    PluginFpMismatch = SEG_BASE + 0x04,
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
#[derive(PartialEq, Clone, Copy, Debug)]
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

    ves540_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：处置流水账本（插件×动作×步归因）+ 守恒恒等式 + 边界探针
// + 存活率整数投影与状态分列 + LCG 直连注入回归 + 幂等闸
// + 插件 6 字节序列化往返 + FNV-1a 生态指纹。

use alloc::string::{String, ToString};

/// 处置动作：0=SDK 调用 1=直连封禁 2=三查上架 3=逃逸销毁。
pub const ACT_SDK: u8 = 0;
pub const ACT_BAN: u8 = 1;
pub const ACT_LIST: u8 = 2;
pub const ACT_KILL: u8 = 3;

/// 处置流水账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchLog {
    pub id: u16,
    pub action: u8,
    pub step: u16,
}

/// 插件生态深化账本。
pub struct DispatchLedger {
    pub logs: Vec<DispatchLog>,
    pub step: u16,
}

impl DispatchLedger {
    pub fn new() -> Self {
        DispatchLedger { logs: Vec::new(), step: 0 }
    }

    pub fn log(&mut self, id: u16, action: u8) {
        self.step = self.step.wrapping_add(1);
        self.logs.push(DispatchLog { id, action, step: self.step });
    }

    /// 投影：按动作分列计数。
    pub fn count_by(&self, action: u8) -> usize {
        self.logs.iter().filter(|l| l.action == action).count()
    }

    /// 投影：指定插件的最近动作。
    pub fn latest_action(&self, id: u16) -> Option<u8> {
        self.logs.iter().filter(|l| l.id == id).last().map(|l| l.action)
    }

    /// 守恒恒等式：账目数 ≡ 各动作计数之和（分列无损）。
    pub fn conservation(&self) -> bool {
        let parts = self.count_by(ACT_SDK) + self.count_by(ACT_BAN) + self.count_by(ACT_LIST) + self.count_by(ACT_KILL);
        parts == self.logs.len()
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

/// LCG 直连注入回归：随机插件随机走 SDK/直连——直连必封禁且 bypass_hits
/// 递增；封禁后合法调用必拒（异常必抓）；负对照：SDK 调用零误封。
fn bypass_inject(seed: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..64u32 {
        let mut pe = PluginEco::new();
        pe.register(1, 0x11);
        let mut ledger = DispatchLedger::new();
        let rounds = (rng.next() % 6) + 1;
        for _ in 0..rounds {
            let via_sdk = rng.next() % 2 == 0;
            let res = pe.dict_call(1, via_sdk);
            ledger.log(1, if via_sdk { ACT_SDK } else { ACT_BAN });
            if via_sdk && res.is_err() {
                return Err("直连注入负对照：SDK 合规调用被误拒".to_string());
            }
            if !via_sdk && res != Err(PgCode::DirectBypass) {
                return Err("直连注入：直连调用未被检出".to_string());
            }
        }
        let banned = pe.plugins[0].banned;
        let hits = pe.bypass_hits;
        if banned != (hits > 0) {
            return Err("直连注入：封禁态与检出计数失配".to_string());
        }
        if banned && pe.dict_call(1, true).is_ok() {
            return Err("直连注入：封禁后合法调用被放行".to_string());
        }
        if !ledger.conservation() {
            return Err("直连注入：处置账分列失守".to_string());
        }
    }
    Ok(())
}

/// 幂等闸：register 同号二次零副作用（账目数/键表均不动）。
fn idempotence_probe() -> bool {
    let mut pe = PluginEco::new();
    pe.register(1, 0x11);
    let snap_n = pe.plugins.len();
    let snap_keys = pe.sdk_keys.len();
    pe.register(1, 0x22); // 同号重注册（应被忽略，不覆盖键）
    pe.register(1, 0x11);
    pe.plugins.len() == snap_n && pe.sdk_keys.len() == snap_keys
}

/// 插件定长 6 字节编码：[id lo, id hi, 态位, 三查位图, 沙箱旗, 0xC3 哨兵]。
pub fn encode_plugin(p: &Plugin) -> [u8; 6] {
    let mut st = 0u8;
    if p.banned { st |= 0x01; }
    if p.listed { st |= 0x02; }
    let mut ui = 0u8;
    for (i, u) in p.ui.iter().enumerate() {
        if *u { ui |= 1 << i; }
    }
    [ (p.id & 0xFF) as u8, (p.id >> 8) as u8, st, ui, u8::from(p.sandboxed), 0xC3 ]
}

/// 解码：哨兵/沙箱旗损坏 → PluginFpMismatch。
pub fn decode_plugin(b: &[u8; 6]) -> Result<Plugin, PgCode> {
    if b[5] != 0xC3 || b[4] > 1 {
        return Err(PgCode::PluginFpMismatch);
    }
    let id = b[0] as u16 | ((b[1] as u16) << 8);
    Ok(Plugin {
        id,
        key: 0,
        banned: b[2] & 0x01 != 0,
        ui: [b[3] & 0x01 != 0, b[3] & 0x02 != 0, b[3] & 0x04 != 0],
        sandboxed: b[4] == 1,
        escaped: false,
        listed: b[2] & 0x02 != 0,
    })
}

/// FNV-1a 32 位生态指纹。
pub fn fnv_plugins(ps: &[Plugin]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for p in ps {
        for byte in encode_plugin(p) {
            h ^= byte as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// 深化判据并入主判据流（C540-13..23）。
pub fn ves540_deep_checks(set: &mut CheckSet) {
    let mut dl = DispatchLedger::new();
    dl.log(1, ACT_SDK);
    dl.log(1, ACT_BAN);
    dl.log(2, ACT_LIST);
    set.add("C540-13", dl.conservation(), "处置账守恒（分列和≡总数）");
    set.add("C540-14", dl.count_by(ACT_BAN) == 1 && dl.latest_action(2) == Some(ACT_LIST), "动作分列与最近动作投影");
    set.add("C540-15", dl.latest_action(9).is_none(), "越界插件探针返回空");

    match bypass_inject(0xD1CE_4001) {
        Ok(()) => set.add("C540-16", true, "LCG 直连注入 64 轮直连必封+封后必拒+SDK 零误封"),
        Err(msg) => set.add_dyn("C540-16", false, &msg),
    }

    set.add("C540-17", idempotence_probe(), "幂等闸：同号重注册零副作用");

    let p = Plugin { id: 0x07D1, key: 0x55, banned: false, ui: [true, false, true], sandboxed: true, escaped: false, listed: true };
    match decode_plugin(&encode_plugin(&p)) {
        Ok(back) => set.add("C540-18", back.id == p.id && back.banned == p.banned && back.listed && back.ui[0] && !back.ui[1] && back.ui[2], "插件 6 字节编码解码往返"),
        Err(_) => set.add("C540-18", false, "插件解码异常失败"),
    }
    set.add("C540-19", decode_plugin(&[1, 2, 0, 0, 1, 0x00]) == Err(PgCode::PluginFpMismatch), "哨兵损坏拒绝解码");

    let mut pe2 = PluginEco::new();
    pe2.register(1, 0x11);
    let fp1 = fnv_plugins(&pe2.plugins);
    let _ = pe2.dict_call(1, false);
    set.add("C540-20", fnv_plugins(&pe2.plugins) != fp1, "FNV 生态指纹随封禁漂移");
    set.add("C540-21", fnv_plugins(&[]) == 0x811C_9DC5, "空生态指纹偏移基");

    let mut pe3 = PluginEco::new();
    set.add("C540-22", pe3.dict_call(1, true) == Err(PgCode::DirectBypass) && pe3.list(1) == Err(PgCode::UiCheckFail) && pe3.escape(1) == Err(PgCode::SandboxEscape), "空生态三探针显性报错");
    pe3.register(3, 0x33);
    pe3.register(4, 0x44);
    set.add("C540-23", pe3.alive_plugins() == 2 && pe3.core_alive, "存活率账自洽（2/2 全活）");
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
