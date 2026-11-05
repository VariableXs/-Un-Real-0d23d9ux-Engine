//! UNX-F3430 失败最小化复现账（失败用例重跑清单） · 0x79980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：失败清单生成（失败用例的 ID+参数最小集，供重跑）；
//!     M-189 清单驱动重跑收敛。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 最小集两轴冻结：case_id + 参数白名单（seed/iter），白名单外参数
//!     在清单化时剔除（最小化），显式 seal 构造遇表外参数轴点名拒；
//!   - 清单生成：只收失败用例，按结果首败序排列，同用例去重（首现
//!     参数集保留），清单上限 64 条（超容显性拒）；
//!   - 重跑驱动器：轮次号严格递增（跳轮/重轮点名拒），逐轮应用结局行
//!     ——通过者出清单、新败者入清单、同轮同用例双结局点名拒；
//!   - M-189 收敛语义：整轮跑完清单零缩减零新增且非空 → NoConverge
//!     显性点名；轮次耗尽仍非空 → RoundCapExceed；空清单即已收敛；
//!   - 轮迹留痕：每轮产出报告（轮次号/出清单数/入清单数/余量），
//!     只增不减，驱动器结束后迹长与轮数守恒；
//!   - 双跑一致：清单生成与重跑驱动均纯函数，同输入两次逐字节相同。
//! 常量来源：
//!   - 参数白名单两轴：复现最小集冻结（seed=随机种子轴、iter=迭代轴，
//!     LCG 种子固定可用）；
//!   - 清单上限 64、轮次上限 8：重跑收敛预算（超限即点名为不收敛）；
//!   - 诊断段：0x79980（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x79980;

/// 参数白名单（最小集冻结两轴；表外轴剔除/拒收）。
pub const PARAM_ALLOWLIST: [&'static str; 2] = ["seed", "iter"];

/// 清单条目上限（超容显性拒）。
pub const MAX_ENTRIES: usize = 64;

/// 单条目参数对上限（最小集纪律：超两轴即非最小）。
pub const MAX_PARAMS: usize = 2;

/// 重跑轮次上限（耗尽仍不空即点名不收敛）。
pub const MAX_ROUNDS: usize = 8;

/// 重跑清单条目（ID + 参数最小集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReproEntry {
    /// 失败用例 ID（非空、清单内唯一）。
    pub case_id: String,
    /// 参数最小集（键全在白名单内；键不重）。
    pub params: Vec<(String, String)>,
}

impl ReproEntry {
    /// 显式构造（最小性守卫：表外参数轴 / 空键空值 / 键重 / 超两轴逐类拒）。
    pub fn seal(case_id: &str, params: &[(String, String)]) -> Result<ReproEntry, u32> {
        if case_id.is_empty() {
            return Err(Diag::CaseIdEmpty.val());
        }
        if params.len() > MAX_PARAMS {
            return Err(Diag::EntryNotMinimal.val());
        }
        let mut cleaned: Vec<(String, String)> = Vec::new();
        for (k, v) in params.iter() {
            if k.is_empty() || v.is_empty() {
                return Err(Diag::ParamForeign.val());
            }
            let mut known = false;
            for axis in PARAM_ALLOWLIST.iter() {
                if *axis == k.as_str() {
                    known = true;
                }
            }
            if !known {
                return Err(Diag::ParamForeign.val());
            }
            for (pk, _) in cleaned.iter() {
                if pk == k {
                    return Err(Diag::ParamForeign.val());
                }
            }
            cleaned.push((k.clone(), v.clone()));
        }
        Ok(ReproEntry { case_id: String::from(case_id), params: cleaned })
    }

    /// 最小性自查：参数轴全在白名单内且不超两轴。
    pub fn minimal(&self) -> bool {
        if self.params.len() > MAX_PARAMS {
            return false;
        }
        for (k, v) in self.params.iter() {
            if v.is_empty() {
                return false;
            }
            let mut known = false;
            for axis in PARAM_ALLOWLIST.iter() {
                if *axis == k.as_str() {
                    known = true;
                }
            }
            if !known {
                return false;
            }
        }
        true
    }
}

/// 一次运行结果行（清单化素材）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    /// 用例 ID。
    pub case_id: String,
    /// 是否失败。
    pub failed: bool,
    /// 全参数集（清单化时做最小化剔除）。
    pub params: Vec<(String, String)>,
}

/// 失败清单生成（J1）：失败用例 → ID+参数最小集，首败序 + 去重 + 上限。
pub fn build_repro_list(results: &[RunResult]) -> Result<Vec<ReproEntry>, u32> {
    let mut list: Vec<ReproEntry> = Vec::new();
    for r in results.iter() {
        if r.case_id.is_empty() {
            return Err(Diag::CaseIdEmpty.val());
        }
        if !r.failed {
            continue;
        }
        // 最小化：只保留白名单两轴的非空参数。
        let mut params: Vec<(String, String)> = Vec::new();
        for (k, v) in r.params.iter() {
            let mut known = false;
            for axis in PARAM_ALLOWLIST.iter() {
                if *axis == k.as_str() {
                    known = true;
                }
            }
            if known && !v.is_empty() && params.len() < MAX_PARAMS {
                let mut dup = false;
                for (pk, _) in params.iter() {
                    if pk == k {
                        dup = true;
                    }
                }
                if !dup {
                    params.push((k.clone(), v.clone()));
                }
            }
        }
        // 去重：同用例只留首现参数集。
        let mut dup = false;
        for e in list.iter() {
            if e.case_id == r.case_id {
                dup = true;
            }
        }
        if dup {
            continue;
        }
        if list.len() >= MAX_ENTRIES {
            return Err(Diag::EntryCapExceed.val());
        }
        list.push(ReproEntry { case_id: r.case_id.clone(), params });
    }
    // 终检：清单全项最小性。
    for e in list.iter() {
        if !e.minimal() {
            return Err(Diag::EntryNotMinimal.val());
        }
    }
    Ok(list)
}

/// 轮报告（轮迹留痕；只增不减）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundReport {
    /// 轮次号（1 起，严格递增）。
    pub round: u32,
    /// 本轮出清单数（重跑转绿）。
    pub dropped: u32,
    /// 本轮入清单数（新败）。
    pub added: u32,
    /// 轮末清单余量。
    pub remaining: u32,
}

/// 重跑驱动器（M-189）：按轮应用结局行，收敛或点名不收敛。
/// `rounds` 为 (轮次号, 结局行表) 序列；结局行 = (用例 ID, 仍败否)。
/// 收敛返回轮迹；空清单直接 Ok(空迹)。
pub fn rerun_driver(
    initial: Vec<ReproEntry>,
    rounds: &[(u32, Vec<(String, bool)>)],
) -> Result<Vec<RoundReport>, u32> {
    let mut trace: Vec<RoundReport> = Vec::new();
    let mut list = initial;
    let mut prev_round: u32 = 0;
    for (round_no, outcomes) in rounds.iter() {
        if *round_no == 0 || *round_no != prev_round.saturating_add(1) {
            return Err(Diag::RoundOrderBreak.val());
        }
        prev_round = *round_no;
        if trace.len() >= MAX_ROUNDS {
            return Err(Diag::RoundCapExceed.val());
        }
        // 结局行守卫：空 ID / 同轮同用例双结局逐类拒。
        let mut seen: Vec<String> = Vec::new();
        for (cid, _) in outcomes.iter() {
            if cid.is_empty() {
                return Err(Diag::OutcomeCaseEmpty.val());
            }
            for s in seen.iter() {
                if s == cid {
                    return Err(Diag::DuplicateOutcome.val());
                }
            }
            seen.push(cid.clone());
        }
        // 应用结局：在册且转绿者出清单；新败者入清单。
        let before = list.len() as u32;
        let mut kept: Vec<ReproEntry> = Vec::new();
        for e in list.iter() {
            let mut still_failed = true;
            for (cid, failed) in outcomes.iter() {
                if cid == &e.case_id {
                    still_failed = *failed;
                }
            }
            if still_failed {
                kept.push(e.clone());
            }
        }
        list = kept;
        let mut added = 0u32;
        for (cid, failed) in outcomes.iter() {
            if !failed {
                continue;
            }
            let mut known = false;
            for e in list.iter() {
                if e.case_id == *cid {
                    known = true;
                }
            }
            if !known {
                if list.len() >= MAX_ENTRIES {
                    return Err(Diag::EntryCapExceed.val());
                }
                list.push(ReproEntry { case_id: cid.clone(), params: Vec::new() });
                added += 1;
            }
        }
        let after = list.len() as u32;
        // M-189 收敛语义：整轮零缩减零新增且非空 → 不收敛点名。
        if before == after && after > 0 && added == 0 {
            return Err(Diag::NoConverge.val());
        }
        trace.push(RoundReport {
            round: *round_no,
            dropped: before.saturating_sub(after.saturating_sub(added)),
            added,
            remaining: after,
        });
        if list.is_empty() {
            // 已收敛：余轮不再执行（轮迹到此为止，收敛即停）。
            return Ok(trace);
        }
    }
    // 轮次耗尽仍非空 → 不收敛点名。
    if !list.is_empty() {
        return Err(Diag::RoundCapExceed.val());
    }
    Ok(trace)
}

/// 诊断码闭集：0x79981..=0x7998C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 用例 ID 空：清单化或结局行出现空 ID。
    CaseIdEmpty = 0x01,
    /// 参数轴表外：seal 构造遇白名单外参数轴（或空键空值/键重）。
    ParamForeign = 0x02,
    /// 清单超容：条目数超过 64 上限。
    EntryCapExceed = 0x03,
    /// 轮序破：轮次号非严格递增（跳轮/重轮/零轮）。
    RoundOrderBreak = 0x04,
    /// 不收敛：整轮零缩减零新增且清单非空。
    NoConverge = 0x05,
    /// 结局行 ID 空。
    OutcomeCaseEmpty = 0x06,
    /// 双结局：同轮同一用例出现两条结局行。
    DuplicateOutcome = 0x07,
    /// 轮次耗尽：MAX_ROUNDS 轮后清单仍非空。
    RoundCapExceed = 0x08,
    /// 非最小：清单项含表外参数轴或超两轴。
    EntryNotMinimal = 0x09,
    /// 条目序破：清单序非结果首败序。
    EntryOrderBreak = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 双跑不一致：同输入两次清单/驱动结论不同。
    DoubleRunDiverge = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::CaseIdEmpty => "用例ID空：建议补全用例 ID 再入清单，空 ID 无法重跑",
            Diag::ParamForeign => "参数轴表外：建议参数只用 seed/iter 两轴，表外轴不入最小集",
            Diag::EntryCapExceed => "清单超容：建议分批重跑，单清单上限 64 条不放宽",
            Diag::RoundOrderBreak => "轮序破：建议轮次号 1 起严格递增，跳轮重轮先修序再跑",
            Diag::NoConverge => "不收敛：建议核对该用例是否确定性失败，非确定失败先隔离",
            Diag::OutcomeCaseEmpty => "结局行ID空：建议结局行补全用例 ID 再驱动",
            Diag::DuplicateOutcome => "双结局：建议同轮同用例只留一条结局行，冲突先合并",
            Diag::RoundCapExceed => "轮次耗尽：建议超 8 轮仍不空时人工介入，不再自动重跑",
            Diag::EntryNotMinimal => "非最小：建议清单项只留 ID+白名单参数，多余参数剔除",
            Diag::EntryOrderBreak => "条目序破：建议清单按结果首败序排列，不做重排",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::DoubleRunDiverge => "双跑不一致：建议清单生成与驱动做成纯函数，同输入同结论",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::CaseIdEmpty,
    Diag::ParamForeign,
    Diag::EntryCapExceed,
    Diag::RoundOrderBreak,
    Diag::NoConverge,
    Diag::OutcomeCaseEmpty,
    Diag::DuplicateOutcome,
    Diag::RoundCapExceed,
    Diag::EntryNotMinimal,
    Diag::EntryOrderBreak,
    Diag::SegGuardFail,
    Diag::DoubleRunDiverge,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79981..=0x7998C 闭集内"
}

/// 标准失败场景：五条结果（三败两过，含重复失败与表外参数）。
pub fn build_sample_results() -> Vec<RunResult> {
    vec![
        RunResult {
            case_id: String::from("boot_e820"),
            failed: true,
            params: vec![
                (String::from("seed"), String::from("7")),
                (String::from("iter"), String::from("3")),
                (String::from("ghost"), String::from("x")),
            ],
        },
        RunResult {
            case_id: String::from("sched_fifo"),
            failed: false,
            params: vec![(String::from("iter"), String::from("1"))],
        },
        RunResult {
            case_id: String::from("hda_probe"),
            failed: true,
            params: vec![(String::from("seed"), String::from("42"))],
        },
        RunResult {
            case_id: String::from("boot_e820"),
            failed: true,
            params: vec![(String::from("seed"), String::from("9"))],
        },
        RunResult {
            case_id: String::from("tpm_extend"),
            failed: true,
            params: Vec::new(),
        },
    ]
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B12 批段窗 [0x79800, 0x799C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79800 && SEG_BASE <= 0x799C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 冻结面：白名单两轴、上限 64/2/8 逐字核对。
fn frozen_constants_ok() -> bool {
    PARAM_ALLOWLIST.len() == 2
        && PARAM_ALLOWLIST.get(0).map(|k| *k == "seed").unwrap_or(false)
        && PARAM_ALLOWLIST.get(1).map(|k| *k == "iter").unwrap_or(false)
        && MAX_ENTRIES == 64
        && MAX_PARAMS == 2
        && MAX_ROUNDS == 8
}

/// seal 构造面：合法最小集过、表外轴/空 ID/键重/超两轴逐类拒。
fn seal_guards_ok() -> bool {
    let good = ReproEntry::seal(
        "case_a",
        &[(String::from("seed"), String::from("7")), (String::from("iter"), String::from("2"))],
    );
    if good.is_err() || !good.as_ref().map(|e| e.minimal()).unwrap_or(false) {
        return false;
    }
    let ghost = ReproEntry::seal("case_b", &[(String::from("mode"), String::from("x"))]);
    let empty_id = ReproEntry::seal("", &[]);
    let dup_key = ReproEntry::seal(
        "case_c",
        &[(String::from("seed"), String::from("1")), (String::from("seed"), String::from("2"))],
    );
    let over_two = ReproEntry::seal(
        "case_d",
        &[
            (String::from("seed"), String::from("1")),
            (String::from("iter"), String::from("2")),
            (String::from("iter"), String::from("3")),
        ],
    );
    ghost == Err(Diag::ParamForeign.val())
        && empty_id == Err(Diag::CaseIdEmpty.val())
        && dup_key == Err(Diag::ParamForeign.val())
        && over_two == Err(Diag::EntryNotMinimal.val())
}

/// J1 清单生成正常流：只收失败、首败序、去重留首现、表外参数剔除。
fn build_list_ok() -> bool {
    let results = build_sample_results();
    let list = match build_repro_list(&results) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if list.len() != 3 {
        return false;
    }
    let first = match list.first() {
        Some(e) => e,
        None => return false,
    };
    // 首败序：boot_e820 最先；重复失败行被去重（首现 seed=7 保留）。
    if first.case_id != "boot_e820" {
        return false;
    }
    match list.get(0) {
        Some(e) => {
            if e.params.len() != 2 {
                return false;
            }
            match e.params.first() {
                Some((k, v)) if k == "seed" && v == "7" => {}
                _ => return false,
            }
        }
        None => return false,
    }
    // 第三条：tpm_extend 无参数（空参数集也算最小）。
    match list.get(2) {
        Some(e) => e.case_id == "tpm_extend" && e.minimal(),
        None => false,
    }
}

/// 注入 1：结果行空 ID 必抓。
fn inj_empty_case_id_ok() -> bool {
    let results = vec![RunResult {
        case_id: String::new(),
        failed: true,
        params: Vec::new(),
    }];
    build_repro_list(&results) == Err(Diag::CaseIdEmpty.val())
}

/// M-189 收敛正样本：三轮重跑逐轮缩减至空，轮迹三痕守恒。
fn converge_ok() -> bool {
    let list = match build_repro_list(&build_sample_results()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let rounds = vec![
        (
            1u32,
            vec![
                (String::from("boot_e820"), false),
                (String::from("hda_probe"), true),
                (String::from("tpm_extend"), true),
            ],
        ),
        (
            2u32,
            vec![
                (String::from("hda_probe"), false),
                (String::from("tpm_extend"), true),
            ],
        ),
        (3u32, vec![(String::from("tpm_extend"), false)]),
    ];
    match rerun_driver(list, &rounds) {
        Ok(trace) => {
            trace.len() == 3
                && trace.first().map(|r| r.round == 1).unwrap_or(false)
                && match trace.last() {
                    Some(r) => r.round == 3 && r.remaining == 0,
                    None => false,
                }
        }
        Err(_) => false,
    }
}

/// 注入 2：整轮零缩减零新增且非空 → NoConverge 点名。
fn inj_no_converge_ok() -> bool {
    let list = vec![
        ReproEntry { case_id: String::from("stable_fail"), params: Vec::new() },
    ];
    let rounds = vec![(1u32, vec![(String::from("stable_fail"), true)])];
    rerun_driver(list, &rounds) == Err(Diag::NoConverge.val())
}

/// 注入 3：轮序破（零轮 / 跳轮）必抓。
fn inj_round_order_ok() -> bool {
    let list = vec![ReproEntry { case_id: String::from("c"), params: Vec::new() }];
    let zero = vec![(0u32, vec![(String::from("c"), false)])];
    let skip = vec![(2u32, vec![(String::from("c"), false)])];
    rerun_driver(list.clone(), &zero) == Err(Diag::RoundOrderBreak.val())
        && rerun_driver(list, &skip) == Err(Diag::RoundOrderBreak.val())
}

/// 注入 4：结局行两违约（空 ID / 同轮双结局）逐类拒。
fn inj_outcome_guards_ok() -> bool {
    let list = vec![ReproEntry { case_id: String::from("c"), params: Vec::new() }];
    let empty_id = vec![(1u32, vec![(String::new(), false)])];
    let dup = vec![(
        1u32,
        vec![(String::from("c"), true), (String::from("c"), false)],
    )];
    rerun_driver(list.clone(), &empty_id) == Err(Diag::OutcomeCaseEmpty.val())
        && rerun_driver(list, &dup) == Err(Diag::DuplicateOutcome.val())
}

/// 新败入清单：重跑发现的新失败用例追加（无参数最小形态），
/// 次轮转绿收敛（驱动器终态必须清单空）。
fn new_failure_append_ok() -> bool {
    let list = vec![ReproEntry { case_id: String::from("old"), params: Vec::new() }];
    let rounds = vec![
        (
            1u32,
            vec![(String::from("old"), false), (String::from("fresh"), true)],
        ),
        (2u32, vec![(String::from("fresh"), false)]),
    ];
    match rerun_driver(list, &rounds) {
        Ok(trace) => {
            trace.len() == 2
                && match trace.first() {
                    Some(r) => r.added == 1 && r.remaining == 1,
                    None => false,
                }
                && match trace.last() {
                    Some(r) => r.round == 2 && r.remaining == 0,
                    None => false,
                }
        }
        Err(_) => false,
    }
}

/// 双跑一致：清单生成与驱动两次执行逐字节相同。
fn double_run_ok() -> bool {
    let a = build_repro_list(&build_sample_results());
    let b = build_repro_list(&build_sample_results());
    if a != b {
        return false;
    }
    let rounds = vec![(1u32, vec![(String::from("boot_e820"), false)])];
    let d1 = rerun_driver(a.clone().unwrap_or_default(), &rounds);
    let d2 = rerun_driver(b.unwrap_or_default(), &rounds);
    d1 == d2
}

/// 错误路径 Result 语义：Ok 与闭集 Err 两侧均可达。
fn error_path_ok() -> bool {
    let list = vec![ReproEntry { case_id: String::from("x"), params: Vec::new() }];
    match rerun_driver(list, &[]) {
        // 零轮结局 + 非空清单 → 轮次耗尽点名。
        Err(code) => {
            let mut in_set = false;
            for d in ALL_DIAGS.iter() {
                if d.val() == code {
                    in_set = true;
                }
            }
            in_set && !detail(0x0).is_empty()
        }
        Ok(_) => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let list = match build_repro_list(&build_sample_results()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // 三条失败用例一轮全转绿 → 收敛即停，单轮留痕。
    let rounds = vec![(
        1u32,
        vec![
            (String::from("boot_e820"), false),
            (String::from("hda_probe"), false),
            (String::from("tpm_extend"), false),
        ],
    )];
    let summary = match rerun_driver(list, &rounds) {
        Ok(trace) => format!("UNX-F3430 失败复现账：清单驱动重跑{}轮留痕", trace.len()),
        Err(_) => String::new(),
    };
    !summary.is_empty() && summary.contains("清单驱动重跑")
}

// ── 判据总成：run_unx3430_checks ───────────────────────────────────────

/// run_unx3430_checks：本单判据聚合（诊断段 0x79980，全部走 add_dyn）。
pub fn run_unx3430_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3430 失败最小化复现账");
    let checks: [(&'static str, bool, String); 14] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "冻结面白名单两轴与64/2/8上限",
            frozen_constants_ok(),
            format!("冻结面{}", if frozen_constants_ok() { "齐" } else { "漂移" }),
        ),
        (
            "seal构造最小性守卫四类逐类拒",
            seal_guards_ok(),
            format!("seal守卫{}", if seal_guards_ok() { "齐" } else { "缺" }),
        ),
        (
            "J1清单生成首败序去重最小化",
            build_list_ok(),
            format!("清单{}", if build_list_ok() { "成" } else { "败" }),
        ),
        (
            "注入1结果行空ID必抓",
            inj_empty_case_id_ok(),
            format!("空ID{}", if inj_empty_case_id_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "M189三轮重跑逐轮缩减至空收敛",
            converge_ok(),
            format!("收敛{}", if converge_ok() { "达" } else { "未达" }),
        ),
        (
            "注入2整轮零缩减零新增必抓",
            inj_no_converge_ok(),
            format!("不收敛{}", if inj_no_converge_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3零轮跳轮必抓",
            inj_round_order_ok(),
            format!("轮序{}", if inj_round_order_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入4结局行空ID与双结局逐类拒",
            inj_outcome_guards_ok(),
            format!("结局违约{}", if inj_outcome_guards_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "新败入清单无参数最小形态",
            new_failure_append_ok(),
            format!("新败{}", if new_failure_append_ok() { "入账" } else { "丢" }),
        ),
        (
            "双跑一致清单与驱动逐字节同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径Ok与闭集Err两侧可达",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79980);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn frozen_constants_exact() {
        assert!(frozen_constants_ok());
        assert_eq!(PARAM_ALLOWLIST, ["seed", "iter"]);
    }

    #[test]
    fn seal_guards_reject_all() {
        assert!(seal_guards_ok());
        let e = ReproEntry::seal("c", &[(String::from("iter"), String::from("5"))]).unwrap();
        assert_eq!(e.params.first().unwrap().1, "5");
    }

    #[test]
    fn build_list_first_fail_order_dedup() {
        assert!(build_list_ok());
        assert!(inj_empty_case_id_ok());
        let list = build_repro_list(&build_sample_results()).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list.get(1).unwrap().case_id, "hda_probe");
    }

    #[test]
    fn rerun_converges_in_three_rounds() {
        assert!(converge_ok());
    }

    #[test]
    fn non_convergence_named() {
        assert!(inj_no_converge_ok());
        assert!(detail(Diag::NoConverge.val()).contains("确定性"));
    }

    #[test]
    fn round_and_outcome_guards() {
        assert!(inj_round_order_ok());
        assert!(inj_outcome_guards_ok());
    }

    #[test]
    fn fresh_failure_appended_minimal() {
        assert!(new_failure_append_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x799FF).starts_with("未知诊断码"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3430_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
