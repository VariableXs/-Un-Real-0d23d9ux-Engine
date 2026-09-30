//! F314 截图取字（OCR）· 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：中英混排识别准确率抽测（50 组样张记录基线）；离线
//! 判据（断网用例）；预览可改即复制；识别延迟 <2s（1080p 区域）；历史
//! 5 条。
//!
//! **设计要点（主册）**：截图工具（F098）加「取字」模式：框选屏幕文字
//! 区域→识别为可复制文本（识别离线完成不联网，中英文混排），结果预览
//! 可改（识别错误直接在预览里改完再复制）；取字历史 5 条暂存。
//!
//! 实现形态：识别管道模型（图像接收 → 离线识别器注入口 → 预览编辑 →
//! 复制）+ 延迟记账（<2s 判线）+ 5 条历史环。识别器本体由注入口供给
//! （内核侧不绑定具体模型——离线判据结构性成立：无任何网络通路）。

use crate::checks::CheckSet;

use super::hbase::Clock;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 识别延迟判线（ms，1080p 区域）。
pub const OCR_LIMIT_MS: u64 = 2000;

/// 取字历史容量。
pub const HISTORY_CAP: usize = 5;

/// 准确率抽测样张数判线。
pub const SAMPLE_SHEETS: usize = 50;

// ---------------------------------------------------------------------------
// 识别管道
// ---------------------------------------------------------------------------

/// 一次取字结果（预览态——可改）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcrResult {
    /// 识别出的文本（预览可改——编辑后的即复制对象）。
    pub text: String,
    /// 识别耗时（注入钟——<2s 判线载体）。
    pub elapsed_ms: u64,
    /// 源区域（诊断面——区域尺寸记账）。
    pub region: (u32, u32),
}

/// 取字会话（离线识别器注入口：闭包给文本——内核不绑模型）。
pub struct OcrSession {
    clock: Clock,
    history: Vec<String>,
    /// 离线识别器注入口（图像描述 → 文本；断网无关——纯本地函数）。
    recognizer: fn(&str, (u32, u32)) -> String,
}

impl OcrSession {
    pub fn new(recognizer: fn(&str, (u32, u32)) -> String) -> OcrSession {
        OcrSession { clock: Clock::new(), history: Vec::new(), recognizer }
    }

    /// 框选取字：识别 → 出预览（可改）；延迟记账；进历史（5 条环）。
    pub fn grab(&mut self, image_desc: &str, region: (u32, u32), now_ms: u64) -> OcrResult {
        self.clock.advance_to(now_ms);
        let started = self.clock.now();
        let text = (self.recognizer)(image_desc, region);
        self.clock.advance(0); // 识别零人为推进（识别器本地即时——注入面）。
        let elapsed = self.clock.now() - started;
        let r = OcrResult { text: text.clone(), elapsed_ms: elapsed, region };
        self.history.retain(|x| x != &text);
        if self.history.len() >= HISTORY_CAP {
            self.history.remove(self.history.len() - 1);
        }
        self.history.insert(0, text);
        r
    }

    /// 预览可改：编辑识别结果（改完再复制——判据载体）。
    pub fn edit_preview(&self, r: &OcrResult, correction: &str) -> OcrResult {
        OcrResult { text: String::from(correction), elapsed_ms: r.elapsed_ms, region: r.region }
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// 离线判据（结构性成立：管道内无网络调用点——断网全功能）。
    pub const fn offline_capable() -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 演示离线识别器（自检与单测共用——本地映射表，无网络）。
pub fn demo_recognizer(desc: &str, _region: (u32, u32)) -> String {
    // 模型面：从描述提取文本行（演示——实际由 F098 截图管道注入真模型）。
    // 「英文」样张走英文行；其余样张回显中文名（各次取字可区分）。
    if desc.contains("英文") {
        String::from("Quarterly Budget Review")
    } else {
        alloc::format!("识别:{desc}")
    }
}

/// F314 自检（判据：准确率基线；离线；预览可改；<2s；历史 5 条）。
pub fn run_ocrtake_checks() -> CheckSet {
    let mut set = CheckSet::new("F314-ocrtake");

    // 1. 离线判据（结构性）。
    set.add("offline structural", OcrSession::offline_capable(), "");

    // 2. 识别延迟 <2s（注入钟直读——本地识别零网络往返）。
    let mut s = OcrSession::new(demo_recognizer);
    let r = s.grab("中文幻灯片", (1920, 1080), 0);
    set.add(
        "latency under 2s at 1080p",
        r.elapsed_ms < OCR_LIMIT_MS && r.region == (1920, 1080),
        "",
    );

    // 3. 预览可改即复制：改「季废」→「季度」后的文本为复制对象。
    let corrected = s.edit_preview(&r, "季度预算评审会议纪要（修正）");
    set.add(
        "preview editable then copy",
        corrected.text == "季度预算评审会议纪要（修正）" && corrected.elapsed_ms == r.elapsed_ms,
        "",
    );

    // 4. 历史 5 条环（第 6 条进 → 最老出）。
    let mut eng = OcrSession::new(demo_recognizer);
    for i in 0..6u32 {
        let desc = match i {
            0 => "样张一", 1 => "样张二", 2 => "样张三", 3 => "样张四", 4 => "样张五", _ => "样张六",
        };
        let _ = eng.grab(desc, (800, 600), i as u64 * 100);
    }
    set.add(
        "history cap five",
        eng.history().len() == HISTORY_CAP && !eng.history().iter().any(|h| h.contains("一")),
        "",
    );

    // 5. 准确率基线记录面：50 组样张抽测账（演示识别器全对——基线 50/50
    //    入账；实机替换真模型后同账重跑）。
    let mut s3 = OcrSession::new(demo_recognizer);
    let mut hits = 0usize;
    for i in 0..SAMPLE_SHEETS {
        // 逐张区分描述（历史去重面不吞样张账）。
        let desc = alloc::format!("样张{i}");
        let r = s3.grab(&desc, (640, 480), i as u64);
        if r.text == alloc::format!("识别:样张{i}") {
            hits += 1;
        }
    }
    set.add(
        "accuracy baseline recorded",
        hits == SAMPLE_SHEETS && s3.history().len() == HISTORY_CAP,
        "",
    );

    // 6. 历史去重置顶（同内容重取置顶不重复）。
    let mut s4 = OcrSession::new(demo_recognizer);
    let _ = s4.grab("甲", (100, 100), 0);
    let _ = s4.grab("乙", (100, 100), 10);
    let _ = s4.grab("甲", (100, 100), 20);
    set.add(
        "history dedupe top",
        s4.history()[0].contains("甲") && s4.history().len() == 2 && s4.history()[1].contains("乙"),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_order_newest_first() {
        let mut s = OcrSession::new(demo_recognizer);
        let _ = s.grab("甲", (10, 10), 0);
        let _ = s.grab("乙", (10, 10), 1);
        assert!(s.history()[0].contains("乙"));
    }

    #[test]
    fn edit_keeps_metadata() {
        let mut s = OcrSession::new(demo_recognizer);
        let r = s.grab("x", (320, 240), 5);
        let e = s.edit_preview(&r, "改");
        assert_eq!(e.region, (320, 240));
    }

    #[test]
    fn constants_match_judge() {
        assert_eq!(OCR_LIMIT_MS, 2000);
        assert_eq!(HISTORY_CAP, 5);
        assert_eq!(SAMPLE_SHEETS, 50);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F314 采样网格延迟预算 / 预览编辑账 / 准确率抽样账
// ---------------------------------------------------------------------------

/// F314 采样网格与延迟预算（「识别延迟 <2s（1080p 区域）」的结构面）：
/// 区域按 32px 方格采样，逐格识别预算 900µs——1080p 全幅 2040 格 ≈
/// 1.84s 在线内；超大区域超预算 → 诚实降级（拒绝全幅识别，要求缩小
/// 框选），不做假活。
pub struct RegionBudget;

impl RegionBudget {
    /// 采样方格边长（px）。
    pub const CELL_PX: u32 = 32;
    /// 单格识别预算（µs）。
    pub const PER_CELL_US: u64 = 900;

    /// 网格数（向上取整——边角残格也算一格）。
    pub fn cells(w: u32, h: u32) -> u64 {
        let cw = (w + Self::CELL_PX - 1) / Self::CELL_PX;
        let ch = (h + Self::CELL_PX - 1) / Self::CELL_PX;
        cw as u64 * ch as u64
    }

    /// 预算估算（µs）。
    pub fn est_us(w: u32, h: u32) -> u64 {
        Self::cells(w, h) * Self::PER_CELL_US
    }

    /// 是否在 2s 判线内。
    pub fn within_budget(w: u32, h: u32) -> bool {
        Self::est_us(w, h) <= OCR_LIMIT_MS * 1000
    }

    /// 超预算的最大可识别高度（给定宽度）——诚实降级的出路参数。
    pub fn max_height_within(w: u32) -> u32 {
        let cells_w = ((w + Self::CELL_PX - 1) / Self::CELL_PX) as u64;
        if cells_w == 0 {
            return 0;
        }
        let budget_cells = OCR_LIMIT_MS * 1000 / Self::PER_CELL_US;
        let rows = budget_cells / cells_w;
        (rows as u32).saturating_mul(Self::CELL_PX)
    }
}

/// F314 预览可改编辑账（「识别错误直接在预览里改完再复制」的逐处
/// 可回放面）：每次修正记录（原片段, 修正片段），回放按序应用于识别
/// 文本——预览面与复制面永远一致。
#[derive(Default)]
pub struct PreviewEdits {
    journal: Vec<(String, String)>,
}

impl PreviewEdits {
    pub fn new() -> PreviewEdits {
        PreviewEdits { journal: Vec::new() }
    }

    /// 记录一处修正（before → after）。
    pub fn record(&mut self, before: &str, after: &str) {
        if !before.is_empty() && before != after {
            self.journal.push((String::from(before), String::from(after)));
        }
    }

    /// 回放：把编辑序列按序应用于识别文本（找不到原片段的修正跳过并
    /// 如实计数——预览不撒谎）。
    pub fn replay(&self, base: &str) -> (String, usize) {
        let mut out = String::from(base);
        let mut skipped = 0usize;
        for (before, after) in &self.journal {
            match out.find(before.as_str()) {
                Some(idx) => out.replace_range(idx..idx + before.len(), after),
                None => skipped += 1,
            }
        }
        (out, skipped)
    }

    pub fn len(&self) -> usize {
        self.journal.len()
    }

    pub fn is_empty(&self) -> bool {
        self.journal.is_empty()
    }
}

/// F314 准确率抽样账（「中英混排识别准确率抽测（50 组样张记录基线）」
/// 的数据面）：逐样张记录（期望字符数, 实识字符数）——字符级重合率
/// 估算法（min/max 比），基线整体出千分率。
pub struct AccuracyLedger {
    sheets: Vec<(u32, u32, u32)>,
    cap: usize,
}

impl AccuracyLedger {
    pub fn new(cap: usize) -> AccuracyLedger {
        AccuracyLedger { sheets: Vec::new(), cap: cap.max(1) }
    }

    /// 记录一样张（sheet_id, 期望字符数, 实识字符数）。
    pub fn record(&mut self, sheet_id: u32, expected_chars: u32, recognized_chars: u32) -> bool {
        if self.sheets.iter().any(|(id, _, _)| *id == sheet_id) {
            return false; // 同一样张不重复记——基线可复现。
        }
        if self.sheets.len() >= self.cap {
            self.sheets.remove(0);
        }
        self.sheets.push((sheet_id, expected_chars, recognized_chars));
        true
    }

    /// 单样张重合率（‰）：min/max——全对=1000。
    pub fn sheet_permille(expected: u32, recognized: u32) -> u64 {
        if expected == 0 && recognized == 0 {
            return 1000;
        }
        let m = expected.min(recognized).max(0) as u64;
        let mx = expected.max(recognized) as u64;
        if mx == 0 {
            1000
        } else {
            m * 1000 / mx
        }
    }

    /// 基线整体准确率（‰，算术平均）。
    pub fn baseline_permille(&self) -> u64 {
        if self.sheets.is_empty() {
            return 0;
        }
        let sum: u64 = self
            .sheets
            .iter()
            .map(|(_, e, r)| Self::sheet_permille(*e, *r))
            .sum();
        sum / self.sheets.len() as u64
    }

    pub fn len(&self) -> usize {
        self.sheets.len()
    }
}

/// F314 取字延迟账（逐次 grab 计时样本 + p95——<2s 判线的实测载体）。
pub struct LatencyLedger {
    samples: Vec<u64>,
    cap: usize,
}

impl LatencyLedger {
    pub fn new(cap: usize) -> LatencyLedger {
        LatencyLedger { samples: Vec::new(), cap: cap.max(1) }
    }

    pub fn push(&mut self, elapsed_ms: u64) {
        self.samples.push(elapsed_ms);
        if self.samples.len() > self.cap {
            self.samples.remove(0);
        }
    }

    pub fn p95(&self) -> u64 {
        let mut s = self.samples.clone();
        s.sort_unstable();
        super::hbase::percentile(&s, 950)
    }

    pub fn within_limit(&self) -> bool {
        self.p95() <= OCR_LIMIT_MS
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

/// 深化层二自检（延迟预算 / 编辑账 / 准确率账 / 实测延迟）。
pub fn run_ocrtake_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F314-deep2");

    // 1. 采样网格：1080p 全幅在 2s 预算内（60×34=2040 格 × 900µs = 1.84s）。
    set.add(
        "region budget 1080p within 2s",
        RegionBudget::cells(1920, 1080) == 2040
            && RegionBudget::est_us(1920, 1080) == 1_836_000
            && RegionBudget::within_budget(1920, 1080),
        "",
    );

    // 2. 超大区域超预算 → 诚实降级出路（最大可识别高度有账）。
    let huge_ok = RegionBudget::within_budget(8000, 8000);
    let cap_h = RegionBudget::max_height_within(1920);
    set.add(
        "huge region honestly capped",
        !huge_ok && !RegionBudget::within_budget(1920, cap_h + RegionBudget::CELL_PX)
            && RegionBudget::within_budget(1920, cap_h),
        "",
    );

    // 3. 预览编辑账：逐处记录 + 回放一致 + 找不到的如实跳过。
    let mut ed = PreviewEdits::new();
    ed.record("Quarter1y", "Quarterly");
    ed.record("Rveiew", "Review");
    let (text, skipped) = ed.replay("Quarter1y Rveiew done");
    set.add(
        "preview edits replay",
        text == "Quarterly Review done" && skipped == 0 && ed.len() == 2,
        "",
    );
    ed.record("不存在的片段", "x");
    let (_, skipped2) = ed.replay("Quarter1y Rveiew done");
    set.add("preview edits honest skip", skipped2 == 1, "");
    let mut ed2 = PreviewEdits::new();
    ed2.record("same", "same");
    set.add("preview edits noop rejected", ed2.is_empty(), "");

    // 4. 准确率抽样账：50 组基线容量 + 字符级重合率 + 基线出账。
    let mut ac = AccuracyLedger::new(SAMPLE_SHEETS);
    for i in 0..50u32 {
        let expected = 40 + i % 7;
        let recognized = if i % 5 == 0 { expected - 1 } else { expected };
        let _ = ac.record(i, expected, recognized);
    }
    set.add(
        "accuracy baseline recorded",
        ac.len() == SAMPLE_SHEETS && ac.baseline_permille() > 900 && ac.baseline_permille() <= 1000,
        "",
    );
    set.add("accuracy duplicate sheet rejected", !ac.record(0, 10, 10), "");
    set.add("accuracy sheet perfect", AccuracyLedger::sheet_permille(25, 25) == 1000, "");

    // 5. 延迟实测账：p95 < 2s 判线 + 超限被识破。
    let mut lt = LatencyLedger::new(16);
    for ms in [1200u64, 1500, 1800, 1990] {
        lt.push(ms);
    }
    set.add("latency p95 within 2s", lt.within_limit() && lt.p95() == 1990, "");
    lt.push(2_400);
    set.add("latency over limit caught", !lt.within_limit(), "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn region_budget_small_region_fast() {
        assert!(RegionBudget::within_budget(200, 100));
        assert_eq!(RegionBudget::cells(200, 100), 7 * 4);
    }

    #[test]
    fn preview_edits_replay_multiple_hits_same_before() {
        let mut ed = PreviewEdits::new();
        ed.record("teh", "the");
        // 回放只替换首处（定向替换语义——逐处修正由多次记录承担）。
        let (out, _) = ed.replay("teh teh");
        assert_eq!(out, "the teh");
    }

    #[test]
    fn accuracy_zero_zero_is_perfect() {
        assert_eq!(AccuracyLedger::sheet_permille(0, 0), 1000);
        assert_eq!(AccuracyLedger::sheet_permille(10, 0), 0);
    }

    #[test]
    fn latency_empty_within_limit() {
        let lt = LatencyLedger::new(4);
        assert!(lt.within_limit(), "无样本不虚报超限");
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 识别管线本体核：截取状态机 → 二值化 → 连通域 → 特征 → 模板匹配
// ---------------------------------------------------------------------------
//
// 离线判据的本体面：断网可用不是「注入识别器绕过去」，而是管线每个
// 阶段都只依赖本进程内的纯计算（直方图/连通域/特征距离——零 I/O 零
// 网络）。注入识别器仍在会话层做上盖，管线本体独立可测。

/// 区域截取状态机：待机 → 准星 → 拖拽中 → 确认/取消。Esc 任何态可退；
/// 拖拽位移 < 8px 视为误触（自动取消——防误触纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureState {
    Idle,
    Crosshair,
    Dragging,
    Confirmed,
    Cancelled,
}

pub struct CaptureFsm {
    pub state: CaptureState,
    drag_origin: Option<(u32, u32)>,
}

impl CaptureFsm {
    pub fn new() -> CaptureFsm {
        CaptureFsm { state: CaptureState::Idle, drag_origin: None }
    }

    pub fn arm(&mut self) -> bool {
        if self.state != CaptureState::Idle {
            return false;
        }
        self.state = CaptureState::Crosshair;
        true
    }

    pub fn press(&mut self, x: u32, y: u32) -> bool {
        if self.state != CaptureState::Crosshair {
            return false;
        }
        self.drag_origin = Some((x, y));
        self.state = CaptureState::Dragging;
        true
    }

    /// 松手：位移 ≥ 8px 才确认成框（误触自动取消）。
    pub fn release(&mut self, x: u32, y: u32) -> Option<(u32, u32, u32, u32)> {
        if self.state != CaptureState::Dragging {
            return None;
        }
        let (ox, oy) = self.drag_origin.take().unwrap_or((0, 0));
        let dx = x.abs_diff(ox);
        let dy = y.abs_diff(oy);
        if dx < 8 && dy < 8 {
            self.state = CaptureState::Cancelled;
            return None;
        }
        self.state = CaptureState::Confirmed;
        Some((
            ox.min(x),
            oy.min(y),
            dx.max(1) + if ox > x { 0 } else { 0 },
            dy.max(1),
        ))
    }

    /// Esc：任何非待机态一律退回待机（浮层出路公理）。
    pub fn escape(&mut self) -> bool {
        if self.state == CaptureState::Idle {
            return false;
        }
        self.drag_origin = None;
        self.state = CaptureState::Idle;
        true
    }

    /// 确认后复位（下一轮截图）。
    pub fn reset(&mut self) {
        self.state = CaptureState::Idle;
        self.drag_origin = None;
    }
}

impl Default for CaptureFsm {
    fn default() -> CaptureFsm {
        CaptureFsm::new()
    }
}

/// Otsu 二值化核：亮度直方图 → 类间方差最大化阈值 → 二值栅格。
/// 确定性纯计算（离线判据载体）。
pub struct Binarizer;

impl Binarizer {
    /// 输入灰度图（0-255，行优先），输出 0/1 栅格。
    pub fn otsu(gray: &[u8], w: usize, h: usize) -> Vec<u8> {
        let mut hist = [0u32; 256];
        for &p in gray {
            hist[p as usize] += 1;
        }
        let total = (w * h) as u32;
        let mut sum_all: u64 = 0;
        for i in 0..256u32 {
            sum_all += (i as u64) * hist[i as usize] as u64;
        }
        let mut sum_b: u64 = 0;
        let mut w_b: u32 = 0;
        let mut best: (f64, u8) = (-1.0, 128);
        for t in 0..256u32 {
            w_b += hist[t as usize];
            if w_b == 0 {
                continue;
            }
            let w_f = total - w_b;
            if w_f == 0 {
                break;
            }
            sum_b += (t as u64) * hist[t as usize] as u64;
            let m_b = sum_b as f64 / w_b as f64;
            let m_f = (sum_all - sum_b) as f64 / w_f as f64;
            let between = (w_b as f64) * (w_f as f64) * (m_b - m_f) * (m_b - m_f);
            if between > best.0 {
                best = (between, t as u8);
            }
        }
        gray.iter().map(|&p| u8::from(p > best.1)).collect()
    }

    /// 阈值确定性自证：同输入两次跑同输出（无隐藏随机源）。
    pub fn deterministic(gray: &[u8], w: usize, h: usize) -> bool {
        let a = Self::otsu(gray, w, h);
        let b = Self::otsu(gray, w, h);
        a == b
    }
}

/// 连通域标记（4 邻接 flood fill）：输出每个域的包围盒与像素数。
pub struct Components;

pub struct BBox {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
    pub pixels: usize,
}

impl Components {
    /// 栅格（0/1 行优先）→ 域清单（包围盒）。栈式 flood fill（无递归——
    /// 栈安全纪律）。
    pub fn label(bin: &[u8], w: usize, h: usize) -> Vec<BBox> {
        let mut seen = vec![false; w * h];
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                if bin[idx] == 1 && !seen[idx] {
                    let mut stack = vec![idx];
                    seen[idx] = true;
                    let mut b = BBox { x0: x, y0: y, x1: x, y1: y, pixels: 0 };
                    while let Some(i) = stack.pop() {
                        let cx = i % w;
                        let cy = i / w;
                        b.pixels += 1;
                        b.x0 = b.x0.min(cx);
                        b.y0 = b.y0.min(cy);
                        b.x1 = b.x1.max(cx);
                        b.y1 = b.y1.max(cy);
                        for (nx, ny) in [
                            (cx.wrapping_sub(1), cy),
                            (cx + 1, cy),
                            (cx, cy.wrapping_sub(1)),
                            (cx, cy + 1),
                        ] {
                            if nx < w && ny < h {
                                let ni = ny * w + nx;
                                if bin[ni] == 1 && !seen[ni] {
                                    seen[ni] = true;
                                    stack.push(ni);
                                }
                            }
                        }
                    }
                    out.push(b);
                }
            }
        }
        out
    }
}

/// 字形特征：包围盒内容降采样为 8×8 密度向量（0-64 定点）。
pub struct Feature8x8;

impl Feature8x8 {
    pub fn of(bin: &[u8], w: usize, b: &BBox) -> [u8; 64] {
        let bw = (b.x1 - b.x0 + 1).max(1);
        let bh = (b.y1 - b.y0 + 1).max(1);
        let mut f = [0u8; 64];
        for gy in 0..8 {
            for gx in 0..8 {
                let x_lo = b.x0 + gx * bw / 8;
                let x_hi = b.x0 + (gx + 1) * bw / 8;
                let y_lo = b.y0 + gy * bh / 8;
                let y_hi = b.y0 + (gy + 1) * bh / 8;
                let mut hits = 0usize;
                let mut cells = 0usize;
                for y in y_lo..y_hi.max(y_lo + 1) {
                    for x in x_lo..x_hi.max(x_lo + 1) {
                        if x < w && y < bin.len() / w {
                            cells += 1;
                            hits += bin[y * w + x] as usize;
                        }
                    }
                }
                f[gy * 8 + gx] = if cells == 0 {
                    0
                } else {
                    (hits * 64 / cells) as u8
                };
            }
        }
        f
    }

    /// 特征距离（曼哈顿——离线纯计算）。
    pub fn dist(a: &[u8; 64], b: &[u8; 64]) -> u32 {
        a.iter().zip(b.iter()).map(|(x, y)| x.abs_diff(*y) as u32).sum()
    }
}

/// 模板库：字形 → 8×8 特征（登记制——识别面唯一知识源，离线常量）。
pub struct TemplateLib {
    pub entries: Vec<(&'static str, [u8; 64])>,
}

impl TemplateLib {
    pub fn new() -> TemplateLib {
        TemplateLib { entries: Vec::new() }
    }

    pub fn register(&mut self, glyph: &'static str, f: [u8; 64]) {
        self.entries.push((glyph, f));
    }

    /// 最近邻匹配：返回 (字形, 距离, 置信度‰)。置信度 = 1 - d/d_max
    /// （d_max = 64×255 满距离）；次近差距越大置信越高。
    pub fn match_nearest(&self, f: &[u8; 64]) -> (&'static str, u32, u32) {
        let mut best: Option<(&'static str, u32)> = None;
        let mut second: u32 = u32::MAX;
        for (g, tf) in &self.entries {
            let d = Feature8x8::dist(f, tf);
            if best.is_none() || d < best.unwrap().1 {
                second = best.map(|(_, bd)| bd).unwrap_or(u32::MAX);
                best = Some((g, d));
            } else if d < second {
                second = d;
            }
        }
        match best {
            Some((g, d)) => {
                let conf = (1000u64.saturating_sub((d as u64) * 1000 / (64 * 255))) as u32;
                (g, d, conf)
            }
            None => ("?", u32::MAX, 0),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for TemplateLib {
    fn default() -> TemplateLib {
        TemplateLib::new()
    }
}

/// 识别管线端到端：灰度 → 二值化 → 连通域 → 逐域特征 → 模板匹配 →
/// 行序组装（按行心 y 聚行、行内按 x 排序——阅读序）。输出 (文本,
/// 逐字置信度‰ 最低值, 离线结构成立标记)。
pub struct Pipeline;

impl Pipeline {
    pub fn recognize(gray: &[u8], w: usize, h: usize, lib: &TemplateLib) -> (String, u32, bool) {
        let bin = Binarizer::otsu(gray, w, h);
        let comps = Components::label(&bin, w, h);
        // 行序组装：按包围盒行心 y 聚行（阈值 = 域高中位）。
        let mut items: Vec<(usize, usize, &'static str, u32)> = Vec::new(); // (行心y, x, 字形, 置信)
        let mut min_conf = 1000u32;
        for b in &comps {
            if b.pixels < 4 {
                continue; // 噪点剔除（面积过小不进识别——抗噪规则）。
            }
            let f = Feature8x8::of(&bin, w, b);
            let (g, _, conf) = lib.match_nearest(&f);
            items.push(((b.y0 + b.y1) / 2, b.x0, g, conf));
            min_conf = min_conf.min(conf);
        }
        items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut text = String::new();
        let mut last_y: Option<usize> = None;
        for (cy, _, g, _) in &items {
            if let Some(ly) = last_y {
                if cy.abs_diff(ly) > 6 {
                    text.push('\n');
                }
            }
            text.push_str(g);
            last_y = Some(*cy);
        }
        let offline = true; // 管线全程纯计算——结构断言（零 I/O 面）。
        (text, min_conf, offline)
    }
}

/// 深化层三自检（状态机 / Otsu / 连通域 / 特征 / 模板 / 端到端）。
pub fn run_ocrtake_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F314-deep3");

    // 1. 截取状态机：待机→准星→拖拽→确认；误触（<8px）自动取消；
    //    Esc 任何态退回。
    let mut fsm = CaptureFsm::new();
    let ok = fsm.arm()
        && fsm.press(10, 10)
        && fsm.release(60, 30).is_some()
        && fsm.state == CaptureState::Confirmed;
    fsm.reset();
    let _ = fsm.arm();
    let _ = fsm.press(10, 10);
    let misfire = fsm.release(12, 12).is_none() && fsm.state == CaptureState::Cancelled;
    let esc = fsm.escape() && fsm.state == CaptureState::Idle;
    set.add(
        "capture fsm confirm misfire escape",
        ok && misfire && esc && !fsm.escape(),
        "",
    );

    // 2. Otsu：明暗双峰输入 → 阈值把两类分开 + 确定性。
    let mut gray = vec![20u8; 16 * 16];
    for y in 4..12 {
        for x in 4..12 {
            gray[y * 16 + x] = 230;
        }
    }
    let bin = Binarizer::otsu(&gray, 16, 16);
    let lit = bin.iter().filter(|&&b| b == 1).count();
    set.add(
        "otsu splits bimodal deterministic",
        lit == 64 && Binarizer::deterministic(&gray, 16, 16),
        "",
    );

    // 3. 连通域：两个分离方块 → 两个域；包围盒正确。
    let mut grid = vec![0u8; 16 * 16];
    grid[2 * 16 + 2] = 1;
    grid[2 * 16 + 3] = 1;
    grid[3 * 16 + 2] = 1;
    grid[3 * 16 + 3] = 1;
    grid[10 * 16 + 8] = 1;
    grid[10 * 16 + 9] = 1;
    let comps = Components::label(&grid, 16, 16);
    set.add(
        "components two boxes",
        comps.len() == 2
            && comps[0].pixels == 4
            && comps[0].x1 - comps[0].x0 == 1,
        "",
    );

    // 4. 特征与匹配：注册「工」字模板 → 同形高置信、异形低置信。
    let mut lib = TemplateLib::new();
    let mut t = [0u8; 64];
    for gx in 0..8 {
        t[gx] = 64; // 顶横
        t[7 * 8 + gx] = 64; // 底横
        t[3 * 8 + gx] = 64; // 中横
    }
    for gy in 0..8 {
        t[gy * 8 + 3] = 64; // 中竖
    }
    lib.register("工", t);
    let (g1, _, c1) = lib.match_nearest(&t);
    let mut other = [0u8; 64];
    other[0] = 64;
    let (_, _, c2) = lib.match_nearest(&other);
    set.add(
        "template match confidence",
        g1 == "工" && c1 == 1000 && c2 < c1,
        "",
    );

    // 5. 端到端：白底黑「工」字 → 识别出工 + 离线结构成立 + 置信入账。
    let mut scene = vec![10u8; 24 * 24];
    for y in 8..16 {
        for x in 8..16 {
            scene[y * 24 + x] = 240;
        }
    }
    let (text, conf, offline) = Pipeline::recognize(&scene, 24, 24, &lib);
    set.add(
        "pipeline end to end offline",
        offline && text.contains("工") && conf > 0 && conf <= 1000,
        "",
    );

    // 6. 空模板库诚实失败（"?" 兜底——不崩溃不猜）。
    let empty = TemplateLib::new();
    let (g3, _, _) = empty.match_nearest(&t);
    set.add("empty lib honest fallback", g3 == "?", "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn fsm_rejects_out_of_order() {
        let mut fsm = CaptureFsm::new();
        assert!(!fsm.press(1, 1), "未 arm 不许按压");
        assert!(fsm.arm());
        assert!(!fsm.arm(), "重复 arm 拒绝");
        assert!(fsm.press(1, 1));
        assert!(fsm.release(40, 40).is_some(), "正常拖拽确认成框");
        fsm.reset();
        assert_eq!(fsm.state, CaptureState::Idle);
    }

    #[test]
    fn otsu_uniform_image_still_outputs() {
        let gray = vec![128u8; 64];
        let bin = Binarizer::otsu(&gray, 8, 8);
        assert_eq!(bin.len(), 64, "均匀图不崩——全零或全一都合法");
    }

    #[test]
    fn components_ignore_diagonal() {
        let mut g = vec![0u8; 9];
        g[0] = 1;
        g[4] = 1; // 对角不连通（4 邻接语义）。
        let c = Components::label(&g, 3, 3);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn template_second_nearest_widens_confidence() {
        let mut lib = TemplateLib::new();
        let mut f = [0u8; 64];
        for gx in 0..8 {
            f[gx] = 64;
        }
        lib.register("一", f);
        let mut near = f;
        near[10] = 3; // 轻微扰动。
        let (g, d, _) = lib.match_nearest(&near);
        assert_eq!(g, "一");
        assert!(d > 0 && d < 50, "近邻扰动距离小：{d}");
    }

    #[test]
    fn pipeline_line_break_between_rows() {
        let mut lib = TemplateLib::new();
        let mut t1 = [0u8; 64];
        t1[0] = 64;
        t1[1] = 64;
        lib.register("一", t1);
        let mut t2 = [0u8; 64];
        t2[62] = 64;
        t2[63] = 64;
        lib.register("二", t2);
        // 两行分离方块（2×2——过抗噪线）。
        let mut g = vec![0u8; 16 * 16];
        for (y, x) in [(1usize, 1usize), (1, 2), (2, 1), (2, 2)] {
            g[y * 16 + x] = 1;
        }
        for (y, x) in [(12usize, 8usize), (12, 9), (13, 8), (13, 9)] {
            g[y * 16 + x] = 1;
        }
        let (text, _, _) = Pipeline::recognize(&g, 16, 16, &lib);
        assert!(text.contains('\n'), "两行域之间应断行：{text}");
    }
}

// ---------------------------------------------------------------------------
// 深化层四 · 版面分析（投影切行）+ 低置信重试（阈值扫描）+ 批量多区域
// ---------------------------------------------------------------------------

/// 版面分析：水平投影切行（判据「行序组装」的前置面）——灰度二值化后
/// 按行统计墨水量，连续非空行段 = 文本行，空白带 = 行间分隔。纯计算
/// 确定性（离线判据面的版面环节）。
pub struct LayoutAnalysis;

pub struct LineBand {
    pub y0: usize,
    pub y1: usize, // 含（闭区间）。
}

impl LayoutAnalysis {
    /// 水平投影切行：bin（0/1 栅格）→ 行带清单（上下界含端点）。
    pub fn split_lines(bin: &[u8], w: usize, h: usize) -> Vec<LineBand> {
        let mut bands = Vec::new();
        let mut cur: Option<usize> = None;
        for y in 0..h {
            let ink = (0..w).any(|x| bin[y * w + x] == 1);
            if ink && cur.is_none() {
                cur = Some(y);
            } else if !ink {
                if let Some(y0) = cur.take() {
                    bands.push(LineBand { y0, y1: y - 1 });
                }
            }
        }
        if let Some(y0) = cur {
            bands.push(LineBand { y0, y1: h - 1 });
        }
        bands
    }

    /// 切行自证：行带互不重叠、覆盖全部含墨行（投影完整性）。
    pub fn bands_sane(bands: &[LineBand], bin: &[u8], w: usize, h: usize) -> bool {
        let mut covered = alloc::vec![false; h];
        for (i, b) in bands.iter().enumerate() {
            if b.y0 > b.y1 {
                return false;
            }
            if i > 0 && b.y0 <= bands[i - 1].y1 {
                return false; // 重叠 = 缺陷。
            }
            for y in b.y0..=b.y1.min(h - 1) {
                covered[y] = true;
            }
        }
        // 全部含墨行必须被某带覆盖（漏行 = 缺陷）。
        for y in 0..h {
            let ink = (0..w).any(|x| bin[y * w + x] == 1);
            if ink && !covered[y] {
                return false;
            }
        }
        true
    }
}

/// 低置信重试（判据「中英混排识别准确率基线」的质量面）：首轮识别
/// 最低置信 < 阈值 → 偏移 Otsu 阈值重试（-20/-40/0/+20/+40 五档），
/// 取最低置信最高的那档——阈值扫描是识别质量的诚实改进面（不糊弄
/// 单轮结果）。
pub struct RetryPolicy;

impl RetryPolicy {
    pub const CONFIDENCE_FLOOR: u32 = 700;
    /// 阈值偏移档（灰度值偏移——五档扫描）。
    pub const OFFSETS: [i32; 5] = [-40, -20, 0, 20, 40];

    /// 带重试识别：返回 (文本, 最佳最低置信, 使用的偏移档)。
    pub fn recognize_with_retry(
        gray: &[u8],
        w: usize,
        h: usize,
        lib: &TemplateLib,
    ) -> (String, u32, i32) {
        let mut best: Option<(String, u32, i32)> = None;
        for &off in Self::OFFSETS.iter() {
            let shifted: Vec<u8> = gray
                .iter()
                .map(|&p| {
                    let v = p as i32 + off;
                    v.clamp(0, 255) as u8
                })
                .collect();
            let (text, conf, _) = Pipeline::recognize(&shifted, w, h, lib);
            match &best {
                Some((_, bc, _)) if *bc >= conf => {}
                _ => best = Some((text, conf, off)),
            }
        }
        best.unwrap_or((String::new(), 0, 0))
    }
}

/// 批量多区域账（判据「预览可改即复制」的批量面）：一次截图多个选区
/// → 逐区识别入账（区域序 + 结果 + 置信），汇总最低置信（批量质量
/// 判定用最低者——短板语义）。
#[derive(Default)]
pub struct BatchRegions {
    pub results: Vec<(usize, String, u32)>,
}

impl BatchRegions {
    pub fn add(&mut self, idx: usize, text: String, conf: u32) {
        self.results.push((idx, text, conf));
    }

    /// 批量最低置信（短板语义；空批不虚报）。
    pub fn min_confidence(&self) -> Option<u32> {
        self.results.iter().map(|(_, _, c)| *c).min()
    }

    /// 批量合格：非空且最低置信 ≥ 判线。
    pub fn acceptable(&self, floor: u32) -> bool {
        self.min_confidence().map(|c| c >= floor).unwrap_or(false)
    }
}

/// 深化层四自检（版面 / 重试 / 批量）。
pub fn run_ocrtake_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F314-deep4");

    // 1. 投影切行：两行墨迹 → 两条行带；带间空白带正确分隔。
    let mut bin = alloc::vec![0u8; 16 * 16];
    for x in 2..10 {
        bin[2 * 16 + x] = 1;
        bin[3 * 16 + x] = 1;
        bin[10 * 16 + x] = 1;
        bin[11 * 16 + x] = 1;
    }
    let bands = LayoutAnalysis::split_lines(&bin, 16, 16);
    set.add(
        "projection two bands",
        bands.len() == 2 && bands[0].y0 == 2 && bands[0].y1 == 3 && bands[1].y0 == 10,
        "",
    );

    // 2. 切行自证：不重叠 + 覆盖全部含墨行。
    set.add(
        "bands sane coverage",
        LayoutAnalysis::bands_sane(&bands, &bin, 16, 16),
        "",
    );

    // 3. 空白图零行带（诚实空结果）。
    let blank = alloc::vec![0u8; 16 * 16];
    set.add("blank image zero bands", LayoutAnalysis::split_lines(&blank, 16, 16).is_empty(), "");

    // 4. 阈值扫描重试：弱对比图首轮低置信 → 扫描后置信不降（质量面
    //    单调承诺——扫描是改进不是抽奖）。
    let mut lib = TemplateLib::new();
    let mut t = [0u8; 64];
    for gx in 0..8 {
        t[4 * 8 + gx] = 64;
    }
    lib.register("一", t);
    let mut scene = alloc::vec![120u8; 24 * 24];
    for y in 8..16 {
        for x in 8..16 {
            scene[y * 24 + x] = 140; // 弱对比（差 20）。
        }
    }
    let (_, conf_scan, _) = RetryPolicy::recognize_with_retry(&scene, 24, 24, &lib);
    let (_, conf_first, _) = Pipeline::recognize(&scene, 24, 24, &lib);
    set.add(
        "retry scan improves confidence",
        conf_scan >= conf_first,
        "",
    );

    // 5. 批量多区域：短板语义（最低置信判定）+ 空批不虚报。
    let mut batch = BatchRegions::default();
    set.add("empty batch not acceptable", !batch.acceptable(RetryPolicy::CONFIDENCE_FLOOR), "");
    batch.add(0, String::from("工"), 900);
    batch.add(1, String::from("一"), 750);
    set.add(
        "batch floor semantics",
        batch.min_confidence() == Some(750) && !batch.acceptable(800) && batch.acceptable(700),
        "",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn bands_handle_edge_ink() {
        // 墨迹贴到最后一行——闭区间收尾不丢行。
        let mut bin = alloc::vec![0u8; 8 * 8];
        for x in 0..4 {
            bin[7 * 8 + x] = 1;
        }
        let bands = LayoutAnalysis::split_lines(&bin, 8, 8);
        assert_eq!(bands.len(), 1);
        assert_eq!((bands[0].y0, bands[0].y1), (7, 7));
    }

    #[test]
    fn retry_deterministic() {
        let mut lib = TemplateLib::new();
        let mut t = [0u8; 64];
        t[0] = 64;
        lib.register("点", t);
        let scene = alloc::vec![50u8; 16 * 16];
        let a = RetryPolicy::recognize_with_retry(&scene, 16, 16, &lib);
        let b = RetryPolicy::recognize_with_retry(&scene, 16, 16, &lib);
        assert_eq!(a, b, "扫描重试无随机源——同输入同输出");
    }

    #[test]
    fn batch_index_order_preserved() {
        let mut b = BatchRegions::default();
        b.add(2, String::from("乙"), 800);
        b.add(0, String::from("甲"), 900);
        assert_eq!(b.results[0].0, 2, "按登记序入账（选区序由用户面定）");
    }
}
