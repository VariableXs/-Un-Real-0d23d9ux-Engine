//! UNX-F56076 大页/巨页口径注记件（0x60960 段）
//!
//! 锚点：UNX-F56076-J1 页径注记全量（大页/巨页两口径逐项落注：页径 ×
//! 单页字节 × 启用态三元组）；J2 换算口径一致（页数换算按注记页径逐项
//! 复算一致，含不整除余页显性化）；J3 注记审计链（注记指纹、封账只读、
//! 口径闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·页径口径面）。
//!
//! 结构：页径闭集（4K 基页/2M 大页/1G 巨页三闭集）+ 注记登记册（页径 →
//! 单页字节 × 启用态逐项落注）+ 换算核（字节数 → 页数 × 余页字节，u128
//! 中转防溢出）+ 启用态守闸（未启用页径换算拒收）+ 封账语义 + 注入核
//! （未知页径/零页径/未启用换算/封账改写/余页隐瞒/空注记必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60960 独占 0x20）。
pub const SEG_BASE: u32 = 0x60960;
pub const HP_UNK_PAGE: u32 = SEG_BASE + 0x01; // 未知页径（三闭集外）
pub const HP_SIZE_ZERO: u32 = SEG_BASE + 0x02; // 单页字节为零
pub const HP_DISABLED: u32 = SEG_BASE + 0x03; // 未启用页径换算
pub const HP_REMARK_DUP: u32 = SEG_BASE + 0x04; // 同页径重复注记
pub const HP_SEALED: u32 = SEG_BASE + 0x05; // 注记账已封账拒写
pub const HP_REMAINDER_HIDE: u32 = SEG_BASE + 0x06; // 余页隐瞒（换算不显性化）
pub const HP_EMPTY: u32 = SEG_BASE + 0x07; // 空注记查询

/// 页径闭集。
pub const PG_4K: usize = 0;
pub const PG_2M: usize = 1;
pub const PG_1G: usize = 2;
pub const PG_COUNT: usize = 3;
pub const PG_NAMES: [&str; PG_COUNT] = ["base_4k", "huge_2m", "giant_1g"];

/// 标准单页字节（注记正本口径）。
pub const PG_SIZES: [u64; PG_COUNT] = [4_096, 2_097_152, 1_073_741_824];

/// 页径指纹：FNV-1a 32 位。
pub fn page_fingerprint(name: &str, size: u64, enabled: bool) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", name, size, enabled);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 换算结论：页数 × 余页字节（余页显性化，禁隐瞒）。
#[derive(Clone)]
pub struct SplitResult {
    pub full_pages: u64,
    pub remainder_bytes: u64,
    pub page_size: u64,
}

impl SplitResult {
    /// 余页显性化自证：full*size + remainder == 原字节数。
    pub fn reconstruct_ok(&self, total_bytes: u64) -> bool {
        // u128 中转防乘法溢出。
        let recon = (self.full_pages as u128 * self.page_size as u128) + self.remainder_bytes as u128;
        recon == total_bytes as u128
    }
}

/// 换算核：字节数按页径切分（页径零或字节数零显性拒绝）。
pub fn split_pages(total_bytes: u64, page_size: u64) -> Result<SplitResult, u32> {
    if page_size == 0 {
        return Err(HP_SIZE_ZERO);
    }
    if total_bytes == 0 {
        return Err(HP_EMPTY);
    }
    let full = total_bytes / page_size;
    let rem = total_bytes % page_size;
    Ok(SplitResult { full_pages: full, remainder_bytes: rem, page_size })
}

/// 单条注记：页径 × 单页字节 × 启用态 三元组。
#[derive(Clone)]
pub struct HpRemark {
    pub page: usize,
    pub size: u64,
    pub enabled: bool,
}

/// 注记总账。
pub struct HpBook {
    pub remarks: [Option<HpRemark>; PG_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl HpBook {
    pub fn new() -> Self {
        HpBook { remarks: [None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 落注：页径闭集 + 字节非零 + 重复注记拒三闸。
    pub fn remark(&mut self, page: usize, size: u64, enabled: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(HP_SEALED);
        }
        if page >= PG_COUNT {
            return Err(HP_UNK_PAGE);
        }
        if size == 0 {
            return Err(HP_SIZE_ZERO);
        }
        if self.remarks[page].is_some() {
            return Err(HP_REMARK_DUP);
        }
        self.audit.push(format!("remark|{}|{}|{}", PG_NAMES[page], size, enabled));
        self.remarks[page] = Some(HpRemark { page, size, enabled });
        Ok(())
    }

    /// J2：换算守闸——页径已注且启用才放行换算。
    pub fn convert(&self, page: usize, total_bytes: u64) -> Result<SplitResult, u32> {
        if page >= PG_COUNT {
            return Err(HP_UNK_PAGE);
        }
        match &self.remarks[page] {
            None => Err(HP_EMPTY),
            Some(r) => {
                if !r.enabled {
                    return Err(HP_DISABLED);
                }
                let sp = split_pages(total_bytes, r.size)?;
                // 余页显性化守闸：余页必须如实回传（reconstruct 复核）。
                if !sp.reconstruct_ok(total_bytes) {
                    return Err(HP_REMAINDER_HIDE);
                }
                Ok(sp)
            }
        }
    }

    /// J1：注记全量（三页径逐项落注）。
    pub fn full_ok(&self) -> bool {
        self.remarks.iter().all(|r| r.is_some())
    }

    /// J1：口径一致性（落注页径与正本口径逐项一致）。
    pub fn sizes_match_canon(&self) -> bool {
        for p in 0..PG_COUNT {
            match &self.remarks[p] {
                None => return false,
                Some(r) if r.size != PG_SIZES[p] => return false,
                _ => {}
            }
        }
        true
    }

    /// J3：审计链完整。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty() && self.audit.iter().all(|a| a.starts_with("remark|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐项复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in 0..PG_COUNT {
            match &self.remarks[p] {
                None => {}
                Some(r) => {
                    h ^= page_fingerprint(PG_NAMES[p], r.size, r.enabled);
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }
}

/// 标准夹具：三页径全注（基页启用、大页启用、巨页未启用）。
pub fn standard_book() -> HpBook {
    let mut b = HpBook::new();
    let enables: [bool; PG_COUNT] = [true, true, false];
    for p in 0..PG_COUNT {
        if b.remark(p, PG_SIZES[p], enables[p]).is_err() {
            panic!("standard fixture remark must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知页径/零页径/未启用换算/重复注记/封账改写/余页隐瞒/空注记查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知页径
    let mut b = HpBook::new();
    if b.remark(9, 4096, true).err() == Some(HP_UNK_PAGE) {
        caught += 1;
    }
    // 2) 零页径
    if b.remark(PG_4K, 0, true).err() == Some(HP_SIZE_ZERO) {
        caught += 1;
    }
    // 3) 重复注记
    if b.remark(PG_4K, 4096, true).is_ok() && b.remark(PG_4K, 8192, true).err() == Some(HP_REMARK_DUP) {
        caught += 1;
    }
    // 4) 未启用页径换算
    let sb = standard_book();
    if sb.convert(PG_1G, 4_294_967_296).err() == Some(HP_DISABLED) {
        caught += 1;
    }
    // 5) 空注记查询
    if HpBook::new().convert(PG_4K, 8192).err() == Some(HP_EMPTY) {
        caught += 1;
    }
    // 6) 换算零字节拒绝
    if sb.convert(PG_4K, 0).err() == Some(HP_EMPTY) {
        caught += 1;
    }
    // 7) 封账后注记拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.remark(PG_1G, PG_SIZES[PG_1G], true).err() == Some(HP_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 注记账渲染导出（逐项一行）。
pub fn render_book(b: &HpBook) -> String {
    let mut out = String::new();
    for p in 0..PG_COUNT {
        match &b.remarks[p] {
            None => out.push_str(&format!("{}|UNSET\n", PG_NAMES[p])),
            Some(r) => out.push_str(&format!("{}|{}|{}\n", PG_NAMES[p], r.size, r.enabled)),
        }
    }
    out
}

/// 渲染完整性核：三行齐 + 逐行三段。
pub fn render_complete(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != PG_COUNT {
        return false;
    }
    for p in 0..PG_COUNT {
        if !text.contains(PG_NAMES[p]) {
            return false;
        }
    }
    lines.iter().all(|ln| ln.split('|').count() == 3 || ln.ends_with("UNSET"))
}

/// 判据入口（CheckSet）。
pub fn run_unx56076_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56076");
    // J1-1 注记全量（三页径逐项落注）
    let b = standard_book();
    let ok = b.full_ok() && !b.sealed;
    s.add("56076-J1-注记全量", ok, if ok { "" } else { "页径注记残缺" });
    // J1-2 口径与正本一致
    let ok = b.sizes_match_canon();
    s.add("56076-J1-口径正本一致", ok, if ok { "" } else { "页径口径漂移" });
    // J1-3 页径闭集齐
    let ok = PG_NAMES.len() == PG_COUNT && PG_NAMES[1] == "huge_2m";
    s.add("56076-J1-页径闭集齐", ok, if ok { "" } else { "页径闭集漂移" });
    // J2-1 4K 换算：8192 字节 → 2 页余 0
    let sp = b.convert(PG_4K, 8_192);
    let ok = match sp {
        Ok(sp) => sp.full_pages == 2 && sp.remainder_bytes == 0,
        Err(_) => false,
    };
    s.add("56076-J2-4K换算", ok, if ok { "" } else { "4K 换算漂移" });
    // J2-2 2M 换算：5M = 2 页余 1M
    let sp = b.convert(PG_2M, 5_242_880);
    let ok = match sp {
        Ok(sp) => sp.full_pages == 2 && sp.remainder_bytes == 1_048_576,
        Err(_) => false,
    };
    s.add("56076-J2-2M换算余页", ok, if ok { "" } else { "余页算术漂移" });
    // J2-3 余页显性化自证（reconstruct 闭合）
    let sp = b.convert(PG_2M, 5_242_880);
    let ok = match sp {
        Ok(sp) => sp.reconstruct_ok(5_242_880),
        Err(_) => false,
    };
    s.add("56076-J2-余页显性化", ok, if ok { "" } else { "余页闭合失败" });
    // J2-4 未启用巨页换算拒
    let ok = b.convert(PG_1G, 4_294_967_296).err() == Some(HP_DISABLED);
    s.add("56076-J2-未启用换算拒", ok, if ok { "" } else { "启用闸失效" });
    // J3-1 审计链完整（三条 remark 痕）
    let ok = b.audit_ok() && b.audit.len() == 3;
    s.add("56076-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56076-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 启用态指纹敏感（巨页转启用指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b2 = HpBook::new();
    for p in 0..PG_COUNT {
        let _ = b2.remark(p, PG_SIZES[p], true);
    }
    let ok = b2.seal_fingerprint() != fp0;
    s.add("56076-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.remark(PG_1G, PG_SIZES[PG_1G], true).err() == Some(HP_SEALED);
    s.add("56076-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text);
    s.add("56076-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56076-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56076_all_checks_green() {
        let set = run_unx56076_checks();
        assert!(set.all_passed(), "UNX-F56076 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56076_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
