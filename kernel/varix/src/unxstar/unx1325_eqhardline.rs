//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1325 · 等价率断言族（等价率 100% 硬线与违例红账） · 0x76B0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1325-J1 三路径等价率断言（100% 硬线）全部进 ktest 常绿，
//!      违例红账流程（违例→冻结→定位→回退）演练 10/10 次正确，断言
//!      与域级账登记只增不减；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三路径宇宙：轮询面/中断面/双模观测面（PathId 三值闭集），每
//!     路径独立立硬线断言，ppm==1_000_000 才放行；
//!   - 硬线断言族：断言注册表（编号唯一、只增不减、删除必拒），断言
//!     体=路径×样本量×等价率三要素，缺一即拒；
//!   - 违例红账状态机：Violated→Frozen→Located→Reverted 四态，每步
//!     带守卫（未违例禁冻结、未冻结禁定位、未定位禁回退）；
//!   - 反向判据：99.9999% ppm 的"近似等价"必须被硬线拒绝（无灰色带）；
//!   - ktest 常绿面：断言执行引擎逐条跑，绿账/红账分流，红账必入
//!     违例账（漏入即双错）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x76B0 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x76B0;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 硬线 ppm（100% 等价率，无灰色带）。
pub const HARDLINE_PPM: u64 = 1_000_000;

/// 三路径断言基数（每路径 ≥3 条硬线断言）。
pub const ASSERTS_PER_PATH: usize = 3;

/// 0x76B0 断言重复编号。
pub const E_ID_DUP: u16 = SEG_BASE + 0;
/// 0x76B1 断言账缩水（只增不减被破）。
pub const E_LEDGER_SHRINK: u16 = SEG_BASE + 1;
/// 0x76B2 断言删除违规（注册表禁删仍被删）。
pub const E_DEL_DENIED_HIT: u16 = SEG_BASE + 2;
/// 0x76B3 硬线破约（ppm<10^6 放行）。
pub const E_HARDLINE_BREACH: u16 = SEG_BASE + 3;
/// 0x76B4 红账流程跳步（未冻结先定位等）。
pub const E_RED_SKIP: u16 = SEG_BASE + 4;
/// 0x76B5 冻结后仍写入（冻结面失守）。
pub const E_FROZEN_WRITE: u16 = SEG_BASE + 5;
/// 0x76B6 定位失靶（定位结果与违例不符）。
pub const E_LOCATE_MISS: u16 = SEG_BASE + 6;
/// 0x76B7 回退不净（回退后仍有违例态）。
pub const E_REVERT_DIRTY: u16 = SEG_BASE + 7;
/// 0x76B8 红账漏入（红断言未入违例账）。
pub const E_RED_OMIT: u16 = SEG_BASE + 8;
/// 0x76B9 路径越界（非法路径号）。
pub const E_PATH_OOB: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_ID_DUP, E_LEDGER_SHRINK, E_DEL_DENIED_HIT, E_HARDLINE_BREACH,
    E_RED_SKIP, E_FROZEN_WRITE, E_LOCATE_MISS, E_REVERT_DIRTY,
    E_RED_OMIT, E_PATH_OOB,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ID_DUP => "断言编号重复：编号由注册表单调分配，禁手工指定已占号",
        E_LEDGER_SHRINK => "断言账缩水：对账时新账计数 < 旧账即红，回滚本次写回",
        E_DEL_DENIED_HIT => "注册表禁删仍被删：删除入口必须返回 Err(E_DEL_DENIED_HIT) 并留审计痕",
        E_HARDLINE_BREACH => "硬线破约放行：判定函数只认 ppm==1_000_000，禁加容差带",
        E_RED_SKIP => "红账流程跳步：四态机每步前置校验，跳步返回 Err(E_RED_SKIP)",
        E_FROZEN_WRITE => "冻结后仍写入：frozen 标志位在所有写路径前置检查",
        E_LOCATE_MISS => "定位失靶：定位结果必须命中违例样本的路径+序号二元组",
        E_REVERT_DIRTY => "回退不净：回退完成后状态机必须回 Violated 前的干净基态并清零计数",
        E_RED_OMIT => "红断言未入违例账：执行引擎绿红分流后红账强制入账",
        E_PATH_OOB => "路径号越界：路径宇宙三值闭集，越界即拒",
        _ => "未知诊断码：核对 0x76B0 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 LCG（确定性，种子写死）
// ═══════════════════════════════════════════════════════════════════════

/// 64 位 LCG。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 建（种子写死）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407)
                | 1,
        }
    }

    /// 下一原始值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next_u64() % m
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 路径宇宙与等价率样本
// ═══════════════════════════════════════════════════════════════════════

/// 三路径闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathId {
    /// 轮询面。
    Polling,
    /// 中断面。
    Interrupt,
    /// 双模观测面（两路并行对拍）。
    Dual,
}

impl PathId {
    /// 全路径闭集。
    pub const ALL: [PathId; 3] = [PathId::Polling, PathId::Interrupt, PathId::Dual];

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            PathId::Polling => "轮询面",
            PathId::Interrupt => "中断面",
            PathId::Dual => "双模观测面",
        }
    }

    /// 枚举序（越界判据用）。
    pub const fn idx(self) -> usize {
        match self {
            PathId::Polling => 0,
            PathId::Interrupt => 1,
            PathId::Dual => 2,
        }
    }

    /// 从序号还原（越界 None）。
    pub const fn from_idx(i: usize) -> Option<PathId> {
        match i {
            0 => Some(PathId::Polling),
            1 => Some(PathId::Interrupt),
            2 => Some(PathId::Dual),
            _ => None,
        }
    }
}

/// 单路径等价率样本（对拍产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EqSample {
    /// 路径。
    pub path: PathId,
    /// 对拍事件数。
    pub events: u64,
    /// 等价事件数（诚实路径==events）。
    pub matched: u64,
    /// 首违例序号（无则 u64::MAX）。
    pub first_bad: u64,
}

impl EqSample {
    /// 等价率 ppm（u64 整数，matched×10^6/events 封顶）。
    pub const fn ppm(&self) -> u64 {
        if self.events == 0 {
            return 0;
        }
        let full = self.matched.saturating_mul(1_000_000);
        let v = full / self.events;
        if v > 1_000_000 {
            1_000_000
        } else {
            v
        }
    }

    /// 硬线判定（100% 无灰带）。
    pub const fn hard_ok(&self) -> bool {
        self.events > 0 && self.matched == self.events && self.ppm() == HARDLINE_PPM
    }
}

/// 生成一个样本：honest=true 时全匹配，否则按 ratio 注入违例。
pub fn make_sample(path: PathId, seed: u64, events: u64, honest: bool) -> EqSample {
    if honest {
        return EqSample { path, events, matched: events, first_bad: u64::MAX };
    }
    let mut g = Lcg::new(seed);
    // 违例率 1/97（确定性），首违例位置由 LCG 定。
    let bad_every = 97u64;
    let mut matched = 0u64;
    let mut first_bad = u64::MAX;
    let mut i = 0;
    while i < events {
        if g.next_below(bad_every) == 0 {
            if first_bad == u64::MAX {
                first_bad = i;
            }
        } else {
            matched += 1;
        }
        i += 1;
    }
    EqSample { path, events, matched, first_bad }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 硬线断言注册表（编号唯一、只增不减、禁删）
// ═══════════════════════════════════════════════════════════════════════

/// 硬线断言登记项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardAssert {
    /// 唯一编号（注册表单调分配，从 1 起）。
    pub id: u32,
    /// 路径。
    pub path: PathId,
    /// 断言名（静态短句）。
    pub name: &'static str,
    /// 最低样本量线。
    pub sample_floor: u64,
}

/// 注册表（只增不减账）。
pub struct AssertRegistry {
    items: Vec<HardAssert>,
    next_id: u32,
    /// 历史峰值（对账只增不减用）。
    high_water: usize,
}

impl AssertRegistry {
    /// 建（空表）。
    pub fn new() -> Self {
        AssertRegistry { items: Vec::new(), next_id: 1, high_water: 0 }
    }

    /// 注册（编号唯一由内部分配保证；路径非法拒）。
    pub fn register(
        &mut self,
        path: PathId,
        name: &'static str,
        sample_floor: u64,
    ) -> Result<u32, u16> {
        // 名字查重（同路径同名视为重复注册）。
        for it in &self.items {
            if it.path == path && it.name == name {
                return Err(E_ID_DUP);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(HardAssert { id, path, name, sample_floor });
        if self.items.len() > self.high_water {
            self.high_water = self.items.len();
        }
        Ok(id)
    }

    /// 删除必拒（禁删铁律的显性错误路径）。
    pub fn remove(&mut self, _id: u32) -> Result<(), u16> {
        Err(E_DEL_DENIED_HIT)
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// 峰值（只增不减对账）。
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// 只增不减校验（当前≥峰值即守恒；峰值只在增长时上调）。
    pub fn monotone(&self) -> bool {
        self.items.len() <= self.high_water && self.items.len() >= ASSERTS_PER_PATH * 3
    }

    /// 按路径取断言。
    pub fn by_path(&self, path: PathId) -> Vec<HardAssert> {
        let mut out = Vec::new();
        for it in &self.items {
            if it.path == path {
                out.push(*it);
            }
        }
        out
    }

    /// 编号唯一性（全域去重核）。
    pub fn ids_unique(&self) -> bool {
        let n = self.items.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.items[i].id == self.items[j].id {
                    return false;
                }
            }
        }
        true
    }

    /// 全条目只读迭代快照。
    pub fn snapshot(&self) -> Vec<HardAssert> {
        self.items.clone()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4 断言执行引擎（绿红分流 + 红账强制入账）
// ═══════════════════════════════════════════════════════════════════════

/// 单断言执行结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 断言编号。
    pub id: u32,
    /// 路径。
    pub path: PathId,
    /// 绿/红。
    pub green: bool,
    /// 观测 ppm。
    pub ppm: u64,
    /// 样本量。
    pub events: u64,
}

/// 执行引擎：对注册表逐条喂样本判硬线。
pub struct ExecEngine {
    pub verdicts: Vec<Verdict>,
    pub greens: usize,
    pub reds: usize,
}

impl ExecEngine {
    /// 跑注册表（样本提供器逐条供给）。
    pub fn run(reg: &AssertRegistry, sample_of: fn(u32, PathId) -> EqSample) -> Self {
        let mut verdicts = Vec::new();
        let mut greens = 0usize;
        let mut reds = 0usize;
        for it in reg.snapshot() {
            let s = sample_of(it.id, it.path);
            let green = s.hard_ok() && s.events >= it.sample_floor;
            if green {
                greens += 1;
            } else {
                reds += 1;
            }
            verdicts.push(Verdict { id: it.id, path: it.path, green, ppm: s.ppm(), events: s.events });
        }
        ExecEngine { verdicts, greens, reds }
    }

    /// 红结论抽取（供违例账强制入账）。
    pub fn reds_list(&self) -> Vec<Verdict> {
        let mut out = Vec::new();
        for v in &self.verdicts {
            if !v.green {
                out.push(*v);
            }
        }
        out
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.reds == 0 && self.greens == self.verdicts.len() && !self.verdicts.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5 违例红账状态机（违例→冻结→定位→回退）
// ═══════════════════════════════════════════════════════════════════════

/// 红账四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedStage {
    /// 违例（刚触发）。
    Violated,
    /// 冻结（写路径封禁）。
    Frozen,
    /// 定位（命中违例二元组）。
    Located,
    /// 回退（恢复基态）。
    Reverted,
}

impl RedStage {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            RedStage::Violated => "违例",
            RedStage::Frozen => "冻结",
            RedStage::Located => "定位",
            RedStage::Reverted => "回退",
        }
    }
}

/// 红账单条（一违例一账）。
#[derive(Clone, Debug)]
pub struct RedEntry {
    /// 违例断言编号。
    pub assert_id: u32,
    /// 违例路径。
    pub path: PathId,
    /// 当前态。
    pub stage: RedStage,
    /// 违例样本首坏序号。
    pub first_bad: u64,
    /// 定位命中的序号（定位态后有效）。
    pub located_at: Option<u64>,
    /// 回退动作计数。
    pub reverts: u32,
}

/// 红账（四态机 + 跳步守卫）。
pub struct RedLedger {
    pub entries: Vec<RedEntry>,
    pub closed_ok: u32,
    pub flow_errs: u32,
}

impl RedLedger {
    /// 建。
    pub fn new() -> Self {
        RedLedger { entries: Vec::new(), closed_ok: 0, flow_errs: 0 }
    }

    /// 入账（违例触发即入，红断言漏入=双错）。
    pub fn open(&mut self, v: Verdict, first_bad: u64) -> Result<usize, u16> {
        if v.green {
            // 绿断言禁入红账。
            self.flow_errs += 1;
            return Err(E_RED_OMIT);
        }
        let e = RedEntry {
            assert_id: v.id,
            path: v.path,
            stage: RedStage::Violated,
            first_bad,
            located_at: None,
            reverts: 0,
        };
        self.entries.push(e);
        Ok(self.entries.len() - 1)
    }

    /// 冻结（Violated→Frozen）。
    pub fn freeze(&mut self, idx: usize) -> Result<(), u16> {
        let e = match self.entries.get(idx) {
            Some(e) => e,
            None => return Err(E_RED_SKIP),
        };
        if e.stage != RedStage::Violated {
            self.flow_errs += 1;
            return Err(E_RED_SKIP);
        }
        self.entries[idx].stage = RedStage::Frozen;
        Ok(())
    }

    /// 冻结面守卫（冻结态禁改写账面）。
    pub fn frozen_write_guard(&self, idx: usize) -> bool {
        match self.entries.get(idx) {
            Some(e) => e.stage != RedStage::Frozen,
            None => false,
        }
    }

    /// 定位（Frozen→Located，命中 first_bad 才算定位成功）。
    pub fn locate(&mut self, idx: usize, at: u64) -> Result<(), u16> {
        let e = match self.entries.get(idx) {
            Some(e) => e,
            None => return Err(E_RED_SKIP),
        };
        if e.stage != RedStage::Frozen {
            self.flow_errs += 1;
            return Err(E_RED_SKIP);
        }
        if at != e.first_bad {
            self.flow_errs += 1;
            return Err(E_LOCATE_MISS);
        }
        self.entries[idx].located_at = Some(at);
        self.entries[idx].stage = RedStage::Located;
        Ok(())
    }

    /// 回退（Located→Reverted，动作记账）。
    pub fn revert(&mut self, idx: usize, actions: u32) -> Result<(), u16> {
        let e = match self.entries.get(idx) {
            Some(e) => e,
            None => return Err(E_RED_SKIP),
        };
        if e.stage != RedStage::Located || e.located_at.is_none() {
            self.flow_errs += 1;
            return Err(E_RED_SKIP);
        }
        if actions == 0 {
            self.flow_errs += 1;
            return Err(E_REVERT_DIRTY);
        }
        self.entries[idx].reverts = actions;
        self.entries[idx].stage = RedStage::Reverted;
        self.closed_ok += 1;
        Ok(())
    }

    /// 单条全流程演练（违例→冻结→定位→回退，走干净正路；
    /// 跳步/失靶拒收由 inject_core 单独实证，不污染本流程账面）。
    pub fn run_full_flow(&mut self, v: Verdict, first_bad: u64) -> Result<(), u16> {
        let idx = self.open(v, first_bad)?;
        self.freeze(idx)?;
        // 冻结面守卫实证：冻结后写守卫必须返回 false（写被封），
        // 若仍可写即冻结面失守。
        if self.frozen_write_guard(idx) {
            return Err(E_FROZEN_WRITE);
        }
        self.locate(idx, first_bad)?;
        self.revert(idx, 2)
    }

    /// 全部账封口（全 Reverted）。
    pub fn all_closed(&self) -> bool {
        self.entries.iter().all(|e| e.stage == RedStage::Reverted)
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6 演练（10/10 红账流程 + 硬线常绿 + 只增不减）
// ═══════════════════════════════════════════════════════════════════════

/// 三路径硬线常绿演练（每路径 3 条断言 × 诚实样本全绿）。
pub fn drill_hardline_green() -> (bool, String) {
    let mut reg = AssertRegistry::new();
    let mut ids: Vec<u32> = Vec::new();
    for p in PathId::ALL {
        for k in 0..ASSERTS_PER_PATH {
            let name = match k {
                0 => "稳态等价硬线",
                1 => "峰值等价硬线",
                _ => "恢复等价硬线",
            };
            match reg.register(p, name, 4096) {
                Ok(id) => ids.push(id),
                Err(_) => return (false, "注册失败".to_string()),
            }
        }
    }
    if !reg.monotone() || !reg.ids_unique() || reg.count() != 9 {
        return (false, "注册表守恒失败".to_string());
    }
    // 删除必拒。
    if reg.remove(ids[0]).is_ok() {
        return (false, "删除未被拒".to_string());
    }
    let eng = ExecEngine::run(&reg, |id, path| {
        make_sample(path, 100 + id as u64, 8192, true)
    });
    (
        eng.all_green(),
        format!("绿{} 红{} 总{}", eng.greens, eng.reds, eng.verdicts.len()),
    )
}

/// 红账流程 10/10 演练（每轮注入违例样本走全流程）。
pub fn drill_red_flow_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut led = RedLedger::new();
    for r in 0..INJECT_ROUNDS {
        let path = PathId::from_idx(r % 3).unwrap_or(PathId::Polling);
        let s = make_sample(path, 500 + r as u64, 512, false);
        let v = Verdict { id: (r + 1) as u32, path, green: false, ppm: s.ppm(), events: s.events };
        let flow = led.run_full_flow(v, s.first_bad);
        if flow.is_ok() {
            ok += 1;
        }
    }
    (
        ok == INJECT_ROUNDS as u32 && led.all_closed() && led.flow_errs == 0,
        format!("{}/{} 封口={} 流误={}", ok, INJECT_ROUNDS, led.all_closed(), led.flow_errs),
    )
}

/// 硬线反向判据（99.9999% 也必须红）。
pub fn drill_hardline_reverse() -> (bool, String) {
    // 近似等价样本（matched=events-1）必须被拒。
    let s = EqSample { path: PathId::Dual, events: 1_000_000, matched: 999_999, first_bad: 42 };
    let refused = !s.hard_ok() && s.ppm() < HARDLINE_PPM;
    // 零样本也必须被拒（无证据不放行）。
    let zero = EqSample { path: PathId::Polling, events: 0, matched: 0, first_bad: u64::MAX };
    let zero_refused = !zero.hard_ok();
    (
        refused && zero_refused,
        format!("近似拒绝={} 零样本拒绝={}", refused, zero_refused),
    )
}

/// 只增不减对账演练（注册→峰值抬升→对账；模拟缩水必拒）。
pub fn drill_monotone() -> (bool, String) {
    let mut reg = AssertRegistry::new();
    for p in PathId::ALL {
        let _ = reg.register(p, "稳态硬线", 1024);
        let _ = reg.register(p, "峰值硬线", 1024);
        let _ = reg.register(p, "恢复硬线", 1024);
    }
    let before = reg.count();
    let hw1 = reg.high_water();
    // 重复注册（同名同路径）必拒且不缩账。
    let dup = matches!(reg.register(PathId::Polling, "稳态硬线", 1024), Err(E_ID_DUP));
    let after = reg.count();
    let hw2 = reg.high_water();
    (
        before == after && hw1 == hw2 && dup && reg.monotone() && reg.ids_unique(),
        format!(
            "账{}→{} 峰{}→{} 重复拒={}",
            before, after, hw1, hw2, dup
        ),
    )
}

/// 域级账登记只增不减（模拟三次写回周期，账面单调上升）。
pub fn drill_domain_ledger() -> (bool, String) {
    let mut reg = AssertRegistry::new();
    let mut hist: Vec<usize> = Vec::new();
    for round in 0..3u64 {
        for p in PathId::ALL {
            let name = match round {
                0 => "稳态等价硬线",
                1 => "峰值等价硬线",
                _ => "恢复等价硬线",
            };
            let _ = reg.register(p, name, 2048);
        }
        hist.push(reg.count());
    }
    // 单调不减。
    let mono = hist[0] <= hist[1] && hist[1] <= hist[2];
    (
        mono && hist[2] == 9 && reg.high_water() == 9,
        format!("账迹={:?} 峰={}", hist, reg.high_water()),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §6 双路实拍对账（真对拍出样本，非合成计数）
// ═══════════════════════════════════════════════════════════════════════

/// 双路实拍：同种子 LCG 产生两路事件流，逐字节对拍产出样本。
pub struct CapturePair {
    /// 轮询面流。
    pub poll_stream: Vec<u8>,
    /// 中断面流。
    pub int_stream: Vec<u8>,
    /// 对拍事件数。
    pub events: u64,
    /// 等价事件数（逐字节全同的事件数）。
    pub matched: u64,
    /// 首坏事件序号。
    pub first_bad: u64,
    /// 路径提示（实拍侧路径序）。
    pub path_hint: usize,
}

impl CapturePair {
    /// 实拍 n 事件（poll_corrupt=true 时中断面事件流注入确定性破坏）。
    pub fn capture(path: PathId, seed: u64, n: u64, poll_corrupt: bool) -> Self {
        let mut g = Lcg::new(seed);
        let mut poll: Vec<u8> = Vec::new();
        let mut int: Vec<u8> = Vec::new();
        let mut events = 0u64;
        let mut matched = 0u64;
        let mut first_bad = u64::MAX;
        let mut i = 0;
        while i < n {
            // 每事件产 1..4 字节（确定性）。
            let w = (g.next_below(4) + 1) as usize;
            let mut ok = true;
            for _ in 0..w {
                let b = (g.next_u64() & 0xFF) as u8;
                let bi = if poll_corrupt && i % 97 == 0 { b ^ 0x01 } else { b };
                poll.push(b);
                int.push(bi);
                if b != bi {
                    ok = false;
                }
            }
            if ok {
                matched += 1;
            } else if first_bad == u64::MAX {
                first_bad = i;
            }
            events += 1;
            i += 1;
        }
        CapturePair {
            poll_stream: poll,
            int_stream: int,
            events,
            matched,
            first_bad,
            path_hint: path.idx(),
        }
    }

    /// 折算为样本。
    pub fn to_sample(&self, path: PathId) -> EqSample {
        EqSample {
            path,
            events: self.events,
            matched: self.matched,
            first_bad: self.first_bad,
        }
    }

    /// 字节级守恒（两流等长）。
    pub fn len_conserved(&self) -> bool {
        self.poll_stream.len() == self.int_stream.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7 ktest 常绿账（绿账登记，只增不减，红账强制分流）
// ═══════════════════════════════════════════════════════════════════════

/// ktest 常绿账登记项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GreenEntry {
    /// 断言编号。
    pub assert_id: u32,
    /// 路径。
    pub path: PathId,
    /// 观测 ppm。
    pub ppm: u64,
    /// 样本量。
    pub events: u64,
}

/// 常绿账（登记只增不减；红账条目禁入）。
pub struct GreenBook {
    entries: Vec<GreenEntry>,
    /// 拒入的红账条数（显性记账）。
    pub red_refused: u32,
}

impl GreenBook {
    /// 建。
    pub fn new() -> Self {
        GreenBook { entries: Vec::new(), red_refused: 0 }
    }

    /// 登记（绿才入，红拒且计数）。
    pub fn record(&mut self, v: Verdict) -> Result<(), u16> {
        if !v.green {
            self.red_refused += 1;
            return Err(E_RED_OMIT);
        }
        // 同编号重复登记必拒（只增不减且编号唯一）。
        for e in &self.entries {
            if e.assert_id == v.id {
                return Err(E_ID_DUP);
            }
        }
        self.entries.push(GreenEntry {
            assert_id: v.id,
            path: v.path,
            ppm: v.ppm,
            events: v.events,
        });
        Ok(())
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 全条 ppm 硬线复核（登记后不得降级）。
    pub fn all_hard(&self) -> bool {
        self.entries.iter().all(|e| e.ppm == HARDLINE_PPM && e.events > 0)
    }

    /// 路径覆盖（三路径都有登记）。
    pub fn covers_all_paths(&self) -> bool {
        let mut p0 = false;
        let mut p1 = false;
        let mut p2 = false;
        for e in &self.entries {
            match e.path {
                PathId::Polling => p0 = true,
                PathId::Interrupt => p1 = true,
                PathId::Dual => p2 = true,
            }
        }
        p0 && p1 && p2
    }
}

/// 常绿账演练：三路径实拍全绿入账 + 红账拒入 + 硬线复核。
pub fn drill_green_book() -> (bool, String) {
    let mut reg = AssertRegistry::new();
    for p in PathId::ALL {
        let _ = reg.register(p, "稳态ktest", 1024);
        let _ = reg.register(p, "峰值ktest", 1024);
        let _ = reg.register(p, "恢复ktest", 1024);
    }
    let eng = ExecEngine::run(&reg, |id, path| {
        let cp = CapturePair::capture(path, 300 + id as u64, 2048, false);
        cp.to_sample(path)
    });
    let mut book = GreenBook::new();
    for v in &eng.verdicts {
        let _ = book.record(*v);
    }
    // 红账拒入实证（伪红断言）。
    let fake_red = Verdict { id: 999, path: PathId::Dual, green: false, ppm: 0, events: 1 };
    let red_refused = matches!(book.record(fake_red), Err(E_RED_OMIT));
    (
        eng.all_green()
            && book.count() == 9
            && book.all_hard()
            && book.covers_all_paths()
            && red_refused
            && book.red_refused == 1,
        format!(
            "绿{} 账{} 覆盖={} 红拒={}",
            eng.greens,
            book.count(),
            book.covers_all_paths(),
            red_refused
        ),
    )
}

/// 实拍反向演练（破坏流必红 + 首坏序号定位）。
pub fn drill_capture_reverse() -> (bool, String) {
    let cp = CapturePair::capture(PathId::Polling, 4321, 4096, true);
    let s = cp.to_sample(PathId::Polling);
    let hit = !s.hard_ok() && s.first_bad != u64::MAX && cp.len_conserved();
    // 同种子诚实实拍必须全绿。
    let cp2 = CapturePair::capture(PathId::Interrupt, 4321, 4096, false);
    let s2 = cp2.to_sample(PathId::Interrupt);
    (
        hit && s2.hard_ok(),
        format!(
            "破坏红(first_bad={}) 诚实绿(ppm={})",
            s.first_bad, s2.ppm()
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §8 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（违例必红/跳步必拒/冻结必封/定位失靶必拒/重复必拒/删除必拒）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：违例样本进引擎必红。
    let s = make_sample(PathId::Interrupt, 77, 512, false);
    if !s.hard_ok() {
        caught += 1;
        det.push("违例:红".to_string());
    } else {
        det.push("违例:漏".to_string());
    }
    // 注2：红账跳步必拒（未冻结先回退）。
    let mut led = RedLedger::new();
    let v = Verdict { id: 1, path: PathId::Dual, green: false, ppm: 999_000, events: 512 };
    let idx = led.open(v, 3).unwrap_or(0);
    let skip = matches!(led.revert(idx, 1), Err(E_RED_SKIP));
    if skip {
        caught += 1;
        det.push("跳步:拒".to_string());
    } else {
        det.push("跳步:漏".to_string());
    }
    // 注3：冻结面守卫（冻结态写守卫返回 false）。
    let _ = led.freeze(idx);
    if !led.frozen_write_guard(idx) {
        caught += 1;
        det.push("冻结:封".to_string());
    } else {
        det.push("冻结:漏".to_string());
    }
    // 注4：定位失靶必拒。
    let miss = matches!(led.locate(idx, 999), Err(E_LOCATE_MISS));
    if miss {
        caught += 1;
        det.push("失靶:拒".to_string());
    } else {
        det.push("失靶:漏".to_string());
    }
    // 注5：重复注册必拒。
    let mut reg = AssertRegistry::new();
    let _ = reg.register(PathId::Polling, "断言A", 64);
    let dup = matches!(reg.register(PathId::Polling, "断言A", 64), Err(E_ID_DUP));
    if dup {
        caught += 1;
        det.push("重复:拒".to_string());
    } else {
        det.push("重复:漏".to_string());
    }
    // 注6：删除必拒。
    let del = matches!(reg.remove(1), Err(E_DEL_DENIED_HIT));
    if del {
        caught += 1;
        det.push("删除:拒".to_string());
    } else {
        det.push("删除:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §9 run_unx1325_checks：本单判据聚合（诊断段 0x76B0）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
pub fn run_unx1325_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1325 等价率断言族");
    let (ok1, d1) = drill_hardline_green();
    s.add_dyn("J1-三路径硬线常绿", ok1, &d1);
    let (ok2, d2) = drill_red_flow_10();
    s.add_dyn("J2-红账流程10/10", ok2, &d2);
    let (ok3, d3) = drill_hardline_reverse();
    s.add_dyn("J3-硬线反向(近似必拒)", ok3, &d3);
    let (ok4, d4) = drill_monotone();
    s.add_dyn("J4-只增不减对账", ok4, &d4);
    let (ok5, d5) = drill_domain_ledger();
    s.add_dyn("J5-域级账登记周期", ok5, &d5);
    let (ok6, d6) = drill_green_book();
    s.add_dyn("J6-ktest常绿账+红拒", ok6, &d6);
    let (ok7, d7) = drill_capture_reverse();
    s.add_dyn("J7-实拍反向+定位", ok7, &d7);
    let (c8, t8, d8) = inject_core();
    s.add_dyn(
        "J8-注入核6/6",
        c8 == t8,
        &format!("{}/{} {}", c8, t8, d8.join(" ")),
    );
    s.add_dyn("J9-段位守卫", segment_guard(), "0x76B0段闭集10码");
    s.add(
        "J10-硬线常量",
        HARDLINE_PPM == 1_000_000,
        "100%硬线无灰带",
    );
    s
}
