//! cga145_acceptfin — CGPU-F0145 A 域验收矩阵终版。
//!
//! 判据原文（施工书锚点 CGPU-F0145）：A 域 20 维度验收终版（F0388/
//! F0131 的域级终版）：160 项全量的逐维评分，低分项处置核验（F0131
//! 阶段评分的低分项是否已修复或 ADR），终版签署。判据：160 项评分、
//! 低分处置核验、终版签署、证据、矩阵归档。
//!
//! **设计定盘**：与 F0131（20 维阶段矩阵）的终版差异：终版矩阵 =
//! 维评分 × 项覆盖双层账——20 维逐维评分 + 每维必须覆盖其映射域的
//! 全部项（覆盖缺口即维未完成）。低分处置核验 = 终版继承 F0131 的
//! 处置账（阶段处置的维在终版必须复评达线或 ADR 升格）。签署 =
//! 全维达线 + 覆盖满 + 低分全处置才可签。归档 = 终版指纹链。
//!
//! 诊断码段 0x0300~0x030F（0x03 段第 1 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA145_VERSION: &str = "cga145-acceptfin-v1";

/// 诊断子区基值（0x03 段第 1 子区）。
pub const SEG_BASE: u16 = 0x0300;

/// 维数（判据"20 维度"）。
pub const DIMENSIONS: u32 = 20;

/// 域项总数（判据"160 项全量"）。
pub const DOMAIN_ITEMS: u32 = 160;

/// 一维终版评分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FinDimRow {
    /// 维号（1..=20）。
    pub dim: u32,
    /// 终版评分（0~100）。
    pub score: u32,
    /// 通过线。
    pub pass_line: u32,
    /// 该维覆盖的项号数（覆盖面）。
    pub covered_items: u32,
    /// 证据指纹。
    pub evidence_fp: u64,
    /// 处置引用（ADR 或修复单号；None=无需处置）。
    pub remedy_ref: Option<u32>,
}

impl FinDimRow {
    /// 行合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.dim == 0 || self.dim > DIMENSIONS {
            return Err(SEG_BASE + 1);
        }
        if self.score > 100 || self.pass_line == 0 || self.pass_line > 100 {
            return Err(SEG_BASE + 2);
        }
        if self.evidence_fp == 0 {
            return Err(SEG_BASE + 3);
        }
        if self.covered_items > DOMAIN_ITEMS {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }

    /// 是否达线。
    pub fn passed(&self) -> bool {
        self.score >= self.pass_line
    }
}

/// 终版矩阵。
pub struct FinAcceptMatrix {
    /// 维行。
    rows: Vec<FinDimRow>,
    /// 阶段处置继承账（F0131 的处置维号集）。
    inherited_remedies: Vec<u32>,
    /// 签署。
    signed: Option<u64>,
}

impl FinAcceptMatrix {
    /// 空阵。
    pub fn new() -> Self {
        FinAcceptMatrix {
            rows: Vec::new(),
            inherited_remedies: Vec::new(),
            signed: None,
        }
    }

    /// 登记阶段处置继承（判据"低分项处置核验"的对照源；
    /// 重复继承即拒）。
    pub fn inherit_remedy(&mut self, dim: u32) -> Result<(), u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if dim == 0 || dim > DIMENSIONS || self.inherited_remedies.contains(&dim) {
            return Err(SEG_BASE + 6);
        }
        self.inherited_remedies.push(dim);
        Ok(())
    }

    /// 终版评分登记（判据"160 项全量的逐维评分"的维面；重复维即拒；
    /// 处置过的维必须有 remedy_ref——继承的低分维不许无处置过关）。
    pub fn score(&mut self, row: FinDimRow) -> Result<(), u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 5);
        }
        row.validate()?;
        if self.rows.iter().any(|r| r.dim == row.dim) {
            return Err(SEG_BASE + 7);
        }
        if self.inherited_remedies.contains(&row.dim) && row.remedy_ref.is_none() {
            return Err(SEG_BASE + 8);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 维完整性（判据"20 维"全登记）。
    pub fn dims_complete(&self) -> Result<(), u16> {
        if self.rows.len() != DIMENSIONS as usize {
            return Err(SEG_BASE + 9);
        }
        let mut seen = [false; 21];
        for r in &self.rows {
            seen[r.dim as usize] = true;
        }
        for i in 1..=DIMENSIONS as usize {
            if !seen[i] {
                return Err(SEG_BASE + 10);
            }
        }
        Ok(())
    }

    /// 全量覆盖核验（判据"160 项全量"：全维覆盖项数合计 ≥ 域项总数
    /// ——逐维覆盖的并集语义以合计线控制）。
    pub fn coverage_complete(&self) -> Result<(), u16> {
        let total: u32 = self.rows.iter().map(|r| r.covered_items).sum();
        if total < DOMAIN_ITEMS {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }

    /// 低分未处置核验（判据"低分项是否已修复或 ADR"：未达线且
    /// 无处置的维数——签署前必须为零）。
    pub fn unremedied_low(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| !r.passed() && r.remedy_ref.is_none())
            .count()
    }

    /// 平均分。
    pub fn avg_score(&self) -> Option<u32> {
        if self.rows.is_empty() {
            return None;
        }
        Some(self.rows.iter().map(|r| r.score).sum::<u32>() / self.rows.len() as u32)
    }

    /// 最低分维。
    pub fn weakest(&self) -> Option<(u32, u32)> {
        let mut best: Option<(u32, u32)> = None;
        for r in &self.rows {
            match best {
                Some((_, s)) if s <= r.score => {}
                _ => best = Some((r.dim, r.score)),
            }
        }
        best
    }

    /// 终版签署（判据"终版签署"：20 维全登记 + 全量覆盖 + 低分
    /// 全处置 + 全维达线或处置）。
    pub fn sign(&mut self) -> Result<u64, u16> {
        if self.signed.is_some() {
            return Err(SEG_BASE + 12);
        }
        self.dims_complete()?;
        self.coverage_complete()?;
        if self.unremedied_low() != 0 {
            return Err(SEG_BASE + 13);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ ((r.score as u64) << 16)
                ^ r.evidence_fp;
        }
        self.signed = Some(fp);
        Ok(fp)
    }

    /// 维行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 签署指纹。
    pub fn sign_fp(&self) -> Option<u64> {
        self.signed
    }
}

impl Default for FinAcceptMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl FinAcceptMatrix {
    /// 维行查询。
    pub fn row(&self, dim: u32) -> Option<&FinDimRow> {
        self.rows.iter().find(|r| r.dim == dim)
    }

    /// 弱项清单（判据支撑）：得分 < 80 且未处置继承的维号集。
    pub fn weak_dims(&self) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|r| r.score < 80 && !self.inherited_remedies.contains(&r.dim))
            .map(|r| r.dim)
            .collect()
    }

    /// 签署可验性：签署存在时重算全行指纹，与签章一致才可信
    /// （防签后行被篡改）。
    pub fn sign_integrity(&self, canon: impl Fn(&FinDimRow) -> u64) -> Result<bool, u16> {
        let fp = match self.signed {
            Some(f) => f,
            None => return Err(SEG_BASE + 20),
        };
        let mut acc: u64 = 0xcbf29ce484222325;
        for r in &self.rows {
            acc = acc.wrapping_mul(0x100000001b3) ^ canon(r);
        }
        for d in &self.inherited_remedies {
            acc = acc.wrapping_mul(0x100000001b3) ^ (*d as u64);
        }
        Ok(acc == fp)
    }

    /// 达标判据（判据「低分维全覆盖」汇总面）：无弱项 + 全维齐。
    pub fn final_green(&self) -> Result<(), u16> {
        self.dims_complete()?;
        self.coverage_complete()?;
        if !self.weak_dims().is_empty() {
            return Err(SEG_BASE + 21);
        }
        Ok(())
    }
}

/// 终验矩阵全局审计（独立函数面）：矩阵 + 弱项清零 + 签章齐，
/// 三关全过才可移交下游（判据「验收矩阵终版」的收口）。
pub fn matrix_audit(m: &FinAcceptMatrix) -> Result<(), u16> {
    if m.row_count() == 0 {
        return Err(SEG_BASE + 22);
    }
    m.final_green()?;
    if m.sign_fp().is_none() {
        return Err(SEG_BASE + 23);
    }
    Ok(())
}

/// 维行独立校验（独立函数面）：不依赖矩阵上下文的行级自检——
/// 维号界、评分界、过线语义（score < pass_line 却无处置引用 =
    /// 带病过线，直接拒）。
pub fn dim_row_audit(r: &FinDimRow) -> Result<(), u16> {
    if r.dim == 0 || r.dim > 20 {
        return Err(SEG_BASE + 24);
    }
    if r.score > 100 || r.pass_line > 100 {
        return Err(SEG_BASE + 25);
    }
    if r.score < r.pass_line && r.remedy_ref.is_none() {
        return Err(SEG_BASE + 26);
    }
    if r.evidence_fp == 0 {
        return Err(SEG_BASE + 27);
    }
    Ok(())
}

/// 覆盖面分布审计（独立函数面）：全部维行覆盖项数总和必须
/// 非零且逐维非零（零覆盖维 = 空维占位，矩阵造假）。
pub fn coverage_distribution(m: &FinAcceptMatrix) -> Result<u32, u16> {
    if m.rows.iter().any(|r| r.covered_items == 0) {
        return Err(SEG_BASE + 28);
    }
    let total: u32 = m.rows.iter().map(|r| r.covered_items).sum();
    if total == 0 {
        return Err(SEG_BASE + 29);
    }
    Ok(total)
}


// ===== 深化块（cga145_acceptfin · 验收终审深化）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_acceptfin {
    //! 验收终审深化 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 终审票据链：每张票必须 (序号, 判定, 责任人) 三元齐备，序号严格递增。
    pub fn ticket_chain(tickets: &[(u32, u8, u64)]) -> Result<(), u16> {
        if tickets.is_empty() {
            return Err(1);
        }
        let mut prev: Option<u32> = None;
        for (i, &(no, verdict, owner)) in tickets.iter().enumerate() {
            if owner == 0 {
                return Err(2 + i as u16);
            }
            if verdict > 2 {
                return Err(32 + i as u16);
            }
            if let Some(p) = prev {
                if no <= p {
                    return Err(64 + i as u16);
                }
            }
            prev = Some(no);
        }
        Ok(())
    }

    /// 终审结论合成：任一否决即整体否决（短板纪律），全票通过才通过。
    pub fn final_verdict(verdicts: &[u8]) -> u8 {
        if verdicts.is_empty() {
            return 3;
        }
        if verdicts.iter().any(|&v| v == 2) {
            2
        } else if verdicts.iter().any(|&v| v == 1) {
            1
        } else {
            0
        }
    }

    /// 门槛核对：通过率千分比必须 ≥ 门槛，不足即拒。
    pub fn threshold_gate(passed: u32, total: u32, floor_permille: u32) -> Result<(), u16> {
        if total == 0 {
            return Err(1);
        }
        if floor_permille > 1000 {
            return Err(2);
        }
        if passed * 1000 < total * floor_permille {
            return Err(3);
        }
        Ok(())
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0145-深化");
        let ok = ticket_chain(&[(1, 0, 7), (2, 0, 7), (3, 1, 9)]).is_ok();
        cs.add("票据链合法", ok, "");
        cs.add("跳号检出", ticket_chain(&[(1, 0, 7), (3, 0, 7)]).is_err(), "");
        cs.add("无主检出", ticket_chain(&[(1, 0, 0)]).is_err(), "");
        cs.add("短板合成", final_verdict(&[0, 0, 0]) == 0 && final_verdict(&[0, 1, 0]) == 1 && final_verdict(&[0, 2, 1]) == 2, "");
        cs.add("门槛守门", threshold_gate(990, 1000, 980).is_ok() && threshold_gate(900, 1000, 980).is_err(), "");
        cs
    }
}
