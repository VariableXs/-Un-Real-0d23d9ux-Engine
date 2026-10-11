//! cgg12_nvbench — CGPU-F0972 NVIDIA 基准（SKU 成绩公开+NVENC 吞吐+DLSS 预留）。
//!
//! 判据原文（施工书锚点 CGPU-F0972）：基准：NVIDIA 代表 SKU 基准数据（各代
//! D08 考卷成绩公开）；NVENC 吞吐；DLSS 场景预留数据——入册回归。判据：三
//! 基准、公开、入册、判据。
//!
//! 职责定位：SKU 成绩公开账（各代代表 SKU 的 D08 考卷成绩逐代公开登记——
//! 代缺成绩 0x7E01，公开=账面可查非口说）+NVENC 吞吐基准（编码吞吐红线
//! 0x7E02）+DLSS 场景预留数据位（预留=显性 Empty 不造假——填假数据 0x7E03，
//! 与 vck04 空态语义同源）+入册回归（同口径复跑漂移 0x7E04；口径版本防挑
//! 参 0x7E05）。诊断码段 0x7E01~0x7E05。

use alloc::vec::Vec;

/// 模块版本。
pub const NV_BENCH_VERSION: &str = "G12-nvbench-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x7E 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7E01 SKU 成绩缺（代际成绩未公开登记）。
pub const XC_SKU_SCORE_MISS: u32 = 0x7E01;
/// 0x7E02 NVENC 吞吐红线。
pub const XC_NVENC_TPUT_BREACH: u32 = 0x7E02;
/// 0x7E03 DLSS 预留数据失真（预留位填假数据）。
pub const XC_DLSS_RESERVE_FAKE: u32 = 0x7E03;
/// 0x7E04 回归漂移。
pub const XC_NV_REG_DRIFT: u32 = 0x7E04;
/// 0x7E05 口径作弊。
pub const XC_NV_CHERRY_PICK: u32 = 0x7E05;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// NVIDIA 代际闭集（六代——与 cgg11 白皮书同源）。
pub const NV_SKU_GENS: usize = 6;
/// NVENC 吞吐红线（编码会话/秒下限）。
pub const NVENC_TPUT_FLOOR: u32 = 8;
/// 回归样本数。
pub const NV_REG_RUNS: usize = 3;
/// 口径版本。
pub const NV_BENCH_PROFILE_VER: u32 = 0x4E42_0001; // "NB"1

// ---------------------------------------------------------------------------
// 一、SKU 成绩公开账
// ---------------------------------------------------------------------------

/// SKU 成绩行。
#[derive(Clone, Copy, Debug)]
pub struct SkuScore {
    /// 代际序（0..=5）。
    pub gen: usize,
    /// D08 考卷成绩（归一化万分比）。
    pub d08_score_bp: u32,
    /// 公开标记（成绩公开=账面可查）。
    pub published: bool,
}

/// SKU 成绩公开账（判据"各代 D08 考卷成绩公开"）。
#[derive(Debug)]
pub struct SkuScoreBoard {
    /// 成绩表（六代闭集）。
    pub scores: Vec<SkuScore>,
    /// 成绩缺事件数（0x7E01）。
    pub miss_events: u32,
}

impl SkuScoreBoard {
    /// 新账。
    pub const fn new() -> SkuScoreBoard {
        SkuScoreBoard {
            scores: Vec::new(),
            miss_events: 0,
        }
    }

    /// 登记成绩（未公开的成绩不入账——公开语义显性）。
    pub fn publish(&mut self, s: SkuScore) -> Result<(), u32> {
        if s.gen >= NV_SKU_GENS || !s.published || s.d08_score_bp > 10_000 {
            self.miss_events += 1;
            return Err(XC_SKU_SCORE_MISS);
        }
        // 同代覆盖（新成绩替换旧成绩——最新公开数据说话）。
        let mut i = 0;
        while i < self.scores.len() {
            if self.scores[i].gen == s.gen {
                self.scores[i] = s;
                return Ok(());
            }
            i += 1;
        }
        self.scores.push(s);
        Ok(())
    }

    /// 完备裁决：六代成绩全公开（0x7E01）。
    pub fn complete(&mut self) -> Result<(), u32> {
        if self.scores.len() < NV_SKU_GENS {
            self.miss_events += 1;
            return Err(XC_SKU_SCORE_MISS);
        }
        Ok(())
    }

    /// 成绩查询（公开账面）。
    pub fn score_of(&self, gen: usize) -> Option<u32> {
        let mut i = 0;
        while i < self.scores.len() {
            if self.scores[i].gen == gen {
                return Some(self.scores[i].d08_score_bp);
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 二、NVENC 吞吐基准
// ---------------------------------------------------------------------------

/// NVENC 吞吐基准（判据"NVENC 吞吐"）。
#[derive(Debug)]
pub struct NvencThroughputBench {
    /// 口径版本。
    pub profile_ver: u32,
    /// 最近实测吞吐（会话/秒）。
    pub last_tput: u32,
    /// 超红事件数（0x7E02）。
    pub breach_events: u32,
}

impl NvencThroughputBench {
    /// 新基准。
    pub const fn new() -> NvencThroughputBench {
        NvencThroughputBench {
            profile_ver: NV_BENCH_PROFILE_VER,
            last_tput: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮（sessions 为编码会话数，elapsed_ms 为时长）。
    pub fn run(&mut self, sessions: u32, elapsed_ms: u32) -> Result<u32, u32> {
        if self.profile_ver != NV_BENCH_PROFILE_VER {
            self.breach_events += 1;
            return Err(XC_NV_CHERRY_PICK);
        }
        if elapsed_ms == 0 {
            self.breach_events += 1;
            return Err(XC_NVENC_TPUT_BREACH);
        }
        let tput = (sessions as u64 * 1000 / elapsed_ms as u64) as u32;
        self.last_tput = tput;
        if tput < NVENC_TPUT_FLOOR {
            self.breach_events += 1;
            return Err(XC_NVENC_TPUT_BREACH);
        }
        Ok(tput)
    }
}

// ---------------------------------------------------------------------------
// 三、DLSS 场景预留数据位
// ---------------------------------------------------------------------------

/// DLSS 预留位（判据"DLSS 场景预留数据"——显性 Empty 不造假）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlssReserve {
    /// 显性空态（数据未采——预留）。
    Empty,
    /// 已采数据（帧率万分比增益）。
    Measured(u32),
}

/// 预留位簿。
#[derive(Debug)]
pub struct DlssReserveBook {
    /// 预留位（六代各一）。
    pub slots: [DlssReserve; NV_SKU_GENS],
    /// 失真事件数（0x7E03）。
    pub fake_events: u32,
}

impl DlssReserveBook {
    /// 新簿（全 Empty——预留显性）。
    pub const fn new() -> DlssReserveBook {
        DlssReserveBook {
            slots: [DlssReserve::Empty; NV_SKU_GENS],
            fake_events: 0,
        }
    }

    /// 采数入位。
    pub fn measure(&mut self, gen: usize, gain_bp: u32) -> Result<(), u32> {
        if gen >= NV_SKU_GENS {
            self.fake_events += 1;
            return Err(XC_DLSS_RESERVE_FAKE);
        }
        self.slots[gen] = DlssReserve::Measured(gain_bp);
        Ok(())
    }

    /// 失真裁决：Measured(0)=假数据（采了等于没采还不承认空态）→0x7E03。
    pub fn audit(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < NV_SKU_GENS {
            if self.slots[i] == DlssReserve::Measured(0) {
                self.fake_events += 1;
                return Err(XC_DLSS_RESERVE_FAKE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 空态诚实：Empty 位查询显性返回 None（不编造增益）。
    pub fn query(&self, gen: usize) -> Option<u32> {
        if gen >= NV_SKU_GENS {
            return None;
        }
        match self.slots[gen] {
            DlssReserve::Empty => None,
            DlssReserve::Measured(g) => Some(g),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、入册回归
// ---------------------------------------------------------------------------

/// 回归账（判据"入册回归"）。
#[derive(Debug)]
pub struct NvBenchRegression {
    /// 口径版本。
    pub profile_ver: u32,
    /// 样本册（成绩/NVENC/DLSS 三槽）。
    pub samples_score: Vec<u32>,
    pub samples_nvenc: Vec<u32>,
    /// 漂移事件数（0x7E04）。
    pub drift_events: u32,
    /// 作弊事件数（0x7E05）。
    pub cheat_events: u32,
}

impl NvBenchRegression {
    /// 新账。
    pub const fn new() -> NvBenchRegression {
        NvBenchRegression {
            profile_ver: NV_BENCH_PROFILE_VER,
            samples_score: Vec::new(),
            samples_nvenc: Vec::new(),
            drift_events: 0,
            cheat_events: 0,
        }
    }

    /// 入册。
    pub fn enroll(&mut self, slot: usize, val: u32) -> Result<(), u32> {
        if self.profile_ver != NV_BENCH_PROFILE_VER {
            self.cheat_events += 1;
            return Err(XC_NV_CHERRY_PICK);
        }
        match slot {
            0 => self.samples_score.push(val),
            1 => self.samples_nvenc.push(val),
            _ => {
                self.cheat_events += 1;
                return Err(XC_NV_CHERRY_PICK);
            }
        }
        Ok(())
    }

    /// 回归裁决（漂移 >10% 立案）。
    pub fn adjudicate(&mut self, slot: usize) -> Result<(), u32> {
        let (n, first, last) = match slot {
            0 => (self.samples_score.len(), self.samples_score.first().copied(), self.samples_score.last().copied()),
            1 => (self.samples_nvenc.len(), self.samples_nvenc.first().copied(), self.samples_nvenc.last().copied()),
            _ => {
                self.cheat_events += 1;
                return Err(XC_NV_CHERRY_PICK);
            }
        };
        if n < NV_REG_RUNS {
            return Ok(());
        }
        match (first, last) {
            (Some(f), Some(l)) => {
                if f == 0 {
                    if l != 0 {
                        self.drift_events += 1;
                        return Err(XC_NV_REG_DRIFT);
                    }
                    return Ok(());
                }
                let diff = if l > f { l - f } else { f - l };
                if diff * 10 > f {
                    self.drift_events += 1;
                    return Err(XC_NV_REG_DRIFT);
                }
                Ok(())
            }
            _ => {
                self.drift_events += 1;
                Err(XC_NV_REG_DRIFT)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（三基准/回归/判据三组+段对账）
// ---------------------------------------------------------------------------

/// NVIDIA 基准总引擎。
#[derive(Debug)]
pub struct NvBenchEngine {
    /// SKU 成绩公开账。
    pub board: SkuScoreBoard,
    /// NVENC 吞吐基准。
    pub nvenc: NvencThroughputBench,
    /// DLSS 预留位簿。
    pub dlss: DlssReserveBook,
    /// 回归账。
    pub reg: NvBenchRegression,
}

impl NvBenchEngine {
    /// 出厂。
    pub const fn new() -> NvBenchEngine {
        NvBenchEngine {
            board: SkuScoreBoard::new(),
            nvenc: NvencThroughputBench::new(),
            dlss: DlssReserveBook::new(),
            reg: NvBenchRegression::new(),
        }
    }

    /// 组一：三基准——成绩账缺代显性；未公开拒；NVENC 红线；DLSS 空态诚实。
    pub fn selfcheck_benches(&mut self) -> Result<(), u32> {
        let mut e = NvBenchEngine::new();
        // 成绩缺代→0x7E01。
        if e.board.complete() != Err(XC_SKU_SCORE_MISS) {
            return Err(XC_SKU_SCORE_MISS);
        }
        // 未公开成绩拒入账。
        if e.board.publish(SkuScore { gen: 0, d08_score_bp: 8_000, published: false })
            != Err(XC_SKU_SCORE_MISS)
        {
            return Err(XC_SKU_SCORE_MISS);
        }
        // 六代成绩全公开。
        let mut g = 0;
        while g < NV_SKU_GENS {
            e.board.publish(SkuScore {
                gen: g,
                d08_score_bp: 7_000 + g as u32 * 300,
                published: true,
            })?;
            g += 1;
        }
        e.board.complete()?;
        // NVENC：80 会话/1000ms=80 过；7 会话/1000ms=7 红。
        e.nvenc.run(80, 1_000)?;
        if e.nvenc.run(7, 1_000) != Err(XC_NVENC_TPUT_BREACH) {
            return Err(XC_NVENC_TPUT_BREACH);
        }
        // DLSS：空态查询 None（不造假）。
        if e.dlss.query(2).is_some() {
            return Err(XC_DLSS_RESERVE_FAKE);
        }
        e.dlss.measure(2, 4_500)?;
        if e.dlss.query(2) != Some(4_500) {
            return Err(XC_DLSS_RESERVE_FAKE);
        }
        // 假数据（Measured(0)）→0x7E03。
        e.dlss.slots[3] = DlssReserve::Measured(0);
        if e.dlss.audit() != Err(XC_DLSS_RESERVE_FAKE) {
            return Err(XC_DLSS_RESERVE_FAKE);
        }
        Ok(())
    }

    /// 组二：口径防作弊。
    pub fn selfcheck_antipick(&mut self) -> Result<(), u32> {
        let mut e = NvBenchEngine::new();
        e.nvenc.profile_ver = 0xDEAD;
        if e.nvenc.run(80, 1_000) != Err(XC_NV_CHERRY_PICK) {
            return Err(XC_NV_CHERRY_PICK);
        }
        if e.reg.enroll(9, 1) != Err(XC_NV_CHERRY_PICK) {
            return Err(XC_NV_CHERRY_PICK);
        }
        e.reg.profile_ver = 0xBEEF;
        if e.reg.enroll(0, 1) != Err(XC_NV_CHERRY_PICK) {
            return Err(XC_NV_CHERRY_PICK);
        }
        if e.reg.cheat_events != 2 {
            return Err(XC_NV_CHERRY_PICK);
        }
        Ok(())
    }

    /// 组三：回归——一致过/漂移立案。
    pub fn selfcheck_regression(&mut self) -> Result<(), u32> {
        let mut e = NvBenchEngine::new();
        e.reg.enroll(0, 7_000)?;
        e.reg.adjudicate(0)?;
        e.reg.enroll(0, 7_000)?;
        e.reg.enroll(0, 7_100)?;
        e.reg.enroll(0, 7_200)?;
        e.reg.adjudicate(0)?;
        // 漂移：7200 → 8500（18%）。
        e.reg.enroll(0, 7_200)?;
        e.reg.enroll(0, 7_200)?;
        e.reg.enroll(0, 8_500)?;
        if e.reg.adjudicate(0) != Err(XC_NV_REG_DRIFT) {
            return Err(XC_NV_REG_DRIFT);
        }
        if e.reg.drift_events != 1 {
            return Err(XC_NV_REG_DRIFT);
        }
        Ok(())
    }

    /// 组四：判据——端到端：成绩公开+NVENC+DLSS 诚实+回归齐绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = NvBenchEngine::new();
        let mut g = 0;
        while g < NV_SKU_GENS {
            let s = 7_000 + g as u32 * 300;
            e.board.publish(SkuScore { gen: g, d08_score_bp: s, published: true })?;
            e.reg.enroll(0, s)?;
            g += 1;
        }
        e.board.complete()?;
        e.reg.adjudicate(0)?;
        let mut i = 0;
        while i < NV_REG_RUNS {
            let t = e.nvenc.run(80, 1_000)?;
            e.reg.enroll(1, t)?;
            i += 1;
        }
        e.reg.adjudicate(1)?;
        // 全链零事件。
        if e.board.miss_events != 0
            || e.nvenc.breach_events != 0
            || e.dlss.fake_events != 0
            || e.reg.drift_events != 0
            || e.reg.cheat_events != 0
        {
            return Err(XC_SKU_SCORE_MISS);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_benches()?;
        self.selfcheck_antipick()?;
        self.selfcheck_regression()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7E00..0x7F00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SKU_SCORE_MISS,
            XC_NVENC_TPUT_BREACH,
            XC_DLSS_RESERVE_FAKE,
            XC_NV_REG_DRIFT,
            XC_NV_CHERRY_PICK,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7E00 || codes[i] >= 0x7F00 {
                return Err(XC_SKU_SCORE_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SKU_SCORE_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
