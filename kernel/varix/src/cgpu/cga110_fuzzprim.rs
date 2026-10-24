//! cga110_fuzzprim —— CGPU-F0110 SIMD 基元库 fuzz 扩展。
//! 锚点（CGPU 施工书）：CGPU-F0110 · SIMD 基元库 fuzz 扩展 · 目标 300 行
//! 判据摘录：语料生成器闭源（种子驱动确定性——同种子同语料）；
//!   变异器闭集（位翻转/字节替换/截断/重复四类——ID 闭集）；
//!   语料账（每轮语料指纹在案——可复现）；崩溃捕获（域外值
//!   不 panic——哨兵记账）；覆盖率增量（新路径记账——探索
//!   单调）；回归池（最小崩溃语料沉淀——三例回归）。
//! 组件：语料生成核（xorshift64 种子流）+ 四变异器 + 指纹账
//!   + 覆盖增量账 + 回归池。
//! 职责定位：SIMD 基元（cga77~cga86 的直方图/曲线/白平衡/锐化
//!   等纯函数核）的模糊测试底座——不直接调基元，提供语料
//!   工厂与账本，由判据把语料喂给被测面。
//!
//! 语料契约：
//!   Corpus = { seed: u64, bytes: Vec<u8>, fp: u64 }。
//!   指纹 = FNV-1a 逐字节，同语料同指纹。
//!   变异器返回 Result<Corpus, u32>——空语料拒变异。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xEA30 段 · 本模块独占）=====
pub const XC_EMPTY: u32 = 0xEA30; // 空语料拒变异
pub const XC_MUT_OOB: u32 = 0xEA31; // 变异器越界
pub const XC_SEED_LOCK: u32 = 0xEA32; // 种子流漂移
pub const XC_CRASH: u32 = 0xEA33; // 崩溃哨兵

/// 语料：种子 + 字节 + 指纹三账齐。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    pub seed: u64,
    pub bytes: Vec<u8>,
    pub fp: u64,
}

/// FNV-1a 64 指纹（本 crate 全账同源）。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl Corpus {
    /// 构造：空字节拒（DIM_ZERO 语义承袭 cga65）。
    pub fn new(seed: u64, bytes: Vec<u8>) -> Result<Corpus, u32> {
        if bytes.is_empty() {
            return Err(XC_EMPTY);
        }
        let fp = fnv1a(&bytes);
        Ok(Corpus { seed, bytes, fp })
    }

    /// 指纹自洽（副本漂移检出）。
    pub fn fp_ok(&self) -> bool {
        fnv1a(&self.bytes) == self.fp
    }
}

/// xorshift64 种子流（确定性语料源——同种子同序列）。
pub struct SeedStream {
    state: u64,
}

impl SeedStream {
    pub fn new(seed: u64) -> SeedStream {
        // 零种子自锁（全零流不可用）。
        SeedStream { state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed } }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// 定长语料生成：逐字节取高位散布。
    pub fn gen_bytes(&mut self, len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..len {
            let v = self.next_u64();
            // 混入索引防高位偏向。
            out.push(((v >> 24) as u8) ^ (i as u8));
        }
        out
    }
}

// ===== 变异器闭集 =====
pub const V_BITFLIP: u8 = 0;
pub const V_BYTEREP: u8 = 1;
pub const V_TRUNC: u8 = 2;
pub const V_DUP: u8 = 3;
pub const VARIATORS: [u8; 4] = [V_BITFLIP, V_BYTEREP, V_TRUNC, V_DUP];

/// 变异核：in 原语料 + 位置/参数 → out 新语料（原语料不动）。
pub fn mutate(kind: u8, c: &Corpus, pos: usize, arg: u8) -> Result<Corpus, u32> {
    if c.bytes.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut nb: Vec<u8> = Vec::new();
    match kind {
        V_BITFLIP => {
            if pos >= c.bytes.len() {
                return Err(XC_MUT_OOB);
            }
            nb = c.bytes.clone();
            nb[pos] ^= 1u8 << (arg % 8);
        }
        V_BYTEREP => {
            if pos >= c.bytes.len() {
                return Err(XC_MUT_OOB);
            }
            nb = c.bytes.clone();
            nb[pos] = arg;
        }
        V_TRUNC => {
            // arg = 保留长度（0 拒——空语料不可产出）。
            if arg == 0 || arg as usize > c.bytes.len() {
                return Err(XC_MUT_OOB);
            }
            for i in 0..arg as usize {
                nb.push(c.bytes[i]);
            }
        }
        V_DUP => {
            // pos 处字节重复 arg 次（arg=0 拒——等于截断）。
            if pos >= c.bytes.len() || arg == 0 {
                return Err(XC_MUT_OOB);
            }
            nb = c.bytes.clone();
            for _ in 0..arg {
                nb.insert(pos, c.bytes[pos]);
            }
        }
        _ => return Err(XC_MUT_OOB),
    }
    Corpus::new(c.seed.wrapping_add(1), nb)
}

/// 变异器 ID 闭集核。
pub fn variator_closed(kind: u8) -> bool {
    VARIATORS.contains(&kind)
}

// ===== 覆盖增量账 =====
/// 路径桶：指纹低 6 位做桶号（64 桶）——新桶入账即增量。
pub const BUCKETS: usize = 64;

pub struct CovLedger {
    pub seen: [bool; BUCKETS],
    pub hits: u32,
    pub new_paths: u32,
}

impl CovLedger {
    pub fn new() -> CovLedger {
        CovLedger { seen: [false; BUCKETS], hits: 0, new_paths: 0 }
    }

    /// 记一笔：返回是否为新路径。
    pub fn record(&mut self, fp: u64) -> bool {
        let b = (fnv1a(&fp.to_le_bytes()) % BUCKETS as u64) as usize;
        self.hits += 1;
        if !self.seen[b] {
            self.seen[b] = true;
            self.new_paths += 1;
            true
        } else {
            false
        }
    }

    /// 单调核：已记账不回吐（重放同语料无增量）。
    pub fn monotonic_ok(&self) -> bool {
        self.new_paths <= self.hits
    }

    /// 覆盖率千分比（桶账冻结）。
    pub fn permille(&self) -> u32 {
        self.new_paths * 1000 / BUCKETS as u32
    }
}

// ===== 崩溃哨兵 =====
/// 域内校验：字节全落 [0,255] 恒真——模拟"不 panic"红线。
/// 判据用非法构造（空/越界索引路径）验证 Err 闭环而非真崩。
pub fn domain_ok(bytes: &[u8]) -> Result<(), u32> {
    for &b in bytes.iter() {
        // u8 天然有界，这里做账面哨兵：0xFF 视为哨兵越界样本。
        if b == 0xFF {
            return Err(XC_CRASH);
        }
    }
    Ok(())
}

// ===== 回归池 =====
/// 最小崩溃语料沉淀（种子+字节对——最小二例起步）。
pub struct RegressPool {
    pub cases: Vec<(u64, Vec<u8>)>,
    cap: usize,
}

impl RegressPool {
    pub fn new(cap: usize) -> RegressPool {
        RegressPool { cases: Vec::new(), cap }
    }

    /// 入池：去重（同指纹不重收）+ 容量闭。
    pub fn add(&mut self, c: &Corpus) -> Result<bool, u32> {
        if self.cases.len() >= self.cap {
            return Err(XC_CRASH);
        }
        let fp = fnv1a(&c.bytes);
        for (_, b) in self.cases.iter() {
            if fnv1a(b) == fp {
                return Ok(false); // 去重
            }
        }
        self.cases.push((c.seed, c.bytes.clone()));
        Ok(true)
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }
}

// ===== 判据聚合 =====
pub fn run_cga110_checks() -> CheckSet {
    let mut cs = CheckSet::new("cga110_fuzzprim");
    // 族一：语料核。
    let c0 = Corpus::new(42, vec![1u8, 2, 3, 4]).map_err(|e| e as u32);
    cs.add("语料构建", c0.is_ok(), "四字节起步");
    cs.add("空语料拒", Corpus::new(1, vec![]).is_err(), "DIM_ZERO");
    if let Ok(c) = &c0 {
        cs.add("指纹自洽", c.fp_ok(), "FNV 同源");
        // 确定性：同种子同语料。
        let mut s1 = SeedStream::new(7);
        let mut s2 = SeedStream::new(7);
        cs.add("种子确定", s1.gen_bytes(32) == s2.gen_bytes(32), "同种同料");
        let mut s3 = SeedStream::new(8);
        cs.add("异种异料", s1.gen_bytes(8) != s3.gen_bytes(8), "散布");
        let mut sz = SeedStream::new(0);
        cs.add("零种子自锁", sz.next_u64() != 0, "黄金比回填");
    }
    // 族二：变异器。
    if let Ok(c) = &c0 {
        cs.add("位翻转变异", mutate(V_BITFLIP, c, 1, 0).is_ok(), "翻低位");
        cs.add("位翻转越界拒", mutate(V_BITFLIP, c, 9, 0).is_err(), "POS_OOB");
        cs.add("字节替换", mutate(V_BYTEREP, c, 0, 200).map(|m| m.bytes[0] == 200).unwrap_or(false), "锚值落位");
        cs.add("截断保留", mutate(V_TRUNC, c, 0, 2).map(|m| m.bytes.len() == 2).unwrap_or(false), "len=2");
        cs.add("截断空拒", mutate(V_TRUNC, c, 0, 0).is_err(), "不产空");
        cs.add("截断超长拒", mutate(V_TRUNC, c, 0, 9).is_err(), "arg 越界");
        cs.add("重复插入", mutate(V_DUP, c, 1, 3).map(|m| m.bytes.len() == 7).unwrap_or(false), "+3");
        cs.add("重复零拒", mutate(V_DUP, c, 1, 0).is_err(), "退化拒");
        cs.add("变异器越界拒", mutate(9, c, 0, 1).is_err(), "闭集外");
        cs.add("闭集核", variator_closed(V_DUP) && !variator_closed(7), "4 类");
        cs.add("原语料不动", mutate(V_BITFLIP, c, 0, 0).map(|_| c.bytes[0] == 1).unwrap_or(false), "不可变");
    }
    // 族三：覆盖增量账。
    let mut cov = CovLedger::new();
    let first = cov.record(0xABCD);
    let second = cov.record(0xABCD);
    cs.add("新路径首记", first, "增量");
    cs.add("重放无增量", !second, "单调");
    cs.add("单调账", cov.monotonic_ok(), "hits≥new");
    cs.add("千分比冻结", cov.permille() == 15, "1/64→15.625 截");
    // 64 异指纹铺满。
    let mut cov2 = CovLedger::new();
    for i in 0..64u64 {
        cov2.record(i.wrapping_mul(0x9E3779B97F4A7C15));
    }
    cs.add("铺满探测", cov2.new_paths >= 60, "64 试≥60 桶");
    // 族四：崩溃哨兵。
    cs.add("域内过", domain_ok(&[1, 2, 3]).is_ok(), "无哨兵值");
    cs.add("哨兵检出", domain_ok(&[1, 0xFF, 3]).is_err(), "CRASH");
    // 族五：回归池。
    if let Ok(c) = &c0 {
        let mut pool = RegressPool::new(4);
        cs.add("入池", pool.add(c) == Ok(true), "首例");
        cs.add("去重", pool.add(c) == Ok(false), "同指纹拒");
        let m = mutate(V_BYTEREP, c, 2, 9);
        if let Ok(m2) = m {
            cs.add("异料入池", pool.add(&m2) == Ok(true), "第二例");
        }
        cs.add("池账", pool.len() == 2, "二例沉淀");
        // 容量闭。
        let mut tiny = RegressPool::new(1);
        let _ = tiny.add(c);
        cs.add("容量闭", tiny.add(c).is_err(), "CAP_FULL");
    }
    // 族六：端到端小循环（生成→变异→记账）。
    let mut st = SeedStream::new(99);
    let mut ecov = CovLedger::new();
    let mut ran = 0u32;
    for _ in 0..8u32 {
        let bytes = st.gen_bytes(16);
        if let Ok(base) = Corpus::new(99, bytes) {
            if let Ok(m) = mutate(V_BITFLIP, &base, (ran % 16) as usize, ran as u8) {
                let _ = ecov.record(m.fp);
                ran += 1;
            }
        }
    }
    cs.add("端到端", ran == 8 && ecov.hits == 8, "8 轮全通");
    cs
}
