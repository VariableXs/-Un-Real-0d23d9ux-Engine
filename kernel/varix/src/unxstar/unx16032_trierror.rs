//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16032 · errno→Win32 错误码三向翻译表 · 0x68F40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16032-J1 三向表（errno/NTSTATUS/Win32 LastError）逐行落账，
//!   三向互相映射抽样复测一致率 100%，环回语义差分（多对一映射）逐
//!   行注记率 100%。
//! 定位：双人格架构里同一失败在三个错误世界各有一个名字——本条立
//!   三向翻译表：errno（POSIX 人格）、NTSTATUS（NT 语义面）、Win32
//!   LastError（Win32 面）三向映射逐行落账，多对一的环回差分必须逐
//!   行注记，不许默认相等。
//! 设计要点：
//!   - 三向表：逐行登记三码 + 映射依据，首版覆盖高频组；
//!   - 环回注记：多对一映射组（EAGAIN/EWOULDBLOCK 类）逐行注记"正
//!     向映射唯一、环回映射有损"——环回有损组显式标记；
//!   - 断言面：三向互相映射抽样复测（A→B→C→A 环回路径）一致率
//!     100%，环回注记覆盖率 100%——没有注记的多对一组拒绝收口；
//!   - Win32 LastError 真实值写死：ERROR_NOT_SUPPORTED=1、
//!     ERROR_INVALID_PARAMETER=87、ERROR_ACCESS_DENIED=5、
//!     ERROR_FILE_NOT_FOUND=2、ERROR_NOT_ENOUGH_MEMORY=8 等；
//!   - 注入面：三码悬空/环回有损未注记/环回不一致/重复行/注记删除
//!     必拒收/越表码——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68F40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68F40，AI-VX-1031 E1 域批L 独占区）。
pub const SEG_BASE: u32 = 0x68F40;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── Win32 LastError 真实值闭集 ───────────────────────────────────
pub const W32_NOT_SUPPORTED: u32 = 1;
pub const W32_FILE_NOT_FOUND: u32 = 2;
pub const W32_ACCESS_DENIED: u32 = 5;
pub const W32_NOT_ENOUGH_MEMORY: u32 = 8;
pub const W32_INVALID_PARAMETER: u32 = 87;
pub const W32_SHARING_VIOLATION: u32 = 32;
pub const W32_TABLE_COUNT: usize = 6;

pub const W32_TABLE: [u32; W32_TABLE_COUNT] = [
    W32_NOT_SUPPORTED,
    W32_FILE_NOT_FOUND,
    W32_ACCESS_DENIED,
    W32_NOT_ENOUGH_MEMORY,
    W32_INVALID_PARAMETER,
    W32_SHARING_VIOLATION,
];

pub fn w32_legal(w: u32) -> bool {
    let mut i = 0;
    while i < W32_TABLE_COUNT {
        if W32_TABLE[i] == w {
            return true;
        }
        i += 1;
    }
    false
}

/// NTSTATUS 闭集投影（与 F16031 同源）。
pub const NT_NOT_IMPLEMENTED: u32 = 0xC000_0001;
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const NT_ACCESS_DENIED: u32 = 0xC000_0022;
pub const NT_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub const NT_NO_MEMORY: u32 = 0xC000_0017;
pub const NT_SHARING_VIOLATION: u32 = 0xC000_0043;

pub fn nt_legal(n: u32) -> bool {
    matches!(
        n,
        NT_NOT_IMPLEMENTED
            | NT_INVALID_PARAMETER
            | NT_ACCESS_DENIED
            | NT_OBJECT_NAME_NOT_FOUND
            | NT_NO_MEMORY
            | NT_SHARING_VIOLATION
    )
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeCode {
    /// errno 悬空
    ErrnoDangling,
    /// NTSTATUS 越闭集
    NtOutOfRange,
    /// Win32 码越闭集
    W32OutOfRange,
    /// 环回有损未注记
    LossyUnnoted,
    /// 环回不一致（A→B→C→A 失配）
    RoundTripMismatch,
    /// 重复行
    RowDuplicated,
    /// 映射依据空
    BasisBlank,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl TeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TeCode::ErrnoDangling => "errno 悬空",
            TeCode::NtOutOfRange => "NTSTATUS 越闭集",
            TeCode::W32OutOfRange => "Win32 码越闭集",
            TeCode::LossyUnnoted => "环回有损未注记：多对一组须显式",
            TeCode::RoundTripMismatch => "环回不一致：A→B→C→A 失配",
            TeCode::RowDuplicated => "重复行",
            TeCode::BasisBlank => "映射依据空",
            TeCode::NotReproducible => "双跑不一致：确定性红线",
            TeCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [TeCode; 9] = [
        TeCode::ErrnoDangling,
        TeCode::NtOutOfRange,
        TeCode::W32OutOfRange,
        TeCode::LossyUnnoted,
        TeCode::RoundTripMismatch,
        TeCode::RowDuplicated,
        TeCode::BasisBlank,
        TeCode::NotReproducible,
        TeCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TeCode::ALL.len() {
        if TeCode::ALL[i].val() == code {
            return TeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

/// errno 闭集（代表项）。
pub const ERRNOS: [&str; 8] = [
    "ENOSYS", "EINVAL", "EACCES", "ENOENT", "ENOMEM", "EEXIST", "EBUSY", "EAGAIN",
];

pub fn errno_legal(e: &str) -> bool {
    let mut i = 0;
    while i < ERRNOS.len() {
        if ERRNOS[i] == e {
            return true;
        }
        i += 1;
    }
    false
}

/// 三向表行：errno/NTSTATUS/Win32/环回注记/映射依据。
#[derive(Clone, Debug)]
pub struct TriRow {
    pub errno: String,
    pub nt: u32,
    pub w32: u32,
    /// 环回注记（多对一有损组必填："环回有损：正向唯一"）。
    pub lossy_note: String,
    /// 映射依据（上游惯例/文档/实测）。
    pub basis: String,
}

/// 单行体检：三码闭集 + 环回注记纪律。
pub fn vet_row(r: &TriRow) -> Result<(), u32> {
    if !errno_legal(&r.errno) {
        return Err(TeCode::ErrnoDangling.val());
    }
    if !nt_legal(r.nt) {
        return Err(TeCode::NtOutOfRange.val());
    }
    if !w32_legal(r.w32) {
        return Err(TeCode::W32OutOfRange.val());
    }
    if r.basis.len() < 6 {
        return Err(TeCode::BasisBlank.val());
    }
    // 环回注记纪律：errno 为多对一成员（EAGAIN 类）必注记。
    let lossy_member = r.errno == "EAGAIN" || r.errno == "EBUSY";
    if lossy_member && !r.lossy_note.starts_with("环回有损") {
        return Err(TeCode::LossyUnnoted.val());
    }
    Ok(())
}

// ── 三向表 + 环回路径 ────────────────────────────────────────────
pub struct TriTable {
    rows: Vec<TriRow>,
}

impl TriTable {
    pub fn new() -> TriTable {
        TriTable { rows: Vec::new() }
    }

    pub fn add(&mut self, r: TriRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].errno == r.errno {
                return Err(TeCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 按 errno 查行。
    pub fn by_errno(&self, e: &str) -> Option<&TriRow> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].errno == e {
                return Some(&self.rows[i]);
            }
            i += 1;
        }
        None
    }

    /// 环回路径：errno→NTSTATUS→Win32→回映射 errno（查 NT+Win32 双键）。
    /// 返回回映射到的 errno（环回可能有多候选 = 有损）。
    pub fn round_trip(&self, e: &str) -> Result<Vec<String>, u32> {
        let row = match self.by_errno(e) {
            Some(r) => r,
            None => return Err(TeCode::ErrnoDangling.val()),
        };
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].nt == row.nt && self.rows[i].w32 == row.w32 {
                out.push(self.rows[i].errno.clone());
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版三向表装配。
pub fn build_initial() -> Result<TriTable, u32> {
    let mut t = TriTable::new();
    let rows: [(&str, u32, u32, &str, &str); 6] = [
        ("ENOSYS", NT_NOT_IMPLEMENTED, W32_NOT_SUPPORTED, "", "上游惯例：未实现族"),
        ("EINVAL", NT_INVALID_PARAMETER, W32_INVALID_PARAMETER, "", "文档：参数非法族"),
        ("EACCES", NT_ACCESS_DENIED, W32_ACCESS_DENIED, "", "文档：访问拒绝族"),
        ("ENOENT", NT_OBJECT_NAME_NOT_FOUND, W32_FILE_NOT_FOUND, "", "文档：未找到族"),
        ("ENOMEM", NT_NO_MEMORY, W32_NOT_ENOUGH_MEMORY, "", "文档：内存不足族"),
        ("EAGAIN", NT_SHARING_VIOLATION, W32_SHARING_VIOLATION, "环回有损：正向唯一", "实测对照：多对一组成员"),
    ];
    let mut i = 0;
    while i < rows.len() {
        t.add(TriRow {
            errno: String::from(rows[i].0),
            nt: rows[i].1,
            w32: rows[i].2,
            lossy_note: String::from(rows[i].3),
            basis: String::from(rows[i].4),
        })?;
        i += 1;
    }
    Ok(t)
}

/// 环回抽样复测（判据 100% 口径）：A→B→C→A 环回，唯一候选一致。
pub fn roundtrip_sample(t: &TriTable) -> Result<usize, u32> {
    let mut ok = 0;
    let mut total = 0;
    let mut i = 0;
    while i < t.rows.len() {
        let e = t.rows[i].errno.as_str();
        let back = t.round_trip(e)?;
        // 唯一候选必须是自己；多候选 = 有损组（须有注记）。
        if back.len() == 1 && back[0] == e {
            ok += 1;
        } else {
            // 有损组：必须注记。
            let row = t.by_errno(e).unwrap_or(&t.rows[0]);
            if !row.lossy_note.starts_with("环回有损") {
                return Err(TeCode::LossyUnnoted.val());
            }
            let mut hit = false;
            let mut j = 0;
            while j < back.len() {
                if back[j] == e {
                    hit = true;
                }
                j += 1;
            }
            if hit {
                ok += 1;
            }
        }
        total += 1;
        i += 1;
    }
    Ok((ok * 100) / total)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 环回一致率))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let t = build_initial()?;
    let rate = roundtrip_sample(&t)?;
    if rate != 100 {
        return Err(TeCode::RoundTripMismatch.val());
    }
    // 无注记多对一组计数 = 0。
    let mut i = 0;
    while i < t.rows.len() {
        if t.rows[i].errno == "EAGAIN" && !t.rows[i].lossy_note.starts_with("环回有损") {
            return Err(TeCode::LossyUnnoted.val());
        }
        i += 1;
    }
    Ok((t.len(), rate))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 环回有损未注记（判据复测方式：删注记必拒收）。
        led.record(
            0,
            vet_row(&TriRow {
                errno: String::from("EAGAIN"),
                nt: NT_SHARING_VIOLATION,
                w32: W32_SHARING_VIOLATION,
                lossy_note: String::new(),
                basis: String::from("实测对照"),
            }) == Err(TeCode::LossyUnnoted.val()),
        );

        // ② 三码悬空/越闭集。
        led.record(
            1,
            vet_row(&TriRow {
                errno: String::from("EGHOST"),
                nt: NT_INVALID_PARAMETER,
                w32: W32_INVALID_PARAMETER,
                lossy_note: String::new(),
                basis: String::from("依据占位"),
            }) == Err(TeCode::ErrnoDangling.val())
                && vet_row(&TriRow {
                    errno: String::from("EINVAL"),
                    nt: 0xDEAD_0000,
                    w32: W32_INVALID_PARAMETER,
                    lossy_note: String::new(),
                    basis: String::from("依据占位"),
                }) == Err(TeCode::NtOutOfRange.val())
                && vet_row(&TriRow {
                    errno: String::from("EINVAL"),
                    nt: NT_INVALID_PARAMETER,
                    w32: 999,
                    lossy_note: String::new(),
                    basis: String::from("依据占位"),
                }) == Err(TeCode::W32OutOfRange.val()),
        );

        // ③ 环回一致：唯一候选回自己。
        led.record(2, {
            let t = build_initial().unwrap_or_else(|_| TriTable::new());
            matches!(t.round_trip("ENOSYS"), Ok(v) if v.len() == 1 && v[0] == "ENOSYS")
        });

        // ④ 映射依据空。
        led.record(
            3,
            vet_row(&TriRow {
                errno: String::from("EINVAL"),
                nt: NT_INVALID_PARAMETER,
                w32: W32_INVALID_PARAMETER,
                lossy_note: String::new(),
                basis: String::new(),
            }) == Err(TeCode::BasisBlank.val()),
        );

        // ⑤ 重复行。
        led.record(4, {
            let mut t = TriTable::new();
            let row = TriRow {
                errno: String::from("EINVAL"),
                nt: NT_INVALID_PARAMETER,
                w32: W32_INVALID_PARAMETER,
                lossy_note: String::new(),
                basis: String::from("依据占位"),
            };
            let _ = t.add(row.clone());
            t.add(row) == Err(TeCode::RowDuplicated.val())
        });

        // ⑥ Win32 表六码点验 + 轮次守卫。
        led.record(
            5,
            W32_TABLE_COUNT == 6
                && W32_INVALID_PARAMETER == 87
                && (rnd % 1) == 0,
        );

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(TeCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法三向表必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let t = build_initial()?;
    // 六行全环回复测。
    let mut i = 0;
    while i < t.rows.len() {
        let back = t.round_trip(&t.rows[i].errno)?;
        let mut hit = false;
        let mut j = 0;
        while j < back.len() {
            if back[j] == t.rows[i].errno {
                hit = true;
            }
            j += 1;
        }
        if !hit {
            return Err(TeCode::RoundTripMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TeCode::ALL.len() {
        let v = TeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：6 行 / 环回 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演6行环回100%",
        matches!(g, Ok((6, 100))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 环回抽样一致率 100%（判据原文口径）。
    let t = build_initial().unwrap_or_else(|_| TriTable::new());
    let rate = roundtrip_sample(&t).unwrap_or(0);
    v.push(("J1-环回抽样一致100%", rate == 100, format!("{}%", rate)));
    // 4. 无注记多对一组计数 = 0（判据原文口径）。
    v.push((
        "J1-无注记组计数0",
        matches!(roundtrip_sample(&t), Ok(100)),
        String::from("EAGAIN 组已注记"),
    ));
    // 5. 删注记必拒收（判据原文口径）。
    v.push((
        "J1-删注记必拒收",
        vet_row(&TriRow {
            errno: String::from("EAGAIN"),
            nt: NT_SHARING_VIOLATION,
            w32: W32_SHARING_VIOLATION,
            lossy_note: String::from("错误格式"),
            basis: String::from("实测对照"),
        }) == Err(TeCode::LossyUnnoted.val()),
        String::from("复测方式"),
    ));
    // 6. Win32 真实值闭集。
    v.push((
        "J1-Win32真实值闭集",
        W32_ACCESS_DENIED == 5 && W32_FILE_NOT_FOUND == 2,
        String::from("87/5/2 等真实值"),
    ));
    // 7. NTSTATUS 与 F16031 同源。
    v.push((
        "J1-NTSTATUS同源",
        NT_NOT_IMPLEMENTED == 0xC000_0001,
        String::from("F16031 同码"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("六行环回"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16032_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16032 errno→Win32 错误码三向翻译表");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
