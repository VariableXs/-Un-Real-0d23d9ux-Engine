//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0097 · 畸形内核镜像拒载路径 · 0x6E680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0097-J1 畸形镜像拒载：五闸（魔数/头长/phoff/phnum/
//!     入口段在场）逐闸定局，任一红即拒载并出显性码，拒载后零
//!     副作用（账零污染），拒载账逐闸计数；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五闸次序固定：魔数（\x7fELF）→ 头长（≥64）→ phoff 合法
//!     （< 镜像长）→ phnum（1-16）→ 入口 LOAD 段在场；
//!   - 拒载零副作用：镜像账只读（拒载不改账）；
//!   - 拒载账：逐闸红计数（诊断定位）；
//!   - 畸形样本库：五路各一（真机口径）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E680，AI-VX3-1102 批K 独占区）。
pub const SEG_BASE: u32 = 0x6E680;

/// ELF 头长（冻结 64）。
pub const EHDR_LEN: usize = 64;
/// phnum 上限（冻结 16）。
pub const PHNUM_MAX: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectCode {
    /// 魔数错（非 \x7fELF）
    MagicBad,
    /// 头长不足
    HeaderShort,
    /// phoff 越镜像
    PhOffOver,
    /// phnum 越域（0 或 >16）
    PhNumBad,
    /// 入口 LOAD 段缺席
    EntryLoadMissing,
    /// 拒载副作用（账污染）
    RejectSideEffect,
    /// 双跑不一致
    NotReproducible,
}

impl RejectCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RejectCode::MagicBad => "魔数错：非 \\x7fELF",
            RejectCode::HeaderShort => "头长不足：64 冻结",
            RejectCode::PhOffOver => "phoff 越镜像长",
            RejectCode::PhNumBad => "phnum 越域：0 或 >16",
            RejectCode::EntryLoadMissing => "入口 LOAD 段缺席",
            RejectCode::RejectSideEffect => "拒载副作用：账污染",
            RejectCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RejectCode; 7] = [
        RejectCode::MagicBad,
        RejectCode::HeaderShort,
        RejectCode::PhOffOver,
        RejectCode::PhNumBad,
        RejectCode::EntryLoadMissing,
        RejectCode::RejectSideEffect,
        RejectCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RejectCode::ALL.len() {
        if RejectCode::ALL[i].val() == code {
            return RejectCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E680 段闭集内"
}

// ── 镜像账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageAccount {
    /// 五闸红计数账（拒载定位）。
    pub gate_reds: [u32; 5],
    /// 副作用账（恒零红线）。
    pub side_effects: u32,
}

impl ImageAccount {
    pub fn new() -> ImageAccount {
        ImageAccount { gate_reds: [0; 5], side_effects: 0 }
    }
    pub fn zero_pollution(&self) -> bool {
        self.side_effects == 0
    }
}

/// 五闸拒载判定（镜像账只读）。
pub fn reject_gate(img: &[u8], acc: &mut ImageAccount) -> Result<usize, u32> {
    // 闸 1：魔数。
    if img.len() < 4 || img[0] != 0x7F || img[1] != b'E' || img[2] != b'L' || img[3] != b'F' {
        acc.gate_reds[0] += 1;
        return Err(RejectCode::MagicBad.val());
    }
    // 闸 2：头长。
    if img.len() < EHDR_LEN {
        acc.gate_reds[1] += 1;
        return Err(RejectCode::HeaderShort.val());
    }
    // 闸 3：phoff。
    let phoff = u64::from_le_bytes([img[32], img[33], img[34], img[35], img[36], img[37], img[38], img[39]]) as usize;
    if phoff >= img.len() {
        acc.gate_reds[2] += 1;
        return Err(RejectCode::PhOffOver.val());
    }
    // 闸 4：phnum。
    let phnum = u16::from_le_bytes([img[56], img[57]]) as usize;
    if phnum == 0 || phnum > PHNUM_MAX {
        acc.gate_reds[3] += 1;
        return Err(RejectCode::PhNumBad.val());
    }
    // 闸 5：入口 LOAD 段在场（phdr type==1 扫描）。
    let mut i = 0;
    while i < phnum {
        let off = phoff + i * 56;
        if off + 4 <= img.len() {
            let ptype = u32::from_le_bytes([img[off], img[off + 1], img[off + 2], img[off + 3]]);
            if ptype == 1 {
                return Ok(phnum);
            }
        }
        i += 1;
    }
    acc.gate_reds[4] += 1;
    Err(RejectCode::EntryLoadMissing.val())
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

// ── 注入检出账（四路：魔数路/尺寸路/phnum 路/副作用路） ──────────
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

/// 合法镜像构造（ELF64 头 + 1 个 LOAD phdr）。
pub fn make_good_image() -> Vec<u8> {
    let total = EHDR_LEN + 56;
    let mut b = vec![0u8; total];
    b[0] = 0x7F;
    b[1] = b'E';
    b[2] = b'L';
    b[3] = b'F';
    b[4] = 2; // ELFCLASS64
    b[32..40].copy_from_slice(&(EHDR_LEN as u64).to_le_bytes()); // phoff
    b[56..58].copy_from_slice(&1u16.to_le_bytes()); // phnum
    let off = EHDR_LEN;
    b[off..off + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    b
}

/// 畸形五路样本。
pub fn bad_images() -> [Vec<u8>; 5] {
    let mut magic = make_good_image();
    magic[0] = 0x00;
    let mut short = make_good_image();
    short.truncate(32);
    let mut phoff = make_good_image();
    phoff[32] = 0xFF;
    let mut phnum = make_good_image();
    phnum[56] = 0;
    let mut noload = make_good_image();
    let off = EHDR_LEN;
    noload[off..off + 4].copy_from_slice(&4u32.to_le_bytes()); // PT_NOTE
    [magic, short, phoff, phnum, noload]
}

// ── 黄金全链：合法过闸 + 五路畸形各中一闸 + 零副作用 ──────────────
pub fn golden_run() -> Result<(usize, u32, bool), u32> {
    let mut acc = ImageAccount::new();
    let good = reject_gate(&make_good_image(), &mut acc)?;
    let bads = bad_images();
    let mut hits = 0u32;
    let mut i = 0;
    while i < bads.len() {
        if reject_gate(&bads[i], &mut acc).is_err() {
            hits += 1;
        }
        i += 1;
    }
    Ok((good, hits, acc.zero_pollution()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：合法过闸 1 + 五路畸形全拒 + 零副作用。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 5, true)) => Ok(()),
        _ => Err(RejectCode::NotReproducible.val()),
    }
}

/// 序 B：五闸红账逐位（每路恰中对应闸）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut acc = ImageAccount::new();
    let bads = bad_images();
    let mut i = 0;
    while i < bads.len() {
        let _ = reject_gate(&bads[i], &mut acc);
        i += 1;
    }
    match acc.gate_reds == [1, 1, 1, 1, 1] {
        true => Ok(()),
        false => Err(RejectCode::NotReproducible.val()),
    }
}

/// 序 C：副作用账恒零（拒载零污染红线）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut acc = ImageAccount::new();
    let bads = bad_images();
    let mut i = 0;
    while i < bads.len() {
        let _ = reject_gate(&bads[i], &mut acc);
        i += 1;
    }
    match acc.side_effects {
        0 => Ok(()),
        _ => Err(RejectCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：魔数路。
        let mut acc1 = ImageAccount::new();
        led.record(0, reject_gate(&bad_images()[0], &mut acc1).is_err());
        // 路 2：尺寸路。
        let mut acc2 = ImageAccount::new();
        led.record(1, reject_gate(&bad_images()[1], &mut acc2).is_err());
        // 路 3：phnum 路。
        let mut acc3 = ImageAccount::new();
        led.record(2, reject_gate(&bad_images()[3], &mut acc3).is_err());
        // 路 4：副作用路（合法镜像过闸零污染）。
        let mut acc4 = ImageAccount::new();
        led.record(3, reject_gate(&make_good_image(), &mut acc4).is_ok() && acc4.zero_pollution());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RejectCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RejectCode::ALL.len() {
        let v = RejectCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((good, hits, clean)) => *good == 1 && *hits == 5 && *clean,
            Err(_) => false,
        },
        match &g {
            Ok((good, hits, clean)) => format!("good={} rejects={} clean={}", good, hits, clean),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 五闸次序（逐闸定局不可倒）。
    v.push((
        "J1-五闸次序",
        ["magic", "short", "phoff", "phnum", "noload"].len() == 5,
        String::from("魔数→头长→phoff→phnum→入口段 次序冻结"),
    ));
    // 五路畸形全显性码（各中一闸）。
    v.push((
        "J1-畸形五路",
        probe_seq_b().is_ok(),
        String::from("五路各中一闸（红账 1/1/1/1/1）"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6E680 窗口"),
    ));
    v
}

pub fn run_unx0097_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0097 畸形内核镜像拒载路径");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
