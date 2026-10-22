//! cga128_errtable — CGPU-F0128 A 域错误码总表。
//!
//! 判据原文（施工书锚点 CGPU-F0128）：A 域 160 项错误码总表（F0385 范
//! 式）：段按批分配、唯一性机检、跨算子错误语义统一表（同类错误在栅格
//! 化/混合/编解码算子的语义一致）、表驱动测试生成。判据：160 项入表、
//! 唯一机检、语义统一、表驱动、追加纪律。
//!
//! **设计定盘**：总表模型：错误码条目 =（码值 + 语义类 + 归属族）。
//! 唯一性机检 = 码值全局去重（重复即拒，且机检可枚举重复对）。语义类
//! 闭集（Bounds/Format/State/Resource/Unknown 五类）——跨算子语义统一
//! = 同语义类的码在任意族里的处置语义一致（账面化为：语义类的处置档
//! 位在总表内唯一，族别不得私自分叉）。段按批分配 = 码值必须落在登记
//! 的段区间内（段外码拒收）。追加纪律 = 封表后只可追加不可改写，
//! 改写即版本事件。表驱动测试生成 = 由总表直接派生测试用例清单
//! （每码一例：正例 + 反例）。
//!
//! 诊断码段 0x01E0~0x01EF（0x01 段第 15 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA128_VERSION: &str = "cga128-errtable-v1";

/// 诊断子区基值（0x01 段第 15 子区）。
pub const SEG_BASE: u16 = 0x01E0;

/// 总表容量（判据"160 项"）。
pub const TABLE_CAP: usize = 160;

/// 语义类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemClass {
    /// 越界。
    Bounds,
    /// 格式。
    Format,
    /// 状态。
    State,
    /// 资源。
    Resource,
    /// 未知（兜底类，只许一个码）。
    Unknown,
}

impl SemClass {
    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            SemClass::Bounds => 0,
            SemClass::Format => 1,
            SemClass::State => 2,
            SemClass::Resource => 3,
            SemClass::Unknown => 4,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 处置档位（语义统一的载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 可恢复：调用方重试或修正参数。
    Recoverable,
    /// 致命：算子实例作废。
    Fatal,
    /// 静默：内部兜底，不上抛。
    Swallow,
}

/// 码条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    /// 码值（16 位）。
    pub code: u16,
    /// 语义类。
    pub cls: SemClass,
    /// 归属族（算子族序号，0..6）。
    pub fam: u8,
    /// 处置档位。
    pub disp: Disposition,
}

/// 错误码总表。
pub struct ErrTable {
    /// 段区间登记（已分配批段，闭区间）。
    segs: Vec<(u16, u16)>,
    /// 条目。
    items: Vec<ErrEntry>,
    /// 各语义类的处置档（语义统一判据的账面化——类级唯一）。
    class_disp: [Option<Disposition>; 5],
    /// 封表。
    sealed: Option<u64>,
}

impl ErrTable {
    /// 空表。
    pub fn new() -> Self {
        ErrTable {
            segs: Vec::new(),
            items: Vec::new(),
            class_disp: [None; 5],
            sealed: None,
        }
    }

    /// 分配段（判据"段按批分配"：段不重叠；段长 ≥ 4——
    /// 段太碎无法承载批语义）。
    pub fn alloc_segment(&mut self, lo: u16, hi: u16) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if hi < lo || hi - lo + 1 < 4 {
            return Err(SEG_BASE + 7);
        }
        if self.segs.iter().any(|&(a, b)| lo <= b && a <= hi) {
            return Err(SEG_BASE + 8);
        }
        self.segs.push((lo, hi));
        Ok(())
    }

    /// 段归属查询。
    pub fn in_segment(&self, code: u16) -> bool {
        self.segs.iter().any(|&(a, b)| code >= a && code <= b)
    }

    /// 入表（判据"唯一性机检"+"段外拒收"：码必须在已分配段内、
    /// 全表唯一、族号 < 6）。
    pub fn insert(&mut self, e: ErrEntry) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 9);
        }
        if e.fam > 5 {
            return Err(SEG_BASE + 10);
        }
        if !self.in_segment(e.code) {
            return Err(SEG_BASE + 11);
        }
        if self.items.iter().any(|x| x.code == e.code) {
            return Err(SEG_BASE + 12);
        }
        // 语义统一：类级处置档登记，同类异档即拒。
        let slot = e.cls.ordinal() as usize;
        match self.class_disp[slot] {
            None => self.class_disp[slot] = Some(e.disp),
            Some(d) if d != e.disp => return Err(SEG_BASE + 13),
            Some(_) => {}
        }
        self.items.push(e);
        Ok(())
    }

    /// 唯一性机检（判据"唯一性机检"显性入口：返回重复码对数；
    /// 总表自身维护唯一，此入口供外部账对总表的复核）。
    pub fn uniqueness_audit(&self) -> usize {
        let mut dup = 0usize;
        for i in 0..self.items.len() {
            for j in (i + 1)..self.items.len() {
                if self.items[i].code == self.items[j].code {
                    dup += 1;
                }
            }
        }
        dup
    }

    /// 容量核验（判据"160 项入表"）。
    pub fn table_complete(&self) -> Result<(), u16> {
        if self.items.len() != TABLE_CAP {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }

    /// 表驱动测试生成（判据"表驱动"：每码一例，返回用例清单——
    /// 用例 =（码, 正例期望 Recoverable 或反例期望 Fatal/Swallow 语义））。
    pub fn derive_test_cases(&self) -> Vec<(u16, Disposition)> {
        self.items
            .iter()
            .map(|e| (e.code, e.disp))
            .collect()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 查条目。
    pub fn lookup(&self, code: u16) -> Option<&ErrEntry> {
        self.items.iter().find(|e| e.code == code)
    }

    /// 封表（判据"追加纪律"：封表后 insert 拒收——只可另开新表
    /// 作为版本事件）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 15);
        }
        self.table_complete()?;
        // Unknown 兜底类只许一个码。
        if self
            .items
            .iter()
            .filter(|e| e.cls == SemClass::Unknown)
            .count()
            > 1
        {
            return Err(SEG_BASE + 16);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for e in &self.items {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ e.code as u64;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for ErrTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 跨族语义一致性投影行（判据"跨算子错误语义统一"的账面）：一个码
/// 的语义类 + 处置档，供跨族抽查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemRow {
    /// 码值。
    pub code: u16,
    /// 语义类。
    pub cls: SemClass,
    /// 处置档。
    pub disp: Disposition,
}

/// 段利用率投影。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegUsage {
    /// 段下界。
    pub lo: u16,
    /// 段上界。
    pub hi: u16,
    /// 段内已入表码数。
    pub used: usize,
    /// 段容量。
    pub cap: usize,
}

impl ErrTable {
    /// 语义类计数投影（五类各多少码）。
    pub fn class_census(&self) -> [usize; 5] {
        let mut out = [0usize; 5];
        for e in &self.items {
            out[e.cls.ordinal() as usize] += 1;
        }
        out
    }

    /// 处置档计数投影（判据"表驱动"的分布面）。
    pub fn disposition_census(&self) -> (usize, usize, usize) {
        let r = self.items.iter().filter(|e| e.disp == Disposition::Recoverable).count();
        let f = self.items.iter().filter(|e| e.disp == Disposition::Fatal).count();
        let s = self.items.iter().filter(|e| e.disp == Disposition::Swallow).count();
        (r, f, s)
    }

    /// 族覆盖核验（判据"跨算子语义统一"的前提：六族每族至少一码
    /// ——零码族的"语义一致"无从谈起）。
    pub fn fam_coverage(&self) -> Result<(), u16> {
        for fam in 0u8..6 {
            if !self.items.iter().any(|e| e.fam == fam) {
                return Err(SEG_BASE + 17);
            }
        }
        Ok(())
    }

    /// 类级处置档读取（判据"语义统一"的查询面；未登记类返回 None）。
    pub fn class_disposition(&self, cls: SemClass) -> Option<Disposition> {
        self.class_disp[cls.ordinal() as usize]
    }

    /// 段利用率投影（判据"段按批分配"的账面）。
    pub fn seg_usage(&self) -> Vec<SegUsage> {
        self.segs
            .iter()
            .map(|&(lo, hi)| SegUsage {
                lo,
                hi,
                used: self
                    .items
                    .iter()
                    .filter(|e| e.code >= lo && e.code <= hi)
                    .count(),
                cap: (hi - lo + 1) as usize,
            })
            .collect()
    }

    /// 跨族抽查投影（判据"语义一致"的抽检面：同语义类的全部码
    /// 按 (码, 类, 档) 列出——抽检 20% 的样本源）。
    pub fn class_sample(&self, cls: SemClass) -> Vec<SemRow> {
        self.items
            .iter()
            .filter(|e| e.cls == cls)
            .map(|e| SemRow { code: e.code, cls: e.cls, disp: e.disp })
            .collect()
    }

    /// 封表指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }

    /// 段数。
    pub fn seg_count(&self) -> usize {
        self.segs.len()
    }
}
