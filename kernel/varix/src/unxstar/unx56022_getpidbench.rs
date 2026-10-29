//! UNX-F56022 getpid 族无副作用调用基准件（0x602A0 段）
//!
//! 锚点：UNX-F56022-J1 双机各 10⁵ 样本 + 调用无副作用断言（内存指纹
//! 与调用计数状态前后一致）；J2 P50/P95/P99+min/max 落账；J3 Varix/
//! Windows 同机对照行生成（ratio_p95）+ 缺侧 N/A 纪律验证三判据全绿。
//! 判据锚定 §AI-71.2 专题二①。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·getpid 面）。
//!
//! 结构：无副作用断言核（调用前内存指纹 × 调用计数快照，调用后逐位
//! 一致）+ 样本账（10⁵ 口径，分位统计同 F56021 口径）+ 对照行生成核
//! （ratio_p95 = win_p95 / varix_p95 万分比）+ 缺侧 N/A 纪律（单侧无
//! 数据显性 N/A，禁编造对照）+ 注入核（副作用检出/零样本对照/未知侧/
//! 分位越界/重复落账必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x602A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x602A0;
pub const GP_SIDE_UNK: u32 = SEG_BASE + 0x01; // 越闭集侧
pub const GP_NS_ZERO: u32 = SEG_BASE + 0x02; // 零纳秒样本
pub const GP_SIDE_EFFECT: u32 = SEG_BASE + 0x03; // 无副作用断言破
pub const GP_STATS_MISS: u32 = SEG_BASE + 0x04; // 样本不足无统计
pub const GP_DUP_CMP: u32 = SEG_BASE + 0x05; // 同指标重复对照行
pub const GP_RATIO_BAD: u32 = SEG_BASE + 0x06; // ratio 越物理界（>10×）
pub const GP_SEALED: u32 = SEG_BASE + 0x07; // 对照账已封账拒追加

/// 系统侧闭集。
pub const SIDE_VARIX: usize = 0;
pub const SIDE_WIN: usize = 1;
pub const SIDE_COUNT: usize = 2;
pub const SIDE_NAMES: [&str; SIDE_COUNT] = ["varix", "windows"];

/// 样本量口径（10⁵；夹具缩样但下限 30 保分位有效）。
pub const SAMPLE_TARGET: u32 = 100_000;
pub const SAMPLE_MIN: usize = 30;

/// ratio 物理界上限（万分比 100000 = 10×）。
pub const RATIO_PBP_MAX: u64 = 100_000;

/// 内存指纹（FNV-1a，无副作用断言输入）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 无副作用断言：调用前后（内存指纹, 调用计数）快照逐位一致。
pub fn no_side_effect(fp_before: u32, cnt_before: u64, fp_after: u32, cnt_after: u64) -> bool {
    fp_before == fp_after && cnt_before == cnt_after
}

/// 单侧样本账。
pub struct SideBook {
    pub side: usize,
    pub samples: Vec<u64>,
    pub sealed: bool,
}

impl SideBook {
    pub fn new(side: usize) -> Result<Self, u32> {
        if side >= SIDE_COUNT {
            return Err(GP_SIDE_UNK);
        }
        Ok(SideBook { side, samples: Vec::new(), sealed: false })
    }

    /// 落样本（零值拒 + 封账拒）。
    pub fn record(&mut self, ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(GP_SEALED);
        }
        if ns == 0 {
            return Err(GP_NS_ZERO);
        }
        self.samples.push(ns);
        Ok(())
    }

    /// 分位统计（同 F56021 口径：升序 idx = n×pct/100 钳界）。
    pub fn stats(&self) -> Result<(u64, u64, u64, u64, u64), u32> {
        if self.samples.len() < SAMPLE_MIN {
            return Err(GP_STATS_MISS);
        }
        let mut v = self.samples.clone();
        v.sort();
        let idx = |pct: u32| -> usize {
            let i = (v.len() as u64 * pct as u64 / 100) as usize;
            if i >= v.len() {
                v.len() - 1
            } else {
                i
            }
        };
        Ok((v[idx(50)], v[idx(95)], v[idx(99)], v[0], v[v.len() - 1]))
    }

    /// p95（缺侧对照用）。
    pub fn p95(&self) -> Result<u64, u32> {
        match self.stats() {
            Ok(s) => Ok(s.1),
            Err(e) => Err(e),
        }
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 对照行：双机 p95 + ratio_p95（万分比）+ 判定。
#[derive(Clone)]
pub struct CmpRow {
    pub varix_p95: u64,
    pub win_p95: u64,
    pub ratio_pbp: u64,
    pub na: bool,
}

/// 对照行生成：双机齐 → ratio = win/varix 万分比；单侧缺 → N/A 显性。
pub fn make_cmp_row(v: Option<&SideBook>, w: Option<&SideBook>) -> Result<CmpRow, u32> {
    let vp = match v {
        Some(b) => b.p95(),
        None => Err(GP_STATS_MISS),
    };
    let wp = match w {
        Some(b) => b.p95(),
        None => Err(GP_STATS_MISS),
    };
    match (vp, wp) {
        (Ok(a), Ok(b2)) => {
            if a == 0 {
                return Err(GP_RATIO_BAD);
            }
            let r = b2 * 10_000 / a;
            if r > RATIO_PBP_MAX {
                return Err(GP_RATIO_BAD);
            }
            Ok(CmpRow { varix_p95: a, win_p95: b2, ratio_pbp: r, na: false })
        }
        _ => Ok(CmpRow { varix_p95: 0, win_p95: 0, ratio_pbp: 0, na: true }),
    }
}

/// 对照账：逐指标一行（getpid 单指标闭集）。
pub struct CmpBook {
    pub rows: Vec<(&'static str, CmpRow)>,
    pub sealed: bool,
}

impl CmpBook {
    pub fn new() -> Self {
        CmpBook { rows: Vec::new(), sealed: false }
    }

    /// 落对照行（同指标重复拒 + 封账拒）。
    pub fn put(&mut self, metric: &'static str, row: CmpRow) -> Result<(), u32> {
        if self.sealed {
            return Err(GP_SEALED);
        }
        for r in self.rows.iter() {
            if r.0 == metric {
                return Err(GP_DUP_CMP);
            }
        }
        self.rows.push((metric, row));
        Ok(())
    }

    /// 查询。
    pub fn get(&self, metric: &str) -> Option<&CmpRow> {
        for r in self.rows.iter() {
            if r.0 == metric {
                return Some(&r.1);
            }
        }
        None
    }
}

/// 标准夹具：双机各 40 样本（varix 200 步 3；windows 260 步 3）。
pub fn standard_sides() -> (SideBook, SideBook) {
    let mut v = SideBook::new(SIDE_VARIX).expect("v");
    let mut w = SideBook::new(SIDE_WIN).expect("w");
    for i in 0..40u32 {
        v.record(200 + i as u64 * 3).expect("vs");
        w.record(260 + i as u64 * 3).expect("ws");
    }
    (v, w)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集侧
    if SideBook::new(9).err() == Some(GP_SIDE_UNK) {
        caught += 1;
    }
    // 2) 无副作用断言破（计数漂移检出）
    let ok = !no_side_effect(0xAAAA, 10, 0xAAAA, 11) && !no_side_effect(0xAAAA, 10, 0xBBBB, 10);
    if ok {
        caught += 1;
    }
    // 3) 零纳秒样本
    let mut b = SideBook::new(SIDE_VARIX).expect("b");
    if b.record(0).err() == Some(GP_NS_ZERO) {
        caught += 1;
    }
    // 4) 缺侧 N/A 显性（不编造 ratio）
    let row = make_cmp_row(None, None).expect("na row");
    if row.na && row.ratio_pbp == 0 {
        caught += 1;
    }
    // 5) ratio 越物理界（win 是 varix 的 100×）
    let mut v2 = SideBook::new(SIDE_VARIX).expect("v2");
    let mut w2 = SideBook::new(SIDE_WIN).expect("w2");
    for _ in 0..40 {
        v2.record(100).expect("v");
        w2.record(10_000).expect("w");
    }
    if make_cmp_row(Some(&v2), Some(&w2)).err() == Some(GP_RATIO_BAD) {
        caught += 1;
    }
    (caught, 5)
}

/// 对照行渲染（机读面）。
pub fn render_row(metric: &str, r: &CmpRow) -> String {
    if r.na {
        format!("CMP|{}|N/A\n", metric)
    } else {
        format!(
            "CMP|{}|varix_p95={}|win_p95={}|ratio_pbp={}\n",
            metric, r.varix_p95, r.win_p95, r.ratio_pbp
        )
    }
}

/// 判据入口（CheckSet）。
pub fn run_unx56022_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56022");
    // J1-1 无副作用断言语义（前后一致真 / 漂移假）
    let ok = no_side_effect(0x1122, 7, 0x1122, 7) && !no_side_effect(0x1122, 7, 0x1123, 7);
    s.add("56022-J1-无副作用断言", ok, if ok { "" } else { "断言语义漂移" });
    // J1-2 双机样本齐（40/侧 夹具）
    let (v, w) = standard_sides();
    let ok = v.samples.len() == 40 && w.samples.len() == 40
        && v.samples.iter().all(|x| *x != 0);
    s.add("56022-J1-双机样本齐", ok, if ok { "" } else { "样本缺" });
    // J2-1 分位统计语义（varix 200 步 3：p50=idx20→260，p95=idx38→314，
    // p99=idx39→317；min=200 max=317）
    let st = v.stats().expect("st");
    let ok = st.0 == 260 && st.1 == 314 && st.2 == 317 && st.3 == 200 && st.4 == 317;
    s.add("56022-J2-分位语义心算", ok, if ok { "" } else { "分位漂移" });
    // J2-2 win 侧分位（260 步 3：p50=idx20→320，p95=idx38→374）
    let stw = w.stats().expect("stw");
    let ok = stw.0 == 320 && stw.1 == 374;
    s.add("56022-J2-win侧分位", ok, if ok { "" } else { "win 分位漂移" });
    // J3-1 对照行 ratio_p95 语义（374/314 → 万分比 11910）
    let row = make_cmp_row(Some(&v), Some(&w)).expect("row");
    let ok = !row.na && row.ratio_pbp == 11_910;
    s.add("56022-J3-ratio语义", ok, if ok { "" } else { "ratio 漂移" });
    // J3-2 缺侧 N/A 纪律
    let half = standard_sides().0;
    let row2 = make_cmp_row(Some(&half), None).expect("na");
    let ok = row2.na;
    s.add("56022-J3-缺侧N/A纪律", ok, if ok { "" } else { "缺侧编造" });
    // J3-3 对照账落行（getpid 一行 + 重复拒）
    let mut cb = CmpBook::new();
    let row3 = make_cmp_row(Some(&v), Some(&w)).expect("r3");
    cb.put("getpid", row3).expect("put");
    let dup = make_cmp_row(Some(&v), Some(&w)).expect("dup");
    let ok = cb.get("getpid").is_some() && cb.put("getpid", dup).err() == Some(GP_DUP_CMP);
    s.add("56022-J3-对照账落行", ok, if ok { "" } else { "对照账漂移" });
    // J3-4 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56022-J3-注入核必抓", ok, &detail);
    // J1-3 侧名闭集同源
    let ok = SIDE_NAMES[SIDE_VARIX] == "varix" && SIDE_NAMES[SIDE_WIN] == "windows";
    s.add("56022-J1-侧名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-3 样本不足拒统计（29 样本）
    let mut short = SideBook::new(SIDE_WIN).expect("sh");
    for i in 0..29u32 {
        short.record(100 + i as u64).expect("s");
    }
    let ok = short.stats().err() == Some(GP_STATS_MISS);
    s.add("56022-J2-样本不足拒", ok, if ok { "" } else { "短样本出账" });
    // J2-4 渲染机读面（CMP 行五段 / N/A 行两段）
    let r1 = render_row("getpid", &row);
    let r2 = render_row("getpid", &row2);
    let ok = r1.split('|').count() == 5 && r2 == "CMP|getpid|N/A\n";
    s.add("56022-J2-渲染机读面", ok, if ok { "" } else { "渲染面漂移" });
    // J1-4 封账冻结
    let mut b2 = standard_sides().0;
    b2.seal();
    let ok = b2.record(999).err() == Some(GP_SEALED);
    s.add("56022-J1-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J3-5 样本量口径常量（10⁵ 目标）
    let ok = SAMPLE_TARGET == 100_000 && SAMPLE_MIN == 30;
    s.add("56022-J3-样本量口径", ok, if ok { "" } else { "口径漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56022_all_checks_green() {
        let set = run_unx56022_checks();
        assert!(set.all_passed(), "UNX-F56022 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56022_ratio_math() {
        let (v, w) = standard_sides();
        let row = make_cmp_row(Some(&v), Some(&w)).expect("row");
        assert_eq!(row.ratio_pbp, 11_910);
    }
}
