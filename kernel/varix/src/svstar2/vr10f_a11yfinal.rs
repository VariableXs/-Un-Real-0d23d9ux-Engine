//! VE-F3786 · R 域无障碍终审（R10 收口组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3786`
//!
//! **判据（锚点原文）**：三册终审、眼动量化、豁免禁令、一票联动、双签确认、
//! 判据。
//!
//! **本条是什么**：双维无障碍域级终审——工具维（五关+双标）+ 产出维
//! （分类验证+豁免禁令）+ 隐私册三册终审；WCAG 矩阵复核；跨域联动
//! （E/S/N 三对端）；眼动 80% 量化证据核验；1000+ 断言独立门。
//! 豁免非零即 P0（一票联动）。
//!
//! 诊断码段：独占 0xB1xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB1xx）
// ─────────────────────────────────────────────────────────────

/// F3786 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B1Code(pub u16);

impl B1Code {
    /// 缺口无着落——NO-GO 风险（一票联动）。
    pub const GAP_NO_OWNER: B1Code = B1Code(0xB101);
    /// 三方分歧——立案。
    pub const TRIPARTITE: B1Code = B1Code(0xB102);
    /// 双维混册——分册（红线）。
    pub const MIXED_BOOK: B1Code = B1Code(0xB103);
    /// 虚登——伪证。
    pub const FALSE_RECORD: B1Code = B1Code(0xB104);
    /// 豁免非零——P0。
    pub const EXEMPT_NONZERO: B1Code = B1Code(0xB105);
    /// 三册缺册。
    pub const BOOK_MISS: B1Code = B1Code(0xB106);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB101 => "无障碍缺口无责任方，一票联动 NO-GO。",
            0xB102 => "跨域三方分歧，需立案仲裁。",
            0xB103 => "双维混册，须分册（红线）。",
            0xB104 => "量化证据虚登，按伪证处理。",
            0xB105 => "产出豁免非零，P0 红线。",
            0xB106 => "三册终审缺册。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三册（工具维/产出维/隐私册）
// ─────────────────────────────────────────────────────────────

/// 终审册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Book {
    /// 工具维（五关+双标）。
    Tool,
    /// 产出维（分类验证+豁免禁令）。
    Output,
    /// 隐私册。
    Privacy,
}

impl Book {
    /// 全集。
    pub const ALL: [Book; 3] = [Book::Tool, Book::Output, Book::Privacy];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Book::Tool => 0,
            Book::Output => 1,
            Book::Privacy => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Book::Tool => "工具维册",
            Book::Output => "产出维册",
            Book::Privacy => "隐私册",
        }
    }
}

/// 三册终审表。
#[derive(Clone, Debug)]
pub struct TriBook {
    verified: [bool; 3],
    /// 工具维豁免数（须 0）。
    pub tool_exempt: u32,
    /// 产出维豁免数（须 0——禁令）。
    pub output_exempt: u32,
    /// 双维分册标志（混册 = 红线）。
    pub separated: bool,
}

impl TriBook {
    /// 新建。
    pub const fn new() -> Self {
        TriBook {
            verified: [false; 3],
            tool_exempt: 0,
            output_exempt: 0,
            separated: true,
        }
    }

    /// 核一册。
    pub fn verify(&mut self, b: Book) -> bool {
        let i = b.ordinal();
        if self.verified[i] {
            false
        } else {
            self.verified[i] = true;
            true
        }
    }

    /// 已核数。
    pub fn count(&self) -> usize {
        self.verified.iter().filter(|v| **v).count()
    }

    /// 三册齐。
    pub fn complete(&self) -> bool {
        self.count() == 3
    }

    /// 豁免禁令通过（双维豁免皆 0）。
    pub fn exempt_zero(&self) -> bool {
        self.tool_exempt == 0 && self.output_exempt == 0
    }

    /// 一票联动判定（任一硬伤即 false）。
    pub fn joint_pass(&self) -> bool {
        self.complete() && self.exempt_zero() && self.separated
    }
}

// ─────────────────────────────────────────────────────────────
// 眼动量化 + 断言独立门
// ─────────────────────────────────────────────────────────────

/// 量化证据（眼动 80% 线）。
#[derive(Clone, Copy, Debug)]
pub struct QuantEvidence {
    /// 样本数。
    pub samples: u32,
    /// 达标数。
    pub hits: u32,
    /// 是否虚登。
    pub falsified: bool,
}

impl QuantEvidence {
    /// 新建。
    pub const fn new() -> Self {
        QuantEvidence {
            samples: 0,
            hits: 0,
            falsified: false,
        }
    }

    /// 达标率万分比。
    pub fn rate_bp(&self) -> u32 {
        if self.samples == 0 {
            0
        } else {
            (self.hits * 10_000) / self.samples
        }
    }

    /// 过 80% 线（8000 基点）。
    pub fn pass_80(&self) -> bool {
        self.rate_bp() >= 8_000 && !self.falsified
    }
}

/// 断言独立门（1000+ 断言）。
#[derive(Clone, Debug)]
pub struct AssertGate {
    /// 断言总数。
    pub total: u32,
    /// 通过数。
    pub pass: u32,
}

impl AssertGate {
    /// 新建。
    pub const fn new() -> Self {
        AssertGate { total: 0, pass: 0 }
    }

    /// 记断言。
    pub fn record(&mut self, ok: bool) {
        self.total += 1;
        if ok {
            self.pass += 1;
        }
    }

    /// 过门（≥1000 断言且全过）。
    pub fn gate_pass(&self) -> bool {
        self.total >= 1000 && self.pass == self.total
    }

    /// 失败数。
    pub fn fail(&self) -> u32 {
        self.total.saturating_sub(self.pass)
    }
}

// ─────────────────────────────────────────────────────────────
// 跨域联动（E/S/N 三对端）
// ─────────────────────────────────────────────────────────────

/// 对端域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Peer {
    /// E 域。
    E,
    /// S 域。
    S,
    /// N 域。
    N,
}

impl Peer {
    /// 全集。
    pub const ALL: [Peer; 3] = [Peer::E, Peer::S, Peer::N];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Peer::E => 0,
            Peer::S => 1,
            Peer::N => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Peer::E => "E",
            Peer::S => "S",
            Peer::N => "N",
        }
    }
}

/// 联动协议。
#[derive(Clone, Debug)]
pub struct JointProtocol {
    /// 三对端确认。
    ack: [bool; 3],
    /// 分歧数。
    pub disputes: u32,
}

impl JointProtocol {
    /// 新建。
    pub const fn new() -> Self {
        JointProtocol {
            ack: [false; 3],
            disputes: 0,
        }
    }

    /// 对端确认。
    pub fn ack_peer(&mut self, p: Peer) -> bool {
        let i = p.ordinal();
        if self.ack[i] {
            false
        } else {
            self.ack[i] = true;
            true
        }
    }

    /// 三方齐。
    pub fn tri_ready(&self) -> bool {
        self.ack.iter().filter(|a| **a).count() == 3
    }

    /// 无分歧。
    pub fn no_dispute(&self) -> bool {
        self.disputes == 0
    }

    /// 联动通过。
    pub fn pass(&self) -> bool {
        self.tri_ready() && self.no_dispute()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3786 判据集。
pub fn run_vr10f_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3786");

    // 0xB106 三册
    {
        let mut t = TriBook::new();
        set.add("0xB106-初始零册", t.count() == 0, "");
        for b in Book::ALL.iter() {
            t.verify(*b);
        }
        set.add("0xB106-三册齐", t.complete(), "");
        set.add("0xB106-重核拒", !t.verify(Book::Tool), "");
    }

    // 0xB105 豁免禁令
    {
        let mut t = TriBook::new();
        for b in Book::ALL.iter() {
            t.verify(*b);
        }
        set.add("0xB105-零豁免过", t.exempt_zero(), "");
        t.output_exempt = 1;
        set.add("0xB105-产出豁免P0", !t.exempt_zero(), "");
        set.add("0xB105-一票联动拒", !t.joint_pass(), "");
    }

    // 0xB103 双维分册
    {
        let mut t = TriBook::new();
        for b in Book::ALL.iter() {
            t.verify(*b);
        }
        set.add("0xB103-分册过", t.joint_pass(), "");
        t.separated = false;
        set.add("0xB103-混册拒", !t.joint_pass(), "");
    }

    // 0xB104 眼动量化
    {
        let mut q = QuantEvidence {
            samples: 100,
            hits: 80,
            falsified: false,
        };
        set.add("0xB104-80%正好过", q.rate_bp() == 8000 && q.pass_80(), "");
        q.hits = 79;
        set.add("0xB104-79%不过", !q.pass_80(), "");
        q.falsified = true;
        q.hits = 99;
        set.add("0xB104-虚登不过", !q.pass_80(), "");
    }

    // 断言独立门
    {
        let mut g = AssertGate::new();
        for _ in 0..1000 {
            g.record(true);
        }
        set.add("0xB101-千断全过", g.gate_pass() && g.fail() == 0, "");
        g.record(false);
        set.add("0xB101-一败不过门", !g.gate_pass() && g.fail() == 1, "");
    }

    // 0xB102 跨域联动
    {
        let mut j = JointProtocol::new();
        set.add("0xB102-初始不齐", !j.tri_ready(), "");
        for p in Peer::ALL.iter() {
            j.ack_peer(*p);
        }
        set.add("0xB102-三方齐", j.tri_ready(), "");
        set.add("0xB102-无分歧过", j.pass(), "");
        j.disputes = 1;
        set.add("0xB102-分歧不过", !j.pass(), "");
        set.add("0xB102-重确认拒", !j.ack_peer(Peer::E), "");
    }

    // 册序自洽
    {
        let mut ok = true;
        for (i, b) in Book::ALL.iter().enumerate() {
            if b.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB106-册序自洽", ok, "");
    }

    set
}
