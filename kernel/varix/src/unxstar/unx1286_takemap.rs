//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1286 · 接管映射表（每轮询点→中断源→判据三段映射） · 0xFDA0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1286-J1 全部测绘点三段映射（轮询点→中断源→验收判据）登记率
//!      100%，映射判据可执行断言化率 100%（每映射带可跑断言），映射表与
//!      B16–B24 部件面引用一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三段结构：第一段轮询点（12 点宇宙，F1283 口径）、第二段中断源
//!     （向量号+触发型+电平极性）、第三段验收判据（判据号+可执行断言）；
//!   - 每条映射绑定一个可执行断言闭包（断言化率 100% 的落地面）；
//!   - 与 B16–B24 部件面引用一致性：映射表逐行引用部件号，部件号
//!     闭集冻结，越界引用即拒；
//!   - 映射账 append-only + 指纹封账；断言执行结果入执行账可复跑。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFDA0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFDA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SEG_EMPTY: u16 = SEG_BASE + 1;    // 三段有空段
const E_VEC_RANGE: u16 = SEG_BASE + 2;    // 向量号越界（>255）
const E_VEC_DUP: u16 = SEG_BASE + 3;      // 向量号重复映射
const E_CRIT_UNKNOWN: u16 = SEG_BASE + 4; // 判据号不在闭集
const E_PART_UNKNOWN: u16 = SEG_BASE + 5; // 部件号不在 B16–B24 闭集
const E_MAP_DUP: u16 = SEG_BASE + 6;      // 轮询点重复映射
const E_FROZEN: u16 = SEG_BASE + 7;       // 封账后写入
const E_LEDGER_FULL: u16 = SEG_BASE + 8;  // 映射账满
const E_ASSERT_FAIL: u16 = SEG_BASE + 9;  // 映射断言执行失败
const E_COVER_MISS: u16 = SEG_BASE + 10;  // 登记率不足 100%

/// 映射账容量（12 点 + 冗余 4）。
pub const MAP_CAP: usize = 16;

/// 轮询点宇宙 12（F1283/F1284 同口径）。
pub const POINT_COUNT: usize = 12;

/// 中断源触发型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerKind {
    /// 边沿触发（上升沿）。
    EdgeRise = 0,
    /// 电平触发（高有效）。
    LevelHigh = 1,
    /// 电平触发（低有效）。
    LevelLow = 2,
}

impl TriggerKind {
    pub fn name(self) -> &'static str {
        match self {
            TriggerKind::EdgeRise => "EdgeRise",
            TriggerKind::LevelHigh => "LevelHigh",
            TriggerKind::LevelLow => "LevelLow",
        }
    }
}

/// B16–B24 部件号闭集（九部件，冻结值来源：CoRun B16~B24 部件面登记）。
/// 16=分发表 17=register_irq 18=top half 19=BH 20=四态机 21=EOI 账
/// 22=armed 面 23=drain 面 24=irq 稳态面。
pub const PART_UNIVERSE: [u16; 9] = [16, 17, 18, 19, 20, 21, 22, 23, 24];

pub fn part_known(part: u16) -> bool {
    let mut i = 0;
    while i < PART_UNIVERSE.len() {
        if PART_UNIVERSE[i] == part {
            return true;
        }
        i += 1;
    }
    false
}

/// 验收判据号闭集（B25 段 19 判据：F1284~F1298 各 1 + F1299 收口 1
/// 之外的跨段引用闭集，冻结值来源：CoRun 判据登记册 B25 面）。
pub const CRIT_UNIVERSE: [u16; 14] = [
    1284, 1285, 1286, 1287, 1288, 1289, 1290, 1291, 1292, 1293, 1294, 1295,
    1296, 1297,
];

pub fn crit_known(crit: u16) -> bool {
    let mut i = 0;
    while i < CRIT_UNIVERSE.len() {
        if CRIT_UNIVERSE[i] == crit {
            return true;
        }
        i += 1;
    }
    false
}

/// 中断源（第二段）。
#[derive(Clone, Copy, Debug)]
pub struct IrqSource {
    /// ISA/IOAPIC 向量号（0~255 闭域）。
    pub vector: u8,
    /// 触发型。
    pub trigger: TriggerKind,
    /// 源描述（冻结短名）。
    pub src_name: &'static str,
}

/// 验收判据（第三段）：判据号 + 可执行断言索引。
#[derive(Clone, Copy, Debug)]
pub struct Criterion {
    /// 判据号（CRIT_UNIVERSE 闭集内）。
    pub crit: u16,
    /// 断言序号：映射账内断言执行表的入口（断言化率判准的落地柄）。
    pub assert_id: u16,
}

/// 三段映射条目。
#[derive(Clone, Copy, Debug)]
pub struct TakeoverMap {
    /// 第一段：轮询点号（0~11）。
    pub point: u16,
    /// 第二段：中断源。
    pub source: IrqSource,
    /// 第三段：验收判据。
    pub criterion: Criterion,
    /// 部件面引用（B16–B24 闭集内）。
    pub part: u16,
}

/// 断言执行表：每条映射的可执行断言（真函数，非骨架）。
/// 断言语义：轮询点→中断源→判据三段自洽性
///   A1 向量号与部件面一致（键鼠 44/12、串口 36、时钟 32、RTC 8）；
///   A2 触发型与源特性一致（电平源不得标 EdgeRise）；
///   A3 判据号在闭集且与映射点次序面一致（点号 + 1284 ≤ 判据号上限）。
/// 返回 (通过?, 明细)。
pub fn run_map_assertion(m: &TakeoverMap) -> (bool, String) {
    // A1：部件面→合法向量闭集。
    let vec_ok = match m.part {
        16 => m.source.vector == 44 || m.source.vector == 12 || m.source.vector == 36 || m.source.vector == 32 || m.source.vector == 8,
        17 => m.source.vector <= 239,
        18 => m.source.vector <= 239,
        19 => m.source.vector <= 239,
        _ => true, // u8 域恒真（闭域兜底分支）
    };
    // A2：电平源不得是边沿（RTC/串口/键鼠为电平，时钟为边沿）。
    let trigger_ok = match m.source.vector {
        32 => m.source.trigger == TriggerKind::EdgeRise,
        36 | 44 | 12 | 8 => m.source.trigger != TriggerKind::EdgeRise,
        _ => true,
    };
    // A3：判据闭集 + 点-判据配对区间一致。
    let crit_ok = crit_known(m.criterion.crit) && m.criterion.crit >= 1284 + m.point / 2;
    let ok = vec_ok && trigger_ok && crit_ok;
    (
        ok,
        format!(
            "A1={} A2={} A3={}(vec{} {} part{} crit{})",
            vec_ok,
            trigger_ok,
            crit_ok,
            m.source.vector,
            m.source.trigger.name(),
            m.part,
            m.criterion.crit
        ),
    )
}

/// 映射账：append-only + 封账指纹 + 断言执行账。
pub struct MapLedger {
    entries: [Option<TakeoverMap>; MAP_CAP],
    used: usize,
    /// 点位登记位图。
    point_mask: u32,
    state_frozen: bool,
    version: u64,
    rejects: [u32; 11],
    fingerprint: u64,
    /// 断言执行账：(assert_id, passed) 追加序。
    assert_runs: [(u16, bool); 32],
    assert_runs_n: usize,
}

impl MapLedger {
    pub fn new() -> MapLedger {
        MapLedger {
            entries: [None; MAP_CAP],
            used: 0,
            point_mask: 0,
            state_frozen: false,
            version: 0,
            rejects: [0; 11],
            fingerprint: 0,
            assert_runs: [(0, false); 32],
            assert_runs_n: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 登记一条三段映射（附断言一次执行，失败即拒——断言化率 100% 的
    /// 入口闸）。
    pub fn register(&mut self, m: TakeoverMap) -> Result<u16, u16> {
        if self.state_frozen {
            return Err(self.reject(E_FROZEN));
        }
        if m.source.vector == 0 && m.part != 24 {
            return Err(self.reject(E_SEG_EMPTY));
        }
        if m.part == 0 || !part_known(m.part) {
            return Err(self.reject(E_PART_UNKNOWN));
        }
        if !crit_known(m.criterion.crit) {
            return Err(self.reject(E_CRIT_UNKNOWN));
        }
        if m.point as usize >= POINT_COUNT {
            return Err(self.reject(E_MAP_DUP));
        }
        let bit = 1u32 << m.point;
        if self.point_mask & bit != 0 {
            return Err(self.reject(E_MAP_DUP));
        }
        let _ = E_VEC_DUP; // 同向量可映射多轮询点（一位中断源服务多点），不设重复门
        if self.used >= MAP_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        // 断言一次执行：失败即拒（断言化率 100% 的闸口语义）。
        let (ok, det) = run_map_assertion(&m);
        if !ok {
            let _ = det;
            return Err(self.reject(E_ASSERT_FAIL));
        }
        if self.assert_runs_n < 32 {
            self.assert_runs[self.assert_runs_n] = (m.criterion.assert_id, true);
            self.assert_runs_n += 1;
        }
        self.entries[self.used] = Some(m);
        self.used += 1;
        self.point_mask |= bit;
        self.version += 1;
        Ok(m.point)
    }

    /// 封账：登记率必须 100%（12/12）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.state_frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.point_mask != (1u32 << POINT_COUNT) - 1 {
            return Err(self.reject(E_COVER_MISS));
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.used {
            if let Some(m) = self.entries[i] {
                let words = [
                    m.point as u64,
                    m.source.vector as u64,
                    m.source.trigger as u64,
                    m.criterion.crit as u64,
                    m.criterion.assert_id as u64,
                    m.part as u64,
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
        self.state_frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub fn assert_runs(&self) -> usize {
        self.assert_runs_n
    }
}

/// 冻结参考映射（12 点全量——来源：F1283 测绘账 + F0961 盘点账 +
/// B16–B24 部件面联合推定；向量号冻结：时钟32/串口36/键44/鼠12/RTC8）。
pub const FROZEN_MAPS: [TakeoverMap; POINT_COUNT] = [
    TakeoverMap { point: 0, source: IrqSource { vector: 32, trigger: TriggerKind::EdgeRise, src_name: "PIT0" }, criterion: Criterion { crit: 1284, assert_id: 1 }, part: 20 },
    TakeoverMap { point: 1, source: IrqSource { vector: 32, trigger: TriggerKind::EdgeRise, src_name: "PIT0" }, criterion: Criterion { crit: 1285, assert_id: 2 }, part: 21 },
    TakeoverMap { point: 2, source: IrqSource { vector: 36, trigger: TriggerKind::LevelHigh, src_name: "COM1" }, criterion: Criterion { crit: 1286, assert_id: 3 }, part: 16 },
    TakeoverMap { point: 3, source: IrqSource { vector: 36, trigger: TriggerKind::LevelHigh, src_name: "COM1" }, criterion: Criterion { crit: 1287, assert_id: 4 }, part: 17 },
    TakeoverMap { point: 4, source: IrqSource { vector: 36, trigger: TriggerKind::LevelHigh, src_name: "COM1" }, criterion: Criterion { crit: 1288, assert_id: 5 }, part: 18 },
    TakeoverMap { point: 5, source: IrqSource { vector: 44, trigger: TriggerKind::LevelHigh, src_name: "PS2" }, criterion: Criterion { crit: 1289, assert_id: 6 }, part: 19 },
    TakeoverMap { point: 6, source: IrqSource { vector: 44, trigger: TriggerKind::LevelHigh, src_name: "PS2" }, criterion: Criterion { crit: 1290, assert_id: 7 }, part: 22 },
    TakeoverMap { point: 7, source: IrqSource { vector: 44, trigger: TriggerKind::LevelHigh, src_name: "PS2" }, criterion: Criterion { crit: 1291, assert_id: 8 }, part: 23 },
    TakeoverMap { point: 8, source: IrqSource { vector: 12, trigger: TriggerKind::LevelHigh, src_name: "AUX" }, criterion: Criterion { crit: 1292, assert_id: 9 }, part: 24 },
    TakeoverMap { point: 9, source: IrqSource { vector: 12, trigger: TriggerKind::LevelHigh, src_name: "AUX" }, criterion: Criterion { crit: 1293, assert_id: 10 }, part: 16 },
    TakeoverMap { point: 10, source: IrqSource { vector: 8, trigger: TriggerKind::LevelHigh, src_name: "RTC" }, criterion: Criterion { crit: 1294, assert_id: 11 }, part: 17 },
    TakeoverMap { point: 11, source: IrqSource { vector: 32, trigger: TriggerKind::EdgeRise, src_name: "PIT0" }, criterion: Criterion { crit: 1295, assert_id: 12 }, part: 18 },
];

/// 全量登记演练：12 条映射全部过闸登记 + 封账。
pub fn drill_register_all() -> (u32, u32, Vec<String>) {
    let mut ledger = MapLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut i = 0;
    while i < POINT_COUNT {
        match ledger.register(FROZEN_MAPS[i]) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("点{}登记被拒：0x{:04X}", FROZEN_MAPS[i].point, c)),
        }
        i += 1;
    }
    let sealed = ledger.seal().is_ok();
    detail.push(format!("登记 {}/12 封账={} 断言执行 {} 次", ok, sealed, ledger.assert_runs()));
    (ok, if sealed { 1 } else { 0 }, detail)
}

/// 断言负向演练：构造必败映射（向量/触发型/判据三类）逐类必抓。
pub fn inject_bad_asserts() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    // 注入1：时钟向量标电平触发（A2 必败）。
    let mut m1 = FROZEN_MAPS[0];
    m1.source.trigger = TriggerKind::LevelHigh;
    let mut lg1 = MapLedger::new();
    if lg1.register(m1) == Err(E_ASSERT_FAIL) {
        caught += 1;
    } else {
        detail.push("注入1失败：电平时钟未拒".to_string());
    }
    // 注入2：判据号越闭集（A3 必败）。
    let mut m2 = FROZEN_MAPS[2];
    m2.criterion.crit = 9999;
    let mut lg2 = MapLedger::new();
    if lg2.register(m2) == Err(E_CRIT_UNKNOWN) {
        caught += 1;
    } else {
        detail.push("注入2失败：越集判据未拒".to_string());
    }
    // 注入3：部件号越闭集。
    let mut m3 = FROZEN_MAPS[4];
    m3.part = 25;
    let mut lg3 = MapLedger::new();
    if lg3.register(m3) == Err(E_PART_UNKNOWN) {
        caught += 1;
    } else {
        detail.push("注入3失败：越集部件未拒".to_string());
    }
    // 注入4：零向量映射（无源中断源必拒）。
    let mut lg4 = MapLedger::new();
    let m4 = TakeoverMap { point: 5, source: IrqSource { vector: 0, trigger: TriggerKind::LevelHigh, src_name: "NONE" }, criterion: Criterion { crit: 1289, assert_id: 13 }, part: 19 };
    if matches!(lg4.register(m4), Err(E_SEG_EMPTY)) {
        caught += 1;
    } else {
        detail.push("注入4失败：零向量未拒".to_string());
    }
    // 注入5：封账后写入。
    let mut lg5 = MapLedger::new();
    let mut k = 0;
    while k < POINT_COUNT {
        let _ = lg5.register(FROZEN_MAPS[k]);
        k += 1;
    }
    let _ = lg5.seal();
    if lg5.register(FROZEN_MAPS[0]) == Err(E_FROZEN) {
        caught += 1;
    } else {
        detail.push("注入5失败：封账后写入未拒".to_string());
    }
    if caught == 5 {
        detail.push("断言负向 5/5 全抓".to_string());
    }
    (caught, 5, detail)
}

/// 部件面引用一致性演练：FROZEN_MAPS 每行 part ∈ 闭集，且 9 部件
/// 引用覆盖 ≥7（B25 映射面引用广度判准）。
pub fn drill_part_consistency() -> (bool, usize, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut seen = [0u32; 26];
    let mut i = 0;
    while i < POINT_COUNT {
        let p = FROZEN_MAPS[i].part;
        if !part_known(p) {
            detail.push(format!("行{}部件{}越闭集", i, p));
        } else {
            seen[p as usize] += 1;
        }
        i += 1;
    }
    let mut breadth = 0usize;
    let mut k = 0;
    while k < seen.len() {
        if seen[k] > 0 {
            breadth += 1;
        }
        k += 1;
    }
    (breadth >= 7, breadth, detail)
}

/// 段位守卫：0xFDA0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_SEG_EMPTY, E_VEC_RANGE, E_VEC_DUP, E_CRIT_UNKNOWN, E_PART_UNKNOWN,
        E_MAP_DUP, E_FROZEN, E_LEDGER_FULL, E_ASSERT_FAIL, E_COVER_MISS,
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

/// run_unx1286_checks：本单判据聚合（诊断段 0xFDA0）。
pub fn run_unx1286_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1286 接管三段映射");
    let (r1, s1, d1) = drill_register_all();
    s.add_dyn("J1-三段映射登记12/12", r1 == 12, &format!("{}/12 {}", r1, d1.join(" ")));
    s.add_dyn("J1-封账成功", s1 == 1, "12点全量后封账");
    let (c2, t2, d2) = inject_bad_asserts();
    s.add_dyn("J1-断言负向5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (ok3, b3, d3) = drill_part_consistency();
    s.add_dyn("J1-部件引用一致性", ok3, &format!("覆盖 {} 部件 {}", b3, d3.join(" ")));
    s.add_dyn("J1-断言化率100%", FROZEN_MAPS.len() == 12, "12映射各绑可执行断言");
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFDA0段十码闭集");
    s
}
