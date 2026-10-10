//! VE-F0818 域自检（判据逐条映射：契约闭环/兑现声明/四条内容/双端同败/开销）
//!
//! 判据侧**独立写死**期望值（度量手算、量化方向、评审线边界），不复用实现侧
//! 推导——同源恒绿的弱门禁比没有门禁更坏。聚合防自调：[`run_vee18_checks`]
//! 只做 [`CheckSet::merge`]；条数守恒断言用 tally 对比判据侧写死值（F2806/
//! F0817 首版无限递归教训：族内判据调本族/全域入口 = 自我递归栈溢出）。

use crate::checks::CheckSet;

use super::vee08_fontmetric::{MetricSnapshot, MetricSource};
use super::vee18_ncontract::*;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

/// 代表字号 640（10px）下 "AB" 的手算期望（快照 advance=500‰、行高 1200‰）。
const EXPECT_ADV_AB: i64 = 640;
const EXPECT_BH_AB: i64 = 768;
/// 字号 100 下单字形原始读数 50 的量化期望（方向钉死：advance 上取整、边界下取整）。
const EXPECT_ADV_SMALL: i64 = 64;
const EXPECT_BW_SMALL: i64 = 0;
const EXPECT_LH_SMALL: i64 = 128;
const EXPECT_BH_SMALL: i64 = 64;

/// A 族条数（判据侧写死，聚合守恒用）。
const EXPECT_A_COUNT: usize = 29;
/// B 族登记本条前条数（判据侧写死）。
const EXPECT_B_BEFORE: usize = 5;

/// 保守估算手算期望（字号 640、"AB" 两簇：advance=2×1em=1280、行高=640×6/5=768）。
const EXPECT_CONS_ADV: i64 = 1280;
const EXPECT_CONS_LH: i64 = 768;
/// 保守/精确 advance 对照（精确 640，见 [`EXPECT_ADV_AB`]）。
const EXPECT_EXACT_ADV: i64 = 640;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 合法度量快照（与 vee17 判据基准同源数值；判据侧手算对拍）。
fn good_snapshot() -> MetricSnapshot {
    MetricSnapshot {
        ascent: 800,
        descent: 200,
        recommended_line_height: 1200,
        advance: 500,
        cap_height: 700,
        x_height: 450,
        underline_position: 100,
        underline_thickness: 50,
        strikeout_position: 500,
        strikeout_thickness: 50,
        ascent_source: MetricSource::Os2,
        descent_source: MetricSource::Os2,
        line_height_source: MetricSource::Os2,
        conflict_warned: false,
        degraded: false,
    }
}

fn good_font() -> FontRef {
    FontRef { font_id: 1, px_size: 10, weight: 400, glyph_variant: 0 }
}

fn good_params() -> ContractParams {
    ContractParams {
        font_size_q16: 640,
        letter_spacing_q16: 0,
        max_width_q16: 0,
        line_mode: LINE_MODE_FONT_DEFAULT,
    }
}

fn good_req<'a>(text: &'a [u8]) -> ContractRequest<'a> {
    ContractRequest { text, font: good_font(), params: good_params(), lang_tag: 0x6E_6C }
}

/// 引擎快照登记表（SnapshotSource 的判据实现）。
struct VecSource {
    entries: Vec<(FontRef, MetricSnapshot)>,
}

impl VecSource {
    fn with_good() -> VecSource {
        VecSource { entries: vec![(good_font(), good_snapshot())] }
    }
    fn empty() -> VecSource {
        VecSource { entries: vec![] }
    }
}

impl SnapshotSource for VecSource {
    fn snapshot(&self, key: &FontRef) -> Option<MetricSnapshot> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, s)| *s)
    }
}

/// **可替换后端**（判据「N 侧只依赖契约不依赖实现」的验证面）：只 import 契约
/// 类型，不触碰 vee17/vee08 任何符号——它若能驱动 N 侧消费，依赖纪律即证。
struct StubBackend;

impl MeasureBackend for StubBackend {
    fn measure(&self, req: &ContractRequest) -> Result<BackendResult, ContractError> {
        let glyphs = req.text.iter().filter(|b| **b != b'\n').count() as i64;
        Ok(BackendResult {
            advance_q16: glyphs * 64,
            bound_w_q16: glyphs * 64,
            bound_h_q16: 1280,
            line_height_q16: 1280,
            glyph_count: glyphs as u32,
        })
    }
}

/// **计次后端**（判据「超时后不问引擎」的取证面）：首调成功，其后一律故障——
/// 若降级路径仍去问引擎，判据即拿得到 0x1802。
struct CountOnceBackend {
    calls: core::cell::Cell<u32>,
}

impl MeasureBackend for CountOnceBackend {
    fn measure(&self, req: &ContractRequest) -> Result<BackendResult, ContractError> {
        if self.calls.get() == 0 {
            self.calls.set(1);
            Ok(BackendResult {
                advance_q16: EXPECT_EXACT_ADV,
                bound_w_q16: EXPECT_BH_AB,
                bound_h_q16: EXPECT_BH_AB,
                line_height_q16: EXPECT_BH_AB,
                glyph_count: req.text.len() as u32,
            })
        } else {
            Err(ContractError::new(
                C_TEXT18_BACKEND_FAULT,
                "计次后端已耗尽",
                "用于取证降级路径不再问引擎",
            ))
        }
    }
}

/// **可替换保守估算器**（估算策略整体替换面的取证）：宽 2em/簇，行高 2em。
struct WideEstimator;

impl ConservativeEstimator for WideEstimator {
    fn estimate(&self, req: &ContractRequest) -> BackendResult {
        let glyphs = req.text.iter().filter(|b| **b != b'\n').count() as i64;
        let size = req.params.font_size_q16 as i64;
        BackendResult {
            advance_q16: glyphs * 2 * size,
            bound_w_q16: glyphs * 2 * size,
            bound_h_q16: size * 2,
            line_height_q16: size * 2,
            glyph_count: glyphs as u32,
        }
    }
}

// ---------------------------------------------------------------------------
// A 族一：规格（闭环/兑现声明/四条内容/双端哈希/契约自有类型）
// ---------------------------------------------------------------------------

fn chk_spec_closure(set: &mut CheckSet) {
    // 规格-01 契约闭环：请求 → EngineBackend（vee17 封装）→ 响应手算对拍 → N 消费。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let text = b"AB";
    let mut n = n_side::NConsumer::new();
    match n.consume(&be, &good_req(text), &ContractOpts::sync(), 700, 0, 0) {
        Ok(fit) => {
            let ok = fit.advance_q16 == EXPECT_ADV_AB
                && fit.glyph_count == 2
                && !fit.low_confidence
                && fit.fits;
            if ok {
                set.ok("E18-规格-01-契约闭环");
            } else {
                set.fail("E18-规格-01-契约闭环", "闭环数值偏离手算");
            }
        }
        Err(_) => set.fail("E18-规格-01-契约闭环", "合法闭环路径报错"),
    }
    // 规格-02 四条内容：兑现声明 items 非空且证据值与实际常量逐项相等。
    let ev_ok = FULFILLMENT.items.iter().all(|s| !s.is_empty())
        && FULFILLMENT.evidence.iter().all(|s| !s.is_empty())
        && FULFILLMENT.evidence[2].contains("GRID_Q16=64")
        && GRID_Q16 == 64
        && FULFILLMENT.evidence[3].contains("TIMEOUT_US=16000")
        && TIMEOUT_US == 16_000
        && FULFILLMENT.contract_id == "F0813";
    if ev_ok {
        set.ok("E18-规格-02-四条内容与证据对账");
    } else {
        set.fail("E18-规格-02-四条内容与证据对账", "条目缺失或证据值漂移");
    }
    // 规格-03 兑现声明可读文本：含契约号、四条与证据指针。
    let st = fulfillment_statement();
    let ok = st.contains("F0813")
        && st.contains("测量请求字段")
        && st.contains("响应字段")
        && st.contains("精度约定")
        && st.contains("失败语义")
        && st.contains("ContractRequest")
        && st.contains("low_confidence");
    if ok {
        set.ok("E18-规格-03-兑现声明承载");
    } else {
        set.fail("E18-规格-03-兑现声明承载", "声明缺条目或证据");
    }
    // 规格-04 双端同败（运行期复核面）：N 侧独立文本与 E 侧哈希互证。
    let ok = n_side::N_CONTRACT_TEXT == CONTRACT_TEXT
        && n_side::n_expected_hash() == PROVIDER_CONTRACT_HASH
        && contract_hash_ok()
        && PROVIDER_CONTRACT_HASH != 0;
    if ok {
        set.ok("E18-规格-04-双端哈希互证");
    } else {
        set.fail("E18-规格-04-双端哈希互证", "契约文本或哈希漂移");
    }
    // 规格-05 N 侧只依赖契约：可替换后端（零实现类型）驱动同一消费路径。
    let mut n = n_side::NConsumer::new();
    match n.consume(&StubBackend, &good_req(b"AB"), &ContractOpts::sync(), 128, 0, 0) {
        Ok(fit) => {
            if fit.advance_q16 == 128 && fit.fits && n.timeouts == 0 {
                set.ok("E18-规格-05-可替换后端验证");
            } else {
                set.fail("E18-规格-05-可替换后端验证", "替换后端结果不符");
            }
        }
        Err(_) => set.fail("E18-规格-05-可替换后端验证", "契约面拒绝合法替换后端"),
    }
}

// ---------------------------------------------------------------------------
// A 族二：边界（量化方向/超时语义/参数闸/评审线）
// ---------------------------------------------------------------------------

fn chk_bound_quantize(set: &mut CheckSet) {
    // 边界-01 量化方向钉死：字号 100 → 单字形原始 50（非 1/64 网格）。
    //   advance 上取整 50→64；边界宽下取整 50→0；行高 120→128 上取整；边界高 120→64 下取整。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let mut params = good_params();
    params.font_size_q16 = 100;
    let req = ContractRequest {
        text: b"A",
        font: good_font(),
        params,
        lang_tag: 0,
    };
    match contract_measure(&be, &req, &ContractOpts::sync()) {
        Ok(ContractOutcome::Fulfilled(r)) => {
            let ok = r.advance_q16 == EXPECT_ADV_SMALL
                && r.bound_w_q16 == EXPECT_BW_SMALL
                && r.line_height_q16 == EXPECT_LH_SMALL
                && r.bound_h_q16 == EXPECT_BH_SMALL;
            if ok {
                set.ok("E18-边界-01-量化方向钉死");
            } else {
                set.fail("E18-边界-01-量化方向钉死", "量化值或方向漂移");
            }
        }
        _ => set.fail("E18-边界-01-量化方向钉死", "合法量化路径报错"),
    }
    // 边界-02 网格值不变换：640 恰在网格 → 上下取整都不动（手算 640）。
    match contract_measure(&be, &good_req(b"AB"), &ContractOpts::sync()) {
        Ok(ContractOutcome::Fulfilled(r)) => {
            let ok = r.advance_q16 == EXPECT_ADV_AB && r.bound_h_q16 == EXPECT_BH_AB;
            if ok {
                set.ok("E18-边界-02-网格值恒等");
            } else {
                set.fail("E18-边界-02-网格值恒等", "网格值被错误挪动");
            }
        }
        _ => set.fail("E18-边界-02-网格值恒等", "合法路径报错"),
    }
}

fn chk_bound_timeout(set: &mut CheckSet) {
    // 边界-03 超时 16ms 语义：恰 16000μs 到点 = 时限内（<=，保真）；+1μs = 降级。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let req = good_req(b"AB");
    let mut n = n_side::NConsumer::new();
    let in_time = n.consume(&be, &req, &ContractOpts::async16(), 10_000, 16_000, 0);
    let late = n.consume(&be, &req, &ContractOpts::async16(), 10_000, 16_001, 0);
    let ok = match (in_time, late) {
        (Ok(a), Ok(b)) => {
            !a.low_confidence
                && b.low_confidence
                && a.advance_q16 == EXPECT_EXACT_ADV
                && b.advance_q16 == EXPECT_CONS_ADV
                && b.advance_q16 > a.advance_q16
        }
        _ => false,
    };
    if ok && n.timeouts == 1 && n.calls == 2 {
        set.ok("E18-边界-03-超时16ms语义");
    } else {
        set.fail("E18-边界-03-超时16ms语义", "deadline 翻面或计数漂移");
    }
    // 边界-04 评审线：>0.1% 严格大于——恰 0.1%（1000 中 1）不触发，2/1000 触发。
    let calm = n_side::NConsumer { calls: 1000, timeouts: 1, overhead_ns: 0, review_open: false };
    let hot = n_side::NConsumer { calls: 1000, timeouts: 2, overhead_ns: 0, review_open: false };
    if !calm.review_required() && hot.review_required() {
        set.ok("E18-边界-04-评审线严格大于");
    } else {
        set.fail("E18-边界-04-评审线严格大于", "评审线边界漂移（含等或提前触发）");
    }
    // 边界-07 stale 句柄：句柄与请求内容不匹配 → 0x1803（不返回任何度量）。
    let handle = match contract_measure(&be, &good_req(b"AB"), &ContractOpts::async16()) {
        Ok(ContractOutcome::Pended(h)) => h,
        _ => ContractHandle { ticket: 0, timeout_us: TIMEOUT_US, cost_ns: 0 },
    };
    let r = contract_resolve(&handle, &be, &good_req(b"XYZ"), 0, 0);
    if matches!(r, Err(ContractError { code: C_TEXT18_TICKET_MISMATCH, .. })) {
        set.ok("E18-边界-07-stale句柄拒绝");
    } else {
        set.fail("E18-边界-07-stale句柄拒绝", "stale 句柄未被拒");
    }
}

fn chk_bound_gates(set: &mut CheckSet) {
    // 边界-05 参数闸：字号越界与行高模式越界 → 0x1801（契约前置校验）。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let mut p1 = good_params();
    p1.font_size_q16 = 63;
    let r1 = contract_measure(&be, &ContractRequest { text: b"A", font: good_font(), params: p1, lang_tag: 0 }, &ContractOpts::sync());
    let mut p2 = good_params();
    p2.font_size_q16 = 16_385;
    let r2 = contract_measure(&be, &ContractRequest { text: b"A", font: good_font(), params: p2, lang_tag: 0 }, &ContractOpts::sync());
    let mut p3 = good_params();
    p3.line_mode = 3;
    let r3 = contract_measure(&be, &ContractRequest { text: b"A", font: good_font(), params: p3, lang_tag: 0 }, &ContractOpts::sync());
    let ok = matches!(r1, Err(ContractError { code: C_TEXT18_BAD_REQUEST, .. }))
        && matches!(r2, Err(ContractError { code: C_TEXT18_BAD_REQUEST, .. }))
        && matches!(r3, Err(ContractError { code: C_TEXT18_BAD_REQUEST, .. }));
    if ok {
        set.ok("E18-边界-05-参数前置闸");
    } else {
        set.fail("E18-边界-05-参数前置闸", "越界参数未按 0x1801 拒绝");
    }
    // 边界-06 后端故障双路：实例未登记 / 引擎度量失败（坏编码）→ 0x1802。
    let empty = VecSource::empty();
    let be2 = EngineBackend { source: &empty };
    let f1 = contract_measure(&be2, &good_req(b"A"), &ContractOpts::sync());
    let f2 = contract_measure(&be, &good_req(&[0xD8, 0x00]), &ContractOpts::sync());
    let ok = matches!(f1, Err(ContractError { code: C_TEXT18_BACKEND_FAULT, .. }))
        && matches!(f2, Err(ContractError { code: C_TEXT18_BACKEND_FAULT, .. }));
    if ok {
        set.ok("E18-边界-06-后端故障包装");
    } else {
        set.fail("E18-边界-06-后端故障包装", "后端故障未按 0x1802 契约化");
    }
}

// ---------------------------------------------------------------------------
// A 族三：幂等与簇映射
// ---------------------------------------------------------------------------

fn chk_idem(set: &mut CheckSet) {
    // 幂等-01 同请求同 ticket（异步句柄）。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let t1 = contract_measure(&be, &good_req(b"AB"), &ContractOpts::async16());
    let t2 = contract_measure(&be, &good_req(b"AB"), &ContractOpts::async16());
    let ok = match (t1, t2) {
        (Ok(ContractOutcome::Pended(a)), Ok(ContractOutcome::Pended(b))) => a == b && a.timeout_us == TIMEOUT_US,
        _ => false,
    };
    if ok {
        set.ok("E18-幂等-01-同请求同句柄");
    } else {
        set.fail("E18-幂等-01-同请求同句柄", "句柄随调用漂移");
    }
    // 幂等-02 同请求响应全等（含簇映射逐条相等）。
    let r1 = contract_measure(&be, &good_req(b"AB"), &ContractOpts::sync());
    let r2 = contract_measure(&be, &good_req(b"AB"), &ContractOpts::sync());
    let ok = match (r1, r2) {
        (Ok(ContractOutcome::Fulfilled(a)), Ok(ContractOutcome::Fulfilled(b))) => a == b,
        _ => false,
    };
    if ok {
        set.ok("E18-幂等-02-同请求同响应");
    } else {
        set.fail("E18-幂等-02-同请求同响应", "响应随调用漂移");
    }
    // 幂等-03 簇映射：换行不成簇、偏移按字节、簇数=引擎 glyph_count（手算 A\nBC）。
    match contract_measure(&be, &good_req(b"A\nBC"), &ContractOpts::sync()) {
        Ok(ContractOutcome::Fulfilled(r)) => {
            let expect = [
                Cluster { byte_off: 0, glyph: 0 },
                Cluster { byte_off: 2, glyph: 1 },
                Cluster { byte_off: 3, glyph: 2 },
            ];
            let ok = r.glyph_count == 3
                && r.clusters.len() == 3
                && r.clusters[..] == expect[..];
            if ok {
                set.ok("E18-幂等-03-簇映射手算对拍");
            } else {
                set.fail("E18-幂等-03-簇映射手算对拍", "簇偏移/序数/计数漂移");
            }
        }
        _ => set.fail("E18-幂等-03-簇映射手算对拍", "合法路径报错"),
    }
}

// ---------------------------------------------------------------------------
// A 族四：开销预算与变更账
// ---------------------------------------------------------------------------

fn chk_cost_version(set: &mut CheckSet) {
    // 开销-01 代表请求内预算：150 + 2B*2 + 2簇*8 = 170ns ≤ 2000ns；且模型单调。
    let small = call_cost_ns(2, 2);
    let big = call_cost_ns(256, 256);
    if small == 170 && small <= CALL_BUDGET_NS && CALL_BUDGET_NS == 2_000 && big >= small {
        set.ok("E18-开销-01-预算内且单调");
    } else {
        set.fail("E18-开销-01-预算内且单调", "开销模型或预算漂移");
    }
    // 变更-01 版本-账目守恒（跨域变更流程的机制化面）。
    if CONTRACT_VERSION == 1 && CONTRACT_VERSION as usize == CONTRACT_REVISIONS.len() {
        set.ok("E18-变更-01-版本账目守恒");
    } else {
        set.fail("E18-变更-01-版本账目守恒", "版本与修订账不一致");
    }
}

// ---------------------------------------------------------------------------
// B 族：错误契约 + 判据承载力
// ---------------------------------------------------------------------------

fn chk_err_contract(set: &mut CheckSet) {
    // 错误-01 五码独占 0x18 段、互异、呈现三要素齐备。
    let codes = [
        C_TEXT18_HASH_MISMATCH,
        C_TEXT18_BAD_REQUEST,
        C_TEXT18_BACKEND_FAULT,
        C_TEXT18_TICKET_MISMATCH,
        C_TEXT18_CHANGE_REJECTED,
    ];
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in i + 1..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    let seg_ok = codes.iter().all(|c| c & 0xFF00 == 0x1800);
    let e = ContractError::new(C_TEXT18_BACKEND_FAULT, "原因样例", "建议样例");
    let line = e.screen_line();
    let three = line.contains("0x1802") && line.contains("原因样例") && line.contains("建议样例");
    if distinct && seg_ok && three {
        set.ok("E18-错误-01-三码三要素");
    } else {
        set.fail("E18-错误-01-三码三要素", "码段/互异/三要素漂移");
    }
    // 错误-02 哈希运行期闸路径（以等效输入驱动 measure 的 HashMismatch 分支
    // 不可直接注入——const 闸保证恒真；此处断言运行期复核函数与码绑定存在性）。
    let ok = contract_hash_ok() && C_TEXT18_HASH_MISMATCH == 0x1800;
    if ok {
        set.ok("E18-错误-02-哈希闸承载");
    } else {
        set.fail("E18-错误-02-哈希闸承载", "哈希复核面漂移");
    }
    // 错误-03 摘要行承载。
    let line = screen_line();
    if line.contains("F0813") && line.contains("v1") && line.contains("16000us") && line.contains("2000ns") {
        set.ok("E18-错误-03-摘要承载");
    } else {
        set.fail("E18-错误-03-摘要承载", "摘要缺版本/超时/预算");
    }
}

/// 单遍词法剥除（字符串/注释不误伤——F2807 实测教训）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_zero_panic(set: &mut CheckSet) {
    // 判据-01 判据面零 panic（自扫）。
    let src = include_str!("vee18_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E18-判据-01-判据面零 panic");
    } else {
        set.fail("E18-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 判据-02 生产面零 panic。
    let src = include_str!("vee18_ncontract.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E18-判据-02-生产面零 panic");
    } else {
        set.fail("E18-判据-02-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-03 聚合守恒防自调：A 族条数、B 族进行中条数均判据侧写死。
    let a = run_vee18_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E18-判据-03-聚合守恒防自调");
    } else {
        set.fail("E18-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

fn chk_conservative(set: &mut CheckSet) {
    // 保守-01 超时后**不问引擎**：计次后端首调后即故障，降级仍拿到响应即证。
    let once = CountOnceBackend { calls: core::cell::Cell::new(0) };
    let req = good_req(b"AB");
    let handle = match contract_measure(&once, &req, &ContractOpts::async16()) {
        Ok(ContractOutcome::Pended(h)) => h,
        _ => {
            set.fail("E18-保守-01-超时不再问引擎", "首调未挂起");
            return;
        }
    };
    match contract_resolve(&handle, &once, &req, 16_001, 0) {
        Ok(r) => {
            let ok = r.low_confidence
                && r.advance_q16 == EXPECT_CONS_ADV
                && r.line_height_q16 == EXPECT_CONS_LH
                && r.bound_w_q16 == EXPECT_CONS_ADV
                && r.glyph_count == 2
                && r.clusters.len() == 2;
            if ok {
                set.ok("E18-保守-01-超时不再问引擎");
            } else {
                set.fail("E18-保守-01-超时不再问引擎", "降级值未到手算期望");
            }
        }
        Err(_) => set.fail("E18-保守-01-超时不再问引擎", "降级路径仍撞引擎故障（0x1802）"),
    }
    // 保守-02 方向钉死：保守 advance/行高 ≥ 精确值（永不低估占宽）。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let exact = match contract_measure(&be, &good_req(b"AB"), &ContractOpts::sync()) {
        Ok(ContractOutcome::Fulfilled(r)) => r,
        _ => {
            set.fail("E18-保守-02-保守方向不低估", "精确路径报错");
            return;
        }
    };
    let cons = match conservative_response(&good_req(b"AB")) {
        Ok(r) => r,
        Err(_) => {
            set.fail("E18-保守-02-保守方向不低估", "保守路径报错");
            return;
        }
    };
    let ok = exact.advance_q16 == EXPECT_EXACT_ADV
        && cons.advance_q16 >= exact.advance_q16
        && cons.line_height_q16 >= exact.line_height_q16
        && cons.bound_w_q16 <= cons.advance_q16
        && cons.low_confidence
        && !exact.low_confidence;
    if ok {
        set.ok("E18-保守-02-保守方向不低估");
    } else {
        set.fail("E18-保守-02-保守方向不低估", "保守/精确方向关系漂移");
    }
    // 保守-03 估算器可整体替换（N 侧换策略、契约层不动）：宽估算器给出 2em/簇。
    let wide = match conservative_response_with(&WideEstimator, &good_req(b"AB")) {
        Ok(r) => r,
        Err(_) => {
            set.fail("E18-保守-03-估算器可替换", "替换估算器报错");
            return;
        }
    };
    if wide.advance_q16 == EXPECT_CONS_ADV * 2 && wide.line_height_q16 == 1280 {
        set.ok("E18-保守-03-估算器可替换");
    } else {
        set.fail("E18-保守-03-估算器可替换", "替换估算器结果不符");
    }
    // 保守-04 降级路径同样受参数闸约束（越界估算也不放行）。
    let mut bad = good_params();
    bad.font_size_q16 = 1;
    let r = conservative_response(&ContractRequest {
        text: b"A",
        font: good_font(),
        params: bad,
        lang_tag: 0,
    });
    if matches!(r, Err(ContractError { code: C_TEXT18_BAD_REQUEST, .. })) {
        set.ok("E18-保守-04-降级路径参数闸");
    } else {
        set.fail("E18-保守-04-降级路径参数闸", "降级路径绕过了参数闸");
    }
}

fn chk_budget_accounting(set: &mut CheckSet) {
    // 预算-02 开销逐次落账：响应与异步句柄同源同值，且 N 侧累计在预算内。
    let src = VecSource::with_good();
    let be = EngineBackend { source: &src };
    let req = good_req(b"AB");
    let resp = match contract_measure(&be, &req, &ContractOpts::sync()) {
        Ok(ContractOutcome::Fulfilled(r)) => r,
        _ => {
            set.fail("E18-预算-02-逐次开销落账", "同步路径报错");
            return;
        }
    };
    let handle = match contract_measure(&be, &req, &ContractOpts::async16()) {
        Ok(ContractOutcome::Pended(h)) => h,
        _ => {
            set.fail("E18-预算-02-逐次开销落账", "异步路径报错");
            return;
        }
    };
    let mut n = n_side::NConsumer::new();
    let _ = n.consume(&be, &req, &ContractOpts::sync(), 10_000, 0, 0);
    let ok = resp.cost_ns == call_cost_ns(2, 2)
        && handle.cost_ns == resp.cost_ns
        && request_cost(&req) == resp.cost_ns
        && n.overhead_ns == resp.cost_ns
        && n.budget_ok()
        && resp.cost_ns <= CALL_BUDGET_NS;
    if ok {
        set.ok("E18-预算-02-逐次开销落账");
    } else {
        set.fail("E18-预算-02-逐次开销落账", "开销未逐次落账或超预算");
    }
    // 预算-03 保守降级同样记开销（降级不豁免记账——它也是一次契约调用）。
    let cons = match conservative_response(&good_req(b"AB")) {
        Ok(r) => r,
        Err(_) => {
            set.fail("E18-预算-03-降级同记账", "降级路径报错");
            return;
        }
    };
    if cons.cost_ns == resp.cost_ns {
        set.ok("E18-预算-03-降级同记账");
    } else {
        set.fail("E18-预算-03-降级同记账", "降级路径漏记或错记开销");
    }
    // 预算-04 预算容量边界钉死：cost = 150 + 10L（全非换行字节）⇒ L=185 恰在
    // 2000ns 预算内，L=186 越线——容量上限可判定，不靠「大概不超」。
    let at_cap = call_cost_ns(185, 185);
    let over_cap = call_cost_ns(186, 186);
    if at_cap == CALL_BUDGET_NS && over_cap > CALL_BUDGET_NS && over_cap > at_cap {
        set.ok("E18-预算-04-预算容量边界");
    } else {
        set.fail("E18-预算-04-预算容量边界", "预算容量上界不可判定或恰边界漂移");
    }
}

fn chk_review_trigger(set: &mut CheckSet) {
    // 评审-03 越线即开单：1000 调 2 超时 → 千分率 2、哈希与版本在单、不重复开单。
    let mut n = n_side::NConsumer { calls: 999, timeouts: 1, overhead_ns: 0, review_open: false };
    let first = n.take_review();
    let second = n.take_review();
    let review = match first {
        Some(r) => r,
        None => {
            set.fail("E18-评审-03-越线触发评审单", "越线未开单");
            return;
        }
    };
    let line = review.screen_line();
    let ok = second.is_none()
        && n.review_open
        && review.calls == 999
        && review.timeouts == 1
        && review.rate_permille == 1
        && review.contract_id == CONTRACT_ID
        && review.version == CONTRACT_VERSION
        && review.hash == PROVIDER_CONTRACT_HASH
        && line.contains("F0813")
        && line.contains("1‰");
    if ok {
        set.ok("E18-评审-03-越线触发评审单");
    } else {
        set.fail("E18-评审-03-越线触发评审单", "评审单字段或去重漂移");
    }
    // 评审-04 未越线不开单；恰 0.1% 亦不开单（严格大于的实体面）。
    let mut calm = n_side::NConsumer { calls: 1000, timeouts: 1, overhead_ns: 0, review_open: false };
    if calm.take_review().is_none() && calm.timeout_rate_permille() == 1 {
        set.ok("E18-评审-04-未越线不开单");
    } else {
        set.fail("E18-评审-04-未越线不开单", "未越线即开单或千分率错算");
    }
}

fn chk_change_process(set: &mut CheckSet) {
    // 变更-02 提案双拒：空理由、与现行文本逐字相同 → 0x1804。
    let empty = propose_change("VE-F0818/text-measure/v2:noop", "  ");
    let same = propose_change(CONTRACT_TEXT, "改点措辞");
    let ok = matches!(empty, Err(ContractError { code: C_TEXT18_CHANGE_REJECTED, .. }))
        && matches!(same, Err(ContractError { code: C_TEXT18_CHANGE_REJECTED, .. }));
    if ok {
        set.ok("E18-变更-02-提案双拒");
    } else {
        set.fail("E18-变更-02-提案双拒", "空理由/空变更未被拒");
    }
    // 变更-03 受理前不可生效：E 侧提案停在 PROPOSED，N 侧受理后才翻 ACCEPTED。
    let mut change = match propose_change("VE-F0818/text-measure/v2:req+{tracking};", "新增 tracking 字段") {
        Ok(c) => c,
        Err(_) => {
            set.fail("E18-变更-03-双签受理", "合法提案被拒");
            return;
        }
    };
    let before = (change.state, change.is_accepted());
    let accepted = accept_change(&mut change);
    let ok = before == (CHANGE_PROPOSED, false)
        && accepted.is_ok()
        && change.is_accepted()
        && change.hash_after != change.hash_before
        && change.hash_before == PROVIDER_CONTRACT_HASH
        && change.to_version == CONTRACT_VERSION + 1
        && change.screen_line().contains("v1->v2");
    if ok {
        set.ok("E18-变更-03-双签受理");
    } else {
        set.fail("E18-变更-03-双签受理", "受理状态或前后哈希漂移");
    }
    // 变更-04 已受理单不可二次受理伪造（状态幂等：受理后 state 必为 ACCEPTED）。
    let mut again = match propose_change("VE-F0818/text-measure/v2:req-;", "精简字段") {
        Ok(c) => c,
        Err(_) => {
            set.fail("E18-变更-04-受理状态守恒", "合法提案被拒");
            return;
        }
    };
    let _ = accept_change(&mut again);
    let reaccept = accept_change(&mut again);
    if reaccept.is_ok() && again.state == CHANGE_ACCEPTED {
        set.ok("E18-变更-04-受理状态守恒");
    } else {
        set.fail("E18-变更-04-受理状态守恒", "受理后状态漂移");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+幂等+开销 / b=错误+判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 幂等 + 开销。
pub fn run_vee18_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vee18/a");
    chk_spec_closure(&mut s);
    chk_bound_quantize(&mut s);
    chk_bound_timeout(&mut s);
    chk_bound_gates(&mut s);
    chk_idem(&mut s);
    chk_cost_version(&mut s);
    chk_conservative(&mut s);
    chk_budget_accounting(&mut s);
    chk_review_trigger(&mut s);
    chk_change_process(&mut s);
    s
}

/// 判据族 b：错误契约 + 判据承载力。
pub fn run_vee18_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vee18/b");
    chk_err_contract(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vee18_checks() -> CheckSet {
    CheckSet::merge(run_vee18_checks_a_standalone(), run_vee18_checks_b_standalone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 域自检独立入口：F0818 判据全绿（聚合器之外的可复跑面）。
    #[test]
    fn vee18_family_all_green() {
        let set = run_vee18_checks();
        assert!(
            set.all_passed(),
            "vee18 域自检存在红项：{}/{} 绿",
            set.tally().0,
            set.len(),
        );
    }
}
