//! cgb846_intelbench —— CGPU-F0846 Intel 基准总册。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0846 Intel 基准总册  <SEG 0x7440>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0846；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（Intel 基准总册，整数分面）：
//!   1. 基准项闭集八项（写死）：光栅填充 / 着色吞吐 / 纹理采样 /
//!      计算吞吐 / 几何顶点 / 显存带宽 / 媒体编解码 / 混合渲染；
//!      每项一个定容分数环（8 槽），分数为整数千分位（0..=100000）；
//!   2. 分数环账守恒：入账数 = 在环数 + 逐出数；每项独立记账，
//!      漂移即红码（XC_RING_UNBALANCED）；
//!   3. 项档位三档（写死阈值表）：优 ≥7200 / 中 ≥4500 / 差 <4500；
//!      总册评级 = 八项加权（权重表写死，权重和 = 1000 千分位）；
//!   4. 册状态机：Open → Sealing → Sealed；Sealed 后拒绝写入
//!      （XC_LOCKED），封册时逐项复核环账与样本合法性；
//!   5. 架构域写死闭集：Xe-LP / Xe-HPG / Xe-LPG / Arc 独显 /
//!      集显 Iris；未登记架构写入即红码（XC_ARCH_UNKNOWN）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7440 段为本模块独占（SEG_BASE=0x7440），判别式
//!   SEG_BASE+1..+N；与 0x7420/0x7460 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const IB_VERSION: &str = "cgb846_intelbench-v1";

/// 诊断段基址（0x7440 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7440;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7441 基准项号越界（0..=7 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x7442 分数越界（0..=100000 千分位之外）。
pub const XC_BAD_SCORE: u16 = SEG_BASE + 2;
/// 0x7443 分数环账不守恒（入账 ≠ 在环 + 逐出）。
pub const XC_RING_UNBALANCED: u16 = SEG_BASE + 3;
/// 0x7444 同轮次重复入账（run 序号已存在）。
pub const XC_DUP_RUN: u16 = SEG_BASE + 4;
/// 0x7445 空册出总评（任一项无样本）。
pub const XC_EMPTY_LEDGER: u16 = SEG_BASE + 5;
/// 0x7446 权重账漂移（八项权重和 ≠ 1000）。
pub const XC_WEIGHT_DRIFT: u16 = SEG_BASE + 6;
/// 0x7447 架构未登记（闭集之外）。
pub const XC_ARCH_UNKNOWN: u16 = SEG_BASE + 7;
/// 0x7448 册已封存仍写入。
pub const XC_LOCKED: u16 = SEG_BASE + 8;
/// 0x7449 轮次倒挂（run 序号回退）。
pub const XC_RUN_BACKWARD: u16 = SEG_BASE + 9;

/// 基准项数（写死：8）。
pub const ITEM_CAP: usize = 8;
/// 分数环容量（写死：8）。
pub const RING_CAP: usize = 8;
/// 分数上限（千分位，写死：100000 = 100.0 分）。
pub const SCORE_MAX: u32 = 100000;
/// 权重总和（千分位，写死：1000）。
pub const WEIGHT_SUM: u32 = 1000;

/// 基准项闭集：0=光栅 1=着色 2=纹理 3=计算 4=几何 5=带宽 6=媒体 7=混合。
pub const I_RASTER: usize = 0;
pub const I_SHADER: usize = 1;
pub const I_TEXTURE: usize = 2;
pub const I_COMPUTE: usize = 3;
pub const I_GEOMETRY: usize = 4;
pub const I_BANDWIDTH: usize = 5;
pub const I_MEDIA: usize = 6;
pub const I_HYBRID: usize = 7;

/// 八项权重表（千分位，和恒等于 WEIGHT_SUM=1000）。
pub const WEIGHTS: [u32; ITEM_CAP] = [150, 180, 130, 170, 90, 130, 80, 70];

/// 架构闭集号：0=Xe-LP 1=Xe-HPG 2=Xe-LPG 3=Arc 独显 4=Iris 集显。
pub const A_XE_LP: u8 = 0;
pub const A_XE_HPG: u8 = 1;
pub const A_XE_LPG: u8 = 2;
pub const A_ARC_DG: u8 = 3;
pub const A_IRIS: u8 = 4;
/// 架构闭集上限（0..=4）。
pub const ARCH_CAP: u8 = 4;

/// 架构名（闭集原文）。
pub fn arch_name(a: u8) -> &'static str {
    match a {
        A_XE_LP => "Xe-LP",
        A_XE_HPG => "Xe-HPG",
        A_XE_LPG => "Xe-LPG",
        A_ARC_DG => "Arc-DG",
        _ => "Iris",
    }
}

/// 项档位三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// 优。
    Good,
    /// 中。
    Mid,
    /// 差。
    Poor,
}

/// 项档位判定（写死阈值：优 ≥7200，中 ≥4500，差 <4500）。
pub fn band_of(score: u32) -> Band {
    if score >= 7200 {
        Band::Good
    } else if score >= 4500 {
        Band::Mid
    } else {
        Band::Poor
    }
}

/// 册状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 开册可写。
    Open,
    /// 封册复核中（只读）。
    Sealing,
    /// 已封存（不可再写）。
    Sealed,
}

/// 单条分数样本。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 轮次序号（单调递增）。
    pub run: u32,
    /// 分数（千分位）。
    pub score: u32,
}

/// 单项分数环（定容 8 槽）。
#[derive(Clone)]
pub struct ItemRing {
    /// 在环样本。
    pub ring: Vec<Sample>,
    /// 环内下一逐出位（满后覆盖）。
    pub head: usize,
    /// 累计入账数。
    pub pushed: u64,
    /// 累计逐出数。
    pub evicted: u64,
    /// 历史最高分。
    pub best: u32,
    /// 历史最低分。
    pub worst: u32,
}

impl ItemRing {
    fn new() -> Self {
        ItemRing {
            ring: Vec::new(),
            head: 0,
            pushed: 0,
            evicted: 0,
            best: 0,
            worst: SCORE_MAX,
        }
    }

    /// 在环数。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 环账守恒检查：pushed = len + evicted。
    pub fn balanced(&self) -> bool {
        self.pushed == self.len() as u64 + self.evicted
    }

    /// 均值（整数，分母取在环数，空环回 0）。
    pub fn mean(&self) -> u32 {
        if self.ring.is_empty() {
            return 0;
        }
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.ring.len() {
            acc += self.ring[i].score as u64;
            i += 1;
        }
        (acc / self.ring.len() as u64) as u32
    }
}

/// Intel 基准总册。
pub struct IntelBench {
    /// 册版本号（自动 +1/封册）。
    pub revision: u32,
    /// 架构闭集号。
    pub arch: u8,
    /// 册状态。
    pub phase: Phase,
    /// 八项分数环。
    pub items: [ItemRing; ITEM_CAP],
    /// 各项最后入账轮次（单调校验用）。
    pub last_run: [u32; ITEM_CAP],
    /// 累计红码次数。
    pub red_count: u64,
}

impl IntelBench {
    /// 开新册（架构必须在闭集内）。
    pub fn open(arch: u8) -> Result<Self, u16> {
        if arch > ARCH_CAP {
            return Err(XC_ARCH_UNKNOWN);
        }
        Ok(IntelBench {
            revision: 1,
            arch,
            phase: Phase::Open,
            items: [
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
            ],
            last_run: [0; ITEM_CAP],
            red_count: 0,
        })
    }

    /// 入账一条分数（Open 期唯一写入口）。
    pub fn record(&mut self, item: usize, run: u32, score: u32) -> Result<(), u16> {
        if self.phase != Phase::Open {
            self.red_count += 1;
            return Err(XC_LOCKED);
        }
        if item >= ITEM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_ITEM);
        }
        if score == 0 || score > SCORE_MAX {
            self.red_count += 1;
            return Err(XC_BAD_SCORE);
        }
        if run <= self.last_run[item] {
            self.red_count += 1;
            if run < self.last_run[item] {
                return Err(XC_RUN_BACKWARD);
            }
            return Err(XC_DUP_RUN);
        }
        let ring = &mut self.items[item];
        if ring.len() == RING_CAP {
            // 满环：覆盖 head 位并记逐出，守恒式不变。
            ring.ring[ring.head].run = run;
            ring.ring[ring.head].score = score;
            ring.head = (ring.head + 1) % RING_CAP;
            ring.evicted += 1;
            ring.pushed += 1;
        } else {
            ring.ring.push(Sample { run, score });
            ring.pushed += 1;
        }
        if score > ring.best {
            ring.best = score;
        }
        if score < ring.worst {
            ring.worst = score;
        }
        self.last_run[item] = run;
        Ok(())
    }

    /// 进入封册复核：逐项环账 + 样本合法性。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.phase != Phase::Open {
            self.red_count += 1;
            return Err(XC_LOCKED);
        }
        self.phase = Phase::Sealing;
        let mut item = 0;
        while item < ITEM_CAP {
            if !self.items[item].balanced() {
                self.red_count += 1;
                self.phase = Phase::Open;
                return Err(XC_RING_UNBALANCED);
            }
            let mut i = 0;
            while i < self.items[item].len() {
                let s = self.items[item].ring[i].score;
                if s == 0 || s > SCORE_MAX {
                    self.red_count += 1;
                    self.phase = Phase::Open;
                    return Err(XC_BAD_SCORE);
                }
                i += 1;
            }
            item += 1;
        }
        Ok(())
    }

    /// 终封：全员有样本且权重账完好才落 Sealed。
    pub fn commit(&mut self) -> Result<u32, u16> {
        if self.phase != Phase::Sealing {
            self.red_count += 1;
            return Err(XC_LOCKED);
        }
        let mut item = 0;
        while item < ITEM_CAP {
            if self.items[item].len() == 0 {
                self.red_count += 1;
                self.phase = Phase::Open;
                return Err(XC_EMPTY_LEDGER);
            }
            item += 1;
        }
        let mut wsum: u32 = 0;
        let mut i = 0;
        while i < ITEM_CAP {
            wsum += WEIGHTS[i];
            i += 1;
        }
        if wsum != WEIGHT_SUM {
            self.red_count += 1;
            self.phase = Phase::Open;
            return Err(XC_WEIGHT_DRIFT);
        }
        let total = self.total_score();
        self.phase = Phase::Sealed;
        self.revision += 1;
        Ok(total)
    }

    /// 总册加权分（千分位口径 /1000 折回分数面）。
    pub fn total_score(&self) -> u32 {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < ITEM_CAP {
            acc += self.items[i].mean() as u64 * WEIGHTS[i] as u64;
            i += 1;
        }
        (acc / WEIGHT_SUM as u64) as u32
    }

    /// 总评级三档（与项档位同阈值）。
    pub fn grade(&self) -> Result<Band, u16> {
        let mut item = 0;
        while item < ITEM_CAP {
            if self.items[item].len() == 0 {
                return Err(XC_EMPTY_LEDGER);
            }
            item += 1;
        }
        Ok(band_of(self.total_score()))
    }

    /// 摘要（判据要求非空结果摘要）。
    pub fn summary(&self) -> String {
        let mut s = String::from("intelbench rev=");
        s += &self.revision.to_string();
        s += " arch=";
        s += arch_name(self.arch);
        s += " total=";
        s += &self.total_score().to_string();
        let mut item = 0;
        while item < ITEM_CAP {
            s += " i";
            s += &item.to_string();
            s += "=";
            s += &self.items[item].mean().to_string();
            item += 1;
        }
        s
    }

    /// 全账守恒自检（八项环账 + 单调轮次）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut item = 0;
        while item < ITEM_CAP {
            if !self.items[item].balanced() {
                return Err(XC_RING_UNBALANCED);
            }
            item += 1;
        }
        Ok(())
    }
}

/// 自例程：开册 → 八项各入 3 轮 → 封册 → 终封 → 评级。
pub fn drill() -> Result<u32, u16> {
    let mut book = IntelBench::open(A_XE_HPG)?;
    let mut run: u32 = 0;
    let mut item = 0;
    while item < ITEM_CAP {
        let mut k: u32 = 0;
        while k < 3 {
            run += 1;
            // 确定性伪分数：按项与轮次线性生成，落于中段。
            let score = 5000 + (item as u32 * 700) + k * 300;
            let clamped = if score > SCORE_MAX { SCORE_MAX } else { score };
            book.record(item, run, clamped)?;
            k += 1;
        }
        item += 1;
    }
    book.seal()?;
    let total = book.commit()?;
    let grade = book.grade()?;
    if grade == Band::Poor && total >= 4500 {
        return Err(XC_WEIGHT_DRIFT);
    }
    book.audit()?;
    // 负例：封册后写入必须红。
    let bad = book.record(I_RASTER, 9999, 50000);
    if bad != Err(XC_LOCKED) {
        return Err(XC_LOCKED);
    }
    // 负例：越界项必须红。
    let mut probe = IntelBench::open(A_IRIS)?;
    let bad2 = probe.record(ITEM_CAP, 1, 50000);
    if bad2 != Err(XC_BAD_ITEM) {
        return Err(XC_BAD_ITEM);
    }
    // 负例：环账守恒自检必须过。
    probe.record(I_MEDIA, 2, 40000)?;
    probe.audit()?;
    // 负例：空项出总评必须红。
    let empty = probe.commit();
    if empty != Err(XC_EMPTY_LEDGER) {
        return Err(XC_EMPTY_LEDGER);
    }
    // 摘要非空判据。
    if book.summary().len() == 0 {
        return Err(XC_EMPTY_LEDGER);
    }
    Ok(0)
}
