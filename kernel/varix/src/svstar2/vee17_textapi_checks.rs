//! VE-F0817 域自检（VE-E 域 · 文字渲染 API 判据层）
//!
//! 判据侧**独立写死**期望值（签名文本、码段、度量手算值），不复用实现侧
//! 常量——同源恒绿的弱门禁比没有门禁更坏。聚合防自调：[`run_vee17_checks`]
//! 只做 [`CheckSet::merge`]，签名冻结断言用 tally 合计对比而非自调全域
//! 入口（F2806 无限递归教训）。

use crate::checks::CheckSet;

use super::vee08_fontmetric::{LineHeightMode, MetricKey, MetricSnapshot, MetricSource};
use super::vee17_textapi as api;
use super::vee17_textapi::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望（不 import 实现侧对应常量）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的三函数签名文本（与实现侧 [`api::SIGNATURE_TEXTS`] 对拍）。
const EXPECT_SIGNATURES: [&str; 3] = [
    "measure_text(text: &[u8], instance: &MetricSnapshot, params: &TypesetParams, opts: &MeasureOptions) -> Result<MeasureOutcome, TextApiError>",
    "render_text(req: &RenderRequest) -> Result<RenderReceipt, TextApiError>",
    "cache_control(gate: &mut AtlasGate, op: CacheOp) -> Result<CacheWatermark, TextApiError>",
];

/// 判据侧独立写死的 0x17 码段与七细分码。
const EXPECT_CODES: [u16; 7] = [
    0x1700, 0x1701, 0x1702, 0x1703, 0x1704, 0x1705, 0x1706,
];

/// 判据侧签名指纹：标准 FNV-1a（域分隔串 + 三签名顺连），独立第二实现。
fn expect_fingerprint() -> u64 {
    fn std_fnv(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in data {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
    let mut h = std_fnv(b"VE-F0817/signatures/v1");
    for s in EXPECT_SIGNATURES.iter() {
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 合法度量快照（判据基准实例；数值判据侧手算对拍）。
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

fn good_params() -> TypesetParams {
    TypesetParams {
        font_size_q16: 640, // 10px
        letter_spacing_q16: 0,
        max_width_q16: 0,
        line_mode: LineHeightMode::FontDefault,
    }
}

/// 合法渲染请求（度量手算的伴生语料）。
fn good_request() -> RenderRequest {
    RenderRequest {
        text_len: 2,
        text_hash: 0xABCD_1234,
        instance: MetricKey::new(1, 10, 400, 0),
        params: good_params(),
        dst_x_q16: 0,
        dst_y_q16: 0,
        dst_w_q16: 640,
        dst_h_q16: 768,
        effect_flags: 0,
        opts: RenderOptions { space: 0, z_q16: 0, legacy_glow: None },
    }
}

// ---------------------------------------------------------------------------
// 一、规格（锚点判据：三函数 / 十年签名 / 四段错误码 / 呈现三要素）
// ---------------------------------------------------------------------------

fn chk_spec_three_fns(set: &mut CheckSet) {
    // 规格-01：三函数存在且合法路径各通一次。
    let snap = good_snapshot();
    let params = good_params();
    let opts = MeasureOptions::sync();
    let m_ok = match measure_text(b"AB", &snap, &params, &opts) {
        Ok(MeasureOutcome::Sync(res)) => {
            // 规格-02：度量手算对拍（advance=500‰×10px×2 字形 = 2×320 = 640）。
            let adv_ok = res.advance_q16 == 640
                && res.bound_w_q16 == 640
                && res.bound_h_q16 == 768
                && res.line_height_q16 == 768
                && res.glyph_count == 2;
            if adv_ok {
                set.ok("E17-规格-02-度量手算对拍");
            } else {
                set.fail("E17-规格-02-度量手算对拍", "度量值偏离手算");
            }
            true
        }
        _ => {
            set.fail("E17-规格-02-度量手算对拍", "合法测量未同步返回");
            false
        }
    };
    let r_ok = render_text(&good_request()).is_ok();
    let mut gate = AtlasGate::new(100);
    let c_ok = cache_control(&mut gate, CacheOp::Watermark).is_ok();
    if m_ok && r_ok && c_ok {
        set.ok("E17-规格-01-三函数可达");
    } else {
        set.fail("E17-规格-01-三函数可达", "存在函数合法路径不通");
    }
}

fn chk_spec_presentation(set: &mut CheckSet) {
    // 规格-03：呈现三要素（码 | 原因 | 建议，全非空且带段名）。
    let err = TextApiError::Cache {
        code: E_TEXTAPI_CACHE_FULL,
        reason: String::from("预热超容"),
        advice: String::from("先逐出再预热"),
    };
    let line = err.screen_line();
    let three = line.contains("[缓存]")
        && line.contains("0x1703")
        && line.contains("预热超容")
        && line.contains("先逐出再预热");
    if three {
        set.ok("E17-规格-03-呈现三要素");
    } else {
        set.fail("E17-规格-03-呈现三要素", "读屏缺段名/码/原因/建议之一");
    }
    // 规格-04：四段错误码齐全（四段各构造一例，段名互异）。
    let e_enc = TextApiError::Encoding {
        code: E_TEXTAPI_BAD_UTF8,
        reason: String::from("r"),
        advice: String::from("a"),
    };
    let segs = [
        e_enc.segment(),
        TextApiError::Font { code: 0, reason: String::new(), advice: String::new() }.segment(),
        TextApiError::Cache { code: 0, reason: String::new(), advice: String::new() }.segment(),
        TextApiError::Pipeline { code: 0, reason: String::new(), advice: String::new() }.segment(),
    ];
    let distinct = segs[0] != segs[1]
        && segs[1] != segs[2]
        && segs[2] != segs[3]
        && segs[0] != segs[3]
        && segs[1] != segs[3]
        && segs[0] != segs[2];
    if distinct {
        set.ok("E17-规格-04-四段错误码");
    } else {
        set.fail("E17-规格-04-四段错误码", "段名缺失或撞名");
    }
}

fn chk_spec_signature_frozen(set: &mut CheckSet) {
    // 规格-05：签名冻结断言——判据侧独立文本重算指纹对拍。
    let impl_fp = signature_fingerprint();
    let expect_fp = expect_fingerprint();
    if impl_fp == expect_fp {
        set.ok("E17-规格-05-签名冻结指纹对拍");
    } else {
        set.fail("E17-规格-05-签名冻结指纹对拍", "签名文本漂移或指纹算法漂移");
    }
    // 规格-06：签名文本非空且互异（十年承诺的文本面完整性）。
    let mut nonempty = true;
    let mut distinct = true;
    let mut i = 0usize;
    while i < EXPECT_SIGNATURES.len() {
        if EXPECT_SIGNATURES[i].is_empty() {
            nonempty = false;
        }
        let mut j = i + 1;
        while j < EXPECT_SIGNATURES.len() {
            if EXPECT_SIGNATURES[i] == EXPECT_SIGNATURES[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    if nonempty && distinct {
        set.ok("E17-规格-06-签名文本非空互异");
    } else {
        set.fail("E17-规格-06-签名文本非空互异", "文本缺失或撞文本");
    }
}

fn chk_spec_options_extension(set: &mut CheckSet) {
    // 规格-07：options 追加面——同步默认与自定义构造并存（扩展只加字段）。
    let d = MeasureOptions::sync();
    let custom = MeasureOptions { async_timeout_us: 16_000, lang_tag: 0x6E_6C };
    if d.async_timeout_us == 0 && custom.async_timeout_us == 16_000 {
        set.ok("E17-规格-07-options 追加面");
    } else {
        set.fail("E17-规格-07-options 追加面", "构造面漂移");
    }
    // 规格-08：MeasureText 只读不缓存——同参数两次调用逐字段相等，
    // 且传入快照未变（快照是 Copy，比对两次调用后仍与初始值相等）。
    let snap = good_snapshot();
    let before = snap;
    let params = good_params();
    let r1 = measure_text(b"ABC", &snap, &params, &MeasureOptions::sync());
    let r2 = measure_text(b"ABC", &snap, &params, &MeasureOptions::sync());
    let pure = r1 == r2 && snap == before;
    if pure {
        set.ok("E17-规格-08-测量只读不缓存");
    } else {
        set.fail("E17-规格-08-测量只读不缓存", "测量非纯或快照被改");
    }
}

// ---------------------------------------------------------------------------
// 二、边界（四段错误码逐段可达 + 异步超时语义）
// ---------------------------------------------------------------------------

fn chk_bound_encoding_font(set: &mut CheckSet) {
    // 边界-01：编码段可达——非 UTF-8 拒测，码恰为 0x1700。
    let snap = good_snapshot();
    let params = good_params();
    let bad: [u8; 2] = [0xD8, 0x00]; // 孤立高位代理 = 非 UTF-8
    match measure_text(&bad, &snap, &params, &MeasureOptions::sync()) {
        Err(e) => {
            if e.code() == 0x1700 && e.segment() == "编码" {
                set.ok("E17-边界-01-编码段可达");
            } else {
                set.fail("E17-边界-01-编码段可达", "码或段漂移");
            }
        }
        Ok(_) => set.fail("E17-边界-01-编码段可达", "坏编码未拒测"),
    }
    // 边界-02：字体段两闸——字号越界与空实例，码互异且都在字体段。
    let tiny = TypesetParams { font_size_q16: 63, ..good_params() };
    let e1 = measure_text(b"A", &snap, &tiny, &MeasureOptions::sync());
    let empty = MetricSnapshot {
        ascent: 0,
        descent: 0,
        recommended_line_height: 0,
        advance: 0,
        cap_height: 0,
        x_height: 0,
        underline_position: 0,
        underline_thickness: 0,
        strikeout_position: 0,
        strikeout_thickness: 0,
        ascent_source: MetricSource::EmFallback,
        descent_source: MetricSource::EmFallback,
        line_height_source: MetricSource::EmFallback,
        conflict_warned: false,
        degraded: true,
    };
    let e2 = measure_text(b"A", &empty, &good_params(), &MeasureOptions::sync());
    let font_ok = matches!(e1, Err(TextApiError::Font { code: E_TEXTAPI_SIZE_RANGE, .. }))
        && matches!(e2, Err(TextApiError::Font { code: E_TEXTAPI_EMPTY_INSTANCE, .. }));
    if font_ok {
        set.ok("E17-边界-02-字体段双闸");
    } else {
        set.fail("E17-边界-02-字体段双闸", "字号闸或空实例闸未按码拒绝");
    }
}

fn chk_bound_async(set: &mut CheckSet) {
    // 边界-03：异步句柄语义——timeout>0 得句柄、同请求同 ticket、
    // 未超时保真、超时置低置信。
    let snap = good_snapshot();
    let params = good_params();
    let opts = MeasureOptions { async_timeout_us: 1_000, lang_tag: 0 };
    let a = measure_text(b"XY", &snap, &params, &opts);
    let b = measure_text(b"XY", &snap, &params, &opts);
    let (h1, h2) = match (a, b) {
        (Ok(MeasureOutcome::Pended(h1)), Ok(MeasureOutcome::Pended(h2))) => (h1, h2),
        _ => {
            set.fail("E17-边界-03-异步句柄语义", "异步模式未返回句柄");
            return;
        }
    };
    if h1 != h2 {
        set.fail("E17-边界-03-异步句柄语义", "同请求 ticket 不同（幂等前提破坏）");
        return;
    }
    if let Ok(MeasureOutcome::Sync(res)) =
        measure_text(b"XY", &snap, &params, &MeasureOptions::sync())
    {
        let in_time = resolve_measure(h1, res, 500, 0).low_confidence == false;
        let late = resolve_measure(h1, res, 2_000, 0).low_confidence == true;
        if in_time && late {
            set.ok("E17-边界-03-异步句柄语义");
        } else {
            set.fail("E17-边界-03-异步句柄语义", "超时语义不随 deadline 翻面");
        }
    } else {
        set.fail("E17-边界-03-异步句柄语义", "同步基线测量失败");
    }
}

fn chk_bound_cache_pipeline(set: &mut CheckSet) {
    // 边界-04：缓存段两闸——未 init 拒绝、预热超容拒绝且不部分预热。
    let mut zero = AtlasGate::new(0);
    let uninit = cache_control(&mut zero, CacheOp::Watermark);
    let mut gate = AtlasGate::new(100);
    gate.resident = 90;
    let over = cache_control(&mut gate, CacheOp::Warmup(20));
    let uninit_ok = matches!(uninit, Err(TextApiError::Cache { code: E_TEXTAPI_CACHE_UNINIT, .. }));
    let over_ok = matches!(over, Err(TextApiError::Cache { code: E_TEXTAPI_CACHE_FULL, .. }))
        && gate.resident == 90;
    if uninit_ok && over_ok {
        set.ok("E17-边界-04-缓存段双闸");
    } else {
        set.fail("E17-边界-04-缓存段双闸", "未 init 或超容闸未按码拒绝/部分预热");
    }
    // 边界-05：管线段两闸——批上限拒绝、legacy 冲突拒绝。
    let mut big = good_request();
    big.text_len = 2_001;
    let limit = render_text(&big);
    let mut conflict = good_request();
    conflict.effect_flags = 0b1000;
    conflict.opts.legacy_glow = Some(128);
    let conflict = render_text(&conflict);
    let pipe_ok = matches!(limit, Err(TextApiError::Pipeline { code: E_TEXTAPI_BATCH_LIMIT, .. }))
        && matches!(conflict, Err(TextApiError::Pipeline { code: E_TEXTAPI_LEGACY_CONFLICT, .. }));
    if pipe_ok {
        set.ok("E17-边界-05-管线段双闸");
    } else {
        set.fail("E17-边界-05-管线段双闸", "批上限或双轨冲突闸未按码拒绝");
    }
}

// ---------------------------------------------------------------------------
// 三、幂等（RenderText 内容指纹 / 度量确定性）
// ---------------------------------------------------------------------------

fn chk_idem_render(set: &mut CheckSet) {
    // 幂等-01：同请求两次提交收据全等。
    let req = good_request();
    let r1 = render_text(&req);
    let r2 = render_text(&req);
    if r1.is_ok() && r1 == r2 {
        set.ok("E17-幂等-01-同请求同收据");
    } else {
        set.fail("E17-幂等-01-同请求同收据", "收据随调用漂移（幂等破坏）");
    }
    // 幂等-02：内容敏感性——每个语义字段变，指纹必变（逐字段变异扫）。
    let base = render_text(&good_request());
    let mut all_sensitive = true;
    let mut variant = good_request();
    variant.text_len += 1;
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    let mut variant = good_request();
    variant.text_hash ^= 1;
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    let mut variant = good_request();
    variant.params.font_size_q16 += 64;
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    let mut variant = good_request();
    variant.dst_x_q16 += 1;
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    let mut variant = good_request();
    variant.effect_flags = 1;
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    let mut variant = good_request();
    variant.opts.z_q16 = -1;
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    let mut variant = good_request();
    variant.opts.legacy_glow = Some(1);
    if render_text(&variant) == base {
        all_sensitive = false;
    }
    if all_sensitive {
        set.ok("E17-幂等-02-指纹内容敏感");
    } else {
        set.fail("E17-幂等-02-指纹内容敏感", "存在语义字段变异后指纹不变");
    }
    // 幂等-03：空文本合法（空批收据，quad=0）。
    let mut empty = good_request();
    empty.text_len = 0;
    match render_text(&empty) {
        Ok(rc) if rc.quad_count == 0 => set.ok("E17-幂等-03-空批合法"),
        _ => set.fail("E17-幂等-03-空批合法", "空文本被拒或 quad 计数错"),
    }
}

fn chk_idem_measure(set: &mut CheckSet) {
    // 幂等-04：度量确定性——换行走行数翻倍、字距推进单调、多行取最长行。
    let snap = good_snapshot();
    let opts = MeasureOptions::sync();
    // 换行：两行各行 1 字形 → glyph_count=2，行高×2。
    let two = measure_text(b"A\nB", &snap, &good_params(), &opts);
    let one = measure_text(b"AB", &snap, &good_params(), &opts);
    let (two_ok, one_ok) = match (two, one) {
        (Ok(MeasureOutcome::Sync(t)), Ok(MeasureOutcome::Sync(o))) => {
            (t.bound_h_q16 == o.bound_h_q16 * 2 && t.glyph_count == 2, o.advance_q16 == 640)
        }
        _ => (false, false),
    };
    if two_ok && one_ok {
        set.ok("E17-幂等-04-度量确定性");
    } else {
        set.fail("E17-幂等-04-度量确定性", "换行/推进模型偏离确定性");
    }
    // 幂等-05：字距影响推进（spacing=+64 → 每字形多 1px）。
    let spaced = TypesetParams { letter_spacing_q16: 64, ..good_params() };
    match measure_text(b"AB", &snap, &spaced, &opts) {
        Ok(MeasureOutcome::Sync(res)) if res.advance_q16 == 640 + 2 * 64 => {
            set.ok("E17-幂等-05-字距进推进");
        }
        _ => set.fail("E17-幂等-05-字距进推进", "字距未按预期进推进"),
    }
}

// ---------------------------------------------------------------------------
// 四、降级（图集水位 / 双轨渐弃）
// ---------------------------------------------------------------------------

fn chk_degrade_watermark(set: &mut CheckSet) {
    // 降级-01：水位计算与高水位线两侧翻面（969 假 / 970 真——边界钉死）。
    let mut gate = AtlasGate::new(1000);
    let _ = cache_control(&mut gate, CacheOp::Warmup(969));
    let w969 = cache_control(&mut gate, CacheOp::Watermark);
    let _ = cache_control(&mut gate, CacheOp::Warmup(1));
    let w970 = cache_control(&mut gate, CacheOp::Watermark);
    let ok = matches!(
        (&w969, &w970),
        (
            Ok(CacheWatermark { permille: 969, high: false, .. }),
            Ok(CacheWatermark { permille: 970, high: true, .. })
        )
    );
    if ok {
        set.ok("E17-降级-01-水位线两侧翻面");
    } else {
        set.fail("E17-降级-01-水位线两侧翻面", "水位计算或阈值判定漂移");
    }
    // 降级-02：Evict 清零且 high 翻假。
    let _ = cache_control(&mut gate, CacheOp::Evict);
    match cache_control(&mut gate, CacheOp::Watermark) {
        Ok(CacheWatermark { used_slots: 0, permille: 0, high: false, .. }) => {
            set.ok("E17-降级-02-逐出清零");
        }
        _ => set.fail("E17-降级-02-逐出清零", "逐出后水位未归零"),
    }
    // 降级-03：双轨渐弃——legacy 命中计数递增且收据带标记；版本倒计时恰 2。
    let mut legacy = good_request();
    legacy.opts.legacy_glow = Some(64);
    let before = deprecation_hits();
    let hit = render_text(&legacy);
    let after = deprecation_hits();
    let ok = match hit {
        Ok(rc) => rc.legacy_warned && after == before + 1,
        Err(_) => false,
    } && DEPRECATION_VERSIONS_LEFT == 2;
    if ok {
        set.ok("E17-降级-03-双轨渐弃计数");
    } else {
        set.fail("E17-降级-03-双轨渐弃计数", "废弃命中未计数或倒计时漂移");
    }
}

// ---------------------------------------------------------------------------
// 五、判据承载力（零 panic 面 / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

/// 单遍词法剥除（与 F2807 同源；字符串内 `//` 不得误伤——F2807 实测教训）。
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

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vee17_textapi_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E17-判据-01-判据面零 panic");
    } else {
        set.fail("E17-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-09：生产面零 panic（扫实现文件）。
    let src = include_str!("vee17_textapi.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E17-规格-09-生产面零 panic");
    } else {
        set.fail("E17-规格-09-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——七码高 8 位皆 0x17、互异，且与判据侧
    // 写死的期望逐位相等（不走实现侧常量）。
    let got = [
        E_TEXTAPI_BAD_UTF8,
        E_TEXTAPI_EMPTY_INSTANCE,
        E_TEXTAPI_SIZE_RANGE,
        E_TEXTAPI_CACHE_FULL,
        E_TEXTAPI_CACHE_UNINIT,
        E_TEXTAPI_BATCH_LIMIT,
        E_TEXTAPI_LEGACY_CONFLICT,
    ];
    let mut all_17 = true;
    let mut distinct = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x1700 {
            all_17 = false;
        }
        if got[i] != EXPECT_CODES[i] {
            all_17 = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    if all_17 && distinct {
        set.ok("E17-判据-02-码段独占独立复核");
    } else {
        set.fail("E17-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——**只**调用不递归的 A 族，本族 tally 用进行中的
    // set 自身；绝不自调 B 族或 merged 入口（F2806 无限递归教训、F0817
    // 首版栈溢出复发教训：族内判据调本族/全域入口 = 自我递归）。
    // 条数期望判据侧写死（A 族 18 条；判据-03 登记前 B 族进行中 6 条；
    // 合计 25 条）。
    let a = run_vee17_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 18;
    let self_ok = sp + sf == 6;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E17-判据-03-聚合守恒防自调");
    } else {
        set.fail("E17-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+幂等 / b=降级+判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 幂等。
pub fn run_vee17_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vee17/a");
    chk_spec_three_fns(&mut s);
    chk_spec_presentation(&mut s);
    chk_spec_signature_frozen(&mut s);
    chk_spec_options_extension(&mut s);
    chk_bound_encoding_font(&mut s);
    chk_bound_async(&mut s);
    chk_bound_cache_pipeline(&mut s);
    chk_idem_render(&mut s);
    chk_idem_measure(&mut s);
    s
}

/// 判据族 b：降级 + 判据承载力。
pub fn run_vee17_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vee17/b");
    chk_degrade_watermark(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vee17_checks() -> CheckSet {
    CheckSet::merge(run_vee17_checks_a_standalone(), run_vee17_checks_b_standalone())
}
