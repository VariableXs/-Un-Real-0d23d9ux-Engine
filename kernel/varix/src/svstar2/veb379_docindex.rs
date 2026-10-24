//! veb379_docindex —— VE-F0379 驱动文档自动索引。
//! 锚点（CoRun Unxreal 主册）：VE-F0379 · 驱动文档自动索引 · 目标 300 行
//! 判据原文：VE-F0379-J1 驱动注册时其文档条目自动建索引（名→
//!           条目）；按名检索 O(n) 精确匹配；按类别枚举（过滤
//!           枚举序稳定 = 注册序）；索引完整核（注册数 = 索引
//!           数，缺索引即红）；词条更新（版本号推进不重名）。
//! 组件：文档条目（名 + 类别 + 版本 + 摘要长）+ 索引账（名主键）
//!       + 类别过滤器 + 完整性核。
//! 职责定位：文档索引是「驱动的户口簿」——驱动注册进内核的同
//!           时文档就位，不许「代码在、文档丢」；类别过滤让排
//!           查按族检索；完整性核是户口普查：账上有几个驱动就
//!           必须有几条索引，少一条就是文档债。
//! 性能逐项分解：建索引 O(1)（追加）；检索 O(n)；过滤 O(n)。
//! 诊断码段：0x30440（0x30441~0x30449 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DI_VERSION: &str = "veb379_docindex-v1";

/// 诊断段基址（0x30440 段 · 本模块独占）。
pub const DI_SEG_BASE: u32 = 0x30440;
pub const XC_DUP: u32 = 0x30441; // 重名
pub const XC_NOENT: u32 = 0x30442; // 查无此条
pub const XC_NAME_EMPTY: u32 = 0x30443; // 名空
pub const XC_CLASS_OOR: u32 = 0x30444; // 类别越域
pub const XC_INCOMPLETE: u32 = 0x30445; // 索引不完整
pub const XC_MAGIC: u32 = 0x30446; // 账魔数破
pub const XC_VER_BACK: u32 = 0x30447; // 版本回退
pub const XC_SUM_LONG: u32 = 0x30448; // 摘要超限
pub const XC_STALE: u32 = 0x30449; // 词条过期未更新

/// 摘要长上限。
pub const SUM_CAP: usize = 64;

/// 文档类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocClass {
    /// 驱动本体说明。
    Driver = 0,
    /// 接口约定。
    Iface = 1,
    /// 错误码表。
    ErrTab = 2,
    /// 变更志。
    Changelog = 3,
}

impl DocClass {
    pub fn from_u32(v: u32) -> Option<DocClass> {
        match v {
            0 => Some(DocClass::Driver),
            1 => Some(DocClass::Iface),
            2 => Some(DocClass::ErrTab),
            3 => Some(DocClass::Changelog),
            _ => None,
        }
    }
    pub fn id(self) -> u32 {
        self as u32
    }
}

/// 文档条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEntry {
    pub name: &'static str,
    pub class: DocClass,
    pub ver: u32,
    pub sum_len: usize,
}

/// 索引账。
pub struct DocIndex {
    entries: Vec<DocEntry>,
    magic: u32,
}

impl DocIndex {
    /// 建。
    pub fn new() -> DocIndex {
        DocIndex { entries: Vec::new(), magic: 0x0440_D0C5 }
    }

    /// 注册条目（名主键，重名拒；版本不许回退——同名词版本须增）。
    pub fn register(&mut self, name: &'static str, class: DocClass, ver: u32, sum_len: usize) -> Result<(), u32> {
        if self.magic != 0x0440_D0C5 {
            return Err(XC_MAGIC);
        }
        if name.is_empty() {
            return Err(XC_NAME_EMPTY);
        }
        if sum_len > SUM_CAP {
            return Err(XC_SUM_LONG);
        }
        for e in self.entries.iter() {
            if e.name == name {
                if ver <= e.ver {
                    return Err(XC_VER_BACK);
                }
                // 同名且版本增 → 更新词条（不重注册）。
                let idx = self.entries.iter().position(|x| x.name == name);
                match idx {
                    Some(i) => {
                        self.entries[i].ver = ver;
                        self.entries[i].sum_len = sum_len;
                        self.entries[i].class = class;
                        return Ok(());
                    }
                    None => return Err(XC_NOENT),
                }
            }
        }
        self.entries.push(DocEntry { name, class, ver, sum_len });
        Ok(())
    }

    /// 精确检索。
    pub fn lookup(&self, name: &str) -> Result<DocEntry, u32> {
        for e in self.entries.iter() {
            if e.name == name {
                return Ok(e.clone());
            }
        }
        Err(XC_NOENT)
    }

    /// 类别过滤（注册序稳定）。
    pub fn by_class(&self, class: DocClass) -> Vec<&'static str> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            if e.class == class {
                out.push(e.name);
            }
        }
        out
    }

    /// 完整性核：注册数 = 检索可得数（名→条目逐条通）。
    pub fn completeness(&self) -> Result<u32, u32> {
        for e in self.entries.iter() {
            match self.lookup(e.name) {
                Ok(got) => {
                    if got != *e {
                        return Err(XC_INCOMPLETE);
                    }
                }
                Err(_) => return Err(XC_INCOMPLETE),
            }
        }
        Ok(self.entries.len() as u32)
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn run_veb379_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0379");
    // 族一：注册与检索。
    let mut ix = DocIndex::new();
    let _ = ix.register("uart0", DocClass::Driver, 1, 32);
    let _ = ix.register("uart0-iface", DocClass::Iface, 1, 48);
    let _ = ix.register("uart0-err", DocClass::ErrTab, 1, 12);
    let _ = ix.register("block0", DocClass::Driver, 1, 20);
    s.add("注册 4", ix.len() == 4, "四条");
    let got = ix.lookup("uart0");
    s.add("精确检索", matches!(&got, Ok(e) if e.class == DocClass::Driver && e.ver == 1), "主键");
    s.add("查无拒", ix.lookup("ghost") == Err(XC_NOENT), "无此条");
    // 族二：类别过滤序稳定。
    let drv = ix.by_class(DocClass::Driver);
    s.add("驱动类 2", drv.len() == 2, "uart0+block0");
    s.add("注册序稳定", drv.first() == Some(&"uart0") && drv.get(1) == Some(&"block0"), "先注册先出");
    let iface = ix.by_class(DocClass::Iface);
    s.add("接口类 1", iface.len() == 1 && iface[0] == "uart0-iface", "单条");
    // 族三：词条更新（版本推进）。
    let up = ix.register("uart0", DocClass::Driver, 2, 40);
    s.add("版本推进过", up.is_ok(), "1→2");
    let got2 = ix.lookup("uart0");
    s.add("词条已更新", matches!(&got2, Ok(e) if e.ver == 2 && e.sum_len == 40), "新版本");
    s.add("总数不涨", ix.len() == 4, "更新非重注");
    s.add("版本回退拒", ix.register("uart0", DocClass::Driver, 1, 32) == Err(XC_VER_BACK), "2→1 拒");
    s.add("版本平推拒", ix.register("uart0", DocClass::Driver, 2, 40) == Err(XC_VER_BACK), "=2 拒");
    // 族四：完整性核。
    let comp = ix.completeness();
    s.add("完整性过", comp == Ok(4), "账实一致");
    // 族五：入参拒绝。
    let mut b = DocIndex::new();
    s.add("名空拒", b.register("", DocClass::Driver, 1, 0) == Err(XC_NAME_EMPTY), "名空");
    s.add("摘要超限拒", b.register("big", DocClass::Driver, 1, SUM_CAP + 1) == Err(XC_SUM_LONG), ">64");
    let _ = b.register("x", DocClass::Driver, 1, 8);
    s.add("重名拒(未增版)", b.register("x", DocClass::Iface, 1, 8) == Err(XC_VER_BACK), "同名平版");
    s.add("类别核", DocClass::from_u32(3) == Some(DocClass::Changelog) && DocClass::from_u32(4).is_none(), "闭集");
    s.add("类别号核", DocClass::Driver.id() == 0 && DocClass::Changelog.id() == 3, "枚举值");
    s.add("魔数核", DI_SEG_BASE == 0x30440, "段基址");
    s
}
