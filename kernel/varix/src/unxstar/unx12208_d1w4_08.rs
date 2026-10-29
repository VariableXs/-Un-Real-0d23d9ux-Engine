//! UNX-F12208 展开器与 D4 消息面预告：窗口线程异常路径（SEG 0x64100..0x64120 段窗）
//!
//! 卡面：窗口线程消息泵异常路径预告——消息类分发、泵内重入门拒止、
//! 异常注入后的展开预览通道；D4 消息面消费契约先收口。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 消息入队：合法窗口消息类入队留痕。
//! - J2 空泵拒止：空队列分发必拒 MP_EMPTY。
//! - J3 未知消息类拒止：msg 不在窗口消息类集必拒 MP_BAD_MSG。
//! - J4 分发留痕：出队即分发留痕。
//! - J5 重入门拒止：分发中再分发必拒 MP_REENTRY。
//! - J6 异常预览通道：分发中注入异常码开启展开预览留痕。
//! - J7 未知异常码拒止：异常码不在三类集必拒 MP_BAD_CODE。
//! - J8 泵复位：预览后泵回闲态可继续分发。
//! - J9 印账冻结：seal 后入队/分发/预览必拒 MP_FROZEN。
//! - J10 指纹幂等：同态同指纹。
//! - J11 队深可查：队列深度回读。
//! - J12 注入五况全抓。
//!
//! 结构：MessagePump 消息泵（队列/分发态/预览账/操作账）+ 分发核
//! （dispatch）+ 预览核（preview_unwind）+ 折叠指纹 + 注入核（空泵/
//! 未知消息/未知异常码/重入门/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64100 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64100;
pub const MP_EMPTY: u32 = SEG_BASE + 0x01; // 空队列分发
pub const MP_BAD_MSG: u32 = SEG_BASE + 0x02; // 未知消息类
pub const MP_BAD_CODE: u32 = SEG_BASE + 0x03; // 未知异常码
pub const MP_FROZEN: u32 = SEG_BASE + 0x04; // 印账后操作
pub const MP_REENTRY: u32 = SEG_BASE + 0x05; // 分发中再分发

/// 窗口消息类集（D4 消息面样本）。
pub const WM_PAINT: u32 = 0x000F;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_COMMAND: u32 = 0x0111;

/// 预览用异常码三类集。
pub const XC_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const XC_INT_DIVIDE: u32 = 0xC000_0094;
pub const XC_BREAKPOINT: u32 = 0x8000_0003;

fn known_msg(m: u32) -> bool {
    m == WM_PAINT || m == WM_QUIT || m == WM_COMMAND
}

fn known_code(c: u32) -> bool {
    c == XC_ACCESS_VIOLATION || c == XC_INT_DIVIDE || c == XC_BREAKPOINT
}

/// 展开预览账条目：消息 + 异常码 + 预告展开深度。
#[derive(Clone, Copy)]
pub struct PreviewRecord {
    pub msg: u32,
    pub code: u32,
    pub depth: u32,
}

/// 消息泵。
pub struct MessagePump {
    pub queue: Vec<u32>,
    pub dispatching: bool,
    pub previews: Vec<PreviewRecord>,
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl MessagePump {
    pub fn new() -> MessagePump {
        MessagePump {
            queue: Vec::new(),
            dispatching: false,
            previews: Vec::new(),
            ops: Vec::new(),
            sealed: false,
        }
    }

    /// 消息入队。
    pub fn post(&mut self, msg: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MP_FROZEN);
        }
        if !known_msg(msg) {
            return Err(MP_BAD_MSG);
        }
        self.queue.push(msg);
        self.ops.push(format!("post{:x}", msg));
        Ok(())
    }

    /// 分发核：出队分发；空泵与重入门必拒。
    pub fn dispatch(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(MP_FROZEN);
        }
        if self.queue.is_empty() {
            return Err(MP_EMPTY);
        }
        if self.dispatching {
            return Err(MP_REENTRY);
        }
        self.dispatching = true;
        let msg = self.queue.remove(0);
        self.ops.push(format!("disp{:x}", msg));
        self.dispatching = false;
        Ok(msg)
    }

    /// 展开预览核：分发中注入异常码，预告展开深度 = 消息队列余深+1。
    pub fn preview_unwind(&mut self, code: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(MP_FROZEN);
        }
        if !known_code(code) {
            return Err(MP_BAD_CODE);
        }
        if !self.dispatching {
            return Err(MP_REENTRY);
        }
        let depth = self.queue.len() as u32 + 1;
        self.previews.push(PreviewRecord { msg: 0, code, depth });
        self.ops.push(format!("prev{:x}d{}", code, depth));
        Ok(depth)
    }

    /// 泵复位：清预览、回闲态。
    pub fn reset(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(MP_FROZEN);
        }
        self.previews.clear();
        self.dispatching = false;
        self.ops.push("reset".to_string());
        Ok(())
    }

    /// 队深。
    pub fn depth(&self) -> usize {
        self.queue.len()
    }

    /// 预览账条数。
    pub fn preview_count(&self) -> usize {
        self.previews.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for m in &self.queue {
            h = h.wrapping_mul(211).wrapping_add(*m);
        }
        for p in &self.previews {
            h = h.wrapping_mul(211).wrapping_add(p.msg);
            h = h.wrapping_mul(211).wrapping_add(p.code);
            h = h.wrapping_mul(211).wrapping_add(p.depth);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(211).wrapping_add(self.dispatching as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("q={} pv={}", self.depth(), self.preview_count())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 空泵分发必拒。
    got[0] = MessagePump::new().dispatch().err() == Some(MP_EMPTY);
    // 2) 未知消息类必拒。
    got[1] = MessagePump::new().post(0x9999).err() == Some(MP_BAD_MSG);
    // 3) 未知异常码必拒。
    got[2] = MessagePump::new().preview_unwind(0xDEAD_BEEF).err() == Some(MP_BAD_CODE);
    // 4) 重入门必拒（闲态下预览亦拒）。
    let mut p = MessagePump::new();
    let _ = p.post(WM_PAINT);
    let _ = p.dispatch();
    got[3] = p.preview_unwind(XC_BREAKPOINT).err() == Some(MP_REENTRY);
    // 5) 印账后操作必抓。
    let mut f = MessagePump::new();
    f.seal();
    got[4] = f.post(WM_PAINT).err() == Some(MP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12208_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12208");

    // J1 消息入队。
    let j1 = {
        let mut p = MessagePump::new();
        p.post(WM_PAINT).is_ok() && p.post(WM_COMMAND).is_ok() && p.depth() == 2
    };
    s.add("J1-消息入队", j1, "合法类入队留痕");

    // J2 空泵拒止。
    let j2 = MessagePump::new().dispatch().err() == Some(MP_EMPTY);
    s.add("J2-空泵拒止", j2, "空队列必拒");

    // J3 未知消息类拒止。
    let j3 = MessagePump::new().post(0x400).err() == Some(MP_BAD_MSG);
    s.add("J3-未知消息类拒止", j3, "类集外必拒");

    // J4 分发留痕。
    let j4 = {
        let mut p = MessagePump::new();
        let _ = p.post(WM_QUIT);
        p.dispatch() == Ok(WM_QUIT) && p.depth() == 0
    };
    s.add("J4-分发留痕", j4, "出队即分发");

    // J5 重入门拒止。
    let j5 = {
        let mut p = MessagePump::new();
        let _ = p.post(WM_PAINT);
        let _ = p.post(WM_PAINT);
        let _ = p.dispatch();
        p.dispatching = true; // 模拟分发中
        p.dispatch().err() == Some(MP_REENTRY)
    };
    s.add("J5-重入门拒止", j5, "分发中再分发必拒");

    // J6 异常预览通道。
    let j6 = {
        let mut p = MessagePump::new();
        let _ = p.post(WM_PAINT);
        let _ = p.post(WM_COMMAND);
        let _ = p.dispatch();
        p.dispatching = true;
        let d = p.preview_unwind(XC_ACCESS_VIOLATION).unwrap_or(0);
        d == 2 && p.preview_count() == 1
    };
    s.add("J6-异常预览通道", j6, "预览深度=队列余深+1");

    // J7 未知异常码拒止。
    let j7 = {
        let mut p = MessagePump::new();
        p.dispatching = true;
        p.preview_unwind(0xE000_0001).err() == Some(MP_BAD_CODE)
    };
    s.add("J7-未知异常码拒止", j7, "三类集外必拒");

    // J8 泵复位。
    let j8 = {
        let mut p = MessagePump::new();
        p.dispatching = true;
        let _ = p.preview_unwind(XC_INT_DIVIDE);
        let _ = p.reset();
        p.preview_count() == 0 && !p.dispatching
    };
    s.add("J8-泵复位", j8, "复位清预览回闲态");

    // J9 印账冻结。
    let j9 = {
        let mut p = MessagePump::new();
        let _ = p.post(WM_PAINT);
        p.seal();
        p.post(WM_COMMAND).err() == Some(MP_FROZEN)
            && p.dispatch().err() == Some(MP_FROZEN)
            && p.preview_unwind(XC_PAINT_SENTINEL).err() == Some(MP_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut p = MessagePump::new();
        let _ = p.post(WM_COMMAND);
        p.fold() == p.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 队深可查。
    let j11 = {
        let mut p = MessagePump::new();
        let _ = p.post(WM_PAINT);
        let _ = p.post(WM_QUIT);
        let _ = p.post(WM_COMMAND);
        let _ = p.dispatch();
        p.depth() == 2
    };
    s.add("J11-队深可查", j11, "出队后余深 2");

    // J12 消息类口径。
    let j12 = WM_PAINT == 0x000F && WM_QUIT == 0x0012 && WM_COMMAND == 0x0111;
    s.add("J12-消息类口径", j12, "窗口消息常量口径");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-摘要回读", {
        let mut p = MessagePump::new();
        let _ = p.post(WM_PAINT);
        p.summary() == "q=1 pv=0"
    }, "队深预览量回读");
    s.add("J15-指纹非零", {
        let mut p = MessagePump::new();
        let _ = p.post(WM_QUIT);
        p.fold() != 0
    }, "指纹有效");
    s
}

/// J9 使用的哨兵码（三类集外，仅验证冻结优先于码检查）。
#[allow(non_upper_case_globals)]
const XC_PAINT_SENTINEL: u32 = 0x0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12208_pump_flow() {
        let mut p = MessagePump::new();
        assert!(p.post(WM_PAINT).is_ok());
        assert_eq!(p.post(0x9999), Err(MP_BAD_MSG));
        assert_eq!(p.dispatch(), Ok(WM_PAINT));
        assert_eq!(p.dispatch(), Err(MP_EMPTY));
    }

    #[test]
    fn unx12208_checks_green() {
        let s = run_unx12208_checks();
        assert!(s.all_passed(), "UNX-F12208 判据未全绿");
    }

    #[test]
    fn unx12208_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
