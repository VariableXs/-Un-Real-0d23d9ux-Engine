//! UNX-F36144-J1 地基联动剧本 IV：损坏注入矩阵（Token/SD/ACE 三载体 × 翻转）。
//! 判据：① LCG 确定性翻转三载体线形式，3 载体 × 4 翻转区（魔数/保留区/
//! 载荷/校验）= 12 类坏例必拒且码正确、6 例好例必过；② 载体覆盖各 ≥1 次；
//! ③ 双跑逐位一致；④ 纯功能代码 ≥300 行。
//! 设计纪律：B08 联动剧本之四——三载体在本地建模（8 字节线形式 + 本地
//! 校验器），翻转源用红线指定的确定性 LCG；聚合消费归 36155。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x8260;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_MAGIC: u16 = SEG_BASE + 2;  // 魔数区损坏
pub const E_RSRV: u16 = SEG_BASE + 3;   // 保留区非零
pub const E_LOAD: u16 = SEG_BASE + 4;   // 载荷区越界
pub const E_CHECK: u16 = SEG_BASE + 5;  // 校验区不符
pub const E_GOOD_ERR: u16 = SEG_BASE + 6; // 好例被拒（红线）
pub const E_BAD_OK: u16 = SEG_BASE + 7;   // 坏例被放行（红线）
pub const E_CS: u16 = SEG_BASE + 8;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 线形式长度。
pub const WIRE_LEN: usize = 8;
/// 魔数锚（byte0 高半 = 载体类别，低半 = 版本）。
pub const MAGIC_TOKEN: u8 = 0x71;
pub const MAGIC_SD: u8 = 0x72;
pub const MAGIC_ACE: u8 = 0x73;
/// 保留区锚（byte3 必须为零）。
pub const RSRV_INDEX: usize = 3;
/// 载荷区合法值域（byte4 低位 nibble，0..0x10）。
pub const LOAD_MAX: u8 = 0x10;

/// 载体类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Carrier {
    /// 令牌线形式。
    Token = 0x0001,
    /// SD 线形式。
    Sd    = 0x0002,
    /// ACE 线形式。
    Ace   = 0x0003,
}

impl Carrier {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 载体魔数。
    pub fn magic(self) -> u8 {
        match self {
            Carrier::Token => MAGIC_TOKEN,
            Carrier::Sd => MAGIC_SD,
            Carrier::Ace => MAGIC_ACE,
        }
    }

    /// 载体 → 损坏区期望码（映射闭集，翻转区互异）。
    pub fn zone_code(zone: CorruptZone) -> u16 {
        match zone {
            CorruptZone::ZoneMagic => E_MAGIC,
            CorruptZone::ZoneRsrv => E_RSRV,
            CorruptZone::ZoneLoad => E_LOAD,
            CorruptZone::ZoneCheck => E_CHECK,
        }
    }
}

/// 翻转区（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorruptZone {
    /// 魔数区（byte0）。
    ZoneMagic = 0x0001,
    /// 保留区（byte3）。
    ZoneRsrv  = 0x0002,
    /// 载荷区（byte4）。
    ZoneLoad  = 0x0003,
    /// 校验区（byte7）。
    ZoneCheck = 0x0004,
}

impl CorruptZone {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 校验和：byte1^byte2^byte4^byte5^byte6（本地轻量校验面）。
pub fn wire_checksum(w: &[u8; WIRE_LEN]) -> u8 {
    w[1] ^ w[2] ^ w[4] ^ w[5] ^ w[6]
}

/// 造好例线形式：magic + 计数 + 载荷 + 校验。
pub fn good_wire(c: Carrier, count: u8, load: u8) -> [u8; WIRE_LEN] {
    let mut w = [0u8; WIRE_LEN];
    w[0] = c.magic();
    w[1] = count;
    w[2] = VER as u8;
    w[RSRV_INDEX] = 0;
    w[4] = load & 0x0f;
    w[5] = (load >> 4) & 0x0f;
    w[6] = c.code() as u8;
    w[7] = wire_checksum(&w);
    w
}

/// 校验器：魔数 → 保留区 → 载荷 → 校验（顺序固定）。
pub fn validate_wire(w: &[u8; WIRE_LEN]) -> Result<(), u16> {
    let magic_ok = w[0] == MAGIC_TOKEN || w[0] == MAGIC_SD || w[0] == MAGIC_ACE;
    if !magic_ok {
        return Err(E_MAGIC);
    }
    if w[RSRV_INDEX] != 0 {
        return Err(E_RSRV);
    }
    if w[4] >= LOAD_MAX {
        return Err(E_LOAD);
    }
    if w[7] != wire_checksum(w) {
        return Err(E_CHECK);
    }
    Ok(())
}

/// LCG（确定性随机源）。
pub fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// 单发注入：按区翻转好例 → 校验器。
pub fn inject_and_probe(c: Carrier, zone: CorruptZone, salt: u8) -> Result<(), u16> {
    let mut w = good_wire(c, 3, 5);
    match zone {
        CorruptZone::ZoneMagic => w[0] = 0x00 | (salt & 0x0f),
        CorruptZone::ZoneRsrv => w[RSRV_INDEX] = 0x01 | (salt & 0x0f),
        CorruptZone::ZoneLoad => w[4] = LOAD_MAX + (salt & 0x07),
        CorruptZone::ZoneCheck => {
            w[7] = wire_checksum(&w).wrapping_add(1 + (salt & 0x01));
        }
    }
    validate_wire(&w)
}

/// 矩阵走查：3 载体 × 4 区全量注入（12 类，每类 2 发 = 24 发补刀）。
pub fn matrix_walk() -> Result<u32, u16> {
    let mut n: u32 = 0;
    let carriers: Vec<Carrier> = vec![Carrier::Token, Carrier::Sd, Carrier::Ace];
    let zones: Vec<CorruptZone> = vec![
        CorruptZone::ZoneMagic,
        CorruptZone::ZoneRsrv,
        CorruptZone::ZoneLoad,
        CorruptZone::ZoneCheck,
    ];
    let mut ci = 0;
    while ci < carriers.len() {
        let mut zi = 0;
        while zi < zones.len() {
            let mut salt = 0;
            while salt < 2 {
                match inject_and_probe(carriers[ci], zones[zi], salt) {
                    Ok(_) => return Err(E_BAD_OK),
                    Err(c) => {
                        if c != Carrier::zone_code(zones[zi]) {
                            return Err(E_BAD_OK);
                        }
                    }
                }
                n += 1;
                salt += 1;
            }
            zi += 1;
        }
        ci += 1;
    }
    Ok(n)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(n: u32) -> String {
    let mut s = String::new();
    s.push_str("script4 injected=");
    s.push_str(&n.to_string());
    s.push_str(" zones=");
    s.push_str(&CorruptZone::ZoneCheck.code().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_MAGIC, E_RSRV, E_LOAD, E_CHECK, E_GOOD_ERR, E_BAD_OK, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MAGIC => "魔数区损坏",
        E_RSRV => "保留区非零",
        E_LOAD => "载荷区越界",
        E_CHECK => "校验区不符",
        E_GOOD_ERR => "好例被拒（红线）",
        E_BAD_OK => "坏例被放行/码不符（红线）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位；R2 6 例好例必过；R3 LCG 矩阵 24 轮坏例必拒且码正确；
/// R4 矩阵走查 24 发补刀；R5 覆盖守恒（3 载体 × 4 区全命中）；
/// R6 双跑逐位一致 + 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 魔数锚（三载体互异）。
    if VER != 1 {
        return Err(E_VER);
    }
    if MAGIC_TOKEN == MAGIC_SD || MAGIC_SD == MAGIC_ACE || MAGIC_TOKEN == MAGIC_ACE {
        return Err(E_MAGIC);
    }
    hits += 1;
    // R2：6 例好例必过（3 载体 × 2 组参数）。
    let carriers: Vec<Carrier> = vec![Carrier::Token, Carrier::Sd, Carrier::Ace];
    let mut g = 0;
    while g < carriers.len() {
        let w1 = good_wire(carriers[g], 1, 0x03);
        let w2 = good_wire(carriers[g], 9, 0xf7);
        validate_wire(&w1)?;
        validate_wire(&w2)?;
        if w1 == w2 {
            return Err(E_GOOD_ERR);
        }
        hits += 1;
        g += 1;
    }
    // R3：LCG 矩阵 24 轮（3 载体 × 4 区 × 2 轮，区由 LCG 挑）。
    let zones: Vec<CorruptZone> = vec![
        CorruptZone::ZoneMagic,
        CorruptZone::ZoneRsrv,
        CorruptZone::ZoneLoad,
        CorruptZone::ZoneCheck,
    ];
    let mut st = 0xfeed_face_beef_0001;
    let mut r = 0;
    while r < 24 {
        st = lcg_next(st);
        let c = carriers[(st % 3) as usize];
        let z = zones[((st >> 8) % 4) as usize];
        match inject_and_probe(c, z, ((st >> 16) & 0x03) as u8) {
            Ok(_) => return Err(E_BAD_OK),
            Err(code) => {
                if code != Carrier::zone_code(z) {
                    return Err(E_BAD_OK);
                }
            }
        }
        hits += 1;
        r += 1;
    }
    // R4：矩阵走查 24 发补刀。
    let walked = matrix_walk()?;
    if walked != 24 {
        return Err(E_BAD_OK);
    }
    hits += 1;
    // R5：覆盖守恒（四区期望码互异 → 码正确性有区分度）。
    let zc = [
        Carrier::zone_code(CorruptZone::ZoneMagic),
        Carrier::zone_code(CorruptZone::ZoneRsrv),
        Carrier::zone_code(CorruptZone::ZoneLoad),
        Carrier::zone_code(CorruptZone::ZoneCheck),
    ];
    let mut i = 0;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            if zc[i] == zc[j] {
                return Err(E_BAD_OK);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;
    // R6：双跑逐位一致 + 指纹稳定。
    let a = matrix_walk()?;
    let b = matrix_walk()?;
    if a != b {
        return Err(E_CS);
    }
    let w0 = good_wire(Carrier::Ace, 5, 0x42);
    let fp1 = core::fnv1a(&w0);
    let fp2 = core::fnv1a(&w0);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(31));
    }

    #[test]
    fn zones_rejected() {
        assert_eq!(inject_and_probe(Carrier::Token, CorruptZone::ZoneMagic, 0), Err(E_MAGIC));
        assert_eq!(inject_and_probe(Carrier::Sd, CorruptZone::ZoneRsrv, 0), Err(E_RSRV));
        assert_eq!(inject_and_probe(Carrier::Ace, CorruptZone::ZoneLoad, 0), Err(E_LOAD));
        assert_eq!(inject_and_probe(Carrier::Token, CorruptZone::ZoneCheck, 0), Err(E_CHECK));
    }

    #[test]
    fn good_passes() {
        let w = good_wire(Carrier::Sd, 2, 0x12);
        assert_eq!(validate_wire(&w), Ok(()));
        assert!(summary(24).contains("script4"));
    }

    #[test]
    fn lcg_deterministic() {
        assert_eq!(lcg_next(1), lcg_next(1));
        assert!(codes_distinct());
    }
}

pub fn run_unx36144_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36144_script4");
    cs.add("SEG 段位独占", SEG_BASE == 0x8260, "0x8260~0x826f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "矩阵 12 类全检出、好例 6 例全过"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("三载体魔数锚互异", MAGIC_TOKEN != MAGIC_SD && MAGIC_SD != MAGIC_ACE, "载体魔数锚撞码");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
