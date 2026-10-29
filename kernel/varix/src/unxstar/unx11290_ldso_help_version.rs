//! UNX-F11290 ld.so --help/--version 面（0x1f650 段）
//!
//! 锚点：UNX-F11290-J1 `--version` 输出版本串与钉版一致，`--help` 选项表与
//! glibc 2.39 一致。
//!
//! 纯功能模型：把 ld.so 的版本面与选项面各自立表。版本面钉住
//! `ld.so (GNU libc) 2.39` 及版权/免责/作者三行；选项面按 glibc 2.39 的
//! 十四项登记（长名、是否带参、摘要），逐项渲染成 `--help` 行。解析器对
//! 未知选项、缺参、重名安装一律拒；钉版一经冻结，改版即 FROZEN。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f650;
/// 未知长选项
pub const HV_EOPT: u32 = SEG_BASE + 0x01;
/// 带参选项缺参
pub const HV_EARG: u32 = SEG_BASE + 0x02;
/// 选项重名安装
pub const HV_EDUP: u32 = SEG_BASE + 0x03;
/// 选项名不以 `--` 起头
pub const HV_ENAME: u32 = SEG_BASE + 0x04;
/// 钉版冻结后改版
pub const HV_EFROZEN: u32 = SEG_BASE + 0x05;
/// 版本行与钉版不符
pub const HV_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 摘要为空
pub const HV_EBLANK: u32 = SEG_BASE + 0x07;

/// 钉定主版本。
pub const PIN_MAJOR: u8 = 2;
/// 钉定次版本（glibc 2.39）。
pub const PIN_MINOR: u8 = 39;
/// 选项条数（glibc 2.39 十四个长选项）。
pub const OPTION_COUNT: usize = 14;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 参数形态：无参 / 必带一参。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    None,
    Required,
}

/// 单个长选项规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptSpec {
    pub long: &'static str,
    pub arg: ArgKind,
    pub summary: &'static str,
}

impl OptSpec {
    /// 渲染一行 `--help`：`      --long[=ARG]  summary`。
    pub fn render(&self) -> String {
        let pad = if self.arg == ArgKind::Required { "=ARG" } else { "" };
        format!("      {}{}  {}", self.long, pad, self.summary)
    }

    /// 长名合法性：以 `--` 起头且长度 > 2。
    pub fn name_ok(&self) -> bool {
        self.long.starts_with("--") && self.long.len() > 2
    }
}

/// 版本钉：主次版本 + 冻结位。
#[derive(Clone, Debug)]
pub struct VersionPin {
    major: u8,
    minor: u8,
    frozen: bool,
    locked: Option<u64>,
    pub repins: u64,
    pub rejects: u64,
}

impl VersionPin {
    /// 钉版 2.39。
    pub fn pinned() -> VersionPin {
        VersionPin {
            major: PIN_MAJOR,
            minor: PIN_MINOR,
            frozen: false,
            locked: None,
            repins: 0,
            rejects: 0,
        }
    }

    /// 首行版本串（与 glibc 逐字一致）。
    pub fn version_line(&self) -> String {
        format!("ld.so (GNU libc) {}.{}", self.major, self.minor)
    }

    /// 版权行。
    pub fn copyright_line() -> String {
        "Copyright (C) 2024 Free Software Foundation, Inc.".to_string()
    }

    /// 免责两行。
    pub fn warranty_lines() -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push("This is free software; see the source for copying conditions.  There is NO".to_string());
        v.push("warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.".to_string());
        v
    }

    /// 作者行。
    pub fn author_line() -> String {
        "Written by Roland McGrath and Ulrich Drepper.".to_string()
    }

    /// 完整 `--version` 输出（五行）。
    pub fn render(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(self.version_line());
        v.push(VersionPin::copyright_line());
        let w = VersionPin::warranty_lines();
        let mut k = 0usize;
        while k < w.len() {
            if let Some(line) = w.get(k) {
                v.push(line.clone());
            }
            k = k.wrapping_add(1);
        }
        v.push(VersionPin::author_line());
        v
    }

    /// 版本串比对：外部文本须逐字等于本钉的版本行。
    pub fn matches(&self, text: &str) -> bool {
        text == self.version_line()
    }

    /// 是否等于钉定 2.39。
    pub fn at_pin(&self) -> bool {
        self.major == PIN_MAJOR && self.minor == PIN_MINOR
    }

    /// 改版：冻结拒、同版不动作、跨版重钉。
    pub fn repin(&mut self, major: u8, minor: u8) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HV_EFROZEN);
        }
        if major == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HV_EMISMATCH);
        }
        self.major = major;
        self.minor = minor;
        self.repins = self.repins.wrapping_add(1);
        Ok(())
    }

    /// 版本面指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.major as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.minor as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.frozen { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结：幂等取回锁定指纹。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.locked;
        }
        let v = self.fold();
        self.locked = Some(v);
        self.frozen = true;
        self.locked
    }

}

/// glibc 2.39 的十四项长选项（顺序即 `--help` 呈现序）。
pub const GLIBC239_OPTIONS: [OptSpec; OPTION_COUNT] = [
    OptSpec { long: "--list", arg: ArgKind::None, summary: "list all dependencies and how they are resolved" },
    OptSpec { long: "--verify", arg: ArgKind::None, summary: "verify that given object really is a dynamically linked object" },
    OptSpec { long: "--inhibit-cache", arg: ArgKind::None, summary: "Do not use /etc/ld.so.cache" },
    OptSpec { long: "--library-path", arg: ArgKind::Required, summary: "use given PATH instead of LD_LIBRARY_PATH" },
    OptSpec { long: "--glibc-hwcaps-prepend", arg: ArgKind::Required, summary: "prepend a list of glibc-hwcaps subdirectories" },
    OptSpec { long: "--glibc-hwcaps-mask", arg: ArgKind::Required, summary: "mask a list of glibc-hwcaps subdirectories" },
    OptSpec { long: "--inhibit-rpath", arg: ArgKind::Required, summary: "ignore RUNPATH and RPATH information in LIST" },
    OptSpec { long: "--audit", arg: ArgKind::Required, summary: "use objects named in LIST as auditors" },
    OptSpec { long: "--preload", arg: ArgKind::Required, summary: "preload objects named in LIST" },
    OptSpec { long: "--argv0", arg: ArgKind::Required, summary: "set argv[0] to STRING before running" },
    OptSpec { long: "--list-tunables", arg: ArgKind::None, summary: "list all tunables with minimum and maximum values" },
    OptSpec { long: "--list-diagnostics", arg: ArgKind::None, summary: "list diagnostics information" },
    OptSpec { long: "--help", arg: ArgKind::None, summary: "print this help and exit" },
    OptSpec { long: "--version", arg: ArgKind::None, summary: "print version information and exit" },
];

/// 动作面：解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Help,
    Version,
    List,
    Verify,
    ListTunables,
    ListDiagnostics,
    InhibitCache,
    Run,
}

/// 选项册：安装序 + 冻结位。
#[derive(Clone, Debug)]
pub struct OptionTable {
    opts: Vec<OptSpec>,
    frozen: bool,
    pub installs: u64,
    pub rejects: u64,
}

impl OptionTable {
    /// 空册。
    pub fn empty() -> OptionTable {
        OptionTable { opts: Vec::new(), frozen: false, installs: 0, rejects: 0 }
    }

    /// glibc 2.39 标准册：十四项按序装入。
    pub fn standard() -> OptionTable {
        let mut t = OptionTable::empty();
        let mut k = 0usize;
        while k < GLIBC239_OPTIONS.len() {
            if let Some(spec) = GLIBC239_OPTIONS.get(k) {
                let _ = t.install(*spec);
            }
            k = k.wrapping_add(1);
        }
        t
    }

    pub fn len(&self) -> usize {
        self.opts.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 按长名查项。
    pub fn find(&self, long: &str) -> Option<&OptSpec> {
        let mut k = 0usize;
        while k < self.opts.len() {
            if let Some(spec) = self.opts.get(k) {
                if spec.long == long {
                    return Some(spec);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 安装一项：冻结 / 名非法 / 摘要空 / 重名四关。
    pub fn install(&mut self, spec: OptSpec) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HV_EFROZEN);
        }
        let r = self.install_inner(spec);
        match r {
            Ok(()) => {
                self.installs = self.installs.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    fn install_inner(&mut self, spec: OptSpec) -> Result<(), u32> {
        if !spec.name_ok() {
            return Err(HV_ENAME);
        }
        if spec.summary.is_empty() {
            return Err(HV_EBLANK);
        }
        if self.find(spec.long).is_some() {
            return Err(HV_EDUP);
        }
        self.opts.push(spec);
        Ok(())
    }

    /// `--help` 全文：用法首行 + 说明 + Options 段。
    pub fn render_help(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push("Usage: ld.so [OPTION]... EXECUTABLE-FILE [ARGS-FOR-PROGRAM...]".to_string());
        out.push("You have invoked `ld.so', the helper program for shared library executables.".to_string());
        out.push(String::new());
        out.push("Options:".to_string());
        let mut k = 0usize;
        while k < self.opts.len() {
            if let Some(spec) = self.opts.get(k) {
                out.push(spec.render());
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 解析一枚选项；带参项须紧随一参（空参视同缺参）。
    pub fn parse_one(&self, args: &[&str], at: usize) -> Result<(Action, usize), u32> {
        let name = match args.get(at) {
            Some(v) => *v,
            None => return Err(HV_EOPT),
        };
        let spec = match self.find(name) {
            Some(s) => s,
            None => return Err(HV_EOPT),
        };
        let mut next = at.wrapping_add(1);
        if spec.arg == ArgKind::Required {
            match args.get(next) {
                Some(v) if !v.is_empty() => next = next.wrapping_add(1),
                _ => return Err(HV_EARG),
            }
        }
        let action = match name {
            "--help" => Action::Help,
            "--version" => Action::Version,
            "--list" => Action::List,
            "--verify" => Action::Verify,
            "--list-tunables" => Action::ListTunables,
            "--list-diagnostics" => Action::ListDiagnostics,
            "--inhibit-cache" => Action::InhibitCache,
            _ => Action::Run,
        };
        Ok((action, next))
    }

    /// 全表自检：条数对、名唯一、名合法、摘要非空四关。
    pub fn consistent(&self) -> bool {
        if self.opts.len() != OPTION_COUNT {
            return false;
        }
        let mut i = 0usize;
        while i < self.opts.len() {
            let a = match self.opts.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !a.name_ok() || a.summary.is_empty() {
                return false;
            }
            let mut j = i.wrapping_add(1);
            while j < self.opts.len() {
                match self.opts.get(j) {
                    Some(b) if b.long == a.long => return false,
                    Some(_) => {}
                    None => return false,
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 带参项数（glibc 2.39 为七：library-path / hwcaps×2 / inhibit-rpath /
    /// audit / preload / argv0）。
    pub fn argful_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.opts.len() {
            if let Some(s) = self.opts.get(k) {
                if s.arg == ArgKind::Required {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 表指纹：长名字节链 + 带参位。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.opts.len() {
            if let Some(s) = self.opts.get(k) {
                let bytes = s.long.as_bytes();
                let mut b = 0usize;
                while b < bytes.len() {
                    h = (h ^ bytes[b] as u64).wrapping_mul(FNV_PRIME);
                    b = b.wrapping_add(1);
                }
                h = (h ^ if s.arg == ArgKind::Required { 1 } else { 0 })
                    .wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 冻结：幂等取回锁定指纹（表项此后不再增删）。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return Some(self.fold());
        }
        self.frozen = true;
        Some(self.fold())
    }
}

/// 版本面与选项面合装配：钉版 → 标准册 → 双向自检。
pub fn build_help_surface() -> (VersionPin, OptionTable, bool) {
    let pin = VersionPin::pinned();
    let table = OptionTable::standard();
    let ok = pin.at_pin() && table.consistent() && table.len() == OPTION_COUNT;
    (pin, table, ok)
}

/// 四路注入：未知选项 / 缺参 / 重名安装 / 冻结后改版，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let table = OptionTable::standard();

    // ① 未知长选项
    got[0] = table.parse_one(&["--diagnose-magic"], 0).err() == Some(HV_EOPT)
        && table.parse_one(&["-h"], 0).err() == Some(HV_EOPT);

    // ② 带参选项缺参（末尾无参 / 空串参）
    got[1] = table.parse_one(&["--preload"], 0).err() == Some(HV_EARG)
        && table.parse_one(&["--library-path", ""], 0).err() == Some(HV_EARG);

    // ③ 重名安装与非法名
    let mut dup = OptionTable::standard();
    let again = dup.install(OptSpec {
        long: "--list",
        arg: ArgKind::None,
        summary: "duplicate",
    });
    got[2] = again.err() == Some(HV_EDUP)
        && dup.install(OptSpec { long: "list", arg: ArgKind::None, summary: "bad" })
            .err()
            == Some(HV_ENAME);

    // ④ 冻结后装项与改版 + 冻结幂等
    let (mut pin, mut frozen_table, ok) = build_help_surface();
    let proot = pin.freeze();
    let troot = frozen_table.freeze();
    got[3] = ok
        && pin.freeze() == proot
        && frozen_table.freeze() == troot
        && pin.repin(2, 40).err() == Some(HV_EFROZEN)
        && frozen_table
            .install(OptSpec { long: "--late", arg: ArgKind::None, summary: "late" })
            .err()
            == Some(HV_EFROZEN);
    got
}

/// 判据台：ld.so 版本面与选项面的十四条判据。
pub fn run_unx11290_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11290");

    let (mut pin, mut table, built) = build_help_surface();
    let ver = pin.render();
    let help = table.render_help();

    // J1 版本串与钉版一致
    let first = ver.first().map(|x| x.as_str()).unwrap_or("");
    let prefix = "ld.so (GNU libc) ";
    let tail = first.strip_prefix(prefix).unwrap_or("");
    s.add(
        "J1-版本串钉版",
        first == "ld.so (GNU libc) 2.39"
            && tail == "2.39"
            && pin.matches(first)
            && pin.at_pin(),
        "首行版本串逐字等于 ld.so (GNU libc) 2.39",
    );

    // J2 版本面五行齐备
    s.add(
        "J2-版本五行",
        ver.len() == 5
            && ver.iter().any(|l| l.starts_with("Copyright (C) 2024"))
            && ver.iter().any(|l| l.starts_with("Written by Roland McGrath"))
            && ver.iter().any(|l| l.contains("There is NO"))
            && ver.iter().any(|l| l.contains("warranty; not even")),
        "版权/免责/作者三面齐、共五行",
    );

    // J3 选项表十四条
    s.add(
        "J3-选项十四",
        table.len() == OPTION_COUNT && table.consistent() && built,
        "glibc 2.39 十四长选项全在册且互异",
    );

    // J4 help 首行用法
    s.add(
        "J4-help 用法行",
        help.first().map(|x| x.as_str())
            == Some("Usage: ld.so [OPTION]... EXECUTABLE-FILE [ARGS-FOR-PROGRAM...]")
            && help.iter().any(|l| l == "Options:"),
        "--help 首行为标准用法行、Options 段在册",
    );

    // J5 逐项渲染对位
    let mut render_ok = help.len() == OPTION_COUNT + 4;
    let mut k = 0usize;
    while k < GLIBC239_OPTIONS.len() {
        if let Some(spec) = GLIBC239_OPTIONS.get(k) {
            if table.find(spec.long).is_none() || !help.iter().any(|l| l.contains(spec.long)) {
                render_ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("J5-逐项渲染", render_ok, "十四项逐条出现在 help 行中");

    // J6 带参项七个
    s.add(
        "J6-带参七项",
        table.argful_count() == 7,
        "library-path/hwcaps×2/inhibit-rpath/audit/preload/argv0",
    );

    // J7 --help 解析
    s.add(
        "J7-help 解析",
        table.parse_one(&["--help"], 0) == Ok((Action::Help, 1)),
        "--help 解析为帮助动作",
    );

    // J8 --version 解析
    s.add(
        "J8-version 解析",
        table.parse_one(&["--version"], 0) == Ok((Action::Version, 1)),
        "--version 解析为版本动作",
    );

    // J9 带参项消耗一参
    s.add(
        "J9-带参消耗",
        table.parse_one(&["--preload", "liba.so", "--list"], 0) == Ok((Action::Run, 2))
            && table.parse_one(&["--list"], 0) == Ok((Action::List, 1)),
        "--preload 吃掉一参后停在 2 位",
    );

    // J10 未知选项拒
    let mut bad = OptionTable::standard();
    s.add(
        "J10-未知选项拒",
        bad.parse_one(&["--frobnicate"], 0).err() == Some(HV_EOPT)
            && bad.install(OptSpec { long: "-x", arg: ArgKind::None, summary: "bad" }).err()
                == Some(HV_ENAME),
        "未知长名与非 -- 起头名两路皆拒",
    );

    // J11 缺参拒
    s.add(
        "J11-缺参拒",
        table.parse_one(&["--argv0"], 0).err() == Some(HV_EARG)
            && table.parse_one(&["--audit", ""], 0).err() == Some(HV_EARG),
        "带参项无参或空参皆拒",
    );

    // J12 重名安装拒
    let mut dup = OptionTable::standard();
    s.add(
        "J12-重名拒",
        dup.install(OptSpec { long: "--verify", arg: ArgKind::None, summary: "dup" }).err()
            == Some(HV_EDUP)
            && dup.len() == OPTION_COUNT,
        "重名安装拒且表长不增",
    );

    // J13 冻结幂等
    let proot = pin.freeze();
    let troot = table.freeze();
    s.add(
        "J13-冻结幂等",
        pin.freeze() == proot && table.freeze() == troot && proot.is_some() && troot.is_some(),
        "版本面与选项面重复冻结取回同一指纹",
    );

    // J14 冻结拒改
    s.add(
        "J14-冻结拒改",
        pin.repin(2, 40).err() == Some(HV_EFROZEN)
            && table
                .install(OptSpec { long: "--late", arg: ArgKind::None, summary: "late" })
                .err()
                == Some(HV_EFROZEN),
        "冻结后改版与装项两路皆拒",
    );

    // J15 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J15-注入四拒", all, "未知/缺参/重名/冻结四路全拒");

    // J16 动态明细
    s.add_dyn(
        "J16-版本面明细",
        table.rejects == 1 && pin.rejects == 1,
        &format!(
            "opts={} arg={} rej={}/{} fp={:#x}",
            table.len(),
            table.argful_count(),
            table.rejects,
            pin.rejects,
            table.fold()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11290_ldso_flow() {
        let (mut pin, mut table, built) = build_help_surface();
        assert!(built);
        assert_eq!(pin.version_line(), "ld.so (GNU libc) 2.39");
        assert!(pin.matches("ld.so (GNU libc) 2.39"));
        assert!(!pin.matches("ld.so (GNU libc) 2.38"));
        assert_eq!(pin.render().len(), 5);
        assert_eq!(table.len(), OPTION_COUNT);
        assert_eq!(table.argful_count(), 7);
        assert!(table.consistent());

        let help = table.render_help();
        assert_eq!(help.len(), OPTION_COUNT + 4);
        assert!(help.iter().any(|l| l.contains("--list-diagnostics")));

        assert_eq!(table.parse_one(&["--help"], 0), Ok((Action::Help, 1)));
        assert_eq!(table.parse_one(&["--version"], 0), Ok((Action::Version, 1)));
        assert_eq!(
            table.parse_one(&["--library-path", "/lib"], 0),
            Ok((Action::Run, 2))
        );
        assert_eq!(table.parse_one(&["--library-path"], 0).err(), Some(HV_EARG));
        assert_eq!(table.parse_one(&["--nope"], 0).err(), Some(HV_EOPT));

        assert!(pin.freeze().is_some());
        assert_eq!(pin.freeze(), pin.freeze());
        assert_eq!(pin.repin(2, 40).err(), Some(HV_EFROZEN));
        assert!(table.freeze().is_some());
        assert_eq!(table.freeze(), table.freeze());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11290_checks_green() {
        let s = run_unx11290_checks();
        assert!(s.all_passed(), "UNX-F11290 判据未全绿");
    }
}

// ---------------------------------------------------------------- 检索面
/// 选项表只读检索视图：前缀检索、带参抽取、帮文列宽与对齐渲染。
pub struct OptionView<'a> {
    table: &'a OptionTable,
}

impl<'a> OptionView<'a> {
    pub fn new(table: &'a OptionTable) -> OptionView<'a> {
        OptionView { table }
    }

    /// 长名以 prefix 起头的项（保安装序）。
    pub fn matching(&self, prefix: &str) -> Vec<&'a OptSpec> {
        let mut out: Vec<&'a OptSpec> = Vec::new();
        let mut k = 0usize;
        while k < self.table.opts.len() {
            if let Some(s) = self.table.opts.get(k) {
                if s.long.starts_with(prefix) {
                    out.push(s);
                }
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 带参项长名（安装序），与 `--help` 呈现序一致。
    pub fn argful(&self) -> Vec<&'a str> {
        let mut out: Vec<&'a str> = Vec::new();
        let mut k = 0usize;
        while k < self.table.opts.len() {
            if let Some(s) = self.table.opts.get(k) {
                if s.arg == ArgKind::Required {
                    out.push(s.long);
                }
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 无参项数：总条数减去带参项数。
    pub fn inert_count(&self) -> usize {
        self.table.len().wrapping_sub(self.table.argful_count())
    }

    /// 最长长名的显示宽度（带参项计入 `=ARG` 四列）。
    pub fn name_width(&self) -> usize {
        let mut w = 0usize;
        let mut k = 0usize;
        while k < self.table.opts.len() {
            if let Some(s) = self.table.opts.get(k) {
                let extra = if s.arg == ArgKind::Required { 4 } else { 0 };
                let width = s.long.len().wrapping_add(extra);
                if width > w {
                    w = width;
                }
            }
            k = k.wrapping_add(1);
        }
        w
    }

    /// 对齐渲染：摘要列左缘统一落到 `6 + name_width + 2` 列。
    pub fn render_aligned(&self) -> Vec<String> {
        let w = self.name_width();
        let mut out: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.table.opts.len() {
            if let Some(s) = self.table.opts.get(k) {
                let pad = if s.arg == ArgKind::Required { "=ARG" } else { "" };
                let mut line = format!("      {}{}", s.long, pad);
                let mut need = w.wrapping_sub(s.long.len().wrapping_add(pad.len()));
                while need > 0 {
                    line.push(' ');
                    need = need.wrapping_sub(1);
                }
                line.push_str("  ");
                line.push_str(s.summary);
                out.push(line);
            }
            k = k.wrapping_add(1);
        }
        out
    }
}

/// 版本序：主版本先行、次版本随后，返回 -1/0/1。
pub fn version_cmp(a: (u8, u8), b: (u8, u8)) -> i8 {
    if a.0 < b.0 {
        return -1;
    }
    if a.0 > b.0 {
        return 1;
    }
    if a.1 < b.1 {
        return -1;
    }
    if a.1 > b.1 {
        return 1;
    }
    0
}

/// 从 `ld.so (GNU libc) X.Y` 取回版本对；前缀不符或数位非法返回 None。
pub fn parse_version_line(text: &str) -> Option<(u8, u8)> {
    let rest = text.strip_prefix("ld.so (GNU libc) ")?;
    let (maj, min) = rest.split_once('.')?;
    if maj.is_empty() || min.is_empty() {
        return None;
    }
    if !maj.bytes().all(|c| c.is_ascii_digit()) || !min.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let m: u8 = maj.parse().ok()?;
    let n: u8 = min.parse().ok()?;
    Some((m, n))
}

#[cfg(test)]
mod retrieval_tests {
    use super::*;

    #[test]
    fn prefix_and_argful_groups() {
        let (_pin, table, _ok) = build_help_surface();
        let view = OptionView::new(&table);
        assert_eq!(view.matching("--list").len(), 3);
        assert_eq!(view.matching("--list")[0].long, "--list");
        assert!(view.matching("--lis").len() >= 3);
        assert_eq!(view.matching("--zzz").len(), 0);
        let argful = view.argful();
        assert_eq!(argful.len(), table.argful_count());
        assert!(argful.contains(&"--library-path"));
        assert!(argful.contains(&"--preload"));
        assert_eq!(view.inert_count() + argful.len(), OPTION_COUNT);
    }

    #[test]
    fn aligned_render_columns() {
        let (_pin, table, _ok) = build_help_surface();
        let view = OptionView::new(&table);
        let w = view.name_width();
        assert!(w > 0);
        let lines = view.render_aligned();
        assert_eq!(lines.len(), OPTION_COUNT);
        let mut k = 0usize;
        while k < lines.len() {
            let line = &lines[k];
            let spec = &GLIBC239_OPTIONS[k];
            let start = line.find(spec.summary);
            assert_eq!(start, Some(6 + w + 2));
            assert!(line.starts_with("      "));
            k += 1;
        }
    }

    #[test]
    fn version_order_and_parse() {
        assert_eq!(version_cmp((2, 39), (2, 39)), 0);
        assert_eq!(version_cmp((2, 39), (2, 40)), -1);
        assert_eq!(version_cmp((3, 0), (2, 99)), 1);
        assert_eq!(version_cmp((2, 9), (2, 10)), -1);
        assert_eq!(parse_version_line("ld.so (GNU libc) 2.39"), Some((2, 39)));
        assert_eq!(parse_version_line("ld.so (GNU libc) 2.4x"), None);
        assert_eq!(parse_version_line("ld.so (GNU libc) 2."), None);
        assert_eq!(parse_version_line("glibc 2.39"), None);
        let pin = VersionPin::pinned();
        assert_eq!(parse_version_line(&pin.version_line()), Some((2, 39)));
        assert!(pin.at_pin());
    }

    #[test]
    fn frozen_view_is_stable() {
        let (_pin, mut table, _ok) = build_help_surface();
        let before = {
            let view = OptionView::new(&table);
            (view.name_width(), view.render_aligned(), view.argful().len())
        };
        let locked = table.freeze();
        assert!(locked.is_some());
        assert_eq!(table.freeze(), locked);
        let spec = OptSpec {
            long: "--late",
            arg: ArgKind::None,
            summary: "late install must be refused",
        };
        assert_eq!(table.install(spec), Err(HV_EFROZEN));
        let view = OptionView::new(&table);
        assert_eq!(view.name_width(), before.0);
        assert_eq!(view.render_aligned(), before.1);
        assert_eq!(view.argful().len(), before.2);
        assert_eq!(table.len(), OPTION_COUNT);
    }
}
