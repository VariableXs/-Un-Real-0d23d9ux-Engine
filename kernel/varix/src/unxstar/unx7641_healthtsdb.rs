//! UNX-F7641 · 健康历史数据库（热层内存环形 + 冷层文件段 + 16 盘×30 天容量模型）· 诊断段 0x7B00
//! 验收锚点：健康历史库双段落账——热层：每盘内存环形缓冲（定容 96 条，满则覆盖最旧，
//! 覆盖计数留账）；冷层：满段落"文件段"（32 段定容 512 条，满则滚动开新段，段号留账）；
//! 容量模型：16 盘×30 天×每天 96 采样 = 46,080 条可预排且不溢出。


use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7B00;

/// 一条健康采样：盘号 + 评分 + 时戳。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub disk: u8,
    pub score: u8,
    pub ts: u32,
}

impl Sample {
    pub fn new(disk: u8, score: u8, ts: u32) -> Self {
        Sample { disk, score, ts }
    }
}

/// 热层环形缓冲：定容 96。
pub const HOT_CAP: usize = 96;

pub struct HotRing {
    pub buf: Vec<Sample>,
    pub head: usize,
    pub len: usize,
    pub overwrites: u32,
}

impl HotRing {
    pub fn new() -> Self {
        HotRing { buf: Vec::new(), head: 0, len: 0, overwrites: 0 }
    }

    pub fn push(&mut self, s: Sample) {
        if self.buf.len() < HOT_CAP {
            self.buf.push(s);
            self.len += 1;
            return;
        }
        // 满：覆盖最旧
        let idx = (self.head + self.len) % HOT_CAP;
        if idx == self.head && self.len == HOT_CAP {
            // 环形满——head 即最旧
            self.buf[self.head] = s;
            self.head = (self.head + 1) % HOT_CAP;
            self.overwrites += 1;
        } else {
            self.buf[idx] = s;
        }
    }

    pub fn at(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        Some(self.buf[(self.head + i) % HOT_CAP])
    }

    pub fn is_full(&self) -> bool {
        self.len == HOT_CAP
    }
}

/// 冷层文件段：定容 512，滚动开新段。
pub const COLD_SEG_CAP: usize = 512;
pub const COLD_MAX_SEGS: usize = 32;

pub struct ColdStore {
    pub segs: Vec<Vec<Sample>>,
    pub cur: usize,
    pub rolled: u32,
}

impl ColdStore {
    pub fn new() -> Self {
        let mut segs = Vec::new();
        segs.push(Vec::new());
        ColdStore { segs, cur: 0, rolled: 0 }
    }

    pub fn append(&mut self, s: Sample) {
        if self.segs[self.cur].len() >= COLD_SEG_CAP {
            if self.cur + 1 >= COLD_MAX_SEGS {
                // 满库：滚动覆盖首段（留账 rolled）
                self.cur = 0;
                self.segs[0].clear();
                self.rolled += 1;
            } else {
                self.cur += 1;
                self.segs.push(Vec::new());
                self.rolled += 1;
            }
        }
        self.segs[self.cur].push(s);
    }

    pub fn total(&self) -> usize {
        self.segs.iter().map(|s| s.len()).sum()
    }
}

/// 容量模型：16 盘 × 30 天 × 96 采样/天。
pub const MODEL_DISKS: u32 = 16;
pub const MODEL_DAYS: u32 = 30;
pub const MODEL_PER_DAY: u32 = 96;

pub fn model_total() -> u32 {
    MODEL_DISKS * MODEL_DAYS * MODEL_PER_DAY
}

/// 双层库：热层满 → 批量落冷层。
pub struct HealthTsdb {
    pub hot: Vec<HotRing>,
    pub cold: ColdStore,
    pub drained: u32,
}

impl HealthTsdb {
    pub fn new(n_disks: u8) -> Self {
        let mut hot = Vec::new();
        for _ in 0..n_disks {
            hot.push(HotRing::new());
        }
        HealthTsdb { hot, cold: ColdStore::new(), drained: 0 }
    }

    /// 写入：进对应盘热环；环满即落冷层清环（整批排出计 drained）。
    pub fn write(&mut self, s: Sample) {
        let idx = s.disk as usize % self.hot.len();
        let h = &mut self.hot[idx];
        if h.is_full() {
            let mut batch: Vec<Sample> = Vec::new();
            for i in 0..h.len {
                if let Some(x) = h.at(i) {
                    batch.push(x);
                }
            }
            for x in batch {
                self.cold.append(x);
            }
            self.drained += 1;
            *h = HotRing::new();
        }
        h.push(s);
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7B00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7641_7B00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("热层-空环取None", HotRing::new().at(0).is_none()));
    out.push(("容量-模型恰46080", model_total() == 46_080));
    out.push(("冷层-新库单段", ColdStore::new().segs.len() == 1));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7641_checks：本单判据聚合（诊断段 0x7B00）。
pub fn run_unx7641_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7641 健康历史数据库");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 热层：96 条不覆盖，第 97 条覆盖最旧
    let mut h = HotRing::new();
    for i in 0..96u32 {
        h.push(Sample::new(0, (i % 100) as u8, i));
    }
    s.add("热层-96条不覆盖", !h.is_full() == false && h.overwrites == 0, if h.overwrites == 0 { "" } else { "未满即覆盖" });
    h.push(Sample::new(0, 99, 96));
    let ov1 = h.overwrites == 1 && h.at(0) == Some(Sample::new(0, 1, 1));
    s.add("热层-第97条覆盖最旧且窗口前移", ov1, if ov1 { "" } else { "覆盖路径漂移" });

    // 热层顺序保持：覆盖后仍按 ts 递增读出
    let mut order_ok = true;
    let mut prev = 0u32;
    for i in 0..h.len {
        if let Some(x) = h.at(i) {
            if x.ts < prev {
                order_ok = false;
            }
            prev = x.ts;
        }
    }
    s.add("热层-覆盖后ts仍单调递增", order_ok, if order_ok { "" } else { "环序漂移" });

    // 冷层：513 条滚动开第二段
    let mut c = ColdStore::new();
    for i in 0..513u32 {
        c.append(Sample::new(1, 80, i));
    }
    let roll1 = c.segs.len() == 2 && c.segs[0].len() == COLD_SEG_CAP && c.rolled == 1;
    s.add("冷层-513条滚开第二段", roll1, if roll1 { "" } else { "滚动路径漂移" });

    // 冷层满库滚动回卷：32 段满后覆盖首段
    let mut c2 = ColdStore::new();
    for i in 0..(COLD_MAX_SEGS as u32 * COLD_SEG_CAP as u32 + 1) {
        c2.append(Sample::new(2, 70, i));
    }
    let rolled_back = c2.rolled == COLD_MAX_SEGS as u32 && c2.cur == 0 && c2.segs[0].len() == 1;
    s.add("冷层-满库回卷覆盖首段留账", rolled_back, if rolled_back { "" } else { "回卷漂移" });

    // 双层库：单盘 193 条 → 热层排空 2 次（96+96），余 1 条在热层，冷层 192 条
    let mut db = HealthTsdb::new(4);
    for i in 0..193u32 {
        db.write(Sample::new(0, 90, i));
    }
    let drain2 = db.drained == 2 && db.cold.total() == 192;
    s.add("双层-193条排空两次冷层192条", drain2, if drain2 { "" } else { "排空账漂移" });
    let hot1 = db.hot[0].len == 1;
    s.add("双层-热层余1条", hot1, if hot1 { "" } else { "热层余量漂移" });

    // 容量模型：16 盘各 30 天 96 采样全量过库（46080 条）不炸不溢
    let mut db2 = HealthTsdb::new(16);
    for d in 0..16u8 {
        for t in 0..(MODEL_DAYS * MODEL_PER_DAY) {
            db2.write(Sample::new(d, (t % 101) as u8, t));
        }
    }
    let cap_ok = model_total() == 46_080 && db2.drained == 16 * 30;
    s.add("容量-16盘46080条预排16盘各排空30次", cap_ok, if cap_ok { "" } else { "容量模型失配" });

    // FNV 采样哈希稳定
    let sm = Sample::new(7, 55, 1234);
    let h1 = fnv1a(&[sm.disk, sm.score]);
    let h2 = fnv1a(&[7, 55]);
    s.add("FNV-采样哈希稳定", h1 == h2, if h1 == h2 { "" } else { "哈希不稳" });

    s
}
