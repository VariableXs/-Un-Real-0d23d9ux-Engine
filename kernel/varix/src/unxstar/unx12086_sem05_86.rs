//! UNX-F12086 RootDirectory 相对打开语义（0x19ce0 段）
//!
//! 卡面：OBJECT_ATTRIBUTES→RootDirectory——相对打开：路径相对根句柄解析；
//! 绝对路径与根句柄同给必拒；无根无路径必拒；根须为目录类型。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 相对必持根：相对路径（不以 '\' 起）而无 RootDirectory 必拒。
//! - J2 绝对不持根：绝对路径（以 '\' 起）而持 RootDirectory 必拒。
//! - J3 双空拒：根句柄零且路径空必拒。
//! - J4 根存在性：未知根句柄必拒。
//! - J5 根类型：根句柄非目录类型必拒。
//! - J6 相对解析：相对路径拼到根名后可回读。
//! - J7 印账冻结：seal 后打开必拒。
//! - J8 注入五况全抓。
//!
//! 结构：RootTable 根句柄册（含目录类型）+ OpenReq 打开请求 +
//! 解析核 + 折叠指纹（211 进制卷积）+ 注入核（绝对带根/坏根/非目录/
//! 相对无根/印后打开）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ce0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ce0;
pub const RO_ABS_WITH_ROOT: u32 = SEG_BASE + 0x01; // 绝对带根
pub const RO_BAD_ROOT: u32 = SEG_BASE + 0x02; // 根不存在
pub const RO_NOT_DIR: u32 = SEG_BASE + 0x03; // 根非目录
pub const RO_NO_ROOT: u32 = SEG_BASE + 0x04; // 相对无根
pub const RO_NULL_BOTH: u32 = SEG_BASE + 0x05; // 双空
pub const RO_FROZEN: u32 = SEG_BASE + 0x06; // 印账后打开

/// 根句柄行。
#[derive(Clone, Copy)]
pub struct RootRow {
    pub handle: u32,
    pub is_dir: bool,
    pub name: &'static str,
}

/// 根句柄册。
pub struct RootTable {
    pub rows: Vec<RootRow>,
    pub sealed: bool,
}

impl RootTable {
    pub fn new() -> RootTable {
        RootTable { rows: Vec::new(), sealed: false }
    }

    pub fn add(&mut self, handle: u32, is_dir: bool, name: &'static str) {
        self.rows.push(RootRow { handle, is_dir, name });
    }

    fn find(&self, handle: u32) -> Option<&RootRow> {
        self.rows.iter().find(|r| r.handle == handle)
    }

    /// 打开请求解析核。
    pub fn open(&self, root: u32, path: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(RO_FROZEN);
        }
        if root == 0 && path.is_empty() {
            return Err(RO_NULL_BOTH);
        }
        if path.starts_with('\\') {
            if root != 0 {
                return Err(RO_ABS_WITH_ROOT);
            }
            return Ok(path.to_string());
        }
        if root == 0 {
            return Err(RO_NO_ROOT);
        }
        let row = match self.find(root) {
            Some(r) => r,
            None => return Err(RO_BAD_ROOT),
        };
        if !row.is_dir {
            return Err(RO_NOT_DIR);
        }
        Ok(format!("\\{}\\{}", row.name, path))
    }

    /// 根行数。
    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.handle);
            h = h.wrapping_mul(211).wrapping_add(r.is_dir as u32);
            for by in r.name.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("roots={}", self.rows.len())
    }

    /// 诊断面：根名回读。
    pub fn name_of(&self, handle: u32) -> String {
        match self.find(handle) {
            Some(r) => r.name.to_string(),
            None => "missing".to_string(),
        }
    }

    /// 诊断面：根类型查询（缺行显性 None）。
    pub fn root_kind(&self, handle: u32) -> Option<bool> {
        self.find(handle).map(|r| r.is_dir)
    }

    /// 诊断面：目录根计数。
    pub fn dir_count(&self) -> usize {
        self.rows.iter().filter(|r| r.is_dir).count()
    }

    /// 路径段合法性：非空、无裸 '..'、无内嵌 NUL（口径：'\0'）。
    pub fn path_ok(path: &str) -> bool {
        !path.is_empty()
            && !path.contains('\0')
            && !path.split('\\').any(|seg| seg == "..")
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = RootTable::new();
    t.add(4, true, "Devices");
    t.add(8, false, "FileObj");
    // 1) 绝对路径带根必拒。
    got[0] = t.open(4, "\\Abs\\Path").err() == Some(RO_ABS_WITH_ROOT);
    // 2) 未知根必拒。
    got[1] = t.open(99, "rel.txt").err() == Some(RO_BAD_ROOT);
    // 3) 非目录根必拒。
    got[2] = t.open(8, "rel.txt").err() == Some(RO_NOT_DIR);
    // 4) 相对路径无根必拒。
    got[3] = t.open(0, "rel.txt").err() == Some(RO_NO_ROOT);
    // 5) 印账后打开必抓。
    t.seal();
    got[4] = t.open(4, "rel.txt").err() == Some(RO_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12086_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12086");
    let mk = |dir_root: bool| {
        let mut t = RootTable::new();
        t.add(4, dir_root, "Devices");
        t
    };

    // J1 绝对带根拒。
    let j1 = mk(true).open(4, "\\Abs").err() == Some(RO_ABS_WITH_ROOT);
    s.add("J1-绝对带根拒", j1, "绝对路径不持根");

    // J2 未知根拒。
    let j2 = mk(true).open(99, "rel").err() == Some(RO_BAD_ROOT);
    s.add("J2-未知根拒", j2, "根句柄必须存在");

    // J3 非目录根拒。
    let j3 = mk(false).open(4, "rel").err() == Some(RO_NOT_DIR);
    s.add("J3-非目录拒", j3, "根须目录类型");

    // J4 相对无根拒。
    let j4 = mk(true).open(0, "rel").err() == Some(RO_NO_ROOT);
    s.add("J4-相对无根拒", j4, "相对路径必须持根");

    // J5 双空拒。
    let j5 = mk(true).open(0, "").err() == Some(RO_NULL_BOTH);
    s.add("J5-双空拒", j5, "根路径双空必拒");

    // J6 绝对纯路径放行。
    let j6 = {
        let t = RootTable::new();
        t.open(0, "\\Only\\Path").is_ok()
    };
    s.add("J6-绝对放行", j6, "无根绝对路径合法");

    // J7 相对解析拼接。
    let j7 = {
        let t = mk(true);
        match t.open(4, "Sub\\Obj") {
            Ok(p) => p == "\\Devices\\Sub\\Obj",
            Err(_) => false,
        }
    };
    s.add("J7-相对拼接", j7, "根名 + 相对路径");

    // J8 根名回读。
    let j8 = mk(true).name_of(4) == "Devices" && mk(true).name_of(9) == "missing";
    s.add("J8-根名回读", j8, "根名可查且缺行显性");

    // J9 摘要回读。
    let j9 = mk(true).summary().contains("roots=1");
    s.add("J9-摘要回读", j9, "根数可查");

    // J10 印账冻结。
    let j10 = {
        let mut t = mk(true);
        t.seal();
        t.open(4, "rel").err() == Some(RO_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut t = mk(true);
        t.fold() == t.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut t = RootTable::new();
    t.add(4, true, "Devices");
    t.add(8, false, "FileObj");

    s.add("根数回读", t.count() == 2, "双根在册");
    s.add("指纹非零", t.fold() != 0, "指纹有效");

    // J13 根类型查询。
    let j13 = t.root_kind(4) == Some(true) && t.root_kind(8) == Some(false)
        && t.root_kind(99).is_none();
    s.add("J13-根类型查询", j13, "目录性可查且缺行显性");

    // J14 目录根计数。
    let j14 = t.dir_count() == 1;
    s.add("J14-目录根计数", j14, "仅 Devices 为目录");

    // J15 路径段合法性。
    let j15 = RootTable::path_ok("Sub\\Obj")
        && !RootTable::path_ok("..\\Esc")
        && !RootTable::path_ok("")
        && !RootTable::path_ok("bad\0nul");
    s.add("J15-路径段合法", j15, "逃逸/空/内嵌NUL 皆拒");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12086_open_flow() {
        let mut t = RootTable::new();
        t.add(4, true, "Devices");
        assert_eq!(t.open(0, "rel"), Err(RO_NO_ROOT));
        assert_eq!(t.open(4, "\\abs"), Err(RO_ABS_WITH_ROOT));
        assert!(t.open(4, "sub").is_ok());
    }

    #[test]
    fn unx12086_checks_green() {
        let s = run_unx12086_checks();
        assert!(s.all_passed(), "UNX-F12086 判据未全绿");
    }

    #[test]
    fn not_dir_guard() {
        let mut t = RootTable::new();
        t.add(8, false, "FileObj");
        assert_eq!(t.open(8, "rel"), Err(RO_NOT_DIR));
    }

    #[test]
    fn root_kind_and_path_gate() {
        let mut t = RootTable::new();
        t.add(4, true, "Devices");
        assert_eq!(t.root_kind(4), Some(true));
        assert_eq!(t.root_kind(9), None);
        assert_eq!(t.dir_count(), 1);
        assert!(!RootTable::path_ok("..\\x"));
        assert!(RootTable::path_ok("a\\b"));
    }
}
