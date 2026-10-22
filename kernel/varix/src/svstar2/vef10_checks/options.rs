//! F1008 判据域 · 选项行为判据组：快速模式（判据三）/ 非法组合拒绝（判据四）/
//! 门禁与自洽
//! （判据锚点见外壳 `vef10_checks.rs` 头注）。
//!
//! 逐格扫描统一走外壳的 `for_each_cell` 纯遍历骨架与本文件的
//! `fold_cells` / `cells_all` 归约骨架消重；
//! `blank_fault` / `abs_diff_usize` 夹具为纯计算；判据之间互不调用，
//! 判据 ID 与失败措辞逐字保留。

use alloc::string::String;

use crate::checks::CheckSet;

use crate::svstar2::vef02_pngenc::ColorType;
use crate::svstar2::vef10_pngopts::*;

// ==================== 判据侧夹具（纯计算） ====================

/// 「校验通过却要读故障」时的占位故障；判据只在 Err 分支真正使用其字段。
fn blank_fault() -> OptionFault {
    OptionFault {
        kind: OptionFaultKind::IllegalCombo,
        what: String::new(),
        why: String::new(),
        advice: String::new(),
        nearest: None,
    }
}

/// 两下标的绝对差（判据侧独立重算曼哈顿距离用）。
fn abs_diff_usize(a: usize, b: usize) -> usize {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// 泛型归约骨架（消重核心）：逐格扫描按 `f` 折叠，遍历顺序与拆分前一致。
/// `T: Copy`：本域三个实例化（bool/u32/usize）均为 Copy，折叠取值不发生移动。
fn fold_cells<T: Copy>(init: T, mut f: impl FnMut(T, usize, usize) -> T) -> T {
    let mut acc = init;
    super::for_each_cell(|ti, di| {
        acc = f(acc, ti, di);
    });
    acc
}

/// 全称归约骨架：逐格扫描，全部格 `probe` 为真才通过。
/// 不短路（与拆分前逐格置 flag 的口径一致）。
fn cells_all(mut probe: impl FnMut(usize, usize) -> bool) -> bool {
    fold_cells(true, |ok, ti, di| if probe(ti, di) { ok } else { false })
}

/// 非法组合夹具：强制 (色, 位深) 且不允许降档（判据四两处共用纯构造）。
fn combo_no_downgrade(base: &WriteOptions, c: ColorType, d: u8) -> WriteOptions {
    WriteOptions {
        force_color: Some(c),
        force_depth: Some(d),
        allow_downgrade: false,
        ..base.clone()
    }
}

/// 校验给定选项并取故障；Ok 时给占位故障（判据只在 Err 分支真正使用其字段）。
fn fault_of(m: &LegalMatrix, o: &WriteOptions) -> OptionFault {
    match validate(m, o) {
        Err(v) => v,
        Ok(()) => blank_fault(),
    }
}

// ==================== 判据三：快速模式收益 ====================

/// 快速模式域编排：基本收益 → 基线换算 → 行数/级别不变性（顺序即判据序）。
pub(super) fn check_fast_mode(s: &mut CheckSet) {
    fast_basic(s);
    fast_baseline(s);
    fast_invariance(s);
}

/// 开/关快速的基本收益断言（试探次数、体积与耗时代价声明）。
fn fast_basic(s: &mut CheckSet) {
    let rows = 64u32;
    let fast = estimate_fast_effect(true, rows, 6);
    let slow = estimate_fast_effect(false, rows, 6);
    s.add(
        "C1008-快速-开启后试探恒一",
        fast.filter_trials == 1,
        "锚点「跳过滤波试探」：试探次数必须与行数无关",
    );
    s.add(
        "C1008-快速-关闭时试探随行数放大",
        slow.filter_trials == rows,
        "非快速须逐行试探，否则「快速」省的不是试探开销",
    );
    s.add(
        "C1008-快速-体积代价为正（有损）",
        fast.size_cost_permille > 0 && fast.lossy,
        "又省时又省体积是假的；锚点明写体积 +8% 代价",
    );
    s.add(
        "C1008-快速-体积代价约百分之八",
        fast.size_cost_permille == 80,
        "锚点原值 +8%，写错常数会让 UI 承诺与实际不符",
    );
    s.add(
        "C1008-快速-耗时节省约百分之四十",
        fast.time_saved_permille == 400,
        "锚点原值 −40%",
    );
    s.add(
        "C1008-快速-关闭时无节省无代价声明",
        slow.time_saved_permille == 0 && slow.size_cost_permille == 0 && !slow.lossy,
        "非快速不该声明任何快速收益",
    );
}

/// 基线量纲换算与判据侧独立重算（弱门禁：判据不能向被测函数问答案）。
fn fast_baseline(s: &mut CheckSet) {
    let fast = estimate_fast_effect(true, 64, 6);
    // 【量纲校正】time_ratio_baseline 是**倍数×100**（级别1=18 即 0.18×），
    // time_saved_permille 是**千分比**（400 即 40%）。两者不同量纲，
    // 直接比较是判据自身写错（初版就是这么错的：18 >= 400 恒假）。
    // 正确口径：把基线换算成千分比（×1000）后与节省量比，且**必须**
    // 省下的量小于基线本身（省 100% 等于不耗时，物理上荒谬）。
    let baseline_permille: u16 = fast.time_ratio_baseline.saturating_mul(10);
    s.add(
        "C1008-快速-节省量为基线的一部分",
        fast.time_saved_permille < baseline_permille,
        "换算同量纲后：节省(千分比)须小于基线(倍数×10)，省满即荒谬",
    );
    // 判据侧**独立重算**基线（弱门禁第 7 条：判据向被测函数问答案＝自证式）。
    // 不复用 baseline_permille 反推，而是回级别权衡表按 level==6 取原始
    // time_ratio 再换算，两侧独立算出同一常数才算口径自洽。
    let expect_baseline_permille: u16 = level_table()
        .iter()
        .find(|r| r.level == 6)
        .map(|r| r.time_ratio.saturating_mul(10))
        .unwrap_or(0);
    s.add(
        "C1008-快速-基线取自级别六且换算自洽",
        fast.time_ratio_baseline == 160
            && expect_baseline_permille == 1600
            && baseline_permille == expect_baseline_permille,
        "级别6 耗时 1.60× ⇒ 千分比 1600；基线须取自权衡表而非硬编码",
    );
}

/// 行数无关性、级别影响与零行数安全。
fn fast_invariance(s: &mut CheckSet) {
    let rows = 64u32;
    // 行数无关性：不同行数下快速模式效果完全相同（结构事实）。
    let fast_small = estimate_fast_effect(true, 1, 6);
    let fast_big = estimate_fast_effect(true, 4096, 6);
    s.add(
        "C1008-快速-效果与行数无关",
        fast_small.filter_trials == fast_big.filter_trials
            && fast_small.time_saved_permille == fast_big.time_saved_permille
            && fast_small.size_cost_permille == fast_big.size_cost_permille,
        "跨行数逐项对账，抓「按行数偷偷改收益」的实现",
    );
    // 级别影响基线（快速模式不豁免级别对耗时的影响）。
    let fast_l1 = estimate_fast_effect(true, rows, 1);
    let fast_l9 = estimate_fast_effect(true, rows, 9);
    s.add(
        "C1008-快速-基线随级别变化",
        fast_l1.time_ratio_baseline != fast_l9.time_ratio_baseline,
        "级别 1 与 9 的耗时基线须不同（否则基线没接权衡表）",
    );
    // 零行数不 panic。
    let zero = estimate_fast_effect(false, 0, 6);
    s.add(
        "C1008-快速-零行不panic",
        zero.filter_trials == 0,
        "空图像也要给出可解释的 0 次试探",
    );
}

// ==================== 判据四：非法组合拒绝 ====================

/// 拒绝域编排：基础拒绝 → 恰近/建议 → 同距裁决 → 非法分支 → 级别/滤波
/// （顺序即判据序，六段各自独立登记）。
pub(super) fn check_rejection(m: &LegalMatrix, s: &mut CheckSet) {
    reject_combo(m, s);
    reject_nearest(m, s);
    reject_tie(m, s);
    reject_illegal_branch(m, s);
    reject_levels(m, s);
    reject_filters(m, s);
}

/// 基础拒绝：锚点原例「1-bit+真彩」与三要素/锚点/建议/合成行。
fn reject_combo(m: &LegalMatrix, s: &mut CheckSet) {
    // 锚点点名的例子：1-bit + 真彩（夹具基座就地内联构造）。
    let bad = combo_no_downgrade(&WriteOptions::default_options(), ColorType::Rgb, 1);
    let r = validate(m, &bad);
    s.add(
        "C1008-拒绝-一比特真彩被拒",
        r.is_err(),
        "锚点原例「1-bit+真彩」必须拒绝",
    );
    // validate 为纯函数，此处复取故障字段（Err 分支已由上一条钉死）。
    let f = fault_of(m, &bad);
    s.add(
        "C1008-拒绝-三要素齐全",
        !f.what.is_empty() && !f.why.is_empty() && !f.advice.is_empty(),
        "锚点「拒绝信息给合法组合建议（三要素）」",
    );
    s.add(
        "C1008-拒绝-规则引用带锚点",
        f.why.contains("F1008"),
        "为什么必须引用规则出处",
    );
    s.add(
        "C1008-拒绝-给出最近合法组合",
        f.nearest.is_some(),
        "非法组合必须附建议，否则用户只能自己猜",
    );
    s.add(
        "C1008-拒绝-三要素合成非空",
        f.three_elements().len() > 12 && f.three_elements().contains('｜'),
        "三要素合成本行供诊断直接输出",
    );
}

/// 恰近核对：建议距离须等于独立重算的最小曼哈顿距离；建议目标合法。
fn reject_nearest(m: &LegalMatrix, s: &mut CheckSet) {
    // 【恰近】独立重算最小曼哈顿距离，逐个非法格核对。
    let (nearest_ok, checked) = nearest_distances_ok(m);
    s.add(
        "C1008-拒绝-最近合法格恰为最小距离",
        nearest_ok && checked > 0,
        "独立重算最小曼哈顿距离并逐格核对；「总返回第一个合法格」会因距离不等变红",
    );
    // 建议的目标格本身必须合法（不能建议到一个非法格）。
    let suggestion_legal = suggestions_all_legal(m);
    s.add(
        "C1008-拒绝-建议目标本身合法",
        suggestion_legal,
        "建议到一个非法格等于把用户再坑一次",
    );
    // 同距留痕：距为 0 的查询（本身合法）不应报告同距。
    let tie_self = nearest_legal_with_tie(m, ColorType::Gray, 8);
    s.add(
        "C1008-拒绝-自身合法距离为零",
        tie_self.map(|(v, _)| v.distance == 0).unwrap_or(false),
        "合法格查最近应得距离 0（夹逼下界）",
    );
}

/// 同距裁决：确定、语料前提成立、取类型序最小（独立重算）。
fn reject_tie(m: &LegalMatrix, s: &mut CheckSet) {
    // 同距裁决必须**确定**：同一输入两次查询结果完全相同。
    // 【语料修正】原语料用 (Rgb,16) —— 该格本身合法、距离 0、**根本不发生同距**，
    // 于是 `tie.tied == false` 分支直接放过，同距规则压根没被观测到。
    // 真同距点是 (Palette,16)：该格 IllegalNoAlt，最近合法候选有三个同为距离 1
    // —— Rgb+16 / Palette+8 / GrayAlpha+16，正确裁决是类型序最小者 Rgb。
    let t1 = nearest_legal_with_tie(m, ColorType::Palette, 16);
    let t2 = nearest_legal_with_tie(m, ColorType::Palette, 16);
    let deterministic = match (t1, t2) {
        (Some((a, _)), Some((b, _))) => a.color == b.color && a.depth == b.depth,
        _ => false,
    };
    s.add(
        "C1008-拒绝-同距裁决确定",
        deterministic,
        "同距必须按类型序小者裁决，否则建议会飘",
    );
    // 【前提钉死】测试点必须**真的**发生同距，否则下面那条规则判据是空跑。
    s.add(
        "C1008-拒绝-同距语料前提成立",
        t1.map(|(_, tie)| tie.tied && tie.rivals >= 2).unwrap_or(false),
        "(Palette,16) 须报告同距且候选≥2；否则同距规则判据形同虚设",
    );
    // 同距时目标类型序号须为同距候选中的最小者（独立重算，不看实现）。
    let tie_rule_ok = match t1 {
        Some((v, tie)) => tie_rule_holds(m, v.color, v.distance as usize, tie.tied),
        None => false,
    };
    s.add(
        "C1008-拒绝-同距取类型序最小",
        tie_rule_ok,
        "独立重算同距候选中的最小类型序号，要求与实际返回一致",
    );
}

/// 非法格（IllegalNoAlt）分支的三要素/锚点/建议——补覆盖盲区。
fn reject_illegal_branch(m: &LegalMatrix, s: &mut CheckSet) {
    let base = WriteOptions::default_options();
    // 上面的组合 (Rgb,1) 走的是**降档分支**（LegalByDowngrade + 未允许降档），
    // 而 (Palette,16) 才走 `_` 非法分支。两支的 what/why/advice/nearest
    // 是四份独立文本，只测一支等于放掉另一支。
    let illegal_combo = combo_no_downgrade(&base, ColorType::Palette, 16);
    let ic_f = fault_of(m, &illegal_combo);
    s.add(
        "C1008-拒绝-非法格三要素齐全",
        !ic_f.what.is_empty() && !ic_f.why.is_empty() && !ic_f.advice.is_empty(),
        "(Palette,16) 走 IllegalNoAlt 分支，该支的三要素同样不许缺项",
    );
    s.add(
        "C1008-拒绝-非法格规则引用带锚点",
        ic_f.why.contains("F1008"),
        "非法分支的 why 也必须引用规则出处（两支各断一次）",
    );
    s.add(
        "C1008-拒绝-非法格附最近合法建议",
        ic_f.nearest.is_some(),
        "非法分支同样要给就近合法组合，不能只降档分支给",
    );
}

/// 级别取值边界：级别 10 与级别 0 都被拒。
fn reject_levels(m: &LegalMatrix, s: &mut CheckSet) {
    // 级别越界（夹具基座就地内联构造，两个越界夹具互不共享可变基座）。
    let bad_level = WriteOptions {
        level: 10,
        ..WriteOptions::default_options()
    };
    s.add(
        "C1008-拒绝-级别十被拒",
        validate(m, &bad_level).is_err(),
        "级别取值 1..=9",
    );
    let bad_level0 = WriteOptions {
        level: 0,
        ..WriteOptions::default_options()
    };
    s.add(
        "C1008-拒绝-级别零被拒",
        validate(m, &bad_level0).is_err(),
        "级别 0 非法（不是「未指定」而是越界）",
    );
}

/// 滤波编号边界：越界被拒、边界 0/4 合法（夹逼）。
fn reject_filters(m: &LegalMatrix, s: &mut CheckSet) {
    let base = WriteOptions::default_options();
    // 滤波编号越界。
    let bad_filter = WriteOptions {
        filter: FilterChoice::Fixed(9),
        ..base.clone()
    };
    s.add(
        "C1008-拒绝-滤波编号越界被拒",
        validate(m, &bad_filter).is_err(),
        "指定单滤波编号 0..=4",
    );
    // 边界：滤波 0 与 4 合法。
    let f0 = WriteOptions {
        filter: FilterChoice::Fixed(0),
        ..base.clone()
    };
    let f4 = WriteOptions {
        filter: FilterChoice::Fixed(4),
        ..base.clone()
    };
    s.add(
        "C1008-拒绝-滤波边界零四合法",
        validate(m, &f0).is_ok() && validate(m, &f4).is_ok(),
        "夹逼：越界判据不能把合法边界一起判死",
    );
}

/// 单个非法格的恰近核对：建议距离须等于独立重算的最小曼哈顿距离。
fn nearest_distance_ok(m: &LegalMatrix, ti: usize, di: usize) -> bool {
    let got = match nearest_legal(m, COLOR_TYPES[ti], DEPTHS[di]) {
        Some(v) => v,
        None => return false,
    };
    // 独立重算：从所有非非法格子里找最小距离，且不得更小。
    let best = recompute_min_manhattan(m, ti, di);
    let reachable = best != u32::MAX;
    let exact = reachable && got.distance as u32 == best;
    exact
}

/// 独立重算：从所有非非法格子里找最小曼哈顿距离（无可达格给 u32::MAX）。
fn recompute_min_manhattan(m: &LegalMatrix, ti: usize, di: usize) -> u32 {
    fold_cells(u32::MAX, |best, ci, cdi| {
        if m.cell(ci, cdi) != Some(CellVerdict::IllegalNoAlt) {
            let dist = (abs_diff_usize(ci, ti) + abs_diff_usize(cdi, di)) as u32;
            if dist < best {
                dist
            } else {
                best
            }
        } else {
            best
        }
    })
}

/// 【恰近】逐个非法格核对：建议距离须等于独立重算的最小距离。
/// 返回 (是否全部恰近, 核对过的非法格数)。
fn nearest_distances_ok(m: &LegalMatrix) -> (bool, usize) {
    let mut checked = 0usize;
    let nearest_ok = cells_all(|ti, di| {
        if m.cell(ti, di) != Some(CellVerdict::IllegalNoAlt) {
            return true;
        }
        checked += 1;
        nearest_distance_ok(m, ti, di)
    });
    (nearest_ok, checked)
}

/// 单格建议目标合法性核对：非非法格视为通过；
/// 建议目标（或其索引）不可解析、或落在非法格，则不通过。
fn suggestion_target_legal(m: &LegalMatrix, si: usize, sd: usize) -> bool {
    if m.cell(si, sd) != Some(CellVerdict::IllegalNoAlt) {
        return true;
    }
    let n = match nearest_legal(m, COLOR_TYPES[si], DEPTHS[sd]) {
        Some(n) => n,
        None => return true,
    };
    let ni = match color_index(n.color) {
        Some(v) => v,
        None => return false,
    };
    let ndi = match depth_index(n.depth) {
        Some(v) => v,
        None => return false,
    };
    m.cell(ni, ndi) != Some(CellVerdict::IllegalNoAlt)
}

/// 建议的目标格本身必须合法（不能建议到一个非法格）。
fn suggestions_all_legal(m: &LegalMatrix) -> bool {
    cells_all(|si, sd| suggestion_target_legal(m, si, sd))
}

/// 独立重算：与 (ti0, di0) 同距候选中的最小类型序号（无候选给 usize::MAX）。
fn min_type_index_at_distance(m: &LegalMatrix, ti0: usize, di0: usize, distance: usize) -> usize {
    fold_cells(usize::MAX, |min_ti, ci, cdi| {
        let hit = m.cell(ci, cdi) != Some(CellVerdict::IllegalNoAlt)
            && abs_diff_usize(ci, ti0) + abs_diff_usize(cdi, di0) == distance;
        if hit && ci < min_ti {
            ci
        } else {
            min_ti
        }
    })
}

/// 同距时目标类型序号须为同距候选中的最小者（独立重算，不看实现）。
fn tie_rule_holds(m: &LegalMatrix, got_color: ColorType, distance: usize, tied: bool) -> bool {
    if !tied {
        return false;
    }
    let (ti0, di0, got) = match (
        color_index(ColorType::Palette),
        depth_index(16),
        color_index(got_color),
    ) {
        (Some(ti0), Some(di0), Some(g)) => (ti0, di0, g),
        _ => return false,
    };
    let min_ti = min_type_index_at_distance(m, ti0, di0, distance);
    got != usize::MAX && got == min_ti
}

// ==================== 门禁与自洽 ====================

/// 门禁域编排：效果预声明 → 滤波选路 → 滤波边界 → 默认/容量（顺序即判据序）。
pub(super) fn check_gates(m: &LegalMatrix, s: &mut CheckSet) {
    gates_effect(m, s);
    gates_filter(s);
    gates_filter_bound(s);
    gates_default(m, s);
}

/// 效果预声明：隔行与快速互不干扰、元数据计数、非法选项无预声明。
fn gates_effect(m: &LegalMatrix, s: &mut CheckSet) {
    // 隔行开关与快速模式互相独立（不能联动）。
    let base = WriteOptions::default_options();
    let inter = WriteOptions {
        interlace: true,
        ..base.clone()
    };
    let both = WriteOptions {
        interlace: true,
        fast_mode: true,
        ..base.clone()
    };
    let (d1, d2) = (declare_effect(m, &inter), declare_effect(m, &both));
    s.add(
        "C1008-门禁-隔行与快速互不干扰",
        d1.as_ref().map(|d| d.interlace).unwrap_or(false)
            && d2.as_ref().map(|d| d.interlace && d.fast_mode).unwrap_or(false),
        "两个独立开关可同时开（Adam7 联动 F1003 不依赖快速模式）",
    );
    s.add(
        "C1008-门禁-效果预声明含元数据计数",
        d2.as_ref().map(|d| d.metadata_kept + d.metadata_dropped).unwrap_or(1) == 0,
        "预声明须把元数据保留/丢弃数带出来",
    );
    // 非法选项不得产出预声明（不能「带病预声明」）。
    let bad = WriteOptions {
        level: 99,
        ..base.clone()
    };
    s.add(
        "C1008-门禁-非法选项无预声明",
        declare_effect(m, &bad).is_none(),
        "校验没过就不该有效果预声明",
    );
}

/// 滤波四选路齐备与语义标记。
fn gates_filter(s: &mut CheckSet) {
    // 四选路齐备。
    s.add(
        "C1008-门禁-滤波四选路齐备",
        FILTER_CHOICES.len() == 5,
        "全部不滤波/最小绝对差和/最小熵/逐行自适应 + 指定单滤波一族",
    );
    let adaptive = FilterChoice::PerLineAdaptive;
    s.add(
        "C1008-门禁-逐行自适应标记为逐行",
        adaptive.is_per_line() && !FilterChoice::AllFixedNone.is_per_line(),
        "「自适应」必须真的逐行，否则与非快速试探语义冲突",
    );
    s.add(
        "C1008-门禁-指定滤波越界给空",
        FilterChoice::Fixed(3).fixed_kind().is_some()
            && FilterChoice::Fixed(9).fixed_kind().is_none(),
        "编号 3 合法、9 越界",
    );
}

/// 滤波编号边界：夹逼对、非 Fixed 无编号、滤波名非空。
fn gates_filter_bound(s: &mut CheckSet) {
    // 夹逼对：上界 4 必须**恰好**合法。原判据只测了 3 与 9，
    // 把上界从 4 改成 3 时Fixed(3) 仍合法 ⇒ 漏过。
    // 这里 0..=4 逐个断 Some、5..=9 逐个断 None，且返回值须等于入参。
    let bound_ok = filter_bounds_ok();
    s.add(
        "C1008-门禁-滤波上界四为夹逼对",
        bound_ok,
        "0..=4 逐个 Some(等于入参)、5..=9 逐个 None；上界挪动立即变红",
    );
    // 非Fixed 变体一律不给编号（一个 bool 不得表达两件事）。
    s.add(
        "C1008-门禁-非指定滤波无编号",
        !FilterChoice::AllFixedNone.fixed_kind().is_some()
            && !FilterChoice::AllMinAbsSum.fixed_kind().is_some()
            && !FilterChoice::AllMinEntropy.fixed_kind().is_some()
            && !FilterChoice::PerLineAdaptive.fixed_kind().is_some(),
        "只有 Fixed(k) 才有编号，其余四路须给 None",
    );
    s.add(
        "C1008-门禁-滤波名非空",
        FILTER_CHOICES.iter().all(|c| !c.wire().is_empty()),
        "滤波名供 UI 选项列表，空则显示空白项",
    );
}

/// 默认选项自洽与判据容量。
fn gates_default(m: &LegalMatrix, s: &mut CheckSet) {
    s.add(
        "C1008-门禁-默认选项自洽",
        {
            let d = WriteOptions::default_options();
            d.level == 6 && validate(m, &d).is_ok()
        },
        "默认选项必须自身合法（否则每次调用都报错）",
    );
    // 判据容量未溢出。
    s.add(
        "C1008-门禁-判据容量未溢出",
        !s.truncated(),
        "域自检项数须在 MAX_CHECKS 内，超出会静默丢红",
    );
}

/// 滤波编号夹逼：0..=4 逐个 Some(等于入参)、5..=9 逐个 None。
fn filter_bounds_ok() -> bool {
    let in_bounds = (0u8..=4).all(|bk| FilterChoice::Fixed(bk).fixed_kind() == Some(bk));
    let out_of_bounds = (5u8..=9).all(|bk| FilterChoice::Fixed(bk).fixed_kind().is_none());
    in_bounds && out_of_bounds
}
