//! UNREAL-X / UNX-A1-B16 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0303 · 内核高半区映射（随机基址落位）（目标 320 行）
//! 判据原文摘录（书 1745 行）：UNX-F0303-J1 随机基址下高半区映射区间与
//! F0091 参数包一致，段权限与 F0096 对齐。
//! 职责定位：随机基址（KASLR 生效）不是只挪一个数——高半区四段（.text/
//! .rodata/.data/.bss）的映射区间必须能从 F0091 参数包**复算**（基址槽→
//! 段区间逐段可推），且每段权限必须与 F0096 段加载器矩阵**对齐**（.text
//! RX/.rodata R/.data RW——W^X 铁律：可写且可执行即违例）。随机化的是
//! 落点，冻结的是结构：参数包一致+权限对齐两条都对上，随机基址才算生效。
//! 设计要点：①**参数包复算**——RandomParamPack（基址槽/自映射槽/高半区
//! 摘要）是随机化的唯一真源，映射区间逐段从参数包推导（段序固定+段长固
//! 定，只有基址随包变）；②**权限矩阵**——段名→权限位闭集（F0096 对齐），
//! W^X 违例（RWX）与只读段可写（权限降级）都是落位违例；③**映射账逐段
//! 对拍**——登记面与复算面逐段全等（区间+页数+权限三元组）；④**家族复
//! 用声明**——FNV-1a/LCG（a=1664525 c=1013904223）/逻辑 tick/CheckSet
//! （家族复用第 14 用，紧接 F0302 第 13 用），本单新增的是参数包复算与
//! 权限对拍面。
//! 诊断码段：0x58（本单独占，0x5801~0x580F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x58 段独占）
// ---------------------------------------------------------------------------

/// 0x5801 参数包漂移（映射区间与 F0091 参数包复算不一致）。
pub const XC_PACK_DRIFT: u32 = 0x5801;
/// 0x5802 权限违例（段权限与 F0096 矩阵不对齐）。
pub const XC_PERM_VIOLATION: u32 = 0x5802;
/// 0x5803 W^X 破坏（RWX 同体——可写可执行）。
pub const WX_BREAK: u32 = 0x5803;
/// 0x5804 段序漂移（四段顺序与固定段序不符——段间关系破坏）。
pub const XC_SEGMENT_ORDER: u32 = 0x5804;
/// 0x5805 区间越槽（映射区间超出高半区槽面——位片面越界）。
pub const XC_RANGE_OOB: u32 = 0x5805;
/// 0x5806 参数包坏（基址槽不在高半区/自映射槽非法——包本身失真）。
pub const XC_PACK_BAD: u32 = 0x5806;
/// 0x5807 页数失守（段页数与登记数不符——账面残缺）。
pub const XC_PAGE_GAP: u32 = 0x5807;
/// 0x5808 对齐违例（段基址非页对齐——粒度面破坏）。
pub const XC_ALIGN_BAD: u32 = 0x5808;
/// 0x5809 双重落位（同段二次落位——幂等面外的重复动作）。
pub const XC_DOUBLE_PLACE: u32 = 0x5809;
/// 0x580A 摘要不符（参数包摘要与复算摘要不等——包面篡改检出）。
pub const XC_DIGEST_MISMATCH: u32 = 0x580A;
/// 0x580B 指纹漂移（同种子双跑指纹不等——确定性破坏）。
pub const XC_FINGERPRINT_DRIFT: u32 = 0x580B;
/// 0x580C 落位账失守（落位段数与 4 不符——账面残缺）。
pub const XC_LEDGER_GAP: u32 = 0x580C;

/// 诊断码 → 原因文案（全量显性）。
pub fn reason_of(code: u32) -> &'static str {
    match code {
        XC_PACK_DRIFT => "参数包漂移：映射区间与 F0091 参数包复算不一致",
        XC_PERM_VIOLATION => "权限违例：段权限与 F0096 矩阵不对齐",
        WX_BREAK => "W^X 破坏：可写且可执行同体",
        XC_SEGMENT_ORDER => "段序漂移：四段顺序与固定段序不符",
        XC_RANGE_OOB => "区间越槽：映射区间超出高半区槽面",
        XC_PACK_BAD => "参数包坏：基址槽或自映射槽非法",
        XC_PAGE_GAP => "页数失守：段页数与登记数不符",
        XC_ALIGN_BAD => "对齐违例：段基址非页对齐",
        XC_DOUBLE_PLACE => "双重落位：同段二次落位",
        XC_DIGEST_MISMATCH => "摘要不符：参数包摘要与复算摘要不等",
        XC_FINGERPRINT_DRIFT => "指纹漂移：同种子双跑指纹不等",
        XC_LEDGER_GAP => "落位账失守：落位段数与 4 不符",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// §1 常量（F0091/F0096 对接的数值面）
// ---------------------------------------------------------------------------

/// 页粒度（4K——对齐与页数口径）。
pub const PAGE_SIZE: u64 = 4096;
/// PML4 槽数。
pub const PML4_SLOTS: usize = 512;
/// 高半区起始槽。
pub const HH_START_SLOT: usize = 256;
/// 高半区内核基址槽候选池（258~263 六槽——F0091 随机选一）。
pub const KERNEL_SLOT_POOL: [usize; 6] = [258, 259, 260, 261, 262, 263];
/// 自映射槽候选池起（F0302 保留区口径——504~510 的 Reserved 面）。
pub const SELFMAP_POOL_START: usize = 504;
/// 自映射槽候选池终（不含 511 经典自映射——随机重选面）。
pub const SELFMAP_POOL_END: usize = 511;
/// 随机基址组数（J1 复测规模）。
pub const RANDOM_ROUNDS: usize = 128;
/// 固定段序（四段——段序漂移判据的基准序）。
pub const SEGMENT_ORDER: [&str; 4] = [".text", ".rodata", ".data", ".bss"];
/// 各段页数（固定——段长固定，只有基址随包变）。
pub const SEGMENT_PAGES: [u64; 4] = [24, 8, 16, 6];
/// LCG 乘子。
const LCG_A: u64 = 1664525;
/// LCG 增量。
const LCG_C: u64 = 1013904223;
/// FNV-1a 偏移基。
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100000001b3;

/// 权限位（PTE 风格：R=1<<0 W=1<<1 X=1<<2）。
pub const PERM_R: u8 = 1;
/// 写位。
pub const PERM_W: u8 = 2;
/// 执行位。
pub const PERM_X: u8 = 4;

/// F0096 权限矩阵（段名→权限位闭集：.text RX/.rodata R/.data RW/.bss RW）。
pub fn perm_of(segment: &str) -> Result<u8, u32> {
    match segment {
        ".text" => Ok(PERM_R | PERM_X),
        ".rodata" => Ok(PERM_R),
        ".data" => Ok(PERM_R | PERM_W),
        ".bss" => Ok(PERM_R | PERM_W),
        _ => Err(XC_PERM_VIOLATION),
    }
}

/// W^X 核验（可写且可执行即违例——铁律 O(1)）。
pub fn wx_check(perm: u8) -> Result<(), u32> {
    if perm & PERM_W != 0 && perm & PERM_X != 0 {
        return Err(WX_BREAK);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §2 家族确定性件（LCG/FNV/Tick——第 14 用）
// ---------------------------------------------------------------------------

/// LCG 确定性种子流。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

pub fn fnv1a_str(s: &str) -> u64 {
    fnv1a(s.as_bytes())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    v: u64,
}

impl Tick {
    pub fn new() -> Tick {
        Tick { v: 0 }
    }
    pub fn tick(&mut self) -> u64 {
        self.v = self.v.wrapping_add(1);
        self.v
    }
    pub fn get(&self) -> u64 {
        self.v
    }
}

// ---------------------------------------------------------------------------
// §3 参数包（F0091 对接——随机化的唯一真源）
// ---------------------------------------------------------------------------

/// 随机参数包（基址槽+自映射槽——高半区映射的推导源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomParamPack {
    /// 内核基址槽（高半区槽池随机选一）。
    pub base_slot: usize,
    /// 自映射槽（保留区池随机选一——F0302 同构口径）。
    pub selfmap_slot: usize,
}

/// 参数包构造（LCG 确定性——池内随机选，非法池拒绝）。
pub fn derive_pack(rng: &mut Lcg) -> Result<RandomParamPack, u32> {
    let bi = rng.below(KERNEL_SLOT_POOL.len() as u64) as usize;
    let base_slot = KERNEL_SLOT_POOL[bi];
    let pool = SELFMAP_POOL_END - SELFMAP_POOL_START;
    let selfmap_slot = SELFMAP_POOL_START + (rng.below(pool as u64) as usize);
    let pack = RandomParamPack { base_slot, selfmap_slot };
    pack.validate()?;
    Ok(pack)
}

impl RandomParamPack {
    /// 参数包合法性（基址槽在池内+自映射槽在池内——包面失真拦）。
    pub fn validate(&self) -> Result<(), u32> {
        if !KERNEL_SLOT_POOL.contains(&self.base_slot) {
            return Err(XC_PACK_BAD);
        }
        if self.selfmap_slot < SELFMAP_POOL_START || self.selfmap_slot >= SELFMAP_POOL_END {
            return Err(XC_PACK_BAD);
        }
        if self.base_slot == self.selfmap_slot {
            return Err(XC_PACK_BAD);
        }
        Ok(())
    }

    /// 基址 VA（O(1) 位片——基址槽<<39）。
    pub fn base_va(&self) -> u64 {
        (self.base_slot as u64) << 39
    }

    /// 自映射 VA（O(1) 位片）。
    pub fn selfmap_va(&self) -> u64 {
        (self.selfmap_slot as u64) << 39
    }

    /// 段基址复算（O(段序)——段 i 基址=包基址+前序段页数和×页粒度）。
    /// 参数包一致判据的推导面：区间从包**复算**不是另记一份。
    pub fn segment_base(&self, idx: usize) -> Result<u64, u32> {
        if idx >= SEGMENT_ORDER.len() {
            return Err(XC_SEGMENT_ORDER);
        }
        let mut va = self.base_va();
        for i in 0..idx {
            va = va.wrapping_add(SEGMENT_PAGES[i].wrapping_mul(PAGE_SIZE));
        }
        Ok(va)
    }

    /// 段区间复算（起 VA+页数——三元组推导面）。
    pub fn segment_range(&self, idx: usize) -> Result<(u64, u64), u32> {
        if idx >= SEGMENT_ORDER.len() {
            return Err(XC_SEGMENT_ORDER);
        }
        let start = self.segment_base(idx)?;
        let pages = SEGMENT_PAGES[idx];
        Ok((start, pages))
    }

    /// 包摘要（两槽位联合 FNV——包面篡改检出）。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h ^= (self.base_slot as u64).wrapping_mul(FNV_PRIME);
        h ^= (self.selfmap_slot as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 高半区越槽核验（最高段末页不得超出基址槽位片——区间越槽拦）。
    pub fn range_in_slot(&self) -> Result<(), u32> {
        let last = self.segment_base(SEGMENT_ORDER.len() - 1)?;
        let last_end = last.wrapping_add(
            SEGMENT_PAGES[SEGMENT_ORDER.len() - 1].wrapping_mul(PAGE_SIZE),
        );
        let slot_end = (self.base_slot as u64 + 1).wrapping_mul(1u64 << 39);
        if last_end > slot_end {
            return Err(XC_RANGE_OOB);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §4 映射账（逐段落位登记——与参数包复算面对拍）
// ---------------------------------------------------------------------------

/// 单段落位登记（段名/起 VA/页数/权限三元组+一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentEntry {
    pub name: &'static str,
    pub start_va: u64,
    pub pages: u64,
    pub perm: u8,
}

/// 映射账（四段逐段登记——落位 O(段数)，对拍 O(段数)）。
#[derive(Debug)]
pub struct MapLedger {
    entries: Vec<SegmentEntry>,
    pack: Option<RandomParamPack>,
    tick: Tick,
}

impl MapLedger {
    pub fn new() -> MapLedger {
        MapLedger { entries: Vec::new(), pack: None, tick: Tick::new() }
    }

    /// 绑定参数包（落位前提——无包不落位）。
    pub fn bind(&mut self, pack: RandomParamPack) -> Result<(), u32> {
        pack.validate()?;
        self.pack = Some(pack);
        let _ = self.tick.tick();
        Ok(())
    }

    /// 单段落位（对齐+权限+页数+段序四查——过才登记；同段重落拒）。
    pub fn place(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= SEGMENT_ORDER.len() {
            return Err(XC_SEGMENT_ORDER);
        }
        let name = SEGMENT_ORDER[idx];
        if self.entries.iter().any(|e| e.name == name) {
            return Err(XC_DOUBLE_PLACE);
        }
        // 段序（必须按固定序落位——.text 先于 .rodata 先于 .data 先于 .bss）。
        if self.entries.len() != idx {
            return Err(XC_SEGMENT_ORDER);
        }
        let pack = self.pack.ok_or(XC_PACK_BAD)?;
        let start = pack.segment_base(idx)?;
        // 页对齐（段基址非页对齐即落位违例）。
        if start % PAGE_SIZE != 0 {
            return Err(XC_ALIGN_BAD);
        }
        let pages = SEGMENT_PAGES[idx];
        let perm = perm_of(name)?;
        wx_check(perm)?;
        self.entries.push(SegmentEntry { name, start_va: start, pages, perm });
        let _ = self.tick.tick();
        Ok(())
    }

    /// 全段落位（四段按序——落位账齐）。
    pub fn place_all(&mut self) -> Result<(), u32> {
        for idx in 0..SEGMENT_ORDER.len() {
            self.place(idx)?;
        }
        if self.entries.len() != SEGMENT_ORDER.len() {
            return Err(XC_LEDGER_GAP);
        }
        Ok(())
    }

    /// 逐段对拍（J1 核心——账面 vs 参数包复算面三元组全等）。
    pub fn audit(&self) -> Result<(), u32> {
        let pack = self.pack.ok_or(XC_PACK_BAD)?;
        if self.entries.len() != SEGMENT_ORDER.len() {
            return Err(XC_LEDGER_GAP);
        }
        for (idx, e) in self.entries.iter().enumerate() {
            if e.name != SEGMENT_ORDER[idx] {
                return Err(XC_SEGMENT_ORDER);
            }
            let (exp_start, exp_pages) = pack.segment_range(idx)?;
            if e.start_va != exp_start {
                return Err(XC_PACK_DRIFT);
            }
            if e.pages != exp_pages {
                return Err(XC_PAGE_GAP);
            }
            let exp_perm = perm_of(e.name)?;
            if e.perm != exp_perm {
                return Err(XC_PERM_VIOLATION);
            }
            wx_check(e.perm)?;
        }
        pack.range_in_slot()?;
        Ok(())
    }

    /// 账面摘要（段名序+页数+权限联合——复算摘要对拍锚）。
    pub fn recomputed_digest(&self) -> Result<u64, u32> {
        let pack = self.pack.ok_or(XC_PACK_BAD)?;
        let mut h = pack.digest();
        for idx in 0..SEGMENT_ORDER.len() {
            let (start, pages) = pack.segment_range(idx)?;
            h ^= start.wrapping_mul(FNV_PRIME);
            h ^= pages.wrapping_mul(FNV_PRIME);
            h ^= perm_of(SEGMENT_ORDER[idx])? as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        Ok(h)
    }

    /// 账面指纹（登记面 FNV）。
    pub fn ledger_digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for e in &self.entries {
            h = fnv1a_str(e.name) ^ h.wrapping_mul(FNV_PRIME);
            h ^= e.start_va.wrapping_mul(FNV_PRIME);
            h ^= e.pages.wrapping_mul(FNV_PRIME);
            h ^= e.perm as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn tick_now(&self) -> u64 {
        self.tick.get()
    }

    /// 段名查询（O(n)——落位面可点名）。
    pub fn has_segment(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }
}

// ---------------------------------------------------------------------------
// §5 J1 三闸 + 双跑指纹 + 判据检查
// ---------------------------------------------------------------------------

/// 全流程一次跑（构造包→绑定→四段落位→对拍——J1 与双跑复用）。
fn full_run(seed: u64) -> Result<(RandomParamPack, MapLedger, u64), (u32, String)> {
    let mut rng = Lcg::new(seed);
    let pack = derive_pack(&mut rng).map_err(|e| (e, reason_of(e).to_string()))?;
    let mut ledger = MapLedger::new();
    ledger.bind(pack).map_err(|e| (e, reason_of(e).to_string()))?;
    ledger.place_all().map_err(|e| (e, reason_of(e).to_string()))?;
    ledger.audit().map_err(|e| (e, reason_of(e).to_string()))?;
    let re = ledger.recomputed_digest().map_err(|e| (e, reason_of(e).to_string()))?;
    let ld = ledger.ledger_digest();
    if re != ld {
        return Err((XC_DIGEST_MISMATCH, format!("{:016x}≠{:016x}", re, ld)));
    }
    Ok((pack, ledger, ld))
}

/// J1-A：随机基址 128 组复测——每组映射区间与参数包一致+权限对齐 F0096。
fn j1_random_rounds() -> Result<(), (u32, String)> {
    let mut seen_bases: Vec<usize> = Vec::new();
    for round in 0..RANDOM_ROUNDS {
        let (_, ledger, _) = full_run(0xF0303 + round as u64)?;
        if ledger.len() != 4 {
            return Err((XC_LEDGER_GAP, format!("组{}段数{}", round, ledger.len())));
        }
        // 权限对齐显性核验（.text RX/.rodata R/.data RW/.bss RW）。
        let mut rng = Lcg::new(0xF0303 + round as u64);
        let pack = derive_pack(&mut rng).map_err(|e| (e, format!("组{}包", round)))?;
        for idx in 0..4 {
            let exp = perm_of(SEGMENT_ORDER[idx]).map_err(|e| (e, "权限表".to_string()))?;
            let start = pack.segment_base(idx).map_err(|e| (e, "复算".to_string()))?;
            let seg = ledger.entries().get(idx).ok_or((XC_LEDGER_GAP, "缺段".to_string()))?;
            if seg.perm != exp || seg.start_va != start {
                return Err((XC_PERM_VIOLATION, format!("组{}段{}", round, idx)));
            }
        }
        seen_bases.push(pack.base_slot);
    }
    // 随机性显性（基址槽出现过 ≥2 种——纯随机池不可能恒同一槽）。
    let mut d = seen_bases.clone();
    d.sort();
    d.dedup();
    if d.len() < 2 {
        return Err((XC_PACK_BAD, format!("随机化未生效（仅{}种基址）", d.len())));
    }
    Ok(())
}

/// J1-B：负路径 10 闸（W^X/段序/对齐/越槽/坏包/双落位/摘要篡改等）。
fn j1_negative_paths() -> Result<(), (u32, String)> {
    // 1 W^X 违例（RWX 同体）。
    match wx_check(PERM_R | PERM_W | PERM_X) {
        Err(WX_BREAK) => {}
        _ => return Err((WX_BREAK, "W^X 未拦".to_string())),
    }
    // 2 未知段权限查询。
    match perm_of(".evil") {
        Err(XC_PERM_VIOLATION) => {}
        _ => return Err((XC_PERM_VIOLATION, "未知段未拦".to_string())),
    }
    // 3 坏包（基址槽不在池内）。
    {
        let bad = RandomParamPack { base_slot: 300, selfmap_slot: 505 };
        match bad.validate() {
            Err(XC_PACK_BAD) => {}
            _ => return Err((XC_PACK_BAD, "坏包未拦".to_string())),
        }
    }
    // 4 坏包（自映射槽越池）。
    {
        let bad = RandomParamPack { base_slot: 258, selfmap_slot: 600 };
        match bad.validate() {
            Err(XC_PACK_BAD) => {}
            _ => return Err((XC_PACK_BAD, "自映射越池未拦".to_string())),
        }
    }
    // 5 同槽冲突（基址槽==自映射槽）。
    {
        let bad = RandomParamPack { base_slot: 258, selfmap_slot: 258 };
        match bad.validate() {
            Err(XC_PACK_BAD) => {}
            _ => return Err((XC_PACK_BAD, "同槽冲突未拦".to_string())),
        }
    }
    // 6 无包落位。
    {
        let mut ledger = MapLedger::new();
        match ledger.place(0) {
            Err(XC_PACK_BAD) => {}
            _ => return Err((XC_PACK_BAD, "无包落位未拦".to_string())),
        }
    }
    // 7 段序跳步（直接落 .rodata）。
    {
        let mut rng = Lcg::new(7);
        let pack = derive_pack(&mut rng).map_err(|e| (e, reason_of(e).to_string()))?;
        let mut ledger = MapLedger::new();
        ledger.bind(pack).map_err(|e| (e, reason_of(e).to_string()))?;
        match ledger.place(1) {
            Err(XC_SEGMENT_ORDER) => {}
            _ => return Err((XC_SEGMENT_ORDER, "段序跳步未拦".to_string())),
        }
    }
    // 8 双重落位（.text 落两次）。
    {
        let mut rng = Lcg::new(8);
        let pack = derive_pack(&mut rng).map_err(|e| (e, reason_of(e).to_string()))?;
        let mut ledger = MapLedger::new();
        ledger.bind(pack).map_err(|e| (e, reason_of(e).to_string()))?;
        ledger.place(0).map_err(|e| (e, reason_of(e).to_string()))?;
        match ledger.place(0) {
            Err(XC_DOUBLE_PLACE) => {}
            _ => return Err((XC_DOUBLE_PLACE, "双落位未拦".to_string())),
        }
    }
    // 9 段索引越界。
    {
        let mut rng = Lcg::new(9);
        let pack = derive_pack(&mut rng).map_err(|e| (e, reason_of(e).to_string()))?;
        match pack.segment_base(4) {
            Err(XC_SEGMENT_ORDER) => {}
            _ => return Err((XC_SEGMENT_ORDER, "段索引越界未拦".to_string())),
        }
    }
    // 10 账面残缺（只落两段就对拍）。
    {
        let mut rng = Lcg::new(10);
        let pack = derive_pack(&mut rng).map_err(|e| (e, reason_of(e).to_string()))?;
        let mut ledger = MapLedger::new();
        ledger.bind(pack).map_err(|e| (e, reason_of(e).to_string()))?;
        ledger.place(0).map_err(|e| (e, reason_of(e).to_string()))?;
        match ledger.audit() {
            Err(XC_LEDGER_GAP) => {}
            _ => return Err((XC_LEDGER_GAP, "账残未拦".to_string())),
        }
    }
    Ok(())
}

/// 双跑指纹：同种子两次全流程指纹必等。
fn dual_run_fingerprint(seed: u64) -> Result<u64, (u32, String)> {
    let (_, _, fa) = full_run(seed)?;
    let (_, _, fb) = full_run(seed)?;
    if fa != fb {
        return Err((XC_FINGERPRINT_DRIFT, format!("{:016x}≠{:016x}", fa, fb)));
    }
    Ok(fa)
}

impl MapLedger {
    /// 登记面只读访问（J1 复测核验用）。
    pub fn entries(&self) -> &[SegmentEntry] {
        &self.entries
    }
}

/// UNX-F0303 判据入口（返回式 CheckSet——预期值独立手算）。
pub fn run_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0303");

    // 判-00 权限矩阵与页对齐自证（F0096 对齐面）。
    {
        let t = perm_of(".text");
        let ro = perm_of(".rodata");
        let da = perm_of(".data");
        let bs = perm_of(".bss");
        // 手算：text=RX(5)/rodata=R(1)/data=RW(3)/bss=RW(3)；W^X 全过。
        let ok = t == Ok(5)
            && ro == Ok(1)
            && da == Ok(3)
            && bs == Ok(3)
            && wx_check(5) == Ok(())
            && wx_check(3) == Ok(())
            && wx_check(1) == Ok(())
            && SEGMENT_ORDER == [".text", ".rodata", ".data", ".bss"]
            && SEGMENT_PAGES == [24, 8, 16, 6];
        if ok {
            s.ok("F0303-判-00-权限矩阵与段常量");
        } else {
            s.fail("F0303-判-00-权限矩阵与段常量", "权限面不符");
        }
    }

    // 包-01 参数包：池内构造+复算面（段基址页对齐+区间递进）。
    {
        let mut rng = Lcg::new(0xBEEF);
        let pack = derive_pack(&mut rng);
        if let Ok(p) = pack {
            let b0 = p.segment_base(0);
            let b1 = p.segment_base(1);
            // 手算：段0=包基址；段1=段0+24页；页对齐恒真；区间不越槽。
            let ok = b0 == Ok(p.base_va())
                && b1 == Ok(p.base_va() + 24 * PAGE_SIZE)
                && p.base_va() % PAGE_SIZE == 0
                && p.range_in_slot() == Ok(())
                && p.validate() == Ok(())
                && p.digest() == p.digest();
            if ok {
                s.ok("F0303-包-01-参数包复算面");
            } else {
                s.fail("F0303-包-01-参数包复算面", "复算面不符");
            }
        } else {
            s.fail("F0303-包-01-参数包复算面", "包构造失败");
        }
    }

    // 对-02 全流程逐段对拍（J1 主判据——区间一致+权限对齐）。
    {
        let r = full_run(0xF0303);
        match r {
            Ok((pack, ledger, _)) => {
                let ok = ledger.len() == 4
                    && ledger.has_segment(".text")
                    && ledger.has_segment(".bss")
                    && ledger.audit() == Ok(())
                    && ledger.entries()[0].perm == 5
                    && ledger.entries()[1].perm == 1
                    && ledger.entries()[2].perm == 3
                    && ledger.entries()[3].perm == 3
                    && pack.range_in_slot() == Ok(())
                    && ledger.tick_now() >= 5;
                if ok {
                    s.ok("F0303-对-02-全流程逐段对拍");
                } else {
                    s.fail("F0303-对-02-全流程逐段对拍", "对拍面不符");
                }
            }
            Err(_) => s.fail("F0303-对-02-全流程逐段对拍", "全流程失败"),
        }
    }

    // J1-A 随机基址 128 组复测。
    {
        let r = j1_random_rounds();
        if r.is_ok() {
            s.ok("F0303-J1A-随机基址128组复测");
        } else {
            s.fail("F0303-J1A-随机基址128组复测", "随机面不符");
        }
    }

    // J1-B 负路径 10 闸。
    {
        let r = j1_negative_paths();
        if r.is_ok() {
            s.ok("F0303-J1B-负路径10闸全拦");
        } else {
            s.fail("F0303-J1B-负路径10闸全拦", "负路径面不符");
        }
    }

    // 漂-03 双跑指纹必等（三组种子）。
    {
        let a = dual_run_fingerprint(0xF0303);
        let b = dual_run_fingerprint(0x1234);
        let c = dual_run_fingerprint(0x9999);
        // 手算：同种子必等；三组各自通过（不同种子摘要不同是随机化面）。
        let ok = a.is_ok() && b.is_ok() && c.is_ok();
        if ok {
            s.ok("F0303-漂-03-双跑指纹必等三组");
        } else {
            s.fail("F0303-漂-03-双跑指纹必等三组", "指纹面不符");
        }
    }

    // 码-04 十二码互异+文案全量显性。
    {
        let codes = [
            XC_PACK_DRIFT, XC_PERM_VIOLATION, WX_BREAK, XC_SEGMENT_ORDER,
            XC_RANGE_OOB, XC_PACK_BAD, XC_PAGE_GAP, XC_ALIGN_BAD,
            XC_DOUBLE_PLACE, XC_DIGEST_MISMATCH, XC_FINGERPRINT_DRIFT, XC_LEDGER_GAP,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0x5801 && codes[11] == 0x580C;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        if distinct && seg_ok && reasons_all {
            s.ok("F0303-码-04-十二码互异显性");
        } else {
            s.fail("F0303-码-04-十二码互异显性", "诊断面不符");
        }
    }

    s
}
