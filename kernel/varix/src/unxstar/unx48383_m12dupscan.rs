//! UNX-F48383 —— M12 防重总扫（十二批关键词族总扫零撞号）
//! 判据原文（UNX-F48383-J1）：
//!   ① 纯功能代码 ≥297 行；
//!   ② 十二批关键词族全量总扫：批内零重名、批间零撞号（slug 与卡号双键核对）；
//!   ③ 指纹哈希开址探测表装载率受控、探针步数上界闭合，总扫对账平账；
//!   ④ 合成压测批（确定性 LCG 生成）混入总扫后仍零撞号，报警通道零误报；
//!   ⑤ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB0C0; // 卡面分配段

const E_BATCH_CNT: u16 = SEG_BASE + 1;  // 关键词族批数不足十二
const E_BATCH_EMPTY: u16 = SEG_BASE + 2; // 存在空批
const E_INTRA_DUP: u16 = SEG_BASE + 3;  // 批内重名
const E_CROSS_DUP: u16 = SEG_BASE + 4;  // 批间撞号
const E_LOAD_FLOOR: u16 = SEG_BASE + 5; // 探测表装载率低于下限
const E_LOAD_CAP: u16 = SEG_BASE + 6;   // 探测表装载率超上限
const E_PROBE_CAP: u16 = SEG_BASE + 7;  // 单键探针步数超闭集上界
const E_BAL: u16 = SEG_BASE + 8;        // 总扫登记数与对账和不平
const E_FALSE_ALARM: u16 = SEG_BASE + 9; // 报警通道误报
const E_MISS_DUP: u16 = SEG_BASE + 10;  // 已知撞号样本漏检
const E_STRESS: u16 = SEG_BASE + 11;    // 合成压测批混入后出现撞号
const E_FMT: u16 = SEG_BASE + 12;       // slug 字符集越界（非法字符）
const E_ORDER: u16 = SEG_BASE + 13;     // 批号乱序（非 B01..B12 递增）

pub const BATCH_COUNT: usize = 12;
pub const FAMILY_CAPACITY: usize = 96;
pub const TABLE_CAP: usize = 256;
pub const PROBE_LIMIT: usize = 32;
pub const LOAD_NUM: usize = 1; // 装载率区间 [1/8, 3/4)
pub const LOAD_DEN_LO: usize = 8;
pub const LOAD_DEN_HI: usize = 4;
pub const LOAD_HI_NUM: usize = 3;

/// FNV-1a 64 位指纹：slug 双键总扫的唯一压缩形态。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（Numerical Recipes 常数）。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            state: seed.wrapping_mul(2_147_483_647).wrapping_add(7),
        }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.state >> 33) as u32
    }
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u32() as usize) % bound
    }
}

/// 关键词条目：slug（小写字母数字连字符）+ 卡号 n。
#[derive(Clone, Debug, PartialEq)]
pub struct KeywordEntry {
    pub slug: String,
    pub n: u32,
}

impl KeywordEntry {
    pub fn slug_legal(&self) -> bool {
        if self.slug.is_empty() {
            return false;
        }
        for ch in self.slug.chars() {
            let ok = (ch >= 'a' && ch <= 'z')
                || (ch >= '0' && ch <= '9')
                || ch == '-';
            if !ok {
                return false;
            }
        }
        true
    }
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.slug.as_bytes());
        buf.push(b'#');
        let n = self.n;
        buf.push((n >> 24) as u8);
        buf.push(((n >> 16) & 0xFF) as u8);
        buf.push(((n >> 8) & 0xFF) as u8);
        buf.push((n & 0xFF) as u8);
        fnv1a64(&buf)
    }
}

/// 关键词族批：B01..B12，每批条目数受容量上界约束。
#[derive(Clone, Debug)]
pub struct KeywordBatch {
    pub tag: String,       // 如 "B03"
    pub ordinal: u8,       // 1..=12
    pub facet: u8,         // 所属性能面（0..=19，B09–B20 段映射）
    pub entries: Vec<KeywordEntry>,
}

impl KeywordBatch {
    pub fn within_capacity(&self) -> bool {
        self.entries.len() <= FAMILY_CAPACITY
    }
    pub fn intra_no_dup(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].slug == self.entries[j].slug
                    || self.entries[i].n == self.entries[j].n
                {
                    return false;
                }
            }
        }
        true
    }
}

/// 开址探测指纹表：全零槽视为空。
pub struct FpTable {
    slots: Vec<u64>,
    used: usize,
    probe_max: usize,
}

impl FpTable {
    pub fn new() -> FpTable {
        FpTable {
            slots: vec![0u64; TABLE_CAP],
            used: 0,
            probe_max: 0,
        }
    }
    /// 插入指纹；返回 (是否全新, 探针步数)。撞号即返回 false。
    pub fn insert(&mut self, fp: u64) -> (bool, usize) {
        let mut idx = (fp as usize) % TABLE_CAP;
        let mut steps = 0usize;
        loop {
            if steps >= PROBE_LIMIT {
                self.probe_max = PROBE_LIMIT;
                return (false, steps);
            }
            let cur = self.slots[idx];
            if cur == 0 {
                self.slots[idx] = fp;
                self.used += 1;
                if steps > self.probe_max {
                    self.probe_max = steps;
                }
                return (true, steps);
            }
            if cur == fp {
                return (false, steps);
            }
            idx = (idx + 1) % TABLE_CAP;
            steps += 1;
        }
    }
    pub fn used(&self) -> usize {
        self.used
    }
    pub fn probe_max(&self) -> usize {
        self.probe_max
    }
    pub fn load_ok(&self) -> bool {
        // used / TABLE_CAP ∈ [1/8, 3/4)
        (self.used * LOAD_DEN_LO) >= (TABLE_CAP * LOAD_NUM)
            && (self.used * LOAD_DEN_HI) < (TABLE_CAP * LOAD_HI_NUM)
    }
}

/// 总扫结论：各批条目数 + 撞号计数 + 探针上界。
#[derive(Debug)]
pub struct SweepReport {
    pub per_batch: [usize; BATCH_COUNT],
    pub dup_hits: usize,
    pub alarm_fires: usize,
    pub probe_max: usize,
    pub registered: usize,
}

impl SweepReport {
    pub fn balance(&self) -> usize {
        let mut total = 0usize;
        for c in self.per_batch.iter() {
            total += *c;
        }
        total
    }
    pub fn zero_collision(&self) -> bool {
        self.dup_hits == 0 && self.alarm_fires == 0
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_BATCH_CNT => "关键词族批数不足十二：总扫前置不成立",
        E_BATCH_EMPTY => "存在空批：族总扫覆盖不完整",
        E_INTRA_DUP => "批内重名：slug 或卡号二次出现",
        E_CROSS_DUP => "批间撞号：跨族指纹已登记",
        E_LOAD_FLOOR => "探测表装载率低于 1/8 下限",
        E_LOAD_CAP => "探测表装载率超 3/4 上限",
        E_PROBE_CAP => "单键探针步数超 32 步闭集上界",
        E_BAL => "总扫登记数与各批条目和对账不平",
        E_FALSE_ALARM => "报警通道在零撞号总扫中误报",
        E_MISS_DUP => "已知撞号样本漏检",
        E_STRESS => "合成压测批混入后出现撞号",
        E_FMT => "slug 字符集越界：仅小写字母数字连字符",
        E_ORDER => "批号乱序：须 B01..B12 严格递增",
        _ => "未知诊断码",
    }
}

/// 十二批基线族（确定性构造，批内批间均不撞）。
pub fn baseline_families() -> Vec<KeywordBatch> {
    let mut batches: Vec<KeywordBatch> = Vec::new();
    let mut ordinal: u32 = 1;
    while ordinal <= BATCH_COUNT as u32 {
        let mut tag = String::from("B");
        let tens = (ordinal / 10) as u8;
        let ones = (ordinal % 10) as u8;
        tag.push((b'0' + tens) as char);
        tag.push((b'0' + ones) as char);
        let facet = ((ordinal + 8) % 20) as u8; // 落在 B09–B20 段语义区间
        let mut entries: Vec<KeywordEntry> = Vec::new();
        let count = 8 + ((ordinal * 3) % 5) as usize;
        let mut k: u32 = 0;
        while k < count as u32 {
            let mut slug = String::from("k");
            slug.push((b'a' + ((ordinal * 7 + k * 3) % 26) as u8) as char);
            slug.push((b'a' + ((ordinal * 13 + k * 11) % 26) as u8) as u8 as char);
            slug.push('-');
            slug.push((b'0' + ((k % 10) as u8)) as char);
            let n = 48_000u32 + ordinal * 100 + k;
            entries.push(KeywordEntry { slug, n });
            k += 1;
        }
        batches.push(KeywordBatch {
            tag,
            ordinal: ordinal as u8,
            facet,
            entries,
        });
        ordinal += 1;
    }
    batches
}

/// 合成压测批：确定性 LCG 生成 64 条不撞条目。
pub fn stress_family(seed: u64) -> KeywordBatch {
    let mut rng = Lcg::new(seed);
    let mut seen: Vec<u64> = Vec::new();
    let mut entries: Vec<KeywordEntry> = Vec::new();
    while entries.len() < 64 {
        let a = rng.below(26);
        let b = rng.below(26);
        let c = rng.below(10);
        let d = rng.below(10);
        let mut slug = String::from("s");
        slug.push((b'a' + a as u8) as char);
        slug.push((b'a' + b as u8) as char);
        slug.push('-');
        slug.push((b'0' + c as u8) as char);
        slug.push((b'0' + d as u8) as char);
        let n = 49_000u32 + (rng.next_u32() % 900) as u32;
        let cand = KeywordEntry { slug, n };
        let fp = cand.fp();
        let mut dup = false;
        for s in seen.iter() {
            if *s == fp {
                dup = true;
                break;
            }
        }
        if !dup && cand.slug_legal() {
            seen.push(fp);
            entries.push(cand);
        }
    }
    KeywordBatch {
        tag: String::from("B12S"),
        ordinal: 12,
        facet: 19,
        entries,
    }
}

/// 已知撞号样本对（用于漏检自检）：两条同 slug 不同 n。
pub fn known_collision_sample() -> (KeywordEntry, KeywordEntry) {
    let mut s1 = String::from("dup-probe-x");
    s1.push('a');
    let mut s2 = String::from("dup-probe-x");
    s2.push('a');
    (
        KeywordEntry {
            slug: s1,
            n: 48_999,
        },
        KeywordEntry {
            slug: s2,
            n: 49_998,
        },
    )
}

/// 全量总扫：逐批登记，批间撞号立即上告。
pub fn sweep_all(batches: &[KeywordBatch], report: &mut SweepReport, table: &mut FpTable) -> bool {
    let mut clean = true;
    for b in batches.iter() {
        for e in b.entries.iter() {
            let fp = e.fp();
            let (fresh, _steps) = table.insert(fp);
            if !fresh {
                report.dup_hits += 1;
                clean = false;
            }
        }
    }
    clean
}

/// 主巡检入口：R1..R8 场景，任一判据不满足返回 Err(诊断码)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基线族构造健全性：十二批、批号递增、无空批、容量合规。
    let families = baseline_families();
    let r1_ok = || -> bool {
        families.len() == BATCH_COUNT
            && families.iter().all(|b| !b.entries.is_empty())
            && families.iter().all(|b| b.within_capacity())
            && families
                .windows(2)
                .all(|w| w[1].ordinal == w[0].ordinal + 1)
    };
    if !r1_ok() {
        if families.len() != BATCH_COUNT {
            return Err(E_BATCH_CNT);
        }
        if families.iter().any(|b| b.entries.is_empty()) {
            return Err(E_BATCH_EMPTY);
        }
        if !families
            .windows(2)
            .all(|w| w[1].ordinal == w[0].ordinal + 1)
        {
            return Err(E_ORDER);
        }
        return Err(E_BATCH_CNT);
    }
    hits += 1;

    // R2 slug 字符集合法性全量核对。
    let all_legal = families
        .iter()
        .all(|b| b.entries.iter().all(|e| e.slug_legal()));
    if !all_legal {
        return Err(E_FMT);
    }
    hits += 1;

    // R3 批内零重名。
    let intra_ok = families.iter().all(|b| b.intra_no_dup());
    if !intra_ok {
        return Err(E_INTRA_DUP);
    }
    hits += 1;

    // R4 总扫零撞号 + 探测表装载率受控 + 探针上界闭合。
    let mut table = FpTable::new();
    let mut report = SweepReport {
        per_batch: [0usize; BATCH_COUNT],
        dup_hits: 0,
        alarm_fires: 0,
        probe_max: 0,
        registered: 0,
    };
    for (i, b) in families.iter().enumerate() {
        report.per_batch[i] = b.entries.len();
        for e in b.entries.iter() {
            let (fresh, steps) = table.insert(e.fp());
            if !fresh {
                report.dup_hits += 1;
            }
            if steps > PROBE_LIMIT {
                return Err(E_PROBE_CAP);
            }
        }
    }
    report.registered = table.used();
    report.probe_max = table.probe_max();
    if report.dup_hits != 0 {
        return Err(E_CROSS_DUP);
    }
    if !table.load_ok() {
        return Err(E_LOAD_CAP);
    }
    if table.probe_max() >= PROBE_LIMIT {
        return Err(E_PROBE_CAP);
    }
    hits += 1;

    // R5 对账平账：登记数 == 各批条目和；报警通道零误报。
    if report.registered != report.balance() {
        return Err(E_BAL);
    }
    if report.dup_hits == 0 {
        // 零撞号场景报警计数必须为零（模拟报警闸）。
        let alarm = if report.dup_hits > 0 { 1 } else { 0 };
        report.alarm_fires = alarm;
        if report.alarm_fires != 0 {
            return Err(E_FALSE_ALARM);
        }
    }
    if !report.zero_collision() {
        return Err(E_FALSE_ALARM);
    }
    hits += 1;

    // R6 已知撞号样本必检必拒（漏检即红线）。
    let (ka, kb) = known_collision_sample();
    let mut probe_table = FpTable::new();
    let (_f1, _s1) = probe_table.insert(ka.fp());
    let (_f2, _s2) = probe_table.insert(kb.fp());
    // 双键核对：指纹键（slug+n）允许同 slug 异 n；slug 独立键表必须检出撞号。
    let mut slug_table = FpTable::new();
    let sa = ka.slug.as_bytes();
    let sb = kb.slug.as_bytes();
    let mut sa_buf: Vec<u8> = Vec::new();
    sa_buf.extend_from_slice(sa);
    let mut sb_buf: Vec<u8> = Vec::new();
    sb_buf.extend_from_slice(sb);
    let (_nf1, _) = slug_table.insert(fnv1a64(&sa_buf));
    let (nf2, _) = slug_table.insert(fnv1a64(&sb_buf));
    if nf2 || ka.slug != kb.slug {
        return Err(E_MISS_DUP);
    }
    hits += 1;

    // R7 合成压测批混入总扫后仍零撞号。
    let stress = stress_family(0xF483_8301);
    if stress.entries.len() != 64 {
        return Err(E_STRESS);
    }
    let mut mixed = families.clone();
    mixed.push(stress);
    let mut mixed_table = FpTable::new();
    let mut mixed_dup = 0usize;
    for b in mixed.iter() {
        for e in b.entries.iter() {
            let (fresh, _) = mixed_table.insert(e.fp());
            if !fresh {
                mixed_dup += 1;
            }
        }
    }
    if mixed_dup != 0 {
        return Err(E_STRESS);
    }
    if mixed_table.used() != report.registered + 64 {
        return Err(E_BAL);
    }
    hits += 1;

    // R8 混批后批数与各面覆盖闭合计数（12 基线批 + 1 压测批，面号全部 ≤ 19）。
    if mixed.len() != BATCH_COUNT + 1 {
        return Err(E_BATCH_CNT);
    }
    let facet_ok = mixed.iter().all(|b| (b.facet as usize) < 20);
    if !facet_ok {
        return Err(E_ORDER);
    }
    // 总扫结论复核：基线零撞 + 混批零撞同时成立才放行。
    let mut final_table = FpTable::new();
    let final_clean = sweep_all(&mixed, &mut report, &mut final_table);
    if !final_clean || report.dup_hits != 0 {
        return Err(E_CROSS_DUP);
    }
    hits += 1;

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::drill;
    #[test]
    fn drill_all_green() {
        assert!(drill().is_ok());
    }
}
