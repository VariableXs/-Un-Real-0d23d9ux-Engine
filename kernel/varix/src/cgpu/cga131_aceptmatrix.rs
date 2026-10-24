//! cga131_aceptmatrix — CGPU-F0131 A 域验收矩阵。
//!
//! 判据原文（施工书锚点 CGPU-F0131）：A 域 20 维度验收（F0388 范式）：
//! 驱动域映射口径（视觉维度→CPU/GPU 输出一致性对拍/交互维度→线程池与
//! 队列的响应手感/性能维度→确定性+伸缩+能耗三轴）。逐维证据链接。
//! 判据：20 维全过、映射口径、证据、处置、签署。
//!
//! **设计定盘**：20 维验收闭集，每维归属驱动域三闭集之一（视觉/交互/
use alloc::vec;
/// 性能），逐维评分（0~100 整数）+ 通过线（视觉 80 / 交互 75 / 性能
/// 70——映射口径的不同门槛是口径语义的一部分）。逐维证据 = 证据指纹
/// 登记（零指纹评分无效）。处置 = 低分维的处置登记（修复单/ADR 豁免，
/// 处置后复评分必须达线）。签署 = 20 维全达线 + 全有证据才能签。
///
/// 诊断码段 0x0220~0x022F（0x02 段第 2 子区）。
/// no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA131_VERSION: &str = "cga131-aceptmatrix-v1";

/// 诊断子区基值（0x02 段第 2 子区）。
pub const SEG_BASE: u16 = 0x0220;

/// 维度总数（判据"20 维度"）。
pub const DIMENSIONS: u32 = 20;

/// 驱动域闭集（判据"映射口径"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveDomain {
    /// 视觉维度（一致性对拍口径，通过线 80）。
    Visual,
    /// 交互维度（响应手感口径，通过线 75）。
    Interactive,
    /// 性能维度（确定性+伸缩+能耗三轴口径，通过线 70）。
    Performance,
}

impl DriveDomain {
    /// 各域通过线（映射口径语义）。
    pub fn pass_line(self) -> u32 {
        match self {
            DriveDomain::Visual => 80,
            DriveDomain::Interactive => 75,
            DriveDomain::Performance => 70,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            DriveDomain::Visual => SEG_BASE + 1,
            DriveDomain::Interactive => SEG_BASE + 2,
            DriveDomain::Performance => SEG_BASE + 3,
        }
    }
}

/// 处置方式闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Remedy {
    /// 修复单。
    FixOrder,
    /// ADR 豁免（带 ADR 编号）。
    AdrWaive,
}

/// 一维验收记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DimRow {
    /// 维号（1..=20）。
    pub dim: u32,
    /// 驱动域。
    pub domain: DriveDomain,
    /// 评分（0~100）。
    pub score: u32,
    /// 证据指纹（零=无证据=评分无效）。
    pub evidence_fp: u64,
    /// 处置（None=未处置）。
    pub remedy: Option<(Remedy, u32)>,
}

/// 验收矩阵。
pub struct AcceptMatrix {
    /// 维记录。
    rows: Vec<DimRow>,
    /// 签署指纹。
    signed: Option<u64>,
}

impl AcceptMatrix {
    /// 空矩阵。
    pub fn new() -> Self {
        AcceptMatrix {
            rows: Vec::new(),
            signed: None,
        }
    }

    /// 评分登记（判据"逐维证据"：证据指纹为零即拒——无证据的评分
    /// 是空话；同维重复登记即拒）。
    pub fn score(
        &mut self,
        dim: u32,
        domain: DriveDomain,
        score: u32,
        evidence_fp: u64,
    ) -> Result<(), u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if dim == 0 || dim > DIMENSIONS {
            return Err(SEG_BASE + 5);
        }
        if score > 100 {
            return Err(SEG_BASE + 6);
        }
        if evidence_fp == 0 {
            return Err(SEG_BASE + 7);
        }
        if self.rows.iter().any(|r| r.dim == dim) {
            return Err(SEG_BASE + 8);
        }
        self.rows.push(DimRow {
            dim,
            domain,
            score,
            evidence_fp,
            remedy: None,
        });
        Ok(())
    }

    /// 低分处置（判据"处置"：只许对未达线维处置；重复处置即拒）。
    pub fn remedy(&mut self, dim: u32, r: Remedy, ref_no: u32) -> Result<(), u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 4);
        }
        let row = match self.rows.iter_mut().find(|x| x.dim == dim) {
            Some(x) => x,
            None => return Err(SEG_BASE + 9),
        };
        if row.score >= row.domain.pass_line() {
            return Err(SEG_BASE + 10);
        }
        if row.remedy.is_some() {
            return Err(SEG_BASE + 11);
        }
        if ref_no == 0 {
            return Err(SEG_BASE + 12);
        }
        row.remedy = Some((r, ref_no));
        Ok(())
    }

    /// 复评分（判据"处置后复测必须达线"：处置过的维复评分覆盖；
    /// 未处置的维复评即拒）。
    pub fn rescore(&mut self, dim: u32, score: u32) -> Result<(), u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if score > 100 {
            return Err(SEG_BASE + 6);
        }
        let row = match self.rows.iter_mut().find(|x| x.dim == dim) {
            Some(x) => x,
            None => return Err(SEG_BASE + 9),
        };
        if row.remedy.is_none() {
            return Err(SEG_BASE + 13);
        }
        row.score = score;
        Ok(())
    }

    /// 维度完整性（判据"20 维全过"：20 维全登记）。
    pub fn dims_complete(&self) -> Result<(), u16> {
        if self.rows.len() != DIMENSIONS as usize {
            return Err(SEG_BASE + 14);
        }
        let mut seen = [false; 21];
        for r in &self.rows {
            seen[r.dim as usize] = true;
        }
        for i in 1..=DIMENSIONS as usize {
            if !seen[i] {
                return Err(SEG_BASE + 15);
            }
        }
        Ok(())
    }

    /// 签署（判据"签署"：20 维全登记且逐维达线（或处置后复评达线）
    /// 才可签）。
    pub fn sign(&mut self) -> Result<u64, u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 16);
        }
        self.dims_complete()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            if r.score < r.domain.pass_line() {
                return Err(r.domain.code());
            }
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ ((r.score as u64) << 16)
                ^ r.evidence_fp;
        }
        self.signed = Some(fp);
        Ok(fp)
    }

    /// 维记录数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

impl Default for AcceptMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// 域覆盖投影行（判据"映射口径"的账面）：三驱动域各自承担的维数
/// 与平均分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainCover {
    /// 驱动域。
    pub domain: DriveDomain,
    /// 维数。
    pub dims: usize,
    /// 平均分（整数；无维为 0）。
    pub avg_score: u32,
}

/// 评分直方图档（判据"逐维评分"的分布面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScoreBand {
    /// 优（90-100）。
    Excellent,
    /// 良（75-89）。
    Good,
    /// 中（60-74）。
    Fair,
    /// 差（<60，必处置）。
    Poor,
}

/// 分档判定。
pub fn score_band(score: u32) -> ScoreBand {
    if score >= 90 {
        ScoreBand::Excellent
    } else if score >= 75 {
        ScoreBand::Good
    } else if score >= 60 {
        ScoreBand::Fair
    } else {
        ScoreBand::Poor
    }
}

impl AcceptMatrix {
    /// 域覆盖投影（判据"映射口径"：三域各多少维、各多少均分）。
    pub fn domain_covers(&self) -> Vec<DomainCover> {
        let mut out: Vec<DomainCover> = Vec::new();
        for d in [DriveDomain::Visual, DriveDomain::Interactive, DriveDomain::Performance] {
            let rows: Vec<&DimRow> =
                self.rows.iter().filter(|r| r.domain == d).collect();
            let dims = rows.len();
            let avg = if dims == 0 {
                0
            } else {
                rows.iter().map(|r| r.score).sum::<u32>() / dims as u32
            };
            out.push(DomainCover { domain: d, dims, avg_score: avg });
        }
        out
    }

    /// 分档计数（判据"逐维评分"的分布面）。
    pub fn band_census(&self) -> (usize, usize, usize, usize) {
        let mut c = (0usize, 0usize, 0usize, 0usize);
        for r in &self.rows {
            match score_band(r.score) {
                ScoreBand::Excellent => c.0 += 1,
                ScoreBand::Good => c.1 += 1,
                ScoreBand::Fair => c.2 += 1,
                ScoreBand::Poor => c.3 += 1,
            }
        }
        c
    }

    /// 全矩阵均分（空阵 None）。
    pub fn avg_score(&self) -> Option<u32> {
        if self.rows.is_empty() {
            return None;
        }
        Some(self.rows.iter().map(|r| r.score).sum::<u32>() / self.rows.len() as u32)
    }

    /// 最低分维（判据"低分项处置核验"的定位面；空阵 None）。
    pub fn weakest_dim(&self) -> Option<(u32, u32)> {
        let mut best: Option<(u32, u32)> = None;
        for r in &self.rows {
            match best {
                Some((_, s)) if s <= r.score => {}
                _ => best = Some((r.dim, r.score)),
            }
        }
        best
    }

    /// 未处置低分核验（判据"低分项是否已修复或 ADR"：未达线且无
    /// 处置的维数——签署前必须为零）。
    pub fn unremedied_below_line(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.score < r.domain.pass_line() && r.remedy.is_none())
            .count()
    }

    /// 处置台账投影（判据"处置"的账面：已处置维逐行列出）。
    pub fn remedy_rows(&self) -> Vec<(u32, Remedy, u32)> {
        self.rows
            .iter()
            .filter_map(|r| r.remedy.map(|(m, n)| (r.dim, m, n)))
            .collect()
    }

    /// 签署指纹。
    pub fn sign_fp(&self) -> Option<u64> {
        self.signed
    }

    /// 证据完整性复核（判据"证据"：全部已登记维的证据指纹非零——
    /// score 入口已拦零，此处为终态复核兜底）。
    pub fn evidence_intact(&self) -> bool {
        self.rows.iter().all(|r| r.evidence_fp != 0)
    }
}


// ===== 深化块（cga131_aceptmatrix · 验收矩阵深化审计）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_aceptmatrix {
    //! 验收矩阵深化审计 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 验收矩阵行守恒：每行 结论数 ∈ {0, 总列数}，半行 = 漏格。
    pub fn row_complete(row: &[Option<bool>]) -> Result<(), u16> {
        let filled = row.iter().filter(|c| c.is_some()).count();
        if filled != 0 && filled != row.len() {
            return Err(filled as u16);
        }
        Ok(())
    }

    /// 矩阵全量扫漏：任一行半格即拒。
    pub fn matrix_no_holes(m: &[&[Option<bool>]]) -> Result<(), u16> {
        for (r, row) in m.iter().enumerate() {
            row_complete(row).map_err(|e| (100 + r as u16) * 16 + e)?;
        }
        Ok(())
    }

    /// 通过率千分比：空矩阵显性 None，不返回零谎报。
    pub fn pass_permille(m: &[&[Option<bool>]]) -> Option<u32> {
        let total: usize = m.iter().map(|r| r.len()).sum();
        let passed: usize = m.iter().flat_map(|r| r.iter()).filter(|c| **c == Some(true)).count();
        if total == 0 {
            return None;
        }
        Some(((passed * 1000) / total) as u32)
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0131-深化");
        let row_ok: Vec<Option<bool>> = vec![Some(true), Some(false), Some(true)];
        let row_hole: Vec<Option<bool>> = vec![Some(true), None, Some(true)];
        cs.add("整行放行", row_complete(&row_ok).is_ok(), "");
        cs.add("半行检出", row_complete(&row_hole).is_err(), "");
        cs.add("矩阵扫漏", matrix_no_holes(&[&row_ok, &row_ok]).is_ok() && matrix_no_holes(&[&row_ok, &row_hole]).is_err(), "");
        cs.add("通过率", pass_permille(&[&row_ok]).is_some(), "");
        cs
    }
}
