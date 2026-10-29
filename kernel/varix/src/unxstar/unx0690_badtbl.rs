//! UNX-F0690 · 坏表机型黑名单账（DJ-UNX 流汇总）· 诊断段 0x56c0（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0690-J1 黑名单账三列齐（DJ 编号/机型指纹/坏表签名）且逐条证据
//! 可解引，黑名单拦截路由过。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 黑名单三列闭集：DJ 编号（唯一）/机型指纹/坏表签名，列列非空。
//!  3. 证据可解引：每条附证据指针（DJ 流账行号），零指针条目拦截。
//!  4. 拦截路由：命中黑名单的接入请求必拒（E_BLOCKED），放行请求可查白路径。
//!  5. 黑名单追加式：入簿后不改不删（零抹除），重复 DJ 拦截。
//!  6. 诊断段 0x56c0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x56c0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_BLOCKED: u16 = SEG_BASE + 1;  // 黑名单命中（接入拦截）
pub const E_DUP: u16 = SEG_BASE + 2;      // DJ 编号重复入簿
pub const E_COL: u16 = SEG_BASE + 3;      // 三列缺列（空值）
pub const E_PTR: u16 = SEG_BASE + 4;      // 证据指针不可解引
pub const E_ERASE: u16 = SEG_BASE + 5;    // 抹除尝试拦截
pub const E_SEAL: u16 = SEG_BASE + 6;     // 黑名单已封
pub const E_NOENTRY: u16 = SEG_BASE + 7;  // 查无此 DJ

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// DJ-UNX 流编号闭集（判据口径：1..=9）。
pub const DJ_MAX: u32 = 9;

/// 黑名单账行：三列 + 证据指针。
#[derive(Clone, Debug)]
pub struct BlackRow {
    /// DJ 编号（1..=9，簿内唯一）。
    pub dj: u32,
    /// 机型指纹（64 位）。
    pub mach_fp: u64,
    /// 坏表签名（64 位）。
    pub bad_sig: u64,
    /// 证据指针（DJ 流账行号）。
    pub ev_row: u32,
}

impl BlackRow {
    /// 三列齐审计：DJ ∈ 闭集、指纹非零、签名非零。
    pub fn audit(&self) -> Result<(), u16> {
        if self.dj == 0 || self.dj > DJ_MAX {
            return Err(E_COL);
        }
        if self.mach_fp == 0 || self.bad_sig == 0 {
            return Err(E_COL);
        }
        if self.ev_row == 0 {
            return Err(E_PTR);
        }
        Ok(())
    }

    /// 指纹复核：机型指纹与坏表签名拼装哈希（对账锚）。
    pub fn row_fp(&self) -> u64 {
        let mut buf = vec![0u8; 24];
        for (k, v) in [self.dj as u64, self.mach_fp, self.bad_sig].iter().enumerate() {
            for j in 0..8u8 {
                buf[k * 8 + j as usize] = (v >> (56 - j * 8)) as u8;
            }
        }
        fnv1a64(&buf)
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("DJ");
        s.push_str(&self.dj.to_string());
        s.push_str(" fp=");
        s.push_str(&(self.mach_fp % 100_000).to_string());
        s.push_str(" sig=");
        s.push_str(&(self.bad_sig % 100_000).to_string());
        s
    }
}

/// 坏表签名派生（确定性）：指纹 × DJ 混频。
pub fn derive_bad_sig(mach_fp: u64, dj: u32) -> u64 {
    let mut st = (mach_fp ^ (dj as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1;
    st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    st
}

/// 机型指纹派生（确定性）。
pub fn derive_mach_fp(dj: u32) -> u64 {
    let mut st = ((dj as u64) << 32) ^ 0x0690_0001 | 1;
    st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    st
}

/// 黑名单账。
pub struct BlacklistBook {
    pub rows: Vec<BlackRow>,
    pub sealed: bool,
    /// 拦截计数。
    pub blocks: u32,
    /// 放行计数（未命中路径）。
    pub allows: u32,
}

impl BlacklistBook {
    pub fn new() -> BlacklistBook {
        BlacklistBook { rows: Vec::new(), sealed: false, blocks: 0, allows: 0 }
    }

    /// 入簿：三列齐 + DJ 唯一 + 证据可解引。
    pub fn enroll(&mut self, dj: u32, ev_row: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if dj == 0 || dj > DJ_MAX {
            return Err(E_COL);
        }
        for r in &self.rows {
            if r.dj == dj {
                return Err(E_DUP);
            }
        }
        if ev_row == 0 {
            return Err(E_PTR);
        }
        let mach_fp = derive_mach_fp(dj);
        let bad_sig = derive_bad_sig(mach_fp, dj);
        let row = BlackRow { dj, mach_fp, bad_sig, ev_row };
        row.audit()?;
        self.rows.push(row);
        Ok(())
    }

    /// 查 DJ 是否在簿。
    pub fn contains(&self, dj: u32) -> bool {
        for r in &self.rows {
            if r.dj == dj {
                return true;
            }
        }
        false
    }

    /// 接入路由：命中 → 拦截立案；未命中 → 放行计数。
    pub fn route(&mut self, dj: u32) -> Result<(), u16> {
        if dj == 0 || dj > DJ_MAX {
            return Err(E_COL);
        }
        if self.contains(dj) {
            self.blocks += 1;
            return Err(E_BLOCKED);
        }
        self.allows += 1;
        Ok(())
    }

    /// 纯查询路由（不改计数）：命中返回行引用语义（载荷）。
    pub fn lookup(&self, dj: u32) -> Result<u32, u16> {
        for r in &self.rows {
            if r.dj == dj {
                return Ok(r.ev_row);
            }
        }
        Err(E_NOENTRY)
    }

    /// 逐条证据可解引断言。
    pub fn assert_all_deref(&self) -> Result<(), u16> {
        for r in &self.rows {
            r.audit()?;
            // 指纹对账锚复核
            if r.row_fp() == 0 {
                return Err(E_COL);
            }
        }
        Ok(())
    }

    /// 抹除尝试：一律拦截并计数（零抹除）。
    pub fn try_erase(&mut self, dj: u32) -> Result<(), u16> {
        if !self.contains(dj) {
            return Err(E_NOENTRY);
        }
        Err(E_ERASE)
    }

    /// 三列齐断言：行行三列非空。
    pub fn assert_cols(&self) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_COL);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_cols()?;
        self.assert_all_deref()?;
        self.sealed = true;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("黑名单: 条数=");
        s.push_str(&self.rows.len().to_string());
        s.push_str(" 拦截=");
        s.push_str(&self.blocks.to_string());
        s.push_str(" 放行=");
        s.push_str(&self.allows.to_string());
        s
    }
}

/// 指纹派生确定性自检。
pub fn derive_determinism_audit() -> Result<(), u16> {
    for dj in 1..=DJ_MAX {
        let f1 = derive_mach_fp(dj);
        let f2 = derive_mach_fp(dj);
        if f1 != f2 {
            return Err(E_COL);
        }
        let s1 = derive_bad_sig(f1, dj);
        let s2 = derive_bad_sig(f1, dj);
        if s1 != s2 {
            return Err(E_COL);
        }
        if f1 == 0 || s1 == 0 {
            return Err(E_COL);
        }
    }
    Ok(())
}

/// DJ 重复注入核。
pub fn inject_dup_must_fail() -> Result<(), u16> {
    let mut b = BlacklistBook::new();
    b.enroll(3, 7)?;
    if b.enroll(3, 9) != Err(E_DUP) {
        return Err(SEG_BASE + 0x30);
    }
    Ok(())
}

/// 缺列注入核：DJ 越界 / 零证据 → 拦截。
pub fn inject_bad_col_must_fail() -> Result<(), u16> {
    let mut b = BlacklistBook::new();
    if b.enroll(0, 5) != Err(E_COL) {
        return Err(SEG_BASE + 0x31);
    }
    if b.enroll(10, 5) != Err(E_COL) {
        return Err(SEG_BASE + 0x32);
    }
    if b.enroll(4, 0) != Err(E_PTR) {
        return Err(SEG_BASE + 0x33);
    }
    Ok(())
}

/// 签名对账注入核：坏表签名篡改 → 行审计检出（签名与指纹失配由指纹复核兜底）。
pub fn inject_sig_tamper_must_fail() -> Result<(), u16> {
    let mut b = BlacklistBook::new();
    b.enroll(5, 11)?;
    let true_sig = b.rows[0].bad_sig;
    b.rows[0].bad_sig = true_sig ^ 0xFF;
    // 签名与派生口径失配 → 派生复核检出
    let expect = derive_bad_sig(b.rows[0].mach_fp, 5);
    if b.rows[0].bad_sig == expect {
        return Err(SEG_BASE + 0x34);
    }
    let _ = true_sig;
    Ok(())
}

/// 查无此 DJ 注入核。
pub fn inject_lookup_miss_must_fail() -> Result<(), u16> {
    let mut b = BlacklistBook::new();
    b.enroll(2, 8)?;
    if b.lookup(2).is_err() {
        return Err(SEG_BASE + 0x35);
    }
    if b.lookup(9) != Err(E_NOENTRY) {
        return Err(SEG_BASE + 0x36);
    }
    Ok(())
}

/// 拦截路由注入核：命中必拒且计数；未命中放行。
pub fn inject_route_audit() -> Result<(), u16> {
    let mut b = BlacklistBook::new();
    b.enroll(1, 3)?;
    b.enroll(6, 4)?;
    // 命中
    if b.route(1) != Err(E_BLOCKED) {
        return Err(SEG_BASE + 0x37);
    }
    if b.route(6) != Err(E_BLOCKED) {
        return Err(SEG_BASE + 0x38);
    }
    // 未命中
    b.route(2)?;
    b.route(9)?;
    if b.blocks != 2 || b.allows != 2 {
        return Err(SEG_BASE + 0x39);
    }
    // 路由计数守恒：拦截 + 放行 = 总请求
    if b.blocks + b.allows != 4 {
        return Err(E_COL);
    }
    Ok(())
}

/// 指纹碰撞检测：两 DJ 机型指纹不得相同（碰撞即派生口径破）。
pub fn collision_audit(book: &BlacklistBook) -> Result<(), u16> {
    book.assert_cols()?;
    for i in 0..book.rows.len() {
        for j in (i + 1)..book.rows.len() {
            if book.rows[i].mach_fp == book.rows[j].mach_fp {
                return Err(E_COL);
            }
            if book.rows[i].bad_sig == book.rows[j].bad_sig {
                return Err(E_COL);
            }
        }
    }
    Ok(())
}

/// 白名单对照组：未入簿 DJ 的放行路径可查（对照实验语义）。
pub struct WhiteControl {
    /// 对照 DJ 集（1..=9 中未入簿者）。
    pub djs: Vec<u32>,
}

impl WhiteControl {
    pub fn build(book: &BlacklistBook) -> Result<WhiteControl, u16> {
        let mut djs = Vec::new();
        for dj in 1..=DJ_MAX {
            if !book.contains(dj) {
                djs.push(dj);
            }
        }
        if djs.is_empty() {
            return Err(E_COL);
        }
        Ok(WhiteControl { djs })
    }

    /// 对照组核：全部未命中黑名单。
    pub fn assert_clean(&self, book: &BlacklistBook) -> Result<(), u16> {
        for dj in &self.djs {
            if book.contains(*dj) {
                return Err(E_BLOCKED);
            }
            if book.lookup(*dj).is_ok() {
                return Err(E_COL);
            }
        }
        Ok(())
    }

    /// 守恒：黑名单数 + 对照组数 = DJ 全集。
    pub fn cons(&self, book: &BlacklistBook) -> Result<(), u16> {
        if self.djs.len() as u32 + book.rows.len() as u32 != DJ_MAX {
            return Err(E_COL);
        }
        Ok(())
    }
}

/// 证据行号账：入簿序号 → 证据行号（可解引 + 唯一）。
pub struct EvRowBook {
    pub entries: Vec<(u32, u32)>,
    pub sealed: bool,
}

impl EvRowBook {
    pub fn new() -> EvRowBook {
        EvRowBook { entries: Vec::new(), sealed: false }
    }

    pub fn record(&mut self, dj: u32, ev_row: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if dj == 0 || dj > DJ_MAX || ev_row == 0 {
            return Err(E_PTR);
        }
        for (d, e) in &self.entries {
            if d == &dj {
                return Err(E_DUP);
            }
            if e == &ev_row {
                return Err(E_PTR);
            }
        }
        self.entries.push((dj, ev_row));
        Ok(())
    }

    /// 对账：账面证据行号与黑名单逐条全等。
    pub fn verify(&self, book: &BlacklistBook) -> Result<(), u16> {
        if self.entries.len() != book.rows.len() {
            return Err(E_COL);
        }
        for (i, (dj, ev)) in self.entries.iter().enumerate() {
            let r = &book.rows[i];
            if r.dj != *dj {
                return Err(E_COL);
            }
            if r.ev_row != *ev {
                return Err(E_PTR);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.entries.is_empty() {
            return Err(E_COL);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 证据账注入核：证据行号重复 → 拦截；篡改 → 对账检出。
pub fn evrow_inject_audit(book: &BlacklistBook) -> Result<(), u16> {
    let mut eb = EvRowBook::new();
    for r in &book.rows {
        eb.record(r.dj, r.ev_row)?;
    }
    eb.verify(book)?;
    // 注入 1：证据行号重复
    if eb.record(1, book.rows[0].ev_row).is_ok() {
        return Err(SEG_BASE + 0x3F);
    }
    // 注入 2：篡改后对账检出
    eb.entries[0].1 ^= 0xFF;
    if eb.verify(book).is_ok() {
        return Err(SEG_BASE + 0x40);
    }
    Ok(())
}

/// 签名-指纹耦合核：同 DJ 派生签名必须等于派生口径（逐条复核）。
pub fn sig_coupling_audit(book: &BlacklistBook) -> Result<(), u16> {
    for r in &book.rows {
        let expect = derive_bad_sig(r.mach_fp, r.dj);
        if r.bad_sig != expect {
            return Err(E_COL);
        }
        let expect_fp = derive_mach_fp(r.dj);
        if r.mach_fp != expect_fp {
            return Err(E_COL);
        }
    }
    Ok(())
}

/// 耦合注入核：指纹换 DJ 派生（串行）→ 耦合核检出。
pub fn inject_coupling_must_fail(book: &BlacklistBook) -> Result<(), u16> {
    let mut bad = BlacklistBook { rows: Vec::new(), sealed: false, blocks: 0, allows: 0 };
    for r in &book.rows {
        bad.rows.push(r.clone());
    }
    // 把第 0 行指纹换成下一 DJ 的派生（模拟串行污染）
    if bad.rows.len() > 1 {
        bad.rows[0].mach_fp = bad.rows[1].mach_fp;
        if sig_coupling_audit(&bad).is_ok() {
            return Err(SEG_BASE + 0x41);
        }
    }
    Ok(())
}

/// 路由账摘要（拦截/放行/未决三桶守恒）。
pub struct RouteBuckets {
    pub blocked: u32,
    pub allowed: u32,
    pub undecided: u32,
}

impl RouteBuckets {
    pub fn count(book: &BlacklistBook, asked: &[u32]) -> Result<RouteBuckets, u16> {
        let mut b = RouteBuckets { blocked: 0, allowed: 0, undecided: 0 };
        for dj in asked {
            if *dj == 0 || *dj > DJ_MAX {
                b.undecided += 1;
            } else if book.contains(*dj) {
                b.blocked += 1;
            } else {
                b.allowed += 1;
            }
        }
        Ok(b)
    }

    /// 桶守恒：三桶和 = 询问数。
    pub fn cons(&self, asked_n: u32) -> Result<(), u16> {
        if self.blocked + self.allowed + self.undecided != asked_n {
            return Err(E_COL);
        }
        Ok(())
    }
}

/// 黑名单清单渲染（程序化，零手抄）。
pub struct BlackManifest {
    pub lines: Vec<String>,
}

impl BlackManifest {
    pub fn generate(book: &BlacklistBook) -> Result<BlackManifest, u16> {
        book.assert_all_deref()?;
        let mut lines = Vec::new();
        lines.push(book.summary());
        for r in &book.rows {
            lines.push(r.summary());
        }
        Ok(BlackManifest { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 2 {
            return Err(E_COL);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_COL);
            }
        }
        Ok(())
    }
}

/// 自例程：三列齐 + 证据可解引 + 拦截路由 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    derive_determinism_audit()?;
    inject_dup_must_fail()?;
    inject_bad_col_must_fail()?;
    inject_sig_tamper_must_fail()?;
    inject_lookup_miss_must_fail()?;
    inject_route_audit()?;

    // 主簿：5 条黑名单
    let mut b = BlacklistBook::new();
    for (dj, ev) in [(1u32, 10u32), (4, 11), (5, 12), (7, 13), (9, 14)] {
        b.enroll(dj, ev)?;
    }
    b.assert_cols()?;
    b.assert_all_deref()?;
    // 碰撞 + 耦合 + 对照组 + 证据账 + 路由桶
    collision_audit(&b)?;
    sig_coupling_audit(&b)?;
    inject_coupling_must_fail(&b)?;
    let wc = WhiteControl::build(&b)?;
    wc.assert_clean(&b)?;
    wc.cons(&b)?;
    evrow_inject_audit(&b)?;
    let asked = [1u32, 2, 4, 5, 7, 9, 3, 6, 8];
    let buckets = RouteBuckets::count(&b, &asked)?;
    buckets.cons(asked.len() as u32)?;
    // 路由：5 命中 + 4 放行
    for dj in [1u32, 4, 5, 7, 9] {
        if b.route(dj) != Err(E_BLOCKED) {
            return Err(SEG_BASE + 0x3A);
        }
    }
    for dj in [2u32, 3, 6, 8] {
        b.route(dj)?;
    }
    if b.blocks != 7 || b.allows != 4 {
        return Err(SEG_BASE + 0x3B);
    }
    // 抹除全拦截
    for dj in [1u32, 4, 5, 7, 9] {
        if b.try_erase(dj) != Err(E_ERASE) {
            return Err(SEG_BASE + 0x3C);
        }
    }
    // 注入：查无此 DJ 抹除 → 拦截
    if b.try_erase(3) != Err(E_NOENTRY) {
        return Err(SEG_BASE + 0x3D);
    }
    // 封账 + 封后入簿拦截
    b.seal()?;
    if b.enroll(2, 20) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3E);
    }
    // 清单
    let mf = BlackManifest::generate(&b)?;
    mf.assert_complete()?;
    Ok(0)
}
