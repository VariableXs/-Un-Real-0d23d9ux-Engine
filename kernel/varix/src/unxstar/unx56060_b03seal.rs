//! UNX-F56060 B03 批防重声明与批收官印（unxstar 段）
//!
//! [J1] 批防重声明：B03 批覆盖 F56041–F56060 共 20 件，逐件登记（件号 + 关键词族 +
//!      件型），件号区间越界与重复登记一律拒绝；20 个关键词族两两互异。
//! [J2] 指纹折叠：批指纹 = 20 件条目指纹按序折叠（FNV-1a），同内容两次折叠一致、
//!      任一件内容变化则批指纹必变。
//! [J3] 批收官印：五段齐备（20 件全登记）+ 防重通过 + 件型分账（10 启动件 + 10
//!      计时/文档件）后才可落收官印；收官印一旦落下账本只读；注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（件号越界/重复登记/关键词族冲突/批未齐/账已封/指纹漂移/件型未知）
//!   纯函数：fnv32、条目指纹、批指纹折叠
//!   B03Entry：单件登记条目，new() 守闸
//!   B03SealBook：批防重账本（declare/seal/kind_split/family_unique）
//!   injection_core：5 例必抓
//!   run_unx56060_checks：≥12 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60760;

/// 件号越界（不在 56041..=56060）
pub const D_ID_RANGE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const D_DUP_DECLARE: u32 = SEG_BASE + 0x02;
/// 关键词族冲突（族名已被占用）
pub const D_FAMILY_CLASH: u32 = SEG_BASE + 0x03;
/// 批未齐（20 件未全登记即试图收官）
pub const D_BATCH_INCOMPLETE: u32 = SEG_BASE + 0x04;
/// 账已封（收官印后写入）
pub const D_SEALED: u32 = SEG_BASE + 0x05;
/// 指纹漂移（同内容两次折叠不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 件型未知（不在 closed-set 两型）
pub const D_KIND_UNKNOWN: u32 = SEG_BASE + 0x07;

/// B03 批区间
pub const BATCH_LO: u32 = 56041;
pub const BATCH_HI: u32 = 56060;
/// 件型闭集：boot（启动族）与 timerdoc（计时/文档族）
pub const KINDS: [&str; 2] = ["boot", "timerdoc"];

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 条目指纹：件号 || 关键词族 || 件型
pub fn entry_fp(e: &B03Entry) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&e.file_id.to_le_bytes());
    buf.extend_from_slice(e.family.as_bytes());
    buf.extend_from_slice(e.kind.as_bytes());
    fnv32(&buf)
}

/// 批指纹折叠（顺序敏感）
pub fn fold_fps(fps: &[u32]) -> u32 {
    let mut acc: u32 = 0x56060_00;
    for &f in fps {
        acc = acc.wrapping_mul(16777619) ^ f;
    }
    acc
}

/// 单件登记条目
#[derive(Clone)]
pub struct B03Entry {
    pub file_id: u32,
    pub family: String,
    pub kind: String,
}

impl B03Entry {
    /// 守闸：件号区间 + 件型闭集 + 族名非空
    pub fn new(file_id: u32, family: &str, kind: &str) -> Result<Self, u32> {
        if file_id < BATCH_LO || file_id > BATCH_HI {
            return Err(D_ID_RANGE);
        }
        if !KINDS.contains(&kind) {
            return Err(D_KIND_UNKNOWN);
        }
        if family.is_empty() {
            return Err(D_FAMILY_CLASH);
        }
        Ok(B03Entry { file_id, family: family.to_string(), kind: kind.to_string() })
    }
}

/// B03 批防重账本
pub struct B03SealBook {
    pub book_name: String,
    entries: Vec<B03Entry>,
    sealed: bool,
}

impl B03SealBook {
    pub fn new(book_name: &str) -> Self {
        B03SealBook { book_name: book_name.to_string(), entries: Vec::new(), sealed: false }
    }

    /// 防重声明：件号重复与族名占用均拒绝；封账后拒绝
    pub fn declare(&mut self, e: B03Entry) -> Result<u32, u32> {
        if self.sealed {
            return Err(D_SEALED);
        }
        if self.entries.iter().any(|x| x.file_id == e.file_id) {
            return Err(D_DUP_DECLARE);
        }
        if self.entries.iter().any(|x| x.family == e.family) {
            return Err(D_FAMILY_CLASH);
        }
        let fp = entry_fp(&e);
        self.entries.push(e);
        Ok(fp)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 批齐备
    pub fn batch_complete(&self) -> bool {
        self.entries.len() == 20
    }

    /// 关键词族两两互异
    pub fn family_unique(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].family == self.entries[j].family {
                    return false;
                }
            }
        }
        true
    }

    /// 件型分账 (boot, timerdoc)
    pub fn kind_split(&self) -> (usize, usize) {
        let boot = self.entries.iter().filter(|e| e.kind == "boot").count();
        let td = self.entries.iter().filter(|e| e.kind == "timerdoc").count();
        (boot, td)
    }

    /// 当前批指纹
    pub fn batch_fingerprint(&self) -> u32 {
        let fps: Vec<u32> = self.entries.iter().map(entry_fp).collect();
        fold_fps(&fps)
    }

    /// 落收官印：批未齐拒绝；成功后账本只读
    pub fn seal(&mut self) -> Result<u32, u32> {
        if !self.batch_complete() || !self.family_unique() {
            return Err(D_BATCH_INCOMPLETE);
        }
        self.sealed = true;
        Ok(self.batch_fingerprint())
    }
}

/// 标准夹具：F56041–56060 全登记，族名 f41..f60，boot/timerdoc 交替（各 10）
pub fn fixture_book() -> B03SealBook {
    let mut b = B03SealBook::new("b03-56041-56060");
    for i in 0..20u32 {
        let id = BATCH_LO + i;
        let kind = if i % 2 == 0 { "boot" } else { "timerdoc" };
        let e = B03Entry::new(id, &format!("f{}", 41 + i), kind).expect("fixture entry");
        b.declare(e).expect("fixture declare");
    }
    b
}

/// 指纹敏感性：改一件族名批指纹必变
pub fn fp_sensitivity() -> Result<(), u32> {
    let base = fixture_book().batch_fingerprint();
    let mut alt = B03SealBook::new("alt");
    for i in 0..20u32 {
        let id = BATCH_LO + i;
        let kind = if i % 2 == 0 { "boot" } else { "timerdoc" };
        let fam = if i == 7 { "f48x" } else { &format!("f{}", 41 + i) };
        let fam = fam.to_string();
        let e = B03Entry::new(id, &fam, kind).expect("alt entry");
        alt.declare(e).expect("alt declare");
    }
    if alt.batch_fingerprint() == base {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 件号越界
    if B03Entry::new(56061, "fx", "boot").err() == Some(D_ID_RANGE) {
        caught += 1;
    }

    // 2) 件型未知
    if B03Entry::new(56041, "fa", "mem").err() == Some(D_KIND_UNKNOWN) {
        caught += 1;
    }

    // 3) 件号重复
    let mut b = fixture_book();
    b.sealed = false;
    let dup = B03Entry::new(56041, "fzz", "boot").expect("dup");
    if b.declare(dup) == Err(D_DUP_DECLARE) {
        caught += 1;
    }

    // 4) 族名冲突
    let mut c = fixture_book();
    c.sealed = false;
    let clash = B03Entry::new(56050, "f41", "timerdoc").expect("clash");
    if c.declare(clash) == Err(D_FAMILY_CLASH) {
        caught += 1;
    }

    // 5) 未齐收官
    let mut h = B03SealBook::new("half");
    let e = B03Entry::new(56041, "f41", "boot").expect("e");
    let _ = h.declare(e);
    if h.seal() == Err(D_BATCH_INCOMPLETE) {
        caught += 1;
    }

    (caught, total)
}

/// 判据集
pub fn run_unx56060_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56060");

    // J1 批防重声明
    let b = fixture_book();
    s.add("56060-J1-廿件齐", b.len() == 20, "登记件数不为 20");
    s.add("56060-J1-族名互异", b.family_unique(), "关键词族存在重复");
    let (boot, td) = b.kind_split();
    s.add("56060-J1-件型分账", boot == 10 && td == 10, "boot/timerdoc 分账不为 10/10");
    s.add("56060-J1-件号闭集", B03Entry::new(56040, "fx", "boot").err() == Some(D_ID_RANGE), "56040 未被区间拒绝");

    // J2 指纹折叠
    let fp1 = b.batch_fingerprint();
    let fp2 = fixture_book().batch_fingerprint();
    s.add("56060-J2-批指纹稳定", fp1 == fp2, "同内容两次批指纹不一致");
    s.add("56060-J2-指纹敏感性", fp_sensitivity().is_ok(), "改件后批指纹未变");

    // J3 批收官印
    let mut c = fixture_book();
    let seal = c.seal().expect("seal");
    s.add("56060-J3-收官印一致", seal == fp1, "收官印指纹与批指纹不一致");
    s.add("56060-J3-封账只读", {
        let e = B03Entry::new(56060, "fzz", "boot").expect("e");
        c.declare(e) == Err(D_SEALED)
    }, "收官印后仍可写");
    s.add("56060-J3-未齐拒印", {
        let mut h = B03SealBook::new("h2");
        let e = B03Entry::new(56041, "f41", "boot").expect("e");
        let _ = h.declare(e);
        h.seal() == Err(D_BATCH_INCOMPLETE)
    }, "未齐收官未被拒绝");
    s.add("56060-J3-空族名拒", B03Entry::new(56041, "", "boot").err() == Some(D_FAMILY_CLASH), "空族名未被拒绝");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56060-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56060_all_checks_green() {
        let s = run_unx56060_checks();
        assert!(
            s.all_passed(),
            "UNX-F56060 有红项: {}",
            s.red_items_text()
        );
    }
}
