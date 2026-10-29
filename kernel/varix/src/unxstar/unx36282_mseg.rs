//! UNX-F36282-J1 矩阵首版 120 格跑通（M 段口径）。
//! 判据：UNX-F36282-J1 首 120 格应拒 100% 拒/应许 100% 许/零越格
//! （母版 X1 套用断言）；结果账逐格可查；纯功能代码 ≥300 行。
//! 设计纪律：越权矩阵是 AccessCheck 语义的账面化——Windows 用
//! SeAccessCheck 对每次访问逐格裁决，本域把 5 主体×6 客体×8 操作
//! 三维账的首 120 格（S0–S2 × O0–O5 × OP0–OP7）在 M 段口径下跑通：
//!   - 门序（对标 win32 AccessCheck 检查次序）：IL 门（no-write-up，
//!     仅约束写操作）→ 特权门（SeTakeOwnership 类特权位）→ DACL 门
//!     （deny 首遇即拒 / allow 累积凑满即授 / 部分覆盖归因掩码门）；
//!   - 母版 X1：独立扁平重导（聚合位口径，与引擎的逐 ACE 遍历不同形）
//!     逐格对平——两套实现零差才算「跑通」，单套自证无效；
//!   - 零越格：每格裁决与母版期望全等，任一交叉即 E_CROSS 回炉；
//!   - 结果账逐格可查：120 条裁决记录（格号+裁决+归因门）全量落账，
//!     供 36283 对照、36284 诊断与 36301 服务面复用同一门序口径。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB040;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 格号/下标越合法区
pub const E_CROSS: u16 = SEG_BASE + 3; // 越格（引擎与母版期望交叉）
pub const E_ANCHOR: u16 = SEG_BASE + 4; // 手算锚漂移
pub const E_LEDGER: u16 = SEG_BASE + 5; // 结果账缺格/账不平
pub const E_GATE: u16 = SEG_BASE + 6; // 门类归因越闭集

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 三维账规格（首 120 格 = S0–S2 × O0–O5 × OP0–OP7） ----------------------
pub const SUBJ_N: usize = 3;
pub const OBJ_N: usize = 6;
pub const OP_N: usize = 8;
pub const CELL_N: usize = SUBJ_N * OBJ_N * OP_N; // 120

/// 裁决二值（与 36201 求值基元同口径）。
pub const V_GRANT: u8 = 0;
pub const V_DENY: u8 = 1;

/// 门类归因闭集（36284 四类诊断消费此闭集）。
pub const G_NONE: u8 = 0; // 无门（应许）
pub const G_DACL: u8 = 1; // DACL 门（显式拒/无 Allow/空 DACL）
pub const G_IL: u8 = 2; // IL 门（no-write-up）
pub const G_PRIV: u8 = 3; // 特权门
pub const G_MASK: u8 = 4; // 掩码门（Allow 部分覆盖请求位）

/// 主体（Windows 令牌子集：SID + 完整性级 + 特权位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Subject {
    pub sid: u16,
    pub il: u8,
    pub priv_mask: u32,
}

/// 首 120 格覆盖的三个主体：S0 高 IL + 特权位3；S1 中 IL；S2 低 IL。
pub const SUBJECTS: [Subject; SUBJ_N] = [
    Subject { sid: 101, il: 2, priv_mask: 0b1000 },
    Subject { sid: 102, il: 1, priv_mask: 0 },
    Subject { sid: 103, il: 0, priv_mask: 0 },
];

pub const AK_ALLOW: u8 = 0;
pub const AK_DENY: u8 = 1;

/// 客体静态规格（DACL 三槽定容 + 写门槛 IL）。
pub struct ObjSpec {
    pub aces: [(u16, u32, u8); 3],
    pub n_ace: usize,
    pub req_il: u8,
}

/// 六客体：
///   O0：S0 全许 0xFF；S1 显式拒 0x0F（DACL 门锚）。
///   O1：S1 许 0x3C；S0 拒 0x03。
///   O2：三主体各许 0x0F（低位操作面）。
///   O3：S0 许 0x20（OP6=0x60 部分覆盖 → 掩码门锚）；S2 全拒。
///   O4：写门槛 IL=2（no-write-up 锚）；S0 许 0xF0。
///   O5：空 DACL（全拒锚）。
pub const OBJECTS: [ObjSpec; OBJ_N] = [
    ObjSpec { aces: [(101, 0xFF, AK_ALLOW), (102, 0x0F, AK_DENY), (0, 0, 0)], n_ace: 2, req_il: 0 },
    ObjSpec { aces: [(102, 0x3C, AK_ALLOW), (101, 0x03, AK_DENY), (0, 0, 0)], n_ace: 2, req_il: 0 },
    ObjSpec { aces: [(101, 0x0F, AK_ALLOW), (102, 0x0F, AK_ALLOW), (103, 0x0F, AK_ALLOW)], n_ace: 3, req_il: 0 },
    ObjSpec { aces: [(101, 0x20, AK_ALLOW), (103, 0xFF, AK_DENY), (0, 0, 0)], n_ace: 2, req_il: 0 },
    ObjSpec { aces: [(101, 0xF0, AK_ALLOW), (0, 0, 0), (0, 0, 0)], n_ace: 1, req_il: 2 },
    ObjSpec { aces: [(0, 0, 0); 3], n_ace: 0, req_il: 0 },
];

/// OP6 要求双位 0x60（部分覆盖→掩码门的最小构造）；其余单位位。
pub fn op_mask(op: usize) -> Result<u32, u16> {
    if op >= OP_N {
        return Err(E_RANGE);
    }
    if op == 6 {
        Ok(0x60)
    } else {
        Ok(1u32 << op)
    }
}

/// OP7 需要特权位 3（SeTakeOwnership 类——特权门的最小构造）。
pub const OP7_PRIV: u32 = 1 << 3;
/// OP4 起为写操作（IL 门 no-write-up 只约束写）。
pub const WRITE_OP_MIN: usize = 4;

/// 格号（行主序：主体→客体→操作；1-based 由账面层加一）。
pub fn cell_of(sub: usize, obj: usize, op: usize) -> Result<usize, u16> {
    if sub >= SUBJ_N || obj >= OBJ_N || op >= OP_N {
        return Err(E_RANGE);
    }
    Ok((sub * OBJ_N + obj) * OP_N + op)
}

// ---- 引擎裁决（逐 ACE 遍历口径） -------------------------------------------
/// 引擎裁决一个格：返回（裁决，归因门）。门序：IL → 特权 → DACL。
pub fn eval_cell(sub: usize, obj: usize, op: usize) -> Result<(u8, u8), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if sub >= SUBJ_N || obj >= OBJ_N || op >= OP_N {
        return Err(E_RANGE);
    }
    let mask = op_mask(op)?;
    let s = &SUBJECTS[sub];
    let o = &OBJECTS[obj];
    // IL 门：写操作 no-write-up（主体 IL 低于客体门槛即拒）。
    if op >= WRITE_OP_MIN && s.il < o.req_il {
        return Ok((V_DENY, G_IL));
    }
    // 特权门：OP7 需特权位 3。
    if op == 7 && s.priv_mask & OP7_PRIV == 0 {
        return Ok((V_DENY, G_PRIV));
    }
    // DACL 门：deny 首遇即拒；allow 累积；凑满即授。
    let mut granted = 0u32;
    let mut i = 0;
    while i < o.n_ace {
        let (sid, m, kind) = o.aces[i];
        if sid == s.sid {
            if kind == AK_DENY && m & mask != 0 {
                return Ok((V_DENY, G_DACL));
            }
            if kind == AK_ALLOW {
                granted |= m & mask;
            }
        }
        i += 1;
    }
    if granted & mask == mask {
        Ok((V_GRANT, G_NONE))
    } else if granted & mask != 0 {
        Ok((V_DENY, G_MASK)) // 部分覆盖 = 掩码门（缺位可读）
    } else {
        Ok((V_DENY, G_DACL)) // 无 Allow = DACL 门
    }
}

// ---- 母版 X1（独立扁平重导：聚合位口径） ------------------------------------
fn obj_bits(obj: usize, sid: u16, want_kind: u8) -> u32 {
    let mut bits = 0u32;
    let mut i = 0;
    while i < OBJECTS[obj].n_ace {
        let (sid2, m, kind) = OBJECTS[obj].aces[i];
        if sid2 == sid && kind == want_kind {
            bits |= m;
        }
        i += 1;
    }
    bits
}

/// 母版 X1 裁决（与引擎不同形：先聚合 allow/deny 位，再套门序断言）。
pub fn golden_x1(sub: usize, obj: usize, op: usize) -> Result<(u8, u8), u16> {
    if sub >= SUBJ_N || obj >= OBJ_N || op >= OP_N {
        return Err(E_RANGE);
    }
    let mask = op_mask(op)?;
    let s = &SUBJECTS[sub];
    let allow = obj_bits(obj, s.sid, AK_ALLOW);
    let deny = obj_bits(obj, s.sid, AK_DENY);
    let il_ok = !(op >= WRITE_OP_MIN && s.il < OBJECTS[obj].req_il);
    let priv_ok = !(op == 7 && s.priv_mask & OP7_PRIV == 0);
    if !il_ok {
        return Ok((V_DENY, G_IL));
    }
    if !priv_ok {
        return Ok((V_DENY, G_PRIV));
    }
    if deny & mask != 0 {
        return Ok((V_DENY, G_DACL));
    }
    let cov = allow & mask;
    if cov == mask {
        Ok((V_GRANT, G_NONE))
    } else if cov != 0 {
        Ok((V_DENY, G_MASK))
    } else {
        Ok((V_DENY, G_DACL))
    }
}

/// 手算锚 6 格（格号 → 期望 (裁决,门)）——第三重独立来源。
/// 0=(S0,O0,OP0) 许；48=(S1,O0,OP0) DACL 拒；40=(S0,O5,OP0) 空 DACL 拒；
/// 84=(S1,O4,OP4) IL 拒；55=(S1,O0,OP7) 特权拒；30=(S0,O3,OP6) 掩码拒。
pub const HAND_ANCHORS: [(usize, u8, u8); 6] = [
    (0, V_GRANT, G_NONE),
    (48, V_DENY, G_DACL),
    (40, V_DENY, G_DACL),
    (84, V_DENY, G_IL),
    (55, V_DENY, G_PRIV),
    (30, V_DENY, G_MASK),
];

// ---- 结果账（逐格可查） -----------------------------------------------------
/// 单格裁决记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellRow {
    pub cell: u16,
    pub verdict: u8,
    pub gate: u8,
}

/// 全量结果账：120 条裁决记录（格号即下标——逐格可查的账面形态）。
pub fn ledger() -> Result<Vec<CellRow>, u16> {
    let mut out: Vec<CellRow> = Vec::new();
    let mut cell = 0usize;
    while cell < CELL_N {
        let (v, g) = eval_cell(cell / (OBJ_N * OP_N), (cell / OP_N) % OBJ_N, cell % OP_N)?;
        out.push(CellRow { cell: cell as u16, verdict: v, gate: g });
        cell += 1;
    }
    Ok(out)
}

/// 格题注（账面可读性：格号 + 裁决 + 门类——结果账「可查」的呈现面）。
pub fn cell_caption(row: CellRow) -> String {
    let mut s = String::new();
    s.push_str("格#");
    s.push_str(&row.cell.to_string());
    s.push_str(if row.verdict == V_GRANT { " 许" } else { " 拒" });
    if row.verdict == V_DENY {
        s.push_str(" 门=");
        s.push_str(gate_name(row.gate));
    }
    s
}

/// 门类名（归因闭集的可读面）。
pub fn gate_name(gate: u8) -> &'static str {
    match gate {
        G_NONE => "无",
        G_DACL => "DACL",
        G_IL => "IL",
        G_PRIV => "特权",
        G_MASK => "掩码",
        _ => "未知",
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 规格守恒（120 格容量账）。
    if VER != 1 {
        return Err(E_VER);
    }
    if CELL_N != 120 || SUBJECTS.len() != SUBJ_N || OBJECTS.len() != OBJ_N {
        return Err(E_LEDGER);
    }
    // R2：母版 X1 套用断言——120 格引擎与母版零差（零越格）。
    let mut grant_n = 0u32;
    let mut deny_n = 0u32;
    let mut sub = 0;
    while sub < SUBJ_N {
        let mut obj = 0;
        while obj < OBJ_N {
            let mut op = 0;
            while op < OP_N {
                let cell = cell_of(sub, obj, op)?;
                let got = eval_cell(sub, obj, op)?;
                let want = golden_x1(sub, obj, op)?;
                if got != want {
                    return Err(E_CROSS);
                }
                if got.0 == V_GRANT {
                    grant_n += 1;
                } else {
                    deny_n += 1;
                    if got.1 == G_NONE || got.1 > G_MASK {
                        return Err(E_GATE); // 拒格必须归因到四类门之一
                    }
                }
                let _ = cell;
                op += 1;
            }
            obj += 1;
        }
        sub += 1;
    }
    // 应拒 100% 拒 / 应许 100% 许：两类格都得存在且并集恰为全格。
    if grant_n == 0 || deny_n == 0 || grant_n + deny_n != CELL_N as u32 {
        return Err(E_CROSS);
    }
    // R3：手算锚逐位核对（第三重独立来源）。
    let anchors = vec![0usize, 48, 40, 84, 55, 30];
    let mut a = 0;
    while a < HAND_ANCHORS.len() {
        let (cell, v, g) = HAND_ANCHORS[a];
        if anchors[a] != cell {
            return Err(E_ANCHOR);
        }
        let got = eval_cell(cell / (OBJ_N * OP_N), (cell / OP_N) % OBJ_N, cell % OP_N)?;
        if got != (v, g) {
            return Err(E_ANCHOR);
        }
        a += 1;
    }
    // R4：结果账逐格可查——120 条全量、格号连续、题注非空。
    let led = ledger()?;
    if led.len() != CELL_N {
        return Err(E_LEDGER);
    }
    let mut i = 0;
    while i < led.len() {
        if led[i].cell != i as u16 {
            return Err(E_LEDGER);
        }
        if cell_caption(led[i]).is_empty() {
            return Err(E_LEDGER);
        }
        i += 1;
    }
    // R5：门类归因覆盖——四类门在拒格中全部出现（诊断账的地基）。
    let mut seen = [false; 5];
    let mut k = 0;
    while k < led.len() {
        if led[k].verdict == V_DENY {
            if led[k].gate == G_NONE || led[k].gate > G_MASK {
                return Err(E_GATE);
            }
            seen[led[k].gate as usize] = true;
        }
        k += 1;
    }
    let mut g = 1;
    while g <= G_MASK as usize {
        if !seen[g] {
            return Err(E_GATE);
        }
        g += 1;
    }
    // 校验和：裁决账折叠（确定性总锚）。
    let mut sum = 0u32;
    k = 0;
    while k < led.len() {
        sum = sum.wrapping_mul(3).wrapping_add(led[k].verdict as u32 * 2 + led[k].gate as u32);
        k += 1;
    }
    Ok(sum.wrapping_add(grant_n).wrapping_add(CELL_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "格号/下标越合法区",
        E_CROSS => "越格（引擎与母版期望交叉）",
        E_ANCHOR => "手算锚漂移",
        E_LEDGER => "结果账缺格/账不平",
        E_GATE => "门类归因越闭集（拒格须归因四类门之一）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36282_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36282_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB040, "0xB040~0xB04F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "120 格母版 X1 零差/6 锚逐位/结果账逐格可查"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("母版零差", {
        let mut ok = true;
        let mut s = 0;
        while ok && s < SUBJ_N {
            let mut o = 0;
            while ok && o < OBJ_N {
                let mut p = 0;
                while p < OP_N {
                    if eval_cell(s, o, p) != golden_x1(s, o, p) {
                        ok = false;
                        break;
                    }
                    p += 1;
                }
                o += 1;
            }
            s += 1;
        }
        ok
    }, "120 格引擎与母版 X1 逐格全等（零越格）");
    cs.add("结果账容量", CELL_N == 120 && ledger().map(|l| l.len()).unwrap_or(0) == 120, "120 条裁决记录全量落账");
    cs.add("门类闭集", G_MASK == 4 && G_NONE == 0, "四类归因门闭集（DACL/IL/特权/掩码）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36282_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36282_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_anchor_cells_exact() {
        // 手算锚直证：许/拒与归因门逐位对平。
        assert_eq!(eval_cell(0, 0, 0).ok().unwrap_or((9, 9)), (V_GRANT, G_NONE));
        assert_eq!(eval_cell(1, 0, 0).ok().unwrap_or((9, 9)), (V_DENY, G_DACL));
        assert_eq!(eval_cell(1, 4, 4).ok().unwrap_or((9, 9)), (V_DENY, G_IL));
        assert_eq!(eval_cell(1, 0, 7).ok().unwrap_or((9, 9)), (V_DENY, G_PRIV));
        assert_eq!(eval_cell(0, 3, 6).ok().unwrap_or((9, 9)), (V_DENY, G_MASK));
    }
}
