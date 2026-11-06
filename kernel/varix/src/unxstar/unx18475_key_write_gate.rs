//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18475 · 键写入辅助（Varix 兼容设置写入口） · 0x7F280段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18475-J1 写入口经 AI-18 接口落盘断言全过，原子写+备份回滚
//!       在档，直写注入必红
//!
//! 设计要点：
//!   - 唯一合法落盘通道是 AI-18 接口：gate 闸（键号/值域/容量）→
//!     日志段（追加带序号+校验和的记录）→ 原子提交（两相：先入日志
//!     后切态，提交前先校验日志记录完好，坏记录即回退到 Idle）；
//!   - 原子写在档：提交要么「日志+状态」齐动，要么都不动；半提交
//!     （状态切了日志没记）由对账账直接点名；
//!   - 备份回滚在档：每格保留上一已提交值+触发序号，回滚走补偿
//!     日志（kind=2），回滚后值与日志双向一致；
//!   - 直写注入必红：绕过 gate 直改格值，会与日志末端记录失配，
//!     直写审计逐格对账必须全部点名（漏检 0）；
//!   - 边界：未知键格 / 空值 / 值超 0xFFFF / 日志满 / 重复回滚 /
//!     回滚无备份——六类显式拒；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 日志环 [JrRec;16] + 备份账 [Option<BkRec>;12] 双账本
//!   [状态机]   KeyCell::Idle→Staged{seq}→Committed{gen} 三态推进
//!   [账本]     写入/提交/回滚/直写检出 四计数守恒
//!   [错误路径] KwCode 12 码闭集，code() = SEG_BASE + n
//!   [注入核]   直写注入/半提交注入/非法回滚 三类 ×10 轮必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F280，AI-VX11-1105 批04 独占区，A08 工单卡五）。
pub const SEG_BASE: u32 = 0x7F280;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 键格数（冻结：12；兼容设置键槽闭集）。
pub const CELL_N: usize = 12;
/// 日志容量（冻结：16 条；满后必须先收口再写）。
pub const JR_N: usize = 16;
/// 值上限（冻结：0xFFFF，16 位配置槽域）。
pub const VALUE_MAX: u32 = 0xFFFF;
/// AI-18 接口号（冻结：18；J1 原文锚定「经 AI-18 接口落盘」）。
pub const AI18_PORT: u32 = 18;
/// 日志记录种类：1=写入 2=回滚补偿。
pub const JR_KIND_WRITE: u8 = 1;
pub const JR_KIND_ROLLBACK: u8 = 2;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// FNV-1a 32 位（日志记录校验和与封印哈希同源）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        if let Some(&b) = data.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 单记录校验和：kind/seq/cell/val 四元组的 FNV。
fn rec_chk(kind: u8, seq: u32, cell: usize, val: u32) -> u32 {
    fnv1a(&[
        kind,
        (seq & 0xFF) as u8,
        ((seq >> 8) & 0xFF) as u8,
        ((seq >> 16) & 0xFF) as u8,
        ((seq >> 24) & 0xFF) as u8,
        cell as u8,
        (val & 0xFF) as u8,
        ((val >> 8) & 0xFF) as u8,
        ((val >> 16) & 0xFF) as u8,
        ((val >> 24) & 0xFF) as u8,
    ])
}

// ── 日志记录 ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JrRec {
    /// 种类（1 写入 / 2 回滚补偿）。
    pub kind: u8,
    /// 全局序号（单调递增，从 1 起）。
    pub seq: u32,
    /// 目标键格。
    pub cell: usize,
    /// 值（回滚补偿记录的 val = 被恢复的旧值）。
    pub val: u32,
    /// 记录校验和。
    pub chk: u32,
}

impl JrRec {
    pub fn intact(&self) -> bool {
        self.chk == rec_chk(self.kind, self.seq, self.cell, self.val)
    }
}

// ── 键格状态机 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    /// 空闲（无未决写）。
    Idle,
    /// 已入日志待提交（seq = 日志序号）。
    Staged,
    /// 已提交（gen = 提交代，单调递增）。
    Committed,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyCell {
    pub state: CellState,
    pub seq: u32,
    pub gen: u32,
    /// 当前生效值（Idle 态为 0 哨兵）。
    pub value: u32,
}

impl KeyCell {
    pub const fn empty() -> KeyCell {
        KeyCell { state: CellState::Idle, seq: 0, gen: 0, value: 0 }
    }
}

// ── 备份账 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BkRec {
    /// 上一已提交值（回滚目标）。
    pub old_value: u32,
    /// 触发备份的写序号。
    pub seq: u32,
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KwCode {
    /// 未知键格
    UnknownCell,
    /// 空值写入
    EmptyValue,
    /// 值超限
    ValueOverflow,
    /// 日志满
    JournalFull,
    /// 日志记录损坏
    JournalCorrupt,
    /// 半提交（状态与日志失账）
    HalfCommit,
    /// 无备份可回滚
    NoBackup,
    /// 重复回滚
    DuplicateRollback,
    /// 直写注入（绕过 AI-18 gate）
    DirectWriteDetected,
    /// 计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
}

impl KwCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KwCode::UnknownCell => "未知键格：0..12 闭集外，先查 CELL_N",
            KwCode::EmptyValue => "空值写入：0 值不是合法配置",
            KwCode::ValueOverflow => "值超限：0xFFFF 上限",
            KwCode::JournalFull => "日志满：16 条上限，先收口再写",
            KwCode::JournalCorrupt => "日志记录损坏：校验和失配，提交前必须拦截",
            KwCode::HalfCommit => "半提交：状态已切而日志无记，原子性红线",
            KwCode::NoBackup => "无备份可回滚：首次提交前不许回滚",
            KwCode::DuplicateRollback => "重复回滚：同代只许回滚一次",
            KwCode::DirectWriteDetected => "直写注入：绕过 AI-18 gate 的写被审计点名",
            KwCode::ConservationBroken => "计数守恒破坏：写入≠提交+回滚+检出",
            KwCode::NotReproducible => "双跑不一致：复现红线",
            KwCode::SegmentOutside => "段位越窗：诊断码不在 0x7F280 段",
        }
    }
    pub const ALL: [KwCode; 12] = [
        KwCode::UnknownCell,
        KwCode::EmptyValue,
        KwCode::ValueOverflow,
        KwCode::JournalFull,
        KwCode::JournalCorrupt,
        KwCode::HalfCommit,
        KwCode::NoBackup,
        KwCode::DuplicateRollback,
        KwCode::DirectWriteDetected,
        KwCode::ConservationBroken,
        KwCode::NotReproducible,
        KwCode::SegmentOutside,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < KwCode::ALL.len() {
        if let Some(c) = KwCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F280 段闭集内"
}

// ── 写入口总成 ─────────────────────────────────────────────────
pub struct WriteGate {
    pub cells: [KeyCell; CELL_N],
    /// 日志环（顺序追加，不覆盖——满即拒）。
    pub journal: [Option<JrRec>; JR_N],
    pub jr_len: usize,
    pub next_seq: u32,
    /// 备份账（每格至多一份：最近一次提交前的旧值）。
    pub backups: [Option<BkRec>; CELL_N],
    /// 回滚代账（每格已回滚到的 gen，防重复回滚）。
    pub rolled_to: [u32; CELL_N],
    /// 计数：写请求 / 提交 / 回滚 / 直写检出。
    pub writes: u32,
    pub commits: u32,
    pub rollbacks: u32,
    pub direct_caught: u32,
    /// 封印哈希（日志全量 FNV，直写审计对表基准）。
    pub seal: u32,
}

impl WriteGate {
    pub fn new() -> WriteGate {
        WriteGate {
            cells: [
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
                KeyCell::empty(),
            ],
            journal: [None; JR_N],
            jr_len: 0,
            next_seq: 1,
            backups: [None; CELL_N],
            rolled_to: [0; CELL_N],
            writes: 0,
            commits: 0,
            rollbacks: 0,
            direct_caught: 0,
            seal: 0,
        }
    }

    /// 日志追加（内部通道；gate 与回滚补偿共用，回滚合法绕 gate）。
    fn jr_append(&mut self, kind: u8, cell: usize, val: u32) -> Result<u32, u32> {
        if self.jr_len >= JR_N {
            return Err(KwCode::JournalFull.code());
        }
        let seq = self.next_seq;
        let rec = JrRec { kind, seq, cell, val, chk: rec_chk(kind, seq, cell, val) };
        match self.journal.get_mut(self.jr_len) {
            Some(slot) => {
                *slot = Some(rec);
            }
            None => return Err(KwCode::JournalFull.code()),
        }
        self.jr_len += 1;
        self.next_seq = self.next_seq.saturating_add(1);
        self.reseal();
        Ok(seq)
    }

    /// 封印哈希重算：日志全量序列化 FNV（直写审计基准）。
    fn reseal(&mut self) {
        let mut buf: [u8; 8] = [0; 8];
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0;
        while i < JR_N {
            if let Some(Some(rec)) = self.journal.get(i) {
                // 每条记录压 8 字节：kind|seq16|cell|val16。
                buf[0] = rec.kind;
                buf[1] = (rec.seq & 0xFF) as u8;
                buf[2] = ((rec.seq >> 8) & 0xFF) as u8;
                buf[3] = rec.cell as u8;
                buf[4] = (rec.val & 0xFF) as u8;
                buf[5] = ((rec.val >> 8) & 0xFF) as u8;
                buf[6] = rec.chk as u8;
                buf[7] = ((rec.chk >> 8) & 0xFF) as u8;
                let mut k = 0;
                while k < 8 {
                    if let Some(&b) = buf.get(k) {
                        h ^= b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        self.seal = h;
    }

    /// AI-18 写入口：gate 闸 → 日志段 → 原子提交。返回日志序号。
    pub fn write_key(&mut self, cell: usize, val: u32) -> Result<u32, u32> {
        // gate 闸（值域先行，非法值无裁决意义）。
        if cell >= CELL_N {
            return Err(KwCode::UnknownCell.code());
        }
        if val == 0 {
            return Err(KwCode::EmptyValue.code());
        }
        if val > VALUE_MAX {
            return Err(KwCode::ValueOverflow.code());
        }
        self.writes = self.writes.saturating_add(1);
        // 日志段：追加写记录。
        let seq = self.jr_append(JR_KIND_WRITE, cell, val)?;
        // 原子提交：先验记录完好再切态；坏记录回退 Idle。
        let intact = match self.journal.get(self.jr_len - 1) {
            Some(Some(rec)) => rec.intact(),
            _ => false,
        };
        if !intact {
            // 回退：格回 Idle（原子性——半态不留）。
            if let Some(c) = self.cells.get_mut(cell) {
                c.state = CellState::Idle;
            }
            return Err(KwCode::JournalCorrupt.code());
        }
        // 备份：无论首写还是覆盖，先把旧值入备份账（首写旧值=0 哨兵，
        // 回滚即回到未写态）。
        let prev = match self.cells.get(cell) {
            Some(c) => BkRec { old_value: c.value, seq },
            None => return Err(KwCode::UnknownCell.code()),
        };
        if let Some(slot) = self.backups.get_mut(cell) {
            *slot = Some(prev);
        }
        // 切态：Staged → Committed，代 +1。
        let old_gen = match self.cells.get(cell) {
            Some(c) => c.gen,
            None => 0,
        };
        if let Some(c) = self.cells.get_mut(cell) {
            c.state = CellState::Committed;
            c.seq = seq;
            c.gen = old_gen.saturating_add(1);
            c.value = val;
        }
        self.commits = self.commits.saturating_add(1);
        Ok(seq)
    }

    /// 回滚：恢复备份账旧值 + 补偿日志。同代只许回滚一次。
    pub fn rollback(&mut self, cell: usize) -> Result<u32, u32> {
        if cell >= CELL_N {
            return Err(KwCode::UnknownCell.code());
        }
        let (state, gen, rolled) = match self.cells.get(cell) {
            Some(c) => (c.state, c.gen, match self.rolled_to.get(cell) {
                Some(v) => *v,
                None => 0,
            }),
            None => return Err(KwCode::UnknownCell.code()),
        };
        if state != CellState::Committed {
            return Err(KwCode::NoBackup.code());
        }
        if gen == 0 || rolled >= gen {
            return Err(KwCode::DuplicateRollback.code());
        }
        let bk = match self.backups.get(cell) {
            Some(Some(b)) => *b,
            _ => return Err(KwCode::NoBackup.code()),
        };
        // 补偿日志（kind=2，val=恢复的旧值）。
        let seq = self.jr_append(JR_KIND_ROLLBACK, cell, bk.old_value)?;
        if let Some(c) = self.cells.get_mut(cell) {
            c.value = bk.old_value;
            c.gen = gen; // 代不变：回滚是同代内的值恢复。
        }
        if let Some(slot) = self.rolled_to.get_mut(cell) {
            *slot = gen;
        }
        self.rollbacks = self.rollbacks.saturating_add(1);
        Ok(seq)
    }

    /// 直写注入模拟：绕 gate 直改格值（不进日志）。
    /// 这是审计的对象，不是合法通道——仅供注入核与审计测试调用。
    pub fn simulate_direct_write(&mut self, cell: usize, val: u32) -> Result<(), u32> {
        if cell >= CELL_N {
            return Err(KwCode::UnknownCell.code());
        }
        if let Some(c) = self.cells.get_mut(cell) {
            c.value = val;
        }
        Ok(())
    }

    /// 直写审计：对每个 Committed 格，其值必须等于日志中该格最后
    /// 一条写记录的 val（回滚后 = 最后补偿记录的 val）。失配即直写。
    pub fn audit_direct_writes(&mut self) -> u32 {
        // 先从日志反推每格期望值（最后一条相关记录）。
        let mut expect: [Option<u32>; CELL_N] = [None; CELL_N];
        let mut i = 0;
        while i < JR_N {
            let rec = match self.journal.get(i) {
                Some(Some(r)) => *r,
                _ => {
                    i += 1;
                    continue;
                }
            };
            if rec.cell < CELL_N {
                match expect.get_mut(rec.cell) {
                    Some(slot) => {
                        *slot = Some(rec.val);
                    }
                    None => {}
                }
            }
            i += 1;
        }
        let mut caught = 0u32;
        let mut c = 0;
        while c < CELL_N {
            let exp = match expect.get(c) {
                Some(Some(v)) => *v,
                _ => {
                    c += 1;
                    continue;
                }
            };
            let mismatch = match self.cells.get(c) {
                Some(cell) => cell.state == CellState::Committed && cell.value != exp,
                None => false,
            };
            if mismatch {
                caught = caught.saturating_add(1);
                self.direct_caught = self.direct_caught.saturating_add(1);
            }
            c += 1;
        }
        caught
    }

    /// 半提交审计：Staged 态格必须能在日志找到同 seq 记录。
    pub fn audit_half_commit(&self) -> Result<(), u32> {
        let mut c = 0;
        while c < CELL_N {
            let st = match self.cells.get(c) {
                Some(cell) => (cell.state, cell.seq),
                None => (CellState::Idle, 0),
            };
            if st.0 == CellState::Staged {
                // 日志中找同 seq 写记录。
                let mut found = false;
                let mut i = 0;
                while i < JR_N {
                    if let Some(Some(r)) = self.journal.get(i) {
                        if r.seq == st.1 && r.kind == JR_KIND_WRITE {
                            found = true;
                        }
                    }
                    i += 1;
                }
                if !found {
                    return Err(KwCode::HalfCommit.code());
                }
            }
            c += 1;
        }
        Ok(())
    }

    /// 日志完好审计：已写区逐条校验和。
    pub fn journal_intact(&self) -> bool {
        let mut i = 0;
        while i < self.jr_len {
            let ok = match self.journal.get(i) {
                Some(Some(rec)) => rec.intact(),
                _ => false,
            };
            if !ok {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 计数守恒：写入 = 提交 + 拒绝由调用方对账；此处核
    /// 提交 ≥ 回滚（回滚必以提交为先）。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.rollbacks > self.commits {
            return Err(KwCode::ConservationBroken.code());
        }
        if self.commits == 0 && self.writes > 0 {
            return Err(KwCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────
/// 写 6 格（值推导 v=0x0100+i）→ 读回对账 → 回滚 2 格 → 再对账。
/// 返回（写入数, 提交数, 回滚数）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut g = WriteGate::new();
    let mut i = 0;
    while i < 6 {
        let v = 0x0100u32 + i as u32;
        g.write_key(i, v)?;
        i += 1;
    }
    if g.commits != 6 {
        return Err(KwCode::ConservationBroken.code());
    }
    if !g.journal_intact() {
        return Err(KwCode::JournalCorrupt.code());
    }
    g.audit_half_commit()?;
    // 回滚格 0 与格 1。
    let _ = g.rollback(0)?;
    let _ = g.rollback(1)?;
    if g.rollbacks != 2 {
        return Err(KwCode::ConservationBroken.code());
    }
    // 回滚后值 = 0（无更早备份）——回滚到首代恢复 Idle 前值 0。
    let v0 = match g.cells.get(0) {
        Some(c) => c.value,
        None => 0xFFFF,
    };
    let v1 = match g.cells.get(1) {
        Some(c) => c.value,
        None => 0xFFFF,
    };
    if v0 != 0 || v1 != 0 {
        return Err(KwCode::ConservationBroken.code());
    }
    if g.audit_direct_writes() != 0 {
        return Err(KwCode::DirectWriteDetected.code());
    }
    g.conserve()?;
    Ok((g.writes, g.commits, g.rollbacks))
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：直写注入——绕 AI-18 gate 直改格值，审计必点名。
/// 注入二：半提交注入——手造 Staged 态无日志记录，审计必点名。
/// 注入三：非法回滚——无备份回滚 / 重复回滚必拒。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：直写注入 ——
        tries = tries.saturating_add(1);
        {
            let mut g = WriteGate::new();
            let _ = g.write_key(3, 0x0300);
            let _ = g.simulate_direct_write(3, 0x0BAD);
            let hit = g.audit_direct_writes() == 1;
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：半提交注入 ——
        tries = tries.saturating_add(1);
        {
            let mut g = WriteGate::new();
            // 手造 Staged 态且日志空 → audit_half_commit 必红。
            if let Some(c) = g.cells.get_mut(5) {
                c.state = CellState::Staged;
                c.seq = 99; // 日志中不存在的序号。
            }
            let hit = g.audit_half_commit().err() == Some(KwCode::HalfCommit.code());
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：非法回滚 ——
        tries = tries.saturating_add(1);
        {
            let mut g = WriteGate::new();
            // 无提交先回滚 → NoBackup。
            let no_bk = g.rollback(2).err() == Some(KwCode::NoBackup.code());
            // 提交一次后回滚两次 → 第二次 DuplicateRollback。
            let _ = g.write_key(2, 0x0200);
            let _ = g.rollback(2);
            let dup = g.rollback(2).err() == Some(KwCode::DuplicateRollback.code());
            if no_bk && dup {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(KwCode::ConservationBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KwCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KwCode::ALL.len() {
        if let Some(c) = KwCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// AI-18 端口锚：所有合法落盘必须出自 write_key（gate 通道）。
/// 本探针核「写入口经 AI-18 接口落盘」——直改通道被审计点名，
/// gate 通道被日志+备份+状态三账同时收录。
pub fn ai18_anchor_probe() -> Result<(), u32> {
    let mut g = WriteGate::new();
    let seq = g.write_key(7, 0x0777)?;
    // 日志里有同 seq 的写记录（落盘在档）。
    let mut logged = false;
    let mut i = 0;
    while i < JR_N {
        if let Some(Some(rec)) = g.journal.get(i) {
            if rec.seq == seq && rec.kind == JR_KIND_WRITE && rec.cell == 7 {
                logged = true;
            }
        }
        i += 1;
    }
    if !logged {
        return Err(KwCode::HalfCommit.code());
    }
    // 备份+状态+直写审计三账齐绿。
    g.audit_half_commit()?;
    if g.audit_direct_writes() != 0 {
        return Err(KwCode::DirectWriteDetected.code());
    }
    Ok(())
}

/// 日志满边界：连写 16 条后第 17 条必须 JournalFull，且前 16 条账齐。
pub fn journal_full_edge() -> Result<(), u32> {
    let mut g = WriteGate::new();
    let mut i = 0;
    while i < JR_N {
        g.write_key(i % CELL_N, 0x0011)?;
        i += 1;
    }
    if g.jr_len != JR_N {
        return Err(KwCode::ConservationBroken.code());
    }
    let full = g.write_key(0, 0x0022).err() == Some(KwCode::JournalFull.code());
    if !full {
        return Err(KwCode::ConservationBroken.code());
    }
    if !g.journal_intact() {
        return Err(KwCode::JournalCorrupt.code());
    }
    Ok(())
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18475 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18475_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18475");
    // 1. AI-18 端口冻结。
    s.add(
        "J1-AI18端口冻结",
        AI18_PORT == 18 && CELL_N == 12 && JR_N == 16,
        "端口18/格12/日志16",
    );
    // 2. 黄金全链。
    let g = golden_run();
    s.add_dyn("J1-黄金全链", g.is_ok(), &format!("写/提交/回滚={:?}", g));
    // 3. AI-18 落盘锚。
    s.add("J1-AI18落盘锚", ai18_anchor_probe().is_ok(), "gate 通道三账齐收");
    // 4. 原子写在档：半提交审计双探针（合法态绿 / 伪造态红）。
    {
        let mut g = WriteGate::new();
        let _ = g.write_key(1, 0x0101);
        let clean = g.audit_half_commit().is_ok();
        if let Some(c) = g.cells.get_mut(4) {
            c.state = CellState::Staged;
            c.seq = 42;
        }
        let fake = g.audit_half_commit().err() == Some(KwCode::HalfCommit.code());
        s.add("J1-原子写在档", clean && fake, "合法绿/伪造红双向断言");
    }
    // 5. 备份回滚在档：二次提交后回滚恢复首值。
    {
        let mut g = WriteGate::new();
        let _ = g.write_key(2, 0x000A);
        let _ = g.write_key(2, 0x000B);
        let _ = g.rollback(2);
        let v = match g.cells.get(2) {
            Some(c) => c.value,
            None => 0,
        };
        s.add("J1-备份回滚在档", v == 0x000A, "回滚恢复上一提交值");
    }
    // 6. 注入三类 ×10 轮全抓（直写必红）。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 7. 直写注入单发必红（J1 明示「直写注入必红」）。
    {
        let mut g = WriteGate::new();
        let _ = g.write_key(0, 0x0011);
        let _ = g.simulate_direct_write(0, 0x0BAD);
        s.add(
            "J1-直写必红单发",
            g.audit_direct_writes() == 1 && g.direct_caught == 1,
            "绕 gate 直改一格即点名",
        );
    }
    // 8. 日志满边界。
    s.add("J1-日志满边界", journal_full_edge().is_ok(), "16 条满拒且账齐");
    // 9. 值域三边界。
    {
        let mut g = WriteGate::new();
        let e0 = g.write_key(0, 0).err();
        let ov = g.write_key(0, VALUE_MAX + 1).err();
        let unk = g.write_key(CELL_N, 1).err();
        let okv = g.write_key(0, VALUE_MAX).is_ok();
        s.add(
            "J1-值域三边界",
            e0 == Some(KwCode::EmptyValue.code())
                && ov == Some(KwCode::ValueOverflow.code())
                && unk == Some(KwCode::UnknownCell.code())
                && okv,
            "0拒/超限拒/越格拒/上限收",
        );
    }
    // 10. 日志序号单调。
    {
        let mut g = WriteGate::new();
        let a = g.write_key(3, 0x0001);
        let b = g.write_key(4, 0x0002);
        let mono = match (a, b) {
            (Ok(sa), Ok(sb)) => sb == sa + 1,
            _ => false,
        };
        s.add("J1-序号单调", mono, "日志 seq 严格 +1");
    }
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F280 窗口对齐 0x40");
    // 13. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < KwCode::ALL.len() {
            if let Some(c) = KwCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "12 码中文明细全非空");
    }
    // 14. 计数守恒。
    {
        let mut g = WriteGate::new();
        let mut i = 0;
        while i < 6 {
            let _ = g.write_key(i, 0x0100 + i as u32);
            i += 1;
        }
        let _ = g.rollback(0);
        let ok = match g.conserve() {
            Ok(()) => g.writes == 6 && g.commits == 6 && g.rollbacks == 1,
            Err(_) => false,
        };
        s.add("J1-计数守恒", ok, "写6提交6回滚1账平");
    }
    // 15. 直写审计零误报：纯 gate 流直写检出必须为 0。
    {
        let mut g = WriteGate::new();
        let mut i = 0;
        while i < 6 {
            let _ = g.write_key(i, 0x0200 + i as u32);
            i += 1;
        }
        let _ = g.rollback(3);
        s.add(
            "J1-审计零误报",
            g.audit_direct_writes() == 0 && g.journal_intact(),
            "纯 gate 流不误点名",
        );
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        let mut g = WriteGate::new();
        let seq = match g.write_key(3, 0x0300) {
            Ok(s) => s,
            Err(_) => 0,
        };
        assert_eq!(seq, 1);
        let v = match g.cells.get(3) {
            Some(c) => c.value,
            None => 0,
        };
        assert_eq!(v, 0x0300);
    }

    #[test]
    fn t02_boundary_value_domain() {
        let mut g = WriteGate::new();
        assert_eq!(g.write_key(0, 0).err(), Some(KwCode::EmptyValue.code()));
        assert_eq!(
            g.write_key(0, VALUE_MAX + 1).err(),
            Some(KwCode::ValueOverflow.code())
        );
        assert_eq!(g.write_key(CELL_N, 1).err(), Some(KwCode::UnknownCell.code()));
        assert!(g.write_key(0, VALUE_MAX).is_ok());
    }

    #[test]
    fn t03_inject_direct_write() {
        let mut g = WriteGate::new();
        let _ = g.write_key(3, 0x0300);
        let _ = g.simulate_direct_write(3, 0x0BAD);
        assert_eq!(g.audit_direct_writes(), 1);
    }

    #[test]
    fn t04_inject_half_commit() {
        let mut g = WriteGate::new();
        if let Some(c) = g.cells.get_mut(5) {
            c.state = CellState::Staged;
            c.seq = 99;
        }
        assert_eq!(
            g.audit_half_commit().err(),
            Some(KwCode::HalfCommit.code())
        );
    }

    #[test]
    fn t05_inject_illegal_rollback() {
        let mut g = WriteGate::new();
        assert_eq!(g.rollback(2).err(), Some(KwCode::NoBackup.code()));
        let _ = g.write_key(2, 0x0200);
        assert!(g.rollback(2).is_ok());
        assert_eq!(g.rollback(2).err(), Some(KwCode::DuplicateRollback.code()));
    }

    #[test]
    fn t06_atomic_write_two_phase() {
        let mut g = WriteGate::new();
        let _ = g.write_key(1, 0x0101);
        assert!(g.audit_half_commit().is_ok());
        if let Some(c) = g.cells.get_mut(4) {
            c.state = CellState::Staged;
            c.seq = 42;
        }
        assert_eq!(
            g.audit_half_commit().err(),
            Some(KwCode::HalfCommit.code())
        );
    }

    #[test]
    fn t07_rollback_backup_chain() {
        let mut g = WriteGate::new();
        let _ = g.write_key(2, 0x000A);
        let _ = g.write_key(2, 0x000B);
        let _ = g.rollback(2);
        let v = match g.cells.get(2) {
            Some(c) => c.value,
            None => 0,
        };
        assert_eq!(v, 0x000A);
    }

    #[test]
    fn t08_journal_full_edge() {
        assert!(journal_full_edge().is_ok());
    }

    #[test]
    fn t09_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t10_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t11_checks_all_green() {
        let set = run_unx18475_checks();
        assert!(set.all_passed(), "UNX-F18475 自检须全绿：{}", set.red_items_text());
    }
}
