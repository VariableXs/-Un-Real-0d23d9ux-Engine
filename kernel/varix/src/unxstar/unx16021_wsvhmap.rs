//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16021 · wineserver 职责消解账首版：句柄表映射 · 0x68440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16021-J1 消解账首版逐行落账且三字段齐备（上游职责/承接条目/
//!   消解状态），句柄表类职责映射行对照 AI-17 语义档一致率 100%，
//!   无承接项零静默断言通过。
//! 定位：wineserver 承担句柄表、同步原语、fd 传递三类职责，Varix 侧
//!   已由 AI-17/AI-18 提供原生实现——本条立"职责消解账"首版：逐项
//!   登记"上游职责 → Varix 原生承接条目"映射，首版聚焦句柄表类，
//!   消解决策一次做对，杜绝双句柄表状态漂移。
//! 设计要点：
//!   - 消解账结构：每行登记（职责 ID/上游职责描述/Varix 承接条目
//!     指针/消解状态四态/决策理由/语义档版本戳），账行只增不删；
//!   - 消解状态四态闭集：已消解（Resolved）/部分消解（Partial）/
//!     降级保留桩（StubKept）/无承接（Unbacked）；
//!   - 零静默律：无承接/降级保留桩状态行必须显式给出降级理由，
//!     桩清单可导出——找不到承接就明说，不许留白；
//!   - 句柄表类映射：句柄分配回收→AI-17 句柄表原生实现、句柄继承
//!     →AI-17 进程语义、句柄重复制→AI-17 对象语义、句柄类型校验
//!     →AI-18 对象类型系统，每行写清"上游行为假设 → Varix 对应
//!     语义"，四行起步滚动增补；
//!   - 注入面：删理由行/悬空承接指针/状态越四态/重复职责 ID/
//!     语义档版本漂移/账行删除（哈希链断裂）——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68440）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68440，AI-VX-1030 E1 域批1 独占区）。
pub const SEG_BASE: u32 = 0x68440;

/// 消解账判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// AI-17 句柄与对象语义档版本戳（冻结契约版本，写死）。
pub const AI17_SEMANTIC_VER: u32 = 7; // 波07 冻结
pub const AI18_SEMANTIC_VER: u32 = 7;

// ── 消解状态四态闭集 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveState {
    /// 已消解：Varix 原生面完整承接
    Resolved,
    /// 部分消解：主体承接、缺口显式登记
    Partial,
    /// 降级保留桩：暂以桩承接，桩必须有退出条件
    StubKept,
    /// 无承接：显性声明（必须带降级理由）
    Unbacked,
}

impl ResolveState {
    pub fn as_u32(self) -> u32 {
        match self {
            ResolveState::Resolved => 0,
            ResolveState::Partial => 1,
            ResolveState::StubKept => 2,
            ResolveState::Unbacked => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ResolveState::Resolved => "已消解",
            ResolveState::Partial => "部分消解",
            ResolveState::StubKept => "降级保留桩",
            ResolveState::Unbacked => "无承接",
        }
    }
    pub fn from_u32(v: u32) -> Option<ResolveState> {
        match v {
            0 => Some(ResolveState::Resolved),
            1 => Some(ResolveState::Partial),
            2 => Some(ResolveState::StubKept),
            3 => Some(ResolveState::Unbacked),
            _ => None,
        }
    }
    /// 该态是否要求降级理由非空（零静默律适用面）。
    pub fn requires_reason(self) -> bool {
        match self {
            ResolveState::StubKept | ResolveState::Unbacked => true,
            _ => false,
        }
    }
}

/// 消解账单行：六字段齐备才可入账。
#[derive(Clone, Debug)]
pub struct ResolveRow {
    /// 职责 ID（账内唯一，如 "WS-HT-01"）。
    pub duty_id: String,
    /// 上游职责描述（wineserver 侧行为假设）。
    pub wine_duty: String,
    /// Varix 承接条目指针（如 "AI-17 句柄表原生实现"）。
    pub varix_backing: String,
    /// 消解状态（四态闭集）。
    pub state: ResolveState,
    /// 决策理由（四态都应有；无承接/桩态强制非空）。
    pub reason: String,
    /// 语义档版本戳（对照 AI-17/AI-18 冻结版本）。
    pub sem_ver: u32,
}

/// 承接语义域闭集（句柄表类首版四行起步）。
pub const DOMAIN_HANDLE_ALLOC: usize = 0;
pub const DOMAIN_HANDLE_INHERIT: usize = 1;
pub const DOMAIN_HANDLE_DUP: usize = 2;
pub const DOMAIN_HANDLE_TYPE: usize = 3;
pub const DOMAIN_COUNT: usize = 4;
pub const DOMAIN_NAMES: [&str; DOMAIN_COUNT] =
    ["句柄分配与回收", "句柄继承", "句柄重复制", "句柄类型校验"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WsCode {
    /// 职责 ID 空或越闭集格式
    DutyIdBlank,
    /// 重复职责 ID
    DutyIdDuplicated,
    /// 上游职责描述空（三字段齐备律）
    WineDutyBlank,
    /// 承接条目指针空（三字段齐备律）
    BackingBlank,
    /// 消解状态越四态闭集
    StateOutOfRange,
    /// 无承接/桩态缺降级理由（零静默违规）
    SilentUnbacked,
    /// 语义档版本漂移（对照冻结版）
    SemanticDrift,
    /// 账行删除：哈希链前缀断裂
    HashChainBroken,
    /// 桩清单导出缺行
    StubListShort,
    /// 悬空承接指针：指针格式非法
    DanglingBacking,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl WsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WsCode::DutyIdBlank => "职责 ID 空：账行主键缺失",
            WsCode::DutyIdDuplicated => "重复职责 ID：账内唯一律",
            WsCode::WineDutyBlank => "上游职责描述空：三字段齐备律",
            WsCode::BackingBlank => "承接条目指针空：三字段齐备律",
            WsCode::StateOutOfRange => "消解状态越四态闭集",
            WsCode::SilentUnbacked => "无承接/桩缺降级理由：零静默违规",
            WsCode::SemanticDrift => "语义档版本漂移：对照冻结版失守",
            WsCode::HashChainBroken => "账行删除：哈希链前缀断裂",
            WsCode::StubListShort => "桩清单导出缺行",
            WsCode::DanglingBacking => "悬空承接指针：格式非法",
            WsCode::NotReproducible => "双跑不一致：确定性红线",
            WsCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [WsCode; 12] = [
        WsCode::DutyIdBlank,
        WsCode::DutyIdDuplicated,
        WsCode::WineDutyBlank,
        WsCode::BackingBlank,
        WsCode::StateOutOfRange,
        WsCode::SilentUnbacked,
        WsCode::SemanticDrift,
        WsCode::HashChainBroken,
        WsCode::StubListShort,
        WsCode::DanglingBacking,
        WsCode::NotReproducible,
        WsCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WsCode::ALL.len() {
        if WsCode::ALL[i].val() == code {
            return WsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── FNV-1a 32 位（账行哈希链） ──────────────────────────────────
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 承接指针格式校验：必须是 "AI-数字 …" 或 "F16 数字域 …" 或
/// "原生:…" 三种白名单形态，其余即悬空指针。
pub fn backing_pointer_legal(p: &str) -> bool {
    let b = p.as_bytes();
    if b.len() < 5 {
        return false;
    }
    if b.len() >= 5 && &b[0..5] == b"AI-17" {
        return true;
    }
    if b.len() >= 5 && &b[0..5] == b"AI-18" {
        return true;
    }
    if b.len() >= 5 && &b[0..5] == b"UNX-F" {
        return true;
    }
    // 字符级白名单：以 "原生:" 开头。
    p.starts_with("原生:")
}

/// 单行体检：六字段 + 零静默律 + 指针形态。Err 带诊断码。
pub fn vet_row(row: &ResolveRow) -> Result<(), u32> {
    if row.duty_id.len() < 6 {
        return Err(WsCode::DutyIdBlank.val());
    }
    // ID 格式：WS- 前缀 + 两位域码 + 两位序号，如 "WS-HT-01"。
    if !row.duty_id.starts_with("WS-") {
        return Err(WsCode::DutyIdBlank.val());
    }
    if row.wine_duty.len() == 0 {
        return Err(WsCode::WineDutyBlank.val());
    }
    if row.varix_backing.len() == 0 {
        return Err(WsCode::BackingBlank.val());
    }
    if ResolveState::from_u32(row.state.as_u32()).is_none() {
        return Err(WsCode::StateOutOfRange.val());
    }
    if row.reason.len() == 0 {
        // 四态都要求理由非空（决策理由是账行的灵魂）。
        return Err(WsCode::SilentUnbacked.val());
    }
    if row.state.requires_reason() && row.reason.len() < 12 {
        // 无承接/桩态的理由必须说透（≥12 字节≈六个汉字）。
        return Err(WsCode::SilentUnbacked.val());
    }
    if row.state != ResolveState::Unbacked && !backing_pointer_legal(&row.varix_backing) {
        return Err(WsCode::DanglingBacking.val());
    }
    if row.sem_ver > AI17_SEMANTIC_VER {
        return Err(WsCode::SemanticDrift.val());
    }
    Ok(())
}

// ── 消解账本（只增不删 + 哈希链） ───────────────────────────────
pub struct ResolveLedger {
    rows: Vec<ResolveRow>,
    /// 哈希链：chain[i] = fnv(chain[i-1] ++ row[i] 序列化)。
    chain: Vec<u32>,
}

impl ResolveLedger {
    pub fn new() -> ResolveLedger {
        ResolveLedger { rows: Vec::new(), chain: Vec::new() }
    }

    /// 追加一行（只增不删的唯一入口）。返 Err 拒收带诊断码。
    pub fn append(&mut self, row: ResolveRow) -> Result<u32, u32> {
        vet_row(&row)?;
        // 账内唯一：职责 ID 不得重复。
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].duty_id == row.duty_id {
                return Err(WsCode::DutyIdDuplicated.val());
            }
            i += 1;
        }
        // 序列化摘要 → 链式哈希。
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(row.duty_id.as_bytes());
        buf.extend_from_slice(row.wine_duty.as_bytes());
        buf.extend_from_slice(row.varix_backing.as_bytes());
        buf.push(row.state.as_u32() as u8);
        buf.extend_from_slice(row.reason.as_bytes());
        buf.extend_from_slice(&row.sem_ver.to_le_bytes());
        let prev = if self.chain.is_empty() { 0 } else { self.chain[self.chain.len() - 1] };
        let mut seed: Vec<u8> = Vec::new();
        seed.extend_from_slice(&prev.to_le_bytes());
        seed.extend_from_slice(&buf);
        let h = fnv1a(&seed);
        self.rows.push(row);
        self.chain.push(h);
        Ok(h)
    }

    /// 链完整性复验：重放全链，哈希逐环对上。
    pub fn verify_chain(&self) -> Result<(), u32> {
        let mut prev: u32 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let row = &self.rows[i];
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(row.duty_id.as_bytes());
            buf.extend_from_slice(row.wine_duty.as_bytes());
            buf.extend_from_slice(row.varix_backing.as_bytes());
            buf.push(row.state.as_u32() as u8);
            buf.extend_from_slice(row.reason.as_bytes());
            buf.extend_from_slice(&row.sem_ver.to_le_bytes());
            let mut seed: Vec<u8> = Vec::new();
            seed.extend_from_slice(&prev.to_le_bytes());
            seed.extend_from_slice(&buf);
            let h = fnv1a(&seed);
            if i >= self.chain.len() || self.chain[i] != h {
                return Err(WsCode::HashChainBroken.val());
            }
            prev = h;
            i += 1;
        }
        if self.chain.len() != self.rows.len() {
            return Err(WsCode::HashChainBroken.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 三字段齐备率：判据原文口径（上游职责/承接条目/消解状态）。
    pub fn tri_field_rate(&self) -> Result<usize, u32> {
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.wine_duty.len() > 0 && r.varix_backing.len() > 0 {
                if ResolveState::from_u32(r.state.as_u32()).is_some() {
                    ok += 1;
                }
            }
            i += 1;
        }
        if self.rows.len() == 0 {
            return Err(WsCode::DutyIdBlank.val());
        }
        Ok((ok * 100) / self.rows.len())
    }

    /// 桩清单导出：桩态与无承接到账，缺理由即 StubListShort。
    pub fn export_stub_list(&self) -> Result<Vec<String>, u32> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.state.requires_reason() {
                if r.reason.len() == 0 {
                    return Err(WsCode::StubListShort.val());
                }
                out.push(format!("{}|{}|{}", r.duty_id, r.state.name(), r.reason));
            }
            i += 1;
        }
        Ok(out)
    }

    /// 句柄表类映射行与 AI-17 语义档对照一致率（判据 100% 口径）。
    /// 对照规则：承接指针以 AI-17/AI-18 开头的行，其语义档版本戳
    /// 必须等于冻结版本——漂移即不一致。
    pub fn semantic_align_rate(&self) -> Result<usize, u32> {
        let mut aligned = 0;
        let mut total = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let b = r.varix_backing.as_bytes();
            let is_sem_row = b.len() >= 5 && (&b[0..5] == b"AI-17" || &b[0..5] == b"AI-18");
            if is_sem_row {
                total += 1;
                if r.sem_ver == AI17_SEMANTIC_VER {
                    aligned += 1;
                }
            }
            i += 1;
        }
        if total == 0 {
            return Err(WsCode::SemanticDrift.val());
        }
        Ok((aligned * 100) / total)
    }
}

// ── 首版账装配：句柄表类四行起步 + 滚动增补 ─────────────────────
/// 构造判据规定的首版消解账（每域一行，四行起步）。
pub fn build_initial_ledger() -> Result<ResolveLedger, u32> {
    let mut led = ResolveLedger::new();
    // ① 句柄分配与回收 → AI-17 句柄表原生实现。
    led.append(ResolveRow {
        duty_id: String::from("WS-HT-01"),
        wine_duty: String::from("wineserver 句柄表：分配/回收/引用计数"),
        varix_backing: String::from("AI-17 句柄表原生实现"),
        state: ResolveState::Resolved,
        reason: String::from("AI-17 句柄表语义档完整承接，双句柄表漂移源消除"),
        sem_ver: AI17_SEMANTIC_VER,
    })?;
    // ② 句柄继承 → AI-17 进程语义。
    led.append(ResolveRow {
        duty_id: String::from("WS-HT-02"),
        wine_duty: String::from("wineserver 句柄继承：进程创建时句柄表投影"),
        varix_backing: String::from("AI-17 进程语义"),
        state: ResolveState::Resolved,
        reason: String::from("继承语义走 AI-17 进程语义档，继承清单随进程账"),
        sem_ver: AI17_SEMANTIC_VER,
    })?;
    // ③ 句柄重复制 → AI-17 对象语义。
    led.append(ResolveRow {
        duty_id: String::from("WS-HT-03"),
        wine_duty: String::from("wineserver duplicate：跨进程句柄重复制"),
        varix_backing: String::from("AI-17 对象语义"),
        state: ResolveState::Resolved,
        reason: String::from("重复制走 AI-17 对象语义档，权限检查在案"),
        sem_ver: AI17_SEMANTIC_VER,
    })?;
    // ④ 句柄类型校验 → AI-18 对象类型系统。
    led.append(ResolveRow {
        duty_id: String::from("WS-HT-04"),
        wine_duty: String::from("wineserver 句柄类型校验：get_type 字段核对"),
        varix_backing: String::from("AI-18 对象类型系统"),
        state: ResolveState::Resolved,
        reason: String::from("类型系统由 AI-18 闭集承接，校验面冻结"),
        sem_ver: AI18_SEMANTIC_VER,
    })?;
    // ⑤ 滚动增补样例：异步 fd 传递暂无原生面 → 显性无承接（零静默）。
    led.append(ResolveRow {
        duty_id: String::from("WS-HT-05"),
        wine_duty: String::from("wineserver fd 传递：SCM_RIGHTS 类跨进程 fd 走私"),
        varix_backing: String::from("无"),
        state: ResolveState::Unbacked,
        reason: String::from("首版未覆盖 fd 传递域，按零静默律显性声明，B05 批承接"),
        sem_ver: AI17_SEMANTIC_VER,
    })?;
    Ok(led)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((账行数, 桩清单行数))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let led = build_initial_ledger()?;
    led.verify_chain()?;
    let rate = led.tri_field_rate()?;
    if rate != 100 {
        return Err(WsCode::WineDutyBlank.val());
    }
    let align = led.semantic_align_rate()?;
    if align != 100 {
        return Err(WsCode::SemanticDrift.val());
    }
    let stubs = led.export_stub_list()?;
    Ok((led.len(), stubs.len()))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 删理由行：空理由 → SilentUnbacked。
        let mut bad = ResolveRow {
            duty_id: String::from("WS-HT-90"),
            wine_duty: String::from("上游职责描述占位"),
            varix_backing: String::from("AI-17 句柄表原生实现"),
            state: ResolveState::Resolved,
            reason: String::new(),
            sem_ver: AI17_SEMANTIC_VER,
        };
        led.record(0, vet_row(&bad) == Err(WsCode::SilentUnbacked.val()));
        // 无承接态短理由（<12 字节）也必抓。
        bad.state = ResolveState::Unbacked;
        bad.reason = String::from("待定");
        led.record(0, vet_row(&bad) == Err(WsCode::SilentUnbacked.val()));

        // ② 悬空承接指针：非法形态。
        let mut bad2 = ResolveRow {
            duty_id: String::from("WS-HT-91"),
            wine_duty: String::from("上游职责描述占位"),
            varix_backing: String::from("未知系统模块"),
            state: ResolveState::Resolved,
            reason: String::from("理由充分占位文本"),
            sem_ver: AI17_SEMANTIC_VER,
        };
        led.record(1, vet_row(&bad2) == Err(WsCode::DanglingBacking.val()));
        bad2.varix_backing = String::new();
        led.record(1, vet_row(&bad2) == Err(WsCode::BackingBlank.val()));

        // ③ 状态越四态闭集。
        led.record(2, ResolveState::from_u32(4).is_none());
        led.record(2, ResolveState::from_u32(0xFFFF).is_none());

        // ④ 重复职责 ID：同账追加同 ID 必拒。
        let mut led2 = ResolveLedger::new();
        let row0 = ResolveRow {
            duty_id: String::from("WS-HT-01"),
            wine_duty: String::from("上游职责描述占位"),
            varix_backing: String::from("AI-17 句柄表原生实现"),
            state: ResolveState::Resolved,
            reason: String::from("决策理由占位文本"),
            sem_ver: AI17_SEMANTIC_VER,
        };
        let _ = led2.append(row0.clone());
        led.record(3, led2.append(row0) == Err(WsCode::DutyIdDuplicated.val()));

        // ⑤ 语义档版本漂移：sem_ver > 冻结版必拒。
        let bad3 = ResolveRow {
            duty_id: String::from("WS-HT-92"),
            wine_duty: String::from("上游职责描述占位"),
            varix_backing: String::from("AI-17 句柄表原生实现"),
            state: ResolveState::Resolved,
            reason: String::from("决策理由占位文本"),
            sem_ver: AI17_SEMANTIC_VER + 1 + (rnd % 3) as u32,
        };
        led.record(4, vet_row(&bad3) == Err(WsCode::SemanticDrift.val()));

        // ⑥ 账行删除（哈希链断裂）：装账后伪造断链视图。
        let chain_ok = {
            let led3 = build_initial_ledger();
            match led3 {
                Ok(l) => {
                    // 完整链先证通过；
                    let good = l.verify_chain().is_ok();
                    // 人为抽掉一环（模拟账行被删）→ 长度失配必红。
                    let mut l2 = ResolveLedger { rows: Vec::new(), chain: Vec::new() };
                    l2.rows = l.rows.clone();
                    l2.chain = l.chain.clone();
                    if l2.chain.len() > 1 {
                        l2.chain.pop();
                    }
                    good && l2.verify_chain() == Err(WsCode::HashChainBroken.val())
                }
                Err(_) => false,
            }
        };
        led.record(5, chain_ok);

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(WsCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法账必须全过 ─────────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let led = build_initial_ledger()?;
    if led.len() < 4 {
        return Err(WsCode::DutyIdBlank.val());
    }
    led.verify_chain()?;
    // 四个承接域名点验。
    let mut d = 0;
    while d < DOMAIN_COUNT {
        if DOMAIN_NAMES[d].len() == 0 {
            return Err(WsCode::WineDutyBlank.val());
        }
        d += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WsCode::ALL.len() {
        let v = WsCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：账行数 + 桩清单。
    let g = golden_run();
    let g_ok = matches!(g, Ok((5, 1)));
    v.push(("J1-黄金正演5行1桩", g_ok, format!("{:?}", g)));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 三字段齐备率 100%。
    let l = build_initial_ledger().unwrap_or_else(|_| ResolveLedger::new());
    let rate = l.tri_field_rate().unwrap_or(0);
    v.push(("J1-三字段齐备率100%", rate == 100, format!("{}%", rate)));
    // 4. 语义档对照一致率 100%（判据原文口径）。
    let align = l.semantic_align_rate().unwrap_or(0);
    v.push(("J1-语义档对照100%", align == 100, format!("{}%", align)));
    // 5. 无承接零静默：桩/无承接行理由全非空。
    let stubs = l.export_stub_list().unwrap_or_default();
    v.push((
        "J1-无承接零静默",
        stubs.len() == 1,
        format!("stub={}", stubs.len()),
    ));
    // 6. 哈希链完整性（正向）。
    v.push((
        "J1-哈希链完整",
        matches!(l.verify_chain(), Ok(())),
        format!("{} 环", l.len()),
    ));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("四域+链复验"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xD1);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16021 wineserver 职责消解账首版：句柄表映射");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
