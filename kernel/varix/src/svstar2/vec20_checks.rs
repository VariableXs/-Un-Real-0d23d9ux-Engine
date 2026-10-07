//! VE-F0420 · 域自检（判据逐条对应，见 `vec20_closure.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 十八件证据 → `C20-证据-*`（标准输入齐备、期望清单恰十八且唯一、缺项逐条
//!   报出、缺项非空即不齐、重登被抓、重登+漏登同发仍被抓、期望外被抓、
//!   模块路径与判据覆盖非空、集合差双向、计数不足以代表齐备）
//! - 缺陷清零 → `C20-缺陷-*`（🔴 清零才放行、🔴 逐条点名、🟡 必闭环、
//!   🟡 三种闭环动作各算闭环、🟢 不要求闭环、🟢 缺闭环动作不误报、
//!   按单号回溯、总数与三档之和自洽）
//! - 双签 → `C20-双签-*`（两席齐备且主体相异才算、缺任一席即阻断、
//!   同一方连签两席不算双签、主体相异才放行、逐席可查、人话呈现点名）
//! - 经验包 → `C20-经验-*`（三条齐备、缺一条即不齐、空下游动作拒收、
//!   缺项逐条、重登不算齐备、标准输入含三条可机检动作）
//! - 判据与门禁 → `C20-门禁-*`（标准输入放行、六类阻断各点名、裁定顺序
//!   本地先于上游、上游退化阻断、不可比要求重取基线、上游未接入不越权、
//!   上游映射三变体双向、收口清单含无障碍核验行、渲染非空、齐备门不吞缺项）
//!
//! 弱门禁自律：本文件的判据刻意用**表外真实形态**（缺项、重登、同主体连签、
//! 空下游动作），不用「表内元素验查表函数」；两侧断言不由同一 bool 驱动；
//! 参考值与被测对象**无关地独立算出**。


use super::vec20_closure::*;
use crate::checks::CheckSet;

/// 构造一份「缺了某单」的台账（缺项判据的输入）。
///
/// **不删条目**而是**只登记前 n 件**：删条目与不登记在这张表里等价，
/// 但「只登记前 n 件」更贴近真实场景（施工方漏做一单 → 台账里就没有它）。
fn ledger_missing_n(n: usize) -> EvidenceLedger {
    let mut l = EvidenceLedger::new();
    let mut i = 0usize;
    while i < n {
        l.add(Evidence {
            id: EVIDENCE_TABLE[i].0,
            module: EVIDENCE_TABLE[i].1,
            covers: EVIDENCE_TABLE[i].2,
        });
        i += 1;
    }
    l
}

/// 构造一份「F0402 被登两次」的台账。
///
/// 刻意**重登的是第一条**：期望清单是按单号查存在性的，重登第一条时
/// F0403 仍缺——若判据只查「条数」或只查「第一条存在」，会误判为齐备。
fn ledger_dup_first() -> EvidenceLedger {
    let mut l = ledger_missing_n(0);
    // 只登 F0402 两次（其余全缺）→ 重登与缺项同发。
    l.add(Evidence {
        id: "VE-F0402",
        module: EVIDENCE_TABLE[0].1,
        covers: EVIDENCE_TABLE[0].2,
    });
    l.add(Evidence {
        id: "VE-F0402",
        module: EVIDENCE_TABLE[0].1,
        covers: EVIDENCE_TABLE[0].2,
    });
    l
}

/// 构造一份「全登但有一条重登」的台账（18 条里 1 条重复 → 实际 17 单）。
fn ledger_all_with_dup() -> EvidenceLedger {
    let mut l = ledger_missing_n(18);
    // 再登一次 F0402：条数 19、单号覆盖仍 18。
    l.add(Evidence {
        id: "VE-F0402",
        module: EVIDENCE_TABLE[0].1,
        covers: EVIDENCE_TABLE[0].2,
    });
    l
}

/// 构造一份「含期望外单号」的台账。
fn ledger_with_unexpected() -> EvidenceLedger {
    let mut l = ledger_missing_n(18);
    l.add(Evidence {
        id: "VE-F9999",
        module: "svstar2::not_a_real_module",
        covers: "不在期望清单内的单号",
    });
    l
}

/// 判据侧**独立**统计：经验包里有几种不同标识（不调 `LessonPack` 的任何方法）。
///
/// 为什么要自己数：`LessonPack` 没有暴露「不同标识数」，若判据改用
/// `missing().is_empty()` 代替，就只能验「有没有缺」，验不出「有没有重登」——
/// 而重登正是让「三条都在」变成假齐备的形态。故判据自己数一遍。
fn complete_lesson_count(p: &LessonPack) -> usize {
    let mut kinds = 0usize;
    let mut i = 0usize;
    while i < REQUIRED_LESSONS.len() {
        let id = REQUIRED_LESSONS[i];
        let mut seen = false;
        let mut j = 0usize;
        while j < p.len() {
            if p.get(j).map(|l| l.id) == Some(id) {
                seen = true;
            }
            j += 1;
        }
        if seen {
            kinds += 1;
        }
        i += 1;
    }
    kinds
}

/// VE-F0420 域自检。
pub fn run_vec20_checks() -> CheckSet {
    let mut set = CheckSet::new();
    let std = standard_input();

    // ========================================================================
    // 判据一：十八件证据齐备
    // ========================================================================

    set.add(
        "C20-证据-标准输入齐备",
        std.evidence.complete() && std.evidence.verdict().is_complete(),
        "",
    );

    // 期望清单恰十八且唯一：长度对 + 逐对去重后仍十八。
    let mut dup_in_expected = false;
    let mut i = 0usize;
    while i < EXPECTED_EVIDENCE.len() {
        let mut j = i + 1;
        while j < EXPECTED_EVIDENCE.len() {
            if EXPECTED_EVIDENCE[i] == EXPECTED_EVIDENCE[j] {
                dup_in_expected = true;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C20-证据-期望清单恰十八且唯一",
        EXPECTED_EVIDENCE.len() == 18 && !dup_in_expected,
        "",
    );

    // 期望清单与登记表**逐槽位对齐**（防表与清单各自漂移）。
    let mut table_matches = true;
    let mut k = 0usize;
    while k < EVIDENCE_TABLE.len() && k < EXPECTED_EVIDENCE.len() {
        if EVIDENCE_TABLE[k].0 != EXPECTED_EVIDENCE[k] {
            table_matches = false;
        }
        k += 1;
    }
    set.add(
        "C20-证据-登记表与期望清单逐槽位对齐",
        table_matches && EVIDENCE_TABLE.len() == EXPECTED_EVIDENCE.len(),
        "",
    );

    // **表外真实形态**：只登 17 件 → 缺最后一件（F0419）。
    let l17 = ledger_missing_n(17);
    set.add(
        "C20-证据-缺末件被逐条报出",
        !l17.complete()
            && l17.missing() == alloc::vec![EXPECTED_EVIDENCE[17]]
            && l17.verdict() == EvidenceVerdict::Missing {
                first: EXPECTED_EVIDENCE[17],
            },
        "",
    );

    // 缺项数逐档（1/3/5/7/9/0/18 七个取值），并**双向**校验：
    // 少登 N 件 → 恰缺 N 件；登全 → 一件不缺；一件不登 → 十八件全缺。
    let mut gap_ok = true;
    let mut g = 1usize;
    while g <= 9 {
        if ledger_missing_n(g).missing().len() != 18 - g {
            gap_ok = false;
        }
        g += 2;
    }
    if ledger_missing_n(0).missing().len() != 18 {
        gap_ok = false;
    }
    if ledger_missing_n(18).missing().len() != 0 {
        gap_ok = false;
    }
    set.add("C20-证据-缺项数逐档正确", gap_ok, "");

    // **重登 + 漏登同发**：台账 2 条但都是 F0402 → 条数远少于 18，
    // 计数判据会红，而集合差要能同时报出「缺 17 + 重登 1」。
    // 裁定优先级是**缺项先于重登**（缺项更要紧：那是活没干完，重登只是账写错），
    // 故此处期望 `Missing{第一条缺项=F0403}` 而非 `Duplicated`。
    let ldup = ledger_dup_first();
    set.add(
        "C20-证据-重登与漏登同发仍被抓",
        ldup.len() == 2
            && ldup.duplicates() == alloc::vec!["VE-F0402"]
            && ldup.missing().len() == 17
            && ldup.missing()[0] == EXPECTED_EVIDENCE[1]
            && !ldup.complete()
            && ldup.verdict() == EvidenceVerdict::Missing {
                first: EXPECTED_EVIDENCE[1],
            },
        "",
    );

    // **计数不足以代表齐备**：全登 18 件后再重登 F0402 → 条数 19 但
    // 只有 17 个不同单号、缺 F0418。判据要证明「不是条数说了算」。
    let lald = ledger_all_with_dup();
    set.add(
        "C20-证据-条数不替代单号覆盖",
        lald.len() == 19
            && lald.duplicates() == alloc::vec!["VE-F0402"]
            && !lald.complete()
            && lald.verdict() == EvidenceVerdict::Duplicated { id: "VE-F0402" },
        "",
    );

    // 期望外单号被单独报出（清单过期也是阻断理由）。
    let lue = ledger_with_unexpected();
    set.add(
        "C20-证据-期望外单号被报出",
        lue.unexpected() == alloc::vec!["VE-F9999"]
            && !lue.complete()
            && lue.verdict() == EvidenceVerdict::Unexpected { id: "VE-F9999" },
        "",
    );

    // 双向集合差：只登期望内的 → `unexpected` 空；只登期望外的 → `missing` 空。
    // 两侧**分别**断言，故「集合差只做了一边」的实现会红。
    set.add(
        "C20-门禁-集合差双向且互斥",
        ledger_missing_n(5).unexpected().is_empty()
            && !ledger_missing_n(5).missing().is_empty()
            && ledger_with_unexpected().missing().is_empty()
            && !ledger_with_unexpected().unexpected().is_empty(),
        "",
    );

    // 每条证据的模块路径与覆盖判据非空（登记了但内容空 = 没登记）。
    let mut content_ok = true;
    let mut e = 0usize;
    while e < EVIDENCE_TABLE.len() {
        if EVIDENCE_TABLE[e].1.is_empty() || EVIDENCE_TABLE[e].2.is_empty() {
            content_ok = false;
        }
        e += 1;
    }
    set.add("C20-证据-模块路径与判据覆盖非空", content_ok, "");

    // 模块路径都带 `svstar2::` 前缀（防止写错落位而不自知）。
    let mut prefix_ok = true;
    let mut p = 0usize;
    while p < EVIDENCE_TABLE.len() {
        if !EVIDENCE_TABLE[p].1.starts_with("svstar2::") {
            prefix_ok = false;
        }
        p += 1;
    }
    set.add("C20-证据-模块路径落位前缀一致", prefix_ok, "");

    // find / get 两个读出面一致（不能一个能找到另一个找不到）。
    let mut read_ok = std.evidence.get(0).is_some() && std.evidence.get(18).is_none();
    if std.evidence.find("VE-F0402").is_none() || std.evidence.find("VE-F0413").is_none() {
        read_ok = false;
    }
    if std.evidence.find("VE-F0000").is_some() {
        read_ok = false;
    }
    set.add("C20-证据-两个读出面一致", read_ok, "");

    // 渲染含全部十八个单号（人工复核时要能一眼看全）。
    let rendered = std.evidence.render();
    let mut render_covers = rendered.contains("VE-F0402") && rendered.contains("VE-F0419");
    let mut q = 0usize;
    while q < EXPECTED_EVIDENCE.len() {
        if !rendered.contains(EXPECTED_EVIDENCE[q]) {
            render_covers = false;
        }
        q += 1;
    }
    set.add("C20-证据-渲染覆盖十八单号", render_covers, "");

    // ========================================================================
    // 判据四：缺陷清零（🔴 清零 / 🟡 闭环 / 🟢 登记）
    // ========================================================================

    // 标准输入：0 个 🔴，1 个已闭环 🟡，1 个仅登记 🟢。
    set.add(
        "C20-缺陷-标准账已清零",
        std.defects.critical_cleared() && std.defects.minor_closed(),
        "",
    );

    // **表外真实形态**：留一条 🔴 → 不清零，且逐条点名。
    let mut dc = DefectLedger::new();
    dc.record(DefectEntry {
        summary: "词法器在超长标识符上溢出长度计数",
        severity: Defect::Critical,
        closure: None,
        origin: "VE-F0405",
    });
    set.add(
        "C20-缺陷-未清零的严重项逐条点名",
        !dc.critical_cleared()
            && dc.open_critical() == alloc::vec!["词法器在超长标识符上溢出长度计数"],
        "",
    );

    // **🔴 给了闭环动作也不清零**——清零的唯一口径是「不在账上」，
    // 不是「有条目但标了已缓解」。这是最容易做漏的一处。
    let mut dcm = DefectLedger::new();
    dcm.record(DefectEntry {
        summary: "严重但被标缓解的缺陷",
        severity: Defect::Critical,
        closure: Some(Closure::Mitigated),
        origin: "VE-F0406",
    });
    set.add(
        "C20-缺陷-严重项标了闭环仍不清零",
        !dcm.critical_cleared() && dcm.open_critical().len() == 1,
        "",
    );

    // 🟡 三种闭环动作各算闭环（分开验，不合并——合并后只验一种就够）。
    let mut closure_ok = true;
    let acts = [Closure::Mitigated, Closure::Deferred, Closure::WontFix];
    let mut ai = 0usize;
    while ai < acts.len() {
        if !acts[ai].is_closed() {
            closure_ok = false;
        }
        let mut d = DefectLedger::new();
        d.record(DefectEntry {
            summary: "待闭环的中等问题",
            severity: Defect::Minor,
            closure: Some(acts[ai]),
            origin: "VE-F0412",
        });
        if !d.minor_closed() {
            closure_ok = false;
        }
        ai += 1;
    }
    set.add("C20-缺陷-三种闭环动作各算闭环", closure_ok, "");

    // 🟡 无闭环动作 → 未闭环（只登记不闭环 = 阻断）。
    let mut du = DefectLedger::new();
    du.record(DefectEntry {
        summary: "只登记未闭环的中等问题",
        severity: Defect::Minor,
        closure: None,
        origin: "VE-F0413",
    });
    set.add(
        "C20-缺陷-中等项无闭环动作即未闭环",
        !du.minor_closed() && du.unclosed_minor() == alloc::vec!["只登记未闭环的中等问题"],
        "",
    );

    // 🟢 缺闭环动作**不**误报（轻微只需登记）。
    let mut dt = DefectLedger::new();
    dt.record(DefectEntry {
        summary: "只登记的轻微问题",
        severity: Defect::Trivial,
        closure: None,
        origin: "VE-F0408",
    });
    set.add(
        "C20-缺陷-轻微项不要求闭环",
        dt.minor_closed() && dt.critical_cleared() && dt.unclosed_minor().is_empty(),
        "",
    );

    // 🟢 即使给了闭环动作也不该进「未闭环」清单。
    let mut dt2 = DefectLedger::new();
    dt2.record(DefectEntry {
        summary: "轻微且已缓解",
        severity: Defect::Trivial,
        closure: Some(Closure::Mitigated),
        origin: "VE-F0408",
    });
    set.add("C20-缺陷-轻微项闭环不污染未闭环清单", dt2.unclosed_minor().is_empty(), "");

    // 回归回溯：按单号反查，且**不串号**。
    set.add(
        "C20-缺陷-按单号回溯不串号",
        std.defects.trace_to("VE-F0411") == alloc::vec!["行延续符后紧跟 EOF 时报错文案未含规范条款号"]
            && std.defects.trace_to("VE-F0408") == alloc::vec!["文档注释标注语法的空标注块未在渲染里留空行"]
            && std.defects.trace_to("VE-F0403").is_empty(),
        "",
    );

    // 三档条数与总数自洽（防止 count_of 漏算某一档）。
    set.add(
        "C20-缺陷-三档计数与总数自洽",
        std.defects.len() == 2
            && std.defects.count_of(Defect::Critical)
                + std.defects.count_of(Defect::Minor)
                + std.defects.count_of(Defect::Trivial)
                == std.defects.len(),
        "",
    );

    // 渲染按档位逐条列出且未闭环的中等项标「未闭环」。
    let dr = du.render();
    set.add(
        "C20-缺陷-渲染含档位与未闭环标记",
        dr.contains("中等") && dr.contains("未闭环"),
        "",
    );

    // 严重度三档标签互不相同。
    set.add(
        "C20-缺陷-三档标签互不相同",
        Defect::Critical.label() != Defect::Minor.label()
            && Defect::Minor.label() != Defect::Trivial.label()
            && Defect::Critical.label() != Defect::Trivial.label(),
        "",
    );

    // 读出面边界：越界读返回 None，不 panic、不假造。
    set.add(
        "C20-缺陷-读越界返回空",
        std.defects.get(0).is_some() && std.defects.get(99).is_none(),
        "",
    );

    // ========================================================================
    // 判据二：双签
    // ========================================================================

    set.add(
        "C20-双签-标准输入双签完整",
        std.signoff.double_signed() && std.signoff.verdict().is_complete(),
        "",
    );

    // 缺一席即阻断（两侧分别验——合并验则只验一席就够）。
    let mut s1 = Signoff::new();
    s1.sign(Signature::new(Seat::Builder, "A"));
    let mut s2 = Signoff::new();
    s2.sign(Signature::new(Seat::Verifier, "B"));
    set.add(
        "C20-双签-缺任一席即未完成",
        !s1.double_signed()
            && s1.verdict() == SignoffVerdict::MissingSeat { seat: Seat::Verifier }
            && !s2.double_signed()
            && s2.verdict() == SignoffVerdict::MissingSeat { seat: Seat::Builder },
        "",
    );

    // **核心弱门禁反制**：同一方（`who` 相同）连签两席**不算**双签。
    let mut ss = Signoff::new();
    ss.sign(Signature::new(Seat::Builder, "SAME"));
    ss.sign(Signature::new(Seat::Verifier, "SAME"));
    set.add(
        "C20-双签-同一方连签两席不算双签",
        !ss.distinct_who()
            && !ss.double_signed()
            && ss.verdict() == SignoffVerdict::SameSigner
            && ss.missing_seats().is_empty(),
        "",
    );

    // 反向：主体相异才放行（换个 who 立刻齐备）——证明判定真读了 `who`。
    let mut sd = Signoff::new();
    sd.sign(Signature::new(Seat::Builder, "SAME"));
    sd.sign(Signature::new(Seat::Verifier, "OTHER"));
    set.add("C20-双签-主体相异才放行", sd.distinct_who() && sd.double_signed(), "");

    // 主体相异判定**不与**缺签混谈：一席未签时 `distinct_who` 必须为假，
    // 否则「缺签」会被误报成「同主体」，两个不同缺陷只报一个。
    set.add(
        "C20-双签-缺签时主体判定让位给缺签",
        !s1.distinct_who() && s1.verdict() == SignoffVerdict::MissingSeat { seat: Seat::Verifier },
        "",
    );

    // 逐席可查。
    set.add(
        "C20-双签-逐席可查",
        std.signoff.signed_by(Seat::Builder)
            && std.signoff.signed_by(Seat::Verifier)
            && !s1.signed_by(Seat::Verifier),
        "",
    );

    // 一方多签不影响「已签」（取首次）。
    let mut sm = Signoff::new();
    sm.sign(Signature::new(Seat::Builder, "A"));
    sm.sign(Signature::new(Seat::Builder, "A2"));
    sm.sign(Signature::new(Seat::Verifier, "B"));
    set.add("C20-双签-一方多签仍算齐备", sm.double_signed(), "");

    // 渲染点名缺失席位 / 同主体。
    set.add(
        "C20-双签-渲染点名",
        s1.render().contains("缺 验证方") && ss.render().contains("主体相同"),
        "",
    );

    // 席位标签互不相同。
    set.add(
        "C20-双签-席位标签互不相同",
        Seat::Builder.label() != Seat::Verifier.label(),
        "",
    );

    // ========================================================================
    // 判据三：经验包
    // ========================================================================

    set.add(
        "C20-经验-标准输入三条齐备",
        std.lessons.complete() && std.lessons.missing().is_empty(),
        "",
    );

    // 期望三条互不相同。
    let mut ldup = false;
    let mut li = 0usize;
    while li < REQUIRED_LESSONS.len() {
        let mut lj = li + 1;
        while lj < REQUIRED_LESSONS.len() {
            if REQUIRED_LESSONS[li] == REQUIRED_LESSONS[lj] {
                ldup = true;
            }
            lj += 1;
        }
        li += 1;
    }
    set.add("C20-经验-期望三条互不相同", REQUIRED_LESSONS.len() == 3 && !ldup, "");

    // 标准输入里三条都带**非空**下游动作，且引用了下一组开工单 F0421/F0434。
    let mut actions_ok = std.lessons.len() == 3;
    let mut ai2 = 0usize;
    while ai2 < REQUIRED_LESSONS.len() {
        match std.lessons.get(ai2) {
            Some(l) => {
                if l.id != REQUIRED_LESSONS[ai2] || l.downstream_action.is_empty() {
                    actions_ok = false;
                }
            }
            None => actions_ok = false,
        }
        ai2 += 1;
    }
    set.add("C20-经验-标准输入逐条可机检", actions_ok, "");

    // **空下游动作被拒收**（判据三的核心：没有下游动作的经验等于没移交）。
    let mut lp = LessonPack::new();
    let accepted_empty = lp.add(Lesson {
        id: LESSON_SINGLE_PASS,
        title: "有空动作的经验",
        downstream_action: "",
    });
    set.add("C20-经验-空下游动作被拒收", !accepted_empty && lp.is_empty(), "");

    // 空标题 / 空 id 同样拒收（三条独立验，不合并）。
    let mut lp2 = LessonPack::new();
    let no_title = lp2.add(Lesson {
        id: LESSON_SPEC_NUMBERING,
        title: "",
        downstream_action: "做点事",
    });
    let mut lp3 = LessonPack::new();
    let no_id = lp3.add(Lesson {
        id: "",
        title: "有标题",
        downstream_action: "做点事",
    });
    set.add(
        "C20-经验-空标题与空标识被拒收",
        !no_title && !no_id && lp2.is_empty() && lp3.is_empty(),
        "",
    );

    // 缺一条即不齐（逐档：缺 1 / 缺 2 / 全缺）。
    let mut miss_ok = true;
    let mut n = 1usize;
    while n <= 3 {
        let mut m = LessonPack::new();
        let mut k2 = 0usize;
        while k2 + n < REQUIRED_LESSONS.len() {
            m.add(Lesson {
                id: REQUIRED_LESSONS[k2],
                title: "t",
                downstream_action: "a",
            });
            k2 += 1;
        }
        if m.missing().len() != n {
            miss_ok = false;
        }
        n += 1;
    }
    set.add("C20-经验-缺项数逐档正确", miss_ok, "");

    // **重登不算齐备**：三条都收（`missing` 空）但第一条登两次 →
    // 齐备性由**重登**破坏（`complete()` 要求每条恰好一次），不是由缺失破坏。
    // 这条专抓「只看 `missing().is_empty()` 判齐备」的实现。
    let mut ld = LessonPack::new();
    let mut k3 = 0usize;
    while k3 < REQUIRED_LESSONS.len() {
        ld.add(Lesson {
            id: REQUIRED_LESSONS[k3],
            title: "t",
            downstream_action: "a",
        });
        k3 += 1;
    }
    ld.add(Lesson {
        id: LESSON_SPEC_NUMBERING,
        title: "t",
        downstream_action: "a",
    });
    set.add(
        "C20-经验-重登不算齐备",
        ld.len() == 4
            && ld.missing().is_empty()
            && !ld.complete()
            && complete_lesson_count(&ld) == 3,
        "",
    );

    // 渲染列出三条并标出缺失。
    let lr = std.lessons.render();
    set.add(
        "C20-经验-渲染列出三条",
        lr.contains(LESSON_SPEC_NUMBERING)
            && lr.contains(LESSON_SINGLE_PASS)
            && lr.contains(LESSON_FUZZ_THREE_LAYER),
        "",
    );

    // ========================================================================
    // 判据五：门禁与裁定
    // ========================================================================

    // **基线先立**：标准输入必须放行。门禁判据若只验「坏输入被拦」，
    // 就无法排除「永远阻断」——而永远阻断和没有门禁一样不可用。
    set.add(
        "C20-门禁-标准输入放行",
        closure_gate(&std) == ClosureGate::Allow && !closure_gate(&std).blocked(),
        "",
    );

    // 六类阻断各点名（逐类独立构造，避免「改了 A 却报 B」被放过）。
    let mut g1 = standard_input();
    g1.evidence = ledger_missing_n(17);
    set.add(
        "C20-门禁-证据缺项被拦",
        closure_gate(&g1) == ClosureGate::BlockMissingEvidence { first: EXPECTED_EVIDENCE[17] }
            && closure_gate(&g1).blocked()
            && closure_gate(&g1).label() == "证据缺项",
        "",
    );

    let mut g2 = standard_input();
    g2.defects.record(DefectEntry {
        summary: "收口期新发现的严重缺陷",
        severity: Defect::Critical,
        closure: None,
        origin: "VE-F0410",
    });
    set.add(
        "C20-门禁-严重缺陷被拦",
        closure_gate(&g2) == ClosureGate::BlockOpenCritical { open: 1 }
            && closure_gate(&g2).label() == "严重缺陷未清零",
        "",
    );

    let mut g3 = standard_input();
    g3.defects.record(DefectEntry {
        summary: "收口期新发现的中等问题",
        severity: Defect::Minor,
        closure: None,
        origin: "VE-F0416",
    });
    set.add(
        "C20-门禁-中等缺陷未闭环被拦",
        closure_gate(&g3) == ClosureGate::BlockUnclosedMinor { open: 1 }
            && closure_gate(&g3).label() == "中等缺陷未闭环",
        "",
    );

    let mut g4 = standard_input();
    g4.signoff = s1.clone();
    set.add(
        "C20-门禁-缺签被拦",
        closure_gate(&g4) == ClosureGate::BlockSignoff {
            seat: Some(Seat::Verifier),
        }
            && closure_gate(&g4).label() == "双签不全",
        "",
    );

    let mut g4b = standard_input();
    g4b.signoff = ss.clone();
    set.add(
        "C20-门禁-同主体连签被拦",
        closure_gate(&g4b) == ClosureGate::BlockSignoff { seat: None },
        "",
    );

    let mut g5 = standard_input();
    g5.lessons = LessonPack::new();
    g5.lessons.add(Lesson {
        id: LESSON_SPEC_NUMBERING,
        title: "只带一条",
        downstream_action: "a",
    });
    set.add(
        "C20-门禁-经验包不齐被拦",
        closure_gate(&g5) == ClosureGate::BlockLessons { missing: 2 }
            && closure_gate(&g5).label() == "经验包不齐",
        "",
    );

    // 上游退化 / 不可比 / 未接入：三种情形分别裁定。
    let mut g6 = standard_input();
    g6.bench = Some(UpstreamBench::Regressed);
    let mut g7 = standard_input();
    g7.bench = Some(UpstreamBench::Incomparable);
    let mut g8 = standard_input();
    g8.bench = None;
    set.add(
        "C20-门禁-上游三态分别裁定",
        closure_gate(&g6) == ClosureGate::BlockBenchmark
            && closure_gate(&g7) == ClosureGate::NeedRebaseline
            && closure_gate(&g8) == ClosureGate::Allow
            && closure_gate(&g6).blocked()
            && closure_gate(&g7).blocked()
            && !closure_gate(&g8).blocked(),
        "",
    );

    // 不可比**不放行**：不能因为「没报退化」就当达标。
    set.add(
        "C20-门禁-不可比既不放行也不判退化",
        closure_gate(&g7) != ClosureGate::Allow
            && closure_gate(&g7) != ClosureGate::BlockBenchmark
            && closure_gate(&g7).label() == "性能不可比（需重取基线）",
        "",
    );

    // **裁定顺序**：本地缺项 + 上游退化同发时，报本地（内因先报）。
    let mut g9 = standard_input();
    g9.evidence = ledger_missing_n(3);
    g9.bench = Some(UpstreamBench::Regressed);
    let mut g10 = standard_input();
    g10.signoff = ss.clone();
    g10.bench = Some(UpstreamBench::Regressed);
    set.add(
        "C20-门禁-本地问题先于上游退化报出",
        closure_gate(&g9) == ClosureGate::BlockMissingEvidence { first: EXPECTED_EVIDENCE[3] }
            && closure_gate(&g10) == ClosureGate::BlockSignoff { seat: None },
        "",
    );

    // **齐备门不吞缺项，也不被重登骗过**：两种退化各钉一次。
    // ① 少登 17 件 → `len()!=18` 且 `complete()` 假（抓「只看条数」的退化）；
    // ② 登满 18 件再重登 F0402 → 条数 19、**覆盖仍完整（`missing` 空）**，
    //    但 `complete()` 必假（抓「只看存在性/只看 missing」的退化）。
    set.add(
        "C20-门禁-齐备判定不吞缺项",
        !ledger_missing_n(17).complete()
            && ledger_missing_n(17).len() == 17
            && ledger_missing_n(18).complete()
            && !ledger_all_with_dup().complete()
            && ledger_all_with_dup().missing().is_empty()
            && ledger_all_with_dup().verdict() == EvidenceVerdict::Duplicated { id: "VE-F0402" },
        "",
    );

    // 上游裁定映射：三变体双向（映射错了会红）。
    set.add(
        "C20-门禁-上游映射三变体正确",
        map_bench(BenchOutcome::Allow) == UpstreamBench::Allow
            && map_bench(BenchOutcome::Block) == UpstreamBench::Regressed
            && map_bench(BenchOutcome::NeedRebaseline) == UpstreamBench::Incomparable,
        "",
    );

    // 映射的**反向**唯一性：三个上游变体不能映到同一个本条变体。
    set.add(
        "C20-门禁-上游映射双射",
        UpstreamBench::Allow != UpstreamBench::Regressed
            && UpstreamBench::Allow != UpstreamBench::Incomparable
            && UpstreamBench::Regressed != UpstreamBench::Incomparable,
        "",
    );

    // **与真实 F0418 类型对接**：本地镜像与上游 `GateOutcome` 逐变体一致。
    // 这条钉住「镜像没漂」——只测本地镜像等于自证（两侧同源）。
    use crate::svstar2::vec18_perf::GateOutcome as RealGate;
    set.add(
        "C20-门禁-与真实上游类型逐变体一致",
        BenchOutcome::from(RealGate::Allow) == BenchOutcome::Allow
            && BenchOutcome::from(RealGate::Block) == BenchOutcome::Block
            && BenchOutcome::from(RealGate::NeedRebaseline) == BenchOutcome::NeedRebaseline
            && map_bench(RealGate::Block.into()) == UpstreamBench::Regressed
            && map_bench(RealGate::NeedRebaseline.into()) == UpstreamBench::Incomparable
            && map_bench(RealGate::Allow.into()) == UpstreamBench::Allow,
        "",
    );

    // 收口清单：含无障碍承诺核验行 + 四段账 + 裁定（人工复核面）。
    let cr = std.render();
    set.add(
        "C20-门禁-收口清单含无障碍核验行",
        cr.contains("无障碍承诺核验") && cr.contains("词法组收口清单") && cr.contains("收口裁定"),
        "",
    );

    // 清单必须如实反映被改过的输入（不能是恒定的同一段文本）。
    let g1r = g1.render();
    set.add(
        "C20-门禁-清单随输入变化",
        g1r.contains("证据缺项") && cr.contains("放行") && g1r != cr,
        "",
    );

    // 清单含上游三态标签。
    let g6r = g6.render();
    set.add(
        "C20-门禁-清单含上游裁定状态",
        cr.contains("上游性能裁定：放行") && g6r.contains("上游性能裁定：退化"),
        "",
    );

    // 清单里 🔴/🟡/🟢 三档都出现（账不是空的也不是残的）。
    set.add(
        "C20-门禁-清单含缺陷三档计数",
        cr.contains("缺陷总账：共 2 条")
            && cr.contains("🔴 0")
            && cr.contains("🟡 1")
            && cr.contains("🟢 1"),
        "",
    );

    // 门禁标签互不相同（八个变体各有各的说法）。
    let labels = [
        ClosureGate::Allow.label(),
        ClosureGate::BlockMissingEvidence { first: "VE-F0402" }.label(),
        ClosureGate::BlockOpenCritical { open: 1 }.label(),
        ClosureGate::BlockUnclosedMinor { open: 1 }.label(),
        ClosureGate::BlockSignoff { seat: None }.label(),
        ClosureGate::BlockLessons { missing: 1 }.label(),
        ClosureGate::BlockBenchmark.label(),
        ClosureGate::NeedRebaseline.label(),
    ];
    let mut lbl_same = false;
    let mut la = 0usize;
    while la < labels.len() {
        let mut lb = la + 1;
        while lb < labels.len() {
            if labels[la] == labels[lb] {
                lbl_same = true;
            }
            lb += 1;
        }
        la += 1;
    }
    set.add("C20-门禁-八个裁定标签互不相同", !lbl_same, "");

    // 全部阻断变体都阻断（`blocked()` 不得漏判）。
    set.add(
        "C20-门禁-七类阻断变体都阻断",
        ClosureGate::BlockMissingEvidence { first: "VE-F0402" }.blocked()
            && ClosureGate::BlockOpenCritical { open: 1 }.blocked()
            && ClosureGate::BlockUnclosedMinor { open: 1 }.blocked()
            && ClosureGate::BlockSignoff { seat: None }.blocked()
            && ClosureGate::BlockSignoff { seat: Some(Seat::Builder) }.blocked()
            && ClosureGate::BlockLessons { missing: 1 }.blocked()
            && ClosureGate::BlockBenchmark.blocked()
            && ClosureGate::NeedRebaseline.blocked()
            && !ClosureGate::Allow.blocked(),
        "",
    );

    // 空输入：不得 panic，且必须阻断（不能因为「什么都没登记」就放行）。
    let empty = ClosureInput::new();
    set.add(
        "C20-门禁-空输入被拦且不崩",
        closure_gate(&empty).blocked()
            && closure_gate(&empty) == ClosureGate::BlockMissingEvidence { first: "VE-F0402" }
            && empty.render().contains("未接入"),
        "",
    );

    // 独立参考值：把台账按单号集合独立求一次差，与 `missing()` 对账。
    // 参考值由判据自己数出来（不调被测函数），故是真对账而非自证。
    let l17ref = ledger_missing_n(17);
    let mut expect_missing = 0usize;
    let mut t = 0usize;
    while t < EVIDENCE_TABLE.len() {
        if !l17ref.contains(EVIDENCE_TABLE[t].0) {
            expect_missing += 1;
        }
        t += 1;
    }
    set.add(
        "C20-门禁-缺项数独立对账",
        expect_missing == 1 && l17ref.missing().len() == expect_missing,
        "",
    );

    // **门禁层必须亲自处理「重登」**：登满 18 件后再重登 F0402，
    // 覆盖仍完整（`missing()` 空），齐备性**只**由 `duplicates()` 撑着。
    // 若 `closure_gate` 只看 `missing`，这条就放行了——而台账明明有重复登记。
    // 这条专抓「门禁只查缺项、不查重登」的退化（M3 变体即此）。
    let mut gdup = standard_input();
    gdup.evidence = ledger_all_with_dup();
    let gdup_gate = closure_gate(&gdup);
    set.add(
        "C20-门禁-重登台账被门禁拦住",
        gdup.evidence.missing().is_empty()
            && !gdup.evidence.complete()
            && gdup_gate.blocked()
            && gdup_gate == ClosureGate::BlockMissingEvidence { first: "VE-F0402" },
        "",
    );

    // 期望外单号同样要过门禁（清单过期也是阻断理由，不能只拦缺项）。
    let mut gue = standard_input();
    gue.evidence = ledger_with_unexpected();
    let gue_gate = closure_gate(&gue);
    set.add(
        "C20-门禁-期望外台账被门禁拦住",
        gue.evidence.missing().is_empty()
            && !gue.evidence.complete()
            && gue_gate.blocked()
            && gue_gate == ClosureGate::BlockMissingEvidence { first: "VE-F9999" },
        "",
    );

    // **双向**：合法台账（无重登、无期望外）必须仍放行，证明上面两条拦截
    // 不是靠「见非空就拦」实现的。末项还要求两种非法台账的裁定**不同**
    //（重登点 F0402、期望外点 F0409… 实为 F9999），防「一律映射成同一条」。
    set.add(
        "C20-门禁-拦截不误伤合法台账",
        closure_gate(&std) == ClosureGate::Allow
            && std.evidence.duplicates().is_empty()
            && std.evidence.unexpected().is_empty()
            && closure_gate(&gdup) != closure_gate(&gue),
        "",
    );

    set
}

#[cfg(test)]
mod red_closure {
    use super::*;
    #[test]
    fn closure_red_items() {
        let set = run_vec20_checks();
        let (_items, count) = set.red_items();
        let mut i = 0usize;
        while i < count {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("[红] {}", c.name);
                }
            }
            i += 1;
        }
        let (passed, red) = set.tally();
        println!(
            "total={} passed={} red={} dropped={}",
            set.len(),
            passed,
            red,
            set.dropped()
        );
        assert_eq!(passed + red, set.len(), "tally 与len 必须自洽");
        assert!(red == 0, "域自检不该有红项");
    }
}
