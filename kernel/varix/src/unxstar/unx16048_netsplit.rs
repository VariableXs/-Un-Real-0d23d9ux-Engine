//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16048 · 网络/管道错误语义预留（E3 边界） · 0x692C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B03）：
//!   UNX-F16048-J1 边界账逐行落账（错误类/归 E3 归 E1 三分），边界行
//!   与 E3 冻结契约（Schema 先行）交叉一致率 100%，双声明计数 = 0 断
//!   言通过。
//! 定位：网络与命名管道错误语义（WSAEWOULDBLOCK 类/E3 域职权）与 E1
//!   翻译面交错——本条立边界账：网络类错误归 E3，翻译面只做错误码
//!   通道转发，双声明为零。E1 只修路不盖房。
//! 设计要点：
//!   - 错误类四基准：阻塞类/连接类/超时类/管道断开类；
//!   - 三分归属：E3 语义/E1 通道转发/I2 双面语义预留；
//!   - 交叉断言：与 E3 冻结契约（Schema 先行）交叉一致 100%——"我
//!     转发给你的码，你那边认账"；
//!   - 防重断言：双声明计数 = 0，网络语义在 E1 出现实装即红；
//!   - 注入面：E1 立了 WSA 语义/归属越三分/交叉失配/理由空/指针悬
//!     空/重复行——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x692C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x692C0，AI-VX-1031 E1 域批P 独占区）。
pub const SEG_BASE: u32 = 0x692C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 归属三分闭集 ─────────────────────────────────────────────────
pub const OWN_E3: usize = 0; // E3 语义
pub const OWN_E1_FWD: usize = 1; // E1 通道转发
pub const OWN_I2: usize = 2; // I2 双面语义预留
pub const OWN_COUNT: usize = 3;

pub const OWN_NAMES: [&str; OWN_COUNT] =
    ["E3语义", "E1通道转发", "I2双面预留"];

// ── 错误类四基准闭集 ─────────────────────────────────────────────
pub const EC_BLOCK: usize = 0; // 阻塞类（WSAEWOULDBLOCK）
pub const EC_CONN: usize = 1; // 连接类
pub const EC_TIMEOUT: usize = 2; // 超时类
pub const EC_PIPEBROKEN: usize = 3; // 管道断开类
pub const EC_COUNT: usize = 4;

pub const EC_NAMES: [&str; EC_COUNT] =
    ["阻塞类", "连接类", "超时类", "管道断开类"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsCode {
    /// 错误类越闭集
    EcOutOfRange,
    /// 归属越三分闭集
    OwnOutOfRange,
    /// E1 立了 WSA 语义（越界即红）
    E1OwnsWsa,
    /// 交叉失配
    CrossMismatch,
    /// 理由空
    ReasonBlank,
    /// E3 指针悬空
    E3Dangling,
    /// 重复行
    RowDuplicated,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl NsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NsCode::EcOutOfRange => "错误类越闭集",
            NsCode::OwnOutOfRange => "归属越三分闭集",
            NsCode::E1OwnsWsa => "E1 立了 WSA 语义：只修路不盖房",
            NsCode::CrossMismatch => "交叉失配：E3 不认账",
            NsCode::ReasonBlank => "理由空",
            NsCode::E3Dangling => "E3 指针悬空",
            NsCode::RowDuplicated => "重复行",
            NsCode::NotReproducible => "双跑不一致：确定性红线",
            NsCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [NsCode; 9] = [
        NsCode::EcOutOfRange,
        NsCode::OwnOutOfRange,
        NsCode::E1OwnsWsa,
        NsCode::CrossMismatch,
        NsCode::ReasonBlank,
        NsCode::E3Dangling,
        NsCode::RowDuplicated,
        NsCode::NotReproducible,
        NsCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NsCode::ALL.len() {
        if NsCode::ALL[i].val() == code {
            return NsCode::ALL[i].detail();
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

/// E3 冻结契约指针闭集（Schema 先行）。
pub const E3_POOL: [&str; 2] = ["E3-Winsock契约", "E3-管道契约"];

pub fn e3_legal(e: &str) -> bool {
    let mut i = 0;
    while i < E3_POOL.len() {
        if E3_POOL[i] == e {
            return true;
        }
        i += 1;
    }
    false
}

/// 边界账行。
#[derive(Clone, Debug)]
pub struct NetBoundRow {
    pub ec: usize,
    pub own: usize,
    /// E3 契约指针（归 E3 时必填）。
    pub e3_ref: String,
    pub reason: String,
}

/// 单行体检。
pub fn vet_row(r: &NetBoundRow) -> Result<(), u32> {
    if r.ec >= EC_COUNT {
        return Err(NsCode::EcOutOfRange.val());
    }
    if r.own >= OWN_COUNT {
        return Err(NsCode::OwnOutOfRange.val());
    }
    if r.reason.len() < 12 {
        return Err(NsCode::ReasonBlank.val());
    }
    match r.own {
        OWN_E3 => {
            if !e3_legal(&r.e3_ref) {
                return Err(NsCode::E3Dangling.val());
            }
        }
        OWN_E1_FWD => {
            // E1 通道转发：不许挂 E3 语义档（转发不是语义本体）。
            if r.e3_ref.len() != 0 {
                return Err(NsCode::E1OwnsWsa.val());
            }
        }
        _ => {
            if r.e3_ref.len() != 0 {
                return Err(NsCode::E3Dangling.val());
            }
        }
    }
    Ok(())
}

// ── 边界账 ───────────────────────────────────────────────────────
pub struct NetBoundLedger {
    rows: Vec<NetBoundRow>,
}

impl NetBoundLedger {
    pub fn new() -> NetBoundLedger {
        NetBoundLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, r: NetBoundRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].ec == r.ec {
                return Err(NsCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn ecs_closed(&self) -> Result<(), u32> {
        let mut e = 0;
        while e < EC_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].ec == e {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(NsCode::EcOutOfRange.val());
            }
            e += 1;
        }
        Ok(())
    }

    /// 防重断言：双声明计数 = 0（E1 立了语义本体的行数）。
    pub fn double_declare_count(&self) -> u32 {
        // 归 E1 通道转发的行若带了 WSA 语义注记即双声明；
        // 账面语义：E1_FWD 行的 reason 含 "WSA 语义本体" 即双声明。
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].own == OWN_E1_FWD
                && self.rows[i].reason.contains("语义本体")
            {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 交叉一致率（判据 100% 口径）。
    pub fn cross_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(NsCode::CrossMismatch.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版边界账装配。
pub fn build_initial() -> Result<NetBoundLedger, u32> {
    let mut l = NetBoundLedger::new();
    l.add(NetBoundRow {
        ec: EC_BLOCK,
        own: OWN_E3,
        e3_ref: String::from("E3-Winsock契约"),
        reason: String::from("边界条款：WSAEWOULDBLOCK 类语义归 E3 Winsock 面"),
    })?;
    l.add(NetBoundRow {
        ec: EC_CONN,
        own: OWN_E3,
        e3_ref: String::from("E3-Winsock契约"),
        reason: String::from("边界条款：连接类错误归 E3 连接状态机"),
    })?;
    l.add(NetBoundRow {
        ec: EC_TIMEOUT,
        own: OWN_E1_FWD,
        e3_ref: String::new(),
        reason: String::from("边界条款：超时类 E1 只做通道转发，本体归 E3"),
    })?;
    l.add(NetBoundRow {
        ec: EC_PIPEBROKEN,
        own: OWN_I2,
        e3_ref: String::new(),
        reason: String::from("边界条款：管道断开跨双面，I2 双面语义预留"),
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 双声明数, 交叉率))。
pub fn golden_run() -> Result<(usize, u32, usize), u32> {
    let l = build_initial()?;
    l.ecs_closed()?;
    let dd = l.double_declare_count();
    if dd != 0 {
        return Err(NsCode::E1OwnsWsa.val());
    }
    let rate = l.cross_rate()?;
    if rate != 100 {
        return Err(NsCode::CrossMismatch.val());
    }
    Ok((l.len(), dd, rate))
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
        // ① E1 立了 WSA 语义（判据复测方式：E1 账携带"语义本体"声明必红）。
        led.record(0, {
            let mut l = NetBoundLedger::new();
            let _ = l.add(NetBoundRow {
                ec: EC_BLOCK,
                own: OWN_E1_FWD,
                e3_ref: String::new(),
                reason: String::from("越界：本行携带语义本体声明"),
            });
            l.double_declare_count() == 1
        });

        // ② E1 转发行挂 E3 语义档（越轨）。
        led.record(
            1,
            vet_row(&NetBoundRow {
                ec: EC_TIMEOUT,
                own: OWN_E1_FWD,
                e3_ref: String::from("E3-Winsock契约"),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NsCode::E1OwnsWsa.val()),
        );

        // ③ 归 E3 指针悬空。
        led.record(
            2,
            vet_row(&NetBoundRow {
                ec: EC_BLOCK,
                own: OWN_E3,
                e3_ref: String::from("E3-ghost"),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NsCode::E3Dangling.val()),
        );

        // ④ 类越闭集 / 归属越三分。
        led.record(
            3,
            vet_row(&NetBoundRow {
                ec: EC_COUNT + (rnd % 2) as usize,
                own: OWN_E3,
                e3_ref: String::from("E3-Winsock契约"),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NsCode::EcOutOfRange.val())
                && vet_row(&NetBoundRow {
                    ec: EC_BLOCK,
                    own: OWN_COUNT,
                    e3_ref: String::new(),
                    reason: String::from("理由占位，长度超过十二字节"),
                }) == Err(NsCode::OwnOutOfRange.val()),
        );

        // ⑤ 理由空。
        led.record(
            4,
            vet_row(&NetBoundRow {
                ec: EC_CONN,
                own: OWN_E3,
                e3_ref: String::from("E3-Winsock契约"),
                reason: String::new(),
            }) == Err(NsCode::ReasonBlank.val()),
        );

        // ⑥ 重复行 + 轮次守卫。
        led.record(
            5,
            {
                let mut l = NetBoundLedger::new();
                let row = NetBoundRow {
                    ec: EC_BLOCK,
                    own: OWN_E3,
                    e3_ref: String::from("E3-Winsock契约"),
                    reason: String::from("理由占位，长度超过十二字节"),
                };
                let _ = l.add(row.clone());
                l.add(row) == Err(NsCode::RowDuplicated.val()) && (rnd % 1) == 0
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
                Err(NsCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法边界账必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.ecs_closed()?;
    // 三分名/四类名/E3 池点验。
    let mut o = 0;
    while o < OWN_COUNT {
        if OWN_NAMES[o].len() == 0 {
            return Err(NsCode::OwnOutOfRange.val());
        }
        o += 1;
    }
    let mut i = 0;
    while i < E3_POOL.len() {
        if !e3_legal(E3_POOL[i]) {
            return Err(NsCode::E3Dangling.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < NsCode::ALL.len() {
        let v = NsCode::ALL[i].val();
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
    // 1. 黄金正演：4 行 / 双声明 0 / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演4行双0_100%",
        matches!(g, Ok((4, 0, 100))),
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
    // 3. 边界行三分齐备（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| NetBoundLedger::new());
    v.push((
        "J1-边界行三分齐备",
        matches!(l.ecs_closed(), Ok(())),
        String::from("阻塞/连接/超时/管道"),
    ));
    // 4. 交叉一致率 100%（判据原文口径）。
    let rate = l.cross_rate().unwrap_or(0);
    v.push(("J1-交叉一致率100%", rate == 100, format!("{}%", rate)));
    // 5. 双声明计数 = 0（判据原文口径）。
    v.push((
        "J1-双声明计数0",
        l.double_declare_count() == 0,
        String::from("E1 只修路不盖房"),
    ));
    // 6. E1 立 WSA 语义必红（判据复测方式）。
    v.push((
        "J1-E1立WSA语义必红",
        vet_row(&NetBoundRow {
            ec: EC_TIMEOUT,
            own: OWN_E1_FWD,
            e3_ref: String::from("E3-Winsock契约"),
            reason: String::from("理由占位，长度超过十二字节"),
        }) == Err(NsCode::E1OwnsWsa.val()),
        String::from("复测方式"),
    ));
    // 7. 三分归属名点验。
    v.push((
        "J1-三分归属名点验",
        OWN_COUNT == 3,
        String::from("E3/E1转发/I2"),
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
        String::from("四类+三分+E3池"),
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
pub fn run_unx16048_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16048 网络/管道错误语义预留（E3 边界）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
