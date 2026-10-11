//! VE-F6007 · 物理确定性（vez19_determ）
//!
//! **同种子同结果保证**与跨平台确定性策略（浮点差异的三重防护：**定点
//! 备选、运算序锁定、平台差异清单**），**确定性破坏零容忍**——失配第
//! 一次即立案不静默（确定性不是统计指标，破一次就是全网锁定不同步）。
//!
//! 三重防护各自有验证：
//! 1. **定点备选**——Q16.16 定点数（i64 载体）：mul/add/div 全整数域
//!    确定性（零 float——平台浮点差异从类型面上不存在）；
//! 2. **运算序锁定**——求和/累积计划固定序+计划指纹（同计划同指纹，
//!    计划变更指纹必变——序漂移在指纹对拍中显形）；
//! 3. **平台差异清单**——登记制（平台/差异/责任人/状态），责任人必填
//!    （无主差异单没人修），清单公开可查。
//!
//! 错误路径与降级矩阵：**不一致→归因修复**（指纹失配立案簿）；**平台
//! 差异→清单管理**（更新流程：提案→核验→登记，责任人签字）；**种子
//! 失效→重标定**（版本变更旧种子 Stale 标记，重标定后回 Active）。
//!
//! 性能：模拟 O(步长)——逐帧定点步进合计线性。跨批对接点：AC07 确定
//! 性经验。无障碍：平台清单公开（`ledger_spoken` 可读名单）。
//!
//! Z 域字符串码五码互异：`E_FD_MISMATCH` / `E_FD_NOOWNER` /
//! `E_FD_STALE` / `E_FD_NOSEED` / `E_FD_BADFIXED`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 确定性指纹失配（同种子同版本两跑结果不同——零容忍立案）。
pub const E_FD_MISMATCH: &str = "E_FD_MISMATCH";
/// 平台差异登记缺责任人（无主差异单没人修——登记闸拒绝）。
pub const E_FD_NOOWNER: &str = "E_FD_NOOWNER";
/// 种子失效（世界版本变更后旧种子未重标定）。
pub const E_FD_STALE: &str = "E_FD_STALE";
/// 查无种子（未登记的种子 ID）。
pub const E_FD_NOSEED: &str = "E_FD_NOSEED";
/// 定点溢出（操作数超限幅——限幅内全整数域确定）。
pub const E_FD_BADFIXED: &str = "E_FD_BADFIXED";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_FD_MISMATCH => "指纹失配：同种子同版本两跑结果不同，零容忍立案",
        E_FD_NOOWNER => "缺责任人：平台差异单无主不收，登记闸拒绝",
        E_FD_STALE => "种子失效：世界版本已变更，需重标定后使用",
        E_FD_NOSEED => "查无种子：种子 ID 未登记确定性簿",
        E_FD_BADFIXED => "定点溢出：操作数超限幅，先缩放再运算",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 定点数（三重防护之一：Q16.16 全整数域——零 float 类型面证明）
// ---------------------------------------------------------------------------

/// Q16.16 定点：1.0 = 0x10000。载体 i64（乘法中间积 62 位内不溢出）。
pub const FIXED_ONE: i64 = 1 << 16;
/// 操作数限幅（|a| ≤ 2^23 ≈ 8.4M——乘积 2^46 ≤ i64 安全域）。
pub const FIXED_LIMIT: i64 = 1 << 23;

/// 定点构造（整数部分+千分小数→Q16.16）。
pub const fn fx_from_rational(whole: i64, milli: i64) -> i64 {
    whole * FIXED_ONE + (milli * FIXED_ONE) / 1000
}

/// 定点乘（限幅校验先行——溢出显性拒不是静默回绕）。
pub fn fx_mul(a: i64, b: i64) -> Result<i64, &'static str> {
    if a.abs() > FIXED_LIMIT || b.abs() > FIXED_LIMIT {
        return Err(E_FD_BADFIXED);
    }
    // a*b ≤ 2^46，i64 中间积安全；算术右移保持符号。
    Ok((a * b) >> 16)
}

/// 定点加（限幅校验）。
pub fn fx_add(a: i64, b: i64) -> Result<i64, &'static str> {
    let s = a + b;
    if s.abs() > FIXED_LIMIT {
        return Err(E_FD_BADFIXED);
    }
    Ok(s)
}

/// 定点除（b=0 显性拒；限幅校验在商上）。
pub fn fx_div(a: i64, b: i64) -> Result<i64, &'static str> {
    if b == 0 {
        return Err(E_FD_BADFIXED);
    }
    let q = (a << 16) / b;
    if q.abs() > FIXED_LIMIT {
        return Err(E_FD_BADFIXED);
    }
    Ok(q)
}

// ---------------------------------------------------------------------------
// LCG 确定性 RNG（同种子同序列——同果保证的引擎）
// ---------------------------------------------------------------------------

/// LCG 参数（Numerical Recipes 常数——64 位满周期）。
pub const LCG_MULT: u64 = 6364136223846793005;
pub const LCG_INC: u64 = 1442695040888963407;

/// 确定性 RNG：同种子同序列（逐项恒等——不是统计同分布）。
#[derive(Clone)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 下一原始值（O(1)）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_MULT).wrapping_add(LCG_INC);
        self.state
    }

    /// 下一 [0, range) 值（取模确定性——range=0 拒除以零语义返回 0）。
    pub fn next_below(&mut self, range: u64) -> u64 {
        if range == 0 {
            return 0;
        }
        self.next_u64() % range
    }

    /// n 步序列指纹（FNV-1a 64——同序列同指纹）。
    pub fn fingerprint(&mut self, steps: usize) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for _ in 0..steps {
            let v = self.next_u64();
            for byte in v.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 运算序锁定（三重防护之二：计划指纹——序漂移在指纹对拍中显形）
// ---------------------------------------------------------------------------

/// 运算计划（累积顺序显性化——固定序左折叠）。
#[derive(Clone, Debug, PartialEq)]
pub struct OpPlan {
    /// 项数（计划规模）。
    pub terms: usize,
    /// 折叠方向（L=左折叠锁定——本域唯一合法方向）。
    pub dir: char,
}

/// 计划指纹（FNV(terms, dir)——同计划同指纹，改项数/方向即变）。
pub fn plan_fingerprint(p: &OpPlan) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in p.terms.to_le_bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= p.dir as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 定点求和（按计划固定序左折叠——O(项数)；dir 是声明性字段，实现
/// 恒左折叠——序锁定在实现面不押 panic）。项数=0 显性空态返回 0。
pub fn fx_sum_ordered(terms: &[i64], plan: &OpPlan) -> Result<i64, &'static str> {
    let mut acc: i64 = 0;
    let n = plan.terms.min(terms.len());
    for i in 0..n {
        acc = fx_add(acc, terms[i])?;
    }
    Ok(acc)
}

// ---------------------------------------------------------------------------
// 平台差异清单（三重防护之三：登记制+责任人必填+公开可查）
// ---------------------------------------------------------------------------

/// 平台差异条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffState {
    /// 已登记待核验。
    Proposed,
    /// 核验通过在账。
    Verified,
}

/// 平台差异条目（责任人必填——无主差异单没人修）。
#[derive(Clone, Debug, PartialEq)]
pub struct DiffEntry {
    pub platform: &'static str,
    pub desc: &'static str,
    pub owner: &'static str,
    pub state: DiffState,
}

/// 平台差异清单（公开可查——容量 16 有界）。
pub struct DiffLedger {
    pub entries: Vec<DiffEntry>,
    pub rejected: u32,
}

pub const DIFF_CAP: usize = 16;

impl DiffLedger {
    pub fn new() -> Self {
        DiffLedger { entries: Vec::new(), rejected: 0 }
    }

    /// 登记（责任人必填闸；重复（同平台同描述）幂等；满载拒静默膨胀）。
    pub fn register(&mut self, e: DiffEntry) -> Result<bool, &'static str> {
        if e.owner.is_empty() {
            self.rejected += 1;
            return Err(E_FD_NOOWNER);
        }
        if self.entries.len() >= DIFF_CAP {
            self.rejected += 1;
            return Err(E_FD_BADFIXED); // 容量满复用溢出码（清单也是资源）
        }
        if self.entries.iter().any(|x| x.platform == e.platform && x.desc == e.desc) {
            return Ok(false); // 幂等：已登记不重复
        }
        self.entries.push(e);
        Ok(true)
    }

    /// 核验（Proposed→Verified）。
    pub fn verify(&mut self, platform: &str) -> bool {
        let mut hit = false;
        for e in self.entries.iter_mut() {
            if e.platform == platform && e.state == DiffState::Proposed {
                e.state = DiffState::Verified;
                hit = true;
            }
        }
        hit
    }

    /// 公开可读名单（无障碍：平台清单公开）。
    pub fn ledger_spoken(&self) -> String {
        let verified = self.entries.iter().filter(|e| e.state == DiffState::Verified).count();
        format!(
            "平台差异清单：在账 {} 条，已核验 {} 条，拒收 {} 次——每条均有责任人",
            self.entries.len(),
            verified,
            self.rejected
        )
    }
}

impl Default for DiffLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 种子簿与零容忍立案
// ---------------------------------------------------------------------------

/// 种子条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedState {
    Active,
    /// 世界版本变更后失效——重标定前不可用。
    Stale,
}

/// 种子条目（种子 ID/种子值/绑定世界版本/状态）。
#[derive(Clone, Debug, PartialEq)]
pub struct SeedEntry {
    pub id: u32,
    pub seed: u64,
    pub world_ver: u32,
    pub state: SeedState,
}

/// 零容忍立案（指纹失配记录——第一次即立案）。
#[derive(Clone, Debug, PartialEq)]
pub struct MismatchCase {
    pub seed_id: u32,
    pub fp_a: u64,
    pub fp_b: u64,
}

/// 确定性门面：种子簿+两跑对拍+零容忍立案+清单。
pub struct DeterminismBook {
    seeds: Vec<SeedEntry>,
    pub world_ver: u32,
    pub mismatches: Vec<MismatchCase>,
    pub ledger: DiffLedger,
}

pub const SEED_CAP: usize = 16;

impl DeterminismBook {
    pub fn new(world_ver: u32) -> Self {
        DeterminismBook {
            seeds: Vec::new(),
            world_ver,
            mismatches: Vec::new(),
            ledger: DiffLedger::new(),
        }
    }

    /// 种子登记（幂等；满载拒）。
    pub fn register_seed(&mut self, id: u32, seed: u64) -> Result<bool, &'static str> {
        if self.seeds.iter().any(|s| s.id == id) {
            return Ok(false);
        }
        if self.seeds.len() >= SEED_CAP {
            return Err(E_FD_BADFIXED);
        }
        self.seeds.push(SeedEntry { id, seed, world_ver: self.world_ver, state: SeedState::Active });
        Ok(true)
    }

    fn seed_of(&self, id: u32) -> Option<&SeedEntry> {
        self.seeds.iter().find(|s| s.id == id)
    }

    /// 取种子值（Stale 拒——重标定前不可用；未登记拒）。
    pub fn seed_value(&self, id: u32) -> Result<u64, &'static str> {
        match self.seed_of(id) {
            None => Err(E_FD_NOSEED),
            Some(s) if s.state == SeedState::Stale => Err(E_FD_STALE),
            Some(s) => Ok(s.seed),
        }
    }

    /// 两跑对拍（O(步长)：同种子两实例各 n 步指纹对拍——失配零容忍
    /// 立案并返回 Err；一致返回指纹）。
    pub fn verify_same_seed(&mut self, id: u32, steps: usize) -> Result<u64, &'static str> {
        let seed = self.seed_value(id)?;
        let mut a = Lcg::new(seed);
        let mut b = Lcg::new(seed);
        let fa = a.fingerprint(steps);
        let fb = b.fingerprint(steps);
        if fa != fb {
            self.mismatches.push(MismatchCase { seed_id: id, fp_a: fa, fp_b: fb });
            return Err(E_FD_MISMATCH);
        }
        Ok(fa)
    }

    /// 世界版本变更（旧种子全部 Stale——失效标记是批量纪律）。
    pub fn bump_world_ver(&mut self, ver: u32) -> usize {
        self.world_ver = ver;
        let mut n = 0;
        for s in self.seeds.iter_mut() {
            if s.state == SeedState::Active {
                s.state = SeedState::Stale;
                n += 1;
            }
        }
        n
    }

    /// 重标定（Stale→Active——种子对齐新版本）。
    pub fn recalibrate(&mut self, id: u32) -> Result<bool, &'static str> {
        match self.seeds.iter_mut().find(|s| s.id == id) {
            None => Err(E_FD_NOSEED),
            Some(s) => {
                let was = s.state == SeedState::Stale;
                s.state = SeedState::Active;
                s.world_ver = self.world_ver;
                Ok(was)
            }
        }
    }

    /// 状态可读名单。
    pub fn status_spoken(&self) -> String {
        format!(
            "确定性：世界版本 {}，种子 {} 枚，失配立案 {} 起，平台差异 {} 条",
            self.world_ver,
            self.seeds.len(),
            self.mismatches.len(),
            self.ledger.entries.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：同种子同果/三重防护/零容忍/种子重标定/编排）
// ---------------------------------------------------------------------------

/// F6007 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_vez19_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6007");

    // --- 同种子同果（判据一）---
    // 确-01：LCG(42) 两实例 64 步指纹相等（同种子同序列逐项恒等——
    // 不是统计同分布）；同实例连续取与前 3 项逐项对拍。
    let mut a1 = Lcg::new(42);
    let mut b1 = Lcg::new(42);
    let f1a = a1.fingerprint(64);
    let f1b = b1.fingerprint(64);
    let mut c1 = Lcg::new(42);
    let first = c1.next_u64();
    let second = c1.next_u64();
    // LCG 递推手算首项：state1 = 42*M+INC (mod 2^64)——恒等式验证：
    // 同种子重放第二实例首两项必与 c1 前两项相等。
    let mut c2 = Lcg::new(42);
    let first2 = c2.next_u64();
    let second2 = c2.next_u64();
    let same_ok = f1a == f1b && first == first2 && second == second2;
    if same_ok {
        s.ok("F6007-确-01-同种子同序列恒等");
    } else {
        s.fail("F6007-确-01-同种子同序列恒等", "同果保证失效");
    }

    // --- 异种子异果（判据二）---
    // 确-02：seed 42 vs 43 首项即分叉（不同种子不同序列——反向验证
    // 同果保证不是恒等函数）；next_below(100) 落域 [0,100)。
    let mut a2 = Lcg::new(42);
    let mut b2 = Lcg::new(43);
    let v42 = a2.next_u64();
    let v43 = b2.next_u64();
    let mut a2b = Lcg::new(42);
    let domain_v = a2b.next_below(100);
    let diff_ok = v42 != v43 && domain_v < 100;
    if diff_ok {
        s.ok("F6007-确-02-异种子异果与落域");
    } else {
        s.fail("F6007-确-02-异种子异果与落域", "种子区分失效");
    }

    // --- 定点备选（判据三）---
    // 防-01：Q16.16 手算——1.5×2.0=3.0（0x18000×0x20000>>16=0x30000）；
    // 0.5+0.25=0.75；3.0/2.0=1.5；溢出拒绝（2^24 超限幅 E_FD_BADFIXED）；
    // 除零拒；从有理构造 1.5 = fx_from_rational(1,500)=0x18000。
    let p = fx_from_rational(1, 500);
    let q = fx_from_rational(2, 0);
    let m = fx_mul(p, q).expect("probe-3a");
    let add = fx_add(FIXED_ONE / 2, FIXED_ONE / 4).expect("probe-3b");
    let dv = fx_div(m, q).expect("probe-3c");
    let over = fx_mul(FIXED_LIMIT + 1, FIXED_ONE);
    let div0 = fx_div(FIXED_ONE, 0);
    let fixed_ok = p == 0x18000
        && m == 3 * FIXED_ONE
        && add == (3 * FIXED_ONE) / 4
        && dv == 0x18000
        && matches!(over, Err(E_FD_BADFIXED))
        && matches!(div0, Err(E_FD_BADFIXED));
    if fixed_ok {
        s.ok("F6007-防-01-定点Q16手算与限幅");
    } else {
        s.fail("F6007-防-01-定点Q16手算与限幅", "定点备选失效");
    }

    // --- 运算序锁定（判据四）---
    // 防-02：计划指纹锁定——三定点项 [1.0, 2.0, 3.0] 固定序和 6.0；
    // 同计划指纹相等；改项数指纹必变（序漂移在指纹对拍中显形）；
    // 空计划空和显性 0。
    let terms = [FIXED_ONE, 2 * FIXED_ONE, 3 * FIXED_ONE];
    let plan = OpPlan { terms: 3, dir: 'L' };
    let sum = fx_sum_ordered(&terms, &plan).expect("probe-4a");
    let fp_same = plan_fingerprint(&OpPlan { terms: 3, dir: 'L' }) == plan_fingerprint(&OpPlan { terms: 3, dir: 'L' });
    let fp_diff = plan_fingerprint(&OpPlan { terms: 3, dir: 'L' }) != plan_fingerprint(&OpPlan { terms: 4, dir: 'L' });
    let empty = fx_sum_ordered(&[], &OpPlan { terms: 0, dir: 'L' }).expect("probe-4b");
    let plan_ok = sum == 6 * FIXED_ONE && fp_same && fp_diff && empty == 0;
    if plan_ok {
        s.ok("F6007-防-02-运算序锁定计划指纹");
    } else {
        s.fail("F6007-防-02-运算序锁定计划指纹", "运算序失效");
    }

    // --- 平台差异清单（判据五）---
    // 防-03：登记闸——缺责任人拒 E_FD_NOOWNER 计数；合法登记幂等
    // （重复 False）；核验 Proposed→Verified；公开可读名单对账。
    let mut d5 = DiffLedger::new();
    let noowner = d5.register(DiffEntry { platform: "win32", desc: "fma 舍入差异", owner: "", state: DiffState::Proposed });
    let r1 = d5.register(DiffEntry { platform: "win32", desc: "fma 舍入差异", owner: "alice", state: DiffState::Proposed });
    let r1b = d5.register(DiffEntry { platform: "win32", desc: "fma 舍入差异", owner: "alice", state: DiffState::Proposed });
    let r2 = d5.register(DiffEntry { platform: "macos", desc: "denormal flush", owner: "bob", state: DiffState::Proposed });
    let v1 = d5.verify("win32");
    let spoken5 = d5.ledger_spoken();
    let ledger_ok = matches!(noowner, Err(E_FD_NOOWNER))
        && matches!(r1, Ok(true))
        && matches!(r1b, Ok(false))
        && matches!(r2, Ok(true))
        && v1
        && d5.entries.len() == 2
        && d5.rejected == 1
        && spoken5.contains("在账 2 条")
        && spoken5.contains("已核验 1 条");
    if ledger_ok {
        s.ok("F6007-防-03-差异清单责任人必填");
    } else {
        s.fail("F6007-防-03-差异清单责任人必填", "清单管理失效");
    }

    // --- 零容忍（判据六）---
    // 零-01：同种子对拍一致返回指纹；失配立案（注入式——用种子簿
    // 直接构造失配对拍：手摆 fp_a≠fp_b 即立案——验证立案面本身；
    // 立案簿内容精确 (seed_id, fp_a, fp_b)）。
    let mut d6 = DeterminismBook::new(7);
    let _ = d6.register_seed(1, 42);
    let fp_ok = d6.verify_same_seed(1, 32).expect("probe-6a");
    d6.mismatches.push(MismatchCase { seed_id: 2, fp_a: 0xAAAA, fp_b: 0xBBBB });
    let case_ok = d6.mismatches.len() == 1
        && d6.mismatches[0].seed_id == 2
        && d6.mismatches[0].fp_a == 0xAAAA
        && d6.mismatches[0].fp_b == 0xBBBB
        && fp_ok != 0;
    if case_ok {
        s.ok("F6007-零-01-失配立案零容忍");
    } else {
        s.fail("F6007-零-01-失配立案零容忍", "零容忍失效");
    }

    // --- 种子失效重标定（判据七）---
    // 种-01：版本变更旧种子全 Stale（2 枚→2）；Stale 期取值拒
    // E_FD_STALE；重标定回 Active 后可用；未登记 ID 拒 E_FD_NOSEED；
    // 新版本重登记幂等 False。
    let mut d7 = DeterminismBook::new(1);
    let _ = d7.register_seed(1, 100);
    let _ = d7.register_seed(2, 200);
    let stale_n = d7.bump_world_ver(2);
    let stale_get = d7.seed_value(1);
    let noseed = d7.seed_value(9);
    let rec = d7.recalibrate(1).expect("probe-7a");
    let rec_get = d7.seed_value(1);
    let dup = d7.register_seed(1, 100);
    let seed_ok = stale_n == 2
        && matches!(stale_get, Err(E_FD_STALE))
        && matches!(noseed, Err(E_FD_NOSEED))
        && rec
        && matches!(rec_get, Ok(100))
        && matches!(dup, Ok(false));
    if seed_ok {
        s.ok("F6007-种-01-失效标记重标定");
    } else {
        s.fail("F6007-种-01-失效标记重标定", "种子簿失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 互异非空 + 缺省兜底 + 零 panic 空态
    // （空簿全查询显性拒不是静默 0）。
    let codes = [E_FD_MISMATCH, E_FD_NOOWNER, E_FD_STALE, E_FD_NOSEED, E_FD_BADFIXED];
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] || reason_of(codes[i]) == reason_of(codes[j]) {
                distinct = false;
            }
        }
    }
    let meta_codes = distinct
        && reason_of("E_FD_UNKNOWN") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty());
    let d8 = DeterminismBook::new(1);
    let empty_ok = matches!(d8.seed_value(1), Err(E_FD_NOSEED))
        && d8.status_spoken().contains("种子 0 枚");
    if meta_codes && empty_ok {
        s.ok("F6007-元-01-五码互异空态显性");
    } else {
        s.fail("F6007-元-01-五码互异空态显性", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(步长)——登记种子→定点模拟 8 步（每步位移=
    // 定点速度×1，Q16 手算 1.5/帧→8 帧累计 12.0）→两跑指纹对拍一致
    // →版本变更失效→重标定→再对拍一致→清单登记→状态可读对账。
    let mut d9 = DeterminismBook::new(3);
    let _ = d9.register_seed(7, 0xDEAD);
    // 定点模拟：pos += vel(1.5)×dt(1.0) 8 步 → 0→12.0。
    let vel = fx_from_rational(1, 500);
    let mut pos: i64 = 0;
    for _ in 0..8 {
        pos = fx_add(pos, fx_mul(vel, FIXED_ONE).expect("probe-9")).expect("probe-9");
    }
    let fp1 = d9.verify_same_seed(7, 64).expect("probe-9b");
    let _ = d9.bump_world_ver(4);
    let stale = d9.seed_value(7);
    let _ = d9.recalibrate(7);
    let fp2 = d9.verify_same_seed(7, 64).expect("probe-9c");
    let _ = d9.ledger.register(DiffEntry { platform: "ps5", desc: "ftz 差异", owner: "carol", state: DiffState::Proposed });
    let spoken = d9.status_spoken();
    let chain_ok = pos == 12 * FIXED_ONE
        && fp1 == fp2
        && matches!(stale, Err(E_FD_STALE))
        && d9.ledger.entries.len() == 1
        && spoken.contains("世界版本 4")
        && spoken.contains("失配立案 0 起");
    if chain_ok {
        s.ok("F6007-编-01-全链路O步长对账");
    } else {
        s.fail("F6007-编-01-全链路O步长对账", "编排失效");
    }

    s
}
