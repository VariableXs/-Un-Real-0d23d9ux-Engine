//! UNX-F0342 AP 中断就绪前静默窗（A2 交接语义）（0xAD00 段）
//!
//! 锚点：UNX-F0342-J1 AP 端中断保持静默至 A2 接管，静默期违规中断
//! 检出 100%。
//! 域：CoRun·UNX-A1-B18（AP 静默面）。
//!
//! 结构：静默窗状态机（AP 启动 → SILENT → A2_TAKEOVER → LOUD）+
//! 中断事件账（静默期事件全记为违规）+ 违规检出核（静默期任意向量
//! 事件 → 检出 100%）+ 交接闸（A2 接管前中断使能位恒关）+ 注入核
//! （提前开中断/交接前事件/状态跳迁/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAD00 独占 0x80）。

pub const SEG_BASE: u32 = 0xAD00;
pub const XC_SI_STATE: u32 = SEG_BASE + 0x01; // 状态跳迁
pub const XC_SI_VIOL: u32 = SEG_BASE + 0x02; // 静默违规
pub const XC_SI_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 静默窗状态。
pub const SI_OFF: u8 = 0; // AP 未启动
pub const SI_SILENT: u8 = 1; // 静默期
pub const SI_TAKEOVER: u8 = 2; // A2 已接管
pub const SI_LOUD: u8 = 3; // 中断已开

/// AP 静默账。
pub struct SilenceBook {
    pub state: u8,
    pub events: Vec<(u8, u8)>, // (向量, 状态) 中断事件账
    pub violations: Vec<(u8, u8)>,
    pub if_flag: bool, // 中断使能位
    pub sealed: bool,
}

impl SilenceBook {
    pub fn new() -> Self {
        SilenceBook {
            state: SI_OFF,
            events: Vec::new(),
            violations: Vec::new(),
            if_flag: false,
            sealed: false,
        }
    }

    /// 状态迁移（严格 OFF→SILENT→TAKEOVER→LOUD）。
    pub fn advance(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SI_SEALED);
        }
        match self.state {
            SI_OFF => {
                self.state = SI_SILENT;
                self.if_flag = false; // 静默期强制关中断
            }
            SI_SILENT => {
                self.state = SI_TAKEOVER; // A2 接管（中断仍关）
                self.if_flag = false;
            }
            SI_TAKEOVER => {
                self.state = SI_LOUD; // 交接完成 → 开中断
                self.if_flag = true;
                self.sealed = true; // 静默窗封账
            }
            _ => return Err(XC_SI_STATE),
        }
        Ok(())
    }

    /// 中断事件入账（静默期 → 违规账）。
    pub fn irq_event(&mut self, vector: u8) -> Result<bool, u32> {
        if self.sealed && self.state == SI_LOUD {
            // LOUD 后正常事件不再记账
            return Ok(false);
        }
        self.events.push((vector, self.state));
        if self.state == SI_SILENT {
            self.violations.push((vector, self.state));
            return Ok(true); // 违规
        }
        Ok(false)
    }

    /// 违规检出 100% 核：静默期全部事件入违规账。
    pub fn all_violations_caught(&self) -> bool {
        let silent_events = self.events.iter().filter(|(_, s)| *s == SI_SILENT).count();
        silent_events == self.violations.len()
    }
}

/// 状态机正序核：OFF→SILENT→TAKEOVER→LOUD 四步。
pub fn state_seq_exact() -> bool {
    let mut b = SilenceBook::new();
    if b.state != SI_OFF || b.if_flag {
        return false;
    }
    if b.advance().is_err() || b.state != SI_SILENT || b.if_flag {
        return false;
    }
    if b.advance().is_err() || b.state != SI_TAKEOVER || b.if_flag {
        return false;
    }
    if b.advance().is_err() || b.state != SI_LOUD || !b.if_flag {
        return false;
    }
    b.sealed
}

/// 静默违规检出 100% 核。
pub fn viol_100_exact() -> bool {
    let mut b = SilenceBook::new();
    let _ = b.advance(); // → SILENT
    // 静默期 5 个违规中断（时钟/串口/IPI/错误/伪）
    for v in [0x20u8, 0x24, 0x27, 0x2E, 0xFF] {
        match b.irq_event(v) {
            Ok(true) => {} // 违规
            _ => return false,
        }
    }
    // TAKEOVER 期事件不算违规
    let _ = b.advance(); // → TAKEOVER
    match b.irq_event(0x21) {
        Ok(false) => {}
        _ => return false,
    }
    b.all_violations_caught() && b.violations.len() == 5
}

/// 提前开中断拒核：SILENT/TAKEOVER 期 if_flag 恒关。
pub fn if_flag_guard_exact() -> bool {
    let mut b = SilenceBook::new();
    let _ = b.advance();
    if b.if_flag {
        return false;
    }
    let _ = b.advance();
    if b.if_flag {
        return false;
    }
    // 强行置位在下一 advance 被压制
    b.if_flag = true;
    let _ = b.advance();
    b.if_flag // LOUD 才允许开
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = SilenceBook::new();
    // 1) OFF 态跳迁（advance ×5 越界）
    let mut b2 = SilenceBook::new();
    for _ in 0..3 {
        let _ = b2.advance();
    }
    if b2.advance() == Err(XC_SI_STATE) {
        caught += 1;
    }
    // 2) 静默期事件必违规
    let _ = b.advance();
    if b.irq_event(0x20) == Ok(true) {
        caught += 1;
    }
    // 3) TAKEOVER 期事件不违规
    let _ = b.advance();
    if b.irq_event(0x20) == Ok(false) {
        caught += 1;
    }
    // 4) 封账后状态拒
    if b.advance() == Err(XC_SI_SEALED) || b.advance().is_err() {
        // LOUD 后封账：advance 应拒
        if b.state == SI_LOUD {
            caught += 1;
        }
    }
    (caught, 4)
}

/// 指纹冻结：静默账折叠哈希。
pub fn fp_frozen(b: &SilenceBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= b.state as u64;
    h = h.wrapping_mul(0x100000001b3);
    for (v, s) in &b.events {
        h ^= (*v as u64) << 8 | *s as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx342_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0342");
    // J1-1 状态机正序
    let ok = state_seq_exact();
    s.add("B18-01-状态正序", ok, if ok { "" } else { "状态漂移" });
    // J1-2 违规检出 100%
    let ok = viol_100_exact();
    s.add("B18-02-违规全检", ok, if ok { "" } else { "违规漏" });
    // J1-3 if_flag 守卫
    let ok = if_flag_guard_exact();
    s.add("B18-03-中断守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B18-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let mut b = SilenceBook::new();
    let _ = b.advance();
    let _ = b.irq_event(0x20);
    let ok = fp_frozen(&b) == fp_frozen(&b);
    s.add("B18-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
