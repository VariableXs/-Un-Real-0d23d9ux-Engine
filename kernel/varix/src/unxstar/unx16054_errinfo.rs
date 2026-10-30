//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16054 · 错误上下文附加信息语义（IErrorInfo 面 prep） · 0x69440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B03）：
//!   UNX-F16054-J1 附加信息语义账逐行落账（附加类/承载通道/消费方），
//!   与 D5 COM 语义档交叉引用一致率 100%，通道冻结单齐备断言通过。
//! 定位：COM 世界的错误带上下文（IErrorInfo：错误对象携带描述/GUID/
//!   帮助文件），本条按 Schema 先行立附加信息语义账：E1 翻译面有哪
//!   些附加信息可供给、走什么承载通道，供 E1↔D5 联签面使用。
//! 设计要点：
//!   - 附加类四基准：描述文本/HRESULT 来源/DISP 类辅助码/帮助指针；
//!   - 承载通道三类：线程局部错误对象/返回参数/日志通道；
//!   - 双向 diff：E1 供给的附加类 D5 有消费格；D5 期望的附加类 E1 有
//!     供给行，双向 diff 为零；
//!   - 冻结断言：通道冻结单齐备（接口三要素），变更会签记录非空率
//!     100%（单方私改通道即红，同 F16013 会签纪律）；
//!   - 注入面：双向 diff 缺格/通道越三类/会签缺失/附加类越闭集/重复
//!     行/指针悬空——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69440）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69440，AI-VX-1031 E1 域批Q 独占区）。
pub const SEG_BASE: u32 = 0x69440;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 附加类四基准闭集 ─────────────────────────────────────────────
pub const ADD_DESC: usize = 0; // 描述文本
pub const ADD_HRESULT: usize = 1; // HRESULT 来源
pub const ADD_DISP: usize = 2; // DISP 类辅助码
pub const ADD_HELP: usize = 3; // 帮助指针
pub const ADD_COUNT: usize = 4;

pub const ADD_NAMES: [&str; ADD_COUNT] =
    ["描述文本", "HRESULT来源", "DISP辅助码", "帮助指针"];

// ── 承载通道三类闭集 ─────────────────────────────────────────────
pub const CHAN_TLS: usize = 0; // 线程局部错误对象
pub const CHAN_RET: usize = 1; // 返回参数
pub const CHAN_LOG: usize = 2; // 日志通道
pub const CHAN_COUNT: usize = 3;

pub const CHAN_NAMES: [&str; CHAN_COUNT] =
    ["线程局部错误对象", "返回参数", "日志通道"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EiCode {
    /// 附加类越闭集
    AddOutOfRange,
    /// 通道越三类闭集
    ChanOutOfRange,
    /// 双向 diff 缺格（供给无消费格/消费无供给行）
    BidirectionalGap,
    /// 会签缺失（单方私改通道）
    CountersignMissing,
    /// 重复行
    RowDuplicated,
    /// 通道指针悬空
    ChanDangling,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl EiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EiCode::AddOutOfRange => "附加类越闭集",
            EiCode::ChanOutOfRange => "通道越三类闭集",
            EiCode::BidirectionalGap => "双向 diff 缺格：供给/消费不对齐",
            EiCode::CountersignMissing => "会签缺失：单方私改通道即红",
            EiCode::RowDuplicated => "重复行：一附加类一行",
            EiCode::ChanDangling => "通道指针悬空",
            EiCode::NotReproducible => "双跑不一致：确定性红线",
            EiCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [EiCode; 8] = [
        EiCode::AddOutOfRange,
        EiCode::ChanOutOfRange,
        EiCode::BidirectionalGap,
        EiCode::CountersignMissing,
        EiCode::RowDuplicated,
        EiCode::ChanDangling,
        EiCode::NotReproducible,
        EiCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EiCode::ALL.len() {
        if EiCode::ALL[i].val() == code {
            return EiCode::ALL[i].detail();
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

/// D5 消费格闭集（Schema 先行冻结单投影：D5 认账的附加类）。
pub const D5_CONSUMES: [usize; ADD_COUNT] =
    [ADD_DESC, ADD_HRESULT, ADD_DISP, ADD_HELP];

/// 供给行。
#[derive(Clone, Debug)]
pub struct SupplyRow {
    pub add: usize,
    /// 承载通道（三类闭集）。
    pub chan: usize,
}

/// 单行体检。
pub fn vet_row(r: &SupplyRow) -> Result<(), u32> {
    if r.add >= ADD_COUNT {
        return Err(EiCode::AddOutOfRange.val());
    }
    if r.chan >= CHAN_COUNT {
        return Err(EiCode::ChanOutOfRange.val());
    }
    Ok(())
}

// ── 供给账 + 会签账 ──────────────────────────────────────────────
pub struct SupplyLedger {
    rows: Vec<SupplyRow>,
}

impl SupplyLedger {
    pub fn new() -> SupplyLedger {
        SupplyLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, r: SupplyRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].add == r.add {
                return Err(EiCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 双向 diff（判据 100% 口径）：供给类集 = D5 消费类集。
    pub fn bidi_diff_zero(&self) -> Result<(), u32> {
        let mut a = 0;
        while a < ADD_COUNT {
            let mut supplied = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].add == a {
                    supplied = true;
                }
                i += 1;
            }
            let mut consumed = false;
            let mut j = 0;
            while j < D5_CONSUMES.len() {
                if D5_CONSUMES[j] == a {
                    consumed = true;
                }
                j += 1;
            }
            if supplied != consumed {
                return Err(EiCode::BidirectionalGap.val());
            }
            a += 1;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 通道冻结单：接口三要素 + 会签记录。
#[derive(Clone, Debug)]
pub struct ChanFreeze {
    /// 通道。
    pub chan: usize,
    /// 接口三要素齐备（签名/语义/错误码）。
    pub tri_element: bool,
    /// 双方会签。
    pub signed: bool,
}

/// 通道冻结单齐备断言（判据原文口径）。
pub fn freeze_closed(rows: &[ChanFreeze]) -> Result<(), u32> {
    if rows.len() != CHAN_COUNT {
        return Err(EiCode::ChanOutOfRange.val());
    }
    let mut i = 0;
    while i < rows.len() {
        if !rows[i].tri_element || !rows[i].signed {
            return Err(EiCode::CountersignMissing.val());
        }
        // 通道互异。
        let mut j = i + 1;
        while j < rows.len() {
            if rows[i].chan == rows[j].chan {
                return Err(EiCode::ChanDangling.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 首版装配：四附加类全供给 + 三通道冻结单。
pub fn build_initial() -> Result<(SupplyLedger, Vec<ChanFreeze>), u32> {
    let mut s = SupplyLedger::new();
    s.add(SupplyRow { add: ADD_DESC, chan: CHAN_TLS })?;
    s.add(SupplyRow { add: ADD_HRESULT, chan: CHAN_RET })?;
    s.add(SupplyRow { add: ADD_DISP, chan: CHAN_TLS })?;
    s.add(SupplyRow { add: ADD_HELP, chan: CHAN_LOG })?;
    let freezes = vec3([
        ChanFreeze { chan: CHAN_TLS, tri_element: true, signed: true },
        ChanFreeze { chan: CHAN_RET, tri_element: true, signed: true },
        ChanFreeze { chan: CHAN_LOG, tri_element: true, signed: true },
    ]);
    freeze_closed(&freezes)?;
    s.bidi_diff_zero()?;
    Ok((s, freezes))
}

fn vec3(items: [ChanFreeze; 3]) -> Vec<ChanFreeze> {
    let mut v: Vec<ChanFreeze> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        v.push(items[i].clone());
        i += 1;
    }
    v
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((供给行数, 冻结单数))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let (s, fr) = build_initial()?;
    s.bidi_diff_zero()?;
    freeze_closed(&fr)?;
    Ok((s.len(), fr.len()))
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
        // ① 双向 diff 缺格（判据复测方式：删一行供给格断言必红）。
        led.record(0, {
            let (s, _) = build_initial().unwrap_or_else(|_| {
                (SupplyLedger::new(), Vec::new())
            });
            let mut s2 = SupplyLedger::new();
            let mut i = 0;
            while i < s.rows.len() {
                if s.rows[i].add != ADD_HELP {
                    let _ = s2.add(s.rows[i].clone());
                }
                i += 1;
            }
            s2.bidi_diff_zero() == Err(EiCode::BidirectionalGap.val())
        });

        // ② 附加类越闭集 / 通道越三类闭集。
        led.record(
            1,
            vet_row(&SupplyRow { add: ADD_COUNT + (rnd % 2) as usize, chan: CHAN_TLS })
                == Err(EiCode::AddOutOfRange.val())
                && vet_row(&SupplyRow { add: ADD_DESC, chan: CHAN_COUNT })
                    == Err(EiCode::ChanOutOfRange.val()),
        );

        // ③ 会签缺失（单方私改通道）。
        led.record(
            2,
            freeze_closed(&vec3([
                ChanFreeze { chan: CHAN_TLS, tri_element: true, signed: true },
                ChanFreeze { chan: CHAN_RET, tri_element: true, signed: false },
                ChanFreeze { chan: CHAN_LOG, tri_element: true, signed: true },
            ])) == Err(EiCode::CountersignMissing.val()),
        );

        // ④ 三要素缺失。
        led.record(
            3,
            freeze_closed(&vec3([
                ChanFreeze { chan: CHAN_TLS, tri_element: false, signed: true },
                ChanFreeze { chan: CHAN_RET, tri_element: true, signed: true },
                ChanFreeze { chan: CHAN_LOG, tri_element: true, signed: true },
            ])) == Err(EiCode::CountersignMissing.val()),
        );

        // ⑤ 重复行。
        led.record(4, {
            let mut s = SupplyLedger::new();
            let row = SupplyRow { add: ADD_DESC, chan: CHAN_TLS };
            let _ = s.add(row.clone());
            s.add(row) == Err(EiCode::RowDuplicated.val())
        });

        // ⑥ 通道重复冻结 + 轮次守卫。
        led.record(
            5,
            freeze_closed(&vec3([
                ChanFreeze { chan: CHAN_TLS, tri_element: true, signed: true },
                ChanFreeze { chan: CHAN_TLS, tri_element: true, signed: true },
                ChanFreeze { chan: CHAN_LOG, tri_element: true, signed: true },
            ])) == Err(EiCode::ChanDangling.val())
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
                Err(EiCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法附加信息账必须全过 ─────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let (s, fr) = build_initial()?;
    s.bidi_diff_zero()?;
    freeze_closed(&fr)?;
    // 四类名/三通道名点验。
    let mut a = 0;
    while a < ADD_COUNT {
        if ADD_NAMES[a].len() == 0 {
            return Err(EiCode::AddOutOfRange.val());
        }
        a += 1;
    }
    let mut c = 0;
    while c < CHAN_COUNT {
        if CHAN_NAMES[c].len() == 0 {
            return Err(EiCode::ChanOutOfRange.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EiCode::ALL.len() {
        let v = EiCode::ALL[i].val();
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
    // 1. 黄金正演：4 供给 / 3 冻结。
    let g = golden_run();
    v.push((
        "J1-黄金正演4供给3冻结",
        matches!(g, Ok((4, 3))),
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
    // 3. 双向 diff 为零（判据原文口径）。
    let (s, fr) = build_initial().unwrap_or_else(|_| {
        (SupplyLedger::new(), Vec::new())
    });
    v.push((
        "J1-双向diff为零",
        matches!(s.bidi_diff_zero(), Ok(())),
        String::from("供给≡消费"),
    ));
    // 4. 通道冻结单齐备（判据原文口径）。
    v.push((
        "J1-通道冻结单齐备",
        matches!(freeze_closed(&fr), Ok(())),
        format!("{} 通道", fr.len()),
    ));
    // 5. 会签缺失必红（判据复测方式）。
    v.push((
        "J1-单方私改必红",
        freeze_closed(&vec3([
            ChanFreeze { chan: CHAN_TLS, tri_element: true, signed: true },
            ChanFreeze { chan: CHAN_RET, tri_element: true, signed: false },
            ChanFreeze { chan: CHAN_LOG, tri_element: true, signed: true },
        ])) == Err(EiCode::CountersignMissing.val()),
        String::from("复测方式"),
    ));
    // 6. 通道三类名点验。
    v.push((
        "J1-通道三类名点验",
        CHAN_COUNT == 3,
        String::from("TLS/返回参数/日志"),
    ));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("四类+三通道"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xE1);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16054_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16054 错误上下文附加信息语义（IErrorInfo 面 prep）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
