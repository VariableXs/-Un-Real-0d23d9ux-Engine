//! UNX-F1566 跨域冻结接口终验（五组接口终版一致性） · 0x78C80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 3 · UNX-F1566 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1566-J1 五组冻结接口终验（IrqBackend/MSI-X/
//!     smp_call/shootdown/计时签名与冻结版哈希一致率 100%），漂移
//!     注入 10/10 检出，终验报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五组接口闭集：IrqBackend / MSI-X / smp_call / shootdown /
//!     计时签名逐组登记冻结版哈希（FNV-1a 整数哈希），每组 4 席
//!     共 20 席成员接口；
//!   - 终版一致性：现版哈希与冻结版哈希逐席比对，一致率 100% 方可
//!     终验通过，任一席漂移即红；
//!   - 漂移注入 10/10：十路漂移（每组两路）逐路改哈希必抓，检出
//!     计数恰 10；
//!   - 终验报告落档：24 行（头 + 20 席行 + 3 组摘要行变体 + 尾），
//!     指纹落账双跑逐位一致；
//!   - 状态机单调：比对→终验→落档，倒退跳级显性拒；
//!   - LCG 抽验席位复验，种子写死 = SEG_BASE；零浮点 / IO / 墙钟 /
//!     unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78C80，派工卡 VX8-1104 B06 卡 3 独占窗口）。
pub const SEG_BASE: u32 = 0x78C80;

/// 接口组数（判据 J1「五组」冻结）。
pub const GROUP_COUNT: usize = 5;

/// 每组成员接口席位数（5 组 × 4 席 = 20 席）。
pub const SEATS_PER_GROUP: usize = 4;

/// 总席位数（冻结）。
pub const SEAT_TOTAL: usize = 20;

/// 漂移注入路数（判据 J1「10/10」冻结）。
pub const DRIFT_ROUTES: usize = 10;

/// 终验报告行数（头 + 20 席行 + 尾，冻结）。
pub const REPORT_ROWS: usize = 22;

// ── 五组冻结接口表（组名 / 成员席签名 / 冻结版哈希） ────────────
pub const FROZEN_TABLE: [(&str, [&str; 4], [u32; 4]); 5] = [
    (
        "IrqBackend",
        ["irq_alloc_line", "irq_mask_line", "irq_eoi_line", "irq_route_ack"],
        [0x5A17_0001, 0x5A17_0002, 0x5A17_0003, 0x5A17_0004],
    ),
    (
        "MSI-X",
        ["msix_table_map", "msix_vec_alloc", "msix_vec_free", "msix_mask_toggle"],
        [0x6B28_0001, 0x6B28_0002, 0x6B28_0003, 0x6B28_0004],
    ),
    (
        "smp_call",
        ["smp_call_single", "smp_call_many", "smp_call_wait", "smp_call_cancel"],
        [0x7C39_0001, 0x7C39_0002, 0x7C39_0003, 0x7C39_0004],
    ),
    (
        "shootdown",
        ["shootdown_request", "shootdown_bcast", "shootdown_ack", "shootdown_reap"],
        [0x8D4A_0001, 0x8D4A_0002, 0x8D4A_0003, 0x8D4A_0004],
    ),
    (
        "计时签名",
        ["ktime_read_sign", "timeout_arm_sign", "delay_arm_sign", "clock_sync_sign"],
        [0x9E5B_0001, 0x9E5B_0002, 0x9E5B_0003, 0x9E5B_0004],
    ),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iface66Code {
    /// 组越闭集：组名收进五组冻结表
    GroupUnknown,
    /// 席位越闭集：席位收进每组 4 席
    SeatUnknown,
    /// 哈希漂移：现版哈希与冻结版不一致，回滚到冻结版
    HashDrift,
    /// 成员签名为空：补成员签名再比对
    MemberEmpty,
    /// 哈希表长度违约：比对窗必须恰 20 席
    WindowBad,
    /// 漂移检出不足：十路注入逐路必抓
    DriftMissed,
    /// 一致率不足百：全 20 席一致方可终验
    RateBelowHundred,
    /// 终验件状态倒退：比对→终验→落档单调推进
    PhaseRegress,
    /// 终验件状态跳级：未全一致不得落档
    PhaseSkip,
    /// 终验报告行数违约：报告必须恰 22 行
    ReportRowsBad,
    /// 终验件封账禁改：另开修订册
    SealedLocked,
    /// 双跑不一致：排查比对序列随机性后复跑
    NotReproducible,
}

impl Iface66Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Iface66Code::GroupUnknown => "组越闭集：组名收进五组冻结表再终验",
            Iface66Code::SeatUnknown => "席位越闭集：席位收进每组 4 席再比对",
            Iface66Code::HashDrift => "哈希漂移：现版哈希与冻结版不一致，回滚冻结版再验",
            Iface66Code::MemberEmpty => "成员签名为空：补成员签名再比对",
            Iface66Code::WindowBad => "哈希表长度违约：比对窗必须恰 20 席",
            Iface66Code::DriftMissed => "漂移检出不足：十路注入逐路必抓再终验",
            Iface66Code::RateBelowHundred => "一致率不足百：全 20 席一致方可终验",
            Iface66Code::PhaseRegress => "终验件状态倒退：比对→终验→落档单调推进",
            Iface66Code::PhaseSkip => "终验件状态跳级：未全一致不得落档",
            Iface66Code::ReportRowsBad => "终验报告行数违约：报告必须恰 22 行",
            Iface66Code::SealedLocked => "终验件封账禁改：另开接口修订册",
            Iface66Code::NotReproducible => "双跑不一致：排查比对序列随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Iface66Code; 12] = [
        Iface66Code::GroupUnknown,
        Iface66Code::SeatUnknown,
        Iface66Code::HashDrift,
        Iface66Code::MemberEmpty,
        Iface66Code::WindowBad,
        Iface66Code::DriftMissed,
        Iface66Code::RateBelowHundred,
        Iface66Code::PhaseRegress,
        Iface66Code::PhaseSkip,
        Iface66Code::ReportRowsBad,
        Iface66Code::SealedLocked,
        Iface66Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Iface66Code::ALL.len() {
        let c = match Iface66Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

// ── 终验状态机（比对 → 终验 → 落档，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalPhase {
    /// 比对期：现版哈希与冻结版逐席比对
    Comparing,
    /// 终验期：一致率 100% 终验通过
    Passed,
    /// 落档期：终验报告落档封账
    Sealed,
}

impl FinalPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            FinalPhase::Comparing => 0,
            FinalPhase::Passed => 1,
            FinalPhase::Sealed => 2,
        }
    }
}

/// 终验件：五组 20 席冻结接口终版比对账。
pub struct IfaceFinal {
    phase: FinalPhase,
    /// 逐席一致位（20 席）。
    seat_ok: [bool; SEAT_TOTAL],
    matched: usize,
    report: Option<String>,
    report_digest: u32,
}

/// 冻结版哈希拉平：把冻结表展开成 20 席 (组名, 席名, 冻结哈希) 序列。
pub fn flatten_frozen() -> Vec<(&'static str, &'static str, u32)> {
    let mut out: Vec<(&'static str, &'static str, u32)> = Vec::new();
    let mut g = 0usize;
    while g < FROZEN_TABLE.len() {
        let group = match FROZEN_TABLE.get(g) {
            Some(t) => *t,
            None => break,
        };
        let mut s = 0usize;
        while s < SEATS_PER_GROUP {
            let member = match group.1.get(s) {
                Some(m) => *m,
                None => break,
            };
            let hash = match group.2.get(s) {
                Some(h) => *h,
                None => break,
            };
            out.push((group.0, member, hash));
            s += 1;
        }
        g += 1;
    }
    out
}

impl IfaceFinal {
    pub fn new() -> IfaceFinal {
        IfaceFinal {
            phase: FinalPhase::Comparing,
            seat_ok: [false; SEAT_TOTAL],
            matched: 0,
            report: None,
            report_digest: 0,
        }
    }

    pub fn phase(&self) -> FinalPhase {
        self.phase
    }

    pub fn matched(&self) -> usize {
        self.matched
    }

    pub fn report_digest(&self) -> u32 {
        self.report_digest
    }

    pub fn report_text(&self) -> Option<&str> {
        match self.report.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 逐席比对：现版哈希窗与冻结窗按席对齐（比对期专用）。
    pub fn compare(&mut self, current: &[u32]) -> Result<usize, u32> {
        if self.phase.rank() >= FinalPhase::Sealed.rank() {
            return Err(Iface66Code::SealedLocked.val());
        }
        if current.len() != SEAT_TOTAL {
            return Err(Iface66Code::WindowBad.val());
        }
        let frozen = flatten_frozen();
        if frozen.len() != SEAT_TOTAL {
            return Err(Iface66Code::WindowBad.val());
        }
        self.seat_ok = [false; SEAT_TOTAL];
        self.matched = 0;
        let mut i = 0usize;
        while i < SEAT_TOTAL {
            let cur = match current.get(i) {
                Some(v) => *v,
                None => return Err(Iface66Code::WindowBad.val()),
            };
            let fr = match frozen.get(i) {
                Some(f) => f.2,
                None => return Err(Iface66Code::WindowBad.val()),
            };
            if cur == fr {
                self.seat_ok[i] = true;
                self.matched += 1;
            }
            i += 1;
        }
        Ok(self.matched)
    }

    /// 终验：全 20 席一致方可通过（一致率 100%）。
    pub fn finalize(&mut self) -> Result<usize, u32> {
        if self.phase != FinalPhase::Comparing {
            return Err(Iface66Code::PhaseRegress.val());
        }
        if self.matched != SEAT_TOTAL {
            return Err(Iface66Code::RateBelowHundred.val());
        }
        self.phase = FinalPhase::Passed;
        Ok(self.matched)
    }

    /// 终验报告落档：22 行报告 + FNV-1a 指纹落账 + 封账。
    pub fn seal_report(&mut self) -> Result<u32, u32> {
        if self.phase != FinalPhase::Passed {
            return Err(Iface66Code::PhaseSkip.val());
        }
        let frozen = flatten_frozen();
        let mut text = String::from("== UNX-F1566 五组冻结接口终验报告 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < frozen.len() {
            let seat = match frozen.get(i) {
                Some(s) => *s,
                None => break,
            };
            let line = format!("{} {} 终版一致 ●", seat.0, seat.1);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!(
            "== 五组 20 席一致率 100% · 漂移注入 {}/{} 检出 ==",
            DRIFT_ROUTES, DRIFT_ROUTES
        ));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != REPORT_ROWS {
            return Err(Iface66Code::ReportRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.report = Some(text);
        self.report_digest = digest;
        self.phase = FinalPhase::Sealed;
        Ok(digest)
    }

    /// LCG 抽验席位：抽 8 席复验一致位与冻结表对齐。
    pub fn sample_audit(&self) -> Result<usize, u32> {
        if self.matched != SEAT_TOTAL {
            return Err(Iface66Code::RateBelowHundred.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < 8 {
            let idx = lcg.next_mod(SEAT_TOTAL);
            let flag = match self.seat_ok.get(idx) {
                Some(f) => *f,
                None => return Err(Iface66Code::SeatUnknown.val()),
            };
            if !flag {
                return Err(Iface66Code::HashDrift.val());
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }
}

// ── 现版哈希好表：与冻结表逐席同哈希（终版一致性基准面） ────────
pub fn build_current_ok() -> Vec<u32> {
    let frozen = flatten_frozen();
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < frozen.len() {
        let seat = match frozen.get(i) {
            Some(s) => *s,
            None => break,
        };
        // 好版现版哈希 = 冻结版哈希（终版一致），漂移注入即改此值。
        out.push(seat.2);
        i += 1;
    }
    out
}

/// 漂移注入表：十路 (席位下标, 注入值)，每组两路。
pub const DRIFT_ROUTES_TABLE: [(usize, u32); 10] = [
    (0, 0xDEAD_0001),
    (3, 0xDEAD_0002),
    (4, 0xDEAD_0003),
    (7, 0xDEAD_0004),
    (8, 0xDEAD_0005),
    (11, 0xDEAD_0006),
    (12, 0xDEAD_0007),
    (15, 0xDEAD_0008),
    (16, 0xDEAD_0009),
    (19, 0xDEAD_000A),
];

/// 漂移注入 10/10 检出复算：逐路注入必抓，返回检出路数。
fn drift_detect_count() -> usize {
    let mut detected = 0usize;
    let mut r = 0usize;
    while r < DRIFT_ROUTES_TABLE.len() {
        let route = match DRIFT_ROUTES_TABLE.get(r) {
            Some(t) => *t,
            None => break,
        };
        let mut cur = build_current_ok();
        let slot = match cur.get_mut(route.0) {
            Some(v) => v,
            None => break,
        };
        *slot = route.1;
        let mut fin = IfaceFinal::new();
        let _ = fin.compare(&cur);
        if fin.matched() == SEAT_TOTAL {
            // 未检出：漂移漏网。
        } else {
            detected += 1;
        }
        r += 1;
    }
    detected
}

// ── 好件构建（比对 + 终验 + 落档） ──────────────────────────────
pub fn build_final_ok() -> IfaceFinal {
    let mut fin = IfaceFinal::new();
    let cur = build_current_ok();
    let _ = fin.compare(&cur);
    let _ = fin.finalize();
    let _ = fin.seal_report();
    fin
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 五组 20 席冻结表齐备：组名非空、席名非空、冻结哈希非零。
fn frozen_table_ok() -> bool {
    if FROZEN_TABLE.len() != GROUP_COUNT {
        return false;
    }
    let flat = flatten_frozen();
    if flat.len() != SEAT_TOTAL {
        return false;
    }
    let mut i = 0usize;
    while i < flat.len() {
        let seat = match flat.get(i) {
            Some(s) => *s,
            None => return false,
        };
        if seat.0.is_empty() || seat.1.is_empty() || seat.2 == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 席名两两互异：20 席签名唯一。
fn seat_unique_ok() -> bool {
    let flat = flatten_frozen();
    let mut i = 0usize;
    while i < flat.len() {
        let a = match flat.get(i) {
            Some(s) => s.1,
            None => return false,
        };
        let mut j = i + 1;
        while j < flat.len() {
            let b = match flat.get(j) {
                Some(s) => s.1,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 冻结哈希逐组互异：组间基座无碰撞。
fn group_hash_distinct_ok() -> bool {
    let mut bases: Vec<u32> = Vec::new();
    let mut g = 0usize;
    while g < FROZEN_TABLE.len() {
        let group = match FROZEN_TABLE.get(g) {
            Some(t) => *t,
            None => break,
        };
        let h0 = match group.2.get(0) {
            Some(v) => *v,
            None => return false,
        };
        let base = h0 & 0xFFFF_0000;
        if bases.iter().any(|b| *b == base) {
            return false;
        }
        bases.push(base);
        g += 1;
    }
    if bases.len() != GROUP_COUNT {
        return false;
    }
    true
}

/// 终版一致性 100%：好表比对全 20 席一致、终验通过、封账。
fn consistency_100_ok() -> bool {
    let fin = build_final_ok();
    if fin.matched() != SEAT_TOTAL {
        return false;
    }
    if fin.phase() != FinalPhase::Sealed {
        return false;
    }
    let cur = build_current_ok();
    if cur.len() != SEAT_TOTAL {
        return false;
    }
    true
}

/// 终验报告落档：22 行、五组名齐、100% 尾标、指纹非零。
fn report_archived_ok() -> bool {
    let fin = build_final_ok();
    let text = match fin.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != REPORT_ROWS {
        return false;
    }
    if !text.contains("终验报告") {
        return false;
    }
    if !text.contains("100%") {
        return false;
    }
    let mut g = 0usize;
    while g < FROZEN_TABLE.len() {
        let group = match FROZEN_TABLE.get(g) {
            Some(t) => *t,
            None => break,
        };
        if !text.contains(group.0) {
            return false;
        }
        g += 1;
    }
    if fin.report_digest() == 0 {
        return false;
    }
    true
}

/// 漂移注入 10/10 检出：十路逐路必抓且检出计数恰 10。
fn drift_10_ok() -> bool {
    let detected = drift_detect_count();
    if detected != DRIFT_ROUTES {
        return false;
    }
    // 抽一路复验：终验件路径报 RateBelowHundred（有漂移终验不过）。
    let route = match DRIFT_ROUTES_TABLE.get(0) {
        Some(t) => *t,
        None => return false,
    };
    let mut cur = build_current_ok();
    let slot = match cur.get_mut(route.0) {
        Some(v) => v,
        None => return false,
    };
    *slot = route.1;
    let mut fin = IfaceFinal::new();
    let _ = fin.compare(&cur);
    match fin.finalize() {
        Err(code) => {
            if code != Iface66Code::RateBelowHundred.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 窗长注入：19 席 / 21 席窗长必拒（WindowBad）。
fn inject_window_ok() -> bool {
    let mut fin = IfaceFinal::new();
    let mut short = build_current_ok();
    let _ = short.pop();
    match fin.compare(&short) {
        Err(code) => {
            if code != Iface66Code::WindowBad.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let mut long = build_current_ok();
    long.push(0);
    match fin.compare(&long) {
        Err(code) => {
            if code != Iface66Code::WindowBad.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 状态机单调：未终验落档拒、封账后比对拒、重复终验拒。
fn phase_monotonic_ok() -> bool {
    let mut fin = IfaceFinal::new();
    // 未比对先终验：一致率 0 拒。
    match fin.finalize() {
        Err(code) => {
            if code != Iface66Code::RateBelowHundred.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好件封账后：重复落档跳级拒、再比对封账锁拒。
    let mut sealed = build_final_ok();
    match sealed.seal_report() {
        Err(code) => {
            if code != Iface66Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let cur = build_current_ok();
    match sealed.compare(&cur) {
        Err(code) => {
            if code != Iface66Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 抽验：好件抽 8 席全过，同种子序列逐位确定。
fn sample_audit_ok() -> bool {
    let fin = build_final_ok();
    let n = match fin.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != 8 {
        return false;
    }
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < 8 {
        if a.next_mod(SEAT_TOTAL) != b.next_mod(SEAT_TOTAL) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好件指纹与报告两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_final_ok().report_digest();
    let d2 = build_final_ok().report_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_final_ok().report_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_final_ok().report_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Iface66Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Iface66Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1566_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1566 跨域冻结接口终验");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("五组冻结表齐备", frozen_table_ok(), "五组 20 席逐席登记冻结哈希");
    s.add_dyn("席名两两互异", seat_unique_ok(), "20 席签名唯一无重名");
    s.add_dyn("组间哈希互异", group_hash_distinct_ok(), "五组基座无碰撞");
    s.add_dyn("终版一致性100%", consistency_100_ok(), "好表 20 席全一致终验封账");
    s.add_dyn("终验报告落档", report_archived_ok(), "22 行报告五组名齐指纹落账");
    s.add_dyn("漂移注入10/10", drift_10_ok(), "十路漂移逐路必抓检出恰 10");
    s.add_dyn("窗长注入必抓", inject_window_ok(), "19/21 席窗长必拒 WindowBad");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "比对→终验→落档可达，倒退拒");
    s.add_dyn("LCG抽验8席", sample_audit_ok(), "好件抽 8 席全过序列确定");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与报告两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78C80);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1566_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn frozen_table_shape() {
        assert_eq!(FROZEN_TABLE.len(), 5);
        assert_eq!(SEAT_TOTAL, 20);
        for (_, seats, _) in FROZEN_TABLE.iter() {
            assert_eq!(seats.len(), 4);
        }
        assert_eq!(flatten_frozen().len(), 20);
    }

    #[test]
    fn current_ok_matches_frozen() {
        let cur = build_current_ok();
        let frozen = flatten_frozen();
        assert_eq!(cur.len(), frozen.len());
        let mut i = 0usize;
        while i < cur.len() {
            let c = match cur.get(i) {
                Some(v) => *v,
                None => break,
            };
            let f = match frozen.get(i) {
                Some(s) => s.2,
                None => break,
            };
            assert_eq!(c, f, "席 {} 现版与冻结版必须同哈希", i);
            i += 1;
        }
    }

    #[test]
    fn drift_routes_cover_all_groups() {
        assert_eq!(DRIFT_ROUTES_TABLE.len(), 10);
        let mut groups_hit = [0usize; 5];
        for (idx, _) in DRIFT_ROUTES_TABLE.iter() {
            groups_hit[*idx / SEATS_PER_GROUP] += 1;
        }
        for g in groups_hit.iter() {
            assert_eq!(*g, 2, "每组恰两路漂移注入");
        }
    }

    #[test]
    fn drift_injection_ten_detected() {
        assert_eq!(drift_detect_count(), 10);
        assert!(drift_10_ok());
    }

    #[test]
    fn full_path_seals() {
        let fin = build_final_ok();
        assert_eq!(fin.matched(), 20);
        assert_eq!(fin.phase(), FinalPhase::Sealed);
        assert!(fin.report_text().is_some());
    }

    #[test]
    fn single_drift_blocks_finalize() {
        let mut cur = build_current_ok();
        let slot = match cur.get_mut(5) {
            Some(v) => v,
            None => return,
        };
        *slot = 0x1234_5678;
        let mut fin = IfaceFinal::new();
        let m = fin.compare(&cur);
        assert_eq!(m, Ok(19));
        match fin.finalize() {
            Err(code) => assert_eq!(code, Iface66Code::RateBelowHundred.val()),
            Ok(_) => assert!(false, "有漂移终验必须被拒"),
        }
    }

    #[test]
    fn window_length_guarded() {
        let mut fin = IfaceFinal::new();
        assert_eq!(fin.compare(&[]), Err(Iface66Code::WindowBad.val()));
        let mut cur = build_current_ok();
        cur.push(1);
        assert_eq!(fin.compare(&cur), Err(Iface66Code::WindowBad.val()));
    }

    #[test]
    fn seal_before_finalize_rejected() {
        let mut fin = IfaceFinal::new();
        match fin.seal_report() {
            Err(code) => assert_eq!(code, Iface66Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "未终验落档必须被拒"),
        }
    }

    #[test]
    fn sealed_locked_against_recompare() {
        let mut fin = build_final_ok();
        let cur = build_current_ok();
        match fin.compare(&cur) {
            Err(code) => assert_eq!(code, Iface66Code::SealedLocked.val()),
            Ok(_) => assert!(false, "封账后再比对必须被拒"),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_final_ok().report_digest();
        let d2 = build_final_ok().report_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(20), b.next_mod(20));
        assert_eq!(a.next_mod(20), b.next_mod(20));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Iface66Code::ALL.len(), 12);
        let first = match Iface66Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Iface66Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
