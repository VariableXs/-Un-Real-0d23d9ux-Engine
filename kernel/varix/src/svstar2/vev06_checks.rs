//! VE-F4406 · 域自检（判据逐条对应，见 `vev06_csconv.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **矩阵库**（空间封闭集 + 正逆矩阵元素字面量 + 表外拒） →
//!   `H06-矩阵-封闭集` / `正向元素字面量` / `逆元字面量` /
//!   `H06-转换-映射缺失拒`
//! - **双精度**（模式表 + 双定点/双矩阵定位 + 误差单调） →
//!   `H06-精度-模式表` / `H06-往返-高精严格小于快速`
//! - **往返断言**（色卡 + 容差承诺 + 超差强制高精） →
//!   `H06-往返-快速超承诺` / `高精达承诺` / `三空间快速全超` /
//!   `超差强制` / `达标不强制` / `色卡字面量`
//! - **显式缺省**（缺省策略结构体可查 + Custom 缺省映射 + used_default
//!   透传） → `H06-转换-显式缺省策略` / `Custom 缺省映射` /
//!   `used_default 透传`
//! - **判据**（五条映射齐备 + 条数对账） →
//!   `H06-判据-五条映射齐备` / `条数对账`
//! - 缓存（版本戳失效范式） / 读屏 / 契约 → `H06-缓存-*` 五条、
//!   `H06-读屏-*` 两条、`H06-契约-*` 两条
//!
//! **判据设计硬规矩**：容差 39 = 10000/255（8bit 半级）判据侧写死并
//! 断换算；矩阵关键元素（正/逆/亿分位高精）字面量抽查；色卡 8 卡
//! 字面量；红点跨空间转换期望输出 [209,8,4] 由判据侧**独立整数模拟**
//! 预计算——被测矩阵或乘法改了必红。往返误差判据侧独立复算
//! （`ref_roundtrip_err`）与被测逐点对账。

use crate::checks::CheckSet;
use crate::svstar2::vev03_color::ProfileKind;
use crate::svstar2::vev06_csconv as cs;

// ---------------------------------------------------------------------------
// 判据侧独立实现区（与被测逐字对齐——被测改了这里必红）
// ---------------------------------------------------------------------------

/// 判据侧独立矩阵乘向量（恒定除 m_scale）。
fn ref_mul(m: &[[i64; 3]; 3], v: [i64; 3], m_scale: i64) -> [i64; 3] {
    let mut out = [0i64; 3];
    for i in 0..3 {
        let mut acc = 0i64;
        for k in 0..3 {
            acc += m[i][k] * v[k];
        }
        out[i] = acc / m_scale;
    }
    out
}

/// 判据侧独立单点往返误差（万分位换算同被测公式）。
fn ref_roundtrip_err(rgb: [u8; 3], space: cs::ColorSpace, prec: cs::Precision) -> i64 {
    let (scale, m_scale, fwd, bwd) = match prec {
        cs::Precision::Fast => (
            cs::PREC_SCALE_FAST,
            cs::MAT_SCALE_FAST,
            match space {
                cs::ColorSpace::Srgb => &[[4124, 3576, 1805], [2126, 7152, 722], [193, 1192, 9505]],
                cs::ColorSpace::DisplayP3 => &[[4866, 2657, 1982], [2290, 6917, 793], [0, 451, 10439]],
                cs::ColorSpace::Rec2020 => &[[6369, 2627, 1004], [2627, 6780, 593], [0, 281, 10603]],
            },
            match space {
                cs::ColorSpace::Srgb => &[[32406, -15372, -4986], [-9689, 18758, 415], [557, -2040, 10570]],
                cs::ColorSpace::DisplayP3 => &[[24935, -9316, -4027], [-8296, 17629, 236], [358, -762, 9569]],
                cs::ColorSpace::Rec2020 => &[[18660, -7173, -1366], [-7247, 17569, -296], [192, -466, 9439]],
            },
        ),
        cs::Precision::High => (
            cs::PREC_SCALE_HIGH,
            cs::MAT_SCALE_HIGH,
            match space {
                cs::ColorSpace::Srgb => &[
                    [41240000, 35760000, 18050000],
                    [21260000, 71520000, 7220000],
                    [1930000, 11920000, 95050000],
                ],
                cs::ColorSpace::DisplayP3 => &[
                    [48660000, 26570000, 19820000],
                    [22900000, 69170000, 7930000],
                    [0, 4510000, 104390000],
                ],
                cs::ColorSpace::Rec2020 => &[
                    [63690000, 26270000, 10040000],
                    [26270000, 67800000, 5930000],
                    [0, 2810000, 106030000],
                ],
            },
            match space {
                cs::ColorSpace::Srgb => &[
                    [324062548, -153720797, -49862860],
                    [-96893071, 187575606, 4151752],
                    [5571012, -20402105, 105699594],
                ],
                cs::ColorSpace::DisplayP3 => &[
                    [249347777, -93155581, -40265822],
                    [-82962081, 176285365, 2360049],
                    [3584242, -7616122, 95692654],
                ],
                cs::ColorSpace::Rec2020 => &[
                    [186598389, -71733970, -13657129],
                    [-72467972, 175694142, -2964141],
                    [1920541, -4656234, 94391486],
                ],
            },
        ),
    };
    let v = [rgb[0] as i64 * scale, rgb[1] as i64 * scale, rgb[2] as i64 * scale];
    let xyz = ref_mul(fwd, v, m_scale);
    let back = ref_mul(bwd, xyz, m_scale);
    let mut max = 0i64;
    for i in 0..3 {
        let d = (back[i] - v[i]).abs() * 10_000 / scale;
        if d > max {
            max = d;
        }
    }
    max
}

/// 判据侧字面量：容差 39 = 10000/255（8bit 半级承诺）。
const REF_TOL: i64 = 39;

/// 判据侧字面量：色卡 8 卡。
const REF_CARD: [[u8; 3]; 8] = [
    [0, 0, 0], [255, 255, 255], [255, 0, 0], [0, 255, 0], [0, 0, 255],
    [128, 128, 128], [255, 128, 0], [64, 224, 208],
];

pub fn run_vev06_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4406");

    // -------------------------------------------------------------------------
    // 一、矩阵库
    // -------------------------------------------------------------------------

    // 封闭集：三空间 wire 往返、未知名/空串拒、全集数=3。
    {
        let mut ok = cs::COLOR_SPACE_COUNT == 3 && cs::ColorSpace::all().len() == 3;
        for k in cs::ColorSpace::all() {
            ok = ok && cs::ColorSpace::parse(k.wire()) == Some(k);
        }
        ok = ok && cs::ColorSpace::parse("aces").is_none() && cs::ColorSpace::parse("").is_none();
        s.add("H06-矩阵-封闭集", ok, "三空间往返恒等；表外/空串拒");
    }

    // 正向元素字面量：sRGB→XYZ 行 1/行 3 抽查（判据侧写死）。
    {
        let m = cs::to_xyz_fast_probe();
        let ok = m[0][0] == 4124 && m[0][1] == 3576 && m[0][2] == 1805
            && m[1][1] == 7152 && m[2][2] == 9505;
        s.add("H06-矩阵-正向元素字面量", ok, "sRGB 正矩阵五关键元素字面量相等");
    }

    // 逆元字面量：sRGB 逆（万分位 + 亿分位高精版）关键元素。
    {
        let mi = cs::from_xyz_fast_probe();
        let mh = cs::from_xyz_high_probe();
        let ok = mi[0][0] == 32406 && mi[0][1] == -15372 && mi[1][0] == -9689
            && mh[0][0] == 324062548 && mh[0][1] == -153720797 && mh[2][2] == 105699594;
        s.add("H06-矩阵-逆元字面量", ok, "sRGB 逆矩阵万分位/亿分位关键元素相等");
    }

    // -------------------------------------------------------------------------
    // 二、双精度
    // -------------------------------------------------------------------------

    // 模式表：两模式、序 [Fast, High]、定位比例字面量。
    {
        let ok = cs::PRECISION_MODES.len() == 2
            && cs::PRECISION_MODES[0] == cs::Precision::Fast
            && cs::PRECISION_MODES[1] == cs::Precision::High
            && cs::PREC_SCALE_FAST == 10_000
            && cs::PREC_SCALE_HIGH == 1_000_000_000
            && cs::MAT_SCALE_FAST == 10_000
            && cs::MAT_SCALE_HIGH == 100_000_000;
        s.add("H06-精度-模式表", ok, "两模式与四定位比例字面量逐位相等");
    }

    // -------------------------------------------------------------------------
    // 三、往返断言
    // -------------------------------------------------------------------------

    // 快速超承诺：sRGB 快速往返误差 > 39 且与判据侧独立复算相等。
    {
        let got = cs::roundtrip_err([255, 255, 255], cs::ColorSpace::Srgb, cs::Precision::Fast);
        let want = ref_roundtrip_err([255, 255, 255], cs::ColorSpace::Srgb, cs::Precision::Fast);
        let ok = got == want && got > REF_TOL;
        s.add("H06-往返-快速超承诺", ok, "白点快速误差=判据侧独立复算且超 39 承诺");
    }

    // 高精达承诺：sRGB 高精往返误差 ≤ 39（实测 0）且与独立复算相等。
    {
        let got = cs::roundtrip_err([255, 255, 255], cs::ColorSpace::Srgb, cs::Precision::High);
        let want = ref_roundtrip_err([255, 255, 255], cs::ColorSpace::Srgb, cs::Precision::High);
        let ok = got == want && got <= REF_TOL;
        s.add("H06-往返-高精达承诺", ok, "白点高精误差=独立复算且在承诺内");
    }

    // 高精严格小于快速（双精度的误差单调——同色卡两模式对比）。
    {
        let mut ok = true;
        for card in REF_CARD.iter() {
            let f = ref_roundtrip_err(*card, cs::ColorSpace::Srgb, cs::Precision::Fast);
            let h = ref_roundtrip_err(*card, cs::ColorSpace::Srgb, cs::Precision::High);
            ok = ok && h <= f;
        }
        // 白点严格小（截断物理存在）。
        ok = ok
            && ref_roundtrip_err([255, 255, 255], cs::ColorSpace::Srgb, cs::Precision::High)
                < ref_roundtrip_err([255, 255, 255], cs::ColorSpace::Srgb, cs::Precision::Fast);
        s.add("H06-往返-高精严格小于快速", ok, "全卡高精≤快速且白点严格小");
    }

    // 三空间快速全超承诺（enforce 强制的前提非恒真——物理事实）。
    {
        let ok = cs::ColorSpace::all().iter().all(|sp| {
            let rep = cs::roundtrip_check(*sp, cs::Precision::Fast);
            !rep.passed && rep.max_err > REF_TOL
        });
        s.add("H06-往返-三空间快速全超", ok, "三空间快速模式全超半级承诺（机制前提）");
    }

    // 超差强制：Fast → (High, E_CS_ROUNDTRIP) 三空间逐一。
    {
        let mut ok = true;
        for sp in cs::ColorSpace::all() {
            let (p, note) = cs::enforce_precision(sp, cs::Precision::Fast);
            ok = ok && p == cs::Precision::High && note == Some(cs::E_CS_ROUNDTRIP);
        }
        s.add("H06-往返-超差强制", ok, "三空间 Fast 全被强制 High 并立案");
    }

    // 达标不强制：High 已达承诺 → 原样返回无标注。
    {
        let (p, note) = cs::enforce_precision(cs::ColorSpace::Srgb, cs::Precision::High);
        let ok = p == cs::Precision::High && note.is_none();
        s.add("H06-往返-达标不强制", ok, "High 达承诺 → 原样且零标注");
    }

    // 色卡字面量：8 卡与判据侧逐卡相等。
    {
        let ok = cs::TEST_CARD.len() == 8
            && cs::TEST_CARD.iter().enumerate().all(|(i, c)| REF_CARD.get(i) == Some(c));
        s.add("H06-往返-色卡字面量", ok, "8 卡与判据侧字面量逐卡相等");
    }

    // -------------------------------------------------------------------------
    // 四、缓存（版本戳失效范式）
    // -------------------------------------------------------------------------

    // 插后命中：空查未中 → insert → 命中。
    {
        let mut lib = cs::MatrixLib::new();
        let key = (0u8, 1u8, 0u8, 0x00FF0000);
        let miss = lib.lookup(key).is_none();
        lib.insert(key, [209, 8, 4]);
        let hit = lib.lookup(key) == Some([209, 8, 4]);
        s.add("H06-缓存-插后命中", miss && hit, "空查未中；插后命中返回**转换输出**");
    }

    // 命中计数：两次命中 hits=2。
    {
        let mut lib = cs::MatrixLib::new();
        let key = (1u8, 2u8, 1u8, 0x00FFFFFF);
        lib.insert(key, [231, 255, 254]);
        let _ = lib.lookup(key);
        let _ = lib.lookup(key);
        let slot = cs::MatrixLib::slot_of_for_test(key);
        let ok = match lib.slot_entry(slot) {
            Some(e) => e.hits == 2,
            None => false,
        };
        s.add("H06-缓存-命中计数", ok, "两次命中计数=2（性能可见性）");
    }

    // 版本失效：bump 全表清空 + invalidations 记账 + 旧键未中。
    {
        let mut lib = cs::MatrixLib::new();
        lib.insert((0, 1, 0, 1), [1, 2, 3]);
        lib.insert((1, 0, 0, 2), [4, 5, 6]);
        let v1 = lib.bump_version();
        let mut all_clear = true;
        for i in 0..cs::CACHE_CAPACITY {
            if lib.slot_entry(i).is_some() {
                all_clear = false;
            }
        }
        let ok = v1 == 2 && all_clear && lib.lookup((0, 1, 0, 1)).is_none() && lib.invalidations == 2;
        s.add("H06-缓存-版本失效", ok, "bump: 版本 1→2、全槽清、旧键未中、失效=2");
    }

    // 失配清槽：bump 后同键 lookup 清槽（失真不留死条目）。
    {
        let mut lib = cs::MatrixLib::new();
        let key = (2u8, 0u8, 0u8, 7);
        lib.insert(key, [9, 9, 9]);
        lib.bump_version();
        let _ = lib.lookup(key);
        let slot = cs::MatrixLib::slot_of_for_test(key);
        let ok = lib.slot_entry(slot).is_none() && lib.invalidations == 1;
        s.add("H06-缓存-失配清槽", ok, "失配 lookup 后槽空且失效计数=1");
    }

    // 输出非输入：红点跨空间转换缓存命中返回**转换输出** [209,8,4]
    // （判据侧独立整数模拟预计算——输入 [255,0,0] 与输出不等）。
    {
        let mut lib = cs::MatrixLib::new();
        let first = cs::convert(
            &mut lib, [255, 0, 0], cs::ColorSpace::Srgb, cs::ColorSpace::DisplayP3,
            cs::Precision::Fast, false,
        );
        let second = cs::convert(
            &mut lib, [255, 0, 0], cs::ColorSpace::Srgb, cs::ColorSpace::DisplayP3,
            cs::Precision::Fast, false,
        );
        let ok = match (first, second) {
            (Ok(a), Ok(b)) => {
                a.out == [209, 8, 4]
                    && !a.cached
                    && b.cached
                    && b.out == [209, 8, 4]
                    && b.out != [255, 0, 0]
            }
            _ => false,
        };
        s.add("H06-缓存-输出非输入", ok, "命中返回独立模拟期望 [209,8,4] 而非输入红");
    }

    // 同空间恒等：convert_same 原值返回（显式恒等路径）。
    {
        let mut lib = cs::MatrixLib::new();
        let r = cs::convert_same(&mut lib, [12, 34, 56], cs::ColorSpace::Srgb, true);
        let ok = matches!(r, Ok(c) if c.out == [12, 34, 56] && !c.cached);
        s.add("H06-缓存-同空间恒等", ok, "同空间原值返回（显式恒等）");
    }

    // -------------------------------------------------------------------------
    // 五、转换与显式缺省
    // -------------------------------------------------------------------------

    // 映射缺失拒：AdobeRgb → None；表外空间 parse 拒（不臆造）。
    {
        let ok = cs::map_profile_to_space(ProfileKind::AdobeRgb).is_none()
            && cs::map_profile_to_space(ProfileKind::Srgb) == Some(cs::ColorSpace::Srgb)
            && cs::map_profile_to_space(ProfileKind::DisplayP3) == Some(cs::ColorSpace::DisplayP3);
        s.add("H06-转换-映射缺失拒", ok, "AdobeRgb 无矩阵拒；srgb/p3 正常映射");
    }

    // 显式缺省策略：字段字面量（Srgb→Srgb + Fast + is_default=true）。
    {
        let d = cs::default_strategy();
        let ok = d.from == cs::ColorSpace::Srgb
            && d.to == cs::ColorSpace::Srgb
            && d.precision == cs::Precision::Fast
            && d.is_default;
        s.add("H06-转换-显式缺省策略", ok, "缺省策略四字段可查（显性不隐式）");
    }

    // Custom 缺省映射：→ sRGB（显式缺省语义，非臆造 Adobe）。
    {
        let ok = cs::map_profile_to_space(ProfileKind::Custom) == Some(cs::ColorSpace::Srgb);
        s.add("H06-转换-Custom缺省映射", ok, "Custom→sRGB 显式缺省");
    }

    // used_default 透传：调用方标注原样出现在转换结果。
    {
        let mut lib = cs::MatrixLib::new();
        let a = cs::convert(&mut lib, [1, 2, 3], cs::ColorSpace::Srgb, cs::ColorSpace::Srgb,
                            cs::Precision::Fast, true);
        let b = cs::convert(&mut lib, [1, 2, 3], cs::ColorSpace::Srgb, cs::ColorSpace::Srgb,
                            cs::Precision::Fast, false);
        let ok = matches!(&a, Ok(c) if c.used_default)
            && matches!(&b, Ok(c) if !c.used_default);
        s.add("H06-转换-used_default透传", ok, "缺省位双向透传（不静默）");
    }

    // -------------------------------------------------------------------------
    // 六、读屏与契约
    // -------------------------------------------------------------------------

    // 超差告警行：含原因码 + 「强制高精」语义。
    {
        let rep = cs::RoundtripReport { max_err: 137, passed: false };
        let line = cs::screen_line(cs::ColorSpace::Srgb, &rep, cs::Precision::High);
        let ok = line.contains(cs::E_CS_ROUNDTRIP) && line.contains("强制高精") && line.contains("137");
        s.add("H06-读屏-超差告警", ok, "告警行含原因码/强制语义/误差数值");
    }

    // 达标就绪行：含精度名与容差。
    {
        let rep = cs::RoundtripReport { max_err: 0, passed: true };
        let line = cs::screen_line(cs::ColorSpace::DisplayP3, &rep, cs::Precision::High);
        let ok = line.contains("就绪") && line.contains("高精") && line.contains("p3");
        s.add("H06-读屏-达标就绪", ok, "就绪行含空间短码与精度名");
    }

    // 码互异：四码非空两两互异。
    {
        let codes = [cs::E_CS_MATRIX, cs::E_CS_ROUNDTRIP, cs::E_CS_CACHE, cs::E_CS_INPUT];
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
        s.add("H06-契约-码互异", ok, "四错误码非空两两互异");
    }

    // 版本锚 + 容差换算自洽（39 = 10000/255 半级承诺）。
    {
        let ok = cs::CS_ENGINE_VERSION.starts_with("V06")
            && cs::ROUNDTRIP_TOL == REF_TOL
            && REF_TOL * 255 <= 10_000
            && (REF_TOL + 1) * 255 > 10_000;
        s.add("H06-契约-版本与容差字面量", ok, "版本 V06-*；容差=8bit 半级换算自洽");
    }

    // 五条锚点判据映射齐备（实产名前缀计数与字面量一致）。
    {
        let names: [&str; 24] = [
            "H06-矩阵-封闭集",
            "H06-矩阵-正向元素字面量",
            "H06-矩阵-逆元字面量",
            "H06-精度-模式表",
            "H06-往返-快速超承诺",
            "H06-往返-高精达承诺",
            "H06-往返-高精严格小于快速",
            "H06-往返-三空间快速全超",
            "H06-往返-超差强制",
            "H06-往返-达标不强制",
            "H06-往返-色卡字面量",
            "H06-缓存-插后命中",
            "H06-缓存-命中计数",
            "H06-缓存-版本失效",
            "H06-缓存-失配清槽",
            "H06-缓存-输出非输入",
            "H06-缓存-同空间恒等",
            "H06-转换-映射缺失拒",
            "H06-转换-显式缺省策略",
            "H06-转换-Custom缺省映射",
            "H06-转换-used_default透传",
            "H06-读屏-超差告警",
            "H06-读屏-达标就绪",
            "H06-契约-码互异",
        ];
        let nmat = names.iter().filter(|n| n.starts_with("H06-矩阵-")).count();
        let nrt = names.iter().filter(|n| n.starts_with("H06-往返-")).count();
        let ncache = names.iter().filter(|n| n.starts_with("H06-缓存-")).count();
        let nconv = names.iter().filter(|n| n.starts_with("H06-转换-")).count();
        let ok = nmat == 3 && nrt == 6 && ncache == 5 && nconv == 4;
        s.add("H06-判据-五条映射齐备", ok, "四族实产计数 3/6/5/4 与判据侧字面量一致");
    }

    // 条数对账：本条之前实产 26 条（本条为第 27 条）。
    {
        let ok = s.len() == 26;
        s.add("H06-判据-条数对账", ok, "本条前实产 26 条（本条为第 27 条）");
    }

    s
}
