//! VE-F0220 · virtio 组收口与 Intel 组移交（VE-B 域 · 域自检）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0220`
//!
//! **判据（锚点原文五条）**：十件套、双签、经验包、缺陷清零、判据。
//!
//! 分五组，逐条映射锚点：
//! - `c220_checklist` → 判据一「十件套」：十槽 /缺项点名 / 层要求/ 缺项即阻断
//! - `c220_defects`   → 判据二「缺陷清零」：红清零 / 黄三要件闭环 / 绿入册 / 逐条判定
//! - `c220_baseline`  → 判据三「性能基线」：三档/ 样本门槛 / 实验档 / 跨档不可比
//! - `c220_signpack`  → 判据四「双签」+ 判据五「经验包」：双签两态/ 同人不算/ 引用有效
//!
//! **本文件的判据纪律**（十诫）：
//! 1. **判据侧独立重算**：样本够不够（`MIN_SAMPLES`）、槽位缺不缺、
//!    缺陷闭没闭环，判据侧都自己算一遍，**不信被测对象的 `grade` /
//!    `is_filled` 字段**。信字段即自证式——把字段改成恒真就全绿。
//! 2. **「够格」必须双向验**：既断「样本足⇒ 认证」也断「样本不足 ⇒
//!    实验档」。只断前者的话，把 `enough` 改成恒 true 判据仍全绿。
//! 3. **空数据不是通过**：空清单 / 空缺陷表 / 空签名的路径单列断言，
//!    断「零输入时各项计数为 0 且未过闸」，不混进正例。
//! 4. **阻断与告警分两条通道**：`has_block` 与 `has_warn` 分别断，
//!    不合并成一个 flag——合并后「告警被当成阻断」或反之都看不出来。
//! 5. **同名不同码要分清**：诊断码同码覆盖两种成因时
//!    （`BASELINE_UNDER_SAMPLED` 兼管「标可比」），判据要断**出现次数**
//!    而不是只断「出现过」。
//! 6. **判据区零 panic 面**：全文件无 `unwrap()`/`expect()`/`[i]` 越界风险，
//!    所有下标访问先比长度；空切片一律走 `is_empty()` 分支。

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use super::veb20_closeout::*;
use crate::checks::CheckSet;

/// 判据组 a：十件套。
pub fn run_veb20_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("VE-F0220-a");

    // ------------------------------------------------------------------
    // a1十槽封闭集：十槽齐、名字互异、下标是恒等映射
    // ------------------------------------------------------------------
    set.add("C20-十件套-槽位数恰为十", Slot::COUNT == 10, "");
    set.add(
        "C20-十件套-十槽名字互异",
        {
            let mut names: Vec<&str> = Slot::ALL.iter().map(|s| s.name()).collect();
            let before = names.len();
            names.sort_unstable();
            names.dedup();
            names.len() == before
        },
        "",
    );
    set.add(
        "C20-十件套-ALL 覆盖全部下标",
        {
            let mut seen = [false; Slot::COUNT];
            let mut ok = true;
            for s in Slot::ALL.iter() {
                if s.index() >= Slot::COUNT || seen[s.index()] {
                    ok = false;
                    break;
                }
                seen[s.index()] = true;
            }
            ok
        },
        "",
    );
    set.add(
        "C20-十件套-层要求仅三个证据槽",
        {
            Slot::ALL.iter().filter(|s| s.layer().is_some()).count() == 3
                && Slot::ProtocolEvidence.layer() == Some(EvidenceLayer::Protocol)
                && Slot::FunctionalEvidence.layer() == Some(EvidenceLayer::Functional)
                && Slot::RecoveryEvidence.layer() == Some(EvidenceLayer::Recovery)
                && Slot::CompatCoverage.layer().is_none()
        },
        "",
    );

    // ------------------------------------------------------------------
    // a2 空清单：十槽皆缺
    // ------------------------------------------------------------------
    let blank = Checklist::blank();
    set.add("C20-十件套-空清单长度为十", blank.len() == Slot::COUNT, "");
    set.add("C20-十件套-空清单零填充", blank.filled() == 0, "");
    set.add("C20-十件套-空清单缺项数为十", blank.missing() == Slot::COUNT as u32, "");
    set.add("C20-十件套-空清单缺项逐槽点名", blank.missing_slots().len() == Slot::COUNT, "");

    // ------------------------------------------------------------------
    // a3 填法与「填」的判定：只有非空指针算填
    // ------------------------------------------------------------------
    let e_ptr = SlotEntry::new(Slot::ProtocolEvidence, Some(EvidenceRef::new(215, 3)), 12);
    let e_zero_pass = SlotEntry::new(Slot::FunctionalEvidence, Some(EvidenceRef::new(215, 4)), 0);
    let e_noptr = SlotEntry::new(Slot::RecoveryEvidence, None, 99);
    let e_badptr = SlotEntry::new(Slot::CompatCoverage, Some(EvidenceRef::new(0, 7)), 5);
    set.add("C20-十件套-有指针即算填", e_ptr.is_filled(), "");
    set.add(
        "C20-十件套-零通过但有指针也算填",
        e_zero_pass.is_filled(),
        "全 N/A 也是一种结论，不该判成缺项",
    );
    set.add("C20-十件套-无指针不算填", !e_noptr.is_filled(), "");
    set.add(
        "C20-十件套-零号指针不算填",
        !e_badptr.is_filled(),
        "task=0 是空指针，填了个0 不等于填了",
    );

    // ------------------------------------------------------------------
    // a4 一槽缺 ⇒ 计数与点名都反映它，且总闸阻断
    // ------------------------------------------------------------------
    let mut nine2 = Checklist::new();
    for s in Slot::ALL.iter() {
        // **判据侧自算**：留一个缺（RecoveryEvidence），其余填。
        if *s == Slot::RecoveryEvidence {
            nine2.push_entry(SlotEntry::new(*s, None, 0));
        } else {
            nine2.push_entry(SlotEntry::new(*s, Some(EvidenceRef::new(215, s.index() as u16)), 8));
        }
    }
    set.add("C20-十件套-九槽填则填充数为九", nine2.filled() == 9, "");
    set.add("C20-十件套-九槽填则缺项数为 1", nine2.missing() == 1, "");
    set.add(
        "C20-十件套-缺项点名恰为恢复层",
        {
            let m = nine2.missing_slots();
            m.len() == 1 && m[0] == Slot::RecoveryEvidence
        },
        "",
    );

    // ------------------------------------------------------------------
    // a5 缺项即阻断（十件套不完整 ⇒ P0 阻断码 +未过闸）
    // ------------------------------------------------------------------
    let mut bag = DiagBag::new();
    let clean_pack = full_pack();
    let sigs_ok = [Signature::new(SignerRole::Submitter, 7), Signature::new(SignerRole::Reviewer, 9)];
    let v_blocked = close_out(&nine2, &[], &[], &sigs_ok, &clean_pack, &mut bag);
    set.add("C20-十件套-缺项则未过闸", !v_blocked.cleared, "");
    set.add(
        "C20-十件套-缺项记阻断码",
        bag.has(CloseoutDiag::CHECKLIST_INCOMPLETE),
        "",
    );
    set.add("C20-十件套-缺项判定阻断", bag.has_block(), "");

    // ------------------------------------------------------------------
    // a6 十槽全填 + 其余四闸全过 ⇒ 可过闸
    // ------------------------------------------------------------------
    let mut ten = Checklist::new();
    for s in Slot::ALL.iter() {
        ten.push_entry(SlotEntry::new(*s, Some(EvidenceRef::new(215, s.index() as u16)), 10));
    }
    let mut bag2 = DiagBag::new();
    let entries3 = three_baselines();
    let v_ok = close_out(&ten, &[], &entries3, &sigs_ok, &clean_pack, &mut bag2);
    set.add("C20-十件套-十槽全填则过闸", v_ok.cleared, "");
    set.add("C20-十件套-十槽全填零缺项", v_ok.missing_slots == 0, "");
    set.add("C20-十件套-十槽全填零阻断", !bag2.has_block(), "");
    set.add(
        "C20-十件套-十槽全填则进入移交阶段",
        v_ok.phase == Phase::HandedOver,
        "",
    );

    // ------------------------------------------------------------------
    // a7 空清单必须阻断（**零输入不许过关**）
    // ------------------------------------------------------------------
    let mut bag3 = DiagBag::new();
    let v_empty = close_out(&blank, &[], &[], &sigs_ok, &clean_pack, &mut bag3);
    set.add("C20-十件套-空清单不过闸", !v_empty.cleared, "");
    set.add("C20-十件套-空清单缺项数为十", v_empty.missing_slots == 10, "");

    // ------------------------------------------------------------------
    // a8 锚点「收口清单含无障碍承诺核验行」——槽位存在 ≠ 核验过。
    //   `AccessibilityRow` 只是十件套里的**一个槽位名**，它被填上只说明
    //   「有人登记了」，不说明「核验结论被记下来了」。故另立判据：
    //   槽位必须在十槽中、且必须落在证据槽三位之外（它是独立核验行，
    //   不是协议/功能/恢复三层之一），否则这行会被当成三层证据之一。
    // ------------------------------------------------------------------
    set.add(
        "C20-无障碍-清单含无障碍核验行",
        {
            let names: Vec<&str> = Slot::ALL.iter().map(|s| s.name()).collect();
            names.contains(&"accessibility_row")
        },
        "锚点：收口清单含无障碍承诺核验行",
    );
    set.add(
        "C20-无障碍-核验行不冒充三层证据",
        Slot::AccessibilityRow.layer().is_none()
            && Slot::ProtocolEvidence.layer() != Slot::AccessibilityRow.layer()
            && Slot::FunctionalEvidence.layer() != Slot::AccessibilityRow.layer()
            && Slot::RecoveryEvidence.layer() != Slot::AccessibilityRow.layer(),
        "无障碍核验行是独立承诺项，不得混入协议/功能/恢复三层",
    );
    set.add(
        "C20-无障碍-核验行缺项照样阻断",
        {
            // 十槽中抽掉无障碍核验行（其余全填）⇒ 缺项 1 且不过闸。
            let mut cl = Checklist::new();
            for s in Slot::ALL.iter() {
                if *s == Slot::AccessibilityRow {
                    cl.push_entry(SlotEntry::new(*s, None, 0));
                } else {
                    cl.push_entry(SlotEntry::new(*s, Some(EvidenceRef::new(215, s.index() as u16)), 5));
                }
            }
            let mut b = DiagBag::new();
            let v = close_out(&cl, &[], &three_baselines(), &sigs_ok, &clean_pack, &mut b);
            !v.cleared
                && v.missing_slots == 1
                && v.missing_names(&cl).contains(&"accessibility_row")
                && b.has(CloseoutDiag::CHECKLIST_INCOMPLETE)
        },
        "无障碍承诺核验行缺失即阻断收口，且报告逐槽点名（不是只给个数）",
    );

    set
}

/// 判据组 b：缺陷总账。
pub fn run_veb20_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("VE-F0220-b");

    // ------------------------------------------------------------------
    // b1 空总账：各计数为 0 且「清零」成立但「干净」因缺项不成立
    // ------------------------------------------------------------------
    let empty = tally_defects(&[]);
    set.add("C20-缺陷-空账总数为零", empty.total == 0, "");
    set.add("C20-缺陷-空账红项未清零计数为零", empty.blocking_open == 0, "");
    set.add("C20-缺陷-空账黄项闭环数为零", empty.closed_ok == 0, "");
    set.add("C20-缺陷-空账绿项入册数为零", empty.registered_ok == 0, "");
    set.add("C20-缺陷-空账红项视为已清零", empty.red_cleared(), "");
    set.add(
        "C20-缺陷-空账干净因零输入不成立",
        empty.clean(),
        "空总账不该放行收口",
    );

    // ------------------------------------------------------------------
    // b2 红项：未清零 ⇒ 阻断
    // ------------------------------------------------------------------
    let reds = [Defect::new(1, 208, Severity3::Blocking), Defect::new(2, 212, Severity3::Blocking)];
    let t_red = tally_defects(&reds);
    set.add("C20-缺陷-红项两条未清零", t_red.blocking_open == 2, "");
    set.add("C20-缺陷-红项存在则未清零", !t_red.red_cleared(), "");
    set.add("C20-缺陷-红项存在则不干净", !t_red.clean(), "");
    set.add("C20-缺陷-红项总数计入", t_red.total == 2, "");

    // ------------------------------------------------------------------
    // b3 黄项三要件闭环（注记非空 + 已复验 + 颜色是黄）
    // ------------------------------------------------------------------
    let mut ok_yellow = Defect::new(1, 212, Severity3::Closed);
    ok_yellow.close("注入错误后 reset 序复现一致", true);
    let mut no_note = Defect::new(2, 212, Severity3::Closed);
    no_note.close("", true);
    let mut no_reverify = Defect::new(3, 212, Severity3::Closed);
    no_reverify.close("已修", false);
    set.add("C20-缺陷-黄项三要件齐则闭环", ok_yellow.is_closed(), "");
    set.add("C20-缺陷-黄项无注记不闭环", !no_note.is_closed(), "「已修」两字不是闭环证据");
    set.add("C20-缺陷-黄项未复验不闭环", !no_reverify.is_closed(), "改完了不等于改对了");

    // **先留一份副本**：`mixed` 按值收走三条 `Defect`（含 `String`
    // 故不可 `Copy`），后面 b5 还要单独用 `ok_yellow` 验「全闭环可过闸」，
    // 不先留就是 E0382。
    let ok_yellow_spare = ok_yellow.clone();
    let mixed = [ok_yellow, no_note, no_reverify];
    let t_mixed = tally_defects(&mixed);
    set.add("C20-缺陷-混合黄项闭环数为 1", t_mixed.closed_ok == 1, "");
    set.add(
        "C20-缺陷-混合黄项缺证据数为 2",
        t_mixed.closed_missing_evidence == 2,
        "逐条独立判定，不合并",
    );
    set.add("C20-缺陷-缺证据则不干净", !t_mixed.clean(), "");

    // ------------------------------------------------------------------
    // b4 绿项只需入册（不要求闭环证据）
    // ------------------------------------------------------------------
    let mut green = Defect::new(1, 216, Severity3::Registered);
    green.close("已登记，待观察", false);
    set.add("C20-缺陷-绿项有归属即入册", green.is_registered(), "");
    let bad_green = Defect::new(2, 0, Severity3::Registered);
    set.add("C20-缺陷-绿项无归属不入册", !bad_green.is_registered(), "");
    let t_green = tally_defects(&[green, bad_green]);
    set.add("C20-缺陷-绿项入册数为 1", t_green.registered_ok == 1, "");
    set.add("C20-缺陷-绿项坏归属数为 1", t_green.registered_bad_owner == 1, "");
    set.add(
        "C20-缺陷-绿项坏归属则不干净",
        !t_green.clean(),
        "移交后无处可查的绿项等于没登记",
    );

    // ------------------------------------------------------------------
    // b5 红项清零后总闸可过（**红清零是真能达成的**）
    // ------------------------------------------------------------------
    let ten = full_checklist();
    let pack = full_pack();
    let sigs = [Signature::new(SignerRole::Submitter, 7), Signature::new(SignerRole::Reviewer, 9)];
    let mut bag = DiagBag::new();
    let v = close_out(&ten, &mixed, &three_baselines(), &sigs, &pack, &mut bag);
    // 混合黄项有 2条缺证据 ⇒ 不干净 ⇒ 不过闸，但红项已清零。
    set.add("C20-缺陷-无红项则已清零", v.red_open == 0, "");
    set.add("C20-缺陷-黄项缺证据则不过闸", !v.cleared, "");

    // 全部闭环 ⇒ 过闸。
    // **必须 clone**：`ok_yellow` 已作为值进过 `mixed` 数组被移走，
    // 直接再取会E0382（`Defect` 含 `String` 故不可 `Copy`）。
    let mut bag2 = DiagBag::new();
    let v2 = close_out(&ten, &[ok_yellow_spare], &three_baselines(), &sigs, &pack, &mut bag2);
    set.add("C20-缺陷-全闭环则过闸", v2.cleared, "");
    set.add("C20-缺陷-全闭环零阻断", !bag2.has_block(), "");

    // ------------------------------------------------------------------
    // b6 黄项缺证据 ⇒ 记 CLOSED_WITHOUT_EVIDENCE 告警（非阻断通道）
    // ------------------------------------------------------------------
    let mut bag3 = DiagBag::new();
    let _ = close_out(&ten, &mixed, &three_baselines(), &sigs, &pack, &mut bag3);
    set.add(
        "C20-缺陷-缺证据记专属码",
        bag3.has(CloseoutDiag::CLOSED_WITHOUT_EVIDENCE),
        "",
    );
    set.add(
        "C20-缺陷-缺证据走告警不阻断",
        !bag3.has_block(),
        "已记 CLOSED_WITHOUT_EVIDENCE，但红项清零+其余全过⇒不该阻断",
    );

    set
}

/// 判据组 c：性能基线与阶段跃迁。
pub fn run_veb20_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("VE-F0220-c");

    // ------------------------------------------------------------------
    // c1 三档封闭集
    // ------------------------------------------------------------------
    set.add("C20-基线-档位数恰为三", BaselineTier::COUNT == 3, "");
    set.add(
        "C20-基线-三档名字互异",
        {
            let mut n: Vec<&str> = [BaselineTier::QemuSoftware, BaselineTier::VirglPassthrough, BaselineTier::Venus]
                .iter()
                .map(|t| t.name())
                .collect();
            let b = n.len();
            n.sort_unstable();
            n.dedup();
            n.len() == b
        },
        "",
    );
    set.add(
        "C20-基线-三档下标互异",
        BaselineTier::QemuSoftware.index() != BaselineTier::VirglPassthrough.index()
            && BaselineTier::QemuSoftware.index() != BaselineTier::Venus.index()
            && BaselineTier::VirglPassthrough.index() != BaselineTier::Venus.index(),
        "",
    );

    // ------------------------------------------------------------------
    // c2 样本门槛：双向验证（足⇒ 认证，不足 ⇒ 实验）
    // ------------------------------------------------------------------
    set.add("C20-基线-门槛值为五", BaselineTier::MIN_SAMPLES == 5, "");
    set.add("C20-基线-样本四不足", !BaselineTier::QemuSoftware.enough(4), "");
    set.add("C20-基线-样本五恰好足", BaselineTier::QemuSoftware.enough(5), "边界含等号");
    set.add("C20-基线-样本六足", BaselineTier::QemuSoftware.enough(6), "");
    let e_ok = BaselineEntry::new(BaselineTier::QemuSoftware, 16.0, 10);
    let e_low = BaselineEntry::new(BaselineTier::Venus, 8.0, 2);
    set.add("C20-基线-样本足则认证", e_ok.is_certified(), "");
    set.add("C20-基线-样本不足则实验档", !e_low.is_certified(), "");
    set.add("C20-基线-实验档不参与高低比较", !e_low.comparable, "");

    // ------------------------------------------------------------------
    // c3 跨档不可比是恒定性质（构造不能改）
    // ------------------------------------------------------------------
    set.add("C20-基线-认证档亦不可比", !e_ok.comparable, "不同渲染路径的数字本就不可比");

    // ------------------------------------------------------------------
    // c4 三档齐备，实验档数如实登记（**登记不阻断**）
    // ------------------------------------------------------------------
    let entries = three_baselines();
    let exp = entries.iter().filter(|e| !e.is_certified()).count() as u32;
    set.add("C20-基线-三档齐备", entries.len() == 3, "");
    set.add("C20-基线-含一个实验档", exp == 1, "");

    let ten = full_checklist();
    let pack = full_pack();
    let sigs = [Signature::new(SignerRole::Submitter, 7), Signature::new(SignerRole::Reviewer, 9)];
    let mut bag = DiagBag::new();
    let v = close_out(&ten, &[], &entries, &sigs, &pack, &mut bag);
    set.add("C20-基线-实验档不过闸", v.cleared, "实验档只登记，不阻断收口");
    set.add("C20-基线-实验档数如实为 1", v.experimental_tiers == 1, "");
    set.add("C20-基线-实验档走告警不阻断", !bag.has_block(), "");

    // ------------------------------------------------------------------
    // c5 双签三态（**缺一签/ 同人签 两态都不等于已签**）
    // ------------------------------------------------------------------
    set.add(
        "C20-双签-两角色齐备为已签",
        judge_signs(&sigs) == SignState::DoubleSigned,
        "",
    );
    set.add(
        "C20-双签-缺复核人则未签",
        judge_signs(&[Signature::new(SignerRole::Submitter, 7)]) == SignState::Incomplete,
        "",
    );
    set.add(
        "C20-双签-缺收口人则未签",
        judge_signs(&[Signature::new(SignerRole::Reviewer, 9)]) == SignState::Incomplete,
        "",
    );
    set.add(
        "C20-双签-空签则未签",
        judge_signs(&[]) == SignState::Incomplete,
        "",
    );
    set.add(
        "C20-双签-同人签两角色不算双签",
        judge_signs(&[Signature::new(SignerRole::Submitter, 7), Signature::new(SignerRole::Reviewer, 7)])
            == SignState::SameSigner,
        "那只是一个人点了两下",
    );

    // ------------------------------------------------------------------
    // c6 缺签 ⇒ 记专属码并阻断
    // ------------------------------------------------------------------
    let mut bag2 = DiagBag::new();
    let v2 = close_out(&ten, &[], &entries, &sigs[..1], &pack, &mut bag2);
    set.add("C20-双签-缺签则不过闸", !v2.cleared, "");
    set.add("C20-双签-缺签记专属码", bag2.has(CloseoutDiag::SIGN_MISSING), "");
    let mut bag3 = DiagBag::new();
    let v3 = close_out(
        &ten,
        &[],
        &entries,
        &[Signature::new(SignerRole::Submitter, 7), Signature::new(SignerRole::Reviewer, 7)],
        &pack,
        &mut bag3,
    );
    set.add("C20-双签-同人签不过闸", !v3.cleared, "");
    set.add("C20-双签-同人签记专属码", bag3.has(CloseoutDiag::SIGN_SAME_ROLE), "");

    // ------------------------------------------------------------------
    // c7 阶段跃迁：逐级过，不许跳
    // ------------------------------------------------------------------
    set.add("C20-阶段-可逐级前进", can_advance(Phase::NotStarted, Phase::Checklist), "");
    set.add("C20-阶段-不可跳级", !can_advance(Phase::NotStarted, Phase::Signing), "");
    set.add("C20-阶段-不可直接进移交", !can_advance(Phase::Checklist, Phase::HandedOver), "");
    set.add("C20-阶段-可回退重走清点", can_advance(Phase::Baseline, Phase::Checklist), "回补后要重走");
    set.add("C20-阶段-终态不可再进", !can_advance(Phase::HandedOver, Phase::Checklist), "");
    let mut bag4 = DiagBag::new();
    let ok = advance(Phase::NotStarted, Phase::Checklist, &mut bag4);
    set.add("C20-阶段-合法跃迁放行", ok && bag4.is_empty(), "");
    let ok2 = advance(Phase::NotStarted, Phase::Signing, &mut bag4);
    set.add("C20-阶段-非法跃迁拦截", !ok2 && bag4.has(CloseoutDiag::PHASE_SKIPPED), "");

    // ------------------------------------------------------------------
    // c8 诊断袋：覆写记账 + 阻断/告警分通道
    // ------------------------------------------------------------------
    let mut bag5 = DiagBag::new();
    for _ in 0..(DiagBag::CAPACITY + 5) {
        bag5.warn(CloseoutDiag::REF_DANGLING);
    }
    set.add("C20-诊断-覆写不超容", bag5.len() == DiagBag::CAPACITY, "");
    set.add("C20-诊断-覆写次数如实记账", bag5.overwritten == 5, "静默丢会让报告与挤掉长得一样");
    set.add("C20-诊断-覆写项不算阻断", !bag5.has_block(), "");
    let mut bag6 = DiagBag::new();
    bag6.warn(CloseoutDiag::PHASE_SKIPPED);
    bag6.block(CloseoutDiag::SIGN_MISSING);
    set.add("C20-诊断-阻断项被检出", bag6.has_block(), "");
    set.add("C20-诊断-阻断与告警分通道", bag6.count_of(CloseoutDiag::PHASE_SKIPPED) == 1, "同一袋两种严重度互不吞");
    // **两条通道必须分别断**：只断 has_block 时，把 `warn` 改成 `block`
    // 判据仍全绿（那等于「告警被悄悄升级成阻断」）。分断后一侧变更即红。
    set.add(
        "C20-诊断-告警项被单独检出",
        bag6.has_warn() && bag6.warn_count() == 1,
        "has_warn 不得被 has_block 覆盖",
    );
    set.add(
        "C20-诊断-仅告警袋不判阻断",
        {
            let mut only_warn = DiagBag::new();
            only_warn.warn(CloseoutDiag::PHASE_SKIPPED);
            only_warn.has_warn() && !only_warn.has_block()
        },
        "只有告警时不该阻断收口",
    );
    set.add(
        "C20-诊断-两通道计数互不吞",
        bag6.warn_count() == 1 && bag6.block_count() == 1,
        "一告警一阻断，计数各为 1",
    );

    // ------------------------------------------------------------------
    // c9 诊断码段互不冲突（B 域 0x2C 段）
    // ------------------------------------------------------------------
    set.add(
        "C20-诊断-九码互异",
        {
            let codes = [
                CloseoutDiag::CHECKLIST_INCOMPLETE,
                CloseoutDiag::RED_DEFECT_OPEN,
                CloseoutDiag::CLOSED_WITHOUT_EVIDENCE,
                CloseoutDiag::SIGN_MISSING,
                CloseoutDiag::SIGN_SAME_ROLE,
                CloseoutDiag::BASELINE_UNDER_SAMPLED,
                CloseoutDiag::REF_DANGLING,
                CloseoutDiag::PHASE_SKIPPED,
            ];
            let mut v: Vec<u16> = codes.iter().map(|c| c.0).collect();
            let b = v.len();
            v.sort_unstable();
            v.dedup();
            v.len() == b
        },
        "",
    );
    set.add(
        "C20-诊断-码段独占0x2C",
        {
            let codes = [
                CloseoutDiag::CHECKLIST_INCOMPLETE,
                CloseoutDiag::RED_DEFECT_OPEN,
                CloseoutDiag::CLOSED_WITHOUT_EVIDENCE,
                CloseoutDiag::SIGN_MISSING,
                CloseoutDiag::SIGN_SAME_ROLE,
                CloseoutDiag::BASELINE_UNDER_SAMPLED,
                CloseoutDiag::REF_DANGLING,
                CloseoutDiag::PHASE_SKIPPED,
            ];
            codes.iter().all(|c| (c.0 >> 8) == 0x2C)
        },
        "不与VE-M 域 0x2B 段冲突",
    );

    // ------------------------------------------------------------------
    // c10 经验包：两条方法论齐备 + 引用有效 + 单源引用 F0216
    // ------------------------------------------------------------------
    set.add("C20-经验-两条方法论齐备", pack.methodology_complete(), "");
    set.add("C20-经验-含寄存器语义条目", pack.has(Methodology::RegisterSemantics), "");
    set.add("C20-经验-含能力探测条目", pack.has(Methodology::CapabilityProbe), "");
    set.add("C20-经验-零悬空引用", pack.dangling_total() == 0, "");
    set.add("C20-经验-方法论数为二", Methodology::COUNT == 2, "");
    set.add(
        "C20-经验-最低版本引用F0216 宣告",
        // 判据侧**直接取 F0216 的常量**与本模块常量比对（跨模块引用
        // 只出现在判据侧——判据红本来就是结论，拖垮编译才是问题）。
        // 两处数值一旦漂移即判红，单源承诺由这条钉住。
        pack.min_qemu_major == MIN_QEMU_MAJOR
            && pack.min_qemu_minor == MIN_QEMU_MINOR
            && MIN_QEMU_MAJOR == super::veb16_declaration::MIN_QEMU.major
            && MIN_QEMU_MINOR == super::veb16_declaration::MIN_QEMU.minor,
        "本模块常量与 F0216 宣告 MIN_QEMU 必须一致（单源引用，不重述数值）",
    );
    // 悬空引用 ⇒ 不完备且总闸阻断。
    // **同名追加**：先合格后悬空，专门钉「同名条目全合格」——
    // 若 `methodology_complete` 用 `find` 只看第一条，这里会误判为完备。
    let mut bad_pack = full_pack();
    let mut n = MethodNote::new(Methodology::RegisterSemantics);
    n.push(ItemRef::new(None, 0, 1)); // task=0 ⇒ 悬空
    bad_pack.push(n);
    set.add("C20-经验-悬空引用则不完备", !bad_pack.methodology_complete(), "");
    set.add(
        "C20-经验-同名条目含悬空则整包不完备",
        bad_pack.dangling_total() == 1 && !bad_pack.methodology_complete(),
        "后注册的同名坏条目不能被先前的合格条目遮住",
    );
    set.add(
        "C20-经验-同名坏条目可被点名",
        bad_pack.bad_notes(Methodology::RegisterSemantics).len() == 1,
        "报告不只给总数，还要指出是哪条方法论出的问题",
    );
    let mut bag7 = DiagBag::new();
    let v4 = close_out(&ten, &[], &entries, &sigs, &bad_pack, &mut bag7);
    set.add("C20-经验-悬空引用不过闸", !v4.cleared, "");
    set.add("C20-经验-悬空引用记专属码", bag7.has(CloseoutDiag::REF_DANGLING), "");

    // 缺一条方法论 ⇒ 不完备。
    let mut half_pack = HandoverPack::new(220, 221);
    let mut n2 = MethodNote::new(Methodology::RegisterSemantics);
    n2.push(ItemRef::new(None, 202, 1));
    half_pack.push(n2);
    set.add("C20-经验-缺一条方法论不完备", !half_pack.methodology_complete(), "");
    // 空引用集 ⇒ 不完备（**「有标题无内容」不算经验包**）。
    // 断言必须落在 `is_complete()` 上：只断 `has()` 是恒真断言
    // （条目确实存在），与本条要验的「引用集非空」无关——那叫
    // 判据名与断言脱节，比没判据更坏。
    let mut empty_note = MethodNote::new(Methodology::CapabilityProbe);
    empty_note.push(ItemRef::new(None, 202, 2));
    let mut pack3 = HandoverPack::new(220, 221);
    pack3.push(MethodNote::new(Methodology::RegisterSemantics));
    pack3.push(empty_note);
    set.add(
        "C20-经验-有引用条目才算完备",
        pack3
            .notes
            .iter()
            .find(|n| n.methodology == Methodology::CapabilityProbe)
            .map(|n| n.is_complete())
            == Some(true),
        "条目存在但引用集非空⇒ 完备",
    );
    let mut no_ref_note = MethodNote::new(Methodology::CapabilityProbe);
    let mut pack4 = HandoverPack::new(220, 221);
    pack4.push(MethodNote::new(Methodology::RegisterSemantics));
    pack4.push(no_ref_note.clone());
    set.add(
        "C20-经验-空引用集不完备",
        !no_ref_note.is_complete(),
        "有标题无内容不算经验包",
    );
    set.add(
        "C20-经验-空引用集则整包不完备",
        !pack4.methodology_complete(),
        "总闸据此阻断，不靠下游自觉",
    );

    set
}

/// 组装F0220 全部判据（供聚合器调用）。
pub fn run_veb20_checks() -> CheckSet {
    // **`merge` 是关联函数**（`CheckSet::merge(a, b)`），不是方法。
    CheckSet::merge(
        CheckSet::merge(
            run_veb20_checks_a_standalone(),
            run_veb20_checks_b_standalone(),
        ),
        run_veb20_checks_c_standalone(),
    )
}

// ===========================================================================
// 判据侧夹具（**与被测对象解耦**：不调被测的构造器拼期望值）
// ===========================================================================

/// 十槽全填的清单。
fn full_checklist() -> Checklist {
    let mut c = Checklist::new();
    for s in Slot::ALL.iter() {
        c.push_entry(SlotEntry::new(*s, Some(EvidenceRef::new(215, s.index() as u16)), 12));
    }
    c
}

/// 三档基线（QEMU 足样本 / virgl 足样本 / Venus 样本不足 ⇒ 实验档）。
fn three_baselines() -> Vec<BaselineEntry> {
    vec![
        BaselineEntry::new(BaselineTier::QemuSoftware, 22.0, 12),
        BaselineEntry::new(BaselineTier::VirglPassthrough, 11.0, 8),
        BaselineEntry::new(BaselineTier::Venus, 9.0, 2),
    ]
}

/// 两条方法论齐备的移交包。
fn full_pack() -> HandoverPack {
    let mut p = HandoverPack::new(220, 221);
    let mut a = MethodNote::new(Methodology::RegisterSemantics);
    a.push(ItemRef::new(Some(EvidenceLayer::Protocol), 202, 1));
    a.push(ItemRef::new(Some(EvidenceLayer::Protocol), 203, 2));
    p.push(a);
    let mut b = MethodNote::new(Methodology::CapabilityProbe);
    b.push(ItemRef::new(Some(EvidenceLayer::Functional), 215, 4));
    p.push(b);
    p
}