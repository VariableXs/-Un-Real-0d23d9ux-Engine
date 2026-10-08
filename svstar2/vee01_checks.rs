//! VE-F0801 · 域自检（判据逐条对应，见 `vee01_arch.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **补写显性**（S117 审计发现签收，未签收禁开工） → `E01-补写-S117签收显性`
//! - **四段架构**（四段齐备、接口冻结、单向流、跳段/逆流拦截） → `E01-架构-四段与单向流`
//! - **段间接口冻结**（上游未冻结禁开工下游、冻结幂等） → `E01-架构-接口冻结闸`
//! - **三向兑现**（字形/排版/渲染三轴判据逐条挂证据） → `E01-兑现-三轴判据挂证据`
//! - **复用声明**（X01 开工范式 + 六件套同构） → `E01-复用-X01声明与六件套`
//! - **1.5ms 预算**（六段分解自洽、三水位、降级序、无障碍红线） → `E01-预算-1.5ms与三水位`
//! - **ADR**（架构变更走 ADR，提案态不可用） → `E01-变更-ADR门禁`
//! - **供给边分型**（供给边不污染数据流核验、防旁路） → `E01-架构-供给边分型`
//! - **域就绪闸**（六闸齐绿才就绪，缺项可读） → `E01-就绪-六闸齐绿`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vee01_arch::*;
use crate::checks::CheckSet;

/// 造一条完整开工骨架（签收 + 冻结 + 四段开工 + 单向流）。
fn ready_skeleton() -> TextPipeline {
    let mut p = TextPipeline::new();
    p.signoff_s117();
    for slot in 0..STAGE_COUNT - 1 {
        let _ = p.freeze_interface(slot);
    }
    for stage in 0..STAGE_COUNT {
        let _ = p.open_stage(stage);
    }
    for slot in 0..STAGE_COUNT - 1 {
        let _ = p.declare_flow(slot, slot + 1);
    }
    p
}

/// 三向证据一次性挂齐（返回是否全兑现）。
fn fulfil_three_way(p: &mut TextPipeline) -> bool {
    for c in p.commitments().to_vec() {
        let axis = c.axis;
        for k in c.criteria {
            p.attach_evidence(axis, &k.text, "证据在册（金样/对拍/双通道计时）");
        }
    }
    p.three_way_fulfilled()
}

/// 把六件套填齐并出具复用声明。
fn pack_ready(p: &mut TextPipeline) {
    for s in OPEN_PACK_SLOTS {
        p.fill_open_slot(s, "已填（X01 体例同构）");
    }
    p.issue_reuse_declaration();
}

/// VE-F0801 域自检。
pub fn run_vee01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee01");

    // ---- 补写显性 ----

    // 判据：S117 审计发现被显性签收；未签收禁止开工；签收进审计账。
    {
        let mut p = TextPipeline::new();
        assert!(!p.supplement_signed());
        // 未签收即开工被拒（补写显性的硬闸）。
        assert!(p.open_stage(0).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SUPPLEMENT_NOT_SIGNED"));
        // 越界段与越界槽位也显性报错。
        assert!(p.freeze_interface(9).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SLOT_OUT_OF_RANGE"));
        assert!(p.open_stage(9).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_STAGE_OUT_OF_RANGE"));
        // 签收后入审计账，manifest 点名区段与补写批次。
        p.signoff_s117();
        assert!(p.supplement_signed());
        assert!(p.audits().iter().any(|a| a.contains("签收 S117 审计发现")));
        assert!(SUPPLEMENT_MANIFEST.contains("F0801-F1000"));
        assert!(SUPPLEMENT_MANIFEST.contains("Eb01-Eb10"));
        set.add("E01-补写-S117签收显性", true, "");
    }

    // ---- 四段架构 + 单向流 ----

    // 判据：四段齐备且段名正确；单向链完整；跳段与逆流一律拒绝并记账。
    {
        let mut p = ready_skeleton();
        assert_eq!(p.open_stage_count(), 4);
        assert_eq!(STAGE_NAMES[SEG_GLYPH], "编码字形");
        assert_eq!(STAGE_NAMES[SEG_SHAPING], "排版Shaping");
        assert_eq!(STAGE_NAMES[SEG_FONT], "字体管理");
        assert_eq!(STAGE_NAMES[SEG_RENDER], "渲染输出");
        let chain_ok = p.verify_flow() && p.flows().len() == STAGE_COUNT - 1;
        // 跳段（0→2）与逆流（3→1）双双被拒，且不污染链。
        assert!(p.declare_flow(0, 2).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_FLOW_BYPASS"));
        assert!(p.declare_flow(3, 1).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_FLOW_BACKWARD"));
        // 越界边被拒。
        assert!(p.declare_flow(0, 9).is_err());
        let still_ok = p.verify_flow();
        let doc = UNIDIRECTIONAL_FLOW_DOC.contains("紧邻上游")
            && UNIDIRECTIONAL_FLOW_DOC.contains("单向数据流契约");
        set.add(
            "E01-架构-四段与单向流",
            chain_ok && still_ok && doc,
            "",
        );
    }

    // ---- 接口冻结闸 ----

    // 判据：上游接口未冻结禁止开工下游；冻结幂等；冻结态可观测。
    {
        let mut p = TextPipeline::new();
        p.signoff_s117();
        assert!(p.open_stage(0).is_ok(), "段 0 无上游，可直接开工");
        assert!(p.open_stage(2).is_err(), "段 2 上游未冻结，禁开工");
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_UPSTREAM_NOT_FROZEN"));
        // 冻结槽 0 → 段 1 解锁；但段 2 仍缺槽 1。
        let fp0 = p.freeze_interface(0).unwrap();
        assert!(p.open_stage(1).is_ok());
        assert!(p.open_stage(2).is_err());
        let _ = p.freeze_interface(1);
        assert!(p.open_stage(2).is_ok(), "全部上游冻结后才解锁");
        // 冻结幂等（同token 重放）；4 段之间共 3 条段间接口，槽 3 不存在。
        assert_eq!(p.freeze_interface(0).unwrap(), fp0);
        assert_eq!(p.frozen_flags(), alloc::vec![true, true, false], "槽 0/1 已冻结，槽 2 未冻");
        assert!(p.freeze_interface(3).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SLOT_OUT_OF_RANGE"));
        // 指纹对字段变化敏感。
        let a = InterfaceSpec::new("X", 1, &["a", "b"]);
        let b = InterfaceSpec::new("X", 1, &["a", "c"]);
        let c2 = InterfaceSpec::new("X", 2, &["a", "b"]);
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), c2.fingerprint());
        set.add("E01-架构-接口冻结闸", true, "");
    }

    // ---- 供给边分型 ----

    // 判据：供给边与数据流分型；自环拒；与数据流边重复登记拒（防借供给边开旁路）。
    {
        let mut p = ready_skeleton();
        assert!(p.declare_supply(SEG_FONT, SEG_GLYPH, "FontTables").is_ok());
        assert_eq!(p.supplies().len(), 1);
        assert_eq!(p.supplies()[0].payload, "FontTables");
        assert!(p.verify_flow(), "供给边不参与数据流核验");
        assert!(p.declare_supply(1, 1, "SelfLoop").is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SUPPLY_SELF_LOOP"));
        assert!(p.declare_supply(0, 1, "Shadow").is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SUPPLY_SHADOWS_FLOW"));
        assert!(p.declare_supply(0, 9, "Oob").is_err());
        set.add("E01-架构-供给边分型", true, "");
    }

    // ---- 三向兑现 ----

    // 判据：三轴判据逐条挂证据才兑现；空证据与临时新增判据一律拒。
    {
        let mut p = ready_skeleton();
        assert_eq!(p.three_way().len(), 3, "三向齐备");
        let axes_ok = p.three_way()[0].0 == Axis::Glyph
            && p.three_way()[1].0 == Axis::Layout
            && p.three_way()[2].0 == Axis::Render;
        let promises = p
            .commitments()
            .iter()
            .all(|c| c.promise.contains("质量") || c.promise.contains("正确") || c.promise.contains("预算"));
        assert!(!p.three_way_fulfilled(), "初始未兑现");
        let pending0 = p.three_way()[0].2;
        assert_eq!(pending0, 3, "字形向三条判据待证");
        // 空证据拒（不静默接受）。
        let first = p.commitments()[0].criteria[0].text.clone();
        assert!(!p.attach_evidence(Axis::Glyph, &first, ""));
        assert!(p.warnings().iter().any(|w| w.contains("W_EMPTY_EVIDENCE")));
        // 判据集唯一源：临时新增判据拒。
        assert!(!p.attach_evidence(Axis::Glyph, "临时判据", "x"));
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_CRITERION_NOT_FOUND"));
        // 不在册轴取不到承诺。
        assert!(p.commitment(Axis::Layout).is_some());
        // 逐轴兑现。
        for c in p.commitments()[0].criteria.clone() {
            assert!(p.attach_evidence(Axis::Glyph, &c.text, "金样剖面 0.5% 内"));
        }
        assert_eq!(p.three_way()[0].2, 0, "字形向兑现");
        assert!(!p.three_way_fulfilled(), "排版/渲染未兑现仍不算齐");
        let all = fulfil_three_way(&mut p);
        assert!(all && p.three_way().iter().all(|(_, ok, pend)| *ok && *pend == 0));
        set.add("E01-兑现-三轴判据挂证据", axes_ok && promises && all, "");
    }

    // ---- 复用声明 + 六件套 ----

    // 判据：复用 X01 开工范式并出具声明；六件套缺槽不得开工；未知槽显性拒。
    {
        let mut p = ready_skeleton();
        assert!(!p.open_pack_ready());
        assert_eq!(p.open_pack().missing().len(), 6);
        for s in OPEN_PACK_SLOTS {
            assert!(p.fill_open_slot(s, "已填"));
        }
        assert!(p.open_pack().complete());
        assert!(!p.open_pack_ready(), "复用声明未出具仍不算齐");
        assert!(!p.fill_open_slot("第七槽", "x"));
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_OPEN_SLOT_UNKNOWN"));
        p.issue_reuse_declaration();
        assert!(p.open_pack_ready());
        let r = p.reuse().unwrap();
        let decl_ok = r.paradigm == "X01 开工范式"
            && r.items.len() == OPEN_PACK_SLOTS.len()
            && r.items.iter().all(|i| i.starts_with("六件套槽："))
            && r.divergence.contains("同构不同参");
        assert!(X01_REUSE_DOC.contains("六件套") && X01_REUSE_DOC.contains("X01 开工范式"));
        // OPEN_PACK_SLOTS 顺序即 X01 体例。
        assert_eq!(
            OPEN_PACK_SLOTS,
            ["定位", "架构", "契约", "预算", "风险", "里程碑"]
        );
        set.add("E01-复用-X01声明与六件套", decl_ok, "");
    }

    // ---- 1.5ms 预算 + 三水位 + 降级序 ----

    // 判据：六段分解自洽（合计 1500µs）；三水位阈值；超预算记账；降级序三步且
    // 无障碍路径受保护。
    {
        let mut p = ready_skeleton();
        assert!(p.budget_consistent());
        assert_eq!(FRAME_BUDGET_US, 1500);
        let sum: u32 = BUDGET_SLICES.iter().map(|(_, v)| *v).sum();
        assert_eq!(sum, 1500, "六段分解合计 = 预算");
        assert_eq!(TELEMETRY_SLICE_US, 20, "遥测段 0.02ms");
        // 三水位阈值。
        assert_eq!(Water::judge(WATER_GREEN_MAX_US), Water::Green);
        assert_eq!(Water::judge(WATER_GREEN_MAX_US + 1), Water::Yellow);
        assert_eq!(Water::judge(WATER_YELLOW_MAX_US), Water::Yellow);
        assert_eq!(Water::judge(WATER_YELLOW_MAX_US + 1), Water::Red);
        assert_eq!(WATER_GREEN_MAX_US, 300);
        assert_eq!(WATER_YELLOW_MAX_US, 750);
        assert_eq!(Water::Green.name(), "绿");
        // 段配额超限告警。
        assert!(p.charge(1, BUDGET_SLICES[1].1 + 1).is_ok());
        assert!(p.warnings().iter().any(|w| w.contains("W_SLICE_OVER")));
        // 总超预算 → 记账 + 出降级序建议，不静默截断。
        assert!(p.charge(2, 1000).is_ok());
        let w = p.settle_frame();
        assert_eq!(w, Water::Red);
        assert_eq!(p.budget_breaches(), 1);
        let e = p.errors().last().unwrap();
        assert_eq!(e.1, "E_BUDGET_BREACH");
        assert!(e.2.contains("超架构预算"));
        assert_eq!(p.frame_total_us(), 0, "结算后清零");
        // 绿帧不记账错误。
        let errs_before = p.errors().len();
        assert!(p.charge(4, TELEMETRY_SLICE_US).is_ok());
        assert_eq!(p.settle_frame(), Water::Green);
        assert_eq!(p.errors().len(), errs_before, "绿帧零错误记账");
        assert!(p.charge(9, 1).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SLICE_OUT_OF_RANGE"));
        set.add("E01-预算-1.5ms与三水位", true, "");
    }

    // ---- 降级序与无障碍红线 ----

    // 判据：降级序= Hinting → 亚像素相位 → 动字号采样率；无障碍路径永不在序内，
    // 申报越线即拒。
    {
        let mut p = ready_skeleton();
        let plan = p.degradation_plan();
        let order_ok = plan.len() == 3
            && plan[0].name.contains("Hinting")
            && plan[1].name.contains("亚像素")
            && plan[2].name.contains("动字号")
            && plan[0].order == 0
            && plan[1].order == 1
            && plan[2].order == 2;
        let a11y_clean = plan.iter().all(|s| !s.touches_a11y);
        assert!(p.declare_degrade_step(0, "降 Hinting 档", false).is_ok());
        assert!(p.declare_degrade_step(1, "关高对比模式", true).is_err());
        assert!(p.declare_degrade_step(2, "砍放大倍率", true).is_err());
        assert_eq!(
            p.errors().iter().filter(|(_, c, _)| *c == "E_A11Y_PROTECTED").count(),
            2
        );
        set.add("E01-降级-序位与无障碍红线", order_ok && a11y_clean, "");
    }

    // ---- ADR 门禁 ----

    // 判据：改已冻结接口必先有已接受 ADR；提案态/否决态/不存在皆拒；指纹随变更。
    {
        let mut p = ready_skeleton();
        let before = p.iface(0).unwrap().fingerprint();
        let new_spec = InterfaceSpec::new("GlyphSink", 2, &["glyph_id", "bitmap", "advance"]);
        assert!(p.revise_interface(0, new_spec.clone(), 999).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_ADR_REQUIRED"));
        let id = p.propose_adr("GlyphSink 字段精简", "去掉冗余字段，降契约体积", &[SEG_GLYPH]);
        assert_eq!(p.adrs().len(), 1);
        assert_eq!(p.adrs()[0].state, AdrState::Proposed);
        assert!(p.revise_interface(0, new_spec.clone(), id).is_err(), "提案态不可用");
        assert!(p.accept_adr(id).is_ok());
        let after = p.revise_interface(0, new_spec, id).unwrap();
        assert_ne!(before, after);
        assert_eq!(p.iface(0).unwrap().adr, Some(id));
        assert_eq!(p.iface(0).unwrap().version, 2);
        assert!(p.iface(0).unwrap().frozen(), "改后仍冻结");
        // 否决态不可用；不存在编号显性报错。
        let id2 = p.propose_adr("换后端", "评估中", &[SEG_RENDER]);
        assert!(p.reject_adr(id2).is_ok());
        assert!(p.revise_interface(1, InterfaceSpec::new("X", 1, &[]), id2).is_err());
        assert!(p.accept_adr(4242).is_err());
        assert!(p.reject_adr(4242).is_err());
        assert_eq!(
            p.errors().iter().filter(|(_, c, _)| *c == "E_ADR_NOT_FOUND").count(),
            2
        );
        // 槽位越界显性拒。
        assert!(p.revise_interface(9, InterfaceSpec::new("X", 1, &[]), id).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_SLOT_OUT_OF_RANGE"));
        set.add("E01-变更-ADR门禁", true, "");
    }

    // ---- 域就绪闸 ----

    // 判据：六闸（签收/复用/六件套/四段开工/单向流/三向兑现+预算自洽）齐绿才就绪；
    // 缺项以人话列出。
    {
        let mut p = TextPipeline::new();
        assert!(!p.domain_ready());
        assert!(!p.not_ready_reasons().is_empty());
        assert!(!p.open_gates_ready());
        p.signoff_s117();
        for slot in 0..STAGE_COUNT - 1 {
            let _ = p.freeze_interface(slot);
        }
        for stage in 0..STAGE_COUNT {
            let _ = p.open_stage(stage);
        }
        for slot in 0..STAGE_COUNT - 1 {
            let _ = p.declare_flow(slot, slot + 1);
        }
        assert!(!p.open_gates_ready(), "六件套未齐");
        pack_ready(&mut p);
        assert!(p.open_gates_ready());
        assert!(!p.domain_ready(), "三向未兑现不得就绪");
        assert!(p.not_ready_reasons().iter().any(|r| r.contains("字形向")));
        assert!(fulfil_three_way(&mut p));
        assert!(p.domain_ready(), "六闸齐绿");
        assert!(p.not_ready_reasons().is_empty());
        let rep = p.domain_report();
        let rep_ok = rep.contains("文字渲染域总架构")
            && rep.contains("段 0 编码字形")
            && rep.contains("域就绪：是")
            && rep.contains("1500µs/帧");
        // 默认构造等价。
        assert_eq!(TextPipeline::default().open_stage_count(), 0);
        set.add("E01-就绪-六闸齐绿", rep_ok, "");
    }

    // ---- 零静默 ----

    // 判据：每条错误都带非空可读原因与建议动作；告警账独立可查。
    {
        let mut p = TextPipeline::new();
        assert!(p.open_stage(0).is_err());
        assert!(p.declare_flow(2, 0).is_err());
        assert!(p.declare_flow(0, 3).is_err());
        assert!(p.charge(99, 1).is_err());
        assert!(p.freeze_interface(42).is_err());
        assert!(p.fill_open_slot("无此槽", "x").eq(&false));
        assert!(!p.errors().is_empty());
        let all_detail = p.errors().iter().all(|(_, c, d)| !c.is_empty() && !d.is_empty());
        let audited = !p.audits().is_empty() || p.errors().len() >= 5;
        set.add("E01-防护-零静默可读", all_detail && audited, "");
    }

    set
}