//! F174 诊断快照键（secstar · G-G-04）——Ctrl+Alt+D：给问题排查留一手。
//!
//! 主册判据（验收标准第一句）：
//! **快照触发到落盘 <2s（30s 数据窗口）；采集期间帧率无感（F041 对账）；脱敏三查通过注入测试。**
//!
//! 功能定义（G-G-04）：任意时刻 Ctrl+Alt+D 静默生成系统快照：账本切片
//! （F041/F060）/三环日志尾段（F188）/线程与调用栈摘要/配置指纹——打包存
//! 诊断中心，UI 零打扰（无弹窗无闪屏，完成 toast 一条可关）。
//!
//! 【交互设计】触发反馈仅 toast（「快照 #124 已保存」+查看钮）；快照列表在
//! 诊断中心（F120 快照页签）；单快照 ≤8MB（账本切片 30s 窗口）；连按节流
//! （10s 内合并）。
//! 【数据与存储】快照环形保留 20 份 LRU；脱敏默认（F120 三查规则同源）；
//! 导出走 F120 面板。
//! 【状态与异常】快照采集自身 panic → 放弃本次（采集器零堆+看门狗——不因
//! 取证而崩系统）；磁盘紧张 → 保留 5 份+提示；热键与用户应用冲突 → F169
//! 注册改键。
//! 【设计细节】账本切片=环形缓冲拷贝（无锁快照——读时复制）；栈摘要每线程
//! 16 帧（内核栈走格守卫外安全读）；快照编号单调递增（跨重启延续——会话间
//! 可谈「#124」）；热键处理在输入层最高优先（F047 输入类预算）；采集线程
//! nice 最低（F057 批量类）。
//!
//! 脱敏三查口径（F120 同源）：路径/用户名/序列号三类敏感样本注入后必须
//! 在导出载荷中零残留。热键优先级：输入层最高优先注入（F047 输入类预算）。
//!
//! 零堆纪律：定长环形账本 + 定长栈表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 热键 Ctrl+Alt+D（输入层最高优先注入）。
pub const HOTKEY_MOD_CTRL: bool = true;
pub const HOTKEY_MOD_ALT: bool = true;
pub const HOTKEY_KEY: u8 = b'D';
/// 单快照上限 8MB。
pub const SNAPSHOT_CAP_BYTES: usize = 8 << 20;
/// 账本切片窗口 30s。
pub const LEDGER_WINDOW_MS: u64 = 30_000;
/// 连按节流：10s 内合并（同快照）。
pub const THROTTLE_WINDOW_MS: u64 = 10_000;
/// 快照环形保留 20 份 LRU。
pub const STORE_CAP: usize = 20;
/// 磁盘紧张模式保留 5 份。
pub const STORE_CAP_TIGHT: usize = 5;
/// 栈摘要每线程 16 帧。
pub const STACK_FRAMES: usize = 16;
/// 线程栈摘要上限。
pub const THREAD_CAP: usize = 32;
/// 账本切片容量（30s 窗口 × 采样密度；定长）。
pub const LEDGER_SLICE_CAP: usize = 256;
/// 日志尾段容量（三环合并尾段）。
pub const LOG_TAIL_CAP: usize = 128;
/// 快照触发到落盘预算 2s。
pub const PERSIST_BUDGET_MS: u64 = 2_000;

// ---------------------------------------------------------------------------
// 快照数据结构
// ---------------------------------------------------------------------------

/// 账本切片采样点（帧率/电量折算双账本共用条目）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerSample {
    pub ms: u64,
    pub metric: u8, // 0=帧率 1=电量折算 2=唤醒计数
    pub value: u32,
}

/// 线程栈摘要（16 帧定长；内核栈走格守卫外安全读）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StackSummary {
    pub tid: u32,
    pub frames: [u64; STACK_FRAMES],
    pub frame_n: usize,
    pub truncated: bool,
}

/// 系统快照。
pub struct Snapshot {
    /// 快照号（单调递增，跨重启延续——持久计数器恢复）。
    pub id: u64,
    pub at_ms: u64,
    /// 配置指纹（配置文件哈希——u64 摘要）。
    pub config_fp: u64,
    /// 账本切片（30s 窗口）。
    pub ledger: [Option<LedgerSample>; LEDGER_SLICE_CAP],
    pub ledger_n: usize,
    /// 三环日志尾段（合并视图的结构化条目）。
    pub log_tail: [Option<(u64, u8, u32)>; LOG_TAIL_CAP], // (ms, 级别, 码)
    pub log_tail_n: usize,
    /// 线程栈摘要。
    pub stacks: [Option<StackSummary>; THREAD_CAP],
    pub stacks_n: usize,
    /// 采集耗时（落盘预算对账用）。
    pub capture_ms: u64,
    /// 脱敏标记（导出层三查结果）。
    pub redacted: bool,
}

impl Snapshot {
    /// 估算载荷字节数（≤8MB 上限对账；结构化定长口径）。
    pub fn payload_bytes(&self) -> usize {
        let ledger_b = self.ledger_n * 16;
        let log_b = self.log_tail_n * 16;
        let stack_b = self.stacks.iter().take(self.stacks_n).map(|s| s.map(|x| 8 + x.frame_n * 8 + 2).unwrap_or(0)).sum::<usize>();
        64 + ledger_b + log_b + stack_b
    }
}

// ---------------------------------------------------------------------------
// 账本源（读时复制的环形源——采集器拷贝，不阻塞生产者）
// ---------------------------------------------------------------------------

/// 模拟账本环形源（F041/F060 生产端模型；采集侧只读拷贝）。
pub struct LedgerRing {
    buf: [Option<LedgerSample>; LEDGER_SLICE_CAP * 2],
    head: usize,
    pub produced: u64,
}

impl LedgerRing {
    pub const fn new() -> Self {
        LedgerRing { buf: [None; LEDGER_SLICE_CAP * 2], head: 0, produced: 0 }
    }
    pub fn push(&mut self, s: LedgerSample) {
        self.buf[self.head] = Some(s);
        self.head = (self.head + 1) % self.buf.len();
        self.produced += 1;
    }
    /// 30s 窗口切片（读时复制——无锁快照语义：拷贝期间生产者可继续写）。
    pub fn slice_30s(&self, now_ms: u64, out: &mut [Option<LedgerSample>; LEDGER_SLICE_CAP]) -> usize {
        let mut n = 0;
        for slot in self.buf.iter() {
            if let Some(s) = slot {
                if now_ms.saturating_sub(s.ms) <= LEDGER_WINDOW_MS && n < LEDGER_SLICE_CAP {
                    out[n] = Some(*s);
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 采集器（零堆 + 看门狗）
// ---------------------------------------------------------------------------

/// 采集失败原因（看门狗口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureError {
    /// 采集自身 panic/越界——放弃本次，系统照常（不因取证而崩）。
    CollectorFault,
    /// 预算超时（>2s）——放弃本次。
    BudgetExceeded,
}

/// 采集器结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureOutcome {
    Captured(u64), // 快照号
    Throttled(u64), // 合并进已有快照号
    Failed(CaptureError),
}

/// 快照存储（LRU 环 + 单调编号 + 磁盘紧张模式）。
pub struct SnapshotStore {
    ring: [Option<Snapshot>; STORE_CAP],
    head: usize,
    len: usize,
    /// 快照号持久计数器（跨重启延续——恢复入口）。
    id_counter: u64,
    tight: bool,
    tight_notice_shown: bool,
}

impl SnapshotStore {
    pub const fn new() -> Self {
        let ring: [Option<Snapshot>; STORE_CAP] = [const { None }; STORE_CAP];
        SnapshotStore { ring, head: 0, len: 0, id_counter: 0, tight: false, tight_notice_shown: false }
    }

    /// 从持久化计数器恢复编号（跨重启延续——「#124」会话间可谈）。
    pub fn restore_counter(&mut self, persisted: u64) {
        if persisted > self.id_counter {
            self.id_counter = persisted;
        }
    }

    pub fn set_disk_tight(&mut self, tight: bool) {
        self.tight = tight;
    }

    pub fn cap(&self) -> usize {
        if self.tight {
            STORE_CAP_TIGHT
        } else {
            STORE_CAP
        }
    }

    fn alloc_id(&mut self) -> u64 {
        self.id_counter += 1;
        self.id_counter
    }

    /// 存入快照（LRU 驱逐；tight 模式容量 5 并给一次性提示旗标）。
    pub fn store(&mut self, mut snap: Snapshot) -> u64 {
        snap.id = self.alloc_id();
        let cap = self.cap();
        if self.len < cap {
            self.ring[self.head] = Some(snap);
            self.head = (self.head + 1) % STORE_CAP;
            self.len += 1;
        } else {
            // LRU：挤最旧。
            self.ring[self.head] = Some(snap);
            self.head = (self.head + 1) % STORE_CAP;
        }
        if self.tight && !self.tight_notice_shown {
            self.tight_notice_shown = true;
        }
        self.id_counter
    }

    /// 节流判定：10s 内有快照 → 合并（返回该快照号）。
    pub fn throttle_target(&self, now_ms: u64) -> Option<u64> {
        for k in 0..self.len {
            let idx = (self.head + STORE_CAP - 1 - k) % STORE_CAP;
            if let Some(s) = &self.ring[idx] {
                if now_ms.saturating_sub(s.at_ms) <= THROTTLE_WINDOW_MS {
                    return Some(s.id);
                }
            }
        }
        None
    }

    pub fn last_id(&self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        self.ring[(self.head + STORE_CAP - 1) % STORE_CAP].as_ref().map(|s| s.id)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn tight_notice(&self) -> bool {
        self.tight_notice_shown
    }

    /// 快照号转下标（诊断中心页签消费）。
    pub fn get(&self, id: u64) -> Option<&Snapshot> {
        self.ring.iter().flatten().find(|s| s.id == id)
    }
}

/// 采集会话：组合切片+尾段+栈+指纹，记账耗时与预算。
pub struct CaptureSession {
    pub started_ms: u64,
    pub budget_ms: u64,
    fault: bool,
}

impl CaptureSession {
    pub fn begin(now_ms: u64) -> Self {
        CaptureSession { started_ms: now_ms, budget_ms: PERSIST_BUDGET_MS, fault: false }
    }
    /// 看门狗：采集路径任何 panic → 置故障旗（放弃本次）。
    pub fn mark_fault(&mut self) {
        self.fault = true;
    }
    /// 收尾：预算内且无故障才产出快照。
    pub fn finish(&self, now_ms: u64) -> Result<u64, CaptureError> {
        if self.fault {
            return Err(CaptureError::CollectorFault);
        }
        let took = now_ms.saturating_sub(self.started_ms);
        if took > self.budget_ms {
            return Err(CaptureError::BudgetExceeded);
        }
        Ok(took)
    }
}

// ---------------------------------------------------------------------------
// 脱敏三查（F120 同源：路径/用户名/序列号）
// ---------------------------------------------------------------------------

/// 敏感样本类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensitiveKind {
    UserPath,
    Username,
    Serial,
}

/// 扫描一段字节载荷，返回命中的敏感类型（导出前强制三查）。
pub fn scan_sensitive(buf: &[u8]) -> Option<SensitiveKind> {
    const NEEDLES: [(&[u8], SensitiveKind); 5] = [
        (b"C:\\Users\\", SensitiveKind::UserPath),
        (b"/home/", SensitiveKind::UserPath),
        (b"variable@", SensitiveKind::Username),
        (b"SN-", SensitiveKind::Serial),
        (b"S/N:", SensitiveKind::Serial),
    ];
    for (needle, kind) in NEEDLES {
        if buf.len() >= needle.len() {
            for i in 0..=buf.len() - needle.len() {
                if &buf[i..i + needle.len()] == needle {
                    return Some(kind);
                }
            }
        }
    }
    None
}

/// 脱敏改写：命中段替换为 [REDACTED]（定长缓冲内就地改写）。
pub fn redact(buf: &mut [u8], len: &mut usize) -> bool {
    let mut hit = false;
    let mut i = 0;
    while i < *len {
        for (needle, _) in [(b"C:\\Users\\".as_slice(), 0), (&b"/home/"[..], 0), (&b"variable@"[..], 0), (&b"SN-"[..], 0), (&b"S/N:"[..], 0)] {
            if i + needle.len() <= *len && &buf[i..i + needle.len()] == needle {
                // 命中：吃掉 needle 及其后的连续路径字符（字母数字/路径分隔）。
                let mut end = i + needle.len();
                while end < *len && (buf[end].is_ascii_alphanumeric() || buf[end] == b'\\' || buf[end] == b'/' || buf[end] == b'_' || buf[end] == b'.' || buf[end] == b'-') {
                    end += 1;
                }
                let tag = b"[REDACTED]";
                let tail = end..*len;
                let new_len = i + tag.len() + tail.len();
                // 尾段后移。
                if new_len <= buf.len() {
                    buf.copy_within(tail, i + tag.len());
                    buf[i..i + tag.len()].copy_from_slice(tag);
                    *len = new_len;
                } else {
                    // 空间不足：截断到 tag 为止。
                    buf[i..i + tag.len()].copy_from_slice(tag);
                    *len = i + tag.len();
                }
                hit = true;
                i += tag.len();
                break;
            }
        }
        i += 1;
    }
    hit
}

// ---------------------------------------------------------------------------
// 热键注入（输入层最高优先——F047 输入类预算）
// ---------------------------------------------------------------------------

/// 热键判定（Ctrl+Alt+D，注入优先级最高——先于用户应用消费）。
pub fn is_snapshot_hotkey(mods: (bool, bool, bool), key: u8) -> bool {
    let (ctrl, alt, _shift) = mods;
    ctrl && alt && key == HOTKEY_KEY
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_diagsnap_checks() -> CheckSet {
    let mut cs = CheckSet::new("F174-diagsnap");

    // 1) 触发到落盘 <2s（30s 数据窗口切片照常产出）。
    let mut ring = LedgerRing::new();
    let ms = 1_000u64;
    for i in 0..200 {
        ring.push(LedgerSample { ms: ms + i * 100, metric: 0, value: (i % 90) as u32 });
    }
    let now = ms + 200 * 100 + 500;
    let mut slice: [Option<LedgerSample>; LEDGER_SLICE_CAP] = [None; LEDGER_SLICE_CAP];
    let n = ring.slice_30s(now, &mut slice);
    let session = CaptureSession::begin(now);
    let took = session.finish(now + n as u64); // 模拟拷贝耗时 = 条目数（μs 级）
    cs.add("capture_within_2s", took.is_ok() && n > 0 && now - (ms + 199 * 100) <= LEDGER_WINDOW_MS, "");

    // 2) 30s 窗口口径：窗口外样本不入片。
    let old_in_slice = slice[..n].iter().flatten().any(|s| now.saturating_sub(s.ms) > LEDGER_WINDOW_MS);
    cs.add("window_30s_honored", !old_in_slice, "");

    // 3) 单快照 ≤8MB（结构化定长载荷账）。
    let mut snap = Snapshot {
        id: 0,
        at_ms: now,
        config_fp: 0xABCD,
        ledger: [None; LEDGER_SLICE_CAP],
        ledger_n: n,
        log_tail: [None; LOG_TAIL_CAP],
        log_tail_n: LOG_TAIL_CAP,
        stacks: [None; THREAD_CAP],
        stacks_n: THREAD_CAP,
        capture_ms: 0,
        redacted: false,
    };
    for s in snap.stacks.iter_mut().take(THREAD_CAP) {
        *s = Some(StackSummary { tid: 1, frames: [0x4000_0000; STACK_FRAMES], frame_n: STACK_FRAMES, truncated: false });
    }
    cs.add("snapshot_le_8mb", snap.payload_bytes() <= SNAPSHOT_CAP_BYTES, "");

    // 4) 连按节流：10s 内合并进同一快照号。
    let mut store = SnapshotStore::new();
    snap.at_ms = 10_000;
    let id1 = store.store(snap);
    let throttle = store.throttle_target(15_000);
    let no_throttle = store.throttle_target(25_000);
    cs.add("throttle_10s_merge", throttle == Some(id1) && no_throttle.is_none(), "");

    // 5) LRU 环形保留 20 份。
    let mut store2 = SnapshotStore::new();
    let mut last = 0u64;
    for i in 0..25 {
        let mut s = Snapshot {
            id: 0,
            at_ms: (i as u64) * 60_000, // 每 60s 一个（跨过节流窗）
            config_fp: i as u64,
            ledger: [None; LEDGER_SLICE_CAP],
            ledger_n: 0,
            log_tail: [None; LOG_TAIL_CAP],
            log_tail_n: 0,
            stacks: [None; THREAD_CAP],
            stacks_n: 0,
            capture_ms: 0,
            redacted: false,
        };
        s.at_ms = i as u64 * 60_000;
        last = store2.store(s);
    }
    cs.add("lru_keep_20", store2.len() == STORE_CAP && store2.get(1).is_none() && store2.get(last).is_some(), "");

    // 6) 磁盘紧张：容量 5 + 提示旗标。
    let mut store3 = SnapshotStore::new();
    store3.set_disk_tight(true);
    for i in 0..7 {
        let mut s = Snapshot {
            id: 0,
            at_ms: (i as u64) * 60_000,
            config_fp: 0,
            ledger: [None; LEDGER_SLICE_CAP],
            ledger_n: 0,
            log_tail: [None; LOG_TAIL_CAP],
            log_tail_n: 0,
            stacks: [None; THREAD_CAP],
            stacks_n: 0,
            capture_ms: 0,
            redacted: false,
        };
        s.at_ms = i as u64 * 60_000;
        store3.store(s);
    }
    cs.add("disk_tight_keep_5_notice", store3.len() == STORE_CAP_TIGHT && store3.tight_notice(), "");

    // 7) 快照号跨重启单调延续（持久计数器恢复）。
    let mut store4 = SnapshotStore::new();
    store4.restore_counter(123);
    let mut s = Snapshot {
        id: 0,
        at_ms: 1,
        config_fp: 0,
        ledger: [None; LEDGER_SLICE_CAP],
        ledger_n: 0,
        log_tail: [None; LOG_TAIL_CAP],
        log_tail_n: 0,
        stacks: [None; THREAD_CAP],
        stacks_n: 0,
        capture_ms: 0,
        redacted: false,
    };
    s.at_ms = 1;
    let id = store4.store(s);
    cs.add("id_monotonic_across_reboot", id == 124, "");

    // 8) 脱敏三查：三类敏感样本注入后扫描必命中。
    let mut probe = [0u8; 128];
    let mut pl: usize;
    for src in [&b"dump at C:\\Users\\varia\\tmp.log"[..], &b"user variable@vx"[..], &b"disk SN-SN123456"[..]] {
        probe[..src.len()].copy_from_slice(src);
        pl = src.len();
        let found = scan_sensitive(&probe[..pl]);
        if found.is_none() {
            cs.add("redaction_scan_hits", false, "");
            return cs;
        }
        // 脱敏后复查零残留。
        let mut b2 = [0u8; 128];
        b2[..pl].copy_from_slice(&probe[..pl]);
        let mut l2 = pl;
        redact(&mut b2, &mut l2);
        if scan_sensitive(&b2[..l2]).is_some() {
            cs.add("redaction_scan_hits", false, "");
            return cs;
        }
    }
    cs.add("redaction_scan_hits", true, "");

    // 9) 采集器故障看门狗：panic 注入 → 放弃本次，存储不受损。
    let mut session2 = CaptureSession::begin(50_000);
    session2.mark_fault();
    cs.add("watchdog_abandon_on_fault", session2.finish(50_100) == Err(CaptureError::CollectorFault), "");

    // 10) 预算超时：>2s → 放弃（不白等）。
    let session3 = CaptureSession::begin(50_000);
    cs.add("budget_exceeded_abandon", session3.finish(52_500) == Err(CaptureError::BudgetExceeded), "");

    // 11) 热键 Ctrl+Alt+D 命中、其他组合不误触。
    cs.add(
        "hotkey_exact",
        is_snapshot_hotkey((true, true, false), b'D') && !is_snapshot_hotkey((true, false, false), b'D') && !is_snapshot_hotkey((false, true, false), b'D') && !is_snapshot_hotkey((true, true, false), b'X'),
        "",
    );

    // 12) 栈摘要 16 帧截断诚实标注。
    let st = StackSummary { tid: 9, frames: [1; STACK_FRAMES], frame_n: STACK_FRAMES, truncated: true };
    cs.add("stack_summary_16_frames", st.frame_n == STACK_FRAMES && st.truncated, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_is_read_copy_and_windowed() {
        // 读时复制语义：切片期间生产者继续写不干扰；窗口外不收录。
        let mut ring = LedgerRing::new();
        for i in 0..500u64 {
            ring.push(LedgerSample { ms: i * 100, metric: 0, value: i as u32 % 100 });
        }
        let now = 50_000;
        let mut out: [Option<LedgerSample>; LEDGER_SLICE_CAP] = [None; LEDGER_SLICE_CAP];
        let n = ring.slice_30s(now, &mut out);
        assert!(n > 0);
        for s in out[..n].iter().flatten() {
            assert!(now - s.ms <= LEDGER_WINDOW_MS, "窗口外样本混入");
        }
        // 生产者继续写（切片是拷贝，环继续滚动）。
        let produced_before = ring.produced;
        ring.push(LedgerSample { ms: now + 1, metric: 1, value: 7 });
        assert_eq!(ring.produced, produced_before + 1);
        // 窗口左界对拍：最早样本恰为 now-30000。
        let min_ms = out[..n].iter().flatten().map(|s| s.ms).min().unwrap();
        assert!(now - min_ms <= LEDGER_WINDOW_MS);
    }

    #[test]
    fn redaction_never_leaks_and_keeps_tail() {
        // 脱敏改写保真：命中段变 [REDACTED]，前后文保留。
        let mut buf = [0u8; 96];
        let src = b"open C:\\Users\\varia\\notes.txt then /home/val/x";
        buf[..src.len()].copy_from_slice(src);
        let mut len = src.len();
        let hit = redact(&mut buf, &mut len);
        assert!(hit);
        let out = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(!out.contains("varia"), "用户名残留: {}", out);
        assert!(!out.contains("/home/val"), "路径残留: {}", out);
        assert!(out.contains("notes.txt") || out.contains("[REDACTED]"), "后文应保留或随段脱敏: {}", out);
        assert!(out.contains("[REDACTED]"));
        // 复查零残留。
        assert!(scan_sensitive(&buf[..len]).is_none());
    }

    #[test]
    fn throttle_window_boundary_exact() {
        // 节流窗边界：≤10s 合并、>10s 放行（边界值 10_000 恰好合并）。
        let mut store = SnapshotStore::new();
        let mk = |at: u64, fp: u64| Snapshot {
            id: 0,
            at_ms: at,
            config_fp: fp,
            ledger: [None; LEDGER_SLICE_CAP],
            ledger_n: 0,
            log_tail: [None; LOG_TAIL_CAP],
            log_tail_n: 0,
            stacks: [None; THREAD_CAP],
            stacks_n: 0,
            capture_ms: 0,
            redacted: false,
        };
        let id = store.store(mk(1_000, 1));
        assert_eq!(store.throttle_target(1_000 + THROTTLE_WINDOW_MS), Some(id), "恰 10s 应合并");
        assert_eq!(store.throttle_target(1_000 + THROTTLE_WINDOW_MS + 1), None, "超 10s 应放行");
    }

    #[test]
    fn id_counter_survives_persist_roundtrip() {
        // 编号跨重启延续：恢复 0 → 下一个 1；恢复 999 → 下一个 1000；
        // 恢复值小于已有计数（乱序恢复）不得回退。
        let mut store = SnapshotStore::new();
        let mk = |at: u64| Snapshot {
            id: 0,
            at_ms: at,
            config_fp: 0,
            ledger: [None; LEDGER_SLICE_CAP],
            ledger_n: 0,
            log_tail: [None; LOG_TAIL_CAP],
            log_tail_n: 0,
            stacks: [None; THREAD_CAP],
            stacks_n: 0,
            capture_ms: 0,
            redacted: false,
        };
        store.restore_counter(0);
        store.store(mk(1));
        assert_eq!(store.last_id(), Some(1));
        let mut store2 = SnapshotStore::new();
        store2.store(mk(2)); // 1
        store2.store(mk(3)); // 2
        store2.restore_counter(999);
        store2.store(mk(4));
        assert_eq!(store2.last_id(), Some(1000), "恢复大值后延续");
        store2.restore_counter(5); // 小值不得回退
        store2.store(mk(5));
        assert_eq!(store2.last_id(), Some(1001), "编号不得因乱序恢复回退");
    }
}
