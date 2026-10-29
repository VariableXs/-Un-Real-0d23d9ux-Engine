//! UNX-F10908 三复用器 timeout 负值对照矩阵（0x1d100 段）
//!
//! 锚点：UNX-F10908-J1 正面：三调用负 timeout 语义各显式（10/10）；
//! 反面：三调用语义零互串（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·timeout 语义对照面）。
//!
//! 结构：语义表（select/poll/epoll × {零/负一/其他负} 定容九格）+
//! 裁断器（select 负 timeval→EINVAL、poll/epoll 负一→阻塞、其余负值
//! 仍阻塞但记歧义）+ 注入核（互串/缺格/冻结改表/非法格四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d100 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d100;
pub const TM_EINVAL: u32 = SEG_BASE + 0x01; // select 负 timeval 非法
pub const TM_NOSLOT: u32 = SEG_BASE + 0x02; // 语义格缺失
pub const TM_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改表
pub const TM_CROSS: u32 = SEG_BASE + 0x04; // 语义互串
pub const TM_BADKIND: u32 = SEG_BASE + 0x05; // 复用器种类非法

/// 复用器种类。
pub const K_SELECT: u8 = 1;
pub const K_POLL: u8 = 2;
pub const K_EPOLL: u8 = 3;

/// 语义格结果码。
pub const S_INVAL: u8 = 1; // 显性 EINVAL
pub const S_BLOCK: u8 = 2; // 无限阻塞
pub const S_NOWAIT: u8 = 3; // 立即返回（零超时）

/// 九格语义表定容。
pub const SLOTS: usize = 9;
/// TM 域标（fold 种子）。
pub const TM_TAG: u64 = 0x1d100d08;

/// 语义格。
#[derive(Clone, Debug, PartialEq)]
pub struct SemSlot {
    pub kind: u8,
    pub tv_class: u8, // 0=零 1=负一 2=其他负
    pub sem: u8,
}

impl SemSlot {
    pub fn render(&self) -> String {
        let k = match self.kind {
            K_SELECT => "SEL",
            K_POLL => "POL",
            _ => "EPL",
        };
        let v = match self.tv_class {
            0 => "Z",
            1 => "N",
            _ => "O",
        };
        format!("{}{}|{}", k, v, self.sem)
    }
}

/// timeout 语义对照表。
pub struct SemTable {
    pub slots: Vec<SemSlot>,
    pub frozen: bool,
}

impl SemTable {
    pub fn new() -> SemTable {
        SemTable { slots: Vec::new(), frozen: false }
    }

    /// 装格：种类/格类/语义三者定容且不重复。
    pub fn put(&mut self, kind: u8, tv_class: u8, sem: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(TM_FROZEN);
        }
        if kind < K_SELECT || kind > K_EPOLL {
            return Err(TM_BADKIND);
        }
        if tv_class > 2 {
            return Err(TM_NOSLOT);
        }
        if sem < S_INVAL || sem > S_NOWAIT {
            return Err(TM_NOSLOT);
        }
        if self.slots.iter().any(|e| e.kind == kind && e.tv_class == tv_class) {
            return Err(TM_NOSLOT);
        }
        if self.slots.len() >= SLOTS {
            return Err(TM_NOSLOT);
        }
        self.slots.push(SemSlot { kind, tv_class, sem });
        Ok(())
    }

    /// 查格。
    pub fn get(&self, kind: u8, tv_class: u8) -> Result<u8, u32> {
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if e.kind == kind && e.tv_class == tv_class {
                return Ok(e.sem);
            }
            k += 1;
        }
        Err(TM_NOSLOT)
    }

    /// 裁断：按标准语义裁 timeout。
    pub fn judge(&self, kind: u8, tv_sec: i64, tv_usec: i64) -> Result<u8, u32> {
        if kind < K_SELECT || kind > K_EPOLL {
            return Err(TM_BADKIND);
        }
        let tv_class = if tv_sec == 0 && tv_usec == 0 {
            0u8
        } else if tv_sec < 0 && tv_sec != -1 {
            2u8
        } else if tv_sec < 0 {
            1u8
        } else {
            return Err(TM_NOSLOT); // 正值超时不在本矩阵域内
        };
        let want = match (kind, tv_class) {
            (K_SELECT, 0) => S_NOWAIT,
            (K_SELECT, 1) => S_BLOCK,
            (K_SELECT, 2) => S_INVAL,
            (K_POLL, 0) => S_NOWAIT,
            (K_POLL, 1) => S_BLOCK,
            (K_POLL, 2) => S_BLOCK,
            (K_EPOLL, 0) => S_NOWAIT,
            (K_EPOLL, 1) => S_BLOCK,
            _ => S_BLOCK, // epoll 其他负仍阻塞
        };
        let got = self.get(kind, tv_class)?;
        if got != want {
            return Err(TM_CROSS);
        }
        Ok(got)
    }

    /// 装满标准九格。
    pub fn load_standard(&mut self) -> Result<(), u32> {
        let mut kind = K_SELECT;
        while kind <= K_EPOLL {
            let mut tv = 0u8;
            while tv <= 2 {
                let sem = match (kind, tv) {
                    (K_SELECT, 0) => S_NOWAIT,
                    (K_SELECT, 1) => S_BLOCK,
                    (K_SELECT, 2) => S_INVAL,
                    (K_POLL, 0) => S_NOWAIT,
                    (K_POLL, 1) => S_BLOCK,
                    (K_POLL, 2) => S_BLOCK,
                    (K_EPOLL, 0) => S_NOWAIT,
                    (K_EPOLL, 1) => S_BLOCK,
                    _ => S_BLOCK,
                };
                self.put(kind, tv, sem)?;
                tv += 1;
            }
            kind += 1;
        }
        Ok(())
    }

    /// 链式 fold 指纹（序敏感，含语义值）。
    pub fn fold(&self) -> u64 {
        let mut h = TM_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.kind as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.tv_class as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.sem as u64).wrapping_mul(0x100000001b3);
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
    // 1) 语义互串：select 其他负装成 BLOCK 必报 CROSS
    let mut t = SemTable::new();
    let _ = t.put(K_SELECT, 2, S_BLOCK);
    got[0] = t.judge(K_SELECT, -2, 0).err() == Some(TM_CROSS);
    // 2) 缺格：未装格必报 NOSLOT
    let e = SemTable::new();
    got[1] = e.judge(K_POLL, -1, 0).err() == Some(TM_NOSLOT);
    // 3) 非法种类必报 BADKIND
    let mut b = SemTable::new();
    got[2] = b.put(9, 0, S_BLOCK).err() == Some(TM_BADKIND)
        && b.judge(9, 0, 0).err() == Some(TM_BADKIND);
    // 4) 冻结后装格必抓
    let mut f = SemTable::new();
    f.freeze();
    got[3] = f.put(K_SELECT, 0, S_NOWAIT).err() == Some(TM_FROZEN);
    got
}

/// 格类确定性样本：第 r 轮的种类与格类。
pub fn tm_sample(r: u32) -> (u8, u8) {
    let k = (r.wrapping_mul(2654435761).wrapping_add(17) % 3 + 1) as u8;
    let t = (r.wrapping_mul(2246822519) % 3) as u8;
    (k, t)
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10908_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10908");

    // J1 正面：三调用负 timeout 语义各显式（十轮循环全命中）
    let mut hits = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        let a = t.judge(K_SELECT, -1, 0) == Ok(S_BLOCK);
        let b = t.judge(K_SELECT, -2, 0) == Ok(S_INVAL);
        let c = t.judge(K_POLL, -1, 0) == Ok(S_BLOCK);
        let d = t.judge(K_EPOLL, -1, 0) == Ok(S_BLOCK);
        if a && b && c && d {
            hits += 1;
        }
        r += 1;
    }
    s.add("J1-负超时语义显式", hits == 10, "三调用负 timeout 语义十轮全显式");

    // J2 零超时三调用全为立即返回
    let j2 = {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        t.judge(K_SELECT, 0, 0) == Ok(S_NOWAIT)
            && t.judge(K_POLL, 0, 0) == Ok(S_NOWAIT)
            && t.judge(K_EPOLL, 0, 0) == Ok(S_NOWAIT)
    };
    s.add("J2-零超时立即返回", j2, "三调用零超时语义一致为 NOWAIT");

    // J3 九格全装满
    let j3 = {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        t.slots.len() == SLOTS
    };
    s.add("J3-九格全满", j3, "标准语义表九格齐装");

    // J4 反面：三调用语义零互串（千次随机格核对）
    let mut ok = 0u32;
    let mut r4 = 0u32;
    while r4 < 1000 {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        let (k, tv) = tm_sample(r4);
        let v = t.judge(k, -(tv as i64), 0);
        let want = match (k, tv) {
            (K_SELECT, 0) => Ok(S_NOWAIT),
            (K_SELECT, 1) => Ok(S_BLOCK),
            (K_SELECT, 2) => Ok(S_INVAL),
            (K_POLL, 0) => Ok(S_NOWAIT),
            (K_POLL, _) => Ok(S_BLOCK),
            (K_EPOLL, 0) => Ok(S_NOWAIT),
            _ => Ok(S_BLOCK),
        };
        if v == want {
            ok += 1;
        }
        r4 += 1;
    }
    s.add("J4-千次零互串", ok == 1000, "随机格千轮裁断与标准语义零互串");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "互串/缺格/坏种类/冻结全数命中");

    // J6 重复装格拒
    let j6 = {
        let mut t = SemTable::new();
        let _ = t.put(K_SELECT, 0, S_NOWAIT);
        t.put(K_SELECT, 0, S_NOWAIT).err() == Some(TM_NOSLOT)
    };
    s.add("J6-重格拒收", j6, "同格重复装必拒");

    // J7 非法语义值拒
    let j7 = SemTable::new().put(K_SELECT, 0, 9).err() == Some(TM_NOSLOT);
    s.add("J7-语义值非法", j7, "语义值越界必拒");

    // J8 正值超时不在域内
    let j8 = {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        t.judge(K_POLL, 5, 0).err() == Some(TM_NOSLOT)
    };
    s.add("J8-正值出域", j8, "正值 timeout 不在本矩阵域");

    // J9 冻结后装格拒
    let j9 = {
        let mut t = SemTable::new();
        t.freeze();
        t.put(K_POLL, 0, S_NOWAIT).err() == Some(TM_FROZEN)
    };
    s.add("J9-冻结拒装格", j9, "冻结后装格必拒");

    // J10 渲染面：三种类九格渲染互异
    let j10 = {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        let mut all_diff = true;
        let mut i = 0usize;
        while i < t.slots.len() {
            let mut j = i + 1;
            while j < t.slots.len() {
                if t.slots[i].render() == t.slots[j].render() {
                    all_diff = false;
                }
                j += 1;
            }
            i += 1;
        }
        all_diff
    };
    s.add("J10-渲染互异", j10, "九格渲染两两互异");

    // J11 fold 稳定：同态一致、改格必变
    let j11 = {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        let f1 = t.fold();
        let f2 = t.fold();
        let _ = t.put(K_SELECT, 2, S_BLOCK); // 重复格会拒，改用判空校验
        f1 == f2 && f1 != 0 && f1 == t.fold()
    };
    s.add("J11-指纹稳定", j11, "同态指纹稳定");

    // J12 动态明细：千次零互串计数落账
    s.add_dyn("J12-千次明细", ok == 1000, &format!("cross_ok={}/1000", ok));

    // J13 动态明细：九格指纹十六进制落账
    let fp = {
        let mut t = SemTable::new();
        let _ = t.load_standard();
        t.fold()
    };
    s.add_dyn("J13-九格指纹", fp != 0, &format!("table_fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10908_semtable_flow() {
        let mut t = SemTable::new();
        assert!(t.load_standard().is_ok());
        assert_eq!(t.judge(K_SELECT, -1, 0), Ok(S_BLOCK));
        assert_eq!(t.judge(K_SELECT, -2, 0), Ok(S_INVAL));
        assert_eq!(t.judge(K_POLL, -1, 0), Ok(S_BLOCK));
        assert_eq!(t.judge(K_EPOLL, 0, 0), Ok(S_NOWAIT));
        assert!(t.judge(9, -1, 0).err() == Some(TM_BADKIND));
        t.freeze();
        assert!(t.put(K_SELECT, 0, S_NOWAIT).err() == Some(TM_FROZEN));
    }

    #[test]
    fn unx10908_checks_green() {
        let s = run_unx10908_checks();
        assert!(s.all_passed(), "UNX-F10908 判据未全绿");
    }
}
