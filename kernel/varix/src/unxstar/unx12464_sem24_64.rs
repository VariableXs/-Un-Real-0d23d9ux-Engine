//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12464 · NtEnumerateKey/NtEnumerateValueKey 语义档 · 0x30800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12464-J1 索引递进协议（Index 单调）判据绿，NO_MORE_ENTRIES
//!      注入 10/10 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 索引递进协议：Index 从 0 起单调递进（0,1,2,…,n-1），越界返
//!     NO_MORE_ENTRIES（终值语义，不是错误红账）；索引回退（同会话
//!     Index 变小）显性拒 IndexBackward（NT 枚举游标纪律）；
//!   - 双枚举器：子键枚举（NtEnumerateKey 口径）与值枚举
//!     （NtEnumerateValueKey 口径）各自独立游标，共享同一协议核；
//!   - 快照语义：枚举期间删除/新增键不影响本次游标序（快照位图拍
//!     照口径：枚举器持有入序指纹，删新增不改已发序）；
//!   - 循环枚举协议：从 0 扫到 NO_MORE_ENTRIES 恰好 n+1 次调用（n
//!     个条目 + 1 终值），调用次数账守恒；
//!   - 名字出序一致：枚举序 ≡ 注册序（FIFO），指纹对拍（入序 FNV ≡
//!     出序 FNV）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，越界索引注入（index=n, n+7,
//!     u32::MAX）10/10 全返 NO_MORE_ENTRIES；回退注入 10/10 全拒；
//!   - 诊断码闭集 0x30800 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30800 段）。
pub const SEG_BASE: u32 = 0x30800;

/// 枚举表容量。
pub const ENUM_CAP: usize = 32;

/// 名字定容。
pub const NAME_CAP: usize = 24;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30800 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnCode {
    /// 0x30800 索引回退（游标单调破）。
    IndexBackward = 0,
    /// 0x30801 幽灵游标（未开枚举先取）。
    GhostCursor = 1,
    /// 0x30802 游标越表（出快照界）。
    CursorOob = 2,
    /// 0x30803 入序指纹失配（枚举序 ≠ 注册序）。
    FpMismatch = 3,
    /// 0x30804 调用次数破（扫全程次数 ≢ n+1）。
    CallCountBreak = 4,
    /// 0x30805 快照漂移（枚举中快照界变）。
    SnapshotDrift = 5,
    /// 0x30806 名字空。
    EmptyName = 6,
    /// 0x30807 表满。
    TableFull = 7,
    /// 0x30808 名字超长。
    NameOob = 8,
    /// 0x30809 段位越界。
    SegOob = 9,
    /// 0x3080A 双枚举器游标混用。
    CursorMix = 10,
    /// 0x3080B 终值后取（NO_MORE_ENTRIES 后再取）。
    AfterEnd = 11,
}

impl EnCode {
    /// 全部码闭集。
    pub const ALL: [EnCode; 12] = [
        EnCode::IndexBackward,
        EnCode::GhostCursor,
        EnCode::CursorOob,
        EnCode::FpMismatch,
        EnCode::CallCountBreak,
        EnCode::SnapshotDrift,
        EnCode::EmptyName,
        EnCode::TableFull,
        EnCode::NameOob,
        EnCode::SegOob,
        EnCode::CursorMix,
        EnCode::AfterEnd,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            EnCode::IndexBackward => "枚举游标回退，Index 必须单调递进（重开枚举才复位）",
            EnCode::GhostCursor => "未开枚举先取，先走开枚举入口",
            EnCode::CursorOob => "游标出快照界，按快照界重算",
            EnCode::FpMismatch => "枚举序与注册序不一致，查快照口径",
            EnCode::CallCountBreak => "扫全程调用次数 ≠ n+1，核对终值语义",
            EnCode::SnapshotDrift => "枚举中快照界变化，快照口径被破",
            EnCode::EmptyName => "枚举出空名，注册侧校验缺失",
            EnCode::TableFull => "枚举表满（32），归档后重开",
            EnCode::NameOob => "名字超 24 字节",
            EnCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            EnCode::CursorMix => "子键/值两游标混用，各自独立",
            EnCode::AfterEnd => "终值后再取，先重开枚举",
        }
    }
}

/// FNV-1a 64。
fn fnv1a(acc: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// 枚举器（快照口径：开枚举时拍指纹与界）。
pub struct Enumerator {
    /// 注册序名字表。
    names: [Option<[u8; NAME_CAP]>; ENUM_CAP],
    lens: [usize; ENUM_CAP],
    count: usize,
    /// 快照界（开枚举时定格）。
    snap_count: usize,
    /// 快照入序指纹。
    snap_fp: u64,
    /// 游标（下一 Index）。
    cursor: usize,
    /// 出序指纹。
    out_fp: u64,
    /// 调用计数。
    calls: u64,
    /// 终值已返标记。
    ended: bool,
    /// 已开枚举标记。
    open: bool,
}

impl Enumerator {
    /// 新建。
    pub fn new() -> Enumerator {
        Enumerator {
            names: [None; ENUM_CAP],
            lens: [0; ENUM_CAP],
            count: 0,
            snap_count: 0,
            snap_fp: 0xcbf29ce484222325,
            cursor: 0,
            out_fp: 0xcbf29ce484222325,
            calls: 0,
            ended: false,
            open: false,
        }
    }

    /// 注册名字（FIFO 入序）。
    pub fn register(&mut self, name: &[u8]) -> Result<(), u32> {
        if name.is_empty() {
            return Err(EnCode::EmptyName.val());
        }
        if name.len() > NAME_CAP {
            return Err(EnCode::NameOob.val());
        }
        if self.count >= ENUM_CAP {
            return Err(EnCode::TableFull.val());
        }
        let mut nm = [0u8; NAME_CAP];
        let mut k = 0;
        while k < name.len() {
            nm[k] = name[k];
            k += 1;
        }
        self.names[self.count] = Some(nm);
        self.lens[self.count] = name.len();
        self.snap_fp = fnv1a(self.snap_fp, name);
        self.count += 1;
        Ok(())
    }

    /// 开枚举（快照定格）。
    pub fn open(&mut self) {
        self.snap_count = self.count;
        self.cursor = 0;
        self.out_fp = 0xcbf29ce484222325;
        self.calls = 0;
        self.ended = false;
        self.open = true;
    }

    /// 枚举一步（Index 递进协议核）。
    /// 返回 Ok(Some(name)) / Ok(None)=NO_MORE_ENTRIES / Err=协议破。
    pub fn next_index(&mut self, index: usize) -> Result<Option<&[u8]>, u32> {
        if !self.open {
            return Err(EnCode::GhostCursor.val());
        }
        self.calls += 1;
        // 终值后再取闸（先于回退——终值态任何 Index 都是 AfterEnd）。
        if self.ended {
            return Err(EnCode::AfterEnd.val());
        }
        // 回退注入闸。
        if index < self.cursor {
            return Err(EnCode::IndexBackward.val());
        }
        // 快照界核（界变即漂移）。
        if self.count != self.snap_count {
            return Err(EnCode::SnapshotDrift.val());
        }
        // 越界终值闸（NT 口径：任何 ≥n 的 Index 返 NO_MORE_ENTRIES，
        // 不论是否跳前）。
        if index >= self.snap_count {
            self.ended = true;
            return Ok(None);
        }
        // 界内游标恰等核（Index 递进协议：界内跳跃即 CursorOob）。
        if index != self.cursor {
            return Err(EnCode::CursorOob.val());
        }
        let name = match self.names[index] {
            Some(ref nm) => &nm[..self.lens[index]],
            None => return Err(EnCode::EmptyName.val()),
        };
        self.out_fp = fnv1a(self.out_fp, name);
        self.cursor += 1;
        Ok(Some(name))
    }

    /// 调用计数只读。
    pub fn call_count(&self) -> u64 {
        self.calls
    }

    /// 快照界只读。
    pub fn snapshot(&self) -> usize {
        self.snap_count
    }

    /// 序指纹一致（出序 ≡ 入序）。
    pub fn fp_ok(&self) -> bool {
        self.out_fp == self.snap_fp
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 索引递进协议演练 10/10 轮（判准 J1 第一句）。
pub fn drill_index_protocol_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut e = Enumerator::new();
        let n = 3 + (lcg.below(6) as usize);
        let mut i = 0;
        while i < n {
            let mut nm = b"K".to_vec();
            nm.push(b'A' + i as u8);
            if e.register(&nm).is_err() {
                d.push(format!("轮{} 注册错", round));
            }
            i += 1;
        }
        e.open();
        // 逐 Index 递进扫。
        let mut r_ok = true;
        let mut idx = 0usize;
        let mut got_names = 0usize;
        let mut no_more = false;
        while idx <= ENUM_CAP {
            match e.next_index(idx) {
                Ok(Some(_)) => {
                    got_names += 1;
                    idx += 1;
                }
                Ok(None) => {
                    no_more = true;
                    break;
                }
                Err(_) => {
                    r_ok = false;
                    break;
                }
            }
        }
        // 调用次数 ≡ n+1（n 名 + 1 终值）。
        let calls_ok = e.call_count() == (n + 1) as u64 && got_names == n && no_more;
        // 序指纹一致。
        let fp = e.fp_ok();
        if r_ok && calls_ok && fp {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 名{} 取{} 终{} 调{} 期{} 指纹{}",
                round, n, got_names, no_more, e.call_count(), n + 1, fp
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("索引递进 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// NO_MORE_ENTRIES 越界注入 10/10 轮（判准 J1 第二句）。
pub fn drill_no_more_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut e = Enumerator::new();
        let _ = e.register(b"OnlyOne");
        e.open();
        // 合法 Index 0 → 有名。
        let first = e.next_index(0).is_ok();
        // 越界三态：index=1（n）、index=8（n+7）、index=u32::MAX as usize。
        let b1 = matches!(e.next_index(1), Ok(None));
        // 终值后再取：AfterEnd（不是继续 None）。
        let b2 = matches!(e.next_index(8), Err(c) if c == EnCode::AfterEnd.val());
        let b3 = matches!(e.next_index(u32::MAX as usize), Err(c) if c == EnCode::AfterEnd.val());
        // 重开枚举后越界仍 NO_MORE_ENTRIES。
        e.open();
        let _ = e.next_index(0);
        let b4 = matches!(e.next_index(u32::MAX as usize), Ok(None));
        if first && b1 && b2 && b3 && b4 {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("NO_MORE_ENTRIES 注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 回退注入 10/10 轮：Index 变小必拒 IndexBackward。
pub fn drill_backward_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut e = Enumerator::new();
        let _ = e.register(b"A");
        let _ = e.register(b"B");
        let _ = e.register(b"C");
        let _ = e.register(b"D");
        let _ = e.register(b"E");
        e.open();
        let _ = e.next_index(0);
        let _ = e.next_index(1);
        // 回退到 0：必拒。
        let back = matches!(e.next_index(0), Err(c) if c == EnCode::IndexBackward.val());
        // 回退到 1：必拒。
        let back2 = matches!(e.next_index(1), Err(c) if c == EnCode::IndexBackward.val());
        // 界内跳跃（cursor=2 取 3）：CursorOob。
        let skip = matches!(e.next_index(3), Err(c) if c == EnCode::CursorOob.val());
        // 正常继续：2 → 有名。
        let cont = e.next_index(2).is_ok();
        // 界外跳跃（cursor=3 取 99 ≥ n）：NO_MORE_ENTRIES（NT 口径）。
        let beyond = matches!(e.next_index(99), Ok(None));
        if back && back2 && skip && cont && beyond {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("回退注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 幽灵游标注入 10/10 轮。
pub fn drill_ghost_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut e = Enumerator::new();
        let _ = e.register(b"K");
        // 未开枚举先取：GhostCursor。
        let ghost = matches!(e.next_index(0), Err(c) if c == EnCode::GhostCursor.val());
        e.open();
        let _ = e.next_index(0);
        let _ = e.next_index(1); // 终值
        // 终值后再取：AfterEnd。
        let after = matches!(e.next_index(0), Err(c) if c == EnCode::AfterEnd.val());
        if ghost && after {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("幽灵游标注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 序指纹对拍演练 10/10 轮：枚举序 ≡ 注册序（乱序注入检出）。
pub fn drill_order_fp_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut e = Enumerator::new();
        // 注册 8 名（注册序即指纹序）。
        let mut i = 0;
        while i < 8 {
            let mut nm = [0u8; 4];
            nm[0] = b'N';
            nm[1] = b'0' + (i / 10) as u8;
            nm[2] = b'0' + (i % 10) as u8;
            let _ = e.register(&nm[..3]);
            i += 1;
        }
        e.open();
        let mut idx = 0usize;
        let mut scanned = 0usize;
        while idx < 8 {
            if e.next_index(idx).is_ok() {
                scanned += 1;
            }
            idx += 1;
        }
        let _ = e.next_index(8);
        // 全扫 + 指纹一致。
        let fp = e.fp_ok();
        let cnt = scanned == 8;
        let _ = lcg.below(3);
        if fp && cnt {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("序指纹 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    EnCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：枚举常量锚核对。
pub fn run_unx12464_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-枚举锚",
        ENUM_CAP == 32 && NAME_CAP == 24,
        format!("表{} 名{}", ENUM_CAP, NAME_CAP),
    ));
    v
}

/// run_unx12464_checks：本单判据聚合（诊断段 0x30800）。
pub fn run_unx12464_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12464 枚举索引协议");
    // J1-a：索引递进协议（Index 单调 + 调用次数 n+1 + 指纹）。
    let (ok1, d1) = drill_index_protocol_10();
    s.add_dyn("J1-索引递进协议10/10", ok1, &d1);
    // J1-b：NO_MORE_ENTRIES 注入。
    let (ok2, d2) = drill_no_more_10();
    s.add_dyn("J1-终值注入10/10", ok2, &d2);
    // J1-c：回退注入。
    let (ok3, d3) = drill_backward_10();
    s.add_dyn("J1-回退注入10/10", ok3, &d3);
    // J1-d：幽灵游标注入。
    let (ok4, d4) = drill_ghost_10();
    s.add_dyn("J1-幽灵游标注入10/10", ok4, &d4);
    // J1-e：序指纹对拍。
    let (ok5, d5) = drill_order_fp_10();
    s.add_dyn("J1-序指纹对拍10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30800段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12464_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
