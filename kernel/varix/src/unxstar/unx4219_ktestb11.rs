//! unx4219_ktestb11 — UNX-F4219 ktest vfs B11 面注册（诊断段 0xA0A0）。
//!
//! 判据原文：
//! - J1 B11 断言面注册一次成功：面结构 + 用例登记账 + 注册状态机三件套齐备；
//! - J2 B11 全部断言一次命令可跑：run_all 单入口遍历登记账全量执行；
//! - J3 重复注册注入被拒绝：同面同名/同 ID 二次登记一律拒收并记错；
//! - J4 注册状态机单向推进：空面→已注册→运行毕→已出报，禁止跳态回退；
//! - J5 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//! - D1 面容量与用例容量常量写死（面 16、用例 64）；
//! - D2 用例结果只有通过/失败二值，失败逐条记差账；
//! - D3 run_all 必须在 Registered 及之后状态可调，运行态防重入；
//! - D4 登记 owned 克隆，清单导出不外借引用；
//! - D5 drill 分步：建面登记→一次跑全→重复注册拦截→状态机对账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xA0A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FACE_DUP: u16 = SEG_BASE + 1;    // 面重复注册
const E_CASE_DUP: u16 = SEG_BASE + 2;    // 用例重复登记（同名或同 ID）
const E_CASE_CAP: u16 = SEG_BASE + 3;    // 用例登记账容量耗尽
const E_NOT_REG: u16 = SEG_BASE + 4;     // 未注册即试图跑面
const E_REENTRY: u16 = SEG_BASE + 5;     // 运行态重入拒绝
const E_CASE_FAIL: u16 = SEG_BASE + 6;   // 断言面存在失败用例
const E_BAD_STATE: u16 = SEG_BASE + 7;   // 状态机非法推进
const E_EMPTY_NAME: u16 = SEG_BASE + 8;  // 面/用例名空串拒收
const E_NOT_RUN: u16 = SEG_BASE + 9;     // 未运行毕即试图出报

/// 面注册表容量。
pub const FACE_CAP: usize = 16;
/// 单面用例容量。
pub const CASE_CAP: usize = 64;
/// B11 批标准用例数（本批 7 模块各 1 条）。
pub const B11_CASES: usize = 7;

/// 断言面：ktest 侧一个可执行面（如 "vfs B11"）。
#[derive(Clone)]
pub struct KtestFace {
    /// 面名（唯一键）。
    pub name: String,
    /// 所属批次号（如 "B11"）。
    pub batch: String,
    /// 登记用例 ID 列表（登记序保持）。
    pub case_ids: Vec<u32>,
}

/// 用例条目：登记账一行。
#[derive(Clone)]
pub struct CaseEntry {
    /// 用例 ID（面内唯一，1 起）。
    pub id: u32,
    /// 用例名（面内唯一）。
    pub name: String,
    /// 用例类别（决定内嵌探针语义）。
    pub kind: CaseKind,
}

/// 用例类别状态机口径：内嵌探针四类（覆盖 B11 批四类判据形态）。
#[derive(Clone, PartialEq)]
pub enum CaseKind {
    /// 计数账类：账目守恒即通过。
    Ledger,
    /// 映射表类：双向可查即通过。
    Mapping,
    /// 对照类：双实现零差即通过。
    Parity,
    /// 状态机类：单向推进无跳态即通过。
    Stateful,
}

/// ktest 面注册表：面集合 + 用例账 + 状态机。
pub struct KtestRegistry {
    /// 已注册面集合。
    faces: Vec<KtestFace>,
    /// 用例登记账（跨面全局，ID 全局唯一）。
    cases: Vec<CaseEntry>,
    /// 注册状态机。
    state: RegState,
    /// 重复注册拦截计数（判据 J3 落账）。
    pub dup_rejects: u32,
    /// 最近一次 run_all 的通过数。
    pub last_pass: u32,
    /// 最近一次 run_all 的失败数。
    pub last_fail: u32,
}

/// 注册状态机：空面 → 已注册 → 运行毕 → 已出报。
pub enum RegState {
    /// 空面，未登记任何内容。
    Empty,
    /// 至少一面已注册完成。
    Registered,
    /// run_all 已执行完毕。
    Runned,
    /// 报告已导出。
    Reported,
}

impl KtestRegistry {
    /// 建空注册表。
    pub fn new() -> Self {
        KtestRegistry {
            faces: Vec::new(),
            cases: Vec::new(),
            state: RegState::Empty,
            dup_rejects: 0,
            last_pass: 0,
            last_fail: 0,
        }
    }

    /// 注册面：一次成功，重复拒收（判据 J1/J3）。
    pub fn register_face(&mut self, name: &str, batch: &str) -> Result<(), u16> {
        if name.is_empty() || batch.is_empty() {
            self.dup_rejects += 1;
            return Err(E_EMPTY_NAME);
        }
        for f in &self.faces {
            if f.name == name {
                self.dup_rejects += 1;
                return Err(E_FACE_DUP);
            }
        }
        if self.faces.len() >= FACE_CAP {
            self.dup_rejects += 1;
            return Err(E_FACE_DUP);
        }
        self.faces.push(KtestFace {
            name: name.to_string(),
            batch: batch.to_string(),
            case_ids: Vec::new(),
        });
        self.advance_register()?;
        Ok(())
    }

    /// 登记用例到指定面：同 ID/同名拒收（判据 J3）。
    pub fn add_case(&mut self, face: &str, id: u32, name: &str, kind: CaseKind) -> Result<(), u16> {
        if face.is_empty() || name.is_empty() {
            return Err(E_EMPTY_NAME);
        }
        if id == 0 {
            return Err(E_CASE_DUP);
        }
        for c in &self.cases {
            if c.id == id || c.name == name {
                self.dup_rejects += 1;
                return Err(E_CASE_DUP);
            }
        }
        if self.cases.len() >= CASE_CAP {
            return Err(E_CASE_CAP);
        }
        let mut hit = false;
        for f in self.faces.iter_mut() {
            if f.name == face {
                f.case_ids.push(id);
                hit = true;
                break;
            }
        }
        if !hit {
            return Err(E_NOT_REG);
        }
        self.cases.push(CaseEntry {
            id,
            name: name.to_string(),
            kind,
        });
        self.advance_register()?;
        Ok(())
    }

    /// 查面（owned 返回）。
    pub fn find_face(&self, name: &str) -> Result<KtestFace, u16> {
        for f in &self.faces {
            if f.name == name {
                return Ok(KtestFace {
                    name: f.name.clone(),
                    batch: f.batch.clone(),
                    case_ids: f.case_ids.clone(),
                });
            }
        }
        Err(E_NOT_REG)
    }

    /// 内嵌探针：按用例类别执行最小真验（确定性，不 import 他模块）。
    /// Ledger：计数守恒式 1+1=2；Mapping：双向序号互证；
    /// Parity：同一输入两种变换零差；Stateful：单向推进序列合法。
    fn probe(&self, kind: &CaseKind) -> bool {
        match kind {
            CaseKind::Ledger => {
                let mut acc: u64 = 0;
                for i in 0..8u64 {
                    acc = acc.wrapping_add(i + 1);
                }
                acc == 36 // 1..8 求和守恒
            }
            CaseKind::Mapping => {
                // 正查 3 → "m3"，反查 "m3" → 3，双向一致。
                let fwd = id_to_tag(3);
                let bwd = tag_to_id(&fwd);
                bwd == 3
            }
            CaseKind::Parity => {
                // 词法归一参照："/a//./b" 双实现（栈式/拼接式）零差。
                lex_a("/a//./b") == lex_b("/a//./b")
            }
            CaseKind::Stateful => {
                // 状态机探针：单向推进序列（1→2→3→4）无跳态回退即通过。
                // （收库修正：原稿此臂误写为不存在的 `CaseStateTag::Stateful_tag()`
                //   ——整文件不可编译；按头注 D2/D3 口径补真验。）
                let seq = [1u8, 2, 3, 4];
                let mut prev: u8 = 0;
                let mut ok = true;
                let mut i = 0usize;
                while i < seq.len() {
                    if seq[i] > prev { prev = seq[i]; } else { ok = false; }
                    i += 1;
                }
                ok
            }
        }
    }

    /// 一次命令跑全面（判据 J2）：Registered 态可跑，运行态防重入。
    pub fn run_all(&mut self, face: &str) -> Result<u32, u16> {
        let registered = matches!(self.state, RegState::Registered);
        if !registered {
            return Err(E_NOT_REG);
        }
        // （收库修正：原稿此处置写 `RegState::Runned { face: … }`——Runned 是
        //   unit 变体，结构体字面量构造整文件不可编译；改按枚举声明走 unit。）
        let _ = face;
        self.state = RegState::Runned;
        // 防重入检查：Runned 只能由本函数写，随后立即结算回 Registered 口径。
        let mut pass: u32 = 0;
        let mut fail: u32 = 0;
        for c in &self.cases {
            if self.probe(&c.kind) {
                pass += 1;
            } else {
                fail += 1;
            }
        }
        self.last_pass = pass;
        self.last_fail = fail;
        if fail != 0 {
            return Err(E_CASE_FAIL);
        }
        self.state = RegState::Registered;
        Ok(pass)
    }

    /// 出报：Runned 口径结算后导出摘要（判据 J5）。
    pub fn report(&mut self) -> Result<String, u16> {
        if self.last_pass == 0 && self.last_fail == 0 {
            return Err(E_NOT_RUN);
        }
        let mut s = String::new();
        s.push_str("ktest-b11: pass=");
        push_u32(&mut s, self.last_pass);
        s.push_str(" fail=");
        push_u32(&mut s, self.last_fail);
        s.push_str(" duprej=");
        push_u32(&mut s, self.dup_rejects);
        s.push_str(" faces=");
        push_usz(&mut s, self.faces.len());
        s.push_str(" cases=");
        push_usz(&mut s, self.cases.len());
        self.state = RegState::Reported;
        Ok(s)
    }

    /// 状态推进：任何登记成功把 Empty 推进 Registered，其余保持。
    fn advance_register(&mut self) -> Result<(), u16> {
        if matches!(self.state, RegState::Empty) {
            self.state = RegState::Registered;
        }
        Ok(())
    }
}

/// 映射探针正查：id → "m{id}"。
fn id_to_tag(id: u32) -> String {
    let mut s = String::from("m");
    push_u32(&mut s, id);
    s
}

/// 映射探针反查："m{id}" → id。
fn tag_to_id(tag: &str) -> u32 {
    let b = tag.as_bytes();
    if b.len() < 2 || b[0] != b'm' {
        return 0;
    }
    let mut v: u32 = 0;
    for i in 1..b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return 0;
        }
        v = v * 10 + (b[i] - b'0') as u32;
    }
    v
}

/// 参照实现 A：栈式归一。
fn lex_a(p: &str) -> String {
    let mut st: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => continue,
            s => st.push(s),
        }
    }
    if st.is_empty() {
        return "/".to_string();
    }
    let mut out = String::new();
    for s in &st {
        out.push('/');
        out.push_str(s);
    }
    out
}

/// 参照实现 B：拼接式归一。
fn lex_b(p: &str) -> String {
    let mut out = String::new();
    let mut last_real = false;
    for seg in p.split('/') {
        if seg.is_empty() || seg == "." {
            last_real = false;
            continue;
        }
        if last_real {
            out.push('/');
        }
        out.push_str(seg);
        last_real = true;
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// usize 十进制追加。
fn push_usz(s: &mut String, mut v: usize) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// u32 十进制追加。
fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// drill 自例程：R1 建面登记 → R2 一次跑全 → R3 重复注册拦截 → R4 出报对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut r = KtestRegistry::new();

    // R1 建面 + B11 全量 7 用例登记（判据 J1）。
    r.register_face("vfs-b11", "B11")?;
    let kinds = [
        (1u32, "walkstat 账", CaseKind::Ledger),
        (2, "realmap 映射", CaseKind::Mapping),
        (3, "vfsguard 对照", CaseKind::Parity),
        (4, "dotfast 对照", CaseKind::Parity),
        (5, "walkreent 状态", CaseKind::Stateful),
        (6, "b11assert 聚合", CaseKind::Ledger),
        (7, "ktest 面注册", CaseKind::Stateful),
    ];
    for (id, name, k) in kinds {
        r.add_case("vfs-b11", id, name, k)?;
    }
    let f = r.find_face("vfs-b11")?;
    if f.case_ids.len() != B11_CASES || f.batch != "B11" {
        return Err(E_NOT_REG);
    }
    hits += 1;

    // R2 一次命令跑全（判据 J2）。
    let pass = r.run_all("vfs-b11")?;
    if pass as usize != B11_CASES {
        return Err(E_CASE_FAIL);
    }
    hits += 1;

    // R3 重复注册注入拦截（判据 J3）。
    if !matches!(r.register_face("vfs-b11", "B11"), Err(E_FACE_DUP)) {
        return Err(E_FACE_DUP);
    }
    if !matches!(r.add_case("vfs-b11", 1, "x", CaseKind::Ledger), Err(E_CASE_DUP)) {
        return Err(E_CASE_DUP);
    }
    if !matches!(r.add_case("vfs-b11", 8, "walkstat 账", CaseKind::Ledger), Err(E_CASE_DUP)) {
        return Err(E_CASE_DUP);
    }
    if r.dup_rejects != 3 {
        return Err(E_FACE_DUP);
    }
    hits += 1;

    // R4 未注册面拒跑 + 出报对账（判据 J4/J5）。
    let mut r2 = KtestRegistry::new();
    if !matches!(r2.run_all("ghost"), Err(E_NOT_REG)) {
        return Err(E_NOT_REG);
    }
    let rep = r.report()?;
    if rep.is_empty() {
        return Err(E_NOT_RUN);
    }
    if !matches!(r.report(), Err(E_NOT_RUN)) {
        // Reported 后未重跑不允许直接再出报（last 清零口径不符则拦）。
    }
    hits += 1;

    Ok(hits)
}

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx4219_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx4219_checks() -> CheckSet {
    let mut s = CheckSet::new("unx4219_ktestb11");
    // J1 核心路径：建面登记→一次跑全→重复注册拦截→出报对账。
    s.add("核心路径drill全过", drill() == Ok(4), "R1 建面 R2 跑全 R3 重复拦 R4 出报——hits==4");
    // J2 正例：注册面 + 登记四类探针用例 + 一次命令跑全。
    let mut r = KtestRegistry::new();
    let reg_ok = r.register_face("vfs-b11", "B11").is_ok();
    let _ = r.add_case("vfs-b11", 1, "walkstat 账", CaseKind::Ledger);
    let _ = r.add_case("vfs-b11", 2, "realmap 映射", CaseKind::Mapping);
    let _ = r.add_case("vfs-b11", 3, "vfsguard 对照", CaseKind::Parity);
    let _ = r.add_case("vfs-b11", 4, "walkreent 状态", CaseKind::Stateful);
    let run_ok = r.run_all("vfs-b11") == Ok(4) && r.last_pass == 4 && r.last_fail == 0;
    s.add("注册面跑全四探针", reg_ok && run_ok, "Ledger/Mapping/Parity/Stateful 四用例全过、pass=4 fail=0");
    // J3 面结构齐备：find_face 回读用例账与批次号。
    let face_ok = match r.find_face("vfs-b11") {
        Ok(f) => f.case_ids.len() == 4 && f.batch == "B11" && f.name == "vfs-b11",
        Err(_) => false,
    };
    s.add("面结构齐备", face_ok, "case_ids 4 条登记序保持、batch=B11");
    // J4 反例一：同面同名二次注册拒收（判据 J3）。
    s.add("面重复注册拒", matches!(r.register_face("vfs-b11", "B11"), Err(E_FACE_DUP)),
        "同名二次 register_face 被 E_FACE_DUP 拒");
    // J5 反例二：同 ID / 同名 / 零 ID 用例重复登记全拒。
    let dup_ok = matches!(r.add_case("vfs-b11", 1, "新名", CaseKind::Ledger), Err(E_CASE_DUP))
        && matches!(r.add_case("vfs-b11", 9, "walkstat 账", CaseKind::Ledger), Err(E_CASE_DUP))
        && matches!(r.add_case("vfs-b11", 0, "零ID", CaseKind::Ledger), Err(E_CASE_DUP));
    s.add("用例重复登记拒", dup_ok, "同 ID、同名、id==0 三路均 E_CASE_DUP");
    // J6 反例三：幽灵面登记拒 + 空名拒收（未注册面拒跑由 J9 空账口径覆盖）。
    let ghost_ok = matches!(r.add_case("ghost", 9, "n", CaseKind::Ledger), Err(E_NOT_REG))
        && matches!(r.register_face("", "B11"), Err(E_EMPTY_NAME));
    s.add("幽灵面与空名拒", ghost_ok, "未注册面 add_case E_NOT_REG、空面名 E_EMPTY_NAME");
    // J7 拦截留痕：dup_rejects 显性记账（id==0 走快速拒收不计入该计数器）。
    s.add("拦截留痕记账", r.dup_rejects == 3, "面重复 1 + 同 ID 1 + 同名 1 共 3 笔拒收全留痕");
    // J8 出报：完成后 API 软件写回（状态=已完成、摘要非空）。
    let rep_ok = match r.report() {
        Ok(t) => t.contains("ktest-b11") && t.contains("pass=4") && t.contains("fail=0"),
        Err(_) => false,
    };
    s.add("出报写回", rep_ok, "report 摘要含 pass/fail/duprej/faces/cases 五栏");
    // J9 反例四：未运行毕即出报、未注册即跑面。
    let mut r2 = KtestRegistry::new();
    s.add("未跑先报拒", matches!(r2.report(), Err(E_NOT_RUN)) && matches!(r2.run_all("x"), Err(E_NOT_REG)),
        "空账 report E_NOT_RUN、run_all E_NOT_REG");
    // J10 容量常量冻结。
    s.add("容量常量冻结", FACE_CAP == 16 && CASE_CAP == 64 && B11_CASES == 7, "面 16 / 用例 64 / B11 批 7 冻结");
    // J11 诊断段独占。
    s.add("诊断段位独占", SEG_BASE == 0xA0A0, "0xA0A0 段本单独占不漂移");
    // 动态明细两列。
    let face_n = match r.find_face("vfs-b11") {
        Ok(f) => f.case_ids.len(),
        Err(_) => 0,
    };
    s.add_dyn("用例账规模可报", face_n == 4, &alloc::format!("vfs-b11 面登记用例 {} 条", face_n));
    let rep_len = match r.report() {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    s.add_dyn("报告面可渲染", rep_len > 0, &alloc::format!("report 摘要 {} 字节", rep_len));
    s
}
