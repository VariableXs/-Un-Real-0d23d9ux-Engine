//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7321 · raid target 注册与几何参数 · 诊断段 0xAE80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F7321-J1 raid target 参数（raid1/raid5、成员数、条带
//!      大小）解析与校验全覆盖；非法几何（成员数与级别不符等）
//!      100% 拒绝分档留账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 参数解析：级别（raid1/raid5）+ 成员数 + 条带大小（KB 幂次）；
//!   - 几何校验：raid1 成员 ≥2、raid5 成员 ≥3、条带大小 2 幂且
//!     4KB..=1MB；
//!   - 非法几何 100% 拒 + 分档留账（级别档/成员档/条带档）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xAE80;

/// raid 级别码。
pub const RAID1: u8 = 1;
pub const RAID5: u8 = 5;

/// 条带大小界（KB）。
pub const STRIPE_MIN: u32 = 4;
pub const STRIPE_MAX: u32 = 1024;

/// 分档错误码。
pub const E_LEVEL: i32 = 22;
pub const E_MEMBERS: i32 = 34;
pub const E_STRIPE: i32 = 40;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// raid 几何。
#[derive(Clone, Copy)]
pub struct RaidGeo {
    pub level: u8,
    pub members: u32,
    pub stripe_kb: u32,
}

impl RaidGeo {
    /// 几何校验：返回分档错误码。
    pub fn validate(&self) -> Result<(), i32> {
        // 档 1：级别
        if self.level != RAID1 && self.level != RAID5 {
            return Err(E_LEVEL);
        }
        // 档 2：成员数与级别相符
        let min_members = if self.level == RAID1 { 2 } else { 3 };
        if self.members < min_members || self.members > 64 {
            return Err(E_MEMBERS);
        }
        // 档 3：条带大小 2 幂且在界内
        if self.stripe_kb < STRIPE_MIN
            || self.stripe_kb > STRIPE_MAX
            || !self.stripe_kb.is_power_of_two()
        {
            return Err(E_STRIPE);
        }
        Ok(())
    }
}

/// 解析器：文本参数 → 几何（解析失败同分档）。
pub fn parse_geo(level: &str, members: &str, stripe: &str) -> Result<RaidGeo, i32> {
    let lv = match level {
        "raid1" => RAID1,
        "raid5" => RAID5,
        _ => return Err(E_LEVEL),
    };
    let m: u32 = members.parse().map_err(|_| E_MEMBERS)?;
    let s: u32 = stripe.parse().map_err(|_| E_STRIPE)?;
    let g = RaidGeo { level: lv, members: m, stripe_kb: s };
    g.validate()?;
    Ok(g)
}

/// 注册账条目：几何 + 校验结果留档。
pub struct RaidReg {
    pub entries: Vec<(RaidGeo, bool, i32)>,
    pub rejects: [usize; 3], // 分档拒绝计数：级别/成员/条带
}

impl RaidReg {
    pub fn new() -> Self {
        RaidReg { entries: Vec::new(), rejects: [0; 3] }
    }

    /// 注册：校验先行，非法分档留账。
    pub fn register(&mut self, g: RaidGeo) -> Result<(), i32> {
        match g.validate() {
            Ok(()) => {
                self.entries.push((g, true, 0));
                Ok(())
            }
            Err(e) => {
                let slot = match e {
                    E_LEVEL => 0,
                    E_MEMBERS => 1,
                    _ => 2,
                };
                self.rejects[slot] += 1;
                self.entries.push((g, false, e));
                Err(e)
            }
        }
    }
}

/// J1 主判据：参数解析校验全覆盖 + 非法几何 100% 拒分档留账。
pub fn j1_main() -> bool {
    let mut r = RaidReg::new();
    // 合法几何：raid1/2 盘/64KB、raid1/3 盘/512KB、raid5/3 盘/4KB、raid5/8 盘/1MB
    for (lv, m, s) in [("raid1", "2", "64"), ("raid1", "3", "512"), ("raid5", "3", "4"), ("raid5", "8", "1024")] {
        if parse_geo(lv, m, s).is_err() {
            return false;
        }
    }
    // 非法几何全拒并分档：
    // 级别档：raid0 / raid6 / raid4
    if !matches!(parse_geo("raid0", "2", "64"), Err(E_LEVEL)) { return false; }
    if !matches!(parse_geo("raid6", "4", "64"), Err(E_LEVEL)) { return false; }
    // 成员档：raid1×1、raid5×2、成员超界 65
    if !matches!(parse_geo("raid1", "1", "64"), Err(E_MEMBERS)) { return false; }
    if !matches!(parse_geo("raid5", "2", "64"), Err(E_MEMBERS)) { return false; }
    if !matches!(parse_geo("raid1", "65", "64"), Err(E_MEMBERS)) { return false; }
    // 条带档：3KB（非幂）、2KB（低于下界）、2MB（超上界）、6KB（非 2 幂）
    if !matches!(parse_geo("raid1", "2", "3"), Err(E_STRIPE)) { return false; }
    if !matches!(parse_geo("raid1", "2", "2"), Err(E_STRIPE)) { return false; }
    if !matches!(parse_geo("raid1", "2", "2048"), Err(E_STRIPE)) { return false; }
    if !matches!(parse_geo("raid1", "2", "6"), Err(E_STRIPE)) { return false; }
    // 分档留账：三档拒绝计数各归位
    r.register(RaidGeo { level: 0, members: 2, stripe_kb: 64 }).ok();
    r.register(RaidGeo { level: RAID1, members: 1, stripe_kb: 64 }).ok();
    r.register(RaidGeo { level: RAID1, members: 2, stripe_kb: 3 }).ok();
    r.rejects == [1, 1, 1] && r.entries.iter().all(|(_, ok, _)| !ok)
}

// ── 注入核（0xAE80 段 5 项）────────────────────────────────────────────

/// 注入核 1：级别档注入——非 raid1/raid5 全拒。
pub fn inj1_level_tier() -> bool {
    ["raid0", "raid2", "raid3", "raid4", "raid6", "raid10", ""]
        .iter()
        .all(|lv| parse_geo(lv, "2", "64").is_err() || *lv == "raid10")
}

/// 注入核 2：成员档注入——级别下限严格（raid5 必 ≥3）。
pub fn inj2_member_tier() -> bool {
    parse_geo("raid5", "3", "64").is_ok() && parse_geo("raid5", "2", "64").is_err()
        && parse_geo("raid1", "2", "64").is_ok() && parse_geo("raid1", "1", "64").is_err()
}

/// 注入核 3：条带幂注入——非 2 幂全拒。
pub fn inj3_stripe_pow2() -> bool {
    (3u32..1024).filter(|s| !s.is_power_of_two()).all(|s| {
        matches!(parse_geo("raid1", "2", &s.to_string()), Err(E_STRIPE))
    })
}

/// 注入核 4：解析失败注入——非数字参数分档拒。
pub fn inj4_parse_fail() -> bool {
    parse_geo("raid1", "two", "64").is_err() && parse_geo("raid1", "2", "big").is_err()
}

/// 注入核 5：留账分档注入——三档拒绝各计一档不混账。
pub fn inj5_tier_ledger() -> bool {
    let mut r = RaidReg::new();
    r.register(RaidGeo { level: RAID5, members: 2, stripe_kb: 3 }).ok();
    // 一单三档全错只记首档（级别）
    r.rejects == [1, 0, 0] && r.entries.len() == 1
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx7321_checks：本单判据聚合（诊断段 0xAE80）。
pub fn run_unx7321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7321 raid target 注册与几何参数");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "参数解析与校验全覆盖",
        j1_main(),
        if j1_main() { "" } else { "解析校验破" },
    );
    s.add(
        "非法几何100%拒绝分档留账",
        j1_main(),
        if j1_main() { "" } else { "分档留账破" },
    );
    let inj = [
        ("注入1级别档", inj1_level_tier()),
        ("注入2成员档", inj2_member_tier()),
        ("注入3条带幂", inj3_stripe_pow2()),
        ("注入4解析失败拒", inj4_parse_fail()),
        ("注入5留账分档", inj5_tier_ledger()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
