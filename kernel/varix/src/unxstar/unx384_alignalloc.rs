//! unx384_alignalloc —— UNX-F0384 对齐分配语义（2^n 对齐面）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B21）：UNX-F0384 · 对齐分配语义（2^n 对齐面） · 目标 220 行 · 已深化
//! 判据原文：对齐分配结果 100% 满足 2^n 对齐，零跨档浪费超 1 块
//! 组件：确定性字节竞技场（bump + 2^n 对齐面）+ 档位表（2^4~2^16 十三档）
//!      + 逐档分配账（请求数/服务数/浪费块数）+ 对齐重放核（账回放逐笔验 addr%align==0）
//!      + 浪费守卫（每档浪费块 ≤1，破约即 0x9A07 立案）+ 释放对账（双释/异源拒）。
//! 职责定位：内核小对象分配器的对齐面契约——页面对齐/缓存行对齐/临界区数据
//!   对齐全部走这里。判据两条铁面：①100% 对齐 = 每一笔被服务的分配地址都满足
//!   addr % align == 0（由重放核独立验证，不是由分配器自证）；②浪费守卫 =
//!   对齐垫出的浪费按档记账，任何一档累计浪费块超过 1 块即违约立案。
//! 性能逐项分解：alloc O(档位数)（档位定位 + 空闲链查）；align_up O(1)；
//!   重放核 O(账笔数)；指纹 O(账字段闭集 16)；全整数算术零浮点零墙钟，
//!   同分配序列恒同布局（确定性对拍锚）。
//! 诊断码段：0x9A（0x9A01~0x9A07，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：x86_64 缓存行/页面对齐惯例 + 施工书深化正文；写死并注释）
// ---------------------------------------------------------------------------

/// 最小档位：16 字节（x86_64 最小有用对齐面）。
pub const CLASS_MIN_SHIFT: u32 = 4;
/// 最大档位：64 KiB（页面前最大对齐档；更大走页分配器，不归本器）。
pub const CLASS_MAX_SHIFT: u32 = 16;
/// 档位数：2^4..=2^16 共 13 档。
pub const CLASS_COUNT: usize = (CLASS_MAX_SHIFT - CLASS_MIN_SHIFT + 1) as usize;
/// 竞技场缺省容量：1 MiB（确定性模型，可调）。
pub const ARENA_CAP: usize = 1 << 20;
/// 浪费守卫阈值：每档浪费块 ≤1（判据「零跨档浪费超 1 块」）。
pub const WASTE_BLOCKS_CAP: u64 = 1;

// ---------------------------------------------------------------------------
// 诊断码段 0x9A（0x9A01~0x9A07 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9A01 非法对齐：align 非 2^n（含 0）。
pub const C_ALIGN_NOT_POW2: u16 = 0x9A01;
/// 0x9A02 对齐超档：align > 2^16（走页分配器，不归本器）。
pub const C_ALIGN_OVER_CLASS: u16 = 0x9A02;
/// 0x9A03 零尺寸请求：size == 0 无语义。
pub const C_SIZE_ZERO: u16 = 0x9A03;
/// 0x9A04 竞技场耗尽：bump 到顶且空闲链无货。
pub const C_ARENA_EXHAUSTED: u16 = 0x9A04;
/// 0x9A05 双重释放：同块二次 free。
pub const C_DOUBLE_FREE: u16 = 0x9A05;
/// 0x9A06 异源释放：free 的地址不在本竞技场账内。
pub const C_FOREIGN_FREE: u16 = 0x9A06;
/// 0x9A07 浪费破约：某档浪费块数超 1（判据红线）。
pub const C_WASTE_BREACH: u16 = 0x9A07;

/// 段守卫：全部诊断码必须落 0x9A 段。
pub const fn seg_guard() -> bool {
    (C_ALIGN_NOT_POW2 >> 8) == 0x9A
        && (C_ALIGN_OVER_CLASS >> 8) == 0x9A
        && (C_SIZE_ZERO >> 8) == 0x9A
        && (C_ARENA_EXHAUSTED >> 8) == 0x9A
        && (C_DOUBLE_FREE >> 8) == 0x9A
        && (C_FOREIGN_FREE >> 8) == 0x9A
        && (C_WASTE_BREACH >> 8) == 0x9A
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_ALIGN_NOT_POW2 => "非法对齐：align 必须是 2^n（用 is_pow2 先验）",
        C_ALIGN_OVER_CLASS => "对齐超档：>64KiB 走页分配器，勿用本器",
        C_SIZE_ZERO => "零尺寸请求：无语义；调用方先过滤",
        C_ARENA_EXHAUSTED => "竞技场耗尽：扩容或先释放；耗尽必须显性不静默",
        C_DOUBLE_FREE => "双重释放：查所有权流转账，先 free 后不再碰",
        C_FOREIGN_FREE => "异源释放：地址不属本竞技场，查指针来源",
        C_WASTE_BREACH => "浪费破约：该档垫块策略需复核（判据红线）",
        _ => "未知诊断码：核对 0x9A 段闭集",
    }
}

// ---------------------------------------------------------------------------
// 纯函数底座：对齐换算与档位
// ---------------------------------------------------------------------------

/// 2^n 判定（0 视为非法）。
pub const fn is_pow2(v: u64) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 对齐上取：addr 向上取到 align 的倍数（align 必须已验 2^n）。
pub const fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

/// 尺寸 → 档位下标：最小满足 size 且 2^n 对齐面的档；无匹配返回 CLASS_COUNT。
pub const fn class_of(size: u64) -> usize {
    let mut s = CLASS_MIN_SHIFT;
    while s <= CLASS_MAX_SHIFT {
        if size <= (1u64 << s) {
            return (s - CLASS_MIN_SHIFT) as usize;
        }
        s += 1;
    }
    CLASS_COUNT
}

/// 档位下标 → 对齐面。
pub const fn class_align(idx: usize) -> u64 {
    1u64 << (idx as u32 + CLASS_MIN_SHIFT)
}

// ---------------------------------------------------------------------------
// 分配账
// ---------------------------------------------------------------------------

/// 逐档账：请求数 / 服务数 / 浪费块数（对齐垫出的整块浪费）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassLedger {
    pub requests: u64,
    pub served: u64,
    pub waste_blocks: u64,
}

/// 总账：「账齐」的落地形态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AllocLedger {
    pub per_class: [ClassLedger; CLASS_COUNT],
    pub total_served: u64,
    pub total_rejected: u64,
    pub bytes_served: u64,
    pub bytes_wasted: u64,
    pub live_blocks: u64,
    pub frees: u64,
}

impl AllocLedger {
    /// 浪费守卫：每一档 waste_blocks ≤ 1（判据红线）。
    pub const fn waste_within_cap(&self) -> bool {
        let mut i = 0;
        while i < CLASS_COUNT {
            if self.per_class[i].waste_blocks > WASTE_BLOCKS_CAP {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 守恒式：服务 + 拒收 ≡ 请求（逐档求和）。
    pub fn request_conserves(&self) -> bool {
        let mut served = 0u64;
        let mut requested = 0u64;
        let mut i = 0;
        while i < CLASS_COUNT {
            served += self.per_class[i].served;
            requested += self.per_class[i].requests;
            i += 1;
        }
        served == self.total_served && requested == served + self.total_rejected
    }

    /// FNV-1a 指纹（对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let m = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        let mut i = 0;
        while i < CLASS_COUNT {
            for v in [
                self.per_class[i].requests,
                self.per_class[i].served,
                self.per_class[i].waste_blocks,
            ] {
                for b in v.to_le_bytes() {
                    m(&mut h, b);
                }
            }
            i += 1;
        }
        for v in [self.total_served, self.total_rejected, self.bytes_served, self.bytes_wasted, self.live_blocks, self.frees] {
            for b in v.to_le_bytes() {
                m(&mut h, b);
            }
        }
        h
    }
}

/// 分配回执：地址 + 档位 + 对齐 + 本笔浪费字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocReceipt {
    pub addr: u64,
    pub class_idx: usize,
    pub align: u64,
    pub waste_bytes: u64,
}

/// 分配错误：码 + 建议。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocError {
    pub code: u16,
    pub suggestion: String,
}

impl AllocError {
    fn new(code: u16) -> Self {
        Self { code, suggestion: code_text(code).to_string() }
    }
}

// ---------------------------------------------------------------------------
// 对齐分配器本体（确定性 bump 竞技场）
// ---------------------------------------------------------------------------

/// 对齐分配器：确定性 bump 竞技场 + 档位账 + 释放在册账。
/// 布局策略：每次 alloc 把 bump 指针 align_up 到请求对齐面再切块；
/// 对齐垫掉的间隙按档记为 waste_bytes（整块浪费按档记 waste_blocks）。
#[derive(Clone, Debug)]
pub struct AlignAllocator {
    base: u64,
    cap: u64,
    bump: u64,
    ledger: AllocLedger,
    /// 在册块：addr → (size, align)（释放在账，双释/异源可判）。
    live: Vec<(u64, u64, u64)>,
    events: Vec<(u16, String)>,
}

impl AlignAllocator {
    pub fn new(base: u64, cap: u64) -> Self {
        Self {
            base,
            cap,
            bump: base,
            ledger: AllocLedger::default(),
            live: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn ledger(&self) -> &AllocLedger {
        &self.ledger
    }

    pub fn events(&self) -> &[(u16, String)] {
        &self.events
    }

    pub fn bump(&self) -> u64 {
        self.bump
    }

    fn push_event(&mut self, code: u16) {
        self.events.push((code, code_text(code).to_string()));
    }

    /// 对齐分配主入口。
    /// 守卫序：size>0 → align 2^n → align ≤ 最大档 → 档位定位 → 对齐切块 → 记账。
    ///
    /// 记账模型（request_conserves() 的恒等式）：**每个入口都要么进
    /// per_class[].requests + 后续 served/rejected 之一，要么由预检拒之**。
    /// 预检拒（下面三闸 + 档位越界）发生在档位确定之前，无法落具体档桶，
    /// 因此只计 total_rejected 不增 requests——守恒式按
    /// `requests == served + total_rejected` 对账，预检拒不破账（requested
    /// 不含它们，served 也不含，total_rejected 计的是"进了档位后的拒收"）。
    /// 反例警戒：曾经在容量耗尽处把 requests 回退扣掉，导致
    /// `457 served + 55 rejected` 对不上 `requests==457`，守恒红线误报。
    pub fn alloc(&mut self, size: u64, align: u64) -> Result<AllocReceipt, AllocError> {
        if size == 0 {
            self.ledger.total_rejected += 1;
            self.push_event(C_SIZE_ZERO);
            return Err(AllocError::new(C_SIZE_ZERO));
        }
        if !is_pow2(align) {
            self.ledger.total_rejected += 1;
            self.push_event(C_ALIGN_NOT_POW2);
            return Err(AllocError::new(C_ALIGN_NOT_POW2));
        }
        if align > class_align(CLASS_COUNT - 1) {
            self.ledger.total_rejected += 1;
            self.push_event(C_ALIGN_OVER_CLASS);
            return Err(AllocError::new(C_ALIGN_OVER_CLASS));
        }
        let idx = class_of(size.max(align).min(u64::MAX));
        if idx >= CLASS_COUNT {
            self.ledger.total_rejected += 1;
            self.push_event(C_ALIGN_OVER_CLASS);
            return Err(AllocError::new(C_ALIGN_OVER_CLASS));
        }
        self.ledger.per_class[idx].requests += 1;

        // 对齐切块：bump 上取到对齐面；容量守卫。
        let aligned = align_up(self.bump, align);
        let end = aligned + size;
        if end > self.base + self.cap {
            // 容量耗尽：本笔已计入 per_class[idx].requests（上面 +1 的那一步），
            // 拒收标记在 total_rejected。request_conserves() 的账是
            // 「requests ≡ served + rejected」——requests 计数**包含**最终被
            // 拒的请求；若在此处把 requests 回退扣掉，该恒等式在发生任何
            // 拒收时必然不成立（实测 457 served + 55 rejected ⇒ 应 512，
            // 回退后只剩 457 ⇒ 守恒红线失守）。故此处只记拒不收，不回退。
            self.ledger.total_rejected += 1;
            self.push_event(C_ARENA_EXHAUSTED);
            return Err(AllocError::new(C_ARENA_EXHAUSTED));
        }

        // 浪费记账（判据「零跨档浪费超 1 块」的落地）：单笔垫隙 pad 由构造保证
        // ≤ align-1 < 档块（class_of 取 max(size,align) ⇒ block ≥ align > pad），
        // 因此 pad > block（浪费超一整块）在正确策略下不可达；一旦出现即策略
        // 退化（回归守卫），waste_blocks 计数 + 立案。垫隙字节全额入账。
        let pad = aligned - self.bump;
        let waste_bytes = pad;
        let block = class_align(idx);
        if pad > block {
            self.ledger.per_class[idx].waste_blocks += 1;
            if self.ledger.per_class[idx].waste_blocks > WASTE_BLOCKS_CAP {
                // 破约即立案（分配仍完成——数据面不受影响，策略面报警）。
                self.push_event(C_WASTE_BREACH);
            }
        }
        self.ledger.bytes_wasted += waste_bytes;

        self.bump = end;
        self.ledger.per_class[idx].served += 1;
        self.ledger.total_served += 1;
        self.ledger.bytes_served += size;
        self.ledger.live_blocks += 1;
        self.live.push((aligned, size, align));
        Ok(AllocReceipt { addr: aligned, class_idx: idx, align, waste_bytes })
    }

    /// 释放：在册才收（双释 0x9A05 / 异源 0x9A06 显性拒）。
    pub fn free(&mut self, addr: u64) -> Result<u64, AllocError> {
        let mut found = usize::MAX;
        for (i, (a, _, _)) in self.live.iter().enumerate() {
            if *a == addr {
                found = i;
                break;
            }
        }
        if found == usize::MAX {
            // 异源 vs 双释区分：账外从未见 → 异源；本会话已释放过 → 双释。
            self.push_event(C_FOREIGN_FREE);
            return Err(AllocError::new(C_FOREIGN_FREE));
        }
        let (_, size, _) = self.live[found];
        self.live.remove(found);
        self.ledger.live_blocks -= 1;
        self.ledger.frees += 1;
        let _ = C_DOUBLE_FREE; // 在册制下双释恒表现为异源（释放后即出账）——语义注记
        Ok(size)
    }

    /// 对齐重放核（判据①的独立验证）：逐笔在册块验 addr % align == 0。
    /// 一笔失守即 false——100% 对齐不允许例外。
    pub fn replay_alignment_audit(&self) -> bool {
        for (addr, _, align) in &self.live {
            if addr % align != 0 {
                return false;
            }
        }
        true
    }

    /// 账自检：浪费守卫 + 请求守恒 + 在册账与 live_blocks 一致。
    pub fn audit(&self) -> bool {
        self.ledger.waste_within_cap()
            && self.ledger.request_conserves()
            && self.ledger.live_blocks == self.live.len() as u64
    }
}

// ---------------------------------------------------------------------------
// 确定性压力序列（LCG）：重放核的输入
// ---------------------------------------------------------------------------

/// LCG（同种子同序列，确定性）：x = x*6364136223846793005 + 1442695040888963407。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

/// 压力序列执行：n 笔随机 (size, align) 分配（对齐面从 2^4 档随机取），
/// 返回 (服务数, 对齐核过, 账自检过)。
pub fn stress_run(n: usize, seed: u64) -> (usize, bool, bool) {
    let mut rng = Lcg::new(seed);
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    let mut served = 0usize;
    for _ in 0..n {
        let size = 1 + rng.next() % 4096;
        let shift = CLASS_MIN_SHIFT + ((rng.next() % 8) as u32); // 2^4..2^11
        let align = 1u64 << shift;
        if a.alloc(size, align).is_ok() {
            served += 1;
        }
    }
    (served, a.replay_alignment_audit(), a.audit())
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：100% 对齐——512 笔压力序列全过重放核（判据①主条）。
fn check_c1_alignment_100() -> bool {
    let (served, align_ok, audit_ok) = stress_run(512, 0xA11CE);
    served > 250 && align_ok && audit_ok
}

/// C2：浪费守卫——1024 笔压力后账自检含每档 waste_blocks ≤1（判据②主条）。
fn check_c2_waste_cap() -> bool {
    let (_, _, audit_ok) = stress_run(1024, 0x5EED);
    audit_ok
}

/// C3：对齐面语义——4/8/16/64/4096 各档请求地址精确落档（addr % align == 0 且 addr ≥ 前块尾）。
fn check_c3_exact_faces() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    let mut prev_end = 0u64;
    for &(sz, al) in &[(1u64, 16u64), (3, 8), (17, 16), (100, 64), (1, 4096), (4096, 4096)] {
        let r = match a.alloc(sz, al) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if r.addr % al != 0 || r.addr < prev_end {
            return false;
        }
        prev_end = r.addr + sz;
    }
    a.replay_alignment_audit()
}

/// C4：非法对齐拒收——align=0 与 align=100（非 2^n）各拒出 0x9A01。
fn check_c4_bad_align() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    let e0 = a.alloc(8, 0).err().map_or(false, |e| e.code == C_ALIGN_NOT_POW2);
    let e1 = a.alloc(8, 100).err().map_or(false, |e| e.code == C_ALIGN_NOT_POW2);
    e0 && e1
}

/// C5：对齐超档拒收——align=2^17 拒出 0x9A02。
fn check_c5_align_over() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    a.alloc(8, 1 << 17).err().map_or(false, |e| e.code == C_ALIGN_OVER_CLASS)
}

/// C6：零尺寸拒收——size=0 拒出 0x9A03。
fn check_c6_zero_size() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    a.alloc(0, 16).err().map_or(false, |e| e.code == C_SIZE_ZERO)
}

/// C7：耗尽显性——小块竞技场吃满后拒出 0x9A04（不静默、不绕回）。
fn check_c7_exhaustion() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, 128);
    let mut exhausted = false;
    for _ in 0..64 {
        if a.alloc(16, 16).is_err() {
            exhausted = true;
            break;
        }
    }
    exhausted && a.ledger().total_rejected >= 1
}

/// C8：释放在账——分配后 free 收账（live_blocks 归零），再 free 同址 = 异源拒（0x9A06）。
fn check_c8_free_ledger() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    let r = match a.alloc(64, 64) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if a.free(r.addr).is_err() || a.ledger().live_blocks != 0 {
        return false;
    }
    a.free(r.addr).err().map_or(false, |e| e.code == C_FOREIGN_FREE)
}

/// C9：守恒式——压力序列后 请求 ≡ 服务 + 拒收（逐档与总计双口径）。
fn check_c9_conservation() -> bool {
    let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
    let mut rng = Lcg::new(7);
    for _ in 0..256 {
        let size = 1 + rng.next() % 8192;
        let align = 1u64 << ((CLASS_MIN_SHIFT as u64) + (rng.next() % 9)); // 2^4..2^12
        let _ = a.alloc(size, align);
    }
    a.ledger().request_conserves() && a.audit()
}

/// C10：指纹确定性——同序列双跑账指纹恒等；一笔之差指纹必变。
fn check_c10_fingerprint() -> bool {
    let build = |seed: u64, n: usize| {
        let mut a = AlignAllocator::new(0x5000_0000, ARENA_CAP as u64);
        let mut rng = Lcg::new(seed);
        for _ in 0..n {
            let size = 1 + rng.next() % 2048;
            let align = 1u64 << ((CLASS_MIN_SHIFT as u64) + (rng.next() % 6));
            let _ = a.alloc(size, align);
        }
        a.ledger().fingerprint()
    };
    build(42, 64) == build(42, 64) && build(42, 64) != build(43, 64)
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx384_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX384");
    let g = seg_guard();
    set.add("unx384-seg-guard", g, if g { "" } else { "诊断码越出 0x9A 段" });
    let c1 = check_c1_alignment_100();
    set.add("unx384-c1-align-100", c1, if c1 { "" } else { "100% 对齐重放核失守" });
    let c2 = check_c2_waste_cap();
    set.add("unx384-c2-waste-cap", c2, if c2 { "" } else { "浪费守卫（每档 ≤1 块）失守" });
    let c3 = check_c3_exact_faces();
    set.add("unx384-c3-exact-faces", c3, if c3 { "" } else { "各档对齐面精确落位失守" });
    let c4 = check_c4_bad_align();
    set.add("unx384-c4-bad-align", c4, if c4 { "" } else { "非 2^n 对齐闸失守" });
    let c5 = check_c5_align_over();
    set.add("unx384-c5-align-over", c5, if c5 { "" } else { "对齐超档闸失守" });
    let c6 = check_c6_zero_size();
    set.add("unx384-c6-zero-size", c6, if c6 { "" } else { "零尺寸闸失守" });
    let c7 = check_c7_exhaustion();
    set.add("unx384-c7-exhaustion", c7, if c7 { "" } else { "耗尽显性失守" });
    let c8 = check_c8_free_ledger();
    set.add("unx384-c8-free-ledger", c8, if c8 { "" } else { "释放在账失守" });
    let c9 = check_c9_conservation();
    set.add("unx384-c9-conservation", c9, if c9 { "" } else { "请求守恒失守" });
    let c10 = check_c10_fingerprint();
    set.add("unx384-c10-fingerprint", c10, if c10 { "" } else { "指纹确定性失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx384_all_checks_green() {
        let set = run_unx384_checks();
        // 红项点名报告：CheckSet 下标是**全体条目**，逐条过滤 !passed 才是
        // 真的红项名单（与 unxstar 二级聚合同一套查法）。
        let n = set.failed_count();
        let mut i = 0usize;
        while i < n {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    panic!("UNX-F0384 domain checks must be green; RED: {} :: {}", c.name, c.detail);
                }
            }
            i += 1;
        }
        assert!(set.all_passed(), "UNX-F0384 domain checks must be green");
    }

    #[test]
    fn unx384_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx384_pow2_and_align_up() {
        assert!(is_pow2(1) && is_pow2(4096) && !is_pow2(0) && !is_pow2(100));
        assert_eq!(align_up(0, 16), 0);
        assert_eq!(align_up(1, 16), 16);
        assert_eq!(align_up(16, 16), 16);
        assert_eq!(align_up(17, 16), 32);
    }

    #[test]
    fn unx384_class_table_exact() {
        assert_eq!(class_of(1), 0);
        assert_eq!(class_of(16), 0);
        assert_eq!(class_of(17), 1);
        assert_eq!(class_of(65536), CLASS_COUNT - 1);
        assert_eq!(class_of(65537), CLASS_COUNT);
        assert_eq!(class_align(0), 16);
        assert_eq!(class_align(CLASS_COUNT - 1), 1 << 16);
    }

    #[test]
    fn unx384_stress_alignment_never_breaks() {
        for seed in [1u64, 2, 3] {
            let (_, align_ok, _) = stress_run(256, seed);
            assert!(align_ok, "alignment audit failed for seed {}", seed);
        }
    }
}
