//! 深化层二 · F140 本地化开放（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】词条 schema 与【设计细节】数字/日期随区域、
//! RTL 前瞻、双语审查（主册 G-D-15）：词条域注册表与重名检测、译文
//! 等义审查器（占位符序列+标签对称）、区域格式规则表（F187 联动）、
//! RTL 影响面评估、两级覆盖率矩阵、补翻清单优先级排序。

use crate::checks::CheckSet;
use crate::stareco::l10nopen::{context_ok, key_parts, placeholders_match};

// ---------------------------------------------------------------------------
// 词条域注册表：域.页面.用途 三级键的域级管理（防命名空间膨胀）
// ---------------------------------------------------------------------------

pub struct DomainRegistry {
    domains: alloc::vec::Vec<&'static str>,
    /// (完整 key) 有序表——重名检测的数据面。
    keys: alloc::vec::Vec<&'static str>,
}

impl DomainRegistry {
    pub fn new() -> DomainRegistry {
        DomainRegistry { domains: alloc::vec::Vec::new(), keys: alloc::vec::Vec::new() }
    }

    pub fn register_domain(&mut self, d: &'static str) -> Result<(), &'static str> {
        if d.is_empty() || d.len() > 16 {
            return Err("域名为空或超长（≤16）");
        }
        if self.domains.contains(&d) {
            return Err("域已注册：一域一注册");
        }
        self.domains.push(d);
        Ok(())
    }

    /// 词条登记：键的域必须已注册 + 键结构三级合法 + 全局唯一。
    pub fn register_key(&mut self, key: &'static str) -> Result<(), &'static str> {
        let Some((dom, _page, _use)) = key_parts(key) else {
            return Err("键必须三级：域.页面.用途");
        };
        if !self.domains.contains(&dom) {
            return Err("域未注册：先注册域再进词条");
        }
        if self.keys.contains(&key) {
            return Err("键重复：同一用途一条词条");
        }
        self.keys.push(key);
        Ok(())
    }

    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// 域内词条清单（补翻清单按域分组的基础）。
    pub fn keys_in_domain(&self, dom: &str) -> alloc::vec::Vec<&'static str> {
        self.keys
            .iter()
            .filter(|k| key_parts(k).map_or(false, |(d, _, _)| d == dom))
            .copied()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 译文等义审查：占位符序列 + 尖括号标签对称（译不过三查不上线）
// ---------------------------------------------------------------------------

/// 译文结构审查：占位符多重集合一致 + 尖括号开闭数一致。
pub fn translation_structure_ok(source: &str, translated: &str) -> bool {
    if !placeholders_match(source, translated) {
        return false;
    }
    let open = translated.matches('<').count();
    let close = translated.matches('>').count();
    open == close
}

/// 上下文审查强化：context 字段必须过基础层四类词表（button/title/desc/error）。
pub fn entry_context_ok(context: &str) -> bool {
    context_ok(context)
}

// ---------------------------------------------------------------------------
// 区域格式规则表（F187 联动：数字/日期随区域）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionFormat {
    /// 2026/09/26。
    YmdSlash,
    /// 26.09.2026。
    DmyDot,
    /// Sep 26, 2026（英文月名）。
    MdyEn,
}

/// 日期格式化（区域规则表驱动——一处一事实：规则只此一份）。
pub fn format_date(y: u32, m: u32, d: u32, region: RegionFormat) -> alloc::string::String {
    match region {
        RegionFormat::YmdSlash => alloc::format!("{}/{:02}/{:02}", y, m, d),
        RegionFormat::DmyDot => alloc::format!("{:02}.{:02}.{}", d, m, y),
        RegionFormat::MdyEn => {
            const MONTHS: [&str; 12] =
                ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            alloc::format!("{} {}, {}", MONTHS[(m as usize - 1).min(11)], d, y)
        }
    }
}

/// 数字千分位（区域分组符：英式逗号 / 德式点——两位足够示范规则表机制）。
pub fn format_number_grouped(n: u64, dot_separator: bool) -> alloc::string::String {
    let s = n.to_string();
    let (sep, group) = if dot_separator { ('.', 3usize) } else { (',', 3usize) };
    let bytes = s.as_bytes();
    let mut out = alloc::string::String::new();
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % group == 0 {
            out.push(sep);
        }
        out.push(*b as char);
    }
    out
}

// ---------------------------------------------------------------------------
// RTL 前瞻评估：布局镜像影响面清单（评估件——不实装镜像）
// ---------------------------------------------------------------------------

/// 需要镜像的布局元素类（RTL 影响面评估的产出清单）。
pub const RTL_MIRROR_SURFACE: [&str; 6] =
    ["side-panel", "back-arrow", "list-rows", "progress-fill", "tab-order", "text-align"];

/// RTL 就绪度：镜像面清单齐 + 词序占位符兼容（{n} 位置无关词序）。
pub fn rtl_readiness(has_mirror_plan: bool, placeholders_position_free: bool) -> Result<u32, &'static str> {
    if !has_mirror_plan {
        return Err("镜像方案缺：RTL 不可上线（前瞻接口保持关闭）");
    }
    if !placeholders_position_free {
        return Err("占位符含词序假设：RTL 译文会错位");
    }
    Ok(RTL_MIRROR_SURFACE.len() as u32)
}

// ---------------------------------------------------------------------------
// 两级覆盖率矩阵（域 × 页面）与补翻清单
// ---------------------------------------------------------------------------

/// 覆盖矩阵单元：域.页面 → (源词条数, 已译数)。
pub struct CoverageCell {
    pub domain: &'static str,
    pub page: &'static str,
    pub total: usize,
    pub translated: usize,
}

impl CoverageCell {
    pub fn bp(&self) -> u32 {
        if self.total == 0 {
            return 10_000; // 空页面视为全覆盖（不出假红）
        }
        ((self.translated.min(self.total) * 10_000) / self.total) as u32
    }
}

/// 补翻清单：未译词条按域分组导出（优先级：错误 > 按钮 > 标题 > 描述——
/// 用户看得见错误的地方先翻）。
pub fn missing_keys(domain: &'static str, all: &[&'static str], translated: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    let mut out: alloc::vec::Vec<&'static str> = all
        .iter()
        .filter(|k| {
            key_parts(k).map_or(false, |(d, _, _)| d == domain) && !translated.contains(k)
        })
        .copied()
        .collect();
    out.sort_by_key(|k| {
        let priority = match k.rsplit('.').next().unwrap_or("") {
            "error" => 0,
            "button" => 1,
            "title" => 2,
            _ => 3,
        };
        (priority, *k)
    });
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F140E_TAG: &str = "stareco-F140-deep2";

pub fn run_f140_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F140E_TAG);

    // 域注册表
    let mut reg = DomainRegistry::new();
    let _ = reg.register_domain("shell");
    let _ = reg.register_domain("settings");
    set.add("f140e domain count", reg.domain_count() == 2, "双域注册");
    set.add("f140e domain dup", reg.register_domain("shell").is_err(), "重复域拒绝");
    set.add("f140e domain long", reg.register_domain("a-very-long-domain-name").is_err(), "超长域拒绝");
    let _ = reg.register_key("shell.taskbar.button.pin");
    set.add("f140e key ok", reg.key_count() == 1, "合法键登记");
    set.add("f140e key unreg domain", reg.register_key("editor.menu.item.cut").is_err(), "未注册域拒绝");
    let _ = reg.register_key("shell.taskbar.button.unpin");
    set.add("f140e key group", reg.keys_in_domain("shell").len() == 2, "域内分组");

    // 等义审查
    set.add(
        "f140e structure ok",
        translation_structure_ok("复制 {n} 个文件", "Copy {n} files"),
        "占位符一致",
    );
    set.add(
        "f140e structure missing",
        !translation_structure_ok("复制 {n} 个文件", "Copy files"),
        "丢占位符判红",
    );
    set.add(
        "f140e structure tag",
        translation_structure_ok("<b>加粗</b> 文本", "<b>bold</b> text"),
        "标签对称放行",
    );
    set.add(
        "f140e structure tag broken",
        !translation_structure_ok("<b>加粗</b>", "<b>bold"),
        "标签不对称判红",
    );
    set.add("f140e context gate", entry_context_ok("button") && !entry_context_ok("whatever"), "上下文四类词表");

    // 区域格式
    set.add(
        "f140e date ymd",
        format_date(2026, 9, 26, RegionFormat::YmdSlash) == "2026/09/26",
        "英式斜杠",
    );
    set.add(
        "f140e date dmy",
        format_date(2026, 9, 26, RegionFormat::DmyDot) == "26.09.2026",
        "德式点分",
    );
    set.add(
        "f140e date mdy",
        format_date(2026, 9, 26, RegionFormat::MdyEn) == "Sep 26, 2026",
        "英文月名",
    );
    set.add(
        "f140e number en",
        format_number_grouped(1_234_567, false) == "1,234,567",
        "英式千分位",
    );
    set.add(
        "f140e number de",
        format_number_grouped(1_234_567, true) == "1.234.567",
        "德式千分位",
    );

    // RTL 前瞻
    set.add(
        "f140e rtl ready",
        rtl_readiness(true, true) == Ok(6),
        "六类镜像面就绪",
    );
    set.add("f140e rtl no mirror", rtl_readiness(false, true).is_err(), "无镜像方案拒绝");
    set.add("f140e rtl word order", rtl_readiness(true, false).is_err(), "词序假设拒绝");

    // 覆盖矩阵与补翻
    let cell = CoverageCell { domain: "shell", page: "taskbar", total: 8, translated: 6 };
    set.add("f140e cell bp", cell.bp() == 7_500, "单元覆盖 75%");
    let empty = CoverageCell { domain: "x", page: "y", total: 0, translated: 0 };
    set.add("f140e cell empty", empty.bp() == 10_000, "空页不出假红");
    let all_keys = ["shell.taskbar.error.nospace", "shell.taskbar.button.pin", "shell.taskbar.title.bar"];
    let done = ["shell.taskbar.button.pin"];
    let missing = missing_keys("shell", &all_keys, &done);
    set.add(
        "f140e missing priority",
        missing == alloc::vec!["shell.taskbar.error.nospace", "shell.taskbar.title.bar"],
        "错误优先于标题（补翻顺序）",
    );
    set.add(
        "f140e missing other domain",
        missing_keys("settings", &all_keys, &done).is_empty(),
        "域外词条不混入",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;
    use crate::stareco::l10nopen::placeholders;

    #[test]
    fn placeholders_seq() {
        assert!(placeholders("{a} 与 {b}").len() == 2);
        assert!(placeholders_match("{a}{b}", "{b}{a}")); // 顺序无关（多重集合）
        assert!(!placeholders_match("{a}", "{c}"));
    }

    #[test]
    fn number_grouping_small() {
        assert_eq!(format_number_grouped(999, false), "999");
        assert_eq!(format_number_grouped(1000, false), "1,000");
        assert_eq!(format_number_grouped(0, true), "0");
    }

    #[test]
    fn month_clamp_safety() {
        // 非法月份被钳制到表内（不 panic——错误边界）
        let s = format_date(2026, 99, 1, RegionFormat::MdyEn);
        assert!(s.contains("2026"));
    }
}
