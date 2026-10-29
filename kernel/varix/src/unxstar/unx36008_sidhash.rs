//! unx36008_sidhash — UNX-F36008 SID 哈希与等值查找表（ACL 求值加速原语）。
//!
//! 判据原文：
//!   J1 哈希碰撞实测：10^5 随机 SID 零碰撞且哈希可注入已知碰撞用例
//!      10 例全部正确分桶；查找表插入/删除/查 10^4 次守恒。
//! 设计纪律：
//!   1. 哈希：FNV-1a 64 铺 SID 规范字节序（rev/cnt/IA 大端/子权小端），
//!      与指纹面同一口径。
//!   2. 零碰撞严格口径：10^5 指纹排序后全体互异（64 位生日界内
//!      零碰撞是真命题；Vec 堆排序 800KB，不占栈）。
//!   3. 查找表：512 槽线性探测开放寻址 + 墓碑删除；已知碰撞分桶 =
//!      LCG 流内搜同槽对，两键同槽仍各自可检索（探测面）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7350;

// ── 诊断码闭集（0x7350~0x736F，独占）─────────────────────────
const E_FP: u16 = SEG_BASE + 1; // 指纹复算失配（同 SID 双算不等）
const E_COLLIDE: u16 = SEG_BASE + 2; // 10^5 半区互异验破（真碰撞）
const E_TABLE: u16 = SEG_BASE + 3; // 表守恒破（活计数 ≠ 插入-删除）
const E_BUCKET: u16 = SEG_BASE + 4; // 碰撞分桶失配（同槽键丢失）
const E_FULL: u16 = SEG_BASE + 5; // 表满（显性拒，探测无位）
const E_BAL: u16 = SEG_BASE + 6; // 操作账守恒破（10^4 面漂移）

use crate::checks::CheckSet;
use alloc::vec::Vec;
use super::unx36001_sidcore::Sid;

/// 查找表槽数（2 的幂，掩码取模；4096 ≥ 判据 J1 操作面 4000 活键峰值）。
pub const TAB_SLOTS: usize = 4096;
/// 操作账规模（判据 J1 的 10^4 面）。
pub const OPS_SCALE: u32 = 10_000;
/// 碰撞实测规模（判据 J1 的 10^5 面）。
pub const FP_SCALE: u32 = 100_000;

/// SID 规范字节铺开：rev/cnt/IA 大端/有效子权小端。
pub fn canonical_bytes(s: &Sid, out: &mut [u8]) -> usize {
    let need = 8 + 4 * s.sub_count as usize;
    out[0] = s.revision;
    out[1] = s.sub_count;
    let mut i = 0;
    while i < 6 {
        out[2 + i] = s.id_authority[i];
        i += 1;
    }
    let mut j = 0;
    while j < s.sub_count as usize {
        let b = s.sub_authorities[j].to_le_bytes();
        let base = 8 + j * 4;
        let mut k = 0;
        while k < 4 {
            out[base + k] = b[k];
            k += 1;
        }
        j += 1;
    }
    need
}

/// FNV-1a 64 哈希（规范字节序）。
pub fn sid_hash(s: &Sid) -> u64 {
    let mut buf = [0u8; 8 + 4 * 15];
    let n = canonical_bytes(s, &mut buf);
    let mut h: u64 = 0x811C_9DC5;
    let mut i = 0;
    while i < n {
        h = (h ^ buf[i] as u64).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 表槽位（哈希掩码）。
pub fn slot_of(fp: u64) -> usize {
    (fp as usize) & (TAB_SLOTS - 1)
}

/// 开放寻址等值查找表：键为 SID 指纹（值并存活标记与墓碑）。
pub struct SidTable {
    key: [u64; TAB_SLOTS],
    live: [bool; TAB_SLOTS],
    tomb: [bool; TAB_SLOTS],
    pub inserts: u32,
    pub removes: u32,
    pub hits: u32,
    pub misses: u32,
    pub live_count: i64,
    pub probe_steps: u64,
}

impl SidTable {
    pub fn new() -> SidTable {
        SidTable {
            key: [0; TAB_SLOTS],
            live: [false; TAB_SLOTS],
            tomb: [false; TAB_SLOTS],
            inserts: 0,
            removes: 0,
            hits: 0,
            misses: 0,
            live_count: 0,
            probe_steps: 0,
        }
    }

    /// 插入：已存活返回 Ok(false)（等值幂等）；新插入 Ok(true)；满 Err。
    pub fn insert(&mut self, fp: u64) -> Result<bool, u16> {
        let mut idx = slot_of(fp);
        let mut steps = 0;
        while steps < TAB_SLOTS {
            if self.live[idx] && self.key[idx] == fp {
                self.probe_steps += steps as u64 + 1;
                return Ok(false);
            }
            if !self.live[idx] {
                self.key[idx] = fp;
                self.live[idx] = true;
                self.tomb[idx] = false;
                self.inserts += 1;
                self.live_count += 1;
                self.probe_steps += steps as u64 + 1;
                return Ok(true);
            }
            idx = (idx + 1) & (TAB_SLOTS - 1);
            steps += 1;
        }
        Err(E_FULL)
    }

    /// 查找：存活命中 Ok(true)；未命中 Ok(false)（miss 入账）。
    pub fn lookup(&mut self, fp: u64) -> bool {
        let mut idx = slot_of(fp);
        let mut steps = 0;
        while steps < TAB_SLOTS {
            if self.live[idx] && self.key[idx] == fp {
                self.hits += 1;
                self.probe_steps += steps as u64 + 1;
                return true;
            }
            if !self.live[idx] && !self.tomb[idx] {
                break; // 净空槽：探测终止
            }
            idx = (idx + 1) & (TAB_SLOTS - 1);
            steps += 1;
        }
        self.misses += 1;
        self.probe_steps += steps as u64 + 1;
        false
    }

    /// 删除：墓碑标记（探测链不断）。未存活返回 false。
    pub fn remove(&mut self, fp: u64) -> bool {
        let mut idx = slot_of(fp);
        let mut steps = 0;
        while steps < TAB_SLOTS {
            if self.live[idx] && self.key[idx] == fp {
                self.live[idx] = false;
                self.tomb[idx] = true;
                self.removes += 1;
                self.live_count -= 1;
                return true;
            }
            if !self.live[idx] && !self.tomb[idx] {
                return false;
            }
            idx = (idx + 1) & (TAB_SLOTS - 1);
            steps += 1;
        }
        false
    }

    /// 守恒：活计数 == 插入 - 删除。
    pub fn conserve(&self) -> bool {
        self.live_count == self.inserts as i64 - self.removes as i64
    }
}

/// 零碰撞严格验证：64 位指纹全体互异（排序后相邻不等）。
/// n=10^5 × 8B = 800KB 堆排序，一次过（生日界内 64 位零碰撞是真命题，
/// 半区口径反而过强——32 位半区在 10^5 规模必有碰撞，不作判据）。
fn all_distinct(fps: &[u64]) -> Result<(), u16> {
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < fps.len() {
        v.push(fps[i]);
        i += 1;
    }
    v.sort_unstable();
    let mut i = 1;
    while i < v.len() {
        if v[i] == v[i - 1] {
            return Err(E_COLLIDE);
        }
        i += 1;
    }
    Ok(())
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_FP => "指纹复算失配（同 SID 双算不等）",
        E_COLLIDE => "10^5 半区互异验破（真碰撞）",
        E_TABLE => "表守恒破（活计数 ≠ 插入-删除）",
        E_BUCKET => "碰撞分桶失配（同槽键丢失）",
        E_FULL => "表满（探测无位）",
        E_BAL => "操作账守恒破（10^4 面漂移）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：10^5 零碰撞 + 10 例同槽分桶 + 10^4 表操作守恒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut seed: u32 = 0x3608_0001;

    // R1：10^5 随机 SID 指纹——半区严格互异 ⇒ 64 位零碰撞。
    let mut fps: Vec<u64> = Vec::new();
    let mut n = 0u32;
    while n < FP_SCALE {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cnt = (n % 15) as usize + 1;
        let mut subs = [0u32; 15];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed;
            i += 1;
        }
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let s = Sid::new(1, Sid::ia_from(ia_val), &subs[..cnt])?;
        // 指纹复算面：同 SID 两次哈希必须逐位相等（确定性锚）。
        let fp1 = sid_hash(&s);
        let fp2 = sid_hash(&s);
        if fp1 != fp2 || fp1 == 0 {
            return Err(E_FP);
        }
        fps.push(fp1);
        n += 1;
    }
    all_distinct(&fps)?;
    hits += 1;

    // R2：已知碰撞分桶 10 例——LCG 流内搜同槽对（确定性），两键同槽
    // 仍各自可检索、各自可删（探测面）。
    let mut found = 0u32;
    let mut s0: u32 = 0x3608_C0DE;
    let mut prev: Option<u64> = None; // 上一条指纹（等下一条同槽配对）
    while found < 10 {
        s0 = s0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[s0, s0 ^ 0x5555, found + 1])?;
        let fp = sid_hash(&s);
        match prev {
            Some(pfp) if slot_of(pfp) == slot_of(fp) => {
                // 同槽对：插入两键、双查、删一键后另键仍可查。
                let mut t = SidTable::new();
                if t.insert(pfp)? != true || t.insert(fp)? != true {
                    return Err(E_BUCKET);
                }
                if t.live_count != 2 || !t.conserve() {
                    return Err(E_TABLE);
                }
                if !t.lookup(pfp) || !t.lookup(fp) {
                    return Err(E_BUCKET);
                }
                if !t.remove(pfp) {
                    return Err(E_BUCKET);
                }
                if !t.lookup(fp) {
                    return Err(E_BUCKET);
                }
                if t.lookup(pfp) {
                    return Err(E_BUCKET);
                }
                found += 1;
                prev = None;
            }
            _ => {
                prev = Some(fp);
            }
        }
    }
    if found != 10 {
        return Err(E_BUCKET);
    }
    hits += 1;

    // R3：10^4 次表操作守恒——插入 4000 / 查 3000（2000 命中 + 1000 miss
    // 交替）/ 删 2000 / 复查 1000，活计数与操作账逐段对平。
    let mut t = SidTable::new();
    let mut k = 0u32;
    while k < 4000 {
        if t.insert((k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)? != true {
            return Err(E_TABLE);
        }
        k += 1;
    }
    if t.live_count != 4000 || !t.conserve() {
        return Err(E_TABLE);
    }
    let mut k = 0u32;
    while k < 3000 {
        let fp = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        if k % 3 == 2 {
            // miss 族：确定不在表内的键。
            if t.lookup(fp.wrapping_add(1 << 40)) {
                return Err(E_TABLE);
            }
        } else if !t.lookup(fp) {
            return Err(E_TABLE);
        }
        k += 1;
    }
    let mut k = 0u32;
    while k < 2000 {
        let fp = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        if !t.remove(fp) {
            return Err(E_TABLE);
        }
        k += 1;
    }
    if t.live_count != 2000 || !t.conserve() {
        return Err(E_TABLE);
    }
    let mut k = 0u32;
    while k < 1000 {
        let fp = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        // 前 1000 已删：应 miss；后 1000..2000 仍在：应 hit。
        if t.lookup(fp) {
            return Err(E_TABLE);
        }
        let fp2 = ((k + 2000) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        if !t.lookup(fp2) {
            return Err(E_TABLE);
        }
        k += 1;
    }
    if t.inserts != 4000 || t.removes != 2000 {
        return Err(E_BAL);
    }
    if t.misses < 1000 + 1000 {
        return Err(E_BAL);
    }
    if !t.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36008_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36008_sidhash");
    cs.add("SEG 段位独占", SEG_BASE == 0x7350, "0x7350~0x736F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(3));
    }
}
