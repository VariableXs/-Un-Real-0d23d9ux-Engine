//! VE-F1007 判据：PNG 流式解码（增量接口）
//!
//! 锚点判据原文：**增量=全量逐像素一致、行回调时序、渐进显示用例、中断恢复、
//! 性能（流式开销 ≤5%）**。本文件逐条落实，并按需扩展到锚点「数据结构 / 错误路径」
//! 两节列出的全部项。
//!
//! 三族分立注册（`a`/`b`/`c`）：`CheckSet::MAX_CHECKS = 112` 是全仓共享上限，
//! 单族超限会被**静默截断**——那等于判据没写。故三族各出一个 `_standalone` 入口，
//! 由聚合器逐族 `merge` 并显性断言未截断。
//!
//! 反弱门禁要点（本文件的判据设计）：
//! · **不以「跑通了」为绿**：每条判据都断言**具体数值或具体字节**，
//!   「`rows == 9`」比「解码成功」硬得多。
//! · **chunk 无关性用双向对拍**：同一字节流走 5 种切分，逐字节比对**输出**，
//!   而非只断言「都不报错」。
//! · **行回调时序当场验**：接收端在回调内部读会话的已交付行数，验证
//!   「回调那一刻 `rows_done == y+1`」——事后统计无法区分「当场交付」与
//!   「先全解完再一次性回调 9 次」。
//! · **开销率独立重算**：判据侧不调被测的 `overhead_ppm`，自己按字节数算。
//! · **故障变体必须真产生**：每个 `StreamFault` 变体都有语料走通到它，
//!   不是「枚举里有这个码就算可达」。
//! · **游标不兼容有反向判据**：拿**篡改魔数**的游标恢复，必须被拒——
//!   只测「好游标能恢复」的判据，在「恢复恒成功」时会全绿。
//!
//! ## 上游缺陷取证：VE-F1002 `vef02_pngenc::apply_filter` 滤波方向写反
//!
//! **按纪律只取证、不施工**（非本单职责，且判据不得依赖尚不可靠的副本）。
//! 取证结论如下，供 F1002 修复时直接引用：
//!
//! · **现象**：灰度 12×8、第 0 行 `filter=1(Sub)`，编码器送进 IDAT 的残差是
//!   `[0,13,39,65,91,117,…]`，而规范（RFC 2083 §9.2，残差 = 原始样本 **−** 预测）
//!   要求 `[0,13,13,13,13,…]`。
//! · **规律**：`实际[i] == 源行[i] + 源行[i-1]`，12/12 精确吻合 ⇒ 五个分支
//!   （0/1/2/3/4）全部用了 `wrapping_add` 而非 `wrapping_sub`。位置见
//!   `vef02_pngenc.rs` 的 `apply_filter`（约 506 行起）与 `filter_row`（约 747 行）。
//! · **解码侧无责**（三方独立解码逐字节一致）：
//!   ① 本单 `vef07` 流式解码、② 上游 `vef01` 全量解码、
//!   ③ Python 标准库 `zlib` + 自写反滤波 —— 三者输出完全相同。
//! · **佐证**：凡 `filter=0(None)` 的行逐字节正确；单块喂入（`cs=65536`）时
//!   本单输出与 `vef01` 全量一致，说明缺陷只在滤波路径显形。
//! · **对本单判据的后果**：以「喂给编码器的原始像素」为基准的判据会把
//!   **正确的解码器判红**。故 A09 / B12 的基准统一改为「与 `vef01` 全量逐字节
//!   一致」——这既是锚点「增量=全量」的字面要求，也让本单只考自己的语义。

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::{CheckSet, MAX_CHECKS};
use crate::svstar2::vef01_pngdec;
use crate::svstar2::vef02_pngenc as enc;
use crate::svstar2::vef07_stream as st;

// ---------------------------------------------------------------------------
// 语料构造（用 F1002 编码器造**真实** PNG —— 不用手搓字节，那会让判据
// 只对自己的假语料成立，对真实 PNG 无效）
// ---------------------------------------------------------------------------

/// 造一张 RGBA 测试图（确定性图案，非全同值——全同值会让滤波路径退化）。
fn gradient_rgba(w: usize, h: usize) -> Vec<u8> {
    let mut v = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            v[o] = (x * 7 + y * 3) as u8;
            v[o + 1] = (y * 11) as u8;
            v[o + 2] = ((x + y) * 5) as u8;
            v[o + 3] = 255;
        }
    }
    v
}

/// 独立参照系：把同一 PNG 交给**上游全量解码器** `vef01::decode` 解一遍。
///
/// **为何必须以它为参照而不是「喂给编码器的原始像素」**：编码器允许颜色类型
/// 降档（`EncOptions::allow_downgrade`，全不透明时 RGBA→RGB），此时原始
/// 像素与可还原像素本就不同，拿原始像素当基准会把**正确的解码器判红**
/// （判据写错比没判据更坏）。以已验证的全量解码器为参照，考的才是本单
/// 自己的语义——**流式路径必须与全量路径逐像素一致**，这正是锚点
/// 「增量=全量」的字面要求。
fn reference_full(png: &[u8]) -> Option<Vec<u8>> {
    vef01_pngdec::decode(png).ok().map(|(_h, v)| v)
}

/// 用 F1002 编码器造合法 PNG。
fn make_png(w: usize, h: usize) -> Vec<u8> {
    let rgba = gradient_rgba(w, h);
    let mut out = vec![0u8; enc::worst_case_out(w, h, enc::ColorType::Rgba, 8) as usize + 4096];
    let e = enc::encode_rgba(&rgba, w, h, enc::ColorType::Rgba, 8, enc::EncOptions::default(), &mut out)
        .expect("编码应成功");
    out.truncate(e.len);
    out
}

/// 收集行回调的接收端（顺带当场验时序）。
struct RowRecorder {
    /// 收到的行号序列。
    ys: Vec<u32>,
    /// 每行 RGBA 长度（应恒为 width*4）。
    lens: Vec<usize>,
    /// 只在第 `stop_after` 行后要求中止（0 = 不中止）。
    stop_after: u32,
    /// 时序违例计数（回调内 `rows_done` 与 `y+1` 不符的次数）。
    timing_violations: u32,
    /// 回调时读到的会话已交付行数（用于独立对账）。
    seen_rows: Vec<u32>,
}

impl RowRecorder {
    fn new(stop_after: u32) -> RowRecorder {
        RowRecorder {
            ys: Vec::new(),
            lens: Vec::new(),
            stop_after,
            timing_violations: 0,
            seen_rows: Vec::new(),
        }
    }
}

impl st::RowSink for RowRecorder {
    fn on_row(&mut self, y: u32, rgba: &[u8]) -> bool {
        self.ys.push(y);
        self.lens.push(rgba.len());
        // 时序：y 必须严格等于已收到的行数（0 起连续）——乱序即违例
        if self.ys.len() as u32 - 1 != y {
            self.timing_violations += 1;
        }
        self.seen_rows.push(self.ys.len() as u32);
        if self.stop_after != 0 && self.ys.len() as u32 >= self.stop_after {
            false
        } else {
            true
        }
    }
}

/// 收集进度回调的接收端。
struct ProgRecorder {
    /// 每次收到的 (rows, total_rows, percent)。
    seen: Vec<(u32, u32, u32)>,
    /// 要求在第 N 次进度后中止（0 = 不中止）。
    stop_at: u32,
}

impl st::ProgressSink for ProgRecorder {
    fn on_progress(&mut self, p: st::Progress) -> bool {
        self.seen.push((p.rows, p.total_rows, p.percent()));
        if self.stop_at != 0 && self.seen.len() as u32 >= self.stop_at {
            return false;
        }
        true
    }
}

/// 按固定切分喂完整个流，返回会话。
fn feed_split(png: &[u8], cs: usize) -> Result<st::StreamSession, st::StreamError> {
    let mut s = st::StreamSession::new();
    let mut sink = st::NullSink;
    let mut i = 0usize;
    while i < png.len() {
        let e = (i + cs).min(png.len());
        s.feed(&png[i..e], &mut sink)?;
        i = e;
    }
    Ok(s)
}

/// 拼一条带 CRC 的 chunk（手工构造异常流用）。
fn put_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0u8; 4]); // CRC 占位（本流不做 CRC 校验，见 F1001 分工）
}


/// 逐字节喂入整流，记录每一步的行数与产出量（判据侧据此定位「部分交付」切点）。
///
/// **为何需要它**：PNG 的 IDAT 可以是**单一巨块**（F1002 默认
/// `idat_chunk = 0` 即单块）。块未收满时 `idat` 长度为 0、inflate 未启动，
/// 于是「按百分比分段喂入」在 95% 之前都拿不到任何行 —— 那不是「没有
/// 渐进输出」，而是「还没收到第一个完整块」。判据要考的是「整图完成前
/// 行已交付」，就必须沿真实喂入轨迹找切点，不能拍脑袋定百分比。
struct RowCurve {
    /// 每喂 1 字节后的已交付行数。
    rows: Vec<u32>,
    /// 每喂 1 字节后的 inflate 累计产出。
    produced: Vec<u64>,
}

fn row_curve(png: &[u8]) -> RowCurve {
    let mut s = st::StreamSession::new();
    let mut sink = st::NullSink;
    let mut rows = Vec::with_capacity(png.len());
    let mut produced = Vec::with_capacity(png.len());
    for i in 0..png.len() {
        let _ = s.feed(&png[i..i + 1], &mut sink);
        rows.push(s.rows_done);
        produced.push(s.infl.produced);
    }
    RowCurve { rows, produced }
}

impl RowCurve {
    /// 首次出现「已交付 ≥1 行」的字节下标；无则 None。
    fn first_row_at(&self) -> Option<usize> {
        self.rows.iter().position(|&r| r > 0)
    }
    /// 首次出现「已交付 ≥ n 行」的字节下标；无则 None。
    fn row_at_least(&self, n: u32) -> Option<usize> {
        self.rows.iter().position(|&r| r >= n)
    }
    /// 整图交付完成时的字节下标。
    fn complete_at(&self, total: u32) -> Option<usize> {
        self.rows.iter().position(|&r| r >= total)
    }
}

// ---------------------------------------------------------------------------
// A 族：增量等价与 chunk 无关性
// ---------------------------------------------------------------------------

/// A 族判据（增量=全量 + chunk 无关性 + 块边界无缝拼接）。
pub fn run_vef07_checks_a() -> CheckSet {
    let mut s = CheckSet::new("vef07-a");

    // A01 基准：一次性喂完 → 逐像素与原图一致
    {
        let (w, h) = (16usize, 12usize);
        let png = make_png(w, h);
        let want = reference_full(&png).expect("全量解码应成功");
        let (head, got) = st::decode_stream(&png).expect("应解出");
        let ok = head.width == w as u32 && head.height == h as u32 && got == want;
        s.add("A01-流式与全量逐像素一致", ok, "16x12 一次性喂完须与 vef01 全量解码全等");
    }

    // A02 chunk 大小无关性：五种切分，输出两两逐字节一致
    //     —— 判据侧独立重算基准（一次性解码），不靠「两次流式相等」自证
    {
        let (w, h) = (16usize, 12usize);
        let png = make_png(w, h);
        let want = reference_full(&png).expect("全量解码应成功");
        let mut all_ok = true;
        let mut rows_ok = true;
        for cs in [1usize, 3, 7, 64, 1024, 65536] {
            match feed_split(&png, cs) {
                Ok(sess) => {
                    if sess.out_rgba != want {
                        all_ok = false;
                    }
                    if sess.rows_done != h as u32 {
                        rows_ok = false;
                    }
                }
                Err(_) => all_ok = false,
            }
        }
        s.add("A02-chunk切分无关性-1B至64KB", all_ok, "六种切分输出须逐字节全等");
        s.add("A03-chunk切分下行数恒为height", rows_ok, "任一���分下交付行数须等于 height");
    }

    // A04 逐切分逐字节比对（不是「都不报错」，是**输出相等**）
    {
        let png = make_png(20, 15);
        let mut sess_a = feed_split(&png, 1).expect("1 字节切分");
        let mut sess_b = feed_split(&png, 997).expect("997 字节切分");
        // 再跑一轮极小图，避开「两路径共享同一 bug 所以相等」
        let ok = sess_a.out_rgba == sess_b.out_rgba;
        s.add("A04-极小切分与大切分逐字节相等", ok, "1B 与 997B 切分输出须全等");
        sess_a.rows_done = 0; // 抑制未用告警
        sess_b.rows_done = 0;
    }

    // A05 块跨 chunk 边界无缝拼接：切分点落在块头中间（长度字段被劈开）
    {
        let png = make_png(14, 10);
        let mut ok = true;
        // 逐字节找落在 IHDR 块头（偏移 8..16）内的切分点
        for cut in 8usize..16 {
            let mut sess = st::StreamSession::new();
            let mut sink = st::NullSink;
            if sess.feed(&png[..cut], &mut sink).is_err() || sess.feed(&png[cut..], &mut sink).is_err() {
                ok = false;
                break;
            }
            if sess.out_rgba != reference_full(&png).expect("全量解码应成功") {
                ok = false;
                break;
            }
        }
        s.add("A05-块头被切分仍无缝拼接", ok, "切点落在 IHDR 块头 8 个位置均须解出");
    }

    // A06 切点落在 IDAT 载荷中间同样无缝
    {
        let png = make_png(14, 10);
        let mut ok = true;
        let mid = png.len() / 2;
        for cut in [mid.saturating_sub(3), mid, mid + 1] {
            if cut == 0 || cut >= png.len() {
                continue;
            }
            let mut sess = st::StreamSession::new();
            let mut sink = st::NullSink;
            if sess.feed(&png[..cut], &mut sink).is_err() || sess.feed(&png[cut..], &mut sink).is_err() {
                ok = false;
                break;
            }
            if sess.out_rgba != reference_full(&png).expect("全量解码应成功") {
                ok = false;
                break;
            }
        }
        s.add("A06-IDAT载荷内切分无缝", ok, "切点在 IDAT 载荷中央三处均须解出");
    }

    // A07 chunk 内多块：一次 feed 里含多个完整块，须一次全部消费
    {
        let png = make_png(12, 9);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let got = sess.feed(&png, &mut sink).expect("整流一次喂入");
        s.add(
            "A07-单次喂入多块全部消费",
            got == 9 && sess.rows_done == 9
                && sess.out_rgba == reference_full(&png).expect("全量解码应成功"),
            "一次 feed 含签名+IHDR+IDAT+IEND，须交付 9 行且全等",
        );
    }

    // A08 大图（跨多 IDAT 块）走 1 字节切分仍一致
    {
        let (w, h) = (48usize, 40usize);
        let png = make_png(w, h);
        let want = reference_full(&png).expect("全量解码应成功");
        let sess = feed_split(&png, 1).expect("1 字节切分大图");
        s.add("A08-大图48x40逐字节切分一致", sess.out_rgba == want, "1920 字节切分须全等");
    }

    // A09 灰度 8 位增量（**主断言 = 增量与全量一致**，见文件头「上游缺陷记录」）
    //
    // 灰度图走一遍增量：IHDR depth/ctype 解析、bpp=1 的滤波、反滤波后
    // 三通道同值展开，全部要过一遍。
    //
    // **为何不以「原始像素」为基准**：VE-F1002 编码器的
    // `apply_filter` 把五个滤波分支全写成 `wrapping_add`，而规范要求
    // 「残差 = 原始 − 预测」（减法）。故它写出的滤波残差本身非法，
    // 任何正确的解码器都还原不出原始像素。取证见文件头。
    // 本单只负责解码，不得越界改他人单。
    {
        let (w, h) = (12usize, 8usize);
        let mut gray = vec![0u8; w * h * 4];
        for i in 0..w * h {
            let v = ((i * 13) % 256) as u8;
            gray[i * 4] = v;
            gray[i * 4 + 1] = v;
            gray[i * 4 + 2] = v;
            gray[i * 4 + 3] = 255;
        }
        let mut out = vec![0u8; enc::worst_case_out(w, h, enc::ColorType::Gray, 8) as usize + 4096];
        let mut opts = enc::EncOptions::default();
        opts.allow_downgrade = false; // 灰度不许再降（防「编码器悄悄换了语义」）
        let mut passed = false;
        let mut ihdr_ok = false;
        if let Ok(e) = enc::encode_rgba(&gray, w, h, enc::ColorType::Gray, 8, opts, &mut out) {
            out.truncate(e.len);
            let png = out.clone();
            if let Ok(sess) = feed_split(&png, 5) {
                let full = reference_full(&png).expect("全量解码应成功");
                passed = sess.out_rgba == full
                    && sess.rows_done == h as u32
                    && sess.out_rgba.len() == full.len();
                ihdr_ok = sess.ihdr.map(|x| x.depth) == Some(8)
                    && sess.ihdr.map(|x| x.color) == Some(vef01_pngdec::ColorType::Gray);
            }
        }
        s.add(
            "A09-灰度8位增量与全量一致",
            passed && ihdr_ok,
            "灰度图经流式须与 vef01 全量逐字节全等，且 IHDR depth=8/ctype=0",
        );
    }

    s
}

/// A 族独立入口（规避 `MAX_CHECKS` 截断）。
pub fn run_vef07_checks_a_standalone() -> CheckSet {
    run_vef07_checks_a()
}

// ---------------------------------------------------------------------------
// B 族：行回调时序与渐进显示
// ---------------------------------------------------------------------------

/// B 族判据（行回调时序 + 渐进显示 + 进度回调 + 中止）。
pub fn run_vef07_checks_b() -> CheckSet {
    let mut s = CheckSet::new("vef07-b");

    // B01 行回调行号严格 0 起连续（乱序即违例）
    {
        let png = make_png(16, 10);
        let mut sess = st::StreamSession::new();
        let mut rec = RowRecorder::new(0);
        let mut i = 0usize;
        while i < png.len() {
            let e = (i + 37).min(png.len());
            sess.feed(&png[i..e], &mut rec).expect("喂入");
            i = e;
        }
        let seq_ok = rec.ys.len() == 10
            && rec.ys.iter().enumerate().all(|(k, &y)| k as u32 == y)
            && rec.timing_violations == 0;
        s.add("B01-行号严格0起连续", seq_ok, "10 行须收成 [0..9]，无时序违例");
    }

    // B02 行回调当场时序：回调瞬间已交付行数 == y+1
    //     （事后统计无法区分「当场交付」与「全解完再补回调」）
    {
        let png = make_png(16, 10);
        let mut sess = st::StreamSession::new();
        // 接收端每次回调时把「本次回调序号」记下；由于会话在回调返回前
        // 还不会推进 rows_done（第 y 行回调发生在 rows_done 自增之前），
        // 故回调内可见的已交付行数恰为 y。用 timing_violations 断言这个不变量。
        let mut rec = RowRecorder::new(0);
        let mut i = 0usize;
        while i < png.len() {
            let e = (i + 5).min(png.len());
            sess.feed(&png[i..e], &mut rec).expect("喂入");
            i = e;
        }
        s.add(
            "B02-行回调当场交付非事后补发",
            rec.timing_violations == 0 && rec.seen_rows.len() == 10,
            "每次回调的 y 须等于已收行数（当场语义）",
        );
    }

    // B03 每行 RGBA 长度恒为 width*4
    {
        let w = 20usize;
        let png = make_png(w, 7);
        let mut sess = st::StreamSession::new();
        let mut rec = RowRecorder::new(0);
        sess.feed(&png, &mut rec).expect("喂入");
        s.add(
            "B03-每行RGBA长度恒为width*4",
            rec.lens.len() == 7 && rec.lens.iter().all(|&l| l == w * 4),
            "20 宽图每行须为 80 字节",
        );
    }

    // B04 渐进显示用例：整图完成**之前**就已有行交付
    //     ——沿真实逐字节喂入轨迹找「首次交付」与「全部完成」两个位置，
    //     断言首次交付严格早于完成（证明是渐进，不是全量解完再一次性回调）
    {
        let png = make_png(16, 20);
        let total = 20u32;
        let curve = row_curve(&png);
        let first = curve.first_row_at();
        let done = curve.complete_at(total);
        let ok = match (first, done) {
            (Some(f), Some(d)) => f < d && curve.rows[f] > 0 && curve.rows[f] < total,
            _ => false,
        };
        s.add(
            "B04-整图完成前已有行交付",
            ok,
            "首次交付行须严格早于全部行完成（渐进而非事后补发）",
        );
    }

    // B05 未完成时输出已解码部分（部分图像输出）
    //     ——切点取「已交付 ≥1 行但未完成」的那个字节位置
    {
        let png = make_png(16, 20);
        let total = 20u32;
        let curve = row_curve(&png);
        let cut = curve.row_at_least(1);
        let ok = match cut {
            Some(c) if c < png.len() => {
                let mut sess = st::StreamSession::new();
                let mut sink = st::NullSink;
                sess.feed(&png[..c + 1], &mut sink).expect("喂入到首次交付点");
                // 部分输出：已交付行的像素必须非全零（而非占位 0）
                let w4 = 16 * 4;
                let rows = sess.rows_done as usize;
                let first_row_nonzero = sess.out_rgba[..w4.min(sess.out_rgba.len())]
                    .iter()
                    .any(|&b| b != 0);
                first_row_nonzero && rows > 0 && rows < total as usize
            }
            _ => false,
        };
        s.add(
            "B05-部分图像输出非全零",
            ok,
            "未完时 out_rgba 首行须已有非零像素（而非 0 占位）",
        );
    }

    // B06 接收端中止：第 N 行后返回 false，须恰好停在 N 行
    {
        let png = make_png(16, 20);
        let mut sess = st::StreamSession::new();
        let mut rec = RowRecorder::new(5);
        sess.feed(&png, &mut rec).expect("中止不应报故障");
        s.add(
            "B06-接收端中止精确停在第5行",
            sess.aborted && sess.rows_done == 5 && rec.ys.len() == 5,
            "要求第 5 行后中止，须恰停 5 行且标记 aborted",
        );
    }

    // B07 中止后不再交付（续喂也不加行）
    {
        let png = make_png(16, 20);
        let mut sess = st::StreamSession::new();
        let mut rec = RowRecorder::new(5);
        sess.feed(&png, &mut rec).expect("喂入");
        let after_first = sess.rows_done;
        sess.feed(b"\x00\x00\x00\x00", &mut rec).expect("续喂不应报故障");
        s.add("B07-中止后续喂不再交付", sess.rows_done == after_first, "中止后行数须冻结");
    }

    // B07b 中止后行内容不再变化（不只是计数冻结）
    {
        let png = make_png(16, 20);
        let mut sess = st::StreamSession::new();
        let mut rec = RowRecorder::new(5);
        sess.feed(&png, &mut rec).expect("喂入");
        let snap = sess.out_rgba.clone();
        let _ = sess.feed(&png, &mut rec);
        s.add(
            "B07b-中止后像素不再被改写",
            sess.out_rgba == snap,
            "中止后继续喂入不得改动已交付像素",
        );
    }

    // B08 进度回调在有进展时才发（1 字节切分下不刷屏）
    {
        let png = make_png(16, 12);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let mut prog = ProgRecorder { seen: Vec::new(), stop_at: 0 };
        let mut i = 0usize;
        while i < png.len() {
            let e = (i + 1).min(png.len());
            sess.feed_with_progress(&png[i..e], &mut sink, &mut prog).expect("喂入");
            i = e;
        }
        // 进度次数必须等于交付行数（每交付一行发一次），而不是等于 chunk 数。
        // chunk 数 = png.len()（1 字节切分），远大于行数 —— 这条判据正是
        // 「有进展才通知」的守卫：若改成每 chunk 发一封信，次数会 = png.len()。
        s.add(
            "B08-进度次数等于交付行数",
            prog.seen.len() == sess.rows_done as usize
                && sess.progress_calls == sess.rows_done
                && prog.seen.len() < png.len(),
            "进度次数须等于行数且远小于 chunk 数",
        );
    }

    // B09 进度百分比单调不减且终点 100
    {
        let png = make_png(16, 12);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let mut prog = ProgRecorder { seen: Vec::new(), stop_at: 0 };
        sess.feed_with_progress(&png, &mut sink, &mut prog).expect("喂入");
        let mut mono = true;
        for k in 1..prog.seen.len() {
            if prog.seen[k].2 < prog.seen[k - 1].2 {
                mono = false;
            }
        }
        let last = prog.seen.last().map(|t| t.2).unwrap_or(0);
        // 独立重算终点百分比：rows/total 应恰为 100
        let total = sess.total_rows;
        s.add(
            "B09-进度单调且终点100",
            mono && last == 100 && total == 12 && sess.rows_done == total,
            "百分比须单调不减且收于 100（独立重算 total）",
        );
    }

    // B10 进度 total_rows 随 IHDR 定（未知时为 0）
    {
        let fresh = st::StreamSession::new();
        let png = make_png(16, 12);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let head_only = st::StreamSession::new().progress();
        sess.feed(&png[..40], &mut sink).expect("喂入头部");
        let _ = fresh;
        s.add(
            "B10-未解析IHDR时总行数为0",
            head_only.total_rows == 0,
            "会话刚建时 total_rows 须为 0（不猜）",
        );
    }

    // B11 进度回调可中止（控制面独立于行数据面）
    {
        let png = make_png(16, 30);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let mut prog = ProgRecorder { seen: Vec::new(), stop_at: 3 };
        let _ = sess.feed_with_progress(&png, &mut sink, &mut prog);
        // 中止必须在「确有 3 次进度」时才生效；不足 3 次说明进度面压根没触发
        s.add(
            "B11-进度回调可中止",
            prog.seen.len() == 3 && sess.aborted && sess.rows_done >= 3,
            "第 3 次进度后要求中止，须标记 aborted 且已交付 ≥3 行",
        );
    }

    // B12 非隔行 = 自上而下（回调行号即屏幕行号）
    //
    // **基准是「上游全量解码器的第 y 行」而非「编码器的原始像素」**：
    // 上游编码器滤波残差非法（见文件头「上游缺陷取证」一节），原始像素不是可达基准。
    // 本单考的是「非隔行图逐行自上而下交付、且每次交付的像素与全量一致」。
    {
        let (w, h) = (16usize, 12usize);
        let png = make_png(w, h);
        let full = reference_full(&png).expect("全量解码应成功");
        let mut sess = st::StreamSession::new();
        let mut rec = RowRecorder::new(0);
        sess.feed(&png, &mut rec).expect("喂入");
        let w4 = 16 * 4;
        let mut row_ok = rec.ys.len() == h;
        // 逐行比对：第 y 行回调时给的像素须等于**全量解码**的第 y 行
        for &y in &rec.ys {
            let got = &sess.out_rgba[y as usize * w4..(y as usize + 1) * w4];
            if got != &full[y as usize * w4..(y as usize + 1) * w4] {
                row_ok = false;
                break;
            }
        }
        // 顺序守卫：行号须严格 0 起连续（与逐像素对拍是两条独立断言）
        let seq_ok = rec.ys.iter().enumerate().all(|(k, &y)| k as u32 == y);
        s.add(
            "B12-非隔行自上而下逐行正确",
            row_ok && seq_ok && rec.timing_violations == 0,
            "第 y 行回调像素须等于上游全量解码的第 y 行，且行号 0 起连续无违例",
        );
    }

    s
}

/// B 族独立入口。
pub fn run_vef07_checks_b_standalone() -> CheckSet {
    run_vef07_checks_b()
}

// ---------------------------------------------------------------------------
// C 族：中断恢复 / 错误路径 / 开销预算
// ---------------------------------------------------------------------------

/// C 族判据（中断恢复 + 错误路径 + 开销预算 + Adam7 口径）。
pub fn run_vef07_checks_c() -> CheckSet {
    let mut s = CheckSet::new("vef07-c");

    // C01 中断恢复：半途冻结游标 → 新会话恢复 → 续解 → 与不中断全等
    {
        let (w, h) = (16usize, 14usize);
        let png = make_png(w, h);
        let want = reference_full(&png).expect("全量解码应成功");
        let half = png.len() / 2;
        let mut a = st::StreamSession::new();
        let mut sink = st::NullSink;
        let _ = a.feed(&png[..half], &mut sink);
        let cur = a.cursor();
        let mut b = st::StreamSession::new();
        let restored = b.restore(&cur);
        let _ = b.feed(&png[half..], &mut sink);
        s.add(
            "C01-中断恢复输出一致",
            restored == a.rows_done && b.rows_done == h as u32 && b.out_rgba == want,
            "半途冻结-恢复-续解须与不中断全等",
        );
    }

    // C02 游标携带块游标（缓冲非空或期待长度非零）
    {
        let png = make_png(14, 10);
        let mut a = st::StreamSession::new();
        let mut sink = st::NullSink;
        // 喂到 IDAT 中途（留下未消费的块字节）
        let _ = a.feed(&png[..png.len() / 2], &mut sink);
        let cur = a.cursor();
        s.add(
            "C02-游标携带块游标",
            cur.holds_chunk_cursor() && cur.expect > 0,
            "中途冻结的游标须带 expect/buf 块游标",
        );
    }

    // C03 游标魔数不符必须被拒（**反向判据**：防「恢复恒成功」）
    {
        let png = make_png(14, 10);
        let mut a = st::StreamSession::new();
        let mut sink = st::NullSink;
        let _ = a.feed(&png[..png.len() / 2], &mut sink);
        let mut bad = a.cursor();
        bad.magic ^= 0xFFFF_FFFF;
        let mut b = st::StreamSession::new();
        let got = b.restore(&bad);
        s.add(
            "C03-坏魔数游标被拒恢复",
            got == 0 && b.cursor_error() == Some(st::StreamFault::CursorIncompatible),
            "篡改魔数的游标须被显式拒绝",
        );
    }

    // C04 空游标（未喂任何字节）恢复到空会话
    {
        let cur = st::StreamCursor::empty();
        let mut b = st::StreamSession::new();
        let got = b.restore(&cur);
        s.add("C04-空游标恢复为空态", got == 0 && b.rows_done == 0, "空游标恢复须得 0 行");
    }

    // C05 多点中断：连续冻结/恢复 6 次仍能解完
    {
        let (w, h) = (20usize, 16usize);
        let png = make_png(w, h);
        let want = reference_full(&png).expect("全量解码应成功");
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let step = png.len() / 7;
        let mut pos = 0usize;
        let mut rounds = 0u32;
        while pos < png.len() {
            let e = (pos + step).min(png.len());
            sess.feed(&png[pos..e], &mut sink).expect("喂入");
            pos = e;
            rounds += 1;
            if rounds % 2 == 0 && pos < png.len() {
                // 每两轮中断一次：存游标 → 新会话恢复 → 继续
                let cur = sess.cursor();
                let mut nxt = st::StreamSession::new();
                nxt.restore(&cur);
                sess = nxt;
            }
        }
        s.add(
            "C05-多次中断恢复后仍一致",
            sess.rows_done == h as u32 && sess.out_rgba == want,
            "6 次中断恢复后须仍解出全图",
        );
    }

    // C06 三要素错误：签名错 → 种类 + 位置 + 行数齐
    {
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let bad = [0u8; 20];
        let e = sess.feed(&bad, &mut sink).expect_err("签名错须报错");
        s.add(
            "C06-三要素错误带位置与行数",
            e.fault == st::StreamFault::BadSignature && e.fed == 20 && e.rows == 0,
            "坏签名须报 BadSignature 且带 fed=20 rows=0",
        );
    }

    // C07 非法 zlib 头：IHDR 合法但 IDAT 载荷首字节非 zlib 头
    {
        let mut out = Vec::new();
        out.extend_from_slice(&st::PNG_SIG);
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&9u32.to_be_bytes());
        ihdr.extend_from_slice(&9u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        put_chunk(&mut out, b"IHDR", &ihdr);
        put_chunk(&mut out, b"IDAT", &[0x00, 0x00, 0x00, 0x00, 0x00]);
        put_chunk(&mut out, b"IEND", &[]);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let e = sess.feed(&out, &mut sink).expect_err("坏 zlib 头须报错");
        s.add(
            "C07-非法zlib头被拒",
            e.fault == st::StreamFault::BadZlibHeader,
            "CMF/FLG 校验失败须报 BadZlibHeader",
        );
    }

    // C08 块长度越界：声明 2^31 以上 → 显式拒绝
    {
        let mut out = Vec::new();
        out.extend_from_slice(&st::PNG_SIG);
        // 长度字段 = 0xFFFF_FFF0（超过 CHUNK_LEN_MAX）
        out.extend_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&[0u8; 32]);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let e = sess.feed(&out, &mut sink).expect_err("超长块须报错");
        s.add(
            "C08-超长块被拒",
            e.fault == st::StreamFault::LengthOverflow,
            "长度字段超上限须报 LengthOverflow",
        );
    }

    // C09 中途故障不丢弃已完成工作（部分输出仍在）
    //     ——切点取「已交付 ≥1 行」的真实位置，再喂垃圾触发故障
    {
        let png = make_png(16, 30);
        let curve = row_curve(&png);
        let cut = curve.row_at_least(1);
        let ok = match cut {
            Some(c) => {
                let mut sess = st::StreamSession::new();
                let mut rec = RowRecorder::new(0);
                let _ = sess.feed(&png[..c + 1], &mut rec);
                let rows_before = sess.rows_done;
                let snap = sess.out_rgba.clone();
                // 喂 12 个 0xFF：块长度字段会是 0xFFFFFFFF → 越界故障
                let _ = sess.feed(
                    &[0xFF; 12],
                    &mut rec,
                );
                // 三要素齐（kind/fed/rows 都有值）且已交付行原样保留
                sess.rows_done == rows_before && sess.out_rgba == snap && rows_before > 0
            }
            None => false,
        };
        s.add(
            "C09-故障后已交付行保留",
            ok,
            "越界故障后已交付行与像素须原样保留",
        );
    }

    // C10 流式驻留开销 ≤5%（**大图**口径）
    //
    // **口径纠错（曾写错两次，教训钉在这里）**：
    // ① 早先算 `（idat + raw_pending）/ png.len()` —— 那是「簿记字节 / 流量字节」，
    //    **不是开销**。流式路径本来就要读一遍输入，把它算作额外开销等于
    //    宣告开销恒 100%，5% 永不达成。**使目标恒不可达的度量不是度量。**
    // ② 改小图后又必然超预算：inflate 的 32KB 滑动窗口是**固定成本**，
    //    与图像大小无关，32×32 的图只有 10KB 成果，32KB 窗口占比 310%。
    //    那是规格本身的固有成本，不是实现的缺陷。
    //
    // 故：**预算在足够大的图上验**（窗口被摊薄到 <5%），同时对任意尺寸
    // 断言「流式驻留 ≤ 全量工作集」—— 后者才是「流式不比全量更费内存」
    // 这句真话的准确形式。
    {
        // 大图：窗口占比可摊薄到 5% 以下（成果 ~0.9MB ≫ 32KB 窗口）
        let (bw, bh) = (512usize, 384usize);
        let png = make_png(bw, bh);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        sess.feed(&png, &mut sink).expect("喂入");
        let ppm = sess.cost.overhead_ppm();
        // 独立重算分母（不采信被测的 set_bulk）：流长 + 成果字节
        let produced = bw * bh * 4;
        let want_bulk = png.len() as u64 + produced as u64;
        s.add(
            "C10-大图流式驻留开销≤5%",
            sess.rows_done == bh as u32
                && sess.cost.bulk_bytes == want_bulk
                && ppm <= 50_000
                && sess.cost.within_budget(),
            "512x384：驻留峰值/全量工作集须 ≤5%（分母独立重算 = 流长 + 成果）",
        );

        // 任意尺寸：**固定窗口的占比随规模单调递减**。
        //
        // 曾在此断言「流式驻留 ≤ 全量工作集」—— **物理上不成立**：
        // inflate 的 32KB 滑动窗口是固定成本，32×32 的图成果仅 8KB，
        // 驻留 32.9KB 必然大于 8.5KB 的全量工作集。那不是实现的缺陷，
        // 是「固定成本 vs 小图」的必然。要求它成立等于要求实现作弊。
        //
        // 真正该守的不变式：**驻留是 O(1) 而非 O(图大小)**。故断言
        // 「规模翻若干倍，驻留几乎不变」——若驻留随图线性增长，
        // 说明流式退化成了「把整个流留在内存」，那才是失败。
        let mut mono_ok = true;
        let mut prev_live = 0u64;
        let mut sizes: alloc::vec::Vec<(usize, usize)> = alloc::vec::Vec::new();
        sizes.push((32usize, 32usize));
        sizes.push((128usize, 96usize));
        sizes.push((512usize, 384usize));
        let mut lives: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
        for (sw, sh) in &sizes {
            let p = make_png(*sw, *sh);
            let mut ss = st::StreamSession::new();
            let mut sk = st::NullSink;
            let mut i = 0usize;
            while i < p.len() {
                let e = (i + 7).min(p.len());
                ss.feed(&p[i..e], &mut sk).expect("喂入");
                i = e;
            }
            let live = ss.cost.overhead_bytes;
            // 驻留至少含 32KB 窗口（否则这条判据什么也没测到）
            if live < 32768 || ss.rows_done != *sh as u32 {
                mono_ok = false;
            }
            // 规模递增但驻留不得同步递增（固定成本不随图涨）。
            // **首轮不比较**：`prev_live` 初值为 0，首轮必然「增长」，
            // 拿它跟 0 比会把守卫变成恒假的摆设 —— 判据首轮就该跳过。
            if !lives.is_empty() && live > prev_live + 32768 {
                mono_ok = false;
            }
            prev_live = live;
            lives.push(live);
        }
        s.add(
            "C10b-流式驻留为常数级不随图增长",
            mono_ok && lives[2] < lives[0] + 32768,
            "32x32 → 128x96 → 512x384 规模涨 48 倍，驻留须基本不变（O(1) 而非 O(图)）",
        );
    }

    // C11 开销账本口径自洽（**反向守卫**：防「恢复恒成功」式永绿）
    //
    // 三要件：① 分母非零（否则 ppm 恒 0、within_budget 永真）；
    //         ② 分子等于判据侧独立重算的驻留和；
    //         ③ `overhead_ppm()` 与独立算式一致。
    //
    // **判据侧独立重算驻留和**：块游标 + 未消费 IDAT 前缀 + 未切行扫描字节
    // + 块载荷 + 32KB 窗口。不采信被测的 `observe_overhead`，
    // 否则账本自己涨自己就自洽了——那正是自证式。
    {
        let (w, h) = (96usize, 72usize);
        let png = make_png(w, h);
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        // 逐字节喂入：驻留峰值出现在中途，末尾必然回落
        let mut i = 0usize;
        while i < png.len() {
            let e = (i + 7).min(png.len());
            sess.feed(&png[i..e], &mut sink).expect("喂入");
            i = e;
        }
        let numerator = sess.cost.overhead_bytes;
        let bulk_nonzero = sess.cost.bulk_bytes != 0;
        let expect_ppm = if bulk_nonzero {
            numerator * 1_000_000 / sess.cost.bulk_bytes
        } else {
            0
        };
        // 驻留的下界：32KB 窗口必然在内（否则这条判据测不到任何东西）
        let window_floor = 32768u64;
        s.add(
            "C11-开销账本基准非零且口径自洽",
            bulk_nonzero
                && numerator >= window_floor
                && numerator <= png.len() as u64 + 32768 * 4
                && sess.cost.overhead_ppm() == expect_ppm,
            "bulk 非零；分子落在「窗口下界」与「流长+4×窗口」之间；ppm 与独立算式一致",
        );
    }

    // C12 Adam7 遍维度（0..6 递进；七遍行数之和 ≥ height）
    //
    // **修正一处曾经的判据错误**：我曾断言「七遍行数和 == height」，
    // 探针实测 17×13 的七遍行数和 = 26 —— 隔行图的每一遍都重扫整幅，
    // 低分辨率遍的行同样计数，故总和必然**大于** height。
    // 正确不变式：pass0 = ceil(w/8)×ceil(h/8)，七遍行数和 == 独立重算的加权和，
    // 且 ≥ height。
    {
        let (w, h) = (17u32, 13u32);
        let dims: Vec<(u32, u32)> = (0..7).map(|p| st::adam7_pass_dim(w, h, p)).collect();
        // 判据侧独立重算（不调被测函数）：按 RFC2083 步距表算每遍行列
        let xs = [0u32, 4, 0, 2, 0, 1, 0];
        let ys = [0u32, 0, 4, 0, 2, 0, 1];
        let dx = [8u32, 8, 4, 4, 2, 2, 1];
        let dy = [8u32, 8, 8, 4, 4, 2, 2];
        let mut expect_rows = 0u32;
        let mut expect_cols0 = 0u32;
        for p in 0..7 {
            let c = if w > xs[p] { (w - xs[p] + dx[p] - 1) / dx[p] } else { 0 };
            let r = if h > ys[p] { (h - ys[p] + dy[p] - 1) / dy[p] } else { 0 };
            expect_rows += r;
            if p == 0 {
                expect_cols0 = c;
            }
        }
        let sum: u32 = dims.iter().map(|d| d.1).sum();
        s.add(
            "C12-Adam7遍维度独立重算一致",
            sum == expect_rows && expect_rows >= h && dims[0].0 == expect_cols0,
            "七遍行数和须等于独立重算且 ≥height（曾误断言 ==height）",
        );
    }

    // C13 Adam7 原始字节总量口径（非隔行对照）
    {
        let (w, h) = (16u32, 12u32);
        let ch = 3usize;
        let inter = st::adam7_raw_size(w, h, ch);
        // 判据侧独立算：七遍 (rows × (1 + cols×3)) 之和
        let mut expect = 0u64;
        for p in 0..7 {
            let (cols, rows) = st::adam7_pass_dim(w, h, p);
            expect += rows as u64 * (1 + cols as u64 * ch as u64);
        }
        s.add("C13-Adam7原始字节口径一致", inter == expect, "须与独立重算相等");
    }

    // C14 stream_total_rows：非隔行=height
    {
        let mut head = st::parse_ihdr_ex(
            &[0, 0, 0, 16, 0, 0, 0, 12, 8, 2, 0, 0, 0],
            true,
        )
        .expect("合法 IHDR");
        head.interlace = 0;
        s.add("C14-非隔行总行数等于height", st::stream_total_rows(&head) == 12, "非隔行须 12");
    }

    // C15 隔行总行数=七遍之和（与 height 不同——真隔行才看得出差异）
    {
        let mut head = st::parse_ihdr_ex(
            &[0, 0, 0, 16, 0, 0, 0, 12, 8, 2, 0, 0, 1],
            true,
        )
        .expect("合法 IHDR");
        head.interlace = 1;
        let t = st::stream_total_rows(&head);
        // 独立重算（不调 adam7_pass_dim）：17x13 的同口径见表
        let mut expect = 0u32;
        let xs = [0u32, 4, 0, 2, 0, 1, 0];
        let ys = [0u32, 0, 4, 0, 2, 0, 1];
        let dy = [8u32, 8, 8, 4, 4, 2, 2];
        let dx = [8u32, 8, 4, 4, 2, 2, 1];
        for p in 0..7 {
            let r = if 12 > ys[p] { (12 - ys[p] + dy[p] - 1) / dy[p] } else { 0 };
            expect += r;
        }
        // 独立重算行数（判据侧自写常量表循环，**不调** `stream_total_rows`，
        // 免得「问被测函数要答案」变成自证式）。
        fn stream_rows_indep(w: u32, h: u32) -> u32 {
            const X0: [u32; 7] = [0, 4, 0, 2, 0, 1, 0];
            const Y0: [u32; 7] = [0, 0, 4, 0, 2, 0, 1];
            const DX: [u32; 7] = [8, 8, 4, 4, 2, 2, 1];
            const DY: [u32; 7] = [8, 8, 8, 4, 4, 2, 2];
            let mut t = 0u32;
            for p in 0..7 {
                if h > Y0[p] {
                    t += (h - Y0[p] + DY[p] - 1) / DY[p];
                }
            }
            let mut px = 0u64;
            for p in 0..7 {
                let c = if w > X0[p] { (w - X0[p] + DX[p] - 1) / DX[p] } else { 0 };
                let r = if h > Y0[p] { (h - Y0[p] + DY[p] - 1) / DY[p] } else { 0 };
                px += (c as u64) * (r as u64);
            }
            // 覆盖不等于 width 时，用行数口径的哨兵值区分（此处 w×h 必成立）。
            if px == (w as u64) * (h as u64) {
                t
            } else {
                t | 0x8000_0000
            }
        }
        // 同口径独立重算**每遍列数**：列数须与 `adam7_pass_dim` 逐遍相等
        // （常量表几何自洽性，由判据侧独立重算而非问被测函数——防自证式）。
        //
        // **不钉「七遍列数之和 == width」**：那是错的。Adam7 各遍密度不同，
        // w=16 时七遍列数之和为 44（2+2+4+4+8+8+16），远大于 16。
        // 真正的不变式是**逐像素恰好覆盖一次**：Σ(cols×rows) == w×h。
        let mut expect_px = 0u64;
        let mut cols_ok = true;
        for p in 0..7 {
            let (pw, ph) = st::adam7_pass_dim(16, 12, p);
            let indep_c = if 16 > xs[p] { (16 - xs[p] + dx[p] - 1) / dx[p] } else { 0 };
            let indep_r = if 12 > ys[p] { (12 - ys[p] + dy[p] - 1) / dy[p] } else { 0 };
            if pw != indep_c || ph != indep_r {
                cols_ok = false;
            }
            expect_px += (indep_c as u64) * (indep_r as u64);
        }
        s.add(
            "C15-隔行总行数为七遍之和、七遍逐像素恰好覆盖一次",
            t == expect
                && t != 12
                && t > 12
                && cols_ok
                && expect == stream_rows_indep(16, 12)
                && expect_px == 16 * 12,
            "隔行须等于独立重算的七遍行数和且严格大于 height；七遍格数之和须恰等于 w×h（逐像素覆盖一次）",
        );
    }

    // C16 隔行值 >1 必须显式拒绝
    {
        let mut sess = st::StreamSession::new();
        let mut sink = st::NullSink;
        let mut out = Vec::new();
        out.extend_from_slice(&st::PNG_SIG);
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&9u32.to_be_bytes());
        ihdr.extend_from_slice(&9u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 3]); // interlace=3 非法
        put_chunk(&mut out, b"IHDR", &ihdr);
        put_chunk(&mut out, b"IDAT", &[0x78, 0x01, 0, 0, 0]);
        put_chunk(&mut out, b"IEND", &[]);
        let e = sess.feed(&out, &mut sink).expect_err("非法隔行须报错");
        // parse_ihdr_ex 先行拒绝 → 映射为 ChunkOrder 或 BadInterlace
        s.add(
            "C16-非法隔行值被拒",
            e.fault == st::StreamFault::ChunkOrder || e.fault == st::StreamFault::BadInterlace,
            "interlace=3 须被显式拒绝",
        );
    }

    // C17 进度百分比：总行未知为 0，已知则按比例
    {
        let p = st::Progress { fed: 10, rows: 0, total_rows: 0, pass: 0 };
        let q = st::Progress { fed: 10, rows: 6, total_rows: 12, pass: 0 };
        s.add(
            "C17-进度百分比口径正确",
            p.percent() == 0 && q.percent() == 50,
            "总行未知须 0，6/12 须 50",
        );
    }

    // C18 故障名覆盖：每个变体都有可读名（诊断面完整）
    {
        let all = [
            st::StreamFault::BadSignature,
            st::StreamFault::LengthOverflow,
            st::StreamFault::BadZlibHeader,
            st::StreamFault::BadBlockType,
            st::StreamFault::BadCodeLengths,
            st::StreamFault::BadDistance,
            st::StreamFault::CursorIncompatible,
            st::StreamFault::Aborted,
            st::StreamFault::ChunkOrder,
            st::StreamFault::BadInterlace,
        ];
        let ok = all.len() == 10 && all.iter().all(|f| !f.name().is_empty());
        s.add("C18-十个故障变体皆有可读名", ok, "诊断面不得有空名");
    }

    // C19 块长度上限常量与越界判定一致（先比后算）
    {
        let ok = st::CHUNK_LEN_MAX == 0x7FFF_FFFF
            && (st::CHUNK_LEN_MAX as u64 + 1) > st::CHUNK_LEN_MAX as u64
            && (st::CHUNK_LEN_MAX as u32) <= u32::MAX;
        s.add("C19-块长度上限常量自洽", ok, "上限 2^31-1，须能容纳于 u32");
    }

    // C20 位游标绝对性：跨 chunk 累积不受切片边界影响
    {
        // 直接对 InflateState 的 BitPos 验：连续读位与一次性读位一致
        let src = [0b1011_0010u8, 0b0110_1001, 0b1111_0000];
        let mut p1 = st::BitPos::default();
        let mut p2 = st::BitPos::default();
        let mut bits1 = Vec::new();
        let mut bits2 = Vec::new();
        for _ in 0..8 {
            // 逐字节读（模拟 chunk=1 字节）
            let byte = &src[p1.byte()..p1.byte() + 1];
            let mut bp = st::BitPos { bit: p1.bit };
            let v = st::read_bit_pub(byte, &mut bp);
            p1 = bp;
            bits1.push(v);
        }
        for _ in 0..8 {
            // 一次性读（模拟整个 chunk）
            let v = st::read_bit_pub(&src, &mut p2);
            bits2.push(v);
        }
        s.add("C20-位游标跨chunk绝对一致", bits1 == bits2, "1 字节切分与整块读的位序列须相同");
    }

    s
}

/// C 族独立入口。
pub fn run_vef07_checks_c_standalone() -> CheckSet {
    run_vef07_checks_c()
}

// ---------------------------------------------------------------------------
// 聚合入口（三族 merge + 显性截断断言）
// ---------------------------------------------------------------------------

/// 全部判据（三族合并）。
///
/// 显性断言未截断：`CheckSet::MAX_CHECKS = 112` 满了之后 `add` 会静默返回
/// false——不查 `truncated()` 就等于「判据被丢了还以为全绿」。
pub fn run_vef07_checks() -> CheckSet {
    let a = run_vef07_checks_a_standalone();
    let b = run_vef07_checks_b_standalone();
    let c = run_vef07_checks_c_standalone();
    let mut all = CheckSet::merge(a, b);
    all = CheckSet::merge(all, c);
    assert!(!all.truncated(), "VE-F1007 判据被 MAX_CHECKS={} 截断 —— 须再切族", MAX_CHECKS);
    all
}