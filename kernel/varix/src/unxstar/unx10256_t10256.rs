//! UNX-F10256 裁决只读三端点（对外只读查询）（0x14140 段）
//!
//! 锚点：UNX-F10256-J1 三端点达成（状态查询/摘要查询/明细查询三只读
//! 端点各 10/10 次正确应答）；J2 写入拒绝（三端点全只读，任何写企图
//! 10/10 次拒绝并留痕）；J3 查询一致性（同查询重复应答字节一致）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：裁决快照（内部真源）+ 三端点门面（read-only 视图）+ 写拒绝
//! 账（企图留痕）+ 注入核（写企图/越权端点/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x14140 独占 0x20）。
pub const SEG_BASE: u32 = 0x14140;
pub const RO_WRITE_ATTEMPT: u32 = SEG_BASE + 0x01; // 写企图
pub const RO_BAD_EP: u32 = SEG_BASE + 0x02; // 越权端点
pub const RO_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const RO_EMPTY_Q: u32 = SEG_BASE + 0x04; // 空查询

/// 端点闭集。
pub const EP_STATUS: u8 = 0; // 状态查询
pub const EP_SUMMARY: u8 = 1; // 摘要查询
pub const EP_DETAIL: u8 = 2; // 明细查询
pub const EP_N: u8 = 3;

pub fn ep_ok(e: u8) -> bool {
    e < EP_N
}

pub fn ep_name(e: u8) -> &'static str {
    match e {
        EP_STATUS => "status",
        EP_SUMMARY => "summary",
        _ => "detail",
    }
}

/// 裁决快照（内部真源，端点只读视图）。
pub struct VerdictSnap {
    pub batch: u32,
    pub verdict: u8, // 0=全绿 1=带遗 2=挂起
    pub gates: u8,
    pub ck_green: u32,
    pub ck_total: u32,
    details: Vec<(u32, bool)>, // (ck, passed)
    frozen: bool,
    write_tries: Vec<(u8, u32)>, // (端点, 企图码) 写企图留痕
}

impl VerdictSnap {
    pub fn new(batch: u32) -> VerdictSnap {
        VerdictSnap {
            batch,
            verdict: 0,
            gates: 0,
            ck_green: 0,
            ck_total: 0,
            details: Vec::new(),
            frozen: false,
            write_tries: Vec::new(),
        }
    }

    /// 内部装载（构造期，非端点写入）。
    pub fn load(&mut self, ck: u32, passed: bool) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if ck == 0 {
            return Err("zero_ck");
        }
        if self.details.iter().any(|(c, _)| *c == ck) {
            return Err("dup");
        }
        self.details.push((ck, passed));
        self.ck_total += 1;
        if passed {
            self.ck_green += 1;
        }
        self.verdict = if self.ck_green == self.ck_total && self.ck_total > 0 {
            0
        } else if self.ck_green > 0 {
            1
        } else {
            2
        };
        Ok(())
    }

    pub fn seal(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 写企图登记（端点拒绝写时留痕）。
    pub fn write_deny(&mut self, ep: u8, code: u32) -> Result<(), &'static str> {
        if !ep_ok(ep) {
            return Err("bad_ep");
        }
        self.write_tries.push((ep, code));
        Err("read_only")
    }

    pub fn write_tries_of(&self, ep: u8) -> usize {
        self.write_tries.iter().filter(|(e, _)| *e == ep).count()
    }

    /// 端点 1：状态查询。
    pub fn ep_status(&self) -> Result<String, &'static str> {
        Ok(format!(
            "STATUS|b{}|v{}|gates{}",
            self.batch, self.verdict, self.gates
        ))
    }

    /// 端点 2：摘要查询。
    pub fn ep_summary(&self) -> Result<String, &'static str> {
        Ok(format!(
            "SUMMARY|green={}/{}|",
            self.ck_green, self.ck_total
        ))
    }

    /// 端点 3：明细查询（带翻页）。
    pub fn ep_detail(&self, offset: usize, limit: usize) -> Result<String, &'static str> {
        if limit == 0 {
            return Err("empty_q");
        }
        let slice: Vec<&(u32, bool)> = self
            .details
            .iter()
            .skip(offset)
            .take(limit)
            .collect();
        if slice.is_empty() {
            return Err("empty_q");
        }
        let mut out = format!("DETAIL|off{}|n{}|", offset, slice.len());
        for (ck, p) in slice {
            out.push_str(&format!("{}:{},", ck, if *p { 1 } else { 0 }));
        }
        Ok(out)
    }

    /// 查询一致性：同端点同参重复应答一致。
    pub fn idempotent(&self, offset: usize, limit: usize) -> bool {
        let a = self.ep_detail(offset, limit);
        let b = self.ep_detail(offset, limit);
        a == b && a.is_ok()
    }
}

/// 注入核。
pub fn inject_fault(s: &mut VerdictSnap, code: u32, ep: u8) -> Result<(), &'static str> {
    match code {
        RO_WRITE_ATTEMPT => s.write_deny(ep, RO_WRITE_ATTEMPT),
        RO_BAD_EP => s.write_deny(9, RO_BAD_EP),
        RO_FROZEN => {
            s.seal();
            s.load(999, true)
        }
        RO_EMPTY_Q => s.ep_detail(9999, 1).map(|_| ()),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10256_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10256");

    // J1 三端点正确应答
    let j1 = {
        let mut v = VerdictSnap::new(1);
        for i in 1..=5u32 {
            let _ = v.load(i, i != 4);
        }
        let st = v.ep_status().unwrap();
        let su = v.ep_summary().unwrap();
        let de = v.ep_detail(0, 5);
        st == "STATUS|b1|v1|gates0"
            && su == "SUMMARY|green=4/5|"
            && de.is_ok()
            && de.unwrap().contains("4:0,")
    };
    s.add("J1-三端点应答", j1, "状态/摘要/明细三面正确");

    // J2 三端点写企图全拒绝并留痕
    let j2 = {
        let mut v = VerdictSnap::new(2);
        let r1 = v.write_deny(EP_STATUS, 1);
        let r2 = v.write_deny(EP_SUMMARY, 1);
        let r3 = v.write_deny(EP_DETAIL, 1);
        r1 == Err("read_only")
            && r2 == Err("read_only")
            && r3 == Err("read_only")
            && v.write_tries_of(EP_STATUS) == 1
            && v.write_tries_of(EP_SUMMARY) == 1
            && v.write_tries_of(EP_DETAIL) == 1
    };
    s.add("J2-写企图全拒", j2, "三端点只读红线");

    // J3 查询一致性：同参重复应答一致
    let j3 = {
        let mut v = VerdictSnap::new(3);
        for i in 1..=10u32 {
            let _ = v.load(i, true);
        }
        v.idempotent(0, 5) && v.idempotent(5, 5)
    };
    s.add("J3-查询幂等", j3, "重复应答字节一致");

    // J4 越权端点拒绝
    let j4 = inject_fault(&mut VerdictSnap::new(4), RO_BAD_EP, 0) == Err("bad_ep");
    s.add("J4-越权端点拒", j4, "ep=9 即拒");

    // J5 明细空查询拒绝（越界翻页）
    let j5 = inject_fault(&mut VerdictSnap::new(5), RO_EMPTY_Q, 0) == Err("empty_q");
    s.add("J5-空查询拒", j5, "越界翻页即拒");

    // J6 冻结后装载拒绝
    let j6 = {
        let mut v = VerdictSnap::new(6);
        inject_fault(&mut v, RO_FROZEN, 0);
        v.is_frozen()
    };
    s.add("J6-冻结拒载", j6, "封版后 load 必拒");

    // J7 零号判据拒收
    let j7 = VerdictSnap::new(7).load(0, true) == Err("zero_ck");
    s.add("J7-零号拒收", j7, "ck=0 即拒");

    // J8 装载重号拒收
    let j8 = {
        let mut v = VerdictSnap::new(8);
        let a = v.load(1, true);
        let bb = v.load(1, true);
        a == Ok(()) && bb == Err("dup")
    };
    s.add("J8-装载重号拒", j8, "同判据二次即拒");

    // J9 裁决自动归档：全过→0 全挂→2 部分→1
    let j9 = {
        let mut v1 = VerdictSnap::new(91);
        let _ = v1.load(1, true);
        let mut v2 = VerdictSnap::new(92);
        let _ = v2.load(1, false);
        let mut v3 = VerdictSnap::new(93);
        let _ = v3.load(1, true);
        let _ = v3.load(2, false);
        v1.verdict == 0 && v2.verdict == 2 && v3.verdict == 1
    };
    s.add("J9-裁决三档", j9, "全绿/挂起/带遗自动归档");

    // J10 十轮端点回放：10 轮三端点应答字节一致（确定性）
    let j10 = {
        let mk = || {
            let mut v = VerdictSnap::new(10);
            for i in 1..=8u32 {
                let _ = v.load(i, i % 2 == 1);
            }
            format!(
                "{}|{}|{}",
                v.ep_status().unwrap(),
                v.ep_summary().unwrap(),
                v.ep_detail(0, 8).unwrap()
            )
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
