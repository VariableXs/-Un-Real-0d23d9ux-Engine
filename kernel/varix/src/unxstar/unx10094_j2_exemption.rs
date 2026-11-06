//! UNX-F10094 双判据豁免语义（J2 豁免从严）· 0x92740 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10095-J1 字段扩展核（快照 J2 段追加 10/10 次），私改快照结构 10/10 次拦截
//!
//! SEG_BASE = 0x92740（本卡独占 0x40 窗口，下一卡 0x92780）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：ADR 四要素解析器（形制 `ADR|背景=..|决策=..|后果=..|备选=..`，
//!      四段缺一/空值/标错段名一律拒绝），ADR 是 J2 豁免的前置证据。
//!   ② 状态机：豁免请求生命周期 Pending→AdrAttached→Rechecked→Granted
//!      单向推进；J1 豁免零容忍直接拒绝，J2 豁免必挂 ADR+复核。
//!   ③ 账本：豁免账本 WaiverLedger，逐请求登记判别面（J1/J2）、ADR 四要素
//!      与复核位；授予前逐项核证据，轻豁免逐次显性拦截。
//!   ④ 注入核：轻豁免 / 缺 ADR 字段 / 未复核授予 / J1 豁免 四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10094 独占 0x40 窗口，下一卡 0x92780）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92740;

/// 注入复测轮数（判据要求 10/10；复核门重测轮数同源）。
pub const INJECT_ROUNDS: u32 = 10;

/// 豁免账本容量上限。
pub const WAIVER_CAP: usize = 48;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaiverCode {
    /// ADR 规格串段数不足或段名错
    AdrMalformed,
    /// ADR 段值空白
    EmptyAdrText,
    /// 缺 ADR 即申请授予
    LightExemption,
    /// ADR 四要素缺项
    MissingAdrField,
    /// 复核未做即授予
    MissingRecheck,
    /// J1 面申请豁免（零容忍）
    J1WaiverRefused,
    /// 同条目重复申请
    DupWaiver,
    /// 未知豁免请求
    UnknownRequest,
    /// 生命周期越态
    StageViolation,
    /// ADR 字段标签往返不一致
    FieldRoundTrip,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl WaiverCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            WaiverCode::AdrMalformed => "ADR 规格非被拒：形制必须 \
                ADR|背景=..|决策=..|后果=..|备选=..；请按四要素重写",
            WaiverCode::EmptyAdrText => "ADR 空段被拒：四要素逐段必须非空；\
                空段即缺件 ADR",
            WaiverCode::LightExemption => "轻豁免被拒：J2 豁免必须先挂 ADR 再申请授予；\
                无 ADR 的豁免一律拦截",
            WaiverCode::MissingAdrField => "ADR 缺件被拒：四要素 背景/决策/后果/备选 必须齐备；\
                请补齐缺段",
            WaiverCode::MissingRecheck => "未复核被拒：授予前必须完成一轮从严复核；\
                请先走复核门",
            WaiverCode::J1WaiverRefused => "J1 豁免被拒：正判据达成路径不得豁免；\
                未达成请修复而非免检",
            WaiverCode::DupWaiver => "重复申请被拒：同条目只能有一条在途豁免；\
                请先处置在途请求",
            WaiverCode::UnknownRequest => "未知豁免请求：账本中无此条目号；请先开立请求",
            WaiverCode::StageViolation => "生命周期越态：豁免态必须 \
                Pending→AdrAttached→Rechecked→Granted 单向推进",
            WaiverCode::FieldRoundTrip => "ADR 标签往返不一致：label 再解析必须回到原字段；\
                请检查闭集文案与解析器是否同步",
            WaiverCode::RejectedTrace => "拒绝留痕被拒：被拒申请不得改动任何已有请求；\
                请排查拒绝路径是否误提交",
            WaiverCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [WaiverCode; 12] = [
        WaiverCode::AdrMalformed,
        WaiverCode::EmptyAdrText,
        WaiverCode::LightExemption,
        WaiverCode::MissingAdrField,
        WaiverCode::MissingRecheck,
        WaiverCode::J1WaiverRefused,
        WaiverCode::DupWaiver,
        WaiverCode::UnknownRequest,
        WaiverCode::StageViolation,
        WaiverCode::FieldRoundTrip,
        WaiverCode::RejectedTrace,
        WaiverCode::SegmentViolation,
    ];
}

// ── 判别面闭集 ────────────────────────────────────────────────────────
/// 豁免判别面：J1 正判据 / J2 反判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaiverSide {
    /// 正判据面（禁豁免）
    J1,
    /// 反判据面（从严豁免）
    J2,
}

impl WaiverSide {
    /// 中文面名。
    pub fn label(self) -> &'static str {
        match self {
            WaiverSide::J1 => "正判据(J1)",
            WaiverSide::J2 => "反判据(J2)",
        }
    }

    /// 闭集全量。
    pub const ALL: [WaiverSide; 2] = [WaiverSide::J1, WaiverSide::J2];
}

// ── ADR 四要素解析器 ──────────────────────────────────────────────────
/// ADR 四要素标签闭集（固定序）。
pub const ADR_FIELDS: [&str; 4] = ["背景", "决策", "后果", "备选"];

/// 解析后的 ADR：四段非空文本。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AdrDoc {
    /// 背景段
    pub context: String,
    /// 决策段
    pub decision: String,
    /// 后果段
    pub consequence: String,
    /// 备选段
    pub alternative: String,
}

impl AdrDoc {
    /// 四要素逐段非空核（供账本证据核）。
    pub fn complete(&self) -> bool {
        !self.context.is_empty() && !self.decision.is_empty()
            && !self.consequence.is_empty() && !self.alternative.is_empty()
    }

    /// 四要素齐备成文（指纹语料用）。
    pub fn brief(&self) -> String {
        format!("ADR|背景={}|决策={}|后果={}|备选={}",
            self.context, self.decision, self.consequence, self.alternative)
    }
}

/// ADR 解析：形制 `ADR|背景=<..>|决策=<..>|后果=<..>|备选=<..>`。
/// 段数不足 / 段名错 / 空段值逐项拒绝；段名按 ADR_FIELDS 固定序核对。
pub fn parse_adr(text: &str) -> Result<AdrDoc, WaiverCode> {
    if !text.starts_with("ADR") {
        return Err(WaiverCode::AdrMalformed);
    }
    let body = match text.get(3..) {
        Some(b) if b.starts_with('|') => match b.get(1..) {
            Some(rest) => rest,
            None => return Err(WaiverCode::AdrMalformed),
        },
        _ => return Err(WaiverCode::AdrMalformed),
    };
    // 手工按 '|' 切四段（零 panic）。
    let mut parts: [&str; 4] = ["", "", "", ""];
    let mut pi = 0usize;
    let mut cur = body;
    loop {
        if cur.is_empty() && pi == 4 {
            break;
        }
        match cur.find('|') {
            Some(p) => {
                if pi >= 4 {
                    return Err(WaiverCode::AdrMalformed);
                }
                parts[pi] = match cur.get(..p) {
                    Some(h) => h,
                    None => return Err(WaiverCode::AdrMalformed),
                };
                pi += 1;
                cur = match cur.get(p + 1..) {
                    Some(r) => r,
                    None => return Err(WaiverCode::AdrMalformed),
                };
            }
            None => {
                if pi >= 4 {
                    return Err(WaiverCode::AdrMalformed);
                }
                parts[pi] = cur;
                pi += 1;
                if pi < 4 {
                    return Err(WaiverCode::MissingAdrField);
                }
                break;
            }
        }
    }
    let mut vals: [Option<&str>; 4] = [None, None, None, None];
    let mut fi = 0usize;
    while fi < 4 {
        let seg = match parts.get(fi) {
            Some(s) => *s,
            None => return Err(WaiverCode::MissingAdrField),
        };
        let tag = match ADR_FIELDS.get(fi) {
            Some(t) => *t,
            None => return Err(WaiverCode::MissingAdrField),
        };
        let prefix_len = tag.len() + 1; // "标签="
        let got = match seg.get(..prefix_len) {
            Some(g) => g,
            None => return Err(WaiverCode::AdrMalformed),
        };
        if got != format!("{}=", tag) {
            return Err(WaiverCode::AdrMalformed);
        }
        let val = match seg.get(prefix_len..) {
            Some(v) => v,
            None => return Err(WaiverCode::AdrMalformed),
        };
        if val.is_empty() {
            return Err(WaiverCode::EmptyAdrText);
        }
        vals[fi] = Some(val);
        fi += 1;
    }
    Ok(AdrDoc {
        context: String::from(vals[0].unwrap_or("")),
        decision: String::from(vals[1].unwrap_or("")),
        consequence: String::from(vals[2].unwrap_or("")),
        alternative: String::from(vals[3].unwrap_or("")),
    })
}

// ── 豁免生命周期状态机 ────────────────────────────────────────────────
/// 请求态：Pending → AdrAttached → Rechecked → Granted。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaiverStage {
    /// 待挂 ADR
    Pending,
    /// ADR 已挂
    AdrAttached,
    /// 复核已过
    Rechecked,
    /// 已授予
    Granted,
}

impl WaiverStage {
    /// 态序（0..=3）。
    pub fn rank(self) -> u8 {
        match self {
            WaiverStage::Pending => 0,
            WaiverStage::AdrAttached => 1,
            WaiverStage::Rechecked => 2,
            WaiverStage::Granted => 3,
        }
    }

    /// 中文态名。
    pub fn label(self) -> &'static str {
        match self {
            WaiverStage::Pending => "待挂ADR",
            WaiverStage::AdrAttached => "ADR已挂",
            WaiverStage::Rechecked => "复核已过",
            WaiverStage::Granted => "已授予",
        }
    }
}

// ── 豁免账本 ──────────────────────────────────────────────────────────
/// 豁免请求条目。
#[derive(Clone, Debug)]
pub struct WaiverRequest {
    /// 条目号
    pub id: u32,
    /// 判别面
    pub side: WaiverSide,
    /// 生命周期态
    pub stage: WaiverStage,
    /// 已挂 ADR
    pub adr: Option<AdrDoc>,
}

/// 豁免账本：J2 从严三道门（挂 ADR / 复核 / 授予），J1 零容忍。
#[derive(Clone, Debug)]
pub struct WaiverLedger {
    reqs: Vec<WaiverRequest>,
    cap: usize,
}

impl WaiverLedger {
    /// 空账构造（容量 WAIVER_CAP）。
    pub fn new() -> WaiverLedger {
        WaiverLedger { reqs: Vec::new(), cap: WAIVER_CAP }
    }

    /// 开立请求：同号重复拒绝；J1 面立即拒绝（零容忍）。
    pub fn open(&mut self, id: u32, side: WaiverSide) -> Result<WaiverStage, WaiverCode> {
        if side == WaiverSide::J1 {
            return Err(WaiverCode::J1WaiverRefused);
        }
        if self.reqs.len() >= self.cap {
            return Err(WaiverCode::RejectedTrace);
        }
        let mut i = 0usize;
        while i < self.reqs.len() {
            if let Some(r) = self.reqs.get(i) {
                if r.id == id {
                    return Err(WaiverCode::DupWaiver);
                }
            }
            i += 1;
        }
        self.reqs.push(WaiverRequest { id, side, stage: WaiverStage::Pending, adr: None });
        Ok(WaiverStage::Pending)
    }

    /// 请求数。
    pub fn len(&self) -> usize {
        self.reqs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.reqs.is_empty()
    }

    /// 挂 ADR：只有 Pending 态可挂；解析必须四要素齐备。
    pub fn attach_adr(&mut self, id: u32, text: &str) -> Result<WaiverStage, WaiverCode> {
        let doc = parse_adr(text)?;
        let mut i = 0usize;
        while i < self.reqs.len() {
            if let Some(r) = self.reqs.get_mut(i) {
                if r.id == id {
                    if r.stage != WaiverStage::Pending {
                        return Err(WaiverCode::StageViolation);
                    }
                    r.adr = Some(doc);
                    r.stage = WaiverStage::AdrAttached;
                    return Ok(r.stage);
                }
            }
            i += 1;
        }
        Err(WaiverCode::UnknownRequest)
    }

    /// 复核：只有 AdrAttached 态可复核（ADR 先挂从严）。
    pub fn recheck(&mut self, id: u32) -> Result<WaiverStage, WaiverCode> {
        let mut i = 0usize;
        while i < self.reqs.len() {
            if let Some(r) = self.reqs.get_mut(i) {
                if r.id == id {
                    match r.stage {
                        WaiverStage::AdrAttached => {
                            r.stage = WaiverStage::Rechecked;
                            return Ok(r.stage);
                        }
                        WaiverStage::Pending => return Err(WaiverCode::LightExemption),
                        _ => return Err(WaiverCode::StageViolation),
                    }
                }
            }
            i += 1;
        }
        Err(WaiverCode::UnknownRequest)
    }

    /// 授予：必须 Rechecked 态且 ADR 四要素齐备；否则逐因拒绝。
    pub fn grant(&mut self, id: u32) -> Result<WaiverStage, WaiverCode> {
        let mut i = 0usize;
        while i < self.reqs.len() {
            if let Some(r) = self.reqs.get_mut(i) {
                if r.id == id {
                    match r.stage {
                        WaiverStage::Pending => return Err(WaiverCode::LightExemption),
                        WaiverStage::AdrAttached => return Err(WaiverCode::MissingRecheck),
                        WaiverStage::Granted => return Err(WaiverCode::StageViolation),
                        WaiverStage::Rechecked => {
                            let doc_ok = match &r.adr {
                                Some(d) => d.complete(),
                                None => false,
                            };
                            if !doc_ok {
                                return Err(WaiverCode::MissingAdrField);
                            }
                            r.stage = WaiverStage::Granted;
                            return Ok(r.stage);
                        }
                    }
                }
            }
            i += 1;
        }
        Err(WaiverCode::UnknownRequest)
    }

    /// 已授予数。
    pub fn granted_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.reqs.len() {
            if let Some(r) = self.reqs.get(i) {
                if r.stage == WaiverStage::Granted {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 全账审计：所有在途请求必须 J2 面且 ADR 已挂（轻豁免 = 0 条）。
    pub fn strict_compliant(&self) -> bool {
        let mut i = 0usize;
        while i < self.reqs.len() {
            match self.reqs.get(i) {
                None => return false,
                Some(r) => {
                    if r.side != WaiverSide::J2 {
                        return false;
                    }
                    let doc_ok = match &r.adr {
                        Some(d) => d.complete(),
                        None => false,
                    };
                    if !doc_ok {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }
}

// ── 注入核：坏豁免样本矩阵 ────────────────────────────────────────────
/// 四类坏豁免样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadWaiver {
    /// 轻豁免：未挂 ADR 直接授予
    LightExemption,
    /// 缺 ADR 字段
    MissingAdrField,
    /// 未复核授予
    MissingRecheck,
    /// J1 面豁免
    J1WaiverRefused,
}

impl BadWaiver {
    /// 闭集全量。
    pub const ALL: [BadWaiver; 4] = [
        BadWaiver::LightExemption,
        BadWaiver::MissingAdrField,
        BadWaiver::MissingRecheck,
        BadWaiver::J1WaiverRefused,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadWaiver::LightExemption => "轻豁免（无 ADR 授予）",
            BadWaiver::MissingAdrField => "ADR 缺件",
            BadWaiver::MissingRecheck => "未复核授予",
            BadWaiver::J1WaiverRefused => "J1 面豁免",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), WaiverCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(WaiverCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏豁免注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_waiver_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadWaiver::ALL.len() {
        let case = match BadWaiver::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadWaiver::LightExemption => {
                    let mut l = WaiverLedger::new();
                    let ok1 = l.open(701, WaiverSide::J2).is_ok();
                    ok1 && l.grant(701).err() == Some(WaiverCode::LightExemption)
                }
                BadWaiver::MissingAdrField => {
                    let mut l = WaiverLedger::new();
                    let ok1 = l.open(702, WaiverSide::J2).is_ok();
                    ok1 && l.attach_adr(702, "ADR|背景=b|决策=d|后果=|备选=a")
                        .err() == Some(WaiverCode::EmptyAdrText)
                }
                BadWaiver::MissingRecheck => {
                    let mut l = WaiverLedger::new();
                    let ok1 = l.open(703, WaiverSide::J2).is_ok();
                    let ok2 = l.attach_adr(703,
                        "ADR|背景=fd 只读|决策=预期 EBADF|后果=写失败受控|备选=改写模式")
                        .is_ok();
                    ok1 && ok2
                        && l.grant(703).err() == Some(WaiverCode::MissingRecheck)
                }
                BadWaiver::J1WaiverRefused => {
                    let mut l = WaiverLedger::new();
                    l.open(704, WaiverSide::J1).err() == Some(WaiverCode::J1WaiverRefused)
                        && l.is_empty()
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 豁免成文（纯函数） ────────────────────────────────────────────────
/// 豁免成文：「E<n>(<面>)=<态>;ADR=<齐|缺>」；账外先拒。
pub fn compose_waiver_summary(l: &WaiverLedger, id: u32) -> Result<String, WaiverCode> {
    if l.is_empty() {
        return Err(WaiverCode::UnknownRequest);
    }
    let mut i = 0usize;
    while i < l.reqs.len() {
        if let Some(r) = l.reqs.get(i) {
            if r.id == id {
                let adr = if match &r.adr { Some(d) => d.complete(), None => false } {
                    "齐"
                } else {
                    "缺"
                };
                let out = format!("E{}({})={};ADR={}", r.id, r.side.label(), r.stage.label(), adr);
                if out.is_empty() {
                    return Err(WaiverCode::UnknownRequest);
                }
                return Ok(out);
            }
        }
        i += 1;
    }
    Err(WaiverCode::UnknownRequest)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；J1 豁免/越态操作族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；ADR 段名错族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；ADR 缺件/空段族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；轻豁免/未复核授予族"),
    ("EALREADY", -114, 190, "Windows ERROR_ALREADY_ISSUED（近似）；重复申请族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：两条 J2 请求走全从严三道门 → ADR brief 并文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = WaiverLedger::new();
    let _ = l.open(801, WaiverSide::J2);
    let _ = l.open(802, WaiverSide::J2);
    let a1 = "ADR|背景=J2 红未决|决策=挂预期差|后果=豁免一年|备选=修复后撤豁免";
    let a2 = "ADR|背景=J2 红复现|决策=限面豁免|后果=不扩面|备选=下期修复";
    let _ = l.attach_adr(801, a1);
    let _ = l.attach_adr(802, a2);
    let _ = l.recheck(801);
    let _ = l.grant(801);
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 2 {
        match compose_waiver_summary(&l, 801 + e) {
            Ok(s) => {
                bytes.extend_from_slice(s.as_bytes());
                bytes.push(b';');
            }
            Err(_) => {}
        }
        e += 1;
    }
    bytes.extend_from_slice(a1.as_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10094 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10094_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10094");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92740..=0x9277F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. ADR 解析器：四要素齐全全通过
    let parse_ok = match parse_adr("ADR|背景=红未决|决策=挂账|后果=限期|备选=修复") {
        Ok(d) => d.complete() && d.decision == "挂账",
        Err(_) => false,
    };
    s.add("ADR 解析器：四要素齐全全解析", parse_ok,
          "ADR 是 J2 豁免的前置证据，四要素缺一即缺件");

    // 3. ADR 缺件注入：缺段/空段/段名错/非 ADR 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_adr("ADR|背景=b|决策=d|后果=c").is_ok()
            || parse_adr("ADR|背景=b|决策=d|后果=|备选=a").is_ok()
            || parse_adr("ADR|背景=b|决策=d|错段=c|备选=a").is_ok()
            || parse_adr("ADR").is_ok()
            || parse_adr("XX|背景=b|决策=d|后果=c|备选=a").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("ADR 缺件注入：缺段/空段/段名错 10/10 拒绝", amb_ok,
          "缺件 ADR 必须逐次显性拒绝，空段与错段名同罪");

    // 4. J1 零容忍：J1 面开豁免立即拒绝且不入账
    let mut l = WaiverLedger::new();
    let j1_ok = l.open(1, WaiverSide::J1).err() == Some(WaiverCode::J1WaiverRefused)
        && l.is_empty();
    s.add("J1 零容忍：正判据面豁免直接拒绝", j1_ok,
          "J1 是达成路径，未达成只能修复；豁免零容忍");

    // 5. 从严三道门全流程：挂 ADR → 复核 → 授予
    let mut flow_ok = l.open(21, WaiverSide::J2).is_ok();
    flow_ok = flow_ok
        && l.attach_adr(21, "ADR|背景=J2 红未决|决策=挂预期差|后果=限期一年|备选=修复撤免")
            .is_ok()
        && l.recheck(21).is_ok()
        && l.grant(21) == Ok(WaiverStage::Granted)
        && l.granted_count() == 1;
    s.add("从严三道门：挂 ADR→复核→授予逐门放行", flow_ok,
          "J2 豁免三道门缺一不可，逐门留痕");

    // 6. 注入核：四类坏豁免样本各 10/10 拒绝
    let (inj, per_class) = run_bad_waiver_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏豁免样本各 10/10 拒绝", all_caught,
          "轻豁免/缺 ADR 件/未复核授予/J1 豁免必须逐类逐次拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadWaiver::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 轻豁免拦截：Pending 直接授予被拒
    let mut l2 = WaiverLedger::new();
    let _ = l2.open(31, WaiverSide::J2);
    let light_ok = l2.grant(31).err() == Some(WaiverCode::LightExemption)
        && l2.granted_count() == 0;
    s.add("轻豁免拦截：无 ADR 授予被拒且授予数不变", light_ok,
          "无 ADR 的 J2 豁免即轻豁免，逐次拦截");

    // 9. 未复核拦截：挂 ADR 后跳过复核授予被拒
    let mut l3 = WaiverLedger::new();
    let _ = l3.open(41, WaiverSide::J2);
    let _ = l3.attach_adr(41, "ADR|背景=断端写|决策=预期 EPIPE|后果=受控复现|备选=重连");
    let recheck_ok = l3.grant(41).err() == Some(WaiverCode::MissingRecheck);
    s.add("未复核拦截：ADR 已挂但复核未走授予被拒", recheck_ok,
          "复核门是从严豁免的第二次把关，跳门即拒");

    // 10. 生命周期越态：已授予再复核拒绝
    let mut l4 = WaiverLedger::new();
    let _ = l4.open(51, WaiverSide::J2);
    let _ = l4.attach_adr(51, "ADR|背景=坏缓冲|决策=预期 EFAULT|后果=受控复现|备选=修指针");
    let _ = l4.recheck(51);
    let _ = l4.grant(51);
    let stage_ok = l4.recheck(51).is_err() && l4.grant(51).is_err();
    s.add("生命周期越态：终态后再操作显性拒绝", stage_ok,
          "Granted 是终态，再复核/再授予均越态");

    // 11. 全账审计：在途请求全数 J2 且 ADR 齐
    let audit_ok = l.strict_compliant() && {
        let mut l5 = WaiverLedger::new();
        let _ = l5.open(61, WaiverSide::J2);
        !l5.strict_compliant()
    };
    s.add("全账审计：在途请求 J2 面且 ADR 齐，缺口暴露", audit_ok,
          "轻豁免 = 0 条是收口前置，审计逐请求核对");

    // 12. 成文非空且含面与态 + 双跑一致
    let sum = compose_waiver_summary(&l, 21);
    let text_ok = match &sum {
        Ok(t) => t.contains("反判据(J2)") && t.contains("已授予") && t.contains("ADR=齐"),
        Err(_) => false,
    };
    s.add("豁免成文：面/态/ADR 齐备且两次成文逐字节一致",
          text_ok && sum == compose_waiver_summary(&l, 21),
          "成文必须携带 面/态/ADR 标记且为纯函数");

    // 13. 账外成文拒绝
    s.add("账外成文拒绝：未知请求不得产出小结",
          compose_waiver_summary(&l, 999).err() == Some(WaiverCode::UnknownRequest)
              && compose_waiver_summary(&WaiverLedger::new(), 1)
                  .err() == Some(WaiverCode::UnknownRequest),
          "空账与账外请求都无对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(2)
        && agrees_with_linux(13) && agrees_with_linux(114)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < WaiverCode::ALL.len() {
        let mut j = i + 1;
        while j < WaiverCode::ALL.len() {
            if let (Some(a), Some(b)) = (WaiverCode::ALL.get(i), WaiverCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = WaiverCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. ADR 标签往返 + 双跑指纹
    let rt_ok = ADR_FIELDS.iter().all(|t| !t.is_empty())
        && parse_adr("ADR|背景=x|决策=y|后果=z|备选=w").map(|d| d.brief())
            == Ok(String::from("ADR|背景=x|决策=y|后果=z|备选=w"));
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：ADR 四要素可回文且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("豁免小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adr_parser_complete() {
        let d = parse_adr("ADR|背景=a|决策=b|后果=c|备选=d").unwrap_or(AdrDoc {
            context: String::new(),
            decision: String::new(),
            consequence: String::new(),
            alternative: String::new(),
        });
        assert!(d.complete());
        assert_eq!(d.brief(), "ADR|背景=a|决策=b|后果=c|备选=d");
    }

    #[test]
    fn adr_parser_rejects() {
        assert_eq!(parse_adr("ADR|背景=a|决策=b").err(), Some(WaiverCode::MissingAdrField));
        assert_eq!(parse_adr("ADR|背景=|决策=b|后果=c|备选=d").err(), Some(WaiverCode::EmptyAdrText));
        assert_eq!(parse_adr("ADR|X=a|决策=b|后果=c|备选=d").err(), Some(WaiverCode::AdrMalformed));
        assert_eq!(parse_adr("nope").err(), Some(WaiverCode::AdrMalformed));
    }

    #[test]
    fn j1_never_waivable() {
        let mut l = WaiverLedger::new();
        assert_eq!(l.open(1, WaiverSide::J1).err(), Some(WaiverCode::J1WaiverRefused));
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn strict_gates_in_order() {
        let mut l = WaiverLedger::new();
        let _ = l.open(2, WaiverSide::J2);
        assert_eq!(l.grant(2).err(), Some(WaiverCode::LightExemption));
        let _ = l.attach_adr(2, "ADR|背景=a|决策=b|后果=c|备选=d");
        assert_eq!(l.grant(2).err(), Some(WaiverCode::MissingRecheck));
        let _ = l.recheck(2);
        assert_eq!(l.grant(2), Ok(WaiverStage::Granted));
        assert!(l.grant(2).is_err());
    }

    #[test]
    fn dup_waiver_rejected() {
        let mut l = WaiverLedger::new();
        let _ = l.open(3, WaiverSide::J2);
        assert_eq!(l.open(3, WaiverSide::J2).err(), Some(WaiverCode::DupWaiver));
    }

    #[test]
    fn recheck_needs_adr_first() {
        let mut l = WaiverLedger::new();
        let _ = l.open(4, WaiverSide::J2);
        assert_eq!(l.recheck(4).err(), Some(WaiverCode::LightExemption));
    }

    #[test]
    fn bad_waiver_matrix_green() {
        let (inj, per) = run_bad_waiver_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn audit_exposes_pending_without_adr() {
        let mut l = WaiverLedger::new();
        let _ = l.open(5, WaiverSide::J2);
        assert!(!l.strict_compliant());
        let _ = l.attach_adr(5, "ADR|背景=a|决策=b|后果=c|备选=d");
        assert!(l.strict_compliant());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(114));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(13), -13);
    }

    #[test]
    fn unx10094_checks_green() {
        let set = run_unx10094_checks();
        assert!(set.all_passed(), "UNX-F10094 red: {}", set.red_items_text());
    }
}
