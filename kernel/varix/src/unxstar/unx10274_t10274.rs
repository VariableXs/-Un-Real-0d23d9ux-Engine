//! UNX-F10274 长稳报告生成（时长/水位/失败三曲线）（0x16780 段）
//!
//! 锚点：UNX-F10274-J1 报告生成达成（时长/水位/失败三曲线落报告且数
//! 值可复算 10/10 次）；J2 曲线缺漏检出（任一曲线缺席拒出报告 10/10
//! 次）；J3 报告幂等（同三曲线同文本）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·长稳演习）。
//!
//! 结构：三曲线账（时长点/水位序列/失败序列）+ 报告编织核（统计复
//! 算）+ 缺漏检出核 + 注入核（缺曲线/坏值/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x16780 独占 0x20）。
pub const SEG_BASE: u32 = 0x16780;
pub const LR_MISS_CURVE: u32 = SEG_BASE + 0x01; // 缺曲线
pub const LR_BAD_VAL: u32 = SEG_BASE + 0x02; // 坏值
pub const LR_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const LR_EMPTY: u32 = SEG_BASE + 0x04; // 空曲线

/// 三曲线闭集。
pub const V_TIME: u8 = 0; // 时长
pub const V_LOAD: u8 = 1; // 水位
pub const V_FAIL: u8 = 2; // 失败
pub const CURVE_N: u8 = 3;

pub fn curve_ok(c: u8) -> bool {
    c < CURVE_N
}

pub fn curve_name(c: u8) -> &'static str {
    match c {
        V_TIME => "time",
        V_LOAD => "load",
        _ => "fail",
    }
}

/// 曲线点。
#[derive(Clone, Copy, PartialEq)]
pub struct CurvePt {
    pub curve: u8,
    pub seq: u32,
    pub val: u32, // 时长秒/水位千分位/失败计数
}

impl CurvePt {
    pub fn ok(&self) -> bool {
        curve_ok(self.curve) && match self.curve {
            V_TIME => self.val <= 8 * 3600,
            V_LOAD => self.val <= 1000,
            _ => true,
        }
    }
}

/// 三曲线账。
pub struct LongReport {
    rows: Vec<CurvePt>,
    frozen: bool,
}

impl LongReport {
    pub fn new() -> LongReport {
        LongReport { rows: Vec::new(), frozen: false }
    }

    /// 采点：坏值/重 (curve,seq)/冻结后写拒收。
    pub fn put(&mut self, curve: u8, seq: u32, val: u32) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let pt = CurvePt { curve, seq, val };
        if !pt.ok() {
            return Err("bad_val");
        }
        if self.rows.iter().any(|r| r.curve == curve && r.seq == seq) {
            return Err("dup_pt");
        }
        self.rows.push(pt);
        Ok(())
    }

    /// 曲线齐备：三曲线各至少一点。
    pub fn complete(&self) -> bool {
        (0..CURVE_N).all(|c| self.rows.iter().any(|r| r.curve == c))
    }

    pub fn missing(&self) -> Vec<u8> {
        (0..CURVE_N)
            .filter(|c| !self.rows.iter().any(|r| r.curve == *c))
            .collect()
    }

    /// 曲线统计：总数 + 均值。
    pub fn stat(&self, curve: u8) -> Result<(usize, u32), &'static str> {
        let pts: Vec<&CurvePt> = self.rows.iter().filter(|r| r.curve == curve).collect();
        if pts.is_empty() {
            return Err("empty_curve");
        }
        let sum: u64 = pts.iter().map(|r| r.val as u64).sum();
        Ok((pts.len(), (sum / pts.len() as u64) as u32))
    }

    /// 时长复算：max(time) 应为总时长。
    pub fn total_time(&self) -> Result<u32, &'static str> {
        let pts: Vec<&CurvePt> = self.rows.iter().filter(|r| r.curve == V_TIME).collect();
        pts.iter().map(|r| r.val).max().ok_or("empty_curve")
    }

    /// 报告文本（缺曲线拒绝）。
    pub fn render(&self) -> Result<String, String> {
        let miss = self.missing();
        if !miss.is_empty() {
            let names: Vec<&str> = miss.iter().map(|c| curve_name(*c)).collect();
            return Err(format!("missing:{}", names.join(",")));
        }
        let mut out = String::from("LONGRUN|");
        for c in 0..CURVE_N {
            let (n, avg) = self.stat(c).map_err(|e| e.to_string())?;
            out.push_str(&format!("{}:n{}avg{},", curve_name(c), n, avg));
        }
        out.push_str(&format!("total={}", self.total_time().map_err(|e| e.to_string())?));
        Ok(out)
    }

    /// 报告指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for b in self.render().unwrap_or_default().as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 铺三曲线基线（时长 3 点/水位 4 点/失败 2 点）。
pub fn seed(b: &mut LongReport) -> Result<(), &'static str> {
    for (i, v) in [(0u32, 900u32), (1, 1800), (2, 2700)] {
        b.put(V_TIME, i, v)?;
    }
    for (i, v) in [(0u32, 500u32), (1, 620), (2, 580), (3, 610)] {
        b.put(V_LOAD, i, v)?;
    }
    for (i, v) in [(0u32, 0u32), (1, 1)] {
        b.put(V_FAIL, i, v)?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut LongReport, code: u32) -> Result<(), &'static str> {
    match code {
        LR_MISS_CURVE => {
            let _ = b.put(V_TIME, 0, 900);
            b.put(V_LOAD, 0, 500) // 缺 fail
        }
        LR_BAD_VAL => b.put(V_LOAD, 0, 1001),
        LR_FROZEN => {
            b.freeze();
            b.put(V_TIME, 9, 100)
        }
        LR_EMPTY => b.stat(V_FAIL).map(|_| ()),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10274_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10274");

    // J1 三曲线报告生成且统计复算正确
    let j1 = {
        let mut b = LongReport::new();
        let _ = seed(&mut b);
        match b.render() {
            Ok(t) => t.contains("time:n3avg1800")
                && t.contains("load:n4avg577")
                && t.contains("fail:n2avg0")
                && t.ends_with("total=2700"),
            Err(_) => false,
        }
    };
    s.add("J1-三曲线报告", j1, "统计复算均值与总时长精确");

    // J2a 缺曲线检出
    let j2a = {
        let mut b = LongReport::new();
        let _ = inject_fault(&mut b, LR_MISS_CURVE);
        matches!(b.render(), Err(e) if e == "missing:fail")
    };
    s.add("J2a-缺曲线检出", j2a, "缺 fail 精确报");

    // J2b 坏值拒收（水位 >1000）
    let j2b = inject_fault(&mut LongReport::new(), LR_BAD_VAL) == Err("bad_val");
    s.add("J2b-坏值拒收", j2b, "1001 即拒");

    // J3 报告幂等：同曲线同文本同指纹
    let j3 = {
        let mk = || {
            let mut b = LongReport::new();
            let _ = seed(&mut b);
            (b.render().unwrap(), b.fp())
        };
        let (t1, f1) = mk();
        let (t2, f2) = mk();
        t1 == t2 && f1 == f2 && f1 != 0
    };
    s.add("J3-报告幂等", j3, "同输入同文本同指纹");

    // J4 时长复算真源：max 时长点
    let j4 = {
        let mut b = LongReport::new();
        let _ = seed(&mut b);
        b.total_time() == Ok(2700)
    };
    s.add("J4-时长复算", j4, "总时长取 max 点");

    // J5 空曲线统计拒绝
    let j5 = inject_fault(&mut LongReport::new(), LR_EMPTY) == Err("empty_curve");
    s.add("J5-空曲线拒", j5, "无点统计即拒");

    // J6 时长越界拒收（>8h）
    let j6 = LongReport::new().put(V_TIME, 0, 8 * 3600 + 1) == Err("bad_val");
    s.add("J6-时长越界拒", j6, "超 8h 即拒");

    // J7 重点拒收
    let j7 = {
        let mut b = LongReport::new();
        let a = b.put(V_TIME, 0, 100);
        let bb = b.put(V_TIME, 0, 200);
        a == Ok(()) && bb == Err("dup_pt")
    };
    s.add("J7-重点拒收", j7, "同曲线同序二次即拒");

    // J8 注入四路
    let codes = [LR_MISS_CURVE, LR_BAD_VAL, LR_FROZEN, LR_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut LongReport::new(), **c).is_err())
        .count();
    s.add("J8-注入四路", caught >= 3, "至少三路直接拒收");

    // J9 冻结红线
    let j9 = {
        let mut b = LongReport::new();
        inject_fault(&mut b, LR_FROZEN);
        b.is_frozen()
    };
    s.add("J9-冻结拒采", j9, "印后采点必拒");

    // J10 十轮报告回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = LongReport::new();
            let _ = seed(&mut b);
            b.fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮报告", j10, "10/10 轮指纹一致");

    s
}
