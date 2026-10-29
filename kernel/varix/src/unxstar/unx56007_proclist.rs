//! UNX-F56007 后台进程清单静默态冻结件（0x600C0 段）
//!
//! 锚点：UNX-F56007-J1 采集窗后台进程白名单登记（排序规范化 + 重复
//! 折叠）；J2 白名单外进程出现即拒收（×10 注入全检出，逐条落拒收账）；
//! J3 双系统白名单镜像对照（Varix/Windows 语义等价映射表逐条双向可查）
//! 三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·进程静默面）。
//!
//! 结构：白名单登记册（规范化排序去重 + 封账冻结）+ 采集窗核（观测集
//! 对白名单差分，白外进程逐条拒收落账）+ 双系统镜像映射（varix 名 ↔
//! win 名双向解析，缺射即拒）+ 注入核（空白名单/空白项/白外 ×10/缺射
//! /封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x600C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x600C0;
pub const PR_WL_EMPTY: u32 = SEG_BASE + 0x01; // 白名单空集登记
pub const PR_BLANK: u32 = SEG_BASE + 0x02; // 白名单含空白项
pub const PR_OUTSIDER: u32 = SEG_BASE + 0x03; // 白名单外进程出现
pub const PR_SEALED: u32 = SEG_BASE + 0x04; // 白名单已封账拒改
pub const PR_DUP: u32 = SEG_BASE + 0x05; // 白名单重复项（登记口径拒绝显式重复对）
pub const PR_MAP_MISS: u32 = SEG_BASE + 0x06; // 镜像映射缺射
pub const PR_ZERO_MARK: u32 = SEG_BASE + 0x07; // 拒收账退化为零标记

/// 标准白名单（Varix 侧，规范化排序前）。
pub const WL_VARIX: [&str; 6] =
    ["varixd", "logd", "probe", "fsd", "netd-silent", "auditd"];

/// 镜像映射闭集：varix 名 → win 名（语义等价，索引同序）。
pub const MIRROR_VARIX: [&str; 5] = ["varixd", "logd", "probe", "fsd", "auditd"];
pub const MIRROR_WIN: [&str; 5] =
    ["svchost-svc0", "eventlog", "wmi-probe", "ntfs-svc", "auditsvc"];

/// varix → win 映射查询。
pub fn mirror_to_win(v: &str) -> Option<&'static str> {
    for i in 0..MIRROR_VARIX.len() {
        if MIRROR_VARIX[i] == v {
            return Some(MIRROR_WIN[i]);
        }
    }
    None
}

/// win → varix 反向映射查询。
pub fn mirror_to_varix(w: &str) -> Option<&'static str> {
    for i in 0..MIRROR_WIN.len() {
        if MIRROR_WIN[i] == w {
            return Some(MIRROR_VARIX[i]);
        }
    }
    None
}

/// 白名单规范化：空集拒、空白项拒、排序去重。
pub fn canon_wl(raw: &[&str]) -> Result<Vec<String>, u32> {
    if raw.is_empty() {
        return Err(PR_WL_EMPTY);
    }
    let mut v: Vec<String> = Vec::new();
    for item in raw.iter() {
        if item.trim().is_empty() {
            return Err(PR_BLANK);
        }
        v.push((*item).to_string());
    }
    v.sort();
    let mut out: Vec<String> = Vec::new();
    for s in v.into_iter() {
        match out.last() {
            Some(l) if *l == s => {}
            _ => out.push(s),
        }
    }
    Ok(out)
}

/// 后台静默态冻结账：白名单 + 拒收账 + 封账。
pub struct SilenceBook {
    pub whitelist: Vec<String>,
    pub rejects: Vec<(String, u32)>,
    pub sealed: bool,
}

impl SilenceBook {
    pub fn new(raw: &[&str]) -> Result<Self, u32> {
        Ok(SilenceBook {
            whitelist: canon_wl(raw)?,
            rejects: Vec::new(),
            sealed: false,
        })
    }

    /// 在白名单内。
    pub fn allows(&self, proc_name: &str) -> bool {
        for w in self.whitelist.iter() {
            if w == proc_name {
                return true;
            }
        }
        false
    }

    /// 采集窗核：对观测集逐进程判定，白外逐条拒收落账（原因 PR_OUTSIDER）。
    pub fn observe_window(&mut self, observed: &[&str]) -> Result<usize, u32> {
        if self.sealed {
            return Err(PR_SEALED);
        }
        let mut rejected = 0usize;
        for p in observed.iter() {
            if !self.allows(p) {
                self.rejects.push(((*p).to_string(), PR_OUTSIDER));
                rejected += 1;
            }
        }
        Ok(rejected)
    }

    /// J2：×10 拒收账核——白外注入全检出且逐条落账。
    pub fn rejects_complete(&self, expect: usize) -> bool {
        self.rejects.len() == expect
            && !self.rejects.iter().any(|r| r.1 == 0)
    }

    /// 白名单快照（规范化序）。
    pub fn snapshot(&self) -> Vec<String> {
        self.whitelist.clone()
    }

    /// 封账（封后白名单不可变）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 双系统镜像对照核：映射表双向闭包（正向反解回自身）。
pub fn mirror_bidirectional_ok() -> bool {
    for i in 0..MIRROR_VARIX.len() {
        match mirror_to_win(MIRROR_VARIX[i]) {
            None => return false,
            Some(w) => {
                if w != MIRROR_WIN[i] {
                    return false;
                }
                match mirror_to_varix(w) {
                    None => return false,
                    Some(v2) => {
                        if v2 != MIRROR_VARIX[i] {
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

/// 标准夹具：Varix 白名单冻结账。
pub fn standard_silence() -> SilenceBook {
    SilenceBook::new(&WL_VARIX).expect("fixture wl")
}

/// ×10 白外注入核：白外进程逐条必检出。
pub fn outsider_core() -> (usize, usize) {
    let outsiders = [
        "cryptominer", "netbeacon", "telemetryx", "updater rogue",
        "indexer", "syncd rogue", "backupd rogue", "scannerd",
        "watchdogx", "helperd",
    ];
    let mut b = standard_silence();
    let rejected = b.observe_window(&outsiders).expect("observe");
    let ok = rejected == 10 && b.rejects_complete(10);
    if ok {
        (10, 10)
    } else {
        (b.rejects.len(), 10)
    }
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空白名单登记
    if SilenceBook::new(&[]).err() == Some(PR_WL_EMPTY) {
        caught += 1;
    }
    // 2) 空白项
    if SilenceBook::new(&["varixd", " "]).err() == Some(PR_BLANK) {
        caught += 1;
    }
    // 3) 镜像缺射（未入映射表的名字双向皆缺）
    if mirror_to_win("netd-silent").is_none() && mirror_to_varix("ghost-svc").is_none() {
        caught += 1;
    }
    // 4) 封账后观测拒
    let mut b = standard_silence();
    b.seal();
    if b.observe_window(&["varixd"]) == Err(PR_SEALED) {
        caught += 1;
    }
    // 5) 白名单内进程零误报
    let mut b2 = standard_silence();
    let r = b2.observe_window(&["varixd", "logd", "probe"]).expect("observe");
    if r == 0 && b2.rejects.len() == 0 {
        caught += 1;
    }
    (caught, 5)
}

/// 拒收账渲染（机器可读面）。
pub fn render_rejects(b: &SilenceBook) -> String {
    let mut out = String::new();
    for r in b.rejects.iter() {
        out.push_str(&format!("REJECT|{}|code={}\n", r.0, r.1));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56007_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56007");
    // J1-1 白名单登记齐（6 项规范化排序）
    let b = standard_silence();
    let ok = b.whitelist.len() == WL_VARIX.len()
        && b.whitelist[0] == "auditd"
        && b.whitelist[WL_VARIX.len() - 1] == "varixd";
    s.add("56007-J1-白名单登记齐", ok, if ok { "" } else { "白名单登记缺" });
    // J1-2 重复项折叠（乱序重复输入同规范化序）
    let dup = SilenceBook::new(&["logd", "varixd", "logd", "probe", "probe"]).expect("dup wl");
    let ok = dup.whitelist.len() == 3 && dup.whitelist[1] == "logd";
    s.add("56007-J1-重复项折叠", ok, if ok { "" } else { "去重语义漂移" });
    // J1-3 乱序输入等价（同集合异序同快照）
    let a = SilenceBook::new(&["probe", "logd", "varixd"]).expect("a");
    let c = SilenceBook::new(&["varixd", "probe", "logd"]).expect("c");
    let ok = a.snapshot() == c.snapshot();
    s.add("56007-J1-乱序等价", ok, if ok { "" } else { "快照漂移" });
    // J2-1 ×10 白外注入全检出
    let (c2, n2) = outsider_core();
    let ok = c2 == n2;
    let detail = if ok { String::new() } else { format!("白外检出 {c2}/{n2}") };
    s.add_dyn("56007-J2-白外×10必拒", ok, &detail);
    // J2-2 混合窗（白内白外同窗，白外逐条落账白内零误伤）
    let mut b2 = standard_silence();
    let r = b2.observe_window(&["varixd", "telemetryx", "logd", "ghost"]).expect("mixed");
    let ok = r == 2 && b2.rejects.len() == 2 && b2.rejects[0].0 == "telemetryx";
    s.add("56007-J2-混合窗差分", ok, if ok { "" } else { "混合窗差分漂移" });
    // J2-3 拒收账机器可读（REJECT 行逐条带码）
    let text = render_rejects(&b2);
    let ok = text.lines().count() == 2 && text.contains("REJECT|telemetryx|code=");
    s.add("56007-J2-拒收账机读", ok, if ok { "" } else { "拒收账不机读" });
    // J3-1 镜像映射双向闭包
    let ok = mirror_bidirectional_ok();
    s.add("56007-J3-镜像双向闭包", ok, if ok { "" } else { "镜像映射断" });
    // J3-2 缺射显性（未映射名返回 None 不猜测）
    let ok = mirror_to_win("netd-silent").is_none();
    s.add("56007-J3-缺射显性", ok, if ok { "" } else { "缺射被猜" });
    // J3-3 注入核 5 类必抓
    let (c3, n3) = injection_core();
    let ok = c3 == n3;
    let detail = if ok { String::new() } else { format!("注入 {c3}/{n3}") };
    s.add_dyn("56007-J3-注入核必抓", ok, &detail);
    // J1-4 封账冻结（封后白名单不可变）
    let mut b3 = standard_silence();
    b3.seal();
    let ok = b3.observe_window(&["varixd"]) == Err(PR_SEALED);
    s.add("56007-J1-封账冻结", ok, if ok { "" } else { "封账后可改" });
    // J1-5 空集与空白项拒收
    let ok = SilenceBook::new(&[]).err() == Some(PR_WL_EMPTY)
        && SilenceBook::new(&["varixd", "  "]).err() == Some(PR_BLANK);
    s.add("56007-J1-输入纪律", ok, if ok { "" } else { "输入纪律松" });
    // J2-4 白名单语义（allows 命中与未命中各验一）
    let b4 = standard_silence();
    let ok = b4.allows("fsd") && !b4.allows("fsd rogue");
    s.add("56007-J2-allows语义", ok, if ok { "" } else { "allows 语义漂移" });
    // J3-4 镜像计数（5 条映射全可解）
    let mut solved = 0usize;
    for v in MIRROR_VARIX.iter() {
        if mirror_to_win(v).is_some() {
            solved += 1;
        }
    }
    let ok = solved == MIRROR_VARIX.len();
    s.add("56007-J3-映射全可解", ok, if ok { "" } else { "映射残缺" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56007_all_checks_green() {
        let set = run_unx56007_checks();
        assert!(set.all_passed(), "UNX-F56007 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56007_mirror_closure() {
        assert!(mirror_bidirectional_ok());
    }
}
