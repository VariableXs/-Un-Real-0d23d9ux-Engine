//! STAR-V2 生态域共享底盘（AI-V2 · F131-F150 共用基础设施）。
//!
//! 生态域的二十项功能高度同构：都要**登记（ledger）—追踪（trace id）—
//! 状态流转（state machine）—限频（rate limit）—验真（fingerprint）**。
//! 五件公共件收在本底盘，全域一处一事实：
//!
//! - [`TraceId`] 追踪编号：`前缀-YYYYMMDD-序号` 三段式（F132 差异条目、
//!   F139 反馈报告、F142 安全报告共用同一条编号纪律）；
//! - [`SeqLedger`] append-only 序号链台账：条目带前序指纹串联，改一条
//!   断链即检出（F148 裁决记录册 / F144 授予册 / F131 回馈记录册共用）；
//! - [`State5`] 五态状态机：提交→确认→处理→已解决→关闭，非法跳转
//!   显式拒绝（F139 与 F129 复用同引擎——一处一事实，F129 落地时直接
//!   引用本件）；
//! - [`RateGate`] 固定窗限频器：防刷量（F139 恶意刷量 / F134 举报滥用
//!   共用同一条限频纪律）；
//! - [`fnv1a64`] 与 [`Coverage`]：签名指纹（F137 API 签名 / F144 徽标
//!   验真 / F146 源签名共用同一条哈希纪律）与覆盖度计算（F135 API 提取
//!   覆盖率 / F140 翻译覆盖度共用同一条口径）。
//!
//! 时间纪律：一切时间由调用方以参数注入（日序/毫秒戳），模块不持真实
//! 时钟——宿主测试可确定复现，内核侧由上层供给真值。

use alloc::vec::Vec;
use core::fmt;

// ---------------------------------------------------------------------------
// TraceId — 追踪编号（前缀-YYYYMMDD-序号）
// ---------------------------------------------------------------------------

/// 追踪编号三段式。前缀定长（≤8 字节），日期为 `yyyymmdd` 压缩整数，
/// 序号当日从 1 递增。
///
/// 判据锚：F132「每条有追踪编号可查」/ F139「编号全局唯一（日期+序号）」
/// / F142「报告编号」——三处共用同一条编号纪律（一处一事实）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceId {
    prefix: [u8; 8],
    prefix_len: u8,
    /// 压缩日期 yyyymmdd（如 20260926）。
    pub day: u32,
    pub seq: u32,
}

impl TraceId {
    /// 生成编号。前缀超 8 字节或含非法字符（非 ASCII 大写字母/数字），
    /// 日期非 8 位十进制（月份 01-12、日 01-31 之外的直接拒绝），序号
    /// 为 0 —— 全部按非法入参处理，回退为 `BAD-00000000-0` 语义占位
    /// （显性化，绝不静默生成看似合法的编号）。
    pub fn new(prefix: &str, day: u32, seq: u32) -> TraceId {
        let mut id = TraceId { prefix: [0; 8], prefix_len: 0, day: 0, seq: 0 };
        let bytes = prefix.as_bytes();
        if bytes.is_empty() || bytes.len() > 8 {
            return id;
        }
        for (i, &b) in bytes.iter().enumerate() {
            let ok = b.is_ascii_uppercase() || b.is_ascii_digit();
            if !ok {
                return id;
            }
            id.prefix[i] = b;
            id.prefix_len = i as u8 + 1;
        }
        if !valid_day(day) || seq == 0 {
            return TraceId { prefix: [0; 8], prefix_len: 0, day: 0, seq: 0 };
        }
        id.day = day;
        id.seq = seq;
        id
    }

    pub fn is_valid(&self) -> bool {
        self.prefix_len > 0 && self.day != 0 && self.seq != 0
    }

    /// 渲染为 `PREFIX-YYYYMMDD-SEQ`（缓冲不足截断并返回写入数——调用方
    /// 必须核对返回长度，诚实面不许假装写完）。
    pub fn render(&self, buf: &mut [u8]) -> usize {
        if !self.is_valid() {
            const BAD: &[u8] = b"BAD-ID";
            let n = BAD.len().min(buf.len());
            buf[..n].copy_from_slice(&BAD[..n]);
            return n;
        }
        let mut pos = 0;
        for i in 0..self.prefix_len as usize {
            if pos >= buf.len() {
                return pos;
            }
            buf[pos] = self.prefix[i];
            pos += 1;
        }
        // 渲染预算：'-' + 8 位日期 + '-' + 序号位数
        let need = |pos: usize, more: usize| pos + more <= buf.len();
        if !need(pos, 1 + 8 + 1 + digits10(self.seq)) {
            return pos;
        }
        buf[pos] = b'-';
        pos += 1;
        let mut daybuf = [0u8; 8];
        write_day(self.day, &mut daybuf);
        buf[pos..pos + 8].copy_from_slice(&daybuf);
        pos += 8;
        buf[pos] = b'-';
        pos += 1;
        let seq_digits = digits10(self.seq);
        write_u32(self.seq, &mut buf[pos..pos + seq_digits]);
        pos += seq_digits;
        pos
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0u8; 32];
        let n = self.render(&mut buf);
        // render 只产出 ASCII
        match core::str::from_utf8(&buf[..n]) {
            Ok(s) => f.write_str(s),
            Err(_) => f.write_str("BAD-ID"),
        }
    }
}

fn valid_day(day: u32) -> bool {
    let y = day / 10000;
    let m = (day / 100) % 100;
    let d = day % 100;
    (1000..=9999).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d)
}

fn digits10(mut v: u32) -> usize {
    let mut n = 1;
    while v >= 10 {
        v /= 10;
        n += 1;
    }
    n
}

fn write_u32(mut v: u32, out: &mut [u8]) {
    for i in (0..out.len()).rev() {
        out[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
}

fn write_day(day: u32, out: &mut [u8; 8]) {
    out[0] = b'0' + (day / 10_000_000 % 10) as u8;
    out[1] = b'0' + (day / 1_000_000 % 10) as u8;
    out[2] = b'0' + (day / 100_000 % 10) as u8;
    out[3] = b'0' + (day / 10_000 % 10) as u8;
    out[4] = b'0' + (day / 1_000 % 10) as u8;
    out[5] = b'0' + (day / 100 % 10) as u8;
    out[6] = b'0' + (day / 10 % 10) as u8;
    out[7] = b'0' + (day % 10) as u8;
}

/// 当日序号分配器：按日期换日归零，全局唯一由「日期+序号」二元组保证。
pub struct SeqAlloc {
    cur_day: u32,
    next: u32,
}

impl SeqAlloc {
    pub const fn new() -> SeqAlloc {
        SeqAlloc { cur_day: 0, next: 1 }
    }

    /// 取当日下一个序号；换日自动归零。日序上限 99,999（第 10 万条起
    /// 显式返回 0 表示当日额度耗尽——调用方必须拒绝而非绕过）。
    pub fn take(&mut self, day: u32) -> u32 {
        if day != self.cur_day {
            self.cur_day = day;
            self.next = 1;
        }
        if self.next > 99_999 {
            return 0;
        }
        let s = self.next;
        self.next += 1;
        s
    }
}

// ---------------------------------------------------------------------------
// SeqLedger — append-only 序号链台账
// ---------------------------------------------------------------------------

/// 台账条目：业务载荷指纹 + 前序指纹串联。
///
/// 判据锚：F148「裁决记录册 append-only（F194 序号链同款）」/ F137
/// 「CI 签名比对门禁」/ F144「签名验真」——链条完整性 = 逐条重算指纹
/// 与前序字段一致。改任何一条历史记录，其后续所有指纹全部断链。
pub struct SeqLedger {
    entries: Vec<LedgerEntry>,
}

#[derive(Clone, Copy, Debug)]
pub struct LedgerEntry {
    pub seq: u32,
    /// 载荷 FNV-1a 64 指纹。
    pub payload_fp: u64,
    /// 前序条目链指纹（首条为 0）。
    pub prev_chain: u64,
    /// 本条链指纹 = fnv1a64(payload_fp ^ prev_chain ^ seq)。
    pub chain: u64,
}

impl SeqLedger {
    pub const fn new() -> SeqLedger {
        SeqLedger { entries: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 追加一条：返回链指纹。载荷指纹由调用方对不可变字节算出——本层
    /// 不复制载荷，只钉指纹（内存纪律：台账只存 24 字节/条）。
    pub fn append(&mut self, payload_fp: u64) -> u64 {
        let seq = self.entries.len() as u32 + 1;
        let prev_chain = self.entries.last().map(|e| e.chain).unwrap_or(0);
        let chain = fnv1a64(&payload_fp.to_le_bytes())
            ^ fnv1a64(&prev_chain.to_le_bytes())
            ^ fnv1a64(&seq.to_le_bytes());
        self.entries.push(LedgerEntry { seq, payload_fp, prev_chain, chain });
        chain
    }

    /// 完整性自证：逐条重算链指纹，全对返回 true。
    pub fn verify(&self) -> bool {
        let mut expect_prev = 0u64;
        let mut expect_seq = 1u32;
        for e in &self.entries {
            let chain = fnv1a64(&e.payload_fp.to_le_bytes())
                ^ fnv1a64(&expect_prev.to_le_bytes())
                ^ fnv1a64(&e.seq.to_le_bytes());
            if e.seq != expect_seq || e.prev_chain != expect_prev || e.chain != chain {
                return false;
            }
            expect_seq += 1;
            expect_prev = e.chain;
        }
        true
    }

    /// 篡改注入演示：把第 `idx` 条载荷指纹改为 `fake_fp` 后整链必红
    /// （判据「改一条序号断链即检出」的自证路径）。
    pub fn tamper_detect(&self, idx: usize, fake_fp: u64) -> bool {
        let mut probe = SeqLedger { entries: self.entries.clone() };
        if idx >= probe.entries.len() {
            return false;
        }
        probe.entries[idx].payload_fp = fake_fp;
        !probe.verify()
    }
}

// ---------------------------------------------------------------------------
// State5 — 五态状态机
// ---------------------------------------------------------------------------

/// 五态：Submitted → Confirmed → Investigating → Resolved → Closed。
///
/// 判据锚：F139「状态五态与 F129 复用同引擎（一处一事实）」。
/// 合法前进路径单向不回头（已解决回退 = 开新态记录，不改历史——与
/// append-only 纪律同源）；`Resolved → Closed` 允许旁路 `Reopened`
/// 语义用**新建同链编号 +0x10000 偏移序号**表达，不在本机内回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State5 {
    Submitted,
    Confirmed,
    Investigating,
    Resolved,
    Closed,
}

impl State5 {
    pub fn index(self) -> u8 {
        match self {
            State5::Submitted => 0,
            State5::Confirmed => 1,
            State5::Investigating => 2,
            State5::Resolved => 3,
            State5::Closed => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            State5::Submitted => "submitted",
            State5::Confirmed => "confirmed",
            State5::Investigating => "investigating",
            State5::Resolved => "resolved",
            State5::Closed => "closed",
        }
    }

    /// 严格单步前进：只允许 +1 步。跳步（0→3）与倒退一律拒绝。
    pub fn advance(self) -> Option<State5> {
        match self {
            State5::Submitted => Some(State5::Confirmed),
            State5::Confirmed => Some(State5::Investigating),
            State5::Investigating => Some(State5::Resolved),
            State5::Resolved => Some(State5::Closed),
            State5::Closed => None,
        }
    }
}

/// 一条状态轨迹：当前态 + 流转历史（天数序列，供时限审计）。
#[derive(Clone, Copy, Debug)]
pub struct StateTrack {
    pub state: State5,
    /// 每次进入当前态的日序（逐态记录，容量 5 恰好对应五态）。
    entered_on: [u32; 5],
    steps: u8,
}

impl StateTrack {
    pub fn new(day: u32) -> StateTrack {
        let mut entered_on = [0u32; 5];
        entered_on[0] = day;
        StateTrack { state: State5::Submitted, entered_on, steps: 1 }
    }

    /// 前进一步。返回 Err(current) 表示非法（终态或重复推进同一请求）。
    pub fn advance(&mut self, day: u32) -> Result<State5, State5> {
        match self.state.advance() {
            Some(next) => {
                self.state = next;
                let i = next.index() as usize;
                self.entered_on[i] = day;
                self.steps += 1;
                Ok(next)
            }
            None => Err(self.state),
        }
    }

    /// 时限审计：从 Submitted 到 Confirmed 是否在 `limit_days` 内
    /// （F142 的 48h=2 天线复用本件口径）。
    pub fn confirm_within(&self, limit_days: u32) -> bool {
        if self.steps < 2 || self.entered_on[1] == 0 {
            return false;
        }
        self.entered_on[1].saturating_sub(self.entered_on[0]) <= limit_days
    }
}

// ---------------------------------------------------------------------------
// RateGate — 固定窗限频器
// ---------------------------------------------------------------------------

/// 固定窗限频：窗宽 `window_ms` 毫秒内最多 `cap` 次。
///
/// 判据锚：F139「恶意刷量 → F129 同限频」/ F134 举报防滥用。
pub struct RateGate {
    window_ms: u64,
    cap: u32,
    win_start: u64,
    win_count: u32,
    pub rejected_total: u64,
}

impl RateGate {
    pub const fn new(window_ms: u64, cap: u32) -> RateGate {
        RateGate { window_ms, cap, win_start: 0, win_count: 0, rejected_total: 0 }
    }

    /// 请求放行一次。`now_ms` 单调递增（回拨视为同窗——时钟回拨不许
    /// 变相绕限频，宁可错杀不放过）。
    pub fn admit(&mut self, now_ms: u64) -> bool {
        if now_ms < self.win_start {
            self.rejected_total += 1;
            return false;
        }
        if now_ms.saturating_sub(self.win_start) >= self.window_ms {
            self.win_start = now_ms;
            self.win_count = 0;
        }
        if self.win_count >= self.cap {
            self.rejected_total += 1;
            return false;
        }
        self.win_count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// 指纹与覆盖度
// ---------------------------------------------------------------------------

/// FNV-1a 64：生态域统一指纹算法。用途都是「完整性比对」不是密码学
/// 防御——F142 的真签名走内核既有 ksha256/kvault 通道，本件只做轻量
/// 对账（登记入完成报告的边界声明）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 覆盖度：covered/total，返回千分比与「测试版」标注。
///
/// 判据锚：F140「覆盖度 <90% 标测试版语言包」/ F135「API 提取覆盖率
/// >90%」。两个方向共用同一条千分比口径，判定阈值由调用方传入。
pub const COVERAGE_BETA_THRESHOLD_PPT: u32 = 900;

pub fn coverage_ppt(covered: usize, total: usize) -> u32 {
    if total == 0 {
        return 0;
    }
    ((covered as u64 * 1000) / total as u64) as u32
}

pub fn coverage_is_beta(covered: usize, total: usize) -> bool {
    total == 0 || coverage_ppt(covered, total) < COVERAGE_BETA_THRESHOLD_PPT
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// 域标识（CheckSet 聚合用）。
pub const EBASE_TAG: &str = "stareco-ebase";

pub fn run_ebase_checks() -> CheckSet {
    let mut set = CheckSet::new(EBASE_TAG);

    // TraceId：合法 / 前缀非法字符 / 日期非法 / 渲染往返
    let id = TraceId::new("DIFF", 20260926, 7);
    set.add("trace valid id", id.is_valid() && id.day == 20260926 && id.seq == 7, "fields");
    set.add("trace bad char rejected", !TraceId::new("diff-x", 20260926, 1).is_valid(), "lowercase+dash");
    set.add("trace bad day rejected", !TraceId::new("DIFF", 20261399, 1).is_valid(), "month13 day99");
    set.add("trace zero seq rejected", !TraceId::new("DIFF", 20260926, 0).is_valid(), "seq0");
    let mut buf = [0u8; 32];
    let n = id.render(&mut buf);
    set.add(
        "trace render roundtrip",
        core::str::from_utf8(&buf[..n]) == Ok("DIFF-20260926-7"),
        "ascii render",
    );

    // SeqAlloc：换日归零 / 额度耗尽
    let mut alloc = SeqAlloc::new();
    let s1 = alloc.take(20260926);
    let s2 = alloc.take(20260926);
    let s3 = alloc.take(20260927);
    set.add("seq alloc monotonic", s1 == 1 && s2 == 2, "same day increments");
    set.add("seq alloc day rollover", s3 == 1, "next day resets");

    // SeqLedger：追加-验证-篡改检出
    let mut ledger = SeqLedger::new();
    let c1 = ledger.append(fnv1a64(b"entry-1"));
    let c2 = ledger.append(fnv1a64(b"entry-2"));
    let c3 = ledger.append(fnv1a64(b"entry-3"));
    set.add("ledger verify clean", ledger.verify() && c1 != c2 && c2 != c3, "chain green");
    set.add("ledger tamper detected", ledger.tamper_detect(1, fnv1a64(b"forged")), "break detected");
    set.add("ledger out-of-range probe", !ledger.tamper_detect(9, 1), "no false alarm");

    // State5：单步前进 / 跳步拒绝 / 终态
    let mut tr = StateTrack::new(100);
    let a = tr.advance(101);
    let b = tr.advance(102);
    let c = tr.advance(102); // 同日重复推进合法（不跨日约束）
    set.add(
        "state single-step path",
        a == Ok(State5::Confirmed) && b == Ok(State5::Investigating) && c == Ok(State5::Resolved),
        "ladder",
    );
    set.add("state terminal", tr.advance(103) == Ok(State5::Closed) && tr.advance(104).is_err(), "closed stops");
    let mut fast = StateTrack::new(200);
    let _ = fast.advance(201);
    set.add("state confirm within 2d", fast.confirm_within(2), "48h line");
    let mut slow = StateTrack::new(200);
    let _ = slow.advance(210);
    set.add("state confirm late", !slow.confirm_within(2), "late flagged");

    // RateGate：窗内封顶 / 换窗复位 / 时钟回拨拒绝
    let mut gate = RateGate::new(1000, 3);
    let g1 = gate.admit(0) && gate.admit(100) && gate.admit(200);
    let g2 = !gate.admit(300);
    let g3 = gate.admit(1500);
    set.add("rate gate windowed", g1 && g2 && g3, "cap then reset");
    set.add("rate gate rollback refused", !gate.admit(1400), "monotonic only");

    // 覆盖度
    set.add("coverage ppt", coverage_ppt(95, 100) == 950, "95%");
    set.add("coverage beta line", coverage_is_beta(89, 100) && !coverage_is_beta(90, 100), "900 threshold");
    set.add("coverage empty total", coverage_is_beta(0, 0), "empty counts as beta");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_render() {
        let id = TraceId::new("SEC", 20261231, 12345);
        let mut buf = [0u8; 32];
        let n = id.render(&mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n]), Ok("SEC-20261231-12345"));
    }

    #[test]
    fn ledger_chain_integrity() {
        let mut l = SeqLedger::new();
        for i in 0..50u8 {
            l.append(fnv1a64(&[i, i, i]));
        }
        assert!(l.verify());
        assert!(l.tamper_detect(0, 0xDEAD_BEEF));
    }

    #[test]
    fn state_machine_discipline() {
        let mut t = StateTrack::new(1);
        for d in 2..=5u32 {
            assert!(t.advance(d).is_ok());
        }
        assert!(t.advance(6).is_err());
    }
}
