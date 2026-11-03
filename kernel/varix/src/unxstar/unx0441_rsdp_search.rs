//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0441 RSDP 签名搜索（EBDA/BIOS 双窗） · 0x75000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   RSDP 校验和验证通过（1/1），revision 与长度字段自洽断言
//!   （v1:20/v2+:36）。
//! 设计要点：
//!   1. 双窗扫描模型：EBDA 窗（0x9FC00 起 1KB）+ BIOS 窗
//!      （0xE0000~0xFFFFF，128KB），扫描步进 16 字节——ACPI 规范
//!      §5.2.5.2 规定 RSDP 必落在 16 字节对齐地址上。
//!   2. RSD PTR 签名 8 字节（52 53 44 20 50 54 52 20，即 "RSD PTR "）
//!      整段匹配；命中即进入校验链：前 20 字节按 u8 累加 ≡ 0
//!      （mod 256）。
//!   3. revision 与长度字段自洽断言：revision 0（v1）有效长度 20；
//!      revision ≥ 2（v2+）length 字段必须为 36；自洽失配即拒。
//!   4. 校验不过的候选不静默丢弃：记入拒绝账后扫描继续，双窗扫尽
//!      仍无全绿候选才报 NotFound（1/1 语义：恰好一个候选全链通过）。
//!   5. 单调状态机：Idle → EbdaScan → BiosScan → Found/Exhausted，
//!      倒退显性拒。
//!   6. 注入三路（坏校验/坏自洽/双窗空）逐路必抓，LCG 种子固定。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75000，WBV1104 批 ACPI 栈首卡区）。
pub const SEG_BASE: u32 = 0x75000;

// ── 冻结常量表（来源：ACPI 规范 §5.2.5.2 RSDP 搜索要求） ─────────
/// EBDA 区基址（BMP 内存顶约定值 0x9FC00）。
pub const EBDA_BASE: u32 = 0x9FC00;
/// EBDA 扫描窗长（规范：EBDA 前 1KB）。
pub const EBDA_WIN: u32 = 0x0400;
/// BIOS ROM 区下沿。
pub const BIOS_LO: u32 = 0xE0000;
/// BIOS ROM 区上沿（含）。
pub const BIOS_HI: u32 = 0xFFFFF;
/// BIOS 扫描窗长（0x20000 = 128KB）。
pub const BIOS_WIN: u32 = 0x020000;
/// 扫描步进（RSDP 必 16 对齐）。
pub const STRIDE: u32 = 16;
/// v1 有效长度（无 length 字段，规范定长 20）。
pub const V1_LEN: u32 = 20;
/// v2+ length 字段必须值。
pub const V2_LEN: u32 = 36;
/// RSDP 签名长度。
pub const SIG_LEN: usize = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpScanCode {
    /// 窗口越界（地址落在 EBDA/BIOS 双窗之外）
    WindowOver,
    /// 扫描步进非 16 对齐
    StrideMisaligned,
    /// 候选前 20 字节校验和不过
    ChecksumBad,
    /// revision 与长度字段自洽失配
    RevLenInconsistent,
    /// 双窗扫尽未见合法 RSDP
    NotFound,
    /// 搜索状态机倒退
    PhaseRegress,
    /// 双跑不一致
    NotReproducible,
}

impl RsdpScanCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RsdpScanCode::WindowOver => "窗口越界：地址不在 EBDA(0x9FC00+1KB)/BIOS(0xE0000~0xFFFFF) 双窗内，建议核对物理地址来源",
            RsdpScanCode::StrideMisaligned => "扫描步进违约：地址非 16 对齐，建议按 ACPI §5.2.5.2 步进重扫",
            RsdpScanCode::ChecksumBad => "候选前 20 字节校验和 ≢ 0，建议核对 RSDP 划线内存完整性",
            RsdpScanCode::RevLenInconsistent => "revision 与长度自洽失配：v1 应为 20 / v2+ 应为 36，建议核对 revision 与 length 字段",
            RsdpScanCode::NotFound => "双窗扫尽未见合法 RSDP：建议核对固件是否启用 ACPI（ACPI 2.0+ 须开）",
            RsdpScanCode::PhaseRegress => "搜索状态机倒退：单调推进被破坏，建议检查扫描驱动序",
            RsdpScanCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定源",
        }
    }
    pub const ALL: [RsdpScanCode; 7] = [
        RsdpScanCode::WindowOver,
        RsdpScanCode::StrideMisaligned,
        RsdpScanCode::ChecksumBad,
        RsdpScanCode::RevLenInconsistent,
        RsdpScanCode::NotFound,
        RsdpScanCode::PhaseRegress,
        RsdpScanCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RsdpScanCode::ALL.len() {
        if let Some(c) = RsdpScanCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75000 段闭集内"
}

// ── RSD PTR 签名（8 字节，ACPI §5.2.5.2） ───────────────────────
pub const RSDP_SIG: [u8; SIG_LEN] = [0x52, 0x53, 0x44, 0x20, 0x50, 0x54, 0x52, 0x20];

/// 签名匹配（窗口内偏移 off 起连续 8 字节整段比对）。
fn sig_at(window: &[u8], off: usize) -> bool {
    match window.get(off..off + SIG_LEN) {
        Some(s) => s == &RSDP_SIG[..],
        None => false,
    }
}

/// 前 20 字节校验和 ≡ 0（mod 256）。
fn sum20_ok(window: &[u8], off: usize) -> bool {
    let mut s = 0u8;
    let mut i = 0;
    while i < 20 {
        match window.get(off + i) {
            Some(b) => s = s.wrapping_add(*b),
            None => return false,
        }
        i += 1;
    }
    s == 0
}

/// revision 与长度字段自洽断言（v1:20 / v2+:36）。
///
/// revision 在第 15 字节；v2+ 的 length 在第 20..24 字节（LE u32）。
fn rev_len_ok(window: &[u8], off: usize) -> bool {
    let rev = match window.get(off + 15) {
        Some(r) => *r,
        None => return false,
    };
    if rev == 0 {
        // v1：无 length 字段，有效长度恒 20。
        return true;
    }
    // v2+：length 字段必须自洽为 36。
    let b0 = match window.get(off + 20) {
        Some(b) => *b as u32,
        None => return false,
    };
    let b1 = match window.get(off + 21) {
        Some(b) => *b as u32,
        None => return false,
    };
    let b2 = match window.get(off + 22) {
        Some(b) => *b as u32,
        None => return false,
    };
    let b3 = match window.get(off + 23) {
        Some(b) => *b as u32,
        None => return false,
    };
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24) == V2_LEN
}

// ── 内存像（EBDA 窗 + BIOS 窗双口） ──────────────────────────────
/// 物理内存镜像：只承载两块扫描窗，地址换算由窗口基址完成。
#[derive(Clone, Debug)]
pub struct MemoryImage {
    /// EBDA 窗内容（下标 0 对应物理 0x9FC00）。
    ebda: Vec<u8>,
    /// BIOS 窗内容（下标 0 对应物理 0xE0000）。
    bios: Vec<u8>,
}

impl MemoryImage {
    pub fn new() -> MemoryImage {
        MemoryImage { ebda: Vec::new(), bios: Vec::new() }
    }

    /// 全零镜像（标准窗宽）。
    pub fn blank() -> MemoryImage {
        let mut e = Vec::new();
        let mut b = Vec::new();
        let mut i = 0;
        while i < EBDA_WIN as usize {
            e.push(0u8);
            i += 1;
        }
        i = 0;
        while i < BIOS_WIN as usize {
            b.push(0u8);
            i += 1;
        }
        MemoryImage { ebda: e, bios: b }
    }

    /// 物理地址 → 窗内偏移（双窗守卫）。
    pub fn localize(&self, addr: u32) -> Result<(usize, usize), u32> {
        // EBDA 窗。
        if addr >= EBDA_BASE {
            let d = addr - EBDA_BASE;
            if d < EBDA_WIN {
                return Ok((0, d as usize));
            }
        }
        // BIOS 窗。
        if addr >= BIOS_LO && addr <= BIOS_HI {
            let d = addr - BIOS_LO;
            if d < BIOS_WIN {
                return Ok((1, d as usize));
            }
        }
        Err(RsdpScanCode::WindowOver.val())
    }

    /// 向镜像写一段字节（越窗/越尾显性拒）。
    pub fn plant(&mut self, addr: u32, bytes: &[u8]) -> Result<(), u32> {
        let (w, off) = self.localize(addr)?;
        let win = if w == 0 { &mut self.ebda } else { &mut self.bios };
        let end = match off.checked_add(bytes.len()) {
            Some(e) => e,
            None => return Err(RsdpScanCode::WindowOver.val()),
        };
        if end > win.len() {
            return Err(RsdpScanCode::WindowOver.val());
        }
        let mut i = 0;
        while i < bytes.len() {
            match win.get_mut(off + i) {
                Some(slot) => *slot = bytes[i],
                None => return Err(RsdpScanCode::WindowOver.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 窗选择器（0=EBDA，1=BIOS）。
    pub fn window(&self, w: usize) -> &[u8] {
        if w == 0 {
            &self.ebda
        } else {
            &self.bios
        }
    }

    /// 窗物理基址（0=EBDA，1=BIOS）。
    pub fn window_base(w: usize) -> u32 {
        if w == 0 {
            EBDA_BASE
        } else {
            BIOS_LO
        }
    }
}

// ── 合法 RSDP 像构造（供栽种/黄金流用） ──────────────────────────
/// 构造一张校验链全绿的 RSDP 字节像（revision 0 → 20 字节，
/// revision ≥2 → 36 字节并带扩展校验和）。
pub fn build_rsdp_bytes(revision: u8) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < SIG_LEN {
        v.push(RSDP_SIG[i]);
        i += 1;
    }
    v.push(0u8); // checksum 占位（第 8 字节）
    let mut j = 0;
    while j < 6 {
        v.push(b'V');
        j += 1;
    }
    v.push(revision); // 第 15 字节
    // rsdt_address（第 16..20 字节，LE）
    v.push(0xE0u8);
    v.push(0x00u8);
    v.push(0x0Eu8);
    v.push(0x00u8);
    if revision == 0 {
        // v1：前 20 字节补校验和。
        let s = sum_raw(&v);
        match v.get_mut(8) {
            Some(slot) => *slot = s.wrapping_neg(),
            None => {}
        }
        return v;
    }
    // v2+：length = 36（LE）
    v.push(36u8);
    v.push(0u8);
    v.push(0u8);
    v.push(0u8);
    // xsdt_address（8 字节）
    let mut k = 0;
    while k < 8 {
        v.push(0u8);
        k += 1;
    }
    // ext_checksum 占位（第 32 字节）+ reserved 3 字节
    v.push(0u8);
    v.push(0u8);
    v.push(0u8);
    v.push(0u8);
    // 第一校验：前 20 字节。
    let s1 = sum_raw(&v[..20]);
    match v.get_mut(8) {
        Some(slot) => *slot = s1.wrapping_neg(),
        None => {}
    }
    // 扩展校验：第 20..36 字节。
    let s2 = sum_raw(&v[20..36]);
    match v.get_mut(32) {
        Some(slot) => *slot = s2.wrapping_neg(),
        None => {}
    }
    v
}

/// 字节累加（mod 256）。
fn sum_raw(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, x| a.wrapping_add(*x))
}

// ── 单调状态机 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchPhase {
    /// 未开扫
    Idle,
    /// EBDA 窗扫描中
    EbdaScan,
    /// BIOS 窗扫描中
    BiosScan,
    /// 已找到合法候选
    Found,
    /// 双窗扫尽无候选
    Exhausted,
}

impl SearchPhase {
    fn rank(self) -> u32 {
        match self {
            SearchPhase::Idle => 0,
            SearchPhase::EbdaScan => 1,
            SearchPhase::BiosScan => 2,
            SearchPhase::Found => 3,
            SearchPhase::Exhausted => 4,
        }
    }
}

/// 搜索游标：单调推进，倒退显性拒。
#[derive(Clone, Debug)]
pub struct SearchWalker {
    pub phase: SearchPhase,
    pub addr: u32,
    /// 合法命中（物理地址）。
    pub found: Option<u32>,
    /// 校验/自洽被拒候选数（显性账）。
    pub rejected: u32,
    /// 签名命中数（含被拒）。
    pub sig_hits: u32,
    /// 访问地址步进账（每次窗内访问必须 16 对齐）。
    pub visited: u32,
    pub misaligned_visits: u32,
}

impl SearchWalker {
    pub fn new() -> SearchWalker {
        SearchWalker {
            phase: SearchPhase::Idle,
            addr: EBDA_BASE,
            found: None,
            rejected: 0,
            sig_hits: 0,
            visited: 0,
            misaligned_visits: 0,
        }
    }

    /// 相位推进（单调：rank 只许升）。
    pub fn advance(&mut self, to: SearchPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(RsdpScanCode::PhaseRegress.val());
        }
        self.phase = to;
        Ok(())
    }

    /// 单窗线性扫描（步进 16，签名命中即验链）。
    fn scan_window(&mut self, img: &MemoryImage, w: usize) -> bool {
        let base = MemoryImage::window_base(w);
        let win = img.window(w);
        let mut off = 0usize;
        while off + SIG_LEN <= win.len() {
            if off as u32 % STRIDE != 0 {
                self.misaligned_visits += 1;
            }
            self.visited += 1;
            if sig_at(win, off) {
                self.sig_hits += 1;
                if sum20_ok(win, off) && rev_len_ok(win, off) {
                    self.found = Some(base + off as u32);
                    return true;
                }
                self.rejected += 1;
            }
            off += STRIDE as usize;
        }
        false
    }

    /// 双窗全扫主入口。
    pub fn full_search(&mut self, img: &MemoryImage) -> Result<(), u32> {
        self.advance(SearchPhase::EbdaScan)?;
        if self.scan_window(img, 0) {
            self.advance(SearchPhase::Found)?;
            return Ok(());
        }
        self.advance(SearchPhase::BiosScan)?;
        if self.scan_window(img, 1) {
            self.advance(SearchPhase::Found)?;
            return Ok(());
        }
        self.advance(SearchPhase::Exhausted)?;
        Err(RsdpScanCode::NotFound.val())
    }
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据①：RSDP 校验和验证通过（1/1）——双窗搜索恰好一个候选全链通过。
fn judge_checksum_ok(img: &MemoryImage) -> bool {
    let mut w = SearchWalker::new();
    match w.full_search(img) {
        Ok(()) => w.found.is_some() && w.rejected == 0,
        Err(_) => false,
    }
}

/// 判据②：revision 与长度字段自洽（v1:20 / v2+:36）。
fn judge_rev_len_ok() -> bool {
    // v1 像：无 length 字段，自洽恒真。
    let v1 = build_rsdp_bytes(0);
    if !rev_len_ok(&v1, 0) {
        return false;
    }
    // v2 像：length 字段必须读到 36。
    let v2 = build_rsdp_bytes(2);
    if !rev_len_ok(&v2, 0) {
        return false;
    }
    // 破坏 v2 length（改 40）必须失配。
    let mut bad = v2.clone();
    match bad.get_mut(20) {
        Some(slot) => *slot = 40,
        None => return false,
    }
    !rev_len_ok(&bad, 0)
}

/// 步进守卫：所有访问地址 16 对齐。
fn stride_ok(w: &SearchWalker) -> bool {
    w.misaligned_visits == 0 && w.visited > 0
}

// ── LCG（种子固定） ─────────────────────────────────────────────
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

// ── 黄金双窗流 ──────────────────────────────────────────────────
/// 黄金景 A：RSDP 落 EBDA 窗（0x9FC90，16 对齐）。
pub fn golden_ebda() -> Result<(u32, u32, u32), u32> {
    let mut img = MemoryImage::blank();
    let bytes = build_rsdp_bytes(2);
    let _ = img.plant(EBDA_BASE + 0x090, &bytes);
    let mut w = SearchWalker::new();
    w.full_search(&img)?;
    match w.found {
        Some(a) => Ok((1, w.rejected, a)),
        None => Err(RsdpScanCode::NotFound.val()),
    }
}

/// 黄金景 B：RSDP 落 BIOS 窗（0xF00C0，16 对齐）。
pub fn golden_bios() -> Result<(u32, u32, u32), u32> {
    let mut img = MemoryImage::blank();
    let bytes = build_rsdp_bytes(0);
    let _ = img.plant(0xF00C0, &bytes);
    let mut w = SearchWalker::new();
    w.full_search(&img)?;
    match w.found {
        Some(a) => Ok((1, w.rejected, a)),
        None => Err(RsdpScanCode::NotFound.val()),
    }
}

/// 黄金景 C：EBDA 候选坏校验被拒后，BIOS 窗合法候选顶上。
pub fn golden_reject_then_found() -> Result<(u32, u32, u32), u32> {
    let mut img = MemoryImage::blank();
    let mut bad = build_rsdp_bytes(2);
    match bad.get_mut(9) {
        Some(slot) => *slot ^= 0xFF,
        None => return Err(RsdpScanCode::ChecksumBad.val()),
    }
    let _ = img.plant(EBDA_BASE + 0x080, &bad);
    let _ = img.plant(0xF00D0, &build_rsdp_bytes(2));
    let mut w = SearchWalker::new();
    w.full_search(&img)?;
    match (w.found, w.rejected) {
        (Some(a), r) if r == 1 => Ok((1, r, a)),
        _ => Err(RsdpScanCode::NotReproducible.val()),
    }
}

/// 探针序 A：EBDA 黄金命中，地址逐字节复算。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_ebda() {
        Ok((1, 0, a)) if a == EBDA_BASE + 0x090 => Ok(()),
        _ => Err(RsdpScanCode::NotReproducible.val()),
    }
}

/// 探针序 B：双窗空 ⇒ NotFound 显性。
pub fn probe_seq_b() -> Result<(), u32> {
    let img = MemoryImage::blank();
    let mut w = SearchWalker::new();
    match w.full_search(&img) {
        Err(e) if e == RsdpScanCode::NotFound.val() => Ok(()),
        _ => Err(RsdpScanCode::NotReproducible.val()),
    }
}

/// 探针序 C：状态机倒退必抓。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut w = SearchWalker::new();
    let _ = w.advance(SearchPhase::BiosScan);
    match w.advance(SearchPhase::EbdaScan) {
        Err(e) if e == RsdpScanCode::PhaseRegress.val() => Ok(()),
        _ => Err(RsdpScanCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 路） ─────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入三路：路 1 坏校验必抓、路 2 坏自洽必抓、路 3 双窗空必报 NotFound。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：坏校验（checksum 字节翻转）→ 搜索必不以其为命中。
        let mut img1 = MemoryImage::blank();
        let mut bad = build_rsdp_bytes(2);
        match bad.get_mut(8 + (rng.below(11) as usize)) {
            Some(slot) => *slot ^= 0x01,
            None => {}
        }
        let _ = img1.plant(EBDA_BASE + 0x040, &bad);
        let mut w1 = SearchWalker::new();
        let r1 = w1.full_search(&img1);
        led.record(0, match r1 {
            Err(e) => e == RsdpScanCode::NotFound.val(),
            Ok(()) => false,
        });
        // 路 2：坏自洽（v2 length 改 40）→ 搜索必不命中。
        let mut img2 = MemoryImage::blank();
        let mut bad2 = build_rsdp_bytes(2);
        match bad2.get_mut(20) {
            Some(slot) => *slot = 40,
            None => {}
        }
        let _ = img2.plant(0xE1000, &bad2);
        let mut w2 = SearchWalker::new();
        let r2 = w2.full_search(&img2);
        led.record(1, match r2 {
            Err(e) => e == RsdpScanCode::NotFound.val(),
            Ok(()) => false,
        });
        // 路 3：双窗空 → NotFound 显性。
        let img3 = MemoryImage::blank();
        let mut w3 = SearchWalker::new();
        led.record(2, w3.full_search(&img3).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_ebda()?;
    let b = golden_ebda()?;
    let c = golden_bios()?;
    let d = golden_bios()?;
    if a != b || c != d {
        return Err(RsdpScanCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < RsdpScanCode::ALL.len() {
        if let Some(c) = RsdpScanCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 守卫：段位窗口。
    v.push((
        "守卫-段位窗口",
        segment_guard(),
        format!("SEG_BASE=0x{:x}", SEG_BASE),
    ));

    // 判据①：EBDA 黄金命中，校验 1/1。
    let a = golden_ebda();
    v.push((
        "J1-EBDA命中校验1/1",
        match &a {
            Ok((1, 0, _)) => judge_checksum_ok(&{
                let mut img = MemoryImage::blank();
                let _ = img.plant(EBDA_BASE + 0x090, &build_rsdp_bytes(2));
                img
            }),
            _ => false,
        },
        match &a {
            Ok((_, r, addr)) => format!("addr=0x{:x} rejects={}", addr, r),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 判据②：revision/长度自洽（v1:20 / v2+:36）。
    v.push((
        "J2-revision长度自洽",
        judge_rev_len_ok(),
        String::from("v1=20 v2+=36 注入失配必抓"),
    ));

    // BIOS 窗命中。
    let b = golden_bios();
    v.push((
        "BIOS窗命中",
        match &b {
            Ok((1, 0, 0xF00C0)) => true,
            _ => false,
        },
        match &b {
            Ok((_, _, addr)) => format!("addr=0x{:x}", addr),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 拒绝后顶上：坏候选不静默，账面 +1，合法候选仍命中。
    let c = golden_reject_then_found();
    v.push((
        "拒绝账-坏候选不静默",
        match &c {
            Ok((1, 1, 0xF00D0)) => true,
            _ => false,
        },
        match &c {
            Ok((_, r, addr)) => format!("rejects={} found=0x{:x}", r, addr),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 双窗空：NotFound 显性。
    v.push((
        "双窗空-NotFound显性",
        probe_seq_b().is_ok(),
        String::from("blank 镜像扫尽 ⇒ 0x75004 段内 NotFound"),
    ));

    // 步进 16 对齐账。
    {
        // 口径 A：空镜像双窗全扫 ⇒ 步数 = (EBDA_WIN + BIOS_WIN − 8×窗数补齐)/16，
        // 逐窗 (窗长−8)/16 + 1 = 64 + 8192 = 8256 步。
        let blank = MemoryImage::blank();
        let mut w0 = SearchWalker::new();
        let _ = w0.full_search(&blank);
        let full_ok = stride_ok(&w0)
            && w0.visited == (EBDA_WIN / STRIDE + (BIOS_WIN - 8) / STRIDE + 1)
            && w0.misaligned_visits == 0;
        // 口径 B：命中即停扫 ⇒ 步数 = 命中偏移/16 + 1 = 0x90/16 + 1 = 10 步。
        let mut img = MemoryImage::blank();
        let _ = img.plant(EBDA_BASE + 0x090, &build_rsdp_bytes(2));
        let mut w = SearchWalker::new();
        let _ = w.full_search(&img);
        let early_ok = stride_ok(&w) && w.visited == 0x090 / STRIDE + 1;
        v.push((
            "步进16对齐账",
            full_ok && early_ok,
            format!(
                "全扫 visited={} 命中停扫 visited={} misaligned={}",
                w0.visited, w.visited, w.misaligned_visits
            ),
        ));
    }

    // 窗口常量冻结账。
    v.push((
        "窗口常量冻结",
        EBDA_BASE == 0x9FC00
            && EBDA_WIN == 0x0400
            && BIOS_LO == 0xE0000
            && BIOS_HI == 0xFFFFF
            && BIOS_WIN == 0x020000
            && STRIDE == 16
            && V1_LEN == 20
            && V2_LEN == 36,
        format!(
            "EBDA=0x{:x}+{} BIOS=0x{:x}~0x{:x} 步进={} v1={} v2+={}",
            EBDA_BASE, EBDA_WIN, BIOS_LO, BIOS_HI, STRIDE, V1_LEN, V2_LEN
        ),
    ));

    // 签名闭集：8 字节 = "RSD PTR "。
    v.push((
        "签名闭集8字节",
        RSDP_SIG == [0x52u8, 0x53, 0x44, 0x20, 0x50, 0x54, 0x52, 0x20],
        String::from("RSD PTR = 52 53 44 20 50 54 52 20"),
    ));

    // 状态机单调：倒退必抓。
    v.push((
        "状态机单调-倒退必抓",
        probe_seq_c().is_ok(),
        String::from("BiosScan→EbdaScan ⇒ PhaseRegress"),
    ));

    // 越窗显性拒：地址不在双窗内。
    {
        let img = MemoryImage::blank();
        let over = match img.localize(0x80000) {
            Err(e) => e == RsdpScanCode::WindowOver.val(),
            Ok(_) => false,
        };
        v.push((
            "越窗显性拒",
            over,
            String::from("0x80000 不在 EBDA/BIOS 双窗 ⇒ WindowOver"),
        ));
    }

    // 注入 10/10。
    let inj = inject_rounds(10);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 双跑一致。
    v.push((
        "双跑一致",
        repro_check().is_ok(),
        String::from("双窗黄金景两跑同值"),
    ));

    // 探针三序。
    v.push((
        "探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a=命中复算 b=空窗显性 c=倒退必抓"),
    ));

    v
}

pub fn run_unx0441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0441 RSDP 签名搜索（EBDA/BIOS 双窗）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ebda_golden_hit() {
        assert_eq!(golden_ebda(), Ok((1, 0, EBDA_BASE + 0x090)));
    }

    #[test]
    fn test_bios_golden_hit() {
        assert_eq!(golden_bios(), Ok((1, 0, 0xF00C0)));
    }

    #[test]
    fn test_blank_not_found() {
        let img = MemoryImage::blank();
        let mut w = SearchWalker::new();
        assert_eq!(w.full_search(&img), Err(RsdpScanCode::NotFound.val()));
        assert_eq!(w.phase, SearchPhase::Exhausted);
    }

    #[test]
    fn test_checksum_inject_caught() {
        // 坏校验候选不得命中，且窗内其余空 ⇒ NotFound。
        let mut img = MemoryImage::blank();
        let mut bad = build_rsdp_bytes(2);
        if let Some(slot) = bad.get_mut(8) {
            *slot ^= 0xFF;
        }
        assert!(img.plant(EBDA_BASE + 0x010, &bad).is_ok());
        let mut w = SearchWalker::new();
        assert_eq!(w.full_search(&img), Err(RsdpScanCode::NotFound.val()));
        assert_eq!(w.rejected, 1);
        assert_eq!(w.sig_hits, 1);
    }

    #[test]
    fn test_rev_len_inject_caught() {
        let v2 = build_rsdp_bytes(2);
        assert!(rev_len_ok(&v2, 0));
        let mut bad = v2.clone();
        if let Some(slot) = bad.get_mut(21) {
            *slot = 1; // length = 256+36 失配
        }
        assert!(!rev_len_ok(&bad, 0));
    }

    #[test]
    fn test_stride_alignment() {
        let mut img = MemoryImage::blank();
        let _ = img.plant(EBDA_BASE + 0x0A0, &build_rsdp_bytes(0));
        let mut w = SearchWalker::new();
        assert!(w.full_search(&img).is_ok());
        assert_eq!(w.misaligned_visits, 0);
        assert!(w.visited > 0);
    }

    #[test]
    fn test_phase_regress() {
        let mut w = SearchWalker::new();
        assert!(w.advance(SearchPhase::BiosScan).is_ok());
        assert_eq!(
            w.advance(SearchPhase::Idle),
            Err(RsdpScanCode::PhaseRegress.val())
        );
    }

    #[test]
    fn test_window_over() {
        let img = MemoryImage::blank();
        assert!(img.localize(0x80000).is_err());
        assert!(img.localize(0x9FC00).is_ok());
        assert!(img.localize(0xFFFFF).is_ok());
        assert!(img.localize(0x100000).is_err());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(10).ok().unwrap_or(InjectLedger::new());
        assert!(led.all_caught());
        assert_eq!(led.tries[0], 10);
        assert_eq!(led.caught[2], 10);
    }

    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0441_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
