//! UNX-F50094 字体冻结接口（管线消费单点面）（0x437e0 段）
//!
//! 锚点：UNX-F50094-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 冻结接口解决的是「同一份字体在渲染前被多方各改一版」的问题：登记时
//! 把内容摘要一次性算死，冻结后任何消费者拿到的都是同一个不可变摘要；
//! 冻结是幂等的（重复冻结返回同值），但一旦被消费就不再允许重新冻结，
//! 防止「先给 A 用了 v1、又原地改成 v2 给 B」的隐性串版。
//!
//! 状态机：Draft → Frozen → Consumed → Locked。
//! * Draft    仅登记，不可被管线读取；
//! * Frozen   摘要冻结，可被消费；
//! * Consumed 至少一个消费者已取用，串行号递增；
//! * Locked   交付期终态，拒绝一切写入与消费。
//!
//! 结构清单：
//! * 内容对象 [`FontContent`] + 摘要 [`content_digest`]；
//! * 状态机 [`FreezeState`] 与槽位 [`FreezeSlot`]；
//! * 注册表 [`FreezeRegistry`]（定容、键唯一、审计 [`FreezeRegistry::verify`]）；
//! * 消费面 [`FreezeRegistry::consume`]（消费者唯一、串行号单调）；
//! * 注入核 [`injection_core`]：8 类坏样本逐类断言具体诊断码。
//!
//! 诊断码闭集：`SEG_BASE + n`，n ∈ [1,11]。

use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x437e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x437e0;

pub const E_EMPTY: u16 = 1;
pub const E_BADMETRICS: u16 = 2;
pub const E_DUP: u16 = 3;
pub const E_CAP: u16 = 4;
pub const E_NOTFOUND: u16 = 5;
pub const E_NOTFROZEN: u16 = 6;
pub const E_CONSUMED: u16 = 7;
pub const E_LOCKED: u16 = 8;
pub const E_DIGEST: u16 = 9;
pub const E_CONSUMERDUP: u16 = 10;
pub const E_STATE: u16 = 11;

const FNV_PRIME: u64 = 0x1000_0000_01b3;
const FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// 冻结面错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    Empty,
    BadMetrics,
    Duplicate,
    Cap,
    NotFound,
    NotFrozen,
    Consumed,
    Locked,
    DigestMismatch,
    ConsumerDup,
    State,
}

impl FreezeError {
    /// 段内序号（闭集 1..=11）。
    pub fn ordinal(self) -> u16 {
        match self {
            FreezeError::Empty => E_EMPTY,
            FreezeError::BadMetrics => E_BADMETRICS,
            FreezeError::Duplicate => E_DUP,
            FreezeError::Cap => E_CAP,
            FreezeError::NotFound => E_NOTFOUND,
            FreezeError::NotFrozen => E_NOTFROZEN,
            FreezeError::Consumed => E_CONSUMED,
            FreezeError::Locked => E_LOCKED,
            FreezeError::DigestMismatch => E_DIGEST,
            FreezeError::ConsumerDup => E_CONSUMERDUP,
            FreezeError::State => E_STATE,
        }
    }

    /// 全仓诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            FreezeError::Empty => "font name empty",
            FreezeError::BadMetrics => "font metrics outside legal range",
            FreezeError::Duplicate => "key already registered",
            FreezeError::Cap => "registry capacity exhausted",
            FreezeError::NotFound => "key not registered",
            FreezeError::NotFrozen => "consume before freeze",
            FreezeError::Consumed => "refreeze after consumption",
            FreezeError::Locked => "slot locked",
            FreezeError::DigestMismatch => "content digest does not recompute",
            FreezeError::ConsumerDup => "consumer already holds a lease",
            FreezeError::State => "slot state inconsistent",
        }
    }
}

/// 待冻结内容。冻结后 `digest` 才是管线唯一可见的身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontContent {
    pub name: String,
    pub units_per_em: u16,
    pub glyphs: u16,
    pub tables: u16,
}

impl FontContent {
    /// 构造并校验；非法度量显式拒绝，不做 clamp。
    pub fn new(name: &str, units_per_em: u16, glyphs: u16, tables: u16) -> Result<FontContent, FreezeError> {
        let c = FontContent {
            name: name.to_string(),
            units_per_em,
            glyphs,
            tables,
        };
        c.validate()?;
        Ok(c)
    }

    /// 度量合法性：upem 16..=16384（OpenType 上限），字形/表数非零。
    pub fn validate(&self) -> Result<(), FreezeError> {
        if self.name.is_empty() {
            return Err(FreezeError::Empty);
        }
        if self.units_per_em < 16 || self.units_per_em > 16384 {
            return Err(FreezeError::BadMetrics);
        }
        if self.glyphs == 0 || self.tables == 0 {
            return Err(FreezeError::BadMetrics);
        }
        Ok(())
    }
}

fn step(h: u64, v: u64) -> u64 {
    let mut x = v;
    let mut out = h;
    let mut k = 0usize;
    while k < 8 {
        out = (out ^ (x & 0xFF)).wrapping_mul(FNV_PRIME);
        x >>= 8;
        k += 1;
    }
    out
}

/// 内容摘要：名字字节 + upem + 字形数 + 表数，顺序固定、逐位可复算。
pub fn content_digest(c: &FontContent) -> u64 {
    let mut h = step(FNV_SEED, c.units_per_em as u64);
    h = step(h, c.glyphs as u64);
    h = step(h, c.tables as u64);
    h = step(h, c.name.len() as u64);
    let mut i = 0usize;
    while let Some(b) = c.name.as_bytes().get(i) {
        h = step(h, *b as u64);
        i += 1;
    }
    h
}

/// 冻结档位。数值进审计快照，改动即视为接口变更。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    Draft,
    Frozen,
    Consumed,
    Locked,
}

impl FreezeState {
    /// 档位码（0..3）。
    pub fn code(self) -> u8 {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Frozen => 1,
            FreezeState::Consumed => 2,
            FreezeState::Locked => 3,
        }
    }

    /// 档位标签。
    pub fn label(self) -> &'static str {
        match self {
            FreezeState::Draft => "draft",
            FreezeState::Frozen => "frozen",
            FreezeState::Consumed => "consumed",
            FreezeState::Locked => "locked",
        }
    }
}

/// 冻结槽位：内容、摘要、状态与消费者租约。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreezeSlot {
    pub key: u16,
    pub content: FontContent,
    pub digest: u64,
    pub state: FreezeState,
    pub consumers: Vec<u16>,
    pub serial: u32,
}

/// 冻结注册表。单写者、键唯一、容量固定。
pub struct FreezeRegistry {
    pub slots: Vec<FreezeSlot>,
    cap: usize,
}

impl FreezeRegistry {
    /// 新建注册表；容量 0 时首次登记即 Cap。
    pub fn new(cap: usize) -> FreezeRegistry {
        FreezeRegistry {
            slots: Vec::with_capacity(cap),
            cap,
        }
    }

    /// 已登记槽位数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn index_of(&self, key: u16) -> Result<usize, FreezeError> {
        let mut i = 0usize;
        while let Some(s) = self.slots.get(i) {
            if s.key == key {
                return Ok(i);
            }
            i += 1;
        }
        Err(FreezeError::NotFound)
    }

    /// 登记内容并算出摘要；键重复/容量满/度量非法全部显式失败。
    pub fn register(&mut self, key: u16, content: FontContent) -> Result<u64, FreezeError> {
        content.validate()?;
        let mut i = 0usize;
        while let Some(s) = self.slots.get(i) {
            if s.key == key {
                return Err(FreezeError::Duplicate);
            }
            i += 1;
        }
        if self.slots.len() >= self.cap {
            return Err(FreezeError::Cap);
        }
        let digest = content_digest(&content);
        self.slots.push(FreezeSlot {
            key,
            content,
            digest,
            state: FreezeState::Draft,
            consumers: Vec::new(),
            serial: 0,
        });
        Ok(digest)
    }

    /// 冻结：Draft→Frozen；Frozen 重复调用幂等返回同摘要；
    /// Consumed/Locked 拒绝，避免交付后又改身份。
    pub fn freeze(&mut self, key: u16) -> Result<u64, FreezeError> {
        let idx = self.index_of(key)?;
        let slot = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(FreezeError::NotFound),
        };
        match slot.state {
            FreezeState::Draft => {
                slot.state = FreezeState::Frozen;
                Ok(slot.digest)
            }
            FreezeState::Frozen => Ok(slot.digest),
            FreezeState::Consumed => Err(FreezeError::Consumed),
            FreezeState::Locked => Err(FreezeError::Locked),
        }
    }

    /// 消费单点面：Frozen/Consumed 下登记消费者并发串行号；
    /// 同一消费者只能持有一份租约（ConsumerDup），未冻结不可读。
    pub fn consume(&mut self, key: u16, consumer: u16) -> Result<u32, FreezeError> {
        let idx = self.index_of(key)?;
        let slot = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(FreezeError::NotFound),
        };
        match slot.state {
            FreezeState::Draft => Err(FreezeError::NotFrozen),
            FreezeState::Locked => Err(FreezeError::Locked),
            FreezeState::Frozen | FreezeState::Consumed => {
                let mut dup = false;
                let mut i = 0usize;
                while let Some(c) = slot.consumers.get(i) {
                    if *c == consumer {
                        dup = true;
                    }
                    i += 1;
                }
                if dup {
                    return Err(FreezeError::ConsumerDup);
                }
                slot.consumers.push(consumer);
                slot.serial = match slot.serial.checked_add(1) {
                    Some(v) => v,
                    None => return Err(FreezeError::State),
                };
                slot.state = FreezeState::Consumed;
                Ok(slot.serial)
            }
        }
    }

    /// 交付期锁定。Draft 不可锁（还没冻结）；Frozen/Consumed→Locked；
    /// 重复锁定幂等。
    pub fn lock(&mut self, key: u16) -> Result<(), FreezeError> {
        let idx = self.index_of(key)?;
        let slot = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(FreezeError::NotFound),
        };
        match slot.state {
            FreezeState::Draft => Err(FreezeError::NotFrozen),
            FreezeState::Frozen | FreezeState::Consumed => {
                slot.state = FreezeState::Locked;
                Ok(())
            }
            FreezeState::Locked => Ok(()),
        }
    }

    /// 只读取槽位。
    pub fn get(&self, key: u16) -> Result<&FreezeSlot, FreezeError> {
        let idx = self.index_of(key)?;
        match self.slots.get(idx) {
            Some(s) => Ok(s),
            None => Err(FreezeError::NotFound),
        }
    }

    /// 审计快照：键、摘要、档位、串行号、消费者数全部入摘要。
    pub fn snapshot(&self) -> u64 {
        let mut h = step(FNV_SEED, self.slots.len() as u64);
        let mut i = 0usize;
        while let Some(s) = self.slots.get(i) {
            h = step(h, s.key as u64);
            h = step(h, s.digest);
            h = step(h, s.state.code() as u64);
            h = step(h, s.serial as u64);
            h = step(h, s.consumers.len() as u64);
            let mut j = 0usize;
            while let Some(c) = s.consumers.get(j) {
                h = step(h, *c as u64);
                j += 1;
            }
            i += 1;
        }
        h
    }

    /// 全表审计：摘要必须能从内容重算；串行号必须等于消费者数；
    /// 档位与计数必须自洽；消费者不得重复。
    pub fn verify(&self) -> Result<(), FreezeError> {
        let mut i = 0usize;
        while let Some(s) = self.slots.get(i) {
            if content_digest(&s.content) != s.digest {
                return Err(FreezeError::DigestMismatch);
            }
            if s.serial as usize != s.consumers.len() {
                return Err(FreezeError::State);
            }
            let consistent = match s.state {
                FreezeState::Draft => s.serial == 0,
                FreezeState::Frozen => s.serial == 0,
                FreezeState::Consumed => s.serial >= 1,
                FreezeState::Locked => true,
            };
            if !consistent {
                return Err(FreezeError::State);
            }
            let mut a = 0usize;
            while let Some(x) = s.consumers.get(a) {
                let mut b = a + 1;
                while let Some(y) = s.consumers.get(b) {
                    if x == y {
                        return Err(FreezeError::State);
                    }
                    b += 1;
                }
                a += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

fn sample_content() -> FontContent {
    FontContent {
        name: "Freeze Sans".to_string(),
        units_per_em: 2048,
        glyphs: 1234,
        tables: 12,
    }
}

/// 8 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    let empty = FontContent::new("", 2048, 10, 4);
    if empty.err() == Some(FreezeError::Empty) {
        caught += 1;
    }
    let bad = FontContent::new("bad", 0, 10, 4);
    if bad.err() == Some(FreezeError::BadMetrics) {
        caught += 1;
    }
    let mut dup = FreezeRegistry::new(4);
    if dup.register(1, sample_content()).is_ok()
        && dup.register(1, sample_content()).err() == Some(FreezeError::Duplicate)
    {
        caught += 1;
    }
    let mut cap = FreezeRegistry::new(1);
    if cap.register(1, sample_content()).is_ok()
        && cap.register(2, sample_content()).err() == Some(FreezeError::Cap)
    {
        caught += 1;
    }
    let mut notfrozen = FreezeRegistry::new(4);
    if notfrozen.register(1, sample_content()).is_ok()
        && notfrozen.consume(1, 7).err() == Some(FreezeError::NotFrozen)
    {
        caught += 1;
    }
    let mut consumed = FreezeRegistry::new(4);
    let mut refrozen = false;
    if consumed.register(1, sample_content()).is_ok()
        && consumed.freeze(1).is_ok()
        && consumed.consume(1, 7).is_ok()
    {
        refrozen = consumed.freeze(1).err() == Some(FreezeError::Consumed);
    }
    if refrozen {
        caught += 1;
    }
    let mut locked = FreezeRegistry::new(4);
    let mut locked_hit = false;
    if locked.register(1, sample_content()).is_ok()
        && locked.freeze(1).is_ok()
        && locked.lock(1).is_ok()
    {
        locked_hit = locked.consume(1, 7).err() == Some(FreezeError::Locked);
    }
    if locked_hit {
        caught += 1;
    }
    let mut tampered = FreezeRegistry::new(4);
    if tampered.register(1, sample_content()).is_ok() {
        if let Some(s) = tampered.slots.get_mut(0) {
            s.digest ^= 1;
        }
        if tampered.verify().err() == Some(FreezeError::DigestMismatch) {
            caught += 1;
        }
    }
    (caught, total)
}

/// 运行 UNX-F50094 的 15 条判据。
pub fn run_unx50094_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50094");
    let content = sample_content();
    let want = content_digest(&content);

    let mut reg = FreezeRegistry::new(8);
    let got = reg.register(1, content.clone());
    let ok = got == Ok(want) && reg.len() == 1;
    let det = format!("digest={:?}", got);
    s.add_dyn("50094-J1-登记定摘要", ok, &det);

    let first = reg.freeze(1);
    let second = reg.freeze(1);
    let ok = first.is_ok() && first == second && reg.get(1).map(|x| x.state) == Ok(FreezeState::Frozen);
    s.add("50094-J1-冻结幂等", ok, if ok { "" } else { "重复冻结漂移" });

    let mut reg2 = FreezeRegistry::new(8);
    let d2 = reg2.register(9, sample_content());
    let ok = d2 == Ok(want);
    s.add("50094-J1-摘要内容决定", ok, if ok { "" } else { "同内容异摘要" });

    let mut draft = FreezeRegistry::new(4);
    let ok = draft.register(2, sample_content()).is_ok()
        && draft.consume(2, 1).err() == Some(FreezeError::NotFrozen);
    s.add("50094-J1-未冻结不可读", ok, if ok { "" } else { "Draft 被消费" });

    let mut one = FreezeRegistry::new(4);
    let ok = one.register(3, sample_content()).is_ok()
        && one.freeze(3).is_ok()
        && one.consume(3, 5) == Ok(1)
        && one.get(3).map(|x| x.state) == Ok(FreezeState::Consumed);
    s.add("50094-J1-首消费串行1", ok, if ok { "" } else { "首消费计数错" });

    let ok = one.consume(3, 6) == Ok(2) && one.get(3).map(|x| x.serial) == Ok(2);
    s.add("50094-J1-串行号单调", ok, if ok { "" } else { "串行号不增" });

    let ok = one.consume(3, 5).err() == Some(FreezeError::ConsumerDup);
    s.add("50094-J1-消费者唯一", ok, if ok { "" } else { "重复租约放行" });

    let ok = one.freeze(3).err() == Some(FreezeError::Consumed);
    s.add("50094-J1-消费后禁冻", ok, if ok { "" } else { "消费后仍可重冻" });

    let mut locked = FreezeRegistry::new(4);
    let ok = locked.register(4, sample_content()).is_ok()
        && locked.freeze(4).is_ok()
        && locked.lock(4).is_ok()
        && locked.lock(4).is_ok()
        && locked.consume(4, 1).err() == Some(FreezeError::Locked)
        && locked.freeze(4).err() == Some(FreezeError::Locked);
    s.add("50094-J1-锁定拒读写", ok, if ok { "" } else { "锁定后可写" });

    let ok = reg.verify().is_ok() && reg.get(999).err() == Some(FreezeError::NotFound);
    s.add("50094-J1-审计与缺键", ok, if ok { "" } else { "审计失败" });

    let mut bad_digest = FreezeRegistry::new(4);
    let ok = bad_digest.register(5, sample_content()).is_ok();
    if let Some(slot) = bad_digest.slots.get_mut(0) {
        slot.digest ^= 1;
    }
    let ok = ok && bad_digest.verify().err() == Some(FreezeError::DigestMismatch);
    s.add("50094-J1-摘要篡改可检", ok, if ok { "" } else { "摘要篡改漏检" });

    let mut bad_state = FreezeRegistry::new(4);
    let ok = bad_state.register(6, sample_content()).is_ok();
    if let Some(slot) = bad_state.slots.get_mut(0) {
        slot.serial = 3;
    }
    let ok = ok && bad_state.verify().err() == Some(FreezeError::State);
    s.add("50094-J1-状态自洽可检", ok, if ok { "" } else { "状态漂移漏检" });

    let mut cap = FreezeRegistry::new(1);
    let ok = cap.register(7, sample_content()).is_ok()
        && cap.register(8, sample_content()).err() == Some(FreezeError::Cap);
    s.add("50094-J1-容量必拒", ok, if ok { "" } else { "容量未拒" });

    let mut dup = FreezeRegistry::new(4);
    let ok = dup.register(9, sample_content()).is_ok()
        && dup.register(9, sample_content()).err() == Some(FreezeError::Duplicate);
    s.add("50094-J1-重复键必拒", ok, if ok { "" } else { "重复键放行" });

    let ok = FontContent::new("", 2048, 1, 1).err() == Some(FreezeError::Empty)
        && FontContent::new("x", 0, 1, 1).err() == Some(FreezeError::BadMetrics)
        && FontContent::new("x", 2048, 0, 1).err() == Some(FreezeError::BadMetrics);
    s.add("50094-J1-度量校验", ok, if ok { "" } else { "非法度量入表" });

    let snap0 = reg.snapshot();
    let snap1 = reg.snapshot();
    let mut moved = FreezeRegistry::new(4);
    let ok = moved.register(10, sample_content()).is_ok() && moved.freeze(10).is_ok();
    let before = moved.snapshot();
    let consumed = moved.consume(10, 1).is_ok();
    let after = moved.snapshot();
    let ok = ok && consumed && snap0 == snap1 && before != after;
    s.add("50094-J1-快照确定性", ok, if ok { "" } else { "快照不稳定" });

    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50094-J1-注入必抓", ok, &det);

    let all = [
        FreezeError::Empty,
        FreezeError::BadMetrics,
        FreezeError::Duplicate,
        FreezeError::Cap,
        FreezeError::NotFound,
        FreezeError::NotFrozen,
        FreezeError::Consumed,
        FreezeError::Locked,
        FreezeError::DigestMismatch,
        FreezeError::ConsumerDup,
        FreezeError::State,
    ];
    let mut unique = true;
    let mut i = 0usize;
    while let Some(a) = all.get(i) {
        if a.code() < SEG_BASE + 1 || a.code() > SEG_BASE + 11 {
            unique = false;
        }
        let mut j = i + 1;
        while let Some(b) = all.get(j) {
            if a.code() == b.code() {
                unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = unique;
    s.add("50094-J1-码闭集", ok, if ok { "" } else { "码值越界或重复" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50094_checks_green() {
        let set = run_unx50094_checks();
        assert!(set.all_passed(), "UNX-F50094 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50094_digest_deterministic() {
        let a = content_digest(&sample_content());
        let b = content_digest(&sample_content());
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }
}
