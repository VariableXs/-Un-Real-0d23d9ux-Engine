//! UNX-F48384 —— M12 延迟账段汇总（B09–B20 各性能面 P99 并账）
//! 判据原文（UNX-F48384-J1）：
//!   ① 纯功能代码 ≥337 行；
//!   ② B09–B20 十二个性能面逐面采样（确定性 LCG），面内 P99 分桶统计可复跑一致；
//!   ③ 并账：面 P99 逐级并入总账，总账须满足单调上界（并账 P99 ≥ 任一面 P99）；
//!   ④ 预算封顶：任一面 P99 超 B 段封顶线即 Err；总账 P99 超全局封顶线即 Err；
//!   ⑤ 对账：并账分桶总数 == 各面样本数之和；复跑指纹一致；
//!   ⑥ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1A0; // 卡面分配段

const E_FACET_CNT: u16 = SEG_BASE + 1;  // 性能面不足十二（B09–B20）
const E_FACET_ORDER: u16 = SEG_BASE + 2; // 面号乱序（非 B09..B20 递增）
const E_SAMPLE_EMPTY: u16 = SEG_BASE + 3; // 某面样本数为零
const E_SAMPLE_NEG: u16 = SEG_BASE + 4;  // 样本延迟越下界（负值/零点异常）
const E_BUDGET_FACE: u16 = SEG_BASE + 5; // 面级 P99 超 B 段封顶线
const E_BUDGET_GLOBAL: u16 = SEG_BASE + 6; // 总账 P99 超全局封顶线
const E_MERGE_MONO: u16 = SEG_BASE + 7;  // 并账单调上界破坏
const E_BAL: u16 = SEG_BASE + 8;         // 分桶总数与样本和对账不平
const E_RERUN_FP: u16 = SEG_BASE + 9;    // 复跑指纹不一致
const E_PCT_BOUND: u16 = SEG_BASE + 10;  // P99 越出样本值域（非样本排序点）
const E_LEDGER_DUP: u16 = SEG_BASE + 11; // 并账总账条目面号重复
const E_LEDGER_TAG: u16 = SEG_BASE + 12; // 账目标签缺面号前缀

pub const FACET_LO: u8 = 9;   // B09
pub const FACET_HI: u8 = 20;  // B20
pub const FACET_COUNT: usize = 12;
pub const SAMPLES_PER_FACE: usize = 128;
pub const BUCKETS: usize = 64;         // 分桶数：延迟值域等宽切分
pub const BUCKET_SPAN_MS: u32 = 16;    // 每桶 16ms，值域 0..1024ms
pub const FACE_BUDGET_MS: u32 = 960;   // 面级封顶
pub const GLOBAL_BUDGET_MS: u32 = 1000; // 全局封顶
pub const LAT_MAX_MS: u32 = 1023;      // 采样值域上界

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            state: seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040),
        }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.state >> 33) as u32
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        self.next_u32() % bound
    }
}

/// 单面延迟账：B09..B20 之一，样本与分桶并行登记。
#[derive(Debug)]
pub struct LatencyFace {
    pub ordinal: u8,   // 9..=20
    pub tag: String,   // "B09".."B20"
    pub samples: Vec<u32>, // 毫秒
    pub buckets: Vec<u32>, // BUCKETS 个计数桶
}

impl LatencyFace {
    pub fn new(ordinal: u8) -> LatencyFace {
        let mut tag = String::from("B");
        tag.push((b'0' + (ordinal / 10)) as char);
        tag.push((b'0' + (ordinal % 10)) as char);
        LatencyFace {
            ordinal,
            tag,
            samples: Vec::new(),
            buckets: vec![0u32; BUCKETS],
        }
    }
    pub fn record(&mut self, ms: u32) {
        let b = if ms >= (BUCKETS as u32) * BUCKET_SPAN_MS {
            BUCKETS - 1
        } else {
            (ms / BUCKET_SPAN_MS) as usize
        };
        self.buckets[b] += 1;
        self.samples.push(ms);
    }
    pub fn bucket_total(&self) -> u64 {
        let mut t: u64 = 0;
        for c in self.buckets.iter() {
            t += *c as u64;
        }
        t
    }
    pub fn legal_ordinal(&self) -> bool {
        self.ordinal >= FACET_LO && self.ordinal <= FACET_HI
    }
    /// P99 分桶近似：r = ceil(0.99n) 自底计秩，自顶累计至 n-r+1 个，返回桶上界毫秒。
    pub fn p99_ms(&self) -> u32 {
        let n = self.samples.len();
        if n == 0 {
            return 0;
        }
        let rank = (n * 99 + 99) / 100; // ceil(n*0.99)：自底向上的秩
        let target = n - rank + 1;      // 自顶累计阈值：n-rank+1
        let mut acc: u64 = 0;
        let mut b = BUCKETS;
        while b > 0 {
            b -= 1;
            acc += self.buckets[b] as u64;
            if acc >= target as u64 {
                return ((b as u32) + 1) * BUCKET_SPAN_MS;
            }
        }
        BUCKET_SPAN_MS
    }
    pub fn max_ms(&self) -> u32 {
        let mut m: u32 = 0;
        for s in self.samples.iter() {
            if *s > m {
                m = *s;
            }
        }
        m
    }
    pub fn mean_x100(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sum: u64 = 0;
        for s in self.samples.iter() {
            sum += *s as u64;
        }
        (sum * 100) / (self.samples.len() as u64)
    }
}

/// 并账总账条目：面号 + 面 P99 + 面 P99 桶归属。
#[derive(Debug, PartialEq)]
pub struct LedgerEntry {
    pub ordinal: u8,
    pub face_p99: u32,
    pub bucket_index: usize,
    pub tag: String,
}

/// 全局并账账本。
#[derive(Debug)]
pub struct RollupLedger {
    pub entries: Vec<LedgerEntry>,
    pub total_buckets: Vec<u64>, // 各面 P99 并入全局桶
}

impl RollupLedger {
    pub fn new() -> RollupLedger {
        RollupLedger {
            entries: Vec::new(),
            total_buckets: vec![0u64; BUCKETS],
        }
    }
    pub fn merge_face(&mut self, face: &LatencyFace) {
        let p99 = face.p99_ms();
        let bi = if p99 == 0 {
            0
        } else {
            ((p99 / BUCKET_SPAN_MS) as usize).saturating_sub(1).min(BUCKETS - 1)
        };
        self.total_buckets[bi] += 1;
        self.entries.push(LedgerEntry {
            ordinal: face.ordinal,
            face_p99: p99,
            bucket_index: bi,
            tag: face.tag.clone(),
        });
    }
    pub fn merged_p99_ms(&self) -> u32 {
        let n: u64 = self.total_buckets.iter().fold(0u64, |a, c| a + *c);
        if n == 0 {
            return 0;
        }
        let rank = (n * 99 + 99) / 100;
        let target = n - rank + 1;
        let mut acc: u64 = 0;
        let mut b = BUCKETS;
        while b > 0 {
            b -= 1;
            acc += self.total_buckets[b];
            if acc >= target {
                return ((b as u32) + 1) * BUCKET_SPAN_MS;
            }
        }
        BUCKET_SPAN_MS
    }
    pub fn no_dup_ordinal(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].ordinal == self.entries[j].ordinal {
                    return false;
                }
            }
        }
        true
    }
    pub fn tag_prefixed(&self) -> bool {
        self.entries.iter().all(|e| e.tag.len() == 3 && e.tag.starts_with('B'))
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_FACET_CNT => "性能面不足十二：B09–B20 覆盖不完整",
        E_FACET_ORDER => "面号乱序：须 B09..B20 严格递增",
        E_SAMPLE_EMPTY => "某面样本数为零：采样通道失联",
        E_SAMPLE_NEG => "样本延迟越下界：零点异常须剔除重测",
        E_BUDGET_FACE => "面级 P99 超 B 段封顶线 960ms",
        E_BUDGET_GLOBAL => "总账 P99 超全局封顶线 1000ms",
        E_MERGE_MONO => "并账单调上界破坏：并账 P99 小于某面 P99",
        E_BAL => "分桶总数与样本和对账不平",
        E_RERUN_FP => "复跑指纹不一致：采样或分桶非确定",
        E_PCT_BOUND => "P99 越出样本值域：分桶统计失真",
        E_LEDGER_DUP => "并账总账面号重复：同一面二次并账",
        E_LEDGER_TAG => "账目标签缺 B 前缀或长度非三",
        _ => "未知诊断码",
    }
}

/// 单面确定性采样：混合基线 + 抖动 + 稀疏尾刺（LCG 决定）。
pub fn sample_face(face: &mut LatencyFace, seed: u64) {
    let mut rng = Lcg::new(seed);
    let mut k: usize = 0;
    while k < SAMPLES_PER_FACE {
        let base = 40u32 + ((face.ordinal as u32) * 12);
        let jitter = rng.below(96);
        let spike = if rng.below(32) == 0 {
            rng.below(480)
        } else {
            0
        };
        let ms = base + jitter + spike;
        if ms > 0 && ms <= LAT_MAX_MS {
            face.record(ms);
            k += 1;
        }
    }
}

/// 采样指纹：样本序列 FNV-1a（复跑一致性判据）。
pub fn face_fp(face: &LatencyFace) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in face.samples.iter() {
        let v = *s;
        h ^= ((v >> 24) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= ((v >> 16) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= ((v >> 8) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= (v & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 生成 B09..B20 全部十二面（严格递增、确定性采样）。
pub fn build_faces() -> Vec<LatencyFace> {
    let mut faces: Vec<LatencyFace> = Vec::new();
    let mut o: u8 = FACET_LO;
    while o <= FACET_HI {
        let mut f = LatencyFace::new(o);
        let seed = 0xF483_8400u64 + (o as u64) * 97;
        sample_face(&mut f, seed);
        faces.push(f);
        o += 1;
    }
    faces
}

/// 主巡检入口：R1..R9 场景，任一判据不满足返回 Err(诊断码)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 面覆盖与序：十二面、B09..B20 严格递增、标签合规。
    let faces = build_faces();
    let r1_ok = || -> bool {
        faces.len() == FACET_COUNT
            && faces.iter().all(|f| f.legal_ordinal())
            && faces.windows(2).all(|w| w[1].ordinal == w[0].ordinal + 1)
    };
    if !r1_ok() {
        if faces.len() != FACET_COUNT {
            return Err(E_FACET_CNT);
        }
        return Err(E_FACET_ORDER);
    }
    for f in faces.iter() {
        if !(f.tag.len() == 3 && f.tag.starts_with('B')) {
            return Err(E_LEDGER_TAG);
        }
    }
    hits += 1;

    // R2 样本健全：每面样本数足额、值域 (0, 1023]。
    for f in faces.iter() {
        if f.samples.len() != SAMPLES_PER_FACE {
            return Err(E_SAMPLE_EMPTY);
        }
        for s in f.samples.iter() {
            if *s == 0 || *s > LAT_MAX_MS {
                return Err(E_SAMPLE_NEG);
            }
        }
    }
    hits += 1;

    // R3 分桶对账：每面 bucket_total == 样本数。
    for f in faces.iter() {
        if f.bucket_total() != f.samples.len() as u64 {
            return Err(E_BAL);
        }
    }
    hits += 1;

    // R4 P99 值域闭合：每面 0 < p99 ≤ max 所在桶上界，且 p99 落在桶格上（16 的倍数）。
    for f in faces.iter() {
        let p = f.p99_ms();
        let m = f.max_ms();
        let m_up = ((m / BUCKET_SPAN_MS) + 1) * BUCKET_SPAN_MS;
        if p == 0 || p > m_up || (p % BUCKET_SPAN_MS) != 0 {
            return Err(E_PCT_BOUND);
        }
    }
    hits += 1;

    // R5 面级预算封顶：任一面 P99 ≤ 960ms。
    for f in faces.iter() {
        if f.p99_ms() > FACE_BUDGET_MS {
            return Err(E_BUDGET_FACE);
        }
    }
    hits += 1;

    // R6 并账 + 总账条目唯一性 + 标签前缀 + 全局封顶。
    let mut ledger = RollupLedger::new();
    for f in faces.iter() {
        ledger.merge_face(f);
    }
    if ledger.entries.len() != FACET_COUNT {
        return Err(E_LEDGER_DUP);
    }
    if !ledger.no_dup_ordinal() {
        return Err(E_LEDGER_DUP);
    }
    if !ledger.tag_prefixed() {
        return Err(E_LEDGER_TAG);
    }
    let g = ledger.merged_p99_ms();
    if g > GLOBAL_BUDGET_MS {
        return Err(E_BUDGET_GLOBAL);
    }
    hits += 1;

    // R7 并账单调上界：并账 P99 ≥ 任一面 P99。
    for f in faces.iter() {
        if g < f.p99_ms() {
            return Err(E_MERGE_MONO);
        }
    }
    hits += 1;

    // R8 并账对账：全局桶总数 == 面数（每面并账一条 P99）；桶归属与面 P99 位一致。
    let ledger_total: u64 = ledger.total_buckets.iter().fold(0u64, |a, c| a + *c);
    if ledger_total != faces.len() as u64 {
        return Err(E_BAL);
    }
    // 并账条目桶归属与面 P99 桶位一致。
    for (i, e) in ledger.entries.iter().enumerate() {
        if e.bucket_index != faces[i].p99_bucket_index() {
            return Err(E_BAL);
        }
    }
    hits += 1;

    // R9 复跑指纹一致：重建全部面，样本序列 FNV 指纹须逐面相同。
    let rerun = build_faces();
    if rerun.len() != faces.len() {
        return Err(E_RERUN_FP);
    }
    for (a, b) in faces.iter().zip(rerun.iter()) {
        if face_fp(a) != face_fp(b) || a.p99_ms() != b.p99_ms() {
            return Err(E_RERUN_FP);
        }
    }
    hits += 1;

    Ok(hits)
}

impl LatencyFace {
    /// 面 P99 的桶位下标（0..BUCKETS），供并账归属核对。
    pub fn p99_bucket_index(&self) -> usize {
        let p = self.p99_ms();
        if p == 0 {
            0
        } else {
            ((p / BUCKET_SPAN_MS) as usize).saturating_sub(1).min(BUCKETS - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::drill;
    #[test]
    fn drill_all_green() {
        assert!(drill().is_ok());
    }
}
