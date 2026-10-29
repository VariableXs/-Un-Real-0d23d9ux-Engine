//! UNX-F46430 头文件扫描报告格式（缺失/漂移/多余三分类输出，可导出）（0x401D0 段）
//!
//! 锚点：UNX-F46430-J1 三分类注入 ×30 逐条正确归类。
//! 域：CoRun·UNX-S1-B02（工具链域·报告格式面）。
//!
//! 结构：报告条目（头 × 分类 × 期望指纹 × 实测指纹）+ 三分类器
//! （缺失 = 期望有实测无 / 漂移 = 双有指纹异 / 多余 = 期望无实测有）
//! + 扫描报告账（逐条登记 × 封账）+ 可导出渲染（分段报告文本）
//! + 注入核（30 条注入逐条归位 + 重复登记/越头集/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x401D0 独占 0x10）。
pub const SEG_BASE: u32 = 0x401D0;
pub const RF_BAD_CLASS: u32 = SEG_BASE + 0x01; // 非法分类值
pub const RF_DUP: u32 = SEG_BASE + 0x02; // 重复登记
pub const RF_OOB: u32 = SEG_BASE + 0x03; // 越头集
pub const RF_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const RF_EMPTY: u32 = SEG_BASE + 0x05; // 空报告导出拒

/// 期望头集 ×20（与 F46421/46429 同源）。
pub const RF_HEADS: [&str; 20] = [
    "stdio.h", "stdlib.h", "string.h", "stdint.h", "stddef.h",
    "stdbool.h", "unistd.h", "fcntl.h", "errno.h", "signal.h",
    "time.h", "math.h", "assert.h", "ctype.h", "limits.h",
    "locale.h", "setjmp.h", "stdarg.h", "wchar.h", "pthread.h",
];

/// 三分类值：0 = CLEAN（无异常），1 = MISSING，2 = DRIFT，3 = EXTRA。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RfClass {
    Clean,
    Missing,
    Drift,
    Extra,
}

impl RfClass {
    pub fn name(&self) -> &'static str {
        match self {
            RfClass::Clean => "CLEAN",
            RfClass::Missing => "MISSING",
            RfClass::Drift => "DRIFT",
            RfClass::Extra => "EXTRA",
        }
    }

    pub fn code(&self) -> u8 {
        match self {
            RfClass::Clean => 0,
            RfClass::Missing => 1,
            RfClass::Drift => 2,
            RfClass::Extra => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<RfClass, u32> {
        match c {
            0 => Ok(RfClass::Clean),
            1 => Ok(RfClass::Missing),
            2 => Ok(RfClass::Drift),
            3 => Ok(RfClass::Extra),
            _ => Err(RF_BAD_CLASS),
        }
    }
}

/// 报告条目。
#[derive(Clone)]
pub struct ReportEntry {
    pub head: String,
    pub class: RfClass,
    pub expect_fp: u32,
    pub actual_fp: u32,
}

/// 三分类器：期望集 × 实测集 → 逐头归类。
/// 期望以 RF_HEADS 为序；实测中不属于期望头集的逐条 EXTRA。
pub fn classify(
    expected: &[(String, u32)],
    observed: &[(String, u32)],
) -> Vec<ReportEntry> {
    let mut out = Vec::new();
    // 1) 期望头逐个判定。
    for (h, efp) in expected.iter() {
        match observed.iter().find(|(oh, _)| oh == h) {
            None => out.push(ReportEntry {
                head: h.clone(),
                class: RfClass::Missing,
                expect_fp: *efp,
                actual_fp: 0,
            }),
            Some((_, afp)) => {
                let class = if afp == efp { RfClass::Clean } else { RfClass::Drift };
                out.push(ReportEntry {
                    head: h.clone(),
                    class,
                    expect_fp: *efp,
                    actual_fp: *afp,
                });
            }
        }
    }
    // 2) 实测多余头（不在期望集内即多余，无论其是否期望头集成员）。
    for (oh, ofp) in observed.iter() {
        if !expected.iter().any(|(h, _)| h == oh) {
            out.push(ReportEntry {
                head: oh.clone(),
                class: RfClass::Extra,
                expect_fp: 0,
                actual_fp: *ofp,
            });
        }
    }
    out
}

/// 扫描报告账：逐条登记 × 封账 × 计数。
pub struct ScanReport {
    pub entries: Vec<ReportEntry>,
    pub sealed: bool,
}

impl ScanReport {
    pub fn new() -> Self {
        ScanReport { entries: Vec::new(), sealed: false }
    }

    /// 登记一条报告（重复头/越头集/封账拒绝；Extra 头必须是非期望头集成员）。
    pub fn add(&mut self, e: ReportEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(RF_SEALED);
        }
        let is_known = RF_HEADS.contains(&e.head.as_str());
        if !is_known && e.class != RfClass::Extra {
            return Err(RF_OOB);
        }
        if is_known && e.class == RfClass::Extra {
            return Err(RF_BAD_CLASS);
        }
        if self.entries.iter().any(|x| x.head == e.head) {
            return Err(RF_DUP);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 分类计数。
    pub fn count(&self, c: RfClass) -> usize {
        self.entries.iter().filter(|e| e.class == c).count()
    }

    /// 异常总数（MISSING + DRIFT + EXTRA）。
    pub fn anomalies(&self) -> usize {
        self.entries.len() - self.count(RfClass::Clean)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 报告指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for e in self.entries.iter() {
            for b in format!("{}|{}|{:x}|{:x};", e.head, e.class.code(), e.expect_fp, e.actual_fp)
                .as_bytes()
            {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 可导出渲染：分段文本报告（异常分段在前，CLEAN 汇总在后）。
pub fn render_report(r: &ScanReport) -> Result<String, u32> {
    if r.entries.is_empty() {
        return Err(RF_EMPTY);
    }
    let mut out = String::new();
    out.push_str("== UNX-F46430 头扫描报告 ==\n");
    for cls in [RfClass::Missing, RfClass::Drift, RfClass::Extra] {
        let n = r.count(cls);
        out.push_str(&format!("[{}] n={}\n", cls.name(), n));
        for e in r.entries.iter() {
            if e.class == cls {
                out.push_str(&format!(
                    "- {} expect={:08x} actual={:08x}\n",
                    e.head, e.expect_fp, e.actual_fp
                ));
            }
        }
    }
    out.push_str(&format!(
        "[CLEAN] n={} total={} sealed={}\n",
        r.count(RfClass::Clean),
        r.entries.len(),
        r.sealed
    ));
    Ok(out)
}

/// 标准夹具：期望集 = 20 头基线指纹；实测集由注入清单修饰。
pub fn baseline() -> Vec<(String, u32)> {
    RF_HEADS
        .iter()
        .map(|h| ((*h).to_string(), fp_of(h)))
        .collect()
}

/// 基线指纹（确定性）。
pub fn fp_of(head: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in format!("rf30|{}", head).as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 注入清单构造：10 缺失 + 10 漂移 + 10 多余（互不重叠，确定性）。
/// 缺失 = 期望头去掉实测（前 10 头）；漂移 = 中 10 头指纹翻转；
/// 多余 = 追加 10 个非期望头。
pub fn injected_observed() -> (Vec<(String, u32)>, usize, usize, usize) {
    let mut obs: Vec<(String, u32)> = Vec::new();
    // 后 10 头在测且全部漂移；前 10 头缺席（缺失）；另有 10 个非期望头（多余）。
    for (i, h) in RF_HEADS.iter().enumerate() {
        if i < 10 {
            continue; // 前 10 头 = 缺失
        }
        obs.push(((*h).to_string(), fp_of(h) ^ 0xA5A5A5A5)); // 后 10 头全漂移
    }
    // 多余 10 头。
    for k in 0..10usize {
        obs.push((format!("extra{}.h", k), fp_of(&format!("extra{}.h", k))));
    }
    (obs, 10, 10, 10)
}

/// 注入核：30 条注入逐条归位 + 4 类守卫必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 三分类逐条归位（10+10+10）。
    let base = baseline();
    let (obs, m, d, x) = injected_observed();
    let cls = classify(&base, &obs);
    let n_missing = cls.iter().filter(|e| e.class == RfClass::Missing).count();
    let n_drift = cls.iter().filter(|e| e.class == RfClass::Drift).count();
    let n_extra = cls.iter().filter(|e| e.class == RfClass::Extra).count();
    let n_clean = cls.iter().filter(|e| e.class == RfClass::Clean).count();
    if n_missing == m && n_drift == d && n_extra == x && n_clean == 0 {
        caught += 1;
    }
    // 2) 报告登记归位（账计数 = 注入计数）。
    let mut rep = ScanReport::new();
    let mut all_ok = true;
    for e in cls.iter() {
        if rep.add(e.clone()).is_err() {
            all_ok = false;
        }
    }
    if all_ok
        && rep.count(RfClass::Missing) == 10
        && rep.count(RfClass::Drift) == 10
        && rep.count(RfClass::Extra) == 10
        && rep.anomalies() == 30
    {
        caught += 1;
    }
    // 3) 重复登记必抓。
    let dup = rep.add(cls[0].clone());
    if dup == Err(RF_DUP) {
        caught += 1;
    }
    // 4) 非期望头非 EXTRA 必抓。
    let oob = rep.add(ReportEntry {
        head: "ghost.h".to_string(),
        class: RfClass::Clean,
        expect_fp: 0,
        actual_fp: 1,
    });
    if oob == Err(RF_OOB) {
        caught += 1;
    }
    // 5) 期望头标 EXTRA 必抓。
    let badcls = rep.add(ReportEntry {
        head: "stdio.h".to_string(),
        class: RfClass::Extra,
        expect_fp: 0,
        actual_fp: 0,
    });
    if badcls == Err(RF_BAD_CLASS) {
        caught += 1;
    }
    // 6) 封账改写必抓。
    rep.seal();
    if rep.add(cls[1].clone()) == Err(RF_SEALED) {
        caught += 1;
    }
    // 7) 空报告导出拒 + 非空报告导出行数核。
    let empty = ScanReport::new();
    if render_report(&empty) == Err(RF_EMPTY) {
        caught += 1;
    }
    if let Ok(txt) = render_report(&rep) {
        let lines = txt.lines().count();
        // 头 1 + 三分段 3 + 异常明细 30 + CLEAN 汇总 1 = 35。
        if lines == 35 && txt.contains("[MISSING] n=10") && txt.contains("[DRIFT] n=10")
            && txt.contains("[EXTRA] n=10")
        {
            caught += 1;
        }
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx46430_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46430");
    // J1-1 三分类注入 ×30 逐条正确归类。
    let base = baseline();
    let (obs, m, d, x) = injected_observed();
    let cls = classify(&base, &obs);
    let n_m = cls.iter().filter(|e| e.class == RfClass::Missing).count();
    let n_d = cls.iter().filter(|e| e.class == RfClass::Drift).count();
    let n_x = cls.iter().filter(|e| e.class == RfClass::Extra).count();
    let ok = n_m == m && n_d == d && n_x == x && cls.len() == 30;
    s.add("46430-J1-三分类归位", ok, if ok { "" } else { "归类漂移" });
    // J1-2 全量无注入 = 全 CLEAN。
    let full_obs: Vec<(String, u32)> = RF_HEADS
        .iter()
        .map(|h| ((*h).to_string(), fp_of(h)))
        .collect();
    let cls2 = classify(&base, &full_obs);
    let ok = cls2.len() == 20 && cls2.iter().all(|e| e.class == RfClass::Clean);
    s.add("46430-J1-无注入全净", ok, if ok { "" } else { "全净漂移" });
    // J1-3 账计数与异常总数。
    let mut rep = ScanReport::new();
    for e in cls.iter() {
        rep.add(e.clone()).expect("report add");
    }
    let ok = rep.anomalies() == 30
        && rep.count(RfClass::Clean) == 0
        && rep.entries.len() == 30;
    s.add("46430-J1-账计数", ok, if ok { "" } else { "账计数漂移" });
    // J1-4 可导出：行数与分段齐全。
    let txt = render_report(&rep).unwrap_or_default();
    let ok = txt.lines().count() == 35
        && txt.contains("== UNX-F46430 头扫描报告 ==")
        && txt.contains("[CLEAN] n=0");
    s.add("46430-J1-可导出", ok, if ok { "" } else { "导出漂移" });
    // J1-5 分类码闭环（code/from_code 往返）。
    let ok = (0..4).all(|c| RfClass::from_code(c).map(|k| k.code() == c).unwrap_or(false))
        && RfClass::from_code(9) == Err(RF_BAD_CLASS);
    s.add("46430-J1-分类码闭环", ok, if ok { "" } else { "分类码漂移" });
    // J1-6 报告指纹双算稳定。
    let ok = rep.fingerprint() == rep.fingerprint();
    s.add("46430-J1-指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 7/7。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46430-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46430_all_checks_green() {
        let set = run_unx46430_checks();
        assert!(set.all_passed(), "UNX-F46430 checks must be green: {}", set.red_items_text());
    }
}
