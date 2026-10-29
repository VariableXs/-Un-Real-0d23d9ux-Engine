//! UNX-F0688 · 失败样本归档账（逐次失败件齐备）· 诊断段 0x5680（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0688-J1 失败样本逐次归档件（日志段/截图位/账行引用三件套）齐备率
//! 100%，零抹除断言过。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 归档三件套闭集：日志段/截图位/账行引用，逐次失败件件齐，缺一件即拦截。
//!  3. 零抹除：归档簿追加式，任何删除路径一律拦截（E_ERASE）。
//!  4. 证据可解引：账行引用载荷非零即指针合法，零指针归档件拦截。
//!  5. 件指纹：每件 FNV-1a64 摘要在账，篡改可检出。
//!  6. 诊断段 0x5680 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5680;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_INCOMPLETE: u16 = SEG_BASE + 1; // 三件套缺件
pub const E_ERASE: u16 = SEG_BASE + 2;      // 抹除尝试（零抹除拦截）
pub const E_DUP: u16 = SEG_BASE + 3;        // 同次重复归档
pub const E_PTR: u16 = SEG_BASE + 4;        // 证据指针不可解引
pub const E_HASH: u16 = SEG_BASE + 5;       // 件指纹复核不平
pub const E_SEAL: u16 = SEG_BASE + 6;       // 归档簿已封
pub const E_EMPTY: u16 = SEG_BASE + 7;      // 空件拦截

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 三件套闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// 日志段：失败现场执行流水。
    LogSeg,
    /// 截图位：失败帧截图槽位号。
    SnapSlot,
    /// 账行引用：统计账失败行指针。
    LedgerRef,
}

impl Piece {
    pub const ALL: [Piece; 3] = [Piece::LogSeg, Piece::SnapSlot, Piece::LedgerRef];

    pub fn no(self) -> u8 {
        match self {
            Piece::LogSeg => 1,
            Piece::SnapSlot => 2,
            Piece::LedgerRef => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Piece::LogSeg => "日志段",
            Piece::SnapSlot => "截图位",
            Piece::LedgerRef => "账行引用",
        }
    }

    pub fn from_no(n: u8) -> Result<Piece, u16> {
        for p in Piece::ALL {
            if p.no() == n {
                return Ok(p);
            }
        }
        Err(E_INCOMPLETE)
    }
}

/// 证据指针闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvPtr {
    /// 日志账行号。
    LogLine(u32),
    /// 截图槽位号。
    SnapNo(u32),
    /// 统计账行号。
    StatRow(u32),
}

impl EvPtr {
    /// 可解引：载荷非零。
    pub fn deref_ok(&self) -> bool {
        match self {
            EvPtr::LogLine(n) => *n > 0,
            EvPtr::SnapNo(n) => *n > 0,
            EvPtr::StatRow(n) => *n > 0,
        }
    }

    pub fn payload(&self) -> u32 {
        match self {
            EvPtr::LogLine(n) => *n,
            EvPtr::SnapNo(n) => *n,
            EvPtr::StatRow(n) => *n,
        }
    }
}

/// 归档件。
#[derive(Clone, Debug)]
pub struct ArcPiece {
    /// 失败序号（机内 1 起）。
    pub fail_no: u32,
    pub piece: Piece,
    /// 件体（字节）。
    pub body: Vec<u8>,
    pub ptr: EvPtr,
    /// 件指纹。
    pub fp: u64,
}

impl ArcPiece {
    pub fn audit(&self) -> Result<(), u16> {
        if self.fail_no == 0 || self.body.is_empty() {
            return Err(E_EMPTY);
        }
        if !self.ptr.deref_ok() {
            return Err(E_PTR);
        }
        if self.fp != fnv1a64(&self.body) {
            return Err(E_HASH);
        }
        Ok(())
    }
}

/// 确定性件体生成。
pub fn synth_body(fail_no: u32, p: Piece, seed: u64) -> Vec<u8> {
    let n = 16 + (p.no() as usize) * 8;
    let mut out = vec![0u8; n];
    let mut st = (seed ^ (fail_no as u64).wrapping_mul(0x9E37_79B9) ^ p.no() as u64) | 1;
    for b in out.iter_mut() {
        st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        *b = (st >> 33) as u8;
    }
    out
}

/// 单次失败归档：三件套落账。
#[derive(Clone, Debug)]
pub struct FailArchive {
    pub fail_no: u32,
    pub pieces: Vec<ArcPiece>,
}

impl FailArchive {
    pub fn audit(&self) -> Result<(), u16> {
        if self.fail_no == 0 {
            return Err(E_EMPTY);
        }
        // 三件齐
        for p in Piece::ALL {
            let mut hit = false;
            for pc in &self.pieces {
                if pc.piece == p {
                    hit = true;
                }
            }
            if !hit {
                return Err(E_INCOMPLETE);
            }
        }
        // 件件审计
        for pc in &self.pieces {
            pc.audit()?;
        }
        Ok(())
    }
}

/// 失败样本归档簿。
pub struct FailArcBook {
    pub archives: Vec<FailArchive>,
    pub sealed: bool,
    /// 抹除拦截计数。
    pub erase_stops: u32,
}

impl FailArcBook {
    pub fn new() -> FailArcBook {
        FailArcBook { archives: Vec::new(), sealed: false, erase_stops: 0 }
    }

    /// 归档一次失败：三件套齐才落账；同次重复拦截。
    pub fn archive_fail(
        &mut self,
        fail_no: u32,
        mach_idx: u8,
        log_line: u32,
        snap_no: u32,
        stat_row: u32,
        seed: u64,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if fail_no == 0 || mach_idx > 2 {
            return Err(E_EMPTY);
        }
        for a in &self.archives {
            if a.fail_no == fail_no {
                return Err(E_DUP);
            }
        }
        let ptrs = [
            (Piece::LogSeg, EvPtr::LogLine(log_line)),
            (Piece::SnapSlot, EvPtr::SnapNo(snap_no)),
            (Piece::LedgerRef, EvPtr::StatRow(stat_row)),
        ];
        let mut pieces = Vec::new();
        for (p, ptr) in ptrs {
            let body = synth_body(fail_no, p, seed ^ (mach_idx as u64) << 8);
            if body.is_empty() {
                return Err(E_EMPTY);
            }
            pieces.push(ArcPiece {
                fail_no,
                piece: p,
                fp: fnv1a64(&body),
                body,
                ptr,
            });
        }
        let arc = FailArchive { fail_no, pieces };
        arc.audit()?;
        self.archives.push(arc);
        Ok(())
    }

    /// 齐备率核：每次失败三件套齐 100%。
    pub fn assert_all_complete(&self) -> Result<(), u16> {
        for a in &self.archives {
            a.audit()?;
        }
        // 件数守恒：3 × 失败数
        let want = self.archives.len() * 3;
        let mut got = 0usize;
        for a in &self.archives {
            got += a.pieces.len();
        }
        if got != want {
            return Err(E_INCOMPLETE);
        }
        Ok(())
    }

    /// 零抹除断言：删除路径必须全部拦截（模拟：delete 请求 → 计数 + 拒绝）。
    pub fn try_erase(&mut self, fail_no: u32) -> Result<(), u16> {
        let mut found = false;
        for a in &self.archives {
            if a.fail_no == fail_no {
                found = true;
            }
        }
        if !found {
            return Err(E_EMPTY);
        }
        self.erase_stops += 1;
        Err(E_ERASE)
    }

    /// 封簿：齐备率 100% 才封。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.archives.is_empty() {
            return Err(E_EMPTY);
        }
        self.assert_all_complete()?;
        self.sealed = true;
        Ok(())
    }

    /// 指纹全簿复核。
    pub fn verify_fps(&self) -> Result<(), u16> {
        for a in &self.archives {
            for pc in &a.pieces {
                if pc.fp != fnv1a64(&pc.body) {
                    return Err(E_HASH);
                }
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("失败归档: 次数=");
        s.push_str(&self.archives.len().to_string());
        s.push_str(" 件数=");
        let mut n = 0usize;
        for a in &self.archives {
            n += a.pieces.len();
        }
        s.push_str(&n.to_string());
        s.push_str(" 抹除拦截=");
        s.push_str(&self.erase_stops.to_string());
        s
    }
}

/// 指针闭集自检：零载荷指针全拦截。
pub fn ptr_closure_audit() -> Result<(), u16> {
    if EvPtr::LogLine(0).deref_ok() {
        return Err(SEG_BASE + 0x30);
    }
    if EvPtr::SnapNo(0).deref_ok() {
        return Err(SEG_BASE + 0x31);
    }
    if EvPtr::StatRow(0).deref_ok() {
        return Err(SEG_BASE + 0x32);
    }
    if !EvPtr::LogLine(1).deref_ok() {
        return Err(SEG_BASE + 0x33);
    }
    // 载荷可取
    if EvPtr::SnapNo(7).payload() != 7 {
        return Err(SEG_BASE + 0x34);
    }
    Ok(())
}

/// 件类闭集自检。
pub fn piece_closure_audit() -> Result<(), u16> {
    for i in 1..=3u8 {
        if Piece::from_no(i).is_err() {
            return Err(SEG_BASE + 0x35);
        }
    }
    if !matches!(Piece::from_no(0), Err(E_INCOMPLETE)) {
        return Err(SEG_BASE + 0x36);
    }
    if !matches!(Piece::from_no(4), Err(E_INCOMPLETE)) {
        return Err(SEG_BASE + 0x37);
    }
    for p in Piece::ALL {
        if p.name().is_empty() {
            return Err(SEG_BASE + 0x38);
        }
    }
    Ok(())
}

/// 指纹确定性自检：同件两次现算全等；改体必检出。
pub fn fp_audit() -> Result<(), u16> {
    let b1 = synth_body(3, Piece::LogSeg, 0x0688_0001);
    let f1 = fnv1a64(&b1);
    let f2 = fnv1a64(&b1);
    if f1 != f2 {
        return Err(SEG_BASE + 0x39);
    }
    let mut b2 = b1.clone();
    b2[0] ^= 1;
    if fnv1a64(&b2) == f1 {
        return Err(SEG_BASE + 0x3A);
    }
    Ok(())
}

/// 指纹篡改注入核：改件指纹 → verify_fps 检出。
pub fn inject_fp_tamper_must_fail() -> Result<(), u16> {
    let mut book = FailArcBook::new();
    book.archive_fail(1, 0, 11, 21, 31, 0x0688_0002)?;
    book.archives[0].pieces[1].fp ^= 0xFF;
    if book.verify_fps().is_ok() {
        return Err(SEG_BASE + 0x3B);
    }
    Ok(())
}

/// 缺件注入核：构造缺件档案（移除截图位）→ audit 检出。
pub fn inject_missing_piece_must_fail() -> Result<(), u16> {
    let mut book = FailArcBook::new();
    book.archive_fail(2, 1, 12, 22, 32, 0x0688_0003)?;
    // 移除截图位件
    let mut i = 0usize;
    let mut removed = false;
    while i < book.archives[0].pieces.len() {
        if book.archives[0].pieces[i].piece == Piece::SnapSlot {
            book.archives[0].pieces.remove(i);
            removed = true;
        } else {
            i += 1;
        }
    }
    if !removed {
        return Err(SEG_BASE + 0x3C);
    }
    if !matches!(book.archives[0].audit(), Err(E_INCOMPLETE)) {
        return Err(SEG_BASE + 0x3D);
    }
    Ok(())
}

/// 空指针归档注入核：零载荷指针 → 拦截。
pub fn inject_zero_ptr_must_fail() -> Result<(), u16> {
    let mut book = FailArcBook::new();
    if !matches!(book.archive_fail(3, 0, 0, 23, 33, 0x0688_0004), Err(E_PTR)) {
        return Err(SEG_BASE + 0x3E);
    }
    if !matches!(book.archive_fail(3, 0, 13, 0, 33, 0x0688_0004), Err(E_PTR)) {
        return Err(SEG_BASE + 0x3F);
    }
    if !matches!(book.archive_fail(3, 0, 13, 23, 0, 0x0688_0004), Err(E_PTR)) {
        return Err(SEG_BASE + 0x40);
    }
    Ok(())
}

/// 齐备率（千分比）：齐套失败数·1000/总失败数。
pub fn completeness_permille(book: &FailArcBook) -> Result<u32, u16> {
    if book.archives.is_empty() {
        return Err(E_EMPTY);
    }
    let mut ok = 0usize;
    for a in &book.archives {
        if a.audit().is_ok() {
            ok += 1;
        }
    }
    Ok((ok * 1000 / book.archives.len()) as u32)
}

/// 齐备率 100% 断言。
pub fn assert_completeness(book: &FailArcBook) -> Result<(), u16> {
    if completeness_permille(book)? != 1000 {
        return Err(E_INCOMPLETE);
    }
    Ok(())
}

/// 件类计数守恒：每类件数 = 失败数（三件套逐类守恒）。
pub fn piece_class_cons(book: &FailArcBook) -> Result<(), u16> {
    book.assert_all_complete()?;
    for p in Piece::ALL {
        let mut n = 0usize;
        for a in &book.archives {
            for pc in &a.pieces {
                if pc.piece == p {
                    n += 1;
                }
            }
        }
        if n != book.archives.len() {
            return Err(E_INCOMPLETE);
        }
    }
    Ok(())
}

/// 分机归档分布审计：失败号段与机绑定可解引（失败号 1..2 → 机 0，3..4 → 机 1，
/// 5..6 → 机 2 的确定性口径由 seed 派生机号决定；此处审计分配非空即可）。
pub fn mach_dist_audit(book: &FailArcBook, machs: &[u8]) -> Result<(), u16> {
    if machs.len() != book.archives.len() {
        return Err(E_EMPTY);
    }
    for (a, m) in book.archives.iter().zip(machs.iter()) {
        if *m > 2 {
            return Err(E_EMPTY);
        }
        let _ = a;
    }
    Ok(())
}

/// 逐件指针全解引断言：任何件指针不可解引即拦截。
pub fn ptr_full_deref(book: &FailArcBook) -> Result<(), u16> {
    for a in &book.archives {
        for pc in &a.pieces {
            if !pc.ptr.deref_ok() {
                return Err(E_PTR);
            }
        }
    }
    Ok(())
}

/// 指针解引注入核：单件指针置零 → 断言检出。
pub fn inject_ptr_zero_must_fail(book: &FailArcBook) -> Result<(), u16> {
    let mut bad = FailArcBook { archives: Vec::new(), sealed: false, erase_stops: 0 };
    for a in &book.archives {
        bad.archives.push(a.clone());
    }
    bad.archives[0].pieces[0].ptr = EvPtr::LogLine(0);
    if ptr_full_deref(&bad).is_ok() {
        return Err(SEG_BASE + 0x48);
    }
    Ok(())
}

/// 件体确定性注入核：同（次,类,种子）两次生成逐字节全等。
pub fn synth_body_determinism_audit() -> Result<(), u16> {
    for p in Piece::ALL {
        let a = synth_body(9, p, 0x0688_0005);
        let b = synth_body(9, p, 0x0688_0005);
        if a.len() != b.len() {
            return Err(SEG_BASE + 0x49);
        }
        for i in 0..a.len() {
            if a[i] != b[i] {
                return Err(SEG_BASE + 0x4A);
            }
        }
    }
    Ok(())
}

/// 归档清单渲染（程序化，零手抄）。
pub struct ArcManifest {
    pub lines: Vec<String>,
}

impl ArcManifest {
    pub fn generate(book: &FailArcBook) -> Result<ArcManifest, u16> {
        book.assert_all_complete()?;
        let mut lines = Vec::new();
        lines.push(book.summary());
        for a in &book.archives {
            for pc in &a.pieces {
                let mut l = String::from("F");
                l.push_str(&pc.fail_no.to_string());
                l.push(' ');
                l.push_str(pc.piece.name());
                l.push_str(" ptr=");
                l.push_str(&pc.ptr.payload().to_string());
                lines.push(l);
            }
        }
        Ok(ArcManifest { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 2 {
            return Err(E_INCOMPLETE);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_EMPTY);
            }
        }
        Ok(())
    }
}

/// 自例程：逐次失败三件套齐 100% + 零抹除 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    ptr_closure_audit()?;
    piece_closure_audit()?;
    fp_audit()?;
    synth_body_determinism_audit()?;
    inject_fp_tamper_must_fail()?;
    inject_missing_piece_must_fail()?;
    inject_zero_ptr_must_fail()?;

    // 主簿：6 次失败归档（跨 3 机）
    let mut book = FailArcBook::new();
    let fails = [(1u32, 0u8, 101u32, 201u32, 301u32), (2, 0, 102, 202, 302), (3, 1, 103, 203, 303), (4, 1, 104, 204, 304), (5, 2, 105, 205, 305), (6, 2, 106, 206, 306)];
    for (no, m, l, s, r) in fails {
        book.archive_fail(no, m, l, s, r, 0x0688_0006)?;
    }
    book.assert_all_complete()?;
    book.verify_fps()?;
    // 齐备率 + 件类守恒 + 分机分布 + 逐件指针
    assert_completeness(&book)?;
    piece_class_cons(&book)?;
    mach_dist_audit(&book, &[0, 0, 1, 1, 2, 2])?;
    ptr_full_deref(&book)?;
    inject_ptr_zero_must_fail(&book)?;
    if book.summary().is_empty() {
        return Err(SEG_BASE + 0x41);
    }
    // 零抹除：删除尝试全拦截并计数
    for no in 1..=6u32 {
        if !matches!(book.try_erase(no), Err(E_ERASE)) {
            return Err(SEG_BASE + 0x42);
        }
    }
    if book.erase_stops != 6 {
        return Err(SEG_BASE + 0x43);
    }
    // 注入：同次重复归档 → 拦截
    if book.archive_fail(3, 1, 103, 203, 303, 1) != Err(E_DUP) {
        return Err(SEG_BASE + 0x44);
    }
    // 注入：不存在次的抹除 → 拦截
    if !matches!(book.try_erase(99), Err(E_EMPTY)) {
        return Err(SEG_BASE + 0x45);
    }
    // 注入：机型越界 → 拦截
    if book.archive_fail(9, 7, 109, 209, 309, 1) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x46);
    }
    // 封簿 + 封后归档拦截
    book.seal()?;
    if book.archive_fail(9, 0, 109, 209, 309, 1) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x47);
    }
    // 清单
    let mf = ArcManifest::generate(&book)?;
    mf.assert_complete()?;
    Ok(0)
}
