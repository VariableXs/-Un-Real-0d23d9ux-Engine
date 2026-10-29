//! UNX-F56099 管线端到端冒烟样例账（五指标齐套干跑）（0x60C40 段）
//!
//! 锚点：UNX-F56099-J1 冒烟样例全量（五指标干跑逐项落账：指标 × 干跑值
//! × 结论三元组）；J2 齐套语义（五指标全干跑才判冒烟通过，缺项即拒）；
//! J3 冒烟审计链（指标指纹链、封账只读、结论闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·冒烟面）。
//!
//! 结构：五指标闭集（rss/avail/io/fps/latency）+ 干跑登记册（指标 →
//! 干跑值 × 结论逐项落账）+ 干跑值域闸（逐指标物理界）+ 齐套收官语义
//! + 指纹链 + 注入核（未知指标/干跑越界/缺项收官/重复落账/封账改写/
//! 空套查询/结论漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60C40 独占 0x20）。
pub const SEG_BASE: u32 = 0x60C40;
pub const SM_UNK_METRIC: u32 = SEG_BASE + 0x01; // 未知指标（五闭集外）
pub const SM_VAL_RANGE: u32 = SEG_BASE + 0x02; // 干跑值越物理界
pub const SM_SEAL_SHORT: u32 = SEG_BASE + 0x03; // 缺项收官
pub const SM_DUP: u32 = SEG_BASE + 0x04; // 重复落账
pub const SM_SEALED: u32 = SEG_BASE + 0x05; // 冒烟账已封账拒写
pub const SM_EMPTY: u32 = SEG_BASE + 0x06; // 空套查询
pub const SM_VERDICT_BAD: u32 = SEG_BASE + 0x07; // 结论漂移（闭集外）

/// 五指标闭集。
pub const M_RSS: usize = 0;
pub const M_AVAIL: usize = 1;
pub const M_IO: usize = 2;
pub const M_FPS: usize = 3;
pub const M_LATENCY: usize = 4;
pub const M_COUNT: usize = 5;
pub const M_NAMES: [&str; M_COUNT] = ["rss_kb", "avail_kb", "io_iops", "fps_avg", "latency_ms"];

/// 干跑值物理界（逐指标 [min, max]）。
pub const VAL_RANGES: [(u64, u64); M_COUNT] = [
    (1_024, 134_217_728),
    (1_024, 268_435_456),
    (1, 10_000_000),
    (1, 1_000),
    (0, 10_000),
];

/// 结论闭集。
pub const R_OK: usize = 0;
pub const R_WARN: usize = 1;
pub const R_COUNT: usize = 2;
pub const R_NAMES: [&str; R_COUNT] = ["ok", "warn"];

/// 干跑值域核。
pub fn value_in_range(metric: usize, val: u64) -> bool {
    if metric >= M_COUNT {
        return false;
    }
    val >= VAL_RANGES[metric].0 && val <= VAL_RANGES[metric].1
}

/// 冒烟账目：指标 × 干跑值 × 结论。
#[derive(Clone)]
pub struct SmokeEntry {
    pub metric: usize,
    pub val: u64,
    pub verdict: usize,
}

/// 冒烟总账。
pub struct SmokeBook {
    pub entries: [Option<SmokeEntry>; M_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl SmokeBook {
    pub fn new() -> Self {
        SmokeBook { entries: [None, None, None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 干跑落账：指标闭集 + 值域闸 + 结论闭集 + 重复拒四闸。
    pub fn run(&mut self, metric: usize, val: u64, verdict: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SM_SEALED);
        }
        if metric >= M_COUNT {
            return Err(SM_UNK_METRIC);
        }
        if !value_in_range(metric, val) {
            return Err(SM_VAL_RANGE);
        }
        if verdict >= R_COUNT {
            return Err(SM_VERDICT_BAD);
        }
        if self.entries[metric].is_some() {
            return Err(SM_DUP);
        }
        self.audit.push(format!("run|{}|{}|{}", M_NAMES[metric], val, R_NAMES[verdict]));
        self.entries[metric] = Some(SmokeEntry { metric, val, verdict });
        Ok(())
    }

    /// 查询指标结论（未干跑显性拒绝）。
    pub fn verdict_of(&self, metric: usize) -> Result<(u64, usize), u32> {
        if metric >= M_COUNT {
            return Err(SM_UNK_METRIC);
        }
        match &self.entries[metric] {
            None => Err(SM_EMPTY),
            Some(e) => Ok((e.val, e.verdict)),
        }
    }

    /// J1：齐套全量（五指标逐项干跑）。
    pub fn kit_ok(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
    }

    /// J2：冒烟通过（五项全 ok 才通过）。
    pub fn passed(&self) -> bool {
        if !self.kit_ok() {
            return false;
        }
        for e in self.entries.iter() {
            if let Some(x) = e {
                if x.verdict != R_OK {
                    return false;
                }
            }
        }
        true
    }

    /// 已干跑项数。
    pub fn done_count(&self) -> usize {
        let mut n = 0;
        for e in self.entries.iter() {
            if e.is_some() {
                n += 1;
            }
        }
        n
    }

    /// J3：审计链完整（链长 = 干跑项数）。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.len() == self.done_count()
            && self.audit.iter().all(|a| a.starts_with("run|"))
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(SM_SEALED);
        }
        if !self.kit_ok() {
            return Err(SM_SEAL_SHORT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 冒烟指纹（逐项复合）。
    pub fn smoke_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for m in 0..M_COUNT {
            if let Some(e) = &self.entries[m] {
                let line = format!("{}|{}|{}", M_NAMES[m], e.val, R_NAMES[e.verdict]);
                for b in line.as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }
}

/// 标准夹具：五指标全干跑全 ok。
pub fn standard_book() -> SmokeBook {
    let mut b = SmokeBook::new();
    let plan: [(usize, u64, usize); 5] = [
        (M_RSS, 51_200, R_OK),
        (M_AVAIL, 1_048_576, R_OK),
        (M_IO, 12_345, R_OK),
        (M_FPS, 120, R_OK),
        (M_LATENCY, 2, R_OK),
    ];
    for p in plan.iter() {
        if b.run(p.0, p.1, p.2).is_err() {
            panic!("standard fixture run must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知指标/干跑越界/结论漂移/重复落账/缺项收官/封账改写/空套查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知指标
    let mut b = SmokeBook::new();
    if b.run(9, 100, R_OK) == Err(SM_UNK_METRIC) {
        caught += 1;
    }
    // 2) 干跑越界（fps 0 越下界）
    if b.run(M_FPS, 0, R_OK) == Err(SM_VAL_RANGE) {
        caught += 1;
    }
    // 3) 结论漂移
    if b.run(M_FPS, 60, 9) == Err(SM_VERDICT_BAD) {
        caught += 1;
    }
    // 4) 重复落账
    if b.run(M_FPS, 60, R_OK).is_ok() && b.run(M_FPS, 90, R_OK) == Err(SM_DUP) {
        caught += 1;
    }
    // 5) 缺项收官
    if b.seal() == Err(SM_SEAL_SHORT) {
        caught += 1;
    }
    // 6) 空套查询
    if SmokeBook::new().verdict_of(M_RSS).err() == Some(SM_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.run(M_RSS, 60_000, R_OK) == Err(SM_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 冒烟账渲染导出（逐项一行）。
pub fn render_book(b: &SmokeBook) -> String {
    let mut out = String::new();
    for m in 0..M_COUNT {
        match &b.entries[m] {
            None => out.push_str(&format!("{}|UNSET\n", M_NAMES[m])),
            Some(e) => out.push_str(&format!("{}|{}|{}\n", M_NAMES[m], e.val, R_NAMES[e.verdict])),
        }
    }
    out
}

/// 渲染完整性核：五行齐 + 全干跑无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != M_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for m in 0..M_COUNT {
        if !text.contains(M_NAMES[m]) {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56099_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56099");
    // J1-1 冒烟样例全量（五指标逐项干跑）
    let b = standard_book();
    let ok = b.kit_ok() && b.done_count() == M_COUNT && !b.sealed;
    s.add("56099-J1-冒烟样例全量", ok, if ok { "" } else { "指标缺项" });
    // J1-2 指标闭集齐
    let ok = M_NAMES.len() == M_COUNT && M_NAMES[4] == "latency_ms";
    s.add("56099-J1-指标闭集齐", ok, if ok { "" } else { "指标漂移" });
    // J1-3 值域边界自证（逐指标 min/max 在界内）
    let mut ok = true;
    for m in 0..M_COUNT {
        if !value_in_range(m, VAL_RANGES[m].0) || !value_in_range(m, VAL_RANGES[m].1) {
            ok = false;
        }
    }
    s.add("56099-J1-值域边界自证", ok, if ok { "" } else { "值域漂移" });
    // J2-1 冒烟通过（五项全 ok）
    let ok = b.passed();
    s.add("56099-J2-冒烟通过", ok, if ok { "" } else { "冒烟语义漂移" });
    // J2-2 warn 不通过
    let mut b2 = standard_book();
    if let Some(e) = b2.entries[M_LATENCY].as_mut() {
        e.verdict = R_WARN;
    }
    let ok = !b2.passed();
    s.add("56099-J2-warn不通过", ok, if ok { "" } else { "warn 穿透" });
    // J2-3 干跑越界拒
    let ok = b2.run(M_RSS, 0, R_OK) == Err(SM_VAL_RANGE);
    s.add("56099-J2-越界拒", ok, if ok { "" } else { "值域闸失效" });
    // J2-4 缺项收官拒
    let mut b3 = SmokeBook::new();
    let _ = b3.run(M_RSS, 51_200, R_OK);
    let ok = b3.seal() == Err(SM_SEAL_SHORT);
    s.add("56099-J2-缺项收官拒", ok, if ok { "" } else { "缺项穿透" });
    // J2-5 齐套收官放行
    let mut b4 = standard_book();
    let ok = b4.seal().is_ok() && b4.sealed;
    s.add("56099-J2-齐套收官", ok, if ok { "" } else { "收官漂移" });
    // J3-1 审计链完整（链长 5）
    let ok = b.audit_ok() && b.audit.len() == 5;
    s.add("56099-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 冒烟指纹稳定
    let ok = standard_book().smoke_fingerprint() == standard_book().smoke_fingerprint();
    s.add("56099-J3-冒烟指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（改一项值指纹变）
    let fp0 = standard_book().smoke_fingerprint();
    let mut b5 = standard_book();
    if let Some(e) = b5.entries[M_FPS].as_mut() {
        e.val = 119;
    }
    let ok = b5.smoke_fingerprint() != fp0;
    s.add("56099-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let ok = b4.run(M_RSS, 60_000, R_OK) == Err(SM_SEALED);
    s.add("56099-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐无 UNSET
    let text = render_book(&standard_book());
    let ok = render_complete(&text, true) && text.contains("io_iops|12345|ok");
    s.add("56099-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56099-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56099_all_checks_green() {
        let set = run_unx56099_checks();
        assert!(set.all_passed(), "UNX-F56099 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56099_fingerprint_stable() {
        assert_eq!(standard_book().smoke_fingerprint(), standard_book().smoke_fingerprint());
    }
}
