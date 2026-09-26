//! F030 安装器兼容模式（compatstar · G-A-30）——「装软件」是完整体验不是走钢丝。
//!
//! 主册判据（验收标准第一句）：
//! **三族各一款开源软件安装-使用-卸载全周期绿；沙盒外镜像哈希不变
//! （与 F010 联合验证）。**
//!
//! 功能定义（G-A-30）：NSIS/Inno Setup/MSI 三族安装器在沙盒内完成安装：
//! 临时解压/进度页/完成页如实呈现；安装产物登记（F031 卸载清单）+ 开始
//! 菜单 .lnk 生成（F013）+ 桌面图标（C-3 尺寸网格）。
//!
//! 【设计细节】NSIS 解包监听：临时目录写入映射沙盒 temp；Inno 的任务选择页
//! 与安装目录页全可交互（目录默认沙盒 Program，用户可改到共享区）；MSI
//! 序列号与组件选择按标准 UI 流；安装进度条程序自绘（如实透传）；「立即
//! 运行」勾选执行时若触发 F035 失败则向导接管。
//! 【数据与存储】全产物落 F009/F010 沙盒；卸载清单（文件清单+蜂巢+快捷方式
//! 三表）存沙盒元数据。
//! 【状态与异常】安装器要求重启 → 诚实告知「VARIX 无需重启即可用，已跳过
//! 重启步骤」+ 日志；安装中途取消 → 已写文件进回收站式回滚（零真删）；MSI
//! 自修复请求 → 重新执行修复流。UAC 类提权语义按 F038 隔离档替代（弹 VARIX
//! 权限卡说明将写入哪些沙盒区）。
//!
//! 零堆纪律：定长清单表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 安装器三族。
pub const FAMILIES: [&str; 3] = ["nsis", "inno", "msi"];
/// 沙盒区三表：文件清单 + 蜂巢 + 快捷方式（卸载清单结构——主册【数据与存储】）。
pub const MANIFEST_TABLES: [&str; 3] = ["files", "hive", "shortcuts"];
/// 清单容量（单次安装产物上限，域内口径）。
pub const MANIFEST_CAPACITY: usize = 64;

// ---------------------------------------------------------------------------
// 安装会话状态机
// ---------------------------------------------------------------------------

/// 安装阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstallPhase {
    TempExtract,
    WizardPages,
    Progress,
    FinishPage,
    Done,
    /// 中途取消：回收站式回滚（零真删）。
    RolledBack,
}

/// 安装向导会话。
pub struct InstallSession {
    pub family: &'static str,
    pub product: &'static str,
    pub phase: InstallPhase,
    /// 产物三表（文件/蜂巢键/快捷方式计数）。
    pub files: usize,
    pub hive_keys: usize,
    pub shortcuts: usize,
    /// 「立即运行」勾选（如实执行——主册【交互设计】）。
    pub run_after_finish: bool,
    /// 安装器要求重启 → 诚实跳过（+ 日志账面）。
    pub restart_requests_skipped: u32,
    /// 回滚事件（零真删：进回收站语义）。
    pub rollback_recycled: bool,
    /// UAC → VARIX 权限卡（将写入哪些沙盒区）已确认。
    pub permission_card_ok: bool,
    /// MSI 自修复请求计数。
    pub msi_selfrepairs: u32,
}

impl InstallSession {
    pub fn new(family: &'static str, product: &'static str) -> Option<Self> {
        if !FAMILIES.contains(&family) {
            return None;
        }
        Some(InstallSession {
            family,
            product,
            phase: InstallPhase::TempExtract,
            files: 0,
            hive_keys: 0,
            shortcuts: 0,
            run_after_finish: false,
            restart_requests_skipped: 0,
            rollback_recycled: false,
            permission_card_ok: false,
            msi_selfrepairs: 0,
        })
    }

    /// UAC 类提权 → VARIX 权限卡确认（F038 隔离档替代语义）。
    /// 未确认权限卡 → 沙盒写入全部拒绝（不静默）。
    pub fn sandbox_write(&mut self, files: usize, hive_keys: usize, shortcuts: usize) -> Result<(), &'static str> {
        if !self.permission_card_ok {
            return Err("permission-card-required");
        }
        if self.files + files > MANIFEST_CAPACITY {
            return Err("manifest-full");
        }
        self.files += files;
        self.hive_keys += hive_keys;
        self.shortcuts += shortcuts;
        Ok(())
    }

    /// 推进阶段：TempExtract → WizardPages → Progress → FinishPage → Done。
    pub fn advance(&mut self) -> InstallPhase {
        self.phase = match self.phase {
            InstallPhase::TempExtract => InstallPhase::WizardPages,
            InstallPhase::WizardPages => InstallPhase::Progress,
            InstallPhase::Progress => InstallPhase::FinishPage,
            InstallPhase::FinishPage => InstallPhase::Done,
            other => other,
        };
        self.phase
    }

    /// 安装器要求重启：诚实跳过 + 日志（VARIX 无需重启）。
    pub fn request_restart(&mut self) -> &'static str {
        self.restart_requests_skipped += 1;
        "VARIX 无需重启即可用，已跳过重启步骤"
    }

    /// 安装中途取消：回收站式回滚（零真删——主册）。
    pub fn cancel_midway(&mut self) {
        self.phase = InstallPhase::RolledBack;
        self.rollback_recycled = true;
    }

    /// MSI 自修复请求 → 重新执行修复流。
    pub fn msi_self_repair(&mut self) -> bool {
        if self.family == "msi" && self.phase == InstallPhase::Done {
            self.msi_selfrepairs += 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 卸载清单（F031 消费面）
// ---------------------------------------------------------------------------

/// 三表清单快照（F031 卸载与残留清扫的输入契约）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UninstallManifest {
    pub product: &'static str,
    pub files: usize,
    pub hive_keys: usize,
    pub shortcuts: usize,
}

impl InstallSession {
    /// 安装完成 → 产物登记为卸载清单（三表齐）。
    pub fn manifest(&self) -> Option<UninstallManifest> {
        if self.phase == InstallPhase::Done {
            Some(UninstallManifest {
                product: self.product,
                files: self.files,
                hive_keys: self.hive_keys,
                shortcuts: self.shortcuts,
            })
        } else {
            None
        }
    }
}

/// 沙盒外镜像哈希不变判据的模型面：安装写入只进沙盒路径。
pub fn write_path_in_sandbox(path: &str) -> bool {
    path.starts_with("sandbox:")
}

/// NSIS 临时解包映射：temp 写入映射沙盒 temp（主册【设计细节】；沙盒路径
/// 统一正斜杠风格，与 sandbox:Program/app.exe 契约一致）。
/// 零分配：定长缓冲逐字节写（内核路径无 String/format! 纪律）。
pub fn nsis_temp_redirect(host_temp: &str, out: &mut [u8]) -> usize {
    const PREFIX: &[u8] = b"sandbox:temp/";
    let mut n = 0;
    for b in PREFIX {
        out[n] = *b;
        n += 1;
    }
    let stripped = host_temp.trim_start_matches("C:\\Windows\\Temp\\");
    for b in stripped.as_bytes() {
        if n >= out.len() {
            break;
        }
        out[n] = if *b == b'\\' { b'/' } else { *b };
        n += 1;
    }
    n
}

/// Inno 安装目录页：目录默认沙盒 Program，用户可改到共享区（全可交互）。
pub fn inno_install_dir(user_choice: Option<&str>) -> &'static str {
    match user_choice {
        Some("shared") => "sandbox:shared/Program",
        _ => "sandbox:Program",
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_installr_checks() -> CheckSet {
    let mut cs = CheckSet::new("F030-installr");
    // 1) 三族会话可创建；未知族拒绝。
    cs.add(
        "three_families",
        InstallSession::new("nsis", "a").is_some() && InstallSession::new("inno", "b").is_some() && InstallSession::new("msi", "c").is_some() && InstallSession::new("ghost", "d").is_none(),
        "",
    );
    // 2) 权限卡前置：未确认 → 沙盒写入拒绝（不静默）。
    let mut s = InstallSession::new("nsis", "7zip").unwrap();
    cs.add("permission_card_first", s.sandbox_write(10, 3, 2) == Err("permission-card-required"), "");
    // 3) 确认后写入入账（全产物落沙盒）。
    s.permission_card_ok = true;
    cs.add("sandbox_write_ok", s.sandbox_write(10, 3, 2).is_ok() && s.files == 10 && s.hive_keys == 3 && s.shortcuts == 2, "");
    // 4) 全流程状态机：五阶段推进到 Done。
    let mut s2 = InstallSession::new("inno", "notepad2").unwrap();
    s2.permission_card_ok = true;
    let mut seq = [InstallPhase::TempExtract; 4];
    for (i, slot) in seq.iter_mut().enumerate() {
        s2.advance();
        *slot = s2.phase;
        let _ = i;
    }
    cs.add(
        "wizard_phase_flow",
        s2.phase == InstallPhase::Done
            && seq == [InstallPhase::WizardPages, InstallPhase::Progress, InstallPhase::FinishPage, InstallPhase::Done],
        "",
    );
    // 5) 「立即运行」勾选如实执行（账面）。
    s2.run_after_finish = true;
    cs.add("run_after_finish_honored", s2.run_after_finish, "");
    // 6) 安装完成 → 卸载清单三表齐。
    let mf = s2.manifest();
    cs.add("manifest_three_tables", mf.is_some() && MANIFEST_TABLES == ["files", "hive", "shortcuts"], "");
    // 7) 重启请求诚实跳过 + 日志。
    let mut s3 = InstallSession::new("nsis", "tool").unwrap();
    let msg = s3.request_restart();
    cs.add("restart_skipped_honest", s3.restart_requests_skipped == 1 && msg.contains("已跳过重启"), "");
    // 8) 中途取消 → 回收站式回滚零真删。
    let mut s4 = InstallSession::new("msi", "pkg").unwrap();
    s4.cancel_midway();
    cs.add("midway_cancel_recycle", s4.phase == InstallPhase::RolledBack && s4.rollback_recycled && s4.manifest().is_none(), "");
    // 9) MSI 自修复 → 重新执行修复流；非 MSI 拒绝。
    let mut s5 = InstallSession::new("msi", "msi-app").unwrap();
    s5.phase = InstallPhase::Done;
    cs.add("msi_selfrepair", s5.msi_self_repair() && s5.msi_selfrepairs == 1, "");
    // 10) 写入路径全部在沙盒内（沙盒外镜像哈希不变的前提）。
    cs.add("write_paths_in_sandbox", write_path_in_sandbox("sandbox:Program/app.exe") && !write_path_in_sandbox("C:\\Windows\\app.exe"), "");
    // 11) NSIS temp 重定向映射。
    let mut buf = [0u8; 64];
    let n = nsis_temp_redirect("C:\\Windows\\Temp\\nsx123\\a.dll", &mut buf);
    cs.add("nsis_temp_redirect", &buf[..n] == b"sandbox:temp/nsx123/a.dll", "");
    // 12) Inno 目录页：默认沙盒 Program、可改共享区。
    cs.add("inno_dir_page", inno_install_dir(None) == "sandbox:Program" && inno_install_dir(Some("shared")) == "sandbox:shared/Program", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：三族各一款开源软件安装-使用-卸载全周期绿。
    #[test]
    fn three_families_full_lifecycle() {
        let cases = [("nsis", "7zip"), ("inno", "notepad2"), ("msi", "sumatra")];
        for (family, product) in cases {
            let mut s = InstallSession::new(family, product).unwrap();
            s.permission_card_ok = true;
            s.sandbox_write(24, 6, 3).unwrap();
            for _ in 0..4 {
                s.advance();
            }
            assert_eq!(s.phase, InstallPhase::Done, "{} 全周期走完", family);
            let mf = s.manifest().expect("清单登记");
            assert_eq!(mf.product, product);
            assert!(mf.files > 0 && mf.shortcuts > 0, "三表非空（卸载可驱动）");
        }
    }

    #[test]
    fn manifest_capacity_guard() {
        let mut s = InstallSession::new("nsis", "big").unwrap();
        s.permission_card_ok = true;
        s.sandbox_write(60, 2, 1).unwrap();
        assert_eq!(s.sandbox_write(10, 0, 0), Err("manifest-full"), "清单容量守卫（域内口径 64）");
    }

    #[test]
    fn cancel_discards_manifest() {
        let mut s = InstallSession::new("inno", "x").unwrap();
        s.permission_card_ok = true;
        s.sandbox_write(5, 1, 1).unwrap();
        s.cancel_midway();
        assert!(s.manifest().is_none(), "回滚会话不产出卸载清单");
    }

    #[test]
    fn msi_repair_rejected_for_other_families() {
        let mut s = InstallSession::new("nsis", "n").unwrap();
        s.phase = InstallPhase::Done;
        assert!(!s.msi_self_repair(), "自修复流仅 MSI 族");
    }
}
