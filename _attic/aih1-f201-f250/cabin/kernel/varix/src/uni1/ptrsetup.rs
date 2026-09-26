//! F444 打印机安装向导 · 完整设计（STAR I 主册 G-I-44）。
//!
//! **判据（主册）**：USB 自动链路用例；手动搜索匹配；驱动来源三态
//! 标注；失败出路引导；测试页一键；与 F289 队列衔接。＋通12。
//!
//! 设计：打印机安装核——两路：自动（USB 插入事件 → 枚举识别 → 驱动
//! 匹配 → 装好通知一条）与手动（内置库列表 + 按型号子串搜索）；驱动
//! 来源三态诚实标注（BuiltIn 内置库 / VendorVendor 厂商包 vxapp / 
//! NeedsManual 需手动装——不假装都支持）；失败归因 → 出路引导（哪找
//! 驱动、怎么装）；测试页一键（装完当场验证）；装好即入 F289 打印
//! 队列登记（衔接判据）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 驱动来源三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverSource {
    /// 内置库。
    BuiltIn,
    /// 厂商包（vxapp 形式）。
    VendorPackage,
    /// 需要手动装（诚实标注——不假装支持）。
    NeedsManual,
}

impl DriverSource {
    pub fn label(self) -> &'static str {
        match self {
            DriverSource::BuiltIn => "驱动：内置库",
            DriverSource::VendorPackage => "驱动：厂商包",
            DriverSource::NeedsManual => "驱动：需手动安装",
        }
    }
}

/// 内置库型号条目。
#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub model: String,
    pub source: DriverSource,
}

/// 安装状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallState {
    NotInstalled,
    Installing,
    Ready,
    Failed,
}

/// 一台打印机。
#[derive(Clone, Debug)]
pub struct Printer {
    pub model: String,
    pub source: DriverSource,
    pub state: InstallState,
    /// 已登记进 F289 打印队列。
    pub queue_registered: bool,
}

/// 打印机安装向导核。
pub struct PrinterWizard {
    pub catalog: Vec<CatalogEntry>,
    pub installed: Vec<Printer>,
    /// USB 自动链路事件账。
    pub auto_events: Vec<&'static str>,
}

impl PrinterWizard {
    pub fn new() -> PrinterWizard {
        PrinterWizard { catalog: Vec::new(), installed: Vec::new(), auto_events: Vec::new() }
    }

    /// 手动搜索：型号子串匹配内置库。
    pub fn search(&self, query: &str) -> Vec<&CatalogEntry> {
        self.catalog
            .iter()
            .filter(|e| e.model.contains(query))
            .collect()
    }

    /// 安装（两路共用落点）：驱动来源三态分流——NeedsManual 不假装装好。
    pub fn install(&mut self, model: &str, source: DriverSource) -> bool {
        if source == DriverSource::NeedsManual {
            return false; // 诚实：需要手动装的不会假装 Ready
        }
        self.installed.push(Printer {
            model: String::from(model),
            source,
            state: InstallState::Installing,
            queue_registered: false,
        });
        true
    }

    /// 安装推进 → Ready + 自动登记 F289 队列。
    pub fn finish(&mut self, model: &str) -> bool {
        match self.installed.iter_mut().find(|p| p.model == model && p.state == InstallState::Installing) {
            Some(p) => {
                p.state = InstallState::Ready;
                p.queue_registered = true; // 与 F289 队列衔接
                true
            }
            None => false,
        }
    }

    /// 失败出路引导：装失败给「哪找驱动、怎么装」的人话。
    pub fn failure_guidance(source: DriverSource) -> &'static str {
        match source {
            DriverSource::BuiltIn => "内置库驱动安装失败——重启系统后重试一次",
            DriverSource::VendorPackage => "厂商包安装失败——到 vx 星图搜索该型号的厂商包重装",
            DriverSource::NeedsManual => "此型号需要厂商驱动——请从厂商官网获取后用 vxapp 安装",
        }
    }

    /// 测试页一键：仅 Ready 且已入队的打印机可打（当场验证装没装好）。
    pub fn print_test_page(&mut self, model: &str) -> bool {
        match self.installed.iter_mut().find(|p| p.model == model) {
            Some(p) if p.state == InstallState::Ready && p.queue_registered => true,
            _ => false,
        }
    }

    /// USB 自动链路：插入 → 识别 → 匹配 → 装好（事件账逐步留痕）。
    pub fn usb_auto(&mut self, _usb_id: &str, model: &str) -> bool {
        self.auto_events.push("usb-插入：已识别新打印设备");
        let matched = self.catalog.iter().find(|e| e.model == model);
        let Some(entry) = matched else {
            self.auto_events.push("usb-匹配失败：内置库无此型号——给出手动安装出路");
            return false;
        };
        self.auto_events.push("usb-驱动匹配：内置库命中");
        if self.install(model, entry.source) {
            self.auto_events.push("usb-安装完成：通知一条（不打断手头工作）");
            self.finish(model)
        } else {
            false
        }
    }
}

pub fn run_ptrsetup_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F444");
    let mut w = PrinterWizard::new();
    w.catalog.push(CatalogEntry { model: String::from("StarJet 2000"), source: DriverSource::BuiltIn });
    w.catalog.push(CatalogEntry { model: String::from("StarJet 3000 Pro"), source: DriverSource::VendorPackage });
    w.catalog.push(CatalogEntry { model: String::from("AncientDot 90"), source: DriverSource::NeedsManual });
    // 手动搜索匹配（子串）。
    set.add(
        "f444-manual-search",
        w.search("StarJet").len() == 2 && w.search("3000")[0].source == DriverSource::VendorPackage && w.search("不存在").is_empty(),
        "",
    );
    // 驱动来源三态标注。
    set.add(
        "f444-source-labels",
        DriverSource::BuiltIn.label() == "驱动：内置库"
            && DriverSource::VendorPackage.label() == "驱动：厂商包"
            && DriverSource::NeedsManual.label() == "驱动：需手动安装",
        "",
    );
    // NeedsManual 不假装装好（诚实判据）。
    set.add("f444-needs-manual-honest", !w.install("AncientDot 90", DriverSource::NeedsManual), "");
    // USB 自动链路（事件账逐步留痕）。
    set.add(
        "f444-usb-auto-chain",
        w.usb_auto("USB\\VID_1234", "StarJet 2000")
            && w.installed[0].state == InstallState::Ready
            && w.auto_events.len() == 3,
        "",
    );
    // F289 队列衔接：Ready 即登记。
    set.add("f444-queue-registered", w.installed[0].queue_registered, "");
    // 测试页一键：Ready 可打；未装完不可打。
    set.add(
        "f444-test-page",
        w.print_test_page("StarJet 2000") && !w.print_test_page("没装的型号"),
        "",
    );
    // 失败出路引导（三态人话齐）。
    set.add(
        "f444-failure-guidance",
        PrinterWizard::failure_guidance(DriverSource::NeedsManual).contains("厂商")
            && !PrinterWizard::failure_guidance(DriverSource::BuiltIn).is_empty(),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_unknown_model_gives_manual_path() {
        let mut w = PrinterWizard::new();
        w.catalog.push(CatalogEntry { model: String::from("StarJet 2000"), source: DriverSource::BuiltIn });
        assert!(!w.usb_auto("USB\\VID_9999", "Ghost 1"), "内置库无此型号：不走假安装");
        assert!(w.auto_events.last().unwrap().contains("手动安装出路"), "失败给诚实出路");
        assert!(w.installed.is_empty());
    }

    #[test]
    fn finish_requires_installing_state() {
        let mut w = PrinterWizard::new();
        assert!(w.install("StarJet 2000", DriverSource::BuiltIn));
        assert!(w.finish("StarJet 2000"));
        assert!(!w.finish("StarJet 2000"), "已完成的不重复 finish");
    }
}
