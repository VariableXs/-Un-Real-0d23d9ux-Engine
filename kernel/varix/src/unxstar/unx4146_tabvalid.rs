//! unx4146_tabvalid — UNX-F4146 表注册校验器（逐函数签名/非空/裁断检查）（诊断段 0x9400）。
//!
//! 判据原文：UNX-F4146-J1 校验器对签名错/空必选函数/裁断不合格三类注入各 10/10 次拒绝
//!           并给出定位信息（错误报告三要素齐备）。
//!   J2 三类错误各给专码，互不吞并。
//!   J3 报告三要素（行号/类别/码）齐备。
//!   J4 同一表两次校验结论相同且不留相位残留。
//! 设计纪律：
//!   1. 三类错误各立专码、互不吞并：签名错（元数/返回类不符）、空必选（必选位未置）、
//!      裁断不合格（组合层面不满足最小集）。三类混判会让返工方向错，故必须分离。
//!   2. 错误报告三要素固定为「行号 + 错误类 + 说明码」：行号定位到表内第几行，
//!      错误类定返工方向，说明码定具体不符项；缺一即视为报告不完整。
//!   3. 校验是纯读操作：合法性判断不改表、不留状态，只有 ValidState 推进；
//!      同一表重复校验必须得同一结论（确定性）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9400 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9400;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SIG: u16 = SEG_BASE + 1; // 签名错（元数或返回类不符）
const E_EMPTYREQ: u16 = SEG_BASE + 2; // 空必选函数（必选位未置）
const E_ADJ: u16 = SEG_BASE + 3; // 裁断不合格（最小集不成立）
const E_ROW: u16 = SEG_BASE + 4; // 行号越界
const E_COUNT: u16 = SEG_BASE + 5; // 行数不符
const E_REPORT: u16 = SEG_BASE + 6; // 错误报告要素不齐
const E_STATE: u16 = SEG_BASE + 7; // 状态机非法迁移
const E_DUP: u16 = SEG_BASE + 8; // 表内函数名重复
const E_ACCT: u16 = SEG_BASE + 9; // 注入账/通过账对平失败

/// 表行数（8 行：5 必选 + 3 可选）。
pub const ROW_COUNT: usize = 8;
/// 必选行数。
pub const REQ_ROWS: usize = 5;

/// 返回类别（签名的一部分）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetClass {
    Int = 0,
    Ptr = 1,
    Void = 2,
}

/// 错误类别（三要素之二）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrClass {
    None = 0,
    Signature = 1,
    EmptyMandatory = 2,
    Adjudication = 3,
}

impl ErrClass {
    pub fn code(self) -> u16 {
        match self {
            ErrClass::None => 0,
            ErrClass::Signature => E_SIG,
            ErrClass::EmptyMandatory => E_EMPTYREQ,
            ErrClass::Adjudication => E_ADJ,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ErrClass::None => "none",
            ErrClass::Signature => "signature",
            ErrClass::EmptyMandatory => "empty-mandatory",
            ErrClass::Adjudication => "adjudication",
        }
    }
}

/// 校验相位机：Unchecked → Checking → Valid / Invalid。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValidState {
    Unchecked,
    Checking,
    Valid,
    Invalid,
}

/// 表行：名 + 元数 + 返回类 + 是否必选 + 是否已绑定实现。
#[derive(Clone, Copy)]
pub struct TabRow {
    /// 函数名
    pub name: &'static str,
    /// 元数
    pub argc: u8,
    /// 返回类别
    pub ret: RetClass,
    /// 是否必选
    pub mandatory: bool,
    /// 是否绑定了实现
    pub bound: bool,
}

/// 规范签名（8 行的冻结值）。
pub const CANON: [TabRow; ROW_COUNT] = [
    TabRow { name: "alloc_inode", argc: 2, ret: RetClass::Ptr, mandatory: true, bound: true },
    TabRow { name: "destroy_inode", argc: 2, ret: RetClass::Void, mandatory: true, bound: true },
    TabRow { name: "get_tree", argc: 3, ret: RetClass::Ptr, mandatory: true, bound: true },
    TabRow { name: "kill_sb", argc: 1, ret: RetClass::Void, mandatory: true, bound: true },
    TabRow { name: "lookup", argc: 4, ret: RetClass::Ptr, mandatory: true, bound: true },
    TabRow { name: "statfs", argc: 2, ret: RetClass::Int, mandatory: false, bound: false },
    TabRow { name: "sync_fs", argc: 2, ret: RetClass::Int, mandatory: false, bound: false },
    TabRow { name: "show_options", argc: 3, ret: RetClass::Int, mandatory: false, bound: false },
];

/// 错误报告：三要素（行号 / 错误类 / 说明码）。
#[derive(Clone, Copy)]
pub struct Report {
    /// 要素 1：表内行号（越界时为 ROW_COUNT）
    pub row: usize,
    /// 要素 2：错误类
    pub class: ErrClass,
    /// 要素 3：说明码（与 class 对应的诊断码）
    pub code: u16,
}

impl Report {
    /// 三要素齐备：行号在界内（或明确的越界哨兵）、类别非 None、说明码非 0。
    pub fn complete(&self) -> bool {
        self.row <= ROW_COUNT && self.class != ErrClass::None && self.code != 0
    }

    /// 一句话描述（日志用，不参与判定）。
    pub fn text(&self) -> &'static str {
        match self.class {
            ErrClass::Signature => "签名不符：元数或返回类与冻结值不一致",
            ErrClass::EmptyMandatory => "必选函数未绑定实现",
            ErrClass::Adjudication => "裁断不合格：最小集不成立",
            ErrClass::None => "无错误",
        }
    }
}

/// 校验器：表 + 状态 + 三类拒收账。
pub struct TabValidator {
    /// 被校验表
    pub rows: Vec<TabRow>,
    /// 相位
    pub state: ValidState,
    /// 签名类拒收次数
    pub sig_rejects: u32,
    /// 空必选类拒收次数
    pub empty_rejects: u32,
    /// 裁断类拒收次数
    pub adj_rejects: u32,
    /// 通过次数
    pub passes: u32,
}

impl TabValidator {
    pub fn new() -> TabValidator {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < ROW_COUNT {
            v.push(CANON[i]);
            i += 1;
        }
        TabValidator {
            rows: v,
            state: ValidState::Unchecked,
            sig_rejects: 0,
            empty_rejects: 0,
            adj_rejects: 0,
            passes: 0,
        }
    }

    /// 表内函数名唯一性（重名即签名类错误的特例）。
    fn dup_name(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let mut k = i + 1;
            while k < self.rows.len() {
                if self.rows[i].name == self.rows[k].name {
                    return Some(k);
                }
                k += 1;
            }
            i += 1;
        }
        None
    }

    /// 校验主流程：行数 → 重名 → 逐行签名 → 逐行必选非空 → 裁断。
    pub fn validate(&mut self) -> Result<(), Report> {
        self.state = ValidState::Checking;
        if self.rows.len() != ROW_COUNT {
            self.state = ValidState::Invalid;
            self.sig_rejects += 1;
            return Err(Report {
                row: self.rows.len(),
                class: ErrClass::Signature,
                code: E_COUNT,
            });
        }
        if let Some(r) = self.dup_name() {
            self.state = ValidState::Invalid;
            self.sig_rejects += 1;
            return Err(Report {
                row: r,
                class: ErrClass::Signature,
                code: E_DUP,
            });
        }
        // 逐行签名
        let mut i = 0usize;
        while i < ROW_COUNT {
            if self.rows[i].name != CANON[i].name {
                self.state = ValidState::Invalid;
                self.sig_rejects += 1;
                return Err(Report {
                    row: i,
                    class: ErrClass::Signature,
                    code: E_SIG,
                });
            }
            if self.rows[i].argc != CANON[i].argc || self.rows[i].ret != CANON[i].ret {
                self.state = ValidState::Invalid;
                self.sig_rejects += 1;
                return Err(Report {
                    row: i,
                    class: ErrClass::Signature,
                    code: E_SIG,
                });
            }
            i += 1;
        }
        // 逐行必选非空
        let mut k = 0usize;
        while k < ROW_COUNT {
            if self.rows[k].mandatory && !self.rows[k].bound {
                self.state = ValidState::Invalid;
                self.empty_rejects += 1;
                return Err(Report {
                    row: k,
                    class: ErrClass::EmptyMandatory,
                    code: E_EMPTYREQ,
                });
            }
            k += 1;
        }
        // 裁断：必选 5 项必须齐备且必选行元数合计与冻结值一致
        let mut req_ok = 0usize;
        let mut argc_sum = 0u32;
        let mut m = 0usize;
        while m < ROW_COUNT {
            if self.rows[m].mandatory {
                req_ok += 1;
                argc_sum += self.rows[m].argc as u32;
            }
            m += 1;
        }
        if req_ok != REQ_ROWS || argc_sum != 12 {
            self.state = ValidState::Invalid;
            self.adj_rejects += 1;
            return Err(Report {
                row: ROW_COUNT,
                class: ErrClass::Adjudication,
                code: E_ADJ,
            });
        }
        self.state = ValidState::Valid;
        self.passes += 1;
        Ok(())
    }

    /// 三类拒收账守恒：三数之和 == 全部校验次数 - 通过次数。
    pub fn accounted(&self) -> u32 {
        self.sig_rejects + self.empty_rejects + self.adj_rejects
    }

    /// 结论稳定：同一表两次校验结论相同（确定性自查）。
    pub fn stable(&mut self) -> bool {
        let before = self.state;
        let a = self.validate().is_ok();
        self.state = before;
        let b = self.validate().is_ok();
        self.state = before; // 自查不得留下相位残留
        a == b
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SIG => "签名错（元数或返回类不符）",
        E_EMPTYREQ => "空必选函数（必选位未绑定实现）",
        E_ADJ => "裁断不合格（最小集不成立）",
        E_ROW => "行号越界",
        E_COUNT => "行数与冻结表不符",
        E_REPORT => "错误报告三要素不齐",
        E_STATE => "状态机非法迁移",
        E_DUP => "表内函数名重复",
        E_ACCT => "注入账/通过账对平失败",
        _ => "未知诊断码",
    }
}

/// 确定性演练：正表通过 → 三类注入各 10 次 → 报告三要素 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：规范表（5 必选已绑 + 3 可选未绑）一次通过。
    let mut v = TabValidator::new();
    if v.validate().is_err() {
        return Err(E_ADJ);
    }
    if v.state != ValidState::Valid || v.passes != 1 || v.accounted() != 0 {
        return Err(E_STATE);
    }
    hits += 1;

    // R2：签名错注入 10/10——每轮改一行的元数，必报 E_SIG 且行号精确。
    let mut sig = 0u32;
    let mut i = 0usize;
    while i < 10 {
        let mut vv = TabValidator::new();
        let row = i % ROW_COUNT;
        vv.rows[row].argc = vv.rows[row].argc.wrapping_add(1);
        match vv.validate() {
            Err(r) => {
                if r.class != ErrClass::Signature || r.row != row || r.code != E_SIG {
                    return Err(E_REPORT);
                }
                if !r.complete() {
                    return Err(E_REPORT); // 三要素齐备
                }
            }
            Ok(()) => return Err(E_SIG),
        }
        if vv.sig_rejects != 1 || vv.state != ValidState::Invalid {
            return Err(E_STATE);
        }
        sig += 1;
        i += 1;
    }
    if sig != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：空必选注入 10/10——逐轮把一条必选行的 bound 置假，必报 E_EMPTYREQ。
    let mut emp = 0u32;
    let mut k = 0usize;
    while k < 10 {
        let mut vv = TabValidator::new();
        let row = k % REQ_ROWS; // 只在前 5 行（必选）里挑
        vv.rows[row].bound = false;
        match vv.validate() {
            Err(r) => {
                if r.class != ErrClass::EmptyMandatory || r.row != row || r.code != E_EMPTYREQ {
                    return Err(E_REPORT);
                }
                if !r.complete() || r.text().len() == 0 {
                    return Err(E_REPORT);
                }
            }
            Ok(()) => return Err(E_EMPTYREQ),
        }
        if vv.empty_rejects != 1 {
            return Err(E_ACCT);
        }
        emp += 1;
        k += 1;
    }
    if emp != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：裁断不合格注入 10/10——把一条必选行降级为可选（必选数变 4）。
    let mut adj = 0u32;
    let mut m = 0usize;
    while m < 10 {
        let mut vv = TabValidator::new();
        let row = m % REQ_ROWS;
        vv.rows[row].mandatory = false;
        match vv.validate() {
            Err(r) => {
                if r.class != ErrClass::Adjudication || r.code != E_ADJ || !r.complete() {
                    return Err(E_REPORT);
                }
            }
            Ok(()) => return Err(E_ADJ),
        }
        if vv.adj_rejects != 1 {
            return Err(E_ACCT);
        }
        adj += 1;
        m += 1;
    }
    if adj != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：三要素分离——三类报告的行号/类别/码三元组互不相同（不吞并）。
    let mut a = TabValidator::new();
    a.rows[0].argc = 9;
    let ra = match a.validate() {
        Err(r) => r,
        Ok(()) => return Err(E_SIG),
    };
    let mut b = TabValidator::new();
    b.rows[0].bound = false;
    let rb = match b.validate() {
        Err(r) => r,
        Ok(()) => return Err(E_EMPTYREQ),
    };
    let mut c = TabValidator::new();
    c.rows[0].mandatory = false;
    let rc = match c.validate() {
        Err(r) => r,
        Ok(()) => return Err(E_ADJ),
    };
    if ra.class == rb.class || rb.class == rc.class || ra.class == rc.class {
        return Err(E_REPORT);
    }
    if ra.code == rb.code || rb.code == rc.code || ra.code == rc.code {
        return Err(E_REPORT);
    }
    if ra.row != 0 || rb.row != 0 {
        return Err(E_REPORT);
    }
    hits += 1;

    // I1：行数不符 → 签名类 + E_COUNT，行号为实际长度。
    let mut d = TabValidator::new();
    d.rows.pop();
    match d.validate() {
        Err(r) => {
            if r.code != E_COUNT || r.row != ROW_COUNT - 1 || r.class != ErrClass::Signature {
                return Err(E_COUNT);
            }
        }
        Ok(()) => return Err(E_COUNT),
    }
    hits += 1;

    // I2：表内重名 → 签名类 + E_DUP，行号指向后出现的那一行。
    let mut e = TabValidator::new();
    let dup_name = match e.rows.get(0) {
        Some(r) => r.name,
        None => return Err(E_COUNT),
    };
    e.rows[7].name = dup_name;
    match e.validate() {
        Err(r) => {
            if r.code != E_DUP || r.row != 7 {
                return Err(E_DUP);
            }
        }
        Ok(()) => return Err(E_DUP),
    }
    hits += 1;

    // I3：结论稳定——同一表两次校验同结论（确定性）。
    let mut f = TabValidator::new();
    if !f.stable() {
        return Err(E_STATE);
    }
    if f.state != ValidState::Unchecked {
        return Err(E_STATE); // stable 内不得留下相位残留
    }
    hits += 1;

    // C1：账目终审——注入账与通过账相加等于校验次数（含 stable 的两次）。
    if v.passes != 1 || v.accounted() != 0 {
        return Err(E_ACCT);
    }
    // a/b/c/d/e 各 1 次失败；f 通过 2 次（stable 内两次）
    if a.accounted() != 1 || b.accounted() != 1 || c.accounted() != 1 {
        return Err(E_ACCT);
    }
    if d.accounted() != 1 || e.accounted() != 1 {
        return Err(E_ACCT);
    }
    if f.passes != 2 || f.accounted() != 0 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
