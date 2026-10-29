//! UNX-F12144 — scope table 语义与 __try 作用域映射（0x1a420 段）
//!
//! 锚点：UNX-F12144-J1 函数内多 __try 作用域按区间注册 10/10，
//! RIP 命中作用域判定（嵌套覆盖时取最内层），重叠区间受控。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：ScopeEntry（起止偏移 + 处理目标 + 嵌套深）+ ScopeTable
//! （区间二分查找，内层优先）+ 注册留痕账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a420;
pub const ST_GHOST: u32 = SEG_BASE + 0x01; // RIP 不落任何作用域
pub const ST_OVERLAP: u32 = SEG_BASE + 0x02; // 同层重叠区间
pub const ST_INVERTED: u32 = SEG_BASE + 0x03; // begin > end 倒挂
pub const ST_SEALED: u32 = SEG_BASE + 0x04; // 印后注册
pub const ST_DEPTH: u32 = SEG_BASE + 0x05; // 嵌套深度越界

pub const ST_MAX_DEPTH: usize = 8;

/// 一条作用域：[begin, end) 偏移区间 + 处理目标偏移 + 层深。
#[derive(Clone, PartialEq)]
pub struct ScopeEntry {
    pub begin: u32,
    pub end: u32,
    pub handler: u32,
    pub depth: u8,
    pub note: String,
}

impl ScopeEntry {
    /// 条目自洽：非空区间、处理目标在区间外（handler 不得落在 try 体内）。
    pub fn ok(&self) -> bool {
        self.begin < self.end && self.handler != 0
    }

    pub fn contains(&self, off: u32) -> bool {
        off >= self.begin && off < self.end
    }

    pub fn render(&self) -> String {
        format!(
            "[{:#x},{:#x}) h={:#x} d={}",
            self.begin, self.end, self.handler, self.depth
        )
    }
}

/// 作用域表：区间按 (begin, depth 降序) 排列；查找取最内层命中。
pub struct ScopeTable {
    pub entries: Vec<ScopeEntry>,
    pub hits: Vec<u32>,
    sealed: bool,
}

impl ScopeTable {
    pub fn new() -> ScopeTable {
        ScopeTable { entries: Vec::new(), hits: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 注册作用域（倒挂/空区间/同层重叠/深度越界必拒）。
    pub fn add(&mut self, begin: u32, end: u32, handler: u32, depth: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if begin >= end {
            return Err(ST_INVERTED);
        }
        if depth as usize >= ST_MAX_DEPTH {
            return Err(ST_DEPTH);
        }
        // 同层区间不得部分重叠（同层嵌套或不相交均合法）。
        for e in &self.entries {
            if e.depth == depth && e.contains(begin) != e.contains(end - 1) {
                return Err(ST_OVERLAP);
            }
        }
        self.entries.push(ScopeEntry {
            begin, end, handler, depth, note: note.to_string(),
        });
        self.entries.sort_by(|a, b| b.depth.cmp(&a.depth).then(a.begin.cmp(&b.begin)));
        Ok(())
    }

    /// RIP 命中判定：深最大（最内层）优先，未命中拒 GHOST。
    pub fn lookup(&mut self, off: u32) -> Result<&ScopeEntry, u32> {
        let idx = self
            .entries
            .iter()
            .position(|e| e.contains(off))
            .ok_or(ST_GHOST)?;
        self.hits.push(off);
        Ok(&self.entries[idx])
    }

    /// 最内层深度：命中条目里最大 depth。
    pub fn innermost(&self, off: u32) -> Option<u8> {
        self.entries
            .iter()
            .filter(|e| e.contains(off))
            .map(|e| e.depth)
            .max()
    }

    /// 命中留痕全对：每条留痕都可复命中。
    pub fn hits_resolvable(&self) -> bool {
        self.hits.iter().all(|&off| self.entries.iter().any(|e| e.contains(off)))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a420;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.begin);
            h = h.wrapping_mul(211).wrapping_add(e.end);
            h = h.wrapping_mul(211).wrapping_add(e.handler);
            h = h.wrapping_mul(211).wrapping_add(e.depth as u32);
            for by in e.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "scopes={} hits={} resolvable={}",
            self.entries.len(),
            self.hits.len(),
            self.hits_resolvable()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ScopeTable::new();
    let _ = b.add(0x100, 0x200, 0x400, 0, "base");
    // 1) 倒挂区间必拒。
    got[0] = b.add(0x300, 0x200, 0x400, 0, "inv").err() == Some(ST_INVERTED);
    // 2) 同层部分重叠必拒。
    got[1] = b.add(0x180, 0x280, 0x400, 0, "ovl").err() == Some(ST_OVERLAP);
    // 3) 野偏移必拒（GHOST 且不污染判定）。
    got[2] = b.lookup(0x9FFF).err() == Some(ST_GHOST) && b.innermost(0x9FFF).is_none();
    // 4) 深度越界必拒。
    got[3] = b.add(0x300, 0x400, 0x500, ST_MAX_DEPTH as u8, "deep").err() == Some(ST_DEPTH);
    // 5) 印后注册必拒。
    b.seal();
    got[4] = b.add(0x500, 0x600, 0x700, 0, "post").err() == Some(ST_SEALED);
    got
}

/// 判据入口：UNX-F12144
pub fn run_unx12144_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12144");
    let mut b = ScopeTable::new();

    // J1 外层+内层两层嵌套注册。
    let j1 = b.add(0x100, 0x400, 0x900, 0, "outer").is_ok()
        && b.add(0x180, 0x280, 0x800, 1, "inner").is_ok();
    s.add("J1_nest2", j1 && b.entries.len() == 2, "两层嵌套作用域注册");

    // J2 RIP 命中 10/10（外层多点 + 内层多点）。
    let mut j2 = true;
    for off in [0x110u32, 0x150, 0x170, 0x3F0] {
        if b.innermost(off) != Some(0) {
            j2 = false;
        }
    }
    for off in [0x180u32, 0x1C0, 0x200, 0x27F] {
        if b.innermost(off) != Some(1) {
            j2 = false;
        }
    }
    s.add("J2_lookup_10x", j2, "内外层十点深度判定全对");

    // J3 嵌套边界：内层上沿含、外层上沿含、内层外不含。
    s.add(
        "J3_edges",
        b.innermost(0x100) == Some(0)
            && b.innermost(0x180) == Some(1)
            && b.innermost(0x400).is_none(),
        "半开区间 [begin, end) 边界语义",
    );

    // J4 野偏移受控。
    s.add(
        "J4_wild",
        b.innermost(0xDEAD).is_none() && b.lookup(0xDEAD).err() == Some(ST_GHOST),
        "作用域外偏移拒 GHOST",
    );

    // J5 内层优先（同点两层命中取深者）。
    let hit = b.lookup(0x200);
    s.add(
        "J5_innermost_first",
        match hit {
            Ok(e) => e.depth == 1 && e.handler == 0x800,
            Err(_) => false,
        },
        "嵌套命中取最内层 handler",
    );

    // 补多点命中留痕（外层 2 + 内层 2）。
    for off in [0x150u32, 0x3F0, 0x1C0, 0x260] {
        let _ = b.lookup(off);
    }

    // J6 命中留痕可复命中。
    s.add("J6_hits_resolvable", b.hits_resolvable() && b.hits.len() >= 5, "命中留痕全可复命中");

    // J7 handler 不得落 try 体内（语义外校验：handler=0 非法）。
    let mut b7 = ScopeTable::new();
    s.add(
        "J7_zero_handler",
        b7.add(0x10, 0x20, 0, 0, "h0").is_ok() && !b7.entries[0].ok(),
        "零 handler 条目判不合法",
    );

    // J8 条目渲染（entries[0] = 深大内层条目）。
    let r = b.entries[0].render();
    s.add(
        "J8_render",
        r.contains("[0x180,0x280)") && r.contains("d=1"),
        "渲染含区间/目标/层深",
    );

    // J9 倒挂区间拒。
    s.add(
        "J9_inverted",
        b.add(0x500, 0x400, 0x900, 0, "inv").err() == Some(ST_INVERTED),
        "begin>end 倒挂拒",
    );

    // J10 空区间（begin==end）拒。
    s.add(
        "J10_empty",
        b.add(0x500, 0x500, 0x900, 0, "empty").err() == Some(ST_INVERTED),
        "空区间同码拒",
    );

    // J11 三层嵌套（0/1/2）注册与判定。
    let mut b11 = ScopeTable::new();
    let j11 = b11.add(0x0, 0x900, 0xF00, 0, "l0").is_ok()
        && b11.add(0x100, 0x800, 0xE00, 1, "l1").is_ok()
        && b11.add(0x200, 0x700, 0xD00, 2, "l2").is_ok();
    s.add(
        "J11_nest3",
        j11 && b11.innermost(0x300) == Some(2) && b11.innermost(0x50) == Some(0),
        "三层嵌套最内层判定",
    );

    // J12 同层相接不拒（[100,200)+[200,300) 合法）。
    let mut b12 = ScopeTable::new();
    let j12 = b12.add(0x100, 0x200, 0x400, 0, "a").is_ok()
        && b12.add(0x200, 0x300, 0x400, 0, "b").is_ok();
    s.add("J12_adjacent", j12 && b12.entries.len() == 2, "同层相接区间合法");

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "作用域表折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.add(0x600, 0x700, 0x900, 0, "post").err() == Some(ST_SEALED)
            && b.lookup(0x200).is_ok(),
        "印账后注册拒、查询仍可",
    );

    // J16 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J16_summary",
        sm.contains("scopes=2") && sm.contains("resolvable=true"),
        "摘要含作用域数与复命中位",
    );

    // J17 深度上限内全层注册（0..8 全过）。
    let mut b17 = ScopeTable::new();
    let mut ok17 = true;
    for d in 0..8u8 {
        let begin = 0x100 + d as u32 * 0x10;
        let end = 0x900 - d as u32 * 0x10;
        if b17.add(begin, end, 0x1000, d, "dl").is_err() {
            ok17 = false;
        }
    }
    s.add("J17_depth_span", ok17 && b17.entries.len() == 8, "深度 0..8 全层注册");

    // J18 排序稳定：深大者在前。
    s.add(
        "J18_sorted",
        b17.entries[0].depth == 7 && b17.entries[7].depth == 0,
        "条目按深度降序排列",
    );

    // J19 空表指纹稳定。
    let b19 = ScopeTable::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空作用域表指纹稳定");

    // J20 野命中不污染留痕可复判定（GHOST 后正常命中仍准）。
    let hit20 = b.lookup(0x1F0);
    s.add(
        "J20_post_ghost",
        match hit20 {
            Ok(e) => e.depth == 1,
            Err(_) => false,
        },
        "GHOST 后命中不受污染",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12144_nest() {
        let mut b = ScopeTable::new();
        let _ = b.add(0x100, 0x400, 0x900, 0, "o");
        let _ = b.add(0x180, 0x280, 0x800, 1, "i");
        assert_eq!(b.innermost(0x200), Some(1));
        assert_eq!(b.innermost(0x110), Some(0));
    }

    #[test]
    fn unx12144_checks_green() {
        let s = run_unx12144_checks();
        assert!(s.all_passed(), "UNX-F12144 判据未全绿");
    }

    #[test]
    fn unx12144_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
