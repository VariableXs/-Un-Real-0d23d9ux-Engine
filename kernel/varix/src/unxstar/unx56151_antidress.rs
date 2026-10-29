//! UNX-F56151 "数字化妆"对抗检查件（AI-82 扩展域）（0x612E0 段）
//!
//! 锚点：UNX-F56151-J1 对抗检查账（数字化妆五手法——尾位裁切/单位漂
//! 移/样本择优/时间窗挑拣/基线重置——逐手法构造检测样本落账）；J2 检
//! 测核（五手法检测器逐手法必报：手法 × 检测位 × 置信四元组）；J3 对
//! 抗印（五手法全检出才可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·对抗检查面）。
//!
//! 结构：五手法样本构造器（每手法一对原始/化妆样本）+ 检测器组（尾
//! 位熵检/单位字典检/样本离散度检/时间窗连续性检/基线跳变检）+ 检测
//! 账（手法唯一索引）+ 对抗印（五检全报可印）+ 注入核（未检手法放行/
//! 重复手法账/印后改账/置信漂移/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x612E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x612E0;
pub const DC_UNK_TRICK: u32 = SEG_BASE + 0x01; // 未知手法
pub const DC_UNDETECTED: u32 = SEG_BASE + 0x02; // 手法未检出
pub const DC_DUP_TRICK: u32 = SEG_BASE + 0x03; // 重复手法账
pub const DC_SEALED_WRITE: u32 = SEG_BASE + 0x04; // 印后改账
pub const DC_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空账印

/// 五手法闭集。
pub const T_TAIL_TRIM: usize = 0; // 尾位裁切（均值化好看）
pub const T_UNIT_DRIFT: usize = 1; // 单位漂移（fps→frame_ms 偷换）
pub const T_SAMPLE_PICK: usize = 2; // 样本择优（丢低点）
pub const T_WINDOW_PICK: usize = 3; // 时间窗挑拣（只报好窗）
pub const T_BASELINE_RESET: usize = 4; // 基线重置（重录高基线）
pub const TRICK_COUNT: usize = 5;

/// 手法名渲染。
pub const TRICK_NAMES: [&str; TRICK_COUNT] = [
    "tail-trim", "unit-drift", "sample-pick", "window-pick", "baseline-reset",
];

/// 检测器一：尾位裁切检——序列尾位 0 比例异常高（>50%）报。
pub fn detect_tail_trim(vals: &[u32]) -> bool {
    if vals.is_empty() {
        return false;
    }
    let zeros = vals.iter().filter(|v| **v % 10 == 0).count();
    zeros * 2 > vals.len()
}

/// 检测器二：单位漂移检——键名 fps 但值域像毫秒（值 < 33 视为可疑 fps）。
pub fn detect_unit_drift(key: &str, vals: &[u32]) -> bool {
    key.contains("fps") && !vals.is_empty() && vals.iter().all(|v| *v < 33 && *v > 0)
}

/// 检测器三：样本择优检——样本离散度异常低（全同值）报。
pub fn detect_sample_pick(vals: &[u32]) -> bool {
    vals.len() >= 3 && vals.windows(2).all(|w| w[0] == w[1])
}

/// 检测器四：时间窗挑拣检——窗间隙跳号（相邻窗差 > 1）报。
pub fn detect_window_pick(windows: &[u32]) -> bool {
    windows.len() >= 2
        && windows
            .windows(2)
            .any(|w| w[1] > w[0] && w[1] - w[0] > 1)
}

/// 检测器五：基线重置检——基线序列跳升 > 20% 报。
pub fn detect_baseline_reset(baselines: &[u32]) -> bool {
    baselines.len() >= 2 && baselines.windows(2).any(|w| {
        w[0] > 0 && (w[1] as u64 * 100) > (w[0] as u64 * 120)
    })
}

/// 单条检测账。
#[derive(Clone)]
pub struct DetectionEntry {
    pub trick: usize,
    pub detected: bool,
    pub confidence: u32,
}

impl DetectionEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|c{}",
            TRICK_NAMES[self.trick],
            if self.detected { "HIT" } else { "MISS" },
            self.confidence
        )
    }
}

/// 对抗检查总账。
pub struct DetectionBook {
    entries: Vec<DetectionEntry>,
    sealed: bool,
}

impl DetectionBook {
    pub fn new() -> Self {
        DetectionBook {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条检测：手法闭集 + 重复拒 + 印后拒三闸。
    pub fn record(&mut self, trick: usize, detected: bool, confidence: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_SEALED_WRITE);
        }
        if trick >= TRICK_COUNT {
            return Err(DC_UNK_TRICK);
        }
        if self.entries.iter().any(|e| e.trick == trick) {
            return Err(DC_DUP_TRICK);
        }
        self.entries.push(DetectionEntry {
            trick,
            detected,
            confidence,
        });
        Ok(())
    }

    /// 对抗印：五手法全检出才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.len() != TRICK_COUNT {
            return false;
        }
        if self.entries.iter().any(|e| !e.detected) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn entry(&self, trick: usize) -> Option<&DetectionEntry> {
        self.entries.iter().find(|e| e.trick == trick)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 检出计数。
    pub fn hit_count(&self) -> usize {
        self.entries.iter().filter(|e| e.detected).count()
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("DETECTION-SEAL\n");
        }
        out
    }
}

impl Default for DetectionBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 五手法检测全跑：构造样本 → 逐检 → 落账。返回 (5, hits)。
fn run_all_detections(book: &mut DetectionBook) -> (usize, usize) {
    // T1 尾位裁切：化妆后尾位 0 扎堆
    let t1 = detect_tail_trim(&[9500, 9500, 9500, 9500, 9500, 0, 0, 0]);
    // T2 单位漂移：fps 键下毫秒值域
    let t2 = detect_unit_drift("frame_fps", &[16, 16, 17, 16]);
    // T3 样本择优：全同值
    let t3 = detect_sample_pick(&[9500, 9500, 9500, 9500]);
    // T4 时间窗挑拣：跳号窗
    let t4 = detect_window_pick(&[1, 2, 7, 8]);
    // T5 基线重置：跳升 25%
    let t5 = detect_baseline_reset(&[9500, 12000]);
    let hits = [t1, t2, t3, t4, t5];
    for (t, hit) in hits.iter().enumerate() {
        let _ = book.record(t, *hit, if *hit { 90 } else { 10 });
    }
    (TRICK_COUNT, hits.iter().filter(|h| **h).count())
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知手法
    let mut b = DetectionBook::new();
    if b.record(9, true, 90) == Err(DC_UNK_TRICK) {
        caught += 1;
    }
    // 注 2：重复手法
    let mut b = DetectionBook::new();
    let _ = b.record(T_TAIL_TRIM, true, 90);
    if b.record(T_TAIL_TRIM, true, 80) == Err(DC_DUP_TRICK) {
        caught += 1;
    }
    // 注 3：未检全拒印
    let mut b = DetectionBook::new();
    for t in 0..TRICK_COUNT {
        let _ = b.record(t, t < 4, 90); // T5 未检
    }
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    // 注 4：印后改账
    let mut b = DetectionBook::new();
    for t in 0..TRICK_COUNT {
        let _ = b.record(t, true, 90);
    }
    let _ = b.seal();
    if b.record(T_TAIL_TRIM, false, 10) == Err(DC_SEALED_WRITE) {
        caught += 1;
    }
    // 注 5：空账印
    let mut b = DetectionBook::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 对抗账；J2 检测核；J3 对抗印。
pub fn run_unx56151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56151");
    // J1-1 五手法全落账全检出
    let mut book = DetectionBook::new();
    let (n, hits) = run_all_detections(&mut book);
    let ok = n == 5 && hits == 5 && book.hit_count() == 5;
    s.add("56151-J1-五手法全检", ok, if ok { "" } else { "检出面漂移" });
    // J1-2 渲染导出齐
    let text = book.render_book();
    let ok = text.contains("tail-trim|HIT|c90") && text.contains("baseline-reset|HIT");
    s.add("56151-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 尾位裁切检：正例报 + 负例不报
    let ok = detect_tail_trim(&[9500, 9500, 0, 0]) && !detect_tail_trim(&[9501, 9502, 9503]);
    s.add("56151-J2-尾裁检", ok, if ok { "" } else { "尾裁检漂移" });
    // J2-2 单位漂移检：fps 键小值报 + 非 fps 键不报
    let ok = detect_unit_drift("frame_fps", &[16, 17]) && !detect_unit_drift("latency_ms", &[16, 17]);
    s.add("56151-J2-单位检", ok, if ok { "" } else { "单位检漂移" });
    // J2-3 样本择优检：全同报 + 正常离散不报
    let ok = detect_sample_pick(&[100, 100, 100]) && !detect_sample_pick(&[98, 100, 102]);
    s.add("56151-J2-择优检", ok, if ok { "" } else { "择优检漂移" });
    // J2-4 时间窗挑拣检：跳号报 + 连续不报
    let ok = detect_window_pick(&[1, 2, 7]) && !detect_window_pick(&[1, 2, 3]);
    s.add("56151-J2-挑窗检", ok, if ok { "" } else { "挑窗检漂移" });
    // J2-5 基线重置检：跳升报 + 平稳不报
    let ok = detect_baseline_reset(&[9500, 12000]) && !detect_baseline_reset(&[9500, 9600]);
    s.add("56151-J2-基线检", ok, if ok { "" } else { "基线检漂移" });
    // J2-6 检出但印前置不足（T5 未检时拒印）
    let mut b2 = DetectionBook::new();
    let _ = b2.record(T_TAIL_TRIM, true, 90);
    let _ = b2.record(T_UNIT_DRIFT, true, 90);
    let _ = b2.record(T_SAMPLE_PICK, true, 90);
    let _ = b2.record(T_WINDOW_PICK, true, 90);
    let _ = b2.record(T_BASELINE_RESET, false, 10);
    let ok = !b2.seal() && b2.hit_count() == 4;
    s.add("56151-J2-缺检拒印", ok, if ok { "" } else { "印闸失效" });
    // J3-1 对抗印
    let mut b3 = DetectionBook::new();
    let _ = run_all_detections(&mut b3);
    let ok = b3.seal() && b3.sealed();
    s.add("56151-J3-对抗印", ok, if ok { "" } else { "对抗印失败" });
    // J3-2 印后只读
    let ok = b3.record(T_TAIL_TRIM, true, 90) == Err(DC_SEALED_WRITE);
    s.add("56151-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b3.render_book().contains("DETECTION-SEAL");
    s.add("56151-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56151-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56151_all_checks_green() {
        let set = run_unx56151_checks();
        assert!(set.all_passed(), "UNX-F56151 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56151_detectors_stable() {
        assert!(detect_tail_trim(&[9500, 0, 0]));
        assert!(!detect_tail_trim(&[9501, 9502, 9503]));
    }
}
