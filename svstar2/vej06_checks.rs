//! VE-F1806 · 域自检（判据逐条对应，见 `vej06_area.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 接口预留 → `J06-接口-类型枚举注册`（枚举编码双向自洽 + 预留位可登记）
//! - 诚实标注 → `J06-诚实-贡献恒零与标注`、`J06-诚实-LTC标注预期非实测`
//! - 前向兼容 → `J06-前向-约束文本在册`、`J06-前向-扩展字段追加不插入`
//! - LTC 研究位 → `J06-LTC-研究位无代码路径`
//! - 显性报错 → `J06-报错-预留类型不静默`、`J06-报错-指路替代方案`
//!
//! 门禁设计纪律：
//! ① 用表外形态——枚举自洽用「未知码」而非表内元素互验（表内验表恒真）；
//! ② 诚实标注判据必须能识别「把研究位写成已实现」的退化表述，
//!    故断言刻意包含否定式检查（不得出现「已实现」类措辞）；
//! ③ 每族至少两项判据——单点判据等于无门禁。

use super::vej06_area::*;
use crate::checks::CheckSet;

/// 合法的参数块草案（各判据共用前置）。
fn draft_ok() -> AreaLight {
    AreaLight::draft(
        AreaLightKind::Rect,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, -1.0),
        (1.0, 1.0, 1.0),
        1.0,
        10.0,
        2.0,
        3.0,
        0.0,
        false,
    )
    .unwrap()
}

/// VE-F1806 域自检。
pub fn run_vej06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vej06");

    // ---- 判据「接口预留」 ----

    // 枚举双向自洽：码→类型→码 恒等（表内元素验自身，属弱门禁，
    // 故与下面「未知码不被猜测」分列成两项）。
    {
        let mut roundtrip = true;
        for k in AREA_KINDS {
            if AreaLightKind::from_code(k.code()) != Some(k) {
                roundtrip = false;
            }
        }
        // 两种类型编码必须不同（否则注册表塌缩成一个类型）。
        let distinct = AREA_KINDS[0].code() != AREA_KINDS[1].code();
        let labels_ok = AREA_KINDS
            .iter()
            .all(|k| k.label().contains("预留") || k.label().contains('('));
        set.add(
            "J06-接口-类型枚举注册",
            roundtrip && distinct && labels_ok,
            "",
        );
    }

    // 预留位可登记但默认空（接口位存在、不占值）。
    {
        let a = draft_ok();
        let slots_empty = a.ltc_matrix_ref.is_none() && a.contact_hardening_ref.is_none();
        // 枚举注册表恰为两项且都被登记。
        let reg = registered_kinds();
        set.add(
            "J06-接口-预留位可登记",
            slots_empty && reg.len() == 2 && reg == AREA_KINDS,
            "",
        );
    }

    // ---- 判据「诚实标注」 ----

    // 贡献恒零 + 标注可读（不假装可用）。
    {
        let a = draft_ok();
        let zero_near = a.contribution((1.0, 1.0, 1.0)) == (0.0, 0.0, 0.0);
        let zero_far = a.contribution((1000.0, 1000.0, 1000.0)) == (0.0, 0.0, 0.0);
        let note_honest = a.not_implemented_note().contains("未实现");
        set.add(
            "J06-诚实-贡献恒零与标注",
            zero_near && zero_far && note_honest,
            "",
        );
    }

    // LTC 标注必须声明「预期值非实测」——防止研究位被当成已实现结论。
    {
        let doc = LTC_RESEARCH_NOTE;
        let marks_research = doc.contains("研究位") && doc.contains("一期无实现");
        // 关键诚实性：成本数字必须带「预期/非实测」限定。
        let marks_estimate = doc.contains("预期") && doc.contains("非实测");
        // 且必须指明实测入账本（可追溯）。
        let marks_ledger = doc.contains("F1811");
        set.add(
            "J06-诚实-LTC标注预期非实测",
            marks_research && marks_estimate && marks_ledger,
            "",
        );
    }

    // ---- 判据「前向兼容」 ----

    // 约束文本在册且含关键冻结项。
    {
        let doc = AREA_FWD_COMPAT_DOC;
        let frozen_fields = doc.contains("pos(0)") && doc.contains("range(4)");
        let append_only = doc.contains("只能追加");
        let ltc_optional = doc.contains("不得转为必填参数");
        set.add(
            "J06-前向-约束文本在册",
            frozen_fields && append_only && ltc_optional,
            "",
        );
    }

    // 扩展字段只追加在既有五项之后（顺序纪律，可执行验证）。
    {
        // 既有五项语义冻结：构造出的草案前五项与入参逐一对齐。
        let a = draft_ok();
        let base_ok = a.pos == (0.0, 0.0, 0.0)
            && a.dir == (0.0, 0.0, -1.0)
            && a.color == (1.0, 1.0, 1.0)
            && a.intensity == 1.0
            && a.range == 10.0;
        // 扩展项独立取值，不污染既有五项。
        let ext_ok = a.width == 2.0 && a.height == 3.0 && a.orientation == 0.0 && !a.two_sided;
        set.add("J06-前向-扩展字段追加不插入", base_ok && ext_ok, "");
    }

    // ---- 判据「LTC 研究位」 ----

    // 研究位无代码路径：绑定引用位后贡献仍恒零（防「绑纹理即实现」误解）。
    {
        let mut a = draft_ok();
        a.bind_ltc_matrix(42);
        let still_zero = a.contribution((3.0, 3.0, 3.0)) == (0.0, 0.0, 0.0);
        // 引用位可读回（登记确实生效，但只登记不消费）。
        let ref_ok = a.ltc_matrix_ref == Some(42);
        set.add("J06-LTC-研究位无代码路径", still_zero && ref_ok, "");
    }

    // ---- 判据「显性报错」 ----

    // 引用预留类型 → 报错（非静默放行），且三要素齐发。
    //
    // **门禁纪律**：此处不得用 `unwrap_err()`——预留类型若被改成静默放行，
    // `unwrap_err` 会 panic，判据以「崩溃」而非「变红」表达失效（实测踩过：
    // 注入静默放行后自检直接 panic，看不出是哪条判据失守）。
    // 改为显式 match，把「该报错却没报错」如实记为红项。
    {
        let describe_of = |r: Result<(), AreaError>| -> Option<String> {
            match r {
                Ok(()) => None,
                Err(e) => Some(e.describe()),
            }
        };
        let rect = describe_of(request_area_light(AreaLightKind::Rect));
        let tube = describe_of(request_area_light(AreaLightKind::Tube));
        let rect_rejected = rect.is_some();
        let tube_rejected = tube.is_some();
        let three = rect
            .as_deref()
            .map(|d| d.contains("未实现") && d.contains("下一步") && d.contains("预留"))
            .unwrap_or(false);
        // 错误码断言（不用 unwrap_err）：确认是「未实现」而非别的错。
        let right_variant = matches!(
            request_area_light(AreaLightKind::Rect),
            Err(AreaError::NotImplemented { code: 0x0300, .. })
        );
        set.add(
            "J06-报错-预留类型不静默",
            rect_rejected && tube_rejected && three && right_variant,
            "",
        );
    }

    // 报错必须指路当前替代方案（锚点显式要求）。
    {
        let soft = substitute_plan(0.1);
        let hard = substitute_plan(0.0);
        let heavy = substitute_plan(0.9);
        let illegal = substitute_plan(f32::NAN);
        // 硬边需求给点光（不是阵列）；强软度需求建议等待。
        let routes = soft.contains("阵列") && hard.contains("点光") && heavy.contains("等待");
        // 软度非法时也必须给出可读回应，不 panic 不空串。
        let illegal_ok = !illegal.is_empty() && illegal.contains("面光源");
        set.add("J06-报错-指路替代方案", routes && illegal_ok, "");
    }

    // 未知码不猜测类型（表外形态：0x0999 不在登记表内）。
    // 同样不用 unwrap_err——静默放行时应变红而非 panic。
    {
        let unknown_err = match request_area_light_by_code(0x0999) {
            Err(AreaError::UnknownKind { code: 0x0999 }) => Some(true),
            Err(_) => Some(false), // 报成了别的错：类型映射有偏
            Ok(_) => None,         // 静默放行：判据失守
        };
        let no_guess = match request_area_light_by_code(0x0999) {
            Err(e) => e.describe().contains("不猜测类型"),
            Ok(_) => false,
        };
        // 反向门禁：已登记码报「未实现」而非「未知」。
        let known = matches!(
            request_area_light_by_code(AreaLightKind::Rect.code()),
            Err(AreaError::NotImplemented { .. })
        );
        set.add(
            "J06-报错-未知码不猜测",
            unknown_err == Some(true) && no_guess && known,
            "",
        );
    }

    // 预留类型不参与调度（一期恒否）。
    {
        let none_schedules = AREA_KINDS.iter().all(|k| !participates_in_scheduling(*k));
        set.add("J06-调度-预留类型不参与", none_schedules, "");
    }

    set
}
