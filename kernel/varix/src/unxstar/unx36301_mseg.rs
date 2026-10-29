//! UNX-F36301-J1 矩阵第 121–150 格：服务主体面（低 IL 请求者 ×6 客体
//! ×5 操作）。
//! 判据：UNX-F36301-J1 服务主体 30 格应拒 100% 拒/应许 100% 许零越格；
//! 服务 SD 面（联 AI-51 锚）判定对平；纯功能代码 ≥300 行。
//! 设计纪律：服务主体是低完整性级请求者——Windows 服务以低 IL 运行
/// 且服务 SD 携带服务锚（service SID 面，联 AI-51 锚），本域跑通
/// 矩阵第 121–150 格（30 格 = 服务主体 ×6 客体 ×5 操作）：
///   - 门序（与 36282 同口径）：IL 门（no-write-up，写操作 = OP2 起）
///     → 特权门（OP3 需特权位 0）→ DACL 门（deny 首遇即拒 / allow
///     累积凑满即授 / 部分覆盖掩码门）；
///   - 服务 SD 面：每客体 SD 携带 AI-51 服务锚（0x00A15101），锚漂
///     即 E_ANCHOR——服务面判定先验锚后走门序；
///   - 母版 X2：独立聚合位重导逐格对平（与引擎不同形）；
///   - 零越格 + 手算锚 6 格 + 结果账逐格可查（格号 121..150 对位）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB170;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 格号/下标越合法区
pub const E_CROSS: u16 = SEG_BASE + 3; // 越格（引擎与母版期望交叉）
pub const E_ANCHOR: u16 = SEG_BASE + 4; // AI-51 服务锚漂移
pub const E_LEDGER: u16 = SEG_BASE + 5; // 结果账缺格/账不平
pub const E_GATE: u16 = SEG_BASE + 6; // 门类归因越闭集

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 服务主体面规格（30 格 = 1 主体 × 6 客体 × 5 操作） ----------------------
pub const SVC_SID: u16 = 900;
pub const SVC_IL: u8 = 0; // 低 IL 请求者
pub const SVC_PRIV: u32 = 0; // 无特权位

pub const OBJ_N: usize = 6;
pub const OP_N: usize = 5;
pub const CELL_N: usize = OBJ_N * OP_N; // 30
/// 格号区间 121–150（矩阵首版 120 格之后的延伸段）。
pub const CELL_FIRST: u16 = 121;
pub const CELL_LAST: u16 = 150;

/// AI-51 服务锚（服务 SD 面的联签锚——漂移即拒判）。
pub const SVC_AI51_ANCHOR: u32 = 0x00A1_5101;

pub const V_GRANT: u8 = 0;
pub const V_DENY: u8 = 1;

/// 门类闭集（与 36282 同口径）。
pub const G_NONE: u8 = 0;
pub const G_DACL: u8 = 1;
pub const G_IL: u8 = 2;
pub const G_PRIV: u8 = 3;
pub const G_MASK: u8 = 4;

pub const AK_ALLOW: u8 = 0;
pub const AK_DENY: u8 = 1;

/// 写操作自 OP2 起（OP2..OP4 为写；IL 门约束面）。
pub const WRITE_OP_MIN: usize = 2;
/// OP3 需要特权位 0（服务面特权门）。
pub const OP3_PRIV: u32 = 1 << 0;
/// OP4 要求双位 0x18（部分覆盖 → 掩码门的最小构造）。
pub fn op_mask(op: usize) -> Result<u32, u16> {
    if op >= OP_N {
        return Err(E_RANGE);
    }
    if op == 4 {
        Ok(0x18)
    } else {
        Ok(1u32 << op)
    }
}

/// 服务客体 SD 规格（DACL 两槽 + 写门槛 IL + AI-51 锚）。
pub struct SvcObj {
    pub aces: [(u16, u32, u8); 2],
    pub n_ace: usize,
    pub req_il: u8,
    pub ai51_anchor: u32,
}

/// 六服务客体：
///   OS0：许 0x03（OP0/1 应许）；OS1：许 0x01 拒 0x02（DACL 锚）；
///   OS2：写门槛 IL=2、许 0x1F（IL 锚）；OS3：门槛 IL=1、全拒 0xFF；
///   OS4：许 0x08（OP4=0x18 部分覆盖 → 掩码锚）；OS5：空 DACL。
pub const SVC_OBJECTS: [SvcObj; OBJ_N] = [
    SvcObj { aces: [(SVC_SID, 0x03, AK_ALLOW), (0, 0, 0)], n_ace: 1, req_il: 0, ai51_anchor: SVC_AI51_ANCHOR },
    SvcObj { aces: [(SVC_SID, 0x01, AK_ALLOW), (SVC_SID, 0x02, AK_DENY)], n_ace: 2, req_il: 0, ai51_anchor: SVC_AI51_ANCHOR },
    SvcObj { aces: [(SVC_SID, 0x1F, AK_ALLOW), (0, 0, 0)], n_ace: 1, req_il: 2, ai51_anchor: SVC_AI51_ANCHOR },
    SvcObj { aces: [(SVC_SID, 0xFF, AK_DENY), (0, 0, 0)], n_ace: 1, req_il: 1, ai51_anchor: SVC_AI51_ANCHOR },
    SvcObj { aces: [(SVC_SID, 0x08, AK_ALLOW), (0, 0, 0)], n_ace: 1, req_il: 0, ai51_anchor: SVC_AI51_ANCHOR },
    SvcObj { aces: [(0, 0, 0), (0, 0, 0)], n_ace: 0, req_il: 0, ai51_anchor: SVC_AI51_ANCHOR },
];

/// 格号（0-based 内部序 → 账面 121..150）。
pub fn cell_of(obj: usize, op: usize) -> Result<usize, u16> {
    if obj >= OBJ_N || op >= OP_N {
        return Err(E_RANGE);
    }
    Ok(obj * OP_N + op)
}

// ---- 服务引擎裁决（逐 ACE 遍历口径） ----------------------------------------
/// 服务 SD 面先验锚后走门序——返回（裁决，归因门）。
pub fn svc_eval(obj: usize, op: usize) -> Result<(u8, u8), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if obj >= OBJ_N || op >= OP_N {
        return Err(E_RANGE);
    }
    let o = &SVC_OBJECTS[obj];
    // AI-51 锚先验：锚漂即拒判（E_ANCHOR——服务面判定对平的前提）。
    if o.ai51_anchor != SVC_AI51_ANCHOR {
        return Err(E_ANCHOR);
    }
    let mask = op_mask(op)?;
    // IL 门：写操作 no-write-up（低 IL 请求者撞高门槛客体）。
    if op >= WRITE_OP_MIN && SVC_IL < o.req_il {
        return Ok((V_DENY, G_IL));
    }
    // 特权门：OP3 需特权位 0。
    if op == 3 && SVC_PRIV & OP3_PRIV == 0 {
        return Ok((V_DENY, G_PRIV));
    }
    // DACL 门：deny 首遇即拒；allow 累积；凑满即授。
    let mut granted = 0u32;
    let mut i = 0;
    while i < o.n_ace {
        let (sid, m, kind) = o.aces[i];
        if sid == SVC_SID {
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
        Ok((V_DENY, G_MASK))
    } else {
        Ok((V_DENY, G_DACL))
    }
}

// ---- 母版 X2（独立聚合位重导：与服务引擎不同形） -----------------------------
fn svc_bits(obj: usize, want_kind: u8) -> u32 {
    let mut bits = 0u32;
    let mut i = 0;
    while i < SVC_OBJECTS[obj].n_ace {
        let (sid, m, kind) = SVC_OBJECTS[obj].aces[i];
        if sid == SVC_SID && kind == want_kind {
            bits |= m;
        }
        i += 1;
    }
    bits
}

/// 母版 X2 裁决（聚合位 + 门序断言——与服务引擎逐 ACE 遍历对平）。
pub fn golden_x2(obj: usize, op: usize) -> Result<(u8, u8), u16> {
    if obj >= OBJ_N || op >= OP_N {
        return Err(E_RANGE);
    }
    let mask = op_mask(op)?;
    let allow = svc_bits(obj, AK_ALLOW);
    let deny = svc_bits(obj, AK_DENY);
    let il_ok = !(op >= WRITE_OP_MIN && SVC_IL < SVC_OBJECTS[obj].req_il);
    let priv_ok = !(op == 3 && SVC_PRIV & OP3_PRIV == 0);
    if SVC_OBJECTS[obj].ai51_anchor != SVC_AI51_ANCHOR {
        return Err(E_ANCHOR);
    }
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

/// 手算锚 6 格（账面格号 → 期望 (裁决,门)）——第三重独立来源。
/// 121=(OS0,OP0) 许；133=(OS2,OP2) IL 拒；129=(OS1,OP3) 特权拒；
/// 145=(OS4,OP4) 掩码拒；136=(OS3,OP0) DACL 拒；146=(OS5,OP0) 空 DACL 拒。
pub const HAND_ANCHORS: [(u16, u8, u8); 6] = [
    (121, V_GRANT, G_NONE),
    (133, V_DENY, G_IL),
    (129, V_DENY, G_PRIV),
    (145, V_DENY, G_MASK),
    (136, V_DENY, G_DACL),
    (146, V_DENY, G_DACL),
];

// ---- 结果账（逐格可查，格号 121..150 对位） ---------------------------------
/// 单格裁决记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SvcRow {
    pub cell: u16,
    pub verdict: u8,
    pub gate: u8,
}

/// 全量结果账：30 条（格号 121..150 连续对位）。
pub fn svc_ledger() -> Result<Vec<SvcRow>, u16> {
    let mut out: Vec<SvcRow> = Vec::new();
    let mut idx = 0usize;
    while idx < CELL_N {
        let (v, g) = svc_eval(idx / OP_N, idx % OP_N)?;
        let cell = CELL_FIRST + idx as u16;
        if cell > CELL_LAST {
            return Err(E_RANGE);
        }
        out.push(SvcRow { cell, verdict: v, gate: g });
        idx += 1;
    }
    Ok(out)
}

/// 格题注（账面可读：格号 + 裁决 + 门类）。
pub fn row_caption(r: &SvcRow) -> String {
    let mut s = String::new();
    s.push_str("格#");
    s.push_str(&r.cell.to_string());
    s.push_str(if r.verdict == V_GRANT { " 许" } else { " 拒" });
    if r.verdict == V_DENY {
        s.push_str(" 门=");
        s.push_str(gate_name(r.gate));
    }
    s
}

/// 门类名。
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
    // R1：版本锚 + 对位账（30 格恰落 121..150）。
    if VER != 1 {
        return Err(E_VER);
    }
    if CELL_N != 30 || CELL_FIRST != 121 || CELL_LAST != 150 {
        return Err(E_LEDGER);
    }
    if CELL_LAST - CELL_FIRST + 1 != CELL_N as u16 {
        return Err(E_LEDGER);
    }
    // R2：母版 X2 套用断言——30 格引擎与母版零差（零越格）。
    let mut grant_n = 0u32;
    let mut deny_n = 0u32;
    let mut obj = 0;
    while obj < OBJ_N {
        let mut op = 0;
        while op < OP_N {
            let got = svc_eval(obj, op)?;
            let want = golden_x2(obj, op)?;
            if got != want {
                return Err(E_CROSS);
            }
            if got.0 == V_GRANT {
                grant_n += 1;
            } else {
                deny_n += 1;
                if got.1 == G_NONE || got.1 > G_MASK {
                    return Err(E_GATE);
                }
            }
            op += 1;
        }
        obj += 1;
    }
    // 应拒 100% 拒 / 应许 100% 许：两类格齐备且并集恰为全格。
    if grant_n == 0 || deny_n == 0 || grant_n + deny_n != CELL_N as u32 {
        return Err(E_CROSS);
    }
    // R3：手算锚逐位核对（第三重独立来源）。
    let anchors = vec![121u16, 133, 129, 145, 136, 146];
    let mut a = 0;
    while a < HAND_ANCHORS.len() {
        let (cell, v, g) = HAND_ANCHORS[a];
        if anchors[a] != cell {
            return Err(E_ANCHOR);
        }
        let idx = (cell - CELL_FIRST) as usize;
        let got = svc_eval(idx / OP_N, idx % OP_N)?;
        if got != (v, g) {
            return Err(E_ANCHOR);
        }
        a += 1;
    }
    // R4：结果账逐格可查——30 条全量、格号 121..150 连续、题注非空。
    let led = svc_ledger()?;
    if led.len() != CELL_N {
        return Err(E_LEDGER);
    }
    let mut i = 0;
    while i < led.len() {
        if led[i].cell != CELL_FIRST + i as u16 {
            return Err(E_LEDGER);
        }
        if row_caption(&led[i]).is_empty() {
            return Err(E_LEDGER);
        }
        i += 1;
    }
    // R5：服务 SD 锚全验——六客体锚恒 0x00A15101（漂移即 E_ANCHOR）。
    obj = 0;
    while obj < OBJ_N {
        if SVC_OBJECTS[obj].ai51_anchor != SVC_AI51_ANCHOR {
            return Err(E_ANCHOR);
        }
        obj += 1;
    }
    // R6：门类覆盖——IL/特权/DACL/掩码拒格全出现（服务面四门齐）。
    let mut seen = [false; 5];
    i = 0;
    while i < led.len() {
        if led[i].verdict == V_DENY {
            if led[i].gate == G_NONE || led[i].gate > G_MASK {
                return Err(E_GATE);
            }
            seen[led[i].gate as usize] = true;
        }
        i += 1;
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
    i = 0;
    while i < led.len() {
        sum = sum.wrapping_mul(5).wrapping_add(led[i].verdict as u32 * 2 + led[i].gate as u32);
        i += 1;
    }
    Ok(sum.wrapping_add(grant_n).wrapping_add(CELL_FIRST as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "格号/下标越合法区",
        E_CROSS => "越格（引擎与母版 X2 期望交叉）",
        E_ANCHOR => "AI-51 服务锚漂移",
        E_LEDGER => "结果账缺格/账不平（121..150 对位破）",
        E_GATE => "门类归因越闭集",
        _ => "未知诊断码",
    }
}

pub fn run_unx36301_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36301_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB170, "0xB170~0xB17F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "30 格母版 X2 零差/6 锚逐位/AI-51 锚全验/121..150 对位"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("母版零差", {
        let mut ok = true;
        let mut o = 0;
        while ok && o < OBJ_N {
            let mut p = 0;
            while p < OP_N {
                if svc_eval(o, p) != golden_x2(o, p) {
                    ok = false;
                    break;
                }
                p += 1;
            }
            o += 1;
        }
        ok
    }, "30 格引擎与母版 X2 逐格全等（零越格）");
    cs.add("格号对位", CELL_FIRST == 121 && CELL_LAST == 150 && CELL_N == 30, "矩阵第 121–150 格服务主体面恰 30 格");
    cs.add("AI-51 锚全验", {
        let mut ok = true;
        let mut o = 0;
        while ok && o < OBJ_N {
            if SVC_OBJECTS[o].ai51_anchor != SVC_AI51_ANCHOR {
                ok = false;
            }
            o += 1;
        }
        ok
    }, "六客体服务 SD 锚恒 0x00A15101");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36301_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36301_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_anchor_cells_exact() {
        // 手算锚直证：许/拒与四门归因逐位对平（低 IL 服务面）。
        assert_eq!(svc_eval(0, 0).ok().unwrap_or((9, 9)), (V_GRANT, G_NONE));
        assert_eq!(svc_eval(2, 2).ok().unwrap_or((9, 9)), (V_DENY, G_IL));
        assert_eq!(svc_eval(1, 3).ok().unwrap_or((9, 9)), (V_DENY, G_PRIV));
        assert_eq!(svc_eval(4, 4).ok().unwrap_or((9, 9)), (V_DENY, G_MASK));
        assert_eq!(svc_eval(3, 0).ok().unwrap_or((9, 9)), (V_DENY, G_DACL));
        // 结果账格号 121..150 连续对位。
        let led = svc_ledger().ok().unwrap_or_default();
        assert_eq!(led.len(), 30);
        assert_eq!(led[0].cell, 121);
        assert_eq!(led[29].cell, 150);
    }
}
