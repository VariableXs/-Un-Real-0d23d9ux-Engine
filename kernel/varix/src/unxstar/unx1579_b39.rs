//! UNX-F1579 B39 批内互联对账（十八面式批内互查） · 0x78FC0段
//!
//! 判据原文（UNX-F1579-J1 摘录）：「B39 批内 19 条互联对账一致率 100%
//! （总表/联签件×3/终验/包/总账/保留/演练/反馈/三向/安检/时点/口径/导览/
//! 健康/文档 十八面互查），互联矩阵可查可导出」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 十八面闭集：总表/联签件甲乙丙/终验/包/总账/保留/演练/反馈/三向/安检/
//!    时点/口径/导览/健康/域聚合 十八面逐面入网，缺面即拒；
//! 2. 19 条互联边冻结：面链 17 条 + 交叉 2 条（总表↔三向、包↔安检），
//!    边表逐条审计，边两端必须都在网且批内标签逐字一致；
//! 3. 一致率 100%：19 条边逐条对账，任一条失配即整批不一致，失配边点名；
//! 4. 互联矩阵可查可导出：边级查询（面×面→对账判定）+ 矩阵全文导出
//!    （19 行逐边行，含判定与标签），导出与内态重放逐字一致；
//! 5. 状态机 组网中→已对账→矩阵已封 单向，倒退显性拒；诊断码闭集
//!    0x78FC1..0x78FCC。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78FC0;

/// 批内面数（十八面）。
pub const FACE_COUNT: usize = 18;

/// 批内互联边数（19 条，判据钦定）。
pub const EDGE_COUNT: usize = 19;

/// 批内标签（B39 钦定批号）。
pub const BATCH_TAG: u64 = 0xB39;

/// B39 批内互联对账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 面名非法（空名或册外名）。
    ENode,
    /// 面重复入网。
    EDupNode,
    /// 组网缺面。
    EMissing,
    /// 批内标签不符。
    ETag,
    /// 互联边非法（端点越界或自环）。
    EEdge,
    /// 边数不符（非 19 条）。
    EEdgeCount,
    /// 对账未发起即导出。
    ENoAudit,
    /// 失配未清禁止封矩阵。
    ERedBlock,
    /// 矩阵已封。
    ESealed,
    /// 重复发起对账。
    EDupAudit,
    /// 空网样本。
    EEmpty,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENode => 0x01,
                Diag::EDupNode => 0x02,
                Diag::EMissing => 0x03,
                Diag::ETag => 0x04,
                Diag::EEdge => 0x05,
                Diag::EEdgeCount => 0x06,
                Diag::ENoAudit => 0x07,
                Diag::ERedBlock => 0x08,
                Diag::ESealed => 0x09,
                Diag::EDupAudit => 0x0a,
                Diag::EEmpty => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENode => "面名非法：按十八面名册逐面入网，空名册外名拒收",
            Diag::EDupNode => "面重复入网：每面只入一次，删重复项",
            Diag::EMissing => "组网缺面：十八面未齐先补面，再发起批内对账",
            Diag::ETag => "批内标签不符：面标签必须等于 B39 批号，逐面校正",
            Diag::EEdge => "互联边非法：端点必须在十八面内且不得自环",
            Diag::EEdgeCount => "边数不符：互联边恒为 19 条，先核边表",
            Diag::ENoAudit => "对账未发起：先批内对账，再谈矩阵导出",
            Diag::ERedBlock => "失配未清：存在失配边禁止封矩阵，先逐边校正",
            Diag::ESealed => "矩阵已封：封后写入拒绝，改动走下一批互联",
            Diag::EDupAudit => "对账已发起：重复发起拒绝，先取回上一轮矩阵",
            Diag::EEmpty => "空网样本：无面不构成互联网，先逐面入网",
            Diag::EPhase => "相位倒退：组网中→已对账→矩阵已封 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENode,
    Diag::EDupNode,
    Diag::EMissing,
    Diag::ETag,
    Diag::EEdge,
    Diag::EEdgeCount,
    Diag::ENoAudit,
    Diag::ERedBlock,
    Diag::ESealed,
    Diag::EDupAudit,
    Diag::EEmpty,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78FC1..0x78FCC 闭集内"
}

/// 十八面名册（判据面钦定顺序）。
pub const FACE_NAMES: [&str; FACE_COUNT] = [
    "总表", "联签件甲", "联签件乙", "联签件丙", "终验", "包",
    "总账", "保留", "演练", "反馈", "三向", "安检",
    "时点", "口径", "导览", "健康", "文档", "域聚合",
];

/// 19 条互联边表（面下标对；链 17 条 + 交叉 2 条，冻结）。
pub const EDGE_TABLE: [(usize, usize); EDGE_COUNT] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 8),
    (8, 9),
    (9, 10),
    (10, 11),
    (11, 12),
    (12, 13),
    (13, 14),
    (14, 15),
    (15, 16),
    (16, 17),
    (0, 10),
    (5, 11),
];

/// 单条互联边判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeVerdict {
    /// 边端点 a（面下标）。
    pub a: usize,
    /// 边端点 b（面下标）。
    pub b: usize,
    /// 一致（两端在网且批内标签一致）。
    pub consistent: bool,
    /// 失配原因（一致则空）。
    pub reason: &'static str,
}

/// 网相位（组网中→已对账→矩阵已封 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 组网中：逐面入网中。
    Wiring,
    /// 已对账：19 条边逐条对账完成。
    Audited,
    /// 矩阵已封：矩阵冻结、写入拒绝。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Wiring => 0,
            Phase::Audited => 1,
            Phase::Sealed => 2,
        }
    }
}

/// FNV-1a 折叠（整数哈希）。
fn fold_word(mut h: u64, w: u64) -> u64 {
    let mut k = w;
    let mut i = 0;
    while i < 8 {
        let b = (k & 0xFF) as u8;
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001B3);
        k >>= 8;
        i += 1;
    }
    h
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001B3);
    }
    acc
}

/// 互联矩阵：十八面入网 + 19 边对账 + 导出封存。
#[derive(Clone, Debug)]
pub struct InterlinkMatrix {
    phase: Phase,
    /// 逐面标签（None=未入网）。
    nodes: Vec<Option<u64>>,
    /// 逐边判定（对账后落定）。
    verdicts: Vec<EdgeVerdict>,
    /// 矩阵折叠根。
    root: u64,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl InterlinkMatrix {
    /// 新矩阵（空网）。
    pub fn new() -> InterlinkMatrix {
        InterlinkMatrix {
            phase: Phase::Wiring,
            nodes: (0..FACE_COUNT).map(|_| None).collect(),
            verdicts: Vec::new(),
            root: 0,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已入网面数。
    pub fn wired(&self) -> usize {
        self.nodes.iter().filter(|x| x.is_some()).count()
    }

    /// 入一面（面下标 0..18，标签必须等于 B39 批号）。
    pub fn wire(&mut self, face: usize, tag: u64) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if face >= FACE_COUNT {
            self.rejects += 1;
            return Err(Diag::ENode.val());
        }
        let dup = match self.nodes.get(face) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            self.rejects += 1;
            return Err(Diag::EDupNode.val());
        }
        if tag != BATCH_TAG {
            self.rejects += 1;
            return Err(Diag::ETag.val());
        }
        match self.nodes.get_mut(face) {
            Some(slot) => {
                *slot = Some(tag);
                Ok(())
            }
            None => Err(Diag::ENode.val()),
        }
    }

    /// 篡改一面标签（注入用；封后拒；面未入网拒）。
    pub fn corrode(&mut self, face: usize, tag: u64) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if face >= FACE_COUNT {
            return Err(Diag::ENode.val());
        }
        match self.nodes.get_mut(face) {
            Some(Some(slot)) => {
                *slot = tag;
                Ok(())
            }
            Some(None) => Err(Diag::EMissing.val()),
            None => Err(Diag::ENode.val()),
        }
    }

    /// 面标签。
    pub fn tag_at(&self, face: usize) -> Option<u64> {
        match self.nodes.get(face) {
            Some(Some(x)) => Some(*x),
            _ => None,
        }
    }

    /// 批内对账：十八面齐 + 19 边逐条判定。
    pub fn audit(&mut self) -> Result<Vec<EdgeVerdict>, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase == Phase::Audited {
            return Err(Diag::EDupAudit.val());
        }
        if self.wired() == 0 {
            return Err(Diag::EEmpty.val());
        }
        if self.wired() < FACE_COUNT {
            return Err(Diag::EMissing.val());
        }
        if EDGE_TABLE.len() != EDGE_COUNT {
            return Err(Diag::EEdgeCount.val());
        }
        let mut verdicts = Vec::new();
        let mut h: u64 = 0xCBF29CE484222325;
        for (a, b) in EDGE_TABLE {
            let v = if a >= FACE_COUNT || b >= FACE_COUNT || a == b {
                EdgeVerdict { a, b, consistent: false, reason: "边非法" }
            } else {
                let ta = self.tag_at(a);
                let tb = self.tag_at(b);
                match (ta, tb) {
                    (Some(x), Some(y)) if x == y && x == BATCH_TAG => {
                        EdgeVerdict { a, b, consistent: true, reason: "" }
                    }
                    (Some(_), Some(_)) => EdgeVerdict { a, b, consistent: false, reason: "标签不符" },
                    _ => EdgeVerdict { a, b, consistent: false, reason: "端点缺" },
                }
            };
            h = fold_word(h, v.a as u64);
            h = fold_word(h, v.b as u64);
            h = fold_word(h, if v.consistent { 1 } else { 0 });
            verdicts.push(v);
        }
        self.verdicts = verdicts.clone();
        self.root = h;
        self.phase = Phase::Audited;
        Ok(verdicts)
    }

    /// 已落定判定。
    pub fn verdicts(&self) -> &[EdgeVerdict] {
        &self.verdicts
    }

    /// 失配边数。
    pub fn mismatch_count(&self) -> usize {
        self.verdicts.iter().filter(|v| !v.consistent).count()
    }

    /// 边级查询（面×面→判定；可查面）。
    pub fn edge_query(&self, a: usize, b: usize) -> Option<EdgeVerdict> {
        if self.verdicts.is_empty() {
            return None;
        }
        self.verdicts.iter().find(|v| (v.a == a && v.b == b) || (v.a == b && v.b == a)).map(|v| *v)
    }

    /// 矩阵折叠根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 矩阵全文导出（19 行逐边行；可导出面）。
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.verdicts.len() {
            if let Some(v) = self.verdicts.get(i) {
                let na = FACE_NAMES.get(v.a).unwrap_or(&"?");
                let nb = FACE_NAMES.get(v.b).unwrap_or(&"?");
                let line = if v.consistent {
                    format!("边{} {}↔{} 一致", i + 1, na, nb)
                } else {
                    format!("边{} {}↔{} 失配：{}", i + 1, na, nb, v.reason)
                };
                out.push_str(&line);
                out.push('\n');
            }
            i += 1;
        }
        out
    }

    /// 封矩阵：失配必须已清零。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase != Phase::Audited {
            return Err(Diag::ENoAudit.val());
        }
        if self.mismatch_count() > 0 {
            return Err(Diag::ERedBlock.val());
        }
        if self.root == 0 {
            return Err(Diag::EEmpty.val());
        }
        self.phase = Phase::Sealed;
        Ok(self.root)
    }

    /// 封后重放：19 边复判 + 根复算逐字一致。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::EPhase.val());
        }
        let mut h: u64 = 0xCBF29CE484222325;
        let mut ok = 0u32;
        for (a, b) in EDGE_TABLE {
            let v = match (self.tag_at(a), self.tag_at(b)) {
                (Some(x), Some(y)) if x == y && x == BATCH_TAG => true,
                _ => false,
            };
            h = fold_word(h, a as u64);
            h = fold_word(h, b as u64);
            h = fold_word(h, if v { 1 } else { 0 });
            if v {
                ok += 1;
            }
        }
        if h != self.root {
            return Err(Diag::EPhase.val());
        }
        if ok != EDGE_COUNT as u32 {
            return Err(Diag::EPhase.val());
        }
        Ok(ok)
    }
}

/// 标准网：十八面全部以 B39 批号入网（一致率 100% 基线批）。
pub fn build_matrix() -> InterlinkMatrix {
    let mut m = InterlinkMatrix::new();
    let mut f = 0usize;
    while f < FACE_COUNT {
        let _ = m.wire(f, BATCH_TAG);
        f += 1;
    }
    m
}

/// 失配注入 10/10：十路逐面标签篡改，凡触边必须失配、封矩阵必须被拒。
/// 返回十布尔（true=该路篡改被点出）。篡改面取下标 0..10（链首段与交叉段）。
pub fn corrode_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let mut i = 0usize;
    while i < 10 {
        let mut m = build_matrix();
        let face = i;
        // 篡改该面标签为批外值。
        let corroded = m.corrode(face, 0xDEAD) == Ok(());
        let rep = m.audit();
        // 该面涉及的边数（链邻边 1~2 条 + 交叉边 0~1 条）。
        let mut touched = 0usize;
        let mut k = 0usize;
        while k < EDGE_COUNT {
            if let Some((a, b)) = EDGE_TABLE.get(k) {
                if *a == face || *b == face {
                    touched += 1;
                }
            }
            k += 1;
        }
        let audit_ok = match &rep {
            Ok(vs) => vs.len() == EDGE_COUNT && m.mismatch_count() == touched && touched >= 1,
            Err(_) => false,
        };
        let seal_blocked = m.seal() == Err(Diag::ERedBlock.val());
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        *cell = corroded && audit_ok && seal_blocked;
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78FC0
}

/// 边表冻结守卫（19 条、端点在十八面内、无自环、无重边）。
fn edge_table_ok() -> bool {
    if EDGE_TABLE.len() != EDGE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < EDGE_TABLE.len() {
        let (a, b) = match EDGE_TABLE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a >= FACE_COUNT || b >= FACE_COUNT || a == b {
            return false;
        }
        let mut j = i + 1;
        while j < EDGE_TABLE.len() {
            let (c, d) = match EDGE_TABLE.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if (a == c && b == d) || (a == d && b == c) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 十八面名册闭合（互异非空）。
fn face_names_ok() -> bool {
    if FACE_NAMES.len() != FACE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < FACE_NAMES.len() {
        let a = match FACE_NAMES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < FACE_NAMES.len() {
            let b = match FACE_NAMES.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 基线网 19 边一致率 100%。
fn baseline_ok(m: &InterlinkMatrix) -> bool {
    m.wired() == FACE_COUNT && m.mismatch_count() == 0 && m.verdicts().len() == EDGE_COUNT
}

/// 失配注入 10/10。
fn corrode_ok() -> bool {
    corrode_matrix() == [true; 10]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let mut a = build_matrix();
    let mut b = build_matrix();
    let ra = a.audit();
    let rb = b.audit();
    match (ra, rb) {
        (Ok(x), Ok(y)) => x == y && a.root() == b.root() && a.root() != 0,
        _ => false,
    }
}

/// 十三项判据：UNX-F1579 B39 批内互联对账。
pub fn run_unx1579_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1579");
    let mut m = build_matrix();

    s.add_dyn(
        "F1579-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1579-02 十八面名册闭合",
        face_names_ok(),
        &format!("faces={} first={} last={}", FACE_NAMES.len(), FACE_NAMES.get(0).unwrap_or(&"?"), FACE_NAMES.get(FACE_COUNT - 1).unwrap_or(&"?")),
    );

    s.add_dyn(
        "F1579-03 互联边表冻结守卫",
        edge_table_ok(),
        &format!("edges={}", EDGE_TABLE.len()),
    );

    // 结构拒绝：越界面、批外标签、重入网、空网对账、缺面对账。
    let mut r = InterlinkMatrix::new();
    let over = r.wire(FACE_COUNT, BATCH_TAG) == Err(Diag::ENode.val());
    let bad_tag = r.wire(0, 0xDEAD) == Err(Diag::ETag.val());
    let _ = r.wire(0, BATCH_TAG);
    let dup = r.wire(0, BATCH_TAG) == Err(Diag::EDupNode.val());
    let empty_audit = InterlinkMatrix::new().audit() == Err(Diag::EEmpty.val());
    let missing = r.audit() == Err(Diag::EMissing.val());
    s.add_dyn(
        "F1579-04 结构拒绝五门",
        over && bad_tag && dup && empty_audit && missing,
        &format!("rejects={}", r.rejects()),
    );

    // 基线对账：19 边全一致、相位推进、重复对账拒绝。
    let rep = m.audit();
    let rep_ok = match &rep {
        Ok(vs) => vs.len() == EDGE_COUNT && m.mismatch_count() == 0,
        Err(_) => false,
    };
    let dup = m.audit() == Err(Diag::EDupAudit.val());
    s.add_dyn(
        "F1579-05 十九边一致率百分百",
        rep_ok && dup && m.phase() == Phase::Audited,
        &format!("phase_ord={} mismatches={}", m.phase().ord(), m.mismatch_count()),
    );

    // 失配注入 10/10。
    s.add_dyn(
        "F1579-06 标签篡改十路全点出",
        corrode_ok(),
        &format!("matrix={:?}", corrode_matrix()),
    );

    // 失配边点名（篡改「包」面：链邻边 2 条 + 交叉边 1 条 = 3 条失配）。
    let mut q = build_matrix();
    let _ = q.corrode(5, 0xBAD);
    let _ = q.audit();
    let q_ok = q.mismatch_count() == 3
        && q.edge_query(4, 5).map(|v| !v.consistent && v.reason.contains("标签")).unwrap_or(false)
        && q.edge_query(5, 11).map(|v| !v.consistent).unwrap_or(false);
    s.add_dyn(
        "F1579-07 失配边点名可查",
        q_ok,
        &format!("mismatches={}", q.mismatch_count()),
    );

    // 边级查询（可查面）：一致网查任意表内边均可查且一致。
    let query_ok = m.edge_query(0, 10).map(|v| v.consistent).unwrap_or(false)
        && m.edge_query(5, 11).map(|v| v.consistent).unwrap_or(false)
        && m.edge_query(0, 5).is_none();
    s.add_dyn(
        "F1579-08 互联矩阵可查",
        query_ok,
        &format!("q010={:?}", m.edge_query(0, 10).map(|v| v.consistent)),
    );

    // 导出面：19 行逐边行齐（可导出面）。
    let text = m.export_text();
    let export_ok = text.matches('\n').count() == EDGE_COUNT
        && text.contains("边1 总表↔联签件甲 一致")
        && text.contains("边18 总表↔三向 一致")
        && text.contains("边19 包↔安检 一致");
    s.add_dyn(
        "F1579-09 互联矩阵可导出",
        export_ok,
        &format!("bytes={}", text.len()),
    );

    // 封矩阵闸门：失配未清拒绝；清零放行；封后写入拒绝。
    let q_seal = q.seal() == Err(Diag::ERedBlock.val());
    let seal = m.seal();
    let seal_ok = match seal {
        Ok(root) => root != 0,
        Err(_) => false,
    };
    let write_after = m.wire(0, BATCH_TAG) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1579-10 封矩阵闸门三向",
        q_seal && seal_ok && write_after && m.phase() == Phase::Sealed,
        &format!("q_seal={} seal_ok={}", q_seal, seal_ok),
    );

    // 封后重放：19 边复判逐字一致。
    s.add_dyn(
        "F1579-11 封后重放十九边一致",
        m.replay() == Ok(EDGE_COUNT as u32),
        &format!("replay={:?}", m.replay()),
    );

    // 相位倒退显性拒。
    let back = m.audit() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1579-12 相位倒退显性拒",
        back && m.phase().ord() == 2,
        &format!("phase_ord={}", m.phase().ord()),
    );

    s.add_dyn(
        "F1579-13 双跑一致与批号守卫",
        dual_run_ok() && BATCH_TAG == 0xB39 && ALL.len() == 12,
        &format!("root={:016x} tag=0x{:x}", build_matrix().root(), BATCH_TAG),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_names_closed() {
        assert_eq!(FACE_NAMES.len(), FACE_COUNT);
        for name in FACE_NAMES {
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn edge_table_frozen() {
        assert_eq!(EDGE_TABLE.len(), EDGE_COUNT);
        assert!(edge_table_ok());
        // 链 17 条 + 交叉 2 条。
        let mut cross = 0usize;
        for (a, b) in EDGE_TABLE {
            if b.wrapping_sub(a) != 1 {
                cross += 1;
            }
        }
        assert_eq!(cross, 2);
    }

    #[test]
    fn wire_gates() {
        let mut m = InterlinkMatrix::new();
        assert_eq!(m.wire(FACE_COUNT, BATCH_TAG), Err(Diag::ENode.val()));
        assert_eq!(m.wire(0, 0xDEAD), Err(Diag::ETag.val()));
        assert_eq!(m.wire(0, BATCH_TAG), Ok(()));
        assert_eq!(m.wire(0, BATCH_TAG), Err(Diag::EDupNode.val()));
        assert_eq!(m.rejects(), 3);
    }

    #[test]
    fn audit_needs_full_net() {
        let mut m = InterlinkMatrix::new();
        assert_eq!(m.audit(), Err(Diag::EEmpty.val()));
        let _ = m.wire(0, BATCH_TAG);
        assert_eq!(m.audit(), Err(Diag::EMissing.val()));
    }

    #[test]
    fn baseline_audit_all_consistent() {
        let mut m = build_matrix();
        assert_eq!(m.wired(), FACE_COUNT);
        let vs = m.audit().expect("基线对账");
        assert_eq!(vs.len(), EDGE_COUNT);
        assert_eq!(m.mismatch_count(), 0);
        assert_eq!(m.audit(), Err(Diag::EDupAudit.val()));
    }

    #[test]
    fn corrode_touches_expected_edges() {
        let mut q = build_matrix();
        let _ = q.corrode(5, 0xBAD);
        let _ = q.audit();
        // 面 5（包）涉及边 (4,5)(5,6)(5,11) 共 3 条。
        assert_eq!(q.mismatch_count(), 3);
        assert!(q.edge_query(4, 5).map(|v| !v.consistent).unwrap_or(false));
        assert!(q.edge_query(5, 6).map(|v| !v.consistent).unwrap_or(false));
        assert!(q.edge_query(5, 11).map(|v| !v.consistent).unwrap_or(false));
        assert!(q.edge_query(0, 10).map(|v| v.consistent).unwrap_or(false));
        assert_eq!(q.seal(), Err(Diag::ERedBlock.val()));
    }

    #[test]
    fn corrode_ten_of_ten() {
        let m = corrode_matrix();
        assert_eq!(m, [true; 10]);
        assert!(corrode_ok());
    }

    #[test]
    fn export_and_seal_and_replay() {
        let mut m = build_matrix();
        let _ = m.audit();
        let text = m.export_text();
        assert_eq!(text.matches('\n').count(), EDGE_COUNT);
        assert!(text.contains("一致"));
        let root = m.seal().expect("零失配封矩阵");
        assert_ne!(root, 0);
        assert_eq!(m.replay(), Ok(EDGE_COUNT as u32));
        assert_eq!(m.wire(0, BATCH_TAG), Err(Diag::ESealed.val()));
        assert_eq!(m.corrode(1, BATCH_TAG), Err(Diag::ESealed.val()));
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ENode.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x90), "未知诊断码：不在 0x78FC1..0x78FCC 闭集内");
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1579_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
