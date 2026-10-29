#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0771 · 四项齐备全域抽核（定位/边界/正文长度）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 2 单）
// 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥570 行；② 判据原文：UNX-F0771-J1 抽核过（定位/边界全域长度 100%
//   + 正文 ≥300 字全域 len 复算 + 齐备性抽样 ≥40 条）；③ 完成时 API/软件写回：
//   状态=已完成、结果摘要非空。
// 设计纪律：① 定位指针与边界区间两项走全域 100% 长度核（800/800），正文走
//   全域 len 复算（双法交叉：chars 计数 vs UTF-8 首字节扫描计数，必须逐条一致）。
// ② 齐备性走定步长抽样核（stride=20 恰 40 条，≥40 判据下限），序列写死可复算。
// ③ 计数走三路守恒（总账 = 定位过 + 定位红 = 边界过 + 边界红 = 正文过 + 正文红）。
// ④ 状态机为流水线闭集（Intake→…→Closed），乱序进入一律管线拒绝。
// ⑤ 常量写死注来源；正文下限 300 字、抽样 40 条直接取自判据原文。
// 诊断段声明：0x5F20–0x5F3F 本单独占（清单 <SEG 0x5f20>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 1. 诊断码段（本单独占 0x5F20–0x5F3F）
// ===========================================================================

pub const SEG_BASE: u16 = 0x5F20; // 诊断码段基数，来源：W7 清单 UNX-F0771 行 <SEG 0x5f20>
pub const SEG_TOP: u16 = 0x5F3F; // 诊断码段顶界（含），32 码位闭集

pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 管线阶段非法（乱序/回头边）
pub const XC_TOTAL_SHORT: u16 = SEG_BASE + 2; // 登记总数不足 800（全域口径被破坏）
pub const XC_DUP_ID: u16 = SEG_BASE + 3; // 文档号重复
pub const XC_EMPTY_BATCH: u16 = SEG_BASE + 4; // 空批次喂入
pub const XC_LOCATE_BAD: u16 = SEG_BASE + 5; // 定位指针长度/格式不达标（全域核）
pub const XC_BOUNDARY_BAD: u16 = SEG_BASE + 6; // 边界区间非法（起止倒挂/越界/零跨度）
pub const XC_BODY_SHORT: u16 = SEG_BASE + 7; // 正文不足 300 字
pub const XC_RECOUNT_DIFF: u16 = SEG_BASE + 8; // 双法 len 复算不一致（账面被扰动）
pub const XC_SUM_MISMATCH: u16 = SEG_BASE + 9; // 三路守恒断裂
pub const XC_SUM_OVERFLOW: u16 = SEG_BASE + 10; // 计数加法溢出
pub const XC_QUAD_NOT_READY: u16 = SEG_BASE + 11; // 前置长度面未走完即请求抽样
pub const XC_SAMPLE_SHORT: u16 = SEG_BASE + 12; // 抽样条数不足 40
pub const XC_SAMPLE_DUP: u16 = SEG_BASE + 13; // 抽样下标重复
pub const XC_QUAD_MISSING: u16 = SEG_BASE + 14; // 抽样核检出四项缺项
pub const XC_ALREADY_CLOSED: u16 = SEG_BASE + 15; // 管线已闭卷后再请求核

// ===========================================================================
// 2. 判据常量（全部写死，来源=判据原文或头注假设）
// ===========================================================================

pub const TOTAL_ENTRIES: usize = 800; // 全域条数：800，来源：与 F0770 同批册口径（头注假设）
pub const SAMPLE_MIN: usize = 40; // 齐备性抽样下限：40，来源：判据原文「抽样 ≥40 条」
pub const SAMPLE_STRIDE: usize = TOTAL_ENTRIES / SAMPLE_MIN; // 步长 20，全域等距派生
pub const BODY_MIN_CHARS: usize = 300; // 正文下限：300 字，来源：判据原文「正文 ≥300 字」
pub const LOCATE_MIN_LEN: usize = 6; // 定位指针最短长度，来源：@L+4 位号格式派生（头注假设）
pub const DOC_EXTENT: u32 = 4096; // 文档总行数上界，来源：批册卷 extent 常量（头注假设）
pub const BOUNDARY_MIN_SPAN: u32 = 1; // 边界最小跨度（end−start ≥ 1），来源：非空区间定义

// ===========================================================================
// 3. 数据结构（四项：定位/边界/正文/齐备标记）
// ===========================================================================

/// 一条待核条目：对应批册中一份卡面文档的四项载体。
#[derive(Clone, Debug)]
pub struct QuadEntry {
    pub did: u16,       // 文档号（1..=800，全域唯一）
    pub locate: String, // 定位指针（要求 @L 前缀 + 4 位号，长度 ≥ LOCATE_MIN_LEN）
    pub start_line: u32, // 边界起行（含，≥1）
    pub end_line: u32,   // 边界止行（含，> start，≤ DOC_EXTENT）
    pub body: String,    // 正文（复算对象，要求 ≥300 字）
    pub quad_tag: u32,   // 齐备标记（= did*4+1 的确定性配对值，抽样核对象）
}

/// 长度面（定位+边界+正文）逐项结论。
#[derive(Clone, Debug)]
pub struct LengthReport {
    pub locate_ok: u32,     // 定位过条数
    pub locate_bad: u32,    // 定位红条数
    pub boundary_ok: u32,   // 边界过条数
    pub boundary_bad: u32,  // 边界红条数
    pub body_ok: u32,       // 正文过条数
    pub body_bad: u32,      // 正文红条数
    pub first_bad_did: u16, // 首个红案文档号（无红案为 0）
    pub first_bad_code: u16, // 首个红案码
}

/// 齐备性抽样单条记录。
#[derive(Clone, Debug, PartialEq)]
pub struct QuadSampleRecord {
    pub index: usize,     // 全域下标
    pub did: u16,         // 文档号
    pub quad_ok: bool,    // 四项齐备结论
    pub note: String,     // 结论注记
}

/// 抽样报告。
#[derive(Clone, Debug, PartialEq)]
pub struct QuadSampleReport {
    pub sampled: u32,          // 抽样条数（≥40）
    pub missing: u32,          // 缺项条数（必须 0）
    pub records: Vec<QuadSampleRecord>,
}

/// 终判报告。
#[derive(Clone, Debug, PartialEq)]
pub struct QuadVerdict {
    pub length_all_pass: bool, // 长度面 100% 过
    pub sample_all_pass: bool, // 抽样面全过
    pub body_total_chars: u64, // 全域正文总字数（复算累计，可对拍）
    pub summary: String,       // 结论摘要（非空）
    pub fingerprint: u64,      // 账面指纹（FNV-1a-64）
}

// ===========================================================================
// 4. 管线状态机（合法边闭集：单向前进了，禁回头）
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeStage {
    Intake = 0,       // 登记中
    LengthAudit = 1,  // 长度面（定位/边界/正文）已过
    QuadSample = 2,   // 齐备性抽样已过
    Closed = 3,       // 已闭卷（只读）
}

impl PipeStage {
    pub fn can_step(self, next: PipeStage) -> bool {
        (self as u8) + 1 == next as u8 // 流水线只允许严格 +1 的边
    }
}

// ===========================================================================
// 5. 抽核机本体
// ===========================================================================

pub struct QuadAuditor {
    stage: PipeStage,
    book: Vec<QuadEntry>,
    seen_ids: Vec<u16>, // did 定位账（与 book 等长，守恒：len 相等）
    length: Option<LengthReport>,
    sample: Option<QuadSampleReport>,
    events: Vec<String>, // 审计事件账（只增不删）
}

impl QuadAuditor {
    pub fn new() -> Self {
        QuadAuditor {
            stage: PipeStage::Intake,
            book: Vec::new(),
            seen_ids: Vec::new(),
            length: None,
            sample: None,
            events: Vec::new(),
        }
    }

    pub fn feed_batch(&mut self, batch: &[QuadEntry]) -> Result<u32, u16> {  // 喂入一批条目：非空批、did 唯一、总量守恒。
        if batch.is_empty() {
            return Err(XC_EMPTY_BATCH);
        }
        if self.stage != PipeStage::Intake {
            return Err(XC_STATE_INVALID);
        }
        let mut added: u32 = 0;
        for e in batch.iter() {
            if self.book.len() >= TOTAL_ENTRIES {
                return Err(XC_TOTAL_SHORT);
            }
            let mut dup = false;
            let mut i = 0usize;
            while i < self.seen_ids.len() {
                if self.seen_ids[i] == e.did {
                    dup = true;
                    break;
                }
                i += 1;
            }
            if dup {
                return Err(XC_DUP_ID);
            }
            self.seen_ids.push(e.did);
            self.book.push(e.clone());
            added = match added.checked_add(1) {
                Some(v) => v,
                None => return Err(XC_SUM_OVERFLOW),
            };
        }
        self.events.push(alloc::format!("feed +{} total={}", added, self.book.len()));
        Ok(added)
    }

    pub fn is_full(&self) -> bool {
        self.book.len() == TOTAL_ENTRIES
    }
    pub fn stage(&self) -> PipeStage {
        self.stage
    }
    pub fn registered(&self) -> usize {
        self.book.len()
    }

    /// 定位指针单条核：@L 前缀 + 长度下限。
    fn locate_ok(e: &QuadEntry) -> bool {
        e.locate.len() >= LOCATE_MIN_LEN && e.locate.starts_with("@L")
    }

    /// 边界单条核：1 ≤ start < end ≤ extent 且跨度达下限。
    fn boundary_ok(e: &QuadEntry) -> bool {
        e.start_line >= 1
            && e.end_line > e.start_line
            && e.end_line <= DOC_EXTENT
            && (e.end_line - e.start_line) >= BOUNDARY_MIN_SPAN
    }

    /// 正文 len 复算双法：A=chars().count()，B=UTF-8 首字节扫描。必须相等。
    fn body_recount(e: &QuadEntry) -> Result<usize, u16> {
        let a = e.body.chars().count();
        let bytes = e.body.as_bytes();
        let mut b = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] & 0xC0 != 0x80 {
                b += 1; // 首字节（含 ASCII）即一个字符
            }
            i += 1;
        }
        if a == b {
            Ok(a)
        } else {
            Err(XC_RECOUNT_DIFF)
        }
    }

    pub fn length_audit(&mut self) -> Result<LengthReport, u16> {  // 长度面全域核：定位/边界/正文三项 800/800 逐条 100%。
        if !self.stage.can_step(PipeStage::LengthAudit) {
            return Err(if self.book.len() != TOTAL_ENTRIES {
                XC_TOTAL_SHORT
            } else {
                XC_STATE_INVALID
            });
        }
        let mut rep = LengthReport {
            locate_ok: 0,
            locate_bad: 0,
            boundary_ok: 0,
            boundary_bad: 0,
            body_ok: 0,
            body_bad: 0,
            first_bad_did: 0,
            first_bad_code: 0,
        };
        let mut total_chars: u64 = 0;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            let mut bad: Option<u16> = None;
            if Self::locate_ok(e) {
                rep.locate_ok += 1;
            } else {
                rep.locate_bad += 1;
                bad = Some(XC_LOCATE_BAD);
            }
            if Self::boundary_ok(e) {
                rep.boundary_ok += 1;
            } else if bad.is_none() {
                rep.boundary_bad += 1;
                bad = Some(XC_BOUNDARY_BAD);
            } else {
                rep.boundary_bad += 1;
            }
            match Self::body_recount(e) {
                Ok(n) => {
                    total_chars = match total_chars.checked_add(n as u64) {
                        Some(v) => v,
                        None => return Err(XC_SUM_OVERFLOW),
                    };
                    if n >= BODY_MIN_CHARS {
                        rep.body_ok += 1;
                    } else if bad.is_none() {
                        rep.body_bad += 1;
                        bad = Some(XC_BODY_SHORT);
                    } else {
                        rep.body_bad += 1;
                    }
                }
                Err(c) => {
                    rep.body_bad += 1;
                    if bad.is_none() {
                        bad = Some(c);
                    }
                }
            }
            if let Some(c) = bad {
                if rep.first_bad_did == 0 {
                    rep.first_bad_did = e.did;
                    rep.first_bad_code = c;
                }
            }
            i += 1;
        }
        // 三路守恒：每路的 ok+bad 都必须 == 800。
        let l = rep.locate_ok.checked_add(rep.locate_bad).ok_or(XC_SUM_OVERFLOW)?;
        let b = rep.boundary_ok.checked_add(rep.boundary_bad).ok_or(XC_SUM_OVERFLOW)?;
        let d = rep.body_ok.checked_add(rep.body_bad).ok_or(XC_SUM_OVERFLOW)?;
        if l != TOTAL_ENTRIES as u32 || b != TOTAL_ENTRIES as u32 || d != TOTAL_ENTRIES as u32 {
            return Err(XC_SUM_MISMATCH);
        }
        self.stage = PipeStage::LengthAudit;
        self.length = Some(rep.clone());
        self.events
            .push(alloc::format!("length audit chars_total={}", total_chars));
        Ok(rep)
    }

    pub fn quad_sample(&mut self) -> Result<QuadSampleReport, u16> {  // 齐备性抽样核：下标 0,20,…,780 恰 40 条，四项齐备逐条核。
        if !self.stage.can_step(PipeStage::QuadSample) {
            return Err(if self.length.is_none() {
                XC_QUAD_NOT_READY
            } else {
                XC_STATE_INVALID
            });
        }
        let mut records: Vec<QuadSampleRecord> = Vec::new();
        let mut missing: u32 = 0;
        let mut k = 0usize;
        while k < SAMPLE_MIN {
            let idx = k * SAMPLE_STRIDE;
            if idx >= self.book.len() {
                return Err(XC_SAMPLE_SHORT);
            }
            let mut t = 0usize;
            while t < records.len() {
                if records[t].index == idx {
                    return Err(XC_SAMPLE_DUP);
                }
                t += 1;
            }
            let e = &self.book[idx];
            // 四项齐备 = 定位+边界+正文三项过 + quad_tag 配对值正确。
            let tag_ok = e.quad_tag == (e.did as u32) * 4 + 1;
            let q = Self::locate_ok(e) && Self::boundary_ok(e) && tag_ok;
            let mut note = String::from("quad ok");
            if !q {
                note = String::from("quad missing: locate/boundary/tag 缺项");
                missing += 1;
            }
            records.push(QuadSampleRecord { index: idx, did: e.did, quad_ok: q, note });
            k += 1;
        }
        if records.len() < SAMPLE_MIN {
            return Err(XC_SAMPLE_SHORT);
        }
        let rep = QuadSampleReport { sampled: records.len() as u32, missing, records };
        self.stage = PipeStage::QuadSample;
        self.sample = Some(rep.clone());
        self.events.push(alloc::format!("quad sample n={} missing={}", rep.sampled, missing));
        Ok(rep)
    }

    pub fn close(&mut self) -> Result<QuadVerdict, u16> {  // 闭卷终判：长度面 100% + 抽样全过 → 摘要非空定格。
        if !self.stage.can_step(PipeStage::Closed) {
            return Err(if self.stage == PipeStage::Closed {
                XC_ALREADY_CLOSED
            } else {
                XC_QUAD_NOT_READY
            });
        }
        let len = match &self.length {
            Some(l) => l.clone(),
            None => return Err(XC_QUAD_NOT_READY),
        };
        let smp = match &self.sample {
            Some(s) => s.clone(),
            None => return Err(XC_QUAD_NOT_READY),
        };
        let mut total_chars: u64 = 0;
        let mut i = 0usize;
        while i < self.book.len() {
            match Self::body_recount(&self.book[i]) {
                Ok(n) => {
                    total_chars = match total_chars.checked_add(n as u64) {
                        Some(v) => v,
                        None => return Err(XC_SUM_OVERFLOW),
                    };
                }
                Err(c) => return Err(c),
            }
            i += 1;
        }
        let length_all_pass = len.locate_bad == 0 && len.boundary_bad == 0 && len.body_bad == 0;
        let sample_all_pass = smp.missing == 0 && smp.sampled >= SAMPLE_MIN as u32;
        let summary = if length_all_pass && sample_all_pass {
            String::from("UNX-F0771-J1 抽核过：定位/边界全域 100%，正文 ≥300 字全域复算一致，齐备抽样 40/40 过")
        } else {
            String::from("UNX-F0771-J1 抽核未过：长度面或齐备面存在红案")
        };
        let fp = self.fingerprint();
        self.stage = PipeStage::Closed;
        Ok(QuadVerdict {
            length_all_pass,
            sample_all_pass,
            body_total_chars: total_chars,
            summary,
            fingerprint: fp,
        })
    }

    pub fn fingerprint(&self) -> u64 {  // FNV-1a-64 指纹：对（did, locate, start, end, body 采样字节）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            for v in [e.did as u64, e.start_line as u64, e.end_line as u64, e.quad_tag as u64] {
                let mut b = 0usize;
                while b < 8 {
                    h ^= (v >> (b * 8)) & 0xFF;
                    h = h.wrapping_mul(0x0000_0100_0000_01B3);
                    b += 1;
                }
            }
            // 正文取前 16 字节入指纹（800 条全量字节成本可控性取舍）。
            let bs = e.body.as_bytes();
            let mut j = 0usize;
            while j < bs.len() && j < 16 {
                h ^= bs[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
                j += 1;
            }
            i += 1;
        }
        h
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 按 did 查定位指针（账面只读查询 API，供写回面解引）。
    pub fn locate_of(&self, did: u16) -> Option<String> {
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].did == did {
                return Some(self.book[i].locate.clone());
            }
            i += 1;
        }
        None
    }

    /// 按 did 查正文复算长度（None=未登记；Err=双法复算不一致红案）。
    pub fn body_len_of(&self, did: u16) -> Option<Result<usize, u16>> {
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].did == did {
                return Some(Self::body_recount(&self.book[i]));
            }
            i += 1;
        }
        None
    }

    /// 边界跨度查询（end−start 整数跨度；未登记返回 None）。
    pub fn span_of(&self, did: u16) -> Option<u32> {
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].did == did {
                let e = &self.book[i];
                return Some(e.end_line.saturating_sub(e.start_line));
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// 6. 样本发生器（确定性 LCG；正文由中文句重复拼装，逐条 ≥300 字）
// ===========================================================================

const BODY_SENTENCE: &str = "判据正文锚点复算段";

pub fn gen_quads(seed: u64) -> Vec<QuadEntry> {  // 确定性生成 800 条四项齐备的条目。
    let mut s = seed;
    let mut out: Vec<QuadEntry> = Vec::new();
    let mut did: u16 = 1;
    while did <= TOTAL_ENTRIES as u16 {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let r = (s >> 33) as u32;
        // 正文：句子重复 22 次（每句 9 字 → 198 字？不够）→ 34 次保证 ≥300 字。
        let mut body = String::new();
        let mut n = 0usize;
        while n < 34 {
            body.push_str(BODY_SENTENCE);
            n += 1;
        }
        let start = 1 + (r % (DOC_EXTENT / 2));
        let end = start + 1 + (r % 32);
        out.push(QuadEntry {
            did,
            locate: alloc::format!("@L{:04}", did),
            start_line: start,
            end_line: end,
            body,
            quad_tag: (did as u32) * 4 + 1,
        });
        did += 1;
    }
    out
}

// ===========================================================================
// 7. 自例程 drill：主路径 + 错误路径，0=全过
// ===========================================================================

pub fn drill() -> Result<u32, u16> {
    // —— 主路径：分 4 批喂满 → 长度面 → 抽样 → 闭卷 ——
    let mut a = QuadAuditor::new();
    let quads = gen_quads(0x5F20_0000_0000_0001);
    let mut off = 0usize;
    while off < TOTAL_ENTRIES {
        let end = if off + 200 < TOTAL_ENTRIES { off + 200 } else { TOTAL_ENTRIES };
        a.feed_batch(&quads[off..end])?;
        off = end;
    }
    if !a.is_full() || a.registered() != TOTAL_ENTRIES {
        return Err(XC_TOTAL_SHORT);
    }
    let lr = a.length_audit()?;
    if lr.locate_bad != 0 || lr.boundary_bad != 0 || lr.body_bad != 0 {
        return Err(XC_LOCATE_BAD);
    }
    let sr = a.quad_sample()?;
    if sr.sampled != SAMPLE_MIN as u32 || sr.missing != 0 {
        return Err(XC_QUAD_MISSING);
    }
    let v = a.close()?;
    if !v.length_all_pass || !v.sample_all_pass || v.summary.is_empty() {
        return Err(XC_QUAD_NOT_READY);
    }
    // 闭卷后再闭卷 → 必红。
    if a.close() != Err(XC_ALREADY_CLOSED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：长度面未过即抽样 → XC_QUAD_NOT_READY ——
    let mut a2 = QuadAuditor::new();
    a2.feed_batch(&quads[0..40])?;
    if a2.quad_sample() != Err(XC_QUAD_NOT_READY) {
        return Err(XC_QUAD_NOT_READY);
    }
    // —— 错误路径 2：did 重复 ——
    let mut a3 = QuadAuditor::new();
    let mut dup = quads[0..2].to_vec();
    dup.push(quads[0].clone());
    if a3.feed_batch(&dup) != Err(XC_DUP_ID) {
        return Err(XC_DUP_ID);
    }
    // —— 错误路径 3：正文 <300 字 → 长度面红案定位 ——
    let mut a4 = QuadAuditor::new();
    let mut short = quads.clone();
    short[5].body = String::from("短正文");
    a4.feed_batch(&short)?;
    let l4 = a4.length_audit()?;
    if l4.body_bad != 1 || l4.first_bad_code != XC_BODY_SHORT || l4.first_bad_did != 6 {
        return Err(XC_BODY_SHORT);
    }
    // —— 错误路径 4：边界倒挂 ——
    let mut a5 = QuadAuditor::new();
    let mut inv = quads.clone();
    inv[9].start_line = 200;
    inv[9].end_line = 100;
    a5.feed_batch(&inv)?;
    let l5 = a5.length_audit()?;
    if l5.boundary_bad != 1 || l5.first_bad_code != XC_BOUNDARY_BAD {
        return Err(XC_BOUNDARY_BAD);
    }
    // —— 错误路径 5：齐备标记错 → 抽样必检出 ——
    let mut a6 = QuadAuditor::new();
    let mut tagbad = quads.clone();
    tagbad[SAMPLE_STRIDE].quad_tag = 0; // 配对值破坏
    a6.feed_batch(&tagbad)?;
    let _l6 = a6.length_audit()?;
    let s6 = a6.quad_sample()?;
    if s6.missing != 1 || a6.close().is_ok() {
        return Err(XC_QUAD_MISSING);
    }
    // —— 查询 API 一致性：locate_of / body_len_of / span_of 与账面一致 ——
    let loc1 = match a.locate_of(1) {
        Some(s) => s,
        None => return Err(XC_LOCATE_BAD),
    };
    if loc1 != quads[0].locate {
        return Err(XC_LOCATE_BAD);
    }
    match a.body_len_of(1) {
        Some(Ok(n)) if n == quads[0].body.chars().count() => {}
        _ => return Err(XC_RECOUNT_DIFF),
    }
    let span0 = quads[0].end_line - quads[0].start_line;
    if a.span_of(1) != Some(span0) || a.locate_of(9999).is_some() {
        return Err(XC_BOUNDARY_BAD);
    }
    Ok(0)
}
