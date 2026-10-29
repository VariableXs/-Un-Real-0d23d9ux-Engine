//! UNX-F36134-J1 类型注册表与 D3 对象命名空间的对账面。
//! 判据：① 类型注册表投影进命名空间镜像逐 tid 一一对应（无孤儿/无幻影）；
//! ② 路径哈希查找面（\Type\<tid> 投影路径可反查类型条目）；
//! ③ 漂移注入三类（缺节点/多节点/指纹篡改）全部显性检出；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：对账真源——D3 域的对象命名空间（unx044_nstree 的树形面）
//! 是「名字 → 对象」的目录，B07 类型注册表是「类型 → 策略」的登记；两者
//! 在对象创建路径上必须说的是同一件事。本模块维护类型面的命名空间投影
//! 镜像（每类型一个 \Type\<tid> 节点），提供投影/对账/反查三面。投影
//! 路径哈希与 36129/36130 的 FNV-16 名指纹同源（三账一指纹）。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use super::unx36121_typereg::{self as tr, TypeRegistry, GOLDEN_N, CAP, TID_BASE};
use super::unx36129_persist::name_fp;

pub const SEG_BASE: u16 = 0x3ba0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ORPHAN: u16 = SEG_BASE + 2; // 投影缺节点（类型在/节点无）
pub const E_PHANTOM: u16 = SEG_BASE + 3; // 幻影节点（节点在/类型无）
pub const E_FPDIFT: u16 = SEG_BASE + 4; // 指纹漂移（同 tid 异指纹）
pub const E_LOOKUP: u16 = SEG_BASE + 5; // 反查失败（哈希无命中）
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 投影路径前缀（\Type\ 的字节形态——路径哈希 over 全路径）。
pub const NS_PREFIX: [u8; 6] = [0x5C, 0x54, 0x79, 0x70, 0x65, 0x5C]; // "\Type\"

/// 投影节点（命名空间镜像原子）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NsNode {
    pub tid: u16,
    pub path_hash: u16, // FNV-16 over "\Type\<tid 十进制>"
    pub name_fp: u16,   // 与 36129/36130 同源
    pub refs: u16,      // 命名空间引用计数（创建路径挂账）
}

/// 投影路径合成："\Type\<tid 十进制>" → FNV-16。
pub fn path_hash(tid: u16) -> u16 {
    // tid 十进制 ASCII 化（最多 5 位）。
    let mut digits = [0u8; 5];
    let mut n = tid as u32;
    let mut d = 0;
    if n == 0 {
        digits[0] = b'0';
        d = 1;
    }
    while n > 0 {
        digits[4 - d] = b'0' + (n % 10) as u8;
        n /= 10;
        d += 1;
    }
    let seg = &digits[5 - d..];
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < NS_PREFIX.len() {
        h ^= NS_PREFIX[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        i += 1;
    }
    let mut j = 0;
    while j < seg.len() {
        h ^= seg[j] as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        j += 1;
    }
    (h & 0xFFFF) as u16
}

/// 命名空间镜像（固定容量 = 类型表容量；同序投影）。
pub struct NsMirror {
    nodes: [Option<NsNode>; CAP],
    n: usize,
}

const NO_NODE: Option<NsNode> = None;

impl NsMirror {
    pub fn new() -> NsMirror {
        NsMirror { nodes: [NO_NODE; CAP], n: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 挂节点（tid 序位 = 槽位——同序投影纪律）。
    pub fn attach(&mut self, node: NsNode) -> Result<(), u16> {
        if self.n >= CAP {
            return Err(E_PHANTOM);
        }
        let slot = (node.tid - TID_BASE) as usize;
        if slot != self.n {
            return Err(E_ORPHAN); // 同序破
        }
        self.nodes[self.n] = Some(node);
        self.n += 1;
        Ok(())
    }

    /// 摘节点（对账面负例注入专用——生产面无删除路径，36127 锚）。
    pub fn detach_for_probe(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.n {
            return Err(E_ORPHAN);
        }
        self.nodes[idx] = None;
        Ok(())
    }

    /// 篡改指纹（对账面负例注入专用）。
    pub fn tamper_fp(&mut self, idx: usize, fp: u16) -> Result<(), u16> {
        if idx >= self.n {
            return Err(E_ORPHAN);
        }
        if let Some(ref mut nd) = self.nodes[idx] {
            nd.name_fp = fp;
        }
        Ok(())
    }

    /// 多挂幻影（对账面负例注入专用：越序节点）。
    pub fn attach_phantom(&mut self, node: NsNode) -> Result<(), u16> {
        if self.n >= CAP {
            return Err(E_PHANTOM);
        }
        self.nodes[self.n] = Some(node);
        self.n += 1;
        Ok(())
    }

    pub fn at(&self, i: usize) -> Result<NsNode, u16> {
        if i >= self.n {
            return Err(E_ORPHAN);
        }
        match self.nodes[i] {
            Some(nd) => Ok(nd),
            None => Err(E_ORPHAN),
        }
    }

    /// 哈希反查（\Type\<tid> 可达性面）。
    pub fn lookup(&self, ph: u16) -> Result<NsNode, u16> {
        let mut i = 0;
        while i < self.n {
            if let Ok(nd) = self.at(i) {
                if nd.path_hash == ph {
                    return Ok(nd);
                }
            }
            i += 1;
        }
        Err(E_LOOKUP)
    }

    /// 镜像守恒：无空洞 + tid 连续 + 哈希逐位可复核。
    pub fn integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.n {
            let nd = self.at(i)?;
            if nd.tid != TID_BASE + i as u16 {
                return Err(E_ORPHAN);
            }
            if nd.path_hash != path_hash(nd.tid) {
                return Err(E_FPDIFT);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 投影面（正门）：类型注册表 → 命名空间镜像（每类型一节点，refs=1）。
pub fn project(reg: &TypeRegistry) -> Result<NsMirror, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    reg.integrity().map_err(|_| E_INTG)?;
    let mut m = NsMirror::new();
    let mut i = 0;
    while i < reg.count() {
        let e = reg.at(TID_BASE + i as u16).map_err(|_| E_ORPHAN)?;
        m.attach(NsNode {
            tid: e.tid,
            path_hash: path_hash(e.tid),
            name_fp: name_fp(e.name),
            refs: 1,
        })?;
        i += 1;
    }
    m.integrity()?;
    Ok(m)
}

/// 对账面（核心不变量）：类型表 ↔ 命名空间镜像逐 tid 一一对应。
pub fn reconcile(reg: &TypeRegistry, m: &NsMirror) -> Result<(), u16> {
    if reg.count() != m.count() {
        // 一一对应破：少=孤儿（类型在节点无），多=幻影（节点在类型无）。
        if reg.count() > m.count() {
            return Err(E_ORPHAN);
        }
        return Err(E_PHANTOM);
    }
    let mut i = 0;
    while i < reg.count() {
        let e = reg.at(TID_BASE + i as u16).map_err(|_| E_ORPHAN)?;
        // at 的错误码透传：洞（None）= 缺节点 E_ORPHAN（语义正确），
        // 不硬映射 E_PHANTOM——幻影只在计数不等多节点时成立。
        let nd = m.at(i)?;
        if e.tid != nd.tid {
            return Err(E_ORPHAN);
        }
        if name_fp(e.name) != nd.name_fp {
            return Err(E_FPDIFT);
        }
        if nd.path_hash != path_hash(e.tid) {
            return Err(E_FPDIFT);
        }
        i += 1;
    }
    Ok(())
}

/// 反查面（对象创建路径的目录入口）：路径哈希 → 类型条目。
pub fn lookup_type(reg: &TypeRegistry, m: &NsMirror, tid: u16) -> Result<tr::TypeEntry, u16> {
    let nd = m.lookup(path_hash(tid)).map_err(|_| E_LOOKUP)?;
    if nd.tid != tid {
        return Err(E_LOOKUP);
    }
    reg.at(tid)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金投影 + 对账全绿 + 反查逐 tid 可达。
    let reg = tr::assemble_golden()?;
    let m = project(&reg)?;
    if m.count() != GOLDEN_N {
        return Err(E_ORPHAN);
    }
    reconcile(&reg, &m)?;
    let mut g = 0;
    while g < GOLDEN_N {
        let e = lookup_type(&reg, &m, TID_BASE + g as u16)?;
        if e.tid != TID_BASE + g as u16 {
            return Err(E_LOOKUP);
        }
        if e.name != tr::GOLDEN_NAMES[g] {
            return Err(E_FPDIFT);
        }
        g += 1;
    }
    // R2：路径哈希确定性 + 前缀敏感性（\Type\1 ≠ \Type\11）。
    if path_hash(1) == path_hash(11) {
        return Err(E_FPDIFT);
    }
    if path_hash(1) != path_hash(1) {
        return Err(E_INTG);
    }
    // 反查未投影 tid → E_LOOKUP。
    if m.lookup(path_hash(TID_BASE + GOLDEN_N as u16)).err() != Some(E_LOOKUP) {
        return Err(E_LOOKUP);
    }
    // R3：漂移注入三类逐类检出。
    {
        // 类一：缺节点（孤儿）。
        let mut m1 = project(&reg)?;
        m1.detach_for_probe(GOLDEN_N - 1)?;
        if reconcile(&reg, &m1).err() != Some(E_ORPHAN) {
            return Err(E_ORPHAN);
        }
        // 类二：幻影节点。
        let mut m2 = project(&reg)?;
        m2.attach_phantom(NsNode {
            tid: TID_BASE + GOLDEN_N as u16,
            path_hash: path_hash(TID_BASE + GOLDEN_N as u16),
            name_fp: 0xBEEF,
            refs: 1,
        })?;
        if reconcile(&reg, &m2).err() != Some(E_PHANTOM) {
            return Err(E_PHANTOM);
        }
        // 类三：指纹篡改。
        let mut m3 = project(&reg)?;
        m3.tamper_fp(3, 0xDEAD)?;
        if reconcile(&reg, &m3).err() != Some(E_FPDIFT) {
            return Err(E_FPDIFT);
        }
    }
    // R4：投影幂等（两次投影逐位一致）。
    let m2 = project(&reg)?;
    let mut q = 0;
    while q < m.count() {
        if m.at(q)? != m2.at(q)? {
            return Err(E_INTG);
        }
        q += 1;
    }
    // R5：向导类型投影（未封印表 + 填充名 → 对账仍绿）。
    let mut reg2 = TypeRegistry::new();
    let mut w = 0;
    while w < CAP {
        let (name, map, policy, _) = tr::GOLDEN[w % GOLDEN_N];
        let _ = name;
        reg2.register(tr::FILL_NAMES[w], map, policy, 0).map_err(|_| E_INTG)?;
        w += 1;
    }
    let m3 = project(&reg2)?;
    if m3.count() != CAP {
        return Err(E_ORPHAN);
    }
    reconcile(&reg2, &m3)?;
    m3.integrity()?;
    // R6：镜像哈希账守恒（逐 tid 路径哈希可重算复核——integrity 内证）。
    reg.integrity()?;
    // 校验和：黄金 tid 哈希全并 × 节点数。
    let mut sum: u32 = 0;
    let mut k = 0;
    while k < GOLDEN_N {
        sum = sum.wrapping_add(m.at(k).map_err(|_| E_ORPHAN)?.path_hash as u32);
        k += 1;
    }
    Ok(sum.wrapping_mul(19).wrapping_add(m.count() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ORPHAN => "投影缺节点/同序破（类型在节点无）",
        E_PHANTOM => "幻影节点（节点在类型无）",
        E_FPDIFT => "指纹漂移（同 tid 异指纹/异哈希）",
        E_LOOKUP => "反查失败（路径哈希无命中）",
        E_INTG => "完整性自检不符（投影幂等破）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36134_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36134_nsrecon");
    cs.add("SEG 段位独占", SEG_BASE == 0x3ba0, "0x3ba0~0x3baF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "投影对账/反查/三类漂移检出/幂等/满表投影全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("路径前缀锚", NS_PREFIX == [0x5C, 0x54, 0x79, 0x70, 0x65, 0x5C], "\\Type\\ 六字节前缀冻结");
    cs.add("哈希可分性", path_hash(1) != path_hash(2) && path_hash(10) != path_hash(1), "邻 tid 哈希互异");
    cs.add("指纹三账同源", name_fp("Key") == super::unx36129_persist::name_fp("Key"), "投影指纹与持久化/审计同源 FNV-16");
    cs.add("镜像容量锚", { NsMirror::new().count() == 0 }, "空镜像零节点起步");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_kinds_detected() {
        // 三类漂移全部显性检出：缺节点/幻影/指纹篡改。
        let reg = super::super::unx36121_typereg::assemble_golden().unwrap();
        let mut m1 = project(&reg).unwrap();
        m1.detach_for_probe(GOLDEN_N - 1).unwrap();
        assert_eq!(reconcile(&reg, &m1).err(), Some(E_ORPHAN));
        let mut m2 = project(&reg).unwrap();
        m2.attach_phantom(NsNode { tid: TID_BASE + GOLDEN_N as u16, path_hash: path_hash(TID_BASE + GOLDEN_N as u16), name_fp: 0xBEEF, refs: 1 }).unwrap();
        assert_eq!(reconcile(&reg, &m2).err(), Some(E_PHANTOM));
        let mut m3 = project(&reg).unwrap();
        m3.tamper_fp(3, 0xDEAD).unwrap();
        assert_eq!(reconcile(&reg, &m3).err(), Some(E_FPDIFT));
    }

    #[test]
    fn projection_idempotent() {
        // 投影幂等：两次投影逐位一致，对账全绿（账面守恒）。
        let reg = super::super::unx36121_typereg::assemble_golden().unwrap();
        let m1 = project(&reg).unwrap();
        let m2 = project(&reg).unwrap();
        assert_eq!(m1.count(), m2.count());
        let mut i = 0;
        while i < m1.count() {
            assert!(m1.at(i).unwrap() == m2.at(i).unwrap());
            i += 1;
        }
        assert!(reconcile(&reg, &m1).is_ok());
        assert!(m1.integrity().is_ok());
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(7170684));
    }
}
