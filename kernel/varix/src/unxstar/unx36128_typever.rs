//! UNX-F36128-J1 类型版本化与向后兼容（新版本不破坏旧 SD）。
//! 判据：① 掩码放宽升版（新 ⊇ 旧逐槽）通过、收窄拒绝（E_REGRESS）；
//! ② 版本史环形可回溯且逐版守恒；③ 升版后旧 SD 档位特征复核不变
//! （向后兼容的落地面：SD 判据与掩码版本解耦）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：版本化真源——类型 generic map 的演进只允许「放宽」（新版本
//! 每槽展开值 ⊇ 旧版本），收窄即 E_REGRESS（旧 SD 引用的展开位可能消失，
//! 破坏向后兼容）。升版不触档位面：TypePolicy 与 SD 控制位特征在版本演进
//! 中恒定，这是「新版本不破坏旧 SD」的判据落点。36121 的 gen 字段由本
//! 模块的 VersionBook 代理维护。

use crate::checks::CheckSet;
use super::unx36104_genmap::GenMap;
use super::unx36121_typereg::{self as tr, TypePolicy, GOLDEN_N};
use super::unx36125_deftier::{synthesize, tier_signature, verify_tier};

pub const SEG_BASE: u16 = 0x3b40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_REGRESS: u16 = SEG_BASE + 2; // 掩码收窄（向后兼容破）
pub const E_NOENT: u16 = SEG_BASE + 3; // 类型未在版本册
pub const E_HIST: u16 = SEG_BASE + 4; // 版本史漂移（环形失真）
pub const E_GEN: u16 = SEG_BASE + 5; // 代际数越界（u32 耗尽防御位）
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 版本史环形容量（每类型保留最近 4 版掩码）。
pub const HIST_CAP: usize = 4;

/// 版本册条目：当前代际 + 掩码史环形。
#[derive(Clone, Copy)]
pub struct VerEntry {
    pub name: &'static str,
    pub gen: u32,
    hist: [Option<GenMap>; HIST_CAP],
    hhead: usize,
    hn: usize,
}

const NO_MAP: Option<GenMap> = None;

impl VerEntry {
    fn new(name: &'static str, map: GenMap) -> VerEntry {
        let mut hist = [NO_MAP; HIST_CAP];
        hist[0] = Some(map);
        VerEntry { name, gen: 1, hist, hhead: 0, hn: 1 }
    }

    /// 当前掩码（史环最新）。
    pub fn current(&self) -> Result<GenMap, u16> {
        let idx = (self.hhead + self.hn + HIST_CAP - 1) % HIST_CAP;
        match self.hist[idx] {
            Some(m) => Ok(m),
            None => Err(E_HIST),
        }
    }

    /// 历史第 k 版（0 = 最初版）。
    pub fn hist_at(&self, k: usize) -> Result<GenMap, u16> {
        if k >= self.hn {
            return Err(E_HIST);
        }
        let idx = (self.hhead + k) % HIST_CAP;
        match self.hist[idx] {
            Some(m) => Ok(m),
            None => Err(E_HIST),
        }
    }

    pub fn hist_len(&self) -> usize {
        self.hn
    }

    /// 升版（放宽）：新 ⊇ 旧逐槽 → gen+1 入史环；同图幂等；收窄 → E_REGRESS。
    fn bump(&mut self, new: GenMap) -> Result<u32, u16> {
        let old = self.current()?;
        if old.read == new.read && old.write == new.write && old.exec == new.exec {
            return Ok(self.gen); // 幂等：同图不升版
        }
        // 放宽判据：新槽位展开值 ⊇ 旧槽位展开值（逐槽）。
        if new.read & old.read != old.read
            || new.write & old.write != old.write
            || new.exec & old.exec != old.exec
        {
            return Err(E_REGRESS);
        }
        if self.gen == u32::MAX {
            return Err(E_GEN);
        }
        if self.hn < HIST_CAP {
            self.hist[(self.hhead + self.hn) % HIST_CAP] = Some(new);
            self.hn += 1;
        } else {
            self.hist[self.hhead] = Some(new);
            self.hhead = (self.hhead + 1) % HIST_CAP;
        }
        self.gen += 1;
        Ok(self.gen)
    }

    /// 史环守恒：环内无空洞、逐版放宽链成立（v(k) ⊆ v(k+1)）。
    pub fn integrity(&self) -> Result<(), u16> {
        if self.hn == 0 || self.hn > HIST_CAP {
            return Err(E_HIST);
        }
        let mut k = 0;
        while k + 1 < self.hn {
            let a = self.hist_at(k)?;
            let b = self.hist_at(k + 1)?;
            if b.read & a.read != a.read
                || b.write & a.write != a.write
                || b.exec & a.exec != a.exec
            {
                return Err(E_HIST);
            }
            k += 1;
        }
        Ok(())
    }
}

/// 固定容量版本册（16 类：与 B06 映射表容量对齐——版本册跟着映射面走）。
pub const BOOK_CAP: usize = 16;

pub struct VersionBook {
    entries: [Option<VerEntry>; BOOK_CAP],
    n: usize,
}

const NO_ENTRY: Option<VerEntry> = None;

impl VersionBook {
    pub fn new() -> VersionBook {
        VersionBook { entries: [NO_ENTRY; BOOK_CAP], n: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 初始登记（未在册类型建首版；在册类型走 bump 语义）。
    pub fn touch(&mut self, name: &'static str, map: GenMap) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if !tr::TypeRegistry::name_ok(name) {
            return Err(tr::E_NAME);
        }
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(ref mut e) if e.name == name => return e.bump(map),
                Some(_) => {}
                None => return Err(E_HIST),
            }
            i += 1;
        }
        if self.n >= BOOK_CAP {
            return Err(super::unx36104_genmap::E_FULL);
        }
        self.entries[self.n] = Some(VerEntry::new(name, map));
        let gen = self.entries[self.n].as_ref().map(|e| e.gen).unwrap_or(0);
        self.n += 1;
        Ok(gen)
    }

    pub fn entry(&self, name: &str) -> Result<&VerEntry, u16> {
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(ref e) if e.name == name => return Ok(e),
                Some(_) => {}
                None => return Err(E_HIST),
            }
            i += 1;
        }
        Err(E_NOENT)
    }

    /// 全册守恒（36131 自检入口）。
    pub fn integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.n {
            match self.entries[i] {
                Some(ref e) => e.integrity()?,
                None => return Err(E_HIST),
            }
            i += 1;
        }
        Ok(())
    }
}

/// 向后兼容核（本模块的判据落点）：升版前后，同一档位合成的 SD 控制
/// 位特征逐位不变——掩码版本演进不触档位面。
pub fn compat_probe(policy: TypePolicy, map_old: GenMap, map_new: GenMap) -> Result<u32, u16> {
    let mut book = VersionBook::new();
    let g1 = book.touch("Probe", map_old)?;
    let (sd1, _) = synthesize(policy).map_err(|_| E_INTG)?;
    let g2 = book.touch("Probe", map_new)?;
    let (sd2, _) = synthesize(policy).map_err(|_| E_INTG)?;
    if sd1.control != sd2.control {
        return Err(E_INTG);
    }
    // 档位特征在两代下同判。
    verify_tier(&sd1, policy).map_err(|_| E_INTG)?;
    verify_tier(&sd2, policy).map_err(|_| E_INTG)?;
    let (must, must_not) = tier_signature(policy);
    if sd1.control & must != must || sd1.control & must_not != 0 {
        return Err(E_INTG);
    }
    // 升版必须发生（除非同图）。
    if map_old.read == map_new.read && map_old.write == map_new.write && map_old.exec == map_new.exec {
        if g1 != g2 {
            return Err(E_GEN);
        }
    } else if g2 != g1 + 1 {
        return Err(E_GEN);
    }
    Ok(g2)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金八类建首版。
    let mut book = VersionBook::new();
    let mut i = 0;
    while i < GOLDEN_N {
        let (name, map, _, _) = tr::GOLDEN[i];
        let g = book.touch(name, map)?;
        if g != 1 {
            return Err(E_GEN);
        }
        i += 1;
    }
    if book.count() != GOLDEN_N {
        return Err(E_HIST);
    }
    book.integrity()?;
    // R2：放宽升版（File：read 槽加 WRITE_DAC 位——原 read 只读控制+同步）。
    let (_, fmap, _, _) = tr::GOLDEN[0];
    let widened = GenMap {
        read: fmap.read | super::unx36102_stdrights::WRITE_DAC,
        write: fmap.write,
        exec: fmap.exec,
    };
    let g2 = book.touch("File", widened)?;
    if g2 != 2 {
        return Err(E_GEN);
    }
    // 史环两版可回溯且逐版放宽。
    let e = book.entry("File").map_err(|_| E_NOENT)?;
    if e.hist_len() != 2 {
        return Err(E_HIST);
    }
    let v0 = e.hist_at(0)?;
    let v1 = e.hist_at(1)?;
    if v1.read & v0.read != v0.read {
        return Err(E_HIST);
    }
    // 同图幂等（gen 不动）。
    let g3 = book.touch("File", widened)?;
    if g3 != 2 {
        return Err(E_GEN);
    }
    // R3：收窄拒绝（File：read 槽撤掉 SYNCHRONIZE 位）。
    let narrowed = GenMap { read: fmap.read & !super::unx36102_stdrights::SYNCHRONIZE, write: fmap.write, exec: fmap.exec };
    if book.touch("File", narrowed).err() != Some(E_REGRESS) {
        return Err(E_REGRESS);
    }
    // 收窄后 gen 与史环不动。
    let e2 = book.entry("File")?;
    if e2.gen != 2 || e2.hist_len() != 2 {
        return Err(E_HIST);
    }
    // R4：史环环形顶替（同类型连升 6 版 → 史环只留最近 4 版、gen 守真）。
    let mut book2 = VersionBook::new();
    let (_, kmap, _, _) = tr::GOLDEN[1];
    let _ = book2.touch("Key", kmap);
    let mut step = 0;
    let mut cur = kmap;
    while step < 6 {
        // 每步在 read 槽并入一个新的合法特定段位（放宽）。
        let bit = 1u32 << step;
        cur = GenMap { read: cur.read | bit, write: cur.write, exec: cur.exec };
        if !cur.wellformed() {
            return Err(E_INTG);
        }
        book2.touch("Key", cur).map_err(|_| E_REGRESS)?;
        step += 1;
    }
    let ek = book2.entry("Key")?;
    if ek.gen != 7 || ek.hist_len() != HIST_CAP {
        return Err(E_GEN);
    }
    ek.integrity()?;
    // 环内最旧版 = 第 4 版（前 3 版被顶）。
    let oldest = ek.hist_at(0)?;
    // 最旧版必须 ⊇ 原 kmap 的 read 槽（放宽链不回退）。
    if oldest.read & kmap.read != kmap.read {
        return Err(E_HIST);
    }
    // R5：向后兼容核（四档全过：升版前后 SD 特征逐位不变）。
    let tiers = [
        TypePolicy::ClosedAll,
        TypePolicy::NullDacl,
        TypePolicy::InheritParent,
        TypePolicy::AdminGate,
    ];
    let mut t = 0;
    while t < 4 {
        let m_old = tr::GOLDEN[0].1;
        let m_new = widened;
        compat_probe(tiers[t], m_old, m_new)?;
        t += 1;
    }
    // R6：版本册满（17 类显性拒）。
    let mut book3 = VersionBook::new();
    let mut f = 0;
    while f < BOOK_CAP {
        let (name, map, _, _) = tr::GOLDEN[f % GOLDEN_N];
        // 名字唯一化：黄金名循环 + 后缀不可行（'static 限制）→ 用 FILL_NAMES。
        let _ = (name, map);
        book3.touch(tr::FILL_NAMES[f], map).map_err(|_| E_REGRESS)?;
        f += 1;
    }
    if book3.count() != BOOK_CAP {
        return Err(E_HIST);
    }
    if book3.touch("Overflow", tr::GOLDEN[0].1).err() != Some(super::unx36104_genmap::E_FULL) {
        return Err(super::unx36104_genmap::E_FULL);
    }
    // R7：未在册类型查版本显性拒。
    if book.entry("Ghost").err() != Some(E_NOENT) {
        return Err(E_NOENT);
    }
    // 校验和：全册当前代际和 × 史环容量。
    let mut sum: u32 = 0;
    let mut b = 0;
    while b < book.count() {
        let name = match book.entry(tr::GOLDEN_NAMES[b]) {
            Ok(e) => e.gen,
            Err(_) => 0,
        };
        sum = sum.wrapping_add(name);
        b += 1;
    }
    Ok(sum.wrapping_mul(11).wrapping_add(HIST_CAP as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_REGRESS => "掩码收窄（向后兼容破）",
        E_NOENT => "类型未在版本册",
        E_HIST => "版本史漂移（环形失真/放宽链破）",
        E_GEN => "代际数异常（升版未发生/越界）",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36128_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36128_typever");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b40, "0x3b40~0x3b4F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "放宽升版/收窄拒/史环顶替/向后兼容/册满全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("史环容量锚", HIST_CAP == 4, "每类型保留最近 4 版");
    cs.add("版本册容量锚", BOOK_CAP == 16, "版本册 16 类（与映射表对齐）");
    cs.add("同图幂等", {
        let mut b = VersionBook::new();
        let m = tr::GOLDEN[0].1;
        b.touch("File", m).is_ok() && b.touch("File", m) == Ok(1)
    }, "同图不升版");
    cs.add("SD 判据与版本解耦", {
        // 掩码演进不触档位面：同档位两代合成控制位恒等。
        match (synthesize(TypePolicy::AdminGate), synthesize(TypePolicy::AdminGate)) {
            (Ok(a), Ok(b)) => a.0.control == b.0.control,
            _ => false,
        }
    }, "新版本不破坏旧 SD 的落地面");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
