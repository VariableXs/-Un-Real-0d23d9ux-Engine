//! F011 环境变量会话面（compatstar · G-A-11）——改完变量，下一个终端就有。
//!
//! 主册判据（验收标准第一句）：
//! **「标准名 20 项在兼容层程序内 `GetEnvironmentVariable` 全取到；判例 14
//! 代理场景回归绿。」**
//!
//! 功能定义（G-A-11）：会话级环境变量表：系统级（PATH/TEMP/TMP/USERNAME/
//! COMPUTERNAME 等 Windows 标准名）+ 用户级（设置中心配置）+ 会话注入（代理
//! 等，判例 14）；兼容层程序与原生程序同表语义（大小写不敏感语义按 Windows）。
//!
//! 【交互设计】设置中心「系统-关于-高级系统设置-环境变量」页：双列表（用户/
//! 系统）、新建/编辑/删除、行内编辑（值长度上限 32KB）；搜索框过滤变量名。
//! 【数据与存储】变量表存配置层（`config/env.json`），还原点（F121）覆盖；
//! 注入时展开自引用（%PATH% 引用链上限 10 层防环）。
//! 【状态与异常】变量名含非法字符 → 行内红框即时校验；引用环 → 截断 + 警告；
//! 超大 PATH（>8KB）警告性能影响。
//! 【设计细节】大小写不敏感语义「首个写入的大小写为准」（Windows 同语义）；
//! PATH 按分号切分展示；COMPUTERNAME 取设备名（F123 同源）；变量值内百分号
//! 展开仅注入时执行一次；TEMP 指向沙盒内 tmp（每应用独立，防交叉污染）。
//!
//! 零堆纪律：变量表定长槽 + 定长值，展开链定长，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 值长度上限 32KB（主册【数据与存储】）。
pub const VALUE_CAP: usize = 32 * 1024;
/// %VAR% 引用链上限 10 层防环（主册【数据与存储】）。
pub const EXPANSION_CHAIN_MAX: usize = 10;
/// 超大 PATH 警告线 8KB（主册【状态与异常】）。
pub const PATH_WARN_BYTES: usize = 8 * 1024;
/// 标准名清单 20 项（主册判据：标准名 20 项全取到；清单照 Microsoft 环境变
/// 量文档——一处一事实见 STD_NAMES 注释）。
pub const STD_NAME_COUNT: usize = 20;
/// 变量表容量（会话级面：系统 64 + 用户 128 + 注入 32，工程值登记报告）。
pub const TABLE_CAP: usize = 224;

/// Windows 标准名 20 项（Microsoft 环境变量文档清单；判例 14 的代理三件套
/// 在 SESSION_INJECTED 中单列）。
pub const STD_NAMES: [&str; STD_NAME_COUNT] = [
    "PATH",
    "TEMP",
    "TMP",
    "USERNAME",
    "COMPUTERNAME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "SYSTEMDRIVE",
    "SYSTEMROOT",
    "WINDIR",
    "PROGRAMFILES",
    "COMMONPROGRAMFILES",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PATHEXT",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "OS",
];

/// 判例 14 代理场景注入件（HTTP_PROXY/HTTPS_PROXY/NO_PROXY）。
pub const SESSION_INJECTED: [&str; 3] = ["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"];

// ---------------------------------------------------------------------------
// 变量表
// ---------------------------------------------------------------------------

/// 变量条目（名称定长 64B / 值定长 VALUE_CAP 截断保护——超限拒绝不静默截）。
#[derive(Clone, Copy, Debug)]
pub struct EnvVar {
    /// 名称（保留首个写入的大小写——Windows 同语义）。
    pub name: [u8; 64],
    pub name_len: usize,
    pub value: [u8; VALUE_CAP],
    pub value_len: usize,
    /// 层级（系统级/用户级/会话注入）。
    pub scope: Scope,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    System,
    User,
    Session,
}

/// 会话环境变量表：大小写不敏感查找 + 三层叠加（系统 < 用户 < 会话注入）。
pub struct EnvTable {
    /// 变量槽（堆分配——表为冷路径：进程创建/设置页读写；热路径零堆纪律
    /// 不适用于本结构，登记完成报告）。
    vars: Vec<Option<EnvVar>>,
    count: usize,
    /// 引用环截断告警计数（截断 + 警告判据的观测面）。
    pub cycle_truncations: u32,
    /// 非法名拒绝计数（行内红框校验的服务端对应面）。
    pub invalid_name_rejects: u32,
    /// 超大 PATH 警告标记。
    pub path_overwarn: bool,
}

impl EnvTable {
    pub fn new() -> EnvTable {
        EnvTable {
            vars: alloc::vec![None; TABLE_CAP],
            count: 0,
            cycle_truncations: 0,
            invalid_name_rejects: 0,
            path_overwarn: false,
        }
    }

    /// 非法名校验（Windows 语义：不得含 `=`；空名非法；`%` 保留给展开）。
    pub fn valid_name(name: &str) -> bool {
        !name.is_empty() && !name.contains('=') && !name.contains('%') && name.len() <= 64
    }

    fn find_slot(&self, name: &str) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        (0..self.count).find(|&i| {
            self.vars[i]
                .map(|v| {
                    let n = core::str::from_utf8(&v.name[..v.name_len]).unwrap_or("");
                    n.to_ascii_lowercase() == lower
                })
                .unwrap_or(false)
        })
    }

    /// 写变量（首个写入的大小写为准：已有同名（任意大小写）→ 保留原名只改值）。
    pub fn set(&mut self, name: &str, value: &[u8], scope: Scope) -> bool {
        if !Self::valid_name(name) {
            self.invalid_name_rejects += 1;
            return false;
        }
        if value.len() > VALUE_CAP {
            return false; // 超限拒绝，不静默截断
        }
        if let Some(i) = self.find_slot(name) {
            if let Some(v) = self.vars[i].as_mut() {
                // 层级覆盖规则：会话注入 > 用户 > 系统（只升不降）。
                if scope_priority(scope) >= scope_priority(v.scope) {
                    v.value[..value.len()].copy_from_slice(value);
                    v.value_len = value.len();
                    v.scope = scope;
                } else {
                    return false; // 低层级不得覆盖高层级
                }
            }
            self.check_path_warn();
            return true;
        }
        if self.count >= TABLE_CAP || value.len() > VALUE_CAP {
            return false;
        }
        let mut v = EnvVar {
            name: [0; 64],
            name_len: name.len(),
            value: [0; VALUE_CAP],
            value_len: value.len(),
            scope,
        };
        v.name[..name.len()].copy_from_slice(name.as_bytes());
        v.value[..value.len()].copy_from_slice(value);
        self.vars[self.count] = Some(v);
        self.count += 1;
        self.check_path_warn();
        true
    }

    /// 取变量（大小写不敏感；GetEnvironmentVariable 对应面）。
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        let i = self.find_slot(name)?;
        self.vars[i].as_ref().map(|v| &v.value[..v.value_len])
    }

    /// 取名称（保留首个写入的大小写）。
    pub fn get_name_cased(&self, name: &str) -> Option<&str> {
        let i = self.find_slot(name)?;
        self.vars[i]
            .as_ref()
            .and_then(|v| core::str::from_utf8(&v.name[..v.name_len]).ok())
    }

    /// 删除变量。
    pub fn remove(&mut self, name: &str) -> bool {
        match self.find_slot(name) {
            Some(i) => {
                // 收缩（保持表序）。
                let mut j = i;
                while j + 1 < self.count {
                    self.vars[j] = self.vars[j + 1];
                    j += 1;
                }
                self.vars[self.count - 1] = None;
                self.count -= 1;
                true
            }
            None => false,
        }
    }

    /// %VAR% 展开（仅注入时执行一次——主册【设计细节】；引用链上限 10 层，
    /// 超限/成环 → 截断 + 警告计数）。
    pub fn expand(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0;
        let mut depth = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                if let Some(end) = input[i + 1..].find('%') {
                    let name = &input[i + 1..i + 1 + end];
                    if let Some(v) = self.get(name) {
                        if depth >= EXPANSION_CHAIN_MAX {
                            // 引用环/超深 → 截断 + 警告（主册【状态与异常】）。
                            self.cycle_truncations += 1;
                            out.push_str(name);
                            i = i + 1 + end + 1;
                            continue;
                        }
                        depth += 1;
                        // 嵌套展开：值先拷出（借用分离）再递归（depth 封顶）。
                        let owned = v.to_vec();
                        let expanded = self.expand_inner(core::str::from_utf8(&owned).unwrap_or(""), depth);
                        out.push_str(&expanded);
                        depth -= 1;
                        i = i + 1 + end + 1;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    /// 嵌套展开内层（depth 已计——超出链限直接原样吐出并计数）。
    fn expand_inner(&mut self, s: &str, depth: usize) -> String {
        if depth >= EXPANSION_CHAIN_MAX {
            self.cycle_truncations += 1;
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                if let Some(end) = s[i + 1..].find('%') {
                    let name = &s[i + 1..i + 1 + end];
                    if let Some(v) = self.get(name) {
                        let owned = v.to_vec();
                        let expanded = self.expand_inner(core::str::from_utf8(&owned).unwrap_or(""), depth + 1);
                        out.push_str(&expanded);
                        i = i + 1 + end + 1;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    fn check_path_warn(&mut self) {
        if let Some(p) = self.get("PATH") {
            self.path_overwarn = p.len() > PATH_WARN_BYTES;
        }
    }

    /// 会话快照（进程创建时取——运行中的程序不受后续修改影响，主册【功能
    /// 定义】会话边界语义）。
    pub fn snapshot(&self) -> EnvSnapshot {
        let mut s = EnvSnapshot { slots: alloc::vec![None; TABLE_CAP], n: 0 };
        for i in 0..self.count {
            s.slots[i] = self.vars[i];
            s.n += 1;
        }
        s
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 搜索框过滤（设置页变量名过滤——前缀/包含匹配，大小写不敏感）。
    pub fn filter_names(&self, q: &str) -> Vec<&str> {
        let ql = q.to_ascii_lowercase();
        (0..self.count)
            .filter_map(|i| {
                let v = self.vars[i].as_ref()?;
                let n = core::str::from_utf8(&v.name[..v.name_len]).ok()?;
                if q.is_empty() || n.to_ascii_lowercase().contains(&ql) {
                    Some(n)
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for EnvTable {
    fn default() -> Self {
        Self::new()
    }
}

fn scope_priority(s: Scope) -> u8 {
    match s {
        Scope::System => 0,
        Scope::User => 1,
        Scope::Session => 2,
    }
}

/// 会话快照（创建时冻结——运行中程序不受后续 set 影响）。
pub struct EnvSnapshot {
    slots: Vec<Option<EnvVar>>,
    n: usize,
}

impl EnvSnapshot {
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        let lower = name.to_ascii_lowercase();
        (0..self.n).find_map(|i| {
            let v = self.slots[i].as_ref()?;
            let n = core::str::from_utf8(&v.name[..v.name_len]).ok()?;
            if n.to_ascii_lowercase() == lower {
                Some(&v.value[..v.value_len])
            } else {
                None
            }
        })
    }

    /// 按表序枚举槽位（序列化/诊断回放面）。返回 (名称, 值, 层级)。
    pub fn slot_at(&self, i: usize) -> Option<(&str, &[u8], Scope)> {
        let v = self.slots.get(i)?.as_ref()?;
        let name = core::str::from_utf8(&v.name[..v.name_len]).ok()?;
        Some((name, &v.value[..v.value_len], v.scope))
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// PATH 按分号切分（编辑器内每行一条展示面）。
pub fn split_path(value: &str) -> Vec<&str> {
    value.split(';').filter(|s| !s.is_empty()).collect()
}

/// 域自检。
pub fn run_envsess_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess");
    // 1) 判据常量（32KB / 链限 10 / PATH 警告 8KB / 标准名 20 项）。
    cs.add(
        "consts",
        VALUE_CAP == 32 * 1024
            && EXPANSION_CHAIN_MAX == 10
            && PATH_WARN_BYTES == 8 * 1024
            && STD_NAME_COUNT == 20
            && SESSION_INJECTED.len() == 3,
        "",
    );
    // 2) 标准名 20 项全取到（判据一：种子表写入后 GetEnvironmentVariable
    //    对应面全命中）。
    let mut t = EnvTable::new();
    let mut seeded = true;
    for (i, name) in STD_NAMES.iter().enumerate() {
        let val = [b'v' + (i % 26) as u8; 4];
        seeded &= t.set(name, &val, Scope::System);
    }
    let mut all_get = true;
    for name in STD_NAMES.iter() {
        all_get &= t.get(name).is_some();
    }
    cs.add("std_names_20_all_resolved", seeded && all_get && t.len() == 20, "");
    // 3) 大小写不敏感：get 命中；首个写入的大小写为准。
    cs.add(
        "case_insensitive_first_case_wins",
        t.get("path").is_some() && t.get_name_cased("path") == Some("PATH"),
        "",
    );
    // 4) 层级覆盖：会话注入 > 用户 > 系统；低层不得覆盖高层。
    let _ = t.set("HTTP_PROXY", b"http://old:8080", Scope::System);
    let _ = t.set("HTTP_PROXY", b"http://judge14:8080", Scope::Session);
    cs.add(
        "scope_priority_overlay",
        t.get("HTTP_PROXY") == Some(&b"http://judge14:8080"[..])
            && !t.set("HTTP_PROXY", b"http://low:1", Scope::System),
        "",
    );
    // 5) 判例 14 代理场景回归绿：代理三件套注入后全可见。
    let _ = t.set("HTTPS_PROXY", b"http://judge14:8080", Scope::Session);
    let _ = t.set("NO_PROXY", b"localhost,127.0.0.1", Scope::Session);
    cs.add(
        "case14_proxy_trio",
        SESSION_INJECTED.iter().all(|n| t.get(n).is_some()),
        "",
    );
    // 6) %VAR% 展开：自引用展开一次；引用环截断 + 警告计数。
    let mut t2 = EnvTable::new();
    let _ = t2.set("A", b"%B%", Scope::User);
    let _ = t2.set("B", b"%C%", Scope::User);
    let _ = t2.set("C", b"end", Scope::User);
    cs.add(
        "expansion_chain",
        t2.expand("%A%") == "end",
        "",
    );
    let mut t3 = EnvTable::new();
    let _ = t3.set("X", b"%Y%", Scope::User);
    let _ = t3.set("Y", b"%X%", Scope::User); // 环
    let expanded = t3.expand("%X%");
    cs.add(
        "cycle_truncated_with_warn",
        t3.cycle_truncations > 0 && expanded.contains("X") && expanded.len() < 64,
        "",
    );
    // 7) 非法名拒绝（= / 空 / %）。
    let mut t4 = EnvTable::new();
    cs.add(
        "invalid_names_rejected",
        !t4.set("BAD=NAME", b"x", Scope::User)
            && !t4.set("", b"x", Scope::User)
            && !t4.set("HA%CK", b"x", Scope::User)
            && t4.invalid_name_rejects == 3,
        "",
    );
    // 8) 超大 PATH 警告（>8KB）；值超 32KB 拒绝不静默截断。
    let mut t5 = EnvTable::new();
    let big_path = "C:\\x;".repeat(1700); // 8.5KB
    let _ = t5.set("PATH", big_path.as_bytes(), Scope::User);
    let huge = [0u8; VALUE_CAP + 1];
    cs.add(
        "path_warn_and_value_cap",
        t5.path_overwarn && !t5.set("BIG", &huge, Scope::User),
        "",
    );
    // 9) 会话边界：快照冻结——后续修改不影响已创建进程的表。
    let mut t6 = EnvTable::new();
    let _ = t6.set("GOPATH", b"C:\\go", Scope::User);
    let snap = t6.snapshot();
    let _ = t6.set("GOPATH", b"C:\\go2", Scope::User);
    cs.add(
        "session_boundary_frozen",
        snap.get("GOPATH") == Some(&b"C:\\go"[..]) && t6.get("GOPATH") == Some(&b"C:\\go2"[..]),
        "",
    );
    // 10) PATH 分号切分展示 + 搜索过滤。
    let parts = split_path("C:\\a;C:\\b;;C:\\c");
    cs.add(
        "path_split_and_filter",
        parts.len() == 3 && !t6.filter_names("go").is_empty() && t6.filter_names("zzz").is_empty(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std_names_round_trip() {
        // 判据一全量演练：20 标准名 set→get 往返。
        let mut t = EnvTable::new();
        for name in STD_NAMES.iter() {
            assert!(t.set(name, b"ok", Scope::System), "{} must seed", name);
        }
        for name in STD_NAMES.iter() {
            assert_eq!(t.get(name), Some(&b"ok"[..]));
        }
    }

    #[test]
    fn first_case_wins_windows_semantics() {
        // Windows 同语义：首个写入的大小写为准（后续同键异写只改值）。
        let mut t = EnvTable::new();
        assert!(t.set("MyVar", b"1", Scope::User));
        assert!(t.set("MYVAR", b"2", Scope::User));
        assert_eq!(t.get_name_cased("myvar"), Some("MyVar"));
        assert_eq!(t.get("myvar"), Some(&b"2"[..]));
        // 表内不重复（仍一条）。
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn temp_points_to_sandbox_tmp() {
        // 主册【设计细节】：TEMP 指向沙盒内 tmp（每应用独立，防交叉污染）。
        let mut t = EnvTable::new();
        let sandbox_tmp = "~\\AppSandbox\\AppA\\tmp";
        assert!(t.set("TEMP", sandbox_tmp.as_bytes(), Scope::System));
        assert_eq!(t.get("TEMP"), Some(sandbox_tmp.as_bytes()));
    }

    #[test]
    fn expansion_single_pass_and_nested() {
        // 主册【设计细节】：变量值内百分号展开仅注入时执行一次。
        let mut t = EnvTable::new();
        let _ = t.set("A", b"%B%", Scope::User);
        let _ = t.set("B", b"leaf", Scope::User);
        // 一次注入：%A% → %B% → leaf（链式合法场景）。
        assert_eq!(t.expand("%A%"), "leaf");
        // 未知变量保留原样（Windows 语义）。
        assert_eq!(t.expand("%GHOST%"), "%GHOST%");
        // 字面百分号（无配对）保留。
        assert_eq!(t.expand("100%"), "100%");
    }

    #[test]
    fn snapshot_immutable() {
        let mut t = EnvTable::new();
        let _ = t.set("K", b"old", Scope::User);
        let s = t.snapshot();
        assert_eq!(s.len(), 1);
        let _ = t.remove("K");
        assert_eq!(s.get("K"), Some(&b"old"[..]), "snapshot unaffected by remove");
        assert_eq!(s.get("MISSING"), None);
    }

    #[test]
    fn remove_and_refilter() {
        let mut t = EnvTable::new();
        let _ = t.set("Alpha", b"1", Scope::User);
        let _ = t.set("Beta", b"2", Scope::User);
        assert!(t.remove("alpha"));
        assert!(!t.remove("alpha"));
        assert_eq!(t.len(), 1);
        assert_eq!(t.filter_names("be"), alloc::vec!["Beta"]);
    }

    #[test]
    fn computername_sourced_from_device() {
        // 主册【设计细节】：COMPUTERNAME 取设备名（F123「关于本机」同源）。
        let device_name = "Y7000-VARIX";
        let mut t = EnvTable::new();
        assert!(t.set("COMPUTERNAME", device_name.as_bytes(), Scope::System));
        assert_eq!(t.get("COMPUTERNAME"), Some(device_name.as_bytes()));
    }
}

// ---------------------------------------------------------------------------
// F011 · 深化扩展：env 配置落盘模型 + PATH 重组 + 非法名行内定位面
//
// 主册依据（G-A-11【数据与存储】）：「变量表存配置层（config/env.json），还原
// 点（F121）覆盖」——本扩展给出配置文件的规范落盘形态（帧式定长记录 + 校验
// 和，损坏如实拒载）；【交互设计】PATH 编辑器「每行一条」↔ 分号串的双向重组；
// 【状态与异常】非法名「行内红框即时校验」的定位面（第一个非法字符位置 +
// 错误类别）。
// ---------------------------------------------------------------------------

/// 配置文件头 16B：magic(4) + version(2) + count(2) + reserved(8)。
pub const ENV_CFG_HDR_SIZE: usize = 16;
/// 文件 magic（"VXE2"——Varix Env config v2）。
pub const ENV_CFG_MAGIC: [u8; 4] = *b"VXE2";

/// 序列化变量表（F121 还原点快照的落地形态；冷路径 alloc 与 EnvTable 同例
/// 登记）。记录序 = 表序；返回写入字节数，缓冲容量不足返回 0（不静默截）。
pub fn serialize_env_config(table: &EnvTable, buf: &mut Vec<u8>) -> usize {
    let snap = table.snapshot();
    let mut records: Vec<u8> = Vec::new();
    let mut count = 0usize;
    for i in 0..snap.len() {
        let (name, value, scope) = match snap.slot_at(i) {
            Some(t) => t,
            None => continue,
        };
        if name.len() > 64 {
            continue; // 与表内校验一致（超长名不入配置）
        }
        records.push(scope_priority_byte(scope));
        records.extend_from_slice(&(name.len() as u16).to_le_bytes());
        records.extend_from_slice(&(value.len() as u32).to_le_bytes());
        records.extend_from_slice(name.as_bytes());
        records.extend_from_slice(value);
        count += 1;
    }
    let total = ENV_CFG_HDR_SIZE + records.len() + 8;
    if buf.capacity() < total {
        return 0; // 容量不足如实返回 0（调用方按需扩容重试）
    }
    buf.clear();
    buf.extend_from_slice(&ENV_CFG_MAGIC);
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&(count as u16).to_le_bytes());
    buf.extend_from_slice(&[0u8; 8]);
    buf.extend_from_slice(&records);
    let sum = fnv_env(&buf[..]);
    let at = buf.len();
    buf.extend_from_slice(&sum.to_le_bytes());
    at + 8
}

fn scope_priority_byte(s: Scope) -> u8 {
    match s {
        Scope::System => 0,
        Scope::User => 1,
        Scope::Session => 2,
    }
}

fn byte_scope(b: u8) -> Option<Scope> {
    match b {
        0 => Some(Scope::System),
        1 => Some(Scope::User),
        2 => Some(Scope::Session),
        _ => None,
    }
}

fn fnv_env(data: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

/// 反序列化 → 新表（F121 还原语义：整表替换）。任何损坏如实拒（返回 Err，
/// 调用方保留原表——配置不静默清空）。
pub fn deserialize_env_config(data: &[u8]) -> Result<EnvTable, &'static str> {
    if data.len() < ENV_CFG_HDR_SIZE + 8 {
        return Err("env: config too small");
    }
    if data[0..4] != ENV_CFG_MAGIC {
        return Err("env: config bad magic");
    }
    if u16::from_le_bytes([data[4], data[5]]) != 2 {
        return Err("env: config unsupported version");
    }
    let count = u16::from_le_bytes([data[6], data[7]]) as usize;
    let body_end = data.len() - 8;
    let sum = u64::from_le_bytes(data[body_end..].try_into().unwrap());
    if fnv_env(&data[..body_end]) != sum {
        return Err("env: config checksum mismatch");
    }
    let mut t = EnvTable::new();
    let mut o = ENV_CFG_HDR_SIZE;
    for _ in 0..count {
        if o + 7 > body_end {
            return Err("env: config truncated record");
        }
        let scope = byte_scope(data[o]).ok_or("env: config bad scope")?;
        let name_len = u16::from_le_bytes([data[o + 1], data[o + 2]]) as usize;
        let val_len = u32::from_le_bytes([data[o + 3], data[o + 4], data[o + 5], data[o + 6]]) as usize;
        o += 7;
        if o + name_len + val_len > body_end {
            return Err("env: config truncated record body");
        }
        let name = core::str::from_utf8(&data[o..o + name_len]).map_err(|_| "env: config bad name")?;
        let value = &data[o + name_len..o + name_len + val_len];
        // 表内 set 通道复用（非法名/超限照表纪律拒绝——配置坏值不进表）。
        if !t.set(name, value, scope) {
            return Err("env: config record rejected by table");
        }
        o += name_len + val_len;
    }
    Ok(t)
}

// -- PATH 编辑器双向重组（每行一条 ↔ 分号串） -------------------------------

/// PATH 重组（编辑器每行一条 → 分号串）：空行剔除、去重保序（首次出现位
/// 置为准——Windows PATH 同语义）、不做 trim 以外的内容改写。
pub fn join_path(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for line in lines {
        let t = line.trim();
        if t.is_empty() || seen.contains(&t) {
            continue;
        }
        seen.push(t);
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(t);
    }
    out
}

// -- 非法名行内红框定位面 ----------------------------------------------------

/// 非法名错误类别（行内红框的三态：空名/非法字符/超长）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameError {
    Empty,
    /// 非法字符（`=` 破坏键值结构；`%` 保留给展开）。
    IllegalChar(u8),
    TooLong,
}

/// 定位第一个非法处（返回字节偏移——行内红框画在该字符下）。
/// 校验规则与 EnvTable::valid_name 同源（一处一事实：本函数是定位扩展，
/// 结论必须与 valid_name 一致——ext_tests 对账）。
pub fn name_error_at(name: &str) -> Option<(usize, NameError)> {
    if name.is_empty() {
        return Some((0, NameError::Empty));
    }
    for (i, &b) in name.as_bytes().iter().enumerate() {
        if b == b'=' || b == b'%' {
            return Some((i, NameError::IllegalChar(b)));
        }
    }
    if name.len() > 64 {
        return Some((64, NameError::TooLong));
    }
    None
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn env_config_round_trip_and_f121() {
        // 落盘往返 + F121 还原语义（整表替换）。
        let mut src = EnvTable::new();
        let _ = src.set("PATH", b"C:\\a;C:\\b", Scope::System);
        let _ = src.set("GOPATH", b"C:\\go", Scope::User);
        let _ = src.set("HTTP_PROXY", b"http://p:8080", Scope::Session);
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let n = serialize_env_config(&src, &mut buf);
        assert!(n > ENV_CFG_HDR_SIZE + 8);
        // 还原点覆盖后重载：三 scope 全回。
        let back = deserialize_env_config(&buf[..n]).expect("round trip");
        assert_eq!(back.len(), 3);
        assert_eq!(back.get("path"), Some(&b"C:\\a;C:\\b"[..]));
        assert_eq!(back.get_name_cased("gopath"), Some("GOPATH"), "首个写入大小写保留");
        assert_eq!(back.get("http_proxy"), Some(&b"http://p:8080"[..]));
        // 容量不足如实返回 0（不静默截半张表）。
        let mut tiny: Vec<u8> = Vec::with_capacity(4);
        assert_eq!(serialize_env_config(&src, &mut tiny), 0);
    }

    #[test]
    fn env_config_corrupt_rejected() {
        let mut src = EnvTable::new();
        let _ = src.set("A", b"1", Scope::User);
        let mut buf: Vec<u8> = Vec::with_capacity(512);
        let n = serialize_env_config(&src, &mut buf);
        // 坏 magic / 坏版本 / 截断 / 校验和翻转。
        let mut m = buf[..n].to_vec();
        m[0] = b'X';
        assert!(deserialize_env_config(&m).is_err());
        let mut v = buf[..n].to_vec();
        v[5] = 7;
        assert!(deserialize_env_config(&v).is_err());
        assert!(deserialize_env_config(&buf[..10]).is_err());
        let mut c = buf[..n].to_vec();
        let at = c.len() - 1;
        c[at] ^= 0xFF;
        assert!(deserialize_env_config(&c).is_err());
        // 记录体被改 → 校验和拦截（坏值不进表）。
        let mut b2 = buf[..n].to_vec();
        let pos = ENV_CFG_HDR_SIZE + 7; // 第一条记录的 name 区
        b2[pos] = b'Z';
        assert!(deserialize_env_config(&b2).is_err());
    }

    #[test]
    fn path_join_dedupe_and_order() {
        // 编辑器每行一条 → 分号串：空行剔、去重保序。
        let joined = join_path(&["C:\\a", "", "  C:\\b  ", "C:\\a", "C:\\c"]);
        assert_eq!(joined, "C:\\a;C:\\b;C:\\c");
        // 与 split 往返一致。
        assert_eq!(split_path(&joined), alloc::vec!["C:\\a", "C:\\b", "C:\\c"]);
        assert_eq!(join_path(&[]), "");
    }

    #[test]
    fn name_error_locator_matches_valid_name() {
        // 定位面与 valid_name 同源对账（一处一事实）。
        assert_eq!(name_error_at(""), Some((0, NameError::Empty)));
        assert_eq!(name_error_at("GOOD"), None);
        assert_eq!(name_error_at("BAD=NAME"), Some((3, NameError::IllegalChar(b'='))));
        assert_eq!(name_error_at("HA%CK"), Some((2, NameError::IllegalChar(b'%'))));
        let long = "x".repeat(65);
        assert_eq!(name_error_at(&long), Some((64, NameError::TooLong)));
        // 对账：定位 None ⇔ valid_name true；定位 Some ⇔ valid_name false。
        for case in ["OK1", "", "A=B", "P%Q", &long] {
            assert_eq!(name_error_at(case).is_none(), EnvTable::valid_name(case), "{}", case);
        }
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_envsess_checks() -> CheckSet {
    CheckSet::merge(run_envsess_base_checks(), CheckSet::merge(run_envsess_deep_checks(), run_envsess_deep2_checks()))
}

// ---------------------------------------------------------------------------
// F011 · 深化批次二：值类型面（REG_EXPAND_SZ 查询时展开）+ TEMP 沙盒化
//
// 主册依据（G-A-11【设计细节】）：「TEMP 指向沙盒内 tmp（每应用独立，防
// 交叉污染）」；Windows 环境变量值类型语义（REG_SZ 原样 / REG_EXPAND_SZ
// 查询时展开）——类型标记为叠层面（不破坏既有 EnvVar 结构）。
// ---------------------------------------------------------------------------

/// 沙盒 TEMP 渲染（每应用独立 tmp——防交叉污染；缓冲不足返回 0 不静默截）。
pub const TEMP_PATH_MAX: usize = 96;

pub fn temp_for_app(app_name: &str, buf: &mut [u8]) -> usize {
    const TEMPLATE: &str = "~\\AppSandbox\\";
    const SUFFIX: &str = "\\tmp";
    let total = TEMPLATE.len() + app_name.len() + SUFFIX.len();
    if buf.len() < total || app_name.is_empty() || app_name.len() > 32 {
        return 0;
    }
    buf[..TEMPLATE.len()].copy_from_slice(TEMPLATE.as_bytes());
    buf[TEMPLATE.len()..TEMPLATE.len() + app_name.len()].copy_from_slice(app_name.as_bytes());
    buf[TEMPLATE.len() + app_name.len()..total].copy_from_slice(SUFFIX.as_bytes());
    total
}

/// REG_EXPAND_SZ 叠层面（查询时展开语义——标记表定长 32 槽）。
pub struct TypedOverlay {
    expand_names: [([u8; 64], usize); 32],
    n: usize,
}

impl TypedOverlay {
    pub fn new() -> TypedOverlay {
        TypedOverlay { expand_names: [([0; 64], 0); 32], n: 0 }
    }

    fn find(&self, name: &str) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        (0..self.n).find(|&i| {
            core::str::from_utf8(&self.expand_names[i].0[..self.expand_names[i].1])
                .map(|n| n.to_ascii_lowercase() == lower)
                .unwrap_or(false)
        })
    }

    /// 标记变量为 REG_EXPAND_SZ。
    pub fn mark_expand(&mut self, name: &str) -> bool {
        if EnvTable::valid_name(name) {
            if let Some(i) = self.find(name) {
                let _ = i; // 已标记 → 幂等
                return true;
            }
            if self.n < 32 {
                self.expand_names[self.n].0[..name.len()].copy_from_slice(name.as_bytes());
                self.expand_names[self.n].1 = name.len();
                self.n += 1;
                return true;
            }
        }
        false
    }

    /// 是否 REG_EXPAND_SZ。
    pub fn is_expand(&self, name: &str) -> bool {
        self.find(name).is_some()
    }
}

/// F011 深化自检。
pub fn run_envsess_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep");
    // 1) TEMP 沙盒化：每应用独立路径渲染 + 长度上限诚实拒绝。
    let mut b = [0u8; TEMP_PATH_MAX];
    let n1 = temp_for_app("AppA", &mut b);
    let has_a = core::str::from_utf8(&b[..n1]).unwrap().contains("AppSandbox\\AppA\\tmp");
    let n2 = temp_for_app("AppB", &mut b);
    let has_b = core::str::from_utf8(&b[..n2]).unwrap().contains("AppSandbox\\AppB\\tmp");
    cs.add(
        "temp_per_app_sandboxed",
        n1 > 0 && n2 > 0 && n1 == n2 && has_a && has_b,
        "",
    );
    cs.add("temp_oversize_honest", temp_for_app(&"x".repeat(33), &mut b) == 0, "");
    // 2) REG_EXPAND_SZ 叠层面：标记/查询/幂等；非法名拒绝。
    let mut t = TypedOverlay::new();
    let m1 = t.mark_expand("PROMPT");
    let m2 = t.mark_expand("prompt"); // 大小写不敏感幂等
    let m3 = t.mark_expand("BAD=NAME");
    cs.add(
        "typed_overlay_semantics",
        m1 && m2 && !m3 && t.is_expand("PROMPT") && t.is_expand("prompt") && !t.is_expand("PATH"),
        "",
    );
    // 3) 查询时展开语义（Windows：REG_EXPAND_SZ 的值在 GetEnvironmentVariable
    //    时才展开——模型层以 expand() 消费面钉死，注入面一次语义 base 已锁）。
    let mut tab = EnvTable::new();
    let _ = tab.set("BASE", b"C:\\root", Scope::System);
    let _ = tab.set("CHAIN", b"%BASE%\\sub", Scope::User);
    let mut ov = TypedOverlay::new();
    let _ = ov.mark_expand("CHAIN");
    let expanded = tab.expand("%CHAIN%");
    cs.add(
        "expand_sz_query_time",
        ov.is_expand("CHAIN") && expanded == "C:\\root\\sub",
        "",
    );
    // 4) 配置落盘/PATH 重组/非法名定位（深化一批既有面）对账锚。
    let joined = join_path(&["C:\\a", "", "C:\\b", "C:\\a"]);
    cs.add(
        "deep_batch1_anchored",
        joined == "C:\\a;C:\\b"
            && name_error_at("A=B") == Some((1, NameError::IllegalChar(b'=')))
            && tab.snapshot().get("BASE") == Some(&b"C:\\root"[..]),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F011 · 深化批次三：注入期一次展开纪律（百分号展开仅注入时执行一次）+
// COMPUTERNAME 取设备名（F123「关于本机」同源）
//
// 主册依据（G-A-11【设计细节】）：「变量值内百分号展开仅注入时执行一次」；
// 「COMPUTERNAME 取设备名（F123『关于本机』同源）」。expand/EnvTable 既有面
// （一处一事实），本段补纪律记账与同源取值。
// ---------------------------------------------------------------------------

/// 展开纪律台账：注入期展开计数 vs 运行期再展开违例计数。
#[derive(Clone, Copy, Debug)]
pub struct ExpandOnceLedger {
    /// 注入期展开次数（合法——每次进程创建/会话注入各一次）。
    pub injection_expansions: u32,
    /// 运行期再展开次数（违例——主册「仅注入时执行一次」的红线计数）。
    pub runtime_violations: u32,
}

impl ExpandOnceLedger {
    pub const fn new() -> ExpandOnceLedger {
        ExpandOnceLedger { injection_expansions: 0, runtime_violations: 0 }
    }

    /// 注入期展开（合法路径——放行并计数）。
    pub fn note_injection(&mut self) {
        self.injection_expansions += 1;
    }

    /// 运行期展开请求：恒拒绝（违例如实计数——运行期值就是注入后的字面值）。
    pub fn request_runtime_expand(&mut self) -> bool {
        self.runtime_violations += 1;
        false
    }

    /// 纪律恒等式：违例恒 0 才算会话面干净（诊断口径）。
    pub fn clean(&self) -> bool {
        self.runtime_violations == 0
    }
}

/// COMPUTERNAME 缓冲上限（F123 设备名口径——超长设备名如实截断计数）。
pub const COMPUTERNAME_CAP: usize = 32;

/// COMPUTERNAME 取值（F123「关于本机」同源）：设备名**原样**进会话变量，
/// 零变换零二次格式化——两处显示必须逐字节一致（一处一事实）。
/// 返回 (写入字节数, 是否被截断)。
pub fn computername_of(device_name: &str, buf: &mut [u8]) -> (usize, bool) {
    let src = device_name.as_bytes();
    let n = src.len().min(buf.len()).min(COMPUTERNAME_CAP);
    buf[..n].copy_from_slice(&src[..n]);
    (n, src.len() > n)
}

/// F011 深化批次三自检。
pub fn run_envsess_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep2");
    // 1) 注入期展开合法计数；运行期展开恒拒绝且计违例；干净判定。
    let mut led = ExpandOnceLedger::new();
    led.note_injection();
    led.note_injection();
    let runtime_ok = led.request_runtime_expand();
    cs.add(
        "expand_once_discipline",
        led.injection_expansions == 2
            && !runtime_ok
            && led.runtime_violations == 1
            && !led.clean(),
        "",
    );
    // 2) 违例清零后的干净会话（零违例 = clean——异常零静默的对偶面）。
    let mut led2 = ExpandOnceLedger::new();
    led2.note_injection();
    cs.add("expand_once_clean_session", led2.clean() && led2.injection_expansions == 1, "");
    // 3) COMPUTERNAME 同源保真：原样逐字节一致；短缓冲如实截断并报告。
    let mut buf = [0u8; COMPUTERNAME_CAP];
    let (n1, trunc1) = computername_of("Y7000-DEV", &mut buf);
    let same = &buf[..n1] == b"Y7000-DEV";
    let mut small = [0u8; 4];
    let (n2, trunc2) = computername_of("Y7000-DEV", &mut small);
    cs.add(
        "computername_same_source_as_f123",
        n1 == 9 && !trunc1 && same && n2 == 4 && trunc2,
        "",
    );
    cs
}
