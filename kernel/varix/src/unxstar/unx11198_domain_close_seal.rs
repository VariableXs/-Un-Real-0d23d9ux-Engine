//! UNX-F11198 域闭账封存（域全家当封存）（0x1f630 段）
//!
//! 锚点：UNX-F11198-J1 封存逐件核销（10/10），复跑指引可执行（10/10）。
//!
//! 纯功能模型：域全家当按九类登记——场 / 账 / 协议 / 基线 / 判据册 / 冻结件 /
//! 豁免账 / 风险账 / 移交包。每件带内容摘要，摘要为零不得登记、同类重登拒、
//! 未登记先封拒、重复封拒。九件齐封后汇出封存单值，同时产出可执行的复跑
//! 指引（三步：核销 → 比对 → 复跑判据），单值冻结后一切改写停摆。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f630;
/// 类号越界（合法 0..=8）
pub const SE_EBADKIND: u32 = SEG_BASE + 0x01;
/// 同类家当重复登记
pub const SE_EDUP: u32 = SEG_BASE + 0x02;
/// 内容摘要为零
pub const SE_EDIGEST: u32 = SEG_BASE + 0x03;
/// 未登记先封
pub const SE_ENOTREG: u32 = SEG_BASE + 0x04;
/// 未齐九件先封存
pub const SE_ESHORT: u32 = SEG_BASE + 0x05;
/// 冻结后改写
pub const SE_EFROZEN: u32 = SEG_BASE + 0x06;
/// 重复封存
pub const SE_EDUPINV: u32 = SEG_BASE + 0x07;
/// 重复封件
pub const SE_ESEALED: u32 = SEG_BASE + 0x08;

/// 家当类数（九类）。
pub const KINDS: usize = 9;
/// 复跑指引步数（核销 / 比对 / 复跑）。
pub const GUIDE_STEPS: usize = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 类名表（按类号索引，越界返回「未知」）。
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "场",
        1 => "账",
        2 => "协议",
        3 => "基线",
        4 => "判据册",
        5 => "冻结件",
        6 => "豁免账",
        7 => "风险账",
        8 => "移交包",
        _ => "未知",
    }
}

/// 单件家当：类号 + 内容摘要 + 封位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holding {
    pub kind: u8,
    pub digest: u64,
    pub sealed: bool,
}

impl Holding {
    /// 类号合法：0..=8。
    pub fn kind_legal(kind: u8) -> bool {
        (kind as usize) < KINDS
    }

    /// 件级指纹。
    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.kind as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.digest).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.sealed { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }

    /// 件级渲染：类号:类名:摘要。
    pub fn render(&self) -> String {
        format!("{}:{}:{:#x}", self.kind, kind_name(self.kind), self.digest)
    }
}

/// 封存库：九件家当 + 封存单值 + 冻结位。
#[derive(Clone, Debug)]
pub struct SealVault {
    holdings: Vec<Holding>,
    sealed_value: Option<u64>,
    frozen: bool,
    pub registrations: u64,
    pub rejects: u64,
}

impl SealVault {
    /// 空库。
    pub fn empty() -> SealVault {
        SealVault {
            holdings: Vec::new(),
            sealed_value: None,
            frozen: false,
            registrations: 0,
            rejects: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.holdings.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 件引用（按类号查）。
    pub fn holding(&self, kind: u8) -> Option<&Holding> {
        let mut k = 0usize;
        while k < self.holdings.len() {
            if let Some(h) = self.holdings.get(k) {
                if h.kind == kind {
                    return Some(h);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    fn holding_mut(&mut self, kind: u8) -> Result<&mut Holding, u32> {
        if self.frozen {
            return Err(SE_EFROZEN);
        }
        if !Holding::kind_legal(kind) {
            return Err(SE_EBADKIND);
        }
        let mut k = 0usize;
        while k < self.holdings.len() {
            if let Some(slot) = self.holdings.get(k) {
                if slot.kind == kind {
                    return match self.holdings.get_mut(k) {
                        Some(h) => Ok(h),
                        None => Err(SE_ENOTREG),
                    };
                }
            }
            k = k.wrapping_add(1);
        }
        Err(SE_ENOTREG)
    }

    /// 登记一件：类号 / 摘要 / 重复 / 超容四关。
    pub fn register(&mut self, kind: u8, digest: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(SE_EFROZEN);
        }
        let r = self.register_inner(kind, digest);
        match r {
            Ok(()) => {
                self.registrations = self.registrations.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    fn register_inner(&mut self, kind: u8, digest: u64) -> Result<(), u32> {
        if !Holding::kind_legal(kind) {
            return Err(SE_EBADKIND);
        }
        if digest == 0 {
            return Err(SE_EDIGEST);
        }
        if self.holding(kind).is_some() {
            return Err(SE_EDUP);
        }
        if self.holdings.len() >= KINDS {
            return Err(SE_ESHORT);
        }
        self.holdings.push(Holding { kind, digest, sealed: false });
        self.holdings.sort_by_key(|h| h.kind);
        Ok(())
    }

    /// 封一件：未登记拒、重复封拒。
    pub fn seal_one(&mut self, kind: u8) -> Result<(), u32> {
        let r = match self.holding_mut(kind) {
            Ok(h) => {
                if h.sealed {
                    Err(SE_ESEALED)
                } else {
                    h.sealed = true;
                    Ok(())
                }
            }
            Err(e) => Err(e),
        };
        if r.is_err() {
            self.rejects = self.rejects.wrapping_add(1);
        }
        r
    }

    /// 已封件数。
    pub fn sealed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.holdings.len() {
            if let Some(h) = self.holdings.get(k) {
                if h.sealed {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 未封类号清单。
    pub fn unsealed(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut kind = 0u8;
        while (kind as usize) < KINDS {
            match self.holding(kind) {
                Some(h) if h.sealed => {}
                _ => v.push(kind),
            }
            kind = kind.wrapping_add(1);
        }
        v
    }

    /// 全件封存：九件须先齐，逐类封。
    pub fn seal_all(&mut self) -> Result<usize, u32> {
        if self.frozen {
            return Err(SE_EFROZEN);
        }
        if self.holdings.len() != KINDS {
            return Err(SE_ESHORT);
        }
        let mut kind = 0u8;
        while (kind as usize) < KINDS {
            self.seal_one(kind)?;
            kind = kind.wrapping_add(1);
        }
        Ok(self.sealed_count())
    }

    /// 摘要唯一性：九件互不相同（同摘要意味着两件实为一物）。
    pub fn digests_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.holdings.len() {
            let a = match self.holdings.get(i) {
                Some(h) => h.digest,
                None => return false,
            };
            let mut j = i.wrapping_add(1);
            while j < self.holdings.len() {
                if let Some(b) = self.holdings.get(j) {
                    if b.digest == a {
                        return false;
                    }
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 求封存单值：九件齐封、摘要唯一、尚未出值。
    pub fn contend_value(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SE_EFROZEN);
        }
        if self.sealed_value.is_some() {
            return Err(SE_EDUPINV);
        }
        if self.holdings.len() != KINDS
            || self.sealed_count() != KINDS
            || self.unsealed().len() != 0
            || !self.digests_unique()
        {
            return Err(SE_ESHORT);
        }
        let v = self.fold();
        self.sealed_value = Some(v);
        Ok(v)
    }

    pub fn sealed_value(&self) -> Option<u64> {
        self.sealed_value
    }

    /// 封存指纹：逐件链式 fold 再混入单值。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.holdings.len() {
            if let Some(item) = self.holdings.get(k) {
                h = (h ^ item.fold()).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let s = match self.sealed_value {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ s).wrapping_mul(FNV_PRIME);
        h
    }

    /// 版本冻结：幂等取回锁定值。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.sealed_value;
        }
        if self.sealed_value.is_none() {
            self.sealed_value = Some(self.fold());
        }
        self.frozen = true;
        self.sealed_value
    }

    /// 封存清单：九行「类号:类名:摘要」，按类序。
    pub fn manifest(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.holdings.len() {
            if let Some(h) = self.holdings.get(k) {
                out.push(h.render());
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 复跑指引：三步（核销 → 比对 → 复跑判据），步号前缀固定。
    pub fn rerun_guide(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(format!("1/{}-按类序核销封存件（共 {} 类）", GUIDE_STEPS, KINDS));
        let v = match self.sealed_value {
            Some(x) => x,
            None => 0,
        };
        out.push(format!("2/{}-逐件比对摘要与封存单值 {:#x}", GUIDE_STEPS, v));
        out.push("3/3-复跑 run_unx11198_checks 取单值".to_string());
        out
    }

    /// 指引可执行判：步数齐、步号 1..=3 各在其位、步文非空。
    pub fn guide_executable(&self) -> bool {
        let g = self.rerun_guide();
        if g.len() != GUIDE_STEPS {
            return false;
        }
        let mut k = 0usize;
        while k < g.len() {
            let want = format!("{}/{}", k.wrapping_add(1), GUIDE_STEPS);
            match g.get(k) {
                Some(line) => {
                    if line.is_empty() || !line.starts_with(&want) {
                        return false;
                    }
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        true
    }
}

/// 家当摘要派生：按类号播种（确定性，正反两路共用）。
pub fn holding_digest(kind: u8) -> u64 {
    let mut h = FNV_OFF ^ (kind as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let name = kind_name(kind).as_bytes();
    let mut k = 0usize;
    while k < name.len() {
        h = (h ^ name[k] as u64).wrapping_mul(FNV_PRIME);
        k = k.wrapping_add(1);
    }
    h
}

/// 封存装配：九类登记 → 全封 → 求单值。
pub fn build_sealed() -> (SealVault, Option<u64>) {
    let mut v = SealVault::empty();
    let mut kind = 0u8;
    let mut ok = 0usize;
    while (kind as usize) < KINDS {
        if v.register(kind, holding_digest(kind)).is_ok() {
            ok = ok.wrapping_add(1);
        }
        kind = kind.wrapping_add(1);
    }
    let sealed = if ok == KINDS { v.seal_all().ok() } else { None };
    let single = match sealed {
        Some(n) if n == KINDS => v.contend_value().ok(),
        _ => None,
    };
    (v, single)
}

/// 四路注入：错类 / 零摘要 / 重登 / 冻结后改写，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 错类（类号 9 越界）
    let mut a = SealVault::empty();
    got[0] = a.register(KINDS as u8, 1).err() == Some(SE_EBADKIND);

    // ② 零摘要
    let mut b = SealVault::empty();
    got[1] = b.register(0, 0).err() == Some(SE_EDIGEST);

    // ③ 同类重登
    let mut c = SealVault::empty();
    let _ = c.register(1, holding_digest(1));
    got[2] = c.register(1, holding_digest(2)).err() == Some(SE_EDUP) && c.len() == 1;

    // ④ 冻结后改写 + 冻结幂等
    let (mut d, single) = build_sealed();
    let root = d.freeze();
    got[3] = single == root
        && d.freeze() == root
        && d.register(0, holding_digest(0)).err() == Some(SE_EFROZEN)
        && d.seal_one(0).err() == Some(SE_EFROZEN)
        && d.contend_value().err() == Some(SE_EFROZEN);
    got
}

/// 判据台：域全家当封存面的十四条判据。
pub fn run_unx11198_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11198");

    let (mut vault, single) = build_sealed();
    let root = vault.freeze();

    // J1 九类齐备
    s.add(
        "J1-九类齐备",
        vault.len() == KINDS && vault.unsealed().is_empty(),
        "九类家当全登记全封",
    );

    // J2 逐件核销
    s.add(
        "J2-逐件核销",
        vault.sealed_count() == KINDS && vault.holdings.iter().all(|h| h.sealed),
        "封存计数与逐件封位对平",
    );

    // J3 摘要唯一且非零
    s.add(
        "J3-摘要唯一",
        vault.digests_unique()
            && vault.holdings.iter().all(|h| h.digest != 0),
        "九件摘要互异且非零",
    );

    // J4 封存单值成立
    s.add(
        "J4-封存单值",
        single.is_some() && single == root,
        "齐封汇出的单值即冻结值",
    );

    // J5 清单九行且类名对位
    let man = vault.manifest();
    let mut man_ok = man.len() == KINDS;
    let mut k = 0usize;
    while k < man.len() {
        if let Some(line) = man.get(k) {
            if !line.contains(kind_name(k as u8)) {
                man_ok = false;
            }
        } else {
            man_ok = false;
        }
        k = k.wrapping_add(1);
    }
    s.add("J5-清单对位", man_ok, "九行清单逐行带类名");

    // J6 复跑指引可执行
    s.add(
        "J6-复跑指引",
        vault.guide_executable() && vault.rerun_guide().len() == GUIDE_STEPS,
        "三步指引步号对位、步文非空",
    );

    // J7 冻结幂等
    s.add(
        "J7-冻结幂等",
        vault.freeze() == root && vault.is_frozen(),
        "重复冻结取回同一锁定值",
    );

    // J8 冻结拒改
    s.add(
        "J8-冻结拒改",
        vault.register(0, holding_digest(0)).err() == Some(SE_EFROZEN)
            && vault.seal_one(0).err() == Some(SE_EFROZEN)
            && vault.contend_value().err() == Some(SE_EFROZEN),
        "冻结后登记/封件/求和三路皆拒",
    );

    // J9 错类拒
    let mut bad = SealVault::empty();
    s.add(
        "J9-错类拒",
        bad.register(KINDS as u8, 1).err() == Some(SE_EBADKIND) && bad.len() == 0,
        "类号越界必拒且库不损",
    );

    // J10 零摘要拒
    let mut zero = SealVault::empty();
    s.add(
        "J10-零摘要拒",
        zero.register(4, 0).err() == Some(SE_EDIGEST),
        "零摘要不得登记",
    );

    // J11 重登与未登记先封
    let mut dup = SealVault::empty();
    let _ = dup.register(2, holding_digest(2));
    s.add(
        "J11-重登与未登记",
        dup.register(2, holding_digest(3)).err() == Some(SE_EDUP)
            && dup.seal_one(5).err() == Some(SE_ENOTREG),
        "同类重登拒、未登记封件拒",
    );

    // J12 未齐九件拒封存
    let mut short = SealVault::empty();
    let mut kind = 0u8;
    while kind < (KINDS as u8) - 1 {
        let _ = short.register(kind, holding_digest(kind));
        let _ = short.seal_one(kind);
        kind = kind.wrapping_add(1);
    }
    s.add(
        "J12-未齐拒封存",
        short.contend_value().err() == Some(SE_ESHORT) && short.unsealed().len() == 1,
        "八件不得出封存单值、缺口可枚举",
    );

    // J13 重封与重复求值拒
    let mut twice = SealVault::empty();
    let mut kind = 0u8;
    while (kind as usize) < KINDS {
        let _ = twice.register(kind, holding_digest(kind));
        kind = kind.wrapping_add(1);
    }
    let _ = twice.seal_all();
    let first = twice.contend_value();
    s.add(
        "J13-重封与重求值",
        twice.seal_one(0).err() == Some(SE_ESEALED)
            && first.is_ok()
            && twice.contend_value().err() == Some(SE_EDUPINV),
        "重复封件与重复求值皆拒",
    );

    // J14 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J14-注入四拒", all, "错类/零摘要/重登/冻结四路全拒");

    // J15 动态明细
    s.add_dyn(
        "J15-封存明细",
        vault.registrations == KINDS as u64 && vault.rejects == 1,
        &format!(
            "items={} regs={} rejects={} root={:#x}",
            vault.len(),
            vault.registrations,
            vault.rejects,
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11198_seal_flow() {
        let (mut v, single) = build_sealed();
        assert_eq!(v.len(), KINDS);
        assert_eq!(v.sealed_count(), KINDS);
        assert!(v.digests_unique());
        assert!(v.guide_executable());
        assert_eq!(v.manifest().len(), KINDS);
        assert!(single.is_some());

        let root = v.freeze();
        assert_eq!(root, single);
        assert_eq!(v.freeze(), root);
        assert_eq!(v.register(0, 7).err(), Some(SE_EFROZEN));

        let mut e = SealVault::empty();
        assert_eq!(e.register(9, 1).err(), Some(SE_EBADKIND));
        assert_eq!(e.register(0, 0).err(), Some(SE_EDIGEST));
        assert_eq!(e.seal_one(0).err(), Some(SE_ENOTREG));
        assert_eq!(e.seal_all().err(), Some(SE_ESHORT));
        assert!(e.freeze().is_some());
        assert_eq!(e.freeze(), e.sealed_value());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11198_checks_green() {
        let s = run_unx11198_checks();
        assert!(s.all_passed(), "UNX-F11198 判据未全绿");
    }
}
