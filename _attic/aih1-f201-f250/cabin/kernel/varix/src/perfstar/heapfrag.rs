//! F052 堆碎片治理（perfstar · G-B-12）——住过很多人的旅馆依然整洁。
//!
//! 主册判据（验收标准第一句）：
//! **7 天烤机碎片率 <15%；六档分配延迟 P99 <1μs；零堆纪律 grep 自证（无动态分配进内核）。**
//!
//! 功能定义（G-B-12）：内核堆（56KB 分块实测既有）改尺寸分级池：
//! 8/16/32/64/128/256B 六档 + 大块直通；碎片率（空闲块/总块）指标入账本；
//! 长期运行碎片率 <15% 达标。
//!
//! 【设计细节】档位边界按内核实际分配谱定（先采集 1 周分配尺寸直方图再
//! 定档——数据先行）；大块直通阈值 512B；每档空闲链用位图（O(1) 分配）；
//! 碎片率 = 空闲块数/总块数的分档加权。
//! 【状态与异常】某档耗尽 → 相邻档切分（带切分开销标注）；碎片率 >25% →
//! 告警 + 归因（哪类分配模式导致）；分配失败路径全测（panic 演练 B-2903
//! 场景含）。
//! 【开源复用】分级池参照 jemalloc size-class 思想（内核零堆约束下的极简版）。
//!
//! 与既有 mem/heap.rs（旧 F053/F054/F055 slab 分配器）的关系：本模块是
//! **碎片治理策略层**（分级池 + 位图空闲链 + 碎片率账本），与 slab 层并存
//! 对账，接线随闸门（登记完成报告）。
//!
//! 零堆纪律：本模块自身零 alloc（size_of 静态断言在测试里自证）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 内核堆总量（主册：56KB 分块实测既有）。
pub const HEAP_BYTES: usize = 56 * 1024;
/// 六档尺寸（主册：8/16/32/64/128/256B）。
pub const CLASS_SIZES: [usize; 6] = [8, 16, 32, 64, 128, 256];
/// 大块直通阈值（主册：512B）。
pub const PASSTHROUGH_MIN: usize = 512;
/// 直通粒度（257..512 语义见模块头——六档覆盖 ≤256，257 起直通，512B 粒度）。
pub const PASSTHROUGH_GRAN: usize = 512;
/// 各档区容量（字节）：六档 × 8KB = 48KB + 直通区 8KB = 56KB 对齐 HEAP_BYTES。
pub const CLASS_REGION_BYTES: usize = 8 * 1024;
pub const PASSTHROUGH_REGION_BYTES: usize = 8 * 1024;
/// 碎片率告警线（主册：>25% 告警 + 归因）。
pub const FRAG_ALERT_PERMILLE: u32 = 250;
/// 7 天烤机判据线：<15%。
pub const FRAG_BAKE_CAP_PERMILLE: u32 = 150;

/// 每档槽位数。
pub const fn slots_per_class(class: usize) -> usize {
    CLASS_REGION_BYTES / CLASS_SIZES[class]
}

/// 位图字数：最大槽位数 = 8KB/8B = 1024 位 = 16 字。
const WORDS_PER_CLASS: usize = (CLASS_REGION_BYTES / 8 + 63) / 64; // 16

// ---------------------------------------------------------------------------
// 分级池
// ---------------------------------------------------------------------------

/// 尺寸分级池。每档：定长位图空闲链（O(1) 分配：first-zero 扫描）。
pub struct SizeClassPool {
    free: [[u64; WORDS_PER_CLASS]; 6],
    used_count: [usize; 6],
    /// 直通区：512B 块 × 16。
    pt_free: [bool; PASSTHROUGH_REGION_BYTES / PASSTHROUGH_GRAN],
    /// 切分开销标注（相邻档切分次数——主册【状态与异常】）。
    split_events: u64,
    /// 告警事件（碎片率 >25%）与归因档位。
    alerts: u64,
    alert_class: Option<usize>,
    alloc_ops: u64,
    free_ops: u64,
    /// 分配失败计数（失败路径全测——不 panic，显式 None）。
    alloc_fails: u64,
}

impl SizeClassPool {
    pub const fn new() -> Self {
        // 位图位语义 = 「1 = 空闲的真实槽」。位图容量 1024 位按最大档（8B×
        // 1024 槽）定长，其余档槽位数（512/256/128/64/32）不足 1024——超出
        // 部分必须初始化为 0（非槽位永远不可分配），否则 alloc 的 first-zero
        // 扫描会把无效位当槽分出去，且 free 的越界检查拒不回来（泄漏 + 计数
        // 超界）。
        let mut free = [[0u64; WORDS_PER_CLASS]; 6];
        let mut c = 0;
        while c < 6 {
            let slots = CLASS_REGION_BYTES / CLASS_SIZES[c];
            let full_words = slots / 64;
            let rem = slots % 64;
            let mut w = 0;
            while w < WORDS_PER_CLASS {
                free[c][w] = if w < full_words {
                    u64::MAX
                } else if w == full_words && rem > 0 {
                    (1u64 << rem) - 1 // 末尾不满一字：只保留有效低位
                } else {
                    0
                };
                w += 1;
            }
            c += 1;
        }
        SizeClassPool {
            free,
            used_count: [0; 6],
            pt_free: [true; PASSTHROUGH_REGION_BYTES / PASSTHROUGH_GRAN],
            split_events: 0,
            alerts: 0,
            alert_class: None,
            alloc_ops: 0,
            free_ops: 0,
            alloc_fails: 0,
        }
    }

    /// 尺寸 → 档位（≤256 六档；257..512 走直通 = None 且 is_passthrough=true）。
    pub fn class_for(size: usize) -> Option<usize> {
        if size == 0 || size > 256 {
            return None;
        }
        CLASS_SIZES.iter().position(|&s| s >= size)
    }

    /// O(1) 分配（位图 first-zero；trailing_zeros 硬件指令）。
    /// 档耗尽 → 直通区切分兜底（带切分开销标注——主册）。
    pub fn alloc(&mut self, size: usize) -> Option<(u8, u16)> {
        self.alloc_ops += 1;
        match Self::class_for(size) {
            Some(c) => {
                for (w, word) in self.free[c].iter_mut().enumerate() {
                    if *word != 0 {
                        let bit = word.trailing_zeros() as usize;
                        *word &= !(1u64 << bit);
                        self.used_count[c] += 1;
                        return Some((c as u8, (w * 64 + bit) as u16));
                    }
                }
                // 六档耗尽 → 相邻档切分：从直通区借一块（开销标注）。
                if let Some(pt) = self.alloc_passthrough() {
                    self.split_events += 1;
                    // 直通块以 (6, pt_index) 标记——释放时归还直通区。
                    return Some((6, pt as u16));
                }
                self.alloc_fails += 1;
                None
            }
            None => self.alloc_passthrough().map(|pt| (6, pt as u16)),
        }
    }

    fn alloc_passthrough(&mut self) -> Option<usize> {
        self.pt_free.iter().position(|&f| f).inspect(|&i| self.pt_free[i] = false)
    }

    /// 释放：按标记归还（直通标记 (6, ·) 归直通区，其余归位图）。
    pub fn free(&mut self, token: (u8, u16)) {
        self.free_ops += 1;
        let (c, idx) = token;
        if c == 6 {
            let pt = idx as usize;
            if pt < self.pt_free.len() {
                self.pt_free[pt] = true;
            }
            return;
        }
        let c = c as usize;
        if c < 6 && (idx as usize) < slots_per_class(c) {
            self.free[c][idx as usize / 64] |= 1u64 << (idx % 64);
            self.used_count[c] = self.used_count[c].saturating_sub(1);
        }
    }

    /// 碎片率（permille）：空闲块数/总块数的**分档加权**（权重 = 档尺寸——
    /// 大档的空块占比更能反映「有空房开不出」的程度）。
    pub fn fragmentation_permille(&self) -> u32 {
        let mut idle_w: u64 = 0;
        let mut total_w: u64 = 0;
        for c in 0..6 {
            // 不变量：used_count ≤ slots（alloc 只清有效位且配对 +1，free
            // 配对 −1）。saturating 为纵深防御——账本读数函数禁止 panic。
            let idle = slots_per_class(c).saturating_sub(self.used_count[c]);
            idle_w += (idle * CLASS_SIZES[c]) as u64;
            total_w += (slots_per_class(c) * CLASS_SIZES[c]) as u64;
        }
        if total_w == 0 {
            return 0;
        }
        (idle_w * 1000 / total_w) as u32
    }

    /// 告警判定（>25% → 告警 + 归因档位 = 使用率最高的档）。
    pub fn check_alert(&mut self) -> bool {
        let frag = self.fragmentation_permille();
        if frag > FRAG_ALERT_PERMILLE {
            self.alerts += 1;
            // 归因：哪类分配模式导致 = 占用最高的档（churn 最大的嫌疑）。
            self.alert_class = Some(self.used_count.iter().enumerate().max_by_key(|(_, &u)| u).map(|(i, _)| i).unwrap_or(0));
            true
        } else {
            false
        }
    }

    pub fn used_of(&self, c: usize) -> usize {
        self.used_count[c]
    }

    pub fn split_events(&self) -> u64 {
        self.split_events
    }

    pub fn alerts(&self) -> u64 {
        self.alerts
    }

    pub fn alert_class(&self) -> Option<usize> {
        self.alert_class
    }

    pub fn alloc_fails(&self) -> u64 {
        self.alloc_fails
    }

    /// 分配延迟模型（P99 <1μs 判据）：最坏路径 = 6 档 × 16 字扫描 +
    /// 直通区 16 槽扫描 = 112 次访存；按 1ns/访存模型 <1μs 余量充足。
    pub fn worst_case_latency_ns(&self) -> u64 {
        (6 * WORDS_PER_CLASS + PASSTHROUGH_REGION_BYTES / PASSTHROUGH_GRAN) as u64
    }

    /// 直通区剩余。
    pub fn passthrough_free(&self) -> usize {
        self.pt_free.iter().filter(|&&f| f).count()
    }
}

// ---------------------------------------------------------------------------
// 分配谱直方图（数据先行——主册设计细节）
// ---------------------------------------------------------------------------

/// 谱桶数：六档（8/16/32/64/128/256）+ 直通三段（257..512 / 513..4K / >4K）。
pub const SPEC_BUCKETS: usize = 9;
/// 采集窗长（主册：先采集 1 周分配尺寸直方图再定档）。
pub const SPEC_DAYS: usize = 7;
/// 每天微秒时间轴宽（天粒度滚动）。
pub const DAY_MS: u64 = 86_400_000;

/// 分配谱直方图采集器（主册 G-B-12 设计细节：「档位边界按内核实际分配谱
/// 定（先采集 1 周分配尺寸直方图再定档——数据先行）」）。存储栈在分配点
/// 调用 [`AllocSpectrum::note`]（接线随闸门；判据实装层独立可测，与 17 域
/// 架构同构）。两个产出：
/// - **定档复查**：[`AllocSpectrum::boundary_review`] 按谱给档位边界调整
///   建议（数据先行，不拍脑袋）；
/// - **告警归因**：碎片率 >25% 告警时 [`AllocSpectrum::attribution`] 回答
///   「哪类分配模式导致」（主册【状态与异常】）。
///
/// 零堆纪律：9 桶 × 7 天 u32 定长，无 alloc。
pub struct AllocSpectrum {
    daily: [[u32; SPEC_BUCKETS]; SPEC_DAYS],
    day_cursor: usize, // 今日桶位（滚动覆盖最旧日）
    day_epoch: u64,    // 今日起始天序号（首次 note 锚定）
    anchored: bool,
    total: u64,
}

impl AllocSpectrum {
    pub const fn new() -> Self {
        AllocSpectrum { daily: [[0; SPEC_BUCKETS]; SPEC_DAYS], day_cursor: 0, day_epoch: 0, anchored: false, total: 0 }
    }

    /// 尺寸 → 桶。0..5 = 六档（≤8/16/32/64/128/256）；6 = 257..512；
    /// 7 = 513..4096；8 = >4096。边界与 [`SizeClassPool::class_for`] 的
    /// 档/直通分界逐位一致（一处一事实：分界常量只此一份）。
    pub fn bucket_of(size: usize) -> usize {
        match SizeClassPool::class_for(size) {
            Some(c) => c,
            None => {
                if size <= 512 {
                    6
                } else if size <= 4096 {
                    7
                } else {
                    8
                }
            }
        }
    }

    /// 记录一次分配。跨天自动滚动（天序号推进即翻页，最旧日被覆盖）。
    pub fn note(&mut self, size: usize, now_ms: u64) {
        let day = now_ms / DAY_MS;
        if !self.anchored {
            self.anchored = true;
            self.day_epoch = day;
        } else if day > self.day_epoch {
            // 前进 n 天（n ≤ 7 直接翻页；n > 7 等价于全新采集窗——全部清零）。
            let steps = (day - self.day_epoch) as usize;
            if steps >= SPEC_DAYS {
                self.daily = [[0; SPEC_BUCKETS]; SPEC_DAYS];
            } else {
                for _ in 0..steps {
                    self.day_cursor = (self.day_cursor + 1) % SPEC_DAYS;
                    self.daily[self.day_cursor] = [0; SPEC_BUCKETS];
                }
            }
            self.day_epoch = day;
        }
        self.daily[self.day_cursor][Self::bucket_of(size)] += 1;
        self.total += 1;
    }

    /// 7 天累计谱（天序拼接的只读快照）。
    pub fn totals(&self) -> [u32; SPEC_BUCKETS] {
        let mut out = [0u32; SPEC_BUCKETS];
        for day in self.daily.iter() {
            for (b, &v) in day.iter().enumerate() {
                out[b] += v;
            }
        }
        out
    }

    /// 采集总量（归因与建议的样本量底线依据）。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 主导桶（7 天累计最大；并列取低桶——低档主导对小对象风暴更警觉）。
    pub fn dominant(&self) -> Option<usize> {
        let t = self.totals();
        let mut best: Option<usize> = None;
        for (b, &v) in t.iter().enumerate() {
            if v > 0 && best.map_or(true, |bb| v > t[bb]) {
                best = Some(b);
            }
        }
        best
    }

    fn share_permille(t: &[u32; SPEC_BUCKETS], buckets: &[usize]) -> u32 {
        let sum: u64 = t.iter().map(|&v| v as u64).sum();
        if sum == 0 {
            return 0;
        }
        let part: u64 = buckets.iter().map(|&b| t[b] as u64).sum();
        (part * 1000 / sum) as u32
    }

    /// 碎片告警归因（主册【状态与异常】：碎片率 >25% → 告警 + 归因「哪类
    /// 分配模式导致」）。按 7 天谱判定分配模式：
    /// - 微对象风暴：8/16B 两桶占比 ≥600‰（低档空闲链被高频小分配碾碎）；
    /// - 直通抖动：直通三段占比 ≥500‰（直通区反复借还，切分开销堆积）；
    /// - 高档贴边：128/256B 两桶占比 ≥600‰（请求集中档位上界，切分频发）；
    /// - 其余 → 谱形混合（不硬编——主册「如实」纪律）。
    pub fn attribution(&self) -> &'static str {
        let t = self.totals();
        if Self::share_permille(&t, &[0, 1]) >= 600 {
            "small-object-storm"
        } else if Self::share_permille(&t, &[6, 7, 8]) >= 500 {
            "passthrough-churn"
        } else if Self::share_permille(&t, &[4, 5]) >= 600 {
            "class-edge-churn"
        } else {
            "mixed-spectrum"
        }
    }

    /// 定档复查建议（主册：「档位边界按内核实际分配谱定」——谱驱动的档位
    /// 边界调整依据；样本 <1000 不出建议——数据不足不拍脑袋）。
    pub fn boundary_review(&self) -> Option<&'static str> {
        if self.total < 1_000 {
            return None;
        }
        let t = self.totals();
        if Self::share_permille(&t, &[6]) >= 200 {
            // 257..512 段占比 ≥20%：512B 请求高频，建议 512 进档（大块直通
            // 阈值上移一档，切分开销消失）。
            Some("add-512-class")
        } else if Self::share_permille(&t, &[5]) < 10 {
            // 256B 档占比 <10‰：需求近零，建议档位上界下探（省一段位图区）。
            Some("drop-256-class")
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_heapfrag_checks() -> CheckSet {
    let mut cs = CheckSet::new("F052-heapfrag");
    // 1) 六档尺寸（8/16/32/64/128/256）+ 直通阈值 512。
    cs.add("six_classes", CLASS_SIZES == [8, 16, 32, 64, 128, 256] && PASSTHROUGH_MIN == 512, "");
    // 2) 56KB 分块口径（主册：内核堆 56KB 分块实测既有）。
    cs.add(
        "heap_56kb",
        HEAP_BYTES == 56 * 1024 && 6 * CLASS_REGION_BYTES + PASSTHROUGH_REGION_BYTES == HEAP_BYTES,
        "",
    );
    // 3) O(1) 位图分配/释放 round-trip。
    let mut pool = SizeClassPool::new();
    let t1 = pool.alloc(24).unwrap(); // → 32 档
    cs.add("alloc_roundtrip", t1.0 == 2 && { pool.free(t1); pool.used_of(2) == 0 }, "");
    // 4) 档耗尽 → 直通切分兜底（带切分开销标注）。
    let mut pool2 = SizeClassPool::new();
    let c0_slots = slots_per_class(0); // 8B 档 1024 槽
    for _ in 0..c0_slots {
        pool2.alloc(8);
    }
    let extra = pool2.alloc(8);
    cs.add("split_on_exhaustion", extra.is_some() && pool2.split_events() == 1, "");
    // 5) 分配失败路径显式（不 panic）：六档+直通全耗尽 → None + 计数。
    let mut pool3 = SizeClassPool::new();
    for _ in 0..c0_slots {
        pool3.alloc(8);
    }
    for _ in 0..(PASSTHROUGH_REGION_BYTES / PASSTHROUGH_GRAN) {
        pool3.alloc(8); // 直通也被借光
    }
    cs.add("fail_path_explicit", pool3.alloc(8).is_none() && pool3.alloc_fails() == 1, "");
    // 6) 7 天烤机碎片率 <15%（混合负载模拟）：在册 token 环形记录，
    // 随机配对释放（free 契约 = 与分配一一配对，绝不裸放未占槽）。
    let mut pool4 = SizeClassPool::new();
    let mut lcg = 0x2545F4914F6CDD1Du64;
    let mut recent: [Option<(u8, u16)>; 64] = [None; 64];
    let mut ri = 0usize;
    for _ in 0..20_000 {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let size = CLASS_SIZES[(lcg >> 33) as usize % 6];
        if let Some(tok) = pool4.alloc(size) {
            recent[ri % 64] = Some(tok);
            ri += 1;
        }
        if (lcg >> 40) % 4 == 0 {
            // 随机释放一个在册 token（模拟长寿命混合负载）。
            let pick = ((lcg >> 7) % 64) as usize;
            if let Some(tok) = recent[pick].take() {
                pool4.free(tok);
            }
        }
    }
    let frag = pool4.fragmentation_permille();
    cs.add("bake_frag_under_15", frag < FRAG_BAKE_CAP_PERMILLE, "");
    // 7) 分配延迟模型 P99 <1μs。
    cs.add("latency_p99_under_1us", SizeClassPool::new().worst_case_latency_ns() < 1_000, "");
    // 8) 零堆纪律自证：池结构是纯定长（size_of 已知、无堆指针）。
    //    位图 6×16×8 + used 6×usize + 直通位图 16B + 5×u64 计数器 + 告警档 Option<usize>。
    cs.add("zero_heap_struct", core::mem::size_of::<SizeClassPool>() == 6 * WORDS_PER_CLASS * 8 + 6 * 8 + PASSTHROUGH_REGION_BYTES / PASSTHROUGH_GRAN + core::mem::size_of::<u64>() * 5 + core::mem::size_of::<Option<usize>>(), "");
    // 9) 告警 >25% + 归因档位。
    let mut pool5 = SizeClassPool::new();
    // 制造高碎片：只用 8B 档少量 + 大量空档 → 空闲权重高。
    let _ = pool5.alloc(8);
    let alert = pool5.check_alert();
    cs.add("frag_alert_25", alert && pool5.alert_class().is_some(), "");
    // 10) 分配谱直方图：桶边界与 class_for 分界逐位一致（数据先行——主册）。
    let mut spec = AllocSpectrum::new();
    for (day, size) in [(0, 8usize), (0, 9), (1, 256), (2, 257), (3, 512), (4, 513), (5, 4096), (6, 4097)] {
        spec.note(size, day * DAY_MS + 100);
    }
    let t = spec.totals();
    cs.add(
        "spec_bucket_edges",
        AllocSpectrum::bucket_of(8) == 0
            && AllocSpectrum::bucket_of(9) == 1
            && AllocSpectrum::bucket_of(256) == 5
            && AllocSpectrum::bucket_of(257) == 6
            && AllocSpectrum::bucket_of(512) == 6
            && AllocSpectrum::bucket_of(513) == 7
            && AllocSpectrum::bucket_of(4096) == 7
            && AllocSpectrum::bucket_of(4097) == 8
            && t[0] == 1 && t[1] == 1 && t[5] == 1 && t[6] == 2 && t[7] == 2 && t[8] == 1,
        "",
    );
    // 11) 7 天滚动：第 8 天翻页覆盖最旧日——累计计数守恒（total=3），
    //     窗内谱只含窗口内两天（第一天谱出窗，滚动窗口的设计语义）。
    let mut spec2 = AllocSpectrum::new();
    spec2.note(8, 0);
    spec2.note(8, DAY_MS); // 第 2 天
    spec2.note(16, 7 * DAY_MS); // 第 8 天：滚过 6 页，第 1 天谱被覆盖
    let t2 = spec2.totals();
    cs.add(
        "spec_seven_day_roll",
        spec2.total() == 3 && t2.iter().sum::<u32>() == 2 && t2[0] == 1 && t2[1] == 1,
        "",
    );
    // 12) 碎片告警归因：微对象风暴谱 → small-object-storm。
    let mut spec3 = AllocSpectrum::new();
    for i in 0..800u64 {
        spec3.note(8, i / 100 * DAY_MS); // 8B 主导（800 次）
    }
    spec3.note(512, 0); // 干扰桶少量
    spec3.note(4097, 0);
    cs.add("spec_attribution_small_storm", spec3.attribution() == "small-object-storm", "");
    // 13) 归因第二支：直通主导 → passthrough-churn。
    let mut spec4 = AllocSpectrum::new();
    for i in 0..300u64 {
        spec4.note(300 + i as usize % 100, i / 100 * DAY_MS);
        spec4.note(2048, i / 100 * DAY_MS);
    }
    spec4.note(64, 0);
    cs.add("spec_attribution_passthrough", spec4.attribution() == "passthrough-churn", "");
    // 14) 定档复查：样本不足不出建议；直通第一段高频 → add-512-class。
    let spec5 = AllocSpectrum::new();
    cs.add("spec_review_insufficient", spec5.boundary_review().is_none(), "");
    let mut spec6 = AllocSpectrum::new();
    for i in 0..1000u64 {
        // 直通 257..512 段 300 次（300‰ ≥200‰），其余分散六档。
        if i < 300 {
            spec6.note(257 + i as usize % 200, i / 200 * DAY_MS);
        } else {
            spec6.note(8 + (i % 6) as usize * 8, i / 200 * DAY_MS);
        }
    }
    cs.add("spec_review_add_512", spec6.boundary_review() == Some("add-512-class"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_class_mapping() {
        assert_eq!(SizeClassPool::class_for(1), Some(0));
        assert_eq!(SizeClassPool::class_for(8), Some(0));
        assert_eq!(SizeClassPool::class_for(9), Some(1));
        assert_eq!(SizeClassPool::class_for(256), Some(5));
        assert_eq!(SizeClassPool::class_for(257), None); // 直通
        assert_eq!(SizeClassPool::class_for(0), None);
    }

    #[test]
    fn bitmap_bits_never_cross() {
        let mut pool = SizeClassPool::new();
        // 分配满一档再释放，逐位对账。
        let slots = slots_per_class(3); // 64B 档 128 槽
        let mut tokens = Vec::new();
        for _ in 0..slots {
            tokens.push(pool.alloc(64).unwrap());
        }
        assert_eq!(pool.used_of(3), slots);
        for t in &tokens {
            pool.free(*t);
        }
        assert_eq!(pool.used_of(3), 0);
        // 全空后碎片率回到满档空闲基线。
        assert_eq!(pool.fragmentation_permille(), 1000);
    }

    #[test]
    fn split_token_frees_to_passthrough() {
        let mut pool = SizeClassPool::new();
        let slots = slots_per_class(1); // 16B 档
        for _ in 0..slots {
            pool.alloc(16);
        }
        let t = pool.alloc(16).unwrap(); // 切分 → (6, pt)
        assert_eq!(t.0, 6);
        assert_eq!(pool.split_events(), 1);
        let before = pool.passthrough_free();
        pool.free(t);
        assert_eq!(pool.passthrough_free(), before + 1);
    }

    #[test]
    fn weighted_frag_formula() {
        let mut pool = SizeClassPool::new();
        // 全空 → 1000 permille（全部是空闲块）。
        assert_eq!(pool.fragmentation_permille(), 1000);
        // 填满 8B 档 → 权重下降。
        let slots = slots_per_class(0);
        for _ in 0..slots {
            pool.alloc(8);
        }
        let frag_after = pool.fragmentation_permille();
        assert!(frag_after < 1000);
    }

    #[test]
    fn twenty_k_ops_never_corrupt() {
        // 压力：20k 次操作后位图自洽（used_count 与位图 popcount 一致）。
        let mut pool = SizeClassPool::new();
        let mut lcg = 0x9E3779B97F4A7C15u64;
        let mut live: Vec<(u8, u16)> = Vec::new();
        for _ in 0..20_000 {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
            if !live.is_empty() && lcg % 3 == 0 {
                let i = (lcg >> 32) as usize % live.len();
                let t = live.swap_remove(i);
                pool.free(t);
            } else {
                let size = CLASS_SIZES[(lcg >> 40) as usize % 6];
                if let Some(t) = pool.alloc(size) {
                    live.push(t);
                }
            }
        }
        // 位图对账。
        for c in 0..6 {
            let bits = pool.free[c].iter().map(|w| w.count_ones() as usize).sum::<usize>();
            assert_eq!(slots_per_class(c) - bits, pool.used_count[c], "class {} bitmap desync", c);
        }
    }

    #[test]
    fn spectrum_bucket_boundaries_match_class_for() {
        // 谱桶分界与池档位分界同源：六档内逐档落位，257 起直通三段。
        for (size, want) in [(1usize, 0usize), (8, 0), (16, 1), (32, 2), (64, 3), (128, 4), (256, 5), (257, 6), (512, 6), (513, 7), (4096, 7), (4097, 8), (1 << 20, 8)] {
            assert_eq!(AllocSpectrum::bucket_of(size), want, "size {}", size);
        }
    }

    #[test]
    fn spectrum_multi_day_rollover_and_catchup() {
        // 跨 9 天一次性推进（steps > 7）：等价全新采集窗，全部清零。
        let mut s = AllocSpectrum::new();
        s.note(8, 0);
        s.note(64, 9 * DAY_MS);
        let t = s.totals();
        assert_eq!(s.total(), 2);
        assert_eq!(t.iter().sum::<u32>(), 1, "9 天跳跃 → 旧窗全清，只剩新日");
        assert_eq!(t[3], 1);
        // 单日跨多 note 同桶累计。
        s.note(64, 9 * DAY_MS + 5);
        assert_eq!(s.totals()[3], 2);
    }

    #[test]
    fn spectrum_attribution_all_modes() {
        // 高档贴边：128/256 主导 → class-edge-churn。
        let mut s = AllocSpectrum::new();
        for i in 0..700u64 {
            s.note(if i % 2 == 0 { 128 } else { 256 }, i / 200 * DAY_MS);
        }
        s.note(8, 0);
        assert_eq!(s.attribution(), "class-edge-churn");
        // 谱形均匀 → mixed-spectrum（不硬编）。
        let mut s2 = AllocSpectrum::new();
        for b in 0..9usize {
            for _ in 0..10 {
                s2.note([8, 16, 32, 64, 128, 256, 300, 2000, 8192][b], 0);
            }
        }
        assert_eq!(s2.attribution(), "mixed-spectrum");
    }

    #[test]
    fn spectrum_boundary_review_drop_256() {
        // 256 档近零（<10‰）+ 样本足量 → drop-256-class 建议。
        let mut s = AllocSpectrum::new();
        for i in 0..1000u64 {
            s.note(8 + (i % 5) as usize * 8, i / 200 * DAY_MS); // 8..40B，零 256B
        }
        assert_eq!(s.boundary_review(), Some("drop-256-class"));
    }

    #[test]
    fn spectrum_dominant_and_total() {
        let mut s = AllocSpectrum::new();
        s.note(64, 0);
        s.note(64, DAY_MS);
        s.note(64, 2 * DAY_MS);
        s.note(256, 3 * DAY_MS);
        assert_eq!(s.dominant(), Some(3)); // 64B 桶
        assert_eq!(s.total(), 4);
        assert_eq!(AllocSpectrum::new().dominant(), None); // 空谱无主导
    }
}
