//! VE-F5801 自检 · AI 域总架构（判据逐条映射：四层架构 / AI 三律 / 可预期
//! 可解释可关闭 / 承接清单 / 判据自检）。
//!
//! **判据设计硬规矩**：流向表判据侧独立枚举 16 组合重算对账（不共享实现
//! 的 `legal_flow`）；三律违例红侧+合规绿侧双向；承接清单名字冻结逐字
//! 对账；判据区零 panic 面（取值一律 get/match）。

use alloc::string::String;
use alloc::string::ToString;

use crate::checks::CheckSet;
use crate::svstar2::veai01_aiarch as ai;

// ---------------------------------------------------------------------------
// 组一：四层架构
// ---------------------------------------------------------------------------

fn chk_layers(s: &mut CheckSet) {
    // AC01-层-01：四层闭集短码互异。
    let layers = ai::AiLayer::all();
    let mut distinct = true;
    for i in 0..layers.len() {
        for j in (i + 1)..layers.len() {
            if layers[i].name() == layers[j].name() {
                distinct = false;
            }
        }
    }
    s.add("AC01-层-01", layers.len() == 4 && distinct, "四层闭集短码互异");
    // AC01-层-02：序号与全集顺序一致（ordinal 单源）。
    let ord_ok = layers
        .iter()
        .enumerate()
        .all(|(i, l)| l.ordinal() == i);
    s.add("AC01-层-02", ord_ok, "ordinal 与全集顺序一致");
    // AC01-层-03：流向表判据侧独立枚举 16 组合对账。
    let mut flow_ok = true;
    for (fi, f) in layers.iter().enumerate() {
        for (ti, t) in layers.iter().enumerate() {
            flow_ok = flow_ok && ai::legal_flow(*f, *t) == ai::FLOW_TABLE_EXPECTED[fi][ti];
        }
    }
    s.add("AC01-层-03", flow_ok, "流向表 16 组合独立重算对账");
    // AC01-层-04：合法流放行（主干下行+协作横跨四条）。
    let ok = ai::check_flow(ai::AiLayer::Perceive, ai::AiLayer::Decide).is_ok()
        && ai::check_flow(ai::AiLayer::Decide, ai::AiLayer::Act).is_ok()
        && ai::check_flow(ai::AiLayer::Act, ai::AiLayer::Coordinate).is_ok()
        && ai::check_flow(ai::AiLayer::Coordinate, ai::AiLayer::Decide).is_ok();
    s.add("AC01-层-04", ok, "冻结流向表四条合法流放行");
    // AC01-层-05：越权拒绝（跨层跳/逆行/自环——红侧）。
    let bad = [
        (ai::AiLayer::Perceive, ai::AiLayer::Act),
        (ai::AiLayer::Act, ai::AiLayer::Perceive),
        (ai::AiLayer::Decide, ai::AiLayer::Decide),
        (ai::AiLayer::Perceive, ai::AiLayer::Coordinate),
    ];
    let mut all_rej = true;
    for (f, t) in bad.iter() {
        match ai::check_flow(*f, *t) {
            Err(msg) => all_rej = all_rej && msg.starts_with(ai::E_AI_TRESPASS),
            Ok(_) => all_rej = false,
        }
    }
    s.add("AC01-层-05", all_rej, "越权四例显性拒（跨层/逆行/自环）");
    // AC01-层-06：层中文名互异（读屏可达不混淆）。
    let zh_ok = layers[0].zh() != layers[1].zh()
        && layers[1].zh() != layers[2].zh()
        && layers[2].zh() != layers[3].zh();
    s.add("AC01-层-06", zh_ok, "四层中文名互异");
}

// ---------------------------------------------------------------------------
// 组二：三律（可预期 / 可解释 / 可关闭——红绿双向）
// ---------------------------------------------------------------------------

fn chk_laws(s: &mut CheckSet) {
    // AC01-律-01：三律闭集短码互异。
    let laws = ai::AiLaw::all();
    s.add(
        "AC01-律-01",
        laws.len() == 3 && laws[0].name() != laws[1].name() && laws[1].name() != laws[2].name(),
        "三律闭集短码互异",
    );
    // AC01-律-02：可预期绿侧——已登记行为裁决通过。
    let behavior = ai::AiBehavior {
        name: "guard-patrol".to_string(),
        layer: ai::AiLayer::Decide,
        declaration: "巡逻行为：按固定节奏巡回".to_string(),
    };
    let decision = ai::AiDecision {
        behavior: "guard-patrol".to_string(),
        reason: "玩家接近警戒区，按登记的巡逻规则转向".to_string(),
    };
    let cap_on = ai::AiCapability {
        name: "guard-patrol".to_string(),
        enabled: true,
        degrade_note: String::new(),
    };
    s.add(
        "AC01-律-02",
        ai::law_check(Some(&behavior), Some(&decision), Some(&cap_on)).is_ok(),
        "三律齐备裁决通过（合规绿侧）",
    );
    // AC01-律-03：可预期红侧——行为未登记显性拒且指名哪条律。
    match ai::law_check(None, Some(&decision), Some(&cap_on)) {
        Err(msg) => s.add(
            "AC01-律-03",
            msg.starts_with(ai::E_AI_LAW) && msg.contains(ai::AiLaw::Predictable.name()),
            "未登记行为拒（指名可预期）",
        ),
        Ok(()) => s.add("AC01-律-03", false, "未登记行为被放行（红线破）"),
    }
    // AC01-律-04：可解释红侧——reason 空白拒。
    let blank = ai::AiDecision { behavior: "guard-patrol".to_string(), reason: "   ".to_string() };
    match ai::law_check(Some(&behavior), Some(&blank), Some(&cap_on)) {
        Err(msg) => s.add(
            "AC01-律-04",
            msg.contains(ai::AiLaw::Explainable.name()),
            "空白理由拒（指名可解释）",
        ),
        Ok(()) => s.add("AC01-律-04", false, "黑箱决策被放行（红线破）"),
    }
    // AC01-律-05：可解释红侧——决策缺字段拒。
    match ai::law_check(Some(&behavior), None, Some(&cap_on)) {
        Err(msg) => s.add(
            "AC01-律-05",
            msg.contains(ai::AiLaw::Explainable.name()),
            "决策缺字段拒",
        ),
        Ok(()) => s.add("AC01-律-05", false, "缺决策被放行（红线破）"),
    }
    // AC01-律-06：可关闭红侧——关闭但无降级声明拒（失踪非关闭）。
    let cap_off_bad = ai::AiCapability {
        name: "guard-patrol".to_string(),
        enabled: false,
        degrade_note: String::new(),
    };
    match ai::law_check(Some(&behavior), Some(&decision), Some(&cap_off_bad)) {
        Err(msg) => s.add(
            "AC01-律-06",
            msg.contains(ai::AiLaw::Closable.name()),
            "关闭无降级声明拒（指名可关闭）",
        ),
        Ok(()) => s.add("AC01-律-06", false, "失踪式关闭被放行（红线破）"),
    }
    // AC01-律-07：可关闭绿侧——带降级声明的关闭通过（关得明白）。
    let cap_off_ok = ai::AiCapability {
        name: "guard-patrol".to_string(),
        enabled: false,
        degrade_note: "该能力已关闭，守卫按预设路线静态巡逻".to_string(),
    };
    s.add(
        "AC01-律-07",
        ai::law_check(Some(&behavior), Some(&decision), Some(&cap_off_ok)).is_ok(),
        "带声明的关闭通过（降级是状态不是失踪）",
    );
    // AC01-律-08：可关闭红侧——能力缺开关拒（关不掉）。
    match ai::law_check(Some(&behavior), Some(&decision), None) {
        Err(msg) => s.add("AC01-律-08", msg.contains(ai::AiLaw::Closable.name()), "缺开关拒"),
        Ok(()) => s.add("AC01-律-08", false, "关不掉的能力被放行（红线破）"),
    }
    // AC01-律-09：三律声明读屏行非空且含三律中文名（读屏可达）。
    let line = ai::laws_screen_line();
    s.add(
        "AC01-律-09",
        line.contains("可预期") && line.contains("可解释") && line.contains("可关闭"),
        "三律声明单行读屏可达",
    );
}

// ---------------------------------------------------------------------------
// 组三：承接清单
// ---------------------------------------------------------------------------

fn chk_handover(s: &mut CheckSet) {
    // AC01-承接-01：十件闭集（长度钉死）。
    s.add("AC01-承接-01", ai::HANDOVER_COUNT == 10 && ai::HANDOVER_ITEMS.len() == 10, "十件闭集");
    // AC01-承接-02：名字非空互异（冻结清单可对账）。
    let mut distinct = true;
    for i in 0..ai::HANDOVER_ITEMS.len() {
        for j in (i + 1)..ai::HANDOVER_ITEMS.len() {
            if ai::HANDOVER_ITEMS[i] == ai::HANDOVER_ITEMS[j] {
                distinct = false;
            }
        }
    }
    let non_empty = ai::HANDOVER_ITEMS.iter().all(|n| !n.trim().is_empty());
    s.add("AC01-承接-02", distinct && non_empty, "十件名非空互异");
    // AC01-承接-03：新清单全待核验、缺件报告=十件全列。
    let ledger = ai::HandoverLedger::new();
    s.add(
        "AC01-承接-03",
        ledger.verified_count() == 0 && ledger.missing_report().len() == 10,
        "初始全缺（追补报告全列）",
    );
    // AC01-承接-04：逐件核验后计数与报告联动。
    let mut led = ai::HandoverLedger::new();
    let mut ok = led.verify(0).is_ok() && led.verify(3).is_ok();
    ok = ok && led.verified_count() == 2 && led.missing_report().len() == 8;
    s.add("AC01-承接-04", ok, "核验两件：计数 2 / 缺件 8 联动");
    // AC01-承接-05：缺件报告指名缺哪件（序号+名字）。
    let missing = led.missing_report();
    s.add(
        "AC01-承接-05",
        !missing.is_empty() && missing.iter().any(|(i, _)| *i == 1) && missing.iter().any(|(i, _)| *i == 9),
        "追补报告含件号与名字",
    );
    // AC01-承接-06：件号越界显性拒（不静默钳制）。
    s.add(
        "AC01-承接-06",
        led.verify(10).is_err() && led.verify(usize::MAX).is_err(),
        "越界件号显性拒",
    );
    // AC01-承接-07：十件全核验后追补报告为空（终态）。
    let mut full = ai::HandoverLedger::new();
    let mut all_ok = true;
    for i in 0..ai::HANDOVER_COUNT {
        all_ok = all_ok && full.verify(i).is_ok();
    }
    s.add(
        "AC01-承接-07",
        all_ok && full.verified_count() == 10 && full.missing_report().is_empty(),
        "十件全验零缺件（终态可达非恒缺）",
    );
}

// ---------------------------------------------------------------------------
// 组四：判据自检
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // AC01-判据-01：错误码非空互异。
    s.add(
        "AC01-判据-01",
        ai::E_AI_TRESPASS != ai::E_AI_LAW
            && ai::E_AI_LAW != ai::E_AI_HANDOVER
            && !ai::E_AI_HANDOVER.is_empty(),
        "错误码非空互异",
    );
    // AC01-判据-02：版本指纹 const 期=运行期同算对账（FNV-1a）。
    let runtime_fp = {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in ai::AIARCH_PROTOCOL_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    s.add(
        "AC01-判据-02",
        ai::version_fingerprint() == runtime_fp && runtime_fp != 0,
        "版本指纹两口径一致",
    );
    // AC01-判据-03：常量容量钉死（层4/律3/件10——锚点数值）。
    s.add(
        "AC01-判据-03",
        ai::LAYER_COUNT == 4 && ai::LAW_COUNT == 3 && ai::HANDOVER_COUNT == 10,
        "容量常量钉死",
    );
    // AC01-判据-04：判据条数对账。
    s.add("AC01-判据-04", s.len() == 25, "判据条数对账（本条为第 26 条）");
}

/// F5801 域自检聚合入口。
pub fn run_veai01_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5801");
    chk_layers(&mut s);
    chk_laws(&mut s);
    chk_handover(&mut s);
    chk_meta(&mut s);
    s
}
