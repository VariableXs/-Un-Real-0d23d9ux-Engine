//! VE-F3751 · 创作无障碍豁免
//!
//! 锚点：创作无障碍豁免（F3152 模式创作版复刻：豁免类型（创作工具豁免
//! （技术不可行证明制复述；豁免禁类（创作产出禁类（产出无障碍不可豁
//! （产出豁免禁令：创作者不能豁免自己作品的对比度（产出豁免禁令 P0——用户受害
//! 不可豁（禁令逻辑声明：工具豁免影响创作者，产出豁免影响终端用户——终端用户
//! 保护优先（保护优先红线；台账归一复述；到期强制复述。
//!
//! 数据结构：证明制复述；产出禁令（P0）；台账复述；到期复述。
//! 错误路径与降级矩阵：产出豁免检出→P0（红线最高级实测——豁免清单审计）；
//! 无证明→流程拒绝（复述）；到期未失效→P1（复用红线实测）；台账分叉→归一（复述）。
//! 跨批对接点：F3152 单源复用声明；F3607 验证对端；F3757 文化对端。
//!
//! 诊断码段：独占 0x8Exx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x8Exx）
// ─────────────────────────────────────────────────────────────

/// F3751 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct X7Code(pub u16);

impl X7Code {
    /// 产出豁免检出——P0 最高级。
    pub const OUTPUT_EXEMPT_P0: X7Code = X7Code(0x8E01);
    /// 无证明——流程拒绝。
    pub const NO_PROOF: X7Code = X7Code(0x8E02);
    /// 到期未失效——P1 复用红线。
    pub const EXPIRED_ACTIVE: X7Code = X7Code(0x8E03);
    /// 台账分叉——归一。
    pub const LEDGER_FORK: X7Code = X7Code(0x8E04);
    /// 工具豁免缺到期——强制复述。
    pub const NO_DEADLINE: X7Code = X7Code(0x8E05);
    /// 保护优先违例——终端优先红线。
    pub const PROTECT_VIOLATE: X7Code = X7Code(0x8E06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8E01 => "检出产出豁免，P0 最高级红线（终端用户受害不可豁）。",
            0x8E02 => "豁免无技术不可行证明，流程拒绝。",
            0x8E03 => "豁免已到期未失效，P1 复用红线不通过。",
            0x8E04 => "豁免台账分叉，需归一。",
            0x8E05 => "工具豁免缺到期日，强制复述不通过。",
            0x8E06 => "保护优先原则违例，终端用户保护被降级。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 豁免类型与产出禁令
// ─────────────────────────────────────────────────────────────

/// 豁免影响面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptScope {
    /// 工具豁免（影响创作者）。
    Tool,
    /// 产出豁免（影响终端用户）——禁类。
    Output,
}

impl ExemptScope {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            ExemptScope::Tool => 0,
            ExemptScope::Output => 1,
        }
    }
}

/// 豁免条目。
#[derive(Clone, Debug)]
pub struct Exempt {
    /// 豁免 id。
    pub id: u32,
    /// 影响面。
    pub scope: ExemptScope,
    /// 技术不可行证明 id（0=无证明）。
    pub proof_id: u32,
    /// 到期时刻（逻辑时钟，0=无到期）。
    pub deadline: u64,
    /// 当前时刻。
    pub now: u64,
    /// 是否仍生效。
    pub active: bool,
}

impl Exempt {
    /// 工具豁免构造。
    pub fn tool(id: u32, proof_id: u32, deadline: u64) -> Exempt {
        Exempt {
            id,
            scope: ExemptScope::Tool,
            proof_id,
            deadline,
            now: 0,
            active: true,
        }
    }

    /// 产出豁免构造（禁类，仅供演练注入）。
    pub fn output_forbidden(id: u32) -> Exempt {
        Exempt {
            id,
            scope: ExemptScope::Output,
            proof_id: 1,
            deadline: u64::MAX,
            now: 0,
            active: true,
        }
    }

    /// 有证明。
    pub fn proven(&self) -> bool {
        self.proof_id != 0
    }

    /// 已到期。
    pub fn expired(&self) -> bool {
        self.deadline != 0 && self.now >= self.deadline
    }

    /// P0 禁令：产出豁免恒禁。
    pub fn is_p0_forbidden(&self) -> bool {
        self.scope == ExemptScope::Output
    }

    /// 工具豁免合规：有证明 + 有到期 + 未到期 + 生效。
    pub fn tool_ok(&self) -> bool {
        if self.is_p0_forbidden() {
            return false;
        }
        self.proven() && self.deadline != 0 && !self.expired() && self.active
    }

    /// 推进时钟。
    pub fn tick(&mut self, now: u64) {
        self.now = now;
        if self.expired() {
            self.active = false;
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 豁免审计器（保护优先红线）
// ─────────────────────────────────────────────────────────────

/// 豁免审计器。
#[derive(Clone, Debug, Default)]
pub struct ExemptAuditor {
    exempts: Vec<Exempt>,
    /// 保护优先断言：产出保护等级 ≥ 工具保护等级。
    protect_level_tool: u8,
    protect_level_output: u8,
}

impl ExemptAuditor {
    /// 空审计器（保护等级默认输出更高）。
    pub fn new() -> ExemptAuditor {
        ExemptAuditor {
            exempts: Vec::new(),
            protect_level_tool: 1,
            protect_level_output: 2,
        }
    }

    /// 尝试登记：产出豁免拒绝（P0）；无证明拒绝；无到期拒绝。
    /// 返回拒绝码（0=接受）。
    pub fn try_log(&mut self, e: Exempt) -> u16 {
        if e.is_p0_forbidden() {
            return X7Code::OUTPUT_EXEMPT_P0.raw();
        }
        if !e.proven() {
            return X7Code::NO_PROOF.raw();
        }
        if e.deadline == 0 {
            return X7Code::NO_DEADLINE.raw();
        }
        self.exempts.push(e);
        0
    }

    /// 演练：强行塞入产出豁免（检测路径）。
    pub fn inject_output_exempt(&mut self, e: Exempt) {
        self.exempts.push(e);
    }

    /// 扫描 P0。
    pub fn scan_p0(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.exempts.iter() {
            if e.is_p0_forbidden() && e.active {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描到期未失效。
    pub fn scan_expired_active(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.exempts.iter() {
            if e.expired() && e.active {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 保护优先合规：产出保护等级 ≥ 工具。
    pub fn protect_ok(&self) -> bool {
        self.protect_level_output >= self.protect_level_tool
    }

    /// 演练：倒挂保护等级。
    pub fn inject_protect_invert(&mut self) {
        self.protect_level_output = 0;
    }

    /// 修正保护等级。
    pub fn fix_protect(&mut self) {
        self.protect_level_tool = 1;
        self.protect_level_output = 2;
    }

    /// 推进时钟（到期失效）。
    pub fn tick_all(&mut self, now: u64) {
        for e in self.exempts.iter_mut() {
            e.tick(now);
        }
    }

    /// 生效条目数。
    pub fn active_count(&self) -> usize {
        self.exempts.iter().filter(|e| e.active).count()
    }

    /// 总条目数。
    pub fn len(&self) -> usize {
        self.exempts.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.exempts.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3751 判据集。
pub fn run_vr08k_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3751");

    // 0x8E01 产出豁免 P0
    {
        let mut a = ExemptAuditor::new();
        let code = a.try_log(Exempt::output_forbidden(1));
        out.add("0x8E01-产出豁免拒收", code == 0x8E01, "");
        out.add("0x8E01-未入库", a.is_empty(), "");
    }

    // 0x8E02 证明制
    {
        let mut a = ExemptAuditor::new();
        let e = Exempt::tool(2, 0, 100);
        let code = a.try_log(e);
        out.add("0x8E02-无证明拒收", code == 0x8E02, "");
    }

    // 0x8E05 到期强制
    {
        let mut a = ExemptAuditor::new();
        let e = Exempt::tool(3, 50, 0);
        let code = a.try_log(e);
        out.add("0x8E05-无到期拒收", code == 0x8E05, "");
    }

    // 0x8E03 到期失效
    {
        let mut a = ExemptAuditor::new();
        let e = Exempt::tool(4, 50, 100);
        a.try_log(e);
        a.tick_all(150);
        out.add("0x8E03-到期自动失效", a.active_count() == 0, "");
    }

    // 0x8E06 保护优先
    {
        let mut a = ExemptAuditor::new();
        out.add("0x8E06-默认优先合规", a.protect_ok(), "");
        a.inject_protect_invert();
        out.add("0x8E06-倒挂检出", !a.protect_ok(), "");
        a.fix_protect();
        out.add("0x8E06-修正后过", a.protect_ok(), "");
    }

    // 0x8E01 注入检测
    {
        let mut a = ExemptAuditor::new();
        a.inject_output_exempt(Exempt::output_forbidden(9));
        out.add("0x8E01-注入P0检出", a.scan_p0().len() == 1, "");
    }

    out
}
