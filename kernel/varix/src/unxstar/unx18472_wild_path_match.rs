//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18472 · 通配路径与精确路径匹配 · 0x7F1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18472-J1 通配（*\setup.exe 类）匹配语义与 Windows 对照 10 例零偏差
//!
//! 设计要点：
//!   - 匹配语义对齐 Windows PathMatchSpec 家族：`*` 吞任意序列（含空）、
//!     `?` 恰吞一个字符、比较前 ASCII 大小写折叠（仅 A-Z↔a-z，非 ASCII
//!     字节不折叠——与 Windows 组件比较一致）；
//!   - 无通配符的模式走「精确路径」通道：大小写不敏感的整串相等；
//!   - 对照表：10 例（模式,路径,期望）三元组写死，来自 Windows 语义
//!     手工推导（后缀陷阱/大小写陷阱/点号陷阱各占位），逐例复算
//!     必须零偏差；
//!   - 匹配器用双指针 + 单星回溯（贪心回溯经典式），无递归无浮点；
//!   - 边界：空模式、空路径、模式长于路径、路径长于模式、纯星模式、
//!     超长（>64）一律显式裁决，不 panic；
//!   - 注入核：三类坏样本——后缀陷阱错判、大小写陷阱错判、`?` 宽度
//!     错判——各 10 轮，逐轮断言裁决器显式拒绝（10/10 必抓）；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 对照裁决表 W10（10 例三元组冻结）+ MatchLedger 逐例账本
//!   [状态机]   匹配器双指针回溯态（pi/ti/star_p/star_t 四游标单调推进）
//!   [账本]     MatchLedger：逐例裁决字节数组 + 偏差计数 + 运行计数
//!   [错误路径] PmCode 9 码闭集，code() = SEG_BASE + n
//!   [注入核]   三类错判样本 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F1C0，AI-VX11-1105 批04 独占区，A08 工单卡二）。
pub const SEG_BASE: u32 = 0x7F1C0;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 对照例数（冻结：10，J1 原文锚定）。
pub const EXAMPLE_N: usize = 10;
/// 模式长度上限（冻结：64；组件级比较域，超长即拒）。
pub const PATTERN_LEN_MAX: usize = 64;
/// 路径长度上限（冻结：64）。
pub const PATH_LEN_MAX: usize = 64;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// ASCII 大小写折叠：仅 A-Z 降到 a-z，其余原样。
/// 为什么只折叠 ASCII：Windows 文件组件比较按 upcase 表逐码位折，
/// 兼容层第一版收敛到 ASCII 域，多字节原样字节比较。
pub fn fold_ascii(c: u8) -> u8 {
    if c >= b'A' && c <= b'Z' {
        c + 32
    } else {
        c
    }
}

/// 模式是否含通配符（`*` 或 `?`）。
pub fn has_wildcard(p: &[u8]) -> bool {
    let mut i = 0;
    while i < p.len() {
        if let Some(&c) = p.get(i) {
            if c == b'*' || c == b'?' {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 通配匹配器（双指针 + 单星回溯） ────────────────────────────
/// 通配匹配：`*` 吞任意序列含空，`?` 恰吞一个。回溯只记最近一次
/// 星位（贪心回溯），与 Windows 逐字符语义在组件域等价。
/// 无 panic：下标全部走 len 守卫。
pub fn glob_match(pattern: &[u8], path: &[u8]) -> bool {
    let mut pi = 0usize;
    let mut ti = 0usize;
    // 最近一次星位（模式游标）与星吞起点（路径游标）。
    let mut star_p: Option<usize> = None;
    let mut star_t = 0usize;
    while ti < path.len() {
        let pc: Option<u8> = if pi < pattern.len() {
            pattern.get(pi).map(|c| fold_ascii(*c))
        } else {
            None
        };
        let tc = match path.get(ti) {
            Some(c) => fold_ascii(*c),
            None => break,
        };
        match pc {
            Some(b'*') => {
                // 星位入账：当前 ti 即星吞起点（含空语义从 ti 开始试）。
                star_p = Some(pi);
                star_t = ti;
                pi += 1;
            }
            Some(c) if c == tc || c == b'?' => {
                pi += 1;
                ti += 1;
            }
            _ => {
                match star_p {
                    Some(sp) => {
                        // 回溯：星多吞一个字符，模式回星后一位重试。
                        star_t = star_t.saturating_add(1);
                        ti = star_t;
                        pi = sp + 1;
                    }
                    None => return false,
                }
            }
        }
    }
    // 路径耗尽：模式尾部残余必须全是星。
    while pi < pattern.len() {
        match pattern.get(pi) {
            Some(b'*') => pi += 1,
            _ => return false,
        }
    }
    true
}

// ── 精确路径通道 ───────────────────────────────────────────────
/// 精确匹配：无通配前提下的折叠全等。含通配的模式禁止走本通道
/// （调用方先 has_wildcard 分流），误入返回 false 由调用方兜底。
pub fn exact_match(pattern: &[u8], path: &[u8]) -> bool {
    if has_wildcard(pattern) {
        return false;
    }
    if pattern.len() != path.len() {
        return false;
    }
    let mut i = 0;
    while i < pattern.len() {
        let a = match pattern.get(i) {
            Some(c) => fold_ascii(*c),
            None => return false,
        };
        let b = match path.get(i) {
            Some(c) => fold_ascii(*c),
            None => return false,
        };
        if a != b {
            return false;
        }
        i += 1;
    }
    true
}

/// 统一入口：按模式是否含通配分流。超长一律 false（不判真）。
pub fn path_match(pattern: &[u8], path: &[u8]) -> bool {
    if pattern.len() == 0 || pattern.len() > PATTERN_LEN_MAX {
        return false;
    }
    if path.len() > PATH_LEN_MAX {
        return false;
    }
    if has_wildcard(pattern) {
        glob_match(pattern, path)
    } else {
        exact_match(pattern, path)
    }
}

// ── 对照裁决表（Windows 语义手工推导，冻结） ───────────────────
/// 单例：模式 / 路径 / 期望（true=应匹配）。
#[derive(Clone, Copy)]
pub struct Example {
    pub pattern: &'static str,
    pub path: &'static str,
    pub expect: bool,
}

/// 10 例对照（推导批注随行）：
///   E0 `*.exe` 吞 setup           —— 基础星；
///   E1 后缀陷阱：.exe.bak 不许被 *.exe 吞（尾残余非星）；
///   E2 星中缀：setup*.exe 吞 setup.exe（星吞空）；
///   E3 问号恰一：setup?.exe 吞 setupa.exe；
///   E4 问号恰一：不吞两位 setupab.exe；
///   E5 双星夹中缀：*setup* 吞 my_setup_tool.exe；
///   E6 模式大写折叠：*.EXE 吞 app.exe；
///   E7 精确通道折叠：setup.exe 吞 SETUP.EXE；
///   E8 精确通道整串：setup 不许吞 setupx（前缀不等于全等）；
///   E9 点号陷阱：*.* 不许吞无点 readme（星回溯穷尽后尾点无着落）。
pub const W10: [Example; EXAMPLE_N] = [
    Example { pattern: "*.exe", path: "setup.exe", expect: true },
    Example { pattern: "*.exe", path: "setup.exe.bak", expect: false },
    Example { pattern: "setup*.exe", path: "setup.exe", expect: true },
    Example { pattern: "setup?.exe", path: "setupa.exe", expect: true },
    Example { pattern: "setup?.exe", path: "setupab.exe", expect: false },
    Example { pattern: "*setup*", path: "my_setup_tool.exe", expect: true },
    Example { pattern: "*.EXE", path: "app.exe", expect: true },
    Example { pattern: "setup.exe", path: "SETUP.EXE", expect: true },
    Example { pattern: "setup", path: "setupx", expect: false },
    Example { pattern: "*.*", path: "readme", expect: false },
];

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCode {
    /// 模式超长（>64）
    PatternTooLong,
    /// 路径超长（>64）
    PathTooLong,
    /// 对照例下标越界
    ExampleIndexOOB,
    /// 对照偏差（引擎裁决 ≠ 冻结期望）
    LedgerDeviation,
    /// 注入错判未被拒（漏检）
    WrongClaimPassed,
    /// 守恒破坏（注入 ≠ 检出 + 漏检）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
    /// 精确通道误收通配模式
    ExactTookWildcard,
}

impl PmCode {
    /// 诊断码 = SEG_BASE + 枚举序号（段内 0x40 窗口）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PmCode::PatternTooLong => "模式超长：64 上限，拆分组件再配",
            PmCode::PathTooLong => "路径超长：64 上限，查组件规范化",
            PmCode::ExampleIndexOOB => "对照例下标越界：查 W10 闭集",
            PmCode::LedgerDeviation => "对照偏差：引擎裁决与 Windows 期望不符，零容忍",
            PmCode::WrongClaimPassed => "错判漏检：注入的坏断言未被拒，裁决器失效",
            PmCode::ConservationBroken => "守恒破坏：注入≠检出+漏检",
            PmCode::NotReproducible => "双跑不一致：复现红线",
            PmCode::SegmentOutside => "段位越窗：诊断码不在 0x7F1C0 段",
            PmCode::ExactTookWildcard => "精确通道误收通配：分流闸失效",
        }
    }
    pub const ALL: [PmCode; 9] = [
        PmCode::PatternTooLong,
        PmCode::PathTooLong,
        PmCode::ExampleIndexOOB,
        PmCode::LedgerDeviation,
        PmCode::WrongClaimPassed,
        PmCode::ConservationBroken,
        PmCode::NotReproducible,
        PmCode::SegmentOutside,
        PmCode::ExactTookWildcard,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < PmCode::ALL.len() {
        if let Some(c) = PmCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F1C0 段闭集内"
}

// ── 逐例账本 ───────────────────────────────────────────────────
/// 逐例裁决账：0=未跑 1=匹配 2=不匹配；偏差计数恒 0 才收口。
#[derive(Clone, Copy, Debug)]
pub struct MatchLedger {
    pub verdicts: [u8; EXAMPLE_N],
    pub deviations: u32,
    pub ran: u32,
}

impl MatchLedger {
    pub const fn new() -> MatchLedger {
        MatchLedger { verdicts: [0; EXAMPLE_N], deviations: 0, ran: 0 }
    }
    /// 记一例：slot 越界静默不入账（边界收敛）。
    pub fn record(&mut self, slot: usize, matched: bool) {
        if let Some(v) = self.verdicts.get_mut(slot) {
            *v = if matched { 1 } else { 2 };
            self.ran = self.ran.saturating_add(1);
        }
    }
    /// 与冻结期望对表：期望与本账逐例一致才算收口。
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < EXAMPLE_N {
            let expect = match W10.get(i) {
                Some(e) => e.expect,
                None => return Err(PmCode::ExampleIndexOOB.code()),
            };
            let v = match self.verdicts.get(i) {
                Some(0) => return Err(PmCode::LedgerDeviation.code()),
                Some(1) => true,
                Some(2) => false,
                _ => return Err(PmCode::LedgerDeviation.code()),
            };
            if v != expect {
                return Err(PmCode::LedgerDeviation.code());
            }
            i += 1;
        }
        if self.ran != EXAMPLE_N as u32 {
            return Err(PmCode::LedgerDeviation.code());
        }
        Ok(())
    }
}

// ── 黄金对照：10 例逐例复算 ────────────────────────────────────
/// 跑满 10 例并落账。返回（匹配数, 不匹配数）。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = MatchLedger::new();
    let mut hits = 0u32;
    let mut misses = 0u32;
    let mut i = 0;
    while i < EXAMPLE_N {
        let e = match W10.get(i) {
            Some(e) => *e,
            None => return Err(PmCode::ExampleIndexOOB.code()),
        };
        let m = path_match(e.pattern.as_bytes(), e.path.as_bytes());
        if m != e.expect {
            return Err(PmCode::LedgerDeviation.code());
        }
        led.record(i, m);
        if m {
            hits = hits.saturating_add(1);
        } else {
            misses = misses.saturating_add(1);
        }
        i += 1;
    }
    led.audit()?;
    // 冻结推导：命中例 E0/E2/E3/E5/E6/E7 = 6；不命中 E1/E4/E8/E9 = 4。
    if hits != 6 || misses != 4 {
        return Err(PmCode::LedgerDeviation.code());
    }
    Ok((hits, misses))
}

// ── 边界账 ─────────────────────────────────────────────────────
/// 边界五况显式裁决（空模式/空路径/纯星/模式长于路径/超长）。
pub fn edge_verdicts() -> Result<[bool; 5], u32> {
    // 空模式 vs 空路径：模式长度 0 直接 false（显式裁决：空模式非法）。
    let e0 = path_match(b"", b"");
    // 纯星 vs 空路径：星含空语义 → true。
    let e1 = glob_match(b"*", b"");
    // 模式长于路径且无星：false。
    let e2 = path_match(b"setup.exe", b"setup");
    // 纯问号 vs 单字符：true。
    let e3 = glob_match(b"?", b"a");
    // 超长模式：false（显式拒，不判真）。
    let mut long = Vec::new();
    let mut q = 0;
    while q <= PATTERN_LEN_MAX {
        long.push(b'a');
        q += 1;
    }
    let e4 = path_match(&long, b"a");
    Ok([e0, e1, e2, e3, e4])
}

// ── 注入核：三类错判样本 ×10 轮 ────────────────────────────────
/// 注入语义：坏断言 =（模式, 路径, 错误期望）。裁决器以引擎真值
/// 对表，错误期望必须被逐轮拒绝（claimed ≠ truth → 拒）。
///   类一 后缀陷阱错判：声称 *.exe 匹配 .exe.bak（真值 false）；
///   类二 大小写陷阱错判：声称 SETUP.EXE 不匹配 setup.exe（真值 true）；
///   类三 问号宽度错判：声称 setup?.exe 匹配 setupab.exe（真值 false）。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // 类一：后缀陷阱坏断言（声称匹配）。
        tries = tries.saturating_add(1);
        {
            let truth = path_match(b"*.exe", b"setup.exe.bak");
            let claimed = true;
            if truth != claimed {
                caught = caught.saturating_add(1);
            }
        }
        // 类一补：反向坏断言（声称不匹配 *.exe / setup.exe）。
        tries = tries.saturating_add(1);
        {
            let truth = path_match(b"*.exe", b"setup.exe");
            let claimed = false;
            if truth != claimed {
                caught = caught.saturating_add(1);
            }
        }
        // 类二：大小写陷阱坏断言（声称大小写敏感拒配）。
        tries = tries.saturating_add(1);
        {
            let truth = exact_match(b"setup.exe", b"SETUP.EXE");
            let claimed = false;
            if truth != claimed {
                caught = caught.saturating_add(1);
            }
        }
        // 类三：问号宽度坏断言（声称 ? 吞两位）。
        tries = tries.saturating_add(1);
        {
            let truth = path_match(b"setup?.exe", b"setupab.exe");
            let claimed = true;
            if truth != claimed {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(PmCode::WrongClaimPassed.code());
    }
    Ok((tries, caught))
}

/// 分流闸：精确通道禁止吞通配模式（内部闸，注入第四类）。
pub fn inject_route_guard(rounds: u32) -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        if !exact_match(b"*.exe", b"setup.exe") {
            caught = caught.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    if caught != rounds {
        return Err(PmCode::ExactTookWildcard.code());
    }
    Ok(caught)
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PmCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PmCode::ALL.len() {
        if let Some(c) = PmCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18472 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18472_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18472");
    // 1. 对照表冻结：10 例。
    s.add("J1-对照表10例", EXAMPLE_N == 10 && W10.len() == EXAMPLE_N, "Windows 对照闭集冻结");
    // 2. 逐例复算零偏差（黄金对照）。
    let g = golden_run();
    let g_txt = format!("命中/不命中={:?}", g);
    s.add_dyn("J1-10例零偏差", g.is_ok(), &g_txt);
    // 3. 命中分布冻结：6 匹配 4 不匹配。
    s.add(
        "J1-命中分布6/4",
        match g {
            Ok((h, m)) => h == 6 && m == 4,
            Err(_) => false,
        },
        "推导冻结：E0/E2/E3/E5/E6/E7 中，E1/E4/E8/E9 不中",
    );
    // 4. 边界五况。
    {
        let ev = edge_verdicts();
        let ok = match ev {
            Ok(v) => {
                // e0 空模式 false；e1 纯星对空 true；e2 长模式短路径 false；
                // e3 ? 对单字符 true；e4 超长 false。
                v.get(0) == Some(&false)
                    && v.get(1) == Some(&true)
                    && v.get(2) == Some(&false)
                    && v.get(3) == Some(&true)
                    && v.get(4) == Some(&false)
            }
            Err(_) => false,
        };
        s.add("J1-边界五况", ok, "空模式/纯星/长短/问号/超长显式裁决");
    }
    // 5. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入三类10/10",
        inj_ok,
        &format!("tries/caught={:?}", inj),
    );
    // 6. 分流闸：精确通道拒通配 10 轮全抓。
    let rg = inject_route_guard(INJECT_ROUNDS);
    s.add(
        "J1-分流闸",
        rg.is_ok(),
        "exact_match 拒收通配模式 10/10",
    );
    // 7. 账本收口：audit 与冻结期望逐例一致。
    {
        let led = MatchLedger::new();
        let mut led = led;
        let mut i = 0;
        while i < EXAMPLE_N {
            if let Some(e) = W10.get(i) {
                let m = path_match(e.pattern.as_bytes(), e.path.as_bytes());
                led.record(i, m);
            }
            i += 1;
        }
        s.add("J1-账本收口", led.audit().is_ok() && led.deviations == 0, "逐例账与期望一致");
    }
    // 8. 折叠域收敛：非 ASCII 字节不折叠（0xE5 原样比较）。
    {
        let a = [0xE5u8];
        let b = [0xC5u8];
        s.add(
            "J1-折叠域收敛",
            !exact_match(&a, &b) && fold_ascii(0xE5) == 0xE5,
            "仅 A-Z 折叠，高位字节原样",
        );
    }
    // 9. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 10. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F1C0 窗口对齐 0x40");
    // 11. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < PmCode::ALL.len() {
            if let Some(c) = PmCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "9 码中文明细全非空");
    }
    // 12. 通配语义单元：? 恰一、* 含空、星不回吞已确认字符。
    {
        let q1 = glob_match(b"?", b"a") && !glob_match(b"?", b"ab") && !glob_match(b"?", b"");
        let s1 = glob_match(b"*", b"") && glob_match(b"*", b"abcdef");
        let m1 = glob_match(b"a*b*c", b"axxbxc") && !glob_match(b"a*b*c", b"axbxa");
        s.add("J1-通配三语义", q1 && s1 && m1, "?恰一/*含空/回溯不假阳");
    }
    // 13. 精确语义单元：长度不等拒、逐字节折叠相等收。
    {
        let e1 = exact_match(b"abc", b"abc") && exact_match(b"ABC", b"abc");
        let e2 = !exact_match(b"abc", b"abcd") && !exact_match(b"abd", b"abc");
        s.add("J1-精确两语义", e1 && e2, "长度闸+折叠全等");
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        assert!(glob_match(b"*.exe", b"setup.exe"));
        assert!(exact_match(b"setup.exe", b"SETUP.EXE"));
    }

    #[test]
    fn t02_boundary_empty() {
        assert!(!path_match(b"", b""));
        assert!(glob_match(b"*", b""));
        assert!(!glob_match(b"?", b""));
        assert!(glob_match(b"?", b"a"));
    }

    #[test]
    fn t03_boundary_length() {
        let mut long = Vec::new();
        let mut q = 0;
        while q <= PATTERN_LEN_MAX {
            long.push(b'a');
            q += 1;
        }
        assert!(!path_match(&long, b"a"));
        assert!(!path_match(b"setup.exe", b"setup"));
    }

    #[test]
    fn t04_inject_suffix_trap() {
        assert!(!glob_match(b"*.exe", b"setup.exe.bak"));
        assert!(glob_match(b"*.exe", b"setup.exe"));
    }

    #[test]
    fn t05_inject_case_trap() {
        assert!(exact_match(b"setup.exe", b"SETUP.EXE"));
        assert!(!exact_match(b"setup.exe", b"setup.exe "));
    }

    #[test]
    fn t06_inject_width_trap() {
        assert!(glob_match(b"setup?.exe", b"setupa.exe"));
        assert!(!glob_match(b"setup?.exe", b"setupab.exe"));
    }

    #[test]
    fn t07_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t08_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t09_checks_all_green() {
        let set = run_unx18472_checks();
        assert!(set.all_passed(), "UNX-F18472 自检须全绿：{}", set.red_items_text());
    }
}
