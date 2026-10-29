//! UNX-F10377 正演哈希终锁（0x1b9a0 段）
//!
//! 锚点：UNX-F10377-J1 正演哈希终锁——五段产物齐备才许终锁，终锁后任何
//! 再写必拒，缺件终锁 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·哈希谱系）。
//!
//! 结构：产物登记册（五段各一产物业主）+ 终锁闸（覆盖核→冻结）
//! + 终锁后只读 + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b9a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b9a0;
pub const HL_SHORT_LOCK: u32 = SEG_BASE + 0x01; // 缺件终锁
pub const HL_FROZEN: u32 = SEG_BASE + 0x02; // 终锁后再写
pub const HL_DUP_STAGE: u32 = SEG_BASE + 0x03; // 段重号
pub const HL_EMPTY_NAME: u32 = SEG_BASE + 0x04; // 产物名空
pub const HL_BAD_HASH: u32 = SEG_BASE + 0x05; // 零哈希
pub const HL_UNKNOWN_STAGE: u32 = SEG_BASE + 0x06; // 未知段

/// 五段产物定容。
pub const STAGES: usize = 5;
/// 终锁覆盖锚：五段齐才可终锁。
pub const LOCK_ANCHOR: usize = STAGES;

/// 单条产物登记。
#[derive(Clone)]
pub struct Artifact {
    pub stage: u32,
    pub name: String,
    pub hash: u32,
}

impl Artifact {
    pub fn ok(&self) -> bool {
        (1..=STAGES as u32).contains(&self.stage)
            && !self.name.is_empty()
            && self.hash != 0
    }

    pub fn render(&self) -> String {
        format!("A{}|{}|{:#010x}", self.stage, self.name, self.hash)
    }
}

/// 产物登记册：登记 → 终锁（覆盖核）→ 冻结只读。
pub struct HashLock {
    pub items: Vec<Artifact>,
    pub frozen: bool,
    pub note: String,
}

impl HashLock {
    pub fn new(note: &str) -> HashLock {
        HashLock { items: Vec::new(), frozen: false, note: note.to_string() }
    }

    /// 登记产物：段重号、空名、零哈希、冻结均拒。
    pub fn put(&mut self, stage: u32, name: &str, hash: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(HL_FROZEN);
        }
        if !(1..=STAGES as u32).contains(&stage) {
            return Err(HL_UNKNOWN_STAGE);
        }
        if name.is_empty() {
            return Err(HL_EMPTY_NAME);
        }
        if hash == 0 {
            return Err(HL_BAD_HASH);
        }
        if self.items.iter().any(|a| a.stage == stage) {
            return Err(HL_DUP_STAGE);
        }
        self.items.push(Artifact { stage, name: name.to_string(), hash });
        Ok(())
    }

    /// 覆盖核：五段各有且仅有一件产物。
    pub fn covered(&self) -> bool {
        self.items.len() == LOCK_ANCHOR
            && (1..=STAGES as u32).all(|st| {
                self.items.iter().filter(|a| a.stage == st).count() == 1
            })
    }

    /// 终锁闸：覆盖不达锚拒（缺件终锁必检出）；通过后冻结。
    pub fn final_lock(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(HL_FROZEN);
        }
        if !self.covered() {
            return Err(HL_SHORT_LOCK);
        }
        if !self.items.iter().all(|a| a.ok()) {
            return Err(HL_BAD_HASH);
        }
        self.frozen = true;
        Ok(self.fold())
    }

    /// 缺段诊断。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g = Vec::new();
        for st in 1..=STAGES as u32 {
            if !self.items.iter().any(|a| a.stage == st) {
                g.push(st);
            }
        }
        g
    }

    /// 哈希重算一致性：登记哈希与按 (段,名) 重算的哈希一致。
    pub fn recheck(&self) -> bool {
        self.items.iter().all(|a| a.hash == hash_of(a.stage, &a.name))
    }

    /// 折叠指纹（终锁后即域根哈希）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9a00;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.frozen as u32);
        for a in &self.items {
            h = h.wrapping_mul(31).wrapping_add(a.stage);
            h = h.wrapping_mul(31).wrapping_add(a.hash);
            for b in a.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 产物哈希合成（段号+名折叠，恒非零）。
pub fn hash_of(stage: u32, name: &str) -> u32 {
    let mut h = 0x0d00_0000u32;
    h = h.wrapping_mul(31).wrapping_add(stage);
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h | 1
}

/// 注入核（五项）：[缺件终锁, 终锁再写, 段重号, 零哈希, 未知段] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺件终锁必拒
    let mut l = HashLock::new("inj-short");
    let _ = l.put(1, "run.sh", hash_of(1, "run.sh"));
    got[0] = l.final_lock() == Err(HL_SHORT_LOCK) && l.gaps().len() == STAGES - 1;
    // 2) 终锁后再写必拒
    let mut l2 = HashLock::new("inj-frozen");
    for st in 1..=STAGES as u32 {
        let nm = format!("a{}", st);
        let _ = l2.put(st, &nm, hash_of(st, &nm));
    }
    got[1] = l2.final_lock().is_ok()
        && l2.put(1, "overwrite", hash_of(1, "overwrite")) == Err(HL_FROZEN);
    // 3) 段重号必拒
    let mut l3 = HashLock::new("inj-dup");
    let _ = l3.put(1, "a", hash_of(1, "a"));
    got[2] = l3.put(1, "b", hash_of(1, "b")) == Err(HL_DUP_STAGE);
    // 4) 零哈希必拒
    let mut l4 = HashLock::new("inj-zero");
    got[3] = l4.put(1, "a", 0) == Err(HL_BAD_HASH);
    // 5) 未知段必拒
    let mut l5 = HashLock::new("inj-unk");
    got[4] = l5.put(9, "a", hash_of(9, "a")) == Err(HL_UNKNOWN_STAGE);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10377_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10377");

    // 正路径：五段产物齐备 → 终锁成功 → 指纹非零
    let mut lock = HashLock::new("正演哈希终锁");
    let names = ["run.sh", "cond.txt", "reg.out", "verdict.txt", "report.md"];
    for (k, nm) in names.iter().enumerate() {
        let st = k as u32 + 1;
        let _ = lock.put(st, nm, hash_of(st, nm));
    }
    let lock_fp = lock.final_lock();
    let j1 = lock_fp.is_ok() && lock_fp.unwrap_or(0) != 0;
    s.add("J1-五段齐锁", j1, "五段产物齐备终锁成功");

    // 终锁后冻结：covered 与 gaps 语义保持只读
    let j2 = lock.frozen && lock.covered() && lock.gaps().is_empty();
    s.add("J2-终锁冻结态", j2, "终锁后冻结且覆盖满");

    // 哈希重算一致：登记哈希与重算全等
    let j3 = lock.recheck();
    s.add("J3-哈希重算一致", j3, "登记哈希与按段名重算一致");

    // 终锁指纹内容敏感：同内容复现一致，异内容互异
    let j4 = {
        let build = |name3: &str| -> u32 {
            let mut c = HashLock::new("det");
            let _ = c.put(1, "a.bin", hash_of(1, "a.bin"));
            let _ = c.put(2, "b.bin", hash_of(2, "b.bin"));
            let _ = c.put(3, name3, hash_of(3, name3));
            let _ = c.put(4, "d.bin", hash_of(4, "d.bin"));
            let _ = c.put(5, "e.bin", hash_of(5, "e.bin"));
            c.final_lock().unwrap_or(0)
        };
        build("c.bin") == build("c.bin") && build("c.bin") != build("x.bin")
    };
    s.add("J4-指纹内容敏感", j4, "同内容终锁指纹一致且异内容互异");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "缺件终锁/终锁再写/段重号必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "零哈希与未知段必检出");

    // 终锁后再写（正册复现）
    let j7 = lock.put(2, "late", hash_of(2, "late")) == Err(HL_FROZEN);
    s.add("J7-终锁拒写", j7, "终锁后产物登记必拒");

    // 指纹稳定
    let j8 = lock.fold() == lock.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 哈希口径：段或名变则哈希变，且恒非零
    let j9 = hash_of(1, "a") != hash_of(2, "a")
        && hash_of(1, "a") != hash_of(1, "b")
        && hash_of(1, "a") != 0;
    s.add("J9-哈希口径", j9, "段名变则哈希变且恒非零");

    // 缺件终锁诊断：缺口可枚举
    let j10 = {
        let mut c = HashLock::new("short2");
        let _ = c.put(2, "cond.txt", hash_of(2, "cond.txt"));
        let _ = c.put(4, "verdict.txt", hash_of(4, "verdict.txt"));
        c.gaps() == vec![1, 3, 5] && c.final_lock() == Err(HL_SHORT_LOCK)
    };
    s.add("J10-缺口可枚举", j10, "缺段编号全部列出且终锁拒");

    // 明细落账含段号与哈希
    let j11 = {
        let mut c = HashLock::new("render");
        let _ = c.put(3, "reg.out", hash_of(3, "reg.out"));
        let r = c.items[0].render();
        r.contains("A3|") && r.contains("0x")
    };
    s.add("J11-明细落账", j11, "渲染含段号与哈希字样");

    // 覆盖核口径：六件（不可能，段重号挡）→ 重号路径覆盖
    let j12 = {
        let mut c = HashLock::new("dup2");
        let _ = c.put(1, "a", hash_of(1, "a"));
        let _ = c.put(1, "a2", hash_of(1, "a2"));
        !c.covered()
    };
    s.add("J12-重号不覆盖", j12, "段重号下覆盖核必假");

    // 空册终锁拒且指纹为空册值
    let j13 = {
        let mut c = HashLock::new("empty");
        c.final_lock() == Err(HL_SHORT_LOCK) && c.gaps().len() == STAGES
    };
    s.add("J13-空册终锁拒", j13, "空册缺件终锁必拒");

    // 只读不改账
    let j14 = {
        let mut c = HashLock::new("stable");
        let _ = c.put(1, "a", hash_of(1, "a"));
        let f1 = c.fold();
        let _ = c.covered();
        let _ = c.gaps();
        let _ = c.recheck();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-终锁指纹落账",
        lock.fold() != 0,
        &format!("fold={:#x}", lock.fold()),
    );
    let empty = HashLock::new("empty2").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1b9a00u32.wrapping_mul(31).wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10377_hashlock_flow() {
        let mut l = HashLock::new("t");
        assert!(l.put(1, "a", hash_of(1, "a")).is_ok());
        assert!(l.put(1, "b", hash_of(1, "b")) == Err(HL_DUP_STAGE));
        assert!(l.final_lock() == Err(HL_SHORT_LOCK));
        for st in 2..=5u32 {
            let nm = format!("p{}", st);
            assert!(l.put(st, &nm, hash_of(st, &nm)).is_ok());
        }
        assert!(l.final_lock().is_ok());
        assert!(l.put(3, "late", hash_of(3, "late")) == Err(HL_FROZEN));
    }

    #[test]
    fn unx10377_checks_green() {
        let s = run_unx10377_checks();
        assert!(s.all_passed(), "UNX-F10377 判据未全绿");
    }
}
