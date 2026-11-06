//! UNX-F10078 M 型段移交 E 型面（交接判据）· 0x92380 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10079-J1 交接四面齐备（口径基座/机制清单/闸位清单/待办清单），缺面移交 10/10 次拦截
//!
//! 设计要点：
//!   ① 交接四面闭集：口径基座 / 机制清单 / 闸位清单 / 待办清单；每面必须
//!      带非空条目集（≥1 条且逐条非空），缺任何一面 → MissingFace，
//!      空面 → EmptyFaceItems，缺面移交 10/10 次拦截。
//!   ② 移交清单是状态机：组面（set_face）→ 验收（accept）两相；重复组面
//!      DupFace、未知面 UnknownFace、空条目 EmptyItem 显性拒绝，
//!      拒绝不留痕；验收通过后清单指纹冻结（FNV-1a）。
//!   ③ 交接账守恒式：条目总数 = 四面条目之和，恒成立；四面齐备才可
//!      验收，验收产出移交凭据（指纹+条目数）供 E 型面接收复核。
//!   ④ 注入核：缺面移交 / 空面移交 / 未知面组面 / 重复组面四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10078 独占 0x40 窗口，下一卡 0x923C0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92380;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 单面条目容量上限。
pub const FACE_ITEM_CAP: usize = 32;

/// 交接四面总数。
pub const HANDOVER_FACE_COUNT: usize = 4;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverCode {
    /// 未知面对照 token
    UnknownFace,
    /// 空面对照 token
    EmptyFace,
    /// 面条目为空（0 条或全空串）
    EmptyFaceItems,
    /// 同面重复组面
    DupFace,
    /// 单条空条目
    EmptyItem,
    /// 面条目容量满
    ItemsFull,
    /// 交接账失守恒
    HandoverRegress,
    /// 缺面移交（四面未齐即验收）
    MissingFace,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 面标签往返不一致
    FaceRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl HandoverCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            HandoverCode::UnknownFace => "未知面被拒：四面只认 caliber|mech|gate|todo；\
                请改用闭集内对照",
            HandoverCode::EmptyFace => "空面对照被拒：面 token 不能为空串；请先确认四面表",
            HandoverCode::EmptyFaceItems => "空面被拒：每面必须 ≥1 条非空条目；\
                空面移交等于失面，逐次拦截",
            HandoverCode::DupFace => "重复组面被拒：同一面只能组一次；\
                改条目请走更正留痕而非二次组面",
            HandoverCode::EmptyItem => "空条目被拒：交接条目不能为空串；\
                请补全条目内容后再组面",
            HandoverCode::ItemsFull => "容量满被拒：面条目已达上限；请先归档旧条目再组面",
            HandoverCode::HandoverRegress => "交接账失守恒：条目总数必须等于四面之和；\
                请排查是否绕过条目计数器",
            HandoverCode::MissingFace => "缺面移交被拒：四面齐备才可验收；\
                口径基座/机制/闸位/待办缺一即拦（10/10）",
            HandoverCode::EmptySummary => "空成文被拒：移交小结不得为空串；请检查清单是否为空",
            HandoverCode::RejectedTrace => "拒绝留痕被拒：被拒组面不得改动清单；\
                请排查拒绝路径是否误提交",
            HandoverCode::FaceRoundTrip => "面往返不一致：label 再对照必须回到原面；\
                请检查闭集文案与对照器是否同步",
            HandoverCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [HandoverCode; 12] = [
        HandoverCode::UnknownFace,
        HandoverCode::EmptyFace,
        HandoverCode::EmptyFaceItems,
        HandoverCode::DupFace,
        HandoverCode::EmptyItem,
        HandoverCode::ItemsFull,
        HandoverCode::HandoverRegress,
        HandoverCode::MissingFace,
        HandoverCode::EmptySummary,
        HandoverCode::RejectedTrace,
        HandoverCode::FaceRoundTrip,
        HandoverCode::SegmentViolation,
    ];
}

// ── 交接四面闭集与解析器 ──────────────────────────────────────────────
/// 交接四面闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverFace {
    /// 口径基座（对账口径与守恒式清单）
    CaliberBase,
    /// 机制清单（已固化机制与出处）
    MechList,
    /// 闸位清单（挡板位与判定条件）
    GateList,
    /// 待办清单（移交后待办事项）
    TodoList,
}

impl HandoverFace {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            HandoverFace::CaliberBase => "口径基座",
            HandoverFace::MechList => "机制清单",
            HandoverFace::GateList => "闸位清单",
            HandoverFace::TodoList => "待办清单",
        }
    }

    /// 面序（四面固定序）。
    pub fn index(self) -> usize {
        match self {
            HandoverFace::CaliberBase => 0,
            HandoverFace::MechList => 1,
            HandoverFace::GateList => 2,
            HandoverFace::TodoList => 3,
        }
    }

    /// 闭集全量。
    pub const ALL: [HandoverFace; 4] = [
        HandoverFace::CaliberBase,
        HandoverFace::MechList,
        HandoverFace::GateList,
        HandoverFace::TodoList,
    ];
}

/// 面对照解析：英文 token 对照表，其余一律 UnknownFace，空串 EmptyFace。
pub fn parse_handover_face(tag: &str) -> Result<HandoverFace, HandoverCode> {
    if tag.is_empty() {
        return Err(HandoverCode::EmptyFace);
    }
    match tag {
        "caliber" => Ok(HandoverFace::CaliberBase),
        "mech" => Ok(HandoverFace::MechList),
        "gate" => Ok(HandoverFace::GateList),
        "todo" => Ok(HandoverFace::TodoList),
        _ => Err(HandoverCode::UnknownFace),
    }
}

// ── 移交清单（两相状态机 + 账本） ─────────────────────────────────────
/// 面条目集：面 + 非空条目列表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FaceItems {
    /// 面
    pub face: HandoverFace,
    /// 条目列表（逐条非空）
    pub items: Vec<String>,
}

impl FaceItems {
    /// 条目数。
    pub fn count(&self) -> usize {
        self.items.len()
    }
}

/// 移交清单：组面（set_face）→ 验收（accept）两相状态机。
#[derive(Clone, Debug)]
pub struct HandoverManifest {
    faces: [Option<FaceItems>; HANDOVER_FACE_COUNT],
    accepted: bool,
}

impl HandoverManifest {
    /// 空清单：四面全缺、未验收。
    /// 注：FaceItems 含 Vec<String> 不可 Copy，不能用 [None; N] 重复写法。
    pub fn new() -> HandoverManifest {
        HandoverManifest { faces: [None, None, None, None], accepted: false }
    }

    /// 组一面：逐条目非空校验（空条目 EmptyItem）、面满 ItemsFull、
    /// 重复组面 DupFace；任何拒绝路径不留痕。
    pub fn set_face(&mut self, face: HandoverFace, items: &[&str]) -> Result<usize, HandoverCode> {
        let idx = face.index();
        if idx >= HANDOVER_FACE_COUNT {
            return Err(HandoverCode::UnknownFace);
        }
        if self.faces[idx].is_some() {
            return Err(HandoverCode::DupFace);
        }
        if items.is_empty() {
            return Err(HandoverCode::EmptyFaceItems);
        }
        let mut stored: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < items.len() {
            let it = match items.get(i) {
                Some(v) => *v,
                None => break,
            };
            if it.is_empty() {
                return Err(HandoverCode::EmptyItem);
            }
            if stored.len() >= FACE_ITEM_CAP {
                return Err(HandoverCode::ItemsFull);
            }
            stored.push(String::from(it));
            i += 1;
        }
        self.faces[idx] = Some(FaceItems { face, items: stored });
        match self.faces.get(idx) {
            Some(Some(f)) => Ok(f.count()),
            _ => Err(HandoverCode::RejectedTrace),
        }
    }

    /// 查面条目集。
    pub fn face(&self, f: HandoverFace) -> Option<FaceItems> {
        match self.faces.get(f.index()) {
            Some(Some(v)) => Some(v.clone()),
            _ => None,
        }
    }

    /// 缺面清单（供诊断）。
    pub fn missing(&self) -> Vec<HandoverFace> {
        let mut out: Vec<HandoverFace> = Vec::new();
        let mut i = 0usize;
        while i < HANDOVER_FACE_COUNT {
            if self.faces[i].is_none() {
                if let Some(f) = HandoverFace::ALL.get(i) {
                    out.push(*f);
                }
            }
            i += 1;
        }
        out
    }

    /// 四面是否齐备。
    pub fn complete(&self) -> bool {
        let mut i = 0usize;
        while i < HANDOVER_FACE_COUNT {
            match self.faces.get(i) {
                Some(Some(f)) => {
                    if f.count() == 0 {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 条目总数（守恒账分子）。
    pub fn total_items(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < HANDOVER_FACE_COUNT {
            if let Some(Some(f)) = self.faces.get(i) {
                n += f.count();
            }
            i += 1;
        }
        n
    }

    /// 验收：四面齐备才通过；通过即冻结（accepted 置位，指纹随后可取）；
    /// 缺面 MissingFace 逐次拦截。验收后再组面按重复/私改路径显性拒绝。
    pub fn accept(&mut self) -> Result<u32, HandoverCode> {
        if self.accepted {
            return Err(HandoverCode::RejectedTrace);
        }
        if !self.complete() {
            return Err(HandoverCode::MissingFace);
        }
        self.accepted = true;
        Ok(self.fingerprint())
    }

    /// 是否已验收。
    pub fn is_accepted(&self) -> bool {
        self.accepted
    }

    /// 清单指纹：四面标签+条目数+逐条内容确定性推导（FNV-1a）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < HANDOVER_FACE_COUNT {
            match self.faces.get(i) {
                Some(Some(f)) => {
                    h ^= f.face.label().as_bytes().first().copied().unwrap_or(0) as u32;
                    h = h.wrapping_mul(0x0100_0193);
                    h ^= (f.count() as u32).wrapping_mul(0x9E37_79B9);
                    h = h.wrapping_mul(0x0100_0193);
                    let mut k = 0usize;
                    while k < f.items.len() {
                        if let Some(t) = f.items.get(k) {
                            let fb = fnv1a(t.as_bytes());
                            h ^= fb;
                            h = h.wrapping_mul(0x0100_0193);
                        }
                        k += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        h
    }
}

// ── 注入核：坏移交样本矩阵 ────────────────────────────────────────────
/// 四类坏移交样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadHandover {
    /// 缺面移交（三面即验收）
    MissingFace,
    /// 空面移交（0 条条目）
    EmptyFace,
    /// 未知面组面
    UnknownFaceWrite,
    /// 重复组面
    DupFaceWrite,
}

impl BadHandover {
    /// 闭集全量。
    pub const ALL: [BadHandover; 4] = [
        BadHandover::MissingFace,
        BadHandover::EmptyFace,
        BadHandover::UnknownFaceWrite,
        BadHandover::DupFaceWrite,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadHandover::MissingFace => "缺面移交",
            BadHandover::EmptyFace => "空面移交",
            BadHandover::UnknownFaceWrite => "未知面组面",
            BadHandover::DupFaceWrite => "重复组面",
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

    /// 记一次注入结果；账失守恒返回 HandoverRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), HandoverCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(HandoverCode::HandoverRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏移交注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新清单，确保被拒样本不留痕。
pub fn run_bad_handover_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadHandover::ALL.len() {
        let case = match BadHandover::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadHandover::MissingFace => {
                    // 只组三面（缺待办清单）即验收 → MissingFace
                    let mut m = manifest_three_faces();
                    m.accept().err() == Some(HandoverCode::MissingFace)
                }
                BadHandover::EmptyFace => {
                    let mut m = HandoverManifest::new();
                    m.set_face(HandoverFace::CaliberBase, &[]).err()
                        == Some(HandoverCode::EmptyFaceItems)
                }
                BadHandover::UnknownFaceWrite => {
                    parse_handover_face("secret").is_err()
                        && parse_handover_face("").err() == Some(HandoverCode::EmptyFace)
                }
                BadHandover::DupFaceWrite => {
                    let mut m = manifest_three_faces();
                    // 补第四面成功后再组同面 → DupFace
                    m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A])
                        .is_ok()
                        && m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_B]).err()
                            == Some(HandoverCode::DupFace)
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

/// 组面语料（待办条目）。
const HANDOVER_TODO_A: &str = "HANDOVER_TODO_A：E 型面五战线预排核";
const HANDOVER_TODO_B: &str = "HANDOVER_TODO_B：E 型双判据样例条补齐";

/// 三面清单（缺待办清单）——注入与判据共用的半成品。
pub fn manifest_three_faces() -> HandoverManifest {
    let mut m = HandoverManifest::new();
    let _ = m.set_face(HandoverFace::CaliberBase, &["守恒式：批行数=预算行数", "SEG 窗口 0x40 对齐"]);
    let _ = m.set_face(HandoverFace::MechList, &["机制一：红账分型出处冻结", "机制二：双档覆盖口径冻结"]);
    let _ = m.set_face(HandoverFace::GateList, &["闸位一：差异未决闸保持红", "闸位二：归档冻结核"]);
    m
}

// ── 移交成文（纯函数） ────────────────────────────────────────────────
/// 移交成文：「面数=<n>;条目=<n>;指纹=<hex>」；缺面先拒。
pub fn compose_handover_summary(m: &HandoverManifest, fp: u32) -> Result<String, HandoverCode> {
    if !m.complete() {
        return Err(HandoverCode::MissingFace);
    }
    let out = format!(
        "面数={};条目={};指纹={:08X}",
        HANDOVER_FACE_COUNT,
        m.total_items(),
        fp
    );
    if out.is_empty() {
        return Err(HandoverCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知面/空对照族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺面移交族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_MORE_ITEMS；空面条目族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复组面族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；验收后私改族"),
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

/// 规范语料双跑指纹：四面齐备 → 验收 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut m = manifest_three_faces();
    let _ = m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A, HANDOVER_TODO_B]);
    match m.accept() {
        Ok(fp) => {
            let bytes = match compose_handover_summary(&m, fp) {
                Ok(s) => s.into_bytes(),
                Err(_) => Vec::new(),
            };
            fnv1a(&bytes)
        }
        Err(_) => 0,
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10078 判据集（17 条；动态明细 ≤8 条）。
pub fn run_unx10078_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10078");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92380..=0x923BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 面解析器：四 token 全通过
    let mut parse_ok = true;
    let mut i = 0usize;
    let toks = ["caliber", "mech", "gate", "todo"];
    while i < HandoverFace::ALL.len() {
        match (HandoverFace::ALL.get(i), toks.get(i)) {
            (Some(f), Some(t)) => {
                if parse_handover_face(t) != Ok(*f) {
                    parse_ok = false;
                }
            }
            _ => parse_ok = false,
        }
        i += 1;
    }
    s.add("面解析器：四 token 对照全通过", parse_ok,
          "四面闭集对照必须全可解析，闭集外显性拒绝");

    // 3. 未知面注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_handover_face("secret").is_ok()
            || parse_handover_face("").is_ok()
            || parse_handover_face("Caliber").is_ok()
            || parse_handover_face("todo ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知面注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "面歧义必须逐次显性拒绝，不许静默归并");

    // 4. 四面组面：规范清单四面齐备且条目守恒
    let mut m = manifest_three_faces();
    let todo_n = m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A, HANDOVER_TODO_B]);
    let build_ok = todo_n == Ok(2)
        && m.complete()
        && m.total_items() == 8
        && m.missing().is_empty();
    s.add("四面组面：口径/机制/闸位/待办四面齐备", build_ok,
          "四面条目逐条非空，条目总数=四面之和");

    // 5. 验收：四面齐备验收通过且指纹冻结
    let fp = m.accept();
    s.add("移交验收：四面齐备验收通过且指纹冻结",
          fp.is_ok() && m.is_accepted() && fp != Ok(0),
          "验收产出移交凭据（指纹+条目数）供接收面复核");

    // 6. 缺面移交拦截：三面即验收 10/10 次拦截
    let mut miss_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut m2 = manifest_three_faces();
        if m2.accept().err() != Some(HandoverCode::MissingFace)
            || m2.missing().len() != 1
        {
            miss_ok = false;
        }
        r2 += 1;
    }
    s.add("缺面移交拦截：三面即验收 10/10 次拦截", miss_ok,
          "缺面移交等于失面，逐次显性拦截");

    // 7. 逐面缺面归因：四面逐面摘除都能定位缺哪面
    let mut attr_ok = true;
    let mut k = 0usize;
    while k < HandoverFace::ALL.len() {
        // 摘第 k 面：重建时跳过该面
        let mut m3 = HandoverManifest::new();
        let mut j = 0usize;
        while j < HandoverFace::ALL.len() {
            if j != k {
                match (HandoverFace::ALL.get(j), j) {
                    (Some(f), 0) => {
                        let _ = m3.set_face(*f, &["守恒式占位"]);
                    }
                    (Some(f), 1) => {
                        let _ = m3.set_face(*f, &["机制占位"]);
                    }
                    (Some(f), 2) => {
                        let _ = m3.set_face(*f, &["闸位占位"]);
                    }
                    (Some(f), _) => {
                        let _ = m3.set_face(*f, &[HANDOVER_TODO_A]);
                    }
                    _ => {}
                }
            }
            j += 1;
        }
        if m3.accept().err() != Some(HandoverCode::MissingFace)
            || m3.missing().get(0) != Some(HandoverFace::ALL.get(k).unwrap_or(&HandoverFace::TodoList))
        {
            attr_ok = false;
        }
        k += 1;
    }
    s.add("缺面归因：四面逐面摘除均可定位缺面", attr_ok,
          "缺面必须可归因到具体面，不许泛化报错");

    // 8. 空面移交拦截：0 条目组面 10/10 次拒绝
    let mut empty_ok = true;
    let mut r3 = 0u32;
    while r3 < INJECT_ROUNDS {
        let mut m2 = HandoverManifest::new();
        if m2.set_face(HandoverFace::GateList, &[]).err() != Some(HandoverCode::EmptyFaceItems)
            || !m2.face(HandoverFace::GateList).is_none()
        {
            empty_ok = false;
        }
        r3 += 1;
    }
    s.add("空面移交拦截：零条目组面 10/10 次拒绝", empty_ok,
          "空面移交等于失面，组面阶段即拦截");

    // 9. 空条目拦截：条目空串逐次拒绝
    let mut item_ok = true;
    let mut r4 = 0u32;
    while r4 < INJECT_ROUNDS {
        let mut m2 = HandoverManifest::new();
        if m2.set_face(HandoverFace::CaliberBase, &["条目一", ""])
            .err() != Some(HandoverCode::EmptyItem)
            || !m2.face(HandoverFace::CaliberBase).is_none()
        {
            item_ok = false;
        }
        r4 += 1;
    }
    s.add("空条目拦截：空串条目 10/10 次拒绝", item_ok,
          "条目内容不得为空串，空条目即虚报");

    // 10. 注入核：四类坏移交样本各 10/10 拒绝
    let (inj, per_class) = run_bad_handover_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏移交样本各 10/10 拒绝", all_caught,
          "缺面/空面/未知面/重复组面必须逐类逐次拒绝");

    // 11. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadHandover::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 12. 重复组面拒绝
    let mut m5 = manifest_three_faces();
    let _ = m5.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
    s.add("重复组面：DupFace 显性拒绝",
          m5.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_B]).err() == Some(HandoverCode::DupFace),
          "同一面只能组一次，改条目走更正留痕");

    // 13. 验收后私改拦截：已验收清单再组面被拒
    let fp_before = m.fingerprint();
    let _ = m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_B]);
    s.add("验收后私改拦截：已验收清单拒绝再组面",
          m.set_face(HandoverFace::MechList, &["x"]).err() == Some(HandoverCode::DupFace)
              && m.fingerprint() == fp_before,
          "验收即冻结，任何改写即私改交接面");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(61)
        && agrees_with_linux(17) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(1111);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < HandoverCode::ALL.len() {
        let mut j = i2 + 1;
        while j < HandoverCode::ALL.len() {
            if let (Some(a), Some(bb)) = (HandoverCode::ALL.get(i2), HandoverCode::ALL.get(j)) {
                if a.code() == bb.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let text_ok = HandoverCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 面往返 + 成文 + 双跑指纹
    let rt_ok = HandoverFace::ALL.iter().all(|f| {
        let tag = match f {
            HandoverFace::CaliberBase => "caliber",
            HandoverFace::MechList => "mech",
            HandoverFace::GateList => "gate",
            HandoverFace::TodoList => "todo",
        };
        parse_handover_face(tag) == Ok(*f)
    });
    let mut m6 = manifest_three_faces();
    let _ = m6.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
    let fp6 = m6.accept().unwrap_or(0);
    let sum = compose_handover_summary(&m6, fp6);
    let sum_ok = match &sum {
        Ok(t) => t.contains("面数=4") && t.contains("条目=7"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：面标签可回解析且双跑哈希逐位相等", rt_ok && sum_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 17. 移交凭据复核：指纹重算一致（接收面复核路径）
    let mut m7 = manifest_three_faces();
    let _ = m7.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A, HANDOVER_TODO_B]);
    let fp7a = m7.fingerprint();
    let mut m8 = manifest_three_faces();
    let _ = m8.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A, HANDOVER_TODO_B]);
    let fp7b = m8.fingerprint();
    s.add("凭据复核：同内容清单指纹逐位一致", fp7a == fp7b && fp7a != 0,
          "接收面以指纹复核移交内容，重算不一致即失真");

    // 动态明细（≤8）：注入账快照 + 条目账快照 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let items_text = format!("四面条目 {}（验收 {}）", m6.total_items(), m6.is_accepted());
    s.add_dyn("条目账快照", m6.complete() && m6.total_items() == 7, &items_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_parser_closed_set() {
        assert_eq!(parse_handover_face("caliber"), Ok(HandoverFace::CaliberBase));
        assert_eq!(parse_handover_face("todo"), Ok(HandoverFace::TodoList));
        assert!(parse_handover_face("secret").is_err());
        assert_eq!(parse_handover_face("").err(), Some(HandoverCode::EmptyFace));
    }

    #[test]
    fn manifest_builds_and_conserves() {
        let mut m = manifest_three_faces();
        assert_eq!(m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A, HANDOVER_TODO_B]), Ok(2));
        assert!(m.complete());
        assert_eq!(m.total_items(), 8);
        assert!(m.missing().is_empty());
    }

    #[test]
    fn accept_freezes_manifest() {
        let mut m = manifest_three_faces();
        let _ = m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
        assert!(m.accept().is_ok());
        assert!(m.is_accepted());
        assert_eq!(m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_B]).err(),
                   Some(HandoverCode::DupFace));
    }

    #[test]
    fn missing_face_rejected() {
        let mut m = manifest_three_faces();
        assert_eq!(m.accept().err(), Some(HandoverCode::MissingFace));
        let miss = m.missing();
        assert_eq!(miss.len(), 1);
        assert_eq!(miss.get(0), Some(&HandoverFace::TodoList));
    }

    #[test]
    fn empty_face_and_item_rejected() {
        let mut m = HandoverManifest::new();
        assert_eq!(m.set_face(HandoverFace::CaliberBase, &[]).err(),
                   Some(HandoverCode::EmptyFaceItems));
        assert_eq!(m.set_face(HandoverFace::CaliberBase, &[""]).err(),
                   Some(HandoverCode::EmptyItem));
        assert!(m.face(HandoverFace::CaliberBase).is_none());
    }

    #[test]
    fn dup_face_rejected() {
        let mut m = manifest_three_faces();
        let _ = m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
        assert_eq!(m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_B]).err(),
                   Some(HandoverCode::DupFace));
    }

    #[test]
    fn bad_handover_matrix_green() {
        let (inj, per) = run_bad_handover_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_reproducible() {
        let mut a = manifest_three_faces();
        let _ = a.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
        let mut b = manifest_three_faces();
        let _ = b.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn summary_compose() {
        let mut m = manifest_three_faces();
        let _ = m.set_face(HandoverFace::TodoList, &[HANDOVER_TODO_A]);
        let fp = m.accept().unwrap_or(0);
        let s = compose_handover_summary(&m, fp).unwrap_or_default();
        assert!(s.contains("面数=4") && s.contains("条目=7"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(61));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10078_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10078_checks_green() {
        let set = run_unx10078_checks();
        assert!(set.all_passed(), "UNX-F10078 red: {}", set.red_items_text());
    }
}
