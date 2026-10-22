//! F1010 判据域 · 形态 / 优先级 / 色键 / 截断 / 拒绝
//! （判据一、二、三与错误路径一、二；判据锚点见外壳 `vef12_checks.rs` 头注）。
//!
//! 判据 ID 与失败措辞逐字保留；判据之间互不调用，
//! 被测语义统一来自 [`crate::svstar2::vef12_alpha`]。

use crate::checks::CheckSet;

use crate::svstar2::vef02_pngenc::ColorType;
use crate::svstar2::vef12_alpha::*;

// =====================================================================
// 判据一：全透明形态（锚点「alpha 通道 / 调色板索引 / 灰度真彩色键」）
// =====================================================================

pub(super) fn check_forms(s: &mut CheckSet) {
    // 形态 1：alpha 通道（类型 4/6，无 tRNS）。
    let o = resolve_trns(ColorType::Rgba, 8, &[], 0, false);
    s.add(
        "C1010-形态-alpha通道",
        matches!(&o, Ok(r) if r.form == TransparencyForm::AlphaChannel && r.warnings.is_empty()),
        "类型 6 无 tRNS ⇒ AlphaChannel 无告警",
    );
    // 形态 2：调色板透明索引。
    let o = resolve_trns(ColorType::Palette, 8, &[255, 128, 0], 3, true);
    s.add(
        "C1010-形态-调色板索引",
        matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), warnings }) if a.len() == 3 && a[1] == 128 && warnings.is_empty()),
        "类型 3 的 tRNS 是 PLTE 尺寸内的 per-entry alpha 数组",
    );
    // 形态 3：灰度色键。
    let o = resolve_trns(ColorType::Gray, 16, &[0x12, 0x34], 0, true);
    s.add(
        "C1010-形态-灰度色键",
        matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::ColorKey(ColorKey::Gray(0x1234)), warnings }) if warnings.is_empty()),
        "类型 0 的 tRNS 是 2 字节大端灰度键",
    );
    // 形态 4：真彩色键（三分量逐一还原）。
    let o = resolve_trns(ColorType::Rgb, 16, &[0, 1, 0, 2, 0, 3], 0, true);
    s.add(
        "C1010-形态-真彩色键",
        matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::ColorKey(ColorKey::Rgb(1, 2, 3)), warnings }) if warnings.is_empty()),
        "类型 2 的 tRNS 是 6 字节大端 RGB 键",
    );
    // 形态 5：无透明（有 tRNS 之前）。
    let o0 = resolve_trns(ColorType::Gray, 8, &[], 0, false);
    let o2 = resolve_trns(ColorType::Rgb, 8, &[], 0, false);
    s.add(
        "C1010-形态-无透明",
        matches!(&o0, Ok(TrnsOutcome { form: TransparencyForm::None, warnings }) if warnings.is_empty())
            && matches!(o2, Ok(TrnsOutcome { form: TransparencyForm::None, .. })),
        "类型 0/2 无 tRNS ⇒ 无透明语义",
    );
}

// =====================================================================
// 判据二：优先级规则（tRNS+alpha 共存 → alpha 优先 + 计数告警）
// =====================================================================

pub(super) fn check_priority(s: &mut CheckSet) {
    let o = resolve_trns(ColorType::GrayAlpha, 8, &[255, 255], 0, true);
    s.add(
        "C1010-优先-alpha胜出",
        matches!(&o, Ok(r) if r.form == TransparencyForm::AlphaChannel),
        "tRNS 与 alpha 共存 ⇒ 语义以 alpha 为准（锚点原文）",
    );
    s.add(
        "C1010-优先-忽略计数告警",
        matches!(&o, Ok(r) if r.warning_count(true) == 1 && r.warning_count(false) == 0),
        "tRNS 被忽略必须计数告警；静默忽略是无从排查的事故",
    );
    // 两类 alpha 颜色类型都仲裁一致。
    let o6 = resolve_trns(ColorType::Rgba, 16, &[1, 2, 3, 4, 5, 6], 0, true);
    let o4 = resolve_trns(ColorType::GrayAlpha, 16, &[1, 2], 0, true);
    s.add(
        "C1010-优先-四六型一致",
        matches!(&o6, Ok(r) if r.form == TransparencyForm::AlphaChannel && r.warning_count(true) == 1)
            && matches!(&o4, Ok(r) if r.form == TransparencyForm::AlphaChannel && r.warning_count(true) == 1),
        "类型 4 与类型 6 的共存仲裁口径必须一致",
    );
}

// =====================================================================
// 判据三：色键精确匹配（语义是相等，不是范围）
// =====================================================================

pub(super) fn check_color_key(s: &mut CheckSet) {
    let key = ColorKey::Gray(0x00FF);
    s.add(
        "C1010-色键-精确匹配命中",
        matches!(key_matches(&key, &[0x00FF]), Ok(true)),
        "样本与键全等 ⇒ 透明",
    );
    // 【弱门禁二】差 1 的邻值必须**不**匹配——只断正向挡不住范围匹配。
    s.add(
        "C1010-色键-邻值不透明",
        matches!(key_matches(&key, &[0x0100]), Ok(false))
            && matches!(key_matches(&key, &[0x00FE]), Ok(false)),
        "差 1 的两侧邻值都不透明；范围式匹配会把整段亮度抠空",
    );
    let rgb = ColorKey::Rgb(10, 20, 30);
    s.add(
        "C1010-色键-真彩逐分量全等",
        matches!(key_matches(&rgb, &[10, 20, 30]), Ok(true))
            && matches!(key_matches(&rgb, &[10, 20, 31]), Ok(false))
            && matches!(key_matches(&rgb, &[11, 20, 30]), Ok(false)),
        "任一分量不等即不透明（精确匹配逐分量）",
    );
    s.add(
        "C1010-色键-形状不符报错",
        matches!(
            key_matches(&ColorKey::Gray(1), &[1, 2]),
            Err(Fault::SampleShapeMismatch { want: 1, got: 2 })
        ),
        "灰度键配 2 样本是调用方 bug，报错不猜",
    );
}

// =====================================================================
// 错误路径一：tRNS 超长截断 + 告警（锚点原文）
// =====================================================================

/// 截断域编排：超长主体 → 边界裁决 → 稀疏/缺省语义（顺序即判据序）。
pub(super) fn check_truncation(s: &mut CheckSet) {
    truncation_overlong(s);
    truncation_boundary(s);
    truncation_defaulted(s);
}

/// 超长截断主体：截到合法长度 + 告警留痕（kept/dropped 可查）。
fn truncation_overlong(s: &mut CheckSet) {
    // 5 项 alpha、PLTE 只有 3 项 ⇒ 截断到 3 并告警 kept=3 dropped=2。
    let o = resolve_trns(ColorType::Palette, 8, &[255, 128, 0, 64, 32], 3, true);
    s.add(
        "C1010-截断-超PLTE截到合法长度",
        matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), .. }) if a.len() == 3 && a[0] == 255 && a[2] == 0),
        "超长部分按锚点语义丢弃，保留部分照用（有损降级非报错）",
    );
    s.add(
        "C1010-截断-告警记kept与dropped",
        matches!(
            &o,
            Ok(r) if matches!(
                r.warnings.as_slice(),
                [TrnsWarning::TruncatedToPlte { kept: 3, dropped: 2 }, ..]
            )
        ),
        "截断必须留痕（kept/dropped 可查）；静默截断会让「少了两项透明」无从对账",
    );
}

/// 截断边界：恰超长 1 项、恰等长、无 PLTE 三种裁决都钉死。
fn truncation_boundary(s: &mut CheckSet) {
    // 截断边界：恰超长 1 项（payload = PLTE+1）也必须截且告警——
    // 阈值 off-by-one（`>` 写成 `>=` 或 +1）恰在这一项上漏网。
    let o_b = resolve_trns(ColorType::Palette, 8, &[7, 8, 9, 10], 3, true);
    s.add(
        "C1010-截断-恰超长一项也截",
        matches!(&o_b, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), .. }) if a.len() == 3 && a[2] == 9)
            && matches!(&o_b, Ok(r) if matches!(
                r.warnings.as_slice(),
                [TrnsWarning::TruncatedToPlte { kept: 3, dropped: 1 }, ..]
            )),
        "payload=PLTE+1 是截断语义的最小正例；阈值差一会让超长部分静默生效",
    );
    // 恰好等长不告警。
    let o_eq = resolve_trns(ColorType::Palette, 8, &[9, 9, 9], 3, true);
    s.add(
        "C1010-截断-等长不告警",
        matches!(&o_eq, Ok(r) if r.warnings.is_empty() && r.warning_count(false) == 0),
        "长度等于 PLTE 是合法形态，不得误报",
    );
    // 调色板图无 PLTE 却有 tRNS ⇒ 拒。
    s.add(
        "C1010-截断-无PLTE拒绝",
        matches!(
            resolve_trns(ColorType::Palette, 8, &[1, 2], 0, true),
            Err(Fault::PlteMissing)
        ),
        "没有 PLTE 的 alpha 数组无从定界，拒块",
    );
}

/// 稀疏/缺省语义：短 tRNS 合法、缺省项全不透明、缺省查表越界仍拒。
fn truncation_defaulted(s: &mut CheckSet) {
    // 短 tRNS（< PLTE）是合法稀疏形态：不是截断也不告警——
    // 「截断」只指超长丢弃；短于 PLTE 是规范明文允许的缺省形态。
    let o_short = resolve_trns(ColorType::Palette, 8, &[64, 128], 4, true);
    s.add(
        "C1010-形态-短tRNS稀疏合法不告警",
        matches!(&o_short, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), warnings }) if a.len() == 2 && warnings.is_empty()),
        "tRNS 短于 PLTE 不是错误：数组在 PLTE 尺寸内稀疏合法，截断语义只管超长支",
    );
    // 缺省项全不透明（ISO/IEC 15948 §4.3.2.1）：范围内的未写项查得 255，
    // 不是 Err——把缺省当越界会让合法稀疏 tRNS 的整段调色板变故障。
    s.add(
        "C1010-形态-缺省项全不透明",
        matches!(palette_alpha_defaulted(&[64], 3, 0), Ok(64))
            && matches!(palette_alpha_defaulted(&[64], 3, 1), Ok(255))
            && matches!(palette_alpha_defaulted(&[64], 3, 2), Ok(255)),
        "tRNS 未写到的调色板项按规范全不透明（255），不是越界也不是随机值",
    );
    // 缺省查表的越界语义与严查表一致：PLTE 外仍拒、无 PLTE 仍拒。
    s.add(
        "C1010-形态-缺省查表越界仍拒",
        matches!(
            palette_alpha_defaulted(&[64], 3, 3),
            Err(Fault::AlphaIndexOutOfRange { index: 3, count: 3 })
        ) && matches!(
            palette_alpha_defaulted(&[64], 0, 0),
            Err(Fault::PlteMissing)
        ),
        "缺省语义只放宽 PLTE 内的未写项；PLTE 外索引与无 PLTE 照旧拒块",
    );
}

// =====================================================================
// 错误路径二：色键与位深不匹配 → 拒绝该块（锚点原文）
// =====================================================================

/// 拒绝域编排：量程边界 → 输入合法性（顺序即判据序）。
pub(super) fn check_key_reject(s: &mut CheckSet) {
    key_reject_range(s);
    key_reject_payload(s);
}

/// 量程边界：灰度/真彩键越界拒、满量程边界不误拒。
fn key_reject_range(s: &mut CheckSet) {
    // 灰度键 300、位深 8（量程 0..=255）⇒ 拒。
    s.add(
        "C1010-拒绝-灰度键越界",
        matches!(
            resolve_trns(ColorType::Gray, 8, &[0x01, 0x2C], 0, true),
            Err(Fault::KeyOutOfRange { key: 300, max: 255 })
        ),
        "键值超出 2^位深−1 ⇒ 拒块；按错误口径匹配会抠掉不该透明的像素",
    );
    // 真彩键任一分量越界 ⇒ 拒整块。
    s.add(
        "C1010-拒绝-真彩键分量越界",
        matches!(
            resolve_trns(ColorType::Rgb, 8, &[0, 10, 0, 20, 1, 0], 0, true),
            Err(Fault::KeyOutOfRange { .. })
        ),
        "蓝分量 256 > 255 ⇒ 拒整块（半校验半使用 = 口径分裂）",
    );
    // 位深 16 的满量程键合法（边界不误拒）。
    let o = resolve_trns(ColorType::Gray, 16, &[0xFF, 0xFF], 0, true);
    s.add(
        "C1010-拒绝-满量程不误拒",
        matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::ColorKey(ColorKey::Gray(0xFFFF)), .. })),
        "位深 16 的键 0xFFFF 恰在量程内；把上界写成 < 会误拒合法文件",
    );
}

/// 输入合法性：位深非法先拒、键载荷长度不符先拒。
fn key_reject_payload(s: &mut CheckSet) {
    // 位深本身非法（IHDR 不可信输入）。
    s.add(
        "C1010-拒绝-位深非法",
        matches!(
            resolve_trns(ColorType::Gray, 0, &[0, 1], 0, true),
            Err(Fault::BadBitDepth { bit_depth: 0 })
        ) && matches!(
            resolve_trns(ColorType::Gray, 17, &[0, 1], 0, true),
            Err(Fault::BadBitDepth { bit_depth: 17 })
        ),
        "位深 0 与 >16 都先拒（1u32<<17 不溢出但语义荒谬）",
    );
    // 载荷长度不符（灰度须 2、真彩须 6）。
    s.add(
        "C1010-拒绝-键载荷长度不符",
        matches!(
            resolve_trns(ColorType::Gray, 8, &[1], 0, true),
            Err(Fault::KeyPayloadLength { want: 2, got: 1 })
        ) && matches!(
            resolve_trns(ColorType::Rgb, 8, &[1, 2, 3, 4], 0, true),
            Err(Fault::KeyPayloadLength { want: 6, got: 4 })
        ),
        "灰度 2 字节、真彩 6 字节；长度不对说明解析口径已错",
    );
}
