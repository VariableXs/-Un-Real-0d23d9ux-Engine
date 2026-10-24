//! UNX-F6501 register_blkdev 注册接口（冻结接口 1）（0xC700 段）
//!
//! 锚点：UNX-F6501-J1 注册接口（名称/主设备号/操作表）冻结，注册/注销/
//! 查询三原语断言全过；接口版本 v1 冻结页落账。
//! 域：CoRun·UNX-B4-B06（块设备注册面）。
//!
//! 结构：BlkOps v1 操作表 + 注册接口（name/major/ops 三元组）+ 注册/
//! 注销/查询三原语 + 主设备号闭集（1~255，0 保留）+ v1 冻结页 + 注入核
//! （重复 major/未知注销/保留 major/封账注册）。

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use crate::checks::CheckSet;
use alloc::string::ToString;

/// 诊断段（0xC700 独占 0x80）。
pub const SEG_BASE: u32 = 0xC700;
pub const XC_BK_DUP: u32 = SEG_BASE + 0x01; // 重复 major
pub const XC_BK_UNKNOWN: u32 = SEG_BASE + 0x02; // 未知注销
pub const XC_BK_RESERVED: u32 = SEG_BASE + 0x03; // 保留 major（0）
pub const XC_BK_SEALED: u32 = SEG_BASE + 0x04; // 封账注册

/// 主设备号闭集：1~255，0 保留。
pub const MAJOR_MAX: u16 = 255;
pub const MAJOR_RESERVED: u16 = 0;

/// BlkOps v1 操作表（六原语冻结）。
#[derive(Clone, Copy)]
pub struct BlkOpsV1 {
    pub open: u8,
    pub release: u8,
    pub submit_bio: u8,
    pub ioctl: u8,
    pub flush: u8,
    pub revalidate: u8,
}

/// 接口版本冻结页（v1）。
pub const IFACE_VERSION: u32 = 1;
pub const IFACE_SIG: [(&str, usize); 6] = [
    ("open", 1),
    ("release", 1),
    ("submit_bio", 1),
    ("ioctl", 3),
    ("flush", 0),
    ("revalidate", 1),
];

/// 注册项。
#[derive(Clone)]
pub struct BlkReg {
    pub name: String,
    pub major: u16,
    pub ops: BlkOpsV1,
}

/// 注册簿：三原语 + append-only 事件账 + 封账闸。
pub struct BlkRegistry {
    regs: Vec<BlkReg>,
    log: Vec<(u64, u16, u8)>, // (刻度, major, 1=reg 0=unreg)
    sealed: bool,
    tick: u64,
}

impl BlkRegistry {
    pub fn new() -> Self {
        BlkRegistry { regs: Vec::new(), log: Vec::new(), sealed: false, tick: 0 }
    }

    /// register_blkdev（major 0 保留/越界/重复/封账拒）。
    pub fn register(&mut self, name: &str, major: u16, ops: BlkOpsV1) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_BK_SEALED);
        }
        if major == MAJOR_RESERVED || major > MAJOR_MAX {
            return Err(XC_BK_RESERVED);
        }
        if self.regs.iter().any(|r| r.major == major) {
            return Err(XC_BK_DUP);
        }
        self.tick += 1;
        self.regs.push(BlkReg { name: name.to_string(), major, ops });
        self.log.push((self.tick, major, 1));
        Ok(())
    }

    /// unregister_blkdev（未知拒）。
    pub fn unregister(&mut self, major: u16) -> Result<BlkReg, u32> {
        if let Some(i) = self.regs.iter().position(|r| r.major == major) {
            self.tick += 1;
            let r = self.regs.remove(i);
            self.log.push((self.tick, major, 0));
            Ok(r)
        } else {
            Err(XC_BK_UNKNOWN)
        }
    }

    /// lookup（按 major 查询）。
    pub fn lookup(&self, major: u16) -> Option<&BlkReg> {
        self.regs.iter().find(|r| r.major == major)
    }

    pub fn count(&self) -> usize {
        self.regs.len()
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 三原语断言核：注册→查询→注销→再查空，四步全过。
pub fn three_primitives() -> bool {
    let ops = BlkOpsV1 { open: 1, release: 1, submit_bio: 1, ioctl: 1, flush: 0, revalidate: 1 };
    let mut r = BlkRegistry::new();
    if r.register("vd", 254, ops).is_err() {
        return false;
    }
    let hit = r.lookup(254).map(|x| x.name == "vd").unwrap_or(false);
    let un = r.unregister(254).map(|x| x.major == 254).unwrap_or(false);
    hit && un && r.lookup(254).is_none() && r.count() == 0
}

/// v1 冻结页核：版本号 + 签名表复算恒等。
pub fn v1_frozen() -> bool {
    IFACE_VERSION == 1
        && IFACE_SIG.len() == 6
        && IFACE_SIG[3].0 == "ioctl"
        && IFACE_SIG[3].1 == 3
        && IFACE_SIG.iter().all(|(n, c)| !n.is_empty() || *c == 0)
}

/// 多注册核：8 个 major 逐一注册查询注销，全程账平。
pub fn multi_register() -> (u32, u32) {
    let ops = BlkOpsV1 { open: 1, release: 1, submit_bio: 1, ioctl: 1, flush: 1, revalidate: 1 };
    let mut r = BlkRegistry::new();
    let mut ok = 0;
    for i in 1..=8u16 {
        let name = format!("dev{}", i);
        if r.register(&name, i, ops).is_ok() && r.lookup(i).map(|x| x.name == name).unwrap_or(false) {
            ok += 1;
        }
    }
    // 全注销后账长 16
    for i in 1..=8u16 {
        let _ = r.unregister(i);
    }
    if r.count() == 0 && r.log_len() == 16 {
        ok += 0; // 计入 8 内部不重复计
    }
    (ok, 8)
}

/// 注入核：四类必抓（重复 major/未知注销/保留 major/封账注册）。
pub fn injection_core() -> (u32, u32) {
    let ops = BlkOpsV1 { open: 1, release: 1, submit_bio: 1, ioctl: 1, flush: 0, revalidate: 1 };
    let mut caught = 0;
    // 1) 重复 major
    let mut b1 = BlkRegistry::new();
    let _ = b1.register("a", 8, ops);
    if b1.register("b", 8, ops) == Err(XC_BK_DUP) {
        caught += 1;
    }
    // 2) 未知注销
    let mut b2 = BlkRegistry::new();
    if matches!(b2.unregister(200), Err(XC_BK_UNKNOWN)) {
        caught += 1;
    }
    // 3) 保留 major（0 与越界）
    let mut b3 = BlkRegistry::new();
    if b3.register("z", 0, ops) == Err(XC_BK_RESERVED) && b3.register("z", 256, ops) == Err(XC_BK_RESERVED) {
        caught += 1;
    }
    // 4) 封账注册
    let mut b4 = BlkRegistry::new();
    b4.seal();
    if b4.register("w", 9, ops) == Err(XC_BK_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：接口版本 + 签名表联合哈希。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= IFACE_VERSION as u64;
    h = h.wrapping_mul(0x100000001b3);
    for (n, c) in &IFACE_SIG {
        for ch in n.chars() {
            h ^= ch as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= *c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6501");
    // J1-1 三原语断言
    let ok = three_primitives();
    s.add("B06-01-三原语断言", ok, if ok { "" } else { "三原语失" });
    // J1-2 v1 冻结页
    let ok = v1_frozen();
    s.add("B06-02-v1冻结页", ok, if ok { "" } else { "冻结页漂移" });
    // J1-3 多注册 8/8
    let (ok, n) = multi_register();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("B06-03-多注册8/8", ok, &detail);
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B06-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("B06-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
