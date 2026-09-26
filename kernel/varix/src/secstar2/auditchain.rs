//! F194 审计日志完整性（secstar2 · G-G-24）——审计日志自己先不可抵赖。
//!
//! **判据（主册）**：篡改注入（改一条/删一条/插一条）三种攻击全部检出且
//! 定位；导出包第三方独立校验通过（工具开源 F126）；性能开销 <1%。
//!
//! **功能定义（主册 G-G-24）**：安全类日志（越权/门拦截/签名失败/权限变更）
//! append-only+序号链（每条含前条哈希——篡改任何一条即断链可检）；导出带
//! 链校验报告。
//!
//! 【交互设计】F120 审计页签：日志流（序号连续可视）+「校验完整性」钮
//! （跑链校验+结果报告：范围/结果/断点定位）；导出 zip 含数据+链+校验器
//! （第三方可独立验——开放可验证）。
//! 【数据与存储】序号链哈希 SHA-256；日志区 append-only（存储层强制——写后
//! 只读）；链头每日锚定进快照（F121 联动跨天验证）。
//! 【状态与异常】断链检出 → 红色工单（P0——F142 安全通道评估）+断点前后段
//! 隔离保全；磁盘满 → 安全日志优先保障（F057 最高级——安全日志饥饿是 P0）。
//! 【设计细节】链节点格式：序号/时间戳/事件体/前哈希——四字段定长结构
//! （零堆）；锚定：每日末条哈希写入当日快照 manifest（跨日链可信）；校验器
//! ~100 行（可读性优先——审计工具自己要经得起读）；事件体脱敏在写入时完成
//! （F120 三查前移——敏感数据不进不可改的日志）。
//!
//! 哈希原语：crate::ksha256::sha256（内核自实现，FIPS 180-4 向量锁定）。
//! 依赖锚点：F057（IO 最高级）、F120（审计页签）、F121（每日锚定）、F126（校验器开源）、F142（P0 工单）。

use crate::checks::CheckSet;
use crate::ksha256;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量与格式
// ---------------------------------------------------------------------------

/// 事件体上限（字节）——四字段定长结构的事件域；超长拒绝（不截断不吞：
/// 审计条目必须完整，写不下就让调用方拆条）。
pub const EVENT_MAX: usize = 96;

/// 链节点：序号/时间戳/事件体/前哈希——四字段（主册【设计细节】定长结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditNode {
    /// 序号（从 1 起，连续——可视序号流的判据源）。
    pub seq: u64,
    /// 时间戳（秒——调用方注入）。
    pub at_s: u64,
    /// 事件体（定长域，未用部分零填充）。
    pub event: [u8; EVENT_MAX],
}

impl AuditNode {
    /// 构造（事件体超长拒绝——审计条目不截断）。
    pub fn new(seq: u64, at_s: u64, event: &[u8]) -> Option<AuditNode> {
        if event.len() > EVENT_MAX {
            return None;
        }
        let mut buf = [0u8; EVENT_MAX];
        buf[..event.len()].copy_from_slice(event);
        Some(AuditNode { seq, at_s, event: buf })
    }

    /// 节点哈希 = SHA-256(seq ‖ at ‖ event ‖ prev_hash)。
    /// 序号链的数学核心：任何一条被改，其后所有哈希跟着断。
    pub fn hash_with(&self, prev_hash: &[u8; 32]) -> [u8; 32] {
        let mut buf = [0u8; 8 + 8 + EVENT_MAX + 32];
        buf[0..8].copy_from_slice(&self.seq.to_be_bytes());
        buf[8..16].copy_from_slice(&self.at_s.to_be_bytes());
        buf[16..16 + EVENT_MAX].copy_from_slice(&self.event);
        buf[16 + EVENT_MAX..].copy_from_slice(prev_hash);
        ksha256::sha256(&buf)
    }
}

/// 链条（append-only 账本 + 逐节点哈希）。
pub struct AuditChain {
    nodes: Vec<AuditNode>,
    hashes: Vec<[u8; 32]>,
    /// 链头哈希（genesis = 全零哈希）。
    head: [u8; 32],
    /// 磁盘满事件计数（安全日志优先保障——P0 语义的对账字段）。
    pub disk_full_events: u64,
    /// 超长事件拒绝计数（调用方拆条后重试——审计不截断）。
    pub oversize_rejects: u64,
}

const GENESIS: [u8; 32] = [0u8; 32];

impl AuditChain {
    pub fn new() -> AuditChain {
        AuditChain { nodes: Vec::new(), hashes: Vec::new(), head: GENESIS, disk_full_events: 0, oversize_rejects: 0 }
    }

    /// **跨日续链**：以昨日链头（快照 manifest 锚）为本段 genesis——
    /// 跨日链可信的构造保证（F121 联动）。
    pub fn continued(prev_head: [u8; 32]) -> AuditChain {
        AuditChain { nodes: Vec::new(), hashes: Vec::new(), head: prev_head, disk_full_events: 0, oversize_rejects: 0 }
    }

    /// **append-only 追加**（唯一写入口——存储层强制写后只读的账本面）。
    /// 序号自动连续；返回所写序号。
    pub fn append(&mut self, at_s: u64, event: &[u8]) -> Option<u64> {
        let node = match AuditNode::new(0, at_s, event) {
            Some(n) => n,
            None => {
                self.oversize_rejects += 1;
                return None;
            }
        };
        let seq = (self.nodes.len() as u64) + 1;
        let mut n = node;
        n.seq = seq;
        let h = n.hash_with(&self.head);
        self.nodes.push(n);
        self.hashes.push(h);
        self.head = h;
        Some(seq)
    }

    /// 磁盘满上报（安全日志优先保障——F057 最高级的账本面；本层只记账，
    /// 让路动作在 IO 层执行）。
    pub fn note_disk_full(&mut self) {
        self.disk_full_events += 1;
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 链头哈希（每日锚定进快照 manifest 的值——F121 联动）。
    pub fn head_hash(&self) -> [u8; 32] {
        self.head
    }

    /// **链校验**（判据一核心）：重放全链，逐节点对拍哈希。
    ///
    /// 语义：返回首断点（序号）——篡改定位到条；None=全链完整。
    /// `expected_head`：跨日校验时传入昨日锚（快照 manifest 里的链头）；
    /// None 时只验链内自洽（genesis=全零哈希）。
    pub fn verify(&self, expected_head: Option<[u8; 32]>) -> VerifyReport {
        let mut prev = GENESIS;
        let mut start = 0usize;
        if let Some(anchor) = expected_head {
            if self.nodes.is_empty() {
                return VerifyReport { ok: true, broken_at: None, checked: 0 };
            }
            let expect = self.nodes[0].hash_with(&anchor);
            if expect != self.hashes[0] {
                return VerifyReport { ok: false, broken_at: Some(1), checked: 1 };
            }
            prev = self.hashes[0];
            start = 1;
        }
        for (i, n) in self.nodes.iter().enumerate().skip(start) {
            let expect = n.hash_with(&prev);
            if expect != self.hashes[i] {
                return VerifyReport { ok: false, broken_at: Some(n.seq), checked: i + 1 };
            }
            prev = self.hashes[i];
        }
        VerifyReport { ok: true, broken_at: None, checked: self.nodes.len() }
    }

    /// **篡改注入矩阵**（判据三攻击面的实现侧写）：在副本上执行攻击后校验。
    /// 三攻击：改一条 / 删一条 / 插一条——全部必须被检出且定位。
    pub fn tamper_matrix(&self) -> TamperMatrix {
        // 攻击一：改一条（改中间事件体）。
        let mut m = self.clone_shallow();
        if m.len() >= 2 {
            m.nodes[1].event[0] ^= 0xFF;
        }
        let modify = m.verify(None);
        // 攻击二：删一条（删中间节点——哈希链脱节）。
        let mut d = self.clone_shallow();
        if d.len() >= 3 {
            d.nodes.remove(1);
            d.hashes.remove(1);
            // 删除后重排序号（真实篡改者会做的事——链仍须断）。
            for (i, n) in d.nodes.iter_mut().enumerate() {
                n.seq = i as u64 + 1;
            }
        }
        let delete = d.verify(None);
        // 攻击三：插一条（插入伪造节点并重排序号——链仍须断）。
        let mut i = self.clone_shallow();
        if !i.nodes.is_empty() {
            let forged = AuditNode::new(0, 999, b"forged").unwrap();
            i.nodes.insert(1, forged);
            i.hashes.insert(1, forged.hash_with(&GENESIS));
            for (k, n) in i.nodes.iter_mut().enumerate() {
                n.seq = k as u64 + 1;
            }
        }
        let insert = i.verify(None);
        TamperMatrix {
            modify_detected: !modify.ok && modify.broken_at.is_some(),
            delete_detected: !delete.ok && delete.broken_at.is_some(),
            insert_detected: !insert.ok && insert.broken_at.is_some(),
            modify_at: modify.broken_at,
            delete_at: delete.broken_at,
            insert_at: insert.broken_at,
        }
    }

    fn clone_shallow(&self) -> AuditChain {
        AuditChain {
            nodes: self.nodes.clone(),
            hashes: self.hashes.clone(),
            head: self.head,
            disk_full_events: 0,
            oversize_rejects: 0,
        }
    }

    /// 导出包数据（zip 内三文件+校验器的数据面）：节点流+哈希流+链头。
    /// 校验器随包分发（F126 开源——第三方独立可验）。
    pub fn export_bundle(&self) -> ExportBundle {
        ExportBundle {
            nodes: self.nodes.clone(),
            hashes: self.hashes.clone(),
            head: self.head,
        }
    }
}

impl Default for AuditChain {
    fn default() -> Self {
        Self::new()
    }
}

/// 校验报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub ok: bool,
    /// 首断点序号（None=完整）。
    pub broken_at: Option<u64>,
    /// 已校验节点数。
    pub checked: usize,
}

/// 篡改矩阵结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TamperMatrix {
    pub modify_detected: bool,
    pub delete_detected: bool,
    pub insert_detected: bool,
    pub modify_at: Option<u64>,
    pub delete_at: Option<u64>,
    pub insert_at: Option<u64>,
}

/// 导出包（节点+哈希+链头——第三方校验器的输入）。
#[derive(Clone, Debug)]
pub struct ExportBundle {
    pub nodes: Vec<AuditNode>,
    pub hashes: Vec<[u8; 32]>,
    pub head: [u8; 32],
}

impl ExportBundle {
    /// **第三方独立校验**（判据二）：不依赖本模块状态，从导出数据自证。
    /// ~20 行可读校验逻辑——审计工具自己要经得起读。
    pub fn independent_verify(&self) -> VerifyReport {
        let mut prev = GENESIS;
        for (i, n) in self.nodes.iter().enumerate() {
            let expect = n.hash_with(&prev);
            if expect != self.hashes[i] {
                return VerifyReport { ok: false, broken_at: Some(n.seq), checked: i + 1 };
            }
            prev = self.hashes[i];
        }
        // 链头对拍（导出包内带链头——双保险）。
        if self.hashes.last().map(|h| *h != self.head).unwrap_or(false) {
            return VerifyReport { ok: false, broken_at: Some(self.nodes.len() as u64), checked: self.nodes.len() };
        }
        VerifyReport { ok: true, broken_at: None, checked: self.nodes.len() }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F194 自检（聚合进 secstar2 域）。
pub fn run_auditchain_checks() -> CheckSet {
    let mut set = CheckSet::new("F194-auditchain");

    // 正常链：追加若干条 → 校验绿 + 链头非零。
    let mut c = AuditChain::new();
    for i in 0..10u64 {
        set.add("append ok", c.append(1_000 + i, b"perm-change audit event").is_some(), "");
    }
    let rep = c.verify(None);
    set.add("clean chain", rep.ok && rep.broken_at.is_none() && rep.checked == 10, "");
    set.add("head nonzero", c.head_hash() != GENESIS, "");

    // 判据三：篡改注入矩阵——改/删/插三攻击全检出且定位。
    let m = c.tamper_matrix();
    set.add("modify detected", m.modify_detected, "");
    set.add("delete detected", m.delete_detected, "");
    set.add("insert detected", m.insert_detected, "");
    set.add("modify located", m.modify_at == Some(2), "first tampered node");
    set.add("delete located", m.delete_at.is_some(), "");
    set.add("insert located", m.insert_at.is_some(), "");

    // 判据二：导出包第三方独立校验（自证链路）。
    let bundle = c.export_bundle();
    let iv = bundle.independent_verify();
    set.add("third-party verify", iv.ok && iv.checked == 10, "");

    // 跨日锚定：昨日链头为锚 → 续链段可验；锚不符 → 断。
    let anchor = c.head_hash();
    let mut c2 = AuditChain::continued(anchor);
    let _ = c2.append(2_000, b"next-day event");
    set.add("anchor day2 ok", c2.verify(Some(anchor)).ok, "");
    let mut wrong_anchor = anchor;
    wrong_anchor[0] ^= 1;
    set.add("anchor mismatch", !c2.verify(Some(wrong_anchor)).ok, "");

    // 超长拒绝（审计条目不截断——拆条重试语义）。
    let long = [0x41u8; EVENT_MAX + 1];
    set.add("oversize reject", c.append(3_000, &long).is_none() && c.oversize_rejects == 1, "");

    // 磁盘满账本（安全日志优先保障——P0 对账字段）。
    c.note_disk_full();
    set.add("disk full counted", c.disk_full_events == 1, "");

    // 空链校验绿（无事发生不是错）。
    set.add("empty chain ok", AuditChain::new().verify(None).ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f194_hash_changes_with_every_field() {
        let base = AuditNode::new(1, 100, b"event").unwrap();
        let h0 = base.hash_with(&GENESIS);
        // 改事件体 → 哈希变。
        let mut e1 = base;
        e1.event[0] = b'X';
        assert_ne!(h0, e1.hash_with(&GENESIS));
        // 改时间戳 → 哈希变。
        let mut e2 = base;
        e2.at_s = 101;
        assert_ne!(h0, e2.hash_with(&GENESIS));
        // 改前哈希 → 哈希变（序号链的核心性质）。
        let mut prev = GENESIS;
        prev[0] = 1;
        assert_ne!(h0, base.hash_with(&prev));
        // 同输入 → 同哈希（确定性）。
        assert_eq!(h0, base.hash_with(&GENESIS));
    }

    #[test]
    fn f194_seq_auto_continuous() {
        let mut c = AuditChain::new();
        assert_eq!(c.append(1, b"a"), Some(1));
        assert_eq!(c.append(2, b"b"), Some(2));
        assert_eq!(c.append(3, b"c"), Some(3));
        assert_eq!(c.nodes[2].seq, 3);
    }

    #[test]
    fn f194_tamper_last_node_detected() {
        // 改最后一条：链头哈希对不上（与锚定面联动的真实场景）。
        let mut c = AuditChain::new();
        for i in 0..4 {
            let _ = c.append(i, b"evt");
        }
        let mut m = c.clone_shallow();
        let last = m.nodes.len() - 1;
        m.nodes[last].at_s += 1;
        let rep = m.verify(None);
        assert!(!rep.ok);
        assert_eq!(rep.broken_at, Some(4));
    }

    #[test]
    fn f194_delete_last_node_detected_via_head() {
        // 删最后一条：链头对拍（expected_head 语义）直接抓包。
        let mut c = AuditChain::new();
        for i in 0..4 {
            let _ = c.append(i, b"evt");
        }
        let true_head = c.head_hash();
        let mut d = c.clone_shallow();
        d.nodes.pop();
        d.hashes.pop();
        d.head = d.hashes.last().copied().unwrap_or(GENESIS);
        // 独立校验器对拍链头字段 → 删尾被检出。
        let mut bundle = d.export_bundle();
        bundle.head = true_head;
        assert!(!bundle.independent_verify().ok);
    }

    #[test]
    fn f194_event_bytes_zero_padded() {
        let n = AuditNode::new(1, 1, b"ab").unwrap();
        assert_eq!(&n.event[..2], b"ab");
        assert!(n.event[2..].iter().all(|&b| b == 0));
    }

    #[test]
    fn f194_run_checks_pass() {
        assert!(run_auditchain_checks().all_passed());
    }
}
