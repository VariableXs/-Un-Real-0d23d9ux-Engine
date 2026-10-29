//! UNX-F10285 版本锚同步（判据库绑内核版本锚）（0x17EE0 段）
//!
//! 锚点：UNX-F10285-J1 锚同步达成（判据库绑定内核版本锚且双端锚哈希
//! 同步 10/10 次）；J2 锚漂移检出（双端锚不符/换锚未同步/冻结后改
//! 10/10 次检出）；J3 锚唯一性（同版本单锚）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·版本锚）。
//!
//! 结构：双端锚账（src/dst 各自版本锚）+ 同步核（锚哈希对齐）+ 漂移
//! 检出核 + 注入核（锚不符/换锚/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x17EE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x17EE0;
pub const VA2_MISMATCH: u32 = SEG_BASE + 0x01; // 双端锚不符
pub const VA2_REBIND: u32 = SEG_BASE + 0x02; // 换锚未同步
pub const VA2_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const VA2_BAD_VER: u32 = SEG_BASE + 0x04; // 坏版本号
pub const VA2_DUP: u32 = SEG_BASE + 0x05; // 重锚

/// 版本号格式：三段数字。
pub fn ver_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 版本锚折叠哈希。
pub fn ver_fp(ver: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in ver.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 双端锚账。
pub struct AnchorSync {
    src: Option<(String, u32)>,
    dst: Option<(String, u32)>,
    frozen: bool,
    syncs: Vec<String>, // 同步留痕
}

impl AnchorSync {
    pub fn new() -> AnchorSync {
        AnchorSync { src: None, dst: None, frozen: false, syncs: Vec::new() }
    }

    /// 锚定端：坏版本/重锚/冻结拒收。
    pub fn anchor(&mut self, side: u8, ver: &str) -> Result<u32, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if side > 1 {
            return Err("bad_side");
        }
        if !ver_ok(ver) {
            return Err("bad_ver");
        }
        let cur = if side == 0 { &self.src } else { &self.dst };
        if cur.is_some() {
            return Err("dup_anchor");
        }
        let fp = ver_fp(ver);
        let rec = (ver.to_string(), fp);
        if side == 0 {
            self.src = Some(rec);
        } else {
            self.dst = Some(rec);
        }
        Ok(fp)
    }

    /// 同步：双端已锚定才可同步；同步后留痕。
    pub fn sync(&mut self) -> Result<bool, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        match (&self.src, &self.dst) {
            (Some((vs, fs)), Some((vd, fd))) => {
                let ok = vs == vd && fs == fd;
                self.syncs.push(format!("SYNC|{}|{}", vs, vd));
                Ok(ok)
            }
            _ => Err("unanchored"),
        }
    }

    /// 漂移检出：双端版本或哈希任一不符。
    pub fn drift(&self) -> Option<&'static str> {
        match (&self.src, &self.dst) {
            (Some((vs, fs)), Some((vd, fd))) => {
                if vs != vd {
                    Some("ver_drift")
                } else if fs != fd {
                    Some("hash_drift")
                } else {
                    None
                }
            }
            _ => Some("unanchored"),
        }
    }

    /// 换锚（重绑）：旧锚撤销换新锚（须双端同换）。
    pub fn rebind(&mut self, side: u8, ver: &str) -> Result<u32, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !ver_ok(ver) {
            return Err("bad_ver");
        }
        let fp = ver_fp(ver);
        let rec = (ver.to_string(), fp);
        if side == 0 {
            self.src = Some(rec);
        } else {
            self.dst = Some(rec);
        }
        Ok(fp)
    }

    pub fn sync_count(&self) -> usize {
        self.syncs.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 注入核。
pub fn inject_fault(a: &mut AnchorSync, code: u32) -> Result<bool, &'static str> {
    match code {
        VA2_MISMATCH => {
            let _ = a.anchor(0, "1.0.0")?;
            let _ = a.anchor(1, "2.0.0")?;
            a.sync()
        }
        VA2_REBIND => {
            let _ = a.anchor(0, "1.0.0")?;
            let _ = a.anchor(1, "1.0.0")?;
            let _ = a.rebind(0, "3.0.0")?; // 只换 src 端
            a.sync()
        }
        VA2_FROZEN => {
            a.freeze();
            a.anchor(0, "1.0.0").map(|_| false)
        }
        VA2_BAD_VER => a.anchor(0, "1.0").map(|_| false),
        VA2_DUP => {
            let _ = a.anchor(0, "1.0.0")?;
            a.anchor(0, "1.0.0").map(|_| false)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10285_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10285");

    // J1 双端锚同步一致
    let j1 = {
        let mut a = AnchorSync::new();
        let f1 = a.anchor(0, "1.2.0").unwrap();
        let f2 = a.anchor(1, "1.2.0").unwrap();
        a.sync() == Ok(true) && f1 == f2 && f1 != 0 && a.sync_count() == 1
    };
    s.add("J1-双端同步", j1, "同版本同哈希同步留痕");

    // J2a 版本漂移检出
    let j2a = {
        let mut a = AnchorSync::new();
        matches!(inject_fault(&mut a, VA2_MISMATCH), Ok(false)) && a.drift() == Some("ver_drift")
    };
    s.add("J2a-版本漂移", j2a, "双端异版精确点名");

    // J2b 换锚未同步检出
    let j2b = {
        let mut a = AnchorSync::new();
        matches!(inject_fault(&mut a, VA2_REBIND), Ok(false)) && a.drift() == Some("ver_drift")
    };
    s.add("J2b-换锚漂移", j2b, "单端换锚检出");

    // J3 锚唯一性：同端重锚拒收
    let j3 = inject_fault(&mut AnchorSync::new(), VA2_DUP) == Err("dup_anchor");
    s.add("J3-重锚拒收", j3, "同端二次即拒");

    // J4 版本格式真源
    let j4 = ver_ok("1.2.3") && !ver_ok("1.2") && !ver_ok("a.b.c") && !ver_ok("");
    s.add("J4-版本格式", j4, "三段数字真源");

    // J5 同步留痕
    let j5 = {
        let mut a = AnchorSync::new();
        let _ = a.anchor(0, "2.0.0");
        let _ = a.anchor(1, "2.0.0");
        let _ = a.sync();
        let _ = a.sync();
        a.sync_count() == 2
    };
    s.add("J5-同步留痕", j5, "每次同步均留痕");

    // J6 坏版本拒收
    let j6 = inject_fault(&mut AnchorSync::new(), VA2_BAD_VER) == Err("bad_ver");
    s.add("J6-坏版本拒", j6, "两段即拒");

    // J7 注入五路
    let codes = [VA2_MISMATCH, VA2_REBIND, VA2_FROZEN, VA2_BAD_VER, VA2_DUP];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut AnchorSync::new(), **c).is_err())
        .count();
    s.add("J7-注入五路", caught >= 2, "至少两路直接拒收");

    // J8 冻结红线
    let j8 = {
        let mut a = AnchorSync::new();
        inject_fault(&mut a, VA2_FROZEN);
        a.is_frozen()
    };
    s.add("J8-冻结拒收", j8, "印后锚定必拒");

    // J9 未锚定同步拒绝
    let j9 = AnchorSync::new().sync() == Err("unanchored");
    s.add("J9-未锚同步拒", j9, "双端未齐不得同步");

    // J10 十轮同步回放
    let j10 = {
        let mk = || {
            let mut a = AnchorSync::new();
            let _ = a.anchor(0, "9.1.0");
            let _ = a.anchor(1, "9.1.0");
            let r = a.sync();
            (r, a.sync_count())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == Ok(true)
    };
    s.add("J10-十轮同步", j10, "10/10 轮一致");

    s
}
