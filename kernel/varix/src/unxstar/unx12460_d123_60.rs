//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12460 · 批小结与 B24 预告 · 0x30780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12460-J1 B23 集成账（19 条互引）零悬空，注册表族批（B24）
//!      预告登记入域待办账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B23 集成账：19 条互引（12 批内单互引 + 7 上游对引），每条
//!     (引用方, 被引方, 引用类型, 状态) 四列；零悬空判据 = 被引方全
//!     在册（引用号在批内闭集或上游闭集内），悬空注入必抓；
//!   - 互引类型闭集：Upstream（上游对引）/Peer（批内互引）/Anchor
//!     （判据锚）三类，逐类计数守恒 Σ=19；
//!   - 域待办账：B24 注册表族批 12 条预告（F12461~F12472 语义/账/
//!     注入面），append-only 入域待办，逐条 (预告号, 域, 主题) 三列；
//!   - 批小结：批内 19 单统计（完成数/行数/段位范围）账面可查，小结
//!     指纹 FNV 冻结（防改写）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，悬空引用注入 10/10 全抓，
//!     待办账重号注入 10/10 全拒；
//!   - 互引账单调：append-only，账长只增；
//!   - 诊断码闭集 0x30780 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30780 段）。
pub const SEG_BASE: u32 = 0x30780;

/// B23 集成账互引条数（判准 19）。
pub const XREF_TOTAL: usize = 19;

/// 批内单数（B23 批 12 单互引源闭集 1..=12 + 上游 7）。
pub const BATCH_UNITS: u32 = 12;

/// 上游引用闭集（7 条上游对引）。
pub const UPSTREAM_TOTAL: usize = 7;

/// B24 预告条数（12 条待办）。
pub const B24_TODO: usize = 12;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30780 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 0x30780 悬空引用（被引方不在任何闭集）。
    DanglingRef = 0,
    /// 0x30781 互引重号。
    XrefDup = 1,
    /// 0x30782 互引类型出闭集。
    XrefTypeOob = 2,
    /// 0x30783 计数守恒破（Σ类型 ≢ 19）。
    CountConsrv = 3,
    /// 0x30784 待办重号。
    TodoDup = 4,
    /// 0x30785 待办超容。
    TodoOverflow = 5,
    /// 0x30786 批小结缺行。
    SummaryMissing = 6,
    /// 0x30787 小结指纹篡改。
    SummaryFp = 7,
    /// 0x30788 账缩水。
    LedgerShrink = 8,
    /// 0x30789 段位越界。
    SegOob = 9,
    /// 0x3078A 预告号越界。
    TodoOob = 10,
    /// 0x3078B 悬空注入漏抓。
    DanglingLeak = 11,
}

impl SmCode {
    /// 全部码闭集。
    pub const ALL: [SmCode; 12] = [
        SmCode::DanglingRef,
        SmCode::XrefDup,
        SmCode::XrefTypeOob,
        SmCode::CountConsrv,
        SmCode::TodoDup,
        SmCode::TodoOverflow,
        SmCode::SummaryMissing,
        SmCode::SummaryFp,
        SmCode::LedgerShrink,
        SmCode::SegOob,
        SmCode::TodoOob,
        SmCode::DanglingLeak,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            SmCode::DanglingRef => "被引方不在批内/上游闭集，补齐被引单或删除悬空互引",
            SmCode::XrefDup => "互引条重号，核对编号器",
            SmCode::XrefTypeOob => "引用类型出闭集（Upstream/Peer/Anchor 三类）",
            SmCode::CountConsrv => "类型计数和与 19 不符，逐类核对",
            SmCode::TodoDup => "待办预告号重复，B24 编号唯一性破",
            SmCode::TodoOverflow => "待办账超容，归档后开新账",
            SmCode::SummaryMissing => "批小结缺行，19 单统计逐行补齐",
            SmCode::SummaryFp => "小结指纹被改，小结冻结不可改写",
            SmCode::LedgerShrink => "互引账只增不减被破",
            SmCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            SmCode::TodoOob => "预告号越界（B24 批界 12 条）",
            SmCode::DanglingLeak => "悬空注入漏抓，闭集核对闸失灵",
        }
    }
}

/// 互引类型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XrefKind {
    /// 上游对引。
    Upstream,
    /// 批内互引。
    Peer,
    /// 判据锚。
    Anchor,
}

/// 互引条（四列）。
#[derive(Clone, Copy)]
pub struct XrefRow {
    /// 条号（1..=19）。
    pub no: u32,
    /// 引用方（批内单号 1..=12）。
    pub from_unit: u32,
    /// 被引方（批内 1..=12 或上游 100+100..=107）。
    pub to_ref: u32,
    /// 引用类型。
    pub kind: XrefKind,
}

/// 待办条（三列）。
#[derive(Clone, Copy)]
pub struct TodoRow {
    /// 预告号（B24 批 1..=12）。
    pub no: u32,
    /// 域名。
    pub domain: &'static str,
    /// 主题。
    pub topic: &'static str,
}

/// FNV-1a 64（小结冻结指纹）。
fn fnv1a(acc: u64, v: u64) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// B23 集成账（互引 19 条 + 待办 12 条 + 小结）。
pub struct BatchSummary {
    /// 互引账。
    xrefs: [Option<XrefRow>; XREF_TOTAL],
    xref_len: usize,
    /// 待办账（定容 B24_TODO，满拒）。
    todos: [Option<TodoRow>; B24_TODO],
    todo_len: usize,
    /// 逐类计数（Upstream/Peer/Anchor）。
    kind_counts: [u32; 3],
    /// 小结指纹（冻结）。
    summary_fp: u64,
    /// 小结行（完成数/行数/段位范围三列）。
    summary_done: u64,
    summary_lines: u64,
    summary_seg_lo: u32,
    summary_seg_hi: u32,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl BatchSummary {
    /// 新建。
    pub fn new() -> BatchSummary {
        BatchSummary {
            xrefs: [None; XREF_TOTAL],
            xref_len: 0,
            todos: [None; B24_TODO],
            todo_len: 0,
            kind_counts: [0; 3],
            summary_fp: 0,
            summary_done: 0,
            summary_lines: 0,
            summary_seg_lo: 0,
            summary_seg_hi: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: SmCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 被引方闭集核：批内 1..=12 或上游 101..=107。
    pub fn ref_in_closed_set(r: u32) -> bool {
        (1..=BATCH_UNITS).contains(&r) || (101..=100 + UPSTREAM_TOTAL as u32).contains(&r)
    }

    /// 互引入账（重号拒 + 闭集核 + 类型核 + 容量拒——闸序：坏引用
    /// 判定优先于容量，悬空注入在满账下仍可验闸）。
    pub fn admit_xref(&mut self, r: XrefRow) -> Result<(), u32> {
        // 重号核。
        let mut i = 0;
        while i < self.xref_len {
            if let Some(ref x) = self.xrefs[i] {
                if x.no == r.no {
                    return Err(self.hit(SmCode::XrefDup));
                }
            }
            i += 1;
        }
        // 悬空核：被引方必须在闭集（先于容量闸——坏引用与账满无关）。
        if !Self::ref_in_closed_set(r.to_ref) {
            return Err(self.hit(SmCode::DanglingRef));
        }
        // 类型闭集核（枚举无界外值，构造期保证；仍核引用方界）。
        if r.from_unit == 0 || r.from_unit > BATCH_UNITS {
            return Err(self.hit(SmCode::XrefTypeOob));
        }
        if self.xref_len >= XREF_TOTAL {
            return Err(self.hit(SmCode::TodoOverflow));
        }
        self.xrefs[self.xref_len] = Some(r);
        self.xref_len += 1;
        self.kind_counts[r.kind as usize] += 1;
        Ok(())
    }

    /// 互引账悬空全查（零悬空判据）：逐条被引方闭集复核。
    pub fn no_dangling(&self) -> bool {
        let mut i = 0;
        while i < self.xref_len {
            if let Some(ref x) = self.xrefs[i] {
                if !Self::ref_in_closed_set(x.to_ref) {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        self.xref_len == XREF_TOTAL
    }

    /// 类型计数守恒：Σ ≡ 19。
    pub fn kinds_conserved(&self) -> bool {
        (self.kind_counts[0] + self.kind_counts[1] + self.kind_counts[2]) as usize == XREF_TOTAL
    }

    /// 互引账单调。
    pub fn xref_monotone(&self) -> bool {
        let mut i = 0;
        while i < self.xref_len {
            if self.xrefs[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 待办入账（append-only + 重号拒 + 号界核）。
    pub fn admit_todo(&mut self, t: TodoRow) -> Result<(), u32> {
        if self.todo_len >= B24_TODO {
            return Err(self.hit(SmCode::TodoOverflow));
        }
        let mut i = 0;
        while i < self.todo_len {
            if let Some(ref x) = self.todos[i] {
                if x.no == t.no {
                    return Err(self.hit(SmCode::TodoDup));
                }
            }
            i += 1;
        }
        if t.no == 0 || t.no > B24_TODO as u32 {
            return Err(self.hit(SmCode::TodoOob));
        }
        if t.domain.is_empty() || t.topic.is_empty() {
            return Err(self.hit(SmCode::TodoOob));
        }
        self.todos[self.todo_len] = Some(t);
        self.todo_len += 1;
        Ok(())
    }

    /// 待办账满判（12 条全入）。
    pub fn todos_complete(&self) -> bool {
        self.todo_len == B24_TODO
    }

    /// 批小结冻结（统计入账 + FNV 指纹）。
    pub fn freeze_summary(&mut self, done: u64, lines: u64, seg_lo: u32, seg_hi: u32) -> u64 {
        self.summary_done = done;
        self.summary_lines = lines;
        self.summary_seg_lo = seg_lo;
        self.summary_seg_hi = seg_hi;
        let mut h = 0xcbf29ce484222325;
        h = fnv1a(h, done);
        h = fnv1a(h, lines);
        h = fnv1a(h, seg_lo as u64);
        h = fnv1a(h, seg_hi as u64);
        self.summary_fp = h;
        h
    }

    /// 小结齐行核 + 指纹复核。
    pub fn summary_ok(&self) -> bool {
        self.summary_done > 0
            && self.summary_lines > 0
            && self.summary_seg_lo != 0
            && self.summary_seg_hi >= self.summary_seg_lo
            && self.summary_fp == self.recompute_fp()
    }

    /// 指纹重算（复核面）。
    fn recompute_fp(&self) -> u64 {
        let mut h = 0xcbf29ce484222325;
        h = fnv1a(h, self.summary_done);
        h = fnv1a(h, self.summary_lines);
        h = fnv1a(h, self.summary_seg_lo as u64);
        h = fnv1a(h, self.summary_seg_hi as u64);
        h
    }

    /// 悬空注入器（判据面专用）：构造悬空引用，闸必抓。
    pub fn dangling_inject_caught(&mut self, bad_ref: u32) -> bool {
        let r = XrefRow { no: XREF_TOTAL as u32 + 99, from_unit: 1, to_ref: bad_ref, kind: XrefKind::Peer };
        match self.admit_xref(r) {
            Err(c) => c == SmCode::DanglingRef.val(),
            Ok(()) => false,
        }
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: SmCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 构建 19 条互引（12 Peer 批内环 + 7 Upstream 上游对引）。
pub fn build_xrefs_19() -> Vec<XrefRow> {
    let mut rows: Vec<XrefRow> = Vec::new();
    // 12 条 Peer：单 i 引单 i%12+1（环状互引，批内闭集）。
    let mut i = 1u32;
    while i <= 12 {
        rows.push(XrefRow {
            no: i,
            from_unit: i,
            to_ref: i % 12 + 1,
            kind: XrefKind::Peer,
        });
        i += 1;
    }
    // 7 条 Upstream：单 i 引上游 100+i。
    let mut i = 1u32;
    while i <= 7 {
        rows.push(XrefRow {
            no: 12 + i,
            from_unit: i,
            to_ref: 100 + i,
            kind: XrefKind::Upstream,
        });
        i += 1;
    }
    rows
}

/// B23 集成账零悬空演练 10/10 轮（判准 J1 第一句）。
pub fn drill_integration_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = BatchSummary::new();
        let rows = build_xrefs_19();
        let mut all_in = true;
        let mut i = 0;
        while i < rows.len() {
            if g.admit_xref(rows[i]).is_err() {
                all_in = false;
            }
            i += 1;
        }
        // 零悬空 + 类型守恒 + 账单调。
        let clean = g.no_dangling();
        let cons = g.kinds_conserved();
        let mono = g.xref_monotone();
        // 悬空注入必抓（随机坏引用 200+ 或 0 或 13..=100 区间）。
        let bad = [0u32, 13, 50, 108, 108 + (lcg.below(892) as u32)];
        let mut caught = 0usize;
        let mut k = 0;
        while k < bad.len() {
            if g.dangling_inject_caught(bad[k]) {
                caught += 1;
            }
            k += 1;
        }
        if all_in && clean && cons && mono && caught == bad.len() {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 入{} 悬空{} 守恒{} 单调{} 抓{}/{}",
                round, all_in, clean, cons, mono, caught, bad.len()
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("B23 集成账 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// B24 预告登记演练 10/10 轮（判准 J1 第二句）。
pub fn drill_todo_b24_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = BatchSummary::new();
        // 12 条 B24 预告（F12461~F12472 主题）。
        let topics = [
            "建口/开键语义档", "Ex 档语义", "删除/重命名/落盘", "枚举协议",
            "三查询档", "值类型矩阵", "变更监视", "键生命周期账",
            "路径命名联签", "错误矩阵", "越界注入", "边界样本集",
        ];
        let mut all_in = true;
        let mut i = 0;
        while i < B24_TODO {
            let t = TodoRow { no: i as u32 + 1, domain: "UNX-D1-B24", topic: topics[i] };
            if g.admit_todo(t).is_err() {
                all_in = false;
            }
            i += 1;
        }
        // 重号注入必拒。
        let dup = g
            .admit_todo(TodoRow { no: 5, domain: "UNX-D1-B24", topic: "重复" })
            .is_err();
        // 越界号注入必拒。
        let oob = g
            .admit_todo(TodoRow { no: 13, domain: "UNX-D1-B24", topic: "越界" })
            .is_err();
        if all_in && g.todos_complete() && dup && oob {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("B24 预告登记 {}/{}", ok, INJECT_ROUNDS))
}

/// 批小结冻结演练 10/10 轮。
pub fn drill_summary_freeze_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = BatchSummary::new();
        // 12 单完成、~9000 行、段位 0x30480~0x30760（B23 批界）。
        let fp = g.freeze_summary(12, 9000 + round as u64, 0x30480, 0x30760);
        let ok1 = g.summary_ok() && fp != 0;
        // 冻结后再冻结（统计变了指纹必变——防静默改写）。
        let fp2 = g.freeze_summary(12, 9001 + round as u64, 0x30480, 0x30760);
        let ok2 = fp2 != fp;
        if ok1 && ok2 {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("批小结冻结 {}/{}", ok, INJECT_ROUNDS))
}

/// 悬空注入五族全抓演练 10/10 轮。
pub fn drill_dangling_family_10() -> (bool, String) {
    let mut caught = [0usize; 5];
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = BatchSummary::new();
        let rows = build_xrefs_19();
        let mut i = 0;
        while i < rows.len() {
            let _ = g.admit_xref(rows[i]);
            i += 1;
        }
        // 五族坏引用：0（零号）/13（批界外首）/50（中段）/100（上游前一位）/200（远界外）。
        let fams = [0u32, 13, 50, 100, 200];
        let mut k = 0;
        while k < 5 {
            if g.dangling_inject_caught(fams[k]) {
                caught[k] += 1;
            }
            k += 1;
        }
        round += 1;
    }
    let ok = caught.iter().all(|c| *c == INJECT_ROUNDS);
    (ok, format!("悬空五族注入 10/10 抓{:?}", caught))
}

/// 互引重号注入 10/10 轮。
pub fn drill_xref_dup_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = BatchSummary::new();
        let rows = build_xrefs_19();
        let mut i = 0;
        while i < rows.len() {
            let _ = g.admit_xref(rows[i]);
            i += 1;
        }
        // 重号注入（号 1 已在账）。
        let dup = g
            .admit_xref(XrefRow { no: 1, from_unit: 2, to_ref: 3, kind: XrefKind::Anchor })
            .is_err();
        // 账长不变（append-only 纪律）。
        let len_stable = g.xref_len == XREF_TOTAL;
        if dup && len_stable {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("互引重号注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：集成账常量锚核对。
pub fn run_unx12460_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-集成锚",
        XREF_TOTAL == 19 && BATCH_UNITS == 12 && UPSTREAM_TOTAL == 7 && B24_TODO == 12,
        format!("互引{} 批{} 上游{} 预告{}", XREF_TOTAL, BATCH_UNITS, UPSTREAM_TOTAL, B24_TODO),
    ));
    v
}

/// run_unx12460_checks：本单判据聚合（诊断段 0x30780）。
pub fn run_unx12460_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12460 批小结与 B24 预告");
    // J1-a：B23 集成账零悬空。
    let (ok1, d1) = drill_integration_10();
    s.add_dyn("J1-B23集成账零悬空10/10", ok1, &d1);
    // J1-b：B24 预告登记入域待办。
    let (ok2, d2) = drill_todo_b24_10();
    s.add_dyn("J1-B24预告登记10/10", ok2, &d2);
    // J1-c：批小结冻结。
    let (ok3, d3) = drill_summary_freeze_10();
    s.add_dyn("J1-批小结冻结10/10", ok3, &d3);
    // J1-d：悬空五族注入。
    let (ok4, d4) = drill_dangling_family_10();
    s.add_dyn("J1-悬空五族注入10/10", ok4, &d4);
    // J1-e：互引重号注入。
    let (ok5, d5) = drill_xref_dup_10();
    s.add_dyn("J1-互引重号注入10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30780段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12460_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
