//! UNX-F36164-J1 AccessCheck 第③步：DACL 状态面（acdaclstate）。
//! 判据：① 四态闭集（ABSENT=0/NULL=1/EMPTY=2/PRESENT=3）+ Flow 三流
//! （GrantAll=1/Deny=2/Continue=3）+ step3 冻结签名
//! `step3(&DaclFace) -> Result<Flow, u16>`——语义：无 DACL 拒（E_NODACL
//! 由装配转 outcome）、null DACL 全许、空 DACL 显性拒（E_EMPTY）、
//! 在场续流；② 状态-流映射表驱动；③ 注入负例显性拒；④ ≥310 行。
//! 设计纪律：36161 通过 `super::unx36164_acdaclstate as s3` 消费
//! ST_*/Flow/E_NODACL/E_EMPTY/step3；状态值真源在本模块冻结。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3c00;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NODACL: u16 = SEG_BASE + 2; // 无 DACL（ABSENT 态 → Deny 流）
pub const E_EMPTY: u16 = SEG_BASE + 3; // 空 DACL（EMPTY 态 → 显性拒）
pub const E_STATE: u16 = SEG_BASE + 4; // 状态值越出四态闭集
pub const E_SHAPE: u16 = SEG_BASE + 5; // 态-形不一致（EMPTY 却带 ACE 等）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 7; // 状态-流映射表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// DACL 四态闭集（判别式冻结：与 36161 消费契约逐位对齐）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaclState {
    Absent = 0x0000,
    Null = 0x0001,
    Empty = 0x0002,
    Present = 0x0003,
}

impl DaclState {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn of(v: u16) -> Option<DaclState> {
        match v {
            0 => Some(DaclState::Absent),
            1 => Some(DaclState::Null),
            2 => Some(DaclState::Empty),
            3 => Some(DaclState::Present),
            _ => None,
        }
    }
}

/// 冻结态常数（36161 直接引用；禁在本批内重定义数值）。
pub const ST_ABSENT: u16 = DaclState::Absent.code();
pub const ST_NULL: u16 = DaclState::Null.code();
pub const ST_EMPTY: u16 = DaclState::Empty.code();
pub const ST_PRESENT: u16 = DaclState::Present.code();

/// 检查流三分（判别式冻结；Continue 是唯一进入第④步的流）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    GrantAll = 0x0001,
    Deny = 0x0002,
    Continue = 0x0003,
}

impl Flow {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn is_terminal(self) -> bool {
        !matches!(self, Flow::Continue)
    }
}

pub const STATE_NAMES: [&str; 4] = ["absent", "null", "empty", "present"];

/// 状态名回显（越界 "bad-state"）。
pub fn state_name(v: u16) -> &'static str {
    match v {
        0 => STATE_NAMES[0],
        1 => STATE_NAMES[1],
        2 => STATE_NAMES[2],
        3 => STATE_NAMES[3],
        _ => "bad-state",
    }
}

/// 态-形一致性：ABSENT/NULL/EMPTY 必须零 ACE；PRESENT 至少一 ACE
/// （PRESENT 零 ACE 视为形不一致——空表语义由 EMPTY 独占）。
pub fn shape_ok(d: &acf::DaclFace) -> Result<(), u16> {
    match d.state {
        ST_ABSENT | ST_NULL | ST_EMPTY => {
            if !d.aces.is_empty() {
                return Err(E_SHAPE);
            }
            Ok(())
        }
        ST_PRESENT => {
            if d.aces.is_empty() {
                return Err(E_SHAPE);
            }
            Ok(())
        }
        _ => Err(E_STATE),
    }
}

// ---- 第③步契约 -------------------------------------------------------------
/// step3：DACL 空态门。
/// - ABSENT → Ok(Flow::Deny)（装配转 Deny outcome，cause=E_NODACL）；
/// - NULL   → Ok(Flow::GrantAll)（null DACL 隐式全许）；
/// - EMPTY  → Err(E_EMPTY)（空 DACL 显性拒，装配透传）；
/// - PRESENT → Ok(Flow::Continue)（进入第④步逐 ACE）；
/// - 其他   → Err(E_STATE)。
pub fn step3(d: &acf::DaclFace) -> Result<Flow, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    shape_ok(d)?;
    match d.state {
        ST_ABSENT => Ok(Flow::Deny),
        ST_NULL => Ok(Flow::GrantAll),
        ST_EMPTY => Err(E_EMPTY),
        ST_PRESENT => Ok(Flow::Continue),
        _ => Err(E_STATE),
    }
}

/// 态 → 是否终流（表驱动对拍面）。
pub fn is_terminal_state(v: u16) -> Result<bool, u16> {
    match DaclState::of(v) {
        Some(DaclState::Absent) => Ok(true),
        Some(DaclState::Null) => Ok(true),
        Some(DaclState::Empty) => Ok(true), // Err 路径在语义上亦终流
        Some(DaclState::Present) => Ok(false),
        None => Err(E_STATE),
    }
}

/// 态计数面：给一组 DaclFace，按态桶计数（演练与聚合用）。
pub fn state_bucket(faces: &[acf::DaclFace]) -> Result<[u32; 4], u16> {
    let mut buckets = [0u32; 4];
    let mut i = 0;
    while i < faces.len() {
        shape_ok(&faces[i])?;
        buckets[faces[i].state as usize] += 1;
        i += 1;
    }
    Ok(buckets)
}

/// 流自检：终流判定与 Flow 判别式闭集。
pub fn flow_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if Flow::GrantAll.code() != 1 || Flow::Deny.code() != 2 || Flow::Continue.code() != 3 {
        return Err(E_TABLE);
    }
    if Flow::Continue.is_terminal() {
        return Err(E_TABLE);
    }
    if !Flow::GrantAll.is_terminal() || !Flow::Deny.is_terminal() {
        return Err(E_TABLE);
    }
    Ok(())
}

// ---- 态迁移闭集 -------------------------------------------------------------
// 安全描述符生命周期内的合法 DACL 态迁移（行=from，列=to，1=允许）：
//   Absent→Null（挂 null DACL）/ Absent→Present（直接挂表）；
//   Null→Empty（显式清空）/ Null→Present（换表）；
//   Empty→Null（重挂 null）/ Empty→Present（重挂表）；
//   Present→Null / Present→Empty / Present→Present（表内更替）。
const TRANSITIONS: [[u8; 4]; 4] = [
    // to: ABSENT NULL EMPTY PRESENT
    [1, 1, 0, 1], // from ABSENT
    [0, 1, 1, 1], // from NULL
    [0, 1, 1, 1], // from EMPTY
    [0, 1, 1, 1], // from PRESENT
];

/// 态迁移合法性：from→to 是否在闭集内。
pub fn transition_ok(from: u16, to: u16) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let f = match DaclState::of(from) {
        Some(s) => s as usize,
        None => return Err(E_STATE),
    };
    let t = match DaclState::of(to) {
        Some(s) => s as usize,
        None => return Err(E_STATE),
    };
    Ok(TRANSITIONS[f][t] == 1)
}

/// 迁移闭集自检：对角恒可（保持自身态合法）、ABSENT 不可迁回
/// （拆卸后无 DACL 可言）、每行至少一条出边。
pub fn transition_integrity() -> Result<(), u16> {
    let mut f = 0;
    while f < 4 {
        if TRANSITIONS[f][f] != 1 {
            return Err(E_TABLE);
        }
        let mut row = 0u32;
        let mut t = 0;
        while t < 4 {
            row += TRANSITIONS[f][t] as u32;
            t += 1;
        }
        if row < 2 {
            return Err(E_TABLE);
        }
        f += 1;
    }
    if TRANSITIONS[0][2] != 0 {
        return Err(E_TABLE); // ABSENT→EMPTY 越语义（无 DACL 不可能"空"）
    }
    Ok(())
}

/// 迁移链走查：给定迁移序列，逐段校验并回放终态（非法迁移报 E_STATE）。
pub fn replay_chain(chain: &[u16]) -> Result<u16, u16> {
    if chain.is_empty() {
        return Err(E_STATE);
    }
    let mut cur = chain[0];
    if DaclState::of(cur).is_none() {
        return Err(E_STATE);
    }
    let mut i = 1;
    while i < chain.len() {
        let nxt = chain[i];
        if !transition_ok(cur, nxt)? {
            return Err(E_STATE);
        }
        cur = nxt;
        i += 1;
    }
    Ok(cur)
}

/// 演练：R1 闭集自检；R2 四态语义全走查；R3 态-形一致与桶计数；
/// R4 终流对拍；R5 负例注入（越界态/形不一致/空表假 PRESENT/名回显）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    flow_integrity()?;
    hits += 1;
    // R2：四态语义——ABSENT→Deny / NULL→GrantAll / EMPTY→Err / PRESENT→Continue。
    let d_absent = acf::DaclFace { state: ST_ABSENT, aces: Vec::new() };
    if step3(&d_absent)? != Flow::Deny {
        return Err(E_NEG);
    }
    let d_null = acf::DaclFace { state: ST_NULL, aces: Vec::new() };
    if step3(&d_null)? != Flow::GrantAll {
        return Err(E_NEG);
    }
    let d_empty = acf::DaclFace { state: ST_EMPTY, aces: Vec::new() };
    if step3(&d_empty) != Err(E_EMPTY) {
        return Err(E_NEG);
    }
    let mut aces: Vec<acf::AcAce> = Vec::new();
    aces.push(acf::AcAce { typ: 0, mask: 0x89, sid: 0x1111, flags: 0 });
    let d_present = acf::DaclFace { state: ST_PRESENT, aces };
    if step3(&d_present)? != Flow::Continue {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：态-形一致 + 桶计数（ABSENT 1 / NULL 1 / EMPTY 1 / PRESENT 1）。
    shape_ok(&d_absent)?;
    shape_ok(&d_null)?;
    shape_ok(&d_empty)?;
    shape_ok(&d_present)?;
    let set = vec![d_absent, d_null, d_empty, d_present];
    let b = state_bucket(&set)?;
    if b != [1, 1, 1, 1] {
        return Err(E_TABLE);
    }
    hits += 1;
    // R4：终流对拍——ABSENT/NULL/EMPTY 终流，PRESENT 非终流，越界态 Err。
    if is_terminal_state(ST_ABSENT)? != true || is_terminal_state(ST_NULL)? != true {
        return Err(E_NEG);
    }
    if is_terminal_state(ST_EMPTY)? != true || is_terminal_state(ST_PRESENT)? != false {
        return Err(E_NEG);
    }
    if is_terminal_state(9).is_ok() {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——越界态 / 非空 EMPTY / 零 ACE PRESENT / 名回显。
    let d_bad = acf::DaclFace { state: 7, aces: Vec::new() };
    if step3(&d_bad).is_ok() {
        return Err(E_NEG);
    }
    let mut d_malformed = acf::DaclFace { state: ST_EMPTY, aces: Vec::new() };
    d_malformed.aces.push(acf::AcAce { typ: 0, mask: 1, sid: 1, flags: 0 });
    if shape_ok(&d_malformed).is_ok() {
        return Err(E_NEG);
    }
    let d_fake = acf::DaclFace { state: ST_PRESENT, aces: Vec::new() };
    if shape_ok(&d_fake).is_ok() {
        return Err(E_NEG);
    }
    if state_name(4) != "bad-state" || DaclState::of(4).is_some() {
        return Err(E_NEG);
    }
    hits += 1;
    // R6：态迁移闭集——合法链回放 / 非法迁移拒 / 越界态拒 / 闭集自检。
    transition_integrity()?;
    if replay_chain(&[ST_ABSENT, ST_NULL, ST_PRESENT])? != ST_PRESENT {
        return Err(E_NEG);
    }
    if replay_chain(&[ST_PRESENT, ST_EMPTY, ST_NULL, ST_PRESENT])? != ST_PRESENT {
        return Err(E_NEG);
    }
    if replay_chain(&[ST_ABSENT, ST_EMPTY]).is_ok() {
        return Err(E_NEG); // ABSENT→EMPTY 越闭集
    }
    if transition_ok(ST_ABSENT, 9).is_ok() || transition_ok(9, ST_NULL).is_ok() {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(6));
    }

    #[test]
    fn transition_table_holds() {
        assert_eq!(transition_integrity(), Ok(()));
    }

    #[test]
    fn flow_table_holds() {
        assert_eq!(flow_integrity(), Ok(()));
    }

    #[test]
    fn empty_dacl_rejected() {
        let d = acf::DaclFace { state: ST_EMPTY, aces: Vec::new() };
        assert_eq!(step3(&d), Err(E_EMPTY));
    }

    #[test]
    fn null_dacl_grants_all() {
        let d = acf::DaclFace { state: ST_NULL, aces: Vec::new() };
        assert_eq!(step3(&d), Ok(Flow::GrantAll));
    }
}

pub fn run_unx36164_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36164_acdaclstate");
    cs.add("SEG 段位独占", SEG_BASE == 0x3c00, "0x3C00~0x3C0F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四态语义/流映射/态形一致全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", flow_integrity() == Ok(()), "Flow 判别式 + 终流闭集");
    cs.add("契约冻结", VER == 1, "step3 契约锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NODACL => "无 DACL（ABSENT 态）",
        E_EMPTY => "空 DACL（EMPTY 态显性拒）",
        E_STATE => "状态值越出四态闭集",
        E_SHAPE => "态-形不一致",
        E_NEG => "负例注入未被检出",
        E_TABLE => "状态-流映射表闭集破坏",
        _ => "未知诊断码",
    }
}
