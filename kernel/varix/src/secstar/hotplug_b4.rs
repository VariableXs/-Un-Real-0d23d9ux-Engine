//! F184 热插拔体验 · 批次四深化（secstar · G-G-14）。
//!
//! 批次四功能面（与批次三互补：批次三管「单卷状态机与修复」，本批管
//! 「多卷花名册与弹出治理」）：
//! - [`VolumeRoster`]：多卷花名册——8 卷状态一览 + 脏卷定位
//!   （谁的图标亮绿灯、谁的在转圈——管理页的数据面）；
//! - [`EjectQueue`]：弹出请求队列——同卷重复请求合并、持锁拒绝
//!   （连点弹出键不产生双份请求——防抖纪律的弹出面）；
//! - [`HolderPolicy`]：占用者策略——有 holder 拒弹出 + 强制选项二段
//!   （「正在使用中」不是死胡同：给路但不撒谎）；
//! - [`DamagedLedger`]：损伤账——file_ids 批注 + 修复状态追踪
//!   （脏拔后的文件级后果有账可查）。
//!
//! 零堆纪律：定长花名册 + 定长队列，无 alloc。

use super::hotplug::{HOLDER_CAP, VolState};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 多卷花名册
// ---------------------------------------------------------------------------

/// 卷数上限（与主层 HOLDER_CAP 同尺的卷面容量）。
pub const ROSTER_CAP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterState {
    Healthy,
    Busy,
    Flushing,
    Dirty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RosterEntry {
    pub drive: u8,
    pub state: RosterState,
}

pub struct VolumeRoster {
    entries: [Option<RosterEntry>; ROSTER_CAP],
    pub n: usize,
}

impl VolumeRoster {
    pub const fn new() -> VolumeRoster {
        VolumeRoster { entries: [const { None }; ROSTER_CAP], n: 0 }
    }

    /// 登记卷（盘符唯一）。
    pub fn add(&mut self, drive: u8) -> bool {
        if self.n >= ROSTER_CAP || self.pos(drive).is_some() {
            return false;
        }
        self.entries[self.n] = Some(RosterEntry { drive, state: RosterState::Healthy });
        self.n += 1;
        true
    }

    fn pos(&self, drive: u8) -> Option<usize> {
        self.entries[..self.n].iter().position(|e| matches!(e, Some(x) if x.drive == drive))
    }

    /// 状态迁移（花名册侧的轻状态机：四态封闭集）。
    pub fn set_state(&mut self, drive: u8, s: RosterState) -> bool {
        match self.pos(drive) {
            Some(i) => {
                self.entries[i].as_mut().unwrap().state = s;
                true
            }
            None => false,
        }
    }

    pub fn state_of(&self, drive: u8) -> Option<RosterState> {
        self.pos(drive).map(|i| self.entries[i].unwrap().state)
    }

    /// 脏卷定位：全部 Dirty 盘符（修复路径的清单来源）。
    pub fn dirty_drives(&self, out: &mut [u8; ROSTER_CAP]) -> usize {
        let mut k = 0;
        for e in self.entries[..self.n].iter().flatten() {
            if e.state == RosterState::Dirty && k < ROSTER_CAP {
                out[k] = e.drive;
                k += 1;
            }
        }
        k
    }

    /// 移除卷（拔出后出册）。
    pub fn remove(&mut self, drive: u8) -> bool {
        match self.pos(drive) {
            Some(i) => {
                for j in i..self.n - 1 {
                    self.entries[j] = self.entries[j + 1];
                }
                self.entries[self.n - 1] = None;
                self.n -= 1;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 弹出请求队列
// ---------------------------------------------------------------------------

/// 队列容量。
pub const EJECT_QUEUE_CAP: usize = 4;

pub struct EjectQueue {
    pending: [Option<u8>; EJECT_QUEUE_CAP],
    pub n: usize,
    pub merged: u32,
    pub rejected_busy: u32,
}

impl EjectQueue {
    pub const fn new() -> EjectQueue {
        EjectQueue { pending: [const { None }; EJECT_QUEUE_CAP], n: 0, merged: 0, rejected_busy: 0 }
    }

    /// 请求弹出：同卷已在队 = 合并计数（不产生双份请求）；
    /// 队满 = 诚实拒（rejected_busy 计数——防抖与容量两面）。
    pub fn request(&mut self, drive: u8) -> bool {
        if self.pending[..self.n].contains(&Some(drive)) {
            self.merged += 1;
            return false;
        }
        if self.n >= EJECT_QUEUE_CAP {
            self.rejected_busy += 1;
            return false;
        }
        self.pending[self.n] = Some(drive);
        self.n += 1;
        true
    }

    /// 取队首（FIFO——先到先弹）。
    pub fn pop(&mut self) -> Option<u8> {
        if self.n == 0 {
            return None;
        }
        let v = self.pending[0];
        for i in 0..self.n - 1 {
            self.pending[i] = self.pending[i + 1];
        }
        self.pending[self.n - 1] = None;
        self.n -= 1;
        v
    }
}

// ---------------------------------------------------------------------------
// 占用者策略
// ---------------------------------------------------------------------------

/// 占用裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HolderVerdict {
    /// 无占用 → 直接弹。
    Free,
    /// 有占用 → 拒 + 列占用者（给路但不撒谎）。
    Held,
}

/// 占用裁决：holders >0 → Held；HolderCap 边界内计数。
pub fn holder_verdict(holder_count: usize) -> HolderVerdict {
    if holder_count == 0 {
        HolderVerdict::Free
    } else {
        HolderVerdict::Held
    }
}

/// 强制弹出需二段确认（强制 = 承认脏后果—— DirtyWithMarker 语义联动）。
pub fn force_eject_needs_confirm(force_pressed: bool, already_confirmed: bool) -> bool {
    force_pressed && !already_confirmed
}

// ---------------------------------------------------------------------------
// 损伤账
// ---------------------------------------------------------------------------

/// 损伤文件账容量。
pub const DAMAGED_LEDGER_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageState {
    Detected,
    Repaired,
    Lost,
}

#[derive(Clone, Copy, Debug)]
pub struct DamageRec {
    pub file_id: u32,
    pub state: DamageState,
}

pub struct DamagedLedger {
    recs: [Option<DamageRec>; DAMAGED_LEDGER_CAP],
    pub n: usize,
}

impl DamagedLedger {
    pub const fn new() -> DamagedLedger {
        DamagedLedger { recs: [const { None }; DAMAGED_LEDGER_CAP], n: 0 }
    }

    /// 批注损伤文件（file_id 唯一——重复检出即修复状态更新走 update）。
    pub fn note(&mut self, file_id: u32) -> bool {
        if self.n >= DAMAGED_LEDGER_CAP || self.find(file_id).is_some() {
            return false;
        }
        self.recs[self.n] = Some(DamageRec { file_id, state: DamageState::Detected });
        self.n += 1;
        true
    }

    pub fn find(&self, file_id: u32) -> Option<DamageState> {
        self.recs[..self.n].iter().flatten().find(|r| r.file_id == file_id).map(|r| r.state)
    }

    /// 修复状态推进（Detected→Repaired 或 Lost——两出口都有名）。
    pub fn update(&mut self, file_id: u32, to: DamageState) -> bool {
        for r in self.recs[..self.n].iter_mut().flatten() {
            if r.file_id == file_id {
                r.state = to;
                return true;
            }
        }
        false
    }

    pub fn count_state(&self, s: DamageState) -> usize {
        self.recs[..self.n].iter().flatten().filter(|r| r.state == s).count()
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_hotplug_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F184-b4");

    // 1) 花名册：登记/查重/移除（多卷管理三面）。
    let mut r = VolumeRoster::new();
    let a = r.add(b'E');
    let dup = r.add(b'E');
    let b = r.add(b'F');
    let removed = r.remove(b'E');
    cs.add(
        "roster_crud",
        a && !dup && b && removed && r.n == 1 && r.state_of(b'F') == Some(RosterState::Healthy),
        "",
    );

    // 2) 花名册状态迁移：Healthy→Flushing→Dirty（轻状态机封闭集）。
    r.set_state(b'F', RosterState::Flushing);
    r.set_state(b'F', RosterState::Dirty);
    cs.add("roster_states", r.state_of(b'F') == Some(RosterState::Dirty), "");

    // 3) 脏卷定位：两脏一好 → 恰好两盘符出列（修复清单来源）。
    let mut r2 = VolumeRoster::new();
    for d in [b'C', b'D', b'E'] {
        r2.add(d);
    }
    r2.set_state(b'D', RosterState::Dirty);
    r2.set_state(b'E', RosterState::Dirty);
    let mut out = [0u8; ROSTER_CAP];
    let k = r2.dirty_drives(&mut out);
    cs.add("roster_dirty_located", k == 2 && out[0] == b'D' && out[1] == b'E', "");

    // 4) 弹出队列合并：同卷连点两次 → 第二次合并计数（防抖面）。
    let mut q = EjectQueue::new();
    let first = q.request(b'E');
    let second = q.request(b'E');
    cs.add("eject_merge", first && !second && q.merged == 1 && q.n == 1, "");

    // 5) 弹出队列 FIFO：E/F/G 依次请求 → E 先出（先到先弹）。
    q.request(b'F');
    q.request(b'G');
    let p1 = q.pop();
    let p2 = q.pop();
    cs.add("eject_fifo", p1 == Some(b'E') && p2 == Some(b'F'), "");

    // 6) 弹出队列满容诚实拒：4 满后第 5 卷拒（容量面）。
    let mut q2 = EjectQueue::new();
    for d in [b'C', b'D', b'E', b'F'] {
        q2.request(d);
    }
    let over = q2.request(b'G');
    cs.add("eject_queue_cap", !over && q2.rejected_busy == 1 && q2.n == EJECT_QUEUE_CAP, "");

    // 7) 占用裁决：零占用 Free、有占用 Held（给路但不撒谎）。
    cs.add(
        "holder_verdict",
        holder_verdict(0) == HolderVerdict::Free && holder_verdict(1) == HolderVerdict::Held && holder_verdict(HOLDER_CAP) == HolderVerdict::Held,
        "",
    );

    // 8) 强制弹出一二段确认：首按需确认、确认后不需（防手滑两面）。
    cs.add(
        "force_eject_two_step",
        force_eject_needs_confirm(true, false) && !force_eject_needs_confirm(true, true) && !force_eject_needs_confirm(false, false),
        "",
    );

    // 9) 损伤账：批注/主键去重/状态推进（文件级后果有账）。
    let mut l = DamagedLedger::new();
    let n1 = l.note(42);
    let dup = l.note(42);
    let upd = l.update(42, DamageState::Repaired);
    cs.add(
        "damaged_ledger_crud",
        n1 && !dup && upd && l.find(42) == Some(DamageState::Repaired) && l.count_state(DamageState::Repaired) == 1,
        "",
    );

    // 10) 损伤两出口：Repaired 与 Lost 分列（丢就是丢——诚实）。
    l.note(43);
    l.update(43, DamageState::Lost);
    cs.add(
        "damaged_two_outcomes",
        l.count_state(DamageState::Repaired) == 1 && l.count_state(DamageState::Lost) == 1,
        "",
    );

    // 11) 主册常量贯通：占用者 8 上限 / 卷状态枚举面复用一处一事实。
    cs.add(
        "consts_aligned",
        HOLDER_CAP == 8 && VolState::SafeToEject != VolState::Removed,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn roster_eight_drives_full() {
        // 8 卷满册、第 9 拒（管理页容量边界）。
        let mut r = VolumeRoster::new();
        for d in b'C'..b'K' {
            assert!(r.add(d), "drive {}", d as char);
        }
        assert!(!r.add(b'K'));
        assert_eq!(r.n, ROSTER_CAP);
    }

    #[test]
    fn eject_queue_drains_completely() {
        // 队列清空后复用（FIFO 收尾 + 再入队）。
        let mut q = EjectQueue::new();
        q.request(b'E');
        q.request(b'F');
        assert_eq!(q.pop(), Some(b'E'));
        assert_eq!(q.pop(), Some(b'F'));
        assert_eq!(q.pop(), None);
        assert!(q.request(b'G'));
        assert_eq!(q.pop(), Some(b'G'));
    }

    #[test]
    fn damaged_ledger_cap_and_unknown_update() {
        // 16 满容拒 + 未批注文件更新拒（账面边界两测）。
        let mut l = DamagedLedger::new();
        for fid in 0..DAMAGED_LEDGER_CAP as u32 {
            assert!(l.note(fid));
        }
        assert!(!l.note(999));
        assert!(!l.update(999, DamageState::Lost));
        assert_eq!(l.count_state(DamageState::Detected), DAMAGED_LEDGER_CAP);
    }
}
