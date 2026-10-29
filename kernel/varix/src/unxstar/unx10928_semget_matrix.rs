//! UNX-F10928 semget 错误矩阵（四错误面）（0x1d380 段）
//!
//! 锚点：UNX-F10928-J1 正面：EACCES/EEXIST/ENOSPC/EINVAL 四错误各
//! 10/10；反面：合法 key 零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·semget 面）。
//!
//! 结构：信号量集册（key→集合定容登记，含 nsems 上限与权限位）+
//! 裁断器（EACCES/EEXIST/ENOSPC/EINVAL 依 flag 分派）+ 注入核（无权/
//! 重创/表满/坏 nsems 四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d380 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d380;
pub const SG_EACCES: u32 = SEG_BASE + 0x01; // 无权限
pub const SG_EEXIST: u32 = SEG_BASE + 0x02; // 带 EXCL 重复建
pub const SG_ENOSPC: u32 = SEG_BASE + 0x03; // 表满
pub const SG_EINVAL: u32 = SEG_BASE + 0x04; // nsems 非法
pub const SG_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改册
pub const SG_ENOENT: u32 = SEG_BASE + 0x06; // 键缺失

/// flag 位。
pub const G_CREAT: u32 = 0x1;
pub const G_EXCL: u32 = 0x2;
pub const G_RDONLY: u32 = 0x4;
/// SEMMNI 上限。
pub const SEMMNI: usize = 10;
/// 每集信号量数上限。
pub const SEMMSL: u32 = 8;
/// SG 域标（fold 种子）。
pub const SG_TAG: u64 = 0x1d38221;

/// 信号量集条目。
#[derive(Clone, Debug)]
pub struct SemSet {
    pub key: u32,
    pub semid: u32,
    pub nsems: u32,
    pub mode: u32,
    pub alive: bool,
}

impl SemSet {
    pub fn readable(&self) -> bool {
        self.alive && self.mode & G_RDONLY != 0
    }

    pub fn render(&self) -> String {
        let a = if self.alive { "A" } else { "D" };
        format!("SS{}|{}|{}", self.key, a, self.nsems)
    }
}

/// semget 裁断册。
pub struct SemGetBoard {
    pub sets: Vec<SemSet>,
    pub frozen: bool,
    pub next_id: u32,
}

impl SemGetBoard {
    pub fn new() -> SemGetBoard {
        SemGetBoard { sets: Vec::new(), frozen: false, next_id: 1 }
    }

    /// 存活集数。
    pub fn alive_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.sets.len() {
            if self.sets[k].alive {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// semget 主裁断。
    pub fn get(&mut self, key: u32, nsems: u32, flags: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        if nsems == 0 || nsems > SEMMSL {
            return Err(SG_EINVAL);
        }
        if flags & !(G_CREAT | G_EXCL | G_RDONLY) != 0 {
            return Err(SG_EINVAL);
        }
        // 查活集。
        let mut k = 0usize;
        while k < self.sets.len() {
            if self.sets[k].key == key && self.sets[k].alive {
                if flags & G_CREAT != 0 && flags & G_EXCL != 0 {
                    return Err(SG_EEXIST);
                }
                if !self.sets[k].readable() {
                    return Err(SG_EACCES);
                }
                return Ok(self.sets[k].semid);
            }
            k += 1;
        }
        if flags & G_CREAT == 0 {
            return Err(SG_ENOENT);
        }
        if self.alive_count() >= SEMMNI {
            return Err(SG_ENOSPC);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.sets.push(SemSet { key, semid: id, nsems, mode: flags, alive: true });
        Ok(id)
    }

    /// 删集合（冻结后改册一律拒）。
    pub fn rmid(&mut self, semid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        let mut idx = self.sets.len();
        let mut k = 0usize;
        while k < self.sets.len() {
            if self.sets[k].semid == semid && self.sets[k].alive {
                idx = k;
            }
            k += 1;
        }
        if idx == self.sets.len() {
            return Err(SG_ENOENT);
        }
        self.sets[idx].alive = false;
        Ok(())
    }

    /// 链式 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h = SG_TAG;
        let mut k = 0usize;
        while k < self.sets.len() {
            let q = &self.sets[k];
            h = (h ^ q.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.semid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.nsems as u64).wrapping_mul(0x100000001b3);
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
    // 1) 无权限必报 EACCES
    let mut b = SemGetBoard::new();
    let _ = b.get(100, 1, G_CREAT);
    got[0] = b.get(100, 1, 0).err() == Some(SG_EACCES);
    // 2) EXCL 重复建必报 EEXIST
    let mut d = SemGetBoard::new();
    let _ = d.get(200, 1, G_CREAT | G_RDONLY);
    got[1] = d.get(200, 1, G_CREAT | G_EXCL | G_RDONLY).err() == Some(SG_EEXIST);
    // 3) 表满必报 ENOSPC
    let mut t = SemGetBoard::new();
    let mut k = 1u32;
    while k <= SEMMNI as u32 {
        let _ = t.get(k * 10, 1, G_CREAT | G_RDONLY);
        k += 1;
    }
    got[2] = t.get(999, 1, G_CREAT | G_RDONLY).err() == Some(SG_ENOSPC);
    // 4) 坏 nsems 必报 EINVAL
    let mut e = SemGetBoard::new();
    got[3] = e.get(300, 0, G_CREAT).err() == Some(SG_EINVAL)
        && e.get(300, SEMMSL + 1, G_CREAT).err() == Some(SG_EINVAL);
    got
}

/// key 确定性样本。
pub fn sg_sample_key(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(67) % 400 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10928_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10928");

    // J1 正面·EACCES 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = SemGetBoard::new();
        let _ = b.get(r + 100, 1, G_CREAT);
        if b.get(r + 100, 1, 0).err() == Some(SG_EACCES) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EACCES十连", h1 == 10, "无权限十轮全数 EACCES");

    // J2 正面·EEXIST 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut b = SemGetBoard::new();
        let _ = b.get(r2 + 200, 1, G_CREAT | G_RDONLY);
        if b.get(r2 + 200, 1, G_CREAT | G_EXCL | G_RDONLY).err() == Some(SG_EEXIST) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EEXIST十连", h2 == 10, "EXCL 重创十轮全数 EEXIST");

    // J3 正面·ENOSPC 十连
    let mut h3 = 0u32;
    let mut r3 = 0u32;
    while r3 < 10 {
        let mut t = SemGetBoard::new();
        let mut k = 1u32;
        while k <= SEMMNI as u32 {
            let _ = t.get(k * 10 + r3, 1, G_CREAT | G_RDONLY);
            k += 1;
        }
        if t.get(9000 + r3, 1, G_CREAT | G_RDONLY).err() == Some(SG_ENOSPC) {
            h3 += 1;
        }
        r3 += 1;
    }
    s.add("J3-ENOSPC十连", h3 == 10, "SEMMNI 满十轮全数 ENOSPC");

    // J4 正面·EINVAL 十连（nsems 零与超限混合）
    let mut h4 = 0u32;
    let mut r4 = 0u32;
    while r4 < 10 {
        let mut e = SemGetBoard::new();
        let bad = if r4 % 2 == 0 {
            e.get(r4 + 300, 0, G_CREAT).err() == Some(SG_EINVAL)
        } else {
            e.get(r4 + 300, SEMMSL + 1, G_CREAT).err() == Some(SG_EINVAL)
        };
        if bad {
            h4 += 1;
        }
        r4 += 1;
    }
    s.add("J4-EINVAL十连", h4 == 10, "坏 nsems 十轮全数 EINVAL");

    // J5 反面：合法 key 千次零误报
    let mut ok = 0u32;
    let mut r5 = 0u32;
    while r5 < 1000 {
        let mut b = SemGetBoard::new();
        let key = sg_sample_key(r5);
        let a = b.get(key, 2, G_CREAT | G_RDONLY);
        let c = b.get(key, 2, G_RDONLY);
        if a.is_ok() && c == a {
            ok += 1;
        }
        r5 += 1;
    }
    s.add("J5-合法千次零误报", ok == 1000, "建后即开千轮同 id");

    // J6 注入四路全抓
    let inj = inject_four();
    s.add("J6-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "无权/重创/表满/坏nsems全数命中");

    // J7 坏 flag 拒
    let j7 = SemGetBoard::new().get(400, 1, 0x8000).err() == Some(SG_EINVAL);
    s.add("J7-坏flag拒", j7, "越界 flag 必拒");

    // J8 查无键无 CREAT 报 ENOENT
    let j8 = SemGetBoard::new().get(500, 1, 0).err() == Some(SG_ENOENT);
    s.add("J8-查无键拒", j8, "无 CREAT 查无键必报 ENOENT");

    // J9 删后键可重建
    let j9 = {
        let mut b = SemGetBoard::new();
        let id = b.get(600, 1, G_CREAT | G_RDONLY);
        let _ = b.rmid(id.unwrap_or(0));
        b.get(600, 1, G_CREAT | G_RDONLY).is_ok()
    };
    s.add("J9-删后复活", j9, "删后同键可重建");

    // J10 冻结后 get/rmid 双拒
    let j10 = {
        let mut b = SemGetBoard::new();
        b.freeze();
        b.get(700, 1, G_CREAT).err() == Some(SG_FROZEN)
            && b.rmid(1).err() == Some(SG_FROZEN)
    };
    s.add("J10-冻结双拒", j10, "冻结后 get 与 rmid 均必拒");

    // J11 semid 单调分配
    let j11 = {
        let mut b = SemGetBoard::new();
        let a = b.get(800, 1, G_CREAT | G_RDONLY);
        let c = b.get(801, 1, G_CREAT | G_RDONLY);
        a == Ok(1) && c == Ok(2)
    };
    s.add("J11-id单调", j11, "semid 按建序单调");

    // J12 渲染面：活/死渲染可区分
    let j12 = {
        let a = SemSet { key: 1, semid: 1, nsems: 1, mode: G_RDONLY, alive: true };
        let mut b = a.clone();
        b.alive = false;
        a.render() != b.render()
    };
    s.add("J12-渲染分旗", j12, "活/死渲染可区分");

    // J13 fold 稳定
    let j13 = {
        let mut b = SemGetBoard::new();
        let f0 = b.fold();
        let _ = b.get(900, 1, G_CREAT | G_RDONLY);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J13-指纹稳定", j13, "建集入指纹且同态稳定");

    // J14 动态明细：四错误十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10 && h3 == 10 && h4 == 10, &format!("eacces={} eexist={} enospc={} einval={}", h1, h2, h3, h4));

    // J15 动态明细：千次零误报计数落账
    s.add_dyn("J15-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10928_semgetboard_flow() {
        let mut b = SemGetBoard::new();
        let id = b.get(9, 2, G_CREAT | G_RDONLY);
        assert_eq!(id, Ok(1));
        assert!(b.get(9, 1, G_CREAT | G_EXCL | G_RDONLY).err() == Some(SG_EEXIST));
        assert_eq!(b.get(9, 2, G_RDONLY), Ok(1));
        let mut p = SemGetBoard::new();
        let _ = p.get(10, 1, G_CREAT);
        assert!(p.get(10, 1, 0).err() == Some(SG_EACCES));
        assert!(b.get(11, 1, 0).err() == Some(SG_ENOENT));
        assert!(b.get(12, 0, G_CREAT).err() == Some(SG_EINVAL));
        let _ = b.rmid(1);
        assert!(b.get(9, 2, G_RDONLY).err() == Some(SG_ENOENT));
        b.freeze();
        assert!(b.get(9, 2, G_CREAT).err() == Some(SG_FROZEN));
    }

    #[test]
    fn unx10928_checks_green() {
        let s = run_unx10928_checks();
        assert!(s.all_passed(), "UNX-F10928 判据未全绿");
    }
}
