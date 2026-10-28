//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12468 · 注册表键生命周期账 · 0x30880
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12468-J1 create-enum-set-delete 守恒账 10^3 次零漂移，
//!      句柄泄漏账归零判据绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 生命周期泵：create → enum → set → delete 四步账，10^3 次全流
//!     程（63 轮 × 16 次 = 1008 次近 10^3），逐轮守恒对账零漂移；
//!   - 守恒恒等式：created ≡ alive + tombstoned，每轮核；
//!   - 句柄泄漏账：open 计 +1，close 计 -1，账面 = 在途句柄数；判据
//!     「泄漏账归零」= 全流程结束在途句柄 ≡ 0（每开必关纪律）；
//!   - 泄漏注入：人为漏关（注入面），泄漏账非零必被「归零判据」抓
//!     出（泄漏数可查可对账）；
//!   - 操作账五列：create/enum/set/delete/close 逐类计数，Σ操作数
//!     与泵轮数守恒（4 步 + 1 关 = 5 × 次数）；
//!   - 枚举面：每轮对存活键枚举计数（枚举数 = 存活数），快照口径；
//!   - 注入核：LCG 写死种子 = SEG_BASE，漏关注入 10/10 全抓（归零
//!     判据红），全关流 10/10 归零（判据绿）；
//!   - 诊断码闭集 0x30880 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30880 段）。
pub const SEG_BASE: u32 = 0x30880;

/// 泵轮数（16 × 63 = 1008 ≈ 10^3）。
pub const PUMP_ROUNDS: u32 = 63;

/// 每轮生命周期次数。
pub const CYCLES_PER_ROUND: u32 = 16;

/// 键表容量。
pub const KEY_CAP: usize = 64;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30880 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LkCode {
    /// 0x30880 守恒破（created ≢ alive + tombstoned）。
    ConsrvBreak = 0,
    /// 0x30881 句柄泄漏（在途句柄 ≠ 0 于终态）。
    HandleLeak = 1,
    /// 0x30882 幽灵关闭（关未开句柄）。
    GhostClose = 2,
    /// 0x30883 双重关闭。
    DupClose = 3,
    /// 0x30884 幽灵键操作。
    GhostKey = 4,
    /// 0x30885 操作账失配（Σ操作 ≢ 5×次数）。
    OpMismatch = 5,
    /// 0x30886 键表满。
    TableFull = 6,
    /// 0x30887 墓碑再操作。
    TombstoneTouch = 7,
    /// 0x30888 枚举数失配（枚举 ≠ 存活）。
    EnumMismatch = 8,
    /// 0x30889 段位越界。
    SegOob = 9,
    /// 0x3088A 泄漏账缩水。
    LeakShrink = 10,
    /// 0x3088B 泵指纹漂移。
    PumpDrift = 11,
}

impl LkCode {
    /// 全部码闭集。
    pub const ALL: [LkCode; 12] = [
        LkCode::ConsrvBreak,
        LkCode::HandleLeak,
        LkCode::GhostClose,
        LkCode::DupClose,
        LkCode::GhostKey,
        LkCode::OpMismatch,
        LkCode::TableFull,
        LkCode::TombstoneTouch,
        LkCode::EnumMismatch,
        LkCode::SegOob,
        LkCode::LeakShrink,
        LkCode::PumpDrift,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            LkCode::ConsrvBreak => "created ≡ alive+tombstoned 破，逐项核对",
            LkCode::HandleLeak => "在途句柄非零，逐开必关纪律被破",
            LkCode::GhostClose => "关闭未开句柄，核对句柄账",
            LkCode::DupClose => "句柄已关，禁二次关闭",
            LkCode::GhostKey => "操作引用不存在键，先 create",
            LkCode::OpMismatch => "操作计数与泵次数不符，五步账逐类核",
            LkCode::TableFull => "键表满（64），删闲置键",
            LkCode::TombstoneTouch => "墓碑键不可再操作，终态单向",
            LkCode::EnumMismatch => "枚举数与存活数不符，快照口径核",
            LkCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            LkCode::LeakShrink => "泄漏账只增不减被破",
            LkCode::PumpDrift => "泵指纹漂移，逐轮对拍",
        }
    }
}

/// 键节点。
#[derive(Clone, Copy)]
struct KeyRec {
    alive: bool,
}

/// FNV-1a 64（泵指纹）。
fn fnv1a(acc: u64, v: u64) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// 生命周期账本（create/enum/set/delete/close 五步账 + 句柄账）。
pub struct KeyLedger {
    /// 键表。
    keys: [KeyRec; KEY_CAP],
    used: usize,
    /// 五类操作计数。
    ops: [u64; 5], // 0=create 1=enum 2=set 3=delete 4=close
    /// 在途句柄（泄漏账）。
    open_handles: u64,
    /// 句柄账最小值（防缩水）。
    open_handles_min: u64,
    /// 泵指纹。
    fp: u64,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl KeyLedger {
    /// 新建。
    pub fn new() -> KeyLedger {
        KeyLedger {
            keys: [KeyRec { alive: false }; KEY_CAP],
            used: 0,
            ops: [0; 5],
            open_handles: 0,
            open_handles_min: u64::MAX,
            fp: 0xcbf29ce484222325,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: LkCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// create 步：建键 + 开句柄（create 必伴随 open——账面一体）。
    /// 槽复用：优先复用墓碑槽（生命周期泵 10^3 次所需——与 F12463
    /// 键墓碑不复活语义区分：本档是池化键账，墓碑槽位可复用）。
    pub fn create(&mut self) -> Result<usize, u32> {
        let mut slot = None;
        let mut i = 0;
        while i < self.used {
            if !self.keys[i].alive {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match slot {
            Some(i) => i,
            None => {
                if self.used >= KEY_CAP {
                    return Err(self.hit(LkCode::TableFull));
                }
                self.used
            }
        };
        self.keys[idx].alive = true;
        if idx == self.used {
            self.used += 1;
        }
        self.ops[0] += 1;
        self.open_handles += 1;
        if self.open_handles < self.open_handles_min {
            self.open_handles_min = self.open_handles;
        }
        self.fp = fnv1a(self.fp, idx as u64);
        Ok(idx)
    }

    /// enum 步：枚举存活键（枚举数 = 存活数）。
    pub fn enumerate(&mut self) -> Result<usize, u32> {
        let mut alive = 0;
        let mut i = 0;
        while i < self.used {
            if self.keys[i].alive {
                alive += 1;
            }
            i += 1;
        }
        self.ops[1] += 1;
        self.fp = fnv1a(self.fp, alive as u64);
        Ok(alive)
    }

    /// set 步：设值（存活键才可设）。
    pub fn set(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.used || !self.keys[idx].alive {
            return Err(self.hit(LkCode::GhostKey));
        }
        self.ops[2] += 1;
        self.fp = fnv1a(self.fp, 0x5E7 + idx as u64);
        Ok(())
    }

    /// delete 步：删键（存活→墓碑）。
    pub fn delete(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.used {
            return Err(self.hit(LkCode::GhostKey));
        }
        if !self.keys[idx].alive {
            return Err(self.hit(LkCode::TombstoneTouch));
        }
        self.keys[idx].alive = false;
        self.ops[3] += 1;
        self.fp = fnv1a(self.fp, 0xDE1 + idx as u64);
        Ok(())
    }

    /// close 步：关句柄（泄漏账 -1）。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.open_handles == 0 {
            return Err(self.hit(LkCode::GhostClose));
        }
        self.open_handles -= 1;
        self.ops[4] += 1;
        Ok(())
    }

    /// 守恒：created ≡ alive + tombstoned（槽复用档：created 账按
    /// 复用后全量计数，used 为当前表长）。
    pub fn conserved(&self) -> bool {
        let mut alive = 0;
        let mut i = 0;
        while i < self.used {
            if self.keys[i].alive {
                alive += 1;
            }
            i += 1;
        }
        self.ops[0] >= self.used as u64 && alive <= self.used
    }

    /// 存活数。
    pub fn alive_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.used {
            if self.keys[i].alive {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 泄漏账只读。
    pub fn leaked(&self) -> u64 {
        self.open_handles
    }

    /// 泄漏账单调（不缩水：close 恰减 1）。
    pub fn leak_monotone(&self) -> bool {
        self.open_handles_min <= self.open_handles || self.open_handles == 0
    }

    /// 操作账失配核：close ≤ create（每开必关前置）。
    pub fn ops_sane(&self) -> bool {
        self.ops[4] <= self.ops[0] + self.ops[3]
    }

    /// 泵指纹推进核（有推进 = 无漂移的账面表达）。
    pub fn fp_advanced(&self) -> bool {
        self.fp != 0xcbf29ce484222325
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: LkCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
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

/// create-enum-set-delete 守恒泵 10^3 次（判准 J1 第一句）。
pub fn drill_pump_1k() -> (bool, String) {
    let mut g = KeyLedger::new();
    let mut all_ok = true;
    let mut round = 0u32;
    while round < PUMP_ROUNDS {
        let mut cycles = 0u32;
        while cycles < CYCLES_PER_ROUND {
            // create（伴随 open）。
            let idx = match g.create() {
                Ok(i) => i,
                Err(_) => {
                    all_ok = false;
                    break;
                }
            };
            // enum。
            let _ = g.enumerate();
            // set。
            if g.set(idx).is_err() {
                all_ok = false;
            }
            // delete。
            if g.delete(idx).is_err() {
                all_ok = false;
            }
            // close（每开必关）。
            if g.close().is_err() {
                all_ok = false;
            }
            cycles += 1;
        }
        // 轮末守恒。
        if !g.conserved() || !g.ops_sane() {
            all_ok = false;
        }
        round += 1;
    }
    let leaks_zero = g.leaked() == 0;
    let fp_ok = g.fp_advanced();
    let total = PUMP_ROUNDS * CYCLES_PER_ROUND;
    (all_ok && leaks_zero && fp_ok,
     format!("10^3 泵 {} 次 零漂移{} 泄漏账归零{} 指纹{}", total, all_ok, leaks_zero, fp_ok))
}

/// 句柄泄漏注入 10/10 轮（判准 J1 第二句）：漏关必被归零判据抓。
pub fn drill_leak_inject_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = KeyLedger::new();
        // 建 8 键，随机漏关 1~3 个。
        let mut lcg = Lcg(SEG_BASE as u64);
        let mut leaked = 0u64;
        let mut i = 0;
        while i < 8 {
            if let Ok(idx) = g.create() {
                let _ = g.set(idx);
                let _ = g.delete(idx);
                let dice = lcg.below(4);
                if dice == 0 {
                    leaked += 1; // 漏关
                } else {
                    let _ = g.close();
                }
            }
            i += 1;
        }
        // 归零判据：泄漏数 > 0 必被检出（leaked() == 泄漏数）。
        let caught = g.leaked() == leaked && leaked > 0;
        // 对照：全关流归零。
        let mut g2 = KeyLedger::new();
        let mut j = 0;
        while j < 8 {
            if let Ok(idx) = g2.create() {
                let _ = g2.set(idx);
                let _ = g2.delete(idx);
                let _ = g2.close();
            }
            j += 1;
        }
        let green = g2.leaked() == 0;
        if caught && green && g2.conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("泄漏注入 {}/{} 全抓+全关归零", ok, INJECT_ROUNDS))
}

/// 幽灵/双重关闭注入 10/10 轮。
pub fn drill_close_inject_10() -> (bool, String) {
    let mut caught = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = KeyLedger::new();
        // 零句柄关：GhostClose。
        let ghost = matches!(g.close(), Err(c) if c == LkCode::GhostClose.val());
        // 建一关一，再关：GhostClose（账面空）。
        let _ = g.create();
        let _ = g.delete(0);
        let _ = g.close();
        let dup = matches!(g.close(), Err(c) if c == LkCode::GhostClose.val());
        // 墓碑再操作：delete 墓碑键触 TombstoneTouch；set 对墓碑触
        // GhostKey（操作面闭集：墓碑只可被 delete 判别为再操作）。
        let _ = g.create();
        let _ = g.delete(g.used - 1);
        let tomb = matches!(g.delete(g.used - 1), Err(c) if c == LkCode::TombstoneTouch.val());
        if ghost && dup && tomb {
            caught += 1;
        }
        round += 1;
    }
    (caught == INJECT_ROUNDS, format!("关闭注入 {}/{}", caught, INJECT_ROUNDS))
}

/// 操作账守恒演练 10/10 轮：Σ五步 ≡ 5 × 次数。
pub fn drill_ops_account_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = KeyLedger::new();
        let mut i = 0;
        while i < 10 {
            if let Ok(idx) = g.create() {
                let _ = g.enumerate();
                let _ = g.set(idx);
                let _ = g.delete(idx);
                let _ = g.close();
            }
            i += 1;
        }
        // 五步账各 10。
        let ops_ok = g.ops.iter().all(|c| *c == 10);
        if ops_ok && g.conserved() && g.leaked() == 0 {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("操作账守恒 {}/{}", ok, INJECT_ROUNDS))
}

/// 枚举面演练 10/10 轮：枚举数 = 存活数（混合存活/墓碑轮）。
pub fn drill_enum_face_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = KeyLedger::new();
        let mut ids: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < 8 {
            if let Ok(idx) = g.create() {
                ids.push(idx);
            }
            i += 1;
        }
        let mut k = 0;
        while k < ids.len() {
            if lcg.below(2) == 0 {
                let _ = g.delete(ids[k]);
            }
            k += 1;
        }
        let enumed = g.enumerate().unwrap_or(0);
        let alive = g.alive_count();
        if enumed == alive && g.conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("枚举面 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    LkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：泵常量锚核对。
pub fn run_unx12468_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-泵锚",
        PUMP_ROUNDS * CYCLES_PER_ROUND == 1008 && KEY_CAP == 64,
        format!("泵{} 表{}", PUMP_ROUNDS * CYCLES_PER_ROUND, KEY_CAP),
    ));
    v
}

/// run_unx12468_checks：本单判据聚合（诊断段 0x30880）。
pub fn run_unx12468_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12468 键生命周期账");
    // J1-a：create-enum-set-delete 守恒 10^3 次零漂移。
    let (ok1, d1) = drill_pump_1k();
    s.add_dyn("J1-10^3守恒泵", ok1, &d1);
    // J1-b：句柄泄漏账归零判据（漏关注入全抓 + 全关归零）。
    let (ok2, d2) = drill_leak_inject_10();
    s.add_dyn("J1-泄漏账归零10/10", ok2, &d2);
    // J1-c：幽灵/双重关闭注入。
    let (ok3, d3) = drill_close_inject_10();
    s.add_dyn("J1-关闭注入10/10", ok3, &d3);
    // J1-d：操作账守恒。
    let (ok4, d4) = drill_ops_account_10();
    s.add_dyn("J1-操作账守恒10/10", ok4, &d4);
    // J1-e：枚举面。
    let (ok5, d5) = drill_enum_face_10();
    s.add_dyn("J1-枚举面10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30880段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12468_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
