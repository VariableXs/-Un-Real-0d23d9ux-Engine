//! UNX-F0218 · 报告消费方接口冻结（AI-87/89）
//!
//! 锚点判据（施工书原文）：UNX-F0218-J1 验收官/镜像官消费接口
//! 冻结联签，字段契约变更走 ADR。
//!
//! 设计：两消费方（验收官 / 镜像官）各自消费字段契约清单 →
//! 接口冻结簿（契约 + 双联签 + 版本）→ 变更闸：字段契约任何
//! 变更（增/删/类型变）必须携带 ADR 编号才能过闸，无 ADR 拒。
//! 红线：①冻结后无 ADR 变更必拒；②联签缺一（任一方未签）必拒；
//! ③消费读取只许走契约字段（越界读拒）。
//!
//! 错误码段：0x2CA0（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0218_VERSION: &str = "F0218-consfrozen-v1";

/// 诊断码段基址（UNX-F0218 独占）。
pub const SEG_BASE: u16 = 0x2CA0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 无 ADR 变更被拒。
    NoAdr = SEG_BASE + 1,
    /// 联签缺一。
    MissingCoSign = SEG_BASE + 2,
    /// 越界字段读。
    OutOfContract = SEG_BASE + 3,
    /// 接口未冻结即消费。
    NotFrozen = SEG_BASE + 4,
    /// 契约重复登记。
    DupField = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 消费方角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// 验收官（AI-87）。
    Acceptance,
    /// 镜像官（AI-89）。
    Image,
}

impl Role {
    /// 角色署名。
    pub const fn sign_name(self) -> &'static str {
        match self {
            Role::Acceptance => "AI-87",
            Role::Image => "AI-89",
        }
    }
}

/// 契约字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractField {
    /// 字段名。
    pub name: String,
    /// 类型名。
    pub ty: &'static str,
}

/// 冻结簿（接口契约 + 双联签 + 变更账）。
#[derive(Debug)]
pub struct FrozenApi {
    /// 契约字段。
    pub fields: Vec<ContractField>,
    /// 双联签（角色 → 是否已签）。
    signed: [(Role, bool); 2],
    /// 冻结态。
    frozen: bool,
    /// 接口版本（每次 ADR 变更 +1）。
    pub api_version: u32,
    /// ADR 账。
    pub adrs: Vec<String>,
}

impl Default for FrozenApi {
    fn default() -> Self {
        FrozenApi {
            fields: Vec::new(),
            signed: [(Role::Acceptance, false), (Role::Image, false)],
            frozen: false,
            api_version: 0,
            adrs: Vec::new(),
        }
    }
}

impl FrozenApi {
    /// 登记契约字段（冻结前）。
    pub fn declare(&mut self, name: &str, ty: &'static str) -> Result<(), u16> {
        if self.frozen {
            return Err(E::NoAdr.code());
        }
        if self.fields.iter().any(|f| f.name == name) {
            return Err(E::DupField.code());
        }
        self.fields.push(ContractField {
            name: String::from(name),
            ty,
        });
        Ok(())
    }

    /// 联签（双方各签一次）。
    pub fn co_sign(&mut self, role: Role) -> Result<(), u16> {
        let slot = match role {
            Role::Acceptance => 0,
            Role::Image => 1,
        };
        self.signed[slot] = (role, true);
        Ok(())
    }

    /// 冻结：双方齐签才生效。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if !self.signed.iter().all(|(_, s)| *s) {
            return Err(E::MissingCoSign.code());
        }
        self.frozen = true;
        self.api_version = 1;
        Ok(())
    }

    /// 契约内字段读（消费方读取）。
    pub fn read(&self, name: &str) -> Result<&ContractField, u16> {
        if !self.frozen {
            return Err(E::NotFrozen.code());
        }
        self.fields
            .iter()
            .find(|f| f.name == name)
            .ok_or(E::OutOfContract.code())
    }

    /// ADR 变更闸：契约变更必须携带 ADR 编号。
    pub fn amend(&mut self, name: &str, ty: &'static str, adr: &str) -> Result<(), u16> {
        if adr.is_empty() {
            return Err(E::NoAdr.code());
        }
        if !self.frozen {
            return Err(E::NotFrozen.code());
        }
        match self.fields.iter_mut().find(|f| f.name == name) {
            Some(f) => f.ty = ty,
            None => self.fields.push(ContractField {
                name: String::from(name),
                ty,
            }),
        }
        self.adrs.push(String::from(adr));
        self.api_version += 1;
        Ok(())
    }
}

/// J1 自证（判据原文：双消费方接口冻结联签，变更走 ADR）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：验收官 3 字段 + 镜像官共用契约。
    let mut api = FrozenApi::default();
    api.declare("boot-ok", "boolean")?;
    api.declare("cost-ms", "number")?;
    api.declare("report-hash", "string")?;

    // 二、未冻结消费拒；联签缺一冻结拒。
    if api.read("boot-ok").is_ok() {
        return Err(E::NotFrozen.code());
    }
    api.co_sign(Role::Acceptance)?;
    if api.freeze().is_ok() {
        return Err(E::MissingCoSign.code());
    }
    api.co_sign(Role::Image)?;
    api.freeze()?;

    // 三、J1 主体：双方契约内读全绿；版本 1。
    api.read("boot-ok")?;
    api.read("cost-ms")?;
    api.read("report-hash")?;
    if api.api_version != 1 {
        return Err(E::NotFrozen.code());
    }

    // 四、变更闸：无 ADR 拒；带 ADR 过闸 + 版本推进。
    if api.amend("cost-ms", "string", "").is_ok() {
        return Err(E::NoAdr.code());
    }
    api.amend("boot-ok", "boolean", "ADR-2026-041")?;
    if api.api_version != 2 || api.adrs.len() != 1 {
        return Err(E::NoAdr.code());
    }
    api.read("boot-ok")?;

    // 五、红线逐项：越界读 / 冻结后裸登记拒。
    if api.read("ghost").is_ok() {
        return Err(E::OutOfContract.code());
    }
    if api.declare("naked", "number").is_ok() {
        return Err(E::NoAdr.code());
    }
    if api.declare("boot-ok", "number").is_ok() {
        return Err(E::DupField.code());
    }

    // 六、判别式段位对拍。
    if E::NoAdr.code() != SEG_BASE + 1 || E::DupField.code() != SEG_BASE + 5 {
        return Err(E::NoAdr.code());
    }
    Ok(())
}
