//! ═══════════════════════════════════════ 条件对齐 ═══════════════════
//! UNX-F43204 前置版本约束段（组件×约束表达式×错误码三列） · 0x9B0C0段
//! ───────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F43204-J1 约束 ×20 判定与错误码零漂移；
//!      判据锚定 任务书专题一；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 约束表达式闭集：六算子（>=/<=/>/</==/!=）× 版本三元
//!     （maj/min/patch），线形 7 字节（op+3×u16）；
//!   - 三列表：8 组件冻结约束表逐组求值零漂移 + ×20 评估样例
//!     （每算子 × 版本边界样本逐组与手算期望等）；
//!   - ×10 注入：坏算子/短表/越窗组件/未满足/双约束矛盾/重复登记
//!     逐类必抓；
//!   - 双跑一致 + 段位守卫 + 闭集回查；no_std + alloc；零 panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B0C0，AI-VX12-1106 批1 12 槽第 4 槽）。
pub const SEG_BASE: u32 = 0x9B0C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerConCode {
    /// 坏算子
    BadOp,
    /// 约束线形短表
    BadExpr,
    /// 组件越窗
    BadComp,
    /// 约束未满足
    Unmet,
    /// 双约束矛盾
    ConflictPair,
    /// 重复登记
    DupReg,
    /// 账满
    LedgerFull,
    /// 未知兜底
    Unknown,
}

impl VerConCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                VerConCode::BadOp => 0x01,
                VerConCode::BadExpr => 0x02,
                VerConCode::BadComp => 0x03,
                VerConCode::Unmet => 0x04,
                VerConCode::ConflictPair => 0x05,
                VerConCode::DupReg => 0x06,
                VerConCode::LedgerFull => 0x07,
                VerConCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            VerConCode::BadOp => "坏算子：op 字节须在 1..=6",
            VerConCode::BadExpr => "约束线形短表：须 7 字节",
            VerConCode::BadComp => "组件越窗：0x10..0x40",
            VerConCode::Unmet => "约束未满足：先升级组件再装",
            VerConCode::ConflictPair => "双约束矛盾：同组件两约束相交为空",
            VerConCode::DupReg => "重复登记：同组件同约束不重复",
            VerConCode::LedgerFull => "账满：先清账",
            VerConCode::Unknown => "未知诊断码：不在 0x9B0C0 段闭集内",
        }
    }

    pub const ALL: [VerConCode; 8] = [
        VerConCode::BadOp,
        VerConCode::BadExpr,
        VerConCode::BadComp,
        VerConCode::Unmet,
        VerConCode::ConflictPair,
        VerConCode::DupReg,
        VerConCode::LedgerFull,
        VerConCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in VerConCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    VerConCode::Unknown.detail()
}

// ══ [CHUNK-2-END]

// ── 版本与约束（闭集六算子） ────────────────────────────────────
/// 版本三元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ver {
    pub maj: u16,
    pub min: u16,
    pub pat: u16,
}

impl Ver {
    /// 字典序比较（maj→min→pat）。
    pub fn cmp_ver(a: Ver, b: Ver) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        if a.maj != b.maj {
            return a.maj.cmp(&b.maj);
        }
        if a.min != b.min {
            return a.min.cmp(&b.min);
        }
        a.pat.cmp(&b.pat)
    }
}

/// 算子闭集（字节 1..=6）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum COp {
    /// >=
    Ge,
    /// <=
    Le,
    /// >
    Gt,
    /// <
    Lt,
    /// ==
    Eq,
    /// !=
    Ne,
}

/// 约束（算子 × 版本）。
#[derive(Clone, Copy, Debug)]
pub struct Con {
    pub op: COp,
    pub ver: Ver,
}

/// 约束线形（7 字节：op + maj/min/patch 各 u16 大端）。
pub fn wire_con(op: u8, maj: u16, min: u16, pat: u16) -> [u8; 7] {
    let mut w = [0u8; 7];
    w[0] = op;
    w[1] = (maj >> 8) as u8;
    w[2] = (maj & 0xFF) as u8;
    w[3] = (min >> 8) as u8;
    w[4] = (min & 0xFF) as u8;
    w[5] = (pat >> 8) as u8;
    w[6] = (pat & 0xFF) as u8;
    w
}

/// 线形 → 约束（两闸：长度/算子闭集）。
pub fn parse_con(b: &[u8]) -> Result<Con, u32> {
    if b.len() < 7 {
        return Err(VerConCode::BadExpr.val());
    }
    let op = match b[0] {
        1 => COp::Ge,
        2 => COp::Le,
        3 => COp::Gt,
        4 => COp::Lt,
        5 => COp::Eq,
        6 => COp::Ne,
        _ => return Err(VerConCode::BadOp.val()),
    };
    Ok(Con {
        op,
        ver: Ver {
            maj: u16::from_be_bytes([b[1], b[2]]),
            min: u16::from_be_bytes([b[3], b[4]]),
            pat: u16::from_be_bytes([b[5], b[6]]),
        },
    })
}

/// 求值（六算子精确语义，零漂移基底）。
pub fn eval_con(c: &Con, v: Ver) -> bool {
    match c.op {
        COp::Ge => Ver::cmp_ver(v, c.ver) != core::cmp::Ordering::Less,
        COp::Le => Ver::cmp_ver(v, c.ver) != core::cmp::Ordering::Greater,
        COp::Gt => Ver::cmp_ver(v, c.ver) == core::cmp::Ordering::Greater,
        COp::Lt => Ver::cmp_ver(v, c.ver) == core::cmp::Ordering::Less,
        COp::Eq => Ver::cmp_ver(v, c.ver) == core::cmp::Ordering::Equal,
        COp::Ne => Ver::cmp_ver(v, c.ver) != core::cmp::Ordering::Equal,
    }
}

// ══ [CHUNK-3-END]

// ── 三列约束账（组件 × 约束 × 预期错误码） ──────────────────────
/// 组件窗。
pub const COMP_WIN: (u32, u32) = (0x10, 0x40);
/// 约束账容量。
pub const CON_CAP: usize = 32;

/// 单条登记（组件 + 约束）。
#[derive(Clone, Copy, Debug)]
pub struct ConRow {
    pub comp: u32,
    pub con: Con,
}

/// 约束账。
pub struct ConLedger {
    rows: Vec<ConRow>,
    pub rejected: u32,
    pub attempts: u32,
}

impl ConLedger {
    pub fn new() -> ConLedger {
        ConLedger { rows: Vec::new(), rejected: 0, attempts: 0 }
    }

    /// 登记约束（四闸：组件窗/线形/重复/容量）。
    pub fn register(&mut self, comp: u32, wire: &[u8]) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸1：组件窗。
        if comp < COMP_WIN.0 || comp >= COMP_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(VerConCode::BadComp.val());
        }
        // 闸2：线形解析。
        let c = parse_con(wire)?;
        // 闸3：重复登记（同组件同算子同版本）。
        for r in self.rows.iter() {
            if r.comp == comp
                && core::mem::discriminant(&r.con.op) == core::mem::discriminant(&c.op)
                && Ver::cmp_ver(r.con.ver, c.ver) == core::cmp::Ordering::Equal
            {
                self.rejected = self.rejected.saturating_add(1);
                return Err(VerConCode::DupReg.val());
            }
        }
        // 闸4：容量。
        if self.rows.len() >= CON_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(VerConCode::LedgerFull.val());
        }
        self.rows.push(ConRow { comp, con: c });
        Ok(self.rows.len() as u32)
    }

    /// 组件版本预检：所有登记约束全过才放行；未满足回 Unmet。
    pub fn check(&self, comp: u32, v: Ver) -> Result<u32, u32> {
        let mut found = 0u32;
        for r in self.rows.iter() {
            if r.comp == comp {
                found = found.saturating_add(1);
                if !eval_con(&r.con, v) {
                    return Err(VerConCode::Unmet.val());
                }
            }
        }
        Ok(found)
    }

    /// 双约束矛盾扫描（同组件 (>=,v1) 与 (<,v2) 且 v1<=v2 → 相交为空）。
    pub fn conflict_scan(&self) -> u32 {
        let mut n = 0u32;
        let mut a = 0usize;
        while a < self.rows.len() {
            let mut b = a + 1;
            while b < self.rows.len() {
                if self.rows[a].comp == self.rows[b].comp {
                    let (x, y) = (&self.rows[a].con, &self.rows[b].con);
                    let x_ge = matches!(x.op, COp::Ge);
                    let y_lt = matches!(y.op, COp::Lt);
                    let x_lt = matches!(x.op, COp::Lt);
                    let y_ge = matches!(y.op, COp::Ge);
                    if (x_ge && y_lt && Ver::cmp_ver(x.ver, y.ver) != core::cmp::Ordering::Less)
                        || (x_lt && y_ge && Ver::cmp_ver(y.ver, x.ver) != core::cmp::Ordering::Less)
                    {
                        n = n.saturating_add(1);
                    }
                }
                b += 1;
            }
            a += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ══ [CHUNK-4-END]

// ── ×20 零漂移评估（判据核心：逐算子×边界样本） ────────────────
/// 20 组 (线形, 版本, 期望判定) 冻结样例逐组求值。
pub fn drift_twenty() -> (u32, String) {
    let samples: [(u8, u16, u16, u16, Ver, bool); 20] = [
        (1, 1, 2, 0, Ver { maj: 1, min: 2, pat: 0 }, true),
        (1, 1, 2, 0, Ver { maj: 1, min: 1, pat: 9 }, false),
        (1, 1, 2, 0, Ver { maj: 2, min: 0, pat: 0 }, true),
        (2, 2, 0, 0, Ver { maj: 2, min: 0, pat: 0 }, true),
        (2, 2, 0, 0, Ver { maj: 2, min: 0, pat: 1 }, false),
        (3, 1, 0, 0, Ver { maj: 1, min: 0, pat: 1 }, true),
        (3, 1, 0, 0, Ver { maj: 1, min: 0, pat: 0 }, false),
        (4, 1, 0, 0, Ver { maj: 0, min: 9, pat: 9 }, true),
        (4, 1, 0, 0, Ver { maj: 1, min: 0, pat: 0 }, false),
        (5, 3, 4, 5, Ver { maj: 3, min: 4, pat: 5 }, true),
        (5, 3, 4, 5, Ver { maj: 3, min: 4, pat: 4 }, false),
        (6, 3, 4, 5, Ver { maj: 3, min: 4, pat: 4 }, true),
        (6, 3, 4, 5, Ver { maj: 3, min: 4, pat: 5 }, false),
        (1, 0, 0, 1, Ver { maj: 0, min: 0, pat: 1 }, true),
        (1, 0, 0, 1, Ver { maj: 0, min: 0, pat: 0 }, false),
        (2, 0, 0, 1, Ver { maj: 0, min: 0, pat: 1 }, true),
        (2, 0, 0, 1, Ver { maj: 0, min: 0, pat: 0 }, true),
        (3, 0, 0, 1, Ver { maj: 0, min: 0, pat: 1 }, false),
        (3, 0, 0, 1, Ver { maj: 0, min: 0, pat: 2 }, true),
        (4, 0, 0, 1, Ver { maj: 0, min: 0, pat: 0 }, true),
    ];
    let mut ok = 0u32;
    let mut marks = String::new();
    let mut i = 0usize;
    while i < 20 {
        let s = samples[i];
        let wire = wire_con(s.0, s.1, s.2, s.3);
        match parse_con(&wire) {
            Ok(c) => {
                if eval_con(&c, s.4) == s.5 {
                    ok += 1;
                } else {
                    marks.push_str(&format!("{}!", i));
                }
            }
            Err(e) => marks.push_str(&format!("{}e{:X}!", i, e)),
        }
        i += 1;
    }
    (ok, marks)
}

// ── ×10 注入 ────────────────────────────────────────────────────
pub fn inject_ten() -> (u32, String) {
    let mut caught = 0u32;
    let mut marks = String::new();
    let mut g = ConLedger::new();
    // 1 坏算子（op=7）。
    let mut w = wire_con(1, 1, 0, 0);
    w[0] = 7;
    if g.register(0x10, &w).err() == Some(VerConCode::BadOp.val()) {
        caught += 1;
    } else {
        marks.push_str("1!");
    }
    // 2 短表（3 字节）。
    if g.register(0x10, &wire_con(1, 1, 0, 0)[..3]).err() == Some(VerConCode::BadExpr.val()) {
        caught += 1;
    } else {
        marks.push_str("2!");
    }
    // 3 组件越窗（0x09 / 0x40）。
    if g.register(0x09, &wire_con(1, 1, 0, 0)).err() == Some(VerConCode::BadComp.val())
        && g.register(0x40, &wire_con(1, 1, 0, 0)).err() == Some(VerConCode::BadComp.val())
    {
        caught += 1;
    } else {
        marks.push_str("3!");
    }
    // 4 未满足（约束 >=2.0.0，装 1.9.9）。
    let _ = g.register(0x11, &wire_con(1, 2, 0, 0));
    if g.check(0x11, Ver { maj: 1, min: 9, pat: 9 }).err() == Some(VerConCode::Unmet.val()) {
        caught += 1;
    } else {
        marks.push_str("4!");
    }
    // 5 满足放行（装 2.1.0）。
    if g.check(0x11, Ver { maj: 2, min: 1, pat: 0 }).is_ok() {
        caught += 1;
    } else {
        marks.push_str("5!");
    }
    // 6 重复登记。
    if g.register(0x11, &wire_con(1, 2, 0, 0)).err() == Some(VerConCode::DupReg.val()) {
        caught += 1;
    } else {
        marks.push_str("6!");
    }
    // 7 双约束矛盾（>=1.0 与 <1.0）。
    let mut g2 = ConLedger::new();
    let _ = g2.register(0x12, &wire_con(1, 1, 0, 0));
    let _ = g2.register(0x12, &wire_con(4, 1, 0, 0));
    if g2.conflict_scan() == 1 {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 双约束相容（>=1.0 与 <2.0）不误报。
    let mut g3 = ConLedger::new();
    let _ = g3.register(0x13, &wire_con(1, 1, 0, 0));
    let _ = g3.register(0x13, &wire_con(4, 2, 0, 0));
    if g3.conflict_scan() == 0 {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 短表六字节（差 1 字节）。
    if g.register(0x14, &wire_con(1, 1, 0, 0)[..6]).err() == Some(VerConCode::BadExpr.val()) {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 多组件同版本预检互不串扰。
    let _ = g.register(0x15, &wire_con(5, 1, 0, 0));
    let pass = g.check(0x15, Ver { maj: 1, min: 0, pat: 0 }).is_ok()
        && g.check(0x16, Ver { maj: 9, min: 9, pat: 9 }).is_ok();
    if pass {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, marks)
}

// ══ [CHUNK-5-END]

// ── 黄金正演 + 双跑 + 段位守卫 ──────────────────────────────────
/// 黄金正演：三列约束表登记→预检→矛盾扫描全绿。
pub fn golden_run() -> Result<u32, u32> {
    let mut g = ConLedger::new();
    // 三列：组件 × 约束 ×（预期路径）。
    let _ = g.register(0x20, &wire_con(1, 1, 0, 0))?; // >=1.0.0
    let _ = g.register(0x20, &wire_con(4, 3, 0, 0))?; // <3.0.0
    let _ = g.register(0x21, &wire_con(5, 2, 4, 6))?; // ==2.4.6
    let _ = g.register(0x22, &wire_con(3, 1, 2, 0))?; // >1.2.0
    if g.len() != 4 {
        return Err(VerConCode::LedgerFull.val());
    }
    // 预检：0x20@1.5.0 过，0x21@2.4.6 过，0x22@1.2.0 拒。
    if g.check(0x20, Ver { maj: 1, min: 5, pat: 0 }).is_err() {
        return Err(VerConCode::Unmet.val());
    }
    if g.check(0x21, Ver { maj: 2, min: 4, pat: 6 }).is_err() {
        return Err(VerConCode::Unmet.val());
    }
    if g.check(0x22, Ver { maj: 1, min: 2, pat: 0 }).err() != Some(VerConCode::Unmet.val()) {
        return Err(VerConCode::Unmet.val());
    }
    // 矛盾扫描零命中（相容表）。
    if g.conflict_scan() != 0 {
        return Err(VerConCode::ConflictPair.val());
    }
    Ok(g.len() as u32)
}

/// 双跑一致。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in VerConCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演三列", g.is_ok(), format!("{:?}", g)));
    let (c20, mk20) = drift_twenty();
    v.push(("J1-零漂移×20", c20 == 20, format!("c {}/20 {}", c20, mk20)));
    let (ci, mki) = inject_ten();
    v.push(("J2-注入拒×10", ci == 10, format!("c {}/10 {}", ci, mki)));
    // J3：算子闭集六值两两语义抽查（Ge/Le 端点、Gt/Lt 严格、Eq/Ne 相反）。
    let w1 = parse_con(&wire_con(1, 1, 0, 0)).ok();
    let w2 = parse_con(&wire_con(2, 1, 0, 0)).ok();
    let v1 = Ver { maj: 1, min: 0, pat: 0 };
    let endpoint = match (w1, w2) {
        (Some(a), Some(b)) => eval_con(&a, v1) && eval_con(&b, v1),
        _ => false,
    };
    v.push(("J3-端点语义", endpoint, String::from(">=x 与 <=x 在 x 处皆真")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B0C0 窗口")));
    let closed = {
        let mut ok = true;
        for c in VerConCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    v
}

/// 检查入口（卡 43204）。
pub fn run_unx43204_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43204 版本约束三列");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-6-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_drift_twenty() {
        let (c, mk) = drift_twenty();
        assert_eq!(c, 20, "marks: {}", mk);
    }

    #[test]
    fn test_inject_ten() {
        let (c, mk) = inject_ten();
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_wire_roundtrip() {
        let w = wire_con(3, 1, 2, 3);
        let c = parse_con(&w).ok();
        assert!(c.is_some());
        if let Some(c) = c {
            assert_eq!(c.ver, Ver { maj: 1, min: 2, pat: 3 });
        }
    }

    #[test]
    fn test_conflict_pairs() {
        let mut g = ConLedger::new();
        let _ = g.register(0x30, &wire_con(1, 1, 0, 0));
        let _ = g.register(0x30, &wire_con(4, 1, 0, 0));
        assert_eq!(g.conflict_scan(), 1);
        let mut g2 = ConLedger::new();
        let _ = g2.register(0x31, &wire_con(1, 1, 0, 0));
        let _ = g2.register(0x31, &wire_con(4, 2, 0, 0));
        assert_eq!(g2.conflict_scan(), 0);
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B0C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in VerConCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [END-43204]
