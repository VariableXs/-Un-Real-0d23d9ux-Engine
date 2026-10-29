//! UNX-F10921 msgget 错误矩阵（key 四错误面）（0x1d2a0 段）
//!
//! 锚点：UNX-F10921-J1 正面：ENOENT/EEXIST/EACCES/EINVAL 四错误各
//! 10/10；反面：合法 key 零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgget 面）。
//!
//! 结构：键册（key→队列定容登记，含权限位与私有键哨兵）+ 裁断器
//! （ENOENT/EEXIST/EACCES/EINVAL 依 flag 分派）+ 注入核（查无键/
//! 重复创建/无权限/坏 flag 四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d2a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d2a0;
pub const MG_ENOENT: u32 = SEG_BASE + 0x01; // 键不存在且无 IPC_CREAT
pub const MG_EEXIST: u32 = SEG_BASE + 0x02; // 键已存在且带 IPC_EXCL
pub const MG_EACCES: u32 = SEG_BASE + 0x03; // 无读权限
pub const MG_EINVAL: u32 = SEG_BASE + 0x04; // flag 非法
pub const MG_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改册
pub const MG_NOPERM: u32 = SEG_BASE + 0x06; // 权限位缺失

/// flag 位。
pub const F_CREAT: u32 = 0x1;
pub const F_EXCL: u32 = 0x2;
pub const F_RDONLY: u32 = 0x4;
/// 私有键哨兵。
pub const KEY_PRIVATE: u32 = 0xffff_ffff;
/// 键册定容。
pub const CAP: usize = 16;
/// MG 域标（fold 种子）。
pub const MG_TAG: u64 = 0x1d2a215;

/// 队列登记项。
#[derive(Clone, Debug)]
pub struct MsgQueue {
    pub key: u32,
    pub qid: u32,
    pub mode: u32,
    pub alive: bool,
}

impl MsgQueue {
    /// 读权限判定。
    pub fn readable(&self) -> bool {
        self.alive && self.mode & F_RDONLY != 0
    }

    pub fn render(&self) -> String {
        let a = if self.alive { "A" } else { "D" };
        format!("Q{}|{}|{:x}", self.key, a, self.mode)
    }
}

/// msgget 错误裁断册。
pub struct KeyBoard {
    pub queues: Vec<MsgQueue>,
    pub frozen: bool,
    pub next_qid: u32,
}

impl KeyBoard {
    pub fn new() -> KeyBoard {
        KeyBoard { queues: Vec::new(), frozen: false, next_qid: 1 }
    }

    /// 建队列（IPC_CREAT 语义）。
    pub fn create(&mut self, key: u32, mode: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(MG_FROZEN);
        }
        if key == KEY_PRIVATE {
            // 私有键必新建。
            return self.push(key, mode);
        }
        if self.queues.iter().any(|q| q.key == key && q.alive) {
            return Err(MG_EEXIST);
        }
        self.push(key, mode)
    }

    /// 打开队列（无 IPC_CREAT）。
    pub fn open(&self, key: u32) -> Result<u32, u32> {
        let mut k = 0usize;
        while k < self.queues.len() {
            if self.queues[k].key == key && self.queues[k].alive {
                if !self.queues[k].readable() {
                    return Err(MG_EACCES);
                }
                return Ok(self.queues[k].qid);
            }
            k += 1;
        }
        Err(MG_ENOENT)
    }

    /// msgget 主裁断：按 flag 分派四错误。
    pub fn get(&mut self, key: u32, flags: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(MG_FROZEN);
        }
        if flags & !(F_CREAT | F_EXCL | F_RDONLY) != 0 {
            return Err(MG_EINVAL);
        }
        if flags & F_CREAT != 0 {
            self.create(key, flags)
        } else {
            self.open(key)
        }
    }

    /// 实际入册。
    fn push(&mut self, key: u32, mode: u32) -> Result<u32, u32> {
        if self.queues.len() >= CAP {
            return Err(MG_EINVAL);
        }
        let qid = self.next_qid;
        self.next_qid += 1;
        self.queues.push(MsgQueue { key, qid, mode, alive: true });
        Ok(qid)
    }

    /// 链式 fold 指纹（序敏感，含键与权限位）。
    pub fn fold(&self) -> u64 {
        let mut h = MG_TAG;
        let mut k = 0usize;
        while k < self.queues.len() {
            let q = &self.queues[k];
            h = (h ^ q.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.mode as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.alive as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 查无键必报 ENOENT
    let b = KeyBoard::new();
    got[0] = b.open(100).err() == Some(MG_ENOENT);
    // 2) 重复创建必报 EEXIST
    let mut d = KeyBoard::new();
    let _ = d.create(200, F_RDONLY);
    got[1] = d.create(200, F_RDONLY).err() == Some(MG_EEXIST);
    // 3) 无读权限必报 EACCES
    let mut p = KeyBoard::new();
    let _ = p.create(300, 0);
    got[2] = p.open(300).err() == Some(MG_EACCES);
    // 4) 坏 flag 必报 EINVAL
    let mut e = KeyBoard::new();
    got[3] = e.get(400, 0x8000).err() == Some(MG_EINVAL);
    got
}

/// key 确定性样本。
pub fn mg_sample_key(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(37) % 1000 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10921");

    // J1 正面·ENOENT 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let b = KeyBoard::new();
        if b.open(mg_sample_key(r + 50)).err() == Some(MG_ENOENT) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-ENOENT十连", h1 == 10, "查无键十轮全数 ENOENT");

    // J2 正面·EEXIST 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut d = KeyBoard::new();
        let k = mg_sample_key(r2 + 60);
        let _ = d.create(k, F_RDONLY);
        if d.create(k, F_RDONLY).err() == Some(MG_EEXIST) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EEXIST十连", h2 == 10, "重复创建十轮全数 EEXIST");

    // J3 正面·EACCES 十连
    let mut h3 = 0u32;
    let mut r3 = 0u32;
    while r3 < 10 {
        let mut p = KeyBoard::new();
        let _ = p.create(mg_sample_key(r3 + 70), 0);
        if p.open(mg_sample_key(r3 + 70)).err() == Some(MG_EACCES) {
            h3 += 1;
        }
        r3 += 1;
    }
    s.add("J3-EACCES十连", h3 == 10, "无权限十轮全数 EACCES");

    // J4 正面·EINVAL 十连（坏 flag）
    let mut h4 = 0u32;
    let mut r4 = 0u32;
    while r4 < 10 {
        let mut e = KeyBoard::new();
        if e.get(mg_sample_key(r4 + 80), 0x100 << r4 % 4).err() == Some(MG_EINVAL) {
            h4 += 1;
        }
        r4 += 1;
    }
    s.add("J4-EINVAL十连", h4 == 10, "坏 flag 十轮全数 EINVAL");

    // J5 反面：合法 key 千次零误报
    let mut ok = 0u32;
    let mut r5 = 0u32;
    while r5 < 1000 {
        let mut b = KeyBoard::new();
        let k = mg_sample_key(r5);
        let q1 = b.get(k, F_CREAT | F_RDONLY);
        let q2 = b.get(k, F_RDONLY);
        if q1.is_ok() && q2 == q1 {
            ok += 1;
        }
        r5 += 1;
    }
    s.add("J5-合法千次零误报", ok == 1000, "建后即开千轮回读一致");

    // J6 私有键必新建
    let j6 = {
        let mut b = KeyBoard::new();
        let a = b.create(KEY_PRIVATE, F_RDONLY);
        let c = b.create(KEY_PRIVATE, F_RDONLY);
        a != c
    };
    s.add("J6-私有键新建", j6, "私有键两次建得不同 qid");

    // J7 qid 单调分配
    let j7 = {
        let mut b = KeyBoard::new();
        let a = b.create(1, F_RDONLY);
        let c = b.create(2, F_RDONLY);
        a == Ok(1) && c == Ok(2)
    };
    s.add("J7-qid单调", j7, "qid 按建序单调分配");

    // J8 注入四路全抓
    let inj = inject_four();
    s.add("J8-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "查无/重创/无权/坏flag全数命中");

    // J9 死队列重建复活
    let j9 = {
        let mut b = KeyBoard::new();
        let _ = b.create(500, F_RDONLY);
        b.queues[0].alive = false;
        b.create(500, F_RDONLY).is_ok()
    };
    s.add("J9-死键复活", j9, "死队列键可重建");

    // J10 冻结后建/开均拒
    let j10 = {
        let mut b = KeyBoard::new();
        b.freeze();
        b.get(600, F_CREAT).err() == Some(MG_FROZEN)
    };
    s.add("J10-冻结拒操作", j10, "冻结后 msgget 必拒");

    // J11 渲染面：活/死队列渲染可区分
    let j11 = {
        let a = MsgQueue { key: 1, qid: 1, mode: F_RDONLY, alive: true };
        let mut c = a.clone();
        c.alive = false;
        a.render() != c.render()
    };
    s.add("J11-渲染分旗", j11, "活/死渲染可区分");

    // J12 fold 稳定：建队列改变指纹、同态一致
    let j12 = {
        let mut b = KeyBoard::new();
        let f0 = b.fold();
        let _ = b.create(700, F_RDONLY);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "建队列入指纹且同态稳定");

    // J13 动态明细：千次零误报计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：四错误十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10 && h3 == 10 && h4 == 10, &format!("enoent={} eexist={} eacces={} einval={}", h1, h2, h3, h4));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10921_keyboard_flow() {
        let mut b = KeyBoard::new();
        let q = b.create(9, F_RDONLY);
        assert_eq!(q, Ok(1));
        assert!(b.create(9, F_RDONLY).err() == Some(MG_EEXIST));
        assert_eq!(b.open(9), Ok(1));
        let mut p = KeyBoard::new();
        let _ = p.create(10, 0);
        assert!(p.open(10).err() == Some(MG_EACCES));
        assert!(b.open(11).err() == Some(MG_ENOENT));
        assert!(b.get(12, 0x8000).err() == Some(MG_EINVAL));
        b.freeze();
        assert!(b.get(13, F_CREAT).err() == Some(MG_FROZEN));
    }

    #[test]
    fn unx10921_checks_green() {
        let s = run_unx10921_checks();
        assert!(s.all_passed(), "UNX-F10921 判据未全绿");
    }
}
