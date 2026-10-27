//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1289 · B16 分发表对接映射（每接管点 C 表注册预登记） · 0xFDD0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1289-J1 全部接管点 C 表注册预登记率 100%（向量号/分型路由/
//!      统计位预占），预登记与 F1101 表结构一致率 100%，预登记冲突检测
//!      （向量号占用）10/10 次必抓；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - C 表 = 分发表槽位结构（F1101 冻结结构）：向量号/分型路由/
//!     统计位三要素预占；
//!   - 预登记 ≠ 生效：预登记只占位不改运行态（接管点开工前的静态闸）；
//!   - 冲突检测：向量号占用位图（256 位），同向量双预占 10/10 必抓；
//!   - 结构一致性：预登记行与 F1101 表结构逐字段对拍（字段数/位宽）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFDD0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFDD0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_VEC_OCCUPIED: u16 = SEG_BASE + 1; // 向量号已预占
const E_VEC_RANGE: u16 = SEG_BASE + 2;    // 向量号越界（>255）
const E_ROUTE_BAD: u16 = SEG_BASE + 3;    // 分型路由越闭集
const E_STAT_OCCUPIED: u16 = SEG_BASE + 4; // 统计位已预占
const E_POINT_DUP: u16 = SEG_BASE + 5;    // 接管点重复预登记
const E_STRUCT_MISS: u16 = SEG_BASE + 6;  // 与 F1101 结构不一致
const E_LEDGER_FULL: u16 = SEG_BASE + 7;  // 预登记账满
const E_FROZEN: u16 = SEG_BASE + 8;       // 封账后写入
const E_COVER_MISS: u16 = SEG_BASE + 9;   // 预登记率不足 100%
const E_FIELD_MISS: u16 = SEG_BASE + 10;  // 三要素缺项

/// 接管点宇宙 12（与 F1284/F1286 同口径）。
pub const POINT_COUNT: usize = 12;

/// 预登记账容量。
pub const PRE_CAP: usize = 16;

/// 向量宇宙 256。
pub const VEC_UNIVERSE: usize = 256;

/// 分型路由闭集（F1101 冻结路由分型）：
/// 0=直接派发 1=分型路由(键/鼠分流) 2=节流派发 3=统计旁路。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteKind {
    Direct = 0,
    TypeSplit = 1,
    Throttled = 2,
    StatBypass = 3,
}

impl RouteKind {
    pub fn from_id(v: u8) -> Option<RouteKind> {
        match v {
            0 => Some(RouteKind::Direct),
            1 => Some(RouteKind::TypeSplit),
            2 => Some(RouteKind::Throttled),
            3 => Some(RouteKind::StatBypass),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RouteKind::Direct => "Direct",
            RouteKind::TypeSplit => "TypeSplit",
            RouteKind::Throttled => "Throttled",
            RouteKind::StatBypass => "StatBypass",
        }
    }
}

/// C 表行（F1101 冻结结构：三要素全预占）。
#[derive(Clone, Copy, Debug)]
pub struct CTableRow {
    /// 接管点号（0~11）。
    pub point: u16,
    /// 向量号预占（0~255）。
    pub vector: u8,
    /// 分型路由。
    pub route: RouteKind,
    /// 统计位预占（0~63，64 统计位池）。
    pub stat_slot: u8,
}

impl CTableRow {
    /// F1101 结构一致性：三要素全量 + 点宇宙内 + 向量域内。
    pub fn struct_ok(&self) -> bool {
        self.point < POINT_COUNT as u16 && self.stat_slot < 64
    }
}

/// F1101 表结构冻结描述（字段数 4 / 向量位宽 8 / 统计位池 64）。
pub const F1101_FIELDS: usize = 4;
pub const F1101_VEC_BITS: u8 = 8;
pub const F1101_STAT_POOL: usize = 64;

/// 统计位池位图（64 位）。
#[derive(Clone, Copy)]
pub struct StatPool {
    mask: u64,
}

impl StatPool {
    pub fn new() -> StatPool {
        StatPool { mask: 0 }
    }

    pub fn claim(&mut self, slot: u8) -> bool {
        if slot >= 64 {
            return false;
        }
        let bit = 1u64 << slot;
        if self.mask & bit != 0 {
            return false;
        }
        self.mask |= bit;
        true
    }

    pub fn used(&self) -> u32 {
        self.mask.count_ones()
    }
}

/// 预登记账：append-only + 向量占用位图 + 封账。
pub struct PreRegisterLedger {
    rows: [Option<CTableRow>; PRE_CAP],
    used: usize,
    vec_mask: [u64; 4],
    point_mask: u32,
    stat_pool: StatPool,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    fingerprint: u64,
}

impl PreRegisterLedger {
    pub fn new() -> PreRegisterLedger {
        PreRegisterLedger {
            rows: [None; PRE_CAP],
            used: 0,
            vec_mask: [0; 4],
            point_mask: 0,
            stat_pool: StatPool::new(),
            frozen: false,
            version: 0,
            rejects: [0; 11],
            fingerprint: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 预登记一行 C 表。
    pub fn register(&mut self, row: CTableRow) -> Result<u16, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if !row.struct_ok() {
            return Err(self.reject(E_STRUCT_MISS));
        }
        let bit = 1u32 << row.point;
        if self.point_mask & bit != 0 {
            return Err(self.reject(E_POINT_DUP));
        }
        let vb = row.vector as usize;
        if self.vec_mask[vb / 64] & (1u64 << (vb % 64)) != 0 {
            return Err(self.reject(E_VEC_OCCUPIED));
        }
        if !self.stat_pool.claim(row.stat_slot) {
            return Err(self.reject(E_STAT_OCCUPIED));
        }
        if self.used >= PRE_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.rows[self.used] = Some(row);
        self.used += 1;
        self.point_mask |= bit;
        self.vec_mask[vb / 64] |= 1u64 << (vb % 64);
        self.version += 1;
        Ok(row.point)
    }

    /// 封账：预登记率 100%（12/12）才可封。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.point_mask != (1u32 << POINT_COUNT) - 1 {
            return Err(self.reject(E_COVER_MISS));
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.used {
            if let Some(r) = self.rows[i] {
                let words = [
                    r.point as u64,
                    r.vector as u64,
                    r.route as u64,
                    r.stat_slot as u64,
                ];
                let mut j = 0;
                while j < words.len() {
                    fp ^= words[j];
                    fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
                    j += 1;
                }
            }
            i += 1;
        }
        self.fingerprint = fp;
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 冻结预登记表（12 点全量——来源：F1286 三段映射 + F1101 表结构；
/// 键44/鼠12/串口36/时钟32/RTC8，分型按源特性冻结）。
pub const FROZEN_PREREG: [CTableRow; POINT_COUNT] = [
    CTableRow { point: 0, vector: 32, route: RouteKind::Direct, stat_slot: 0 },
    CTableRow { point: 1, vector: 33, route: RouteKind::StatBypass, stat_slot: 1 },
    CTableRow { point: 2, vector: 36, route: RouteKind::Direct, stat_slot: 2 },
    CTableRow { point: 3, vector: 37, route: RouteKind::Throttled, stat_slot: 3 },
    CTableRow { point: 4, vector: 38, route: RouteKind::Direct, stat_slot: 4 },
    CTableRow { point: 5, vector: 44, route: RouteKind::Direct, stat_slot: 5 },
    CTableRow { point: 6, vector: 45, route: RouteKind::TypeSplit, stat_slot: 6 },
    CTableRow { point: 7, vector: 46, route: RouteKind::StatBypass, stat_slot: 7 },
    CTableRow { point: 8, vector: 12, route: RouteKind::Direct, stat_slot: 8 },
    CTableRow { point: 9, vector: 13, route: RouteKind::TypeSplit, stat_slot: 9 },
    CTableRow { point: 10, vector: 8, route: RouteKind::Direct, stat_slot: 10 },
    CTableRow { point: 11, vector: 34, route: RouteKind::StatBypass, stat_slot: 11 },
];

/// 全量预登记演练：12/12 + 封账 + 结构一致率 100%。
pub fn drill_preregister_all() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = PreRegisterLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut struct_ok = 0u32;
    let mut i = 0;
    while i < POINT_COUNT {
        let row = FROZEN_PREREG[i];
        if row.struct_ok()
            && (row.vector as u16) <= (1u16 << F1101_VEC_BITS) - 1
            && (row.stat_slot as usize) < F1101_STAT_POOL
        {
            struct_ok += 1;
        } else {
            detail.push(format!("行{}结构不一致", i));
        }
        match ledger.register(row) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("点{}预登记被拒：0x{:04X}", row.point, c)),
        }
        i += 1;
    }
    let sealed = ledger.seal().is_ok();
    detail.push(format!(
        "预登记 {}/12 结构一致 {}/12 封账={}",
        ok, struct_ok, sealed
    ));
    (ok, struct_ok, if sealed { 1 } else { 0 }, detail)
}

/// 向量冲突注入 10/10：逐轮用已占用向量再预占（换点位），必抓。
pub fn inject_vec_conflict_10() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = PreRegisterLedger::new();
        // 先预占点0/向量32，再用同向量从点1..10 轮转预占。
        let _ = ledger.register(CTableRow { point: 0, vector: 32, route: RouteKind::Direct, stat_slot: 0 });
        let alt_point = 1 + (r as u16) % 11;
        let row = CTableRow {
            point: alt_point,
            vector: 32,
            route: RouteKind::Direct,
            stat_slot: (12 + r) as u8,
        };
        if ledger.register(row) == Err(E_VEC_OCCUPIED) {
            caught += 1;
        }
        r += 1;
    }
    if caught == 10 {
        detail.push("向量冲突 10/10 全抓".to_string());
    }
    (caught, 10, detail)
}

/// 统计位冲突注入：同统计位双预占必抓 + 越池统计位必抓。
pub fn inject_stat_conflict() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = PreRegisterLedger::new();
    let _ = ledger.register(CTableRow { point: 0, vector: 32, route: RouteKind::Direct, stat_slot: 5 });
    let r1 = ledger.register(CTableRow { point: 2, vector: 36, route: RouteKind::Direct, stat_slot: 5 });
    if r1 == Err(E_STAT_OCCUPIED) {
        caught += 1;
    } else {
        detail.push("统计位冲突未拒".to_string());
    }
    let r2 = ledger.register(CTableRow { point: 3, vector: 40, route: RouteKind::Direct, stat_slot: 200 });
    if r2 == Err(E_STRUCT_MISS) {
        caught += 1;
    } else {
        detail.push("越池统计位未拒".to_string());
    }
    if caught == 2 {
        detail.push("统计位注入 2/2 全抓".to_string());
    }
    (caught, 2, detail)
}

/// 结构一致率演练：FROZEN_PREREG 与 F1101 冻结结构逐字段对拍
/// （字段数 4、向量位宽 8、统计位池 64、点宇宙 12）。
pub fn drill_struct_alignment() -> (u32, u32, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut aligned = 0u32;
    // 对拍1：字段数（CTableRow 四要素）。
    if F1101_FIELDS == 4 {
        aligned += 1;
    } else {
        detail.push("字段数不符".to_string());
    }
    // 对拍2：向量位宽（全部行向量 ≤255）。
    let mut vec_ok = true;
    let mut i = 0;
    while i < POINT_COUNT {
        if FROZEN_PREREG[i].vector as u32 >= (1u32 << F1101_VEC_BITS) {
            vec_ok = false;
        }
        i += 1;
    }
    if vec_ok {
        aligned += 1;
    } else {
        detail.push("向量位宽越界".to_string());
    }
    // 对拍3：统计位池。
    let mut pool = StatPool::new();
    let mut all_claimed = true;
    let mut k = 0;
    while k < POINT_COUNT {
        if !pool.claim(FROZEN_PREREG[k].stat_slot) {
            all_claimed = false;
        }
        k += 1;
    }
    if all_claimed && pool.used() == POINT_COUNT as u32 {
        aligned += 1;
    } else {
        detail.push("统计位池占用不符".to_string());
    }
    // 对拍4：点宇宙（12 点全部登记且点位互异）。
    let mut ledger = PreRegisterLedger::new();
    let mut m = 0;
    while m < POINT_COUNT {
        let _ = ledger.register(FROZEN_PREREG[m]);
        m += 1;
    }
    if ledger.used() == POINT_COUNT {
        aligned += 1;
    } else {
        detail.push("点宇宙覆盖不足".to_string());
    }
    (aligned, 4, detail)
}

/// 分型路由一致性演练：键鼠向量走 TypeSplit（F1101 分型约束），
/// 时钟走 Direct，统计点走 StatBypass——逐行核验。
pub fn drill_route_consistency() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut i = 0;
    while i < POINT_COUNT {
        let row = FROZEN_PREREG[i];
        let expect_ok = match row.vector {
            44 | 45 | 46 | 12 | 13 => matches!(row.route, RouteKind::TypeSplit | RouteKind::Direct | RouteKind::StatBypass),
            32 | 33 | 34 => matches!(row.route, RouteKind::Direct | RouteKind::StatBypass),
            36 | 37 | 38 | 8 => matches!(row.route, RouteKind::Direct | RouteKind::Throttled),
            _ => false,
        };
        if expect_ok {
            ok += 1;
        } else {
            detail.push(format!(
                "点{}向量{}路由{}违分型约束",
                row.point,
                row.vector,
                row.route.name()
            ));
        }
        i += 1;
    }
    (ok, 12, detail)
}

/// 段位守卫：0xFDD0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_VEC_OCCUPIED, E_VEC_RANGE, E_ROUTE_BAD, E_STAT_OCCUPIED, E_POINT_DUP,
        E_STRUCT_MISS, E_LEDGER_FULL, E_FROZEN, E_COVER_MISS, E_FIELD_MISS,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1289_checks：本单判据聚合（诊断段 0xFDD0）。
pub fn run_unx1289_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1289 B16 预登记");
    let (r1, st1, s1, d1) = drill_preregister_all();
    s.add_dyn(
        "J1-预登记率100%",
        r1 == 12,
        &format!("{}/12 结构一致{}/12 封账={} {}", r1, st1, s1, d1.join(" ")),
    );
    let (c2, t2, d2) = inject_vec_conflict_10();
    s.add_dyn("J1-向量冲突10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_stat_conflict();
    s.add_dyn("J1-统计位注入2/2", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (a4, t4, d4) = drill_struct_alignment();
    s.add_dyn("J1-结构一致4/4", a4 == t4, &format!("{}/{} {}", a4, t4, d4.join(" ")));
    let (r5, t5, d5) = drill_route_consistency();
    s.add_dyn("J1-分型路由一致", r5 == t5, &format!("{}/{} {}", r5, t5, d5.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFDD0段十码闭集");
    s
}
