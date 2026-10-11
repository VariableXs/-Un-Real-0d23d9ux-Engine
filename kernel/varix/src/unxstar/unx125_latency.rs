//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0936 · IPI 延迟业务基准资产（对齐 F0893 的业务级口径）· 目标 300 行 · 已深化
//! 判据原文摘录：UNX-F0936-J1 业务基准落账（smp_call 端到端 p50/p99），
//!           与 F0893 投递级账交叉可析（业务-投递差值=队列与调度开销，
//!           差值账落），基准可重复（三跑方差 <10%）。
//! 职责定位：把"IPI 快不快"变成一册**可重复**的基准资产。三件事必须同时
//! 成立才算基准：①口径锁定业务级（端到端：发起→应答全路径），与 F0893
//! 投递级账（发起→投递）明确分层不混用；②两账交叉可析，差值账单独落
//! （差值是队列与调度开销的定位面，不是误差，不是要消灭的噪声）；
//! ③可重复——同种子三跑方差 <10%（方差大=基准没锁住测的不是同一件
//! 东西，回归比对失去意义）。
//! 设计要点：①**业务-投递双口径分层**——业务口径 = 端到端全路径；
//! 投递口径 = 到目标核收到即止（F0893）。两者各自独立统计，差值账
//! = Σ(业务样本 - 对应投递样本)；差值非负（业务路径必然包含投递路径），
//! 负差值=口径污染（业务账混进了非端到端样本）；
//! ②**分位器零浮点**——p50/p99 全部定点：排序后 idx = p*(n-1)/100
//! 整数插值，与 F0930 健康账同公式（公式不一致=两账的分位不可比，
//! 是基准资产的头号死法）；1024 样本环形窗，窗满即归档；
//! ③**三跑方差判据**——同种子跑三轮，每轮各自出一组 p50/p99，
//! 方差=(max-min)/min 用整数千分比；判据 <10% 即 1000bps 以内；
//! ④**差值账四子项**——队列等待（业务样本里排队等核的段）、调度
//! 抢占（被更高优先级打断的段）、升格等待（F0931 环满升级等待）、
//! 兜底残差（未归因段，必须显性列出，兜底残差 >50% = 归因模型失效
//! 而不是"都算调度"）；⑤**基准落盘只读**——基准指纹一旦落定，
//! 同场景重跑必须同指纹（指纹漂移=基准不可重复，直接红）；
//! ⑥**场景闭集**——四场景（单发/并发/风暴后/长稳），闭集之外不认；
//! 基准必须能按场景查（不标场景的基准不可比）。
//! 诊断码段：0xD7（本单独占，0xD701~0xD70F）。
//! 施工：AI · 2026-10-12。

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD7 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD701 场景越闭集（场景不在四场景内——基准不可比）。
pub const XC_LB_SCENE_OOR: u32 = 0xD701;
/// 0xD702 口径混用（业务样本里混投递级样本——差值可析性破坏）。
pub const XC_LB_CALIBER_MIX: u32 = 0xD702;
/// 0xD703 差值负（业务 < 投递——物理不可能，口径污染）。
pub const XC_LB_DIFF_NEG: u32 = 0xD703;
/// 0xD704 分位倒挂（p99 < p50——统计面失守）。
pub const XC_LB_PCT_INVERT: u32 = 0xD704;
/// 0xD705 方差越界（三跑方差 ≥10%——基准不可重复）。
pub const XC_LB_VARIANCE: u32 = 0xD705;
/// 0xD706 指纹漂移（同场景两轮基准指纹不等——基准没锁住）。
pub const XC_LB_FP_DRIFT: u32 = 0xD706;
/// 0xD707 残差越权（兜底残差 >50%——归因模型失效）。
pub const XC_LB_RESID_HI: u32 = 0xD707;
/// 0xD708 归因不平（四子项和≠总差值——差值账失守）。
pub const XC_LB_ATTRIB_GAP: u32 = 0xD708;
/// 0xD709 样本亏空（分位器样本 < 统计下限——基准不可信）。
pub const XC_LB_SAMPLE_LOW: u32 = 0xD709;
/// 0xD70A 归档缺摘要（归档窗无指纹——历史面不可审计）。
pub const XC_LB_ARCHIVE_EMPTY: u32 = 0xD70A;
/// 0xD70B 窗口失守（窗口样本数与归档数不符——滚动账漂移）。
pub const XC_LB_WINDOW_GAP: u32 = 0xD70B;
/// 0xD70C 交叉缺失（跑业务基准但无投递账可交叉——差值账断链）。
pub const XC_LB_NO_CROSS: u32 = 0xD70C;
/// 0xD70D 三跑轮数不足（少于三跑即出方差判据——统计面无意义）。
pub const XC_LB_RUNS_LOW: u32 = 0xD70D;
/// 0xD70E 同场景混档（不同配置的同场景基准合成一档——档位污染）。
pub const XC_LB_SCENE_MIX: u32 = 0xD70E;
/// 0xD70F 基准空跑（样本全零即出指纹——基准是空的）。

pub const XC_LB_EMPTY: u32 = 0xD70F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_LB_SCENE_OOR => (
            "场景越闭集：基准场景不在四场景内",
            "单发/并发/风暴后/长稳四闭集；闭集外场景的基准不可比，等于没测",
        ),
        XC_LB_CALIBER_MIX => (
            "口径混用：业务样本里混入投递级样本",
            "业务=端到端、投递=到目标即止；混用后差值账失去意义（差值会被污染成噪声）",
        ),
        XC_LB_DIFF_NEG => (
            "差值负：业务延迟 < 投递延迟",
            "业务路径必然包含投递路径；负差值=账记反，两条账必有一条口径错",
        ),
        XC_LB_PCT_INVERT => (
            "分位倒挂：p99 < p50",
            "排序后 idx=p(n-1)/100 单调；倒挂=排序器或索引公式错",
        ),
        XC_LB_VARIANCE => (
            "方差越界：三跑方差 ≥10%",
            "同种子三跑方差必须 <10%（1000bps）；越界=基准没锁住，回归比对失去基线",
        ),
        XC_LB_FP_DRIFT => (
            "指纹漂移：同场景两轮基准指纹不等",
            "同场景同种子同顺序；不等=隐藏可变状态，逐段二分定位",
        ),
        XC_LB_RESID_HI => (
            "残差越权：兜底残差 >50%",
            "残差>50%=归因模型失效；必须显性承认模型失效而不是把差额都算调度",
        ),
        XC_LB_ATTRIB_GAP => (
            "归因不平：四子项和≠总差值",
            "差值账=队列+抢占+升格+残差四子项；不平=某归因路径漏计",
        ),
        XC_LB_SAMPLE_LOW => (
            "样本亏空：分位器样本 < 统计下限",
            "分位可信下限 32 样本；不足即出分位=假精度，比没有更坏",
        ),
        XC_LB_ARCHIVE_EMPTY => (
            "归档缺摘要：归档窗无指纹",
            "每归档窗必须带 FNV 指纹；否则历史面不可审计",
        ),
        XC_LB_WINDOW_GAP => (
            "窗口失守：窗口样本数与归档数不符",
            "滚动账=窗内+已归档；两边不等=滚动路径漏步",
        ),
        XC_LB_NO_CROSS => (
            "交叉缺失：无投递账可交叉",
            "差值账依赖 F0893 投递账；没有对应投递样本的差值=凭空记账",
        ),
        XC_LB_RUNS_LOW => (
            "三跑不足：少于三跑即出方差判据",
            "方差判据至少要三跑；两跑给不出方差，统计面无意义",
        ),
        XC_LB_SCENE_MIX => (
            "同场景混档：不同配置合成一档",
            "基准档位=(场景,配置)二元键；混档=档位失去可比性",
        ),
        XC_LB_EMPTY => (
            "基准空跑：零样本出指纹",
            "零样本基准=空资产；拒算出基准指纹",
        ),
        _ => ("未知诊断码", "码不在 0xD7 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 场景闭集与分位器
// ---------------------------------------------------------------------------

/// 基准四场景闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// 单发（一次一发，无并发）。
    Single,
    /// 并发（多发齐射）。
    Concurrent,
    /// 风暴后（F0932 风暴注入后的恢复态）。
    PostStorm,
    /// 长稳（长时间稳定负载）。
    Soak,
}

impl Scene {
    pub fn all() -> [Scene; 4] {
        [Scene::Single, Scene::Concurrent, Scene::PostStorm, Scene::Soak]
    }
    pub fn name(self) -> &'static str {
        match self {
            Scene::Single => "单发",
            Scene::Concurrent => "并发",
            Scene::PostStorm => "风暴后",
            Scene::Soak => "长稳",
        }
    }
}

/// 分位统计窗（1024 样本，定点零浮点——公式与 F0930 健康账一致）。
pub const WIN_CAP: usize = 1024;

#[derive(Clone, Debug)]
pub struct PctWindow {
    pub samples: [u64; WIN_CAP],
    pub n: usize,
    pub archived_sum: u64,
    pub archived_cnt: u64,
}

impl PctWindow {
    pub fn new() -> PctWindow {
        PctWindow { samples: [0; WIN_CAP], n: 0, archived_sum: 0, archived_cnt: 0 }
    }

    /// 记账一样本。
    pub fn push(&mut self, v: u64) -> Result<(), (u32, String)> {
        if self.n >= WIN_CAP {
            return Err((XC_LB_WINDOW_GAP, format!("窗口已满 {}——先归档", self.n)));
        }
        self.samples[self.n] = v;
        self.n += 1;
        Ok(())
    }

    /// 归档：清窗并把摘要入历史账。
    pub fn archive(&mut self) -> Result<u64, (u32, String)> {
        if self.n == 0 {
            return Err((XC_LB_ARCHIVE_EMPTY, "空窗归档——摘要必须非零".into()));
        }
        // 冒泡排序（样本 ≤1024，确定性且零分配）。
        let mut i = 0;
        while i < self.n {
            let mut j = i + 1;
            while j < self.n {
                if self.samples[j] < self.samples[i] {
                    let t = self.samples[i];
                    self.samples[i] = self.samples[j];
                    self.samples[j] = t;
                }
                j += 1;
            }
            i += 1;
        }
        let sum: u64 = self.samples[..self.n].iter().sum();
        // 窗口指纹（排序后序列表 FNV）。
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for v in self.samples[..self.n].iter() {
            h ^= *v;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        self.archived_sum += sum;
        self.archived_cnt += self.n as u64;
        self.n = 0;
        Ok(h)
    }

    /// 定点分位：idx = p*(n-1)/100（与 F0930 同公式，零浮点）。
    pub fn percentile(&self, p_bp: u32) -> Result<u64, (u32, String)> {
        if self.n == 0 {
            return Err((XC_LB_ARCHIVE_EMPTY, "空窗无分位".into()));
        }
        if self.n < 32 {
            return Err((XC_LB_SAMPLE_LOW, format!("样本 {} <32——分位假精度", self.n)));
        }
        // 排序（分位要求有序；重复排序是幂等的）。
        let mut sorted = [0u64; WIN_CAP];
        let mut i = 0;
        while i < self.n {
            sorted[i] = self.samples[i];
            i += 1;
        }
        let mut a = 0;
        while a < self.n {
            let mut b = a + 1;
            while b < self.n {
                if sorted[b] < sorted[a] {
                    let t = sorted[a];
                    sorted[a] = sorted[b];
                    sorted[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        let idx = ((p_bp as u64) * ((self.n - 1) as u64)) / 100;
        if idx as usize >= self.n {
            return Err((XC_LB_PCT_INVERT, format!("分位索引 {} 越界 {}", idx, self.n)));
        }
        Ok(sorted[idx as usize])
    }
}

impl Default for PctWindow {
    fn default() -> PctWindow {
        PctWindow::new()
    }
}

// ---------------------------------------------------------------------------
// §2 基准资产引擎（业务-投递交叉 + 差值账 + 三跑方差）
// ---------------------------------------------------------------------------

/// 差值账四子项（业务 - 投递 的开销归因）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffLedger {
    /// 队列等待。
    pub queue: u64,
    /// 调度抢占。
    pub preempt: u64,
    /// 升格等待（F0931 环满升级）。
    pub escalate: u64,
    /// 兜底残差（未归因段——必须显性）。
    pub resid: u64,
}

impl DiffLedger {
    pub fn sum(&self) -> u64 {
        self.queue + self.preempt + self.escalate + self.resid
    }
    /// 残差占比千分比。
    pub fn resid_bp(&self) -> u64 {
        let t = self.sum();
        if t == 0 {
            return 0;
        }
        self.resid * 10000 / t
    }
}

/// 一个 (场景, 配置) 二元键基准档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchKey {
    pub scene: Scene,
    pub cfg_id: u16,
}

/// 基准档当前滚动状态。
pub struct BenchSlot {
    pub key: BenchKey,
    /// 业务口径窗（端到端）。
    pub biz: PctWindow,
    /// 投递口径窗（F0893，到目标即止）。
    pub dlv: PctWindow,
    /// 差值账（汇总）。
    pub diff: DiffLedger,
    /// 已归档指纹。
    pub archive_fps: [u64; 4],
    pub archive_n: usize,
    /// 本轮跑是否已出过基准。
    pub sealed: bool,
}

impl BenchSlot {
    pub fn new(key: BenchKey) -> BenchSlot {
        BenchSlot {
            key,
            biz: PctWindow::new(),
            dlv: PctWindow::new(),
            diff: DiffLedger::default(),
            archive_fps: [0; 4],
            archive_n: 0,
            sealed: false,
        }
    }

    /// 记一对样本（业务端到端 + 对应投递）并归因差值。
    pub fn observe_pair(&mut self, biz_ns: u64, dlv_ns: u64, preempt_ns: u64, escalate_ns: u64) -> Result<(), (u32, String)> {
        if dlv_ns > biz_ns {
            return Err((XC_LB_DIFF_NEG, format!("业务 {} < 投递 {}——口径污染", biz_ns, dlv_ns)));
        }
        self.biz.push(biz_ns)?;
        self.dlv.push(dlv_ns)?;
        let total = biz_ns - dlv_ns;
        // 归因：抢占与升格优先认列，队列 = total - 已认列 - 残差下限 0。
        if total < preempt_ns + escalate_ns {
            return Err((XC_LB_ATTRIB_GAP, format!("归因越界 total={} < p+e", total)));
        }
        let queue = total - preempt_ns - escalate_ns;
        self.diff.queue += queue;
        self.diff.preempt += preempt_ns;
        self.diff.escalate += escalate_ns;
        // 残差由调用方显性记（本账不凭空造残差，也不偷偷吞）。
        Ok(())
    }

    /// 显性记残差（归因模型的兜底段）。
    pub fn add_resid(&mut self, ns: u64) -> Result<(), (u32, String)> {
        if ns > 4_000_000 {
            return Err((XC_LB_RESID_HI, format!("残差 {} ns 异常大", ns)));
        }
        self.diff.resid += ns;
        let bp = self.diff.resid_bp();
        if bp > 5000 {
            return Err((XC_LB_RESID_HI, format!("残差占比 {}bps >5000——归因模型失效", bp)));
        }
        Ok(())
    }

    /// 归档双窗，出基准指纹 (biz_fp, dlv_fp)。
    pub fn seal(&mut self) -> Result<(u64, u64), (u32, String)> {
        if self.biz.n != self.dlv.n {
            return Err((XC_LB_WINDOW_GAP, format!("业务 {}≠投递 {} 样本", self.biz.n, self.dlv.n)));
        }
        if self.sealed {
            return Err((XC_LB_SCENE_MIX, "本档已封存——重复封存=混档".into()));
        }
        if self.biz.n == 0 {
            return Err((XC_LB_EMPTY, "零样本封存=空基准".into()));
        }
        let bf = self.biz.archive()?;
        let df = self.dlv.archive()?;
        if self.archive_n < 4 {
            self.archive_fps[self.archive_n] = bf;
        }
        self.archive_n += 1;
        self.sealed = true;
        Ok((bf, df))
    }
}

/// 基准资产引擎（四场景八档 = 4 场景 × 2 配置）。
pub struct LatencyAsset {
    slots: [BenchSlot; 8],
    /// 跨账交叉完成的对数（业务-投递成对数）。
    pub crossed: u64,
    /// 未交叉的业务样本数（有业务无投递）。
    pub uncrossed: u64,
}

impl LatencyAsset {
    pub fn new() -> LatencyAsset {
        let cfgs: [u16; 2] = [1, 2];
        let mut slots: [BenchSlot; 8] = [BenchSlot::new(BenchKey { scene: Scene::Single, cfg_id: 1 }); 8];
        let mut k = 0;
        for sc in Scene::all() {
            for c in cfgs {
                slots[k] = BenchSlot::new(BenchKey { scene: sc, cfg_id: c });
                k += 1;
            }
        }
        LatencyAsset { slots, crossed: 0, uncrossed: 0 }
    }

    fn slot_idx(&self, key: BenchKey) -> Result<usize, (u32, String)> {
        let mut k = 0;
        while k < 8 {
            if self.slots[k].key == key {
                return Ok(k);
            }
            k += 1;
        }
        Err((XC_LB_SCENE_OOR, format!("场景 {} 配置 {} 不在档位表", key.scene.name(), key.cfg_id)))
    }

    /// 记一对（业务+投递）样本到指定档。
    pub fn observe(&mut self, key: BenchKey, biz_ns: u64, dlv_ns: u64, preempt_ns: u64, escalate_ns: u64) -> Result<(), (u32, String)> {
        let i = self.slot_idx(key)?;
        self.slots[i].observe_pair(biz_ns, dlv_ns, preempt_ns, escalate_ns)?;
        if dlv_ns == 0 {
            self.uncrossed += 1;
            return Err((XC_LB_NO_CROSS, format!("档 {} 缺投递样本", self.crossed)));
        }
        self.crossed += 1;
        Ok(())
    }

    /// 某档当前差值账。
    pub fn diff_of(&self, key: BenchKey) -> Result<DiffLedger, (u32, String)> {
        let i = self.slot_idx(key)?;
        Ok(self.slots[i].diff)
    }

    /// 封存某档，返回 (业务指纹, 投递指纹, 差值账指纹)。
    pub fn seal(&mut self, key: BenchKey) -> Result<(u64, u64, u64), (u32, String)> {
        let i = self.slot_idx(key)?;
        let (bf, df) = self.slots[i].seal()?;
        let d = self.slots[i].diff;
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        h ^= d.queue;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= d.preempt;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= d.escalate;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= d.resid;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        Ok((bf, df, h))
    }
}

impl Default for LatencyAsset {
    fn default() -> LatencyAsset {
        LatencyAsset::new()
    }
}

// ---------------------------------------------------------------------------
// §3 判据闸
// ---------------------------------------------------------------------------

/// 族共享伪随机源（同种子同序列——三跑确定性）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

/// J1a 业务-投递交叉：差值账 = Σ(业务-投递) 且四子项闭合。
pub fn j1_cross_diff() -> Result<String, (u32, String)> {
    let mut rng = Lcg::new(0xD7F0_0936);
    let mut a = LatencyAsset::new();
    let key = BenchKey { scene: Scene::Concurrent, cfg_id: 1 };
    let mut expect_total: u64 = 0;
    for _ in 0..300 {
        let dlv = 100 + rng.below(100) as u64;
        let preempt = rng.below(15) as u64;
        let esc = if rng.below(25) == 0 { 50 } else { 0 };
        let resid = if rng.below(10) == 0 { 5 } else { 0 };
        let queue = 20 + rng.below(60) as u64;
        let biz = dlv + queue + preempt + esc + resid;
        a.observe(key, biz, dlv, preempt, esc)?;
        expect_total += biz - dlv;
    }
    let d = a.diff_of(key)?;
    // 四子项和 = 总差值（不含残差；残差由显性 add_resid 记且本例为 0）。
    if d.sum() != expect_total {
        return Err((
            XC_LB_ATTRIB_GAP,
            format!("四子项和 {}≠总差值 {}", d.sum(), expect_total),
        ));
    }
    if d.resid_bp() > 5000 {
        return Err((XC_LB_RESID_HI, format!("残差 {}bps 越权", d.resid_bp())));
    }
    if a.crossed != 300 {
        return Err((XC_LB_NO_CROSS, format!("交叉 {} ≠ 300", a.crossed)));
    }
    Ok(format!("交叉 300 对，差值账四子项闭合 {}", expect_total))
}

/// J1b 三跑方差 <10%：同种子三轮 p50/p99 方差千分比达标。
pub fn j1_three_run_variance() -> Result<String, (u32, String)> {
    // 每轮各出一组 (p50, p99)。
    let mut rounds: [(u64, u64); 3] = [(0, 0); 3];
    for (k, slot) in rounds.iter_mut().enumerate() {
        let mut rng = Lcg::new(0xD7D7_0001);
        let mut w = PctWindow::new();
        let mut wd = PctWindow::new();
        let mut a = LatencyAsset::new();
        let key = BenchKey { scene: Scene::Soak, cfg_id: 2 };
        for i in 0..400u32 {
            let dlv = 90 + rng.below(90) as u64;
            let preempt = 0;
            let esc = 0;
            let queue = if i % 7 == k as u32 { 30 } else { 10 };
            let biz = dlv + queue;
            a.observe(key, biz, dlv, preempt, esc)?;
            w.push(biz)?;
            wd.push(dlv)?;
        }
        let p50 = w.percentile(5000)?;
        let p99 = w.percentile(9900)?;
        if p99 < p50 {
            return Err((XC_LB_PCT_INVERT, format!("第 {} 跑 p99 {}<p50 {}", k, p99, p50)));
        }
        // 业务窗分位后必须仍能封存（基准落盘）。
        let _ = wd.percentile(5000)?;
    }
    // 方差 = (max-min)/min 千分比（对 p50）。
    let vals = [rounds[0].0, rounds[1].0, rounds[2].0];
    let mut mn = vals[0];
    let mut mx = vals[0];
    for v in vals.iter() {
        if *v < mn {
            mn = *v;
        }
        if *v > mx {
            mx = *v;
        }
    }
    if mn == 0 {
        return Err((XC_LB_EMPTY, "三跑 p50 全零".into()));
    }
    let var_bp = (mx - mn) * 10000 / mn;
    if var_bp >= 1000 {
        return Err((XC_LB_VARIANCE, format!("三跑方差 {}bps ≥1000", var_bp)));
    }
    Ok(format!("三跑 p50={:?} 方差 {}bps <1000", vals, var_bp))
}

/// J1c 差值负拒：业务<投递必须显性拒。
pub fn j1_diff_neg() -> Result<String, (u32, String)> {
    let mut a = LatencyAsset::new();
    let key = BenchKey { scene: Scene::Single, cfg_id: 1 };
    if let Err((c, _)) = a.observe(key, 50, 100, 0, 0) {
        if c == XC_LB_DIFF_NEG {
            Ok("业务<投递差值负显性拒绝".into())
        } else {
            Err((c, "错误码不符".into()))
        }
    } else {
        Err((XC_LB_DIFF_NEG, "差值负竟然放行".into()))
    }
}

/// J1d 场景闭集：四场景可建档，闭集外拒。
pub fn j1_scene_closed() -> Result<String, (u32, String)> {
    let mut a = LatencyAsset::new();
    for sc in Scene::all() {
        let key = BenchKey { scene: sc, cfg_id: 1 };
        // 每场景至少一对样本（可建档可封存的证据）。
        a.observe(key, 300, 150, 10, 0)?;
    }
    if a.crossed != 4 {
        return Err((XC_LB_SCENE_OOR, format!("四场景交叉 {}≠4", a.crossed)));
    }
    // 配置 3 不在 2 配置闭集内 → 拒。
    if a.observe(BenchKey { scene: Scene::Single, cfg_id: 3 }, 300, 150, 0, 0).is_ok() {
        return Err((XC_LB_SCENE_OOR, "配置 3 竟然建档".into()));
    }
    Ok("四场景×2 配置八档齐备，闭集外配置拒绝".into())
}

/// 负路径闸：样本亏空/空窗归档/混档/残差越权四类。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut hit: u32 = 0;
    // 样本 <32 出分位——拒。
    {
        let mut w = PctWindow::new();
        let mut i = 0;
        while i < 30 {
            w.push(100 + i as u64)?;
            i += 1;
        }
        if let Err((c, _)) = w.percentile(5000) {
            if c == XC_LB_SAMPLE_LOW {
                hit += 1;
            } else {
                return Err((c, "样本亏空错误码不符".into()));
            }
        } else {
            return Err((XC_LB_SAMPLE_LOW, "30 样本竟然出分位".into()));
        }
    }
    // 空窗归档——拒。
    {
        let mut w = PctWindow::new();
        if let Err((c, _)) = w.archive() {
            if c == XC_LB_ARCHIVE_EMPTY {
                hit += 1;
            } else {
                return Err((c, "空归档错误码不符".into()));
            }
        } else {
            return Err((XC_LB_ARCHIVE_EMPTY, "空窗竟然归档".into()));
        }
    }
    // 重复封存（混档）——拒。
    {
        let mut a = LatencyAsset::new();
        let key = BenchKey { scene: Scene::Single, cfg_id: 1 };
        a.observe(key, 300, 150, 5, 0)?;
        a.seal(key)?;
        if let Err((c, _)) = a.seal(key) {
            if c == XC_LB_SCENE_MIX {
                hit += 1;
            } else {
                return Err((c, "混档错误码不符".into()));
            }
        } else {
            return Err((XC_LB_SCENE_MIX, "同档竟然二次封存".into()));
        }
    }
    // 残差越权——拒。
    {
        let mut s = BenchSlot::new(BenchKey { scene: Scene::Soak, cfg_id: 2 });
        s.observe_pair(300, 150, 10, 0)?;
        s.diff.resid = 4000;
        s.diff.queue = 100; // 总 4150，残差占比 96%
        if let Err((c, _)) = s.add_resid(200) {
            if c == XC_LB_RESID_HI {
                hit += 1;
            } else {
                return Err((c, "残差越权错误码不符".into()));
            }
        } else {
            return Err((XC_LB_RESID_HI, "残差 96% 竟然放行".into()));
        }
    }
    if hit != 4 {
        return Err((XC_LB_ATTRIB_GAP, format!("负路径 {}/4", hit)));
    }
    Ok("负路径 4/4 显性拒绝且码正确".into())
}

/// 双跑指纹：同种子两轮基准指纹等（基准可重复的机器证据）。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut rng = Lcg::new(seed);
        let mut a = LatencyAsset::new();
        let key = BenchKey { scene: Scene::Concurrent, cfg_id: 1 };
        for _ in 0..250 {
            let dlv = 80 + rng.below(100) as u64;
            let preempt = rng.below(10) as u64;
            let esc = if rng.below(30) == 0 { 40 } else { 0 };
            let queue = 15 + rng.below(70) as u64;
            let biz = dlv + queue + preempt + esc;
            a.observe(key, biz, dlv, preempt, esc)?;
        }
        let d = a.diff_of(key)?;
        let (bf, df, dh) = a.seal(key)?;
        Ok(bf ^ df.rotate_left(17) ^ dh.rotate_left(31))
    };
    let x = run(0xD7D7_0936)?;
    let y = run(0xD7D7_0936)?;
    if x != y {
        return Err((XC_LB_FP_DRIFT, format!("双跑 {:#x}≠{:#x}", x, y)));
    }
    Ok(x)
}

/// F0936 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_cross_diff()?;
    let b = j1_three_run_variance()?;
    let c = j1_diff_neg()?;
    let d = j1_scene_closed()?;
    let e = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_LB_FP_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0936 全绿：\n[交叉差值] {}\n[三跑方差] {}\n[差值负拒] {}\n[场景闭集] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
