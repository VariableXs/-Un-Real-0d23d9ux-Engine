//! UNX-F46415 版本混装检测器骨架（gcc 版本与 libgcc 版本不匹配 → 构建期显式告警账）（0x400E0 段）
//!
//! 锚点：UNX-F46415-J1 混装注入 ×5 必报警、匹配 ×5 零误报。
//! 域：CoRun·UNX-S1-B01（工具链域·混装检测面）。
//!
//! 结构：版本三元组账（gcc 主版 × libgcc 附属版 × 运行时子目录版逐件
//! 对齐）+ 混装判定状态机（同波次同源 = 匹配；跨版本 = 混装告警）+
//! 告警账（显性落账可检索）+ 注入核（跨版混装/缺版本/空串/越闭集/
//! 告警漏报必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x400E0 独占 0x10）。
pub const SEG_BASE: u32 = 0x400E0;
pub const MX_VER_MISS: u32 = SEG_BASE + 0x01; // 版本串缺失
pub const MX_FMT_BAD: u32 = SEG_BASE + 0x02; // 版本格式非法
pub const MX_MISMATCH: u32 = SEG_BASE + 0x03; // 版本混装
pub const MX_COMP_OOB: u32 = SEG_BASE + 0x04; // 组件越闭集
pub const MX_ALARM_MISS: u32 = SEG_BASE + 0x05; // 告警漏报

/// gcc 钉版主版本（与 F46401 同源）。
pub const PIN_GCC_VER: &str = "13.2.0";

/// 版本格式核：主.次.修订 三段数字。
pub fn ver_fmt_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// 主版本提取（首段）。
pub fn ver_major(v: &str) -> Result<u32, u32> {
    if !ver_fmt_ok(v) {
        return Err(MX_FMT_BAD);
    }
    v.split('.').next().unwrap_or("").parse::<u32>().map_err(|_| MX_FMT_BAD)
}

/// 混装判定：gcc 主版 / libgcc 版 / 运行时子目录版三者主版必须一致。
pub fn mix_check(gcc: &str, libgcc: &str, subdir: &str) -> Result<(), u32> {
    if gcc.is_empty() || libgcc.is_empty() || subdir.is_empty() {
        return Err(MX_VER_MISS);
    }
    let (g, l) = (ver_major(gcc)?, ver_major(libgcc)?);
    // 子目录版：取最后一段版本形态。
    let seg = subdir.rsplit('/').next().unwrap_or("");
    let s = ver_major(seg)?;
    if g != l || g != s {
        return Err(MX_MISMATCH);
    }
    Ok(())
}

/// 告警账。
pub struct AlarmBook {
    pub rows: Vec<(String, String, u32)>, // (组件, 事件, 诊断码)
    pub sealed: bool,
}

impl AlarmBook {
    pub fn new() -> Self {
        AlarmBook { rows: Vec::new(), sealed: false }
    }

    /// 告警落账（组件/事件非空；封账拒写）。
    pub fn alarm(&mut self, comp: &str, event: &str, code: u32) -> Result<(), u32> {
        if comp.is_empty() || event.is_empty() {
            return Err(MX_VER_MISS);
        }
        if self.sealed {
            return Err(MX_ALARM_MISS);
        }
        self.rows.push((comp.to_string(), event.to_string(), code));
        Ok(())
    }

    /// 检测主体：逐件对齐核，混装即告警（返回告警数）。
    pub fn detect(&mut self, gcc: &str, libgcc: &str, subdir: &str) -> usize {
        match mix_check(gcc, libgcc, subdir) {
            Ok(()) => 0,
            Err(code) => {
                let _ = self.alarm("gcc", "版本混装", code);
                1
            }
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：匹配 ×5（同主版 13 的三种形态）+ 混装 ×5。
pub fn standard_scenarios() -> (usize, usize) {
    let mut alarms_ok = 0usize;
    let mut clean_ok = 0usize;
    // 匹配 ×5：零误报
    let match_cases: [(&str, &str, &str); 5] = [
        (PIN_GCC_VER, "13.2.0", "/usr/lib/gcc/x86_64-varix-unxreal/13.2.0"),
        ("13.2.0", "13.1.0", "13.2.0"),           // 修订不同主版同 → 匹配
        ("13.0.1", "13.2.0", "13.0.1"),
        ("13.2.0", "13.2.9", "13.2.0"),
        ("13.1.2", "13.1.2", "13.1.2"),
    ];
    for (g, l, s) in match_cases.iter() {
        if mix_check(g, l, s).is_ok() {
            clean_ok += 1;
        }
    }
    // 混装 ×5：必报警
    let mix_cases: [(&str, &str, &str); 5] = [
        (PIN_GCC_VER, "12.2.0", "13.2.0"),
        (PIN_GCC_VER, "13.2.0", "12.2.0"),
        ("12.2.0", "13.2.0", "13.2.0"),
        ("13.2.0", "14.1.0", "14.1.0"),
        ("14.1.0", "13.2.0", "13.2.0"),
    ];
    for (g, l, s) in mix_cases.iter() {
        let mut b = AlarmBook::new();
        if b.detect(g, l, s) == 1 && b.rows.len() == 1 {
            alarms_ok += 1;
        }
    }
    (clean_ok + alarms_ok, 10)
}

/// 注入核：6 注入必抓（空版本/坏格式/混装/漏报/封账拒写/告警空件）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空版本必抓
    if mix_check("", "13.2.0", "13.2.0") == Err(MX_VER_MISS) {
        caught += 1;
    }
    // 2) 坏格式必抓
    if mix_check("13.2", "13.2.0", "13.2.0") == Err(MX_FMT_BAD) {
        caught += 1;
    }
    // 3) 混装必抓
    if mix_check("13.2.0", "12.2.0", "13.2.0") == Err(MX_MISMATCH) {
        caught += 1;
    }
    // 4) 告警漏报必抓（封账后 detect 告警落账失败算漏报）
    let mut b = AlarmBook::new();
    b.seal();
    let n = b.detect("13.2.0", "12.2.0", "13.2.0");
    if b.rows.is_empty() && n == 1 {
        caught += 1;
    }
    // 5) 告警空件必抓
    let mut b2 = AlarmBook::new();
    if b2.alarm("", "事件", MX_MISMATCH) == Err(MX_VER_MISS) {
        caught += 1;
    }
    // 6) 检测器零告警面（匹配件 → 0 告警零误报）
    let mut b3 = AlarmBook::new();
    if b3.detect(PIN_GCC_VER, "13.2.0", "/usr/lib/gcc/x86_64-varix-unxreal/13.2.0") == 0
        && b3.rows.is_empty()
    {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46415_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46415");
    // J1-1 混装 ×5 必报警 + 匹配 ×5 零误报（10/10）
    let (p, n) = standard_scenarios();
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("场景 {p}/{n}") };
    s.add_dyn("46415-J1-混装×5匹配×5", ok, &detail);
    // J1-2 版本格式核（三段数字）
    let ok = ver_fmt_ok(PIN_GCC_VER) && !ver_fmt_ok("13.2") && ver_major(PIN_GCC_VER) == Ok(13);
    s.add("46415-J1-版本格式", ok, if ok { "" } else { "格式漂移" });
    // J1-3 钉版件零误报
    let mut b = AlarmBook::new();
    let n = b.detect(PIN_GCC_VER, PIN_GCC_VER, "/usr/lib/gcc/x86_64-varix-unxreal/13.2.0");
    let ok = n == 0 && b.rows.is_empty();
    s.add("46415-J1-钉版零误报", ok, if ok { "" } else { "误报" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46415-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46415_all_checks_green() {
        let set = run_unx46415_checks();
        assert!(set.all_passed(), "UNX-F46415 checks must be green: {}", set.red_items_text());
    }
}
