//! F174 诊断快照键 · 批次四深化（secstar · G-G-04）。
//!
//! 批次四功能面（与批次三互补：批次三管「组装与脱敏」，本批管
//! 「线程、热键与完整性」）：
//! - [`ThreadTable`]：32 线程摘要表——tid/状态/栈顶三字段定长账
//!   （快照的线程面：谁在跑、卡在哪、栈在哪一线可见）；
//! - [`HotkeyChord`]：热键组合状态机——Ctrl/Alt/D 按序采集、乱序
//!   释放在途取消（组合期不误触发——IME 纪律的快照面）；
//! - [`ChunkExporter`]：导出分块器——定长块序列化（块序号/总块数/
//!   尾块短块），接收端可断点续收；
//! - [`integrity_hash`]：快照完整性 FNV-64——存储后复算防位腐
//!   （8MB 快照在盘上不腐烂的验证面）。
//!
//! 零堆纪律：定长表 + 定长块缓冲，无 alloc。

use super::diagsnap::{HOTKEY_KEY, LEDGER_WINDOW_MS, STORE_CAP, STORE_CAP_TIGHT, THREAD_CAP};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 线程摘要表
// ---------------------------------------------------------------------------

/// 线程状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Waiting,
    Blocked,
    Zombie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadRec {
    pub tid: u32,
    pub state: ThreadState,
    pub stack_top: u64,
}

/// 32 线程表（THREAD_CAP 同源）。
pub struct ThreadTable {
    recs: [Option<ThreadRec>; THREAD_CAP],
    pub n: usize,
}

impl ThreadTable {
    pub const fn new() -> ThreadTable {
        ThreadTable { recs: [const { None }; THREAD_CAP], n: 0 }
    }

    /// 登记（tid 重复拒——表内 tid 是主键）。
    pub fn add(&mut self, tid: u32, state: ThreadState, stack_top: u64) -> bool {
        if self.n >= THREAD_CAP || self.find(tid).is_some() {
            return false;
        }
        self.recs[self.n] = Some(ThreadRec { tid, state, stack_top });
        self.n += 1;
        true
    }

    pub fn find(&self, tid: u32) -> Option<ThreadRec> {
        self.recs[..self.n].iter().flatten().copied().find(|r| r.tid == tid)
    }

    /// 状态计数（快照摘要行——每状态多少线程）。
    pub fn count_state(&self, s: ThreadState) -> usize {
        self.recs[..self.n].iter().flatten().filter(|r| r.state == s).count()
    }

    /// 卡死嫌疑：Blocked 超过半数 → 摘要行黄标（趋势提示不诊断）。
    pub fn blocked_suspect(&self) -> bool {
        self.n > 0 && self.count_state(ThreadState::Blocked) * 2 > self.n
    }

    /// 状态字节面（导出用——状态打包 2bit）。
    pub fn state_byte(&self, s: ThreadState) -> u8 {
        match s {
            ThreadState::Running => 0,
            ThreadState::Waiting => 1,
            ThreadState::Blocked => 2,
            ThreadState::Zombie => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// 热键组合状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordState {
    /// 空闲（无键按下）。
    Idle,
    /// 部分按下（有修饰键但未齐）。
    Partial,
    /// 组合齐活（Ctrl+Alt+D 全按——触发窗口开 200ms）。
    Armed,
    /// 已触发（一次性——不重复触发）。
    Fired,
    /// 乱序释放取消（组合期松开任一键 → 回 Idle，不误触发）。
    Cancelled,
}

/// 触发窗口（组合齐活后 200ms 内 D 才算——防长按误连发）。
pub const CHORD_WINDOW_MS: u64 = 200;

pub struct HotkeyChord {
    pub state: ChordState,
    ctrl: bool,
    alt: bool,
    d_down: bool,
}

impl HotkeyChord {
    pub const fn new() -> HotkeyChord {
        HotkeyChord { state: ChordState::Idle, ctrl: false, alt: false, d_down: false }
    }

    /// 键事件（down=true 按下 / false 释放）。返回是否组合刚齐活。
    pub fn key(&mut self, key: u8, down: bool) -> bool {
        match key {
            b'C' => {
                self.ctrl = down;
            }
            b'A' => {
                self.alt = down;
            }
            k if k == HOTKEY_KEY => {
                self.d_down = down;
            }
            _ => {}
        }
        if !self.ctrl || !self.alt {
            // 修饰键不全：任何释放回 Idle，D 按下不触发。
            if self.state == ChordState::Armed && (!self.ctrl || !self.alt) {
                self.state = ChordState::Cancelled;
                return false;
            }
            if !self.ctrl && !self.alt && !self.d_down {
                self.state = ChordState::Idle;
            } else if self.ctrl || self.alt {
                self.state = ChordState::Partial;
            }
            return false;
        }
        if self.d_down {
            if self.state == ChordState::Armed {
                return false; // 已武装不连发（防抖——长按 D 只武装一次）
            }
            if self.state != ChordState::Fired {
                self.state = ChordState::Armed;
                return true; // 刚齐活
            }
            return false;
        }
        false
    }

    /// 超时一拍：Armed 200ms 内未确认 → 取消（窗口纪律）。
    pub fn tick(&mut self, armed_for_ms: u64) {
        if self.state == ChordState::Armed && armed_for_ms > CHORD_WINDOW_MS {
            self.state = ChordState::Cancelled;
        }
    }

    pub fn consume(&mut self) -> bool {
        if self.state == ChordState::Armed {
            self.state = ChordState::Fired;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 导出分块器
// ---------------------------------------------------------------------------

/// 块长（4KB——传输与落盘同尺）。
pub const CHUNK_SIZE: usize = 4_096;

/// 分块规划：payload 字节数 → (总块数, 尾块字节数)。
pub fn chunk_plan(payload_len: usize) -> (usize, usize) {
    if payload_len == 0 {
        return (0, 0);
    }
    let full = payload_len / CHUNK_SIZE;
    let tail = payload_len % CHUNK_SIZE;
    if tail == 0 {
        (full, 0)
    } else {
        (full + 1, tail)
    }
}

/// 块序号 → 字节区间（越界 None——接收端可断点续收）。
pub fn chunk_range(payload_len: usize, chunk_idx: usize) -> Option<(usize, usize)> {
    let (total, _) = chunk_plan(payload_len);
    if chunk_idx >= total {
        return None;
    }
    let start = chunk_idx * CHUNK_SIZE;
    let end = (start + CHUNK_SIZE).min(payload_len);
    Some((start, end))
}

// ---------------------------------------------------------------------------
// 完整性哈希（FNV-64）
// ---------------------------------------------------------------------------

/// FNV-1a 64（快照指纹——存储后复算防位腐）。
pub fn integrity_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 完整性验证：存储指纹与复算一致（位腐检出）。
pub fn integrity_ok(data: &[u8], stored: u64) -> bool {
    integrity_hash(data) == stored
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diagsnap_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F174-b4");

    // 1) 线程表：登记/主键去重/查询（tid 唯一性）。
    let mut t = ThreadTable::new();
    t.add(1, ThreadState::Running, 0x7F00_0000);
    let dup = t.add(1, ThreadState::Zombie, 0);
    cs.add(
        "thread_table_pk",
        t.n == 1 && !dup && t.find(1).unwrap().state == ThreadState::Running && t.find(2).is_none(),
        "",
    );

    // 2) 状态计数与卡死嫌疑：Blocked 2/3 过半 → 黄标（趋势提示）。
    let mut t2 = ThreadTable::new();
    t2.add(1, ThreadState::Running, 0x1000);
    t2.add(2, ThreadState::Blocked, 0x2000);
    t2.add(3, ThreadState::Blocked, 0x3000);
    let suspect = t2.blocked_suspect();
    t2.add(4, ThreadState::Waiting, 0x4000);
    cs.add(
        "thread_blocked_suspect",
        suspect && !t2.blocked_suspect() && t2.count_state(ThreadState::Blocked) == 2,
        "",
    );

    // 3) 状态字节面：四状态 2bit 打包（0-3 可逆）。
    cs.add(
        "thread_state_bytes",
        t2.state_byte(ThreadState::Running) == 0
            && t2.state_byte(ThreadState::Waiting) == 1
            && t2.state_byte(ThreadState::Blocked) == 2
            && t2.state_byte(ThreadState::Zombie) == 3,
        "",
    );

    // 4) 热键按序齐活：Ctrl→Alt→D → Armed 且消费一次（触发一次性）。
    let mut h = HotkeyChord::new();
    h.key(b'C', true);
    h.key(b'A', true);
    let armed = h.key(HOTKEY_KEY, true);
    let consumed = h.consume();
    let again = h.consume();
    cs.add("chord_arm_consume", armed && consumed && !again && h.state == ChordState::Fired, "");

    // 5) 热键乱序释放取消：D 按下后松 Ctrl → Cancelled 不误触发。
    let mut h2 = HotkeyChord::new();
    h2.key(b'C', true);
    h2.key(b'A', true);
    h2.key(HOTKEY_KEY, true);
    h2.key(b'C', false);
    cs.add("chord_release_cancels", h2.state == ChordState::Cancelled && !h2.consume(), "");

    // 6) 热键窗口：Armed 超 200ms 未确认 → 取消（防长按误连发）。
    let mut h3 = HotkeyChord::new();
    h3.key(b'C', true);
    h3.key(b'A', true);
    h3.key(HOTKEY_KEY, true);
    h3.tick(CHORD_WINDOW_MS + 1);
    cs.add("chord_window_cancels", h3.state == ChordState::Cancelled, "");

    // 7) 热键长按不连发：D 保持按下只武装一次（防抖）。
    let mut h4 = HotkeyChord::new();
    h4.key(b'C', true);
    h4.key(b'A', true);
    let first = h4.key(HOTKEY_KEY, true);
    let repeat = h4.key(HOTKEY_KEY, true);
    cs.add("chord_no_autorepeat", first && !repeat, "");

    // 8) 分块规划：12KB → 3 满块尾 0；12KB+1 → 4 块尾 1（规划两态）。
    cs.add(
        "chunk_plan",
        chunk_plan(3 * CHUNK_SIZE) == (3, 0) && chunk_plan(3 * CHUNK_SIZE + 1) == (4, 1) && chunk_plan(0) == (0, 0),
        "",
    );

    // 9) 分块区间：逐块区间覆盖全载荷不重不漏（续收面）。
    let payload = CHUNK_SIZE * 2 + 100;
    let (total, _) = chunk_plan(payload);
    let mut covered = 0usize;
    let mut contiguous = true;
    for i in 0..total {
        let (s, e) = chunk_range(payload, i).unwrap();
        contiguous &= s == covered;
        covered = e;
    }
    cs.add("chunk_ranges_cover", contiguous && covered == payload && chunk_range(payload, total).is_none(), "");

    // 10) 完整性哈希：同数据同指纹、一位翻转必检出（两面）。
    let data = [0xA5u8; 512];
    let h1 = integrity_hash(&data);
    let mut corrupted = data;
    corrupted[300] ^= 0x01;
    cs.add(
        "integrity_two_ways",
        integrity_ok(&data, h1) && !integrity_ok(&corrupted, h1),
        "",
    );

    // 11) 主册常量贯通：热键 D / 30s 账窗 / 20 与 5 槽档位一处一事实。
    cs.add(
        "consts_aligned",
        HOTKEY_KEY == b'D' && LEDGER_WINDOW_MS == 30_000 && STORE_CAP == 20 && STORE_CAP_TIGHT == 5 && THREAD_CAP == 32,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn chord_full_lifecycle() {
        // 全生命周期：齐活→消费→释放回 Idle→再次齐活（复用语义）。
        let mut h = HotkeyChord::new();
        h.key(b'C', true);
        h.key(b'A', true);
        assert!(h.key(HOTKEY_KEY, true));
        assert!(h.consume());
        h.key(HOTKEY_KEY, false);
        h.key(b'A', false);
        h.key(b'C', false);
        assert_eq!(h.state, ChordState::Idle);
        h.key(b'C', true);
        h.key(b'A', true);
        assert!(h.key(HOTKEY_KEY, true), "二次组合应重新武装");
    }

    #[test]
    fn thread_table_cap_boundary() {
        // 恰 32 线程全收、第 33 拒（容量边界逐点）。
        let mut t = ThreadTable::new();
        for tid in 0..THREAD_CAP as u32 {
            assert!(t.add(tid, ThreadState::Waiting, tid as u64));
        }
        assert!(!t.add(999, ThreadState::Running, 0));
        assert_eq!(t.n, THREAD_CAP);
    }

    #[test]
    fn integrity_scales() {
        // 大数据完整性：8KB 数据一位翻转检出（快照尺度的验证面）。
        let data = vec![0x3Cu8; 8 * 1024];
        let h = integrity_hash(&data);
        let mut bad = data.clone();
        bad[8 * 1024 - 1] ^= 0x80;
        assert!(integrity_ok(&data, h));
        assert!(!integrity_ok(&bad, h));
    }
}
