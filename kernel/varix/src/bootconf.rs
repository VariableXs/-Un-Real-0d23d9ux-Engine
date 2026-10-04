//! WP-101 · limine.conf 逐字段固化与解析（MD2 篇 1.3，B-102/B-105 底座）。
//!
//! 配置是引导链的"法律"（MD3 WP-101 施工要点第一条）：本模块是 limine.conf
//! 的**唯一权威解析面**——菜单灰显（WD-003）、闸门三条件的目标存在判定、
//! OneShot 的 default_entry 改写，全部吃这里吐出的结构，绝不各自摸文本。
//!
//! # 定案口径（MD2 篇 1.3）
//!
//! - `timeout: 5`：菜单停留五秒，倒计时归零选默认条目，任意键停表（WD-005）。
//! - 双条目：`/kernel/varix`（VARIX 本体）+ `/Windows 11 (USB)`（交接目标，
//!   指向 WINESP 分区 GUID `636786cb-e967-49f6-b0df-7608909d1f11` 内的
//!   Windows 引导文件）。第二个条目的**存在性就是防自锁闸门校验的一部分**。
//! - 配置头部 `interface_version` 与 `hash:` 注释字段：配置被意外篡改时
//!   启动画面警告（不阻断启动，但"配置校验失败"计入闸门判定）。
//! - 条目注释 `comment:` 字段：人类可读版本说明，菜单不显示、磁盘可查。
//!
//! # 纪律
//!
//! 纯逻辑 + 固定容量数组（返回值按值携带，无 static、无 alloc、无数据竞争
//! ——并发调用各持独立副本，宿主测试天然安全）。解析器对**任何输入字节
//! 串**不 panic——配置文件是磁盘上的外部输入，fuzz 规则与全部解析器一致
//! （tests/fuzz.rs 同款）。

use crate::bootchain::hash_bytes;

/// 配置解析常量：条目上限 16（Limine 场景双条目起步，裕量按 8 倍给）。
pub const MAX_ENTRIES: usize = 16;
/// 单行上限 512 字节：limine.conf 的合法行远短于此，超长行视为损坏证据。
pub const MAX_LINE: usize = 512;
/// 单条目注释上限 128 字节。
pub const MAX_COMMENT: usize = 128;
/// 问题登记上限 8 条（超出记 truncated，不静默丢弃）。
pub const MAX_ISSUES: usize = 8;

/// 配置内建 interface_version（MD2 篇 1.3：固定协议版本）。
pub const INTERFACE_VERSION: &str = "varix-bli-1";

/// WINESP 分区 GUID 文本（MD2 篇 1.3 定案的交接目标分区）。
pub const WINESP_GUID_TEXT: &str = "636786cb-e967-49f6-b0df-7608909d1f11";

/// 一个引导条目的解析结果（字段借用配置文本，生命周期随输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// 条目名（行首 `/` 后到行尾，原样保留；Limine 菜单显示名）。
    pub name: &'a str,
    /// `protocol:` 子键（`limine` / `efi` / 其他原样）。
    pub protocol: &'a str,
    /// `kernel_path:` 或 `path:` 子键的值（引导文件位置）。
    pub path: &'a str,
    /// `comment:` 子键（人类可读版本说明；缺省空串）。
    pub comment: &'a str,
}

impl<'a> Entry<'a> {
    /// 该条目是否为 VARIX 内核本体（协议判据；名字是给人看的，可改）。
    pub fn is_varix(&self) -> bool {
        self.protocol == "limine"
    }

    /// 该条目是否形似交接目标（Windows 条目）：efi 协议 + 名字含 windows。
    pub fn is_windows_target(&self) -> bool {
        self.protocol == "efi" && ascii_contains_ci(self.name, "windows")
    }
}

/// ASCII 大小写不敏感子串（条目名/路径都是 ASCII 域，够用且零分配）。
fn ascii_contains_ci(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    for i in 0..=(h.len() - n.len()) {
        let mut ok = true;
        for k in 0..n.len() {
            if h[i + k].to_ascii_lowercase() != n[k].to_ascii_lowercase() {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

/// 配置问题（B-105 注入矩阵的判定词表；每条带定位行号便于诊断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfIssue {
    /// 超长行（> MAX_LINE）：被截断处理，视为潜在损坏。
    OversizedLine { line: usize },
    /// 条目行缺 protocol 子键：该条目菜单渲染为灰显（WD-003 的形态之一）。
    EntryMissingProtocol { line: usize },
    /// protocol 值未知（既非 limine 也非 efi）：记问题但不拒收——
    /// 未知协议条目按"不可引导"灰显，绝不当可引导条目用。
    UnknownProtocol { line: usize },
    /// 条目 path 为空：同上灰显。
    EntryMissingPath { line: usize },
    /// hash 字段存在但格式非法（非 8 位十六进制）。
    MalformedHash { line: usize },
    /// default_entry 缺席或指到不存在的条目下标。
    DefaultEntryOutOfRange { line: usize },
    /// 双条目契约被破坏：找不到 VARIX 条目或 Windows 交接条目。
    ContractMissing { detail: &'static str },
    /// 条目数超过 MAX_ENTRIES：多出的条目不参与闸门判定（并登记）。
    EntriesOverflow { line: usize },
}

/// 拥有型解析产出（固定容量，按值返回——零静态状态，并发安全）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootConf<'a> {
    /// `timeout:` 值；缺失/非法 → None（调用方回落 DEFAULT_TIMEOUT_SECS）。
    pub timeout: Option<u32>,
    /// `default_entry:` 值（条目下标）；缺失 → None（Limine 默认第一条）。
    pub default_entry: Option<usize>,
    /// 条目列表（保持 conf 行序；解析即序，不重排）。
    pub entries: [Option<Entry<'a>>; MAX_ENTRIES],
    /// 实际条目数（`entries[..n_entries]` 有效）。
    pub n_entries: usize,
    /// `hash:` 头部注释字段值（配置自身校验和，十六进制 8 位）；缺失 → None。
    pub declared_hash: Option<u32>,
    /// 问题登记（`issues[..n_issues]` 有效）。
    pub issues: [Option<ConfIssue>; MAX_ISSUES],
    pub n_issues: usize,
    /// 问题是否超过容量（true = issues 列表不完整）。
    pub issues_truncated: bool,
}

impl<'a> BootConf<'a> {
    /// 第 i 个条目（越界返回 None——外部下标一律防御）。
    pub fn entry(&self, i: usize) -> Option<Entry<'a>> {
        if i < self.n_entries {
            self.entries[i]
        } else {
            None
        }
    }

    /// 找 VARIX 内核条目（双条目契约的第一条；按解析序取第一个命中）。
    pub fn varix_entry(&self) -> Option<Entry<'a>> {
        (0..self.n_entries).map(|i| self.entry(i)).flatten().find(|e| e.is_varix())
    }

    /// 找 Windows 交接条目（闸门条件一的目标）。
    pub fn windows_entry(&self) -> Option<Entry<'a>> {
        (0..self.n_entries)
            .map(|i| self.entry(i))
            .flatten()
            .find(|e| e.is_windows_target())
    }

    /// 问题迭代器。
    pub fn issue_list(&self) -> impl Iterator<Item = ConfIssue> + '_ {
        self.issues[..self.n_issues].iter().map(|o| o.unwrap())
    }
}

/// 解析配置文本。对任何输入不 panic；契约破坏登记为问题而非拒收——
/// 灰显与人话提示（WD-003）负责把"坏了"讲给用户，解析器负责把"哪里坏了"
/// 讲给诊断。
pub fn parse(text: &str) -> BootConf<'_> {
    let mut conf = BootConf {
        timeout: None,
        default_entry: None,
        entries: [None; MAX_ENTRIES],
        n_entries: 0,
        declared_hash: None,
        issues: [None; MAX_ISSUES],
        n_issues: 0,
        issues_truncated: false,
    };
    let issue_push = |conf: &mut BootConf, iss: ConfIssue| {
        if conf.n_issues < MAX_ISSUES {
            conf.issues[conf.n_issues] = Some(iss);
            conf.n_issues += 1;
        } else {
            conf.issues_truncated = true;
        }
    };

    let mut cur: Option<usize> = None; // 当前条目下标
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        if raw.len() > MAX_LINE {
            issue_push(&mut conf, ConfIssue::OversizedLine { line: line_no });
        }
        // 截断处理：只取前 512 字节参与解析（后续判定照常）。
        let line: &str = {
            let b = raw.as_bytes();
            let cut = b.len().min(MAX_LINE);
            match core::str::from_utf8(&b[..cut]) {
                Ok(s) => s,
                Err(_) => "", // 非法 UTF-8 行按空行处理（字段判定自然失败）
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            // 条目名行：`/name`（Limine 8 格式）。
            if let Some(name) = trimmed.strip_prefix('/') {
                if conf.n_entries < MAX_ENTRIES {
                    conf.entries[conf.n_entries] = Some(Entry {
                        name,
                        protocol: "",
                        path: "",
                        comment: "",
                    });
                    cur = Some(conf.n_entries);
                    conf.n_entries += 1;
                } else {
                    issue_push(&mut conf, ConfIssue::EntriesOverflow { line: line_no });
                    cur = None;
                }
            } else if let Some(v) = trimmed.strip_prefix("timeout:") {
                conf.timeout = v.trim().parse::<u32>().ok();
            } else if let Some(v) = trimmed.strip_prefix("default_entry:") {
                match v.trim().parse::<usize>() {
                    Ok(n) => conf.default_entry = Some(n),
                    Err(_) => {
                        issue_push(
                            &mut conf,
                            ConfIssue::DefaultEntryOutOfRange { line: line_no },
                        );
                    }
                }
            } else if let Some(v) = trimmed.strip_prefix("hash:") {
                match parse_hash_field(v.trim()) {
                    Some(h) => conf.declared_hash = Some(h),
                    None => issue_push(&mut conf, ConfIssue::MalformedHash { line: line_no }),
                }
            }
            // 其他顶层键（serial 等）Limine 自管，本模块不解析不校验。
        } else {
            // 子键行：挂到当前条目。
            let Some(ci) = cur else { continue };
            let (key, val) = match trimmed.split_once(':') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (trimmed, ""),
            };
            if let Some(e) = conf.entries[ci].as_mut() {
                match key {
                    "protocol" => e.protocol = val,
                    "kernel_path" | "path" => e.path = val,
                    "comment" => {
                        // 注释按上限截断（纯装饰字段，超长不记损坏）。
                        let b = val.as_bytes();
                        let cut = b.len().min(MAX_COMMENT);
                        e.comment = core::str::from_utf8(&b[..cut]).unwrap_or("");
                    }
                    _ => {}
                }
            }
        }
    }

    // 条目级校验：缺 protocol / 未知 protocol / 缺 path。
    for i in 0..conf.n_entries {
        let Some(e) = conf.entry(i) else { continue };
        if e.protocol.is_empty() {
            issue_push(&mut conf, ConfIssue::EntryMissingProtocol { line: 0 });
        } else if e.protocol != "limine" && e.protocol != "efi" {
            issue_push(&mut conf, ConfIssue::UnknownProtocol { line: 0 });
        }
        if e.path.is_empty() {
            issue_push(&mut conf, ConfIssue::EntryMissingPath { line: 0 });
        }
    }
    // 双条目契约：VARIX 条目与 Windows 交接条目至少各一。
    if conf.varix_entry().is_none() {
        issue_push(&mut conf, ConfIssue::ContractMissing { detail: "no varix entry" });
    }
    if conf.windows_entry().is_none() {
        issue_push(&mut conf, ConfIssue::ContractMissing { detail: "no windows entry" });
    }
    // default_entry 越界复核（前面只查了语法）。
    if let Some(d) = conf.default_entry {
        if d >= conf.n_entries {
            issue_push(&mut conf, ConfIssue::DefaultEntryOutOfRange { line: 0 });
        }
    }
    conf
}

/// `hash:` 字段解析：8 位十六进制 → u32（配置自身校验和的展示口径）。
fn parse_hash_field(s: &str) -> Option<u32> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

/// 计算配置文本的 FNV-1a 校验和（`hash:` 字段的生产口径，篇 1.3）。
/// 与 [`crate::bootchain::hash_bytes`] 同源：账本里只有一种哈希。
pub fn conf_hash(text: &str) -> u32 {
    hash_bytes(text.as_bytes())
}

/// 灰显渲染决策（WD-003 / B-102）：条目在菜单里的形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryRender {
    /// 正常可选。
    Ready,
    /// 灰显 + 人话原因（写入菜单条目副标题与诊断事件）。
    Grey(GreyReason),
}

/// 灰显原因词表（人话口径，三要素文案的引导层形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreyReason {
    /// 条目缺 protocol 或 path：菜单亮得出但引导器装不动。
    Incomplete,
    /// 目标文件探测失败：交接目标不在了（闸门条件一的红灯）。
    TargetMissing,
}

/// WD-003 判定：给定条目与目标文件探针结果，产出渲染形态。
///
/// 探针由调用方注入（引导期读 WINESP 的实现面在存储域；菜单只需要判定），
/// 本函数是纯逻辑——20 组注入矩阵（B-105）与宿主测试都打这里。
pub fn entry_render(entry: &Entry, target_file_present: Option<bool>) -> EntryRender {
    if entry.protocol.is_empty() || entry.path.is_empty() {
        return EntryRender::Grey(GreyReason::Incomplete);
    }
    if entry.is_windows_target() && target_file_present == Some(false) {
        return EntryRender::Grey(GreyReason::TargetMissing);
    }
    EntryRender::Ready
}

// ---------------------------------------------------------------------------
// 生成器（安装器/更新器单源；B-1302 镜像单源的引导面）
// ---------------------------------------------------------------------------

/// 渲染 limine.conf 文本。双条目契约写死（MD2 篇 1.3），字段顺序即本文件
/// 解析顺序——生成与解析同文件对表，漂移即测试可见。
pub fn render(varix_kernel_path: &str, windows_target_path: &str, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let push = |s: &str, out: &mut [u8], n: &mut usize| {
        for &b in s.as_bytes() {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    push("# interface_version: ", out, &mut n);
    push(INTERFACE_VERSION, out, &mut n);
    push("\n", out, &mut n);
    push("timeout: 5\n", out, &mut n);
    push("default_entry: 0\n", out, &mut n);
    push("/kernel/varix\n", out, &mut n);
    push("    protocol: limine\n", out, &mut n);
    push("    kernel_path: boot():", out, &mut n);
    push(varix_kernel_path, out, &mut n);
    push("\n    comment: VARIX kernel (STAR I)\n", out, &mut n);
    push("/Windows 11 (USB)\n", out, &mut n);
    push("    protocol: efi\n", out, &mut n);
    push("    path: ", out, &mut n);
    push(windows_target_path, out, &mut n);
    push("\n    comment: handoff target (WINESP ", out, &mut n);
    push(WINESP_GUID_TEXT, out, &mut n);
    push(")\n", out, &mut n);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仓库根 limine.conf 的编译期嵌入（WP-101 固化版）：测试与磁盘实形
    /// 逐字节对表——文件漂移即红，宿主侧守卫「配置是法律」。
    const REPO_CONF: &str = include_str!("../../../limine.conf");

    #[test]
    fn live_conf_parses_with_contract_intact() {
        let c = parse(REPO_CONF);
        assert_eq!(c.timeout, Some(5));
        // WP-101 固化字段（MD2 篇 1.3）：默认项 = varix（下标 0）。
        assert_eq!(c.default_entry, Some(0));
        assert_eq!(c.n_entries, 2, "双条目契约");
        assert!(c.varix_entry().is_some());
        let win = c.windows_entry().expect("交接条目必须存在");
        assert!(
            win.path.contains(WINESP_GUID_TEXT),
            "交接路径必须锚定 WINESP GUID"
        );
        assert!(
            win.path.contains("bootmgfw.efi"),
            "交接路径必须指向 Windows 引导文件"
        );
        // comment 字段固化在场（人类可读版本说明，磁盘可查）。
        assert!(win.comment.contains("handoff target"), "Windows 条目 comment 缺失");
        assert_eq!(c.n_issues, 0, "健康配置零问题: {:?}", c.issues);
        assert!(!c.issues_truncated);
    }

    /// WP-101 固化闭环：文件头 `# hash:` 注释必须与正文实算一致（口径：
    /// 首个非注释非空行起至 EOF，头部注释不参与）。这是闸门「目标可信」
    /// 在配置面的宿主侧复核——磁盘文件与内核解析面永远对表。
    #[test]
    fn frozen_conf_hash_meta_consistent() {
        assert!(
            REPO_CONF.contains("# interface_version: varix-bli-1"),
            "interface_version 元数据缺失"
        );
        // 跳头：统计头部注释/空行的字节长度，得到正文起点。
        let body_off = REPO_CONF
            .split_inclusive('\n')
            .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
            .map(|l| l.len())
            .sum::<usize>();
        let body = &REPO_CONF[body_off..];
        assert!(body.starts_with("timeout:"), "跳头口径落点错误");
        // 声明值：`# hash: xxxxxxxx` 标记行（与元数据解释行前缀不同源）。
        let declared = REPO_CONF
            .lines()
            .find_map(|l| l.strip_prefix("# hash: "))
            .map(|s| s.trim().to_string());
        let expected = format!("{:08x}", conf_hash(body));
        assert_eq!(
            declared.as_deref(),
            Some(expected.as_str()),
            "hash 注释与正文不一致（正文改动后未回填元数据）"
        );
    }

    #[test]
    fn hash_field_roundtrip_and_malformed() {
        let text = "hash: deadbeef\ntimeout: 5\n/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n";
        let c = parse(text);
        assert_eq!(c.declared_hash, Some(0xdead_beef));
        // 生成→哈希→解析 的闭环自证（宿主测试允许堆分配拼 hash 字段）。
        let mut buf = [0u8; 1024];
        let n = render("/kernel/varix", "guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi", &mut buf);
        let rendered = core::str::from_utf8(&buf[..n]).unwrap();
        let h = conf_hash(rendered);
        assert_eq!(parse_hash_field(&format!("{h:08x}")), Some(h));
        // 非法 hash 字段 → 问题登记、不拒收整体
        let bad = "hash: xyz\ntimeout: 5\n/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n";
        let c2 = parse(bad);
        assert!(c2
            .issue_list()
            .any(|i| matches!(i, ConfIssue::MalformedHash { .. })));
        assert_eq!(c2.declared_hash, None);
    }

    #[test]
    fn grey_logic_wd003() {
        let ok = Entry {
            name: "Windows 11 (USB)",
            protocol: "efi",
            path: "guid(...):/EFI/Microsoft/Boot/bootmgfw.efi",
            comment: "",
        };
        let broken = Entry {
            name: "Windows 11 (USB)",
            protocol: "",
            path: "",
            comment: "",
        };
        assert_eq!(entry_render(&ok, Some(true)), EntryRender::Ready);
        assert_eq!(
            entry_render(&ok, Some(false)),
            EntryRender::Grey(GreyReason::TargetMissing)
        );
        // 探针不可用（None）→ 不灰显（如实而非猜测；警告层另行提示）
        assert_eq!(entry_render(&ok, None), EntryRender::Ready);
        assert_eq!(
            entry_render(&broken, Some(true)),
            EntryRender::Grey(GreyReason::Incomplete)
        );
    }

    #[test]
    fn injected_corruption_produces_issues_not_panics() {
        // B-105 注入样组：缺 protocol / 未知 protocol / 契约缺失 / 越界。
        let no_proto = "/kernel/varix\n    kernel_path: boot():/kernel/varix\n";
        let c = parse(no_proto);
        assert!(c
            .issue_list()
            .any(|i| matches!(i, ConfIssue::EntryMissingProtocol { .. })));
        assert!(c
            .issue_list()
            .any(|i| matches!(i, ConfIssue::ContractMissing { .. })));

        let bad_proto = "/kernel/varix\n    protocol: grub\n    kernel_path: boot():/k\n/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n";
        let c2 = parse(bad_proto);
        assert!(c2
            .issue_list()
            .any(|i| matches!(i, ConfIssue::UnknownProtocol { .. })));

        let no_win = "/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n";
        let c3 = parse(no_win);
        assert!(c3
            .issue_list()
            .any(|i| matches!(i, ConfIssue::ContractMissing { detail: "no windows entry" })));

        let no_varix = "/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n";
        let c4 = parse(no_varix);
        assert!(c4
            .issue_list()
            .any(|i| matches!(i, ConfIssue::ContractMissing { detail: "no varix entry" })));

        // default_entry 越界（语法合法但下标超条目数）
        let oob = "default_entry: 9\n/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n";
        let c5 = parse(oob);
        assert!(c5
            .issue_list()
            .any(|i| matches!(i, ConfIssue::DefaultEntryOutOfRange { .. })));
        // default_entry 非数字
        let nan = format!("default_entry: banana\n{}", REPO_CONF);
        let c6 = parse(&nan);
        assert!(c6
            .issue_list()
            .any(|i| matches!(i, ConfIssue::DefaultEntryOutOfRange { .. })));
    }

    #[test]
    fn hostile_input_never_panics() {
        // 外部输入全清洗：空串/垃圾/超长/纯空白 一律走问题登记路径。
        for hostile in [
            "",
            "\n\n\n",
            "###",
            "    \t    ",
            "/",
            "/x\n",
            "timeout: banana",
            "timeout: -3",
            "default_entry: -1",
            "hash:",
            "hash: 1234567",
            "/a\n    protocol\n",
            "/a\n\tbogus-key\n",
        ] {
            let _ = parse(hostile); // 不 panic 即达标；issues 有没有都对
        }
        // 超长行不 panic（截断处理 + 问题登记）
        let long = format!("/{}\n", "x".repeat(4096));
        let c = parse(&long);
        assert!(c.issue_list().any(|i| matches!(i, ConfIssue::OversizedLine { .. })));
    }

    #[test]
    fn entries_overflow_is_recorded_not_fatal() {
        // 17 个条目：前 16 登记，第 17 个记 overflow；契约判定基于前 16。
        let mut text = String::new();
        text.push_str("/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n");
        text.push_str("/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n");
        for i in 0..15 {
            text.push_str(&format!("/extra{i}\n    protocol: efi\n    path: /x\n"));
        }
        text.push_str("/overflow17\n    protocol: efi\n    path: /y\n");
        let c = parse(&text);
        assert_eq!(c.n_entries, MAX_ENTRIES);
        assert!(c.issue_list().any(|i| matches!(i, ConfIssue::EntriesOverflow { .. })));
    }

    #[test]
    fn render_then_parse_roundtrip() {
        let mut buf = [0u8; 1024];
        let n = render(
            "/kernel/varix-20260925.elf",
            "guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi",
            &mut buf,
        );
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        let c = parse(text);
        assert_eq!(c.n_entries, 2);
        assert_eq!(c.timeout, Some(5));
        assert_eq!(c.default_entry, Some(0));
        assert!(c.varix_entry().is_some());
        assert!(c.windows_entry().is_some());
        assert_eq!(c.n_issues, 0, "roundtrip 必须零问题: {:?}", c.issues);
        // 输出必须含 interface_version 与 WINESP GUID（篇 1.3 字段级定案）
        assert!(text.contains(INTERFACE_VERSION));
        assert!(text.contains(WINESP_GUID_TEXT));
    }

    #[test]
    fn concurrent_parse_is_racesafe() {
        // 拥有型返回结构的核心自证：并发解析互不干扰（此前 static 缓冲
        // 方案在此测试下必然数据竞争——结构修正后必须全绿）。
        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..100 {
                        let c = parse(REPO_CONF);
                        assert_eq!(c.n_entries, 2);
                        assert_eq!(c.n_issues, 0);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}
