//! UNX-F1569 联签保留项处置（保留意见逐项闭环） · 0x78D40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 6 · UNX-F1569 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1569-J1 保留项闭环率 100%（保留意见逐项登记
//!     处置：项/方案/状态三列），悬空保留 10/10 告警，处置账落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 保留项闭集：10 条保留意见逐项登记（项号 R01..R10 / 项述 /
//!     处置方案 / 状态），项号唯一、项述非空；
//!   - 三列处置：项 / 方案 / 状态逐列入账，方案非空、状态闭集
//!     （待办 / 处置中 / 已闭环）；
//!   - 悬空告警：方案为空或状态停在待办的保留项即悬空，悬空项
//!     逐项告警，悬空注入 10/10 必警；
//!   - 闭环率 100%：10 项状态全部推进到已闭环方可达标，留一项
//!     待办即闭环率 <100% 显性拒；
//!   - 处置账落档：13 行（头 + 10 项行 + 尾）处置账 + FNV-1a 指纹
//!     落档双跑逐位一致；
//!   - 状态机单调：待办→处置中→已闭环逐项单调推进，倒退显性拒；
//!   - LCG 抽验复验，种子写死 = SEG_BASE；零浮点 / IO / 墙钟 /
//!     unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78D40，派工卡 VX8-1104 B06 卡 6 独占窗口）。
pub const SEG_BASE: u32 = 0x78D40;

/// 保留项条数（判据 J1 冻结闭集 10 条）。
pub const RESERVE_COUNT: usize = 10;

/// 处置账落档行数（头 + 10 项行 + 尾，冻结）。
pub const DISPOSAL_ROWS: usize = 12;

/// 悬空注入路数（判据 J1「10/10」冻结）。
pub const DANGLING_ROUTES: usize = 10;

// ── 保留项闭集（项号 / 项述） ───────────────────────────────────
pub const RESERVES: [(&str, &str); 10] = [
    ("R01", "中断屏蔽恢复窗口与冻结口径存在半拍差"),
    ("R02", "ktime 副钟锚定重校的双跑样本量待扩"),
    ("R03", "MSI-X 向量释放后的掩码残留位未全清"),
    ("R04", "smp_call 取消路径的回执计数口径待统一"),
    ("R05", "shootdown 广播确认窗的边界拍数待冻结"),
    ("R06", "页表冻结写口的越窗告警明细列待补"),
    ("R07", "内存账本快照指纹的回退路径待演练"),
    ("R08", "移交包缺件注入的告警文案待双语化"),
    ("R09", "联签总账时刻列的时区口径待标注"),
    ("R10", "反馈通道三跳追溯的回执超窗阈值待复核"),
];

// ── 处置方案闭集（项号 / 方案述） ───────────────────────────────
pub const PLANS: [(&str, &str); 10] = [
    ("R01", "按冻结口径重排屏蔽恢复序并复测半拍差"),
    ("R02", "扩样本至百组并按 LCG 种子复跑比对"),
    ("R03", "补掩码残留清扫位并在释放路径复扫"),
    ("R04", "统一取消路径回执计数并入健康一屏"),
    ("R05", "冻结广播确认窗拍数并注入越窗必抓"),
    ("R06", "补越窗告警明细列并回查历史告警"),
    ("R07", "演练快照回退路径并落演练报告"),
    ("R08", "告警文案补双语列并终验对平"),
    ("R09", "时刻列标注时区口径并入总账头注"),
    ("R10", "复核三跳追溯超窗阈值并冻结取值"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reserve69Code {
    /// 项号越闭集：项号收进 R01..R10
    IdUnknown,
    /// 项号重复：一项号一登记，重登先撤旧
    IdDuplicate,
    /// 项号为空：补项号再登记
    IdEmpty,
    /// 项述为空：补项述再登记
    DescEmpty,
    /// 方案为空：保留项悬空，补处置方案再闭环
    PlanEmpty,
    /// 状态越闭集：状态收进待办/处置中/已闭环
    StatusUnknown,
    /// 保留项状态倒退：待办→处置中→已闭环单调推进
    StatusRegress,
    /// 悬空保留未告警：悬空项逐项告警不静默
    DanglingSilent,
    /// 闭环率不足百：全 10 项已闭环方可达标
    CloseBelowHundred,
    /// 处置账状态倒退：登记→闭环→落档单调推进
    PhaseRegress,
    /// 处置账状态跳级：未全闭环不得落档
    PhaseSkip,
    /// 处置账行数违约：落档必须恰 12 行
    LedgerRowsBad,
}

impl Reserve69Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Reserve69Code::IdUnknown => "项号越闭集：项号收进 R01..R10 再登记",
            Reserve69Code::IdDuplicate => "项号重复：一项号一登记，重登先撤旧再补",
            Reserve69Code::IdEmpty => "项号为空：补项号再登记",
            Reserve69Code::DescEmpty => "项述为空：补项述再登记",
            Reserve69Code::PlanEmpty => "方案为空：保留项悬空，补处置方案再闭环",
            Reserve69Code::StatusUnknown => "状态越闭集：状态收进待办/处置中/已闭环",
            Reserve69Code::StatusRegress => "保留项状态倒退：待办→处置中→已闭环单调推进",
            Reserve69Code::DanglingSilent => "悬空保留未告警：悬空项逐项告警不静默",
            Reserve69Code::CloseBelowHundred => "闭环率不足百：全 10 项已闭环方可达标",
            Reserve69Code::PhaseRegress => "处置账状态倒退：登记→闭环→落档单调推进",
            Reserve69Code::PhaseSkip => "处置账状态跳级：未全闭环不得落档",
            Reserve69Code::LedgerRowsBad => "处置账行数违约：落档必须恰 12 行",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Reserve69Code; 12] = [
        Reserve69Code::IdUnknown,
        Reserve69Code::IdDuplicate,
        Reserve69Code::IdEmpty,
        Reserve69Code::DescEmpty,
        Reserve69Code::PlanEmpty,
        Reserve69Code::StatusUnknown,
        Reserve69Code::StatusRegress,
        Reserve69Code::DanglingSilent,
        Reserve69Code::CloseBelowHundred,
        Reserve69Code::PhaseRegress,
        Reserve69Code::PhaseSkip,
        Reserve69Code::LedgerRowsBad,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Reserve69Code::ALL.len() {
        let c = match Reserve69Code::ALL.get(i) {
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

// ── 处置状态机（登记 → 闭环 → 落档，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisposalPhase {
    /// 登记期：10 条保留项逐项三列入账
    Enrolling,
    /// 闭环期：全 10 项推进到已闭环
    Closed,
    /// 落档期：处置账落档封账
    Sealed,
}

impl DisposalPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            DisposalPhase::Enrolling => 0,
            DisposalPhase::Closed => 1,
            DisposalPhase::Sealed => 2,
        }
    }
}

/// 保留项状态闭集位次：0 待办 / 1 处置中 / 2 已闭环。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemStatus {
    /// 待办：方案未定或未启动
    Todo,
    /// 处置中：方案已定正在处置
    Doing,
    /// 已闭环：方案执行完毕复验通过
    Done,
}

impl ItemStatus {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            ItemStatus::Todo => 0,
            ItemStatus::Doing => 1,
            ItemStatus::Done => 2,
        }
    }
    /// 中文列名。
    pub fn label(self) -> &'static str {
        match self {
            ItemStatus::Todo => "待办",
            ItemStatus::Doing => "处置中",
            ItemStatus::Done => "已闭环",
        }
    }
}

/// 单条保留项处置行（项/方案/状态三列）。
#[derive(Clone, Debug)]
pub struct ReserveRow {
    pub id: String,
    pub desc: String,
    pub plan: String,
    pub status: ItemStatus,
    /// 悬空告警位（悬空即置位，闭环后清除）。
    pub alarmed: bool,
}

/// 保留项处置账。
pub struct DisposalBook {
    phase: DisposalPhase,
    rows: Vec<ReserveRow>,
    ledger_digest: u32,
    ledger_text: Option<String>,
}

/// 项号格式校验：R + 两位序号，序号 1..=10。
fn valid_reserve_id(id: &str) -> bool {
    if !id.starts_with('R') {
        return false;
    }
    let suffix = match id.strip_prefix('R') {
        Some(s) => s,
        None => return false,
    };
    let bytes = suffix.as_bytes();
    if bytes.len() != 2 {
        return false;
    }
    let mut value = 0u32;
    let mut k = 0usize;
    while k < bytes.len() {
        let b = match bytes.get(k) {
            Some(b) => *b,
            None => return false,
        };
        if !b.is_ascii_digit() {
            return false;
        }
        value = value.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        k += 1;
    }
    value >= 1 && value <= RESERVE_COUNT as u32
}

impl DisposalBook {
    pub fn new() -> DisposalBook {
        DisposalBook {
            phase: DisposalPhase::Enrolling,
            rows: Vec::new(),
            ledger_digest: 0,
            ledger_text: None,
        }
    }

    pub fn phase(&self) -> DisposalPhase {
        self.phase
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn ledger_digest(&self) -> u32 {
        self.ledger_digest
    }

    pub fn text(&self) -> Option<&str> {
        match self.ledger_text.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 悬空判定：方案为空或状态停在待办即悬空。
    pub fn is_dangling(row: &ReserveRow) -> bool {
        row.plan.is_empty() || row.status == ItemStatus::Todo
    }

    /// 悬空清点：逐项扫描并置告警位，返回悬空项数。
    pub fn scan_dangling(&mut self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            let dangling = match self.rows.get(i) {
                Some(r) => DisposalBook::is_dangling(r),
                None => false,
            };
            if dangling {
                n += 1;
                if let Some(r) = self.rows.get_mut(i) {
                    r.alarmed = true;
                }
            }
            i += 1;
        }
        n
    }

    /// 悬空告警数。
    pub fn alarm_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.alarmed {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 保留项登记：项号闭集格式 + 唯一 + 项述非空 + 方案可空。
    pub fn enroll(&mut self, id: &str, desc: &str, plan: &str) -> Result<(), u32> {
        if self.phase.rank() >= DisposalPhase::Closed.rank() {
            return Err(Reserve69Code::PhaseRegress.val());
        }
        if id.is_empty() {
            return Err(Reserve69Code::IdEmpty.val());
        }
        if !valid_reserve_id(id) {
            return Err(Reserve69Code::IdUnknown.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return Err(Reserve69Code::IdUnknown.val()),
            };
            if r.id == id {
                return Err(Reserve69Code::IdDuplicate.val());
            }
            i += 1;
        }
        if desc.is_empty() {
            return Err(Reserve69Code::DescEmpty.val());
        }
        self.rows.push(ReserveRow {
            id: id.to_string(),
            desc: desc.to_string(),
            plan: plan.to_string(),
            status: ItemStatus::Todo,
            alarmed: false,
        });
        Ok(())
    }

    /// 定方案：补处置方案并推进到处置中（悬空随之消除）。
    /// 已闭环项再定方案即状态倒退，显性拒（只对目标项判位次）。
    pub fn set_plan(&mut self, id: &str, plan: &str) -> Result<(), u32> {
        if plan.is_empty() {
            return Err(Reserve69Code::PlanEmpty.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    if r.status.rank() > ItemStatus::Doing.rank() {
                        return Err(Reserve69Code::StatusRegress.val());
                    }
                    if let Some(r) = self.rows.get_mut(i) {
                        r.plan = plan.to_string();
                        r.status = ItemStatus::Doing;
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(Reserve69Code::IdUnknown.val())
    }

    /// 闭环：处置中项复验通过推进到已闭环（悬空项闭环必拒）。
    pub fn close(&mut self, id: &str) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    if DisposalBook::is_dangling(r) {
                        return Err(Reserve69Code::PlanEmpty.val());
                    }
                    if let Some(r) = self.rows.get_mut(i) {
                        r.status = ItemStatus::Done;
                        r.alarmed = false;
                    }
                    return Ok(self.closed_count());
                }
            }
            i += 1;
        }
        Err(Reserve69Code::IdUnknown.val())
    }

    /// 已闭环项数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.status == ItemStatus::Done {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 闭环收口：登记齐 10 项且全部已闭环方可达标。
    pub fn close_all(&mut self) -> Result<usize, u32> {
        if self.phase != DisposalPhase::Enrolling {
            return Err(Reserve69Code::PhaseRegress.val());
        }
        if self.rows.len() != RESERVE_COUNT {
            return Err(Reserve69Code::CloseBelowHundred.val());
        }
        if self.closed_count() != RESERVE_COUNT {
            return Err(Reserve69Code::CloseBelowHundred.val());
        }
        self.phase = DisposalPhase::Closed;
        Ok(self.closed_count())
    }

    /// 处置账落档：12 行处置账 + FNV-1a 指纹 + 封账。
    pub fn seal_ledger(&mut self) -> Result<u32, u32> {
        if self.phase != DisposalPhase::Closed {
            return Err(Reserve69Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1569 保留项处置账 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let plan_show: &str = if r.plan.is_empty() { "（方案空）" } else { &r.plan };
            let line = format!("{} {} {} ●", r.id, plan_show, r.status.label());
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!(
            "== 保留项闭环率 100% · 悬空告警 {}/{} ==",
            self.alarm_count(),
            self.alarm_count()
        ));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != DISPOSAL_ROWS {
            return Err(Reserve69Code::LedgerRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.ledger_text = Some(text);
        self.ledger_digest = digest;
        self.phase = DisposalPhase::Sealed;
        Ok(digest)
    }

    /// LCG 抽验：抽 5 项复验已闭环且方案非空。
    pub fn sample_audit(&self) -> Result<usize, u32> {
        if self.rows.len() != RESERVE_COUNT {
            return Err(Reserve69Code::CloseBelowHundred.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < 5 {
            let idx = lcg.next_mod(RESERVE_COUNT);
            let r = match self.rows.get(idx) {
                Some(r) => r,
                None => return Err(Reserve69Code::IdUnknown.val()),
            };
            if r.status != ItemStatus::Done || r.plan.is_empty() {
                return Err(Reserve69Code::CloseBelowHundred.val());
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }
}

// ── 好账构建：10 项登记 + 定方案 + 逐项闭环 + 收口 + 落档 ───────
pub fn build_book_ok() -> DisposalBook {
    let mut bk = DisposalBook::new();
    let mut i = 0usize;
    while i < RESERVES.len() {
        let row = match RESERVES.get(i) {
            Some(r) => *r,
            None => break,
        };
        let plan = match PLANS.get(i) {
            Some(p) => p.1,
            None => "",
        };
        let _ = bk.enroll(row.0, row.1, plan);
        i += 1;
    }
    // 逐项定方案（待办→处置中）再逐项闭环（处置中→已闭环）。
    let mut j = 0usize;
    while j < RESERVES.len() {
        let row = match RESERVES.get(j) {
            Some(r) => r.0,
            None => break,
        };
        let plan = match PLANS.get(j) {
            Some(p) => p.1,
            None => "",
        };
        let _ = bk.set_plan(row, plan);
        let _ = bk.close(row);
        j += 1;
    }
    let _ = bk.close_all();
    let _ = bk.seal_ledger();
    bk
}

/// 悬空注入表：十路注入（每项一路——方案置空后扫描必警）。
pub const DANGLING_ROUTES_TABLE: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

/// 悬空注入 10/10 告警复算：逐路构造悬空项必警，返回警路数。
fn dangling_detect_count() -> usize {
    let mut alarmed_routes = 0usize;
    let mut r = 0usize;
    while r < DANGLING_ROUTES_TABLE.len() {
        let route = match DANGLING_ROUTES_TABLE.get(r) {
            Some(v) => *v,
            None => break,
        };
        let mut bk = DisposalBook::new();
        // 全登好项，仅把第 route 项方案置空构造悬空。
        let mut i = 0usize;
        while i < RESERVES.len() {
            let row = match RESERVES.get(i) {
                Some(rr) => *rr,
                None => break,
            };
            let plan: &str = if i == route { "" } else { "冻结方案占位" };
            let _ = bk.enroll(row.0, row.1, plan);
            i += 1;
        }
        let n = bk.scan_dangling();
        if n >= 1 && bk.alarm_count() >= 1 {
            alarmed_routes += 1;
        }
        r += 1;
    }
    alarmed_routes
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 三列齐备：10 项项/方案/状态逐列入账且全闭环。
fn three_columns_ok() -> bool {
    let bk = build_book_ok();
    if bk.row_count() != RESERVE_COUNT {
        return false;
    }
    if bk.closed_count() != RESERVE_COUNT {
        return false;
    }
    if bk.phase() != DisposalPhase::Sealed {
        return false;
    }
    true
}

/// 闭环率 100%：好账收口 10/10 已闭环。
fn close_rate_ok() -> bool {
    let bk = build_book_ok();
    if bk.closed_count() != RESERVE_COUNT {
        return false;
    }
    let n = match bk.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != 5 {
        return false;
    }
    true
}

/// 处置账落档：12 行、10 项号齐、100% 尾标、指纹非零。
fn ledger_archived_ok() -> bool {
    let bk = build_book_ok();
    let text = match bk.text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != DISPOSAL_ROWS {
        return false;
    }
    if !text.contains("处置账") {
        return false;
    }
    if !text.contains("闭环率 100%") {
        return false;
    }
    let mut i = 0usize;
    while i < RESERVES.len() {
        let row = match RESERVES.get(i) {
            Some(r) => r.0,
            None => break,
        };
        if !text.contains(row) {
            return false;
        }
        i += 1;
    }
    if bk.ledger_digest() == 0 {
        return false;
    }
    true
}

/// 悬空注入 10/10 告警：十路逐路必警且警路数恰 10。
fn dangling_10_ok() -> bool {
    let n = dangling_detect_count();
    if n != DANGLING_ROUTES {
        return false;
    }
    // 抽一路复验：悬空项 close 必拒（PlanEmpty）。
    let mut bk = DisposalBook::new();
    let row0 = match RESERVES.get(0) {
        Some(r) => *r,
        None => return false,
    };
    if bk.enroll(row0.0, row0.1, "").is_err() {
        return false;
    }
    let _ = bk.scan_dangling();
    match bk.close(row0.0) {
        Err(code) => {
            if code != Reserve69Code::PlanEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 项号注入：越闭集 / 三位序号 / 零号 / 越号四路必抓。
fn inject_badid_ok() -> bool {
    let mut bk = DisposalBook::new();
    // 越闭集（非 R 前缀）。
    match bk.enroll("X99", "凭空项", "方案") {
        Err(code) => {
            if code != Reserve69Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 三位序号。
    match bk.enroll("R001", "三位序号", "方案") {
        Err(code) => {
            if code != Reserve69Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 零号。
    match bk.enroll("R00", "零号项", "方案") {
        Err(code) => {
            if code != Reserve69Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 越号。
    match bk.enroll("R99", "越号项", "方案") {
        Err(code) => {
            if code != Reserve69Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 空项号 / 空项述。
    match bk.enroll("", "空号项", "方案") {
        Err(code) => {
            if code != Reserve69Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match bk.enroll("R01", "", "方案") {
        Err(code) => {
            if code != Reserve69Code::DescEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if bk.row_count() != 0 {
        return false;
    }
    true
}

/// 重号注入：同项号二登必抓（IdDuplicate）。
fn inject_dup_ok() -> bool {
    let mut bk = DisposalBook::new();
    let row0 = match RESERVES.get(0) {
        Some(r) => *r,
        None => return false,
    };
    if bk.enroll(row0.0, row0.1, "方案一").is_err() {
        return false;
    }
    match bk.enroll(row0.0, "另一项述", "方案二") {
        Err(code) => {
            if code != Reserve69Code::IdDuplicate.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if bk.row_count() != 1 {
        return false;
    }
    true
}

/// 状态倒退注入：已闭环项再定方案必拒（StatusRegress）。
fn inject_regress_ok() -> bool {
    let mut bk = DisposalBook::new();
    let row0 = match RESERVES.get(0) {
        Some(r) => *r,
        None => return false,
    };
    if bk.enroll(row0.0, row0.1, "方案甲").is_err() {
        return false;
    }
    if bk.set_plan(row0.0, "方案甲").is_err() {
        return false;
    }
    if bk.close(row0.0).is_err() {
        return false;
    }
    match bk.set_plan(row0.0, "方案乙") {
        Err(code) => {
            if code != Reserve69Code::StatusRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 状态不得倒退：closed_count 保持 1。
    if bk.closed_count() != 1 {
        return false;
    }
    true
}

/// 率不足注入：留一项待办收口必拒（CloseBelowHundred）。
fn inject_below_ok() -> bool {
    let mut bk = DisposalBook::new();
    let mut i = 0usize;
    while i < RESERVES.len() {
        let row = match RESERVES.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = bk.enroll(row.0, row.1, "冻结方案");
        // 末项留在待办（悬空），前 9 项定方案后闭环。
        if i + 1 != RESERVES.len() {
            let _ = bk.set_plan(row.0, "冻结方案");
            let _ = bk.close(row.0);
        }
        i += 1;
    }
    let _ = bk.scan_dangling();
    if bk.alarm_count() != 1 {
        return false;
    }
    match bk.close_all() {
        Err(code) => {
            if code != Reserve69Code::CloseBelowHundred.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 状态机单调：未闭环落档拒、封账后再登记拒。
fn phase_monotonic_ok() -> bool {
    let mut bk = DisposalBook::new();
    // 未闭环先落档：跳级拒。
    match bk.seal_ledger() {
        Err(code) => {
            if code != Reserve69Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好账封账后再登记：倒退拒。
    let mut sealed = build_book_ok();
    let row0 = match RESERVES.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match sealed.enroll(row0.0, row0.1, "方案") {
        Err(code) => {
            if code != Reserve69Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 抽验：好账抽 5 项全过，同种子序列逐位确定。
fn sample_audit_ok() -> bool {
    let bk = build_book_ok();
    let n = match bk.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != 5 {
        return false;
    }
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < 5 {
        if a.next_mod(RESERVE_COUNT) != b.next_mod(RESERVE_COUNT) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好账指纹与处置账两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_book_ok().ledger_digest();
    let d2 = build_book_ok().ledger_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_book_ok().text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_book_ok().text() {
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
    for c in Reserve69Code::ALL.iter() {
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
    let first = match Reserve69Code::ALL.get(0) {
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
pub fn run_unx1569_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1569 联签保留项处置");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("三列齐备", three_columns_ok(), "10 项项/方案/状态逐列入账全闭环");
    s.add_dyn("闭环率100%", close_rate_ok(), "好账 10/10 已闭环抽验 5 项全过");
    s.add_dyn("处置账落档", ledger_archived_ok(), "12 行处置账落档指纹落账");
    s.add_dyn("悬空注入10/10", dangling_10_ok(), "十路悬空逐路必警警路恰 10");
    s.add_dyn("项号注入必抓", inject_badid_ok(), "越闭集/三位/零号/越号/空列六路必抓");
    s.add_dyn("重号注入必抓", inject_dup_ok(), "同项号二登必抓 IdDuplicate");
    s.add_dyn("状态倒退注入", inject_regress_ok(), "已闭环再定方案必拒 StatusRegress");
    s.add_dyn("率不足注入", inject_below_ok(), "留一项待办收口必拒悬空告警 1");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→闭环→落档可达，倒退拒");
    s.add_dyn("LCG抽验5项", sample_audit_ok(), "好账抽 5 项全过序列确定");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与处置账两次构建逐位相等");
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
        assert_eq!(SEG_BASE, 0x78D40);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1569_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn reserve_tables_aligned() {
        assert_eq!(RESERVES.len(), 10);
        assert_eq!(PLANS.len(), 10);
        assert_eq!(DISPOSAL_ROWS, 12);
        let mut i = 0usize;
        while i < RESERVES.len() {
            let r = match RESERVES.get(i) {
                Some(r) => *r,
                None => break,
            };
            let p = match PLANS.get(i) {
                Some(p) => *p,
                None => break,
            };
            assert_eq!(r.0, p.0, "项号须逐位对齐");
            assert!(!r.1.is_empty());
            assert!(!p.1.is_empty());
            i += 1;
        }
    }

    #[test]
    fn full_path_seals() {
        let bk = build_book_ok();
        assert_eq!(bk.row_count(), 10);
        assert_eq!(bk.closed_count(), 10);
        assert_eq!(bk.phase(), DisposalPhase::Sealed);
        assert_ne!(bk.ledger_digest(), 0);
    }

    #[test]
    fn dangling_without_plan_alarms() {
        let mut bk = DisposalBook::new();
        let row0 = match RESERVES.get(0) {
            Some(r) => *r,
            None => return,
        };
        assert!(bk.enroll(row0.0, row0.1, "").is_ok());
        assert_eq!(bk.scan_dangling(), 1);
        assert_eq!(bk.alarm_count(), 1);
    }

    #[test]
    fn dangling_injection_ten_routes() {
        assert_eq!(dangling_detect_count(), 10);
        assert_eq!(DANGLING_ROUTES_TABLE.len(), 10);
    }

    #[test]
    fn bad_id_formats_rejected() {
        let mut bk = DisposalBook::new();
        assert!(bk.enroll("R1", "一位序号", "方案").is_err());
        assert!(bk.enroll("R001", "三位序号", "方案").is_err());
        assert!(bk.enroll("R00", "零号", "方案").is_err());
        assert!(bk.enroll("R11", "越号", "方案").is_err());
        assert_eq!(bk.row_count(), 0);
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut bk = DisposalBook::new();
        let row3 = match RESERVES.get(3) {
            Some(r) => *r,
            None => return,
        };
        assert!(bk.enroll(row3.0, row3.1, "方案").is_ok());
        match bk.enroll(row3.0, "另述", "另案") {
            Err(code) => assert_eq!(code, Reserve69Code::IdDuplicate.val()),
            Ok(_) => assert!(false, "重号二登必须被拒"),
        }
    }

    #[test]
    fn close_requires_plan() {
        let mut bk = DisposalBook::new();
        let row5 = match RESERVES.get(5) {
            Some(r) => *r,
            None => return,
        };
        assert!(bk.enroll(row5.0, row5.1, "").is_ok());
        match bk.close(row5.0) {
            Err(code) => assert_eq!(code, Reserve69Code::PlanEmpty.val()),
            Ok(_) => assert!(false, "悬空项闭环必须被拒"),
        }
        assert!(bk.set_plan(row5.0, "补方案").is_ok());
        assert!(bk.close(row5.0).is_ok());
        assert_eq!(bk.closed_count(), 1);
    }

    #[test]
    fn close_all_short_rejected() {
        let mut bk = DisposalBook::new();
        let mut i = 0usize;
        while i < 5 {
            let row = match RESERVES.get(i) {
                Some(r) => *r,
                None => break,
            };
            let _ = bk.enroll(row.0, row.1, "方案");
            let _ = bk.close(row.0);
            i += 1;
        }
        match bk.close_all() {
            Err(code) => assert_eq!(code, Reserve69Code::CloseBelowHundred.val()),
            Ok(_) => assert!(false, "项数不足收口必须被拒"),
        }
    }

    #[test]
    fn seal_before_closed_rejected() {
        let mut bk = DisposalBook::new();
        match bk.seal_ledger() {
            Err(code) => assert_eq!(code, Reserve69Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "未闭环落档必须被拒"),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_book_ok().ledger_digest();
        let d2 = build_book_ok().ledger_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(10), b.next_mod(10));
        assert_eq!(a.next_mod(10), b.next_mod(10));
    }

    #[test]
    fn ledger_rows_and_markers() {
        let bk = build_book_ok();
        let text = match bk.text() {
            Some(t) => t.to_string(),
            None => {
                assert!(false, "处置账未落档");
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 12);
        assert!(text.contains("闭环率 100%"));
        assert!(text.contains("R10"));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Reserve69Code::ALL.len(), 12);
        let first = match Reserve69Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Reserve69Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
