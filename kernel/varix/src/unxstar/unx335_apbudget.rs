//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0335 · AP 数量上限与预算（Max AP 账）· 诊断段 0x3ee0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0335-J1 AP 上限（255）与资源预算账齐，超限 AP 拒绝有账。
//! 难点分析：
//!   每多唤醒一个 AP 就要多付一份硬预算：内核栈页、per-AP 结构、
//!   trampoline 表项、中断向量段。超上限的 AP 不是「慢点也要上」
//!   而是「必须拒」——但拒绝必须留账（哪个 AP、超了哪项预算），
//!   否则用户看到 8 核机器只起来 4 核会以为内核有 bug。本模块把
//!   上限（255）与分项预算做成一张账，逐 AP 扣款，不足即拒并记账。
//! 纪律约束：
//!   - 硬上限 255：超限即拒，无例外开关；
//!   - 分项预算：栈页/结构/向量三账独立扣；
//!   - 拒必有账：(AP, 拒因) 落账可查；
//!   - 账目闭合：在编数 + 拒绝数 == 申请数。
//! 成本记账：扣款 O(1)；审计 O(AP 数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3ee0;

/// 硬上限（x86 APIC ID 8 位空间的可唤醒上界，本内核取 255）。
pub const MAX_AP: u32 = 255;

/// 分项预算（每 AP）。
#[derive(Debug, Clone, Copy)]
pub struct PerApBudget {
    /// 内核栈页。
    pub stack_pages: u32,
    /// per-AP 结构字节。
    pub struct_bytes: u32,
    /// 中断向量段配额。
    pub vector_slots: u32,
}

/// 拒因闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDeny {
    OverHardLimit,
    StackExhausted,
    StructExhausted,
    VectorExhausted,
}

impl BudgetDeny {
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                BudgetDeny::OverHardLimit => 1,
                BudgetDeny::StackExhausted => 2,
                BudgetDeny::StructExhausted => 3,
                BudgetDeny::VectorExhausted => 4,
            }
    }
    pub fn detail(self) -> &'static str {
        match self {
            BudgetDeny::OverHardLimit => "超硬上限 255",
            BudgetDeny::StackExhausted => "栈页预算不足",
            BudgetDeny::StructExhausted => "per-AP 结构预算不足",
            BudgetDeny::VectorExhausted => "向量配额不足",
        }
    }
}

/// 拒绝账行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenyRow {
    pub ap_id: u32,
    pub why: BudgetDeny,
}

/// Max AP 账。
#[derive(Debug)]
pub struct ApBudgetLedger {
    /// 池总量。
    pub pool: PerApBudget,
    /// 已扣。
    pub used: PerApBudget,
    /// 在编 AP。
    pub enrolled: Vec<u32>,
    /// 拒绝账。
    pub denied: Vec<DenyRow>,
    /// 申请总数（闭合审计用）。
    pub requested: u32,
}

impl ApBudgetLedger {
    pub fn new(pool: PerApBudget) -> Self {
        ApBudgetLedger { pool, used: PerApBudget { stack_pages: 0, struct_bytes: 0, vector_slots: 0 }, enrolled: Vec::new(), denied: Vec::new(), requested: 0 }
    }

    /// 申请编入一个 AP：扣款成功即在编，不足即拒并记账。
    pub fn enroll(&mut self, ap_id: u32, cost: PerApBudget) -> Result<(), BudgetDeny> {
        self.requested += 1;
        if self.enrolled.len() as u32 >= MAX_AP {
            let d = DenyRow { ap_id, why: BudgetDeny::OverHardLimit };
            self.denied.push(d);
            return Err(BudgetDeny::OverHardLimit);
        }
        if self.used.stack_pages + cost.stack_pages > self.pool.stack_pages {
            let d = DenyRow { ap_id, why: BudgetDeny::StackExhausted };
            self.denied.push(d);
            return Err(BudgetDeny::StackExhausted);
        }
        if self.used.struct_bytes + cost.struct_bytes > self.pool.struct_bytes {
            let d = DenyRow { ap_id, why: BudgetDeny::StructExhausted };
            self.denied.push(d);
            return Err(BudgetDeny::StructExhausted);
        }
        if self.used.vector_slots + cost.vector_slots > self.pool.vector_slots {
            let d = DenyRow { ap_id, why: BudgetDeny::VectorExhausted };
            self.denied.push(d);
            return Err(BudgetDeny::VectorExhausted);
        }
        self.used.stack_pages += cost.stack_pages;
        self.used.struct_bytes += cost.struct_bytes;
        self.used.vector_slots += cost.vector_slots;
        self.enrolled.push(ap_id);
        Ok(())
    }

    /// 账目闭合：在编 + 拒绝 == 申请。
    pub fn closed(&self) -> bool {
        self.enrolled.len() as u32 + self.denied.len() as u32 == self.requested
    }

    /// 剩余预算。
    pub fn remaining(&self) -> PerApBudget {
        PerApBudget {
            stack_pages: self.pool.stack_pages - self.used.stack_pages,
            struct_bytes: self.pool.struct_bytes - self.used.struct_bytes,
            vector_slots: self.pool.vector_slots - self.used.vector_slots,
        }
    }
}

/// 标准单 AP 成本。
pub const STD_COST: PerApBudget = PerApBudget { stack_pages: 16, struct_bytes: 4096, vector_slots: 8 };

/// 黄金池：可编 4 AP。
pub fn golden_pool() -> ApBudgetLedger {
    ApBudgetLedger::new(PerApBudget { stack_pages: 64, struct_bytes: 16_384, vector_slots: 32 })
}

/// UNX-F0335 判据（J1 上限与预算账齐，超限拒绝有账）。
pub fn run_unx335_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0335 AP 数量上限与预算");

    // J1 主线：黄金池编满 4 AP
    let mut g = golden_pool();
    for id in 1..=4u32 {
        let _ = g.enroll(id, STD_COST);
    }
    cs.add("J1 四 AP 全编", g.enrolled == vec![1, 2, 3, 4], "预算内");
    cs.add("J1 账目闭合", g.closed(), "4+0==4");
    cs.add("J1 预算扣净", g.remaining().stack_pages == 0 && g.remaining().vector_slots == 0, "64/32 扣完");

    // 注入 1：栈页预算不足 → 拒且有账
    let mut r1 = golden_pool();
    let _ = r1.enroll(1, STD_COST);
    let fat = PerApBudget { stack_pages: 100, struct_bytes: 4096, vector_slots: 8 };
    let e1 = r1.enroll(2, fat);
    cs.add("注入1 栈不足拒收", e1 == Err(BudgetDeny::StackExhausted), "100>48");
    cs.add("注入1 拒绝有账", r1.denied == vec![DenyRow { ap_id: 2, why: BudgetDeny::StackExhausted }], "一行账");
    cs.add("注入1 账目闭合", r1.closed(), "1+1==2");
    cs.add("注入1 拒绝不扣款", r1.used.stack_pages == 16, "半途不扣");

    // 注入 2：向量配额不足（顺序在结构之后校验——顺序保真）
    let mut r2 = golden_pool();
    let _ = r2.enroll(1, PerApBudget { stack_pages: 16, struct_bytes: 8192, vector_slots: 30 });
    let e2 = r2.enroll(2, PerApBudget { stack_pages: 16, struct_bytes: 8192, vector_slots: 8 });
    cs.add("注入2 向量不足拒收", e2 == Err(BudgetDeny::VectorExhausted), "30+8>32");
    cs.add("注入2 拒因顺序正确", r2.denied[0].why == BudgetDeny::VectorExhausted, "前两项够");

    // 注入 3：硬上限 255（迷你池模拟——直接测 MAX_AP 语义）
    let mut r3 = ApBudgetLedger::new(PerApBudget { stack_pages: u32::MAX, struct_bytes: u32::MAX, vector_slots: u32::MAX });
    for id in 1..=255u32 {
        let _ = r3.enroll(id, PerApBudget { stack_pages: 0, struct_bytes: 0, vector_slots: 0 });
    }
    let e3 = r3.enroll(256, PerApBudget { stack_pages: 0, struct_bytes: 0, vector_slots: 0 });
    cs.add("注入3 上限 255 拒第 256", e3 == Err(BudgetDeny::OverHardLimit), "硬门槛");
    cs.add("注入3 拒因=硬上限", r3.denied[0].why == BudgetDeny::OverHardLimit, "非预算");
    cs.add("注入3 闭合 255+1", r3.closed(), "编255拒1申256");

    // 注入 4：混合拒绝账序保真
    let mut r4 = golden_pool();
    let _ = r4.enroll(1, STD_COST);
    let _ = r4.enroll(2, fat);
    let _ = r4.enroll(3, PerApBudget { stack_pages: 16, struct_bytes: 4096, vector_slots: 8 });
    let _ = r4.enroll(4, fat);
    cs.add("注入4 拒账序保真", r4.denied.iter().map(|d| d.ap_id).collect::<Vec<_>>() == vec![2, 4], "申请序");
    cs.add("注入4 在编不受拒染", r4.enrolled == vec![1, 3], "2/4 拒");

    // 拒因码闭集
    let codes = [BudgetDeny::OverHardLimit.code(), BudgetDeny::StackExhausted.code(), BudgetDeny::StructExhausted.code(), BudgetDeny::VectorExhausted.code()];
    cs.add("拒因码闭集互异", codes.windows(2).all(|w| w[0] != w[1]), "四码");

    cs
}
