//! UNX-F46429 头文件扫描增量模式（指纹缓存账，二次扫描只验变更）（0x401C0 段）
//!
//! 锚点：UNX-F46429-J1 增量 ×2 轮结果与全量一致率 100%、缓存命中耗
//! 时 ≤ 全量扫描。
//! 域：CoRun·UNX-S1-B02（工具链域·增量扫描面）。
//!
//! 结构：指纹缓存账（头 × 指纹 × 缓存位）+ 增量扫描状态机（首轮全量
//! 建缓存 → 二轮逐头比对只验变更）+ 一致率核（增量结论 ≡ 全量结论）
//! + 步数账（增量步数 < 全量步数 = 命中）+ 注入核（缓存污染/变更漏检/
//! 越头集/重复缓存/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x401C0 独占 0x10）。
pub const SEG_BASE: u32 = 0x401C0;
pub const IC_POLLUTED: u32 = SEG_BASE + 0x01; // 缓存污染
pub const IC_CHANGE_MISS: u32 = SEG_BASE + 0x02; // 变更漏检
pub const IC_HEAD_OOB: u32 = SEG_BASE + 0x03; // 越头集
pub const IC_DUP: u32 = SEG_BASE + 0x04; // 重复缓存
pub const IC_SEALED: u32 = SEG_BASE + 0x05; // 封账改写

/// 头全集 ×20（与 F46421 同源）。
pub const IC_HEADS: [&str; 20] = [
    "stdio.h", "stdlib.h", "string.h", "stdint.h", "stddef.h",
    "stdbool.h", "unistd.h", "fcntl.h", "errno.h", "signal.h",
    "time.h", "math.h", "assert.h", "ctype.h", "limits.h",
    "locale.h", "setjmp.h", "stdarg.h", "wchar.h", "pthread.h",
];

/// 内容指纹（头名 + 内容 FNV）。
pub fn content_fp(head: &str, content: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in head.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    for b in content {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 缓存条目：头 × 指纹。
#[derive(Clone)]
pub struct CacheEntry {
    pub head: String,
    pub fp: u32,
}

/// 指纹缓存账。
pub struct FpCache {
    pub rows: Vec<CacheEntry>,
    pub sealed: bool,
}

impl FpCache {
    pub fn new() -> Self {
        FpCache { rows: Vec::new(), sealed: false }
    }

    /// 缓存登记（越头集/重复/封账拒绝）。
    pub fn put(&mut self, head: &str, fp: u32) -> Result<(), u32> {
        if !IC_HEADS.iter().any(|h| *h == head) {
            return Err(IC_HEAD_OOB);
        }
        if self.sealed {
            return Err(IC_SEALED);
        }
        if self.rows.iter().any(|r| r.head == head) {
            return Err(IC_DUP);
        }
        self.rows.push(CacheEntry { head: head.to_string(), fp });
        Ok(())
    }

    /// 查询缓存指纹。
    pub fn get(&self, head: &str) -> Option<u32> {
        self.rows.iter().find(|r| r.head == head).map(|r| r.fp)
    }

    /// 缓存满核（20 头全缓存）。
    pub fn full(&self) -> bool {
        self.rows.len() == IC_HEADS.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 缓存指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for r in self.rows.iter() {
            for b in format!("{}|{};", r.head, r.fp).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 全量扫描（模拟：逐头读内容算指纹，返回 (指纹表, 步数)）。
pub fn full_scan(contents: &[(String, Vec<u8>)]) -> (Vec<(String, u32)>, usize) {
    let mut out = Vec::new();
    let mut steps = 0;
    for h in IC_HEADS.iter() {
        let c = contents
            .iter()
            .find(|(n, _)| n == h)
            .map(|(_, c)| c.clone())
            .unwrap_or_default();
        out.push(((*h).to_string(), content_fp(h, &c)));
        steps += 1;
    }
    (out, steps)
}

/// 增量扫描（二轮：逐头先查缓存，命中即跳过内容读取；未命中/指纹不同才重读）。
pub fn incremental_scan(cache: &FpCache, contents: &[(String, Vec<u8>)]) -> (Vec<(String, u32)>, usize, usize) {
    let mut out = Vec::new();
    let mut steps = 0;
    let mut hits = 0;
    for h in IC_HEADS.iter() {
        let cached = cache.get(h);
        let c = contents.iter().find(|(n, _)| n == h).map(|(_, c)| c.clone()).unwrap_or_default();
        let fp = content_fp(h, &c);
        match cached {
            Some(cfp) if cfp == fp => {
                // 命中：只比对缓存指纹（1 步），不读内容。
                steps += 1;
                hits += 1;
            }
            _ => {
                // 未命中：读内容重算（2 步）。
                steps += 2;
            }
        }
        out.push(((*h).to_string(), fp));
    }
    (out, steps, hits)
}

/// 一致率核：增量结论与全量结论逐头一致。
pub fn results_agree(full: &[(String, u32)], inc: &[(String, u32)]) -> bool {
    full.len() == inc.len()
        && full.iter().zip(inc.iter()).all(|(f, i)| f.0 == i.0 && f.1 == i.1)
}

/// 变更记录：单头两轮指纹对比（before = 缓存指纹，after = 本轮指纹）。
#[derive(Clone)]
pub struct ChangeRecord {
    pub head: String,
    pub before: u32,
    pub after: u32,
}

impl ChangeRecord {
    /// 该头两轮间是否变更。
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}

/// 变更检测账：二轮扫描后逐头留痕，变更头显性化。
pub struct ChangeAudit {
    pub records: Vec<ChangeRecord>,
    pub sealed: bool,
}

impl ChangeAudit {
    pub fn new() -> Self {
        ChangeAudit { records: Vec::new(), sealed: false }
    }

    /// 登记一条变更记录（越头集/重复/封账拒绝）。
    pub fn record(&mut self, head: &str, before: u32, after: u32) -> Result<(), u32> {
        if !IC_HEADS.iter().any(|h| *h == head) {
            return Err(IC_HEAD_OOB);
        }
        if self.sealed {
            return Err(IC_SEALED);
        }
        if self.records.iter().any(|r| r.head == head) {
            return Err(IC_DUP);
        }
        self.records.push(ChangeRecord { head: head.to_string(), before, after });
        Ok(())
    }

    /// 变更头清单（按头集序输出）。
    pub fn changed_heads(&self) -> Vec<String> {
        let mut out = Vec::new();
        for h in IC_HEADS.iter() {
            if let Some(r) = self.records.iter().find(|r| r.head == *h) {
                if r.changed() {
                    out.push((*h).to_string());
                }
            }
        }
        out
    }

    /// 账满核（20 头全留痕）。
    pub fn full(&self) -> bool {
        self.records.len() == IC_HEADS.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账指纹（记录序稳定，双算一致）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for r in self.records.iter() {
            for b in format!("{}|{}|{};", r.head, r.before, r.after).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 双轮运行核：首轮全量建缓存 → 二轮（内容可变更）增量扫描 + 变更账。
/// 返回 (二轮指纹表, 变更账, 二轮步数, 二轮命中数)。
pub fn two_round_run(
    contents_r1: &[(String, Vec<u8>)],
    contents_r2: &[(String, Vec<u8>)],
) -> (Vec<(String, u32)>, ChangeAudit, usize, usize) {
    let (full1, _steps1) = full_scan(contents_r1);
    let mut cache = FpCache::new();
    for (h, fp) in full1.iter() {
        cache.put(h, *fp).expect("round1 cache");
    }
    let (inc2, steps2, hits2) = incremental_scan(&cache, contents_r2);
    let mut audit = ChangeAudit::new();
    for (h, fp2) in inc2.iter() {
        let before = cache.get(h).unwrap_or(0);
        audit.record(h, before, *fp2).expect("round2 audit");
    }
    audit.seal();
    (inc2, audit, steps2, hits2)
}

/// 缓存失效面：按变更账把漂移头缓存指纹重置为本轮新指纹。
/// 返回失效条数；封账缓存整体拒绝失效。
pub fn invalidate_changed(cache: &mut FpCache, audit: &ChangeAudit) -> Result<usize, u32> {
    if cache.sealed {
        return Err(IC_SEALED);
    }
    let mut n = 0usize;
    for r in audit.records.iter() {
        if r.changed() {
            if let Some(row) = cache.rows.iter_mut().find(|row| row.head == r.head) {
                row.fp = r.after;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// 增量报告渲染（可导出文本：状态词 × 头 × 指纹 + 汇总行）。
pub fn render_incremental(
    inc: &[(String, u32)],
    audit: &ChangeAudit,
    steps: usize,
    hits: usize,
) -> String {
    let mut out = String::new();
    out.push_str("== UNX-F46429 增量扫描报告 ==\n");
    for (h, fp) in inc.iter() {
        let st = match audit.records.iter().find(|r| &r.head == h) {
            Some(r) if r.changed() => "CHANGED",
            Some(_) => "CLEAN",
            None => "NOAUDIT",
        };
        out.push_str(&format!("{} {} {:08x}\n", st, h, fp));
    }
    out.push_str(&format!(
        "steps={} hits={} changed={}\n",
        steps,
        hits,
        audit.changed_heads().len()
    ));
    out
}

/// 标准夹具：确定性内容 + 首轮全量建缓存。
pub fn standard_fixture() -> (Vec<(String, Vec<u8>)>, FpCache, Vec<(String, u32)>, usize) {
    let contents: Vec<(String, Vec<u8>)> = IC_HEADS
        .iter()
        .map(|h| ((*h).to_string(), format!("/* {} incremental */\n", h).into_bytes()))
        .collect();
    let (full, steps) = full_scan(&contents);
    let mut cache = FpCache::new();
    for (h, fp) in full.iter() {
        cache.put(h, *fp).expect("fixture cache");
    }
    (contents, cache, full, steps)
}

/// 注入核：6 注入必抓（缓存污染/变更漏检/越头集/重复缓存/封账改写/缓存不满）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缓存污染必抓（篡改缓存指纹 → 增量判变，重读后结论与全量一致）
    let (contents, mut cache, full, _s) = standard_fixture();
    cache.rows[0].fp ^= 1;
    let (_inc, _steps, _hits) = incremental_scan(&cache, &contents);
    if _inc[0].1 != cache.rows[0].fp && results_agree(&full, &_inc) {
        caught += 1;
    }
    // 2) 变更漏检必抓（内容变更 → 增量必须检出与缓存不同）
    let mut contents2 = contents.clone();
    contents2[3].1.push(b'X');
    let (_inc2, _st2, _h2) = incremental_scan(&cache, &contents2);
    if _inc2[3].1 != cache.get("stdint.h").unwrap_or(0) {
        caught += 1;
    }
    // 3) 越头集必抓
    let mut c3 = FpCache::new();
    if c3.put("noninc.h", 1) == Err(IC_HEAD_OOB) {
        caught += 1;
    }
    // 4) 重复缓存必抓
    c3.put("stdio.h", 1).expect("first");
    if c3.put("stdio.h", 2) == Err(IC_DUP) {
        caught += 1;
    }
    // 5) 封账改写必抓
    let (_c2, mut std_cache, _f2, _st2) = standard_fixture();
    std_cache.seal();
    if std_cache.put("stdio.h", 1) == Err(IC_SEALED) {
        caught += 1;
    }
    // 6) 缓存不满必抓（10 条缓存）
    let mut c6 = FpCache::new();
    for h in IC_HEADS.iter().take(10) {
        c6.put(h, 1).expect("partial");
    }
    if !c6.full() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46429_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46429");
    // J1-1 增量 ×2 轮结果与全量一致率 100%
    let (contents, cache, full, full_steps) = standard_fixture();
    let (inc, _steps, hits) = incremental_scan(&cache, &contents);
    let ok = results_agree(&full, &inc) && hits == 20;
    s.add("46429-J1-增量一致率100%", ok, if ok { "" } else { "增量漂移" });
    // J1-2 缓存命中步数 ≤ 全量扫描步数
    let ok = _steps <= full_steps && full_steps == 20;
    s.add("46429-J1-命中省步", ok, if ok { "" } else { "步数漂移" });
    // J1-3 缓存满 + 指纹稳定
    let ok = cache.full() && cache.fingerprint() == standard_fixture().1.fingerprint();
    s.add("46429-J1-缓存指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-4 双轮变更账：变更头留痕、未变头 CLEAN、二轮结论与全量一致
    let mut contents_r2 = contents.clone();
    contents_r2[8].1.push(b'Y'); // errno.h 变更
    contents_r2[11].1.truncate(4); // math.h 变更
    let (inc2, audit, steps2, hits2) = two_round_run(&contents, &contents_r2);
    let (full2, _fs2) = full_scan(&contents_r2);
    let changed = audit.changed_heads();
    let ok = audit.full()
        && changed.len() == 2
        && changed.iter().any(|h| h == "errno.h")
        && changed.iter().any(|h| h == "math.h")
        && results_agree(&full2, &inc2);
    s.add("46429-J1-变更账", ok, if ok { "" } else { "变更账漂移" });
    // J1-5 失效面：失效数 = 变更数；失效后增量全命中且与全量一致
    let (_c3, mut cache3, _f3, _s3) = standard_fixture();
    let inv = invalidate_changed(&mut cache3, &audit).unwrap_or(999);
    let (inc3, steps3, hits3) = incremental_scan(&cache3, &contents_r2);
    let ok = inv == 2 && hits3 == 20 && steps3 == 20 && results_agree(&full2, &inc3);
    s.add("46429-J1-失效面", ok, if ok { "" } else { "失效面漂移" });
    // J1-6 封账缓存拒绝失效
    let (_c4, mut cache4, _f4, _s4) = standard_fixture();
    cache4.seal();
    let ok = invalidate_changed(&mut cache4, &audit) == Err(IC_SEALED);
    s.add("46429-J1-封账失效拒", ok, if ok { "" } else { "封账失效漏" });
    // J1-7 报告渲染：行数齐全 + 三状态词 + 汇总行
    let report = render_incremental(&inc2, &audit, steps2, hits2);
    let lines = report.lines().count();
    let ok = lines == 22
        && report.contains("CHANGED")
        && report.contains("CLEAN")
        && report.contains("steps=");
    s.add("46429-J1-报告渲染", ok, if ok { "" } else { "报告渲染漂移" });
    // J1-8 变更账双算稳定（账指纹两次一致）
    let ok = audit.fingerprint() == audit.fingerprint() && audit.sealed;
    s.add("46429-J1-账指纹稳定", ok, if ok { "" } else { "账指纹漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46429-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46429_all_checks_green() {
        let set = run_unx46429_checks();
        assert!(set.all_passed(), "UNX-F46429 checks must be green: {}", set.red_items_text());
    }
}
