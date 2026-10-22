//! cga150_kmfin — CGPU-F0150 A 域知识库终版。
//!
//! 判据原文（施工书锚点 CGPU-F0150）：A 域知识库终版（F0139 的收官
//! 扩充）：四方法论终版 + S1-S11 全部会话教训汇编（30+ 条教训的分类
//! 知识化）——教训即资产（每条教训是别人踩过的坑）。判据：方法论
//! 终版、教训汇编、分类知识化、消费验证、更新流程。
//!
//! **设计定盘**：方法论终版 = F0139 四份的冻结转版（转版后旧版进
//! 历史——知识不抹史）。教训汇编 = 教训条目账：每条教训
//! （编号 + 分类 + 教训文 + 根因 + 防线）五元组，分类闭集四类
//! （工程纪律/技术陷阱/流程改进/文档纪律）。30+ 条下限核验。
//! 分类知识化 = 每类至少 3 条（分类不是装饰——孤类不算知识化）。
//! 教训关联 = 同根因教训的聚类投影（关联分析面）。更新流程 = 方法
//! 论版本演进沿用 F0139 语义。
//!
//! 诊断码段 0x0350~0x035F（0x03 段第 6 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;
use super::cga147_secfin::fnv1a64;

/// 模块版本。
pub const CGA150_VERSION: &str = "cga150-kmfin-v1";

/// 诊断子区基值（0x03 段第 6 子区）。
pub const SEG_BASE: u16 = 0x0350;

/// 教训条数下限（判据"30+ 条"）。
pub const LESSON_FLOOR: u32 = 30;

/// 每类下限（判据"分类知识化"）。
pub const PER_CLASS_FLOOR: u32 = 3;

/// 教训分类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessonClass {
    /// 工程纪律类。
    Discipline,
    /// 技术陷阱类。
    TechTrap,
    /// 流程改进类。
    Process,
    /// 文档纪律类。
    DocRule,
}

impl LessonClass {
    /// 全集稳定序。
    pub const ALL: [LessonClass; 4] = [
        LessonClass::Discipline,
        LessonClass::TechTrap,
        LessonClass::Process,
        LessonClass::DocRule,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            LessonClass::Discipline => 0,
            LessonClass::TechTrap => 1,
            LessonClass::Process => 2,
            LessonClass::DocRule => 3,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 一条教训五元组。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Lesson {
    /// 教训编号。
    pub id: u32,
    /// 分类。
    pub cls: LessonClass,
    /// 教训文指纹。
    pub body_fp: u64,
    /// 根因指纹（同类教训聚类用——同根因指纹即关联）。
    pub cause_fp: u64,
    /// 防线指纹（零=无防线=教训白记）。
    pub guard_fp: u64,
}

impl Lesson {
    /// 行合法性：防线必须非零——判据"防线条目"是教训的出口。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.body_fp == 0 || self.cause_fp == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.guard_fp == 0 {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
}

/// 知识库终版账。
pub struct KmFinalLedger {
    /// 方法论终版（方法论号 → 终版指纹）。
    methods: Vec<(u8, u64)>,
    /// 方法论历史档。
    method_history: Vec<(u8, u64)>,
    /// 教训册。
    lessons: Vec<Lesson>,
    /// 消费验证（域号, 方法论号）。
    consumes: Vec<(u8, u8)>,
    /// 归档。
    archived: Option<u64>,
}

impl KmFinalLedger {
    /// 空账。
    pub fn new() -> Self {
        KmFinalLedger {
            methods: Vec::new(),
            method_history: Vec::new(),
            lessons: Vec::new(),
            consumes: Vec::new(),
            archived: None,
        }
    }

    /// 方法论终版转版（判据"方法论终版"：四份闭集 0..=4；同号重复
    /// 即拒）。
    pub fn freeze_method(&mut self, m: u8, body: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if m > 3 {
            return Err(SEG_BASE + 8);
        }
        if body.is_empty() || self.methods.iter().any(|(k, _)| *k == m) {
            return Err(SEG_BASE + 9);
        }
        let fp = fnv1a64(body);
        self.methods.push((m, fp));
        Ok(fp)
    }

    /// 方法论更新（判据"更新流程"：旧指纹进历史档）。
    pub fn revise_method(&mut self, m: u8, body: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if body.is_empty() {
            return Err(SEG_BASE + 10);
        }
        let new_fp = fnv1a64(body);
        let idx = match self.methods.iter().position(|(k, _)| *k == m) {
            Some(i) => i,
            None => return Err(SEG_BASE + 11),
        };
        let old = self.methods[idx].1;
        if old == new_fp {
            return Err(SEG_BASE + 12);
        }
        self.method_history.push((m, old));
        self.methods[idx].1 = new_fp;
        Ok(new_fp)
    }

    /// 教训入册（判据"教训汇编"：同号重复即拒；五元组校验）。
    pub fn add_lesson(&mut self, l: Lesson) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        l.validate()?;
        if self.lessons.iter().any(|x| x.id == l.id) {
            return Err(SEG_BASE + 13);
        }
        self.lessons.push(l);
        Ok(())
    }

    /// 教训下限核验（判据"30+ 条"）。
    pub fn lessons_floor(&self) -> Result<(), u16> {
        if (self.lessons.len() as u32) < LESSON_FLOOR {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }

    /// 分类知识化核验（判据"分类知识化"：四类每类至少 3 条）。
    pub fn classes_knowledge(&self) -> Result<(), u16> {
        for c in LessonClass::ALL {
            let n = self
                .lessons
                .iter()
                .filter(|l| l.cls == c)
                .count();
            if (n as u32) < PER_CLASS_FLOOR {
                return Err(c.code());
            }
        }
        Ok(())
    }

    /// 分类计数投影。
    pub fn class_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for l in &self.lessons {
            out[l.cls.ordinal() as usize] += 1;
        }
        out
    }

    /// 关联聚类投影（判据"关联分析"：同根因指纹的教训聚类，
    /// 返回 (根因指纹, 聚内条数)——多于一即有关联）。
    pub fn cause_clusters(&self) -> Vec<(u64, usize)> {
        let mut out: Vec<(u64, usize)> = Vec::new();
        for l in &self.lessons {
            match out.iter_mut().find(|(f, _)| *f == l.cause_fp) {
                Some((_, n)) => *n += 1,
                None => out.push((l.cause_fp, 1)),
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    /// 消费验证登记（判据"消费验证"：域号 0=B 1=C 2=G；同对重复即拒）。
    pub fn consume(&mut self, domain: u8, m: u8) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if domain > 2 || m > 3 {
            return Err(SEG_BASE + 15);
        }
        if self.consumes.contains(&(domain, m)) {
            return Err(SEG_BASE + 16);
        }
        self.consumes.push((domain, m));
        Ok(())
    }

    /// 教训数。
    pub fn lesson_count(&self) -> usize {
        self.lessons.len()
    }

    /// 历史档数。
    pub fn history_count(&self) -> usize {
        self.method_history.len()
    }

    /// 归档（判据五面：四方法论齐 + 教训下限 + 分类知识化 + 消费
    /// 非空）。
    pub fn archive(&mut self) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 17);
        }
        if self.methods.len() != 4 {
            return Err(SEG_BASE + 18);
        }
        self.lessons_floor()?;
        self.classes_knowledge()?;
        if self.consumes.is_empty() {
            return Err(SEG_BASE + 19);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for (m, f) in &self.methods {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ ((*m as u64) << 32) ^ f;
        }
        for l in &self.lessons {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ l.cause_fp ^ l.guard_fp;
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

impl Default for KmFinalLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl KmFinalLedger {
    /// 方法论覆盖普查：已终版方法论号集。
    pub fn method_ids(&self) -> Vec<u8> {
        self.methods.iter().map(|(id, _)| *id).collect()
    }

    /// 消费闭环检查（判据「方法论被消费」强校验）：每条终版
    /// 方法论至少被一个域消费过——无人消费的终版是死档。
    pub fn consumption_closed(&self) -> Result<(), u16> {
        for (id, _) in &self.methods {
            if !self.consumes.iter().any(|(_, c)| c == id) {
                return Err(SEG_BASE + 20);
            }
        }
        Ok(())
    }

    /// 教训分类普查。
    pub fn lesson_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for l in &self.lessons {
            let i = match l.cls {
                LessonClass::Discipline => 0,
                LessonClass::TechTrap => 1,
                LessonClass::Process => 2,
                LessonClass::DocRule => 3,
            };
            out[i] += 1;
        }
        out
    }

    /// 方法论演进深度：历史档条数（有演进史的册才算活册）。
    pub fn evolution_depth(&self) -> usize {
        self.method_history.len()
    }
}
