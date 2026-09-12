//! C13 · appfw 注册描述符（壳C 向内核 appmgr 登记的身份与能力）。
//!
//! 内核侧入口：`varix/src/aurora/appfw.rs` 的 `app_register(name, perms, stack)`。
//! 用户侧这里只做**声明**；能否注册、授予多少，由内核裁决（最小能力原则）。
//! 权限位是镜像常量：两边各持一份、以内核自检为准，漂移会让注册失败并暴露。

/// 内核 appfw 权限位镜像（对齐 `varix/src/aurora/appfw.rs` `PERM_FS`）。
pub const PERM_FS: u32 = 1 << 1;

/// fs 子能力：读。折算入 [`PERM_FS`]。
pub const CAP_FS_READ: u32 = 1;

/// fs 子能力：写。折算入 [`PERM_FS`]。
pub const CAP_FS_WRITE: u32 = 2;

/// 应用名（appfw 规则：非空、≤15 字节、小写字母/数字）。
pub const APP_NAME: &str = "codeanalysis";

/// 栈大小（对齐 appfw 注册参数单位：字节）。
pub const STACK_BYTES: u64 = 64 * 1024;

/// 注册描述符：`app_register` 的用户侧声明（C13）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppManifest {
    pub name: &'static str,
    /// fs 子能力位（[`CAP_FS_READ`] / [`CAP_FS_WRITE`]）。
    pub caps: u32,
    pub stack_bytes: u64,
    /// ELF 入口符号（内核加载器按此跳转）。
    pub entry_symbol: &'static str,
}

impl AppManifest {
    /// 本应用的注册清单：只需要文件系统（读 + 写），别的一概不申请。
    pub const fn codeanalysis() -> Self {
        AppManifest {
            name: APP_NAME,
            caps: CAP_FS_READ | CAP_FS_WRITE,
            stack_bytes: STACK_BYTES,
            entry_symbol: "_start",
        }
    }

    /// 子能力折算成内核权限位。申请了任一 fs 子能力 ⇒ 需要 `PERM_FS`；
    /// 什么都不申请 ⇒ 0（纯计算应用也应当能注册）。
    pub const fn kernel_perms(&self) -> u32 {
        if self.caps & (CAP_FS_READ | CAP_FS_WRITE) != 0 {
            PERM_FS
        } else {
            0
        }
    }

    /// appfw 注册名规则：非空、≤15 字节、全小写字母/数字。
    pub fn name_ok(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= 15
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    }

    /// 清单自洽：名合法 + 栈按页对齐（4 KiB）+ 入口符号非空。
    pub fn valid(&self) -> bool {
        self.name_ok()
            && self.stack_bytes % 4096 == 0
            && self.stack_bytes >= 4096
            && !self.entry_symbol.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_declares_fs_only() {
        let m = AppManifest::codeanalysis();
        assert!(m.valid());
        assert_eq!(m.kernel_perms(), PERM_FS);
        assert_eq!(m.caps, CAP_FS_READ | CAP_FS_WRITE);
    }

    #[test]
    fn empty_caps_need_no_perms() {
        let m = AppManifest {
            caps: 0,
            ..AppManifest::codeanalysis()
        };
        assert_eq!(m.kernel_perms(), 0);
    }

    #[test]
    fn name_rules_match_appfw() {
        let m = AppManifest::codeanalysis();
        assert!(m.name_ok());
        let bad = AppManifest {
            name: "CodeAnalysis!",
            ..m
        };
        assert!(!bad.name_ok());
        let long = AppManifest {
            name: "averyveryverylongappname",
            ..m
        };
        assert!(!long.name_ok());
    }

    #[test]
    fn stack_must_be_page_aligned() {
        let m = AppManifest {
            stack_bytes: 4096 + 1,
            ..AppManifest::codeanalysis()
        };
        assert!(!m.valid());
    }
}
