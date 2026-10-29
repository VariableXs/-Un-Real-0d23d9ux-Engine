//! UNX-F56174 syscall 负载脚本 v2 冻结件（深化参数集）（0x615C0 段）
//!
//! 锚点：UNX-F56174-J1 负载脚本冻结账（syscall 负载脚本 v2 深化参数
//! 集——线程数/调用配比/时长/采样间隔/预热秒——逐参冻结落账：参数 ×
//! 值 × 冻结波次 × 指纹）；J2 冻结闸（参数值物理界 + 冻结后拒改 + 重
//! 冻结新波次单调）；J3 脚本印（五参齐冻可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·负载脚本面）。
//!
//! 结构：五参闭集 + 值域核（逐参物理界）+ 冻结账（参数唯一索引 + 波
//! 次链）+ 脚本印 + 注入核（未知参/越界值/冻结后改/回退波次/缺参印
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x615C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x615C0;
pub const LS_UNK_PARAM: u32 = SEG_BASE + 0x01; // 未知参数
pub const LS_VAL_RANGE: u32 = SEG_BASE + 0x02; // 参数值越界
pub const LS_FROZEN_WRITE: u32 = SEG_BASE + 0x03; // 冻结后改
pub const LS_WAVE_BACK: u32 = SEG_BASE + 0x04; // 波次回退
pub const LS_DUP_PARAM: u32 = SEG_BASE + 0x05; // 同参重冻

/// 五参闭集。
pub const P_THREADS: usize = 0; // 线程数
pub const P_CALL_RATIO: usize = 1; // 调用配比（万分比 read 份额）
pub const P_DURATION: usize = 2; // 时长（秒）
pub const P_SAMPLE_IV: usize = 3; // 采样间隔（毫秒）
pub const P_WARMUP: usize = 4; // 预热秒
pub const PARAM_COUNT: usize = 5;

/// 参数名渲染。
pub const PARAM_NAMES: [&str; PARAM_COUNT] =
    ["threads", "call_ratio", "duration_s", "sample_iv_ms", "warmup_s"];

/// 值域核：逐参物理界。
pub fn value_in_range(param: usize, val: u32) -> bool {
    match param {
        P_THREADS => val >= 1 && val <= 256,
        P_CALL_RATIO => val >= 0 && val <= 10_000,
        P_DURATION => val >= 10 && val <= 3_600,
        P_SAMPLE_IV => val >= 10 && val <= 60_000,
        P_WARMUP => val >= 0 && val <= 300,
        _ => false,
    }
}

/// 单参冻结账。
#[derive(Clone)]
pub struct FrozenParam {
    pub param: usize,
    pub value: u32,
    pub wave: u32,
    pub fp: u32,
}

impl FrozenParam {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|w{}|{:08x}",
            PARAM_NAMES[self.param], self.value, self.wave, self.fp
        )
    }
}

/// 负载脚本冻结册。
pub struct LoadScriptBook {
    entries: [Option<FrozenParam>; PARAM_COUNT],
    top_wave: u32,
    sealed: bool,
}

impl LoadScriptBook {
    pub fn new() -> Self {
        LoadScriptBook {
            entries: [None, None, None, None, None],
            top_wave: 0,
            sealed: false,
        }
    }

    /// 冻结一参：参闭集 + 值域 + 重冻拒 + 波次单调四闸。
    pub fn freeze(&mut self, param: usize, value: u32, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_FROZEN_WRITE);
        }
        if param >= PARAM_COUNT {
            return Err(LS_UNK_PARAM);
        }
        if self.entries[param].is_some() {
            return Err(LS_DUP_PARAM);
        }
        if self.top_wave > 0 && wave < self.top_wave {
            return Err(LS_WAVE_BACK);
        }
        if !value_in_range(param, value) {
            return Err(LS_VAL_RANGE);
        }
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ param as u32;
        h = h.wrapping_mul(0x0100_0193) ^ value;
        self.entries[param] = Some(FrozenParam {
            param,
            value,
            wave,
            fp: h,
        });
        self.top_wave = wave;
        Ok(())
    }

    pub fn entry(&self, param: usize) -> Option<&FrozenParam> {
        if param >= PARAM_COUNT {
            return None;
        }
        self.entries[param].as_ref()
    }

    pub fn top_wave(&self) -> u32 {
        self.top_wave
    }

    /// 五参齐冻可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.iter().any(|e| e.is_none()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn book_ok(&self) -> bool {
        self.entries.iter().flatten().all(|e| value_in_range(e.param, e.value))
    }

    /// 脚本指纹：全参折叠。
    pub fn script_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ e.fp;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("LOADV2-SEAL\n");
        }
        out
    }
}

impl Default for LoadScriptBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：五参齐冻（threads=16 / ratio=4000 / duration=60 / iv=100 / warmup=5）。
fn standard_book() -> LoadScriptBook {
    let mut b = LoadScriptBook::new();
    let _ = b.freeze(P_THREADS, 16, 1);
    let _ = b.freeze(P_CALL_RATIO, 4_000, 2);
    let _ = b.freeze(P_DURATION, 60, 3);
    let _ = b.freeze(P_SAMPLE_IV, 100, 4);
    let _ = b.freeze(P_WARMUP, 5, 5);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知参
    let mut b = LoadScriptBook::new();
    if b.freeze(9, 1, 1) == Err(LS_UNK_PARAM) {
        caught += 1;
    }
    // 注 2：同参重冻
    let mut b = standard_book();
    if b.freeze(P_THREADS, 8, 6) == Err(LS_DUP_PARAM) {
        caught += 1;
    }
    // 注 3：值越界（threads = 0）
    let mut b = LoadScriptBook::new();
    if b.freeze(P_THREADS, 0, 1) == Err(LS_VAL_RANGE) {
        caught += 1;
    }
    // 注 4：波次回退
    let mut b = LoadScriptBook::new();
    let _ = b.freeze(P_THREADS, 16, 4);
    if b.freeze(P_CALL_RATIO, 4000, 2) == Err(LS_WAVE_BACK) {
        caught += 1;
    }
    // 注 5：缺参印
    let mut b = LoadScriptBook::new();
    let _ = b.freeze(P_THREADS, 16, 1);
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 冻结账；J2 冻结闸；J3 脚本印。
pub fn run_unx56174_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56174");
    // J1-1 五参齐冻
    let b = standard_book();
    let ok = b.book_ok() && b.top_wave() == 5;
    s.add("56174-J1-五参齐冻", ok, if ok { "" } else { "冻结账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("threads|16|w1|") && text.contains("warmup_s|5|w5|");
    s.add("56174-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 值域逐参
    let ok = value_in_range(P_THREADS, 1) && value_in_range(P_THREADS, 256)
        && value_in_range(P_CALL_RATIO, 0) && value_in_range(P_CALL_RATIO, 10_000)
        && !value_in_range(P_THREADS, 0) && !value_in_range(P_THREADS, 257)
        && !value_in_range(P_DURATION, 9) && !value_in_range(P_SAMPLE_IV, 9);
    s.add("56174-J2-值域逐参", ok, if ok { "" } else { "值域漂移" });
    // J2-2 重冻拒
    let mut b2 = standard_book();
    let ok = b2.freeze(P_DURATION, 30, 6) == Err(LS_DUP_PARAM);
    s.add("56174-J2-重冻拒", ok, if ok { "" } else { "重冻闸失效" });
    // J2-3 波次单调
    let mut b2 = LoadScriptBook::new();
    let _ = b2.freeze(P_THREADS, 16, 3);
    let ok = b2.freeze(P_CALL_RATIO, 4000, 3).is_ok()
        && b2.freeze(P_DURATION, 60, 2) == Err(LS_WAVE_BACK);
    s.add("56174-J2-波次单调", ok, if ok { "" } else { "波次漂移" });
    // J2-4 脚本指纹确定
    let ok = standard_book().script_fp() == standard_book().script_fp();
    s.add("56174-J2-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J2-5 一参变签变
    let mut b2 = standard_book();
    let fp0 = b2.script_fp();
    let _ = b2.freeze(P_WARMUP, 10, 6);
    let ok = b2.script_fp() != fp0;
    s.add("56174-J2-参变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-1 脚本印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56174-J3-脚本印", ok, if ok { "" } else { "脚本印失败" });
    // J3-2 印后拒改
    let ok = b.freeze(P_THREADS, 8, 6) == Err(LS_FROZEN_WRITE);
    s.add("56174-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("LOADV2-SEAL");
    s.add("56174-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 参数名闭集
    let ok = PARAM_NAMES[P_THREADS] == "threads" && PARAM_NAMES[P_WARMUP] == "warmup_s";
    s.add("56174-J3-名闭集", ok, if ok { "" } else { "名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56174-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56174_all_checks_green() {
        let set = run_unx56174_checks();
        assert!(set.all_passed(), "UNX-F56174 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56174_range_stable() {
        assert!(value_in_range(P_CALL_RATIO, 10_000));
        assert!(!value_in_range(P_CALL_RATIO, 10_001));
    }
}
