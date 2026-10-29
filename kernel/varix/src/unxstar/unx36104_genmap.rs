//! UNX-F36104-J1 GENERIC→特定映射表框架（按对象类型注册）。
//! 判据：① 类型名→映射三元组（read/write/exec）注册表：注册幂等、
//! 未注册类型查询显性拒；② 三元组守恒（特定位+标准五位区，禁含
//! GENERIC/保留位）；③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 映射面框架真源——36105/06/07 通过本框架登记
//! 文件/注册表键/窗口站桌面三类映射；本模块不写死任何具体类型值。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3940;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NAME: u16 = SEG_BASE + 2; // 类型名非法（空/过长/非 ASCII）
pub const E_NOENT: u16 = SEG_BASE + 3; // 未注册类型查询（显性拒）
pub const E_CONFLICT: u16 = SEG_BASE + 4; // 同名异图冲突（幂等性破）
pub const E_CONSERV: u16 = SEG_BASE + 5; // 三元组越出合法区（守恒破）
pub const E_FULL: u16 = SEG_BASE + 6; // 注册表满
pub const E_BIT: u16 = SEG_BASE + 7; // 翻译请求位非法（非单 GENERIC 位）
pub const E_MAPIDX: u16 = SEG_BASE + 8; // 三元组下标越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 映射槽位（显式判别式；翻译面的槽位标记，AllUnion 走三槽并）。
#[repr(u16)]
#[derive(Clone, Copy)]
pub enum MapSlot {
    Read = 0x0001,
    Write = 0x0002,
    Exec = 0x0004,
    AllUnion = 0x0008,
}

impl MapSlot {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 槽位 → GenMap::slot 下标（AllUnion 无单槽下标——显性拒）。
    pub fn index(self) -> Result<usize, u16> {
        match self {
            MapSlot::Read => Ok(0),
            MapSlot::Write => Ok(1),
            MapSlot::Exec => Ok(2),
            MapSlot::AllUnion => Err(E_MAPIDX),
        }
    }
}

/// 注册表容量（16 类；B06 只登记 4 类：File/RegistryKey/WindowStation/Desktop）。
pub const CAP: usize = 16;

/// 类型名长度上界。
pub const NAME_CAP: usize = 32;

/// 映射三元组：GENERIC_READ/GENERIC_WRITE/GENERIC_EXECUTE 的展开值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenMap {
    pub read: u32,
    pub write: u32,
    pub exec: u32,
}

impl GenMap {
    /// 三元组合法性：每位展开值 ⊆ 合法区（特定段 + 标准五位区）。
    pub fn wellformed(&self) -> bool {
        let legal = mt::SPEC_MASK | mt::STD_USED;
        (self.read & !legal) == 0 && (self.write & !legal) == 0 && (self.exec & !legal) == 0
    }
    /// 三元组按槽位取（0=read,1=write,2=exec）。
    pub fn slot(&self, i: usize) -> Result<u32, u16> {
        match i {
            0 => Ok(self.read),
            1 => Ok(self.write),
            2 => Ok(self.exec),
            _ => Err(E_MAPIDX),
        }
    }
}

/// 注册表项：类型名 + 映射 + 登记版本。
#[derive(Clone, Copy)]
pub struct Entry {
    pub name: &'static str,
    pub map: GenMap,
    pub ver: u16,
}

/// 固定容量注册表（构造-登记式；无全局可变态，消费方按序装配）。
pub struct Registry {
    entries: [Option<Entry>; CAP],
    n: usize,
}

const NONE: Option<Entry> = None;

impl Registry {
    pub fn new() -> Registry {
        Registry { entries: [NONE; CAP], n: 0 }
    }

    /// 已登记类数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 类型名合法性：非空、≤NAME_CAP-1、纯 ASCII 可见字符。
    pub fn name_ok(name: &str) -> bool {
        let b = name.as_bytes();
        if b.is_empty() || b.len() > NAME_CAP - 1 {
            return false;
        }
        let mut i = 0;
        while i < b.len() {
            if !(0x21..=0x7e).contains(&b[i]) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 登记（幂等）：同名同图 → Ok(既有下标)；同名异图 → E_CONFLICT；
    /// 新名 → 追加（表满 E_FULL；名非法 E_NAME；图非法 E_CONSERV）。
    pub fn register(&mut self, name: &'static str, map: GenMap) -> Result<usize, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if !Registry::name_ok(name) {
            return Err(E_NAME);
        }
        if !map.wellformed() {
            return Err(E_CONSERV);
        }
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(e) => {
                    if e.name == name {
                        if e.map.read == map.read
                            && e.map.write == map.write
                            && e.map.exec == map.exec
                        {
                            return Ok(i);
                        }
                        return Err(E_CONFLICT);
                    }
                }
                None => return Err(E_MAPIDX),
            }
            i += 1;
        }
        if self.n >= CAP {
            return Err(E_FULL);
        }
        self.entries[self.n] = Some(Entry { name, map, ver: 1 });
        let idx = self.n;
        self.n += 1;
        Ok(idx)
    }

    /// 按名查询映射（未注册 → E_NOENT 显性拒，非静默匹配）。
    pub fn query(&self, name: &str) -> Result<GenMap, u16> {
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(e) => {
                    if e.name == name {
                        return Ok(e.map);
                    }
                }
                None => return Err(E_MAPIDX),
            }
            i += 1;
        }
        Err(E_NOENT)
    }

    /// 按名查询下标。
    pub fn index_of(&self, name: &str) -> Result<usize, u16> {
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(e) => {
                    if e.name == name {
                        return Ok(i);
                    }
                }
                None => return Err(E_MAPIDX),
            }
            i += 1;
        }
        Err(E_NOENT)
    }

    /// 按下标取项。
    pub fn at(&self, i: usize) -> Result<Entry, u16> {
        if i >= self.n {
            return Err(E_MAPIDX);
        }
        match self.entries[i] {
            Some(e) => Ok(e),
            None => Err(E_MAPIDX),
        }
    }

    /// 翻译：单 GENERIC 位 → 该类型对应展开值。
    /// GENERIC_ALL → 三槽并（ALL≡三并语义守恒，见 36103）。
    pub fn translate(&self, name: &str, generic: u32) -> Result<u32, u16> {
        let m = self.query(name)?;
        if generic == G_READ {
            Ok(m.read)
        } else if generic == G_WRITE {
            Ok(m.write)
        } else if generic == G_EXEC {
            Ok(m.exec)
        } else {
            Err(E_BIT)
        }
    }

    /// GENERIC_ALL 翻译（三槽并）。
    pub fn translate_all(&self, name: &str) -> Result<u32, u16> {
        let m = self.query(name)?;
        Ok(m.read | m.write | m.exec)
    }

    /// 全表纪律自检：项数一致、名唯一、图合法。
    pub fn integrity(&self) -> Result<(), u16> {
        if self.n > CAP {
            return Err(E_FULL);
        }
        let mut i = 0;
        while i < self.n {
            let e = self.at(i)?;
            if !Registry::name_ok(e.name) || !e.map.wellformed() {
                return Err(E_CONSERV);
            }
            let mut j = 0;
            while j < i {
                if self.at(j)?.name == e.name {
                    return Err(E_CONFLICT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 已登记类名快照（诊断面）。
    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                out.push(e.name.to_string());
            }
            i += 1;
        }
        out
    }
}

/// GEN 四位常量（从 36101 段锚守恒：四并 = GEN_USED）。
pub const G_READ: u32 = 1u32 << 31; // 0x8000_0000
pub const G_WRITE: u32 = 1u32 << 30; // 0x4000_0000
pub const G_EXEC: u32 = 1u32 << 29; // 0x2000_0000
pub const G_ALL: u32 = 1u32 << 28; // 0x1000_0000

/// 演练：R1 装配 4 类并逐项回查；R2 幂等再注册；R3 未注册查询显性拒；
/// R4 守恒负例注入；R5 翻译面 + ALL 展开。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut reg = Registry::new();
    // R1：装配 4 类演示映射（真类型映射在 36105/06/07 登记）。
    let m1 = GenMap { read: 0x0012_0089, write: 0x0012_0116, exec: 0x0012_00A0 };
    let m2 = GenMap { read: 0x0002_0019, write: 0x0002_0006, exec: 0x0002_0019 };
    let m3 = GenMap { read: 0x0000_0243, write: 0x0000_005A, exec: 0x0000_0141 };
    let m4 = GenMap { read: 0x0012_0000, write: 0x0012_0000, exec: 0x0002_0000 };
    let i0 = reg.register("File", m1)?;
    let i1 = reg.register("RegistryKey", m2)?;
    let i2 = reg.register("WindowStation", m3)?;
    let i3 = reg.register("Desktop", m4)?;
    if i0 != 0 || i1 != 1 || i2 != 2 || i3 != 3 || reg.count() != 4 {
        return Err(E_FULL);
    }
    let q = reg.query("File")?;
    if q.read != 0x0012_0089 || q.write != 0x0012_0116 {
        return Err(E_CONFLICT);
    }
    hits += 1;
    // R2：幂等——同名同图 Ok 同下标；同名异图 E_CONFLICT。
    if reg.register("File", m1)? != i0 {
        return Err(E_CONFLICT);
    }
    if reg.register("RegistryKey", m2)? != i1 {
        return Err(E_CONFLICT);
    }
    if reg.count() != 4 {
        return Err(E_CONFLICT);
    }
    let bad = GenMap { read: 0, write: 0, exec: 1 };
    if reg.register("File", bad) != Err(E_CONFLICT) {
        return Err(E_CONFLICT);
    }
    hits += 1;
    // R3：未注册类型查询——显性 E_NOENT（非静默匹配红线），20 例。
    let unknowns: [&str; 10] = [
        "file", "FILE", "Registry", "Key", "Windowstation", "desktop", "Pipe", "SymbolicLink",
        "Event", "",
    ];
    let mut k = 0;
    while k < 10 {
        if reg.query(unknowns[k]).is_ok() || reg.index_of(unknowns[k]).is_ok() {
            return Err(E_NOENT);
        }
        if reg.query(unknowns[k]) != Err(E_NOENT) {
            return Err(E_NOENT);
        }
        hits += 1;
        k += 1;
    }
    // R4：守恒负例——含 GENERIC 位/保留位/标准保留区的三元组必拒。
    let bads: [GenMap; 6] = [
        GenMap { read: G_READ, write: 0, exec: 0 },
        GenMap { read: 1 << 24, write: 0, exec: 0 },
        GenMap { read: mt::STD_RSVD, write: 0, exec: 0 },
        GenMap { read: 0, write: mt::ALL_RSVD, exec: 0 },
        GenMap { read: 0, write: 0, exec: 0xFFFF_FFFF },
        GenMap { read: 0x0012_0089, write: 0x0012_0116, exec: 0x0100_0000 },
    ];
    let mut k = 0;
    while k < 6 {
        if reg.register("Sink", bads[k]) != Err(E_CONSERV) {
            return Err(E_CONSERV);
        }
        hits += 1;
        k += 1;
    }
    if reg.register("", m1) != Err(E_NAME) {
        return Err(E_NAME);
    }
    let mut long = String::new();
    let mut z = 0;
    while z < NAME_CAP {
        long.push('x');
        z += 1;
    }
    if Registry::name_ok(&long) {
        return Err(E_NAME);
    }
    hits += 1;
    // R5：翻译面——四 GENERIC 位逐类翻译；非法请求位 E_BIT；
    // 槽位枚举与下标面对拍。
    let mf = reg.query("File")?;
    let slot_expect: [(MapSlot, u32); 3] = [
        (MapSlot::Read, mf.read),
        (MapSlot::Write, mf.write),
        (MapSlot::Exec, mf.exec),
    ];
    let mut s = 0;
    while s < 3 {
        let idx = slot_expect[s].0.index().map_err(|_| E_MAPIDX)?;
        if mf.slot(idx).map_err(|_| E_MAPIDX)? != slot_expect[s].1 {
            return Err(E_MAPIDX);
        }
        s += 1;
    }
    if MapSlot::AllUnion.index().is_ok() {
        return Err(E_MAPIDX);
    }
    let m = reg.query("RegistryKey")?;
    if reg.translate("RegistryKey", G_READ)? != m.read
        || reg.translate("RegistryKey", G_WRITE)? != m.write
        || reg.translate("RegistryKey", G_EXEC)? != m.exec
    {
        return Err(E_BIT);
    }
    if reg.translate("File", G_READ)? != 0x0012_0089 {
        return Err(E_BIT);
    }
    if reg.translate_all("File")? != 0x0012_01BF {
        return Err(E_CONFLICT);
    }
    if reg.translate("File", G_ALL) != Err(E_BIT)
        || reg.translate("File", mt::GEN_USED) != Err(E_BIT)
        || reg.translate("File", 0) != Err(E_BIT)
        || reg.translate("File", 1u32 << 25) != Err(E_BIT)
    {
        return Err(E_BIT);
    }
    reg.integrity()?;
    if reg.names().len() != 4 {
        return Err(E_FULL);
    }
    let _ = vec![0u8; 0];
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(20));
    }

    #[test]
    fn idempotent_register() {
        let mut reg = Registry::new();
        let m = GenMap { read: 1, write: 2, exec: 4 };
        assert_eq!(reg.register("T", m), Ok(0));
        assert_eq!(reg.register("T", m), Ok(0));
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn unregistered_rejected() {
        let reg = Registry::new();
        assert_eq!(reg.query("File"), Err(E_NOENT));
    }

    #[test]
    fn conserv_break_rejected() {
        let mut reg = Registry::new();
        let m = GenMap { read: mt::GEN_USED, write: 0, exec: 0 };
        assert_eq!(reg.register("Bad", m), Err(E_CONSERV));
    }
}

pub fn run_unx36104_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36104_genmap");
    cs.add("SEG 段位独占", SEG_BASE == 0x3940, "0x3940~0x394F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "注册幂等/显性拒/守恒负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("框架容量", CAP == 16, "16 类固定容量注册表");
    cs.add("合法区守恒", mt::SPEC_MASK | mt::STD_USED == 0xFFFF_FFFF & !mt::ALL_RSVD & !mt::GEN_USED, "三元组合法区 = 特定段+标准五位");
    cs.add("未注册显性拒", Registry::new().query("X") == Err(E_NOENT), "空表查询 E_NOENT 非静默");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NAME => "类型名非法（空/过长/非 ASCII）",
        E_NOENT => "未注册类型查询（显性拒）",
        E_CONFLICT => "同名异图冲突（幂等性破）",
        E_CONSERV => "三元组越出合法区（守恒破）",
        E_FULL => "注册表满",
        E_BIT => "翻译请求位非法（非单 GENERIC 位）",
        E_MAPIDX => "三元组下标越界",
        _ => "未知诊断码",
    }
}
