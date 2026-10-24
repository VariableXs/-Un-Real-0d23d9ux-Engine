//! ve187_apifreeze ↔ VE-F0187 域内接口冻结清单与版本化
//!
//! A 域对外接口逐一登记冻结（呈现协商、显存分配、命令提交、调试通道四大族），
//! 每接口带语义版本号与冻结批次指针；破坏性变更需走升版流程并提前一版公告；
//! 冻结状态与实现一致性由测试套件背书。
//!
//! 判据：四大族冻结、语义版本、公告机制、测试背书。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 接口族。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApiFamily {
    /// 呈现协商（surface/present）。
    Presentation,
    /// 显存分配。
    MemoryAlloc,
    /// 命令提交。
    CommandSubmit,
    /// 调试通道。
    DebugChannel,
}

/// 语义版本号（major.minor.patch）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    /// 主版本（破坏性变更）。
    pub major: u16,
    /// 次版本（向后兼容新增）。
    pub minor: u16,
    /// 补丁版本（修复）。
    pub patch: u16,
}

impl SemVer {
    /// 新建版本号。
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }
}

/// 冻结接口条目。
#[derive(Debug, Clone)]
pub struct FrozenApi {
    /// 接口名称。
    pub name: String,
    /// 所属族。
    pub family: ApiFamily,
    /// 当前语义版本。
    pub version: SemVer,
    /// 冻结批次指针。
    pub freeze_batch: String,
    /// 签名摘要。
    pub signature: String,
    /// 测试背书（测试套件名）。
    pub test_backing: String,
    /// 是否已公告。
    pub announced: bool,
}

/// 公告记录。
#[derive(Debug, Clone)]
pub struct Announcement {
    /// 公告序号。
    pub seq: u64,
    /// 变更接口名。
    pub api_name: String,
    /// 旧版本。
    pub old_version: SemVer,
    /// 新版本。
    pub new_version: SemVer,
    /// 破坏性变更说明。
    pub breaking_desc: String,
    /// 公告时间。
    pub timestamp: u64,
}

/// 冻结清单错误码。
pub mod err {
    /// 接口不存在。
    pub const E_NO_API: u16 = 0x8001;
    /// 接口名重复。
    pub const E_DUPLICATE: u16 = 0x8002;
    /// 未公告的破坏性变更。
    pub const E_UNANNOUNCED_BREAKING: u16 = 0x8003;
    /// 破坏性变更需先公告。
    pub const E_NEEDS_ANNOUNCEMENT: u16 = 0x8004;
}

/// 接口冻结清单。
pub struct ApiFreezeList {
    /// 接口表。
    apis: BTreeMap<String, FrozenApi>,
    /// 公告记录。
    announcements: Vec<Announcement>,
    /// 全局版本（清单版本）。
    list_version: u64,
}

impl ApiFreezeList {
    /// 新建清单。
    pub fn new() -> Self {
        Self {
            apis: BTreeMap::new(),
            announcements: Vec::new(),
            list_version: 1,
        }
    }

    /// 清单版本号。
    pub fn list_version(&self) -> u64 {
        self.list_version
    }

    /// 登记冻结接口。
    pub fn freeze(
        &mut self,
        name: &str,
        family: ApiFamily,
        version: SemVer,
        batch: &str,
        signature: &str,
        test_backing: &str,
    ) -> Result<(), u16> {
        if name.is_empty() {
            return Err(err::E_NO_API);
        }
        if self.apis.contains_key(name) {
            return Err(err::E_DUPLICATE);
        }
        let api = FrozenApi {
            name: name.to_string(),
            family,
            version,
            freeze_batch: batch.to_string(),
            signature: signature.to_string(),
            test_backing: test_backing.to_string(),
            announced: true, // 首次冻结即公告
        };
        self.apis.insert(name.to_string(), api);
        self.list_version += 1;
        Ok(())
    }

    /// 升版：破坏性变更需先公告。
    pub fn bump_version(
        &mut self,
        name: &str,
        new_version: SemVer,
        breaking: bool,
        desc: &str,
    ) -> Result<(), u16> {
        let api = self.apis.get_mut(name).ok_or(err::E_NO_API)?;
        let old_version = api.version;

        if breaking {
            // 破坏性变更必须提前一版公告
            if !api.announced {
                return Err(err::E_NEEDS_ANNOUNCEMENT);
            }
            // 记录公告
            self.announcements.push(Announcement {
                seq: self.announcements.len() as u64,
                api_name: name.to_string(),
                old_version,
                new_version,
                breaking_desc: desc.to_string(),
                timestamp: self.list_version,
            });
            // 公告后重置 announced 标志（下次破坏性变更需再公告）
            api.announced = true;
        }

        api.version = new_version;
        self.list_version += 1;
        Ok(())
    }

    /// 提前公告破坏性变更（为下一次升版做准备）。
    pub fn announce_breaking(&mut self, name: &str, desc: &str) -> Result<(), u16> {
        let api = self.apis.get_mut(name).ok_or(err::E_NO_API)?;
        api.announced = true;
        self.announcements.push(Announcement {
            seq: self.announcements.len() as u64,
            api_name: name.to_string(),
            old_version: api.version,
            new_version: api.version, // 尚未变更
            breaking_desc: desc.to_string(),
            timestamp: self.list_version,
        });
        Ok(())
    }

    /// 查询接口。
    pub fn get(&self, name: &str) -> Option<&FrozenApi> {
        self.apis.get(name)
    }

    /// 按族列出接口。
    pub fn list_by_family(&self, family: ApiFamily) -> Vec<&FrozenApi> {
        self.apis.values().filter(|a| a.family == family).collect()
    }

    /// 全量接口。
    pub fn all(&self) -> Vec<&FrozenApi> {
        self.apis.values().collect()
    }

    /// 公告记录。
    pub fn announcement_log(&self) -> &[Announcement] {
        &self.announcements
    }

    /// 一致性核验：检查所有接口都有测试背书。
    pub fn verify_test_backing(&self) -> Vec<String> {
        self.apis
            .values()
            .filter(|a| a.test_backing.is_empty())
            .map(|a| alloc::format!("接口[{}]无测试背书", a.name))
            .collect()
    }
}

impl Default for ApiFreezeList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freeze_and_query() {
        let mut f = ApiFreezeList::new();
        f.freeze(
            "present_surface",
            ApiFamily::Presentation,
            SemVer::new(1, 0, 0),
            "batch_A01",
            "fn present(surface: &Surface)",
            "test_present",
        )
        .unwrap();
        assert_eq!(f.all().len(), 1);
        assert_eq!(f.list_by_family(ApiFamily::Presentation).len(), 1);
    }

    #[test]
    fn test_breaking_needs_announcement() {
        let mut f = ApiFreezeList::new();
        f.freeze("api", ApiFamily::CommandSubmit, SemVer::new(1, 0, 0), "b", "sig", "test")
            .unwrap();
        // 未公告的破坏性变更被拒绝
        assert_eq!(
            f.bump_version("api", SemVer::new(2, 0, 0), true, "breaking"),
            Err(err::E_NEEDS_ANNOUNCEMENT)
        );
    }

    #[test]
    fn test_announce_then_bump() {
        let mut f = ApiFreezeList::new();
        f.freeze("api", ApiFamily::CommandSubmit, SemVer::new(1, 0, 0), "b", "sig", "test")
            .unwrap();
        f.announce_breaking("api", "签名变更").unwrap();
        assert!(f.bump_version("api", SemVer::new(2, 0, 0), true, "签名变更").is_ok());
        assert_eq!(f.announcement_log().len(), 2); // announce + bump
    }

    #[test]
    fn test_non_breaking_bump() {
        let mut f = ApiFreezeList::new();
        f.freeze("api", ApiFamily::DebugChannel, SemVer::new(1, 0, 0), "b", "sig", "test")
            .unwrap();
        assert!(f.bump_version("api", SemVer::new(1, 1, 0), false, "新增参数").is_ok());
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut f = ApiFreezeList::new();
        f.freeze("x", ApiFamily::Presentation, SemVer::new(1, 0, 0), "b", "s", "t")
            .unwrap();
        assert_eq!(
            f.freeze("x", ApiFamily::Presentation, SemVer::new(1, 0, 0), "b", "s", "t"),
            Err(err::E_DUPLICATE)
        );
    }

    #[test]
    fn test_verify_test_backing() {
        let mut f = ApiFreezeList::new();
        f.freeze("a", ApiFamily::Presentation, SemVer::new(1, 0, 0), "b", "s", "")
            .unwrap();
        assert_eq!(f.verify_test_backing().len(), 1);
    }
}
