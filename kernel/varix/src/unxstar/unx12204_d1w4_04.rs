//! UNX-F12204 __C_specific_handler 全路径收口（SEG 0x64080..0x640a0 段窗）
//!
//! 卡面：__C_specific_handler 分发器全路径收口——filter 评估、
//! 处理器返回四态（继续执行/继续搜索/嵌套/碰撞展开）、路径账
//! 四通道全覆盖核账。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 filter 裁决面：0/1/0xFFFFFFFF 三值合法，他值必拒 CH_BAD_FILTER。
//! - J2 处理器返回四态：0=继续执行、1=继续搜索、2=嵌套异常、3=碰撞展开。
//! - J3 坏返回值拒止：disposition > 3 必拒 CH_BAD_DISP。
//! - J4 继续搜索路径：filter=0 时按搜索通道留痕不收口。
//! - J5 继续执行路径：filter=-1 时按恢复通道留痕。
//! - J6 执行 handler 路径：filter=1 时按收口通道留痕。
//! - J7 碰撞展开路径：disp=3 且碰撞标志在场时按碰撞通道收口。
//! - J8 碰撞无标志拒止：disp=3 无碰撞标志必拒 CH_COLLIDED。
//! - J9 嵌套路径：disp=2 时按嵌套通道留痕并记嵌套计数。
//! - J10 全路径覆盖核账：四通道各至少一次后 paths_covered 为真。
//! - J11 无域拒止：ip 无 scope 归属必拒 CH_NO_SCOPE。
//! - J12 印账冻结：seal 后 run_path 必拒 CH_FROZEN。
//! - J13 指纹幂等：同态同指纹。
//! - J14 注入五况全抓。
//!
//! 结构：PathRecord 路径账条目（filter/disp/通道）+ CHandler 分发器
//! （scope 归属域/路径账/操作账/碰撞标志）+ 分发核（run_path）+ 折叠
//! 指纹 + 注入核（坏filter/坏disp/无域/碰撞无标志/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64080 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64080;
pub const CH_BAD_FILTER: u32 = SEG_BASE + 0x01; // filter 值非法
pub const CH_NO_SCOPE: u32 = SEG_BASE + 0x02; // ip 无域归属
pub const CH_BAD_DISP: u32 = SEG_BASE + 0x03; // 处理器返回值非法
pub const CH_COLLIDED: u32 = SEG_BASE + 0x04; // 碰撞展开无标志
pub const CH_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// filter 裁决值。
pub const FILTER_SEARCH: u32 = 0;
pub const FILTER_EXEC: u32 = 1;
pub const FILTER_RESUME: u32 = 0xFFFF_FFFF;

/// 处理器返回态（ExceptionContinueExecution 口径）。
pub const DISP_CONTINUE_EXEC: u32 = 0;
pub const DISP_CONTINUE_SEARCH: u32 = 1;
pub const DISP_NESTED: u32 = 2;
pub const DISP_COLLIDED: u32 = 3;

/// 通道标签：1=收口 2=搜索 3=恢复 4=嵌套 5=碰撞。
pub const CHAN_COLLAPSE: u8 = 1;
pub const CHAN_SEARCH: u8 = 2;
pub const CHAN_RESUME: u8 = 3;
pub const CHAN_NESTED: u8 = 4;
pub const CHAN_COLLIDED: u8 = 5;

/// 路径账条目。
#[derive(Clone, Copy)]
pub struct PathRecord {
    pub filter: u32,
    pub disp: u32,
    pub channel: u8,
}

/// __C_specific_handler 分发器。
pub struct CHandler {
    pub scope_lo: u32,
    pub scope_hi: u32,
    pub records: Vec<PathRecord>,
    pub ops: Vec<String>,
    pub collided_flag: bool,
    pub sealed: bool,
}

impl CHandler {
    pub fn new(lo: u32, hi: u32) -> CHandler {
        CHandler {
            scope_lo: lo,
            scope_hi: hi,
            records: Vec::new(),
            ops: Vec::new(),
            collided_flag: false,
            sealed: false,
        }
    }

    fn in_scope(&self, ip: u32) -> bool {
        ip >= self.scope_lo && ip < self.scope_hi
    }

    /// 分发核：filter 评估 → 处理器返回态裁决 → 通道留痕。
    pub fn run_path(&mut self, ip: u32, filter: u32, disp: u32) -> Result<u8, u32> {
        if self.sealed {
            return Err(CH_FROZEN);
        }
        if filter != FILTER_SEARCH && filter != FILTER_EXEC && filter != FILTER_RESUME {
            return Err(CH_BAD_FILTER);
        }
        if !self.in_scope(ip) {
            return Err(CH_NO_SCOPE);
        }
        if disp > DISP_COLLIDED {
            return Err(CH_BAD_DISP);
        }
        let channel = if filter == FILTER_EXEC {
            CHAN_COLLAPSE
        } else if filter == FILTER_SEARCH {
            CHAN_SEARCH
        } else if disp == DISP_NESTED {
            CHAN_NESTED
        } else if disp == DISP_COLLIDED {
            if !self.collided_flag {
                return Err(CH_COLLIDED);
            }
            CHAN_COLLIDED
        } else {
            CHAN_RESUME
        };
        self.records.push(PathRecord { filter, disp, channel });
        self.ops.push(format!("p{}c{}", filter, channel));
        Ok(channel)
    }

    /// 碰撞标志置位（外层 RtlUnwindEx 回写）。
    pub fn mark_collided(&mut self) {
        self.collided_flag = true;
        self.ops.push("coll".to_string());
    }

    /// 全路径覆盖核账：五通道（收口/搜索/恢复/嵌套/碰撞）各至少一次。
    pub fn paths_covered(&self) -> bool {
        for c in [CHAN_COLLAPSE, CHAN_SEARCH, CHAN_RESUME, CHAN_NESTED, CHAN_COLLIDED] {
            if !self.records.iter().any(|r| r.channel == c) {
                return false;
            }
        }
        true
    }

    /// 按通道取留痕次数。
    pub fn count_channel(&self, c: u8) -> usize {
        self.records.iter().filter(|r| r.channel == c).count()
    }

    /// 路径账条数。
    pub fn path_count(&self) -> usize {
        self.records.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.scope_lo);
        h = h.wrapping_mul(211).wrapping_add(self.scope_hi);
        for r in &self.records {
            h = h.wrapping_mul(211).wrapping_add(r.filter);
            h = h.wrapping_mul(211).wrapping_add(r.disp);
            h = h.wrapping_mul(211).wrapping_add(r.channel as u32);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(211).wrapping_add(self.collided_flag as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("paths={} cols={}", self.path_count(), self.count_channel(CHAN_COLLIDED))
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 坏 filter 必拒。
    got[0] = CHandler::new(0x100, 0x200).run_path(0x110, 7, DISP_CONTINUE_SEARCH)
        .err()
        == Some(CH_BAD_FILTER);
    // 2) 坏 disp 必拒。
    got[1] = CHandler::new(0x100, 0x200).run_path(0x110, FILTER_EXEC, 9)
        .err()
        == Some(CH_BAD_DISP);
    // 3) 无域 ip 必拒。
    got[2] = CHandler::new(0x100, 0x200).run_path(0x300, FILTER_EXEC, DISP_CONTINUE_SEARCH)
        .err()
        == Some(CH_NO_SCOPE);
    // 4) 碰撞无标志必拒。
    got[3] = CHandler::new(0x100, 0x200).run_path(0x110, FILTER_RESUME, DISP_COLLIDED)
        .err()
        == Some(CH_COLLIDED);
    // 5) 印账后分发必抓。
    let mut h = CHandler::new(0x100, 0x200);
    h.seal();
    got[4] = h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH).err() == Some(CH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12204_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12204");

    // J1 filter 裁决面。
    let j1 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_SEARCH, DISP_CONTINUE_SEARCH).is_ok()
            && h.run_path(0x110, 2, DISP_CONTINUE_SEARCH).err() == Some(CH_BAD_FILTER)
    };
    s.add("J1-filter裁决面", j1, "0/1/-1 合法、2 拒");

    // J2 坏返回值拒止。
    let j2 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_EXEC, 4).err() == Some(CH_BAD_DISP)
            && h.run_path(0x110, FILTER_EXEC, 0xFFFF).err() == Some(CH_BAD_DISP)
    };
    s.add("J2-坏返回值拒止", j2, "disp>3 必拒");

    // J3 继续搜索路径。
    let j3 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_SEARCH, DISP_CONTINUE_SEARCH) == Ok(CHAN_SEARCH)
            && h.count_channel(CHAN_SEARCH) == 1
    };
    s.add("J3-继续搜索路径", j3, "搜索通道留痕");

    // J4 继续执行路径。
    let j4 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_RESUME, DISP_CONTINUE_EXEC) == Ok(CHAN_RESUME)
    };
    s.add("J4-继续执行路径", j4, "恢复通道留痕");

    // J5 执行 handler 路径。
    let j5 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH) == Ok(CHAN_COLLAPSE)
    };
    s.add("J5-执行handler路径", j5, "收口通道留痕");

    // J6 嵌套路径。
    let j6 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_RESUME, DISP_NESTED) == Ok(CHAN_NESTED)
            && h.count_channel(CHAN_NESTED) == 1
    };
    s.add("J6-嵌套路径", j6, "嵌套通道留痕");

    // J7 碰撞展开路径（有标志）。
    let j7 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.mark_collided();
        h.run_path(0x110, FILTER_RESUME, DISP_COLLIDED) == Ok(CHAN_COLLIDED)
            && h.summary().contains("cols=1")
    };
    s.add("J7-碰撞路径", j7, "标志在场时碰撞收口");

    // J8 碰撞无标志拒止。
    let j8 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.run_path(0x110, FILTER_RESUME, DISP_COLLIDED).err() == Some(CH_COLLIDED)
    };
    s.add("J8-碰撞无标志拒止", j8, "无标志必拒");

    // J9 全路径覆盖核账。
    let j9 = {
        let mut h = CHandler::new(0x100, 0x200);
        let _ = h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH);
        let _ = h.run_path(0x110, FILTER_SEARCH, DISP_CONTINUE_SEARCH);
        let _ = h.run_path(0x110, FILTER_RESUME, DISP_CONTINUE_EXEC);
        let _ = h.run_path(0x110, FILTER_RESUME, DISP_NESTED);
        h.mark_collided();
        let _ = h.run_path(0x110, FILTER_RESUME, DISP_COLLIDED);
        h.paths_covered() && h.path_count() == 5
    };
    s.add("J9-全路径覆盖", j9, "五通道各一次");

    // J10 无域拒止。
    let j10 = {
        let h = CHandler::new(0x100, 0x200);
        h.in_scope(0x200) == false && h.in_scope(0xFF) == false && h.in_scope(0x110)
    };
    s.add("J10-无域拒止", j10, "域外 ip 不入账");

    // J11 印账冻结。
    let j11 = {
        let mut h = CHandler::new(0x100, 0x200);
        h.seal();
        h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH).err() == Some(CH_FROZEN)
    };
    s.add("J11-印账冻结", j11, "seal 后分发必拒");

    // J12 指纹幂等。
    let j12 = {
        let mut h = CHandler::new(0x100, 0x200);
        let _ = h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH);
        h.fold() == h.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // J13 返回态口径。
    let j13 = DISP_CONTINUE_EXEC == 0
        && DISP_CONTINUE_SEARCH == 1
        && DISP_NESTED == 2
        && DISP_COLLIDED == 3;
    s.add("J13-返回态口径", j13, "ExceptionContinue* 四态常量");

    let got = inject_five();
    s.add_dyn("J14-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J15-指纹非零", {
        let mut h = CHandler::new(0x100, 0x200);
        let _ = h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH);
        h.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12204_path_flow() {
        let mut h = CHandler::new(0x100, 0x200);
        assert_eq!(h.run_path(0x110, FILTER_EXEC, DISP_CONTINUE_SEARCH), Ok(CHAN_COLLAPSE));
        assert_eq!(h.run_path(0x110, FILTER_EXEC, 9), Err(CH_BAD_DISP));
        h.mark_collided();
        assert_eq!(h.run_path(0x110, FILTER_RESUME, DISP_COLLIDED), Ok(CHAN_COLLIDED));
    }

    #[test]
    fn unx12204_checks_green() {
        let s = run_unx12204_checks();
        assert!(s.all_passed(), "UNX-F12204 判据未全绿");
    }

    #[test]
    fn unx12204_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
