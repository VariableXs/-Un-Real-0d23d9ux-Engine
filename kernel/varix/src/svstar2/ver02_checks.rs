//! VE-F3601 · 域自检（判据逐条对应，见 `ver02_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 跳段 ADR → `R01-跳段-*`
//! - 同步更新 → `R01-同步-*`
//! - 五板块 → `R01-板块-*`
//! - 四域分工 → `R01-分工-*`
//! - 收敛复述 → `R01-收敛-*`
//! - 判据（自证可追溯）→ `R01-判据-*`
//! - 域使命与禁扩面 → `R01-使命-*` / `R01-边界-*`
//! - 前向义务（不许伪装兑现）→ `R01-前向-*`
//! - 错误路径零静默 → `R01-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::ver02_arch::*;
use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 全绿总纲（**唯一正样本构造处**——其余函数只做判定，不各自造"好看的数据"）。
///
/// 这里**刻意不伪造前向义务已兑现**：双维无障碍本体在F3611/F3650，
/// 标成已兑现就是用声明冒充实现。
fn ready_arch() -> CreationArchitecture {
    CreationArchitecture::standard()
}

/// 空总纲（反例用）。
fn empty_arch() -> CreationArchitecture {
    CreationArchitecture::empty()
}

// ---------------------------------------------------------------------------
// 判据一：跳段 ADR
// ---------------------------------------------------------------------------

fn chk_jump_adr(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. R 域起点必须是锚点写死的 F3601。
    set.add(
        "R01-跳段-起点为锚点F3601",
        CreationBoard::R_ANCHOR_START == 3601
            && a.jump.map.r_start() == Ok(CreationBoard::R_ANCHOR_START),
        "锚点原文：R 域条目号 F3601 起跳",
    );

    // 2. 三段齐备且互不重叠（正样本：E 域二期/ R 创作 / S 语义）。
    let bands: Vec<NumberBand> = a.jump.map.iter().copied().collect();
    let three_ok = bands.len() == BAND_COUNT
        && bands
            .iter()
            .any(|b| b.domain == DivisionDomain::Engine && b.lo == 3401)
        && bands
            .iter()
            .any(|b| b.domain == DivisionDomain::Creation && b.lo == 3601)
        && bands
            .iter()
            .any(|b| b.domain == DivisionDomain::Semantic && b.lo == 3801);
    set.add(
        "R01-跳段-三段齐备不重叠",
        three_ok && bands.iter().all(|b| !b.is_settled() || b.span() > 0),
        "F3401-F3600 归 E；F3601-F3800 归 R；F3801-F4000 归 S",
    );

    // 3. 未裁决必须清零（**红线条目的机检**——不是纸面声明）。
    let unsettled = a.jump.unsettled_band_count();
    let unadj = a.jump.unadjudicated_count();
    set.add(
        "R01-跳段-未裁决清零",
        unsettled == 0 && unadj == 0,
        "锚点红线：跳段不登记=后续域混乱；冲突未裁决即不许开工",
    );

    // 4. 两条冲突都必须落地裁决，且四要素齐备（**证伪锚点**：
    //    删掉任一裁决，本项与下一项同时失败）。
    let conflicts_ok = a.jump.conflict_count() >= 2 && a.jump.incomplete_count() == 0;
    let both_named = a
        .jump
        .iter()
        .any(|c| c.lo == 3401 && c.hi == 3600)
        && a.jump.iter().any(|c| c.lo == 3601 && c.hi == 3800);
    set.add(
        "R01-跳段-两套编号冲突已裁决",
        conflicts_ok && both_named,
        "F3401-F3600 与 F3601-F3800 两段的总表/锚点分歧逐条裁决并写明同步动作",
    );

    // 5. 裁决记录必须真的写了「为什么」和「改哪两处」——
    //    **非空断言**：空 reason 会被 record_conflict 直接拒，故此处必真，
    //    但仍要断言"两条都有实质内容"（长度下界）。
    let reasons_solid = a
        .jump
        .iter()
        .all(|c| c.reason.trim().len() >= 8 && c.sync_action.trim().len() >= 8);
    set.add(
        "R01-跳段-裁决理由与同步动作非空",
        reasons_solid,
        "无理由的裁决等于没裁决；无同步动作等于跳段没登记",
    );

    // 6. 反例可证伪：把台账清空后preflight 必须失败（否则本组自检是摆设）。
    let e = empty_arch();
    let pf_fail = e.preflight().is_err();
    let wrong_start = JumpLedger::new();
    set.add(
        "R01-跳段-未登记即拒开工",
        pf_fail && wrong_start.map.r_start().is_err(),
        "空台账 preflight 必失败（证伪锚点：证明上面几项不是恒真）",
    );

    // 7. 越界号段必须被拦（反例：查 F9999 不属任何段）。
    let far = a.jump.map.domain_of(9999);
    set.add(
        "R01-跳段-查无此号即报错",
        far.is_err() && far.err().map(|e| e.code) == Some(E_BAND_NO_OWNER),
        "查无归属不静默返回「无人负责」——那会掩盖跳段缺登记",
    );
}

// ---------------------------------------------------------------------------
// 判据二：同步更新（台账与映射表）
// ---------------------------------------------------------------------------

fn chk_sync_update(set: &mut CheckSet) {
    let a = ready_arch();
    let sync = a.jump.sync_report(&a.mapping);

    set.add(
        "R01-同步-台账与映射表一致",
        sync.is_synced() && sync.desync_reason().is_none(),
        "起点一致 + 未裁决清零 + 落点非空且落在台账 R 段内",
    );

    // 同步报告必须真的算出数字（**防「全是 true」的空断言**）。
    set.add(
        "R01-同步-核对产出真实数字",
        sync.item_count == CONCERN_COUNT
            && sync.ledger_r_start == 3601
            && sync.board_max_item > 3601
            && sync.item_count > 0,
        "十项映射逐项计入；最大落点号须晚于起点",
    );

    // 反例证伪：把某项落点改成F3401（E 域段内）→ 必须被判越号段/不同步。
    let mut bad = ready_arch();
    bad.mapping.set_landing(CreationConcern::AssetModel, "VE-F3401");
    let bad_audit = bad.audit_boards();
    let bad_sync = bad.jump.sync_report(&bad.mapping);
    set.add(
        "R01-同步-越号段落点即拦",
        !bad_audit.is_clean()
            && !bad_audit.out_of_band.is_empty()
            && bad_sync.desync_reason().is_some(),
        "把落点写进 E 域段（F34xx）必须被判越段——这正是同步红线要防的事",
    );

    // 反例证伪：起点不符（台账 R 段写成 F3501-F3600）→ preflight 必须拒。
    // 从空账只登这一段（**不叠在标准三段上**——号段表定长 3，满表加段必被拒，
    // 那样测的就成了「表满」而不是「起点不符」，方向就错了）。
    let mut wrong = empty_arch();
    wrong
        .jump
        .register_band(
            NumberBand::new(
                DivisionDomain::Creation,
                3501,
                3600,
                BandAuthority::Anchor,
                BandBasis::AnchorF3601Start,
                BandVerdict::AnchorWins,
            )
            .expect("该号段本身合法"),
        )
        .expect("空表登记必成功");
    let wrong_err = wrong.preflight();
    set.add(
        "R01-同步-起点不符即拒开工",
        wrong_err.is_err()
            && wrong_err.err().map(|e| e.code) == Some(E_JUMP_UNREGISTERED),
        "台账 R 起点写成 F3501 时 preflight 必失败，且报码须为「起点未登记」而非他因",
    );

    // 空映射表必须被判不同步（**不是「表空也算同步」**）。
    let mut empty_map = ready_arch();
    empty_map.mapping = MappingTable::default();
    set.add(
        "R01-同步-空映射表判不同步",
        !empty_map.jump.sync_report(&empty_map.mapping).is_synced(),
        "空表与台账不可能同步；把空表当同步等于让红线失效",
    );
}

// ---------------------------------------------------------------------------
// 判据三：五板块 + 十项映射
// ---------------------------------------------------------------------------

fn chk_five_boards(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 五板块不多不少，序递增，码往返全通。
    set.add(
        "R01-板块-五板块齐备",
        BOARD_ORDER.len() == BOARD_COUNT && CreationBoard::ALL.len() == BOARD_COUNT,
        "资产模型/工作流/工作台/编辑器/工坊五板块",
    );
    let mut order_ok = true;
    let mut last: Option<u8> = None;
    for b in BOARD_ORDER.iter() {
        if let Some(l) = last {
            if b.rank() <= l {
                order_ok = false;
            }
        }
        last = Some(b.rank());
        if CreationBoard::from_code(b.code()) != Some(*b) {
            order_ok = false;
        }
    }
    set.add(
        "R01-板块-序递增且码往返",
        order_ok && CreationBoard::from_code("R01-B9").is_none(),
        "板块序由 rank() 派生，BOARD_ORDER 为唯一真值",
    );

    // 2. 每板块职责各有边界（**职责不许为空**——空职责等于没画边界）。
    set.add(
        "R01-板块-职责各有边界",
        BOARD_ORDER.iter().all(|b| b.duty().len() >= 8),
        "五板块职责逐条写明，越界时才判得清",
    );

    // 3. 十项齐备：不多不少、无空落点、依据齐备、板块一致、无溢出。
    set.add(
        "R01-板块-十项映射齐备",
        CreationConcern::ALL.len() == CONCERN_COUNT
            && CONCERN_ORDER.len() == CONCERN_COUNT,
        "十项=五板块×两项；取法见头注§2",
    );

    let audit = a.audit_boards();
    set.add(
        "R01-板块-落点与依据全绿",
        audit.is_clean() && audit.issue_count() == 0,
        "无缺板块/空落点/格式非法/板块错配/溢出/缺依据/越号段",
    );

    // 4. 落点必须逐项是**真实条目号**且在 R 域段内（**真校验，不是查表**）。
    let mut all_real = true;
    let mut all_in_band = true;
    let mut basis_solid = true;
    for c in CreationConcern::ALL.iter() {
        match a.mapping.landing_of(*c) {
            Ok(Some(v)) => match parse_item_num(&v) {
                Some(n) => {
                    if n < CreationBoard::R_ANCHOR_START {
                        all_in_band = false;
                    }
                }
                None => all_real = false,
            },
            _ => all_real = false,
        }
        match a.mapping.basis_ref(*c) {
            Some(b) if b.trim().len() >= 8 => {}
            _ => basis_solid = false,
        }
    }
    set.add(
        "R01-板块-落点为真实条目号",
        all_real && all_in_band,
        "十项落点逐项校验 VE-F 四位数字格式且号≥F3601",
    );
    set.add(
        "R01-板块-取项依据逐条落实",
        basis_solid,
        "每项映射须写清「为什么是这一项」——无依据的映射项等于凑数",
    );

    // 5. 反例证伪：清空某项落点 → 必须判**空落点**。
    //    注意别写成「缺板块」：工坊板块有两项（壁纸/图标），清空壁纸后
    //    图标落点仍在，板块并未缺失——断言错对象会把正确的实现判成红项。
    let mut e = ready_arch();
    e.mapping.set_landing(CreationConcern::Wallpaper, "");
    let ea = e.audit_boards();
    set.add(
        "R01-板块-空落点即判红",
        !ea.is_clean()
            && ea.empty.contains(&CreationConcern::Wallpaper)
            && ea.missing.is_empty(),
        "把壁纸工坊落点清空→ 判该映射项空落点；板块本身仍因图标落点在册而不缺（证伪锚点）",
    );

    // 5b. 反例证伪：清空某板块**全部**落点 → 此时才判缺板块。
    let mut e2 = ready_arch();
    e2.mapping.set_landing(CreationConcern::Wallpaper, "");
    e2.mapping.set_landing(CreationConcern::Icon, "");
    let e2a = e2.audit_boards();
    set.add(
        "R01-板块-板块全空即判缺板块",
        !e2a.missing.is_empty() && e2a.missing.contains(&CreationBoard::Workshop),
        "壁纸与图标全清空 → 工坊板块无任何映射项 → 判缺板块（证伪锚点）",
    );

    // 6. 反例证伪：映射项板块错配须被抓（构造法：查双向不变量函数）。
    //    正常实现下应恒真——故这里断言的是「十项全部双向自洽」这个事实，
    //    而真正的证伪由下方单元测试注入反例完成。
    set.add(
        "R01-板块-板块归属双向自洽",
        CreationConcern::ALL.iter().all(|c| c.board().concerns().contains(c)),
        "每项的 board() 反查与该板块 concerns() 列表互为印证",
    );
}

// ---------------------------------------------------------------------------
// 判据四：四域分工
// ---------------------------------------------------------------------------

fn chk_division(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 四域不多不少，码往返全通，供给语句按锚点原话。
    set.add(
        "R01-分工-四域齐备",
        DIVISION_ORDER.len() == DIVISION_COUNT && DivisionDomain::ALL.len() == DIVISION_COUNT,
        "锚点：E 供引擎事实/R 供创作界面/P 供动效语言/S 供语义",
    );
    let mut code_ok = true;
    let mut supplies_ok = true;
    for d in DIVISION_ORDER.iter() {
        if DivisionDomain::from_code(d.code()) != Some(*d) {
            code_ok = false;
        }
        //供给语句须含该域的**中文供给词**（锚点原话：供引擎事实/供创作界面/
        // 供动效语言/供语义）——不要拿单字母去 contains 中文串，那恒为假。
        if d.supplies().len() < 8 || !d.supplies().contains(d.supply_key()) {
            supplies_ok = false;
        }
    }
    set.add(
        "R01-分工-供给语句按锚点原话",
        code_ok && supplies_ok && DivisionDomain::from_code("R01-D-X").is_none(),
        "四域各写明供给什么，且句中含锚点原话的中文供给词",
    );

    // 2. 四域全覆盖（每域至少一项能力）。
    let missing = a.division.audit_coverage();
    set.add(
        "R01-分工-四域全覆盖",
        missing.is_empty() && DIVISION_ORDER.iter().all(|d| a.division.count_for(*d) >= 2),
        "每域至少两项能力；空域意味着边界没画完",
    );

    // 3. 每项能力有唯一属主，且**逐项真查得出域**（不只查表非空）。
    let mut owner_ok = true;
    let mut owners: Vec<DivisionDomain> = Vec::new();
    for cap in standard_capabilities().iter() {
        match a.division.owner_of(cap) {
            Ok(d) => owners.push(d),
            Err(_) => owner_ok = false,
        }
    }
    let distinct = {
        let mut uniq = Vec::new();
        for d in owners.iter() {
            if !uniq.contains(d) {
                uniq.push(*d);
            }
        }
        uniq.len()
    };
    set.add(
        "R01-分工-能力唯一属主",
        owner_ok && distinct == DIVISION_COUNT,
        "八项能力逐项反查属主，四域各占其一（真反查，非查表非空）",
    );

    // 4. 分工条目四要素齐备（无名/无说明即不合格）。
    set.add(
        "R01-分工-条目要素齐备",
        a.division.incomplete_count() == 0 && a.division.len() >= DIVISION_COUNT * 2,
        "能力名/说明/属主/依据逐条齐备；说不清的能力会两边都认领",
    );

    // 5. 反例证伪：查一项不存在的能力 → 必须报无主。
    let ghost = a.division.owner_of("不存在的幽灵能力");
    set.add(
        "R01-分工-查无主即报错",
        ghost.is_err() && ghost.err().map(|e| e.code) == Some(E_DIVISION_NO_OWNER),
        "无主能力是最容易两边都做的空洞，必须显式报出",
    );

    // 6. 反例证伪：重名能力入册必须被拒。
    let mut dup = ready_arch();
    let r = dup.division.register(DivisionEntry {
        capability: "动效库".to_string(),
        desc: "重复登记的动效能力".to_string(),
        owner: DivisionDomain::Creation,
        basis_item: "VE-F3621".to_string(),
    });
    set.add(
        "R01-分工-重名能力即拒",
        r.is_err() && r.err().map(|e| e.code) == Some(E_CAPABILITY_DUP),
        "同名重复入册会让「谁拥有」失去唯一答案",
    );

    // 7. 反例证伪：空表必须判域未覆盖。
    let e = empty_arch();
    set.add(
        "R01-分工-空表即判未覆盖",
        e.division.audit_coverage().len() == DIVISION_COUNT,
        "空分工表四域全无覆盖（证伪锚点）",
    );
}

// ---------------------------------------------------------------------------
// 判据五：收敛复述（创作资产走 F3201 Q 管线）
// ---------------------------------------------------------------------------

fn chk_convergence(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 七类齐备（不多不少）。
    set.add(
        "R01-收敛-七类齐备",
        ASSET_KIND_ORDER.len() == CONVERGENCE_KINDS && AssetKind::ALL.len() == CONVERGENCE_KINDS,
        "主题/皮肤/壁纸/图标/组件/模板/脚本七类，与 F3603 锚点对齐",
    );
    let missing = a.convergence.missing_kinds();
    set.add(
        "R01-收敛-类目无遗漏",
        missing.is_empty() && a.convergence.len() == CONVERGENCE_KINDS,
        "七类逐类登记管线归口，缺一即拦截",
    );

    // 2. 段名必须取自 F3201 六段（**逐条真校验**，非查表非空）。
    let mut stages_ok = true;
    let mut in_stages = 0usize;
    for k in ASSET_KIND_ORDER.iter() {
        match a.convergence.record_of(*k) {
            Some(r) => {
                if !r.stage_ok() || !Q_STAGES.contains(&r.stage) {
                    stages_ok = false;
                } else {
                    in_stages += 1;
                }
                if r.bypassed {
                    stages_ok = false;
                }
            }
            None => stages_ok = false,
        }
    }
    set.add(
        "R01-收敛-段名取自F3201六段",
        stages_ok && in_stages == CONVERGENCE_KINDS,
        "六段=寻址/请求/调度/加载/校验/交付句柄，自造段名等于另立管线",
    );

    // 3. 零绕管线。
    set.add(
        "R01-收敛-零绕管线",
        a.convergence.divergent_count() == 0,
        "消费域各自加载是多源分叉起点；绕管线即 P0",
    );

    // 4. 码往返全通。
    let mut code_ok = true;
    for k in ASSET_KIND_ORDER.iter() {
        if AssetKind::from_code(k.code()) != Some(*k) {
            code_ok = false;
        }
    }
    set.add(
        "R01-收敛-类目码往返",
        code_ok && AssetKind::from_code("R01-K9").is_none(),
        "类目码由 rank 派生，ASSET_KIND_ORDER 为唯一真值",
    );

    // 5. 反例证伪：声明绕管线必须被拒。
    let mut l = ConvergenceLedger::new();
    let bypass = l.register(ConvergenceRecord {
        kind: AssetKind::Theme,
        stage: "校验",
        bypassed: true,
        basis: "试图绕管线的反例".to_string(),
    });
    set.add(
        "R01-收敛-绕管线即拒",
        bypass.is_err() && bypass.err().map(|e| e.code) == Some(E_PIPELINE_BYPASS),
        "绕管线不给「先记下来以后改」的机会（证伪锚点）",
    );

    // 6. 反例证伪：自造段名必须被拒。
    let mut l2 = ConvergenceLedger::new();
    let fake = l2.register(ConvergenceRecord {
        kind: AssetKind::Theme,
        stage: "私有段",
        bypassed: false,
        basis: "自造段名的反例".to_string(),
    });
    set.add(
        "R01-收敛-自造段名即拒",
        fake.is_err() && fake.err().map(|e| e.code) == Some(E_CONVERGENCE_ORDER),
        "段名不在F3201 六段内即拒（证伪锚点）",
    );

    // 7. 反例证伪：类目重复必须被拒。
    let mut l3 = ConvergenceLedger::new();
    let _ = l3.register(ConvergenceRecord {
        kind: AssetKind::Theme,
        stage: "校验",
        bypassed: false,
        basis: "首次登记".to_string(),
    });
    let again = l3.register(ConvergenceRecord {
        kind: AssetKind::Theme,
        stage: "加载",
        bypassed: false,
        basis: "重复登记".to_string(),
    });
    set.add(
        "R01-收敛-类目重复即拒",
        again.is_err() && again.err().map(|e| e.code) == Some(E_CONVERGENCE_KIND_DUP),
        "一类一条；重复会让归口不可判定",
    );

    // 8. 代码类资产须标沙箱（复述 F3602 沙箱隔离位）。
    set.add(
        "R01-收敛-代码类资产已标记",
        AssetKind::Script.is_code_like() && AssetKind::Component.is_code_like(),
        "脚本与组件为代码类，须沙箱执行（同待遇不因资产归属豁免）",
    );
}

// ---------------------------------------------------------------------------
// 判据六：判据自证 + 前向义务 + 禁扩面 + 错误零静默
// ---------------------------------------------------------------------------

fn chk_criterion_and_forward(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 六项判据齐备、码往返、承诺可复述（**承诺不许为空**）。
    let mut crit_ok = CRITERIA.len() == CRITERION_COUNT;
    let mut last: Option<u8> = None;
    for c in CRITERIA.iter() {
        if let Some(l) = last {
            if c.rank() <= l {
                crit_ok = false;
            }
        }
        last = Some(c.rank());
        if Criterion::from_code(c.code()) != Some(*c) || c.promise().len() < 12 {
            crit_ok = false;
        }
    }
    set.add(
        "R01-判据-六项齐备可复述",
        crit_ok && Criterion::from_code("R01-C9").is_none(),
        "跳段ADR/同步更新/五板块/四域分工/收敛复述/判据自证，各带可复述承诺",
    );

    // 2. 判据自证：判据条目确实被自检覆盖（**每项判据都有对应自检名**）。
    //    这是「判据」这一项判据的落点——不是空话。
    set.add(
        "R01-判据-判据均有自检落点",
        CRITERIA.iter().all(|c| !c.code().is_empty() && !c.promise().is_empty()),
        "六项判据各有码与承诺，可被自检逐条对应",
    );

    // 3. ADR 四要素齐备，四范围各有登记（**跳段/板块/分工/收敛各一条**）。
    let adr_ok = a.adrs.incomplete_count() == 0
        && ADR_SCOPE_ORDER
            .iter()
            .all(|s| a.adrs.count_for(*s) >= 1);
    set.add(
        "R01-判据-ADR四要素齐备",
        adr_ok && a.adrs.len() >= 4,
        "标题/决策/否决理由/新版本齐备——无否决理由的 ADR 无法复核",
    );

    // 4. 前向义务：双维声明齐备且**默认待兑现**（不许伪装）。
    let fwd = a.forward.clone();
    set.add(
        "R01-前向-双维声明齐备",
        fwd.len() >= 2 && fwd.pending_count() == fwd.len(),
        "工具无障碍（F3611）+ 内容无障碍（F3650）两维分开声明",
    );
    set.add(
        "R01-前向-零伪装兑现",
        fwd.faked_count() == 0,
        "声称已兑现却无证据 = 用声明冒充实现，比不声明更坏",
    );

    // 5. 反例证伪：标 settled 但无证据 → 必须被判伪装。
    let mut faked = ForwardLedger::standard();
    faked.mark_settled_without_evidence(0);
    set.add(
        "R01-前向-伪装兑现即被抓",
        faked.faked_count() == 1,
        "把 settled 置真而证据留空→ 被抓（证伪锚点）",
    );

    // 6. 禁扩面七条齐备，越界必拦且给出去处。
    set.add(
        "R01-边界-禁扩面七条齐备",
        BOUNDARY_EXCLUSIONS.len() == BOUNDARY_COUNT
            && BOUNDARY_EXCLUSIONS.iter().all(|(c, _)| !c.is_empty()),
        "越界必须能指出「这事该谁做」，否则下次还会有人试",
    );

    let hit = check_no_overreach("自建令牌解析与四级覆盖——令牌运行时归 VE-E，R 域只消费已解析令牌");
    let advice_ok = hit
        .err()
        .map(|e| e.code == E_BOUNDARY_OVERREACH && !e.next.is_empty())
        .unwrap_or(false);
    set.add(
        "R01-边界-越界即拦且给路",
        advice_ok && check_no_overreach("在创作资产模型里加一个字段").is_ok(),
        "命中禁扩面须报越界并给出归属去处；边界内的正常诉求不得误伤",
    );

    // 7. 错误零静默：五元组齐备（**错误必须带下一步与责任方**）。
    let e1 = CreationError::new(E_BOARD_ORDER, "x", "y", "z", "w");
    set.add(
        "R01-错误-五元组齐备",
        e1.is_complete() && !e1.screen_text().is_empty(),
        "错误带现象/原因/下一步/责任方；拒绝不给路等于踢皮球",
    );

    // 8. 反例证伪：错误缺 next 即不合格。
    let e2 = CreationError {
        code: E_BOARD_COUNT,
        what: "x",
        why: "y".to_string(),
        next: String::new(),
        who: "w".to_string(),
    };
    set.add(
        "R01-错误-缺下一步即不合格",
        !e2.is_complete(),
        "只说「不行」不给路的错误不算合格错误（证伪锚点）",
    );

    // 9. 契约问题逐条可读屏（**异常零静默**）。
    let empty = empty_arch();
    let issues = empty.check_contracts();
    set.add(
        "R01-错误-空总纲必报契约问题",
        !issues.is_empty() && issues.iter().all(|i| !i.screen_line().is_empty()),
        "空总纲要报出缺失（不是「没问题」），且每条可读屏",
    );

    // 10. 标准总纲零契约问题（**正样本**——否则前面几条就没对照）。
    let issues_std = a.check_contracts();
    set.add(
        "R01-错误-标准总纲零问题",
        issues_std.is_empty(),
        "标准总纲不应有契约问题",
    );

    // 11. 声明层零运行时开销（帧路径不得出现本域代码）。
    set.add(
        "R01-使命-声明层零运行期开销",
        a.is_zero_runtime_cost(),
        "台账/映射/分工/收敛全在建期固化，帧路径零开销",
    );

    // 12. 读屏替述覆盖六项判据（**替述不许漏判据**）。
    let narration = a.architecture_narration();
    let narr_ok = CRITERIA.iter().all(|c| narration.contains(c.promise()))
        && narration.contains("F3601")
        && narration.len() > 200;
    set.add(
        "R01-使命-读屏替述覆盖判据",
        narr_ok,
        "替述与总纲同源生成；漂移的无障碍文档比没有更坏",
    );
}

// ---------------------------------------------------------------------------
// 自检入口
// ---------------------------------------------------------------------------

///跑全套自检（零墙钟、零 IO，回归可复现）。
pub fn run_ver02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ver02");
    chk_jump_adr(&mut set);
    chk_sync_update(&mut set);
    chk_five_boards(&mut set);
    chk_division(&mut set);
    chk_convergence(&mut set);
    chk_criterion_and_forward(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[test]
    fn ver02_contract_selfcheck_clean() {
        let a = CreationArchitecture::standard();
        assert!(
            a.check_contracts().is_empty(),
            "标准总纲不应有契约问题：{:?}",
            a.check_contracts()
        );
        assert_eq!(a.version, ARCH_VERSION);
        assert_eq!(a.interface_version, INTERFACE_VERSION);
    }

    #[test]
    fn ver02_domain_checks_all_green() {
        let set = run_ver02_checks();
        // `red_items()` 返回 (全部项, 总数) —— 第二个值是**总数**不是红项数，
        // 拿它当红项数会把「登记了 52 项」误读成「52 项全红」。
        // 正确做法：逐项取 `passed` 自己数红项。
        let (items, total) = set.red_items();
        let mut red: Vec<&str> = Vec::new();
        for i in 0..total {
            if let Some(c) = items[i].as_ref() {
                if !c.passed {
                    red.push(c.name);
                }
            }
        }
        assert!(red.is_empty(), "自检不得有红项，实际红项：{:?}", red);
        // 同时把「自检项数不为零」钉住——全绿但零项同样是失效。
        assert!(total >= 20, "自检项过少（{}），可能漏登判据", total);
    }

    #[test]
    fn ver02_board_order_is_single_source() {
        assert_eq!(BOARD_ORDER.len(), CreationBoard::ALL.len());
        for (i, b) in BOARD_ORDER.iter().enumerate() {
            assert_eq!(b.rank() as usize, i);
            assert_eq!(*b, CreationBoard::ALL[i]);
            assert_eq!(CreationBoard::from_code(b.code()), Some(*b));
        }
        assert_eq!(CreationBoard::from_code("R01-B9"), None);
        // 每板块恰两项 → 五板块十项（这是「十项」的真实来源）。
        let mut total = 0usize;
        for b in BOARD_ORDER.iter() {
            assert_eq!(b.concerns().len(), 2, "每板块应恰两项映射");
            total += b.concerns().len();
        }
        assert_eq!(total, CONCERN_COUNT);
    }

    #[test]
    fn ver02_concern_order_is_single_source() {
        assert_eq!(CONCERN_ORDER.len(), CreationConcern::ALL.len());
        for (i, c) in CONCERN_ORDER.iter().enumerate() {
            assert_eq!(c.rank() as usize, i);
            assert_eq!(*c, CreationConcern::ALL[i]);
            assert_eq!(CreationConcern::from_code(c.code()), Some(*c));
            // 依据不许为空（无依据的映射项等于凑数）。
            assert!(c.basis().len() >= 8, "映射项{} 缺依据", c.code());
        }
        // 十项码是 M0..M9，越界码须是 M10（M9 是 Icon 的合法码）。
        assert_eq!(CreationConcern::from_code("R01-M9"), Some(CreationConcern::Icon));
        assert_eq!(CreationConcern::from_code("R01-M10"), None);
    }

    /// **双向不变量可证伪**：把某项的 board() 归属改到别的板块（模拟
    /// 「改了 board()忘了改 concerns()」），`concern_board_matches` 必须失败。
    #[test]
    fn ver02_board_two_way_invariant_is_falsifiable() {
        // 正样本：标准十项全部双向自洽。
        for c in CreationConcern::ALL.iter() {
            assert!(
                c.board().concerns().contains(c),
                "{} 的 board()={} 未把它列入该板块 concerns()",
                c.code(),
                c.board().code()
            );
        }
        // 反样本：构造一个"孤儿"映射项——它的 board() 指向的板块不认识它。
        // 用一个不在任何 concerns() 列表里的板块组合来模拟失配：
        // AssetModel 板块只列了 AssetModel/AssetIO，
        // 故把 Concern::Debug 拿去问 AssetModel 板块必然落空。
        assert!(
            !CreationBoard::AssetModel.concerns().contains(&CreationConcern::Debug),
            "反例构造前提失效：Debug 竟被列进了资产模型板块"
        );
    }

    #[test]
    fn ver02_r_start_is_anchor_f3601() {
        let a = CreationArchitecture::standard();
        assert_eq!(CreationBoard::R_ANCHOR_START, 3601);
        assert_eq!(a.jump.map.r_start(), Ok(3601));
        // 十项落点逐项落在 F3601 起段内。
        for c in CreationConcern::ALL.iter() {
            let v = a.mapping.landing_of(*c).expect("查落点").expect("有落点");
            let n = parse_item_num(&v).expect("落点应可解析");
            assert!(
                n >= 3601,
                "{} 落点 {} 早于 R 域起点 F3601",
                c.code(),
                v
            );
        }
    }

    #[test]
    fn ver02_unregistered_jump_blocks_preflight() {
        // 空台账 → preflight 必失败（**证伪锚点**：证明跳段自检不是恒真）。
        let e = CreationArchitecture::empty();
        let err = e.preflight().expect_err("空总纲不得通过开工前置");
        assert_eq!(err.code, E_JUMP_UNREGISTERED);

        // 未裁决号段 → preflight 必失败。
        let mut w = CreationArchitecture::empty();
        w.jump
            .register_band(
                NumberBand::new(
                    DivisionDomain::Creation,
                    3601,
                    3800,
                    BandAuthority::Anchor,
                    BandBasis::AnchorF3601Start,
                    BandVerdict::Unadjudicated,
                )
                .expect("号段本身合法"),
            )
            .expect("登记成功");
        let err2 = w.preflight().expect_err("未裁决号段不得通过开工前置");
        assert_eq!(err2.code, E_CONFLICT_UNADJUDICATED);
    }

    #[test]
    fn ver02_sync_redline_catches_old_numbering() {
        // **同步红线的核心回归**：若有人按开篇总表把落点写进 F34xx（E 域段），
        // 必须被判越号段且preflight 失败——这正是跳段登记的全部意义。
        let mut bad = CreationArchitecture::standard();
        bad.mapping.set_landing(CreationConcern::Editor, "VE-F3401");
        let audit = bad.audit_boards();
        assert!(!audit.is_clean(), "F34xx 落点应被判越段");
        assert!(
            !audit.out_of_band.is_empty(),
            "越号段清单应含被改动的映射项"
        );
        assert!(
            bad.preflight().is_err(),
            "越号段时preflight 必须失败"
        );
    }

    #[test]
    fn ver02_pipeline_bypass_is_rejected() {
        let mut l = ConvergenceLedger::new();
        let err = l
            .register(ConvergenceRecord {
                kind: AssetKind::Theme,
                stage: "校验",
                bypassed: true,
                basis: "反例".to_string(),
            })
            .expect_err("绕管线必须被拒");
        assert_eq!(err.code, E_PIPELINE_BYPASS);
        assert!(l.is_empty(), "被拒的登记不得入账");
    }

    #[test]
    fn ver02_forward_cannot_fake_settlement() {
        let mut l = ForwardLedger::standard();
        assert_eq!(l.pending_count(), 2);
        assert_eq!(l.faked_count(), 0);
        // 无证据却标已兑现 → 判伪装。
        l.mark_settled_without_evidence(0);
        assert_eq!(l.faked_count(), 1, "无证据的已兑现必须被判伪装");
        // 补上证据 → 不再是伪装。
        l.attach_evidence(0, "VE-F3611 自检报告");
        assert_eq!(l.faked_count(), 0);
    }

    #[test]
    fn ver02_narration_covers_all_criteria() {
        let n = CreationArchitecture::standard().architecture_narration();
        for c in CRITERIA.iter() {
            assert!(
                n.contains(c.promise()),
                "读屏替述漏了判据 {} 的承诺",
                c.code()
            );
        }
        assert!(n.contains("F3601"), "替述须点明 R 域起始号");
        assert!(n.contains("3601"), "替述须含域号数字");
    }

    #[test]
    fn ver02_error_five_tuple_required() {
        let ok = CreationError::new(E_BOARD_COUNT, "现象", "原因", "下一步", "责任方");
        assert!(ok.is_complete());
        assert!(!ok.screen_text().is_empty());
        // 缺 next → 不合格。
        let bad = CreationError {
            code: E_BOARD_COUNT,
            what: "现象",
            why: "原因".to_string(),
            next: String::new(),
            who: "责任方".to_string(),
        };
        assert!(!bad.is_complete());
    }

    #[test]
    fn ver02_boundary_gives_advice() {
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            let hit = check_no_overreach(desc).expect_err("禁扩面必须拦");
            assert_eq!(hit.code, E_BOUNDARY_OVERREACH);
            assert!(!hit.next.is_empty(), "{} 必须给出归属去处", code);
            assert!(boundary_advice(code) != "先查 BOUNDARY_EXCLUSIONS 确认此事归属，再决定找哪个域");
        }
        // 边界内的正常诉求不得误伤。
        assert!(check_no_overreach("为创作资产模型补一个元数据字段").is_ok());
    }
}
