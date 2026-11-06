//! UNX-F50033 布局冻结接口（管线消费单点面）（0x43040 段）
//!
//! 锚点：UNX-F50033-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 数据段：`SEG_BASE = 0x43040`，诊断码闭集 `0x43040 + 1..=0x43040 + 12`。
//!
//! 结构清单：
//! - 冻结状态机：`FreezeState = Draft → Candidate → Frozen → Retired →(reopen) Draft`。
//!   只有 `freeze()` 能进 Frozen（必须带版本与指纹），只有 `retire()` 能进 Retired，
//!   其余跃迁逐条显式拒绝——「单点面」的前提是**没有第二条进 Frozen 的路**；
//! - 接口登记表：`Registry`（append-only 面表 + 容量 + 单调版本 + 封条）。声明只在
//!   Draft 期开放，封账后 `declare/revise` 一律 Sealed，防止「冻结后偷偷改参数」；
//! - 指纹：`fingerprint_of` 对 (id, revision, width, height, digest) FNV-1a 折叠。
//!   封条里存指纹与面数，`verify_seal` 复算；外部持久化封条用 `verify_with` 重验，
//!   接口演进（reopen 后 revise）会让旧封条显式失效 —— 这是版本审计的落点；
//! - 消费账：`ConsumerLog` 记录每个消费者 (consumer, face, version, digest)。
//!   `audit` 逐条回查「版本一致 + 摘要一致」，漂移返回 Drift；这是 J1 的机器口径：
//!   所有消费者看的必须是**同一版本的同一摘要**；
//! - 注入核：`injection_core` 五类坏操作逐类断言具体诊断码。
//!
//! 为什么 reopen 保留面表与版本号：接口退休后进入下一版本周期是常态，面表是
//! 「当前形态」，版本号是「历史水位」；清空面表等于丢失审计上下文，回退版本号
//! 等于允许封条复用，两者都是静默错误源，故一律不允许。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x43040 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x43040;

/// 接口面容量上限：单点面只服务布局管线，16 个面已覆盖分栏/页边/标记/出血等域。
pub const MAX_FACES: usize = 16;
/// 消费账容量上限：32 条足够覆盖一个编排周期的消费事件，超出显式 Full。
pub const MAX_CONSUMERS: usize = 32;
/// 版本号上限：1e6 留给自动编排的长周期，超出视为参数错误。
pub const MAX_VERSION: u32 = 1_000_000;
/// 面尺寸上限（pt/px 通用）：1e6 以上没有可打印意义。
pub const MAX_DIM: u32 = 1_000_000;

const E_EMPTY: u16 = 1;
const E_FULL: u16 = 2;
const E_DUP: u16 = 3;
const E_UNKNOWN: u16 = 4;
const E_STATE: u16 = 5;
const E_VERSION: u16 = 6;
const E_SEALED: u16 = 7;
const E_NOTSEALED: u16 = 8;
const E_FINGERPRINT: u16 = 9;
const E_DRIFT: u16 = 10;
const E_RETIRED: u16 = 11;
const E_RANGE: u16 = 12;

/// 冻结接口域错误十二型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    Empty,
    Full,
    Duplicate,
    Unknown,
    State,
    Version,
    Sealed,
    NotSealed,
    Fingerprint,
    Drift,
    Retired,
    Range,
}

impl FreezeError {
    pub fn code(self) -> u32 {
        let n = match self {
            FreezeError::Empty => E_EMPTY,
            FreezeError::Full => E_FULL,
            FreezeError::Duplicate => E_DUP,
            FreezeError::Unknown => E_UNKNOWN,
            FreezeError::State => E_STATE,
            FreezeError::Version => E_VERSION,
            FreezeError::Sealed => E_SEALED,
            FreezeError::NotSealed => E_NOTSEALED,
            FreezeError::Fingerprint => E_FINGERPRINT,
            FreezeError::Drift => E_DRIFT,
            FreezeError::Retired => E_RETIRED,
            FreezeError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FreezeError::Empty => "empty name or empty face table",
            FreezeError::Full => "registry or consumer log full",
            FreezeError::Duplicate => "duplicate face name",
            FreezeError::Unknown => "face id not registered",
            FreezeError::State => "state transition not allowed",
            FreezeError::Version => "seal version does not advance",
            FreezeError::Sealed => "face table already sealed",
            FreezeError::NotSealed => "interface not frozen yet",
            FreezeError::Fingerprint => "seal fingerprint mismatch",
            FreezeError::Drift => "consumer saw a different seal",
            FreezeError::Retired => "interface retired",
            FreezeError::Range => "argument outside accepted range",
        }
    }
}

/// 冻结状态机四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    Draft,
    Candidate,
    Frozen,
    Retired,
}

impl FreezeState {
    pub fn code(self) -> u8 {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Candidate => 1,
            FreezeState::Frozen => 2,
            FreezeState::Retired => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FreezeState::Draft => "draft",
            FreezeState::Candidate => "candidate",
            FreezeState::Frozen => "frozen",
            FreezeState::Retired => "retired",
        }
    }
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h ^= ((v >> (k * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    h
}

fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 单面摘要：名称、修订号、宽高共同决定；任一项变化摘要必变。
pub fn face_digest(name: &str, revision: u16, width: u32, height: u32) -> u64 {
    let mut h = fnv_bytes(FNV_OFFSET, name.as_bytes());
    h = fnv_u32(h, revision as u32);
    h = fnv_u32(h, width);
    h = fnv_u32(h, height);
    h
}

/// 一个已登记的接口面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub id: u16,
    pub name: String,
    pub revision: u16,
    pub width: u32,
    pub height: u32,
    pub digest: u64,
}

impl Face {
    pub fn new(id: u16, name: &str, width: u32, height: u32) -> Face {
        Face {
            id,
            name: name.to_string(),
            revision: 1,
            width,
            height,
            digest: face_digest(name, 1, width, height),
        }
    }
}

/// 封条：版本 + 面表指纹 + 面数。三个字段缺一不可，避免「指纹相同但换了面集」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seal {
    pub version: u32,
    pub fingerprint: u64,
    pub faces: u16,
}

/// 消费者拿到的只读视图（不含名称，避免消费侧持有可变文本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceView {
    pub id: u16,
    pub revision: u16,
    pub digest: u64,
    pub width: u32,
    pub height: u32,
}

/// 一条消费记录：谁在哪个版本看到了哪个面的哪个摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerRecord {
    pub consumer: u16,
    pub face: u16,
    pub version: u32,
    pub digest: u64,
}

/// 消费账（append-only，定容）。审计时逐条回查，不抽样。
pub struct ConsumerLog {
    records: Vec<ConsumerRecord>,
    capacity: usize,
}

impl ConsumerLog {
    pub fn new(capacity: usize) -> Result<ConsumerLog, FreezeError> {
        if capacity == 0 || capacity > MAX_CONSUMERS {
            return Err(FreezeError::Range);
        }
        Ok(ConsumerLog {
            records: Vec::with_capacity(capacity),
            capacity,
        })
    }

    pub fn record(&mut self, r: ConsumerRecord) -> Result<(), FreezeError> {
        if self.records.len() >= self.capacity {
            return Err(FreezeError::Full);
        }
        self.records.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn records(&self) -> &[ConsumerRecord] {
        &self.records
    }

    /// 逐条审计消费记录：版本号与摘要都必须等于当前封条口径。
    pub fn audit(&self, reg: &Registry) -> Result<usize, FreezeError> {
        let seal = match reg.seal() {
            Some(s) => s,
            None => return Err(FreezeError::NotSealed),
        };
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.version != seal.version {
                    return Err(FreezeError::Drift);
                }
                match reg.face_digest(r.face) {
                    Some(d) => {
                        if d != r.digest {
                            return Err(FreezeError::Drift);
                        }
                    }
                    None => return Err(FreezeError::Unknown),
                }
            }
            i += 1;
        }
        Ok(self.records.len())
    }
}

/// 布局冻结接口登记表。
pub struct Registry {
    state: FreezeState,
    faces: Vec<Face>,
    seal: Option<Seal>,
    version: u32,
    capacity: usize,
    next_id: u16,
}

impl Registry {
    pub fn new(capacity: usize) -> Result<Registry, FreezeError> {
        if capacity == 0 || capacity > MAX_FACES {
            return Err(FreezeError::Range);
        }
        Ok(Registry {
            state: FreezeState::Draft,
            faces: Vec::with_capacity(capacity),
            seal: None,
            version: 0,
            capacity,
            next_id: 1,
        })
    }

    pub fn state(&self) -> FreezeState {
        self.state
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn seal(&self) -> Option<Seal> {
        self.seal
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// 声明一个接口面（仅 Draft 期）。名字唯一、尺寸非零、容量未满。
    pub fn declare(&mut self, name: &str, width: u32, height: u32) -> Result<u16, FreezeError> {
        match self.state {
            FreezeState::Draft => {}
            FreezeState::Candidate => return Err(FreezeError::State),
            FreezeState::Frozen => return Err(FreezeError::Sealed),
            FreezeState::Retired => return Err(FreezeError::Retired),
        }
        if name.is_empty() {
            return Err(FreezeError::Empty);
        }
        if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
            return Err(FreezeError::Range);
        }
        if self.faces.len() >= self.capacity || self.next_id == u16::MAX {
            return Err(FreezeError::Full);
        }
        let mut i = 0usize;
        while i < self.faces.len() {
            if let Some(f) = self.faces.get(i) {
                if f.name == name {
                    return Err(FreezeError::Duplicate);
                }
            }
            i += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.faces.push(Face::new(id, name, width, height));
        Ok(id)
    }

    /// Draft → Candidate。进 Frozen 必须走 `freeze`，这里不提供捷径。
    pub fn promote(&mut self) -> Result<FreezeState, FreezeError> {
        match self.state {
            FreezeState::Draft => {
                self.state = FreezeState::Candidate;
                Ok(self.state)
            }
            FreezeState::Candidate => Err(FreezeError::State),
            FreezeState::Frozen => Err(FreezeError::State),
            FreezeState::Retired => Err(FreezeError::Retired),
        }
    }

    /// Candidate → Frozen：算指纹、落封条、推进版本水位。
    pub fn freeze(&mut self, version: u32) -> Result<Seal, FreezeError> {
        match self.state {
            FreezeState::Candidate => {}
            FreezeState::Frozen => return Err(FreezeError::Sealed),
            FreezeState::Retired => return Err(FreezeError::Retired),
            FreezeState::Draft => return Err(FreezeError::NotSealed),
        }
        if self.faces.is_empty() {
            return Err(FreezeError::Empty);
        }
        if version == 0 || version > MAX_VERSION {
            return Err(FreezeError::Range);
        }
        if version <= self.version {
            return Err(FreezeError::Version);
        }
        let seal = Seal {
            version,
            fingerprint: fingerprint_of(&self.faces),
            faces: self.faces.len() as u16,
        };
        self.seal = Some(seal);
        self.state = FreezeState::Frozen;
        self.version = version;
        Ok(seal)
    }

    /// Frozen → Retired。
    pub fn retire(&mut self) -> Result<(), FreezeError> {
        match self.state {
            FreezeState::Frozen => {
                self.state = FreezeState::Retired;
                Ok(())
            }
            FreezeState::Retired => Err(FreezeError::Retired),
            _ => Err(FreezeError::State),
        }
    }

    /// Retired → Draft：开启下一版本周期（面表与版本水位延续，见文件头的理由）。
    pub fn reopen(&mut self) -> Result<(), FreezeError> {
        match self.state {
            FreezeState::Retired => {
                self.state = FreezeState::Draft;
                Ok(())
            }
            _ => Err(FreezeError::State),
        }
    }

    /// 修订面参数（仅 Draft/Candidate）。修订号严格 +1，摘要随参数重算。
    pub fn revise(&mut self, id: u16, width: u32, height: u32) -> Result<u16, FreezeError> {
        match self.state {
            FreezeState::Frozen => return Err(FreezeError::Sealed),
            FreezeState::Retired => return Err(FreezeError::Retired),
            _ => {}
        }
        if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
            return Err(FreezeError::Range);
        }
        let mut i = 0usize;
        while i < self.faces.len() {
            let hit = match self.faces.get(i) {
                Some(f) => f.id == id,
                None => false,
            };
            if hit {
                if let Some(f) = self.faces.get_mut(i) {
                    if f.revision == u16::MAX {
                        return Err(FreezeError::Range);
                    }
                    f.revision += 1;
                    f.width = width;
                    f.height = height;
                    f.digest = face_digest(&f.name, f.revision, width, height);
                    return Ok(f.revision);
                }
                break;
            }
            i += 1;
        }
        Err(FreezeError::Unknown)
    }

    /// 单点消费入口：只有 Frozen 可读，读取自动落消费账。
    pub fn consume(
        &self,
        id: u16,
        consumer: u16,
        log: &mut ConsumerLog,
    ) -> Result<FaceView, FreezeError> {
        match self.state {
            FreezeState::Frozen => {}
            FreezeState::Retired => return Err(FreezeError::Retired),
            _ => return Err(FreezeError::NotSealed),
        }
        let seal = match self.seal {
            Some(s) => s,
            None => return Err(FreezeError::NotSealed),
        };
        let mut i = 0usize;
        while i < self.faces.len() {
            if let Some(f) = self.faces.get(i) {
                if f.id == id {
                    let view = FaceView {
                        id: f.id,
                        revision: f.revision,
                        digest: f.digest,
                        width: f.width,
                        height: f.height,
                    };
                    log.record(ConsumerRecord {
                        consumer,
                        face: id,
                        version: seal.version,
                        digest: view.digest,
                    })?;
                    return Ok(view);
                }
            }
            i += 1;
        }
        Err(FreezeError::Unknown)
    }

    pub fn face_digest(&self, id: u16) -> Option<u64> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if let Some(f) = self.faces.get(i) {
                if f.id == id {
                    return Some(f.digest);
                }
            }
            i += 1;
        }
        None
    }

    /// 复算当前面表指纹并与自身封条比对（NotSealed / Fingerprint 各自成码）。
    pub fn verify_seal(&self) -> Result<(), FreezeError> {
        let seal = match self.seal {
            Some(s) => s,
            None => return Err(FreezeError::NotSealed),
        };
        self.verify_with(&seal)
    }

    /// 用外部封条（持久化后的旧封条）复验当前面表。
    pub fn verify_with(&self, seal: &Seal) -> Result<(), FreezeError> {
        if self.faces.len() as u16 != seal.faces {
            return Err(FreezeError::Fingerprint);
        }
        if fingerprint_of(&self.faces) != seal.fingerprint {
            return Err(FreezeError::Fingerprint);
        }
        Ok(())
    }
}

/// 面表指纹：逐面折叠 (id, revision, width, height, digest)。
pub fn fingerprint_of(faces: &[Face]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < faces.len() {
        if let Some(f) = faces.get(i) {
            h = fnv_u32(h, f.id as u32);
            h = fnv_u32(h, f.revision as u32);
            h = fnv_u32(h, f.width);
            h = fnv_u32(h, f.height);
            h = fnv_bytes(h, f.name.as_bytes());
            h ^= f.digest;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

fn frozen_registry() -> Result<Registry, FreezeError> {
    let mut r = Registry::new(8)?;
    r.declare("sheet", 595, 842)?;
    r.declare("gutter", 8, 1)?;
    r.declare("marks", 24, 24)?;
    r.promote()?;
    r.freeze(1)?;
    Ok(r)
}

/// 注入核：五类坏操作必须分别落到 Empty / Duplicate / Sealed / Unknown / Version。
pub fn injection_core() -> (usize, usize) {
    let total = 5usize;
    let mut caught = 0usize;

    let mut empty = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => return (0, total),
    };
    if empty.promote().is_ok() && empty.freeze(1).err() == Some(FreezeError::Empty) {
        caught += 1;
    }

    let mut dup = match Registry::new(4) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if dup.declare("sheet", 595, 842).is_ok()
        && dup.declare("sheet", 420, 595).err() == Some(FreezeError::Duplicate)
    {
        caught += 1;
    }

    let mut sealed = match frozen_registry() {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if sealed.declare("late", 10, 10).err() == Some(FreezeError::Sealed)
        && sealed.revise(1, 10, 10).err() == Some(FreezeError::Sealed)
    {
        caught += 1;
    }

    let unknown = match frozen_registry() {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let mut log = match ConsumerLog::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    if unknown.consume(999, 1, &mut log).err() == Some(FreezeError::Unknown) {
        caught += 1;
    }

    let mut versioned = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if versioned.declare("sheet", 595, 842).is_ok()
        && versioned.promote().is_ok()
        && versioned.freeze(1).is_ok()
        && versioned.retire().is_ok()
        && versioned.reopen().is_ok()
        && versioned.promote().is_ok()
        && versioned.freeze(1).err() == Some(FreezeError::Version)
    {
        caught += 1;
    }
    (caught, total)
}

/// 冻结接口域自检（J1：接口冻结登记 + 封后写拒 + 消费同视）。
pub fn run_unx50033_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50033");

    // J1-1 冻结登记：三面登记后封账，封条面数/版本/指纹齐备且复算通过。
    let (ok, det) = match frozen_registry() {
        Ok(r) => {
            let state_ok = r.state() == FreezeState::Frozen;
            let seal_ok = match r.seal() {
                Some(seal) => {
                    seal.version == 1 && seal.faces == 3 && seal.fingerprint == fingerprint_of(r.faces())
                }
                None => false,
            };
            (
                state_ok && seal_ok && r.verify_seal().is_ok() && r.version() == 1,
                format!(
                    "state={} faces={} version={}",
                    r.state().label(),
                    r.faces().len(),
                    r.version()
                ),
            )
        }
        Err(e) => (false, format!("build={}", e.code())),
    };
    s.add_dyn("50033-J1-接口冻结登记", ok, &det);

    // J1-2 封后写拒：declare / revise 一律 Sealed。
    let write = match frozen_registry() {
        Ok(mut r) => {
            let a = r.declare("late", 1, 1).err() == Some(FreezeError::Sealed);
            let b = r.revise(1, 99, 99).err() == Some(FreezeError::Sealed);
            (a && b, a, b)
        }
        Err(_) => (false, false, false),
    };
    let det = format!("declare={} revise={}", write.1, write.2);
    s.add_dyn("50033-J1-封账写拒", write.0, &det);

    // J1-3 版本单调：v1 封账 → 退休 → 重开 → 同版/零版必拒、v2 放行。
    let mut vs = match Registry::new(4) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let declared = vs.declare("sheet", 595, 842).is_ok();
    let promoted = vs.promote().is_ok();
    let first = vs.freeze(1).is_ok();
    let retired = vs.retire().is_ok();
    let reopened = vs.reopen().is_ok();
    let promoted2 = vs.promote().is_ok();
    let zero_bad = vs.freeze(0).err() == Some(FreezeError::Range);
    let same_bad = vs.freeze(1).err() == Some(FreezeError::Version);
    let second = vs.freeze(2);
    let ok = declared
        && promoted
        && first
        && retired
        && reopened
        && promoted2
        && zero_bad
        && same_bad
        && second.is_ok()
        && vs.version() == 2;
    let det = format!(
        "zero_bad={} same_bad={} second={}",
        zero_bad,
        same_bad,
        second.is_ok()
    );
    s.add_dyn("50033-J1-版本单调", ok, &det);

    // J1-4 空表/空名必拒。
    let mut empty_tab = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let promoted_empty = empty_tab.promote().is_ok();
    let empty_seal = empty_tab.freeze(1).err() == Some(FreezeError::Empty);
    let mut empty_name = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let empty_name_bad = empty_name.declare("", 10, 10).err() == Some(FreezeError::Empty);
    let ok = promoted_empty && empty_seal && empty_name_bad;
    let det = format!("empty_tab={} empty_name={}", empty_seal, empty_name_bad);
    s.add_dyn("50033-J1-空表空名拒", ok, &det);

    // J1-5 重复名拒（第一次成功、第二次 Duplicate）。
    let mut dup = match Registry::new(4) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let first = dup.declare("sheet", 595, 842);
    let again = dup.declare("sheet", 595, 842).err() == Some(FreezeError::Duplicate);
    let ok = first.is_ok() && again && dup.faces().len() == 1;
    s.add("50033-J1-重复名拒", ok, if ok { "" } else { "重名未拒或重复登记" });

    // J1-6 状态机非法：未冻结消费 NotSealed、双 promote State、未冻结 retire State、
    // 未退休 reopen State、退休后 promote/consume/freeze 全部 Retired。
    let mut st = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let mut log = match ConsumerLog::new(8) {
        Ok(l) => l,
        Err(_) => unreachable_log(),
    };
    let _ = st.declare("sheet", 595, 842);
    let before_freeze = st.consume(1, 7, &mut log).err() == Some(FreezeError::NotSealed);
    let p1 = st.promote().is_ok();
    let p2_bad = st.promote().err() == Some(FreezeError::State);
    let retire_early = st.retire().err() == Some(FreezeError::State);
    let reopen_early = st.reopen().err() == Some(FreezeError::State);
    let froze = st.freeze(1).is_ok();
    let retired = st.retire().is_ok();
    let promote_retired = st.promote().err() == Some(FreezeError::Retired);
    let consume_retired = st.consume(1, 7, &mut log).err() == Some(FreezeError::Retired);
    let freeze_retired = st.freeze(2).err() == Some(FreezeError::Retired);
    let ok = before_freeze
        && p1
        && p2_bad
        && retire_early
        && reopen_early
        && froze
        && retired
        && promote_retired
        && consume_retired
        && freeze_retired;
    let det = format!(
        "pre={} p2={} retire={} reopen={} post={}/{}/{}",
        before_freeze, p2_bad, retire_early, reopen_early, promote_retired, consume_retired, freeze_retired
    );
    s.add_dyn("50033-J1-状态机非法", ok, &det);

    // J1-7 指纹守恒与敏感：复算相等；只改一面宽度指纹必变。
    let fp = match frozen_registry() {
        Ok(r) => {
            let base = fingerprint_of(r.faces());
            let sealed = match r.seal() {
                Some(v) => v.fingerprint == base,
                None => false,
            };
            let mut cloned = r.faces().to_vec();
            let mut changed = false;
            if let Some(f) = cloned.first_mut() {
                f.width = f.width.wrapping_add(1);
                f.digest = face_digest(&f.name, f.revision, f.width, f.height);
                changed = fingerprint_of(&cloned) != base;
            }
            (sealed && changed && r.verify_seal().is_ok(), sealed, changed)
        }
        Err(_) => (false, false, false),
    };
    let det = format!("sealed={} sensitive={}", fp.1, fp.2);
    s.add_dyn("50033-J1-指纹守恒", fp.0, &det);

    // J1-8 消费者同视：三消费者读同一面，视图逐位一致，审计 3 条全绿。
    let same = match frozen_registry() {
        Ok(r) => {
            let mut log = match ConsumerLog::new(8) {
                Ok(l) => l,
                Err(_) => unreachable_log(),
            };
            let a = r.consume(1, 10, &mut log);
            let b = r.consume(1, 11, &mut log);
            let c = r.consume(1, 12, &mut log);
            match (a, b, c) {
                (Ok(va), Ok(vb), Ok(vc)) => {
                    let equal = va == vb && vb == vc;
                    let audit = log.audit(&r).ok() == Some(3) && log.len() == 3;
                    (equal && audit, equal, audit)
                }
                _ => (false, false, false),
            }
        }
        Err(_) => (false, false, false),
    };
    let det = format!("view_equal={} audit={}", same.1, same.2);
    s.add_dyn("50033-J1-消费者同视", same.0, &det);

    // J1-9 消费漂移：版本错位 / 摘要错位都必须 Drift。
    let drift = match frozen_registry() {
        Ok(r) => match r.seal() {
            Some(seal) => {
                let real = r.face_digest(1);
                let base = match real {
                    Some(d) => d,
                    None => 0,
                };
                let mut log = match ConsumerLog::new(4) {
                    Ok(l) => l,
                    Err(_) => unreachable_log(),
                };
                let ver_bad = log
                    .record(ConsumerRecord {
                        consumer: 1,
                        face: 1,
                        version: seal.version + 1,
                        digest: base,
                    })
                    .is_ok()
                    && log.audit(&r).err() == Some(FreezeError::Drift);
                let mut log2 = match ConsumerLog::new(4) {
                    Ok(l) => l,
                    Err(_) => unreachable_log(),
                };
                let dig_bad = log2
                    .record(ConsumerRecord {
                        consumer: 2,
                        face: 1,
                        version: seal.version,
                        digest: base ^ 1,
                    })
                    .is_ok()
                    && log2.audit(&r).err() == Some(FreezeError::Drift);
                (ver_bad && dig_bad, ver_bad, dig_bad)
            }
            None => (false, false, false),
        },
        Err(_) => (false, false, false),
    };
    let det = format!("version={} digest={}", drift.1, drift.2);
    s.add_dyn("50033-J1-消费漂移检出", drift.0, &det);

    // J1-10 读幂等：同面读两次视图一致，消费账追加两条（append-only 不合并）。
    let idem = match frozen_registry() {
        Ok(r) => {
            let mut log = match ConsumerLog::new(4) {
                Ok(l) => l,
                Err(_) => unreachable_log(),
            };
            let a = r.consume(2, 20, &mut log);
            let b = r.consume(2, 20, &mut log);
            match (a, b) {
                (Ok(va), Ok(vb)) => (va == vb && log.len() == 2, log.len()),
                _ => (false, 0),
            }
        }
        Err(_) => (false, 0),
    };
    let det = format!("log_len={}", idem.1);
    s.add_dyn("50033-J1-读幂等", idem.0, &det);

    // J1-11 容量满拒：面表 2 满后第 3 面 Full；消费账 1 满后第 2 条 Full。
    let mut cap_reg = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let d1 = cap_reg.declare("a", 10, 10).is_ok();
    let d2 = cap_reg.declare("b", 10, 10).is_ok();
    let d3 = cap_reg.declare("c", 10, 10).err() == Some(FreezeError::Full);
    let mut cap_log = match ConsumerLog::new(1) {
        Ok(l) => l,
        Err(_) => unreachable_log(),
    };
    let r1 = cap_log
        .record(ConsumerRecord {
            consumer: 1,
            face: 1,
            version: 1,
            digest: 0,
        })
        .is_ok();
    let r2 = cap_log
        .record(ConsumerRecord {
            consumer: 1,
            face: 1,
            version: 1,
            digest: 0,
        })
        .err()
        == Some(FreezeError::Full);
    let ok = d1 && d2 && d3 && r1 && r2;
    let det = format!("faces_full={} log_full={}", d3, r2);
    s.add_dyn("50033-J1-容量满拒", ok, &det);

    // J1-12 未知面拒：消费侧（Frozen 门内）与修订侧（Draft 门内）都返回 Unknown。
    let unknown = match frozen_registry() {
        Ok(r) => {
            let mut log = match ConsumerLog::new(2) {
                Ok(l) => l,
                Err(_) => unreachable_log(),
            };
            let c = r.consume(4242, 1, &mut log).err() == Some(FreezeError::Unknown);
            let mut draft = match Registry::new(2) {
                Ok(v) => v,
                Err(_) => unreachable_registry(),
            };
            let _ = draft.declare("sheet", 595, 842);
            let c2 = draft.revise(4242, 10, 10).err() == Some(FreezeError::Unknown);
            (c && c2, c, c2)
        }
        Err(_) => (false, false, false),
    };
    let det = format!("consume_unknown={} revise_unknown={}", unknown.1, unknown.2);
    s.add_dyn("50033-J1-未知面拒", unknown.0, &det);

    // J1-13 范围拒：容量 0、尺寸 0、版本超上限。
    let range_reg = Registry::new(0).err() == Some(FreezeError::Range);
    let range_log = ConsumerLog::new(0).err() == Some(FreezeError::Range);
    let mut zero_dim = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let dim_bad = zero_dim.declare("x", 0, 10).err() == Some(FreezeError::Range);
    let mut high = match Registry::new(2) {
        Ok(r) => r,
        Err(_) => unreachable_registry(),
    };
    let _ = high.declare("x", 10, 10);
    let _ = high.promote();
    let ver_bad = high.freeze(MAX_VERSION + 1).err() == Some(FreezeError::Range);
    let ok = range_reg && range_log && dim_bad && ver_bad;
    let det = format!(
        "reg={} log={} dim={} version={}",
        range_reg, range_log, dim_bad, ver_bad
    );
    s.add_dyn("50033-J1-范围拒", ok, &det);

    // J1-14 旧封条再验：退休→重开→修订后，旧封条必须 Fingerprint 失效。
    let reverify = match frozen_registry() {
        Ok(mut r) => match r.seal() {
            Some(old) => {
                let ok_chain =
                    r.retire().is_ok() && r.reopen().is_ok() && r.revise(1, 600, 850).is_ok();
                let stale = r.verify_with(&old).err() == Some(FreezeError::Fingerprint);
                let unstamped = match Registry::new(2) {
                    Ok(q) => q.verify_seal().err() == Some(FreezeError::NotSealed),
                    Err(_) => false,
                };
                (ok_chain && stale && unstamped, stale, unstamped)
            }
            None => (false, false, false),
        },
        Err(_) => (false, false, false),
    };
    let det = format!("stale={} unstamped={}", reverify.1, reverify.2);
    s.add_dyn("50033-J1-旧封条再验", reverify.0, &det);

    // J1-15 五类注入全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50033-J1-注入五类", ok, &det);

    s
}

/// 只在构造失败分支被取用：自检内部不可能走到，保持无 panic 的返回值形态。
fn unreachable_registry() -> Registry {
    match Registry::new(1) {
        Ok(r) => r,
        Err(_) => Registry {
            state: FreezeState::Draft,
            faces: Vec::new(),
            seal: None,
            version: 0,
            capacity: 1,
            next_id: 1,
        },
    }
}

fn unreachable_log() -> ConsumerLog {
    match ConsumerLog::new(1) {
        Ok(l) => l,
        Err(_) => ConsumerLog {
            records: Vec::new(),
            capacity: 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50033_checks_green() {
        let set = run_unx50033_checks();
        assert!(set.all_passed(), "UNX-F50033 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50033_seal_and_consumers_consistent() {
        let r = match frozen_registry() {
            Ok(v) => v,
            Err(e) => panic!("build {}", e.code()),
        };
        assert_eq!(r.state(), FreezeState::Frozen);
        assert!(r.verify_seal().is_ok());
        let mut log = match ConsumerLog::new(4) {
            Ok(l) => l,
            Err(_) => panic!("log"),
        };
        let a = r.consume(1, 1, &mut log);
        let b = r.consume(1, 2, &mut log);
        assert_eq!(a.ok(), b.ok());
        assert_eq!(log.audit(&r).ok(), Some(2));
    }

    #[test]
    fn unx50033_frozen_write_rejected() {
        let mut r = match frozen_registry() {
            Ok(v) => v,
            Err(_) => panic!("build"),
        };
        assert_eq!(r.declare("late", 1, 1).err(), Some(FreezeError::Sealed));
        assert_eq!(r.revise(1, 5, 5).err(), Some(FreezeError::Sealed));
    }
}
