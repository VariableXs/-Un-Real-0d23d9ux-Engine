//! boot-select.json 配置读取（双域总案·阶段0 步骤2 / 任务4）。
//!
//! 共享分区上的 `boot-select.json` 是两系统共写的引导配置单一事实源。
//! 本模块是它的内核侧读取面：
//!
//! - **路径参数注入**：共享分区路径 `SHARED_BOOT_SELECT_PATH` 与读取函数
//!   都作为参数注入（`load(reader, path)`）。真盘 FS（任务17/18）落地后
//!   只需提供读取实现，解析与容错零改动；当前目标态无盘面 → 传入
//!   `None` reader 即如实走内置默认（不假装读到了配置）。
//! - **手写最小 JSON 子集**：不引第三方；对象/字符串/整数/布尔/null/
//!   未知值跳过（数组与嵌套对象按深度上限跳过）。
//! - **容错三层**（总案口径）：
//!   1. 字段缺失 → 该字段用默认值；
//!   2. 字段类型错/值域非法 → 该字段用默认值（其余字段照常生效）；
//!   3. 整体损坏（结构破坏/深度超限/尾随垃圾）→ 全部内置默认并上报
//!      `CfgSource::Reset`，调用方 kwarn 并画「配置已重置」角标。
//! - **前向兼容**：未知字段一律忽略。
//! - **零 panic**：无切片越界、无算术溢出（u64/i64 饱和运算），
//!   引导期没有 panic 的余地。
//!
//! # 全字段默认值表（入档）
//!
//! | 字段               | 类型    | 默认      | 说明 |
//! |--------------------|---------|-----------|------|
//! | `default_entry`    | string  | `variable`| 倒计时归零进入的项：variable/windows/last |
//! | `timeout_sec`      | integer | `5`       | 倒计时秒；0=静默立即走默认项；负数→默认；>60→钳到60 |
//! | `show_menu`        | bool    | `true`    | false=静默走默认项（等价 timeout=0 语义） |
//! | `last_boot`        | string  | `variable`| 上次实际进入的系统；`last` 语义据此解析 |
//! | `windows_bootnext` | integer/null | null | 部署脚本探测的 Windows 引导项号 0..=0xFFFF |
//! | `handoff`          | bool    | `true`    | A 卡加载完交接给 Windows 上的 Variable（需求 2）；false=落内核自绘 ushell |
//! | `handoff_target`   | string  | `internal`| 交接目标：internal=内置盘 Windows；usb=U 盘 Windows（S1.3 双系统） |
//! | `usb_windows_esp_guid` | string | null   | U 盘 ESP 分区 GUID；`handoff_target=usb` 时按「设备路径含该 GUID」匹配固件项 |
//!
//! 词表映射：配置词表（variable/windows/last）→ 菜单词表（varix/windows/uefi）
//! 由 `BootCfg::resolve_default_entry` 完成；`uefi` 不进配置词表（固件设置
//! 不是双域常态项）。

/// 共享分区上引导配置的契约路径（SHARED 目录契约，总案·阶段1）。
pub const SHARED_BOOT_SELECT_PATH: &str = "/boot-select.json";

/// `default_entry` / `last_boot` 的词表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryWord {
    Variable,
    Windows,
    Last,
}

impl EntryWord {
    fn from_json(s: &str) -> Option<EntryWord> {
        match s {
            "variable" => Some(EntryWord::Variable),
            "windows" => Some(EntryWord::Windows),
            "last" => Some(EntryWord::Last),
            _ => None,
        }
    }
}

/// `last_boot` 只记录实际进入的系统（variable/windows）；`last` 无意义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastBoot {
    Variable,
    Windows,
}

impl LastBoot {
    fn from_json(s: &str) -> Option<LastBoot> {
        match s {
            "variable" => Some(LastBoot::Variable),
            "windows" => Some(LastBoot::Windows),
            _ => None,
        }
    }
}

/// 解析成功的配置（字段级容错已套用默认值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootCfg {
    pub default_entry: EntryWord,
    pub timeout_sec: u32,
    pub show_menu: bool,
    pub last_boot: LastBoot,
    pub windows_bootnext: Option<u16>,
    /// A 卡（variable/varix）加载完是否交接给 Windows 上的 Variable（需求 2）。
    /// 内核里跑不了 Tauri，"进入 Variable"= 写 BootNext 进 Windows 由那边自启。
    pub handoff: bool,
    /// 交接目标（S1.3/S1.5）：internal=内置盘 Windows（默认），usb=U 盘 Windows。
    pub handoff_target: crate::bootopt::HandoffTarget,
    /// U 盘 ESP 分区 GUID（EFI 字节序）；`handoff_target=usb` 时按
    /// 「设备路径含该 GUID」匹配固件项。缺省/解析失败 = None（usb 目标
    /// 无 GUID 时 handoff 如实拒绝，绝不蒙一个内置盘项）。
    pub usb_windows_esp_guid: Option<[u8; 16]>,
}

impl BootCfg {
    /// 内置默认（上表；与 bootopt 的 DEFAULT_* 常量同源同值）。
    pub fn defaults() -> BootCfg {
        BootCfg {
            default_entry: EntryWord::Variable,
            timeout_sec: crate::bootopt::DEFAULT_TIMEOUT_SECS,
            show_menu: true,
            last_boot: LastBoot::Variable,
            windows_bootnext: None,
            handoff: crate::bootopt::DEFAULT_HANDOFF_TO_VARIABLE,
            handoff_target: crate::bootopt::DEFAULT_HANDOFF_TARGET,
            usb_windows_esp_guid: None,
        }
    }

    /// 配置词表 → 菜单词表（bootopt id）。`last` 按 `last_boot` 解析。
    pub fn resolve_default_entry(&self) -> &'static str {
        match self.default_entry {
            EntryWord::Variable => "varix",
            EntryWord::Windows => "windows",
            EntryWord::Last => match self.last_boot {
                LastBoot::Variable => "varix",
                LastBoot::Windows => "windows",
            },
        }
    }

    /// 菜单可见性：show_menu=false 即静默（timeout=0 语义）。
    pub fn menu_visible(&self) -> bool {
        self.show_menu && self.timeout_sec > 0
    }
}

/// 配置来源。调用方据此决定是否 kwarn + 画「配置已重置」角标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgSource {
    /// 无读取通道（目标态无盘面）或文件不存在：内置默认，静默。
    BuiltIn,
    /// 解析成功（含字段级容错）。
    Parsed,
    /// 整体损坏：全部内置默认，必须如实上报。
    Reset,
}

/// 配置读取函数：把 `path` 的内容读进调用方缓冲区，返回读取的字节数。
/// None=文件不存在/无通道/超出缓冲（读取失败如实上报，不假装读到）。
/// 零分配契约：内核侧无堆字符串，缓冲区由调用方提供。
pub type ConfigReader<'a> = &'a mut dyn FnMut(&str, &mut [u8]) -> Option<usize>;

/// 顶层入口：注入读取函数、共享分区路径与读取缓冲。
/// - `None` reader（当前目标态无盘面）→ 内置默认；
/// - reader 返回 None / 0 字节（文件不存在，首次启动常态）→ 内置默认，静默；
/// - 读到字节 → `parse`（三层容错归口）。
pub fn load(reader: Option<ConfigReader>, path: &str, buf: &mut [u8]) -> (BootCfg, CfgSource) {
    match reader {
        None => (BootCfg::defaults(), CfgSource::BuiltIn),
        Some(read) => match read(path, buf) {
            None | Some(0) => (BootCfg::defaults(), CfgSource::BuiltIn),
            Some(n) => parse(&buf[..n]),
        },
    }
}

/// 解析三层容错归口：整体损坏 → 内置默认 + `Reset`。
pub fn parse(bytes: &[u8]) -> (BootCfg, CfgSource) {
    match Parser::new(bytes).document() {
        Ok(p) => (p, CfgSource::Parsed),
        Err(()) => (BootCfg::defaults(), CfgSource::Reset),
    }
}

/// 目标态读取适配：从引导卷模块（limine.conf `module_path` 挂载的
/// boot-select.json）读配置到调用方缓冲。真盘 FS（任务17/18）落地后，
/// 把 SHARED 分区读取实现同样适配成 `ConfigReader` 即可，解析零改动。
/// 契约：模块缺失/超缓冲 → None（如实静默，走内置默认）。
pub fn read_via_limine(path: &str, buf: &mut [u8]) -> Option<usize> {
    let bytes = crate::limine::module_by_path(path)?;
    if bytes.len() > buf.len() {
        return None;
    }
    buf[..bytes.len()].copy_from_slice(bytes);
    Some(bytes.len())
}

// ---------------------------------------------------------------------------
// 手写最小 JSON 子集解析器
// ---------------------------------------------------------------------------

/// 未知值跳过时的嵌套深度上限。本配置词表没有任何嵌套结构，
/// 4 层只为了宽松容纳前向兼容的浅层未知字段；更深一律视为损坏。
const MAX_DEPTH: usize = 4;

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    // 已知字段（None=未出现→保持默认）
    f_default_entry: Option<EntryWord>,
    f_timeout_sec: Option<u32>,
    f_show_menu: Option<bool>,
    f_last_boot: Option<LastBoot>,
    f_bootnext: Option<Option<u16>>,
    f_handoff: Option<bool>,
    f_handoff_target: Option<crate::bootopt::HandoffTarget>,
    f_usb_guid: Option<[u8; 16]>,
}

impl<'a> Parser<'a> {
    fn new(b: &'a [u8]) -> Parser<'a> {
        let mut i = 0;
        // UTF-8 BOM 容忍
        if b.len() >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF {
            i = 3;
        }
        Parser {
            b,
            i,
            f_default_entry: None,
            f_timeout_sec: None,
            f_show_menu: None,
            f_last_boot: None,
            f_bootnext: None,
            f_handoff: None,
            f_handoff_target: None,
            f_usb_guid: None,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), ()> {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    /// 顶层文档：`{...}` 后只允许空白。
    fn document(&mut self) -> Result<BootCfg, ()> {
        self.eat(b'{')?;
        self.members()?;
        self.skip_ws();
        if self.i != self.b.len() {
            return Err(()); // 尾随垃圾 = 整体损坏
        }
        Ok(self.apply())
    }

    fn members(&mut self) -> Result<(), ()> {
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1; // 空对象：全默认
            return Ok(());
        }
        loop {
            let key = self.string()?;
            self.eat(b':')?;
            self.value_for(&key)?;
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        return Err(()); // 尾逗号非本子集 → 整体损坏
                    }
                }
                Some(b'}') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(()),
            }
        }
    }

    /// 已知字段定向解析；未知字段整体跳过；类型错→该字段保持默认。
    fn value_for(&mut self, key: &str) -> Result<(), ()> {
        match key {
            "default_entry" => self.field(|p| {
                p.string().ok().and_then(|s| EntryWord::from_json(&s)).map(FieldVal::Entry)
            }),
            "last_boot" => self.field(|p| {
                p.string().ok().and_then(|s| LastBoot::from_json(&s)).map(FieldVal::Last)
            }),
            "timeout_sec" => self.field(|p| p.uint_field().map(FieldVal::Timeout)),
            "show_menu" => self.field(|p| p.bool_field().map(FieldVal::Menu)),
            "windows_bootnext" => self.field(|p| p.u16_field().map(FieldVal::BootNext)),
            "handoff" => self.field(|p| p.bool_field().map(FieldVal::Handoff)),
            "handoff_target" => self.field(|p| {
                p.string()
                    .ok()
                    .and_then(crate::bootopt::HandoffTarget::from_json)
                    .map(FieldVal::Target)
            }),
            "usb_windows_esp_guid" => self.field(|p| {
                p.string()
                    .ok()
                    .and_then(crate::bootnext::parse_guid_text)
                    .map(FieldVal::UsbGuid)
            }),
            _ => self.skip_value(),
        }
    }

    /// 字段解析骨架：解析失败→跳过该值并保持默认；成功→登记。
    /// （类型错不升级为整体损坏——容错第 2 层。）
    fn field<F>(&mut self, f: F) -> Result<(), ()>
    where
        F: FnOnce(&mut Parser<'a>) -> Option<FieldVal>,
    {
        let mark = self.i;
        match f(self) {
            Some(v) => {
                self.set(v);
                Ok(())
            }
            None => {
                self.i = mark;
                self.skip_value()
            }
        }
    }

    fn set(&mut self, v: FieldVal) {
        match v {
            FieldVal::Entry(w) => self.f_default_entry = Some(w),
            FieldVal::Last(l) => self.f_last_boot = Some(l),
            FieldVal::Timeout(t) => self.f_timeout_sec = Some(t),
            FieldVal::Menu(m) => self.f_show_menu = Some(m),
            FieldVal::BootNext(b) => self.f_bootnext = Some(b),
            FieldVal::Handoff(h) => self.f_handoff = Some(h),
            FieldVal::Target(t) => self.f_handoff_target = Some(t),
            FieldVal::UsbGuid(g) => self.f_usb_guid = Some(g),
        }
    }

    /// "default_entry"/"last_boot" 的字符串字段值。
    fn string(&mut self) -> Result<&'a str, ()> {
        self.skip_ws();
        if self.peek() != Some(b'"') {
            return Err(());
        }
        self.i += 1;
        let start = self.i;
        while let Some(c) = self.peek() {
            match c {
                b'"' => {
                    let s = &self.b[start..self.i];
                    self.i += 1;
                    // 词表全 ASCII；含转义/非ASCII 一律按词表不匹配处理，
                    // 这里直接拒绝非 ASCII 字节，交给词表 fallback。
                    return core::str::from_utf8(s).map_err(|_| ());
                }
                b'\\' => return Err(()), // 词表值不含转义；转义→类型错→默认
                _ => self.i += 1,
            }
        }
        Err(())
    }

    /// 整数（仅十进制，可有负号；饱和读取）。
    fn integer(&mut self) -> Result<i64, ()> {
        self.skip_ws();
        let neg = if self.peek() == Some(b'-') {
            self.i += 1;
            true
        } else {
            false
        };
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            return Err(());
        }
        // 数字后若紧跟字母/. 等（如 1.5、10x）→ 非本子集 → 类型错
        if let Some(c) = self.peek() {
            if c == b'.' || c.is_ascii_alphabetic() {
                return Err(());
            }
        }
        let mut v: i64 = 0;
        for &c in &self.b[start..self.i] {
            v = v.saturating_mul(10).saturating_add((c - b'0') as i64);
        }
        Ok(if neg { -v } else { v })
    }

    fn uint_field(&mut self) -> Option<u32> {
        let v = self.integer().ok()?;
        if v < 0 {
            return None; // 负值语义非法 → 默认（容错第 2 层）
        }
        // 先饱和到 u32 再钳值域，避免截断回绕
        let t = if v > u32::MAX as i64 { u32::MAX } else { v as u32 };
        Some(crate::bootopt::BootOptions::clamp_timeout(t))
    }

    fn bool_field(&mut self) -> Option<bool> {
        self.skip_ws();
        if self.b[self.i..].starts_with(b"true") {
            self.i += 4;
            Some(true)
        } else if self.b[self.i..].starts_with(b"false") {
            self.i += 5;
            Some(false)
        } else {
            None
        }
    }

    fn u16_field(&mut self) -> Option<Option<u16>> {
        match self.integer() {
            Ok(v) if (0..=0xFFFF).contains(&v) => Some(Some(v as u16)),
            _ => None, // 负数/超域/类型错 → 默认 None
        }
    }

    fn literal(&mut self, lit: &[u8]) -> Result<(), ()> {
        if self.b[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(())
        }
    }

    /// 跳过任意值（未知字段/类型错的值）：字符串/数字/true/false/null/
    /// 对象/数组（深度上限 MAX_DEPTH，超限=整体损坏）。
    fn skip_value(&mut self) -> Result<(), ()> {
        self.skip_ws();
        match self.peek().ok_or(())? {
            b'"' => self.skip_string(),
            b'{' | b'[' => self.skip_nested(0),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            b'-' | b'0'..=b'9' => self.skip_number(),
            _ => Err(()),
        }
    }

    fn skip_string(&mut self) -> Result<(), ()> {
        self.i += 1; // 开引号
        let mut esc = false;
        while let Some(c) = self.peek() {
            self.i += 1;
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                return Ok(());
            }
        }
        Err(())
    }

    fn skip_number(&mut self) -> Result<(), ()> {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.i += 1;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn skip_nested(&mut self, depth: usize) -> Result<(), ()> {
        if depth > MAX_DEPTH {
            return Err(()); // 深度炸弹 → 整体损坏
        }
        let open = self.peek().ok_or(())?;
        let close = if open == b'{' { b'}' } else { b']' };
        self.i += 1;
        loop {
            self.skip_ws();
            match self.peek().ok_or(())? {
                c if c == close => {
                    self.i += 1;
                    return Ok(());
                }
                b'"' => self.skip_string()?,
                b'{' | b'[' => self.skip_nested(depth + 1)?,
                b',' | b':' => self.i += 1,
                _ => {
                    // 标量原子：数字/true/false/null 的任一前缀
                    if self.skip_value().is_err() {
                        return Err(());
                    }
                }
            }
        }
    }

    /// 字段汇总：缺失/非法字段保持默认（容错第 1、2 层）。
    fn apply(&self) -> BootCfg {
        let mut cfg = BootCfg::defaults();
        if let Some(w) = self.f_default_entry {
            cfg.default_entry = w;
        }
        if let Some(t) = self.f_timeout_sec {
            cfg.timeout_sec = t;
        }
        if let Some(m) = self.f_show_menu {
            cfg.show_menu = m;
        }
        if let Some(l) = self.f_last_boot {
            cfg.last_boot = l;
        }
        if let Some(b) = self.f_bootnext {
            cfg.windows_bootnext = b;
        }
        if let Some(h) = self.f_handoff {
            cfg.handoff = h;
        }
        if let Some(t) = self.f_handoff_target {
            cfg.handoff_target = t;
        }
        if let Some(g) = self.f_usb_guid {
            cfg.usb_windows_esp_guid = Some(g);
        }
        cfg
    }
}

/// `value_for` 里五类字段值的统一载体（`field_word` 泛型回填用）。
enum FieldVal {
    Entry(EntryWord),
    Last(LastBoot),
    Timeout(u32),
    Menu(bool),
    BootNext(Option<u16>),
    Handoff(bool),
    Target(crate::bootopt::HandoffTarget),
    UsbGuid([u8; 16]),
}

// field_word 的闭包返回 T，但 set 需要 FieldVal——用一个小适配：
// （T = FieldVal 的各变体由 value_for 的闭包先包好）

// ---------------------------------------------------------------------------
// 与 bootopt 的合并（优先级：cmdline 显式 > 共享配置 > 内置默认）
// ---------------------------------------------------------------------------

/// 有效引导选项：cmdline 显式指定的字段不被配置文件覆盖。
pub fn effective(cmdline: crate::bootopt::BootOptions, cfg: &BootCfg, src: CfgSource) -> crate::bootopt::BootOptions {
    let mut o = cmdline;
    if src != CfgSource::Parsed {
        return o; // 内置/损坏：全部维持 cmdline（损坏角标另行走 bootselect 徽标）
    }
    if !o.customized_timeout {
        o.timeout_secs = if cfg.menu_visible() {
            cfg.timeout_sec
        } else {
            0
        };
    }
    if !o.customized_entry {
        o.default_entry = cfg.resolve_default_entry();
    }
    if !o.customized_handoff {
        o.handoff_to_variable = cfg.handoff;
    }
    if !o.customized_handoff_target {
        o.handoff_target = cfg.handoff_target;
    }
    // GUID 只来自配置（cmdline 不携带长 GUID）；config 缺席/损坏时维持 None。
    o.usb_windows_esp_guid = cfg.usb_windows_esp_guid;
    o
}

// ---------------------------------------------------------------------------
// 宿主测试（对齐总案步骤2验收：三种脏数据 + 值域用例 + 1000 组随机 fuzz）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;
    use std::vec::Vec;

    fn parse_ok(s: &str) -> BootCfg {
        let (cfg, src) = parse(s.as_bytes());
        assert_eq!(src, CfgSource::Parsed);
        cfg
    }

    #[test]
    fn defaults_table() {
        let d = BootCfg::defaults();
        assert_eq!(d.default_entry, EntryWord::Variable);
        assert_eq!(d.timeout_sec, 5);
        assert!(d.show_menu);
        assert_eq!(d.last_boot, LastBoot::Variable);
        assert_eq!(d.windows_bootnext, None);
        assert!(d.menu_visible());
        assert_eq!(d.resolve_default_entry(), "varix");
        assert!(d.handoff, "需求 2：交接默认开");
    }

    #[test]
    fn handoff_field_parses_and_defaults() {
        // 缺字段 → 默认开
        assert!(parse_ok("{}").handoff);
        // 显式 false
        assert!(!parse_ok("{\"handoff\": false}").handoff);
        // 显式 true（前向兼容：老配置没有这个键，补上也不炸）
        assert!(parse_ok("{\"handoff\": true}").handoff);
        // 类型错（字符串）→ 该字段保持默认，其余字段照常生效（容错第 2 层）
        let c = parse_ok("{\"handoff\": \"no\", \"timeout_sec\": 9}");
        assert!(c.handoff, "类型错必须回落默认而不是当成 false");
        assert_eq!(c.timeout_sec, 9, "同文档其余字段不受影响");
        // 未知字段照旧忽略
        assert!(parse_ok("{\"handoff_typo\": false}").handoff);
    }

    #[test]
    fn handoff_target_parse_and_merge() {
        use crate::bootopt::{BootOptions, HandoffTarget};
        let (cfg, src) = parse(
            b"{\"handoff_target\": \"usb\", \"usb_windows_esp_guid\": \"{636786cb-e967-49f6-b0df-7608909d1f11}\"}",
        );
        assert_eq!(src, CfgSource::Parsed);
        assert_eq!(cfg.handoff_target, HandoffTarget::Usb);
        assert!(cfg.usb_windows_esp_guid.is_some(), "合法 GUID 必须解析进字段");
        // 合并：cmdline 未显式 → 配置生效；GUID 随配置透传
        let o = effective(BootOptions::default(), &cfg, src);
        assert_eq!(o.handoff_target, HandoffTarget::Usb);
        assert_eq!(o.usb_windows_esp_guid, cfg.usb_windows_esp_guid);
        // cmdline 显式 internal → 覆盖配置
        let o2 = effective(BootOptions::from_cmdline("handoff_target=internal"), &cfg, src);
        assert_eq!(o2.handoff_target, HandoffTarget::Internal);
        // 配置损坏 → 维持默认 internal + GUID None
        let o3 = effective(BootOptions::default(), &cfg, CfgSource::Reset);
        assert_eq!(o3.handoff_target, HandoffTarget::Internal);
        assert!(o3.usb_windows_esp_guid.is_none());
    }

    #[test]
    fn handoff_target_invalid_values_fall_back() {
        use crate::bootopt::{BootOptions, HandoffTarget};
        // 词表外目标 → 字段级回落默认（容错第 2 层，不升级为整体损坏）
        let (cfg, src) = parse(b"{\"handoff_target\": \"floppy\"}");
        assert_eq!(src, CfgSource::Parsed);
        assert_eq!(cfg.handoff_target, HandoffTarget::Internal);
        // GUID 非法（长度不足）→ 字段级回落 None
        let (cfg2, _) = parse(b"{\"handoff_target\": \"usb\", \"usb_windows_esp_guid\": \"636786cb\"}");
        assert_eq!(cfg2.handoff_target, HandoffTarget::Usb);
        assert!(cfg2.usb_windows_esp_guid.is_none(), "非法 GUID 必须回落 None");
        // 默认（键缺失）→ internal
        let (cfg3, _) = parse(b"{\"timeout_sec\": 5}");
        assert_eq!(cfg3.handoff_target, HandoffTarget::Internal);
        assert!(cfg3.usb_windows_esp_guid.is_none());
    }

    #[test]
    fn handoff_merge_priority_cmdline_wins() {
        use crate::bootopt::BootOptions;
        let (cfg, src) = parse(b"{\"handoff\": false}");
        assert_eq!(src, CfgSource::Parsed);
        // 无 cmdline 显式值 → 用共享配置
        let o = effective(BootOptions::default(), &cfg, src);
        assert!(!o.handoff_to_variable, "共享配置的 false 必须生效");
        // cmdline 显式 handoff=1 → 覆盖共享配置
        let o2 = effective(BootOptions::from_cmdline("handoff=1"), &cfg, src);
        assert!(o2.handoff_to_variable, "cmdline 显式值优先");
        // 配置整体损坏 → 维持 cmdline（不把 Reset 当成"配置说了 false"）
        let o3 = effective(BootOptions::default(), &cfg, CfgSource::Reset);
        assert!(o3.handoff_to_variable);
    }

    #[test]
    fn clean_full_document() {
        let cfg = parse_ok(
            r#"{"default_entry":"windows","timeout_sec":12,"show_menu":true,
                "last_boot":"windows","windows_bootnext":2,"unknown_future":42}"#,
        );
        assert_eq!(cfg.default_entry, EntryWord::Windows);
        assert_eq!(cfg.timeout_sec, 12);
        assert_eq!(cfg.last_boot, LastBoot::Windows);
        assert_eq!(cfg.windows_bootnext, Some(2));
        assert_eq!(cfg.resolve_default_entry(), "windows");
    }

    /// 容错第 1 层：字段缺失 → 默认。
    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let cfg = parse_ok(r#"{"show_menu":false}"#);
        assert_eq!(cfg.default_entry, EntryWord::Variable);
        assert_eq!(cfg.timeout_sec, 5);
        assert!(!cfg.show_menu);
        assert_eq!(cfg.windows_bootnext, None);
    }

    #[test]
    fn empty_object_is_all_defaults() {
        let cfg = parse_ok("{}");
        assert_eq!(cfg, BootCfg::defaults());
    }

    /// 容错第 2 层：类型错 → 该字段默认，其余字段照常生效。
    #[test]
    fn wrong_type_field_falls_back_per_field() {
        let cfg = parse_ok(
            r#"{"default_entry":42,"timeout_sec":"abc","show_menu":1,
                "windows_bootnext":"x","last_boot":true,"timeout_foo":1}"#,
        );
        assert_eq!(cfg.default_entry, EntryWord::Variable);
        assert_eq!(cfg.timeout_sec, 5);
        assert!(cfg.show_menu);
        assert_eq!(cfg.windows_bootnext, None);
        assert_eq!(cfg.last_boot, LastBoot::Variable);
    }

    /// 容错第 3 层：整体损坏 → 内置默认 + Reset。
    #[test]
    fn corrupt_documents_reset_to_defaults() {
        for bad in [
            "{",                 // 未闭合
            "}{",                // 结构颠倒
            "[1,2,3]",           // 顶层不是对象
            "not json at all",   // 纯文本
            r#"{"a":}"#,         // 值缺失
            r#"{"timeout_sec":5}x"#, // 尾随垃圾
            r#"{"a":{"b":{"c":{"d":{"e":{"f":{"g":{"h":{"i":1}}}}}}}}}"#, // 深度炸弹
            "\"just a string\"",
            "",
        ] {
            let (cfg, src) = parse(bad.as_bytes());
            assert_eq!(src, CfgSource::Reset, "case: {:?}", bad);
            assert_eq!(cfg, BootCfg::defaults(), "case: {:?}", bad);
        }
    }

    /// timeout 值域：0 合法；负值→默认；超大→钳 60。
    #[test]
    fn timeout_value_domain() {
        assert_eq!(parse_ok(r#"{"timeout_sec":0}"#).timeout_sec, 0);
        assert_eq!(parse_ok(r#"{"timeout_sec":60}"#).timeout_sec, 60);
        assert_eq!(parse_ok(r#"{"timeout_sec":99999}"#).timeout_sec, 60);
        assert_eq!(parse_ok(r#"{"timeout_sec":-5}"#).timeout_sec, 5);
        assert_eq!(parse_ok(r#"{"timeout_sec":1.5}"#).timeout_sec, 5);
        assert!(!parse_ok(r#"{"timeout_sec":0}"#).menu_visible());
    }

    #[test]
    fn show_menu_false_means_silent() {
        let cfg = parse_ok(r#"{"show_menu":false,"timeout_sec":30}"#);
        assert!(!cfg.menu_visible());
    }

    #[test]
    fn bootnext_value_domain() {
        assert_eq!(parse_ok(r#"{"windows_bootnext":0}"#).windows_bootnext, Some(0));
        assert_eq!(
            parse_ok(r#"{"windows_bootnext":65535}"#).windows_bootnext,
            Some(65535)
        );
        assert_eq!(
            parse_ok(r#"{"windows_bootnext":65536}"#).windows_bootnext,
            None
        );
        assert_eq!(
            parse_ok(r#"{"windows_bootnext":-1}"#).windows_bootnext,
            None
        );
        assert_eq!(
            parse_ok(r#"{"windows_bootnext":null}"#).windows_bootnext,
            None
        );
    }

    /// 前向兼容：未知字段（含嵌套对象/数组）整体忽略。
    #[test]
    fn unknown_fields_are_ignored() {
        let cfg = parse_ok(
            r#"{"future_map":{"a":[1,2,{"b":"c"}]},"future_arr":[true,null,"x"],
                "default_entry":"last"}"#,
        );
        assert_eq!(cfg.default_entry, EntryWord::Last);
    }

    #[test]
    fn last_resolves_through_last_boot() {
        assert_eq!(
            parse_ok(r#"{"default_entry":"last","last_boot":"windows"}"#).resolve_default_entry(),
            "windows"
        );
        assert_eq!(
            parse_ok(r#"{"default_entry":"last"}"#).resolve_default_entry(),
            "varix"
        );
    }

    /// load 注入：无 reader / 文件不存在 / 读到内容 三路径。
    #[test]
    fn load_reader_injection() {
        let (cfg, src) = load(None, SHARED_BOOT_SELECT_PATH, &mut []);
        assert_eq!(src, CfgSource::BuiltIn);
        assert_eq!(cfg, BootCfg::defaults());

        let (_cfg, src) = load(Some(&mut |_, _| None), "/boot-select.json", &mut [0u8; 128]);
        assert_eq!(src, CfgSource::BuiltIn); // 文件不存在=首次启动常态，静默

        let mut rbuf = [0u8; 128];
        let (cfg, src) = load(
            Some(&mut |p: &str, b: &mut [u8]| {
                assert_eq!(p, "/boot-select.json"); // 路径参数确实注入
                let bytes = br#"{"timeout_sec":9}"#;
                b[..bytes.len()].copy_from_slice(bytes);
                Some(bytes.len())
            }),
            "/boot-select.json",
            &mut rbuf,
        );
        assert_eq!(src, CfgSource::Parsed);
        assert_eq!(cfg.timeout_sec, 9);
    }

    /// effective 合并：cmdline 显式 > 配置 > 内置；损坏/内置不动 cmdline。
    #[test]
    fn effective_merge_priority() {
        use crate::bootopt::BootOptions;
        let cfg = parse_ok(r#"{"timeout_sec":12,"default_entry":"windows"}"#);

        // cmdline 未定制 → 配置生效
        let o = effective(BootOptions::default(), &cfg, CfgSource::Parsed);
        assert_eq!(o.timeout_secs, 12);
        assert_eq!(o.default_entry, "windows");

        // cmdline 显式 → 配置被忽略
        let cmd = BootOptions::from_cmdline("boot_timeout=7 boot_default=uefi");
        let o = effective(cmd, &cfg, CfgSource::Parsed);
        assert_eq!(o.timeout_secs, 7);
        assert_eq!(o.default_entry, "uefi");

        // 损坏 → 维持 cmdline 原样（角标与 kwarn 由调用方负责）
        let o = effective(BootOptions::default(), &BootCfg::defaults(), CfgSource::Reset);
        assert_eq!(o.timeout_secs, 5);
        assert_eq!(o.default_entry, "varix");
    }

    /// fuzz：1000 组随机字节不 panic，输出恒为合法 BootCfg。
    /// （总案验收：1000 组随机字节 fuzz 不崩溃；样本命中可复现种子。）
    #[test]
    fn fuzz_1000_random_byte_groups() {
        let mut seed: u64 = 0x4A1B_2C3D_5E6F_7081;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as u64
        };
        for _ in 0..1000u32 {
            let len = (next() % 64) as usize;
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..len {
                bytes.push((next() & 0xFF) as u8);
            }
            let (cfg, _src) = parse(&bytes); // 只断言不 panic / 无 UB
            let _ = cfg.timeout_sec;
        }
        // 结构化 fuzz：合法骨架内嵌随机垃圾
        for _ in 0..500u32 {
            let garbage: String = (0..8)
                .map(|_| b"abc\":{},[]0123456789"[(next() % 19) as usize] as char)
                .collect();
            let doc = format!("{{\"timeout_sec\":{},\"x\":\"{}\"}}", next() % 100, garbage);
            let _ = parse(doc.as_bytes());
        }
    }

    /// BOM 容忍与空白容忍。
    #[test]
    fn bom_and_whitespace() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"\n  { \"timeout_sec\" : 8 }  \n");
        let (cfg, src) = parse(&bytes);
        assert_eq!(src, CfgSource::Parsed);
        assert_eq!(cfg.timeout_sec, 8);
    }
}
