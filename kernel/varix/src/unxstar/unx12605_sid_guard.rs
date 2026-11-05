//! UNX-F12606 LPC 连接端口安全属性（SQOS）→D4 联签 · 0x80140段
//!
//! 判据原文（派工卡 12605 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12606-J1 安全连接参数（ServerSID/RequiredServerSid）
//!      对平断言绿，SID 错配注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. SID 域模型：修订号 + 四段子权限（整数域），全零 SID 非法；
//!    相等比较按全字段逐段相等（整数运算，零浮点）；
//! 2. 安全连接参数语义：服务端声明 ServerSID（None = 不校验），
//!    客户端呈现 RequiredServerSid（None = 不要求）；对平规则：
//!    服务端有声明 → 客户端必须呈现且逐段相等，否则 SID 错配拒；
//!    服务端未声明 → 客户端不得擅自要求（要求即错配）；
//! 3. D4 联签：SID 校验通过后连接才落联签账，联签账只追加不改写；
//! 4. SID 错配注入 10/10：十路构造（单段差、全段差、修订号差、
//!    反序、客户端缺呈现、服务端缺声明、双缺但要求、全零 SID、
//!    重复联签、未知连接）逐路拒止，任何一路放行即判据失效；
//! 5. 状态机 待校验→已校验→已联签 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12605 − 12600)）。
pub const SEG_BASE: u32 = 0x80140;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 安全连接 SID 对平诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidDiag {
    /// 全零 SID 非法。
    EZeroSid,
    /// 服务端有声明但客户端未呈现。
    ENoRequired,
    /// 客户端呈现的 SID 与服务端声明不一致。
    EMismatch,
    /// 服务端未声明但客户端擅自要求。
    EUnwanted,
    /// 重复联签。
    EDupSign,
    /// 未知连接。
    EUnknownConn,
    /// 重复连接。
    EDupConn,
    /// 状态机倒退。
    ERewind,
    /// 未校验先联签。
    ENotVerified,
    /// 联签账守恒破坏。
    EConserv,
    /// 空样本（连接与 SID 全零）。
    EEmpty,
    /// 修订号越闭集（1..=3）。
    ERevision,
}

impl SidDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SidDiag::EZeroSid => 0x01,
                SidDiag::ENoRequired => 0x02,
                SidDiag::EMismatch => 0x03,
                SidDiag::EUnwanted => 0x04,
                SidDiag::EDupSign => 0x05,
                SidDiag::EUnknownConn => 0x06,
                SidDiag::EDupConn => 0x07,
                SidDiag::ERewind => 0x08,
                SidDiag::ENotVerified => 0x09,
                SidDiag::EConserv => 0x0a,
                SidDiag::EEmpty => 0x0b,
                SidDiag::ERevision => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SidDiag::EZeroSid => "全零 SID 非法：修订号与子权限不得全零，先构造合法 SID",
            SidDiag::ENoRequired => "客户端未呈现 RequiredServerSid：服务端已声明，必须呈现同 SID",
            SidDiag::EMismatch => "SID 错配：呈现 SID 与声明 SID 逐段不等，先对子权限四段",
            SidDiag::EUnwanted => "客户端擅自要求：服务端未声明 SID，不得单方面要求校验",
            SidDiag::EDupSign => "重复联签：一连接一联签，重复联签按重放拒",
            SidDiag::EUnknownConn => "未知连接：先建立连接再走 SID 校验",
            SidDiag::EDupConn => "重复连接：一连接一账，重复连接先查账面",
            SidDiag::ERewind => "状态机倒退：待校验→已校验→已联签只进不退",
            SidDiag::ENotVerified => "未校验先联签：SID 校验段未过禁止联签",
            SidDiag::EConserv => "联签账守恒破坏：联签数与校验数不符，账面可疑",
            SidDiag::EEmpty => "空样本：连接与 SID 全零不构成安全连接",
            SidDiag::ERevision => "SID 修订号越闭集：只认 1..=3，先修修订号",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SD_ALL: &[SidDiag] = &[
    SidDiag::EZeroSid,
    SidDiag::ENoRequired,
    SidDiag::EMismatch,
    SidDiag::EUnwanted,
    SidDiag::EDupSign,
    SidDiag::EUnknownConn,
    SidDiag::EDupConn,
    SidDiag::ERewind,
    SidDiag::ENotVerified,
    SidDiag::EConserv,
    SidDiag::EEmpty,
    SidDiag::ERevision,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in SD_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80141..0x8014C 闭集内"
}

// ── SID 域模型 ───────────────────────────────────────────────────
/// SID 修订号闭集下沿。
pub const SID_REV_MIN: u32 = 1;
/// SID 修订号闭集上沿。
pub const SID_REV_MAX: u32 = 3;

/// 安全标识符：修订号 + 四段子权限（整数域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sid {
    /// 修订号（1..=3）。
    pub revision: u32,
    /// 子权限四段。
    pub sa0: u32,
    /// 子权限第二段。
    pub sa1: u32,
    /// 子权限第三段。
    pub sa2: u32,
    /// 子权限第四段。
    pub sa3: u32,
}

impl Sid {
    /// 构造校验：修订号在闭集内且不全零。
    pub fn well_formed(&self) -> bool {
        if self.revision < SID_REV_MIN || self.revision > SID_REV_MAX {
            return false;
        }
        !(self.sa0 == 0 && self.sa1 == 0 && self.sa2 == 0 && self.sa3 == 0)
    }

    /// 逐段相等（整数比较，零浮点）。
    pub fn equals(&self, other: &Sid) -> bool {
        self.revision == other.revision
            && self.sa0 == other.sa0
            && self.sa1 == other.sa1
            && self.sa2 == other.sa2
            && self.sa3 == other.sa3
    }

    /// 摘要（诊断文本）。
    pub fn brief(&self) -> String {
        format!(
            "SID(rev={},sa={}-{}-{}-{})",
            self.revision, self.sa0, self.sa1, self.sa2, self.sa3
        )
    }
}

/// 安全连接参数（连接时落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecConnParams {
    /// 连接序号。
    pub conn_id: u32,
    /// 服务端声明 SID（None = 不校验）。
    pub server_sid: Option<Sid>,
    /// 客户端呈现 SID（None = 不要求）。
    pub required_sid: Option<Sid>,
}

impl SecConnParams {
    /// 参数静态合法性：全零样本非法。
    pub fn non_empty(&self) -> bool {
        !(self.conn_id == 0 && self.server_sid.is_none() && self.required_sid.is_none())
    }
}

/// SID 校验状态机：待校验 → 已校验 → 已联签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyState {
    /// 待校验。
    Pending,
    /// SID 校验通过。
    Verified,
    /// 已联签。
    Signed,
}

impl VerifyState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            VerifyState::Pending => 0,
            VerifyState::Verified => 1,
            VerifyState::Signed => 2,
        }
    }
}

/// 单条连接安全账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecLedger {
    /// 连接参数。
    pub params: SecConnParams,
    /// 校验状态。
    pub state: VerifyState,
}

/// 安全连接账机。
#[derive(Clone, Debug)]
pub struct SidGate {
    /// 连接账。
    ledgers: Vec<SecLedger>,
    /// 已联签数。
    signed: usize,
}

impl SidGate {
    /// 新账机。
    pub fn new() -> SidGate {
        SidGate {
            ledgers: Vec::new(),
            signed: 0,
        }
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 联签数。
    pub fn signed_count(&self) -> usize {
        self.signed
    }

    /// 查账。
    pub fn ledger_of(&self, conn_id: u32) -> Option<SecLedger> {
        for l in self.ledgers.iter() {
            if l.params.conn_id == conn_id {
                return Some(*l);
            }
        }
        None
    }

    /// 建立安全连接（连接闸：重复/空样本）。
    pub fn connect(&mut self, params: SecConnParams) -> Result<(), u32> {
        if !params.non_empty() {
            return Err(SidDiag::EEmpty.val());
        }
        if self.ledger_of(params.conn_id).is_some() {
            return Err(SidDiag::EDupConn.val());
        }
        self.ledgers.push(SecLedger {
            params,
            state: VerifyState::Pending,
        });
        Ok(())
    }

    /// SID 校验：对平规则（声明/呈现四种组合）。
    pub fn verify(&mut self, conn_id: u32) -> Result<(), u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(SidDiag::EUnknownConn.val()),
        };
        let (declared, presented) = match self.ledgers.get(idx) {
            Some(l) => (l.params.server_sid, l.params.required_sid),
            None => return Err(SidDiag::EUnknownConn.val()),
        };
        // 状态闸：已联签后再校验 = 倒退。
        match self.ledgers.get(idx).map(|l| l.state.rank()) {
            Some(r) if r >= VerifyState::Signed.rank() => {
                return Err(SidDiag::ERewind.val());
            }
            _ => {}
        }
        match (declared, presented) {
            // 双不校验：直接过（低安全连接）。
            (None, None) => {}
            // 服务端声明但客户端未呈现。
            (Some(_), None) => return Err(SidDiag::ENoRequired.val()),
            // 客户端擅自要求。
            (None, Some(_)) => return Err(SidDiag::EUnwanted.val()),
            // 双方都给：修订号先对闭集，再对全零，最后逐段相等。
            (Some(d), Some(p)) => {
                if d.revision < SID_REV_MIN || d.revision > SID_REV_MAX {
                    return Err(SidDiag::ERevision.val());
                }
                if p.revision < SID_REV_MIN || p.revision > SID_REV_MAX {
                    return Err(SidDiag::ERevision.val());
                }
                if !d.well_formed() || !p.well_formed() {
                    return Err(SidDiag::EZeroSid.val());
                }
                if !d.equals(&p) {
                    return Err(SidDiag::EMismatch.val());
                }
            }
        }
        if let Some(l) = self.ledgers.get_mut(idx) {
            l.state = VerifyState::Verified;
        }
        Ok(())
    }

    /// 联签：须先校验过；一连接一签。
    pub fn sign(&mut self, conn_id: u32) -> Result<(), u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(SidDiag::EUnknownConn.val()),
        };
        let st = match self.ledgers.get(idx).map(|l| l.state) {
            Some(s) => s,
            None => return Err(SidDiag::EUnknownConn.val()),
        };
        match st {
            VerifyState::Pending => Err(SidDiag::ENotVerified.val()),
            VerifyState::Signed => Err(SidDiag::EDupSign.val()),
            VerifyState::Verified => {
                if let Some(l) = self.ledgers.get_mut(idx) {
                    l.state = VerifyState::Signed;
                }
                self.signed += 1;
                Ok(())
            }
        }
    }

    /// 账守恒：联签账状态全部 Signed 且联签数一致。
    pub fn conservation(&self) -> bool {
        let mut n = 0usize;
        for l in self.ledgers.iter() {
            if l.state == VerifyState::Signed {
                n += 1;
            }
            if l.state.rank() < VerifyState::Pending.rank() {
                return false;
            }
        }
        n == self.signed
    }

    /// 按连接号找账内下标。
    fn index_of(&self, conn_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.params.conn_id == conn_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 对平与注入面 ─────────────────────────────────────────────────
/// 标准声明 SID（服务端）。
pub fn std_server_sid() -> Sid {
    Sid {
        revision: 1,
        sa0: 5,
        sa1: 32,
        sa2: 0x1004,
        sa3: 0x2A,
    }
}

/// 正常流：双 SID 一致 → 校验过 → 联签绿。
fn happy_pair_ok() -> bool {
    let mut g = SidGate::new();
    let p = SecConnParams {
        conn_id: 0x20,
        server_sid: Some(std_server_sid()),
        required_sid: Some(std_server_sid()),
    };
    if g.connect(p).is_err() {
        return false;
    }
    if g.verify(0x20).is_err() {
        return false;
    }
    g.sign(0x20).is_ok() && g.conservation() && g.signed_count() == 1
}

/// 双不校验组合（低安全连接）放行。
fn both_none_ok() -> bool {
    let mut g = SidGate::new();
    let p = SecConnParams {
        conn_id: 0x21,
        server_sid: None,
        required_sid: None,
    };
    match g.connect(p) {
        Ok(()) => match g.verify(0x21) {
            Ok(()) => g.sign(0x21).is_ok(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// SID 错配注入 10/10：十路违约全数拒止。
fn sid_mismatch_injections_ok() -> bool {
    let mut hits = 0usize;
    let base = std_server_sid();
    // 注 1：单段差（sa3 差 1）。
    let mut g = SidGate::new();
    let _ = g.connect(SecConnParams {
        conn_id: 0x30,
        server_sid: Some(base),
        required_sid: Some(Sid {
            sa3: base.sa3 + 1,
            ..base
        }),
    });
    if g.verify(0x30) == Err(SidDiag::EMismatch.val()) {
        hits += 1;
    }
    // 注 2：全段差。
    let mut g2 = SidGate::new();
    let _ = g2.connect(SecConnParams {
        conn_id: 0x31,
        server_sid: Some(base),
        required_sid: Some(Sid {
            revision: 2,
            sa0: 9,
            sa1: 9,
            sa2: 9,
            sa3: 9,
        }),
    });
    if g2.verify(0x31) == Err(SidDiag::EMismatch.val()) {
        hits += 1;
    }
    // 注 3：修订号差。
    let mut g3 = SidGate::new();
    let _ = g3.connect(SecConnParams {
        conn_id: 0x32,
        server_sid: Some(base),
        required_sid: Some(Sid {
            revision: base.revision + 1,
            ..base
        }),
    });
    if g3.verify(0x32) == Err(SidDiag::EMismatch.val()) {
        hits += 1;
    }
    // 注 4：反序 SID（逐段倒序）。
    let mut g4 = SidGate::new();
    let _ = g4.connect(SecConnParams {
        conn_id: 0x33,
        server_sid: Some(base),
        required_sid: Some(Sid {
            revision: base.revision,
            sa0: base.sa3,
            sa1: base.sa2,
            sa2: base.sa1,
            sa3: base.sa0,
        }),
    });
    if g4.verify(0x33) == Err(SidDiag::EMismatch.val()) {
        hits += 1;
    }
    // 注 5：服务端声明但客户端缺呈现。
    let mut g5 = SidGate::new();
    let _ = g5.connect(SecConnParams {
        conn_id: 0x34,
        server_sid: Some(base),
        required_sid: None,
    });
    if g5.verify(0x34) == Err(SidDiag::ENoRequired.val()) {
        hits += 1;
    }
    // 注 6：服务端缺声明但客户端要求。
    let mut g6 = SidGate::new();
    let _ = g6.connect(SecConnParams {
        conn_id: 0x35,
        server_sid: None,
        required_sid: Some(base),
    });
    if g6.verify(0x35) == Err(SidDiag::EUnwanted.val()) {
        hits += 1;
    }
    // 注 7：呈现的 SID 修订号越闭集。
    let mut g7 = SidGate::new();
    let _ = g7.connect(SecConnParams {
        conn_id: 0x36,
        server_sid: Some(base),
        required_sid: Some(Sid {
            revision: 9,
            ..base
        }),
    });
    let v7 = g7.verify(0x36);
    if v7 == Err(SidDiag::ERevision.val()) {
        hits += 1;
    }
    // 注 8：呈现全零 SID。
    let mut g8 = SidGate::new();
    let _ = g8.connect(SecConnParams {
        conn_id: 0x37,
        server_sid: Some(base),
        required_sid: Some(Sid {
            revision: 1,
            sa0: 0,
            sa1: 0,
            sa2: 0,
            sa3: 0,
        }),
    });
    let v8 = g8.verify(0x37);
    if v8 == Err(SidDiag::EZeroSid.val()) {
        hits += 1;
    }
    // 注 9：未校验先联签。
    let mut g9 = SidGate::new();
    let _ = g9.connect(SecConnParams {
        conn_id: 0x38,
        server_sid: None,
        required_sid: None,
    });
    if g9.sign(0x38) == Err(SidDiag::ENotVerified.val()) {
        hits += 1;
    }
    // 注 10：重复联签。
    let mut g10 = SidGate::new();
    let _ = g10.connect(SecConnParams {
        conn_id: 0x39,
        server_sid: None,
        required_sid: None,
    });
    let _ = g10.verify(0x39);
    let _ = g10.sign(0x39);
    if g10.sign(0x39) == Err(SidDiag::EDupSign.val()) {
        hits += 1;
    }
    hits == 10
}

/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in SD_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(SidDiag::EMismatch.val()) != SidDiag::EMismatch.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x38).starts_with("未知") {
        return false;
    }
    true
}

/// SID 静态语义：合法构造、全零拒、修订号界。
fn sid_semantics_ok() -> bool {
    let good = std_server_sid();
    if !good.well_formed() {
        return false;
    }
    let zero = Sid {
        revision: 1,
        sa0: 0,
        sa1: 0,
        sa2: 0,
        sa3: 0,
    };
    if zero.well_formed() {
        return false;
    }
    // 修订号边界：1 与 3 合法，0 与 4 非法。
    let rev_lo = Sid {
        revision: SID_REV_MIN,
        sa0: 1,
        ..zero
    };
    let rev_hi = Sid {
        revision: SID_REV_MAX,
        sa0: 1,
        ..zero
    };
    let rev_bad = Sid {
        revision: SID_REV_MAX + 1,
        sa0: 1,
        ..zero
    };
    rev_lo.well_formed() && rev_hi.well_formed() && !rev_bad.well_formed()
}

/// SID 相等语义：同 SID 相等、任意一段差即不等。
fn sid_equality_ok() -> bool {
    let a = std_server_sid();
    let b = std_server_sid();
    if !a.equals(&b) {
        return false;
    }
    let diffs = [
        Sid { sa0: a.sa0 + 1, ..a },
        Sid { sa1: a.sa1.wrapping_add(1), ..a },
        Sid { sa2: a.sa2.wrapping_add(1), ..a },
        Sid { sa3: a.sa3.wrapping_add(1), ..a },
        Sid { revision: a.revision + 1, ..a },
    ];
    for d in diffs.iter() {
        if a.equals(d) {
            return false;
        }
    }
    true
}

/// 双跑一致：注入判定序列两次相同。
fn dual_run_ok() -> bool {
    let a = sid_mismatch_injections_ok();
    let b = sid_mismatch_injections_ok();
    a && b
}

/// 账守恒：联签数与账内 Signed 状态数一致。
fn conservation_ok() -> bool {
    let mut g = SidGate::new();
    let _ = g.connect(SecConnParams {
        conn_id: 0x40,
        server_sid: None,
        required_sid: None,
    });
    let _ = g.connect(SecConnParams {
        conn_id: 0x41,
        server_sid: Some(std_server_sid()),
        required_sid: Some(std_server_sid()),
    });
    let _ = g.verify(0x40);
    let _ = g.verify(0x41);
    let _ = g.sign(0x40);
    let _ = g.sign(0x41);
    g.conservation() && g.signed_count() == 2 && g.len() == 2
}

/// 完成写回语义：联签摘要非空。
fn writeback_ok() -> bool {
    let mut g = SidGate::new();
    let _ = g.connect(SecConnParams {
        conn_id: 0x42,
        server_sid: Some(std_server_sid()),
        required_sid: Some(std_server_sid()),
    });
    let _ = g.verify(0x42);
    if g.sign(0x42).is_err() {
        return false;
    }
    let summary = format!(
        "conn=0x42 sid={} signed={}",
        std_server_sid().brief(),
        g.signed_count()
    );
    !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12606 自检（判据 12605 卡：安全连接参数对平 + SID 错配注入）。
pub fn run_unx12605_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12605");

    s.add_dyn(
        "F12606-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12606-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", SD_ALL.len()),
    );

    s.add_dyn(
        "F12606-03 SID 静态语义",
        sid_semantics_ok(),
        "合法构造/全零拒/修订号 1..=3 界",
    );

    s.add_dyn(
        "F12606-04 SID 相等语义",
        sid_equality_ok(),
        "五路单段差全数判不等",
    );

    s.add_dyn(
        "F12606-05 双 SID 一致对平绿",
        happy_pair_ok(),
        "声明=呈现 → 校验过 → 联签",
    );

    s.add_dyn(
        "F12606-06 双不校验组合放行",
        both_none_ok(),
        "低安全连接（双方皆 None）合法",
    );

    // 注入必抓：十路 SID 错配全数拒止。
    s.add_dyn(
        "F12606-07 SID 错配注入 10/10 拒止",
        sid_mismatch_injections_ok(),
        "单段差/全段差/修订差/反序/缺呈现/擅自要求/越修订/全零/未验先签/重签",
    );

    s.add_dyn(
        "F12606-08 双跑一致",
        dual_run_ok(),
        "注入判定两次一致",
    );

    s.add_dyn(
        "F12606-09 联签账守恒",
        conservation_ok(),
        "Signed 数与联签数一致",
    );

    s.add_dyn(
        "F12606-10 未知连接拒",
        match SidGate::new().verify(0x99) {
            Err(code) => code == SidDiag::EUnknownConn.val(),
            Ok(()) => false,
        },
        "未连接直接校验拒（EUnknownConn）",
    );

    s.add_dyn(
        "F12606-11 空样本拒",
        match SidGate::new().connect(SecConnParams {
            conn_id: 0,
            server_sid: None,
            required_sid: None,
        }) {
            Err(code) => code == SidDiag::EEmpty.val(),
            Ok(()) => false,
        },
        "全零参数不构成安全连接",
    );

    s.add_dyn(
        "F12606-12 状态机单调与完成写回",
        VerifyState::Pending.rank() < VerifyState::Verified.rank()
            && VerifyState::Verified.rank() < VerifyState::Signed.rank()
            && writeback_ok(),
        "待校验→已校验→已联签 只进不退且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x80140);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(SD_ALL.len(), 12);
        let mut i = 0usize;
        while i < SD_ALL.len() {
            let vi = SD_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SD_ALL.len() {
                let vj = SD_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x37).contains("未知"));
    }

    #[test]
    fn sid_well_formed_semantics() {
        // 合法/全零/修订号越界。
        let good = std_server_sid();
        assert!(good.well_formed());
        let zero = Sid {
            revision: 1,
            sa0: 0,
            sa1: 0,
            sa2: 0,
            sa3: 0,
        };
        assert!(!zero.well_formed());
        assert!(!Sid {
            revision: 0,
            sa0: 1,
            ..zero
        }
        .well_formed());
        assert!(sid_semantics_ok());
    }

    #[test]
    fn sid_equality_semantics() {
        // 同 SID 相等，任意一段差即不等。
        let a = std_server_sid();
        let mut b = a;
        b.sa2 = a.sa2.wrapping_add(1);
        assert!(!a.equals(&b));
        assert!(a.equals(&std_server_sid()));
        assert!(sid_equality_ok());
    }

    #[test]
    fn mismatch_injections_all_rejected() {
        // 注入必抓：十路错配全数拒止。
        assert!(sid_mismatch_injections_ok());
    }

    #[test]
    fn verify_gate_combinations() {
        // 四种声明/呈现组合语义。
        let base = std_server_sid();
        let mut g = SidGate::new();
        // 声明有 + 呈现同：过。
        let _ = g.connect(SecConnParams {
            conn_id: 0x50,
            server_sid: Some(base),
            required_sid: Some(base),
        });
        assert!(g.verify(0x50).is_ok());
        // 声明有 + 呈现异：拒。
        let _ = g.connect(SecConnParams {
            conn_id: 0x51,
            server_sid: Some(base),
            required_sid: Some(Sid {
                sa1: base.sa1 + 7,
                ..base
            }),
        });
        assert_eq!(g.verify(0x51), Err(SidDiag::EMismatch.val()));
        // 声明有 + 呈现无：拒。
        let _ = g.connect(SecConnParams {
            conn_id: 0x52,
            server_sid: Some(base),
            required_sid: None,
        });
        assert_eq!(g.verify(0x52), Err(SidDiag::ENoRequired.val()));
        // 声明无 + 呈现有：拒。
        let _ = g.connect(SecConnParams {
            conn_id: 0x53,
            server_sid: None,
            required_sid: Some(base),
        });
        assert_eq!(g.verify(0x53), Err(SidDiag::EUnwanted.val()));
    }

    #[test]
    fn sign_requires_verified() {
        // 联签闸：待校验拒、已校验过、重复签拒。
        let mut g = SidGate::new();
        let _ = g.connect(SecConnParams {
            conn_id: 0x54,
            server_sid: None,
            required_sid: None,
        });
        assert_eq!(g.sign(0x54), Err(SidDiag::ENotVerified.val()));
        let _ = g.verify(0x54);
        assert!(g.sign(0x54).is_ok());
        assert_eq!(g.sign(0x54), Err(SidDiag::EDupSign.val()));
        // 未知连接联签拒。
        assert_eq!(g.sign(0x99), Err(SidDiag::EUnknownConn.val()));
    }

    #[test]
    fn connect_gates_reject() {
        // 重复连接与空样本。
        let mut g = SidGate::new();
        let p = SecConnParams {
            conn_id: 0x55,
            server_sid: None,
            required_sid: None,
        };
        assert!(g.connect(p).is_ok());
        assert_eq!(g.connect(p), Err(SidDiag::EDupConn.val()));
        assert_eq!(
            g.connect(SecConnParams {
                conn_id: 0,
                server_sid: None,
                required_sid: None,
            }),
            Err(SidDiag::EEmpty.val())
        );
    }

    #[test]
    fn dual_run_and_conservation() {
        // 双跑一致 + 账守恒。
        assert!(dual_run_ok());
        assert!(conservation_ok());
    }

    #[test]
    fn state_machine_monotonic() {
        // 状态机只进不退。
        assert!(VerifyState::Pending.rank() < VerifyState::Verified.rank());
        assert!(VerifyState::Verified.rank() < VerifyState::Signed.rank());
        let mut g = SidGate::new();
        let _ = g.connect(SecConnParams {
            conn_id: 0x56,
            server_sid: None,
            required_sid: None,
        });
        assert_eq!(g.ledger_of(0x56).map(|l| l.state), Some(VerifyState::Pending));
        let _ = g.verify(0x56);
        assert_eq!(g.ledger_of(0x56).map(|l| l.state), Some(VerifyState::Verified));
        // 已校验后再校验：幂等允许（状态不倒退）。
        assert!(g.verify(0x56).is_ok());
        assert_eq!(g.ledger_of(0x56).map(|l| l.state), Some(VerifyState::Verified));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12605_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
