//! UNX-F11069 SysV IPC 回归段（0x1e8a0 段）
//!
//! B13/B14/B27 判据在此重放：shm/sem/msg 三类对象共用 key 命名空间，
//! 每个对象登记 (key, id, 尺寸/信号量数, attach 数/字节水位, 生命周期)。
//! shm 走 Live → Marked → Destroyed，最后一个 attach 离开时回收；sem
//! 的 semop 受 [0, 32767] 夹取并记 undo；msg 队列按字节水位收发。
//!
//! 不变式：destroyed 段 attach 必为 0；msg 队列 bytes == 消息长度之和；
//! key 三表全局唯一；冻结后 alloc/op 全以 SV_FROZEN 拒止。
//!
//! 结构：ShmSeg（共享段）/ SemSet（信号量集）/ MsgQueue（消息队列）/
//! SysvLedger（三批总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11069 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e8a0;

/// 空账：key/尺寸为 0、空消息队列或查无对象
pub const SV_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：尺寸超窗、信号量下标越界、semval 出界
pub const SV_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：key 或对象 id 重复
pub const SV_DUP: u32 = SEG_BASE + 0x03;
/// 失配：生命周期相位冲突或 undo 回放不一致
pub const SV_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：attach 表满、消息水位不足或重放席位不足
pub const SV_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的分配/操作
pub const SV_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x69 标记 11069 号卡
pub const SV_TAG: u64 = 0x1e80_69;

/// 本段固定十席（4 shm + 3 sem + 3 msg）
const SEATS: usize = 10;
/// 单段尺寸上限（1 MiB）
const SHM_MAX: u32 = 1_048_576;
/// 单段 attach 上限
const ATTACH_CAP: u16 = 64;
/// 单信号量集容量
const SEM_MAX: usize = 32;
/// semval 上界
const SEMVAL_MAX: i32 = 32_767;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 生命周期闭集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcLife {
    /// 在用
    Live,
    /// 已 IPC_RMID、等待最后一个 attach 离开
    Marked,
    /// 已回收
    Destroyed,
}

impl IpcLife {
    /// 落账判别码
    pub fn code(self) -> u8 {
        match self {
            IpcLife::Live => 0,
            IpcLife::Marked => 1,
            IpcLife::Destroyed => 2,
        }
    }
}

/// 共享内存段
#[derive(Clone, Debug)]
pub struct ShmSeg {
    /// 全局 key
    pub key: u32,
    /// 对象 id
    pub id: u16,
    /// 段尺寸
    pub size: u32,
    /// 当前 attach 数
    pub attaches: u16,
    /// 生命周期
    pub life: IpcLife,
}

impl ShmSeg {
    /// 新建 Live 段
    pub fn new(key: u32, id: u16, size: u32) -> ShmSeg {
        ShmSeg { key, id, size, attaches: 0, life: IpcLife::Live }
    }

    /// attach：非 Live 拒、表满拒
    pub fn attach(&mut self) -> Result<u16, u32> {
        if self.life != IpcLife::Live {
            return Err(SV_MISMATCH);
        }
        if self.attaches >= ATTACH_CAP {
            return Err(SV_SHORT);
        }
        self.attaches = self.attaches.wrapping_add(1);
        Ok(self.attaches)
    }

    /// detach：无 attach 可离
    pub fn detach(&mut self) -> Result<u16, u32> {
        if self.attaches == 0 {
            return Err(SV_EMPTY);
        }
        self.attaches = self.attaches.wrapping_sub(1);
        Ok(self.attaches)
    }

    /// 打 IPC_RMID 标记
    pub fn mark(&mut self) -> Result<(), u32> {
        if self.life != IpcLife::Live {
            return Err(SV_DUP);
        }
        self.life = IpcLife::Marked;
        Ok(())
    }

    /// 若已标记且无人 attach 则回收
    pub fn gc(&mut self) -> bool {
        if self.life == IpcLife::Marked && self.attaches == 0 {
            self.life = IpcLife::Destroyed;
            return true;
        }
        false
    }

    /// 段指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u32(h, self.key);
        h = fnv_u16(h, self.id);
        h = fnv_u32(h, self.size);
        h = fnv_u16(h, self.attaches);
        fnv_byte(h, self.life.code())
    }
}

/// 信号量集
#[derive(Clone, Debug)]
pub struct SemSet {
    /// 全局 key
    pub key: u32,
    /// 对象 id
    pub id: u16,
    /// 各信号量当前值
    pub vals: Vec<i16>,
    /// 各信号量 undo 增量
    pub undo: Vec<i16>,
    /// 等待者数
    pub waiters: u16,
}

impl SemSet {
    /// 建集：0 个或超 32 个信号量拒
    pub fn new(key: u32, id: u16, nsems: usize) -> Result<SemSet, u32> {
        if nsems == 0 {
            return Err(SV_EMPTY);
        }
        if nsems > SEM_MAX {
            return Err(SV_RANGE);
        }
        let mut vals = Vec::new();
        let mut undo = Vec::new();
        let mut i = 0usize;
        while i < nsems {
            vals.push(0i16);
            undo.push(0i16);
            i += 1;
        }
        Ok(SemSet { key, id, vals, undo, waiters: 0 })
    }

    /// semop：下标越界或结果出界拒，成功后记 undo
    pub fn semop(&mut self, idx: usize, delta: i16) -> Result<i16, u32> {
        if idx >= self.vals.len() {
            return Err(SV_RANGE);
        }
        let next = (self.vals[idx] as i32).wrapping_add(delta as i32);
        if next < 0 || next > SEMVAL_MAX {
            return Err(SV_RANGE);
        }
        self.vals[idx] = next as i16;
        self.undo[idx] = self.undo[idx].wrapping_sub(delta);
        Ok(next as i16)
    }

    /// 集中回放 undo，恢复 semadj
    pub fn undo_all(&mut self) -> Result<u16, u32> {
        let mut n = 0u16;
        let mut k = 0usize;
        while k < self.vals.len() {
            let restored = (self.vals[k] as i32).wrapping_add(self.undo[k] as i32);
            if restored < 0 || restored > SEMVAL_MAX {
                return Err(SV_MISMATCH);
            }
            self.vals[k] = restored as i16;
            self.undo[k] = 0;
            n = n.wrapping_add(1);
            k += 1;
        }
        Ok(n)
    }

    /// 集指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u32(h, self.key);
        h = fnv_u16(h, self.id);
        let mut k = 0usize;
        while k < self.vals.len() {
            h = fnv_u16(h, self.vals[k] as u16);
            h = fnv_u16(h, self.undo[k] as u16);
            k += 1;
        }
        fnv_u16(h, self.waiters)
    }
}

/// 消息队列
#[derive(Clone, Debug)]
pub struct MsgQueue {
    /// 全局 key
    pub key: u32,
    /// 对象 id
    pub id: u16,
    /// 在队消息长度
    pub msgs: Vec<u16>,
    /// 当前字节水位
    pub bytes: u32,
    /// 字节上限
    pub max_bytes: u32,
}

impl MsgQueue {
    /// 建队列
    pub fn new(key: u32, id: u16, max_bytes: u32) -> MsgQueue {
        MsgQueue { key, id, msgs: Vec::new(), bytes: 0, max_bytes }
    }

    /// 入队：零长/超水位拒
    pub fn send(&mut self, len: u16) -> Result<u32, u32> {
        if len == 0 {
            return Err(SV_EMPTY);
        }
        let next = self.bytes.wrapping_add(len as u32);
        if next > self.max_bytes {
            return Err(SV_SHORT);
        }
        self.msgs.push(len);
        self.bytes = next;
        Ok(next)
    }

    /// 出队最早一条：空队拒
    pub fn recv(&mut self) -> Result<u16, u32> {
        if self.msgs.is_empty() {
            return Err(SV_EMPTY);
        }
        let len = self.msgs.remove(0);
        self.bytes = self.bytes.wrapping_sub(len as u32);
        Ok(len)
    }

    /// 水位与消息长度之和一致
    pub fn consistent(&self) -> bool {
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < self.msgs.len() {
            sum = sum.wrapping_add(self.msgs[k] as u32);
            k += 1;
        }
        sum == self.bytes
    }

    /// 队列指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u32(h, self.key);
        h = fnv_u16(h, self.id);
        h = fnv_u32(h, self.bytes);
        h = fnv_u32(h, self.max_bytes);
        let mut k = 0usize;
        while k < self.msgs.len() {
            h = fnv_u16(h, self.msgs[k]);
            k += 1;
        }
        h
    }
}

/// B13/B14/B27 三表总账
#[derive(Clone, Debug)]
pub struct SysvLedger {
    /// 共享段表
    pub shm: Vec<ShmSeg>,
    /// 信号量集表
    pub sems: Vec<SemSet>,
    /// 消息队列表
    pub msgs: Vec<MsgQueue>,
    /// 下一个对象 id
    pub next_id: u16,
    /// 最近一次重放合法对象数
    pub verified: u16,
    frozen: Option<u64>,
}

impl SysvLedger {
    /// 空总账
    pub fn new() -> SysvLedger {
        SysvLedger {
            shm: Vec::new(),
            sems: Vec::new(),
            msgs: Vec::new(),
            next_id: 1,
            verified: 0,
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(SV_FROZEN);
        }
        Ok(())
    }

    fn take_id(&mut self) -> u16 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// key 全局唯一检查
    fn key_used(&self, key: u32) -> bool {
        let mut k = 0usize;
        while k < self.shm.len() {
            if self.shm[k].key == key {
                return true;
            }
            k += 1;
        }
        k = 0;
        while k < self.sems.len() {
            if self.sems[k].key == key {
                return true;
            }
            k += 1;
        }
        k = 0;
        while k < self.msgs.len() {
            if self.msgs[k].key == key {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 分配共享段
    pub fn alloc_shm(&mut self, key: u32, size: u32) -> Result<u16, u32> {
        self.guard()?;
        if key == 0 || size == 0 {
            return Err(SV_EMPTY);
        }
        if size > SHM_MAX {
            return Err(SV_RANGE);
        }
        if self.key_used(key) {
            return Err(SV_DUP);
        }
        let id = self.take_id();
        self.shm.push(ShmSeg::new(key, id, size));
        Ok(id)
    }

    /// 分配信号量集
    pub fn alloc_sem(&mut self, key: u32, nsems: usize) -> Result<u16, u32> {
        self.guard()?;
        if key == 0 {
            return Err(SV_EMPTY);
        }
        if self.key_used(key) {
            return Err(SV_DUP);
        }
        let id = self.take_id();
        let set = SemSet::new(key, id, nsems)?;
        self.sems.push(set);
        Ok(id)
    }

    /// 分配消息队列
    pub fn alloc_msg(&mut self, key: u32, max_bytes: u32) -> Result<u16, u32> {
        self.guard()?;
        if key == 0 {
            return Err(SV_EMPTY);
        }
        if max_bytes == 0 {
            return Err(SV_RANGE);
        }
        if self.key_used(key) {
            return Err(SV_DUP);
        }
        let id = self.take_id();
        self.msgs.push(MsgQueue::new(key, id, max_bytes));
        Ok(id)
    }

    fn shm_index(&self, id: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.shm.len() {
            if self.shm[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn sem_index(&self, id: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.sems.len() {
            if self.sems[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn msg_index(&self, id: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.msgs.len() {
            if self.msgs[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// attach 共享段
    pub fn shm_attach(&mut self, id: u16) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.shm_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.shm[idx].attach()
    }

    /// detach 共享段
    pub fn shm_detach(&mut self, id: u16) -> Result<u16, u32> {
        self.guard()?;
        let idx = match self.shm_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.shm[idx].detach()
    }

    /// IPC_RMID：标记并尝试立即回收
    pub fn shm_remove(&mut self, id: u16) -> Result<bool, u32> {
        self.guard()?;
        let idx = match self.shm_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.shm[idx].mark()?;
        Ok(self.shm[idx].gc())
    }

    /// 信号量操作
    pub fn semop(&mut self, id: u16, idx: usize, delta: i16) -> Result<i16, u32> {
        self.guard()?;
        let at = match self.sem_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.sems[at].semop(idx, delta)
    }

    /// 进程退出回放 undo
    pub fn sem_undo(&mut self, id: u16) -> Result<u16, u32> {
        self.guard()?;
        let at = match self.sem_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.sems[at].undo_all()
    }

    /// 消息入队
    pub fn msg_send(&mut self, id: u16, len: u16) -> Result<u32, u32> {
        self.guard()?;
        let at = match self.msg_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.msgs[at].send(len)
    }

    /// 消息出队
    pub fn msg_recv(&mut self, id: u16) -> Result<u16, u32> {
        self.guard()?;
        let at = match self.msg_index(id) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        self.msgs[at].recv()
    }

    /// 生命周期一致：destroyed 段无 attach，key 全局唯一
    pub fn lifecycle_consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.shm.len() {
            let seg = &self.shm[k];
            if seg.life == IpcLife::Destroyed && seg.attaches != 0 {
                return false;
            }
            let mut j = k.wrapping_add(1);
            while j < self.shm.len() {
                if self.shm[j].key == seg.key {
                    return false;
                }
                j += 1;
            }
            k += 1;
        }
        k = 0;
        while k < self.sems.len() {
            if self.sems[k].vals.len() != self.sems[k].undo.len() {
                return false;
            }
            k += 1;
        }
        k = 0;
        while k < self.msgs.len() {
            if !self.msgs[k].consistent() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 三表对象总数
    pub fn object_count(&self) -> usize {
        self.shm.len().wrapping_add(self.sems.len()).wrapping_add(self.msgs.len())
    }

    /// 重放：不足十席 SV_SHORT，任一对象非法 SV_MISMATCH
    pub fn verify(&mut self) -> Result<u16, u32> {
        self.guard()?;
        let total = self.object_count();
        if total < SEATS {
            return Err(SV_SHORT);
        }
        let mut ok = 0u16;
        let mut k = 0usize;
        while k < self.shm.len() {
            let seg = &self.shm[k];
            if seg.life != IpcLife::Destroyed || seg.attaches == 0 {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        k = 0;
        while k < self.sems.len() {
            if !self.sems[k].vals.is_empty() {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        k = 0;
        while k < self.msgs.len() {
            if self.msgs[k].consistent() {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        self.verified = ok;
        if ok as usize != total || !self.lifecycle_consistent() {
            return Err(SV_MISMATCH);
        }
        Ok(ok)
    }

    /// 总账指纹
    pub fn fold(&self) -> u64 {
        let mut h = SV_TAG;
        h = fnv_u16(h, self.next_id);
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.shm.len() {
            h = self.shm[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.sems.len() {
            h = self.sems[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.msgs.len() {
            h = self.msgs[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零 key / 超尺寸 / 重 key / 已标记段 attach
pub fn inject_four() -> [bool; 4] {
    let mut l0 = SysvLedger::new();
    let empty = matches!(l0.alloc_shm(0, 4096), Err(SV_EMPTY));
    let mut l1 = SysvLedger::new();
    let range = matches!(l1.alloc_shm(0x11, SHM_MAX.wrapping_add(1)), Err(SV_RANGE));
    let mut l2 = SysvLedger::new();
    let first = l2.alloc_shm(0x22, 4096).is_ok();
    let dup = first && matches!(l2.alloc_sem(0x22, 2), Err(SV_DUP));
    let mut l3 = SysvLedger::new();
    let seeded = match l3.alloc_shm(0x33, 4096) {
        Ok(id) => l3.shm_remove(id).is_ok(),
        Err(_) => false,
    };
    let mismatch = seeded
        && match l3.shm.first() {
            Some(seg) => matches!(l3.shm_attach(seg.id), Err(SV_MISMATCH)),
            None => false,
        };
    [empty, range, dup, mismatch]
}

/// 构造三表样本（4 shm + 3 sem + 3 msg）
fn sample_ledger() -> Result<SysvLedger, u32> {
    let mut led = SysvLedger::new();
    let s1 = led.alloc_shm(0x101, 4096)?;
    let s2 = led.alloc_shm(0x102, 8192)?;
    let s3 = led.alloc_shm(0x103, 16_384)?;
    let s4 = led.alloc_shm(0x104, 4096)?;
    led.shm_attach(s1)?;
    led.shm_attach(s2)?;
    led.shm_attach(s3)?;
    led.shm_attach(s4)?;
    led.shm_detach(s3)?;
    let pending = led.shm_remove(s4)?;
    if pending {
        return Err(SV_MISMATCH);
    }
    let dropped = led.shm_remove(s3)?;
    if !dropped {
        return Err(SV_MISMATCH);
    }
    let e1 = led.alloc_sem(0x201, 4)?;
    let e2 = led.alloc_sem(0x202, 2)?;
    led.alloc_sem(0x203, 1)?;
    led.semop(e1, 0, 10)?;
    led.semop(e1, 0, -4)?;
    led.sem_undo(e1)?;
    led.semop(e2, 0, 5)?;
    let q1 = led.alloc_msg(0x301, 1024)?;
    let q2 = led.alloc_msg(0x302, 2048)?;
    led.alloc_msg(0x303, 512)?;
    led.msg_send(q1, 100)?;
    led.msg_send(q2, 200)?;
    led.msg_send(q2, 50)?;
    led.msg_recv(q2)?;
    Ok(led)
}

/// UNX-F11069 判据入口
pub fn run_unx11069_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11069");
    let mut led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => SysvLedger::new(),
    };
    let verified = led.verify().unwrap_or(0);
    s.add(
        "J1a-SysV判据全量重放",
        verified == 10 && led.object_count() == SEATS,
        "B13/B14/B27 十对象必须全部合法",
    );
    s.add(
        "J1b-生命周期一致",
        led.lifecycle_consistent() && led.shm.len() == 4,
        "destroyed 段无 attach、key 全局唯一",
    );
    let msg_sum = {
        let q = &led.msgs[1];
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < q.msgs.len() {
            sum = sum.wrapping_add(q.msgs[k] as u32);
            k += 1;
        }
        sum == q.bytes
    };
    s.add(
        "J2-水位守恒",
        led.msgs[0].consistent() && led.msgs[2].consistent() && msg_sum,
        "三队列 bytes 必须等于消息长度之和",
    );
    let inj = inject_four();
    s.add("J3a-零key拒止", inj[0], "key=0 分配必须 SV_EMPTY");
    s.add("J3b-超尺寸拒止", inj[1], "1MiB+1 段必须 SV_RANGE");
    s.add("J3c-重key拒止", inj[2], "同 key 跨表分配必须 SV_DUP");
    s.add("J3d-已标记attach拒", inj[3], "Marked 段 attach 必须 SV_MISMATCH");
    let mut fz = led.clone();
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.alloc_shm(0x999, 4096), Err(SV_FROZEN))
        && matches!(fz.semop(1, 0, 1), Err(SV_FROZEN))
        && matches!(fz.msg_send(1, 8), Err(SV_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后分配/semop/入队全拒且指纹不变",
    );
    let fp_a = led.clone().fold();
    let mut b = led.clone();
    let sens = b.semop(6, 0, 3).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "单信号量 +3 必须改变 fold");
    let underflow = {
        let mut g = SysvLedger::new();
        match g.alloc_sem(0x77, 2) {
            Ok(id) => matches!(g.semop(id, 0, -1), Err(SV_RANGE)),
            Err(_) => false,
        }
    };
    let bad_idx = {
        let mut g = SysvLedger::new();
        match g.alloc_sem(0x78, 2) {
            Ok(id) => matches!(g.semop(id, 9, 1), Err(SV_RANGE)),
            Err(_) => false,
        }
    };
    let empty_recv = {
        let mut g = SysvLedger::new();
        match g.alloc_msg(0x79, 128) {
            Ok(id) => matches!(g.msg_recv(id), Err(SV_EMPTY)),
            Err(_) => false,
        }
    };
    s.add(
        "J6-范围与空账拒止",
        underflow && bad_idx && empty_recv,
        "semval 下溢/下标越界/空队接收三路都必须拒",
    );
    let unique_ids = {
        let mut ok = true;
        let mut k = 0usize;
        while k < led.shm.len() {
            if led.shm[k].id == 0 {
                ok = false;
            }
            k += 1;
        }
        ok && led.next_id == 11
    };
    let dup_key = {
        let mut g = SysvLedger::new();
        g.alloc_msg(0x88, 128).is_ok() && matches!(g.alloc_shm(0x88, 4096), Err(SV_DUP))
    };
    s.add(
        "J7-唯一与序不变式",
        unique_ids && dup_key,
        "id 从 1 连续发放、同 key 跨表必须拒",
    );
    let mut partial = match sample_ledger() {
        Ok(v) => v,
        Err(_) => SysvLedger::new(),
    };
    while !partial.msgs.is_empty() {
        partial.msgs.pop();
    }
    while !partial.sems.is_empty() {
        partial.sems.pop();
    }
    let negative = partial.object_count() == 4 && matches!(partial.verify(), Err(SV_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "只剩四 shm 的半账必须 SV_SHORT，不得伪装成绿",
    );
    // e2 是两槽信号量集，undo 回放两槽后返回 2，且被 semop 推高的槽归零
    let undo_ok = matches!(led.sem_undo(6), Ok(2)) && led.sems[1].vals[0] == 0;
    let mark_twice = {
        let mut g = SysvLedger::new();
        match g.alloc_shm(0x66, 4096) {
            Ok(id) => g.shm_remove(id).is_ok() && matches!(g.shm_remove(id), Err(SV_DUP)),
            Err(_) => false,
        }
    };
    s.add(
        "J10-undo与重复删除",
        undo_ok && mark_twice,
        "undo 必须归零、二次 IPC_RMID 必须 SV_DUP",
    );
    s.add_dyn(
        "J9-动态账",
        verified == 10 && led.next_id == 11,
        &format!(
            "verified={} 对象={} shm={} sem={} msg={}",
            verified,
            led.object_count(),
            led.shm.len(),
            led.sems.len(),
            led.msgs.len()
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0,
        &format!("fold={:016x} tag={:x}", led.fold(), SV_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11069_flow() {
        let mut led = sample_ledger().unwrap_or(SysvLedger::new());
        assert_eq!(led.verify().unwrap_or(0), 10);
        assert!(led.lifecycle_consistent());
        assert_eq!(led.object_count(), 10);
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11069_checks_green() {
        assert!(run_unx11069_checks().all_passed(), "UNX-F11069 判据未全绿");
    }
}
