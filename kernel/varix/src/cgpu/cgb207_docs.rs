//! CGPU-F0207 · 帧图文档（判据段 0x0690）
//! 帧图全文档：架构（数据结构/依赖推导/执行引擎/并行策略）、API
//! 文档（四族 API 全签名）、调试指南（可视化器/面板/回放）、性能
//! 指南（预算/瓶颈/优化）。判据：四类齐、架构准确、指南质量、更新
//! 流程、入知识库。
//!
//! 诊断码段 0x0690（本模块独占：0x0690~0x069F）。
//! no_std + alloc 零 unwrap，全部 Result 化。
//!
//! 文档即代码：每篇文档以结构化章节账本落库——章节可校验、引用
//! 可追溯、指纹可审计。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0690;

/// 文档类别数：架构/API/调试/性能四类。
pub const DOC_CATEGORIES: u8 = 4;

/// 文档类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    /// 架构篇（数据结构/依赖推导/执行引擎/并行策略）。
    Architecture,
    /// API 篇（四族 API 全签名）。
    Api,
    /// 调试篇（可视化器/面板/回放）。
    Debug,
    /// 性能篇（预算/瓶颈/优化）。
    Performance,
}

impl DocKind {
    /// 类别序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            DocKind::Architecture => 0,
            DocKind::Api => 1,
            DocKind::Debug => 2,
            DocKind::Performance => 3,
        }
    }
    /// 类别必需章节数下限（判据「四类齐」的量化线）。
    pub fn required_sections(&self) -> usize {
        match self {
            DocKind::Architecture => 4,
            DocKind::Api => 4,
            DocKind::Debug => 3,
            DocKind::Performance => 3,
        }
    }
}

/// 文档章节（判据「指南质量」原子量）：标题指纹 + 内容指纹 + 引用
/// 的模块判据段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DocSection {
    /// 章节号（篇内从 1 起）。
    pub sec: u32,
    /// 标题指纹。
    pub title_fp: u64,
    /// 内容指纹（正文折并）。
    pub body_fp: u64,
    /// 引用的判据段（0 = 无引用）。
    pub ref_seg: u16,
    /// 正文字数（下限校验用）。
    pub body_words: u32,
}

impl DocSection {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.sec == 0 || self.title_fp == 0 || self.body_fp == 0 {
            return Err(SEG_BASE);
        }
        if self.body_words < 20 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 篇文档（判据「四类齐」一等公民）：类别 + 章节序列。
#[derive(Clone, Debug)]
pub struct DocBook {
    /// 类别。
    pub kind: DocKind,
    /// 版本（文档也是冻结物——随 API 版本走）。
    pub version: u32,
    /// 章节序列。
    pub sections: Vec<DocSection>,
}

impl DocBook {
    /// 篇合法性：章节数达类别下限、章节号连续、引用段指向有效段
    /// （非零段号必须落在已知段位表内）。
    pub fn validate(&self, known_segs: &[u16]) -> Result<(), u16> {
        if self.version == 0 || self.sections.is_empty() {
            return Err(SEG_BASE + 2);
        }
        if self.sections.len() < self.kind.required_sections() {
            return Err(SEG_BASE + 3);
        }
        for (i, s) in self.sections.iter().enumerate() {
            s.validate()?;
            if s.sec != (i + 1) as u32 {
                return Err(SEG_BASE + 4);
            }
            if s.ref_seg != 0 && !known_segs.contains(&s.ref_seg) {
                return Err(SEG_BASE + 5);
            }
        }
        Ok(())
    }
    /// 篇指纹（判据「入知识库」的可审计锚点）。
    pub fn book_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.kind.ordinal() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.version as u64;
        for s in &self.sections {
            acc = acc.wrapping_mul(0x100000001b3) ^ s.title_fp;
            acc = acc.wrapping_mul(0x100000001b3) ^ s.body_fp;
        }
        acc
    }
}

/// 文档库（判据「入知识库」容器）：四类各一篇起。
#[derive(Default)]
pub struct DocLib {
    books: Vec<DocBook>,
}

impl DocLib {
    /// 入库（同类别重复入库拒绝——一篇一类别，改版走版本号）。
    pub fn register(&mut self, b: DocBook, known_segs: &[u16]) -> Result<(), u16> {
        b.validate(known_segs)?;
        if self.books.iter().any(|x| x.kind == b.kind) {
            return Err(SEG_BASE + 6);
        }
        self.books.push(b);
        Ok(())
    }
    /// 库规模。
    pub fn count(&self) -> usize {
        self.books.len()
    }
    /// 按类别查询。
    pub fn get(&self, k: DocKind) -> Result<&DocBook, u16> {
        self.books.iter().find(|b| b.kind == k).ok_or(SEG_BASE + 7)
    }
    /// 全库齐备校验（判据「四类齐」主判据）：四类全在册。
    pub fn all_kinds_present(&self) -> Result<(), u16> {
        for o in 0..DOC_CATEGORIES {
            let k = match o {
                0 => DocKind::Architecture,
                1 => DocKind::Api,
                2 => DocKind::Debug,
                _ => DocKind::Performance,
            };
            if self.get(k).is_err() {
                return Err(SEG_BASE + 8);
            }
        }
        Ok(())
    }
    /// 库指纹。
    pub fn lib_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for b in &self.books {
            acc = acc.wrapping_mul(0x100000001b3) ^ b.book_fp();
        }
        acc
    }
}

/// 架构篇准确性投影（判据「架构准确」主判据）：架构篇必须覆盖
/// 四大架构支柱——每支柱一篇章且引用对应实现段。支柱表：
/// 数据结构→0x0560 段家族、依赖推导→0x03C0、执行引擎→0x0420、
/// 并行策略→0x05B0。引用缺失即不准确。
pub fn architecture_accuracy(book: &DocBook) -> Result<[bool; 4], u16> {
    if book.kind != DocKind::Architecture {
        return Err(SEG_BASE + 9);
    }
    let pillars: [u16; 4] = [0x0560, 0x03C0, 0x0420, 0x05B0];
    let mut ok = [false; 4];
    for s in &book.sections {
        for (i, p) in pillars.iter().enumerate() {
            if s.ref_seg == *p {
                ok[i] = true;
            }
        }
    }
    if ok.iter().any(|o| !o) {
        return Err(SEG_BASE + 10);
    }
    Ok(ok)
}

/// API 篇全签名覆盖（判据「API 文档」主判据）：四族签名各须被至
/// 少一章引用（族签名指纹与章节标题指纹对账）。
pub fn api_signature_coverage(book: &DocBook, fam_fps: &[u64; 4]) -> Result<(), u16> {
    if book.kind != DocKind::Api {
        return Err(SEG_BASE + 11);
    }
    for fp in fam_fps {
        if *fp == 0 {
            return Err(SEG_BASE + 12);
        }
        if !book.sections.iter().any(|s| s.title_fp == *fp) {
            return Err(SEG_BASE + 13);
        }
    }
    Ok(())
}

/// 调试篇工具覆盖（判据「调试指南」主判据）：可视化器/面板/回放
/// 三工具各须一章（以工具段位引用判定：0x0620 面板 / 0x0280 可视
/// 化 / 0x0530 回放——引用任二即过下限，全三最优）。
pub fn debug_tool_coverage(book: &DocBook) -> Result<u8, u16> {
    if book.kind != DocKind::Debug {
        return Err(SEG_BASE + 14);
    }
    let tools: [u16; 3] = [0x0280, 0x0530, 0x0620];
    let mut hit = 0u8;
    for t in tools {
        if book.sections.iter().any(|s| s.ref_seg == t) {
            hit += 1;
        }
    }
    if hit < 2 {
        return Err(SEG_BASE + 15);
    }
    Ok(hit)
}

/// 性能篇主题覆盖（判据「性能指南」主判据）：预算/瓶颈/优化三主
/// 题各须一章（标题指纹与主题指纹表对账）。
pub fn perf_topic_coverage(book: &DocBook, topic_fps: &[u64; 3]) -> Result<(), u16> {
    if book.kind != DocKind::Performance {
        return Err(SEG_BASE + 16);
    }
    for fp in topic_fps {
        if !book.sections.iter().any(|s| s.title_fp == *fp) {
            return Err(SEG_BASE + 17);
        }
    }
    Ok(())
}

/// 文档更新流程（判据「更新流程」主判据）：改版必须版本号递增且
/// 新指纹 ≠ 旧指纹（内容真的变了），版本不递增的改版拒绝。
pub fn revision_flow(old: &DocBook, new: &DocBook) -> Result<(), u16> {
    if old.kind != new.kind {
        return Err(SEG_BASE + 18);
    }
    if new.version <= old.version {
        return Err(SEG_BASE + 19);
    }
    if new.book_fp() == old.book_fp() {
        return Err(SEG_BASE + 20);
    }
    Ok(())
}

/// 知识库索引条目（判据「入知识库」投影面）：类别 + 版本 + 篇指纹
/// 的定长索引——跨册检索用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KbEntry {
    /// 类别序号。
    pub kind_ord: u8,
    /// 版本。
    pub version: u32,
    /// 篇指纹。
    pub fp: u64,
}

impl KbEntry {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.kind_ord >= DOC_CATEGORIES || self.version == 0 || self.fp == 0 {
            return Err(SEG_BASE + 21);
        }
        Ok(())
    }
}

/// 篇 → 知识库条目（判据「入知识库」投影实现）。
pub fn to_kb_entry(b: &DocBook) -> Result<KbEntry, u16> {
    let e = KbEntry { kind_ord: b.kind.ordinal(), version: b.version, fp: b.book_fp() };
    e.validate()?;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sec(n: u32, title: u64, seg: u16) -> DocSection {
        DocSection { sec: n, title_fp: title, body_fp: title.wrapping_mul(31), ref_seg: seg, body_words: 100 }
    }
    const SEGS: &[u16] = &[0x0560, 0x03C0, 0x0420, 0x05B0, 0x0280, 0x0530, 0x0620];
    #[test]
    fn book_validation_and_lib() {
        let mut lib = DocLib::default();
        let arch = DocBook {
            kind: DocKind::Architecture,
            version: 1,
            sections: vec![
                sec(1, 0xA1, 0x0560),
                sec(2, 0xA2, 0x03C0),
                sec(3, 0xA3, 0x0420),
                sec(4, 0xA4, 0x05B0),
            ],
        };
        lib.register(arch, SEGS).unwrap();
        assert_eq!(lib.count(), 1);
        // 同类重复拒绝。
        let dup = DocBook { kind: DocKind::Architecture, version: 2, sections: vec![sec(1, 0xB1, 0)] , };
        assert!(lib.register(dup, SEGS).is_err());
    }
    #[test]
    fn section_count_floor() {
        // Debug 篇要求 ≥3 章。
        let thin = DocBook { kind: DocKind::Debug, version: 1, sections: vec![sec(1, 0xC1, 0x0280), sec(2, 0xC2, 0x0620)] };
        assert_eq!(thin.validate(SEGS).unwrap_err(), SEG_BASE + 3);
    }
    #[test]
    fn architecture_pillars() {
        let b = DocBook {
            kind: DocKind::Architecture,
            version: 1,
            sections: vec![
                sec(1, 0xA1, 0x0560),
                sec(2, 0xA2, 0x03C0),
                sec(3, 0xA3, 0x0420),
                sec(4, 0xA4, 0x05B0),
            ],
        };
        assert_eq!(architecture_accuracy(&b).unwrap(), [true; 4]);
        // 缺执行引擎引用 → 不准确。
        let mut bad = b.clone();
        bad.sections[2].ref_seg = 0;
        assert!(architecture_accuracy(&bad).is_err());
    }
    #[test]
    fn debug_tools_two_of_three() {
        let b = DocBook {
            kind: DocKind::Debug,
            version: 1,
            sections: vec![sec(1, 0xD1, 0x0280), sec(2, 0xD2, 0x0530), sec(3, 0xD3, 0)],
        };
        assert_eq!(debug_tool_coverage(&b).unwrap(), 2);
        // 只引一工具 → 拒绝。
        let mut thin = b.clone();
        thin.sections[1].ref_seg = 0;
        assert!(debug_tool_coverage(&thin).is_err());
    }
    #[test]
    fn revision_increments() {
        let old = DocBook { kind: DocKind::Performance, version: 1, sections: vec![sec(1, 0xE1, 0), sec(2, 0xE2, 0), sec(3, 0xE3, 0)] };
        let mut new = old.clone();
        new.version = 2;
        new.sections[0].title_fp = 0xE9;
        assert!(revision_flow(&old, &new).is_ok());
        // 版本没涨 → 拒绝。
        assert!(revision_flow(&old, &old.clone()).is_err());
        // 版本涨了但内容没变 → 拒绝。
        let same_content = DocBook { kind: DocKind::Performance, version: 2, sections: old.sections.clone() };
        assert!(revision_flow(&old, &same_content).is_err());
    }
    #[test]
    fn kb_projection() {
        let b = DocBook { kind: DocKind::Api, version: 3, sections: vec![sec(1, 0xF1, 0), sec(2, 0xF2, 0), sec(3, 0xF3, 0), sec(4, 0xF4, 0)] };
        let e = to_kb_entry(&b).unwrap();
        assert_eq!(e.kind_ord, 1);
        assert_eq!(e.version, 3);
        assert!(e.validate().is_ok());
    }
}
