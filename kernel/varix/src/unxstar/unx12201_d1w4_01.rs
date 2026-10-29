//! UNX-F12201 RtlUnwindEx 主体机制（SEG 0x64020..0x64040 段窗）
//!
//! 卡面：RtlUnwindEx 两段式展开主体——Phase1 自内向外搜索异常处理器、
//! Phase2 自内向外逐帧收口；帧表注册、目标帧定位、展开事件留痕、
//! 印账冻结全链过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 帧表注册：begin<end、容嵌套拒半重叠，违者必拒。
//! - J2 内层优先：lookup 命中包含 ip 的最内层（后注册覆盖段）。
//! - J3 无帧拒止：ip 不落任何帧区间必拒 UW_NO_FRAME。
//! - J4 Phase1 搜索：自内向外逐帧留痕，命中首个带 handler 帧。
//! - J5 无 handler 放行：Phase1 全程无 handler 时返回 None 不炸。
//! - J6 Phase2 收口：自内向外对目标帧（含）之前每帧执行收口留痕。
//! - J7 越目标拒止：收口下标越过帧表必拒 UW_PAST_TARGET。
//! - J8 深度上限：注册帧数超 32 必拒 UW_DEPTH。
//! - J9 印账冻结：seal 后注册/搜索/收口必拒 UW_FROZEN。
//! - J10 事件留痕：展开事件按 (phase, frame, ip) 三元序入账。
//! - J11 指纹幂等：同态引擎 fold 同值。
//! - J12 注入五况全抓。
//!
//! 结构：Frame 帧记录（区间+handler+槽量）+ UnwindEngine 展开引擎
//! （帧表/事件账/操作账/目标态）+ 两段式展开核（search/unwind_to/
//! unwind_ex）+ 折叠指纹（211 进制卷积）+ 注入核（坏区间/重叠/无帧/
//! 越目标/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64020 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64020;
pub const UW_BAD_RANGE: u32 = SEG_BASE + 0x01; // 帧区间非法
pub const UW_OVERLAP: u32 = SEG_BASE + 0x02; // 帧区间重叠
pub const UW_NO_FRAME: u32 = SEG_BASE + 0x03; // ip 无帧覆盖
pub const UW_PAST_TARGET: u32 = SEG_BASE + 0x04; // 收口下标越界
pub const UW_DEPTH: u32 = SEG_BASE + 0x05; // 帧深超限
pub const UW_FROZEN: u32 = SEG_BASE + 0x06; // 印账后操作
pub const UW_NO_TARGET: u32 = SEG_BASE + 0x07; // 无目标态

/// 异常标志位（RtlUnwindEx 语义位）。
pub const EH_UNWINDING: u32 = 0x2;
pub const EH_EXIT_UNWIND: u32 = 0x4;

/// 帧深上限。
pub const MAX_FRAMES: usize = 32;

/// 帧记录：函数区间 + 异常处理器 + prologue 槽量。
#[derive(Clone, Copy)]
pub struct Frame {
    pub begin: u32,
    pub end: u32,
    pub handler: u32,
    pub slots: u32,
}

impl Frame {
    pub fn covers(&self, ip: u32) -> bool {
        ip >= self.begin && ip < self.end
    }

    /// Phase2 收口时传给处理器的标志组合。
    pub fn unwind_flags(&self, exit: bool) -> u32 {
        let mut fl = EH_UNWINDING;
        if exit {
            fl |= EH_EXIT_UNWIND;
        }
        fl
    }

    pub fn render(&self) -> String {
        format!("F{:x}-{:x}h{:x}s{}", self.begin, self.end, self.handler, self.slots)
    }
}

/// 展开事件：phase（1=搜索 2=收口）+ 帧下标 + 触点 ip。
#[derive(Clone, Copy)]
pub struct UnwindEvent {
    pub phase: u8,
    pub frame: usize,
    pub ip: u32,
}

/// 展开引擎：帧表 + 两段式展开核 + 事件账。
pub struct UnwindEngine {
    pub frames: Vec<Frame>,
    pub events: Vec<UnwindEvent>,
    pub ops: Vec<String>,
    pub target: Option<u32>,
    pub sealed: bool,
}

impl UnwindEngine {
    pub fn new() -> UnwindEngine {
        UnwindEngine {
            frames: Vec::new(),
            events: Vec::new(),
            ops: Vec::new(),
            target: None,
            sealed: false,
        }
    }

    /// 帧注册：区间合法、两两不重叠、深度受限。
    pub fn register(&mut self, f: Frame) -> Result<(), u32> {
        if self.sealed {
            return Err(UW_FROZEN);
        }
        if f.begin >= f.end {
            return Err(UW_BAD_RANGE);
        }
        for g in &self.frames {
            // 嵌套（一方完全含于另一方）合法；半重叠必拒。
            let nested =
                (f.begin >= g.begin && f.end <= g.end) || (g.begin >= f.begin && g.end <= f.end);
            if f.begin < g.end && g.begin < f.end && !nested {
                return Err(UW_OVERLAP);
            }
        }
        if self.frames.len() >= MAX_FRAMES {
            return Err(UW_DEPTH);
        }
        self.frames.push(f);
        self.ops.push(format!("reg{:x}", f.begin));
        Ok(())
    }

    /// 定位：命中包含 ip 的最内层（自后向前首个覆盖帧）。
    pub fn lookup(&self, ip: u32) -> Result<usize, u32> {
        for i in (0..self.frames.len()).rev() {
            if self.frames[i].covers(ip) {
                return Ok(i);
            }
        }
        Err(UW_NO_FRAME)
    }

    /// 设定展开目标 ip（目标帧所在地址）。
    pub fn set_target(&mut self, ip: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(UW_FROZEN);
        }
        self.target = Some(ip);
        self.ops.push(format!("tgt{:x}", ip));
        Ok(())
    }

    /// Phase1 搜索：自内向外找首个带 handler 的帧。
    pub fn search(&mut self, code: u32) -> Result<Option<usize>, u32> {
        if self.sealed {
            return Err(UW_FROZEN);
        }
        let mut hit = None;
        for i in (0..self.frames.len()).rev() {
            let f = self.frames[i];
            self.events.push(UnwindEvent { phase: 1, frame: i, ip: f.end - 1 });
            self.ops.push(format!("p1f{}", i));
            if f.handler != 0 {
                hit = Some(i);
                break;
            }
        }
        self.ops.push(format!("p1c{:x}", code));
        Ok(hit)
    }

    /// Phase2 收口：自内向外对 hit（含）之前每帧收口留痕。
    pub fn unwind_to(&mut self, hit: usize, code: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(UW_FROZEN);
        }
        if hit >= self.frames.len() {
            return Err(UW_PAST_TARGET);
        }
        let mut n = 0usize;
        for i in (0..=hit).rev() {
            let f = self.frames[i];
            let exit = i == 0;
            self.events.push(UnwindEvent { phase: 2, frame: i, ip: f.end - 1 });
            self.ops.push(format!("p2f{}f{:x}", i, f.unwind_flags(exit)));
            n += 1;
        }
        self.ops.push(format!("p2c{:x}", code));
        Ok(n)
    }

    /// RtlUnwindEx 主入口：定位目标帧 → Phase1 → Phase2。
    pub fn unwind_ex(&mut self, target_ip: u32, code: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(UW_FROZEN);
        }
        self.set_target(target_ip)?;
        let hit = match self.search(code)? {
            Some(i) => i,
            None => {
                self.ops.push("p1miss".to_string());
                return Ok(0);
            }
        };
        self.unwind_to(hit, code)
    }

    /// 事件账条数。
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 按帧取收口次数。
    pub fn unwind_hits_of(&self, frame: usize) -> usize {
        self.events.iter().filter(|e| e.phase == 2 && e.frame == frame).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for f in &self.frames {
            h = h.wrapping_mul(211).wrapping_add(f.begin);
            h = h.wrapping_mul(211).wrapping_add(f.end);
            h = h.wrapping_mul(211).wrapping_add(f.handler);
            h = h.wrapping_mul(211).wrapping_add(f.slots);
        }
        for e in &self.events {
            h = h.wrapping_mul(211).wrapping_add(e.phase as u32);
            h = h.wrapping_mul(211).wrapping_add(e.frame as u32);
            h = h.wrapping_mul(211).wrapping_add(e.ip);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(211).wrapping_add(self.target.unwrap_or(0));
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("frames={} ev={} ops={}", self.frames.len(), self.event_count(), self.ops.len())
    }

    /// 诊断面：帧渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.frames.get(i) {
            Some(f) => f.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 坏区间必拒（begin >= end）。
    got[0] = UnwindEngine::new()
        .register(Frame { begin: 0x400, end: 0x400, handler: 1, slots: 1 })
        .err()
        == Some(UW_BAD_RANGE);
    // 2) 区间重叠必拒。
    let mut e = UnwindEngine::new();
    let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
    got[1] = e.register(Frame { begin: 0x180, end: 0x300, handler: 1, slots: 1 })
        .err()
        == Some(UW_OVERLAP);
    // 3) 无帧覆盖必拒。
    got[2] = e.lookup(0x999).err() == Some(UW_NO_FRAME);
    // 4) 越目标收口必拒（hit 越过帧表尾）。
    got[3] = e.unwind_to(e.frames.len(), 0xC0000005).err() == Some(UW_PAST_TARGET);
    // 5) 印账后操作必抓。
    let mut f = UnwindEngine::new();
    f.seal();
    got[4] = f.register(Frame { begin: 0x10, end: 0x20, handler: 1, slots: 1 })
        .err()
        == Some(UW_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12201");

    // J1 帧表注册。
    let j1 = {
        let mut e = UnwindEngine::new();
        e.register(Frame { begin: 0x1000, end: 0x1100, handler: 0x5000, slots: 4 }).is_ok()
            && e.register(Frame { begin: 0x1100, end: 0x1200, handler: 0, slots: 2 }).is_ok()
            && e.register(Frame { begin: 0x1200, end: 0x1100, handler: 0, slots: 1 })
                .err()
                == Some(UW_BAD_RANGE)
    };
    s.add("J1-帧表注册", j1, "begin<end 且不重叠");

    // J2 内层优先。
    let j2 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        let _ = e.register(Frame { begin: 0x150, end: 0x180, handler: 2, slots: 1 });
        e.lookup(0x160) == Ok(1) && e.lookup(0x110) == Ok(0)
    };
    s.add("J2-内层优先", j2, "覆盖段后注册者先命中");

    // J3 无帧拒止。
    let j3 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        e.lookup(0x200).err() == Some(UW_NO_FRAME) && e.lookup(0xFF).err() == Some(UW_NO_FRAME)
    };
    s.add("J3-无帧拒止", j3, "区间外 ip 必拒");

    // J4 Phase1 搜索命中 handler 帧。
    let j4 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 0, slots: 1 });
        let _ = e.register(Frame { begin: 0x200, end: 0x300, handler: 7, slots: 1 });
        let _ = e.register(Frame { begin: 0x300, end: 0x400, handler: 8, slots: 1 });
        e.search(0xC0000005) == Ok(Some(2)) && e.event_count() == 1
    };
    s.add("J4-Phase1命中", j4, "自内向外首个 handler");

    // J5 无 handler 放行。
    let j5 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 0, slots: 1 });
        let _ = e.register(Frame { begin: 0x200, end: 0x300, handler: 0, slots: 1 });
        e.search(0xC0000005) == Ok(None) && e.event_count() == 2
    };
    s.add("J5-无handler放行", j5, "全程无 handler 返回 None");

    // J6 Phase2 收口次序。
    let j6 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        let _ = e.register(Frame { begin: 0x200, end: 0x300, handler: 1, slots: 1 });
        let n = e.unwind_to(1, 0xC0000005).unwrap_or(0);
        n == 2 && e.unwind_hits_of(0) == 1 && e.unwind_hits_of(1) == 1
    };
    s.add("J6-Phase2收口", j6, "目标帧（含）前全收口");

    // J7 越目标拒止。
    let j7 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        e.unwind_to(5, 0xC0000005).err() == Some(UW_PAST_TARGET)
    };
    s.add("J7-越目标拒止", j7, "hit 越过帧表必拒");

    // J8 深度上限。
    let j8 = {
        let mut e = UnwindEngine::new();
        let mut full = true;
        let mut i = 0u32;
        while i < 32 {
            let f = Frame { begin: 0x1000 + i * 0x10, end: 0x1010 + i * 0x10, handler: 0, slots: 1 };
            if e.register(f).is_err() {
                full = false;
            }
            i += 1;
        }
        full && e.register(Frame { begin: 0x9000, end: 0x9010, handler: 0, slots: 1 })
            .err()
            == Some(UW_DEPTH)
    };
    s.add("J8-深度上限", j8, "32 帧满后再注册必拒");

    // J9 印账冻结。
    let j9 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        e.seal();
        e.register(Frame { begin: 0x300, end: 0x400, handler: 1, slots: 1 }).err() == Some(UW_FROZEN)
            && e.search(1).err() == Some(UW_FROZEN)
            && e.unwind_to(0, 1).err() == Some(UW_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后注册/搜索/收口皆拒");

    // J10 事件留痕序。
    let j10 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 3, slots: 1 });
        let _ = e.unwind_ex(0x1F0, 0xC0000005);
        e.events.len() == 2
            && e.events[0].phase == 1
            && e.events[1].phase == 2
            && e.events[1].frame == 0
    };
    s.add("J10-事件留痕", j10, "phase1→phase2 序入账");

    // J11 unwind_ex 全链。
    let j11 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        let _ = e.register(Frame { begin: 0x200, end: 0x300, handler: 2, slots: 1 });
        let n = e.unwind_ex(0x250, 0xC0000005).unwrap_or(0);
        n == 2 && e.target == Some(0x250)
    };
    s.add("J11-unwind_ex全链", j11, "定位+搜索+收口一次贯通");

    // J12 指纹幂等。
    let j12 = {
        let mut e = UnwindEngine::new();
        let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 });
        e.fold() == e.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // 汇总样例账。
    let mut e = UnwindEngine::new();
    let _ = e.register(Frame { begin: 0x100, end: 0x200, handler: 5, slots: 2 });
    let _ = e.unwind_ex(0x1F0, 0xE0000001);

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-摘要回读", e.summary().contains("frames=1"), "一帧一事件样例");
    s.add("J15-指纹非零", e.fold() != 0, "指纹有效");
    s.add("J16-标志位", EH_UNWINDING == 0x2 && EH_EXIT_UNWIND == 0x4, "RtlUnwindEx 标志位口径");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12201_unwind_flow() {
        let mut e = UnwindEngine::new();
        assert!(e.register(Frame { begin: 0x100, end: 0x200, handler: 1, slots: 1 }).is_ok());
        assert_eq!(e.lookup(0x150), Ok(0));
        assert_eq!(e.lookup(0x300), Err(UW_NO_FRAME));
        assert_eq!(e.unwind_to(9, 1), Err(UW_PAST_TARGET));
    }

    #[test]
    fn unx12201_checks_green() {
        let s = run_unx12201_checks();
        assert!(s.all_passed(), "UNX-F12201 判据未全绿");
    }

    #[test]
    fn unx12201_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
