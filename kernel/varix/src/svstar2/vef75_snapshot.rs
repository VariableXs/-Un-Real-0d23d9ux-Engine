//! vef75_snapshot —— VE-F5423 状态快照机制（AA02 组第三单）
//! ============================================================
//! 职责（锚点原文逐条落实）：
//!   · 快照两件：
//!     周期快照——权威状态周期快照（新连接补发+断线恢复用）
//!     快照压缩——快照压缩传输（只存与上帧差异+周期全量锚点）
//!   · 错误路径：快照积压→压缩率调优；压缩失真→哈希校验；锚点丢失→全量重发
//!   · 性能：快照 O(状态量)；压缩 O(差异)；重发 O(全量)
//!   · 对接：F5422 权威上游；F5424 增量消费；F5431 测试
//!
//! 三大机制：
//!   机一 · 周期快照器：按期打全量快照（SNAPSHOT_PERIOD），逾期 0xED02；
//!         保留窗口 SNAPSHOT_KEEP 滚动淘汰（积压治理）；最新快照供
//!         新连接补发与断线恢复（恢复面 ResumeDesk）。
//!   机二 · 差量压缩器：只存与上帧差异（键级 dirty 集）；压缩率统计
//!         （千分比）；压缩率低于底线 → 0xED08 拦截原样传输（压缩
//!         纪律）；底线调参需 Maintainer 双签（0xED0A）。
//!   机三 · 锚点+哈希校验：delta 链应用后终值封印必须 == 当前全量封印
//!         （哈希校验 0xED01 失真即拒收→全量重发标记）；全量锚点周期
//!         ANCHOR_PERIOD 兜底，锚点封印对不上 → 0xED04 丢失→全量重发。
//!
//! 诊断码段 0xED（十码，本单独占）：
//!   0xED01 压缩失真（哈希校验不过）
//!   0xED02 快照周期逾期
//!   0xED03 delta 应用序破坏（基线封印不衔接）
//!   0xED04 锚点丢失（全量重发触发）
//!   0xED05 快照未注册（消费未入册快照）
//!   0xED06 积压超限（保留窗口压穿）
//!   0xED07 恢复请求快照 id 无效
//!   0xED08 压缩率低于底线（原样传输拦截）
//!   0xED09 越权（Runtime 调 Maintainer 操作）
//!   0xED0A 压缩底线调参缺双签
//!
//! 两权分立：Maintainer（打快照、调压缩底线-双签、锚点强制重打）/
//!   Runtime（压缩、应用、恢复请求、审计）。mount 唯一入口；
//!   FNV-1a 64 封印 + 篡改检出。
//! ============================================================

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================
// 第一节 · 全局常量
// ============================================================

/// 快照机制 schema 版本
pub const SNAP_SCHEMA_VER: u32 = 1;
/// 周期快照周期（tick）
pub const SNAPSHOT_PERIOD: u64 = 64;
/// 快照保留窗口（滚动淘汰）
pub const SNAPSHOT_KEEP: usize = 4;
/// 全量锚点周期（tick，快照级兜底）
pub const ANCHOR_PERIOD: u64 = 256;
/// 压缩率底线默认值（‰）：压缩后/原始 低于此值视为原样传输
pub const COMPRESSION_FLOOR_DEFAULT: u32 = 300;
/// delta 链最大长度（超过强制全量锚点）
pub const DELTA_CHAIN_MAX: usize = 8;
/// 审计账满容
pub const AUDIT_CAP: usize = 256;
/// 双签签名人最小长度
pub const SIGNER_MIN_LEN: usize = 2;

// ============================================================
// 第二节 · 诊断码段 0xED
// ============================================================

pub const XC_HASH_MISMATCH: u16 = 0xED01; // 压缩失真：哈希校验不过
pub const XC_PERIOD_LATE: u16 = 0xED02; // 快照周期逾期
pub const XC_CHAIN_BREAK: u16 = 0xED03; // delta 应用序破坏
pub const XC_ANCHOR_LOST: u16 = 0xED04; // 锚点丢失
pub const XC_SNAP_UNKNOWN: u16 = 0xED05; // 快照未注册
pub const XC_BACKLOG_BURST: u16 = 0xED06; // 积压超限
pub const XC_RESUME_BAD_ID: u16 = 0xED07; // 恢复快照 id 无效
pub const XC_BELOW_FLOOR: u16 = 0xED08; // 压缩率低于底线
pub const XC_ROLE: u16 = 0xED09; // 越权
pub const XC_FLOOR_SIGN: u16 = 0xED0A; // 调参缺双签

// ============================================================
// 第三节 · FNV-1a 64
// ============================================================

pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

pub fn seal_str(s: &str) -> u64 {
    fnv1a64(s.as_bytes())
}

pub fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(buf[n] as char);
    }
}

// ============================================================
// 第四节 · 快照与 delta 数据结构
// ============================================================

/// 全量快照条目
#[derive(Clone)]
pub struct FullSnapshot {
    pub snap_id: u32,
    pub tick: u64,
    /// 快照时权威全量封印（F5422 store_seal 传入——双写对拍锚）
    pub store_seal: u64,
    /// 快照时全库最高版本
    pub global_ver: u64,
}

/// 差量包条目
#[derive(Clone)]
pub struct DeltaPack {
    pub pack_id: u32,
    pub tick: u64,
    /// 基线快照 id（必须衔接——链式完整性）
    pub base_snap_id: u32,
    /// 期望的上一封印（基线封印或上一 delta 后封印）
    pub prev_seal: u64,
    /// 本包应用后封印（哈希校验锚）
    pub post_seal: u64,
    /// 差异键数（压缩 O(差异) 的度量）
    pub dirty_count: usize,
    /// 原始规模（键全量数——压缩率分母）
    pub total_keys: usize,
}

/// 审计事件闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditEvent {
    SnapshotTaken,
    PeriodLate,
    SnapshotEvicted,
    DeltaCompressed,
    BelowFloor,
    HashMismatch,
    ChainBreak,
    AnchorTaken,
    AnchorLost,
    FullResent,
    ResumeServed,
    ResumeBadId,
    FloorChanged,
    DualSigned,
    RoleViolation,
}

impl AuditEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEvent::SnapshotTaken => "snapshot-taken",
            AuditEvent::PeriodLate => "period-late",
            AuditEvent::SnapshotEvicted => "snapshot-evicted",
            AuditEvent::DeltaCompressed => "delta-compressed",
            AuditEvent::BelowFloor => "below-floor",
            AuditEvent::HashMismatch => "hash-mismatch",
            AuditEvent::ChainBreak => "chain-break",
            AuditEvent::AnchorTaken => "anchor-taken",
            AuditEvent::AnchorLost => "anchor-lost",
            AuditEvent::FullResent => "full-resent",
            AuditEvent::ResumeServed => "resume-served",
            AuditEvent::ResumeBadId => "resume-bad-id",
            AuditEvent::FloorChanged => "floor-changed",
            AuditEvent::DualSigned => "dual-signed",
            AuditEvent::RoleViolation => "role-violation",
        }
    }
}

struct AuditEntry {
    tick: u64,
    ev: AuditEvent,
    code: u16,
}

pub struct AuditLog {
    entries: Vec<AuditEntry>,
    dropped: usize,
}

impl AuditLog {
    fn new() -> AuditLog {
        AuditLog { entries: Vec::new(), dropped: 0 }
    }

    fn record(&mut self, tick: u64, ev: AuditEvent, code: u16) {
        if self.entries.len() >= AUDIT_CAP {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(AuditEntry { tick, ev, code });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn last_of(&self, ev: AuditEvent) -> Option<(u64, u16)> {
        for e in self.entries.iter().rev() {
            if e.ev == ev {
                return Some((e.tick, e.code));
            }
        }
        None
    }

    pub fn count_of(&self, ev: AuditEvent) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.ev == ev {
                n += 1;
            }
        }
        n
    }

    pub fn at(&self, i: usize) -> Option<(u64, AuditEvent, u16)> {
        if i >= self.entries.len() {
            return None;
        }
        let e = &self.entries[i];
        Some((e.tick, e.ev, e.code))
    }
}

// ============================================================
// 第五节 · 角色与状态
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 维护权：打快照、强制锚点、调压缩底线（双签）
    Maintainer,
    /// 运行权：压缩、应用校验、恢复请求、审计
    Runtime,
}

/// 恢复响应闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResumeOutcome {
    /// 从快照 + 后续 delta 链恢复
    ServedFromSnapshot,
    /// 快照失效 → 全量重发
    FullResend,
}

// ============================================================
// 第六节 · 快照引擎
// ============================================================

pub struct SnapEngine {
    pub role: Role,
    /// 快照册（滚动窗口）
    snaps: Vec<FullSnapshot>,
    next_snap_id: u32,
    /// delta 链（衔接最新快照）
    deltas: Vec<DeltaPack>,
    next_pack_id: u32,
    /// 最近一次快照 tick（周期纪律）
    last_snap_tick: Option<u64>,
    /// 全量锚点
    anchor: Option<(u64, u64)>, // (tick, store_seal)
    /// delta 链长度（超 DELTA_CHAIN_MAX 强制锚点）
    delta_chain_len: usize,
    /// 压缩率底线（‰）
    pub floor_permille: u32,
    /// 调参签名（双签）
    floor_signers: Vec<String>,
    /// 全量重发计数
    full_resends: u32,
    /// 恢复服务计数
    resume_served: u64,
    /// 周期逾期计数
    period_lates: u32,
    seal: u64,
    mounted: bool,
    pub audit: AuditLog,
}

impl SnapEngine {
    /// mount 唯一入口
    pub fn mount(role: Role, tick: u64) -> SnapEngine {
        let mut src = String::new();
        src.push_str("vef75-snapshot|role=");
        src.push_str(match role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, SNAP_SCHEMA_VER as u64);
        SnapEngine {
            role,
            snaps: Vec::new(),
            next_snap_id: 1,
            deltas: Vec::new(),
            next_pack_id: 1,
            last_snap_tick: None,
            anchor: None,
            delta_chain_len: 0,
            floor_permille: COMPRESSION_FLOOR_DEFAULT,
            floor_signers: Vec::new(),
            full_resends: 0,
            resume_served: 0,
            period_lates: 0,
            seal: seal_str(&src),
            mounted: true,
            audit: AuditLog::new(),
        }
    }

    pub fn seal(&self) -> u64 {
        self.seal
    }

    pub fn verify_seal(&self) -> bool {
        let mut src = String::new();
        src.push_str("vef75-snapshot|role=");
        src.push_str(match self.role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, SNAP_SCHEMA_VER as u64);
        seal_str(&src) == self.seal
    }

    fn require_maintainer(&mut self, tick: u64) -> Result<(), u16> {
        if self.role != Role::Maintainer {
            self.audit.record(tick, AuditEvent::RoleViolation, XC_ROLE);
            return Err(XC_ROLE);
        }
        Ok(())
    }

    // ---------- 机一 · 周期快照器 ----------

    /// 打全量快照（Maintainer；store_seal/global_ver 由 F5422 权威侧传入）
    pub fn take_snapshot(
        &mut self,
        store_seal: u64,
        global_ver: u64,
        tick: u64,
    ) -> Result<u32, u16> {
        self.require_maintainer(tick)?;
        // 周期纪律：距上次快照不足半周期 → 节流拒（防快照风暴）
        if let Some(last) = self.last_snap_tick {
            if tick > last && tick - last < SNAPSHOT_PERIOD / 2 {
                self.audit.record(tick, AuditEvent::PeriodLate, XC_PERIOD_LATE);
                return Err(XC_PERIOD_LATE);
            }
        }
        let id = self.next_snap_id;
        self.next_snap_id += 1;
        self.snaps.push(FullSnapshot {
            snap_id: id,
            tick,
            store_seal,
            global_ver,
        });
        self.last_snap_tick = Some(tick);
        // 保留窗口滚动淘汰（积压治理）
        while self.snaps.len() > SNAPSHOT_KEEP {
            self.snaps.remove(0);
            self.audit.record(tick, AuditEvent::SnapshotEvicted, 0);
        }
        // 新快照使既有 delta 链作废（链基已更新）
        self.deltas.clear();
        self.delta_chain_len = 0;
        self.audit.record(tick, AuditEvent::SnapshotTaken, 0);
        Ok(id)
    }

    /// 周期纪律检查：距上次快照超 SNAPSHOT_PERIOD 未打 → 0xED02（逾期告警）
    pub fn period_check(&mut self, tick: u64) -> Result<(), u16> {
        match self.last_snap_tick {
            None => Ok(()), // 尚无快照：首次不判逾期
            Some(last) => {
                if tick > last && tick - last > SNAPSHOT_PERIOD {
                    self.period_lates += 1;
                    self.audit.record(tick, AuditEvent::PeriodLate, XC_PERIOD_LATE);
                    Err(XC_PERIOD_LATE)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// 最新快照（新连接补发用）
    pub fn latest_snapshot(&self) -> Option<&FullSnapshot> {
        if self.snaps.is_empty() {
            return None;
        }
        Some(&self.snaps[self.snaps.len() - 1])
    }

    pub fn snap_count(&self) -> usize {
        self.snaps.len()
    }

    /// 快照在册查询
    pub fn snap_by_id(&self, id: u32) -> Option<&FullSnapshot> {
        for s in self.snaps.iter() {
            if s.snap_id == id {
                return Some(s);
            }
        }
        None
    }

    // ---------- 机二 · 差量压缩器 ----------

    /// 压缩差量包（Runtime 可用）：
    ///   base_snap_id 必须在册且是最新快照（链式衔接）；
    ///   压缩率 = dirty/total 千分比，低于底线 → 0xED08（原样传输拦截）。
    pub fn compress(
        &mut self,
        base_snap_id: u32,
        prev_seal: u64,
        post_seal: u64,
        dirty_count: usize,
        total_keys: usize,
        tick: u64,
    ) -> Result<u32, u16> {
        if !self.mounted {
            return Err(XC_ROLE);
        }
        // 基线快照必须在册
        let base = match self.snap_by_id(base_snap_id) {
            Some(s) => s,
            None => {
                self.audit.record(tick, AuditEvent::ResumeBadId, XC_SNAP_UNKNOWN);
                return Err(XC_SNAP_UNKNOWN);
            }
        };
        // 基线必须是最新的（链式衔接：只允许从最新快照生长 delta）
        let latest_id = self.latest_snapshot().map(|s| s.snap_id).unwrap_or(0);
        if base_snap_id != latest_id {
            self.audit.record(tick, AuditEvent::ChainBreak, XC_CHAIN_BREAK);
            return Err(XC_CHAIN_BREAK);
        }
        // 前封印衔接：首包接快照封印，后续包接上一包 post 封印
        let expect_prev = if self.deltas.is_empty() {
            base.store_seal
        } else {
            self.deltas[self.deltas.len() - 1].post_seal
        };
        if prev_seal != expect_prev {
            self.audit.record(tick, AuditEvent::ChainBreak, XC_CHAIN_BREAK);
            return Err(XC_CHAIN_BREAK);
        }
        // 压缩率底线（压缩纪律：dirty/total ≥ floor 视为有效差异包；
        //   低于底线 = 近乎全量原样传输 → 拦截改走全量锚点）
        if total_keys > 0 {
            // dirty/total × 1000 与 floor 比较（定点）
            let rate = ((dirty_count as u64) * 1000 / total_keys as u64) as u32;
            if rate < self.floor_permille {
                self.audit.record(tick, AuditEvent::BelowFloor, XC_BELOW_FLOOR);
                return Err(XC_BELOW_FLOOR);
            }
        }
        let id = self.next_pack_id;
        self.next_pack_id += 1;
        self.deltas.push(DeltaPack {
            pack_id: id,
            tick,
            base_snap_id,
            prev_seal,
            post_seal,
            dirty_count,
            total_keys,
        });
        self.delta_chain_len += 1;
        // 链长超限 → 强制锚点告警（压缩率调优的消费侧信号）
        if self.delta_chain_len > DELTA_CHAIN_MAX {
            self.deltas.clear();
            self.delta_chain_len = 0;
            self.audit.record(tick, AuditEvent::FullResent, 0);
            self.full_resends += 1;
        }
        self.audit.record(tick, AuditEvent::DeltaCompressed, 0);
        Ok(id)
    }

    /// 哈希校验（压缩失真防线）：delta 应用后封印 == 声称 post 封印。
    /// 失真 → 0xED01 + 全量重发标记。
    pub fn verify_hash(&mut self, pack_id: u32, applied_seal: u64, tick: u64) -> Result<(), u16> {
        let mut idx = usize::MAX;
        let mut i = 0usize;
        for d in self.deltas.iter() {
            if d.pack_id == pack_id {
                idx = i;
            }
            i += 1;
        }
        if idx == usize::MAX {
            self.audit.record(tick, AuditEvent::ResumeBadId, XC_SNAP_UNKNOWN);
            return Err(XC_SNAP_UNKNOWN);
        }
        let claimed = self.deltas[idx].post_seal;
        if applied_seal != claimed {
            // 压缩失真：拒收 + 断链 + 全量重发
            self.deltas.clear();
            self.delta_chain_len = 0;
            self.full_resends += 1;
            self.audit.record(tick, AuditEvent::HashMismatch, XC_HASH_MISMATCH);
            self.audit.record(tick, AuditEvent::FullResent, 0);
            return Err(XC_HASH_MISMATCH);
        }
        Ok(())
    }

    pub fn delta_count(&self) -> usize {
        self.deltas.len()
    }

    // ---------- 机三 · 锚点机制 ----------

    /// 打全量锚点（Maintainer；或链超限自动触发后调用）
    pub fn take_anchor(&mut self, store_seal: u64, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        self.anchor = Some((tick, store_seal));
        self.delta_chain_len = 0;
        self.audit.record(tick, AuditEvent::AnchorTaken, 0);
        Ok(())
    }

    /// 锚点校验：锚点在案但封印对不上 → 0xED04 丢失 → 全量重发。
    pub fn verify_anchor(&mut self, cur_seal: u64, tick: u64) -> Result<(), u16> {
        match self.anchor {
            None => Ok(()), // 无锚点：尚未建立，不判丢
            Some((_t, seal)) => {
                if seal != cur_seal {
                    self.anchor = None; // 丢失：清锚等待重打
                    self.full_resends += 1;
                    self.audit.record(tick, AuditEvent::AnchorLost, XC_ANCHOR_LOST);
                    self.audit.record(tick, AuditEvent::FullResent, 0);
                    Err(XC_ANCHOR_LOST)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn anchor_info(&self) -> Option<(u64, u64)> {
        self.anchor
    }

    // ---------- 恢复面（新连接补发 + 断线恢复） ----------

    /// 断线恢复请求：快照 id 在册且 delta 链可衔接 → ServedFromSnapshot；
    /// id 无效 → 0xED07 + FullResend（全量重发）。
    pub fn request_resume(&mut self, snapshot_id: u32, tick: u64) -> Result<ResumeOutcome, u16> {
        if !self.mounted {
            return Err(XC_ROLE);
        }
        match self.snap_by_id(snapshot_id) {
            Some(s) => {
                // 只有最新快照可作恢复基（旧快照的 delta 链已被淘汰）
                let latest = self.latest_snapshot().map(|x| x.snap_id).unwrap_or(0);
                if snapshot_id == latest {
                    self.resume_served += 1;
                    self.audit.record(tick, AuditEvent::ResumeServed, 0);
                    Ok(ResumeOutcome::ServedFromSnapshot)
                } else {
                    // 旧快照：全量重发（O(全量)）
                    self.full_resends += 1;
                    self.audit.record(tick, AuditEvent::FullResent, 0);
                    Ok(ResumeOutcome::FullResend)
                }
            }
            None => {
                self.full_resends += 1;
                self.audit.record(tick, AuditEvent::ResumeBadId, XC_RESUME_BAD_ID);
                self.audit.record(tick, AuditEvent::FullResent, 0);
                Err(XC_RESUME_BAD_ID)
            }
        }
    }

    /// 新连接补发：拿最新快照 id（无快照 → Err 语义：等首个快照）
    pub fn new_connection(&mut self, tick: u64) -> Result<u32, u16> {
        let sid = match self.latest_snapshot() {
            Some(s) => s.snap_id,
            None => return Err(XC_SNAP_UNKNOWN),
        };
        self.resume_served += 1;
        self.audit.record(tick, AuditEvent::ResumeServed, 0);
        Ok(sid)
    }

    // ---------- 压缩率调优（Maintainer + 双签） ----------

    /// 调压缩率底线（双签）：第一签 Pending、第二签（异人）生效。
    pub fn set_floor(&mut self, permille: u32, signer: &str, tick: u64) -> Result<u32, u16> {
        self.require_maintainer(tick)?;
        if permille == 0 || permille > 1000 {
            return Err(XC_FLOOR_SIGN);
        }
        if signer.len() < SIGNER_MIN_LEN {
            return Err(XC_FLOOR_SIGN);
        }
        for s in self.floor_signers.iter() {
            if s == signer {
                return Err(XC_FLOOR_SIGN); // 同签人重复
            }
        }
        self.floor_signers.push(alloc::format!("{}", signer));
        if self.floor_signers.len() >= 2 {
            self.floor_signers.clear();
            self.floor_permille = permille;
            self.audit.record(tick, AuditEvent::DualSigned, 0);
            self.audit.record(tick, AuditEvent::FloorChanged, 0);
            return Ok(permille);
        }
        self.audit.record(tick, AuditEvent::DualSigned, 0);
        Ok(self.floor_permille)
    }

    // ---------- 统计 ----------

    pub fn full_resends(&self) -> u32 {
        self.full_resends
    }

    pub fn resume_served(&self) -> u64 {
        self.resume_served
    }

    pub fn period_lates(&self) -> u32 {
        self.period_lates
    }

    pub fn delta_chain_len(&self) -> usize {
        self.delta_chain_len
    }
}

// ============================================================
// 第七节 · 自检聚合
// ============================================================

/// 自检一：压缩率定点数学（dirty/total×1000）
pub fn selfcheck_rate_math() -> bool {
    // 30/100 = 300‰
    let r1 = ((30u64) * 1000 / 100u64) as u32;
    // 1/1000 = 1‰
    let r2 = ((1u64) * 1000 / 1000u64) as u32;
    // 0/100 = 0‰
    let r3 = ((0u64) * 1000 / 100u64) as u32;
    r1 == 300 && r2 == 1 && r3 == 0
}

/// 自检二：FNV 稳定性
pub fn selfcheck_fnv() -> bool {
    seal_str("snap") == seal_str("snap") && seal_str("snap") != seal_str("full")
}

/// 自检三：定点转串
pub fn selfcheck_num() -> bool {
    let mut s = String::new();
    push_u64(&mut s, 64);
    s == "64"
}
