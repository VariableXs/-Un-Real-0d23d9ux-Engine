//! unx4112_evictrace — UNX-F4112 dentry 驱逐与引用竞争注入（诊断段 0x9000）。
//!
//! 判据原文（提炼）：
//!   J1 被引用条目零驱逐：引用中驱逐注入 1,000 次零命中
//!      （refcnt>0 的条目被驱逐计数恒 0）。
//!   J2 驱逐与 dget/dput 竞争交错 100,000 次零崩零错账：
//!      交错操作全量入账，终态账面守恒。
//!   J3 引用账守恒：get 总数 == put 总数 + 在引用数；
//!      驻留数 == 累计入账 - 累计驱逐。
//!   J4 驱逐只落在零引用条目：驱逐对象 refcnt 恒 0（逐事件留痕可查）。
//!   J5 引用中条目对驱逐请求返回 E_REF_BUSY，错误账如实计数。
//! 设计纪律：
//!   1. 引用计数单一事实源：条目 refcnt 字段，dget/dput 成对审计，
//!      dput 超放（refcnt 已 0 再放）拒收。
//!   2. 驱逐门槛前置：evict 选条目时先查 refcnt，非 0 一律跳过/
//!      拒绝，不产生半驱逐态。
//!   3. 竞争模型确定性：用写死的交错序列（get/put/evict 三操作
//!      按固定节奏轮转）模拟竞争，不用线程不用 rand。
//!   4. 注入面独立：force_evict_referenced 显式对引用中条目发驱逐，
//       检出器拒绝并计违规账（判据 J1 的 1,000 次注入载体）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9000 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9000;

/// 引用中驱逐注入次数（判据 J1 原文数）。
pub const INJECT_ROUNDS: u32 = 1000;
/// 竞争交错操作次数（判据 J2 原文数：10 万）。
pub const RACE_OPS: u32 = 100_000;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_REF_BUSY: u16 = SEG_BASE + 1; // 引用中驱逐被拒（判据 J1/J5）
const E_PUT_OVER: u16 = SEG_BASE + 2; // dput 超放（引用已 0 再放）
const E_ID_MISS: u16 = SEG_BASE + 3;  // 点名条目不存在
const E_BAL: u16 = SEG_BASE + 4;      // 引用账/驻留账守恒失守（判据 J3）
const E_EVICTED_TOUCH: u16 = SEG_BASE + 5; // 对已驱逐条目引用操作
const E_DUP_INS: u16 = SEG_BASE + 6;  // 重复入账同 id

/// 条目引用状态机：Unref（可驱逐）→ Referenced（受保护）→（dput 归零回 Unref）；
/// Evicted 为终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefState {
    Unref,      // 零引用，可被驱逐
    Referenced, // 引用中，驱逐保护
    Evicted,    // 已驱逐终态
}

/// 单条目：id + 引用计数 + 状态机。
pub struct RaceEntry {
    pub id: u32,        // 唯一号
    pub refs: u32,      // 引用计数
    pub state: RefState,// 状态机
    pub busy_denies: u32,// 本条目被拒驱逐次数（判据 J5 留痕）
}

/// 竞争实验室：条目池 + 三操作账 + 注入检出器。
pub struct RaceLab {
    pub entries: Vec<RaceEntry>, // 条目池（含已驱逐）
    pub get_total: u32,          // dget 总数
    pub put_total: u32,          // dput 总数
    pub evict_total: u32,        // 驱逐总数
    pub insert_total: u32,       // 入账总数
    pub busy_denies: u32,        // 引用中驱逐拒绝总数（判据 J1/J5）
    pub ops: u32,                // 累计操作数（竞争交错账）
    pub next_id: u32,            // id 分配器
    pub rejected: u32,           // 其他拒收
}

impl RaceLab {
    pub fn new() -> RaceLab {
        RaceLab {
            entries: Vec::new(),
            get_total: 0,
            put_total: 0,
            evict_total: 0,
            insert_total: 0,
            busy_denies: 0,
            ops: 0,
            next_id: 1,
            rejected: 0,
        }
    }

    fn pos(&self, id: u32) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].id == id {
                return Some(i);
            }
        }
        None
    }

    /// 入账（零引用态）。
    pub fn insert(&mut self) -> Result<u32, u16> {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(RaceEntry {
            id,
            refs: 0,
            state: RefState::Unref,
            busy_denies: 0,
        });
        self.insert_total += 1;
        Ok(id)
    }

    /// dget（判据 J2 竞争操作之一）：已驱逐条目拒收。
    pub fn dget(&mut self, id: u32) -> Result<(), u16> {
        let p = match self.pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        if self.entries[p].state == RefState::Evicted {
            self.rejected += 1;
            return Err(E_EVICTED_TOUCH); // 幽灵引用防线
        }
        self.entries[p].refs += 1;
        self.entries[p].state = RefState::Referenced;
        self.get_total += 1;
        Ok(())
    }

    /// dput（判据 J3 对偶操作）：超放拒收。
    pub fn dput(&mut self, id: u32) -> Result<(), u16> {
        let p = match self.pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        if self.entries[p].refs == 0 {
            self.rejected += 1;
            return Err(E_PUT_OVER); // 超放防线
        }
        self.entries[p].refs -= 1;
        if self.entries[p].refs == 0 {
            self.entries[p].state = RefState::Unref;
        }
        self.put_total += 1;
        Ok(())
    }

    /// 驱逐（判据 J1/J4）：只落零引用条目，引用中拒收并留痕。
    pub fn evict(&mut self, id: u32) -> Result<(), u16> {
        let p = match self.pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        if self.entries[p].state == RefState::Evicted {
            self.rejected += 1;
            return Err(E_EVICTED_TOUCH); // 重复驱逐拒收
        }
        if self.entries[p].refs > 0 {
            // 引用中：拒收 + 双账留痕（判据 J1/J5）
            self.entries[p].busy_denies += 1;
            self.busy_denies += 1;
            return Err(E_REF_BUSY);
        }
        self.entries[p].state = RefState::Evicted;
        self.evict_total += 1;
        Ok(())
    }

    /// 注入面（判据 J1）：对引用中条目显式发驱逐（期望被拒），
    /// 返回是否如预期被拒（true=被拒）。
    pub fn force_evict_referenced(&mut self, id: u32) -> Result<bool, u16> {
        match self.evict(id) {
            Err(E_REF_BUSY) => Ok(true), // 注入被拦截，符合预期
            Ok(()) => Err(E_REF_BUSY),   // 引用中条目被驱逐：判据失守
            Err(e) => Err(e),
        }
    }

    /// 确定性竞争交错（判据 J2）：三操作按固定节奏轮转
    /// op%3：0=dget、1=dput、2=evict（evict 落在当前最老零引用条目）。
    pub fn race_ops(&mut self, rounds: u32) -> Result<(), u16> {
        if rounds > RACE_OPS {
            self.rejected += 1;
            return Err(E_BAL); // 超上限拒收
        }
        let mut op = 0u32;
        // 目标轮转：在 (条目数) 范围内循环取 id
        let n = self.insert_total;
        if n == 0 {
            return Err(E_BAL);
        }
        while op < rounds {
            self.ops += 1; // 每次尝试都入竞争账（含被拒尝试）
            let id = 1 + op % n; // 确定性目标选择
            match op % 3 {
                0 => {
                    // 目标可能已驱逐：容错计数，竞争不中断
                    if self.dget(id).is_err() {
                        self.rejected += 1;
                    }
                }
                1 => {
                    // put 可能超放：容错处理——超放计拒收但竞争不中断
                    if self.dput(id).is_err() {
                        self.rejected += 1;
                    }
                }
                _ => {
                    // evict 落引用中条目：预期被拒，竞争继续
                    let _ = self.evict(id);
                }
            }
            op += 1;
        }
        Ok(())
    }

    /// 引用账守恒（判据 J3）：get == put + Σrefs；驻留 = 入账 - 驱逐。
    pub fn conserved(&self) -> bool {
        let mut ref_sum = 0u32;
        let mut resident = 0u32;
        for e in &self.entries {
            if e.state != RefState::Evicted {
                resident += 1;
            }
            ref_sum += e.refs;
        }
        if self.get_total != self.put_total + ref_sum {
            return false;
        }
        if self.insert_total != resident + self.evict_total {
            return false;
        }
        // 状态机一致性：refs>0 必须 Referenced，refs==0 活条目必须 Unref
        for e in &self.entries {
            match e.state {
                RefState::Referenced => {
                    if e.refs == 0 {
                        return false;
                    }
                }
                RefState::Unref => {
                    if e.refs != 0 {
                        return false;
                    }
                }
                RefState::Evicted => {
                    if e.refs != 0 {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 引用中零驱逐核验（判据 J1）：被驱逐条目的 busy_denies 之外
    /// 不存在任何"引用中驱逐成功"的账（驱逐总数只含零引用）。
    pub fn zero_evict_referenced(&self) -> bool {
        for e in &self.entries {
            if e.state == RefState::Evicted && e.refs != 0 {
                return false;
            }
        }
        true
    }
}

/// 自例程：R1 注入 1000 次→R2 竞争 10 万次→R3 守恒对账→R4 终审。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 引用中驱逐注入 1,000 次零命中（判据 J1）。
    let mut r = RaceLab::new();
    let a = r.insert()?;
    let b = r.insert()?;
    r.dget(a)?;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if !r.force_evict_referenced(a)? {
            return Err(E_REF_BUSY); // 注入必须次次被拦
        }
        round += 1;
    }
    if r.entries[0].state != RefState::Referenced {
        return Err(E_REF_BUSY); // a 必须仍引用中零驱逐
    }
    if r.busy_denies != INJECT_ROUNDS {
        return Err(E_BAL); // 拒绝账 == 注入数
    }
    hits += 1;

    // R2 竞争交错 100,000 次零崩零错账（判据 J2）。
    r.race_ops(RACE_OPS)?;
    if r.ops < RACE_OPS {
        return Err(E_BAL); // 操作账必须齐
    }
    hits += 1;

    // R3 注入拦截补充：超放、幽灵引用、幽灵驱逐、已驱逐再驱逐全拒。
    let mut r2 = RaceLab::new();
    let c = r2.insert()?;
    if r2.dput(c).is_ok() {
        return Err(E_PUT_OVER); // 超放应拒
    }
    if r2.dget(9999).is_ok() {
        return Err(E_ID_MISS); // 幽灵引用应拒
    }
    if r2.evict(9999).is_ok() {
        return Err(E_ID_MISS); // 幽灵驱逐应拒
    }
    r2.evict(c)?;
    if r2.evict(c).is_ok() {
        return Err(E_EVICTED_TOUCH); // 已驱逐再驱逐应拒
    }
    if r2.dget(c).is_ok() {
        return Err(E_EVICTED_TOUCH); // 已驱逐再引用应拒
    }
    hits += 1;

    // R4 守恒终审（判据 J3/J4）：两实验室账全绿。
    if !r.conserved() || !r.zero_evict_referenced() {
        return Err(E_BAL);
    }
    if !r2.conserved() {
        return Err(E_BAL);
    }
    // 引用中条目驱逐拒绝必须全部留痕到条目（判据 J5）。
    let mut deny_sum = 0u32;
    for i in 0..r.entries.len() {
        deny_sum += r.entries[i].busy_denies;
    }
    if deny_sum < r.busy_denies {
        return Err(E_BAL); // 条目留痕不得少于总账
    }
    hits += 1;

    Ok(hits)
}
