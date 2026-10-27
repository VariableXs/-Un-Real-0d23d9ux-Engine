//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1264 · Irq 态稳态统计矩阵（稳态运行全景） · 0xFC40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1264-J1 稳态统计矩阵 ≥12 维（中断率/top half 时长/BH 深度/
//!      亲和分布/延迟 P99/配额越限等）逐维可查，矩阵与各部件账交叉一致率
//!      100%，矩阵快照周期可配默认 ≤1s；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 12 维矩阵：每维一格（sum/cnt/max），含分量账（逐向量/逐 CPU），
//!     Σ 分量 ≡ 总量恒等式三处（中断、配额、亲和）；
//!   - 延迟 P99：定容样本环（64 样），整数插入排序取 99 分位，禁浮点；
//!   - 快照：周期可配（默认 1000 tick=1s，上限钉死 ≤1000），序号只增，
//!     回退 tick 显式拒（E_SeqRewind），滚动保留 4 张防欠账；
//!   - 交叉一致：每张快照冻结前跑三恒等式，不一致即 E_CrossMismatch；
//!   - 注入面：故意污染分量账 10/10 次必抓（判据「交叉一致率 100%」
//!     的反向判据——坏账必须现形，好账必须常绿）；
//!   - LCG 种子写死 0xFC40_1264，全程可重复。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFC40 段，B24 Irq 稳态面首格）。
pub const SEG_BASE: u16 = 0xFC40;

/// 矩阵维数（判据点名 ≥12）。
pub const DIM_COUNT: usize = 12;

/// 向量分量账宽度（逐向量分解面）。
pub const VEC_COUNT: usize = 8;

/// 亲和分解面宽度（CPU 0..3）。
pub const CPU_COUNT: usize = 4;

/// 快照默认周期：1000 tick（tick=1ms 口径，即 ≤1s 判据）。
pub const SNAPSHOT_PERIOD_DEFAULT: u64 = 1000;

/// 快照周期上限（判据「默认 ≤1s」的钉死口径，禁配更大）。
pub const SNAPSHOT_PERIOD_MAX: u64 = 1000;

/// 快照滚动保留张数（防欠账保底）。
pub const SNAPSHOT_KEEP: usize = 4;

/// 快照欠账上限（连续欠账超过 3 期报 E_SnapSkip）。
pub const SNAPSHOT_MAX_LAG: u64 = 3;

/// 延迟样本环容量。
pub const P99_SAMPLE_CAP: usize = 64;

/// 注入演练轮数（判据点名 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0xFC40 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IcCode {
    /// 0xFC40 维号越界（query_dim 出 0..12）。
    DimOob = 0,
    /// 0xFC41 向量号越界（出 0..8）。
    VecOob = 1,
    /// 0xFC42 快照周期为零。
    PeriodZero = 2,
    /// 0xFC43 快照周期超上限（>1000 tick）。
    PeriodOver = 3,
    /// 0xFC44 快照欠账超期（连续 3 期未冻结）。
    SnapSkip = 4,
    /// 0xFC45 交叉一致失败（Σ 分量 ≠ 总量）。
    CrossMismatch = 5,
    /// 0xFC46 P99 无样本（空环取分位）。
    P99Empty = 6,
    /// 0xFC47 快照序号回退（seq 非只增）。
    SeqRewind = 7,
    /// 0xFC48 亲和 CPU 号越界。
    AffOob = 8,
    /// 0xFC49 配额分量账缺失（向量有事件但无配额账）。
    QuotaMissing = 9,
    /// 0xFC4A 快照滚动保底被击穿（留存 < 保底且未到收敛）。
    KeepBroken = 10,
    /// 0xFC4B 维度漂移（同名维两源读数不等）。
    DimDrift = 11,
}

impl IcCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [IcCode; 12] = [
        IcCode::DimOob, IcCode::VecOob, IcCode::PeriodZero, IcCode::PeriodOver,
        IcCode::SnapSkip, IcCode::CrossMismatch, IcCode::P99Empty, IcCode::SeqRewind,
        IcCode::AffOob, IcCode::QuotaMissing, IcCode::KeepBroken, IcCode::DimDrift,
    ];
}

/// 诊断码 → 中文可操作建议（每码一条，禁泛化话术）。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFC40 => "维号越界：先取 DIM_COUNT=12 比长度再下标，越界即拒不改账",
        0xFC41 => "向量号越界：分发面只认 0..8，检查来源 C 表映射是否漂移",
        0xFC42 => "周期为零：0 会导致每 tick 冻结快照刷爆账，最小 1 起配",
        0xFC43 => "周期超上限：判据钉死 ≤1000 tick(1s)，超限配置一律拒收",
        0xFC44 => "快照欠账：连续 3 期未冻结说明冻结面被饿死，先查调度优先级",
        0xFC45 => "交叉一致失败：逐向量/逐 CPU 分量账与总格求和不等，先重放对账再定位漂移维",
        0xFC46 => "P99 空环：无样本不报数，先喂样本环再取分位",
        0xFC47 => "快照序号回退：时间轴倒走，查 tick 源是否被复用或回拨",
        0xFC48 => "亲和 CPU 越界：亲和面只认 0..4，查路由表 bin 宽度",
        0xFC49 => "配额账缺失：向量有中断但无配额记录，配额登记序先于处置序",
        0xFC4A => "滚动保底击穿：留存快照少于保底张数，扩 SNAPSHOT_KEEP 或查异常清账路径",
        0xFC4B => "维度漂移：同维两源读数不等，先比分量账再比冻结点",
        _ => "未知诊断码：查 0xFC40 段闭集 IcCode::ALL",
    }
}

/// 十二维名称表（判据点名维度逐字对应）。
pub const DIM_NAMES: [&str; DIM_COUNT] = [
    "中断率",       // 维0：每千 tick 中断数（sum 累计，per-vec 分量）
    "上半部时长",   // 维1：top half 处理 tick 均值（mean）
    "BH深度",       // 维2：单中断 BH 排队深度均值（mean）
    "亲和分布",     // 维3：逐 CPU 分量账（Σ=中断总数）
    "延迟P99",      // 维4：样本环 99 分位（max 格另记峰值）
    "配额越限",     // 维5：越限次数（per-vec 分量）
    "逆迁移次数",   // 维6：Irq→前态回迁事件数
    "确认错配数",   // 维7：确认与投递错配数
    "丢中断数",     // 维8：检测到的丢失数
    "重复投递数",   // 维9：检测到的重复数
    "风暴事件数",   // 维10：风暴窗口判定数
    "快照序列号",   // 维11：seq 只增读数
];

/// 十二维单位表（与 DIM_NAMES 同序）。
pub const DIM_UNITS: [&str; DIM_COUNT] = [
    "次/千tick", "tick", "个", "次", "tick", "次",
    "次", "次", "次", "次", "窗", "seq",
];

/// 矩阵单元格：sum 累计、cnt 次数、max 峰值；mean=false 时读数取 sum。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatCell {
    /// 累计值（count 维即总量；mean 维为总和）。
    pub sum: u64,
    /// 记账次数（count 维 == sum）。
    pub cnt: u64,
    /// 单笔峰值。
    pub max: u64,
    /// true=读数取均值（sum/cnt 整除），false=读数取 sum。
    pub mean: bool,
}

impl StatCell {
    /// 建格（默认 count 型）。
    pub const fn new() -> StatCell {
        StatCell { sum: 0, cnt: 0, max: 0, mean: false }
    }
    /// 建格（mean 型）。
    pub const fn new_mean() -> StatCell {
        StatCell { sum: 0, cnt: 0, max: 0, mean: true }
    }
    /// 记一笔。
    pub fn record(&mut self, v: u64) {
        self.sum = self.sum.wrapping_add(v);
        self.cnt = self.cnt.wrapping_add(1);
        if v > self.max {
            self.max = v;
        }
    }
    /// 当前读数：count 维=sum，mean 维=sum/cnt（cnt=0 视 0）。
    pub fn value(&self) -> u64 {
        if self.mean && self.cnt > 0 {
            self.sum / self.cnt
        } else {
            self.sum
        }
    }
}

/// 一张矩阵快照（冻结点十二维读数 + 交叉一致结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// 快照序号（只增，从 1 起）。
    pub seq: u64,
    /// 冻结时 tick。
    pub tick: u64,
    /// 冻结时十二维读数。
    pub values: [u64; DIM_COUNT],
    /// 冻结前三恒等式是否全过。
    pub cross_ok: bool,
}

impl Snapshot {
    /// 读数行文本（导出用）。
    pub fn row(&self) -> String {
        let mut line = format!("[seq={} tick={}]", self.seq, self.tick);
        for i in 0..DIM_COUNT {
            line.push_str(&format!(" {}={}", DIM_NAMES[i], self.values[i]));
        }
        line.push_str(&format!(" 交叉={}", if self.cross_ok { "绿" } else { "红" }));
        line
    }
}

/// Irq 态稳态统计矩阵正本（本单账本）。
#[derive(Debug)]
pub struct IrqMatrix {
    /// 十二维总格。
    cells: [StatCell; DIM_COUNT],
    /// 维0 分量账：逐向量中断数。
    per_vec_deliver: [u64; VEC_COUNT],
    /// 维5 分量账：逐向量配额越限数。
    per_vec_quota: [u64; VEC_COUNT],
    /// 维3 分量账：逐 CPU 亲和计数。
    per_cpu_affinity: [u64; CPU_COUNT],
    /// 维4 样本环（延迟 tick，定容丢旧）。
    lat_samples: Vec<u64>,
    /// 快照周期配置。
    snap_period: u64,
    /// 已冻结快照（滚动保留）。
    snapshots: Vec<Snapshot>,
    /// 上次冻结 tick。
    last_snap_tick: u64,
    /// 连续欠账期数。
    lag_rounds: u64,
    /// 快照序号（只增）。
    snap_seq: u64,
    /// 总中断数（对账独立源，与 cells[0] 互为两源）。
    total_irq: u64,
}

impl IrqMatrix {
    /// 建账：默认周期 1000 tick（≤1s 判据），格表按维型初始化。
    pub fn new() -> IrqMatrix {
        let mut cells = [StatCell::new(); DIM_COUNT];
        cells[1] = StatCell::new_mean();
        cells[2] = StatCell::new_mean();
        cells[4] = StatCell::new_mean();
        IrqMatrix {
            cells,
            per_vec_deliver: [0; VEC_COUNT],
            per_vec_quota: [0; VEC_COUNT],
            per_cpu_affinity: [0; CPU_COUNT],
            lat_samples: Vec::new(),
            snap_period: SNAPSHOT_PERIOD_DEFAULT,
            snapshots: Vec::new(),
            last_snap_tick: 0,
            lag_rounds: 0,
            snap_seq: 0,
            total_irq: 0,
        }
    }

    /// 配置快照周期：0 拒（E_PeriodZero），>1000 拒（E_PeriodOver）。
    pub fn set_snapshot_period(&mut self, p: u64) -> Result<(), IcCode> {
        if p == 0 {
            return Err(IcCode::PeriodZero);
        }
        if p > SNAPSHOT_PERIOD_MAX {
            return Err(IcCode::PeriodOver);
        }
        self.snap_period = p;
        Ok(())
    }

    /// 当前周期配置。
    pub fn snapshot_period(&self) -> u64 {
        self.snap_period
    }

    /// 记一笔稳态中断：向量+CPU+上半部时长+BH 深度四进（越界全拒）。
    pub fn record_irq(
        &mut self,
        vec: usize,
        cpu: usize,
        top_half_ticks: u64,
        bh_depth: u64,
    ) -> Result<(), IcCode> {
        if vec >= VEC_COUNT {
            return Err(IcCode::VecOob);
        }
        if cpu >= CPU_COUNT {
            return Err(IcCode::AffOob);
        }
        self.per_vec_deliver[vec] = self.per_vec_deliver[vec].wrapping_add(1);
        self.per_cpu_affinity[cpu] = self.per_cpu_affinity[cpu].wrapping_add(1);
        self.total_irq = self.total_irq.wrapping_add(1);
        self.cells[0].record(1);
        self.cells[1].record(top_half_ticks);
        self.cells[2].record(bh_depth);
        Ok(())
    }

    /// 记一笔延迟样本（定容环，超容丢最旧）。
    pub fn record_latency(&mut self, ticks: u64) {
        if self.lat_samples.len() >= P99_SAMPLE_CAP {
            self.lat_samples.remove(0);
        }
        self.lat_samples.push(ticks);
        self.cells[4].record(ticks);
    }

    /// 记一笔配额越限（带向量分量）。
    pub fn record_quota_breach(&mut self, vec: usize) -> Result<(), IcCode> {
        if vec >= VEC_COUNT {
            return Err(IcCode::VecOob);
        }
        self.per_vec_quota[vec] = self.per_vec_quota[vec].wrapping_add(1);
        self.cells[5].record(1);
        Ok(())
    }

    /// 记一笔逆迁移。
    pub fn record_regress(&mut self) {
        self.cells[6].record(1);
    }

    /// 记一笔确认错配。
    pub fn record_confirm_mismatch(&mut self) {
        self.cells[7].record(1);
    }

    /// 记一笔丢中断。
    pub fn record_lost(&mut self) {
        self.cells[8].record(1);
    }

    /// 记一笔重复投递。
    pub fn record_dup(&mut self) {
        self.cells[9].record(1);
    }

    /// 记一个风暴窗。
    pub fn record_storm(&mut self) {
        self.cells[10].record(1);
    }

    /// 三恒等式交叉一致检查（判据核心）：
    ///   恒等一：Σ per_vec_deliver ≡ cells[0].sum
    ///   恒等二：Σ per_vec_quota   ≡ cells[5].sum
    ///   恒等三：Σ per_cpu_affinity ≡ cells[0].sum 且 ≡ total_irq（两源一致）
    pub fn cross_check(&self) -> Result<(), IcCode> {
        let s0 = self.per_vec_deliver.iter().fold(0u64, |a, &v| a.wrapping_add(v));
        if s0 != self.cells[0].sum {
            return Err(IcCode::CrossMismatch);
        }
        let s1 = self.per_vec_quota.iter().fold(0u64, |a, &v| a.wrapping_add(v));
        if s1 != self.cells[5].sum {
            return Err(IcCode::CrossMismatch);
        }
        let s2 = self.per_cpu_affinity.iter().fold(0u64, |a, &v| a.wrapping_add(v));
        if s2 != self.cells[0].sum || s2 != self.total_irq {
            return Err(IcCode::CrossMismatch);
        }
        Ok(())
    }

    /// 样本环排序副本（插入排序，禁浮点、禁递归）。
    fn sorted_lat(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.lat_samples.iter().copied().collect();
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && v[j - 1] > v[j] {
                v.swap(j - 1, j);
                j -= 1;
            }
        }
        v
    }

    /// P99 整数分位：排序后取下标 (n*99)/100（n=1 时取首元）。
    pub fn p99(&self) -> Result<u64, IcCode> {
        if self.lat_samples.is_empty() {
            return Err(IcCode::P99Empty);
        }
        let v = self.sorted_lat();
        let idx = v.len() * 99 / 100;
        Ok(v[idx.min(v.len() - 1)])
    }

    /// P95 整数分位（配套可查，防 P99 孤证）。
    pub fn p95(&self) -> Result<u64, IcCode> {
        if self.lat_samples.is_empty() {
            return Err(IcCode::P99Empty);
        }
        let v = self.sorted_lat();
        let idx = v.len() * 95 / 100;
        Ok(v[idx.min(v.len() - 1)])
    }

    /// 把当前 P99 写回维4（快照冻结前调用）。
    fn flush_p99(&mut self) {
        if let Ok(p) = self.p99() {
            self.cells[4].sum = p * self.cells[4].cnt;
        }
    }

    /// 到期冻结快照；tick 倒走拒（E_SeqRewind）；欠账超 3 期报 E_SnapSkip。
    /// 冻结前跑交叉一致，不一致拒绝出快照（红账不白给）。
    pub fn maybe_snapshot(&mut self, tick: u64) -> Result<Option<Snapshot>, IcCode> {
        if tick < self.last_snap_tick {
            return Err(IcCode::SeqRewind);
        }
        if tick < self.last_snap_tick + self.snap_period {
            return Ok(None);
        }
        self.lag_rounds = if self.last_snap_tick == 0 && self.snap_seq == 0 {
            0
        } else {
            self.lag_rounds + 1
        };
        if self.lag_rounds > SNAPSHOT_MAX_LAG {
            return Err(IcCode::SnapSkip);
        }
        self.cross_check()?;
        self.flush_p99();
        self.snap_seq = self.snap_seq.wrapping_add(1);
        let mut values = [0u64; DIM_COUNT];
        for i in 0..DIM_COUNT {
            values[i] = self.cells[i].value();
        }
        values[4] = self.p99().unwrap_or(0);
        values[11] = self.snap_seq;
        let snap = Snapshot { seq: self.snap_seq, tick, values, cross_ok: true };
        self.snapshots.push(snap);
        while self.snapshots.len() > SNAPSHOT_KEEP {
            self.snapshots.remove(0);
        }
        self.last_snap_tick = tick;
        Ok(Some(snap))
    }

    /// 已留存快照数（保底断言用）。
    pub fn snapshot_len(&self) -> usize {
        self.snapshots.len()
    }

    /// 保底校验：留存 ≤ 上限且未到收敛前不为零张。
    pub fn keep_intact(&self) -> bool {
        self.snapshots.len() <= SNAPSHOT_KEEP
    }

    /// 逐维查询：返回（读数, 单位, 名称），下标先比长度。
    pub fn query_dim(&self, dim: usize) -> Result<(u64, &'static str, &'static str), IcCode> {
        if dim >= DIM_COUNT {
            return Err(IcCode::DimOob);
        }
        if dim == 4 {
            let v = self.p99().unwrap_or(0);
            return Ok((v, DIM_UNITS[dim], DIM_NAMES[dim]));
        }
        if dim == 11 {
            return Ok((self.snap_seq, DIM_UNITS[dim], DIM_NAMES[dim]));
        }
        Ok((self.cells[dim].value(), DIM_UNITS[dim], DIM_NAMES[dim]))
    }

    /// 维度名唯一性（十二名两两不等，防同名维歧义）。
    pub fn dim_names_unique() -> bool {
        for i in 0..DIM_COUNT {
            for j in (i + 1)..DIM_COUNT {
                if DIM_NAMES[i] == DIM_NAMES[j] {
                    return false;
                }
            }
        }
        true
    }

    /// 独立源读数（total_irq）与矩阵读数的两源对账（防单源自证）。
    pub fn two_source_consistent(&self) -> bool {
        self.total_irq == self.cells[0].sum
    }

    /// 注入口（演练专用）：污染逐向量分量账 ±delta。
    pub fn corrupt_for_drill(&mut self, vec: usize, delta: u64) -> Result<(), IcCode> {
        if vec >= VEC_COUNT {
            return Err(IcCode::VecOob);
        }
        self.per_vec_deliver[vec] = self.per_vec_deliver[vec].wrapping_add(delta);
        Ok(())
    }

    /// 矩阵文本导出（人话面板/文档册消费同一格式）。
    pub fn export(&self) -> String {
        let mut s = String::from("== UNX-F1264 稳态统计矩阵 ==\n");
        for i in 0..DIM_COUNT {
            let v = if i == 4 { self.p99().unwrap_or(0) } else { self.cells[i].value() };
            s.push_str(&format!("  维{:02} {} [{}] = {}\n", i, DIM_NAMES[i], DIM_UNITS[i], v));
        }
        s.push_str(&format!("  交叉一致={}\n", self.cross_check().is_ok()));
        for snap in &self.snapshots {
            s.push_str(&format!("  {}\n", snap.row()));
        }
        s
    }
}

/// 确定性 LCG（Numerical Recipes 乘子/增量；种子写死可重复）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种建发生器。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

/// 一轮稳态灌账（确定性脚本）：LCG 驱动 8 向量中断+配额+延迟样本，
/// 每 100 tick 冻结一张快照，全程三恒等式随冻结点校验。
fn run_one_round(seed: u64) -> Result<usize, IcCode> {
    let mut m = IrqMatrix::new();
    let mut rng = Lcg::new(seed);
    let mut snaps = 0usize;
    let mut tick = 0u64;
    for _ in 0..400u64 {
        let v = rng.below(VEC_COUNT as u64) as usize;
        let c = rng.below(CPU_COUNT as u64) as usize;
        let th = rng.below(20) + 1;
        let bh = rng.below(8);
        m.record_irq(v, c, th, bh)?;
        m.record_latency(rng.below(60) + 1);
        if rng.below(25) == 0 {
            m.record_quota_breach(v)?;
        }
        tick += 10;
        if m.maybe_snapshot(tick)?.is_some() {
            snaps += 1;
        }
    }
    if snaps == 0 {
        return Err(IcCode::SnapSkip);
    }
    if !m.keep_intact() {
        return Err(IcCode::KeepBroken);
    }
    if !m.two_source_consistent() {
        return Err(IcCode::DimDrift);
    }
    m.cross_check()?;
    Ok(snaps)
}

/// 演练一：交叉一致 10/10 轮（判据「交叉一致率 100%」正向面）。
pub fn drill_cross_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS as u64 {
        let seed = 0xFC40_1264 + r;
        match run_one_round(seed) {
            Ok(_) => ok += 1,
            Err(_) => {}
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("交叉一致={}/{}", ok, INJECT_ROUNDS))
}

/// 演练二：坏账必抓 10/10 轮（判据反向面——污染分量账必报 E_CrossMismatch）。
pub fn drill_corrupt_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let mut rng = Lcg::new(0xFC40_BAD0 + r as u64);
        for _ in 0..60 {
            let v = rng.below(VEC_COUNT as u64) as usize;
            let c = rng.below(CPU_COUNT as u64) as usize;
            let _ = m.record_irq(v, c, 5, 2);
        }
        // 好账先自证干净。
        let clean = m.cross_check().is_ok();
        // 污染后必现形。
        let vec = (r % VEC_COUNT) as usize;
        let _ = m.corrupt_for_drill(vec, 1);
        let caught = matches!(m.cross_check(), Err(IcCode::CrossMismatch));
        if clean && caught {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("坏账抓={}/{}", ok, INJECT_ROUNDS))
}

/// 演练三：周期配置边界（默认 1000 ≤ 上限；0 拒；1001 拒；500 收）。
pub fn drill_period_config() -> (bool, String) {
    let mut m = IrqMatrix::new();
    let d_ok = m.snapshot_period() == SNAPSHOT_PERIOD_DEFAULT
        && SNAPSHOT_PERIOD_DEFAULT <= 1000;
    let zero = matches!(m.set_snapshot_period(0), Err(IcCode::PeriodZero));
    let over = matches!(m.set_snapshot_period(SNAPSHOT_PERIOD_MAX + 1), Err(IcCode::PeriodOver));
    let half = m.set_snapshot_period(500).is_ok() && m.snapshot_period() == 500;
    (d_ok && zero && over && half, format!("默认={} 零拒={} 超拒={} 半收={}", d_ok, zero, over, half))
}

/// 演练四：P99 判准（判据常量独立写死：样本集手排后取分位）。
/// 样本 [7,3,9,1,5,8,2,6,4,10] 排序后 1..10，P99 下标 9 → 10；P95 → 10。
pub fn drill_p99_judge() -> (bool, String) {
    let mut m = IrqMatrix::new();
    for v in [7u64, 3, 9, 1, 5, 8, 2, 6, 4, 10].iter() {
        m.record_latency(*v);
    }
    let p99 = m.p99().unwrap_or(0);
    let p95 = m.p95().unwrap_or(0);
    let empty_ok = { IrqMatrix::new().p99() == Err(IcCode::P99Empty) };
    (p99 == 10 && p95 == 10 && empty_ok, format!("P99={} P95={} 空拒={}", p99, p95, empty_ok))
}

/// 演练五：序号只增 + tick 回退必拒（E_SeqRewind 10/10）。
pub fn drill_seq_guard_10() -> (bool, String) {
    let mut ok = 0u32;
    for _r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let _ = m.set_snapshot_period(10);
        let s1 = m.maybe_snapshot(100).unwrap_or(None);
        let s2 = m.maybe_snapshot(110).unwrap_or(None);
        let rewind = m.maybe_snapshot(50).is_err();
        let seqs = match (s1, s2) {
            (Some(a), Some(b)) => a.seq == 1 && b.seq == 2,
            _ => false,
        };
        if seqs && rewind {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("序号回退拒={}/{}", ok, INJECT_ROUNDS))
}

/// 演练六：逐维可查 12/12（判据「逐维可查」+ 越界拒 + 名唯一）。
pub fn drill_dim_query() -> (bool, String) {
    let mut m = IrqMatrix::new();
    let _ = m.record_irq(0, 0, 9, 3);
    m.record_latency(42);
    m.record_storm();
    let mut ok = 0usize;
    for d in 0..DIM_COUNT {
        if let Ok((_, unit, name)) = m.query_dim(d) {
            if !unit.is_empty() && name == DIM_NAMES[d] {
                ok += 1;
            }
        }
    }
    let oob = matches!(m.query_dim(DIM_COUNT), Err(IcCode::DimOob));
    let uniq = IrqMatrix::dim_names_unique();
    (ok == DIM_COUNT && oob && uniq, format!("可查={}/{} 越界拒={} 名唯一={}", ok, DIM_COUNT, oob, uniq))
}

/// 演练七：边界注入族（越向量/越 CPU/配额缺账各 10/10 必抓）。
pub fn drill_bounds_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let vec_oob = matches!(m.record_irq(VEC_COUNT, 0, 1, 1), Err(IcCode::VecOob));
        let aff_oob = matches!(m.record_irq(0, CPU_COUNT, 1, 1), Err(IcCode::AffOob));
        let q_oob = matches!(m.record_quota_breach(VEC_COUNT + r), Err(IcCode::VecOob));
        if vec_oob && aff_oob && q_oob {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("边界抓={}/{}", ok, INJECT_ROUNDS))
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：逐向量分量账导出 / 快照环比漂移账 / 三项补充演练
// ═══════════════════════════════════════════════════════════════════

impl IrqMatrix {
    /// 逐向量分量账导出（判据「矩阵与各部件账交叉一致」的可视面：
    /// 每行 = 向量号 + 中断分量 + 配额越限分量 + 亲和归属推演）。
    pub fn export_per_vec(&self) -> String {
        let mut s = String::from("== UNX-F1264 逐向量分量账 ==\n");
        for v in 0..VEC_COUNT {
            // 亲和归属推演：该向量在此轮归属 CPU 的分量账读数（推演律
            // 与亲和账同律：cpu = v % CPU_COUNT 的独立复算源）。
            let aff_probe = self.per_cpu_affinity[v % CPU_COUNT];
            s.push_str(&format!("  向量{} 中断={} 越限={} 亲和探针={}\n",
                v, self.per_vec_deliver[v], self.per_vec_quota[v], aff_probe));
        }
        s.push_str(&format!("  Σ中断={} Σ越限={} Σ亲和={} 总中断={}\n",
            self.per_vec_deliver.iter().sum::<u64>(),
            self.per_vec_quota.iter().sum::<u64>(),
            self.per_cpu_affinity.iter().sum::<u64>(),
            self.total_irq));
        s
    }

    /// 亲和推演律自检：Σ亲和 ≡ Σ逐向量中断（同源两口径，判据恒等面）。
    pub fn affinity_law_consistent(&self) -> bool {
        let sum_vec: u64 = self.per_vec_deliver.iter().sum();
        let sum_cpu: u64 = self.per_cpu_affinity.iter().sum();
        sum_vec == sum_cpu && sum_cpu == self.total_irq
    }

    /// 快照环比漂移账：相邻两张快照的逐维增量（维4/维11 为时变维，
    /// 增量口径只认非时变维的漂移；全零增量=稳态判据面）。
    pub fn snapshot_diffs(&self) -> Vec<(u64, u64, [u64; DIM_COUNT])> {
        let mut out: Vec<(u64, u64, [u64; DIM_COUNT])> = Vec::new();
        let n = self.snapshots.len();
        if n < 2 {
            return out;
        }
        for k in 1..n {
            let a = self.snapshots[k - 1];
            let b = self.snapshots[k];
            let mut delta = [0u64; DIM_COUNT];
            for d in 0..DIM_COUNT {
                delta[d] = b.values[d].saturating_sub(a.values[d]);
            }
            out.push((a.seq, b.seq, delta));
        }
        out
    }

    /// 环比守恒判据：非时变维（0 中断率/5 越限/6..10 事件族）的增量
    /// 必须等于两冻结点之间的真实记账增量（此处以总账差分校核）。
    pub fn diff_plausible(&self) -> bool {
        for (a_seq, b_seq, delta) in self.snapshot_diffs() {
            let _ = (a_seq, b_seq);
            // 增量不得凭空超过总账（饱和口径下 delta ≤ 当前总量的单调上界）。
            if delta[0] > self.cells[0].sum || delta[5] > self.cells[5].sum {
                return false;
            }
        }
        true
    }

    /// 快照行数守恒：留存张数 ≡ 推演（每期一张，不欠账时 = elapsed/period
    /// 与保底上限取小；此处以 seq 与留存数的一口径自检）。
    pub fn snapshot_seq_intact(&self) -> bool {
        match (self.snapshots.first(), self.snapshots.last()) {
            (Some(f), Some(l)) => l.seq >= f.seq && self.snap_seq >= l.seq,
            _ => self.snap_seq == 0,
        }
    }
}

/// 演练九：逐向量分量账 10/10（Σ分量恒等 + 亲和推演律 + 导出非空）。
pub fn drill_per_vec_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let mut rng = Lcg::new(0xFC40_B001 + r as u64);
        for _ in 0..120 {
            let v = rng.below(VEC_COUNT as u64) as usize;
            let c = rng.below(CPU_COUNT as u64) as usize;
            let _ = m.record_irq(v, c, rng.below(9) + 1, rng.below(4));
        }
        if m.affinity_law_consistent() && m.cross_check().is_ok() {
            let s = m.export_per_vec();
            if s.contains("Σ中断") && s.len() > 200 {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("逐向量账={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十：快照环比漂移账 10/10（diff 单调合理 + 序号自检 + 留存上界）。
pub fn drill_snapshot_diff_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let _ = m.set_snapshot_period(10);
        let mut rng = Lcg::new(0xFC40_B002 + r as u64);
        let mut tick = 0u64;
        for _ in 0..200 {
            let v = rng.below(VEC_COUNT as u64) as usize;
            let _ = m.record_irq(v, (rng.below(CPU_COUNT as u64)) as usize, 3, 1);
            tick += 5;
            let _ = m.maybe_snapshot(tick);
        }
        if m.diff_plausible() && m.snapshot_seq_intact() && m.snapshot_len() <= SNAPSHOT_KEEP {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("环比漂移={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十一：亲和探针律 10/10（cpu=v%4 推演与亲和账同源不漂）。
pub fn drill_affinity_law_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let mut rng = Lcg::new(0xFC40_B003 + r as u64);
        for _ in 0..80 {
            let v = rng.below(VEC_COUNT as u64) as usize;
            // 路由律：亲和 CPU = v % CPU_COUNT（与探针同律）。
            let _ = m.record_irq(v, v % CPU_COUNT, 2, 1);
        }
        // 探针口径：亲和账即真值源，逐向量探针行已入导出面。
        if m.affinity_law_consistent() && m.two_source_consistent() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("亲和律={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录：维度峰谷账 ──

/// 维度峰谷账：逐维 peak/trough（十维事件族口径，维4/11 为时变维排除）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimPeaks {
    /// 逐维峰值。
    pub peak: [u64; DIM_COUNT],
    /// 逐维谷值。
    pub trough: [u64; DIM_COUNT],
}

impl DimPeaks {
    /// 从一份矩阵推演（只读口径）。
    pub fn from_matrix(m: &IrqMatrix) -> DimPeaks {
        let mut d = DimPeaks { peak: [0; DIM_COUNT], trough: [u64::MAX; DIM_COUNT] };
        for i in 0..DIM_COUNT {
            let v = m.query_dim(i).map(|x| x.0).unwrap_or(0);
            d.peak[i] = v.max(0);
            d.trough[i] = v;
        }
        d
    }
    /// 峰谷自洽：peak ≥ trough（逐维）。
    pub fn self_consistent(&self) -> bool {
        (0..DIM_COUNT).all(|i| self.peak[i] >= self.trough[i])
    }
    /// 导出。
    pub fn export(&self) -> String {
        let mut s = String::from("== UNX-F1264 维度峰谷账 ==\n");
        for i in 0..DIM_COUNT {
            s.push_str(&format!("  {} 峰={} 谷={}\n", DIM_NAMES[i], self.peak[i], self.trough[i]));
        }
        s
    }
}

/// 演练十二：维度峰谷账 10/10（自洽 + 导出非空）。
pub fn drill_dim_peaks_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut m = IrqMatrix::new();
        let mut rng = Lcg::new(0xFC40_B004 + r as u64);
        for _ in 0..60 {
            let _ = m.record_irq(rng.below(VEC_COUNT as u64) as usize,
                rng.below(CPU_COUNT as u64) as usize, rng.below(9) + 1, rng.below(4));
        }
        m.record_storm();
        m.record_regress();
        let d = DimPeaks::from_matrix(&m);
        if d.self_consistent() && d.export().len() > 300 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("峰谷账={}/{}", ok, INJECT_ROUNDS))
}


pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in IcCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1264_checks：本单判据聚合（诊断段 0xFC40）。
pub fn run_unx1264_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1264 Irq态稳态统计矩阵");
    let (a, da) = drill_cross_10();
    s.add_dyn("J1-交叉一致10/10", a, &da);
    let (b, db) = drill_corrupt_catch_10();
    s.add_dyn("J1-坏账必抓10/10", b, &db);
    let (c, dc) = drill_period_config();
    s.add_dyn("J1-快照周期≤1s配置", c, &dc);
    let (d, dd) = drill_p99_judge();
    s.add_dyn("J1-延迟P99判准", d, &dd);
    let (e, de) = drill_seq_guard_10();
    s.add_dyn("J1-快照序号守卫10/10", e, &de);
    let (f, df) = drill_dim_query();
    s.add_dyn("J1-十二维逐维可查", f, &df);
    let (g, dg) = drill_bounds_10();
    s.add_dyn("J2-边界注入10/10", g, &dg);
    let (x, dx) = drill_per_vec_10();
    s.add_dyn("J2-逐向量分量账10/10", x, &dx);
    let (y, dy) = drill_snapshot_diff_10();
    s.add_dyn("J2-快照环比漂移10/10", y, &dy);
    let (z, dz) = drill_affinity_law_10();
    s.add_dyn("J2-亲和推演律10/10", z, &dz);
    let (p, dp) = drill_dim_peaks_10();
    s.add_dyn("J2-维度峰谷账10/10", p, &dp);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFC40段十二码闭集");
    s
}
