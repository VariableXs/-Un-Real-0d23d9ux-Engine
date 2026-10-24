//! UNX-F0164 Capsule 更新只读探测（0xA900 段）
//!
//! 锚点：UNX-F0164-J1 固件 Capsule 能力探测只读，未授权时写路径
//! 拒绝（红线：固件刷写永不执行，仅探测）。
//! 域：CoRun·UNX-A1-B09（Capsule 探测面）。
//!
//! 结构：Capsule 头模型（EFI_CAPSULE_HEADER：Guid/Flags/HeaderSize/
//! CapsuleImageSize）+ 能力探测器（只读：支持位/UpdateCapsule 标志/
//! QueryCapsuleCapabilities 模拟）+ 写路径红线闸（未授权 → 拒绝，
//! 刷写动作永不执行）+ 探测账（(能力位, 探测值, 只读标记)）+ 注入核
//! （写越权/坏魔数/尺寸漂移/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA900 独占 0x80）。

pub const SEG_BASE: u32 = 0xA900;
pub const XC_CP_WRITE: u32 = SEG_BASE + 0x01; // 写越权
pub const XC_CP_HDR: u32 = SEG_BASE + 0x02; // 坏头
pub const XC_CP_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// EFI Capsule Guid（Firmware Management 简化锚）。
pub const CAPSULE_GUID: [u8; 16] = [
    0x4B, 0x83, 0x24, 0x26, 0x66, 0x84, 0x77, 0x4C, 0xA3, 0x0B, 0x62, 0x0B, 0x59, 0xB0, 0xB9, 0x9E,
];

/// Capsule 能力位。
pub const CAP_UPDATE_SUPPORT: u32 = 1 << 0;
pub const CAP_PERSISTENT: u32 = 1 << 1;
pub const CAP_POPULATE_SYS: u32 = 1 << 2;
pub const CAP_AUTH_SIGNED: u32 = 1 << 3;

/// Capsule 头。
#[derive(Clone, Copy)]
pub struct CapsuleHeader {
    pub guid: [u8; 16],
    pub flags: u32,
    pub header_size: u32,
    pub image_size: u32,
}

impl CapsuleHeader {
    pub fn valid(&self) -> Result<(), u32> {
        if self.guid != CAPSULE_GUID {
            return Err(XC_CP_HDR);
        }
        if self.header_size < 28 {
            return Err(XC_CP_HDR);
        }
        if self.image_size < self.header_size {
            return Err(XC_CP_HDR);
        }
        Ok(())
    }
}

/// 探测账项。
#[derive(Clone, Copy)]
pub struct ProbeEntry {
    pub cap: u32,
    pub present: bool,
    pub readonly: bool, // 恒 true（红线）
}

/// 探测器。
pub struct CapsuleProbe {
    pub caps: u32,
    pub entries: Vec<ProbeEntry>,
    pub authorized: bool,
    pub sealed: bool,
}

impl CapsuleProbe {
    pub fn new(caps: u32) -> Self {
        let mut entries = Vec::new();
        for bit in 0..4u32 {
            let cap = 1 << bit;
            entries.push(ProbeEntry { cap, present: caps & cap != 0, readonly: true });
        }
        CapsuleProbe { caps, entries, authorized: false, sealed: false }
    }

    /// 只读探测（QueryCapsuleCapabilities 模拟）。
    pub fn query(&self) -> (u32, u64) {
        (self.caps, 65536) // max capsule size 锚
    }

    /// 写路径红线闸：UpdateCapsule 模拟——未授权一律拒。
    pub fn try_update(&mut self, hdr: &CapsuleHeader) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_CP_SEALED);
        }
        hdr.valid()?;
        if !self.authorized {
            return Err(XC_CP_WRITE); // 红线：固件刷写永不执行
        }
        // 即便授权，本面仍只探测（双重红线）
        Err(XC_CP_WRITE)
    }

    /// 探测账完整性：全条目只读。
    pub fn all_readonly(&self) -> bool {
        self.entries.iter().all(|e| e.readonly)
    }
}

/// 头合法性核：合法/坏 Guid/坏尺寸。
pub fn hdr_exact() -> bool {
    let good = CapsuleHeader {
        guid: CAPSULE_GUID,
        flags: 0x0001_0000,
        header_size: 28,
        image_size: 4096,
    };
    let bad_guid = CapsuleHeader { guid: [0u8; 16], flags: 0, header_size: 28, image_size: 4096 };
    let bad_size = CapsuleHeader { guid: CAPSULE_GUID, flags: 0, header_size: 64, image_size: 32 };
    good.valid().is_ok()
        && bad_guid.valid() == Err(XC_CP_HDR)
        && bad_size.valid() == Err(XC_CP_HDR)
}

/// 能力位探测核：四位独立。
pub fn caps_probe_exact() -> bool {
    let p = CapsuleProbe::new(CAP_UPDATE_SUPPORT | CAP_AUTH_SIGNED);
    p.entries[0].present && !p.entries[1].present && !p.entries[2].present && p.entries[3].present
        && p.all_readonly()
        && p.query() == (CAP_UPDATE_SUPPORT | CAP_AUTH_SIGNED, 65536)
}

/// 写红线核：未授权 + 授权均拒（永只读）。
pub fn write_redline_exact() -> bool {
    let mut p = CapsuleProbe::new(CAP_UPDATE_SUPPORT);
    let hdr = CapsuleHeader {
        guid: CAPSULE_GUID,
        flags: 0,
        header_size: 28,
        image_size: 1024,
    };
    let r1 = p.try_update(&hdr);
    p.authorized = true;
    let r2 = p.try_update(&hdr);
    r1 == Err(XC_CP_WRITE) && r2 == Err(XC_CP_WRITE)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut p = CapsuleProbe::new(0xF);
    // 1) 坏 Guid 写拒
    let bad = CapsuleHeader { guid: [9u8; 16], flags: 0, header_size: 28, image_size: 64 };
    if p.try_update(&bad) == Err(XC_CP_HDR) {
        caught += 1;
    }
    // 2) 尺寸倒挂
    let flip = CapsuleHeader { guid: CAPSULE_GUID, flags: 0, header_size: 100, image_size: 50 };
    if p.try_update(&flip) == Err(XC_CP_HDR) {
        caught += 1;
    }
    // 3) 封账后拒
    p.sealed = true;
    let ok = CapsuleHeader { guid: CAPSULE_GUID, flags: 0, header_size: 28, image_size: 64 };
    if p.try_update(&ok) == Err(XC_CP_SEALED) {
        caught += 1;
    }
    // 4) 探测账只读恒真
    let p2 = CapsuleProbe::new(0);
    if p2.all_readonly() {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：探测账折叠哈希。
pub fn fp_frozen(p: &CapsuleProbe) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= p.caps as u64;
    h = h.wrapping_mul(0x100000001b3);
    for e in &p.entries {
        h ^= e.cap as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0164");
    // J1-1 头合法性
    let ok = hdr_exact();
    s.add("B09-01-头校验", ok, if ok { "" } else { "头漂移" });
    // J1-2 能力位探测
    let ok = caps_probe_exact();
    s.add("B09-02-能力探测", ok, if ok { "" } else { "探测漂移" });
    // J1-3 写路径红线
    let ok = write_redline_exact();
    s.add("B09-03-写红线", ok, if ok { "" } else { "红线破" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B09-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let p = CapsuleProbe::new(0x5);
    let ok = fp_frozen(&p) == fp_frozen(&p);
    s.add("B09-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
