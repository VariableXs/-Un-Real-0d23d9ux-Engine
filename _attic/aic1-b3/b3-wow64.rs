
// ---------------------------------------------------------------------------
// F004 · 深化批次三：位数判定次序显式化（先过 peblock 门再谈位数）+ ARM64
// 如实告知话术
//
// 主册依据（G-A-04【状态与异常】）：「伪装 32 位的恶意样本照走 peblock 门，
// 先过门再谈位数」——门序是安全语义不是实现细节，钉成常量序；【设计细节】
// 「ARM64 声明也识别并如实告知」。MachineVerdict 既有面（一处一事实）。
// ---------------------------------------------------------------------------

/// 门序三步（恶意样本伪装位数也必须先过 peblock 门——次序即安全语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateStep {
    /// 第一步：peblock 门校验（规则/哈希——与位数无关）。
    Peblock,
    /// 第二步：机器位数判定（本门）。
    Machine,
    /// 第三步：装载/出诚实卡片。
    Launch,
}

/// 唯一合法门序（先过门再谈位数——一处一事实）。
pub const GATE_ORDER: [GateStep; 3] =
    [GateStep::Peblock, GateStep::Machine, GateStep::Launch];

/// 校验一段门序是否与 [`GATE_ORDER`] 全等（乱序 = 违例，如实 false）。
pub fn gate_order_respected(seq: &[GateStep]) -> bool {
    seq == GATE_ORDER
}

/// ARM64 声明话术（主册【设计细节】：识别并如实告知——三要素齐，非裸句）。
pub fn arm64_note(v: MachineVerdict) -> Option<&'static str> {
    match v {
        MachineVerdict::Arm64Declared => Some(
            "此程序声明为 ARM64 架构。VARIX 当前运行 x86-64（AMD64）程序；\
             ARM64 兼容在路线图中，可查看 64 位替代品。",
        ),
        _ => None,
    }
}

/// F004 深化批次三自检。
pub fn run_wow64_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep2");
    // 1) 门序：合法序通过；「先谈位数后过门」的乱序如实拒绝（安全语义锚）。
    cs.add(
        "gate_order_peblock_first",
        gate_order_respected(&GATE_ORDER)
            && !gate_order_respected(&[GateStep::Machine, GateStep::Peblock, GateStep::Launch]),
        "",
    );
    // 2) ARM64 话术：仅 Arm64Declared 出话术；三要素齐（含「为什么」（架构不符）
    //    与「下一步」（路线图+替代品）），禁裸句；其余判定 None。
    let note = arm64_note(MachineVerdict::Arm64Declared);
    let honest = match note {
        Some(t) => t.contains("ARM64") && t.contains("路线图") && t.contains("替代品"),
        None => false,
    };
    cs.add(
        "arm64_note_honest_three_parts",
        honest
            && arm64_note(MachineVerdict::Native64).is_none()
            && arm64_note(MachineVerdict::ThirtyTwo).is_none()
            && arm64_note(MachineVerdict::NotPe).is_none(),
        "",
    );
    // 3) 门序常量钉值（Peblock < Machine < Launch 判别序）。
    cs.add(
        "gate_order_const_pinned",
        GATE_ORDER.len() == 3 && GATE_ORDER[0] == GateStep::Peblock && GATE_ORDER[2] == GateStep::Launch,
        "",
    );
    cs
}
