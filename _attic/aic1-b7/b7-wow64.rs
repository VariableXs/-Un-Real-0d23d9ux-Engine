
// ---------------------------------------------------------------------------
// F004 · 深化批次七：拒绝卡片动作对（两动作面——关闭 / 查 64 位替代品）
//
// 主册依据（G-A-04【用户故事】：「附『查看 64 位替代品』按钮直跳星图搜索」
// +【交互设计】：「两个动作（关闭/查替代）」——动作对是卡片交互的契约面
/// （与 HonestCard.alt_query 既有面联动——一处一事实）。
// ---------------------------------------------------------------------------

/// 卡片动作（两动作契约——不做第三动作：拒绝卡片不放危险位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardAction {
    Close,
    FindAlternative,
}

/// 动作对（显示文案 + 键盘焦点序——Tab 先到主动作「查替代」再「关闭」，
/// 破坏性/放弃性动作殿后——交互词典一致性）。
pub const CARD_ACTION_ORDER: [(CardAction, &'static str); 2] = [
    (CardAction::FindAlternative, "查看 64 位替代品"),
    (CardAction::Close, "关闭"),
];

/// F004 深化批次七自检。
pub fn run_wow64_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep6");
    // 1) 动作对契约：恰好两动作、文案非空、主动作在前（Tab 序）。
    cs.add(
        "card_action_pair_contract",
        CARD_ACTION_ORDER.len() == 2
            && CARD_ACTION_ORDER[0].0 == CardAction::FindAlternative
            && CARD_ACTION_ORDER[1].0 == CardAction::Close
            && CARD_ACTION_ORDER.iter().all(|(_, t)| !t.is_empty()),
        "",
    );
    // 2) 与 HonestCard 联动锚：卡片自带的 alt_query 供「查替代」直跳星图
    //    （既有面——动作与数据源对得上）。
    let card = MachineVerdict::ThirtyTwo.honest_card();
    cs.add(
        "card_action_data_link",
        matches!(card, Some(c) if !c.alt_query.is_empty()),
        "",
    );
    cs
}
