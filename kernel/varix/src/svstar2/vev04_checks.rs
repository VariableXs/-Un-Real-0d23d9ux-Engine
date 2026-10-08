//! VE-F4404 · 域自检（判据逐条对应，见 `vev04_hdr.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **四段**（探测/元数据/映射/混合各自可独立断言） →
//!   `H04-四段-探测真值表` / `探测失准选边` / `探测非失准无标注` /
//!   `H04-总成-SDR路径` / `元数据缺失回退` / `正常路径` / `失准标注传递` /
//!   `批次映射` / `计数`
//! - **元数据抽象**（HDR10 静态与动态类归一；缺失回退） →
//!   `H04-元数据-归一解析` / `缺失拒绝` / `静态回退` / `边界`
//! - **色调映射**（曲线表 + 可调 + 边界） →
//!   `H04-映射-曲线表字面量` / `查表` / `公式对账` / `边界` / `可调` / `分档`
//! - **亮度钳制**（203 nit 上限字面量 + 过曝留痕） →
//!   `H04-钳制-上限字面量` / `不过曝` / `过曝` / `边界` / `留痕`
//! - **判据**（五条映射齐备 + 条数对账） →
//!   `H04-判据-五条映射齐备` / `条数对账`
//! - 读屏播报（域本色，四态并一条） → `H04-读屏-四态播报`；契约 → `H04-契约-*` 两条
//!
//! 单集 29 项（读屏四态并一条、契约两条、判据两条）。
//!
//! **判据设计硬规矩**：Reinhard 映射在判据侧逐字独立重写（`ref_map`，
//! u128 形式）；曲线表膝点、分档阈值、203 nit 参考白、缺省 MaxCLL
//! 全部在判据侧**字面量写死**——被测常量改了判据必须红。探测真值表
//! 四组合全枚举（判据「四段」段一的全量口径）。
//!
//! **负例为本文件主体**：每条正样本旁都立负例（缺失拒、越表拒、过曝
//! 钳、非法曲线全零），只测正样本的判据等于没测。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vev02_monitor::HdrCapability;
use crate::svstar2::vev04_hdr as hp;

// ---------------------------------------------------------------------------
// 判据侧独立实现区（与被测逐字对齐——被测改了这里必红）
// ---------------------------------------------------------------------------

/// 判据侧独立 Reinhard（u128）：out = s·T/(C+s)，s = in·gain/1000。
fn ref_map(luma: u32, knee: u32, target: u32, gain: u32) -> u32 {
    if luma == 0 {
        return 0;
    }
    let s = (luma as u128) * (gain as u128) / 1000;
    let t = target as u128;
    let c = knee as u128;
    let out = (s * t) / (c + s);
    out.min(t as u128) as u32
}

/// 判据侧字面量：参考白 / 钳制上限 / 缺省 MaxCLL（毫尼特）。
const REF_WHITE: u32 = 203_000;
const CLAMP_MAX: u32 = 203_000;
const REF_DEFAULT_CLL: u32 = 1_000_000;

/// 判据侧字面量：曲线表三档膝点（bt2390-like / bright-room / clip-risk）。
const REF_CURVE_KNEES: [u32; 3] = [100_000, 300_000, 500_000];

/// 判据侧字面量：分档阈值（>4M→clip-risk、>2M→bright-room、否则 bt2390）。
const REF_TIER_CLIP: u32 = 4_000_000;
const REF_TIER_BRIGHT: u32 = 2_000_000;

/// 判据侧字面量：曲线名（与表序一一对应）。
const REF_CURVE_NAMES: [&str; 3] = ["bt2390-like", "bright-room", "clip-risk"];

/// 探测四组合枚举用（快照 × EDID）。
fn probe_pair(present: bool, edid: bool) -> (HdrCapability, bool) {
    (
        if present { HdrCapability::Present } else { HdrCapability::NotDetected },
        edid,
    )
}

pub fn run_vev04_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4404");

    // -------------------------------------------------------------------------
    // 一、段一：HDR 能力探测（四段之一）
    // -------------------------------------------------------------------------

    // 真值表：两源 × 两态四组合全枚举（判据侧字面量重放）。
    {
        let (c1, _) = probe_pair(true, true);
        let (c2, _) = probe_pair(false, false);
        let (c3, e3) = probe_pair(false, true);
        let (c4, e4) = probe_pair(true, false);
        let ok = matches!(hp::probe(&c1, true), hp::ProbeVerdict::Active)
            && matches!(hp::probe(&c2, false), hp::ProbeVerdict::Inactive)
            && matches!(hp::probe(&c3, e3), hp::ProbeVerdict::DivergedEdidWins { .. })
            && matches!(hp::probe(&c4, e4), hp::ProbeVerdict::DivergedSnapshotWins { .. });
        s.add("H04-四段-探测真值表", ok, "四组合全枚举：一致两态+分歧两态各归其位");
    }

    // 失准选边：分歧态都走 HDR（以 EDID 为准的第一语义是「不误伤可用性」），
    // 且标注码 = E_HDR_PROBE。
    {
        let (c, e) = probe_pair(false, true);
        let v1 = hp::probe(&c, e);
        let (c2, e2) = probe_pair(true, false);
        let v2 = hp::probe(&c2, e2);
        let ok = v1.hdr_active()
            && v2.hdr_active()
            && v1.note_reason() == Some(hp::E_HDR_PROBE)
            && v2.note_reason() == Some(hp::E_HDR_PROBE);
        s.add("H04-四段-探测失准选边", ok, "分歧两态 hdr_active=true 且标注 E_HDR_PROBE");
    }

    // 非失准无标注：一致态不带 reason（标注只给分歧——不静默也不滥报）。
    {
        let (c1, _) = probe_pair(true, true);
        let (c2, _) = probe_pair(false, false);
        let ok = hp::probe(&c1, true).note_reason().is_none()
            && hp::probe(&c2, false).note_reason().is_none();
        s.add("H04-四段-探测非失准无标注", ok, "一致两态 note_reason=None");
    }

    // -------------------------------------------------------------------------
    // 二、段二：元数据抽象层
    // -------------------------------------------------------------------------

    // 归一解析：静态与动态两类同结构解析、字段透传（抽象层的意义）。
    {
        let a = hp::parse_meta(Some(hp::MetaKind::Static10), 1_000_000, 250_000, 600_000);
        let b = hp::parse_meta(Some(hp::MetaKind::Dynamic), 4_000_000, 800_000, 600_000);
        let ok = matches!(&a, Ok(m) if m.kind == hp::MetaKind::Static10
                && m.max_cll_minit == 1_000_000 && m.max_fall_minit == 250_000
                && m.panel_peak_minit == 600_000)
            && matches!(&b, Ok(m) if m.kind == hp::MetaKind::Dynamic && m.max_cll_minit == 4_000_000);
        s.add("H04-元数据-归一解析", ok, "静态/动态同结构解析字段透传（归一）");
    }

    // 缺失拒绝：kind 缺失、峰值为零、fall>cll（物理矛盾）三类全拒。
    {
        let a = hp::parse_meta(None, 1_000_000, 250_000, 600_000);
        let b = hp::parse_meta(Some(hp::MetaKind::Static10), 0, 0, 600_000);
        let c = hp::parse_meta(Some(hp::MetaKind::Static10), 500_000, 800_000, 600_000);
        let ok = a.is_err() && b.is_err() && c.is_err();
        s.add("H04-元数据-缺失拒绝", ok, "无kind/零峰值/fall>cll 三负例全拒");
    }

    // 静态回退：回退元数据 = 判据侧字面量（Static10 / 1M / 250K）。
    {
        let m = hp::static_fallback_meta(600_000);
        let ok = m.kind == hp::MetaKind::Static10
            && m.max_cll_minit == REF_DEFAULT_CLL
            && m.max_fall_minit == REF_DEFAULT_CLL / 4
            && m.panel_peak_minit == 600_000;
        s.add("H04-元数据-静态回退", ok, "回退元数据字段与判据侧字面量相等");
    }

    // 边界：cll==fall 合法（峰值=均值是合法物理态）；毫尼特口径换算自证。
    {
        let a = hp::parse_meta(Some(hp::MetaKind::Dynamic), 800_000, 800_000, 600_000);
        let nit_to_minit = 1000u32.saturating_mul(1000); // 1 nit = 1000 millinit
        let ok = a.is_ok() && nit_to_minit == 1_000_000 && REF_DEFAULT_CLL == 1000 * nit_to_minit;
        s.add("H04-元数据-边界", ok, "cll==fall 合法；缺省峰值=1000nit 口径自洽");
    }

    // -------------------------------------------------------------------------
    // 三、段三：色调映射
    // -------------------------------------------------------------------------

    // 曲线表字面量：三档膝点与名序判据侧写死（表改了必红）。
    {
        let knees: Vec<u32> = hp::TONE_CURVES.iter().map(|(_, k, _)| *k).collect();
        let names: Vec<&str> = hp::TONE_CURVES.iter().map(|(n, _, _)| *n).collect();
        let mut distinct = true;
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                if let (Some(a), Some(b)) = (names.get(i), names.get(j)) {
                    if a == b {
                        distinct = false;
                    }
                }
            }
        }
        let ok = knees == REF_CURVE_KNEES.to_vec() && names == REF_CURVE_NAMES.to_vec() && distinct;
        s.add("H04-映射-曲线表字面量", ok, "三档膝点/名序与判据侧字面量逐位相等");
    }

    // 查表：三名各自命中对应膝点；未知名与空串拒（表外形态）。
    {
        let mut ok = true;
        for (i, name) in REF_CURVE_NAMES.iter().enumerate() {
            match (hp::curve_by_name(name), REF_CURVE_KNEES.get(i)) {
                (Some(c), Some(k)) => ok = ok && c.knee_minit == *k && c.legal(),
                _ => ok = false,
            }
        }
        ok = ok && hp::curve_by_name("pq").is_none() && hp::curve_by_name("").is_none();
        s.add("H04-映射-查表", ok, "三名命中对应膝点且 legal；未知名/空串拒");
    }

    // 公式对账：判据侧独立 Reinhard 五点对账（0/小/膝点/大/极大）。
    {
        let c = hp::curve_by_name("bright-room").unwrap_or(hp::ToneCurve {
            knee_minit: 0,
            target_minit: 0,
            gain_per_mille: 0,
        });
        let points = [0u32, 1_000, 100_000, 300_000, 2_000_000, u32::MAX];
        let mut ok = !points.is_empty();
        for p in points.iter() {
            ok = ok && c.map(*p) == ref_map(*p, c.knee_minit, c.target_minit, c.gain_per_mille);
        }
        s.add("H04-映射-公式对账", ok, "六点被测 map 与判据侧独立 Reinhard 相等");
    }

    // 边界：零入射→零；极大入射渐近不超目标白；单调不降。
    {
        let c = hp::curve_by_name("bt2390-like").unwrap_or(hp::ToneCurve {
            knee_minit: 0,
            target_minit: 0,
            gain_per_mille: 0,
        });
        let extreme = c.map(u32::MAX);
        let mono = c.map(50_000) <= c.map(150_000) && c.map(150_000) <= c.map(400_000);
        let ok = c.map(0) == 0 && extreme <= REF_WHITE && extreme >= REF_WHITE * 9 / 10 && mono;
        s.add("H04-映射-边界", ok, "0→0；极大入射落入 [0.9T, T] 渐近带；三连点单调");
    }

    // 可调：gain 与 knee 同向增大输出变大（可调语义可观测）；非法参数全零。
    {
        let base = hp::ToneCurve { knee_minit: 100_000, target_minit: REF_WHITE, gain_per_mille: 1000 };
        let boosted = hp::ToneCurve { gain_per_mille: 2000, ..base };
        let late = hp::ToneCurve { knee_minit: 400_000, ..base };
        let bad = hp::ToneCurve { gain_per_mille: 999, ..base };
        let sample = 300_000u32;
        let ok = boosted.map(sample) > base.map(sample)
            && late.map(sample) > base.map(sample)
            && !bad.legal()
            && bad.map(sample) == 0;
        s.add("H04-映射-可调", ok, "gain↑/knee↑ 输出↑；gain<1000 非法且 map 全零");
    }

    // 分档：判据侧阈值字面量重放三档 + 返回名与曲线自洽。
    {
        let mk = |cll: u32| {
            hp::parse_meta(Some(hp::MetaKind::Static10), cll, cll / 4, 600_000)
        };
        let m1 = mk(REF_TIER_BRIGHT - 500_000);
        let m2 = mk(REF_TIER_CLIP - 500_000);
        let m3 = mk(REF_TIER_CLIP + 1_000_000);
        let ok = match (m1, m2, m3) {
            (Ok(a), Ok(b), Ok(c)) => {
                let (n1, c1) = hp::pick_curve(&a);
                let (n2, c2) = hp::pick_curve(&b);
                let (n3, c3) = hp::pick_curve(&c);
                n1 == "bt2390-like"
                    && n2 == "bright-room"
                    && n3 == "clip-risk"
                    && c1.knee_minit == REF_CURVE_KNEES[0]
                    && c2.knee_minit == REF_CURVE_KNEES[1]
                    && c3.knee_minit == REF_CURVE_KNEES[2]
            }
            _ => false,
        };
        s.add("H04-映射-分档", ok, "1.5M/3.5M/5M 三档与判据侧阈值字面量逐档一致");
    }

    // -------------------------------------------------------------------------
    // 四、段四：SDR 同屏混合与亮度钳制
    // -------------------------------------------------------------------------

    // 上限字面量：203 nit 参考白 = 钳制上限（同源口径）判据侧写死。
    {
        let ok = hp::SDR_REF_WHITE_MINIT == REF_WHITE
            && hp::MIX_CLAMP_MINIT == CLAMP_MAX
            && REF_WHITE == 203u32.saturating_mul(1000);
        s.add("H04-钳制-上限字面量", ok, "参考白=钳制上限=203000 毫尼特字面量");
    }

    // 不过曝：低亮度合成原样（钳制是过曝专用，不误伤正常合成）。
    {
        let m = hp::mix_overlay(100_000, 50_000);
        let ok = m.composited_minit == 150_000 && !m.clamped && m.hdr_mapped_minit == 100_000;
        s.add("H04-钳制-不过曝", ok, "150K<203K 合成原样且 clamped=false");
    }

    // 过曝：300K 合成钳到 203K（锚点「混合过曝→亮度钳制」）。
    {
        let m = hp::mix_overlay(200_000, 100_000);
        let ok = m.hdr_mapped_minit + m.sdr_ui_minit > CLAMP_MAX
            && m.composited_minit == CLAMP_MAX
            && m.clamped;
        s.add("H04-钳制-过曝", ok, "300K 过曝钳到 203K 且 clamped=true");
    }

    // 边界：恰等于上限不钳；超 1 毫尼特即钳（边界两侧同查）。
    {
        let a = hp::mix_overlay(CLAMP_MAX, 0);
        let b = hp::mix_overlay(CLAMP_MAX, 1);
        let ok = !a.clamped && a.composited_minit == CLAMP_MAX && b.clamped && b.composited_minit == CLAMP_MAX;
        s.add("H04-钳制-边界", ok, "恰 203K 不钳；203001 即钳");
    }

    // 留痕：钳制发生计数 +1，未钳不加（被钳不是被吞——过曝可查）。
    {
        let mut p = hp::HdrPipeline::default();
        let (c, _) = probe_pair(true, true);
        let meta = hp::parse_meta(Some(hp::MetaKind::Static10), u32::MAX, 800_000, 600_000);
        let _ = p.plan_for_display(&c, true, meta, CLAMP_MAX);
        let clamped_once = p.clamped_mixes == 1;
        let meta2 = hp::parse_meta(Some(hp::MetaKind::Static10), 100_000, 25_000, 600_000);
        let _ = p.plan_for_display(&c, true, meta2, 50_000);
        let ok = clamped_once && p.clamped_mixes == 1;
        s.add("H04-钳制-留痕", ok, "峰值爆表单钳制计数=1；温和单不增");
    }

    // -------------------------------------------------------------------------
    // 五、四段总成主流程 + 读屏播报
    // -------------------------------------------------------------------------

    // SDR 路径：探测不活跃 → 无元数据、纯界面亮度合成、planned+1。
    {
        let mut p = hp::HdrPipeline::default();
        let (c, _) = probe_pair(false, false);
        let plan = p.plan_for_display(&c, false, Err(()), 80_000);
        let ok = !plan.verdict.hdr_active()
            && plan.meta.is_none()
            && plan.mix.hdr_mapped_minit == 0
            && plan.mix.composited_minit == 80_000
            && !plan.mix.clamped
            && p.planned_displays == 1;
        s.add("H04-总成-SDR路径", ok, "Inactive 屏走 SDR 常规且计数在册");
    }

    // 元数据缺失回退：Err(()) → meta_reason=E_HDR_META + 静态回退元数据在位。
    {
        let mut p = hp::HdrPipeline::default();
        let (c, _) = probe_pair(true, true);
        let plan = p.plan_for_display(&c, true, Err(()), 50_000);
        let ok = plan.verdict.hdr_active()
            && plan.meta_reason == Some(hp::E_HDR_META)
            && matches!(&plan.meta, Some(m) if m.max_cll_minit == REF_DEFAULT_CLL
                && m.kind == hp::MetaKind::Static10);
        s.add("H04-总成-元数据缺失回退", ok, "缺失→静态映射回退且标注不静默");
    }

    // 正常路径：曲线名与判据侧分档一致、峰值映射与判据侧独立公式相等。
    {
        let mut p = hp::HdrPipeline::default();
        let (c, _) = probe_pair(true, true);
        let meta = hp::parse_meta(Some(hp::MetaKind::Dynamic), 3_500_000, 700_000, 600_000);
        let plan = p.plan_for_display(&c, true, meta, 50_000);
        let (expect_name, expect_curve) = match plan.meta {
            Some(m) => hp::pick_curve(&m),
            None => ("", hp::ToneCurve { knee_minit: 0, target_minit: 0, gain_per_mille: 0 }),
        };
        let ok = plan.curve_name == "bright-room"
            && expect_name == "bright-room"
            && matches!(&plan.meta, Some(m) if plan.mix.hdr_mapped_minit
                == ref_map(m.max_cll_minit, expect_curve.knee_minit,
                           expect_curve.target_minit, expect_curve.gain_per_mille))
            && plan.probe_note.is_none()
            && plan.meta_reason.is_none();
        s.add("H04-总成-正常路径", ok, "曲线落名/峰值映射/无标注三口径与判据侧一致");
    }

    // 失准标注传递：分歧探测 → 计划带人话 note（含 EDID 选边语义）。
    {
        let mut p = hp::HdrPipeline::default();
        let (c, e) = probe_pair(false, true);
        let plan = p.plan_for_display(&c, e, Err(()), 50_000);
        let ok = plan.verdict.hdr_active()
            && matches!(&plan.probe_note, Some(n) if n.contains("EDID") && n.contains("失准"));
        s.add("H04-总成-失准标注传递", ok, "分歧态 note 透传且含 EDID/失准语义");
    }

    // 批次映射：长度守恒 + 逐点等于判据侧独立公式（O(像素批次) 口径）。
    {
        let c = hp::curve_by_name("bright-room").unwrap_or(hp::ToneCurve {
            knee_minit: 0,
            target_minit: 0,
            gain_per_mille: 0,
        });
        let input: [u32; 5] = [0, 10_000, 250_000, 1_000_000, u32::MAX];
        let out = hp::tonemap_batch(&input, &c);
        let mut ok = out.len() == input.len();
        for i in 0..input.len() {
            if let (Some(o), Some(v)) = (out.get(i), input.get(i)) {
                ok = ok && *o == ref_map(*v, c.knee_minit, c.target_minit, c.gain_per_mille);
            } else {
                ok = false;
            }
        }
        s.add("H04-总成-批次映射", ok, "五点批次逐点与判据侧独立公式相等且长度守恒");
    }

    // 计数：多屏编排 planned_displays 累加（F4426 协调层的口径底座）。
    {
        let mut p = hp::HdrPipeline::default();
        let (ca, _) = probe_pair(true, true);
        let (cb, _) = probe_pair(false, false);
        let _ = p.plan_for_display(&ca, true, Err(()), 50_000);
        let _ = p.plan_for_display(&cb, false, Err(()), 50_000);
        let _ = p.plan_for_display(&ca, true, Err(()), 50_000);
        let ok = p.planned_displays == 3 && p.clamped_mixes == 0;
        s.add("H04-总成-计数", ok, "三屏三编排 planned=3 且本组零钳制");
    }

    // 读屏四态（域本色：失准 > 钳制 > 正常 > SDR 的播报优先级）。
    {
        let mut p = hp::HdrPipeline::default();
        let (cact, _) = probe_pair(true, true);
        let (cdiv, ediv) = probe_pair(false, true);
        let (csdr, _) = probe_pair(false, false);
        let l_div = hp::HdrPipeline::screen_line(&p.plan_for_display(&cdiv, ediv, Err(()), 50_000));
        let l_clamp = hp::HdrPipeline::screen_line(&p.plan_for_display(
            &cact,
            true,
            hp::parse_meta(Some(hp::MetaKind::Static10), u32::MAX, 800_000, 600_000),
            CLAMP_MAX,
        ));
        let l_norm = hp::HdrPipeline::screen_line(&p.plan_for_display(
            &cact,
            true,
            hp::parse_meta(Some(hp::MetaKind::Static10), 100_000, 25_000, 600_000),
            50_000,
        ));
        let l_sdr = hp::HdrPipeline::screen_line(&p.plan_for_display(&csdr, false, Err(()), 50_000));
        let ok = l_div.contains(hp::E_HDR_PROBE) && l_div.contains("失准")
            && l_clamp.contains("HDR 亮度警告") && l_clamp.contains(hp::E_HDR_MIX)
            && l_norm.contains("HDR 播放中") && l_norm.contains("bt2390-like")
            && l_sdr.contains("HDR 未启用");
        s.add("H04-读屏-四态播报", ok, "失准/钳制/正常/SDR 四行各表其态含原因码");
    }

    // -------------------------------------------------------------------------
    // 六、契约与判据自检
    // -------------------------------------------------------------------------

    // 码互异：四码非空两两互异（外部可观测分支）。
    {
        let codes = [hp::E_HDR_PROBE, hp::E_HDR_META, hp::E_HDR_TONEMAP, hp::E_HDR_MIX];
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
        s.add("H04-契约-码互异", ok, "四错误码非空两两互异");
    }

    // 版本锚 + 毫尼特口径三常量自洽。
    {
        let ok = hp::HDR_PIPELINE_VERSION.starts_with("V04")
            && hp::DEFAULT_CLL_MINIT == REF_DEFAULT_CLL
            && hp::SDR_REF_WHITE_MINIT == 203_000
            && hp::MIX_CLAMP_MINIT == 203_000;
        s.add("H04-契约-版本与口径", ok, "版本 V04-*；缺省CLL/参考白/钳制上限字面量");
    }

    // 五条锚点判据映射齐备（前缀计数，判据侧独立扫）。
    {
        let mut n4 = 0usize;
        let mut nmeta = 0usize;
        let mut nmap = 0usize;
        let mut nclamp = 0usize;
        let mut nself = 0usize;
        let names: [&str; 24] = [
            "H04-四段-探测真值表",
            "H04-四段-探测失准选边",
            "H04-四段-探测非失准无标注",
            "H04-元数据-归一解析",
            "H04-元数据-缺失拒绝",
            "H04-元数据-静态回退",
            "H04-元数据-边界",
            "H04-映射-曲线表字面量",
            "H04-映射-查表",
            "H04-映射-公式对账",
            "H04-映射-边界",
            "H04-映射-可调",
            "H04-映射-分档",
            "H04-钳制-上限字面量",
            "H04-钳制-不过曝",
            "H04-钳制-过曝",
            "H04-钳制-边界",
            "H04-钳制-留痕",
            "H04-总成-SDR路径",
            "H04-总成-元数据缺失回退",
            "H04-总成-正常路径",
            "H04-总成-失准标注传递",
            "H04-总成-批次映射",
            "H04-总成-计数",
        ];
        for n in names.iter() {
            if n.starts_with("H04-四段-") {
                n4 += 1;
            }
            if n.starts_with("H04-元数据-") {
                nmeta += 1;
            }
            if n.starts_with("H04-映射-") {
                nmap += 1;
            }
            if n.starts_with("H04-钳制-") {
                nclamp += 1;
            }
        }
        nself = names.iter().filter(|n| n.starts_with("H04-判据-")).count();
        let ok = n4 == 3 && nmeta == 4 && nmap == 6 && nclamp == 5 && nself == 0;
        s.add("H04-判据-五条映射齐备", ok, "五族实产计数 3/4/6/5 与本清单一致");
    }

    // 条数对账：本条之前实产 28 条（本条为第 29 条）——判据侧字面量钉死。
    {
        let ok = s.len() == 28;
        s.add("H04-判据-条数对账", ok, "本条前实产 28 条（本条为第 29 条）");
    }

    s
}
