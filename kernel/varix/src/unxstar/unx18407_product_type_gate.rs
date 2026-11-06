//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18407 · wProductType（工作站/域控/服务器）语义
//! · 0x7E180 段 · 批 01 · 配额 340
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18407-J1 三类 ProductType 判定与 bottle 档案（E1 接口）
//!   联动断言全过，越权覆写注入检出。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 三类判定冻结（Windows 口径）：VER_NT_WORKSTATION=1 /
//!     VER_NT_DOMAIN_CONTROLLER=2 / VER_NT_SERVER=3；0=未定（域内保留）；
//!   - 证据派生规则：域控必须 server_capable ∧ is_dc_flag；仅 server_capable
//!     → 服务器；两者皆无 → 工作站；is_dc 但无服务器能力 = 证据冲突 → 拒；
//!   - bottle 档案联动（E1 接口）：三张冻结 bottle 档案（工作站瓶/域控瓶/
//!     服务器瓶）逐张过门，判定值与档案预期逐一相等（联动断言全过）；
//!   - 越权覆写闸：覆写请求按权限分级——Admin 才可受理；Operator/Guest
//!     越权覆写必拒并记账（J1 注入检出）；终批还需证据一致（同一证据
//!     只能派生出同一个型，Admin 亦不得违证）；
//!   - 状态机（每瓶）：Unbound → Bound → OverwriteRequested →
//!     Resolved 单向；Unbound 覆写 = 越级，拒；
//!   - 账本（双方程守恒）：绑定账 bind_tries = binds + 证据拒 + 主张拒 +
//!     容量拒；覆写账 ow_requests = 批准 + 违证拒 + 权限拒 + 越级拒；
//!   - 容量：瓶表 8 槽定容，满即拒；瓶号 0 保留（非法 id）；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] PtCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [数据结构] ProductType / Authority / BottleProfile（E1 接口投影）
//!   [解析器]   derive_type（证据派生）+ 冻结档案表（三行）
//!   [状态机]   BottlePhase 四态单向 + BottleGate
//!   [账本]     BottleLedger（双方程守恒核算）
//!   [注入核]   四类违约 ×10 轮（越权覆写/证据冲突/越级请求/伪造直通）
//!   [收口]     golden_run + run_unx18407_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E180，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E180;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_NT_WORKSTATION（冻结）。
pub const VER_NT_WORKSTATION: u8 = 1;
/// VER_NT_DOMAIN_CONTROLLER（冻结）。
pub const VER_NT_DOMAIN_CONTROLLER: u8 = 2;
/// VER_NT_SERVER（冻结）。
pub const VER_NT_SERVER: u8 = 3;
/// 瓶表容量（8 槽定容）。
pub const BOTTLE_CAP: usize = 8;
/// 瓶号保留值（0 号非法）。
pub const BOTTLE_ID_RESERVED: u32 = 0;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtCode {
    /// 瓶号非法（0 保留号）。
    BottleIdReserved,
    /// 瓶表满 / 瓶号重复（容量耗尽）。
    BottleExhausted,
    /// 证据冲突（is_dc 但无服务器能力）。
    EvidenceConflict,
    /// 主张与证据派生不符（伪造直通）。
    ClaimMismatch,
    /// 越权覆写（Operator/Guest 企图覆写）。
    OverwriteForbidden,
    /// 越级覆写（Unbound 直接覆写请求）。
    PhaseSkipped,
    /// 覆写违证（Admin 也不得违背证据）。
    OverwriteEvidence,
    /// 瓶未绑定（读档/覆写时槽空）。
    BottleUnbound,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 联动断言失败（档案预期与判定不一致）。
    LinkageBroken,
    /// 覆写受理账不一致（受理数与终态账不闭合）。
    OverwriteLedgerBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl PtCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PtCode::BottleIdReserved => "瓶号非法：0 号保留",
            PtCode::BottleExhausted => "瓶表满或瓶号重复：容量耗尽",
            PtCode::EvidenceConflict => "证据冲突：域控旗标无服务器能力",
            PtCode::ClaimMismatch => "主张与证据不符：伪造直通",
            PtCode::OverwriteForbidden => "越权覆写：权限不足",
            PtCode::PhaseSkipped => "越级覆写：瓶未绑定或状态越级",
            PtCode::OverwriteEvidence => "覆写违证：同一证据只派生同型",
            PtCode::BottleUnbound => "瓶未绑定：槽空",
            PtCode::LedgerBroken => "绑定账守恒破坏",
            PtCode::OverwriteLedgerBroken => "覆写账守恒破坏：受理≠批准+拒",
            PtCode::LinkageBroken => "联动断言失败：档案预期与判定不一致",
            PtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PtCode; 12] = [
        PtCode::BottleIdReserved,
        PtCode::BottleExhausted,
        PtCode::EvidenceConflict,
        PtCode::ClaimMismatch,
        PtCode::OverwriteForbidden,
        PtCode::PhaseSkipped,
        PtCode::OverwriteEvidence,
        PtCode::BottleUnbound,
        PtCode::LedgerBroken,
        PtCode::OverwriteLedgerBroken,
        PtCode::LinkageBroken,
        PtCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PtCode::ALL.len() {
        if PtCode::ALL[i].code() == code {
            return PtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E180 段闭集内"
}

// ── 数据结构 ────────────────────────────────────────────────────
/// 产品型（Windows 口径 + 域内未定档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProductType {
    /// 未定（0，域内保留档）。
    Undetermined,
    /// 工作站（VER_NT_WORKSTATION=1）。
    Workstation,
    /// 域控（VER_NT_DOMAIN_CONTROLLER=2）。
    DomainController,
    /// 服务器（VER_NT_SERVER=3）。
    Server,
}

impl ProductType {
    /// Windows 口径整数值。
    pub fn val(self) -> u8 {
        match self {
            ProductType::Undetermined => 0,
            ProductType::Workstation => VER_NT_WORKSTATION,
            ProductType::DomainController => VER_NT_DOMAIN_CONTROLLER,
            ProductType::Server => VER_NT_SERVER,
        }
    }
    /// 整数 → 型（域外 None）。
    pub fn from_val(v: u8) -> Option<ProductType> {
        match v {
            0 => Some(ProductType::Undetermined),
            1 => Some(ProductType::Workstation),
            2 => Some(ProductType::DomainController),
            3 => Some(ProductType::Server),
            _ => None,
        }
    }
}

/// 权限档（三级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Authority {
    /// 访客（0）。
    Guest,
    /// 操作员（1）。
    Operator,
    /// 管理员（2）。
    Admin,
}

/// bottle 档案（E1 接口投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BottleProfile {
    /// 瓶号（非 0）。
    pub bottle_id: u32,
    /// 主张型。
    pub claimed_type: ProductType,
    /// 证据旗标：域控角色。
    pub is_dc_flag: bool,
    /// 证据旗标：服务器能力。
    pub server_capable: bool,
    /// 登记权限（绑定/覆写主体的权限档）。
    pub authority: Authority,
}

/// 证据派生：两旗标 → 型。冲突（is_dc ∧ ¬server_capable）→ Err。
pub fn derive_type(is_dc: bool, server_capable: bool) -> Result<ProductType, u32> {
    if is_dc {
        if server_capable {
            Ok(ProductType::DomainController)
        } else {
            Err(PtCode::EvidenceConflict.code())
        }
    } else if server_capable {
        Ok(ProductType::Server)
    } else {
        Ok(ProductType::Workstation)
    }
}

// ── 冻结 bottle 档案表（E1 接口联动三张） ───────────────────────
/// 冻结档案行（档案 + 预期判定）。
pub struct ExpectedBottle {
    pub profile: BottleProfile,
    pub expect: ProductType,
}

/// 第 i 张档案（越界 None）。
pub fn frozen_bottle(i: usize) -> Option<ExpectedBottle> {
    match i {
        0 => Some(ExpectedBottle {
            profile: BottleProfile {
                bottle_id: 1,
                claimed_type: ProductType::Workstation,
                is_dc_flag: false,
                server_capable: false,
                authority: Authority::Admin,
            },
            expect: ProductType::Workstation,
        }),
        1 => Some(ExpectedBottle {
            profile: BottleProfile {
                bottle_id: 2,
                claimed_type: ProductType::DomainController,
                is_dc_flag: true,
                server_capable: true,
                authority: Authority::Admin,
            },
            expect: ProductType::DomainController,
        }),
        2 => Some(ExpectedBottle {
            profile: BottleProfile {
                bottle_id: 3,
                claimed_type: ProductType::Server,
                is_dc_flag: false,
                server_capable: true,
                authority: Authority::Admin,
            },
            expect: ProductType::Server,
        }),
        _ => None,
    }
}

// ── 状态机（每瓶四态单向） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BottlePhase {
    /// 未绑定。
    Unbound,
    /// 已绑定（证据派生通过，型已定）。
    Bound,
    /// 覆写请求已受理（待批）。
    OverwriteRequested,
    /// 覆写已终决（批准或违证拒）。
    Resolved,
}

/// 单瓶槽。
#[derive(Clone, Copy, Debug)]
pub struct BottleSlot {
    pub id: u32,
    pub phase: BottlePhase,
    pub bound_type: Option<ProductType>,
    pub requested_type: Option<ProductType>,
    pub authority: Authority,
}

/// 瓶门：绑定 → 覆写受理 → 终批，全程权限与证据双闸。
pub struct BottleGate {
    slots: [Option<BottleSlot>; BOTTLE_CAP],
    pub ledger: BottleLedger,
}

/// 瓶账本（双方程守恒）。
#[derive(Clone, Copy, Debug)]
pub struct BottleLedger {
    /// 绑定尝试总数。
    pub bind_tries: u32,
    /// 成功绑定数。
    pub binds: u32,
    /// 绑定期证据冲突拒。
    pub evidence_rej: u32,
    /// 绑定期主张失配拒（伪造直通）。
    pub claim_rej: u32,
    /// 容量/保留号/重复号拒。
    pub cap_rej: u32,
    /// 覆写请求受理成功数。
    pub ow_requests: u32,
    /// 权限拒（越权覆写检出）。
    pub ow_forbidden: u32,
    /// 越级/槽空拒。
    pub ow_phase: u32,
    /// 终批成功数。
    pub approves: u32,
    /// 终批违证拒。
    pub ow_evidence: u32,
}

impl BottleLedger {
    pub fn new() -> BottleLedger {
        BottleLedger {
            bind_tries: 0,
            binds: 0,
            evidence_rej: 0,
            claim_rej: 0,
            cap_rej: 0,
            ow_requests: 0,
            ow_forbidden: 0,
            ow_phase: 0,
            approves: 0,
            ow_evidence: 0,
        }
    }
    /// 绑定账守恒：尝试 = 成功 + 证据拒 + 主张拒 + 容量拒。
    pub fn bind_conserve(&self) -> bool {
        self.bind_tries == self.binds + self.evidence_rej + self.claim_rej + self.cap_rej
    }
    /// 覆写账守恒：受理 = 批准 + 违证拒 + 权限拒 + 越级拒。
    pub fn overwrite_conserve(&self) -> bool {
        self.ow_requests == self.approves + self.ow_evidence + self.ow_forbidden + self.ow_phase
    }
    /// 双方程守恒。
    pub fn conserve(&self) -> bool {
        self.bind_conserve() && self.overwrite_conserve()
    }
}

impl BottleGate {
    pub fn new() -> BottleGate {
        BottleGate { slots: [None; BOTTLE_CAP], ledger: BottleLedger::new() }
    }

    /// 找空槽（满返回 None）。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < BOTTLE_CAP {
            if self.slots[i].is_none() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 瓶号 → 槽位（未绑 None）。
    fn pos_of(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < BOTTLE_CAP {
            match self.slots[i] {
                Some(ref s) => {
                    if s.id == id {
                        return Some(i);
                    }
                }
                None => {}
            }
            i += 1;
        }
        None
    }

    /// 读瓶（只读；越界/未绑 None）。
    pub fn bottle(&self, id: u32) -> Option<BottleSlot> {
        match self.pos_of(id) {
            Some(p) => self.slots[p],
            None => None,
        }
    }

    /// 绑定一瓶：瓶号守卫 + 容量守卫 + 证据派生 + 主张对拍。
    /// 主张与派生不符 = 伪造直通 → 拒并记账。
    pub fn bind(&mut self, p: &BottleProfile) -> Result<ProductType, u32> {
        self.ledger.bind_tries += 1;
        // 瓶号守卫（0 号保留）。
        if p.bottle_id == BOTTLE_ID_RESERVED {
            self.ledger.cap_rej += 1;
            return Err(PtCode::BottleIdReserved.code());
        }
        // 重复号守卫。
        if self.pos_of(p.bottle_id).is_some() {
            self.ledger.cap_rej += 1;
            return Err(PtCode::BottleExhausted.code());
        }
        // 容量守卫。
        let slot = match self.free_slot() {
            Some(s) => s,
            None => {
                self.ledger.cap_rej += 1;
                return Err(PtCode::BottleExhausted.code());
            }
        };
        // 证据派生（冲突即拒）。
        let derived = match derive_type(p.is_dc_flag, p.server_capable) {
            Ok(t) => t,
            Err(_) => {
                self.ledger.evidence_rej += 1;
                return Err(PtCode::EvidenceConflict.code());
            }
        };
        // 主张对拍（伪造直通防线）。
        if p.claimed_type != derived {
            self.ledger.claim_rej += 1;
            return Err(PtCode::ClaimMismatch.code());
        }
        self.slots[slot] = Some(BottleSlot {
            id: p.bottle_id,
            phase: BottlePhase::Bound,
            bound_type: Some(derived),
            requested_type: None,
            authority: p.authority,
        });
        self.ledger.binds += 1;
        Ok(derived)
    }

    /// 覆写请求受理（Bound 态才受理；Unbound/越级拒）。
    pub fn request_overwrite(
        &mut self,
        id: u32,
        new_type: ProductType,
        who: Authority,
    ) -> Result<(), u32> {
        let pos = match self.pos_of(id) {
            Some(p) => p,
            None => {
                self.ledger.ow_phase += 1;
                return Err(PtCode::BottleUnbound.code());
            }
        };
        let cur_phase = match self.slots[pos] {
            Some(s) => s.phase,
            None => {
                self.ledger.ow_phase += 1;
                return Err(PtCode::BottleUnbound.code());
            }
        };
        // 越级闸：仅 Bound 态可请求（OverwriteRequested 重复请求也拒）。
        if cur_phase != BottlePhase::Bound {
            self.ledger.ow_phase += 1;
            return Err(PtCode::PhaseSkipped.code());
        }
        // 权限闸：Operator/Guest 一律越权拒。
        if who != Authority::Admin {
            self.ledger.ow_forbidden += 1;
            return Err(PtCode::OverwriteForbidden.code());
        }
        // 受理（Bound → OverwriteRequested）。
        match self.slots[pos] {
            Some(ref mut s) => {
                s.requested_type = Some(new_type);
                s.phase = BottlePhase::OverwriteRequested;
            }
            None => {
                self.ledger.ow_phase += 1;
                return Err(PtCode::BottleUnbound.code());
            }
        }
        self.ledger.ow_requests += 1;
        Ok(())
    }

    /// 终批：OverwriteRequested → Resolved；新类型必须与证据一致。
    pub fn resolve_overwrite(&mut self, id: u32, approve: bool) -> Result<ProductType, u32> {
        let pos = match self.pos_of(id) {
            Some(p) => p,
            None => {
                self.ledger.ow_phase += 1;
                return Err(PtCode::BottleUnbound.code());
            }
        };
        let cur = match self.slots[pos] {
            Some(s) => s,
            None => {
                self.ledger.ow_phase += 1;
                return Err(PtCode::BottleUnbound.code());
            }
        };
        if cur.phase != BottlePhase::OverwriteRequested {
            self.ledger.ow_phase += 1;
            return Err(PtCode::PhaseSkipped.code());
        }
        let req = match cur.requested_type {
            Some(t) => t,
            None => return Err(PtCode::OverwriteEvidence.code()),
        };
        let bound = match cur.bound_type {
            Some(t) => t,
            None => return Err(PtCode::BottleUnbound.code()),
        };
        // 证据闸：请求型 ≠ 绑定型 = 违证（同证据只派生同型）。
        if req != bound {
            self.ledger.ow_evidence += 1;
            return Err(PtCode::OverwriteEvidence.code());
        }
        match self.slots[pos] {
            Some(ref mut s) => {
                s.phase = BottlePhase::Resolved;
            }
            None => return Err(PtCode::BottleUnbound.code()),
        }
        if approve {
            self.ledger.approves += 1;
            Ok(req)
        } else {
            self.ledger.ow_evidence += 1;
            Err(PtCode::OverwriteEvidence.code())
        }
    }
}

// ── 注入核（四类违约 × 10 轮） ──────────────────────────────────
/// 四类注入 rounds 轮：越权覆写 / 证据冲突 / 越级请求 / 伪造直通。
/// 返回独立计账的账本（各拒账逐轮累计，漏检即 Err）。
pub fn inject_rounds(rounds: u32) -> Result<BottleLedger, u32> {
    let mut led = BottleLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类1 越权覆写：Operator/Guest 轮换企图覆写已绑瓶 → 必拒。
        let mut gate1 = BottleGate::new();
        let _ = gate1.bind(&BottleProfile {
            bottle_id: 1,
            claimed_type: ProductType::Server,
            is_dc_flag: false,
            server_capable: true,
            authority: Authority::Admin,
        });
        let who = match r % 2 {
            0 => Authority::Operator,
            _ => Authority::Guest,
        };
        match gate1.request_overwrite(1, ProductType::Workstation, who) {
            Err(c) => {
                if c != PtCode::OverwriteForbidden.code() {
                    return Err(PtCode::OverwriteForbidden.code());
                }
                led.ow_forbidden += 1;
            }
            Ok(()) => return Err(PtCode::OverwriteForbidden.code()),
        }
        // 类2 证据冲突：is_dc=true 但无服务器能力 → 必拒。
        let mut gate2 = BottleGate::new();
        match gate2.bind(&BottleProfile {
            bottle_id: 1,
            claimed_type: ProductType::DomainController,
            is_dc_flag: true,
            server_capable: false,
            authority: Authority::Admin,
        }) {
            Err(c) => {
                if c != PtCode::EvidenceConflict.code() {
                    return Err(PtCode::EvidenceConflict.code());
                }
                led.evidence_rej += 1;
            }
            Ok(_) => return Err(PtCode::EvidenceConflict.code()),
        }
        // 类3 越级请求：空门上直接覆写请求 → 必拒（槽空）。
        let mut gate3 = BottleGate::new();
        match gate3.request_overwrite(9, ProductType::Server, Authority::Admin) {
            Err(c) => {
                if c != PtCode::BottleUnbound.code() {
                    return Err(PtCode::PhaseSkipped.code());
                }
                led.ow_phase += 1;
            }
            Ok(()) => return Err(PtCode::PhaseSkipped.code()),
        }
        // 类4 伪造直通：主张型与证据派生不符 → 必拒（三方向轮换）。
        let mut gate4 = BottleGate::new();
        let (claim, dc, cap) = match r % 3 {
            0 => (ProductType::DomainController, false, false), // 工作站证据冒充域控
            1 => (ProductType::Workstation, false, true),        // 服务器证据冒充工作站
            _ => (ProductType::Server, true, true),              // 域控证据冒充服务器
        };
        match gate4.bind(&BottleProfile {
            bottle_id: 1,
            claimed_type: claim,
            is_dc_flag: dc,
            server_capable: cap,
            authority: Authority::Admin,
        }) {
            Err(c) => {
                if c != PtCode::ClaimMismatch.code() {
                    return Err(PtCode::ClaimMismatch.code());
                }
                led.claim_rej += 1;
            }
            Ok(_) => return Err(PtCode::ClaimMismatch.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）：三张冻结档案联动 + Admin 覆写链 + 注入全抓。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut gate = BottleGate::new();
    // E1 联动：三张冻结档案逐张过门，判定与预期逐一相等。
    let mut i = 0;
    while i < 3 {
        let fb = match frozen_bottle(i) {
            Some(f) => f,
            None => return Err(PtCode::LinkageBroken.code()),
        };
        let got = gate.bind(&fb.profile)?;
        if got != fb.expect {
            return Err(PtCode::LinkageBroken.code());
        }
        i += 1;
    }
    // Admin 覆写链：受理 → 终批（同证据同型，幂等覆写）。
    gate.request_overwrite(3, ProductType::Server, Authority::Admin)?;
    let resolved = gate.resolve_overwrite(3, true)?;
    if resolved != ProductType::Server {
        return Err(PtCode::OverwriteEvidence.code());
    }
    // Admin 违证覆写：请求型 ≠ 绑定型 → 必拒。
    gate.request_overwrite(1, ProductType::Server, Authority::Admin)?;
    if gate.resolve_overwrite(1, true).is_ok() {
        return Err(PtCode::OverwriteEvidence.code());
    }
    // 账双守恒（绑定 3 试 3 收；覆写 2 受理 = 1 批 + 1 违证拒）。
    if !gate.ledger.conserve() {
        return Err(PtCode::OverwriteLedgerBroken.code());
    }
    if gate.ledger.binds != 3 || gate.ledger.approves != 1 || gate.ledger.ow_evidence != 1 {
        return Err(PtCode::LedgerBroken.code());
    }
    // 注入四类 × 10 轮全抓。
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if led.ow_forbidden != JUDGE_ROUNDS
        || led.evidence_rej != JUDGE_ROUNDS
        || led.ow_phase != JUDGE_ROUNDS
        || led.claim_rej != JUDGE_ROUNDS
    {
        return Err(PtCode::ClaimMismatch.code());
    }
    Ok((gate.ledger.binds, gate.ledger.approves, led.ow_forbidden, gate.ledger.ow_evidence))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PtCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PtCode::ALL.len() {
        let v = PtCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：三张冻结档案联动全过（三类各一）。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut gate = BottleGate::new();
    let mut i = 0;
    while i < 3 {
        let fb = match frozen_bottle(i) {
            Some(f) => f,
            None => return Err(PtCode::LinkageBroken.code()),
        };
        let got = gate.bind(&fb.profile)?;
        if got != fb.expect {
            return Err(PtCode::LinkageBroken.code());
        }
        i += 1;
    }
    Ok(())
}

/// 探针 B：0 号瓶（保留号）绑定 → 必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut gate = BottleGate::new();
    gate.bind(&BottleProfile {
        bottle_id: BOTTLE_ID_RESERVED,
        claimed_type: ProductType::Workstation,
        is_dc_flag: false,
        server_capable: false,
        authority: Authority::Admin,
    })
    .map(|_| ())
}

/// 探针 C：未定档（0）从整数反查（域内保留档可达）。
pub fn probe_seq_c() -> Result<(), u32> {
    match ProductType::from_val(0) {
        Some(t) => {
            if t != ProductType::Undetermined {
                return Err(PtCode::LinkageBroken.code());
            }
            Ok(())
        }
        None => Err(PtCode::LinkageBroken.code()),
    }
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. Windows 口径整数冻结（1/2/3 三类逐一）。
    let vals_ok = ProductType::Workstation.val() == 1
        && ProductType::DomainController.val() == 2
        && ProductType::Server.val() == 3
        && ProductType::Undetermined.val() == 0;
    v.push(("J1-三类口径1/2/3冻结", vals_ok, String::from("WS=1 DC=2 SRV=3")));

    // 2. E1 联动：三张冻结档案逐张过门且判定=预期（联动断言全过）。
    let pa = probe_seq_a();
    v.push(("J1-bottle档案联动三张", pa.is_ok(), format!("{:?}", pa)));

    // 3. 越权覆写注入 10/10（Operator/Guest 轮换）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let ow = match led.as_ref() {
        Ok(l) => l.ow_forbidden,
        Err(_) => 0,
    };
    v.push(("J1-越权覆写注入10/10", ow == JUDGE_ROUNDS, format!("caught={}", ow)));

    // 4. 证据冲突注入 10/10。
    let ev = match led.as_ref() {
        Ok(l) => l.evidence_rej,
        Err(_) => 0,
    };
    v.push(("J1-证据冲突注入10/10", ev == JUDGE_ROUNDS, format!("rej={}", ev)));

    // 5. 越级请求注入 10/10。
    let ph = match led.as_ref() {
        Ok(l) => l.ow_phase,
        Err(_) => 0,
    };
    v.push(("J1-越级请求注入10/10", ph == JUDGE_ROUNDS, format!("rej={}", ph)));

    // 6. 伪造直通注入 10/10（三方向轮换冒充）。
    let cl = match led.as_ref() {
        Ok(l) => l.claim_rej,
        Err(_) => 0,
    };
    v.push(("J1-伪造直通注入10/10", cl == JUDGE_ROUNDS, format!("rej={}", cl)));

    // 7. Admin 违证覆写拒（黄金链内覆写链已验）。
    let g = golden_run();
    let ow_bad_ok = match &g {
        Ok((_b, _a, _c, evr)) => *evr >= 1,
        Err(_) => false,
    };
    v.push(("J1-Admin违证覆写拒", ow_bad_ok, String::from("同证据只派生同型")));

    // 8. 黄金全链（绑定 3 + 批准 1 + 越权检出 10）。
    let g_ok = match &g {
        Ok((b, a, c, _e)) => *b == 3 && *a == 1 && *c == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 9. 账双方程守恒（注入账各拒账逐轮独立记账）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.ow_forbidden + l.evidence_rej + l.ow_phase + l.claim_rej == 4 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类×10轮 40 拒")));

    // 10. 容量：8 槽满后第 9 瓶拒且账守恒。
    let cap_ok = {
        let mut gate = BottleGate::new();
        let mut id = 1u32;
        let mut bound = 0;
        let mut full_err = false;
        while bound < BOTTLE_CAP {
            let p = BottleProfile {
                bottle_id: id,
                claimed_type: ProductType::Workstation,
                is_dc_flag: false,
                server_capable: false,
                authority: Authority::Admin,
            };
            match gate.bind(&p) {
                Ok(_) => bound += 1,
                Err(_) => break,
            }
            id += 1;
        }
        // 第 9 瓶（bound 已 8）。
        let p9 = BottleProfile {
            bottle_id: id,
            claimed_type: ProductType::Workstation,
            is_dc_flag: false,
            server_capable: false,
            authority: Authority::Admin,
        };
        match gate.bind(&p9) {
            Err(c) => full_err = c == PtCode::BottleExhausted.code(),
            Ok(_) => full_err = false,
        }
        full_err && gate.ledger.bind_conserve() && gate.ledger.binds == BOTTLE_CAP as u32
    };
    v.push(("J1-容量8槽满拒", cap_ok, String::from("8 收 1 拒账闭合")));

    // 11. 保留号：0 号瓶拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-0号瓶保留拒", pb, format!("拒={}", pb)));

    // 12. 未定档反查（探针 C）。
    let pc = probe_seq_c().is_ok();
    v.push(("J1-未定档反查", pc, String::from("0→Undetermined")));

    // 13. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（12 码全非空中文明细）。
    let all_ok = {
        let mut ok = PtCode::ALL.len() == 12;
        let mut i = 0;
        while i < PtCode::ALL.len() {
            if PtCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18407_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18407 ProductType 三类判定门");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18407_checks();
        assert!(set.all_passed(), "UNX-F18407 判据集应全绿");
    }

    /// 正常流：三张冻结档案联动全过（三类各一）。
    #[test]
    fn test_probe_seq_a() {
        assert!(probe_seq_a().is_ok());
    }

    /// 注入：越权覆写 10/10。
    #[test]
    fn test_overwrite_forbidden_injection() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert_eq!(l.ow_forbidden, 10),
            Err(_) => assert!(false, "全抓时不应报错"),
        }
    }

    /// 注入：证据冲突 10/10。
    #[test]
    fn test_evidence_conflict_injection() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert_eq!(l.evidence_rej, 10),
            Err(_) => assert!(false, "全抓时不应报错"),
        }
    }

    /// 派生规则：三组合法派生 + 一组冲突。
    #[test]
    fn test_derive_type_rules() {
        assert_eq!(derive_type(false, false), Ok(ProductType::Workstation));
        assert_eq!(derive_type(false, true), Ok(ProductType::Server));
        assert_eq!(derive_type(true, true), Ok(ProductType::DomainController));
        match derive_type(true, false) {
            Err(c) => assert_eq!(c, PtCode::EvidenceConflict.code()),
            Ok(_) => assert!(false, "证据冲突必须拒"),
        }
    }

    /// 越级：空门覆写请求必拒（BottleUnbound）。
    #[test]
    fn test_phase_skip_rejected() {
        let mut gate = BottleGate::new();
        match gate.request_overwrite(1, ProductType::Server, Authority::Admin) {
            Err(c) => assert_eq!(c, PtCode::BottleUnbound.code()),
            Ok(()) => assert!(false, "未绑定覆写必须拒"),
        }
    }

    /// 保留号：0 号瓶拒（PtCode::BottleIdReserved）。
    #[test]
    fn test_reserved_id() {
        assert!(probe_seq_b().is_err());
    }

    /// 容量：8 槽满后第 9 瓶 BottleExhausted。
    #[test]
    fn test_capacity() {
        let mut gate = BottleGate::new();
        let mut id = 1u32;
        let mut bound = 0;
        while bound < BOTTLE_CAP {
            let p = BottleProfile {
                bottle_id: id,
                claimed_type: ProductType::Workstation,
                is_dc_flag: false,
                server_capable: false,
                authority: Authority::Admin,
            };
            assert!(gate.bind(&p).is_ok());
            bound += 1;
            id += 1;
        }
        let p9 = BottleProfile {
            bottle_id: id,
            claimed_type: ProductType::Workstation,
            is_dc_flag: false,
            server_capable: false,
            authority: Authority::Admin,
        };
        match gate.bind(&p9) {
            Err(c) => assert_eq!(c, PtCode::BottleExhausted.code()),
            Ok(_) => assert!(false, "第 9 瓶必须拒"),
        }
        assert!(gate.ledger.bind_conserve());
        assert_eq!(gate.ledger.binds, BOTTLE_CAP as u32);
    }

    /// Admin 覆写链：受理→终批成功；违证覆写拒；重复请求越级拒。
    #[test]
    fn test_admin_overwrite_chain() {
        let mut gate = BottleGate::new();
        let _ = gate.bind(&BottleProfile {
            bottle_id: 1,
            claimed_type: ProductType::Server,
            is_dc_flag: false,
            server_capable: true,
            authority: Authority::Admin,
        });
        assert!(gate.request_overwrite(1, ProductType::Server, Authority::Admin).is_ok());
        assert_eq!(gate.resolve_overwrite(1, true), Ok(ProductType::Server));
        // 终态后再次请求 = 越级拒。
        match gate.request_overwrite(1, ProductType::Server, Authority::Admin) {
            Err(c) => assert_eq!(c, PtCode::PhaseSkipped.code()),
            Ok(()) => assert!(false, "Resolved 后再请求必须拒"),
        }
    }

    /// 复现红线：golden_run 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
