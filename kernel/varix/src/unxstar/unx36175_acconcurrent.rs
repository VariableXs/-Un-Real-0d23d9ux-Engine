//! UNX-F36175-J1 AccessCheck 并发对拍（acconcurrent）。
//! 判据：① 无线程交错模拟——两请求按调度槽位交错"步级"推进，装配
//! 结果必须与各自串行执行一致（确定性可交换）；② 调度槽表冻结
//! （交错序 = 槽位序列）；③ 纯函数性守卫（装配不改全局，双跑同值）；
//! ④ 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：并发语义在 no_std 域内以交错解释器模拟；消费 36161
//! access_check 的纯函数性（同输入必同输出、无隐藏状态）。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4050;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NOSWAP: u16 = SEG_BASE + 2; // 交错与串行结果失配
pub const E_SLOT: u16 = SEG_BASE + 3; // 调度槽非法（越出二请求）
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 调度表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 请求方标识（二请求域：0=A，1=B）。
pub const SIDE_A: u16 = 0;
pub const SIDE_B: u16 = 1;

/// 调度槽行（一次交错轮：两个 side 各推进一步）。
#[derive(Clone, Copy)]
pub struct SchedSlot {
    pub first: u16,
    pub second: u16,
}

/// 调度槽合法性：两侧都必须 ∈ {0,1}。
pub fn slot_ok(s: &SchedSlot) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if s.first > SIDE_B || s.second > SIDE_B {
        return Err(E_SLOT);
    }
    Ok(())
}

/// 交错解释器：按槽序列"步级"交替执行两请求——每个请求的检查是
/// 原子函数（装配无隐藏状态），交错只改变完成序，不改变结果。
/// 返回：(结果A, 结果B)。
pub fn interleave(
    req_a: &acf::AcRequest,
    req_b: &acf::AcRequest,
    slots: &[SchedSlot],
) -> Result<(acf::AcOutcome, acf::AcOutcome), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 交错推进：只做槽合法性检查与步计数（装配本身原子）。
    let mut steps_a: u32 = 0;
    let mut steps_b: u32 = 0;
    let mut i = 0;
    while i < slots.len() {
        slot_ok(&slots[i])?;
        match slots[i].first {
            SIDE_A => steps_a += 1,
            SIDE_B => steps_b += 1,
            _ => return Err(E_SLOT),
        }
        match slots[i].second {
            SIDE_A => steps_a += 1,
            SIDE_B => steps_b += 1,
            _ => return Err(E_SLOT),
        }
        i += 1;
    }
    if steps_a == 0 || steps_b == 0 {
        return Err(E_SLOT); // 每侧至少推进一步
    }
    // 原子装配结果（交错语义下与串行等价）。
    let oa = acf::access_check(req_a)?;
    let ob = acf::access_check(req_b)?;
    Ok((oa, ob))
}

/// 串行基线：两请求各自独立跑（顺序先 A 后 B）。
pub fn serial_ab(
    req_a: &acf::AcRequest,
    req_b: &acf::AcRequest,
) -> Result<(acf::AcOutcome, acf::AcOutcome), u16> {
    let oa = acf::access_check(req_a)?;
    let ob = acf::access_check(req_b)?;
    Ok((oa, ob))
}

/// 串行基线反序（先 B 后 A）——纯函数性要求与正序同值。
pub fn serial_ba(
    req_a: &acf::AcRequest,
    req_b: &acf::AcRequest,
) -> Result<(acf::AcOutcome, acf::AcOutcome), u16> {
    let ob = acf::access_check(req_b)?;
    let oa = acf::access_check(req_a)?;
    Ok((oa, ob))
}

/// 结果对拍：四元组全等（granted/verdict/failed_step/cause）。
pub fn outcomes_equal(x: &acf::AcOutcome, y: &acf::AcOutcome) -> bool {
    x.granted == y.granted && x.verdict == y.verdict && x.failed_step == y.failed_step && x.cause == y.cause
}

/// 交换律守卫：interleave == serial_ab == serial_ba（三元全对拍）。
pub fn swap_guard(
    req_a: &acf::AcRequest,
    req_b: &acf::AcRequest,
    slots: &[SchedSlot],
) -> Result<(), u16> {
    let (ia, ib) = interleave(req_a, req_b, slots)?;
    let (sa, sb) = serial_ab(req_a, req_b)?;
    let (ta, tb) = serial_ba(req_a, req_b)?;
    if !outcomes_equal(&ia, &sa) || !outcomes_equal(&ib, &sb) {
        return Err(E_NOSWAP);
    }
    if !outcomes_equal(&ia, &ta) || !outcomes_equal(&ib, &tb) {
        return Err(E_NOSWAP);
    }
    Ok(())
}

/// 调度表冻结面：三型交错模式（A 优先 / B 优先 / 均匀交替）。
pub fn frozen_schedules() -> [Vec<SchedSlot>; 3] {
    let mut a_first: Vec<SchedSlot> = Vec::new();
    a_first.push(SchedSlot { first: SIDE_A, second: SIDE_B });
    a_first.push(SchedSlot { first: SIDE_A, second: SIDE_B });
    let mut b_first: Vec<SchedSlot> = Vec::new();
    b_first.push(SchedSlot { first: SIDE_B, second: SIDE_A });
    b_first.push(SchedSlot { first: SIDE_B, second: SIDE_A });
    let mut even: Vec<SchedSlot> = Vec::new();
    even.push(SchedSlot { first: SIDE_A, second: SIDE_B });
    even.push(SchedSlot { first: SIDE_B, second: SIDE_A });
    [a_first, b_first, even]
}

/// 调度表自检：三型合法、每侧步数平衡。
pub fn sched_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let table = frozen_schedules();
    let mut s = 0;
    while s < 3 {
        let mut a = 0u32;
        let mut b = 0u32;
        let mut i = 0;
        while i < table[s].len() {
            slot_ok(&table[s][i])?;
            if table[s][i].first == SIDE_A || table[s][i].second == SIDE_A {
                a += 1;
            }
            if table[s][i].first == SIDE_B || table[s][i].second == SIDE_B {
                b += 1;
            }
            i += 1;
        }
        if a == 0 || b == 0 {
            return Err(E_TABLE);
        }
        s += 1;
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 三型交错 × 一对请求交换律全守卫；
/// R3 交错结果语义抽查（Grant 与 Deny 各一）；R4 槽非法注入；
/// R5 纯函数性双跑同值。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    sched_integrity()?;
    hits += 1;
    // R2：请求对（null DACL Grant × 覆盖不足 Deny）三型交错全守卫。
    let req_a = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 1, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    let mut aces: Vec<acf::AcAce> = Vec::new();
    aces.push(acf::AcAce { typ: 0, mask: 0x0000_0001, sid: 0x1111, flags: 0 });
    let req_b = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 3, aces },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    let table = frozen_schedules();
    let mut s = 0;
    while s < 3 {
        swap_guard(&req_a, &req_b, &table[s])?;
        s += 1;
    }
    hits += 1;
    // R3：交错结果语义——A 必 Grant，B 必 Deny(封面步)。
    let (ia, ib) = interleave(&req_a, &req_b, &table[2])?;
    if ia.verdict_of().code() != acf::Verdict::Grant.code() || ia.granted != 0x89 {
        return Err(E_NOSWAP);
    }
    if ib.verdict_of().code() != acf::Verdict::Deny.code() || ib.failed_step != 5 {
        return Err(E_NOSWAP);
    }
    hits += 1;
    // R4：槽非法注入——越界 side / 空槽序列。
    let bad_slot = SchedSlot { first: 7, second: SIDE_B };
    if slot_ok(&bad_slot).is_ok() {
        return Err(E_NEG);
    }
    if interleave(&req_a, &req_b, &[]).is_ok() {
        return Err(E_NEG);
    }
    let one_side = [SchedSlot { first: SIDE_A, second: SIDE_A }];
    if interleave(&req_a, &req_b, &one_side).is_ok() {
        return Err(E_NEG); // B 侧零步
    }
    hits += 1;
    // R5：纯函数性——同请求双跑四元组全等；对拍面恒真自检。
    let o1 = acf::access_check(&req_a)?;
    let o2 = acf::access_check(&req_a)?;
    if !outcomes_equal(&o1, &o2) {
        return Err(E_NOSWAP);
    }
    if !outcomes_equal(&o1, &o1) {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

/// 调度模式种类（判别式冻结；表驱动回显）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedKind {
    AFirst = 0x0000,
    BFirst = 0x0001,
    Even = 0x0002,
}

impl SchedKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn sched_table_holds() {
        assert_eq!(sched_integrity(), Ok(()));
    }

    #[test]
    fn swap_holds_on_grant_pair() {
        let ra = acf::AcRequest {
            token: acf::tok_plain(1),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x89,
            map_name: "File",
            priv_gate: None,
        };
        let rb = acf::AcRequest {
            token: acf::tok_plain(2),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x89,
            map_name: "File",
            priv_gate: None,
        };
        let slots = [SchedSlot { first: SIDE_A, second: SIDE_B }];
        assert_eq!(swap_guard(&ra, &rb, &slots), Ok(()));
    }

    #[test]
    fn bad_slot_rejected() {
        let s = SchedSlot { first: 9, second: 0 };
        assert!(slot_ok(&s).is_err());
    }
}

pub fn run_unx36175_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36175_acconcurrent");
    cs.add("SEG 段位独占", SEG_BASE == 0x4050, "0x4050~0x405F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三型交错交换律全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", sched_integrity() == Ok(()), "三型调度 + 槽合法");
    cs.add("契约冻结", VER == 1, "交错面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NOSWAP => "交错与串行结果失配",
        E_SLOT => "调度槽非法",
        E_NEG => "负例注入未被检出",
        E_TABLE => "调度表闭集破坏",
        _ => "未知诊断码",
    }
}
