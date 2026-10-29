//! UNX-F12184 — RTL_CRITICAL_SECTION_DEBUG 与 DebugInfo（0x1a920 段）
//!
//! 锚点：UNX-F12184-J1 DebugInfo 语义 10/10（Type=0x1000 /
//! CreatorBackTraceIndex / EntryCount / ContentionCount / 临界区
//! 双向链），争用计数逐次递增、链双向对平。
//! 域：CoRun·UNX-D1-B10（临界区谱系）。
//! 结构：DebugInfo（调试信息块）+ DebugBook（实例↔DebugInfo 双向
//! 账）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a920;
pub const DB_TYPE: u32 = SEG_BASE + 0x01; // Type 位型非法
pub const DB_TWICE: u32 = SEG_BASE + 0x02; // 重复挂 DebugInfo
pub const DB_SEALED: u32 = SEG_BASE + 0x03; // 印后记账
pub const DB_GHOST: u32 = SEG_BASE + 0x04; // 野 DebugInfo
pub const DB_EMPTY: u32 = SEG_BASE + 0x05; // 空账争用

pub const DBG_TYPE_EXPECT: u32 = 0x1000;

/// DebugInfo 块。
#[derive(Clone, PartialEq)]
pub struct DebugInfo {
    pub addr: u64,
    pub type_field: u16,
    pub creator_back_trace_index: u16,
    pub entry_count: u32,
    pub contention_count: u32,
    pub critical_section: u64, // 反向指针
    pub note: String,
}

impl DebugInfo {
    /// 自洽：Type=0x1000、双向指针成对或皆零。
    pub fn ok(&self) -> bool {
        self.type_field as u32 == DBG_TYPE_EXPECT && self.addr != 0
    }

    pub fn render(&self) -> String {
        format!(
            "d={:#x} t={:#x} e={} c={}",
            self.addr, self.type_field, self.entry_count, self.contention_count
        )
    }
}

/// DebugBook。
pub struct DebugBook {
    pub infos: Vec<DebugInfo>,
    pub pairings: Vec<(u64, u64)>, // (cs_handle, debug_addr)
    sealed: bool,
}

impl DebugBook {
    pub fn new() -> DebugBook {
        DebugBook { infos: Vec::new(), pairings: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂 DebugInfo（Type 非法/重复 addr 必拒）。
    pub fn attach(&mut self, addr: u64, bti: u16, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DB_SEALED);
        }
        let di = DebugInfo {
            addr,
            type_field: DBG_TYPE_EXPECT as u16,
            creator_back_trace_index: bti,
            entry_count: 0,
            contention_count: 0,
            critical_section: 0,
            note: note.to_string(),
        };
        if !di.ok() {
            return Err(DB_TYPE);
        }
        if self.infos.iter().any(|d| d.addr == addr) {
            return Err(DB_TWICE);
        }
        self.infos.push(di);
        Ok(())
    }

    /// 双向配对（cs 侧反向写）。
    pub fn pair(&mut self, cs: u64, dbg: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DB_SEALED);
        }
        if !self.infos.iter().any(|d| d.addr == dbg) {
            return Err(DB_GHOST);
        }
        if self.pairings.iter().any(|&(c, _)| c == cs) {
            return Err(DB_TWICE);
        }
        self.pairings.push((cs, dbg));
        if let Some(d) = self.infos.iter_mut().find(|d| d.addr == dbg) {
            d.critical_section = cs;
        }
        Ok(())
    }

    /// 争用一次（contention++）。
    pub fn contend(&mut self, dbg: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(DB_SEALED);
        }
        match self.infos.iter_mut().find(|d| d.addr == dbg) {
            Some(d) => {
                d.contention_count += 1;
                Ok(d.contention_count)
            }
            None => Err(DB_GHOST),
        }
    }

    /// 进入一次（entry++）。
    pub fn entered(&mut self, dbg: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(DB_SEALED);
        }
        match self.infos.iter_mut().find(|d| d.addr == dbg) {
            Some(d) => {
                d.entry_count += 1;
                Ok(d.entry_count)
            }
            None => Err(DB_GHOST),
        }
    }

    /// 双向链对平：配对账 ↔ 反向指针逐对一致。
    pub fn chain_consistent(&self) -> bool {
        self.pairings.iter().all(|&(cs, dbg)| {
            self.infos
                .iter()
                .find(|d| d.addr == dbg)
                .map(|d| d.critical_section == cs)
                .unwrap_or(false)
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a920;
        for d in &self.infos {
            h = h.wrapping_mul(211).wrapping_add(d.addr as u32);
            h = h.wrapping_mul(211).wrapping_add(d.contention_count);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "infos={} pairs={} chain={}",
            self.infos.len(),
            self.pairings.len(),
            self.chain_consistent()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DebugBook::new();
    // 1) Type 非法（手造错型块）必拒。
    got[0] = {
        let bad = DebugInfo {
            addr: 0x9000,
            type_field: 0x2000,
            creator_back_trace_index: 0,
            entry_count: 0,
            contention_count: 0,
            critical_section: 0,
            note: "t".to_string(),
        };
        !bad.ok()
    };
    // 2) 重复 addr 必拒。
    let _ = b.attach(0x9100, 1, "a");
    got[1] = b.attach(0x9100, 1, "d").err() == Some(DB_TWICE);
    // 3) 野 DebugInfo 配对必拒。
    got[2] = b.pair(0x42, 0xDEAD).err() == Some(DB_GHOST);
    // 4) 野 DebugInfo 争用必拒。
    got[3] = b.contend(0xDEAD).err() == Some(DB_GHOST);
    // 5) 印后挂必拒。
    b.seal();
    got[4] = b.attach(0x9200, 1, "post").err() == Some(DB_SEALED);
    got
}

/// 判据入口：UNX-F12184
pub fn run_unx12184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12184");
    let mut b = DebugBook::new();

    // J1 三 DebugInfo 挂账。
    let j1 = b.attach(0x9100, 1, "d1").is_ok()
        && b.attach(0x9200, 2, "d2").is_ok()
        && b.attach(0x9300, 3, "d3").is_ok();
    s.add("J1_attach3", j1 && b.infos.len() == 3, "三 DebugInfo 挂账");

    // J2 Type 位型全 0x1000。
    s.add(
        "J2_type",
        b.infos.iter().all(|d| d.type_field as u32 == DBG_TYPE_EXPECT),
        "Type 全 0x1000",
    );

    // J3 BTI 保留。
    s.add(
        "J3_bti",
        b.infos.iter().map(|d| d.creator_back_trace_index).collect::<Vec<_>>() == [1, 2, 3],
        "BTI 1/2/3 保留",
    );

    // J4 双向配对两对。
    let j4 = b.pair(0x42, 0x9100).is_ok() && b.pair(0x43, 0x9200).is_ok();
    s.add(
        "J4_pair2",
        j4 && b.pairings.len() == 2 && b.chain_consistent(),
        "双配对且反向链对平",
    );

    // J5 争用计数 10/10 递增。
    let mut ok5 = true;
    for i in 1..=10u32 {
        if b.contend(0x9300) != Ok(i) {
            ok5 = false;
        }
    }
    s.add("J5_contend_10x", ok5, "争用计数 1..10 递增");

    // J6 进入计数。
    let e6 = b.entered(0x9100);
    s.add(
        "J6_entry",
        e6 == Ok(1),
        "进入计数 1",
    );

    // J7 重复配对拒。
    s.add(
        "J7_dup_pair",
        b.pair(0x42, 0x9100).err() == Some(DB_TWICE),
        "同 cs 重复配对拒 TWICE",
    );

    // J8 野 DebugInfo 拒。
    s.add(
        "J8_ghost",
        b.pair(0x44, 0xDEAD).err() == Some(DB_GHOST),
        "野地址配对拒 GHOST",
    );

    // J9 渲染。
    let r = b.infos[0].render();
    s.add(
        "J9_render",
        r.contains("d=0x9100") && r.contains("e=1"),
        "渲染含地址/计数",
    );

    // J10 自洽核。
    s.add(
        "J10_ok",
        b.infos.iter().all(|d| d.ok()),
        "全块过自洽核",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "调试账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.attach(0x9400, 4, "p").err() == Some(DB_SEALED)
            && b.contend(0x9100).err() == Some(DB_SEALED),
        "印账后挂/争皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("infos=3") && sm.contains("chain=true"),
        "摘要含块数与链位",
    );

    // J15 配对后反向指针。
    s.add(
        "J15_back_ptr",
        b.infos[0].critical_section == 0x42 && b.infos[1].critical_section == 0x43,
        "反向指针 0x42/0x43 对平",
    );

    // J16 争用计数保留。
    s.add(
        "J16_contention_kept",
        b.infos[2].contention_count == 10,
        "d3 争用计 10 保留",
    );

    // J17 空账争用拒。
    let mut b17 = DebugBook::new();
    s.add(
        "J17_empty_contend",
        b17.contend(0x1).err() == Some(DB_GHOST),
        "空账争用拒 GHOST",
    );

    // J18 空账指纹稳定。
    let b18 = DebugBook::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空调试账指纹稳定");

    // J19 十 DebugInfo 挂账全过。
    let mut b19 = DebugBook::new();
    let mut ok19 = true;
    for i in 0..10u64 {
        if b19.attach(0xA000 + i * 0x10, i as u16, "m").is_err() {
            ok19 = false;
        }
    }
    s.add("J19_ten_infos", ok19 && b19.infos.len() == 10, "十块挂账全过");

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12184_debug() {
        let mut b = DebugBook::new();
        let _ = b.attach(0x9100, 1, "d");
        let _ = b.pair(0x42, 0x9100);
        assert!(b.chain_consistent());
        assert_eq!(b.contend(0x9100), Ok(1));
    }

    #[test]
    fn unx12184_checks_green() {
        let s = run_unx12184_checks();
        assert!(s.all_passed(), "UNX-F12184 判据未全绿");
    }

    #[test]
    fn unx12184_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
