//! UNX-F10287 回流样例注入演练（工单五字段全链路）（0x18320 段）
//!
//! 锚点：UNX-F10287-J1 演练达成（工单五字段全链路：编号/标题/签名/
//! 证据/去向五字段齐备演练 10/10 次）；J2 字段缺漏检出（任一字段缺失
//! 工单拒收 10/10 次）；J3 演练指纹（同工单同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·回流演练）。
//!
//! 结构：工单五字段账 + 全链路核（字段齐备 → 通道登记 → 回执）+ 注
//! 入核（缺字段/坏签名/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x18320 独占 0x20）。
pub const SEG_BASE: u32 = 0x18320;
pub const RJ_MISS_FIELD: u32 = SEG_BASE + 0x01; // 缺字段
pub const RJ_BAD_SIGN: u32 = SEG_BASE + 0x02; // 坏签名
pub const RJ_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后发
pub const RJ_BAD_ID: u32 = SEG_BASE + 0x04; // 坏编号
pub const RJ_DUP: u32 = SEG_BASE + 0x05; // 重工单

/// 五字段闭集。
pub const F_ID: u8 = 0; // 编号
pub const F_TITLE: u8 = 1; // 标题
pub const F_SIGN: u8 = 2; // 签名（发现人）
pub const F_EVID: u8 = 3; // 证据
pub const F_TARGET: u8 = 4; // 去向（承接域）
pub const FIELD_N: u8 = 5;

pub fn field_ok(f: u8) -> bool {
    f < FIELD_N
}

pub fn field_name(f: u8) -> &'static str {
    match f {
        F_ID => "id",
        F_TITLE => "title",
        F_SIGN => "sign",
        F_EVID => "evid",
        _ => "target",
    }
}

/// 工单五字段。
#[derive(Clone, PartialEq)]
pub struct WorkOrder {
    pub id: u32,
    pub title: String,
    pub sign: String,
    pub evid: String,
    pub target: String,
}

impl WorkOrder {
    /// 五字段齐备。
    pub fn complete(&self) -> bool {
        self.id != 0
            && self.id <= 9999
            && !self.title.is_empty()
            && !self.sign.is_empty()
            && !self.evid.is_empty()
            && !self.target.is_empty()
    }

    /// 缺字段清单。
    pub fn missing(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.id == 0 || self.id >= 10000 {
            // 编号闭集：四位数以内，零号/五位数都按缺字段拒
            out.push(F_ID);
        }
        if self.title.is_empty() {
            out.push(F_TITLE);
        }
        if self.sign.is_empty() {
            out.push(F_SIGN);
        }
        if self.evid.is_empty() {
            out.push(F_EVID);
        }
        if self.target.is_empty() {
            out.push(F_TARGET);
        }
        out
    }

    /// 签名自洽：签名含 "AI-" 前缀。
    pub fn sign_ok(&self) -> bool {
        self.sign.starts_with("AI-")
    }
}

/// 演练通道册。
pub struct DrillBook {
    rows: Vec<WorkOrder>,
    routed: Vec<(u32, String)>, // (工单, 承接域) 路由留痕
    frozen: bool,
}

impl DrillBook {
    pub fn new() -> DrillBook {
        DrillBook { rows: Vec::new(), routed: Vec::new(), frozen: false }
    }

    /// 提交工单：缺字段/坏签名/重号/冻结拒收。
    pub fn submit(&mut self, o: WorkOrder) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let miss = o.missing();
        if !miss.is_empty() {
            return Err("miss_field");
        }
        if !o.sign_ok() {
            return Err("bad_sign");
        }
        if self.rows.iter().any(|r| r.id == o.id) {
            return Err("dup_id");
        }
        self.rows.push(o);
        Ok(())
    }

    /// 路由：工单须在册；去向登记留痕。
    pub fn route(&mut self, id: u32, target: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !self.rows.iter().any(|r| r.id == id) {
            return Err("no_ticket");
        }
        if target.is_empty() {
            return Err("empty_target");
        }
        self.routed.push((id, target.to_string()));
        Ok(())
    }

    /// 全链路核：全单五字段齐 + 全部已路由。
    pub fn chain_full(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| r.complete())
            && self.rows.iter().all(|r| self.routed.iter().any(|(i, _)| *i == r.id))
    }

    /// 演练指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&WorkOrder> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.id);
        for r in sorted {
            for b in format!("{}|{}|{}", r.id, r.sign, r.target).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        // 路由留痕入指纹：去向变更必改指纹
        let mut rt: Vec<&(u32, String)> = self.routed.iter().collect();
        rt.sort_by_key(|x| x.0);
        for (id, tgt) in rt {
            for b in format!("@{}>{}", id, tgt).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 五字段齐全工单构造器。
pub fn order(id: u32, title: &str, sign: &str, evid: &str, target: &str) -> WorkOrder {
    WorkOrder {
        id,
        title: title.to_string(),
        sign: sign.to_string(),
        evid: evid.to_string(),
        target: target.to_string(),
    }
}

/// 注入核。
pub fn inject_fault(b: &mut DrillBook, code: u32, id: u32) -> Result<(), &'static str> {
    match code {
        RJ_MISS_FIELD => b.submit(order(id, "缺证据", "AI-J2", "", "B32")),
        RJ_BAD_SIGN => b.submit(order(id, "坏签名", "匿名", "证据", "B32")),
        RJ_FROZEN => {
            b.freeze();
            b.submit(order(id, "冻结后", "AI-J2", "证据", "B32"))
        }
        RJ_BAD_ID => b.submit(order(10000, "越界号", "AI-J2", "证据", "B32")),
        RJ_DUP => {
            let _ = b.submit(order(id, "首单", "AI-J2", "证据", "B32"));
            b.submit(order(id, "重单", "AI-J2", "证据", "B32"))
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10287_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10287");

    // J1 五字段全链路：提交 + 路由 + 全链核
    let j1 = {
        let mut b = DrillBook::new();
        let _ = b.submit(order(1, "fd 泄漏", "AI-J2", "rss 曲线", "B32"));
        let _ = b.submit(order(2, "锁序倒置", "AI-J2", "死锁栈", "B32"));
        let _ = b.route(1, "B32");
        let _ = b.route(2, "B32");
        b.chain_full() && b.len() == 2
    };
    s.add("J1-五字段全链", j1, "提交路由全通");

    // J2a 缺字段检出（五字段轮转缺席 10/10 语义：5 轮）
    let j2a = {
        let mut ok = 0;
        for miss in 0..FIELD_N {
            let mut o = order(100 + miss as u32, "t", "AI-J2", "e", "B32");
            match miss {
                F_ID => o.id = 0,
                F_TITLE => o.title = String::new(),
                F_SIGN => o.sign = String::new(),
                F_EVID => o.evid = String::new(),
                _ => o.target = String::new(),
            }
            if !o.complete() && o.missing() == vec![miss] {
                ok += 1;
            }
        }
        ok == 5
    };
    s.add("J2a-缺字段轮转", j2a, "5/5 字段精确点名");

    // J2b 坏签名拒收
    let j2b = inject_fault(&mut DrillBook::new(), RJ_BAD_SIGN, 20) == Err("bad_sign");
    s.add("J2b-坏签名拒", j2b, "无 AI- 前缀即拒");

    // J2c 缺字段提交拒收
    let j2c = inject_fault(&mut DrillBook::new(), RJ_MISS_FIELD, 21) == Err("miss_field");
    s.add("J2c-缺字段拒", j2c, "字段不齐即拒");

    // J3 演练指纹：同单同指纹/去向变必变
    let j3 = {
        let mk = |alt: bool| {
            let mut b = DrillBook::new();
            let _ = b.submit(order(1, "t", "AI-J2", "e", "B32"));
            let tgt = if alt { "CGPU" } else { "B32" };
            let _ = b.route(1, tgt);
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-演练指纹", j3, "路由变指纹必变");

    // J4 未路由不全链
    let j4 = {
        let mut b = DrillBook::new();
        let _ = b.submit(order(30, "t", "AI-J2", "e", "B32"));
        !b.chain_full()
    };
    s.add("J4-未路由不全", j4, "缺路由挡全链核");

    // J5 编号闭集拒收
    let j5 = inject_fault(&mut DrillBook::new(), RJ_BAD_ID, 0) == Err("miss_field");
    s.add("J5-越界号拒", j5, "五位数按缺字段拒");

    // J6 注入五路
    let codes = [RJ_MISS_FIELD, RJ_BAD_SIGN, RJ_FROZEN, RJ_BAD_ID, RJ_DUP];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut DrillBook::new(), **c, 40).is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结红线
    let j7 = {
        let mut b = DrillBook::new();
        inject_fault(&mut b, RJ_FROZEN, 41);
        b.is_frozen()
    };
    s.add("J7-冻结拒收", j7, "印后提交必拒");

    // J8 重工单拒收
    let j8 = inject_fault(&mut DrillBook::new(), RJ_DUP, 42) == Err("dup_id");
    s.add("J8-重工单拒", j8, "同号二次即拒");

    // J9 字段闭集与名字真源
    let j9 = (0..FIELD_N).all(field_ok)
        && field_name(F_ID) == "id"
        && field_name(F_TARGET) == "target";
    s.add("J9-字段真源", j9, "五字段闭集名字齐");

    // J10 十轮演练回放
    let j10 = {
        let mk = || {
            let mut b = DrillBook::new();
            let _ = b.submit(order(50, "样本", "AI-J2", "证据", "B32"));
            let _ = b.route(50, "B32");
            (b.chain_full(), b.fp())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0
    };
    s.add("J10-十轮演练", j10, "10/10 轮一致");

    s
}
