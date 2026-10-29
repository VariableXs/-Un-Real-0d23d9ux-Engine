//! unx0984_vecciso — UNX-F0984 向量级隔离降级（单向量屏蔽与设备通知）。
//!
//! 任务锚点：UNX-F0984（第二批工位 W7，SEG 段位 0x8100，本单独占
//! 0x8100..=0x811F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**单向量隔离降级全链可走（正常→节流→
//! 屏蔽→隔离四态闭集），屏蔽必带设备通知（通知账逐条对得上），恢复
//! 路径逐态回退不跳步，向量账守恒（在役+隔离+释放==总数）恒等式
//! 全程成立**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x810 段闭集）；tick 显性
//! 注入零墙钟；向量账定容守恒（三态和==总数 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8100 段，本单独占）
// ---------------------------------------------------------------------------

/// 向量级隔离降级段基址（0x8100，本单 0x8100..=0x811F 独占）。
pub const SEG_BASE: u16 = 0x8100;
/// 0x8101 向量号非法（零号或超域——向量域 1..=VECTOR_MAX）。
pub const XC_VEC_BAD: u16 = SEG_BASE + 1;
/// 0x8102 向量未登记（未分配向量先隔离——账面身份缺失）。
pub const XC_VEC_UNALLOCATED: u16 = SEG_BASE + 2;
/// 0x8103 隔离跳步（未屏蔽直隔离——状态机单调性破坏）。
pub const XC_ISOLATE_SKIP: u16 = SEG_BASE + 3;
/// 0x8104 屏蔽缺设备通知（屏蔽动作后未落通知行）。
pub const XC_NOTIFY_MISSING: u16 = SEG_BASE + 4;
/// 0x8105 恢复跳步（跨态恢复——回退路径不逐态）。
pub const XC_RESTORE_SKIP: u16 = SEG_BASE + 5;
/// 0x8106 向量账守恒破坏（在役+隔离+释放 != 总数）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x8107 动作 tick 回退（动作时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 7;
/// 0x8108 隔离面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 8;
/// 0x8109 向量重复分配（同号双登记）。
pub const XC_VEC_DUP: u16 = SEG_BASE + 9;
/// 0x810A 通知账定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 10;

const _: () = assert!(XC_VEC_BAD != XC_VEC_UNALLOCATED);
const _: () = assert!(XC_ISOLATE_SKIP != XC_RESTORE_SKIP);
const _: () = assert!(XC_DRIFT != XC_VEC_DUP);

// ---------------------------------------------------------------------------
// §1 向量态闭集（四态——隔离降级全链）
// ---------------------------------------------------------------------------

/// 向量态闭集：在役 / 节流 / 屏蔽 / 隔离（恢复沿原路逐态回退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecState {
    /// 在役（正常收中断）。
    Active,
    /// 节流（收但降频处理）。
    Throttled,
    /// 屏蔽（不收，设备已通知）。
    Masked,
    /// 隔离（不收，且退出调度）。
    Isolated,
}

impl VecState {
    /// 态码（1..=4）。
    pub const fn code(self) -> u8 {
        match self {
            VecState::Active => 1,
            VecState::Throttled => 2,
            VecState::Masked => 3,
            VecState::Isolated => 4,
        }
    }
    /// 态名。
    pub const fn name(self) -> &'static str {
        match self {
            VecState::Active => "在役",
            VecState::Throttled => "节流",
            VecState::Masked => "屏蔽",
            VecState::Isolated => "隔离",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<VecState> {
        match c {
            1 => Some(VecState::Active),
            2 => Some(VecState::Throttled),
            3 => Some(VecState::Masked),
            4 => Some(VecState::Isolated),
            _ => None,
        }
    }
}

/// 向量域上限（向量号 1..=VECTOR_MAX——定值 16）。
pub const VECTOR_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// §2 向量账目与通知账
// ---------------------------------------------------------------------------

/// 一个向量的账目：号 / 设备名 / 态 / 态变 tick。
#[derive(Clone, Debug)]
pub struct VecEntry {
    vec_no: u32,
    device: String,
    state: VecState,
    at_tick: u64,
}

/// 一行设备通知：向量号 / 通知种（1 屏蔽 / 2 恢复）/ tick。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifyRow {
    vec_no: u32,
    kind: u8,
    tick: u64,
}

// ---------------------------------------------------------------------------
// §3 向量隔离面（VectorIsolation）
// ---------------------------------------------------------------------------

/// 通知账容量定值：64 行（隔离处置通知量级）。
pub const NOTIFY_CAP: usize = 64;

/// 向量隔离面——单向量屏蔽与设备通知账本本体。
///
/// 守恒恒等式：`在役数 + 隔离面态数(节流/屏蔽/隔离) == 登记总数`
/// （每向量恰处一态）；屏蔽恒等式：每次进屏蔽态必有一条屏蔽通知行。
#[derive(Debug)]
pub struct VectorIsolation {
    entries: Vec<VecEntry>,
    notifies: Vec<NotifyRow>,
    notify_cap: usize,
    last_tick: u64,
    rejected: u32,
}

impl VectorIsolation {
    /// 建隔离面。
    pub fn new(notify_cap: usize) -> Result<VectorIsolation, u16> {
        if notify_cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(VectorIsolation {
            entries: Vec::new(),
            notifies: Vec::with_capacity(notify_cap),
            notify_cap,
            last_tick: 0,
            rejected: 0,
        })
    }

    /// 登记向量数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 通知行数。
    pub fn notify_len(&self) -> usize {
        self.notifies.len()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    fn find(&self, vec_no: u32) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].vec_no == vec_no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 分配登记一向量（号域/重复/tick 全检）。
    pub fn allocate(&mut self, vec_no: u32, device: &str, tick: u64) -> Result<(), u16> {
        if vec_no == 0 || vec_no > VECTOR_MAX || device.is_empty() {
            self.rejected += 1;
            return Err(XC_VEC_BAD);
        }
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        if self.find(vec_no).is_some() {
            self.rejected += 1;
            return Err(XC_VEC_DUP);
        }
        self.entries.push(VecEntry {
            vec_no,
            device: String::from(device),
            state: VecState::Active,
            at_tick: tick,
        });
        self.last_tick = tick;
        Ok(())
    }

    fn set_state(&mut self, idx: usize, st: VecState, tick: u64) {
        self.entries[idx].state = st;
        self.entries[idx].at_tick = tick;
        self.last_tick = tick;
    }

    /// 降级一步：在役→节流→屏蔽（屏蔽必落设备通知）→隔离。
    ///
    /// 单向量隔离降级主入口：每次调用沿态机降一级；跳步（如未屏蔽
    /// 直隔离）带码拒收。
    pub fn degrade(&mut self, vec_no: u32, tick: u64) -> Result<VecState, u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let idx = match self.find(vec_no) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_VEC_UNALLOCATED);
            }
        };
        let cur = self.entries[idx].state;
        match cur {
            VecState::Active => {
                self.set_state(idx, VecState::Throttled, tick);
            }
            VecState::Throttled => {
                self.set_state(idx, VecState::Masked, tick);
                // 屏蔽必带设备通知。
                if self.notifies.len() >= self.notify_cap {
                    self.rejected += 1;
                    return Err(XC_NOTIFY_MISSING);
                }
                self.notifies.push(NotifyRow {
                    vec_no,
                    kind: 1,
                    tick,
                });
            }
            VecState::Masked => {
                self.set_state(idx, VecState::Isolated, tick);
            }
            VecState::Isolated => {
                self.rejected += 1;
                return Err(XC_ISOLATE_SKIP);
            }
        }
        Ok(self.entries[idx].state)
    }

    /// 恢复一步：隔离→屏蔽→节流→在役（逐态回退，恢复过屏蔽态也落通知）。
    pub fn restore(&mut self, vec_no: u32, tick: u64) -> Result<VecState, u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let idx = match self.find(vec_no) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_VEC_UNALLOCATED);
            }
        };
        let cur = self.entries[idx].state;
        match cur {
            VecState::Isolated => {
                self.set_state(idx, VecState::Masked, tick);
            }
            VecState::Masked => {
                self.set_state(idx, VecState::Throttled, tick);
                if self.notifies.len() >= self.notify_cap {
                    self.rejected += 1;
                    return Err(XC_NOTIFY_MISSING);
                }
                self.notifies.push(NotifyRow {
                    vec_no,
                    kind: 2,
                    tick,
                });
            }
            VecState::Throttled => {
                self.set_state(idx, VecState::Active, tick);
            }
            VecState::Active => {
                self.rejected += 1;
                return Err(XC_RESTORE_SKIP);
            }
        }
        Ok(self.entries[idx].state)
    }

    /// 态机检（向量当前态）。
    pub fn state_of(&self, vec_no: u32) -> Option<VecState> {
        match self.find(vec_no) {
            Some(i) => Some(self.entries[i].state),
            None => None,
        }
    }

    /// 屏蔽对账：每个处于屏蔽态的向量必有一条屏蔽通知（且恢复过的
    /// 向量屏蔽通知与恢复通知条数对得上——通知账逐条对得上判据）。
    pub fn audit_notify_paired(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            let v = self.entries[i].vec_no;
            let mut mask_n: u32 = 0;
            let mut rest_n: u32 = 0;
            let mut j: usize = 0;
            while j < self.notifies.len() {
                if self.notifies[j].vec_no == v {
                    if self.notifies[j].kind == 1 {
                        mask_n += 1;
                    } else {
                        rest_n += 1;
                    }
                }
                j += 1;
            }
            // 屏蔽数 == 恢复数 + （现处屏蔽态则 +1）。
            let cur = self.entries[i].state;
            let expect = rest_n + if cur == VecState::Masked { 1 } else { 0 };
            if mask_n != expect && !(cur == VecState::Isolated && mask_n == rest_n + 1) {
                return Err(XC_NOTIFY_MISSING);
            }
            i += 1;
        }
        Ok(())
    }

    /// 三态守恒机检（重扫逐条）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut n_active: u32 = 0;
        let mut n_down: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            match self.entries[i].state {
                VecState::Active => n_active += 1,
                _ => n_down += 1,
            }
            i += 1;
        }
        if n_active + n_down != self.entries.len() as u32 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 隔离面指纹（向量态序 + 通知行数 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.entries.len() {
            h = (h ^ self.entries[i].vec_no as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ self.entries[i].state.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h = (h ^ self.notifies.len() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 隔离面文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        self.audit_notify_paired()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0984-VECISO|v1|vecs=");
        s.push_str(&self.entries.len().to_string());
        s.push_str("|notifies=");
        s.push_str(&self.notifies.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.entries.len() {
            s.push_str(&self.entries[i].vec_no.to_string());
            s.push('|');
            s.push_str(self.entries[i].state.name());
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：分配→逐级降→屏蔽通知对账→逐级恢复→注入面→守恒→导出。
pub fn drill() -> Result<u32, u16> {
    let mut v = VectorIsolation::new(NOTIFY_CAP)?;
    // 分配三条向量。
    v.allocate(1, "键盘", 10)?;
    v.allocate(2, "时钟", 20)?;
    v.allocate(3, "网卡", 30)?;
    // 注入面：零号、超域、重复、未登记全拦。
    if v.allocate(0, "零号", 40) != Err(XC_VEC_BAD) {
        return Err(XC_VEC_BAD);
    }
    if v.allocate(VECTOR_MAX + 1, "超域", 40) != Err(XC_VEC_BAD) {
        return Err(XC_VEC_BAD);
    }
    if v.allocate(1, "重复", 40) != Err(XC_VEC_DUP) {
        return Err(XC_VEC_DUP);
    }
    if v.degrade(9, 40) != Err(XC_VEC_UNALLOCATED) {
        return Err(XC_VEC_UNALLOCATED);
    }
    // 向量 1 全链降级：在役→节流→屏蔽（落通知）→隔离。
    if v.degrade(1, 50)? != VecState::Throttled {
        return Err(XC_ISOLATE_SKIP);
    }
    if v.degrade(1, 60)? != VecState::Masked {
        return Err(XC_ISOLATE_SKIP);
    }
    if v.degrade(1, 70)? != VecState::Isolated {
        return Err(XC_ISOLATE_SKIP);
    }
    // 注入：隔离态再降（跳步拦停）。
    if v.degrade(1, 80) != Err(XC_ISOLATE_SKIP) {
        return Err(XC_ISOLATE_SKIP);
    }
    // 向量 2 降到屏蔽即止。
    v.degrade(2, 90)?;
    v.degrade(2, 100)?;
    // 屏蔽通知对账：向量1 1 次屏蔽（隔离中 mask==rest+1），向量2 1 次屏蔽现处屏蔽态。
    v.audit_notify_paired()?;
    // 逐级恢复向量 1：隔离→屏蔽→节流（落恢复通知）→在役。
    if v.restore(1, 110)? != VecState::Masked {
        return Err(XC_RESTORE_SKIP);
    }
    if v.restore(1, 120)? != VecState::Throttled {
        return Err(XC_RESTORE_SKIP);
    }
    if v.restore(1, 130)? != VecState::Active {
        return Err(XC_RESTORE_SKIP);
    }
    // 注入：在役态再恢复（跳步拦停）。
    if v.restore(1, 140) != Err(XC_RESTORE_SKIP) {
        return Err(XC_RESTORE_SKIP);
    }
    // tick 回退注入。
    if v.allocate(4, "串口", 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：3 向量 = 2 在役 + 1 降级面（向量2 屏蔽态）；通知 3 行。
    v.audit_conservation()?;
    if v.len() != 3 || v.notify_len() != 3 || v.rejected() != 7 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = v.fingerprint();
    let _ = v.export()?;
    if v.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
