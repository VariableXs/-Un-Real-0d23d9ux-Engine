//! cga143_docfinal — CGPU-F0143 A 域文档终版。
//!
//! 判据原文（施工书锚点 CGPU-F0143）：A 域文档终版生成（F0129 的
//! 收官版）：全算子文档/架构/教学/示例的终版输出，文档与实现一致性
//! 终审（零漂移终验）。判据：四类终版、零漂移、双格式、警告清零、
//! 版本对齐。
//!
//! **设计定盘**：终版账 = F0129 四类（算子 API/线程池/确定性纪律/
//! 微架构调优）+ 收官新增两类（教学/示例——F0121/122 的产出收进
//! 终版）= 六类闭集。终版语义 = 终版条目一旦登记，内容指纹冻结
//! （终版不可再生——再生即新版本事件，终版账面拒绝改写）。零漂移
//! 终验 = 全部终版条目的实现指纹对齐（漂移条目清单必须为空）。双
//! 格式 = 六类全部有 md+html 两格式条目。警告清零 = 无注释算子
//! 警告在终版时必须为零。版本对齐 = 全部条目声明同一域版本号。
//!
//! 诊断码段 0x02E0~0x02EF（0x02 段第 15 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA143_VERSION: &str = "cga143-docfinal-v1";

/// 诊断子区基值（0x02 段第 15 子区）。
pub const SEG_BASE: u16 = 0x02E0;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 六类文档闭集（判据"四类终版"+ 教学示例收官并卷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalDocKind {
    /// 算子 API。
    OpApi,
    /// 线程池。
    ThreadPool,
    /// 确定性纪律。
    DetDiscipline,
    /// 微架构调优。
    MicroArch,
    /// 教学。
    Tutorial,
    /// 示例。
    Examples,
}

impl FinalDocKind {
    /// 全集稳定序。
    pub const ALL: [FinalDocKind; 6] = [
        FinalDocKind::OpApi,
        FinalDocKind::ThreadPool,
        FinalDocKind::DetDiscipline,
        FinalDocKind::MicroArch,
        FinalDocKind::Tutorial,
        FinalDocKind::Examples,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            FinalDocKind::OpApi => 0,
            FinalDocKind::ThreadPool => 1,
            FinalDocKind::DetDiscipline => 2,
            FinalDocKind::MicroArch => 3,
            FinalDocKind::Tutorial => 4,
            FinalDocKind::Examples => 5,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 格式闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    /// Markdown。
    Md,
    /// HTML。
    Html,
}

/// 终版条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FinalEntry {
    /// 类别。
    pub kind: FinalDocKind,
    /// 格式。
    pub fmt: Fmt,
    /// 内容指纹（冻结面）。
    pub content_fp: u64,
    /// 对应实现指纹（零漂移终验的第二半）。
    pub impl_fp: u64,
}

/// 终版账。
pub struct DocFinalLedger {
    /// 终版条目。
    entries: Vec<FinalEntry>,
    /// 无注释算子（终版时必须清零）。
    undoc_ops: Vec<u32>,
    /// 域版本号（全部条目须对齐）。
    domain_ver: Option<u32>,
    /// 封版。
    sealed: Option<u64>,
}

impl DocFinalLedger {
    /// 空账。
    pub fn new() -> Self {
        DocFinalLedger {
            entries: Vec::new(),
            undoc_ops: Vec::new(),
            domain_ver: None,
            sealed: None,
        }
    }

    /// 域版本登记（判据"版本对齐"；重复登记即拒）。
    pub fn set_domain_ver(&mut self, ver: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if ver == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.domain_ver.is_some() {
            return Err(SEG_BASE + 6);
        }
        self.domain_ver = Some(ver);
        Ok(())
    }

    /// 终版条目登记（判据"终版语义"：同类同格式重复即拒；
    /// 内容指纹冻结——登记即终版，改写不允许）。
    pub fn add_entry(
        &mut self,
        kind: FinalDocKind,
        fmt: Fmt,
        content_fp: u64,
        impl_fp: u64,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if content_fp == 0 || impl_fp == 0 {
            return Err(SEG_BASE + 7);
        }
        if self
            .entries
            .iter()
            .any(|e| e.kind == kind && e.fmt == fmt)
        {
            return Err(SEG_BASE + 8);
        }
        self.entries.push(FinalEntry {
            kind,
            fmt,
            content_fp,
            impl_fp,
        });
        Ok(())
    }

    /// 无注释算子登记（重复即拒）。
    pub fn flag_undoc(&mut self, op: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if op == 0 || self.undoc_ops.contains(&op) {
            return Err(SEG_BASE + 9);
        }
        self.undoc_ops.push(op);
        Ok(())
    }

    /// 无注释清零（判据"警告清零"：修复后核销）。
    pub fn clear_undoc(&mut self, op: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        let pos = match self.undoc_ops.iter().position(|x| *x == op) {
            Some(p) => p,
            None => return Err(SEG_BASE + 10),
        };
        self.undoc_ops.remove(pos);
        Ok(())
    }

    /// 漂移清单（判据"零漂移"：内容指纹 ≠ 实现指纹的条目）。
    pub fn drifted(&self) -> Vec<FinalDocKind> {
        let mut out: Vec<FinalDocKind> = Vec::new();
        for e in &self.entries {
            if e.content_fp != e.impl_fp {
                if !out.contains(&e.kind) {
                    out.push(e.kind);
                }
            }
        }
        out
    }

    /// 双格式完整性（判据"双格式"：六类 × 双格式全满）。
    pub fn formats_complete(&self) -> Result<(), u16> {
        for k in FinalDocKind::ALL {
            for f in [Fmt::Md, Fmt::Html] {
                if !self
                    .entries
                    .iter()
                    .any(|e| e.kind == k && e.fmt == f)
                {
                    return Err(k.code());
                }
            }
        }
        Ok(())
    }

    /// 零漂移终验。
    pub fn zero_drift(&self) -> Result<(), u16> {
        if self.drifted().is_empty() {
            Ok(())
        } else {
            Err(SEG_BASE + 11)
        }
    }

    /// 条目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 无注释数。
    pub fn undoc_count(&self) -> usize {
        self.undoc_ops.len()
    }

    /// 封版（判据五面：六类双格式齐 + 零漂移 + 警告清零 + 版本对齐）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 12);
        }
        self.formats_complete()?;
        self.zero_drift()?;
        if !self.undoc_ops.is_empty() {
            return Err(SEG_BASE + 13);
        }
        if self.domain_ver.is_none() {
            return Err(SEG_BASE + 14);
        }
        let mut fp = FNV_OFFSET;
        for e in &self.entries {
            fp = fp.wrapping_mul(FNV_PRIME) ^ e.content_fp ^ e.impl_fp;
        }
        fp = fp.wrapping_mul(FNV_PRIME) ^ self.domain_ver.unwrap_or(0) as u64;
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for DocFinalLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl DocFinalLedger {
    /// 类别计数投影（判据"四类终版"的账面：六类各几条——双格式下
    /// 全为 2）。
    pub fn kind_census(&self) -> [usize; 6] {
        let mut out = [0usize; 6];
        for e in &self.entries {
            out[e.kind.ordinal() as usize] += 1;
        }
        out
    }

    /// 漂移类数。
    pub fn drift_count(&self) -> usize {
        self.drifted().len()
    }

    /// 域版本读取。
    pub fn domain_version(&self) -> Option<u32> {
        self.domain_ver
    }

    /// 无注释投影（只读）。
    pub fn undoc_list(&self) -> &Vec<u32> {
        &self.undoc_ops
    }

    /// 封版指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}
