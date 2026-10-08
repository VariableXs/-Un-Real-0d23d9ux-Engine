//! vct01_tbridge_checks —— CGPU-F3041 T 域开工与 VE 对接总架构 · 域自检（判据逐条映射，四组+META 14 项）
//!
//! 锚点判据 → 判据组：
//! - 签收（S10 移交包七件逐件对账）→ [`group_ack`]
//! - 定位声明（协议桥梁在册）→ [`group_position`]
//! - 四段（命令/状态/资产/事件闭集）→ [`group_segment`]
//! - 兑现确认（四段→四批预告兑现机检）→ [`group_plan`]
//! - 码段（独占段+守门）→ [`group_meta`]
//! - 判据（收口自检）→ [`group_meta2`]
//!
//! 双向验证纪律：七件齐才签收、缺一件必拒（恒绿签收不行）；四段闭集
//! 外段序必拒（闭集没有第五段）；预告兑现必须无缝（区间重叠与缝隙都红）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::cgpu::vct01_tbridge::*;

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vct01_checks() -> CheckSet {
    let mut set = CheckSet::new("CGPU-F3041");
    group_ack(&mut set);
    group_position(&mut set);
    group_segment(&mut set);
    group_plan(&mut set);
    group_meta(&mut set);
    group_meta2(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 签收
// ---------------------------------------------------------------------------

fn group_ack(set: &mut CheckSet) {
    // ① 七件齐签收成功且 acked=7（衔接确认的正面路径）。
    let all = [1u8, 2, 3, 4, 5, 6, 7];
    let ack = ack_handover(&all).ok();
    let ack_ok = matches!(ack, Some(HandoverAck { acked: 7, from_batch: "S10" }));
    set.add(
        "C3041-ACK-01 七件齐签收 acked=7",
        ack_ok,
        "S10 移交包七件逐件在册——衔接确认=签收单出且件数恰七",
    );

    // ② 缺一件必拒（六件不签——缺件签收是空头支票）。
    let mut short = Vec::new();
    let mut s = 1u8;
    while s <= 6 {
        short.push(s);
        s += 1;
    }
    let short_r = ack_handover(&short);
    // ③ 越界件序必拒（0 与 8 都不是移交包内容）。
    let oob = [1u8, 2, 3, 4, 5, 6, 8];
    let oob_r = ack_handover(&oob);
    // ④ 重复件序去重后仍齐七件可签（集合语义：重复不增加件数）。
    let dup = [1u8, 1, 2, 3, 4, 5, 6, 7];
    let dup_r = ack_handover(&dup).ok();
    let reject_ok = matches!(short_r, Err(TbCode::HANDOVER_INCOMPLETE))
        && matches!(oob_r, Err(TbCode::HANDOVER_INCOMPLETE))
        && matches!(dup_r, Some(HandoverAck { acked: 7, .. }));
    set.add(
        "C3041-ACK-02 缺件/越界必拒且重复件序去重不增件数",
        reject_ok,
        "六件不签+件序 8 越界拒；重复 seq 按集合语义去重——账实相符",
    );

    // ⑤ 衔接包核心约定实挂：会话上下文标签双非零合法、零哨兵拒绝。
    let good = SessionCtxTag { session_id: 7, user_tag: 42 };
    let bad_s = SessionCtxTag { session_id: 0, user_tag: 42 };
    let bad_u = SessionCtxTag { session_id: 7, user_tag: 0 };
    let tag_ok = ctx_tag_ok(&good) && !ctx_tag_ok(&bad_s) && !ctx_tag_ok(&bad_u);
    set.add(
        "C3041-ACK-03 衔接包会话上下文标签双非零哨兵校验",
        tag_ok,
        "VE对接衔接包约定落为 SessionCtxTag：零值=未携带哨兵，S域隔离不串户的跨域延续",
    );
}

// ---------------------------------------------------------------------------
// 定位声明
// ---------------------------------------------------------------------------

fn group_position(set: &mut CheckSet) {
    // ⑥ 定位三元组逐字在册（锚点原文 grep 面）+ 信封校验双维度。
    let p_ok = BRIDGE_POSITIONING.initiator == "VE引擎"
        && BRIDGE_POSITIONING.executor == "CGPU加速器"
        && BRIDGE_POSITIONING.relation == "协议桥梁(加速而非替代)";
    let env_good = CmdEnvelope {
        segment: 0,
        ctx: SessionCtxTag { session_id: 1, user_tag: 1 },
    };
    let env_bad_seg = CmdEnvelope {
        segment: 4,
        ctx: SessionCtxTag { session_id: 1, user_tag: 1 },
    };
    let env_bad_ctx = CmdEnvelope {
        segment: 2,
        ctx: SessionCtxTag { session_id: 0, user_tag: 1 },
    };
    let env_ok = envelope_ok(&env_good)
        && !envelope_ok(&env_bad_seg)
        && !envelope_ok(&env_bad_ctx)
        && bridge_ready(&[env_good, env_bad_seg]).is_err()
        && bridge_ready(&[env_good]).ok() == Some(1);
    set.add(
        "C3041-POS-01 定位三元组逐字在册且信封段与标签双校验",
        p_ok && env_ok,
        "协议桥梁定位=发起VE/执行CGPU/关系桥梁逐字可grep；信封校验在入桥前一次完成",
    );

    // ⑦ 预算口径：0.1ms 常量在册、零预算拒绝、界内界外双测。
    let budget_ok = BRIDGE_BUDGET_NS == 100_000
        && matches!(within_budget(99_999, BRIDGE_BUDGET_NS), Ok(true))
        && matches!(within_budget(100_000, BRIDGE_BUDGET_NS), Ok(true))
        && matches!(within_budget(100_001, BRIDGE_BUDGET_NS), Ok(false))
        && matches!(within_budget(1, 0), Err(TbCode::BUDGET));
    set.add(
        "C3041-POS-02 桥路预算 0.1ms 在册且零预算拒绝",
        budget_ok,
        "预算是契约不是感觉：恰端点在界内、超一纳秒即界外；零预算无法核验显性拒绝",
    );
}

// ---------------------------------------------------------------------------
// 四段
// ---------------------------------------------------------------------------

fn group_segment(set: &mut CheckSet) {
    // ⑧ 四段闭集：段序 0..=3 全可还原段名，段序 4 拒绝（没有第五段）。
    let n0 = Segment::from_index(0).map(|s| s.name());
    let n1 = Segment::from_index(1).map(|s| s.name());
    let n2 = Segment::from_index(2).map(|s| s.name());
    let n3 = Segment::from_index(3).map(|s| s.name());
    let n4 = Segment::from_index(4);
    let names_ok = matches!(n0, Some("命令"))
        && matches!(n1, Some("状态"))
        && matches!(n2, Some("资产"))
        && matches!(n3, Some("事件"))
        && n4.is_none();
    set.add(
        "C3041-SEG-01 四段闭集名实对应且段序4拒绝",
        names_ok,
        "命令/状态/资产/事件= S10 预告口径逐字还原；闭集之外不猜测",
    );

    // ⑨ 四段互异（enum 语义+映射行两两不等双证）。
    let mut all_diff = true;
    let mut i = 0usize;
    while i < SEGMENT_PLAN.len() {
        let mut j = i + 1;
        while j < SEGMENT_PLAN.len() {
            if SEGMENT_PLAN[i].seg == SEGMENT_PLAN[j].seg {
                all_diff = false;
            }
            j += 1;
        }
        i += 1;
    }
    let seg_diff = Segment::Command != Segment::State
        && Segment::State != Segment::Asset
        && Segment::Asset != Segment::Event
        && all_diff;
    set.add(
        "C3041-SEG-02 四段两两互异双证",
        seg_diff,
        "段语义互异是四段架构的前提：enum 相等性与映射行相等性双证",
    );

    // ⑩ 段→映射行全可查（闭集内每段都有兑现行——无映射=预告落空）。
    let plan_ok = segment_plan(Segment::Command).is_some()
        && segment_plan(Segment::State).is_some()
        && segment_plan(Segment::Asset).is_some()
        && segment_plan(Segment::Event).is_some();
    set.add(
        "C3041-SEG-03 四段兑现映射全可查",
        plan_ok,
        "逐段实查 segment_plan 非空——预告的四段每段都有去处",
    );
}

// ---------------------------------------------------------------------------
// 兑现确认
// ---------------------------------------------------------------------------

fn group_plan(set: &mut CheckSet) {
    // ⑪ 预告兑现审计：四批区间无缝且命令段首单=3042（紧随开工单）。
    let audit = plan_audit();
    set.add(
        "C3041-PLAN-01 四段→四批区间无缝且首单紧随开工",
        audit,
        "S10 预告兑现的机检面：T02-T05 四批 F3042-F3105 首尾相接无缺口无重叠",
    );

    // ⑫ 兑现行职责句非空且逐行独立（四行职责两两不同）。
    let mut duty_diff = true;
    let mut i = 0usize;
    while i < SEGMENT_PLAN.len() {
        if SEGMENT_PLAN[i].duty.is_empty() {
            duty_diff = false;
        }
        let mut j = i + 1;
        while j < SEGMENT_PLAN.len() {
            if SEGMENT_PLAN[i].duty == SEGMENT_PLAN[j].duty {
                duty_diff = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C3041-PLAN-02 四批职责句逐行独立非空",
        duty_diff,
        "职责句是各段的语义边界：空句=职责缺失、重复句=段责混淆",
    );

    // ⑬ 批次名与单号带逐行对拍（T02-T05 名与 first_id 对应）。
    let batch_ok = SEGMENT_PLAN[0].batch == "T02命令通道协议" && SEGMENT_PLAN[0].first_id == 3042
        && SEGMENT_PLAN[1].batch == "T03状态同步协议" && SEGMENT_PLAN[1].first_id == 3058
        && SEGMENT_PLAN[2].batch == "T04资产互操作协议" && SEGMENT_PLAN[2].first_id == 3074
        && SEGMENT_PLAN[3].batch == "T05事件流协议" && SEGMENT_PLAN[3].first_id == 3090;
    set.add(
        "C3041-PLAN-03 四批名与首单号逐行对拍",
        batch_ok,
        "批次名-首单号一一对应：预告兑现不仅要无缝还要名实相符",
    );
}

// ---------------------------------------------------------------------------
// 码段与判据收口
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑭ 码段独占：0x5A 高字节（0x52-0x59 为既有 cgpu 域占用）。
    set.add(
        "C3041-META-00 码段独占 0x5A",
        TbCode::HANDOVER_INCOMPLETE.code() & 0xFF00 == 0x5A00
            && TbCode::SESSION_TAG.code() & 0xFF00 == 0x5A00
            && TbCode::SEGMENT_INVALID.code() & 0xFF00 == 0x5A00
            && TbCode::BUDGET.code() & 0xFF00 == 0x5A00
            && TbCode::ENVELOPE.code() & 0xFF00 == 0x5A00
            && TbCode::HANDOVER_INCOMPLETE.code() != TbCode::SESSION_TAG.code()
            && TbCode::SEGMENT_INVALID.code() != TbCode::BUDGET.code()
            && TbCode::ENVELOPE.code() != TbCode::HANDOVER_INCOMPLETE.code()
            && TbCode::SESSION_TAG.code() != TbCode::ENVELOPE.code(),
        "码段互异判据用 != 防自判死；跨域分账 0x52-0x59 已占/0x5A 为 T 域开工段",
    );
}

fn group_meta2(set: &mut CheckSet) {
    // ⑮ 实挂条数从 CheckSet 实取：前五组 12 条，META2 段 2 条，合计 14。
    let before = set.len();
    set.add(
        "C3041-META-01 实挂条数+2(META2)=声明条数14",
        before == 12 && before + 2 == 14,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑯ 判据名全集互异。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C3041-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 14,
        "重名判据让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
