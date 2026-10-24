//! unx342_apsilence ↔ UNX-F0342 AP 中断就绪前静默窗（A2 交接语义）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0342，UNX-A1-B18）：
//! 「AP 中断就绪前静默窗（A2 交接语义）| 240 | 已深化 |
//!  UNX-F0342-J1 AP 端中断保持静默至 A2 接管，静默期违规中断检出 100%」
//!
//! 设计要点（全部围绕 AP 启动期中断纪律的真实状态）：
//! 1. **静默窗语义**：AP 从启动到 A2 域接管期间，中断必须静默——
//!    该窗内任何中断到达都是**违规事件**（不是延迟处理而是违约），
//!    逐条检出并立案（检出 100% = 每一条到达都被账面看见）；
//! 2. **接管令牌**：A2 接管是一次显性交接（token 单调），静默窗只在
//!    接管后解除；无令牌的"自动解除"不存在（防止静默被静默解除）；
//! 3. **窗口三态机**：Silent（AP 启动起）→ Takeover（A2 令牌到达）
//!    → Armed（中断放行）；违规中断只能发生在 Silent 态——Armed 后
//!    到达的中断是正常投递，两类严格分账；
//! 4. **分账守恒**：到达中断总数 == 违规（Silent 期）+ 正常投递
//!    （Armed 期）+ 接管瞬间缓冲（Takeover 过渡窗内到达的挂起）；
//!    对不上 = 账面漏中断（检出率不到 100% 的证据）立案；
//! 5. **接管令牌单调**：token 只增不减，旧 token 重复接管拒绝
//!    （防重放接管把窗口二次打开）；
//! 6. 诊断码段 0x97 独占；no_std 零 unwrap 零 expect，tick 注入。
//!
//! 代码落点：kernel/varix/src/unxstar/（CoRun 册 A1-B18）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX342_APSILENCE_VERSION: &str = "unx342-apsilence-v1";

/// 违规账容量（append-only；满显性）。
pub const VIOLATION_CAP: usize = 64;

/// 窗口三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SilenceState {
    /// 静默窗（AP 启动起，中断必须静默）
    Silent,
    /// 接管过渡（A2 令牌已到，中断挂起缓冲）
    Takeover,
    /// 中断放行（A2 就绪面接管完成）
    Armed,
}

impl SilenceState {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            SilenceState::Silent => "静默窗",
            SilenceState::Takeover => "接管过渡",
            SilenceState::Armed => "中断放行",
        }
    }
}

/// 诊断码闭集（0xA1 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApSilenceCode {
    /// 0xA100 静默期违规中断（检出立案）
    SilentViolation,
    /// 0xA101 无令牌解除静默
    UnarmedTakeover,
    /// 0xA102 旧令牌重放接管
    TokenReplay,
    /// 0xA103 违规账满
    LedgerFull,
    /// 0xA104 分账守恒破坏（漏中断）
    Conservation,
    /// 0xA105 非法状态迁移（Armed 后回退等）
    IllegalMove,
    /// 0xA106 接管通过（成功码）
    Taken,
}

impl ApSilenceCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            ApSilenceCode::SilentViolation => 0xA100,
            ApSilenceCode::UnarmedTakeover => 0xA101,
            ApSilenceCode::TokenReplay => 0xA102,
            ApSilenceCode::LedgerFull => 0xA103,
            ApSilenceCode::Conservation => 0xA104,
            ApSilenceCode::IllegalMove => 0xA105,
            ApSilenceCode::Taken => 0xA106,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            ApSilenceCode::SilentViolation => "APSILENCE-VIOLATION",
            ApSilenceCode::UnarmedTakeover => "APSILENCE-UNARMED-TAKEOVER",
            ApSilenceCode::TokenReplay => "APSILENCE-TOKEN-REPLAY",
            ApSilenceCode::LedgerFull => "APSILENCE-LEDGER-FULL",
            ApSilenceCode::Conservation => "APSILENCE-CONSERVATION",
            ApSilenceCode::IllegalMove => "APSILENCE-ILLEGAL-MOVE",
            ApSilenceCode::Taken => "APSILENCE-TAKEN",
        }
    }

    /// 段独占双写互斥（0xA1 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0xA100 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            ApSilenceCode::SilentViolation => "静默期违规中断：A2 接管前 AP 端不得有中断，逐条检出立案",
            ApSilenceCode::UnarmedTakeover => "无令牌解除静默：接管必须携带 A2 令牌，无令牌解除即静默被静默解除",
            ApSilenceCode::TokenReplay => "旧令牌重放：接管令牌单调，旧令牌不得二次接管（防窗口重开）",
            ApSilenceCode::LedgerFull => "违规账满：无法记录本次违规，按显性失败处理",
            ApSilenceCode::Conservation => "分账守恒破坏：到达中断数对不上分账和，存在漏中断（检出率 <100% 的证据）",
            ApSilenceCode::IllegalMove => "非法状态迁移：三态机单向链被破坏",
            ApSilenceCode::Taken => "A2 接管通过：令牌登记、窗口解除、分账全平",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx342_apsilence.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 静默窗错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApSilenceError {
    SilentViolation { vector: u8, tick: u64 },
    UnarmedTakeover,
    TokenReplay { got: u64, want: u64 },
    LedgerFull { cap: usize },
    Conservation { arrived: u64, sum: u64 },
    IllegalMove { from: SilenceState },
}

impl ApSilenceError {
    /// 映射诊断码。
    pub const fn code(self) -> ApSilenceCode {
        match self {
            ApSilenceError::SilentViolation { .. } => ApSilenceCode::SilentViolation,
            ApSilenceError::UnarmedTakeover => ApSilenceCode::UnarmedTakeover,
            ApSilenceError::TokenReplay { .. } => ApSilenceCode::TokenReplay,
            ApSilenceError::LedgerFull { .. } => ApSilenceCode::LedgerFull,
            ApSilenceError::Conservation { .. } => ApSilenceCode::Conservation,
            ApSilenceError::IllegalMove { .. } => ApSilenceCode::IllegalMove,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 违规账单条（append-only；逐条违规都有账）。
#[derive(Clone, Copy, Debug)]
pub struct ViolationEntry {
    /// 中断向量
    pub vector: u8,
    /// 到达 tick
    pub tick: u64,
    /// 违规时窗口态（应恒为 Silent）
    pub at_state: SilenceState,
}

/// AP 静默窗管理器。
#[derive(Clone, Debug)]
pub struct ApSilenceWindow {
    pub state: SilenceState,
    /// 接管令牌（单调递增；0 = 未接管）
    token: u64,
    /// 违规账
    violations: Vec<ViolationEntry>,
    /// 到达中断总数（分账真源）
    arrived: u64,
    /// 正常投递数（Armed 期）
    delivered: u64,
    /// 接管过渡期挂起数
    buffered: u64,
}

impl ApSilenceWindow {
    /// 新窗（AP 启动即静默）。
    pub const fn new() -> Self {
        ApSilenceWindow {
            state: SilenceState::Silent,
            token: 0,
            violations: Vec::new(),
            arrived: 0,
            delivered: 0,
            buffered: 0,
        }
    }

    /// 当前令牌。
    pub const fn current_token(&self) -> u64 {
        self.token
    }

    /// 中断到达（逐条检视：Silent 期 = 违规立案；Takeover 期挂起；
    /// Armed 期正常投递）。
    pub fn irq_arrive(&mut self, vector: u8, tick: u64) -> Result<(), ApSilenceError> {
        self.arrived = self.arrived.saturating_add(1);
        match self.state {
            SilenceState::Silent => {
                // 违规：逐条检出（100% 检出的实现本体 = 每条到达必经此门）
                if self.violations.len() >= VIOLATION_CAP {
                    return Err(ApSilenceError::LedgerFull { cap: VIOLATION_CAP });
                }
                self.violations.push(ViolationEntry {
                    vector,
                    tick,
                    at_state: SilenceState::Silent,
                });
                Err(ApSilenceError::SilentViolation { vector, tick })
            }
            SilenceState::Takeover => {
                self.buffered = self.buffered.saturating_add(1);
                Ok(())
            }
            SilenceState::Armed => {
                self.delivered = self.delivered.saturating_add(1);
                Ok(())
            }
        }
    }

    /// A2 接管（令牌单调；无令牌/旧令牌拒绝）。
    pub fn takeover(&mut self, new_token: u64) -> Result<(), ApSilenceError> {
        if new_token == 0 {
            return Err(ApSilenceError::UnarmedTakeover);
        }
        if new_token <= self.token {
            return Err(ApSilenceError::TokenReplay { got: new_token, want: self.token + 1 });
        }
        match self.state {
            SilenceState::Silent => {
                self.token = new_token;
                self.state = SilenceState::Takeover;
                Ok(())
            }
            SilenceState::Takeover => Err(ApSilenceError::IllegalMove { from: SilenceState::Takeover }),
            SilenceState::Armed => Err(ApSilenceError::IllegalMove { from: SilenceState::Armed }),
        }
    }

    /// 接管完成（Takeover → Armed；必须已有令牌——静默不得无令牌解除）。
    pub fn arm(&mut self) -> Result<(), ApSilenceError> {
        match self.state {
            SilenceState::Takeover if self.token > 0 => {
                self.state = SilenceState::Armed;
                Ok(())
            }
            SilenceState::Takeover => Err(ApSilenceError::UnarmedTakeover),
            SilenceState::Silent => Err(ApSilenceError::UnarmedTakeover),
            SilenceState::Armed => Err(ApSilenceError::IllegalMove { from: SilenceState::Armed }),
        }
    }

    /// 违规条数。
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    /// 正常投递数。
    pub const fn delivered_count(&self) -> u64 {
        self.delivered
    }

    /// 挂起缓冲数。
    pub const fn buffered_count(&self) -> u64 {
        self.buffered
    }

    /// 分账守恒（J1 检出 100% 的账面口径）：
    /// arrived == violations + delivered + buffered + Armed 前挂起已入 buffered。
    pub fn verify_conservation(&self) -> Result<(), ApSilenceError> {
        let v = self.violations.len() as u64;
        let sum = v
            .checked_add(self.delivered)
            .and_then(|x| x.checked_add(self.buffered));
        match sum {
            Some(s) if s == self.arrived => Ok(()),
            Some(s) => Err(ApSilenceError::Conservation { arrived: self.arrived, sum: s }),
            None => Err(ApSilenceError::Conservation { arrived: self.arrived, sum: u64::MAX }),
        }
    }

    /// 违规全可追（每条违规带向量与 tick）。
    pub fn violation_at(&self, idx: usize) -> Option<ViolationEntry> {
        self.violations.get(idx).copied()
    }

    /// 窗口快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        b.push(self.state as u8);
        b.extend_from_slice(&self.token.to_le_bytes());
        b.extend_from_slice(&self.arrived.to_le_bytes());
        b.extend_from_slice(&self.delivered.to_le_bytes());
        b.extend_from_slice(&self.buffered.to_le_bytes());
        for v in self.violations.iter() {
            b.push(v.vector);
            b.extend_from_slice(&v.tick.to_le_bytes());
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in b.iter() {
            h ^= *x as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 域自检（`run_unx342_checks`）：J1 判据逐条可失败断言。
pub fn run_unx342_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx342");

    // C342-01 静默期违规中断逐条检出（J1 核心：检出 100%）
    {
        let mut w = ApSilenceWindow::new();
        let mut detected = 0;
        for i in 0..5u64 {
            if matches!(w.irq_arrive(0x20 + i as u8, i), Err(ApSilenceError::SilentViolation { .. })) {
                detected += 1;
            }
        }
        let ok = detected == 5 && w.violation_count() == 5 && w.state == SilenceState::Silent;
        s.add("C342-01-静默期违规逐条检出", ok, if ok { "" } else { "违规中断漏检" });
    }

    // C342-02 违规账逐条可追（向量 + tick）
    {
        let mut w = ApSilenceWindow::new();
        let _ = w.irq_arrive(0x80, 42);
        let e = w.violation_at(0);
        let ok = match e {
            Some(v) => v.vector == 0x80 && v.tick == 42 && v.at_state == SilenceState::Silent,
            None => false,
        };
        s.add("C342-02-违规账逐条可追", ok, if ok { "" } else { "违规账缺失现场" });
    }

    // C342-03 无令牌解除静默拒绝（静默不得被静默解除）
    {
        let mut w = ApSilenceWindow::new();
        let r1 = w.takeover(0);
        let r2 = w.arm(); // Silent 态直接 arm 也拒
        let ok = matches!(r1, Err(ApSilenceError::UnarmedTakeover))
            && matches!(r2, Err(ApSilenceError::UnarmedTakeover))
            && w.state == SilenceState::Silent;
        s.add("C342-03-无令牌不得解除", ok, if ok { "" } else { "静默被静默解除" });
    }

    // C342-04 旧令牌重放拒绝（单调令牌）
    {
        let mut w = ApSilenceWindow::new();
        let _ = w.takeover(7);
        let _ = w.arm();
        let r = w.takeover(7); // 同 token 重放
        let r2 = w.takeover(5); // 更旧
        let ok = matches!(r, Err(ApSilenceError::TokenReplay { got: 7, want: 8 }))
            && matches!(r2, Err(ApSilenceError::TokenReplay { got: 5, want: 8 }));
        s.add("C342-04-旧令牌重放拒绝", ok, if ok { "" } else { "重放接管被放行" });
    }

    // C342-05 接管全链：令牌 → 过渡（挂起缓冲）→ 放行（J1 正面）
    {
        let mut w = ApSilenceWindow::new();
        let t = w.takeover(1);
        // 过渡期到达挂起
        let b1 = w.irq_arrive(0x30, 10);
        let b2 = w.irq_arrive(0x31, 11);
        let a = w.arm();
        // 放行期正常投递
        let d = w.irq_arrive(0x32, 12);
        let ok = t.is_ok()
            && b1.is_ok()
            && b2.is_ok()
            && a.is_ok()
            && d.is_ok()
            && w.buffered_count() == 2
            && w.delivered_count() == 1
            && w.state == SilenceState::Armed
            && w.verify_conservation().is_ok();
        s.add("C342-05-接管全链分账全平", ok, if ok { "" } else { "接管链分账失平" });
    }

    // C342-06 三态机非法迁移拒绝（Silent 直 arm / Armed 再 takeover）
    {
        let mut w1 = ApSilenceWindow::new();
        let r1 = w1.arm();
        let mut w2 = ApSilenceWindow::new();
        let _ = w2.takeover(3);
        let _ = w2.arm();
        let r2 = w2.takeover(4);
        let ok = matches!(r1, Err(ApSilenceError::UnarmedTakeover))
            && matches!(r2, Err(ApSilenceError::IllegalMove { .. }));
        s.add("C342-06-非法迁移拒绝", ok, if ok { "" } else { "非法迁移被放行" });
    }

    // C342-07 分账守恒破坏检出（漏中断证据）
    {
        // 构造：violations+delivered+buffered 与 arrived 不平
        let mut w = ApSilenceWindow::new();
        let _ = w.takeover(1);
        let _ = w.arm();
        let _ = w.irq_arrive(0x40, 1);
        let _ = w.irq_arrive(0x41, 2);
        // 手工破坏 delivered（模拟账面漏记）
        w.delivered = w.delivered.saturating_sub(1);
        let r = w.verify_conservation();
        let ok = matches!(r, Err(ApSilenceError::Conservation { arrived: 2, sum: 1 }));
        s.add("C342-07-漏中断守恒立案", ok, if ok { "" } else { "漏中断未被检出" });
    }

    // C342-08 违规账满显性拒（不静默覆盖历史）
    {
        let mut w = ApSilenceWindow::new();
        let mut overflow = false;
        for i in 0..(VIOLATION_CAP + 4) {
            if matches!(w.irq_arrive((i % 256) as u8, i as u64), Err(ApSilenceError::LedgerFull { .. })) {
                overflow = true;
            }
        }
        let ok = overflow && w.violation_count() == VIOLATION_CAP;
        s.add("C342-08-违规账满显性拒", ok, if ok { "" } else { "违规账被静默覆盖" });
    }

    // C342-09 违规不改变窗口态（检出后窗仍 Silent——A2 未接管前不解除）
    {
        let mut w = ApSilenceWindow::new();
        let _ = w.irq_arrive(0x21, 1);
        let ok = w.state == SilenceState::Silent && w.current_token() == 0;
        s.add_dyn("C342-09-违规不解除静默", ok, if ok { "" } else { "违规后静默被意外解除" });
    }

    // C342-10 快照指纹确定且态敏感
    {
        let mut w1 = ApSilenceWindow::new();
        let mut w2 = ApSilenceWindow::new();
        let f1a = w1.fingerprint();
        let f1b = w1.fingerprint();
        let _ = w2.takeover(1);
        let f2 = w2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C342-10-快照指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C342-11 诊断码 0xA1 段独占
    {
        let codes = [
            ApSilenceCode::SilentViolation,
            ApSilenceCode::UnarmedTakeover,
            ApSilenceCode::TokenReplay,
            ApSilenceCode::LedgerFull,
            ApSilenceCode::Conservation,
            ApSilenceCode::IllegalMove,
            ApSilenceCode::Taken,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C342-11-诊断码 0xA1 段独占", all, if all { "" } else { "码值越出 0xA1 段" });
    }

    s
}
