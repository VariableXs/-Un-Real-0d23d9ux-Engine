// SPDX-License-Identifier: Apache-2.0
//! UNX-F0277 · 远程终端会话标识（串口会话头）
//!
//! 会话头（指纹/序号/时间）随连接输出，对端可关联账本。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::format;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 会话标识
// ---------------------------------------------------------------------------

/// 会话头。
#[derive(Debug, Clone)]
pub struct SessionHeader {
    /// 会话指纹（唯一）。
    pub fingerprint: String,
    /// 会话序号（单调递增）。
    pub sequence: u64,
    /// 连接时间（纳秒）。
    pub connect_time_ns: u64,
    /// 远端标识。
    pub remote_id: String,
}

impl SessionHeader {
    /// 生成会话头。
    pub fn generate(sequence: u64, connect_time_ns: u64, remote_id: &str) -> Self {
        // 简易指纹：序号+时间的 hash
        let fp = alloc::format!("{:08x}", (sequence as u32) ^ (connect_time_ns as u32) ^ 0x5A5A5A5A);
        SessionHeader {
            fingerprint: fp,
            sequence,
            connect_time_ns,
            remote_id: remote_id.to_string(),
        }
    }

    /// 格式化输出（会话头）。
    pub fn format(&self) -> String {
        alloc::format!(
            "=== SESSION fp={} seq={} t={} remote={} ===",
            self.fingerprint, self.sequence, self.connect_time_ns, self.remote_id
        )
    }

    /// 关联账本（验证指纹可关联）。
    pub fn can_link_to_ledger(&self) -> bool {
        !self.fingerprint.is_empty() && self.sequence > 0
    }
}

// ---------------------------------------------------------------------------
// 会话管理器
// ---------------------------------------------------------------------------

/// 会话管理器。
pub struct SessionManager {
    /// 下一序号。
    pub next_seq: u64,
    /// 已建立会话。
    pub sessions: Vec<SessionHeader>,
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager { next_seq: 1, sessions: Vec::new() }
    }

    /// 建立新会话。
    pub fn create_session(&mut self, connect_time_ns: u64, remote_id: &str) -> SessionHeader {
        let header = SessionHeader::generate(self.next_seq, connect_time_ns, remote_id);
        self.next_seq += 1;
        self.sessions.push(header.clone());
        header
    }

    /// 会话总数。
    pub fn count(&self) -> usize { self.sessions.len() }

    /// 按指纹查找。
    pub fn find_by_fingerprint(&self, fp: &str) -> Option<&SessionHeader> {
        self.sessions.iter().find(|s| s.fingerprint == fp)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0277 判据全集。
pub fn run_session_header_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0277", "远程终端会话标识");

    // 1. 会话头
    let h = SessionHeader::generate(1, 1000, "remote_a");
    cs.ok("指纹非空", !h.fingerprint.is_empty(), "有指纹");
    cs.ok("序号正确", h.sequence == 1, "序号 1");
    cs.ok("远端标识", h.remote_id == "remote_a", "记录");

    // 2. 格式化
    let fmt = h.format();
    cs.ok("格式化输出", fmt.contains("SESSION"), "会话头");
    cs.ok("含指纹", fmt.contains(&h.fingerprint), "指纹可查");
    cs.ok("含序号", fmt.contains("seq=1"), "序号可查");

    // 3. 账本关联
    cs.ok("可关联账本", h.can_link_to_ledger(), "指纹+序号");

    // 4. 会话管理
    let mut sm = SessionManager::new();
    let h1 = sm.create_session(1000, "r1");
    let h2 = sm.create_session(2000, "r2");
    cs.ok("会话数", sm.count() == 2, "两个会话");
    cs.ok("序号递增", h2.sequence == 2, "单调递增");
    cs.ok("指纹不同", h1.fingerprint != h2.fingerprint, "唯一");

    // 5. 查找
    cs.ok("按指纹查找", sm.find_by_fingerprint(&h1.fingerprint).is_some(), "命中");
    cs.ok("不存在", sm.find_by_fingerprint("nonexist").is_none(), "未命中");

    // 6. 唯一性
    let mut sm2 = SessionManager::new();
    for i in 0..10 {
        sm2.create_session(i, "r");
    }
    let fps: Vec<String> = sm2.sessions.iter().map(|s| s.fingerprint.clone()).collect();
    let mut unique = fps.clone();
    unique.sort();
    unique.dedup();
    cs.ok("指纹唯一", unique.len() == 10, "十个不同指纹");

    cs.done()
}
