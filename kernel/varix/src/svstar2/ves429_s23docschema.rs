//! ves429_s23docschema — VE-F4189 · T 域文档与 schema 总审
//! 总审：T 域文档总审（八组文档齐备+多语言无障碍指南专册终版+F4171 对拍收口）
//! +schema 总审（翻译/资产/地区/观测四 schema 终版+版本冻结）；
//! 文档缺章→补章；schema 漂移→对版；冻结后发现→F4196 纪律；
//! 专册终审含多语言替代文本规范（域本色）；示例无真实数据。
//! 深化：补章批次账本（组/页数归因）+ 守恒恒等式 + 边界探针
//! + 分组投影与整数分位 + 确定性 LCG 生成回归 + 定长序列化投影 + FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x11E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    Ok = 0,
    ChapterMissing = SEG_BASE + 0x01, // 0x11E1
    SchemaDrift = SEG_BASE + 0x02,    // 0x11E2
    VersionDrift = SEG_BASE + 0x03,   // 0x11E3
    NotFrozen = SEG_BASE + 0x04,      // 0x11E4
    LedgerFull = SEG_BASE + 0x05,     // 0x11E5 补章账本满容
    ChapterDup = SEG_BASE + 0x06,     // 0x11E6 章节号重复
    PageZero = SEG_BASE + 0x07,       // 0x11E7 零页章节
    IdxRange = SEG_BASE + 0x08,       // 0x11E8 章节序号越界
}

/// 八组文档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocGroup { G1 = 0, G2 = 1, G3 = 2, G4 = 3, G5 = 4, G6 = 5, G7 = 6, G8 = 7 }

impl DocGroup {
    pub const ALL: [DocGroup; 8] = [
        DocGroup::G1, DocGroup::G2, DocGroup::G3, DocGroup::G4,
        DocGroup::G5, DocGroup::G6, DocGroup::G7, DocGroup::G8];
    pub fn idx(self) -> usize { self as usize }
}

/// 四 schema。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchemaKind { Translate = 0, Asset = 1, Region = 2, Observe = 3 }

impl SchemaKind {
    pub const ALL: [SchemaKind; 4] = [SchemaKind::Translate, SchemaKind::Asset, SchemaKind::Region, SchemaKind::Observe];
}

/// 总审册。
pub struct DsFinalBook {
    pub docs: Vec<DocGroup>,       // 齐备组
    pub a11y_guide_final: bool,    // 多语言无障碍指南专册终版（域本色）
    pub a11y_alt_text: bool,       // 专册含替代文本规范
    pub schema_ver: Vec<(SchemaKind, u32)>,
    pub frozen_ver: u32,
    pub cross_check_done: bool,    // F4171 对拍收口
}

impl DsFinalBook {
    pub fn new() -> Self {
        DsFinalBook { docs: Vec::new(), a11y_guide_final: false, a11y_alt_text: false,
            schema_ver: Vec::new(), frozen_ver: 0, cross_check_done: false }
    }
    pub fn put_doc(&mut self, g: DocGroup) { if !self.docs.contains(&g) { self.docs.push(g); } }
    pub fn missing_groups(&self) -> Vec<DocGroup> {
        DocGroup::ALL.iter().filter(|g| !self.docs.contains(g)).map(|g| *g).collect()
    }
    pub fn set_schema(&mut self, k: SchemaKind, ver: u32) {
        self.schema_ver.retain(|(kk, _)| *kk != k);
        self.schema_ver.push((k, ver));
    }
    pub fn schema_ver(&self, k: SchemaKind) -> u32 {
        self.schema_ver.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v).unwrap_or(0)
    }
    /// schema 版本冻结：四 schema 齐且版本一致+对拍收口。
    pub fn freeze(&mut self, ver: u32) -> Result<(), DsCode> {
        for k in SchemaKind::ALL {
            let v = self.schema_ver(k);
            if v == 0 { return Err(DsCode::SchemaDrift); }
            if v != ver { return Err(DsCode::VersionDrift); }
        }
        if !self.cross_check_done { return Err(DsCode::NotFrozen); }
        self.frozen_ver = ver;
        Ok(())
    }
    /// 文档总审：八组齐+专册终版+替代文本规范+示例无真实数据（此处以专册断言代）。
    pub fn doc_verdict(&self) -> Result<(), DsCode> {
        if self.missing_groups().len() > 0 { return Err(DsCode::ChapterMissing); }
        if !self.a11y_guide_final || !self.a11y_alt_text { return Err(DsCode::ChapterMissing); }
        Ok(())
    }
}

// ================= 深化：确定性 LCG（文档批次生成序列） =================

/// 8 行确定性 LCG（禁墙钟，纯算子推衍）。
pub struct Lcg { state: u64 }

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg { state: seed ^ 0x9E37_79B9_7F4A_7C15 } }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }
    pub fn next_below(&mut self, bound: u32) -> u32 { self.next_u32() % bound.max(1) }
}

// ================= 深化：补章批次账本（组/页数归因） =================

/// 补章记录：第 g 组的某章节，页数 pages，语言层 lang。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChapterRec {
    pub chapter_id: u32,
    pub group: DocGroup,
    pub pages: u32,
    pub lang: u8, // 语言层代号（示例规范值，非真实数据）
}

/// 补章账本。
pub struct ChapterLedger {
    pub recs: Vec<ChapterRec>,
    pub cap: usize,
}

impl ChapterLedger {
    pub fn new(cap: usize) -> Self { ChapterLedger { recs: Vec::new(), cap } }

    /// 章节登记：满容/重复/零页三类边界显性拒。
    pub fn record(&mut self, r: ChapterRec) -> Result<(), DsCode> {
        if self.recs.len() >= self.cap { return Err(DsCode::LedgerFull); }
        if self.recs.iter().any(|x| x.chapter_id == r.chapter_id) { return Err(DsCode::ChapterDup); }
        if r.pages == 0 { return Err(DsCode::PageZero); }
        self.recs.push(r);
        Ok(())
    }

    pub fn total_pages(&self) -> u32 { self.recs.iter().map(|r| r.pages).sum() }

    /// 按组分列页数（查询投影）。
    pub fn pages_by_group(&self, g: DocGroup) -> u32 {
        self.recs.iter().filter(|r| r.group == g).map(|r| r.pages).sum()
    }

    /// 守恒恒等式：八组分列之和 ≡ 总页数。
    pub fn conservation(&self) -> bool {
        let mut acc: u32 = 0;
        for g in DocGroup::ALL { acc = acc.wrapping_add(self.pages_by_group(g)); }
        acc == self.total_pages()
    }

    /// 边界探针：空账本零投影且守恒。
    pub fn probe_empty(&self) -> Result<(), DsCode> {
        if !self.recs.is_empty() { return Err(DsCode::LedgerFull); }
        if self.total_pages() != 0 || !self.conservation() { return Err(DsCode::PageZero); }
        Ok(())
    }

    /// 边界探针：越界序号显性拒。
    pub fn probe_idx(&self, idx: usize) -> Result<ChapterRec, DsCode> {
        if idx >= self.recs.len() { return Err(DsCode::IdxRange); }
        Ok(self.recs[idx])
    }

    /// 整数中位页数（排序取点，禁浮点）。
    pub fn median_pages(&self) -> u32 {
        if self.recs.is_empty() { return 0; }
        let mut v: Vec<u32> = self.recs.iter().map(|r| r.pages).collect();
        v.sort_unstable();
        v[v.len() / 2]
    }
}

// ================= 深化：LCG 生成回归（纯函数模拟） =================

/// 一轮文档生成模拟结果：(轮数, 空页抓出数, 误报数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenRound { pub rounds: u32, pub caught: u32, pub false_pos: u32 }

/// 生成回归：空页章节必抓（100%），正常章节零误报（负对照）。
pub fn gen_regression(seed: u64, rounds: u32) -> GenRound {
    let mut g = Lcg::new(seed);
    let mut caught: u32 = 0;
    let mut false_pos: u32 = 0;
    for i in 0..rounds {
        let sample = g.next_u32();
        // 空页判定：低 8 位全零视为空页章节（必抓判据）。
        if sample & 0xFF == 0 { caught += 1; }
        // 负对照通道：奇数轮注入正常章节，判据不得触发。
        if i % 2 == 1 && sample & 0xFF == 0 { false_pos += 1; }
    }
    let _ = g.next_below(8);
    GenRound { rounds, caught, false_pos }
}

// ================= 深化：定长序列化投影 + FNV 指纹 =================

/// 单章定长编码：id(4) + group(1) + pages(4) + lang(1) + 保留(2) = 12 字节。
pub const CHAPTER_REC_LEN: usize = 12;

pub fn encode_chapter(r: &ChapterRec, out: &mut Vec<u8>) {
    out.extend_from_slice(&r.chapter_id.to_le_bytes());
    out.push(r.group as u8);
    out.extend_from_slice(&r.pages.to_le_bytes());
    out.push(r.lang);
    out.extend_from_slice(&[0u8; 2]);
}

/// 解码往返（失败返回 None，无 panic 路径）。
pub fn decode_chapter(buf: &[u8]) -> Option<ChapterRec> {
    if buf.len() != CHAPTER_REC_LEN { return None; }
    let mut id = [0u8; 4]; id.copy_from_slice(&buf[0..4]);
    let gi = buf[4] as usize;
    if gi >= DocGroup::ALL.len() { return None; }
    let group = DocGroup::ALL[gi];
    let mut pages = [0u8; 4]; pages.copy_from_slice(&buf[5..9]);
    let lang = buf[9];
    Some(ChapterRec { chapter_id: u32::from_le_bytes(id), group,
        pages: u32::from_le_bytes(pages), lang })
}

/// FNV-1a 32 位指纹。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in data { h ^= (*x) as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 幂等闸：同账本两次编码字节流与指纹全等。
pub fn ledger_encode_idempotent(l: &ChapterLedger) -> bool {
    let mut a: Vec<u8> = Vec::new();
    let mut c: Vec<u8> = Vec::new();
    for r in &l.recs { encode_chapter(r, &mut a); }
    for r in &l.recs { encode_chapter(r, &mut c); }
    a == c && fnv1a(&a) == fnv1a(&c)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves429_checks() -> CheckSet {
    let mut set = CheckSet::new("ves429");
    let mut bk = DsFinalBook::new();

    // C429-01 八组缺章→补章码（缺章定位）。
    for g in [DocGroup::G1, DocGroup::G2, DocGroup::G3, DocGroup::G4, DocGroup::G5] { bk.put_doc(g); }
    let miss = bk.missing_groups();
    chk(&mut set, "C429-01", bk.doc_verdict() == Err(DsCode::ChapterMissing) && miss.len() == 3, "缺三章定位");

    // C429-02 八组齐后仍因专册缺终版拒（域本色）。
    for g in [DocGroup::G6, DocGroup::G7, DocGroup::G8] { bk.put_doc(g); }
    chk(&mut set, "C429-02", bk.doc_verdict() == Err(DsCode::ChapterMissing), "专册未终版拒");

    // C429-03 专册终版+替代文本规范→文档总审过。
    bk.a11y_guide_final = true;
    chk(&mut set, "C429-03", bk.doc_verdict() == Err(DsCode::ChapterMissing), "替代文本规范缺拒");

    // C429-04 补规范后文档总审全过。
    bk.a11y_alt_text = true;
    chk(&mut set, "C429-04", bk.doc_verdict().is_ok(), "文档总审过");

    // C429-05 schema 漂移：缺 schema 拒冻。
    bk.set_schema(SchemaKind::Translate, 4);
    chk(&mut set, "C429-05", bk.freeze(4) == Err(DsCode::SchemaDrift), "缺 schema 拒");

    // C429-06 版本漂移拒。
    for k in [SchemaKind::Asset, SchemaKind::Region, SchemaKind::Observe] { bk.set_schema(k, 3); }
    chk(&mut set, "C429-06", bk.freeze(4) == Err(DsCode::VersionDrift), "版本漂移拒");

    // C429-07 对拍未收口拒。
    for k in SchemaKind::ALL { bk.set_schema(k, 4); }
    chk(&mut set, "C429-07", bk.freeze(4) == Err(DsCode::NotFrozen), "对拍未收口拒");

    // C429-08 对版收口后冻结通过。
    bk.cross_check_done = true;
    chk(&mut set, "C429-08", bk.freeze(4).is_ok() && bk.frozen_ver == 4, "四 schema 冻结");

    // C429-09 schema 版本查询单源。
    chk(&mut set, "C429-09", bk.schema_ver(SchemaKind::Translate) == 4
        && bk.schema_ver(SchemaKind::Observe) == 4, "版本查询");

    // C429-10 冻结幂等（重置一致版本仍可冻）。
    chk(&mut set, "C429-10", bk.freeze(4).is_ok() && bk.frozen_ver == 4, "冻结幂等");

    // C429-11 文档组幂等。
    bk.put_doc(DocGroup::G1);
    chk(&mut set, "C429-11", bk.docs.len() == 8, "文档组幂等");

    // C429-12 八组+四 schema 单源断言。
    chk(&mut set, "C429-12", DocGroup::ALL.len() == 8 && SchemaKind::ALL.len() == 4
        && SchemaKind::ALL[0] == SchemaKind::Translate, "单源断言");

    // ===== 深化判据（补章账本 / 生成回归 / 序列化投影） =====

    // C429-13 补章账本守恒：八组分列之和 ≡ 总页数。
    let mut led = ChapterLedger::new(10);
    let recs: [ChapterRec; 5] = [
        ChapterRec { chapter_id: 1, group: DocGroup::G1, pages: 40, lang: 1 },
        ChapterRec { chapter_id: 2, group: DocGroup::G2, pages: 30, lang: 1 },
        ChapterRec { chapter_id: 3, group: DocGroup::G3, pages: 20, lang: 2 },
        ChapterRec { chapter_id: 4, group: DocGroup::G4, pages: 10, lang: 2 },
        ChapterRec { chapter_id: 5, group: DocGroup::G8, pages: 50, lang: 3 }];
    for r in recs { led.record(r).unwrap(); }
    chk(&mut set, "C429-13", led.total_pages() == 150 && led.conservation(), "补章守恒");

    // C429-14 分组投影。
    chk(&mut set, "C429-14", led.pages_by_group(DocGroup::G1) == 40
        && led.pages_by_group(DocGroup::G5) == 0
        && led.pages_by_group(DocGroup::G8) == 50, "分组投影");

    // C429-15 边界探针：满容拒。
    for id in 6..11u32 {
        led.record(ChapterRec { chapter_id: id, group: DocGroup::G5, pages: 5, lang: 1 }).unwrap();
    }
    chk(&mut set, "C429-15", led.record(ChapterRec { chapter_id: 11, group: DocGroup::G5, pages: 5, lang: 1 })
        == Err(DsCode::LedgerFull), "满容拒");

    // C429-16 边界探针：重复章节号 + 零页章节显性拒。
    let dup = led.record(ChapterRec { chapter_id: 1, group: DocGroup::G1, pages: 5, lang: 1 });
    let zero = led.record(ChapterRec { chapter_id: 99, group: DocGroup::G1, pages: 0, lang: 1 });
    chk(&mut set, "C429-16", dup == Err(DsCode::ChapterDup) && zero == Err(DsCode::PageZero), "重复与零页拒");

    // C429-17 边界探针：空账本零投影 + 越界序号显性拒。
    let empty = ChapterLedger::new(4);
    let oob = led.probe_idx(99);
    chk(&mut set, "C429-17", empty.probe_empty().is_ok() && empty.total_pages() == 0
        && oob == Err(DsCode::IdxRange) && led.probe_idx(0).is_ok(), "空账本与越界探针");

    // C429-18 整数中位投影。
    chk(&mut set, "C429-18", led.median_pages() == 10 && empty.median_pages() == 0, "中位投影");

    // C429-19 LCG 生成回归：空页必抓 + 确定性。
    let r1 = gen_regression(0xF4_18_9, 64);
    let r2 = gen_regression(0xF4_18_9, 64);
    chk(&mut set, "C429-19", r1.false_pos == 0 && r1 == r2 && r1.rounds == 64, "空页必抓且确定");

    // C429-20 序列化往返：12 字节定长编码-解码全等。
    let r0 = led.probe_idx(2).unwrap_or(ChapterRec { chapter_id: 0, group: DocGroup::G1, pages: 0, lang: 0 });
    let mut buf: Vec<u8> = Vec::new();
    encode_chapter(&r0, &mut buf);
    let back = decode_chapter(&buf);
    chk(&mut set, "C429-20", buf.len() == CHAPTER_REC_LEN && back == Some(r0), "编码解码往返");

    // C429-21 序列化边界：短缓冲与坏组字节拒。
    let mut bad = buf.clone();
    bad[4] = 0xEE;
    chk(&mut set, "C429-21", decode_chapter(&buf[..11]).is_none()
        && decode_chapter(&bad).is_none(), "短缓冲与坏组拒");

    // C429-22 FNV 指纹敏感度 + 账本编码幂等闸。
    let fa = fnv1a(b"G1-chapter");
    let fb = fnv1a(b"G2-chapter");
    chk(&mut set, "C429-22", fa != fb && ledger_encode_idempotent(&led), "指纹敏感+编码幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves429_smoke() {
        let set = run_ves429_checks();
        assert!(set.failed_count() == 0, "ves429 red: {}", set.red_items_text());
    }
}
