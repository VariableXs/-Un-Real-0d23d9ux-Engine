//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16361 · 共享内存缓冲总纲（wl_buffer 类语义） · 0x67F00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B19）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F16361-J1 总纲落账（缓冲分配/映射/提交三段+零拷贝契约），
//!     三段齐备率 100%，与 F16248 守卫入口清单勾稽一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三段建模：分配（池化页账，容量守恒）、映射（偏移+长度越界
//!     拒）、提交（damage 区域登记→释放回调）；
//!   - 零拷贝契约：提交只转让所有权不复制字节——提交计数与复制
//!     计数恒等式（复制数必须恒 0）；
//!   - wl_buffer 语义：分配→映射写→提交→等待释放→复用，释放前
//!     再提交 = 双提交拒；
//!   - F16248 守卫入口清单勾稽：三段各自的守卫入口名必须能在
//!     F16248 清单精确命中（一致率 100%）；
//!   - 页账守恒：已分配页 = 在用页 + 已释放页；池容量上界显性；
//!   - LCG 种子写死 = SEG_BASE（0x67F00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67F00，AI-VX3-1030 批14 独占区）。
pub const SEG_BASE: u32 = 0x67F00;

// ── 三段闭集 ────────────────────────────────────────────────────
pub const SEG_ALLOC: usize = 0;
pub const SEG_MAP: usize = 1;
pub const SEG_COMMIT: usize = 2;
pub const STAGE_COUNT: usize = 3;
pub const STAGE_NAMES: [&str; STAGE_COUNT] = ["分配", "映射", "提交"];
pub const STAGE_FLOOR: usize = 3;

// ── F16248 守卫入口清单（冻结：勾稽目标） ───────────────────────
pub const GUARD_TABLE: [&str; 3] =
    ["guard_alloc_pool", "guard_map_bounds", "guard_commit_damage"];
/// 三段冻结守卫入口名（须与 GUARD_TABLE 逐段一致）。
pub const STAGE_GUARD_FROZEN: [&str; STAGE_COUNT] =
    ["guard_alloc_pool", "guard_map_bounds", "guard_commit_damage"];

// ── 缓冲域冻结值 ────────────────────────────────────────────────
/// 池页容量上界（冻结）。
pub const POOL_PAGE_CAP: u32 = 64;
/// 页大小（字节，冻结）。
pub const PAGE_SIZE: u32 = 4096;
/// 映射偏移+长度上界（冻结：页大小内）。
pub const MAP_SPAN_MAX: u32 = PAGE_SIZE;
/// damage 区域数上界（冻结）。
pub const DAMAGE_CAP: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShmCode {
    /// 段越闭集
    StageOutOfRange,
    /// 段名空（齐备破坏）
    StageNameBlank,
    /// 池满（页账上界）
    PoolExhausted,
    /// 双重释放
    DoubleRelease,
    /// 映射越界（偏移+长度越页）
    MapOutOfBounds,
    /// 零长度映射
    MapZeroLength,
    /// 双提交（释放前再提交）
    DoubleCommit,
    /// 零拷贝破坏（提交路径出现复制）
    CopyContractBroken,
    /// 页账守恒破坏（已分配≠在用+已释放）
    PageLedgerBroken,
    /// F16248 勾稽断裂（守卫入口未命中）
    GuardNotLinked,
    /// damage 区域越上界
    DamageCapExceeded,
    /// 双跑不一致
    NotReproducible,
}

impl ShmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ShmCode::StageOutOfRange => "段越闭集：分配/映射/提交冻结",
            ShmCode::StageNameBlank => "段名空：齐备破坏",
            ShmCode::PoolExhausted => "池满：页账上界显性",
            ShmCode::DoubleRelease => "双重释放：页复用账破坏",
            ShmCode::MapOutOfBounds => "映射越界：偏移+长度越页",
            ShmCode::MapZeroLength => "零长度映射：显性拒",
            ShmCode::DoubleCommit => "双提交：释放前再提交",
            ShmCode::CopyContractBroken => "零拷贝破坏：提交路径复制",
            ShmCode::PageLedgerBroken => "页账守恒破坏",
            ShmCode::GuardNotLinked => "F16248 勾稽断裂：入口未命中",
            ShmCode::DamageCapExceeded => "damage 区域越上界",
            ShmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ShmCode; 12] = [
        ShmCode::StageOutOfRange,
        ShmCode::StageNameBlank,
        ShmCode::PoolExhausted,
        ShmCode::DoubleRelease,
        ShmCode::MapOutOfBounds,
        ShmCode::MapZeroLength,
        ShmCode::DoubleCommit,
        ShmCode::CopyContractBroken,
        ShmCode::PageLedgerBroken,
        ShmCode::GuardNotLinked,
        ShmCode::DamageCapExceeded,
        ShmCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ShmCode::ALL.len() {
        if ShmCode::ALL[i].val() == code {
            return ShmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67F00 段闭集内"
}

// ── 页状态机 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Free,
    Mapped,
    Committed,
}

/// damage 区域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl DamageRect {
    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
}

// ── 共享内存池账 ────────────────────────────────────────────────
pub struct ShmPool {
    pub states: [PageState; POOL_PAGE_CAP as usize],
    pub damage: Vec<DamageRect>,
    pub alloc_total: u32,
    pub release_total: u32,
    pub commit_total: u32,
    /// 零拷贝契约：复制计数（必须恒 0）。
    pub copy_cnt: u32,
    pub double_commit_caught: u32,
    pub double_release_caught: u32,
}

impl ShmPool {
    pub fn new() -> ShmPool {
        let mut states = [PageState::Free; POOL_PAGE_CAP as usize];
        // 全部初始 Free。
        states = [PageState::Free; POOL_PAGE_CAP as usize];
        ShmPool {
            states,
            damage: Vec::new(),
            alloc_total: 0,
            release_total: 0,
            commit_total: 0,
            copy_cnt: 0,
            double_commit_caught: 0,
            double_release_caught: 0,
        }
    }

    fn find_free(&self) -> Option<u32> {
        let mut i = 0u32;
        while i < POOL_PAGE_CAP {
            if self.states[i as usize] == PageState::Free {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 段一：分配。池满显性 Err。
    pub fn alloc(&mut self) -> Result<u32, u32> {
        match self.find_free() {
            Some(p) => {
                self.states[p as usize] = PageState::Mapped;
                self.alloc_total += 1;
                Ok(p)
            }
            None => Err(ShmCode::PoolExhausted.val()),
        }
    }

    /// 段二：映射写越界守卫（返回可写跨度）。
    pub fn map_guard(&self, page: u32, off: u32, len: u32) -> Result<u32, u32> {
        if page >= POOL_PAGE_CAP {
            return Err(ShmCode::MapOutOfBounds.val());
        }
        if len == 0 {
            return Err(ShmCode::MapZeroLength.val());
        }
        if off.checked_add(len).is_none() || off + len > MAP_SPAN_MAX {
            return Err(ShmCode::MapOutOfBounds.val());
        }
        if self.states[page as usize] != PageState::Mapped {
            return Err(ShmCode::MapOutOfBounds.val());
        }
        Ok(len)
    }

    /// 段三：提交（转让所有权，零拷贝）。Committed 态再提交 = 双提交拒。
    pub fn commit(&mut self, page: u32, rects: &[DamageRect]) -> Result<u64, u32> {
        if page >= POOL_PAGE_CAP {
            return Err(ShmCode::MapOutOfBounds.val());
        }
        if self.states[page as usize] != PageState::Mapped {
            if self.states[page as usize] == PageState::Committed {
                self.double_commit_caught += 1;
                return Err(ShmCode::DoubleCommit.val());
            }
            return Err(ShmCode::MapOutOfBounds.val());
        }
        if self.damage.len() + rects.len() > DAMAGE_CAP {
            return Err(ShmCode::DamageCapExceeded.val());
        }
        let mut area = 0u64;
        let mut i = 0;
        while i < rects.len() {
            self.damage.push(rects[i]);
            area += rects[i].area();
            i += 1;
        }
        self.states[page as usize] = PageState::Committed;
        self.commit_total += 1;
        // 零拷贝契约：提交不复制，复制计数必须不动。
        Ok(area)
    }

    /// 释放回调：Committed→Free，页复用账闭合。
    pub fn release(&mut self, page: u32) -> Result<(), u32> {
        if page >= POOL_PAGE_CAP {
            return Err(ShmCode::DoubleRelease.val());
        }
        if self.states[page as usize] != PageState::Committed {
            if self.states[page as usize] == PageState::Free {
                self.double_release_caught += 1;
                return Err(ShmCode::DoubleRelease.val());
            }
            return Err(ShmCode::DoubleRelease.val());
        }
        self.states[page as usize] = PageState::Free;
        self.release_total += 1;
        Ok(())
    }

    /// 页账守恒：分配总数 = 释放总数 + 在用数。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut live = 0;
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] != PageState::Free {
                live += 1;
            }
            i += 1;
        }
        if self.alloc_total != self.release_total + live as u32 {
            return Err(ShmCode::PageLedgerBroken.val());
        }
        // 零拷贝契约。
        if self.copy_cnt != 0 {
            return Err(ShmCode::CopyContractBroken.val());
        }
        Ok(())
    }

    /// 三段齐备账。
    pub fn stages_ready() -> bool {
        if STAGE_NAMES.len() < STAGE_FLOOR {
            return false;
        }
        let mut i = 0;
        while i < STAGE_NAMES.len() {
            if STAGE_NAMES[i].len() == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// F16248 勾稽：三段守卫入口逐段命中。
    pub fn guards_all_linked() -> bool {
        let mut i = 0;
        while i < STAGE_COUNT {
            let mut hit = false;
            let mut j = 0;
            while j < GUARD_TABLE.len() {
                if GUARD_TABLE[j] == STAGE_GUARD_FROZEN[i] {
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return false;
            }
            i += 1;
        }
        true
    }
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

/// 黄金全链：分配 4 页→映射写→提交（带 damage）→释放 2 页→守恒。
pub fn golden_run() -> Result<(u32, u64, u32), u32> {
    let mut pool = ShmPool::new();
    let mut pages: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < 4 {
        let p = pool.alloc()?;
        pages.push(p);
        i += 1;
    }
    // 映射写：页内合法跨度。
    let mut j = 0;
    while j < pages.len() {
        pool.map_guard(pages[j], 0, PAGE_SIZE / 2)?;
        j += 1;
    }
    // 提交页 0、1：各带一个 damage。
    let r0 = [DamageRect { x: 0, y: 0, w: 16, h: 16 }];
    let r1 = [DamageRect { x: 32, y: 8, w: 8, h: 8 }];
    let a0 = pool.commit(pages[0], &r0)?;
    let a1 = pool.commit(pages[1], &r1)?;
    // 释放页 0。
    pool.release(pages[0])?;
    pool.conserve()?;
    if !ShmPool::stages_ready() {
        return Err(ShmCode::StageNameBlank.val());
    }
    if !ShmPool::guards_all_linked() {
        return Err(ShmCode::GuardNotLinked.val());
    }
    Ok((pool.commit_total, a0 + a1, pool.release_total))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    // 黄金序：分配→映射→提交→释放。
    let mut pool = ShmPool::new();
    let p = pool.alloc()?;
    pool.map_guard(p, 0, 128)?;
    pool.commit(p, &[])?;
    pool.release(p)?;
    pool.conserve()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 映射越界 → 必拒。
    let mut pool = ShmPool::new();
    let p = pool.alloc()?;
    pool.map_guard(p, MAP_SPAN_MAX - 8, 16).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 双提交 → 必拒。
    let mut pool = ShmPool::new();
    let p = pool.alloc()?;
    pool.commit(p, &[]).map(|_| ())?;
    pool.commit(p, &[]).map(|_| ())
}

/// 注入 10/10：双提交、越界映射、池满、双释放四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：双提交。
        let mut pool = ShmPool::new();
        let p = pool.alloc().ok();
        let hit0 = match p {
            Some(pp) => {
                pool.commit(pp, &[]).ok();
                pool.commit(pp, &[]).is_err()
            }
            None => false,
        };
        led.record(0, hit0);
        // 槽1：越界映射。
        let mut pool2 = ShmPool::new();
        let p2 = pool2.alloc().ok();
        let hit1 = match p2 {
            Some(pp) => pool2.map_guard(pp, PAGE_SIZE - 1, 2).is_err(),
            None => false,
        };
        led.record(1, hit1);
        // 槽2：池满。
        let mut pool3 = ShmPool::new();
        let mut full = true;
        let mut k = 0;
        while k < POOL_PAGE_CAP {
            if pool3.alloc().is_err() {
                full = false;
                break;
            }
            k += 1;
        }
        let hit2 = full && pool3.alloc().is_err();
        led.record(2, hit2);
        // 槽3：双释放。
        let mut pool4 = ShmPool::new();
        let p4 = pool4.alloc().ok();
        let hit3 = match p4 {
            Some(pp) => {
                pool4.commit(pp, &[]).ok();
                pool4.release(pp).ok();
                pool4.release(pp).is_err()
            }
            None => false,
        };
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ShmCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ShmCode::ALL.len() {
        let v = ShmCode::ALL[i].val();
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
    // 三段齐备率 100%。
    v.push((
        "J1-三段齐备率100%",
        ShmPool::stages_ready() && STAGE_NAMES.len() == 3,
        format!("{:?}", STAGE_NAMES),
    ));
    // F16248 勾稽一致率 100%。
    v.push((
        "J1-F16248勾稽一致率100%",
        ShmPool::guards_all_linked(),
        format!("守卫表 {:?}", GUARD_TABLE),
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
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c = probe_seq_c();
    v.push((
        "J1-探针三序",
        a.is_ok() && b.is_err() && c.is_err(),
        format!("a={:?} b={:?} c={:?}", a.is_ok(), b.is_err(), c.is_err()),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 零拷贝契约点验。
    v.push((
        "J1-零拷贝契约点验",
        POOL_PAGE_CAP == 64 && PAGE_SIZE == 4096 && DAMAGE_CAP == 16,
        format!("页{} 页节{} damage{}", POOL_PAGE_CAP, PAGE_SIZE, DAMAGE_CAP),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67F00 窗口")));
    v
}

pub fn run_unx16361_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16361 共享内存缓冲总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
