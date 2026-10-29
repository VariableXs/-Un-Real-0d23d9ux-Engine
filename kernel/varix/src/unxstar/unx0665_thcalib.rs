//! UNX-F0665 · 阈值试产校准规程（校准→冻结→入基准表）· 诊断段 0x5460（本单独占）
//!
//! 目标行数：≥580（纯功能代码）
//! 判据原文摘录：UNX-F0665-J1 校准规程三步（采样/统计/冻结）留痕齐，冻结后阈值表哈希
//! 封存且变更走 ADR。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 规程状态机三步闭集：采样 → 统计 → 冻结，跳步/回退/重做均拦截。
//!  3. 冻结即哈希封存：FNV-1a64 遍历阈值表，封后任何改写必须持 ADR 单号。
//!  4. 三步留痕：每步落（步号/动作码/输入指纹/输出指纹）账行，零缺步。
//!  5. 统计步以整数分位法（选择算法，无浮点）产出 p50/p95/max 三档阈值。
//!  6. 诊断段 0x5460 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5460;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_PHASE: u16 = SEG_BASE + 1;   // 跳步/回退（状态机非法边）
pub const E_SAMPLE: u16 = SEG_BASE + 2;  // 样本量不足/含零耗时
pub const E_STAT: u16 = SEG_BASE + 3;    // 统计产出非法（阈值非升序）
pub const E_SEALED: u16 = SEG_BASE + 4;  // 已冻结再改写（无 ADR）
pub const E_HASH: u16 = SEG_BASE + 5;    // 封存哈希复核不平
pub const E_ADR: u16 = SEG_BASE + 6;     // ADR 单号非法（空/重用）
pub const E_TRACE: u16 = SEG_BASE + 7;   // 留痕账缺步

/// 规程步闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 第一步：采样（冷启动耗时样本采集）。
    Sampling,
    /// 第二步：统计（整数分位阈值产出）。
    Statistics,
    /// 第三步：冻结（哈希封存入基准表）。
    Freeze,
}

impl Phase {
    pub fn no(self) -> u8 {
        match self {
            Phase::Sampling => 1,
            Phase::Statistics => 2,
            Phase::Freeze => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Phase::Sampling => "采样",
            Phase::Statistics => "统计",
            Phase::Freeze => "冻结",
        }
    }
}

/// 留痕账行：四要素。
#[derive(Clone, Debug)]
pub struct TraceRow {
    pub step: u8,
    /// 动作码：1=开始 2=完成。
    pub action: u8,
    pub in_fp: u64,
    pub out_fp: u64,
}

/// ADR 变更单：封存后的唯一合法改写凭据。
#[derive(Clone, Debug)]
pub struct AdrTicket {
    /// 单号（非零，簿内唯一）。
    pub id: u32,
    /// 变更对象阈值档（0=p50, 1=p95, 2=max）。
    pub target: u8,
    /// 新阈值（ms）。
    pub new_ms: u64,
}

/// LCG64。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 试产采样样本量（判据口径 80 次）。
pub const PILOT_N: usize = 80;

/// 阈值表：三档（p50/p95/max），冻结后哈希封存。
pub struct ThresholdTable {
    /// [p50, p95, max] ms。
    pub tiers: [u64; 3],
    /// 封存哈希（冻结时现算）。
    pub seal_hash: u64,
    pub frozen: bool,
}

impl ThresholdTable {
    pub fn open() -> ThresholdTable {
        ThresholdTable { tiers: [0, 0, 0], seal_hash: 0, frozen: false }
    }

    /// 表指纹（字节序大端逐档拼装后 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = vec![0u8; 24];
        for (i, v) in self.tiers.iter().enumerate() {
            buf[i * 8] = (v >> 56) as u8;
            buf[i * 8 + 1] = (v >> 48) as u8;
            buf[i * 8 + 2] = (v >> 40) as u8;
            buf[i * 8 + 3] = (v >> 32) as u8;
            buf[i * 8 + 4] = (v >> 24) as u8;
            buf[i * 8 + 5] = (v >> 16) as u8;
            buf[i * 8 + 6] = (v >> 8) as u8;
            buf[i * 8 + 7] = *v as u8;
        }
        fnv1a64(&buf)
    }

    /// 冻结：非降序校验 + 哈希封存。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.tiers[0] == 0 || self.tiers[0] > self.tiers[1] || self.tiers[1] > self.tiers[2] {
            return Err(E_STAT);
        }
        self.seal_hash = self.fingerprint();
        self.frozen = true;
        Ok(())
    }

    /// 封存哈希复核。
    pub fn verify_seal(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_SEALED);
        }
        if self.fingerprint() != self.seal_hash {
            return Err(E_HASH);
        }
        Ok(())
    }

    /// 持 ADR 改写：ADR 合法才可改，改后自动重冻结出新哈希。
    pub fn amend_with_adr(&mut self, adr: &AdrTicket, used_ids: &mut Vec<u32>) -> Result<(), u16> {
        if adr.id == 0 {
            return Err(E_ADR);
        }
        if used_ids.contains(&adr.id) {
            return Err(E_ADR);
        }
        if adr.target > 2 {
            return Err(E_ADR);
        }
        if adr.new_ms == 0 {
            return Err(E_ADR);
        }
        let old = self.tiers[adr.target as usize];
        self.tiers[adr.target as usize] = adr.new_ms;
        // 非降序仍须成立
        if self.tiers[0] > self.tiers[1] || self.tiers[1] > self.tiers[2] {
            self.tiers[adr.target as usize] = old;
            return Err(E_STAT);
        }
        used_ids.push(adr.id);
        if self.frozen {
            self.seal_hash = self.fingerprint();
        }
        Ok(())
    }

    /// 无 ADR 改写守卫：一律拦截。
    pub fn try_write_no_adr(&self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEALED);
        }
        Ok(())
    }
}

/// 整数分位：选择法（复制样本后排序取下标），k ∈ [0, n-1]。
pub fn pick_order(samples: &[u64], k: usize) -> Result<u64, u16> {
    if samples.is_empty() {
        return Err(E_SAMPLE);
    }
    if k >= samples.len() {
        return Err(E_STAT);
    }
    let mut v = samples.to_vec();
    // 简单选择：逐轮取最小（n=80，确定性，无浮点）
    for i in 0..=k {
        let mut mi = i;
        for j in (i + 1)..v.len() {
            if v[j] < v[mi] {
                mi = j;
            }
        }
        let t = v[i];
        v[i] = v[mi];
        v[mi] = t;
    }
    Ok(v[k])
}

/// 三档阈值产出：p50 = 序 40（0 起），p95 = 序 75，max = 序 79（n=80）。
pub fn derive_tiers(samples: &[u64]) -> Result<[u64; 3], u16> {
    if samples.len() != PILOT_N {
        return Err(E_SAMPLE);
    }
    for s in samples {
        if *s == 0 {
            return Err(E_SAMPLE);
        }
    }
    let p50 = pick_order(samples, (PILOT_N * 50 / 100) - 1)?;
    let p95 = pick_order(samples, (PILOT_N * 95 / 100) - 1)?;
    let mut max: u64 = 0;
    for s in samples {
        if *s > max {
            max = *s;
        }
    }
    Ok([p50, p95, max])
}

/// 校准规程状态机。
pub struct CalibProcedure {
    pub phase: Phase,
    /// 采样样本。
    pub samples: Vec<u64>,
    pub table: ThresholdTable,
    /// 留痕账。
    pub traces: Vec<TraceRow>,
    /// 已用 ADR 单号。
    pub used_adr: Vec<u32>,
    /// 完成步计数。
    pub steps_done: u8,
}

impl CalibProcedure {
    pub fn new() -> CalibProcedure {
        CalibProcedure {
            phase: Phase::Sampling,
            samples: Vec::new(),
            table: ThresholdTable::open(),
            traces: Vec::new(),
            used_adr: Vec::new(),
            steps_done: 0,
        }
    }

    fn trace(&mut self, action: u8, in_fp: u64, out_fp: u64) {
        self.traces.push(TraceRow {
            step: self.phase.no(),
            action,
            in_fp,
            out_fp,
        });
    }

    /// 第一步：采样。样本逐个入账，量不足拦截。
    pub fn do_sampling(&mut self, seed: u64, n: usize) -> Result<(), u16> {
        if self.phase != Phase::Sampling {
            return Err(E_PHASE);
        }
        if n < PILOT_N {
            return Err(E_SAMPLE);
        }
        self.trace(1, 0, seed);
        let mut rng = Lcg::new(seed);
        let mut base: u64 = 2_200;
        self.samples.clear();
        for _ in 0..n {
            let j = (rng.next_u32() % 400) as u64;
            let ms = base + j;
            if ms == 0 {
                return Err(E_SAMPLE);
            }
            self.samples.push(ms);
            base = base.wrapping_add(2) & 0xFFF; // 轻微漂移带
        }
        let fp = fnv1a64(&samples_fp_bytes(&self.samples));
        self.trace(2, seed, fp);
        self.phase = Phase::Statistics;
        self.steps_done += 1;
        Ok(())
    }

    /// 第二步：统计。三档阈值产出 + 留痕。
    pub fn do_statistics(&mut self) -> Result<(), u16> {
        if self.phase != Phase::Statistics {
            return Err(E_PHASE);
        }
        let in_fp = fnv1a64(&samples_fp_bytes(&self.samples));
        self.trace(1, in_fp, 0);
        let tiers = derive_tiers(&self.samples)?;
        self.table.tiers = tiers;
        let out_fp = self.table.fingerprint();
        self.trace(2, in_fp, out_fp);
        self.phase = Phase::Freeze;
        self.steps_done += 1;
        Ok(())
    }

    /// 第三步：冻结。哈希封存 + 留痕。
    pub fn do_freeze(&mut self) -> Result<(), u16> {
        if self.phase != Phase::Freeze {
            return Err(E_PHASE);
        }
        self.trace(1, self.table.fingerprint(), 0);
        self.table.freeze()?;
        self.table.verify_seal()?;
        self.trace(2, self.table.seal_hash, self.table.seal_hash);
        self.steps_done += 1;
        Ok(())
    }

    /// 留痕审计：三步 × 两动作 = 6 行，步号 1..3 依序。
    pub fn assert_trace_full(&self) -> Result<(), u16> {
        if self.traces.len() != 6 {
            return Err(E_TRACE);
        }
        let mut want_step = 1u8;
        let mut want_action = 1u8;
        for t in &self.traces {
            if t.step != want_step || t.action != want_action {
                return Err(E_TRACE);
            }
            if want_action == 1 {
                want_action = 2;
            } else {
                want_action = 1;
                want_step += 1;
            }
        }
        if self.steps_done != 3 {
            return Err(E_TRACE);
        }
        Ok(())
    }

    /// 跳步注入核：未采样先统计必须拦截。
    pub fn inject_skip_must_fail(&self) -> Result<(), u16> {
        let mut p = CalibProcedure::new();
        p.phase = Phase::Statistics;
        if p.do_statistics().is_ok() {
            return Err(SEG_BASE + 0x30);
        }
        Ok(())
    }
}

/// 样本指纹字节流。
pub fn samples_fp_bytes(samples: &[u64]) -> Vec<u8> {
    let mut buf = vec![0u8; samples.len() * 8];
    for (i, v) in samples.iter().enumerate() {
        buf[i * 8] = (*v >> 56) as u8;
        buf[i * 8 + 1] = (*v >> 48) as u8;
        buf[i * 8 + 2] = (*v >> 40) as u8;
        buf[i * 8 + 3] = (*v >> 32) as u8;
        buf[i * 8 + 4] = (*v >> 24) as u8;
        buf[i * 8 + 5] = (*v >> 16) as u8;
        buf[i * 8 + 6] = (*v >> 8) as u8;
        buf[i * 8 + 7] = *v as u8;
    }
    buf
}

/// 基准表登记簿：冻结后的阈值表按机型入基准表，表哈希登记在案。
pub struct BaselineRegistry {
    /// (机型序号, 表哈希, 入表时戳序号)。
    pub entries: Vec<(usize, u64, u64)>,
    pub seq: u64,
    pub sealed: bool,
}

impl BaselineRegistry {
    pub fn new() -> BaselineRegistry {
        BaselineRegistry { entries: Vec::new(), seq: 0, sealed: false }
    }

    /// 入基准表：同机型重入拦截，表必须已冻结。
    pub fn enroll(&mut self, mach_idx: usize, table: &ThresholdTable) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if mach_idx > 2 {
            return Err(E_PHASE);
        }
        if !table.frozen {
            return Err(E_STAT);
        }
        table.verify_seal()?;
        for (m, _, _) in &self.entries {
            if *m == mach_idx {
                return Err(E_PHASE);
            }
        }
        self.seq += 1;
        self.entries.push((mach_idx, table.seal_hash, self.seq));
        Ok(())
    }

    /// 登记审计：三机齐 + 哈希非零 + 时戳序号单调。
    pub fn audit(&self) -> Result<(), u16> {
        if self.entries.len() != 3 {
            return Err(E_TRACE);
        }
        let mut prev: u64 = 0;
        for (m, h, s) in &self.entries {
            if *h == 0 || *s <= prev {
                return Err(E_TRACE);
            }
            prev = *s;
            let _ = m;
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.audit()?;
        self.sealed = true;
        Ok(())
    }
}

/// ADR 变更史账：每次持单改写留痕（单号/目标档/旧值/新值），史账只读。
pub struct AdrHistory {
    pub rows: Vec<(u32, u8, u64, u64)>,
}

impl AdrHistory {
    pub fn new() -> AdrHistory {
        AdrHistory { rows: Vec::new() }
    }

    pub fn record(&mut self, adr: &AdrTicket, old_ms: u64) -> Result<(), u16> {
        if adr.id == 0 {
            return Err(E_ADR);
        }
        for (id, _, _, _) in &self.rows {
            if id == &adr.id {
                return Err(E_ADR);
            }
        }
        self.rows.push((adr.id, adr.target, old_ms, adr.new_ms));
        Ok(())
    }

    /// 史账复核：行内新值 > 旧值或 < 旧值皆可，但新旧值非零、目标档 ≤2。
    pub fn audit(&self) -> Result<(), u16> {
        for (id, t, old, new) in &self.rows {
            if *id == 0 || *t > 2 || *old == 0 || *new == 0 {
                return Err(E_ADR);
            }
            if old == new {
                return Err(E_ADR);
            }
        }
        Ok(())
    }
}

/// 规程报告（程序化生成，逐行派生）。
pub struct CalibReport {
    pub lines: Vec<String>,
}

impl CalibReport {
    pub fn generate(p: &CalibProcedure) -> Result<CalibReport, u16> {
        p.assert_trace_full()?;
        p.table.verify_seal()?;
        let mut lines = Vec::new();
        // 行 1：规程结论
        lines.push(String::from("校准规程三步完成，阈值表已冻结封存"));
        // 行 2：三步留痕计数
        let mut l = String::from("留痕行数=6 步数=");
        l.push_str(&p.steps_done.to_string());
        lines.push(l);
        // 行 3..5：三档阈值
        let names = ["p50", "p95", "max"];
        for i in 0..3 {
            let mut l = String::from(names[i]);
            l.push('=');
            l.push_str(&p.table.tiers[i].to_string());
            l.push_str("ms");
            lines.push(l);
        }
        // 行 6：封存哈希尾
        let mut l = String::from("封存哈希尾=");
        l.push_str(&(p.table.seal_hash % 1_000_000).to_string());
        lines.push(l);
        Ok(CalibReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 6 {
            return Err(E_TRACE);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_TRACE);
            }
        }
        Ok(())
    }
}

/// 分位函数自检：已知序样本（1..=10）取 k=4 → 5。
pub fn pick_order_audit() -> Result<(), u16> {
    let mut s = Vec::new();
    for i in 1..=10u64 {
        s.push(i * 10);
    }
    if pick_order(&s, 4)? != 50 {
        return Err(E_STAT);
    }
    if pick_order(&s, 0)? != 10 {
        return Err(E_STAT);
    }
    if pick_order(&s, 9)? != 100 {
        return Err(E_STAT);
    }
    if pick_order(&s, 10) != Err(E_STAT) {
        return Err(SEG_BASE + 0x31);
    }
    if pick_order(&[], 0) != Err(E_SAMPLE) {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 规程全跑 + 封存核。
pub fn run_full_procedure(seed: u64) -> Result<CalibProcedure, u16> {
    let mut p = CalibProcedure::new();
    p.do_sampling(seed, PILOT_N)?;
    p.do_statistics()?;
    p.do_freeze()?;
    p.assert_trace_full()?;
    p.table.verify_seal()?;
    Ok(p)
}

/// 自例程：三步留痕齐 + 冻结哈希封存 + ADR 改写路径 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    pick_order_audit()?;

    let mut p = run_full_procedure(0x0665)?;
    // 注入 1：跳步 → 拦截
    p.inject_skip_must_fail()?;
    // 注入 2：冻结后无 ADR 写 → 拦截
    if p.table.try_write_no_adr() != Err(E_SEALED) {
        return Err(SEG_BASE + 0x33);
    }
    // 注入 3：封存哈希篡改 → 复核检出
    let mut tamper = run_full_procedure(0x0665)?;
    tamper.table.tiers[2] += 1;
    if tamper.table.verify_seal() != Err(E_HASH) {
        return Err(SEG_BASE + 0x34);
    }
    // 注入 4：ADR 路径——合法改写必须重封出新哈希
    let mut p2 = run_full_procedure(0x0665)?;
    let old_hash = p2.table.seal_hash;
    let adr = AdrTicket { id: 7, target: 2, new_ms: p2.table.tiers[2] + 100 };
    p2.table.amend_with_adr(&adr, &mut p2.used_adr)?;
    if p2.table.seal_hash == old_hash {
        return Err(SEG_BASE + 0x35);
    }
    p2.table.verify_seal()?;
    // 注入 5：ADR 单号重用 → 拦截
    let adr2 = AdrTicket { id: 7, target: 0, new_ms: 2_000 };
    if p2.table.amend_with_adr(&adr2, &mut p2.used_adr) != Err(E_ADR) {
        return Err(SEG_BASE + 0x36);
    }
    // 注入 6：ADR 单号 0 → 拦截
    let adr3 = AdrTicket { id: 0, target: 0, new_ms: 2_000 };
    if p2.table.amend_with_adr(&adr3, &mut p2.used_adr) != Err(E_ADR) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入 7：改写破非降序 → 拦截且表复原
    let before = p2.table.tiers;
    let adr4 = AdrTicket { id: 8, target: 0, new_ms: p2.table.tiers[2] + 5 };
    if p2.table.amend_with_adr(&adr4, &mut p2.used_adr) != Err(E_STAT) {
        return Err(SEG_BASE + 0x38);
    }
    if p2.table.tiers != before {
        return Err(SEG_BASE + 0x39);
    }
    // 注入 8：样本量不足 → 拦截
    let mut p3 = CalibProcedure::new();
    if p3.do_sampling(1, 10) != Err(E_SAMPLE) {
        return Err(SEG_BASE + 0x3A);
    }
    // 注入 9：回退拦截（冻结后再采样）
    let mut p4 = run_full_procedure(0x0665)?;
    if p4.do_sampling(1, PILOT_N) != Err(E_PHASE) {
        return Err(SEG_BASE + 0x3B);
    }
    // 基准表登记：三机逐机入表 + 审计 + 封簿后写拦截
    let mut reg = BaselineRegistry::new();
    for m in 0..3usize {
        let pt = run_full_procedure(0x0665 ^ (m as u64 + 1))?;
        reg.enroll(m, &pt.table)?;
    }
    reg.audit()?;
    // 注入 10：同机型重入 → 拦截
    if reg.enroll(1, &p.table) != Err(E_PHASE) {
        return Err(SEG_BASE + 0x3C);
    }
    // 注入 11：未冻结表入基准表 → 拦截
    let open_tbl = ThresholdTable::open();
    let mut reg2 = BaselineRegistry::new();
    if reg2.enroll(0, &open_tbl) != Err(E_STAT) {
        return Err(SEG_BASE + 0x3D);
    }
    reg.seal()?;
    if reg.enroll(0, &p.table) != Err(E_SEALED) {
        return Err(SEG_BASE + 0x3E);
    }
    // ADR 史账：合法改写留痕 + 重用拦截 + 审计
    let mut hist = AdrHistory::new();
    let mut used = Vec::new();
    let a = AdrTicket { id: 21, target: 2, new_ms: p.table.tiers[2] + 50 };
    let old_v = p.table.tiers[2];
    p.table.amend_with_adr(&a, &mut used)?;
    hist.record(&a, old_v)?;
    if hist.record(&a, old_v) != Err(E_ADR) {
        return Err(SEG_BASE + 0x3F);
    }
    hist.audit()?;
    // 规程报告：6 行全生成
    let rep = CalibReport::generate(&p4)?;
    rep.assert_complete()?;
    Ok(0)
}
