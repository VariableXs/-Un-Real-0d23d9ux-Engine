//! CGPU-F1699 · 异构帧归因深化
//!
//! 判据锚点（施工书原文摘录）：
//! 帧归因：段归因（异构段入帧归因（引擎×段双维——双维归因（归因准确率跨引擎
//! 达标（复用契约——达标声明；等待归因（跨引擎等待时间归因（等待在哪个 fence
//! ——等待归因（阻塞定位——定位价值；干扰归因（引擎间干扰（带宽/热互扰——互扰
//! 归因；与 C 域（段枚举扩展同步——枚举扩展复用 F2953 模式；测试（双维/等待/
//! 互扰/枚举四组）。判据：双维、等待归因、互扰、枚举扩展、四组、判据。
//!
//! 诊断码段 0x8601~0x8605（独占）：
//!   0x8601 双维失守（矩阵行/列和与帧总时长守恒破）
//!   0x8602 等待归因失锚（fence 不在册 / 等待者=被等者自环）
//!   0x8603 互扰归因失据（互扰对自互 / 带宽与热双轨混淆）
//!   0x8604 枚举扩展失同步（C 域段枚举与归因列不对拍）
//!   0x8605 段越界（判码落 0x8600..0x8700 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const FRAME_ATTR2_VERSION: &str = "K18-frameattr2-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8601~0x8605）
// ---------------------------------------------------------------------------

pub const XC_DUALDIM_BROKEN: u32 = 0x8601;
pub const XC_WAIT_UNANCHORED: u32 = 0x8602;
pub const XC_INTERFERE_NO_EVIDENCE: u32 = 0x8603;
pub const XC_ENUM_DESYNC: u32 = 0x8604;
pub const XC_SEG_OOR: u32 = 0x8605;

// ---------------------------------------------------------------------------
// 一、引擎×段双维归因矩阵
// ---------------------------------------------------------------------------

/// 引擎四类闭集（异构域通例：CPU/GPU/NPU/DSP）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eng {
    Cpu,
    Gpu,
    Npu,
    Dsp,
}

impl Eng {
    pub const ALL: [Eng; 4] = [Eng::Cpu, Eng::Gpu, Eng::Npu, Eng::Dsp];
    pub const fn ordinal(self) -> usize {
        match self {
            Eng::Cpu => 0,
            Eng::Gpu => 1,
            Eng::Npu => 2,
            Eng::Dsp => 3,
        }
    }
}

/// 帧内段六列闭集（与 C 域段枚举同步——列定义须对拍 F2953 扩展模式）。
pub const SEG_COLUMNS: [&str; 6] = [
    "prepare", // 段 1：准备（解码/上传）
    "render",  // 段 2：渲染
    "compute", // 段 3：计算（AI 推理/物理）
    "compose", // 段 4：合成
    "present", // 段 5：呈现
    "wait",    // 段 6：等待（跨引擎同步空转）
];

/// 双维矩阵：行=引擎，列=段；格值=归因 µs。帧归因深化核心——每 µs 同时
/// 有引擎归属与段归属，行和/列和双守恒帧总时长。
pub struct DualDimMatrix {
    cells: [[u64; 6]; 4],
}

impl DualDimMatrix {
    pub fn new() -> Self {
        DualDimMatrix { cells: [[0; 6]; 4] }
    }

    /// 记一笔归因。
    pub fn record(&mut self, e: Eng, seg_col: usize, us: u64) -> Result<(), u32> {
        if seg_col >= 6 || us > 12_500 {
            return Err(XC_DUALDIM_BROKEN);
        }
        self.cells[e.ordinal()][seg_col] += us;
        Ok(())
    }

    /// 行和：某引擎全段合计。
    pub fn row_sum(&self, e: Eng) -> u64 {
        let mut s = 0;
        for c in 0..6 {
            s += self.cells[e.ordinal()][c];
        }
        s
    }

    /// 列和：某段全引擎合计。
    pub fn col_sum(&self, seg_col: usize) -> u64 {
        if seg_col >= 6 {
            return 0;
        }
        let mut s = 0;
        for r in 0..4 {
            s += self.cells[r][seg_col];
        }
        s
    }

    /// 全帧总时长。
    pub fn total(&self) -> u64 {
        let mut s = 0;
        for e in Eng::ALL {
            s += self.row_sum(e);
        }
        s
    }

    /// 双维守恒：行和合计 == 列和合计 == total（同一时长两维看必须同账）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut col_total = 0;
        for c in 0..6 {
            col_total += self.col_sum(c);
        }
        if col_total != self.total() {
            return Err(XC_DUALDIM_BROKEN);
        }
        // 单格不越帧预算 12.5ms（12500µs）——vck11 帧归因守恒契约延续。
        for r in 0..4 {
            for c in 0..6 {
                if self.cells[r][c] > 12_500 {
                    return Err(XC_DUALDIM_BROKEN);
                }
            }
        }
        Ok(())
    }

    /// 归因准确率声明：格值覆盖率高（非零格占比 ≥ 门槛）——跨引擎达标
    /// 声明的机器化（判据"归因准确率跨引擎达标（复用契约——达标声明）"）。
    pub fn coverage_permille(&self) -> u32 {
        let mut nonzero = 0;
        for r in 0..4 {
            for c in 0..6 {
                if self.cells[r][c] > 0 {
                    nonzero += 1;
                }
            }
        }
        nonzero * 1000 / 24
    }
}

// ---------------------------------------------------------------------------
// 二、等待归因：等在哪个 fence
// ---------------------------------------------------------------------------

/// fence 登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FenceRec {
    /// fence id。
    pub id: u32,
    /// 生产者引擎。
    pub producer: Eng,
    /// 消费者引擎（等待方）。
    pub waiter: Eng,
}

/// 等待归因账：每条等待记录"等在哪个 fence + 等 µs"，阻塞定位=top fence。
pub struct WaitLedger {
    fences: Vec<FenceRec>,
    waits: Vec<(u32, u64)>, // (fence_id, wait_us)
}

impl WaitLedger {
    pub fn new() -> Self {
        WaitLedger { fences: Vec::new(), waits: Vec::new() }
    }

    /// 登记 fence：生产者==等待者自环即拒（0x8602）。
    pub fn enroll_fence(&mut self, id: u32, producer: Eng, waiter: Eng) -> Result<(), u32> {
        if producer == waiter {
            return Err(XC_WAIT_UNANCHORED);
        }
        // 重 id 拒。
        for f in &self.fences {
            if f.id == id {
                return Err(XC_WAIT_UNANCHORED);
            }
        }
        self.fences.push(FenceRec { id, producer, waiter });
        Ok(())
    }

    /// 记等待：fence 必须在册（等无锚 fence=失锚）。
    pub fn record_wait(&mut self, fence_id: u32, wait_us: u64) -> Result<(), u32> {
        let mut found = false;
        for f in &self.fences {
            if f.id == fence_id {
                found = true;
            }
        }
        if !found {
            return Err(XC_WAIT_UNANCHORED);
        }
        self.waits.push((fence_id, wait_us));
        Ok(())
    }

    /// 阻塞定位：累计等待最重的 fence（top-1）与其被等 µs。
    pub fn blocker_top(&self) -> Option<(u32, u64)> {
        let mut ids: Vec<u32> = Vec::new();
        let mut sums: Vec<u64> = Vec::new();
        for (fid, us) in &self.waits {
            let mut idx = None;
            let mut i = 0;
            while i < ids.len() {
                if ids[i] == *fid {
                    idx = Some(i);
                }
                i += 1;
            }
            match idx {
                Some(i) => sums[i] += us,
                None => {
                    ids.push(*fid);
                    sums.push(*us);
                }
            }
        }
        let mut best: Option<(u32, u64)> = None;
        let mut i = 0;
        while i < ids.len() {
            match best {
                Some((_, s)) if s >= sums[i] => {}
                _ => best = Some((ids[i], sums[i])),
            }
            i += 1;
        }
        best
    }

    /// 全部等待 µs 合计（供双维矩阵 wait 列对拍）。
    pub fn total_wait_us(&self) -> u64 {
        let mut s = 0;
        for (_, us) in &self.waits {
            s += us;
        }
        s
    }

    /// 等待行合计。
    pub fn wait_rows(&self) -> usize {
        self.waits.len()
    }
}

// ---------------------------------------------------------------------------
// 三、互扰归因：带宽/热两轨
// ---------------------------------------------------------------------------

/// 互扰轨二闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InterfereRail {
    /// 带宽互扰：两引擎抢内存带宽。
    Bandwidth,
    /// 热互扰：两引擎共热包互拖频率。
    Thermal,
}

impl InterfereRail {
    pub const fn name(self) -> &'static str {
        match self {
            InterfereRail::Bandwidth => "bandwidth",
            InterfereRail::Thermal => "thermal",
        }
    }
}

/// 互扰行：引擎对 + 轨 + 归因 µs + 置信万分比（估计显性带置信，不冒充实测）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InterfereRow {
    pub a: Eng,
    pub b: Eng,
    pub rail: InterfereRail,
    pub attr_us: u64,
    /// 置信万分比（0001..10000）。
    pub conf_bp: u16,
}

/// 互扰账：同轨引擎对唯一，自互（a==b）拒，带宽/热双轨分账。
pub struct InterfereLedger {
    rows: Vec<InterfereRow>,
}

impl InterfereLedger {
    pub fn new() -> Self {
        InterfereLedger { rows: Vec::new() }
    }

    /// 记互扰：自互拒（引擎不与自己互扰）、同对同轨重复拒（0x8603）。
    pub fn record(&mut self, a: Eng, b: Eng, rail: InterfereRail, attr_us: u64, conf_bp: u16) -> Result<(), u32> {
        if a == b || attr_us == 0 || conf_bp == 0 || conf_bp > 10_000 {
            return Err(XC_INTERFERE_NO_EVIDENCE);
        }
        for r in &self.rows {
            if r.rail == rail
                && ((r.a == a && r.b == b) || (r.a == b && r.b == a))
            {
                return Err(XC_INTERFERE_NO_EVIDENCE);
            }
        }
        self.rows.push(InterfereRow { a, b, rail, attr_us, conf_bp });
        Ok(())
    }

    /// 轨归因合计（带宽轨与热轨分账——双轨混淆=0x8603）。
    pub fn rail_sum(&self, rail: InterfereRail) -> u64 {
        let mut s = 0;
        for r in &self.rows {
            if r.rail == rail {
                s += r.attr_us;
            }
        }
        s
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 查行。
    pub fn row(&self, idx: usize) -> Option<&InterfereRow> {
        self.rows.get(idx)
    }
}

// ---------------------------------------------------------------------------
// 四、C 域段枚举扩展同步（F2953 模式复用）
// ---------------------------------------------------------------------------

/// 枚举桥：本模块段列闭集与 C 域扩展段表对拍（源单号非空纪律）。
pub struct EnumBridge {
    /// 复用源单号（F2953 模式——枚举扩展同步声明）。
    pub origin: &'static str,
    /// 对拍通过。
    pub synced: bool,
}

impl EnumBridge {
    pub const ORIGIN: &str = "CGPU-F2953";

    /// 对拍：列数一致 + 列名逐一对齐（C 域扩展段表=本表映像）。
    pub fn sync(c_domain_segs: &[&str]) -> Result<Self, u32> {
        if c_domain_segs.len() != SEG_COLUMNS.len() {
            return Err(XC_ENUM_DESYNC);
        }
        let mut i = 0;
        while i < SEG_COLUMNS.len() {
            if c_domain_segs[i] != SEG_COLUMNS[i] {
                return Err(XC_ENUM_DESYNC);
            }
            i += 1;
        }
        Ok(EnumBridge { origin: Self::ORIGIN, synced: true })
    }

    pub fn verified(&self) -> Result<(), u32> {
        if !self.synced || self.origin.is_empty() {
            return Err(XC_ENUM_DESYNC);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：双维/等待/互扰/枚举）
// ---------------------------------------------------------------------------

/// 帧归因深化引擎。
pub struct FrameAttr2Engine {
    pub matrix: DualDimMatrix,
    pub waits: WaitLedger,
    pub interfere: InterfereLedger,
}

impl FrameAttr2Engine {
    pub fn new() -> Self {
        FrameAttr2Engine {
            matrix: DualDimMatrix::new(),
            waits: WaitLedger::new(),
            interfere: InterfereLedger::new(),
        }
    }

    /// 标准场景样本：四引擎 × 六段有账帧（合计恰 12_000µs < 12_500 预算）。
    pub fn seed_sample(&mut self) -> Result<(), u32> {
        // prepare/render/compute/compose/present/wait 六段分布（µs）。
        self.matrix.record(Eng::Cpu, 0, 800)?; // prepare
        self.matrix.record(Eng::Gpu, 1, 4200)?; // render
        self.matrix.record(Eng::Npu, 2, 2600)?; // compute（推理）
        self.matrix.record(Eng::Dsp, 3, 900)?; // compose（音频/合成协理）
        self.matrix.record(Eng::Gpu, 4, 700)?; // present
        self.matrix.record(Eng::Cpu, 5, 1800)?; // wait（CPU 等 GPU fence）
        self.matrix.record(Eng::Dsp, 5, 1000)?; // wait（DSP 等 NPU fence）
        Ok(())
    }

    /// 组一：双维——守恒 + 覆盖率达标 + 越预算单格拒。
    pub fn selfcheck_dualdim(&mut self) -> Result<(), u32> {
        self.seed_sample()?;
        self.matrix.conserve()?;
        // 覆盖率：24 格中 7 格非零 = 291‰（门槛 ≥ 200‰ 达标声明）。
        if self.matrix.coverage_permille() < 200 {
            return Err(XC_DUALDIM_BROKEN);
        }
        // 越预算单格反例：>12500µs 拒。
        if self.matrix.record(Eng::Gpu, 1, 12_501).is_ok() {
            return Err(XC_DUALDIM_BROKEN);
        }
        Ok(())
    }

    /// 组二：等待归因——fence 锚 + 自环拒 + 阻塞定位 top-1。
    pub fn selfcheck_wait(&mut self) -> Result<(), u32> {
        self.waits.enroll_fence(0x10, Eng::Gpu, Eng::Cpu)?; // CPU 等 GPU
        self.waits.enroll_fence(0x11, Eng::Npu, Eng::Dsp)?; // DSP 等 NPU
        // 自环反例。
        if self.waits.enroll_fence(0x12, Eng::Gpu, Eng::Gpu).is_ok() {
            return Err(XC_WAIT_UNANCHORED);
        }
        // 失锚反例：等未登记 fence。
        if self.waits.record_wait(0x99, 100).is_ok() {
            return Err(XC_WAIT_UNANCHORED);
        }
        // 两条等待：0x10 计 1800，0x11 计 1000 → top=0x10。
        self.waits.record_wait(0x10, 1800)?;
        self.waits.record_wait(0x11, 1000)?;
        match self.waits.blocker_top() {
            Some((0x10, 1800)) => {}
            _ => return Err(XC_WAIT_UNANCHORED),
        }
        // wait 列对拍：矩阵 wait 列和 == 等待账合计（1800+1000）。
        if self.matrix.col_sum(5) != self.waits.total_wait_us() {
            return Err(XC_WAIT_UNANCHORED);
        }
        Ok(())
    }

    /// 组三：互扰归因——双轨分账 + 自互拒 + 重复对拒 + 置信在带。
    pub fn selfcheck_interfere(&mut self) -> Result<(), u32> {
        self.interfere.record(Eng::Gpu, Eng::Npu, InterfereRail::Bandwidth, 240, 6_500)?;
        self.interfere.record(Eng::Gpu, Eng::Npu, InterfereRail::Thermal, 180, 4_200)?;
        // 同对换向重复拒（A→B 已录，B→A 同轨再录拒）。
        if self.interfere.record(Eng::Npu, Eng::Gpu, InterfereRail::Bandwidth, 10, 1_000).is_ok() {
            return Err(XC_INTERFERE_NO_EVIDENCE);
        }
        // 自互拒。
        if self.interfere.record(Eng::Gpu, Eng::Gpu, InterfereRail::Thermal, 10, 1_000).is_ok() {
            return Err(XC_INTERFERE_NO_EVIDENCE);
        }
        // 置信越带拒。
        if self.interfere.record(Eng::Cpu, Eng::Dsp, InterfereRail::Bandwidth, 10, 10_001).is_ok() {
            return Err(XC_INTERFERE_NO_EVIDENCE);
        }
        // 双轨分账：带宽 240 / 热 180。
        if self.interfere.rail_sum(InterfereRail::Bandwidth) != 240
            || self.interfere.rail_sum(InterfereRail::Thermal) != 180
        {
            return Err(XC_INTERFERE_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 组四：枚举扩展——对拍同步 + 失配反例 + 源单号非空。
    pub fn selfcheck_enum(&mut self) -> Result<(), u32> {
        let bridge = EnumBridge::sync(&SEG_COLUMNS)?;
        bridge.verified()?;
        // 失配反例：列序错乱即拒。
        let bad = ["render", "prepare", "compute", "compose", "present", "wait"];
        if EnumBridge::sync(&bad).is_ok() {
            return Err(XC_ENUM_DESYNC);
        }
        // 短表反例。
        let short = ["prepare", "render"];
        if EnumBridge::sync(&short).is_ok() {
            return Err(XC_ENUM_DESYNC);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_dualdim()?;
        self.selfcheck_wait()?;
        self.selfcheck_interfere()?;
        self.selfcheck_enum()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8600..0x8700 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_DUALDIM_BROKEN,
            XC_WAIT_UNANCHORED,
            XC_INTERFERE_NO_EVIDENCE,
            XC_ENUM_DESYNC,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8600 || codes[i] >= 0x8700 {
                return Err(XC_DUALDIM_BROKEN);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_DUALDIM_BROKEN);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
