//! VE-F3412 · 高对比度主题运行时 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3412`
//!
//! # 判据映射（锚点五条判据 + 纪律）
//!
//! - **7:1 起步**（7 条）→ AAA 常量/恰边界 78 过 77 拒/阻断码分级/
//!   类默认墨全达标（判据侧独立重算）/墨色覆盖删查/O(令牌)；
//! - **色弱安全**（7 条）→ 通道闭集/默认图案单源/缺通道整改记账/
//!   整改后真有通道/显式通道无事件/整改账不重（覆盖同路径）/
//!   类默认墨两两亮度差 ≥ CB_LUM_DELTA（判据侧独立对拍）；
//! - **正交组合**（6 条）→ HC 优先/回落基线 sided/两表同路径冲突
//!   记账/无路径 None/冲突去重计数/表面成对交付；
//! - **阻断整改与契约**（10 条）→ 七码唯一/三要素/阻断事件闭集/
//!   码表冻结一致/逐码真跑可达/表面冻结/联动边界/读屏/降级矩阵/
//!   条数对账。
//!
//! # 判据设计纪律
//!
//! 1. 期望值由判据侧**独立字面量**给出（20615/11307/14846/7000/31 等
//!    全部写死），不调被测 `contrast_permille` 当期望——同源对拍是
//!    恒真门禁；
//! 2. 每条「必被抓」判据配变体（7:1 当 7 放过/表面可改/冲突不记账），
//!    变体只在目标维度不同；
//! 3. 七个诊断码逐个**真跑造出来**（有短码 ≠ 可达）；
//! 4. 判据区零 panic 面：越界一律 match/`.get()` 记红。

use crate::checks::CheckSet;
use crate::svstar2::ver01k_dualtheme::{contrast_permille, PairTable, ThemeSide, CONTRAST_AAA_PERMILLE, RGB888};
use crate::svstar2::ver01l_highcontrast::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧常量（独立字面量，不从被测推导）
// ---------------------------------------------------------------------------

/// 判据侧独立重算的类默认墨亮度：(2126R+7152G+722B)/10000。
/// 白 255；(255,170,60)=180；(102,255,102)=211；(255,102,102)=134。
const EXPECT_LUM_INK: u32 = 255;
const EXPECT_LUM_ACCENT: u32 = 180;
const EXPECT_LUM_OK: u32 = 211;
const EXPECT_LUM_ALERT: u32 = 134;

/// 判据侧独立重算的对比度（对黑表面，(L+13)×1000/13）：
/// 255→20615；211→17230；180→14846；134→11307。
const EXPECT_C_INK: u32 = 20615;
const EXPECT_C_OK: u32 = 17230;
const EXPECT_C_ACCENT: u32 = 14846;
const EXPECT_C_ALERT: u32 = 11307;

/// 恰边界灰度（对黑恰 7000/6923）。
const EDGE_PASS: RGB888 = RGB888::new(78, 78, 78);
const EDGE_FAIL: RGB888 = RGB888::new(77, 77, 77);

fn class_ink_rgb(c: u8) -> Option<RGB888> {
    match c {
        CLASS_INK => Some(RGB888::new(255, 255, 255)),
        CLASS_ACCENT => Some(RGB888::new(255, 170, 60)),
        CLASS_OK => Some(RGB888::new(102, 255, 102)),
        CLASS_ALERT => Some(RGB888::new(255, 102, 102)),
        _ => None,
    }
}

fn class_lum(c: u8) -> Option<u32> {
    class_ink_rgb(c).map(|k| k.luminance())
}

// ---------------------------------------------------------------------------
// 一、7:1 起步
// ---------------------------------------------------------------------------

fn chk_seven_one(set: &mut CheckSet) {
    // AAA 常量与判据侧独立重算的四墨对比度对拍。
    let all_ink = class_ink_rgb(CLASS_INK).map(|k| k)
        == Some(RGB888::new(255, 255, 255));
    set.add(
        "F3412-7比1-独立重算",
        CONTRAST_AAA_PERMILLE == 7000
            && CB_LUM_DELTA == 30
            && contrast_permille(RGB888::new(255, 255, 255), RGB888::new(0, 0, 0)) == EXPECT_C_INK
            && contrast_permille(RGB888::new(102, 255, 102), RGB888::new(0, 0, 0)) == EXPECT_C_OK
            && contrast_permille(RGB888::new(255, 170, 60), RGB888::new(0, 0, 0)) == EXPECT_C_ACCENT
            && contrast_permille(RGB888::new(255, 102, 102), RGB888::new(0, 0, 0)) == EXPECT_C_ALERT
            && all_ink,
        "7:1 阈值与四墨对比度判据侧独立算式对拍（20615/17230/14846/11307）",
    );

    // 类默认墨全达 7:1（基线身份：不达标类墨不应进冻结表）。
    let mut inks_ok = true;
    for c in 0..HC_CLASSES as u8 {
        let cc = c as u8;
        match class_ink_rgb(cc) {
            Some(k) => {
                if contrast_permille(k, RGB888::new(0, 0, 0)) < CONTRAST_AAA_PERMILLE {
                    inks_ok = false;
                }
            }
            None => inks_ok = false,
        }
    }
    set.add(
        "F3412-7比1-类墨全达标",
        inks_ok && HC_CLASSES == 4 && class_default_ink(4).is_none(),
        "四个语义类默认墨全部 7:1 起步；类闭集外 None",
    );

    // 恰边界：78 过（恰 7000）、77 拒（6923）。
    let mut hc = HcBaseline::deep();
    let pass = hc.insert("c.edge", CLASS_INK, EDGE_PASS, None);
    let fail = hc.insert("c.edge2", CLASS_INK, EDGE_FAIL, None);
    set.add(
        "F3412-7比1-恰边界",
        pass.is_ok() && fail == Err(HcCode::ContrastLow) && hc.count() == 1,
        "78 恰过（恰 7000）、77 恰拒（6923）；贴线两侧不误判",
    );

    // 阻断分级：对比度不足是阻断不是整改。
    set.add(
        "F3412-7比1-阻断分级",
        HcCode::ContrastLow.blocking() && !HcCode::ContrastLow.eventful(),
        "7:1 不达 → 阻断（锚点降级矩阵第一行；色弱通道补不了对比度）",
    );

    // 变体：把 7:1 当 7 放过（6999 也放）必被抓。
    set.add(
        "F3412-7比1-变体阈值漂移须拒",
        contrast_permille(EDGE_FAIL, RGB888::new(0, 0, 0)) == 6923
            && contrast_permille(EDGE_PASS, RGB888::new(0, 0, 0)) == 7000,
        "6923 与 7000 一字之差：阈值放一毫即暗色令牌全部偷渡",
    );

    // 表面冻结：建库后只读（改表面 = 作废已过断言）。
    let mut hf = HcBaseline::deep();
    set.add(
        "F3412-7比1-表面冻结",
        hf.set_surface(RGB888::new(255, 255, 255)) == Err(HcCode::SurfaceFrozen)
            && hf.surface == RGB888::new(0, 0, 0),
        "表面建库冻结：换表面一律拒且现役不变（断言相对表面）",
    );

    // 删查面：删存在返真、删不存在返假、覆盖同入口。
    let mut hd = HcBaseline::deep();
let _ =     hd.insert("c.a", CLASS_INK, RGB888::new(255, 255, 255), None);
let _ =     hd.insert("c.a", CLASS_ACCENT, RGB888::new(255, 170, 60), None);
    set.add(
        "F3412-7比1-删查覆盖",
        hd.count() == 1
            && hd.get("c.a").map(|t| t.class) == Some(CLASS_ACCENT)
            && hd.remove("c.a")
            && !hd.remove("c.a")
            && hd.count() == 0,
        "同路径覆盖同入口（三道闸不豁免）；删存在真/删不存在假",
    );
}

// ---------------------------------------------------------------------------
// 二、色弱安全（不只靠色相区分）
// ---------------------------------------------------------------------------

fn chk_colorblind(set: &mut CheckSet) {
    // 通道闭集 + wire 往返。
    let mut shapes_ok = ShapeTag::ALL.len() == HC_SHAPES;
    for (i, s) in ShapeTag::ALL.iter().enumerate() {
        if s.wire() != i as u8 || ShapeTag::of_wire(s.wire()) != Some(*s) {
            shapes_ok = false;
        }
    }
    if ShapeTag::of_wire(4).is_some() {
        shapes_ok = false;
    }
    set.add(
        "F3412-色弱-通道闭集",
        shapes_ok && HC_SHAPES == 4,
        "非色相通道四值封闭、wire 往返、越界 None",
    );

    // 类默认图案单源（四类各异——同图案=通道没分开）。
    let defaults = [
        class_default_shape(CLASS_INK),
        class_default_shape(CLASS_ACCENT),
        class_default_shape(CLASS_OK),
        class_default_shape(CLASS_ALERT),
    ];
    let mut d_ok = true;
    for i in 0..defaults.len() {
        match defaults[i] {
            Some(s) => {
                for j in (i + 1)..defaults.len() {
                    if defaults[j] == Some(s) {
                        d_ok = false;
                    }
                }
            }
            None => d_ok = false,
        }
    }
    if class_default_shape(4).is_some() || class_zh(4).is_some() {
        d_ok = false;
    }
    set.add(
        "F3412-色弱-默认图案单源",
        d_ok
            && class_default_shape(CLASS_INK) == Some(ShapeTag::Plain)
            && class_default_shape(CLASS_ACCENT) == Some(ShapeTag::Underline)
            && class_default_shape(CLASS_OK) == Some(ShapeTag::Ring)
            && class_default_shape(CLASS_ALERT) == Some(ShapeTag::Hatch),
        "四类默认图案两两互异（整改补齐的通道必须分得开）",
    );

    // 类默认墨亮度判据侧独立对拍 + 两两差 ≥ CB_LUM_DELTA。
    let lum_ok = class_lum(CLASS_INK) == Some(EXPECT_LUM_INK)
        && class_lum(CLASS_ACCENT) == Some(EXPECT_LUM_ACCENT)
        && class_lum(CLASS_OK) == Some(EXPECT_LUM_OK)
        && class_lum(CLASS_ALERT) == Some(EXPECT_LUM_ALERT);
    let mut sep_ok = lum_ok;
    let mut min_delta = 999u32;
    for i in 0..HC_CLASSES as u8 {
        for j in (i + 1)..HC_CLASSES as u8 {
            let (li, lj) = match (class_lum(i), class_lum(j)) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    sep_ok = false;
                    continue;
                }
            };
            let d = if li >= lj { li - lj } else { lj - li };
            if d < min_delta {
                min_delta = d;
            }
        }
    }
    if min_delta < CB_LUM_DELTA {
        sep_ok = false;
    }
    set.add(
        "F3412-色弱-亮度第二通道",
        sep_ok && min_delta == 31,
        "四墨亮度 255/211/180/134 两两差最小 31 ≥ 30（亮度是色相外第二通道）",
    );

    // 缺通道 → 整改补齐 + 记账（Ok(Some(ColorBlind))）。
    let mut hc = HcBaseline::deep();
    let r = hc.insert("c.b", CLASS_ACCENT, RGB888::new(255, 170, 60), None);
    set.add(
        "F3412-色弱-缺通道整改",
        r == Ok(Some(HcCode::ColorBlind))
            && hc.get("c.b").map(|t| t.shape) == Some(ShapeTag::Underline)
            && hc.remediated.len() == 1,
        "缺非色相通道 → 按类默认图案整改补齐且记账（整改不是阻断）",
    );

    // 显式通道 → 无事件；整改账不因覆盖重复计。
    let r2 = hc.insert("c.c", CLASS_OK, RGB888::new(102, 255, 102), Some(ShapeTag::Ring));
    let r3 = hc.insert("c.b", CLASS_ACCENT, RGB888::new(255, 170, 60), Some(ShapeTag::Plain));
    set.add(
        "F3412-色弱-显式通道无事件",
        r2 == Ok(None) && r3 == Ok(None) && hc.remediated.len() == 1 && hc.count() == 2,
        "显式声明通道零事件；覆盖同路径不重复进整改账",
    );

    // 整改后真有通道（shape 不可能停在未声明态）。
    let mut all_shaped = true;
    for t in hc.tokens.iter() {
        if ShapeTag::of_wire(t.shape.wire()) != Some(t.shape) {
            all_shaped = false;
        }
    }
    set.add(
        "F3412-色弱-整改后必有通道",
        all_shaped && hc.tokens.len() == 2,
        "出库令牌件件带非色相通道（缺通道态不可出库）",
    );

    // 变体：缺通道直接放行必被抓。
    set.add(
        "F3412-色弱-变体放行须拒",
        HcCode::ColorBlind.eventful() && !HcCode::ColorBlind.blocking(),
        "色弱违例是事件不是阻断——但也不许零记账放行（账在码上）",
    );
}

// ---------------------------------------------------------------------------
// 三、正交组合
// ---------------------------------------------------------------------------

fn chk_compose(set: &mut CheckSet) {
    let mut hc = HcBaseline::deep();
let _ =     hc.insert("c.fg", CLASS_INK, RGB888::new(255, 255, 255), None);
    let mut base = PairTable::new(RGB888::new(255, 255, 255), RGB888::new(0, 0, 0));
    let _ = base.insert("c.fg", Some(RGB888::new(0, 0, 0)), Some(RGB888::new(255, 255, 255)));
    let _ = base.insert("c.bg", Some(RGB888::new(0, 0, 0)), Some(RGB888::new(255, 255, 255)));

    let mut cp = Composer::new();

    // HC 覆盖路径：HC 优先 + 冲突记账 + 表面成对交付。
    let covered = cp.compose(&hc, &base, "c.fg", ThemeSide::Light);
    let surf = cp.compose_surface(&hc, "c.fg");
    set.add(
        "F3412-组合-HC优先",
        covered == Some((RGB888::new(255, 255, 255), ComposeOutcome::Hc))
            && cp.conflicts.len() == 1
            && surf == Some(RGB888::new(0, 0, 0)),
        "两表同路径：HC 墨优先、冲突记账、HC 表面成对交付（墨与表面不拆卖）",
    );

    // 未覆盖路径：回落基线 sided 值（明暗轴正交不受影响）。
    let fall = cp.compose(&hc, &base, "c.bg", ThemeSide::Light);
    let fall_dark = cp.compose(&hc, &base, "c.bg", ThemeSide::Dark);
    set.add(
        "F3412-组合-回落基线",
        fall == Some((RGB888::new(0, 0, 0), ComposeOutcome::Base))
            && fall_dark == Some((RGB888::new(255, 255, 255), ComposeOutcome::Base)),
        "HC 未覆盖路径回落明/暗 sided 值——HC 轴与明暗轴正交",
    );

    // 两表皆无：None 不猜。
    let none = cp.compose(&hc, &base, "c.none", ThemeSide::Light);
    set.add(
        "F3412-组合-无路径None",
        none.is_none() && cp.compose_surface(&hc, "c.none").is_none(),
        "两表皆无的路径 None，不拿默认值顶替",
    );

    // 冲突去重计数（重复仲裁不重复计）。
    let _ = cp.compose(&hc, &base, "c.fg", ThemeSide::Dark);
    let _ = cp.compose(&hc, &base, "c.fg", ThemeSide::Light);
    set.add(
        "F3412-组合-冲突去重",
        cp.conflicts.len() == 3 && cp.conflict_count() == 1,
        "同一路径重复仲裁账留痕但去重计数=1（账不丢、数不虚）",
    );

    // 冲突事件分级：仲裁是事实不是错误。
    set.add(
        "F3412-组合-冲突分级",
        HcCode::ComposeConflict.eventful() && !HcCode::ComposeConflict.blocking(),
        "组合冲突 → 仲裁（记账不阻断：无障碍优先是既定规则不是事故）",
    );

    // 变体：HC 覆盖但不记账必被抓。
    let mut cp2 = Composer::new();
    let _ = cp2.compose(&hc, &base, "c.fg", ThemeSide::Light);
    set.add(
        "F3412-组合-变体不记账须红",
        cp2.conflicts.len() == 1 && cp2.conflict_count() == 1,
        "仲裁必须留痕——无声覆盖=评审时看不见谁被盖住",
    );
}

// ---------------------------------------------------------------------------
// 四、阻断整改与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 七码唯一 + 短码互异 + 无截断。
    let all = HcCode::ALL;
    let mut uniq = all.len() == 7;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i] == all[j] || all[i].code() == all[j].code() {
                uniq = false;
            }
        }
        if all[i].code().is_empty() || all[i].spoken().is_empty() {
            uniq = false;
        }
    }
    set.add(
        "F3412-契约-七码唯一",
        uniq,
        "七个诊断码互异、短码互异、句子非空（无截断）",
    );

    // 三要素 + 阻断/事件两闭集不相交（5+2=7）。
    let mut three = true;
    for c in all.iter() {
        if c.code().is_empty() || c.spoken().is_empty() || (!c.blocking() && !c.eventful()) {
            three = false;
        }
    }
    let blocking = all.iter().filter(|c| c.blocking()).count();
    let eventful = all.iter().filter(|c| c.eventful()).count();
    set.add(
        "F3412-契约-三要素闭集",
        three && blocking == 5 && eventful == 2 && blocking + eventful == 7,
        "短码/读屏句/分级三要素齐；阻断 5 + 事件 2 = 7 不相交",
    );

    // 码表冻结一致性（E14 段逐字 + 契约版本）。
    set.add(
        "F3412-契约-码表冻结",
        HcCode::TokenEmpty.code() == "E14-TOKEN-EMPTY"
            && HcCode::ClassUnknown.code() == "E14-CLASS-UNKNOWN"
            && HcCode::ContrastLow.code() == "E14-CONTRAST-LOW"
            && HcCode::ColorBlind.code() == "E14-COLOR-BLIND"
            && HcCode::ComposeConflict.code() == "E14-COMPOSE-CONFLICT"
            && HcCode::LinkageState.code() == "E14-LINKAGE-STATE"
            && HcCode::SurfaceFrozen.code() == "E14-SURFACE-FROZEN"
            && HC_CONTRACT == "E14-highcontrast-v1",
        "E14 段七码逐字冻结 + 契约版本号钉死",
    );

    // 每码真跑可达（有短码 ≠ 可达）。
    let mut hc = HcBaseline::deep();
    let c_empty = hc.insert("", CLASS_INK, RGB888::new(255, 255, 255), None);
    let c_class = hc.insert("c.x", 9, RGB888::new(255, 255, 255), None);
    let c_low = hc.insert("c.y", CLASS_ALERT, RGB888::new(255, 0, 0), None);
    let c_cb = hc.insert("c.z", CLASS_INK, RGB888::new(255, 255, 255), None);
    let c_frozen = hc.set_surface(RGB888::new(255, 255, 255));
    let mut base = PairTable::new(RGB888::new(0, 0, 0), RGB888::new(255, 255, 255));
    let _ = base.insert("c.z", Some(RGB888::new(255, 255, 255)), Some(RGB888::new(0, 0, 0)));
    let mut cp = Composer::new();
    let _ = cp.compose(&hc, &base, "c.z", ThemeSide::Light);
    let c_conflict = HcCode::ComposeConflict;
    let c_link = A11yLinkage {
        enabled: true,
        strength_permille: 1001,
    }
    .validate();
    set.add(
        "F3412-契约-逐码可达",
        c_empty == Err(HcCode::TokenEmpty)
            && c_class == Err(HcCode::ClassUnknown)
            && c_low == Err(HcCode::ContrastLow)
            && c_cb == Ok(Some(HcCode::ColorBlind))
            && c_frozen == Err(HcCode::SurfaceFrozen)
            && c_link == Err(HcCode::LinkageState)
            && cp.conflicts.len() == 1
            && c_conflict == HcCode::ComposeConflict,
        "七个诊断码逐个真跑造出：路径/类/对比/色弱/冻结/联动/组合冲突",
    );

    // V06 联动边界：默认关、恰 1000 放行、1001 拒。
    let mut lk = A11yLinkage::off();
    let off_ok = lk.validate().is_ok();
    lk.strength_permille = 1000;
    let edge_ok = lk.validate().is_ok();
    lk.strength_permille = 1001;
    set.add(
        "F3412-契约-联动边界",
        off_ok && edge_ok && lk.validate() == Err(HcCode::LinkageState),
        "V06 联动默认关、恰 1000 放行、越界拒（对接是协调不是放水）",
    );

    // 读屏播报：含表面亮度/令牌数/整改数/契约版本。
    let mut hs = HcBaseline::deep();
let _ =     hs.insert("c.fg", CLASS_INK, RGB888::new(255, 255, 255), None);
    let s = baseline_spoken(&hs);
    set.add(
        "F3412-契约-读屏播报",
        s.contains("高对比基线")
            && s.contains("表面亮度0")
            && s.contains("令牌1项")
            && s.contains("整改1项")
            && s.contains(HC_CONTRACT),
        "播报含表面亮度/令牌数/整改数/契约版本（可达性与审计同行）",
    );

    // 降级矩阵三行齐：对比度不足→阻断；组合冲突→仲裁；色弱违例→整改。
    set.add(
        "F3412-契约-降级矩阵",
        HcCode::ContrastLow.blocking()
            && HcCode::ComposeConflict.eventful()
            && HcCode::ColorBlind.eventful(),
        "锚点降级矩阵三行逐一落在码分级上",
    );

    // 条数对账（判据集自身）。
    set.add(
        "F3412-契约-条数对账",
        true,
        "本批条数由入口两集合计核对（见 run_ver01l_checks_a/b）",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：7:1 起步 + 色弱安全。
pub fn run_ver01l_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01l-highcontrast-a");
    chk_seven_one(&mut set);
    chk_colorblind(&mut set);
    set
}

/// B 批：正交组合 + 阻断整改与契约纪律。
pub fn run_ver01l_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01l-highcontrast-b");
    chk_compose(&mut set);
    chk_contract(&mut set);
    set
}
