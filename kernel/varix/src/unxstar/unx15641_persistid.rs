//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15641 · IPersist 根接口与 CLSID 律 · 0x67600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B23）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15641-J1 根接口成文（GetClassID 唯一稳定律 10/10 次），
//!     漂移注入（同对象两次 GetClassID 结果不同）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - CLSID 唯一稳定律：同一持久对象生命周期内 GetClassID 恒等
//!     （任意两次调用逐字节一致）；漂移 = 注入，如实检出；
//!   - 持久对象状态机：Blank（未载）→ Loaded（载入流）→ Dirty
//!     （脏）→ Saved（存盘回净）；Dirty/Saved 间回环合法，类ID
//!     跨态不变（律的载体）；
//!   - CLSID 建模：16 字节闭集（标准 GUID 尺寸），注册表池写死；
//!     同池两对象可同类，一对象一池类；
//!   - 漂移注入面：篡改对象类ID 后再取 → 与账比对不一致必检出，
//!     10/10 全抓；
//!   - 守恒恒等式：类ID 取用计数 = 对象数 × 每对象取用轮数；
//!   - LCG 种子写死 = SEG_BASE（0x67600）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67600，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67600;

// ── CLSID 尺寸与注册池（写死：标准 GUID 16 字节） ────────────────
pub const CLSID_BYTES: usize = 16;
/// 注册池：4 个预置类（来源：典型持久组件类建模，写死）。
pub const POOL_DOC: usize = 0;
pub const POOL_PICTURE: usize = 1;
pub const POOL_STREAM: usize = 2;
pub const POOL_STORAGE: usize = 3;
pub const POOL_COUNT: usize = 4;
pub const POOL_NAMES: [&str; POOL_COUNT] =
    ["clsid:doc", "clsid:picture", "clsid:stream", "clsid:storage"];

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersistCode {
    /// 类池下标越闭集
    PoolOutOfRange,
    /// 类ID 字节长度不合法（≠16）
    ClsidBadLength,
    /// 漂移：同对象两次 GetClassID 不一致（如实检出）
    ClsidDrift,
    /// 持久态非法迁移（Saved→Loaded 回载）
    IllegalPersistState,
    /// 未载即存（Dirty 前提）
    SaveBeforeLoad,
    /// 脏态不存即终（账面悬挂）
    DirtyHung,
    /// 稳定律守恒破坏（取用计数对不上）
    StabilityConservationBroken,
    /// 类ID 全零（未初始化类ID 拒收）
    ClsidAllZero,
    /// 漂移注入漏抓
    DriftLeak,
    /// 双跑不一致
    NotReproducible,
}

impl PersistCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PersistCode::PoolOutOfRange => "类池下标越闭集：4 预置类冻结",
            PersistCode::ClsidBadLength => "类ID 字节长度不合法：16 字节冻结",
            PersistCode::ClsidDrift => "漂移：同对象两次取类ID 不一致，如实检出",
            PersistCode::IllegalPersistState => "持久态非法迁移：终态回载",
            PersistCode::SaveBeforeLoad => "未载即存：脏前提缺失",
            PersistCode::DirtyHung => "脏态不存即终：账面悬挂",
            PersistCode::StabilityConservationBroken => "稳定律守恒破坏：取用计数对不上",
            PersistCode::ClsidAllZero => "类ID 全零：未初始化拒收",
            PersistCode::DriftLeak => "漂移注入漏抓：检出 10/10 红线",
            PersistCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PersistCode; 10] = [
        PersistCode::PoolOutOfRange,
        PersistCode::ClsidBadLength,
        PersistCode::ClsidDrift,
        PersistCode::IllegalPersistState,
        PersistCode::SaveBeforeLoad,
        PersistCode::DirtyHung,
        PersistCode::StabilityConservationBroken,
        PersistCode::ClsidAllZero,
        PersistCode::DriftLeak,
        PersistCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PersistCode::ALL.len() {
        if PersistCode::ALL[i].val() == code {
            return PersistCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67600 段闭集内"
}

// ── CLSID 建模 ───────────────────────────────────────────────────
/// 类池字节表（写死：池 i 的类ID = 0x67600+i 派生的稳定字节列）。
pub fn pool_clsid(pool: usize) -> Result<[u8; CLSID_BYTES], u32> {
    if pool >= POOL_COUNT {
        return Err(PersistCode::PoolOutOfRange.val());
    }
    let base = SEG_BASE as u64 + pool as u64;
    let mut out = [0u8; CLSID_BYTES];
    let mut i = 0;
    while i < CLSID_BYTES {
        // 写死派生：逐字节取 base 旋转哈希（确定性，非随机）。
        let v = base
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(i as u64 * 0x2545F4914F6CDD1D);
        out[i] = (v >> 24) as u8;
        i += 1;
    }
    // 反向判据基线：全零类ID 非法（池派生恒非全零）。
    let mut all_zero = true;
    let mut i = 0;
    while i < CLSID_BYTES {
        if out[i] != 0 {
            all_zero = false;
        }
        i += 1;
    }
    if all_zero {
        return Err(PersistCode::ClsidAllZero.val());
    }
    Ok(out)
}

pub fn clsid_equal(a: &[u8; CLSID_BYTES], b: &[u8; CLSID_BYTES]) -> bool {
    let mut i = 0;
    while i < CLSID_BYTES {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 类ID 全零判。
pub fn clsid_all_zero(c: &[u8; CLSID_BYTES]) -> bool {
    let mut i = 0;
    while i < CLSID_BYTES {
        if c[i] != 0 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 持久对象状态机 ───────────────────────────────────────────────
/// 四态闭集：Blank → Loaded ↔ Dirty → Saved（可再 Dirty）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersistState {
    Blank,
    Loaded,
    Dirty,
    Saved,
}

pub fn persist_can_move(from: PersistState, to: PersistState) -> bool {
    match (from, to) {
        (PersistState::Blank, PersistState::Loaded) => true,
        (PersistState::Loaded, PersistState::Dirty) => true,
        (PersistState::Dirty, PersistState::Saved) => true,
        (PersistState::Saved, PersistState::Dirty) => true,
        (PersistState::Dirty, PersistState::Dirty) => true, // 再脏幂等
        _ => false,
    }
}

/// 持久对象账（IPersist 根建模）。
pub struct PersistObj {
    pub pool: usize,
    pub clsid: [u8; CLSID_BYTES],
    pub state: PersistState,
    /// GetClassID 取用账（稳定律取用计数）。
    pub get_calls: u32,
    /// 每次取用快照（稳定律对账）。
    pub snapshots: Vec<[u8; CLSID_BYTES]>,
    pub saves: u32,
    pub dirties: u32,
}

impl PersistObj {
    pub fn new(pool: usize) -> Result<PersistObj, u32> {
        let c = pool_clsid(pool)?;
        Ok(PersistObj {
            pool,
            clsid: c,
            state: PersistState::Blank,
            get_calls: 0,
            snapshots: Vec::new(),
            saves: 0,
            dirties: 0,
        })
    }

    fn step(&mut self, to: PersistState) -> Result<(), u32> {
        if !persist_can_move(self.state, to) {
            return Err(PersistCode::IllegalPersistState.val());
        }
        self.state = to;
        Ok(())
    }

    pub fn load(&mut self) -> Result<(), u32> {
        self.step(PersistState::Loaded)
    }
    pub fn dirty(&mut self) -> Result<(), u32> {
        if self.state == PersistState::Blank {
            return Err(PersistCode::SaveBeforeLoad.val());
        }
        self.step(PersistState::Dirty)?;
        self.dirties += 1;
        Ok(())
    }
    pub fn save(&mut self) -> Result<(), u32> {
        if self.state != PersistState::Dirty {
            return Err(PersistCode::SaveBeforeLoad.val());
        }
        self.step(PersistState::Saved)?;
        self.saves += 1;
        Ok(())
    }

    /// GetClassID：类ID 恒等返回 + 快照落账（唯一稳定律载体）。
    pub fn get_class_id(&mut self) -> Result<[u8; CLSID_BYTES], u32> {
        if clsid_all_zero(&self.clsid) {
            return Err(PersistCode::ClsidAllZero.val());
        }
        self.get_calls += 1;
        self.snapshots.push(self.clsid);
        Ok(self.clsid)
    }

    /// 稳定律校验：所有快照与当前类ID 逐字节一致。
    pub fn stability(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.snapshots.len() {
            if !clsid_equal(&self.snapshots[i], &self.clsid) {
                return Err(PersistCode::ClsidDrift.val());
            }
            i += 1;
        }
        if self.get_calls as usize != self.snapshots.len() {
            return Err(PersistCode::StabilityConservationBroken.val());
        }
        Ok(())
    }

    /// 生命周期收敛：非 Blank 必经 Dirty→Saved 或 Loaded 静止合法；
    /// Dirty 悬挂拒。
    pub fn converge(&self) -> Result<(), u32> {
        if self.state == PersistState::Dirty && self.saves == 0 {
            return Err(PersistCode::DirtyHung.val());
        }
        Ok(())
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

// ── 唯一稳定律 10/10 轮 ──────────────────────────────────────────
pub const GOLDEN_ROUNDS: u32 = 10;
/// 每轮 GetClassID 取用次数（写死：稳定律多取）。
pub const GETS_PER_ROUND: u32 = 3;

pub fn judge_stability_rounds() -> Result<(u32, u32), u32> {
    let mut stable_ok = 0;
    let mut lifecycle_ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let pool = r as usize % POOL_COUNT;
        let mut o = PersistObj::new(pool)?;
        o.load()?;
        o.dirty()?;
        o.save()?;
        // 跨态取类ID：3 次逐字节恒等。
        let mut same = true;
        let mut g = 0;
        let first = o.get_class_id()?;
        while g < GETS_PER_ROUND - 1 {
            let c = o.get_class_id()?;
            if !clsid_equal(&first, &c) {
                same = false;
            }
            g += 1;
        }
        o.stability()?;
        if same {
            stable_ok += 1;
        }
        // 生命周期收敛。
        o.converge()?;
        // 再脏再存回环。
        o.dirty()?;
        o.save()?;
        if o.saves == 2 {
            lifecycle_ok += 1;
        }
        r += 1;
    }
    Ok((stable_ok, lifecycle_ok))
}

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_STABLE_OK: u32 = 10;
pub const GOLDEN_DRIFT_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let (s, l) = judge_stability_rounds()?;
    if s != GOLDEN_STABLE_OK || l != GOLDEN_STABLE_OK {
        return Err(PersistCode::StabilityConservationBroken.val());
    }
    // 漂移注入 10/10：篡改类ID 后再取 → 与快照不一致必检出。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut o = PersistObj::new(POOL_DOC).ok();
        if let Some(ref mut obj) = o {
            let _ = obj.load();
            let _ = obj.get_class_id();
            obj.clsid[0] = obj.clsid[0].wrapping_add(1); // 漂移注入
            if let Err(e) = obj.stability() {
                if e == PersistCode::ClsidDrift.val() {
                    caught += 1;
                }
            }
        }
        r += 1;
    }
    if caught != GOLDEN_DRIFT_CAUGHT {
        return Err(PersistCode::DriftLeak.val());
    }
    Ok((s, l, caught))
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
    // 族 0：漂移注入 ×10 必检出。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut o = PersistObj::new(POOL_STREAM).ok();
        if let Some(ref mut obj) = o {
            let _ = obj.load();
            let _ = obj.get_class_id();
            obj.clsid[CLSID_BYTES - 1] = obj.clsid[CLSID_BYTES - 1].wrapping_add(1);
            if let Err(e) = obj.stability() {
                if e == PersistCode::ClsidDrift.val() {
                    ledger.caught[0] += 1;
                }
            }
        }
        r += 1;
    }
    // 族 1：类池越闭集注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = PersistObj::new(POOL_COUNT + 2) {
            if e == PersistCode::PoolOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：未载即存注入 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut o = PersistObj::new(POOL_PICTURE).ok();
        if let Some(ref mut obj) = o {
            if let Err(e) = obj.save() {
                if e == PersistCode::SaveBeforeLoad.val() {
                    ledger.caught[2] += 1;
                }
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法稳定律必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut o = PersistObj::new(POOL_STORAGE)?;
    let c0 = o.get_class_id()?;
    o.load()?;
    o.dirty()?;
    o.save()?;
    let c1 = o.get_class_id()?;
    if !clsid_equal(&c0, &c1) {
        return Err(PersistCode::ClsidDrift.val());
    }
    o.stability()?;
    o.converge()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PersistCode::ALL.len() {
        let v = PersistCode::ALL[i].val();
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
    // 1. 黄金正演：稳定律 10/10 + 漂移注入 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-稳定律10/10+漂移注入10/10检出",
        matches!(g, Ok((10, 10, 10))),
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
    // 3. 类池派生点验：4 池类ID 两两互异（唯一性反向配对）。
    let uniq = {
        let mut ok = true;
        let mut a = 0;
        while a < POOL_COUNT {
            let ca = pool_clsid(a);
            match ca {
                Ok(xa) => {
                    let mut b = a + 1;
                    while b < POOL_COUNT {
                        if let Ok(xb) = pool_clsid(b) {
                            if clsid_equal(&xa, &xb) {
                                ok = false;
                            }
                        }
                        b += 1;
                    }
                }
                Err(_) => ok = false,
            }
            a += 1;
        }
        ok
    };
    v.push(("J1-类池唯一性", uniq, format!("{} 池两两互异", POOL_COUNT)));
    // 4. 持久态边闭集：Saved→Loaded 回载恒非法。
    let closed = !persist_can_move(PersistState::Saved, PersistState::Loaded)
        && !persist_can_move(PersistState::Blank, PersistState::Saved);
    v.push(("J1-持久态边闭集", closed, String::from("Blank→Loaded↔Dirty→Saved")));
    // 5. 反向判据：合法稳定律必须全过。
    v.push(("J1-反向探针合法稳定律", matches!(reverse_probe(), Ok(())), String::from("跨态恒等全过")));
    // 6. 类ID 尺寸点验：16 字节冻结。
    v.push((
        "J1-类ID尺寸冻结",
        CLSID_BYTES == 16,
        format!("{} 字节", CLSID_BYTES),
    ));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x41);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67600 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15641_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15641 IPersist 根接口与 CLSID 律");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
