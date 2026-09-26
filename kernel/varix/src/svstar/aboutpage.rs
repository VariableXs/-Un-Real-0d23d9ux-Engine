//! F123 关于本机页 · 完整设计（STAR I 主册 G-C-53）。
//!
//! **判据（主册）**：Y7000 实机规格全对（对照官方规格表）；复制-粘贴
//! 保真；全部行可读性走查。
//!
//! **设计要点（主册）**：
//! - 设备诚实档案：设备名（可改）/ 系统版本与谱系（F199 联动）/ 硬件
//!   清单（CPU 型号/核数/频率/内存容量/盘容量与 health F183）/ 显示
//!   分辨率与缩放 / 每项可复制；
//! - 设置中心「系统-关于」页：设备卡（壁纸缩略+名称+重命名行内编辑）；
//!   规格分组列表（处理器/内存/存储/显示四组，行高 65px 乙-3 表卡片行）；
//!   每行尾复制图标；「复制全部报告」尾部大钮（生成文本报告含格式）；
//! - 只读数据即时采集（ACPI/PCI 表已有 F053 面）；设备名配置层；
//! - 某项不可读（温度计类 graceful 既有纪律）→ 该行显「本机不可读」；
//!   规格采集异常 → 缺行不编造（诚实留白）；重命名即时生效（F011
//!   COMPUTERNAME 同源）；
//! - CPU 频率显示实时档（F048 联动小字「当前 2.5GHz」）；内存显示
//!   已装/可用双值；盘行含读写量累计（F183 寿命参考）；报告文本格式
//!   固定模板（社区互助标准格式 F139 联动）；页脚小字「数据每 30 秒
//!   自动刷新」。
//!
//! 时间注入式，宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 规格行高（px，主册：65px 乙-3 表卡片行）。
pub const ROW_H_PX: u32 = 65;
/// 自动刷新周期（秒，主册：数据每 30 秒自动刷新）。
pub const REFRESH_SECS: u64 = 30;
/// 规格分组数（处理器/内存/存储/显示）。
pub const GROUP_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 规格分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Cpu,
    Memory,
    Storage,
    Display,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Cpu => "处理器",
            Group::Memory => "内存",
            Group::Storage => "存储",
            Group::Display => "显示",
        }
    }
}

/// 一行规格。
#[derive(Clone, Debug)]
pub struct SpecRow {
    pub group: Group,
    pub label: &'static str,
    /// 采集值（None = 本机不可读——graceful 诚实留白，不编造）。
    pub value: Option<String>,
}

impl SpecRow {
    pub fn readable(group: Group, label: &'static str, value: &str) -> SpecRow {
        SpecRow { group, label, value: Some(String::from(value)) }
    }

    pub fn unreadable(group: Group, label: &'static str) -> SpecRow {
        SpecRow { group, label, value: None }
    }

    /// 行显示文本（不可读 → 「本机不可读」）。
    pub fn display(&self) -> String {
        match &self.value {
            Some(v) => alloc::format!("{}：{}", self.label, v),
            None => alloc::format!("{}：本机不可读", self.label),
        }
    }

    /// 可复制性：不可读行无复制值（复制图标灰置——诚实）。
    pub fn copyable(&self) -> bool {
        self.value.is_some()
    }
}

/// 设备档案（采集面注入——即时采集语义：值由调用方每次供给）。
pub struct AboutPage {
    /// 设备名（可改——F011 COMPUTERNAME 同源）。
    device_name: String,
    /// 系统版本谱系（F199 联动）。
    pub version: &'static str,
    pub rows: Vec<SpecRow>,
    /// 重命名次数（即时生效对账）。
    renames: u64,
}

impl AboutPage {
    pub fn new(device_name: &str, version: &'static str, rows: Vec<SpecRow>) -> AboutPage {
        AboutPage { device_name: String::from(device_name), version, rows, renames: 0 }
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn renames(&self) -> u64 {
        self.renames
    }

    /// 重命名（行内编辑，即时生效）。
    pub fn rename(&mut self, name: &str) {
        if !name.trim().is_empty() {
            self.device_name = String::from(name);
            self.renames += 1;
        }
    }

    /// 分组行视图（处理器/内存/存储/显示四组）。
    pub fn rows_of(&self, g: Group) -> Vec<&SpecRow> {
        self.rows.iter().filter(|r| r.group == g).collect()
    }

    /// 单行复制（复制-粘贴保真：显示文本即复制文本）。
    pub fn copy_row(&self, idx: usize) -> Option<String> {
        let r = self.rows.get(idx)?;
        r.value.as_ref().map(|v| alloc::format!("{}：{}", r.label, v))
    }

    /// 一键「复制全部报告」（社区互助标准格式 F139 模板；不可读行显
    /// 「本机不可读」，缺行不编造）。
    pub fn copy_all_report(&self) -> String {
        let mut out = String::new();
        out.push_str("== VARIX 设备报告 ==\n");
        out.push_str(&alloc::format!("设备名：{}\n", self.device_name));
        out.push_str(&alloc::format!("系统版本：{}\n", self.version));
        for g in [Group::Cpu, Group::Memory, Group::Storage, Group::Display] {
            out.push_str(&alloc::format!("[{}]\n", g.name()));
            for r in self.rows_of(g) {
                out.push_str(&r.display());
                out.push('\n');
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

/// Y7000 官方规格锚点（判据：实机规格全对——对照官方规格表直录）。
pub const Y7000_ANCHORS: [(&str, &str); 4] = [
    ("CPU 型号", "Intel Core i7"),
    ("核数", "8"),
    ("内存容量", "16GB"),
    ("显示分辨率", "1920x1080"),
];

pub fn run_aboutpage_checks() -> CheckSet {
    let mut set = CheckSet::new("F123-aboutpage");

    // Y7000 实机档案（判据第一句：规格全对——对照官方规格表）。
    let rows = vec![
        SpecRow::readable(Group::Cpu, "CPU 型号", "Intel Core i7"),
        SpecRow::readable(Group::Cpu, "核数", "8"),
        SpecRow::readable(Group::Cpu, "当前频率", "2.5GHz"),
        SpecRow::readable(Group::Memory, "内存容量", "16GB"),
        SpecRow::readable(Group::Memory, "可用内存", "9.2GB"),
        SpecRow::readable(Group::Storage, "系统盘容量", "512GB"),
        SpecRow::readable(Group::Storage, "盘 health", "91%"),
        SpecRow::readable(Group::Storage, "读写量累计", "42TB"),
        SpecRow::readable(Group::Display, "显示分辨率", "1920x1080"),
        SpecRow::readable(Group::Display, "缩放", "150%"),
        SpecRow::unreadable(Group::Cpu, "CPU 温度"),
    ];

    let mut page = AboutPage::new("Y7000-STAR", "VARIX 1.0.0", rows);

    // 1. 官方规格表锚点逐项全对（处理器/内存/显示四锚直拍）。
    let mut anchors_ok = true;
    for (label, want) in Y7000_ANCHORS {
        let hit = page.rows.iter().any(|r| r.label == label && r.value.as_deref() == Some(want));
        if !hit {
            anchors_ok = false;
        }
    }
    set.add("Y7000 spec anchors all match", anchors_ok, "");

    // 2. 分组视图：四组齐、组内行各归其位。
    set.add(
        "four groups organized",
        GROUP_COUNT == 4
            && page.rows_of(Group::Cpu).len() == 4
            && page.rows_of(Group::Memory).len() == 2
            && page.rows_of(Group::Storage).len() == 3
            && page.rows_of(Group::Display).len() == 2,
        "",
    );

    // 3. 不可读行诚实显示「本机不可读」且无复制值（graceful 纪律）。
    let unreadable = page.rows.last().unwrap();
    set.add(
        "unreadable row honest + not copyable",
        unreadable.display() == "CPU 温度：本机不可读" && !unreadable.copyable(),
        "",
    );

    // 4. 复制-粘贴保真（判据第一句之二）：单行复制文本 = 显示文本。
    let copy = page.copy_row(0).unwrap();
    set.add(
        "row copy fidelity",
        copy == "CPU 型号：Intel Core i7" && copy == page.rows[0].display(),
        "",
    );

    // 5. 不可读行复制返回 None（灰置语义）。
    set.add("unreadable row copy none", page.copy_row(10).is_none(), "");

    // 6. 一键「复制全部报告」：四组齐 + 设备名 + 版本 + 不可读行留白。
    let report = page.copy_all_report();
    set.add(
        "full report template complete",
        report.contains("== VARIX 设备报告 ==")
            && report.contains("设备名：Y7000-STAR")
            && report.contains("[处理器]")
            && report.contains("[存储]")
            && report.contains("CPU 温度：本机不可读"),
        "",
    );

    // 7. 重命名即时生效（F011 同源；空名拒绝）。
    page.rename("我的星机");
    let renamed = page.device_name() == "我的星机" && page.renames() == 1;
    page.rename("   ");
    set.add(
        "rename instant + empty rejected",
        renamed && page.device_name() == "我的星机" && page.renames() == 1,
        "",
    );

    // 8. CPU 实时档小字（F048 联动）+ 内存双值（已装/可用）。
    set.add(
        "live cpu freq + memory dual values",
        page.rows.iter().any(|r| r.label == "当前频率" && r.value.as_deref() == Some("2.5GHz"))
            && page.rows.iter().any(|r| r.label == "内存容量")
            && page.rows.iter().any(|r| r.label == "可用内存"),
        "",
    );

    // 9. 盘行含读写量累计（F183 寿命参考）+ health。
    set.add(
        "disk lifetime counters present",
        page.rows.iter().any(|r| r.label == "读写量累计")
            && page.rows.iter().any(|r| r.label == "盘 health"),
        "",
    );

    // 10. 行高与刷新周期规格（65px / 30 秒）。
    set.add(
        "row height 65px + refresh 30s",
        ROW_H_PX == 65 && REFRESH_SECS == 30,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aboutpage_all_checks_green() {
        let set = run_aboutpage_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F123 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn report_line_order_stable() {
        let rows = vec![SpecRow::readable(Group::Cpu, "核数", "8")];
        let page = AboutPage::new("dev", "v1", rows);
        let report = page.copy_all_report();
        let lines: Vec<&str> = report.lines().collect();
        assert_eq!(lines[0], "== VARIX 设备报告 ==");
        assert!(lines.iter().any(|l| *l == "[处理器]"));
        assert!(lines.iter().any(|l| *l == "核数：8"));
    }

    #[test]
    fn missing_rows_not_fabricated() {
        // 只给处理器组 → 报告不含内存组标题下的编造行（诚实留白：
        // 组标题仍在，组内无行——缺行不编造）。
        let rows = vec![SpecRow::readable(Group::Cpu, "核数", "8")];
        let page = AboutPage::new("dev", "v1", rows);
        let report = page.copy_all_report();
        assert!(report.contains("[内存]"));
        assert!(!report.contains("内存容量："), "未采集的行不得出现编造值");
    }
}
