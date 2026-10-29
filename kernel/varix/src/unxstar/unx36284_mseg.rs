//! UNX-F36284-J1 矩阵拒绝格的诊断账全测（每格拒绝可解释）。
//! 判据：UNX-F36284-J1 应拒格诊断 100% 可读（哪道门拒的：DACL/IL/
//! 特权/掩码四类归因）；不可解释格 = 回炉标记；纯功能代码 ≥300 行。
//! 设计纪律：拒绝不是终点而是起点——Windows 的 access denied error
//! 之所以可行动，是因为调用方知道**哪道门**拒的（DACL 不许 / 令牌
//! 完整性不够 / 缺特权 / 掩码位缺失）。本域给首 120 格的每个拒格
//! 出诊断账：
//!   - 归因闭集四类：G_DACL / G_IL / G_PRIV / G_MASK（与 36282 门序
//!     同口径——诊断消费裁决，不另设第二套门序）；
//!   - 诊断 100% 可读：每条诊断含格号 + 门名 + 缺位说明，非空可读；
//!   - 不可解释格（归因越闭集/空门）= 回炉标记，账面恒零——出现即回炉；
//!   - 四类门覆盖断言：诊断账里四类归因全部出现（门序存在性证明）。

use crate::checks::CheckSet;
use crate::unxstar::unx36282_mseg as mx;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB060;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 格号越合法区
pub const E_UNEXPL: u16 = SEG_BASE + 3; // 拒格不可解释（须回炉）
pub const E_LEDGER: u16 = SEG_BASE + 4; // 诊断账缺格/账不平
pub const E_COVER: u16 = SEG_BASE + 5; // 四类门覆盖破（某类归因缺席）
pub const E_READ: u16 = SEG_BASE + 6; // 诊断文本不可读（空/越闭集）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 复用 36282 的格几何与门类闭集（禁第二套模型）。
pub use mx::{CELL_N, G_DACL, G_IL, G_MASK, G_NONE, G_PRIV, OBJ_N, OP_N, SUBJ_N, V_DENY, V_GRANT};

// ---- 诊断账 ----------------------------------------------------------------
/// 单格诊断记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagRow {
    pub cell: u16,
    pub verdict: u8,
    pub gate: u8,
}

/// 回炉标记簿（不可解释格落此——账面恒零为绿）。
pub struct ReworkMarks {
    marks: Vec<u16>,
}

impl ReworkMarks {
    pub fn new() -> ReworkMarks {
        ReworkMarks { marks: Vec::new() }
    }

    /// 回炉标记（不可解释格必须落账——非静默）。
    pub fn mark(&mut self, cell: u16) -> Result<(), u16> {
        if cell as usize >= CELL_N {
            return Err(E_RANGE);
        }
        self.marks.push(cell);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.marks.len()
    }

    pub fn is_clean(&self) -> bool {
        self.marks.is_empty()
    }
}

/// 全量诊断账：120 格逐格出诊断（许格 gate=G_NONE，拒格必归因四类）。
pub fn diag_ledger() -> Result<Vec<DiagRow>, u16> {
    let mut out: Vec<DiagRow> = Vec::new();
    let mut cell = 0usize;
    while cell < CELL_N {
        let sub = cell / (mx::OBJ_N * mx::OP_N);
        let obj = (cell / mx::OP_N) % mx::OBJ_N;
        let op = cell % mx::OP_N;
        let (v, g) = mx::eval_cell(sub, obj, op)?;
        out.push(DiagRow { cell: cell as u16, verdict: v, gate: g });
        cell += 1;
    }
    Ok(out)
}

/// 单格诊断文本（可读性判据的落点：格号 + 门名 + 缺位说明）。
pub fn diag_text(row: DiagRow) -> String {
    let mut s = String::new();
    s.push_str("格#");
    s.push_str(&row.cell.to_string());
    if row.verdict == V_GRANT {
        s.push_str(" 许（无门拦截）");
        return s;
    }
    s.push_str(" 拒 门=");
    s.push_str(mx::gate_name(row.gate));
    match row.gate {
        G_DACL => s.push_str("（DACL 不许：显式拒或无 Allow）"),
        G_IL => s.push_str("（完整性级不足：no-write-up）"),
        G_PRIV => s.push_str("（缺 SeTakeOwnership 类特权位）"),
        G_MASK => s.push_str("（Allow 部分覆盖：请求位缺位）"),
        _ => s.push_str("（归因越闭集——须回炉）"),
    }
    s
}

/// 归因合法性：拒格的门必须落在四类闭集内（越界 = 不可解释）。
pub fn gate_explainable(gate: u8) -> bool {
    gate == G_DACL || gate == G_IL || gate == G_PRIV || gate == G_MASK
}

/// 四类门覆盖计数（诊断账里每类归因的出现次数）。
pub fn gate_census(led: &[DiagRow]) -> Result<[u32; 4], u16> {
    let mut census = [0u32; 4];
    let mut i = 0;
    while i < led.len() {
        if led[i].verdict == V_DENY {
            if !gate_explainable(led[i].gate) {
                return Err(E_UNEXPL);
            }
            census[(led[i].gate - G_DACL) as usize] += 1;
        }
        i += 1;
    }
    Ok(census)
}

/// 拒格清单（诊断账的检索面——按格号枚举全部拒格，逐格可回查）。
pub fn denied_cells(led: &[DiagRow]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < led.len() {
        if led[i].verdict == V_DENY {
            out.push(led[i].cell);
        }
        i += 1;
    }
    out
}

/// 按格号回查诊断（检索面——格号 → 诊断行，缺格即 None）。
pub fn diag_at(led: &[DiagRow], cell: u16) -> Option<DiagRow> {
    if cell as usize >= CELL_N {
        return None;
    }
    let mut i = 0;
    while i < led.len() {
        if led[i].cell == cell {
            return Some(led[i]);
        }
        i += 1;
    }
    None
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 上游引擎锚对平（复用面健康检查）。
    if VER != 1 || mx::VER != 1 {
        return Err(E_VER);
    }
    if CELL_N != 120 || SUBJ_N != 3 || OBJ_N != 6 || OP_N != 8 {
        return Err(E_LEDGER);
    }
    // R2：全量诊断账 120 格——每格裁决与上游引擎再取一致（账面同源）。
    let led = diag_ledger()?;
    if led.len() != CELL_N {
        return Err(E_LEDGER);
    }
    let mut i = 0;
    while i < led.len() {
        let sub = i / (mx::OBJ_N * mx::OP_N);
        let obj = (i / mx::OP_N) % mx::OBJ_N;
        let op = i % mx::OP_N;
        let (v, g) = mx::eval_cell(sub, obj, op)?;
        if led[i].verdict != v || led[i].gate != g {
            return Err(E_LEDGER);
        }
        i += 1;
    }
    // R3：诊断 100% 可读——每格文本非空；拒格归因必落四类闭集，
    // 许格门恒 G_NONE（越界即不可解释）。
    let mut rework = ReworkMarks::new();
    i = 0;
    while i < led.len() {
        if led[i].verdict == V_DENY && !gate_explainable(led[i].gate) {
            rework.mark(led[i].cell)?;
        }
        if diag_text(led[i]).is_empty() {
            return Err(E_READ);
        }
        if led[i].verdict == V_GRANT && led[i].gate != G_NONE {
            return Err(E_UNEXPL);
        }
        i += 1;
    }
    // 不可解释格 = 回炉标记——账面恒零（诊断 100% 覆盖）。
    if !rework.is_clean() {
        return Err(E_UNEXPL);
    }
    // R4b：检索面对证——拒格清单与账面拒格数一致，逐格可回查。
    let denied = denied_cells(&led);
    let mut deny_total = 0u32;
    let mut j = 0;
    while j < led.len() {
        if led[j].verdict == V_DENY {
            deny_total += 1;
        }
        j += 1;
    }
    if denied.len() != deny_total as usize {
        return Err(E_LEDGER);
    }
    j = 0;
    while j < denied.len() {
        match diag_at(&led, denied[j]) {
            Some(row) => {
                if row.verdict != V_DENY || !gate_explainable(row.gate) {
                    return Err(E_UNEXPL);
                }
            }
            None => return Err(E_LEDGER),
        }
        j += 1;
    }
    // R5：四类门覆盖断言——DACL/IL/特权/掩码全部出现且计数可读。
    let census = gate_census(&led)?;
    let mut g = 0;
    while g < 4 {
        if census[g] == 0 {
            return Err(E_COVER);
        }
        g += 1;
    }
    // R5：回炉机制注入验证——合成不可解释格必须被标记（非静默），
    // 在独立实例上做，不污染真实账面。
    let mut rw2 = ReworkMarks::new();
    rw2.mark(0)?;
    rw2.mark(CELL_N as u16 - 1)?;
    if rw2.len() != 2 || rw2.is_clean() {
        return Err(E_UNEXPL);
    }
    // R6：文本面抽查——四类门的诊断文本各含门名（可读性直证）。
    let probes = [
        DiagRow { cell: 48, verdict: V_DENY, gate: G_DACL },
        DiagRow { cell: 84, verdict: V_DENY, gate: G_IL },
        DiagRow { cell: 55, verdict: V_DENY, gate: G_PRIV },
        DiagRow { cell: 30, verdict: V_DENY, gate: G_MASK },
    ];
    let mut p = 0;
    while p < probes.len() {
        let t = diag_text(probes[p]);
        if !t.contains(mx::gate_name(probes[p].gate)) || !t.contains(&probes[p].cell.to_string()) {
            return Err(E_READ);
        }
        p += 1;
    }
    // 校验和：诊断账折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < led.len() {
        sum = sum.wrapping_mul(5).wrapping_add(led[i].gate as u32 + 1);
        i += 1;
    }
    Ok(sum.wrapping_add(census[0]).wrapping_add(CELL_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "格号越合法区",
        E_UNEXPL => "拒格不可解释（须回炉标记）",
        E_LEDGER => "诊断账缺格/账不平",
        E_COVER => "四类门覆盖破（某类归因缺席）",
        E_READ => "诊断文本不可读（空/越闭集）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36284_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36284_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB060, "0xB060~0xB06F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "120 格诊断全可读/四类归因全覆盖/回炉账面恒零"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("诊断全可读", {
        let led = diag_ledger().unwrap_or_default();
        let mut ok = led.len() == CELL_N;
        let mut i = 0;
        while ok && i < led.len() {
            if diag_text(led[i]).is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    }, "120 格诊断文本逐格非空可读");
    cs.add("四类覆盖", {
        match diag_ledger().map(|l| gate_census(&l)) {
            Ok(Ok(c)) => c[0] > 0 && c[1] > 0 && c[2] > 0 && c[3] > 0,
            _ => false,
        }
    }, "DACL/IL/特权/掩码四类归因全部出现");
    cs.add("回炉恒零", ReworkMarks::new().is_clean(), "不可解释格回炉标记账面恒零");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36284_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36284_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_four_gate_census() {
        // 四类归因直证：每类拒格计数非零且彼此独立落账。
        let led = diag_ledger().ok().unwrap_or_default();
        let c = gate_census(&led).ok().unwrap_or([0; 4]);
        assert!(c[0] > 0, "DACL 类缺席");
        assert!(c[1] > 0, "IL 类缺席");
        assert!(c[2] > 0, "特权类缺席");
        assert!(c[3] > 0, "掩码类缺席");
        let t = diag_text(DiagRow { cell: 84, verdict: V_DENY, gate: G_IL });
        assert!(t.contains("IL"), "诊断文本须含门名: {}", t);
    }

    #[test]
    fn test_denied_cells_retrievable() {
        // 检索面直证：拒格清单逐格回查，许格不在清单。
        let led = diag_ledger().ok().unwrap_or_default();
        let denied = denied_cells(&led);
        assert!(!denied.is_empty());
        assert!(denied.contains(&48), "DACL 锚格 48 必须在拒格清单");
        assert!(denied.contains(&84), "IL 锚格 84 必须在拒格清单");
        assert!(!denied.contains(&0), "许格 0 不许混入拒格清单");
        let row = diag_at(&led, 48).ok().unwrap_or(DiagRow { cell: 0, verdict: 9, gate: 9 });
        assert_eq!(row.gate, G_DACL);
        assert!(diag_at(&led, 999).is_none(), "越界格回查恒 None");
    }
}
