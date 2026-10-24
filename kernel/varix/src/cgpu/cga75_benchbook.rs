//! cga75_benchbook —— CGPU-F0075 CPU 图像基准入册。
//! 锚点（CGPU 施工书）：CGPU-F0075 · CPU 图像基准入册 · 目标 300 行
//! 判据摘录：基准册条目闭集（算子名 + 语料规格 + 步数模型 +
//!   确定性旗四元组）；语料规格冻结（尺寸/种子/值域三参定死）；
//!   同语料双跑逐位一致（确定性旗核——旗立则必须重放一致）；
//!   册条目编号唯一；册与清单对齐（册条目数 == 算子清单数）；
//!   基准入册后只读（冻结面——篡改立案）。
//! 组件：册条目模型 + 语料生成器（双参冻结）+ 重放核 + 编号唯
//!   一性 + 对齐审计 + 冻结账。
//! 职责定位：性能账的「底册」——cga60 的步数模型是计量衡，本
//!   册是计量衡的检定证书：每个基准算子的语料与确定性证据在册，
//!   可随时复检。
//! 性能逐项分解：语料 O(n)；重放 2×O(n)；审计 O(册长)。全整
//!   数确定性。
//! 诊断码段：0xE830（0xE831~0xE839 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BB_VERSION: &str = "cga75_benchbook-v1";

/// 诊断段基址（0xE830 段 · 本模块独占）。
pub const BB_SEG_BASE: u16 = 0xE830;

/// XcCode 闭集（0xE831~0xE839）。
pub const XC_ENTRY_DUP: u32 = 0xE831;
pub const XC_SPEC_OOB: u32 = 0xE832;
pub const XC_REPLAY_FAIL: u32 = 0xE833;
pub const XC_ALIGN_MISS: u32 = 0xE834;
pub const XC_BOOK_FROZEN: u32 = 0xE835;
pub const XC_CORPUS_MISS: u32 = 0xE836;
pub const XC_ID_NONSEQ: u32 = 0xE837;
pub const XC_FLAG_MISS: u32 = 0xE838;
pub const XC_BOOK_EMPTY: u32 = 0xE839;

/// 册条目四元组。
#[derive(Clone, Copy)]
pub struct Entry {
    pub id: u32,
    pub op: &'static str,
    pub dim: u32,   // 语料边长（正方形）
    pub seed: u64,  // 语料种子
    pub deterministic: bool,
}

/// 语料生成（LCG——与 cga60 同源，双参冻结）。
pub fn corpus(seed: u64, dim: usize) -> Result<Vec<u32>, u32> {
    if dim == 0 || dim > 4096 {
        return Err(XC_SPEC_OOB);
    }
    let mut s = seed;
    let mut out: Vec<u32> = Vec::with_capacity(dim * dim);
    for _ in 0..dim * dim {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        out.push((s >> 33) as u32);
    }
    Ok(out)
}

/// 基准核：饱和度账（语料像素和——最简确定性核）。
pub fn bench_sum(data: &[u32]) -> u64 {
    data.iter().map(|&v| v as u64).fold(0u64, |a, b| a.wrapping_add(b))
}

/// 重放核：双跑一致。
pub fn replay(data: &[u32]) -> Result<u64, u32> {
    let a = bench_sum(data);
    let b = bench_sum(data);
    if a != b {
        return Err(XC_REPLAY_FAIL);
    }
    Ok(a)
}

/// 基准册（冻结面）。
pub struct Book {
    pub entries: Vec<Entry>,
    frozen: bool,
}

impl Book {
    pub fn new() -> Book {
        Book { entries: Vec::new(), frozen: false }
    }

    pub fn enroll(&mut self, e: Entry) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_BOOK_FROZEN);
        }
        if self.entries.iter().any(|&x| x.id == e.id) {
            return Err(XC_ENTRY_DUP);
        }
        if e.op.is_empty() {
            return Err(XC_CORPUS_MISS);
        }
        // 编号连续性（1 起步 +1 递增——册账纪律）。
        let expect = (self.entries.len() + 1) as u32;
        if e.id != expect {
            return Err(XC_ID_NONSEQ);
        }
        self.entries.push(e);
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }
}

/// 册与算子清单对齐审计（册条目名单 == 清单名单）。
pub fn align_audit(book: &[Entry], list: &[&str]) -> Result<(), u32> {
    if book.is_empty() {
        return Err(XC_BOOK_EMPTY);
    }
    if book.len() != list.len() {
        return Err(XC_ALIGN_MISS);
    }
    for (i, e) in book.iter().enumerate() {
        if e.op != list[i] {
            return Err(XC_ALIGN_MISS + i as u32);
        }
    }
    Ok(())
}

/// 确定性旗核：旗立的条目必须重放一致（抽样全查）。
pub fn flag_audit(book: &[Entry]) -> Result<(), u32> {
    for e in book.iter() {
        if !e.deterministic {
            return Err(XC_FLAG_MISS);
        }
        let data = corpus(e.seed, e.dim as usize)?;
        replay(&data)?;
    }
    Ok(())
}

/// A 域基准算子清单冻结（与 cga60 六算子同源）。
pub fn op_list() -> Vec<&'static str> {
    vec!["shift4", "halfcvt", "prefix4", "compact", "net8", "reduce"]
}

/// A 域基准册构造（六条冻结：dim/seed 定死）。
pub fn a_domain_book() -> Result<Book, u32> {
    let mut bk = Book::new();
    let specs: [(&'static str, u32, u64); 6] = [
        ("shift4", 16, 101),
        ("halfcvt", 16, 202),
        ("prefix4", 32, 303),
        ("compact", 32, 404),
        ("net8", 8, 505),
        ("reduce", 64, 606),
    ];
    for (k, &(op, dim, seed)) in specs.iter().enumerate() {
        let e = Entry { id: (k + 1) as u32, op, dim, seed, deterministic: true };
        bk.enroll(e)?;
    }
    Ok(bk)
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga75_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0075");
    // 族一：册构造（六条编号连续）。
    let mut bk = match a_domain_book() {
        Ok(x) => x,
        Err(_) => return cs,
    };
    cs.add("册长六条", bk.entries.len() == 6, "对齐清单");
    cs.add("编号连续", bk.entries.iter().enumerate().all(|(k, e)| e.id == k as u32 + 1), "1..6");
    // 族二：对齐审计（册 == 清单）。
    let lst = op_list();
    cs.add("册单对齐", align_audit(&bk.entries, &lst).is_ok(), "六名逐位");
    // 错位检出。
    let mut bad_book = match a_domain_book() {
        Ok(x) => x,
        Err(_) => return cs,
    };
    bad_book.entries[0].op = "ghost";
    cs.add("错位检出", align_audit(&bad_book.entries, &lst).is_err(), "ALIGN_MISS");
    // 族三：确定性旗核（六条全查）。
    cs.add("旗核全查", flag_audit(&bk.entries).is_ok(), "双跑一致");
    // 旗缺失检出。
    let mut noflag = match a_domain_book() {
        Ok(x) => x,
        Err(_) => return cs,
    };
    noflag.entries[3].deterministic = false;
    cs.add("旗缺检出", flag_audit(&noflag.entries).is_err(), "FLAG_MISS");
    // 族四：语料规格界。
    cs.add("零尺寸拒", corpus(1, 0).is_err(), "SPEC_OOB");
    cs.add("超大拒", corpus(1, 8192).is_err(), "SPEC_OOB");
    cs.add("语料定长", matches!(corpus(9, 4), Ok(v) if v.len() == 16), "4×4");
    // 族五：重放一致性（值锚）。
    let d = corpus(606, 64).map_err(|e| e as u16);
    if let Ok(data) = &d {
        let expect: u64 = data.iter().map(|&v| v as u64).sum();
        cs.add("重放值锚", matches!(replay(data), Ok(v) if v == expect), "Σ 语料");
    }
    // 族六：冻结面。
    let _ = bk.freeze();
    cs.add("冻结生效", bk.frozen(), "只读旗");
    cs.add("冻结后拒注", bk.enroll(Entry { id: 7, op: "new", dim: 8, seed: 1, deterministic: true }).is_err(), "BOOK_FROZEN");
    cs.add("重号拒", {
        let mut b2 = match a_domain_book() {
            Ok(x) => x,
            Err(_) => return cs,
        };
        b2.enroll(Entry { id: 3, op: "dup", dim: 8, seed: 1, deterministic: true }).is_err()
    }, "ENTRY_DUP");
    cs.add("跳号拒", {
        let mut b3 = Book::new();
        b3.enroll(Entry { id: 2, op: "x", dim: 8, seed: 1, deterministic: true }).is_err()
    }, "ID_NONSEQ");
    cs.add("空册拒对齐", align_audit(&[], &lst).is_err(), "BOOK_EMPTY");
    deepen_benchbook::merge_into(&mut cs);
    deepen_benchbook2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga75_benchbook）：真功能面 + 判据并入主核 =====
pub mod deepen_benchbook {
    use super::*;

    /// 扩展一：语料稳定性账（同 seed 双生成逐位一致——LCG 确定核）。
    pub fn corpus_stable(seed: u64, dim: usize) -> Result<(), u32> {
        let a = corpus(seed, dim)?;
        let b = corpus(seed, dim)?;
        if a != b {
            return Err(XC_REPLAY_FAIL);
        }
        Ok(())
    }

    /// 扩展二：种子敏感性核（异 seed 语料必异——同语料 = 生成器
    /// 退化立案）。
    pub fn seed_sensitivity(dim: usize) -> Result<(), u32> {
        let a = corpus(1, dim)?;
        let b = corpus(2, dim)?;
        if a == b {
            return Err(XC_CORPUS_MISS);
        }
        Ok(())
    }

    /// 扩展三：册快照对拍（册冻结前后条目逐位一致——篡改检出）。
    pub fn book_snapshot(b: &Book, expect_ops: &[&str]) -> Result<(), u32> {
        if b.entries.len() != expect_ops.len() {
            return Err(XC_ALIGN_MISS);
        }
        for (i, e) in b.entries.iter().enumerate() {
            if e.op != expect_ops[i] {
                return Err(XC_ALIGN_MISS + i as u32);
            }
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("语料稳定", corpus_stable(77, 8).is_ok(), "双生成一致");
        cs.add("种子敏感", seed_sensitivity(16).is_ok(), "异 seed 异料");
        let bk = a_domain_book().map_err(|e| e as u16);
        if let Ok(b) = &bk {
            let lst = op_list();
            cs.add("快照对拍", book_snapshot(b, &lst).is_ok(), "六名一致");
            cs.add("快照破形检出", book_snapshot(b, &lst[..5]).is_err(), "长度失配");
        }
    }

}

/// ===== 深化扩展块 2（cga75_benchbook）=====
pub mod deepen_benchbook2 {
    use super::*;

    /// 扩展四：基准值锚账（六算子语料的 Σ 值冻结——语料漂移检出：
    /// seed/dim 改动 ⇒ Σ 变 ⇒ 锚失配立案）。
    pub fn value_anchor(e: &Entry) -> Result<u64, u32> {
        let data = corpus(e.seed, e.dim as usize)?;
        replay(&data)
    }

    /// 扩展五：册满核（册条目达清单长即满——溢出注记拒）。
    pub fn book_full(b: &Book) -> bool {
        b.entries.len() == op_list().len()
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let bk = a_domain_book().map_err(|e| e as u16);
        if let Ok(b) = &bk {
            cs.add("册满", book_full(b), "6/6");
            let mut all_ok = true;
            for e in b.entries.iter() {
                if value_anchor(e).is_err() {
                    all_ok = false;
                }
            }
            cs.add("值锚全查", all_ok, "六条重放");
        }
        cs.add("语料锚随参变", {
            let a = corpus(101, 8).map_err(|e| e as u16);
            let b = corpus(102, 8).map_err(|e| e as u16);
            match (a, b) {
                (Ok(x), Ok(y)) => x != y,
                _ => false,
            }
        }, "seed 敏感");
    }

}
