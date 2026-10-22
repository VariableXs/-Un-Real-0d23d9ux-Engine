//! cga140_midmile — CGPU-F0140 A 域阶段性终验与中期里程碑。
//!
//! 判据原文（施工书锚点 CGPU-F0140）：A 域阶段性终验（140/160，87.5%
//! 检查点）：三层阶段验证（文档 140 项在册/代码一致性套件全绿/软渲
//! 全链走查）+ 中期里程碑登记（A 域主体完成宣告——剩余 F0141-F0160
//! 为域收尾批，含真正的域级终验 F0160）。阶段报告呈 Variable 确认
//! 归档。判据（补）：140 项三层验证、中期里程碑登记、剩余 20 项计划
//! 显性、F0160 终验预留声明。判据：三层终验、里程碑登记、报告确认、
//! 进度对账、剩余计划。
//!
//! **设计定盘**：阶段终验三层账：①文档层（140 项在册核验——项号
//! 集合 1..=140 入册零缺）；②代码层（一致性套件全绿账——套件清单
//! 逐项绿）；③体验层（软渲全链走查的走查单账——走查项全过）。中期
//! 里程碑 = 三层全过后落里程碑签章。剩余计划 = F0141-F0160 共 20 项
//! 的显性收尾清单。终验预留 = F0160 的预留声明（防阶段终验被误当
//! 域级终验）。报告 = 三层账 + 里程碑签的链式指纹呈档。
//!
//! 诊断码段 0x02B0~0x02BF（0x02 段第 12 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA140_VERSION: &str = "cga140-midmile-v1";

/// 诊断子区基值（0x02 段第 12 子区）。
pub const SEG_BASE: u16 = 0x02B0;

/// 阶段项数（判据"140/160"）。
pub const PHASE_ITEMS: u32 = 140;

/// 域总项数。
pub const DOMAIN_ITEMS: u32 = 160;

/// 剩余收尾项数。
pub const REMAIN_ITEMS: u32 = DOMAIN_ITEMS - PHASE_ITEMS;

/// 三层闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyLayer {
    /// 文档层。
    Docs,
    /// 代码层。
    Code,
    /// 体验层。
    Experience,
}

impl VerifyLayer {
    /// 全集稳定序。
    pub const ALL: [VerifyLayer; 3] = [
        VerifyLayer::Docs,
        VerifyLayer::Code,
        VerifyLayer::Experience,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            VerifyLayer::Docs => 0,
            VerifyLayer::Code => 1,
            VerifyLayer::Experience => 2,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 阶段终验账。
pub struct MidMileLedger {
    /// ① 文档在册项号集。
    doc_items: Vec<u32>,
    /// ② 代码一致性套件（套件号, 是否绿）。
    suites: Vec<(u32, bool)>,
    /// ③ 软渲走查单（走查项号, 是否过）。
    walkthrough: Vec<(u32, bool)>,
    /// 里程碑签章。
    milestone: Option<u64>,
    /// 剩余计划（1..=20）。
    remain: Vec<u32>,
    /// F0160 终验预留声明。
    final_reserved: bool,
    /// 报告指纹（呈档后）。
    report_fp: Option<u64>,
}

impl MidMileLedger {
    /// 空账。
    pub fn new() -> Self {
        MidMileLedger {
            doc_items: Vec::new(),
            suites: Vec::new(),
            walkthrough: Vec::new(),
            milestone: None,
            remain: Vec::new(),
            final_reserved: false,
            report_fp: None,
        }
    }

    /// 文档在册登记（判据"文档 140 项在册"：项号 1..=140 不重号）。
    pub fn plant_doc(&mut self, item: u32) -> Result<(), u16> {
        if self.milestone.is_some() {
            return Err(SEG_BASE + 4);
        }
        if item == 0 || item > PHASE_ITEMS {
            return Err(SEG_BASE + 5);
        }
        if self.doc_items.contains(&item) {
            return Err(SEG_BASE + 6);
        }
        self.doc_items.push(item);
        Ok(())
    }

    /// 文档层核验（零缺号）。
    pub fn docs_complete(&self) -> Result<(), u16> {
        if self.doc_items.len() != PHASE_ITEMS as usize {
            return Err(SEG_BASE + 7);
        }
        let mut seen = [false; 141];
        for &i in &self.doc_items {
            seen[i as usize] = true;
        }
        for i in 1..=PHASE_ITEMS as usize {
            if !seen[i] {
                return Err(SEG_BASE + 8);
            }
        }
        Ok(())
    }

    /// 代码套件登记（判据"一致性套件全绿"：重复套件号即拒）。
    pub fn add_suite(&mut self, suite: u32, green: bool) -> Result<(), u16> {
        if self.milestone.is_some() {
            return Err(SEG_BASE + 4);
        }
        if suite == 0 || self.suites.iter().any(|(s, _)| *s == suite) {
            return Err(SEG_BASE + 9);
        }
        self.suites.push((suite, green));
        Ok(())
    }

    /// 代码层核验（全部绿）。
    pub fn code_green(&self) -> Result<(), u16> {
        if self.suites.is_empty() {
            return Err(SEG_BASE + 10);
        }
        for (s, g) in &self.suites {
            if !g {
                return Err(SEG_BASE + 11);
            }
            let _ = s;
        }
        Ok(())
    }

    /// 走查项登记（判据"软渲全链走查"；重复项即拒）。
    pub fn add_walkthrough(&mut self, item: u32, passed: bool) -> Result<(), u16> {
        if self.milestone.is_some() {
            return Err(SEG_BASE + 4);
        }
        if item == 0 || self.walkthrough.iter().any(|(i, _)| *i == item) {
            return Err(SEG_BASE + 12);
        }
        self.walkthrough.push((item, passed));
        Ok(())
    }

    /// 体验层核验（全部过）。
    pub fn walkthrough_green(&self) -> Result<(), u16> {
        if self.walkthrough.is_empty() {
            return Err(SEG_BASE + 13);
        }
        for (_, p) in &self.walkthrough {
            if !p {
                return Err(SEG_BASE + 14);
            }
        }
        Ok(())
    }

    /// 剩余计划登记（判据"剩余 20 项计划显性"：号 1..=20 不重号）。
    pub fn plan_remain(&mut self, item: u32) -> Result<(), u16> {
        if self.milestone.is_some() {
            return Err(SEG_BASE + 4);
        }
        if item == 0 || item > REMAIN_ITEMS {
            return Err(SEG_BASE + 15);
        }
        if self.remain.contains(&item) {
            return Err(SEG_BASE + 16);
        }
        self.remain.push(item);
        Ok(())
    }

    /// 终验预留声明（判据"F0160 终验预留声明"；重复声明即拒）。
    pub fn reserve_final(&mut self) -> Result<(), u16> {
        if self.milestone.is_some() {
            return Err(SEG_BASE + 4);
        }
        if self.final_reserved {
            return Err(SEG_BASE + 17);
        }
        self.final_reserved = true;
        Ok(())
    }

    /// 中期里程碑（判据"里程碑登记"：三层全过 + 计划 20 项齐 +
    /// 预留声明，才可落签）。
    pub fn set_milestone(&mut self) -> Result<u64, u16> {
        if self.milestone.is_some() {
            return Err(SEG_BASE + 18);
        }
        self.docs_complete()?;
        self.code_green()?;
        self.walkthrough_green()?;
        if self.remain.len() != REMAIN_ITEMS as usize {
            return Err(SEG_BASE + 19);
        }
        if !self.final_reserved {
            return Err(SEG_BASE + 20);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for &i in &self.doc_items {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ i as u64;
        }
        for (s, _) in &self.suites {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ *s as u64;
        }
        self.milestone = Some(fp);
        Ok(fp)
    }

    /// 报告呈档（判据"报告确认归档"：里程碑落签后呈档）。
    pub fn file_report(&mut self, body: &[u8]) -> Result<u64, u16> {
        if self.report_fp.is_some() {
            return Err(SEG_BASE + 21);
        }
        let m = self.milestone.ok_or(SEG_BASE + 22)?;
        if body.is_empty() {
            return Err(SEG_BASE + 23);
        }
        let mut fp = m;
        for &b in body {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ b as u64;
        }
        self.report_fp = Some(fp);
        Ok(fp)
    }

    /// 阶段进度（87.5% 的千分比账面）。
    pub fn phase_permille(&self) -> u64 {
        PHASE_ITEMS as u64 * 1000 / DOMAIN_ITEMS as u64
    }

    /// 里程碑签章。
    pub fn milestone_fp(&self) -> Option<u64> {
        self.milestone
    }
}

impl Default for MidMileLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl MidMileLedger {
    /// 三层进度投影（判据"三层阶段验证"的账面：各层已收条数）。
    pub fn layer_census(&self) -> (usize, usize, usize) {
        (
            self.doc_items.len(),
            self.suites.len(),
            self.walkthrough.len(),
        )
    }

    /// 文档缺项清单（判据"140 项在册"的定位面）。
    pub fn missing_docs(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut seen = [false; 141];
        for &i in &self.doc_items {
            seen[i as usize] = true;
        }
        for i in 1..=PHASE_ITEMS as usize {
            if !seen[i] {
                out.push(i as u32);
            }
        }
        out
    }

    /// 红套件清单（判据"代码一致性套件全绿"的定位面）。
    pub fn red_suites(&self) -> Vec<u32> {
        self.suites
            .iter()
            .filter(|(_, g)| !g)
            .map(|(s, _)| *s)
            .collect()
    }

    /// 未过走查项清单。
    pub fn failed_walkthrough(&self) -> Vec<u32> {
        self.walkthrough
            .iter()
            .filter(|(_, p)| !p)
            .map(|(i, _)| *i)
            .collect()
    }

    /// 剩余计划进度（20 项中已登记数）。
    pub fn remain_planned(&self) -> usize {
        self.remain.len()
    }

    /// 预留已声明。
    pub fn final_is_reserved(&self) -> bool {
        self.final_reserved
    }

    /// 报告指纹。
    pub fn report(&self) -> Option<u64> {
        self.report_fp
    }
}
