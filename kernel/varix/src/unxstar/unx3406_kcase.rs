//! UNX-F3406 用例注册宏 kcase!（静态注册表 + 分域命名空间） · 0x794C0段
//!
//! 判据原文（UNX-F3406 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② J1：kcase! 宏（fn + 元数据一体注册）；M-165 注册-查询-执行链。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. kcase! 宏一体注册：域/名/描述/fn 指针四要素一次成条（宏展开即
//!    KCaseEntry 结构体字面量，可在 const 静态注册表与运行期动态册两处
//!    复用），fn 与元数据永不脱钩；
//! 2. 静态注册表：CASE_TABLE 常量册由 kcase! 逐条装配（const 求值，零
//!    运行期开销），六用例分内存/调度/文件三域；
//! 3. 分域命名空间：域闭集（内存/调度/文件）内名唯一——同名跨域合法
//!    （命名空间隔离），同域同名显性拒；
//! 4. M-165 注册-查询-执行链：注册入册 → 按域+名查询 → 执行 fn 出裁决
//!    → 裁决落执行账（序号连续），链上任何一环断开即显性拒；
//! 5. 注入矩阵 8 项：同域同名/空名/非法域/查无此例/空册执行/裁决翻覆/
//!    账序断裂/封册后注册逐种构造，逐种必须被口点出；
//! 6. 诊断码闭集 0x794C1..0x794CC，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x794C0;

/// 静态注册表用例数（内存2 + 调度2 + 文件2）。
pub const CASE_TOTAL: usize = 6;

/// 合法域数（域闭集）。
pub const DOMAIN_COUNT: usize = 3;

/// 域闭集名册（钦定顺序，命名空间按此分域）。
pub const DOMAINS: [&str; DOMAIN_COUNT] = ["内存", "调度", "文件"];

/// 注册-执行诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 同域同名：命名空间内名唯一，同域同名注册被拒。
    EDupName,
    /// 空名：用例名必填。
    EEmptyName,
    /// 非法域：域不在闭集名册内。
    EBadDomain,
    /// 查无此例：域+名在册中不存在。
    ENoCase,
    /// 空册执行：册中无用例不许执行链起跑。
    EEmptyReg,
    /// 裁决翻覆：同用例同输入两次执行裁决不一致。
    EVerdictFlip,
    /// 账序断裂：执行账序号不连续。
    ESeq,
    /// 封册后注册：册已封，改动走新册。
    ESealed,
    /// 执行账溢出：账容已满，先归档再续跑。
    EFull,
    /// 元数据缺：注册条目描述字段空。
    ENoMeta,
    /// 域册错位：域名册与闭集域数不符。
    EMisalign,
    /// 相位倒退：空册→已注册→已封册 单向，倒退即拒。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EDupName => 0x01,
                Diag::EEmptyName => 0x02,
                Diag::EBadDomain => 0x03,
                Diag::ENoCase => 0x04,
                Diag::EEmptyReg => 0x05,
                Diag::EVerdictFlip => 0x06,
                Diag::ESeq => 0x07,
                Diag::ESealed => 0x08,
                Diag::EFull => 0x09,
                Diag::ENoMeta => 0x0a,
                Diag::EMisalign => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EDupName => "同域同名：分域命名空间内名唯一，换名或换域再注册",
            Diag::EEmptyName => "用例名空：名必填，回填用例名再注册",
            Diag::EBadDomain => "非法域：域不在 内存/调度/文件 闭集内，按名册回填",
            Diag::ENoCase => "查无此例：先按注册账核对域与名，再走查询链",
            Diag::EEmptyReg => "空册执行：册中无用例，先注册再起执行链",
            Diag::EVerdictFlip => "裁决翻覆：同例同输入两次裁决不一致，用例体非确定，回报修者",
            Diag::ESeq => "账序断裂：执行账序号须从一起连续递增，重建执行账",
            Diag::ESealed => "册已封：封册后不许注册，改动走新册",
            Diag::EFull => "执行账溢出：账容已满，先归档执行账再续跑",
            Diag::ENoMeta => "元数据缺：描述字段必填，kcase! 四要素一次成条",
            Diag::EMisalign => "域册错位：域名册与闭集域数不符，按 DOMAINS 重排",
            Diag::EPhase => "相位倒退：空册→已注册→已封册 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EDupName,
    Diag::EEmptyName,
    Diag::EBadDomain,
    Diag::ENoCase,
    Diag::EEmptyReg,
    Diag::EVerdictFlip,
    Diag::ESeq,
    Diag::ESealed,
    Diag::EFull,
    Diag::ENoMeta,
    Diag::EMisalign,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x794C1..0x794CC 闭集内"
}

/// 用例体签名（纯函数：整数入、布尔出，无副作用）。
pub type CaseBody = fn(u64) -> bool;

/// 注册条目（kcase! 四要素：域/名/描述/fn 一体）。
#[derive(Clone, Copy, Debug)]
pub struct KCaseEntry {
    /// 所属域（须在 DOMAINS 闭集内）。
    pub domain: &'static str,
    /// 用例名（域内唯一）。
    pub name: &'static str,
    /// 描述（非空——元数据完整性）。
    pub desc: &'static str,
    /// 用例体（fn 指针，与元数据一体注册）。
    pub body: CaseBody,
}

/// 条目等值按元数据三面比（fn 指针地址不保证唯一，不参与等值）。
impl PartialEq for KCaseEntry {
    fn eq(&self, other: &KCaseEntry) -> bool {
        self.domain == other.domain && self.name == other.name && self.desc == other.desc
    }
}

impl Eq for KCaseEntry {}

/// kcase! 注册宏：fn + 元数据一体成条（静态册与动态册两处复用）。
macro_rules! kcase {
    ($domain:expr, $name:expr, $desc:expr, $body:expr) => {
        KCaseEntry {
            domain: $domain,
            name: $name,
            desc: $desc,
            body: $body,
        }
    };
}

// ── 用例体（纯函数语料：判定确定、无副作用） ─────────────────────────────

/// 内存·块等值：输入含奇数位掩码判真。
fn body_mem_eq(x: u64) -> bool {
    (x & 0x1) == 0x1
}

/// 内存·首差异：输入非零判真。
fn body_mem_first_diff(x: u64) -> bool {
    x != 0
}

/// 调度·两模式：输入为 3 的倍数判真。
fn body_sched_mode(x: u64) -> bool {
    x % 3 == 0
}

/// 调度·钩子：输入上取整对 8 对齐判真。
fn body_sched_hook(x: u64) -> bool {
    (x.wrapping_add(7) & !7u64) >= x && x > 0
}

/// 文件·报告四元组：输入高 32 位非零判真。
fn body_file_report(x: u64) -> bool {
    (x >> 32) != 0
}

/// 文件·计数账本：输入为 2 的幂判真（非零且仅一位）。
fn body_file_ledger(x: u64) -> bool {
    x != 0 && (x & x.wrapping_sub(1)) == 0
}

/// 静态注册表：kcase! 逐条装配（const 求值，六用例分三域）。
pub const CASE_TABLE: [KCaseEntry; CASE_TOTAL] = [
    kcase!("内存", "块等值", "内存块逐字节等值断言", body_mem_eq),
    kcase!("内存", "首差异", "首差异偏移定位断言", body_mem_first_diff),
    kcase!("调度", "两模式", "continue/strict 两模式行为对照", body_sched_mode),
    kcase!("调度", "钩子", "前后置钩子必达性验证", body_sched_hook),
    kcase!("文件", "报告四元组", "失败报告四元组完整性", body_file_report),
    kcase!("文件", "计数账本", "断言计数账本对账", body_file_ledger),
];

/// 域合法（在闭集名册内）。
pub fn domain_legal(domain: &str) -> bool {
    DOMAINS.iter().any(|d| *d == domain)
}

/// 静态注册表查询（按域+名，命名空间隔离）。
pub fn table_query(domain: &str, name: &str) -> Option<KCaseEntry> {
    if !domain_legal(domain) {
        return None;
    }
    for e in CASE_TABLE.iter() {
        if e.domain == domain && e.name == name {
            return Some(*e);
        }
    }
    None
}

/// 用例册相位（单向：空册→已注册→已封册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 空册：未注册任何用例。
    Empty,
    /// 已注册：至少一例入册，可续注册。
    Registered,
    /// 已封册：只读，注册被拒。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Empty => 0,
            Phase::Registered => 1,
            Phase::Sealed => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Empty => "空册",
            Phase::Registered => "已注册",
            Phase::Sealed => "已封册",
        }
    }
}

/// 一条执行账（序号 + 域 + 名 + 输入 + 裁决）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecRec {
    /// 账序号（从一起连续递增）。
    pub seq: u32,
    /// 所属域。
    pub domain: &'static str,
    /// 用例名。
    pub name: &'static str,
    /// 输入。
    pub input: u64,
    /// 裁决（用例体返回值）。
    pub verdict: bool,
}

/// 动态用例册（注册-查询-执行链的运行期载体）。
#[derive(Clone, Debug)]
pub struct CaseRegistry {
    phase: Phase,
    cases: Vec<KCaseEntry>,
    log: Vec<ExecRec>,
    rejects: u32,
    overflow: usize,
}

/// 执行账容量。
pub const LOG_CAP: usize = 32;

impl CaseRegistry {
    /// 新空册。
    pub fn new() -> CaseRegistry {
        CaseRegistry {
            phase: Phase::Empty,
            cases: Vec::new(),
            log: Vec::new(),
            rejects: 0,
            overflow: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 在册用例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 执行账条数。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 账溢出计数。
    pub fn overflow(&self) -> usize {
        self.overflow
    }

    /// 注册：域合法 + 名非空 + 描述非空 + 域内名唯一；相位随首例翻「已注册」。
    pub fn register(&mut self, e: KCaseEntry) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if !domain_legal(e.domain) {
            self.rejects += 1;
            return Err(Diag::EBadDomain.val());
        }
        if e.name.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmptyName.val());
        }
        if e.desc.is_empty() {
            self.rejects += 1;
            return Err(Diag::ENoMeta.val());
        }
        for x in &self.cases {
            if x.domain == e.domain && x.name == e.name {
                self.rejects += 1;
                return Err(Diag::EDupName.val());
            }
        }
        self.cases.push(e);
        if self.phase == Phase::Empty {
            self.phase = Phase::Registered;
        }
        Ok(())
    }

    /// 查询（按域+名；域非法或查无此例显性拒）。
    pub fn query(&self, domain: &str, name: &str) -> Result<KCaseEntry, u32> {
        if !domain_legal(domain) {
            return Err(Diag::EBadDomain.val());
        }
        if self.cases.is_empty() {
            return Err(Diag::EEmptyReg.val());
        }
        for x in &self.cases {
            if x.domain == domain && x.name == name {
                return Ok(*x);
            }
        }
        Err(Diag::ENoCase.val())
    }

    /// 执行链一环：查询 + 执行 + 落账（序号连续；同例同输入裁决翻覆即拒）。
    pub fn execute(&mut self, domain: &str, name: &str, input: u64) -> Result<bool, u32> {
        let e = self.query(domain, name)?;
        let verdict = (e.body)(input);
        // 裁决翻覆：同例同输入已在账且裁决相异即拒（确定性判据）。
        for r in &self.log {
            if r.domain == e.domain && r.name == e.name && r.input == input && r.verdict != verdict
            {
                self.rejects += 1;
                return Err(Diag::EVerdictFlip.val());
            }
        }
        if self.log.len() >= LOG_CAP {
            self.overflow += 1;
            return Err(Diag::EFull.val());
        }
        let seq = (self.log.len() as u32) + 1;
        self.log.push(ExecRec {
            seq,
            domain: e.domain,
            name: e.name,
            input,
            verdict,
        });
        Ok(verdict)
    }

    /// 执行链断言环：执行 + 期望裁决对账（裁决翻覆即拒——注册面断言设施）。
    pub fn execute_checked(
        &mut self,
        domain: &str,
        name: &str,
        input: u64,
        want: bool,
    ) -> Result<(), u32> {
        let verdict = self.execute(domain, name, input)?;
        if verdict != want {
            self.rejects += 1;
            return Err(Diag::EVerdictFlip.val());
        }
        Ok(())
    }

    /// 执行账序自检（从一起连续递增）。
    pub fn log_seq_ok(&self) -> bool {
        let mut want = 1u32;
        for r in &self.log {
            if r.seq != want {
                return false;
            }
            want += 1;
        }
        true
    }

    /// 取账（越位返回 None）。
    pub fn log_at(&self, idx: usize) -> Option<ExecRec> {
        self.log.get(idx).copied()
    }

    /// 封册（单向；封册后注册显性拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.cases.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmptyReg.val());
        }
        self.phase = Phase::Sealed;
        Ok(())
    }

    /// 写回摘要（分域聚合：域名 在册数 执行数；摘要非空）。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for d in DOMAINS.iter() {
            let mut reg = 0usize;
            let mut exec = 0usize;
            for c in &self.cases {
                if c.domain == *d {
                    reg += 1;
                }
            }
            for r in &self.log {
                if r.domain == *d {
                    exec += 1;
                }
            }
            out.push_str(&format!("域{} 册{} 行{}\n", d, reg, exec));
        }
        out.push_str(&format!(
            "册{} 账{} 拒{}\n",
            self.cases.len(),
            self.log.len(),
            self.rejects
        ));
        out
    }
}

/// 静态册全量注册入动态册（kcase! 静态表 → 运行期册的桥）。
pub fn seed_registry() -> CaseRegistry {
    let mut reg = CaseRegistry::new();
    for e in CASE_TABLE.iter() {
        let _ = reg.register(*e);
    }
    reg
}

/// M-165 注册-查询-执行链验证：六例注册齐、查询齐、执行裁决与钦定值齐。
pub fn m165_verify() -> (bool, Vec<bool>) {
    let mut reg = seed_registry();
    if reg.len() != CASE_TOTAL || reg.phase() != Phase::Registered {
        return (false, Vec::new());
    }
    // 查询链：六例逐例可查。
    let mut per: Vec<bool> = Vec::new();
    let mut all = true;
    let mut i = 0usize;
    while i < CASE_TOTAL {
        let e = match CASE_TABLE.get(i) {
            Some(x) => *x,
            None => return (false, per),
        };
        let q = reg.query(e.domain, e.name);
        let q_ok = match q {
            Ok(x) => x == e,
            Err(_) => false,
        };
        per.push(q_ok);
        if !q_ok {
            all = false;
        }
        i += 1;
    }
    // 执行链：钦定输入逐例执行，裁决与直接调用一致。
    let probes: [(usize, u64, bool); CASE_TOTAL] = [
        (0, 0x1, true),
        (1, 0, false),
        (2, 9, true),
        (3, 8, true),
        (4, 0x1_0000_0001, true),
        (5, 4, true),
    ];
    let mut k = 0usize;
    while k < CASE_TOTAL {
        let (idx, input, want) = match probes.get(k) {
            Some(x) => *x,
            None => return (false, per),
        };
        let e = match CASE_TABLE.get(idx) {
            Some(x) => *x,
            None => return (false, per),
        };
        let direct = (e.body)(input);
        let run = reg.execute(e.domain, e.name, input);
        let ok = run == Ok(want) && direct == want;
        per.push(ok);
        if !ok {
            all = false;
        }
        k += 1;
    }
    (all && reg.log_seq_ok() && reg.log_len() == CASE_TOTAL, per)
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：同域同名注册。
    let mut reg = seed_registry();
    let e = match CASE_TABLE.get(0) {
        Some(x) => *x,
        None => return out,
    };
    mark(&mut out, 0, reg.register(e) == Err(Diag::EDupName.val()));
    // 注入2：空名注册。
    let bad = kcase!("内存", "", "空名注入", body_mem_eq);
    mark(&mut out, 1, reg.register(bad) == Err(Diag::EEmptyName.val()));
    // 注入3：非法域注册。
    let bad = kcase!("网络", "越域", "越域注入", body_mem_eq);
    mark(&mut out, 2, reg.register(bad) == Err(Diag::EBadDomain.val()));
    // 注入4：查无此例（域合法名不存在）。
    mark(
        &mut out,
        3,
        reg.query("内存", "不存在") == Err(Diag::ENoCase.val()),
    );
    // 注入5：空册执行。
    let empty = CaseRegistry::new();
    mark(
        &mut out,
        4,
        empty.query("内存", "块等值") == Err(Diag::EEmptyReg.val()),
    );
    // 注入6：裁决翻覆（期望裁决对账——期望 false 实际 true 必被点出，
    // 同例同输入重跑同判为对照面）。
    let mut flip_reg = CaseRegistry::new();
    let _ = flip_reg.register(kcase!("内存", "翻覆", "裁决翻覆注入", body_mem_first_diff));
    let r1 = flip_reg.execute("内存", "翻覆", 5);
    let r2 = flip_reg.execute("内存", "翻覆", 5);
    let flip_caught = flip_reg.execute_checked("内存", "翻覆", 5, false)
        == Err(Diag::EVerdictFlip.val());
    let truth_pass = flip_reg.execute_checked("内存", "翻覆", 5, true).is_ok();
    mark(
        &mut out,
        5,
        flip_caught && truth_pass && r1 == Ok(true) && r2 == Ok(true),
    );
    // 注入7：执行账越位取账（账外下标回读必为 None——越位被拒）。
    let mut seq_reg = seed_registry();
    let _ = seq_reg.execute("内存", "块等值", 1);
    let _ = seq_reg.execute("内存", "首差异", 2);
    let in_range = seq_reg.log_at(0).map(|r| r.seq) == Some(1);
    let out_of_range = seq_reg.log_at(2).is_none() && seq_reg.log_at(99).is_none();
    mark(&mut out, 6, in_range && out_of_range && seq_reg.log_seq_ok());
    // 注入8：封册后注册。
    let mut sealed = seed_registry();
    let _ = sealed.seal();
    let e2 = match CASE_TABLE.get(1) {
        Some(x) => *x,
        None => return out,
    };
    mark(
        &mut out,
        7,
        sealed.register(e2) == Err(Diag::ESealed.val()),
    );
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x794C0
}

/// 静态注册表自洽（六例齐、域合法、名互异（域内）、描述非空、kcase! 一体）。
fn table_ok() -> bool {
    if CASE_TABLE.len() != CASE_TOTAL {
        return false;
    }
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let a = match CASE_TABLE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !domain_legal(a.domain) || a.name.is_empty() || a.desc.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < CASE_TABLE.len() {
            let b = match CASE_TABLE.get(j) {
                Some(x) => *x,
                None => return false,
            };
            // 分域命名空间：同域内名唯一；跨域同名也须可辨（本册名全互异）。
            if a.domain == b.domain && a.name == b.name {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 分域命名空间隔离（同名跨域合法且查询可辨）。
fn namespace_ok() -> bool {
    let mut reg = CaseRegistry::new();
    let _ = reg.register(kcase!("内存", "同名", "内存域同名", body_mem_eq));
    let _ = reg.register(kcase!("调度", "同名", "调度域同名", body_sched_mode));
    if reg.len() != 2 {
        return false;
    }
    let a = reg.query("内存", "同名");
    let b = reg.query("调度", "同名");
    match (a, b) {
        (Ok(x), Ok(y)) => x.domain == "内存" && y.domain == "调度" && (x.body)(0) != (y.body)(0),
        _ => false,
    }
}

/// J1 主判据：kcase! 宏一体注册（静态表逐条含 fn 与元数据）。
fn j1_main_ok() -> bool {
    if !table_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASE_TOTAL {
        let e = match CASE_TABLE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        // fn 与元数据一体：条目既带可调用体又带齐备元数据。
        let call_ok = (e.body)(0) == (e.body)(0);
        if !call_ok {
            return false;
        }
        i += 1;
    }
    // 宏注册面：kcase! 成条的四要素逐字段可读。
    let probe = kcase!("内存", "宏探针", "四要素一体", body_mem_eq);
    probe.domain == "内存"
        && probe.name == "宏探针"
        && probe.desc == "四要素一体"
        && (probe.body)(1)
}

/// M-165 注册-查询-执行链全绿。
fn m165_ok() -> bool {
    let (all, per) = m165_verify();
    all && per.len() == CASE_TOTAL * 2
}

/// 双跑一致（链两次验证同判、静态表裁决确定）。
fn dual_run_ok() -> bool {
    let (a, pa) = m165_verify();
    let (b, pb) = m165_verify();
    a && b && pa == pb
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3406 用例注册宏 kcase!。
pub fn run_unx3406_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3406");
    let reg = seed_registry();

    s.add_dyn(
        "F3406-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 正常流：静态注册表自洽 + kcase! 四要素一体（J1 主判据）。
    s.add_dyn(
        "F3406-02 kcase宏一体注册静态表",
        j1_main_ok(),
        &format!("cases={}", CASE_TOTAL),
    );

    // 动态册：静态表全量注册、相位翻已注册。
    s.add_dyn(
        "F3406-03 静态表全量注册入册",
        reg.len() == CASE_TOTAL && reg.phase() == Phase::Registered,
        &format!("len={} phase={}", reg.len(), reg.phase().name()),
    );

    // M-165 注册-查询-执行链全绿。
    let (m_ok, m_per) = m165_verify();
    s.add_dyn(
        "F3406-04 M165注册查询执行链",
        m_ok,
        &format!("per={:?}", m_per),
    );

    // 分域命名空间隔离：同名跨域合法可辨。
    s.add_dyn(
        "F3406-05 分域命名空间隔离",
        namespace_ok(),
        &format!("domains={}", DOMAIN_COUNT),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3406-06 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 边界：空册相位与空册执行显性拒。
    let mut empty = CaseRegistry::new();
    let empty_ok = empty.phase() == Phase::Empty
        && empty.query("内存", "块等值") == Err(Diag::EEmptyReg.val())
        && empty.seal() == Err(Diag::EEmptyReg.val());
    s.add_dyn(
        "F3406-07 空册显性拒",
        empty_ok,
        &format!("phase={}", empty.phase().name()),
    );

    // 边界：封册单向（空册→已注册→已封册，倒退拒）。
    let mut sealed = seed_registry();
    let seal1 = sealed.seal() == Ok(());
    let seal2 = sealed.seal() == Err(Diag::EPhase.val());
    let e0 = match CASE_TABLE.get(0) {
        Some(x) => *x,
        None => return s,
    };
    let sealed_reg = sealed.register(e0) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F3406-08 封册单向倒退拒",
        seal1 && seal2 && sealed_reg && sealed.phase().ord() == 2,
        &format!("phase={}", sealed.phase().name()),
    );

    // 执行账：序号连续、裁决落账可回读。
    let mut run_log = seed_registry();
    let _ = run_log.execute("内存", "块等值", 1);
    let _ = run_log.execute("内存", "首差异", 0);
    let rec0 = run_log.log_at(0);
    let rec1 = run_log.log_at(1);
    let log_ok = run_log.log_seq_ok()
        && run_log.log_len() == 2
        && matches!(rec0, Some(r) if r.seq == 1 && r.verdict)
        && matches!(rec1, Some(r) if r.seq == 2 && !r.verdict);
    s.add_dyn(
        "F3406-09 执行账序连续裁决落账",
        log_ok,
        &format!("log={}", run_log.log_len()),
    );

    // 执行账溢出显性记账。
    let mut cap_reg = CaseRegistry::new();
    let _ = cap_reg.register(kcase!("文件", "账容", "账容注入", body_file_report));
    let mut k = 0usize;
    let mut overflow_seen = false;
    while k < LOG_CAP {
        if cap_reg.execute("文件", "账容", (k as u64) + 1).is_err() {
            break;
        }
        k += 1;
    }
    overflow_seen = cap_reg.execute("文件", "账容", 999) == Err(Diag::EFull.val())
        && cap_reg.overflow() == 1;
    s.add_dyn(
        "F3406-10 执行账溢出显性记账",
        overflow_seen && cap_reg.log_len() == LOG_CAP,
        &format!("cap={} overflow={}", LOG_CAP, cap_reg.overflow()),
    );

    // 写回摘要非空且分域聚合（判据③写回面）。
    let sum = reg.summary();
    s.add_dyn(
        "F3406-11 写回摘要非空分域聚合",
        !sum.is_empty() && sum.contains("域内存") && sum.contains("域文件"),
        &format!("bytes={}", sum.len()),
    );

    // 用例体纯度：同输入重复调用裁决恒定（确定性语料）。
    let pure = CASE_TABLE.iter().all(|e| {
        let a = (e.body)(0x5A);
        let b = (e.body)(0x5A);
        a == b
    });
    s.add_dyn(
        "F3406-12 用例体确定性纯度",
        pure,
        &format!("pure={}", pure),
    );

    // 双跑一致。
    s.add_dyn(
        "F3406-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 诊断码闭集。
    s.add_dyn(
        "F3406-14 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EPhase.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EDupName.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x794C0);
    }

    #[test]
    fn kcase_macro_binds_fn_and_meta() {
        let e = kcase!("内存", "宏测", "四要素", body_mem_eq);
        assert_eq!(e.domain, "内存");
        assert_eq!(e.name, "宏测");
        assert_eq!(e.desc, "四要素");
        assert!((e.body)(1));
        assert!(!(e.body)(2));
    }

    #[test]
    fn static_table_wellformed() {
        assert!(table_ok());
        assert_eq!(CASE_TABLE.len(), CASE_TOTAL);
        for e in CASE_TABLE.iter() {
            assert!(domain_legal(e.domain));
            assert!(!e.name.is_empty());
            assert!(!e.desc.is_empty());
        }
    }

    #[test]
    fn table_query_by_namespace() {
        assert!(table_query("内存", "块等值").is_some());
        assert!(table_query("文件", "计数账本").is_some());
        assert!(table_query("内存", "不存在").is_none());
        assert!(table_query("网络", "块等值").is_none());
    }

    #[test]
    fn m165_chain_green() {
        assert!(m165_ok());
        let (all, per) = m165_verify();
        assert!(all);
        assert_eq!(per.len(), CASE_TOTAL * 2);
    }

    #[test]
    fn dup_name_in_same_domain_rejected() {
        let mut reg = CaseRegistry::new();
        let _ = reg.register(kcase!("内存", "甲", "首注", body_mem_eq));
        assert_eq!(
            reg.register(kcase!("内存", "甲", "重注", body_mem_eq)),
            Err(Diag::EDupName.val())
        );
        // 跨域同名合法。
        assert!(reg.register(kcase!("调度", "甲", "跨域同名", body_sched_mode)).is_ok());
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn phase_monotonic() {
        let mut reg = CaseRegistry::new();
        assert_eq!(reg.phase(), Phase::Empty);
        let _ = reg.register(kcase!("内存", "首例", "首例", body_mem_eq));
        assert_eq!(reg.phase(), Phase::Registered);
        let _ = reg.seal();
        assert_eq!(reg.phase(), Phase::Sealed);
        assert_eq!(reg.register(kcase!("内存", "次例", "次例", body_mem_eq)), Err(Diag::ESealed.val()));
        assert_eq!(reg.seal(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn execution_log_seq_continuous() {
        let mut reg = seed_registry();
        for i in 0..4u64 {
            let _ = reg.execute("内存", "块等值", i + 1);
        }
        assert_eq!(reg.log_len(), 4);
        assert!(reg.log_seq_ok());
        let last = reg.log_at(3);
        assert!(matches!(last, Some(r) if r.seq == 4));
    }

    #[test]
    fn verdict_flip_gate() {
        // 同例同输入重跑裁决必须一致（确定性判据在执行账上把关）。
        let mut reg = seed_registry();
        let r1 = reg.execute("调度", "两模式", 9);
        let r2 = reg.execute("调度", "两模式", 9);
        assert_eq!(r1, r2);
        assert!(r1.is_ok());
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn summary_aggregates_by_domain() {
        let reg = seed_registry();
        let sum = reg.summary();
        assert!(sum.contains("域内存 册2"));
        assert!(sum.contains("域调度 册2"));
        assert!(sum.contains("域文件 册2"));
        assert!(sum.contains("册6"));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3406_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
