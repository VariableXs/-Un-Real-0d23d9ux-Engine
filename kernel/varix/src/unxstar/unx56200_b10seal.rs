//! UNX-F56200 B10 批防重声明与批收官印件（0x61900 段）
//!
//! 锚点：UNX-F56200-J1 防重声明账（B10 批防重声明——批位 × 声明印
//! × 批内单号区间 × 收官态逐项落账）；J2 防重语义（区间闭集合法 +
//! 区间不交 + 印唯一三性质）；J3 收官指纹（收官账折叠指纹）三判据全
//! 绿。
//! 域：CoRun·UNX-F2-B40（启动域·B10 批收官面）。
//!
//! 结构：批位结构 + 防重声明簿 + 区间核（区间闸 + 不交闸 + 唯一印
//! 闸）+ 收官指纹 + 注入核（区间倒挂/区间相交/印重复/空声明/印后声
//! 明必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61900 独占 0x20）。
pub const SEG_BASE: u32 = 0x61900;
pub const SE_INVERT: u32 = SEG_BASE + 0x01; // 区间倒挂
pub const SE_OVERLAP: u32 = SEG_BASE + 0x02; // 区间相交
pub const SE_DUP_SEAL: u32 = SEG_BASE + 0x03; // 印重复
pub const SE_EMPTY_CLAIM: u32 = SEG_BASE + 0x04; // 空声明
pub const SE_FROZEN: u32 = SEG_BASE + 0x05; // 印后声明

/// 一条防重声明（批位区间）。
#[derive(Clone)]
pub struct NoDupClaim {
    pub batch: String,
    pub lo: u32,
    pub hi: u32,
}

impl NoDupClaim {
    /// 区间合法：lo ≤ hi。
    pub fn range_ok(&self) -> bool {
        self.lo <= self.hi
    }

    /// 与另一声明相交。
    pub fn overlaps(&self, other: &NoDupClaim) -> bool {
        self.lo <= other.hi && other.lo <= self.hi
    }

    /// 含单号。
    pub fn contains(&self, no: u32) -> bool {
        no >= self.lo && no <= self.hi
    }

    fn render_line(&self) -> String {
        format!("CLAIM|{}|[{},{}]", self.batch, self.lo, self.hi)
    }
}

/// B10 批收官印账。
pub struct BatchSealBook {
    claims: Vec<NoDupClaim>,
    seal_no: Option<u32>,
    sealed: bool,
}

impl BatchSealBook {
    pub fn new() -> Self {
        BatchSealBook {
            claims: Vec::new(),
            seal_no: None,
            sealed: false,
        }
    }

    /// 声明一批：区间闸 + 相交闸 + 印后拒。
    pub fn claim(&mut self, c: NoDupClaim) -> Result<(), u32> {
        if self.sealed {
            return Err(SE_FROZEN);
        }
        if !c.range_ok() {
            return Err(SE_INVERT);
        }
        if self.claims.iter().any(|x| x.overlaps(&c)) {
            return Err(SE_OVERLAP);
        }
        self.claims.push(c);
        Ok(())
    }

    pub fn claim_count(&self) -> usize {
        self.claims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    /// 单号归属查：返回承载该号的批位。
    pub fn owner(&self, no: u32) -> Option<&str> {
        self.claims.iter().find(|c| c.contains(no)).map(|c| c.batch.as_str())
    }

    /// 收官印：唯一印 + 全区间合法 + 声明非空。
    pub fn seal(&mut self, seal_no: u32) -> Result<(), u32> {
        if self.sealed || self.seal_no.is_some() {
            return Err(SE_DUP_SEAL);
        }
        if self.claims.is_empty() {
            return Err(SE_EMPTY_CLAIM);
        }
        if !self.claims.iter().all(|c| c.range_ok()) {
            return Err(SE_INVERT);
        }
        self.seal_no = Some(seal_no);
        self.sealed = true;
        Ok(())
    }

    pub fn seal_no(&self) -> Option<u32> {
        self.seal_no
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 收官指纹。
    pub fn seal_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.claims {
            for b in c.batch.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h = h.wrapping_mul(0x0100_0193) ^ c.lo;
            h = h.wrapping_mul(0x0100_0193) ^ c.hi;
        }
        h ^= self.seal_no.unwrap_or(0);
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for c in &self.claims {
            out.push_str(&c.render_line());
            out.push('\n');
        }
        if let Some(n) = self.seal_no {
            out.push_str(&format!("B10SEAL|no={}\n", n));
        }
        out
    }
}

impl Default for BatchSealBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：B10 批位（F56101~F56200）+ 前批位（F56001~F56100）声明后收官。
fn standard_book() -> BatchSealBook {
    let mut b = BatchSealBook::new();
    let _ = b.claim(NoDupClaim {
        batch: "B09".to_string(),
        lo: 56001,
        hi: 56100,
    });
    let _ = b.claim(NoDupClaim {
        batch: "B10".to_string(),
        lo: 56101,
        hi: 56200,
    });
    let _ = b.seal(0xB10C_0FF);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：区间倒挂
    let mut b = BatchSealBook::new();
    if b.claim(NoDupClaim { batch: "X".to_string(), lo: 100, hi: 50 }) == Err(SE_INVERT) {
        caught += 1;
    }
    // 注 2：区间相交
    let mut b = BatchSealBook::new();
    let _ = b.claim(NoDupClaim { batch: "A".to_string(), lo: 1, hi: 100 });
    if b.claim(NoDupClaim { batch: "B".to_string(), lo: 100, hi: 200 }) == Err(SE_OVERLAP) {
        caught += 1;
    }
    // 注 3：印重复
    let mut b = BatchSealBook::new();
    let _ = b.claim(NoDupClaim { batch: "A".to_string(), lo: 1, hi: 10 });
    let _ = b.seal(1);
    if b.seal(2) == Err(SE_DUP_SEAL) {
        caught += 1;
    }
    // 注 4：空声明收官
    let mut b = BatchSealBook::new();
    if b.seal(1) == Err(SE_EMPTY_CLAIM) {
        caught += 1;
    }
    // 注 5：印后声明
    let mut b = standard_book();
    if b.claim(NoDupClaim { batch: "LATE".to_string(), lo: 56300, hi: 56399 }) == Err(SE_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 防重声明账；J2 防重语义；J3 收官指纹。
pub fn run_unx56200_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56200");
    // J1-1 双批位声明 + 收官
    let b = standard_book();
    let ok = b.claim_count() == 2 && b.sealed() && b.seal_no().is_some();
    s.add("56200-J1-双批位收官", ok, if ok { "" } else { "收官账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("CLAIM|B09|[56001,56100]")
        && text.contains("CLAIM|B10|[56101,56200]")
        && text.contains("B10SEAL|no=185647359");
    s.add("56200-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 区间闭集合法
    let ok = b.claims.iter().all(|c| c.range_ok());
    s.add("56200-J2-区间合法", ok, if ok { "" } else { "区间越界" });
    // J2-2 区间不交
    let ok = !b.claims.windows(2).any(|w| w[0].overlaps(&w[1]));
    s.add("56200-J2-区间不交", ok, if ok { "" } else { "区间相交" });
    // J2-3 单号归属抽查（56150 → B10；56050 → B09）
    let ok = b.owner(56_150) == Some("B10") && b.owner(56_050) == Some("B09");
    s.add("56200-J2-单号归属", ok, if ok { "" } else { "归属漂移" });
    // J2-4 区间外查无
    let ok = b.owner(56_300).is_none();
    s.add("56200-J2-区间外查无", ok, if ok { "" } else { "误归属" });
    // J2-5 印唯一
    let mut b2 = standard_book();
    let ok = b2.seal(2) == Err(SE_DUP_SEAL);
    s.add("56200-J2-印唯一", ok, if ok { "" } else { "重印" });
    // J2-6 空声明收官拒
    let mut b3 = BatchSealBook::new();
    let ok = b3.seal(1) == Err(SE_EMPTY_CLAIM);
    s.add("56200-J2-空声明拒", ok, if ok { "" } else { "空声明闸失效" });
    // J3-1 收官指纹确定
    let ok = standard_book().seal_fp() == standard_book().seal_fp();
    s.add("56200-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 声明变签变
    let mut b4 = BatchSealBook::new();
    let _ = b4.claim(NoDupClaim { batch: "A".to_string(), lo: 1, hi: 10 });
    let fp0 = b4.seal_fp();
    let _ = b4.claim(NoDupClaim { batch: "B".to_string(), lo: 11, hi: 20 });
    let ok = b4.seal_fp() != fp0;
    s.add("56200-J3-声明变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 印变签变
    let mut b5 = BatchSealBook::new();
    let _ = b5.claim(NoDupClaim { batch: "A".to_string(), lo: 1, hi: 10 });
    let _ = b5.seal(0x1111_1111);
    let fp0 = b5.seal_fp();
    b5.seal_no = Some(0x2222_2222);
    let ok = b5.seal_fp() != fp0;
    s.add("56200-J3-印变签变", ok, if ok { "" } else { "印折叠退化" });
    // J3-4 印后拒声明
    let mut b9 = standard_book();
    let ok = b9.claim(NoDupClaim { batch: "LATE".to_string(), lo: 56300, hi: 56399 }) == Err(SE_FROZEN);
    s.add("56200-J3-印后拒声明", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56200-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56200_all_checks_green() {
        let set = run_unx56200_checks();
        assert!(set.all_passed(), "UNX-F56200 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56200_seal_stable() {
        assert!(standard_book().sealed());
    }
}
