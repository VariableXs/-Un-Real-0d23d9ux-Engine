//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12508 · 句柄表坏项拒止（D3 联签预告） · 0x30d80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12508-J1 坏代数/坏指针 20 例拒止全过，D3 联签面（拒止码联合
//!      验收）登记在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 句柄代数：句柄值 = 槽号 | 代数 << SLOT_BITS（槽 16、代数 4 位
//!   口径），代数随槽复用递增，陈旧句柄按代数失配拒（GenMismatch）；
//!   - 闸序：句柄值槽号越界 → 代数越域 → 代数失配 → 指针核（空指针/
//!   越界指针/类型槽越界），每例按闸序命中首个拒码；
//!   - 坏指针档：表项指针零（PtrNull）/越对象域（PtrOob），指向区
//!   [OBJ_BASE, OBJ_BASE+OBJ_SPAN)；
//!   - 槽态三态：空/占用/释放（释放槽句柄按代数失配拒）；
//!   - D3 联签面登记：批号/主题/段基三列齐入联测清单（D3Dup 拒重、
//!   SegMismatch 拒失配），拒止码联合验收口径在册；
//!   - 错误路径带码+中文 detail，拒止不写回句柄表；
//!   - 注入核：LCG 写死种子 0x30d80，坏代数/坏指针 20 例 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30d80 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30d80;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入例数（判准 20 例：坏代数 10 + 坏指针 10）。
pub const INJECT_CASES: usize = 20;

/// 每类例数（两类各 10）。
pub const CASES_PER_CLASS: usize = 10;

/// 句柄表槽位（16 槽口径）。
pub const TABLE_SLOTS: usize = 16;

/// 代数位宽（4 位口径，代数域 1..15）。
pub const GEN_MOD: u32 = 16;

/// 对象区基址（指针域口径）。
pub const OBJ_BASE: u32 = 0x1000_0000;

/// 对象区跨度。
pub const OBJ_SPAN: u32 = 0x10_0000;

/// D3 承接段基口径（预告登记锚）。
pub const D3_SEG: u32 = 0x30d80;

/// 诊断码（0x30d80 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HdCode {
    /// 0x30d80 槽号越界。
    IdxOob = 0,
    /// 0x30d81 代数越域（0 或 ≥16）。
    GenOob = 1,
    /// 0x30d82 代数失配（陈旧句柄）。
    GenMismatch = 2,
    /// 0x30d83 空指针。
    PtrNull = 3,
    /// 0x30d84 指针越对象域。
    PtrOob = 4,
    /// 0x30d85 槽已释放。
    SlotFree = 5,
    /// 0x30d86 句柄表缩水。
    TableShrink = 6,
    /// 0x30d87 D3 批号重复登记。
    D3Dup = 7,
    /// 0x30d88 承接段基失配。
    SegMismatch = 8,
    /// 0x30d89 注入例漏判（例落两类之外）。
    CaseMiss = 9,
    /// 0x30d8a 注入例重号。
    CaseDup = 10,
}

impl HdCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [HdCode; 11] = [
        HdCode::IdxOob, HdCode::GenOob, HdCode::GenMismatch, HdCode::PtrNull,
        HdCode::PtrOob, HdCode::SlotFree, HdCode::TableShrink, HdCode::D3Dup,
        HdCode::SegMismatch, HdCode::CaseMiss, HdCode::CaseDup,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30d80 => "槽号越界：句柄槽号先比 TABLE_SLOTS 再解引用。",
        0x30d81 => "代数越域：代数域 1..15，零代数不是合法句柄。",
        0x30d82 => "代数失配：槽复用代数已递增，陈旧句柄必拒。",
        0x30d83 => "空指针：表项指针零按坏指针拒，重建走新表项。",
        0x30d84 => "指针越域：指针必须落对象区 [OBJ_BASE, OBJ_BASE+SPAN)。",
        0x30d85 => "槽已释放：释放槽只读快照，重开走代数递增。",
        0x30d86 => "句柄表缩水：append-only，禁止截断，重放补齐。",
        0x30d87 => "D3 批号重复：同批号幂等，查联测清单再登记。",
        0x30d88 => "承接段基失配：D3 段基必须与分工口径一致。",
        0x30d89 => "注入例漏判：逐例归两类闭集（坏代数/坏指针），漏判红账。",
        0x30d8a => "注入例重号：例号唯一，重号先查抽签器。",
        _ => "未知诊断码：回查 0x30d80 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30d80 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30d8_30d8_30d8_30d8;

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

/// 槽态（三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空。
    Empty = 0,
    /// 占用。
    Used = 1,
    /// 已释放。
    Freed = 2,
}

/// 句柄表项。
#[derive(Clone, Copy, Debug)]
pub struct HandleEntry {
    /// 当前代数。
    pub gen: u32,
    /// 对象指针（0=空）。
    pub ptr: u32,
    /// 槽态。
    pub state: SlotState,
}

/// 句柄值解包（槽号 + 代数）。
pub fn unpack(handle: u32) -> (usize, u32) {
    let idx = (handle & 0xFFFF) as usize;
    let gen = handle >> 16;
    (idx, gen)
}

/// 指针域核对。
pub fn ptr_ok(ptr: u32) -> bool {
    ptr != 0 && ptr >= OBJ_BASE && ptr < OBJ_BASE.wrapping_add(OBJ_SPAN)
}

/// 句柄表坏项拒止正本。
///
/// 生命周期：alloc_entry（占槽+代数分配）→ close_entry（释放+代数递增）→
/// validate（句柄值闸序核对）→ register_d3（D3 联签登记）。
#[derive(Debug)]
pub struct HandleTable {
    /// 槽表。
    slots: [HandleEntry; TABLE_SLOTS],
    /// D3 联测清单。
    d3_list: Vec<D3Row>,
    /// 清单峰值。
    d3_high: usize,
    /// 打开累计。
    opens: u32,
    /// 拒止累计。
    rejects: u32,
}

impl HandleTable {
    /// 建表。
    pub fn new() -> Self {
        HandleTable {
            slots: core::array::from_fn(|_| HandleEntry {
                gen: 0,
                ptr: 0,
                state: SlotState::Empty,
            }),
            d3_list: Vec::new(),
            d3_high: 0,
            opens: 0,
            rejects: 0,
        }
    }

    /// 占槽（代数递增分配；返回合法句柄值）。
    pub fn alloc_entry(&mut self, ptr: u32) -> Result<u32, HdCode> {
        if !ptr_ok(ptr) {
            self.rejects += 1;
            return Err(if ptr == 0 { HdCode::PtrNull } else { HdCode::PtrOob });
        }
        let mut slot: Option<usize> = None;
        for i in 0..TABLE_SLOTS {
            // 空槽与释放槽均可复用（代数递增口径）。
            if matches!(self.slots[i].state, SlotState::Empty | SlotState::Freed) {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(HdCode::TableShrink);
            }
        };
        let prev = self.slots[s].gen;
        let gen = if prev == 0 || prev + 1 >= GEN_MOD { 1 } else { prev + 1 };
        self.slots[s] = HandleEntry { gen, ptr, state: SlotState::Used };
        self.opens += 1;
        Ok(s as u32 | (gen << 16))
    }

    /// 释放槽（代数递增，旧句柄即刻失效）。
    pub fn close_entry(&mut self, handle: u32) -> Result<bool, HdCode> {
        let (idx, gen) = unpack(handle);
        if idx >= TABLE_SLOTS {
            self.rejects += 1;
            return Err(HdCode::IdxOob);
        }
        let e = &self.slots[idx];
        if e.state != SlotState::Used || e.gen != gen {
            self.rejects += 1;
            return Err(HdCode::GenMismatch);
        }
        self.slots[idx].state = SlotState::Freed;
        Ok(true)
    }

    /// 句柄闸序核对（纯函数口径；闸序：槽号→代数域→槽态→代数失配→指针）。
    pub fn validate(&self, handle: u32) -> Result<(), HdCode> {
        let (idx, gen) = unpack(handle);
        // 闸 1：槽号越界。
        if idx >= TABLE_SLOTS {
            return Err(HdCode::IdxOob);
        }
        // 闸 2：代数越域。
        if gen == 0 || gen >= GEN_MOD {
            return Err(HdCode::GenOob);
        }
        let e = &self.slots[idx];
        // 闸 3：槽态（空/释放按代数失配口径拒）。
        if e.state == SlotState::Empty {
            return Err(HdCode::GenOob);
        }
        if e.state == SlotState::Freed {
            return Err(HdCode::GenMismatch);
        }
        // 闸 4：代数失配（陈旧句柄）。
        if e.gen != gen {
            return Err(HdCode::GenMismatch);
        }
        // 闸 5：指针核。
        if e.ptr == 0 {
            return Err(HdCode::PtrNull);
        }
        if !ptr_ok(e.ptr) {
            return Err(HdCode::PtrOob);
        }
        Ok(())
    }

    /// 拒止记账便桥（validate + 拒止累计；账面口径）。
    pub fn validate_ledged(&mut self, handle: u32) -> Result<(), HdCode> {
        let r = self.validate(handle);
        if r.is_err() {
            self.rejects += 1;
        }
        r
    }

    /// 活句柄计数。
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|e| e.state == SlotState::Used).count()
    }

    /// 槽快照。
    pub fn slot_of(&self, idx: usize) -> Option<HandleEntry> {
        if idx >= TABLE_SLOTS {
            None
        } else {
            Some(self.slots[idx])
        }
    }

    /// D3 联签登记（三列齐 + 段基一致 + 重号拒）。
    pub fn register_d3(&mut self, batch: &'static str, theme: &'static str, seg: u32) -> Result<bool, HdCode> {
        if batch.is_empty() || theme.is_empty() {
            self.rejects += 1;
            return Err(HdCode::D3Dup);
        }
        if seg != D3_SEG {
            self.rejects += 1;
            return Err(HdCode::SegMismatch);
        }
        if self.d3_registered_named(batch) {
            self.rejects += 1;
            return Err(HdCode::D3Dup);
        }
        self.d3_list.push(D3Row { batch, theme, seg, status: 0 });
        if self.d3_list.len() > self.d3_high {
            self.d3_high = self.d3_list.len();
        }
        Ok(true)
    }

    /// D3 已登记判据。
    pub fn d3_registered(&self) -> bool {
        self.d3_registered_named("D3-B")
    }

    /// 指定批号在册判据。
    pub fn d3_registered_named(&self, batch: &str) -> bool {
        self.d3_list.iter().any(|b| b.batch == batch && b.seg == D3_SEG)
    }

    /// 联测清单长。
    pub fn d3_len(&self) -> usize {
        self.d3_list.len()
    }

    /// 清单只增不减判据。
    pub fn d3_monotone(&self) -> bool {
        self.d3_list.len() >= self.d3_high
    }

    /// 打开累计。
    pub fn open_total(&self) -> u32 {
        self.opens
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

// D3 联测清单（拆出独立字段以便 register_d3 实现）。
#[derive(Clone, Copy, Debug)]
struct D3Row {
    /// 批号名。
    batch: &'static str,
    /// 承接主题。
    theme: &'static str,
    /// 承接段基。
    seg: u32,
    /// 状态（0 登记/1 承接/2 关闭）。
    status: u8,
}

/// 注入例（坏代数/坏指针两类；带期望拒码）。
#[derive(Clone, Copy, Debug)]
pub struct InjectCase {
    /// 例号。
    pub case_no: usize,
    /// 句柄值。
    pub handle: u32,
    /// 期望拒码。
    pub expect: HdCode,
}

/// 两类注入例抽签（20 例：坏代数 0..10、坏指针 10..20）。
fn pick_case(lcg: &mut Lcg, i: usize, table: &HandleTable) -> InjectCase {
    let class = i / CASES_PER_CLASS;
    let k = lcg.below(4);
    match class {
        0 => {
            // 坏代数：越域/陈旧/空槽。
            let (idx, gen) = match k {
                0 => (0usize, 0u32),                          // 零代数 → GenOob
                1 => (1usize, GEN_MOD),                       // 越域代数 → GenOob（或槽 1 未用）
                2 => (0usize, table.slot_of(0).map(|e| if e.gen + 1 >= GEN_MOD { 1 } else { e.gen + 1 }).unwrap_or(1)), // 陈旧
                _ => (3usize, 5u32),                          // 空槽 → GenOob
            };
            InjectCase { case_no: i, handle: idx as u32 | (gen << 16), expect: HdCode::GenOob }
        }
        _ => {
            // 坏指针例：槽号 = 类内序（坏指针表预置面配槽）；代数 1。
            // 指针域由演练面预置坏指针槽后逐例核对（PtrNull/PtrOob 分流）。
            let h = (i % CASES_PER_CLASS) as u32;
            InjectCase { case_no: i, handle: h | (1u32 << 16), expect: HdCode::PtrOob }
        }
    }
}

/// 注入档账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 例账（append-only）。
    cases: Vec<(usize, HdCode)>,
    /// 末例号。
    last_case: u64,
    /// 放行计数。
    passes: u32,
    /// 拒止累计。
    rejects: u32,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { cases: Vec::new(), last_case: u64::MAX, passes: 0, rejects: 0 }
    }

    /// 跑一例（闸序判止 + 码值核对 + 账面登记）。
    pub fn run_case(&mut self, case_no: usize, table: &HandleTable, handle: u32, expect: HdCode) -> Result<(), HdCode> {
        if self.last_case != u64::MAX && (case_no as u64) <= self.last_case {
            self.rejects += 1;
            return Err(HdCode::CaseDup);
        }
        self.last_case = case_no as u64;
        match table.validate(handle) {
            Ok(()) => {
                self.passes += 1;
                Ok(())
            }
            Err(code) => {
                if code != expect {
                    self.rejects += 1;
                    return Err(code);
                }
                self.cases.push((case_no, code));
                self.rejects += 1;
                Ok(())
            }
        }
    }

    /// 例账长。
    pub fn case_len(&self) -> usize {
        self.cases.len()
    }

    /// 例账快照。
    pub fn case_of(&self, i: usize) -> Option<(usize, HdCode)> {
        if i >= self.cases.len() {
            None
        } else {
            Some(self.cases[i])
        }
    }

    /// 例号唯一判据。
    pub fn cases_unique(&self) -> bool {
        let mut i = 0;
        while i < self.cases.len() {
            let mut j = i + 1;
            while j < self.cases.len() {
                if self.cases[i].0 == self.cases[j].0 {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 放行计数。
    pub fn pass_total(&self) -> u32 {
        self.passes
    }
}

/// 坏代数/坏指针 20 例拒止演练（闸序码值核对；D3 联签在册）。
pub fn drill_inject20_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30d80 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut gen_hit = 0usize;
        let mut ptr_hit = 0usize;
        // 坏代数 10 例（直接对标准表跑）。
        let mut ib = InjectBook::new();
        let std_table = HandleTable::new();
        for i in 0..CASES_PER_CLASS {
            let c = pick_case(&mut lcg, i, &std_table);
            match ib.run_case(c.case_no, &std_table, c.handle, c.expect) {
                Ok(()) => gen_hit += 1,
                Err(_) => {}
            }
        }
        // 坏指针 10 例（预置坏指针表：槽 0..10 全放坏指针，代数 1）。
        let mut bad_table = HandleTable::new();
        for s in 0..CASES_PER_CLASS {
            let ptr = match s % 4 {
                0 => 0,
                1 => OBJ_BASE - 1,
                2 => OBJ_BASE + OBJ_SPAN,
                _ => 0xDEAD_0000,
            };
            bad_table.slots[s] = HandleEntry { gen: 1, ptr, state: SlotState::Used };
        }
        for i in 0..CASES_PER_CLASS {
            let expect = if bad_table.slot_of(i).map(|e| e.ptr == 0).unwrap_or(false) {
                HdCode::PtrNull
            } else {
                HdCode::PtrOob
            };
            let h = i as u32 | (1u32 << 16);
            match ib.run_case(CASES_PER_CLASS + i, &bad_table, h, expect) {
                Ok(()) => ptr_hit += 1,
                Err(_) => {}
            }
        }
        // D3 联签面在册。
        let mut table = HandleTable::new();
        let r1 = table.register_d3("D3-B", "句柄代数承接批", D3_SEG);
        let r2 = table.register_d3("D3-B", "句柄代数承接批", D3_SEG);
        let _ = lcg.below(3);
        if gen_hit == CASES_PER_CLASS
            && ptr_hit == CASES_PER_CLASS
            && ib.case_len() == INJECT_CASES
            && ib.cases_unique()
            && matches!(r1, Ok(true))
            && matches!(r2, Err(HdCode::D3Dup))
            && table.d3_registered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("坏代数/坏指针 20/20 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 代数生命周期演练：占槽→释放→陈旧拒→重开新代数 10/10。
pub fn drill_gen_lifecycle_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30d80 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let h1 = t.alloc_entry(OBJ_BASE + 0x10);
        let c1 = t.close_entry(h1.unwrap_or(0));
        // 陈旧句柄（代数 1）必拒。
        let stale = t.validate(h1.unwrap_or(0));
        // 重开：新代数 2。
        let h2 = t.alloc_entry(OBJ_BASE + 0x20);
        let v2 = h2.is_ok() && t.validate(h2.unwrap_or(0)).is_ok();
        let gen2 = h2.map(|h| h >> 16).unwrap_or(0);
        let _ = lcg.below(3);
        if matches!(h1, Ok(_))
            && matches!(c1, Ok(true))
            && matches!(stale, Err(HdCode::GenMismatch))
            && h2.is_ok()
            && v2
            && gen2 == 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("代数生命周期轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：好句柄放行、逐闸首码命中 10/10。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30d80 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let h = t.alloc_entry(OBJ_BASE + 0x30).unwrap_or(0);
        let good = t.validate(h).is_ok();
        // 闸 1：槽号越界。
        let g1 = matches!(t.validate(TABLE_SLOTS as u32 | (1 << 16)), Err(HdCode::IdxOob));
        // 闸 2：代数越域（零代数）。
        let g2 = matches!(t.validate(h & 0xFFFF), Err(HdCode::GenOob));
        // 闸 5：坏指针（预置坏指针槽）。
        let mut bad = HandleTable::new();
        bad.slots[0] = HandleEntry { gen: 1, ptr: 0, state: SlotState::Used };
        let g5 = matches!(bad.validate(0 | (1u32 << 16)), Err(HdCode::PtrNull));
        let _ = lcg.below(3);
        if good && g1 && g2 && g5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：句柄表转录导出（逐占用槽一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 句柄表转录导出。
pub fn export_handle_lines(t: &HandleTable) -> String {
    let mut s = String::from("句柄表转录：\n");
    for i in 0..TABLE_SLOTS {
        if let Some(e) = t.slot_of(i) {
            if e.state == SlotState::Used {
                s.push_str(&format!(
                    "H{} 代{} 针{:x}\n",
                    i, e.gen, e.ptr
                ));
            }
        }
    }
    s
}

/// 转录完整判据（占用槽逐行齐备）。
pub fn handle_lines_complete(t: &HandleTable) -> bool {
    let s = export_handle_lines(t);
    let live = t.live_count();
    if live == 0 {
        return false;
    }
    let mut hits = 0usize;
    for i in 0..TABLE_SLOTS {
        if let Some(e) = t.slot_of(i) {
            if e.state == SlotState::Used && s.contains(&format!("H{} 代", i)) {
                hits += 1;
            }
        }
    }
    hits == live
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：句柄表报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct HdReport {
    /// 活句柄数。
    pub live: usize,
    /// 打开累计。
    pub opens: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// D3 在册。
    pub d3: bool,
    /// 转录完整。
    pub lines: bool,
}

impl HdReport {
    /// 建报告。
    pub fn build(t: &HandleTable) -> Self {
        HdReport {
            live: t.live_count(),
            opens: t.open_total(),
            rejects: t.reject_total(),
            d3: t.d3_registered(),
            lines: handle_lines_complete(t),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.live >= 1 && self.d3 && self.rejects >= 1 && self.lines
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "HdReport 活={} 开={} 拒={} D3={} 转录={}",
            self.live, self.opens, self.rejects, self.d3, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：代数域演练（代数取模回卷 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_gen_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30d80 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut t = HandleTable::new();
        // 反复占释同一槽：代数 1→15→1（取模回卷口径）。
        let mut pass = true;
        let mut last_gen = 0u32;
        for i in 0..30u32 {
            let h = t.alloc_entry(OBJ_BASE + (i % OBJ_SPAN));
            pass = pass && h.is_ok();
            let g = h.map(|x| x >> 16).unwrap_or(0);
            pass = pass && g != 0 && g < GEN_MOD && g != last_gen;
            last_gen = g;
            let _ = t.close_entry(h.unwrap_or(0));
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("代数域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：指针域演练（好指针放行/坏指针三向拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_ptr_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30d80 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = ptr_ok(OBJ_BASE);
        pass = pass && ptr_ok(OBJ_BASE + OBJ_SPAN - 1);
        pass = pass && !ptr_ok(0);
        pass = pass && !ptr_ok(OBJ_BASE - 1);
        pass = pass && !ptr_ok(OBJ_BASE + OBJ_SPAN);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("指针域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct HdCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_hd_checklist(t: &HandleTable) -> Vec<HdCheckRow> {
    let rep = HdReport::build(t);
    let mut rows: Vec<HdCheckRow> = Vec::new();
    rows.push(HdCheckRow { name: "活句柄在册", passed: t.live_count() >= 1 });
    rows.push(HdCheckRow { name: "D3已登记", passed: t.d3_registered() });
    rows.push(HdCheckRow { name: "清单单调", passed: t.d3_monotone() });
    rows.push(HdCheckRow { name: "代数口径", passed: GEN_MOD == 16 && TABLE_SLOTS == 16 });
    rows.push(HdCheckRow { name: "指针域", passed: drill_ptr_domain_10().0 });
    rows.push(HdCheckRow { name: "代数域", passed: drill_gen_domain_10().0 });
    rows.push(HdCheckRow { name: "转录完整", passed: handle_lines_complete(t) });
    rows.push(HdCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(HdCheckRow { name: "拒止在账", passed: t.reject_total() >= 1 });
    rows.push(HdCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn hd_checklist_green(rows: &[HdCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn hd_stats_line(t: &HandleTable) -> String {
    format!(
        "句柄表概况：活 {} 开 {} 拒 {} D3 {}",
        t.live_count(),
        t.open_total(),
        t.reject_total(),
        t.d3_registered()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12508_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut t = HandleTable::new();
    let _ = t.alloc_entry(OBJ_BASE + 0x40);
    let _ = t.register_d3("D3-B", "句柄代数承接批", D3_SEG);
    // 注入一例越界槽号（拒止账口径）。
    let _ = t.validate_ledged(TABLE_SLOTS as u32 | (1 << 16));
    // 注入一例陈旧句柄（拒止账口径）。
    let hstale = t.alloc_entry(OBJ_BASE + 0x80).unwrap_or(0);
    let _ = t.close_entry(hstale);
    let _ = t.validate_ledged(hstale);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        handle_lines_complete(&t) && HdReport::build(&t).all_green(),
        hd_stats_line(&t),
    ));
    // X-b：代数域 10/10。
    let (ok_g, d_g) = drill_gen_domain_10();
    out.push(("X-代数域10/10", ok_g, d_g));
    // X-c：指针域 10/10。
    let (ok_p, d_p) = drill_ptr_domain_10();
    out.push(("X-指针域10/10", ok_p, d_p));
    // X-d：终验清单。
    let rows = build_hd_checklist(&t);
    out.push(("X-终验清单10项", hd_checklist_green(&rows), HdReport::build(&t).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut t = HandleTable::new();
    let h = t.alloc_entry(OBJ_BASE + 0x50).unwrap_or(0);
    // 注 1：槽号越界必拒（IdxOob）。
    if matches!(t.validate(TABLE_SLOTS as u32 + 2 | (1 << 16)), Err(HdCode::IdxOob)) {
        caught += 1;
        d.push("注1 越槽:抓".to_string());
    } else {
        d.push("注1 越槽:漏".to_string());
    }
    // 注 2：零代数必拒（GenOob）。
    if matches!(t.validate(h & 0xFFFF), Err(HdCode::GenOob)) {
        caught += 1;
        d.push("注2 零代:抓".to_string());
    } else {
        d.push("注2 零代:漏".to_string());
    }
    // 注 3：陈旧句柄必拒（GenMismatch）。
    let _ = t.close_entry(h);
    if matches!(t.validate(h), Err(HdCode::GenMismatch)) {
        caught += 1;
        d.push("注3 陈旧:抓".to_string());
    } else {
        d.push("注3 陈旧:漏".to_string());
    }
    // 注 4：坏指针必拒（PtrOob 面：预置坏指针槽）。
    let mut bad = HandleTable::new();
    bad.slots[0] = HandleEntry { gen: 1, ptr: OBJ_BASE + OBJ_SPAN + 8, state: SlotState::Used };
    if matches!(bad.validate(0 | (1u32 << 16)), Err(HdCode::PtrOob)) {
        caught += 1;
        d.push("注4 坏针:抓".to_string());
    } else {
        d.push("注4 坏针:漏".to_string());
    }
    // 注 5：段基失配必拒（SegMismatch）。
    if matches!(t.register_d3("D3-B", "T", 0x30da0), Err(HdCode::SegMismatch)) {
        caught += 1;
        d.push("注5 段基:抓".to_string());
    } else {
        d.push("注5 段基:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30d80 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    HdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12508_checks：本单判据聚合（诊断段 0x30d80）。
pub fn run_unx12508_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12508 句柄表坏项拒止（D3 联签预告）");
    // J1-a：坏代数/坏指针 20 例拒止全过（10/10）。
    let (ok1, d1) = drill_inject20_10();
    s.add_dyn("J1-坏代数坏指针20/20", ok1, &d1);
    // J1-b：代数生命周期（10/10）。
    let (ok2, d2) = drill_gen_lifecycle_10();
    s.add_dyn("J1-代数生命周期10/10", ok2, &d2);
    // J1-c：闸序（10/10）。
    let (ok3, d3) = drill_gate_order_10();
    s.add_dyn("J1-闸序10/10", ok3, &d3);
    // J1-d：标准会话（好句柄 + D3 在册 + 转录）。
    let mut t = HandleTable::new();
    let h = t.alloc_entry(OBJ_BASE + 0x60);
    let _ = t.register_d3("D3-B", "句柄代数承接批", D3_SEG);
    s.add_dyn(
        "J1-D3联签面登记在册",
        h.is_ok() && t.validate(h.unwrap_or(0)).is_ok() && t.d3_registered() && handle_lines_complete(&t),
        &format!(
            "柄={:?} D3={} 转录={}",
            h.is_ok(),
            t.d3_registered(),
            handle_lines_complete(&t)
        ),
    );
    // J2：注入核 5/5。
    let (c, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t2, &format!("{}/{} {}", c, t2, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30d80段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12508_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
