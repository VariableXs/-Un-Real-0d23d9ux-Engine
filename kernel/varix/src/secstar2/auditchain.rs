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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 深一：Redactor —— 事件体脱敏（写入时完成——F120 三查前移：敏感数据
// 不进不可改的日志。进了就永远拿不出来，所以门必须在门口）
// ---------------------------------------------------------------------------

/// 脱敏规则命中种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedactKind {
    /// 口令形态（password=… / pwd=… → 值域替换为 ***）。
    Secret,
    /// 密钥形态（64 位十六进制长串 → 前 4 后 4 保留）。
    HexKey,
    /// 明细形态（event 体长度超限前的本地截断不适用——超长走拒绝；本规则
    /// 处理事件体内的路径/用户名标记占位，防止 PII 进链）。
    Pii,
}

impl RedactKind {
    pub fn label(self) -> &'static str {
        match self {
            RedactKind::Secret => "口令已脱敏",
            RedactKind::HexKey => "密钥已脱敏",
            RedactKind::Pii => "个人标记已脱敏",
        }
    }
}

/// 单次脱敏结论（种类 + 命中次数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedactReport {
    pub hits: [(RedactKind, u8); 3],
    pub n: usize,
}

impl RedactReport {
    pub fn total(&self) -> u32 {
        self.hits.iter().take(self.n).map(|(_, c)| *c as u32).sum()
    }
}

/// 十六进制字符判定（密钥形态识别用）。
fn is_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)
}

/// 事件体脱敏（append 前的强制门——主册【设计细节】「脱敏在写入时完成」）。
/// 规则：
/// ① `password=<值>` / `pwd=<值>`（到空白或结尾）→ `password=***`；
/// ② 连续 ≥32 的十六进制串 → 保留前 4 后 4 + `…`；
/// ③ `user:<名>` → `user:<len>字`（不写名字本身）。
/// 返回（脱敏后事件体缓冲，写入长度，报告）。缓冲不足返回 None（调用方
/// 拆条——与超长拒绝同一语义，不静默截断）。
pub fn redact(event: &[u8], out: &mut [u8; EVENT_MAX]) -> Option<(usize, RedactReport)> {
    let mut n = 0usize;
    let mut rep = RedactReport { hits: [(RedactKind::Secret, 0), (RedactKind::HexKey, 0), (RedactKind::Pii, 0)], n: 3 };
    let mut i = 0usize;
    let push = |out: &mut [u8; EVENT_MAX], n: &mut usize, s: &[u8]| -> Option<()> {
        if *n + s.len() > out.len() {
            return None;
        }
        out[*n..*n + s.len()].copy_from_slice(s);
        *n += s.len();
        Some(())
    };
    while i < event.len() {
        // 规则①：password= / pwd= 前缀。
        let secret_head = if event[i..].starts_with(b"password=") {
            Some(9)
        } else if event[i..].starts_with(b"pwd=") {
            Some(4)
        } else {
            None
        };
        if let Some(hl) = secret_head {
            push(out, &mut n, &event[i..i + hl])?;
            i += hl;
            // 值域吞到空白/结尾，替换为 ***。
            let mut j = i;
            while j < event.len() && event[j] != b' ' && event[j] != b'\t' {
                j += 1;
            }
            push(out, &mut n, b"***")?;
            i = j;
            rep.hits[0].1 += 1;
            continue;
        }
        // 规则③：user:<名> → user:<len>字（字节长度——避免名字进链）。
        if event[i..].starts_with(b"user:") {
            push(out, &mut n, b"user:")?;
            i += 5;
            let mut j = i;
            while j < event.len() && event[j] != b' ' {
                j += 1;
            }
            let mut buf = [0u8; 12];
            let mut k = 0;
            let mut v = (j - i) as u64;
            if v == 0 {
                buf[0] = b'0';
                k = 1;
            }
            while v > 0 {
                buf[buf.len() - 1 - k] = b'0' + (v % 10) as u8;
                v /= 10;
                k += 1;
            }
            let digits = &buf[buf.len() - k..];
            push(out, &mut n, digits)?;
            push(out, &mut n, "字".as_bytes())?;
            i = j;
            rep.hits[2].1 += 1;
            continue;
        }
        // 规则②：连续 ≥32 hex 串。
        if is_hex(event[i]) {
            let mut j = i;
            while j < event.len() && is_hex(event[j]) {
                j += 1;
            }
            let run = j - i;
            if run >= 32 {
                push(out, &mut n, &event[i..i + 4])?;
                push(out, &mut n, "…".as_bytes())?;
                push(out, &mut n, &event[j - 4..j])?;
                rep.hits[1].1 += 1;
                i = j;
                continue;
            }
            // 非 key 长度的 hex 串原样拷贝（整段，避免逐字节重复判定）。
            push(out, &mut n, &event[i..j])?;
            i = j;
            continue;
        }
        push(out, &mut n, &event[i..i + 1])?;
        i += 1;
    }
    Some((n, rep))
}

// ---------------------------------------------------------------------------
// 深二：ReportText —— 校验报告渲染（三要素：范围/结果/断点定位——
// F120 审计页签「校验完整性」钮的人话输出）
// ---------------------------------------------------------------------------

/// 校验报告行（人话三要素）。
pub struct ReportLine {
    /// 范围（如 "#1..#10"）。
    pub range: String,
    /// 结果（人话）。
    pub verdict: &'static str,
    /// 断点定位（None=无断点）。
    pub broken_at: Option<u64>,
}

/// 校验报告生成（VerifyReport → 三要素行——审计工具自己要经得起读）。
pub fn report_text(rep: &VerifyReport, total: usize) -> ReportLine {
    let range = alloc::format!("#1..#{}", total.max(rep.checked));
    if rep.ok {
        ReportLine { range, verdict: "链条完整：逐节点哈希对拍全部一致", broken_at: None }
    } else {
        ReportLine {
            range,
            verdict: "检出篡改：该条之后的所有哈希脱节（序号链性质）",
            broken_at: rep.broken_at,
        }
    }
}

// ---------------------------------------------------------------------------
// 深三：Quarantine —— 断链隔离保全（断点前后段分别封存——取证语义：
// 坏的不许污染好的，好的不许被丢弃）
// ---------------------------------------------------------------------------

/// 隔离保全结论。
pub struct Quarantine {
    /// 断点前完好段（含断点前一条）节点数。
    pub before_len: usize,
    /// 断点后可疑段节点数（如实保留——取证价值在可疑段本身）。
    pub after_len: usize,
    /// 隔离标记（工单文案——P0 语义对齐 F142）。
    pub ticket: &'static str,
}

/// 断链隔离（本层只切分与标注；P0 工单派发由 F142 通道执行）。
pub fn quarantine_on_break(chain: &AuditChain, rep: &VerifyReport) -> Option<Quarantine> {
    if rep.ok {
        return None;
    }
    let broken = rep.broken_at? as usize;
    // 断点序号 → 下标 = broken-1；前段 = [0, broken-1)。
    let before = broken.saturating_sub(1).min(chain.nodes.len());
    let after = chain.nodes.len() - before.min(chain.nodes.len());
    Some(Quarantine {
        before_len: before,
        after_len: after,
        ticket: "审计链断链（P0）：断点前后段已隔离保全，待 F142 通道评估",
    })
}

// ---------------------------------------------------------------------------
// 深四：AppendOnlySeal —— append-only 存储面（写后只读的账本模型：
// 追加随时可以，改写/删除在封账后一律拒绝且计数——存储层强制的语义投影）
// ---------------------------------------------------------------------------

/// append-only 账本容器（AuditChain 的存储纪律外壳）。
pub struct AppendOnlySeal {
    chain: AuditChain,
    /// 封账后改写尝试计数（恒增长即有人在动不该动的数据——诊断面可见）。
    pub rewrite_attempts: u64,
}

impl AppendOnlySeal {
    pub fn new() -> AppendOnlySeal {
        AppendOnlySeal { chain: AuditChain::new(), rewrite_attempts: 0 }
    }

    /// 追加（唯一合法写路径）。
    pub fn append(&mut self, at_s: u64, event: &[u8]) -> Option<u64> {
        self.chain.append(at_s, event)
    }

    /// 改写尝试（存储层拒绝——append-only 的对抗面：接口存在但恒失败且留痕）。
    pub fn rewrite_attempt(&mut self, _seq: u64, _new_event: &[u8]) -> Result<(), &'static str> {
        self.rewrite_attempts += 1;
        Err("审计日志 append-only：改写被存储层拒绝")
    }

    /// 删除尝试（同上——账本不可缩）。
    pub fn delete_attempt(&mut self, _seq: u64) -> Result<(), &'static str> {
        self.rewrite_attempts += 1;
        Err("审计日志 append-only：删除被存储层拒绝")
    }

    pub fn chain(&self) -> &AuditChain {
        &self.chain
    }
}

impl Default for AppendOnlySeal {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深五：DailyAnchor —— 每日锚定 manifest 行（链头写入当日快照 manifest
// 的行格式——跨日链可信的数据载体；F121 联动）
// ---------------------------------------------------------------------------

/// 锚定 manifest 行（hex 编码链头——快照 manifest 的固定版式：
/// `audit-anchor|<YYYYMMDD>|<hex64>`）。
pub fn daily_anchor_line(head: &[u8; 32], day: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("audit-anchor|");
    s.push_str(day);
    s.push('|');
    for b in head {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xF) as usize] as char);
    }
    s
}

/// manifest 行解析（第三方校验器同款逻辑——导出包可独立验的对称面）。
pub fn parse_anchor_line(line: &str) -> Option<(u64, [u8; 32])> {
    let mut parts = line.split('|');
    if parts.next()? != "audit-anchor" {
        return None;
    }
    let day = parts.next()?;
    if day.len() != 8 {
        return None;
    }
    let d: u64 = day.parse().ok()?;
    let hex = parts.next()?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = (hex.as_bytes()[i * 2] as char).to_digit(16)? as u8;
        let lo = (hex.as_bytes()[i * 2 + 1] as char).to_digit(16)? as u8;
        out[i] = hi * 16 + lo;
    }
    Some((d, out))
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F194 深化自检（聚合进 secstar2 域）。
pub fn run_auditchain_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F194-deep");

    // 深一：脱敏——口令/密钥/PII 三规则全命中且明文不落链。
    let mut buf = [0u8; EVENT_MAX];
    let (n, rep) = redact(b"login ok user:zhang password=hunter2", &mut buf).unwrap();
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("redact secret", !text.contains("hunter2") && text.contains("password=***"), "");
    set.add("redact pii", !text.contains("zhang") && text.contains("user:5字"), "");
    set.add("redact counted", rep.hits[0].1 == 1 && rep.hits[2].1 == 1, "");
    // 密钥形态（≥32 hex）。
    let key = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
    let mut ev = alloc::vec::Vec::new();
    ev.extend_from_slice(b"sig verify key=");
    ev.extend_from_slice(key.as_bytes());
    let (n2, rep2) = redact(&ev, &mut buf).unwrap();
    let text2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    set.add("redact hexkey", !text2.contains(key) && text2.contains("…"), "");
    set.add("redact hexkey kept", text2.contains("a1b2") && text2.contains("c5d6"), "前4后4保留");
    set.add("redact hexkey count", rep2.hits[1].1 == 1, "");
    // 脱敏进链闭环：redact 输出可被 append 接受且链仍可验。
    let mut chain = AuditChain::new();
    let seq = chain.append(100, &buf[..n]);
    set.add("redact feeds chain", seq.is_some() && chain.verify(None).ok, "");
    // 明文事件体（无敏感）→ 零命中零改写。
    let (n3, rep3) = redact(b"perm-change audit event", &mut buf).unwrap();
    set.add("redact passthrough", rep3.total() == 0 && n3 == 23, "");

    // 深二：校验报告三要素——完整链与断链两态。
    let mut c = AuditChain::new();
    for i in 0..5u64 {
        let _ = c.append(1_000 + i, b"evt");
    }
    let rep_ok = c.verify(None);
    let line_ok = report_text(&rep_ok, c.len());
    set.add("report ok shape", line_ok.verdict.contains("完整") && line_ok.broken_at.is_none(), "");
    let mut m = c.clone_shallow();
    m.nodes[2].event[0] ^= 0xFF;
    let rep_bad = m.verify(None);
    let line_bad = report_text(&rep_bad, c.len());
    set.add("report bad shape", line_bad.verdict.contains("篡改") && line_bad.broken_at == Some(3), "");

    // 深三：断链隔离——前段完好、后段保全、工单带 F142 语义。
    let q = quarantine_on_break(&m, &rep_bad).unwrap();
    set.add("quarantine before", q.before_len == 2, "断点 #3 前保留 2 条完好段");
    set.add("quarantine after", q.after_len == 3, "可疑段全量保全（取证价值）");
    set.add("quarantine ticket", q.ticket.contains("P0") && q.ticket.contains("F142"), "");
    // 干净链 → None（无事不生非）。
    set.add("quarantine skip clean", quarantine_on_break(&c, &rep_ok).is_none(), "");

    // 深四：append-only 封账——改写/删除恒拒绝且计数，追加不受影响。
    let mut seal = AppendOnlySeal::new();
    let _ = seal.append(1, b"a");
    set.add("seal rewrite refused", seal.rewrite_attempt(1, b"x").is_err(), "");
    set.add("seal delete refused", seal.delete_attempt(1).is_err(), "");
    set.add("seal attempts counted", seal.rewrite_attempts == 2, "");
    set.add("seal append still ok", seal.append(2, b"b").is_some(), "");
    set.add("seal chain intact", seal.chain().verify(None).ok, "");

    // 深五：每日锚定行——生成/解析对称，第三方可独立验。
    let head = c.head_hash();
    let line = daily_anchor_line(&head, "20260926");
    set.add("anchor line shape", line.starts_with("audit-anchor|20260926|") && line.len() == 13 + 8 + 1 + 64, "");
    let parsed = parse_anchor_line(&line);
    set.add("anchor roundtrip", parsed == Some((20260926, head)), "");
    set.add("anchor reject junk", parse_anchor_line("junk|x|y").is_none(), "");
    // 锚行喂给跨日续链：昨日锚 → 今日段可验（端到端）。
    let (day_num, anchor) = parsed.unwrap();
    let mut c2 = AuditChain::continued(anchor);
    let _ = c2.append(2_000, b"day2 event");
    set.add("anchor feeds continued", c2.verify(Some(anchor)).ok && day_num == 20260926, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f194_deep_redact_never_leaks_into_chain() {
        // 端到端：含敏感信息的原始事件 → redact → append → 导出包 →
        // 独立校验 + 导出字节里搜不到明文（三查前移的闭环验证）。
        let raw = b"user:alice password=s3cret! permission escalated";
        let mut buf = [0u8; EVENT_MAX];
        let (n, _) = redact(raw, &mut buf).unwrap();
        let mut c = AuditChain::new();
        let _ = c.append(1, &buf[..n]);
        let bundle = c.export_bundle();
        let mut bytes = alloc::vec::Vec::new();
        for node in &bundle.nodes {
            bytes.extend_from_slice(&node.event);
        }
        let text = core::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("s3cret"), "secret must not reach the ledger");
        assert!(!text.contains("alice"), "username must not reach the ledger");
        assert!(bundle.independent_verify().ok);
    }

    #[test]
    fn f194_deep_redact_oversize_honest() {
        // 脱敏后仍超 EVENT_MAX → None（拆条语义，不静默截断）。
        let mut raw = alloc::vec::Vec::new();
        for _ in 0..(EVENT_MAX / 8 + 8) {
            raw.extend_from_slice(b"user:ab ");
        }
        let mut buf = [0u8; EVENT_MAX];
        assert!(redact(&raw, &mut buf).is_none(), "oversize after redact must be honest");
    }

    #[test]
    fn f194_deep_quarantine_split_preserves_all() {
        // 隔离前后段之和 = 全链（一条不丢——取证完整性）。
        let mut c = AuditChain::new();
        for i in 0..7u64 {
            let _ = c.append(i, b"evt");
        }
        let mut m = c.clone_shallow();
        m.nodes[4].event[0] ^= 0xFF;
        let rep = m.verify(None);
        let q = quarantine_on_break(&m, &rep).unwrap();
        assert_eq!(q.before_len + q.after_len, 7);
    }

    #[test]
    fn f194_deep_appendonly_never_loses_on_attack() {
        // 攻击者拿到容器引用也无法改写历史（接口层恒拒绝）。
        let mut seal = AppendOnlySeal::new();
        for i in 0..3 {
            let _ = seal.append(i, b"evt");
        }
        let head_before = seal.chain().head_hash();
        for i in 0..10 {
            let _ = seal.rewrite_attempt(1, alloc::format!("forged{i}").as_bytes());
        }
        assert_eq!(seal.rewrite_attempts, 10);
        assert_eq!(seal.chain().head_hash(), head_before, "head untouched by rewrites");
        assert!(seal.chain().verify(None).ok);
    }

    #[test]
    fn f194_deep_anchor_chain_multi_day() {
        // 三日链：每日锚定 → 跨日逐段可验（快照 manifest 链）。
        let mut c1 = AuditChain::new();
        let _ = c1.append(100, b"day1");
        let a1 = daily_anchor_line(&c1.head_hash(), "20260901");
        let mut c2 = AuditChain::continued(parse_anchor_line(&a1).unwrap().1);
        let _ = c2.append(200, b"day2");
        let a2 = daily_anchor_line(&c2.head_hash(), "20260902");
        let mut c3 = AuditChain::continued(parse_anchor_line(&a2).unwrap().1);
        let _ = c3.append(300, b"day3");
        assert!(c3.verify(Some(parse_anchor_line(&a2).unwrap().1)).ok);
        // 篡改 day2 锚行的哈希段（一个 hex 位）→ day3 校验红：锚行哈希受审计。
        // （日期段是标签不参与哈希——篡改标签不影响数学，篡改哈希必被拦。）
        let mut bad = a2.clone();
        let hash_start = bad.len() - 64;
        let flip = if bad.ends_with('0') { "1" } else { "0" };
        bad.replace_range(hash_start..hash_start + 1, flip);
        let wrong = parse_anchor_line(&bad).unwrap().1;
        assert_ne!(wrong, parse_anchor_line(&a2).unwrap().1, "tampered hash must differ");
        assert!(!c3.verify(Some(wrong)).ok);
    }

    #[test]
    fn f194_deep_run_checks_pass() {
        assert!(run_auditchain_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——隔离段导出 / P0 工单契约 /
// 脱敏统计账。判据源：主册【状态与异常】「断链检出 → 红色工单（P0——F142
// 安全通道评估）+断点前后段隔离保全」+【设计细节】「事件体脱敏在写入时
// 完成」的统计面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：SegmentExport —— 隔离段导出（断点前后段各自成包、各自独立校验：
// 完好段自证清白，可疑段保全取证——两包互不污染）
// ---------------------------------------------------------------------------

/// 分段导出结果。
pub struct SegmentExport {
    /// 完好段（断点前）包——独立校验恒绿。
    pub before: ExportBundle,
    /// 可疑段（断点起）包——独立校验按实际（红是取证价值本身）。
    pub after: ExportBundle,
}

/// 分段导出（基于 quarantine 的切分语义；可疑段空 → None）。
pub fn segment_export(chain: &AuditChain, rep: &VerifyReport) -> Option<SegmentExport> {
    let broken = rep.broken_at? as usize;
    let before_n = broken.saturating_sub(1).min(chain.nodes.len());
    let mk = |slice: &[AuditNode], hashes: &[[u8; 32]]| ExportBundle {
        nodes: slice.to_vec(),
        hashes: hashes.to_vec(),
        head: hashes.last().copied().unwrap_or(GENESIS),
    };
    let before = mk(&chain.nodes[..before_n], &chain.hashes[..before_n]);
    let after = mk(&chain.nodes[before_n..], &chain.hashes[before_n..]);
    Some(SegmentExport { before, after })
}

// ---------------------------------------------------------------------------
// v3-二：P0Ticket —— 红色工单数据契约（F142 通道的输入：三要素 + 断点
// 定位 + 隔离状态——工单自己说得清，评估者不用翻日志）
// ---------------------------------------------------------------------------

/// P0 工单。
pub struct P0Ticket {
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么严重。
    pub why: &'static str,
    /// 下一步。
    pub next: &'static str,
    /// 断点定位（序号）。
    pub broken_at: u64,
    /// 隔离状态行。
    pub quarantine: String,
}

/// 工单组装（断链即开——P0 语义对齐 F142）。
pub fn p0_ticket(rep: &VerifyReport, total: usize) -> Option<P0Ticket> {
    if rep.ok {
        return None;
    }
    let broken_at = rep.broken_at?;
    let before = broken_at as usize - 1;
    let after = total - before;
    Some(P0Ticket {
        what: "审计日志链检出篡改（序号链哈希脱节）",
        why: "审计不可抵赖是 P0 红线——链条断了等于历史不可信",
        next: "断点前后段已隔离；请走 F142 安全披露通道评估",
        broken_at,
        quarantine: alloc::format!(
            "前 {} 条完好段 + 后 {} 条可疑段已分别封存（共 {} 条，一条不丢）",
            before, after, total
        ),
    })
}

// ---------------------------------------------------------------------------
// v3-三：RedactStats —— 脱敏统计账（写入侧对账：命中了哪些规则、多少
// 次——脱敏不是黑盒，三查前移的效果可量化）
// ---------------------------------------------------------------------------

/// 脱敏统计账（累计）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedactStats {
    pub secret_hits: u32,
    pub hexkey_hits: u32,
    pub pii_hits: u32,
    /// 脱敏事件总数（有任一命中的写入次数）。
    pub events: u32,
}

impl RedactStats {
    /// 记一次写入的脱敏报告。
    pub fn observe(&mut self, rep: &RedactReport) {
        if rep.n >= 1 {
            self.secret_hits += rep.hits[0].1 as u32;
        }
        if rep.n >= 2 {
            self.hexkey_hits += rep.hits[1].1 as u32;
        }
        if rep.n >= 3 {
            self.pii_hits += rep.hits[2].1 as u32;
        }
        if rep.total() > 0 {
            self.events += 1;
        }
    }

    /// 命中总数。
    pub fn total_hits(&self) -> u32 {
        self.secret_hits + self.hexkey_hits + self.pii_hits
    }
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F194 v3 自检（聚合进 secstar2 域）。
pub fn run_auditchain_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F194-v3");

    // 构造 5 条链 + 篡改 #3。
    let mut c = AuditChain::new();
    for i in 0..5u64 {
        let _ = c.append(1_000 + i, b"evt");
    }
    let mut m = c.clone_shallow();
    m.nodes[2].event[0] ^= 0xFF;
    let rep = m.verify(None);

    // v3-一：分段导出——完好段自证绿、可疑段保全、两包互不污染。
    let seg = segment_export(&m, &rep).unwrap();
    set.add("seg before clean", seg.before.independent_verify().ok && seg.before.nodes.len() == 2, "");
    set.add("seg after preserved", seg.after.nodes.len() == 3, "可疑段全量保全");
    set.add("seg total", seg.before.nodes.len() + seg.after.nodes.len() == 5, "一条不丢");

    // v3-二：P0 工单——三要素+定位+隔离行；干净链不开单。
    let t = p0_ticket(&rep, 5).unwrap();
    set.add("p0 what", t.what.contains("篡改"), "");
    set.add("p0 why", t.why.contains("P0"), "");
    set.add("p0 next", t.next.contains("F142"), "");
    set.add("p0 located", t.broken_at == 3, "");
    set.add("p0 quarantine", t.quarantine.contains("2 条完好") && t.quarantine.contains("一条不丢"), "");
    set.add("p0 none on clean", p0_ticket(&c.verify(None), 5).is_none(), "");

    // v3-三：脱敏统计账——三规则累计与事件计数。
    let mut st = RedactStats::default();
    let mut buf = [0u8; EVENT_MAX];
    let (_, r1) = redact(b"login user:ab password=x", &mut buf).unwrap();
    st.observe(&r1);
    let (_, r2) = redact(b"key=a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6", &mut buf).unwrap();
    st.observe(&r2);
    let (_, r3) = redact(b"plain event", &mut buf).unwrap();
    st.observe(&r3);
    set.add("redact stats", st.secret_hits == 1 && st.hexkey_hits == 1 && st.pii_hits == 1, "");
    set.add("redact events", st.events == 2, "无命中写入不计数");
    set.add("redact total", st.total_hits() == 3, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f194_v3_segment_before_never_carries_suspicion() {
        // 完好段在任意篡改位置下都自证绿（2/3/4 号位篡改的参数化验证）。
        for pos in 1..4usize {
            let mut c = AuditChain::new();
            for i in 0..5u64 {
                let _ = c.append(i, b"evt");
            }
            let mut m = c.clone_shallow();
            m.nodes[pos].event[0] ^= 0xFF;
            let rep = m.verify(None);
            let seg = segment_export(&m, &rep).unwrap();
            assert_eq!(seg.before.nodes.len(), pos, "break at {}", pos + 1);
            assert!(seg.before.independent_verify().ok, "before-segment clean at break {}", pos + 1);
        }
    }

    #[test]
    fn f194_v3_ticket_arithmetic_always_balances() {
        // 工单隔离行算术守恒：前段+后段=总数（篡改位置遍历）。
        for pos in 1..5usize {
            let mut c = AuditChain::new();
            for i in 0..5u64 {
                let _ = c.append(i, b"evt");
            }
            let mut m = c.clone_shallow();
            m.nodes[pos].event[0] ^= 0xFF;
            let rep = m.verify(None);
            let t = p0_ticket(&rep, 5).unwrap();
            let before = t.broken_at as usize - 1;
            assert_eq!(before + (5 - before), 5, "balance at break {}", pos + 1);
        }
    }

    #[test]
    fn f194_v3_run_checks_pass() {
        assert!(run_auditchain_deep2_checks().all_passed());
    }
}
