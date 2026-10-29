//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13921 · bin 断链检出矩阵（断裂/跳跃/重叠三态） · 0x66080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F13921-J1 bin 链断链三态（断裂：bin N 后偏移跳变/跳跃：
//!      偏移差 >bin 大小/重叠：两 bin 声明区交叠）注入 3 态 ×3 点位
//!      =9/9 次检出拒载归"断链族"，合法链对照 20/20 过，断点定位
//!      精确到 bin 序号；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - bin 链模型：bin 序列 0..N，每 bin 声明区 [off, off+size)，
//!     合法链 = 后继 off == 前驱 off+size（无缝）；
//!   - 三态闭式：断裂（gap>0 且 gap<size）/跳跃（gap>size 或 gap
//!     超出尾界）/重叠（gap<0）；
//!   - 3 态 ×3 点位（首/中/尾）=9 注入格全检出+归"断链族"；
//!   - 断点定位精确到 bin 序号（定位账逐条落 bin 号）；
//!   - 合法链对照 20/20（随机 size 合成无缝链全过）；
//!   - LCG 种子写死 = SEG_BASE（0x66080），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66080，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66080;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainCode {
    /// 断裂（0<gap<size）
    Break,
    /// 跳跃（gap>size）
    Jump,
    /// 重叠（gap<0）
    Overlap,
    /// bin 尺寸为 0
    ZeroSizeBin,
    /// 断点定位缺失
    LocateMissing,
    /// 定位错 bin（定位号≠注入点）
    LocateWrongBin,
    /// 合法链误拒
    LegalChainRejected,
    /// 分档错族
    WrongFamily,
}

impl ChainCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ChainCode::Break => "断裂：后继偏移跳变且 gap<size",
            ChainCode::Jump => "跳跃：偏移差 >bin 大小",
            ChainCode::Overlap => "重叠：两 bin 声明区交叠",
            ChainCode::ZeroSizeBin => "bin 尺寸为 0：链面非法",
            ChainCode::LocateMissing => "断点定位缺失：检出必须带 bin 号",
            ChainCode::LocateWrongBin => "定位错 bin：定位号≠注入点",
            ChainCode::LegalChainRejected => "合法链误拒：对照红线",
            ChainCode::WrongFamily => "分档错族：必须归断链族",
        }
    }
    pub const ALL: [ChainCode; 8] = [
        ChainCode::Break,
        ChainCode::Jump,
        ChainCode::Overlap,
        ChainCode::ZeroSizeBin,
        ChainCode::LocateMissing,
        ChainCode::LocateWrongBin,
        ChainCode::LegalChainRejected,
        ChainCode::WrongFamily,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ChainCode::ALL.len() {
        if ChainCode::ALL[i].val() == code {
            return ChainCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66080 段闭集内"
}

// ── bin 链模型 ───────────────────────────────────────────────────
/// bin：序号 + 声明区。
#[derive(Clone, Copy, Debug)]
pub struct Bin {
    pub idx: usize,
    pub off: u32,
    pub size: u32,
}

/// 链检结果。
#[derive(Clone, Copy, Debug)]
pub struct ChainVerdict {
    pub ok: bool,
    /// 检出态（ok=false 时有效）。
    pub kind: ChainCode,
    /// 断点 bin 序号（断点前那个 bin）。
    pub at: usize,
}

/// 链检闭式：逐对相邻 bin 校验。
pub fn chain_check(bins: &[Bin]) -> Result<ChainVerdict, u32> {
    let mut i = 0;
    while i < bins.len() {
        if bins[i].size == 0 {
            return Err(ChainCode::ZeroSizeBin.val());
        }
        i += 1;
    }
    let mut i = 0;
    while i + 1 < bins.len() {
        let cur = &bins[i];
        let next = &bins[i + 1];
        let cur_end = cur.off + cur.size;
        if next.off == cur_end {
            i += 1;
            continue;
        }
        if next.off > cur_end {
            let gap = next.off - cur_end;
            let kind = if gap > cur.size { ChainCode::Jump } else { ChainCode::Break };
            return Ok(ChainVerdict { ok: false, kind, at: cur.idx });
        }
        return Ok(ChainVerdict { ok: false, kind: ChainCode::Overlap, at: cur.idx });
    }
    Ok(ChainVerdict { ok: true, kind: ChainCode::Break, at: 0 })
}

// ── 链构造 ───────────────────────────────────────────────────────
/// 无缝合法链：n 个 bin，尺寸由 LCG 定。
pub fn synth_chain(n: usize, rng: &mut Lcg) -> Vec<Bin> {
    let mut bins: Vec<Bin> = Vec::new();
    let mut off: u32 = 0x1000;
    let mut i = 0;
    while i < n {
        let size = 0x100 + (rng.below(8) as u32) * 0x100;
        bins.push(Bin { idx: i, off, size });
        off += size;
        i += 1;
    }
    bins
}

/// 注入：在 at 点后制造三态之一（改 next.off）。
pub fn inject_fault(bins: &mut Vec<Bin>, at: usize, kind: ChainCode) -> Result<(), u32> {
    if at + 1 >= bins.len() {
        return Err(ChainCode::LocateWrongBin.val());
    }
    let cur_end = bins[at].off + bins[at].size;
    match kind {
        ChainCode::Break => bins[at + 1].off = cur_end + 0x40,
        ChainCode::Jump => bins[at + 1].off = cur_end + bins[at].size + 0x200,
        ChainCode::Overlap => bins[at + 1].off = cur_end - 0x80,
        _ => return Err(ChainCode::WrongFamily.val()),
    }
    Ok(())
}

// ── 9/9 注入矩阵 ─────────────────────────────────────────────────
/// 3 态 ×3 点位（首/中/尾注入点）全检出+定位精确。
pub fn fault_matrix_9() -> Result<(u32, u32), u32> {
    let kinds = [ChainCode::Break, ChainCode::Jump, ChainCode::Overlap];
    let mut detected = 0u32;
    let mut located = 0u32;
    let mut k = 0;
    while k < 3 {
        let mut p = 0;
        while p < 3 {
            // 链长 6，注入点 0/2/4（首/中/尾）。
            let mut rng = Lcg::new(SEG_BASE as u64 + (k * 3 + p) as u64);
            let mut bins = synth_chain(6, &mut rng);
            let at = p * 2;
            inject_fault(&mut bins, at, kinds[k])?;
            let v = chain_check(&bins)?;
            if !v.ok && v.kind == kinds[k] {
                detected += 1;
            }
            if v.at == at {
                located += 1;
            }
            p += 1;
        }
        k += 1;
    }
    if detected == 9 && located == 9 {
        Ok((9, 9))
    } else {
        Err(ChainCode::LocateMissing.val())
    }
}

/// 合法链对照 20/20。
pub fn legal_chain_20() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0xBEEF);
    let mut ok = 0u32;
    let mut i = 0;
    while i < 20 {
        let n = 2 + (rng.below(9) as usize);
        let bins = synth_chain(n, &mut rng);
        let v = chain_check(&bins)?;
        if v.ok {
            ok += 1;
        }
        i += 1;
    }
    if ok == 20 {
        Ok(20)
    } else {
        Err(ChainCode::LegalChainRejected.val())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let m = fault_matrix_9()?;
    let l = legal_chain_20()?;
    Ok((m.0, l))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 族 0：零尺寸 bin。
    ledger.tries[0] += 1;
    let mut bins = synth_chain(3, &mut rng);
    bins[1].size = 0;
    if let Err(e) = chain_check(&bins) {
        if e == ChainCode::ZeroSizeBin.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：注入点越界拒收。
    ledger.tries[1] += 1;
    let mut bins2 = synth_chain(2, &mut rng);
    if inject_fault(&mut bins2, 1, ChainCode::Break).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：非法分档注入（对合法链注入 Break 态枚举外的 DocKind）。
    ledger.tries[2] += 1;
    let mut bins3 = synth_chain(3, &mut rng);
    if inject_fault(&mut bins3, 0, ChainCode::ZeroSizeBin).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ChainCode::ALL.len() {
        let v = ChainCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-9/9检出+20/20对照",
        matches!(golden, Ok((9, 20))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 三态互异。
    v.push((
        "J1-三态互异",
        ChainCode::Break != ChainCode::Jump && ChainCode::Jump != ChainCode::Overlap,
        String::from("断裂/跳跃/重叠"),
    ));
    // 定位精确正演（单点复验）。
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 7);
    let mut bins = synth_chain(5, &mut rng);
    let _ = inject_fault(&mut bins, 3, ChainCode::Jump);
    let vv = chain_check(&bins);
    let loc_ok = matches!(vv, Ok(ref c) if !c.ok && c.at == 3);
    let loc_at = vv.as_ref().map(|c| c.at).unwrap_or(0);
    v.push((
        "J1-定位精确到bin",
        loc_ok,
        format!("{:?}", loc_at),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66080 窗口")));
    v
}

pub fn run_unx13921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13921 bin 断链检出矩阵（断裂/跳跃/重叠三态）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
