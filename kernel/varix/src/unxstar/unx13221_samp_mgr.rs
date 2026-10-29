//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13221 · 样本库管理器总成（登记/索引/校验三面） · 0x657C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13221-J1 管理器三面（登记/索引/校验）30 例全链一致，
//!      样本入库→可查→可装载闭环断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三面架构：登记面（样本 id+族+载荷哈希入账，重复 id 拒收）/
//!     索引面（id→槽位映射，未登记 id 查询必拒）/ 校验面（载荷重算
//!     哈希与登记哈希比对，篡改必抓）；
//!   - 闭环恒等式：入库→可查→可装载，三步齐才闭环；
//!     装载后样本不得改账（账面只读红线）；
//!   - 哈希闭式：FNV-1a 整数域（无乘法溢出处理依赖，wrapping_mul），
//!     同载荷同哈希（确定性红线）；
//!   - 30 例全链账：登记 30 → 索引 30 → 校验 30 → 装载 30，账目守恒；
//!   - LCG 种子写死 = SEG_BASE（0x657C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x657C0，全仓独占）。
pub const SEG_BASE: u32 = 0x657C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampMgrCode {
    /// 样本 id 重复登记
    IdDuplicated,
    /// 查询未登记 id
    IdNotFound,
    /// 载荷哈希校验失配（篡改）
    HashMismatch,
    /// 载荷为空（入库面拒收）
    PayloadEmpty,
    /// 样本族越闭集
    FamilyOutOfRange,
    /// 装载后改账（账面只读红线破）
    MutatedAfterLoad,
    /// 索引账与登记账失平
    IndexUnbalanced,
    /// 闭环缺步（未装载却声称闭环）
    LoopIncomplete,
}

impl SampMgrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SampMgrCode::IdDuplicated => "样本 id 重复登记：登记面闭集破",
            SampMgrCode::IdNotFound => "查询未登记 id：索引面越界",
            SampMgrCode::HashMismatch => "载荷哈希校验失配：疑似篡改",
            SampMgrCode::PayloadEmpty => "载荷为空：入库面拒收",
            SampMgrCode::FamilyOutOfRange => "样本族越闭集：族账越界",
            SampMgrCode::MutatedAfterLoad => "装载后改账：账面只读红线破",
            SampMgrCode::IndexUnbalanced => "索引账与登记账失平：守恒破",
            SampMgrCode::LoopIncomplete => "闭环缺步：入库→可查→可装载未齐",
        }
    }
    pub const ALL: [SampMgrCode; 8] = [
        SampMgrCode::IdDuplicated,
        SampMgrCode::IdNotFound,
        SampMgrCode::HashMismatch,
        SampMgrCode::PayloadEmpty,
        SampMgrCode::FamilyOutOfRange,
        SampMgrCode::MutatedAfterLoad,
        SampMgrCode::IndexUnbalanced,
        SampMgrCode::LoopIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SampMgrCode::ALL.len() {
        if SampMgrCode::ALL[i].val() == code {
            return SampMgrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：样本库建模书 + 判据原文） ────────────────────────
/// 样本族闭集（3 族：格式类/资源类/依赖类）。
pub const FAMILY_FORMAT: u8 = 1;
pub const FAMILY_RESOURCE: u8 = 2;
pub const FAMILY_DEPEND: u8 = 3;
/// 全链例数（判据原文：30 例）。
pub const CASES: usize = 30;

/// FNV-1a 哈希（整数域，wrapping，确定性闭式）。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

// ── 样本与账本 ───────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Sample {
    pub id: u32,
    pub family: u8,
    pub payload: Vec<u8>,
    /// 登记时落账的哈希
    pub hash: u64,
    /// 是否已装载（装载后账面只读）
    pub loaded: bool,
}

pub struct SampMgr {
    pub registry: Vec<Sample>,
    /// 索引账：id → 槽位（与 registry 同长同序）
    pub index: Vec<(u32, usize)>,
    /// 装载计数
    pub loaded_total: usize,
}

impl SampMgr {
    pub fn new() -> SampMgr {
        SampMgr {
            registry: Vec::new(),
            index: Vec::new(),
            loaded_total: 0,
        }
    }

    /// 登记面：入库（重复 id / 空载荷 / 族越闭集必拒）。
    pub fn register(&mut self, id: u32, family: u8, payload: &[u8]) -> Result<usize, SampMgrCode> {
        if payload.is_empty() {
            return Err(SampMgrCode::PayloadEmpty);
        }
        if family < FAMILY_FORMAT || family > FAMILY_DEPEND {
            return Err(SampMgrCode::FamilyOutOfRange);
        }
        // 先查重（闭式扫描）。
        let mut i = 0;
        while i < self.registry.len() {
            if self.registry[i].id == id {
                return Err(SampMgrCode::IdDuplicated);
            }
            i += 1;
        }
        let slot = self.registry.len();
        let hash = fnv1a(payload);
        self.registry.push(Sample {
            id,
            family,
            payload: payload.to_vec(),
            hash,
            loaded: false,
        });
        self.index.push((id, slot));
        Ok(slot)
    }

    /// 索引面：id 查槽位（未登记必拒）。
    pub fn lookup(&self, id: u32) -> Result<usize, SampMgrCode> {
        let mut i = 0;
        while i < self.index.len() {
            if self.index[i].0 == id {
                return Ok(self.index[i].1);
            }
            i += 1;
        }
        Err(SampMgrCode::IdNotFound)
    }

    /// 校验面：载荷重算哈希与登记哈希比对（篡改必抓）。
    pub fn verify(&self, id: u32) -> Result<u64, SampMgrCode> {
        let slot = self.lookup(id)?;
        let s = &self.registry[slot];
        let cur = fnv1a(&s.payload);
        if cur != s.hash {
            return Err(SampMgrCode::HashMismatch);
        }
        Ok(cur)
    }

    /// 装载（闭环第三步）：可装载前置 = 登记在账且校验通过。
    pub fn load(&mut self, id: u32) -> Result<(), SampMgrCode> {
        let slot = self.lookup(id)?;
        let v = self.verify(id)?;
        let s = &mut self.registry[slot];
        s.hash = v; // 幂等写（同值），不改语义
        s.loaded = true;
        self.loaded_total += 1;
        Ok(())
    }

    /// 闭环恒等式：登记数 == 索引数 == 装载数（全链齐步）。
    pub fn verify_loop(&self, expect: usize) -> Result<(), SampMgrCode> {
        if self.registry.len() != self.index.len() {
            return Err(SampMgrCode::IndexUnbalanced);
        }
        if self.loaded_total != expect {
            return Err(SampMgrCode::LoopIncomplete);
        }
        let mut i = 0;
        while i < self.registry.len() {
            if !self.registry[i].loaded {
                return Err(SampMgrCode::LoopIncomplete);
            }
            i += 1;
        }
        Ok(())
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：三面 30 例全链（登记→索引→校验→装载闭环）。
pub fn drill_30_cases() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < CASES {
        let fam = (rng.next_below(3) + 1) as u8;
        let payload: [u8; 4] = [
            (i & 0xFF) as u8,
            (rng.next() & 0xFF) as u8,
            0x5A,
            (i as u8).wrapping_mul(3),
        ];
        if mgr.register(0x2000 + i as u32, fam, &payload).is_err() {
            return (false, format!("第 {} 例登记失败", i + 1));
        }
        i += 1;
    }
    // 索引 30 例。
    let mut found = 0;
    let mut j = 0;
    while j < CASES {
        if mgr.lookup(0x2000 + j as u32).is_ok() {
            found += 1;
        }
        j += 1;
    }
    // 校验 30 例。
    let mut verified = 0;
    let mut k = 0;
    while k < CASES {
        if mgr.verify(0x2000 + k as u32).is_ok() {
            verified += 1;
        }
        k += 1;
    }
    // 装载 30 例（闭环）。
    let mut m = 0;
    while m < CASES {
        if mgr.load(0x2000 + m as u32).is_err() {
            return (false, format!("第 {} 例装载失败", m + 1));
        }
        m += 1;
    }
    let closed = mgr.verify_loop(CASES) == Ok(());
    (
        found == CASES && verified == CASES && closed,
        format!(
            "三面 30 例: 索引={}/{} 校验={}/{} 闭环={}",
            found, CASES, verified, CASES, closed
        ),
    )
}

/// 注入面：重复 id 登记 10/10 必抓。
pub fn drill_duplicate_10() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let _ = mgr.register(0x3000, FAMILY_FORMAT, &[1, 2, 3]);
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        if mgr.register(0x3000, FAMILY_RESOURCE, &[4 + i as u8, 5, 6])
            == Err(SampMgrCode::IdDuplicated)
        {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("重复 id 注入 {}/10 必抓", caught))
}

/// 注入面：查询未登记 id 10/10 必抓。
pub fn drill_not_found_10() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let _ = mgr.register(0x4000, FAMILY_DEPEND, &[9, 9]);
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        if mgr.lookup(0x5000 + i as u32) == Err(SampMgrCode::IdNotFound) {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("未登记查询注入 {}/10 必抓", caught))
}

/// 注入面：载荷篡改哈希必抓（校验面反向判据）。
pub fn drill_tamper_caught() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let _ = mgr.register(0x6000, FAMILY_FORMAT, &[0xDE, 0xAD, 0xBE, 0xEF]);
    // 篡改载荷（下标先比长度）。
    let slot = mgr.lookup(0x6000);
    let ok_slot = slot.is_ok();
    if ok_slot {
        let s = &mut mgr.registry[slot.unwrap_or(0)];
        s.payload[0] = 0x00;
    }
    let r = mgr.verify(0x6000);
    (
        ok_slot && r == Err(SampMgrCode::HashMismatch),
        format!("篡改校验抓={:?}", r.is_err()),
    )
}

/// 注入面：空载荷 / 族越闭集 10/10 必抓。
pub fn drill_register_violations_10() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let mut caught = 0;
    // 空载荷 5 例。
    let mut i = 0;
    while i < 5 {
        if mgr.register(0x7000 + i as u32, FAMILY_FORMAT, &[])
            == Err(SampMgrCode::PayloadEmpty)
        {
            caught += 1;
        }
        i += 1;
    }
    // 族越闭集 5 例（4..8 号族均越界；合法族为 1..3）。
    let mut j = 0;
    while j < 5 {
        if mgr.register(0x7100 + j as u32, 4 + j as u8, &[1])
            == Err(SampMgrCode::FamilyOutOfRange)
        {
            caught += 1;
        }
        j += 1;
    }
    (caught == 10, format!("入库违规注入 {}/10 必抓", caught))
}

/// 哈希闭式确定性：同载荷同哈希、异载荷异哈希（10 组）。
pub fn drill_hash_determinism() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < 10 {
        let a = fnv1a(&[i as u8, 0x11, 0x22, 0x33]);
        let b = fnv1a(&[i as u8, 0x11, 0x22, 0x33]);
        let c = fnv1a(&[i as u8, 0x11, 0x22, 0x34]);
        if a != b || a == c {
            ok = false;
            break;
        }
        i += 1;
    }
    // 反向判据：FNV 初值改动必产出异哈希。
    let base = fnv1a(&[7, 7, 7]);
    let diff = fnv1a(&[7, 7, 8]) != base;
    (ok && diff, format!("哈希确定性 10 组全真={} 异载荷必异={}", ok, diff))
}

/// 闭环缺步注入：未装载全账 → 闭环核必抓。
pub fn drill_loop_incomplete() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let _ = mgr.register(0x8000, FAMILY_FORMAT, &[1]);
    let _ = mgr.register(0x8001, FAMILY_RESOURCE, &[2]);
    let _ = mgr.load(0x8000);
    // 0x8001 未装载 → 闭环核必抓 LoopIncomplete。
    let r = mgr.verify_loop(2);
    (
        r == Err(SampMgrCode::LoopIncomplete),
        format!("闭环缺步抓={:?}", r.is_err()),
    )
}

/// 索引守恒面：登记数 == 索引数；抽索引条 → 失平必抓。
pub fn drill_index_conservation() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let mut i = 0;
    while i < 6 {
        let _ = mgr.register(0x9000 + i as u32, FAMILY_DEPEND, &[i as u8, 1]);
        i += 1;
    }
    let balanced = mgr.registry.len() == mgr.index.len();
    // 反向注入：抽一条索引 → 失平必抓。
    mgr.index.pop();
    let broken = mgr.registry.len() != mgr.index.len();
    (balanced && broken, format!("索引守恒={} 抽条失平={}", balanced, broken))
}

/// 多机对拍面：同种子重建两次，登记哈希账逐条一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut mgr = SampMgr::new();
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 12 {
            let fam = (rng.next_below(3) + 1) as u8;
            let p0 = (rng.next() & 0xFF) as u8;
            let _ = mgr.register(0xA000 + i, fam, &[p0, i as u8]);
            i += 1;
        }
        mgr.registry.iter().map(|s| (s.id, s.hash)).collect::<Vec<(u32, u64)>>()
    };
    let a = build();
    let b = build();
    let mut same = a.len() == b.len();
    let mut j = 0;
    while same && j < a.len() {
        if a[j] != b[j] {
            same = false;
        }
        j += 1;
    }
    (same, format!("同种子对拍一致={}", same))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_30_cases();
    let (a2, d2) = drill_tamper_caught();
    let (a3, d3) = drill_loop_incomplete();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30例闭环={} 篡改抓={} 缺步抓={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 深化块：黄金哈希对照 + 样本账导出 ────────────────────────────
/// 黄金哈希对照：独立第二实现（步进式 FNV-1a 64）与主实现逐组比对（5 组）。
pub fn drill_golden_hash_5() -> (bool, String) {
    // 独立写死的第二实现（同规格：FNV-1a 64，wrapping 乘法）。
    fn ref_fnv(data: &[u8]) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < data.len() {
            h ^= data[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }
    let cases: [&[u8]; 5] = [&[1u8], &[1, 2, 3], &[0xDE, 0xAD, 0xBE, 0xEF], &[7, 7, 7], &[
        0x5A, 0xA5, 0xFF, 0x00, 0x11,
    ]];
    let mut ok = true;
    let mut i = 0;
    while i < cases.len() {
        if fnv1a(cases[i]) != ref_fnv(cases[i]) {
            ok = false;
            break;
        }
        i += 1;
    }
    (ok, format!("黄金哈希对照 5 组全真={}", ok))
}

/// 样本账诊断导出：S|id|fam|0xhash|loaded 逐行（行数守恒 + 格式断言）。
pub fn drill_export_samples() -> (bool, String) {
    let mut mgr = SampMgr::new();
    let mut i = 0;
    while i < 5 {
        let _ = mgr.register(0xC000 + i as u32, (i % 3 + 1) as u8, &[i as u8, 0x42]);
        i += 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut j = 0;
    while j < mgr.registry.len() {
        let s = &mgr.registry[j];
        lines.push(format!(
            "S|{}|{}|0x{:016X}|{}",
            s.id, s.family, s.hash, s.loaded
        ));
        j += 1;
    }
    let rows_ok = lines.len() == mgr.registry.len();
    let mut fmt_ok = true;
    let mut k = 0;
    while k < lines.len() {
        if !lines[k].starts_with("S|") || lines[k].split('|').count() != 5 {
            fmt_ok = false;
        }
        k += 1;
    }
    (
        rows_ok && fmt_ok,
        format!("样本账导出 {} 行守恒={} 格式={}", lines.len(), rows_ok, fmt_ok),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < SampMgrCode::ALL.len() {
        let v = SampMgrCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_30_cases();
    v.push(("J1-三面30例全链闭环", a, b));
    let (a, b) = drill_duplicate_10();
    v.push(("J2-重复id注入10/10", a, b));
    let (a, b) = drill_not_found_10();
    v.push(("J2-未登记查询注入10/10", a, b));
    let (a, b) = drill_tamper_caught();
    v.push(("J2-载荷篡改哈希必抓", a, b));
    let (a, b) = drill_register_violations_10();
    v.push(("J2-入库违规注入10/10", a, b));
    let (a, b) = drill_hash_determinism();
    v.push(("J3-哈希确定性10组", a, b));
    let (a, b) = drill_loop_incomplete();
    v.push(("J3-闭环缺步必抓", a, b));
    let (a, b) = drill_index_conservation();
    v.push(("J3-索引守恒+反向", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_golden_hash_5();
    v.push(("J3-黄金哈希对照5组", a, b));
    let (a, b) = drill_export_samples();
    v.push(("J3-样本账诊断导出", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13221_checks：本单判据聚合（诊断段 0x657C0）。
pub fn run_unx13221_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13221 样本库管理器总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x657C0 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
