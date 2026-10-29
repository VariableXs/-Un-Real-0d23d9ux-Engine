//! UNX-F10362 正演脚本终版锁定（与 B38 会签脚本哈希核）（0x1b7c0 段）
//!
//! 锚点：UNX-F10362-J1 正演脚本终版锁定——正演脚本步序（步号接续+动
//! 作码在册）逐步入账，步数达锚后算全脚本 u64 哈希，与 B38 会签脚本
//! 哈希对表，哈希不核即拒锁，哈希核过即终版锁定，锁后步序不得增删改。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：脚本账（步=号+动作码）+ 步序核（号接续不重）+ 哈希核（u64
//! FNV 全脚本叠算）+ 锁定核（对表 B38 方锁）+ 注入核（重步/坏码/短
//! 脚本/哈希不核四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b7c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b7c0;
pub const SL_SEQ: u32 = SEG_BASE + 0x01; // 步号不接续
pub const SL_OP: u32 = SEG_BASE + 0x02; // 动作码越界
pub const SL_CAP: u32 = SEG_BASE + 0x03; // 账超容
pub const SL_DUP: u32 = SEG_BASE + 0x04; // 步号重落
pub const SL_SHORT: u32 = SEG_BASE + 0x05; // 步数不足锁
pub const SL_MISMATCH: u32 = SEG_BASE + 0x06; // 与 B38 哈希不核
pub const SL_LOCKED: u32 = SEG_BASE + 0x07; // 已锁再动

/// 动作码六型。
pub const OP_SETUP: u32 = 1;
pub const OP_RUN: u32 = 2;
pub const OP_VERIFY: u32 = 3;
pub const OP_SNAP: u32 = 4;
pub const OP_REPLAY: u32 = 5;
pub const OP_WRAP: u32 = 6;
pub const OP_N: u32 = 7;

/// 步数锚。
pub const STEPS_NEEDED: usize = 6;
/// 账定容。
pub const CAP: usize = 12;

/// 正演脚本终版账。
#[derive(Clone)]
pub struct ScriptLock {
    pub steps: Vec<(u32, u32)>,
    pub locked: bool,
    pub locked_hash: Option<u64>,
}

impl ScriptLock {
    /// 空账。
    pub fn new() -> Self {
        ScriptLock {
            steps: Vec::new(),
            locked: false,
            locked_hash: None,
        }
    }

    /// 落步：号接续、动作码在册、重号即拒。
    pub fn add_step(&mut self, no: u32, op: u32) -> Result<(), u32> {
        if self.locked {
            return Err(SL_LOCKED);
        }
        if op < OP_SETUP || op >= OP_N {
            return Err(SL_OP);
        }
        if self.steps.len() >= CAP {
            return Err(SL_CAP);
        }
        if no as usize != self.steps.len() + 1 {
            return Err(SL_SEQ);
        }
        let mut i = 0;
        while i < self.steps.len() {
            if self.steps[i].0 == no {
                return Err(SL_DUP);
            }
            i += 1;
        }
        self.steps.push((no, op));
        Ok(())
    }

    /// 步数达锚？
    pub fn long_enough(&self) -> bool {
        self.steps.len() >= STEPS_NEEDED
    }

    /// 全脚本 u64 哈希（FNV-1a 逐步叠 号/动作码）。
    pub fn script_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.steps.len() {
            h = (h ^ (self.steps[i].0 as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.steps[i].1 as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }

    /// 与 B38 会签哈希对表。
    pub fn matches_b38(&self, b38_hash: u64) -> bool {
        self.script_hash() == b38_hash
    }

    /// 终版锁定：步数达锚且哈希核过 B38 方可。
    pub fn lock(&mut self, b38_hash: u64) -> Result<u64, u32> {
        if self.locked {
            return Err(SL_LOCKED);
        }
        if !self.long_enough() {
            return Err(SL_SHORT);
        }
        if !self.matches_b38(b38_hash) {
            return Err(SL_MISMATCH);
        }
        let h = self.script_hash();
        self.locked = true;
        self.locked_hash = Some(h);
        Ok(h)
    }

    /// 动作码序列。
    pub fn op_seq(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.steps.len() {
            out.push(self.steps[i].1);
            i += 1;
        }
        out
    }

    /// 折叠指纹 u32（哈希高 32 位叠步数）。
    pub fn fold(&self) -> u32 {
        let h = self.script_hash();
        (h as u32) ^ ((h >> 32) as u32) ^ (self.steps.len() as u32)
    }
}

/// 正演脚本样例：六步全链。
pub fn sample_script() -> [(u32, u32); 6] {
    [
        (1, OP_SETUP),
        (2, OP_RUN),
        (3, OP_VERIFY),
        (4, OP_SNAP),
        (5, OP_REPLAY),
        (6, OP_WRAP),
    ]
}

/// B38 会签脚本哈希（与样例脚本一致的基准）。
pub fn b38_hash() -> u64 {
    let mut s = ScriptLock::new();
    let mut k = 0;
    while k < sample_script().len() {
        let (no, op) = sample_script()[k];
        let _ = s.add_step(no, op);
        k += 1;
    }
    s.script_hash()
}

/// 注入核：四路坏账必抓。
/// 0: 步号跳落；1: 坏动作码；2: 短脚本锁拒；3: 哈希不核锁拒。
pub fn inject_four() -> [bool; 4] {
    let mut s = ScriptLock::new();
    let mut k = 0;
    while k < sample_script().len() {
        let (no, op) = sample_script()[k];
        let _ = s.add_step(no, op);
        k += 1;
    }
    let d0 = s.add_step(8, OP_RUN).err() == Some(SL_SEQ);
    let d1 = s.add_step(7, 9).err() == Some(SL_OP);
    let d2 = {
        let mut short = ScriptLock::new();
        let _ = short.add_step(1, OP_SETUP);
        short.lock(b38_hash()).err() == Some(SL_SHORT)
    };
    let d3 = s.lock(b38_hash() ^ 0xdead).err() == Some(SL_MISMATCH);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10362_checks。
pub fn run_unx10362_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10362");
    // J1 正路径：六步全落。
    let mut sc = ScriptLock::new();
    let mut j1 = true;
    let mut k = 0;
    while k < sample_script().len() {
        let (no, op) = sample_script()[k];
        if sc.add_step(no, op).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-六步落账", j1 && sc.steps.len() == 6, "全链六步入账");
    // J2 注入：步号跳落必抓。
    let mut s2 = ScriptLock::new();
    let _ = s2.add_step(1, OP_SETUP);
    let j2 = s2.add_step(3, OP_RUN).err() == Some(SL_SEQ);
    s.add("J2-跳步抓", j2, "步号不接续即 SL_SEQ");
    // J3 注入：重步号必抓（接续错位同诊）。
    let mut s3 = ScriptLock::new();
    let _ = s3.add_step(1, OP_SETUP);
    let j3 = s3.add_step(1, OP_RUN).err() == Some(SL_SEQ);
    s.add("J3-重步抓", j3, "重号即步序违 SL_SEQ");
    // J4 注入：坏动作码必抓。
    let mut s4 = ScriptLock::new();
    let j4 = s4.add_step(1, 0).err() == Some(SL_OP)
        && s4.add_step(1, 7).err() == Some(SL_OP);
    s.add("J4-坏码抓", j4, "动作码越界即 SL_OP");
    // J5 注入：短脚本锁拒。
    let mut s5 = ScriptLock::new();
    let mut k5 = 0;
    while k5 < 3 {
        let (no, op) = sample_script()[k5];
        let _ = s5.add_step(no, op);
        k5 += 1;
    }
    let j5 = s5.lock(b38_hash()).err() == Some(SL_SHORT);
    s.add("J5-短锁抓", j5, "三步不足锚即 SL_SHORT");
    // J6 注入：哈希不核锁拒。
    let mut s6 = ScriptLock::new();
    let mut k6 = 0;
    while k6 < sample_script().len() {
        let (no, op) = sample_script()[k6];
        let _ = s6.add_step(no, op);
        k6 += 1;
    }
    let j6 = s6.lock(b38_hash() + 1).err() == Some(SL_MISMATCH);
    s.add("J6-哈错抓", j6, "对表不核即 SL_MISMATCH");
    // J7 正路径：哈希核过终版锁定。
    let mut s7 = ScriptLock::new();
    let mut k7 = 0;
    while k7 < sample_script().len() {
        let (no, op) = sample_script()[k7];
        let _ = s7.add_step(no, op);
        k7 += 1;
    }
    let h7 = s7.lock(b38_hash());
    let j7 = h7.is_ok() && s7.locked && s7.locked_hash == Some(b38_hash());
    s.add("J7-锁定成", j7, "哈希核过 B38 终版锁定");
    // J8 注入：锁定后再落步拒。
    let mut s8 = ScriptLock::new();
    let mut k8 = 0;
    while k8 < sample_script().len() {
        let (no, op) = sample_script()[k8];
        let _ = s8.add_step(no, op);
        k8 += 1;
    }
    let _ = s8.lock(b38_hash());
    let j8 = s8.add_step(7, OP_WRAP).err() == Some(SL_LOCKED);
    s.add("J8-锁后落拒", j8, "锁定后落步即 SL_LOCKED");
    // J9 边界：重复锁拒。
    let mut s9 = s7.clone();
    let j9 = s9.lock(b38_hash()).err() == Some(SL_LOCKED);
    s.add("J9-重锁拒", j9, "二次锁定即 SL_LOCKED");
    // J10 动态：动作码序列与 B38 基准对表。
    let seq = sc.op_seq();
    let j10 = seq == vec![1, 2, 3, 4, 5, 6];
    s.add_dyn(
        "J10-动作对表",
        j10,
        &format!("动作码序列 {:?} 与 B38 会签稿同构", seq),
    );
    // J11 动态：哈希出数与指纹。
    let h = sc.script_hash();
    let j11 = h == b38_hash() && sc.fold() != 0;
    s.add_dyn(
        "J11-哈希出数",
        j11,
        &format!("脚本哈希 {:016x} 与 B38 基准一致，指纹 {:08x}", h, sc.fold()),
    );
    // J12 边界：超容拒。
    let mut s12 = ScriptLock::new();
    let mut n12 = 0;
    let mut full = true;
    while n12 < CAP {
        let op = n12 % 6 + 1;
        if s12.add_step((n12 + 1) as u32, op as u32).is_err() {
            full = false;
        }
        n12 += 1;
    }
    let j12 = full && s12.add_step(13, OP_RUN).err() == Some(SL_CAP);
    s.add("J12-超容拒", j12, "第十三步入账即 SL_CAP");
    // J13 边界：单步差异即哈希不同。
    let mut s13 = ScriptLock::new();
    let mut k13 = 0;
    while k13 < sample_script().len() {
        let (no, op) = sample_script()[k13];
        let op13 = if no == 4 { OP_SNAP + 1 } else { op };
        let _ = s13.add_step(no, op13);
        k13 += 1;
    }
    let j13 = !s13.matches_b38(b38_hash()) && s13.matches_b38(s13.script_hash());
    s.add("J13-异稿异哈", j13, "单步改码哈希即变");
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "跳步/坏码/短锁/哈错四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_lock() {
        let mut sc = ScriptLock::new();
        let mut k = 0;
        while k < sample_script().len() {
            let (no, op) = sample_script()[k];
            assert!(sc.add_step(no, op).is_ok());
            k += 1;
        }
        assert!(sc.long_enough());
        assert!(sc.matches_b38(b38_hash()));
        let h = sc.lock(b38_hash());
        assert!(h.is_ok());
        assert!(sc.add_step(7, OP_RUN).err() == Some(SL_LOCKED));
        assert!(sc.lock(b38_hash()).err() == Some(SL_LOCKED));
        assert!(sc.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10362_checks();
        assert!(s.all_passed());
    }
}
