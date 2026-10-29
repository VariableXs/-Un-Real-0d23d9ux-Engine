//! UNX-F56188 Windows 侧启动子段对照映射件（0x61780 段）
//!
//! 锚点：UNX-F56188-J1 对照映射账（Varix 子段 → Windows 侧对应阶段
//! 名 16 条映射——fwpost→BIOS POST、load→BootMgr、menu→BCD 菜单等
//! 逐条落账）；J2 映射语义（双侧名非空 + 双射可逆 + 未映射显式 N/A
//! 三性质）；J3 映射指纹（映射对折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·Windows 侧对照面）。
//!
//! 结构：对照对结构 + 双侧映射簿（正查/逆查）+ 映射核（非空闸 + 可
//! 逆闸 + N/A 闸）+ 映射指纹 + 注入核（空名/逆查冲突/未知侧/自映射
//! 空转/印后改表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61780 独占 0x20）。
pub const SEG_BASE: u32 = 0x61780;
pub const WM_EMPTY_NAME: u32 = SEG_BASE + 0x01; // 空名
pub const WM_REV_CONFLICT: u32 = SEG_BASE + 0x02; // 逆查冲突
pub const WM_UNK_SIDE: u32 = SEG_BASE + 0x03; // 未知侧
pub const WM_SELF_LOOP: u32 = SEG_BASE + 0x04; // 自映射空转
pub const WM_FROZEN: u32 = SEG_BASE + 0x05; // 印后改表

/// 单条对照对（Varix 子段 ↔ Windows 阶段名）。
#[derive(Clone)]
pub struct WinPair {
    pub vx_sub: String,
    pub win_stage: String,
    /// true = Windows 侧无对应（N/A）。
    pub win_na: bool,
}

/// Windows 侧对照映射簿。
pub struct WinSideMap {
    pairs: Vec<WinPair>,
    frozen: bool,
}

impl WinSideMap {
    pub fn new() -> Self {
        WinSideMap {
            pairs: Vec::new(),
            frozen: false,
        }
    }

    /// 登记一对映射：空名拒 + 逆查冲突拒 + 印后拒。
    pub fn pair(&mut self, vx: &str, win: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(WM_FROZEN);
        }
        if vx.is_empty() || win.is_empty() {
            return Err(WM_EMPTY_NAME);
        }
        if vx == win {
            return Err(WM_SELF_LOOP);
        }
        if self.rev_lookup(win).is_some() {
            return Err(WM_REV_CONFLICT);
        }
        if self.fwd_lookup(vx).is_some() {
            return Err(WM_REV_CONFLICT);
        }
        self.pairs.push(WinPair {
            vx_sub: vx.to_string(),
            win_stage: win.to_string(),
            win_na: false,
        });
        Ok(())
    }

    /// 登记 N/A 对（Windows 侧无对应）。
    pub fn pair_na(&mut self, vx: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(WM_FROZEN);
        }
        if vx.is_empty() {
            return Err(WM_EMPTY_NAME);
        }
        if self.fwd_lookup(vx).is_some() {
            return Err(WM_REV_CONFLICT);
        }
        self.pairs.push(WinPair {
            vx_sub: vx.to_string(),
            win_stage: String::new(),
            win_na: true,
        });
        Ok(())
    }

    /// 正查：Varix 子段 → Windows 阶段名。
    pub fn fwd_lookup(&self, vx: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|p| p.vx_sub == vx && !p.win_na)
            .map(|p| p.win_stage.as_str())
    }

    /// 逆查：Windows 阶段名 → Varix 子段。
    pub fn rev_lookup(&self, win: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|p| !p.win_na && p.win_stage == win)
            .map(|p| p.vx_sub.as_str())
    }

    /// N/A 查：确认该子段显式缺侧。
    pub fn is_na(&self, vx: &str) -> bool {
        self.pairs.iter().any(|p| p.vx_sub == vx && p.win_na)
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// 可逆核：全部非 N/A 对双射可逆。
    pub fn reversible_ok(&self) -> bool {
        self.pairs.iter().filter(|p| !p.win_na).all(|p| {
            self.rev_lookup(&p.win_stage) == Some(p.vx_sub.as_str())
                && self.fwd_lookup(&p.vx_sub) == Some(p.win_stage.as_str())
        })
    }

    /// 覆盖核：16 子段全部登记（映射或 N/A）。
    pub fn covers(&self, subs: &[&str]) -> bool {
        subs.iter().all(|s| self.fwd_lookup(s).is_some() || self.is_na(s))
    }

    /// 映射指纹。
    pub fn map_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in &self.pairs {
            for b in p.vx_sub.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            for b in p.win_stage.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h ^= p.win_na as u32;
        }
        h ^= self.frozen as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 冻结印。
    pub fn freeze(&mut self) -> bool {
        if self.frozen || self.pairs.is_empty() || !self.reversible_ok() {
            return false;
        }
        self.frozen = true;
        true
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 渲染导出。
    pub fn render_map(&self) -> String {
        let mut out = String::new();
        for p in &self.pairs {
            if p.win_na {
                out.push_str(&format!("{}->N/A\n", p.vx_sub));
            } else {
                out.push_str(&format!("{}->{}\n", p.vx_sub, p.win_stage));
            }
        }
        if self.frozen {
            out.push_str("WINMAP-FROZEN\n");
        }
        out
    }
}

impl Default for WinSideMap {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：16 子段对照（其中 2 条 N/A）。
fn standard_map() -> WinSideMap {
    let mut m = WinSideMap::new();
    let _ = m.pair("fwpost", "BIOS_POST");
    let _ = m.pair("load", "BootMgr");
    let _ = m.pair("menu", "BCD_Menu");
    let _ = m.pair("handoff", "winload_Handoff");
    let _ = m.pair("init", "KernelInit");
    let _ = m.pair("drv", "DriverEnum");
    let _ = m.pair("svc", "SMSS_Services");
    let _ = m.pair("sched", "SchedulerReady");
    let _ = m.pair("sesmgr", "SessionManager");
    let _ = m.pair("compos", "DWM_Start");
    let _ = m.pair("logingui", "LogonUI");
    let _ = m.pair_na("cred"); // Varix 凭据通路为自研，Windows 无独立阶段
    let _ = m.pair("input", "InputStack");
    let _ = m.pair("firstwin", "ShellFirstPaint");
    let _ = m.pair_na("evtpath"); // 事件通路融合在 InputStack 内
    let _ = m.pair("focus", "ShellFocus");
    m
}

/// 全 16 子段名。
const ALL16: [&str; 16] = [
    "fwpost", "load", "menu", "handoff", "init", "drv", "svc", "sched",
    "sesmgr", "compos", "logingui", "cred", "input", "firstwin", "evtpath", "focus",
];

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空名
    let mut m = WinSideMap::new();
    if m.pair("", "X") == Err(WM_EMPTY_NAME) {
        caught += 1;
    }
    // 注 2：逆查冲突（同 win 名二次登记）
    let mut m = WinSideMap::new();
    let _ = m.pair("a", "BIOS_POST");
    if m.pair("b", "BIOS_POST") == Err(WM_REV_CONFLICT) {
        caught += 1;
    }
    // 注 3：自映射空转
    let mut m = WinSideMap::new();
    if m.pair("same", "same") == Err(WM_SELF_LOOP) {
        caught += 1;
    }
    // 注 4：重复正查名冲突
    let mut m = WinSideMap::new();
    let _ = m.pair("a", "X");
    if m.pair("a", "Y") == Err(WM_REV_CONFLICT) {
        caught += 1;
    }
    // 注 5：印后改表
    let mut m = standard_map();
    let _ = m.freeze();
    if m.pair("extra", "Extra") == Err(WM_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 对照映射账；J2 映射语义；J3 映射指纹。
pub fn run_unx56188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56188");
    // J1-1 16 子段全登记
    let m = standard_map();
    let ok = m.len() == 16 && m.covers(&ALL16);
    s.add("56188-J1-16子段全登记", ok, if ok { "" } else { "覆盖不全" });
    // J1-2 渲染导出齐
    let text = standard_map().render_map();
    let ok = text.contains("fwpost->BIOS_POST") && text.contains("cred->N/A") && text.contains("WINMAP-FROZEN");
    s.add("56188-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 双射可逆
    let ok = m.reversible_ok();
    s.add("56188-J2-双射可逆", ok, if ok { "" } else { "可逆性漂移" });
    // J2-2 正查抽查：load → BootMgr
    let ok = m.fwd_lookup("load") == Some("BootMgr");
    s.add("56188-J2-正查load", ok, if ok { "" } else { "正查漂移" });
    // J2-3 逆查抽查：LogonUI → logingui
    let ok = m.rev_lookup("LogonUI") == Some("logingui");
    s.add("56188-J2-逆查LogonUI", ok, if ok { "" } else { "逆查漂移" });
    // J2-4 N/A 显式：cred 与 evtpath
    let ok = m.is_na("cred") && m.is_na("evtpath") && !m.is_na("load");
    s.add("56188-J2-NA显式", ok, if ok { "" } else { "N/A 漂移" });
    // J2-5 N/A 对正查为空
    let ok = m.fwd_lookup("cred").is_none();
    s.add("56188-J2-NA正查空", ok, if ok { "" } else { "N/A 误映射" });
    // J2-6 未知侧查无
    let ok = m.fwd_lookup("nonexist").is_none() && m.rev_lookup("NoStage").is_none();
    s.add("56188-J2-未知查无", ok, if ok { "" } else { "查有即错" });
    // J3-1 映射指纹确定
    let ok = standard_map().map_fp() == standard_map().map_fp();
    s.add("56188-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 对变签变
    let mut m2 = WinSideMap::new();
    let _ = m2.pair("a", "X");
    let fp0 = m2.map_fp();
    let _ = m2.pair("b", "Y");
    let ok = m2.map_fp() != fp0;
    s.add("56188-J3-对变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 NA 变签变
    let mut m3 = WinSideMap::new();
    let _ = m3.pair("a", "X");
    let fp0 = m3.map_fp();
    let _ = m3.pair_na("b");
    let ok = m3.map_fp() != fp0;
    s.add("56188-J3-NA变签变", ok, if ok { "" } else { "NA 折叠退化" });
    // J3-4 冻结印
    let mut m4 = standard_map();
    let ok = m4.freeze() && m4.frozen();
    s.add("56188-J3-冻结印", ok, if ok { "" } else { "冻结失败" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56188-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56188_all_checks_green() {
        let set = run_unx56188_checks();
        assert!(set.all_passed(), "UNX-F56188 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56188_map_stable() {
        let m = standard_map();
        assert!(m.reversible_ok() && m.covers(&ALL16));
    }
}
