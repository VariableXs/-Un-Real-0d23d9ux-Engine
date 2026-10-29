//! UNX-F56053 启动噪声源登记件（0x60680 段）
//!
//! 锚点：UNX-F56053-J1 影响启动时长的噪声源清单+静默规则（网络唤醒/
//! 盘唤醒/服务自启三类闭集逐源落账）；J2 静默态断言（网络断开 + 自启
//! 服务冻结清单逐源核）×3 轮全过；J3 违例拒排轮（静默破的源不落排程
//! 留痕）三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（启动分段域·噪声源面）。
//!
//! 结构：噪声源闭集登记册（三类 × 静默规则逐源落账）+ 静默态断言核
//! （断言集逐源核，全真才放行）+ 违例留痕账 + 排轮闸（静默全过才可排
//! 轮）+ 注入核（越闭集源/空规则/违例放行/静默未断言/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60680 独占 0x20）。
pub const SEG_BASE: u32 = 0x60680;
pub const NZ_SRC_UNK: u32 = SEG_BASE + 0x01; // 越闭集源
pub const NZ_RULE_EMPTY: u32 = SEG_BASE + 0x02; // 静默规则空
pub const NZ_VIOL_PASS: u32 = SEG_BASE + 0x03; // 违例放行
pub const NZ_ASSERT_LOST: u32 = SEG_BASE + 0x04; // 静默未断言
pub const NZ_SEALED: u32 = SEG_BASE + 0x05; // 登记册已封账拒写
pub const NZ_DUP_SRC: u32 = SEG_BASE + 0x06; // 同源重复登记
pub const NZ_SCHED_BLOCK: u32 = SEG_BASE + 0x07; // 静默未全过禁排轮

/// 噪声源闭集。
pub const S_NET: usize = 0;
pub const S_DISK: usize = 1;
pub const S_SERVICE: usize = 2;
pub const S_COUNT: usize = 3;
pub const S_NAMES: [&str; S_COUNT] = ["net_wake", "disk_wake", "service_autostart"];

/// 静默规则断言闭集（逐源一条断言名）。
pub const ASSERT_NAMES: [&str; S_COUNT] =
    ["net_link_down", "disk_spin_frozen", "service_list_frozen"];

pub fn assert_of(src: usize) -> Option<&'static str> {
    if src < S_COUNT {
        Some(ASSERT_NAMES[src])
    } else {
        None
    }
}

/// 噪声源账：源 × 静默规则。
#[derive(Clone)]
pub struct NoiseEntry {
    pub src: usize,
    pub rule: String,
}

impl NoiseEntry {
    pub fn new(src: usize, rule: &str) -> Result<Self, u32> {
        if src >= S_COUNT {
            return Err(NZ_SRC_UNK);
        }
        if rule.is_empty() {
            return Err(NZ_RULE_EMPTY);
        }
        Ok(NoiseEntry { src, rule: rule.to_string() })
    }
}

/// 噪声源登记册 + 静默断言账 + 排轮闸。
pub struct NoiseBook {
    pub entries: [Option<NoiseEntry>; S_COUNT],
    /// 逐源静默断言态（true = 断言过）。
    pub asserted: [bool; S_COUNT],
    pub violations: Vec<String>,
    pub sealed: bool,
}

impl NoiseBook {
    pub fn new() -> Self {
        NoiseBook {
            entries: [None, None, None],
            asserted: [false; S_COUNT],
            violations: Vec::new(),
            sealed: false,
        }
    }

    /// 逐源登记（同源重复拒 + 封账拒）。
    pub fn register(&mut self, e: NoiseEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(NZ_SEALED);
        }
        if e.src >= S_COUNT {
            return Err(NZ_SRC_UNK);
        }
        if self.entries[e.src].is_some() {
            return Err(NZ_DUP_SRC);
        }
        let src_i = e.src;
        self.entries[src_i] = Some(e);
        Ok(())
    }

    /// 静默断言（断言名与源对应才生效——防串源断言）。
    pub fn assert_silent(&mut self, src: usize, assert_name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(NZ_SEALED);
        }
        if src >= S_COUNT || self.entries[src].is_none() {
            return Err(NZ_SRC_UNK);
        }
        match assert_of(src) {
            None => return Err(NZ_SRC_UNK),
            Some(want) => {
                if want != assert_name {
                    self.violations.push(format!("VIOL|{}|wrong_assert", S_NAMES[src]));
                    return Err(NZ_VIOL_PASS);
                }
            }
        }
        self.asserted[src] = true;
        Ok(())
    }

    /// 静默全过核（三源断言全真）。
    pub fn all_silent(&self) -> bool {
        for a in self.asserted.iter() {
            if !a {
                return false;
            }
        }
        true
    }

    /// J3：排轮闸（静默全过才可排轮）。
    pub fn schedule_ready(&self) -> bool {
        self.all_silent()
    }

    /// 违例留痕齐。
    pub fn trail_ok(&self) -> bool {
        !self.violations.is_empty()
            && self.violations.iter().all(|v| v.starts_with("VIOL|"))
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：三源登记 + 三源断言全过。
pub fn standard_book() -> NoiseBook {
    let mut b = NoiseBook::new();
    let rules = ["net-iface-down", "disk-standby-freeze", "autostart-list-frozen"];
    for s in 0..S_COUNT {
        b.register(NoiseEntry::new(s, rules[s]).expect("fixture")).expect("reg");
    }
    for s in 0..S_COUNT {
        b.assert_silent(s, ASSERT_NAMES[s]).expect("assert");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集源
    if NoiseEntry::new(9, "r").err() == Some(NZ_SRC_UNK) {
        caught += 1;
    }
    // 2) 空规则
    if NoiseEntry::new(S_NET, "").err() == Some(NZ_RULE_EMPTY) {
        caught += 1;
    }
    // 3) 串源断言拒（盘断言用在网上）
    let mut b = standard_book();
    if b.assert_silent(S_NET, ASSERT_NAMES[S_DISK]) == Err(NZ_VIOL_PASS) {
        caught += 1;
    }
    // 4) 未断言排轮拒
    let mut b2 = NoiseBook::new();
    for s in 0..S_COUNT {
        b2.register(NoiseEntry::new(s, "r").expect("e")).expect("reg");
    }
    // 只断言两源
    b2.assert_silent(S_NET, ASSERT_NAMES[S_NET]).expect("a1");
    b2.assert_silent(S_DISK, ASSERT_NAMES[S_DISK]).expect("a2");
    if !b2.schedule_ready() {
        caught += 1;
    }
    // 5) 封账改写
    let mut b3 = standard_book();
    b3.seal();
    if b3.register(NoiseEntry::new(S_NET, "x").expect("s")) == Err(NZ_SEALED)
        && b3.assert_silent(S_NET, ASSERT_NAMES[S_NET]) == Err(NZ_SEALED)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56053_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56053");
    // J1-1 三源清单落账
    let b = standard_book();
    let mut reg = 0usize;
    for e in b.entries.iter() {
        if e.is_some() {
            reg += 1;
        }
    }
    let ok = reg == S_COUNT;
    s.add("56053-J1-三源清单落账", ok, if ok { "" } else { "清单缺源" });
    // J1-2 静默规则语义（三规则各就位）
    let ok = b.entries[S_NET].as_ref().expect("n").rule == "net-iface-down"
        && b.entries[S_SERVICE].as_ref().expect("sv").rule == "autostart-list-frozen";
    s.add("56053-J1-静默规则语义", ok, if ok { "" } else { "规则漂移" });
    // J1-3 源名/断言名闭集同源
    let ok = S_NAMES[S_NET] == "net_wake" && ASSERT_NAMES[S_DISK] == "disk_spin_frozen";
    s.add("56053-J1-命名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 静默态断言全过（×1 标准夹具 + ×2 复跑）
    let mut all_green = true;
    for _ in 0..3 {
        let b2 = standard_book();
        if !b2.all_silent() {
            all_green = false;
        }
    }
    let ok = all_green;
    s.add("56053-J2-三断言全过", ok, if ok { "" } else { "静默破" });
    // J2-2 断言名对应核（逐源断言名与闭集一致）
    let mut names_ok = true;
    for s in 0..S_COUNT {
        if assert_of(s) != Some(ASSERT_NAMES[s]) {
            names_ok = false;
        }
    }
    let ok = names_ok && assert_of(9).is_none();
    s.add("56053-J2-断言名对应", ok, if ok { "" } else { "断言串源" });
    // J2-3 断言后排轮就绪
    let ok = standard_book().schedule_ready();
    s.add("56053-J2-排轮就绪", ok, if ok { "" } else { "排轮闸误闭" });
    // J3-1 串源断言留痕（违例行落账）
    let mut b3 = standard_book();
    let _ = b3.assert_silent(S_NET, ASSERT_NAMES[S_SERVICE]);
    let ok = b3.violations.len() == 1 && b3.trail_ok();
    s.add("56053-J3-串源留痕", ok, if ok { "" } else { "违例被吞" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56053-J3-注入核必抓", ok, &detail);
    // J1-4 同源重复登记拒
    let mut b4 = NoiseBook::new();
    b4.register(NoiseEntry::new(S_DISK, "r1").expect("d1")).expect("reg1");
    let ok = b4.register(NoiseEntry::new(S_DISK, "r2").expect("d2")) == Err(NZ_DUP_SRC);
    s.add("56053-J1-重复登记拒", ok, if ok { "" } else { "重复穿透" });
    // J3-3 封账冻结
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.register(NoiseEntry::new(S_NET, "x").expect("s")).is_err()
        && b5.assert_silent(S_NET, ASSERT_NAMES[S_NET]).is_err();
    s.add("56053-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J3-4 未登记源断言拒
    let mut b6 = NoiseBook::new();
    let ok = b6.assert_silent(S_NET, ASSERT_NAMES[S_NET]) == Err(NZ_SRC_UNK);
    s.add("56053-J3-未登记拒断言", ok, if ok { "" } else { "未登记穿透" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56053_all_checks_green() {
        let set = run_unx56053_checks();
        assert!(set.all_passed(), "UNX-F56053 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56053_all_silent() {
        assert!(standard_book().all_silent());
    }
}
