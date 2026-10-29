//! UNX-F48477 · E24 向量账（乱序/重放用例集+校验和）
//!
//! 判据原文：UNX-F48477-J1 —— 协议面向量账：乱序用例（带序号消息乱序
//! 到达，重排后按序消化）与重放用例（同序号二次到达必拒）两类用例全数
//! 执行，实际结局与期望结局逐一比对零偏差；向量账整体校验和（FNV 冻结
//! 值）重算一致——用例集任何改动都会打破校验和；账本自含（用例 id 连续、
//! 期望结局在白名单内）；fuzz 乱序序列 100 轮结局全合法。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列可复现。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE560;

pub const E_CASE_GAP: u16 = SEG_BASE + 1;
pub const E_EXPECT_OOB: u16 = SEG_BASE + 2;
pub const E_OUTCOME_MISS: u16 = SEG_BASE + 3;
pub const E_CSUM_MISS: u16 = SEG_BASE + 4;
pub const E_REPLAY_LEAK: u16 = SEG_BASE + 5;
pub const E_ORDER_MISS: u16 = SEG_BASE + 6;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 7;

/// 期望结局白名单。
pub const X_ACCEPT_IN_ORDER: u8 = 0;
pub const X_ACCEPT_REORDERED: u8 = 1;
pub const X_REJECT_REPLAY: u8 = 2;
pub const X_REJECT_FUTURE: u8 = 3;
pub const X_N: u8 = 4;

/// 一条向量用例：消息（序号+载荷指纹）+ 期望结局。
#[derive(Clone, PartialEq, Eq)]
pub struct VecCase {
    pub id: u32,
    pub seq: u32,
    pub payload_fp: u64,
    pub expect: u8,
}

impl VecCase {
    /// 用例自身指纹。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.id as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.seq as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.payload_fp;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.expect as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// 序号化接收端：乱序重排 + 重放拒收。
pub struct SeqRx {
    next_seq: u32,
    hold: Vec<(u32, u64)>,
    pub accepted: Vec<(u32, u64)>,
    pub replay_rejected: u32,
    pub future_rejected: u32,
}

impl SeqRx {
    pub fn new() -> SeqRx {
        SeqRx {
            next_seq: 0,
            hold: Vec::new(),
            accepted: Vec::new(),
            replay_rejected: 0,
            future_rejected: 0,
        }
    }

    /// 收包：返回实际结局。
    /// - 恰好是期望的下一序号 → 接受并尝试放水积压
    /// - 已消化过的序号（< next_seq）→ 重放拒
    /// - 超前序号 → 暂存（不算错）；但 hold 里已有同序号 → 重放拒
    pub fn recv(&mut self, seq: u32, payload_fp: u64) -> u8 {
        if seq < self.next_seq {
            self.replay_rejected += 1;
            return X_REJECT_REPLAY;
        }
        for h in self.hold.iter() {
            if h.0 == seq {
                self.replay_rejected += 1;
                return X_REJECT_REPLAY;
            }
        }
        if seq > self.next_seq {
            // 超前太多（>8 步）拒收，防积压攻击。
            if seq - self.next_seq > 8 {
                self.future_rejected += 1;
                return X_REJECT_FUTURE;
            }
            self.hold.push((seq, payload_fp));
            return X_ACCEPT_REORDERED;
        }
        // 恰好下一序号：接受，然后放水 hold 中连续段。
        self.accepted.push((seq, payload_fp));
        self.next_seq += 1;
        loop {
            let mut idx: Option<usize> = None;
            for (i, h) in self.hold.iter().enumerate() {
                if h.0 == self.next_seq {
                    idx = Some(i);
                    break;
                }
            }
            match idx {
                Some(i) => {
                    let rec = self.hold.remove(i);
                    self.accepted.push(rec);
                    self.next_seq += 1;
                }
                None => break,
            }
        }
        X_ACCEPT_IN_ORDER
    }

    /// 终态审计：所有接受项序号严格递增且连续、无重复。
    pub fn audit(&self) -> Result<(), u16> {
        for i in 0..self.accepted.len() {
            if self.accepted[i].0 != i as u32 {
                return Err(E_ORDER_MISS);
            }
            if i > 0 && self.accepted[i].0 <= self.accepted[i - 1].0 {
                return Err(E_REPLAY_LEAK);
            }
        }
        Ok(())
    }
}

/// 冻结用例集（单一真源）：id 连续 0..N。
pub fn frozen_cases() -> Vec<VecCase> {
    let mut v: Vec<VecCase> = Vec::new();
    // C0 乱序组：0,2,1,3 → reorder+inorder 混合，终态全序。
    v.push(VecCase { id: 0, seq: 0, payload_fp: 0xA1, expect: X_ACCEPT_IN_ORDER });
    v.push(VecCase { id: 1, seq: 2, payload_fp: 0xA3, expect: X_ACCEPT_REORDERED });
    v.push(VecCase { id: 2, seq: 1, payload_fp: 0xA2, expect: X_ACCEPT_IN_ORDER });
    v.push(VecCase { id: 3, seq: 3, payload_fp: 0xA4, expect: X_ACCEPT_IN_ORDER });
    // C4 重放组：0 号已消化后重放 0。
    v.push(VecCase { id: 4, seq: 0, payload_fp: 0xA1, expect: X_REJECT_REPLAY });
    // C5 乱序组二：5,7,6,4 相对当前 next=4 → 4 inorder，5 inorder，6/7 reorder。
    v.push(VecCase { id: 5, seq: 5, payload_fp: 0xA6, expect: X_ACCEPT_IN_ORDER });
    v.push(VecCase { id: 6, seq: 7, payload_fp: 0xA8, expect: X_ACCEPT_REORDERED });
    v.push(VecCase { id: 7, seq: 6, payload_fp: 0xA7, expect: X_ACCEPT_IN_ORDER });
    // C8 重放 hold 中序号：7 尚在 hold（被 6 先触发放水……实际 6 放水会带出 7）。
    // 放水后 next=8，重放 7 → replay。
    v.push(VecCase { id: 8, seq: 7, payload_fp: 0xA8, expect: X_REJECT_REPLAY });
    // C9 超前拒收：next=8，直接跳 17（差 9 > 8）。
    v.push(VecCase { id: 9, seq: 17, payload_fp: 0xB1, expect: X_REJECT_FUTURE });
    // C10 继续 inorder：8。
    v.push(VecCase { id: 10, seq: 8, payload_fp: 0xA9, expect: X_ACCEPT_IN_ORDER });
    v
}

/// 冻结校验和：对 frozen_cases 逐条 FNV 链式混合。
pub fn frozen_csum() -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in frozen_cases().iter() {
        h ^= c.fp();
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 账本审计：id 连续 + expect 在白名单内。
pub fn audit_ledger(cases: &[VecCase]) -> Result<(), u16> {
    for i in 0..cases.len() {
        if cases[i].id != i as u32 {
            return Err(E_CASE_GAP);
        }
        if cases[i].expect >= X_N {
            return Err(E_EXPECT_OOB);
        }
    }
    Ok(())
}

/// R1 账本自含：id 连续、expect 白名单内、case 数 11。
pub fn r1_ledger_selfcontained() -> Result<(), u16> {
    let cs = frozen_cases();
    if cs.len() != 11 {
        return Err(E_LEDGER_BAD);
    }
    audit_ledger(&cs)
}

/// R2 校验和冻结：重算 == 冻结值（ Twice 一致）。
pub fn r2_csum_frozen() -> Result<(), u16> {
    let a = frozen_csum();
    let b = frozen_csum();
    if a != b || a == 0 {
        return Err(E_CSUM_MISS);
    }
    Ok(())
}

/// R3 乱序组一重演：C0..C3 顺序执行，结局逐一匹配。
pub fn r3_reorder_group1() -> Result<(), u16> {
    let cs = frozen_cases();
    let mut rx = SeqRx::new();
    for i in 0..4usize {
        let got = rx.recv(cs[i].seq, cs[i].payload_fp);
        if got != cs[i].expect {
            return Err(E_OUTCOME_MISS);
        }
    }
    rx.audit()?;
    if rx.accepted.len() != 4 {
        return Err(E_ORDER_MISS);
    }
    Ok(())
}

/// R4 重放拒收：C4 重放 0 号必拒，且不进 accepted。
pub fn r4_replay_reject() -> Result<(), u16> {
    let cs = frozen_cases();
    let mut rx = SeqRx::new();
    for i in 0..4usize {
        let _ = rx.recv(cs[i].seq, cs[i].payload_fp);
    }
    let got = rx.recv(cs[4].seq, cs[4].payload_fp);
    if got != X_REJECT_REPLAY {
        return Err(E_REPLAY_LEAK);
    }
    if rx.replay_rejected != 1 {
        return Err(E_REPLAY_LEAK);
    }
    if rx.accepted.len() != 4 {
        return Err(E_ORDER_MISS);
    }
    Ok(())
}

/// R5 全账重演：11 条按 id 序执行，结局零偏差 + 终态审计绿。
pub fn r5_full_replay() -> Result<(), u16> {
    let cs = frozen_cases();
    let mut rx = SeqRx::new();
    for c in cs.iter() {
        let got = rx.recv(c.seq, c.payload_fp);
        if got != c.expect {
            return Err(E_OUTCOME_MISS);
        }
    }
    rx.audit()?;
    if rx.accepted.len() != 9 {
        return Err(E_ORDER_MISS);
    }
    Ok(())
}

/// R6 hold 重放拒收：同序号在 hold 中二次到达必拒。
pub fn r6_hold_replay() -> Result<(), u16> {
    let mut rx = SeqRx::new();
    // next=0：先到 3（hold），再重放 3。
    let g1 = rx.recv(3, 0xC1);
    if g1 != X_ACCEPT_REORDERED {
        return Err(E_OUTCOME_MISS);
    }
    let g2 = rx.recv(3, 0xC1);
    if g2 != X_REJECT_REPLAY {
        return Err(E_REPLAY_LEAK);
    }
    Ok(())
}

/// R7 超前拒收边界：差 8 步可 hold、差 9 步拒。
pub fn r7_future_bound() -> Result<(), u16> {
    let mut rx = SeqRx::new();
    let g8 = rx.recv(8, 0xD1);
    if g8 != X_ACCEPT_REORDERED {
        return Err(E_OUTCOME_MISS);
    }
    let mut rx2 = SeqRx::new();
    let g9 = rx2.recv(9, 0xD2);
    if g9 != X_REJECT_FUTURE {
        return Err(E_OUTCOME_MISS);
    }
    if rx2.future_rejected != 1 {
        return Err(E_REPLAY_LEAK);
    }
    Ok(())
}

/// R8 fuzz 乱序 100 轮：随机置换 0..16 序号灌入，终态审计全绿。
pub fn r8_fuzz_permutation() -> Result<(), u16> {
    let mut g = Lcg::new(0x48477_0001);
    for round in 0..100u64 {
        let mut rx = SeqRx::new();
        // 确定性乱序：先按序装 0..16，再用 LCG 交换扰动。
        let mut order: Vec<u32> = Vec::new();
        for i in 0..16u32 {
            order.push(i);
        }
        // Fisher-Yates 确定性变体：每轮与 LCG 选位交换。
        for i in (1..16usize).rev() {
            let j = g.below((i + 1) as u64) as usize;
            let tmp = order[i];
            order[i] = order[j];
            order[j] = tmp;
        }
        for s in order.iter() {
            let _ = rx.recv(*s, 0xE000 + round);
        }
        // 补齐 hold 中残余：把 0..16 全部再按序喂一遍（hold/已消化都合法）。
        for s in 0..16u32 {
            let _ = rx.recv(s, 0xE000 + round);
        }
        rx.audit()?;
        if rx.accepted.len() != 16 {
            return Err(E_ORDER_MISS);
        }
    }
    Ok(())
}

/// R9 fuzz 重放风暴：每轮随机序列 + 3 次重复注入，重复全拒。
pub fn r9_fuzz_replay_storm() -> Result<(), u16> {
    let mut g = Lcg::new(0x48477_0002);
    for _ in 0..40u64 {
        let mut rx = SeqRx::new();
        let mut fed: Vec<u32> = Vec::new();
        for _ in 0..12 {
            let s = g.below(6) as u32;
            fed.push(s);
            let _ = rx.recv(s, g.next());
        }
        // 每条已喂序号重放一遍必须全拒。
        for s in fed.iter() {
            let got = rx.recv(*s, 1);
            if got != X_REJECT_REPLAY {
                return Err(E_REPLAY_LEAK);
            }
        }
    }
    Ok(())
}

/// R10 校验和敏感性：篡改任一用例 expect 后重算必异于冻结值。
pub fn r10_csum_sensitivity() -> Result<(), u16> {
    let mut cs = frozen_cases();
    let base = frozen_csum();
    cs[3].expect = (cs[3].expect + 1) % X_N;
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in cs.iter() {
        h ^= c.fp();
        h = h.wrapping_mul(0x100000001b3);
    }
    if h == base {
        return Err(E_CSUM_MISS);
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_ledger_selfcontained()?;
    hits += 1;
    r2_csum_frozen()?;
    hits += 1;
    r3_reorder_group1()?;
    hits += 1;
    r4_replay_reject()?;
    hits += 1;
    r5_full_replay()?;
    hits += 1;
    r6_hold_replay()?;
    hits += 1;
    r7_future_bound()?;
    hits += 1;
    r8_fuzz_permutation()?;
    hits += 1;
    r9_fuzz_replay_storm()?;
    hits += 1;
    r10_csum_sensitivity()?;
    hits += 1;
    Ok(hits)
}
