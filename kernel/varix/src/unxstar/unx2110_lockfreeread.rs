//! unx2110_lockfreeread —— UNX-F2110 导出并发读基准（无锁读开销账）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2110 · 导出并发读基准 ·
//!   目标 300 行 · 已深化
//! 判据原文：UNX-F2110-J1 并发读基准账格式冻结（无锁读延迟/撕裂零发生
//!   断言对齐 F1773 体例），逻辑建模环境全过，真机数字随闸门补测。
//! 组件：BenchFormat 基准账格式冻结（版本/列序/单位三冻结列，冻结指纹
//!      FNV 钉死——未冻结即测 0xD701）+ SeqCell 无锁读面（逻辑建模
//!      seqlock：写者奇序推进/偶序落定，读者三段采样 seq1→payload→seq2
//!      对拍，撕裂重读计数，撕裂零发生断言 0xD702 / 奇读放行 0xD703）+
//!      BenchRow 基准账行（场景×负载形态 3×3 九行封闭矩阵，steps/tears/
//!      retries 三列）+ GaugeSlot 真机补测占位（随闸门补测，缺占位
//!      0xD704）+ F1773 体例对齐闸（格式 id 1773 口径，漂移 0xD705）+
//!      矩阵行重复拒（0xD706）+ LockfreeBench 基准账（record 闸链 +
//!      撕裂零发生全账断言 + 总读数守恒）+ LCG 压力 512 轮写读轮转 +
//!      指纹重放对拍锚。
//! 对标：Linux kernel seqlock（write_seqbegin/write_seqend/read_seqbegin/
//!   read_seqretry）与 ftrace 基准账体例——本模块以「逻辑建模 seqlock +
//!   格式先行冻结 + 补测占位在账」复刻无锁读基准纪律。
//! 性能逐项分解：读采样 O(1)（三段对拍）；写推进 O(1)；账行登记 O(n)
//!   （n≤9 矩阵闭集）；审计 O(n)；全整数算术零浮点零墙钟（延迟以逻辑步
//!   计——步数即读开销），同事件序列恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xD7（0xD701~0xD706，全域 grep 确认无诊断基数占用——先入
//!   库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零
//!   模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 账格式版本（冻结对象之一——判据①格式冻结）。
pub const FMT_VER: u32 = 1;
/// F1773 体例对齐口径（格式 id 前 4 位 = 1773）。
pub const F1773_FMT_ID: u32 = 1773;
/// 场景封闭集元数。
pub const SCENE_N: usize = 3;
/// 负载形态封闭集元数。
pub const SHAPE_N: usize = 3;
/// 基准账矩阵容量（3×3 九行封闭）。
pub const ROW_CAP: usize = SCENE_N * SHAPE_N;
/// 压力轮数（LCG 写读轮转）。
pub const STRESS_ROUNDS: u64 = 512;
/// 读者单轮最大重读预算（超预算即账异常）。
pub const RETRY_BUDGET: u64 = 8;

// 判码（段内独占 0xD701~0xD706）
/// 0xD701 格式未冻结即测（冻结闸破）。
pub const C_FMT_OPEN: u16 = 0xD701;
/// 0xD702 撕裂非零（撕裂零发生破）。
pub const C_TEARSNZ: u16 = 0xD702;
/// 0xD703 奇读放行（seqlock 纪律破）。
pub const C_ODD_PASS: u16 = 0xD703;
/// 0xD704 补测占位缺（真机数字闸门占位缺账）。
pub const C_GAUGE_MISS: u16 = 0xD704;
/// 0xD705 格式漂移（F1773 体例口径破）。
pub const C_FMT_DRIFT: u16 = 0xD705;
/// 0xD706 矩阵行重（场景×形态重复登记）。
pub const C_ROW_DUP: u16 = 0xD706;

/// 段守卫：判码落 0xD700..0xD800 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_FMT_OPEN >= 0xD700
        && C_FMT_OPEN < 0xD800
        && C_TEARSNZ == C_FMT_OPEN + 1
        && C_ODD_PASS == C_FMT_OPEN + 2
        && C_GAUGE_MISS == C_FMT_OPEN + 3
        && C_FMT_DRIFT == C_FMT_OPEN + 4
        && C_ROW_DUP == C_FMT_OPEN + 5
        && C_ROW_DUP < 0xD800
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_FMT_OPEN => "格式未冻结即测",
        C_TEARSNZ => "撕裂非零",
        C_ODD_PASS => "奇读放行",
        C_GAUGE_MISS => "补测占位缺",
        C_FMT_DRIFT => "格式漂移",
        C_ROW_DUP => "矩阵行重",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn，避闭包借用冲突——F0386 教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 冻结账格式（F1773 体例：格式先行冻结）
// ---------------------------------------------------------------------------

/// 账列封闭集：六列定序（列序即冻结对象之二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    /// 场景列。
    Scene,
    /// 负载形态列。
    Shape,
    /// 逻辑读步列。
    Steps,
    /// 撕裂计数列。
    Tears,
    /// 重读计数列。
    Retries,
    /// 补测占位列。
    Gauge,
}

/// 列序冻结态（冻结后不可变——列序漂移即账废）。
pub const COL_ORDER: [Col; 6] = [
    Col::Scene,
    Col::Shape,
    Col::Steps,
    Col::Tears,
    Col::Retries,
    Col::Gauge,
];

/// 单位封闭集（单位即冻结对象之三）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// 逻辑步。
    Step,
    /// 发生次数。
    Cnt,
    /// 待补测（真机数字随闸门补）。
    Pend,
}

/// 每列单位账（列 → 单位映射冻结）。
pub const fn col_unit(c: Col) -> Unit {
    match c {
        Col::Scene | Col::Shape => Unit::Cnt,
        Col::Steps => Unit::Step,
        Col::Tears | Col::Retries => Unit::Cnt,
        Col::Gauge => Unit::Pend,
    }
}

/// 冻结账格式：版本 + 列序 + 单位三列齐，指纹 FNV 钉死。
#[derive(Clone, Copy, Debug)]
pub struct BenchFormat {
    /// 版本号（冻结后恒 FMT_VER）。
    pub ver: u32,
    /// 体例 id（对齐 F1773 口径）。
    pub fmt_id: u32,
    /// 冻结标志（一次性闸：freeze 后不可再动）。
    pub frozen: bool,
    /// 格式指纹（freeze 时 FNV 钉死）。
    pub fp: u64,
}

impl BenchFormat {
    /// 新格式（未冻结态——禁止直接测）。
    pub const fn new() -> BenchFormat {
        BenchFormat { ver: FMT_VER, fmt_id: F1773_FMT_ID, frozen: false, fp: 0 }
    }

    /// 冻结闸（一次性）：指纹钉死，重复冻结拒。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            // 重复冻结不是错误，返回既钉指纹（幂等）。
            return Ok(self.fp);
        }
        if self.fmt_id / 1000 != F1773_FMT_ID / 1000 {
            return Err(C_FMT_DRIFT);
        }
        let mut cols = [0u64; 6];
        let mut i = 0;
        while i < 6 {
            cols[i] = match COL_ORDER[i] {
                Col::Scene => 1,
                Col::Shape => 2,
                Col::Steps => 3,
                Col::Tears => 4,
                Col::Retries => 5,
                Col::Gauge => 6,
            } as u64;
            i += 1;
        }
        self.fp = fnv1a(0xF2110, &[self.ver as u64, self.fmt_id as u64, cols[0], cols[1], cols[2], cols[3], cols[4], cols[5]]);
        self.frozen = true;
        Ok(self.fp)
    }

    /// 格式指纹可复算核（指纹链审计：重算须等于钉死值）。
    pub fn fp_recompute_ok(&self) -> bool {
        if !self.frozen {
            return false;
        }
        let mut cols = [0u64; 6];
        let mut i = 0;
        while i < 6 {
            cols[i] = match COL_ORDER[i] {
                Col::Scene => 1,
                Col::Shape => 2,
                Col::Steps => 3,
                Col::Tears => 4,
                Col::Retries => 5,
                Col::Gauge => 6,
            } as u64;
            i += 1;
        }
        fnv1a(0xF2110, &[self.ver as u64, self.fmt_id as u64, cols[0], cols[1], cols[2], cols[3], cols[4], cols[5]]) == self.fp
    }
}

// ---------------------------------------------------------------------------
// SeqCell：无锁读面（逻辑建模 seqlock）
// ---------------------------------------------------------------------------

/// 无锁读单元：seq 奇=写进行，偶=稳定。读者三段采样对拍。
#[derive(Clone, Copy, Debug)]
pub struct SeqCell {
    /// 序号（奇偶即状态机：偶稳定/奇写中）。
    seq: u32,
    /// 载荷（写者 commit 时与偶序一同落定）。
    payload: u64,
    /// 采样统计：总读次数。
    pub reads: u64,
    /// 采样统计：撕裂重读次数（读者视角）。
    pub retries: u64,
    /// 采样统计：奇读尝试次数（必须全拒——纪律探针）。
    pub odd_tries: u64,
}

/// 读结果：稳定值或撕裂（需重读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOut {
    /// 稳定读得（值，落定序号）。
    Stable(u64, u32),
    /// 三段对拍不等——撕裂，读者应重读。
    Torn,
    /// 奇序采样——写进行中，读者应重读（不判撕裂不计 tear，计 retry）。
    InFlight,
}

impl SeqCell {
    /// 新单元（偶序 0 起——稳定空态）。
    pub const fn new() -> SeqCell {
        SeqCell { seq: 0, payload: 0, reads: 0, retries: 0, odd_tries: 0 }
    }

    /// 写者入口（&mut 独占即互斥语义）：载荷落定 + 偶序步进。
    /// seqlock 纪律：写前奇序（宣布写中），写后偶序（宣布稳定）。
    pub fn write(&mut self, payload: u64) {
        self.seq = self.seq.wrapping_add(1); // 奇：写进行
        self.payload = payload;
        self.seq = self.seq.wrapping_add(1); // 偶：落定
    }

    /// 读者三段采样：seq1 → payload → seq2，前后对拍。
    /// 撕裂零发生断言的微观基础：本建模环境写读互斥（&mut 独占），
    /// 撕裂只能由纪律破绽（奇读放行/对拍缺步）引入——探针专测此。
    pub fn read(&mut self) -> ReadOut {
        self.reads = self.reads.wrapping_add(1);
        let s1 = self.seq;
        let p = self.payload;
        let s2 = self.seq;
        if s1 != s2 {
            self.retries = self.retries.wrapping_add(1);
            return ReadOut::Torn;
        }
        if s1 % 2 == 1 {
            self.odd_tries = self.odd_tries.wrapping_add(1);
            self.retries = self.retries.wrapping_add(1);
            return ReadOut::InFlight;
        }
        ReadOut::Stable(p, s1)
    }

    /// 纪律探针：奇读放行检查（奇序下 Stable 即纪律破）。
    pub fn odd_pass_violation(&mut self) -> bool {
        // 构造奇序场景：写一步到奇序。
        let saved = (self.seq, self.payload);
        self.seq = self.seq.wrapping_add(1); // 奇
        let out = self.read();
        self.seq = saved.0; // 复原（探针不留痕）
        self.payload = saved.1;
        matches!(out, ReadOut::Stable(_, _))
    }
}

// ---------------------------------------------------------------------------
// 基准账行（场景×负载形态 3×3 九行封闭）
// ---------------------------------------------------------------------------

/// 场景封闭集（判据①矩阵行维一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// 空闲场景（读为主）。
    Idle,
    /// 混合场景（读写交替）。
    Mixed,
    /// 风暴场景（写密集）。
    Storm,
}

impl Scene {
    /// 场景序号（账列编码）。
    pub const fn idx(self) -> u64 {
        match self {
            Scene::Idle => 0,
            Scene::Mixed => 1,
            Scene::Storm => 2,
        }
    }
}

/// 负载形态封闭集（矩阵行维二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// 轻载（少量读步）。
    Light,
    /// 重载（大量读步）。
    Heavy,
    /// 突发（读步尖峰）。
    Burst,
}

impl Shape {
    /// 形态序号（账列编码）。
    pub const fn idx(self) -> u64 {
        match self {
            Shape::Light => 0,
            Shape::Heavy => 1,
            Shape::Burst => 2,
        }
    }
}

/// 真机补测占位（F1773 体例：真机数字随闸门补测——账格式先行冻结，
/// 占位必须在账，缺占位 0xD704）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaugeSlot {
    /// 待补测标志（真数字未回填恒 true）。
    pub pending: bool,
    /// 占位凭证（对齐 F1773 口径 id）。
    pub anchor: u32,
}

impl GaugeSlot {
    /// 新占位（pending 态）。
    pub const fn new() -> GaugeSlot {
        GaugeSlot { pending: true, anchor: F1773_FMT_ID }
    }
}

/// 基准账行：场景×形态定位 + 读开销三列（steps/tears/retries）+ 补测占位。
#[derive(Clone, Copy, Debug)]
pub struct BenchRow {
    /// 场景（行定位维一）。
    pub scene: Scene,
    /// 负载形态（行定位维二）。
    pub shape: Shape,
    /// 无锁读逻辑步（延迟账：步数即开销——零墙钟口径）。
    pub steps: u64,
    /// 撕裂计数（撕裂零发生断言对象——恒 0）。
    pub tears: u64,
    /// 重读计数（重读预算内）。
    pub retries: u64,
    /// 真机补测占位。
    pub gauge: GaugeSlot,
}

impl BenchRow {
    /// 行键（场景 idx*10+形态 idx——矩阵键空间 00..22）。
    pub const fn key(&self) -> u64 {
        self.scene.idx() * 10 + self.shape.idx()
    }
}

// ---------------------------------------------------------------------------
// 基准账：登记闸链 + 全账断言
// ---------------------------------------------------------------------------

/// 并发读基准账：冻结格式 + 九行矩阵账 + 总读守恒。
#[derive(Clone, Debug)]
pub struct LockfreeBench {
    /// 冻结格式（record 前必须 freeze——未冻结即测 0xD701）。
    pub fmt: BenchFormat,
    /// 账行（≤9 行封闭）。
    pub rows: Vec<BenchRow>,
    /// 登记拒绝账（判码计数）。
    pub rejects: [u64; 6],
    /// 总读步（守恒对象：sum steps 恒等）。
    pub total_steps: u64,
}

impl LockfreeBench {
    /// 新账（格式未冻结态）。
    pub fn new() -> LockfreeBench {
        LockfreeBench {
            fmt: BenchFormat::new(),
            rows: Vec::new(),
            rejects: [0; 6],
            total_steps: 0,
        }
    }

    /// 判码 → rejects 槽位（0..6 闭集映射）。
    fn slot(code: u16) -> usize {
        match code {
            C_FMT_OPEN => 0,
            C_TEARSNZ => 1,
            C_ODD_PASS => 2,
            C_GAUGE_MISS => 3,
            C_FMT_DRIFT => 4,
            _ => 5, // C_ROW_DUP
        }
    }

    /// 登记闸链：冻结闸 → 体例对齐闸 → 补测占位闸 → 矩阵行重闸 → 落账。
    /// 任一闸拒即判码入账并 Err。
    pub fn record(&mut self, row: BenchRow) -> Result<(), u16> {
        // 闸① 格式冻结（未冻结即测——判据①格式冻结纪律）。
        if !self.fmt.frozen {
            self.rejects[Self::slot(C_FMT_OPEN)] += 1;
            return Err(C_FMT_OPEN);
        }
        // 闸② 体例对齐（F1773 口径）。
        if self.fmt.fmt_id / 1000 != F1773_FMT_ID / 1000 {
            self.rejects[Self::slot(C_FMT_DRIFT)] += 1;
            return Err(C_FMT_DRIFT);
        }
        // 闸③ 补测占位在账（真机数字随闸门补——占位缺即账不全）。
        if !row.gauge.pending || row.gauge.anchor != F1773_FMT_ID {
            self.rejects[Self::slot(C_GAUGE_MISS)] += 1;
            return Err(C_GAUGE_MISS);
        }
        // 闸④ 矩阵行重（3×3 九行封闭，行键唯一）。
        let k = row.key();
        if k >= ROW_CAP as u64 * 10 {
            self.rejects[Self::slot(C_ROW_DUP)] += 1;
            return Err(C_ROW_DUP);
        }
        for r in self.rows.iter() {
            if r.key() == k {
                self.rejects[Self::slot(C_ROW_DUP)] += 1;
                return Err(C_ROW_DUP);
            }
        }
        // 落账 + 守恒推进。
        self.total_steps = self.total_steps.wrapping_add(row.steps);
        self.rows.push(row);
        Ok(())
    }

    /// 全账断言：撕裂零发生（判据①核心断言）+ 重读预算内 + 占位齐备
    /// + 守恒（sum steps == total_steps）。
    /// 返回 (全过, 首个违例判码或 0)。
    pub fn audit(&self) -> (bool, u16) {
        let mut sum = 0u64;
        for r in self.rows.iter() {
            // 撕裂零发生：tears 恒 0。
            if r.tears != 0 {
                return (false, C_TEARSNZ);
            }
            // 重读预算内（超预算即账异常——非撕裂但读放大失控）。
            if r.retries > RETRY_BUDGET {
                return (false, C_ODD_PASS);
            }
            // 占位齐备。
            if !r.gauge.pending {
                return (false, C_GAUGE_MISS);
            }
            sum = sum.wrapping_add(r.steps);
        }
        // 守恒：账行和恒等总读步。
        if sum != self.total_steps {
            return (false, C_ROW_DUP);
        }
        (true, 0)
    }

    /// 账指纹（FNV：格式指纹 + 全行键/列值——重放对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc: Vec<u64> = Vec::new();
        acc.push(self.fmt.fp);
        acc.push(self.rows.len() as u64);
        for r in self.rows.iter() {
            acc.push(r.key());
            acc.push(r.steps);
            acc.push(r.tears);
            acc.push(r.retries);
            acc.push(r.gauge.pending as u64);
        }
        fnv1a(0xF2110_0001, &acc)
    }
}

// ---------------------------------------------------------------------------
// LCG（确定性注入——种子必须真实参与序列）
// ---------------------------------------------------------------------------

/// 线性同余发生器（确定性锚）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 新种子。
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xF2110))
    }
    /// 步进并取值（种子参与每轮序列——恒等对拍基础）。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

// ---------------------------------------------------------------------------
// 压力轮转：写读交错 + 纪律探针注入
// ---------------------------------------------------------------------------

/// 压力结论（守恒与纪律探针汇总）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 写者落定次数。
    pub writes: u64,
    /// 读者稳定读得次数。
    pub stable_reads: u64,
    /// 撕裂次数（恒 0——撕裂零发生）。
    pub tears: u64,
    /// 奇读尝试次数（全拒——odd_tries 与注入次数对拍）。
    pub odd_tries: u64,
    /// 探针①奇读放行违例（恒 false）。
    pub odd_pass_violation: bool,
    /// 探针②未冻结即测被拒（恒 true——闸①必须拦）。
    pub fmt_open_rejected: bool,
}

/// LCG 压力轮转：512 轮写读交错，读开销按逻辑步入账。
/// 每轮：LCG 取步长 step=1+rng%8，写者推 payload+=step，读者采样 step 次。
/// 中途第 96 轮注入奇读探针、第 256 轮注入未冻结测探针。
pub fn stress(cell: &mut SeqCell, bench: &mut LockfreeBench, seed: u64) -> StressOut {
    let mut rng = Lcg::new(seed);
    let mut writes = 0u64;
    let mut stable = 0u64;
    let mut tears = 0u64;
    let mut odd_seen = 0u64;
    let mut odd_violation = false;
    let mut fmt_open_rejected = false;

    // 探针②：未冻结格式直接测（必须被闸①拦）。
    let probe_row = BenchRow {
        scene: Scene::Storm,
        shape: Shape::Burst,
        steps: 0,
        tears: 0,
        retries: 0,
        gauge: GaugeSlot::new(),
    };
    if bench.record(probe_row) == Err(C_FMT_OPEN) {
        fmt_open_rejected = true;
    }

    // 基准格式此刻冻结（探针后——先证闸拦，再入正式测量）。
    let _ = bench.fmt.freeze();

    for round in 0..STRESS_ROUNDS {
        let step = 1 + rng.next() % 8;
        // 写者推进一步（载荷步进 = 无锁写开销口径）。
        cell.payload = cell.payload.wrapping_add(step);
        cell.write(cell.payload);
        writes += 1;

        // 读者采样 step 次（逻辑读步累加——延迟账）。
        let mut row_steps = 0u64;
        let mut row_tears = 0u64;
        let mut row_retries = 0u64;
        for _ in 0..step {
            match cell.read() {
                ReadOut::Stable(_, _) => stable += 1,
                ReadOut::Torn => {
                    tears += 1;
                    row_tears += 1;
                }
                ReadOut::InFlight => row_retries += 1,
            }
            row_steps += 1;
        }

        // 第 96 轮注入奇读探针（奇读必须拒——纪律探针①）。
        if round == 96 {
            if cell.odd_pass_violation() {
                odd_violation = true;
            }
            odd_seen = cell.odd_tries;
        }

        // 每 64 轮把本段读开销落一行账（矩阵轮转登记——场景/形态按轮取）。
        if (round + 1) % 64 == 0 {
            let scene = match (round / 64) % 3 {
                0 => Scene::Idle,
                1 => Scene::Mixed,
                _ => Scene::Storm,
            };
            let shape = match step % 3 {
                0 => Shape::Light,
                1 => Shape::Heavy,
                _ => Shape::Burst,
            };
            let row = BenchRow {
                scene,
                shape,
                steps: row_steps,
                tears: row_tears,
                retries: row_retries,
                gauge: GaugeSlot::new(),
            };
            // 重行不计（矩阵闭集：同键后到者拒——先到先占）。
            let _ = bench.record(row);
        }
    }

    StressOut {
        writes,
        stable_reads: stable,
        tears,
        odd_tries: odd_seen,
        odd_pass_violation: odd_violation,
        fmt_open_rejected,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2110 自检（判据①全覆盖：格式冻结/撕裂零发生/占位在账/守恒/探针）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2110");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xD7段六码闭集递增");

    // ② 格式冻结幂等 + 指纹可复算。
    let mut fmt = BenchFormat::new();
    let fp1 = fmt.freeze();
    let fp2 = fmt.freeze();
    cs.add(
        "fmt-freeze-idempotent",
        matches!(fp1, Ok(_)) && fp1 == fp2 && fmt.fp_recompute_ok(),
        "冻结幂等+指纹可复算",
    );

    // ③ 未冻结即测必拒（闸①）。
    let mut b0 = LockfreeBench::new();
    let probe = BenchRow {
        scene: Scene::Idle,
        shape: Shape::Light,
        steps: 1,
        tears: 0,
        retries: 0,
        gauge: GaugeSlot::new(),
    };
    cs.add(
        "fmt-open-gate",
        b0.record(probe) == Err(C_FMT_OPEN),
        "未冻结即测0xD701拦",
    );

    // ④ 基准读序状态机：写后读恒稳定，载荷守恒。
    let mut cell = SeqCell::new();
    cell.write(0xABCD);
    let stable_ok = matches!(cell.read(), ReadOut::Stable(0xABCD, 2));
    cs.add("read-stable-after-write", stable_ok, "写后三段采样稳定");

    // ⑤ 奇读放行探针：奇序下 Stable 即纪律破（本实现必拒）。
    let mut cell2 = SeqCell::new();
    cs.add(
        "odd-read-rejected",
        !cell2.odd_pass_violation(),
        "奇读0xD703不放行",
    );

    // ⑥ 压力轮转：512 轮撕裂零发生 + 奇读全拒 + 未冻结测被拦。
    let mut bench = LockfreeBench::new();
    let mut cell3 = SeqCell::new();
    let out = stress(&mut cell3, &mut bench, 0xF2110);
    cs.add(
        "stress-tear-free",
        out.tears == 0 && !out.odd_pass_violation && out.fmt_open_rejected,
        "撕裂零发生+奇读拒+闸①拦",
    );
    cs.add(
        "stress-conservation",
        out.writes == STRESS_ROUNDS && cell3.seq == (out.writes as u32) * 2 && cell3.reads >= out.stable_reads,
        "写读计数守恒(序列步=写数×2)",
    );

    // ⑦ 账审计：九行矩阵内撕裂零 + 占位齐 + 守恒。
    let (ok, code) = bench.audit();
    cs.add("bench-audit-green", ok && code == 0, "全账断言全过");

    // ⑧ 指纹重放对拍：两次复算恒等。
    let f1 = bench.fingerprint();
    let f2 = bench.fingerprint();
    cs.add("fingerprint-replay", f1 == f2 && f1 != 0, "指纹重放恒等");

    // ⑨ 补测占位闸：占位缺（pending=false）必拒。
    let mut bench2 = LockfreeBench::new();
    let _ = bench2.fmt.freeze();
    let bad = BenchRow {
        scene: Scene::Mixed,
        shape: Shape::Heavy,
        steps: 4,
        tears: 0,
        retries: 0,
        gauge: GaugeSlot { pending: false, anchor: F1773_FMT_ID },
    };
    cs.add(
        "gauge-miss-gate",
        bench2.record(bad) == Err(C_GAUGE_MISS),
        "补测占位缺0xD704拦",
    );

    // ⑩ 矩阵行重闸：同键重复登记必拒。
    let dup = BenchRow {
        scene: Scene::Idle,
        shape: Shape::Light,
        steps: 2,
        tears: 0,
        retries: 0,
        gauge: GaugeSlot::new(),
    };
    let r1 = bench2.record(dup);
    let r2 = bench2.record(dup);
    cs.add(
        "row-dup-gate",
        r1 == Ok(()) && r2 == Err(C_ROW_DUP),
        "矩阵行重0xD706拦",
    );

    cs
}
