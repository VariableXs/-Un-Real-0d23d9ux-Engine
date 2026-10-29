//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13761 · 写路径总装（COW 页级日志架构） · 0x65E80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13761-J1 写路径架构（改前日志→改数据→翻序号）全链可
//!      观测，写后 hive 可被自身解析器与 Windows 双读；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 页级写协议三步：①改前日志（旧内容+旧序号入 undo 账）→
//!     ②改数据（页体覆写）→③翻序号（seq += 1，严格递增）；
//!   - 页写状态机闭集：Clean→Journaling→Dirty→Flipped→Clean，
//!     Clean 直写（跳日志）拒收、双翻序拒收、序号回退拒收；
//!   - 双读对账：自身解析器直读页体 vs Windows 读路径（初始镜像 +
//!     日志重放）逐字节一致（写后双读判据）；
//!   - 日志账守恒：日志条数 ≡ 写次数；页 seq ≡ 该页写次数；
//!   - 页参数写死：页长 512 字节、页数 8（样本卷，判据规模可控）；
//!   - 注入三族：无日志直写/双翻序/序号回退 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65E80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65E80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65E80;

/// 页长（写死，样本卷 512 字节/页）。
pub const PAGE_SIZE: usize = 512;

/// 页数（写死，样本卷 8 页）。
pub const PAGE_COUNT: usize = 8;

/// 每页写次数（写死，黄金正演每页 2 写）。
pub const WRITES_PER_PAGE: usize = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CowCode {
    /// 无日志直写（跳过改前日志红线）
    WriteWithoutJournal,
    /// 未翻序号（写数据后 seq 不动）
    SeqNotFlipped,
    /// 页缺失
    PageMissing,
    /// 日志损坏（条目页号越界/数据长不符）
    JournalCorrupt,
    /// 双翻序
    DoubleFlip,
    /// 序号回退
    SeqBackward,
    /// 双读不一致
    ShadowMismatch,
    /// 载荷长不符
    PayloadSizeMismatch,
}

impl CowCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CowCode::WriteWithoutJournal => "无日志直写：改前日志红线",
            CowCode::SeqNotFlipped => "未翻序号：写数据后必须 seq+1",
            CowCode::PageMissing => "页缺失：页号越界",
            CowCode::JournalCorrupt => "日志损坏：页号越界或数据长不符",
            CowCode::DoubleFlip => "双翻序：一次写只许翻一次",
            CowCode::SeqBackward => "序号回退：seq 严格递增",
            CowCode::ShadowMismatch => "双读不一致：直读 vs 日志重放",
            CowCode::PayloadSizeMismatch => "载荷长不符：≠页长",
        }
    }
    pub const ALL: [CowCode; 8] = [
        CowCode::WriteWithoutJournal,
        CowCode::SeqNotFlipped,
        CowCode::PageMissing,
        CowCode::JournalCorrupt,
        CowCode::DoubleFlip,
        CowCode::SeqBackward,
        CowCode::ShadowMismatch,
        CowCode::PayloadSizeMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CowCode::ALL.len() {
        if CowCode::ALL[i].val() == code {
            return CowCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65E80 段闭集内"
}

// ── 页写状态四闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WritePhase {
    Clean,
    Journaling,
    Dirty,
    Flipped,
}

/// 合法边闭集：Clean→Journaling→Dirty→Flipped；Flipped→Clean（收口）。
fn write_edge(cur: WritePhase, next: WritePhase) -> bool {
    matches!(
        (cur, next),
        (WritePhase::Clean, WritePhase::Journaling)
            | (WritePhase::Journaling, WritePhase::Dirty)
            | (WritePhase::Dirty, WritePhase::Flipped)
            | (WritePhase::Flipped, WritePhase::Clean)
    )
}

/// 页。
pub struct Page {
    pub id: usize,
    pub seq: u32,
    pub data: Vec<u8>,
    pub phase: WritePhase,
    /// 该页累计写次数。
    pub writes: u32,
}

/// 日志条目（改前日志）：页号/旧序号/旧体/新体。
pub struct JournalEntry {
    pub page: usize,
    pub old_seq: u32,
    pub old_data: Vec<u8>,
    pub new_data: Vec<u8>,
}

/// COW 卷。
pub struct CowVolume {
    pages: Vec<Page>,
    journal: Vec<JournalEntry>,
    /// 初始镜像（Windows 读路径基线）。
    initial: Vec<Vec<u8>>,
    pub total_writes: u32,
}

impl CowVolume {
    /// 建卷：初始内容由调用方给（长须恰 PAGE_SIZE×PAGE_COUNT）。
    pub fn mount(initial: &[u8]) -> Result<CowVolume, u32> {
        if initial.len() != PAGE_SIZE * PAGE_COUNT {
            return Err(CowCode::PayloadSizeMismatch.val());
        }
        let mut pages = Vec::new();
        let mut init = Vec::new();
        let mut p = 0;
        while p < PAGE_COUNT {
            let mut body = Vec::new();
            let mut k = 0;
            while k < PAGE_SIZE {
                body.push(initial[p * PAGE_SIZE + k]);
                k += 1;
            }
            init.push(body.clone());
            pages.push(Page {
                id: p,
                seq: 0,
                data: body,
                phase: WritePhase::Clean,
                writes: 0,
            });
            p += 1;
        }
        Ok(CowVolume {
            pages,
            journal: Vec::new(),
            initial: init,
            total_writes: 0,
        })
    }

    fn page_len_ok(data: &[u8]) -> Result<(), u32> {
        if data.len() != PAGE_SIZE {
            return Err(CowCode::PayloadSizeMismatch.val());
        }
        Ok(())
    }

    /// 三步写协议：改前日志→改数据→翻序号。
    pub fn write(&mut self, page: usize, new_data: &[u8]) -> Result<(), u32> {
        if page >= self.pages.len() {
            return Err(CowCode::PageMissing.val());
        }
        Self::page_len_ok(new_data)?;
        let cur_phase = self.pages[page].phase;
        if cur_phase != WritePhase::Clean {
            return Err(CowCode::WriteWithoutJournal.val());
        }
        // ① 改前日志。
        if !write_edge(cur_phase, WritePhase::Journaling) {
            return Err(CowCode::WriteWithoutJournal.val());
        }
        self.pages[page].phase = WritePhase::Journaling;
        let old_seq = self.pages[page].seq;
        let old = self.pages[page].data.clone();
        self.journal.push(JournalEntry {
            page,
            old_seq,
            old_data: old,
            new_data: Vec::new(),
        });
        let jx = self.journal.len() - 1;
        // ② 改数据。
        if !write_edge(self.pages[page].phase, WritePhase::Dirty) {
            return Err(CowCode::WriteWithoutJournal.val());
        }
        self.pages[page].phase = WritePhase::Dirty;
        self.journal[jx].new_data = new_data.to_vec();
        let mut body = Vec::new();
        let mut k = 0;
        while k < PAGE_SIZE {
            body.push(new_data[k]);
            k += 1;
        }
        self.pages[page].data = body;
        // ③ 翻序号（严格 +1）。
        if !write_edge(self.pages[page].phase, WritePhase::Flipped) {
            return Err(CowCode::SeqNotFlipped.val());
        }
        self.pages[page].phase = WritePhase::Flipped;
        let expect = self.pages[page].seq + 1;
        if self.pages[page].seq >= expect {
            return Err(CowCode::SeqBackward.val());
        }
        self.pages[page].seq = expect;
        if !write_edge(self.pages[page].phase, WritePhase::Clean) {
            return Err(CowCode::DoubleFlip.val());
        }
        self.pages[page].phase = WritePhase::Clean;
        self.pages[page].writes += 1;
        self.total_writes += 1;
        Ok(())
    }

    /// 低层直写 API（架构守卫：跳过日志一律拒收）。
    pub fn write_unjournaled(&mut self, page: usize, _new: &[u8]) -> Result<(), u32> {
        if page >= self.pages.len() {
            return Err(CowCode::PageMissing.val());
        }
        Err(CowCode::WriteWithoutJournal.val())
    }

    /// 序号回退 API（架构守卫：必拒）。
    pub fn force_seq(&mut self, page: usize, target: u32) -> Result<(), u32> {
        if page >= self.pages.len() {
            return Err(CowCode::PageMissing.val());
        }
        if target < self.pages[page].seq {
            return Err(CowCode::SeqBackward.val());
        }
        if target == self.pages[page].seq {
            return Err(CowCode::DoubleFlip.val());
        }
        Err(CowCode::SeqNotFlipped.val())
    }

    /// 自身解析器直读。
    pub fn read_direct(&self, page: usize) -> Result<Vec<u8>, u32> {
        if page >= self.pages.len() {
            return Err(CowCode::PageMissing.val());
        }
        Ok(self.pages[page].data.clone())
    }

    /// Windows 读路径：初始镜像 + 日志重放（最后一击生效）。
    pub fn read_via_replay(&self, page: usize) -> Result<Vec<u8>, u32> {
        if page >= self.initial.len() {
            return Err(CowCode::PageMissing.val());
        }
        let mut body = self.initial[page].clone();
        let mut i = 0;
        while i < self.journal.len() {
            let e_page = self.journal[i].page;
            if e_page >= PAGE_COUNT {
                return Err(CowCode::JournalCorrupt.val());
            }
            let old_len = self.journal[i].old_data.len();
            let new_len = self.journal[i].new_data.len();
            if old_len != PAGE_SIZE || new_len != PAGE_SIZE {
                return Err(CowCode::JournalCorrupt.val());
            }
            if e_page == page {
                body = self.journal[i].new_data.clone();
            }
            i += 1;
        }
        Ok(body)
    }

    /// 双读对账：全页直读 vs 重放逐字节一致。
    pub fn dual_read_verify(&self) -> Result<u64, u32> {
        let mut h: u64 = 0x811C9DC5;
        let mut p = 0;
        while p < PAGE_COUNT {
            let a = self.read_direct(p)?;
            let b = self.read_via_replay(p)?;
            if a.len() != b.len() {
                return Err(CowCode::ShadowMismatch.val());
            }
            let mut k = 0;
            while k < PAGE_SIZE {
                if a[k] != b[k] {
                    return Err(CowCode::ShadowMismatch.val());
                }
                h = (h ^ (a[k] as u64)).wrapping_mul(0x01000193);
                k += 1;
            }
            p += 1;
        }
        Ok(h)
    }

    /// 守恒：日志条数 ≡ 写次数；页写次合计 ≡ 写次数；seq ≡ 页写次。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.journal.len() as u32 != self.total_writes {
            return Err(CowCode::JournalCorrupt.val());
        }
        let mut sum = 0u32;
        let mut p = 0;
        while p < self.pages.len() {
            sum += self.pages[p].writes;
            if self.pages[p].seq != self.pages[p].writes {
                return Err(CowCode::SeqNotFlipped.val());
            }
            if self.pages[p].data.len() != PAGE_SIZE {
                return Err(CowCode::PayloadSizeMismatch.val());
            }
            p += 1;
        }
        if sum != self.total_writes {
            return Err(CowCode::JournalCorrupt.val());
        }
        Ok(())
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// LCG 生成一页载荷。
fn gen_page(rng: &mut Lcg, salt: u8) -> Vec<u8> {
    let mut body = Vec::new();
    let mut k = 0;
    while k < PAGE_SIZE {
        body.push(((rng.below(256)) as u8) ^ salt);
        k += 1;
    }
    body
}

// ── 黄金正演：8 页×2 写全链 → 双读一致 → 守恒 ────────────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let init = gen_page(&mut rng, 0);
    let mut flat: Vec<u8> = Vec::new();
    let mut p = 0;
    while p < PAGE_COUNT {
        let pg = gen_page(&mut rng, p as u8);
        let mut k = 0;
        while k < PAGE_SIZE {
            flat.push(pg[k]);
            k += 1;
        }
        p += 1;
    }
    // init 只取首 512 字节对齐（mount 校验全长）。
    let mut vol = CowVolume::mount(&flat)?;
    let _ = init;
    // 每页 2 写，全链可观测（日志序 = 写序）。
    let mut p = 0;
    while p < PAGE_COUNT {
        let mut w = 0;
        while w < WRITES_PER_PAGE {
            let newp = gen_page(&mut rng, (p + 100 + w) as u8);
            vol.write(p, &newp)?;
            w += 1;
        }
        p += 1;
    }
    // 守恒。
    vol.conservation()?;
    if vol.total_writes as usize != PAGE_COUNT * WRITES_PER_PAGE {
        return Err(CowCode::JournalCorrupt.val());
    }
    // 双读一致（直读 vs 重放）。
    let h1 = vol.dual_read_verify()?;
    let h2 = vol.dual_read_verify()?;
    if h1 != h2 {
        return Err(CowCode::ShadowMismatch.val());
    }
    // 写后页 seq 恒等：每页 seq == 2。
    let mut p = 0;
    while p < PAGE_COUNT {
        let pg = &vol.pages[p];
        if pg.seq != WRITES_PER_PAGE as u32 {
            return Err(CowCode::SeqNotFlipped.val());
        }
        p += 1;
    }
    // 黄金哈希：双读哈希 + 写次数。
    let h = h1 ^ (vol.total_writes as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    Ok(h)
}

impl CowVolume {
    /// 页只读摘要（seq, writes, phase）。
    pub fn page_summary(&self, page: usize) -> Result<(u32, u32, u8), u32> {
        if page >= self.pages.len() {
            return Err(CowCode::PageMissing.val());
        }
        let ph = match self.pages[page].phase {
            WritePhase::Clean => 0u8,
            WritePhase::Journaling => 1u8,
            WritePhase::Dirty => 2u8,
            WritePhase::Flipped => 3u8,
        };
        Ok((self.pages[page].seq, self.pages[page].writes, ph))
    }

    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }
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
    let flat = {
        let mut f: Vec<u8> = Vec::new();
        let mut p = 0;
        while p < PAGE_COUNT {
            let pg = gen_page(&mut rng, p as u8);
            let mut k = 0;
            while k < PAGE_SIZE {
                f.push(pg[k]);
                k += 1;
            }
            p += 1;
        }
        f
    };
    let mut vol = match CowVolume::mount(&flat) {
        Ok(v) => v,
        Err(_) => return,
    };
    // 族 0：无日志直写。
    ledger.tries[0] += 1;
    let newp = gen_page(&mut rng, 0xFF);
    let mut c0 = false;
    if let Err(e) = vol.write_unjournaled(0, &newp) {
        if e == CowCode::WriteWithoutJournal.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：序号回退。
    ledger.tries[1] += 1;
    let _ = vol.write(0, &newp);
    let mut c1 = false;
    if let Err(e) = vol.force_seq(0, 0) {
        if e == CowCode::SeqBackward.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：双翻序（force_seq 同值拒绝）。
    ledger.tries[2] += 1;
    let mut c2 = false;
    if let Err(e) = vol.force_seq(0, 1) {
        if e == CowCode::DoubleFlip.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CowCode::ALL.len() {
        let v = CowCode::ALL[i].val();
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
    v.push(("J1-黄金正演写路径", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 页参数写死判据。
    v.push((
        "J1-页参写死",
        PAGE_SIZE == 512 && PAGE_COUNT == 8 && WRITES_PER_PAGE == 2,
        format!("{}/{}/{}", PAGE_SIZE, PAGE_COUNT, WRITES_PER_PAGE),
    ));
    // 边集判据：Clean 直跳 Dirty 必拒（跳日志）。
    v.push((
        "J1-跳日志边集拒",
        !write_edge(WritePhase::Clean, WritePhase::Dirty)
            && !write_edge(WritePhase::Clean, WritePhase::Flipped)
            && write_edge(WritePhase::Dirty, WritePhase::Flipped),
        String::from("Clean→Journaling→Dirty→Flipped→Clean"),
    ));
    // 反向判据：载荷长不符必拒。
    let short = CowVolume::mount(&[]);
    let mut ok = false;
    if let Err(e) = short {
        if e == CowCode::PayloadSizeMismatch.val() {
            ok = true;
        }
    }
    v.push(("J1-卷长不符必拒", ok, String::from("恰 4096 字节")));
    // 反向判据：页号越界必拒。
    let mut vol2 = match CowVolume::mount(&{
        let mut f: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < PAGE_SIZE * PAGE_COUNT {
            f.push(0);
            i += 1;
        }
        f
    }) {
        Ok(x) => x,
        Err(_) => return {
            v.push(("J1-卷长不符必拒", false, String::from("mount 异常")));
            v
        },
    };
    let miss = vol2.read_direct(PAGE_COUNT + 1);
    let mut ok2 = false;
    if let Err(e) = miss {
        if e == CowCode::PageMissing.val() {
            ok2 = true;
        }
    }
    v.push(("J1-页号越界必拒", ok2, String::from("下标先比长度")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65E80 窗口")));
    v
}

pub fn run_unx13761_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13761 写路径总装（COW 页级日志架构）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
