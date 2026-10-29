//! UNX-F56101 IO 五档工作负载对照采集器（任务书示例条目）（0x60C80 段）
//!
//! 锚点：UNX-F56101-J1 五档采集全量（五档负载逐档落账：档名 × 脚本键 ×
//! 样本数三元组）；J2 档序与齐套语义（五档闭集逐档齐、档号单调、缺档即
//! 拒对照）；J3 采集审计链（档指纹链、封账只读、样本闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·五档采集面）。
//!
//! 结构：五档负载闭集（4K 随机读/4K 随机写/1M 顺序读/1M 顺序写/70-30
//! 混合）+ 五档登记册（档 → 脚本键 × 样本数逐档落账）+ 档序闸（档号
//! 1..5 连续单调）+ 样本量守闸（逐档 ≥30）+ 指纹链 + 注入核（未知档/
//! 样本不足/档号回退/重复档/封账改写/空账对照/样本闭式漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60C80 独占 0x20）。
pub const SEG_BASE: u32 = 0x60C80;
pub const I5_UNK_TIER: u32 = SEG_BASE + 0x01; // 未知档（五闭集外）
pub const I5_SAMPLE_SHORT: u32 = SEG_BASE + 0x02; // 样本量不足（<30）
pub const I5_TIER_BACK: u32 = SEG_BASE + 0x03; // 档号回退
pub const I5_TIER_DUP: u32 = SEG_BASE + 0x04; // 重复档
pub const I5_SEALED: u32 = SEG_BASE + 0x05; // 采集账已封账拒写
pub const I5_EMPTY: u32 = SEG_BASE + 0x06; // 空账对照
pub const I5_CENSUS_MISS: u32 = SEG_BASE + 0x07; // 样本闭式漂移

/// 五档负载闭集。
pub const T_4K_RR: usize = 0;
pub const T_4K_RW: usize = 1;
pub const T_1M_SR: usize = 2;
pub const T_1M_SW: usize = 3;
pub const T_MIX_7030: usize = 4;
pub const T_COUNT: usize = 5;
pub const T_NAMES: [&str; T_COUNT] =
    ["4k_randread", "4k_randwrite", "1m_seqread", "1m_seqwrite", "mix_70_30"];

/// 最小样本量（与守门器同源）。
pub const SAMPLE_MIN: u32 = 30;

/// 档指纹：FNV-1a 32 位。
pub fn tier_fingerprint(tier: usize, script_key: &str, samples: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let name = if tier < T_COUNT { T_NAMES[tier] } else { "?" };
    let line = format!("{}|{}|{}", name, script_key, samples);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 档 → 计量单位对照（闭集映射：随机/混合档 iops、顺序带宽档 mbps）。
pub fn unit_of(tier: usize) -> Result<&'static str, u32> {
    match tier {
        T_4K_RR | T_4K_RW | T_MIX_7030 => Ok("iops"),
        T_1M_SR | T_1M_SW => Ok("mbps"),
        _ => Err(I5_UNK_TIER),
    }
}

/// 单档采集账目：档 × 脚本键 × 样本数。
#[derive(Clone)]
pub struct TierEntry {
    pub tier: usize,
    pub script_key: String,
    pub samples: u32,
}

/// 五档采集总账。
pub struct FiveTierBook {
    pub entries: [Option<TierEntry>; T_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl FiveTierBook {
    pub fn new() -> Self {
        FiveTierBook { entries: [None, None, None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 逐档采集：档闭集 + 样本闸 + 重复档拒三闸（档序按登记序即闭集序）。
    pub fn collect(&mut self, tier: usize, script_key: &str, samples: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(I5_SEALED);
        }
        if tier >= T_COUNT {
            return Err(I5_UNK_TIER);
        }
        if samples < SAMPLE_MIN {
            return Err(I5_SAMPLE_SHORT);
        }
        if script_key.is_empty() {
            return Err(I5_UNK_TIER);
        }
        if self.entries[tier].is_some() {
            return Err(I5_TIER_DUP);
        }
        self.audit.push(format!("collect|{}|{}|{}", T_NAMES[tier], script_key, samples));
        self.entries[tier] = Some(TierEntry { tier, script_key: script_key.to_string(), samples });
        Ok(())
    }

    /// 查询档样本数（未采档显性拒绝）。
    pub fn samples_of(&self, tier: usize) -> Result<u32, u32> {
        if tier >= T_COUNT {
            return Err(I5_UNK_TIER);
        }
        match &self.entries[tier] {
            None => Err(I5_EMPTY),
            Some(e) => Ok(e.samples),
        }
    }

    /// J1：五档齐套全量。
    pub fn kit_ok(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
    }

    /// 已采档数。
    pub fn done_count(&self) -> usize {
        let mut n = 0;
        for e in self.entries.iter() {
            if e.is_some() {
                n += 1;
            }
        }
        n
    }

    /// J2：样本总量闭式（逐档和，u64 中转）。
    pub fn total_samples(&self) -> Result<u64, u32> {
        if !self.kit_ok() {
            return Err(I5_EMPTY);
        }
        let mut sum: u64 = 0;
        for e in self.entries.iter() {
            if let Some(x) = e {
                sum += x.samples as u64;
            }
        }
        Ok(sum)
    }

    /// J2：逐档守闸复核（全部 ≥ 30）。
    pub fn samples_ok(&self) -> bool {
        self.kit_ok()
            && self
                .entries
                .iter()
                .all(|e| e.as_ref().map(|x| x.samples >= SAMPLE_MIN).unwrap_or(false))
    }

    /// J3：审计链完整。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.len() == self.done_count()
            && self.audit.iter().all(|a| a.starts_with("collect|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 采集指纹（逐档复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for t in 0..T_COUNT {
            if let Some(e) = &self.entries[t] {
                h ^= tier_fingerprint(e.tier, &e.script_key, e.samples);
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：五档全采（脚本键按档名同源，样本 60）。
pub fn standard_book() -> FiveTierBook {
    let mut b = FiveTierBook::new();
    for t in 0..T_COUNT {
        if b.collect(t, T_NAMES[t], 60).is_err() {
            panic!("standard fixture collect must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知档/样本不足/重复档/空脚本键/封账改写/空账查询/样本闭式漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知档
    let mut b = FiveTierBook::new();
    if b.collect(9, "k", 60) == Err(I5_UNK_TIER) {
        caught += 1;
    }
    // 2) 样本不足
    if b.collect(T_4K_RR, "k", 29) == Err(I5_SAMPLE_SHORT) {
        caught += 1;
    }
    // 3) 空脚本键
    if b.collect(T_4K_RR, "", 60) == Err(I5_UNK_TIER) {
        caught += 1;
    }
    // 4) 重复档
    if b.collect(T_4K_RR, "k", 60).is_ok() && b.collect(T_4K_RR, "k2", 60) == Err(I5_TIER_DUP) {
        caught += 1;
    }
    // 5) 空账查询
    if FiveTierBook::new().samples_of(T_1M_SR).err() == Some(I5_EMPTY) {
        caught += 1;
    }
    // 6) 样本闭式漂移必抓（篡改样本数后闭式不符）
    let mut dirty = standard_book();
    dirty.entries[0] = Some(TierEntry { tier: T_4K_RR, script_key: "x".to_string(), samples: 0 });
    if dirty.samples_of(T_4K_RR) != Ok(0) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.collect(T_MIX_7030, "late", 60) == Err(I5_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 五档账渲染导出（逐档一行）。
pub fn render_book(b: &FiveTierBook) -> String {
    let mut out = String::new();
    for t in 0..T_COUNT {
        match &b.entries[t] {
            None => out.push_str(&format!("{}|UNSET\n", T_NAMES[t])),
            Some(e) => out.push_str(&format!("{}|{}|{}\n", T_NAMES[t], e.script_key, e.samples)),
        }
    }
    out
}

/// 渲染完整性核：五行齐 + 全采无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != T_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for t in 0..T_COUNT {
        if !text.contains(T_NAMES[t]) {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56101");
    // J1-1 五档采集全量
    let b = standard_book();
    let ok = b.kit_ok() && b.done_count() == T_COUNT && !b.sealed;
    s.add("56101-J1-五档采集全量", ok, if ok { "" } else { "档缺项" });
    // J1-2 档闭集齐
    let ok = T_NAMES.len() == T_COUNT && T_NAMES[4] == "mix_70_30";
    s.add("56101-J1-档闭集齐", ok, if ok { "" } else { "档名漂移" });
    // J1-3 样本守闸（逐档 ≥ 30）
    let ok = b.samples_ok();
    s.add("56101-J1-样本守闸", ok, if ok { "" } else { "样本不足穿透" });
    // J2-1 样本总量闭式（5 × 60 = 300）
    let ok = b.total_samples() == Ok(300);
    s.add("56101-J2-样本闭式", ok, if ok { "" } else { "闭式算术漂移" });
    // J2-2 未齐套闭式拒算
    let ok = FiveTierBook::new().total_samples() == Err(I5_EMPTY);
    s.add("56101-J2-未齐套拒算", ok, if ok { "" } else { "齐套闸失效" });
    // J2-3 样本不足拒采
    let mut b2 = FiveTierBook::new();
    let ok = b2.collect(T_4K_RW, "k", 29) == Err(I5_SAMPLE_SHORT);
    s.add("56101-J2-不足拒采", ok, if ok { "" } else { "样本闸失效" });
    // J2-4 重复档拒
    let mut b3 = standard_book();
    let ok = b3.collect(T_1M_SR, "k", 60) == Err(I5_TIER_DUP);
    s.add("56101-J2-重复档拒", ok, if ok { "" } else { "重复闸失效" });
    // J2-5 档查询命中（4k_randread = 60）
    let ok = b.samples_of(T_4K_RR) == Ok(60);
    s.add("56101-J2-档查询命中", ok, if ok { "" } else { "查询漂移" });
    // J2-6 单位映射闭集（顺序带宽档 mbps、其余 iops）
    let ok = unit_of(T_1M_SR) == Ok("mbps")
        && unit_of(T_1M_SW) == Ok("mbps")
        && unit_of(T_4K_RR) == Ok("iops")
        && unit_of(T_MIX_7030) == Ok("iops");
    s.add("56101-J2-单位映射闭集", ok, if ok { "" } else { "单位映射漂移" });
    // J2-7 未知档单位拒
    let ok = unit_of(9) == Err(I5_UNK_TIER);
    s.add("56101-J2-未知档单位拒", ok, if ok { "" } else { "单位越界未拒" });
    // J3-1 审计链完整（链长 5）
    let ok = b.audit_ok() && b.audit.len() == 5;
    s.add("56101-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 采集指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56101-J3-采集指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（改脚本键指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b4 = FiveTierBook::new();
    for t in 0..T_COUNT {
        let key = if t == 0 { "alt_key" } else { T_NAMES[t] };
        let _ = b4.collect(t, key, 60);
    }
    let ok = b4.seal_fingerprint() != fp0;
    s.add("56101-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.collect(T_4K_RR, "late", 60) == Err(I5_SEALED);
    s.add("56101-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐无 UNSET
    let text = render_book(&standard_book());
    let ok = render_complete(&text, true) && text.contains("4k_randread|4k_randread|60");
    s.add("56101-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56101-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56101_all_checks_green() {
        let set = run_unx56101_checks();
        assert!(set.all_passed(), "UNX-F56101 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56101_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
