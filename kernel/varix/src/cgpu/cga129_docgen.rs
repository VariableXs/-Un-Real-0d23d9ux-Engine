//! cga129_docgen — CGPU-F0129 A 域文档生成。
//!
//! 判据原文（施工书锚点 CGPU-F0129）：A 域文档生成（F0386 范式）：算子
//! API 文档/线程池文档/确定性纪律文档/微架构调优文档，双格式，警告
//! 门槛（无注释算子=警告）。判据：生成物零漂移、四类文档、双格式、
//! 门槛、更新。
//!
//! **设计定盘**：生成器账面化：四类文档闭集（算子 API/线程池/确定性
//! 纪律/微架构调优），每类登记章节清单与源锚指纹。生成物零漂移 =
//! 文档声明的源锚指纹与代码落点指纹逐对一致（漂移即红）。双格式 =
//! 每类文档必须同时有 md 与 html 两个格式条目（单格式即拒）。警告
//! 门槛 = 无注释算子清单非空时生成「警告档」产物（不阻断但显性
//! 列出——无注释算子是债务不是错误）。更新 = 增量再生成：源锚指纹
//! 变化的章节才重生成（生成次数计数可审计）。
//!
//! 诊断码段 0x01F0~0x01FF（0x01 段第 16 子区，0x01 段满额收官）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA129_VERSION: &str = "cga129-docgen-v1";

/// 诊断子区基值（0x01 段第 16 子区）。
pub const SEG_BASE: u16 = 0x01F0;

/// 四类文档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    /// 算子 API。
    OpApi,
    /// 线程池。
    ThreadPool,
    /// 确定性纪律。
    Determinism,
    /// 微架构调优。
    MicroArch,
}

impl DocKind {
    /// 全集（稳定序）。
    pub const ALL: [DocKind; 4] = [
        DocKind::OpApi,
        DocKind::ThreadPool,
        DocKind::Determinism,
        DocKind::MicroArch,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            DocKind::OpApi => 0,
            DocKind::ThreadPool => 1,
            DocKind::Determinism => 2,
            DocKind::MicroArch => 3,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 格式闭集（判据"双格式"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    /// Markdown。
    Md,
    /// HTML。
    Html,
}

/// 一条生成产物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenArtifact {
    /// 文档类。
    pub kind: DocKind,
    /// 格式。
    pub fmt: Fmt,
    /// 产物指纹。
    pub out_fp: u64,
}

/// 生成账。
pub struct DocGenLedger {
    /// 产物册。
    artifacts: Vec<GenArtifact>,
    /// 源锚指纹对（章节锚 → 代码指纹）。
    src_anchors: Vec<(&'static str, u64)>,
    /// 文档声明指纹对（章节锚 → 文档指纹）。
    doc_anchors: Vec<(&'static str, u64)>,
    /// 无注释算子清单（警告门槛）。
    undoc_ops: Vec<u32>,
    /// 再生成计数。
    regen_count: u32,
    /// 封账。
    sealed: Option<u64>,
}

impl DocGenLedger {
    /// 空账。
    pub fn new() -> Self {
        DocGenLedger {
            artifacts: Vec::new(),
            src_anchors: Vec::new(),
            doc_anchors: Vec::new(),
            undoc_ops: Vec::new(),
            regen_count: 0,
            sealed: None,
        }
    }

    /// 登记产物（判据"双格式"：同类同格式重复即拒；指纹非零拒零）。
    pub fn add_artifact(&mut self, kind: DocKind, fmt: Fmt, out_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if out_fp == 0 {
            return Err(SEG_BASE + 6);
        }
        if self
            .artifacts
            .iter()
            .any(|a| a.kind == kind && a.fmt == fmt)
        {
            return Err(SEG_BASE + 7);
        }
        self.artifacts.push(GenArtifact {
            kind,
            fmt,
            out_fp,
        });
        Ok(())
    }

    /// 源锚登记（判据"零漂移"：锚重复即拒）。
    pub fn add_source(&mut self, anchor: &'static str, code_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if anchor.is_empty() {
            return Err(SEG_BASE + 8);
        }
        if self.src_anchors.iter().any(|(a, _)| *a == anchor) {
            return Err(SEG_BASE + 9);
        }
        self.src_anchors.push((anchor, code_fp));
        Ok(())
    }

    /// 文档声明登记（与源锚同锚；漂移在 drift_check 显性）。
    pub fn declare_doc(&mut self, anchor: &'static str, doc_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if !self.src_anchors.iter().any(|(a, _)| *a == anchor) {
            return Err(SEG_BASE + 10);
        }
        if self.doc_anchors.iter().any(|(a, _)| *a == anchor) {
            return Err(SEG_BASE + 11);
        }
        self.doc_anchors.push((anchor, doc_fp));
        Ok(())
    }

    /// 零漂移核验：全部锚的文档指纹 == 代码指纹。
    pub fn drift_check(&self) -> Result<(), u16> {
        for (a, dfp) in &self.doc_anchors {
            let sfp = self
                .src_anchors
                .iter()
                .find(|(s, _)| s == a)
                .map(|(_, f)| *f)
                .unwrap_or(0);
            if sfp != *dfp {
                return Err(SEG_BASE + 12);
            }
        }
        Ok(())
    }

    /// 警告门槛登记（判据"门槛"：无注释算子登记；重复即拒）。
    pub fn flag_undoc(&mut self, op: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if op == 0 || self.undoc_ops.contains(&op) {
            return Err(SEG_BASE + 13);
        }
        self.undoc_ops.push(op);
        Ok(())
    }

    /// 增量再生成（判据"更新"：只对漂移锚重生成——返回需重生成的
    /// 锚数并累加计数）。
    pub fn regen_drifted(&mut self) -> Result<usize, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        let mut n = 0usize;
        for (a, dfp) in &self.doc_anchors {
            let sfp = self
                .src_anchors
                .iter()
                .find(|(s, _)| s == a)
                .map(|(_, f)| *f)
                .unwrap_or(0);
            if sfp != *dfp {
                n += 1;
            }
        }
        self.regen_count += 1;
        Ok(n)
    }

    /// 再生成计数。
    pub fn regen_total(&self) -> u32 {
        self.regen_count
    }

    /// 双格式完整性（四类 × 双格式八格全满）。
    pub fn formats_complete(&self) -> Result<(), u16> {
        for k in DocKind::ALL {
            for f in [Fmt::Md, Fmt::Html] {
                if !self
                    .artifacts
                    .iter()
                    .any(|a| a.kind == k && a.fmt == f)
                {
                    return Err(k.code());
                }
            }
        }
        Ok(())
    }

    /// 产物数。
    pub fn artifact_count(&self) -> usize {
        self.artifacts.len()
    }

    /// 封账（判据五面：双格式齐 + 零漂移 + 四类全有）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 14);
        }
        self.formats_complete()?;
        self.drift_check()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for a in &self.artifacts {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ a.out_fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for DocGenLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 漂移锚投影行（判据"零漂移"的处置面：逐锚列出双指纹供修复定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftRow {
    /// 章节锚。
    pub anchor: &'static str,
    /// 代码指纹。
    pub code_fp: u64,
    /// 文档声明指纹。
    pub doc_fp: u64,
}

impl DocGenLedger {
    /// 漂移清单（drift_check 的枚举面：全部漂移锚逐行列出——
    /// 首错即停的 drift_check 之外，给收口会话一次看全）。
    pub fn drift_rows(&self) -> Vec<DriftRow> {
        let mut out: Vec<DriftRow> = Vec::new();
        for (a, dfp) in &self.doc_anchors {
            let sfp = self
                .src_anchors
                .iter()
                .find(|(s, _)| s == a)
                .map(|(_, f)| *f)
                .unwrap_or(0);
            if sfp != *dfp {
                out.push(DriftRow { anchor: a, code_fp: sfp, doc_fp: *dfp });
            }
        }
        out
    }

    /// 警告档投影（判据"门槛"：无注释算子清单的只读投影）。
    pub fn undoc_list(&self) -> Vec<u32> {
        self.undoc_ops.clone()
    }

    /// 警告是否清零（判据"警告清零"的终态面）。
    pub fn warnings_clear(&self) -> bool {
        self.undoc_ops.is_empty()
    }

    /// 类别产物计数（四类各几件——双格式下应为偶数）。
    pub fn kind_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for a in &self.artifacts {
            out[a.kind.ordinal() as usize] += 1;
        }
        out
    }

    /// 源锚数。
    pub fn source_count(&self) -> usize {
        self.src_anchors.len()
    }

    /// 封账指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}
