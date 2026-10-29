//! UNX-F0708 · 断言-判据映射总表（全域 J1 双向对账）· 诊断段 0x5880（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0708] 断言-判据映射总表（全域 J1 双向对账） <SEG 0x5880>
//!   UNX-F0708-J1 全域映射总表落簿（≥300 行：断言名/判据号/分项/批号四列）
//!   双向对账零错位（正反两向 100%）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 四列闭集：映射行恰四列（断言名哈希/J 判据号/分项号/批号），
//!      列外无字段；列完整性由构造入口保证，审计逐行复列。
//!   2. 落簿下限：总表 ≥300 行（TABLE_TARGET=300），不足拒封。
//!   3. 双向对账：正向（断言→判据）逐行解引核对；反向（判据→断言组）
//!      逐判据聚合核对；两向计数守恒（Σ判据组 == 总行数），错位必检出。
//!   4. 判据号域：J 号编码 (域位<<8)|序号，域位闭集 0..8；分项号 1..=16；
//!      批号 1..=64——三域越界一律拒收。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5880~0x589F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥580（判据原文「纯功能代码 ≥580 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5880 独占 0x5880~0x589F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5880;

/// 0x5881 总表已封板仍尝试落簿
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5882 总表行数不足：补全后 <300
const E_TARGET_SHORT: u16 = SEG_BASE + 0x2;
/// 0x58A0 保留（本段边界注记）
const _SEG_END_MARK: u16 = SEG_BASE + 0x20;
/// 0x5883 判据号越域：域位越闭集
const E_JDOM_RANGE: u16 = SEG_BASE + 0x3;
/// 0x5884 分项号越域（合法 1..=16）
const E_SUBITEM_RANGE: u16 = SEG_BASE + 0x4;
/// 0x58A5 批号越域（合法 1..=64）
const E_BATCH_RANGE: u16 = SEG_BASE + 0x5;
/// 0x5886 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x6;
/// 0x5887 映射重号：同断言同判据同分项二次落簿
const E_MAP_DUP: u16 = SEG_BASE + 0x7;
/// 0x5888 正向对账错位：断言行解引判据失败
const E_FWD_MISMATCH: u16 = SEG_BASE + 0x8;
/// 0x5889 反向对账错位：判据组聚合与正向不符
const E_REV_MISMATCH: u16 = SEG_BASE + 0x9;
/// 0x588A 守恒破：Σ判据组计数 ≠ 总行数
const E_CONSERVATION: u16 = SEG_BASE + 0xA;
/// 0x588B 判据组空：某判据零映射行却入组表
const E_JGROUP_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x588C 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x588D 总表溢出：行数超定容
const E_TABLE_FULL: u16 = SEG_BASE + 0xD;
/// 0x588E 行号悬空：解引越界行
const E_REF_DANGLE: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5880~0x589F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "总表已封板：落簿冻结，扩表请开新表",
        E_TARGET_SHORT => "行数不足：补全后 <300，请继续落簿",
        E_JDOM_RANGE => "判据号越域：域位闭集合法 0..8",
        E_SUBITEM_RANGE => "分项号越域：合法 1..=16",
        E_BATCH_RANGE => "批号越域：合法 1..=64",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_MAP_DUP => "映射重号：同断言同判据同分项已落簿",
        E_FWD_MISMATCH => "正向对账错位：断言行解引判据失败",
        E_REV_MISMATCH => "反向对账错位：判据组聚合与正向不符",
        E_CONSERVATION => "守恒破：Σ判据组计数 ≠ 总行数",
        E_JGROUP_EMPTY => "判据组空：零映射行判据不得入组表",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_TABLE_FULL => "总表溢出：行数超定容上限",
        E_REF_DANGLE => "行号悬空：解引越界",
        _ => "未知诊断码（0x5880 段外或保留位）",
    }
}

// ── 常量（来源：判据 ≥300 行与全域结构） ─────────────────────────────
/// 总表行数目标（判据原文：≥300 行）。
pub const TABLE_TARGET: usize = 300;
/// 判据域位闭集容量。
pub const JDOMAIN_COUNT: u8 = 8;
/// 分项号上限。
pub const SUBITEM_MAX: u8 = 16;
/// 批号上限。
pub const BATCH_MAX: u16 = 64;
/// 判据组定容（域位×组内判据号）。
pub const MAX_JGROUPS: usize = 128;
/// 总表定容。
pub const MAX_ROWS: usize = 1024;
/// 断言名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(TABLE_TARGET == 300);
const _: () = assert!(JDOMAIN_COUNT == 8);
const _: () = assert!(SUBITEM_MAX == 16);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── J 判据号（编码域位|序号） ────────────────────────────────────────
/// J 判据号：高 8 位域位（0..8），低 8 位序号（1..=255）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JCode {
    /// 域位（0..8）。
    pub domain: u8,
    /// 序号（1..=255）。
    pub seq: u8,
}

impl JCode {
    /// 编码为 u16。
    pub const fn encode(self) -> u16 {
        ((self.domain as u16) << 8) | (self.seq as u16)
    }

    /// 从 u16 解码（域位越闭集即 Err）。
    pub fn decode(v: u16) -> Result<JCode, u16> {
        let d = (v >> 8) as u8;
        let s = (v & 0xFF) as u8;
        if d >= JDOMAIN_COUNT {
            return Err(E_JDOM_RANGE);
        }
        if s == 0 {
            return Err(E_JDOM_RANGE);
        }
        Ok(JCode { domain: d, seq: s })
    }
}

// ── 映射行（四列闭集） ───────────────────────────────────────────────
/// 映射总表一行：恰四列（断言名哈希/判据号/分项号/批号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapRow {
    /// 列一：断言名 FNV 哈希。
    pub assert_fp: u64,
    /// 列二：J 判据号（编码值）。
    pub jcode: u16,
    /// 列三：分项号 1..=16。
    pub subitem: u8,
    /// 列四：批号 1..=64。
    pub batch: u16,
}

impl MapRow {
    /// 行合法性：三域逐列核对。
    pub fn validate(&self) -> Result<(), u16> {
        JCode::decode(self.jcode)?;
        if self.subitem < 1 || self.subitem > SUBITEM_MAX {
            return Err(E_SUBITEM_RANGE);
        }
        if self.batch < 1 || self.batch > BATCH_MAX {
            return Err(E_BATCH_RANGE);
        }
        if self.assert_fp == 0 {
            return Err(E_NAME_BAD);
        }
        Ok(())
    }
}

// ── 判据组（反向对账载体） ───────────────────────────────────────────
/// 判据组：判据号 + 组内行索引表（反向索引）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JGroup {
    /// 判据号（编码值）。
    pub jcode: u16,
    /// 组内行索引（升序，指向总表行号）。
    pub rows: Vec<usize>,
}

// ── 映射总表 ─────────────────────────────────────────────────────────
/// 断言-判据映射总表：四列行账 + 判据组反向索引 + 双向对账引擎。
pub struct MappingTable {
    /// 行账（登记序）。
    rows: Vec<MapRow>,
    /// 判据组表（反向索引，判据号升序维护）。
    groups: Vec<JGroup>,
    /// 封板位。
    sealed: bool,
    /// 正向对账通过位。
    fwd_ok: bool,
    /// 反向对账通过位。
    rev_ok: bool,
}

impl MappingTable {
    /// 新总表。
    pub fn new() -> Self {
        MappingTable {
            rows: Vec::new(),
            groups: Vec::new(),
            sealed: false,
            fwd_ok: false,
            rev_ok: false,
        }
    }

    /// 落簿一行：三域核对 + 重号检出 + 组索引增量维护。
    pub fn put(
        &mut self,
        assert_name: &str,
        jcode: JCode,
        subitem: u8,
        batch: u16,
    ) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.rows.len() >= MAX_ROWS {
            return Err(E_TABLE_FULL);
        }
        if assert_name.is_empty() || assert_name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        let row = MapRow {
            assert_fp: fnv_str(assert_name),
            jcode: jcode.encode(),
            subitem,
            batch,
        };
        row.validate()?;
        // 重号检出：同断言同判据同分项。
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.assert_fp == row.assert_fp
                && r.jcode == row.jcode
                && r.subitem == row.subitem
            {
                return Err(E_MAP_DUP);
            }
            i += 1;
        }
        self.rows.push(row);
        let idx = self.rows.len() - 1;
        // 组索引增量维护：找组或建组。
        let mut g = 0usize;
        let mut found = false;
        while g < self.groups.len() {
            if self.groups[g].jcode == row.jcode {
                found = true;
                break;
            }
            g += 1;
        }
        if found {
            self.groups[g].rows.push(idx);
        } else {
            let mut gr = JGroup {
                jcode: row.jcode,
                rows: Vec::new(),
            };
            gr.rows.push(idx);
            self.groups.push(gr);
        }
        Ok(idx)
    }

    /// 总行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 判据组数。
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// 行解引。
    pub fn row(&self, idx: usize) -> Result<MapRow, u16> {
        if idx >= self.rows.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.rows[idx])
    }

    /// 正向对账（断言→判据 100%）：逐行复列核对 + 域内可解引。
    pub fn audit_forward(&mut self) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            // 列复检。
            r.validate()?;
            // 正向解引：判据组中必含本行。
            let mut found = false;
            let mut g = 0usize;
            while g < self.groups.len() {
                if self.groups[g].jcode == r.jcode {
                    let mut k = 0usize;
                    while k < self.groups[g].rows.len() {
                        if self.groups[g].rows[k] == i {
                            found = true;
                            break;
                        }
                        k += 1;
                    }
                    break;
                }
                g += 1;
            }
            if !found {
                self.fwd_ok = false;
                return Err(E_FWD_MISMATCH);
            }
            i += 1;
        }
        self.fwd_ok = true;
        Ok(self.rows.len())
    }

    /// 反向对账（判据→断言组 100%）：逐组行数与正向计数聚合核对。
    pub fn audit_reverse(&mut self) -> Result<usize, u16> {
        let mut sum = 0usize;
        let mut g = 0usize;
        while g < self.groups.len() {
            if self.groups[g].rows.is_empty() {
                self.rev_ok = false;
                return Err(E_JGROUP_EMPTY);
            }
            sum += self.groups[g].rows.len();
            // 组内行号逐条回解：行号必在总表域内且判据号回指本组。
            let mut k = 0usize;
            while k < self.groups[g].rows.len() {
                let idx = self.groups[g].rows[k];
                if idx >= self.rows.len() {
                    self.rev_ok = false;
                    return Err(E_REV_MISMATCH);
                }
                if self.rows[idx].jcode != self.groups[g].jcode {
                    self.rev_ok = false;
                    return Err(E_REV_MISMATCH);
                }
                k += 1;
            }
            g += 1;
        }
        if sum != self.rows.len() {
            self.rev_ok = false;
            return Err(E_CONSERVATION);
        }
        self.rev_ok = true;
        Ok(sum)
    }

    /// 判据组解引。
    pub fn group(&self, idx: usize) -> Result<&JGroup, u16> {
        if idx >= self.groups.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(&self.groups[idx])
    }

    /// 封板：双向对账全过 + 行数达标才落锁。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.audit_forward()?;
        self.audit_reverse()?;
        if self.rows.len() < TABLE_TARGET {
            return Err(E_TARGET_SHORT);
        }
        self.sealed = true;
        Ok(self.rows.len())
    }

    /// 封板位。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 对账位只读。
    pub const fn reconcile_flags(&self) -> (bool, bool) {
        (self.fwd_ok, self.rev_ok)
    }

    /// 表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((r.assert_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, (r.jcode & 0xFF) as u8);
            h = fnv_mix(h, ((r.jcode >> 8) & 0xFF) as u8);
            h = fnv_mix(h, r.subitem | 0x10);
            h = fnv_mix(h, (r.batch & 0xFF) as u8);
            i += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.rows.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("映射总表: 行=");
        out.push_str(&itoa(self.rows.len() as u64));
        out.push_str("/300 组=");
        out.push_str(&itoa(self.groups.len() as u64));
        out.push_str(" 正向=");
        out.push_str(&itoa(self.fwd_ok as u64));
        out.push_str(" 反向=");
        out.push_str(&itoa(self.rev_ok as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

fn assert_name(i: usize) -> String {
    let mut s = String::new();
    s.push_str("unx_a1_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：落簿 300 行（4 域 × 若干判据 × 3 分项）→ 封板全过 ——
    let mut tab = MappingTable::new();
    let mut n = 0usize;
    let mut d = 0u8;
    while d < 4 {
        let mut sq = 1u8;
        while sq <= 25 {
            let mut sub = 1u8;
            while sub <= 3 {
                let jc = JCode { domain: d, seq: sq };
                tab.put(&assert_name(n), jc, sub, ((n % 64) as u16) + 1)?;
                n += 1;
                sub += 1;
            }
            sq += 1;
        }
        d += 1;
    }
    // 4 域 × 25 判据 × 3 分项 = 300。
    if tab.row_count() != TABLE_TARGET {
        return Err(E_TARGET_SHORT);
    }
    tab.audit_forward()?;
    tab.audit_reverse()?;
    let total = tab.seal()?;
    if total != 300 {
        return Err(E_TARGET_SHORT);
    }
    if !tab.is_sealed() {
        return Err(E_SEALED);
    }
    // 封板后落簿必拦。
    if tab.put(&assert_name(999), JCode { domain: 0, seq: 1 }, 1, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let (f, r) = tab.reconcile_flags();
    if !f || !r {
        return Err(E_FWD_MISMATCH);
    }
    let sm = tab.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 行解引抽查。
    let row0 = tab.row(0)?;
    if row0.subitem != 1 {
        return Err(E_REF_DANGLE);
    }
    if tab.row(TABLE_TARGET) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 组解引抽查：组 0 行数 == 3。
    let g0 = tab.group(0)?;
    if g0.rows.len() != 3 {
        return Err(E_REV_MISMATCH);
    }

    // —— 行数不足拦截：297 行拒封 ——
    let mut tab2 = MappingTable::new();
    let mut m = 0usize;
    let mut d2 = 0u8;
    while d2 < 4 {
        let mut sq = 1u8;
        while sq <= 25 {
            let mut sub = 1u8;
            while sub <= 3 {
                if m < 297 {
                    tab2.put(&assert_name(1000 + m), JCode { domain: d2, seq: sq }, sub, 1)?;
                    m += 1;
                }
                sub += 1;
            }
            sq += 1;
        }
        d2 += 1;
    }
    if tab2.seal() != Err(E_TARGET_SHORT) {
        return Err(E_TARGET_SHORT);
    }

    // —— 反向错位注入：手工破坏组内行号 → 必拦 ——
    let mut tab3 = MappingTable::new();
    tab3.put("a1", JCode { domain: 0, seq: 1 }, 1, 1)?;
    tab3.put("a2", JCode { domain: 0, seq: 1 }, 2, 1)?;
    if let Some(g) = tab3.groups.get_mut(0) {
        g.rows[0] = 99;
    }
    if tab3.audit_reverse() != Err(E_REV_MISMATCH) {
        return Err(E_REV_MISMATCH);
    }

    // —— 守恒注入：组行数拉爆 → 必拦 ——
    let mut tab4 = MappingTable::new();
    tab4.put("b1", JCode { domain: 1, seq: 1 }, 1, 1)?;
    if let Some(g) = tab4.groups.get_mut(0) {
        g.rows.push(0);
    }
    if tab4.audit_reverse() != Err(E_CONSERVATION) {
        return Err(E_CONSERVATION);
    }

    // —— 错误路径样本 ——
    if JCode::decode(0x0801) != Err(E_JDOM_RANGE) {
        return Err(E_JDOM_RANGE);
    }
    if JCode::decode(0x0000) != Err(E_JDOM_RANGE) {
        return Err(E_JDOM_RANGE);
    }
    let mut tab5 = MappingTable::new();
    if tab5.put("", JCode { domain: 0, seq: 1 }, 1, 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if tab5.put("x", JCode { domain: 0, seq: 1 }, 0, 1) != Err(E_SUBITEM_RANGE) {
        return Err(E_SUBITEM_RANGE);
    }
    if tab5.put("x", JCode { domain: 0, seq: 1 }, 17, 1) != Err(E_SUBITEM_RANGE) {
        return Err(E_SUBITEM_RANGE);
    }
    if tab5.put("x", JCode { domain: 0, seq: 1 }, 1, 0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if tab5.put("x", JCode { domain: 0, seq: 1 }, 1, 65) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    tab5.put("x", JCode { domain: 0, seq: 1 }, 1, 1)?;
    if tab5.put("x", JCode { domain: 0, seq: 1 }, 1, 2) != Err(E_MAP_DUP) {
        return Err(E_MAP_DUP);
    }
    // 组空注入。
    let mut tab6 = MappingTable::new();
    tab6.put("c1", JCode { domain: 2, seq: 1 }, 1, 1)?;
    if let Some(g) = tab6.groups.get_mut(0) {
        g.rows.clear();
    }
    if tab6.audit_reverse() != Err(E_JGROUP_EMPTY) {
        return Err(E_JGROUP_EMPTY);
    }
    // 悬空组解引。
    if tab6.group(5) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // 诊断表抽查。
    if diag_detail(E_FWD_MISMATCH) == "未知诊断码（0x5880 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5880 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
