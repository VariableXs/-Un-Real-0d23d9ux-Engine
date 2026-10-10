//! VE-F1629 · 域自检（锚点判据五条逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1629`
//!
//! **锚点判据五条 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 条数 |
//! |---|---|---|
//! | 状态排序 | `C32-排序-*` | 13（稳定保序/PsoFirst 字典序手算/切换达下限/收益量化/非法先拒/seq 互异双向/空与单边界/全键切换量化/置换不变量双向/保序自检反恒假/策略族封闭/三策略切换手算/at_floor 适用范围） |
//! | 状态排序·切换成本 | `C32-成本-*` | 4（分档权重/分档成本手算/三策略成本手算/均匀权重退化对拍） |
//! | 合并 | `C32-合并-*` | 12（同 PSO 合并/跨 PSO 不硬合/空绘制跳过/收益量化/成员保序/合计与容量/实例守卫双向/合并自检反恒假/触顶切批/容量上限非法双向/材质跨度双向/可合并相邻批必拒） |
//! | 预算 | `C32-预算-*` | 8（恰预算判内/超 1 边界/大超精确/非法双向/默认冻结/告警是信号非闸/分档判定/headroom 饱和减） |
//! | 边界 | `C32-边界-*` | 6（正路径/I04 关键词/非实例合并与预算口径/实例语义闭环/材质跨度承载/容量上限声明） |
//! | 判据 | `C32-判据-*` | 10（主管线端到端/主管线超预算/呈现文本/版本在案/码段真调邻域/码值写死/码互异/双策略端到端/脏输入拦截/条数对账） |
//!
//! # 本层核心纪律：判据侧手算写死，不向被测要答案
//!
//! - 排序/合并/成本收益（before/after/saved）全部判据侧手算后写死对拍——
//!   收益数字若由被测回算自比自身，改了计量口径判据也恒真。
//! - 码段判据**真调邻域模块的码常量**（vef31 `IdCode` 八码、vef27
//!   `MtCode` 五码）逐个与本族七码比对，而不是只断「高字节全等 0x4E」——
//!   后者对本族恒真，断不出串码。邻域码取自模块而非写死字面量：写死
//!   则邻域换码本判据仍绿，那成了假证据。
//! - 判据区**零 panic 面**：无 `unwrap()`/`expect()`/下标索引，一律走
//!   `.get()` + `match`，被测改坏时症状必须是可定位红项而非判据先崩。
//! - 双向断言：跨 PSO 必须不合并（拒「该合不合」的反面——「不该合
//!   也合」即伪造管线状态），恰预算必须不告警，未触顶的同 PSO 相邻
//!   批必须被拦（拒「留白送调用」）。

use crate::checks::CheckSet;
use crate::svstar2::vef32_batching as bt;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立真值区（字面量写死，不引用被测常量自比自身）
// ---------------------------------------------------------------------------

const REF_VERSION: &str = "F32-batching-v1";
const REF_DRAW_INVALID: u16 = 0x4E01;
const REF_SORT_VIOLATION: u16 = 0x4E02;
const REF_MERGE_VIOLATION: u16 = 0x4E03;
const REF_BUDGET_INVALID: u16 = 0x4E04;
const REF_BOUNDARY_MISMATCH: u16 = 0x4E05;
const REF_INSTANCE_FORBIDDEN: u16 = 0x4E06;
const REF_CAP_INVALID: u16 = 0x4E07;
const REF_CODE_COUNT: usize = 7;
const REF_DEFAULT_BUDGET: u32 = 2_000;
const REF_MAX_BUDGET: u32 = 100_000;
const REF_I04_KEYWORDS: [&str; 3] = ["I04", "实例", "非实例合并"];
const REF_STRATEGY_COUNT: usize = 3;

/// 判据总条数（条数对账用——运行值而非源码行数计）。
const REF_CHECK_TOTAL: usize = 53;

/// 邻域模块的码段（真调常量——不写死字面量）。
fn neighbour_codes() -> Vec<u16> {
    use crate::svstar2::vef27_morphtarget::MtCode;
    use crate::svstar2::vef31_indirect::IdCode;
    let mut v: Vec<u16> = Vec::new();
    v.push(IdCode::OFFSET_MISALIGNED.0);
    v.push(IdCode::ARGS_INVALID.0);
    v.push(IdCode::BATCH_OVERFLOW.0);
    v.push(IdCode::ARGS_OVERLAP.0);
    v.push(IdCode::NOT_SUPPORTED.0);
    v.push(IdCode::ANNOTATION_MISMATCH.0);
    v.push(IdCode::BRIDGE_INVALID.0);
    v.push(IdCode::RANGE_INVALID.0);
    v.push(MtCode::WEIGHT_OUT_OF_RANGE.0);
    v.push(MtCode::WEIGHT_SUM_EXCEEDS.0);
    v.push(MtCode::UNKNOWN_TARGET.0);
    v.push(MtCode::EMPTY_TRACK.0);
    v.push(MtCode::KEYS_UNORDERED.0);
    v
}

/// 判据侧构造 draw（seq 显式传——提交序是稳定排序的语义前提）。
fn d(seq: u32, pso: u32, mat: u32, index: u32, inst: u32) -> bt::DrawCall {
    bt::DrawCall {
        key: bt::SortKey {
            pso: bt::PsoId(pso),
            mat: bt::MatId(mat),
        },
        index_count: index,
        instance_count: inst,
        seq,
    }
}

/// 越界兜底哨兵（`.get()` 落 `None` 时的替代值——判据必红而非崩）。
const NONE_SEQ: u32 = u32::MAX;
const NONE_U32: u32 = u32::MAX;

/// 取第 `i` 条 draw（零 panic 面：越界落哨兵）。
fn at(v: &[bt::DrawCall], i: usize) -> bt::DrawCall {
    match v.get(i) {
        Some(x) => *x,
        None => d(NONE_SEQ, NONE_U32, NONE_U32, NONE_U32, NONE_U32),
    }
}

/// 判据侧取提交序（越界落哨兵）。
fn seq_at(v: &[bt::DrawCall], i: usize) -> u32 {
    at(v, i).seq
}

/// 交错序输入（排序前 PSO 切换 3 次 / 全键切换 4 次的手算语料）。
fn interleaved() -> Vec<bt::DrawCall> {
    let mut v: Vec<bt::DrawCall> = Vec::new();
    v.push(d(0, 1, 1, 100, 1));
    v.push(d(1, 2, 1, 50, 2));
    v.push(d(2, 1, 1, 30, 1));
    v.push(d(3, 1, 2, 0, 1)); // 空绘制（被剔除）
    v.push(d(4, 2, 1, 20, 1));
    v
}

/// 同 PSO 满编语料（触顶切批手算用——6 条全 PSO=1）。
fn same_pso_run() -> Vec<bt::DrawCall> {
    let mut v: Vec<bt::DrawCall> = Vec::new();
    let mut i = 0u32;
    while i < 6 {
        v.push(d(i, 1, 1, 10, 1));
        i += 1;
    }
    v
}

/// 交替材质语料（同 PSO 单批内材质交替——材质跨度近似记账的反例语料：
/// 按「与首材质不同者递增」记账会虚报为 3，真值恰 2）。
fn multi_mat() -> Vec<bt::DrawCall> {
    let mut v: Vec<bt::DrawCall> = Vec::new();
    v.push(d(0, 1, 1, 10, 1));
    v.push(d(1, 1, 2, 20, 1));
    v.push(d(2, 1, 2, 30, 1));
    v.push(d(3, 1, 1, 40, 1));
    v
}

/// 三策略 seq 序 / 切换数 / 分档成本手算真值（判据侧独立推导，不问被测）：
///
/// | 策略 | seq 序 | PSO 切换 | 全键切换 | 分档成本 |
/// |---|---|---|---|---|
/// | `PsoFirst` | 0,2,3,1,4 | 1 | 2 | 102 |
/// | `MaterialFirst` | 0,2,1,4,3 | 2 | 2 | 201 |
/// | `RarestKeyFirst` | 3,0,2,1,4 | 1 | 2 | 101 |
const REF_RAW_PSO_SWITCH: usize = 3;
const REF_RAW_KEY_SWITCH: usize = 4;
/// 原始序分档成本手算：100+100+1+101 = 302。
const REF_RAW_COST: u32 = 302;
/// 原始序均匀权重成本手算：逐转移「变了几档」1+1+1+2 = 5。
const REF_UNIFORM_RAW_COST: u32 = 5;
/// PsoFirst 排序后均匀权重成本手算：0+1+2+0 = 3。
const REF_UNIFORM_SORTED_COST: u32 = 3;
const REF_PSOFIRST_SEQS: [u32; 5] = [0, 2, 3, 1, 4];
const REF_MATFIRST_SEQS: [u32; 5] = [0, 2, 1, 4, 3];
const REF_RAREST_SEQS: [u32; 5] = [3, 0, 2, 1, 4];
const REF_PSOFIRST_PSO_SWITCH: usize = 1;
const REF_MATFIRST_PSO_SWITCH: usize = 2;
const REF_RAREST_PSO_SWITCH: usize = 1;
const REF_KEY_SWITCH_AFTER: usize = 2;
const REF_PSOFIRST_COST: u32 = 102;
const REF_MATFIRST_COST: u32 = 201;
const REF_RAREST_COST: u32 = 101;
/// 排序后合并语料手算：4 条真实 draw → 2 批；索引 130 / 70；实例 1 / 2。
const REF_MERGE_BATCHES: usize = 2;
const REF_MERGE_B0_INDEX: u32 = 130;
const REF_MERGE_B0_INSTANCE: u32 = 1;
const REF_MERGE_B1_INDEX: u32 = 70;
const REF_MERGE_B1_INSTANCE: u32 = 2;
/// 未排序交错序直接合并：每条真实 draw 各自成批 → 4 批。
const REF_UNSORTED_BATCHES: usize = 4;

/// F1629 绘制调用批处理判据（五条锚点判据映射 53 项）。
pub fn run_vef32_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1629");

    // ================= 一、状态排序（C32-排序-* 13 项） =================

    // 排序-01：稳定排序——同键保 seq 提交序（稳定是语义不是偏好）。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let mut stable = true;
        let mut i = 1usize;
        while i < draws.len() {
            let p = at(&draws, i - 1);
            let c = at(&draws, i);
            if p.key == c.key && p.seq > c.seq {
                stable = false;
            }
            i += 1;
        }
        let pair_ok = match (
            draws.iter().position(|x| x.seq == 0),
            draws.iter().position(|x| x.seq == 2),
        ) {
            (Some(x), Some(y)) => x < y,
            _ => false,
        };
        s.add("C32-排序-01 稳定排序同键保序", stable && pair_ok, "");
    }

    // 排序-02：PsoFirst 字典序全序手算（0,2,3,1,4）。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let hit = (0..5).all(|i| seq_at(&draws, i) == REF_PSOFIRST_SEQS[i]);
        s.add("C32-排序-02 字典序全序手算", hit && draws.len() == 5, "");
    }

    // 排序-03：PSO 切换恰达下限——after == 去重 PSO 数 − 1。
    {
        let mut draws = interleaved();
        let ok = match bt::sort_draws_checked(&mut draws) {
            Ok(g) => {
                g.before == REF_RAW_PSO_SWITCH
                    && g.after == REF_PSOFIRST_PSO_SWITCH
                    && g.distinct_pso == 2
                    && g.at_floor()
            }
            Err(_) => false,
        };
        s.add("C32-排序-03 切换恰达下限", ok, "");
    }

    // 排序-04：收益量化——3 → 1，saved 恰 2（手算对拍）。
    {
        let mut draws = interleaved();
        let ok = match bt::sort_draws_checked(&mut draws) {
            Ok(g) => g.saved() == 2 && g.saved() == g.before - g.after,
            Err(_) => false,
        };
        s.add("C32-排序-04 收益量化对拍", ok, "");
    }

    // 排序-05：非法输入先拒——实例 0 拒 DRAW_INVALID 且不进排序（序不变）。
    {
        let mut bad = interleaved();
        let mut it = bad.iter_mut();
        if let Some(x) = it.nth(2) {
            x.instance_count = 0;
        }
        let before_seq: Vec<u32> = bad.iter().map(|x| x.seq).collect();
        let refused = match bt::sort_draws_checked(&mut bad) {
            Err(c) => c == bt::BtCode::DRAW_INVALID,
            Ok(_) => false,
        };
        let after_seq: Vec<u32> = bad.iter().map(|x| x.seq).collect();
        s.add(
            "C32-排序-05 非法先拒不进排序",
            refused && before_seq == after_seq,
            "",
        );
    }

    // 排序-06：seq 互异双向——重复拒、合法过。
    {
        let mut dup = interleaved();
        if let Some(x) = dup.get_mut(3) {
            x.seq = 0;
        }
        let dup_refused = match bt::validate_batch(&dup) {
            Err(c) => c == bt::BtCode::DRAW_INVALID,
            Ok(()) => false,
        };
        let ok = interleaved();
        let ok_passed = bt::validate_batch(&ok).is_ok();
        s.add("C32-排序-06 seq互异双向", dup_refused && ok_passed, "");
    }

    // 排序-07：恰边界——空输入与单 draw 排序无副作用。
    {
        let mut empty: Vec<bt::DrawCall> = Vec::new();
        let g0 = bt::sort_draws_checked(&mut empty);
        let mut one: Vec<bt::DrawCall> = Vec::new();
        one.push(d(7, 3, 3, 12, 1));
        let g1 = bt::sort_draws_checked(&mut one);
        let ok = match (g0, g1) {
            (Ok(a), Ok(b)) => {
                a.before == 0 && a.after == 0 && a.at_floor() && b.before == 0 && b.after == 0
            }
            _ => false,
        };
        s.add("C32-排序-07 空与单恰边界", ok, "");
    }

    // 排序-08：材质绑定切换量化——全键切换 4 → 2（第二成本面同样入册）。
    {
        let raw = interleaved();
        let mut draws = interleaved();
        let before_keys = bt::count_key_switches(&raw);
        let _ = bt::sort_draws_checked(&mut draws);
        let after_keys = bt::count_key_switches(&draws);
        s.add(
            "C32-排序-08 全键切换量化",
            before_keys == REF_RAW_KEY_SWITCH
                && after_keys == REF_KEY_SWITCH_AFTER
                && bt::count_switches(&raw) == REF_RAW_PSO_SWITCH,
            "",
        );
    }

    // 排序-09：置换不变量双向——排序前后同指纹同总量过；篡改一条必拒。
    {
        let before = interleaved();
        let mut after = interleaved();
        let sorted_ok = bt::sort_draws_checked(&mut after).is_ok();
        let good = sorted_ok && bt::verify_sort_preserves(&before, &after).is_ok();
        let tampered = interleaved();
        let mut tampered_sorted = interleaved();
        let _ = bt::sort_draws_checked(&mut tampered_sorted);
        if let Some(x) = tampered_sorted.get_mut(4) {
            x.index_count = x.index_count.saturating_add(1);
        }
        let bad = match bt::verify_sort_preserves(&tampered, &tampered_sorted) {
            Err(c) => c == bt::BtCode::SORT_VIOLATION,
            Ok(()) => false,
        };
        s.add("C32-排序-09 置换不变量双向", good && bad, "");
    }

    // 排序-10：稳定保序的实现面自检反恒假——同键 seq 人为倒置必拒。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let good = bt::verify_sorted(&draws).is_ok();
        // 把 seq0 改成 9（同键的 seq2 之前落 9）：键序仍合法但提交序已失守。
        let mut i = 0usize;
        while i < draws.len() {
            if seq_at(&draws, i) == 0 {
                if let Some(x) = draws.get_mut(i) {
                    x.seq = 9;
                }
            }
            i += 1;
        }
        let bad = match bt::verify_sorted(&draws) {
            Err(c) => c == bt::BtCode::SORT_VIOLATION,
            Ok(()) => false,
        };
        s.add("C32-排序-10 保序自检反恒假", good && bad, "");
    }

    // 排序-11：策略族封闭——三态与常量同源、命名非空互异、两策略结论相反。
    {
        let all = bt::SortStrategy::ALL;
        let mut names_ok = true;
        let mut i = 0usize;
        while i < all.len() {
            let a = all[i].name();
            if a.is_empty() {
                names_ok = false;
            }
            let mut j = i + 1;
            while j < all.len() {
                if all[j].name() == a {
                    names_ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        let a = d(0, 1, 2, 10, 1);
        let b = d(1, 2, 1, 10, 1);
        // PsoFirst 看 PSO 头位（1<2 → Less），MaterialFirst 看材质头位
        // （2>1 → Greater）——同一对键两策略结论必须相反，否则「策略族」
        // 是三个名字一个行为。
        let cmp_ok = bt::SortStrategy::PsoFirst.cmp_keys(&a.key, &b.key)
            == core::cmp::Ordering::Less
            && bt::SortStrategy::MaterialFirst.cmp_keys(&a.key, &b.key)
                == core::cmp::Ordering::Greater;
        s.add(
            "C32-排序-11 策略族封闭",
            bt::STRATEGY_COUNT == REF_STRATEGY_COUNT
                && all.len() == REF_STRATEGY_COUNT
                && names_ok
                && cmp_ok,
            "",
        );
    }

    // 排序-12：三策略 seq 序与 PSO 切换数手算。
    {
        let mut a = interleaved();
        let _ = bt::sort_draws_checked_with(&mut a, bt::SortStrategy::PsoFirst);
        let mut b = interleaved();
        let _ = bt::sort_draws_checked_with(&mut b, bt::SortStrategy::MaterialFirst);
        let mut c = interleaved();
        let _ = bt::sort_draws_checked_with(&mut c, bt::SortStrategy::RarestKeyFirst);
        let seqs_a: Vec<u32> = (0..5).map(|i| seq_at(&a, i)).collect();
        let seqs_b: Vec<u32> = (0..5).map(|i| seq_at(&b, i)).collect();
        let seqs_c: Vec<u32> = (0..5).map(|i| seq_at(&c, i)).collect();
        s.add(
            "C32-排序-12 三策略切换手算",
            seqs_a == REF_PSOFIRST_SEQS.to_vec()
                && seqs_b == REF_MATFIRST_SEQS.to_vec()
                && seqs_c == REF_RAREST_SEQS.to_vec()
                && bt::count_switches(&a) == REF_PSOFIRST_PSO_SWITCH
                && bt::count_switches(&b) == REF_MATFIRST_PSO_SWITCH
                && bt::count_switches(&c) == REF_RAREST_PSO_SWITCH
                && bt::verify_order(&b, bt::SortStrategy::MaterialFirst).is_ok()
                && bt::verify_order(&c, bt::SortStrategy::RarestKeyFirst).is_ok(),
            "",
        );
    }

    // 排序-13：at_floor 适用范围——只对 PsoFirst 成立（拿错尺子量错东西）。
    {
        let mut b = interleaved();
        let mat = bt::sort_draws_checked_with(&mut b, bt::SortStrategy::MaterialFirst);
        let mut c = interleaved();
        let rar = bt::sort_draws_checked_with(&mut c, bt::SortStrategy::RarestKeyFirst);
        let ok = match (mat, rar) {
            (Ok(m), Ok(r)) => {
                !m.at_floor()
                    && !r.at_floor()
                    && m.strategy == bt::SortStrategy::MaterialFirst
                    && r.strategy == bt::SortStrategy::RarestKeyFirst
            }
            _ => false,
        };
        s.add("C32-排序-13 at_floor适用范围", ok, "");
    }

    // ================= 二、切换成本（C32-成本-* 4 项） =================

    // 成本-01：分档权重——默认 PSO 档严格大于材质档；均匀档退化不成立。
    {
        let d = bt::SwitchCost::default_cost();
        let u = bt::SwitchCost::uniform();
        s.add(
            "C32-成本-01 分档权重成立",
            d.tiered() && !u.tiered() && d.pso > d.mat,
            "",
        );
    }

    // 成本-02：分档成本手算 302（100+100+1+101）。
    {
        let raw = interleaved();
        s.add(
            "C32-成本-02 分档成本手算",
            bt::switch_cost(&raw, &bt::SwitchCost::default_cost()) == REF_RAW_COST,
            "",
        );
    }

    // 成本-03：三策略排序后成本手算 102 / 201 / 101（策略选择真影响成本）。
    {
        let mut a = interleaved();
        let ga = bt::sort_draws_checked_with(&mut a, bt::SortStrategy::PsoFirst);
        let mut b = interleaved();
        let gb = bt::sort_draws_checked_with(&mut b, bt::SortStrategy::MaterialFirst);
        let mut c = interleaved();
        let gc = bt::sort_draws_checked_with(&mut c, bt::SortStrategy::RarestKeyFirst);
        let ok = match (ga, gb, gc) {
            (Ok(x), Ok(y), Ok(z)) => {
                x.cost.after == REF_PSOFIRST_COST
                    && y.cost.after == REF_MATFIRST_COST
                    && z.cost.after == REF_RAREST_COST
                    && x.cost.before == REF_RAW_COST
                    && x.cost.saved() == REF_RAW_COST - REF_PSOFIRST_COST
                    && !x.cost.regressed()
            }
            _ => false,
        };
        s.add("C32-成本-03 三策略成本手算", ok, "");
    }

    // 成本-04：均匀权重退化对拍（逐转移「变了几档」）——与分档 302 互证
    // 分档记账没把两档混算。
    {
        let raw = interleaved();
        let uniform = bt::switch_cost(&raw, &bt::SwitchCost::uniform());
        let mut a = interleaved();
        let _ = bt::sort_draws_checked(&mut a);
        let uniform_after = bt::switch_cost(&a, &bt::SwitchCost::uniform());
        s.add(
            "C32-成本-04 均匀权重退化对拍",
            uniform == REF_UNIFORM_RAW_COST && uniform_after == REF_UNIFORM_SORTED_COST,
            "",
        );
    }

    // ================= 三、合并执行器（C32-合并-* 12 项） =================

    // 合并-01：同 PSO 合并成批——排序后 5 draw（4 真实）→ 2 批。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, _)) => {
                b.len() == REF_MERGE_BATCHES
                    && b.first().map(|x| x.pso) == Some(bt::PsoId(1))
                    && b.get(1).map(|x| x.pso) == Some(bt::PsoId(2))
            }
            Err(_) => false,
        };
        s.add("C32-合并-01 同PSO合并成批", ok, "");
    }

    // 合并-02：跨 PSO 不硬合——未排序交错序下每条真实 draw 各自成批（4 批）。
    {
        let draws = interleaved();
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, _)) => b.len() == REF_UNSORTED_BATCHES,
            Err(_) => false,
        };
        s.add("C32-合并-02 跨PSO不硬合", ok, "");
    }

    // 合并-03：空绘制跳过不进批——skipped 恰 1 且批内无 seq3。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, g)) => {
                let no_empty = b.iter().all(|x| !x.members.iter().any(|m| *m == 3));
                g.skipped_empty == 1
                    && no_empty
                    && g.before == 5
                    && g.after == REF_MERGE_BATCHES
                    && g.splits == 0
            }
            Err(_) => false,
        };
        s.add("C32-合并-03 空绘制跳过", ok, "");
    }

    // 合并-04：收益量化对拍——before 5 → after 2，saved 恰 3（手算）。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((_, g)) => g.saved() == 3 && g.before - g.after == 3,
            Err(_) => false,
        };
        s.add("C32-合并-04 收益量化对拍", ok, "");
    }

    // 合并-05：成员保序——批内 seq 严格升序（零 panic 面：走 `.get()`）。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, _)) => {
                let mut ordered = true;
                let mut i = 0usize;
                while i < b.len() {
                    let mut j = 1usize;
                    while j < b[i].members.len() {
                        let p = match b[i].members.get(j - 1) {
                            Some(x) => *x,
                            None => {
                                ordered = false;
                                break;
                            }
                        };
                        let c = match b[i].members.get(j) {
                            Some(x) => *x,
                            None => {
                                ordered = false;
                                break;
                            }
                        };
                        if p >= c {
                            ordered = false;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                ordered
            }
            Err(_) => false,
        };
        s.add("C32-合并-05 成员保序", ok, "");
    }

    // 合并-06：合计与容量携带——PSO1 批索引 130 实例 1；PSO2 批 70 实例 2。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, _)) => {
                let b0 = b.iter().find(|x| x.pso == bt::PsoId(1));
                let b1 = b.iter().find(|x| x.pso == bt::PsoId(2));
                match (b0, b1) {
                    (Some(x), Some(y)) => {
                        x.total_index == REF_MERGE_B0_INDEX
                            && x.max_instance == REF_MERGE_B0_INSTANCE
                            && y.total_index == REF_MERGE_B1_INDEX
                            && y.max_instance == REF_MERGE_B1_INSTANCE
                    }
                    _ => false,
                }
            }
            Err(_) => false,
        };
        s.add("C32-合并-06 合计与容量携带", ok, "");
    }

    // 合并-07：实例容量守卫双向——恰等过、放大拒 INSTANCE_FORBIDDEN。
    {
        let equal = bt::guard_merge_instance(2, 2).is_ok();
        let scaled = match bt::guard_merge_instance(2, 4) {
            Err(c) => c == bt::BtCode::INSTANCE_FORBIDDEN,
            Ok(()) => false,
        };
        s.add("C32-合并-07 实例容量守卫双向", equal && scaled, "");
    }

    // 合并-08：合并自检反恒假——合法批过，伪造乱序批拒 MERGE_VIOLATION。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, _)) => {
                let good = bt::verify_merged(&b, 2).is_ok();
                let mut tampered = b;
                if let Some(x) = tampered.get_mut(0) {
                    x.members = vec![2, 0];
                }
                let bad = match bt::verify_merged(&tampered, 2) {
                    Err(c) => c == bt::BtCode::MERGE_VIOLATION,
                    Ok(()) => false,
                };
                good && bad
            }
            Err(_) => false,
        };
        s.add("C32-合并-08 合并自检反恒假", ok, "");
    }

    // 合并-09：触顶切批——6 条同 PSO，cap=2 → 3 批 2 切批（手算）。
    {
        let run = same_pso_run();
        let ok = match bt::merge_with_gain_capped(&run, 2) {
            Ok((b, g)) => {
                b.len() == 3
                    && g.splits == 2
                    && g.before == 6
                    && g.after == 3
                    && b.iter().all(|x| x.len() == 2)
                    && bt::verify_merged_with(&b, 1, 2).is_ok()
            }
            Err(_) => false,
        };
        s.add("C32-合并-09 触顶切批", ok, "");
    }

    // 合并-10：容量上限非法双向——cap=0 拒 CAP_INVALID；cap=1 每 draw 一批。
    {
        let zero = match bt::merge_with_gain_capped(&same_pso_run(), 0) {
            Err(c) => c == bt::BtCode::CAP_INVALID,
            Ok(_) => false,
        };
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let one_cap = match bt::merge_with_gain_capped(&draws, 1) {
            Ok((b, g)) => b.len() == 4 && g.splits == 2 && g.skipped_empty == 1,
            Err(_) => false,
        };
        s.add("C32-合并-10 容量上限非法双向", zero && one_cap, "");
    }

    // 合并-11：材质跨度双向对拍——交替材质跨度恰 2（首见序），篡改必拒。
    //
    // 交替材质（mat1,mat2,mat2,mat1）是「按首材质不同者递增」的近似记账
    // 会虚报为 3 的反例语料——若实现退回近似记账，此条立刻转红。
    {
        let mut multi = multi_mat();
        let _ = bt::sort_draws_checked(&mut multi);
        let ok = match bt::merge_with_gain(&multi) {
            Ok((b, _)) => {
                let good = b.len() == 1
                    && b.first().map(|x| x.distinct_mat()) == Some(2)
                    && b.first().map(|x| x.first_mat()) == Some(Some(bt::MatId(1)))
                    && bt::verify_material_span(&b, &multi).is_ok();
                let mut tampered = b;
                if let Some(x) = tampered.get_mut(0) {
                    x.mats = vec![bt::MatId(9)];
                }
                let bad = match bt::verify_material_span(&tampered, &multi) {
                    Err(c) => c == bt::BtCode::MERGE_VIOLATION,
                    Ok(()) => false,
                };
                good && bad
            }
            Err(_) => false,
        };
        s.add("C32-合并-11 材质跨度双向", ok, "");
    }

    // 合并-12：留下本可合并的同 PSO 相邻批必拒（未触顶即失守）。
    {
        let mut draws = interleaved();
        let _ = bt::sort_draws_checked(&mut draws);
        let ok = match bt::merge_with_gain(&draws) {
            Ok((b, _)) => {
                let good = bt::verify_merged(&b, 2).is_ok();
                // 伪造：两批都改成 PSO=1 且各只带一个成员（未触顶）。
                let mut forged = b;
                let mut i = 0usize;
                while i < forged.len() {
                    if let Some(x) = forged.get_mut(i) {
                        x.pso = bt::PsoId(1);
                        x.members = vec![i as u32];
                    }
                    i += 1;
                }
                let bad = match bt::verify_merged(&forged, 2) {
                    Err(c) => c == bt::BtCode::MERGE_VIOLATION,
                    Ok(()) => false,
                };
                good && bad
            }
            Err(_) => false,
        };
        s.add("C32-合并-12 可合并相邻批必拒", ok, "");
    }

    // ================= 四、帧预算（C32-预算-* 8 项） =================

    // 预算-01：恰预算判内——等于上限不越界（恰边界）。
    {
        let ok = match bt::FrameBudget::new(4) {
            Ok(b) => {
                b.judge(4) == bt::BudgetVerdict::Within && bt::budget_alarm(4, &b).is_none()
            }
            Err(_) => false,
        };
        s.add("C32-预算-01 恰预算判内", ok, "");
    }

    // 预算-02：超 1 恰边界告警——over_by 恰 1。
    {
        let alarmed = match bt::FrameBudget::new(4) {
            Ok(b) => match bt::budget_alarm(5, &b) {
                Some(a) => a.over_by == 1 && a.calls == 5 && a.limit == 4,
                None => false,
            },
            Err(_) => false,
        };
        s.add("C32-预算-02 超1恰边界告警", alarmed, "");
    }

    // 预算-03：大超精确——over_by 手算（limit 10, calls 1_500_010）。
    {
        let over = match bt::FrameBudget::new(10) {
            Ok(b) => match b.judge(1_500_010) {
                bt::BudgetVerdict::Over { over_by, .. } => over_by == 1_500_000,
                bt::BudgetVerdict::Within => false,
            },
            Err(_) => false,
        };
        s.add("C32-预算-03 大超精确", over, "");
    }

    // 预算-04：预算非法双向——0 与 MAX+1 都拒，恰 MAX 放行。
    {
        let zero = match bt::FrameBudget::new(0) {
            Err(c) => c == bt::BtCode::BUDGET_INVALID,
            Ok(_) => false,
        };
        let too_big = match bt::FrameBudget::new(REF_MAX_BUDGET + 1) {
            Err(c) => c == bt::BtCode::BUDGET_INVALID,
            Ok(_) => false,
        };
        let edge_ok = bt::FrameBudget::new(REF_MAX_BUDGET).is_ok();
        s.add("C32-预算-04 预算非法双向", zero && too_big && edge_ok, "");
    }

    // 预算-05：默认预算域内冻结——2000 恰边界判内。
    {
        let b = bt::FrameBudget::default_budget();
        s.add(
            "C32-预算-05 默认预算恰边界",
            b.max_calls == REF_DEFAULT_BUDGET
                && b.judge(REF_DEFAULT_BUDGET as usize) == bt::BudgetVerdict::Within,
            "",
        );
    }

    // 预算-06：告警文案是信号不是闸——建议必含「不拒绘」「合并」口径。
    {
        let ok = match bt::FrameBudget::new(1) {
            Ok(b) => match bt::budget_alarm(2, &b) {
                Some(a) => a.advice.contains("不拒绘") && a.advice.contains("合并"),
                None => false,
            },
            Err(_) => false,
        };
        s.add("C32-预算-06 告警是信号非闸", ok, "");
    }

    // 预算-07：分档判定——limit 10：10 内 / 11 刚超 / 19 刚超 / 20 倍档
    // （恰 2× 边界两面翻）。
    {
        let ok = match bt::FrameBudget::new(10) {
            Ok(b) => {
                let within = b.judge(10) == bt::BudgetVerdict::Within;
                let just = b.judge(11)
                    == bt::BudgetVerdict::Over {
                        over_by: 1,
                        severity: bt::BudgetSeverity::Over,
                    };
                let near = b.judge(19)
                    == bt::BudgetVerdict::Over {
                        over_by: 9,
                        severity: bt::BudgetSeverity::Over,
                    };
                let severe = b.judge(20)
                    == bt::BudgetVerdict::Over {
                        over_by: 10,
                        severity: bt::BudgetSeverity::Severe,
                    };
                within && just && near && severe
            }
            Err(_) => false,
        };
        s.add("C32-预算-07 分档判定", ok, "");
    }

    // 预算-08：headroom 饱和减——不越界给余量，恰好与超量皆 0（不回绕）。
    {
        let ok = match bt::FrameBudget::new(10) {
            Ok(b) => b.headroom(3) == 7 && b.headroom(10) == 0 && b.headroom(99) == 0,
            Err(_) => false,
        };
        s.add("C32-预算-08 headroom饱和减", ok, "");
    }

    // ================= 五、边界声明（C32-边界-* 6 项） =================

    // 边界-01：声明校验正路径过。
    {
        s.add("C32-边界-01 边界声明正路径", bt::verify_boundary().is_ok(), "");
    }

    // 边界-02：I04 关键词判据侧写死对拍（三条关键词逐一在案）。
    {
        let all_in = REF_I04_KEYWORDS
            .iter()
            .all(|k| bt::I04_INSTANCE_BOUNDARY.contains(k));
        s.add("C32-边界-02 I04关键词对拍", all_in, "");
    }

    // 边界-03：非实例合进口径 + 预算信号口径（F1416 与不拒绘两词）。
    {
        let non_instance = bt::I04_INSTANCE_BOUNDARY.contains("非实例合并");
        let budget_ver =
            bt::BUDGET_SIGNAL_NOTICE.contains("F1416") && bt::BUDGET_SIGNAL_NOTICE.contains("不拒绘");
        s.add("C32-边界-03 非实例合并与预算口径", non_instance && budget_ver, "");
    }

    // 边界-04：合并不碰实例语义声明与 I04 同源（跨声明关键词闭环）。
    {
        let cross = bt::MERGE_NOT_TOUCH_INSTANCE.contains("实例")
            && bt::MERGE_NOT_TOUCH_INSTANCE.contains("I04")
            && bt::MERGE_NOT_TOUCH_INSTANCE.contains("不拆不合");
        s.add("C32-边界-04 实例语义不碰闭环", cross, "");
    }

    // 边界-05：材质跨度承载声明——描述符偏移与退回分批两词齐备。
    {
        let ok = bt::MERGE_MATERIAL_NOTICE.contains("描述符偏移")
            && bt::MERGE_MATERIAL_NOTICE.contains("退回分批")
            && bt::MERGE_MATERIAL_NOTICE.contains("不增 draw call");
        s.add("C32-边界-05 材质跨度承载声明", ok, "");
    }

    // 边界-06：容量上限声明——切批不是收益 + F1628 同源。
    {
        let ok = bt::BATCH_CAP_NOTICE.contains("切批不是合并收益")
            && bt::BATCH_CAP_NOTICE.contains("F1628")
            && bt::BATCH_CAP_NOTICE.contains("切批数入册");
        s.add("C32-边界-06 容量上限声明", ok, "");
    }

    // ================= 六、判据自检与主管线（C32-判据-* 10 项） =================

    // 判据-01：主管线端到端——混合输入全量化字段手算对拍。
    {
        let mut draws = interleaved();
        let b = bt::FrameBudget::default_budget();
        let ok = match bt::run_batching(&mut draws, &b) {
            Ok(rep) => {
                rep.sort.before == REF_RAW_PSO_SWITCH
                    && rep.sort.after == REF_PSOFIRST_PSO_SWITCH
                    && rep.saved_switches() == 2
                    && rep.sort.key_before == REF_RAW_KEY_SWITCH
                    && rep.sort.key_after == REF_KEY_SWITCH_AFTER
                    && rep.sort.saved_keys() == 2
                    && rep.merge.before == 5
                    && rep.merge.after == REF_MERGE_BATCHES
                    && rep.saved_calls() == 3
                    && rep.merge.skipped_empty == 1
                    && rep.merge.splits == 0
                    && rep.sort.cost.before == REF_RAW_COST
                    && rep.sort.cost.after == REF_PSOFIRST_COST
                    && rep.saved_cost() == REF_RAW_COST - REF_PSOFIRST_COST
                    && rep.verdict == bt::BudgetVerdict::Within
                    && rep.alarm.is_none()
                    && rep.batches.len() == REF_MERGE_BATCHES
                    && bt::pso_span(&rep.batches) == 2
            }
            Err(_) => false,
        };
        s.add("C32-判据-01 主管线端到端", ok, "");
    }

    // 判据-02：主管线超预算路径——限额 1 恰使 2 批超限告警在案。
    {
        let tiny = bt::FrameBudget::new(1);
        let ok = match tiny {
            Ok(tiny) => {
                let mut draws = interleaved();
                match bt::run_batching(&mut draws, &tiny) {
                    Ok(rep) => {
                        let alarmed = match &rep.alarm {
                            Some(a) => a.over_by == 1 && a.calls == REF_MERGE_BATCHES,
                            None => false,
                        };
                        let over = match rep.verdict {
                            bt::BudgetVerdict::Over {
                                over_by,
                                severity,
                            } => over_by == 1 && severity == bt::BudgetSeverity::Severe,
                            bt::BudgetVerdict::Within => false,
                        };
                        alarmed && over
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };
        s.add("C32-判据-02 主管线超预算路径", ok, "");
    }

    // 判据-03：呈现文本五面透明（切换/材质绑定/调用/成本/预算）。
    {
        let mut draws = interleaved();
        let b = bt::FrameBudget::default_budget();
        let ok = match bt::run_batching(&mut draws, &b) {
            Ok(rep) => {
                let text = rep.present();
                text.contains("PSO 切换 3→1（-2）")
                    && text.contains("全键切换 4→2（-2）")
                    && text.contains("draw call 5→2（-3")
                    && text.contains("切换成本 302→102（-200）")
                    && text.contains("预算内")
            }
            Err(_) => false,
        };
        s.add("C32-判据-03 呈现文本透明", ok, "");
    }

    // 判据-04：版本在案。
    {
        s.add("C32-判据-04 版本在案", bt::BATCH_VERSION == REF_VERSION, "");
    }

    // 判据-05：诊断码段独占——高字节全等 0x4E，且与邻域模块真调码两两
    // 不等（邻域码真调不写死，邻域换码本判据随之改判而非恒绿）。
    {
        let codes = bt::BtCode::all();
        let neigh = neighbour_codes();
        let section_ok = codes.iter().all(|c| (c.code() >> 8) == 0x4E);
        let mut no_clash = true;
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = 0usize;
            while j < neigh.len() {
                if codes[i].code() == neigh[j] {
                    no_clash = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("C32-判据-05 诊断码段独占", section_ok && no_clash, "");
    }

    // 判据-06：码值判据侧写死对拍（wire 值逐一字面量，条数同源）。
    {
        let codes = bt::BtCode::all();
        let ok = codes.len() == REF_CODE_COUNT
            && codes.first().map(|c| c.code()) == Some(REF_DRAW_INVALID)
            && codes.get(1).map(|c| c.code()) == Some(REF_SORT_VIOLATION)
            && codes.get(2).map(|c| c.code()) == Some(REF_MERGE_VIOLATION)
            && codes.get(3).map(|c| c.code()) == Some(REF_BUDGET_INVALID)
            && codes.get(4).map(|c| c.code()) == Some(REF_BOUNDARY_MISMATCH)
            && codes.get(5).map(|c| c.code()) == Some(REF_INSTANCE_FORBIDDEN)
            && codes.get(6).map(|c| c.code()) == Some(REF_CAP_INVALID);
        s.add("C32-判据-06 码值写死对拍", ok, "");
    }

    // 判据-07：码互异 + say 人话非空（互斥防溯源串码）。
    {
        let codes = bt::BtCode::all();
        let mut distinct = true;
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        let say_ok = codes.iter().all(|c| !c.say().is_empty());
        s.add("C32-判据-07 码互异人话非空", distinct && say_ok, "");
    }

    // 判据-08：双策略端到端——策略在主管线里真生效（两策略切换数不同）。
    {
        let b1 = bt::FrameBudget::default_budget();
        let mut a = interleaved();
        let ra = bt::run_batching(&mut a, &b1);
        let mut b = interleaved();
        let rb = bt::run_batching_with(&mut b, bt::SortStrategy::MaterialFirst, &b1);
        let ok = match (ra, rb) {
            (Ok(x), Ok(y)) => {
                x.sort.after == REF_PSOFIRST_PSO_SWITCH
                    && y.sort.after == REF_MATFIRST_PSO_SWITCH
                    && x.sort.after != y.sort.after
                    && y.alarm.is_none()
            }
            _ => false,
        };
        s.add("C32-判据-08 双策略端到端", ok, "");
    }

    // 判据-09：主管线脏输入拦截——dup seq 与实例 0 各自被拦且不改序。
    {
        let mut dup = interleaved();
        if let Some(x) = dup.get_mut(3) {
            x.seq = 0;
        }
        let mut zero_inst = interleaved();
        if let Some(x) = zero_inst.get_mut(1) {
            x.instance_count = 0;
        }
        let b = bt::FrameBudget::default_budget();
        let seqs_before: Vec<u32> = dup.iter().map(|x| x.seq).collect();
        let seqs_after: Vec<u32> = zero_inst.iter().map(|x| x.seq).collect();
        let refused = match (
            bt::run_batching(&mut dup, &b),
            bt::run_batching(&mut zero_inst, &b),
        ) {
            (Err(a), Err(c)) => {
                a == bt::BtCode::DRAW_INVALID && c == bt::BtCode::DRAW_INVALID
            }
            _ => false,
        };
        let untouched: Vec<u32> = dup.iter().map(|x| x.seq).collect();
        let untouched2: Vec<u32> = zero_inst.iter().map(|x| x.seq).collect();
        s.add(
            "C32-判据-09 主管线脏输入拦截",
            refused && seqs_before == untouched && seqs_after == untouched2,
            "",
        );
    }

    // 判据-10：条数对账（放末位——此时加自身恰为 REF_CHECK_TOTAL）。
    {
        let now = s.len() + 1;
        s.add(
            "C32-判据-10 条数对账",
            now == REF_CHECK_TOTAL,
            "条数漂移：判据增删须同步更新 REF_CHECK_TOTAL",
        );
    }

    s
}

#[cfg(test)]
mod tmp_isolated_verify {
    use super::*;

    /// 临时隔离验证（一次性，验完即删）：只跑本域判据，全绿才放行。
    #[test]
    fn vef32_isolated_all_green() {
        let set = run_vef32_checks();
        let (items, n) = set.red_items();
        let mut reds: Vec<&str> = Vec::new();
        let mut i = 0usize;
        while i < n {
            if let Some(c) = items.get(i) {
                match c {
                    Some(x) if !x.passed => reds.push(x.name),
                    _ => {}
                }
            }
            i += 1;
        }
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed() && !set.truncated(),
            "vef32 红项 {:?}（{}/{} 绿）",
            reds,
            passed,
            passed + failed
        );
    }
}