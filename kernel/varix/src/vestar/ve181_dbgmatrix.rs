//! ve181_dbgmatrix ↔ VE-F0181 调试分级能力矩阵
//!
//! 调试能力按构建档位矩阵（release、debug、实验档三列）登记：每项能力在哪个档位可用、
//! 受什么约束；越权请求（release 档请求注入类能力）明确拒绝并说明可用替代；
//! 矩阵版本化随宣告发布。
//!
//! 判据：三档矩阵、越权拒绝、保守未知、版本化。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 构建档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuildProfile {
    /// 发布档（最保守，无注入能力）。
    Release,
    /// 调试档（开发调试能力可用）。
    Debug,
    /// 实验档（全部能力可用，含高风险注入）。
    Experimental,
}

/// 能力约束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// 无约束。
    Unrestricted,
    /// 需确认（弹二次确认）。
    RequiresConfirmation,
    /// 需日志记录（操作留痕）。
    RequiresAudit,
    /// 需非生产环境。
    NonProductionOnly,
}

/// 能力矩阵条目。
#[derive(Debug, Clone)]
pub struct CapabilityEntry {
    /// 能力名称（唯一键）。
    pub name: String,
    /// 三档可用性（release, debug, experimental）。
    pub availability: [bool; 3],
    /// 约束。
    pub constraint: Constraint,
    /// 描述。
    pub desc: String,
    /// 替代建议（当在低档位被拒绝时）。
    pub alternative: String,
}

/// 能力矩阵错误码。
pub mod err {
    /// 越权请求（当前档位不可用）。
    pub const E_DENIED: u16 = 0x7B01;
    /// 能力未登记。
    pub const E_UNKNOWN: u16 = 0x7B02;
    /// 能力名重复。
    pub const E_DUPLICATE: u16 = 0x7B03;
    /// 矩阵数据无效。
    pub const E_BAD_MATRIX: u16 = 0x7B04;
}

/// 调试分级能力矩阵。
pub struct DebugCapabilityMatrix {
    /// 能力表。
    capabilities: BTreeMap<String, CapabilityEntry>,
    /// 矩阵版本号。
    version: u64,
    /// 当前构建档位。
    profile: BuildProfile,
}

impl DebugCapabilityMatrix {
    /// 新建矩阵（默认 release 档）。
    pub fn new() -> Self {
        Self {
            capabilities: BTreeMap::new(),
            version: 1,
            profile: BuildProfile::Release,
        }
    }

    /// 设置当前构建档位。
    pub fn set_profile(&mut self, profile: BuildProfile) {
        self.profile = profile;
    }

    /// 获取当前档位。
    pub fn profile(&self) -> BuildProfile {
        self.profile
    }

    /// 矩阵版本号。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 登记能力。
    pub fn register(
        &mut self,
        name: &str,
        release: bool,
        debug: bool,
        experimental: bool,
        constraint: Constraint,
        desc: &str,
        alternative: &str,
    ) -> Result<(), u16> {
        if name.is_empty() {
            return Err(err::E_BAD_MATRIX);
        }
        if self.capabilities.contains_key(name) {
            return Err(err::E_DUPLICATE);
        }
        // 实验档必须全部可用（实验档是最宽松档位）
        // 注意：这里不要求 experimental=true，允许能力只在特定档位注册
        let entry = CapabilityEntry {
            name: name.to_string(),
            availability: [release, debug, experimental],
            constraint,
            desc: desc.to_string(),
            alternative: alternative.to_string(),
        };
        self.capabilities.insert(name.to_string(), entry);
        self.version += 1;
        Ok(())
    }

    /// 查询能力在当前档位是否可用。
    pub fn is_available(&self, name: &str) -> bool {
        match self.capabilities.get(name) {
            Some(entry) => {
                let idx = match self.profile {
                    BuildProfile::Release => 0,
                    BuildProfile::Debug => 1,
                    BuildProfile::Experimental => 2,
                };
                entry.availability[idx]
            }
            // 未登记能力：按最严档处理（保守未知原则）
            None => false,
        }
    }

    /// 请求能力（带拒绝文案）。
    ///
    /// Ok 返回约束；Err 返回 (错误码, 拒绝文案)。
    pub fn request(&self, name: &str) -> Result<(&Constraint, &str), (u16, String)> {
        match self.capabilities.get(name) {
            Some(entry) => {
                let idx = match self.profile {
                    BuildProfile::Release => 0,
                    BuildProfile::Debug => 1,
                    BuildProfile::Experimental => 2,
                };
                if entry.availability[idx] {
                    Ok((&entry.constraint, entry.desc.as_str()))
                } else {
                    // 越权拒绝 + 替代说明（三要素：原因/约束/替代）
                    let msg = alloc::format!(
                        "拒绝: 能力[{}]在{:?}档位不可用。约束: {:?}。替代: {}",
                        name,
                        self.profile,
                        entry.constraint,
                        if entry.alternative.is_empty() {
                            "无直接替代，请升级构建档位"
                        } else {
                            entry.alternative.as_str()
                        }
                    );
                    Err((err::E_DENIED, msg))
                }
            }
            None => {
                // 未登记能力：按最严档处理并提示补登记
                let msg = alloc::format!(
                    "拒绝: 能力[{}]未登记入矩阵，按最严档处理。请补登记后重试。",
                    name
                );
                Err((err::E_UNKNOWN, msg))
            }
        }
    }

    /// 列出当前档位所有可用能力。
    pub fn list_available(&self) -> Vec<&CapabilityEntry> {
        let idx = match self.profile {
            BuildProfile::Release => 0,
            BuildProfile::Debug => 1,
            BuildProfile::Experimental => 2,
        };
        self.capabilities
            .values()
            .filter(|e| e.availability[idx])
            .collect()
    }

    /// 列出所有登记能力。
    pub fn list_all(&self) -> Vec<&CapabilityEntry> {
        self.capabilities.values().collect()
    }

    /// 获取能力详情。
    pub fn get(&self, name: &str) -> Option<&CapabilityEntry> {
        self.capabilities.get(name)
    }

    /// 检查矩阵与实现一致性：验证三档中 debug 必须是 release 的超集（release 可用的 debug 必可用）。
    pub fn validate_consistency(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for entry in self.capabilities.values() {
            // release 可用但 debug 不可用 → 逻辑矛盾
            if entry.availability[0] && !entry.availability[1] {
                issues.push(alloc::format!(
                    "能力[{}] release可用但debug不可用，矩阵逻辑矛盾",
                    entry.name
                ));
            }
            // debug 可用但 experimental 不可用 → 逻辑矛盾
            if entry.availability[1] && !entry.availability[2] {
                issues.push(alloc::format!(
                    "能力[{}] debug可用但experimental不可用，矩阵逻辑矛盾",
                    entry.name
                ));
            }
        }
        issues
    }
}

impl Default for DebugCapabilityMatrix {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_matrix() -> DebugCapabilityMatrix {
        let mut m = DebugCapabilityMatrix::new();
        m.register("inspect_state", true, true, true, Constraint::Unrestricted, "查看状态", "")
            .unwrap();
        m.register(
            "inject_error",
            false,
            false,
            true,
            Constraint::RequiresConfirmation,
            "注入错误",
            "使用模拟器",
        )
        .unwrap();
        m.register(
            "force_crash",
            false,
            true,
            true,
            Constraint::RequiresAudit,
            "强制崩溃",
            "",
        )
        .unwrap();
        m
    }

    #[test]
    fn test_release_denies_inject() {
        let m = make_matrix();
        assert!(!m.is_available("inject_error"));
        let (code, _) = m.request("inject_error").unwrap_err();
        assert_eq!(code, err::E_DENIED);
    }

    #[test]
    fn test_debug_allows_force_crash() {
        let mut m = make_matrix();
        m.set_profile(BuildProfile::Debug);
        assert!(m.is_available("force_crash"));
        assert!(m.request("force_crash").is_ok());
    }

    #[test]
    fn test_experimental_allows_inject() {
        let mut m = make_matrix();
        m.set_profile(BuildProfile::Experimental);
        assert!(m.is_available("inject_error"));
    }

    #[test]
    fn test_unknown_conservative() {
        let m = make_matrix();
        // 未登记能力：即使 experimental 档也拒绝
        assert!(!m.is_available("unknown_cap"));
        let (code, msg) = m.request("unknown_cap").unwrap_err();
        assert_eq!(code, err::E_UNKNOWN);
        assert!(msg.contains("未登记"));
    }

    #[test]
    fn test_denied_message_has_alternative() {
        let m = make_matrix();
        let (_, msg) = m.request("inject_error").unwrap_err();
        assert!(msg.contains("替代"));
        assert!(msg.contains("模拟器"));
    }

    #[test]
    fn test_consistency_check() {
        let mut m = DebugCapabilityMatrix::new();
        // 故意矛盾：release 可用但 debug 不可用
        m.register("bad", true, false, true, Constraint::Unrestricted, "d", "")
            .unwrap();
        let issues = m.validate_consistency();
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut m = DebugCapabilityMatrix::new();
        m.register("x", true, true, true, Constraint::Unrestricted, "d", "")
            .unwrap();
        assert_eq!(
            m.register("x", true, true, true, Constraint::Unrestricted, "d", ""),
            Err(err::E_DUPLICATE)
        );
    }
}
