//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18451 · 声明→矩阵写入路径 · 0x7EC80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18451-J1 解析结果写入矩阵（F18421 表）断言全过，依据字段
//!       自动登记为 manifest 源
//! 本模块结构清单：
//!   1. 冻结常量层：槽容量 / 维度闭集 / 值域闭集 / 判据轮数（推导注释
//!      写死，不拍脑袋）；
//!   2. 诊断码闭集 DwCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一：DeclParsed（解析结果四元组：进程键/API 族/维度/
//!      值，域守卫 complete()）；
//!   4. 数据结构二：MatrixWriteGate（F18421 表写入闸——槽账本 +
//!      三态状态机 Empty→Claimed→Committed，键唯一性线性扫，冻结后
//!      拒写）；
//!   5. 数据结构三：SourceLedger（依据账——每次 commit 自动登记一条
//!      manifest 源依据，序号单调，与 commit 计数守恒）；
//!   6. 写入路径：parse→claim→commit 三段，段间守恒
//!      （claim 数 = commit 数 + 回滚数）；
//!   7. 注入核 4 类×10 轮：重复键 / 维度越界 / 槽满 / 二次提交，
//!      逐类断言被显式拒绝；
//!   8. 黄金全链 golden_run：12 正例入表→依据账核对→注入全抓→
//!      双跑逐字段一致；
//!   9. run_unx18451_checks：15 条判据（断言性质）。
//! 为什么本卡是写入闸：F18421 裁决表是全兼容面的唯一生效面，声明
//! 解析结果若不经守卫直写，重复键/越界值会污染裁决口径——故本卡
//! 把写入路径做成「声明→闸→矩阵+依据账」的单向流水，闸上六类违约
//! 全部显式拒绝并留账。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EC80，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EC80;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 槽容量（冻结 24：F18421 表按 16 键设计 + 8 槽滚动余量）。
pub const SLOT_CAP: usize = 24;
/// 维度闭集大小（冻结 4：F18421 键第三元=维度，取 0..4）。
pub const DIM_CLOSEN: u32 = 4;
/// API 族闭集大小（冻结 6：F18424 进程键登记面族数）。
pub const API_CLOSEN: u32 = 6;
/// 值域上界（冻结 0xFFFF：四段版本单段上限，复用 F18421 值模型）。
pub const VALUE_MAX: u32 = 0xFFFF;
/// 正例数（冻结 12：每 API 族 2 例）。
pub const GOOD_CASES: usize = 12;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 依据账序号起点（冻结 1：0 留作空槽哨兵）。
pub const BASIS_SEQ_FIRST: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DwCode {
    /// 空键（进程键为 0）
    KeyEmpty,
    /// API 族越界（闭集外）
    ApiIllegal,
    /// 维度越界（闭集外）
    DimIllegal,
    /// 值越界（>0xFFFF）
    ValueIllegal,
    /// 重复键
    KeyDup,
    /// 槽满
    TableFull,
    /// 状态倒退（未声明先提交/已提交再声明）
    StateBackward,
    /// 依据账漏记（commit 后未自动登记 manifest 源）
    SourceLedgerMiss,
    /// 依据账序号断链
    BasisSeqBroken,
    /// 段间守恒破坏（claim≠commit+回滚）
    ConservationBroken,
    /// 冻结后写入
    FrozenWrite,
    /// 双跑不一致
    NotReproducible,
}

impl DwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            DwCode::KeyEmpty => "空键：进程键 0 非法，先取 F18424 进程键号",
            DwCode::ApiIllegal => "API 族越界：闭集 0..6 外",
            DwCode::DimIllegal => "维度越界：闭集 0..4 外",
            DwCode::ValueIllegal => "值越界：单段版本上限 0xFFFF",
            DwCode::KeyDup => "重复键：同 (进程,API,维度) 二次声明拒",
            DwCode::TableFull => "槽满：24 槽容量上限",
            DwCode::StateBackward => "状态倒退：未声明先提交或已提交再声明",
            DwCode::SourceLedgerMiss => "依据账漏记：commit 必须自动登记 manifest 源",
            DwCode::BasisSeqBroken => "依据账序号断链：单调递增被破坏",
            DwCode::ConservationBroken => "段间守恒破坏：claim 数 ≠ commit 数 + 回滚数",
            DwCode::FrozenWrite => "冻结后写入：闸已封",
            DwCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [DwCode; 12] = [
        DwCode::KeyEmpty,
        DwCode::ApiIllegal,
        DwCode::DimIllegal,
        DwCode::ValueIllegal,
        DwCode::KeyDup,
        DwCode::TableFull,
        DwCode::StateBackward,
        DwCode::SourceLedgerMiss,
        DwCode::BasisSeqBroken,
        DwCode::ConservationBroken,
        DwCode::FrozenWrite,
        DwCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DwCode::ALL.len() {
        if DwCode::ALL[i].val() == code {
            return DwCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EC80 段闭集内"
}

// ── 哈希（依据指纹，确定性） ────────────────────────────────────
/// FNV-1a 32 位：依据字段指纹 = hash(进程‖API‖维度‖值)。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 四元组依据指纹（字节序写死：proc,api,dim,value 各按小端四字节拼接）。
pub fn basis_fingerprint(proc: u32, api: u32, dim: u32, value: u32) -> u32 {
    let mut buf = [0u8; 16];
    let mut k = 0;
    while k < 4 {
        buf[k] = (proc >> (8 * k)) as u8;
        buf[4 + k] = (api >> (8 * k)) as u8;
        buf[8 + k] = (dim >> (8 * k)) as u8;
        buf[12 + k] = (value >> (8 * k)) as u8;
        k += 1;
    }
    fnv1a(&buf)
}

// ── 数据结构一：解析结果 ────────────────────────────────────────
/// 声明解析结果（解析段产物，写入段的唯一合法输入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclParsed {
    pub proc_key: u32,
    pub api_family: u32,
    pub dim: u32,
    pub value: u32,
}

impl DeclParsed {
    /// 域守卫：四段全部在闭集内才算完整解析产物。
    pub fn complete(&self) -> bool {
        self.proc_key > 0
            && self.api_family < API_CLOSEN
            && self.dim < DIM_CLOSEN
            && self.value <= VALUE_MAX
    }
    /// 依据指纹（依据字段=manifest 源）。
    pub fn fingerprint(&self) -> u32 {
        basis_fingerprint(self.proc_key, self.api_family, self.dim, self.value)
    }
}

// ── 槽状态机 ────────────────────────────────────────────────────
/// 槽三态：Empty→Claimed→Committed 单向，禁止倒退与跳级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Empty,
    Claimed,
    Committed,
}

/// 写入槽：键 + 状态 + 值 + 依据指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteSlot {
    pub proc_key: u32,
    pub api_family: u32,
    pub dim: u32,
    pub value: u32,
    pub state: SlotState,
    pub basis: u32,
}

impl WriteSlot {
    pub fn empty() -> WriteSlot {
        WriteSlot {
            proc_key: 0,
            api_family: 0,
            dim: 0,
            value: 0,
            state: SlotState::Empty,
            basis: 0,
        }
    }
    /// 键相同判定（三元组全等）。
    pub fn same_key(&self, proc: u32, api: u32, dim: u32) -> bool {
        self.proc_key == proc && self.api_family == api && self.dim == dim
    }
}

// ── 依据账（manifest 源自动登记） ───────────────────────────────
/// 依据账行：序号 + 槽下标 + 源类别（冻结 manifest 源）+ 指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasisRow {
    pub seq: u32,
    pub slot_idx: usize,
    pub src_kind: u32,
    pub fingerprint: u32,
}

/// 源类别冻结值：manifest 源 = 1（0 留空哨兵，2+ 留未来扩展）。
pub const SRC_KIND_MANIFEST: u32 = 1;

// ── 数据结构二：写入闸 ─────────────────────────────────────────
/// F18421 表写入闸：槽账本 + 依据账 + 计数账。
pub struct MatrixWriteGate {
    pub slots: Vec<WriteSlot>,
    pub basis_ledger: Vec<BasisRow>,
    pub claimed: u32,
    pub committed: u32,
    pub rolled_back: u32,
    pub basis_seq_next: u32,
    pub frozen: bool,
    /// 注入检出账（槽 0..3 对应四类注入）。
    pub reject_ledger: Vec<u32>,
}

impl MatrixWriteGate {
    pub fn new() -> MatrixWriteGate {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < SLOT_CAP {
            slots.push(WriteSlot::empty());
            i += 1;
        }
        MatrixWriteGate {
            slots,
            basis_ledger: Vec::new(),
            claimed: 0,
            committed: 0,
            rolled_back: 0,
            basis_seq_next: BASIS_SEQ_FIRST,
            frozen: false,
            reject_ledger: Vec::new(),
        }
    }

    /// 键唯一性线性扫：返回已占用键的槽下标。
    fn find_key(&self, proc: u32, api: u32, dim: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.state != SlotState::Empty && s.same_key(proc, api, dim) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 找首个空槽。
    fn find_empty(&self) -> Option<usize> {
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.state == SlotState::Empty {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 段一：claim——域守卫 + 键唯一 + 容量 + 冻结闸。
    /// 返回槽下标；违约一律 Err(码)。
    pub fn claim(&mut self, d: &DeclParsed) -> Result<usize, u32> {
        if self.frozen {
            return Err(DwCode::FrozenWrite.val());
        }
        if !d.complete() {
            // 域守卫细化：按违约定序返回首个命中码。
            if d.proc_key == 0 {
                return Err(DwCode::KeyEmpty.val());
            }
            if d.api_family >= API_CLOSEN {
                return Err(DwCode::ApiIllegal.val());
            }
            if d.dim >= DIM_CLOSEN {
                return Err(DwCode::DimIllegal.val());
            }
            return Err(DwCode::ValueIllegal.val());
        }
        if let Some(_) = self.find_key(d.proc_key, d.api_family, d.dim) {
            self.reject_ledger.push(DwCode::KeyDup.val());
            return Err(DwCode::KeyDup.val());
        }
        let slot = match self.find_empty() {
            Some(s) => s,
            None => {
                self.reject_ledger.push(DwCode::TableFull.val());
                return Err(DwCode::TableFull.val());
            }
        };
        let fp = d.fingerprint();
        // 先在块内算好要写的内容，块外再改计数（借用法红线）。
        if let Some(s) = self.slots.get_mut(slot) {
            s.proc_key = d.proc_key;
            s.api_family = d.api_family;
            s.dim = d.dim;
            s.value = d.value;
            s.basis = fp;
            s.state = SlotState::Claimed;
        }
        self.claimed = self.claimed.saturating_add(1);
        Ok(slot)
    }

    /// 段二：commit——Claimed→Committed，并自动登记 manifest 源依据。
    pub fn commit(&mut self, slot_idx: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(DwCode::FrozenWrite.val());
        }
        if slot_idx >= self.slots.len() {
            return Err(DwCode::StateBackward.val());
        }
        let (fp, is_claimed) = {
            let s = match self.slots.get(slot_idx) {
                Some(x) => x,
                None => return Err(DwCode::StateBackward.val()),
            };
            if s.state != SlotState::Claimed {
                // Empty 提交=跳级；Committed 再提交=二次提交，同为倒退。
                self.reject_ledger.push(DwCode::StateBackward.val());
                (s.basis, false)
            } else {
                (s.basis, true)
            }
        };
        if !is_claimed {
            return Err(DwCode::StateBackward.val());
        }
        if let Some(s) = self.slots.get_mut(slot_idx) {
            s.state = SlotState::Committed;
        }
        self.committed = self.committed.saturating_add(1);
        // 依据字段自动登记为 manifest 源（J1 后半句）。
        let seq = self.basis_seq_next;
        self.basis_ledger.push(BasisRow {
            seq,
            slot_idx,
            src_kind: SRC_KIND_MANIFEST,
            fingerprint: fp,
        });
        self.basis_seq_next = self.basis_seq_next.saturating_add(1);
        Ok(seq)
    }

    /// 段三：回滚——Claimed→Empty（合法回退，计入守恒账）。
    pub fn rollback(&mut self, slot_idx: usize) -> Result<(), u32> {
        if slot_idx >= self.slots.len() {
            return Err(DwCode::StateBackward.val());
        }
        let ok = {
            let s = match self.slots.get(slot_idx) {
                Some(x) => x,
                None => return Err(DwCode::StateBackward.val()),
            };
            s.state == SlotState::Claimed
        };
        if !ok {
            return Err(DwCode::StateBackward.val());
        }
        if let Some(s) = self.slots.get_mut(slot_idx) {
            s.state = SlotState::Empty;
            s.basis = 0;
        }
        self.rolled_back = self.rolled_back.saturating_add(1);
        Ok(())
    }

    /// 段间守恒：claim 数 = commit 数 + 回滚数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.claimed != self.committed.saturating_add(self.rolled_back) {
            return Err(DwCode::ConservationBroken.val());
        }
        Ok(())
    }

    /// 依据账守恒：commit 数 = 依据行数，且序号从 1 起连续递增。
    pub fn basis_conserve(&self) -> Result<(), u32> {
        if self.committed != self.basis_ledger.len() as u32 {
            return Err(DwCode::SourceLedgerMiss.val());
        }
        let mut i = 0;
        while i < self.basis_ledger.len() {
            let r = match self.basis_ledger.get(i) {
                Some(x) => x,
                None => return Err(DwCode::SourceLedgerMiss.val()),
            };
            if r.seq != BASIS_SEQ_FIRST + i as u32 {
                return Err(DwCode::BasisSeqBroken.val());
            }
            if r.src_kind != SRC_KIND_MANIFEST {
                return Err(DwCode::SourceLedgerMiss.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 查询面：按键查 Committed 槽的值（写入闸的读出口）。
    pub fn query(&self, proc: u32, api: u32, dim: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.state == SlotState::Committed && s.same_key(proc, api, dim) {
                    return Some(s.value);
                }
            }
            i += 1;
        }
        None
    }

    /// 冻结闸：封闸后 claim/commit 全拒（读不受限）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

// ── 正例构造（12 例确定性） ─────────────────────────────────────
/// 构造第 idx 例正例（每 API 族 2 例，值确定性推导）。
pub fn make_good_decl(idx: usize) -> DeclParsed {
    let api = (idx % API_CLOSEN as usize) as u32;
    let half = idx / API_CLOSEN as usize; // 0 或 1
    let proc = 1 + ((idx * 7 + half * 3) % 20) as u32;
    // 同族第二例换维度，避免键撞车（键=三元组）。
    let dim = (api + half as u32 + 1) % DIM_CLOSEN;
    // 值域闭集内：0x0100 基点 + 确定性步进（0x0100+11*11+1 ≤ 0x0186 < 0xFFFF）。
    let value = 0x0100_u32 + (idx as u32) * 0x0B + half as u32;
    DeclParsed { proc_key: proc, api_family: api, dim, value }
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数（必须为 0）。
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：每轮新闸，四类违约逐类打，逐类必被显式拒绝。
/// 槽 0 重复键 / 槽 1 维度越界 / 槽 2 槽满 / 槽 3 二次提交。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：重复键——同键声明两次，第二次必拒。
        let mut g = MatrixWriteGate::new();
        let d = make_good_decl(0);
        let first = g.claim(&d);
        match first {
            Ok(i0) => {
                let c = g.commit(i0);
                let d2 = DeclParsed {
                    proc_key: d.proc_key,
                    api_family: d.api_family,
                    dim: d.dim,
                    value: d.value.wrapping_add(1),
                };
                let hit = g.claim(&d2).err() == Some(DwCode::KeyDup.val());
                led.record(0, hit && c.is_ok());
            }
            Err(_) => led.record(0, false),
        }
        // 槽 1：维度越界——dim=4 闭集外。
        let mut g1 = MatrixWriteGate::new();
        let bad_dim = DeclParsed { proc_key: 3, api_family: 0, dim: DIM_CLOSEN, value: 100 };
        let hit1 = g1.claim(&bad_dim).err() == Some(DwCode::DimIllegal.val());
        led.record(1, hit1);
        // 槽 2：槽满——用小容量语义等价法：连打 24 个合法键填满，
        // 第 25 个（换 proc 键）必拒 TableFull。
        let mut g2 = MatrixWriteGate::new();
        let mut filled = 0;
        let mut k = 0;
        while k < SLOT_CAP {
            let dk = DeclParsed {
                proc_key: (1 + k as u32) % 20 + 1,
                api_family: (k as u32) % API_CLOSEN,
                // 每例独占一个键位：dim 用 k 变化保证键不重复。
                dim: ((k / 4) % 3 + k % DIM_CLOSEN as usize % DIM_CLOSEN as usize) as u32
                    % DIM_CLOSEN,
                value: 1,
            };
            match g2.claim(&dk) {
                Ok(i2) => {
                    g2.commit(i2).ok();
                    filled += 1;
                }
                Err(_) => {}
            }
            k += 1;
        }
        let overflow = DeclParsed { proc_key: 20, api_family: 5, dim: 3, value: 1 };
        let hit2 = filled == SLOT_CAP && g2.claim(&overflow).err() == Some(DwCode::TableFull.val());
        led.record(2, hit2);
        // 槽 3：二次提交——同槽 commit 两次，第二次必拒。
        let mut g3 = MatrixWriteGate::new();
        let d3 = make_good_decl(1);
        match g3.claim(&d3) {
            Ok(i3) => {
                let c1 = g3.commit(i3);
                let hit3 = c1.is_ok() && g3.commit(i3).err() == Some(DwCode::StateBackward.val());
                led.record(3, hit3);
            }
            Err(_) => led.record(3, false),
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：12 正例入表→守恒→依据账→注入全抓→冻结→冻结拒写→双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut g = MatrixWriteGate::new();
    let mut i = 0;
    while i < GOOD_CASES {
        let d = make_good_decl(i);
        let slot = g.claim(&d)?;
        g.commit(slot)?;
        // 写入路径闭环：query 面能读到刚写入的值。
        match g.query(d.proc_key, d.api_family, d.dim) {
            Some(v) if v == d.value => {}
            _ => return Err(DwCode::ConservationBroken.val()),
        }
        i += 1;
    }
    g.conserve()?;
    g.basis_conserve()?;
    if g.committed != GOOD_CASES as u32 {
        return Err(DwCode::ConservationBroken.val());
    }
    if g.basis_ledger.len() != GOOD_CASES {
        return Err(DwCode::SourceLedgerMiss.val());
    }
    // 注入 10 轮全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(DwCode::NotReproducible.val());
    }
    // 冻结后拒写（正探针的负半句）。
    g.freeze();
    let late = make_good_decl(GOOD_CASES);
    if g.claim(&late).err() != Some(DwCode::FrozenWrite.val()) {
        return Err(DwCode::FrozenWrite.val());
    }
    Ok((g.claimed, g.committed, g.basis_ledger.len() as u32))
}

/// 双跑一致：golden_run 纯确定性，两跑逐字段同值。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DwCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：合法声明走完 claim→commit→依据登记 全链。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut g = MatrixWriteGate::new();
    let d = make_good_decl(2);
    let slot = g.claim(&d)?;
    let seq = g.commit(slot)?;
    g.conserve()?;
    g.basis_conserve()?;
    if seq != BASIS_SEQ_FIRST {
        return Err(DwCode::BasisSeqBroken.val());
    }
    Ok(())
}

/// 负探针 1：空键拒（proc_key=0 是 0 哨兵，不是合法进程键）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = MatrixWriteGate::new();
    let d = DeclParsed { proc_key: 0, api_family: 1, dim: 0, value: 5 };
    match g.claim(&d) {
        Err(c) if c == DwCode::KeyEmpty.val() => Ok(()),
        _ => Err(DwCode::KeyEmpty.val()),
    }
}

/// 负探针 2：值越界拒（value = VALUE_MAX+1）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut g = MatrixWriteGate::new();
    let d = DeclParsed { proc_key: 2, api_family: 2, dim: 1, value: VALUE_MAX + 1 };
    match g.claim(&d) {
        Err(c) if c == DwCode::ValueIllegal.val() => Ok(()),
        _ => Err(DwCode::ValueIllegal.val()),
    }
}

/// 段位守卫：诊断码全部落在 [SEG_BASE, SEG_BASE+0x40) 且 0x40 对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DwCode::ALL.len() {
        let v = DwCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18451_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18451 声明→矩阵写入路径");
    // ① 黄金全链（J1 前半句：解析结果写入矩阵全过）。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "claim{:?} commit{:?} 依据{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 12 正例全入表（写入成功数 == 正例数）。
    s.add("正例12全入表", matches!(g, Ok((_, c, _)) if c == GOOD_CASES as u32),
        "解析结果 12/12 过闸落槽");
    // ③ 依据账自动登记（J1 后半句：依据字段自动登记为 manifest 源）。
    s.add("依据账自动登记", matches!(g, Ok((_, _, b)) if b == GOOD_CASES as u32),
        "每次 commit 自动登记 manifest 源一行");
    // ④ 段间守恒：claim = commit + 回滚。
    let mut gx = MatrixWriteGate::new();
    let dxa = make_good_decl(3);
    let dxb = make_good_decl(4);
    if let Ok(i) = gx.claim(&dxa) {
        gx.commit(i).ok();
    }
    if let Ok(i) = gx.claim(&dxb) {
        gx.rollback(i).ok();
    }
    s.add("段间守恒", gx.conserve().is_ok() && gx.claimed == 2 && gx.committed == 1,
        "claim2 = commit1 + 回滚1");
    // ⑤ 依据账序号连续 + 源类别闭集。
    s.add("依据账序号守恒", gx.basis_conserve().is_ok(),
        "seq 从 1 连续递增且源类别=manifest");
    // ⑥ 注入四类 10/10 检出。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑦ 注入漏检为零（计数守恒：注入=检出+漏检）。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入次数=检出次数+漏检次数，漏检=0");
    // ⑧ 键唯一性：重复键注入留下的拒因账非空且可读。
    let mut gd = MatrixWriteGate::new();
    let dd = make_good_decl(0);
    if let Ok(i) = gd.claim(&dd) {
        gd.commit(i).ok();
    }
    let dup = gd.claim(&make_good_decl(0));
    s.add("重复键显式拒绝", dup.err() == Some(DwCode::KeyDup.val()) && gd.reject_ledger.len() == 1,
        "第二次同键声明必拒并留拒因账");
    // ⑨ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "正链/空键拒/值越界拒");
    // ⑩ 状态机单调：Empty→Claimed→Committed 倒退被拒。
    let mut gs = MatrixWriteGate::new();
    let ds = make_good_decl(5);
    let st_ok = match gs.claim(&ds) {
        Ok(i) => {
            let first = gs.commit(i).is_ok();
            let again = gs.commit(i).err() == Some(DwCode::StateBackward.val());
            first && again
        }
        Err(_) => false,
    };
    s.add("状态机倒退拒", st_ok, "已提交槽二次提交必拒");
    // ⑪ 冻结闸：冻结后 claim/commit 全拒。
    let mut gf = MatrixWriteGate::new();
    gf.freeze();
    let df = make_good_decl(6);
    s.add("冻结后拒写",
        gf.claim(&df).err() == Some(DwCode::FrozenWrite.val()),
        "封闸后写入路径全拒，读不受限");
    // ⑫ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑬ 槽满边界：24 槽恰好填满，第 25 键拒且账不丢。
    let mut gb = MatrixWriteGate::new();
    let mut ok_filled = 0u32;
    let mut k = 0;
    while k < SLOT_CAP {
        let dk = DeclParsed {
            proc_key: (k as u32 / DIM_CLOSEN) + 1,
            api_family: (k as u32) % API_CLOSEN,
            dim: (k as u32) % DIM_CLOSEN,
            value: 2,
        };
        if let Ok(i) = gb.claim(&dk) {
            if gb.commit(i).is_ok() {
                ok_filled += 1;
            }
        }
        k += 1;
    }
    s.add("槽满边界账不丢", ok_filled == SLOT_CAP as u32,
        "满容量前合法写入全部落账");
    // ⑭ 域守卫：api/dim 闭集边界值（0 与 闭集-1 合法，闭集值非法）。
    let mut gdom = MatrixWriteGate::new();
    let edge_ok = DeclParsed { proc_key: 1, api_family: API_CLOSEN - 1, dim: DIM_CLOSEN - 1, value: VALUE_MAX };
    let edge_bad_api = DeclParsed { proc_key: 1, api_family: API_CLOSEN, dim: 0, value: 1 };
    let a = gdom.claim(&edge_ok).is_ok();
    let b = gdom.claim(&edge_bad_api).err() == Some(DwCode::ApiIllegal.val());
    s.add("闭集边界守卫", a && b, "api=5/dim=3/value=0xFFFF 合法；api=6 拒");
    // ⑮ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < DwCode::ALL.len() {
        if DwCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "12 码全有中文建议");
    s.add("段位守卫 0x7EC80", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chain_full_pass() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g, (12, 12, 12), "12 正例 claim/commit/依据三账同值");
    }

    #[test]
    fn good_decls_all_complete() {
        let mut i = 0;
        while i < GOOD_CASES {
            let d = make_good_decl(i);
            assert!(d.complete(), "正例 {} 域守卫必过", i);
            assert!(d.fingerprint() != 0, "依据指纹非零");
            i += 1;
        }
    }

    #[test]
    fn duplicate_key_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut g = MatrixWriteGate::new();
            let d = make_good_decl(0);
            let slot = g.claim(&d).unwrap_or(SLOT_CAP);
            g.commit(slot).ok();
            assert_eq!(g.claim(&d).err(), Some(DwCode::KeyDup.val()), "第 {} 轮重复键必拒", r);
            r += 1;
        }
    }

    #[test]
    fn dim_out_of_range_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut g = MatrixWriteGate::new();
            let d = DeclParsed { proc_key: 5, api_family: 0, dim: DIM_CLOSEN, value: 1 };
            assert_eq!(g.claim(&d).err(), Some(DwCode::DimIllegal.val()), "第 {} 轮维度越界必拒", r);
            r += 1;
        }
    }

    #[test]
    fn table_full_boundary_keeps_ledger() {
        let mut g = MatrixWriteGate::new();
        let mut k = 0;
        while k < SLOT_CAP {
            let dk = DeclParsed {
                proc_key: (k as u32 / DIM_CLOSEN) + 1,
                api_family: (k as u32) % API_CLOSEN,
                dim: (k as u32) % DIM_CLOSEN,
                value: 7,
            };
            let slot = g.claim(&dk).unwrap_or(SLOT_CAP);
            g.commit(slot).ok();
            k += 1;
        }
        assert_eq!(g.committed, SLOT_CAP as u32, "24 槽恰好填满");
        let overflow = DeclParsed { proc_key: 9, api_family: 5, dim: 3, value: 1 };
        assert_eq!(g.claim(&overflow).err(), Some(DwCode::TableFull.val()), "第 25 键必拒");
        assert_eq!(g.committed, SLOT_CAP as u32, "拒后账不丢");
    }

    #[test]
    fn state_machine_no_backward() {
        let mut g = MatrixWriteGate::new();
        let d = make_good_decl(7);
        let slot = g.claim(&d).unwrap_or(SLOT_CAP);
        assert!(g.commit(slot).is_ok(), "首次提交过");
        assert_eq!(g.commit(slot).err(), Some(DwCode::StateBackward.val()), "二次提交拒");
        assert_eq!(g.rollback(slot).err(), Some(DwCode::StateBackward.val()), "提交后回滚拒");
    }

    #[test]
    fn order_of_guards_key_first_then_value() {
        // 判定次序协议：空键先于值越界（键域在值域之前判）。
        let mut g = MatrixWriteGate::new();
        let d = DeclParsed { proc_key: 0, api_family: 1, dim: 0, value: VALUE_MAX + 1 };
        assert_eq!(g.claim(&d).err(), Some(DwCode::KeyEmpty.val()), "空键优先于值越界");
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(), "四类注入 10/10 全抓");
        assert_eq!(inj.missed(), 0, "漏检必须为零");
        for i in 0..4 {
            assert_eq!(inj.tries[i], inj.caught[i], "槽 {} 计数守恒", i);
        }
    }

    #[test]
    fn frozen_gate_blocks_write_only() {
        let mut g = MatrixWriteGate::new();
        let d = make_good_decl(8);
        let slot = g.claim(&d).unwrap_or(SLOT_CAP);
        g.commit(slot).ok();
        g.freeze();
        let late = make_good_decl(9);
        assert_eq!(g.claim(&late).err(), Some(DwCode::FrozenWrite.val()), "冻结后 claim 拒");
        assert!(g.query(d.proc_key, d.api_family, d.dim).is_some(), "冻结后读不受限");
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑逐字段一致");
    }

    #[test]
    fn probes_and_segment_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard(), "0x7EC80 段位守卫");
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x7EA00 && SEG_BASE <= 0x7EEC0);
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18451_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
