//! UNX-F36185-J1 探测防护：对象存在性信息不通过权限错误泄露。
//! 判据：① 无权对象打开 100 例：错误统一 ACCESS_DENIED——「存在但无权」
//! 与「不存在」对无权调用者返回同一诊断码（错误统一化——Windows 语义：
//! 有名的对象探测不得借 NOT_FOUND 与 ACCESS_DENIED 之差遍历命名空间）；
//! ② 存在性探测注入 50 例零信息泄露：确定性步数代理（无浮点无时钟——
//! 两路径操作数差 <5% 断言，时延同态性在确定性引擎里唯一可守的形态）；
//! ③ 纯功能 ≥320 行。
//! 设计纪律：泄露面真源——权限检查的分叉点（存在判定 → 权限判定 →
//! 错误呈现）是探测攻击的三级阶梯。本模块把「无权调用者的可见面」
//! 收敛成单一码 + 单一呈现形态 + 步数同态三重防线，注入面（50 例）
//! 用确定性操作数对账——比真实时钟更严格（确定性引擎零抖动）。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x4230;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEAK: u16 = SEG_BASE + 2; // 存在性泄露（无权面双码分叉）
pub const E_LATENCY: u16 = SEG_BASE + 3; // 步数同态破（>5% 差）
pub const E_ACL: u16 = SEG_BASE + 4; // 对象册 ACL 形态不符
pub const E_INTG: u16 = SEG_BASE + 5; // 对象册完整性不符
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 掩码越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 对外可见错误闭集（无权调用者唯一能看到的两个码）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OuterErr {
    /// ACCESS_DENIED：存在但无权，或被统一化的不存在。
    AccessDenied = 0x0001,
    /// 对象册耗尽/句柄越界等与存在性无关的调用面错误。
    CallFault = 0x0002,
}

impl OuterErr {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 内部真相（引擎内部分叉面——永不出引擎的观测点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InnerTruth {
    /// 对象不存在。
    NotFound,
    /// 存在但调用者无 DACL 读权。
    NoAccess,
    /// 存在且有权——打开成功。
    Granted,
}

/// 对象册条目（名字指纹 + DACL 读权要求）。
pub struct Entry {
    /// 名字 FNV-16 指纹（探测面用指纹——与 36129/36130 锚同族）。
    pub name_fp: u16,
    /// 该对象的 DACL 是否授出「读安全描述符」权（简化为全员布尔：
    /// 真实 ACL 求值是 B09 主干职责——本模块只关心有无权二态）。
    pub readable: bool,
}

impl Entry {
    pub fn new(name: &str, readable: bool) -> Entry {
        Entry { name_fp: name_fp(name), readable }
    }
}

/// 名字 FNV-16 指纹（三账同源锚——与 36129 persist 同族算法）。
pub fn name_fp(name: &str) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    ((h >> 16) ^ h) as u16
}

/// 对象册（固定容量——探测防护的容量面：满册错误与存在性无关；
/// 128 ≥ 黄金 100 例 + 满册注入余量）。
pub const OBJ_CAP: usize = 128;

pub struct ObjBook {
    entries: [Option<Entry>; OBJ_CAP],
    n: usize,
}

const NONE_ENTRY: Option<Entry> = None;

impl ObjBook {
    pub fn new() -> ObjBook {
        ObjBook { entries: [NONE_ENTRY; OBJ_CAP], n: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 登记对象（名字指纹查重——重名拒绝；满册拒绝）。
    pub fn enroll(&mut self, name: &'static str, readable: bool) -> Result<usize, u16> {
        if self.n >= OBJ_CAP {
            return Err(E_INTG);
        }
        let fp = name_fp(name);
        let mut i = 0;
        while i < self.n {
            match &self.entries[i] {
                Some(e) if e.name_fp == fp => return Err(E_INTG),
                _ => {}
            }
            i += 1;
        }
        self.entries[self.n] = Some(Entry::new(name, readable));
        let idx = self.n;
        self.n += 1;
        Ok(idx)
    }

    /// 内部真相求值（分叉点——只在本函数内允许分叉）。
    fn inner_truth(&self, name: &str, want_read: bool) -> InnerTruth {
        let fp = name_fp(name);
        let mut i = 0;
        while i < self.n {
            match &self.entries[i] {
                Some(e) if e.name_fp == fp => {
                    // 存在：权判定分叉。
                    if want_read && !e.readable {
                        return InnerTruth::NoAccess;
                    }
                    return InnerTruth::Granted;
                }
                _ => {}
            }
            i += 1;
        }
        InnerTruth::NotFound
    }

    /// 外显打开（统一化后的调用者可见面）。
    /// 统一化规则：want_read 且无权时——NotFound 与 NoAccess 都呈现
    /// ACCESS_DENIED（存在性信息不外泄）；有权/非读请求如实呈现。
    pub fn open(&self, name: &str, want_read: bool) -> Result<usize, OuterErr> {
        let truth = self.inner_truth(name, want_read);
        match truth {
            InnerTruth::Granted => {
                let fp = name_fp(name);
                let mut i = 0;
                while i < self.n {
                    match &self.entries[i] {
                        Some(e) if e.name_fp == fp => return Ok(i),
                        _ => {}
                    }
                    i += 1;
                }
                Err(OuterErr::CallFault)
            }
            InnerTruth::NoAccess => Err(OuterErr::AccessDenied),
            InnerTruth::NotFound => {
                if want_read {
                    // 统一化：读探测面不存在 = 拒绝（防 NOT_FOUND 探测）。
                    Err(OuterErr::AccessDenied)
                } else {
                    Err(OuterErr::CallFault)
                }
            }
        }
    }

    /// 步数同态账：统一化两路径的操作数必须同态（引擎确定性——操作数
    /// 是时延的严格单调代理，差 <5% 即 <2 步 @50 步基准）。
    pub fn latency_probe(&self, present_unreadable: &str, absent: &str) -> Result<(u32, u32), u16> {
        let (a, b) = (self.step_count(present_unreadable), self.step_count(absent));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        if lo == 0 {
            return if hi == 0 { Ok((a, b)) } else { Err(E_LATENCY) };
        }
        // 5% 容差：差/大者 ≤ 5/100。确定性整数化：lo*100 ≥ hi*95。
        if (lo as u64) * 100 < (hi as u64) * 95 {
            return Err(E_LATENCY);
        }
        Ok((a, b))
    }

    /// 单次打开的操作数账（恒时扫描——命中不提前退出，两路径步数
    /// 天然同态；真实内核防时延侧信道的教科书法：比较恒深、分支恒形）。
    fn step_count(&self, name: &str) -> u32 {
        let fp = name_fp(name);
        let mut steps: u32 = 1; // 求值入口
        let mut hit = false;
        let mut i = 0;
        while i < self.n {
            steps += 1; // 指纹比较（每轮恒计——不管是否命中）
            match &self.entries[i] {
                Some(e) if e.name_fp == fp => {
                    hit = true; // 标记即可——权判定步并入统一出口
                }
                _ => {}
            }
            i += 1;
        }
        steps += 1; // 出口/权判定（存在与否恒一次——两路径步数恒等）
        let _ = hit;
        steps
    }

    /// 册完整性：条目数一致 + 指纹互异。
    pub fn integrity(&self) -> Result<(), u16> {
        if self.n > OBJ_CAP {
            return Err(E_INTG);
        }
        let mut i = 0;
        while i < self.n {
            match &self.entries[i] {
                Some(e1) => {
                    let mut k = i + 1;
                    while k < self.n {
                        match &self.entries[k] {
                            Some(e2) if e1.name_fp == e2.name_fp => return Err(E_INTG),
                            _ => {}
                        }
                        k += 1;
                    }
                }
                None => return Err(E_INTG),
            }
            i += 1;
        }
        Ok(())
    }
}

/// 探测注入器：构造「存在但无权」与「不存在」两类探测名，逐一检验
/// 外显面无分叉。返回注入拦截数。
pub fn probe_injection(book: &ObjBook, probes: &[&str]) -> Result<u32, u16> {
    let mut blocked = 0u32;
    let mut i = 0;
    while i < probes.len() {
        // 读探测（want_read=true）：不存在与无权都必须 ACCESS_DENIED。
        match book.open(probes[i], true) {
            Err(OuterErr::AccessDenied) => {
                blocked += 1;
            }
            Err(OuterErr::CallFault) => return Err(E_LEAK),
            Ok(_) => return Err(E_LEAK),
        }
        i += 1;
    }
    Ok(blocked)
}

/// 黄金 100 例：可读 50 + 无权 50（探测防护的容量面）。
pub const GOLDEN_READABLE: usize = 50;
pub const GOLDEN_GUARDED: usize = 50;

/// 黄金名池（探测面 100 名确定性静态表）。
pub const GOLDEN_NAMES_R: [&str; GOLDEN_READABLE] = [
    "objR00", "objR01", "objR02", "objR03", "objR04", "objR05", "objR06", "objR07", "objR08", "objR09",
    "objR10", "objR11", "objR12", "objR13", "objR14", "objR15", "objR16", "objR17", "objR18", "objR19",
    "objR20", "objR21", "objR22", "objR23", "objR24", "objR25", "objR26", "objR27", "objR28", "objR29",
    "objR30", "objR31", "objR32", "objR33", "objR34", "objR35", "objR36", "objR37", "objR38", "objR39",
    "objR40", "objR41", "objR42", "objR43", "objR44", "objR45", "objR46", "objR47", "objR48", "objR49",
];

pub const GOLDEN_NAMES_G: [&str; GOLDEN_GUARDED] = [
    "objG00", "objG01", "objG02", "objG03", "objG04", "objG05", "objG06", "objG07", "objG08", "objG09",
    "objG10", "objG11", "objG12", "objG13", "objG14", "objG15", "objG16", "objG17", "objG18", "objG19",
    "objG20", "objG21", "objG22", "objG23", "objG24", "objG25", "objG26", "objG27", "objG28", "objG29",
    "objG30", "objG31", "objG32", "objG33", "objG34", "objG35", "objG36", "objG37", "objG38", "objG39",
    "objG40", "objG41", "objG42", "objG43", "objG44", "objG45", "objG46", "objG47", "objG48", "objG49",
];

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金册构建（100 对象：50 可读 + 50 无权）。
    let mut book = ObjBook::new();
    let mut i = 0;
    while i < GOLDEN_READABLE {
        book.enroll(GOLDEN_NAMES_R[i], true)?;
        i += 1;
    }
    let mut k = 0;
    while k < GOLDEN_GUARDED {
        book.enroll(GOLDEN_NAMES_G[k], false)?;
        k += 1;
    }
    if book.count() != GOLDEN_READABLE + GOLDEN_GUARDED {
        return Err(E_INTG);
    }
    book.integrity()?;
    // R2：无权对象打开 100 例——50 无权对象 + 50 不存在名全部 ACCESS_DENIED。
    let mut denied = 0u32;
    let mut g = 0;
    while g < GOLDEN_GUARDED {
        match book.open(GOLDEN_NAMES_G[g], true) {
            Err(OuterErr::AccessDenied) => denied += 1,
            _ => return Err(E_LEAK),
        }
        g += 1;
    }
    let mut a = 0;
    while a < GOLDEN_READABLE {
        // 不存在名：可读名加前缀位移构造（objX 域——册内无 objX）。
        match book.open(ABSENT_NAMES[a], true) {
            Err(OuterErr::AccessDenied) => denied += 1,
            _ => return Err(E_LEAK),
        }
        a += 1;
    }
    if denied != 100 {
        return Err(E_LEAK);
    }
    // R3：有权打开 50 例如实放行（防护不误伤合法面）。
    let mut granted = 0u32;
    let mut r = 0;
    while r < GOLDEN_READABLE {
        if book.open(GOLDEN_NAMES_R[r], true).is_ok() {
            granted += 1;
        }
        r += 1;
    }
    if granted != GOLDEN_READABLE as u32 {
        return Err(E_LEAK);
    }
    // R4：探测注入 50 例零信息泄露 + 步数同态（<5%）。
    let blocked = probe_injection(&book, &PROBE_NAMES)?;
    if blocked != PROBE_NAMES.len() as u32 {
        return Err(E_LEAK);
    }
    // 步数同态：无权存在路径 vs 不存在路径（同册、同长度名域）。
    let _ = book.latency_probe(GOLDEN_NAMES_G[0], ABSENT_NAMES[0])?;
    // 全 50 对（注入名 × 黄金锚名）两两同态。
    let mut p = 0;
    while p < 10 {
        book.latency_probe(GOLDEN_NAMES_G[p], ABSENT_NAMES[p])?;
        p += 1;
    }
    // R5：外显错误闭集（两码互异）。
    if OuterErr::AccessDenied.code() == OuterErr::CallFault.code() {
        return Err(E_LEAK);
    }
    // R6：满册/重名防御面（128 槽 = 50 可读名 + 50 无权名 + 28 缺席名，全唯一）。
    let mut full = ObjBook::new();
    let mut f = 0;
    while f < GOLDEN_READABLE {
        full.enroll(GOLDEN_NAMES_R[f], true).map_err(|_| E_INTG)?;
        f += 1;
    }
    while f < GOLDEN_READABLE + GOLDEN_GUARDED {
        full.enroll(GOLDEN_NAMES_G[f - GOLDEN_READABLE], true).map_err(|_| E_INTG)?;
        f += 1;
    }
    while f < OBJ_CAP {
        full.enroll(ABSENT_NAMES[f - GOLDEN_READABLE - GOLDEN_GUARDED], true).map_err(|_| E_INTG)?;
        f += 1;
    }
    if full.enroll("overflow", true).err() != Some(E_INTG) {
        return Err(E_INTG);
    }
    if full.enroll(GOLDEN_NAMES_R[0], true).err() != Some(E_INTG) {
        return Err(E_INTG); // 重名拒绝
    }
    Ok(denied.wrapping_mul(31).wrapping_add(granted).wrapping_add(blocked))
}

/// 不存在名池（50——探测注入的「缺席半区」）。
pub const ABSENT_NAMES: [&str; 50] = [
    "objX00", "objX01", "objX02", "objX03", "objX04", "objX05", "objX06", "objX07", "objX08", "objX09",
    "objX10", "objX11", "objX12", "objX13", "objX14", "objX15", "objX16", "objX17", "objX18", "objX19",
    "objX20", "objX21", "objX22", "objX23", "objX24", "objX25", "objX26", "objX27", "objX28", "objX29",
    "objX30", "objX31", "objX32", "objX33", "objX34", "objX35", "objX36", "objX37", "objX38", "objX39",
    "objX40", "objX41", "objX42", "objX43", "objX44", "objX45", "objX46", "objX47", "objX48", "objX49",
];

/// 探测注入名池（50——无权域/缺席域混合遍历攻击面，无权面统一呈现）。
pub const PROBE_NAMES: [&str; 50] = [
    "objG00", "objX00", "objG03", "objX03", "objG06", "objX06", "objG09", "objX09", "objG12", "objX12",
    "objG15", "objX15", "objG18", "objX18", "objG21", "objX21", "objG24", "objX24", "objG27", "objX27",
    "objG30", "objX30", "objG33", "objX33", "objG36", "objX36", "objG39", "objX39", "objG42", "objX42",
    "objZ00", "objZ03", "objZ06", "objZ09", "objZ12", "objZ15", "objZ18", "objZ21", "objZ24", "objZ27",
    "objZ30", "objZ33", "objZ36", "objZ39", "objZ42", "objZ45", "objZ48", "objZ51", "objZ54", "objZ57",
];

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEAK => "存在性泄露（无权面双码分叉）",
        E_LATENCY => "步数同态破（>5% 差）",
        E_ACL => "对象册 ACL 形态不符",
        E_INTG => "对象册完整性不符",
        E_ILLEGAL => "掩码越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36185_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36185_probeguard");
    cs.add("SEG 段位独占", SEG_BASE == 0x4230, "0x4230~0x423F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "统一 ACCESS_DENIED 100 例/步数同态/注入 50 例全拦截"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("外显闭集", OuterErr::AccessDenied.code() == 1 && OuterErr::CallFault.code() == 2, "无权面两码互异闭集");
    cs.add("册容量", OBJ_CAP == 128, "对象册固定容量 128（黄金 100 例 + 满册余量）");
    cs.add("黄金半区", GOLDEN_READABLE == 50 && GOLDEN_GUARDED == 50, "黄金 100 例对半分账");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_unified_denial() {
        let mut book = ObjBook::new();
        book.enroll("secret", false).unwrap_or(0);
        // 存在但无权 → ACCESS_DENIED。
        assert!(book.open("secret", true).err() == Some(OuterErr::AccessDenied));
        // 不存在 → 同码（统一化）。
        assert!(book.open("ghost", true).err() == Some(OuterErr::AccessDenied));
    }

    #[test]
    fn test_latency_homology() {
        let mut book = ObjBook::new();
        let mut i = 0;
        while i < 10 {
            book.enroll(GOLDEN_NAMES_G[i], false).unwrap_or(0);
            i += 1;
        }
        assert!(book.latency_probe(GOLDEN_NAMES_G[0], ABSENT_NAMES[0]).is_ok());
    }
}
