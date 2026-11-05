//! UNX-F1513 回归与 CI 挂接（触发语义与门禁） · 0x781C0段
//!
//! 判据原文（UNX-F1513-J1 摘录）：「回归-CI 挂接语义冻结 10/10 次（随批
//! 收口触发/随域闸门触发/手动触发三档，门禁：红账阻断合并语义），触发演练
//! 10/10 次按语义执行，误触发注入 10/10 必拦」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 触发三档枚举闭合：随批收口/随域闸门/手动，档外取值在触发口即拒；
//! 2. 触发语义冻结：随批收口要求批已封口且红账为零；随域闸门要求域自检
//!    全绿；手动档恒合法——语义表是一段冻结的纯函数，不许现场改；
//! 3. 门禁语义冻结：红账非零一律阻断合并（手动触发也只告警不放行）；
//! 4. 误触发注入 10/10：十组违约事件（未封口随批触发/红批随批触发/红域
//!    闸门触发及其组合）触发口全数必拦；
//! 5. 挂接板 演练中→已封板 单向，演练记录只追加，封板后重放逐条复核；
//!    诊断码闭集 0x781C1..0x781CC。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x781C0;

/// 回归-CI 挂接诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 触发档非法。
    EKind,
    /// 批未收口不得随批触发。
    ENotSealed,
    /// 红批不得随批收口触发。
    ERedBatch,
    /// 红域不得随域闸门触发。
    EDomainRed,
    /// 门禁：红账阻断合并。
    EBlock,
    /// 重复挂接。
    EDup,
    /// 挂接超容。
    ECap,
    /// 空挂接。
    EEmpty,
    /// 已封板。
    ESealed,
    /// 未挂接的触发。
    ENoHook,
    /// 演练序乱。
    EOrder,
    /// 封板重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EKind => 0x01,
                Diag::ENotSealed => 0x02,
                Diag::ERedBatch => 0x03,
                Diag::EDomainRed => 0x04,
                Diag::EBlock => 0x05,
                Diag::EDup => 0x06,
                Diag::ECap => 0x07,
                Diag::EEmpty => 0x08,
                Diag::ESealed => 0x09,
                Diag::ENoHook => 0x0a,
                Diag::EOrder => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EKind => "触发档非法：只认随批收口/随域闸门/手动三档",
            Diag::ENotSealed => "批未收口：随批收口触发必须先封口本批",
            Diag::ERedBatch => "红批不得随批触发：先清红账再收口",
            Diag::EDomainRed => "红域不得闸门触发：域自检全绿才许触发",
            Diag::EBlock => "门禁阻断：红账非零一律不许合并，先修红",
            Diag::EDup => "重复挂接：同档只挂一条钩子，改语义走新版本",
            Diag::ECap => "挂接超容：三档已齐，多余钩子无处可挂",
            Diag::EEmpty => "空挂接：无档位的钩子不构成挂接",
            Diag::ESealed => "已封板：封板后挂接与演练拒绝，开新板",
            Diag::ENoHook => "未挂接：该档钩子不存在，先挂接再演练",
            Diag::EOrder => "演练序乱：演练按事件序 1,2,3.. 顺序执行",
            Diag::ESeal => "封板重放不符：演练结论复算不一致，板可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EKind,
    Diag::ENotSealed,
    Diag::ERedBatch,
    Diag::EDomainRed,
    Diag::EBlock,
    Diag::EDup,
    Diag::ECap,
    Diag::EEmpty,
    Diag::ESealed,
    Diag::ENoHook,
    Diag::EOrder,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x781C1..0x781CC 闭集内"
}

/// 触发三档：随批收口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    /// 随批收口触发。
    BatchClose,
    /// 随域闸门触发。
    DomainGate,
    /// 手动触发。
    Manual,
}

/// 三档闭集数组。
pub const KIND_ALL: &[TriggerKind] = &[TriggerKind::BatchClose, TriggerKind::DomainGate, TriggerKind::Manual];

impl TriggerKind {
    /// 档序号（入折叠）。
    pub fn ord(self) -> u64 {
        match self {
            TriggerKind::BatchClose => 0,
            TriggerKind::DomainGate => 1,
            TriggerKind::Manual => 2,
        }
    }

    /// 中文档名。
    pub fn name(self) -> &'static str {
        match self {
            TriggerKind::BatchClose => "随批收口",
            TriggerKind::DomainGate => "随域闸门",
            TriggerKind::Manual => "手动",
        }
    }
}

/// 一条触发事件（演练输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerEvent {
    /// 事件序（1..=N，按演练序）。
    pub seq: u16,
    /// 触发档。
    pub kind: TriggerKind,
    /// 批是否已收口。
    pub batch_sealed: bool,
    /// 域自检是否全绿。
    pub domain_green: bool,
    /// 红账条数。
    pub red_count: u32,
}

/// 触发判决（语义输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// 是否触发（发 CI 事件）。
    pub fire: bool,
    /// 是否允许合并（门禁：红账为零才许）。
    pub merge_allowed: bool,
}

/// FNV 折叠辅助。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 挂接容量（三档各一钩）。
pub const HOOKS: usize = 3;
/// 演练事件数（10/10）。
pub const DRILLS: usize = 10;

/// 冻结的触发语义：档位前置条件 + 门禁（红账阻断合并）。
/// 这是本卡的语义冻结点——任何修改都必须走版本演进，不许现场改。
pub fn semantics(e: &TriggerEvent) -> Result<Decision, u32> {
    match e.kind {
        TriggerKind::BatchClose => {
            if !e.batch_sealed {
                return Err(Diag::ENotSealed.val());
            }
            if e.red_count > 0 {
                return Err(Diag::ERedBatch.val());
            }
        }
        TriggerKind::DomainGate => {
            if !e.domain_green {
                return Err(Diag::EDomainRed.val());
            }
        }
        TriggerKind::Manual => {}
    }
    Ok(Decision { fire: true, merge_allowed: e.red_count == 0 })
}

/// 合并请求：触发判决之上的门禁执行点。
pub fn request_merge(d: Decision) -> Result<(), u32> {
    if !d.fire {
        return Err(Diag::ENoHook.val());
    }
    if !d.merge_allowed {
        return Err(Diag::EBlock.val());
    }
    Ok(())
}

/// 挂接板相位：演练中 → 已封板（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 演练中。
    Drilling,
    /// 已封板。
    Sealed,
}

/// CI 挂接板：三档钩子 + 演练记录（只追加）。
pub struct CiBoard {
    hooks: Vec<TriggerKind>,
    log: Vec<(u16, Decision)>,
    phase: Phase,
    rejects: u32,
    root: u64,
}

impl CiBoard {
    /// 空板。
    pub fn new() -> CiBoard {
        CiBoard { hooks: Vec::new(), log: Vec::new(), phase: Phase::Drilling, rejects: 0, root: 0 }
    }

    /// 挂接一档钩子（同档重复拒绝）。
    pub fn hook(&mut self, kind: TriggerKind) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.hooks.len() >= HOOKS {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if !KIND_ALL.iter().any(|k| *k == kind) {
            self.rejects += 1;
            return Err(Diag::EKind.val());
        }
        for k in &self.hooks {
            if *k == kind {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        self.hooks.push(kind);
        Ok(())
    }

    /// 已挂钩档数。
    pub fn hook_count(&self) -> usize {
        self.hooks.len()
    }

    /// 某档是否已挂。
    pub fn hooked(&self, kind: TriggerKind) -> bool {
        self.hooks.iter().any(|k| *k == kind)
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 演练记录数。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 取演练结论。
    pub fn log_at(&self, i: usize) -> Option<(u16, Decision)> {
        self.log.get(i).copied()
    }

    /// 执行一次触发演练：钩子在位 + 语义冻结 + 结论落档（只追加）。
    pub fn drill(&mut self, e: TriggerEvent) -> Result<Decision, u32> {
        if self.phase == Phase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if e.seq == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if !self.hooked(e.kind) {
            self.rejects += 1;
            return Err(Diag::ENoHook.val());
        }
        if e.seq as usize != self.log.len() + 1 {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        let d = semantics(&e)?;
        self.log.push((e.seq, d));
        Ok(d)
    }

    /// 板折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.hooks.len() as u64);
        for k in &self.hooks {
            h = fold_word(h, k.ord());
        }
        h = fold_word(h, self.log.len() as u64);
        for (seq, d) in &self.log {
            h = fold_word(h, *seq as u64);
            h = fold_word(h, d.fire as u64);
            h = fold_word(h, d.merge_allowed as u64);
        }
        h
    }

    /// 封板：三档齐 + 演练满额才许封（单向）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.hook_count() < HOOKS {
            return Err(Diag::ENoHook.val());
        }
        if self.log_len() < DRILLS {
            return Err(Diag::EOrder.val());
        }
        self.root = self.fold();
        self.phase = Phase::Sealed;
        Ok(self.root)
    }

    /// 封板根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：逐条按语义复算演练结论。
    pub fn replay(&self, events: &[TriggerEvent]) -> Result<usize, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::ESeal.val());
        }
        if events.len() != self.log.len() {
            return Err(Diag::ESeal.val());
        }
        let mut n = 0usize;
        for (i, e) in events.iter().enumerate() {
            let d = match semantics(e) {
                Ok(x) => x,
                Err(_) => return Err(Diag::ESeal.val()),
            };
            let rec = match self.log.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if rec.0 != e.seq || rec.1 != d {
                return Err(Diag::ESeal.val());
            }
            n += 1;
        }
        if self.root != self.fold() {
            return Err(Diag::ESeal.val());
        }
        Ok(n)
    }
}

/// 挂接板摘要（落档面）。
pub fn board_text(b: &CiBoard) -> String {
    format!(
        "ci hooks={} drills={} sealed={} root={:016x}",
        b.hook_count(),
        b.log_len(),
        b.phase() == Phase::Sealed,
        b.root(),
    )
}

/// 标准演练事件组（10/10 按语义执行）：
/// 绿批收口 3 次、绿域闸门 3 次、手动 2 次、手动带红账 2 次（触发但门禁阻断）。
pub fn drill_events() -> Vec<TriggerEvent> {
    let mut out = Vec::new();
    // 1..3：绿批收口。
    let mut i = 0u16;
    while i < 3 {
        out.push(TriggerEvent {
            seq: i + 1,
            kind: TriggerKind::BatchClose,
            batch_sealed: true,
            domain_green: true,
            red_count: 0,
        });
        i += 1;
    }
    // 4..6：绿域闸门。
    while i < 6 {
        out.push(TriggerEvent {
            seq: i + 1,
            kind: TriggerKind::DomainGate,
            batch_sealed: true,
            domain_green: true,
            red_count: 0,
        });
        i += 1;
    }
    // 7..8：手动（绿账）。
    while i < 8 {
        out.push(TriggerEvent {
            seq: i + 1,
            kind: TriggerKind::Manual,
            batch_sealed: false,
            domain_green: true,
            red_count: 0,
        });
        i += 1;
    }
    // 9..10：手动带红账（触发但门禁阻断合并）。
    while i < 10 {
        out.push(TriggerEvent {
            seq: i + 1,
            kind: TriggerKind::Manual,
            batch_sealed: true,
            domain_green: false,
            red_count: 2,
        });
        i += 1;
    }
    out
}

/// 执行标准演练并封板。
pub fn build_board() -> CiBoard {
    let mut b = CiBoard::new();
    let _ = b.hook(TriggerKind::BatchClose);
    let _ = b.hook(TriggerKind::DomainGate);
    let _ = b.hook(TriggerKind::Manual);
    let events = drill_events();
    for e in &events {
        let _ = b.drill(*e);
    }
    b
}

/// 误触发注入 10/10：十组违约事件，触发口全数必拦。
pub fn misfire_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    // 0..3：未封口批随批触发（绿/红域、零/非零红账组合）。
    out[0] = semantics(&TriggerEvent {
        seq: 1,
        kind: TriggerKind::BatchClose,
        batch_sealed: false,
        domain_green: true,
        red_count: 0,
    }) == Err(Diag::ENotSealed.val());
    out[1] = semantics(&TriggerEvent {
        seq: 2,
        kind: TriggerKind::BatchClose,
        batch_sealed: false,
        domain_green: false,
        red_count: 1,
    }) == Err(Diag::ENotSealed.val());
    // 2..4：红批随批触发。
    out[2] = semantics(&TriggerEvent {
        seq: 3,
        kind: TriggerKind::BatchClose,
        batch_sealed: true,
        domain_green: true,
        red_count: 1,
    }) == Err(Diag::ERedBatch.val());
    out[3] = semantics(&TriggerEvent {
        seq: 4,
        kind: TriggerKind::BatchClose,
        batch_sealed: true,
        domain_green: false,
        red_count: 3,
    }) == Err(Diag::ERedBatch.val());
    // 4..7：红域闸门触发。
    out[4] = semantics(&TriggerEvent {
        seq: 5,
        kind: TriggerKind::DomainGate,
        batch_sealed: true,
        domain_green: false,
        red_count: 0,
    }) == Err(Diag::EDomainRed.val());
    out[5] = semantics(&TriggerEvent {
        seq: 6,
        kind: TriggerKind::DomainGate,
        batch_sealed: false,
        domain_green: false,
        red_count: 0,
    }) == Err(Diag::EDomainRed.val());
    out[6] = semantics(&TriggerEvent {
        seq: 7,
        kind: TriggerKind::DomainGate,
        batch_sealed: true,
        domain_green: false,
        red_count: 5,
    }) == Err(Diag::EDomainRed.val());
    // 7..9：门禁阻断（合法触发但红账禁并）。
    let d1 = semantics(&TriggerEvent {
        seq: 8,
        kind: TriggerKind::Manual,
        batch_sealed: true,
        domain_green: true,
        red_count: 1,
    });
    out[7] = d1 == Ok(Decision { fire: true, merge_allowed: false })
        && request_merge(d1.unwrap_or(Decision { fire: false, merge_allowed: false }))
            == Err(Diag::EBlock.val());
    let d2 = semantics(&TriggerEvent {
        seq: 9,
        kind: TriggerKind::DomainGate,
        batch_sealed: false,
        domain_green: true,
        red_count: 7,
    });
    out[8] = d2 == Ok(Decision { fire: true, merge_allowed: false })
        && request_merge(d2.unwrap_or(Decision { fire: false, merge_allowed: false }))
            == Err(Diag::EBlock.val());
    // 9：未挂接档演练必拦（板只挂两档）。
    let mut b = CiBoard::new();
    let _ = b.hook(TriggerKind::BatchClose);
    let _ = b.hook(TriggerKind::DomainGate);
    out[9] = b.drill(TriggerEvent {
        seq: 1,
        kind: TriggerKind::Manual,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    }) == Err(Diag::ENoHook.val());
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x781C0
}

/// 挂接语义冻结 10/10：三档齐挂且十次演练全部按语义落档。
fn hooking_ok(b: &CiBoard) -> bool {
    if b.hook_count() != HOOKS || b.log_len() != DRILLS {
        return false;
    }
    b.hooked(TriggerKind::BatchClose)
        && b.hooked(TriggerKind::DomainGate)
        && b.hooked(TriggerKind::Manual)
}

/// 触发演练 10/10 按语义执行：结论与冻结语义逐条相等。
fn drills_ok(b: &CiBoard) -> bool {
    let events = drill_events();
    if events.len() != b.log_len() {
        return false;
    }
    let mut i = 0usize;
    while i < events.len() {
        let want = match semantics(&events[i]) {
            Ok(x) => x,
            Err(_) => return false,
        };
        let got = match b.log_at(i) {
            Some(x) => x,
            None => return false,
        };
        if got.0 != events[i].seq || got.1 != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 误触发注入 10/10 必拦。
fn misfire_ok() -> bool {
    misfire_matrix() == [true; 10]
}

/// 门禁语义：绿账放行、红账阻断。
fn gate_ok() -> bool {
    let green = semantics(&TriggerEvent {
        seq: 1,
        kind: TriggerKind::BatchClose,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    });
    let red = semantics(&TriggerEvent {
        seq: 2,
        kind: TriggerKind::Manual,
        batch_sealed: false,
        domain_green: true,
        red_count: 1,
    });
    match (green, red) {
        (Ok(g), Ok(r)) => g.merge_allowed && request_merge(g) == Ok(()) && !r.merge_allowed,
        _ => false,
    }
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_board();
    let b = build_board();
    a.fold() == b.fold() && a.fold() != 0
}

/// 十五项判据。
pub fn run_unx1513_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1513");
    let mut b = build_board();

    s.add_dyn(
        "F1513-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1513-02 三档枚举闭合",
        KIND_ALL.len() == 3
            && TriggerKind::BatchClose.ord() == 0
            && TriggerKind::Manual.ord() == 2,
        &format!(
            "n={} names={}/{}/{}",
            KIND_ALL.len(),
            TriggerKind::BatchClose.name(),
            TriggerKind::DomainGate.name(),
            TriggerKind::Manual.name()
        ),
    );

    s.add_dyn(
        "F1513-03 挂接语义冻结十次",
        hooking_ok(&b),
        &format!("hooks={} drills={}", b.hook_count(), b.log_len()),
    );

    s.add_dyn(
        "F1513-04 触发演练十次按语义",
        drills_ok(&b),
        &format!("log={}", b.log_len()),
    );

    s.add_dyn(
        "F1513-05 误触发注入十路必拦",
        misfire_ok(),
        &format!("misfire10={:?}", misfire_matrix()),
    );

    s.add_dyn(
        "F1513-06 门禁红账阻断合并",
        gate_ok(),
        &format!("gate={}", gate_ok()),
    );

    // 手动带红账：触发但合并被门禁阻断（语义面）。
    let manual_red = semantics(&TriggerEvent {
        seq: 9,
        kind: TriggerKind::Manual,
        batch_sealed: true,
        domain_green: false,
        red_count: 2,
    });
    let manual_red_ok = match manual_red {
        Ok(d) => d.fire && !d.merge_allowed && request_merge(d) == Err(Diag::EBlock.val()),
        Err(_) => false,
    };
    s.add_dyn(
        "F1513-07 手动档红账告警不放行",
        manual_red_ok,
        &format!("{:?}", manual_red),
    );

    // 重复挂接与超容拒绝。
    let mut h = CiBoard::new();
    let _ = h.hook(TriggerKind::BatchClose);
    let dup = h.hook(TriggerKind::BatchClose) == Err(Diag::EDup.val());
    let _ = h.hook(TriggerKind::DomainGate);
    let _ = h.hook(TriggerKind::Manual);
    let cap = h.hook(TriggerKind::Manual) == Err(Diag::ECap.val());
    s.add_dyn(
        "F1513-08 挂接口重复超容必拒",
        dup && cap && h.rejects() == 2 && h.hook_count() == 3,
        &format!("rejects={}", h.rejects()),
    );

    // 未挂接演练拒绝（触发前先挂接）。
    let mut bare = CiBoard::new();
    let nohook = bare.drill(TriggerEvent {
        seq: 1,
        kind: TriggerKind::BatchClose,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    }) == Err(Diag::ENoHook.val());
    s.add_dyn(
        "F1513-09 未挂接触发必拦",
        nohook && bare.rejects() == 1 && bare.log_len() == 0,
        &format!("rejects={}", bare.rejects()),
    );

    // 演练序门：跳序必拒。
    let mut o = CiBoard::new();
    let _ = o.hook(TriggerKind::BatchClose);
    let _ = o.hook(TriggerKind::DomainGate);
    let _ = o.hook(TriggerKind::Manual);
    let first = o.drill(TriggerEvent {
        seq: 1,
        kind: TriggerKind::BatchClose,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    })
    .is_ok();
    let jump = o.drill(TriggerEvent {
        seq: 3,
        kind: TriggerKind::Manual,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    }) == Err(Diag::EOrder.val());
    let empty_seq = o.drill(TriggerEvent {
        seq: 0,
        kind: TriggerKind::Manual,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    }) == Err(Diag::EEmpty.val());
    s.add_dyn(
        "F1513-10 演练序门",
        first && jump && empty_seq && o.rejects() == 2 && o.log_len() == 1,
        &format!("rejects={}", o.rejects()),
    );

    // 封板单向：封板后挂接与演练拒绝。
    let root = b.seal();
    let re_hook = b.hook(TriggerKind::Manual) == Err(Diag::ESealed.val());
    let re_drill = b.drill(TriggerEvent {
        seq: 11,
        kind: TriggerKind::Manual,
        batch_sealed: true,
        domain_green: true,
        red_count: 0,
    }) == Err(Diag::ESealed.val());
    let re_seal = b.seal() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1513-11 封板单向",
        root.is_ok() && re_hook && re_drill && re_seal && b.phase() == Phase::Sealed,
        &format!("root={:016x}", b.root()),
    );

    s.add_dyn(
        "F1513-12 封板重放逐条一致",
        b.replay(&drill_events()) == Ok(DRILLS),
        &format!("replay={:?}", b.replay(&drill_events())),
    );

    // 欠额封板拒绝。
    let mut under = CiBoard::new();
    let _ = under.hook(TriggerKind::BatchClose);
    s.add_dyn(
        "F1513-13 欠额封板拒绝",
        under.seal() == Err(Diag::ENoHook.val()) && under.phase() == Phase::Drilling,
        &format!("phase={:?}", under.phase()),
    );

    s.add_dyn(
        "F1513-14 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", build_board().fold()),
    );

    s.add_dyn(
        "F1513-15 挂接摘要非空",
        {
            let c = build_board();
            let t = board_text(&c);
            t.contains("hooks=3") && t.contains("drills=10") && t.len() > 30
        },
        &board_text(&b),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_enum_closed() {
        assert_eq!(KIND_ALL.len(), 3);
        assert_ne!(TriggerKind::BatchClose, TriggerKind::DomainGate);
        assert_ne!(TriggerKind::DomainGate, TriggerKind::Manual);
        assert_eq!(TriggerKind::BatchClose.ord(), 0);
        assert_eq!(TriggerKind::Manual.ord(), 2);
    }

    #[test]
    fn semantics_frozen() {
        let green = TriggerEvent {
            seq: 1,
            kind: TriggerKind::BatchClose,
            batch_sealed: true,
            domain_green: true,
            red_count: 0,
        };
        assert_eq!(semantics(&green), Ok(Decision { fire: true, merge_allowed: true }));
        assert_eq!(
            semantics(&TriggerEvent {
                seq: 2,
                kind: TriggerKind::BatchClose,
                batch_sealed: false,
                domain_green: true,
                red_count: 0
            }),
            Err(Diag::ENotSealed.val())
        );
        assert_eq!(
            semantics(&TriggerEvent {
                seq: 3,
                kind: TriggerKind::BatchClose,
                batch_sealed: true,
                domain_green: true,
                red_count: 1
            }),
            Err(Diag::ERedBatch.val())
        );
        assert_eq!(
            semantics(&TriggerEvent {
                seq: 4,
                kind: TriggerKind::DomainGate,
                batch_sealed: true,
                domain_green: false,
                red_count: 0
            }),
            Err(Diag::EDomainRed.val())
        );
    }

    #[test]
    fn gate_blocks_red_merge() {
        let d = semantics(&TriggerEvent {
            seq: 1,
            kind: TriggerKind::Manual,
            batch_sealed: true,
            domain_green: true,
            red_count: 4,
        })
        .unwrap();
        assert!(d.fire);
        assert!(!d.merge_allowed);
        assert_eq!(request_merge(d), Err(Diag::EBlock.val()));
        let g = semantics(&TriggerEvent {
            seq: 2,
            kind: TriggerKind::BatchClose,
            batch_sealed: true,
            domain_green: true,
            red_count: 0,
        })
        .unwrap();
        assert_eq!(request_merge(g), Ok(()));
    }

    #[test]
    fn hooks_gates() {
        let mut b = CiBoard::new();
        assert_eq!(b.hook(TriggerKind::BatchClose), Ok(()));
        assert_eq!(b.hook(TriggerKind::BatchClose), Err(Diag::EDup.val()));
        assert_eq!(b.hook(TriggerKind::DomainGate), Ok(()));
        assert_eq!(b.hook(TriggerKind::Manual), Ok(()));
        assert_eq!(b.hook(TriggerKind::Manual), Err(Diag::ECap.val()));
        assert_eq!(b.rejects(), 2);
        assert!(b.hooked(TriggerKind::Manual));
        assert!(b.hooked(TriggerKind::DomainGate));
    }

    #[test]
    fn drill_order_and_no_hook() {
        let mut b = CiBoard::new();
        let _ = b.hook(TriggerKind::BatchClose);
        assert_eq!(
            b.drill(TriggerEvent {
                seq: 1,
                kind: TriggerKind::DomainGate,
                batch_sealed: true,
                domain_green: true,
                red_count: 0
            }),
            Err(Diag::ENoHook.val())
        );
        assert_eq!(
            b.drill(TriggerEvent {
                seq: 2,
                kind: TriggerKind::BatchClose,
                batch_sealed: true,
                domain_green: true,
                red_count: 0
            }),
            Err(Diag::EOrder.val())
        );
        assert_eq!(
            b.drill(TriggerEvent {
                seq: 1,
                kind: TriggerKind::BatchClose,
                batch_sealed: true,
                domain_green: true,
                red_count: 0
            }),
            Ok(Decision { fire: true, merge_allowed: true })
        );
        assert_eq!(b.log_len(), 1);
        assert_eq!(b.rejects(), 2);
    }

    #[test]
    fn misfires_all_blocked() {
        assert_eq!(misfire_matrix(), [true; 10]);
        assert!(misfire_ok());
    }

    #[test]
    fn board_ten_drills_and_seal() {
        let mut b = build_board();
        assert_eq!(b.log_len(), 10);
        assert!(hooking_ok(&b));
        assert!(drills_ok(&b));
        // 手动带红账两条：触发但阻断。
        let d8 = b.log_at(8).unwrap().1;
        assert!(d8.fire && !d8.merge_allowed);
        let root = b.seal();
        assert!(root.is_ok());
        assert_eq!(b.phase(), Phase::Sealed);
        assert_eq!(b.replay(&drill_events()), Ok(10));
        assert_eq!(b.seal(), Err(Diag::ESealed.val()));
        assert_eq!(b.hook(TriggerKind::Manual), Err(Diag::ESealed.val()));
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EKind.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x70), "未知诊断码：不在 0x781C1..0x781CC 闭集内");
    }

    #[test]
    fn checks_all_green_fifteen() {
        let s = run_unx1513_checks();
        assert_eq!(s.len(), 15);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(board_text(&build_board()).contains("drills=10"));
    }
}
