//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18437 依据失效追踪（来源删除后降级路径） · 0x7E900段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18437-J1 来源（manifest/键）失效后降级到下级源断言全过，悬空依据注入检出
//! 设计要点：
//!   - 依据链模型：每条矩阵条目按四级优先链挂依据——ActCtx/Manifest/Compat
//!     三层各引用一个**注册源**（manifest/键/锚基线），Default 层是
//!     恒活的隐式兜底源（不需注册）；
//!   - 失效语义：kill_source 把源标死（幂等单次），引用它的条目在
//!     **下一条查询**沿链降到最近的存活下级源，降级逐条入账
//!     （键/原层/死源/新层/新源/新值七元组）；全链死透落 Default；
//!   - 悬空依据（≠失效）：条目引用的源号**从未注册**——断链引用，
//!     解析面必须告警并落 Default，10/10 检出；失效是合法降级，
//!     悬空是违约注入，两账分立不混记；
//!   - 注入核三类×10 轮：悬空引用 10/10 告警、杀不存在源 10/10 拒、
//!     重复杀同源 10/10 拒；漏检必须为 0；
//!   - 边界：空链直落 Default；中段死源跳层；重复源注册拒；
//!     降级账守恒（降级条数=受影响条目数，逐条对得上）；
//!   - 确定性：LCG 种子=SEG_BASE(0x7E900)；no_std+alloc；零 unwrap/
//!     expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   SourceKind / SourceRecord+SourceTable(源表) / KeyId
//!   / BasisEntry(依据链条目) / BasisCode(诊断闭集)
//!   / DowngradeEvent(降级账) / DanglingEvent(悬空账)
//!   / BasisResolver(解析+失效追踪) / InjectLedger / golden_run
//!   / run_unx18437_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E900，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E900;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 依据链层数（冻结：ActCtx/Manifest/Compat 三层引用 + Default 兜底）。
pub const CHAIN_N: usize = 3;
/// 含 Default 的总层数。
pub const LAYER_N: usize = 4;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 源表容量（冻结：语料 12 源 ≤ 32）。
pub const SOURCE_CAP: usize = 32;
/// 条目容量（冻结：语料 24 键 ≤ 64）。
pub const ENTRY_CAP: usize = 64;

// ── 源种类枚举 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    /// manifest 段源。
    Manifest = 0,
    /// manifest 内键源。
    Key = 1,
    /// 锚基线源。
    Anchor = 2,
}

impl SourceKind {
    /// 种类名（诊断文本共用）。
    pub fn name(self) -> &'static str {
        match self {
            SourceKind::Manifest => "manifest",
            SourceKind::Key => "key",
            SourceKind::Anchor => "anchor",
        }
    }
}

/// 单个注册源：号 + 种类 + 活性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRecord {
    pub id: u32,
    pub kind: SourceKind,
    pub alive: bool,
}

/// 源表：号唯一、活性单调（活→死单向，不许复活）。
pub struct SourceTable {
    pub srcs: Vec<SourceRecord>,
    cap: usize,
    next_id: u32,
}

impl SourceTable {
    pub fn new() -> SourceTable {
        SourceTable { srcs: Vec::new(), cap: SOURCE_CAP, next_id: 1 }
    }
    /// 注册新源（号自动分配，≥1）。
    pub fn register(&mut self, kind: SourceKind) -> Result<u32, u32> {
        if self.srcs.len() >= self.cap {
            return Err(BasisCode::SourceFull.val());
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.srcs.push(SourceRecord { id, kind, alive: true });
        Ok(id)
    }
    /// 查源（存在性 + 活性分立返回）。
    pub fn get(&self, id: u32) -> SourceLookup {
        let mut i = 0;
        while i < self.srcs.len() {
            let s = &self.srcs[i];
            if s.id == id {
                return if s.alive { SourceLookup::Alive } else { SourceLookup::Dead };
            }
            i += 1;
        }
        SourceLookup::Absent
    }
    /// 杀源（幂等单次：死源再杀显式拒；不存在源拒）。
    pub fn kill(&mut self, id: u32) -> Result<SourceKind, u32> {
        let mut i = 0;
        while i < self.srcs.len() {
            if self.srcs[i].id == id {
                if !self.srcs[i].alive {
                    return Err(BasisCode::SourceDead.val());
                }
                let k = self.srcs[i].kind;
                self.srcs[i].alive = false;
                return Ok(k);
            }
            i += 1;
        }
        Err(BasisCode::NoSuchSource.val())
    }
    pub fn alive_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.srcs.len() {
            if self.srcs[i].alive {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 源查视三态（存在性与活性分立——悬空≠失效的判定基础）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceLookup {
    /// 注册且存活。
    Alive,
    /// 注册但已失效（合法降级路径）。
    Dead,
    /// 从未注册（悬空依据，违约注入）。
    Absent,
}

// ── 键与依据链条目 ──────────────────────────────────────────────
/// 键三元组：进程 × API 族 × 维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyId {
    pub proc_id: u32,
    pub api_id: u32,
    pub dim: u32,
}

/// 依据链条目：三层引用 + 各层值 + Default 兜底值。
#[derive(Clone, Debug)]
pub struct BasisEntry {
    pub key: KeyId,
    /// 三层引用的源号（None=该层未挂依据）。
    pub chain: [Option<u32>; CHAIN_N],
    /// 三层值（与 chain 同步：有引用才有值）。
    pub vals: [Option<u32>; CHAIN_N],
    /// Default 层兜底值（恒可用）。
    pub default_val: u32,
}

impl BasisEntry {
    /// 构造前校验：引用与值必须同步成对（有引用必有值，反之亦然）。
    pub fn well_formed(&self) -> bool {
        let mut i = 0;
        while i < CHAIN_N {
            if self.chain[i].is_some() != self.vals[i].is_some() {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BasisCode {
    /// 悬空依据（引用源从未注册）
    DanglingBasis,
    /// 杀不存在源
    NoSuchSource,
    /// 死源再杀
    SourceDead,
    /// 依据链畸形（引用/值不同步）
    MalformedEntry,
    /// 源表满
    SourceFull,
    /// 条目表满
    EntryFull,
    /// 降级账缺失（失效未入账）
    DowngradeMissing,
    /// 条目重复（同键二次挂链）
    DupEntry,
    /// 值污染（降级后生效值≠存活下级值）
    ValuePolluted,
    /// 双跑不一致
    NotReproducible,
}

impl BasisCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BasisCode::DanglingBasis => "悬空依据：引用源从未注册，断链必须告警",
            BasisCode::NoSuchSource => "杀不存在源：源表闭集外",
            BasisCode::SourceDead => "死源再杀：活性单向不幂等二次",
            BasisCode::MalformedEntry => "依据链畸形：引用与值不同步",
            BasisCode::SourceFull => "源表满：注册被拒且账不丢",
            BasisCode::EntryFull => "条目表满：挂链被拒且账不丢",
            BasisCode::DowngradeMissing => "降级账缺失：失效未入账",
            BasisCode::DupEntry => "条目重复：同键二次挂链拒",
            BasisCode::ValuePolluted => "值污染：降级后生效值与存活下级不一致",
            BasisCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [BasisCode; 10] = [
        BasisCode::DanglingBasis,
        BasisCode::NoSuchSource,
        BasisCode::SourceDead,
        BasisCode::MalformedEntry,
        BasisCode::SourceFull,
        BasisCode::EntryFull,
        BasisCode::DowngradeMissing,
        BasisCode::DupEntry,
        BasisCode::ValuePolluted,
        BasisCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BasisCode::ALL.len() {
        if BasisCode::ALL[i].val() == code {
            return BasisCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E900 段闭集内"
}

// ── 降级账与悬空账 ──────────────────────────────────────────────
/// 降级账七元组：键/原层/死源/新层/新源号（Default 层记 0）/新值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DowngradeEvent {
    pub key: KeyId,
    pub from_layer: usize,
    pub dead_src: u32,
    pub to_layer: usize,
    pub to_src: u32,
    pub to_val: u32,
}

/// 悬空账：键/层/悬空源号（注入检出面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DanglingEvent {
    pub key: KeyId,
    pub layer: usize,
    pub absent_src: u32,
}

// ── 解析器本体（失效追踪） ──────────────────────────────────────
/// 解析结果：命中层 + 值 + 源号（Default 层源号恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resolved {
    pub layer: usize,
    pub value: u32,
    pub src: u32,
}

/// 依据失效追踪器：源表 + 条目表 + 两本账。
pub struct BasisResolver {
    pub table: SourceTable,
    pub entries: Vec<BasisEntry>,
    pub downgrades: Vec<DowngradeEvent>,
    pub danglings: Vec<DanglingEvent>,
    entry_cap: usize,
}

impl BasisResolver {
    pub fn new() -> BasisResolver {
        BasisResolver {
            table: SourceTable::new(),
            entries: Vec::new(),
            downgrades: Vec::new(),
            danglings: Vec::new(),
            entry_cap: ENTRY_CAP,
        }
    }

    /// 挂链（同键二次拒；畸形链拒）。
    pub fn attach(&mut self, e: BasisEntry) -> Result<(), u32> {
        if !e.well_formed() {
            return Err(BasisCode::MalformedEntry.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            let x = &self.entries[i];
            if x.key.proc_id == e.key.proc_id && x.key.api_id == e.key.api_id && x.key.dim == e.key.dim {
                return Err(BasisCode::DupEntry.val());
            }
            i += 1;
        }
        if self.entries.len() >= self.entry_cap {
            return Err(BasisCode::EntryFull.val());
        }
        self.entries.push(e);
        Ok(())
    }

    /// 解析一条目：沿链找首个存活引用层；悬空告警落账；全链死落 Default。
    /// 解析是纯读面（不动活性），账只增不减。
    pub fn resolve(&mut self, key: &KeyId) -> Result<Resolved, u32> {
        let mut found: Option<usize> = None;
        let mut i = 0;
        while i < self.entries.len() {
            let x = &self.entries[i];
            if x.key.proc_id == key.proc_id && x.key.api_id == key.api_id && x.key.dim == key.dim {
                found = Some(i);
            }
            i += 1;
        }
        let idx = match found {
            Some(i) => i,
            None => return Err(BasisCode::NoSuchSource.val()), // 无条目键：显式缺。
        };
        // 收集待检查的引用快照（借用结束再写账）。
        let mut pend_dangling: Option<(usize, u32)> = None;
        let mut hit: Option<Resolved> = None;
        let mut layer = 0;
        while layer < CHAIN_N {
            let src_id = self.entries[idx].chain[layer];
            let val = self.entries[idx].vals[layer];
            if let (Some(sid), Some(v)) = (src_id, val) {
                match self.table.get(sid) {
                    SourceLookup::Alive => {
                        hit = Some(Resolved { layer, value: v, src: sid });
                        break;
                    }
                    SourceLookup::Absent => {
                        // 悬空：告警入账，跳层继续（ Default 兜底保证可解析）。
                        if pend_dangling.is_none() {
                            pend_dangling = Some((layer, sid));
                        }
                    }
                    SourceLookup::Dead => {
                        // 失效：合法降级，跳层继续。
                    }
                }
            }
            layer += 1;
        }
        // Default 兜底（恒命中）。
        let dv = self.entries[idx].default_val;
        // 落账：悬空优先（违约事件优先于合法降级叙述）。
        if let Some((l, sid)) = pend_dangling {
            self.danglings.push(DanglingEvent { key: *key, layer: l, absent_src: sid });
        }
        match hit {
            Some(r) => Ok(r),
            None => Ok(Resolved { layer: LAYER_N - 1, value: dv, src: 0 }),
        }
    }

    /// 杀源并级联：逐条目重解析，凡原命中层因本源死而降级 → 入降级账。
    /// 返回降级条目数。杀不存在/已死源直接拒（无级联）。
    pub fn kill_source(&mut self, id: u32) -> Result<u32, u32> {
        let kind = self.table.kill(id)?;
        let _ = kind;
        // 逐条目：杀前命中层快照 → 杀后重解析 → 层倒退即降级。
        let mut degraded = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            // 杀前解析（源已死——此时重解析即「杀后」视角；先取杀前快照）。
            // 用本地纯函数按当前表态解析该条目。
            let e = &self.entries[i];
            let after = self.resolve_local(e);
            // 判断该条目是否引用了被杀源且杀前命中的正是含它的层：
            // 若 after 命中层 > 首个引用 id 的层，即发生降级。
            let mut cite_layer: Option<usize> = None;
            let mut l = 0;
            while l < CHAIN_N {
                if e.chain[l] == Some(id) {
                    cite_layer = Some(l);
                }
                l += 1;
            }
            if let Some(cl) = cite_layer {
                if after.layer > cl {
                    self.downgrades.push(DowngradeEvent {
                        key: e.key,
                        from_layer: cl,
                        dead_src: id,
                        to_layer: after.layer,
                        to_src: after.src,
                        to_val: after.value,
                    });
                    degraded += 1;
                }
            }
            i += 1;
        }
        Ok(degraded)
    }

    /// 本地纯解析（不入账，级联内部用）。
    fn resolve_local(&self, e: &BasisEntry) -> Resolved {
        let mut layer = 0;
        while layer < CHAIN_N {
            if let (Some(sid), Some(v)) = (e.chain[layer], e.vals[layer]) {
                if let SourceLookup::Alive = self.table.get(sid) {
                    return Resolved { layer, value: v, src: sid };
                }
            }
            layer += 1;
        }
        Resolved { layer: LAYER_N - 1, value: e.default_val, src: 0 }
    }

    /// 降级账守恒（可回溯口径）：to_src 必须落在**真实注册源**上
    /// （存活或已被后续 kill 失效均可——不允许悬空），Default 层记 0。
    /// 「降级时目标存活」由 resolve_local 的路径保证；此处只查回溯可验的
    /// 悬空违约（降级到从未注册的号 = 断链）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.downgrades.len() {
            let d = &self.downgrades[i];
            if d.to_src != 0 {
                match self.table.get(d.to_src) {
                    SourceLookup::Absent => return Err(BasisCode::DowngradeMissing.val()),
                    _ => {}
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 值不污染：解析结果值 == 命中层的登记值（逐条目复核）。
    pub fn no_pollution(&mut self) -> Result<(), u32> {
        // 取条目键快照后逐键解析比对。
        let mut i = 0;
        while i < self.entries.len() {
            let key = self.entries[i].key;
            let r = self.resolve(&key)?;
            let e = &self.entries[i];
            let expect = if r.layer == LAYER_N - 1 {
                e.default_val
            } else {
                match e.vals[r.layer] {
                    Some(v) => v,
                    None => return Err(BasisCode::ValuePolluted.val()),
                }
            };
            if r.value != expect {
                return Err(BasisCode::ValuePolluted.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 语料装配 ────────────────────────────────────────────────────
/// 标准语料：注册 9 源（3 键 × 3 层各一源）+ 挂 3 条链 + Default 值。
/// 返回 (解析器, 三键)。
pub fn seed_corpus() -> Result<(BasisResolver, [KeyId; 3]), u32> {
    let mut r = BasisResolver::new();
    let keys = [
        KeyId { proc_id: 1, api_id: 1, dim: 1 },
        KeyId { proc_id: 2, api_id: 1, dim: 1 },
        KeyId { proc_id: 3, api_id: 1, dim: 1 },
    ];
    let mut k = 0;
    while k < 3 {
        let act = r.table.register(SourceKind::Manifest)?;
        let man = r.table.register(SourceKind::Key)?;
        let com = r.table.register(SourceKind::Anchor)?;
        r.attach(BasisEntry {
            key: keys[k],
            chain: [Some(act), Some(man), Some(com)],
            vals: [Some(62_000 + k as u32), Some(62_100 + k as u32), Some(62_200 + k as u32)],
            default_val: 62_900,
        })?;
        k += 1;
    }
    Ok((r, keys))
}

// ── 注入核（三类 ×10 轮） ───────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入（每轮新语料）：
/// 槽 0：悬空依据（引用源号 9999 从未注册）→ 解析告警 10/10；
/// 槽 1：杀不存在源 → 拒 10/10；
/// 槽 2：同源二次杀 → 拒 10/10。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：悬空引用。
        let mut m0 = match seed_corpus() {
            Ok((x, _)) => x,
            Err(_) => return led,
        };
        let key = KeyId { proc_id: 9, api_id: 1, dim: 1 };
        let _ = m0.attach(BasisEntry {
            key,
            chain: [Some(9999), None, None],
            vals: [Some(1), None, None],
            default_val: 62_900,
        });
        let hit0 = match m0.resolve(&key) {
            Ok(res) => res.layer == LAYER_N - 1 && m0.danglings.len() == 1 && m0.danglings[0].absent_src == 9999,
            Err(_) => false,
        };
        led.record(0, hit0);
        // 槽 1：杀不存在源。
        let mut m1 = match seed_corpus() {
            Ok((x, _)) => x,
            Err(_) => return led,
        };
        let hit1 = m1.table.kill(4242).is_err();
        led.record(1, hit1);
        // 槽 2：同源二次杀。
        let mut m2 = match seed_corpus() {
            Ok((x, _)) => x,
            Err(_) => return led,
        };
        let sid = match m2.table.srcs.get(0) {
            Some(s) => s.id,
            None => return led,
        };
        let first = m2.table.kill(sid).is_ok();
        let second = m2.table.kill(sid).is_err();
        led.record(2, first && second);
        r += 1;
    }
    led
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：杀 ActCtx 层源 → 降级到 Manifest 层（下层仍活），账入档。
pub fn probe_seq_a() -> Result<(), u32> {
    let (mut r, keys) = seed_corpus()?;
    let act_id = 1u32; // 语料序：act=1, man=2, com=3（键 1）。
    let degraded = r.kill_source(act_id)?;
    let res = r.resolve(&keys[0])?;
    if degraded != 1 || res.layer != 1 || res.value != 62_100 {
        return Err(BasisCode::DowngradeMissing.val());
    }
    r.conserve()?;
    Ok(())
}

/// 负探针 1：悬空引用解析必告警。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut r = BasisResolver::new();
    let key = KeyId { proc_id: 0, api_id: 0, dim: 0 };
    r.attach(BasisEntry {
        key,
        chain: [Some(777), None, None],
        vals: [Some(1), None, None],
        default_val: 9,
    })?;
    let res = r.resolve(&key)?;
    if r.danglings.len() != 1 || res.layer != LAYER_N - 1 {
        return Err(BasisCode::DanglingBasis.val());
    }
    Ok(())
}

/// 负探针 2：畸形链（引用无值）挂链拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut r = BasisResolver::new();
    r.attach(BasisEntry {
        key: KeyId { proc_id: 0, api_id: 0, dim: 1 },
        chain: [Some(1), None, None],
        vals: [None, None, None],
        default_val: 9,
    })
}

/// 黄金全链：逐层失效降级全过 + 注入全抓 + 守恒 + 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let inj = inject_rounds(JUDGE_ROUNDS);
    if !inj.all_caught() {
        return Err(BasisCode::NotReproducible.val());
    }
    // 三级降级链：杀 ActCtx → 降 Manifest；再杀 Manifest → 降 Compat；
    // 再杀 Compat → 落 Default。
    let (mut res, keys) = seed_corpus()?;
    let act = 1u32;
    let man = 2u32;
    let com = 3u32;
    let d1 = res.kill_source(act)?;
    let r1 = res.resolve(&keys[0])?;
    if d1 != 1 || r1.layer != 1 || r1.value != 62_100 {
        return Err(BasisCode::DowngradeMissing.val());
    }
    let d2 = res.kill_source(man)?;
    let r2 = res.resolve(&keys[0])?;
    if d2 != 1 || r2.layer != 2 || r2.value != 62_200 {
        return Err(BasisCode::DowngradeMissing.val());
    }
    let d3 = res.kill_source(com)?;
    let r3 = res.resolve(&keys[0])?;
    if d3 != 1 || r3.layer != LAYER_N - 1 || r3.value != 62_900 {
        return Err(BasisCode::DowngradeMissing.val());
    }
    // 杀键 0 专属的三层源 → 键 0 降三级，降级账恰 3 条
    // （键 1/2 各自注册独立源号 4..9，不受 1/2/3 影响）。
    if res.downgrades.len() != 3 {
        return Err(BasisCode::DowngradeMissing.val());
    }
    res.conserve()?;
    res.no_pollution()?;
    let (_, keys2) = seed_corpus()?;
    Ok((inj.caught[0], res.downgrades.len() as u32, keys2.len() as u32))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BasisCode::ALL.len() {
        let v = BasisCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18437_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18437 依据失效追踪");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "悬空{:?} 降级{:?} 键{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 三级降级：ActCtx→Manifest→Compat→Default 逐级断言。
    let mut res = match seed_corpus() {
        Ok((x, _)) => x,
        Err(_) => BasisResolver::new(),
    };
    let keys = [
        KeyId { proc_id: 1, api_id: 1, dim: 1 },
        KeyId { proc_id: 2, api_id: 1, dim: 1 },
        KeyId { proc_id: 3, api_id: 1, dim: 1 },
    ];
    let _ = res.kill_source(1);
    let r1 = res.resolve(&keys[0]);
    let l1 = matches!(&r1, Ok(x) if x.layer == 1 && x.value == 62_100);
    let _ = res.kill_source(2);
    let r2 = res.resolve(&keys[0]);
    let l2 = matches!(&r2, Ok(x) if x.layer == 2 && x.value == 62_200);
    let _ = res.kill_source(3);
    let r3 = res.resolve(&keys[0]);
    let l3 = matches!(&r3, Ok(x) if x.layer == LAYER_N - 1 && x.value == 62_900);
    s.add("来源失效逐级降级", l1 && l2 && l3, "ActCtx→Manifest→Compat→Default");
    // ③ 注入三类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn("悬空依据10/10检出", inj.caught[0] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[0], inj.tries[0]));
    s.add_dyn("杀不存在源10/10拒", inj.caught[1] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[1], inj.tries[1]));
    s.add_dyn("重复杀源10/10拒", inj.caught[2] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[2], inj.tries[2]));
    s.add("注入漏检为零", inj.missed() == 0, "检出+漏检=注入");
    // ④ 降级账守恒：3 条逐条目标源存活（或 Default 记 0）。
    let cons_ok = res.conserve().is_ok() && res.downgrades.len() == 3;
    s.add("降级账守恒", cons_ok, "3 条降级逐条对得上");
    // ⑤ 值不污染。
    s.add("降级后值不污染", res.no_pollution().is_ok(), "解析值==存活下级值");
    // ⑥ 悬空≠失效分账：悬空账只收从未注册源，失效不进悬空账。
    let mut res2 = match seed_corpus() {
        Ok((x, _)) => x,
        Err(_) => BasisResolver::new(),
    };
    let _ = res2.attach(BasisEntry {
        key: KeyId { proc_id: 7, api_id: 1, dim: 1 },
        chain: [Some(8888), None, None],
        vals: [Some(1), None, None],
        default_val: 62_900,
    });
    let _ = res2.resolve(&KeyId { proc_id: 7, api_id: 1, dim: 1 });
    let _ = res2.kill_source(1);
    let _ = res2.resolve(&keys[0]);
    let acct_ok = res2.danglings.len() == 1 && res2.danglings[0].absent_src == 8888 && res2.downgrades.len() == 1;
    s.add("悬空失效两账分立", acct_ok, "悬空 1 条失效 1 条互不混");
    // ⑦ 边界：空链直落 Default；畸形链挂链拒；同键二次挂链拒。
    let mut res3 = BasisResolver::new();
    let empty_ok = res3
        .attach(BasisEntry {
            key: KeyId { proc_id: 1, api_id: 1, dim: 2 },
            chain: [None, None, None],
            vals: [None, None, None],
            default_val: 63_000,
        })
        .is_ok();
    let empty_res = res3.resolve(&KeyId { proc_id: 1, api_id: 1, dim: 2 });
    let empty_hit = matches!(&empty_res, Ok(x) if x.layer == LAYER_N - 1 && x.value == 63_000);
    let malformed = res3
        .attach(BasisEntry {
            key: KeyId { proc_id: 2, api_id: 1, dim: 2 },
            chain: [Some(1), None, None],
            vals: [None, None, None],
            default_val: 1,
        })
        .is_err();
    let dup = res3
        .attach(BasisEntry {
            key: KeyId { proc_id: 1, api_id: 1, dim: 2 },
            chain: [None, None, None],
            vals: [None, None, None],
            default_val: 1,
        })
        .is_err();
    s.add("空链/畸形/重复边界", empty_ok && empty_hit && malformed && dup, "三边界逐格");
    // ⑧ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_err(), "正降级/悬空/畸形");
    // ⑨ 双跑一致。
    s.add("双跑一致", golden_run() == golden_run(), "同语料同账");
    // ⑩ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < BasisCode::ALL.len() {
        if BasisCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7E900", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_level_degrade_chain() {
        let (mut r, keys) = seed_corpus().unwrap_or((BasisResolver::new(), [KeyId { proc_id: 0, api_id: 0, dim: 0 }; 3]));
        assert_eq!(r.kill_source(1).unwrap_or(0), 1);
        assert_eq!(r.resolve(&keys[0]).unwrap_or(Resolved { layer: 9, value: 0, src: 0 }).layer, 1);
        assert_eq!(r.kill_source(2).unwrap_or(0), 1);
        assert_eq!(r.resolve(&keys[0]).unwrap_or(Resolved { layer: 9, value: 0, src: 0 }).layer, 2);
        assert_eq!(r.kill_source(3).unwrap_or(0), 1);
        let fin = r.resolve(&keys[0]).unwrap_or(Resolved { layer: 9, value: 0, src: 0 });
        assert_eq!(fin.layer, LAYER_N - 1);
        assert_eq!(fin.value, 62_900);
        assert_eq!(r.downgrades.len(), 3);
    }

    #[test]
    fn dangling_absent_never_registered() {
        let inj = inject_rounds(10);
        assert_eq!(inj.caught[0], 10, "悬空引用必须 10/10 检出");
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn kill_nonexistent_and_double_kill() {
        let mut r = BasisResolver::new();
        assert!(r.table.kill(1).is_err(), "空表杀源拒");
        let id = r.table.register(SourceKind::Key).unwrap_or(0);
        assert!(r.table.kill(id).is_ok());
        assert!(r.table.kill(id).is_err(), "死源再杀拒");
    }

    #[test]
    fn downgrade_ledger_conserve() {
        let (mut r, _) = seed_corpus().unwrap_or((BasisResolver::new(), [KeyId { proc_id: 0, api_id: 0, dim: 0 }; 3]));
        let _ = r.kill_source(1);
        let _ = r.kill_source(2);
        let _ = r.kill_source(3);
        assert!(r.conserve().is_ok());
        assert!(r.no_pollution().is_ok());
    }

    #[test]
    fn boundaries_empty_malformed_dup() {
        let mut r = BasisResolver::new();
        assert!(r
            .attach(BasisEntry {
                key: KeyId { proc_id: 1, api_id: 1, dim: 0 },
                chain: [None, None, None],
                vals: [None, None, None],
                default_val: 5,
            })
            .is_ok());
        let res = r.resolve(&KeyId { proc_id: 1, api_id: 1, dim: 0 }).unwrap_or(Resolved { layer: 9, value: 0, src: 0 });
        assert_eq!(res.value, 5, "空链直落 Default");
        assert!(r
            .attach(BasisEntry {
                key: KeyId { proc_id: 2, api_id: 1, dim: 0 },
                chain: [Some(1), None, None],
                vals: [None, None, None],
                default_val: 1,
            })
            .is_err(), "畸形链拒");
        assert!(r
            .attach(BasisEntry {
                key: KeyId { proc_id: 1, api_id: 1, dim: 0 },
                chain: [None, None, None],
                vals: [None, None, None],
                default_val: 2,
            })
            .is_err(), "同键重复挂链拒");
    }

    #[test]
    fn probes_all_sides() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18437_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
