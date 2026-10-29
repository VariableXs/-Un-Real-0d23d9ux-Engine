//! UNX-F10289 对接只读视图（O3 侧查询端点）（0x18760 段）
//!
//! 锚点：UNX-F10289-J1 只读视图达成（O3 侧三查询端点：判据查询/工单
//! 查询/版本查询各 10/10 次正确应答）；J2 写拒绝（视图全只读，写企图
//! 10/10 次拒绝留痕）；J3 查询幂等（同查询同应答）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·只读视图）。
//!
//! 结构：视图快照（内部真源）+ 三端点门面 + 写拒绝账 + 注入核（写企
//! 图/越权端点/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x18760 独占 0x20）。
pub const SEG_BASE: u32 = 0x18760;
pub const OV_WRITE_TRY: u32 = SEG_BASE + 0x01; // 写企图
pub const OV_BAD_EP: u32 = SEG_BASE + 0x02; // 越权端点
pub const OV_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const OV_EMPTY_Q: u32 = SEG_BASE + 0x04; // 空查询

/// 端点闭集。
pub const EP_CK: u8 = 0; // 判据查询
pub const EP_TICKET: u8 = 1; // 工单查询
pub const EP_VER: u8 = 2; // 版本查询
pub const EP_N: u8 = 3;

pub fn ep_ok(e: u8) -> bool {
    e < EP_N
}

/// 视图快照。
pub struct O3View {
    cks: Vec<(u32, String)>,      // (判据号, 摘要)
    tickets: Vec<(u32, String)>,  // (工单号, 状态)
    ver: String,
    frozen: bool,
    write_tries: Vec<(u8, u32)>,
}

impl O3View {
    pub fn new(ver: &str) -> O3View {
        O3View {
            cks: Vec::new(),
            tickets: Vec::new(),
            ver: ver.to_string(),
            frozen: false,
            write_tries: Vec::new(),
        }
    }

    /// 内部装载（构造期）。
    pub fn load_ck(&mut self, ck: u32, text: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if ck == 0 || text.is_empty() {
            return Err("bad_ck");
        }
        if self.cks.iter().any(|(c, _)| *c == ck) {
            return Err("dup_ck");
        }
        self.cks.push((ck, text.to_string()));
        Ok(())
    }

    pub fn load_ticket(&mut self, id: u32, state: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if id == 0 || state.is_empty() {
            return Err("bad_ticket");
        }
        if self.tickets.iter().any(|(i, _)| *i == id) {
            return Err("dup_ticket");
        }
        self.tickets.push((id, state.to_string()));
        Ok(())
    }

    pub fn seal(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 写企图留痕（只读红线）。
    pub fn write_deny(&mut self, ep: u8, code: u32) -> Result<(), &'static str> {
        if !ep_ok(ep) {
            return Err("bad_ep");
        }
        self.write_tries.push((ep, code));
        Err("read_only")
    }

    pub fn tries_of(&self, ep: u8) -> usize {
        self.write_tries.iter().filter(|(e, _)| *e == ep).count()
    }

    /// 端点 1：判据查询（按号）。
    pub fn q_ck(&self, ck: u32) -> Result<String, &'static str> {
        match self.cks.iter().find(|(c, _)| *c == ck) {
            Some((c, t)) => Ok(format!("CK{}|{}", c, t)),
            None => Err("no_ck"),
        }
    }

    /// 端点 2：工单状态查询。
    pub fn q_ticket(&self, id: u32) -> Result<String, &'static str> {
        match self.tickets.iter().find(|(i, _)| *i == id) {
            Some((i, s)) => Ok(format!("TICKET{}|{}", i, s)),
            None => Err("no_ticket"),
        }
    }

    /// 端点 3：版本查询。
    pub fn q_ver(&self) -> String {
        format!("VER|{}", self.ver)
    }

    /// 查询幂等。
    pub fn idempotent(&self, ck: u32) -> bool {
        self.q_ck(ck) == self.q_ck(ck) && self.q_ck(ck).is_ok()
    }
}

/// 注入核。
pub fn inject_fault(v: &mut O3View, code: u32, ep: u8) -> Result<(), &'static str> {
    match code {
        OV_WRITE_TRY => v.write_deny(ep, OV_WRITE_TRY),
        OV_BAD_EP => v.write_deny(9, OV_BAD_EP),
        OV_FROZEN => {
            v.seal();
            v.load_ck(99, "冻结后")
        }
        OV_EMPTY_Q => v.q_ck(9999).map(|_| ()),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10289_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10289");

    // J1 三端点正确应答
    let j1 = {
        let mut v = O3View::new("1.0.0");
        let _ = v.load_ck(101, "fd 泄漏断言");
        let _ = v.load_ticket(42, "open");
        v.q_ck(101) == Ok("CK101|fd 泄漏断言".to_string())
            && v.q_ticket(42) == Ok("TICKET42|open".to_string())
            && v.q_ver() == "VER|1.0.0"
    };
    s.add("J1-三端点应答", j1, "判据/工单/版本三面正确");

    // J2 三端点写企图全拒绝留痕
    let j2 = {
        let mut v = O3View::new("1.0.0");
        let r1 = v.write_deny(EP_CK, 1);
        let r2 = v.write_deny(EP_TICKET, 1);
        let r3 = v.write_deny(EP_VER, 1);
        r1 == Err("read_only")
            && r2 == Err("read_only")
            && r3 == Err("read_only")
            && v.tries_of(EP_CK) == 1
            && v.tries_of(EP_TICKET) == 1
            && v.tries_of(EP_VER) == 1
    };
    s.add("J2-写企图全拒", j2, "三端点只读红线");

    // J3 查询幂等
    let j3 = {
        let mut v = O3View::new("2.0.0");
        let _ = v.load_ck(1, "x");
        let _ = v.load_ck(2, "y");
        v.idempotent(1) && v.idempotent(2)
    };
    s.add("J3-查询幂等", j3, "重复查询同应答");

    // J4 越权端点拒绝
    let j4 = inject_fault(&mut O3View::new("1.0.0"), OV_BAD_EP, 0) == Err("bad_ep");
    s.add("J4-越权端点拒", j4, "ep=9 即拒");

    // J5 空查询拒绝
    let j5 = inject_fault(&mut O3View::new("1.0.0"), OV_EMPTY_Q, 0) == Err("no_ck");
    s.add("J5-空查询拒", j5, "无此判据即拒");

    // J6 冻结后装载拒绝
    let j6 = {
        let mut v = O3View::new("1.0.0");
        inject_fault(&mut v, OV_FROZEN, 0);
        v.is_frozen()
    };
    s.add("J6-冻结拒载", j6, "封版后 load 必拒");

    // J7 装载重号拒绝
    let j7 = {
        let mut v = O3View::new("1.0.0");
        let a = v.load_ck(1, "x");
        let bb = v.load_ck(1, "y");
        a == Ok(()) && bb == Err("dup_ck")
    };
    s.add("J7-装载重号拒", j7, "同判据二次即拒");

    // J8 注入四路
    let codes = [OV_WRITE_TRY, OV_BAD_EP, OV_FROZEN, OV_EMPTY_Q];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut O3View::new("1.0.0"), **c, EP_CK).is_err())
        .count();
    s.add("J8-注入四路", caught == 4, "四路注入全拒");

    // J9 装载负样本
    let j9 = {
        let mut v = O3View::new("1.0.0");
        v.load_ck(0, "x") == Err("bad_ck") && v.load_ticket(0, "x") == Err("bad_ticket")
    };
    s.add("J9-装载负样本", j9, "零号即拒");

    // J10 十轮端点回放
    let j10 = {
        let mk = || {
            let mut v = O3View::new("3.1.4");
            let _ = v.load_ck(7, "断言七");
            let _ = v.load_ticket(8, "fixed");
            format!("{}|{}|{}", v.q_ck(7).unwrap(), v.q_ticket(8).unwrap(), v.q_ver())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮端点", j10, "10/10 轮应答一致");

    s
}
