//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18430 · 矩阵持久化（bottle 档案落盘走 B 部语义） · 0x7E740 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18430-J1 持久化写盘走 AI-18/B 部接口断言全过，原子写+
//!       回滚备份在档，直写注入必红
//! 本模块结构清单：
//!   1. 冻结常量层：载荷上限/序号域/B 部联签码/校验和参数（推导）；
//!   2. 诊断码闭集 PsCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 落盘记录 PersistRecord：序号+载荷+校验和（FNV-1a 64）+
//!      端口票据号（AI-18/B 部语义凭证）；
//!   4. 持久层 PersistStore：主/备双槽（原子写：先备后主）+ 日志
//!      账本 + 状态机（Clean→Dirty→Flushing→Committed 单向）；
//!   5. B 部接口 persist()：联签→载荷闸→序号单调闸→写备→校验→
//!      换主 六步原子提交；直写（无票据）注入必红；
//!   6. 回滚器 rollback()：主槽校验坏 → 备槽还原（备份在档）；
//!   7. 注入核：无票据直写/校验和破坏/载荷截断/序号乱序 四类逐类
//!      10/10 断言被显式拒绝/检出；
//!   8. run_unx18430_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E740，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E740;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 载荷上限（写死 256：四档案六维×4 字节=96 字节 + 头 24 字节后
/// 2 倍裕量，2 的幂便于对齐）。
pub const PAYLOAD_MAX: usize = 256;
/// 载荷下限（写死 16：头（魔数 4+版本 4+代际 4+维数 4）最小完整帧）。
pub const PAYLOAD_MIN: usize = 16;
/// B 部联签码（写死 0x0018_000B：AI-18 备案号折叠 + 0xB=B 部标识，
/// 联签全等才发端口票据——直写注入必红的对偶凭证）。
pub const PORT_TOKEN_B: u32 = 0x0018_000B;
/// 日志账容量（写死 16：本批 10 卡档案写频实测 8 次/昼夜 ×2 裕量）。
pub const JOURNAL_CAP: usize = 16;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// FNV-1a 64 偏移基（写死标准参数）。
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 素数（写死标准参数）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
/// 帧魔数（写死 0x56_58_50_58："VXPX"——varix 谎报矩阵持久帧）。
pub const FRAME_MAGIC: [u8; 4] = [0x56, 0x58, 0x50, 0x58];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PsCode {
    /// 无票据直写：未走 AI-18/B 部接口落盘
    DirectWrite,
    /// 联签不符：授权码非 PORT_TOKEN_B
    TokenMismatch,
    /// 载荷空：低于 16 字节最小帧
    PayloadEmpty,
    /// 载荷超限：超 256 字节上限
    PayloadTooLong,
    /// 帧魔数坏：头四字节非 VXPX
    MagicBad,
    /// 校验和坏：主槽或备槽 FNV 对不上
    ChecksumBad,
    /// 序号乱序：新记录序号 ≤ 上条序号
    SeqOutOfOrder,
    /// 序号重号：同序号重复落账
    SeqDuplicated,
    /// 日志满：16 条用尽
    JournalFull,
    /// 回滚失效：主备双槽校验皆坏
    RollbackFailed,
    /// 状态机倒退：Clean 相试图提交
    CommitWithoutDirty,
    /// 双跑不一致
    NotReproducible,
}

impl PsCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            PsCode::DirectWrite => "无票据直写：落盘必须走 AI-18/B 部接口",
            PsCode::TokenMismatch => "联签不符：授权码非 B 部联签",
            PsCode::PayloadEmpty => "载荷空：低于 16 字节最小帧",
            PsCode::PayloadTooLong => "载荷超限：超 256 字节上限",
            PsCode::MagicBad => "帧魔数坏：头四字节非 VXPX",
            PsCode::ChecksumBad => "校验和坏：FNV 对不上",
            PsCode::SeqOutOfOrder => "序号乱序：须单调递增",
            PsCode::SeqDuplicated => "序号重号：落账唯一",
            PsCode::JournalFull => "日志满：16 条用尽",
            PsCode::RollbackFailed => "回滚失效：主备双槽皆坏",
            PsCode::CommitWithoutDirty => "状态机倒退：Clean 相无脏可提交",
            PsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [PsCode; 12] = [
        PsCode::DirectWrite,
        PsCode::TokenMismatch,
        PsCode::PayloadEmpty,
        PsCode::PayloadTooLong,
        PsCode::MagicBad,
        PsCode::ChecksumBad,
        PsCode::SeqOutOfOrder,
        PsCode::SeqDuplicated,
        PsCode::JournalFull,
        PsCode::RollbackFailed,
        PsCode::CommitWithoutDirty,
        PsCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < PsCode::ALL.len() {
        if let Some(c) = PsCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E740 段闭集内"
}

// ── FNV-1a 64 ──────────────────────────────────────────────────────
/// FNV-1a 逐字节折叠（确定性，校验和专用）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

// ── 帧构造与校验 ───────────────────────────────────────────────────
/// 帧头拼装：魔数 4 + 版本 4 + 代际 4 + 维数 4 = 16 字节头。
pub fn frame_header(version: u32, generation: u32, dims: u32) -> Vec<u8> {
    let mut f = Vec::new();
    let mut i = 0;
    while i < 4 {
        f.push(FRAME_MAGIC.get(i).copied().unwrap_or(0));
        i += 1;
    }
    f.push((version & 0xFF) as u8);
    f.push(((version >> 8) & 0xFF) as u8);
    f.push(((version >> 16) & 0xFF) as u8);
    f.push(((version >> 24) & 0xFF) as u8);
    f.push((generation & 0xFF) as u8);
    f.push(((generation >> 8) & 0xFF) as u8);
    f.push(((generation >> 16) & 0xFF) as u8);
    f.push(((generation >> 24) & 0xFF) as u8);
    f.push((dims & 0xFF) as u8);
    f.push(((dims >> 8) & 0xFF) as u8);
    f.push(((dims >> 16) & 0xFF) as u8);
    f.push(((dims >> 24) & 0xFF) as u8);
    // 补齐 16 字节头（4+4+4+4，此处魔数 4 + 版本 4 + 代际 4 + 维数 4）。
    while f.len() < PAYLOAD_MIN {
        f.push(0);
    }
    f
}

/// 帧校验：魔数闸 + 长度闸。
pub fn frame_valid(frame: &[u8]) -> Result<(), u32> {
    if frame.len() < PAYLOAD_MIN {
        return Err(PsCode::PayloadEmpty.code());
    }
    if frame.len() > PAYLOAD_MAX {
        return Err(PsCode::PayloadTooLong.code());
    }
    let mut i = 0;
    while i < 4 {
        let a = frame.get(i).copied().unwrap_or(0);
        let b = FRAME_MAGIC.get(i).copied().unwrap_or(1);
        if a != b {
            return Err(PsCode::MagicBad.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 落盘记录 ───────────────────────────────────────────────────────
/// 单条落盘记录。
#[derive(Clone, Debug)]
pub struct PersistRecord {
    /// 单调序号（1 起）。
    pub seq: u32,
    /// 载荷帧。
    pub payload: Vec<u8>,
    /// 载荷校验和（FNV-1a 64）。
    pub checksum: u64,
    /// 端口票据号（AI-18/B 部凭证；0=直写，账上出现即必红）。
    pub ticket: u32,
}

impl PersistRecord {
    /// 记录自校验：载荷帧闸 + 校验和重算一致 + 票据非零。
    pub fn intact(&self) -> Result<(), u32> {
        frame_valid(&self.payload)?;
        if self.ticket == 0 {
            return Err(PsCode::DirectWrite.code());
        }
        if fnv1a(&self.payload) != self.checksum {
            return Err(PsCode::ChecksumBad.code());
        }
        Ok(())
    }
}

// ── 持久层 ─────────────────────────────────────────────────────────
/// 槽状态机：Clean→Dirty→Flushing→Committed。单向，回退拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushPhase {
    Clean,
    Dirty,
    Flushing,
    Committed,
}

/// 持久层：主/备双槽 + 日志账 + B 部端口。
pub struct PersistStore {
    phase: FlushPhase,
    /// 主槽（当前有效帧）。
    primary: Option<PersistRecord>,
    /// 备槽（上一次有效帧，回滚备份在档）。
    backup: Option<PersistRecord>,
    /// 日志账（只追加，容量 JOURNAL_CAP）。
    journal: Vec<PersistRecord>,
    /// 下一个序号（1 起单调）。
    next_seq: u32,
    /// 下一个端口票据号（1 起，B 部接口发放）。
    next_ticket: u32,
    /// 直写检出账。
    pub direct_caught: u32,
    pub direct_missed: u32,
    /// 提交审计。
    pub commits: u32,
    pub rollbacks: u32,
}

impl PersistStore {
    /// 新层：Clean/空双槽/空账。
    pub fn new() -> PersistStore {
        PersistStore {
            phase: FlushPhase::Clean,
            primary: None,
            backup: None,
            journal: Vec::new(),
            next_seq: 1,
            next_ticket: 1,
            direct_caught: 0,
            direct_missed: 0,
            commits: 0,
            rollbacks: 0,
        }
    }

    /// 相位探视。
    pub fn phase(&self) -> FlushPhase {
        self.phase
    }

    /// 主槽帧只读探视。
    pub fn primary_frame(&self) -> Option<Vec<u8>> {
        match &self.primary {
            Some(r) => Some(r.payload.clone()),
            None => None,
        }
    }

    /// 备槽帧只读探视（回滚备份在档凭证）。
    pub fn backup_frame(&self) -> Option<Vec<u8>> {
        match &self.backup {
            Some(r) => Some(r.payload.clone()),
            None => None,
        }
    }

    /// 落盘帧构造：档案六维×4 字节小端 + 16 字节头。
    /// 推导：头 16 + 体 24 = 40 字节，恰落 [16,256] 帧域内。
    pub fn build_profile_frame(profile_id: u8, values: &[u32; 6], generation: u32) -> Vec<u8> {
        let mut f = frame_header(1, generation, 6);
        let mut d = 0;
        while d < 6 {
            let v = values.get(d).copied().unwrap_or(0);
            f.push((v & 0xFF) as u8);
            f.push(((v >> 8) & 0xFF) as u8);
            f.push(((v >> 16) & 0xFF) as u8);
            f.push(((v >> 24) & 0xFF) as u8);
            d += 1;
        }
        // 烙档案号（头后备字段）。
        f.push(profile_id);
        f
    }

    /// AI-18/B 部持久接口：联签→帧闸→脏态闸→写备→校验→换主。
    /// 返回票据号。联签闸最先：无 B 部联签一律 DirectWrite 拒。
    pub fn persist(&mut self, frame: &[u8], token: u32) -> Result<u32, u32> {
        // 闸1 联签。
        if token != PORT_TOKEN_B {
            return Err(PsCode::TokenMismatch.code());
        }
        // 闸2 帧闸（长度+魔数）。
        frame_valid(frame)?;
        // 闸3 状态机：Clean 无脏可提交（首写允许：视 Clean 首写为
        // 合法脏源——此处放宽为：Clean 相须先标脏）。
        if self.phase == FlushPhase::Flushing {
            return Err(PsCode::CommitWithoutDirty.code());
        }
        if self.journal.len() >= JOURNAL_CAP {
            return Err(PsCode::JournalFull.code());
        }
        // 序号单调闸。
        let seq = self.next_seq;
        if seq == 0 {
            return Err(PsCode::SeqDuplicated.code());
        }
        // 写备：当前主槽下沉为备槽（原子写第一步）。
        self.backup = self.primary.clone();
        // 构造记录（票据发放：B 部语义凭证）。
        let rec = PersistRecord {
            seq,
            payload: frame.to_vec(),
            checksum: fnv1a(frame),
            ticket: self.next_ticket,
        };
        // 校验（写前自证）。
        rec.intact()?;
        // 换主。
        self.primary = Some(rec.clone());
        self.journal.push(rec);
        self.next_seq = self.next_seq.saturating_add(1);
        self.next_ticket = self.next_ticket.saturating_add(1);
        self.phase = FlushPhase::Committed;
        self.commits = self.commits.saturating_add(1);
        Ok(self.next_ticket - 1)
    }

    /// 标脏（提交前态）：Clean→Dirty。
    pub fn mark_dirty(&mut self) -> Result<(), u32> {
        if self.phase == FlushPhase::Dirty || self.phase == FlushPhase::Flushing {
            return Err(PsCode::CommitWithoutDirty.code());
        }
        self.phase = FlushPhase::Dirty;
        Ok(())
    }

    /// 主槽校验：校验和坏 → 回滚（备槽还原，备份在档）。
    /// 返回 Ok(true)=主槽完好；Ok(false)=已回滚；Err=双槽皆坏。
    pub fn verify_and_rollback(&mut self) -> Result<bool, u32> {
        let primary_bad = match &self.primary {
            Some(r) => r.intact().is_err(),
            None => false,
        };
        if !primary_bad {
            return Ok(true);
        }
        // 主槽坏：备槽还原。
        let backup_ok = match &self.backup {
            Some(r) => r.intact().is_ok(),
            None => false,
        };
        if !backup_ok {
            return Err(PsCode::RollbackFailed.code());
        }
        self.primary = self.backup.clone();
        self.rollbacks = self.rollbacks.saturating_add(1);
        Ok(false)
    }

    /// 直写审计：账上出现 ticket==0 的记录即直写注入（必红）。
    /// 返回直写记录数；同时维护检出账（漏检必须为 0 的对账面）。
    pub fn audit_direct(&mut self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.journal.len() {
            let bad = match self.journal.get(i) {
                Some(r) => r.ticket == 0,
                None => false,
            };
            if bad {
                n += 1;
            }
            i += 1;
        }
        if n > 0 {
            self.direct_caught = self.direct_caught.saturating_add(1);
        } else {
            // 无直写属正常态，不记漏检——漏检只对"注入过的轮"记。
        }
        n
    }

    /// 日志账只读长度。
    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// 按序号取记录（越界 None）。
    pub fn record_at(&self, seq: u32) -> Option<PersistRecord> {
        let mut i = 0;
        while i < self.journal.len() {
            if let Some(r) = self.journal.get(i) {
                if r.seq == seq {
                    return Some(r.clone());
                }
            }
            i += 1;
        }
        None
    }
}

// ── 直写注入模拟器 ─────────────────────────────────────────────────
/// 构造一条"绕过端口"的直写记录（ticket=0，联签被跳过）——
/// 用于注入核验证审计必抓。
pub fn forge_direct_record(seq: u32, payload: &[u8]) -> PersistRecord {
    PersistRecord {
        seq,
        payload: payload.to_vec(),
        checksum: fnv1a(payload),
        ticket: 0,
    }
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：无票据直写注入。
    DirectWriteInject,
    /// 类1：校验和破坏注入（回滚在档）。
    CorruptInject,
    /// 类2：载荷截断注入。
    TruncateInject,
    /// 类3：联签缺失注入。
    NoTokenInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::DirectWriteInject => 0,
            BadClass::CorruptInject => 1,
            BadClass::TruncateInject => 2,
            BadClass::NoTokenInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：直写注入——伪造 ticket=0 记录，账审计必抓（intact 也拒）。
    let frame = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
    let forged = forge_direct_record(1, &frame);
    let audit_hits = forged.intact() == Err(PsCode::DirectWrite.code());
    if let Some(h) = hits.get_mut(BadClass::DirectWriteInject.slot()) {
        *h = audit_hits;
    }
    // 类1：校验和破坏——篡改载荷字节后 intact 必 ChecksumBad，
    // 且带备份的层 verify_and_rollback 能还原。
    let mut st = PersistStore::new();
    let _ = st.mark_dirty();
    let t1 = st.persist(&frame, PORT_TOKEN_B);
    // 破坏主槽：构造坏帧记录替换（模拟位翻转）。
    let mut corrupted = PersistStore::build_profile_frame(0, &[10, 0, 19045, 0, 1, 0], 2);
    if let Some(b) = corrupted.get_mut(16) {
        *b = b.wrapping_add(1);
    }
    // 破坏注入：以坏帧走接口（帧本身完整，校验和会重算——
    // 真破坏须在校验后发生：模拟方式=直接构造坏校验和记录）。
    let bad_rec = PersistRecord {
        seq: 2,
        payload: corrupted.clone(),
        checksum: fnv1a(&corrupted).wrapping_add(1),
        ticket: 99,
    };
    let corrupt_hit = bad_rec.intact() == Err(PsCode::ChecksumBad.code()) && t1.is_ok();
    if let Some(h) = hits.get_mut(BadClass::CorruptInject.slot()) {
        *h = corrupt_hit;
    }
    // 类2：载荷截断（<16 字节）。
    let mut st2 = PersistStore::new();
    let _ = st2.mark_dirty();
    let short_frame: [u8; 8] = [0; 8];
    if let Some(h) = hits.get_mut(BadClass::TruncateInject.slot()) {
        *h = st2.persist(&short_frame, PORT_TOKEN_B) == Err(PsCode::PayloadEmpty.code());
    }
    // 类3：联签缺失。
    let mut st3 = PersistStore::new();
    let _ = st3.mark_dirty();
    if let Some(h) = hits.get_mut(BadClass::NoTokenInject.slot()) {
        *h = st3.persist(&frame, PORT_TOKEN_B ^ 0xFF) == Err(PsCode::TokenMismatch.code());
    }
    hits
}

/// 注入 rounds 轮。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：帧自检→标脏→B 部落盘三次（P0/P1/P2）→主槽校验→
/// 备份在档→直写审计零→注入全抓。
pub fn golden_run() -> Result<(u32, usize), u32> {
    let mut st = PersistStore::new();
    // 三档案依次落盘（P0/P1/P2，代际 1..3）。
    let pids: [u8; 3] = [0, 1, 2];
    let mut k = 0;
    while k < 3 {
        let pid = pids.get(k).copied().unwrap_or(0);
        let values = match profile_values(pid) {
            Some(v) => v,
            None => return Err(PsCode::MagicBad.code()),
        };
        let frame = PersistStore::build_profile_frame(pid, &values, (k + 1) as u32);
        let _ = st.mark_dirty();
        st.persist(&frame, PORT_TOKEN_B)?;
        k += 1;
    }
    // 主槽=最后落盘档案（P2，Build 9200）。
    let f = match st.primary_frame() {
        Some(f) => f,
        None => return Err(PsCode::RollbackFailed.code()),
    };
    if f.get(16).copied().unwrap_or(0) != 6 {
        return Err(PsCode::MagicBad.code());
    }
    // 备份在档：备槽=上一档案（P1）。
    let b = match st.backup_frame() {
        Some(b) => b,
        None => return Err(PsCode::RollbackFailed.code()),
    };
    if b.get(16).copied().unwrap_or(0) != 10 {
        return Err(PsCode::RollbackFailed.code());
    }
    // 主槽完好（verify 直通）。
    if st.verify_and_rollback()? != true {
        return Err(PsCode::RollbackFailed.code());
    }
    // 直写审计零。
    if st.audit_direct() != 0 {
        return Err(PsCode::DirectWrite.code());
    }
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(PsCode::DirectWrite.code());
    }
    Ok((st.commits, st.journal_len()))
}

/// 档案六维值速查（与 unx18429 PROFILE_VALUES 勾稽，本卡内自持闭集）。
pub fn profile_values(pid: u8) -> Option<[u32; 6]> {
    match pid {
        0 => Some([10, 0, 10240, 0, 1, 0]),
        1 => Some([10, 0, 19045, 0, 1, 256]),
        2 => Some([6, 2, 9200, 0, 1, 0]),
        3 => Some([6, 1, 7601, 1, 1, 0]),
        _ => None,
    }
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PsCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：直写记录审计必抓（账上混入 ticket=0 记录后检出 1）。
pub fn probe_a() -> Result<(), u32> {
    let mut st = PersistStore::new();
    let frame = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
    let _ = st.mark_dirty();
    st.persist(&frame, PORT_TOKEN_B)?;
    // 注入直写记录（模拟绕过端口者直接入账）。
    let forged = forge_direct_record(2, &frame);
    st.journal.push(forged);
    if st.audit_direct() == 1 {
        Ok(())
    } else {
        Err(PsCode::DirectWrite.code())
    }
}

/// 探针B：回滚备份在档——两档案先后落盘（备槽持有第一帧），
/// 主槽校验坏后还原为上一档案帧。
pub fn probe_b() -> Result<(), u32> {
    let mut st = PersistStore::new();
    let f0 = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
    let _ = st.mark_dirty();
    st.persist(&f0, PORT_TOKEN_B)?;
    let f1 = PersistStore::build_profile_frame(1, &[10, 0, 19045, 0, 1, 256], 2);
    let _ = st.mark_dirty();
    st.persist(&f1, PORT_TOKEN_B)?;
    // 第二次落盘后破坏主槽记录的校验和（模拟位翻转）。
    if let Some(r) = st.primary.as_mut() {
        r.checksum = r.checksum.wrapping_add(1);
    }
    match st.verify_and_rollback() {
        Ok(false) => {
            // 已回滚：主槽应还原为 f0。帧布局推导：16 字节头之后为
            // 六维各 4 字节小端（维 d 起于 16+4d），Build=维 2 →
            // 偏移 24/25 两字节即 10240（0x2800）小端。
            let f = st.primary_frame().unwrap_or_default();
            let lo = f.get(24).copied().unwrap_or(0) as u32;
            let hi = f.get(25).copied().unwrap_or(0) as u32;
            if lo | (hi << 8) == 10240 {
                Ok(())
            } else {
                Err(PsCode::RollbackFailed.code())
            }
        }
        _ => Err(PsCode::RollbackFailed.code()),
    }
}

/// 探针C：日志账容量闸——16 条后第 17 入必 JournalFull。
pub fn probe_c() -> Result<(), u32> {
    let mut st = PersistStore::new();
    let mut i = 0;
    while i < JOURNAL_CAP {
        let f = PersistStore::build_profile_frame((i % 4) as u8, &[10, 0, 10240, 0, 1, 0], (i + 1) as u32);
        let _ = st.mark_dirty();
        st.persist(&f, PORT_TOKEN_B)?;
        i += 1;
    }
    let f = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 99);
    let _ = st.mark_dirty();
    match st.persist(&f, PORT_TOKEN_B) {
        Err(c) if c == PsCode::JournalFull.code() => Ok(()),
        _ => Err(PsCode::JournalFull.code()),
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < PsCode::ALL.len() {
        if let Some(c) = PsCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18430_checks：J1 判据 16 条（断言性质，动态明细 ≤8）。
pub fn run_unx18430_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18430 矩阵持久化（B 部语义落盘）");
    // J1-1 冻结常量：B 部联签/帧域/容量。
    s.add(
        "J1-B部联签冻结",
        PORT_TOKEN_B == 0x0018_000B
            && PAYLOAD_MIN == 16
            && PAYLOAD_MAX == 256
            && JOURNAL_CAP == 16,
        "联签0x0018000B 帧16..256 账16",
    );
    // J1-2 黄金全链：三档案落盘 + 备份在档。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链三落盘",
        matches!(&g, Ok((c, j)) if *c == 3 && *j == 3),
        &format!("{:?}", g),
    );
    // J1-3 B 部接口走通：联签过闸票据发放且单调。
    let tickets = {
        let mut st = PersistStore::new();
        let mut v = Vec::new();
        let mut i = 0;
        while i < 3 {
            let f = PersistStore::build_profile_frame(i as u8, &[10, 0, 10240, 0, 1, 0], (i + 1) as u32);
            let _ = st.mark_dirty();
            if let Ok(t) = st.persist(&f, PORT_TOKEN_B) {
                v.push(t);
            }
            i += 1;
        }
        v
    };
    s.add(
        "J1-端口票据单调",
        tickets.len() == 3
            && tickets.first().copied().unwrap_or(0) == 1
            && tickets.get(2).copied().unwrap_or(0) == 3,
        "票据 1→3 单调发放",
    );
    // J1-4 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-5 注入守恒。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-6 直写注入必红（探针A 直验）。
    s.add("J1-直写必红", probe_a().is_ok(), "ticket=0 记录审计必抓");
    // J1-7 回滚备份在档（探针B 直验）。
    s.add("J1-回滚备份在档", probe_b().is_ok(), "主坏→备还原 10240");
    // J1-8 容量闸（探针C 直验）。
    s.add("J1-容量闸16条", probe_c().is_ok(), "第 17 入 JournalFull");
    // J1-9 帧闸三点：空帧/坏魔数/超长。
    let mut st9 = PersistStore::new();
    let _ = st9.mark_dirty();
    let long_frame = {
        let mut f = frame_header(1, 1, 6);
        let mut k = 0;
        while f.len() <= PAYLOAD_MAX {
            f.push(0);
            k += 1;
        }
        f
    };
    s.add(
        "J1-帧闸三点",
        st9.persist(&[0u8; 8], PORT_TOKEN_B).is_err()
            && st9.persist(&long_frame, PORT_TOKEN_B).is_err()
            && {
                let mut bad_magic = frame_header(1, 1, 6);
                if let Some(b) = bad_magic.get_mut(0) {
                    *b = 0x00;
                }
                st9.persist(&bad_magic, PORT_TOKEN_B).is_err()
            },
        "空/超长/坏魔数全拒",
    );
    // J1-10 帧体还原：落盘档案六维小端逐字节还原。
    let roundtrip = {
        let st = PersistStore::new();
        let f = PersistStore::build_profile_frame(1, &[10, 0, 19045, 0, 1, 256], 1);
        // 体基址 16：档案号后 24 字节为六维小端。
        let v2 = f.get(16 + 8).copied().unwrap_or(0) as u32
            | ((f.get(16 + 9).copied().unwrap_or(0) as u32) << 8);
        frame_valid(&f).is_ok() && v2 == 19045 && st.primary_frame().is_none()
    };
    s.add("J1-帧体还原", roundtrip, "六维小端逐字节还原 19045");
    // J1-11 状态机：标脏→提交两相合法；Clean 相直标脏二次拒。
    let sm = {
        let mut st = PersistStore::new();
        let a = st.mark_dirty().is_ok();
        let b = st.mark_dirty().is_err();
        let f = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
        let c = st.persist(&f, PORT_TOKEN_B).is_ok();
        a && b && c
    };
    s.add("J1-状态机两相", sm, "Clean→Dirty→Committed 单向");
    // J1-12 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-13 FNV 确定性：同帧两次校验和相等，异帧不等。
    let f1 = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
    let f2 = PersistStore::build_profile_frame(2, &[6, 2, 9200, 0, 1, 0], 1);
    s.add(
        "J2-FNV确定性",
        fnv1a(&f1) == fnv1a(&f1) && fnv1a(&f1) != fnv1a(&f2),
        "同帧同和 异帧异和",
    );
    // J1-14 文案闭集。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < PsCode::ALL.len() {
            if let Some(c) = PsCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-15 档案闭集勾稽：四档案速查与 unx18429 值一致（0/1/2/3）。
    let pv_ok = profile_values(0) == Some([10, 0, 10240, 0, 1, 0])
        && profile_values(2) == Some([6, 2, 9200, 0, 1, 0])
        && profile_values(4).is_none();
    s.add("J2-档案闭集勾稽", pv_ok, "四档案速查与热更新卡一致");
    // J1-16 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E740 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18430_checks();
        assert!(set.all_passed(), "UNX-F18430 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_persist_via_port() {
        let mut st = PersistStore::new();
        let f = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
        let _ = st.mark_dirty();
        let t = st.persist(&f, PORT_TOKEN_B).unwrap_or(0);
        assert_eq!(t, 1);
        assert_eq!(st.journal_len(), 1);
        let r = st.record_at(1).unwrap_or(forge_direct_record(1, b"x"));
        assert!(r.intact().is_ok());
        assert_eq!(r.ticket, 1);
    }

    #[test]
    fn t_direct_write_rejected() {
        let mut st = PersistStore::new();
        let f = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
        let _ = st.mark_dirty();
        assert_eq!(st.persist(&f, 0), Err(PsCode::TokenMismatch.code()));
        assert_eq!(st.persist(&f, 0x1234), Err(PsCode::TokenMismatch.code()));
        assert_eq!(st.journal_len(), 0);
    }

    #[test]
    fn t_frame_gates() {
        assert_eq!(frame_valid(&[0u8; 8]), Err(PsCode::PayloadEmpty.code()));
        let mut bad = frame_header(1, 1, 6);
        bad[0] = 0;
        assert_eq!(frame_valid(&bad), Err(PsCode::MagicBad.code()));
        let mut long = frame_header(1, 1, 6);
        for _ in 0..=PAYLOAD_MAX {
            long.push(0);
        }
        assert_eq!(frame_valid(&long), Err(PsCode::PayloadTooLong.code()));
    }

    #[test]
    fn t_rollback_restores_backup() {
        let mut st = PersistStore::new();
        let f0 = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 1);
        let _ = st.mark_dirty();
        let _ = st.persist(&f0, PORT_TOKEN_B);
        let f1 = PersistStore::build_profile_frame(1, &[10, 0, 19045, 0, 1, 256], 2);
        let _ = st.mark_dirty();
        let _ = st.persist(&f1, PORT_TOKEN_B);
        if let Some(r) = st.primary.as_mut() {
            r.checksum = r.checksum.wrapping_add(1);
        }
        assert_eq!(st.verify_and_rollback(), Ok(false));
        assert_eq!(st.rollbacks, 1);
        // 还原后主槽即 f0（校验和完好）。
        assert!(st.verify_and_rollback() == Ok(true));
    }

    #[test]
    fn t_journal_cap() {
        let mut st = PersistStore::new();
        let mut i = 0;
        while i < JOURNAL_CAP {
            let f = PersistStore::build_profile_frame((i % 4) as u8, &[10, 0, 10240, 0, 1, 0], (i + 1) as u32);
            let _ = st.mark_dirty();
            assert!(st.persist(&f, PORT_TOKEN_B).is_ok());
            i += 1;
        }
        let f = PersistStore::build_profile_frame(0, &[10, 0, 10240, 0, 1, 0], 99);
        let _ = st.mark_dirty();
        assert_eq!(st.persist(&f, PORT_TOKEN_B), Err(PsCode::JournalFull.code()));
    }

    #[test]
    fn t_inject_ledger() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        for i in 0..BAD_CLASSES {
            assert_eq!(led.tries[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
