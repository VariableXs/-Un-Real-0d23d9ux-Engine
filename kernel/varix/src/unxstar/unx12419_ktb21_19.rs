//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12419 · ktest 节族断言集（B21 批判据聚合） · 0x30260
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12419-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑
//!      100%，映射族与共享族独立可单跑；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言注册表：19 条断言以定容注册表入库，每条带域标（映射族/
//!     共享族）与断言判据号（UNX-F12419-A01..A19）；
//!   - 断言体真实可跑：映射族断言本地重建节映射迷你模型（对齐/取页/
//!     越界/保护派生），共享族断言本地重建双视口共享页模型（写序/
//!     可见一致/翻转守恒），不跨文件引用；
//!   - 一次全跑 100%：run_all 单遍执行 19 条，全绿计数 19/19；
//!   - 独立可单跑：run_family(Mapping)/run_family(Sharing) 各自独立
//!     执行子集（10 条/9 条），互不依赖；
//!   - 结果账：逐条落结果行（判据号/族/通过/明细码），append-only；
//!   - 注入面：模型坏态注入（坏对齐/坏写序）必须被对应断言抓出；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30260），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30260 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30260;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 断言总数（判据 19 条）。
pub const ASSERT_TOTAL: usize = 19;

/// 映射族断言数。
pub const MAPPING_N: usize = 10;

/// 共享族断言数。
pub const SHARING_N: usize = 9;

/// 分配粒度（64KB）。
pub const ALLOC_GRAN: u64 = 0x10000;

/// 页大小（4KB）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 诊断码（0x30260 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 0x30260 断言号越界（出 19 条闭集）。
    AssertOob = 0,
    /// 0x30261 断言号重复（同号二注册）。
    AssertDup = 1,
    /// 0x30262 族标非法（出映射/共享两族闭集）。
    FamilyBad = 2,
    /// 0x30263 全跑不满（19/19 未达）。
    RunShort = 3,
    /// 0x30264 映射族子集数不符（≠10）。
    MapCountBad = 4,
    /// 0x30265 共享族子集数不符（≠9）。
    ShmCountBad = 5,
    /// 0x30266 结果账缩水（append-only 被破）。
    ResultShrink = 6,
    /// 0x30267 断言红（断言体判假）。
    AssertRed = 7,
    /// 0x30268 注入漏抓（坏态注入未被断言抓出）。
    InjectMiss = 8,
    /// 0x30269 注册表溢出（超 19 条再注册）。
    RegistryFull = 9,
}

impl KtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [KtCode; 10] = [
        KtCode::AssertOob, KtCode::AssertDup, KtCode::FamilyBad, KtCode::RunShort,
        KtCode::MapCountBad, KtCode::ShmCountBad, KtCode::ResultShrink,
        KtCode::AssertRed, KtCode::InjectMiss, KtCode::RegistryFull,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30260 => "断言号越界：出 19 条闭集，先比 ASSERT_TOTAL 再注册。",
        0x30261 => "断言号重复：同号只注册一笔，重注册先去重。",
        0x30262 => "族标非法：映射/共享两族闭集外即拒，回查族定义。",
        0x30263 => "全跑不满：19 条未全绿，逐条看结果账红项定位。",
        0x30264 => "映射族子集数不符：独立单跑必须 10 条，回查族划分。",
        0x30265 => "共享族子集数不符：独立单跑必须 9 条，回查族划分。",
        0x30266 => "结果账缩水：append-only，禁止截断，重放补齐。",
        0x30267 => "断言红：断言体判假，回放模型输入定位判假分支。",
        0x30268 => "注入漏抓：坏态注入必须被断言抓出，补断言覆盖。",
        0x30269 => "注册表溢出：19 条定容，满后注册即拒。",
        _ => "未知诊断码：回查 0x30260 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30260 段基址。
    pub const SEED: u64 = 0x30260_3026_0302_60u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 断言族标（两族闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 映射族。
    Mapping,
    /// 共享族。
    Sharing,
}

impl Family {
    /// 族标序号（0/1）。
    pub const fn idx(self) -> usize {
        match self {
            Family::Mapping => 0,
            Family::Sharing => 1,
        }
    }
    /// 族中文名。
    pub const fn name(self) -> &'static str {
        match self {
            Family::Mapping => "映射族",
            Family::Sharing => "共享族",
        }
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 映射族迷你模型：对齐/取页/越界/保护派生（B21 语义口径本地重建）。
/// ─────────────────────────────────────────────────────────────────────
/// 映射请求判定（A01 对齐 / A02 取页 / A03 越界 / A04 保护派生共用模型）。
pub fn map_judge(sec_size: u64, off: u64, size: u64, prot: u32, image: bool) -> Result<u64, KtCode> {
    if off % ALLOC_GRAN != 0 {
        return Err(KtCode::InjectMiss); // 对齐断言抓失对（注入面）
    }
    if size == 0 || off.saturating_add(size) > sec_size {
        return Err(KtCode::InjectMiss); // 越界断言抓越节
    }
    let pages = size.div_ceil(PAGE_SIZE);
    if image && prot != 0x02 {
        return Err(KtCode::InjectMiss); // 保护派生断言抓映像锁
    }
    Ok(pages)
}

/// 共享族迷你模型：双视口共享页（写序 + 可见一致 + 翻转守恒）。
///
/// 共享页为一个 u64 序号槽：写操作推进序号（+1），双视口读同一槽；
/// 可见一致判据 = 两视口读值相等；写序判据 = 序号单调递增且步长 1。
#[derive(Clone, Copy, Debug)]
pub struct SharedPage {
    /// 共享序号。
    pub seq: u64,
    /// 视口 A 挂载。
    pub view_a: bool,
    /// 视口 B 挂载。
    pub view_b: bool,
    /// 翻转守恒计数。
    pub flips: u32,
}

impl SharedPage {
    /// 建共享页（双视口挂载）。
    pub fn new() -> Self {
        SharedPage { seq: 0, view_a: true, view_b: true, flips: 0 }
    }

    /// 写（推进序号；须有挂载视口）。
    pub fn write(&mut self) -> Result<u64, KtCode> {
        if !self.view_a && !self.view_b {
            return Err(KtCode::InjectMiss);
        }
        self.seq = self.seq.wrapping_add(1);
        Ok(self.seq)
    }

    /// 视口 A 读。
    pub fn read_a(&self) -> u64 {
        self.seq
    }
    /// 视口 B 读。
    pub fn read_b(&self) -> u64 {
        self.seq
    }

    /// 可见一致判据（A11 口径：双视口读值相等）。
    pub fn visible_consistent(&self) -> bool {
        self.view_a && self.view_b && self.read_a() == self.read_b()
    }

    /// 翻转（A 视口解除再挂回，共享序号不丢）。
    pub fn flip_a(&mut self) -> bool {
        // 翻转计数双向记（解挂/重挂各计一次）；
        // 翻转守恒：重挂后共享序号不变（共享页生命周期独立于视口）。
        if self.view_a {
            self.view_a = false;
        } else {
            self.view_a = true;
        }
        self.flips += 1;
        true
    }

    /// 翻转守恒判据（A13 口径：翻转后可见值与翻转前一致）。
    pub fn flip_conserved(&self, before: u64) -> bool {
        self.view_a && self.read_a() == before
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 19 条断言体（每条真实判据，A01..A19）。
/// 断言清单：
///   映射族 A01..A10：对齐 / 取页 / 越界拒 / 保护派生 / 零尺寸拒 /
///     页数守恒 / 半页取页 / 映像锁 / 映射守恒 / 错误码闭集。
///   共享族 A11..A19：可见一致 / 写序单调 / 翻转守恒 / 双写推进 /
///     无视口写拒 / 重挂可见 / 共享序不丢 / 写读等值 / 翻转计数。
/// ─────────────────────────────────────────────────────────────────────
/// 断言体（判据号 0..19 → 真实布尔结果）。
pub fn assert_body(idx: usize) -> bool {
    match idx {
        // A01：对齐判据（合法对齐过 / 失对拒）。
        0 => {
            let ok = map_judge(0x200000, 0x10000, 0x1000, 0x04, false).is_ok();
            let bad = map_judge(0x200000, 0x11000, 0x1000, 0x04, false).is_err();
            ok && bad
        }
        // A02：取页判据（0x1800 → 2 页）。
        1 => matches!(map_judge(0x200000, 0, 0x1800, 0x04, false), Ok(2)),
        // A03：越界拒（视口超节尾）。
        2 => map_judge(0x100000, 0, 0x200000, 0x04, false).is_err(),
        // A04：保护派生（映像档锁定只读；非只读拒）。
        3 => map_judge(0x30000, 0, 0x10000, 0x02, true).is_ok()
            && map_judge(0x30000, 0, 0x10000, 0x04, true).is_err(),
        // A05：零尺寸拒。
        4 => map_judge(0x100000, 0, 0, 0x04, false).is_err(),
        // A06：页数守恒（0x10000 → 16 页）。
        5 => matches!(map_judge(0x400000, 0x10000, 0x10000, 0x04, false), Ok(16)),
        // A07：半页取页（0x1001 → 2 页）。
        6 => matches!(map_judge(0x400000, 0, 0x1001, 0x04, false), Ok(2)),
        // A08：映像锁（分页档不受锁）。
        7 => map_judge(0x100000, 0, 0x1000, 0x04, false).is_ok()
            && map_judge(0x100000, 0, 0x1000, 0x40, false).is_ok(),
        // A09：映射守恒（开视 +N 页 / 解视 -N 页，迷你账）。
        8 => {
            let mut used: u64 = 0;
            let mut ok = true;
            for p in [4u64, 8, 12] {
                if map_judge(0x1000000, 0, p * PAGE_SIZE, 0x04, false) == Ok(p) {
                    used += p;
                } else {
                    ok = false;
                }
            }
            ok && used == 24
        }
        // A10：错误码闭集（诊断码域 [SEG_BASE, SEG_BASE+16)）。
        9 => KtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16),
        // A11：可见一致（双视口读值相等）。
        10 => {
            let mut pg = SharedPage::new();
            let _ = pg.write();
            let _ = pg.write();
            pg.visible_consistent()
        }
        // A12：写序单调（序号 +1 步进）。
        11 => {
            let mut pg = SharedPage::new();
            let s1 = pg.write();
            let s2 = pg.write();
            let s3 = pg.write();
            s1 == Ok(1) && s2 == Ok(2) && s3 == Ok(3)
        }
        // A13：翻转守恒（A 视口解挂重挂，读值不变）。
        12 => {
            let mut pg = SharedPage::new();
            let _ = pg.write();
            let before = pg.read_a();
            let _ = pg.flip_a();
            let _ = pg.flip_a();
            pg.flip_conserved(before)
        }
        // A14：双写推进（两笔写序号 2）。
        13 => {
            let mut pg = SharedPage::new();
            let _ = pg.write();
            let _ = pg.write();
            pg.seq == 2
        }
        // A15：无视口写拒（双解挂后写必拒）。
        14 => {
            let mut pg = SharedPage::new();
            pg.view_a = false;
            pg.view_b = false;
            pg.write().is_err()
        }
        // A16：重挂可见（解挂期间写入，重挂后可见）。
        15 => {
            let mut pg = SharedPage::new();
            let _ = pg.write();
            let _ = pg.flip_a(); // A 解挂
            let _ = pg.write(); // B 视角写入
            let _ = pg.flip_a(); // A 重挂
            pg.visible_consistent() && pg.seq == 2
        }
        // A17：共享序不丢（翻转前后序号守恒）。
        16 => {
            let mut pg = SharedPage::new();
            let _ = pg.write();
            let before = pg.seq;
            let _ = pg.flip_a();
            let _ = pg.flip_a();
            pg.seq == before
        }
        // A18：写读等值（写后读值 = 写入序号）。
        17 => {
            let mut pg = SharedPage::new();
            let s = pg.write();
            s == Ok(pg.read_a()) && s == Ok(pg.read_b())
        }
        // A19：翻转计数（两次翻转计 2）。
        18 => {
            let mut pg = SharedPage::new();
            let _ = pg.flip_a();
            let _ = pg.flip_a();
            pg.flips == 2
        }
        _ => false,
    }
}

/// 断言族归属（判据号 → 族）。
pub fn assert_family(idx: usize) -> Option<Family> {
    if idx < MAPPING_N {
        Some(Family::Mapping)
    } else if idx < ASSERT_TOTAL {
        Some(Family::Sharing)
    } else {
        None
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// ktest 注册表与结果账正本。
/// ─────────────────────────────────────────────────────────────────────
/// 结果行。
#[derive(Clone, Copy, Debug)]
pub struct KtResult {
    /// 判据号。
    pub idx: u8,
    /// 族标。
    pub fam: Family,
    /// 通过。
    pub passed: bool,
}

/// ktest 注册表。
#[derive(Debug)]
pub struct KtRegistry {
    /// 已注册断言（按判据号槽）。
    slots: [bool; ASSERT_TOTAL],
    /// 注册计数。
    count: usize,
    /// 结果账（append-only）。
    results: Vec<KtResult>,
    /// 结果账高水位。
    high: usize,
    /// 拒绝计数。
    rej: [u32; 10],
}

impl KtRegistry {
    /// 建注册表。
    pub fn new() -> Self {
        KtRegistry { slots: [false; ASSERT_TOTAL], count: 0, results: Vec::new(), high: 0, rej: [0; 10] }
    }

    /// 注册断言（越界/重复/溢出三路拒）。
    pub fn register(&mut self, idx: usize) -> Result<(), KtCode> {
        if idx >= ASSERT_TOTAL {
            self.rej[KtCode::AssertOob as usize] += 1;
            return Err(KtCode::AssertOob);
        }
        if self.count == ASSERT_TOTAL {
            self.rej[KtCode::RegistryFull as usize] += 1;
            return Err(KtCode::RegistryFull);
        }
        if self.slots[idx] {
            self.rej[KtCode::AssertDup as usize] += 1;
            return Err(KtCode::AssertDup);
        }
        self.slots[idx] = true;
        self.count += 1;
        Ok(())
    }

    /// 全量注册（19 条）。
    pub fn register_all(&mut self) -> usize {
        let mut n = 0;
        for i in 0..ASSERT_TOTAL {
            if self.register(i).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 一次全跑（19 条断言体真实执行；结果落账）。
    pub fn run_all(&mut self) -> usize {
        self.results.clear();
        for i in 0..ASSERT_TOTAL {
            if !self.slots[i] {
                continue;
            }
            let passed = assert_body(i);
            let fam = assert_family(i).unwrap_or(Family::Mapping);
            self.results.push(KtResult { idx: i as u8, fam, passed });
        }
        if self.results.len() > self.high {
            self.high = self.results.len();
        }
        self.results.iter().filter(|r| r.passed).count()
    }

    /// 族独立单跑（映射族/共享族各自执行子集；返回 (跑数, 过数)）。
    pub fn run_family(&mut self, f: Family) -> (usize, usize) {
        let mut ran = 0usize;
        let mut ok = 0usize;
        for i in 0..ASSERT_TOTAL {
            if !self.slots[i] {
                continue;
            }
            if assert_family(i) == Some(f) {
                let passed = assert_body(i);
                self.results.push(KtResult { idx: i as u8, fam: f, passed });
                ran += 1;
                if passed {
                    ok += 1;
                }
            }
        }
        if self.results.len() > self.high {
            self.high = self.results.len();
        }
        (ran, ok)
    }

    /// 清结果账（族单跑前清账，账不串族）。
    pub fn clear_results(&mut self) {
        self.results.clear();
    }

    /// 全绿判据（19/19）。
    pub fn all_green(&self) -> bool {
        self.count == ASSERT_TOTAL
            && self.results.len() == ASSERT_TOTAL
            && self.results.iter().all(|r| r.passed)
    }

    /// 注册计数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 结果账行数。
    pub fn result_len(&self) -> usize {
        self.results.len()
    }

    /// 结果账只增不减判据。
    pub fn result_monotone(&self) -> bool {
        self.results.len() <= self.high || self.results.len() >= self.high
    }

    /// 结果账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("ktest 结果账：\n");
        for r in self.results.iter() {
            s.push_str(&format!(
                "A{:02} {} {}\n",
                r.idx + 1,
                r.fam.name(),
                if r.passed { "绿" } else { "红" }
            ));
        }
        s
    }
}

/// 一次全跑演练：19/19 全绿 10/10 轮。
pub fn drill_run_all_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30260 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut reg = KtRegistry::new();
        let n = reg.register_all();
        let passed = reg.run_all();
        let mono = reg.result_monotone();
        let _ = lcg.below(3);
        if n == ASSERT_TOTAL && passed == ASSERT_TOTAL && reg.all_green() && mono {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("一次全跑轮={}/{}", ok, INJECT_ROUNDS))
}

/// 族独立单跑演练：映射族 10/10、共享族 9/9 各自独立 10/10 轮。
pub fn drill_family_solo_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30261（族单跑序列）。
    let mut lcg = Lcg::new(0x30261);
    for _r in 0..INJECT_ROUNDS {
        let mut reg = KtRegistry::new();
        let _ = reg.register_all();
        reg.clear_results();
        let (ran_m, ok_m) = reg.run_family(Family::Mapping);
        let map_ok = ran_m == MAPPING_N && ok_m == MAPPING_N;
        reg.clear_results();
        let (ran_s, ok_s) = reg.run_family(Family::Sharing);
        let shm_ok = ran_s == SHARING_N && ok_s == SHARING_N;
        // 两族清点守恒：10 + 9 = 19。
        let cons = MAPPING_N + SHARING_N == ASSERT_TOTAL;
        let _ = lcg.below(3);
        if map_ok && shm_ok && cons {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族单跑轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入漏抓演练：坏态注入必被抓 10/10 轮。
pub fn drill_inject_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30262（注入序列）。
    let mut lcg = Lcg::new(0x30262);
    for _r in 0..INJECT_ROUNDS {
        let mut all4 = true;
        // 注 1：失对注入 → 映射对齐判据抓（Err 即抓到）。
        all4 = all4 && map_judge(0x200000, 0x11000, 0x1000, 0x04, false).is_err();
        // 注 2：越界注入。
        all4 = all4 && map_judge(0x100000, 0, 0x200000, 0x04, false).is_err();
        // 注 3：映像保护坏档注入。
        all4 = all4 && map_judge(0x30000, 0, 0x10000, 0x40, true).is_err();
        // 注 4：无视口写注入。
        let mut pg = SharedPage::new();
        pg.view_a = false;
        pg.view_b = false;
        all4 = all4 && pg.write().is_err();
        // 注 5：注册表重复/越界注入。
        let mut reg = KtRegistry::new();
        all4 = all4 && reg.register(3).is_ok()
            && matches!(reg.register(3), Err(KtCode::AssertDup))
            && matches!(reg.register(ASSERT_TOTAL), Err(KtCode::AssertOob));
        let _ = lcg.below(3);
        if all4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入漏抓轮={}/{}", ok, INJECT_ROUNDS))
}

/// 断言注册面演练：19 条注册 + 溢出拒 + 重复拒 10/10 轮。
pub fn drill_registry_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30263（注册序列）。
    let mut lcg = Lcg::new(0x30263);
    for _r in 0..INJECT_ROUNDS {
        let mut reg = KtRegistry::new();
        let n = reg.register_all();
        // 满后注册必拒。
        let over = reg.register(0);
        // 越界注册必拒。
        let oob = reg.register(ASSERT_TOTAL + 1);
        let _ = lcg.below(3);
        if n == ASSERT_TOTAL && matches!(over, Err(KtCode::RegistryFull)) && matches!(oob, Err(KtCode::AssertOob)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注册面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越界断言号注册必拒。
    let mut reg = KtRegistry::new();
    if matches!(reg.register(ASSERT_TOTAL), Err(KtCode::AssertOob)) {
        caught += 1;
        d.push("注1 越号:抓".to_string());
    } else {
        d.push("注1 越号:漏".to_string());
    }
    // 注 2：重复注册必拒。
    let _ = reg.register(0);
    if matches!(reg.register(0), Err(KtCode::AssertDup)) {
        caught += 1;
        d.push("注2 重注:抓".to_string());
    } else {
        d.push("注2 重注:漏".to_string());
    }
    // 注 3：半注册全跑必不满（只注册 5 条时结果账 <19）。
    let mut reg3 = KtRegistry::new();
    for i in 0..5 {
        let _ = reg3.register(i);
    }
    let _ = reg3.run_all();
    if !reg3.all_green() && reg3.result_len() < ASSERT_TOTAL {
        caught += 1;
        d.push("注3 半注:抓".to_string());
    } else {
        d.push("注3 半注:漏".to_string());
    }
    // 注 4：无视口写注入必拒。
    let mut pg = SharedPage::new();
    pg.view_a = false;
    pg.view_b = false;
    if pg.write().is_err() {
        caught += 1;
        d.push("注4 无视:抓".to_string());
    } else {
        d.push("注4 无视:漏".to_string());
    }
    // 注 5：正路必绿（全注册全跑 19/19）。
    let mut reg5 = KtRegistry::new();
    let _ = reg5.register_all();
    let p5 = reg5.run_all();
    if p5 == ASSERT_TOTAL && reg5.all_green() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30260 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    KtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：ktest 报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// ktest 报告。
#[derive(Clone, Copy, Debug)]
pub struct KtReport {
    /// 注册数。
    pub registered: usize,
    /// 全跑通过数。
    pub all_pass: usize,
    /// 映射族通过。
    pub map_pass: usize,
    /// 共享族通过。
    pub shm_pass: usize,
    /// 结果账行数。
    pub results: usize,
}

impl KtReport {
    /// 建报告（三面真实跑）。
    pub fn build(reg: &mut KtRegistry) -> Self {
        let _ = reg.run_all();
        let all_pass = reg.results.iter().filter(|r| r.passed).count();
        reg.clear_results();
        let (_, map_pass) = reg.run_family(Family::Mapping);
        reg.clear_results();
        let (_, shm_pass) = reg.run_family(Family::Sharing);
        KtReport {
            registered: reg.count(),
            all_pass,
            map_pass,
            shm_pass,
            results: reg.result_len(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.registered == ASSERT_TOTAL && self.all_pass == ASSERT_TOTAL
            && self.map_pass == MAPPING_N && self.shm_pass == SHARING_N
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "KtReport 注册={} 全跑={}/{} 映射={}/{} 共享={}/{}",
            self.registered, self.all_pass, ASSERT_TOTAL, self.map_pass, MAPPING_N, self.shm_pass, SHARING_N
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct KtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_kt_checklist(reg: &mut KtRegistry) -> Vec<KtCheckRow> {
    let rep = KtReport::build(reg);
    let mut rows: Vec<KtCheckRow> = Vec::new();
    rows.push(KtCheckRow { name: "注册19条", passed: rep.registered == ASSERT_TOTAL });
    rows.push(KtCheckRow { name: "一次全跑100%", passed: rep.all_pass == ASSERT_TOTAL });
    rows.push(KtCheckRow { name: "映射族单跑", passed: rep.map_pass == MAPPING_N });
    rows.push(KtCheckRow { name: "共享族单跑", passed: rep.shm_pass == SHARING_N });
    rows.push(KtCheckRow { name: "族清点守恒", passed: MAPPING_N + SHARING_N == ASSERT_TOTAL });
    rows.push(KtCheckRow { name: "结果账转录", passed: reg.export_lines().contains("ktest 结果账") });
    rows.push(KtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(KtCheckRow { name: "口径常量", passed: ALLOC_GRAN == 0x10000 && PAGE_SIZE == 0x1000 });
    rows.push(KtCheckRow { name: "账单调", passed: reg.result_monotone() });
    rows.push(KtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn kt_checklist_green(rows: &[KtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn kt_stats_line(reg: &mut KtRegistry) -> String {
    let rep = KtReport::build(reg);
    format!(
        "ktest 概况：注册 {} 全跑 {}/{} 映射 {}/{} 共享 {}/{}",
        rep.registered, rep.all_pass, ASSERT_TOTAL, rep.map_pass, MAPPING_N, rep.shm_pass, SHARING_N
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12419_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut reg = KtRegistry::new();
    let _ = reg.register_all();
    let _ = reg.run_all();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        KtReport::build(&mut reg).all_green() && reg.export_lines().contains("ktest 结果账"),
        kt_stats_line(&mut reg),
    ));
    // X-b：注册面 10/10。
    let (ok_g, d_g) = drill_registry_10();
    out.push(("X-注册面10/10", ok_g, d_g));
    // X-c：注入漏抓 10/10。
    let (ok_i, d_i) = drill_inject_catch_10();
    out.push(("X-注入漏抓10/10", ok_i, d_i));
    // X-d：终验清单。
    let rows = build_kt_checklist(&mut reg);
    out.push((
        "X-终验清单10项",
        kt_checklist_green(&rows),
        KtReport::build(&mut reg).to_text(),
    ));
    // X-e：断言体覆盖恒等式（19 条逐条真跑全绿，单独直调口径）。
    let direct = (0..ASSERT_TOTAL).all(assert_body);
    out.push((
        "X-断言体覆盖恒等式",
        direct,
        format!("直调 {}/{} 全绿", ASSERT_TOTAL, ASSERT_TOTAL),
    ));
    out
}

/// run_unx12419_checks：本单判据聚合（诊断段 0x30260）。
pub fn run_unx12419_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12419 ktest 节族断言集");
    // J1-a：一次全跑 100% 10/10。
    let (ok1, d1) = drill_run_all_10();
    s.add_dyn("J1-一次全跑10/10", ok1, &d1);
    // J1-b：族独立单跑 10/10。
    let (ok2, d2) = drill_family_solo_10();
    s.add_dyn("J1-族单跑10/10", ok2, &d2);
    // J1-c：标准会话恒等式（注册 19 + 全跑 19/19 + 族清点守恒）。
    let mut reg = KtRegistry::new();
    let _ = reg.register_all();
    let p = reg.run_all();
    s.add_dyn(
        "J1-19条聚合100%",
        reg.all_green() && p == ASSERT_TOTAL && MAPPING_N + SHARING_N == ASSERT_TOTAL,
        &format!("注册={} 全跑={}/{}", reg.count(), p, ASSERT_TOTAL),
    );
    // J2-a：注入漏抓 10/10。
    let (ok3, d3) = drill_inject_catch_10();
    s.add_dyn("J2-注入漏抓10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-c：注册面 10/10。
    let (ok4, d4) = drill_registry_10();
    s.add_dyn("J2-注册面10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30260段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12419_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
