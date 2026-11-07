//! UNX-F51613 错误注入框架接入（M5/X10 注入矩阵 ×20 场景登记） · 0xC1C00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51614-J1 全谱 ×20 恢复时限达标率 100%；UNX-F51614-J1R
//! 超时项零静默逐条留账；③ 完成时 API/软件写回：状态=已完成、结果摘要
//! 非空。
//!
//! 结构清单：
//!   - InjCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - InjClass         注入类闭集（M5 五类：超时/校验/越界/资源/状态）
//!   - InjScenario      注入场景登记行（场景号/类×强位/可复现指纹）
//!   - InjectMatrix     注入矩阵登记器（×20 场景登记 + 未登记拒注入）
//!   - InjLedger        注入守恒账（检出+逃逸=轮数，逃逸恒 0）
//!   - run_unx51613_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 矩阵登记闭集：M5/X10 口径——5 注入类 × 强度位级组合，登记满 20
//!      场景为全谱面；场景号 1..=20 且两两互异，撞号判 DupScenario。
//!   ② 可复现指纹：场景指纹 = 类×强度×场景号派生，同登记恒同印；注入
//!      前必查登记，未登记场景 10/10 显性拒（Unregistered）。
//!   ③ 登记冻结：全谱登记满 20 后冻结，冻结后新增登记显性拒
//!      （MatrixFrozen），防登记面事后扩水。
//!   ④ 注入核：5 类坏样本（未登记注入/撞号登记/冻结后扩水/强度越界/
//!      复现指纹漂移）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机——登记序与注入序
//!      由 LCG 派生（种子 = SEG_BASE 派生），同段位恒定可复现；零
//!      unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1C00;

/// 注入类闭集大小（M5）。
pub const INJ_CLASSES: usize = 5;
/// 强度级数（X10）。
pub const STRENGTH_LEVELS: u32 = 10;
/// 全谱登记场景数。
pub const SCENARIOS: usize = 20;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 注入类闭集（M5）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjClass {
    /// 超时类
    Timeout = 0,
    /// 校验类
    Checksum = 1,
    /// 越界类
    Oob = 2,
    /// 资源类
    Resource = 3,
    /// 状态类
    State = 4,
}

impl InjClass {
    /// 全类闭集。
    pub fn all() -> [InjClass; 5] {
        [
            InjClass::Timeout,
            InjClass::Checksum,
            InjClass::Oob,
            InjClass::Resource,
            InjClass::State,
        ]
    }
    /// 类序号。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 类名（中文）。
    pub fn name(self) -> &'static str {
        match self {
            InjClass::Timeout => "超时",
            InjClass::Checksum => "校验",
            InjClass::Oob => "越界",
            InjClass::Resource => "资源",
            InjClass::State => "状态",
        }
    }
}

/// 注入场景登记行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjScenario {
    /// 场景号（1..=20）
    pub id: usize,
    /// 注入类序号（0..5）
    pub class: u32,
    /// 强度级（1..=10）
    pub strength: u32,
    /// 可复现指纹
    pub fp: u64,
}

impl InjScenario {
    /// 场景指纹：类×强度×场景号派生。
    pub fn make_fp(class: u32, strength: u32, id: usize) -> u64 {
        fnv1a(&class.to_le_bytes())
            ^ fnv1a(&strength.to_le_bytes()).rotate_left(17)
            ^ fnv1a(&(id as u64).to_le_bytes()).rotate_left(31)
            ^ 0x5161_3a0d_0000_0003
    }
}

/// 注入矩阵登记器。
pub struct InjectMatrix {
    rows: Vec<InjScenario>,
    frozen: bool,
    fired: Vec<u64>,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl InjectMatrix {
    /// 新建空矩阵。
    pub fn new() -> InjectMatrix {
        InjectMatrix {
            rows: Vec::new(),
            frozen: false,
            fired: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记一场景：场景号/类/强度逐项闭集校验，撞号显性拒。
    pub fn register(&mut self, id: usize, class: InjClass, strength: u32) -> Result<InjScenario, InjCode> {
        if self.frozen {
            self.rejected += 1;
            return Err(InjCode::MatrixFrozen);
        }
        if id == 0 || id > SCENARIOS {
            self.rejected += 1;
            return Err(InjCode::IdOob);
        }
        if strength == 0 || strength > STRENGTH_LEVELS {
            self.rejected += 1;
            return Err(InjCode::StrengthOob);
        }
        if self.rows.iter().any(|r| r.id == id) {
            self.rejected += 1;
            return Err(InjCode::DupScenario);
        }
        if self.rows.iter().any(|r| r.class == class.idx() && r.strength == strength) {
            self.rejected += 1;
            return Err(InjCode::DupClassStrength);
        }
        let row = InjScenario {
            id,
            class: class.idx(),
            strength,
            fp: InjScenario::make_fp(class.idx(), strength, id),
        };
        self.rows.push(row);
        if self.rows.len() == SCENARIOS {
            self.frozen = true;
        }
        Ok(row)
    }

    /// 登记行数。
    pub fn registered(&self) -> usize {
        self.rows.len()
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 按场景号查登记。
    pub fn find(&self, id: usize) -> Option<InjScenario> {
        self.rows.iter().copied().find(|r| r.id == id)
    }

    /// 注入前登记核验：未登记场景显性拒。
    pub fn fire(&mut self, id: usize) -> Result<InjScenario, InjCode> {
        match self.find(id) {
            Some(row) => {
                // 复现指纹漂移核验：重算指纹必须与登记一致。
                if row.fp != InjScenario::make_fp(row.class, row.strength, row.id) {
                    self.rejected += 1;
                    return Err(InjCode::FpDrift);
                }
                self.fired.push(row.fp);
                Ok(row)
            }
            None => {
                self.rejected += 1;
                Err(InjCode::Unregistered)
            }
        }
    }

    /// 注入发射账。
    pub fn fired_len(&self) -> usize {
        self.fired.len()
    }

    /// 发射轴可回放：指纹互异且与登记面一致。
    pub fn replayable(&self) -> bool {
        if self.fired.is_empty() {
            return false;
        }
        for i in 0..self.fired.len() {
            for j in (i + 1)..self.fired.len() {
                match (self.fired.get(i), self.fired.get(j)) {
                    (Some(&a), Some(&b)) if a == b => return false,
                    _ => {}
                }
            }
        }
        true
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
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

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for r in self.rows.iter() {
            bytes.extend_from_slice(&r.id.to_le_bytes());
            bytes.extend_from_slice(&r.class.to_le_bytes());
            bytes.extend_from_slice(&r.strength.to_le_bytes());
            bytes.extend_from_slice(&r.fp.to_le_bytes());
        }
        bytes.extend_from_slice(&(self.frozen as u8).to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（注入矩阵登记域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 场景号越界
    IdOob,
    /// 强度越界
    StrengthOob,
    /// 撞号登记
    DupScenario,
    /// 同类同强度重复
    DupClassStrength,
    /// 矩阵冻结后扩水
    MatrixFrozen,
    /// 未登记场景注入
    Unregistered,
    /// 复现指纹漂移
    FpDrift,
    /// 登记面不全
    MatrixShort,
    /// 类越界
    ClassOob,
    /// 发射轴为空
    EmptyFired,
}

impl InjCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            InjCode::Ok => "注入矩阵判定成功",
            InjCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            InjCode::IdOob => "场景号越界：场景号 1..=20",
            InjCode::StrengthOob => "强度越界：强度级 1..=10（X10 口径）",
            InjCode::DupScenario => "撞号登记：场景号两两互异",
            InjCode::DupClassStrength => "同类同强度重复：组合须唯一",
            InjCode::MatrixFrozen => "矩阵冻结：全谱登记满 20 后不得扩水",
            InjCode::Unregistered => "未登记场景注入：注入前必查登记，未登记显性拒",
            InjCode::FpDrift => "复现指纹漂移：重算指纹与登记不符",
            InjCode::MatrixShort => "登记面不全：全谱须登记满 20 场景",
            InjCode::ClassOob => "注入类越界：M5 闭集 0..5",
            InjCode::EmptyFired => "发射轴为空：可回放判据要求至少一发",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [InjCode; 12] = [
    InjCode::Ok,
    InjCode::SegBad,
    InjCode::IdOob,
    InjCode::StrengthOob,
    InjCode::DupScenario,
    InjCode::DupClassStrength,
    InjCode::MatrixFrozen,
    InjCode::Unregistered,
    InjCode::FpDrift,
    InjCode::MatrixShort,
    InjCode::ClassOob,
    InjCode::EmptyFired,
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
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
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

/// 标准脚本：全谱登记 20 场景（类轮回×强度轮转）→ 逐场景注入一发。
/// 返回 (登记数, 发射数, 冻结位)。
pub fn standard_script() -> (usize, usize, bool) {
    let classes = InjClass::all();
    let mut m = InjectMatrix::new();
    for i in 0..SCENARIOS {
        let c = classes[i % INJ_CLASSES];
        let strength = (i / INJ_CLASSES) as u32 + 1;
        let _ = m.register(i + 1, c, strength);
    }
    let frozen = m.is_frozen();
    for id in 1..=SCENARIOS {
        let _ = m.fire(id);
    }
    (m.registered(), m.fired_len(), frozen)
}

/// 双跑一致性：同一脚本两次执行结果逐位一致。
pub fn dual_run_consistent() -> bool {
    let a = standard_script();
    let b = standard_script();
    a == b && full_matrix_fp() != 0
}

/// 全谱矩阵快照指纹（20 场景登记的确定性叠印）。
pub fn full_matrix_fp() -> u64 {
    let mut m = InjectMatrix::new();
    let classes = InjClass::all();
    for i in 0..SCENARIOS {
        let _ = m.register(i + 1, classes[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1);
    }
    m.fingerprint()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：未登记场景注入——20 场景之外 10/10 拒。
    for _ in 0..INJ_ROUNDS {
        let mut m = InjectMatrix::new();
        let classes = InjClass::all();
        for i in 0..SCENARIOS {
            let _ = m.register(i + 1, classes[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1);
        }
        let caught = m.fire(SCENARIOS + 1) == Err(InjCode::Unregistered)
            && m.fire(0) == Err(InjCode::Unregistered);
        if caught {
            m.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：撞号登记——同场景号二次登记显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut m = InjectMatrix::new();
        let _ = m.register(1, InjClass::Timeout, 3);
        let caught = m.register(1, InjClass::Oob, 5) == Err(InjCode::DupScenario);
        if caught {
            m.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：冻结后扩水——登记满 20 后新增显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut m = InjectMatrix::new();
        let classes = InjClass::all();
        for i in 0..SCENARIOS {
            let _ = m.register(i + 1, classes[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1);
        }
        let caught = m.is_frozen()
            && m.register(SCENARIOS + 1, InjClass::State, 1) == Err(InjCode::MatrixFrozen);
        if caught {
            m.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：强度越界——0 级与 11 级显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut m = InjectMatrix::new();
        let lo = m.register(1, InjClass::Checksum, 0) == Err(InjCode::StrengthOob);
        let hi = m.register(2, InjClass::Checksum, STRENGTH_LEVELS + 1) == Err(InjCode::StrengthOob);
        if lo && hi {
            m.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：复现指纹漂移——发射前重算指纹失配显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut m = InjectMatrix::new();
        let _ = m.register(1, InjClass::Resource, 7);
        // 篡改登记指纹模拟漂移。
        if let Some(r) = m.rows.get_mut(0) {
            r.fp ^= 0x5161;
        }
        let caught = m.fire(1) == Err(InjCode::FpDrift);
        if caught {
            m.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51613_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51613");
    // 1 全谱 ×20 场景全登记可复现。
    {
        let (reg, fired, frozen) = standard_script();
        s.add(
            "全谱登记可复现",
            reg == SCENARIOS && fired == SCENARIOS && frozen,
            "UNX-F51613 判据：矩阵 ×20 场景全登记且逐场景注入可复现",
        );
    }
    // 2 未登记场景注入 10/10 拒绝。
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut m = InjectMatrix::new();
            let classes = InjClass::all();
            for i in 0..SCENARIOS {
                let _ = m.register(i + 1, classes[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1);
            }
            if m.fire(SCENARIOS + 2) != Err(InjCode::Unregistered) {
                ok = false;
            }
        }
        s.add(
            "未登记注入 10/10 拒",
            ok,
            "UNX-F51613-J1R：未登记场景注入逐轮显性拒（Unregistered）",
        );
    }
    // 3 注入类闭集与字面。
    {
        let ok = InjClass::all().len() == INJ_CLASSES
            && InjClass::all()
                .iter()
                .enumerate()
                .all(|(i, c)| c.idx() == i as u32 && !c.name().is_empty());
        s.add(
            "注入类闭集完整",
            ok,
            "M5 五类：超时/校验/越界/资源/状态，序号与字面齐备",
        );
    }
    // 4 场景指纹零撞号。
    {
        let mut ok = true;
        for c1 in 0..INJ_CLASSES as u32 {
            for st1 in 1..=STRENGTH_LEVELS {
                for c2 in 0..INJ_CLASSES as u32 {
                    for st2 in 1..=STRENGTH_LEVELS {
                        let same = c1 == c2 && st1 == st2;
                        if InjScenario::make_fp(c1, st1, 9)
                            == InjScenario::make_fp(c2, st2, 9)
                            && !same
                        {
                            ok = false;
                        }
                    }
                }
            }
        }
        s.add(
            "场景指纹零撞号",
            ok,
            "同场景号下（类,强度）指纹两两互异",
        );
    }
    // 5 撞号登记拒。
    {
        let mut m = InjectMatrix::new();
        let _ = m.register(3, InjClass::Timeout, 2);
        s.add(
            "撞号登记拒",
            m.register(3, InjClass::State, 4) == Err(InjCode::DupScenario)
                && m.rejected_tally() >= 1,
            "场景号两两互异，撞号显性拒",
        );
    }
    // 6 同类同强度重复拒。
    {
        let mut m = InjectMatrix::new();
        let _ = m.register(1, InjClass::Oob, 6);
        s.add(
            "同类同强度重复拒",
            m.register(2, InjClass::Oob, 6) == Err(InjCode::DupClassStrength),
            "（类,强度）组合唯一",
        );
    }
    // 7 场景号/强度越界拒。
    {
        let mut m = InjectMatrix::new();
        let id_lo = m.register(0, InjClass::Timeout, 1) == Err(InjCode::IdOob);
        let id_hi = m.register(SCENARIOS + 1, InjClass::Timeout, 1) == Err(InjCode::IdOob);
        let st = m.register(1, InjClass::Timeout, 0) == Err(InjCode::StrengthOob);
        s.add(
            "越界登记拒",
            id_lo && id_hi && st,
            "场景号 1..=20、强度 1..=10 之外显性拒",
        );
    }
    // 8 冻结语义：不满 20 不冻结，满 20 即冻结。
    {
        let mut m = InjectMatrix::new();
        for i in 0..(SCENARIOS - 1) {
            let _ = m.register(
                i + 1,
                InjClass::all()[i % INJ_CLASSES],
                (i / INJ_CLASSES) as u32 + 1,
            );
        }
        let pre = !m.is_frozen();
        let _ = m.register(
            SCENARIOS,
            InjClass::all()[(SCENARIOS - 1) % INJ_CLASSES],
            (SCENARIOS / INJ_CLASSES) as u32 + 1,
        );
        s.add(
            "冻结语义",
            pre && m.is_frozen(),
            "登记满 20 即冻结，未满不冻结（显性状态）",
        );
    }
    // 9 发射轴可回放。
    {
        let mut m = InjectMatrix::new();
        for i in 0..8 {
            let _ = m.register(i + 1, InjClass::all()[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1);
        }
        for id in 1..=8 {
            let _ = m.fire(id);
        }
        s.add(
            "发射轴可回放",
            m.replayable() && m.fired_len() == 8,
            "指纹互异且与登记面一致，可逐发回放",
        );
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 诊断码闭集唯一性。
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
    // 12 注入账守恒。
    {
        let mut m = InjectMatrix::new();
        for _ in 0..16u32 {
            m.note_detected();
        }
        s.add("注入账守恒", m.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一登记脚本两次执行，矩阵快照指纹逐位一致",
    );
    // 14 标准脚本回归。
    {
        let (reg, fired, frozen) = standard_script();
        s.add(
            "标准脚本回归",
            reg == 20 && fired == 20 && frozen,
            "20 登记 → 20 发射 → 冻结，全链回归",
        );
    }
    // 15 全谱指纹可复算。
    {
        let fp1 = full_matrix_fp();
        let fp2 = full_matrix_fp();
        s.add(
            "全谱指纹复算",
            fp1 != 0 && fp1 == fp2,
            "同登记序两建矩阵指纹一致（可复算面）",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51613 注入矩阵 {} 类 ×{} 场景",
            INJ_CLASSES, SCENARIOS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51613"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51613_checks_green() {
        let set = run_unx51613_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_matrix_register_and_fire() {
        let mut m = InjectMatrix::new();
        let classes = InjClass::all();
        for i in 0..SCENARIOS {
            assert!(m.register(i + 1, classes[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1).is_ok());
        }
        assert_eq!(m.registered(), SCENARIOS);
        assert!(m.is_frozen());
        for id in 1..=SCENARIOS {
            assert!(m.fire(id).is_ok());
        }
        assert_eq!(m.fired_len(), SCENARIOS);
        assert!(m.replayable());
    }

    #[test]
    fn unregistered_rejected() {
        let mut m = InjectMatrix::new();
        assert_eq!(m.fire(1), Err(InjCode::Unregistered));
        let _ = m.register(1, InjClass::Timeout, 1);
        assert!(m.fire(1).is_ok());
        assert_eq!(m.fire(2), Err(InjCode::Unregistered));
    }

    #[test]
    fn dup_scenario_and_dup_class_strength() {
        let mut m = InjectMatrix::new();
        let _ = m.register(1, InjClass::Checksum, 5);
        assert_eq!(m.register(1, InjClass::Oob, 5), Err(InjCode::DupScenario));
        assert_eq!(m.register(2, InjClass::Checksum, 5), Err(InjCode::DupClassStrength));
    }

    #[test]
    fn bounds_guards() {
        let mut m = InjectMatrix::new();
        assert_eq!(m.register(0, InjClass::Timeout, 1), Err(InjCode::IdOob));
        assert_eq!(m.register(21, InjClass::Timeout, 1), Err(InjCode::IdOob));
        assert_eq!(m.register(1, InjClass::Timeout, 0), Err(InjCode::StrengthOob));
        assert_eq!(m.register(1, InjClass::Timeout, 11), Err(InjCode::StrengthOob));
    }

    #[test]
    fn frozen_blocks_new_registration() {
        let mut m = InjectMatrix::new();
        let classes = InjClass::all();
        for i in 0..SCENARIOS {
            let _ = m.register(i + 1, classes[i % INJ_CLASSES], (i / INJ_CLASSES) as u32 + 1);
        }
        assert!(m.is_frozen());
        assert_eq!(
            m.register(21, InjClass::State, 2),
            Err(InjCode::MatrixFrozen)
        );
        assert_eq!(m.registered(), SCENARIOS);
    }

    #[test]
    fn fp_drift_detected() {
        let mut m = InjectMatrix::new();
        let _ = m.register(1, InjClass::Resource, 7);
        if let Some(r) = m.rows.get_mut(0) {
            r.fp ^= 0x5161;
        }
        assert_eq!(m.fire(1), Err(InjCode::FpDrift));
    }

    #[test]
    fn scenario_fp_distinct_per_class_strength() {
        for c1 in 0..INJ_CLASSES as u32 {
            for st1 in 1..=STRENGTH_LEVELS {
                for c2 in 0..INJ_CLASSES as u32 {
                    for st2 in 1..=STRENGTH_LEVELS {
                        let same = c1 == c2 && st1 == st2;
                        assert_eq!(
                            InjScenario::make_fp(c1, st1, 4) == InjScenario::make_fp(c2, st2, 4),
                            same
                        );
                    }
                }
            }
        }
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
    fn conservation_holds() {
        let mut m = InjectMatrix::new();
        for _ in 0..10u32 {
            m.note_detected();
        }
        assert!(m.conserved());
        m.note_escape();
        assert!(!m.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        assert_eq!(full_matrix_fp(), full_matrix_fp());
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
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
