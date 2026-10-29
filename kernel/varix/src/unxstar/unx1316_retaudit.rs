//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1316 · 退役审计账（退役全程飞行记录） · 0x7650
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1316-J1 退役/回退/清理/窗口事件审计记录完整率 100%
//!      （五元组），审计可导出 100%，滚动保底常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 事件四类：Retire(退役)/Rollback(回退)/Cleanup(清理)/Window
//!     (窗口)，每事件一条五元组记录（序号/事件类/目标点/逻辑tick/
//!     结果码），缺字段必抓；
//!   - 逻辑时钟：单调递增 tick（禁墙钟），倒流必拒；
//!   - 滚动环：容量 128，环满滚动淘汰最旧；保底判准=最近 FLOOR 条
//!     记录永不丢（滚动保底常绿），环溢未滚必抓；
//!   - 审计校验链：逐记录卷积指纹（×31+字段），链破必抓（篡改面）；
//!   - 导出契约：全量导出（记录数+五元组逐条），导出内容与账内
//!     逐位一致（导出率 1000‰，导出缺失必抓）；
//!   - 记录账 append-only：写后禁改（冻结语义），改写必抓；
//!   - 注入核 5 项（缺字段/序断号/保底破约/导出缺失/校验链破）
//!     全部必抓，LCG 种子写死 0x7650_0012。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7650 段，审计账专属）。
pub const SEG_BASE: u16 = 0x7650;

// ── 诊断码闭集（0x7650..0x7659 十码） ───────────────────────
/// 0x7650 五元缺字段。
pub const E_FIELD_MISS: u16 = SEG_BASE;
/// 0x7651 序断号。
pub const E_SEQ_GAP: u16 = SEG_BASE + 1;
/// 0x7652 保底破约。
pub const E_FLOOR_BURST: u16 = SEG_BASE + 2;
/// 0x7653 导出缺失。
pub const E_EXPORT_MISS: u16 = SEG_BASE + 3;
/// 0x7654 校验链破。
pub const E_CSUM_BAD: u16 = SEG_BASE + 4;
/// 0x7655 审计账冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 5;
/// 0x7656 事件类越界。
pub const E_CLASS_OOB: u16 = SEG_BASE + 6;
/// 0x7657 环溢未滚。
pub const E_RING_FULL: u16 = SEG_BASE + 7;
/// 0x7658 空审计账。
pub const E_EMPTY: u16 = SEG_BASE + 8;
/// 0x7659 时钟倒流。
pub const E_TICK_BACK: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_FIELD_MISS => "五元缺字段：序号/类/点/tick/结果码必须全填",
        E_SEQ_GAP => "序断号：补齐缺失审计记录后重审",
        E_FLOOR_BURST => "保底破约：滚动时必须保留最近 FLOOR 条",
        E_EXPORT_MISS => "导出缺失：全量导出条数必须等于账内条数",
        E_CSUM_BAD => "校验链破：逐记录重算指纹，定位被改写条",
        E_RDONLY => "审计账冻结：写后禁改，补充事件走追加",
        E_CLASS_OOB => "事件类越界：四类只收 0..4",
        E_RING_FULL => "环溢未滚：环满必须淘汰最旧再入账",
        E_EMPTY => "空审计账：零记录不可出完整率，先记账",
        E_TICK_BACK => "时钟倒流：逻辑 tick 必须单调递增",
        _ => "未知 0x7650 段诊断码：核对审计账段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// 环容量。
pub const RING_CAP: usize = 128;
/// 滚动保底条数。
pub const FLOOR_KEEP: usize = 32;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// 事件类数。
pub const CLASS_COUNT: usize = 4;

/// 事件四类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvClass {
    /// 退役。
    Retire,
    /// 回退。
    Rollback,
    /// 清理。
    Cleanup,
    /// 窗口。
    Window,
}

impl EvClass {
    /// 类号。
    pub const fn code(self) -> u8 {
        match self {
            EvClass::Retire => 0,
            EvClass::Rollback => 1,
            EvClass::Cleanup => 2,
            EvClass::Window => 3,
        }
    }
    /// 按号取类。
    pub const fn of_code(c: u8) -> EvClass {
        match c {
            0 => EvClass::Retire,
            1 => EvClass::Rollback,
            2 => EvClass::Cleanup,
            _ => EvClass::Window,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            EvClass::Retire => "退役",
            EvClass::Rollback => "回退",
            EvClass::Cleanup => "清理",
            EvClass::Window => "窗口",
        }
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 五元组记录 ──────────────────────────────────────────────
/// 五元组审计记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRec {
    /// 序号（1 起连续）。
    pub seq: u32,
    /// 事件类。
    pub class: EvClass,
    /// 目标点（0..12；窗口事件记路径号 0..3 + 16 偏移避免歧义）。
    pub target: u16,
    /// 逻辑 tick。
    pub tick: u64,
    /// 结果码（0=成功）。
    pub result: u16,
}

impl AuditRec {
    /// 记录指纹（五字段卷积）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp = self.seq as u64;
        fp = fp.wrapping_mul(31).wrapping_add(self.class.code() as u64);
        fp = fp.wrapping_mul(131).wrapping_add(self.target as u64);
        fp = fp.wrapping_mul(17).wrapping_add(self.tick);
        fp = fp.wrapping_mul(7).wrapping_add(self.result as u64);
        fp
    }

    /// 五字段齐备（序号非零 + tick 非零 + 目标合法）。
    pub fn complete(&self) -> bool {
        self.seq > 0 && self.tick > 0 && self.target <= 16 + 3
    }
}

// ── 滚动环审计账 ────────────────────────────────────────────
/// 滚动环审计账（本单正本）。
#[derive(Debug)]
pub struct AuditRing {
    /// 记录环（保序）。
    recs: Vec<AuditRec>,
    /// 总入账数（含被滚动淘汰的）。
    total: u64,
    /// 滚动淘汰数。
    evicted: u64,
    /// 逻辑时钟。
    tick: u64,
    /// 校验链。
    chain: u64,
    /// 冻结旗（写后禁改）。
    frozen: bool,
}

impl AuditRing {
    /// 新账（tick 从 1 起）。
    pub fn new() -> AuditRing {
        AuditRing { recs: Vec::new(), total: 0, evicted: 0, tick: 1, chain: 0x7650, frozen: false }
    }

    /// 记账（序号自动连续 + tick 单调 + 环满滚动保底）。
    pub fn record(&mut self, class: EvClass, target: u16, result: u16) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if class.code() as usize >= CLASS_COUNT {
            return Err(E_CLASS_OOB);
        }
        if target > 16 + 3 {
            return Err(E_FIELD_MISS);
        }
        if result == u16::MAX {
            return Err(E_FIELD_MISS);
        }
        let seq = (self.total + 1) as u32;
        let tick = self.tick;
        self.tick += 1; // 单调推进。
        let rec = AuditRec { seq, class, target, tick, result };
        if !rec.complete() {
            return Err(E_FIELD_MISS);
        }
        // 环满滚动（保底：滚动淘汰最旧，账内恒 ≥FLOOR_KEEP 且 ≤RING_CAP）。
        if self.recs.len() >= RING_CAP {
            // 滚动淘汰：环满逐条滚出最旧；链按现账重放（保持链可重算）。
            self.recs.remove(0);
            self.evicted += 1;
            let mut c: u64 = 0x7650;
            for r in self.recs.iter() {
                c = c.wrapping_mul(31).wrapping_add(r.fingerprint());
            }
            self.chain = c;
            if self.recs.len() >= RING_CAP {
                return Err(E_RING_FULL);
            }
        }
        self.chain = self.chain.wrapping_mul(31).wrapping_add(rec.fingerprint());
        self.recs.push(rec);
        self.total += 1;
        Ok(seq)
    }

    /// 时钟直接设值（注入面用；倒流必拒）。
    pub fn set_tick(&mut self, t: u64) -> Result<(), u16> {
        if t < self.tick {
            return Err(E_TICK_BACK);
        }
        self.tick = t;
        Ok(())
    }

    /// 账内记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 空账。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 总入账数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 淘汰数。
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// 保底常绿判准（账内条数 ≥ min(总入账, FLOOR_KEEP)）。
    pub fn floor_ok(&self) -> bool {
        let expect = if (self.total as usize) < FLOOR_KEEP { self.total as usize } else { FLOOR_KEEP };
        self.recs.len() >= expect
    }

    /// 序连续审计。
    pub fn seq_continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            // 环滚动后序号仍应递增（起点 = total-len+1）。
            let want = (self.total as usize - self.recs.len() + i + 1) as u32;
            if r.seq != want {
                return false;
            }
        }
        true
    }

    /// tick 单调审计。
    pub fn tick_monotonic(&self) -> bool {
        for w in self.recs.windows(2) {
            if w[1].tick <= w[0].tick {
                return false;
            }
        }
        true
    }

    /// 校验链审计（重放卷积）。
    pub fn chain_ok(&self) -> bool {
        let mut c: u64 = 0x7650;
        for r in self.recs.iter() {
            c = c.wrapping_mul(31).wrapping_add(r.fingerprint());
        }
        c == self.chain
    }

    /// 全审（链 + 序 + tick + 保底 + 完整）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.recs.is_empty() {
            return Err(E_EMPTY);
        }
        if !self.chain_ok() {
            return Err(E_CSUM_BAD);
        }
        if !self.seq_continuous() {
            return Err(E_SEQ_GAP);
        }
        if !self.tick_monotonic() {
            return Err(E_TICK_BACK);
        }
        if !self.floor_ok() {
            return Err(E_FLOOR_BURST);
        }
        for r in self.recs.iter() {
            if !r.complete() {
                return Err(E_FIELD_MISS);
            }
        }
        Ok(())
    }

    /// 记录完整率（千分制：完整条×1000/账内条）。
    pub fn complete_permille(&self) -> u64 {
        if self.recs.is_empty() {
            return 0;
        }
        let n = self.recs.iter().filter(|r| r.complete()).count();
        (n as u64) * 1000 / self.recs.len() as u64
    }

    /// 全量导出（导出内容与账内逐位一致判准用）。
    pub fn export(&self) -> Result<Vec<AuditRec>, u16> {
        if self.recs.is_empty() {
            return Err(E_EMPTY);
        }
        Ok(self.recs.clone())
    }

    /// 导出一致（导出逐条与账内相等）。
    pub fn export_matches(&self) -> bool {
        match self.export() {
            Ok(v) => {
                v.len() == self.recs.len()
                    && v.iter().zip(self.recs.iter()).all(|(a, b)| a == b)
                    // 账实相符：账内条数 == 总入账 - 滚动淘汰。
                    && self.recs.len() as u64 == self.total.saturating_sub(self.evicted)
            }
            Err(_) => false,
        }
    }

    /// 冻结（写后禁改）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 四类覆盖（每类至少一条）。
    pub fn classes_covered(&self) -> bool {
        for c in 0..CLASS_COUNT {
            if !self.recs.iter().any(|r| r.class.code() as usize == c) {
                return false;
            }
        }
        true
    }
}

// ── 标准飞行脚本（全四类事件） ──────────────────────────────
/// 标准飞行：每路径 1 窗+4 退役+1 回退+1 清理 = 7 条 ×3 路径 = 21 条。
pub fn fly_standard(ring: &mut AuditRing) -> Result<usize, u16> {
    let mut n = 0usize;
    for p in 0..3u16 {
        // 窗口（目标=路径号+16 偏移）。
        ring.record(EvClass::Window, 16 + p, 0)?;
        n += 1;
        // 退役 4 点。
        for point in 0..4u16 {
            ring.record(EvClass::Retire, p * 4 + point, 0)?;
            n += 1;
        }
        // 回退 1 点（演练回退）。
        ring.record(EvClass::Rollback, p * 4, 0)?;
        n += 1;
        // 清理 1 点。
        ring.record(EvClass::Cleanup, p * 4, 0)?;
        n += 1;
    }
    Ok(n)
}

// ── 演练 ────────────────────────────────────────────────────
/// 标准飞行演练（21 条全记 + 全审 + 四类覆盖）。
pub fn drill_fly() -> (bool, String) {
    let mut ring = AuditRing::new();
    let n = match fly_standard(&mut ring) {
        Ok(n) => n,
        Err(c) => return (false, format!("飞行错=0x{:04X} {}", c, detail(c))),
    };
    let audit = ring.audit().is_ok();
    (n == 21 && audit && ring.classes_covered() && ring.complete_permille() == 1000,
     format!("记录={} 全审={} 四类齐={}", n, audit, ring.classes_covered()))
}

/// 五元组完整率 1000‰ 演练 10/10。
pub fn drill_complete_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7650_5EED);
    for _ in 0..ROUNDS {
        let mut ring = AuditRing::new();
        let mut good = true;
        for _ in 0..8 {
            let c = rng.below(CLASS_COUNT as u64) as u8;
            let t = rng.below(16) as u16;
            if ring.record(EvClass::of_code(c), t, 0).is_err() {
                good = false;
            }
        }
        if good && ring.complete_permille() == 1000 && ring.audit().is_ok() {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("完整={}/{}", ok, ROUNDS))
}

/// 滚动保底演练（灌 200 条 > 容量 128 → 滚动淘汰 + 保底 32 条常绿）。
pub fn drill_rolling_floor() -> (bool, String) {
    let mut ring = AuditRing::new();
    let mut ok = true;
    for i in 0..200u16 {
        if ring.record(EvClass::Retire, (i % 12) as u16, 0).is_err() {
            ok = false;
        }
    }
    let floor = ring.floor_ok();
    let evicted = ring.evicted();
    let len = ring.len();
    let audit = ring.audit().is_ok();
    // 保底：账内最后 FLOOR_KEEP 条 seq 连续且最新。
    let last_seq = match ring.recs.last() {
        Some(r) => r.seq,
        None => 0,
    };
    (ok && floor && evicted > 0 && len >= FLOOR_KEEP && len <= RING_CAP && audit && last_seq == 200,
     format!("淘汰={} 账内={} 保底={} 最新序={}", evicted, len, floor, last_seq))
}

/// 导出契约演练（导出与账内逐位一致 + 空账导出必拒）。
pub fn drill_export() -> (bool, String) {
    let mut ring = AuditRing::new();
    let _ = fly_standard(&mut ring);
    let matches = ring.export_matches();
    let empty_ring = AuditRing::new();
    let empty_refused = matches!(empty_ring.export(), Err(E_EMPTY));
    (matches && empty_refused,
     format!("导出一致={} 空账拒={}", matches, empty_refused))
}

/// 校验链破必抓演练（篡改账内记录 → 链审计必抓）。
pub fn drill_chain_tamper() -> (bool, String) {
    let mut ring = AuditRing::new();
    let _ = fly_standard(&mut ring);
    let clean = ring.chain_ok();
    // 篡改第 3 条结果码。
    if let Some(r) = ring.recs.get_mut(2) {
        r.result = 99;
    }
    let caught = matches!(ring.audit(), Err(E_CSUM_BAD));
    (clean && caught, format!("净链={} 篡改抓={}", clean, caught))
}

/// 时钟倒流必拒演练。
pub fn drill_tick_back() -> (bool, String) {
    let mut ring = AuditRing::new();
    let _ = ring.record(EvClass::Window, 16, 0);
    let cur = ring.tick;
    let back = matches!(ring.set_tick(cur - 1), Err(E_TICK_BACK));
    let fwd = ring.set_tick(cur + 10).is_ok();
    // 倒流后记账：tick 用新值仍单调。
    let after = ring.record(EvClass::Retire, 0, 0).is_ok();
    let mono = ring.tick_monotonic();
    (back && fwd && after && mono, format!("倒流拒={} 前进收={} 记账={} 单调={}", back, fwd, after, mono))
}

/// 冻结拒写演练。
pub fn drill_frozen() -> (bool, String) {
    let mut ring = AuditRing::new();
    let _ = fly_standard(&mut ring);
    ring.freeze();
    let frozen = ring.is_frozen();
    let refused = matches!(ring.record(EvClass::Retire, 0, 0), Err(E_RDONLY));
    (frozen && refused, format!("冻结={} 拒写={}", frozen, refused))
}

/// 事件类越界（结果码哨兵）必拒演练。
pub fn drill_field_guards() -> (bool, String) {
    let mut ring = AuditRing::new();
    let sentinel = matches!(ring.record(EvClass::Retire, 0, u16::MAX), Err(E_FIELD_MISS));
    let target_bad = matches!(ring.record(EvClass::Retire, 99, 0), Err(E_FIELD_MISS));
    (sentinel && target_bad, format!("哨兵拒={} 目标拒={}", sentinel, target_bad))
}


// ── 审计查询账（按类/按 tick 区间检索） ─────────────────────
/// 审计查询器。
pub struct AuditQuery;

impl AuditQuery {
    /// 按事件类检索。
    pub fn by_class(ring: &AuditRing, class: EvClass) -> Vec<AuditRec> {
        ring.recs.iter().filter(|r| r.class == class).copied().collect()
    }

    /// 按 tick 区间检索（含端点）。
    pub fn by_tick_range(ring: &AuditRing, lo: u64, hi: u64) -> Vec<AuditRec> {
        if lo > hi {
            return Vec::new();
        }
        ring.recs.iter().filter(|r| r.tick >= lo && r.tick <= hi).copied().collect()
    }

    /// 按目标点检索。
    pub fn by_target(ring: &AuditRing, target: u16) -> Vec<AuditRec> {
        ring.recs.iter().filter(|r| r.target == target).copied().collect()
    }

    /// 查询结果自洽（结果 ⊆ 账内且逐条指纹一致）。
    pub fn results_valid(ring: &AuditRing, out: &[AuditRec]) -> bool {
        out.iter().all(|o| ring.recs.iter().any(|r| r == o && r.fingerprint() == o.fingerprint()))
    }
}

// ── 类分布直方图账 ──────────────────────────────────────────
/// 类分布直方图（四类计数 + 占比千分制）。
#[derive(Debug)]
pub struct ClassHistogram {
    /// 四类计数（下标 = 类号）。
    pub counts: [usize; CLASS_COUNT],
}

impl ClassHistogram {
    /// 从账统计。
    pub fn build(ring: &AuditRing) -> ClassHistogram {
        let mut counts = [0usize; CLASS_COUNT];
        for r in ring.recs.iter() {
            counts[r.class.code() as usize] += 1;
        }
        ClassHistogram { counts }
    }

    /// 总数守恒（四类和 == 账内条数）。
    pub fn conserved(&self, ring: &AuditRing) -> bool {
        self.counts.iter().sum::<usize>() == ring.len()
    }

    /// 占比（千分制；空账返回 0）。
    pub fn permille_of(&self, class: EvClass) -> u64 {
        let total: usize = self.counts.iter().sum();
        if total == 0 {
            return 0;
        }
        (self.counts[class.code() as usize] as u64) * 1000 / total as u64
    }

    /// 四类覆盖（每类 >0）。
    pub fn all_present(&self) -> bool {
        self.counts.iter().all(|c| *c > 0)
    }
}

// ── CSV 导出账（逗号分隔逐行导出） ─────────────────────────
/// CSV 导出（seq,class,target,tick,result 逐行）。
pub fn export_csv(ring: &AuditRing) -> Result<String, u16> {
    if ring.is_empty() {
        return Err(E_EMPTY);
    }
    let mut out = String::from("seq,class,target,tick,result
");
    for r in ring.recs.iter() {
        out.push_str(&format!("{},{},{},{},{}
",
            r.seq, r.class.code(), r.target, r.tick, r.result));
    }
    // 行数守恒（数据行 == 账内条数）。
    let data_rows = out.lines().count() - 1;
    if data_rows != ring.len() {
        return Err(E_EXPORT_MISS);
    }
    Ok(out)
}

// ── 追加演练 ────────────────────────────────────────────────
/// 查询演练（按类/按区间/按点三通道 + 结果自洽 + 非法区间空）。
pub fn drill_query() -> (bool, String) {
    let mut ring = AuditRing::new();
    if fly_standard(&mut ring).is_err() {
        return (false, "飞行失败".to_string());
    }
    let by_class = AuditQuery::by_class(&ring, EvClass::Retire);
    let by_range = AuditQuery::by_tick_range(&ring, 1, 6);
    let by_point = AuditQuery::by_target(&ring, 0);
    let bad_range = AuditQuery::by_tick_range(&ring, 9, 1);
    let ok1 = AuditQuery::results_valid(&ring, &by_class);
    let ok2 = AuditQuery::results_valid(&ring, &by_range);
    (by_class.len() == 12 && ok1 && !by_range.is_empty() && ok2
        && !by_point.is_empty() && bad_range.is_empty(),
     format!("类查={} 区间查={} 点查={} 逆区间空={}",
             by_class.len(), by_range.len(), by_point.len(), bad_range.is_empty()))
}

/// 直方图演练（守恒 + 全类覆盖 + 占比和≈1000）。
pub fn drill_histogram() -> (bool, String) {
    let mut ring = AuditRing::new();
    if fly_standard(&mut ring).is_err() {
        return (false, "飞行失败".to_string());
    }
    let h = ClassHistogram::build(&ring);
    let conserved = h.conserved(&ring);
    let present = h.all_present();
    let mut sum = 0u64;
    for c in 0..CLASS_COUNT {
        sum += h.permille_of(EvClass::of_code(c as u8));
    }
    (conserved && present && sum >= 990 && sum <= 1010,
     format!("守恒={} 全类={} 占比和={}", conserved, present, sum))
}

/// CSV 导出演练（行守恒 + 表头 + 空账拒）。
pub fn drill_csv() -> (bool, String) {
    let mut ring = AuditRing::new();
    if fly_standard(&mut ring).is_err() {
        return (false, "飞行失败".to_string());
    }
    let csv = match export_csv(&ring) {
        Ok(c) => c,
        Err(c) => return (false, format!("导出错=0x{:04X} {}", c, detail(c))),
    };
    let header_ok = csv.starts_with("seq,class,target,tick,result");
    let empty = AuditRing::new();
    let empty_refused = matches!(empty.export(), Err(E_EMPTY));
    (header_ok && csv.lines().count() == 22 && empty_refused,
     format!("表头={} 行数={} 空账拒={}", header_ok, csv.lines().count(), empty_refused))
}


// ── 审计摘要账（链指纹 + 逐类计数定格） ────────────────────
/// 审计摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditDigest {
    /// 账内条数。
    pub len: usize,
    /// 总入账。
    pub total: u64,
    /// 淘汰数。
    pub evicted: u64,
    /// 链指纹。
    pub chain: u64,
}

/// 摘要采样器（两次采样逐位一致判准）。
pub struct DigestBook;

impl DigestBook {
    /// 采样。
    pub fn sample(ring: &AuditRing) -> AuditDigest {
        AuditDigest {
            len: ring.len(),
            total: ring.total(),
            evicted: ring.evicted(),
            chain: {
                let mut c: u64 = 0x7650;
                for r in ring.recs.iter() {
                    c = c.wrapping_mul(31).wrapping_add(r.fingerprint());
                }
                c
            },
        }
    }

    /// 两次采样一致。
    pub fn stable(ring: &AuditRing) -> bool {
        DigestBook::sample(ring) == DigestBook::sample(ring)
    }
}

/// 追加演练：审计摘要（两采样逐位一致 + 字段自洽）。
pub fn drill_digest() -> (bool, String) {
    let mut ring = AuditRing::new();
    if fly_standard(&mut ring).is_err() {
        return (false, "飞行失败".to_string());
    }
    let d = DigestBook::sample(&ring);
    let stable = DigestBook::stable(&ring);
    (stable && d.len == 21 && d.total == 21 && d.evicted == 0,
     format!("摘要 条={} 总={} 淘汰={} 稳定={}", d.len, d.total, d.evicted, stable))
}

// ── 保底水位探测（不同总量下的 floor_ok 语义谱） ───────────
/// 水位探测行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloorProbe {
    /// 总入账数。
    pub total: usize,
    /// 账内条数。
    pub kept: usize,
    /// 保底达标。
    pub ok: bool,
}

/// 水位探测谱（total = 4/12/32/64/128/200 六档）。
pub fn floor_probe_spectrum() -> Vec<FloorProbe> {
    let mut out = Vec::new();
    for total in [4usize, 12, 32, 64, 128, 200] {
        let mut ring = AuditRing::new();
        let mut good = true;
        for i in 0..total {
            if ring.record(EvClass::Retire, (i % 12) as u16, 0).is_err() {
                good = false;
            }
        }
        let expect = if total < FLOOR_KEEP { total } else { FLOOR_KEEP };
        out.push(FloorProbe {
            total,
            kept: ring.len(),
            ok: good && ring.len() >= expect && ring.floor_ok(),
        });
    }
    out
}

/// 追加演练：水位探测谱（六档全达标 + 滚动档账内恒 FLOOR）。
pub fn drill_floor_spectrum() -> (bool, String) {
    let sp = floor_probe_spectrum();
    let all_ok = sp.iter().all(|p| p.ok);
    let rolling_kept = sp.iter().filter(|p| p.total >= RING_CAP).all(|p| p.kept == RING_CAP);
    (all_ok && rolling_kept && sp.len() == 6,
     format!("谱 {} 档 全达标={} 滚动档恒环满={}", sp.len(), all_ok, rolling_kept))
}

// ── 类名表审计（四类名闭环 + 类覆盖率千分制） ───────────────
/// 类名表审计。
pub struct ClassAudit;

impl ClassAudit {
    /// 四类名闭环（Retire/Rollback/Cleanup/Window 逐类名非空且码唯一）。
    pub fn names_ok() -> bool {
        let all = [
            (EvClass::Retire.code(), EvClass::Retire.text()),
            (EvClass::Rollback.code(), EvClass::Rollback.text()),
            (EvClass::Cleanup.code(), EvClass::Cleanup.text()),
            (EvClass::Window.code(), EvClass::Window.text()),
        ];
        all.iter().enumerate().all(|(i, (c, n))| *c as usize == i && n.len() > 0)
    }

    /// 类覆盖率（千分制：飞行账覆盖类数×1000/4）。
    pub fn coverage_permille(ring: &AuditRing) -> u64 {
        let mut n = 0u64;
        for c in 0..CLASS_COUNT {
            if ring.classes_covered() || ring.recs.iter().any(|r| r.class.code() as usize == c) {
                n += 1;
            }
        }
        n * 1000 / CLASS_COUNT as u64
    }
}

/// 追加演练：类名闭环 + 覆盖率 1000‰。
pub fn drill_class_audit() -> (bool, String) {
    let mut ring = AuditRing::new();
    if fly_standard(&mut ring).is_err() {
        return (false, "飞行失败".to_string());
    }
    let permille = ClassAudit::coverage_permille(&ring);
    (ClassAudit::names_ok() && permille == 1000,
     format!("类名闭环={} 覆盖率={}‰", ClassAudit::names_ok(), permille))
}

/// 追加演练：导出可逆逐位（export 逐条与账内五字段全等）。
pub fn drill_export_bitwise() -> (bool, String) {
    let mut ring = AuditRing::new();
    if fly_standard(&mut ring).is_err() {
        return (false, "飞行失败".to_string());
    }
    let exported = match ring.export() {
        Ok(v) => v,
        Err(c) => return (false, format!("导出错=0x{:04X} {}", c, detail(c))),
    };
    let bitwise = exported.iter().zip(ring.recs.iter()).all(|(a, b)| {
        a.seq == b.seq && a.class == b.class && a.target == b.target
            && a.tick == b.tick && a.result == b.result && a.fingerprint() == b.fingerprint()
    });
    (bitwise && exported.len() == 21,
     format!("逐位一致={} 条数={} 五字段全等", bitwise, exported.len()))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缺字段（哨兵结果码）。
    {
        let mut ring = AuditRing::new();
        if matches!(ring.record(EvClass::Cleanup, 1, u16::MAX), Err(E_FIELD_MISS)) { caught += 1; d.push("注1 缺字:抓".to_string()); } else { d.push("注1 缺字:漏".to_string()); }
    }
    // 注 2：序断号（伪造跳号）。
    {
        let mut ring = AuditRing::new();
        let _ = fly_standard(&mut ring);
        if let Some(r) = ring.recs.get_mut(4) {
            r.seq += 10;
        }
        if !ring.seq_continuous() { caught += 1; d.push("注2 断号:抓".to_string()); } else { d.push("注2 断号:漏".to_string()); }
    }
    // 注 3：保底破约（伪造账内条数低于保底）。
    {
        let mut ring = AuditRing::new();
        let _ = fly_standard(&mut ring);
        // 21 条 ≥ FLOOR_KEEP：floor_ok 语义是 ≥min(total, FLOOR)=32 → 本例绿。
        // 注入破约：把账内砍到低于 min(total, FLOOR)。
        ring.recs.truncate(3);
        if !ring.floor_ok() { caught += 1; d.push("注3 保底:抓".to_string()); } else { d.push("注3 保底:漏".to_string()); }
    }
    // 注 4：导出缺失（导出条数 < 账内）。
    {
        let mut ring = AuditRing::new();
        let _ = fly_standard(&mut ring);
        ring.recs.pop();
        // 重放链保持链一致（只破坏账实相符：total 不补偿）。
        ring.chain = 0x7650;
        for r in ring.recs.iter() {
            ring.chain = ring.chain.wrapping_mul(31).wrapping_add(r.fingerprint());
        }
        if !ring.export_matches() { caught += 1; d.push("注4 导缺:抓".to_string()); } else { d.push("注4 导缺:漏".to_string()); }
    }
    // 注 5：校验链破。
    {
        let mut ring = AuditRing::new();
        let _ = fly_standard(&mut ring);
        ring.chain = ring.chain.wrapping_add(1);
        if matches!(ring.audit(), Err(E_CSUM_BAD)) { caught += 1; d.push("注5 链破:抓".to_string()); } else { d.push("注5 链破:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7650
        && E_TICK_BACK == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7650 段诊断码：核对审计账段位闭集"
        && RING_CAP > FLOOR_KEEP
}

/// run_unx1316_checks：本单判据聚合（诊断段 0x7650）。
pub fn run_unx1316_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1316 退役审计账");
    let (ok1, d1) = drill_fly();
    s.add_dyn("J1-标准飞行四类齐", ok1, &d1);
    let (ok2, d2) = drill_complete_10();
    s.add_dyn("J1-五元完整率10/10", ok2, &d2);
    let (ok3, d3) = drill_rolling_floor();
    s.add_dyn("J1-滚动保底常绿", ok3, &d3);
    let (ok4, d4) = drill_export();
    s.add_dyn("J1-导出契约", ok4, &d4);
    let (ok5, d5) = drill_chain_tamper();
    s.add_dyn("J1-校验链篡改必抓", ok5, &d5);
    let (ok6, d6) = drill_tick_back();
    s.add_dyn("J1-时钟倒流必拒", ok6, &d6);
    let (ok7, d7) = drill_frozen();
    s.add_dyn("J1-审计账冻结拒写", ok7, &d7);
    let (ok8, d8) = drill_field_guards();
    s.add_dyn("J1-字段守卫", ok8, &d8);
    let (ok9, d9) = drill_query();
    s.add_dyn("J2-审计三通道查询", ok9, &d9);
    let (ok10, d10) = drill_histogram();
    s.add_dyn("J2-类分布直方图", ok10, &d10);
    let (ok11, d11) = drill_csv();
    s.add_dyn("J2-CSV 导出账", ok11, &d11);
    let (ok12, d12) = drill_digest();
    s.add_dyn("J2-审计摘要账", ok12, &d12);
    let (ok13, d13) = drill_floor_spectrum();
    s.add_dyn("J2-保底水位探测谱", ok13, &d13);
    let (ok14, d14) = drill_class_audit();
    s.add_dyn("J2-类名表审计", ok14, &d14);
    let (ok15, d15) = drill_export_bitwise();
    s.add_dyn("J2-导出可逆逐位", ok15, &d15);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7650 段十码闭集");
    s
}
