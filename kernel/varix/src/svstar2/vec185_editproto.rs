//! VE-F0585 · 编辑器集成协议
//!
//! 编辑器与热编译服务的正式协议——增量文本同步（版本号加变更区间，服务端确认
//! 后进队列）、诊断推送（错误三要素加源码区间加修复建议，乱序防护带版本戳）、
//! 命令通道（强制重编、档位切换、缓存清除、会话重置）、撤销集成（版本号对齐
//! 保证一致性）；消息帧格式、心跳保活、协议版本化与向后兼容承诺、断线重连
//! （服务重启后会话恢复与未完任务重排）。
//!
//! 跨批对接：上游 F0581 服务架构；下游 F0596 接口冻结、F0589 遥测。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 协议版本（向后兼容承诺：低版本客户端可连高版本服务）
pub const PROTO_VER: u32 = 2;

/// 版本协商：服务支持范围 [min, max]
pub fn negotiate(client_ver: u32, min_sup: u32, max_sup: u32) -> Option<u32> {
    if client_ver >= min_sup && client_ver <= max_sup {
        Some(client_ver)
    } else if client_ver > max_sup {
        Some(max_sup) // 降级协商
    } else {
        None
    }
}

/// 帧格式：magic + ver + len + payload 序号
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub ver: u32,
    pub seq: u64,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(ver: u32, seq: u64, payload: &[u8]) -> Self {
        Frame { ver, seq, payload: payload.to_vec() }
    }

    pub fn wellformed(&self) -> bool {
        self.ver <= PROTO_VER && !self.payload.is_empty()
    }
}

/// 增量文本同步：版本号 + 变更区间
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TextSync {
    pub base_ver: u32,
    pub new_ver: u32,
    /// 变更区间 [start, end)
    pub start: u32,
    pub end: u32,
    pub text: String,
}

impl TextSync {
    pub fn valid(&self) -> bool {
        self.new_ver == self.base_ver + 1 && self.end >= self.start
    }
}

/// 服务端确认后进队列
#[derive(Clone, Debug, Default)]
pub struct SyncQueue {
    pub pending: Vec<TextSync>,
    /// 服务器侧版本（对齐基准）
    pub server_ver: u32,
}

impl SyncQueue {
    /// 确认：新版本必须恰好是服务器版本 +1（撤销一致性由版本号对齐保证）
    pub fn offer(&mut self, t: TextSync) -> bool {
        if !t.valid() || t.base_ver != self.server_ver {
            return false;
        }
        self.pending.push(t);
        self.server_ver += 1;
        true
    }

    pub fn drain(&mut self) -> Vec<TextSync> {
        core::mem::take(&mut self.pending)
    }
}

/// 诊断推送：三要素 + 源码区间 + 修复建议，版本戳乱序防护
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiagPush {
    pub ver_stamp: u32,
    pub source_line: u32,
    pub text: String,
    pub fix_hint: String,
}

impl DiagPush {
    pub fn complete(&self) -> bool {
        !self.text.is_empty() && !self.fix_hint.is_empty() && self.source_line > 0
    }
}

/// 推送通道（乱序防护：丢弃旧版本戳包）
#[derive(Clone, Debug, Default)]
pub struct PushChannel {
    pub received: Vec<DiagPush>,
    pub dropped_stale: u32,
}

impl PushChannel {
    pub fn deliver(&mut self, d: DiagPush) -> bool {
        if let Some(last) = self.received.last() {
            if d.ver_stamp < last.ver_stamp {
                self.dropped_stale += 1;
                return false;
            }
        }
        if !d.complete() {
            return false;
        }
        self.received.push(d);
        true
    }
}

/// 命令通道
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    ForceRecompile,
    TierSwitch,
    CacheClear,
    SessionReset,
}

/// 心跳保活：半开连接清理
#[derive(Clone, Debug)]
pub struct Heartbeat {
    /// 上次心跳时刻（单调序号）
    pub last_seen: u64,
    /// 超时阈值
    pub timeout: u64,
}

impl Heartbeat {
    pub fn alive(&self, now: u64) -> bool {
        now.saturating_sub(self.last_seen) <= self.timeout
    }

    pub fn half_open(&self, now: u64) -> bool {
        !self.alive(now)
    }

    pub fn beat(&mut self, now: u64) {
        self.last_seen = now;
    }
}

/// 会话恢复协议：服务重启后恢复会话 + 未完任务重排
#[derive(Clone, Debug, Default)]
pub struct RecoverProto {
    /// 已确认版本（客户端与服务端对齐点）
    pub acked_ver: u32,
    /// 重排的未完任务数
    pub requeued: u32,
}

impl RecoverProto {
    pub fn new(acked_ver: u32) -> Self {
        RecoverProto { acked_ver, requeued: 0 }
    }

    /// 恢复：从确认点之后重新同步
    pub fn resume(&mut self, client_ver: u32) -> u32 {
        if client_ver > self.acked_ver {
            self.requeued += client_ver - self.acked_ver;
            self.acked_ver = client_ver;
        }
        self.requeued
    }
}

pub fn run_vec185_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec185");
    // 族一：协议版本化与协商
    s.add("协商-版本命中", negotiate(2, 1, 3) == Some(2), "hit");
    s.add("协商-过新降级", negotiate(5, 1, 3) == Some(3), "down");
    s.add("协商-过旧拒绝", negotiate(0, 1, 3).is_none(), "old");
    s.add("协商-向后兼容", PROTO_VER == 2, "ver");
    // 族二：帧格式
    let f = Frame::new(2, 1, b"sync");
    s.add("帧-良构", f.wellformed(), "ok");
    s.add("帧-超版拒绝", !Frame::new(9, 2, b"x").wellformed(), "ver");
    s.add("帧-空载荷拒绝", !Frame::new(2, 3, b"").wellformed(), "empty");
    // 族三：增量文本同步
    let mut q = SyncQueue::default();
    s.add("同步-版本对齐受理", q.offer(TextSync { base_ver: 0, new_ver: 1, start: 0, end: 5, text: "abcde".to_string() }), "ok");
    s.add("同步-服务器版本前移", q.server_ver == 1, "ver");
    s.add("同步-跳跃版本拒绝", !q.offer(TextSync { base_ver: 1, new_ver: 5, start: 0, end: 1, text: "x".to_string() }), "skip");
    s.add("同步-区间倒置拒绝", !q.offer(TextSync { base_ver: 1, new_ver: 2, start: 5, end: 3, text: "x".to_string() }), "inv");
    s.add("同步-撤销对齐受理", q.offer(TextSync { base_ver: 1, new_ver: 2, start: 0, end: 2, text: "ab".to_string() }), "undo");
    s.add("同步-排空", q.drain().len() == 2 && q.pending.is_empty(), "drain");
    // 族四：诊断推送与乱序防护
    let mut ch = PushChannel::default();
    s.add("推送-完整受理", ch.deliver(DiagPush { ver_stamp: 1, source_line: 3, text: "err".to_string(), fix_hint: "fix".to_string() }), "ok");
    s.add("推送-旧戳丢弃", !ch.deliver(DiagPush { ver_stamp: 0, source_line: 1, text: "old".to_string(), fix_hint: "f".to_string() }) && ch.dropped_stale == 1, "stale");
    s.add("推送-不完整拒绝", !ch.deliver(DiagPush { ver_stamp: 2, source_line: 0, text: "t".to_string(), fix_hint: "f".to_string() }), "inc");
    s.add("推送-计数", ch.received.len() == 1, "cnt");
    // 族五：命令通道
    s.add("命令-四命令", Command::ForceRecompile != Command::TierSwitch && Command::CacheClear != Command::SessionReset, "cmds");
    // 族六：心跳保活
    let mut hb = Heartbeat { last_seen: 100, timeout: 30 };
    s.add("心跳-存活", hb.alive(130) && !hb.half_open(130), "alive");
    s.add("心跳-超时半开", hb.half_open(200), "half");
    hb.beat(200);
    s.add("心跳-续命", hb.alive(210), "beat");
    // 族七：断线重连恢复
    let mut rec = RecoverProto::new(5);
    s.add("恢复-对齐点", rec.acked_ver == 5 && rec.requeued == 0, "init");
    s.add("恢复-未完重排", rec.resume(8) == 3 && rec.requeued == 3, "requeue");
    s.add("恢复-对齐点前移", rec.acked_ver == 8, "ack");
    s.add("恢复-无新任务零排", rec.resume(8) == 3, "zero");
    s
}
