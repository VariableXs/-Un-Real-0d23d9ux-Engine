//! UNX-F12757 SEH 压测收官账（等式断言） · 0x82740段
//!
//! 判据原文（派工卡 12757 逐字锚定）：
//!   ① 纯功能代码 ≥306 行；② 判据原文：UNX-F12758-J1 B39 对齐框架登
//!   记（盘点/复核/冻结三段），衔接零悬空；③ 完成时 API/软件写回：状
//!   态=已完成、结果摘要非空。
//!   补充锚定：收官账等式 = Σ(矩阵格数) × 样本档 × 结果账三账对平；
//!   B39 框架三段登记齐 = 每段非空 + 引用链非空 + 零悬空。
//!
//! 设计要点：
//! 1. 三账对平引擎：矩阵账（F12741 压测矩阵四轴 3×3×4×3 = 108 格）、
//!    样本账（每格 4 档样本：基线/加压/峰值/恢复，逐档分账）、结果账
//!    （每档逐格落果）。收官账等式断言：矩阵格数 × 样本档 ≡ 样本账
//!    总条数 ≡ 结果账总条数，且逐档分账对平（每档 = 108）。
//! 2. B39 对齐框架登记：盘点/复核/冻结三段定容 3 节点，段序强制
//!    盘点→复核→冻结（先冻后盘显性拒 ERewind）；每节点文档非空
//!    （EEmpty）；引用链冻结段→复核段→盘点段→空，链断或指向未登记
//!    节点即悬空（ESegGap）；封账后写回显性拒。
//! 3. 错账注入器三族：族0 结果账漂移（结果账 +1 → 对平破缺）/
//!    族1 样本账缺口（少录一档一条 → 样本账≠矩阵×样本档）/
//!    族2 盘点缺口（少登一格 → 收官口 ESegGap）。每族 10 轮共 30 次
//!    注入全抓：注入数==检出数、漏检==0、逐族守恒。
//! 4. 判定次序冻结：格号越界 → 轴/档越闭集 → 段序倒退 → 段文档空 →
//!    格重复登记 → 段缺口/悬空 → 三账对平 → 正常收官。先到先判。
//! 5. 双跑一致：两次收官全流程（三账三元组 + B39 指纹）逐字段相等，
//!    FNV-1a 指纹全零视为异常。
//! 6. LCG 种子写死 = SEG_BASE（0x82740）；no_std + alloc；零浮点、零
//!    IO、零墙钟、零随机；取值一律 match/if let/.get()/.get_mut()/
//!    checked_*/saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12757 − 12600) = 0x82740）。
pub const SEG_BASE: u32 = 0x82740;

/// 矩阵格数（F12741 四轴闭集：深度 3 × 展开 3 × 码族 4 × 坏表 3）。
pub const MATRIX_CELLS: usize = 3 * 3 * 4 * 3;

/// 四轴档位（出处：F12741 压测矩阵登记，深度/展开/码族/坏表）。
pub const AXIS_DEPTH: usize = 3;
pub const AXIS_UNWIND: usize = 3;
pub const AXIS_FAMILY: usize = 4;
pub const AXIS_BADTABLE: usize = 3;

/// 样本档数（基线/加压/峰值/恢复，压测收官配档）。
pub const SAMPLE_TIERS: usize = 4;

/// 期望收官条数 = 108 格 × 4 档。
pub const EXPECTED_RESULTS: u32 = (MATRIX_CELLS as u32) * (SAMPLE_TIERS as u32);

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScDiag {
    /// 格号越界（不在 0..108）。
    ENoEnt,
    /// 轴/样本档越闭集。
    EBits,
    /// 状态/段序倒退。
    ERewind,
    /// 段文档空（B39 节点文档为空串）。
    EEmpty,
    /// 格重复登记（一格一主）。
    EDouble,
    /// 段缺口/引用悬空/盘点缺口。
    ESegGap,
    /// 错账（三账不对平）。
    ELedgerWrong,
    /// 容量耗尽。
    ECapFull,
    /// 注入漏检。
    EMissed,
    /// 双跑指纹不一致。
    EFp,
}

impl ScDiag {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ScDiag::ENoEnt => 0x01,
                ScDiag::EBits => 0x02,
                ScDiag::ERewind => 0x03,
                ScDiag::EEmpty => 0x04,
                ScDiag::EDouble => 0x05,
                ScDiag::ESegGap => 0x06,
                ScDiag::ELedgerWrong => 0x07,
                ScDiag::ECapFull => 0x08,
                ScDiag::EMissed => 0x09,
                ScDiag::EFp => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ScDiag::ENoEnt => "格号越界：格号不在 0..108，核对矩阵账四轴线性化下标",
            ScDiag::EBits => "轴/样本档越闭集：深度/展开/码族/坏表或样本档不在闭集，按闭集登记",
            ScDiag::ERewind => "状态/段序倒退：B39 段序盘点→复核→冻结只进不退，先补前段",
            ScDiag::EEmpty => "段文档空：B39 节点文档为空串，补盘点/复核/冻结结论文档",
            ScDiag::EDouble => "格重复登记：一格一主，同格不得二次入账，查矩阵账去重",
            ScDiag::ESegGap => "段缺口/悬空：三段未齐或引用链断，补齐前段并接引用链",
            ScDiag::ELedgerWrong => "错账：三账不对平（矩阵×样本档≠样本账≠结果账），逐账复算",
            ScDiag::ECapFull => "容量耗尽：账册判满，先收官封账或换新账册再入新果",
            ScDiag::EMissed => "注入漏检：错账注入未被抓到，收官口对平断言逻辑须重查",
            ScDiag::EFp => "双跑指纹不一致：两次收官指纹漂移，排查非确定源",
        }
    }

    /// 闭集全量。
    pub const ALL: [ScDiag; 10] = [
        ScDiag::ENoEnt,
        ScDiag::EBits,
        ScDiag::ERewind,
        ScDiag::EEmpty,
        ScDiag::EDouble,
        ScDiag::ESegGap,
        ScDiag::ELedgerWrong,
        ScDiag::ECapFull,
        ScDiag::EMissed,
        ScDiag::EFp,
    ];
}

/// 诊断码查表（闭集内回中文文案，闭集外中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ScDiag::ALL.len() {
        if let Some(d) = ScDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82740 段闭集内，按收官账诊断码闭集复核"
}

// ── 矩阵账 ───────────────────────────────────────────────────────
/// 矩阵账：四轴线性化格登记（一格一主）。
pub struct MatrixBook {
    seen: [bool; MATRIX_CELLS],
    booked: u32,
    fp: u64,
}

impl MatrixBook {
    pub fn new() -> Self {
        MatrixBook {
            seen: [false; MATRIX_CELLS],
            booked: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 四轴线性化下标（越轴显性拒）。
    pub fn cell_index(depth: usize, unwind: usize, family: usize, badtable: usize) -> Result<usize, ScDiag> {
        if depth >= AXIS_DEPTH || unwind >= AXIS_UNWIND || family >= AXIS_FAMILY || badtable >= AXIS_BADTABLE {
            return Err(ScDiag::EBits);
        }
        Ok(depth * (AXIS_UNWIND * AXIS_FAMILY * AXIS_BADTABLE)
            + unwind * (AXIS_FAMILY * AXIS_BADTABLE)
            + family * AXIS_BADTABLE
            + badtable)
    }

    /// 登记一格（越轴/重复登记显性拒）。
    pub fn register_cell(&mut self, depth: usize, unwind: usize, family: usize, badtable: usize) -> Result<usize, ScDiag> {
        let idx = Self::cell_index(depth, unwind, family, badtable)?;
        let occupied = match self.seen.get(idx) {
            Some(true) => true,
            _ => false,
        };
        if occupied {
            return Err(ScDiag::EDouble);
        }
        let slot = match self.seen.get_mut(idx) {
            Some(s) => s,
            None => return Err(ScDiag::ENoEnt),
        };
        *slot = true;
        self.booked = self.booked.saturating_add(1);
        self.fnv(0x5300u64 + idx as u64);
        Ok(idx)
    }

    /// 矩阵格数（盘点账 Σ格数）。
    pub fn grid_sum(&self) -> u32 {
        self.booked
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for MatrixBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 样本账 ───────────────────────────────────────────────────────
/// 样本账：逐格逐档录样，逐档分账对平。
pub struct SampleBook {
    count: u32,
    tier_counts: [u32; SAMPLE_TIERS],
    fp: u64,
}

impl SampleBook {
    pub fn new() -> Self {
        SampleBook {
            count: 0,
            tier_counts: [0; SAMPLE_TIERS],
            fp: 0xcbf29ce484222325,
        }
    }

    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 录一条样本（格号/档越闭集显性拒）。
    pub fn register_sample(&mut self, cell_no: usize, tier: usize) -> Result<(), ScDiag> {
        if cell_no >= MATRIX_CELLS {
            return Err(ScDiag::ENoEnt);
        }
        if tier >= SAMPLE_TIERS {
            return Err(ScDiag::EBits);
        }
        self.count = self.count.saturating_add(1);
        if let Some(t) = self.tier_counts.get_mut(tier) {
            *t = t.saturating_add(1);
        }
        self.fnv(0x5301u64 + cell_no as u64 + ((tier as u64) << 16));
        Ok(())
    }

    /// 样本账总条数。
    pub fn sample_sum(&self) -> u32 {
        self.count
    }

    /// 逐档分账。
    pub fn tiers(&self) -> [u32; SAMPLE_TIERS] {
        self.tier_counts
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for SampleBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 结果账 ───────────────────────────────────────────────────────
/// 结果账：逐格逐档落果。
pub struct ResultBook {
    count: u32,
    tier_counts: [u32; SAMPLE_TIERS],
    fp: u64,
}

impl ResultBook {
    pub fn new() -> Self {
        ResultBook {
            count: 0,
            tier_counts: [0; SAMPLE_TIERS],
            fp: 0xcbf29ce484222325,
        }
    }

    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一条结果（格号/档越闭集显性拒）。
    pub fn append_result(&mut self, cell_no: usize, tier: usize) -> Result<(), ScDiag> {
        if cell_no >= MATRIX_CELLS {
            return Err(ScDiag::ENoEnt);
        }
        if tier >= SAMPLE_TIERS {
            return Err(ScDiag::EBits);
        }
        self.count = self.count.saturating_add(1);
        if let Some(t) = self.tier_counts.get_mut(tier) {
            *t = t.saturating_add(1);
        }
        self.fnv(0x5302u64 + cell_no as u64 + ((tier as u64) << 16));
        Ok(())
    }

    /// 结果账总条数。
    pub fn result_sum(&self) -> u32 {
        self.count
    }

    /// 逐档分账。
    pub fn tiers(&self) -> [u32; SAMPLE_TIERS] {
        self.tier_counts
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for ResultBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 收官口：三账对平 ─────────────────────────────────────────────
/// 收官账等式断言：盘点齐（矩阵 108 格）→ 矩阵×样本档 ≡ 样本账 ≡
/// 结果账 → 逐档分账对平（每档 = 矩阵格数）。破缺显性拒。
pub fn close_books(
    m: &MatrixBook,
    s: &SampleBook,
    r: &ResultBook,
) -> Result<(u32, u32, u32), ScDiag> {
    let grid = m.grid_sum();
    if grid != MATRIX_CELLS as u32 {
        return Err(ScDiag::ESegGap);
    }
    let expected = match grid.checked_mul(SAMPLE_TIERS as u32) {
        Some(e) => e,
        None => return Err(ScDiag::ELedgerWrong),
    };
    if s.sample_sum() != expected || r.result_sum() != expected {
        return Err(ScDiag::ELedgerWrong);
    }
    // 逐档分账对平：每档样本数 == 每档结果数 == 矩阵格数。
    let st = s.tiers();
    let rt = r.tiers();
    let mut tier = 0usize;
    while tier < SAMPLE_TIERS {
        let sv = match st.get(tier) {
            Some(x) => *x,
            None => return Err(ScDiag::ELedgerWrong),
        };
        let rv = match rt.get(tier) {
            Some(x) => *x,
            None => return Err(ScDiag::ELedgerWrong),
        };
        if sv != grid || rv != grid {
            return Err(ScDiag::ELedgerWrong);
        }
        tier += 1;
    }
    Ok((grid, s.sample_sum(), r.result_sum()))
}

// ── B39 对齐框架 ─────────────────────────────────────────────────
/// B39 三段闭集（盘点/复核/冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum B39Seg {
    /// 盘点段（第一段）。
    Inventory,
    /// 复核段（第二段，引用盘点）。
    Review,
    /// 冻结段（第三段，引用复核）。
    Freeze,
}

impl B39Seg {
    /// 段序号（0/1/2）。
    pub fn idx(self) -> usize {
        match self {
            B39Seg::Inventory => 0,
            B39Seg::Review => 1,
            B39Seg::Freeze => 2,
        }
    }

    /// 中文段名。
    pub fn name(self) -> &'static str {
        match self {
            B39Seg::Inventory => "盘点",
            B39Seg::Review => "复核",
            B39Seg::Freeze => "冻结",
        }
    }
}

/// 节点状态（Draft→Sealed 单调只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum B39State {
    /// 草稿。
    Draft,
    /// 已封。
    Sealed,
}

/// B39 节点：段 + 非空文档 + 引用链。
#[derive(Clone, Debug)]
pub struct B39Node {
    /// 段。
    pub seg: B39Seg,
    /// 段文档（非空才可登记）。
    pub doc: String,
    /// 引用链（指向被引用段的槽位；盘点段为 None）。
    pub link: Option<usize>,
    /// 状态。
    pub state: B39State,
}

/// B39 对齐框架（定容 3 节点）。
pub struct B39Frame {
    nodes: [Option<B39Node>; 3],
    sealed: bool,
    fp: u64,
}

impl B39Frame {
    pub fn new() -> Self {
        B39Frame {
            nodes: [const { None }; 3],
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 登记一段：段序强制（先冻后盘显性拒）、文档非空、引用链槽位在
    /// 界（悬空留到封账口审计）、重复登记显性拒。
    pub fn register(&mut self, seg: B39Seg, doc: &str, link: Option<usize>) -> Result<usize, ScDiag> {
        // 判定次序冻结：封账后写回 → 段序倒退 → 段文档空 → 引用出界/
        // 悬空 → 重复登记。先到先判。
        if self.sealed {
            return Err(ScDiag::ERewind);
        }
        // 段序强制：复核前必须有盘点，冻结前必须有复核。
        let need = match seg {
            B39Seg::Inventory => 0usize,
            B39Seg::Review => 1usize,
            B39Seg::Freeze => 2usize,
        };
        let mut have = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if let Some(Some(_)) = self.nodes.get(i) {
                have += 1;
            }
            i += 1;
        }
        if have < need {
            return Err(ScDiag::ERewind);
        }
        if doc.is_empty() {
            return Err(ScDiag::EEmpty);
        }
        if let Some(l) = link {
            if l >= 3 {
                return Err(ScDiag::ESegGap);
            }
            // 悬空引用登记即拒：引用目标必须已登记（登记时点校验）。
            let target = match self.nodes.get(l) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !target {
                return Err(ScDiag::ESegGap);
            }
        }
        let at = seg.idx();
        let occupied = match self.nodes.get(at) {
            Some(Some(_)) => true,
            _ => false,
        };
        if occupied {
            return Err(ScDiag::EDouble);
        }
        let slot = match self.nodes.get_mut(at) {
            Some(s) => s,
            None => return Err(ScDiag::ESegGap),
        };
        *slot = Some(B39Node {
            seg,
            doc: String::from(doc),
            link,
            state: B39State::Draft,
        });
        self.fnv(0x5303u64 + at as u64);
        Ok(at)
    }

    /// 引用链悬空判定：任一节点引用指向未登记槽位即悬空。
    pub fn chain_dangling(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let Some(Some(node)) = self.nodes.get(i) {
                if let Some(l) = node.link {
                    let target = match self.nodes.get(l) {
                        Some(Some(_)) => true,
                        _ => false,
                    };
                    if !target {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    /// 引用链闭集判定：盘点→None、复核→盘点、冻结→复核，三链全接。
    pub fn chain_valid(&self) -> bool {
        let inv_link = match self.nodes.get(0) {
            Some(Some(n)) => n.link,
            _ => return false,
        };
        let rev_link = match self.nodes.get(1) {
            Some(Some(n)) => n.link,
            _ => return false,
        };
        let frz_link = match self.nodes.get(2) {
            Some(Some(n)) => n.link,
            _ => return false,
        };
        inv_link.is_none() && rev_link == Some(0) && frz_link == Some(1)
    }

    /// 封账：三段齐 + 每段文档非空 + 引用链零悬空 + 链序闭集。
    pub fn seal(&mut self) -> Result<u64, ScDiag> {
        if self.sealed {
            return Err(ScDiag::ERewind);
        }
        let mut have = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if let Some(Some(n)) = self.nodes.get(i) {
                if n.doc.is_empty() {
                    return Err(ScDiag::EEmpty);
                }
                have += 1;
            }
            i += 1;
        }
        if have < 3 {
            return Err(ScDiag::ESegGap);
        }
        if self.chain_dangling() || !self.chain_valid() {
            return Err(ScDiag::ESegGap);
        }
        let mut j = 0usize;
        while j < 3 {
            if let Some(slot) = self.nodes.get_mut(j) {
                if let Some(n) = slot {
                    n.state = B39State::Sealed;
                }
            }
            j += 1;
        }
        self.sealed = true;
        self.fnv(0x5304);
        Ok(self.fp)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 已登记段数。
    pub fn registered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if let Some(Some(_)) = self.nodes.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

impl Default for B39Frame {
    fn default() -> Self {
        Self::new()
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// 抽 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 收官全流程：登满矩阵 108 格 → 逐格逐档录样 432 → 逐格逐档落果
/// 432 → 三账对平 → B39 三段登记封账。返回（三账三元组, 框架指纹）。
pub fn golden_close() -> Result<((u32, u32, u32), u64), ScDiag> {
    let mut m = MatrixBook::new();
    let mut d = 0usize;
    while d < AXIS_DEPTH {
        let mut u = 0usize;
        while u < AXIS_UNWIND {
            let mut f = 0usize;
            while f < AXIS_FAMILY {
                let mut b = 0usize;
                while b < AXIS_BADTABLE {
                    m.register_cell(d, u, f, b)?;
                    b += 1;
                }
                f += 1;
            }
            u += 1;
        }
        d += 1;
    }
    // LCG（种子=SEG_BASE）打乱落样序（确定性），配合已录位图去重：
    // 先抽格×档，重复抽中跳过；随后按序补齐漏格漏档，保证格×档恰
    // 全覆盖一遍（总条数恰 = 矩阵×样本档）。
    let mut filled = [false; EXPECTED_RESULTS as usize];
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut s = SampleBook::new();
    let mut r = ResultBook::new();
    let mut k = 0u32;
    while k < EXPECTED_RESULTS {
        let cell = rng.below(MATRIX_CELLS as u64) as usize;
        let tier = rng.below(SAMPLE_TIERS as u64) as usize;
        let at = cell * SAMPLE_TIERS + tier;
        let slot = match filled.get_mut(at) {
            Some(x) => x,
            None => return Err(ScDiag::ENoEnt),
        };
        if !*slot {
            *slot = true;
            s.register_sample(cell, tier)?;
            r.append_result(cell, tier)?;
        }
        k += 1;
    }
    let mut cell = 0usize;
    while cell < MATRIX_CELLS {
        let mut tier = 0usize;
        while tier < SAMPLE_TIERS {
            let at = cell * SAMPLE_TIERS + tier;
            let slot = match filled.get_mut(at) {
                Some(x) => x,
                None => return Err(ScDiag::ENoEnt),
            };
            if !*slot {
                *slot = true;
                s.register_sample(cell, tier)?;
                r.append_result(cell, tier)?;
            }
            tier += 1;
        }
        cell += 1;
    }
    let triple = close_books(&m, &s, &r)?;
    let mut frame = B39Frame::new();
    frame.register(B39Seg::Inventory, "盘点：压测矩阵 108 格全登账，四轴闭集点验", None)?;
    frame.register(B39Seg::Review, "复核：三账对平断言绿，逐档分账 108 对平", Some(0))?;
    frame.register(B39Seg::Freeze, "冻结：收官账封账，衔接 B39 零悬空", Some(1))?;
    let fp = frame.seal()?;
    Ok((triple, fp))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
/// 注入三族账（注入/检出/漏检）。
#[derive(Clone, Copy, Debug)]
pub struct InjLedger {
    /// 各族注入次数。
    pub tries: [u32; 3],
    /// 各族检出次数。
    pub caught: [u32; 3],
    /// 各族漏检次数（必须全 0）。
    pub missed: [u32; 3],
}

impl InjLedger {
    pub const fn new() -> Self {
        InjLedger {
            tries: [0; 3],
            caught: [0; 3],
            missed: [0; 3],
        }
    }

    /// 注入数 = 检出数 + 漏检数（逐族守恒）。
    pub fn conservation_ok(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let (Some(t), Some(c), Some(m)) = (self.tries.get(i), self.caught.get(i), self.missed.get(i)) {
                if *t != c.saturating_add(*m) {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 全抓判定。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let (Some(t), Some(c), Some(m)) = (self.tries.get(i), self.caught.get(i), self.missed.get(i)) {
                if c != t || *m != 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

impl Default for InjLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 全账基座（矩阵 108 格 + 样本/结果按格×档补齐到满）。
fn full_books() -> (MatrixBook, SampleBook, ResultBook) {
    let mut m = MatrixBook::new();
    let mut d = 0usize;
    while d < AXIS_DEPTH {
        let mut u = 0usize;
        while u < AXIS_UNWIND {
            let mut f = 0usize;
            while f < AXIS_FAMILY {
                let mut b = 0usize;
                while b < AXIS_BADTABLE {
                    let _ = m.register_cell(d, u, f, b);
                    b += 1;
                }
                f += 1;
            }
            u += 1;
        }
        d += 1;
    }
    let mut s = SampleBook::new();
    let mut r = ResultBook::new();
    let mut cell = 0usize;
    while cell < MATRIX_CELLS {
        let mut tier = 0usize;
        while tier < SAMPLE_TIERS {
            let _ = s.register_sample(cell, tier);
            let _ = r.append_result(cell, tier);
            tier += 1;
        }
        cell += 1;
    }
    (m, s, r)
}

/// 族0 结果账漂移：结果账 +1 → 对平破缺 ELedgerWrong 必抓。
fn inj_family0(led: &mut InjLedger, k: usize) {
    if let Some(t) = led.tries.get_mut(0) {
        *t += 1;
    }
    let (m, s, mut r) = full_books();
    for _ in 0..(1 + k % 3) {
        let _ = r.append_result(0, 0);
    }
    if close_books(&m, &s, &r) == Err(ScDiag::ELedgerWrong) {
        if let Some(c) = led.caught.get_mut(0) {
            *c += 1;
        }
    } else if let Some(miss) = led.missed.get_mut(0) {
        *miss += 1;
    }
}

/// 族1 样本账失衡：样本账 +1、结果账 +2 → 样本账≠矩阵×样本档且两账
/// 失衡，对平破缺 ELedgerWrong 必抓。
fn inj_family1(led: &mut InjLedger, k: usize) {
    if let Some(t) = led.tries.get_mut(1) {
        *t += 1;
    }
    let (m, mut s, mut r) = full_books();
    let skip_cell = k % MATRIX_CELLS;
    let _ = s.register_sample(skip_cell, 0);
    let _ = r.append_result(skip_cell, 0);
    // 只在结果账补，样本账欠一条 → 两账失衡。
    let _ = r.append_result(skip_cell, 1);
    if close_books(&m, &s, &r) == Err(ScDiag::ELedgerWrong) {
        if let Some(c) = led.caught.get_mut(1) {
            *c += 1;
        }
    } else if let Some(miss) = led.missed.get_mut(1) {
        *miss += 1;
    }
}

/// 族2 盘点缺口：少登一格 → 收官口 ESegGap 必抓。
fn inj_family2(led: &mut InjLedger, k: usize) {
    if let Some(t) = led.tries.get_mut(2) {
        *t += 1;
    }
    let mut m = MatrixBook::new();
    let skip = k % MATRIX_CELLS;
    let mut d = 0usize;
    while d < AXIS_DEPTH {
        let mut u = 0usize;
        while u < AXIS_UNWIND {
            let mut f = 0usize;
            while f < AXIS_FAMILY {
                let mut b = 0usize;
                while b < AXIS_BADTABLE {
                    let idx = MatrixBook::cell_index(d, u, f, b).unwrap_or(usize::MAX);
                    if idx != skip {
                        let _ = m.register_cell(d, u, f, b);
                    }
                    b += 1;
                }
                f += 1;
            }
            u += 1;
        }
        d += 1;
    }
    let (mut s, mut r) = (SampleBook::new(), ResultBook::new());
    let mut cell = 0usize;
    while cell < MATRIX_CELLS {
        if cell != skip {
            let mut tier = 0usize;
            while tier < SAMPLE_TIERS {
                let _ = s.register_sample(cell, tier);
                let _ = r.append_result(cell, tier);
                tier += 1;
            }
        }
        cell += 1;
    }
    if close_books(&m, &s, &r) == Err(ScDiag::ESegGap) {
        if let Some(c) = led.caught.get_mut(2) {
            *c += 1;
        }
    } else if let Some(miss) = led.missed.get_mut(2) {
        *miss += 1;
    }
}

/// 跑满 3 族 × 10 轮注入。
pub fn run_injections(led: &mut InjLedger) {
    let mut k = 0usize;
    while k < 10 {
        inj_family0(led, k);
        inj_family1(led, k);
        inj_family2(led, k);
        k += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
/// 收官账等式绿：黄金三元组 == (108, 432, 432) 且指纹非零。
fn closeout_eq_ok() -> bool {
    match golden_close() {
        Ok((triple, fp)) => {
            triple.0 == MATRIX_CELLS as u32
                && triple.1 == EXPECTED_RESULTS
                && triple.2 == EXPECTED_RESULTS
                && fp != 0
        }
        Err(_) => false,
    }
}

/// B39 悬空注入必抓：悬空链 / 断链序 / 先冻后盘 / 空文档 / 链出界五路注入。
fn b39_injections_ok() -> bool {
    // 路1：悬空引用（冻结链指向自身未登记槽位）→ 登记口 ESegGap。
    let mut f1 = B39Frame::new();
    let _ = f1.register(B39Seg::Inventory, "盘点文档", None);
    let _ = f1.register(B39Seg::Review, "复核文档", Some(0));
    let r1 = f1.register(B39Seg::Freeze, "冻结文档", Some(2));
    if r1 != Err(ScDiag::ESegGap) {
        return false;
    }
    // 路2：链序错（冻结引用盘点，跳过复核）→ 封账口 ESegGap。
    let mut f2 = B39Frame::new();
    let _ = f2.register(B39Seg::Inventory, "盘点文档", None);
    let _ = f2.register(B39Seg::Review, "复核文档", Some(0));
    let _ = f2.register(B39Seg::Freeze, "冻结文档", Some(0));
    let r2 = f2.seal();
    if r2 != Err(ScDiag::ESegGap) {
        return false;
    }
    // 路3：段序倒退（未盘点先冻结）→ ERewind。
    let mut f3 = B39Frame::new();
    let r3 = f3.register(B39Seg::Freeze, "冻结文档", Some(1));
    if r3 != Err(ScDiag::ERewind) {
        return false;
    }
    // 路4：空文档 → EEmpty。
    let mut f4 = B39Frame::new();
    let r4 = f4.register(B39Seg::Inventory, "", None);
    if r4 != Err(ScDiag::EEmpty) {
        return false;
    }
    // 路5：链出界（引用槽位 5）→ 登记口 ESegGap。
    let mut f5 = B39Frame::new();
    let r5 = f5.register(B39Seg::Inventory, "盘点文档", Some(5));
    if r5 != Err(ScDiag::ESegGap) {
        return false;
    }
    true
}

/// 判据总谓词：三账对平绿 + 注入全抓 + B39 零悬空。
fn stress_close_ok() -> bool {
    let mut led = InjLedger::new();
    run_injections(&mut led);
    closeout_eq_ok() && led.all_caught() && led.conservation_ok() && b39_injections_ok()
}

// ── 段位守卫 ─────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 0x40 对齐且落在 VX9 域，诊断码闭集全落段内窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE > 0x82FC0 {
        return false;
    }
    let mut i = 0usize;
    while i < ScDiag::ALL.len() {
        if let Some(d) = ScDiag::ALL.get(i) {
            let v = d.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条闭集，extra_checks 逐条进 set）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 正常流：黄金收官三元组绿。
    let golden = golden_close();
    v.push((
        "J1-黄金收官绿",
        match &golden {
            Ok((t, _)) => *t == (MATRIX_CELLS as u32, EXPECTED_RESULTS, EXPECTED_RESULTS),
            Err(_) => false,
        },
        format!("{:?}", golden.map(|(t, f)| (t, f))),
    ));
    // 2. 等式断言：矩阵×样本档 ≡ 样本账 ≡ 结果账。
    v.push((
        "J1-三账对平断言",
        match &golden {
            Ok((t, _)) => t.0.saturating_mul(SAMPLE_TIERS as u32) == t.1 && t.1 == t.2,
            Err(_) => false,
        },
        format!("格数={} 样本档={} 期望={}", MATRIX_CELLS, SAMPLE_TIERS, EXPECTED_RESULTS),
    ));
    // 3. 边界：格号 108 越界拒。
    let mut s1 = SampleBook::new();
    let r1 = s1.register_sample(MATRIX_CELLS, 0);
    v.push(("边界·格号108越界拒", r1 == Err(ScDiag::ENoEnt), format!("{:?}", r1)));
    // 4. 边界：样本档 4 越档拒。
    let mut s2 = SampleBook::new();
    let r2 = s2.register_sample(0, SAMPLE_TIERS);
    v.push(("边界·样本档4越档拒", r2 == Err(ScDiag::EBits), format!("{:?}", r2)));
    // 5. 边界：四轴越闭集拒。
    let r3 = MatrixBook::cell_index(AXIS_DEPTH, 0, 0, 0);
    v.push(("边界·轴越闭集拒", r3 == Err(ScDiag::EBits), format!("{:?}", r3)));
    // 6. 注入三族 10/10 全抓。
    let mut led = InjLedger::new();
    run_injections(&mut led);
    v.push((
        "注入·三族10/10全抓",
        led.all_caught(),
        format!("t={:?} c={:?}", led.tries, led.caught),
    ));
    // 7. 计数守恒：注入数=检出数+漏检数、漏检=0。
    v.push((
        "注入·计数守恒漏检0",
        led.conservation_ok() && led.missed == [0; 3],
        format!("m={:?}", led.missed),
    ));
    // 8. B39 三段登记齐（盘点/复核/冻结各非空）。
    let mut f = B39Frame::new();
    let _ = f.register(B39Seg::Inventory, "盘点：矩阵 108 格全登账", None);
    let _ = f.register(B39Seg::Review, "复核：三账对平断言绿", Some(0));
    let _ = f.register(B39Seg::Freeze, "冻结：收官账封账", Some(1));
    let sealed = f.seal();
    v.push((
        "B39·三段登记齐",
        f.registered() == 3 && sealed.is_ok(),
        format!("段数={} seal={:?}", f.registered(), sealed.is_ok()),
    ));
    // 9. B39 引用链零悬空。
    v.push((
        "B39·引用链零悬空",
        !f.chain_dangling() && f.chain_valid(),
        String::from("盘点→None 复核→0 冻结→1 三链全接"),
    ));
    // 10. B39 悬空/断链/倒退/空文档/出界五路注入必抓。
    v.push((
        "B39·五路注入必抓",
        b39_injections_ok(),
        String::from("悬空/断链/先冻后盘/空文档/链出界"),
    ));
    // 11. 双跑一致。
    let g1 = golden_close();
    let g2 = golden_close();
    v.push((
        "双跑一致",
        g1 == g2 && g1.is_ok(),
        format!("{:?} vs {:?}", g1, g2),
    ));
    // 12. 段位守卫。
    v.push((
        "段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X}", SEG_BASE),
    ));
    // 13. 判据总谓词。
    v.push((
        "J1-stress_close_ok总谓词",
        stress_close_ok(),
        String::from("对平绿+注入全抓+B39零悬空"),
    ));
    v
}

/// 检查入口：extra_checks 逐条进 set；动态明细槽 ≤8，其余走静态明细
/// 不丢红。
pub fn run_unx12757_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12757");
    let checks = extra_checks();
    let mut idx = 0usize;
    for (name, passed, dyn_detail) in checks {
        if idx < 8 {
            s.add_dyn(name, passed, &dyn_detail);
        } else {
            let static_detail: &'static str = match name {
                "注入·计数守恒漏检0" => "注入数=检出数+漏检数，漏检必须为 0",
                "B39·三段登记齐" => "盘点/复核/冻结三段各非空且封账绿",
                "B39·引用链零悬空" => "盘点→None、复核→盘点、冻结→复核三链全接",
                "B39·五路注入必抓" => "悬空/断链/先冻后盘/空文档/链出界全抓",
                "双跑一致" => "两次收官全流程（三元组,指纹）逐字段相等",
                "段位守卫" => "SEG_BASE 0x40 对齐且落在 VX9 域",
                "J1-stress_close_ok总谓词" => "对平绿+注入全抓+B39零悬空总判据",
                _ => "静态收口条目：明细见诊断码闭集 detail()",
            };
            s.add(name, passed, static_detail);
        }
        idx += 1;
    }
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_closeout() {
        let g = golden_close();
        assert!(g.is_ok(), "黄金收官必须成功");
        let (triple, fp) = g.unwrap_or(((0, 0, 0), 0));
        assert_eq!(triple, (MATRIX_CELLS as u32, EXPECTED_RESULTS, EXPECTED_RESULTS));
        assert_ne!(fp, 0);
    }

    #[test]
    fn test_boundary_cell_overflow() {
        let mut s = SampleBook::new();
        assert_eq!(s.register_sample(MATRIX_CELLS, 0), Err(ScDiag::ENoEnt));
        let mut r = ResultBook::new();
        assert_eq!(r.append_result(MATRIX_CELLS, 0), Err(ScDiag::ENoEnt));
    }

    #[test]
    fn test_boundary_tier_overflow() {
        let mut s = SampleBook::new();
        assert_eq!(s.register_sample(0, SAMPLE_TIERS), Err(ScDiag::EBits));
    }

    #[test]
    fn test_boundary_axis_overflow() {
        assert_eq!(MatrixBook::cell_index(3, 0, 0, 0), Err(ScDiag::EBits));
        assert_eq!(MatrixBook::cell_index(0, 3, 0, 0), Err(ScDiag::EBits));
        assert_eq!(MatrixBook::cell_index(0, 0, 4, 0), Err(ScDiag::EBits));
        assert_eq!(MatrixBook::cell_index(0, 0, 0, 3), Err(ScDiag::EBits));
        assert!(MatrixBook::cell_index(2, 2, 3, 2).is_ok());
    }

    #[test]
    fn test_double_book_cell() {
        let mut m = MatrixBook::new();
        let _ = m.register_cell(0, 0, 0, 0);
        assert_eq!(m.register_cell(0, 0, 0, 0), Err(ScDiag::EDouble));
    }

    #[test]
    fn test_injection_all_families_caught() {
        let mut led = InjLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught(), "3 族 10 轮必须全抓");
        assert!(led.conservation_ok());
        let mut i = 0usize;
        while i < 3 {
            assert_eq!(led.tries.get(i), Some(&10));
            i += 1;
        }
    }

    #[test]
    fn test_b39_order_and_empty_doc() {
        let mut f = B39Frame::new();
        assert_eq!(f.register(B39Seg::Freeze, "先冻", Some(1)), Err(ScDiag::ERewind));
        assert_eq!(f.register(B39Seg::Inventory, "", None), Err(ScDiag::EEmpty));
    }

    #[test]
    fn test_b39_dangling_chain() {
        let mut f = B39Frame::new();
        let _ = f.register(B39Seg::Inventory, "盘点文档", None);
        let _ = f.register(B39Seg::Review, "复核文档", Some(0));
        // 悬空引用登记即拒（目标槽位未登记）。
        assert_eq!(f.register(B39Seg::Freeze, "冻结文档", Some(2)), Err(ScDiag::ESegGap));
        assert_eq!(f.register(B39Seg::Inventory, "重复盘点", None), Err(ScDiag::EDouble));
        // 链出界同样登记即拒。
        let mut f2 = B39Frame::new();
        assert_eq!(f2.register(B39Seg::Inventory, "盘点文档", Some(5)), Err(ScDiag::ESegGap));
    }

    #[test]
    fn test_dual_run_identical() {
        let g1 = golden_close();
        let g2 = golden_close();
        assert_eq!(g1, g2, "两次收官必须逐字段相等");
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert!(segment_guard());
    }

    #[test]
    fn test_detail_closed_set() {
        for d in ScDiag::ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).contains("未知诊断码"));
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知诊断码"));
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx12757_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
