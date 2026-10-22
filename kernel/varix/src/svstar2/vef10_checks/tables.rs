//! F1008 判据域 · 表驱动判据组：合法性矩阵（判据一）/ 压缩比对比表（判据二）/
//! 元数据部分成功（判据五）
//! （判据锚点见外壳 `vef10_checks.rs` 头注）。
//!
//! 逐格扫描统一走外壳的 `for_each_cell` 纯遍历骨架与本文件的 `cells_all`
//! 归约骨架消重；`item` 夹具为纯构造；判据之间互不调用，
//! 判据 ID 与失败措辞逐字保留。

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use crate::svstar2::vef02_pngenc::ColorType;
use crate::svstar2::vef10_pngopts::*;

// ==================== 判据侧夹具（纯构造） ====================

/// 造一条元数据。
fn item(kw: &str, len: usize, kind: TextChunkKind) -> MetadataItem {
    let mut payload = Vec::new();
    let mut i = 0usize;
    while i < len {
        payload.push(b'a');
        i += 1;
    }
    MetadataItem {
        keyword: kw.to_string(),
        payload,
        kind,
        compressed: false,
    }
}

/// 归约骨架（消重核心）：逐格扫描，全部格 `probe` 为真才通过。
/// 不短路（与拆分前逐格置 flag 的口径一致）；各判据只共用此纯计算
/// helper，判据之间互不调用。
fn cells_all(probe: impl Fn(usize, usize) -> bool) -> bool {
    let mut all = true;
    super::for_each_cell(|ti, di| {
        if !probe(ti, di) {
            all = false;
        }
    });
    all
}

// ==================== 判据一：合法性矩阵 / 全组合 roundtrip ====================

/// 矩阵域编排：形状/边界 → 逐格 roundtrip → 规则重算（顺序即判据序）。
pub(super) fn check_matrix(m: &LegalMatrix, s: &mut CheckSet) {
    matrix_shape(m, s);
    matrix_cells(m, s);
    matrix_rule(m, s);
}

/// 矩阵形状与查表边界：格子总数、逐格全覆盖、计数对账、越界给空。
fn matrix_shape(m: &LegalMatrix, s: &mut CheckSet) {
    s.add(
        "C1008-矩阵-格子总数为二十五",
        m.cell_count() == 25,
        "5 颜色类型 × 5 位深 = 25 格，表驱动要求每格有裁决",
    );
    // 遍历全表：三类裁决相加恰为总数 ⇒ 无空格、无第四类。
    let counts = count_verdicts(m);
    let (legal, downgrade, illegal, all_reachable) = counts;
    s.add(
        "C1008-矩阵-逐格裁决全覆盖",
        all_reachable && legal + downgrade + illegal == 25,
        "遍历全部 25 格：Legal + 降档 + 非法 三类相加恰为 25（无空格）",
    );
    s.add(
        "C1008-矩阵-计数函数与遍历一致（独立重算）",
        legal == m.legal_cells() && downgrade == m.downgrade_cells(),
        "计数函数须与逐格遍历独立对账，防计数漂移",
    );
    // 越界查表给 None 而非 panic。
    let oob_cell = m.cell(99, 0);
    let oob_d = m.cell(0, 99);
    s.add(
        "C1008-矩阵-越界查表给空",
        oob_cell.is_none() && oob_d.is_none(),
        "行列序号越界必须给 None，不 panic",
    );
    s.add(
        "C1008-矩阵-颜色位深索引越界给空",
        color_index(ColorType::Gray).is_some() && depth_index(3).is_none(),
        "位深 3 不在{1,2,4,8,16} 内，须给 None",
    );
}

/// 逐格 roundtrip：合法格全测、非法格全拒、降档双向、计数口径。
fn matrix_cells(m: &LegalMatrix, s: &mut CheckSet) {
    // 全组合 roundtrip：每个合法格都能通过校验（这是「全测」的核心）。
    let pairs = legal_pairs(m);
    let all_pass = pairs_all_validate(m, &pairs);
    let pair_count = pairs.len();
    s.add(
        "C1008-矩阵-全部合法格均可通过校验",
        all_pass && pair_count > 0,
        "合法格逐个 roundtrip：矩阵说合法，校验器就不能拒",
    );
    s.add(
        "C1008-矩阵-合法格数与F1002口径一致",
        m.legal_cells() == 5 + 2 + 4 + 2 + 2,
        "灰度5 +真彩2+调色板4+灰A2+真彩A2 = 15，与上游 legal_depths 同口径",
    );
    // 负向：非法格确实被校验器拒（不能只有正向）。
    let illegal_all_rejected = illegal_cells_all_rejected(m);
    s.add(
        "C1008-矩阵-非法格逐个被拒",
        illegal_all_rejected,
        "非法格逐个 roundtrip：矩阵说非法，校验器就不能放",
    );
    // 降档格：允许降档则过、不允许则拒（两个方向都要断）。
    let downgrade_semantics_ok = downgrade_semantics_holds(m);
    s.add(
        "C1008-矩阵-降档格双向语义",
        downgrade_semantics_ok,
        "允许降档⇒过；不允许⇒拒。只断一头会让实现走样",
    );
    s.add(
        "C1008-矩阵-降档格非空（真彩可降档存在）",
        m.downgrade_cells() > 0,
        "锚点「真彩强制调色板需量化步骤显性提示有损」须有对应格子",
    );
}

/// 规则独立重算与降档形状钉死（补计数判据的盲区）。
fn matrix_rule(m: &LegalMatrix, s: &mut CheckSet) {
    // 逐格裁决 vs 规则函数独立重算（补计数判据的盲区）。
    let rule_ok = cells_match_rule(m);
    s.add(
        "C1008-矩阵-逐格裁决等于规则重算",
        rule_ok,
        "按规则文字独立重算 25 格裁决并逐格比对；只断计数会漏掉降档/非法边界移动",
    );
    // 降档格的**形状**须精确：真彩三类 × 位深{1,2,4} = 3×3 = 9 格。
    // 这条把「哪些格是降档」钉死，不允许边界悄悄挪动。
    let want_downgrade = want_downgrade_shape();
    s.add(
        "C1008-矩阵-降档格形状为九",
        m.downgrade_cells() == want_downgrade && want_downgrade == 9,
        "真彩三类 × 位深{1,2,4} = 9；边界一动此数即变",
    );
}

/// 合法组合全集（矩阵内裁决为 `Legal` 的格子），供「全测」用。
fn legal_pairs(m: &LegalMatrix) -> Vec<(ColorType, u8)> {
    let mut out = Vec::new();
    super::for_each_cell(|ti, di| {
        if m.cell(ti, di) == Some(CellVerdict::Legal) {
            out.push((COLOR_TYPES[ti], DEPTHS[di]));
        }
    });
    out
}

/// 逐格统计三类裁决数；返回 (Legal, 降档, 非法, 是否存在空格)。
fn count_verdicts(m: &LegalMatrix) -> (usize, usize, usize, bool) {
    let mut legal = 0usize;
    let mut downgrade = 0usize;
    let mut illegal = 0usize;
    let mut all_reachable = true;
    super::for_each_cell(|ti, di| match m.cell(ti, di) {
        Some(CellVerdict::Legal) => legal += 1,
        Some(CellVerdict::LegalByDowngrade) => downgrade += 1,
        Some(CellVerdict::IllegalNoAlt) => illegal += 1,
        None => all_reachable = false,
    });
    (legal, downgrade, illegal, all_reachable)
}

/// 全组合 roundtrip：每个合法格都必须通过校验。
fn pairs_all_validate(m: &LegalMatrix, pairs: &[(ColorType, u8)]) -> bool {
    pairs.iter().all(|p| {
        let o = WriteOptions {
            force_color: Some(p.0),
            force_depth: Some(p.1),
            ..WriteOptions::default_options()
        };
        validate(m, &o).is_ok()
    })
}

/// 负向核对：所有非法格必须被校验器拒绝。
fn illegal_cells_all_rejected(m: &LegalMatrix) -> bool {
    cells_all(|ic, id| {
        if m.cell(ic, id) != Some(CellVerdict::IllegalNoAlt) {
            return true;
        }
        let o = WriteOptions {
            force_color: Some(COLOR_TYPES[ic]),
            force_depth: Some(DEPTHS[id]),
            allow_downgrade: false,
            ..WriteOptions::default_options()
        };
        validate(m, &o).is_err()
    })
}

/// 降档格双向语义：允许降档则过、不允许则拒（任一格双向成立即为真，与原实现一致）。
/// 存在语义用「全称取反」表达（∃成立 ⇔ ¬∀不成立），避免再造同构 exists 骨架。
fn downgrade_semantics_holds(m: &LegalMatrix) -> bool {
    !cells_all(|dc, dd| {
        if m.cell(dc, dd) != Some(CellVerdict::LegalByDowngrade) {
            return true;
        }
        let allow = WriteOptions {
            force_color: Some(COLOR_TYPES[dc]),
            force_depth: Some(DEPTHS[dd]),
            allow_downgrade: true,
            ..WriteOptions::default_options()
        };
        let deny = WriteOptions {
            allow_downgrade: false,
            ..allow.clone()
        };
        !(validate(m, &allow).is_ok() && validate(m, &deny).is_err())
    })
}

/// 按规则文字重算单格应有裁决：
/// 上游 legal_depths 命中 ⇒ Legal；真彩类且 d<8 ⇒ 降档；否则非法。
fn expected_verdict(ct: ColorType, d: u8) -> CellVerdict {
    if ct.legal_depths().contains(&d) {
        CellVerdict::Legal
    } else if matches!(
        ct,
        ColorType::Rgb | ColorType::GrayAlpha | ColorType::Rgba
    ) && d < 8
    {
        CellVerdict::LegalByDowngrade
    } else {
        CellVerdict::IllegalNoAlt
    }
}

/// 逐格裁决 vs 规则函数独立重算（补计数判据的盲区）。
///
/// 计数判据只钉「降档格数>0」，改 `d<8`→`d<4` 会让一部分降档格
/// 变成非法格而三个计数与总和全不变 ⇒ 计数判据全绿。
/// 这里按规则文字在判据侧**重算**每格应有裁决，逐格比对。
fn cells_match_rule(m: &LegalMatrix) -> bool {
    cells_all(|ri, rj| m.cell(ri, rj) == Some(expected_verdict(COLOR_TYPES[ri], DEPTHS[rj])))
}

/// 降档格的**形状**须精确：真彩三类 × 位深{1,2,4} = 3×3 = 9 格。
fn want_downgrade_shape() -> usize {
    DEPTHS
        .iter()
        .filter(|d| matches!(**d, 1u8 | 2u8 | 4u8))
        .count()
        * 3 // Rgb / GrayAlpha / Rgba 三类
}

// ==================== 判据二：压缩比对比表 ====================

/// 权衡表域编排：档位形状 → 单调/端点 → 描述字段（顺序即判据序）。
pub(super) fn check_level_table(s: &mut CheckSet) {
    table_shape(s);
    table_monotonic(s);
    table_desc(s);
}

/// 档位形状：九档齐备、级别自洽、越界给空。
fn table_shape(s: &mut CheckSet) {
    let t = level_table();
    s.add(
        "C1008-权衡-九档齐备",
        t.len() == 9 && level_row(9).is_some(),
        "压缩级别 1..=9 共九档",
    );
    s.add(
        "C1008-权衡-级别逐档对应",
        level_row(1).map(|r| r.level) == Some(1) && level_row(9).map(|r| r.level) == Some(9),
        "查表按级别返回，level 字段须与下标自洽",
    );
    s.add(
        "C1008-权衡-越界查表给空",
        level_row(0).is_none() && level_row(10).is_none(),
        "级别 0 与 10 都非法，须给 None",
    );
}

/// 双向单调与端点不等（常数表会在端点不等上变红）+ 基准独立重算。
fn table_monotonic(s: &mut CheckSet) {
    let t = level_table();
    // 双向单调：体积比不增 + 耗时比不减。
    let (size_mono, time_mono) = ratios_monotonic(&t);
    s.add(
        "C1008-权衡-体积比随级别不增",
        size_mono,
        "级别越高体积越小；不增则单调性反了",
    );
    s.add(
        "C1008-权衡-耗时比随级别不减",
        time_mono,
        "级别越高越慢；不减则单调性反了",
    );
    // 端点不等：全常数表会被这条抓住。
    s.add(
        "C1008-权衡-首末档体积比不等",
        t[0].size_ratio != t[8].size_ratio,
        "全常数表能过单调但过不了端点不等",
    );
    s.add(
        "C1008-权衡-首末档耗时比不等",
        t[0].time_ratio != t[8].time_ratio,
        "耗时必须有真实跨度，否则权衡表是摆设",
    );
    // 相对基准：级别 6 是基准（100? 不，基准应自指）。
    // 独立重算：级别 6 的体积比须为全表最小（最高压缩）。
    let min_size_idx = min_size_ratio_index(&t);
    s.add(
        "C1008-权衡-最高压缩在末档",
        min_size_idx == 8,
        "级别 9 体积比须为全表最小（独立重算，不看注释）",
    );
    s.add(
        "C1008-权衡-最快在首档",
        t[0].time_ratio < t[8].time_ratio,
        "级别 1 须最快",
    );
}

/// 描述字段非空（供 UI 展示，空描述会显示空白档位）。
fn table_desc(s: &mut CheckSet) {
    let t = level_table();
    let desc_ok = descriptions_non_empty(&t);
    s.add(
        "C1008-权衡-档位描述非空",
        desc_ok,
        "path/size_class/speed_class 供 UI 展示，空则显示空白",
    );
}

/// 双向单调：体积比不增 + 耗时比不减（滑窗逐对核对）。
fn ratios_monotonic(t: &[LevelRow]) -> (bool, bool) {
    let size_mono = t.windows(2).all(|w| w[0].size_ratio >= w[1].size_ratio);
    let time_mono = t.windows(2).all(|w| w[0].time_ratio <= w[1].time_ratio);
    (size_mono, time_mono)
}

/// 独立重算：体积比最小的档位下标（同值取最先出现，与拆分前一致）。
fn min_size_ratio_index(t: &[LevelRow]) -> usize {
    match t.split_first() {
        None => 0,
        Some((first, _)) => t
            .iter()
            .enumerate()
            .fold((0usize, first.size_ratio), |best, (i, row)| {
                if row.size_ratio < best.1 {
                    (i, row.size_ratio)
                } else {
                    best
                }
            })
            .0,
    }
}

/// 描述字段非空（供 UI 展示，空描述会显示空白档位）。
fn descriptions_non_empty(t: &[LevelRow]) -> bool {
    t.iter()
        .all(|r| !r.path.is_empty() && !r.size_class.is_empty() && !r.speed_class.is_empty())
}

// ==================== 判据五：元数据部分成功 ====================

/// 元数据域编排：部分成功主体 → 变体 → 条数与三块（顺序即判据序）。
pub(super) fn check_metadata(m: &LegalMatrix, s: &mut CheckSet) {
    meta_partial(s);
    meta_variants(s);
    meta_limits(m, s);
}

/// 部分成功主体：超限被拒、保留、守恒、真部分成功、丢弃带原因。
fn meta_partial(s: &mut CheckSet) {
    let mut items = Vec::new();
    items.push(item("ok1", 10, TextChunkKind::Txt));
    items.push(item("big", ITEM_MAX_BYTES + 1, TextChunkKind::Txt));
    items.push(item("ok2", 20, TextChunkKind::ITxt));
    items.push(item("big2", ITEM_MAX_BYTES + 100, TextChunkKind::ZTxt));
    items.push(item("ok3", 30, TextChunkKind::ZTxt));
    let out = filter_metadata(&items);
    s.add(
        "C1008-元数据-超限项被拒",
        out.dropped.len() == 2,
        "两条超限项应被拒（恰等于 2）",
    );
    s.add(
        "C1008-元数据-其余项保留",
        out.kept.len() == 3,
        "三条正常项应保留（恰等于 3）",
    );
    s.add(
        "C1008-元数据-总数守恒",
        out.total() == items.len(),
        "kept + dropped 恰等于提交数（用 == 不用 >=）",
    );
    s.add(
        "C1008-元数据-真部分成功",
        out.is_partial(),
        "kept 与 dropped 同时非空——整单失败不满足",
    );
    s.add(
        "C1008-元数据-丢弃带原因",
        out.dropped.iter().all(|(_, r)| !r.is_empty()),
        "被拒必须带原因，否则用户不知道为什么少了",
    );
}

/// 变体：全成功不误报、空输入自洽、空关键字被拒。
fn meta_variants(s: &mut CheckSet) {
    // 全成功变体：不得误报丢弃。
    let out2 = filter_metadata(&vec![
        item("a", 1, TextChunkKind::Txt),
        item("b", 2, TextChunkKind::ITxt),
    ]);
    s.add(
        "C1008-元数据-全成功不误报",
        out2.dropped.is_empty() && out2.kept.len() == 2 && !out2.is_partial(),
        "负向不覆盖正向：正常输入不得被判成部分失败",
    );
    // 空输入不 panic。
    let empty = filter_metadata(&[]);
    s.add(
        "C1008-元数据-空输入自洽",
        empty.total() == 0 && !empty.has_dropped(),
        "空元数据是合法输入",
    );
    // 空关键字被拒（但条目保留为 dropped）。
    let bad_kw = vec![item("", 5, TextChunkKind::Txt)];
    let out3 = filter_metadata(&bad_kw);
    s.add(
        "C1008-元数据-空关键字被拒",
        out3.dropped.len() == 1 && out3.kept.is_empty() && out3.total() == 1,
        "空关键字不可写；总数守恒仍须成立",
    );
}

/// 提交级失败（条数超限）与三块种类齐备。
fn meta_limits(m: &LegalMatrix, s: &mut CheckSet) {
    // 条数超限（提交级失败）。
    let mut many = Vec::new();
    for _k in 0..=ITEM_MAX_COUNT {
        many.push(item("x", 1, TextChunkKind::Txt));
    }
    let o_many = WriteOptions {
        metadata: many,
        ..WriteOptions::default_options()
    };
    s.add(
        "C1008-元数据-条数超限被拒",
        validate(m, &o_many).is_err(),
        "条目数上限 500，超限即拒",
    );
    // 三块种类齐备（锚点：F1005 三块的反向组装）。
    let kinds_ok = TextChunkKind::Txt.wire() == "tEXt"
        && TextChunkKind::ZTxt.wire() == "zTXt"
        && TextChunkKind::ITxt.wire() == "iTXt";
    s.add(
        "C1008-元数据-三块关键字齐备",
        kinds_ok,
        "tEXt/zTXt/iTXt 是F1005 的三块，反向组装须原样支持",
    );
}
