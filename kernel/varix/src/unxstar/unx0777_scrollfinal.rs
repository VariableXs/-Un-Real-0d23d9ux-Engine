#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0777 · 域经卷终版（登记簿收官快照定版）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 8 单）
// 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥570 行；② 判据原文：UNX-F0777-J1 终版账齐（登记簿收官快照 +
//   卷哈希 + 与 F0754 导出卷一致核）且快照-簿一致 100%；③ 完成时 API/软件写回：
//   状态=已完成、结果摘要非空。
// 设计纪律：① 快照-簿一致 100%：快照逐条与登记簿逐字段比对（号/标识/档位/
//   通道/强度/残余六字段），任一漂移定位到（条目, 字段）。② 卷哈希 = FNV-1a-64
//   对快照规范序独立复算，与 F0754 导出卷申报哈希一致才准定版。③ 定版单调：
//   Sealed 后禁再快照/再定版；快照行数必须与簿行数守恒。
// ④ 状态机 Draft→Snapshot→Hashed→Sealed 单向闭集；诊断码定位到（条目, 字段）。
// ⑤ 常量写死注来源；禁浮点禁 std。
// 诊断段声明：0x5FE0–0x5FFF 本单独占（清单 <SEG 0x5fe0>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x5FE0–0x5FFF）
pub const SEG_BASE: u16 = 0x5FE0; // 诊断码段基数，来源：W7 清单 UNX-F0777 行 <SEG 0x5fe0>
pub const SEG_TOP: u16 = 0x5FFF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 定版状态机非法边
pub const XC_EMPTY_BOOK: u16 = SEG_BASE + 2; // 登记簿为空
pub const XC_EMPTY_SNAP: u16 = SEG_BASE + 3; // 快照为空
pub const XC_DUP_RID: u16 = SEG_BASE + 4; // 风险号重复
pub const XC_LEN_MISMATCH: u16 = SEG_BASE + 5; // 快照行数 ≠ 簿行数（守恒红）
pub const XC_SNAP_DRIFT: u16 = SEG_BASE + 6; // 快照-簿字段漂移（一致 100% 断言红）
pub const XC_SCROLL_HASH_DIFF: u16 = SEG_BASE + 7; // 卷哈希与 F0754 申报不一致
pub const XC_NOT_SNAPSHOTTED: u16 = SEG_BASE + 8; // 未快照即请求定版
pub const XC_ALREADY_SEALED: u16 = SEG_BASE + 9; // 已定版再操作
pub const XC_GRADE_BAD: u16 = SEG_BASE + 10; // 档位越出三档闭集
pub const XC_RESIDUAL_CAP: u16 = SEG_BASE + 11; // 残余超档位上限
pub const XC_EXPORT_ZERO: u16 = SEG_BASE + 12; // F0754 申报哈希为 0
pub const XC_RID_ZERO: u16 = SEG_BASE + 13; // 风险号为 0
pub const XC_SUM_OVERFLOW: u16 = SEG_BASE + 14; // 计数溢出
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 定版摘要为空

// 2. 卷常量（写死，来源=判据原文/本单口径头注假设）
pub const SCROLL_MIN: usize = 8; // 卷最小行数（与 F0776 登记簿 ≥8 对齐）
pub const SCROLL_CAP: usize = 16; // 卷定容上限（与 F0776 簿容对齐）
pub const FIELDS_PER_ROW: usize = 6; // 每行六字段：号/标识/档位/通道/强度/残余
pub const EXPORT_VOL_ID: u32 = 0x0F54_0001; // F0754 导出卷编号（头注假设定格）

/// 域经卷一行（与 F0776 四列风险登记同构）。
#[derive(Clone, Copy, Debug)]
pub struct ScrollRow {
    pub rid: u32,        // 风险号
    pub risk_hash: u64,  // 风险标识指纹
    pub grade: u8,       // 档位 0..=2
    pub channel: u8,     // 缓解通道 1..=4
    pub strength: u8,    // 缓解强度 0..=3
    pub residual: u8,    // 残余 0..=3
}
impl ScrollRow {
    pub fn residual_cap(grade: u8) -> Result<u8, u16> {  // 残余上限（与 F0776 口径同源）：高→1 中→2 低→3。
        match grade {
            0 => Ok(1),
            1 => Ok(2),
            2 => Ok(3),
            _ => Err(XC_GRADE_BAD),
        }
    }
    pub fn self_ok(&self) -> Result<bool, u16> {  // 行自检：档位闭集 + 残余上限 + 通道闭集。
        let cap = Self::residual_cap(self.grade)?;
        if self.channel < 1 || self.channel > 4 {
            return Err(XC_GRADE_BAD);
        }
        Ok(self.residual <= cap)
    }
}

/// 漂移定位记录。
#[derive(Clone, Debug)]
pub struct DriftMark {
    pub rid: u32,       // 风险号
    pub field: usize,   // 字段序 0..=5
    pub book_val: u64,  // 簿侧值
    pub snap_val: u64,  // 快照侧值
}

/// F0754 导出卷申报单。
#[derive(Clone, Copy, Debug)]
pub struct ExportDecl {
    pub vol_id: u32,        // 卷编号（须 == EXPORT_VOL_ID）
    pub rows: u32,          // 行数申报
    pub declared_hash: u64, // 卷哈希申报
}

/// 定版回执。
#[derive(Clone, Debug, PartialEq)]
pub struct SealReceipt {
    pub rows: u32,          // 定版行数
    pub scroll_hash: u64,   // 卷哈希（复算值）
    pub export_match: bool, // 与 F0754 导出卷一致结论
    pub drift_count: u32,   // 快照-簿漂移数（必须 0）
    pub summary: String,    // 摘要（非空）
    pub fingerprint: u64,   // 卷面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollStage {
    Draft = 0,
    Snapshot = 1,
    Hashed = 2,
    Sealed = 3,
}
impl ScrollStage {
    pub fn can_step(self, next: ScrollStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct ScrollFinalizer {  // 域经卷终版机。
    stage: ScrollStage,
    book: Vec<ScrollRow>, // 登记簿（来源：F0776 收官簿）
    snap: Vec<ScrollRow>, // 收官快照
    drifts: Vec<DriftMark>,
    scroll_hash: u64,
    events: Vec<String>,
}
impl ScrollFinalizer {
    pub fn new() -> Self {
        ScrollFinalizer {
            stage: ScrollStage::Draft,
            book: Vec::new(),
            snap: Vec::new(),
            drifts: Vec::new(),
            scroll_hash: 0,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> ScrollStage {
        self.stage
    }
    pub fn book_len(&self) -> usize {
        self.book.len()
    }
    pub fn snap_len(&self) -> usize {
        self.snap.len()
    }
    pub fn drift_count(&self) -> usize {
        self.drifts.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn load_book(&mut self, rows: &[ScrollRow]) -> Result<u32, u16> {  // 装载登记簿（F0776 收官簿直入，号唯一、行自检）。
        if self.stage != ScrollStage::Draft || !self.book.is_empty() {
            return Err(XC_STATE_INVALID);
        }
        if rows.is_empty() {
            return Err(XC_EMPTY_BOOK);
        }
        let mut added: u32 = 0;
        for r in rows.iter() {
            if self.book.len() >= SCROLL_CAP {
                return Err(XC_SUM_OVERFLOW);
            }
            if r.rid == 0 {
                return Err(XC_RID_ZERO);
            }
            if r.self_ok() != Ok(true) {
                return Err(XC_RESIDUAL_CAP);
            }
            let mut i = 0usize;
            while i < self.book.len() {
                if self.book[i].rid == r.rid {
                    return Err(XC_DUP_RID);
                }
                i += 1;
            }
            self.book.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("book +{}", added));
        Ok(added)
    }
    pub fn take_snapshot(&mut self, snap: &[ScrollRow]) -> Result<u32, u16> {  // 收官快照：快照行数守恒 + 逐字段比对 100%。
        if !self.stage.can_step(ScrollStage::Snapshot) {
            return Err(XC_STATE_INVALID);
        }
        if snap.is_empty() {
            return Err(XC_EMPTY_SNAP);
        }
        if snap.len() != self.book.len() {
            return Err(XC_LEN_MISMATCH);
        }
        self.snap.clear();
        self.drifts.clear();
        let mut i = 0usize;
        while i < self.book.len() {
            let b = &self.book[i];
            // 快照按同序逐条比对（号对号）。
            let s = &snap[i];
            if s.rid != b.rid {
                self.drifts.push(DriftMark { rid: b.rid, field: 0, book_val: b.rid as u64, snap_val: s.rid as u64 });
            }
            if s.risk_hash != b.risk_hash {
                self.drifts.push(DriftMark { rid: b.rid, field: 1, book_val: b.risk_hash, snap_val: s.risk_hash });
            }
            if s.grade != b.grade {
                self.drifts.push(DriftMark { rid: b.rid, field: 2, book_val: b.grade as u64, snap_val: s.grade as u64 });
            }
            if s.channel != b.channel {
                self.drifts.push(DriftMark { rid: b.rid, field: 3, book_val: b.channel as u64, snap_val: s.channel as u64 });
            }
            if s.strength != b.strength {
                self.drifts.push(DriftMark { rid: b.rid, field: 4, book_val: b.strength as u64, snap_val: s.strength as u64 });
            }
            if s.residual != b.residual {
                self.drifts.push(DriftMark { rid: b.rid, field: 5, book_val: b.residual as u64, snap_val: s.residual as u64 });
            }
            self.snap.push(*s);
            i += 1;
        }
        self.stage = ScrollStage::Snapshot;
        self.events
            .push(alloc::format!("snapshot rows={} drifts={}", self.snap.len(), self.drifts.len()));
        Ok(self.drifts.len() as u32)
    }
    pub fn compute_scroll_hash(&mut self) -> Result<u64, u16> {  // 卷哈希复算：FNV-1a-64 对（卷号，行数，逐行六字段）规范序。
        if !self.stage.can_step(ScrollStage::Hashed) {
            return Err(XC_NOT_SNAPSHOTTED);
        }
        let mut h = fnv1a(0xcbf2_9ce4_8422_2325, EXPORT_VOL_ID as u64);
        h = fnv1a(h, self.snap.len() as u64);
        let mut i = 0usize;
        while i < self.snap.len() {
            let r = &self.snap[i];
            h = fnv1a(h, r.rid as u64);
            h = fnv1a(h, r.risk_hash);
            h = fnv1a(h, r.grade as u64);
            h = fnv1a(h, r.channel as u64);
            h = fnv1a(h, r.strength as u64);
            h = fnv1a(h, r.residual as u64);
            i += 1;
        }
        self.scroll_hash = h;
        self.stage = ScrollStage::Hashed;
        self.events.push(alloc::format!("hash {:#018x}", h));
        Ok(h)
    }
    pub fn current_hash(&self) -> u64 {
        self.scroll_hash
    }
    pub fn seal(&mut self, decl: &ExportDecl) -> Result<SealReceipt, u16> {  // 与 F0754 导出卷一致核 + 定版：哈希一致 + 快照-簿零漂移 → 出回执。
        if !self.stage.can_step(ScrollStage::Sealed) {
            return Err(if self.stage == ScrollStage::Sealed {
                XC_ALREADY_SEALED
            } else {
                XC_NOT_SNAPSHOTTED
            });
        }
        if decl.vol_id != EXPORT_VOL_ID {
            return Err(XC_EXPORT_ZERO);
        }
        if decl.declared_hash == 0 {
            return Err(XC_EXPORT_ZERO);
        }
        if decl.rows as usize != self.snap.len() {
            return Err(XC_LEN_MISMATCH);
        }
        if !self.drifts.is_empty() {
            return Err(XC_SNAP_DRIFT);
        }
        if self.scroll_hash != decl.declared_hash {
            return Err(XC_SCROLL_HASH_DIFF);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0777-J1 终版账齐：登记簿收官快照 {} 行与簿一致 100%（零漂移），卷哈希 {:#018x} 与 F0754 导出卷一致，定版完成",
            self.snap.len(), self.scroll_hash
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        self.stage = ScrollStage::Sealed;
        Ok(SealReceipt {
            rows: self.snap.len() as u32,
            scroll_hash: self.scroll_hash,
            export_match: true,
            drift_count: 0,
            summary,
            fingerprint: fp,
        })
    }
    pub fn fingerprint(&self) -> u64 {  // 卷面指纹：快照规范序（独立于卷哈希，双哈希互证）。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.snap.len() {
            let r = &self.snap[i];
            for v in [r.rid as u64, r.risk_hash, r.grade as u64, r.channel as u64, r.strength as u64, r.residual as u64] {
                h = fnv1a(h, v);
            }
            i += 1;
        }
        h
    }
    pub fn book_row(&self, rid: u32) -> Option<ScrollRow> {  // 簿行查询（未登记 None）。
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].rid == rid {
                return Some(self.book[i]);
            }
            i += 1;
        }
        None
    }
    pub fn snap_row(&self, rid: u32) -> Option<ScrollRow> {  // 快照行查询（未快照 None）。
        let mut i = 0usize;
        while i < self.snap.len() {
            if self.snap[i].rid == rid {
                return Some(self.snap[i]);
            }
            i += 1;
        }
        None
    }
    pub fn drifts(&self) -> &[DriftMark] {  // 漂移明细（只读）。
        &self.drifts
    }

    pub fn book_grade_histogram(&self) -> Result<[u32; 3], u16> {  // 档位直方图（簿侧）：[高, 中, 低]，三桶和 == 簿行数（守恒）。
        let mut hist = [0u32; 3];
        let mut i = 0usize;
        while i < self.book.len() {
            let g = self.book[i].grade;
            if g > 2 {
                return Err(XC_GRADE_BAD);
            }
            hist[g as usize] += 1;
            i += 1;
        }
        if hist[0] + hist[1] + hist[2] != self.book.len() as u32 {
            return Err(XC_SUM_OVERFLOW);
        }
        Ok(hist)
    }

    pub fn snap_grade_histogram(&self) -> Result<[u32; 3], u16> {  // 快照侧档位直方图（与簿侧逐桶一致是快照-簿一致的聚合面）。
        let mut hist = [0u32; 3];
        let mut i = 0usize;
        while i < self.snap.len() {
            let g = self.snap[i].grade;
            if g > 2 {
                return Err(XC_GRADE_BAD);
            }
            hist[g as usize] += 1;
            i += 1;
        }
        Ok(hist)
    }

    pub fn histogram_crosscheck(&self) -> Result<bool, u16> {  // 直方图互证：簿侧与快照侧逐桶一致 100%（零漂移的聚合断言）。
        let b = self.book_grade_histogram()?;
        let s = self.snap_grade_histogram()?;
        let mut i = 0usize;
        while i < 3 {
            if b[i] != s[i] {
                return Ok(false);
            }
            i += 1;
        }
        Ok(true)
    }

    pub fn residual_histogram(&self) -> Result<[u32; 4], u16> {  // 残余分布（快照侧）：[0,1,2,3] 四桶计数，桶和 == 快照行数。
        let mut hist = [0u32; 4];
        let mut i = 0usize;
        while i < self.snap.len() {
            if self.snap[i].residual > 3 {
                return Err(XC_RESIDUAL_CAP);
            }
            hist[self.snap[i].residual as usize] += 1;
            i += 1;
        }
        Ok(hist)
    }

    pub fn export_view(&self) -> Result<Vec<String>, u16> {  // 导出卷逐行视图（写回面用）：号 × 档位 × 通道 × 强度 × 残余。
        if self.snap.is_empty() {
            return Err(XC_EMPTY_SNAP);
        }
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.snap.len() {
            let r = &self.snap[i];
            let g = ScrollRow::residual_cap(r.grade)?;
            out.push(alloc::format!(
                "V{} 档{} 通道{} 强度{} 残余{}（上限{}）",
                r.rid, r.grade, r.channel, r.strength, r.residual, g
            ));
            i += 1;
        }
        Ok(out)
    }

    pub fn decl_field_check(&self, decl: &ExportDecl) -> Result<[bool; 3], u16> {  // 申报单逐字段核：卷号 / 行数 / 哈希三字段各自结论（定位粒度）。
        if decl.declared_hash == 0 {
            return Err(XC_EXPORT_ZERO);
        }
        Ok([
            decl.vol_id == EXPORT_VOL_ID,
            decl.rows as usize == self.snap.len(),
            decl.declared_hash == self.scroll_hash,
        ])
    }
}
pub fn gen_book(n: usize) -> Vec<ScrollRow> {  // 样本发生器：确定性 n 行登记簿 + 一致快照（可注入单点漂移）。
    let mut out: Vec<ScrollRow> = Vec::new();
    let mut rid = 1u32;
    let mut i = 0usize;
    while i < n {
        let grade = match i % 4 {
            0 => 0u8,
            1 | 2 => 1u8,
            _ => 2u8,
        };
        let residual = ScrollRow::residual_cap(grade).unwrap_or(0);
        out.push(ScrollRow {
            rid,
            risk_hash: 0x5FE0_0000_0000_0000 | rid as u64,
            grade,
            channel: (i % 4) as u8 + 1,
            strength: 2 + (i % 2) as u8,
            residual,
        });
        rid += 1;
        i += 1;
    }
    out
}
pub fn gen_decl(rows: &[ScrollRow]) -> ExportDecl {  // 申报单派生：由快照复算申报哈希（模拟 F0754 导出方）。
    let mut h = fnv1a(0xcbf2_9ce4_8422_2325, EXPORT_VOL_ID as u64);
    h = fnv1a(h, rows.len() as u64);
    let mut i = 0usize;
    while i < rows.len() {
        let r = &rows[i];
        h = fnv1a(h, r.rid as u64);
        h = fnv1a(h, r.risk_hash);
        h = fnv1a(h, r.grade as u64);
        h = fnv1a(h, r.channel as u64);
        h = fnv1a(h, r.strength as u64);
        h = fnv1a(h, r.residual as u64);
        i += 1;
    }
    ExportDecl { vol_id: EXPORT_VOL_ID, rows: rows.len() as u32, declared_hash: h }
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    // —— 主路径：装簿 → 一致快照 → 复算哈希 → 对齐 F0754 申报 → 定版 ——
    let book = gen_book(10);
    let mut m = ScrollFinalizer::new();
    m.load_book(&book)?;
    if m.book_len() != 10 {
        return Err(XC_EMPTY_BOOK);
    }
    let drifts = m.take_snapshot(&book)?;
    if drifts != 0 || m.drift_count() != 0 {
        return Err(XC_SNAP_DRIFT);
    }
    let h = m.compute_scroll_hash()?;
    let decl = gen_decl(&book);
    if m.current_hash() != decl.declared_hash {
        return Err(XC_SCROLL_HASH_DIFF);
    }
    let rc = m.seal(&decl)?;
    if rc.rows != 10 || !rc.export_match || rc.drift_count != 0 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.seal(&decl) != Err(XC_ALREADY_SEALED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：快照单点漂移（strength 字段）——
    let mut m2 = ScrollFinalizer::new();
    let mut drift_snap = book.clone();
    drift_snap[4].strength = 3;
    m2.load_book(&book)?;
    let d2 = m2.take_snapshot(&drift_snap)?;
    if d2 != 1 {
        return Err(XC_SNAP_DRIFT);
    }
    let dm = m2.drifts();
    if dm[0].rid != 5 || dm[0].field != 4 {
        return Err(XC_SNAP_DRIFT);
    }
    m2.compute_scroll_hash()?;
    if m2.seal(&gen_decl(&drift_snap)) != Err(XC_SNAP_DRIFT) {
        return Err(XC_SNAP_DRIFT);
    }
    // —— 错误路径 2：行数不守恒 ——
    let mut m3 = ScrollFinalizer::new();
    m3.load_book(&book)?;
    let mut short = book.clone();
    short.pop();
    if m3.take_snapshot(&short) != Err(XC_LEN_MISMATCH) {
        return Err(XC_LEN_MISMATCH);
    }
    // —— 错误路径 3：申报哈希被扰 ——
    let mut m4 = ScrollFinalizer::new();
    m4.load_book(&book)?;
    m4.take_snapshot(&book)?;
    m4.compute_scroll_hash()?;
    let mut bad_decl = gen_decl(&book);
    bad_decl.declared_hash ^= 0x1;
    if m4.seal(&bad_decl) != Err(XC_SCROLL_HASH_DIFF) {
        return Err(XC_SCROLL_HASH_DIFF);
    }
    // —— 错误路径 4：未快照即定版 / 空簿 / 重号 / 残余超限 ——
    let mut m5 = ScrollFinalizer::new();
    m5.load_book(&book)?;
    if m5.compute_scroll_hash() != Err(XC_NOT_SNAPSHOTTED) {
        return Err(XC_NOT_SNAPSHOTTED);
    }
    let mut m6 = ScrollFinalizer::new();
    if m6.load_book(&[]) != Err(XC_EMPTY_BOOK) {
        return Err(XC_EMPTY_BOOK);
    }
    let mut m7 = ScrollFinalizer::new();
    let mut dup = gen_book(4);
    dup[3].rid = dup[0].rid;
    if m7.load_book(&dup) != Err(XC_DUP_RID) {
        return Err(XC_DUP_RID);
    }
    let mut m8 = ScrollFinalizer::new();
    let mut capbad = gen_book(4);
    capbad[1].residual = 3; // 高危行（i%4==0？i=1 → 中危上限 2）超限
    if m8.load_book(&capbad) != Err(XC_RESIDUAL_CAP) {
        return Err(XC_RESIDUAL_CAP);
    }
    // —— 查询 API：簿/快照行可解引且一致 ——
    match (m.book_row(3), m.snap_row(3)) {
        (Some(b), Some(s)) if b.rid == s.rid && b.risk_hash == s.risk_hash => {}
        _ => return Err(XC_SNAP_DRIFT),
    }
    if m.book_row(999).is_some() || m.snap_row(999).is_some() {
        return Err(XC_DUP_RID);
    }
    // —— 扩展 API 面：直方图互证 / 残余分布 / 导出视图 / 申报逐字段 ——
    if m.histogram_crosscheck()? != true {
        return Err(XC_SNAP_DRIFT);
    }
    let res_hist = m.residual_histogram()?;
    if res_hist[0] + res_hist[1] + res_hist[2] + res_hist[3] != 10 {
        return Err(XC_SUM_OVERFLOW);
    }
    let view = m.export_view()?;
    if view.len() != 10 {
        return Err(XC_EMPTY_SNAP);
    }
    let fields = m.decl_field_check(&decl)?;
    if !fields[0] || !fields[1] || !fields[2] {
        return Err(XC_SCROLL_HASH_DIFF);
    }
    let mut bad_decl2 = gen_decl(&book);
    bad_decl2.vol_id = 0x9999;
    let f2 = m.decl_field_check(&bad_decl2)?;
    if f2[0] {
        return Err(XC_EXPORT_ZERO);
    }
    // —— 快照未做时互证必红（m3 未快照）——
    if m3.histogram_crosscheck() != Ok(false) && m3.snap_len() == 0 {
        return Err(XC_NOT_SNAPSHOTTED);
    }
    Ok(0)
}
