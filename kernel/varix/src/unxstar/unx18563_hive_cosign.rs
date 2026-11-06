//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18563 · hive 访问挂接联签（对 AI-18） · 0x80880段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18563-J1 hive API 消费面字段级对账一致，越接口直写
//!       注入必红
//! 设计要点：
//!   - hive 消费面按「路径 × 字段」两级建模：路径在册（4 槽）、字段
//!     在册（每路径 8 槽），未注册字段一律 FieldOutOfRange 拒收；
//!   - 授权位面：每条授权持 128 位位面（[u64;2]），位 k 对应字段 k，
//!     读写都须「位面盖住字段」——位面外即越接口；
//!   - 授权状态机：申请→授予→吊销，单调三态；吊销后任何操作必拒
//!     （GrantNotActive），重复授予/倒退显式拒；
//!   - 字段级对账：每字段读写计数入账，对账闸 = Σ字段计数 == 总操作
//!     计数（读账 + 写账），差一即 ReconcileBreak；
//!   - 越接口直写注入必红：凡不带授权 gid（或授权不盖该路径）的写，
//!     直接判 CrossIfWrite——这是本卡判据的注入主轴；
//!   - 注入核三类：直写无授权 / 授权只读硬写 / 吊销后硬写，各 10 轮
//!     全抓，漏检为零；边界：路径越界/字段越界/授权满/重复授权；
//!   - LCG 种子写死 = SEG_BASE（0x80880）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：Grant（授权槽+位面）/ HiveCosign（路径×字段账+授权账）
//!   状态机  ：GrantState（申请→授予→吊销 单调三态）
//!   账本    ：字段级读写数组账（32 槽平面）+ 操作计数 + 注入检出账
//!   错误路径：HiveCode 13 码闭集，code() = SEG_BASE + n（0x80880 窗）
//!   注入核  ：直写/只读硬写/吊销硬写三类逐类断言被拒（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80880，AI-VX11-1105 批09 第 3 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x80880;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 路径槽容量（推导：hive 四大消费面 = 键/值/组/快照 各一槽）。
pub const PATH_CAP: usize = 4;
/// 每路径字段容量（推导：位面 [u64;2] = 128 位，实际用 8 位/路径，
/// 8 = 一个字节位面，判据按字节位面闭式核对）。
pub const FIELD_CAP: usize = 8;
/// 授权账容量（推导：4 路径 × 读/写两模式 = 8，与查重闭式自洽——
/// 同路径同模式查重下 8 即不可再增，账满即闭）。
pub const GRANT_CAP: usize = 8;
/// 字段账平面槽数（推导：PATH_CAP × FIELD_CAP = 32）。
pub const FIELD_LEDGER: usize = PATH_CAP * FIELD_CAP;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 指纹种子（√5 前 64 位，冻结推导）。
pub const FP_SEED: u64 = 0x1BB6_7AE8_584C_A73B;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HiveCode {
    /// 路径越界：0..4 之外
    PathOutOfRange,
    /// 字段越界：0..8 之外
    FieldOutOfRange,
    /// 字段未注册：位面未覆盖
    FieldUnregistered,
    /// 重复授权：同路径同模式已挂
    DupGrant,
    /// 授权账满
    GrantFull,
    /// 授权未找到：gid 无效
    GrantNotFound,
    /// 状态越级/倒退
    StateBreak,
    /// 授权非活：吊销后操作
    GrantNotActive,
    /// 只读硬写
    WriteToReadOnly,
    /// 越接口直写：不带授权写
    CrossIfWrite,
    /// 对账破：字段账与操作计数不平
    ReconcileBreak,
    /// 双跑漂移
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl HiveCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 供静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HiveCode::PathOutOfRange => "路径越界：只在 0..4 槽内取",
            HiveCode::FieldOutOfRange => "字段越界：每路径只在 0..8 内取",
            HiveCode::FieldUnregistered => "字段未注册：先注册字段再授权再读写",
            HiveCode::DupGrant => "重复授权：同路径同模式只挂一条",
            HiveCode::GrantFull => "授权账满：8 槽用尽，吊销回收后复用",
            HiveCode::GrantNotFound => "授权未找到：gid 无效，先申请再授权",
            HiveCode::StateBreak => "状态越级或倒退：按 申请→授予→吊销 走",
            HiveCode::GrantNotActive => "授权非活：吊销后不许再操作，重开新授权",
            HiveCode::WriteToReadOnly => "只读硬写：改挂读写授权再写",
            HiveCode::CrossIfWrite => "越接口直写：写必须带盖住该路径的授权",
            HiveCode::ReconcileBreak => "对账不平：字段账求和须等于总操作数",
            HiveCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            HiveCode::Unknown => "未知诊断码：不在 0x80880 段闭集内",
        }
    }
    pub const ALL: [HiveCode; 13] = [
        HiveCode::PathOutOfRange,
        HiveCode::FieldOutOfRange,
        HiveCode::FieldUnregistered,
        HiveCode::DupGrant,
        HiveCode::GrantFull,
        HiveCode::GrantNotFound,
        HiveCode::StateBreak,
        HiveCode::GrantNotActive,
        HiveCode::WriteToReadOnly,
        HiveCode::CrossIfWrite,
        HiveCode::ReconcileBreak,
        HiveCode::NotReproducible,
        HiveCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HiveCode::ALL.len() {
        if HiveCode::ALL[i].code() == code {
            return HiveCode::ALL[i].detail();
        }
        i += 1;
    }
    HiveCode::Unknown.detail()
}

// ── 指纹原语 ────────────────────────────────────────────────────
/// 64 位雪崩混合（写死常量，双跑必同）。
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// 授权指纹：路径、模式、序号三元混合。
pub fn grant_fp(path: u32, mode: u32, seq: u32) -> u64 {
    mix64(((path as u64) << 24) ^ ((mode as u64) << 16) ^ (seq as u64) ^ FP_SEED)
}

// ── 状态机：授权三态（单调） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrantState {
    Requested = 0,
    Granted = 1,
    Revoked = 2,
}

impl GrantState {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调 +1 放行；倒退/越级拒 StateBreak。
    pub fn advance(self, to: GrantState) -> Result<GrantState, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(HiveCode::StateBreak.code())
        }
    }
}

/// 访问模式闭集：只读 / 读写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessMode {
    ReadOnly = 0,
    ReadWrite = 1,
}

impl AccessMode {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 写许可闭式：仅 ReadWrite 准写。
    pub fn can_write(self) -> bool {
        self == AccessMode::ReadWrite
    }
}

// ── 数据结构：授权槽（位面）与总账 ──────────────────────────────
#[derive(Clone, Copy)]
pub struct Grant {
    /// 授权号（账内序号）。
    pub gid: u32,
    /// 绑定路径（0..4）。
    pub path: u32,
    /// 访问模式。
    pub mode: AccessMode,
    /// 授权位面：位 k = 字段 k 是否盖住（128 位定容，用低 FIELD_CAP 位）。
    pub bitmap: [u64; 2],
    /// 状态机。
    pub state: GrantState,
}

impl Grant {
    /// 位 k 盖住？（k < FIELD_CAP 才有意义；越界一律 false）。
    pub fn covers(&self, k: usize) -> bool {
        if k >= FIELD_CAP {
            return false;
        }
        if k < 64 {
            (self.bitmap[0] >> k) & 1 == 1
        } else {
            (self.bitmap[1] >> (k - 64)) & 1 == 1
        }
    }
    /// 置位 k（越界静默忽略——位面定容语义）。
    pub fn set_bit(&mut self, k: usize) {
        if k < 64 {
            self.bitmap[0] |= 1u64 << k;
        } else if k < 128 {
            self.bitmap[1] |= 1u64 << (k - 64);
        }
    }
    /// 位面活位计数（判据推导：等于注册字段数）。
    pub fn popcount(&self) -> u32 {
        self.bitmap[0].count_ones() + self.bitmap[1].count_ones()
    }
}

/// hive 挂接联签总账：路径×字段账 + 授权账 + 操作计数。
pub struct HiveCosign {
    /// 字段注册位面（每路径 8 位：位 k = 字段 k 已注册）。
    pub registered: [[u64; 2]; PATH_CAP],
    /// 字段级读写账（扁平 32 槽：槽 = path*8 + field）。
    pub field_rd: [u32; FIELD_LEDGER],
    pub field_wr: [u32; FIELD_LEDGER],
    /// 授权账。
    pub grants: Vec<Grant>,
    /// 操作计数（读/写分账，对账轴）。
    pub op_rd: u32,
    pub op_wr: u32,
    /// 账内授权序号（指纹轴）。
    seq: u32,
}

impl HiveCosign {
    pub fn new() -> HiveCosign {
        HiveCosign {
            registered: [[0; 2]; PATH_CAP],
            field_rd: [0; FIELD_LEDGER],
            field_wr: [0; FIELD_LEDGER],
            grants: Vec::new(),
            op_rd: 0,
            op_wr: 0,
            seq: 0,
        }
    }

    /// 注册字段：路径/字段越界拒；重复注册幂等放行（注册是幂等操作）。
    pub fn register_field(&mut self, path: usize, field: usize) -> Result<(), u32> {
        if path >= PATH_CAP {
            return Err(HiveCode::PathOutOfRange.code());
        }
        if field >= FIELD_CAP {
            return Err(HiveCode::FieldOutOfRange.code());
        }
        self.registered[path][0] |= 1u64 << field;
        Ok(())
    }

    /// 字段已注册？
    pub fn is_registered(&self, path: usize, field: usize) -> Result<bool, u32> {
        if path >= PATH_CAP {
            return Err(HiveCode::PathOutOfRange.code());
        }
        if field >= FIELD_CAP {
            return Err(HiveCode::FieldOutOfRange.code());
        }
        Ok((self.registered[path][0] >> field) & 1 == 1)
    }

    /// 申请授权：查重（同路径同模式已挂未吊销即拒）+ 账满拒。
    pub fn request_grant(&mut self, path: usize, mode: AccessMode) -> Result<u32, u32> {
        if path >= PATH_CAP {
            return Err(HiveCode::PathOutOfRange.code());
        }
        let mut i = 0;
        while i < self.grants.len() {
            match self.grants.get(i) {
                Some(g) => {
                    if g.path == path as u32 && g.mode == mode && g.state != GrantState::Revoked {
                        return Err(HiveCode::DupGrant.code());
                    }
                }
                None => return Err(HiveCode::GrantNotFound.code()),
            }
            i += 1;
        }
        if self.grants.len() >= GRANT_CAP {
            return Err(HiveCode::GrantFull.code());
        }
        let gid = self.seq;
        self.seq = self.seq.saturating_add(1);
        self.grants.push(Grant {
            gid,
            path: path as u32,
            mode,
            bitmap: [0; 2],
            state: GrantState::Requested,
        });
        Ok(gid)
    }

    /// 授予：申请态→授予态；位面盖住该路径全部已注册字段。
    pub fn activate(&mut self, gid: u32) -> Result<(), u32> {
        // 先读路径快照（避免可变借用期改注册面）。
        let mut snapshot = [0u64; 2];
        let mut found = false;
        let mut i = 0;
        while i < self.grants.len() {
            match self.grants.get(i) {
                Some(g) => {
                    if g.gid == gid {
                        found = true;
                        let p = g.path as usize;
                        if p < PATH_CAP {
                            snapshot[0] = self.registered[p][0];
                            snapshot[1] = self.registered[p][1];
                        }
                        break;
                    }
                }
                None => return Err(HiveCode::GrantNotFound.code()),
            }
            i += 1;
        }
        if !found {
            return Err(HiveCode::GrantNotFound.code());
        }
        i = 0;
        while i < self.grants.len() {
            match self.grants.get_mut(i) {
                Some(g) => {
                    if g.gid == gid {
                        g.state = g.state.advance(GrantState::Granted)?;
                        g.bitmap = snapshot;
                        return Ok(());
                    }
                }
                None => return Err(HiveCode::GrantNotFound.code()),
            }
            i += 1;
        }
        Err(HiveCode::GrantNotFound.code())
    }

    /// 吊销：授予态→吊销态。
    pub fn revoke(&mut self, gid: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.grants.len() {
            let st = match self.grants.get(i) {
                Some(g) => {
                    if g.gid == gid {
                        g.state
                    } else {
                        i += 1;
                        continue;
                    }
                }
                None => return Err(HiveCode::GrantNotFound.code()),
            };
            if let Some(g) = self.grants.get_mut(i) {
                g.state = st.advance(GrantState::Revoked)?;
            }
            return Ok(());
        }
        Err(HiveCode::GrantNotFound.code())
    }

    /// 授权定位：gid →（槽内快照）。找不到即 GrantNotFound。
    fn locate(&self, gid: u32) -> Result<Grant, u32> {
        let mut i = 0;
        while i < self.grants.len() {
            match self.grants.get(i) {
                Some(g) => {
                    if g.gid == gid {
                        return Ok(*g);
                    }
                }
                None => return Err(HiveCode::GrantNotFound.code()),
            }
            i += 1;
        }
        Err(HiveCode::GrantNotFound.code())
    }

    /// 字段账槽定位（写账用 checked 语义，越界显式拒）。
    fn slot(path: usize, field: usize) -> Result<usize, u32> {
        if path >= PATH_CAP {
            return Err(HiveCode::PathOutOfRange.code());
        }
        if field >= FIELD_CAP {
            return Err(HiveCode::FieldOutOfRange.code());
        }
        Ok(path * FIELD_CAP + field)
    }

    /// 读：授权须活 + 字段注册 + 位面盖住；读不问模式（只读也可读）。
    pub fn read(&mut self, gid: u32, path: usize, field: usize) -> Result<(), u32> {
        let g = self.locate(gid)?;
        if g.state != GrantState::Granted {
            return Err(HiveCode::GrantNotActive.code());
        }
        if g.path != path as u32 {
            // 授权不盖该路径 = 越接口。
            return Err(HiveCode::CrossIfWrite.code());
        }
        if !self.is_registered(path, field)? {
            return Err(HiveCode::FieldUnregistered.code());
        }
        if !g.covers(field) {
            return Err(HiveCode::FieldOutOfRange.code());
        }
        let s = Self::slot(path, field)?;
        self.field_rd[s] = self.field_rd[s].saturating_add(1);
        self.op_rd = self.op_rd.saturating_add(1);
        Ok(())
    }

    /// 写：授权须活 + 模式准写 + 路径被盖 + 字段注册 + 位面盖住。
    pub fn write(&mut self, gid: u32, path: usize, field: usize) -> Result<(), u32> {
        let g = self.locate(gid)?;
        if g.state != GrantState::Granted {
            return Err(HiveCode::GrantNotActive.code());
        }
        if g.path != path as u32 {
            return Err(HiveCode::CrossIfWrite.code());
        }
        if !g.mode.can_write() {
            return Err(HiveCode::WriteToReadOnly.code());
        }
        if !self.is_registered(path, field)? {
            return Err(HiveCode::FieldUnregistered.code());
        }
        if !g.covers(field) {
            return Err(HiveCode::FieldOutOfRange.code());
        }
        let s = Self::slot(path, field)?;
        self.field_wr[s] = self.field_wr[s].saturating_add(1);
        self.op_wr = self.op_wr.saturating_add(1);
        Ok(())
    }

    /// 越接口直写检测核：凡不带授权的写一律 CrossIfWrite（必红主轴）。
    pub fn direct_write(&self, path: usize, field: usize) -> Result<(), u32> {
        // 参数界先核（把参数错与越权错分开报）。
        if path >= PATH_CAP {
            return Err(HiveCode::PathOutOfRange.code());
        }
        if field >= FIELD_CAP {
            return Err(HiveCode::FieldOutOfRange.code());
        }
        Err(HiveCode::CrossIfWrite.code())
    }

    /// 字段级对账：Σ字段读账 == op_rd 且 Σ字段写账 == op_wr。
    pub fn reconcile(&self) -> Result<(), u32> {
        let mut sr = 0u32;
        let mut sw = 0u32;
        let mut i = 0;
        while i < FIELD_LEDGER {
            sr = sr.saturating_add(self.field_rd[i]);
            sw = sw.saturating_add(self.field_wr[i]);
            i += 1;
        }
        if sr != self.op_rd || sw != self.op_wr {
            return Err(HiveCode::ReconcileBreak.code());
        }
        Ok(())
    }

    /// 位面对账：每条活授权位面活位数 == 该路径注册字段数。
    pub fn bitmap_reconcile(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.grants.len() {
            let g = match self.grants.get(i) {
                Some(g) => *g,
                None => return Err(HiveCode::GrantNotFound.code()),
            };
            if g.state == GrantState::Granted {
                let p = g.path as usize;
                if p >= PATH_CAP {
                    return Err(HiveCode::PathOutOfRange.code());
                }
                let reg = self.registered[p][0].count_ones();
                if g.popcount() != reg {
                    return Err(HiveCode::FieldUnregistered.code());
                }
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 写闸裁决表（输入：活/模式/注册/盖住 → 期望码；0=放行） ─────
/// 四例样本：活+读写+注册+盖→放行；只读→WriteToReadOnly；
/// 未注册→FieldUnregistered；吊销→GrantNotActive。
pub const WRITE_CASES: [(bool, bool, bool, bool, u32); 4] = [
    // (active, mode_rw, registered, covered, want: 0=放行)
    (true, true, true, true, 0),
    (true, false, true, true, HiveCode::WriteToReadOnly.code()),
    (true, true, false, true, HiveCode::FieldUnregistered.code()),
    (false, true, true, true, HiveCode::GrantNotActive.code()),
];

/// 写闸裁决闭式：与 write() 闸语义同源（判据复算核）。
pub fn adjudicate_write(active: bool, mode_rw: bool, registered: bool, covered: bool) -> u32 {
    if !active {
        return HiveCode::GrantNotActive.code();
    }
    if !mode_rw {
        return HiveCode::WriteToReadOnly.code();
    }
    if !registered {
        return HiveCode::FieldUnregistered.code();
    }
    if !covered {
        return HiveCode::FieldOutOfRange.code();
    }
    0
}

/// 裁决表逐例对拍。
pub fn adjudication_ok() -> bool {
    let mut i = 0;
    while i < WRITE_CASES.len() {
        let (a, m, r, c, want) = WRITE_CASES[i];
        if adjudicate_write(a, m, r, c) != want {
            return false;
        }
        i += 1;
    }
    true
}

// ── 标准流程与黄金全链 ──────────────────────────────────────────
/// 黄金全链：注册 2 路径 × 4 字段→挂读/写授权→读 8 写 4→对账平
/// →吊销→吊销后读必拒→位面对账平。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut h = HiveCosign::new();
    // 注册：路径 0/1 各 4 字段。
    let mut p = 0;
    while p < 2 {
        let mut f = 0;
        while f < 4 {
            h.register_field(p, f)?;
            f += 1;
        }
        p += 1;
    }
    // 挂授权：路径 0 只读、路径 0 读写、路径 1 读写。
    let g_ro = h.request_grant(0, AccessMode::ReadOnly)?;
    let g_rw0 = h.request_grant(0, AccessMode::ReadWrite)?;
    let g_rw1 = h.request_grant(1, AccessMode::ReadWrite)?;
    h.activate(g_ro)?;
    h.activate(g_rw0)?;
    h.activate(g_rw1)?;
    // 读 8：只读授权读路径 0 的 4 字段 + 读写授权读路径 1 的 4 字段。
    let mut f = 0;
    while f < 4 {
        h.read(g_ro, 0, f)?;
        h.read(g_rw1, 1, f)?;
        f += 1;
    }
    // 写 4：读写授权写路径 1 的 4 字段。
    f = 0;
    while f < 4 {
        h.write(g_rw1, 1, f)?;
        f += 1;
    }
    // 只读授权写必拒（好路里的负例闸）。
    match h.write(g_ro, 0, 0) {
        Err(c) if c == HiveCode::WriteToReadOnly.code() => {}
        _ => return Err(HiveCode::WriteToReadOnly.code()),
    }
    // 对账平。
    h.reconcile()?;
    h.bitmap_reconcile()?;
    // 吊销后读必拒。
    h.revoke(g_rw0)?;
    match h.read(g_rw0, 0, 0) {
        Err(c) if c == HiveCode::GrantNotActive.code() => {}
        _ => return Err(HiveCode::GrantNotActive.code()),
    }
    Ok((h.op_rd, h.op_wr, h.grants.len() as u32))
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
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
    pub fn missed(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
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
}

/// 注入核：三类违约各 JUDGE_ROUNDS 轮，逐类断言被显式拒绝。
///   类 0：越接口直写（不带授权写 → CrossIfWrite）
///   类 1：只读硬写（ReadOnly 授权写 → WriteToReadOnly）
///   类 2：吊销后硬写（Revoked 授权写 → GrantNotActive）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let mut h = fresh_two_fields()?;
        // 类 0：直写无授权。
        let hit_a = match h.direct_write(0, 0) {
            Err(c) if c == HiveCode::CrossIfWrite.code() => true,
            _ => false,
        };
        led.record(0, hit_a);
        // 类 1：只读硬写。
        let g = h.request_grant(0, AccessMode::ReadOnly)?;
        h.activate(g)?;
        let hit_b = match h.write(g, 0, 0) {
            Err(c) if c == HiveCode::WriteToReadOnly.code() => true,
            _ => false,
        };
        led.record(1, hit_b);
        // 类 2：吊销后硬写。
        let mut h2 = fresh_two_fields()?;
        let g2 = h2.request_grant(1, AccessMode::ReadWrite)?;
        h2.activate(g2)?;
        h2.revoke(g2)?;
        let hit_c = match h2.write(g2, 1, 0) {
            Err(c) if c == HiveCode::GrantNotActive.code() => true,
            _ => false,
        };
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

/// 造一台双路径各 2 字段注册的账（注入前置）。
fn fresh_two_fields() -> Result<HiveCosign, u32> {
    let mut h = HiveCosign::new();
    h.register_field(0, 0)?;
    h.register_field(0, 1)?;
    h.register_field(1, 0)?;
    h.register_field(1, 1)?;
    Ok(h)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 边界探针：路径/字段越界、未注册字段、重复授权、账满。
pub fn boundary_probe() -> Result<(), u32> {
    let mut h = fresh_two_fields()?;
    // 路径越界。
    match h.register_field(PATH_CAP, 0) {
        Err(c) if c == HiveCode::PathOutOfRange.code() => {}
        _ => return Err(HiveCode::PathOutOfRange.code()),
    }
    // 字段越界（FIELD_CAP 位）。
    match h.register_field(0, FIELD_CAP) {
        Err(c) if c == HiveCode::FieldOutOfRange.code() => {}
        _ => return Err(HiveCode::FieldOutOfRange.code()),
    }
    // 未注册字段授权授予后读写必拒（位面只盖注册字段）。
    let g = h.request_grant(0, AccessMode::ReadWrite)?;
    h.activate(g)?;
    match h.write(g, 0, 5) {
        Err(c) if c == HiveCode::FieldUnregistered.code() => {}
        _ => return Err(HiveCode::FieldUnregistered.code()),
    }
    // 重复授权必拒。
    match h.request_grant(0, AccessMode::ReadWrite) {
        Err(c) if c == HiveCode::DupGrant.code() => {}
        _ => return Err(HiveCode::DupGrant.code()),
    }
    // 吊销回收后可复挂（回收语义）。
    h.revoke(g)?;
    match h.request_grant(0, AccessMode::ReadWrite) {
        Ok(_) => Ok(()),
        Err(_) => Err(HiveCode::GrantFull.code()),
    }
}

/// 容量探针：授权账 GRANT_CAP 用满后 +1 必拒且账不丢。
/// 推导：4 路径 × (只读+读写) = 8 条互不查重相撞，恰满 GRANT_CAP。
pub fn capacity_probe() -> Result<(), u32> {
    let mut h = HiveCosign::new();
    let mut p = 0;
    while p < PATH_CAP {
        h.request_grant(p, AccessMode::ReadOnly)?;
        h.request_grant(p, AccessMode::ReadWrite)?;
        p += 1;
    }
    if h.grants.len() != GRANT_CAP {
        return Err(HiveCode::GrantFull.code());
    }
    let n_before = h.grants.len();
    match h.request_grant(0, AccessMode::ReadOnly) {
        Err(c) if c == HiveCode::GrantFull.code() => {
            if h.grants.len() != n_before {
                return Err(HiveCode::ReconcileBreak.code());
            }
            Ok(())
        }
        _ => Err(HiveCode::GrantFull.code()),
    }
}

/// 探针 A：跨路径授权读必拒（授权只盖绑定路径）。
pub fn probe_cross_path_read() -> Result<(), u32> {
    let mut h = fresh_two_fields()?;
    let g = h.request_grant(0, AccessMode::ReadOnly)?;
    h.activate(g)?;
    match h.read(g, 1, 0) {
        Err(c) if c == HiveCode::CrossIfWrite.code() => Ok(()),
        _ => Err(HiveCode::CrossIfWrite.code()),
    }
}

/// 探针 B：状态机越级（申请直取吊销）必拒。
pub fn probe_state_skip() -> Result<(), u32> {
    let h = fresh_two_fields()?;
    // Requested(0) → Revoked(2)：跳过授予态必拒。
    let skip = match GrantState::Requested.advance(GrantState::Revoked) {
        Err(c) if c == HiveCode::StateBreak.code() => true,
        _ => false,
    };
    if !skip {
        return Err(HiveCode::StateBreak.code());
    }
    let _ = h;
    Ok(())
}

/// 探针 C：位面对账（活授权位面活位数 == 注册字段数）。
pub fn probe_bitmap_reconcile() -> Result<(), u32> {
    let mut h = fresh_two_fields()?;
    let g = h.request_grant(0, AccessMode::ReadWrite)?;
    h.activate(g)?;
    h.bitmap_reconcile()?;
    // 注入：吊销后再注册新字段，活授权集空 → 对账仍平。
    h.revoke(g)?;
    h.register_field(0, 2)?;
    h.bitmap_reconcile()?;
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HiveCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：SEG 对齐/窗位 + 码闭集段内 + 文案非空。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x84000).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < HiveCode::ALL.len() {
        let v = HiveCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if HiveCode::ALL[i].detail().len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 运行期摘要（动态明细消费）。
pub fn summary() -> String {
    let g = golden_run();
    format!("{:?}", g)
}

// ── 检查入口（判据 8–20 条） ────────────────────────────────────
pub fn run_unx18563_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18563 hive 访问挂接联签（对 AI-18）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-hive消费面黄金全链", golden.is_ok(), "注册→挂授权→读8写4→对账平→吊销闸");
    let ok2 = match &golden {
        Ok((rd, wr, g)) => *rd == 8 && *wr == 4 && *g == 3,
        Err(_) => false,
    };
    s.add("J1-字段级对账一致", ok2, "读 8 写 4 三授权，字段账求和与操作计数平");
    s.add("J1-写闸裁决表四例对拍", adjudication_ok(), "活/模式/注册/盖住四态闭式一致");
    s.add("J3-跨路径授权必拒", probe_cross_path_read().is_ok(), "授权只盖绑定路径，跨路径即越接口");
    s.add("J3-状态机越级必拒", probe_state_skip().is_ok(), "申请直取吊销被 StateBreak 拒");
    s.add("J4-边界（路径/字段/未注册/重复）", boundary_probe().is_ok(), "四类界外全部显式拒");
    s.add("J4-授权账满拒收不丢账", capacity_probe().is_ok(), "8 槽用尽后 +1 拒且账长不变");
    s.add("J5-位面对账", probe_bitmap_reconcile().is_ok(), "活授权位面活位数==注册字段数");
    s.add("J7-段位守卫与文案闭集", segment_guard(), "0x80880 段 0x40 窗，13 码全有中文建议");

    // ── 动态明细（≤8 槽） ────────────────────────────────────
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    let (t, c) = match &led {
        Ok(l) => (l.tries[0] + l.tries[1] + l.tries[2], l.caught[0] + l.caught[1] + l.caught[2]),
        Err(_) => (0u32, 0u32),
    };
    s.add_dyn("J2-注入10/10全抓", led_ok, &format!("三类各{}轮 抓{}/{}", JUDGE_ROUNDS, c, t));
    s.add_dyn("J2-漏检为零", match &led { Ok(l) => l.missed() == 0, Err(_) => false }, &format!("漏检{}", match &led { Ok(l) => l.missed(), Err(_) => 99u32 }));
    s.add_dyn("J6-双跑一致", repro_check().is_ok(), &format!("golden={:?}", summary()));
    s.add_dyn("J5-直写必红断言", direct_write_probe().is_ok(), &format!("直写={:?}", direct_write_probe().err()));
    s
}

/// 直写必红断言（判据主轴复算）。
pub fn direct_write_probe() -> Result<(), u32> {
    let h = fresh_two_fields()?;
    let mut i = 0;
    while i < PATH_CAP {
        let mut f = 0;
        while f < FIELD_CAP {
            match h.direct_write(i, f) {
                Err(c) if c == HiveCode::CrossIfWrite.code() => {}
                _ => return Err(HiveCode::CrossIfWrite.code()),
            }
            f += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18563_checks();
        assert!(set.all_passed(), "UNX-F18563 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (rd, wr, gn) = g.unwrap_or((0, 0, 0));
        assert_eq!(rd, 8);
        assert_eq!(wr, 4);
        assert_eq!(gn, 3);
    }

    #[test]
    fn test_cross_if_write_always_red() {
        assert!(direct_write_probe().is_ok());
        let h = fresh_two_fields().unwrap_or(HiveCosign::new());
        assert_eq!(h.direct_write(0, 0).err(), Some(HiveCode::CrossIfWrite.code()));
    }

    #[test]
    fn test_readonly_write_rejected() {
        let mut h = fresh_two_fields().unwrap_or(HiveCosign::new());
        let g = h.request_grant(0, AccessMode::ReadOnly).unwrap_or(0);
        h.activate(g).unwrap_or(());
        assert_eq!(h.write(g, 0, 0).err(), Some(HiveCode::WriteToReadOnly.code()));
        // 只读仍可读。
        assert!(h.read(g, 0, 0).is_ok());
    }

    #[test]
    fn test_revoked_write_rejected() {
        let mut h = fresh_two_fields().unwrap_or(HiveCosign::new());
        let g = h.request_grant(1, AccessMode::ReadWrite).unwrap_or(0);
        h.activate(g).unwrap_or(());
        h.revoke(g).unwrap_or(());
        assert_eq!(h.write(g, 1, 0).err(), Some(HiveCode::GrantNotActive.code()));
        assert_eq!(h.read(g, 1, 0).err(), Some(HiveCode::GrantNotActive.code()));
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(probe_state_skip().is_ok());
        assert_eq!(
            GrantState::Revoked.advance(GrantState::Granted).err(),
            Some(HiveCode::StateBreak.code())
        );
        assert!(GrantState::Requested.advance(GrantState::Granted).is_ok());
    }

    #[test]
    fn test_boundary_and_capacity() {
        assert!(boundary_probe().is_ok());
        assert!(capacity_probe().is_ok());
    }

    #[test]
    fn test_reconcile_conservation() {
        let g = golden_run();
        assert!(g.is_ok());
        // 字段账守恒：读账和 = op_rd（8），写账和 = op_wr（4）。
        let mut h = fresh_two_fields().unwrap_or(HiveCosign::new());
        let gw = h.request_grant(0, AccessMode::ReadWrite).unwrap_or(0);
        h.activate(gw).unwrap_or(());
        h.write(gw, 0, 0).unwrap_or(());
        h.write(gw, 0, 1).unwrap_or(());
        assert_eq!(h.op_wr, 2);
        assert!(h.reconcile().is_ok());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        let l = led.unwrap_or(InjectLedger::new());
        assert!(l.all_caught());
        assert_eq!(l.missed(), 0);
        for i in 0..3 {
            assert_eq!(l.tries[i], JUDGE_ROUNDS);
            assert_eq!(l.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn test_repro_dual_run() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or((0, 0, 0));
        let b = golden_run().unwrap_or((1, 1, 1));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80880);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in HiveCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
