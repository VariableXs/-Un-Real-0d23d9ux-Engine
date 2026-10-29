//! UNX-F46416 告警账归档与查询面（混装告警可过滤、可检索、可导出）（0x400F0 段）
//!
//! 锚点：UNX-F46416-J1 告警 ×50 条目全落账可检索，过滤/检索/导出三面全通。
//! 域：CoRun·UNX-S1-B01（工具链域·告警查询面）。
//!
//! 结构：告警归档账（50 条目：组件 × 事件 × 级别 × 波次四元组）+ 过滤面
//! （按组件/级别/波次三维过滤器）+ 检索面（按序号定位 + 按指纹验真）+
//! 导出面（三列渲染 schema）+ 注入核（空条目/越级别/越波次/封账改写/
//! 检索越界必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x400F0 独占 0x10）。
pub const SEG_BASE: u32 = 0x400F0;
pub const AR_EMPTY: u32 = SEG_BASE + 0x01; // 空条目
pub const AR_LEVEL_OOB: u32 = SEG_BASE + 0x02; // 级别越闭集
pub const AR_WAVE_OOB: u32 = SEG_BASE + 0x03; // 波次越域（0 拒）
pub const AR_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const AR_IDX_OOB: u32 = SEG_BASE + 0x05; // 检索越界

/// 级别三档闭集。
pub const LV_INFO: u8 = 0;
pub const LV_WARN: u8 = 1;
pub const LV_ERROR: u8 = 2;
pub const LV_COUNT: usize = 3;

pub const LEVEL_NAMES: [&str; LV_COUNT] = ["info", "warn", "error"];

/// 告警条目：四元组。
#[derive(Clone)]
pub struct AlarmEntry {
    pub comp: String,
    pub event: String,
    pub level: u8,
    pub wave: u32,
}

impl AlarmEntry {
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for b in format!("{}|{}|{}|{}", self.comp, self.event, self.level, self.wave).as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 告警归档账。
pub struct AlarmArchive {
    pub rows: Vec<AlarmEntry>,
    pub sealed: bool,
}

impl AlarmArchive {
    pub fn new() -> Self {
        AlarmArchive { rows: Vec::new(), sealed: false }
    }

    /// 落账（四元组逐字段核 + 封账拒写）。
    pub fn put(&mut self, comp: &str, event: &str, level: u8, wave: u32) -> Result<(), u32> {
        if comp.is_empty() || event.is_empty() {
            return Err(AR_EMPTY);
        }
        if level as usize >= LV_COUNT {
            return Err(AR_LEVEL_OOB);
        }
        if wave == 0 {
            return Err(AR_WAVE_OOB);
        }
        if self.sealed {
            return Err(AR_SEALED);
        }
        self.rows.push(AlarmEntry {
            comp: comp.to_string(),
            event: event.to_string(),
            level,
            wave,
        });
        Ok(())
    }

    /// 过滤面：三维过滤器（组件可选 / 级别可选 / 波次可选，None = 不过滤）。
    pub fn filter(&self, comp: Option<&str>, level: Option<u8>, wave: Option<u32>) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            let comp_hit = match comp {
                None => true,
                Some(c) => r.comp == c,
            };
            let level_hit = match level {
                None => true,
                Some(l) => r.level == l,
            };
            let wave_hit = match wave {
                None => true,
                Some(w) => r.wave == w,
            };
            if comp_hit && level_hit && wave_hit {
                out.push(i);
            }
        }
        out
    }

    /// 检索面：按序号定位（越界显性拒绝）。
    pub fn at(&self, idx: usize) -> Result<&AlarmEntry, u32> {
        self.rows.get(idx).ok_or(AR_IDX_OOB)
    }

    /// 指纹验真：序号定位条目指纹与外部给定指纹一致。
    pub fn verify(&self, idx: usize, fp: u32) -> Result<bool, u32> {
        Ok(self.at(idx)?.fingerprint() == fp)
    }

    /// 导出面：三列渲染（序号|级别|事件）。
    pub fn render(&self, idxs: &[usize]) -> String {
        let mut out = String::new();
        for i in idxs.iter() {
            if let Some(r) = self.rows.get(*i) {
                out.push_str(&format!("{}|{}|{}\n", i, LEVEL_NAMES[r.level as usize], r.event));
            }
        }
        out
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：50 条目（gcc ×10 warn、clang ×10 info、rustc ×10 error、
/// git ×10 warn、cmake ×10 info；波次 1~10 轮转）。
pub fn standard_archive() -> AlarmArchive {
    let mut a = AlarmArchive::new();
    let comps = ["gcc", "clang", "rustc", "git", "cmake"];
    let levels = [LV_WARN, LV_INFO, LV_ERROR, LV_WARN, LV_INFO];
    for c in 0..5usize {
        for i in 0..10usize {
            a.put(comps[c], &format!("{} 告警事件 {}", comps[c], i + 1), levels[c], (i as u32 % 10) + 1)
                .expect("fixture alarm");
        }
    }
    a
}

/// 注入核：6 注入必抓（空条目/越级别/越波次/封账改写/检索越界/过滤空集显性）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空条目必抓
    let mut a = AlarmArchive::new();
    if a.put("", "事件", LV_INFO, 1) == Err(AR_EMPTY) {
        caught += 1;
    }
    // 2) 越级别必抓
    if a.put("gcc", "事件", 9, 1) == Err(AR_LEVEL_OOB) {
        caught += 1;
    }
    // 3) 越波次必抓
    if a.put("gcc", "事件", LV_INFO, 0) == Err(AR_WAVE_OOB) {
        caught += 1;
    }
    // 4) 封账改写必抓
    let mut std_a = standard_archive();
    std_a.seal();
    if std_a.put("gcc", "迟到告警", LV_WARN, 1) == Err(AR_SEALED) {
        caught += 1;
    }
    // 5) 检索越界必抓
    if std_a.at(50).err() == Some(AR_IDX_OOB) {
        caught += 1;
    }
    // 6) 过滤空集显性（不存在的组件 → 0 命中，非 panic）
    let none = std_a.filter(Some("nosuch"), None, None);
    if none.is_empty() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46416_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46416");
    // J1-1 ×50 条目全落账
    let a = standard_archive();
    let ok = a.rows.len() == 50;
    s.add("46416-J1-×50条目落账", ok, if ok { "" } else { "条目不足" });
    // J1-2 过滤面：组件过滤 gcc → 10 条；级别过滤 error → 10 条；波次过滤 5 → 5 条
    let by_comp = a.filter(Some("gcc"), None, None);
    let by_level = a.filter(None, Some(LV_ERROR), None);
    let by_wave = a.filter(None, None, Some(5));
    let ok = by_comp.len() == 10 && by_level.len() == 10 && by_wave.len() == 5;
    s.add("46416-J1-三面过滤", ok, if ok { "" } else { "过滤漂移" });
    // J1-3 检索面：序号定位 + 指纹验真
    let fp = a.at(3).expect("at3").fingerprint();
    let ok = a.verify(3, fp) == Ok(true) && a.verify(3, 0) == Ok(false);
    s.add("46416-J1-检索验真", ok, if ok { "" } else { "检索漂移" });
    // J1-4 导出面：50 行渲染齐
    let all: Vec<usize> = (0..50).collect();
    let text = a.render(&all);
    let ok = text.lines().count() == 50 && text.starts_with("0|warn|");
    s.add("46416-J1-导出三列", ok, if ok { "" } else { "导出漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46416-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46416_all_checks_green() {
        let set = run_unx46416_checks();
        assert!(set.all_passed(), "UNX-F46416 checks must be green: {}", set.red_items_text());
    }
}
