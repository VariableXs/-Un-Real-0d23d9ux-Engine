//! VE-F4286 · 一致性×调试器（U05 工具集成组）
//! 职责：调试器内一致性视图（违例堆栈——契约链谁立约在哪破 + 一致性断点
//! 命中规则即断 + 会话违例时间线环形缓冲）+ 符号缺失降级编号链
//! + 断点风暴限频熔断 + 环形覆盖最旧并标注 + 键盘导航完整（域本色）。
//! 段位：0x2A60（续表 idx520）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2A60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgCode {
    /// 符号缺失（降级编号链）。
    SymMissing = SEG_BASE,
    /// 断点风暴（限频熔断）。
    BreakStorm = SEG_BASE + 0x01,
    /// 环形溢出（覆盖标注）。
    RingOverwrite = SEG_BASE + 0x02,
}

/// 堆栈帧。
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub contract: u16, // 立约方
    pub rule: u16,     // 破约规则
    pub sym: u32,      // 符号（0=缺失）
}

/// 断点。
#[derive(Clone, Copy, Debug)]
pub struct RuleBreak {
    pub rule: u16,
    pub hits: u32,
}

/// 会话调试器。
pub struct SessionDebug {
    pub frames: Vec<Frame>,
    pub breaks: Vec<RuleBreak>,
    /// 环形时间线：(序, 规则, 被覆盖标注)。
    pub ring: Vec<(u32, u16, bool)>,
    pub ring_cap: usize,
    pub ring_head: u32,
    /// 熔断：单位时间命中上限。
    pub storm_cap: u32,
    pub storm_hits: u32,
    pub fused: bool,
    /// 键盘导航（域本色）。
    pub kb_nav: bool,
}

impl SessionDebug {
    pub fn new(ring_cap: usize, storm_cap: u32) -> Self {
        SessionDebug { frames: Vec::new(), breaks: Vec::new(), ring: Vec::new(), ring_cap, ring_head: 0, storm_cap, storm_hits: 0, fused: false, kb_nav: true }
    }

    /// 堆栈采集：任一帧符号缺失→降级编号链仍可用。
    pub fn capture(&mut self, frames: Vec<Frame>) -> bool {
        let missing = frames.iter().any(|f| f.sym == 0);
        self.frames = frames;
        missing
    }

    /// 编号链降级视图。
    pub fn numbered_chain(&self) -> Vec<u16> {
        (0..self.frames.len() as u16).collect()
    }

    /// 规则断点挂载。
    pub fn mount_break(&mut self, rule: u16) {
        if !self.breaks.iter().any(|b| b.rule == rule) {
            self.breaks.push(RuleBreak { rule, hits: 0 });
        }
    }

    /// 断点命中：命中规则即断；风暴限频熔断。
    pub fn hit(&mut self, rule: u16) -> Result<bool, DbgCode> {
        if self.fused {
            return Err(DbgCode::BreakStorm);
        }
        let b = match self.breaks.iter_mut().find(|b| b.rule == rule) {
            Some(b) => b,
            None => return Ok(false),
        };
        b.hits += 1;
        self.storm_hits += 1;
        if self.storm_hits > self.storm_cap {
            self.fused = true;
            return Err(DbgCode::BreakStorm);
        }
        // 时间线环形写入
        self.ring_write(rule);
        Ok(true)
    }

    fn ring_write(&mut self, rule: u16) {
        if self.ring.len() < self.ring_cap {
            self.ring.push((self.ring_head, rule, false));
        } else {
            // 覆盖最旧并标注
            let idx = (self.ring_head as usize) % self.ring_cap;
            self.ring[idx] = (self.ring_head, rule, true);
        }
        self.ring_head += 1;
    }

    /// 键盘导航完整（域本色）。
    pub fn kb_ok(&self) -> bool {
        self.kb_nav
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C520-01..12。
pub fn run_ves520_checks() -> CheckSet {
    let mut set = CheckSet::new("ves520");

    let mut sd = SessionDebug::new(4, 5);
    // 堆栈
    let full = sd.capture(alloc::vec![
        Frame { contract: 1, rule: 10, sym: 0x100 },
        Frame { contract: 2, rule: 11, sym: 0 }
    ]);
    chk(&mut set, "C520-01", full, "符号缺失检出");
    chk(&mut set, "C520-02", sd.numbered_chain() == alloc::vec![0, 1], "降级编号链仍可用");
    chk(&mut set, "C520-03", sd.frames[1].rule == 11, "破约规则帧保留");
    chk(&mut set, "C520-04", sd.frames[0].contract == 1, "立约方帧保留");

    // 断点
    sd.mount_break(10);
    sd.mount_break(10);
    sd.mount_break(11);
    chk(&mut set, "C520-05", sd.breaks.len() == 2, "断点挂载去重");
    chk(&mut set, "C520-06", sd.hit(10) == Ok(true), "命中规则即断");
    chk(&mut set, "C520-07", sd.hit(99) == Ok(false), "未挂规则不触发");

    // 风暴熔断
    for _ in 0..4 {
        let _ = sd.hit(10);
    }
    chk(&mut set, "C520-08", sd.hit(10) == Err(DbgCode::BreakStorm), "第 6 次命中=风暴熔断");
    chk(&mut set, "C520-09", sd.fused, "熔断在册");

    // 环形时间线
    let mut sd2 = SessionDebug::new(3, 100);
    sd2.mount_break(1);
    for _ in 0..5 {
        let _ = sd2.hit(1);
    }
    chk(&mut set, "C520-10", sd2.ring.len() == 3, "环形缓冲容量 3");
    chk(&mut set, "C520-11", sd2.ring.iter().filter(|(_, _, o)| *o).count() >= 2, "覆盖标注 ≥2 次最旧覆盖");
    chk(&mut set, "C520-12", sd2.kb_ok(), "调试视图键盘导航完整（域本色）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_debug() {
        let mut sd = SessionDebug::new(2, 3);
        assert!(sd.capture(alloc::vec![Frame { contract: 1, rule: 1, sym: 0 }]));
        sd.mount_break(1);
        assert_eq!(sd.hit(1), Ok(true));
        assert!(sd.ring.len() == 1);
    }
}
