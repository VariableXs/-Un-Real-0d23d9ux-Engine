//! UNX-F10324 基建面联签（调度/内存/中断判据互认）（0x1b300 段）
//!
//! 锚点：UNX-F10324-J1 基建加权共识——调度/内存/中断三域判据逐条带权
//! 登记（权 1..=5），甲乙双裁同向计权，全册互认权占总权四分之三以上
//! 且每域至少两条全互认才算基建面联签过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：带权判据册（行=域号+键+权+双裁）+ 加权核（互认权求和与四分
//! 之三门槛）+ 域锚（三域各至少两条）+ 注入核（坏域号/零权/超权/
//! 未裁判权四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b300 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b300;
pub const IF_DOM: u32 = SEG_BASE + 0x01; // 域号越界
pub const IF_DUP: u32 = SEG_BASE + 0x02; // 键重号
pub const IF_WEIGHT: u32 = SEG_BASE + 0x03; // 权越界
pub const IF_EMPTY: u32 = SEG_BASE + 0x04; // 题名空
pub const IF_CAP: u32 = SEG_BASE + 0x05; // 册超容
pub const IF_NOROW: u32 = SEG_BASE + 0x06; // 未知判据行
pub const IF_REVOTE: u32 = SEG_BASE + 0x07; // 已投再投
pub const IF_SEALED: u32 = SEG_BASE + 0x08; // 联签后再动

/// 基建三域。
pub const DOM_SCHED: u8 = 1;
pub const DOM_MEM: u8 = 2;
pub const DOM_IRQ: u8 = 3;
pub const DOM_N: u8 = 4;

/// 每域最少判据条数。
pub const DOM_NEED: usize = 2;
/// 权上界（含）。
pub const W_MAX: u32 = 5;
/// 册定容。
pub const CAP: usize = 10;

/// 带权判据行。
#[derive(Clone)]
pub struct InfraRow {
    pub dom: u8,
    pub key: String,
    pub name: String,
    pub weight: u32,
    pub a_ok: Option<bool>,
    pub b_ok: Option<bool>,
}

impl InfraRow {
    /// 双裁齐备且同向。
    pub fn agree(&self) -> bool {
        self.a_ok == self.b_ok && self.a_ok.is_some() && self.b_ok.is_some()
    }
}

/// 基建面联签册（加权共识）。
pub struct InfraJoint {
    pub rows: Vec<InfraRow>,
    pub sealed: bool,
}

impl InfraJoint {
    pub fn new() -> InfraJoint {
        InfraJoint { rows: Vec::new(), sealed: false }
    }

    fn pos(&self, key: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].key == key {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn dom_rows(&self, dom: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].dom == dom {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 登记带权判据。
    pub fn register(&mut self, dom: u8, key: &str, name: &str, weight: u32) -> Result<(), u32> {
        if dom == 0 || dom >= DOM_N {
            return Err(IF_DOM);
        }
        if self.sealed {
            return Err(IF_SEALED);
        }
        if self.pos(key).is_some() {
            return Err(IF_DUP);
        }
        if weight == 0 || weight > W_MAX {
            return Err(IF_WEIGHT);
        }
        if key.is_empty() || name.is_empty() {
            return Err(IF_EMPTY);
        }
        if self.rows.len() >= CAP {
            return Err(IF_CAP);
        }
        self.rows.push(InfraRow {
            dom,
            key: key.to_string(),
            name: name.to_string(),
            weight,
            a_ok: None,
            b_ok: None,
        });
        Ok(())
    }

    /// 甲方投票。
    pub fn a_vote(&mut self, key: &str, ok: bool) -> Result<(), u32> {
        match self.pos(key) {
            None => Err(IF_NOROW),
            Some(p) => {
                if self.rows[p].a_ok.is_some() {
                    return Err(IF_REVOTE);
                }
                self.rows[p].a_ok = Some(ok);
                Ok(())
            }
        }
    }

    /// 乙方投票。
    pub fn b_vote(&mut self, key: &str, ok: bool) -> Result<(), u32> {
        match self.pos(key) {
            None => Err(IF_NOROW),
            Some(p) => {
                if self.rows[p].b_ok.is_some() {
                    return Err(IF_REVOTE);
                }
                self.rows[p].b_ok = Some(ok);
                Ok(())
            }
        }
    }

    /// 全册总权。
    pub fn weight_total(&self) -> u32 {
        let mut w = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            w = w.wrapping_add(self.rows[k].weight);
            k += 1;
        }
        w
    }

    /// 互认权（双裁同向行的权之和）。
    pub fn weight_agreed(&self) -> u32 {
        let mut w = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].agree() {
                w = w.wrapping_add(self.rows[k].weight);
            }
            k += 1;
        }
        w
    }

    /// 加权共识：互认权四倍 >= 总权三倍（即 >= 3/4）。
    pub fn consensus(&self) -> bool {
        let t = self.weight_total();
        if t == 0 {
            return false;
        }
        self.weight_agreed() * 4 >= t * 3
    }

    /// 单域核：行数达锚且域内全互认。
    pub fn domain_ok(&self, dom: u8) -> bool {
        if dom == 0 || dom >= DOM_N {
            return false;
        }
        if self.dom_rows(dom) < DOM_NEED {
            return false;
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].dom == dom && !self.rows[k].agree() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 基建面联签过：三域各达锚且加权共识成立。
    pub fn mutual(&self) -> bool {
        let mut d = 1u8;
        while d < DOM_N {
            if !self.domain_ok(d) {
                return false;
            }
            d += 1;
        }
        self.consensus()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 基建面指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3000;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = h.wrapping_mul(31).wrapping_add(r.dom as u32);
            h = h.wrapping_mul(31).wrapping_add(r.weight);
            h = h.wrapping_mul(31).wrapping_add(r.a_ok.unwrap_or(false) as u32 + 2);
            h = h.wrapping_mul(31).wrapping_add(r.b_ok.unwrap_or(false) as u32 + 2);
            let kb = r.key.as_bytes();
            let mut i = 0usize;
            while i < kb.len() {
                h = h.wrapping_mul(31).wrapping_add(kb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut j = InfraJoint::new();
    // 1) 坏域号必拒
    got[0] = j.register(0, "x", "甲", 1).err() == Some(IF_DOM)
        && j.register(4, "y", "乙", 1).err() == Some(IF_DOM);
    // 2) 零权与超权必拒
    let _ = j.register(DOM_SCHED, "rr", "轮转", 3);
    got[1] = j.register(DOM_MEM, "zero", "零权", 0).err() == Some(IF_WEIGHT)
        && j.register(DOM_IRQ, "big", "超权", W_MAX + 1).err() == Some(IF_WEIGHT);
    // 3) 未知行投票必拒
    got[2] = j.a_vote("不存在", true).err() == Some(IF_NOROW);
    // 4) 重投必拒
    let _ = j.a_vote("rr", true);
    got[3] = j.a_vote("rr", false).err() == Some(IF_REVOTE);
    got
}

/// 基建样例（域号, 键, 题名, 权）。
pub fn infra_samples() -> Vec<(u8, &'static str, &'static str, u32)> {
    let mut v: Vec<(u8, &'static str, &'static str, u32)> = Vec::new();
    v.push((DOM_SCHED, "rr_slice", "时间片轮转", 5));
    v.push((DOM_SCHED, "prio", "优先级序", 3));
    v.push((DOM_MEM, "pma_map", "物理映射", 4));
    v.push((DOM_MEM, "frame_free", "帧回收", 2));
    v.push((DOM_IRQ, "eoi_ack", "中断应答", 3));
    v.push((DOM_IRQ, "mask_set", "屏蔽设置", 1));
    v
}

/// 批内判据。
pub fn run_unx10324_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10324");

    // J1 正路径：三域带权登记双裁互认，共识成立
    let mut j = InfraJoint::new();
    let samples = infra_samples();
    let mut ok = true;
    let mut k = 0usize;
    while k < samples.len() {
        let (d, key, name, w) = (samples[k].0, samples[k].1, samples[k].2, samples[k].3);
        if j.register(d, key, name, w).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < samples.len() {
        let (d, key, _, _) = (samples[k].0, samples[k].1, samples[k].2, samples[k].3);
        if j.a_vote(key, true).is_err() || j.b_vote(key, true).is_err() {
            ok = false;
        }
        k += 1;
    }
    s.add(
        "J1-加权共识",
        ok && j.mutual() && j.consensus(),
        "三域六条带权判据全互认且占总权四分之三以上",
    );

    // J2 权账：总权 18 互认权 18
    let j2 = j.weight_total() == 18 && j.weight_agreed() == 18;
    s.add("J2-权账对平", j2, "总权与互认权按登记权求和");

    // J3 注入检出：坏域号 + 权越界
    let inj = inject_four();
    s.add("J3-域号权重检出", inj[0] && inj[1], "坏域号与零权超权必拒");

    // J4 注入检出：未知行 + 重投
    s.add("J4-未知重投检出", inj[2] && inj[3], "未知行投票与重投必拒");

    // J5 低互认权不成共识（互认权 5/18 低于 3/4）
    let j5 = {
        let mut t = InfraJoint::new();
        let _ = t.register(DOM_SCHED, "a", "甲", 5);
        let _ = t.register(DOM_SCHED, "b", "乙", 3);
        let _ = t.register(DOM_MEM, "c", "丙", 4);
        let _ = t.register(DOM_MEM, "d", "丁", 2);
        let _ = t.register(DOM_IRQ, "e", "戊", 3);
        let _ = t.register(DOM_IRQ, "f", "己", 1);
        let _ = t.a_vote("a", true);
        let _ = t.b_vote("a", true);
        !t.consensus()
    };
    s.add("J5-低权不成", j5, "互认权不足四分之三不成共识");

    // J6 负裁拉低互认权
    let j6 = {
        let mut t = InfraJoint::new();
        let _ = t.register(DOM_SCHED, "a", "甲", 5);
        let _ = t.a_vote("a", true);
        let _ = t.b_vote("a", false);
        t.weight_agreed() == 0 && !t.consensus()
    };
    s.add("J6-异裁零权", j6, "异裁行不计互认权");

    // J7 单域缺条不成互认（调度域只登一条）
    let j7 = {
        let mut t = InfraJoint::new();
        let _ = t.register(DOM_SCHED, "only", "唯一", 5);
        let _ = t.register(DOM_MEM, "m1", "一", 2);
        let _ = t.register(DOM_MEM, "m2", "二", 2);
        let _ = t.register(DOM_IRQ, "i1", "一", 1);
        let _ = t.register(DOM_IRQ, "i2", "二", 1);
        !t.domain_ok(DOM_SCHED)
    };
    s.add("J7-域缺条拒", j7, "每域不足两条不成域互认");

    // J8 重复键必拒
    let j8 = {
        let mut t = InfraJoint::new();
        let _ = t.register(DOM_SCHED, "rr", "甲", 1);
        t.register(DOM_MEM, "rr", "重键", 1).err() == Some(IF_DUP)
    };
    s.add("J8-重键拒收", j8, "键全册唯一");

    // J9 容量边界
    let j9 = {
        let mut t = InfraJoint::new();
        let mut n = 0u8;
        let mut filled = true;
        while (n as usize) < CAP {
            let d = 1 + (n % 3);
            let key = format!("k{}", n);
            if t.register(d, &key, "填", 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.register(DOM_SCHED, "over", "超", 1).err() == Some(IF_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原册不损");

    // J10 指纹随投票推进
    let j10 = {
        let mut t = InfraJoint::new();
        let _ = t.register(DOM_SCHED, "a", "甲", 2);
        let f0 = t.fold();
        let _ = t.a_vote("a", true);
        let f1 = t.fold();
        let _ = t.b_vote("a", true);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每投一步指纹一步且终态稳定");

    // J11 域覆盖逐域达锚
    let j11 = j.domain_ok(DOM_SCHED) && j.domain_ok(DOM_MEM) && j.domain_ok(DOM_IRQ);
    s.add("J11-域覆盖", j11, "调度/内存/中断三域各两条全互认");

    // J12 动态明细：互认权占比（万分比）
    let pct = if j.weight_total() > 0 {
        j.weight_agreed() * 10000 / j.weight_total()
    } else {
        0
    };
    s.add_dyn(
        "J12-互认权占比",
        pct == 10000,
        &format!("permyriad={}", pct),
    );

    // J13 动态明细：基建面指纹落账
    let fp = j.fold();
    s.add_dyn("J13-基建指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10324_infra_flow() {
        let mut j = InfraJoint::new();
        assert!(j.register(0, "x", "甲", 1).err() == Some(IF_DOM));
        assert!(j.register(DOM_SCHED, "a", "甲", 0).err() == Some(IF_WEIGHT));
        assert!(j.register(DOM_SCHED, "a", "甲", W_MAX + 1).err() == Some(IF_WEIGHT));
        assert!(j.register(DOM_SCHED, "a", "", 1).err() == Some(IF_EMPTY));
        assert!(j.register(DOM_SCHED, "a", "甲", 3).is_ok());
        assert!(j.register(DOM_MEM, "a", "重键", 1).err() == Some(IF_DUP));
        assert!(j.a_vote("无", true).err() == Some(IF_NOROW));
        assert!(j.a_vote("a", true).is_ok());
        assert!(j.a_vote("a", false).err() == Some(IF_REVOTE));
        assert!(!j.consensus());
        j.seal();
        assert!(j.register(DOM_MEM, "b", "乙", 1).err() == Some(IF_SEALED));
    }

    #[test]
    fn unx10324_checks_green() {
        let s = run_unx10324_checks();
        assert!(s.all_passed(), "UNX-F10324 判据未全绿");
    }
}
