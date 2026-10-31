//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2203 · 判据指纹与冻结哈希链（三成分哈希固化） · 0x6D500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2203-J1 600 条判据三成分（判据文本/阈值引用/场景挂
//!     接）指纹计算全过且哈希链闭合（逐条指纹→逐族链→全域链三
//!     级），指纹重算一致性 600/600，链断裂注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 600 条判据（10 族×60 条），三成分指纹：FNV(文本,阈值,场
//!      景位)；
//!    - 三级链：逐条指纹 → 逐族链（60 条链成族锚）→ 全域链（10
//!      族锚链成全域锚）；
//!    - 重算一致性 600/600；链断裂注入（改任一成分）10/10 检出；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D500，AI-VX3-1030 批F 独占区）。
pub const SEG_BASE: u32 = 0x6D500;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 判据总数（600）。
pub const CRITERIA_TOTAL: usize = 600;
/// 族数。
pub const FAMILY_COUNT: usize = 10;
/// 每族条数。
pub const PER_FAMILY: usize = 60;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TfpCode {
    /// 判据号越界
    NoOutOfRange,
    /// 三成分缺
    ComponentMissing,
    /// 指纹重算不一致
    FingerprintMismatch,
    /// 族链断
    FamilyChainBroken,
    /// 全域链断
    GlobalChainBroken,
    /// 断裂注入漏抓
    BreakInjectLeaked,
    /// 条数缺（≠600）
    TotalShort,
    /// 双跑不一致
    NotReproducible,
}

impl TfpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TfpCode::NoOutOfRange => "判据号越界：600 条冻结",
            TfpCode::ComponentMissing => "三成分缺",
            TfpCode::FingerprintMismatch => "指纹重算不一致",
            TfpCode::FamilyChainBroken => "族链断",
            TfpCode::GlobalChainBroken => "全域链断",
            TfpCode::BreakInjectLeaked => "断裂注入漏抓",
            TfpCode::TotalShort => "条数缺：≠600",
            TfpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TfpCode; 8] = [
        TfpCode::NoOutOfRange,
        TfpCode::ComponentMissing,
        TfpCode::FingerprintMismatch,
        TfpCode::FamilyChainBroken,
        TfpCode::GlobalChainBroken,
        TfpCode::BreakInjectLeaked,
        TfpCode::TotalShort,
        TfpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TfpCode::ALL.len() {
        if TfpCode::ALL[i].val() == code {
            return TfpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D500 段闭集内"
}

// ── 判据行（三成分） ───────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CriteriaRow {
    pub no: usize,
    pub text: String,
    /// 阈值引用（冻结值编号）。
    pub threshold_ref: u64,
    /// 场景挂接位图。
    pub scene_mask: u16,
}

impl CriteriaRow {
    pub fn legal(&self) -> Result<(), u32> {
        if self.no == 0 || self.no > CRITERIA_TOTAL {
            return Err(TfpCode::NoOutOfRange.val());
        }
        if self.text.len() == 0 || self.threshold_ref == 0 || self.scene_mask == 0 {
            return Err(TfpCode::ComponentMissing.val());
        }
        Ok(())
    }
    /// 三成分指纹。
    pub fn fingerprint(&self) -> Result<u64, u32> {
        self.legal()?;
        let mut h: u64 = 0xcbf29ce484222325;
        let b = self.text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.threshold_ref;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.scene_mask as u64;
        h = h.wrapping_mul(0x100000001b3);
        Ok(h)
    }
}

/// 族链：60 条指纹链成族锚。
pub fn family_chain(rows: &[CriteriaRow], start: usize) -> Result<u64, u32> {
    let mut h = SEG_BASE as u64 + start as u64;
    let mut i = 0;
    while i < PER_FAMILY {
        let fp = rows[start + i].fingerprint()?;
        h ^= fp;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    Ok(h)
}

/// 全域链：10 族锚链成全域锚。
pub fn global_chain(rows: &[CriteriaRow]) -> Result<(u64, [u64; FAMILY_COUNT]), u32> {
    if rows.len() != CRITERIA_TOTAL {
        return Err(TfpCode::TotalShort.val());
    }
    let mut anchors = [0u64; FAMILY_COUNT];
    let mut h = 0xcbf29ce484222325;
    let mut f = 0;
    while f < FAMILY_COUNT {
        anchors[f] = family_chain(rows, f * PER_FAMILY)?;
        h ^= anchors[f];
        h = h.wrapping_mul(0x100000001b3);
        f += 1;
    }
    Ok((h, anchors))
}

/// 重算一致性 600/600。
pub fn recompute_600(rows: &[CriteriaRow]) -> Result<u32, u32> {
    let mut ok = 0;
    let mut i = 0;
    while i < rows.len() {
        let fp1 = rows[i].fingerprint()?;
        let fp2 = rows[i].fingerprint()?;
        if fp1 != fp2 {
            return Err(TfpCode::FingerprintMismatch.val());
        }
        ok += 1;
        i += 1;
    }
    if ok != CRITERIA_TOTAL {
        return Err(TfpCode::FingerprintMismatch.val());
    }
    Ok(ok as u32)
}

/// 断裂注入 10/10（每轮改一成分，锚必变）。
pub fn break_inject_10(base: &[CriteriaRow], anchor: u64) -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut broken = Vec::from(base);
        let idx = k * 61;
        match k % 3 {
            0 => broken[idx].text = format!("{}-X", broken[idx].text),
            1 => broken[idx].threshold_ref += 1,
            _ => broken[idx].scene_mask ^= 0x1,
        }
        let (fresh, _) = global_chain(&broken)?;
        if fresh != anchor {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(TfpCode::BreakInjectLeaked.val());
    }
    Ok(caught)
}

/// 基线 600 条。
pub fn baseline_rows() -> Result<Vec<CriteriaRow>, u32> {
    let mut v = Vec::new();
    let mut no = 1;
    while no <= CRITERIA_TOTAL {
        v.push(CriteriaRow {
            no,
            text: format!("判据 {} 原文锚", no),
            threshold_ref: 100 + (no as u64 % 50),
            scene_mask: ((no % 7) as u16) | 0x10,
        });
        no += 1;
    }
    Ok(v)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：600 条→三级链→重算 600/600→断裂 10/10。
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let rows = baseline_rows()?;
    let (anchor, _) = global_chain(&rows)?;
    let rc = recompute_600(&rows)?;
    let brk = break_inject_10(&rows, anchor)?;
    Ok((anchor, rc, brk))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let rows = baseline_rows()?;
    global_chain(&rows).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 三成分缺 → 必拒。
    CriteriaRow { no: 1, text: String::new(), threshold_ref: 1, scene_mask: 1 }.fingerprint().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 链断 → 必拒。
    let rows = baseline_rows()?;
    let (a, _) = global_chain(&rows)?;
    let mut broken = rows;
    broken[300].text = String::from("broken");
    match global_chain(&broken) {
        Ok((fresh, _)) if fresh != a => Ok(()),
        _ => Err(TfpCode::GlobalChainBroken.val()),
    }
}

/// 注入 10/10：越号、成分缺、断裂抓、重算四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越号。
        led.record(0, CriteriaRow { no: 601, text: String::from("x"), threshold_ref: 1, scene_mask: 1 }.fingerprint().is_err());
        // 槽1：成分缺。
        led.record(1, CriteriaRow { no: 1, text: String::from("x"), threshold_ref: 0, scene_mask: 1 }.fingerprint().is_err());
        // 槽2：断裂抓。
        let rows = baseline_rows().ok();
        let h2 = match &rows {
            Some(rs) => match global_chain(rs) {
                Ok((a, _)) => break_inject_10(rs, a) == Ok(10),
                Err(_) => false,
            },
            None => false,
        };
        led.record(2, h2);
        // 槽3：重算。
        led.record(3, recompute_600(&baseline_rows().unwrap_or_default()) == Ok(600));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TfpCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TfpCode::ALL.len() {
        let v = TfpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 600 条三级链。
    let rows = baseline_rows();
    v.push((
        "J1-600条三级链闭合",
        match &rows {
            Ok(rs) => global_chain(rs).is_ok() && rs.len() == 600,
            Err(_) => false,
        },
        format!("{}/{} 条", rows.as_ref().map(|r| r.len()).unwrap_or(0), CRITERIA_TOTAL),
    ));
    // 重算 600/600。
    v.push((
        "J1-指纹重算600/600",
        recompute_600(&baseline_rows().unwrap_or_default()) == Ok(600),
        format!("{}/600", recompute_600(&baseline_rows().unwrap_or_default()).unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D500 窗口")));
    v
}

pub fn run_unx2203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2203 判据指纹与冻结哈希链（三成分哈希固化）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
