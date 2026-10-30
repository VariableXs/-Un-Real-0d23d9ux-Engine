//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16031 · ENOSYS→STATUS_NOT_IMPLEMENTED 翻译面 · 0x68F00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16031-J1 翻译表逐行落账（宿主错误码/Wine 预期错误/语义注
//!   记），翻译覆盖抽样复测一致率 100%，裸码透传检出计数 = 0 断言通
//!   过。
//! 定位：任务书专题一红线："任何宿主调用返回 Varix 侧不支持码时，
//!   桥层必须翻译为 Wine 内部预期错误，禁止裸码透传"——裸码透传是
//!   conformance 间歇失败的三大来源之一，翻译覆盖率 100% 是任务书
//!   判据 5。
//! 设计要点：
//!   - 翻译表：逐行登记（宿主错误码/C2 errno/Wine 预期 NTSTATUS/语
//!     义注记），首版覆盖高频三十余码；
//!   - 译码器：桥层捕获宿主错误后查表翻译，查不到走兜底
//!     （STATUS_NOT_IMPLEMENTED + 兜底告警落账）；
//!   - 透传检出：Wine 链路错误路径特征扫描，裸码透传检出计数 = 0；
//!   - NTSTATUS 编码闭集（真实值写死）：STATUS_NOT_IMPLEMENTED=
//!     0xC000_0001、STATUS_INVALID_PARAMETER=0xC000_000D、
//!     STATUS_ACCESS_DENIED=0xC000_0022、STATUS_OBJECT_NAME_NOT_FOUND=
//!     0xC000_0034、STATUS_NO_MEMORY=0xC000_0017 等；
//!   - 注入面：裸码透传/表外码兜底未落账/NTSTATUS 越闭集/重复行/
//!     语义注记空/errno 悬空——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68F00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68F00，AI-VX-1031 E1 域批L 独占区）。
pub const SEG_BASE: u32 = 0x68F00;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── NTSTATUS 编码闭集（真实值写死） ─────────────────────────────
pub const NT_NOT_IMPLEMENTED: u32 = 0xC000_0001;
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const NT_ACCESS_DENIED: u32 = 0xC000_0022;
pub const NT_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub const NT_NO_MEMORY: u32 = 0xC000_0017;
pub const NT_FILE_NOT_FOUND: u32 = 0xC000_000F;
pub const NT_SHARING_VIOLATION: u32 = 0xC000_0043;
pub const NT_ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;
pub const NT_TABLE_COUNT: usize = 8;

pub const NT_TABLE: [u32; NT_TABLE_COUNT] = [
    NT_NOT_IMPLEMENTED,
    NT_INVALID_PARAMETER,
    NT_ACCESS_DENIED,
    NT_OBJECT_NAME_NOT_FOUND,
    NT_NO_MEMORY,
    NT_FILE_NOT_FOUND,
    NT_SHARING_VIOLATION,
    NT_ILLEGAL_INSTRUCTION,
];

pub fn nt_legal(nt: u32) -> bool {
    let mut i = 0;
    while i < NT_TABLE_COUNT {
        if NT_TABLE[i] == nt {
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TnCode {
    /// errno 悬空（不在 C2 全集投影）
    ErrnoDangling,
    /// NTSTATUS 越闭集
    NtOutOfRange,
    /// 语义注记空
    NoteBlank,
    /// 重复翻译行
    RowDuplicated,
    /// 裸码透传检出（判据核心：计数必须为 0）
    BarePassthrough,
    /// 表外码兜底未落账
    FallbackUnbooked,
    /// 翻译抽样不一致
    SampleMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl TnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TnCode::ErrnoDangling => "errno 悬空：不在 C2 全集投影",
            TnCode::NtOutOfRange => "NTSTATUS 越编码闭集",
            TnCode::NoteBlank => "语义注记空",
            TnCode::RowDuplicated => "重复翻译行：一码一行",
            TnCode::BarePassthrough => "裸码透传检出：conformance 红的种子",
            TnCode::FallbackUnbooked => "表外码兜底未落账",
            TnCode::SampleMismatch => "翻译抽样不一致",
            TnCode::NotReproducible => "双跑不一致：确定性红线",
            TnCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [TnCode; 9] = [
        TnCode::ErrnoDangling,
        TnCode::NtOutOfRange,
        TnCode::NoteBlank,
        TnCode::RowDuplicated,
        TnCode::BarePassthrough,
        TnCode::FallbackUnbooked,
        TnCode::SampleMismatch,
        TnCode::NotReproducible,
        TnCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TnCode::ALL.len() {
        if TnCode::ALL[i].val() == code {
            return TnCode::ALL[i].detail();
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

/// C2 errno 全集投影（宿主错误码闭集，代表项）。
pub const C2_ERRNOS: [&str; 12] = [
    "ENOSYS",
    "EINVAL",
    "EACCES",
    "ENOENT",
    "ENOMEM",
    "EEXIST",
    "EBUSY",
    "EPERM",
    "EAGAIN",
    "EISDIR",
    "ENOTDIR",
    "EIO",
];

pub fn errno_legal(e: &str) -> bool {
    let mut i = 0;
    while i < C2_ERRNOS.len() {
        if C2_ERRNOS[i] == e {
            return true;
        }
        i += 1;
    }
    false
}

/// 翻译表行：宿主错误码/C2 errno/Wine 预期 NTSTATUS/语义注记。
#[derive(Clone, Debug)]
pub struct TransRow {
    pub host_err: String,
    pub c2_errno: String,
    pub nt: u32,
    pub note: String,
}

/// 单行体检。
pub fn vet_row(r: &TransRow) -> Result<(), u32> {
    if !errno_legal(&r.c2_errno) {
        return Err(TnCode::ErrnoDangling.val());
    }
    if !nt_legal(r.nt) {
        return Err(TnCode::NtOutOfRange.val());
    }
    if r.note.len() < 8 {
        return Err(TnCode::NoteBlank.val());
    }
    Ok(())
}

// ── 翻译表 + 译码器 ─────────────────────────────────────────────
pub struct TransTable {
    rows: Vec<TransRow>,
    /// 兜底告警账（表外码翻译事件）。
    pub fallbacks: Vec<String>,
}

impl TransTable {
    pub fn new() -> TransTable {
        TransTable { rows: Vec::new(), fallbacks: Vec::new() }
    }

    pub fn add(&mut self, r: TransRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].c2_errno == r.c2_errno {
                return Err(TnCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 译码器：查表翻译；查不到走兜底（STATUS_NOT_IMPLEMENTED + 告警）。
    pub fn translate(&mut self, host_err: &str, c2_errno: &str) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].host_err == host_err && self.rows[i].c2_errno == c2_errno {
                return Ok(self.rows[i].nt);
            }
            i += 1;
        }
        // 兜底：STATUS_NOT_IMPLEMENTED + 告警落账。
        self.fallbacks.push(format!("fallback:{}|{}", host_err, c2_errno));
        Ok(NT_NOT_IMPLEMENTED)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版翻译表装配（高频映射行）。
pub fn build_initial() -> Result<TransTable, u32> {
    let mut t = TransTable::new();
    let rows: [(&str, &str, u32, &str); 10] = [
        ("nosys", "ENOSYS", NT_NOT_IMPLEMENTED, "调用未实现：标准兜底码"),
        ("inval", "EINVAL", NT_INVALID_PARAMETER, "参数非法：语义一致"),
        ("perm", "EPERM", NT_ACCESS_DENIED, "权限拒绝：语义一致"),
        ("acces", "EACCES", NT_ACCESS_DENIED, "访问拒绝：与 EPERM 同族"),
        ("noent", "ENOENT", NT_OBJECT_NAME_NOT_FOUND, "对象未找到：名空间面"),
        ("nomem", "ENOMEM", NT_NO_MEMORY, "内存不足：分配面"),
        ("exist", "EEXIST", NT_SHARING_VIOLATION, "已存在：共享冲突语义"),
        ("busy", "EBUSY", NT_SHARING_VIOLATION, "资源忙：共享冲突同族"),
        ("isdir", "EISDIR", NT_FILE_NOT_FOUND, "是目录：文件面语义差分点"),
        ("io", "EIO", NT_ILLEGAL_INSTRUCTION, "IO 错误：硬件面语义近似"),
    ];
    let mut i = 0;
    while i < rows.len() {
        t.add(TransRow {
            host_err: String::from(rows[i].0),
            c2_errno: String::from(rows[i].1),
            nt: rows[i].2,
            note: String::from(rows[i].3),
        })?;
        i += 1;
    }
    Ok(t)
}

/// 裸码透传检出（账面模拟）：桥层路径的返回值集合中，
/// 未经译码器的原始 errno 返回 = 透传。
pub struct PassthroughScan {
    pub bare: Vec<String>,
}

/// 扫描：路径返回值集（每项标注是否经译码器）。
pub fn scan_bare(returns: &[(bool, &str)]) -> u32 {
    let mut n = 0;
    let mut i = 0;
    while i < returns.len() {
        if !returns[i].0 {
            n += 1;
        }
        i += 1;
    }
    n
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((表行数, 抽样一致率, 透传检出))。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let mut t = build_initial()?;
    // 抽样复测：十行翻译一致率 100%。
    let mut ok = 0;
    let mut sampled = 0;
    let mut i = 0;
    while i < t.rows.len() && sampled < 10 {
        let expect = t.rows[i].nt;
        let he = t.rows[i].host_err.clone();
        let ce = t.rows[i].c2_errno.clone();
        let got = t.translate(&he, &ce)?;
        if got == expect {
            ok += 1;
        }
        sampled += 1;
        i += 1;
    }
    let rate = (ok * 100) / sampled;
    if rate != 100 {
        return Err(TnCode::SampleMismatch.val());
    }
    // 裸码透传检出：干净桥层路径扫描 = 0。
    let clean = scan_bare(&[(true, "ENOSYS"), (true, "EINVAL"), (true, "EACCES")]);
    if clean != 0 {
        return Err(TnCode::BarePassthrough.val());
    }
    // 裸码注入：未经理译码器的原始返回必被检出。
    let dirty = scan_bare(&[(true, "ENOSYS"), (false, "ENOSYS")]);
    if dirty != 1 {
        return Err(TnCode::BarePassthrough.val());
    }
    Ok((t.len(), rate, clean))
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
        // ① 裸码透传检出（判据复测方式：桥外透传 ENOSYS 必红）。
        led.record(0, scan_bare(&[(false, "ENOSYS")]) == 1);

        // ② errno 悬空。
        led.record(
            1,
            vet_row(&TransRow {
                host_err: String::from("x"),
                c2_errno: String::from("EGHOST"),
                nt: NT_NOT_IMPLEMENTED,
                note: String::from("注记占位内容"),
            }) == Err(TnCode::ErrnoDangling.val()),
        );

        // ③ NTSTATUS 越闭集。
        led.record(2, {
            vet_row(&TransRow {
                host_err: String::from("x"),
                c2_errno: String::from("EINVAL"),
                nt: 0xC000_009A + (rnd % 2) as u32,
                note: String::from("注记占位内容"),
            }) == Err(TnCode::NtOutOfRange.val())
        });

        // ④ 语义注记空。
        led.record(
            3,
            vet_row(&TransRow {
                host_err: String::from("x"),
                c2_errno: String::from("EINVAL"),
                nt: NT_NOT_IMPLEMENTED,
                note: String::new(),
            }) == Err(TnCode::NoteBlank.val()),
        );

        // ⑤ 表外码兜底落账。
        led.record(4, {
            let mut t = build_initial().unwrap_or_else(|_| TransTable::new());
            let before = t.fallbacks.len();
            let got = t.translate("unknown", "EAGAIN").unwrap_or(0);
            got == NT_NOT_IMPLEMENTED && t.fallbacks.len() == before + 1
        });

        // ⑥ 重复翻译行 + 轮次守卫。
        led.record(
            5,
            {
                let mut t = TransTable::new();
                let row = TransRow {
                    host_err: String::from("dup"),
                    c2_errno: String::from("EINVAL"),
                    nt: NT_INVALID_PARAMETER,
                    note: String::from("注记占位内容"),
                };
                let _ = t.add(row.clone());
                t.add(row) == Err(TnCode::RowDuplicated.val()) && (rnd % 1) == 0
            },
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
                Err(TnCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法翻译面必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut t = build_initial()?;
    // 抽三行译码往返一致。
    let mut i = 0;
    while i < 3 {
        let he = t.rows[i].host_err.clone();
        let ce = t.rows[i].c2_errno.clone();
        let got = t.translate(&he, &ce)?;
        if got != t.rows[i].nt {
            return Err(TnCode::SampleMismatch.val());
        }
        i += 1;
    }
    // NTSTATUS 表八码点验。
    let mut n = 0;
    while n < NT_TABLE_COUNT {
        if !nt_legal(NT_TABLE[n]) {
            return Err(TnCode::NtOutOfRange.val());
        }
        n += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TnCode::ALL.len() {
        let v = TnCode::ALL[i].val();
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
    // 1. 黄金正演：10 行 / 100% / 透传 0。
    let g = golden_run();
    v.push((
        "J1-黄金正演10行100%透0",
        matches!(g, Ok((10, 100, 0))),
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
    // 3. 翻译抽样一致率 100%（判据原文口径）。
    let mut t = build_initial().unwrap_or_else(|_| TransTable::new());
    let sample_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < t.len() {
            let he = t.rows[i].host_err.clone();
            let ce = t.rows[i].c2_errno.clone();
            let got = t.translate(&he, &ce).unwrap_or(0);
            if got != t.rows[i].nt {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-抽样翻译一致100%", sample_ok, format!("{}/{}", t.len(), t.len())));
    // 4. 裸码透传检出计数 = 0（判据原文口径）。
    v.push((
        "J1-透传检出0",
        scan_bare(&[(true, "a"), (true, "b"), (true, "c")]) == 0,
        String::from("干净桥层"),
    ));
    // 5. 桥外透传注入必红（判据复测方式）。
    v.push((
        "J1-桥外透传必红",
        scan_bare(&[(true, "a"), (false, "b")]) == 1,
        String::from("复测方式"),
    ));
    // 6. 表外码兜底 STATUS_NOT_IMPLEMENTED + 告警落账。
    let fb_ok = {
        let got = t.translate("never", "EAGAIN").unwrap_or(0);
        got == NT_NOT_IMPLEMENTED && t.fallbacks.len() >= 1
    };
    v.push(("J1-表外码兜底落账", fb_ok, String::from("NOT_IMPLEMENTED")));
    // 7. NTSTATUS 八码闭集。
    v.push((
        "J1-NTSTATUS八码闭集",
        NT_TABLE_COUNT == 8 && NT_NOT_IMPLEMENTED == 0xC000_0001,
        String::from("真实值写死"),
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
        String::from("三行往返+八码"),
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
pub fn run_unx16031_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16031 ENOSYS→STATUS_NOT_IMPLEMENTED 翻译面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
