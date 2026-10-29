//! UNX-F56166 syscall 聚合 O2 深化标定件（0x614C0 段）
//!
//! 锚点：UNX-F56166-J1 O2 标定账（syscall 聚合器 O2 层三参数——归并
//! 阈/桶宽/采样步——逐参标定落账：参数 × 值 × 波次 × 指纹四元组）；
//! J2 标定闸（参数值物理界 + 波次单调 + 同参重标拒三闸）；J3 标定印
//! （三参齐标可印，印后只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·O2 标定面）。
//!
//! 结构：三参闭集（归并阈 MERGE_TH / 桶宽 BUCKET_W / 采样步 SAMPLE_STEP）
//! + 值域核（逐参物理界）+ 标定账（参数唯一索引 + 波次链）+ 标定印 +
//! 注入核（未知参/越界值/重标/回退波次/印后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x614C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x614C0;
pub const SO_UNK_PARAM: u32 = SEG_BASE + 0x01; // 未知参数
pub const SO_VAL_RANGE: u32 = SEG_BASE + 0x02; // 参数值越界
pub const SO_DUP_PARAM: u32 = SEG_BASE + 0x03; // 同参重标
pub const SO_WAVE_BACK: u32 = SEG_BASE + 0x04; // 波次回退
pub const SO_SEALED_WRITE: u32 = SEG_BASE + 0x05; // 印后改

/// 三参闭集。
pub const P_MERGE_TH: usize = 0; // 归并阈（事件数）
pub const P_BUCKET_W: usize = 1; // 桶宽（微秒）
pub const P_SAMPLE_STEP: usize = 2; // 采样步（1/步）
pub const PARAM_COUNT: usize = 3;

/// 参数名渲染。
pub const PARAM_NAMES: [&str; PARAM_COUNT] = ["merge_th", "bucket_w", "sample_step"];

/// 值域闭集。
pub const MERGE_TH_MAX: u32 = 10_000;
pub const BUCKET_W_MIN: u32 = 1;
pub const BUCKET_W_MAX: u32 = 1_000_000;
pub const SAMPLE_STEP_MIN: u32 = 1;
pub const SAMPLE_STEP_MAX: u32 = 1000;

/// 值域核：逐参物理界。
pub fn value_in_range(param: usize, val: u32) -> bool {
    match param {
        P_MERGE_TH => val >= 1 && val <= MERGE_TH_MAX,
        P_BUCKET_W => val >= BUCKET_W_MIN && val <= BUCKET_W_MAX,
        P_SAMPLE_STEP => val >= SAMPLE_STEP_MIN && val <= SAMPLE_STEP_MAX,
        _ => false,
    }
}

/// 单条标定账。
#[derive(Clone)]
pub struct CalibEntry {
    pub param: usize,
    pub value: u32,
    pub wave: u32,
    pub fp: u32,
}

impl CalibEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|w{}|{:08x}",
            PARAM_NAMES[self.param], self.value, self.wave, self.fp
        )
    }
}

/// O2 标定账。
pub struct O2CalibBook {
    entries: [Option<CalibEntry>; PARAM_COUNT],
    top_wave: u32,
    sealed: bool,
}

impl O2CalibBook {
    pub fn new() -> Self {
        O2CalibBook {
            entries: [None, None, None],
            top_wave: 0,
            sealed: false,
        }
    }

    /// 标定一参：参闭集 + 值域 + 重标拒 + 波次单调四闸。
    pub fn calibrate(&mut self, param: usize, value: u32, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SO_SEALED_WRITE);
        }
        if param >= PARAM_COUNT {
            return Err(SO_UNK_PARAM);
        }
        if self.entries[param].is_some() {
            return Err(SO_DUP_PARAM);
        }
        if self.top_wave > 0 && wave < self.top_wave {
            return Err(SO_WAVE_BACK);
        }
        if !value_in_range(param, value) {
            return Err(SO_VAL_RANGE);
        }
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ param as u32;
        h = h.wrapping_mul(0x0100_0193) ^ value;
        self.entries[param] = Some(CalibEntry {
            param,
            value,
            wave,
            fp: h,
        });
        self.top_wave = wave;
        Ok(())
    }

    pub fn entry(&self, param: usize) -> Option<&CalibEntry> {
        if param >= PARAM_COUNT {
            return None;
        }
        self.entries[param].as_ref()
    }

    pub fn top_wave(&self) -> u32 {
        self.top_wave
    }

    /// 三参齐标可印。
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

    /// 全量核。
    pub fn book_ok(&self) -> bool {
        self.entries.iter().flatten().all(|e| value_in_range(e.param, e.value))
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("O2CAL-SEAL\n");
        }
        out
    }
}

impl Default for O2CalibBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三参齐标（merge_th=64 / bucket_w=1000 / sample_step=10）。
fn standard_book() -> O2CalibBook {
    let mut b = O2CalibBook::new();
    let _ = b.calibrate(P_MERGE_TH, 64, 1);
    let _ = b.calibrate(P_BUCKET_W, 1000, 2);
    let _ = b.calibrate(P_SAMPLE_STEP, 10, 3);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知参
    let mut b = O2CalibBook::new();
    if b.calibrate(9, 1, 1) == Err(SO_UNK_PARAM) {
        caught += 1;
    }
    // 注 2：同参重标
    let mut b = standard_book();
    if b.calibrate(P_MERGE_TH, 32, 4) == Err(SO_DUP_PARAM) {
        caught += 1;
    }
    // 注 3：值越界（merge_th = 0）
    let mut b = O2CalibBook::new();
    if b.calibrate(P_MERGE_TH, 0, 1) == Err(SO_VAL_RANGE) {
        caught += 1;
    }
    // 注 4：波次回退
    let mut b = O2CalibBook::new();
    let _ = b.calibrate(P_MERGE_TH, 64, 5);
    if b.calibrate(P_BUCKET_W, 1000, 3) == Err(SO_WAVE_BACK) {
        caught += 1;
    }
    // 注 5：印后改
    let mut b = standard_book();
    let _ = b.seal();
    if b.calibrate(P_SAMPLE_STEP, 20, 4) == Err(SO_SEALED_WRITE) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 标定账；J2 标定闸；J3 标定印。
pub fn run_unx56166_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56166");
    // J1-1 三参齐标
    let b = standard_book();
    let ok = b.book_ok() && b.top_wave() == 3;
    s.add("56166-J1-三参齐标", ok, if ok { "" } else { "标定账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("merge_th|64|w1|") && text.contains("sample_step|10|w3|");
    s.add("56166-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 值域核逐参
    let ok = value_in_range(P_MERGE_TH, 1) && value_in_range(P_MERGE_TH, MERGE_TH_MAX)
        && value_in_range(P_BUCKET_W, BUCKET_W_MIN) && value_in_range(P_BUCKET_W, BUCKET_W_MAX)
        && value_in_range(P_SAMPLE_STEP, SAMPLE_STEP_MIN)
        && !value_in_range(P_MERGE_TH, 0) && !value_in_range(P_BUCKET_W, BUCKET_W_MAX + 1)
        && !value_in_range(P_SAMPLE_STEP, 0);
    s.add("56166-J2-值域逐参", ok, if ok { "" } else { "值域漂移" });
    // J2-2 重标拒
    let mut b2 = standard_book();
    let ok = b2.calibrate(P_BUCKET_W, 500, 4) == Err(SO_DUP_PARAM);
    s.add("56166-J2-重标拒", ok, if ok { "" } else { "重标闸失效" });
    // J2-3 波次单调
    let mut b2 = O2CalibBook::new();
    let _ = b2.calibrate(P_MERGE_TH, 64, 3);
    let ok = b2.calibrate(P_BUCKET_W, 1000, 3).is_ok()
        && b2.calibrate(P_SAMPLE_STEP, 10, 2) == Err(SO_WAVE_BACK);
    s.add("56166-J2-波次单调", ok, if ok { "" } else { "波次漂移" });
    // J2-4 同波次可并存（wave == top_wave 合法）
    let ok = b2.top_wave() == 3;
    s.add("56166-J2-同波并存", ok, if ok { "" } else { "并存漂移" });
    // J2-5 标定指纹确定
    let ok = standard_book().entry(P_MERGE_TH).map(|e| e.fp).unwrap_or(0)
        == standard_book().entry(P_MERGE_TH).map(|e| e.fp).unwrap_or(1);
    s.add("56166-J2-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-1 标定印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56166-J3-标定印", ok, if ok { "" } else { "标定印失败" });
    // J3-2 印后只读
    let ok = b.calibrate(P_MERGE_TH, 32, 4) == Err(SO_SEALED_WRITE);
    s.add("56166-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("O2CAL-SEAL");
    s.add("56166-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 缺参拒印
    let mut b = O2CalibBook::new();
    let _ = b.calibrate(P_MERGE_TH, 64, 1);
    let ok = !b.seal() && !b.sealed();
    s.add("56166-J3-缺参拒印", ok, if ok { "" } else { "印闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56166-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56166_all_checks_green() {
        let set = run_unx56166_checks();
        assert!(set.all_passed(), "UNX-F56166 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56166_range_stable() {
        assert!(value_in_range(P_SAMPLE_STEP, 1));
        assert!(!value_in_range(P_SAMPLE_STEP, 0));
    }
}
