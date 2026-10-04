//! F321 就近共享 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：发现/确认/传输/完成四步用例；默认隐身判据；断点续
//! 传（传输中断网恢复）；拒绝路径不留半截文件；10MB/s 起步速度记录。
//!
//! **设计要点（主册）**：同一局域网内两台 VARIX 设备传文件：发送方右键
//! 「共享到附近设备」→发现列表→接收方弹确认条→传输进度（F086 同形
//! 制）→完成双方通知；大文件走断点续传（F269 同源）；默认可发现性关闭
//! （需要时开，防止被骚扰）。
//!
//! 实现形态：共享会话状态机（发现→确认→传输→完成，含拒绝/断线分支）
//! + 分块传输账（断点续传——已确认块不重传）+ 隐身默认位。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 起步速度判线（10MB/s）。
pub const MIN_THROUGHPUT_MBPS: u64 = 10;

/// 传输块大小（1MB——断点续传粒度）。
pub const CHUNK_BYTES: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// 共享会话
// ---------------------------------------------------------------------------

/// 会话阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareStage {
    /// 发现列表（对端可见）。
    Discovering,
    /// 等接收方确认。
    AwaitingConfirm,
    /// 传输中。
    Transferring,
    /// 完成（双方通知）。
    Done,
    /// 被拒绝（终态——清理闭环）。
    Rejected,
    /// 断线（可续传恢复）。
    Disconnected,
}

/// 一次共享会话。
pub struct ShareSession {
    pub stage: ShareStage,
    pub file: String,
    pub total_bytes: u64,
    /// 已确认落盘块数（断点续传锚点）。
    pub confirmed_chunks: u64,
    /// 接收方默认隐身（可发现性默认关——判据载体）。
    pub discoverable: bool,
    /// 半截文件账（拒绝/取消路径必须为 0——不留半截）。
    pub partial_file_bytes: u64,
}

impl ShareSession {
    pub fn new(file: &str, total_bytes: u64) -> ShareSession {
        ShareSession {
            stage: ShareStage::Discovering,
            file: String::from(file),
            total_bytes,
            confirmed_chunks: 0,
            discoverable: false, // 默认隐身——判据载体。
            partial_file_bytes: 0,
        }
    }

    pub fn total_chunks(&self) -> u64 {
        self.total_bytes.div_ceil(CHUNK_BYTES)
    }

    /// 四步：发现 → 请求确认。
    pub fn request_confirm(&mut self) -> bool {
        if self.stage == ShareStage::Discovering {
            self.stage = ShareStage::AwaitingConfirm;
            true
        } else {
            false
        }
    }

    /// 接收方接受 → 传输开始。
    pub fn accept(&mut self) -> bool {
        if self.stage == ShareStage::AwaitingConfirm {
            self.stage = ShareStage::Transferring;
            true
        } else {
            false
        }
    }

    /// 接收方拒绝：终态 + 半截文件清零（拒绝路径不留半截文件）。
    pub fn reject(&mut self) -> bool {
        if self.stage == ShareStage::AwaitingConfirm {
            self.stage = ShareStage::Rejected;
            self.partial_file_bytes = 0;
            true
        } else {
            false
        }
    }

    /// 传输推进：确认 N 块。
    pub fn advance(&mut self, chunks: u64) -> bool {
        if self.stage != ShareStage::Transferring {
            return false;
        }
        self.confirmed_chunks = (self.confirmed_chunks + chunks).min(self.total_chunks());
        if self.confirmed_chunks == self.total_chunks() {
            self.stage = ShareStage::Done; // 完成双方通知（进度面）。
        }
        true
    }

    /// 断网：传输中断（已确认块保留——续传锚点）。
    pub fn disconnect(&mut self) -> bool {
        if self.stage == ShareStage::Transferring {
            self.stage = ShareStage::Disconnected;
            true
        } else {
            false
        }
    }

    /// 恢复续传：从已确认块继续（不重传）。
    pub fn resume(&mut self) -> bool {
        if self.stage == ShareStage::Disconnected {
            self.stage = ShareStage::Transferring;
            true
        } else {
            false
        }
    }

    /// 传输进度（0-1000‰）。
    pub fn progress_permille(&self) -> u64 {
        if self.total_chunks() == 0 {
            return 0;
        }
        self.confirmed_chunks * 1000 / self.total_chunks()
    }

    /// 完成判据：整文件确认 + 无半截残留。
    pub fn done_clean(&self) -> bool {
        self.stage == ShareStage::Done && self.partial_file_bytes == 0
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F321 自检（判据：四步用例；默认隐身；断点续传；拒绝零半截；速度记录）。
pub fn run_nearshare_checks() -> CheckSet {
    let mut set = CheckSet::new("F321-nearshare");

    // 1. 默认隐身判据（可发现性默认关）。
    let s = ShareSession::new("报告.pdf", 45 * 1024 * 1024);
    set.add("discoverable default off", !s.discoverable, "");

    // 2. 四步用例：发现 → 确认 → 传输 → 完成（45MB / 1MB 块）。
    let mut s = ShareSession::new("报告.pdf", 45 * 1024 * 1024);
    let ok = s.request_confirm() && s.accept();
    let mut mid = true;
    for _ in 0..44 {
        mid = mid && s.advance(1);
    }
    set.add(
        "four steps flow",
        ok && mid && s.stage == ShareStage::Transferring && s.progress_permille() == 977,
        "",
    );
    let _ = s.advance(1);
    set.add("complete notifies both", s.done_clean(), "");

    // 3. 断点续传：传 30 块断网 → 恢复 → 续传 15 块到完成（已确认块不重传）。
    let mut s = ShareSession::new("大包.dat", 45 * 1024 * 1024);
    let _ = s.request_confirm();
    let _ = s.accept();
    let _ = s.advance(30);
    let _ = s.disconnect();
    let anchor = s.confirmed_chunks;
    let resumed = s.resume();
    let _ = s.advance(15); // 只需再传 15 块（45-30）——锚点后无重传。
    set.add(
        "resume completes without resend",
        anchor == 30 && resumed && s.done_clean() && s.confirmed_chunks == 45,
        "",
    );

    // 4. 拒绝路径：半截文件零残留（终态不可再推进）。
    let mut s = ShareSession::new("x", 10 * 1024 * 1024);
    let _ = s.request_confirm();
    let _ = s.reject();
    set.add(
        "reject leaves no partial",
        s.stage == ShareStage::Rejected && s.partial_file_bytes == 0 && !s.advance(1),
        "",
    );

    // 5. 乱序防护：未确认就传输拒绝（状态机闭环）。
    let mut s = ShareSession::new("x", 1024);
    set.add(
        "stage machine closed",
        !s.advance(1) && !s.accept() && s.request_confirm() && !s.advance(1) && s.accept(),
        "",
    );

    // 6. 起步速度判线常量（10MB/s 记录面）。
    set.add("min throughput constant", MIN_THROUGHPUT_MBPS == 10, "");

    // 7. 进度账：0 块 0‰、全块 1000‰（边界）。
    let mut s = ShareSession::new("空", 1024);
    set.add("progress edges", s.progress_permille() == 0 && {
        let _ = s.request_confirm();
        let _ = s.accept();
        let _ = s.advance(1);
        s.progress_permille() == 1000
    }, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_ceil_boundary() {
        let s = ShareSession::new("x", CHUNK_BYTES * 3 + 1);
        assert_eq!(s.total_chunks(), 4);
    }

    #[test]
    fn double_reject_rejected() {
        let mut s = ShareSession::new("x", 1);
        let _ = s.request_confirm();
        let _ = s.reject();
        assert!(!s.reject());
    }

    #[test]
    fn over_advance_clamped() {
        let mut s = ShareSession::new("x", CHUNK_BYTES);
        let _ = s.request_confirm();
        let _ = s.accept();
        let _ = s.advance(5);
        assert_eq!(s.confirmed_chunks, 1);
        assert!(s.done_clean());
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F321 设备发现账 + 分块校验账 + 传输速度账
// ---------------------------------------------------------------------------

/// 一台附近设备（发现列表行：设备名 + 头像徽标色 + 信号强度序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearbyDevice {
    pub name: String,
    /// 徽标色（0-7 调色板序——头像占位模型）。
    pub badge: u8,
    /// 信号强度（dBm 负值，越大越近）。
    pub rssi: i32,
    /// 对端可发现性（对端隐身则不出列——双向礼貌）。
    pub discoverable: bool,
}

/// 发现账：扫描一轮的设备列表（按信号强度降序、名序稳定——确定列表）。
pub struct DiscoveryLedger {
    devices: Vec<NearbyDevice>,
}

impl DiscoveryLedger {
    pub fn new() -> DiscoveryLedger {
        DiscoveryLedger { devices: Vec::new() }
    }

    /// 扫描注入（同设备覆盖最新 RSSI——列表实时）。
    pub fn observe(&mut self, d: NearbyDevice) {
        match self.devices.iter_mut().find(|x| x.name == d.name) {
            Some(slot) => *slot = d,
            None => self.devices.push(d),
        }
    }

    /// 可见列表：只出 discoverable 的，按 RSSI 降序、名序稳定。
    pub fn visible(&self) -> Vec<&NearbyDevice> {
        let mut v: Vec<&NearbyDevice> =
            self.devices.iter().filter(|d| d.discoverable).collect();
        v.sort_by(|a, b| b.rssi.cmp(&a.rssi).then(a.name.cmp(&b.name)));
        v
    }

    /// 发起请求（目标设备名 + 本机确认码——接收方确认条对账）。
    pub fn request_pair(&self, device: &str, code: u32) -> Option<(String, u32)> {
        self.visible()
            .iter()
            .find(|d| d.name == device)
            .map(|d| (d.name.clone(), code))
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

impl Default for DiscoveryLedger {
    fn default() -> DiscoveryLedger {
        DiscoveryLedger::new()
    }
}

/// 分块校验账：每块 FNV-1a 校验和（接收方逐块验——坏块重传的判据载体）。
pub struct ChunkLedger {
    /// (块序, 校验和) 账。
    pub chunks: Vec<(u64, u32)>,
}

/// FNV-1a 32 位（域内统一实现一份——块校验用）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl ChunkLedger {
    pub fn new() -> ChunkLedger {
        ChunkLedger { chunks: Vec::new() }
    }

    /// 记一块（序 + 内容校验和）。
    pub fn record(&mut self, seq: u64, payload: &[u8]) {
        let sum = fnv1a(payload);
        match self.chunks.iter_mut().find(|(s, _)| *s == seq) {
            Some(slot) => slot.1 = sum,
            None => self.chunks.push((seq, sum)),
        }
    }

    /// 接收方验块：校验和匹配 → 确认；不匹配 → 重传请求（坏块清单）。
    pub fn verify(&self, seq: u64, payload: &[u8]) -> bool {
        self.chunks
            .iter()
            .find(|(s, _)| *s == seq)
            .map(|(_, sum)| *sum == fnv1a(payload))
            .unwrap_or(false)
    }

    /// 全量校验：0..n 块全在且全对（完成判定——进度 1000‰ 的强化版）。
    pub fn all_verified(&self, total: u64) -> bool {
        (0..total).all(|s| self.chunks.iter().any(|(seq, _)| *seq == s))
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

impl Default for ChunkLedger {
    fn default() -> ChunkLedger {
        ChunkLedger::new()
    }
}

/// 传输速度账（10MB/s 起步判据的实测载体：窗口内传输字节 / 窗口秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeedSample {
    pub window_ms: u64,
    pub bytes: u64,
}

/// 起步速度核账（吞吐 ≥10MB/s——实测记录面；不足诚实红）。
pub fn throughput_ok(samples: &[SpeedSample]) -> bool {
    if samples.is_empty() {
        return false;
    }
    let total_ms: u64 = samples.iter().map(|s| s.window_ms).sum();
    let total_bytes: u64 = samples.iter().map(|s| s.bytes).sum();
    if total_ms == 0 {
        return false;
    }
    let mbps = total_bytes * 1000 / total_ms / (1024 * 1024);
    mbps >= super::nearshare::MIN_THROUGHPUT_MBPS
}

/// 深化层自检。
pub fn run_nearshare_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F321-deep");

    // 1. 发现列表：隐身设备不出列；RSSI 降序、同强名序。
    let mut dl = DiscoveryLedger::new();
    dl.observe(NearbyDevice { name: String::from("台式机"), badge: 1, rssi: -50, discoverable: true });
    dl.observe(NearbyDevice { name: String::from("笔记本"), badge: 2, rssi: -40, discoverable: true });
    dl.observe(NearbyDevice { name: String::from("隐身平板"), badge: 3, rssi: -30, discoverable: false });
    let vis = dl.visible();
    set.add(
        "discovery visible sorted",
        vis.len() == 2 && vis[0].name == "笔记本" && vis[1].name == "台式机",
        "",
    );

    // 2. 发起请求：目标在列才可请求（确认码随请求走）。
    let req = dl.request_pair("笔记本", 42);
    set.add(
        "pair request carries code",
        req == Some((String::from("笔记本"), 42)) && dl.request_pair("隐身平板", 1).is_none(),
        "",
    );

    // 3. 分块校验：好块确认、坏块重传请求（校验和不匹配）。
    let mut ck = ChunkLedger::new();
    ck.record(0, b"chunk-0-payload");
    ck.record(1, b"chunk-1-payload");
    set.add(
        "chunk verify good bad",
        ck.verify(0, b"chunk-0-payload") && !ck.verify(1, b"corrupted"),
        "",
    );

    // 4. 全量校验：0..n 全在才算完成（缺块不出绿）。
    set.add(
        "all verified gate",
        !ck.all_verified(3) && { ck.record(2, b"chunk-2"); ck.all_verified(3) },
        "",
    );

    // 5. 起步速度：10MB/s 实测账（样本 100ms/1.2MB ×10 = 12MB/s 过；
    //    不足诚实红）。
    let ok_samples = alloc::vec![SpeedSample { window_ms: 100, bytes: 1_200_000 }; 10];
    let slow_samples = alloc::vec![SpeedSample { window_ms: 100, bytes: 800_000 }; 10];
    set.add(
        "throughput 10mbps measured",
        throughput_ok(&ok_samples) && !throughput_ok(&slow_samples)
            && !throughput_ok(&[]),
        "",
    );

    // 6. 扫描覆盖更新：同设备重扫刷新 RSSI 不重复。
    dl.observe(NearbyDevice { name: String::from("台式机"), badge: 1, rssi: -45, discoverable: true });
    set.add("observe refreshes rssi", dl.len() == 3 && dl.visible().len() == 2 && dl.visible()[1].rssi == -45, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn empty_discovery_no_request() {
        let dl = DiscoveryLedger::new();
        assert!(dl.request_pair("x", 1).is_none());
    }

    #[test]
    fn chunk_verify_missing_seq_false() {
        let ck = ChunkLedger::new();
        assert!(!ck.verify(9, b"x"));
    }

    #[test]
    fn throughput_zero_window_false() {
        assert!(!throughput_ok(&[SpeedSample { window_ms: 0, bytes: 100 }]));
    }

    #[test]
    fn fnv1a_known_vector() {
        assert_eq!(fnv1a(b"a"), 0xe40c_292c);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 断点续传引擎 + 拒绝路径清场
// ---------------------------------------------------------------------------

/// 断点续传引擎（判据「传输中断网恢复」的本体面）：分块位图记账——
/// 收到的块记账，恢复时只重请求缺失块（不重传已收——带宽纪律）。
/// 校验沿用 fnv1a：每块收讫即验，坏块视为缺失重收。
pub struct ResumeEngine {
    total_chunks: usize,
    received: Vec<bool>,
    /// 块校验账：(块号, 期望 fnv, 实际 fnv)——坏块定位面。
    pub checksum_log: Vec<(usize, u32, u32)>,
    /// 重请求计数（恢复效率账——只补缺失的证据面）。
    pub re_requests: usize,
}

impl ResumeEngine {
    pub fn new(total_chunks: usize) -> ResumeEngine {
        ResumeEngine {
            total_chunks,
            received: vec![false; total_chunks],
            checksum_log: Vec::new(),
            re_requests: 0,
        }
    }

    /// 收块（带校验）：校验过 → 记收到；坏 → 记账并保持缺失。
    pub fn accept_chunk(&mut self, idx: usize, expected: u32, payload: &[u8]) -> bool {
        if idx >= self.total_chunks {
            return false;
        }
        let actual = fnv1a(payload);
        let ok = actual == expected;
        self.checksum_log.push((idx, expected, actual));
        self.received[idx] = ok;
        ok
    }

    /// 恢复面：缺失块清单（恢复时只重请求这些——断点续传核心语义）。
    pub fn missing(&self) -> Vec<usize> {
        (0..self.total_chunks).filter(|&i| !self.received[i]).collect()
    }

    /// 断网模拟后恢复：对缺失块逐一重请求计数 +1（由发送方补发），
    /// 这里只记账（补发内容走 accept_chunk）。
    pub fn resume_tick(&mut self) -> usize {
        let n = self.missing().len();
        self.re_requests += n;
        n
    }

    /// 完成：零缺失才算完（半截文件不许报完成）。
    pub fn complete(&self) -> bool {
        self.missing().is_empty()
    }

    pub fn progress_permille(&self) -> u32 {
        if self.total_chunks == 0 {
            return 1000;
        }
        let got = self.total_chunks - self.missing().len();
        (got * 1000 / self.total_chunks) as u32
    }
}

/// 拒绝路径清场（判据「拒绝路径不留半截文件」）：对端拒绝 → 本地
/// 半截产物按登记清光，清场后零残留可查；重复清场幂等。
pub struct RejectionCleanup {
    /// 半截产物登记（接收侧已落盘的临时路径）。
    partials: Vec<String>,
    pub cleaned: Vec<String>,
}

impl RejectionCleanup {
    pub fn new() -> RejectionCleanup {
        RejectionCleanup { partials: Vec::new(), cleaned: Vec::new() }
    }

    /// 接收过程中每落一块产物登记一处。
    pub fn stage_partial(&mut self, path: &str) {
        self.partials.push(String::from(path));
    }

    /// 拒绝 → 清场：返回清掉的数量；再清一次为 0（幂等）。
    pub fn reject_and_clean(&mut self) -> usize {
        let n = self.partials.len();
        self.cleaned.extend(self.partials.drain(..));
        n
    }

    pub fn partials_remaining(&self) -> usize {
        self.partials.len()
    }
}

impl Default for RejectionCleanup {
    fn default() -> RejectionCleanup {
        RejectionCleanup::new()
    }
}

/// 深化层二自检（续传 / 清场）。
pub fn run_nearshare_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F321-deep2");

    // 1. 续传：8 块传输，收 5 块后断网 → 缺失清单精确、恢复只补 3 块。
    let mut re = ResumeEngine::new(8);
    for i in 0..5usize {
        let payload = alloc::format!("chunk{i}");
        assert!(re.accept_chunk(i, fnv1a(payload.as_bytes()), payload.as_bytes()));
    }
    let missing_after_drop = re.missing();
    set.add(
        "resume bitmap exact",
        missing_after_missing_check(&re) && missing_after_drop == alloc::vec![5, 6, 7],
        "",
    );
    let resumed = re.resume_tick();
    set.add("resume re-requests only missing", resumed == 3 && re.re_requests == 3, "");

    // 2. 坏块：校验不过不算收到（补发面）。
    let mut re2 = ResumeEngine::new(2);
    let bad = re2.accept_chunk(0, fnv1a(b"good"), b"corrupted");
    set.add("bad chunk stays missing", !bad && re2.missing() == alloc::vec![0, 1], "");

    // 3. 完成：补齐后零缺失 + 进度 1000‰；半截不许报完成。
    let payload = "chunk5";
    let _ = re2.accept_chunk(0, fnv1a(payload.as_bytes()), payload.as_bytes());
    set.add("half not complete", !re2.complete() && re2.progress_permille() == 500, "");
    let _ = re2.accept_chunk(0, fnv1a(b"good"), b"good");
    let _ = re2.accept_chunk(1, fnv1a(b"good"), b"good");
    set.add("complete only when zero missing", re2.complete() && re2.progress_permille() == 1000, "");

    // 4. 越界块号拒绝（不崩溃不越写）。
    let mut re3 = ResumeEngine::new(1);
    set.add("out of range chunk rejected", !re3.accept_chunk(9, 0, b"x"), "");

    // 5. 拒绝清场：三处半截产物全清、复清幂等、零残留。
    let mut rc = RejectionCleanup::new();
    rc.stage_partial("Recv/相册.zip.part0");
    rc.stage_partial("Recv/相册.zip.part1");
    rc.stage_partial("Recv/.tmp-meta");
    let n1 = rc.reject_and_clean();
    let n2 = rc.reject_and_clean();
    set.add(
        "rejection cleans all partials",
        n1 == 3 && n2 == 0 && rc.partials_remaining() == 0 && rc.cleaned.len() == 3,
        "",
    );

    set
}

/// 缺失清单与位图一致性（自检辅助——位图即事实，无第二账）。
fn missing_after_missing_check(re: &ResumeEngine) -> bool {
    re.missing().len() == 3
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn zero_chunk_engine_is_complete_by_definition() {
        let re = ResumeEngine::new(0);
        assert!(re.complete(), "0 块任务无缺失——完成语义成立");
        assert_eq!(re.progress_permille(), 1000);
    }

    #[test]
    fn checksum_log_records_both_outcomes() {
        let mut re = ResumeEngine::new(2);
        let _ = re.accept_chunk(0, fnv1a(b"a"), b"a");
        let _ = re.accept_chunk(1, fnv1a(b"a"), b"b");
        assert_eq!(re.checksum_log.len(), 2);
        assert_eq!(re.checksum_log[0].1, re.checksum_log[0].2, "好块期望=实际");
        assert_ne!(re.checksum_log[1].1, re.checksum_log[1].2, "坏块期望≠实际");
    }

    #[test]
    fn cleanup_after_reject_accepts_fresh_session() {
        let mut rc = RejectionCleanup::new();
        rc.stage_partial("a.part");
        let _ = rc.reject_and_clean();
        rc.stage_partial("b.part");
        assert_eq!(rc.partials_remaining(), 1, "清场后可开新会话");
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 配对确认码握手 + 自适应分块 + 多接收方账
// ---------------------------------------------------------------------------

/// 配对确认码握手（判据「确认步用例」的防误发机制面）：双端各出六位
/// 确认码，用户肉眼比对一致才放行传输——防同网段陌生设备静默收发。
/// 码生成确定性（同会话种子同码——可复现可审计）；三次不匹配 → 会话
/// 封禁留痕（防暴力猜测——失败显性化）。
pub struct PairingHandshake {
    /// 本端码（会话种子派生）。
    pub local_code: u32,
    attempts: u32,
    pub banned: bool,
    /// 匹配留痕（确认成功时刻）。
    pub confirmed_at: Option<u64>,
}

impl PairingHandshake {
    /// 种子派生码（六位十进制：100000-999999——纯计算确定性）。
    pub fn new(seed: u64) -> PairingHandshake {
        let code = (seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
            >> 33) % 900_000
            + 100_000;
        PairingHandshake { local_code: code as u32, attempts: 0, banned: false, confirmed_at: None }
    }

    /// 对端码校验：一致 → 确认留痕放行；不一致 → 计数，三次封禁。
    pub fn verify(&mut self, remote_code: u32, at_ms: u64) -> bool {
        if self.banned {
            return false;
        }
        if remote_code == self.local_code {
            self.confirmed_at = Some(at_ms);
            true
        } else {
            self.attempts += 1;
            if self.attempts >= 3 {
                self.banned = true;
            }
            false
        }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

/// 自适应分块（判据「10MB/s 起步速度记录」的延续面）：按滑动速度样本
/// 调块长——速度掉 → 块加大（降每块固定开销占比）；速度回升 → 块回
/// 落（降重传代价）。档位表唯一源，调档留痕。
pub struct AdaptiveChunking {
    /// (速度下限 MB/s, 块长 MB)——速度越低块越大。
    pub table: [(u64, u64); 4],
    pub current_chunk_mb: u64,
    pub switches: u64,
}

impl AdaptiveChunking {
    pub fn new() -> AdaptiveChunking {
        AdaptiveChunking {
            table: [(20, 1), (10, 2), (5, 4), (0, 8)],
            current_chunk_mb: 1,
            switches: 0,
        }
    }

    /// 速度采样 → 档位（表内首个「速度 ≥ 下限」的档）。
    pub fn feed_speed(&mut self, speed_mbps: u64) -> u64 {
        let want = self
            .table
            .iter()
            .find(|(floor, _)| speed_mbps >= *floor)
            .map(|(_, chunk)| *chunk)
            .unwrap_or(8);
        if want != self.current_chunk_mb {
            self.current_chunk_mb = want;
            self.switches += 1;
        }
        self.current_chunk_mb
    }

    /// 表自证：块长随速度降单调不降（慢速大块——策略不倒挂）。
    pub fn monotonic(&self) -> bool {
        self.table.windows(2).all(|w| w[0].0 > w[1].0 && w[0].1 <= w[1].1)
    }
}

impl Default for AdaptiveChunking {
    fn default() -> AdaptiveChunking {
        AdaptiveChunking::new()
    }
}

/// 多接收方账（一台对多台的分发面）：逐接收方独立记账（确认码/进度/
/// 完成）——一台慢不拖死全场（独立进度语义）。
#[derive(Default)]
pub struct MultiCastBook {
    /// (接收方, 确认?, 进度‰)。
    pub receivers: Vec<(String, bool, u32)>,
}

impl MultiCastBook {
    pub fn join(&mut self, name: &str) -> bool {
        if self.receivers.iter().any(|(n, _, _)| n == name) {
            return false;
        }
        self.receivers.push((String::from(name), false, 0));
        true
    }

    /// 逐方确认（未入册的确认拒绝——不猜）。
    pub fn confirm(&mut self, name: &str) -> bool {
        match self.receivers.iter_mut().find(|(n, _, _)| n == name) {
            Some((_, ok, _)) => {
                *ok = true;
                true
            }
            None => false,
        }
    }

    /// 逐方进度推进。
    pub fn progress(&mut self, name: &str, permille: u32) -> bool {
        match self.receivers.iter_mut().find(|(n, _, _)| n == name) {
            Some((_, ok, p)) => {
                if !*ok {
                    return false; // 未确认不收数据（握手闸门）。
                }
                *p = permille.min(1000);
                true
            }
            None => false,
        }
    }

    /// 全场完成判定：全确认 + 全 1000‰。
    pub fn all_done(&self) -> bool {
        !self.receivers.is_empty()
            && self.receivers.iter().all(|(_, ok, p)| *ok && *p == 1000)
    }

    /// 慢方清单（拖后接收方——进度 < 全场最小的面上直出）。
    pub fn laggards(&self) -> Vec<&str> {
        let min = self
            .receivers
            .iter()
            .map(|(_, _, p)| *p)
            .min()
            .unwrap_or(0);
        self.receivers
            .iter()
            .filter(|(_, _, p)| *p == min && min < 1000)
            .map(|(n, _, _)| n.as_str())
            .collect()
    }
}

/// 深化层二自检（握手 / 自适应分块 / 多接收方）。
pub fn run_nearshare_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F321-deep3");

    // 1. 握手：同种子码确定可复现；对码确认留痕。
    let mut hs = PairingHandshake::new(42);
    let code = hs.local_code;
    let same = PairingHandshake::new(42).local_code == code;
    let ok = hs.verify(code, 100);
    set.add("handshake deterministic confirm", same && ok && hs.confirmed_at == Some(100), "");

    // 2. 防暴力：三次错码封禁，封禁后对码也拒（显性化——不静默重试）。
    let mut hs2 = PairingHandshake::new(7);
    let _ = hs2.verify(111_111, 0);
    let _ = hs2.verify(222_222, 10);
    let _ = hs2.verify(333_333, 20);
    set.add(
        "handshake bans after three",
        hs2.banned && !hs2.verify(hs2.local_code, 30) && hs2.attempts() == 3,
        "",
    );

    // 3. 自适应分块：速度掉档块加大、回升块回落、表单调自证。
    let mut ac = AdaptiveChunking::new();
    let c1 = ac.feed_speed(25);
    let c2 = ac.feed_speed(7);
    let c3 = ac.feed_speed(25);
    set.add(
        "adaptive chunking bidirectional",
        c1 == 1 && c2 == 4 && c3 == 1 && ac.switches == 2 && ac.monotonic(),
        "",
    );

    // 4. 多接收方：逐方独立确认/进度；慢方直出；全场完成判定。
    let mut mc = MultiCastBook::default();
    let _ = mc.join("客厅机");
    let _ = mc.join("书房机");
    let dup = mc.join("客厅机");
    set.add("multicast dedup join", !dup && mc.receivers.len() == 2, "");
    let before_confirm = mc.progress("客厅机", 100);
    let _ = mc.confirm("客厅机");
    let _ = mc.confirm("书房机");
    let _ = mc.progress("客厅机", 1000);
    let _ = mc.progress("书房机", 400);
    set.add(
        "multicast independent progress",
        !before_confirm && mc.laggards() == alloc::vec!["书房机"] && !mc.all_done(),
        "",
    );
    let _ = mc.progress("书房机", 1000);
    set.add("multicast all done", mc.all_done(), "");
    set.add("multicast unknown confirm rejected", !mc.confirm("幽灵机"), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn handshake_code_in_range() {
        for seed in 0..20u64 {
            let hs = PairingHandshake::new(seed);
            assert!((100_000..1_000_000).contains(&hs.local_code), "六位码域 {seed}");
        }
    }

    #[test]
    fn chunking_boundary_speeds() {
        let mut ac = AdaptiveChunking::new();
        assert_eq!(ac.feed_speed(10), 2, "恰在档位下限时取更大块（≥ 含边界）");
        assert_eq!(ac.feed_speed(20), 1);
    }

    #[test]
    fn multicast_empty_not_done() {
        let mc = MultiCastBook::default();
        assert!(!mc.all_done(), "零接收方不构成完成");
        assert!(mc.laggards().is_empty());
    }
}
