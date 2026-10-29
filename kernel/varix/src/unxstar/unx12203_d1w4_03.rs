//! UNX-F12203 scope table 处理器主体（SEG 0x64060..0x64080 段窗）
//!
//! 卡面：编译器生成的 scope table（try 域表）主体——域条目注册、
//! ip 归属查找、filter 值裁决、handler/target 双通道分发；越界与
//! 重叠域拒止、印账冻结全链过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 域条目注册：begin<end 且条目间不重叠，违者必拒。
//! - J2 域表容量：条目数超 16 必拒 SC_DEPTH。
//! - J3 ip 归属：命中包含 ip 的首个域条目，域外必拒 SC_NO_SCOPE。
//! - J4 filter 三值：0=继续搜索、1=执行 handler、0xFFFFFFFF=继续执行。
//! - J5 坏 filter 拒止：filter 不在 {0,1,0xFFFFFFFF} 必拒 SC_BAD_FILTER。
//! - J6 handler 通道：target==0 的 finally 条目按收口通道留痕。
//! - J7 target 通道：target!=0 的 except 条目按跳转通道留痕。
//! - J8 分发次序：自内向外（后注册先裁决）逐条留痕。
//! - J9 空表放行：空域表裁决返回 SC_NO_SCOPE 不炸。
//! - J10 印账冻结：seal 后注册/分发必拒 SC_FROZEN。
//! - J11 指纹幂等：同态同指纹。
//! - J12 注入五况全抓。
//!
//! 结构：ScopeEntry 域条目（begin/end/handler/target）+ ScopeTable
//! 域表（条目/裁决账/操作账）+ 归属核（lookup）+ 裁决核（dispatch）
//! + 折叠指纹 + 注入核（坏区间/重叠/域外/坏filter/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64060 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64060;
pub const SC_BAD_RANGE: u32 = SEG_BASE + 0x01; // 域区间非法
pub const SC_OVERLAP: u32 = SEG_BASE + 0x02; // 域区间重叠
pub const SC_NO_SCOPE: u32 = SEG_BASE + 0x03; // ip 无域归属
pub const SC_BAD_FILTER: u32 = SEG_BASE + 0x04; // filter 值非法
pub const SC_DEPTH: u32 = SEG_BASE + 0x05; // 条目超容
pub const SC_FROZEN: u32 = SEG_BASE + 0x06; // 印账后操作

/// filter 裁决值（EXCEPTION_DISPOSITION 语义）。
pub const FILTER_CONTINUE_SEARCH: u32 = 0;
pub const FILTER_EXECUTE_HANDLER: u32 = 1;
pub const FILTER_CONTINUE_EXEC: u32 = 0xFFFF_FFFF;

/// 域表容量上限。
pub const MAX_ENTRIES: usize = 16;

/// 域条目。
#[derive(Clone, Copy)]
pub struct ScopeEntry {
    pub begin: u32,
    pub end: u32,
    pub handler: u32,
    pub target: u32,
}

impl ScopeEntry {
    pub fn covers(&self, ip: u32) -> bool {
        ip >= self.begin && ip < self.end
    }

    /// 通道类别：target==0 为 finally 收口通道，否则为 except 跳转通道。
    pub fn is_finally(&self) -> bool {
        self.target == 0
    }

    pub fn render(&self) -> String {
        format!("S{:x}-{:x}h{:x}t{:x}", self.begin, self.end, self.handler, self.target)
    }
}

/// 裁决记录：域下标 + filter 值 + 通道标签。
#[derive(Clone, Copy)]
pub struct Adjudication {
    pub entry: usize,
    pub filter: u32,
    pub channel: u8, // 1=finally 收口 2=except 跳转 3=继续搜索 4=继续执行
}

/// scope 表处理器。
pub struct ScopeTable {
    pub entries: Vec<ScopeEntry>,
    pub verdicts: Vec<Adjudication>,
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl ScopeTable {
    pub fn new() -> ScopeTable {
        ScopeTable { entries: Vec::new(), verdicts: Vec::new(), ops: Vec::new(), sealed: false }
    }

    /// 域条目注册。
    pub fn add_entry(&mut self, e: ScopeEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_FROZEN);
        }
        if e.begin >= e.end {
            return Err(SC_BAD_RANGE);
        }
        for g in &self.entries {
            if e.begin < g.end && g.begin < e.end {
                return Err(SC_OVERLAP);
            }
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(SC_DEPTH);
        }
        self.entries.push(e);
        self.ops.push(format!("ent{:x}", e.begin));
        Ok(())
    }

    /// ip 归属：自内向外（后注册先裁决）首个覆盖条目。
    pub fn lookup(&self, ip: u32) -> Result<usize, u32> {
        for i in (0..self.entries.len()).rev() {
            if self.entries[i].covers(ip) {
                return Ok(i);
            }
        }
        Err(SC_NO_SCOPE)
    }

    /// 裁决：filter 值 → 通道分发并留痕。
    pub fn dispatch(&mut self, ip: u32, filter: u32) -> Result<u8, u32> {
        if self.sealed {
            return Err(SC_FROZEN);
        }
        if filter != FILTER_CONTINUE_SEARCH
            && filter != FILTER_EXECUTE_HANDLER
            && filter != FILTER_CONTINUE_EXEC
        {
            return Err(SC_BAD_FILTER);
        }
        let idx = self.lookup(ip)?;
        let e = self.entries[idx];
        let channel = if filter == FILTER_CONTINUE_EXEC {
            4u8
        } else if filter == FILTER_CONTINUE_SEARCH {
            3u8
        } else if e.is_finally() {
            1u8
        } else {
            2u8
        };
        self.verdicts.push(Adjudication { entry: idx, filter, channel });
        self.ops.push(format!("adj{}c{}", idx, channel));
        Ok(channel)
    }

    /// 裁决账条数。
    pub fn verdict_count(&self) -> usize {
        self.verdicts.len()
    }

    /// 按条目取裁决次数。
    pub fn verdicts_of(&self, idx: usize) -> usize {
        self.verdicts.iter().filter(|v| v.entry == idx).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.begin);
            h = h.wrapping_mul(211).wrapping_add(e.end);
            h = h.wrapping_mul(211).wrapping_add(e.handler);
            h = h.wrapping_mul(211).wrapping_add(e.target);
        }
        for v in &self.verdicts {
            h = h.wrapping_mul(211).wrapping_add(v.entry as u32);
            h = h.wrapping_mul(211).wrapping_add(v.filter);
            h = h.wrapping_mul(211).wrapping_add(v.channel as u32);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("ents={} adj={}", self.entries.len(), self.verdict_count())
    }

    /// 诊断面：条目渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.entries.get(i) {
            Some(e) => e.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 坏区间必拒。
    got[0] = ScopeTable::new()
        .add_entry(ScopeEntry { begin: 0x40, end: 0x40, handler: 1, target: 0 })
        .err()
        == Some(SC_BAD_RANGE);
    // 2) 域重叠必拒。
    let mut t = ScopeTable::new();
    let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 1, target: 0 });
    got[1] = t.add_entry(ScopeEntry { begin: 0x180, end: 0x240, handler: 1, target: 0 })
        .err()
        == Some(SC_OVERLAP);
    // 3) 域外 ip 必拒。
    got[2] = t.lookup(0x300).err() == Some(SC_NO_SCOPE);
    // 4) 坏 filter 必拒。
    got[3] = t.dispatch(0x110, 7).err() == Some(SC_BAD_FILTER);
    // 5) 印账后操作必抓。
    let mut f = ScopeTable::new();
    f.seal();
    got[4] = f.add_entry(ScopeEntry { begin: 0x10, end: 0x20, handler: 1, target: 0 })
        .err()
        == Some(SC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12203");

    // J1 域条目注册。
    let j1 = {
        let mut t = ScopeTable::new();
        t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 0x50, target: 0x300 }).is_ok()
            && t.add_entry(ScopeEntry { begin: 0x200, end: 0x200, handler: 0, target: 0 })
                .err()
                == Some(SC_BAD_RANGE)
    };
    s.add("J1-域条目注册", j1, "begin<end 且不重叠");

    // J2 域表容量。
    let j2 = {
        let mut t = ScopeTable::new();
        let mut ok = true;
        let mut i = 0u32;
        while i < 16 {
            let b = 0x1000 + i * 0x10;
            if t.add_entry(ScopeEntry { begin: b, end: b + 0x10, handler: 0, target: 0 }).is_err() {
                ok = false;
            }
            i += 1;
        }
        ok && t.add_entry(ScopeEntry { begin: 0x9000, end: 0x9010, handler: 0, target: 0 })
            .err()
            == Some(SC_DEPTH)
    };
    s.add("J2-域表容量", j2, "16 条满后再注册必拒");

    // J3 ip 归属。
    let j3 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 1, target: 0 });
        let _ = t.add_entry(ScopeEntry { begin: 0x200, end: 0x300, handler: 2, target: 0 });
        t.lookup(0x2F0) == Ok(1) && t.lookup(0x1F0) == Ok(0) && t.lookup(0x300) == Err(SC_NO_SCOPE)
    };
    s.add("J3-ip归属", j3, "域内命中、域外拒");

    // J4 filter 三值裁决。
    let j4 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 9, target: 0x400 });
        t.dispatch(0x110, FILTER_CONTINUE_SEARCH) == Ok(3)
            && t.dispatch(0x110, FILTER_CONTINUE_EXEC) == Ok(4)
    };
    s.add("J4-filter三值", j4, "0=搜索、-1=继续执行");

    // J5 坏 filter 拒止。
    let j5 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 1, target: 0 });
        t.dispatch(0x110, 2).err() == Some(SC_BAD_FILTER)
    };
    s.add("J5-坏filter拒止", j5, "filter=2 必拒");

    // J6 finally 收口通道。
    let j6 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 9, target: 0 });
        t.dispatch(0x110, FILTER_EXECUTE_HANDLER) == Ok(1) && t.verdicts_of(0) == 1
    };
    s.add("J6-finally通道", j6, "target==0 按收口通道");

    // J7 except 跳转通道。
    let j7 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 9, target: 0x400 });
        t.dispatch(0x110, FILTER_EXECUTE_HANDLER) == Ok(2)
    };
    s.add("J7-except通道", j7, "target!=0 按跳转通道");

    // J8 分发次序自内向外。
    let j8 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x180, handler: 1, target: 0 });
        let _ = t.add_entry(ScopeEntry { begin: 0x180, end: 0x200, handler: 1, target: 0x400 });
        let _ = t.dispatch(0x190, FILTER_EXECUTE_HANDLER);
        let _ = t.dispatch(0x110, FILTER_EXECUTE_HANDLER);
        t.verdicts[0].entry == 1 && t.verdicts[1].entry == 0
    };
    s.add("J8-分发次序", j8, "后注册域先裁决");

    // J9 空表放行。
    let j9 = ScopeTable::new().lookup(0x10).err() == Some(SC_NO_SCOPE);
    s.add("J9-空表放行", j9, "空表裁决即域外拒");

    // J10 印账冻结。
    let j10 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 1, target: 0 });
        t.seal();
        t.add_entry(ScopeEntry { begin: 0x300, end: 0x400, handler: 1, target: 0 })
            .err()
            == Some(SC_FROZEN)
            && t.dispatch(0x110, FILTER_EXECUTE_HANDLER).err() == Some(SC_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后注册/裁决皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 1, target: 0x400 });
        t.fold() == t.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 通道口径。
    let j12 = {
        let f = ScopeEntry { begin: 0x10, end: 0x20, handler: 1, target: 0 };
        let x = ScopeEntry { begin: 0x10, end: 0x20, handler: 1, target: 0x40 };
        f.is_finally() && !x.is_finally() && f.render().contains("t0")
    };
    s.add("J12-通道口径", j12, "finally/except 通道识别");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-摘要回读", {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 9, target: 0x400 });
        let _ = t.dispatch(0x110, FILTER_EXECUTE_HANDLER);
        t.summary() == "ents=1 adj=1"
    }, "一条目一裁决样例");
    s.add("J15-指纹非零", {
        let mut t = ScopeTable::new();
        let _ = t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 9, target: 0 });
        t.fold() != 0
    }, "指纹有效");
    s.add("J16-裁决值口径", FILTER_EXECUTE_HANDLER == 1
        && FILTER_CONTINUE_EXEC == 0xFFFF_FFFF, "裁决值常量口径");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12203_scope_flow() {
        let mut t = ScopeTable::new();
        assert!(t.add_entry(ScopeEntry { begin: 0x100, end: 0x200, handler: 1, target: 0 }).is_ok());
        assert_eq!(t.lookup(0x150), Ok(0));
        assert_eq!(t.dispatch(0x150, 5), Err(SC_BAD_FILTER));
        assert_eq!(t.dispatch(0x150, FILTER_EXECUTE_HANDLER), Ok(1));
    }

    #[test]
    fn unx12203_checks_green() {
        let s = run_unx12203_checks();
        assert!(s.all_passed(), "UNX-F12203 判据未全绿");
    }

    #[test]
    fn unx12203_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
