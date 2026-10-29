//! UNX-F24138 显存驱逐 LRU 与迁移搬运·子功能18（迁移搬运目标 VRAM 选择）（0x41320 段）
//!
//! 锚点：UNX-F24138-J1 迁移账（源域/目标域选择与搬运逐条落账）；J2
//! 迁移语义（域兼容闸 + 容量闸 + 源清空三性质）；J3 迁移指纹（搬运
//! 账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（迁移目标选择面）。
//!
//! 结构：双域容量表 + 域选择核 + 搬运核 + 迁移账本 + 折叠指纹 + 注入
//! 核（野对象/不兼容域/容量不足/重复搬运/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41320 独占 0x20）。
pub const SEG_BASE: u32 = 0x41320;
pub const MV_UNK_OBJ: u32 = SEG_BASE + 0x01; // 野对象
pub const MV_BAD_DOMAIN: u32 = SEG_BASE + 0x02; // 不兼容域
pub const MV_NO_SPACE: u32 = SEG_BASE + 0x03; // 容量不足
pub const MV_DUP_MOVE: u32 = SEG_BASE + 0x04; // 重复搬运
pub const MV_FROZEN: u32 = SEG_BASE + 0x05; // 印后搬运

/// 内存域。
pub const DOM_VRAM: u8 = 0;
pub const DOM_GTT: u8 = 1;
pub const DOM_SYS: u8 = 2;
pub const DOM_COUNT: u8 = 3;

/// 域兼容矩阵：VRAM↔GTT、GTT↔SYS 可迁移；VRAM↔SYS 直接迁移不兼容。
pub fn domain_compatible(from: u8, to: u8) -> bool {
    if from >= DOM_COUNT || to >= DOM_COUNT || from == to {
        return false;
    }
    !((from == DOM_VRAM && to == DOM_SYS) || (from == DOM_SYS && to == DOM_VRAM))
}

/// 迁移对象。
#[derive(Clone)]
pub struct MvObj {
    pub obj_id: u32,
    pub pages: u32,
    pub domain: u8,
    pub moved: bool,
}

/// 迁移账。
#[derive(Clone, Debug, PartialEq)]
pub struct MvRec {
    pub seq: u32,
    pub obj_id: u32,
    pub from: u8,
    pub to: u8,
    pub pages: u32,
}

/// 迁移簿。
pub struct MvBook {
    pub objs: Vec<MvObj>,
    pub caps: [u32; 3],
    pub used: [u32; 3],
    pub ledger: Vec<MvRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl MvBook {
    pub fn new(caps: [u32; 3]) -> MvBook {
        MvBook { objs: Vec::new(), caps, used: [0; 3], ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn add(&mut self, obj_id: u32, pages: u32, domain: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(MV_FROZEN);
        }
        if self.objs.iter().any(|o| o.obj_id == obj_id) {
            return Err(MV_UNK_OBJ);
        }
        if domain >= DOM_COUNT {
            return Err(MV_BAD_DOMAIN);
        }
        self.objs.push(MvObj { obj_id, pages, domain, moved: false });
        self.used[domain as usize] += pages;
        Ok(())
    }

    /// 目标域选择：兼容域中剩余容量最大的。
    pub fn pick_target(&self, from: u8) -> Result<u8, u32> {
        if from >= DOM_COUNT {
            return Err(MV_BAD_DOMAIN);
        }
        let mut best: Option<u8> = None;
        let mut best_free: u32 = 0;
        for d in 0..DOM_COUNT {
            if d == from || !domain_compatible(from, d) {
                continue;
            }
            let free = self.caps[d as usize].saturating_sub(self.used[d as usize]);
            if free > best_free {
                best_free = free;
                best = Some(d);
            }
        }
        best.ok_or(MV_BAD_DOMAIN)
    }

    /// 搬运：选目标 + 容量闸 + 落账 + 源清空。
    pub fn move_obj(&mut self, obj_id: u32) -> Result<MvRec, u32> {
        if self.sealed {
            return Err(MV_FROZEN);
        }
        let o = match self.objs.iter().find(|o| o.obj_id == obj_id) {
            Some(o) => o.clone(),
            None => return Err(MV_UNK_OBJ),
        };
        if o.moved {
            return Err(MV_DUP_MOVE);
        }
        let to = match self.pick_target(o.domain) {
            Ok(d) => d,
            Err(e) => return Err(e),
        };
        let ti = to as usize;
        if self.caps[ti] - self.used[ti] < o.pages {
            return Err(MV_NO_SPACE);
        }
        let fi = o.domain as usize;
        self.used[fi] -= o.pages;
        self.used[ti] += o.pages;
        if let Some(o) = self.objs.iter_mut().find(|o| o.obj_id == obj_id) {
            o.domain = to;
            o.moved = true;
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        let rec = MvRec { seq, obj_id, from: fi as u8, to, pages: o.pages };
        self.ledger.push(rec.clone());
        Ok(rec)
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41320;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.obj_id);
            h = h.wrapping_mul(31).wrapping_add(r.from as u32);
            h = h.wrapping_mul(31).wrapping_add(r.to as u32);
            h = h.wrapping_mul(31).wrapping_add(r.pages);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：VRAM/GTT/SYS 三域，四对象。
pub fn mk_book() -> MvBook {
    let mut b = MvBook::new([100, 80, 60]);
    let _ = b.add(1, 20, DOM_VRAM);
    let _ = b.add(2, 30, DOM_VRAM);
    let _ = b.add(3, 25, DOM_GTT);
    let _ = b.add(4, 10, DOM_SYS);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野对象
    got[0] = b.move_obj(99).err() == Some(MV_UNK_OBJ);
    // 2) 不兼容域（VRAM↔SYS 无兼容目标之外——SYS 对象目标只有 GTT，VRAM 对象目标只有 GTT；
    //    专门构造：SYS 对象若 GTT 满则无兼容目标）
    let mut b2 = MvBook::new([100, 20, 60]);
    let _ = b2.add(1, 30, DOM_SYS);
    let _ = b2.add(2, 20, DOM_GTT); // GTT 满
    got[1] = b2.move_obj(1).err() == Some(MV_BAD_DOMAIN);
    // 3) 容量不足
    let mut b3 = MvBook::new([100, 10, 60]);
    let _ = b3.add(1, 30, DOM_VRAM); // 目标 GTT 只剩 10
    let _ = b3.add(2, 10, DOM_GTT);
    got[2] = b3.move_obj(1).err() == Some(MV_NO_SPACE);
    // 4) 重复搬运
    let _ = b.move_obj(1);
    got[3] = b.move_obj(1).err() == Some(MV_DUP_MOVE);
    // 5) 印后搬运必抓
    b.seal();
    got[4] = b.move_obj(2).err() == Some(MV_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24138_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24138");
    let mut b = mk_book();

    // J1 对象账齐
    let j1 = b.objs.len() == 4;
    s.add("J1-对象账齐", j1, "四对象全落表");

    // J2 兼容矩阵
    let j2 = domain_compatible(DOM_VRAM, DOM_GTT)
        && domain_compatible(DOM_GTT, DOM_SYS)
        && !domain_compatible(DOM_VRAM, DOM_SYS)
        && !domain_compatible(DOM_VRAM, DOM_VRAM);
    s.add("J2-兼容矩阵", j2, "VRAM↔GTT↔SYS 通 VRAM↔SYS 断");

    // J3 目标选择取兼容域（VRAM 唯一兼容域 GTT；SYS 唯一兼容域 GTT）
    let j3 = {
        // VRAM 对象：兼容域只有 GTT（free=80-25=55）→ GTT
        b.pick_target(DOM_VRAM) == Ok(DOM_GTT) && b.pick_target(DOM_SYS) == Ok(DOM_GTT)
    };
    s.add("J3-选域最大", j3, "兼容域中剩余容量最大者");

    // J4 搬运落账
    let j4 = {
        match b.move_obj(1) {
            Ok(r) => r.from == DOM_VRAM && r.to == DOM_GTT && r.pages == 20,
            Err(_) => false,
        }
    };
    s.add("J4-搬运落账", j4, "源/目标/页数全落账");

    // J5 源清空目标入账
    let j5 = b.used[DOM_VRAM as usize] == 30 && b.used[DOM_GTT as usize] == 45;
    s.add("J5-源清标入", j5, "源域减页目标域加页");

    // J6 对象域更新
    let j6 = b.objs.iter().find(|o| o.obj_id == 1).map(|o| o.domain == DOM_GTT && o.moved).unwrap_or(false);
    s.add("J6-对象域更", j6, "对象域标记更新");

    // J7 重复搬运拒
    let j7 = b.move_obj(1).err() == Some(MV_DUP_MOVE);
    s.add("J7-重复搬运拒", j7, "已搬对象必拒");

    // J8 容量不足拒
    let j8 = {
        let mut b2 = MvBook::new([100, 10, 60]);
        let _ = b2.add(1, 30, DOM_VRAM);
        let _ = b2.add(2, 10, DOM_GTT);
        b2.move_obj(1).err() == Some(MV_NO_SPACE)
    };
    s.add("J8-容量不足拒", j8, "目标剩余 < 页数必拒");

    // J9 不兼容域无目标拒
    let j9 = {
        let mut b2 = MvBook::new([100, 20, 60]);
        let _ = b2.add(1, 30, DOM_SYS);
        let _ = b2.add(2, 20, DOM_GTT);
        b2.move_obj(1).err() == Some(MV_BAD_DOMAIN)
    };
    s.add("J9-无兼容目标拒", j9, "兼容域满必拒");

    // J10 野对象拒
    let j10 = b.move_obj(99).err() == Some(MV_UNK_OBJ);
    s.add("J10-野对象拒", j10, "野对象搬运必拒");

    // J11 迁移账守恒
    let j11 = b.ledger.iter().all(|r| r.from < DOM_COUNT && r.to < DOM_COUNT && r.from != r.to);
    s.add("J11-账守恒", j11, "迁移账 from/to 合法且相异");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24138_move_flow() {
        let mut b = mk_book();
        assert!(b.move_obj(1).is_ok());
        assert_eq!(b.move_obj(1), Err(MV_DUP_MOVE));
        assert_eq!(b.move_obj(99), Err(MV_UNK_OBJ));
        assert!(b.seal() > 0);
        assert_eq!(b.move_obj(2), Err(MV_FROZEN));
        assert!(!domain_compatible(DOM_VRAM, DOM_SYS));
    }

    #[test]
    fn unx24138_checks_green() {
        let s = run_unx24138_checks();
        assert!(s.all_passed(), "UNX-F24138 判据未全绿");
    }
}
