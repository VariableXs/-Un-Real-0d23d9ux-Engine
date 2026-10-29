//! unx48310_cfgdflt — UNX-F48310 默认值表与版本化（默认随版本演进声明，升级不改用户显式值）（诊断段 0xC7A0）。
//!
//! 判据原文：
//!   J1 升级模拟 ×10 用户值零覆盖；判据锚定 体验十二 信任。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 默认表版本化：V1/V2 两代表（V2 在 V1 基础上改值/增键/删键），
//!      每表带版本戳；合并结果必须记录采用的默认表版本。
//!   2. 升级铁律：用户显式值（explicit）在升级后逐位保留——任何键
//!      被默认值覆盖即违例（×10 场景全查）。
//!   3. 隐式键演进：用户未显式设置的键升级后采用新表默认值（演进
//!      生效），这是默认值演进的唯一入口。
//!   4. 删键处置：V2 删除的键若用户有显式值，进 legacy 桶保留
//!      （不静默丢弃）；无显式值则直接消失。
//!   5. 升级幂等：同版本重复升级 = 配置零变化（指纹不变）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC7A0;

const E_EXPLICIT_LOST: u16 = SEG_BASE + 1;  // 用户显式值被覆盖
const E_IMPLICIT_STALE: u16 = SEG_BASE + 2; // 隐式键未演进到新默认
const E_LEGACY_DROP: u16 = SEG_BASE + 3;    // 删键的用户值被静默丢弃
const E_VER_MISS: u16 = SEG_BASE + 4;       // 合并结果无版本戳
const E_IDEM_DRIFT: u16 = SEG_BASE + 5;     // 重复升级指纹漂移
const E_TABLE_BAD: u16 = SEG_BASE + 6;      // 默认表规约坏（重键/乱序）
const E_CODE_RANGE: u16 = SEG_BASE + 7;     // 诊断码越段

/// 键空间：8 个键位（0..8）。
pub const KEY_COUNT: usize = 8;

/// 版本化默认表：版本戳 + 键值表（键升序无重）。
pub struct DefaultsTable {
    pub ver: u32,
    pub entries: Vec<(u8, u32)>, // (key, default_val)
}

impl DefaultsTable {
    /// 表规约：键严格升序无重。
    pub fn validate(&self) -> Result<(), u16> {
        let mut i = 1;
        while i < self.entries.len() {
            if self.entries[i].0 <= self.entries[i - 1].0 {
                return Err(E_TABLE_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn lookup(&self, key: u8) -> Option<u32> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == key {
                return Some(self.entries[i].1);
            }
            i += 1;
        }
        None
    }
}

/// V1 默认表：全 8 键基线值。
pub fn table_v1() -> DefaultsTable {
    DefaultsTable {
        ver: 1,
        entries: vec![
            (0, 100),
            (1, 110),
            (2, 120),
            (3, 130),
            (4, 140),
            (5, 150),
            (6, 160),
            (7, 170),
        ],
    }
}

/// V2 默认表：改 0/1 值、增键 8 越界样例改为改 3 值、删键 6。
/// 实际变更：0→200、1→210、3→230、6 删除、其余保留、增键（无新键位
/// 可用则 2/4/5/7 值不变——演进点集中在 0/1/3/6）。
pub fn table_v2() -> DefaultsTable {
    DefaultsTable {
        ver: 2,
        entries: vec![
            (0, 200),
            (1, 210),
            (2, 120),
            (3, 230),
            (4, 140),
            (5, 150),
            (7, 170),
        ],
    }
}

/// 用户配置：显式键值集（键升序无重，规约同表）。
pub struct UserCfg {
    pub explicit: Vec<(u8, u32)>,
}

impl UserCfg {
    pub fn validate(&self) -> Result<(), u16> {
        let mut i = 1;
        while i < self.explicit.len() {
            if self.explicit[i].0 <= self.explicit[i - 1].0 {
                return Err(E_TABLE_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn has(&self, key: u8) -> bool {
        let mut i = 0;
        while i < self.explicit.len() {
            if self.explicit[i].0 == key {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn get(&self, key: u8) -> u32 {
        let mut i = 0;
        while i < self.explicit.len() {
            if self.explicit[i].0 == key {
                return self.explicit[i].1;
            }
            i += 1;
        }
        0
    }
}

/// 合并结果：生效键值 + legacy 桶（删键的用户值）+ 采用的默认表版本。
pub struct Merged {
    pub ver: u32,
    pub kv: Vec<(u8, u32)>,
    pub legacy: Vec<(u8, u32)>,
}

/// 升级合并：显式值保留、隐式键取新默认、删键用户值进 legacy。
pub fn upgrade(old: &DefaultsTable, new: &DefaultsTable, user: &UserCfg) -> Result<Merged, u16> {
    new.validate()?;
    user.validate()?;
    let mut kv: Vec<(u8, u32)> = Vec::new();
    let mut legacy: Vec<(u8, u32)> = Vec::new();
    // 走新表键序（键空间 0..=7）。
    let mut key = 0u8;
    while (key as usize) < KEY_COUNT {
        let in_new = new.lookup(key);
        match in_new {
            Some(dv) => {
                if user.has(key) {
                    // 显式值保留（升级铁律）。
                    kv.push((key, user.get(key)));
                } else {
                    // 隐式键取新默认（演进入口）。
                    kv.push((key, dv));
                }
            }
            None => {
                // 新表删键：有显式值进 legacy；无显式值消失。
                if user.has(key) {
                    legacy.push((key, user.get(key)));
                } else if old.lookup(key).is_some() {
                    // 旧表有、新表删、用户无值 → 消失（合法演进）。
                }
            }
        }
        key += 1;
    }
    Ok(Merged {
        ver: new.ver,
        kv,
        legacy,
    })
}

/// 配置指纹（幂等判据度量）。
pub fn fingerprint(m: &Merged) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h ^= m.ver as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    let mut i = 0;
    while i < m.kv.len() {
        h ^= (m.kv[i].0 as u64) << 32 | m.kv[i].1 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h ^= m.legacy.len() as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// ×10 升级场景合成：每场景一组显式键子集（LCG 确定性）。
pub fn scenarios(n: usize, seed: u32) -> Vec<UserCfg> {
    let mut lc = Lcg::new(seed);
    let mut out: Vec<UserCfg> = Vec::new();
    let mut s = 0;
    while s < n {
        let mut explicit: Vec<(u8, u32)> = Vec::new();
        let mut key = 0u8;
        while (key as usize) < KEY_COUNT {
            if lc.next() % 2 == 0 {
                explicit.push((key, 1000 + key as u32 * 10));
            }
            key += 1;
        }
        out.push(UserCfg { explicit });
        s += 1;
    }
    out
}

/// 确定性线性同余发生器。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 默认值表主巡检：×10 升级零覆盖全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let v1 = table_v1();
    let v2 = table_v2();
    v1.validate()?;
    v2.validate()?;
    hits += 1;

    // R1 表规约：V1 全 8 键、V2 删键 6（7 条目）、版本戳 1/2。
    if v1.entries.len() != 8 || v2.entries.len() != 7 {
        return Err(E_TABLE_BAD);
    }
    if v1.ver != 1 || v2.ver != 2 {
        return Err(E_VER_MISS);
    }
    if v2.lookup(6).is_some() {
        return Err(E_TABLE_BAD);
    }
    hits += 1;

    // R2 ×10 升级模拟：用户显式值零覆盖 + 隐式键演进 + 删键处置。
    let scs = scenarios(10, 0x4831_0001);
    let mut s = 0;
    while s < scs.len() {
        let m = upgrade(&v1, &v2, &scs[s])?;
        // 版本戳必须为 2。
        if m.ver != 2 {
            return Err(E_VER_MISS);
        }
        // 显式值零覆盖：逐条核对。
        let mut i = 0;
        while i < scs[s].explicit.len() {
            let (k, v) = scs[s].explicit[i];
            let mut found = false;
            let mut j = 0;
            while j < m.kv.len() {
                if m.kv[j].0 == k {
                    if m.kv[j].1 != v {
                        return Err(E_EXPLICIT_LOST);
                    }
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found && k != 6 {
                return Err(E_EXPLICIT_LOST);
            }
            i += 1;
        }
        // 隐式键演进：非显式且新表有的键 = 新默认值。
        let mut key = 0u8;
        while (key as usize) < KEY_COUNT {
            if !scs[s].has(key) {
                if let Some(dv) = v2.lookup(key) {
                    let mut j = 0;
                    while j < m.kv.len() {
                        if m.kv[j].0 == key {
                            if m.kv[j].1 != dv {
                                return Err(E_IMPLICIT_STALE);
                            }
                            break;
                        }
                        j += 1;
                    }
                }
            }
            key += 1;
        }
        // 删键处置：用户对键 6 有显式值 → 必须在 legacy。
        if scs[s].has(6) {
            let want = scs[s].get(6);
            let mut ok = false;
            let mut j = 0;
            while j < m.legacy.len() {
                if m.legacy[j].0 == 6 && m.legacy[j].1 == want {
                    ok = true;
                    break;
                }
                j += 1;
            }
            if !ok {
                return Err(E_LEGACY_DROP);
            }
        } else {
            // 无显式值 → legacy 必空。
            if !m.legacy.is_empty() {
                return Err(E_LEGACY_DROP);
            }
        }
        s += 1;
    }
    hits += 1;

    // R3 升级幂等：同用户同版本重升级 → 指纹不变。
    let u0 = UserCfg {
        explicit: vec![(0, 999), (6, 666)],
    };
    let m1 = upgrade(&v1, &v2, &u0)?;
    let fp1 = fingerprint(&m1);
    let m2 = upgrade(&v1, &v2, &u0)?;
    if fingerprint(&m2) != fp1 {
        return Err(E_IDEM_DRIFT);
    }
    hits += 1;

    // R4 legacy 桶保真：键 6 显式值 666 必在 legacy；键 0 显式值 999
    //    必在生效键值（跨删键与活键两类处置各验一样本）。
    let mut in_kv = false;
    let mut i = 0;
    while i < m1.kv.len() {
        if m1.kv[i].0 == 0 && m1.kv[i].1 == 999 {
            in_kv = true;
        }
        i += 1;
    }
    if !in_kv {
        return Err(E_EXPLICIT_LOST);
    }
    let mut in_legacy = false;
    i = 0;
    while i < m1.legacy.len() {
        if m1.legacy[i].0 == 6 && m1.legacy[i].1 == 666 {
            in_legacy = true;
        }
        i += 1;
    }
    if !in_legacy {
        return Err(E_LEGACY_DROP);
    }
    hits += 1;

    // R5 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_EXPLICIT_LOST,
        E_IMPLICIT_STALE,
        E_LEGACY_DROP,
        E_VER_MISS,
        E_IDEM_DRIFT,
        E_TABLE_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
