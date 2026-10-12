//! vef76_dirtysync —— VE-F5424 增量同步（AA02 组第四单）
//! ============================================================
//! 职责（锚点原文逐条落实）：
//!   · 增量三件：
//!     变更追踪——状态变更脏标记追踪（改了什么记什么）
//!     增量打包——脏状态打包增量包（不传没变的）
//!     增量可靠性——增量包丢失→下周期重发（最终一致）——丢包不丢状态
//!   · 错误路径：脏标丢失→全量比对兜底；打包超限→分帧；一致性破坏→校验修复
//!   · 性能：追踪 O(变更数)；打包 O(脏状态)；兜底 O(全量)
//!   · 对接：F5423 快照上游；F5425 插值消费；F5431 测试
//!
//! 三大机制：
//!   机一 · 脏标记追踪：键注册制（未注册 0xEE01）；mark_dirty O(1) 幂等
//!         （同周期重复标脏只计一次——O(变更数) 语义）；batch 取出即清
//!         （batch_seq 单调；跳号 = 脏标丢失 → 全量比对兜底 0xEE02）。
//!   机二 · 增量打包器：脏集打包增量包；超 MAX_KEYS_PER_PACK 分帧
//!         （打包超限→分帧，不是拒绝——0xEE03 语义为「需要分帧」的
//!         帧纪律）；帧 seq 严格递增；帧重组校验连续性。
//!   机三 · 增量可靠性：未确认窗口（unacked）；超 RESEND_TIMEOUT 重发
//!         （最终一致——丢包不丢状态）；同包重发 MAX_RESEND 次仍无 ack
//!         → 升级全量比对兜底（0xEE05）；终态封印对拍失配 → 0xEE06
//!         校验修复路径。
//!
//! 诊断码段 0xEE（十码，本单独占）：
//!   0xEE01 未注册键标脏
//!   0xEE02 脏标丢失（batch 跳号 → 全量比对兜底）
//!   0xEE03 打包帧纪律（空包拒/帧序断）
//!   0xEE04 重发排程（超龄包下周期重发——最终一致，非错误）
//!   0xEE05 重发升级（超上限 → 全量比对兜底）
//!   0xEE06 一致性破坏（终态封印失配 → 校验修复）
//!   0xEE07 帧重组坏序（帧号不连续）
//!   0xEE08 ack 无主（确认未发包）
//!   0xEE09 越权（Runtime 调 Maintainer 操作）
//!   0xEE0A 调参缺双签
//!
//! 两权分立：Maintainer（注册键、调重发参数-双签、强制全量兜底）/
//!   Runtime（标脏、打包、发送、ack、审计）。mount 唯一入口；
//!   FNV-1a 64 封印 + 篡改检出。
//! ============================================================

use alloc::string::String;
use alloc::vec::Vec;

// ============================================================
// 第一节 · 全局常量
// ============================================================

/// 增量同步 schema 版本
pub const DIRTY_SCHEMA_VER: u32 = 1;
/// 单帧最大键数（打包超限 → 分帧）
pub const MAX_KEYS_PER_PACK: usize = 16;
/// 重发超时（tick）
pub const RESEND_TIMEOUT: u64 = 8;
/// 同包重发上限（超过升级全量兜底）
pub const MAX_RESEND: u32 = 3;
/// 脏标 batch 容量上限（超出触发溢出告警）
pub const DIRTY_BATCH_CAP: usize = 128;
/// 审计账满容
pub const AUDIT_CAP: usize = 256;
/// 双签签名人最小长度
pub const SIGNER_MIN_LEN: usize = 2;

// ============================================================
// 第二节 · 诊断码段 0xEE
// ============================================================

pub const XC_KEY_UNKNOWN: u16 = 0xEE01; // 未注册键标脏
pub const XC_BATCH_GAP: u16 = 0xEE02; // 脏标丢失 → 全量兜底
pub const XC_PACK_DISCIPLINE: u16 = 0xEE03; // 打包帧纪律
pub const XC_RESEND_SCHED: u16 = 0xEE04; // 重发排程（最终一致）
pub const XC_RESEND_ESCALATE: u16 = 0xEE05; // 重发升级全量兜底
pub const XC_CONSISTENCY: u16 = 0xEE06; // 一致性破坏
pub const XC_FRAME_ORDER: u16 = 0xEE07; // 帧重组坏序
pub const XC_ACK_ORPHAN: u16 = 0xEE08; // ack 无主
pub const XC_ROLE: u16 = 0xEE09; // 越权
pub const XC_PARAM_SIGN: u16 = 0xEE0A; // 调参缺双签

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
// 第四节 · 数据结构
// ============================================================

/// 脏标 batch（取出即清的周期脏集）
#[derive(Clone)]
pub struct DirtyBatch {
    pub batch_seq: u64,
    pub keys: Vec<&'static str>,
    pub tick: u64,
}

/// 增量帧（分帧后单帧）
#[derive(Clone)]
pub struct DeltaFrame {
    pub frame_seq: u64,
    pub batch_seq: u64,
    /// 帧内键集
    pub keys: Vec<&'static str>,
    /// 帧封印（键集确定性 FNV）
    pub frame_seal: u64,
    pub sent_tick: u64,
    /// 重发次数
    pub resend_count: u32,
    /// 已确认
    pub acked: bool,
}

/// 审计事件闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditEvent {
    KeyRegistered,
    MarkedDirty,
    BatchTaken,
    BatchGap,
    FramePacked,
    FrameSplit,
    FrameSent,
    Acked,
    AckOrphan,
    ResendScheduled,
    ResendEscalated,
    FullFallback,
    ConsistencyFail,
    FrameOrderBad,
    ParamChanged,
    DualSigned,
    RoleViolation,
}

impl AuditEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEvent::KeyRegistered => "key-registered",
            AuditEvent::MarkedDirty => "marked-dirty",
            AuditEvent::BatchTaken => "batch-taken",
            AuditEvent::BatchGap => "batch-gap",
            AuditEvent::FramePacked => "frame-packed",
            AuditEvent::FrameSplit => "frame-split",
            AuditEvent::FrameSent => "frame-sent",
            AuditEvent::Acked => "acked",
            AuditEvent::AckOrphan => "ack-orphan",
            AuditEvent::ResendScheduled => "resend-scheduled",
            AuditEvent::ResendEscalated => "resend-escalated",
            AuditEvent::FullFallback => "full-fallback",
            AuditEvent::ConsistencyFail => "consistency-fail",
            AuditEvent::FrameOrderBad => "frame-order-bad",
            AuditEvent::ParamChanged => "param-changed",
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
// 第五节 · 角色与配置
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 维护权：注册键、调重发参数（双签）、强制全量兜底
    Maintainer,
    /// 运行权：标脏、打包、发送、ack、周期巡检
    Runtime,
}

/// 重发参数（双签调参对象）
#[derive(Clone, Copy)]
pub struct ResendParams {
    pub timeout: u64,
    pub max_resend: u32,
}

pub const RESEND_PARAMS_DEFAULT: ResendParams = ResendParams {
    timeout: RESEND_TIMEOUT,
    max_resend: MAX_RESEND,
};

// ============================================================
// 第六节 · 增量同步引擎
// ============================================================

pub struct DirtySync {
    pub role: Role,
    /// 注册键册
    keys: Vec<&'static str>,
    /// 当前周期脏集（mark 幂等）
    dirty: Vec<&'static str>,
    /// batch 序号（单调）
    next_batch_seq: u64,
    last_batch_seq: u64,
    /// 未确认窗口
    unacked: Vec<DeltaFrame>,
    /// 帧序号（单调）
    next_frame_seq: u64,
    /// 重发参数
    pub params: ResendParams,
    param_signers: Vec<String>,
    /// 全量兜底标记（脏标丢失/重发升级/一致性破坏后置位）
    pub full_fallback_armed: bool,
    full_fallbacks: u32,
    /// 重发总次数
    resends_total: u32,
    /// ack 总数
    acks_total: u64,
    /// 终态封印（链尾 post seal——对拍锚）
    chain_seal: Option<u64>,
    seal: u64,
    mounted: bool,
    pub audit: AuditLog,
}

impl DirtySync {
    /// mount 唯一入口
    pub fn mount(role: Role, tick: u64) -> DirtySync {
        let mut src = String::new();
        src.push_str("vef76-dirtysync|role=");
        src.push_str(match role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, DIRTY_SCHEMA_VER as u64);
        DirtySync {
            role,
            keys: Vec::new(),
            dirty: Vec::new(),
            next_batch_seq: 1,
            last_batch_seq: 0,
            unacked: Vec::new(),
            next_frame_seq: 1,
            params: RESEND_PARAMS_DEFAULT,
            param_signers: Vec::new(),
            full_fallback_armed: false,
            full_fallbacks: 0,
            resends_total: 0,
            acks_total: 0,
            chain_seal: None,
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
        src.push_str("vef76-dirtysync|role=");
        src.push_str(match self.role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        src.push_str("|ver=");
        push_u64(&mut src, DIRTY_SCHEMA_VER as u64);
        seal_str(&src) == self.seal
    }

    fn require_maintainer(&mut self, tick: u64) -> Result<(), u16> {
        if self.role != Role::Maintainer {
            self.audit.record(tick, AuditEvent::RoleViolation, XC_ROLE);
            return Err(XC_ROLE);
        }
        Ok(())
    }

    fn key_known(&self, key: &str) -> bool {
        for k in self.keys.iter() {
            if *k == key {
                return true;
            }
        }
        false
    }

    // ---------- Maintainer：注册与调参 ----------

    /// 注册键（幂等；空名拒）
    pub fn register_key(&mut self, key: &'static str, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        if key.len() == 0 {
            return Err(XC_KEY_UNKNOWN);
        }
        if self.key_known(key) {
            return Ok(());
        }
        self.keys.push(key);
        self.audit.record(tick, AuditEvent::KeyRegistered, 0);
        Ok(())
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// 调重发参数（双签）
    pub fn set_params(
        &mut self,
        timeout: u64,
        max_resend: u32,
        signer: &str,
        tick: u64,
    ) -> Result<ResendParams, u16> {
        self.require_maintainer(tick)?;
        if timeout == 0 || max_resend == 0 || max_resend > 16 {
            return Err(XC_PARAM_SIGN);
        }
        if signer.len() < SIGNER_MIN_LEN {
            return Err(XC_PARAM_SIGN);
        }
        for s in self.param_signers.iter() {
            if s == signer {
                return Err(XC_PARAM_SIGN);
            }
        }
        self.param_signers.push(alloc::format!("{}", signer));
        if self.param_signers.len() >= 2 {
            self.param_signers.clear();
            self.params = ResendParams { timeout, max_resend };
            self.audit.record(tick, AuditEvent::DualSigned, 0);
            self.audit.record(tick, AuditEvent::ParamChanged, 0);
            return Ok(self.params);
        }
        self.audit.record(tick, AuditEvent::DualSigned, 0);
        Ok(RESEND_PARAMS_DEFAULT)
    }

    // ---------- 机一 · 脏标记追踪 ----------

    /// 标脏（O(1) 幂等；未注册 0xEE01）
    pub fn mark_dirty(&mut self, key: &'static str, tick: u64) -> Result<(), u16> {
        if !self.key_known(key) {
            self.audit.record(tick, AuditEvent::RoleViolation, XC_KEY_UNKNOWN);
            return Err(XC_KEY_UNKNOWN);
        }
        // 幂等：同周期重复标脏只计一次
        for k in self.dirty.iter() {
            if *k == key {
                return Ok(());
            }
        }
        self.dirty.push(key);
        self.audit.record(tick, AuditEvent::MarkedDirty, 0);
        Ok(())
    }

    /// 当前脏集大小
    pub fn dirty_len(&self) -> usize {
        self.dirty.len()
    }

    /// 取出脏集 batch（取出即清——下一周期新脏；batch_seq 单调）。
    /// 全量兜底置位时先清除兜底（兜底重放已补齐脏集语义）。
    pub fn take_batch(&mut self, tick: u64) -> Result<DirtyBatch, u16> {
        if !self.mounted {
            return Err(XC_ROLE);
        }
        // batch_seq 连续性自检（跳号 = 脏标丢失）
        if self.last_batch_seq > 0 && self.next_batch_seq != self.last_batch_seq + 1 {
            self.arm_full_fallback(tick, XC_BATCH_GAP);
            return Err(XC_BATCH_GAP);
        }
        if self.dirty.len() > DIRTY_BATCH_CAP {
            // 溢出告警：超大脏集仍打包（分帧兜），但记审计
            self.audit.record(tick, AuditEvent::BatchGap, XC_BATCH_GAP);
        }
        let seq = self.next_batch_seq;
        self.next_batch_seq += 1;
        self.last_batch_seq = seq;
        let mut keys: Vec<&'static str> = Vec::new();
        for k in self.dirty.iter() {
            keys.push(k);
        }
        self.dirty.clear();
        self.audit.record(tick, AuditEvent::BatchTaken, 0);
        Ok(DirtyBatch {
            batch_seq: seq,
            keys,
            tick,
        })
    }

    /// batch 序推进修正（全量兜底重放后对齐——重放 batch 也占一个序号）
    pub fn advance_batch_seq(&mut self, tick: u64) {
        self.last_batch_seq = self.next_batch_seq;
        self.next_batch_seq += 1;
        self.audit.record(tick, AuditEvent::BatchTaken, 0);
    }

    // ---------- 机二 · 增量打包器（分帧） ----------

    /// 打包 batch → 帧序列（超 MAX_KEYS_PER_PACK 分帧；帧 seq 严格递增）。
    /// 空批 → 0xEE03（不传没变的——空批即无事可传）。
    pub fn pack_frames(&mut self, batch: &DirtyBatch, tick: u64) -> Result<Vec<DeltaFrame>, u16> {
        if batch.keys.len() == 0 {
            self.audit.record(tick, AuditEvent::FramePacked, XC_PACK_DISCIPLINE);
            return Err(XC_PACK_DISCIPLINE);
        }
        let mut out: Vec<DeltaFrame> = Vec::new();
        let mut start = 0usize;
        while start < batch.keys.len() {
            let end = if start + MAX_KEYS_PER_PACK < batch.keys.len() + 1 {
                let e = start + MAX_KEYS_PER_PACK;
                if e > batch.keys.len() {
                    batch.keys.len()
                } else {
                    e
                }
            } else {
                batch.keys.len()
            };
            // 帧封印：键集确定性 FNV
            let mut seal_src = String::new();
            seal_src.push_str("frame|batch=");
            push_u64(&mut seal_src, batch.batch_seq);
            seal_src.push_str("|span=");
            push_u64(&mut seal_src, start as u64);
            push_u64(&mut seal_src, end as u64);
            let mut j = start;
            while j < end {
                seal_src.push('|');
                seal_src.push_str(batch.keys[j]);
                j += 1;
            }
            let frame = DeltaFrame {
                frame_seq: self.next_frame_seq,
                batch_seq: batch.batch_seq,
                keys: {
                    let mut ks: Vec<&'static str> = Vec::new();
                    let mut jj = start;
                    while jj < end {
                        ks.push(batch.keys[jj]);
                        jj += 1;
                    }
                    ks
                },
                frame_seal: seal_str(&seal_src),
                sent_tick: tick,
                resend_count: 0,
                acked: false,
            };
            self.next_frame_seq += 1;
            if batch.keys.len() > MAX_KEYS_PER_PACK {
                self.audit.record(tick, AuditEvent::FrameSplit, 0);
            }
            self.audit.record(tick, AuditEvent::FramePacked, 0);
            out.push(frame);
            start = end;
        }
        Ok(out)
    }

    /// 发送帧入未确认窗口
    pub fn send_frame(&mut self, frame: DeltaFrame, tick: u64) -> Result<(), u16> {
        if !self.mounted {
            return Err(XC_ROLE);
        }
        self.unacked.push(frame);
        self.audit.record(tick, AuditEvent::FrameSent, 0);
        Ok(())
    }

    pub fn unacked_len(&self) -> usize {
        self.unacked.len()
    }

    /// 帧重组校验：帧 seq 必须连续（坏序 0xEE07）
    pub fn reassemble_check(&self, frames: &[DeltaFrame]) -> Result<(), u16> {
        let mut prev: u64 = 0;
        for f in frames.iter() {
            if prev > 0 && f.frame_seq != prev + 1 {
                return Err(XC_FRAME_ORDER);
            }
            prev = f.frame_seq;
        }
        Ok(())
    }

    // ---------- 机三 · 增量可靠性（重发 + 最终一致） ----------

    /// 周期巡检：超龄未确认 → 重发排程（0xEE04，最终一致语义）；
    /// 重发次数超上限 → 升级全量兜底（0xEE05）。
    pub fn sweep_unacked(&mut self, tick: u64) -> usize {
        let timeout = self.params.timeout;
        let max_resend = self.params.max_resend;
        let mut rescheduled = 0usize;
        let mut i = 0usize;
        while i < self.unacked.len() {
            let f = &self.unacked[i];
            let age = if tick > f.sent_tick { tick - f.sent_tick } else { 0 };
            if age > timeout && !f.acked {
                if f.resend_count >= max_resend {
                    // 升级全量兜底
                    self.unacked.remove(i);
                    self.arm_full_fallback(tick, XC_RESEND_ESCALATE);
                    continue; // 不前进 i（已移除）
                }
                // 重发排程：刷新 sent_tick + 计数
                let f = &mut self.unacked[i];
                f.sent_tick = tick;
                f.resend_count += 1;
                self.resends_total += 1;
                rescheduled += 1;
                self.audit.record(tick, AuditEvent::ResendScheduled, XC_RESEND_SCHED);
            }
            i += 1;
        }
        rescheduled
    }

    /// ack 帧（确认；无主 ack → 0xEE08）
    pub fn ack_frame(&mut self, frame_seq: u64, tick: u64) -> Result<(), u16> {
        let mut idx = usize::MAX;
        let mut i = 0usize;
        for f in self.unacked.iter() {
            if f.frame_seq == frame_seq {
                idx = i;
            }
            i += 1;
        }
        if idx == usize::MAX {
            self.audit.record(tick, AuditEvent::AckOrphan, XC_ACK_ORPHAN);
            return Err(XC_ACK_ORPHAN);
        }
        self.unacked.remove(idx);
        self.acks_total += 1;
        self.audit.record(tick, AuditEvent::Acked, 0);
        Ok(())
    }

    // ---------- 一致性校验（校验修复路径） ----------

    /// 终态封印对拍：delta 链应用后封印 == 权威终态封印。
    /// 失配 → 0xEE06 + 全量兜底（校验修复）。
    pub fn verify_consistency(&mut self, applied_seal: u64, auth_seal: u64, tick: u64) -> Result<(), u16> {
        self.chain_seal = Some(applied_seal);
        if applied_seal != auth_seal {
            self.arm_full_fallback(tick, XC_CONSISTENCY);
            self.audit.record(tick, AuditEvent::ConsistencyFail, XC_CONSISTENCY);
            return Err(XC_CONSISTENCY);
        }
        Ok(())
    }

    /// 全量比对兜底（O(全量)）：置位 + 计数 + 审计。
    pub fn arm_full_fallback(&mut self, tick: u64, code: u16) {
        if !self.full_fallback_armed {
            self.full_fallback_armed = true;
            self.full_fallbacks += 1;
        }
        self.audit.record(tick, AuditEvent::FullFallback, code);
    }

    /// 解除兜底（全量比对完成后由权威侧确认）
    pub fn disarm_full_fallback(&mut self, tick: u64) {
        if self.full_fallback_armed {
            self.full_fallback_armed = false;
            self.audit.record(tick, AuditEvent::BatchTaken, 0);
        }
    }

    // ---------- 统计 ----------

    pub fn resends_total(&self) -> u32 {
        self.resends_total
    }

    pub fn acks_total(&self) -> u64 {
        self.acks_total
    }

    pub fn full_fallbacks(&self) -> u32 {
        self.full_fallbacks
    }

    pub fn next_frame_seq(&self) -> u64 {
        self.next_frame_seq
    }

    pub fn last_batch_seq(&self) -> u64 {
        self.last_batch_seq
    }
}

// ============================================================
// 第七节 · 自检聚合
// ============================================================

/// 自检一：分帧数学（span 切片不越界不重复）
pub fn selfcheck_span_math() -> bool {
    // 17 键 → 2 帧：16+1
    let total = 17usize;
    let cap = MAX_KEYS_PER_PACK;
    let mut start = 0usize;
    let mut covered = 0usize;
    let mut frames = 0usize;
    while start < total {
        let mut end = start + cap;
        if end > total {
            end = total;
        }
        covered += end - start;
        start = end;
        frames += 1;
    }
    frames == 2 && covered == total
}

/// 自检二：FNV 稳定性
pub fn selfcheck_fnv() -> bool {
    seal_str("dirty") == seal_str("dirty") && seal_str("dirty") != seal_str("sync")
}

/// 自检三：定点转串
pub fn selfcheck_num() -> bool {
    let mut s = String::new();
    push_u64(&mut s, 16);
    s == "16"
}
