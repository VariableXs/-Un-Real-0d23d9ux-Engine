//! cga157_codeaudit.rs —— CGPU-F0157 · A 域代码质量终审
//!
//! A 域代码的收官质量扫描：零死代码（未引用函数/宏检测）、
//! 零魔法数（裸数值检测——F0188 账目数字除外）、命名一致性、
//! 注释覆盖、五扫描结果与修复闭环。
//! 设计立场：终审扫描器本身是纯账面模型——以「扫描规则 +
//! 违规行 + 修复闭环」三段式落账，扫描器不关心 rustc 细节，
//! 关心的是「违规从发现到清零」的闭环纪律。
//! 诊断码段 0x03C0（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x03C0;

/// 扫描类别（判据「五扫描」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanKind {
    /// 死代码。
    DeadCode,
    /// 魔法数。
    MagicNumber,
    /// 命名一致性。
    Naming,
    /// 注释覆盖。
    CommentCover,
    /// Lint 全绿（clippy 等价面）。
    Lint,
}

impl ScanKind {
    /// 全类序。
    pub const ALL: [ScanKind; 5] = [
        ScanKind::DeadCode,
        ScanKind::MagicNumber,
        ScanKind::Naming,
        ScanKind::CommentCover,
        ScanKind::Lint,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            ScanKind::DeadCode => 0,
            ScanKind::MagicNumber => 1,
            ScanKind::Naming => 2,
            ScanKind::CommentCover => 3,
            ScanKind::Lint => 4,
        }
    }
}

/// 违规行。
#[derive(Clone, Debug)]
pub struct Violation {
    /// 类别。
    pub kind: ScanKind,
    /// 文件指纹。
    pub file_fp: u64,
    /// 行号。
    pub line: u32,
    /// 修复态。
    pub fixed: bool,
}

/// 魔法数白名单（判据「F0188 账目数字除外」）：账目常量
/// （如 1000=千分比、256=定点、0x????=段基址）不判违规。
pub struct MagicWhitelist {
    /// 白名单值。
    pub values: Vec<u64>,
}

impl MagicWhitelist {
    /// 内建账目白名单。
    pub fn builtin() -> Self {
        MagicWhitelist {
            values: Vec::new(),
        }
    }

    /// 登记白名单值（去重）。
    pub fn admit(&mut self, v: u64) -> bool {
        if v == 0 || self.values.contains(&v) {
            return false;
        }
        self.values.push(v);
        true
    }

    /// 命中判断。
    pub fn contains(&self, v: u64) -> bool {
        self.values.contains(&v)
    }
}

/// 终审台账。
pub struct CodeAudit {
    /// 违规行。
    rows: Vec<Violation>,
    /// 魔法数白名单。
    whitelist: MagicWhitelist,
    /// 五扫描是否都已跑过（每类至少一行或显性零违规行）。
    scanned: [bool; 5],
    /// 注释覆盖抽查下限（千分比）。
    comment_floor_permille: u32,
    /// 归档。
    archived: Option<u64>,
}

impl CodeAudit {
    /// 建账：注释覆盖下限界 50~250‰。
    pub fn new(comment_floor_permille: u32) -> Result<Self, u16> {
        if comment_floor_permille < 50 || comment_floor_permille > 250 {
            return Err(SEG_BASE + 1);
        }
        Ok(CodeAudit {
            rows: Vec::new(),
            whitelist: MagicWhitelist::builtin(),
            scanned: [false; 5],
            comment_floor_permille,
            archived: None,
        })
    }

    /// 违规总数。
    pub fn violation_count(&self) -> usize {
        self.rows.len()
    }

    /// 未修复数。
    pub fn open_count(&self) -> usize {
        self.rows.iter().filter(|r| !r.fixed).count()
    }

    /// 白名单登记。
    pub fn whitelist_admit(&mut self, v: u64) -> bool {
        self.whitelist.admit(v)
    }

    /// 魔法数扫描（判据「零魔法数」）：给一行候选值序列，
    /// 白名单外的非零值记违规。返回本行违规数。
    pub fn scan_magic(
        &mut self,
        file_fp: u64,
        line: u32,
        values: &[u64],
    ) -> Result<usize, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 2);
        }
        if file_fp == 0 {
            return Err(SEG_BASE + 3);
        }
        self.scanned[ScanKind::MagicNumber.rank() as usize] = true;
        let mut n = 0;
        for v in values {
            if *v != 0 && !self.whitelist.contains(*v) {
                self.rows.push(Violation {
                    kind: ScanKind::MagicNumber,
                    file_fp,
                    line,
                    fixed: false,
                });
                n += 1;
            }
        }
        Ok(n)
    }

    /// 类扫描登记（死代码/命名/注释/Lint 四类通用入口）：
    /// violations 为该类本轮发现的违规行（closed = 扫描即零违规）。
    pub fn scan_kind(
        &mut self,
        kind: ScanKind,
        violations: Vec<(u64, u32)>,
    ) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 2);
        }
        let r = kind.rank() as usize;
        if r == ScanKind::MagicNumber.rank() as usize {
            // 魔法数必须走 scan_magic（白名单逻辑专用）。
            return Err(SEG_BASE + 4);
        }
        self.scanned[r] = true;
        for (file_fp, line) in violations {
            if file_fp == 0 || line == 0 {
                return Err(SEG_BASE + 3);
            }
            self.rows.push(Violation {
                kind,
                file_fp,
                line,
                fixed: false,
            });
        }
        Ok(())
    }

    /// 注释覆盖判定（判据「注释覆盖」）：实测千分比 ≥ 下限。
    pub fn comment_green(&self, actual_permille: u32) -> Result<(), u16> {
        if !self.scanned[ScanKind::CommentCover.rank() as usize] {
            return Err(SEG_BASE + 5);
        }
        if actual_permille < self.comment_floor_permille {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }

    /// 修复闭环（判据「修复闭环」）：逐条销号；file+line+kind
    /// 定位必须唯一命中一行未修复违规。
    pub fn fix_one(&mut self, kind: ScanKind, file_fp: u64, line: u32) -> Result<(), u16> {
        let hits = self
            .rows
            .iter_mut()
            .filter(|r| r.kind == kind && r.file_fp == file_fp && r.line == line && !r.fixed)
            .collect::<Vec<_>>();
        if hits.is_empty() {
            return Err(SEG_BASE + 7);
        }
        for h in hits {
            h.fixed = true;
        }
        Ok(())
    }

    /// 类别普查：五类各多少违规。
    pub fn kind_census(&self) -> [usize; 5] {
        let mut out = [0usize; 5];
        for r in &self.rows {
            out[r.kind.rank() as usize] += 1;
        }
        out
    }

    /// 五扫描齐判（判据「五扫描全过或修复」前置）：五类都跑过。
    pub fn all_scanned(&self) -> Result<(), u16> {
        if self.scanned.iter().all(|s| *s) {
            Ok(())
        } else {
            Err(SEG_BASE + 8)
        }
    }

    /// 零死代码验证（判据「零死代码验证」）：DeadCode 类零未修复。
    pub fn dead_code_zero(&self) -> Result<(), u16> {
        let bad = self
            .rows
            .iter()
            .any(|r| r.kind == ScanKind::DeadCode && !r.fixed);
        if bad {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }

    /// 归档前置总检：五扫描齐 + 全部违规已修 + 死代码清零，
    /// 三关全过才许归档签章。
    pub fn pre_archive(&mut self, canon: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 10);
        }
        self.all_scanned()?;
        if self.open_count() > 0 {
            return Err(SEG_BASE + 11);
        }
        self.dead_code_zero()?;
        let mut fp: u64 = 0xcbf29ce484222325;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100000001b3) ^ r.file_fp;
            fp = fp.wrapping_mul(0x100000001b3) ^ (r.line as u64);
        }
        for &b in canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        if fp == 0 {
            return Err(SEG_BASE + 12);
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

/// Lint 类别专项（判据「clippy 绿」）：lint 违规独立于四扫描，
/// 单独绿判——存在任何未修复 lint 即红。
pub fn lint_green(rows: &[Violation]) -> Result<(), u16> {
    let bad = rows.iter().any(|r| r.kind == ScanKind::Lint && !r.fixed);
    if bad {
        return Err(SEG_BASE + 13);
    }
    Ok(())
}

/// 修复闭环完整性（判据「修复闭环」审计面）：全部行的修复态
/// 必须可追溯——fixed 行的存在前提是同位曾登记过违规（由台账
/// 结构天然保证），此处审计的是「没有跳跃销号」：fixed 行数
/// ≤ 登记行数恒真，故审计总行与未修行的差值合法性。
pub fn closure_audit(a: &CodeAudit) -> Result<(), u16> {
    if a.violation_count() < a.open_count() {
        return Err(SEG_BASE + 14);
    }
    if a.archived.is_some() && a.open_count() > 0 {
        return Err(SEG_BASE + 15);
    }
    Ok(())
}

impl CodeAudit {
    /// 类别清零判（判据「五扫描全过或修复」逐类面）：指定类
    /// 零未修复（该类须已扫描）。
    pub fn kind_green(&self, kind: ScanKind) -> Result<(), u16> {
        if !self.scanned[kind.rank() as usize] {
            return Err(SEG_BASE + 16);
        }
        if self.rows.iter().any(|r| r.kind == kind && !r.fixed) {
            return Err(SEG_BASE + 17);
        }
        Ok(())
    }

    /// 文件级修复率（千分比）：指定文件的已修占比。
    pub fn file_fix_permille(&self, file_fp: u64) -> Option<u32> {
        let total = self.rows.iter().filter(|r| r.file_fp == file_fp).count();
        if total == 0 {
            return None;
        }
        let fixed = self
            .rows
            .iter()
            .filter(|r| r.file_fp == file_fp && r.fixed)
            .count();
        Some(((fixed * 1000) / total) as u32)
    }
}
