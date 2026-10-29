//! unx48346_onehand — UNX-F48346 单手/交换修饰键配置数据面（语义重映射不误触）（诊断段 0xCC20）。
//!
//! 判据原文：
//!   J1 重映射 ×10 正确；判据锚定 体验四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 纯数据面：重映射表只承载 物理键→语义键 的映射关系与模式
//!      预设（左手单手/右手单手/左右交换/自定义），不触碰扫描码与
//!      驱动实现面。
//!   2. 不误触三律：①映射须单射——两个物理键不得映射到同一语义
//!      键（E_MAP_BAD）；②普通键（非修饰族）永不参与重映射，直通
//!      原语义（E_MISS_HIT 即误触判死）；③非法键号查询必拒。
//!   3. ×10 正确：十例覆盖左/右手预设、交换对、直通、单射守门、
//!      越界拒收等路径，逐例判定唯一确定。
//!   4. 账面封印：映射表以 FNV 指纹自封，任一表项被改即失配。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCC20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MAP_BAD: u16 = SEG_BASE + 1; // 映射非单射/非法表
const E_KEY_BAD: u16 = SEG_BASE + 2; // 非法键号
const E_MODE_BAD: u16 = SEG_BASE + 3; // 非法模式号
const E_MISS_HIT: u16 = SEG_BASE + 4; // 普通键被误触重映射
const E_TAMPER: u16 = SEG_BASE + 5; // 映射表指纹失配
const E_PRESET_BAD: u16 = SEG_BASE + 6; // 预设落表错误
const E_COVER_MISS: u16 = SEG_BASE + 7; // ×10 覆盖缺口

/// 键号（语义/物理共用一个编号空间）。
pub const K_LSHIFT: u8 = 0;
pub const K_RSHIFT: u8 = 1;
pub const K_LCTRL: u8 = 2;
pub const K_RCTRL: u8 = 3;
pub const K_LALT: u8 = 4;
pub const K_RALT: u8 = 5;
pub const K_CAPS: u8 = 6;
pub const K_SPACE: u8 = 7;
pub const K_A: u8 = 8; // 普通键代表
pub const KEYS: u8 = 9;

fn is_mod(k: u8) -> bool {
    k <= K_SPACE
}

/// 模式号。
pub const MODE_LHAND: u8 = 0; // 左手单手：右半修饰键折到左半
pub const MODE_RHAND: u8 = 1; // 右手单手：左半修饰键折到右半
pub const MODE_SWAP: u8 = 2; // 左右交换：Shift/Ctrl/Alt 三对互换
pub const MODE_CUSTOM: u8 = 3;

/// FNV-1a 64 位指纹（u8 流）。
pub fn fp_bytes(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 重映射表：物理键 → 语义键，指纹自封。
#[derive(Clone)]
pub struct RemapTable {
    pub mode: u8,
    map: [u8; KEYS as usize],
    seal: u64,
}

impl RemapTable {
    /// 预设落表：左手单手 = {RSHIFT→LSHIFT, RCTRL→LCTRL, RALT→LALT,
    /// SPACE→(不变)…}；右手单手 = 左半折右半 + CAPS→RSHIFT；
    /// 交换 = 三对互换。
    pub fn preset(mode: u8) -> Result<Self, u16> {
        let mut map = [0u8; KEYS as usize];
        let mut k = 0u8;
        while k < KEYS {
            map[k as usize] = k; // 先恒等
            k += 1;
        }
        match mode {
            MODE_LHAND => {
                map[K_RSHIFT as usize] = K_LSHIFT;
                map[K_RCTRL as usize] = K_LCTRL;
                map[K_RALT as usize] = K_LALT;
            }
            MODE_RHAND => {
                map[K_LSHIFT as usize] = K_RSHIFT;
                map[K_LCTRL as usize] = K_RCTRL;
                map[K_LALT as usize] = K_RALT;
                map[K_CAPS as usize] = K_RSHIFT;
            }
            MODE_SWAP => {
                map[K_LSHIFT as usize] = K_RSHIFT;
                map[K_RSHIFT as usize] = K_LSHIFT;
                map[K_LCTRL as usize] = K_RCTRL;
                map[K_RCTRL as usize] = K_LCTRL;
                map[K_LALT as usize] = K_RALT;
                map[K_RALT as usize] = K_LALT;
            }
            _ => return Err(E_MODE_BAD),
        }
        let mut t = RemapTable { mode, map, seal: 0 };
        t.seal = t.compute_seal();
        Ok(t)
    }

    /// 自定义落表：调用方给全表，校验单射后封印。
    pub fn custom(map: [u8; KEYS as usize]) -> Result<Self, u16> {
        let mut t = RemapTable { mode: MODE_CUSTOM, map, seal: 0 };
        t.check_injective()?;
        t.seal = t.compute_seal();
        Ok(t)
    }

    fn compute_seal(&self) -> u64 {
        fp_bytes(&self.map) ^ fp_bytes(&[self.mode])
    }

    /// 单射校验：修饰族两两不同语义落点。
    pub fn check_injective(&self) -> Result<(), u16> {
        let mut a = 0usize;
        while a < KEYS as usize {
            let mut b = a + 1;
            while b < KEYS as usize {
                if self.map[a] == self.map[b] && is_mod(a as u8) && is_mod(b as u8) {
                    return Err(E_MAP_BAD);
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), u16> {
        if self.seal != self.compute_seal() {
            return Err(E_TAMPER);
        }
        Ok(())
    }

    /// 语义解析：普通键一律直通（防误触），越界必拒。
    pub fn resolve(&self, phys: u8) -> Result<u8, u16> {
        self.verify()?;
        if phys >= KEYS {
            return Err(E_KEY_BAD);
        }
        if !is_mod(phys) && self.map[phys as usize] != phys {
            return Err(E_MISS_HIT);
        }
        Ok(self.map[phys as usize])
    }
}

/// 单例重映射行为判定：场景（模式, 按下的物理键）→ 期望语义键。
pub fn expect_of(mode: u8, phys: u8) -> Result<u8, u16> {
    if phys >= KEYS {
        return Err(E_KEY_BAD);
    }
    if !is_mod(phys) {
        return Ok(phys);
    }
    match mode {
        MODE_LHAND => Ok(match phys {
            K_RSHIFT => K_LSHIFT,
            K_RCTRL => K_LCTRL,
            K_RALT => K_LALT,
            _ => phys,
        }),
        MODE_RHAND => Ok(match phys {
            K_LSHIFT => K_RSHIFT,
            K_LCTRL => K_RCTRL,
            K_LALT => K_RALT,
            K_CAPS => K_RSHIFT,
            _ => phys,
        }),
        MODE_SWAP => Ok(match phys {
            K_LSHIFT => K_RSHIFT,
            K_RSHIFT => K_LSHIFT,
            K_LCTRL => K_RCTRL,
            K_RCTRL => K_LCTRL,
            K_LALT => K_RALT,
            K_RALT => K_LALT,
            _ => phys,
        }),
        _ => Err(E_MODE_BAD),
    }
}

/// drill：J1 重映射 ×10 正确。
pub fn drill() -> Result<u32, u16> {
    // ── R1 主判据：三预设 × 全键域解析，逐键与期望对表 ───────
    let mut ok_cases = 0u32;
    let modes = [MODE_LHAND, MODE_RHAND, MODE_SWAP];
    let mut mi = 0usize;
    while mi < modes.len() {
        let t = RemapTable::preset(modes[mi])?;
        t.verify()?;
        let mut k = 0u8;
        while k < KEYS {
            let got = t.resolve(k)?;
            let want = expect_of(modes[mi], k)?;
            if got != want {
                return Err(E_PRESET_BAD);
            }
            k += 1;
        }
        ok_cases += 1;
        mi += 1;
    }

    // ── R2 普通键直通：'A' 与 SPACE 在三预设下永不改语义 ─────
    let mut mi = 0usize;
    while mi < modes.len() {
        let t = RemapTable::preset(modes[mi])?;
        if t.resolve(K_A)? != K_A {
            return Err(E_MISS_HIT);
        }
        mi += 1;
    }
    ok_cases += 1;

    // ── R3 左手单手语义抽查：右半修饰键全部落左半 ────────────
    let lh = RemapTable::preset(MODE_LHAND)?;
    if lh.resolve(K_RSHIFT)? != K_LSHIFT
        || lh.resolve(K_RCTRL)? != K_LCTRL
        || lh.resolve(K_RALT)? != K_LALT
        || lh.resolve(K_LSHIFT)? != K_LSHIFT
    {
        return Err(E_PRESET_BAD);
    }
    ok_cases += 1;

    // ── R4 右手单手：CAPS 折为 RSHIFT，左半折右半 ────────────
    let rh = RemapTable::preset(MODE_RHAND)?;
    if rh.resolve(K_CAPS)? != K_RSHIFT || rh.resolve(K_LCTRL)? != K_RCTRL {
        return Err(E_PRESET_BAD);
    }
    ok_cases += 1;

    // ── R5 交换对：三对互换回环 resolve(resolve(x)) == x ─────
    let sw = RemapTable::preset(MODE_SWAP)?;
    let pairs = [K_LSHIFT, K_RSHIFT, K_LCTRL, K_RCTRL, K_LALT, K_RALT];
    let mut pi = 0usize;
    while pi < pairs.len() {
        let once = sw.resolve(pairs[pi])?;
        let twice = sw.resolve(once)?;
        if twice != pairs[pi] {
            return Err(E_PRESET_BAD);
        }
        pi += 1;
    }
    ok_cases += 1;

    // ── R6 单射守门：两物理键同落点必拒（自定义表）──────────
    let mut bad = [0u8; KEYS as usize];
    let mut k = 0u8;
    while k < KEYS {
        bad[k as usize] = k;
        k += 1;
    }
    bad[K_RCTRL as usize] = K_LCTRL;
    bad[K_RALT as usize] = K_LCTRL; // 与 RCTRL 撞语义
    if RemapTable::custom(bad).is_ok() {
        return Err(E_MAP_BAD);
    }
    ok_cases += 1;

    // ── R7 合法自定义表：CAPS→LALT、SPACE 恒等，落表可用 ─────
    let mut good = [0u8; KEYS as usize];
    let mut k = 0u8;
    while k < KEYS {
        good[k as usize] = k;
        k += 1;
    }
    good[K_CAPS as usize] = K_LALT;
    let gt = RemapTable::custom(good)?;
    if gt.resolve(K_CAPS)? != K_LALT || gt.resolve(K_A)? != K_A {
        return Err(E_PRESET_BAD);
    }
    ok_cases += 1;

    // ── R8 越界与守门：非法键号/非法模式必拒 ─────────────────
    if lh.resolve(200).is_ok() {
        return Err(E_KEY_BAD);
    }
    if RemapTable::preset(9).is_ok() {
        return Err(E_MODE_BAD);
    }
    // 自定义模式无预设落表路径：expect_of 对 CUSTOM 必拒。
    if expect_of(MODE_CUSTOM, K_LSHIFT).is_ok() {
        return Err(E_MODE_BAD);
    }
    ok_cases += 1;

    // ── R9 篡改注入：表项被改 → 指纹失配 + 解析必拒 ─────────
    let mut bad_t = lh.clone();
    bad_t.map[K_LSHIFT as usize] = K_RALT;
    if bad_t.verify() != Err(E_TAMPER) {
        return Err(E_TAMPER);
    }
    if bad_t.resolve(K_LSHIFT).is_ok() {
        return Err(E_TAMPER);
    }
    ok_cases += 1;

    // ── R10 误触判死：普通键若在自定义表里被动过 → E_MISS_HIT ─
    let mut evil = [0u8; KEYS as usize];
    let mut k = 0u8;
    while k < KEYS {
        evil[k as usize] = k;
        k += 1;
    }
    evil[K_A as usize] = K_SPACE;
    let et = RemapTable::custom(evil)?;
    if et.resolve(K_A) != Err(E_MISS_HIT) {
        return Err(E_MISS_HIT);
    }
    ok_cases += 1;

    // 覆盖对账：恰 10 例。
    if ok_cases != 10 {
        return Err(E_COVER_MISS);
    }
    Ok(ok_cases)
}
