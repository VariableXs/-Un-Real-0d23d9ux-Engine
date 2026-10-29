//! UNX-F11196 域闭账断言三（40 批全 [已深化] 满态单值）（0x1f610 段）
//!
//! 锚点：UNX-F11196-J1 满态断言单值成立（10/10），断言账版本冻结（10/10）。
//!
//! 纯功能模型：四十批各走四态机——待开（Pending）→ 在册（Registered）→
//! 已深化（Deepened）→ 已封（Sealed）。状态只可逐级前进：未在册先深化必拒、
//! 深化层数低于下限必拒、判据数为零必拒、未深化先封必拒。四十批全数抵达
//! 「已深化」及以上，满态率 100%，方可汇出满态单值；单值冻结后状态机停摆。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f610;
/// 批号越界（1..=40）
pub const DS_EBATCH: u32 = SEG_BASE + 0x01;
/// 状态迁移非法（跳级或回退）
pub const DS_ESTATE: u32 = SEG_BASE + 0x02;
/// 深化层数低于下限
pub const DS_ELAYERS: u32 = SEG_BASE + 0x03;
/// 判据数为零
pub const DS_ENOCRI: u32 = SEG_BASE + 0x04;
/// 冻结后改写
pub const DS_EFROZEN: u32 = SEG_BASE + 0x05;
/// 未满态先求单值
pub const DS_ESHORT: u32 = SEG_BASE + 0x06;
/// 重复求单值
pub const DS_EDUP: u32 = SEG_BASE + 0x07;
/// 重复在册
pub const DS_EREG: u32 = SEG_BASE + 0x08;

/// 批数（B01–B40）。
pub const BATCHES: usize = 40;
/// 深化层数下限（三层以下不算深化）。
pub const MIN_LAYERS: u8 = 3;
/// 满态批数（四十批全深化）。
pub const FULL_DEEP: usize = 40;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 批级四态。数值序即前进方向，回退与跳级同属非法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Pending,
    Registered,
    Deepened,
    Sealed,
}

impl Stage {
    fn rank(self) -> u8 {
        match self {
            Stage::Pending => 0,
            Stage::Registered => 1,
            Stage::Deepened => 2,
            Stage::Sealed => 3,
        }
    }

    /// 是否已达「已深化」及以上（满态口径）。
    pub fn deep(self) -> bool {
        self.rank() >= Stage::Deepened.rank()
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Pending => "待开",
            Stage::Registered => "在册",
            Stage::Deepened => "已深化",
            Stage::Sealed => "已封",
        }
    }
}

/// 单批深化账：状态 + 判据数 + 层数。
#[derive(Clone, Debug, PartialEq)]
pub struct BatchDepth {
    pub no: u16,
    pub stage: Stage,
    pub criteria: u32,
    pub layers: u8,
}

impl BatchDepth {
    fn new(no: u16) -> BatchDepth {
        BatchDepth { no, stage: Stage::Pending, criteria: 0, layers: 0 }
    }

    /// 在册：仅待开态可入，判据数须非零。
    pub fn register(&mut self, criteria: u32) -> Result<(), u32> {
        if self.stage != Stage::Pending {
            return Err(if self.stage == Stage::Registered { DS_EREG } else { DS_ESTATE });
        }
        if criteria == 0 {
            return Err(DS_ENOCRI);
        }
        self.criteria = criteria;
        self.stage = Stage::Registered;
        Ok(())
    }

    /// 深化：仅在册态可入，层数须达下限。
    pub fn deepen(&mut self, layers: u8) -> Result<(), u32> {
        if self.stage != Stage::Registered {
            return Err(DS_ESTATE);
        }
        if layers < MIN_LAYERS {
            return Err(DS_ELAYERS);
        }
        self.layers = layers;
        self.stage = Stage::Deepened;
        Ok(())
    }

    /// 封存：仅已深化的批可封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.stage != Stage::Deepened {
            return Err(DS_ESTATE);
        }
        self.stage = Stage::Sealed;
        Ok(())
    }

    /// 满态判：已深化及以上且层数达下限。
    pub fn deep(&self) -> bool {
        self.stage.deep() && self.layers >= MIN_LAYERS
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.no as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.stage.rank() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.criteria as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.layers as u64).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 满态断言书：四十批深化账 + 单值 + 冻结位。
#[derive(Clone, Debug)]
pub struct DeepBook {
    batches: Vec<BatchDepth>,
    single: Option<u64>,
    frozen: bool,
    pub advances: u64,
    pub rejects: u64,
}

impl DeepBook {
    /// 空书（零批）。
    pub fn empty() -> DeepBook {
        DeepBook { batches: Vec::new(), single: None, frozen: false, advances: 0, rejects: 0 }
    }

    /// 满册开账：四十批全部停在待开态。
    pub fn frame() -> DeepBook {
        let mut batches = Vec::with_capacity(BATCHES);
        let mut no = 1u16;
        while (no as usize) <= BATCHES {
            batches.push(BatchDepth::new(no));
            no = no.wrapping_add(1);
        }
        DeepBook { batches, single: None, frozen: false, advances: 0, rejects: 0 }
    }

    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 批引用（批号 1 起）。
    pub fn batch(&self, no: u16) -> Result<&BatchDepth, u32> {
        if no == 0 {
            return Err(DS_EBATCH);
        }
        match self.batches.get((no as usize).wrapping_sub(1)) {
            Some(b) => Ok(b),
            None => Err(DS_EBATCH),
        }
    }

    /// 批状态（只探测不小改）。
    pub fn stage_of(&self, no: u16) -> Result<Stage, u32> {
        match self.batch(no) {
            Ok(b) => Ok(b.stage),
            Err(e) => Err(e),
        }
    }

    fn batch_mut(&mut self, no: u16) -> Result<&mut BatchDepth, u32> {
        if self.frozen {
            return Err(DS_EFROZEN);
        }
        if no == 0 {
            return Err(DS_EBATCH);
        }
        match self.batches.get_mut((no as usize).wrapping_sub(1)) {
            Some(b) => Ok(b),
            None => Err(DS_EBATCH),
        }
    }

    /// 在册（判据数入账）。
    pub fn register(&mut self, no: u16, criteria: u32) -> Result<(), u32> {
        let r = match self.batch_mut(no) {
            Ok(b) => b.register(criteria),
            Err(e) => Err(e),
        };
        self.note(r)
    }

    /// 深化（层数入账）。
    pub fn deepen(&mut self, no: u16, layers: u8) -> Result<(), u32> {
        let r = match self.batch_mut(no) {
            Ok(b) => b.deepen(layers),
            Err(e) => Err(e),
        };
        self.note(r)
    }

    /// 封存。
    pub fn seal(&mut self, no: u16) -> Result<(), u32> {
        let r = match self.batch_mut(no) {
            Ok(b) => b.seal(),
            Err(e) => Err(e),
        };
        self.note(r)
    }

    fn note(&mut self, r: Result<(), u32>) -> Result<(), u32> {
        match r {
            Ok(()) => {
                self.advances = self.advances.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    /// 已深化批数（含已封）。
    pub fn deep_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                if b.deep() {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 满态率（千分比，四十批全深化为 1000）。
    pub fn deep_rate_permille(&self) -> u32 {
        if self.batches.is_empty() {
            return 0;
        }
        let n = self.deep_count();
        let d = self.batches.len();
        let permille = n.saturating_mul(1000) / d;
        permille as u32
    }

    /// 未达满态的批号（诊断用）。
    pub fn shallow_batches(&self) -> Vec<u16> {
        let mut v: Vec<u16> = Vec::new();
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                if !b.deep() {
                    v.push(b.no);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 求满态单值：四十批全达已深化及以上、尚未出值。
    pub fn contend_single(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(DS_EFROZEN);
        }
        if self.single.is_some() {
            return Err(DS_EDUP);
        }
        if self.batches.len() != BATCHES || self.deep_count() != FULL_DEEP {
            return Err(DS_ESHORT);
        }
        let v = self.fold();
        self.single = Some(v);
        Ok(v)
    }

    pub fn single_value(&self) -> Option<u64> {
        self.single
    }

    /// 账面指纹：逐批链式 fold 再混入单值。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                h = (h ^ b.fold()).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let s = match self.single {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ s).wrapping_mul(FNV_PRIME);
        h
    }

    /// 版本冻结：幂等取回锁定值。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.single;
        }
        if self.single.is_none() {
            self.single = Some(self.fold());
        }
        self.frozen = true;
        self.single
    }
}

/// 标准推进：四十批在册（二十条判据）→ 深化（三到五层）→ 封存。
pub fn advance_all(b: &mut DeepBook) -> usize {
    let mut okay = 0usize;
    let mut no = 1u16;
    while (no as usize) <= BATCHES {
        let layers = MIN_LAYERS.wrapping_add((no % 3) as u8);
        if b.register(no, 20).is_ok() && b.deepen(no, layers).is_ok() && b.seal(no).is_ok() {
            okay = okay.wrapping_add(1);
        }
        no = no.wrapping_add(1);
    }
    okay
}

/// 满态装配：开账 → 全批推进 → 求单值。
pub fn build_full() -> (DeepBook, Option<u64>) {
    let mut b = DeepBook::frame();
    let advanced = advance_all(&mut b);
    let single = if advanced == BATCHES { b.contend_single().ok() } else { None };
    (b, single)
}

/// 四路注入：跳级深化 / 层数不足 / 判据为零 / 冻结后迁移，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 待开态直接深化（跳级）
    let mut a = DeepBook::frame();
    got[0] = a.deepen(1, MIN_LAYERS).err() == Some(DS_ESTATE);

    // ② 层数低于下限
    let mut b = DeepBook::frame();
    let _ = b.register(2, 20);
    got[1] = b.deepen(2, MIN_LAYERS - 1).err() == Some(DS_ELAYERS)
        && b.stage_of(2).ok() == Some(Stage::Registered);

    // ③ 判据数为零
    let mut c = DeepBook::frame();
    got[2] = c.register(3, 0).err() == Some(DS_ENOCRI);

    // ④ 冻结后迁移 + 冻结幂等
    let (mut d, single) = build_full();
    let root = d.freeze();
    got[3] = single == root
        && d.freeze() == root
        && d.register(1, 1).err() == Some(DS_EFROZEN)
        && d.deepen(1, 9).err() == Some(DS_EFROZEN)
        && d.seal(1).err() == Some(DS_EFROZEN)
        && d.contend_single().err() == Some(DS_EFROZEN);
    got
}

/// 判据台：满态断言单值面的十四条判据。
pub fn run_unx11196_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11196");

    let (mut book, single) = build_full();
    let root = book.freeze();

    // J1 四十批全达已深化
    s.add(
        "J1-四十批满态",
        book.deep_count() == FULL_DEEP && book.shallow_batches().is_empty(),
        "40/40 批抵达已深化及以上",
    );

    // J2 满态率单值
    s.add(
        "J2-满态率千分",
        book.deep_rate_permille() == 1000,
        "满态率 1000‰（即 100%）单值成立",
    );

    // J3 满态单值成立
    s.add(
        "J3-满态单值",
        single.is_some() && single == root,
        "满态汇出的单值即冻结值",
    );

    // J4 四态机逐批推进
    let mut st = DeepBook::frame();
    let _ = st.register(1, 20);
    let after_reg = st.stage_of(1).ok();
    let _ = st.deepen(1, MIN_LAYERS);
    let after_deep = st.stage_of(1).ok();
    let _ = st.seal(1);
    let after_seal = st.stage_of(1).ok();
    s.add(
        "J4-四态逐级",
        after_reg == Some(Stage::Registered)
            && after_deep == Some(Stage::Deepened)
            && after_seal == Some(Stage::Sealed),
        "在册→已深化→已封逐级前进",
    );

    // J5 跳级拒：待开态深化与在册态封存
    let mut jump = DeepBook::frame();
    let _ = jump.register(2, 20);
    s.add(
        "J5-跳级拒",
        jump.deepen(1, MIN_LAYERS).err() == Some(DS_ESTATE)
            && jump.seal(2).err() == Some(DS_ESTATE),
        "未在册先深化、未深化先封存两路皆拒",
    );

    // J6 层数不足拒且不回退状态
    let mut low = DeepBook::frame();
    let _ = low.register(3, 20);
    s.add(
        "J6-层数不足",
        low.deepen(3, 2).err() == Some(DS_ELAYERS)
            && low.stage_of(3).ok() == Some(Stage::Registered),
        "两层不算深化、状态停在在册",
    );

    // J7 判据为零拒
    let mut zero = DeepBook::frame();
    s.add(
        "J7-判据为零拒",
        zero.register(4, 0).err() == Some(DS_ENOCRI)
            && zero.stage_of(4).ok() == Some(Stage::Pending),
        "零判据批不得在册",
    );

    // J8 重复在册拒
    let mut reg = DeepBook::frame();
    let _ = reg.register(5, 20);
    s.add(
        "J8-重复在册拒",
        reg.register(5, 20).err() == Some(DS_EREG),
        "同批二次在册必拒",
    );

    // J9 未满态拒求值
    let mut half = DeepBook::frame();
    let mut no = 1u16;
    while no <= 20 {
        let _ = half.register(no, 20);
        let _ = half.deepen(no, MIN_LAYERS);
        no = no.wrapping_add(1);
    }
    s.add(
        "J9-未满拒求值",
        half.contend_single().err() == Some(DS_ESHORT)
            && half.deep_count() == 20
            && half.single_value().is_none(),
        "二十批深化不得出满态单值",
    );

    // J10 缺口可枚举
    s.add(
        "J10-缺口枚举",
        half.shallow_batches().len() == BATCHES - 20,
        "未深化批号逐一批出",
    );

    // J11 冻结幂等
    s.add(
        "J11-冻结幂等",
        book.freeze() == root && book.is_frozen(),
        "重复冻结取回同一锁定值",
    );

    // J12 冻结拒改
    s.add(
        "J12-冻结拒改",
        book.register(1, 1).err() == Some(DS_EFROZEN)
            && book.deepen(1, 9).err() == Some(DS_EFROZEN)
            && book.seal(1).err() == Some(DS_EFROZEN),
        "冻结后状态机停摆",
    );

    // J13 越界与空书
    let mut e = DeepBook::empty();
    s.add(
        "J13-越界与空书",
        e.batch(0).err() == Some(DS_EBATCH)
            && e.register(41, 1).err() == Some(DS_EBATCH)
            && e.contend_single().err() == Some(DS_ESHORT),
        "批号 0/41 越界、空书不满态",
    );

    // J14 重复求值拒
    let mut dup = DeepBook::frame();
    let _ = advance_all(&mut dup);
    s.add(
        "J14-重复求值拒",
        dup.contend_single().is_ok() && dup.contend_single().err() == Some(DS_EDUP),
        "满态单值只出一次",
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
    s.add("J15-注入四拒", all, "跳级/层数/零判据/冻结四路全拒");

    // J16 动态明细
    s.add_dyn(
        "J16-满态明细",
        book.advances == (BATCHES as u64) * 3,
        &format!(
            "deep={} advances={} rejects={} root={:#x}",
            book.deep_count(),
            book.advances,
            book.rejects,
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11196_deepstate_flow() {
        let (mut b, single) = build_full();
        assert_eq!(b.deep_count(), FULL_DEEP);
        assert_eq!(b.deep_rate_permille(), 1000);
        assert_eq!(b.stage_of(1).ok(), Some(Stage::Sealed));
        assert!(single.is_some());

        let root = b.freeze();
        assert_eq!(root, single);
        assert_eq!(b.freeze(), root);
        assert_eq!(b.deepen(1, 9).err(), Some(DS_EFROZEN));

        let mut st = DeepBook::frame();
        assert_eq!(st.deepen(1, 3).err(), Some(DS_ESTATE));
        assert_eq!(st.register(1, 0).err(), Some(DS_ENOCRI));
        assert!(st.register(1, 20).is_ok());
        assert_eq!(st.register(1, 20).err(), Some(DS_EREG));
        assert_eq!(st.deepen(1, 2).err(), Some(DS_ELAYERS));
        assert!(st.deepen(1, 3).is_ok());
        assert!(st.seal(1).is_ok());
        assert_eq!(st.seal(1).err(), Some(DS_ESTATE));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11196_checks_green() {
        let s = run_unx11196_checks();
        assert!(s.all_passed(), "UNX-F11196 判据未全绿");
    }
}
