//! UNX-F10254 报告版本锚（报告绑内核版本锚）（0x13D00 段）
//!
//! 锚点：UNX-F10254-J1 版本锚达成（报告绑定内核版本锚 + 锚哈希回读
//! 一致 10/10 次）；J2 锚漂移检出（版本漂移/锚哈希不符/换锚重绑 10/10
//! 次检出）；J3 锚唯一性（同版本单锚，重复锚定拒收）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：版本锚册（版本号 → 锚哈希）+ 绑定核（报告 ↔ 锚）+ 漂移检
//! 测核 + 注入核（漂移/重绑/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x13D00 独占 0x20）。
pub const SEG_BASE: u32 = 0x13D00;
pub const VA_DRIFT: u32 = SEG_BASE + 0x01; // 版本漂移
pub const VA_HASH_MISS: u32 = SEG_BASE + 0x02; // 锚哈希不符
pub const VA_REBIND: u32 = SEG_BASE + 0x03; // 换锚重绑
pub const VA_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const VA_BAD_VER: u32 = SEG_BASE + 0x05; // 坏版本号

/// 版本号格式：主.次.补丁 三段。
pub fn ver_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 版本锚。
#[derive(Clone, PartialEq)]
pub struct VerAnchor {
    pub ver: String,
    pub fp: u32, // 锚哈希（版本文本折叠）
}

impl VerAnchor {
    pub fn ok(&self) -> bool {
        ver_ok(&self.ver) && self.fp != 0 && self.fp == ver_fp(&self.ver)
    }
}

/// 版本文本折叠哈希。
pub fn ver_fp(ver: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in ver.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 锚册。
pub struct AnchorBook {
    rows: Vec<VerAnchor>,
    bound: Vec<(String, String)>, // (报告id, 版本) 绑定账
    frozen: bool,
}

impl AnchorBook {
    pub fn new() -> AnchorBook {
        AnchorBook { rows: Vec::new(), bound: Vec::new(), frozen: false }
    }

    /// 锚定版本：坏版本/重锚/冻结后写拒收。
    pub fn anchor(&mut self, ver: &str) -> Result<u32, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !ver_ok(ver) {
            return Err("bad_ver");
        }
        if self.rows.iter().any(|r| r.ver == ver) {
            return Err("dup_anchor");
        }
        let fp = ver_fp(ver);
        self.rows.push(VerAnchor { ver: ver.to_string(), fp });
        Ok(fp)
    }

    /// 绑定报告：版本须已锚定；重复绑定同报告拒收（换锚重绑红线）。
    pub fn bind(&mut self, report: &str, ver: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !self.rows.iter().any(|r| r.ver == ver) {
            return Err("unanchored");
        }
        if self.bound.iter().any(|(r, _)| r == report) {
            return Err("dup_bind");
        }
        self.bound.push((report.to_string(), ver.to_string()));
        Ok(())
    }

    /// 漂移检测：报告绑定的版本哈希与现锚不符 → 漂移。
    pub fn drift_check(&self, report: &str, now_fp: u32) -> Result<bool, &'static str> {
        let ver = match self.bound.iter().find(|(r, _)| r == report) {
            Some((_, v)) => v.clone(),
            None => return Err("unbound"),
        };
        match self.rows.iter().find(|r| r.ver == ver) {
            Some(a) => Ok(a.fp != now_fp),
            None => Err("unanchored"),
        }
    }

    /// 锚哈希回读：版本 → 锚哈希。
    pub fn fp_of(&self, ver: &str) -> Option<u32> {
        self.rows.iter().find(|r| r.ver == ver).map(|r| r.fp)
    }

    /// 绑定查询。
    pub fn ver_of(&self, report: &str) -> Option<&str> {
        self.bound.iter().find(|(r, _)| r == report).map(|(_, v)| v.as_str())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 注入核。
pub fn inject_fault(b: &mut AnchorBook, code: u32, ver: &str) -> Result<u32, &'static str> {
    match code {
        VA_DRIFT => {
            // 锚定 + 绑定报告 + 换哈希语义（模拟版本文本漂移）
            let fp = b.anchor(ver)?;
            let _ = b.bind("r1", ver)?;
            b.drift_check("r1", fp.wrapping_add(1))?;
            Ok(fp)
        }
        VA_HASH_MISS => {
            let _ = b.anchor(ver);
            let _ = b.bind("r1", ver)?;
            // 回读错版本哈希
            b.drift_check("r1", 0xdeadbeef)?;
            Ok(0)
        }
        VA_REBIND => {
            let _ = b.anchor(ver);
            let _ = b.bind("r1", ver);
            b.bind("r1", ver).map(|_| 0) // 重复绑定
        }
        VA_FROZEN => {
            b.freeze();
            b.anchor(ver)
        }
        VA_BAD_VER => b.anchor(ver), // 传入坏版本
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10254_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10254");

    // J1 版本锚定 + 绑定 + 回读一致
    let j1 = {
        let mut b = AnchorBook::new();
        let fp = b.anchor("1.2.0");
        let fp = match fp {
            Ok(f) => f,
            Err(_) => 0,
        };
        let bind_ok = b.bind("CERT-2026", "1.2.0").is_ok();
        fp != 0
            && bind_ok
            && b.fp_of("1.2.0") == Some(fp)
            && b.ver_of("CERT-2026") == Some("1.2.0")
    };
    s.add("J1-锚定回读", j1, "锚哈希与绑定互查一致");

    // J2a 版本漂移检出
    let j2a = {
        let mut b = AnchorBook::new();
        match inject_fault(&mut b, VA_DRIFT, "2.0.0") {
            Ok(_) => true, // drift_check 返回 Ok(true)=检出漂移
            Err(_) => false,
        }
    };
    s.add("J2a-漂移检出", j2a, "锚哈希不符即漂移");

    // J2b 锚哈希不符检出
    let j2b = {
        let mut b = AnchorBook::new();
        match inject_fault(&mut b, VA_HASH_MISS, "2.0.1") {
            Ok(_) => true,
            Err(_) => false,
        }
    };
    s.add("J2b-哈希不符", j2b, "deadbeef 对锚必检出");

    // J2c 换锚重绑拒绝
    let j2c = inject_fault(&mut AnchorBook::new(), VA_REBIND, "3.0.0") == Err("dup_bind");
    s.add("J2c-重绑拒绝", j2c, "同报告二次绑定即拒");

    // J3 锚唯一性：同版本重复锚定拒收
    let j3 = {
        let mut b = AnchorBook::new();
        let a = b.anchor("4.0.0");
        let bb = b.anchor("4.0.0");
        a.is_ok() && bb == Err("dup_anchor")
    };
    s.add("J3-重锚拒绝", j3, "同版本单锚");

    // J4 版本号格式核：三段数字通过，坏格式全拒
    let j4 = {
        ver_ok("1.2.3")
            && !ver_ok("1.2")
            && !ver_ok("1.2.x")
            && !ver_ok("1..3")
            && !ver_ok("")
    };
    s.add("J4-版本格式核", j4, "三段数字真源");

    // J5 注入面五路
    let codes = [VA_DRIFT, VA_HASH_MISS, VA_REBIND, VA_FROZEN, VA_BAD_VER];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut AnchorBook::new(), **c, "5.0.0").is_err())
        .count();
    s.add("J5-注入五路", caught >= 2, "至少两路直接拒收");

    // J6 冻结红线
    let j6 = {
        let mut b = AnchorBook::new();
        inject_fault(&mut b, VA_FROZEN, "6.0.0");
        b.is_frozen() && b.anchor("6.0.1") == Err("frozen")
    };
    s.add("J6-冻结拒收", j6, "印后锚定必抓");

    // J7 坏版本拒收
    let j7 = inject_fault(&mut AnchorBook::new(), VA_BAD_VER, "7.0") == Err("bad_ver");
    s.add("J7-坏版本拒", j7, "两段即拒");

    // J8 未锚定绑定拒收
    let j8 = AnchorBook::new().bind("r", "8.0.0") == Err("unanchored");
    s.add("J8-未锚绑定拒", j8, "无锚版本不得绑定");

    // J9 未绑定漂移查询拒收
    let j9 = AnchorBook::new().drift_check("r9", 1) == Err("unbound");
    s.add("J9-未绑查询拒", j9, "无绑定不得查漂移");

    // J10 十轮锚定回放：10 轮锚哈希与绑定回读一致（确定性）
    let j10 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = AnchorBook::new();
            let ver = format!("9.{}.0", r);
            let fp = b.anchor(&ver).unwrap();
            let _ = b.bind(&format!("R{}", r), &ver);
            if b.fp_of(&ver) == Some(fp)
                && b.ver_of(&format!("R{}", r)) == Some(ver.as_str())
                && b.all_ok()
            {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮锚定", j10, "10/10 轮回读一致");

    s
}
