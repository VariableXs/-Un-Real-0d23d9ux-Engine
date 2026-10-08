//! VE-F1008 · 域自检（判据逐条对应，见 `vef10_pngopts.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 选项全组合 roundtrip（合法性矩阵内全测）→ `C1008-矩阵-*`
//! - 压缩比对比表 → `C1008-权衡-*`
//! - 快速模式收益 → `C1008-快速-*`
//! - 非法组合拒绝 → `C1008-拒绝-*`
//! - 元数据部分成功 / 门禁 → `C1008-元数据-*`、`C1008-门禁-*`
//!
//! 弱门禁自律（逐条对照本域最容易犯的五种）：
//! 1. **「矩阵全测」不能只测几个代表格**：只断「灰度 8 位合法、真彩 1 位非法」
//!    的话，一个只填了两格的矩阵会全绿。故**遍历全部25 格**逐格裁决，
//!    并断「格子总数 == 25」「合法+降档+非法 == 25」（三者相加，覆盖无空格）。
//! 2. **「压缩比对比表」不能只断「有9 行」**：一个九行全同值的常数表能全绿。
//!    故断**双向单调**（级别↑ ⇒ 体积比不增 且 耗时比不减）**加端点不等**
//!    （首末档体积比必须不同）——常数表会在端点不等上变红。
//! 3. **「快速模式 −40%」不能靠注释**：no_std 内核拿不到墙钟，故断的是
//!    **可机检的结构事实**：开快速 ⇒ 试探次数恒 1（不随行数放大）；
//!    关快速 ⇒ 试探次数 == 行数；**且体积代价 > 0**（不可能又省时又省体积，
//!    一个「快速模式还更小」的实现会在这里变红）。
//! 4. **「拒绝给最近合法组合」不能靠「返回了某个建议」**：一个「永远返回第一个
//!    合法格」的实现能通过「建议非空」这条弱判据。故断**恰近**：拿两个非法格，
//!    验证建议的曼哈顿距离**逐个独立算出并相等**，且该距离是**最小可达**。
//! 5. **「部分成功」不能靠「有 dropped」**：整单失败（kept 为空）也满足
//!    「有 dropped」。故要求**kept 与 dropped 同时非空**（真部分成功），
//!    并断 `kept + dropped == 提交数`（**恰等于**，用 `==` 不用 `>=`）。

#![allow(clippy::needless_range_loop)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vef02_pngenc::ColorType;
use super::vef10_pngopts::*;

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

/// 合法组合全集（矩阵内裁决为 `Legal` 的格子），供「全测」用。
fn legal_pairs(m: &LegalMatrix) -> Vec<(ColorType, u8)> {
    let mut out = Vec::new();
    let mut ti = 0usize;
    while ti < COLOR_TYPES.len() {
        let mut di = 0usize;
        while di < DEPTHS.len() {
            if m.cell(ti, di) == Some(CellVerdict::Legal) {
                out.push((COLOR_TYPES[ti], DEPTHS[di]));
            }
            di += 1;
        }
        ti += 1;
    }
    out
}

/// F1008 域自检入口。
pub fn run_vef10_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-f-f1008");
    let m = LegalMatrix::new();

    // ==================== 判据一：合法性矩阵 / 全组合 roundtrip ====================
    {
        s.add(
            "C1008-矩阵-格子总数为二十五",
            m.cell_count() == 25,
            "5 颜色类型 × 5 位深 = 25 格，表驱动要求每格有裁决",
        );
        // 遍历全表：三类裁决相加恰为总数 ⇒ 无空格、无第四类。
        let mut legal = 0usize;
        let mut downgrade = 0usize;
        let mut illegal = 0usize;
        let mut all_reachable = true;
        let mut ti = 0usize;
        while ti < COLOR_TYPES.len() {
            let mut di = 0usize;
            while di < DEPTHS.len() {
                match m.cell(ti, di) {
                    Some(CellVerdict::Legal) => legal += 1,
                    Some(CellVerdict::LegalByDowngrade) => downgrade += 1,
                    Some(CellVerdict::IllegalNoAlt) => illegal += 1,
                    None => all_reachable = false,
                }
                di += 1;
            }
            ti += 1;
        }
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
        // 全组合 roundtrip：每个合法格都能通过校验（这是「全测」的核心）。
        let pairs = legal_pairs(&m);
        let mut all_pass = true;
        let mut pi = 0usize;
        while pi < pairs.len() {
            let o = WriteOptions {
                force_color: Some(pairs[pi].0),
                force_depth: Some(pairs[pi].1),
                ..WriteOptions::default_options()
            };
            if validate(&m, &o).is_err() {
                all_pass = false;
            }
            pi += 1;
        }
        s.add(
            "C1008-矩阵-全部合法格均可通过校验",
            all_pass && !pairs.is_empty(),
            "合法格逐个 roundtrip：矩阵说合法，校验器就不能拒",
        );
        s.add(
            "C1008-矩阵-合法格数与F1002口径一致",
            m.legal_cells() == 5 + 2 + 4 + 2 + 2,
            "灰度5 +真彩2+调色板4+灰A2+真彩A2 = 15，与上游 legal_depths 同口径",
        );
        // 负向：非法格确实被校验器拒（不能只有正向）。
        let mut illegal_all_rejected = true;
        let mut ic = 0usize;
        while ic < COLOR_TYPES.len() {
            let mut id = 0usize;
            while id < DEPTHS.len() {
                if m.cell(ic, id) == Some(CellVerdict::IllegalNoAlt) {
                    let o = WriteOptions {
                        force_color: Some(COLOR_TYPES[ic]),
                        force_depth: Some(DEPTHS[id]),
                        allow_downgrade: false,
                        ..WriteOptions::default_options()
                    };
                    if validate(&m, &o).is_ok() {
                        illegal_all_rejected = false;
                    }
                }
                id += 1;
            }
            ic += 1;
        }
        s.add(
            "C1008-矩阵-非法格逐个被拒",
            illegal_all_rejected,
            "非法格逐个 roundtrip：矩阵说非法，校验器就不能放",
        );
        // 降档格：允许降档则过、不允许则拒（两个方向都要断）。
        let mut downgrade_semantics_ok = false;
        let mut dc = 0usize;
        while dc < COLOR_TYPES.len() {
            let mut dd = 0usize;
            while dd < DEPTHS.len() {
                if m.cell(dc, dd) == Some(CellVerdict::LegalByDowngrade) {
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
                    if validate(&m, &allow).is_ok() && validate(&m, &deny).is_err() {
                        downgrade_semantics_ok = true;
                    }
                }
                dd += 1;
            }
            dc += 1;
        }
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
        //逐格裁决 vs 规则函数独立重算（补计数判据的盲区）。
        // 计数判据只钉「降档格数>0」，改 `d<8`→`d<4` 会让一部分降档格
        // 变成非法格而三个计数与总和全不变 ⇒ 计数判据全绿。
        // 这里按规则文字在判据侧**重算**每格应有裁决，逐格比对。
        let mut rule_ok = true;
        let mut ri = 0usize;
        while ri < COLOR_TYPES.len() {
            let ct = COLOR_TYPES[ri];
            let mut rj = 0usize;
            while rj < DEPTHS.len() {
                let d = DEPTHS[rj];
                // 规则：上游legal_depths 命中 ⇒ Legal；真彩类且 d<8 ⇒ 降档；否则非法。
                let want = if ct.legal_depths().contains(&d) {
                    CellVerdict::Legal
                } else if matches!(
                    ct,
                    ColorType::Rgb | ColorType::GrayAlpha | ColorType::Rgba
                ) && d < 8
                {
                    CellVerdict::LegalByDowngrade
                } else {
                    CellVerdict::IllegalNoAlt
                };
                if m.cell(ri, rj) != Some(want) {
                    rule_ok = false;
                }
                rj += 1;
            }
            ri += 1;
        }
        s.add(
            "C1008-矩阵-逐格裁决等于规则重算",
            rule_ok,
            "按规则文字独立重算 25 格裁决并逐格比对；只断计数会漏掉降档/非法边界移动",
        );
        // 降档格的**形状**须精确：真彩三类 × 位深{1,2,4} = 3×3 = 9 格。
        // 这条把「哪些格是降档」钉死，不允许边界悄悄挪动。
        let mut want_downgrade = 0usize;
        let mut wd = 0usize;
        while wd < DEPTHS.len() {
            if matches!(
                DEPTHS[wd],
                1u8 | 2u8 | 4u8
            ) {
                want_downgrade += 3; // Rgb / GrayAlpha / Rgba 三类
            }
            wd += 1;
        }
        s.add(
            "C1008-矩阵-降档格形状为九",
            m.downgrade_cells() == want_downgrade && want_downgrade == 9,
            "真彩三类 × 位深{1,2,4} = 9；边界一动此数即变",
        );
    }

    // ==================== 判据二：压缩比对比表 ====================
    {
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
        // 双向单调：体积比不增 + 耗时比不减。
        let mut size_mono = true;
        let mut time_mono = true;
        let mut i = 1usize;
        while i < t.len() {
            if t[i].size_ratio > t[i - 1].size_ratio {
                size_mono = false;
            }
            if t[i].time_ratio < t[i - 1].time_ratio {
                time_mono = false;
            }
            i += 1;
        }
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
        let mut min_size_idx = 0usize;
        let mut k = 1usize;
        while k < t.len() {
            if t[k].size_ratio < t[min_size_idx].size_ratio {
                min_size_idx = k;
            }
            k += 1;
        }
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
        // 描述字段非空（供 UI 展示，空描述会显示空白档位）。
        let mut desc_ok = true;
        let mut j = 0usize;
        while j < t.len() {
            if t[j].path.is_empty() || t[j].size_class.is_empty() || t[j].speed_class.is_empty() {
                desc_ok = false;
            }
            j += 1;
        }
        s.add(
            "C1008-权衡-档位描述非空",
            desc_ok,
            "path/size_class/speed_class 供 UI 展示，空则显示空白",
        );
    }

    // ==================== 判据三：快速模式收益 ====================
    {
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
    {
        let base = WriteOptions::default_options();
        // 锚点点名的例子：1-bit + 真彩。
        let bad = WriteOptions {
            force_color: Some(ColorType::Rgb),
            force_depth: Some(1),
            allow_downgrade: false,
            ..base.clone()
        };
        let r = validate(&m, &bad);
        s.add(
            "C1008-拒绝-一比特真彩被拒",
            r.is_err(),
            "锚点原例「1-bit+真彩」必须拒绝",
        );
        let f = match r {
            Err(v) => v,
            Ok(()) => OptionFault {
                kind: OptionFaultKind::IllegalCombo,
                what: String::new(),
                why: String::new(),
                advice: String::new(),
                nearest: None,
            },
        };
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
        // 【恰近】独立重算最小曼哈顿距离，逐个非法格核对。
        let mut nearest_ok = true;
        let mut checked = 0usize;
        let mut ti = 0usize;
        while ti < COLOR_TYPES.len() {
            let mut di = 0usize;
            while di < DEPTHS.len() {
                if m.cell(ti, di) == Some(CellVerdict::IllegalNoAlt) {
                    let got = match nearest_legal(&m, COLOR_TYPES[ti], DEPTHS[di]) {
                        Some(v) => v,
                        None => {
                            nearest_ok = false;
                            di += 1;
                            continue;
                        }
                    };
                    // 独立重算：从所有非非法格子里找最小距离，且不得更小。
                    let mut best = u32::MAX;
                    let mut ci = 0usize;
                    while ci < COLOR_TYPES.len() {
                        let mut cdi = 0usize;
                        while cdi < DEPTHS.len() {
                            if m.cell(ci, cdi) != Some(CellVerdict::IllegalNoAlt) {
                                let dt = if ci > ti { ci - ti } else { ti - ci };
                                let dd = if cdi > di { cdi - di } else { di - cdi };
                                let dist = (dt + dd) as u32;
                                if dist < best {
                                    best = dist;
                                }
                            }
                            cdi += 1;
                        }
                        ci += 1;
                    }
                    if best == u32::MAX || got.distance as u32 != best {
                        nearest_ok = false;
                    }
                    checked += 1;
                }
                di += 1;
            }
            ti += 1;
        }
        s.add(
            "C1008-拒绝-最近合法格恰为最小距离",
            nearest_ok && checked > 0,
            "独立重算最小曼哈顿距离并逐格核对；「总返回第一个合法格」会因距离不等变红",
        );
        // 建议的目标格本身必须合法（不能建议到一个非法格）。
        let mut suggestion_legal = true;
        let mut si = 0usize;
        while si < COLOR_TYPES.len() {
            let mut sd = 0usize;
            while sd < DEPTHS.len() {
                if m.cell(si, sd) == Some(CellVerdict::IllegalNoAlt) {
                    if let Some(n) = nearest_legal(&m, COLOR_TYPES[si], DEPTHS[sd]) {
                        let ni = match color_index(n.color) {
                            Some(v) => v,
                            None => {
                                suggestion_legal = false;
                                sd += 1;
                                continue;
                            }
                        };
                        let ndi = match depth_index(n.depth) {
                            Some(v) => v,
                            None => {
                                suggestion_legal = false;
                                sd += 1;
                                continue;
                            }
                        };
                        if m.cell(ni, ndi) == Some(CellVerdict::IllegalNoAlt) {
                            suggestion_legal = false;
                        }
                    }
                }
                sd += 1;
            }
            si += 1;
        }
        s.add(
            "C1008-拒绝-建议目标本身合法",
            suggestion_legal,
            "建议到一个非法格等于把用户再坑一次",
        );
        // 同距留痕：距为 0 的查询（本身合法）不应报告同距。
        let tie_self = nearest_legal_with_tie(&m, ColorType::Gray, 8);
        s.add(
            "C1008-拒绝-自身合法距离为零",
            tie_self.map(|(v, _)| v.distance == 0).unwrap_or(false),
            "合法格查最近应得距离 0（夹逼下界）",
        );

        // 同距裁决必须**确定**：同一输入两次查询结果完全相同。
        // 【语料修正】原语料用 (Rgb,16) —— 该格本身合法、距离 0、**根本不发生同距**，
        // 于是 `tie.tied == false` 分支直接放过，同距规则压根没被观测到。
        // 真同距点是 (Palette,16)：该格 IllegalNoAlt，最近合法候选有三个同为距离 1
        // —— Rgb+16 / Palette+8 / GrayAlpha+16，正确裁决是类型序最小者 Rgb。
        let t1 = nearest_legal_with_tie(&m, ColorType::Palette, 16);
        let t2 = nearest_legal_with_tie(&m, ColorType::Palette, 16);
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
            Some((v, tie)) => {
                if !tie.tied {
                    false
                } else {
                    let mut min_ti = usize::MAX;
                    let mut got = usize::MAX;
                    if let (Some(ti0), Some(di0), Some(g)) =
                        (color_index(ColorType::Palette), depth_index(16), color_index(v.color))
                    {
                        got = g;
                        let mut ci = 0usize;
                        while ci < COLOR_TYPES.len() {
                            let mut cdi = 0usize;
                            while cdi < DEPTHS.len() {
                                if m.cell(ci, cdi) != Some(CellVerdict::IllegalNoAlt) {
                                    let dt = if ci > ti0 { ci - ti0 } else { ti0 - ci };
                                    let dd = if cdi > di0 { cdi - di0 } else { di0 - cdi };
                                    if dt + dd == v.distance as usize && ci < min_ti {
                                        min_ti = ci;
                                    }
                                }
                                cdi += 1;
                            }
                            ci += 1;
                        }
                    }
                    got != usize::MAX && got == min_ti
                }
            }
            None => false,
        };
        s.add(
            "C1008-拒绝-同距取类型序最小",
            tie_rule_ok,
            "独立重算同距候选中的最小类型序号，要求与实际返回一致",
        );
        // ================================================================
        // 非法格（IllegalNoAlt）分支的三要素/锚点/建议 —— 补覆盖盲区。
        // 上面的组合 (Rgb,1) 走的是**降档分支**（LegalByDowngrade + 未允许降档），
        // 而 (Palette,16) 才走 `_` 非法分支。两支的 what/why/advice/nearest
        // 是四份独立文本，只测一支等于放掉另一支。
        // ================================================================
        let illegal_combo = WriteOptions {
            force_color: Some(ColorType::Palette),
            force_depth: Some(16),
            allow_downgrade: false,
            ..base.clone()
        };
        let ic_r = validate(&m, &illegal_combo);
        let ic_f = match ic_r {
            Err(v) => v,
            Ok(()) => OptionFault {
                kind: OptionFaultKind::IllegalCombo,
                what: String::new(),
                why: String::new(),
                advice: String::new(),
                nearest: None,
            },
        };
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
        // 级别越界。
        let bad_level = WriteOptions {
            level: 10,
            ..base.clone()
        };
        s.add(
            "C1008-拒绝-级别十被拒",
            validate(&m, &bad_level).is_err(),
            "级别取值 1..=9",
        );
        let bad_level0 = WriteOptions {
            level: 0,
            ..base.clone()
        };
        s.add(
            "C1008-拒绝-级别零被拒",
            validate(&m, &bad_level0).is_err(),
            "级别 0 非法（不是「未指定」而是越界）",
        );
        // 滤波编号越界。
        let bad_filter = WriteOptions {
            filter: FilterChoice::Fixed(9),
            ..base.clone()
        };
        s.add(
            "C1008-拒绝-滤波编号越界被拒",
            validate(&m, &bad_filter).is_err(),
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
            validate(&m, &f0).is_ok() && validate(&m, &f4).is_ok(),
            "夹逼：越界判据不能把合法边界一起判死",
        );
    }

    // ==================== 判据五：元数据部分成功 ====================
    {
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
        // 全成功变体：不得误报丢弃。
        let all_ok = vec![
            item("a", 1, TextChunkKind::Txt),
            item("b", 2, TextChunkKind::ITxt),
        ];
        let out2 = filter_metadata(&all_ok);
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
        // 条数超限（提交级失败）。
        let mut many = Vec::new();
        let mut k = 0usize;
        while k <= ITEM_MAX_COUNT {
            many.push(item("x", 1, TextChunkKind::Txt));
            k += 1;
        }
        let o_many = WriteOptions {
            metadata: many,
            ..WriteOptions::default_options()
        };
        s.add(
            "C1008-元数据-条数超限被拒",
            validate(&m, &o_many).is_err(),
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

    // ==================== 门禁与自洽 ====================
    {
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
        let d1 = declare_effect(&m, &inter);
        let d2 = declare_effect(&m, &both);
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
            declare_effect(&m, &bad).is_none(),
            "校验没过就不该有效果预声明",
        );
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
        // 夹逼对：上界 4 必须**恰好**合法。原判据只测了 3 与 9，
        // 把上界从 4 改成 3 时Fixed(3) 仍合法 ⇒ 漏过。
        // 这里 0..=4 逐个断 Some、5..=9 逐个断 None，且返回值须等于入参。
        let mut bound_ok = true;
        let mut bk = 0u8;
        while bk <= 4 {
            match FilterChoice::Fixed(bk).fixed_kind() {
                Some(v) if v == bk => {}
                _ => bound_ok = false,
            }
            bk += 1;
        }
        let mut bk2 = 5u8;
        while bk2 <= 9 {
            if FilterChoice::Fixed(bk2).fixed_kind().is_some() {
                bound_ok = false;
            }
            bk2 += 1;
        }
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
        s.add(
            "C1008-门禁-默认选项自洽",
            {
                let d = WriteOptions::default_options();
                d.level == 6 && validate(&m, &d).is_ok()
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

    s
}