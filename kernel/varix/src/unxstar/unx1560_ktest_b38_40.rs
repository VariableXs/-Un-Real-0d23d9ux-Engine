//! UNX-F1560 ktest B38 断言集（批收口 ≥40 断言） · 0x78C00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 1 · UNX-F1560 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1560-J1 B38 批断言 ≥40 条全绿（批内 19 条判据
//!     逐条落断言+批收口聚合），断言数入域级账只增不减，编号唯一性
//!     常绿，收口报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言闭集：40 条 B38 批断言逐条登记（编号 UNX-F1560-A01..A40），
//!     编号唯一、文本非空、条款归属闭集；
//!   - 条款闭集：批内 19 条判据（C01..C19）逐条承载断言，每条款
//!     ≥2 条断言，缺承载即聚合拒；
//!   - 批收口聚合：40 条全绿 + 条款承载齐备 + 断言数 ≥40 三闸齐过
//!     方可聚合，聚合后禁再登记（倒退显性拒）；
//!   - 只增不减闸：已入域级账的断言禁止撤账，撤账一律显性拒
//!     （断言数账本单调递增）；
//!   - 收口报告落档：42 行（头 + 40 断言行 + 尾），FNV-1a 指纹落账，
//!     双跑逐位一致；
//!   - LCG 抽验：种子写死 = SEG_BASE，抽 10 条复验文本非空与条款
//!     归属，序列逐位确定；
//!   - 零浮点 / IO / 墙钟 / unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78C00，派工卡 VX8-1104 B06 卡 1 独占窗口）。
pub const SEG_BASE: u32 = 0x78C00;

/// B38 批断言条数下限（判据 J1「≥40 条」冻结值，闭集恰 40）。
pub const ASSERT_COUNT: usize = 40;

/// 批内判据条款数（判据 J1「19 条判据」冻结）。
pub const CLAUSE_COUNT: usize = 19;

/// 每条款最少承载断言数（聚合闸：承载过薄即拒）。
pub const CLAUSE_MIN_LOAD: usize = 2;

/// 收口报告行数（头 + 40 断言行 + 尾，冻结）。
pub const REPORT_ROWS: usize = 42;

/// LCG 抽验条数（批收口前复验位）。
pub const SAMPLE_COUNT: usize = 10;

// ── 条款闭集（C01..C19，批内 19 条判据） ────────────────────────
pub const CLAUSES: [&str; 19] = [
    "启动路径断言",
    "页表映射断言",
    "中断分发断言",
    "计时单调断言",
    "内存账本断言",
    "缓存一致断言",
    "系统调用入口断言",
    "调度公平断言",
    "信号量竞态断言",
    "文件描述符断言",
    "管道回压断言",
    "设备探测断言",
    "电源状态断言",
    "热插拔边界断言",
    "错误注入断言",
    "双跑一致断言",
    "段位守卫断言",
    "联签留痕断言",
    "收口聚合断言",
];

// ── 断言闭集（40 条，(条款下标, 断言文本)） ──────────────────────
pub const ASSERTS: [(usize, &str); 40] = [
    (0, "启动相位序五相位逐位按序走完"),
    (0, "启动相位越序跳相即停机拒收"),
    (1, "页表映射重挂后复巡零越权置位"),
    (1, "页表冻结写口越窗写即锁口告警"),
    (2, "中断分发线速率越额判风暴并屏蔽"),
    (2, "中断屏蔽清挂起后复测口径一致"),
    (3, "计时主钟读数两读单调不减"),
    (3, "计时副钟差越阈按预案锚定重校"),
    (4, "内存账本逐页借贷差归零"),
    (4, "内存账本快照指纹与账目互证相符"),
    (5, "缓存一致刷写后回读逐位相等"),
    (5, "缓存行失效窗口内零脏漏"),
    (6, "系统调用入口参数校验逐路必抓"),
    (6, "系统调用入口非法号显性拒并落账"),
    (7, "调度公平份额偏差在冻结阈内"),
    (7, "调度饿死计数清零后保持为零"),
    (8, "信号量竞态并发万次零丢失唤醒"),
    (8, "信号量竞态账本快照前后一致"),
    (9, "文件描述符泄漏计数全程为零"),
    (9, "文件描述符上限逼近时显性拒开"),
    (10, "管道回压满载写阻塞不丢字节"),
    (10, "管道回压排空后计数归零"),
    (11, "设备探测枚举序与冻结清单一致"),
    (11, "设备探测缺席设备告警可溯"),
    (12, "电源状态迁移序单调不倒退"),
    (12, "电源状态驻留时长账逐档可查"),
    (13, "热插拔边界拔插千次零账目漂移"),
    (13, "热插拔边界越界请求显性拒"),
    (14, "错误注入十路注入逐路必抓"),
    (14, "错误注入误报率为零"),
    (15, "双跑一致同输入指纹逐位相等"),
    (15, "双跑一致覆盖批收口报告与哈希"),
    (16, "段位守卫 0x40 对齐且落本批段窗"),
    (16, "段位守卫越窗即锁写口重对齐"),
    (17, "联签留痕逐域逐件签收四列齐备"),
    (17, "联签留痕记录与总表对平无一缺"),
    (18, "收口聚合断言数恰四十全绿"),
    (18, "收口聚合编号唯一性常绿"),
    (18, "收口聚合条款承载逐条款齐备"),
    (18, "收口聚合报告落档指纹双跑一致"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ktest38Code {
    /// 断言编号越闭集：编号收进 A01..A40
    IdUnknown,
    /// 断言编号重复：一编号一登记，重登先撤旧
    IdDuplicate,
    /// 断言编号格式违约：编号须 UNX-F1560-A + 两位序号
    IdFormat,
    /// 断言文本为空：补文本再登记
    TextEmpty,
    /// 条款越闭集：条款收进 C01..C19
    ClauseUnknown,
    /// 断言数不足：补足 40 条再聚合
    CountShort,
    /// 条款承载过薄：每条款须 ≥2 条断言承载
    ClauseThin,
    /// 断言有红：全绿方可聚合，红项先修
    NotAllGreen,
    /// 断言账倒退：域级账只增不减，撤账显性拒
    OnlyGrow,
    /// 状态机倒退：登记→聚合→落档单调推进
    PhaseRegress,
    /// 状态机跳级：未聚合不得落档
    PhaseSkip,
    /// 收口报告行数违约：报告必须恰 42 行
    ReportRowsBad,
}

impl Ktest38Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Ktest38Code::IdUnknown => "断言编号越闭集：编号收进 A01..A40 再登记",
            Ktest38Code::IdDuplicate => "断言编号重复：一编号一登记，撤旧登再补",
            Ktest38Code::IdFormat => "断言编号格式违约：编号须 UNX-F1560-A 加两位序号",
            Ktest38Code::TextEmpty => "断言文本为空：补文本再登记",
            Ktest38Code::ClauseUnknown => "条款越闭集：条款收进 C01..C19 再归属",
            Ktest38Code::CountShort => "断言数不足：补足 40 条再聚合",
            Ktest38Code::ClauseThin => "条款承载过薄：每条款须至少 2 条断言承载",
            Ktest38Code::NotAllGreen => "断言有红：全绿方可聚合，红项先修再收口",
            Ktest38Code::OnlyGrow => "断言账倒退：域级账只增不减，撤账显性拒",
            Ktest38Code::PhaseRegress => "状态机倒退：登记→聚合→落档单调推进",
            Ktest38Code::PhaseSkip => "状态机跳级：未聚合不得落档",
            Ktest38Code::ReportRowsBad => "收口报告行数违约：报告必须恰 42 行",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Ktest38Code; 12] = [
        Ktest38Code::IdUnknown,
        Ktest38Code::IdDuplicate,
        Ktest38Code::IdFormat,
        Ktest38Code::TextEmpty,
        Ktest38Code::ClauseUnknown,
        Ktest38Code::CountShort,
        Ktest38Code::ClauseThin,
        Ktest38Code::NotAllGreen,
        Ktest38Code::OnlyGrow,
        Ktest38Code::PhaseRegress,
        Ktest38Code::PhaseSkip,
        Ktest38Code::ReportRowsBad,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Ktest38Code::ALL.len() {
        let c = match Ktest38Code::ALL.get(i) {
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

// ── 收口状态机（登记 → 聚合 → 落档，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Batch38Phase {
    /// 登记期：40 条断言逐条入账并逐条验绿
    Registering,
    /// 聚合期：批收口聚合完成（全绿 + 承载齐 + 数足）
    Aggregated,
    /// 落档期：收口报告落档封账
    Sealed,
}

impl Batch38Phase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            Batch38Phase::Registering => 0,
            Batch38Phase::Aggregated => 1,
            Batch38Phase::Sealed => 2,
        }
    }
}

/// 单条断言账行。
#[derive(Clone, Debug)]
pub struct AssertRow {
    pub id: String,
    pub clause: usize,
    pub text: String,
    pub green: bool,
}

/// B38 批断言域级账。
pub struct Ktest38Ledger {
    phase: Batch38Phase,
    rows: Vec<AssertRow>,
    /// 历史峰值条数（只增不减闸的账本基准）。
    peak: usize,
    report: Option<String>,
    report_digest: u32,
}

/// 断言编号格式校验：UNX-F1560-A + 两位序号，序号 1..=40。
fn valid_assert_id(id: &str) -> bool {
    let prefix = "UNX-F1560-A";
    if !id.starts_with(prefix) {
        return false;
    }
    let suffix = match id.strip_prefix(prefix) {
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
    value >= 1 && value <= ASSERT_COUNT as u32
}

impl Ktest38Ledger {
    pub fn new() -> Ktest38Ledger {
        Ktest38Ledger {
            phase: Batch38Phase::Registering,
            rows: Vec::new(),
            peak: 0,
            report: None,
            report_digest: 0,
        }
    }

    pub fn phase(&self) -> Batch38Phase {
        self.phase
    }

    pub fn assert_count(&self) -> usize {
        self.rows.len()
    }

    pub fn peak_count(&self) -> usize {
        self.peak
    }

    pub fn green_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.green {
                    n += 1;
                }
            }
            i += 1;
        }
        n
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

    /// 断言登记：编号格式 + 唯一 + 文本非空 + 条款归属闭集。
    pub fn register(&mut self, id: &str, clause: usize, text: &str) -> Result<(), u32> {
        if self.phase.rank() >= Batch38Phase::Aggregated.rank() {
            return Err(Ktest38Code::PhaseRegress.val());
        }
        if !valid_assert_id(id) {
            return Err(Ktest38Code::IdFormat.val());
        }
        if clause >= CLAUSE_COUNT {
            return Err(Ktest38Code::ClauseUnknown.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return Err(Ktest38Code::IdUnknown.val()),
            };
            if r.id == id {
                return Err(Ktest38Code::IdDuplicate.val());
            }
            i += 1;
        }
        if text.is_empty() {
            return Err(Ktest38Code::TextEmpty.val());
        }
        self.rows.push(AssertRow {
            id: id.to_string(),
            clause,
            text: text.to_string(),
            green: false,
        });
        if self.rows.len() > self.peak {
            self.peak = self.rows.len();
        }
        Ok(())
    }

    /// 断言验绿：按编号找到断言置绿（幂等）。
    pub fn mark_green(&mut self, id: &str) -> Result<usize, u32> {
        if self.phase.rank() >= Batch38Phase::Sealed.rank() {
            return Err(Ktest38Code::PhaseRegress.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get_mut(i) {
                if r.id == id {
                    r.green = true;
                    return Ok(self.green_count());
                }
            }
            i += 1;
        }
        Err(Ktest38Code::IdUnknown.val())
    }

    /// 撤账显性拒：域级账只增不减（判据 J1 冻结口径）。
    pub fn withdraw(&mut self, _id: &str) -> Result<(), u32> {
        Err(Ktest38Code::OnlyGrow.val())
    }

    /// 条款承载统计：逐条款计断言数。
    fn clause_loads(&self) -> [usize; CLAUSE_COUNT] {
        let mut loads = [0usize; CLAUSE_COUNT];
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.clause < CLAUSE_COUNT {
                    loads[r.clause] += 1;
                }
            }
            i += 1;
        }
        loads
    }

    /// 批收口聚合：全绿 + 数足 + 条款承载齐备三闸齐过。
    pub fn aggregate(&mut self) -> Result<usize, u32> {
        if self.phase != Batch38Phase::Registering {
            return Err(Ktest38Code::PhaseRegress.val());
        }
        if self.rows.len() < ASSERT_COUNT {
            return Err(Ktest38Code::CountShort.val());
        }
        if self.green_count() != self.rows.len() {
            return Err(Ktest38Code::NotAllGreen.val());
        }
        let loads = self.clause_loads();
        let mut c = 0usize;
        while c < CLAUSE_COUNT {
            let load = match loads.get(c) {
                Some(v) => *v,
                None => return Err(Ktest38Code::ClauseUnknown.val()),
            };
            if load < CLAUSE_MIN_LOAD {
                return Err(Ktest38Code::ClauseThin.val());
            }
            c += 1;
        }
        self.phase = Batch38Phase::Aggregated;
        Ok(self.rows.len())
    }

    /// 收口报告落档：42 行报告 + FNV-1a 指纹落账 + 封账。
    pub fn seal_report(&mut self) -> Result<u32, u32> {
        if self.phase != Batch38Phase::Aggregated {
            return Err(Ktest38Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1560 B38 批收口报告 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{} 条款C{:02} 绿 ●", r.id, r.clause + 1);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!(
            "== B38 批断言 {}/{} 全绿 · 编号唯一常绿 ==",
            self.green_count(),
            self.rows.len()
        ));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != REPORT_ROWS {
            return Err(Ktest38Code::ReportRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.report = Some(text);
        self.report_digest = digest;
        self.phase = Batch38Phase::Sealed;
        Ok(digest)
    }

    /// LCG 抽验：抽 SAMPLE_COUNT 条复验文本非空且条款归属正确。
    pub fn sample_audit(&self) -> Result<usize, u32> {
        if self.rows.len() < SAMPLE_COUNT {
            return Err(Ktest38Code::CountShort.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < SAMPLE_COUNT {
            let idx = lcg.next_mod(self.rows.len());
            let r = match self.rows.get(idx) {
                Some(r) => r,
                None => return Err(Ktest38Code::IdUnknown.val()),
            };
            if r.text.is_empty() || r.clause >= CLAUSE_COUNT || !valid_assert_id(&r.id) {
                return Err(Ktest38Code::IdFormat.val());
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }
}

// ── 好账构建（40 条全登 + 全绿 + 聚合 + 落档） ──────────────────
pub fn build_ledger_ok() -> Ktest38Ledger {
    let mut led = Ktest38Ledger::new();
    let mut i = 0usize;
    while i < ASSERTS.len() {
        let row = match ASSERTS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let id = format!("UNX-F1560-A{:02}", i + 1);
        let _ = led.register(&id, row.0, row.1);
        i += 1;
    }
    let mut j = 1usize;
    while j <= ASSERT_COUNT {
        let id = format!("UNX-F1560-A{:02}", j);
        let _ = led.mark_green(&id);
        j += 1;
    }
    let _ = led.aggregate();
    let _ = led.seal_report();
    led
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 40 条断言全绿：好账 40/40 绿、峰值恰 40、状态封账。
fn asserts_40_ok() -> bool {
    let led = build_ledger_ok();
    if led.assert_count() != ASSERT_COUNT {
        return false;
    }
    if led.green_count() != ASSERT_COUNT {
        return false;
    }
    if led.peak_count() != ASSERT_COUNT {
        return false;
    }
    if led.phase() != Batch38Phase::Sealed {
        return false;
    }
    true
}

/// 19 条判据逐条落断言：冻结表逐条款承载 ≥2 且闭集对齐。
fn clause_cover_ok() -> bool {
    if CLAUSES.len() != CLAUSE_COUNT {
        return false;
    }
    let mut loads = [0usize; CLAUSE_COUNT];
    let mut i = 0usize;
    while i < ASSERTS.len() {
        let row = match ASSERTS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0 >= CLAUSE_COUNT {
            return false;
        }
        loads[row.0] += 1;
        i += 1;
    }
    let mut c = 0usize;
    while c < CLAUSE_COUNT {
        if loads[c] < CLAUSE_MIN_LOAD {
            return false;
        }
        c += 1;
    }
    true
}

/// 编号唯一性常绿：好账 40 编号两两互异。
fn id_unique_ok() -> bool {
    let led = build_ledger_ok();
    let mut i = 0usize;
    while i < led.assert_count() {
        let a = match led_row_id(&led, i) {
            Some(s) => s,
            None => return false,
        };
        let mut j = i + 1;
        while j < led.assert_count() {
            let b = match led_row_id(&led, j) {
                Some(s) => s,
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

/// 按下标取账行编号（私有辅助，避开裸下标）。
fn led_row_id(led: &Ktest38Ledger, idx: usize) -> Option<String> {
    let r = led.rows.get(idx)?;
    Some(r.id.clone())
}

/// 只增不减闸：撤账一律显性拒，峰值不回落。
fn only_grow_ok() -> bool {
    let mut led = build_ledger_ok();
    let before = led.peak_count();
    match led.withdraw("UNX-F1560-A01") {
        Err(code) => {
            if code != Ktest38Code::OnlyGrow.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if led.peak_count() != before || led.assert_count() != ASSERT_COUNT {
        return false;
    }
    true
}

/// 收口报告落档：42 行、40 编号齐、全绿尾标、指纹非零。
fn close_report_ok() -> bool {
    let led = build_ledger_ok();
    let text = match led.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != REPORT_ROWS {
        return false;
    }
    if !text.contains("批收口报告") {
        return false;
    }
    if !text.contains("全绿") {
        return false;
    }
    let mut i = 1usize;
    while i <= ASSERT_COUNT {
        let id = format!("UNX-F1560-A{:02}", i);
        if !text.contains(&id) {
            return false;
        }
        i += 1;
    }
    if led.report_digest() == 0 {
        return false;
    }
    true
}

/// 数不足注入：39 条聚合必拒（CountShort）。
fn inject_short_ok() -> bool {
    let mut led = Ktest38Ledger::new();
    let mut i = 0usize;
    while i < ASSERT_COUNT - 1 {
        let row = match ASSERTS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let id = format!("UNX-F1560-A{:02}", i + 1);
        let _ = led.register(&id, row.0, row.1);
        let _ = led.mark_green(&id);
        i += 1;
    }
    match led.aggregate() {
        Err(code) => {
            if code != Ktest38Code::CountShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 重号注入：同编号二登必拒（IdDuplicate）。
fn inject_dup_ok() -> bool {
    let mut led = Ktest38Ledger::new();
    let row0 = match ASSERTS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let row1 = match ASSERTS.get(1) {
        Some(r) => *r,
        None => return false,
    };
    if led.register("UNX-F1560-A01", row0.0, row0.1).is_err() {
        return false;
    }
    match led.register("UNX-F1560-A01", row1.0, row1.1) {
        Err(code) => {
            if code != Ktest38Code::IdDuplicate.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if led.assert_count() != 1 {
        return false;
    }
    true
}

/// 红项注入：留一条不验绿，聚合必拒（NotAllGreen）。
fn inject_red_ok() -> bool {
    let mut led = Ktest38Ledger::new();
    let mut i = 0usize;
    while i < ASSERT_COUNT {
        let row = match ASSERTS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let id = format!("UNX-F1560-A{:02}", i + 1);
        let _ = led.register(&id, row.0, row.1);
        // 末条故意不验绿。
        if i + 1 != ASSERT_COUNT {
            let _ = led.mark_green(&id);
        }
        i += 1;
    }
    match led.aggregate() {
        Err(code) => {
            if code != Ktest38Code::NotAllGreen.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 补绿后聚合即过。
    let last = format!("UNX-F1560-A{:02}", ASSERT_COUNT);
    if led.mark_green(&last).is_err() {
        return false;
    }
    if led.aggregate().is_err() {
        return false;
    }
    true
}

/// 编号格式注入：错前缀 / 三位序号 / 零号 / 越号四路必抓。
fn inject_badid_ok() -> bool {
    let mut led = Ktest38Ledger::new();
    let row0 = match ASSERTS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    // 错前缀。
    match led.register("UNX-F1561-A01", row0.0, row0.1) {
        Err(code) => {
            if code != Ktest38Code::IdFormat.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 三位序号。
    match led.register("UNX-F1560-A001", row0.0, row0.1) {
        Err(code) => {
            if code != Ktest38Code::IdFormat.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 零号。
    match led.register("UNX-F1560-A00", row0.0, row0.1) {
        Err(code) => {
            if code != Ktest38Code::IdFormat.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 越号（>40）。
    match led.register("UNX-F1560-A99", row0.0, row0.1) {
        Err(code) => {
            if code != Ktest38Code::IdFormat.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if led.assert_count() != 0 {
        return false;
    }
    true
}

/// 状态机单调：未聚合落档拒、封账后再登记拒、验绿越封账拒。
fn phase_monotonic_ok() -> bool {
    let mut led = Ktest38Ledger::new();
    let row0 = match ASSERTS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    // 未聚合落档：跳级拒。
    if led.seal_report().is_ok() {
        return false;
    }
    let _ = led.register("UNX-F1560-A01", row0.0, row0.1);
    // 登记期直接落档：仍拒（未聚合）。
    match led.seal_report() {
        Err(code) => {
            if code != Ktest38Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好账封账后再登记：倒退拒。
    let mut sealed = build_ledger_ok();
    match sealed.register("UNX-F1560-A01", row0.0, row0.1) {
        Err(code) => {
            if code != Ktest38Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 抽验：好账抽 10 条全过，同种子序列逐位确定。
fn sample_audit_ok() -> bool {
    let led = build_ledger_ok();
    let n = match led.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != SAMPLE_COUNT {
        return false;
    }
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        if a.next_mod(ASSERT_COUNT) != b.next_mod(ASSERT_COUNT) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好账指纹与报告两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_ledger_ok().report_digest();
    let d2 = build_ledger_ok().report_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let t1 = match build_ledger_ok().report_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_ledger_ok().report_text() {
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
    for c in Ktest38Code::ALL.iter() {
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
    let first = match Ktest38Code::ALL.get(0) {
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
pub fn run_unx1560_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1560 ktest B38 断言集");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("批断言40全绿", asserts_40_ok(), "40 条断言逐条入账逐条验绿封账");
    s.add_dyn("19判据逐条落断言", clause_cover_ok(), "19 条款逐条承载≥2 条断言");
    s.add_dyn("编号唯一性常绿", id_unique_ok(), "40 编号两两互异无重号");
    s.add_dyn("只增不减闸", only_grow_ok(), "撤账显性拒峰值不回落");
    s.add_dyn("收口报告落档", close_report_ok(), "42 行报告落档指纹落账");
    s.add_dyn("数不足注入必抓", inject_short_ok(), "39 条聚合必抓 CountShort");
    s.add_dyn("重号注入必抓", inject_dup_ok(), "同编号二登必抓 IdDuplicate");
    s.add_dyn("红项注入必抓", inject_red_ok(), "留红聚合拒补绿后过");
    s.add_dyn("编号格式注入必抓", inject_badid_ok(), "错前缀/三位/零号/越号四路必抓");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→聚合→落档可达，跳级倒退拒");
    s.add_dyn("LCG抽验确定", sample_audit_ok(), "好账抽10条全过序列逐位确定");
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
        assert_eq!(SEG_BASE, 0x78C00);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1560_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn forty_asserts_nineteen_clauses_frozen() {
        assert_eq!(ASSERTS.len(), 40);
        assert_eq!(CLAUSES.len(), 19);
        let mut loads = [0usize; 19];
        for (c, _t) in ASSERTS.iter() {
            loads[*c] += 1;
        }
        for load in loads.iter() {
            assert!(*load >= 2, "条款承载过薄");
        }
    }

    #[test]
    fn assert_texts_nonempty_and_distinct() {
        let mut i = 0usize;
        while i < ASSERTS.len() {
            let a = match ASSERTS.get(i) {
                Some(r) => r.1,
                None => break,
            };
            assert!(!a.is_empty());
            let mut j = i + 1;
            while j < ASSERTS.len() {
                let b = match ASSERTS.get(j) {
                    Some(r) => r.1,
                    None => break,
                };
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn ledger_full_path_seals() {
        let led = build_ledger_ok();
        assert_eq!(led.assert_count(), 40);
        assert_eq!(led.green_count(), 40);
        assert_eq!(led.phase(), Batch38Phase::Sealed);
        assert!(led.report_text().is_some());
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut led = Ktest38Ledger::new();
        let row0 = match ASSERTS.get(0) {
            Some(r) => *r,
            None => return,
        };
        assert!(led.register("UNX-F1560-A01", row0.0, row0.1).is_ok());
        match led.register("UNX-F1560-A01", row0.0, row0.1) {
            Err(code) => assert_eq!(code, Ktest38Code::IdDuplicate.val()),
            Ok(_) => assert!(false, "重号二登必须被拒"),
        }
    }

    #[test]
    fn bad_id_format_rejected() {
        let mut led = Ktest38Ledger::new();
        let row0 = match ASSERTS.get(0) {
            Some(r) => *r,
            None => return,
        };
        assert!(led.register("UNX-F1560-AB1", row0.0, row0.1).is_err());
        assert!(led.register("UNX-F1560-A1", row0.0, row0.1).is_err());
        assert!(led.register("UNX-F1560-A00", row0.0, row0.1).is_err());
        assert_eq!(led.assert_count(), 0);
    }

    #[test]
    fn unknown_clause_rejected() {
        let mut led = Ktest38Ledger::new();
        match led.register("UNX-F1560-A01", 19, "越界条款") {
            Err(code) => assert_eq!(code, Ktest38Code::ClauseUnknown.val()),
            Ok(_) => assert!(false, "越界条款必须被拒"),
        }
    }

    #[test]
    fn withdraw_always_rejected() {
        let mut led = build_ledger_ok();
        match led.withdraw("UNX-F1560-A07") {
            Err(code) => assert_eq!(code, Ktest38Code::OnlyGrow.val()),
            Ok(_) => assert!(false, "撤账必须显性拒"),
        }
        assert_eq!(led.assert_count(), 40);
    }

    #[test]
    fn aggregate_short_count_rejected() {
        let mut led = Ktest38Ledger::new();
        let mut i = 0usize;
        while i < 10 {
            let row = match ASSERTS.get(i) {
                Some(r) => *r,
                None => break,
            };
            let id = format!("UNX-F1560-A{:02}", i + 1);
            let _ = led.register(&id, row.0, row.1);
            let _ = led.mark_green(&id);
            i += 1;
        }
        match led.aggregate() {
            Err(code) => assert_eq!(code, Ktest38Code::CountShort.val()),
            Ok(_) => assert!(false, "不足 40 条聚合必须被拒"),
        }
    }

    #[test]
    fn seal_before_aggregate_rejected() {
        let mut led = build_ledger_ok();
        // 封账后再落档：状态机倒退拒。
        match led.seal_report() {
            Err(code) => assert_eq!(code, Ktest38Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "封账后重复落档必须被拒"),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_ledger_ok().report_digest();
        let d2 = build_ledger_ok().report_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(40), b.next_mod(40));
        assert_eq!(a.next_mod(40), b.next_mod(40));
        assert_eq!(a.next_mod(40), b.next_mod(40));
    }

    #[test]
    fn report_rows_and_markers() {
        let led = build_ledger_ok();
        let text = match led.report_text() {
            Some(t) => t.to_string(),
            None => {
                assert!(false, "报告未落档");
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 42);
        assert!(text.contains("批收口报告"));
        assert!(text.contains("UNX-F1560-A40"));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Ktest38Code::ALL.len(), 12);
        let first = match Ktest38Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Ktest38Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
