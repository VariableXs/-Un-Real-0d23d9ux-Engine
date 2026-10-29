//! UNX-F11232 getpid/getuid 凭据面消费（0x1f832 段）
//!
//! 锚点：UNX-F11232-J1 凭据查询族（pid/ppid/uid/euid/gid/egid）缓存假设与
//! 失效语义判据绿。
//! 域：CoRun·UNX-C5-B02——pid/ppid 终生不变，uid/euid/gid/egid 可被 set*id
//! 改写；缓存只能缓存「当前值」，任何改写必须先失效再重取。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f832;
pub const CR_ID: u32 = SEG_BASE + 0x01; // 未知凭据字段
pub const CR_CACHE: u32 = SEG_BASE + 0x02; // 缓存未命中却报命中
pub const CR_STALE: u32 = SEG_BASE + 0x03; // 改写后仍取旧值
pub const CR_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改写
pub const CR_IMMUT: u32 = SEG_BASE + 0x05; // 试图改写不可变字段
pub const CR_OVER: u32 = SEG_BASE + 0x06; // 超容
pub const CR_SCOPE: u32 = SEG_BASE + 0x07; // 字段作用域错

/// 六个凭据字段名（索引固定）。
pub const FIELDS: [&str; 6] = ["pid", "ppid", "uid", "euid", "gid", "egid"];
pub const F_PID: usize = 0;
pub const F_PPID: usize = 1;
pub const F_UID: usize = 2;
pub const F_EUID: usize = 3;
pub const F_GID: usize = 4;
pub const F_EGID: usize = 5;
pub const MAX_LOGS: usize = 24;
pub const CR_TAG: u64 = 0x1f832d7;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一份凭据快照。
#[derive(Clone, Debug)]
pub struct Cred {
    pub vals: [u32; 6],
    pub epoch: u32,
}

impl Cred {
    pub fn get(&self, field: usize) -> Result<u32, u32> {
        if field >= self.vals.len() {
            return Err(CR_ID);
        }
        Ok(self.vals[field])
    }

    pub fn immutable(&self, field: usize) -> bool {
        field == F_PID || field == F_PPID
    }
}

/// 缓存状态与查询账。
#[derive(Clone, Debug)]
pub struct CredLedger {
    pub cred: Cred,
    pub valid: bool,
    pub hits: u32,
    pub misses: u32,
    pub invalidations: u32,
    pub log: Vec<(&'static str, u32)>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CredLedger {
    pub fn new(pid: u32, ppid: u32, uid: u32, gid: u32) -> CredLedger {
        CredLedger {
            cred: Cred { vals: [pid, ppid, uid, uid, gid, gid], epoch: 1 },
            valid: false,
            hits: 0,
            misses: 0,
            invalidations: 0,
            log: Vec::new(),
            frozen: false,
            root: None,
        }
    }

    fn note(&mut self, name: &'static str, v: u32) {
        if self.log.len() < MAX_LOGS {
            self.log.push((name, v));
        }
    }

    /// 查询：未命中先计 miss 并置有效。
    pub fn lookup(&mut self, field: usize) -> Result<u32, u32> {
        if field >= FIELDS.len() {
            return Err(CR_ID);
        }
        if self.valid {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.misses = self.misses.saturating_add(1);
            self.valid = true;
        }
        let v = self.cred.get(field)?;
        self.note(FIELDS[field], v);
        Ok(v)
    }

    /// 失效：任何改写前必调。
    pub fn invalidate(&mut self, why: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        self.valid = false;
        self.invalidations = self.invalidations.saturating_add(1);
        self.note(why, self.cred.epoch);
        Ok(())
    }

    /// 改有效身份：改 euid 前必须失效；pid/ppid 不可改。
    pub fn set_euid(&mut self, v: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        self.invalidate("set_euid")?;
        self.cred.vals[F_EUID] = v;
        self.cred.epoch = self.cred.epoch.saturating_add(1);
        Ok(v)
    }

    /// 改有效组身份。
    pub fn set_egid(&mut self, v: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        self.invalidate("set_egid")?;
        self.cred.vals[F_EGID] = v;
        self.cred.epoch = self.cred.epoch.saturating_add(1);
        Ok(v)
    }

    /// 改写不可变字段：一律拒。
    pub fn rewrite_immutable(&mut self, field: usize, v: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        if field >= self.cred.vals.len() || !self.cred.immutable(field) {
            return Err(CR_SCOPE);
        }
        let _ = v;
        Err(CR_IMMUT)
    }

    pub fn fresh(&self) -> bool {
        self.hits + self.misses > 0 && self.invalidations > 0 && self.cred.epoch >= 2
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        if self.cred.vals[F_PID] == 0 || self.cred.vals[F_PPID] == 0 {
            return Err(CR_SCOPE);
        }
        if !self.fresh() {
            return Err(CR_CACHE);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("cred pid {} euid {} hits {} inv {}", self.cred.vals[0], self.cred.vals[3], self.hits, self.invalidations)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = CR_TAG;
        let mut k = 0usize;
        while k < self.cred.vals.len() {
            h = (h ^ self.cred.vals[k] as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.hits as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.misses as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.invalidations as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.cred.epoch as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 组一张查六字段、两次改写并重取的账。
pub fn build() -> CredLedger {
    let mut l = CredLedger::new(1000, 999, 1000, 1000);
    let mut k = 0usize;
    while k < FIELDS.len() {
        let _ = l.lookup(k);
        k += 1;
    }
    let _ = l.set_euid(0);
    let _ = l.set_egid(100);
    let _ = l.lookup(F_EUID);
    let _ = l.lookup(F_EGID);
    l
}

/// 四路对抗：未知字段、改 pid、改普通字段、零查询即冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = CredLedger::new(1, 1, 0, 0);
    out[0] = a.lookup(9) == Err(CR_ID);
    out[1] = a.rewrite_immutable(F_PID, 42) == Err(CR_IMMUT);
    out[2] = a.rewrite_immutable(F_UID, 42) == Err(CR_SCOPE);
    out[3] = a.freeze() == Err(CR_CACHE);
    out
}

/// 判据入口。
pub fn run_unx11232_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11232");
    let mut l = build();
    let root = l.freeze().unwrap_or(0);

    s.add("J1-六字段", l.cred.vals.len() == 6, "pid/ppid/uid/euid/gid/egid 六列齐备");
    s.add("J2-pid 稳定", l.cred.vals[F_PID] == 1000 && l.cred.vals[F_PPID] == 999, "pid/ppid 保持注册值不变");
    s.add("J3-实户主", l.cred.vals[F_UID] == 1000 && l.cred.vals[F_GID] == 1000, "真实 uid/gid 未被 set_euid 波及");
    s.add("J4-有效身份", l.cred.vals[F_EUID] == 0 && l.cred.vals[F_EGID] == 100, "euid/egid 落到改写后的值");
    s.add("J5-缓存命中", l.hits >= 2, "有效期内重复查询计命中");
    s.add("J6-首次未命中", l.misses >= 2, "每次失效后的首查计未命中");
    s.add("J7-失效两次", l.invalidations == 2 && l.cred.epoch == 3, "两次改写各失效一次、纪元递增两次");
    s.add("J8-取新值", {
        let mut x = build();
        x.lookup(F_EUID).unwrap_or(1) == 0
    }, "失效后重取 euid 得到新值而非旧值");
    s.add("J9-冻存根", root != 0 && l.root == Some(root), "凭据账冻存产出非零根");

    s.add("J10-冻后改拒", l.set_euid(1) == Err(CR_FROZEN), "冻存后改身份拒为 CR_FROZEN");
    s.add("J11-冻后查拒", l.invalidate("late") == Err(CR_FROZEN), "冻存后失效拒为 CR_FROZEN");
    s.add("J12-未知字段拒", {
        let mut x = CredLedger::new(1, 1, 0, 0);
        x.lookup(FIELDS.len()) == Err(CR_ID)
    }, "字段号越界拒为 CR_ID");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "未知字段/改 pid/改普通字段/零查询四路全拒");
    s.add_dyn(
        "J14-凭据明细",
        l.cred.vals[F_PID] == 1000 && l.cred.vals[F_EUID] == 0,
        &format!("pid {} euid {} hits {} inv {} epoch {}", l.cred.vals[F_PID], l.cred.vals[F_EUID], l.hits, l.invalidations, l.cred.epoch),
    );
    s.add_dyn(
        "J15-账根",
        root == l.fold(),
        &format!("根 0x{:x}，日志 {} 条", root, l.log.len()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11232_credential_flow() {
        let mut l = build();
        assert_eq!(l.cred.vals[F_EUID], 0);
        assert_eq!(l.lookup(F_EUID).unwrap_or(1), 0);
        let r = l.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(l.set_euid(1), Err(CR_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11232_checks_green() {
        let s = run_unx11232_checks();
        assert!(s.all_passed(), "UNX-F11232 判据未全绿");
    }
}
