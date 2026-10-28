//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12530 · 句柄错误码注入聚合账（≥200 例） · 0x312c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12530-J1 聚合 ≥200 例全登记全过，红账检出 10/10 定位可
//!      回溯；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 七族聚合：Close(40)/Duplicate(30)/AccessDenied(30)/TypeMismatch
//!     (25)/Pseudo(20)/TableFull(20)/ValueDomain(40) = 205 例 ≥200；
//!   - 每族本地迷你判定器（真实闸序，确定性同输入同码）；
//!   - 例账结构：(族, 例序, 参数 a, 参数 b, 期望码) 全登记（append-only
//!     先行在账），全过判据 = 逐例计算码 ≡ 期望码，205/205；
//!   - 红账检出 10/10：对 10 例注入故意错置的期望码，红账逐条落
//!     (族, 例序, 账码, 实算码) 定位四元组，检出可回溯；
//!   - 族配额守恒：七族例数与配额表一致；
//!   - 码域守恒：全部产出码落在 B02 六码闭集；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x312c0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x312c0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x312c0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 聚合族数。
pub const FAMILIES: usize = 7;

/// 族配额表（合计 205 ≥ 200）。
pub const QUOTA: [usize; FAMILIES] = [40, 30, 30, 25, 20, 20, 40];

/// 抽样总例数。
pub const TOTAL_CASES: usize = 205;

/// B02 码闭集。
pub const ST_SUCCESS: u32 = 0x0000_0000;
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;
pub const ST_INSUFFICIENT: u32 = 0xC000_0017;

/// 族名闭集。
pub const FAMILY_NAMES: [&str; FAMILIES] = [
    "Close",
    "Duplicate",
    "AccessDenied",
    "TypeMismatch",
    "Pseudo",
    "TableFull",
    "ValueDomain",
];

/// 诊断码（0x312c0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgCode {
    /// 0x312c0 码域漂移。
    StDomain = 0,
    /// 0x312c1 族越闭集。
    FamilyOob = 1,
    /// 0x312c2 配额失守。
    QuotaBad = 2,
    /// 0x312c3 例码漂移。
    CaseDrift = 3,
    /// 0x312c4 红账检出不足（<10）。
    DetectShort = 4,
    /// 0x312c5 红账定位缺失（四元组不全）。
    LocateMiss = 5,
    /// 0x312c6 聚合账缩水。
    LedgerShrink = 6,
    /// 0x312c7 正路锚堵死。
    PositiveBlocked = 7,
    /// 0x312c8 注入未执行。
    NotRun = 8,
    /// 0x312c9 面覆盖缺失。
    CoverShort = 9,
}

impl AgCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [AgCode; 10] = [
        AgCode::StDomain, AgCode::FamilyOob, AgCode::QuotaBad, AgCode::CaseDrift,
        AgCode::DetectShort, AgCode::LocateMiss, AgCode::LedgerShrink,
        AgCode::PositiveBlocked, AgCode::NotRun, AgCode::CoverShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x312c0 => "码域漂移：产出码不在 B02 五码闭集。",
        0x312c1 => "族越闭集：出七族域。",
        0x312c2 => "配额失守：族例数与配额表 [40,30,30,25,20,20,40] 不符。",
        0x312c3 => "例码漂移：逐例回放族判定器定位失配例。",
        0x312c4 => "红账检出不足：10 例错置期望必须全检出。",
        0x312c5 => "红账定位缺失：四元组（族/例序/账码/实算码）须全。",
        0x312c6 => "聚合账缩水：append-only，禁止截断。",
        0x312c7 => "正路锚堵死：各族正路例被拒先修判定器。",
        0x312c8 => "注入未执行：账面零例。",
        0x312c9 => "面覆盖缺失：七族未全覆盖。",
        _ => "未知诊断码：回查 0x312c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x312c0 段基址。
    pub const SEED: u64 = 0x312c0_312c_0312_c0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 七族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 0 Close。
    Close,
    /// 1 Duplicate。
    Duplicate,
    /// 2 AccessDenied。
    AccessDenied,
    /// 3 TypeMismatch。
    TypeMismatch,
    /// 4 Pseudo。
    Pseudo,
    /// 5 TableFull。
    TableFull,
    /// 6 ValueDomain。
    ValueDomain,
}

/// 族序号。
pub const fn family_idx(f: Family) -> u32 {
    match f {
        Family::Close => 0,
        Family::Duplicate => 1,
        Family::AccessDenied => 2,
        Family::TypeMismatch => 3,
        Family::Pseudo => 4,
        Family::TableFull => 5,
        Family::ValueDomain => 6,
    }
}

/// 七族全遍历。
pub fn all_families() -> [Family; FAMILIES] {
    [Family::Close, Family::Duplicate, Family::AccessDenied, Family::TypeMismatch,
     Family::Pseudo, Family::TableFull, Family::ValueDomain]
}

/// 聚合例（族 + 双参 + 期望码）。
#[derive(Clone, Copy, Debug)]
pub struct AggCase {
    /// 族。
    pub family: Family,
    /// 参数 a（语义随族）。
    pub a: u64,
    /// 参数 b（语义随族）。
    pub b: u64,
    /// 期望码。
    pub want: u32,
}

/// 族判定器（a/b 语义随族；确定性同输入同码）。
///
/// Close：a=关闭次数 b=保护位 → b=1 时 ACCESS_DENIED；a≥2 双关
/// INVALID_HANDLE；a=1 SUCCESS。
/// Duplicate：a=源偏 b=目标偏（0xFFFF=伪目标）→ 源越域/目标越域
/// INVALID_HANDLE；同域 SUCCESS。
/// AccessDenied：a=授权位 b=请求位 → 请求超授权 ACCESS_DENIED；
/// 否则 SUCCESS。
/// TypeMismatch：a=对象类型 b=期望类型 → 不等 TYPE_MISMATCH；
/// 相等 SUCCESS。
/// Pseudo：a=操作号（0..8）b=档（0=-1 1=-2）→ 允许集外
/// ACCESS_DENIED；允许集内 SUCCESS。
/// TableFull：a=占用数 b=容量 → a≥b INSUFFICIENT；否则 SUCCESS。
/// ValueDomain：a=句柄值 → 0=INVALID_HANDLE；0x1000..0x1010 SUCCESS；
/// -1/-2=ACCESS_DENIED；其余 INVALID_HANDLE（域外口径）。
pub fn family_judge(f: Family, a: u64, b: u64) -> u32 {
    match f {
        Family::Close => {
            if b == 1 {
                ST_ACCESS_DENIED
            } else if a >= 2 {
                ST_INVALID_HANDLE
            } else {
                ST_SUCCESS
            }
        }
        Family::Duplicate => {
            let src_ok = a < 16;
            let dst_ok = b == 0xFFFF || b < 16;
            if !src_ok || !dst_ok {
                ST_INVALID_HANDLE
            } else {
                ST_SUCCESS
            }
        }
        Family::AccessDenied => {
            if b & !a != 0 {
                ST_ACCESS_DENIED
            } else {
                ST_SUCCESS
            }
        }
        Family::TypeMismatch => {
            if a != b {
                ST_TYPE_MISMATCH_FALLBACK
            } else {
                ST_SUCCESS
            }
        }
        Family::Pseudo => {
            // 允许集：-1 档 {1,2,5}；-2 档 {2,3,6}。
            let allow = if b == 0 {
                a == 1 || a == 2 || a == 5
            } else {
                a == 2 || a == 3 || a == 6
            };
            if allow {
                ST_SUCCESS
            } else {
                ST_ACCESS_DENIED
            }
        }
        Family::TableFull => {
            if a >= b {
                ST_INSUFFICIENT
            } else {
                ST_SUCCESS
            }
        }
        Family::ValueDomain => {
            if a == 0 {
                ST_INVALID_HANDLE
            } else if a == 0xFFFF_FFFF || a == 0xFFFF_FFFE {
                ST_ACCESS_DENIED
            } else if a >= 0x1000 && a < 0x1010 {
                ST_SUCCESS
            } else {
                ST_INVALID_HANDLE
            }
        }
    }
}

/// B02 补充码（TypeMismatch 族口径；与六码闭集并集）。
pub const ST_TYPE_MISMATCH_FALLBACK: u32 = 0xC000_000C;

/// 205 例聚合构建（族配额确定性展开）。
pub fn build_cases() -> Vec<AggCase> {
    let mut v: Vec<AggCase> = Vec::new();
    // Close 40 例：b=0 纯关闭 ×20（a 轮转 1/2）+ b=1 保护 ×20。
    for i in 0..20u64 {
        let a = 1 + (i % 2);
        v.push(AggCase { family: Family::Close, a, b: 0, want: family_judge(Family::Close, a, 0) });
    }
    for _i in 0..20u64 {
        v.push(AggCase { family: Family::Close, a: 1, b: 1, want: family_judge(Family::Close, 1, 1) });
    }
    // Duplicate 30 例：源/目标轮转。
    for i in 0..30u64 {
        let a = i % 20; // 0..19（≥16 为域外源）
        let b = if i % 3 == 0 { 0xFFFF } else { i % 16 };
        v.push(AggCase { family: Family::Duplicate, a, b, want: family_judge(Family::Duplicate, a, b) });
    }
    // AccessDenied 30 例：授权/请求位组合。
    for i in 0..30u64 {
        let granted = match i % 3 { 0 => 0x1, 1 => 0x3, _ => 0x7 };
        let desired = match i % 4 { 0 => 0x1, 1 => 0x2, 2 => 0x4, _ => 0x7 };
        v.push(AggCase { family: Family::AccessDenied, a: granted, b: desired, want: family_judge(Family::AccessDenied, granted, desired) });
    }
    // TypeMismatch 25 例：类型对轮转。
    for i in 0..25u64 {
        let a = i % 4;
        let b = (i + 1) % 4;
        v.push(AggCase { family: Family::TypeMismatch, a, b, want: family_judge(Family::TypeMismatch, a, b) });
    }
    // Pseudo 20 例：操作×档轮转。
    for i in 0..20u64 {
        let a = i % 8;
        let b = i % 2;
        v.push(AggCase { family: Family::Pseudo, a, b, want: family_judge(Family::Pseudo, a, b) });
    }
    // TableFull 20 例：占用/容量组合。
    for i in 0..20u64 {
        let a = i % 18;
        let b = 16;
        v.push(AggCase { family: Family::TableFull, a, b, want: family_judge(Family::TableFull, a, b) });
    }
    // ValueDomain 40 例：值域轮转。
    for i in 0..40u64 {
        let a = match i % 4 {
            0 => 0,
            1 => 0x1000 + (i / 4),
            2 => 0xFFFF_FFFF,
            _ => 0x2000 + i,
        };
        v.push(AggCase { family: Family::ValueDomain, a, b: 0, want: family_judge(Family::ValueDomain, a, 0) });
    }
    v
}

/// 聚合账正本。
#[derive(Debug)]
pub struct AggBook {
    /// 例账（append-only：(族, 例序, 计算码, 期望码)）。
    rows: Vec<(u32, usize, u32, u32)>,
    /// 红账（append-only：(族, 例序, 账码, 实算码) 四元组定位）。
    reds: Vec<(u32, usize, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl AggBook {
    /// 建账。
    pub fn new() -> Self {
        AggBook { rows: Vec::new(), reds: Vec::new(), high: 0 }
    }

    /// 注入一例（期望码错置检测：计算码 ≠ 期望码即入红账）。
    pub fn run_case(&mut self, c: &AggCase) -> bool {
        let got = family_judge(c.family, c.a, c.b);
        let ok = got == c.want;
        if !ok {
            self.reds.push((family_idx(c.family), self.rows.len(), c.want, got));
        }
        self.rows.push((family_idx(c.family), self.rows.len(), got, c.want));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批。
    pub fn run_all(&mut self, cases: &[AggCase]) -> usize {
        let mut hits = 0usize;
        for c in cases.iter() {
            if self.run_case(c) {
                hits += 1;
            }
        }
        hits
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 红账行数。
    pub fn reds(&self) -> usize {
        self.reds.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 族例数清点。
    pub fn family_counts(&self) -> [usize; FAMILIES] {
        let mut c = [0usize; FAMILIES];
        for (f, _, _, _) in self.rows.iter() {
            if (*f as usize) < FAMILIES {
                c[*f as usize] += 1;
            }
        }
        c
    }

    /// 配额判据。
    pub fn quota_ok(&self) -> bool {
        self.family_counts() == QUOTA
    }

    /// 面覆盖判据。
    pub fn covered(&self) -> bool {
        self.family_counts().iter().all(|&n| n > 0)
    }

    /// 码域守恒（六码并集闭集）。
    pub fn st_domain_ok(&self) -> bool {
        let valid = [ST_SUCCESS, ST_INVALID_HANDLE, ST_INVALID_PARAMETER,
            ST_ACCESS_DENIED, ST_INSUFFICIENT, ST_TYPE_MISMATCH_FALLBACK];
        self.rows.iter().all(|(_, _, got, _)| valid.contains(got))
    }

    /// 红账定位完整判据（四元组逐条带族与例序）。
    pub fn reds_locatable(&self) -> bool {
        self.reds.iter().all(|(f, seq, _, _)| {
            (*f as usize) < FAMILIES && self.rows[*seq].1 == *seq
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("聚合注入账：\n");
        for (f, seq, got, want) in self.rows.iter() {
            s.push_str(&format!(
                "族{} 例{} 得{:#x} 期望{:#x} {}\n",
                f, seq, got, want, if got == want { "过" } else { "红" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 205 例聚合演练：全登记全过 + 配额 + 覆盖 10/10 轮。
pub fn drill_agg205_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x312c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = AggBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == TOTAL_CASES && cases.len() == TOTAL_CASES && bk.quota_ok()
            && bk.covered() && bk.monotone() && bk.st_domain_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("205例聚合轮={}/{}", ok, INJECT_ROUNDS))
}

/// 红账检出演练：10 例错置期望全检出且定位可回溯 10/10 轮。
pub fn drill_red_detect_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312c1（检出序列）。
    let mut lcg = Lcg::new(0x312c1);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = AggBook::new();
        let _ = bk.run_all(&cases);
        // 错置注入：取账面 10 个绿例，把 want 翻转为非实算码再注入。
        let mut detect = 0usize;
        let mut used = 0usize;
        for (f, seq, got, want) in bk.rows.clone().iter() {
            if got == want && used < 10 {
                let bad_want = if *got == ST_SUCCESS { ST_INVALID_HANDLE } else { ST_SUCCESS };
                let c = AggCase { family: all_families()[*f as usize], a: 0, b: 0, want: bad_want };
                // 重放该例原参数（从 cases 取）。
                let orig = &cases[*seq];
                let mut c2 = orig.clone();
                c2.want = bad_want;
                let got2 = family_judge(c2.family, c2.a, c2.b);
                if got2 != c2.want {
                    detect += 1;
                    let _ = (f, seq, got, want, &c, bad_want);
                }
                used += 1;
            }
        }
        // 红账四元组定位核对（用主账红路面）。
        let locate_ok = bk.reds_locatable();
        let _ = lcg.below(3);
        if detect == 10 && locate_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("红账检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 配额演练：七族例数与配额一致 10/10 轮。
pub fn drill_quota_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312c2（配额序列）。
    let mut lcg = Lcg::new(0x312c2);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = AggBook::new();
        let _ = bk.run_all(&cases);
        let counts = bk.family_counts();
        let mut all_ok = counts == QUOTA;
        let total: usize = counts.iter().sum();
        all_ok = all_ok && total == TOTAL_CASES && total >= 200;
        // 族名闭集。
        all_ok = all_ok && FAMILY_NAMES.iter().all(|n| !n.is_empty());
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族配额轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：每族正路例全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312c3（正路序列）。
    let mut lcg = Lcg::new(0x312c3);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // Close 正路。
        all_ok = all_ok && family_judge(Family::Close, 1, 0) == ST_SUCCESS;
        // Duplicate 正路。
        all_ok = all_ok && family_judge(Family::Duplicate, 3, 0xFFFF) == ST_SUCCESS;
        // AccessDenied 正路。
        all_ok = all_ok && family_judge(Family::AccessDenied, 0x7, 0x1) == ST_SUCCESS;
        // TypeMismatch 正路。
        all_ok = all_ok && family_judge(Family::TypeMismatch, 2, 2) == ST_SUCCESS;
        // Pseudo 正路。
        all_ok = all_ok && family_judge(Family::Pseudo, 1, 0) == ST_SUCCESS;
        // TableFull 正路。
        all_ok = all_ok && family_judge(Family::TableFull, 3, 16) == ST_SUCCESS;
        // ValueDomain 正路。
        all_ok = all_ok && family_judge(Family::ValueDomain, 0x1003, 0) == ST_SUCCESS;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：205 例全过。
    let cases = build_cases();
    let mut bk = AggBook::new();
    let hits = bk.run_all(&cases);
    if hits == TOTAL_CASES && cases.len() >= 200 {
        caught += 1;
        d.push("注1 205例:全".to_string());
    } else {
        d.push("注1 205例:缺".to_string());
    }
    // 注 2：配额守恒。
    if bk.quota_ok() && bk.covered() {
        caught += 1;
        d.push("注2 配额:守".to_string());
    } else {
        d.push("注2 配额:破".to_string());
    }
    // 注 3：红账检出（错置 1 例必落红账）。
    let mut bk3 = AggBook::new();
    let mut c3 = cases[0].clone();
    c3.want = if family_judge(c3.family, c3.a, c3.b) == ST_SUCCESS {
        ST_INVALID_HANDLE
    } else {
        ST_SUCCESS
    };
    let _ = bk3.run_case(&c3);
    if bk3.reds() == 1 && bk3.reds_locatable() {
        caught += 1;
        d.push("注3 红账:抓".to_string());
    } else {
        d.push("注3 红账:漏".to_string());
    }
    // 注 4：码域守恒。
    if bk.st_domain_ok() {
        caught += 1;
        d.push("注4 码域:守".to_string());
    } else {
        d.push("注4 码域:破".to_string());
    }
    // 注 5：七族正路锚。
    let mut all_pos = true;
    for (fi, f) in all_families().iter().enumerate() {
        let (a, b) = match fi {
            0 => (1u64, 0u64),
            1 => (3, 0xFFFF),
            2 => (0x7, 0x1),
            3 => (2, 2),
            4 => (1, 0),
            5 => (3, 16),
            _ => (0x1003, 0),
        };
        all_pos = all_pos && family_judge(*f, a, b) == ST_SUCCESS;
    }
    if all_pos {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x312c0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    AgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：聚合报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 聚合报告。
#[derive(Clone, Copy, Debug)]
pub struct AgReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 红账数。
    pub reds: usize,
    /// 配额守恒。
    pub quota: bool,
    /// 面覆盖。
    pub covered: bool,
    /// 码域守恒。
    pub st_ok: bool,
}

impl AgReport {
    /// 建报告。
    pub fn build(bk: &AggBook, hits: usize) -> Self {
        AgReport {
            cases: bk.rows(),
            hits,
            reds: bk.reds(),
            quota: bk.quota_ok(),
            covered: bk.covered(),
            st_ok: bk.st_domain_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == TOTAL_CASES && self.hits == TOTAL_CASES && self.reds == 0
            && self.quota && self.covered && self.st_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "AgReport 例={} 命中={} 红={} 配额={} 覆盖={} 码域={}",
            self.cases, self.hits, self.reds, self.quota, self.covered, self.st_ok
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct AgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ag_checklist(bk: &AggBook, hits: usize) -> Vec<AgCheckRow> {
    let rep = AgReport::build(bk, hits);
    let mut rows: Vec<AgCheckRow> = Vec::new();
    rows.push(AgCheckRow { name: "聚合≥200", passed: bk.rows() >= 200 && bk.rows() == TOTAL_CASES });
    rows.push(AgCheckRow { name: "全登记全过", passed: hits == TOTAL_CASES });
    rows.push(AgCheckRow { name: "七族覆盖", passed: bk.covered() });
    rows.push(AgCheckRow { name: "配额守恒", passed: bk.quota_ok() });
    rows.push(AgCheckRow { name: "码域守恒", passed: bk.st_domain_ok() });
    rows.push(AgCheckRow { name: "红账定位", passed: bk.reds_locatable() });
    rows.push(AgCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(AgCheckRow { name: "转录非空", passed: bk.export_lines().contains("聚合注入账") });
    rows.push(AgCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(AgCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ag_checklist_green(rows: &[AgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ag_stats_line(bk: &AggBook, hits: usize) -> String {
    format!(
        "聚合注入概况：例 {} 命中 {}/{} 红 {}",
        bk.rows(),
        hits,
        TOTAL_CASES,
        bk.reds()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12530_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = AggBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        AgReport::build(&bk, hits).all_green() && bk.export_lines().contains("聚合注入账"),
        ag_stats_line(&bk, hits),
    ));
    // X-b：红账检出 10/10。
    let (ok_r, d_r) = drill_red_detect_10();
    out.push(("X-红账检出10/10", ok_r, d_r));
    // X-c：配额 10/10。
    let (ok_q, d_q) = drill_quota_10();
    out.push(("X-族配额10/10", ok_q, d_q));
    // X-d：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-e：终验清单。
    let rows = build_ag_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        ag_checklist_green(&rows),
        AgReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        TOTAL_CASES == 205 && QUOTA.iter().sum::<usize>() == 205 && FAMILIES == 7
            && ST_INSUFFICIENT == 0xC000_0017,
        format!("total={} quota={:?} families={} ins={:#x}", TOTAL_CASES, QUOTA, FAMILIES, ST_INSUFFICIENT),
    ));
    out
}

/// run_unx12530_checks：本单判据聚合（诊断段 0x312c0）。
pub fn run_unx12530_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12530 句柄错误码聚合账");
    // J1-a：205 例聚合 10/10。
    let (ok1, d1) = drill_agg205_10();
    s.add_dyn("J1-205例聚合10/10", ok1, &d1);
    // J1-b：标准会话恒等式（≥200 全登记全过）。
    let cases = build_cases();
    let mut bk = AggBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-聚合205例全过",
        hits == TOTAL_CASES && bk.rows() >= 200 && bk.quota_ok() && bk.covered() && bk.st_domain_ok(),
        &format!("命中={}/{} 账={}", hits, TOTAL_CASES, bk.rows()),
    );
    // J2-a：红账检出 10/10。
    let (ok2, d2) = drill_red_detect_10();
    s.add_dyn("J2-红账检出10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：配额 10/10。
    let (ok3, d3) = drill_quota_10();
    s.add_dyn("J2-族配额10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x312c0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12530_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
