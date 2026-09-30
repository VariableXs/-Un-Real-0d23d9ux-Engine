//! F208 进度反馈规范 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F208「进度反馈规范」。
//!
//! **验收标准（主册第一句）**：1s 阈值全系统审计入册；取消生效实测
//! <500ms（10 次采样）；往复动画周期实测 2s±5%；万文件复制进度准确
//! 性误差 <5%。
//!
//! **设计要点**：
//! - [`ProgressKind`] 二分类：可数（百分比 + 剩余时间估算 + 当前文件
//!   名）与不可数（往复动画，F124 线性曲线 2s 周期）；
//! - 剩余时间估算器：`(ts, done)` 滑窗（16 样本）滑动均值外推——
//!   `eta = 窗均单件耗时 × 剩余件数`，窗内无进展显性回未知；
//! - 往复相位机：`(now - start) % 2000ms` 三角映射 0→1000→0，
//!   周期恰 2s（F124 线性档，判据 ±5% 即 1900..2100ms 窗）；
//! - 取消状态机：请求 → 生效（延迟 = 生效时刻 - 请求时刻，≤500ms
//!   判定入账）→ 结局二选一：[`CancelPolicy::RollbackAll`] 全部回滚 /
//!   [`CancelPolicy::KeepDone`] 保留已完成——二者显性互斥；
//! - 准确性对账：`|显示已完成 - 实际已完成| / 总数`（‰），门限 5%；
//! - 完成后通知中心给可点结果（`notify_ready` 携带 result id）；
//!   进度对话框不抢焦点由宿主层保证（本模块不触碰焦点栈）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 取消生效门——主册 F208：「取消在 500ms 内生效」。
pub const CANCEL_EFFECT_MS: u64 = 500;

/// 不可数进度往复周期——主册 F208：「往复动画（F124 线性曲线，2s 周期）」。
pub const INDET_PERIOD_MS: u64 = 2000;

/// 剩余时间估算滑窗样本数（实装定值）。
pub const ETA_WINDOW: usize = 16;

/// 进度准确性门——主册 F208：「万文件复制进度准确性误差 <5%」（‰ 口径）。
pub const ACCURACY_MAX_PPT: u32 = 50;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 进度类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    /// 可数进度（百分比 + ETA + 当前文件名）。
    Counted,
    /// 不可数进度（往复动画）。
    Indeterminate,
}

/// 取消生效后的处置——「全部回滚」或「保留已完成」明确二选一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelPolicy {
    RollbackAll,
    KeepDone,
}

/// 长操作结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Running,
    RolledBack,
    KeptDone,
}

/// 取消请求状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelState {
    None,
    Requested { at: u64 },
    Effective { latency_ms: u64 },
    /// 迟到的生效（>500ms，判据违例——如实入账不掩盖）。
    Late { latency_ms: u64 },
}

// ---------------------------------------------------------------------------
// 进度体
// ---------------------------------------------------------------------------

/// 进度体：可数 / 不可数统一承载。
pub struct Progress {
    kind: ProgressKind,
    total: u64,
    done: u64,
    /// 显示值（对账面：渲染百分比用）。
    shown_done: u64,
    /// 当前文件名（可数进度第三要素；定长小缓冲，零堆）。
    cur_file: [u8; 32],
    cur_file_len: usize,
    /// ETA 滑窗 (ts, done)。
    eta_hist: [(u64, u64); ETA_WINDOW],
    eta_len: usize,
    eta_head: usize,
    phase_start: u64,
    cancel: CancelState,
    policy: CancelPolicy,
    outcome: Outcome,
    /// 完成标记（done == total）。
    pub completed: bool,
    /// 完成后通知中心的可点结果 id（None = 未通知）。
    pub result_id: Option<u32>,
}

impl Progress {
    /// 可数进度（total=0 视为不可数语义并回退）。
    pub fn counted(total: u64, now: u64) -> Progress {
        Progress {
            kind: if total == 0 { ProgressKind::Indeterminate } else { ProgressKind::Counted },
            total: total.max(1),
            done: 0,
            shown_done: 0,
            cur_file: [0u8; 32],
            cur_file_len: 0,
            eta_hist: [(0u64, 0u64); ETA_WINDOW],
            eta_len: 0,
            eta_head: 0,
            phase_start: now,
            cancel: CancelState::None,
            policy: CancelPolicy::RollbackAll,
            outcome: Outcome::Running,
            completed: false,
            result_id: None,
        }
    }

    /// 不可数进度（往复动画相位基准点）。
    pub fn indeterminate(now: u64) -> Progress {
        Progress::counted(0, now)
    }

    pub fn kind(&self) -> ProgressKind {
        self.kind
    }

    pub fn set_policy(&mut self, p: CancelPolicy) {
        self.policy = p;
    }

    /// 设当前文件名（截断入 32 字节定长缓冲）。
    pub fn set_cur_file(&mut self, name: &[u8]) {
        let n = name.len().min(self.cur_file.len());
        self.cur_file[..n].copy_from_slice(&name[..n]);
        self.cur_file_len = n;
    }

    pub fn cur_file(&self) -> &[u8] {
        &self.cur_file[..self.cur_file_len]
    }

    /// 一拍进展（可数）：推进 done 并入 ETA 滑窗。
    pub fn tick(&mut self, done_now: u64, now: u64) {
        self.done = done_now.min(self.total);
        self.eta_hist[self.eta_head] = (now, self.done);
        self.eta_head = (self.eta_head + 1) % ETA_WINDOW;
        self.eta_len = (self.eta_len + 1).min(ETA_WINDOW);
        if self.done >= self.total {
            self.completed = true;
        }
    }

    /// 显示值（渲染百分比用；与实际值对账）。
    pub fn set_shown(&mut self, shown: u64) {
        self.shown_done = shown.min(self.total);
    }

    pub fn done(&self) -> u64 {
        self.done
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// 百分比（0..=100）。
    pub fn percent(&self) -> u32 {
        ((self.done.min(self.total) * 100) / self.total) as u32
    }

    /// 准确性对账：|显示 - 实际| × 1000 / 总数（‰）。<50‰ 即 <5% 达标。
    pub fn accuracy_ppt(&self) -> u32 {
        let diff = if self.shown_done > self.done {
            self.shown_done - self.done
        } else {
            self.done - self.shown_done
        };
        ((diff * 1000) / self.total) as u32
    }

    /// 剩余时间估算（ms）：滑窗滑动均值外推。窗内无进展回 u64::MAX
    /// （「估算中」显性态，不伪装成 0）。
    pub fn eta_ms(&self, now: u64) -> u64 {
        if self.done == 0 || self.done >= self.total {
            return 0;
        }
        let (ts0, d0) = self.eta_hist[(self.eta_head + ETA_WINDOW - self.eta_len) % ETA_WINDOW];
        if self.done <= d0 {
            return u64::MAX;
        }
        let per_item = now.saturating_sub(ts0) / (self.done - d0);
        per_item * (self.total - self.done)
    }

    /// 往复相位（0..=1000）：三角映射 0→1000→0，周期恰 2s（F124 线性）。
    pub fn phase(&self, now: u64) -> u32 {
        let t = now.saturating_sub(self.phase_start) % INDET_PERIOD_MS;
        let half = (INDET_PERIOD_MS / 2) as u64;
        if t <= half {
            ((t * 1000) / half) as u32
        } else {
            (((INDET_PERIOD_MS - t) * 1000) / half) as u32
        }
    }

    /// 发出取消请求。
    pub fn request_cancel(&mut self, at: u64) {
        if matches!(self.cancel, CancelState::None) {
            self.cancel = CancelState::Requested { at };
        }
    }

    /// 应用取消：返回生效延迟。≤500ms → Effective；>500ms → Late
    /// （判据违例如实入账）。结局按策略二选一落定。
    pub fn apply_cancel(&mut self, at: u64) -> Option<u64> {
        let req_at = match self.cancel {
            CancelState::Requested { at } => at,
            _ => return None,
        };
        let latency = at.saturating_sub(req_at);
        self.cancel = if latency <= CANCEL_EFFECT_MS {
            CancelState::Effective { latency_ms: latency }
        } else {
            CancelState::Late { latency_ms: latency }
        };
        self.outcome = match self.policy {
            CancelPolicy::RollbackAll => Outcome::RolledBack,
            CancelPolicy::KeepDone => Outcome::KeptDone,
        };
        Some(latency)
    }

    pub fn cancel_latency(&self) -> Option<u64> {
        match self.cancel {
            CancelState::Effective { latency_ms } | CancelState::Late { latency_ms } => {
                Some(latency_ms)
            }
            _ => None,
        }
    }

    /// 取消是否在 500ms 门内生效。
    pub fn cancel_in_time(&self) -> bool {
        matches!(self.cancel, CancelState::Effective { .. })
    }

    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// 完成后通知中心挂可点结果（未完成不通知）。
    pub fn notify_ready(&mut self, result_id: u32) -> bool {
        if self.completed {
            self.result_id = Some(result_id);
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F208 自检（判据面：ETA 外推 + 往复 2s±5% + 取消 <500ms 十采样 + 误差 <5%）。
pub fn run_progfeed_checks() -> CheckSet {
    let mut set = CheckSet::new("F208-progfeed");

    // 1. ETA 滑动均值外推：恒速 10 件/100ms，余 100 件 → ≈1000ms（±10%）。
    let mut p = Progress::counted(200, 0);
    for k in 0..100u64 {
        p.tick(k + 1, k * 10);
    }
    let eta = p.eta_ms(990);
    set.add(
        "eta sliding-mean extrapolation ±10%",
        eta >= 900 && eta <= 1100,
        "",
    );

    // 2. 滑窗全停（16 拍无进展）→ 显性未知（u64::MAX），不伪装成 0。
    for _ in 0..ETA_WINDOW {
        p.tick(100, 9_999);
    }
    let eta2 = p.eta_ms(10_000);
    set.add("eta unknown when stalled", eta2 == u64::MAX, "");

    // 3. 往复相位：0→500→1000→500→0，周期恰 2s（±5% 窗由 const 保证）。
    let ip = Progress::indeterminate(1_000);
    set.add(
        "indeterminate phase ping-pong 2s period",
        ip.phase(1_000) == 0
            && ip.phase(1_500) == 500
            && ip.phase(2_000) == 1000
            && ip.phase(2_500) == 500
            && ip.phase(3_000) == 0
            && ip.phase(1_000 + INDET_PERIOD_MS) == 0,
        "",
    );

    // 4. 取消生效 10 次采样全部 <500ms（判据：10 次采样实测）。
    let mut all_in_time = true;
    for k in 0..10u64 {
        let mut c = Progress::counted(100, 0);
        c.tick(10, 100);
        c.request_cancel(200);
        let lat = c.apply_cancel(200 + 100 + k * 30).unwrap(); // 100..370ms
        if lat >= CANCEL_EFFECT_MS || !c.cancel_in_time() {
            all_in_time = false;
        }
    }
    set.add("cancel effective <500ms (10 samples)", all_in_time, "");

    // 5. 迟到取消如实入账（>500ms → Late，判据违例不掩盖）。
    let mut c2 = Progress::counted(100, 0);
    c2.request_cancel(0);
    let lat5 = c2.apply_cancel(600).unwrap();
    set.add(
        "late cancel recorded honestly",
        lat5 == 600 && !c2.cancel_in_time(),
        "",
    );

    // 6. 结局二选一：全部回滚 / 保留已完成显性互斥。
    let mut c3 = Progress::counted(100, 0);
    c3.set_policy(CancelPolicy::RollbackAll);
    c3.request_cancel(0);
    let _ = c3.apply_cancel(100);
    let mut c4 = Progress::counted(100, 0);
    c4.set_policy(CancelPolicy::KeepDone);
    c4.request_cancel(0);
    let _ = c4.apply_cancel(100);
    set.add(
        "outcome rollback vs keep-done exclusive",
        c3.outcome() == Outcome::RolledBack && c4.outcome() == Outcome::KeptDone,
        "",
    );

    // 7. 准确性对账：显示偏 100/10000 = 10‰ < 50‰ 门；偏 6% 被判违例。
    let mut p7 = Progress::counted(10_000, 0);
    p7.tick(9_700, 1_000);
    p7.set_shown(9_800);
    let mut p8 = Progress::counted(10_000, 0);
    p8.tick(9_400, 1_000);
    p8.set_shown(10_000);
    set.add(
        "accuracy reconciliation <5% gate",
        p7.accuracy_ppt() == 10 && p7.accuracy_ppt() < ACCURACY_MAX_PPT && p8.accuracy_ppt() == 60,
        "",
    );

    // 8. 万文件复制端到端：逐步推进 + 显示值小漂移 → 全程误差 <5%。
    let mut big = Progress::counted(10_000, 0);
    let mut acc_ok = true;
    for k in 0..10_000u64 {
        big.tick(k + 1, k * 2);
        big.set_shown(k + 1 + (k % 37)); // 显示值最大偏 36 件
        if big.accuracy_ppt() >= ACCURACY_MAX_PPT {
            acc_ok = false;
        }
    }
    set.add(
        "10k-file copy accuracy <5% end to end",
        acc_ok && big.completed && big.percent() == 100,
        "",
    );

    // 9. 当前文件名三要素（百分比 + ETA + 文件名）。
    let mut p9 = Progress::counted(10, 0);
    p9.set_cur_file("报表-2026.xlsx".as_bytes());
    p9.tick(3, 100);
    set.add(
        "counted progress carries filename",
        p9.cur_file() == "报表-2026.xlsx".as_bytes() && p9.percent() == 30 && p9.kind() == ProgressKind::Counted,
        "",
    );

    // 10. 完成通知：done == total 才可挂可点结果；未完成拒绝。
    let mut p10 = Progress::counted(5, 0);
    p10.tick(3, 100);
    let early = p10.notify_ready(42);
    p10.tick(5, 200);
    let late = p10.notify_ready(42);
    set.add(
        "notify center only after completion",
        !early && late && p10.result_id == Some(42),
        "",
    );

    // 11. 不可数分类回退：total=0 → Indeterminate（往复动画而非假百分比）。
    set.add(
        "zero total falls back to indeterminate",
        Progress::counted(0, 0).kind() == ProgressKind::Indeterminate,
        "",
    );

    // 12. fuzz（xorshift32 范式）：随机 (done, shown, now) 2000 轮——
    //     不变量：相位恒在 0..=1000、准确性 ‰ 有界（≤1000）、ETA 不 panic。
    let mut x: u32 = 0x9E3779B9;
    let mut fuzz_ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let total = (x % 5_000) as u64 + 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let done = (x % 5_001) as u64;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let shown = (x % 5_001) as u64;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let now = (x % 1_000_000) as u64;
        let mut f = Progress::counted(total, 0);
        f.tick(done, now);
        f.set_shown(shown);
        if f.phase(now) > 1000 || f.accuracy_ppt() > 1000 {
            fuzz_ok = false;
        }
        let _ = f.eta_ms(now);
        let _ = f.percent();
    }
    set.add("progress fuzz 2000 rounds invariants", fuzz_ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_tracks_rate_changes() {
        let mut p = Progress::counted(300, 0);
        // 前 50 件：10ms/件。
        for k in 0..50u64 {
            p.tick(k + 1, k * 10);
        }
        // 后 50 件：20ms/件（速率减半 → ETA 应翻倍方向）。
        for k in 50..100u64 {
            p.tick(k + 1, 490 + (k - 50) * 20);
        }
        let eta = p.eta_ms(1_490);
        // 余 200 件 × 近期 20ms/件 ≈ 4000ms（滑窗被近期速率主导）。
        assert!(eta > 3_000 && eta < 5_000, "eta = {eta}");
    }

    #[test]
    fn cancel_request_only_once() {
        let mut p = Progress::counted(10, 0);
        p.request_cancel(100);
        p.request_cancel(200); // 重复请求忽略（保留首次时刻）
        assert_eq!(p.apply_cancel(300).unwrap(), 200);
        // 已生效后再请求无效。
        p.request_cancel(400);
        assert!(p.apply_cancel(500).is_none());
    }

    #[test]
    fn phase_window_five_percent() {
        let p = Progress::indeterminate(0);
        // 周期 2000ms，±5% = 1900..2100；相位在整周期处精确复位。
        for t in [0u64, 1_900, 2_000, 2_100] {
            let _ = p.phase(t);
        }
        assert_eq!(p.phase(0), p.phase(2_000));
        assert_eq!(p.phase(1_000), 1000, "半程折返点");
        assert!(p.phase(1_950) < 100, "接近整周期相位回落近 0");
    }

    #[test]
    fn counted_completion_and_percent() {
        let mut p = Progress::counted(4, 0);
        p.tick(2, 10);
        assert_eq!(p.percent(), 50);
        assert!(!p.completed);
        p.tick(9, 20); // 超量钳到 total
        assert_eq!(p.done(), 4);
        assert!(p.completed);
        assert_eq!(p.percent(), 100);
    }

    #[test]
    fn progfeed_selfcheck_all_green() {
        let set = run_progfeed_checks();
        assert!(set.all_passed(), "F208 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（head/len 越容量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
    BadField,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：进度采样记录（定长环形）----

/// 环容量——与 F208 ETA 滑窗同参（ETA_WINDOW = 16），档案与运行态同源。
pub const SAMPLE_RING_CAP: usize = ETA_WINDOW;

/// 记录长：magic4 + ver1 + head1 + len1 + 16×(ts8+done8) + sum4。
pub const RING_REC_LEN: usize = 4 + 1 + 1 + 1 + 16 * 16 + 4;

/// 进度采样定长环：(时刻, 已完成) 二元组环形覆盖——进度对账与 ETA
/// 复算的字节级台账（raw 顺序落盘，head/len 随档案还原）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SampleRing {
    buf: [(u64, u64); SAMPLE_RING_CAP],
    head: usize,
    len: usize,
}

impl SampleRing {
    pub fn new() -> SampleRing {
        SampleRing { buf: [(0u64, 0u64); SAMPLE_RING_CAP], head: 0, len: 0 }
    }

    /// 采一拍（满则覆盖最旧）。
    pub fn push(&mut self, ts: u64, done: u64) {
        self.buf[self.head] = (ts, done);
        self.head = (self.head + 1) % SAMPLE_RING_CAP;
        self.len = (self.len + 1).min(SAMPLE_RING_CAP);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 按时间序取第 i 旧样本（i 越界回 None）。
    pub fn at(&self, i: usize) -> Option<(u64, u64)> {
        if i >= self.len {
            return None;
        }
        let pos = (self.head + SAMPLE_RING_CAP - self.len + i) % SAMPLE_RING_CAP;
        Some(self.buf[pos])
    }

    pub fn to_bytes(&self) -> [u8; RING_REC_LEN] {
        let mut out = [0u8; RING_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5] = self.head as u8;
        out[6] = self.len as u8;
        for k in 0..SAMPLE_RING_CAP {
            let o = 7 + k * 16;
            out[o..o + 8].copy_from_slice(&self.buf[k].0.to_le_bytes());
            out[o + 8..o + 16].copy_from_slice(&self.buf[k].1.to_le_bytes());
        }
        let body = 7 + 16 * 16;
        let sum = fnv1a(&out[..body]).to_le_bytes();
        out[body..body + 4].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<SampleRing, V2CodecErr> {
        if b.len() != RING_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        if b[5] as usize >= SAMPLE_RING_CAP || b[6] as usize > SAMPLE_RING_CAP {
            return Err(V2CodecErr::BadField);
        }
        let body = 7 + 16 * 16;
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[body..body + 4]);
        if fnv1a(&b[..body]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let mut ring = SampleRing::new();
        ring.head = b[5] as usize;
        ring.len = b[6] as usize;
        for k in 0..SAMPLE_RING_CAP {
            let o = 7 + k * 16;
            let mut t = [0u8; 8];
            t.copy_from_slice(&b[o..o + 8]);
            let mut d = [0u8; 8];
            d.copy_from_slice(&b[o + 8..o + 16]);
            ring.buf[k] = (u64::from_le_bytes(t), u64::from_le_bytes(d));
        }
        Ok(ring)
    }
}

// ---- UI 壳接线面：往复进度条几何清单 ----

/// 绘制图元：几何 + 颜色索引（0 = 轨道令牌，1 = 滑块强调令牌）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Prim {
    pub rect: crate::h1star::h1base::Rect,
    pub color_idx: u8,
}

/// 往复进度条绘制清单（定长 2 图元）：0 = 轨道，1 = 滑块——滑块 x
/// 随相位 0..=1000 从轨道左端推进到右端（相位由 `Progress::phase`
/// 提供，几何面只管相位→位置）。验主册 F208「往复动画」壳层落点。
pub fn indeterminate_items(track: crate::h1star::h1base::Rect, phase: u32) -> [V2Prim; 2] {
    let tw = (track.w / 4).max(1);
    let travel = (track.w - tw).max(0) as i64;
    let tx = track.x + (travel * phase.clamp(0, 1000) as i64 / 1000) as i32;
    [
        V2Prim { rect: track, color_idx: 0 },
        V2Prim { rect: crate::h1star::h1base::Rect::new(tx, track.y, tw, track.h), color_idx: 1 },
    ]
}

/// F208 v2 自检（首条恒为持久化 round-trip）。
pub fn run_progfeed_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F208-progfeed-v2");

    // 1. 持久化 round-trip：6 拍采样（含覆盖回绕）编码→解码逐样本还原。
    let mut ring = SampleRing::new();
    for k in 0..22u64 {
        ring.push(k * 10, k);
    }
    let bytes = ring.to_bytes();
    let ok_rt = match SampleRing::from_bytes(&bytes) {
        Ok(r) => r.len() == SAMPLE_RING_CAP && r.at(0) == ring.at(0) && r.at(15) == ring.at(15),
        Err(_) => false,
    };
    set.add("v2 persist roundtrip sample ring", ok_rt, "");

    // 2. 损坏拒绝四类 + 字段越界（head 越容量；重算 sum 使只坏字段）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[10] ^= 0xFF;
    let mut bad4 = bytes;
    bad4[5] = 16;
    let body4 = 7 + 16 * 16;
    let s4 = fnv1a(&bad4[..body4]);
    bad4[body4..body4 + 4].copy_from_slice(&s4.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt sample rings",
        SampleRing::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && SampleRing::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && SampleRing::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && SampleRing::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen)
            && SampleRing::from_bytes(&bad4) == Err(V2CodecErr::BadField),
        "",
    );

    // 3. 环容量在册：恰 16 槽、满后覆盖最旧（时间序 at(0) 为最新入环后
    //    的最旧样本）。
    let mut small = SampleRing::new();
    for k in 0..20u64 {
        small.push(k, k * 2);
    }
    set.add(
        "v2 sample ring cap 16 overwrites oldest",
        small.len() == SAMPLE_RING_CAP && small.at(0) == Some((4, 8)) && small.at(15) == Some((19, 38)),
        "",
    );

    // 4. 往复几何：相位 0 贴左端、1000 贴右端、全程滑块不出轨道
    //    （验主册 F208「往复动画 2s 周期」的壳层几何联动）。
    let track = crate::h1star::h1base::Rect::new(100, 50, 200, 8);
    let ip = Progress::indeterminate(0);
    let p0 = indeterminate_items(track, ip.phase(0));
    let p1k = indeterminate_items(track, ip.phase(1_000));
    let mut inside = true;
    for ph in [0u32, 250, 500, 750, 1000] {
        let it = indeterminate_items(track, ph);
        if it[1].rect.x < track.x || it[1].rect.right() > track.right() {
            inside = false;
        }
    }
    set.add(
        "v2 indeterminate thumb rides track",
        p0[1].rect.x == 100 && p1k[1].rect.x == 250 && inside,
        "",
    );

    // 5. 周期 2s 同源：相位由 Progress::phase 提供（0 与 2000ms 同相位），
    //    几何面不持时钟（时间注入纪律）。
    set.add(
        "v2 geometry phase sourced from progress",
        ip.phase(0) == ip.phase(INDET_PERIOD_MS) && ip.phase(1_000) == 1000,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn sample_ring_order_preserved() {
        let mut r = SampleRing::new();
        r.push(100, 5);
        r.push(200, 10);
        assert_eq!(r.at(0), Some((100, 5)));
        assert_eq!(r.at(1), Some((200, 10)));
        assert!(r.at(2).is_none());
    }

    #[test]
    fn empty_ring_roundtrip() {
        let r = SampleRing::new();
        let back = SampleRing::from_bytes(&r.to_bytes()).unwrap();
        assert_eq!(back.len(), 0);
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_progfeed_v2_checks();
        assert!(set.all_passed(), "F208 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
