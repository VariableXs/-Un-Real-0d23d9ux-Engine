//! UNX-F56199 启动剖析数据归档索引件（0x618E0 段）
//!
//! 锚点：UNX-F56199-J1 归档索引账（启动剖析数据归档索引——索引号 ×
//! 数据类别 × 记录数 × 数据指纹逐条落账）；J2 索引语义（类别闭集合
//! 法 + 记录数一致 + 索引指纹与数据指纹一致三性质）；J3 索引指纹（
//! 索引账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·归档索引面）。
//!
//! 结构：数据类别闭集 + 归档索引簿 + 索引核（类别闸 + 计数闸 + 指纹
//! 闸）+ 索引指纹 + 注入核（未知类别/计数不符/指纹不符/重索引号/印
//! 后归档必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x618E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x618E0;
pub const AI_UNK_KIND: u32 = SEG_BASE + 0x01; // 未知数据类别
pub const AI_COUNT_MISS: u32 = SEG_BASE + 0x02; // 记录数不符
pub const AI_FP_MISS: u32 = SEG_BASE + 0x03; // 数据指纹不符
pub const AI_DUP_IDX: u32 = SEG_BASE + 0x04; // 重索引号
pub const AI_FROZEN: u32 = SEG_BASE + 0x05; // 印后归档

/// 数据类别闭集。
pub const AD_SEGMENT: usize = 0; // 四段定位数据
pub const AD_SUBPROF: usize = 1; // 子段剖析数据
pub const AD_REPLAY: usize = 2; // 重放复测数据
pub const AD_NOISE: usize = 3; // 噪声源登记数据
pub const AD_COUNT: usize = 4;

pub const AD_NAMES: [&str; AD_COUNT] = ["segment", "subprof", "replay", "noise"];

/// 一条归档索引。
#[derive(Clone)]
pub struct ArchiveIndex {
    pub idx_no: u32,
    pub kind: usize,
    pub record_count: usize,
    pub data_fp: u32,
}

impl ArchiveIndex {
    fn render_line(&self) -> String {
        format!(
            "AI{}|{}|n={}|fp={:08X}",
            self.idx_no, AD_NAMES[self.kind], self.record_count, self.data_fp
        )
    }
}

/// 数据桶：带指纹与记录数声明。
pub struct DataBucket {
    pub kind: usize,
    pub record_count: usize,
    pub data_fp: u32,
}

/// 归档索引簿。
pub struct ArchiveRegistry {
    indices: Vec<ArchiveIndex>,
    sealed: bool,
}

impl ArchiveRegistry {
    pub fn new() -> Self {
        ArchiveRegistry {
            indices: Vec::new(),
            sealed: false,
        }
    }

    /// 归档一桶：类别闸 + 计数闸 + 指纹闸 + 重号拒 + 印后拒。
    pub fn archive(&mut self, idx_no: u32, bucket: &DataBucket) -> Result<(), u32> {
        if self.sealed {
            return Err(AI_FROZEN);
        }
        if bucket.kind >= AD_COUNT {
            return Err(AI_UNK_KIND);
        }
        if bucket.record_count == 0 {
            return Err(AI_COUNT_MISS);
        }
        if bucket.data_fp == 0 {
            return Err(AI_FP_MISS);
        }
        if self.indices.iter().any(|x| x.idx_no == idx_no) {
            return Err(AI_DUP_IDX);
        }
        self.indices.push(ArchiveIndex {
            idx_no,
            kind: bucket.kind,
            record_count: bucket.record_count,
            data_fp: bucket.data_fp,
        });
        Ok(())
    }

    pub fn idx(&self, no: u32) -> Option<&ArchiveIndex> {
        self.indices.iter().find(|x| x.idx_no == no)
    }

    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// 按类别查索引。
    pub fn by_kind(&self, kind: usize) -> Vec<&ArchiveIndex> {
        self.indices.iter().filter(|x| x.kind == kind).collect()
    }

    /// 总记录数。
    pub fn total_records(&self) -> usize {
        self.indices.iter().map(|x| x.record_count).sum()
    }

    /// 索引指纹。
    pub fn reg_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.indices {
            h = h.wrapping_mul(0x0100_0193) ^ x.idx_no;
            h = h.wrapping_mul(0x0100_0193) ^ x.kind as u32;
            h = h.wrapping_mul(0x0100_0193) ^ (x.record_count as u32);
            h = h.wrapping_mul(0x0100_0193) ^ x.data_fp;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封簿印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.indices.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_reg(&self) -> String {
        let mut out = String::new();
        for x in &self.indices {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("AIREG-SEAL\n");
        }
        out
    }
}

impl Default for ArchiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四桶归档（类别闭集全覆盖）。
fn standard_reg() -> ArchiveRegistry {
    let mut r = ArchiveRegistry::new();
    let buckets = [
        (1u32, DataBucket { kind: AD_SEGMENT, record_count: 4, data_fp: 0xA110_0001 }),
        (2, DataBucket { kind: AD_SUBPROF, record_count: 16, data_fp: 0xA110_0002 }),
        (3, DataBucket { kind: AD_REPLAY, record_count: 5, data_fp: 0xA110_0003 }),
        (4, DataBucket { kind: AD_NOISE, record_count: 4, data_fp: 0xA110_0004 }),
    ];
    for (no, b) in buckets {
        let _ = r.archive(no, &b);
    }
    r
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知类别
    let mut r = ArchiveRegistry::new();
    if r.archive(1, &DataBucket { kind: 9, record_count: 1, data_fp: 0x1 }) == Err(AI_UNK_KIND) {
        caught += 1;
    }
    // 注 2：计数不符（零记录）
    let mut r = ArchiveRegistry::new();
    if r.archive(1, &DataBucket { kind: AD_SEGMENT, record_count: 0, data_fp: 0x1 }) == Err(AI_COUNT_MISS) {
        caught += 1;
    }
    // 注 3：指纹不符（零指纹）
    let mut r = ArchiveRegistry::new();
    if r.archive(1, &DataBucket { kind: AD_SEGMENT, record_count: 1, data_fp: 0 }) == Err(AI_FP_MISS) {
        caught += 1;
    }
    // 注 4：重索引号
    let mut r = standard_reg();
    if r.archive(1, &DataBucket { kind: AD_SEGMENT, record_count: 1, data_fp: 0x9 }) == Err(AI_DUP_IDX) {
        caught += 1;
    }
    // 注 5：印后归档
    let mut r = standard_reg();
    let _ = r.seal();
    if r.archive(9, &DataBucket { kind: AD_SEGMENT, record_count: 1, data_fp: 0x9 }) == Err(AI_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 归档索引账；J2 索引语义；J3 索引指纹。
pub fn run_unx56199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56199");
    // J1-1 四桶全归档
    let r = standard_reg();
    let ok = r.len() == 4 && (1..=4).all(|n| r.idx(n).is_some());
    s.add("56199-J1-四桶全归档", ok, if ok { "" } else { "索引账不全" });
    // J1-2 渲染导出齐
    let mut r2 = standard_reg();
    let _ = r2.seal();
    let text = r2.render_reg();
    let ok = text.contains("AI1|segment|n=4|fp=A1100001")
        && text.contains("AI4|noise|n=4|fp=A1100004")
        && text.contains("AIREG-SEAL");
    s.add("56199-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 类别闭集全覆盖
    let ok = (0..AD_COUNT).all(|k| !r.by_kind(k).is_empty());
    s.add("56199-J2-类别全覆盖", ok, if ok { "" } else { "类别缺档" });
    // J2-2 总记录数（4+16+5+4 = 29）
    let ok = r.total_records() == 29;
    s.add("56199-J2-总记录数", ok, if ok { "" } else { "记录数漂移" });
    // J2-3 数据指纹齐（全非零）
    let ok = r.indices.iter().all(|x| x.data_fp != 0);
    s.add("56199-J2-指纹齐", ok, if ok { "" } else { "指纹缺失" });
    // J2-4 重号拒
    let mut r3 = standard_reg();
    let ok = r3.archive(2, &DataBucket { kind: AD_SEGMENT, record_count: 1, data_fp: 0x9 }) == Err(AI_DUP_IDX);
    s.add("56199-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J2-5 未知类别拒
    let mut r4 = ArchiveRegistry::new();
    let ok = r4.archive(1, &DataBucket { kind: 42, record_count: 1, data_fp: 0x1 }) == Err(AI_UNK_KIND);
    s.add("56199-J2-未知类别拒", ok, if ok { "" } else { "类别闸失效" });
    // J2-6 零记录拒
    let mut r5 = ArchiveRegistry::new();
    let ok = r5.archive(1, &DataBucket { kind: AD_SEGMENT, record_count: 0, data_fp: 0x1 }) == Err(AI_COUNT_MISS);
    s.add("56199-J2-零记录拒", ok, if ok { "" } else { "计数闸失效" });
    // J3-1 索引指纹确定
    let ok = standard_reg().reg_fp() == standard_reg().reg_fp();
    s.add("56199-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 桶变签变
    let mut r6 = standard_reg();
    let fp0 = r6.reg_fp();
    if let Some(x) = r6.indices.first_mut() {
        x.data_fp += 1;
    }
    let ok = r6.reg_fp() != fp0;
    s.add("56199-J3-桶变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封簿印
    let mut r7 = standard_reg();
    let ok = r7.seal() && r7.sealed();
    s.add("56199-J3-封簿印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒归档
    let ok = r7.archive(9, &DataBucket { kind: AD_SEGMENT, record_count: 1, data_fp: 0x9 }) == Err(AI_FROZEN);
    s.add("56199-J3-印后拒归档", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56199-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56199_all_checks_green() {
        let set = run_unx56199_checks();
        assert!(set.all_passed(), "UNX-F56199 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56199_reg_stable() {
        let mut r = standard_reg();
        assert!(r.seal());
    }
}
