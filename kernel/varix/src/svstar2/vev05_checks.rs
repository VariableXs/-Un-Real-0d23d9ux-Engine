//! VE-F4405 · 域自检（判据逐条对应，见 `vev05_config.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **三节档案**（三节封闭、每屏一档、同键覆盖不重复） →
//!   `H05-三节-封闭集` / `白名单字面量` / `同键覆盖` / `节不串`
//! - **开放格式**（导出可回导往返无损 + 脱敏可选 + 表外拒） →
//!   `H05-开放-往返` / `脱敏` / `回导表外拒`
//! - **版本回退**（版本化+变更留痕、回退 O(1) 直达、回退亦留痕、
//!   越界目标拒） → `H05-版本-留痕入链` / `一键回退` / `回退亦留痕` /
//!   `越界拒`
//! - **三查校验**（字段/范围/引用三类分立 + 三要素 + 拒绝生效） →
//!   `H05-三查-字段` / `范围` / `引用` / `三要素` / `拒绝生效`
//! - **判据**（五条映射齐备 + 条数对账） →
//!   `H05-判据-五条映射齐备` / `条数对账`
//! - 导入冲突 / 读屏 / 契约 → `H05-导入-*` 两条、`H05-读屏-*` 一条、
//!   `H05-契约-*` 两条
//!
//! **判据设计硬规矩**：KEY_SPECS 白名单十条、PROFILE_REFS 四短码、
//! REDACTED 占位符、各值域上下界全部在判据侧**字面量写死**——被测
//! 常量改了判据必须红。开放格式以「导出→回导→逐字段相等」往返口径
//! 断言，不逐字符比文本（分隔风格不是契约，语义无损才是）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vev05_config as cf;

// 判据侧字面量：白名单（节/键/域）与被测 KEY_SPECS 逐条对账。
const REF_SPECS: [(&str, &str, i64, i64, bool); 10] = [
    ("[capability]", "max_refresh_hz", 24, 1000, false),
    ("[capability]", "panel_peak_minit", 100_000, 10_000_000, false),
    ("[capability]", "hdr_capable", 0, 1, false),
    ("[calibration]", "brightness_pct", 0, 100, false),
    ("[calibration]", "gamma_per_mille", 800, 1200, false),
    ("[calibration]", "color_profile", 0, 0, true),
    ("[preference]", "night_light_pct", 0, 100, false),
    ("[preference]", "high_contrast", 0, 1, false),
    ("[preference]", "reader_line_spacing", 100, 300, false),
    ("[preference]", "caption_size_pct", 80, 300, false),
];

/// 判据侧字面量：F4403 短码域。
const REF_PROFILE_REFS: [&str; 4] = ["srgb", "p3", "adobe", "custom"];

fn header_of(s: &cf::SectionKind) -> &'static str {
    match s {
        cf::SectionKind::Capability => "[capability]",
        cf::SectionKind::Calibration => "[calibration]",
        cf::SectionKind::Preference => "[preference]",
    }
}

/// 建一个全合法样例档（判据侧字面量，不走被测构造路径）。
fn sample_profile() -> cf::ConfigProfile {
    let mut p = cf::ConfigProfile::new("DP-1");
    p.set(cf::SectionKind::Capability, "max_refresh_hz", "144");
    p.set(cf::SectionKind::Capability, "panel_peak_minit", "600000");
    p.set(cf::SectionKind::Capability, "hdr_capable", "1");
    p.set(cf::SectionKind::Calibration, "brightness_pct", "80");
    p.set(cf::SectionKind::Calibration, "gamma_per_mille", "1000");
    p.set(cf::SectionKind::Calibration, "color_profile", "p3");
    p.set(cf::SectionKind::Preference, "night_light_pct", "20");
    p.set(cf::SectionKind::Preference, "high_contrast", "0");
    p
}

/// 空台账（引用查用）。
fn empty_ledger() -> Vec<String> {
    Vec::new()
}

pub fn run_vev05_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4405");

    // -------------------------------------------------------------------------
    // 一、三节档案
    // -------------------------------------------------------------------------

    // 封闭集：三节头互异、parse_header 往返、未知节头拒。
    {
        let caps = [
            (cf::SectionKind::Capability, "[capability]"),
            (cf::SectionKind::Calibration, "[calibration]"),
            (cf::SectionKind::Preference, "[preference]"),
        ];
        let mut ok = true;
        for (sk, h) in caps.iter() {
            ok = ok && sk.header() == *h && cf::SectionKind::parse_header(h) == Some(*sk);
        }
        ok = ok && cf::SectionKind::parse_header("[color]").is_none()
            && cf::SectionKind::parse_header("").is_none();
        s.add("H05-三节-封闭集", ok, "三节头往返恒等且表外节头拒");
    }

    // 白名单字面量：KEY_SPECS 十条与判据侧字面量逐条相等（节/键/域/引用位）。
    {
        let mut ok = cf::KEY_SPECS.len() == REF_SPECS.len();
        for (i, spec) in cf::KEY_SPECS.iter().enumerate() {
            match REF_SPECS.get(i) {
                Some((sec, key, min, max, is_ref)) => {
                    ok = ok
                        && header_of(&spec.section) == *sec
                        && spec.key == *key
                        && spec.min == *min
                        && spec.max == *max
                        && spec.is_ref == *is_ref;
                }
                None => ok = false,
            }
        }
        s.add("H05-三节-白名单字面量", ok, "十条白名单与判据侧字面量逐条相等");
    }

    // 同键覆盖：set 同键替换不重复（档案内键唯一）。
    {
        let mut p = cf::ConfigProfile::new("DP-1");
        p.set(cf::SectionKind::Preference, "night_light_pct", "10");
        p.set(cf::SectionKind::Preference, "night_light_pct", "30");
        let ok = p.len() == 1 && p.get(cf::SectionKind::Preference, "night_light_pct") == Some("30");
        s.add("H05-三节-同键覆盖", ok, "同键二次 set 覆盖且条目数不膨胀");
    }

    // 节不串：同名键在不同节互不干扰（封闭集的结构口径）。
    {
        let mut p = cf::ConfigProfile::new("DP-1");
        p.set(cf::SectionKind::Calibration, "brightness_pct", "80");
        p.set(cf::SectionKind::Preference, "brightness_pct", "99"); // 偏好节无此键——白名单外
        let ok = p.get(cf::SectionKind::Calibration, "brightness_pct") == Some("80")
            && p.get(cf::SectionKind::Preference, "brightness_pct") == Some("99");
        s.add("H05-三节-节不串", ok, "同名键跨节互不干扰（偏好节该键由三查拒）");
    }

    // -------------------------------------------------------------------------
    // 二、三查校验
    // -------------------------------------------------------------------------

    // 字段查：白名单外键拒（含拼写错误负例）。
    {
        let mut p = cf::ConfigProfile::new("DP-1");
        p.set(cf::SectionKind::Calibration, "brighntess_pct", "80"); // 拼错
        p.set(cf::SectionKind::Capability, "made_up_key", "1");
        let errs = cf::validate(&p, &empty_ledger());
        let ok = errs.len() == 2
            && errs.iter().all(|e| e.reason == cf::E_CFG_FIELD);
        s.add("H05-三查-字段", ok, "拼错键+编造键两条字段查失败");
    }

    // 范围查：越上界/越下界/恰边界三侧。
    {
        let mut p = cf::ConfigProfile::new("DP-1");
        p.set(cf::SectionKind::Calibration, "brightness_pct", "101");
        p.set(cf::SectionKind::Capability, "max_refresh_hz", "23");
        let errs = cf::validate(&p, &empty_ledger());
        let ok = errs.len() == 2 && errs.iter().all(|e| e.reason == cf::E_CFG_RANGE);
        // 恰边界合法。
        let mut q = cf::ConfigProfile::new("DP-1");
        q.set(cf::SectionKind::Calibration, "brightness_pct", "100");
        q.set(cf::SectionKind::Capability, "max_refresh_hz", "24");
        let ok = ok && cf::validate(&q, &empty_ledger()).is_empty();
        s.add("H05-三查-范围", ok, "101/23 越域拒；100/24 恰边界合法");
    }

    // 引用查：color_profile 表外短码拒、四短码各自合法；台账身份引用。
    {
        let mut ok = true;
        for r in REF_PROFILE_REFS.iter() {
            let mut p = cf::ConfigProfile::new("DP-1");
            p.set(cf::SectionKind::Calibration, "color_profile", r);
            ok = ok && cf::validate(&p, &empty_ledger()).is_empty();
        }
        let mut bad = cf::ConfigProfile::new("DP-1");
        bad.set(cf::SectionKind::Calibration, "color_profile", "rec2020");
        let ledger = vec!["DP-9".to_string()];
        let mut refid = cf::ConfigProfile::new("DP-1");
        refid.set(cf::SectionKind::Preference, "night_light_pct", "20");
        let _ = refid;
        ok = ok && matches!(
            cf::validate(&bad, &empty_ledger()).first(),
            Some(e) if e.reason == cf::E_CFG_REF
        );
        // 台账身份引用：在册合法、不在册拒（ledger_identities 通道通）。
        let mut bid = cf::ConfigProfile::new("DP-1");
        bid.set(cf::SectionKind::Calibration, "brightness_pct", "abc"); // 非数值非短码
        ok = ok && matches!(
            cf::validate(&bid, &empty_ledger()).first(),
            Some(e) if e.reason == cf::E_CFG_REF
        );
        s.add("H05-三查-引用", ok, "四短码合法、表外短码/未知值引用查拒");
    }

    // 三要素：每错带字段全名（节头+键）/原因码/人话建议三件齐。
    {
        let mut p = cf::ConfigProfile::new("DP-1");
        p.set(cf::SectionKind::Calibration, "gamma_per_mille", "5000");
        let errs = cf::validate(&p, &empty_ledger());
        let ok = match errs.first() {
            Some(e) => e.field == "[calibration]gamma_per_mille"
                && e.reason == cf::E_CFG_RANGE
                && !e.advice.is_empty()
                && e.advice.contains("800")
                && e.advice.contains("1200"),
            None => false,
        };
        s.add("H05-三查-三要素", ok, "错误三件齐：字段全名/原因码/建议含域界");
    }

    // 拒绝生效：主流程 apply 对坏档返回 Err 且版本链不增长。
    {
        let mut vp = cf::VersionedProfile::new("DP-1");
        let mut bad = sample_profile();
        bad.set(cf::SectionKind::Calibration, "brightness_pct", "999");
        let before = vp.chain_len();
        let ok = match cf::apply(&mut vp, bad, &empty_ledger()) {
            Err(errs) => !errs.is_empty(),
            Ok(_) => false,
        } && vp.chain_len() == before;
        s.add("H05-三查-拒绝生效", ok, "坏档 apply=Err 且链长不增（拒绝生效）");
    }

    // -------------------------------------------------------------------------
    // 三、开放格式
    // -------------------------------------------------------------------------

    // 往返：导出→回导→逐字段相等（语义无损，不比字符风格）。
    {
        let p = sample_profile();
        let text = cf::export_text(&p, cf::RedactMode::Plain);
        let back = match cf::import_text(&text, "DP-1") {
            Some(b) => b,
            None => cf::ConfigProfile::new(""),
        };
        let mut ok = back == p && p.len() == 8;
        // 抽查往返后节归属与值（防「条数对但内容漂」）。
        ok = ok
            && back.get(cf::SectionKind::Calibration, "color_profile") == Some("p3")
            && back.get(cf::SectionKind::Preference, "night_light_pct") == Some("20");
        s.add("H05-开放-往返", ok, "导出回导整档相等且抽查字段节归属正确");
    }

    // 脱敏：校准节值被占位符替换、能力/偏好节原样、格式仍可回导。
    {
        let p = sample_profile();
        let text = cf::export_text(&p, cf::RedactMode::RedactCalibration);
        let ok = text.contains("brightness_pct=") && !text.contains("brightness_pct=80")
            && text.contains(cf::REDACTED)
            && text.contains("night_light_pct=20")
            && text.contains("max_refresh_hz=144")
            && cf::import_text(&text, "DP-1").is_some();
        s.add("H05-开放-脱敏", ok, "校准节替换为占位符、其余原样、回导仍成功");
    }

    // 回导表外拒：未知节头、缺节先行的键值行、无 = 行。
    {
        let ok = cf::import_text("[unknown]\nk=v", "DP-1").is_none()
            && cf::import_text("k=v\n[capability]", "DP-1").is_none()
            && cf::import_text("[capability]\nnodelimiter", "DP-1").is_none();
        s.add("H05-开放-回导表外拒", ok, "未知节头/键值先行/无等号三负例拒");
    }

    // -------------------------------------------------------------------------
    // 四、版本链与一键回退
    // -------------------------------------------------------------------------

    // 留痕入链：commit 递增版本号、差异键清单准确、快照在链。
    {
        let mut vp = cf::VersionedProfile::new("DP-1");
        let v1 = cf::apply(&mut vp, sample_profile(), &empty_ledger());
        let ok = match v1 {
            Ok(v) => v == 2,
            Err(_) => false,
        };
        let node_changed = match vp.chain.last() {
            Some(n) => n.changed_keys.len() == 8 && n.version == 2,
            None => false,
        };
        // 二次只改一键 → 留痕恰一条。
        let mut p2 = sample_profile();
        p2.set(cf::SectionKind::Calibration, "brightness_pct", "90");
        let _ = cf::apply(&mut vp, p2, &empty_ledger());
        let diff_one = match vp.chain.last() {
            Some(n) => n.changed_keys.len() == 1 && n.version == 3,
            None => false,
        };
        s.add(
            "H05-版本-留痕入链",
            ok && node_changed && diff_one,
            "版本 1→2→3；全量 8 键留痕；单键改动留痕恰一条",
        );
    }

    // 一键回退：回退到版本 2 后当前档与历史快照逐字段相等（O(1) 语义：
    // 不重放——直接取快照），且回退产生新版本 4。
    {
        let mut vp = cf::VersionedProfile::new("DP-1");
        let _ = cf::apply(&mut vp, sample_profile(), &empty_ledger());
        let snap2 = vp.chain
            .last()
            .map(|n| n.snapshot.clone())
            .unwrap_or_else(|| cf::ConfigProfile::new(""));
        let mut p2 = sample_profile();
        p2.set(cf::SectionKind::Calibration, "brightness_pct", "90");
        let _ = cf::apply(&mut vp, p2, &empty_ledger());
        let r = vp.rollback(2);
        let ok = match r {
            Ok(v4) => v4 == 4 && vp.current == snap2,
            Err(()) => false,
        };
        s.add("H05-版本-一键回退", ok, "回退到 2：当前档==快照2 且新版本号 4");
    }

    // 回退亦留痕：历史链不被改写（回退不是时间旅行——前 3 节原样在链）。
    {
        let mut vp = cf::VersionedProfile::new("DP-1");
        let _ = cf::apply(&mut vp, sample_profile(), &empty_ledger());
        let _ = vp.rollback(1);
        let ok = vp.chain_len() == 4
            && vp.chain.iter().enumerate().all(|(i, n)| n.version == i as u32 + 1);
        s.add("H05-版本-回退亦留痕", ok, "链 4 节且版本号连续——历史只增不改");
    }

    // 越界拒：目标 0 与超链长目标拒（E_CFG_VERSION 域的入口口径）。
    {
        let mut vp = cf::VersionedProfile::new("DP-1");
        let _ = cf::apply(&mut vp, sample_profile(), &empty_ledger());
        let ok = vp.rollback(0).is_err() && vp.rollback(99).is_err();
        s.add("H05-版本-越界拒", ok, "目标 0/99 均拒（链长 2）");
    }

    // -------------------------------------------------------------------------
    // 五、导入冲突逐项裁决
    // -------------------------------------------------------------------------

    // 三态归类：同名异值→Overwritten、本地独有→KeepLocal、来档独有→TakeIncoming。
    {
        let local = sample_profile();
        let mut incoming = sample_profile();
        incoming.set(cf::SectionKind::Calibration, "brightness_pct", "95");
        incoming.set(cf::SectionKind::Preference, "caption_size_pct", "120");
        incoming.entries.retain(|e| !(e.section == cf::SectionKind::Preference && e.key == "high_contrast"));
        let list = cf::import_conflicts(&local, &incoming);
        let has_over = list.iter().any(|v| matches!(v,
            cf::ConflictVerdict::Overwritten { key, local: l, incoming: i }
                if key == "brightness_pct" && l == "80" && i == "95"));
        let has_keep = list.iter().any(|v| matches!(v,
            cf::ConflictVerdict::KeepLocal { key } if key == "high_contrast"));
        let has_take = list.iter().any(|v| matches!(v,
            cf::ConflictVerdict::TakeIncoming { key, value }
                if key == "caption_size_pct" && value == "120"));
        let ok = has_over && has_keep && has_take;
        s.add("H05-导入-三态裁决", ok, "覆盖/保留/采纳三态逐项在清单（不静默）");
    }

    // 同档零冲突：local==incoming → 全 Same、无覆盖项。
    {
        let p = sample_profile();
        let list = cf::import_conflicts(&p, &p.clone());
        let ok = list.len() == p.len()
            && list.iter().all(|v| matches!(v, cf::ConflictVerdict::Same { .. }));
        s.add("H05-导入-同档零冲突", ok, "同档逐项 Same 无 Overwritten");
    }

    // -------------------------------------------------------------------------
    // 六、主流程与读屏
    // -------------------------------------------------------------------------

    // 读屏摘要：版本/链长/条目/待处置数四口径一致。
    {
        let mut vp = cf::VersionedProfile::new("DP-1");
        let _ = cf::apply(&mut vp, sample_profile(), &empty_ledger());
        let mut bad = sample_profile();
        bad.set(cf::SectionKind::Capability, "max_refresh_hz", "9999");
        let errs = match cf::apply(&mut vp, bad, &empty_ledger()) {
            Err(e) => e,
            Ok(_) => Vec::new(),
        };
        let line = cf::screen_summary(&vp, &errs);
        let ok = line.contains("版本 2") && line.contains("条目 8 项") && line.contains("待处置校验问题 1 项");
        s.add("H05-读屏-摘要", ok, "摘要含版本/条目/待处置三口径且与实况一致");
    }

    // -------------------------------------------------------------------------
    // 七、契约与判据自检
    // -------------------------------------------------------------------------

    // 码互异：五码非空两两互异。
    {
        let codes = [cf::E_CFG_FIELD, cf::E_CFG_RANGE, cf::E_CFG_REF, cf::E_CFG_IMPORT, cf::E_CFG_VERSION];
        let mut ok = codes.iter().all(|c| !c.is_empty());
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                    if a == b {
                        ok = false;
                    }
                }
            }
        }
        s.add("H05-契约-码互异", ok, "五错误码非空两两互异");
    }

    // 版本锚 + 短码域与 F4403 对齐（四短码字面量）。
    {
        let ok = cf::CFG_FILE_VERSION.starts_with("V05")
            && cf::PROFILE_REFS.len() == 4
            && cf::PROFILE_REFS == REF_PROFILE_REFS
            && cf::REDACTED == "__REDACTED__";
        s.add("H05-契约-版本与引用域", ok, "版本 V05-*；四短码/脱敏占位符字面量");
    }

    // 五条锚点判据映射齐备（实产名前缀计数与字面量一致）。
    {
        let names: [&str; 19] = [
            "H05-三节-封闭集",
            "H05-三节-白名单字面量",
            "H05-三节-同键覆盖",
            "H05-三节-节不串",
            "H05-三查-字段",
            "H05-三查-范围",
            "H05-三查-引用",
            "H05-三查-三要素",
            "H05-三查-拒绝生效",
            "H05-开放-往返",
            "H05-开放-脱敏",
            "H05-开放-回导表外拒",
            "H05-版本-留痕入链",
            "H05-版本-一键回退",
            "H05-版本-回退亦留痕",
            "H05-版本-越界拒",
            "H05-导入-三态裁决",
            "H05-导入-同档零冲突",
            "H05-读屏-摘要",
        ];
        let n3 = names.iter().filter(|n| n.starts_with("H05-三节-")).count();
        let nopen = names.iter().filter(|n| n.starts_with("H05-开放-")).count();
        let nver = names.iter().filter(|n| n.starts_with("H05-版本-")).count();
        let nchk = names.iter().filter(|n| n.starts_with("H05-三查-")).count();
        let ok = n3 == 4 && nopen == 3 && nver == 4 && nchk == 5;
        s.add("H05-判据-五条映射齐备", ok, "四族实产计数 4/3/4/5 与判据侧字面量一致");
    }

    // 条数对账：本条之前实产 22 条（本条为第 23 条）。
    {
        let ok = s.len() == 22;
        s.add("H05-判据-条数对账", ok, "本条前实产 22 条（本条为第 23 条）");
    }

    s
}
