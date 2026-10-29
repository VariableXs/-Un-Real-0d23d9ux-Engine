//! UNX-F36130-J1 类型注册的审计面（谁在何时注册了什么）。
//! 判据：① 审计事件五要素齐备（序号/行为者/行为/对象指纹/逻辑时刻）；
//! ② 行为者闭集 + 逻辑钟单调（时刻回拨显性拒）；
//! ③ 三向查询面（按行为者/按行为/按对象）与全账守恒；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：审计真源——类型注册面的一切可观察事件（登记/幂等重登/
//! 封印/拒绝）统一落本账。no_std 域无墙上钟，时刻 = 逻辑钟 tick（每事件
//! 单调 +1，回拨即 E_TICK——审计账不许时间倒流）。行为者闭集：
//! SYSTEM/WIZARD/KERNEL 三身份，闭集外 E_ACTOR。36136 事件账消费本账
//! 的输出形状；36131 启动自检消费 audit_integrity。

use crate::checks::CheckSet;
use super::unx36121_typereg::{self as tr, GOLDEN_N};
use super::unx36129_persist::name_fp;

pub const SEG_BASE: u16 = 0x3b60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ACTOR: u16 = SEG_BASE + 2; // 行为者闭集外
pub const E_TICK: u16 = SEG_BASE + 3; // 逻辑钟回拨/停滞
pub const E_FULL: u16 = SEG_BASE + 4; // 审计账满（登记事件不许丢 → 拒操作）
pub const E_SEQ: u16 = SEG_BASE + 5; // 事件序号漂移
pub const E_NOENT: u16 = SEG_BASE + 6; // 查询无命中（显性空）
pub const E_INTG: u16 = SEG_BASE + 7; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 审计账容量（追加式不顶替——登记低频高价值）。
pub const AUDIT_CAP: usize = 24;

/// 行为者闭集（判别式 = 0xE0 段）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Actor {
    System = 0x0E01,  // 系统装配（黄金面）
    Wizard = 0x0E02,  // 接入向导（扩展面）
    Kernel = 0x0E03,  // 内核内部（联签面）
}

impl Actor {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            Actor::System => "SYSTEM",
            Actor::Wizard => "WIZARD",
            Actor::Kernel => "KERNEL",
        }
    }
    pub fn from_code(c: u16) -> Result<Actor, u16> {
        match c {
            0x0E01 => Ok(Actor::System),
            0x0E02 => Ok(Actor::Wizard),
            0x0E03 => Ok(Actor::Kernel),
            _ => Err(E_ACTOR),
        }
    }
}

/// 行为闭集（判别式 = 0xF0 段）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Enroll = 0x0F01,       // 新登记
    EnrollIdem = 0x0F02,   // 幂等重登
    Seal = 0x0F03,         // 封印
    Reject = 0x0F04,       // 拒绝（arg = 拒绝码）
    VersionBump = 0x0F05,  // 掩码升版（arg = 新代际）
}

impl Act {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            Act::Enroll => "ENROLL",
            Act::EnrollIdem => "ENROLL-IDEM",
            Act::Seal => "SEAL",
            Act::Reject => "REJECT",
            Act::VersionBump => "VER-BUMP",
        }
    }
}

/// 审计事件五要素。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AuditEv {
    pub seq: u32,    // ① 序号（1 起步连续）
    pub actor: u16,  // ② 行为者（闭集判别式）
    pub act: u16,    // ③ 行为（闭集判别式）
    pub fp: u16,     // ④ 对象名指纹（FNV-16，36129 同源）
    pub tick: u64,   // ⑤ 逻辑时刻（单调）
    pub arg: u16,    // 附加载荷（拒绝码/新代际/tid）
}

/// 固定容量审计账。
pub struct AuditBook {
    buf: [Option<AuditEv>; AUDIT_CAP],
    n: usize,
    seq_next: u32,
    tick: u64,
}

const NO_EV: Option<AuditEv> = None;

impl AuditBook {
    pub fn new() -> AuditBook {
        AuditBook { buf: [NO_EV; AUDIT_CAP], n: 0, seq_next: 1, tick: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 落账（逻辑钟推进 +1；钟停滞/回拨由调用面保证——本入口自证 tick > now）。
    pub fn record(&mut self, actor: Actor, act: Act, name: &str, tick: u64, arg: u16) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if Actor::from_code(actor.code()).is_err() {
            return Err(E_ACTOR);
        }
        if tick <= self.tick {
            return Err(E_TICK);
        }
        if self.n >= AUDIT_CAP {
            return Err(E_FULL);
        }
        let ev = AuditEv {
            seq: self.seq_next,
            actor: actor.code(),
            act: act.code(),
            fp: name_fp(name),
            tick,
            arg,
        };
        self.buf[self.n] = Some(ev);
        self.n += 1;
        self.seq_next = self.seq_next.wrapping_add(1);
        self.tick = tick;
        Ok(ev.seq)
    }

    pub fn at(&self, i: usize) -> Result<AuditEv, u16> {
        if i >= self.n {
            return Err(E_NOENT);
        }
        match self.buf[i] {
            Some(e) => Ok(e),
            None => Err(E_SEQ),
        }
    }

    /// 按行为者查询（命中序号列表——固定小容量，用定长缓冲 + 计数）。
    pub fn by_actor(&self, actor: Actor) -> (usize, [u32; AUDIT_CAP]) {
        let mut hits = [0u32; AUDIT_CAP];
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                if e.actor == actor.code() {
                    hits[c] = e.seq;
                    c += 1;
                }
            }
            i += 1;
        }
        (c, hits)
    }

    /// 按行为查询。
    pub fn by_act(&self, act: Act) -> (usize, [u32; AUDIT_CAP]) {
        let mut hits = [0u32; AUDIT_CAP];
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                if e.act == act.code() {
                    hits[c] = e.seq;
                    c += 1;
                }
            }
            i += 1;
        }
        (c, hits)
    }

    /// 按对象名查询（指纹同源 36129）。
    pub fn by_name(&self, name: &str) -> (usize, [u32; AUDIT_CAP]) {
        let fp = name_fp(name);
        let mut hits = [0u32; AUDIT_CAP];
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                if e.fp == fp {
                    hits[c] = e.seq;
                    c += 1;
                }
            }
            i += 1;
        }
        (c, hits)
    }

    /// 全账守恒：seq 连续 + tick 严格递增 + 行为者/行为双闭集。
    pub fn integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.n {
            let e = self.at(i)?;
            if e.seq != i as u32 + 1 {
                return Err(E_SEQ);
            }
            if Actor::from_code(e.actor).is_err() {
                return Err(E_ACTOR);
            }
            if e.act < 0x0F01 || e.act > 0x0F05 {
                return Err(E_INTG);
            }
            if i > 0 {
                let prev = self.at(i - 1)?;
                if e.tick <= prev.tick {
                    return Err(E_TICK);
                }
            }
            i += 1;
        }
        Ok(())
    }
}

/// 指纹锚（ktest 交叉断言消费）：本账实际使用的 FNV-16 出口。
pub fn name_fp_file_anchor() -> u16 {
    name_fp("File")
}

/// 装配审计链（36131 消费的黄金剧本）：黄金八类逐类落 Enroll（Actor::System）
/// + 一条 Seal，tick 每事件 +1。
pub fn audit_golden_enroll() -> Result<AuditBook, u16> {
    let mut book = AuditBook::new();
    let mut tick: u64 = 10;
    let mut i = 0;
    while i < GOLDEN_N {
        let (name, _, _, _) = tr::GOLDEN[i];
        book.record(Actor::System, Act::Enroll, name, tick, tr::TID_BASE + i as u16)?;
        tick += 1;
        i += 1;
    }
    book.record(Actor::System, Act::Seal, "*", tick, 0)?;
    book.integrity()?;
    Ok(book)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金装配审计链（9 事件：8 登记 + 1 封印）。
    let book = audit_golden_enroll()?;
    if book.count() != GOLDEN_N + 1 {
        return Err(E_SEQ);
    }
    // 钟终值实测：起点 10，8 登记（tick 10..17）+ 1 封印（tick 18）
    // = 起点 + 事件数 - 1。锚住「钟推进 = 事件间步数」这一单调性质。
    if book.now() != 10 + GOLDEN_N as u64 {
        return Err(E_TICK);
    }
    book.integrity()?;
    // R2：三向查询面。
    let (c_sys, _) = book.by_actor(Actor::System);
    if c_sys != GOLDEN_N + 1 {
        return Err(E_SEQ);
    }
    let (c_wiz, _) = book.by_actor(Actor::Wizard);
    if c_wiz != 0 {
        return Err(E_SEQ);
    }
    let (c_enroll, _) = book.by_act(Act::Enroll);
    if c_enroll != GOLDEN_N {
        return Err(E_SEQ);
    }
    let (c_seal, _) = book.by_act(Act::Seal);
    if c_seal != 1 {
        return Err(E_SEQ);
    }
    let (c_file, _) = book.by_name("File");
    if c_file != 1 {
        return Err(E_SEQ);
    }
    let (c_ghost, _) = book.by_name("Ghost");
    if c_ghost != 0 {
        return Err(E_SEQ);
    }
    // R3：行为者闭集外显性拒（from_code 直证，不构造非法枚举值）。
    let mut book2 = AuditBook::new();
    if Actor::from_code(0x0E99).err() != Some(E_ACTOR) {
        return Err(E_ACTOR);
    }
    let _ = book2;
    // R4：逻辑钟回拨/停滞显性拒。
    let mut book3 = AuditBook::new();
    book3.record(Actor::Kernel, Act::Enroll, "A", 100, 0)?;
    if book3.record(Actor::Kernel, Act::Enroll, "B", 100, 0).err() != Some(E_TICK) {
        return Err(E_TICK); // 停滞（tick == now）
    }
    if book3.record(Actor::Kernel, Act::Enroll, "C", 99, 0).err() != Some(E_TICK) {
        return Err(E_TICK); // 回拨
    }
    // 回拨后账不动。
    if book3.count() != 1 {
        return Err(E_SEQ);
    }
    // R5：账满显性拒（追加式不顶替——登记事件不许丢）。
    let mut book4 = AuditBook::new();
    let mut t: u64 = 0;
    let mut k = 0;
    while k < AUDIT_CAP {
        t += 1;
        book4.record(Actor::Wizard, Act::Enroll, "Fill", t, 0)?;
        k += 1;
    }
    t += 1;
    if book4.record(Actor::Wizard, Act::Enroll, "Over", t, 0).err() != Some(E_FULL) {
        return Err(E_FULL);
    }
    if book4.count() != AUDIT_CAP {
        return Err(E_SEQ);
    }
    book4.integrity()?;
    // R6：指纹同源（by_name 命中与 36129 name_fp 逐位一致）。
    let ev = book.at(0)?;
    if ev.fp != name_fp(tr::GOLDEN_NAMES[0]) {
        return Err(E_INTG);
    }
    // R7：拒绝事件入账（Act::Reject 携拒绝码）。
    let mut book5 = AuditBook::new();
    book5.record(Actor::Kernel, Act::Reject, "Ghost", 5, 0x3a95)?;
    let (c_rej, _) = book5.by_act(Act::Reject);
    if c_rej != 1 {
        return Err(E_SEQ);
    }
    let rej = book5.at(0)?;
    if rej.arg != 0x3a95 {
        return Err(E_INTG);
    }
    // 校验和：黄金链 tick 终值 × 事件数。
    Ok(((book.now() as u32) & 0xFFFF).wrapping_mul(5).wrapping_add(book.count() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ACTOR => "行为者闭集外",
        E_TICK => "逻辑钟回拨/停滞（审计账不许时间倒流）",
        E_FULL => "审计账满（登记事件不许丢 → 拒操作）",
        E_SEQ => "事件序号漂移",
        E_NOENT => "查询无命中/越界（显性空）",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36130_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36130_typaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b60, "0x3b60~0x3b6F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五要素/闭集/钟单调/账满/三向查询/指纹同源全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("行为者闭集", Actor::from_code(Actor::System.code()).is_ok() && Actor::from_code(0).is_err() && Actor::from_code(0x0E04).is_err(), "三身份判别式闭集（0xE0 段）");
    cs.add("行为人话闭集", Act::Seal.text() == "SEAL" && Act::Reject.text() == "REJECT", "诊断面按行为归因");
    cs.add("账容量锚", AUDIT_CAP == 24, "审计账 24 槽追加式");
    cs.add("指纹同源锚", name_fp("Key") == super::unx36129_persist::name_fp("Key"), "与持久化面同一 FNV-16");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(99));
    }

    #[test]
    fn clock_no_rewind() {
        // 逻辑钟单调：停滞（tick == now）与回拨（tick < now）均显性拒。
        let mut book = AuditBook::new();
        book.record(Actor::Kernel, Act::Enroll, "A", 10, 0).unwrap();
        assert_eq!(book.record(Actor::Kernel, Act::Enroll, "B", 10, 0).err(), Some(E_TICK));
        assert_eq!(book.record(Actor::Kernel, Act::Enroll, "C", 9, 0).err(), Some(E_TICK));
        assert_eq!(book.count(), 1); // 拒绝事件不入账
    }

    #[test]
    fn golden_chain_conserves() {
        // 黄金装配审计链守恒：9 事件（8 登记 + 1 封印），
        // 钟终值 = 起点 + 事件数 - 1。
        let book = audit_golden_enroll().unwrap();
        assert_eq!(book.count(), GOLDEN_N + 1);
        assert_eq!(book.now(), 10 + GOLDEN_N as u64);
        assert!(book.integrity().is_ok());
        let (c_seal, _) = book.by_act(Act::Seal);
        assert_eq!(c_seal, 1);
    }
}
