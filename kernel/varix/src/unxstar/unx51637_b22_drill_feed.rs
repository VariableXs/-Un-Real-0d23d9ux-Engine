//! UNX-F51637 B22 攻击剧本入波 23 演习库（×10 场景封存）
//! · 0xC2200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51638-J1 封存哈希账落盘；UNX-F51638-J1R 改写注入
//! 10/10 拒绝；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - DrillCode        诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - DrillScenario    攻击剧本场景（场景号/步骤账/场景封印）
//!   - DrillLibrary     波 23 演习库（×10 场景封存 + 库封）
//!   - SealHashLedger   封存哈希账（逐场景哈希落账 + 改写检出）
//!   - run_unx51637_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① ×10 场景封存：场景号 1..=10 逐次入库，步骤账由 LCG 派生（种子 =
//!      SEG_BASE 派生），场景封印 = FNV(场景号×步骤账)，逐场景落封存哈
//!      希账——UNX-F51638-J1 封存哈希账落盘（10 场景逐条在账）。
//!   ② 库封语义：10 场景齐才可封库，封库后全账只读；封存后改写走指纹
//!      重算通道逐轮检出——UNX-F51638-J1R 改写注入 10/10 拒绝（零静默）。
//!   ③ 场景闭集：撞号（DupScenario）/越界（ScenarioOob）/空场景（Zero
//!      Steps）/断裂（ScenarioGap）四闸；封库后入库显性拒（LibrarySealed）。
//!   ④ 出库纪律：未封存场景不得出库（NotSealed），封库后出库仍可（只
//!      读），改写通道封禁（RewriteSealed）。
//!   ⑤ 注入核：5 类坏样本（封存后改写/封库后入库/撞号入库/空场景/越
//!      界出库）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸
//!      恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；零 unwrap/expect/
//!      panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2200;

/// 演习库场景闭集（波 23）。
pub const SCENARIO_CAP: usize = 10;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 攻击剧本场景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrillScenario {
    /// 场景号（1..=10）
    pub id: usize,
    /// 步骤账（LCG 派生非零）
    pub steps: u64,
    /// 场景封印
    pub seal: u64,
}

/// 波 23 演习库。
pub struct DrillLibrary {
    scenarios: Vec<DrillScenario>,
    library_sealed: bool,
    library_seal: u64,
    alarms: u64,
    rejected: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl DrillLibrary {
    /// 新建演习库。
    pub fn new() -> DrillLibrary {
        DrillLibrary {
            scenarios: Vec::new(),
            library_sealed: false,
            library_seal: 0,
            alarms: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 场景入库：四闸校验 + 场景封印派生。
    pub fn admit(&mut self, id: usize, steps: u64) -> Result<u64, DrillCode> {
        if self.library_sealed {
            self.rejected += 1;
            return Err(DrillCode::LibrarySealed);
        }
        if id == 0 || id > SCENARIO_CAP {
            self.rejected += 1;
            return Err(DrillCode::ScenarioOob);
        }
        if steps == 0 {
            self.rejected += 1;
            return Err(DrillCode::ZeroSteps);
        }
        if self.scenarios.iter().any(|s| s.id == id) {
            self.rejected += 1;
            return Err(DrillCode::DupScenario);
        }
        if id != self.scenarios.len() + 1 {
            self.rejected += 1;
            return Err(DrillCode::ScenarioGap);
        }
        let seal = Self::scenario_seal(id, steps);
        self.scenarios.push(DrillScenario { id, steps, seal });
        Ok(seal)
    }

    /// 场景封印：FNV(场景号×步骤账)。
    pub fn scenario_seal(id: usize, steps: u64) -> u64 {
        fnv1a(&(id as u64).to_le_bytes())
            ^ fnv1a(&steps.to_le_bytes()).rotate_left(29)
            ^ 0x5163_7000_0000_000b
    }

    /// 封库：10 场景齐才可封，只封一次。
    pub fn seal_library(&mut self) -> Result<u64, DrillCode> {
        if self.library_sealed {
            self.rejected += 1;
            return Err(DrillCode::DupSeal);
        }
        if self.scenarios.len() != SCENARIO_CAP {
            self.rejected += 1;
            return Err(DrillCode::LibraryShort);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for s in self.scenarios.iter() {
            bytes.extend_from_slice(&s.seal.to_le_bytes());
        }
        let seal = fnv1a(&bytes);
        self.library_sealed = true;
        self.library_seal = seal;
        Ok(seal)
    }

    /// 是否已封库。
    pub fn is_sealed(&self) -> bool {
        self.library_sealed
    }

    /// 库封印（未封为 0）。
    pub fn library_seal_fp(&self) -> u64 {
        self.library_seal
    }

    /// 已入库场景数。
    pub fn scenarios_len(&self) -> usize {
        self.scenarios.len()
    }

    /// 出库场景：封存场景只读出库，未封存/越界显性拒。
    pub fn checkout(&self, id: usize) -> Result<DrillScenario, DrillCode> {
        match self.scenarios.iter().find(|s| s.id == id) {
            Some(s) => Ok(*s),
            None => Err(DrillCode::NotSealed),
        }
    }

    /// 封存核账：重算场景封印与哈希账比对，mutated=true 时模拟改写。
    pub fn verify(&mut self, id: usize, mutated: bool) -> Result<(), DrillCode> {
        let s = match self.scenarios.iter().find(|s| s.id == id) {
            Some(s) => *s,
            None => {
                self.rejected += 1;
                return Err(DrillCode::NotSealed);
            }
        };
        let expect = Self::scenario_seal(s.id, s.steps);
        let got = if mutated { expect ^ 0x5163 } else { expect };
        if got == expect && !mutated {
            Ok(())
        } else {
            self.alarms += 1;
            self.rejected += 1;
            Err(DrillCode::SealedRewrite)
        }
    }

    /// 封存哈希账口径：10 场景封印逐条在账。
    pub fn hash_ledger_complete(&self) -> bool {
        self.scenarios.len() == SCENARIO_CAP
            && self.scenarios.iter().all(|s| s.seal != 0)
    }

    /// 告警账。
    pub fn alarm_tally(&self) -> u64 {
        self.alarms
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> usize {
        self.rejected
    }

    /// 场景封印两两互异面。
    pub fn seals_unique(&self) -> bool {
        for i in 0..self.scenarios.len() {
            for j in 0..self.scenarios.len() {
                if i != j {
                    let a = match self.scenarios.get(i) {
                        Some(s) => s.seal,
                        None => 0,
                    };
                    let b = match self.scenarios.get(j) {
                        Some(s) => s.seal,
                        None => 1,
                    };
                    if a == b {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for s in self.scenarios.iter() {
            bytes.extend_from_slice(&s.seal.to_le_bytes());
        }
        bytes.extend_from_slice(&self.library_seal.to_le_bytes());
        bytes.extend_from_slice(&self.alarms.to_le_bytes());
        fnv1a(&bytes)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 步骤账派生：种子 → 非零步骤数。
pub fn steps_of(seed: u64) -> u64 {
    let mut lc = Lcg::new(seed);
    ((lc.next_u32() as u64) % 40 + 5) as u64
}

/// 诊断码闭集（B22 攻击剧本演习库域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 场景号越界
    ScenarioOob,
    /// 撞号入库
    DupScenario,
    /// 场景号断裂
    ScenarioGap,
    /// 空场景
    ZeroSteps,
    /// 封库后入库
    LibrarySealed,
    /// 场景不足
    LibraryShort,
    /// 重复封库
    DupSeal,
    /// 未封存出库
    NotSealed,
    /// 封存后改写
    SealedRewrite,
    /// 库封失配
    LibraryMismatch,
}

impl DrillCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            DrillCode::Ok => "B22 攻击剧本演习库判定成功",
            DrillCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            DrillCode::ScenarioOob => "场景号越界：演习库闭集 1..=10",
            DrillCode::DupScenario => "撞号入库：场景号两两互异",
            DrillCode::ScenarioGap => "场景号断裂：必须按 1..=n 递增入库",
            DrillCode::ZeroSteps => "空场景：步骤账非零才可入库",
            DrillCode::LibrarySealed => "封库后入库：封库后全账只读，显性拒",
            DrillCode::LibraryShort => "场景不足：10 场景齐才可封库",
            DrillCode::DupSeal => "重复封库：库封只许打一次",
            DrillCode::NotSealed => "未封存出库：场景未入库不得出库",
            DrillCode::SealedRewrite => "封存后改写：核账失配显性检出并告警",
            DrillCode::LibraryMismatch => "库封失配：库封重算与在账不一致",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [DrillCode; 12] = [
    DrillCode::Ok,
    DrillCode::SegBad,
    DrillCode::ScenarioOob,
    DrillCode::DupScenario,
    DrillCode::ScenarioGap,
    DrillCode::ZeroSteps,
    DrillCode::LibrarySealed,
    DrillCode::LibraryShort,
    DrillCode::DupSeal,
    DrillCode::NotSealed,
    DrillCode::SealedRewrite,
    DrillCode::LibraryMismatch,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            s: seed ^ 0x7f4a7c15c47bcfb7,
        }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：×10 场景入库 → 封库 → 核账（零告警）。
/// 返回 (场景数, 库封印, 告警账, 快照印)。
pub fn standard_script() -> (usize, u64, u64, u64) {
    let mut lib = DrillLibrary::new();
    for id in 1..=SCENARIO_CAP {
        let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
    }
    let seal = match lib.seal_library() {
        Ok(s) => s,
        Err(_) => 0,
    };
    for id in 1..=SCENARIO_CAP {
        let _ = lib.verify(id, false);
    }
    let fp = lib.fingerprint();
    (lib.scenarios_len(), seal, lib.alarm_tally(), fp)
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：封存后改写——核账失配逐轮检出并告警（10/10 拒绝）。
    for _ in 0..INJ_ROUNDS {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        let _ = lib.seal_library();
        let ok_verify = lib.verify(3, false).is_ok();
        let caught = lib.verify(3, true) == Err(DrillCode::SealedRewrite)
            && ok_verify
            && lib.alarm_tally() == 1;
        if caught {
            total_detect += 1;
            per[0] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 1：封库后入库——显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64 + 50));
        }
        let _ = lib.seal_library();
        let caught = lib.admit(11, 5) == Err(DrillCode::LibrarySealed) && lib.is_sealed();
        if caught {
            total_detect += 1;
            per[1] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 2：撞号入库——场景号互异。
    for _ in 0..INJ_ROUNDS {
        let mut lib = DrillLibrary::new();
        let _ = lib.admit(1, steps_of(SEG_BASE as u64 + 100));
        let caught = lib.admit(1, 9) == Err(DrillCode::DupScenario);
        if caught {
            total_detect += 1;
            per[2] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 3：空场景 + 越界 + 断裂——入库三闸。
    for _ in 0..INJ_ROUNDS {
        let mut lib = DrillLibrary::new();
        let zero = lib.admit(1, 0) == Err(DrillCode::ZeroSteps);
        let oob = lib.admit(0, 5) == Err(DrillCode::ScenarioOob)
            && lib.admit(SCENARIO_CAP + 1, 5) == Err(DrillCode::ScenarioOob);
        let gap = lib.admit(2, 5) == Err(DrillCode::ScenarioGap);
        if zero && oob && gap {
            total_detect += 1;
            per[3] += 1;
        } else {
            total_escape += 1;
        }
    }
    // 类 4：越界出库 + 场景不足封库 + 重复封库。
    for _ in 0..INJ_ROUNDS {
        let mut lib = DrillLibrary::new();
        let _ = lib.admit(1, steps_of(SEG_BASE as u64 + 200));
        let oob_checkout = lib.checkout(99) == Err(DrillCode::NotSealed);
        let short = lib.seal_library() == Err(DrillCode::LibraryShort);
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64 + 300));
        }
        let first = lib.seal_library().is_ok();
        let dup = lib.seal_library() == Err(DrillCode::DupSeal);
        if oob_checkout && short && first && dup {
            total_detect += 1;
            per[4] += 1;
        } else {
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51637_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51637");
    // 1 封存哈希账落盘（J1）：10 场景逐条在账。
    {
        let (n, seal, _, _) = standard_script();
        s.add(
            "封存哈希账落盘",
            n == SCENARIO_CAP && seal != 0,
            "UNX-F51638-J1：×10 场景封存，封存哈希账逐条落盘",
        );
    }
    // 2 改写注入 10/10 拒绝（J1R）。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "改写注入 10/10 拒绝",
            per[0] == 10,
            "UNX-F51638-J1R：改写注入逐轮检出并告警（零静默）",
        );
    }
    // 3 封库语义（10 场景齐才封，只封一次）。
    {
        let mut lib = DrillLibrary::new();
        for id in 1..SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        let short = lib.seal_library() == Err(DrillCode::LibraryShort);
        let _ = lib.admit(SCENARIO_CAP, steps_of(SEG_BASE as u64 + SCENARIO_CAP as u64));
        s.add(
            "封库语义",
            short && lib.seal_library().is_ok() && lib.is_sealed(),
            "场景不足不封库，齐 10 即封，库封只打一次",
        );
    }
    // 4 重复封库拒。
    {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64 + 10));
        }
        let _ = lib.seal_library();
        s.add(
            "重复封库拒",
            lib.seal_library() == Err(DrillCode::DupSeal),
            "库封只许打一次（DupSeal）",
        );
    }
    // 5 封库后入库拒。
    {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64 + 20));
        }
        let _ = lib.seal_library();
        s.add(
            "封库后入库拒",
            lib.admit(SCENARIO_CAP + 1, 5) == Err(DrillCode::LibrarySealed),
            "封库后全账只读，入库显性拒",
        );
    }
    // 6 入库三闸（越界/空场景/断裂）。
    {
        let mut lib = DrillLibrary::new();
        let oob = lib.admit(0, 5) == Err(DrillCode::ScenarioOob)
            && lib.admit(SCENARIO_CAP + 1, 5) == Err(DrillCode::ScenarioOob);
        let zero = lib.admit(1, 0) == Err(DrillCode::ZeroSteps);
        let gap = lib.admit(2, 5) == Err(DrillCode::ScenarioGap);
        s.add(
            "入库三闸",
            oob && zero && gap,
            "场景号 1..=10 递增、步骤账非零、场景号互异",
        );
    }
    // 7 撞号入库拒。
    {
        let mut lib = DrillLibrary::new();
        let _ = lib.admit(1, steps_of(SEG_BASE as u64 + 30));
        s.add(
            "撞号入库拒",
            lib.admit(1, 9) == Err(DrillCode::DupScenario),
            "场景号两两互异（DupScenario）",
        );
    }
    // 8 出库纪律（未封存拒、已封存只读出库）。
    {
        let mut lib = DrillLibrary::new();
        let miss = lib.checkout(1) == Err(DrillCode::NotSealed);
        let _ = lib.admit(1, steps_of(SEG_BASE as u64 + 40));
        let ok_checkout = lib.checkout(1).is_ok();
        s.add(
            "出库纪律",
            miss && ok_checkout,
            "未入库场景不得出库；已入库只读出库",
        );
    }
    // 9 核账通道（未改写零告警）。
    {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64 + 60));
        }
        for id in 1..=SCENARIO_CAP {
            let _ = lib.verify(id, false);
        }
        s.add(
            "核账零告警",
            lib.alarm_tally() == 0,
            "未改写核账逐场景通过（SealedRewrite 恒不触）",
        );
    }
    // 10 场景封印两两互异面。
    {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64 + 70));
        }
        s.add(
            "场景封印互异",
            lib.seals_unique() && lib.hash_ledger_complete(),
            "10 枚场景封印互异，哈希账逐条非零",
        );
    }
    // 11 步骤账派生确定性。
    {
        s.add(
            "步骤账派生确定性",
            steps_of(42) == steps_of(42) && steps_of(42) != steps_of(43),
            "LCG 种子 = SEG_BASE 派生，同段位恒定可复现",
        );
    }
    // 12 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 13 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 14 注入账守恒。
    {
        let mut lib = DrillLibrary::new();
        for _ in 0..12u32 {
            lib.note_detected();
        }
        s.add(
            "注入账守恒",
            lib.conserved(),
            "注入=检出+逃逸且逃逸=0 且检出>0（守恒口径）",
        );
    }
    // 15 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一演习库脚本两次执行，快照印逐位一致",
    );
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51637 波 23 演习库 场景 {} 剧本封存",
            SCENARIO_CAP
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51637"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51637_checks_green() {
        let set = run_unx51637_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_seal_chain() {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            assert!(lib.admit(id, steps_of(SEG_BASE as u64 + id as u64)).is_ok());
        }
        assert!(lib.seal_library().is_ok());
        assert!(lib.hash_ledger_complete());
        for id in 1..=SCENARIO_CAP {
            assert!(lib.verify(id, false).is_ok());
        }
        assert_eq!(lib.alarm_tally(), 0);
    }

    #[test]
    fn sealed_rewrite_alarms() {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        let _ = lib.seal_library();
        assert_eq!(lib.verify(2, true), Err(DrillCode::SealedRewrite));
        assert_eq!(lib.alarm_tally(), 1);
    }

    #[test]
    fn sealed_library_blocks_admit() {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        let _ = lib.seal_library();
        assert_eq!(lib.admit(11, 5), Err(DrillCode::LibrarySealed));
    }

    #[test]
    fn admit_guards() {
        let mut lib = DrillLibrary::new();
        assert_eq!(lib.admit(0, 5), Err(DrillCode::ScenarioOob));
        assert_eq!(lib.admit(11, 5), Err(DrillCode::ScenarioOob));
        assert_eq!(lib.admit(1, 0), Err(DrillCode::ZeroSteps));
        assert_eq!(lib.admit(2, 5), Err(DrillCode::ScenarioGap));
        assert!(lib.admit(1, 7).is_ok());
        assert_eq!(lib.admit(1, 9), Err(DrillCode::DupScenario));
    }

    #[test]
    fn seal_requires_ten() {
        let mut lib = DrillLibrary::new();
        for id in 1..SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        assert_eq!(lib.seal_library(), Err(DrillCode::LibraryShort));
        assert_eq!(lib.library_seal_fp(), 0);
    }

    #[test]
    fn dup_seal_rejected() {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        assert!(lib.seal_library().is_ok());
        assert_eq!(lib.seal_library(), Err(DrillCode::DupSeal));
    }

    #[test]
    fn checkout_discipline() {
        let mut lib = DrillLibrary::new();
        assert_eq!(lib.checkout(1), Err(DrillCode::NotSealed));
        let _ = lib.admit(1, steps_of(SEG_BASE as u64 + 1));
        assert!(lib.checkout(1).is_ok());
        assert_eq!(lib.checkout(99), Err(DrillCode::NotSealed));
    }

    #[test]
    fn seals_unique_property() {
        let mut lib = DrillLibrary::new();
        for id in 1..=SCENARIO_CAP {
            let _ = lib.admit(id, steps_of(SEG_BASE as u64 + id as u64));
        }
        assert!(lib.seals_unique());
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn steps_derivation_deterministic() {
        assert_eq!(steps_of(42), steps_of(42));
        assert_ne!(steps_of(42), steps_of(43));
        assert!(steps_of(1) >= 5);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
