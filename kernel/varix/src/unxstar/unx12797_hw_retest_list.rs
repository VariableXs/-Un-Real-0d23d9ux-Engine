//! UNX-F12797 真机判据随闸门补测清单 · 0x83140段
//!
//! 判据原文（派工卡 12797 逐字锚定）：
//!   UNX-F12798-J1 互引预验绿（F12001–F12799 全扫描），悬空注入 10/10 检出。
//!
//! 设计要点：
//! 1. 清单登记表三列闭集：判据号 / 闸门位 / 预算三列成行，40 行冻结表
//!    （RT_CAP 闭集容量，行号只由 SEG_BASE 种子确定，内容逐位可复现）；
//!    判据号悬空、闸门位越闭集、预算为零或超上限、判据号重复、容量耗尽
//!    五道闸全部显性拒，各占一个诊断码；
//! 2. 随闸门推进语义：闸门位闭集 G0 静态 → G1 单元 → G2 集成 → G3 收官
//!    四档，逐闸推进（advance_to 只许 +1），跳级 EJump、倒退 EREWIND
//!    显性拒，到位闸门单调不回退；
//! 3. 判定器 judge_gate(id, gate) -> Result<GateState, u32>：闸门位不在
//!    闭集 → EINVAL 语义码；判据未登记 → ENOENT 语义码；判定闸门位低于
//!    已到位闸门 → EREWIND 语义码；三级次序守恒（先闭集、再登记、后倒退）；
//!    仅当判据所在闸门位 ≥ 当前已到位闸门才放行（Released），否则扣留
//!    （Held）转积压通道；
//! 4. 互引预验：清单判据号对 F12001–F12799 号面闭窗（窗宽 799）全扫描，
//!    全部落窗、零悬空、零重复；注入悬空号（窗沿两侧 + 0 + u32::MAX）
//!    必被号面闸与登记闸同时抓住，10/10 检出；
//! 5. 注入器三类各 10 轮全抓：悬空号注入（EWindow）、倒退闸门注入
//!    （判定与推进双口 EREWIND）、闭集外闸门位注入（判定/登记/推进三口
//!    EINVAL）；LCG 种子固定（偏移自 SEG_BASE），序列确定，双跑一致；
//! 6. 双跑一致指纹：FNV-1a 折叠清单指纹 + 四闸逐行判定流 + 三类注入
//!    战果，全流程确定性，两次运行逐位相同；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定）；
//!    取值一律 match / if let / .get() / checked_* / saturating_*，
//!    无 unwrap/expect/panic/裸下标/#[allow]。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12797 − 12600) = 0x83140）。
pub const SEG_BASE: u32 = 0x83140;

// ── 号面闭窗常量 ─────────────────────────────────────────────────
/// 号面闭窗下沿：判据号 F12001（派工总纲 VX9 卡面 12600–12799 的判据号域起点）。
pub const CRIT_LO: u32 = 12001;
/// 号面闭窗上沿：判据号 F12799（VX9 批末卡判据号域终点）。
pub const CRIT_HI: u32 = 12799;

/// 判据号是否落在号面闭窗 [F12001, F12799] 内（互引预验的落窗谓词）。
pub fn crit_in_window(crit: u32) -> bool {
    crit >= CRIT_LO && crit <= CRIT_HI
}

// ── 补测清单诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtDiag {
    /// 闸门位越 G0–G3 闭集（EINVAL 语义码）。
    EGateRange,
    /// 判据号未登记（ENOENT 语义码）。
    ENoEntry,
    /// 闸门倒退（EREWIND 语义码：判定/推进闸门位低于已到位闸门）。
    ERewind,
    /// 判据号悬空：越 F12001–F12799 号面闭窗。
    EWindow,
    /// 判据号重复登记。
    EDup,
    /// 预算列为零或超预算上限。
    EBudget,
    /// 清单容量耗尽（40 行闭集）。
    EFull,
    /// 清单已冻结，写入被拒。
    ELocked,
    /// 清单未满 40 行禁止冻结。
    ENotFull,
    /// 闸门推进越级（一次只许进一闸）。
    EJump,
    /// 双跑指纹不一致（全流程确定性被破坏）。
    EFingerprint,
    /// 注入逃脱（注入器没抓住人为构造的违约样本）。
    EInject,
}

impl RtDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RtDiag::EGateRange => 0x01,
                RtDiag::ENoEntry => 0x02,
                RtDiag::ERewind => 0x03,
                RtDiag::EWindow => 0x04,
                RtDiag::EDup => 0x05,
                RtDiag::EBudget => 0x06,
                RtDiag::EFull => 0x07,
                RtDiag::ELocked => 0x08,
                RtDiag::ENotFull => 0x09,
                RtDiag::EJump => 0x0a,
                RtDiag::EFingerprint => 0x0b,
                RtDiag::EInject => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RtDiag::EGateRange => "闸门位越 G0–G3 闭集（EINVAL）：只认 0=静态/1=单元/2=集成/3=收官，先归位闸门位再判定",
            RtDiag::ENoEntry => "判据号未登记（ENOENT）：号在窗内但不在清单，先补登记行（号/闸门位/预算三列）再判定",
            RtDiag::ERewind => "闸门倒退（EREWIND）：判定或推进的闸门位低于已到位闸门，随闸门推进只进不退，改走当前闸门",
            RtDiag::EWindow => "判据号悬空：越 F12001–F12799 号面闭窗，先回对表取正确判据号再登记",
            RtDiag::EDup => "判据号重复：每号只落一行，重复登记先查已有行再决定改号或并账",
            RtDiag::EBudget => "预算列非法：预算为零或超上限（1..=BUDGET_CAP），先核补测轮次预算再入表",
            RtDiag::EFull => "清单容量耗尽：40 行闭集已满，溢出行转下一张补测清单滚动",
            RtDiag::ELocked => "清单已冻结：冻结后写入拒绝，改动走新开清单，冻结表只读",
            RtDiag::ENotFull => "清单未满 40 行禁止冻结：先补齐登记行到容量再走冻结口",
            RtDiag::EJump => "闸门推进越级：一次只许进一闸（+1），跳级先回补中间闸门的补测结论",
            RtDiag::EFingerprint => "双跑指纹不一致：全流程应逐位确定，先查非确定源（种子/浮点/墙钟）再重放",
            RtDiag::EInject => "注入逃脱：人为违约样本未被判定器抓住，判定面失效须整表复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RT_ALL: &[RtDiag] = &[
    RtDiag::EGateRange,
    RtDiag::ENoEntry,
    RtDiag::ERewind,
    RtDiag::EWindow,
    RtDiag::EDup,
    RtDiag::EBudget,
    RtDiag::EFull,
    RtDiag::ELocked,
    RtDiag::ENotFull,
    RtDiag::EJump,
    RtDiag::EFingerprint,
    RtDiag::EInject,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x83141..0x8314C 闭集内"
}

// ── 闸门位闭集 ───────────────────────────────────────────────────
/// 闸门位四档闭集（随闸门推进的档位，冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// 第 0 档：G0 静态闸。
    G0Static,
    /// 第 1 档：G1 单元闸。
    G1Unit,
    /// 第 2 档：G2 集成闸。
    G2Integ,
    /// 第 3 档：G3 收官闸。
    G3Final,
}

impl Gate {
    /// 档位号（闭集序 0..4）。
    pub fn index(self) -> u32 {
        match self {
            Gate::G0Static => 0,
            Gate::G1Unit => 1,
            Gate::G2Integ => 2,
            Gate::G3Final => 3,
        }
    }

    /// 档位中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            Gate::G0Static => "G0静态",
            Gate::G1Unit => "G1单元",
            Gate::G2Integ => "G2集成",
            Gate::G3Final => "G3收官",
        }
    }

    /// 由档位号反查闭集档（越闭集返回 None，即 EINVAL 来源）。
    pub fn from_index(idx: u32) -> Option<Gate> {
        match idx {
            0 => Some(Gate::G0Static),
            1 => Some(Gate::G1Unit),
            2 => Some(Gate::G2Integ),
            3 => Some(Gate::G3Final),
            _ => None,
        }
    }
}

/// 闸门位闭集数组（档序即推进序）。
pub const GATE_ALL: &[Gate] = &[
    Gate::G0Static,
    Gate::G1Unit,
    Gate::G2Integ,
    Gate::G3Final,
];

/// 判定器放行态：随闸门推进语义下的二值出口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    /// 放行：判据所在闸门位 ≥ 当前已到位闸门，补测可执行。
    Released,
    /// 不放行：判据所在闸门位已落后于已到位闸门，转积压通道随下闸再补。
    Held,
}

// ── 清单状态机 ───────────────────────────────────────────────────
/// 清单状态机：开表 → 满 → 冻结，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListState {
    /// 开表：允许逐行登记。
    Open,
    /// 已满 40 行：等待冻结。
    Full,
    /// 冻结：只读。
    Frozen,
}

impl ListState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ListState::Open => 0,
            ListState::Full => 1,
            ListState::Frozen => 2,
        }
    }
}

// ── 三列闭集数据模型 ─────────────────────────────────────────────
/// 清单容量：40 行冻结表（判据号/闸门位/预算三列闭集的行闭集）。
pub const RT_CAP: usize = 40;
/// 单判据补测预算上限（轮次闭集，预算列为 1..=BUDGET_CAP）。
pub const BUDGET_CAP: u32 = 64;

/// 单行三列单元：判据号 / 闸门位 / 预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtRow {
    /// 判据号（F12001..=F12799 原始编号，不含 F 前缀）。
    pub crit: u32,
    /// 闸门位（G0–G3 闭集档）。
    pub gate: Gate,
    /// 预算（补测轮次，1..=BUDGET_CAP）。
    pub budget: u32,
}

impl RtRow {
    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "F{}@{}预算{}",
            self.crit,
            self.gate.name(),
            self.budget
        )
    }
}

/// 真机判据随闸门补测清单（登记表 + 到位闸门 + 冻结态）。
#[derive(Clone, Debug)]
pub struct RetestList {
    /// 已登记行（按登记序，冻结后只读）。
    rows: Vec<RtRow>,
    /// 当前已到位闸门（随闸门推进，单调不回退）。
    reached: Gate,
    /// 清单状态。
    state: ListState,
}

impl RetestList {
    /// 开一张新清单（到位闸门初始为 G0，状态开表）。
    pub fn new() -> RetestList {
        RetestList {
            rows: Vec::new(),
            reached: Gate::G0Static,
            state: ListState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ListState {
        self.state
    }

    /// 当前已到位闸门。
    pub fn reached(&self) -> Gate {
        self.reached
    }

    /// 已登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None，禁裸下标）。
    pub fn row_at(&self, idx: usize) -> Option<RtRow> {
        self.rows.get(idx).copied()
    }

    /// 按判据号查行（线性扫，越窗号自然查不到）。
    pub fn row_of(&self, crit: u32) -> Option<RtRow> {
        for r in self.rows.iter() {
            if r.crit == crit {
                return Some(*r);
            }
        }
        None
    }

    /// 三列登记一行（冻结/悬空/闸门位/预算/重复/容量六道闸）。
    pub fn register(&mut self, crit: u32, gate: u32, budget: u32) -> Result<(), u32> {
        // 冻结闸：冻结表只读。
        if self.state.rank() >= ListState::Frozen.rank() {
            return Err(RtDiag::ELocked.val());
        }
        // 号面闸：判据号必须落 F12001–F12799 闭窗（悬空显性拒）。
        if !crit_in_window(crit) {
            return Err(RtDiag::EWindow.val());
        }
        // 闸门位闭集闸：不在 G0–G3 即 EINVAL 语义。
        let g = Gate::from_index(gate).ok_or(RtDiag::EGateRange.val())?;
        // 预算闸：1..=BUDGET_CAP 闭集。
        if budget == 0 || budget > BUDGET_CAP {
            return Err(RtDiag::EBudget.val());
        }
        // 重复闸：每判据号只落一行。
        if self.row_of(crit).is_some() {
            return Err(RtDiag::EDup.val());
        }
        // 容量闸：40 行闭集。
        if self.rows.len() >= RT_CAP {
            return Err(RtDiag::EFull.val());
        }
        self.rows.push(RtRow {
            crit,
            gate: g,
            budget,
        });
        // 满行即推进状态机（只进不退）。
        if self.rows.len() == RT_CAP {
            self.state = ListState::Full;
        }
        Ok(())
    }

    /// 随闸门推进：把已到位闸门推到 gate（逐闸 +1，跳级/倒退显性拒）。
    pub fn advance_to(&mut self, gate: u32) -> Result<(), u32> {
        // 闭集闸：越 G0–G3 即 EINVAL 语义。
        let g = Gate::from_index(gate).ok_or(RtDiag::EGateRange.val())?;
        // 倒退闸：低于已到位闸门即 EREWIND 语义。
        if gate < self.reached.index() {
            return Err(RtDiag::ERewind.val());
        }
        // 越级闸：一次只许进一闸。
        if gate > self.reached.index().saturating_add(1) {
            return Err(RtDiag::EJump.val());
        }
        self.reached = g;
        Ok(())
    }

    /// 冻结：满 40 行才许冻结；冻结后写入全拒。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ListState::Frozen.rank() {
            return Err(RtDiag::ELocked.val());
        }
        if self.rows.len() != RT_CAP {
            return Err(RtDiag::ENotFull.val());
        }
        self.state = ListState::Frozen;
        Ok(())
    }

    /// 判定器：判据 id 在闸门位 gate 的补测放行判定。
    /// 次序守恒：闸门位闭集（EINVAL）→ 登记（ENOENT）→ 倒退（EREWIND）
    /// → 放行（判据闸门位 ≥ 已到位闸门才 Released，否则 Held）。
    pub fn judge_gate(&self, id: u32, gate: u32) -> Result<GateState, u32> {
        // 第一级：闸门位闭集（EINVAL 语义）。
        let _g = Gate::from_index(gate).ok_or(RtDiag::EGateRange.val())?;
        // 第二级：判据号登记（ENOENT 语义）。
        let row = self.row_of(id).ok_or(RtDiag::ENoEntry.val())?;
        // 第三级：闸门倒退（EREWIND 语义：判定闸门位低于已到位闸门）。
        if gate < self.reached.index() {
            return Err(RtDiag::ERewind.val());
        }
        // 放行判据：判据所在闸门位 ≥ 当前已到位闸门才放行。
        if row.gate.index() >= self.reached.index() {
            Ok(GateState::Released)
        } else {
            Ok(GateState::Held)
        }
    }

    /// 清单指纹：FNV-1a 折叠状态/到位闸门/全部三列行，双跑一致口径。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fnv_u32(h, self.state.rank() as u32);
        h = fnv_u32(h, self.reached.index());
        h = fnv_u32(h, self.rows.len() as u32);
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_u32(h, r.crit);
                h = fnv_u32(h, r.gate.index());
                h = fnv_u32(h, r.budget);
            }
            i += 1;
        }
        h
    }
}

// ── 确定性 LCG（种子固定，零随机红线合规） ────────────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性；常数取 Knuth MMIX 31 位推荐闭集）。
    pub fn draw(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.draw() % bound
    }
}

// ── 冻结表构建（确定性，内容逐位可复现） ─────────────────────────
/// 构建满 40 行未冻结清单：判据号取 313k+17 mod 799 的双射族
/// （gcd(313,799)=1 ⇒ 40 号两两互异全落窗），闸门位 k mod 4 轮转，
/// 预算 LCG 定值 1..=8。
pub fn build_full_list() -> RetestList {
    let mut l = RetestList::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut k = 0u32;
    while k < RT_CAP as u32 {
        let crit = CRIT_LO + (k.wrapping_mul(313).wrapping_add(17)) % 799;
        let gate = k % 4;
        let budget = 1 + rng.below(8);
        let _ = l.register(crit, gate, budget);
        k += 1;
    }
    l
}

/// 构建满 40 行冻结清单（冻结表内容只由 SEG_BASE 种子决定）。
pub fn build_frozen_list() -> RetestList {
    let mut l = build_full_list();
    let _ = l.freeze();
    l
}

// ── FNV-1a 折叠（双跑指纹） ──────────────────────────────────────
/// FNV-1a 偏移基（64 位标准常数）。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数（64 位标准常数）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折叠单字节。
fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折叠 u32（小端四字节）。
fn fnv_u32(h: u64, v: u32) -> u64 {
    let mut h = h;
    h = fnv_byte(h, (v & 0xFF) as u8);
    h = fnv_byte(h, ((v >> 8) & 0xFF) as u8);
    h = fnv_byte(h, ((v >> 16) & 0xFF) as u8);
    fnv_byte(h, ((v >> 24) & 0xFF) as u8)
}

/// 折叠 u64（小端八字节）。
fn fnv_u64(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut s = 0u32;
    while s < 64 {
        h = fnv_byte(h, ((v >> s) & 0xFF) as u8);
        s += 8;
    }
    h
}

/// 互引预验扫描：清单判据号对 F12001–F12799 号面闭窗全扫。
/// 返回 (悬空行数, 扫描行数, 首个悬空描述)；零悬空时描述为空串。
pub fn xref_scan(l: &RetestList) -> (usize, usize, String) {
    let mut dangling = 0usize;
    let mut scanned = 0usize;
    let mut first = String::new();
    let mut i = 0usize;
    while i < l.len() {
        match l.row_at(i) {
            Some(r) => {
                scanned += 1;
                if !crit_in_window(r.crit) {
                    dangling += 1;
                    if first.is_empty() {
                        first = format!("行{}判据号{}悬空", i, r.crit);
                    }
                }
            }
            None => {
                // 越界不可达（i < len），防御性计悬空。
                dangling += 1;
            }
        }
        i += 1;
    }
    (dangling, scanned, first)
}

/// 全流程折叠：冻结表构建 → 四闸逐闸推进并逐行判定 → 三类注入战果，
/// 全部折进 FNV-1a；种子固定 ⇒ 双跑逐位一致（EFingerprint 的直读口径）。
pub fn pipeline_digest() -> u64 {
    let mut h = FNV_OFF;
    let mut l = build_frozen_list();
    h = fnv_u64(h, l.fingerprint());
    // 四闸逐闸推进：到位 G0 起逐行判定，再进 G1/G2/G3。
    let mut g = 0u32;
    while g < 4 {
        match l.advance_to(g) {
            Ok(()) => h = fnv_u32(h, 0xAA55_0000 + g),
            Err(e) => h = fnv_u32(h, e),
        }
        let mut i = 0usize;
        while i < l.len() {
            let crit = l.row_at(i).map(|r| r.crit).unwrap_or(0);
            match l.judge_gate(crit, g) {
                Ok(st) => {
                    let tag = if st == GateState::Released { 1 } else { 2 };
                    h = fnv_u32(h, tag);
                }
                Err(e) => h = fnv_u32(h, e),
            }
            h = fnv_u32(h, crit);
            i += 1;
        }
        g += 1;
    }
    // 三类注入战果折叠（每类 10 轮全抓）。
    let (c1, t1, _) = inject_dangling_sweep();
    h = fnv_u32(h, c1 as u32);
    h = fnv_u32(h, t1 as u32);
    let (c2, t2, _) = inject_rewind_sweep();
    h = fnv_u32(h, c2 as u32);
    h = fnv_u32(h, t2 as u32);
    let (c3, t3, _) = inject_gaterange_sweep();
    h = fnv_u32(h, c3 as u32);
    h = fnv_u32(h, t3 as u32);
    h
}

// ── 注入面一：悬空号注入（10 轮全抓） ────────────────────────────
/// 悬空号注入器：每轮取窗外两侧轮转 + 0/u32::MAX 极值号，
/// 号面谓词与登记闸必须同时拒绝；返回 (抓到, 总轮, 首个逃脱描述)。
pub fn inject_dangling_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x21) as u64);
    let mut k = 0usize;
    while k < 10 {
        // 悬空样本：下沿侧 / 上沿侧 / 极值（0 或 u32::MAX）三路轮转。
        let crit = match k % 3 {
            0 => CRIT_LO - 1 - rng.below(64),
            1 => CRIT_HI + 1 + rng.below(64),
            _ => {
                if rng.below(2) == 0 {
                    0
                } else {
                    u32::MAX
                }
            }
        };
        let mut l = RetestList::new();
        // 登记闸必须以 EWindow 拒悬空号。
        let reg_bad = l.register(crit, 0, 1) == Err(RtDiag::EWindow.val());
        // 号面谓词必须独立判悬空（互引预验第二道独立抓手）。
        let xref_bad = !crit_in_window(crit);
        total += 1;
        if reg_bad && xref_bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}悬空号{}未被拒", k, crit);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入面二：倒退闸门注入（10 轮全抓） ──────────────────────────
/// 倒退闸门注入器：到位 G2 后，每轮以低于到位闸门的档位号做判定与
/// 推进，两口都必须以 EREWIND 拒；返回 (抓到, 总轮, 首个逃脱描述)。
pub fn inject_rewind_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x22) as u64);
    let mut l = build_frozen_list();
    // 先逐闸推进到 G2（0→1→2），倒退样本才有落点。
    if l.advance_to(1).is_err() || l.advance_to(2).is_err() {
        return (0, 10, "前置推进失败".to_string());
    }
    let mut k = 0usize;
    while k < 10 {
        // 倒退样本：0 或 1，均低于已到位 G2。
        let gate = rng.below(2);
        let idx = rng.below(l.len() as u32) as usize;
        let id = l.row_at(idx).map(|r| r.crit).unwrap_or(0);
        // 判定口必须以 EREWIND 拒倒退档位。
        let judge_bad = l.judge_gate(id, gate) == Err(RtDiag::ERewind.val());
        // 推进口必须以 EREWIND 拒倒退档位（到位闸门不动）。
        let adv_bad = l.advance_to(gate) == Err(RtDiag::ERewind.val());
        total += 1;
        if judge_bad && adv_bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}判据{}倒退闸{}未被拒", k, id, gate);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入面三：闭集外闸门位注入（10 轮全抓） ──────────────────────
/// 闭集外闸门位注入器：每轮取 4..=254 的越闭集档位号，判定/登记/
/// 推进三口都必须以 EINVAL 语义（EGateRange）拒；返回 (抓到, 总轮, 逃脱)。
pub fn inject_gaterange_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x23) as u64);
    let l = build_frozen_list();
    let mut k = 0usize;
    while k < 10 {
        // 越闭集样本：4..=254（G0–G3 之外）。
        let gate = 4 + rng.below(251);
        let idx = rng.below(l.len() as u32) as usize;
        let id = l.row_at(idx).map(|r| r.crit).unwrap_or(0);
        // 判定口必须以 EINVAL 拒（且先于 ENOENT——id 必是已登记号）。
        let judge_bad = l.judge_gate(id, gate) == Err(RtDiag::EGateRange.val());
        // 登记口必须以 EINVAL 拒（判据号取窗内合法号）。
        let mut fresh = RetestList::new();
        let pick = CRIT_LO + rng.below(799);
        let reg_bad = fresh.register(pick, gate, 1) == Err(RtDiag::EGateRange.val());
        // 推进口必须以 EINVAL 拒。
        let mut adv = build_frozen_list();
        let adv_bad = adv.advance_to(gate) == Err(RtDiag::EGateRange.val());
        total += 1;
        if judge_bad && reg_bad && adv_bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}越闭集闸{}未被拒", k, gate);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互异、全部落 [SEG_BASE, SEG_BASE+0x40) 窗、
/// 查表闭集命中已知码、未知码回落。
fn diag_closed_ok() -> bool {
    if RT_ALL.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < RT_ALL.len() {
        let vi = match RT_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        if vi < SEG_BASE || vi >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(vi) == "未知诊断码：不在 0x83141..0x8314C 闭集内" {
            return false;
        }
        let mut j = i + 1;
        while j < RT_ALL.len() {
            let vj = match RT_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码必须回落（取闭集外样本码）。
    detail(SEG_BASE + 0x3F).contains("未知")
}

/// 闸门位闭集守卫：四档齐、首尾对、反查往返一致、越闭集返回 None。
fn gate_closed_ok() -> bool {
    if GATE_ALL.len() != 4 {
        return false;
    }
    if GATE_ALL.first().map(|g| *g) != Some(Gate::G0Static) {
        return false;
    }
    if GATE_ALL.last().map(|g| *g) != Some(Gate::G3Final) {
        return false;
    }
    let mut i = 0u32;
    while i < 4 {
        match Gate::from_index(i) {
            Some(g) => {
                if g.index() != i {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    Gate::from_index(4).is_none() && Gate::from_index(u32::MAX).is_none()
}

/// 正常流：40 行登记 → 满态 → 冻结 → 只读。
fn happy_path_ok() -> bool {
    let l = build_frozen_list();
    if l.len() != RT_CAP || l.is_empty() {
        return false;
    }
    if l.state() != ListState::Frozen {
        return false;
    }
    if l.reached() != Gate::G0Static {
        return false;
    }
    // 冻结表逐行三列齐：号落窗、档在闭集、预算在 1..=BUDGET_CAP。
    let mut i = 0usize;
    while i < l.len() {
        match l.row_at(i) {
            Some(r) => {
                if !crit_in_window(r.crit) || r.budget == 0 || r.budget > BUDGET_CAP {
                    return false;
                }
                if Gate::from_index(r.gate.index()) != Some(r.gate) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 判定器放行语义：到位 G0 全放行；推进 G1 后 G0 行转 Held（10 行）
/// 其余仍放行（30 行）——"判据闸门位 ≥ 已到位闸门才放行"直读。
fn judge_semantics_ok() -> bool {
    let mut l = build_frozen_list();
    // 到位 G0：所有行判据闸门位 ≥ 0，全放行。
    let id0 = match l.row_at(0) {
        Some(r) => r.crit,
        None => return false,
    };
    if l.judge_gate(id0, 0) != Ok(GateState::Released) {
        return false;
    }
    // 推进到 G1（逐闸 +1）。
    if l.advance_to(1).is_err() {
        return false;
    }
    let mut held = 0usize;
    let mut released = 0usize;
    let mut i = 0usize;
    while i < l.len() {
        let crit = match l.row_at(i) {
            Some(r) => r.crit,
            None => return false,
        };
        match l.judge_gate(crit, 1) {
            Ok(GateState::Held) => held += 1,
            Ok(GateState::Released) => released += 1,
            Err(_) => return false,
        }
        i += 1;
    }
    // 闸门位 k mod 4 轮转 ⇒ 每档恰 10 行：G0 行扣留 10，放行 30。
    held == 10 && released == 30
}

/// 判定次序守恒：EINVAL 先于 ENOENT 先于 EREWIND，末级才谈放行。
fn judge_order_ok() -> bool {
    let l = build_frozen_list();
    // 越闭集档位 + 窗外未登记号：EINVAL 优先（第一级闸先落）。
    if l.judge_gate(CRIT_LO - 1, 9) != Err(RtDiag::EGateRange.val()) {
        return false;
    }
    // 闭集内档位 + 窗内未登记号：ENOENT（找窗内首个未登记号）。
    let fresh_id = match first_unregistered(&l) {
        Some(c) => c,
        None => return false,
    };
    if l.judge_gate(fresh_id, 1) != Err(RtDiag::ENoEntry.val()) {
        return false;
    }
    // 已登记号 + 倒退档位：EREWIND（先逐闸推进到 G2 制造倒退落点）。
    let mut l2 = build_frozen_list();
    if l2.advance_to(1).is_err() || l2.advance_to(2).is_err() {
        return false;
    }
    let reg_id = match l2.row_at(0) {
        Some(r) => r.crit,
        None => return false,
    };
    if l2.judge_gate(reg_id, 1) != Err(RtDiag::ERewind.val()) {
        return false;
    }
    // 三级全过：合法判定只出 Released/Held 二值，不出错。
    l2.judge_gate(reg_id, 3).is_ok()
}

/// 窗内首个未登记判据号（判定次序用样本）。
fn first_unregistered(l: &RetestList) -> Option<u32> {
    let mut c = CRIT_LO;
    while c <= CRIT_HI {
        if l.row_of(c).is_none() {
            return Some(c);
        }
        c += 1;
    }
    None
}

/// 互引预验绿：全落窗、零悬空、窗宽 799、判据号两两互异。
fn xref_green_ok() -> bool {
    // 窗常量闭集自检。
    if CRIT_LO >= CRIT_HI || CRIT_HI - CRIT_LO + 1 != 799 {
        return false;
    }
    let l = build_frozen_list();
    let (dangling, scanned, first) = xref_scan(&l);
    if dangling != 0 || scanned != RT_CAP || !first.is_empty() {
        return false;
    }
    // 判据号两两互异（零重复）。
    let mut i = 0usize;
    while i < l.len() {
        let ci = match l.row_at(i) {
            Some(r) => r.crit,
            None => return false,
        };
        let mut j = i + 1;
        while j < l.len() {
            let cj = match l.row_at(j) {
                Some(r) => r.crit,
                None => return false,
            };
            if ci == cj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 注入一：悬空号 10/10 全抓零逃脱。
fn inject_dangling_ok() -> bool {
    let (caught, total, escape) = inject_dangling_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入二：倒退闸门 10/10 全抓零逃脱。
fn inject_rewind_ok() -> bool {
    let (caught, total, escape) = inject_rewind_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入三：闭集外闸门位 10/10 全抓零逃脱。
fn inject_gaterange_ok() -> bool {
    let (caught, total, escape) = inject_gaterange_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 双跑一致指纹：全流程折叠指纹两次运行逐位相同（EFingerprint 口径）。
fn dual_run_ok() -> bool {
    let a = pipeline_digest();
    let b = pipeline_digest();
    if a != b {
        return false;
    }
    // 冻结表两次构建逐位同指纹。
    let l1 = build_frozen_list();
    let l2 = build_frozen_list();
    l1.fingerprint() == l2.fingerprint() && l1.len() == RT_CAP && l2.len() == RT_CAP
}

/// 边界四类：号面沿 / 预算沿 / 闸门位沿 / 推进沿。
fn boundary_ok() -> bool {
    // 号面沿：下沿 F12001、上沿 F12799 可登记；出沿一号即 EWindow。
    let mut l = RetestList::new();
    if l.register(CRIT_LO, 0, 1).is_err() {
        return false;
    }
    if l.register(CRIT_HI, 3, 1).is_err() {
        return false;
    }
    if l.register(CRIT_LO - 1, 0, 1) != Err(RtDiag::EWindow.val()) {
        return false;
    }
    if l.register(CRIT_HI + 1, 0, 1) != Err(RtDiag::EWindow.val()) {
        return false;
    }
    // 预算沿：1 与 BUDGET_CAP 可登记；0 与 CAP+1 即 EBudget。
    let mut b = RetestList::new();
    if b.register(12100, 0, 1).is_err() {
        return false;
    }
    if b.register(12101, 1, BUDGET_CAP).is_err() {
        return false;
    }
    if b.register(12102, 2, 0) != Err(RtDiag::EBudget.val()) {
        return false;
    }
    if b.register(12103, 2, BUDGET_CAP + 1) != Err(RtDiag::EBudget.val()) {
        return false;
    }
    // 闸门位沿：G0/G3 可登记；档位 4 越闭集即 EINVAL 语义。
    if b.register(12104, 0, 1).is_err() {
        return false;
    }
    if b.register(12105, 3, 1).is_err() {
        return false;
    }
    if b.register(12106, 4, 1) != Err(RtDiag::EGateRange.val()) {
        return false;
    }
    // 推进沿：跳级 EJump、倒退 EREWIND、逐闸到 G3 后原地推进是 no-op。
    let mut a = build_frozen_list();
    if a.advance_to(2) != Err(RtDiag::EJump.val()) {
        return false;
    }
    if a.advance_to(1).is_err() {
        return false;
    }
    if a.advance_to(0) != Err(RtDiag::ERewind.val()) {
        return false;
    }
    if a.advance_to(2).is_err() {
        return false;
    }
    if a.advance_to(3).is_err() {
        return false;
    }
    a.advance_to(3).is_ok() && a.reached() == Gate::G3Final
}

/// 容量耗尽：40 行满后新号 EFull、已登号 EDup（重复先于容量）。
fn capacity_ok() -> bool {
    let mut l = RetestList::new();
    let mut k = 0u32;
    while k < RT_CAP as u32 {
        let crit = CRIT_LO + k;
        if l.register(crit, k % 4, 1 + (k % 3)).is_err() {
            return false;
        }
        k += 1;
    }
    if l.len() != RT_CAP || l.state() != ListState::Full {
        return false;
    }
    // 新号（窗内未登）→ 容量闸拒。
    if l.register(CRIT_LO + 40, 0, 1) != Err(RtDiag::EFull.val()) {
        return false;
    }
    // 已登号 → 重复闸先拒。
    if l.register(CRIT_LO, 0, 1) != Err(RtDiag::EDup.val()) {
        return false;
    }
    true
}

/// 冻结写保护：未满禁冻结、满后冻结成、冻结后写拒、重冻结拒。
fn freeze_ok() -> bool {
    // 未满：ENotFull。
    let mut l = RetestList::new();
    let _ = l.register(12001, 0, 1);
    if l.freeze() != Err(RtDiag::ENotFull.val()) {
        return false;
    }
    // 满：冻结成功。
    let mut f = build_full_list();
    if f.freeze().is_err() {
        return false;
    }
    if f.state() != ListState::Frozen {
        return false;
    }
    // 冻结后登记 → ELocked 且行数不变。
    let before = f.len();
    if f.register(12600, 0, 1) != Err(RtDiag::ELocked.val()) {
        return false;
    }
    if f.len() != before {
        return false;
    }
    // 重冻结 → ELocked。
    f.freeze() == Err(RtDiag::ELocked.val())
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 本检查节条目总数（收口条的判据条数声明基准）。
pub const CHECK_COUNT: usize = 15;

/// UNX-F12798 自检（判据 12797 卡：互引预验绿 + 悬空注入 10/10 检出）。
pub fn run_unx12797_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12797");
    let list = build_frozen_list();

    s.add_dyn(
        "F12798-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={} win=[0x80000,0x831C0]", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12798-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} lo=0x{:05x} hi=0x{:05x}",
            RT_ALL.len(),
            RT_ALL.first().map(|d| d.val()).unwrap_or(0),
            RT_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12798-03 闸门位闭集",
        gate_closed_ok(),
        &format!(
            "gates={}首={}尾={}",
            GATE_ALL.len(),
            GATE_ALL.first().map(|g| g.name()).unwrap_or("?"),
            GATE_ALL.last().map(|g| g.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12798-04 清单登记正常流-40行冻结",
        happy_path_ok(),
        &format!("rows={} state={:?} reached={}", list.len(), list.state(), list.reached().name()),
    );

    s.add_dyn(
        "F12798-05 判定器放行语义",
        judge_semantics_ok(),
        &format!(
            "reached={} rows={} held10/released30",
            list.reached().name(),
            list.len()
        ),
    );

    s.add_dyn(
        "F12798-06 判定次序守恒",
        judge_order_ok(),
        "EINVAL→ENOENT→EREWIND 三级次序，末级才放行",
    );

    s.add_dyn(
        "F12798-07 互引预验全落窗零悬空",
        xref_green_ok(),
        &format!("窗F{}–F{} rows={}", CRIT_LO, CRIT_HI, list.len()),
    );

    // 注入一：悬空号 10 轮必须全数检出（人为构造违约必被抓）。
    let (c1, t1, e1) = inject_dangling_sweep();
    s.add_dyn(
        "F12798-08 注入一悬空号10/10",
        inject_dangling_ok() && c1 == t1,
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    // 注入二：倒退闸门 10 轮（判定+推进双口）必须全数检出。
    let (c2, t2, e2) = inject_rewind_sweep();
    s.add_dyn(
        "F12798-09 注入二倒退闸门10/10",
        inject_rewind_ok() && c2 == t2,
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    // 注入三：闭集外闸门位 10 轮（判定+登记+推进三口）必须全数检出。
    let (c3, t3, e3) = inject_gaterange_sweep();
    s.add_dyn(
        "F12798-10 注入三闭集外闸门位10/10",
        inject_gaterange_ok() && c3 == t3,
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12798-11 双跑一致指纹",
        dual_run_ok(),
        "FNV-1a 折叠全流程，两次运行逐位相同",
    );

    s.add_dyn(
        "F12798-12 边界四类",
        boundary_ok(),
        "号面沿/预算沿/闸门位沿/推进沿 四点",
    );

    s.add_dyn(
        "F12798-13 容量耗尽",
        capacity_ok(),
        &format!("cap={} 满后新号EFull/已登号EDup", RT_CAP),
    );

    s.add_dyn(
        "F12798-14 冻结写保护",
        freeze_ok(),
        "未满禁冻结/满后冻结成/冻结后写拒/重冻结拒",
    );

    // 收口：判据条数声明（本条自身计入，故比较时先 +1）。
    let declared = s.len() + 1 == CHECK_COUNT;
    s.add_dyn(
        "F12798-15 收口-判据条数声明",
        declared && list.len() == RT_CAP,
        &format!("本节{}条判据，清单{}行冻结", CHECK_COUNT, list.len()),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x83140);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 码互异、全落段窗、查表命中、未知回落。
        assert_eq!(RT_ALL.len(), 12);
        let mut i = 0usize;
        while i < RT_ALL.len() {
            let vi = RT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < RT_ALL.len() {
                let vj = RT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(detail(RtDiag::ERewind.val()), RtDiag::ERewind.detail());
    }

    #[test]
    fn happy_path_freezes_and_judges() {
        // 正常流：40 行满 → 冻结 → G1 到位后 G0 行扣留其余放行。
        let l = build_frozen_list();
        assert_eq!(l.len(), RT_CAP);
        assert_eq!(l.state(), ListState::Frozen);
        assert_eq!(l.reached(), Gate::G0Static);
        assert!(happy_path_ok());
        assert!(judge_semantics_ok());
        let id0 = l.row_at(0).map(|r| r.crit).unwrap_or(0);
        assert_eq!(l.judge_gate(id0, 0), Ok(GateState::Released));
        let id1 = l.row_at(1).map(|r| r.crit).unwrap_or(0);
        assert_eq!(l.judge_gate(id1, 3), Ok(GateState::Released));
    }

    #[test]
    fn judge_order_precedence_holds() {
        // 判定次序：EINVAL 先于 ENOENT 先于 EREWIND，末级才放行。
        let l = build_frozen_list();
        // 越闭集档位优先于未登记号。
        assert_eq!(
            l.judge_gate(CRIT_LO - 1, 9),
            Err(RtDiag::EGateRange.val())
        );
        // 闭集内档位 + 未登记号 → ENOENT。
        let fresh = first_unregistered(&l).unwrap_or(0);
        assert!(crit_in_window(fresh));
        assert_eq!(l.judge_gate(fresh, 1), Err(RtDiag::ENoEntry.val()));
        // 已登记号 + 倒退档位 → EREWIND。
        let mut l2 = build_frozen_list();
        assert!(l2.advance_to(1).is_ok());
        assert!(l2.advance_to(2).is_ok());
        let reg = l2.row_at(0).map(|r| r.crit).unwrap_or(0);
        assert_eq!(l2.judge_gate(reg, 1), Err(RtDiag::ERewind.val()));
        assert_eq!(l2.judge_gate(reg, 0), Err(RtDiag::ERewind.val()));
        // 全级通过：合法二值出口。
        assert_eq!(l2.judge_gate(reg, 3).is_ok(), true);
        assert!(judge_order_ok());
    }

    #[test]
    fn injections_all_caught() {
        // 三类注入各 10 轮全抓、零逃脱。
        let (c1, t1, e1) = inject_dangling_sweep();
        assert_eq!((t1, c1), (10, 10), "悬空号逃脱：{}", e1);
        assert!(e1.is_empty());
        let (c2, t2, e2) = inject_rewind_sweep();
        assert_eq!((t2, c2), (10, 10), "倒退闸门逃脱：{}", e2);
        assert!(e2.is_empty());
        let (c3, t3, e3) = inject_gaterange_sweep();
        assert_eq!((t3, c3), (10, 10), "越闭集闸门位逃脱：{}", e3);
        assert!(e3.is_empty());
        assert!(inject_dangling_ok());
        assert!(inject_rewind_ok());
        assert!(inject_gaterange_ok());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：指纹两次运行逐位相同，冻结表两次构建同指纹。
        assert!(dual_run_ok());
        let a = pipeline_digest();
        let b = pipeline_digest();
        assert_eq!(a, b);
        let l1 = build_frozen_list();
        let l2 = build_frozen_list();
        assert_eq!(l1.fingerprint(), l2.fingerprint());
        let mut i = 0usize;
        while i < RT_CAP {
            assert_eq!(l1.row_at(i), l2.row_at(i));
            i += 1;
        }
    }

    #[test]
    fn capacity_exhaustion_rejects() {
        // 容量耗尽：40 行满后新号 EFull、已登号 EDup、行数不变。
        assert!(capacity_ok());
        let mut l = RetestList::new();
        let mut k = 0u32;
        while k < RT_CAP as u32 {
            assert!(l.register(CRIT_LO + k, k % 4, 1).is_ok());
            k += 1;
        }
        assert_eq!(l.len(), RT_CAP);
        assert_eq!(l.state(), ListState::Full);
        assert_eq!(l.register(CRIT_LO + 40, 0, 1), Err(RtDiag::EFull.val()));
        assert_eq!(l.register(CRIT_LO + 5, 0, 1), Err(RtDiag::EDup.val()));
        assert_eq!(l.len(), RT_CAP);
    }

    #[test]
    fn boundary_edges_reject_or_accept() {
        // 边界四类：号面沿 / 预算沿 / 闸门位沿 / 推进沿。
        assert!(boundary_ok());
        // 号面沿逐点复核。
        let mut l = RetestList::new();
        assert!(l.register(CRIT_LO, 0, 1).is_ok());
        assert!(l.register(CRIT_HI, 3, BUDGET_CAP).is_ok());
        assert_eq!(l.register(12000, 0, 1), Err(RtDiag::EWindow.val()));
        assert_eq!(l.register(12800, 0, 1), Err(RtDiag::EWindow.val()));
        // 预算沿逐点复核。
        assert_eq!(l.register(12110, 0, 0), Err(RtDiag::EBudget.val()));
        assert_eq!(l.register(12111, 0, BUDGET_CAP + 1), Err(RtDiag::EBudget.val()));
        // 推进沿逐点复核：跳级拒 → 逐闸进 → 倒退拒 → 走到 G3。
        let mut a = build_frozen_list();
        assert_eq!(a.advance_to(2), Err(RtDiag::EJump.val()));
        assert!(a.advance_to(1).is_ok());
        assert_eq!(a.advance_to(0), Err(RtDiag::ERewind.val()));
        assert!(a.advance_to(1).is_ok());
        assert!(a.advance_to(2).is_ok());
        assert!(a.advance_to(3).is_ok());
    }

    #[test]
    fn freeze_semantics_write_protect() {
        // 冻结写保护：未满禁冻结、满后冻结成、冻结后写拒、重冻结拒。
        assert!(freeze_ok());
        let mut l = RetestList::new();
        assert_eq!(l.freeze(), Err(RtDiag::ENotFull.val()));
        let _ = l.register(12001, 0, 1);
        assert_eq!(l.freeze(), Err(RtDiag::ENotFull.val()));
        let mut f = build_full_list();
        assert!(f.freeze().is_ok());
        assert_eq!(f.register(12600, 0, 1), Err(RtDiag::ELocked.val()));
        assert_eq!(f.freeze(), Err(RtDiag::ELocked.val()));
        assert_eq!(f.len(), RT_CAP);
    }

    #[test]
    fn gate_closed_set_roundtrip() {
        // 闸门位闭集：四档往返一致、越闭集 None、GATE_ALL 首尾对。
        assert_eq!(GATE_ALL.len(), 4);
        let mut i = 0u32;
        while i < 4 {
            let g = Gate::from_index(i).unwrap_or(Gate::G0Static);
            assert_eq!(g.index(), i);
            assert_eq!(Gate::from_index(g.index()), Some(g));
            assert!(!g.name().is_empty());
            i += 1;
        }
        assert!(Gate::from_index(4).is_none());
        assert!(Gate::from_index(u32::MAX).is_none());
        assert_eq!(GATE_ALL.first().map(|g| *g), Some(Gate::G0Static));
        assert_eq!(GATE_ALL.last().map(|g| *g), Some(Gate::G3Final));
        assert!(gate_closed_ok());
        assert!(xref_green_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿：15 条、零红项、收口条目在列。
        let s = run_unx12797_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), CHECK_COUNT);
        assert_eq!(s.tally().1, 0);
    }
}
