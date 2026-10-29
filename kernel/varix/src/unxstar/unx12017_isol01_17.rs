//! UNX-F12017 双人格进程隔离判据与人格路由消费（0x19440 段）
//!
//! 判据原文：进程人格二值闭集（WinNt 原生 / Win32 子系统）；人格路由
//! 表按人格绑定消费端——跨人格调用必拒；未路由人格调用拒；一人格单
//! 绑定（重复绑定即冲突）；路由指纹随绑定账守恒。
//!
//! 结构：人格路由册（Persona 闭集 × 绑定槽）+ 绑定核 + 调用核
//! （四码守卫）+ 调用账（IsRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12017_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19440 独占 0x20）。
pub const SEG_BASE: u32 = 0x19440;
pub const IS_CROSS_CALL: u32 = SEG_BASE + 0x01; // 跨人格调用
pub const IS_UNROUTED: u32 = SEG_BASE + 0x02; // 未路由人格
pub const IS_DOUBLE_BIND: u32 = SEG_BASE + 0x03; // 重复绑定
pub const IS_BAD_PERSONA: u32 = SEG_BASE + 0x04; // 未知人格值
pub const IS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 人格闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Persona {
    WinNt = 1,
    Win32 = 2,
}

/// 人格值域校验（u16 → 闭集）。
pub fn persona_of(v: u16) -> Option<Persona> {
    match v {
        1 => Some(Persona::WinNt),
        2 => Some(Persona::Win32),
        _ => None,
    }
}

/// 一次调用的账目。
#[derive(Clone)]
pub struct IsRec {
    pub seq: u32,
    pub caller: Persona,
    pub target: Persona,
    /// 是否放行。
    pub granted: bool,
    pub note: String,
}

impl IsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 人格路由册模型。
pub struct IsolRouter {
    /// 绑定槽：人格 → 消费端标识。
    pub binds: Vec<(Persona, u32)>,
    pub ledger: Vec<IsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl IsolRouter {
    pub fn new() -> IsolRouter {
        IsolRouter { binds: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 绑定核：一人格单绑定。
    pub fn bind(&mut self, p: Persona, consumer: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        if note.is_empty() {
            return Err(IS_BAD_PERSONA);
        }
        if self.binds.iter().any(|b| b.0 == p) {
            return Err(IS_DOUBLE_BIND);
        }
        self.binds.push((p, consumer));
        Ok(())
    }

    fn routed(&self, p: Persona) -> bool {
        self.binds.iter().any(|b| b.0 == p)
    }

    /// 调用核：人格路由消费——同人格放行、跨人格拒、未路由拒。
    pub fn call(&mut self, caller: Persona, target: Persona, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        if note.is_empty() {
            return Err(IS_BAD_PERSONA);
        }
        if !self.routed(target) {
            self.ledger.push(IsRec {
                seq: self.seq_next,
                caller,
                target,
                granted: false,
                note: note.to_string(),
            });
            self.seq_next += 1;
            return Err(IS_UNROUTED);
        }
        if caller != target {
            self.ledger.push(IsRec {
                seq: self.seq_next,
                caller,
                target,
                granted: false,
                note: note.to_string(),
            });
            self.seq_next += 1;
            return Err(IS_CROSS_CALL);
        }
        self.ledger.push(IsRec {
            seq: self.seq_next,
            caller,
            target,
            granted: true,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 账面对平：账条数与 seq 推进一致。
    pub fn calls_tally(&self) -> bool {
        self.ledger.len() as u32 == self.seq_next - 1
    }

    /// 放行/拒收清点。
    pub fn granted_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.granted).count()
    }
    pub fn denied_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.granted).count()
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for b in &self.binds {
            h = h.wrapping_mul(211).wrapping_add(b.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(b.1);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut r = IsolRouter::new();
    let _ = r.bind(Persona::WinNt, 0x11, "nt");
    let _ = r.bind(Persona::Win32, 0x22, "w32");
    // 1) 跨人格调用 → IS_CROSS_CALL。
    got[0] = r.call(Persona::WinNt, Persona::Win32, "cross").err() == Some(IS_CROSS_CALL);
    // 2) 重复绑定 → IS_DOUBLE_BIND。
    got[1] = r.bind(Persona::WinNt, 0x33, "dup").err() == Some(IS_DOUBLE_BIND);
    // 3) 未知人格值 → IS_BAD_PERSONA（值域闭集外）。
    got[2] = persona_of(3).is_none() && persona_of(0).is_none() && persona_of(1).is_some();
    // 4) 未路由人格调用：新册只绑 WinNt，Win32 目标 → IS_UNROUTED。
    let mut q = IsolRouter::new();
    let _ = q.bind(Persona::WinNt, 0x44, "only");
    got[3] = q.call(Persona::WinNt, Persona::Win32, "nr").err() == Some(IS_UNROUTED);
    // 5) 印账后调用 → IS_FROZEN。
    r.seal();
    got[4] = r.call(Persona::WinNt, Persona::WinNt, "post").err() == Some(IS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12017_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12017");

    // J1 正路径：双人格双绑定，同人格调用放行。
    let j1 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 0x11, "nt");
        let _ = r.bind(Persona::Win32, 0x22, "w32");
        let a = r.call(Persona::WinNt, Persona::WinNt, "c1").is_ok();
        let b = r.call(Persona::Win32, Persona::Win32, "c2").is_ok();
        a && b && r.granted_count() == 2
    };
    s.add("J1-同人格放行", j1, "双调用全通");

    // J2 跨人格调用拒。
    let j2 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 1, "a");
        let _ = r.bind(Persona::Win32, 2, "b");
        r.call(Persona::Win32, Persona::WinNt, "x").err() == Some(IS_CROSS_CALL)
    };
    s.add("J2-跨人格拒", j2, "隔离边界");

    // J3 未路由人格调用拒。
    let j3 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 1, "a");
        r.call(Persona::Win32, Persona::Win32, "y").err() == Some(IS_UNROUTED)
    };
    s.add("J3-未路由拒", j3, "无绑定目标必拒");

    // J4 人格值域闭集：1/2 收，0/3/9999 拒。
    let j4 = persona_of(1).is_some()
        && persona_of(2).is_some()
        && persona_of(0).is_none()
        && persona_of(3).is_none()
        && persona_of(9999).is_none();
    s.add("J4-人格值域", j4, "闭集外全拒");

    // J5 单绑定纪律：重复绑定拒且原绑定不漂。
    let j5 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 0xAA, "a");
        let _ = r.bind(Persona::WinNt, 0xBB, "b");
        r.binds.len() == 1 && r.binds[0].1 == 0xAA
    };
    s.add("J5-单绑定", j5, "首绑定终身");

    // J6 调用账：拒收也入账（granted=false）。
    let j6 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 1, "a");
        let _ = r.call(Persona::WinNt, Persona::Win32, "denied");
        r.ledger.len() == 1 && !r.ledger[0].granted && r.denied_count() == 1
    };
    s.add("J6-拒收留痕", j6, "拒收也记账");

    // J7 账面对平：granted+denied == 总账。
    let j7 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 1, "a");
        let _ = r.bind(Persona::Win32, 2, "b");
        let _ = r.call(Persona::WinNt, Persona::WinNt, "g");
        let _ = r.call(Persona::WinNt, Persona::Win32, "d");
        r.granted_count() + r.denied_count() == r.ledger.len() && r.calls_tally()
    };
    s.add("J7-账面对平", j7, "1 放 1 拒对平");

    // J8 空留痕拒。
    let j8 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 1, "a");
        r.call(Persona::WinNt, Persona::WinNt, "").err() == Some(IS_BAD_PERSONA)
    };
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = IsolRouter::new();
        let _ = a.bind(Persona::WinNt, 1, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = IsolRouter::new();
        let _ = b.bind(Persona::Win32, 1, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异表异纹");

    // J10 印账冻结：绑定与调用双面全拒。
    let j10 = {
        let mut r = IsolRouter::new();
        let _ = r.bind(Persona::WinNt, 1, "a");
        r.seal();
        r.bind(Persona::Win32, 2, "post").err() == Some(IS_FROZEN)
            && r.call(Persona::WinNt, Persona::WinNt, "post").err() == Some(IS_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：双人格隔离面。
    let mut r = IsolRouter::new();
    let _ = r.bind(Persona::WinNt, 0x1111, "smss");
    let _ = r.bind(Persona::Win32, 0x2222, "csrss");
    let _ = r.call(Persona::Win32, Persona::Win32, "sum");
    let _ = r.call(Persona::WinNt, Persona::Win32, "sum-x");
    s.add("样例一放一拒", r.granted_count() == 1 && r.denied_count() == 1, "隔离面生效");
    s.add("样例账面合法", r.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12017_route_flow() {
        let mut r = IsolRouter::new();
        assert!(r.bind(Persona::WinNt, 1, "a").is_ok());
        assert_eq!(r.bind(Persona::WinNt, 2, "b"), Err(IS_DOUBLE_BIND));
        assert!(r.bind(Persona::Win32, 3, "c").is_ok());
        assert!(r.call(Persona::Win32, Persona::Win32, "ok").is_ok());
        assert_eq!(r.call(Persona::Win32, Persona::WinNt, "x"), Err(IS_CROSS_CALL));
    }

    #[test]
    fn unx12017_checks_green() {
        let s = run_unx12017_checks();
        assert!(s.all_passed(), "UNX-F12017 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12017_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
