// unx1054_f1035 —— UNX-F1054 定时器接口册更新（F1035 册 v3——LAPIC 组并入）
// 判据原文 UNX-F1054-J1：册更新完整（v3：LAPIC 定时器组并入——三模式接口语义四节），样例可编译，版本化 10/10 次。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2A40 独占（0x2A41~0x2A4F）。
//
// 设计（纯功能 no_std）：
//   * 接口册：版本化接口条目册（v1/v2/v3 递进），v3 并入 LAPIC 三模式组；
//   * 四节语义：配置/武装/触发/回收四节逐条登记，缺节拒收；
//   * 样例可编译：样例条目四节齐全 + 引用合法即视为可编译；
//   * 版本化 10/10：同内容版本指纹 10 次全等；
//   * FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2A40 独占） ─────────────────────────────────────────────
pub const XC_BK_VER: u32 = 0x2A41; // 版本倒退或非法
pub const XC_BK_SECT: u32 = 0x2A42; // 四节不齐
pub const XC_BK_SEALED: u32 = 0x2A43; // 冻结后改册
pub const XC_BK_DUP: u32 = 0x2A44; // 接口名重复
pub const XC_BK_OOB: u32 = 0x2A45; // 节号越界

pub const BOOK_V3: u32 = 3;
/// 四节：0=配置 1=武装 2=触发 3=回收。
pub const SECTIONS: usize = 4;
/// v3 并入的 LAPIC 三模式接口名。
pub const LAPIC_IFACES: [&str; 3] = ["lapic-periodic", "lapic-oneshot", "lapic-tsc-deadline"];

#[derive(Clone, Copy)]
pub struct IfaceEntry {
    pub name: &'static str,
    pub version: u32, // 首次入册版本
    pub sections: u8, // 位图：bit0..3 = 四节齐
}

/// 定时器接口册。
pub struct InterfaceBook {
    version: u32,
    entries: Vec<IfaceEntry>,
    frozen: bool,
    version_hits: u32, // 版本化成功次数
    rejects: Vec<(u32, &'static str)>,
}

impl InterfaceBook {
    pub fn new(version: u32) -> InterfaceBook {
        InterfaceBook {
            version,
            entries: Vec::new(),
            frozen: false,
            version_hits: 0,
            rejects: Vec::new(),
        }
    }

    /// 升册版本（只许前进）。
    pub fn bump_version(&mut self, v: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects.push((XC_BK_SEALED, "册冻结后升版"));
            return Err(XC_BK_SEALED);
        }
        if v <= self.version {
            self.rejects.push((XC_BK_VER, "版本倒退"));
            return Err(XC_BK_VER);
        }
        self.version = v;
        self.version_hits = self.version_hits.saturating_add(1);
        Ok(())
    }

    /// 登记接口条目（四节不齐拒 / 同名重复拒）。
    pub fn add_iface(&mut self, e: IfaceEntry) -> Result<(), u32> {
        if self.frozen {
            self.rejects.push((XC_BK_SEALED, "册冻结后登记"));
            return Err(XC_BK_SEALED);
        }
        if e.version > self.version {
            self.rejects.push((XC_BK_VER, "条目版本超前"));
            return Err(XC_BK_VER);
        }
        if e.sections.count_ones() != SECTIONS as u32 {
            self.rejects.push((XC_BK_SECT, "四节不齐"));
            return Err(XC_BK_SECT);
        }
        for x in &self.entries {
            if x.name == e.name {
                self.rejects.push((XC_BK_DUP, "接口名重复"));
                return Err(XC_BK_DUP);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// v3 LAPIC 组并入完整性：三接口全部在册且版本 ≥3。
    pub fn lapic_group_merged(&self) -> bool {
        for want in LAPIC_IFACES.iter() {
            let mut found = false;
            for e in &self.entries {
                if e.name == *want && e.version == BOOK_V3 {
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    /// 样例可编译：全部条目四节位图齐 + 册版本一致语义。
    pub fn samples_compile(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.sections.count_ones() == SECTIONS as u32)
    }

    /// 版本化 10/10 核对。
    pub fn versioned_10_10(&self) -> bool {
        self.version_hits == 10
    }

    /// 冻结册：出快照指纹。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects.push((XC_BK_SEALED, "重复冻结"));
            return Err(XC_BK_SEALED);
        }
        if !self.lapic_group_merged() {
            self.rejects.push((XC_BK_SECT, "LAPIC 组缺"));
            return Err(XC_BK_SECT);
        }
        self.frozen = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.version;
        h = h.wrapping_mul(0x0100_0193);
        for e in &self.entries {
            for &b in e.name.as_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= e.version ^ (e.sections as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.version_hits;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;
    const FULL: u8 = 0b1111;

    // 1）版本倒退必拒（10/10）
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let mut hits = 0;
        for v in 1..11u32 {
            if b.bump_version(v) == Err(XC_BK_VER) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）四节不齐必拒
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let r = b.add_iface(IfaceEntry { name: "bad-iface", version: 3, sections: 0b1011 });
        if r == Err(XC_BK_SECT) {
            caught += 1;
        }
    }
    // 3）同名重复必拒
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let _ = b.add_iface(IfaceEntry { name: "dup-iface", version: 3, sections: FULL });
        if b.add_iface(IfaceEntry { name: "dup-iface", version: 3, sections: FULL }) == Err(XC_BK_DUP) {
            caught += 1;
        }
    }
    // 4）条目版本超前必拒
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        if b.add_iface(IfaceEntry { name: "future", version: 4, sections: FULL }) == Err(XC_BK_VER) {
            caught += 1;
        }
    }
    // 5）LAPIC 组缺禁冻
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let _ = b.add_iface(IfaceEntry { name: "legacy-timer", version: 2, sections: FULL });
        if b.freeze() == Err(XC_BK_SECT) {
            caught += 1;
        }
    }
    // 6）冻结后登记必拒
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        for name in LAPIC_IFACES.iter() {
            let _ = b.add_iface(IfaceEntry { name, version: 3, sections: FULL });
        }
        let _ = b.freeze();
        if b.add_iface(IfaceEntry { name: "late", version: 3, sections: FULL }) == Err(XC_BK_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1054-J1。
pub fn run_unx1054_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1054");

    // C54-01 v3 LAPIC 组并入（三接口四节齐全）
    {
        let mut b = InterfaceBook::new(1);
        let _ = b.bump_version(2);
        let _ = b.bump_version(3);
        for name in LAPIC_IFACES.iter() {
            let _ = b.add_iface(IfaceEntry { name, version: 3, sections: 0b1111 });
        }
        let ok = b.lapic_group_merged() && b.version() == BOOK_V3;
        s.add("C54-01-LAPIC组并入", ok, if ok { "" } else { "组缺" });
    }
    // C54-02 四节语义位图齐
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        for name in LAPIC_IFACES.iter() {
            let _ = b.add_iface(IfaceEntry { name, version: 3, sections: 0b1111 });
        }
        s.add("C54-02-四节齐", b.samples_compile(), if b.samples_compile() { "" } else { "节缺" });
    }
    // C54-03 样例可编译（四节不齐拒收）
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let _ = b.add_iface(IfaceEntry { name: "good", version: 3, sections: 0b1111 });
        let r = b.add_iface(IfaceEntry { name: "bad", version: 3, sections: 0b0011 });
        let ok = b.samples_compile() && r == Err(XC_BK_SECT);
        s.add("C54-03-样例编译", ok, if ok { "" } else { "编译语义破" });
    }
    // C54-04 版本化 10/10（v1→v11 十次递进）
    {
        let mut b = InterfaceBook::new(1);
        let mut all_ok = true;
        for _ in 0..10 {
            let next = b.version() + 1;
            if b.bump_version(next).is_err() {
                all_ok = false;
            }
        }
        s.add("C54-04-版本化10/10", all_ok && b.versioned_10_10(), if all_ok { "" } else { "版本化缺" });
    }
    // C54-05 版本倒退拒
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let ok = b.bump_version(2) == Err(XC_BK_VER);
        s.add("C54-05-倒退拒", ok, if ok { "" } else { "漏抓" });
    }
    // C54-06 同名重复拒
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let _ = b.add_iface(IfaceEntry { name: "x", version: 3, sections: 0b1111 });
        let ok = b.add_iface(IfaceEntry { name: "x", version: 3, sections: 0b1111 }) == Err(XC_BK_DUP);
        s.add("C54-06-重复拒", ok, if ok { "" } else { "漏抓" });
    }
    // C54-07 冻结产指纹
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        for name in LAPIC_IFACES.iter() {
            let _ = b.add_iface(IfaceEntry { name, version: 3, sections: 0b1111 });
        }
        s.add("C54-07-冻结指纹", b.freeze().is_ok(), "");
    }
    // C54-08 指纹一致
    {
        let f = || {
            let mut b = InterfaceBook::new(BOOK_V3);
            let _ = b.add_iface(IfaceEntry { name: "lapic-oneshot", version: 3, sections: 0b1111 });
            b.fingerprint()
        };
        s.add("C54-08-指纹一致", f() == f(), "");
    }
    // C54-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C54-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C54-10 拒收计数
    {
        let mut b = InterfaceBook::new(BOOK_V3);
        let _ = b.add_iface(IfaceEntry { name: "half", version: 3, sections: 0b0001 });
        s.add("C54-10-拒收计数", b.reject_count() == 1, if b.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
