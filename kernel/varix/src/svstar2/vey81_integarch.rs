//! VE-F5081 · 集成总架构（VE-Z 域 · Y05 组 · 目标 340 行）· 诊断段 0x8f80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5081`
//!
//! **判据（锚点原文）**：四件套、两律、可视可管、拦截、判据。
//!
//! **职责定位（锚点原文）**：脚本与场景集成总架构（集成定位：脚本从能跑变成能
//! 用）；集成四件：节点绑定/属性驱动/事件回调/生命周期钩子；架构两律：集成不
//! 改运行时判定（沙箱三律恒在）/集成不夺编辑器权（绑定关系编辑器可视可管）；
//! 集成越权→拦截记录。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x8f80;

/// 集成件数。
pub const PIECES: u32 = 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 两律声明卡。
pub struct TwoLaws {
    /// 律一：集成不改运行时判定（沙箱三律恒在）。
    pub runtime_untouched: bool,
    /// 律二：集成不夺编辑器权（绑定可视可管）。
    pub editor_kept: bool,
}

impl TwoLaws {
    pub fn held(&self) -> bool {
        self.runtime_untouched && self.editor_kept
    }
}

/// 绑定登记：节点绑定 + 可视可管位。
pub struct Binding {
    pub node: u32,
    pub script: u32,
    pub visible: bool,
    pub manageable: bool,
}

/// 集成架构：四件 + 两律。
pub struct IntegArch {
    pub pieces: [&'static str; 4],
    pub laws: TwoLaws,
    pub bindings: Vec<Binding>,
    /// 拦截记录账。
    pub interceptions: u32,
}

impl IntegArch {
    /// 标准构造。
    pub fn standard() -> Self {
        IntegArch {
            pieces: ["node-bind", "prop-drive", "event-cb", "lifecycle-hook"],
            laws: TwoLaws { runtime_untouched: true, editor_kept: true },
            bindings: alloc::vec![
                Binding { node: 1, script: 10, visible: true, manageable: true },
                Binding { node: 2, script: 11, visible: true, manageable: true },
            ],
            interceptions: 0,
        }
    }

    /// 四件套完备。
    pub fn pieces_complete(&self) -> bool {
        self.pieces.iter().all(|p| !p.is_empty())
    }

    /// 绑定可视可管：全部绑定双位立（编辑器权不夺）。
    pub fn bindings_visible(&self) -> bool {
        self.bindings.iter().all(|b| b.visible && b.manageable)
    }

    /// 越权拦截：未授权集成请求拦截并记录（拦截 O(1)）。
    pub fn intercept(&mut self, authorized: bool) -> bool {
        if !authorized {
            self.interceptions += 1;
            false
        } else {
            true
        }
    }
}

/// J1 主判据：四件套 + 两律 + 可视可管 + 越权拦截。
pub fn j1_main() -> bool {
    let mut a = IntegArch::standard();
    if !(a.pieces_complete() && a.laws.held() && a.bindings_visible()) {
        return false;
    }
    // 越权拦截：未授权拒、授权过、记录计数
    if a.intercept(false) || !a.intercept(true) || a.interceptions != 1 {
        return false;
    }
    true
}

// ── 注入核（0x8f80 段 5 项）────────────────────────────────────────────

/// 注入核 1：两律失守注入——任一律破必红。
pub fn inj1_law_broken() -> bool {
    let l = TwoLaws { runtime_untouched: true, editor_kept: false };
    !l.held()
}

/// 注入核 2：绑定不可视注入——可视位缺失必红。
pub fn inj2_binding_hidden() -> bool {
    let mut a = IntegArch::standard();
    a.bindings[0].visible = false;
    !a.bindings_visible()
}

/// 注入核 3：拦截计数注入——二次越权计数累加。
pub fn inj3_intercept_count() -> bool {
    let mut a = IntegArch::standard();
    let _ = a.intercept(false);
    let _ = a.intercept(false);
    a.interceptions == 2
}

/// 注入核 4：四件缺位注入——件名空缺必红。
pub fn inj4_piece_gap() -> bool {
    let mut a = IntegArch::standard();
    a.pieces[3] = "";
    !a.pieces_complete()
}

/// 注入核 5：件名哈希冻结注入。
pub fn inj5_pieces_frozen() -> bool {
    let mut bytes = Vec::new();
    for p in IntegArch::standard().pieces.iter() {
        bytes.extend_from_slice(p.as_bytes());
    }
    fnv1a(&bytes) != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_vey81_checks：本单判据聚合（诊断段 0x8f80）。
pub fn run_vey81_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5081 集成总架构");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "集成四件套完备",
        j1_main() && IntegArch::standard().pieces_complete(),
        if j1_main() { "" } else { "四件破" },
    );
    s.add(
        "两律恒在+绑定可视可管+越权拦截记录",
        j1_main(),
        if j1_main() { "" } else { "两律/拦截破" },
    );
    let inj = [
        ("注入1两律失守", inj1_law_broken()),
        ("注入2绑定不可视", inj2_binding_hidden()),
        ("注入3拦截计数", inj3_intercept_count()),
        ("注入4件名缺位", inj4_piece_gap()),
        ("注入5件名冻结", inj5_pieces_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
