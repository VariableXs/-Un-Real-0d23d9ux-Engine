//! UNX-F10931 SEM_UNDO 终止回滚矩阵（0x1d3e0 段）
//!
//! 纯功能模型：进程级 SEM_UNDO 账与终止回滚。
//! 进程带 UNDO 标志的每次成功 semop 都按 (信号量, 增减量) 记入进程账；
//! 进程终止时按账逐条反向冲销，集合值回到 UNDO 操作前的基线。
//! 反面保证：无 UNDO 标志的进程终止零回滚、零漂移。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d3e0;
/// 操作会使值落负
pub const SU_NEG: u32 = SEG_BASE + 0x01;
/// sem_num 越过集合界
pub const SU_EINVAL: u32 = SEG_BASE + 0x02;
/// 未知进程终止请求
pub const SU_NOPROC: u32 = SEG_BASE + 0x03;
/// 对已终止进程再次派工
pub const SU_DEAD: u32 = SEG_BASE + 0x04;
/// 空操作数组
pub const SU_EMPTY: u32 = SEG_BASE + 0x05;
/// 回滚后发现账目不平
pub const SU_LEDGER: u32 = SEG_BASE + 0x06;
/// 进程账容量溢出
pub const SU_UNDOMAX: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------- 常数
/// 单进程 UNDO 账上限
pub const UNDO_MAX: usize = 16;
/// 集合信号量个数
pub const UNDO_NSEMS: usize = 4;
/// 反面验收轮数
pub const PLAIN_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 进程 UNDO 账中的一条冲销记录。
#[derive(Clone, Debug, PartialEq)]
pub struct UndoEntry {
    /// 目标信号量下标
    pub sem_num: usize,
    /// 当次已生效的增减量（回滚时取反）
    pub delta: i32,
}

/// 进程镜像：账 + 存活位。
#[derive(Clone, Debug)]
pub struct ProcUndo {
    /// 进程号
    pub pid: u32,
    /// UNDO 账（按序累计）
    pub entries: Vec<UndoEntry>,
    /// 是否存活
    pub alive: bool,
}

impl ProcUndo {
    /// 建进程。
    pub fn new(pid: u32) -> Self {
        ProcUndo { pid, entries: Vec::new(), alive: true }
    }

    /// 账内指定信号量的累计净变动。
    pub fn net_of(&self, sem_num: usize) -> i64 {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].sem_num == sem_num {
                acc += self.entries[i].delta as i64;
            }
            i += 1;
        }
        acc
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "ProcUndo{{pid={},entries={},alive={}}}",
            self.pid, self.entries.len(), self.alive as u32
        )
    }
}

/// 回滚结果。
#[derive(Clone, Debug, PartialEq)]
pub struct RollbackReport {
    /// 被冲销的记录条数
    pub undone: usize,
    /// 回滚后集合值
    pub vals: Vec<i32>,
    /// 被终止的进程号
    pub pid: u32,
}

/// 回滚试验台：一个集合 + 一组进程账。
#[derive(Clone, Debug)]
pub struct UndoRig {
    /// 集合值
    pub vals: Vec<i32>,
    /// 进程账
    pub procs: Vec<ProcUndo>,
    /// 成功 semop 轮次
    pub ops_ok: usize,
    /// 已发生终止回滚次数
    pub rollbacks: usize,
    /// 累计冲销记录条数
    pub rollback_entries: usize,
}

impl UndoRig {
    /// 建试验台：nsems 个信号量统一初值。
    pub fn new(nsems: usize, init: i32) -> Self {
        let mut vals = Vec::new();
        let mut i = 0;
        while i < nsems {
            vals.push(init);
            i += 1;
        }
        UndoRig { vals, procs: Vec::new(), ops_ok: 0, rollbacks: 0, rollback_entries: 0 }
    }

    /// 进程查找。
    pub fn find(&self, pid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.procs.len() {
            if self.procs[i].pid == pid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 挂进程。
    pub fn attach(&mut self, pid: u32) -> bool {
        if self.find(pid).is_some() {
            return false;
        }
        self.procs.push(ProcUndo::new(pid));
        true
    }

    /// 派一次 semop：ops 为 (下标, 增减量, 是否 UNDO)。
    /// 判定序：进程在册且存活 → 非空 → 逐条越界 → 预演落负 → 记账提交。
    pub fn semop(&mut self, pid: u32, ops: &[(usize, i32, bool)], round: usize) -> Result<usize, u32> {
        let ix = match self.find(pid) {
            Some(ix) => ix,
            None => return Err(SU_NOPROC),
        };
        if !self.procs[ix].alive {
            return Err(SU_DEAD);
        }
        if ops.is_empty() {
            return Err(SU_EMPTY);
        }
        let mut k = 0;
        while k < ops.len() {
            if ops[k].0 >= self.vals.len() {
                return Err(SU_EINVAL);
            }
            k += 1;
        }
        let mut staged = self.vals.clone();
        let mut k = 0;
        while k < ops.len() {
            let after = staged[ops[k].0].wrapping_add(ops[k].1);
            if after < 0 {
                return Err(SU_NEG);
            }
            staged[ops[k].0] = after;
            k += 1;
        }
        // 记 UNDO 账（超限报账满，账满即整轮失败，保持原子）。
        let mut new_entries: Vec<UndoEntry> = Vec::new();
        let mut k = 0;
        while k < ops.len() {
            if ops[k].2 {
                new_entries.push(UndoEntry { sem_num: ops[k].0, delta: ops[k].1 });
            }
            k += 1;
        }
        if self.procs[ix].entries.len() + new_entries.len() > UNDO_MAX {
            return Err(SU_UNDOMAX);
        }
        self.vals = staged;
        let mut k = 0;
        while k < new_entries.len() {
            self.procs[ix].entries.push(new_entries[k].clone());
            k += 1;
        }
        self.ops_ok = self.ops_ok.wrapping_add(1);
        let _ = round;
        Ok(ops.len())
    }

    /// 终止进程并回滚：未知进程 → NOPROC；无账进程零冲销。
    pub fn terminate(&mut self, pid: u32) -> Result<RollbackReport, u32> {
        let ix = match self.find(pid) {
            Some(ix) => ix,
            None => return Err(SU_NOPROC),
        };
        if !self.procs[ix].alive {
            return Err(SU_DEAD);
        }
        let mut undone = 0;
        let mut k = 0;
        while k < self.procs[ix].entries.len() {
            let e = self.procs[ix].entries[k].clone();
            let back = self.vals[e.sem_num].wrapping_sub(e.delta);
            if back < 0 {
                return Err(SU_LEDGER);
            }
            self.vals[e.sem_num] = back;
            undone += 1;
            k += 1;
        }
        self.procs[ix].alive = false;
        self.procs[ix].entries.clear();
        self.rollbacks = self.rollbacks.wrapping_add(1);
        self.rollback_entries = self.rollback_entries.wrapping_add(undone);
        Ok(RollbackReport { undone, vals: self.vals.clone(), pid })
    }

    /// 全部值总和（对账用）。
    pub fn sum(&self) -> i64 {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < self.vals.len() {
            acc += self.vals[i] as i64;
            i += 1;
        }
        acc
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.vals.len() {
            h = (h ^ (self.vals[i] as i64 as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut i = 0;
        while i < self.procs.len() {
            h = (h ^ (self.procs[i].pid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.procs[i].entries.len() as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.procs[i].alive as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.ops_ok as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "UndoRig{{nsems={},procs={},ops_ok={},rollbacks={},entries={},sum={}}}",
            self.vals.len(), self.procs.len(), self.ops_ok,
            self.rollbacks, self.rollback_entries, self.sum()
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部按预期处置。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut rig = UndoRig::new(UNDO_NSEMS, 4);
    assert!(rig.attach(1));
    assert!(rig.attach(2));
    // 注入 1：UNDO 扣减后终止 → 值回到基线。
    let base = rig.vals.clone();
    let _ = rig.semop(1, &[(0, -2, true)], 1);
    let rep = rig.terminate(1);
    caught[0] = matches!(&rep, Ok(r) if r.undone == 1) && rig.vals == base;
    // 注入 2：混合（UNDO -1 / 非 UNDO -1）→ 只回滚 UNDO 部分。
    let base2 = rig.vals.clone();
    let _ = rig.semop(2, &[(1, -1, true), (1, -1, false)], 2);
    let rep2 = rig.terminate(2);
    caught[1] = matches!(&rep2, Ok(r) if r.undone == 1)
        && rig.vals[1] == base2[1].wrapping_sub(1);
    // 注入 3：无 UNDO 终止 → 零回滚零漂移（非 UNDO 变动合法保留，终止不得再改值）。
    assert!(rig.attach(3));
    let _ = rig.semop(3, &[(2, -1, false)], 3);
    let pre3 = rig.vals.clone();
    let rep3 = rig.terminate(3);
    caught[2] = matches!(&rep3, Ok(r) if r.undone == 0) && rig.vals == pre3;
    // 注入 4：终止未知进程 → NOPROC；对死进程派工 → DEAD。
    caught[3] = matches!(rig.terminate(99), Err(SU_NOPROC))
        && matches!(rig.semop(1, &[(0, 1, false)], 4), Err(SU_DEAD));
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮反面样本：普通进程（无 UNDO）反复扣补，终止后零漂移。
/// 返回（符合预期轮数，终止后值与基线是否一致）。
pub fn sample_plain_thousand(rig: &mut UndoRig) -> (usize, bool) {
    let base = rig.vals.clone();
    assert!(rig.attach(0x77));
    let mut good = 0;
    let mut r = 0;
    while r < PLAIN_ROUNDS {
        // 交替 -1/+1，全程非 UNDO；同一下标成对出现（-1 后紧跟 +1），
        // 否则单号被连续扣减会在中途枯竭而变误报。
        let op = if r % 2 == 0 { -1i32 } else { 1i32 };
        let num = (r / 2) % UNDO_NSEMS;
        let res = rig.semop(0x77, &[(num, op, false)], r);
        let ok = res.is_ok();
        if ok {
            good += 1;
        }
        r += 1;
    }
    let rep = rig.terminate(0x77);
    let stable = matches!(&rep, Ok(x) if x.undone == 0) && rig.vals == base;
    (good, stable)
}

// ---------------------------------------------------------------- 判据
/// UNX-F10931 判据组。
pub fn run_unx10931_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10931");
    // J1 正面：UNDO 操作后终止回滚，十轮全中。
    // 扣减量须留在初值 5 的余量内（-(1..5)），否则预演即落负、轮内零账可回滚。
    let mut restore_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut rig = UndoRig::new(UNDO_NSEMS, 5);
        assert!(rig.attach(0x11 + k as u32));
        let base = rig.vals.clone();
        let _ = rig.semop(0x11 + k as u32, &[(k % UNDO_NSEMS, -(1 + (k as i32 % 5)), true)], k);
        let rep = rig.terminate(0x11 + k as u32);
        if matches!(&rep, Ok(r) if r.undone == 1) && rig.vals == base {
            restore_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面回滚", restore_cnt == 10, "带 UNDO 终止后集合值十轮全部还原");
    // J2 正面补充：混合操作只回滚 UNDO 部分，十轮全中。
    let mut mix_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut rig = UndoRig::new(UNDO_NSEMS, 6);
        assert!(rig.attach(0x22));
        let base = rig.vals.clone();
        let num = k % UNDO_NSEMS;
        let _ = rig.semop(0x22, &[(num, -1, true), (num, -2, false)], k);
        let rep = rig.terminate(0x22);
        let mut expect = base.clone();
        expect[num] = expect[num].wrapping_sub(2);
        if matches!(&rep, Ok(r) if r.undone == 1) && rig.vals == expect {
            mix_cnt += 1;
        }
        k += 1;
    }
    s.add("J2-混合回滚", mix_cnt == 10, "混合操作只冲销 UNDO 记录，非 UNDO 变动保留");
    // J3 反面：无 UNDO 千轮零回滚零漂移。
    let mut rig3 = UndoRig::new(UNDO_NSEMS, 4);
    let (good, stable) = sample_plain_thousand(&mut rig3);
    s.add(
        "J3-反面零回滚",
        good == PLAIN_ROUNDS && stable && rig3.rollback_entries == 0,
        "无 UNDO 千轮后零冲销、值与基线逐位一致",
    );
    // J4 注入四路。
    let inj = inject_four();
    s.add(
        "J4-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "回滚/混合/零回滚/死进程四路全部按预期",
    );
    // J5 多进程账互不串扰：A 的终止不影响 B 的账。
    let mut rig5 = UndoRig::new(UNDO_NSEMS, 8);
    assert!(rig5.attach(1));
    assert!(rig5.attach(2));
    let _ = rig5.semop(1, &[(0, -1, true)], 1);
    let _ = rig5.semop(2, &[(1, -1, true)], 2);
    let _ = rig5.terminate(1);
    s.add(
        "J5-账本隔离",
        rig5.vals[0] == 8 && rig5.vals[1] == 7 && rig5.find(2).map(|ix| rig5.procs[ix].entries.len()) == Some(1),
        "A 回滚后 B 的 UNDO 账原样保留",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        SU_NEG > SEG_BASE && SU_LEDGER < SEG_BASE + 0x20,
        "七个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [SU_NEG, SU_EINVAL, SU_NOPROC, SU_DEAD, SU_EMPTY, SU_LEDGER, SU_UNDOMAX];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码互异", distinct, "七个错误码两两不同");
    // J8 账上限：超 UNDO_MAX 报账满且整轮失败。
    let mut rig8 = UndoRig::new(UNDO_NSEMS, 9);
    assert!(rig8.attach(5));
    let mut over: Vec<(usize, i32, bool)> = Vec::new();
    let mut i = 0;
    while i <= UNDO_MAX {
        over.push((0, 1, true));
        i += 1;
    }
    s.add(
        "J8-账上限",
        matches!(rig8.semop(5, &over, 1), Err(SU_UNDOMAX)) && rig8.vals[0] == 9,
        "UNDO 记录超限整轮拒绝且值不变",
    );
    // J9 fold 稳定。
    let f = rig3.fold();
    let clone = rig3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 原子性：预演落负整轮拒绝。
    let mut rig10 = UndoRig::new(UNDO_NSEMS, 1);
    assert!(rig10.attach(7));
    let before10 = rig10.vals.clone();
    let r10 = rig10.semop(7, &[(0, -2, true), (1, 1, false)], 1);
    s.add(
        "J10-原子预演",
        matches!(r10, Err(SU_NEG)) && rig10.vals == before10,
        "落负整轮拒绝且零残留",
    );
    // J11 渲染非空。
    let txt = rig3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("UndoRig"), "台级渲染含类型名与账目");
    // J12 死进程账已清。
    let dead_clean = rig3.find(0x77).map(|ix| rig3.procs[ix].entries.is_empty()) == Some(true);
    s.add("J12-死账清零", dead_clean, "终止后进程 UNDO 账清空");
    // J13 动态：千轮反面明细。
    s.add_dyn(
        "J13-千轮明细",
        good == PLAIN_ROUNDS && stable,
        &format!("good={} rollbacks={} sum={}", good, rig3.rollbacks, rig3.sum()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10931_semundo_rollback_flow() {
        // 流程：挂进程 → UNDO 扣减 → 终止回滚 → 值还原 → 无 UNDO 轮零漂移。
        let mut rig = UndoRig::new(UNDO_NSEMS, 6);
        assert!(rig.attach(1));
        let base = rig.vals.clone();
        assert_eq!(rig.semop(1, &[(0, -2, true), (1, -1, false)], 1), Ok(2));
        assert_eq!(rig.vals[0], 4);
        let rep = rig.terminate(1);
        assert!(matches!(&rep, Ok(r) if r.undone == 1));
        // 只有带 UNDO 标记的下标回滚；非 UNDO 的扣减是终态变更，不回冲。
        let mut want = base.clone();
        want[1] = want[1].wrapping_sub(1);
        assert!(rig.vals == want);
        let (good, stable) = sample_plain_thousand(&mut UndoRig::new(UNDO_NSEMS, 4));
        assert_eq!(good, PLAIN_ROUNDS);
        assert!(stable);
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10931_checks_green() {
        let s = run_unx10931_checks();
        assert!(s.all_passed(), "UNX-F10931 判据未全绿");
    }
}
