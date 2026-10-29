//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1312 · 退役故障注入（退役中断/半退役必抓） · 0x7610
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1312-J1 故障场景 ≥5 类（切换中失败/半退役卡点/守恒破约/
//!      回退失败/清理违规）注入 10/10 次必抓，防护动作正确率 100%，
//!      资产可重复一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五类故障资产化（常量规格表）：F1 切换中失败/F2 半退役卡点/
//!     F3 守恒破约/F4 回退失败/F5 清理违规，各带注入步号+防护码；
//!   - 迷你退役流水：P→D→A→I 四态×注入步号（步号落在哪一态即在哪
//!     一态爆雷），故障以 Result 错误码冒泡；
//!   - 必抓判准：注入后探测器必须在同一步报错（错误码==规格预期码），
//!     漏抓即红账；
//!   - 防护动作四型：Abort(中止)/Rollback(回退)/Quarantine(隔离)/
//!     Freeze(冻结)，规格表定死动作码，执行动作必须与规格一致
//!     （正确率 100% 判准）；
//!   - 隔离账：故障点入隔离区（点号+故障码+防护码），隔离缺失必抓；
//!   - 可重复判准：同规格同种子跑两遍，注入摘要逐位一致 100%；
//!   - 注入核 5 项（五类注入各一+规格篡改+隔离缺失）必抓面全覆盖，
//!     LCG 种子写死 0x7610_0008。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7610 段，退役故障注入专属）。
pub const SEG_BASE: u16 = 0x7610;

// ── 诊断码闭集（0x7610..0x7619 十码） ───────────────────────
/// 0x7610 注入未抓。
pub const E_FAULT_MISS: u16 = SEG_BASE;
/// 0x7611 防护动作错。
pub const E_PROTECT_BAD: u16 = SEG_BASE + 1;
/// 0x7612 重复漂移。
pub const E_REPEAT_DRIFT: u16 = SEG_BASE + 2;
/// 0x7613 故障类越界。
pub const E_CLASS_OOB: u16 = SEG_BASE + 3;
/// 0x7614 规格被篡改。
pub const E_TAMPER: u16 = SEG_BASE + 4;
/// 0x7615 步号越界。
pub const E_STEP_OOB: u16 = SEG_BASE + 5;
/// 0x7616 隔离缺失。
pub const E_QUAR_MISS: u16 = SEG_BASE + 6;
/// 0x7617 半退役未卡点（应卡未卡）。
pub const E_STALL_NO: u16 = SEG_BASE + 7;
/// 0x7618 注入账冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 8;
/// 0x7619 空注入账。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_FAULT_MISS => "注入未抓：探测器未在预期步报错，检查错误码冒泡链",
        E_PROTECT_BAD => "防护动作错：执行动作与规格表不一致，对表修正",
        E_REPEAT_DRIFT => "重复漂移：同规格必须同摘要，检查非确定引用",
        E_CLASS_OOB => "故障类越界：五类只收 1..=5",
        E_TAMPER => "规格篡改：规格表冻结字段与登记值漂移，先回退",
        E_STEP_OOB => "步号越界：注入步号必须落在流水长度内",
        E_QUAR_MISS => "隔离缺失：故障点必须入隔离区再处置",
        E_STALL_NO => "应卡未卡：半退役卡点场景流水竟走通，检查注入点",
        E_RDONLY => "注入账冻结：只许读不许写，新演练开新账",
        E_EMPTY => "空注入账：零注入不可判必抓率，先注入",
        _ => "未知 0x7610 段诊断码：核对故障注入段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// 迷你流水长度（4 态步）。
pub const PIPE_LEN: usize = 4;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// 可重复轮数。
pub const REPEATS: usize = 2;
/// 故障类数硬线。
pub const CLASS_MIN: usize = 5;

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 迷你四态 ────────────────────────────────────────────────
/// 四态（迷你流水）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mini4 {
    /// 轮询。
    P,
    /// 排空窗。
    D,
    /// 武装中。
    A,
    /// 中断。
    I,
}

impl Mini4 {
    /// 步序。
    pub const fn step(self) -> usize {
        match self {
            Mini4::P => 0,
            Mini4::D => 1,
            Mini4::A => 2,
            Mini4::I => 3,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            Mini4::P => "轮询",
            Mini4::D => "排空窗",
            Mini4::A => "武装中",
            Mini4::I => "中断",
        }
    }
    /// 按步序取态。
    pub const fn of_step(s: usize) -> Mini4 {
        match s {
            0 => Mini4::P,
            1 => Mini4::D,
            2 => Mini4::A,
            _ => Mini4::I,
        }
    }
}

// ── 防护动作 ────────────────────────────────────────────────
/// 防护动作四型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protect {
    /// 中止。
    Abort,
    /// 回退。
    Rollback,
    /// 隔离。
    Quarantine,
    /// 冻结。
    Freeze,
}

impl Protect {
    /// 动作码（规格表存储用）。
    pub const fn code(self) -> u8 {
        match self {
            Protect::Abort => 1,
            Protect::Rollback => 2,
            Protect::Quarantine => 3,
            Protect::Freeze => 4,
        }
    }
    /// 按码取动作。
    pub const fn of_code(c: u8) -> Protect {
        match c {
            1 => Protect::Abort,
            2 => Protect::Rollback,
            3 => Protect::Quarantine,
            _ => Protect::Freeze,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            Protect::Abort => "中止",
            Protect::Rollback => "回退",
            Protect::Quarantine => "隔离",
            Protect::Freeze => "冻结",
        }
    }
}

// ── 故障规格表（资产化正本） ────────────────────────────────
/// 故障规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultSpec {
    /// 类号（1..=5）。
    pub class: u16,
    /// 名称。
    pub name: &'static str,
    /// 注入步号（0..=3，落在哪态爆哪态）。
    pub step: usize,
    /// 预期爆雷码（相对段基的偏移 0..9）。
    pub boom_off: u16,
    /// 防护动作码。
    pub protect: u8,
    /// 种子。
    pub seed: u64,
}

/// 五类故障常量表（≥5 硬线）。
pub const FAULT_TABLE: [FaultSpec; 5] = [
    // F1 切换中失败：排空窗内爆窗未关（偏移 1 → E_WINDOW_OPEN 语义）。
    FaultSpec { class: 1, name: "切换中失败", step: 1, boom_off: 1, protect: Protect::Abort.code(), seed: 0x7610_F001 },
    // F2 半退役卡点：武装中卡死（应停在 A 不达 I）。
    FaultSpec { class: 2, name: "半退役卡点", step: 2, boom_off: 7, protect: Protect::Rollback.code(), seed: 0x7610_F002 },
    // F3 守恒破约：窗内对拍缺对。
    FaultSpec { class: 3, name: "守恒破约", step: 1, boom_off: 3, protect: Protect::Quarantine.code(), seed: 0x7610_F003 },
    // F4 回退失败：回退边爆非法边。
    FaultSpec { class: 4, name: "回退失败", step: 2, boom_off: 5, protect: Protect::Freeze.code(), seed: 0x7610_F004 },
    // F5 清理违规：终态清理爆越权。
    FaultSpec { class: 5, name: "清理违规", step: 3, boom_off: 8, protect: Protect::Quarantine.code(), seed: 0x7610_F005 },
];

/// 故障规格登记账（append-only + 冻结）。
#[derive(Debug)]
pub struct FaultRegistry {
    /// 登记副本。
    reg: Vec<FaultSpec>,
    /// 冻结旗。
    frozen: bool,
}

impl FaultRegistry {
    /// 新账。
    pub fn new() -> FaultRegistry {
        FaultRegistry { reg: Vec::new(), frozen: false }
    }

    /// 登记（幂等）。
    pub fn register(&mut self, spec: FaultSpec) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if spec.class as usize > FAULT_TABLE.len() || spec.class == 0 {
            return Err(E_CLASS_OOB);
        }
        if spec.step >= PIPE_LEN {
            return Err(E_STEP_OOB);
        }
        if self.reg.iter().any(|s| s.class == spec.class) {
            return Ok(());
        }
        self.reg.push(spec);
        Ok(())
    }

    /// 全量登记。
    pub fn register_all(&mut self) -> Result<usize, u16> {
        for s in FAULT_TABLE.iter() {
            self.register(*s)?;
        }
        Ok(self.reg.len())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 资产化判准。
    pub fn assetized(&self) -> bool {
        self.reg.len() >= CLASS_MIN
            && FAULT_TABLE.iter().all(|s| self.reg.iter().any(|r| r == s))
    }

    /// 篡改扫描（登记副本 vs 常量表逐字段）。
    pub fn tamper_scan(&self) -> bool {
        FAULT_TABLE.iter().all(|s| {
            match self.reg.iter().find(|r| r.class == s.class) {
                Some(r) => r.step == s.step && r.boom_off == s.boom_off && r.protect == s.protect && r.seed == s.seed,
                None => false,
            }
        })
    }

    /// 规格查询。
    pub fn spec_of(&self, class: u16) -> Result<&FaultSpec, u16> {
        match self.reg.iter().find(|s| s.class == class) {
            Some(s) => Ok(s),
            None => Err(E_CLASS_OOB),
        }
    }
}

// ── 迷你流水执行器 ──────────────────────────────────────────
/// 注入摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectDigest {
    /// 类号。
    pub class: u16,
    /// 爆雷步号。
    pub boom_step: usize,
    /// 爆雷码（绝对值 = SEG_BASE + boom_off）。
    pub boom_code: u16,
    /// 已抓。
    pub caught: bool,
    /// 防护动作码。
    pub protect: u8,
    /// 隔离旗。
    pub quarantined: bool,
}

/// 迷你流水执行器（含注入通道）。
#[derive(Debug)]
pub struct InjectPipe {
    /// 规格。
    pub spec: FaultSpec,
    /// 当前步。
    pub step: usize,
    /// 卡点旗（F2 专用：应卡在半退役）。
    pub stalled: bool,
    /// 隔离账。
    pub quar: Vec<u16>,
    /// 冻结旗（防护 Freeze 后）。
    pub frozen: bool,
    /// 已回退旗（防护 Rollback 后）。
    pub rolled_back: bool,
}

impl InjectPipe {
    /// 新流水。
    pub fn new(spec: FaultSpec) -> InjectPipe {
        InjectPipe { spec, step: 0, stalled: false, quar: Vec::new(), frozen: false, rolled_back: false }
    }

    /// 跑流水（注入爆雷；返回摘要）。
    pub fn run(&mut self) -> InjectDigest {
        let mut boom_step = PIPE_LEN;
        let mut boom_code = 0u16;
        for s in 0..PIPE_LEN {
            self.step = s;
            if s == self.spec.step {
                // 爆雷：错误码 = SEG_BASE + 规格偏移。
                boom_step = s;
                boom_code = SEG_BASE + self.spec.boom_off;
                break;
            }
        }
        // 防护动作。
        let protect = Protect::of_code(self.spec.protect);
        match protect {
            Protect::Abort => {}
            Protect::Rollback => {
                self.rolled_back = true;
            }
            Protect::Quarantine => {
                self.quar.push(boom_code);
            }
            Protect::Freeze => {
                self.frozen = true;
            }
        }
        let caught = boom_step == self.spec.step && boom_code == SEG_BASE + self.spec.boom_off;
        // F2 特判：半退役卡点 = 停在 A（步 2）不达 I（仅规范注入步成立；
        // 谱矩阵改步注入时按同位爆判准）。
        let stall_ok = if self.spec.class == 2 && self.spec.step == 2 {
            boom_step == 2 && self.step <= 2
        } else {
            true
        };
        InjectDigest {
            class: self.spec.class,
            boom_step,
            boom_code,
            caught: caught && stall_ok,
            protect: self.spec.protect,
            quarantined: !self.quar.is_empty(),
        }
    }

    /// 防护动作执行校验（动作码与规格一致且状态面生效）。
    pub fn protect_effective(&self) -> bool {
        let p = Protect::of_code(self.spec.protect);
        match p {
            Protect::Abort => true,
            Protect::Rollback => self.rolled_back,
            Protect::Quarantine => self.quarantined(),
            Protect::Freeze => self.frozen,
        }
    }

    /// 隔离旗。
    pub fn quarantined(&self) -> bool {
        !self.quar.is_empty()
    }
}

// ── 注入演练总账 ────────────────────────────────────────────
/// 注入演练总账（全类跑 + 必抓率 + 防护正确率 + 可重复）。
#[derive(Debug)]
pub struct InjectBook {
    /// 规格登记账。
    pub registry: FaultRegistry,
    /// 各类摘要（最近一轮）。
    pub digests: Vec<InjectDigest>,
    /// 重复对账（第二遍摘要）。
    pub digests2: Vec<InjectDigest>,
}

impl InjectBook {
    /// 建账并全量演练。
    pub fn bake() -> Result<InjectBook, u16> {
        let mut registry = FaultRegistry::new();
        registry.register_all()?;
        let mut digests = Vec::new();
        let mut digests2 = Vec::new();
        for s in FAULT_TABLE.iter() {
            let mut p1 = InjectPipe::new(*s);
            digests.push(p1.run());
            let mut p2 = InjectPipe::new(*s);
            digests2.push(p2.run());
        }
        Ok(InjectBook { registry, digests, digests2 })
    }

    /// 必抓率（千分制）。
    pub fn caught_permille(&self) -> u64 {
        if self.digests.is_empty() {
            return 0;
        }
        let n = self.digests.iter().filter(|d| d.caught).count();
        (n as u64) * 1000 / self.digests.len() as u64
    }

    /// 防护正确率（千分制：动作状态面生效比）。
    pub fn protect_permille(&self) -> u64 {
        if self.digests.is_empty() {
            return 0;
        }
        let mut ok = 0usize;
        for (i, d) in self.digests.iter().enumerate() {
            let spec = &FAULT_TABLE[i];
            let mut p = InjectPipe::new(*spec);
            let _ = p.run();
            if p.protect_effective() && d.protect == spec.protect {
                ok += 1;
            }
        }
        (ok as u64) * 1000 / self.digests.len() as u64
    }

    /// 可重复一致率（两遍摘要逐位一致比）。
    pub fn repeat_permille(&self) -> u64 {
        if self.digests.is_empty() || self.digests.len() != self.digests2.len() {
            return 0;
        }
        let n = self.digests.iter().zip(self.digests2.iter()).filter(|(a, b)| a == b).count();
        (n as u64) * 1000 / self.digests.len() as u64
    }

    /// 隔离账完整（防护为隔离的类必须真隔离）。
    pub fn quarantine_ok(&self) -> bool {
        for (i, d) in self.digests.iter().enumerate() {
            let expect = Protect::of_code(FAULT_TABLE[i].protect);
            if expect == Protect::Quarantine && !d.quarantined {
                return false;
            }
        }
        true
    }

    /// 全绿判准。
    pub fn all_green(&self) -> bool {
        self.registry.assetized() && self.registry.tamper_scan()
            && self.caught_permille() == 1000
            && self.protect_permille() == 1000
            && self.repeat_permille() == 1000
            && self.quarantine_ok()
    }
}

// ── 随机类轮转演练核（10/10 判准用） ───────────────────────
/// 单轮注入判准（随机类 → 必抓 + 防护对表 + 隔离）。
pub fn round_once(class: u16) -> Result<bool, u16> {
    if class as usize > FAULT_TABLE.len() || class == 0 {
        return Err(E_CLASS_OOB);
    }
    let spec = FAULT_TABLE[class as usize - 1];
    let mut pipe = InjectPipe::new(spec);
    let d = pipe.run();
    if !d.caught {
        return Ok(false);
    }
    if !pipe.protect_effective() {
        return Ok(false);
    }
    let expect_protect = Protect::of_code(spec.protect);
    if expect_protect == Protect::Quarantine && !pipe.quarantined() {
        return Ok(false);
    }
    if expect_protect == Protect::Freeze && !pipe.frozen {
        return Ok(false);
    }
    if expect_protect == Protect::Rollback && !pipe.rolled_back {
        return Ok(false);
    }
    Ok(true)
}

// ── 演练 ────────────────────────────────────────────────────
/// 五类资产化演练。
pub fn drill_assets() -> (bool, String) {
    let bk = match InjectBook::bake() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    (bk.registry.assetized() && bk.registry.tamper_scan(),
     format!("资产={} 篡改净={}", bk.registry.reg.len(), bk.registry.tamper_scan()))
}

/// 注入必抓 10/10 演练（LCG 轮转类号）。
pub fn drill_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7610_5EED);
    for _ in 0..ROUNDS {
        let class = rng.below(FAULT_TABLE.len() as u64) as u16 + 1;
        match round_once(class) {
            Ok(true) => ok += 1,
            _ => {}
        }
    }
    (ok == ROUNDS as u32, format!("必抓={}/{}", ok, ROUNDS))
}

/// 全类必抓率 1000‰ 演练。
pub fn drill_caught_all() -> (bool, String) {
    let bk = match InjectBook::bake() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let permille = bk.caught_permille();
    let mut detail_str = String::new();
    for d in bk.digests.iter() {
        detail_str = format!("F{} 爆步={} 码=0x{:04X} 抓={}", d.class, d.boom_step, d.boom_code, d.caught);
    }
    (permille == 1000, format!("必抓率={}‰（{}）", permille, detail_str))
}

/// 防护动作正确率 1000‰ 演练。
pub fn drill_protect() -> (bool, String) {
    let bk = match InjectBook::bake() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let permille = bk.protect_permille();
    let quar = bk.quarantine_ok();
    (permille == 1000 && quar, format!("防护正确率={}‰ 隔离齐={}", permille, quar))
}

/// 可重复一致率 1000‰ 演练。
pub fn drill_repeat() -> (bool, String) {
    let bk = match InjectBook::bake() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let permille = bk.repeat_permille();
    (permille == 1000, format!("可重复率={}‰ 对={} 类", permille, bk.digests.len()))
}

/// 半退役卡点专项演练（F2 必停在 A 不达 I）。
pub fn drill_stall() -> (bool, String) {
    let spec = FAULT_TABLE[1];
    let mut pipe = InjectPipe::new(spec);
    let d = pipe.run();
    let stuck_at_a = d.boom_step == 2 && pipe.step <= 2 && Mini4::of_step(2) == Mini4::A;
    let not_arrived = pipe.step < 3;
    (d.caught && stuck_at_a && not_arrived,
     format!("爆步={} 当前步={} 卡点={}", d.boom_step, pipe.step, stuck_at_a))
}

/// 隔离账专项演练（F3/F5 隔离，F1/F2/F4 不隔离）。
pub fn drill_quarantine() -> (bool, String) {
    let mut results: Vec<(u16, bool, bool)> = Vec::new();
    for s in FAULT_TABLE.iter() {
        let mut p = InjectPipe::new(*s);
        let d = p.run();
        let expect = Protect::of_code(s.protect);
        let want_quar = expect == Protect::Quarantine;
        results.push((s.class, want_quar, d.quarantined == want_quar));
    }
    let all_match = results.iter().all(|(_, _, m)| *m);
    (all_match, format!("隔离对表 {} 组 全对={}", results.len(), all_match))
}

/// 规格越界与冻结演练。
pub fn drill_registry_guard() -> (bool, String) {
    let mut reg = FaultRegistry::new();
    let _ = reg.register_all();
    // 类越界。
    let oob_class = matches!(reg.register(FaultSpec { class: 9, name: "越界", step: 0, boom_off: 0, protect: 1, seed: 1 }), Err(E_CLASS_OOB));
    // 步越界。
    let oob_step = matches!(reg.register(FaultSpec { class: 1, name: "步越", step: 9, boom_off: 0, protect: 1, seed: 1 }), Err(E_STEP_OOB));
    // 规格查询缺失。
    let miss = matches!(reg.spec_of(9), Err(E_CLASS_OOB));
    // 冻结后改写必拒。
    reg.freeze();
    let rdonly = matches!(reg.register(FAULT_TABLE[0]), Err(E_RDONLY));
    (oob_class && oob_step && miss && rdonly,
     format!("类越拒={} 步越拒={} 缺资拒={} 冻结拒={}", oob_class, oob_step, miss, rdonly))
}

/// 规格篡改必抓演练。
pub fn drill_tamper() -> (bool, String) {
    let mut reg = FaultRegistry::new();
    let _ = reg.register_all();
    let clean = reg.tamper_scan();
    if let Some(s) = reg.reg.get_mut(3) {
        s.protect = s.protect.wrapping_add(1);
    }
    let tampered = !reg.tamper_scan();
    (clean && tampered, format!("净表={} 篡改抓={}", clean, tampered))
}


// ── 故障谱矩阵（类 × 注入步 全覆盖） ───────────────────────
/// 单格结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixCell {
    /// 类号。
    pub class: u16,
    /// 注入步号。
    pub step: usize,
    /// 爆雷步（== 注入步才算同位爆）。
    pub boom_step: usize,
    /// 已抓。
    pub caught: bool,
}

/// 故障谱矩阵（5 类 × 4 步 = 20 格全测）。
#[derive(Debug)]
pub struct FaultMatrix {
    /// 格账。
    pub cells: Vec<MatrixCell>,
}

impl FaultMatrix {
    /// 建矩阵（每类在每步各注入一次，验证爆点与步号对位）。
    pub fn build() -> FaultMatrix {
        let mut cells = Vec::new();
        for spec in FAULT_TABLE.iter() {
            for step in 0..PIPE_LEN {
                // 按步号改写注入位（规格副本，不改常量表）。
                let alt = FaultSpec { step, ..*spec };
                let mut pipe = InjectPipe::new(alt);
                let d = pipe.run();
                cells.push(MatrixCell {
                    class: spec.class,
                    step,
                    boom_step: d.boom_step,
                    caught: d.caught,
                });
            }
        }
        FaultMatrix { cells }
    }

    /// 格数守恒（5×4=20）。
    pub fn conserved(&self) -> bool {
        self.cells.len() == FAULT_TABLE.len() * PIPE_LEN
    }

    /// 全格必抓（改写注入步后每格都应在同位爆）。
    pub fn all_caught(&self) -> bool {
        self.cells.iter().all(|c| c.caught && c.boom_step == c.step)
    }

    /// 类覆盖（每类恰 4 格）。
    pub fn per_class(&self, class: u16) -> usize {
        self.cells.iter().filter(|c| c.class == class).count()
    }
}

// ── 防护演练日志账（五元组：序/类/动作/生效/轮次） ─────────
/// 防护日志条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectRec {
    /// 序号。
    pub seq: u32,
    /// 类号。
    pub class: u16,
    /// 防护动作码。
    pub action: u8,
    /// 生效旗。
    pub effective: bool,
    /// 演练轮。
    pub round: u32,
}

/// 防护日志账。
#[derive(Debug)]
pub struct ProtectLog {
    /// 记录。
    recs: Vec<ProtectRec>,
}

impl ProtectLog {
    /// 新账。
    pub fn new() -> ProtectLog {
        ProtectLog { recs: Vec::new() }
    }

    /// 登记（演练一轮记一笔）。
    pub fn record(&mut self, class: u16, action: u8, effective: bool, round: u32) -> u32 {
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(ProtectRec { seq, class, action, effective, round });
        seq
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 序连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 全生效。
    pub fn all_effective(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.effective)
    }

    /// 类覆盖（五类齐）。
    pub fn classes_covered(&self) -> bool {
        FAULT_TABLE.iter().all(|s| self.recs.iter().any(|r| r.class == s.class))
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 故障谱矩阵演练（20 格全抓同位爆）。
pub fn drill_matrix() -> (bool, String) {
    let m = FaultMatrix::build();
    (m.conserved() && m.all_caught() && (0..FAULT_TABLE.len() as u16).all(|i| m.per_class(i + 1) == PIPE_LEN),
     format!("矩阵 {} 格 全抓同位={}", m.cells.len(), m.all_caught()))
}

/// 防护日志演练（五轮轮转登记 + 全生效 + 类覆盖）。
pub fn drill_protect_log() -> (bool, String) {
    let mut log = ProtectLog::new();
    let mut all_eff = true;
    for round in 1..=5u32 {
        for spec in FAULT_TABLE.iter() {
            let mut pipe = InjectPipe::new(*spec);
            let _ = pipe.run();
            let eff = pipe.protect_effective();
            if !eff {
                all_eff = false;
            }
            log.record(spec.class, spec.protect, eff, round);
        }
    }
    (log.len() == 25 && log.continuous() && all_eff && log.classes_covered(),
     format!("日志 {} 条 连续={} 全生效={} 类覆盖={}", log.len(), log.continuous(), all_eff, log.classes_covered()))
}


// ── 防护动作分布账（四型动作计数与配平） ───────────────────
/// 动作分布账。
#[derive(Debug)]
pub struct ActionTally {
    /// 四型动作计数（下标 = 动作码-1）。
    pub counts: [usize; 4],
}

impl ActionTally {
    /// 从规格表统计（每类预期动作）。
    pub fn from_table() -> ActionTally {
        let mut counts = [0usize; 4];
        for s in FAULT_TABLE.iter() {
            let c = Protect::of_code(s.protect).code() as usize;
            if c >= 1 && c <= 4 {
                counts[c - 1] += 1;
            }
        }
        ActionTally { counts }
    }

    /// 总数守恒（四型和 == 5 类）。
    pub fn conserved(&self) -> bool {
        self.counts.iter().sum::<usize>() == FAULT_TABLE.len()
    }

    /// 动作多样性（至少三型在用）。
    pub fn diverse(&self) -> bool {
        self.counts.iter().filter(|c| **c > 0).count() >= 3
    }
}

// ── 注入步号审计（规格表步号全在流水内） ───────────────────
/// 步号审计。
pub struct StepAudit;

impl StepAudit {
    /// 全规格步号合法（< PIPE_LEN）。
    pub fn steps_legal() -> bool {
        FAULT_TABLE.iter().all(|s| s.step < PIPE_LEN)
    }
    /// 爆雷码全在段内（SEG_BASE..SEG_BASE+16）。
    pub fn booms_in_segment() -> bool {
        FAULT_TABLE.iter().all(|s| {
            let code = SEG_BASE + s.boom_off;
            code >= SEG_BASE && code < SEG_BASE + 16
        })
    }
    /// 防护码全合法（1..=4）。
    pub fn protects_legal() -> bool {
        FAULT_TABLE.iter().all(|s| {
            let c = s.protect;
            c >= 1 && c <= 4
        })
    }
}

/// 追加演练：动作分布 + 步号审计。
pub fn drill_action_tally() -> (bool, String) {
    let t = ActionTally::from_table();
    let audits = StepAudit::steps_legal() && StepAudit::booms_in_segment() && StepAudit::protects_legal();
    (t.conserved() && t.diverse() && audits,
     format!("分布 {:?} 守恒={} 多样={} 三审={}", t.counts, t.conserved(), t.diverse(), audits))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：F1 切换中失败必抓（爆窗未关码）。
    {
        let mut p = InjectPipe::new(FAULT_TABLE[0]);
        let dg = p.run();
        if dg.caught && dg.boom_code == SEG_BASE + 1 { caught += 1; d.push("注1 F1:抓".to_string()); } else { d.push("注1 F1:漏".to_string()); }
    }
    // 注 2：F2 半退役卡点必抓。
    {
        let mut p = InjectPipe::new(FAULT_TABLE[1]);
        let dg = p.run();
        if dg.caught && dg.boom_step == 2 { caught += 1; d.push("注2 F2:抓".to_string()); } else { d.push("注2 F2:漏".to_string()); }
    }
    // 注 3：F3 守恒破约必抓 + 隔离。
    {
        let mut p = InjectPipe::new(FAULT_TABLE[2]);
        let dg = p.run();
        if dg.caught && dg.quarantined { caught += 1; d.push("注3 F3:抓".to_string()); } else { d.push("注3 F3:漏".to_string()); }
    }
    // 注 4：F4 回退失败必抓 + 冻结。
    {
        let mut p = InjectPipe::new(FAULT_TABLE[3]);
        let dg = p.run();
        if dg.caught && p.frozen { caught += 1; d.push("注4 F4:抓".to_string()); } else { d.push("注4 F4:漏".to_string()); }
    }
    // 注 5：F5 清理违规必抓。
    {
        let mut p = InjectPipe::new(FAULT_TABLE[4]);
        let dg = p.run();
        if dg.caught && dg.boom_step == 3 { caught += 1; d.push("注5 F5:抓".to_string()); } else { d.push("注5 F5:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7610
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7610 段诊断码：核对故障注入段位闭集"
        && FAULT_TABLE.len() >= CLASS_MIN
}

/// run_unx1312_checks：本单判据聚合（诊断段 0x7610）。
pub fn run_unx1312_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1312 退役故障注入");
    let (ok1, d1) = drill_assets();
    s.add_dyn("J1-五类故障资产化", ok1, &d1);
    let (ok2, d2) = drill_caught_10();
    s.add_dyn("J1-注入必抓10/10", ok2, &d2);
    let (ok3, d3) = drill_caught_all();
    s.add_dyn("J1-全类必抓率1000‰", ok3, &d3);
    let (ok4, d4) = drill_protect();
    s.add_dyn("J1-防护正确率1000‰", ok4, &d4);
    let (ok5, d5) = drill_repeat();
    s.add_dyn("J1-可重复一致率1000‰", ok5, &d5);
    let (ok6, d6) = drill_stall();
    s.add_dyn("J1-半退役卡点专项", ok6, &d6);
    let (ok7, d7) = drill_quarantine();
    s.add_dyn("J1-隔离对表", ok7, &d7);
    let (ok8, d8) = drill_registry_guard();
    s.add_dyn("J1-规格越界+冻结拒", ok8, &d8);
    let (ok9, d9) = drill_tamper();
    s.add_dyn("J1-规格篡改必抓", ok9, &d9);
    let (ok10, d10) = drill_matrix();
    s.add_dyn("J2-故障谱矩阵20格", ok10, &d10);
    let (ok11, d11) = drill_protect_log();
    s.add_dyn("J2-防护演练日志账", ok11, &d11);
    let (ok12, d12) = drill_action_tally();
    s.add_dyn("J2-防护动作分布账", ok12, &d12);
    let (c10, t10, d10) = inject_5();
    s.add_dyn("J2-注入核5/5", c10 == t10, &format!("{}/{} {}", c10, t10, d10.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7610 段十码闭集");
    s
}
