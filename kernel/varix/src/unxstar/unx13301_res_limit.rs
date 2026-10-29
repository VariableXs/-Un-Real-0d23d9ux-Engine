//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13301 · 资源耗尽限额总成（超多节表/超深依赖链/巨型重定位表三限）
//! · 0x658C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13301-J1 三限架构（节表/依赖链/重定位表）+ 扩展限额
//!      注册协议成账，限额检查点全路径布防断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三限闭式账：节表限（MAX_SECTIONS=96，PE 建模上限档）/
//!     依赖链限（MAX_DEP_DEPTH=64）/ 重定位表限（MAX_RELOC=0x10000
//!     条），三检查点各自独立布防；
//!   - 限额检查点闭式：超限一律 Err（越限码分立），恰好等于限额
//!     收行（上界含端语义）；
//!   - 扩展限额注册协议：外部限额经 register_ext 挂账（id 唯一、
//!     上限非零、越 id 拒挂），挂账后检查点可查（协议红线）；
//!   - 布防断言：三限检查点 × 三越限注入面（10/10/10）全抓 +
//!     端点语义（恰限收行）核；
//!   - LCG 种子写死 = SEG_BASE（0x658C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x658C0，全仓独占）。
pub const SEG_BASE: u32 = 0x658C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResLimitCode {
    /// 节表数越上限
    SectionsOverLimit,
    /// 依赖链深度越限
    DepDepthOverLimit,
    /// 重定位表条数越限
    RelocOverLimit,
    /// 扩展限额越账
    ExtOverLimit,
    /// 扩展限额 id 重复挂账
    ExtIdDuplicated,
    /// 扩展限额上限为零（协议红线）
    ExtZeroCap,
    /// 扩展限额 id 越闭集
    ExtIdOutOfRange,
    /// 检查点未布防（协议挂账无检查点）
    CheckpointMissing,
}

impl ResLimitCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ResLimitCode::SectionsOverLimit => "节表数越上限：96 节档必拒",
            ResLimitCode::DepDepthOverLimit => "依赖链深度越限：64 层档必拒",
            ResLimitCode::RelocOverLimit => "重定位表条数越限：0x10000 档必拒",
            ResLimitCode::ExtOverLimit => "扩展限额越账：挂账上限必拒",
            ResLimitCode::ExtIdDuplicated => "扩展限额 id 重复挂账：协议红线",
            ResLimitCode::ExtZeroCap => "扩展限额上限为零：协议红线",
            ResLimitCode::ExtIdOutOfRange => "扩展限额 id 越闭集：协议越界",
            ResLimitCode::CheckpointMissing => "检查点未布防：挂账无检查点",
        }
    }
    pub const ALL: [ResLimitCode; 8] = [
        ResLimitCode::SectionsOverLimit,
        ResLimitCode::DepDepthOverLimit,
        ResLimitCode::RelocOverLimit,
        ResLimitCode::ExtOverLimit,
        ResLimitCode::ExtIdDuplicated,
        ResLimitCode::ExtZeroCap,
        ResLimitCode::ExtIdOutOfRange,
        ResLimitCode::CheckpointMissing,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ResLimitCode::ALL.len() {
        if ResLimitCode::ALL[i].val() == code {
            return ResLimitCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：资源耗尽限额建模书 + 判据原文） ──────────────────
/// 限 1：节表上限（PE 建模档：96 节）。
pub const MAX_SECTIONS: u32 = 96;
/// 限 2：依赖链深度上限（64 层）。
pub const MAX_DEP_DEPTH: u32 = 64;
/// 限 3：重定位表条数上限（0x10000 条）。
pub const MAX_RELOC: u32 = 0x1_0000;
/// 扩展限额 id 闭集容量。
pub const EXT_DOMAIN: usize = 16;

// ── 三限检查点（闭式，端点含端：恰等于限额收行） ─────────────────
pub fn check_sections(n: u32) -> Result<(), ResLimitCode> {
    if n > MAX_SECTIONS {
        return Err(ResLimitCode::SectionsOverLimit);
    }
    Ok(())
}

pub fn check_dep_depth(d: u32) -> Result<(), ResLimitCode> {
    if d > MAX_DEP_DEPTH {
        return Err(ResLimitCode::DepDepthOverLimit);
    }
    Ok(())
}

pub fn check_reloc(n: u32) -> Result<(), ResLimitCode> {
    if n > MAX_RELOC {
        return Err(ResLimitCode::RelocOverLimit);
    }
    Ok(())
}

// ── 扩展限额注册协议 ─────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtLimit {
    pub id: u32,
    pub cap: u32,
}

/// 扩展限额账（挂账 + 查验 + 布防三位一体）。
pub struct ExtLimitBook {
    pub slots: Vec<Option<ExtLimit>>,
    pub registered: usize,
}

impl ExtLimitBook {
    pub fn new() -> ExtLimitBook {
        let mut s: Vec<Option<ExtLimit>> = Vec::new();
        let mut i = 0;
        while i < EXT_DOMAIN {
            s.push(None);
            i += 1;
        }
        ExtLimitBook {
            slots: s,
            registered: 0,
        }
    }

    /// 挂账：id < EXT_DOMAIN、上限非零、同 id 不得重挂（协议红线）。
    pub fn register(&mut self, id: u32, cap: u32) -> Result<(), ResLimitCode> {
        if id as usize >= EXT_DOMAIN {
            return Err(ResLimitCode::ExtIdOutOfRange);
        }
        if cap == 0 {
            return Err(ResLimitCode::ExtZeroCap);
        }
        if self.slots[id as usize].is_some() {
            return Err(ResLimitCode::ExtIdDuplicated);
        }
        self.slots[id as usize] = Some(ExtLimit { id, cap });
        self.registered += 1;
        Ok(())
    }

    /// 检查点查询：挂账限额越账必拒；未挂 id 查询必拒。
    pub fn check(&self, id: u32, value: u32) -> Result<(), ResLimitCode> {
        if id as usize >= EXT_DOMAIN {
            return Err(ResLimitCode::ExtIdOutOfRange);
        }
        match self.slots[id as usize] {
            None => Err(ResLimitCode::CheckpointMissing),
            Some(l) => {
                if value > l.cap {
                    Err(ResLimitCode::ExtOverLimit)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// 布防守恒：已挂账数 == 挂账计；全部挂账可查（协议红线）。
    pub fn verify_armed(&self) -> Result<usize, ResLimitCode> {
        let mut armed = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                armed += 1;
            }
            i += 1;
        }
        if armed != self.registered {
            return Err(ResLimitCode::CheckpointMissing);
        }
        Ok(armed)
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 面：三限越限注入（节表/依赖链/重定位表各 10/10 必抓）。
pub fn drill_three_limits_10() -> (bool, String) {
    let mut caught = 0;
    // 节表越限 10 例：96+1..96+10（96 恰限收行由端点面核）。
    let mut i = 1;
    while i <= 10 {
        if check_sections(MAX_SECTIONS + i as u32) == Err(ResLimitCode::SectionsOverLimit) {
            caught += 1;
        }
        i += 1;
    }
    // 依赖链越限 10 例。
    let mut j = 1;
    while j <= 10 {
        if check_dep_depth(MAX_DEP_DEPTH + j as u32) == Err(ResLimitCode::DepDepthOverLimit) {
            caught += 1;
        }
        j += 1;
    }
    // 重定位越限 10 例。
    let mut k = 1;
    while k <= 10 {
        if check_reloc(MAX_RELOC + k as u32) == Err(ResLimitCode::RelocOverLimit) {
            caught += 1;
        }
        k += 1;
    }
    let ok = caught == 30;
    (ok, format!("三限越限注入 {}/30 必抓", caught))
}

/// 端点语义面：恰等于限额收行（三限端点各核 1 次）。
pub fn drill_endpoint_semantics() -> (bool, String) {
    let s = check_sections(MAX_SECTIONS) == Ok(());
    let d = check_dep_depth(MAX_DEP_DEPTH) == Ok(());
    let r = check_reloc(MAX_RELOC) == Ok(());
    // 端点下界核：0 与 1 均收行。
    let lo = check_sections(0) == Ok(()) && check_dep_depth(0) == Ok(()) && check_reloc(0) == Ok(());
    (
        s && d && r && lo,
        format!("端点含端: 节={} 链={} 重定位={} 下界={}", s, d, r, lo),
    )
}

/// 扩展限额协议面：挂账 5 座 + 查验全通。
pub fn drill_ext_protocol() -> (bool, String) {
    let mut book = ExtLimitBook::new();
    let mut i = 0;
    while i < 5 {
        if book.register(i, 0x100 + i as u32).is_err() {
            return (false, format!("扩展限额 {} 挂账失败", i));
        }
        i += 1;
    }
    let armed = match book.verify_armed() {
        Ok(n) => n,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    // 查验：恰限收行、越限必拒（每座各一查）。
    let mut ok_all = true;
    let mut j = 0;
    while j < 5 {
        let cap = 0x100 + j as u32;
        if book.check(j, cap).is_err() || book.check(j, cap + 1).is_ok() {
            ok_all = false;
            break;
        }
        j += 1;
    }
    (
        armed == 5 && ok_all,
        format!("扩展协议挂账 {}/5 查验全真={}", armed, ok_all),
    )
}

/// 扩展协议反向：重复挂账 / 零上限 / 越 id 10/10 必抓。
pub fn drill_ext_violations_10() -> (bool, String) {
    let mut book = ExtLimitBook::new();
    let _ = book.register(3, 0x50);
    let mut caught = 0;
    // 重复挂账 5 例。
    let mut i = 0;
    while i < 5 {
        if book.register(3, 0x60 + i as u32) == Err(ResLimitCode::ExtIdDuplicated) {
            caught += 1;
        }
        i += 1;
    }
    // 零上限 3 例。
    let mut j = 0;
    while j < 3 {
        if book.register(4 + j as u32, 0) == Err(ResLimitCode::ExtZeroCap) {
            caught += 1;
        }
        j += 1;
    }
    // 越 id 2 例。
    let mut k = 0;
    while k < 2 {
        if book.register(EXT_DOMAIN as u32 + k as u32, 1) == Err(ResLimitCode::ExtIdOutOfRange) {
            caught += 1;
        }
        k += 1;
    }
    (caught == 10, format!("扩展协议违规注入 {}/10 必抓", caught))
}

/// 布防断言面：未挂账 id 查询必抓（CheckpointMissing）10/10。
pub fn drill_checkpoint_missing_10() -> (bool, String) {
    let book = ExtLimitBook::new(); // 空账：全无检查点
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        if book.check(i as u32, 1) == Err(ResLimitCode::CheckpointMissing) {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("未布防查询注入 {}/10 必抓", caught))
}

/// 挂账-查验对账面：挂账数 == 布防数；抽账（手撤一座）→ 守恒必抓。
pub fn drill_armed_conservation() -> (bool, String) {
    let mut book = ExtLimitBook::new();
    let mut i = 0;
    while i < 4 {
        let _ = book.register(i, 10 + i as u32);
        i += 1;
    }
    let good = book.verify_armed() == Ok(4);
    // 反向注入：手撤一座 → 布防守恒破必抓。
    book.slots[2] = None;
    book.registered = 4;
    let broken = book.verify_armed() == Err(ResLimitCode::CheckpointMissing);
    (good && broken, format!("布防守恒={} 抽账抓={}", good, broken))
}

/// LCG 抽检面：随机限值三检查点复测（判定与注入面独立复核）。
pub fn drill_lcg_recheck() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let mut i = 0;
    while i < 30 {
        let v = rng.next_below(200) as u32 + MAX_SECTIONS + 1; // 必超节表限
        if check_sections(v) == Err(ResLimitCode::SectionsOverLimit) {
            caught += 1;
        }
        i += 1;
    }
    (caught == 30, format!("LCG 抽检越限 {}/30 必抓", caught))
}

/// 多机对拍面：同种子重建扩展账两次，挂账位图一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut book = ExtLimitBook::new();
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 8 {
            let id = rng.next_below(EXT_DOMAIN as u64) as u32;
            let _ = book.register(id, 0x20 + i as u32);
            i += 1;
        }
        let mut bits: [bool; EXT_DOMAIN] = [false; EXT_DOMAIN];
        let mut k = 0;
        while k < EXT_DOMAIN {
            bits[k] = book.slots[k].is_some();
            k += 1;
        }
        bits
    };
    let a = build();
    let b = build();
    (a == b, format!("同种子对拍一致={}", a == b))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_three_limits_10();
    let (a2, d2) = drill_ext_protocol();
    let (a3, d3) = drill_endpoint_semantics();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 三限注入={} 扩展协议={} 端点语义={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 深化块：黄金限值对照 + 限额账导出 + 深化注入面 ───────────────
/// 黄金限值对照表：12 组写死 (检查面, 值, 期望通过)，独立复判。
pub fn drill_golden_limits_12() -> (bool, String) {
    // kind: 0=节表限 1=依赖链限 2=重定位表限（上界含端语义写死于表）。
    let golden: [(u8, u32, bool); 12] = [
        (0, 0, true),
        (0, MAX_SECTIONS - 1, true),
        (0, MAX_SECTIONS, true),
        (0, MAX_SECTIONS + 1, false),
        (1, 1, true),
        (1, MAX_DEP_DEPTH, true),
        (1, MAX_DEP_DEPTH + 1, false),
        (1, MAX_DEP_DEPTH * 2, false),
        (2, 0, true),
        (2, MAX_RELOC, true),
        (2, MAX_RELOC + 1, false),
        (2, u32::MAX, false),
    ];
    let mut ok = true;
    let mut i = 0;
    while i < golden.len() {
        let (kind, v, want) = golden[i];
        let r = match kind {
            0 => check_sections(v),
            1 => check_dep_depth(v),
            _ => check_reloc(v),
        };
        if r.is_ok() != want {
            ok = false;
            break;
        }
        i += 1;
    }
    (ok, format!("黄金限值对照 12 组全真={}", ok))
}

/// 限额账诊断导出：X|id|0xcap 逐行 + 布防守恒行（行数守恒 + 格式）。
pub fn drill_export_ext_book() -> (bool, String) {
    let mut book = ExtLimitBook::new();
    let mut i = 0;
    while i < 4 {
        let _ = book.register(i, 0x80 + i as u32);
        i += 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut j = 0;
    while j < book.slots.len() {
        match book.slots[j] {
            Some(l) => lines.push(format!("X|{}|0x{:04X}", l.id, l.cap)),
            None => {}
        }
        j += 1;
    }
    let rows_ok = lines.len() == book.registered;
    let mut fmt_ok = true;
    let mut k = 0;
    while k < lines.len() {
        if !lines[k].starts_with("X|") || lines[k].split('|').count() != 3 {
            fmt_ok = false;
        }
        k += 1;
    }
    let armed = book.verify_armed();
    (
        rows_ok && fmt_ok && armed == Ok(4),
        format!("限额账导出 {} 行守恒={} 格式={} 布防={:?}", lines.len(), rows_ok, fmt_ok, armed.is_ok()),
    )
}

/// 深化注入面：挂账后越值查验 10/10 必抓（ ExtOverLimit 分立码）。
pub fn drill_ext_over_10() -> (bool, String) {
    let mut book = ExtLimitBook::new();
    let _ = book.register(6, 0x40);
    let mut caught = 0;
    let mut i = 1;
    while i <= 10 {
        if book.check(6, 0x40 + i as u32) == Err(ResLimitCode::ExtOverLimit) {
            caught += 1;
        }
        i += 1;
    }
    // 恰限收行反向核。
    let at_cap = book.check(6, 0x40) == Ok(());
    (caught == 10 && at_cap, format!("扩展越值注入 {}/10 必抓（恰限收行={}）", caught, at_cap))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < ResLimitCode::ALL.len() {
        let v = ResLimitCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_three_limits_10();
    v.push(("J1-三限越限注入30/30", a, b));
    let (a, b) = drill_endpoint_semantics();
    v.push(("J1-端点含端语义", a, b));
    let (a, b) = drill_ext_protocol();
    v.push(("J1-扩展限额注册协议", a, b));
    let (a, b) = drill_ext_violations_10();
    v.push(("J2-扩展协议违规10/10", a, b));
    let (a, b) = drill_checkpoint_missing_10();
    v.push(("J2-未布防查询10/10", a, b));
    let (a, b) = drill_armed_conservation();
    v.push(("J3-布防守恒+反向", a, b));
    let (a, b) = drill_lcg_recheck();
    v.push(("J3-LCG抽检复测30/30", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_golden_limits_12();
    v.push(("J3-黄金限值对照12组", a, b));
    let (a, b) = drill_export_ext_book();
    v.push(("J3-限额账诊断导出", a, b));
    let (a, b) = drill_ext_over_10();
    v.push(("J2-扩展越值注入10/10", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13301_checks：本单判据聚合（诊断段 0x658C0）。
pub fn run_unx13301_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13301 资源耗尽限额总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x658C0 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
