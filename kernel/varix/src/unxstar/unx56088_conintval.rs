//! UNX-F56088 置信区间计算件（"提升 X%"配套）（0x60AE0 段）
//!
//! 锚点：UNX-F56088-J1 区间登记全量（提升声明逐条落账：声明键 × 基线
//! 均值 × 对照均值 × 样本对数四元组）；J2 区间可复算（"提升 X%"必须配
//! 置信区间：区间半宽按给定口径复算一致，无区间提升声明拒收）；J3 区间
//! 审计链（声明指纹链、封账只读、区间语义闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·置信区间面）。
//!
//! 结构：提升声明登记册（声明键 → 基线均值 × 对照均值 × n 逐条落账）+
//! 区间核（均值差 × 波动率半径按样本数折减，简单可信口径：半宽 =
//! 基线 × 波动万分比 / sqrt(n) 折减）+ 提升复核核（差值与声明率复算
//! 一致）+ 无区间拒收语义 + 指纹链 + 注入核（空声明键/样本对不足/无
//! 区间声明/提升率漂移/重复声明/封账改写/零基线必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60AE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60AE0;
pub const CI_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 声明键为空
pub const CI_N_SHORT: u32 = SEG_BASE + 0x02; // 样本对不足（<30）
pub const CI_NO_INTERVAL: u32 = SEG_BASE + 0x03; // 无区间提升声明
pub const CI_RATE_MISS: u32 = SEG_BASE + 0x04; // 提升率复算漂移
pub const CI_DUP: u32 = SEG_BASE + 0x05; // 重复声明
pub const CI_SEALED: u32 = SEG_BASE + 0x06; // 区间账已封账拒写
pub const CI_BASE_ZERO: u32 = SEG_BASE + 0x07; // 零基线（不可比）

/// 最小样本对纪律（与 F56086 同源）。
pub const PAIR_MIN: u32 = 30;

/// 波动率口径（万分比：区间半宽系数）。
pub const VOL_BP: u64 = 300;

/// 区间结论：均值差 × 半宽 × 提升率万分比 × 下限是否过零。
#[derive(Clone)]
pub struct Interval {
    pub delta: i64,
    pub half_width: u64,
    pub rate_bp: i64,
    pub crosses_zero: bool,
}

impl Interval {
    /// 区间下限为正才可声称"确定提升"。
    pub fn significant(&self) -> bool {
        !self.crosses_zero && self.delta > 0
    }
}

/// 平方根整数核（牛顿逐位法，向下取整）。
pub fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// 区间核：delta = ctrl - base；半宽 = |base| × VOL_BP / 10000 / sqrt(n)
/// 折减；提升率 = delta × 10000 / base（i64 中转防负数溢出）。
pub fn compute_interval(base_mean: u64, ctrl_mean: u64, n: u32) -> Result<Interval, u32> {
    if n == 0 {
        return Err(CI_N_SHORT);
    }
    if n < PAIR_MIN {
        return Err(CI_N_SHORT);
    }
    if base_mean == 0 {
        return Err(CI_BASE_ZERO);
    }
    let delta = ctrl_mean as i64 - base_mean as i64;
    let sq = isqrt(n as u64);
    if sq == 0 {
        return Err(CI_N_SHORT);
    }
    let half_width = base_mean * VOL_BP / 10_000 / sq;
    let rate_bp = delta * 10_000 / base_mean as i64;
    let crosses_zero = (delta.unsigned_abs()) <= half_width;
    Ok(Interval { delta, half_width, rate_bp, crosses_zero })
}

/// 声明指纹：FNV-1a 32 位。
pub fn claim_fingerprint(key: &str, base: u64, ctrl: u64, n: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}|{}", key, base, ctrl, n);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 区间账目：声明键 × 基线 × 对照 × n × 区间 × 指纹 × 链。
#[derive(Clone)]
pub struct ClaimRow {
    pub key: String,
    pub base_mean: u64,
    pub ctrl_mean: u64,
    pub n: u32,
    pub interval: Interval,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 区间总账。
pub struct CiBook {
    pub rows: Vec<ClaimRow>,
    pub sealed: bool,
}

impl CiBook {
    pub fn new() -> Self {
        CiBook { rows: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 声明落账：空键拒 + 样本闸 + 重复拒 + 区间必算四闸（无区间即拒）。
    pub fn declare(&mut self, key: &str, base: u64, ctrl: u64, n: u32) -> Result<Interval, u32> {
        if self.sealed {
            return Err(CI_SEALED);
        }
        if key.is_empty() {
            return Err(CI_KEY_EMPTY);
        }
        let itv = compute_interval(base, ctrl, n)?;
        if self.find(key).is_some() {
            return Err(CI_DUP);
        }
        let fp = claim_fingerprint(key, base, ctrl, n);
        let chain = match self.rows.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.rows.push(ClaimRow {
            key: key.to_string(),
            base_mean: base,
            ctrl_mean: ctrl,
            n,
            interval: itv.clone(),
            fp,
            chain,
        });
        Ok(itv)
    }

    /// J2：提升率复核（声明率与区间率逐条一致）。
    pub fn rate_ok(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| {
            let expect = r.interval.delta * 10_000 / r.base_mean as i64;
            expect == r.interval.rate_bp
        })
    }

    /// J1：区间账全量（逐条区间语义合法）。
    pub fn rows_ok(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| {
                r.base_mean > 0 && r.n >= PAIR_MIN && r.interval.half_width > 0
            })
    }

    /// J3：区间指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        for i in 0..self.rows.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.rows[0].fp
            } else {
                self.rows[i - 1].fp ^ self.rows[i].fp
            };
            if self.rows[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.rows.last() {
            None => CHAIN_SEED,
            Some(r) => r.chain,
        }
    }
}

/// 标准夹具：2 条声明（+5% 与 -2%，n=100）。
pub fn standard_book() -> CiBook {
    let mut b = CiBook::new();
    if b.declare("mem|wave1|up", 10_000, 10_500, 100).is_err() {
        panic!("standard fixture up must declare");
    }
    if b.declare("mem|wave1|down", 10_000, 9_800, 100).is_err() {
        panic!("standard fixture down must declare");
    }
    b
}

/// 注入核：7 注入必抓（空声明键/样本不足/零基线/重复声明/封账改写/空账链/提升率漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空声明键
    let mut b = CiBook::new();
    if b.declare("", 10_000, 10_500, 100).err() == Some(CI_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 样本不足（29 < 30）
    if b.declare("k", 10_000, 10_500, 29).err() == Some(CI_N_SHORT) {
        caught += 1;
    }
    // 3) 零基线
    if b.declare("k", 0, 10_500, 100).err() == Some(CI_BASE_ZERO) {
        caught += 1;
    }
    // 4) 重复声明
    if b.declare("k", 10_000, 10_500, 100).is_ok()
        && b.declare("k", 10_000, 10_600, 100).err() == Some(CI_DUP)
    {
        caught += 1;
    }
    // 5) 提升率漂移必抓（篡改率后复算失败）
    let mut dirty = standard_book();
    dirty.rows[0].interval.rate_bp = 999;
    if !dirty.rate_ok() {
        caught += 1;
    }
    // 6) 空账链复核
    if !CiBook::new().chain_ok() {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.declare("late", 10_000, 10_500, 100).err() == Some(CI_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 区间账渲染导出（逐条一行）。
pub fn render_book(b: &CiBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}|d{}|hw{}|{}\n",
            r.key, r.base_mean, r.ctrl_mean, r.n, r.interval.delta, r.interval.half_width,
            r.interval.rate_bp
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行七段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx56088_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56088");
    // J1-1 区间账全量（两声明逐条合法）
    let b = standard_book();
    let ok = b.rows_ok() && b.rows.len() == 2;
    s.add("56088-J1-区间账全量", ok, if ok { "" } else { "声明残缺" });
    // J1-2 isqrt 复算（sqrt(100)=10、sqrt(101)=10、sqrt(0)=0、sqrt(1)=1）
    let ok = isqrt(100) == 10 && isqrt(101) == 10 && isqrt(0) == 0 && isqrt(1) == 1;
    s.add("56088-J1-isqrt复算", ok, if ok { "" } else { "平方根漂移" });
    // J1-3 提升声明区间（+500、半宽 = 10000*300/10000/10 = 300、下限 +200 过零）
    let up = &b.rows[0].interval;
    let ok = up.delta == 500 && up.half_width == 300 && up.rate_bp == 500 && !up.crosses_zero && up.significant();
    s.add("56088-J1-提升区间语义", ok, if ok { "" } else { "区间算术漂移" });
    // J1-4 降级声明语义（-200、下限 -500 过零 → 不显著）
    let dn = &b.rows[1].interval;
    let ok = dn.delta == -200 && dn.rate_bp == -200 && !dn.significant();
    s.add("56088-J1-降级区间语义", ok, if ok { "" } else { "降级语义漂移" });
    // J2-1 无区间声明拒收（样本不足即无区间）
    let mut b2 = CiBook::new();
    let ok = b2.declare("k", 10_000, 10_500, 29).err() == Some(CI_N_SHORT);
    s.add("56088-J2-无区间拒收", ok, if ok { "" } else { "区间闸失效" });
    // J2-2 提升率复核全绿
    let ok = b.rate_ok();
    s.add("56088-J2-提升率复核", ok, if ok { "" } else { "提升率漂移" });
    // J2-3 样本对折减效应（n=10000 半宽 = 10000*300/10000/100 = 3）
    let itv = compute_interval(10_000, 10_100, 10_000);
    let ok = match itv {
        Ok(i) => i.half_width == 3 && i.delta == 100 && i.significant(),
        Err(_) => false,
    };
    s.add("56088-J2-折减效应", ok, if ok { "" } else { "折减算术漂移" });
    // J2-4 过零判定（delta ≤ 半宽即过零）
    let itv = compute_interval(10_000, 10_020, 100);
    let ok = match itv {
        Ok(i) => i.crosses_zero && !i.significant(),
        Err(_) => false,
    };
    s.add("56088-J2-过零判定", ok, if ok { "" } else { "过零语义漂移" });
    // J3-1 区间指纹链复核
    let ok = b.chain_ok();
    s.add("56088-J3-区间指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56088-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改声明值链尾变）
    let mut b3 = CiBook::new();
    let _ = b3.declare("mem|wave1|up", 10_000, 10_501, 100);
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56088-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.declare("late", 10_000, 10_500, 100).err() == Some(CI_SEALED);
    s.add("56088-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J2-5 渲染导出两行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 2) && text.contains("d500|hw300|500");
    s.add("56088-J2-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56088-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56088_all_checks_green() {
        let set = run_unx56088_checks();
        assert!(set.all_passed(), "UNX-F56088 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56088_isqrt_stable() {
        assert_eq!(isqrt(1_000_000), 1_000);
    }
}
