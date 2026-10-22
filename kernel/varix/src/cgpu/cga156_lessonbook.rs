//! cga156_lessonbook.rs —— CGPU-F0156 · A 域经验教训全书
//!
//! S1-S11 会话教训的全书汇编：30+ 条教训分四类（工程纪律类/
//! 技术陷阱类/流程改进类/文档纪律类）逐条知识化——教训文、
//! 根因、防线、适用范围；教训间关联分析（同根因聚类 → 系统性
//! 改进项识别）。教训全书是「不二过」的制度化载体。
//! 与 F0150（KmFin 方法论消费）互补：F0150 管方法论终版与消费，
//! 本册管教训正文与关联分析。
//! 诊断码段 0x03B0（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x03B0;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 教训分类（判据「分类」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessonKind {
    /// 工程纪律。
    Discipline,
    /// 技术陷阱。
    TechTrap,
    /// 流程改进。
    Process,
    /// 文档纪律。
    DocRule,
}

impl LessonKind {
    /// 全类序。
    pub const ALL: [LessonKind; 4] = [
        LessonKind::Discipline,
        LessonKind::TechTrap,
        LessonKind::Process,
        LessonKind::DocRule,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            LessonKind::Discipline => 0,
            LessonKind::TechTrap => 1,
            LessonKind::Process => 2,
            LessonKind::DocRule => 3,
        }
    }
}

/// 一条教训（判据「逐条知识化」的结构化载体）。
#[derive(Clone, Debug)]
pub struct LessonEntry {
    /// 教训号（1..）。
    pub id: u32,
    /// 来源会话号（S1-S11 记 1-11，域外记 0）。
    pub session: u8,
    /// 分类。
    pub kind: LessonKind,
    /// 教训文指纹。
    pub body_fp: u64,
    /// 根因指纹（关联分析的聚类键）。
    pub cause_fp: u64,
    /// 防线指纹（零 = 无防线 = 教训白记，判据「防线条目」）。
    pub guard_fp: u64,
    /// 适用范围：位面 0=内核 1=工具 2=文档 3=流程。
    pub scope_mask: u8,
}

impl LessonEntry {
    /// 自检（判据「防线条目」硬门）：教训文、根因、防线三指纹
    /// 全非零；范围位至少一位（全零 = 无适用范围 = 不可执行）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.session > 11 {
            return Err(SEG_BASE + 2);
        }
        if self.body_fp == 0 || self.cause_fp == 0 {
            return Err(SEG_BASE + 3);
        }
        if self.guard_fp == 0 {
            return Err(SEG_BASE + 4);
        }
        if self.scope_mask == 0 || self.scope_mask > 0x0F {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 系统性改进项（判据「关联分析」产物）：同根因教训 ≥3 条
/// 时识别出的聚类。
#[derive(Clone, Debug)]
pub struct SystemicItem {
    /// 根因指纹（聚类键）。
    pub cause_fp: u64,
    /// 聚到的教训号集。
    pub members: Vec<u32>,
    /// 系统性改进提案指纹。
    pub proposal_fp: u64,
}

/// 教训全书。
pub struct LessonBook {
    /// 教训条目（按号排序登记，不许重号）。
    entries: Vec<LessonEntry>,
    /// 系统性改进项。
    systemic: Vec<SystemicItem>,
    /// 入知识库确认（判据「入知识库」）：全书指纹签章。
    kb_seal: Option<u64>,
    /// 最小收录门槛（判据「30+ 条全汇编」）。
    min_entries: u32,
}

impl LessonBook {
    /// 建书：门槛默认 30（判据线），可调但不得低于 30。
    pub fn new(min_entries: u32) -> Result<Self, u16> {
        if min_entries < 30 {
            return Err(SEG_BASE + 6);
        }
        Ok(LessonBook {
            entries: Vec::new(),
            systemic: Vec::new(),
            kb_seal: None,
            min_entries,
        })
    }

    /// 教训条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 系统性改进项数。
    pub fn systemic_count(&self) -> usize {
        self.systemic.len()
    }

    /// 收录（判据「全汇编」入口）：号不重复、逐条自检。
    pub fn admit(&mut self, e: LessonEntry) -> Result<(), u16> {
        if self.kb_seal.is_some() {
            return Err(SEG_BASE + 7);
        }
        e.validate()?;
        if self.entries.iter().any(|x| x.id == e.id) {
            return Err(SEG_BASE + 8);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 号序检查（判据「全汇编」完整性）：收录号必须连续递增
    /// （1..n 无缺号——缺号意味着有教训被丢弃）。
    pub fn numbering_complete(&self) -> Result<(), u16> {
        for (i, e) in self.entries.iter().enumerate() {
            if e.id as usize != i + 1 {
                return Err(SEG_BASE + 9);
            }
        }
        Ok(())
    }

    /// 分类普查（判据「分类」）。
    pub fn kind_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for e in &self.entries {
            out[e.kind.rank() as usize] += 1;
        }
        out
    }

    /// 会话覆盖（判据「S1-S11」）：11 个会话都有教训回流才算
    /// 全汇编——未回流会话列出来。
    pub fn session_gaps(&self) -> Vec<u8> {
        let mut gaps = Vec::new();
        for s in 1u8..=11 {
            if !self.entries.iter().any(|e| e.session == s) {
                gaps.push(s);
            }
        }
        gaps
    }

    /// 关联分析（判据「关联分析」）：同根因指纹聚类，≥3 条的
    /// 类识别为系统性改进项（提案指纹由调用方给）。
    pub fn correlate(&mut self, proposal_of: impl Fn(u64) -> u64) -> Result<usize, u16> {
        if self.kb_seal.is_some() {
            return Err(SEG_BASE + 7);
        }
        if proposal_of(0) == 0 {
            return Err(SEG_BASE + 10);
        }
        self.systemic.clear();
        let n = self.entries.len();
        let mut used = vec![false; n];
        for i in 0..n {
            if used[i] {
                continue;
            }
            let key = self.entries[i].cause_fp;
            let mut members = Vec::new();
            for e in &self.entries {
                if e.cause_fp == key {
                    members.push(e.id);
                }
            }
            if members.len() >= 3 {
                for (j, e) in self.entries.iter().enumerate() {
                    if e.cause_fp == key {
                        used[j] = true;
                    }
                }
                self.systemic.push(SystemicItem {
                    cause_fp: key,
                    members,
                    proposal_fp: proposal_of(key),
                });
            }
        }
        for item in &self.systemic {
            if item.proposal_fp == 0 {
                return Err(SEG_BASE + 11);
            }
        }
        Ok(self.systemic.len())
    }

    /// 系统性改进项投影。
    pub fn systemic_rows(&self) -> &Vec<SystemicItem> {
        &self.systemic
    }

    /// 范围分布审计（判据支撑）：四范围位各覆盖多少条（零覆盖
    /// 位说明教训都堆在一类面上，提示采集偏科）。
    pub fn scope_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for e in &self.entries {
            for bit in 0..4 {
                if e.scope_mask & (1 << bit) != 0 {
                    out[bit] += 1;
                }
            }
        }
        out
    }

    /// 全书指纹（判据「入知识库」签章原料）：全部条目按号序
    /// 折入 FNV 链 + 系统性改进项收尾。
    pub fn book_fp(&self) -> Result<u64, u16> {
        if (self.entries.len() as u32) < self.min_entries {
            return Err(SEG_BASE + 12);
        }
        self.numbering_complete()?;
        let mut acc: u64 = 0xcbf29ce484222325;
        for e in &self.entries {
            acc = acc.wrapping_mul(0x100000001b3) ^ e.body_fp;
            acc = acc.wrapping_mul(0x100000001b3) ^ e.cause_fp;
            acc = acc.wrapping_mul(0x100000001b3) ^ e.guard_fp;
        }
        for s in &self.systemic {
            acc = acc.wrapping_mul(0x100000001b3) ^ s.proposal_fp;
        }
        if acc == 0 {
            return Err(SEG_BASE + 13);
        }
        Ok(acc)
    }

    /// 入知识库（判据「入知识库」收口）：门槛 + 号序 + 指纹
    /// 三关全过才可签章；签后全书冻结。
    pub fn seal_to_kb(&mut self) -> Result<u64, u16> {
        if self.kb_seal.is_some() {
            return Err(SEG_BASE + 7);
        }
        let fp = self.book_fp()?;
        self.kb_seal = Some(fp);
        Ok(fp)
    }

    /// 签章投影。
    pub fn kb_seal_fp(&self) -> Option<u64> {
        self.kb_seal
    }
}

/// 教训重复检测（判据支撑的独立面）：同教训文 + 同根因 =
/// 重复教训（收录时本应拦截，此函数供抽查已入库全集）。
pub fn duplicate_scan(entries: &[LessonEntry]) -> Result<(), u16> {
    for (i, a) in entries.iter().enumerate() {
        for (j, b) in entries.iter().enumerate() {
            if i < j && a.body_fp == b.body_fp && a.cause_fp == b.cause_fp {
                return Err(SEG_BASE + 14);
            }
        }
    }
    Ok(())
}

/// 会话外教训占比审计：session=0 的条数不得超过总量的 20%
/// （教训全书是 A 域的，域外教训喧宾夺主 = 采集失控）。
pub fn foreign_ratio_permille(entries: &[LessonEntry]) -> Result<u32, u16> {
    if entries.is_empty() {
        return Err(SEG_BASE + 15);
    }
    let foreign = entries.iter().filter(|e| e.session == 0).count();
    let permille = (foreign * 1000) / entries.len();
    if permille > 200 {
        return Err(SEG_BASE + 16);
    }
    Ok(permille as u32)
}

impl LessonBook {
    /// 防线重复审计（判据「防线条目」强化面）：同防线指纹被
    /// 多条教训引用是正常的（一个防线堵一类坑），但同一教训号
    /// 下防线为零已被 validate 拦——此处审计防线指纹零值漏网。
    pub fn guard_zero_scan(&self) -> Result<(), u16> {
        if self.entries.iter().any(|e| e.guard_fp == 0) {
            return Err(SEG_BASE + 17);
        }
        Ok(())
    }

    /// 会话密度普查：11 个会话各回流多少条（采集均衡可见）。
    pub fn session_census(&self) -> [usize; 11] {
        let mut out = [0usize; 11];
        for e in &self.entries {
            if e.session >= 1 && e.session <= 11 {
                out[(e.session - 1) as usize] += 1;
            }
        }
        out
    }

    /// 根因聚类宽度：去重根因指纹数（关联分析的原料广度）。
    pub fn cause_width(&self) -> usize {
        let mut seen: Vec<u64> = Vec::new();
        for e in &self.entries {
            if !seen.contains(&e.cause_fp) {
                seen.push(e.cause_fp);
            }
        }
        seen.len()
    }
}
