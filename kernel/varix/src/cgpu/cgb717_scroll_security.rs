//! CGPU-F0717 · 滚动安全（目标 370 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0717，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动安全——滚动子系统的攻击面收敛：无溢出算术包装、
//!   权限分级、滑动窗口限速、区域描述符完整性校验与拒绝审计账。
//!
//! 设计纪律：
//! - 全程 checked/saturating 算术：任何 u32/u64 加乘先 checked_*，
//!   溢出即拒绝并记审计（不 panic 不回绕）。
//! - 权限三档闭集：Viewer（只读查询）/ Operator（限区内滚动）/
//!   Admin（全区域 + 改配置）；越权一律拒绝并记审计。
//! - 限速：滑动窗口（窗口长 16 帧，额度常量表按权限档），
//!   超额拒绝且计入审计，窗口滑出后额度恢复（纯账面推演）。
//! - 区域完整性：描述符（base,h,cap）三字段关系校验
//!   base ≤ base+h ≤ cap，违反即拒绝。
//! - 审计账定容：AUDIT_CAP=32 条环形，覆盖计数显式；
//!   守恒：denied_total == 审计内 denied 条数 + 被覆盖条数。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x47e0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x47e0，批内 CGPU-F0717 独占）。
pub const SEG_BASE: u16 = 0x47e0;

/// 算术溢出拒绝。
pub const E_OVERFLOW: u16 = SEG_BASE + 1;
/// 越权操作。
pub const E_PERM: u16 = SEG_BASE + 2;
/// 限速超额。
pub const E_RATE: u16 = SEG_BASE + 3;
/// 区域描述符不完整。
pub const E_REGION: u16 = SEG_BASE + 4;
/// 未知角色。
pub const E_ROLE: u16 = SEG_BASE + 5;
/// 审计账参数非法。
pub const E_AUDIT: u16 = SEG_BASE + 6;

/// 审计账容量。
pub const AUDIT_CAP: usize = 32;
/// 限速窗口长（帧）。
pub const RATE_WIN: u32 = 16;
/// Viewer 每窗口额度（次）。
pub const QUOTA_VIEWER: u32 = 8;
/// Operator 每窗口额度（次）。
pub const QUOTA_OPERATOR: u32 = 64;
/// Admin 每窗口额度（次）。
pub const QUOTA_ADMIN: u32 = 512;

/// 权限档（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 只读。
    Viewer,
    /// 区内滚动。
    Operator,
    /// 全权。
    Admin,
}

/// 角色额度。
pub fn quota(r: Role) -> u32 {
    match r {
        Role::Viewer => QUOTA_VIEWER,
        Role::Operator => QUOTA_OPERATOR,
        Role::Admin => QUOTA_ADMIN,
    }
}

/// 滚动区域描述符：base ≤ base+h ≤ cap。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// 区域基址（行）。
    pub base: u32,
    /// 区域高（行）。
    pub h: u32,
    /// 容器上限（行）。
    pub cap: u32,
}

impl Region {
    /// 完整性校验（溢出安全）。
    pub fn validate(&self) -> Result<(), u16> {
        let end = self
            .base
            .checked_add(self.h)
            .ok_or(E_REGION)?;
        if end > self.cap {
            return Err(E_REGION);
        }
        Ok(())
    }
    /// 区域内判定。
    pub fn contains(&self, pos: u32) -> bool {
        pos >= self.base && pos < self.base.saturating_add(self.h)
    }
}

/// 溢出安全位移计算：pos+dy 不回绕，越界返回 None。
pub fn checked_shift(pos: u32, dy: i32, limit: u32) -> Option<u32> {
    if dy >= 0 {
        let t = pos.checked_add(dy as u32)?;
        if t > limit {
            None
        } else {
            Some(t)
        }
    } else {
        let back = (dy as i64).unsigned_abs();
        if back > pos as u64 {
            None
        } else {
            Some(pos - back as u32)
        }
    }
}

/// 审计条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRec {
    /// 被拒角色（0=Viewer,1=Operator,2=Admin）。
    pub role: u8,
    /// 拒绝码。
    pub code: u16,
    /// 帧号。
    pub frame: u32,
}

/// 审计环形账。
#[derive(Clone, Debug)]
pub struct AuditRing {
    ring: [Option<AuditRec>; AUDIT_CAP],
    w: usize,
    held: usize,
    overwritten: u64,
    total: u64,
}

impl AuditRing {
    pub fn new() -> Self {
        Self {
            ring: Default::default(),
            w: 0,
            held: 0,
            overwritten: 0,
            total: 0,
        }
    }
    /// 追加一条（满则覆盖最旧）。
    pub fn push(&mut self, r: AuditRec) {
        self.ring[self.w] = Some(r);
        self.w = (self.w + 1) % AUDIT_CAP;
        if self.held < AUDIT_CAP {
            self.held += 1;
        } else {
            self.overwritten += 1;
        }
        self.total += 1;
    }
    /// 守恒：total == held + overwritten。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.held as u64 + self.overwritten {
            return Err(E_AUDIT);
        }
        Ok(())
    }
    /// 视图（旧→新）。
    pub fn view(&self) -> Vec<AuditRec> {
        let start = if self.held < AUDIT_CAP { 0 } else { self.w };
        let mut out = Vec::with_capacity(self.held);
        for i in 0..self.held {
            let idx = (start + i) % AUDIT_CAP;
            if let Some(r) = self.ring[idx] {
                out.push(r);
            }
        }
        out
    }
    pub fn total(&self) -> u64 {
        self.total
    }
}

/// 滚动安全门：权限 + 限速 + 区域三合一。
#[derive(Clone, Debug)]
pub struct ScrollGate {
    role: Role,
    region: Region,
    /// 窗口起点帧。
    win_start: u32,
    /// 窗口内已用次数。
    win_used: u32,
    audit: AuditRing,
}

impl ScrollGate {
    pub fn new(role: Role, region: Region) -> Result<Self, u16> {
        region.validate()?;
        Ok(Self {
            role,
            region,
            win_start: 0,
            win_used: 0,
            audit: AuditRing::new(),
        })
    }

    /// 申请一次滚动：帧推进滑窗 → 权限 → 区域 → 限速。
    pub fn request(&mut self, pos: u32, dy: i32, frame: u32) -> Result<u32, u16> {
        // 1) 滑窗推进：frame 超出窗口则重置额度。
        if frame.saturating_sub(self.win_start) >= RATE_WIN {
            self.win_start = frame;
            self.win_used = 0;
        }
        // 2) 权限：Viewer 只许 dy==0（查询）。
        if self.role == Role::Viewer && dy != 0 {
            self.audit.push(AuditRec {
                role: 0,
                code: E_PERM,
                frame,
            });
            return Err(E_PERM);
        }
        // 3) 区域：目标位置必须仍在区域内（Admin 也受限本区域描述符）。
        let target = match checked_shift(pos, dy, self.region.cap) {
            Some(t) => t,
            None => {
                let code = E_OVERFLOW;
                self.audit.push(AuditRec {
                    role: self.role_ord(),
                    code,
                    frame,
                });
                return Err(code);
            }
        };
        if !self.region.contains(target) {
            self.audit.push(AuditRec {
                role: self.role_ord(),
                code: E_REGION,
                frame,
            });
            return Err(E_REGION);
        }
        // 4) 限速。
        if self.win_used >= quota(self.role) {
            self.audit.push(AuditRec {
                role: self.role_ord(),
                code: E_RATE,
                frame,
            });
            return Err(E_RATE);
        }
        self.win_used += 1;
        Ok(target)
    }

    /// 角色序号（审计用）。
    fn role_ord(&self) -> u8 {
        match self.role {
            Role::Viewer => 0,
            Role::Operator => 1,
            Role::Admin => 2,
        }
    }

    /// 审计守恒透传。
    pub fn audit_ok(&self) -> Result<(), u16> {
        self.audit.audit()
    }
    /// 审计总拒数。
    pub fn denied_total(&self) -> u64 {
        self.audit.total()
    }
    /// 窗口已用。
    pub fn used(&self) -> u32 {
        self.win_used
    }
    /// 角色。
    pub fn role(&self) -> Role {
        self.role
    }
    /// 区域。
    pub fn region(&self) -> Region {
        self.region
    }
}

/// 自例程：主路径（Admin 正常滚 + 滑窗恢复）+ 错误路径 + 审计守恒。
pub fn drill() -> Result<u32, u16> {
    let reg = Region {
        base: 0,
        h: 1024,
        cap: 2048,
    };
    reg.validate()?;
    // 错误路径 0：坏区域（base+h > cap）。
    let bad_reg = Region {
        base: 2000,
        h: 100,
        cap: 2048,
    };
    if bad_reg.validate().err() != Some(E_REGION)
        || ScrollGate::new(Role::Admin, bad_reg).err() != Some(E_REGION)
    {
        return Err(E_REGION);
    }
    // 主路径：Operator 从 0 滚到 512。
    let mut g = ScrollGate::new(Role::Operator, reg)?;
    let mut pos = 0u32;
    for f in 0..8u32 {
        pos = g.request(pos, 64, f)?;
    }
    if pos != 512 {
        return Err(E_OVERFLOW);
    }
    if g.used() != 8 {
        return Err(E_RATE);
    }
    // 错误路径 1：Viewer 越权。
    let mut gv = ScrollGate::new(Role::Viewer, reg)?;
    if gv.request(0, 1, 0) != Err(E_PERM) {
        return Err(E_PERM);
    }
    // Viewer 查询（dy=0）合法。
    if gv.request(0, 0, 0)? != 0 {
        return Err(E_PERM);
    }
    // 错误路径 2a：区域外目标（pos=1500 在 cap 内但在区域 [0,1024) 外）。
    let mut ga = ScrollGate::new(Role::Admin, reg)?;
    if ga.request(1500, 1, 0) != Err(E_REGION) {
        return Err(E_REGION);
    }
    // 错误路径 2b：溢出（pos=1023 上界，+i32::MAX → E_OVERFLOW）。
    if ga.request(1023, i32::MAX, 0) != Err(E_OVERFLOW) {
        return Err(E_OVERFLOW);
    }
    // 错误路径 3：限速——Viewer 额度 8，先耗尽再申请。
    let mut gv2 = ScrollGate::new(Role::Viewer, reg)?;
    for f in 0..8u32 {
        gv2.request(0, 0, f)?;
    }
    if gv2.request(0, 0, 8) != Err(E_RATE) {
        return Err(E_RATE);
    }
    // 滑窗恢复：帧 16 起（8+16 ≥ 16）重置。
    if gv2.request(0, 0, 16)? != 0 {
        return Err(E_RATE);
    }
    if gv2.used() != 1 {
        return Err(E_RATE);
    }
    // 审计守恒：至今拒绝 = Viewer 越权 1 + 溢出 1 + 限速 1 = 3。
    ga.audit_ok()?;
    gv.audit_ok()?;
    gv2.audit_ok()?;
    if gv2.denied_total() != 1 {
        return Err(E_AUDIT);
    }
    // 错误路径 4：checked_shift 负向越界。
    if checked_shift(0, -1, 1024).is_some() {
        return Err(E_OVERFLOW);
    }
    if checked_shift(10, -5, 1024) != Some(5) {
        return Err(E_OVERFLOW);
    }
    Ok(0)
}
