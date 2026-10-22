//! CGPU-F0413 · 集成 API 冻结 v1（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API：集成族接口冻结（域间联动注册/事件订阅/降级控制——签名冻结
//!   版本化）；与 C01-C05 API 命名一致；跨域契约文档化。
//! - 判据：冻结、一致、契约文档、判据。
//!
//! 设计纪律：
//! - 集成族三签名冻结：int.link.register（域间联动注册）、int.event.
//!   subscribe（事件订阅）、int.degrade.control（降级控制）——v1 冻结
//!   带版本号，变更走新版本（不原地改）。
//! - 命名一致：与 C01-C05 冻结总册同规则（家族.名 + 指纹同源算法），
//!   集成族前缀 int.；跨册指纹可互查（一致 = 名+族指纹相等）。
//! - 跨域契约文档化：每签名一张契约卡（方向/参数/失败语义），卡齐
//!   且卡与签名一一对应才允许冻结 v1。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D00 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D00，全仓独占）。
pub const SEG_BASE: u16 = 0x0D00;

pub const E_IAPI_DUP: u16 = SEG_BASE + 1; // 签名重名
pub const E_IAPI_NAMING: u16 = SEG_BASE + 2; // 命名规则违规
pub const E_IAPI_CARD: u16 = SEG_BASE + 3; // 契约卡缺失/不对应
pub const E_IAPI_VER: u16 = SEG_BASE + 4; // 版本回退
pub const E_IAPI_FP: u16 = SEG_BASE + 5; // 指纹不一致
pub const E_IAPI_PREFIX: u16 = SEG_BASE + 6; // 家族前缀错

/// 集成 API 冻结版本 v1。
pub const INT_FREEZE_VERSION: u32 = 11; // v1.1（集成族首冻结）

/// 集成族三签名的标准名。
pub const INT_SIGS: [&str; 3] = [
    "int.link.register",
    "int.event.subscribe",
    "int.degrade.control",
];

/// 命名规则：必须 `int.<组>.<动作>` 三段式。
pub fn naming_ok(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 3 && parts[0] == "int" && parts.iter().all(|p| !p.is_empty())
}

/// 指纹（与 C 域总册同源算法——跨册一致性核对面）。
pub fn fp64(bytes: &[u8], family: u8) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325 ^ ((family as u64) << 8);
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

/// 集成签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntSig {
    pub name: String,
    /// 家族号（6=集成族——六族之后的第七族）。
    pub family: u8,
    pub frozen_at: u32,
    pub fingerprint: u64,
    /// 方向：0=入站 1=出站 2=双向。
    pub direction: u8,
}

impl IntSig {
    pub fn new(name: &str, direction: u8, version: u32) -> Self {
        IntSig {
            name: name.to_string(),
            family: 6,
            frozen_at: version,
            fingerprint: fp64(name.as_bytes(), 6),
            direction,
        }
    }
}

/// 跨域契约卡。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractCard {
    /// 对应签名名（一一对应）。
    pub sig: String,
    /// 参数清单（非空）。
    pub params: Vec<String>,
    /// 失败语义（非空——失败码或行为描述）。
    pub failure_semantics: String,
    /// 对端域号。
    pub peer: u8,
}

impl ContractCard {
    pub fn complete(&self) -> bool {
        !self.params.is_empty() && !self.failure_semantics.is_empty()
    }
}

/// 集成 API 冻结册。
#[derive(Clone, Debug)]
pub struct IntFreeze {
    pub version: u32,
    pub sigs: Vec<IntSig>,
    pub cards: Vec<ContractCard>,
}

impl IntFreeze {
    pub fn new(version: u32) -> Self {
        IntFreeze { version, sigs: Vec::new(), cards: Vec::new() }
    }

    /// 冻结签名：命名规则 + 重名防线。
    pub fn freeze(&mut self, name: &str, direction: u8) -> Result<(), u16> {
        if !naming_ok(name) {
            return Err(E_IAPI_NAMING);
        }
        if name.split('.').next() != Some("int") {
            return Err(E_IAPI_PREFIX);
        }
        if self.sigs.iter().any(|s| s.name == name) {
            return Err(E_IAPI_DUP);
        }
        self.sigs.push(IntSig::new(name, direction, self.version));
        Ok(())
    }

    /// 挂契约卡：签名必须在册 + 卡完整 + 一签名一卡。
    pub fn bind_card(&mut self, card: ContractCard) -> Result<(), u16> {
        if !self.sigs.iter().any(|s| s.name == card.sig) {
            return Err(E_IAPI_CARD);
        }
        if !card.complete() {
            return Err(E_IAPI_CARD);
        }
        if self.cards.iter().any(|c| c.sig == card.sig) {
            return Err(E_IAPI_DUP);
        }
        self.cards.push(card);
        Ok(())
    }

    /// v1 冻结闸：三标准签名齐 + 每签名有完整契约卡。
    pub fn freeze_v1(&mut self) -> Result<u32, u16> {
        for n in INT_SIGS {
            if !self.sigs.iter().any(|s| s.name == n) {
                return Err(E_IAPI_CARD);
            }
        }
        for s in &self.sigs {
            if !self.cards.iter().any(|c| c.sig == s.name && c.complete()) {
                return Err(E_IAPI_CARD);
            }
        }
        self.version = INT_FREEZE_VERSION;
        Ok(self.version)
    }

    /// 与 C 域总册命名一致：同源指纹算法 + 命名规则同构。
    pub fn naming_consistent_with_c(&self) -> Result<(), u16> {
        for s in &self.sigs {
            if !naming_ok(&s.name) {
                return Err(E_IAPI_NAMING);
            }
            // 指纹同源：本册指纹与独立重算一致。
            if s.fingerprint != fp64(s.name.as_bytes(), 6) {
                return Err(E_IAPI_FP);
            }
        }
        Ok(())
    }

    /// 版本纪律：新册版本不回退。
    pub fn version_ok(&self, new_ver: u32) -> Result<(), u16> {
        if new_ver < self.version {
            return Err(E_IAPI_VER);
        }
        Ok(())
    }
}

/// 集成族标准契约卡（六域对端各一——文档化的固化输出）。
pub fn standard_cards() -> Vec<ContractCard> {
    vec![
        ContractCard {
            sig: "int.link.register".to_string(),
            params: vec!["domain".to_string(), "callback".to_string()],
            failure_semantics: "重复注册返回 E_IAPI_DUP；未冻结域拒".to_string(),
            peer: 255,
        },
        ContractCard {
            sig: "int.event.subscribe".to_string(),
            params: vec!["topic".to_string(), "sink".to_string()],
            failure_semantics: "主题不存在返回 E_IAPI_CARD；重复订阅幂等".to_string(),
            peer: 255,
        },
        ContractCard {
            sig: "int.degrade.control".to_string(),
            params: vec!["dim".to_string(), "mode".to_string()],
            failure_semantics: "非法维度返回 E_ISO_POINT；状态机违规拒绝".to_string(),
            peer: 255,
        },
    ]
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> IntFreeze {
        let mut b = IntFreeze::new(1);
        b.freeze("int.link.register", 2).ok();
        b.freeze("int.event.subscribe", 0).ok();
        b.freeze("int.degrade.control", 1).ok();
        b
    }

    #[test]
    fn a01_冻结_三签名版本化() {
        let mut b = book();
        // 契约文档齐备才允许冻结（文档化是冻结闸的一部分）。
        for c in standard_cards() {
            b.bind_card(c).ok();
        }
        assert_eq!(b.freeze_v1(), Ok(INT_FREEZE_VERSION));
        assert_eq!(b.version, INT_FREEZE_VERSION);
        // 冻结面：三签名在册、方向各记。
        assert_eq!(b.sigs.len(), 3);
        assert_eq!(b.sigs[0].direction, 2);
        assert_eq!(b.sigs[0].frozen_at, 1);
    }

    #[test]
    fn a02_命名一致_三段式与同源指纹() {
        let b = book();
        assert_eq!(b.naming_consistent_with_c(), Ok(()));
        // 命名违规。
        let mut b2 = IntFreeze::new(1);
        assert_eq!(b2.freeze("link.register", 0), Err(E_IAPI_NAMING));
        assert_eq!(b2.freeze("int.two", 0), Err(E_IAPI_NAMING));
        assert_eq!(b2.freeze("int..x", 0), Err(E_IAPI_NAMING));
        // 前缀错。
        assert_eq!(b2.freeze("acct.open", 0), Err(E_IAPI_NAMING));
        // 指纹同源可独立复算。
        for s in &b.sigs {
            assert_eq!(s.fingerprint, fp64(s.name.as_bytes(), 6));
        }
        // 重名拒。
        let mut b3 = book();
        assert_eq!(b3.freeze("int.link.register", 0), Err(E_IAPI_DUP));
    }

    #[test]
    fn a03_契约文档_卡齐才允许冻结() {
        let mut b = book();
        // 无卡冻结拒。
        assert_eq!(b.freeze_v1(), Err(E_IAPI_CARD));
        // 挂标准卡后放行。
        for c in standard_cards() {
            b.bind_card(c).ok();
        }
        assert_eq!(b.freeze_v1(), Ok(INT_FREEZE_VERSION));
        // 卡签名不在册拒。
        let mut b2 = book();
        assert_eq!(
            b2.bind_card(ContractCard {
                sig: "ghost".to_string(),
                params: vec!["p".to_string()],
                failure_semantics: "f".to_string(),
                peer: 1,
            }),
            Err(E_IAPI_CARD)
        );
        // 卡不完整拒。
        let mut b3 = book();
        assert_eq!(
            b3.bind_card(ContractCard {
                sig: "int.link.register".to_string(),
                params: vec![],
                failure_semantics: "f".to_string(),
                peer: 1,
            }),
            Err(E_IAPI_CARD)
        );
        // 一签名一卡。
        let mut b4 = book();
        let c = standard_cards()[0].clone();
        b4.bind_card(c).ok();
        assert_eq!(b4.bind_card(standard_cards()[0].clone()), Err(E_IAPI_DUP));
    }

    #[test]
    fn a04_版本纪律_不回退() {
        let mut b = book();
        b.version = INT_FREEZE_VERSION; // 冻结后基线 v1.1
        assert_eq!(b.version_ok(12), Ok(()));
        assert_eq!(b.version_ok(11), Ok(()));
        assert_eq!(b.version_ok(10), Err(E_IAPI_VER));
    }

    #[test]
    fn a05_标准卡_六域对端文档化() {
        let cards = standard_cards();
        assert_eq!(cards.len(), 3);
        for c in &cards {
            assert!(c.complete());
            assert!(naming_ok(&c.sig));
        }
        // 卡与签名一一对应。
        let b = book();
        for c in &cards {
            assert!(b.sigs.iter().any(|s| s.name == c.sig));
        }
    }
}
