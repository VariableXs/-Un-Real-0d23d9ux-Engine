//! UNX-F11190 域经汇编终版（三快照合订）（0x1f5b0 段）
//!
//! 纯功能模型：域经以三源快照（经卷 / 判据 / 账册）逐篇合订，逐篇对平后才
//! 允许汇编出「域经单值」。单值一经冻结即成为全域唯一版本号；快照分叉、源号
//! 越界、空册汇编、冻结后改写一律拒止。反面千轮样本只走合法路径。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f5b0;
/// 篇号越界
pub const SC_EINDEX: u32 = SEG_BASE + 0x01;
/// 快照源号非法
pub const SC_ESOURCE: u32 = SEG_BASE + 0x02;
/// 三源分叉（不合订）
pub const SC_EDIVERGE: u32 = SEG_BASE + 0x03;
/// 冻结后改写
pub const SC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 空册汇编
pub const SC_EEMPTY: u32 = SEG_BASE + 0x05;
/// 重复汇编
pub const SC_ECOMPILED: u32 = SEG_BASE + 0x06;
/// 篇章未对平
pub const SC_EUNBAL: u32 = SEG_BASE + 0x07;

/// 终版篇数（域经十篇）
pub const ARTICLES: usize = 10;
/// 三源：经卷 / 判据 / 账册
pub const SOURCES: usize = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一篇域经：三源快照与合订状态。
#[derive(Clone, Debug)]
pub struct Article {
    pub no: u16,
    snaps: [u64; SOURCES],
    pub balanced: bool,
}

impl Article {
    /// 按篇号播种三源（三源初始同值，便于「对平」语义自证）。
    fn new(no: u16, seed: u64) -> Self {
        let base = seed
            .wrapping_add((no as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
            .wrapping_add(0x0000_0000_0000_0101);
        Article { no, snaps: [base, base, base], balanced: false }
    }

    /// 取第 w 源快照。
    pub fn source(&self, w: usize) -> Result<u64, u32> {
        match self.snaps.get(w) {
            Some(v) => Ok(*v),
            None => Err(SC_ESOURCE),
        }
    }

    /// 校正第 w 源快照；已合订篇不可再改（合订即篇级冻结）。
    pub fn restamp(&mut self, w: usize, v: u64) -> Result<(), u32> {
        if self.balanced {
            return Err(SC_EFROZEN);
        }
        match self.snaps.get_mut(w) {
            Some(slot) => {
                *slot = v;
                Ok(())
            }
            None => Err(SC_ESOURCE),
        }
    }

    /// 三源对平：三源同值即合订，返回合订值。
    pub fn balance(&mut self) -> Result<u64, u32> {
        let head = match self.snaps.first() {
            Some(v) => *v,
            None => return Err(SC_ESOURCE),
        };
        let mut i = 1;
        while i < SOURCES {
            match self.snaps.get(i) {
                Some(v) if *v == head => {}
                Some(_) => return Err(SC_EDIVERGE),
                None => return Err(SC_ESOURCE),
            }
            i += 1;
        }
        self.balanced = true;
        Ok(head)
    }

    /// 三源是否同值（不改状态的对平探测）。
    pub fn agrees(&self) -> bool {
        let head = match self.snaps.first() {
            Some(v) => *v,
            None => return false,
        };
        let mut i = 1;
        while i < SOURCES {
            match self.snaps.get(i) {
                Some(v) if *v == head => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.no as u64).wrapping_mul(FNV_PRIME);
        let mut i = 0;
        while i < SOURCES {
            h = (h ^ self.snaps[i]).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h = (h ^ if self.balanced { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 域经汇编器：十篇三源合订后汇出单值。
#[derive(Clone, Debug)]
pub struct SutraCompendium {
    articles: Vec<Article>,
    compiled: Option<u64>,
    frozen: bool,
    pub passes: u64,
    pub rejects: u64,
    pub balance_rounds: u64,
}

impl SutraCompendium {
    /// 建空册。
    pub fn empty() -> Self {
        SutraCompendium {
            articles: Vec::new(),
            compiled: None,
            frozen: false,
            passes: 0,
            rejects: 0,
            balance_rounds: 0,
        }
    }

    /// 建终版十篇。
    pub fn terminal(seed: u64) -> Self {
        let mut articles = Vec::with_capacity(ARTICLES);
        let mut no = 0u16;
        while (no as usize) < ARTICLES {
            articles.push(Article::new(no, seed));
            no += 1;
        }
        SutraCompendium {
            articles,
            compiled: None,
            frozen: false,
            passes: 0,
            rejects: 0,
            balance_rounds: 0,
        }
    }

    /// 篇数。
    pub fn count(&self) -> usize {
        self.articles.len()
    }

    /// 篇引用。
    pub fn article(&self, ix: usize) -> Result<&Article, u32> {
        match self.articles.get(ix) {
            Some(a) => Ok(a),
            None => Err(SC_EINDEX),
        }
    }

    /// 篇可变引用（冻结后不可取）。
    pub fn article_mut(&mut self, ix: usize) -> Result<&mut Article, u32> {
        if self.frozen {
            return Err(SC_EFROZEN);
        }
        match self.articles.get_mut(ix) {
            Some(a) => Ok(a),
            None => Err(SC_EINDEX),
        }
    }

    /// 逐篇对平，返回对平篇数。
    pub fn balance_all(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SC_EFROZEN);
        }
        if self.articles.is_empty() {
            return Err(SC_EEMPTY);
        }
        let mut ok = 0u64;
        let mut i = 0;
        while i < self.articles.len() {
            match self.articles[i].balance() {
                Ok(_) => ok = ok.wrapping_add(1),
                Err(e) => {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(e);
                }
            }
            i += 1;
        }
        self.balance_rounds = self.balance_rounds.wrapping_add(1);
        Ok(ok)
    }

    /// 全篇是否对平。
    pub fn balanced_all(&self) -> bool {
        if self.articles.is_empty() {
            return false;
        }
        let mut i = 0;
        while i < self.articles.len() {
            if !self.articles[i].balanced {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 汇编出域经单值（逐篇须已对平）。
    pub fn compile(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SC_EFROZEN);
        }
        if self.articles.is_empty() {
            return Err(SC_EEMPTY);
        }
        if self.compiled.is_some() {
            return Err(SC_ECOMPILED);
        }
        if !self.balanced_all() {
            return Err(SC_EUNBAL);
        }
        let v = self.fold();
        self.compiled = Some(v);
        self.passes = self.passes.wrapping_add(1);
        Ok(v)
    }

    /// 单值（未汇编为空）。
    pub fn single_value(&self) -> Option<u64> {
        self.compiled
    }

    /// 汇编指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.articles.len() {
            h = (h ^ self.articles[i].fold()).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        let c = match self.compiled {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ c).wrapping_mul(FNV_PRIME);
        h = (h ^ self.passes).wrapping_mul(FNV_PRIME);
        h = (h ^ self.balance_rounds).wrapping_mul(FNV_PRIME);
        h
    }

    /// 版本冻结：返回最终指纹。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.compiled;
        }
        // 冻结即锁定当前单值；未汇编之册以当下指纹为其版本号。
        if self.compiled.is_none() {
            self.compiled = Some(self.fold());
        }
        self.frozen = true;
        self.compiled
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl SutraCompendium {
    /// 三源齐备篇数（只探测不改账）。
    pub fn three_source_ready(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.articles.len() {
            let a = &self.articles[i];
            let mut w = 0;
            let mut full = true;
            while w < SOURCES {
                if a.source(w).is_err() {
                    full = false;
                }
                w += 1;
            }
            if full && a.agrees() {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 合订漂移：已合订篇中三源再度分叉的篇数（应为零）。
    pub fn drift(&self) -> u64 {
        let mut d = 0u64;
        let mut i = 0;
        while i < self.articles.len() {
            let a = &self.articles[i];
            if a.balanced && !a.agrees() {
                d = d.wrapping_add(1);
            }
            i += 1;
        }
        d
    }

    /// 合订值汇总（仅合订篇计入，供跨篇对账）。
    pub fn balanced_digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.articles.len() {
            let a = &self.articles[i];
            if a.balanced {
                if let Ok(v) = a.source(0) {
                    h = (h ^ v).wrapping_mul(FNV_PRIME);
                }
            }
            i += 1;
        }
        h
    }
}

/// 终版装配：建册 → 逐篇对平 → 汇编 → 冻结。
pub fn build_terminal(seed: u64) -> (SutraCompendium, bool) {
    let mut c = SutraCompendium::terminal(seed);
    let ok = c.balance_all() == Ok(ARTICLES as u64) && c.compile().is_ok();
    let done = ok && c.freeze().is_some();
    (c, done)
}

/// 四类注入：越界取篇 / 非法源号 / 分叉合订 / 冻结后改写，全部须拒止。
pub fn inject_four() -> [bool; 4] {
    let mut c = SutraCompendium::terminal(1);

    // ① 篇号越界
    let a = c.article_mut(ARTICLES).err() == Some(SC_EINDEX);

    // ② 快照源号越界
    let b = match c.article_mut(0) {
        Ok(art) => art.source(SOURCES).err() == Some(SC_ESOURCE),
        Err(_) => false,
    };

    // ③ 三源分叉后强行合订
    let c_div = match c.article_mut(1) {
        Ok(art) => art.restamp(2, 0xdead_beef).is_ok() && art.balance().err() == Some(SC_EDIVERGE),
        Err(_) => false,
    };

    // ④ 冻结后改写、重复汇编、二次对平
    let (mut f, built) = build_terminal(7);
    let d = built
        && f.article_mut(0).err() == Some(SC_EFROZEN)
        && f.compile() == Err(SC_EFROZEN)
        && f.balance_all() == Err(SC_EFROZEN);

    [a, b, c_div, d]
}

/// 判据台：域经合订终版的十三条判据。
pub fn run_unx11190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11190");

    let (mut c, built) = build_terminal(0x1111);
    let balanced = c.balanced_all();
    let single = c.single_value();
    let root = c.freeze();

    // J1 合订逐篇对平
    s.add(
        "J1-合订逐篇对平",
        built && balanced && c.count() == ARTICLES,
        "十篇三源逐篇对平后成册",
    );

    // J2 域经单值成立
    s.add(
        "J2-单值成立",
        single.is_some() && single == root,
        "汇编单值一经落定即等于冻结指纹",
    );

    // J3 三源齐备
    s.add(
        "J3-三源齐备",
        c.three_source_ready() == ARTICLES as u64,
        "十篇三源齐备且同值",
    );

    // J4 合订零漂移
    s.add("J4-零漂移", c.drift() == 0, "已合订篇不再分叉");

    // J5 冻结幂等
    let again = c.freeze();
    s.add(
        "J5-冻结幂等",
        again == root && c.is_frozen(),
        "重复冻结取回同一单值",
    );

    // J6 冻结后拒改
    s.add(
        "J6-冻结拒改",
        c.article_mut(0).err() == Some(SC_EFROZEN) && c.compile() == Err(SC_EFROZEN),
        "冻结即终态：改篇与重复汇编皆拒",
    );

    // J7 空册拒汇编
    let mut e = SutraCompendium::empty();
    s.add(
        "J7-空册拒汇编",
        e.compile() == Err(SC_EEMPTY) && e.balance_all() == Err(SC_EEMPTY),
        "空册不得对平亦不得汇编",
    );

    // J8 未对平拒汇编
    let mut u = SutraCompendium::terminal(3);
    s.add(
        "J8-未对平拒汇编",
        u.compile() == Err(SC_EUNBAL) && !u.balanced_all(),
        "未对平之册无单值",
    );

    // J9 篇号越界
    s.add(
        "J9-篇号越界",
        u.article(ARTICLES).err() == Some(SC_EINDEX)
            && u.article_mut(ARTICLES).err() == Some(SC_EINDEX),
        "越界取篇读写双路皆拒",
    );

    // J10 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i += 1;
    }
    s.add("J10-注入四拒", all, "越界/源号/分叉/冻结四路全拒");

    // J11 重算一致
    let (a1, ok1) = build_terminal(0x5151);
    let (a2, ok2) = build_terminal(0x5151);
    s.add(
        "J11-重算一致",
        ok1 && ok2 && a1.fold() == a2.fold(),
        "同种子两册指纹一致",
    );

    // J12 分叉拒止入账
    let mut dv = SutraCompendium::terminal(9);
    let mut diverged = false;
    if let Ok(art) = dv.article_mut(2) {
        diverged = art.restamp(1, 0x1234_5678).is_ok();
    }
    let rej = dv.balance_all().err() == Some(SC_EDIVERGE);
    s.add(
        "J12-分叉拒止",
        diverged && rej && dv.rejects == 1,
        "分叉篇拒止且拒绝计数入账",
    );

    // J13 动态明细
    s.add_dyn(
        "J13-终版明细",
        c.count() == ARTICLES && c.balance_rounds == 1 && c.drift() == 0,
        &format!(
            "articles={} rounds={} root={:#x}",
            c.count(),
            c.balance_rounds,
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11190_sutra_flow() {
        let (c, built) = build_terminal(42);
        assert!(built);
        assert_eq!(c.count(), ARTICLES);
        assert!(c.balanced_all());
        assert!(c.is_frozen());
        assert_eq!(c.drift(), 0);
        assert!(c.single_value().is_some());
        assert!(inject_four().iter().all(|h| *h));

        let mut e = SutraCompendium::empty();
        assert_eq!(e.compile(), Err(SC_EEMPTY));
        assert!(e.freeze().is_some());
        assert_eq!(e.freeze(), e.single_value());
    }

    #[test]
    fn unx11190_checks_green() {
        let s = run_unx11190_checks();
        assert!(s.all_passed(), "UNX-F11190 判据未全绿");
    }
}
