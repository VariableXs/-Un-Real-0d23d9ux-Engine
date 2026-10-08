//! VE-F4403 自检 · 色彩管理引擎（VE-W 域）
//!
//! **锚点判据逐条对应**（`#VE-F4403`「四模块、按需激活、缺省标注、
//! 意图仲裁、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 四模块 | `C03-解析-*` / `C03-调度-*` / `C03-缓存-*` / `C03-回退-*` |
//! | 按需激活 | `C03-激活-*`（无校准→缺省+标注；有校准→专用 profile） |
//! | 缺省标注 | `C03-激活-*`（标注单可查可读屏；HDR 屏加粗语义） |
//! | 意图仲裁 | `C03-仲裁-*`（优先级表钉死；空请求拒绝） |
//! | 判据 | `C03-判据-*`（闭集/版本/错误码/条数对账） |
//!
//! **判据设计硬规矩**：期望值独立重算；不变量两头都测；缓存 O(1) 用
//! 「槽位号独立重算」对账；版本戳失效双向（bump 前命中 / bump 后未命中）。

use crate::checks::CheckSet;
use crate::svstar2::vev02_monitor::{CapabilityRecord, HdrCapability};
use crate::svstar2::vev03_color as ce;

// ---------------------------------------------------------------------------
// 组一：配置解析（模块一）
// ---------------------------------------------------------------------------

fn chk_parse(s: &mut CheckSet) {
    // C03-解析-01：有效 kind 行解析成功。
    let ok = match ce::parse_config("kind=p3\nopaque junk", Some("mon-1".into())) {
        Ok(p) => p.kind == ce::ProfileKind::DisplayP3 && p.version == 1,
        Err(()) => false,
    };
    s.add("C03-解析-01", ok, "有效校准解析出 P3 且版本戳=1");

    // C03-解析-02：kind 未知拒绝。
    s.add("C03-解析-02", ce::parse_config("kind=rec2020", None).is_err(), "未知 kind 解析失败");

    // C03-解析-03：空配置拒绝。
    s.add("C03-解析-03", ce::parse_config("", None).is_err(), "空配置解析失败");

    // C03-解析-04：非 kind 开头拒绝。
    s.add("C03-解析-04", ce::parse_config("gamma=2.2", None).is_err(), "缺 kind 行解析失败");

    // C03-解析-05：显式 sRGB 声明走缺省（无需差异变换）。
    s.add(
        "C03-解析-05",
        ce::parse_config("kind=srgb", None).is_err(),
        "显式 sRGB 归缺省路径（降级而非激活）",
    );

    // C03-解析-06：四类短码全部可解析（kind 行逐类验证）。
    let mut ok = true;
    for k in ce::ProfileKind::all().iter() {
        if *k == ce::ProfileKind::Srgb {
            continue;
        }
        let line = alloc::format!("kind={}", k.wire());
        ok = ok && matches!(ce::parse_config(&line, None), Ok(ref p) if p.kind == *k);
    }
    s.add("C03-解析-06", ok, "三类激活 profile 逐类解析成功（srgb 归缺省已单测）");
}

// ---------------------------------------------------------------------------
// 组二：变换调度（模块二，O(1) 查表）
// ---------------------------------------------------------------------------

fn chk_schedule(s: &mut CheckSet) {
    use ce::ProfileKind as K;
    // C03-调度-01：同型直通。
    let p = ce::schedule(K::DisplayP3, K::DisplayP3, ce::RenderIntent::Perceptual);
    s.add("C03-调度-01", p.passthrough, "同 profile 直通");

    // C03-调度-02：双方均为 sRGB 直通。
    let p = ce::schedule(K::Srgb, K::Srgb, ce::RenderIntent::Colorimetric);
    s.add("C03-调度-02", p.passthrough, "sRGB↔sRGB 直通");

    // C03-调度-03：异型非直通且意图透传。
    let p = ce::schedule(K::AdobeRgb, K::DisplayP3, ce::RenderIntent::Saturation);
    s.add(
        "C03-调度-03",
        !p.passthrough && p.from == K::AdobeRgb && p.to == K::DisplayP3
            && p.intent == ce::RenderIntent::Saturation,
        "异型调度保留端点与意图",
    );

    // C03-调度-04：sRGB→P3 非直通（有差异变换）。
    let p = ce::schedule(K::Srgb, K::DisplayP3, ce::RenderIntent::Perceptual);
    s.add("C03-调度-04", !p.passthrough, "sRGB→P3 有变换");

    // C03-调度-05：profile 短码往返（四类）。
    let ok = ce::ProfileKind::all()
        .iter()
        .all(|k| ce::ProfileKind::parse(k.wire()) == Some(*k));
    s.add("C03-调度-05", ok, "profile 短码往返一致");

    // C03-调度-06：profile 短码互异。
    let all = ce::ProfileKind::all();
    let mut ok = true;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i].wire() == all[j].wire() {
                ok = false;
            }
        }
    }
    s.add("C03-调度-06", ok, "profile 短码两两互异");
}

// ---------------------------------------------------------------------------
// 组三：变换缓存（模块三，O(1) + 版本戳）
// ---------------------------------------------------------------------------

fn chk_cache(s: &mut CheckSet) {
    // C03-缓存-01：插入后 O(1) 命中。
    let mut c = ce::TransformCache::new();
    let k = (7u64, 9u64, 0u8);
    c.insert(k);
    s.add("C03-缓存-01", c.lookup(k), "插入即命中");

    // C03-缓存-02：版本戳 bump 后未命中（失效）。
    c.bump_version();
    s.add("C03-缓存-02", !c.lookup(k), "版本戳递增后旧条目失效");

    // C03-缓存-03：失效计数记账（bump 一次记 1）。
    let mut c2 = ce::TransformCache::new();
    c2.insert((1, 2, 3));
    c2.insert((3, 4, 5));
    c2.bump_version();
    s.add("C03-缓存-03", c2.invalidations == 2, "版本失效按失效条数记账");

    // C03-缓存-04：重插新版本后命中。
    c2.insert((1, 2, 3));
    s.add("C03-缓存-04", c2.lookup((1, 2, 3)), "新版本重插后命中");

    // C03-缓存-05：不同键不串槽误命中。
    let mut c3 = ce::TransformCache::new();
    c3.insert((1, 1, 0));
    s.add("C03-缓存-05", !c3.lookup((2, 2, 1)), "异键不误命中");

    // C03-缓存-06：命中计数递增（性能可见）。
    let mut c4 = ce::TransformCache::new();
    c4.insert((5, 5, 2));
    let _ = c4.lookup((5, 5, 2));
    let _ = c4.lookup((5, 5, 2));
    let slot = ce::TransformCache::slot_of_for_test((5, 5, 2));
    let hits = match c4.slot_entry(slot) {
        Some(e) => e.hits == 2,
        None => false,
    };
    s.add("C03-缓存-06", hits, "两次命中计数=2（性能可见性）");

    // C03-缓存-07：槽位号判据侧独立重算对账（FNV 初值 2166136261、乘子
    // 16777619、模 64 三常量在判据侧逐字重写——被测定槽改了这里必红）。
    let mut ok = true;
    for (a, b, c3) in [(1u64, 2u64, 0u8), (0, 0, 0), (u64::MAX, 7, 255), (0xDEAD_BEEF, 0x1234, 42)] {
        let mut h: u64 = 2166136261u64 ^ a;
        h = h.wrapping_mul(16777619u64) ^ b;
        h = h.wrapping_mul(16777619u64) ^ (c3 as u64);
        let expect = (h % 64u64) as usize;
        let got = ce::TransformCache::slot_of_for_test((a, b, c3));
        ok = ok && got == expect && got < ce::CACHE_CAPACITY;
    }
    s.add(
        "C03-缓存-07",
        ok,
        "定槽函数与判据侧独立 FNV 五键对账且界内",
    );

    // C03-缓存-08：版本号 bump 返回递增值。
    let mut c5 = ce::TransformCache::new();
    let v0 = c5.version;
    let v1 = c5.bump_version();
    s.add("C03-缓存-08", v1 == v0 + 1, "版本戳严格递增");
}

// ---------------------------------------------------------------------------
// 组四：异常回退 + 按需激活 + 缺省标注（模块四 + 总成）
// ---------------------------------------------------------------------------

fn chk_activation(s: &mut CheckSet) {
    use ce::ColorActivation as CA;
    // C03-激活-01：无校准 → 缺省降级 + 标注在案。
    let mut t = ce::ActivationTable::new();
    let act = t.activate("mon-a", None);
    let ok = matches!(act, CA::DefaultFallback { reason } if reason == ce::E_COLOR_UNCALIBRATED)
        && t.notes.len() == 1
        && t.notes.first().map_or(false, |n| n.reason == ce::E_COLOR_UNCALIBRATED);
    s.add("C03-激活-01", ok, "无校准走缺省且标注不静默");

    // C03-激活-02：解析失败 → 缺省降级 + 原因=CONFIG。
    let mut t = ce::ActivationTable::new();
    let act = t.activate("mon-a", Some("bogus"));
    let ok = matches!(act, CA::DefaultFallback { reason } if reason == ce::E_COLOR_CONFIG)
        && t.notes.first().map_or(false, |n| n.reason == ce::E_COLOR_CONFIG);
    s.add("C03-激活-02", ok, "解析失败缺省+原因可查（与无校准可区分）");

    // C03-激活-03：有校准 → 激活专用 profile。
    let mut t = ce::ActivationTable::new();
    let act = t.activate("mon-a", Some("kind=p3"));
    let ok = matches!(act, CA::Active { kind: ce::ProfileKind::DisplayP3, version: 1 });
    s.add("C03-激活-03", ok, "校准成功激活专用 profile（合规放行侧）");

    // C03-激活-04：重复激活同显示器覆盖不重复（台账幂等）。
    let mut t = ce::ActivationTable::new();
    let _ = t.activate("mon-a", Some("kind=p3"));
    let _ = t.activate("mon-a", None);
    let ok = t.len() == 1
        && t.get("mon-a").map_or(false, |a| matches!(a, CA::DefaultFallback { .. }));
    s.add("C03-激活-04", ok, "同显示器重激活覆盖台账（不重复记账）");

    // C03-激活-05：缺省标注读屏可读（三要素：显示器/路径/原因）。
    let mut t = ce::ActivationTable::new();
    let _ = t.activate("mon-hdr", None);
    let line = match t.notes.first() {
        Some(n) => n.screen_line(),
        None => alloc::string::String::new(),
    };
    s.add(
        "C03-激活-05",
        line.contains("mon-hdr") && line.contains("sRGB") && line.contains("校准"),
        "缺省标注读屏三要素齐备",
    );

    // C03-激活-06：HDR 屏缺省标注加粗（观感损失提示）。
    let mut t = ce::ActivationTable::new();
    let cap = CapabilityRecord::new(alloc::vec::Vec::new(), HdrCapability::Present);
    let _ = t.activate_from_capability("mon-hdr", &cap, None);
    s.add(
        "C03-激活-06",
        t.notes.first().map_or(false, |n| n.detail.contains("HDR")),
        "HDR 屏未校准标注加粗（观感损失显性）",
    );

    // C03-激活-07：SDR 屏无校准不加粗。
    let mut t = ce::ActivationTable::new();
    let cap = CapabilityRecord::new(alloc::vec::Vec::new(), HdrCapability::NotDetected);
    let _ = t.activate_from_capability("mon-sdr", &cap, None);
    s.add("C03-激活-07", !t.notes.first().map_or(false, |n| n.detail.contains("HDR")), "SDR 屏标注不加粗（诚实口径）");

    // C03-激活-08：激活表读屏摘要三计数一致。
    let mut t = ce::ActivationTable::new();
    let _ = t.activate("a", Some("kind=adobe"));
    let _ = t.activate("b", None);
    let _ = t.activate("c", None);
    let line = t.screen_summary();
    s.add(
        "C03-激活-08",
        t.len() == 3 && t.fallback_count() == 2 && line.contains("3") && line.contains("2"),
        "摘要计数与台账一致（3 在册 2 缺省）",
    );

    // C03-回退-01：回退原因归类三码齐备。
    let a = ce::fallback_note("x", ce::E_COLOR_UNCALIBRATED);
    let b = ce::fallback_note("x", ce::E_COLOR_CONFIG);
    let c = ce::fallback_note("x", ce::E_COLOR_TRANSFORM);
    s.add(
        "C03-回退-01",
        a.detail.contains("无校准") && b.detail.contains("解析失败") && c.detail.contains("回退"),
        "三类降级原因人话各表",
    );

    // C03-回退-02：未知原因有兜底文案（不产生空标注）。
    let d = ce::fallback_note("x", "E_OTHER");
    s.add("C03-回退-02", !d.detail.is_empty(), "未知降级原因兜底文案非空");
}

// ---------------------------------------------------------------------------
// 组五：意图仲裁 / 判据自检
// ---------------------------------------------------------------------------

fn chk_arbitrate_meta(s: &mut CheckSet) {
    use ce::RenderIntent as I;
    // C03-仲裁-01：色度精确最高优先（与饱和度并存取色度）。
    let r = ce::arbitrate_intent(&[I::Saturation, I::Colorimetric]);
    s.add("C03-仲裁-01", r == Some(I::Colorimetric), "冲突取最高优先（色度>饱和度）");

    // C03-仲裁-02：无色度时感知优先于饱和度。
    let r = ce::arbitrate_intent(&[I::Saturation, I::Perceptual]);
    s.add("C03-仲裁-02", r == Some(I::Perceptual), "感知优先于饱和度");

    // C03-仲裁-03：单一意图直接生效。
    s.add("C03-仲裁-03", ce::arbitrate_intent(&[I::Saturation]) == Some(I::Saturation), "单意图直通");

    // C03-仲裁-04：空请求拒绝（Nothing to arbitrate——不静默造默认）。
    s.add("C03-仲裁-04", ce::arbitrate_intent(&[]).is_none(), "空请求集返回 None");

    // C03-仲裁-05：优先级表钉死顺序（色度/感知/饱和度）。
    s.add(
        "C03-仲裁-05",
        ce::INTENT_PRIORITY[0] == I::Colorimetric
            && ce::INTENT_PRIORITY[1] == I::Perceptual
            && ce::INTENT_PRIORITY[2] == I::Saturation,
        "意图优先级表钉死（改序必红）",
    );

    // C03-判据-01：profile 闭集 4。
    s.add(
        "C03-判据-01",
        ce::PROFILE_KIND_COUNT == 4 && ce::ProfileKind::all().len() == 4,
        "profile 四类闭集",
    );

    // C03-判据-02：引擎版本前缀。
    s.add(
        "C03-判据-02",
        ce::COLOR_ENGINE_VERSION.starts_with("V03"),
        "版本 V03-*（跨版本对账锚）",
    );

    // C03-判据-03：错误码非空互异。
    s.add(
        "C03-判据-03",
        !ce::E_COLOR_CONFIG.is_empty()
            && !ce::E_COLOR_UNCALIBRATED.is_empty()
            && !ce::E_COLOR_TRANSFORM.is_empty()
            && !ce::E_COLOR_SCHEDULE.is_empty()
            && ce::E_COLOR_CONFIG != ce::E_COLOR_UNCALIBRATED
            && ce::E_COLOR_TRANSFORM != ce::E_COLOR_SCHEDULE,
        "错误码非空互异（外部可观测分支）",
    );

    // C03-判据-04：缓存容量 64（O(1) 前提的定容口径）。
    s.add("C03-判据-04", ce::CACHE_CAPACITY == 64, "缓存定容 64 钉死");

    // C03-判据-05：判据条数对账（本条前已有 39 条）。
    s.add("C03-判据-05", s.len() == 39, "判据条数对账（本条为第 40 条）");
}

// ---------------------------------------------------------------------------
// 聚合（单集 40 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// F4403 域自检（聚合入口，注册表用）。
pub fn run_vev03_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4403");
    chk_parse(&mut s);
    chk_schedule(&mut s);
    chk_cache(&mut s);
    chk_activation(&mut s);
    chk_arbitrate_meta(&mut s);
    s
}
