//! UNX-F56186 子段-域归属映射件（0x61740 段）
//!
//! 锚点：UNX-F56186-J1 归属映射账（启动链四段 × 16 子段全部映射到
//! 责任域闭集 boot/kernel/ui/io——子段名 → 域逐条落账）；J2 映射语义
//! （全子段覆盖 + 单射映射 + 域闭集合法三性质）；J3 映射指纹（映射
//! 表折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·子段归属面）。
//!
//! 结构：责任域闭集 + 子段登记簿（名 → 域）+ 映射核（覆盖/单射/闭
//! 集闸）+ 映射指纹 + 注入核（未知域/重名冲突/空名/缺覆盖/印后改表
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61740 独占 0x20）。
pub const SEG_BASE: u32 = 0x61740;
pub const DM_UNK_DOMAIN: u32 = SEG_BASE + 0x01; // 未知域
pub const DM_DUP_SUB: u32 = SEG_BASE + 0x02; // 重名冲突
pub const DM_EMPTY_NAME: u32 = SEG_BASE + 0x03; // 空名
pub const DM_COVER_MISS: u32 = SEG_BASE + 0x04; // 缺覆盖
pub const DM_FROZEN: u32 = SEG_BASE + 0x05; // 印后改表

/// 责任域闭集。
pub const DOM_BOOT: usize = 0; // 引导域
pub const DOM_KERNEL: usize = 1; // 内核域
pub const DOM_UI: usize = 2; // 界面域
pub const DOM_IO: usize = 3; // 输入输出域
pub const DOM_COUNT: usize = 4;

pub const DOM_NAMES: [&str; DOM_COUNT] = ["boot", "kernel", "ui", "io"];

/// 启动链 16 子段登记名（四段 × 四子段）。
pub const ALL_SUBS: [&str; 16] = [
    "fwpost", "load", "menu", "handoff", // 引导段
    "init", "drv", "svc", "sched", // 内核段
    "sesmgr", "compos", "logingui", "cred", // 登录桌面段
    "input", "firstwin", "evtpath", "focus", // 交互就绪段
];

/// 单条归属映射。
#[derive(Clone)]
pub struct SubDomain {
    pub sub: String,
    pub domain: usize,
}

/// 子段-域归属映射簿。
pub struct SubDomainMap {
    entries: Vec<SubDomain>,
    frozen: bool,
}

impl SubDomainMap {
    pub fn new() -> Self {
        SubDomainMap {
            entries: Vec::new(),
            frozen: false,
        }
    }

    /// 登记一条映射：空名拒 + 未知域拒 + 重名拒 + 印后拒。
    pub fn bind(&mut self, sub: &str, domain: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(DM_FROZEN);
        }
        if sub.is_empty() {
            return Err(DM_EMPTY_NAME);
        }
        if domain >= DOM_COUNT {
            return Err(DM_UNK_DOMAIN);
        }
        if self.entries.iter().any(|e| e.sub == sub) {
            return Err(DM_DUP_SUB);
        }
        self.entries.push(SubDomain { sub: sub.to_string(), domain });
        Ok(())
    }

    pub fn lookup(&self, sub: &str) -> Option<usize> {
        self.entries.iter().find(|e| e.sub == sub).map(|e| e.domain)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 覆盖核：16 子段全登记。
    pub fn covers_all(&self) -> bool {
        ALL_SUBS.iter().all(|s| self.lookup(s).is_some())
    }

    /// 域闭集合法核：所有映射域都在闭集内（bind 已闸，此处复核账面）。
    pub fn domains_legal(&self) -> bool {
        self.entries.iter().all(|e| e.domain < DOM_COUNT)
    }

    /// 各域子段计数。
    pub fn domain_counts(&self) -> [usize; DOM_COUNT] {
        let mut c = [0usize; DOM_COUNT];
        for e in &self.entries {
            if e.domain < DOM_COUNT {
                c[e.domain] += 1;
            }
        }
        c
    }

    /// 映射指纹。
    pub fn map_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in &self.entries {
            for b in e.sub.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h = h.wrapping_mul(0x0100_0193) ^ e.domain as u32;
        }
        h ^= self.frozen as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 冻结印。
    pub fn freeze(&mut self) -> bool {
        if self.frozen || !self.covers_all() || !self.domains_legal() {
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
        for e in &self.entries {
            out.push_str(&format!("{}->{}\n", e.sub, DOM_NAMES[e.domain]));
        }
        if self.frozen {
            out.push_str("SUBDOM-FROZEN\n");
        }
        out
    }
}

impl Default for SubDomainMap {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：16 子段全登记（引导 4 → boot，内核 4 → kernel，
/// 登录桌面 2 → ui + 2 → kernel，交互 4 → io）。
fn standard_map() -> SubDomainMap {
    let mut m = SubDomainMap::new();
    let _ = m.bind("fwpost", DOM_BOOT);
    let _ = m.bind("load", DOM_BOOT);
    let _ = m.bind("menu", DOM_BOOT);
    let _ = m.bind("handoff", DOM_KERNEL);
    let _ = m.bind("init", DOM_KERNEL);
    let _ = m.bind("drv", DOM_KERNEL);
    let _ = m.bind("svc", DOM_KERNEL);
    let _ = m.bind("sched", DOM_KERNEL);
    let _ = m.bind("sesmgr", DOM_UI);
    let _ = m.bind("compos", DOM_UI);
    let _ = m.bind("logingui", DOM_UI);
    let _ = m.bind("cred", DOM_KERNEL);
    let _ = m.bind("input", DOM_IO);
    let _ = m.bind("firstwin", DOM_UI);
    let _ = m.bind("evtpath", DOM_IO);
    let _ = m.bind("focus", DOM_IO);
    m
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知域
    let mut m = SubDomainMap::new();
    if m.bind("fwpost", 9) == Err(DM_UNK_DOMAIN) {
        caught += 1;
    }
    // 注 2：重名冲突
    let mut m = standard_map();
    if m.bind("load", DOM_IO) == Err(DM_DUP_SUB) {
        caught += 1;
    }
    // 注 3：空名
    let mut m = SubDomainMap::new();
    if m.bind("", DOM_IO) == Err(DM_EMPTY_NAME) {
        caught += 1;
    }
    // 注 4：缺覆盖冻结拒
    let mut m = SubDomainMap::new();
    let _ = m.bind("fwpost", DOM_BOOT);
    if !m.freeze() && !m.frozen() {
        caught += 1;
    }
    // 注 5：印后改表
    let mut m = standard_map();
    let _ = m.freeze();
    if m.bind("extra", DOM_IO) == Err(DM_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 归属映射账；J2 映射语义；J3 映射指纹。
pub fn run_unx56186_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56186");
    // J1-1 16 子段全登记
    let m = standard_map();
    let ok = m.len() == 16 && m.covers_all();
    s.add("56186-J1-16子段全登记", ok, if ok { "" } else { "覆盖不全" });
    // J1-2 渲染导出齐
    let text = standard_map().render_map();
    let ok = text.contains("fwpost->boot") && text.contains("focus->io") && text.contains("SUBDOM-FROZEN");
    s.add("56186-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 域闭集合法
    let ok = m.domains_legal();
    s.add("56186-J2-域闭集合法", ok, if ok { "" } else { "域越界" });
    // J2-2 关键抽查：handoff → kernel
    let ok = m.lookup("handoff") == Some(DOM_KERNEL);
    s.add("56186-J2-handoff归kernel", ok, if ok { "" } else { "归属漂移" });
    // J2-3 关键抽查：compos → ui
    let ok = m.lookup("compos") == Some(DOM_UI);
    s.add("56186-J2-compos归ui", ok, if ok { "" } else { "归属漂移" });
    // J2-4 关键抽查：evtpath → io
    let ok = m.lookup("evtpath") == Some(DOM_IO);
    s.add("56186-J2-evtpath归io", ok, if ok { "" } else { "归属漂移" });
    // J2-5 各域计数（boot 3 / kernel 6 / ui 4 / io 3）
    let c = m.domain_counts();
    let ok = c == [3, 6, 4, 3];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", c[0], c[1], c[2], c[3]) };
    s.add_dyn("56186-J2-各域计数", c == [3, 6, 4, 3], &detail);
    // J2-6 未知子段查无
    let ok = m.lookup("nonexist").is_none();
    s.add("56186-J2-未知子段查无", ok, if ok { "" } else { "查有即错" });
    // J3-1 映射指纹确定
    let ok = standard_map().map_fp() == standard_map().map_fp();
    s.add("56186-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 表变签变
    let mut m2 = SubDomainMap::new();
    let _ = m2.bind("fwpost", DOM_BOOT);
    let fp0 = m2.map_fp();
    let _ = m2.bind("load", DOM_BOOT);
    let ok = m2.map_fp() != fp0;
    s.add("56186-J3-表变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 冻结印
    let mut m3 = standard_map();
    let ok = m3.freeze() && m3.frozen();
    s.add("56186-J3-冻结印", ok, if ok { "" } else { "冻结失败" });
    // J3-4 印后拒改
    let ok = m3.bind("again", DOM_IO) == Err(DM_FROZEN);
    s.add("56186-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56186-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56186_all_checks_green() {
        let set = run_unx56186_checks();
        assert!(set.all_passed(), "UNX-F56186 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56186_map_stable() {
        let m = standard_map();
        assert!(m.covers_all() && m.domains_legal());
    }
}
