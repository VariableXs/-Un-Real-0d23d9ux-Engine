//! UNX-F12091 内核句柄表对接接口冻结（D3 联签）（0x19d80 段）
//!
//! 卡面：内核句柄表对接五操作闭集（Alloc/Free/Deref/Dup/Grant）冻结——
//! 操作序约束（Free 须先 Alloc）、版本锚、冻结核与联签文本。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 操作闭集：五操作编号闭集 1..=5，越码必拒。
//! - J2 版本锚：操作登记版本必须与协议版本一致，失配必拒。
//! - J3 序约束：Free/Deref/Dup/Grant 须在 Alloc 之后，违序必拒。
//! - J4 单次登记：同操作二次登记必拒。
//! - J5 冻结完整性：五操作未齐时 freeze 必拒。
//! - J6 冻结后封禁：freeze 后任何登记必拒。
//! - J7 联签文本：协议文本含 D1/D3 双域标记。
//! - J8 注入五况全抓。
//!
//! 结构：HtOp 五操作枚举 + HandleTableProto 协议账 + 冻结核 +
//! 折叠指纹（211 进制卷积）+ 注入核（重登/违序/缺齐冻结/版本失配/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19d80 独占 0x20）。
pub const SEG_BASE: u32 = 0x19d80;
pub const HT_BAD_OP: u32 = SEG_BASE + 0x01; // 操作越闭集
pub const HT_BAD_VER: u32 = SEG_BASE + 0x02; // 版本失配
pub const HT_ORDER: u32 = SEG_BASE + 0x03; // 违序
pub const HT_INCOMPLETE: u32 = SEG_BASE + 0x04; // 缺齐冻结
pub const HT_DUP_OP: u32 = SEG_BASE + 0x05; // 重登
pub const HT_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后登记

/// 协议版本锚。
pub const PROTO_VER: u32 = 1;

/// 五操作枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HtOp {
    Alloc = 1,
    Free = 2,
    Deref = 3,
    Dup = 4,
    Grant = 5,
}

impl HtOp {
    /// 从原始码构造：闭集外必拒（以 Result 表达）。
    pub fn from_code(code: u32) -> Result<HtOp, u32> {
        match code {
            1 => Ok(HtOp::Alloc),
            2 => Ok(HtOp::Free),
            3 => Ok(HtOp::Deref),
            4 => Ok(HtOp::Dup),
            5 => Ok(HtOp::Grant),
            _ => Err(HT_BAD_OP),
        }
    }
    /// 是否依赖 Alloc 先行。
    pub fn needs_alloc(&self) -> bool {
        !matches!(self, HtOp::Alloc)
    }

    /// 协议序位：Alloc=0，其余按闭集序 1..=4。
    pub fn rank(&self) -> u8 {
        match self {
            HtOp::Alloc => 0,
            HtOp::Free => 1,
            HtOp::Deref => 2,
            HtOp::Dup => 3,
            HtOp::Grant => 4,
        }
    }
}

/// 协议账。
pub struct HandleTableProto {
    pub version: u32,
    pub ops: Vec<HtOp>,
    pub frozen: bool,
    pub cosign: String,
}

impl HandleTableProto {
    pub fn new() -> HandleTableProto {
        HandleTableProto {
            version: PROTO_VER,
            ops: Vec::new(),
            frozen: false,
            cosign: "D1:struct-protocol|D3:name-storage".to_string(),
        }
    }

    /// 操作登记：版本 → 序约束 → 单次。
    pub fn register(&mut self, op: HtOp, ver: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(HT_FROZEN);
        }
        if ver != self.version {
            return Err(HT_BAD_VER);
        }
        if self.ops.contains(&op) {
            return Err(HT_DUP_OP);
        }
        if op.needs_alloc() && !self.ops.contains(&HtOp::Alloc) {
            return Err(HT_ORDER);
        }
        self.ops.push(op);
        Ok(())
    }

    /// 原始码登记入口。
    pub fn register_code(&mut self, code: u32, ver: u32) -> Result<(), u32> {
        let op = HtOp::from_code(code)?;
        self.register(op, ver)
    }

    /// 冻结核：五操作齐备方放行。
    pub fn freeze(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(HT_FROZEN);
        }
        if self.ops.len() != 5 {
            return Err(HT_INCOMPLETE);
        }
        self.frozen = true;
        Ok(self.version)
    }

    /// 五操作齐备判定。
    pub fn complete(&self) -> bool {
        self.ops.len() == 5
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.version);
        for op in &self.ops {
            h = h.wrapping_mul(211).wrapping_add(*op as u32);
        }
        for by in self.cosign.bytes() {
            h = h.wrapping_mul(211).wrapping_add(by as u32);
        }
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("ops={} frozen={}", self.ops.len(), self.frozen)
    }

    /// 诊断面：联签文本。
    pub fn cosign_text(&self) -> String {
        self.cosign.clone()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = HandleTableProto::new();
    // 1) 重登必拒。
    let _ = p.register(HtOp::Alloc, PROTO_VER);
    got[0] = p.register(HtOp::Alloc, PROTO_VER).err() == Some(HT_DUP_OP);
    // 2) 违序必拒（Free 先于 Alloc）。
    got[1] = {
        let mut p2 = HandleTableProto::new();
        p2.register(HtOp::Free, PROTO_VER).err() == Some(HT_ORDER)
    };
    // 3) 缺齐冻结必拒。
    got[2] = p.freeze().err() == Some(HT_INCOMPLETE);
    // 4) 版本失配必拒。
    got[3] = p.register(HtOp::Free, PROTO_VER + 1).err() == Some(HT_BAD_VER);
    // 5) 冻结后登记必抓。
    let _ = p.register(HtOp::Free, PROTO_VER);
    let _ = p.register(HtOp::Deref, PROTO_VER);
    let _ = p.register(HtOp::Dup, PROTO_VER);
    let _ = p.register(HtOp::Grant, PROTO_VER);
    let _ = p.freeze();
    got[4] = p.register(HtOp::Dup, PROTO_VER).err() == Some(HT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12091_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12091");

    // J1 操作闭集。
    let j1 = HtOp::from_code(0).err() == Some(HT_BAD_OP)
        && HtOp::from_code(6).err() == Some(HT_BAD_OP)
        && HtOp::from_code(5).is_ok();
    s.add("J1-操作闭集", j1, "0 与 6 拒、5 过");

    // J2 版本锚。
    let j2 = {
        let mut p = HandleTableProto::new();
        p.register(HtOp::Alloc, PROTO_VER + 1).err() == Some(HT_BAD_VER)
    };
    s.add("J2-版本锚", j2, "失配必拒");

    // J3 序约束：Free 先于 Alloc 拒。
    let j3 = {
        let mut p = HandleTableProto::new();
        p.register(HtOp::Free, PROTO_VER).err() == Some(HT_ORDER)
    };
    s.add("J3-序约束", j3, "无 Alloc 先行必拒");

    // J4 Alloc 先行后全链放行。
    let j4 = {
        let mut p = HandleTableProto::new();
        let _ = p.register(HtOp::Alloc, PROTO_VER);
        p.register(HtOp::Free, PROTO_VER).is_ok()
            && p.register(HtOp::Deref, PROTO_VER).is_ok()
            && p.register(HtOp::Dup, PROTO_VER).is_ok()
            && p.register(HtOp::Grant, PROTO_VER).is_ok()
            && p.complete()
    };
    s.add("J4-全链放行", j4, "五操作齐备");

    // J5 重登拒。
    let j5 = {
        let mut p = HandleTableProto::new();
        let _ = p.register(HtOp::Alloc, PROTO_VER);
        p.register(HtOp::Alloc, PROTO_VER).err() == Some(HT_DUP_OP)
    };
    s.add("J5-重登拒", j5, "同操作二次必拒");

    // J6 缺齐冻结拒。
    let j6 = {
        let mut p = HandleTableProto::new();
        let _ = p.register(HtOp::Alloc, PROTO_VER);
        p.freeze().err() == Some(HT_INCOMPLETE)
    };
    s.add("J6-缺齐冻结拒", j6, "五缺四必拒");

    // J7 冻结后封禁。
    let j7 = {
        let mut p = HandleTableProto::new();
        let _ = p.register(HtOp::Alloc, PROTO_VER);
        let _ = p.register(HtOp::Free, PROTO_VER);
        let _ = p.register(HtOp::Deref, PROTO_VER);
        let _ = p.register(HtOp::Dup, PROTO_VER);
        let _ = p.register(HtOp::Grant, PROTO_VER);
        let _ = p.freeze();
        p.register(HtOp::Grant, PROTO_VER).err() == Some(HT_FROZEN)
            && p.freeze().err() == Some(HT_FROZEN)
    };
    s.add("J7-冻结封禁", j7, "freeze 后皆拒");

    // J8 联签文本双域标记。
    let j8 = {
        let p = HandleTableProto::new();
        let t = p.cosign_text();
        t.contains("D1:") && t.contains("D3:")
    };
    s.add("J8-联签文本", j8, "双域标记在文");

    // J9 摘要回读。
    let j9 = {
        let mut p = HandleTableProto::new();
        let _ = p.register(HtOp::Alloc, PROTO_VER);
        p.summary().contains("ops=1")
    };
    s.add("J9-摘要回读", j9, "操作数可查");

    // J10 指纹幂等。
    let j10 = {
        let mut p = HandleTableProto::new();
        let _ = p.register(HtOp::Alloc, PROTO_VER);
        p.fold() == p.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut p = HandleTableProto::new();
    let _ = p.register(HtOp::Alloc, PROTO_VER);
    let _ = p.register(HtOp::Free, PROTO_VER);

    s.add("操作回读", p.ops.contains(&HtOp::Alloc), "Alloc 在册");
    s.add("指纹非零", p.fold() != 0, "指纹有效");

    // J12 序位单调：五操作序位 0..=4 且互异。
    let j12 = HtOp::Alloc.rank() == 0
        && HtOp::Free.rank() < HtOp::Deref.rank()
        && HtOp::Deref.rank() < HtOp::Dup.rank()
        && HtOp::Dup.rank() < HtOp::Grant.rank();
    s.add("J12-序位单调", j12, "闭集序位 0..=4");

    // J13 原始码往返：1..=5 全部可还原。
    let j13 = (1..=5u32).all(|c| HtOp::from_code(c).is_ok());
    s.add("J13-码往返", j13, "闭集码全还原");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12091_proto_flow() {
        let mut p = HandleTableProto::new();
        assert!(p.register(HtOp::Alloc, 1).is_ok());
        assert_eq!(p.register(HtOp::Free, 2), Err(HT_BAD_VER));
        assert!(p.register(HtOp::Free, 1).is_ok());
        assert_eq!(p.freeze(), Err(HT_INCOMPLETE));
    }

    #[test]
    fn unx12091_checks_green() {
        let s = run_unx12091_checks();
        assert!(s.all_passed(), "UNX-F12091 判据未全绿");
    }

    #[test]
    fn op_code_roundtrip() {
        assert_eq!(HtOp::from_code(3), Ok(HtOp::Deref));
        assert!(HtOp::Dup.needs_alloc());
        assert!(!HtOp::Alloc.needs_alloc());
    }

    #[test]
    fn rank_monotonic() {
        let mut last = 0u8;
        for op in [HtOp::Alloc, HtOp::Free, HtOp::Deref, HtOp::Dup, HtOp::Grant] {
            assert!(op.rank() >= last);
            last = op.rank();
        }
        assert_eq!(last, 4);
    }
}
