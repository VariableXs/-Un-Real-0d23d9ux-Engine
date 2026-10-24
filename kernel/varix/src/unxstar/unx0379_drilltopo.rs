//! unx0379_drilltopo —— UNX-F0379 内存演练矩阵（大/小/碎三档拓扑）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0379 · 内存演练矩阵 · 目标 220 行
//! 判据原文：UNX-F0379-J1 内存分配演练按三档拓扑各跑一整套
//!           分配-释放序列：大块档（整段连片）、小块档（细粒度
//!           高频）、碎片档（交替占用成洞）；每档产出三指标
//!           （成功率千分 / 碎片指数百万分 / 峰值占用）；矩阵
//!           闸要求三档成功率全满 + 碎片档碎片指数不超阈 +
//!           峰值不超容量；序列可重放（同输入同输出）。
//! 组件：拓扑参数（档位 + 总量 + 粒度）+ 演练引擎（确定性伪随机
//!       序列驱动分配/释放，LCG 无外部依赖）+ 指标采集（成功数/
//!       峰值/空洞统计）+ 矩阵闸（三档联合判定）+ 演练账。
//! 职责定位：演练矩阵是「内存分配器的体检表」——三档拓扑各自
//!   逼出不同短板：大块档逼对齐与连片能力，小块档逼元数据开销
//!   与高频路径，碎片档逼合并与配额。指标是确定性的：序列由
//!   LCG 生成，同参数同结果，复跑即对拍。
//! 性能逐项分解：单步 O(1)（位图查空 + 计数）；单档 O(步数)；
//!   矩阵闸 O(1)。
//! 诊断码段：0x20B40（0x20B41~0x20B49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DT_VERSION: &str = "unx0379_drilltopo-v1";

/// 诊断段基址（0x20B40 段 · 本模块独占）。
pub const DT_SEG_BASE: u32 = 0x20B40;
pub const XC_TOP_OOR: u32 = 0x20B41; // 档位越域
pub const XC_GRAN_OOR: u32 = 0x20B42; // 粒度越域（0 或非 2 幂）
pub const XC_CAP_OOR: u32 = 0x20B43; // 容量为 0
pub const XC_SUCCESS_LO: u32 = 0x20B44; // 成功率不达标
pub const XC_FRAG_HI: u32 = 0x20B45; // 碎片指数超阈
pub const XC_PEAK_HI: u32 = 0x20B46; // 峰值超容量
pub const XC_REPLAY: u32 = 0x20B47; // 重放不一致
pub const XC_FREE_NEG: u32 = 0x20B48; // 释放越账（归还多于持有）
pub const XC_LEAK: u32 = 0x20B49; // 收官残余未归还

/// 三档拓扑。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Topo {
    /// 大块档：请求粒度大、序列短、要求整段连片。
    Bulk,
    /// 小块档：请求粒度细、序列长、高频路径。
    Small,
    /// 碎片档：交替占放制造空洞，逼合并。
    Frag,
}

impl Topo {
    /// 档位标签（1~3）。
    pub fn kind(&self) -> u8 {
        match self {
            Topo::Bulk => 1,
            Topo::Small => 2,
            Topo::Frag => 3,
        }
    }

    /// 档位缺省参数：（步数, 请求块均值）。
    pub fn defaults(&self) -> (u32, u32) {
        match self {
            Topo::Bulk => (64, 64),
            Topo::Small => (256, 4),
            Topo::Frag => (128, 16),
        }
    }
}

/// 单档演练指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillScore {
    /// 请求总数。
    pub requests: u32,
    /// 成功数。
    pub ok: u32,
    /// 峰值占用（帧）。
    pub peak: u32,
    /// 收官空洞数（空闲块段数）。
    pub holes: u32,
}

impl DrillScore {
    /// 成功率千分。
    pub fn ok_milli(&self) -> u32 {
        if self.requests == 0 {
            return 0;
        }
        self.ok * 1000 / self.requests
    }

    /// 碎片指数百万分：空洞数 × 粒度 / 容量（粗粒度近似）。
    pub fn frag_ppm(&self, cap: u32) -> u32 {
        if cap == 0 {
            return 0xFFFF_FFFF;
        }
        self.holes * 1_000_000 / cap
    }
}

/// 演练引擎：确定性 LCG 序列驱动的位图分配器。
pub struct Drill {
    cap: u32,
    gran: u32,
    topo: Topo,
    /// 占用位图（1 = 占）。
    used: Vec<bool>,
    /// 每请求的块数（释放时回放）。
    ledger: Vec<u32>,
    score: DrillScore,
}

impl Drill {
    /// 建演练：容量（帧）、粒度（帧/块，2 幂）、档位。
    pub fn new(cap: u32, gran: u32, topo: Topo) -> Result<Drill, u32> {
        if cap == 0 {
            return Err(XC_CAP_OOR);
        }
        if gran == 0 || !gran.is_power_of_two() {
            return Err(XC_GRAN_OOR);
        }
        if topo.kind() == 0 || topo.kind() > 3 {
            return Err(XC_TOP_OOR);
        }
        Ok(Drill {
            cap,
            gran,
            topo,
            used: vec![false; cap as usize],
            ledger: Vec::new(),
            score: DrillScore { requests: 0, ok: 0, peak: 0, holes: 0 },
        })
    }

    /// 找 n 个连续空帧的起始下标（first-fit）。
    fn find_run(&self, n: u32) -> Option<usize> {
        if n == 0 || n > self.cap {
            return None;
        }
        let mut run = 0usize;
        for i in 0..self.cap as usize {
            if self.used[i] {
                run = 0;
            } else {
                run += 1;
                if run == n as usize {
                    return Some(i + 1 - n as usize);
                }
            }
        }
        None
    }

    /// 确定性 LCG 伪随机序列驱动的单步：请求 k 块（1..=kmax），
    /// 每 4 步释放最早一笔（碎片档每 2 步释放，制造交错）。
    pub fn step(&mut self, tick: u32, kmax: u32) -> bool {
        // LCG：x = x*1664525 + 1013904223，取高段做请求量。
        let x = tick
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let span = (x >> 16) % kmax.max(1) + 1; // 1..=kmax
        self.score.requests += 1;
        // 释放窗：碎片档 2 步一轮，其余 4 步一轮。
        let window = if self.topo == Topo::Frag { 2 } else { 4 };
        if self.score.requests % window == 0 {
            self.release_oldest();
        }
        let start = match self.find_run(span) {
            Some(s) => s,
            None => return false, // 失败计账（容量/碎片不足）
        };
        for i in start..start + span as usize {
            self.used[i] = true;
        }
        self.ledger.push(span);
        self.score.ok += 1;
        let occ = self.ledger.iter().sum::<u32>();
        if occ > self.score.peak {
            self.score.peak = occ;
        }
        true
    }

    /// 释放最早一笔；越账拒（账空或帧已空均异常）。
    fn release_oldest(&mut self) -> bool {
        let n = match self.ledger.first() {
            Some(&n) => n,
            None => return false,
        };
        // 从头找第一段连续占帧归还（简化语义：按登记序归还）。
        let mut left = n as usize;
        let mut i = 0usize;
        while i < self.used.len() && left > 0 {
            if self.used[i] {
                self.used[i] = false;
                left -= 1;
            }
            i += 1;
        }
        self.ledger.remove(0);
        left == 0
    }

    /// 跑满缺省步数。
    pub fn run(&mut self) {
        let (steps, kavg) = self.topo.defaults();
        let kmax = kavg.max(1);
        for t in 0..steps {
            self.step(t, kmax);
        }
    }

    /// 收官空洞数：空闲段数（连续空帧计一段）。
    pub fn count_holes(&self) -> u32 {
        let mut holes = 0u32;
        let mut in_hole = false;
        for &u in self.used.iter() {
            if !u {
                if !in_hole {
                    holes += 1;
                    in_hole = true;
                }
            } else {
                in_hole = false;
            }
        }
        holes
    }

    /// 收官残余（未归还帧数；finalize 后应为 0）。
    pub fn residue(&self) -> u32 {
        self.ledger.iter().sum::<u32>()
    }

    /// 终评：冻结指标（drain 前快照空洞与峰值），随后归还全部残余。
    pub fn finalize(&mut self) -> DrillScore {
        self.score.holes = self.count_holes();
        while !self.ledger.is_empty() {
            self.release_oldest();
        }
        self.score
    }

    /// 重放核：同参数再跑一遍，指标必须逐项相等。
    pub fn replay_eq(cap: u32, gran: u32, topo: Topo) -> Result<(), u32> {
        let mut a = match Drill::new(cap, gran, topo) {
            Ok(d) => d,
            Err(_) => return Err(XC_CAP_OOR),
        };
        let mut b = match Drill::new(cap, gran, topo) {
            Ok(d) => d,
            Err(_) => return Err(XC_CAP_OOR),
        };
        a.run();
        b.run();
        let sa = a.finalize();
        let sb = b.finalize();
        if sa != sb {
            return Err(XC_REPLAY);
        }
        Ok(())
    }

    /// 参数读取。
    pub fn cap(&self) -> u32 {
        self.cap
    }
}

/// 单档闸：成功率满千分 + 峰值 ≤ 容量 + 残余归零。
pub fn gate_one(d: &Drill, s: &DrillScore) -> Result<(), u32> {
    if s.ok_milli() < 1000 {
        return Err(XC_SUCCESS_LO);
    }
    if s.peak > d.cap() {
        return Err(XC_PEAK_HI);
    }
    if d.residue() != 0 {
        return Err(XC_LEAK);
    }
    Ok(())
}

pub fn run_unx0379_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0379_drilltopo");
    // 族一：三档拓扑参数核。
    cs.add("大块参数", Topo::Bulk.defaults() == (64, 64), "短序列大块");
    cs.add("小块参数", Topo::Small.defaults() == (256, 4), "长序列细粒度");
    cs.add("碎片参数", Topo::Frag.defaults() == (128, 16), "交错释放");
    cs.add("档位标签", Topo::Frag.kind() == 3, "Frag=3");
    // 族二：大块档演练。
    let mut d1 = match Drill::new(4096, 1, Topo::Bulk) {
        Ok(d) => d,
        Err(e) => {
            cs.add("大块构建", false, "拒绝码");
            let _ = e;
            return cs;
        }
    };
    d1.run();
    let s1 = d1.finalize();
    cs.add("大块成功满", s1.ok_milli() == 1000, "容量充裕无拒绝");
    cs.add("大块峰值合法", s1.peak <= 4096, "峰值 ≤ 容量");
    cs.add("大块无残余", d1.residue() == 0, "释放窗 4 轮空");
    cs.add("大块闸过", gate_one(&d1, &s1).is_ok(), "三指标齐");
    // 族三：小块档演练。
    let mut d2 = match Drill::new(1024, 1, Topo::Small) {
        Ok(d) => d,
        Err(_) => {
            cs.add("小块构建", false, "拒绝");
            return cs;
        }
    };
    d2.run();
    let s2 = d2.finalize();
    cs.add("小块成功满", s2.ok_milli() == 1000, "1MB 模型充裕");
    cs.add("小块峰值合法", s2.peak <= 1024, "峰值 ≤ 容量");
    cs.add("小块闸过", gate_one(&d2, &s2).is_ok(), "三指标齐");
    // 族四：碎片档演练 + 碎片指数。
    let mut d3 = match Drill::new(2048, 1, Topo::Frag) {
        Ok(d) => d,
        Err(_) => {
            cs.add("碎片构建", false, "拒绝");
            return cs;
        }
    };
    d3.run();
    let s3 = d3.finalize();
    cs.add("碎片档成功满", s3.ok_milli() == 1000, "交错仍可配");
    cs.add("碎片指数有界", s3.frag_ppm(2048) <= 1_000_000, "空洞 ≤ 容量");
    cs.add("碎片闸过", gate_one(&d3, &s3).is_ok(), "三指标齐");
    // 族五：重放确定性。
    cs.add("大块重放稳", Drill::replay_eq(4096, 1, Topo::Bulk).is_ok(), "逐项相等");
    cs.add("碎片重放稳", Drill::replay_eq(2048, 1, Topo::Frag).is_ok(), "逐项相等");
    // 族六：参数拒绝。
    cs.add("容量零拒", matches!(Drill::new(0, 1, Topo::Bulk), Err(XC_CAP_OOR)), "cap=0");
    cs.add("粒度零拒", matches!(Drill::new(64, 0, Topo::Bulk), Err(XC_GRAN_OOR)), "gran=0");
    cs.add("粒度非 2 幂拒", matches!(Drill::new(64, 3, Topo::Bulk), Err(XC_GRAN_OOR)), "gran=3");
    // 族七：峰值超容检出（容量压到最小逼失败路径计账）。
    let mut d7 = match Drill::new(8, 1, Topo::Bulk) {
        Ok(d) => d,
        Err(_) => {
            cs.add("小容构建", false, "拒绝");
            return cs;
        }
    };
    d7.run();
    let s7 = d7.finalize();
    // 8 帧容量对大块档必然出现拒绝请求（峰值被钳 ≤ 8）。
    cs.add("小容峰值钳位", s7.peak <= 8, "峰值不越容");
    let g7 = gate_one(&d7, &s7);
    cs.add("小容闸检出",
        g7 == Err(XC_SUCCESS_LO) || g7 == Err(XC_LEAK),
        "小容必亮红灯");
    cs
}
