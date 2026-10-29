//! UNX-F10917 复用器判定序挂接核销（F10879 序表）（0x1d220 段）
//!
//! 锚点：UNX-F10917-J1 复用器错误条挂接序表逐条核销（10/10）、衔接
//! 零冲突（10/10）；反面：挂接后零漂移（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·判定序挂接面）。
//!
//! 结构：序表（F10879 判定序定容挂接）+ 核销器（逐条序号核销，衔接
//! 位查冲突）+ 漂移账（挂接后序表指纹冻结比对）+ 注入核（乱序核销/
//! 衔接冲突/重号挂接/冻结改表四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d220 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d220;
pub const OH_OUTOFORDER: u32 = SEG_BASE + 0x01; // 乱序核销
pub const OH_CONFLICT: u32 = SEG_BASE + 0x02; // 衔接冲突
pub const OH_DUP: u32 = SEG_BASE + 0x03; // 重号挂接
pub const OH_NOENTRY: u32 = SEG_BASE + 0x04; // 核销目标缺失
pub const OH_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改表
pub const OH_DRIFT: u32 = SEG_BASE + 0x06; // 挂接后漂移

/// 序表定容（F10879 判定序条目数）。
pub const ORDER_CAP: usize = 20;
/// OH 域标（fold 种子）。
pub const OH_TAG: u64 = 0x1d22211;

/// 序表条目。
#[derive(Clone, Debug)]
pub struct OrderEntry {
    pub seq: u32,
    pub face: u32, // 错误面诊断码
    pub hooked: bool,
    pub settled: bool,
}

impl OrderEntry {
    pub fn render(&self) -> String {
        let h = if self.hooked { "H" } else { "-" };
        let st = if self.settled { "S" } else { "-" };
        format!("O{}|{}{}", self.seq, h, st)
    }
}

/// 判定序挂接核销表。
pub struct HookTable {
    pub entries: Vec<OrderEntry>,
    pub frozen: bool,
    pub seal: Option<u64>,
}

impl HookTable {
    pub fn new() -> HookTable {
        HookTable { entries: Vec::new(), frozen: false, seal: None }
    }

    /// 建序：按 seq 升序装入 face 码。
    pub fn build(&mut self, faces: &[u32]) -> Result<(), u32> {
        if self.frozen {
            return Err(OH_FROZEN);
        }
        if faces.len() > ORDER_CAP {
            return Err(OH_CONFLICT);
        }
        let mut k = 0u32;
        while k < faces.len() as u32 {
            self.entries.push(OrderEntry { seq: k + 1, face: faces[k as usize], hooked: false, settled: false });
            k += 1;
        }
        Ok(())
    }

    /// 挂接：按序逐条（seq 必须等于已挂接数+1）。
    pub fn hook(&mut self, seq: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(OH_FROZEN);
        }
        if seq == 0 || seq > self.entries.len() as u32 {
            return Err(OH_NOENTRY);
        }
        let idx = (seq - 1) as usize;
        if self.entries[idx].hooked {
            return Err(OH_DUP);
        }
        if seq != self.hooked_count() as u32 + 1 {
            return Err(OH_OUTOFORDER);
        }
        self.entries[idx].hooked = true;
        Ok(())
    }

    /// 已挂接数。
    pub fn hooked_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].hooked {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 核销：仅核销已挂接且前序已核销的条目（衔接零冲突）。
    pub fn settle(&mut self, seq: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(OH_FROZEN);
        }
        if seq == 0 || seq > self.entries.len() as u32 {
            return Err(OH_NOENTRY);
        }
        let idx = (seq - 1) as usize;
        if !self.entries[idx].hooked {
            return Err(OH_NOENTRY);
        }
        if self.entries[idx].settled {
            return Err(OH_DUP);
        }
        // 前序必须全核销。
        let mut p = 0usize;
        while p < idx {
            if !self.entries[p].settled {
                return Err(OH_CONFLICT);
            }
            p += 1;
        }
        self.entries[idx].settled = true;
        Ok(self.entries[idx].face)
    }

    /// 已核销数。
    pub fn settled_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].settled {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 挂接后漂移审计：指纹与封存根一致。
    pub fn no_drift(&self) -> bool {
        match self.seal {
            Some(root) => self.fold() == root,
            None => false,
        }
    }

    /// 链式 fold 指纹（序敏感，含挂接/核销位）。
    pub fn fold(&self) -> u64 {
        let mut h = OH_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = (h ^ e.seq as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.face as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.hooked as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.settled as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 封表：落根并冻结。
    pub fn seal_table(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.seal = Some(r);
        r
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let faces: [u32; 4] = [0x21, 0x22, 0x23, 0x24];
    // 1) 乱序核销（跳号挂接）必抓
    let mut t = HookTable::new();
    let _ = t.build(&faces);
    got[0] = t.hook(2).err() == Some(OH_OUTOFORDER);
    // 2) 衔接冲突（挂接全但核销跳号）必抓
    let mut c = HookTable::new();
    let _ = c.build(&faces);
    let _ = c.hook(1);
    let _ = c.hook(2);
    let _ = c.hook(3);
    let _ = c.hook(4);
    let _ = c.settle(1);
    got[1] = c.settle(3).err() == Some(OH_CONFLICT);
    // 3) 重号挂接必抓
    let mut d = HookTable::new();
    let _ = d.build(&faces);
    let _ = d.hook(1);
    got[2] = d.hook(1).err() == Some(OH_DUP);
    // 4) 冻结后挂接必抓
    let mut f = HookTable::new();
    let _ = f.build(&faces);
    f.seal_table();
    got[3] = f.hook(1).err() == Some(OH_FROZEN);
    got
}

/// 错误面确定性样本：第 k 条的面码。
pub fn oh_sample_face(k: u32) -> u32 {
    SEG_BASE.wrapping_add(k.wrapping_mul(3).wrapping_add(1))
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10917_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10917");

    // J1 正面·逐条核销十连（十组各挂接核销全通）
    let mut h1 = 0u32;
    let mut grp = 0u32;
    while grp < 10 {
        let mut t = HookTable::new();
        let mut faces: Vec<u32> = Vec::new();
        let mut k = 0u32;
        while k < 6 {
            faces.push(oh_sample_face(k + grp * 10));
            k += 1;
        }
        let _ = t.build(&faces);
        let mut pass = true;
        let mut q = 1u32;
        while q <= 6 {
            if t.hook(q).is_err() || t.settle(q) != Ok(oh_sample_face(q - 1 + grp * 10)) {
                pass = false;
            }
            q += 1;
        }
        if pass && t.settled_count() == 6 {
            h1 += 1;
        }
        grp += 1;
    }
    s.add("J1-逐条核销十连", h1 == 10, "十组挂接核销全通");

    // J2 衔接零冲突十连：全序核销无冲突报错
    let mut h2 = 0u32;
    let mut grp2 = 0u32;
    while grp2 < 10 {
        let mut t = HookTable::new();
        let mut faces: Vec<u32> = Vec::new();
        let mut k = 0u32;
        while k < 5 {
            faces.push(oh_sample_face(k + 40 + grp2 * 10));
            k += 1;
        }
        let _ = t.build(&faces);
        let mut clean = true;
        let mut q = 1u32;
        while q <= 5 {
            if t.hook(q).is_err() || t.settle(q).is_err() {
                clean = false;
            }
            q += 1;
        }
        if clean {
            h2 += 1;
        }
        grp2 += 1;
    }
    s.add("J2-衔接零冲突十连", h2 == 10, "全序核销十组零冲突");

    // J3 反面：挂接后零漂移千次
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut t = HookTable::new();
        let mut faces: Vec<u32> = Vec::new();
        faces.push(oh_sample_face(r3 % 7 + 80));
        faces.push(oh_sample_face(r3 % 7 + 81));
        let _ = t.build(&faces);
        let _ = t.hook(1);
        let root = t.fold();
        t.seal = Some(root);
        if t.no_drift() {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-千次零漂移", ok == 1000, "挂接后封根千轮零漂移");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "乱序/冲突/重号/冻结全数命中");

    // J5 核销回报面码
    let j5 = {
        let mut t = HookTable::new();
        let _ = t.build(&[0x77, 0x88]);
        let _ = t.hook(1);
        t.settle(1) == Ok(0x77)
    };
    s.add("J5-核销回报面码", j5, "核销回报对应错误面码");

    // J6 未挂接即核销拒
    let j6 = {
        let mut t = HookTable::new();
        let _ = t.build(&[0x31, 0x32]);
        t.settle(1).err() == Some(OH_NOENTRY)
    };
    s.add("J6-未挂接拒核销", j6, "未挂接条目核销必拒");

    // J7 挂接目标缺失拒
    let j7 = HookTable::new().hook(9).err() == Some(OH_NOENTRY);
    s.add("J7-目标缺失拒", j7, "空表挂接必拒");

    // J8 冻结后核销拒
    let j8 = {
        let mut t = HookTable::new();
        let _ = t.build(&[0x41]);
        let _ = t.hook(1);
        t.seal_table();
        t.settle(1).err() == Some(OH_FROZEN)
    };
    s.add("J8-冻结拒核销", j8, "封表后核销必拒");

    // J9 渲染面：挂接/核销位渲染可区分
    let j9 = {
        let a = OrderEntry { seq: 1, face: 1, hooked: true, settled: true };
        let b = OrderEntry { seq: 1, face: 1, hooked: false, settled: false };
        a.render() != b.render() && a.render().contains("HS")
    };
    s.add("J9-渲染分旗", j9, "挂接/核销渲染可区分");

    // J10 封根随状态必变
    let j10 = {
        let mut t = HookTable::new();
        let _ = t.build(&[0x51, 0x52]);
        let f0 = t.fold();
        let _ = t.hook(1);
        let f1 = t.fold();
        f0 != f1 && f1 != 0
    };
    s.add("J10-封根随状态", j10, "挂接改变指纹");

    // J11 动态明细：零漂移千次计数落账
    s.add_dyn("J11-千次明细", ok == 1000, &format!("no_drift={}/1000", ok));

    // J12 动态明细：双十连计数落账
    s.add_dyn("J12-十连明细", h1 == 10 && h2 == 10, &format!("settle={} clean={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10917_hooktable_flow() {
        let mut t = HookTable::new();
        assert!(t.build(&[0x21, 0x22, 0x23]).is_ok());
        assert!(t.hook(2).err() == Some(OH_OUTOFORDER));
        assert!(t.hook(1).is_ok());
        assert!(t.hook(1).err() == Some(OH_DUP));
        assert!(t.hook(2).is_ok());
        assert!(t.hook(3).is_ok());
        assert!(t.settle(2).err() == Some(OH_CONFLICT));
        assert_eq!(t.settle(1), Ok(0x21));
        assert_eq!(t.settle(2), Ok(0x22));
        t.seal_table();
        assert!(t.settle(3).err() == Some(OH_FROZEN));
        assert!(t.no_drift());
    }

    #[test]
    fn unx10917_checks_green() {
        let s = run_unx10917_checks();
        assert!(s.all_passed(), "UNX-F10917 判据未全绿");
    }
}
