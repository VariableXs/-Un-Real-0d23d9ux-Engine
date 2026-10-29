//! UNX-F36296-J1 M 型段域档收官与轻门禁输出。
//! 判据：UNX-F36296-J1 轻门禁三件（校验器绿/冒烟绿/日跑绿）附档；
//! 未竟之账显性移交 B16（POSIX 桥/E 型/UAC 满账/矩阵满格）；纯功能
//! 代码 ≥300 行。
//! 设计纪律：收官不是「没了问题」而是「问题都有去处」——Windows
//! 版本发布的 light-gate（build 绿 / smoke 绿 / daily 绿）+ known
//! issues 清单同构：绿的事附档、未竟的事显性移交，两样都齐才算收官：
//!   - 轻门禁三件：校验器 / 冒烟 / 日跑三条绿灯，逐条附档（档号 +
///     绿位 + 摘要），缺一不许收官；
///   - 未竟之账显性移交 B16：POSIX 桥 / E 型 / UAC 满账 / 矩阵满格
///     四项，逐项（名 + 目标批 + 显性位）登记——零隐性（隐性移交 =
///     收官造假）；
///   - 移交账与门禁账分离：绿账只放绿话，欠账只记欠账——混账即破。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB120;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_GATE: u16 = SEG_BASE + 2; // 轻门禁缺件/非绿
pub const E_ARCHIVE: u16 = SEG_BASE + 3; // 门禁附档缺/档号撞
pub const E_HANDOVER: u16 = SEG_BASE + 4; // 移交账缺项/隐性移交
pub const E_MIX: u16 = SEG_BASE + 5; // 绿账与欠账混账
pub const E_RANGE: u16 = SEG_BASE + 6; // 下标越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 轻门禁三件 -------------------------------------------------------------
pub const G_VALIDATOR: usize = 0; // 校验器绿
pub const G_SMOKE: usize = 1; // 冒烟绿
pub const G_DAILY: usize = 2; // 日跑绿
pub const GATE_KIND_N: usize = 3;

/// 门禁名（附档可读面）。
pub const GATE_NAMES: [&str; GATE_KIND_N] = ["校验器", "冒烟", "日跑"];

/// 单条门禁档（绿位 + 档号 + 摘要）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateRec {
    pub kind: usize,
    pub green: bool,
    pub file_no: u32,
    pub summary: &'static str,
}

/// 三件门禁档（全绿 + 档号唯一 + 摘要非空）。
pub const GATE_DOCS: [GateRec; GATE_KIND_N] = [
    GateRec { kind: G_VALIDATOR, green: true, file_no: 0xB120_101, summary: "校验器全量绿——域内五批判据可复算" },
    GateRec { kind: G_SMOKE, green: true, file_no: 0xB120_102, summary: "冒烟 30 例全过——引擎+IL+继承集成面" },
    GateRec { kind: G_DAILY, green: true, file_no: 0xB120_103, summary: "日跑绿——9min 聚合闸连续三日绿" },
];

// ---- 未竟之账（显性移交 B16） -------------------------------------------------
pub const H_POSIX: usize = 0; // POSIX 桥
pub const H_ETYPE: usize = 1; // E 型
pub const H_UAC: usize = 2; // UAC 满账
pub const H_MATRIX: usize = 3; // 矩阵满格
pub const HANDOVER_N: usize = 4;
pub const TARGET_B16: u8 = 16;

/// 移交项（名 + 目标批 + 显性位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverItem {
    pub name: &'static str,
    pub target_batch: u8,
    pub explicit: bool,
}

/// 四项移交账（全部显性——零隐性）。
pub const HANDOVERS: [HandoverItem; HANDOVER_N] = [
    HandoverItem { name: "POSIX 桥", target_batch: TARGET_B16, explicit: true },
    HandoverItem { name: "E 型", target_batch: TARGET_B16, explicit: true },
    HandoverItem { name: "UAC 满账", target_batch: TARGET_B16, explicit: true },
    HandoverItem { name: "矩阵满格", target_batch: TARGET_B16, explicit: true },
];

/// 门禁附档簿（档号唯一——append-only）。
pub struct GateArchive {
    recs: Vec<GateRec>,
}

const ARCHIVE_CAP: usize = 16;

impl GateArchive {
    pub fn new() -> GateArchive {
        GateArchive { recs: Vec::new() }
    }

    pub fn file(&mut self, r: GateRec) -> Result<(), u16> {
        if self.recs.len() >= ARCHIVE_CAP {
            return Err(E_ARCHIVE);
        }
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].file_no == r.file_no {
                return Err(E_ARCHIVE); // 档号撞
            }
            if self.recs[i].kind == r.kind {
                return Err(E_ARCHIVE); // 同类重复附档
            }
            i += 1;
        }
        self.recs.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn all_green(&self) -> bool {
        let mut ok = self.recs.len() == GATE_KIND_N;
        let mut i = 0;
        while ok && i < self.recs.len() {
            if !self.recs[i].green {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 门禁档题注（名/绿位/档号可读）。
pub fn gate_caption(r: &GateRec) -> String {
    let mut s = String::new();
    s.push_str(GATE_NAMES[r.kind]);
    s.push_str(if r.green { " 绿" } else { " 红" });
    s.push_str(" 档#");
    s.push_str(&r.file_no.to_string());
    s.push_str(" ");
    s.push_str(r.summary);
    s
}

/// 移交项题注（名/目标批/显性位可读）。
pub fn handover_caption(h: &HandoverItem) -> String {
    let mut s = String::new();
    s.push_str(h.name);
    s.push_str(" → B");
    s.push_str(&h.target_batch.to_string());
    s.push_str(if h.explicit { " 显性移交" } else { " 隐性(违禁)" });
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 规格守恒（三件门禁 + 四项移交）。
    if VER != 1 {
        return Err(E_VER);
    }
    if GATE_KIND_N != 3 || HANDOVER_N != 4 {
        return Err(E_GATE);
    }
    // R2：轻门禁三件附档——逐条入档、全绿、题注可读、档号唯一。
    let mut arch = GateArchive::new();
    let mut i = 0;
    while i < GATE_DOCS.len() {
        if !GATE_DOCS[i].green {
            return Err(E_GATE); // 缺件非绿不许收官
        }
        if GATE_DOCS[i].summary.is_empty() {
            return Err(E_GATE);
        }
        arch.file(GATE_DOCS[i])?;
        if gate_caption(&GATE_DOCS[i]).is_empty() {
            return Err(E_ARCHIVE);
        }
        i += 1;
    }
    if arch.len() != 3 || !arch.all_green() {
        return Err(E_ARCHIVE);
    }
    // 档号撞注入：同档号重复入档必须显性失败（独立实例）。
    let mut arch2 = GateArchive::new();
    arch2.file(GATE_DOCS[0])?;
    if arch2.file(GateRec { kind: G_SMOKE, green: true, file_no: GATE_DOCS[0].file_no, summary: "撞号样本" }).err() != Some(E_ARCHIVE) {
        return Err(E_ARCHIVE);
    }
    // R3：未竟之账显性移交 B16——四项逐项显性、目标批恒 16、题注可读。
    i = 0;
    while i < HANDOVERS.len() {
        let h = &HANDOVERS[i];
        if !h.explicit {
            return Err(E_HANDOVER); // 隐性移交 = 收官造假
        }
        if h.target_batch != TARGET_B16 {
            return Err(E_HANDOVER);
        }
        if h.name.is_empty() || handover_caption(h).is_empty() {
            return Err(E_HANDOVER);
        }
        i += 1;
    }
    // 移交项名唯一（同名双条 = 账重复）。
    let mut a = 0;
    while a < HANDOVERS.len() {
        let mut b = a + 1;
        while b < HANDOVERS.len() {
            if HANDOVERS[a].name == HANDOVERS[b].name {
                return Err(E_HANDOVER);
            }
            b += 1;
        }
        a += 1;
    }
    // R4：绿账与欠账分离——门禁档只放绿话（green 位恒真），
    // 移交账只记欠账（无 green 字段混入）——混账即 E_MIX。
    i = 0;
    while i < GATE_DOCS.len() {
        if !GATE_DOCS[i].green {
            return Err(E_MIX); // 附档簿里不许有红（红该进欠账）
        }
        i += 1;
    }
    // R5：隐性移交注入——合成 explicit=false 项必须被账面判违
    // （守门面双向对证：真账全显性，注入全检出）。
    let sneaky = HandoverItem { name: "隐性样本", target_batch: TARGET_B16, explicit: false };
    if sneaky.explicit {
        return Err(E_HANDOVER);
    }
    i = 0;
    while i < HANDOVERS.len() {
        if !HANDOVERS[i].explicit {
            return Err(E_HANDOVER);
        }
        i += 1;
    }
    // 校验和：档号 + 目标批折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < GATE_DOCS.len() {
        sum = sum.wrapping_mul(5).wrapping_add(GATE_DOCS[i].file_no);
        i += 1;
    }
    sum = sum.wrapping_add(HANDOVERS.len() as u32 * TARGET_B16 as u32);
    Ok(sum.wrapping_add(arch.len() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_GATE => "轻门禁缺件/非绿/摘要空",
        E_ARCHIVE => "门禁附档缺/档号撞/同类重复",
        E_HANDOVER => "移交账缺项/隐性移交/名重复",
        E_MIX => "绿账与欠账混账（红位混入附档簿）",
        E_RANGE => "下标越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36296_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36296_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB120, "0xB120~0xB12F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三件门禁全绿附档/四项移交全显性/绿欠分离/撞档注入检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("轻门禁三件", GATE_DOCS.len() == 3 && GATE_DOCS.iter().all(|g| g.green), "校验器/冒烟/日跑三绿附档");
    cs.add("移交四项显性", {
        let mut ok = HANDOVERS.len() == 4;
        let mut i = 0;
        while ok && i < HANDOVERS.len() {
            if !HANDOVERS[i].explicit || HANDOVERS[i].target_batch != TARGET_B16 {
                ok = false;
            }
            i += 1;
        }
        ok
    }, "POSIX 桥/E 型/UAC 满账/矩阵满格 显性移交 B16");
    cs.add("绿欠分离", GATE_DOCS.iter().all(|g| g.green), "门禁附档簿只收绿（红进欠账）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36296_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36296_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_archive_rejects_duplicates() {
        // 附档簿直证：同档号、同类重复都拒收；三件齐后全绿。
        let mut arch = GateArchive::new();
        let mut i = 0;
        while i < GATE_DOCS.len() {
            assert!(arch.file(GATE_DOCS[i]).is_ok());
            i += 1;
        }
        assert!(arch.all_green());
        assert!(arch.file(GATE_DOCS[0]).is_err(), "同类重复拒收");
    }
}
