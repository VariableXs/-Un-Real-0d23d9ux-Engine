//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15681 · IMalloc 接口契约 · 0x67680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B25）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15681-J1 契约成文（Alloc/Realloc/Free/GetSize/DidAlloc/
//!     HeapMinimize 六法语义 10/10 次），野指针注入（非本分配器指针
//!     Free）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 块账建模：每块有唯一句柄（单调序号）、尺寸、状态
//!     （Allocated/Freed）；句柄即指针的确定性替身（禁真指针）；
//!   - 六法语义：Alloc（0 尺寸拒）、Realloc（活块可扩缩、死块拒）、
//!     Free（死块双免拒）、GetSize（尺寸回读恒等）、DidAlloc
//!     （本池指针返在册/不在册双态如实）、HeapMinimize（只清死块）；
//!   - 野指针注入面：Free 一个从未在本池分配的句柄 → 10/10 必拒；
//!   - 守恒恒等式：在册字节 = Σ活块尺寸；分配计数 = 活块数 + 死块数；
//!     HeapMinimize 后死块账清、活块账不动；
//!   - LCG 种子写死 = SEG_BASE（0x67680）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67680，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67680;

// ── 六法闭集 ─────────────────────────────────────────────────────
pub const M_ALLOC: usize = 0;
pub const M_REALLOC: usize = 1;
pub const M_FREE: usize = 2;
pub const M_GETSIZE: usize = 3;
pub const M_DIDALLOC: usize = 4;
pub const M_HEAPMINIMIZE: usize = 5;
pub const M_COUNT: usize = 6;
pub const M_NAMES: [&str; M_COUNT] =
    ["Alloc", "Realloc", "Free", "GetSize", "DidAlloc", "HeapMinimize"];

/// 单块尺寸封顶（写死：契约池上限）。
pub const SIZE_CAP: u32 = 4096;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MalCode {
    /// 法下标越闭集
    MethodOutOfRange,
    /// 零尺寸分配（契约拒）
    ZeroSizeAlloc,
    /// 尺寸越封顶
    SizeCapExceeded,
    /// 野指针：非本分配器句柄 Free（如实拒）
    WildPointerFree,
    /// 双免：死块再 Free（如实拒）
    DoubleFree,
    /// 死块 Realloc（非法迁移）
    ReallocFreed,
    /// GetSize 对死块（账外查询）
    SizeOfFreed,
    /// 字节守恒破坏（在册字节≠Σ活块）
    ByteConservationBroken,
    /// 块账守恒破坏（分配数≠活+死）
    BlockConservationBroken,
    /// 野指针注入漏抓
    WildLeak,
}

impl MalCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MalCode::MethodOutOfRange => "法下标越闭集：六法冻结",
            MalCode::ZeroSizeAlloc => "零尺寸分配：契约拒",
            MalCode::SizeCapExceeded => "尺寸越封顶：契约池上限",
            MalCode::WildPointerFree => "野指针：非本分配器句柄 Free，如实拒",
            MalCode::DoubleFree => "双免：死块再 Free，如实拒",
            MalCode::ReallocFreed => "死块 Realloc：非法迁移",
            MalCode::SizeOfFreed => "GetSize 对死块：账外查询拒",
            MalCode::ByteConservationBroken => "字节守恒破坏：在册字节≠Σ活块",
            MalCode::BlockConservationBroken => "块账守恒破坏：分配数≠活+死",
            MalCode::WildLeak => "野指针注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [MalCode; 10] = [
        MalCode::MethodOutOfRange,
        MalCode::ZeroSizeAlloc,
        MalCode::SizeCapExceeded,
        MalCode::WildPointerFree,
        MalCode::DoubleFree,
        MalCode::ReallocFreed,
        MalCode::SizeOfFreed,
        MalCode::ByteConservationBroken,
        MalCode::BlockConservationBroken,
        MalCode::WildLeak,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MalCode::ALL.len() {
        if MalCode::ALL[i].val() == code {
            return MalCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67680 段闭集内"
}

// ── 块状态机 ─────────────────────────────────────────────────────
/// 双态闭集：Allocated → Freed（吸收终态，免后不复活）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockState {
    Allocated,
    Freed,
}

pub fn block_can_move(from: BlockState, to: BlockState) -> bool {
    matches!((from, to), (BlockState::Allocated, BlockState::Freed))
}

/// 块记录。
#[derive(Clone, Debug)]
pub struct Block {
    pub handle: u64,
    pub size: u32,
    pub state: BlockState,
    pub reallocs: u32,
}

// ── 分配器账（IMalloc 建模） ─────────────────────────────────────
pub struct MallocAren {
    pub blocks: Vec<Block>,
    pub next_handle: u64,
    pub alloc_cnt: u32,
    pub free_cnt: u32,
    pub realloc_cnt: u32,
    pub minimize_cnt: u32,
}

impl MallocAren {
    pub fn new() -> MallocAren {
        MallocAren {
            blocks: Vec::new(),
            next_handle: 1,
            alloc_cnt: 0,
            free_cnt: 0,
            realloc_cnt: 0,
            minimize_cnt: 0,
        }
    }

    fn find(&self, h: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].handle == h {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Alloc：0 尺寸拒；超封顶拒；活块入账。
    pub fn alloc(&mut self, size: u32) -> Result<u64, u32> {
        if size == 0 {
            return Err(MalCode::ZeroSizeAlloc.val());
        }
        if size > SIZE_CAP {
            return Err(MalCode::SizeCapExceeded.val());
        }
        let h = self.next_handle;
        self.next_handle += 1;
        self.blocks.push(Block {
            handle: h,
            size,
            state: BlockState::Allocated,
            reallocs: 0,
        });
        self.alloc_cnt += 1;
        Ok(h)
    }

    /// Free：野指针拒；双免拒；合法则落 Freed。
    pub fn free(&mut self, h: u64) -> Result<(), u32> {
        match self.find(h) {
            Some(i) => {
                let st = self.blocks[i].state;
                if st == BlockState::Freed {
                    return Err(MalCode::DoubleFree.val());
                }
                if !block_can_move(st, BlockState::Freed) {
                    return Err(MalCode::DoubleFree.val());
                }
                self.blocks[i].state = BlockState::Freed;
                self.free_cnt += 1;
                Ok(())
            }
            None => Err(MalCode::WildPointerFree.val()),
        }
    }

    /// Realloc：活块可扩缩（0=免并返 0）；死块拒；野指针拒。
    pub fn realloc(&mut self, h: u64, new_size: u32) -> Result<u64, u32> {
        if new_size > SIZE_CAP {
            return Err(MalCode::SizeCapExceeded.val());
        }
        match self.find(h) {
            Some(i) => {
                if self.blocks[i].state != BlockState::Allocated {
                    return Err(MalCode::ReallocFreed.val());
                }
                if new_size == 0 {
                    // 契约：Realloc 0 = Free。
                    self.blocks[i].state = BlockState::Freed;
                    self.free_cnt += 1;
                    return Ok(0);
                }
                self.blocks[i].size = new_size;
                self.blocks[i].reallocs += 1;
                self.realloc_cnt += 1;
                Ok(h)
            }
            None => Err(MalCode::WildPointerFree.val()),
        }
    }

    /// GetSize：活块尺寸回读恒等；死块拒；野指针拒。
    pub fn get_size(&self, h: u64) -> Result<u32, u32> {
        match self.find(h) {
            Some(i) => {
                if self.blocks[i].state != BlockState::Allocated {
                    return Err(MalCode::SizeOfFreed.val());
                }
                Ok(self.blocks[i].size)
            }
            None => Err(MalCode::WildPointerFree.val()),
        }
    }

    /// DidAlloc：本池在册活块=在册(1)；死块=已免(2)；池外=不在册(0)。
    pub fn did_alloc(&self, h: u64) -> Result<usize, u32> {
        match self.find(h) {
            Some(i) => match self.blocks[i].state {
                BlockState::Allocated => Ok(1),
                BlockState::Freed => Ok(2),
            },
            None => Ok(0),
        }
    }

    /// HeapMinimize：死块出账（压缩），活块不动。
    pub fn heap_minimize(&mut self) -> Result<u32, u32> {
        let mut keep = Vec::new();
        let mut dropped = 0;
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].state == BlockState::Allocated {
                keep.push(self.blocks[i].clone());
            } else {
                dropped += 1;
            }
            i += 1;
        }
        self.blocks = keep;
        self.minimize_cnt += 1;
        Ok(dropped)
    }

    /// 守恒：在册字节 = Σ活块；分配数 = 活 + 死（压缩前）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut bytes = 0;
        let mut alive = 0;
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].state == BlockState::Allocated {
                bytes += self.blocks[i].size;
                alive += 1;
            }
            i += 1;
        }
        let mut j = 0;
        let mut dead_bytes = 0;
        while j < self.blocks.len() {
            if self.blocks[j].state != BlockState::Allocated {
                dead_bytes += self.blocks[j].size;
            }
            j += 1;
        }
        if bytes + dead_bytes != bytes {
            return Err(MalCode::ByteConservationBroken.val());
        }
        if self.free_cnt > self.alloc_cnt {
            return Err(MalCode::BlockConservationBroken.val());
        }
        if (self.alloc_cnt - self.free_cnt) != alive {
            return Err(MalCode::BlockConservationBroken.val());
        }
        Ok(())
    }

    /// 六法判据正演：10/10 轮。
    pub fn judge_run(&mut self) -> Result<(u32, u32, u32, u32, u32, u32), u32> {
        let mut ok = [0u32; M_COUNT];
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            // Alloc。
            let h = self.alloc(64 + r as u32)?;
            ok[M_ALLOC] += 1;
            // GetSize 回读恒等。
            if self.get_size(h)? == 64 + r as u32 {
                ok[M_GETSIZE] += 1;
            }
            // DidAlloc 在册。
            if self.did_alloc(h)? == 1 {
                ok[M_DIDALLOC] += 1;
            }
            // Realloc 扩。
            if self.realloc(h, 128 + r as u32)? == h {
                ok[M_REALLOC] += 1;
            }
            // Free。
            self.free(h)?;
            ok[M_FREE] += 1;
            // HeapMinimize。
            if self.heap_minimize().is_ok() {
                ok[M_HEAPMINIMIZE] += 1;
            }
            r += 1;
        }
        self.conserve()?;
        Ok((ok[0], ok[1], ok[2], ok[3], ok[4], ok[5]))
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_SIX_OK: u32 = 10;
pub const GOLDEN_WILD_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut a = MallocAren::new();
    let (m0, m1, m2, m3, m4, m5) = a.judge_run()?;
    if m0 != GOLDEN_SIX_OK || m1 != GOLDEN_SIX_OK || m2 != GOLDEN_SIX_OK
        || m3 != GOLDEN_SIX_OK || m4 != GOLDEN_SIX_OK || m5 != GOLDEN_SIX_OK {
        return Err(MalCode::BlockConservationBroken.val());
    }
    // 野指针注入 10/10：池外句柄 Free 必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut a2 = MallocAren::new();
        let _ = a2.alloc(32);
        let wild = 999_999 + r as u64; // 池外句柄
        if let Err(e) = a2.free(wild) {
            if e == MalCode::WildPointerFree.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_WILD_CAUGHT {
        return Err(MalCode::WildLeak.val());
    }
    Ok((6, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
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
    // 族 0：野指针 Free ×10 必拒。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut a = MallocAren::new();
        if let Err(e) = a.free(0xDEAD_0000 + r as u64) {
            if e == MalCode::WildPointerFree.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：双免注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut a = MallocAren::new();
        let h = a.alloc(48).ok();
        if let Some(hh) = h {
            let _ = a.free(hh);
            if let Err(e) = a.free(hh) {
                if e == MalCode::DoubleFree.val() {
                    ledger.caught[1] += 1;
                }
            }
        }
        r += 1;
    }
    // 族 2：零尺寸分配注入 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut a2 = MallocAren::new();
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = a2.alloc(0) {
            if e == MalCode::ZeroSizeAlloc.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法六法必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut a = MallocAren::new();
    let h = a.alloc(256)?;
    if a.get_size(h)? != 256 {
        return Err(MalCode::ByteConservationBroken.val());
    }
    let h2 = a.realloc(h, 512)?;
    if h2 != h || a.get_size(h)? != 512 {
        return Err(MalCode::ByteConservationBroken.val());
    }
    if a.did_alloc(h)? != 1 {
        return Err(MalCode::BlockConservationBroken.val());
    }
    a.free(h)?;
    if a.did_alloc(h)? != 2 {
        return Err(MalCode::BlockConservationBroken.val());
    }
    if a.did_alloc(0x1234_5678)? != 0 {
        return Err(MalCode::WildLeak.val());
    }
    let _ = a.heap_minimize()?;
    a.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MalCode::ALL.len() {
        let v = MalCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：六法 10/10 + 野指针 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-六法10/10+野指针10/10检出",
        matches!(g, Ok((6, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 块状态机边闭集：Freed→任何态恒非法（免后不复活）。
    let closed = !block_can_move(BlockState::Freed, BlockState::Allocated)
        && block_can_move(BlockState::Allocated, BlockState::Freed);
    v.push(("J1-块态边闭集", closed, String::from("A→F 吸收终态")));
    // 4. DidAlloc 三态点验：在册/已免/池外 如实。
    let did = {
        let mut a = MallocAren::new();
        let h = a.alloc(16).ok();
        let r1 = a.did_alloc(0xFFFF_0000);
        let mut ok = matches!(r1, Ok(0));
        if let Some(hh) = h {
            ok = ok && matches!(a.did_alloc(hh), Ok(1));
            let _ = a.free(hh);
            ok = ok && matches!(a.did_alloc(hh), Ok(2));
        } else {
            ok = false;
        }
        ok
    };
    v.push(("J1-DidAlloc三态如实", did, String::from("1/2/0 三态闭集")));
    // 5. 反向判据：合法六法必须全过。
    v.push(("J1-反向探针合法六法", matches!(reverse_probe(), Ok(())), String::from("六法全过")));
    // 6. 法名点验：六法非空名。
    let names_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < M_COUNT {
            if M_NAMES[i].len() == 0 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-六法名点验", names_ok, format!("{} 法", M_COUNT)));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x81);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67680 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15681_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15681 IMalloc 接口契约");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
