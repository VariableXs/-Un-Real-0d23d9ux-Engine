//! UNX-F12059 PEB 字段语义画像档案
//! 分段: 0x19980 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：为环境块关键字段建立"语义画像"档案：
//!   每字段记录 { 生命周期阶段集, 可变主体集, 观察者集, 读写配对规则 }。
//!   校验规则：
//!   - 阶段引用必须落在 P1..P6（映射到 12003 的六相位）；
//!   - 主体必须是 Kernel/Process/Loader 之一；
//!   - 画像未经双重批准（kernel+process 各一签）不得生效 → P59_UNRAT；
//!   - 主体/阶段非法 → P59_ACTOR/P59_PHASE；
//!   - 档案封存后 → P59_SEALED。

use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19980;
pub const SEG_P59_PHASE: u32 = SEG_BASE + 0x01;
pub const SEG_P59_ACTOR: u32 = SEG_BASE + 0x02;
pub const SEG_P59_UNRAT: u32 = SEG_BASE + 0x03;
pub const SEG_P59_SEALED: u32 = SEG_BASE + 0x04;
pub const SEG_P59_DUP: u32 = SEG_BASE + 0x05;

/// 生命周期阶段（对应 12003 六相位）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    P1Init = 1,
    P2Map = 2,
    P3Peb = 3,
    P4Run = 4,
    P5Pend = 5,
    P6Term = 6,
}

impl Phase {
    pub fn from_num(n: u32) -> Option<Phase> {
        match n {
            1 => Some(Phase::P1Init),
            2 => Some(Phase::P2Map),
            3 => Some(Phase::P3Peb),
            4 => Some(Phase::P4Run),
            5 => Some(Phase::P5Pend),
            6 => Some(Phase::P6Term),
            _ => None,
        }
    }
}

/// 主体
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Kernel,
    Process,
    Loader,
}

impl Actor {
    pub fn label(self) -> &'static str {
        match self {
            Actor::Kernel => "KRNL",
            Actor::Process => "PROC",
            Actor::Loader => "LDR",
        }
    }
}

/// 字段画像
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub field: u32,
    pub phases: Vec<u32>,   // 阶段号 1..=6
    pub writers: Vec<Actor>,
    pub rat_kernel: bool,
    pub rat_process: bool,
    pub sealed: bool,
}

impl Profile {
    /// 新建画像：阶段号与主体先做合法性检查
    pub fn new(field: u32, phases: &[u32], writers: &[Actor]) -> Result<Profile, u32> {
        for p in phases {
            if Phase::from_num(*p).is_none() {
                return Err(SEG_P59_PHASE);
            }
        }
        if writers.is_empty() {
            return Err(SEG_P59_ACTOR);
        }
        Ok(Profile {
            field,
            phases: phases.to_vec(),
            writers: writers.to_vec(),
            rat_kernel: false,
            rat_process: false,
            sealed: false,
        })
    }

    /// 签批
    pub fn ratify(&mut self, by: Actor) -> Result<(), u32> {
        if self.sealed {
            return Err(SEG_P59_SEALED);
        }
        match by {
            Actor::Kernel => self.rat_kernel = true,
            Actor::Process => self.rat_process = true,
            Actor::Loader => return Err(SEG_P59_ACTOR),
        }
        Ok(())
    }

    /// 双签生效判定
    pub fn effective(&self) -> bool {
        self.rat_kernel && self.rat_process
    }

    /// 某主体在某阶段是否可写
    pub fn may_write(&self, a: Actor, phase: u32) -> bool {
        if !self.effective() {
            return false;
        }
        if !self.phases.iter().any(|p| *p == phase) {
            return false;
        }
        self.writers.iter().any(|w| *w == a)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 画像档案库
#[derive(Debug, Clone)]
pub struct ProfileBook {
    pub profiles: Vec<Profile>,
    pub sealed: bool,
}

impl ProfileBook {
    pub fn new() -> ProfileBook {
        ProfileBook { profiles: Vec::new(), sealed: false }
    }

    /// 收录：重复字段 → DUP；封存 → SEALED
    pub fn add(&mut self, p: Profile) -> Result<usize, u32> {
        if self.sealed {
            return Err(SEG_P59_SEALED);
        }
        if self.profiles.iter().any(|x| x.field == p.field) {
            return Err(SEG_P59_DUP);
        }
        self.profiles.push(p);
        Ok(self.profiles.len() - 1)
    }

    /// 全库生效计数
    pub fn effective_count(&self) -> usize {
        self.profiles.iter().filter(|p| p.effective()).count()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
        for p in &mut self.profiles {
            p.sealed = true;
        }
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(book: &ProfileBook) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for p in &book.profiles {
        h = (h.wrapping_mul(211).wrapping_add(p.field as u64)) & 0xFFFF_FFFF;
        for ph in &p.phases {
            h = (h.wrapping_mul(211).wrapping_add(*ph as u64)) & 0xFFFF_FFFF;
        }
        for w in &p.writers {
            h = (h.wrapping_mul(211).wrapping_add(w.label().bytes().next().unwrap_or(0) as u64)) & 0xFFFF_FFFF;
        }
        h = (h.wrapping_mul(211).wrapping_add(p.effective() as u64)) & 0xFFFF_FFFF;
    }
    h = (h.wrapping_mul(211).wrapping_add(book.sealed as u64)) & 0xFFFF_FFFF;
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 阶段 7 非法 → PHASE
    if Profile::new(1, &[7], &[Actor::Kernel]) == Err(SEG_P59_PHASE) {
        ok[0] = true;
    }
    // 样2: 空主体集 → ACTOR
    if Profile::new(1, &[1], &[]) == Err(SEG_P59_ACTOR) {
        ok[1] = true;
    }
    // 样3: 未双签可写 → 拒绝（may_write false）
    let p = Profile::new(2, &[4], &[Actor::Kernel]).unwrap_or_else(|_| Profile {
        field: 2, phases: vec![4], writers: vec![Actor::Kernel],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    if !p.may_write(Actor::Kernel, 4) {
        ok[2] = true;
    }
    // 样4: 生效但阶段不在集 → 拒绝
    let mut p2 = Profile::new(3, &[1], &[Actor::Process]).unwrap_or_else(|_| Profile {
        field: 3, phases: vec![1], writers: vec![Actor::Process],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    let _ = p2.ratify(Actor::Kernel);
    let _ = p2.ratify(Actor::Process);
    if p2.effective() && !p2.may_write(Actor::Process, 5) {
        ok[3] = true;
    }
    // 样5: 封存库再收录 → SEALED
    let mut b = ProfileBook::new();
    b.seal();
    if b.add(p).err() == Some(SEG_P59_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12059_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12059");
    // J1 合法画像建立
    let mut p = Profile::new(1, &[1, 3, 6], &[Actor::Kernel, Actor::Process]);
    cs.add("J1", p.is_ok(), "profile build");
    // J2 阶段 0 拒绝
    cs.add("J2", Profile::new(1, &[0], &[Actor::Kernel]).err() == Some(SEG_P59_PHASE), "phase 0 bad");
    // J3 阶段 7 拒绝
    cs.add("J3", Profile::new(1, &[7], &[Actor::Kernel]).err() == Some(SEG_P59_PHASE), "phase 7 bad");
    // J4 六阶段全可引用
    let mut all = true;
    let mut n = 1;
    while n <= 6 {
        all = all && Profile::new(1, &[n], &[Actor::Loader]).is_ok();
        n += 1;
    }
    cs.add("J4", all, "phases 1..6 ok");
    // J5 空主体拒绝
    cs.add("J5", Profile::new(1, &[1], &[]).err() == Some(SEG_P59_ACTOR), "no actor");
    // J6 单签未生效
    let mut p2 = Profile::new(2, &[4], &[Actor::Kernel]).unwrap_or_else(|_| Profile {
        field: 2, phases: vec![4], writers: vec![Actor::Kernel],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    let _ = p2.ratify(Actor::Kernel);
    cs.add("J6", !p2.effective(), "half ratify");
    // J7 双签生效
    let _ = p2.ratify(Actor::Process);
    cs.add("J7", p2.effective(), "full ratify");
    // J8 生效后按规则可写
    cs.add("J8", p2.may_write(Actor::Kernel, 4), "may write run");
    // J9 阶段不在集拒绝
    cs.add("J9", !p2.may_write(Actor::Kernel, 1), "phase not in set");
    // J10 主体不在集拒绝
    let mut p3 = Profile::new(3, &[2], &[Actor::Loader]).unwrap_or_else(|_| Profile {
        field: 3, phases: vec![2], writers: vec![Actor::Loader],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    let _ = p3.ratify(Actor::Kernel);
    let _ = p3.ratify(Actor::Process);
    cs.add("J10", !p3.may_write(Actor::Kernel, 2), "actor not in set");
    // J11 Loader 不可签批
    let mut p4 = Profile::new(4, &[1], &[Actor::Kernel]).unwrap_or_else(|_| Profile {
        field: 4, phases: vec![1], writers: vec![Actor::Kernel],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    cs.add("J11", p4.ratify(Actor::Loader).err() == Some(SEG_P59_ACTOR), "loader cant ratify");
    // J12 档案库收录与重复
    let mut b = ProfileBook::new();
    let mk = || Profile::new(9, &[1], &[Actor::Kernel]).unwrap_or_else(|_| Profile {
        field: 9, phases: vec![1], writers: vec![Actor::Kernel],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    cs.add("J12", b.add(mk()).is_ok() && b.add(mk()).err() == Some(SEG_P59_DUP), "book dup");
    // J13 生效计数
    let mut b2 = ProfileBook::new();
    let mut pa = Profile::new(1, &[1], &[Actor::Kernel]).unwrap_or_else(|_| Profile {
        field: 1, phases: vec![1], writers: vec![Actor::Kernel],
        rat_kernel: false, rat_process: false, sealed: false,
    });
    let _ = pa.ratify(Actor::Kernel);
    let _ = pa.ratify(Actor::Process);
    let _ = b2.add(pa);
    let _ = b2.add(Profile::new(2, &[2], &[Actor::Process]).unwrap_or_else(|_| Profile {
        field: 2, phases: vec![2], writers: vec![Actor::Process],
        rat_kernel: false, rat_process: false, sealed: false,
    }));
    cs.add("J13", b2.effective_count() == 1, "effective 1 of 2");
    // J14 封存传播
    b2.seal();
    cs.add("J14", b2.profiles.iter().all(|p| p.sealed), "seal propagate");
    // J15 主体标签
    cs.add("J15", Actor::Loader.label() == "LDR", "actor label");
    // J16 注入五样
    let inj = inject_five();
    cs.add("J16", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ratify_gate() {
        let mut p = Profile::new(1, &[1, 2], &[Actor::Kernel, Actor::Process])
            .ok().unwrap_or_else(|| Profile { field: 1, phases: vec![1, 2], writers: vec![Actor::Kernel, Actor::Process], rat_kernel: false, rat_process: false, sealed: false });
        assert!(!p.effective());
        let _ = p.ratify(Actor::Kernel);
        assert!(!p.effective());
        let _ = p.ratify(Actor::Process);
        assert!(p.effective());
    }

    #[test]
    fn test_phase_bounds() {
        assert!(Profile::new(1, &[1, 6], &[Actor::Kernel]).is_ok());
        assert_eq!(Profile::new(1, &[0], &[Actor::Kernel]).err(), Some(SEG_P59_PHASE));
        assert_eq!(Profile::new(1, &[7], &[Actor::Kernel]).err(), Some(SEG_P59_PHASE));
    }

    #[test]
    fn test_fold_stable() {
        let mut b = ProfileBook::new();
        let _ = b.add(Profile::new(3, &[2], &[Actor::Loader]).ok().unwrap_or_else(|| Profile {
            field: 3, phases: vec![2], writers: vec![Actor::Loader],
            rat_kernel: false, rat_process: false, sealed: false,
        }));
        let a = fold(&b);
        let c = fold(&b);
        assert_eq!(a, c);
        assert_ne!(a, SEG_BASE);
    }
}
