//! VE-F0136 · 多用户会话隔离渲染（VE-A 域 · A06 环境与系统集成组 · 目标 400 行）
//!
//! 铁律：会话隔离，零共享断言，状态保全，隐私红线。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xEAB0（重排：原 0xE8CE 未对齐）
pub const SEG_BASE: u16 = 0xEAB0;
pub const XC_ISOLATION_BREACH: u16 = SEG_BASE | 0x01;
pub const XC_CROSS_SESSION: u16 = SEG_BASE | 0x02;
pub const XC_STATE_LOST: u16 = SEG_BASE | 0x03;
pub const XC_BAD_SESSION: u16 = SEG_BASE | 0x04;
pub const XC_QUOTA_EXCEEDED: u16 = SEG_BASE | 0x05;

/// 会话资源配额。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionQuota {
    /// 最大表面数。
    pub max_surfaces: u32,
    /// 最大字节数。
    pub max_bytes: u64,
    /// 最大 GPU 时间（微秒/帧）。
    pub max_gpu_us: u32,
}

/// 会话渲染上下文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionCtx {
    /// 会话标识。
    pub id: u32,
    /// 当前表面数。
    pub surfaces: u32,
    /// 当前字节数。
    pub bytes: u64,
    /// 是否活跃。
    pub active: bool,
    /// 保全的状态指纹。
    pub state_fp: u32,
}

/// 多用户会话隔离渲染管理器。
pub struct SessionIsolator {
    /// 全部会话。
    sessions: Vec<SessionCtx>,
    /// 默认配额。
    quota: SessionQuota,
    /// 最近诊断码。
    last_code: u16,
    /// 隔离违例次数（红线）。
    breaches: u32,
}

impl SessionIsolator {
    pub fn new(quota: SessionQuota) -> Self {
        SessionIsolator { sessions: Vec::new(), quota, last_code: 0, breaches: 0 }
    }
    pub fn last_code(&self) -> u16 { self.last_code }

    /// 创建会话。
    pub fn create_session(&mut self, id: u32) -> Result<(), u16> {
        if self.sessions.iter().any(|s| s.id == id) { self.last_code = XC_BAD_SESSION; return Err(XC_BAD_SESSION); }
        self.sessions.push(SessionCtx { id, surfaces: 0, bytes: 0, active: true, state_fp: 0 });
        self.last_code = 0; Ok(())
    }

    /// 跨会话资源访问检测（隐私红线）。
    pub fn assert_isolated(&mut self, from: u32, to: u32) -> Result<(), u16> {
        if from == to { self.last_code = 0; return Ok(()); }
        // 跨会话访问 = 隔离违例 = 红线
        self.breaches += 1;
        self.last_code = XC_ISOLATION_BREACH;
        Err(XC_ISOLATION_BREACH)
    }

    /// 分配资源到会话。
    pub fn alloc_resource(&mut self, session: u32, bytes: u64) -> Result<(), u16> {
        let idx = self.sessions.iter().position(|s| s.id == session);
        let idx = match idx { Some(i) => i, None => { self.last_code = XC_BAD_SESSION; return Err(XC_BAD_SESSION); } };
        if self.sessions[idx].bytes + bytes > self.quota.max_bytes { self.last_code = XC_QUOTA_EXCEEDED; return Err(XC_QUOTA_EXCEEDED); }
        if self.sessions[idx].surfaces + 1 > self.quota.max_surfaces { self.last_code = XC_QUOTA_EXCEEDED; return Err(XC_QUOTA_EXCEEDED); }
        self.sessions[idx].bytes += bytes;
        self.sessions[idx].surfaces += 1;
        self.last_code = 0; Ok(())
    }

    /// 会话切换保全（保存状态指纹）。
    pub fn preserve_state(&mut self, session: u32, fp: u32) -> Result<(), u16> {
        let idx = self.sessions.iter().position(|s| s.id == session);
        let idx = match idx { Some(i) => i, None => { self.last_code = XC_BAD_SESSION; return Err(XC_BAD_SESSION); } };
        self.sessions[idx].state_fp = fp;
        self.sessions[idx].active = false;
        self.last_code = 0; Ok(())
    }

    /// 会话数。
    pub fn session_count(&self) -> usize { self.sessions.len() }
    /// 隔离违例数。
    pub fn breaches(&self) -> u32 { self.breaches }
}

pub fn run_vea136_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea136_session");
    let q = SessionQuota { max_surfaces: 10, max_bytes: 100000, max_gpu_us: 16000 };
    { let s = SessionIsolator::new(q); chk(&mut set, "C136-01", s.session_count() == 0, "err"); }
    { let mut s = SessionIsolator::new(q);
        let r = s.create_session(1);
        chk(&mut set, "C136-02", r.is_ok() && s.session_count() == 1, "err"); }
    { let mut s = SessionIsolator::new(q);
        let _ = s.create_session(1); let r = s.create_session(1);
        chk(&mut set, "C136-03", r.err() == Some(XC_BAD_SESSION), "err"); }
    { let mut s = SessionIsolator::new(q);
        let r = s.assert_isolated(1, 2);
        chk(&mut set, "C136-04", r.err() == Some(XC_ISOLATION_BREACH) && s.breaches() == 1, "err"); }
    { let mut s = SessionIsolator::new(q);
        let r = s.assert_isolated(1, 1);
        chk(&mut set, "C136-05", r.is_ok(), "err"); }
    { let mut s = SessionIsolator::new(q);
        let _ = s.create_session(1);
        let r = s.alloc_resource(1, 50000);
        chk(&mut set, "C136-06", r.is_ok(), "err"); }
    { let mut s = SessionIsolator::new(q);
        let _ = s.create_session(1);
        let r = s.alloc_resource(1, 200000);
        chk(&mut set, "C136-07", r.err() == Some(XC_QUOTA_EXCEEDED), "err"); }
    { let mut s = SessionIsolator::new(q);
        let _ = s.create_session(1);
        let r = s.preserve_state(1, 0xAA);
        chk(&mut set, "C136-08", r.is_ok(), "err"); }
    { let mut s = SessionIsolator::new(q);
        let r = s.alloc_resource(99, 100);
        chk(&mut set, "C136-09", r.err() == Some(XC_BAD_SESSION), "err"); }
    { let mut s = SessionIsolator::new(q);
        let _ = s.create_session(1); let _ = s.create_session(2);
        chk(&mut set, "C136-10", s.session_count() == 2, "err"); }
    set
}
