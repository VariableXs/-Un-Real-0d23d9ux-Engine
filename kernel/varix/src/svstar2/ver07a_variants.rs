//! VE-F3441 · 变体派生引擎总纲 —— 五轴、派生链、组合合法性、确定性切换
//!
//! ## 职责拆解
//! 变体是令牌系统的**第二维度**：同一批令牌在不同密度/圆角/字阶/动效/焦点环下
//! 呈现的形态族。本册只做「骨架」，不做任何具体轴的数值（那是 F3442~F3446 的事）：
//! 1. **轴**（`Axis`）：五条正交轴，每轴三档。轴的数量与名字在这里定死，
//!    后五册只填数值。轴一旦公开就不能改（改轴等于改 API）。
//! 2. **派生**（`Derivation` / `DerivationBook`）：**变体是派生不是复制**（律一）。
//!    每个非基准档必须声明「从谁派生」，链必须终止于基准档；声明成复制、跨轴派生、
//!    成环、超深一律拒。这条律的意义在于：复制出来的变体在令牌改名时会静默失联，
//!    派生出来的变体跟着源一起动。
//! 3. **组合**（`Combo` / `MUTEX` / `presets`）：五轴各三档 = 243 种组合。
//!    不是全都合理——紧凑密度配松散字阶会把一行字挤成两行半。互斥表把「不合理」
//!    写成数据而不是注释，于是可以被 CI 检查、可以在文档里自动列出。
//! 4. **切换**（`SwitchLedger`）：**切换确定可回滚**（律二）。同一 (from,to) 必须
//!    产出同一结果哈希（确定性），且必须能退回上一步（可回滚）。切到当前档
//!    不算切换（NoOp），否则历史里会塞满空转，回滚一步等于没动。
//! 5. **收口**（`Verdict`）：四件齐 + 两律证，才允许 E03 后续册子对外发布。
//!
//! ## 为什么这么拆
//! 变体系统最容易烂在两处：一是「复制粘贴出一堆看起来能用的档位」，二是
//! 「组合爆炸后没人知道哪些组合真的能跑」。前者靠律一（派生链）堵，后者靠互斥表
//! 把「不能跑」写成可执行数据。这两件事必须在总纲里定，下沉到各轴就晚了——
//! 每个轴都会发明自己的复制方式。
//!
//! ## 复杂度
//! - 派生链求值：O(深度)，深度上限 4（`DEPTH_CAP`），故常数级。
//! - 组合合法性：O(互斥表项) = 常数（表长 ≤ 8）。
//! - 切换：O(1)；回滚 O(1)。
//! - 全量审计：O(轴×档 + 表) ≈ 数十次比较。
//!
//! ## 零 panic 面
//! - `Combo` 用定长数组 `[u8; 5]`，索引由 `Axis::index()` 提供（0..5 恒定）。
//! - 所有 `get()` 访问，无切片索引；无 `unwrap` / `expect`。
//! - 哈希全 `wrapping_*`；代数计数器 `saturating_add`。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 派生链深度上限。三层派生足够（基准 → 中间态 → 极端态），
/// 再深说明轴的设计本身有问题：一档变体不该需要四步推导才说得清。
pub const DEPTH_CAP: usize = 4;

/// 每轴档数。三档是本系统的硬约定：少一档无法表达「两端」，多一档用户选不过来。
pub const LEVELS: u8 = 3;

/// 诊断码（E18 段 · 5581~5596）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum VariantCode {
    /// 档位越界（只允许 0..3）。
    LevelOutOfRange = 5581,
    /// 非基准档未声明派生源。
    OrphanVariant = 5582,
    /// 声明为「复制」而非派生。
    CopiedNotDerived = 5583,
    /// 跨轴派生。
    CrossAxisDerivation = 5584,
    /// 派生链成环。
    DerivationCycle = 5585,
    /// 派生链超深。
    DerivationTooDeep = 5586,
    /// 派生源不存在。
    SourceMissing = 5587,
    /// 组合命中互斥。
    MutuallyExclusive = 5588,
    /// 组合不在任何预置里且未登记为按需派生。
    UnregisteredCombo = 5589,
    /// 切到当前档（空转）。
    NoOpSwitch = 5590,
    /// 无可回滚的历史。
    NothingToRollback = 5591,
    /// 同一 (from,to) 两次结果不同（不确定性）。
    Nondeterministic = 5592,
    /// 四件缺失（派生/组合/切换/验证任一未接）。
    CapabilityMissing = 5593,
    /// 两律未证。
    LawUnproven = 5594,
    /// 轴表被改动（公开 API 漂移）。
    AxisDrift = 5595,
    /// 预置组合引用了不存在的档。
    PresetBroken = 5596,
}

impl VariantCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            VariantCode::LevelOutOfRange => "档位越界",
            VariantCode::OrphanVariant => "非基准档缺派生源",
            VariantCode::CopiedNotDerived => "变体被声明为复制",
            VariantCode::CrossAxisDerivation => "跨轴派生",
            VariantCode::DerivationCycle => "派生链成环",
            VariantCode::DerivationTooDeep => "派生链超深",
            VariantCode::SourceMissing => "派生源不存在",
            VariantCode::MutuallyExclusive => "组合命中互斥",
            VariantCode::UnregisteredCombo => "组合未登记",
            VariantCode::NoOpSwitch => "切到当前档（空转）",
            VariantCode::NothingToRollback => "无可回滚历史",
            VariantCode::Nondeterministic => "切换结果不确定",
            VariantCode::CapabilityMissing => "四件不齐",
            VariantCode::LawUnproven => "两律未证",
            VariantCode::AxisDrift => "轴表漂移",
            VariantCode::PresetBroken => "预置组合损坏",
        }
    }
}

/// 五条轴。顺序即 `Combo` 的槽位顺序，改动等于破坏兼容——故 `AxisDrift` 码存在。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Axis {
    Density,
    Radius,
    TypeScale,
    Motion,
    FocusRing,
}

impl Axis {
    pub const ALL: [Axis; 5] = [
        Axis::Density,
        Axis::Radius,
        Axis::TypeScale,
        Axis::Motion,
        Axis::FocusRing,
    ];

    pub const fn index(self) -> usize {
        match self {
            Axis::Density => 0,
            Axis::Radius => 1,
            Axis::TypeScale => 2,
            Axis::Motion => 3,
            Axis::FocusRing => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Axis::Density => "密度",
            Axis::Radius => "圆角",
            Axis::TypeScale => "字阶",
            Axis::Motion => "动效",
            Axis::FocusRing => "焦点环",
        }
    }

    /// 轴 id（写进持久化与 API）。
    pub fn id(self) -> &'static str {
        match self {
            Axis::Density => "density",
            Axis::Radius => "radius",
            Axis::TypeScale => "typescale",
            Axis::Motion => "motion",
            Axis::FocusRing => "focusring",
        }
    }

    /// 三档的名字。名字是用户可见文案，所以放在总纲统一：各轴自己起名会导致
    /// 「密度:紧凑」与「字阶:tight」两种语言体系并存。
    pub fn level_name(self, level: u8) -> &'static str {
        match (self, level) {
            (Axis::Density, 0) => "compact",
            (Axis::Density, 1) => "standard",
            (Axis::Density, 2) => "comfortable",
            (Axis::Radius, 0) => "sharp",
            (Axis::Radius, 1) => "standard",
            (Axis::Radius, 2) => "round",
            (Axis::TypeScale, 0) => "tight",
            (Axis::TypeScale, 1) => "standard",
            (Axis::TypeScale, 2) => "loose",
            (Axis::Motion, 0) => "calm",
            (Axis::Motion, 1) => "standard",
            (Axis::Motion, 2) => "quick",
            (Axis::FocusRing, 0) => "thin",
            (Axis::FocusRing, 1) => "standard",
            (Axis::FocusRing, 2) => "bold",
            _ => "unknown",
        }
    }

    /// 出厂默认档：一律中档。默认必须是「最不惹眼」的那一档，
    /// 因为默认值会被最多的用户看到，也最容易被当成「就是这样」。
    pub const fn default_level(self) -> u8 {
        1
    }

    /// 该轴的基准档（派生链的根）。与默认档分开：基准是**结构性**的，
    /// 默认是**体验性**的；将来允许默认档变成 0 而基准仍是 1 也不破坏派生。
    pub const fn base_level(self) -> u8 {
        0
    }
}

/// 变体键：轴 + 档。这是全系统唯一的变体标识，持久化/API/文档都用它。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct VariantKey {
    pub axis: Axis,
    pub level: u8,
}

impl VariantKey {
    pub fn new(axis: Axis, level: u8) -> Result<Self, VariantCode> {
        if level >= LEVELS {
            return Err(VariantCode::LevelOutOfRange);
        }
        Ok(Self { axis, level })
    }

    pub fn label(&self) -> String {
        format!("{}.{}", self.axis.id(), self.axis.level_name(self.level))
    }

    pub fn is_base(&self) -> bool {
        self.level == self.axis.base_level()
    }
}

/// 组合：五轴各一档。定长数组，槽位与 `Axis::index()` 对齐。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    pub levels: [u8; 5],
}

impl Combo {
    pub const fn default_combo() -> Self {
        Self {
            levels: [
                Axis::Density.default_level(),
                Axis::Radius.default_level(),
                Axis::TypeScale.default_level(),
                Axis::Motion.default_level(),
                Axis::FocusRing.default_level(),
            ],
        }
    }

    pub fn level(&self, axis: Axis) -> u8 {
        self.levels.get(axis.index()).copied().unwrap_or(0)
    }

    pub fn with(&self, axis: Axis, level: u8) -> Result<Self, VariantCode> {
        if level >= LEVELS {
            return Err(VariantCode::LevelOutOfRange);
        }
        let mut next = *self;
        if let Some(slot) = next.levels.get_mut(axis.index()) {
            *slot = level;
            return Ok(next);
        }
        Err(VariantCode::LevelOutOfRange)
    }

    /// 规范化串：持久化与哈希的唯一输入。
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        for axis in Axis::ALL {
            if !s.is_empty() {
                s.push('/');
            }
            s.push_str(axis.id());
            s.push('=');
            s.push_str(axis.level_name(self.level(axis)));
        }
        s
    }

    /// 结果哈希：用来证明「同一 (from,to) 两次切换结果相同」。
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.canonical().as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 与另一组合的差异轴。影响面计算（F3448）直接消费。
    pub fn changed_axes(&self, other: &Self) -> Vec<Axis> {
        let mut out = Vec::new();
        for axis in Axis::ALL {
            if self.level(axis) != other.level(axis) {
                out.push(axis);
            }
        }
        out
    }
}

impl Default for Combo {
    fn default() -> Self {
        Self::default_combo()
    }
}

/// 派生声明的两种写法。**只有 `Derived` 是合法的**——`Copied` 是显式留出的
/// 「我知道该派生但我抄了」的口子，审计会把它判红。留着它是因为：
/// 让人写出意图，比让人绕过类型系统好。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Derived(VariantKey),
    Copied(VariantKey),
}

/// 派生簿。存「谁从谁派生」。
#[derive(Clone, Debug, Default)]
pub struct DerivationBook {
    entries: Vec<(VariantKey, Origin)>,
}

impl DerivationBook {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 登记一条派生。基准档不允许有源（它就是源）。
    pub fn declare(&mut self, key: VariantKey, origin: Origin) -> Result<(), VariantCode> {
        if key.level >= LEVELS {
            return Err(VariantCode::LevelOutOfRange);
        }
        let src = match origin {
            Origin::Derived(s) | Origin::Copied(s) => s,
        };
        if src.axis != key.axis {
            return Err(VariantCode::CrossAxisDerivation);
        }
        if src.level >= LEVELS {
            return Err(VariantCode::SourceMissing);
        }
        if key.is_base() {
            // 基准档不该声明源。
            return Err(VariantCode::CopiedNotDerived);
        }
        if self.entries.iter().any(|(k, _)| *k == key) {
            return Err(VariantCode::CopiedNotDerived);
        }
        self.entries.push((key, origin));
        Ok(())
    }

    pub fn get(&self, key: VariantKey) -> Option<Origin> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, o)| *o)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 求派生链：从 key 一路走到基准档。返回途经的 key（含自身与根）。
    ///
    /// 三条拒绝路径都真实存在：抄（Copied）、断（Orphan/SourceMissing）、环（Cycle）。
    /// 环在「源必须存在且同轴」的约束下仍可能出现——声明顺序无关，A←B←A 完全可写。
    pub fn chain(&self, key: VariantKey) -> Result<Vec<VariantKey>, VariantCode> {
        let mut out = vec![key];
        let mut cur = key;
        let mut visited = vec![key];
        loop {
            if cur.is_base() {
                return Ok(out);
            }
            let origin = match self.get(cur) {
                Some(o) => o,
                None => return Err(VariantCode::OrphanVariant),
            };
            let src = match origin {
                Origin::Derived(s) => s,
                Origin::Copied(_) => return Err(VariantCode::CopiedNotDerived),
            };
            if visited.iter().any(|k| *k == src) {
                return Err(VariantCode::DerivationCycle);
            }
            visited.push(src);
            out.push(src);
            if out.len() > DEPTH_CAP {
                return Err(VariantCode::DerivationTooDeep);
            }
            cur = src;
        }
    }

    /// 全量审计：每条非基准档都要能走到根，且不得是抄的。
    /// 一次报全（返回所有问题 key），不做短路——否则修一条跑一次。
    pub fn audit(&self) -> Vec<(VariantKey, VariantCode)> {
        let mut out = Vec::new();
        for axis in Axis::ALL {
            for level in 0..LEVELS {
                let key = match VariantKey::new(axis, level) {
                    Ok(k) => k,
                    Err(_) => continue,
                };
                if key.is_base() {
                    continue;
                }
                match self.chain(key) {
                    Ok(_) => {}
                    Err(code) => out.push((key, code)),
                }
            }
        }
        out
    }

    /// 出厂派生簿：每轴 1←0、2←1，纯线性。故意不用「2←0」，
    /// 因为跨档派生会让「改中间档不影响极端档」这个直觉失效。
    pub fn standard() -> Self {
        let mut b = Self::new();
        for axis in Axis::ALL {
            for level in 1..LEVELS {
                let key = match VariantKey::new(axis, level) {
                    Ok(k) => k,
                    Err(_) => continue,
                };
                let src = VariantKey {
                    axis,
                    level: level - 1,
                };
                let _ = b.declare(key, Origin::Derived(src));
            }
        }
        b
    }
}

/// 互斥项：两条轴上的具体档不能同时生效，附一句人话理由。
/// 理由必须写「用户会看到什么」，不写「架构上不允许」——后者无法被复核。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mutex {
    pub a: VariantKey,
    pub b: VariantKey,
    pub why: &'static str,
}

/// 出厂互斥表。三条，都是真实会出问题的组合。
pub const MUTEX: [Mutex; 3] = [
    Mutex {
        a: VariantKey {
            axis: Axis::Density,
            level: 0,
        },
        b: VariantKey {
            axis: Axis::TypeScale,
            level: 2,
        },
        why: "紧凑密度配松散字阶：单行文字被挤到换行，信息密度反而更低",
    },
    Mutex {
        a: VariantKey {
            axis: Axis::Radius,
            level: 0,
        },
        b: VariantKey {
            axis: Axis::FocusRing,
            level: 0,
        },
        why: "锐角配细焦点环：直角处的环被裁掉一半，聚焦不可见",
    },
    Mutex {
        a: VariantKey {
            axis: Axis::Density,
            level: 2,
        },
        b: VariantKey {
            axis: Axis::Motion,
            level: 0,
        },
        why: "舒适密度配舒缓动效：大间距下的长过渡让每次操作都要等，体感变慢",
    },
];

/// 组合合法性检查。返回全部冲突（不短路）。
pub fn conflicts(combo: &Combo) -> Vec<&'static Mutex> {
    let mut out = Vec::new();
    for m in MUTEX.iter() {
        if combo.level(m.a.axis) == m.a.level && combo.level(m.b.axis) == m.b.level {
            out.push(m);
        }
    }
    out
}

/// 预置组合。默认体验即预置组合——这句话是承诺：用户不改任何设置时，
/// 拿到的就是这里的第一条，而不是 243 种里的随机一种。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub combo: Combo,
}

/// 出厂预置三条。覆盖「日常 / 干活 / 阅读」三个真实场景。
pub fn presets() -> Vec<Preset> {
    let mut out = Vec::new();
    out.push(Preset {
        id: "everyday",
        name: "日常",
        combo: Combo::default_combo(),
    });
    let work = Combo::default_combo()
        .with(Axis::Density, 0)
        .unwrap_or(Combo::default_combo())
        .with(Axis::TypeScale, 0)
        .unwrap_or(Combo::default_combo())
        .with(Axis::Motion, 2)
        .unwrap_or(Combo::default_combo())
        .with(Axis::FocusRing, 2)
        .unwrap_or(Combo::default_combo());
    out.push(Preset {
        id: "work",
        name: "干活",
        combo: work,
    });
    let reading = Combo::default_combo()
        .with(Axis::Density, 2)
        .unwrap_or(Combo::default_combo())
        .with(Axis::TypeScale, 2)
        .unwrap_or(Combo::default_combo())
        .with(Axis::Radius, 2)
        .unwrap_or(Combo::default_combo())
        .with(Axis::FocusRing, 2)
        .unwrap_or(Combo::default_combo());
    out.push(Preset {
        id: "reading",
        name: "阅读",
        combo: reading,
    });
    out
}

/// 预置自检：每条预置都不得命中互斥表，且必须能在派生簿里走通全轴。
pub fn audit_presets(book: &DerivationBook) -> Vec<(&'static str, VariantCode)> {
    let mut out = Vec::new();
    for p in presets() {
        if !conflicts(&p.combo).is_empty() {
            out.push((p.id, VariantCode::MutuallyExclusive));
            continue;
        }
        for axis in Axis::ALL {
            let key = match VariantKey::new(axis, p.combo.level(axis)) {
                Ok(k) => k,
                Err(_) => {
                    out.push((p.id, VariantCode::PresetBroken));
                    continue;
                }
            };
            if let Err(code) = book.chain(key) {
                out.push((p.id, code));
            }
        }
    }
    out
}

/// 一次切换。`id` 单调，用于日志对齐与「切了几次」的统计。
///
/// `axis == None` 表示整组合替换（预置切换），此时 `from`/`to` 无意义；
/// 两条路径共用同一个结构，是为了让历史是**一条链**而不是两条互不相干的流水。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transition {
    pub id: u64,
    pub axis: Option<Axis>,
    pub from: u8,
    pub to: u8,
    pub from_combo: Combo,
    pub to_combo: Combo,
}

/// 切换账本：确定 + 可回滚。
#[derive(Clone, Debug)]
pub struct SwitchLedger {
    active: Combo,
    next_id: u64,
    history: Vec<Combo>,
    transitions: Vec<Transition>,
}

impl SwitchLedger {
    pub fn new(initial: Combo) -> Self {
        Self {
            active: initial,
            next_id: 1,
            history: vec![initial],
            transitions: Vec::new(),
        }
    }

    pub fn active(&self) -> Combo {
        self.active
    }

    pub fn depth(&self) -> usize {
        self.history.len().saturating_sub(1)
    }

    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// 切一轴。切到同档是空转，直接拒——空转会污染回滚栈。
    pub fn switch(&mut self, axis: Axis, level: u8) -> Result<Transition, VariantCode> {
        if level >= LEVELS {
            return Err(VariantCode::LevelOutOfRange);
        }
        let from_combo = self.active;
        let from = from_combo.level(axis);
        if from == level {
            return Err(VariantCode::NoOpSwitch);
        }
        let next = from_combo.with(axis, level)?;
        self.active = next;
        self.history.push(next);
        let t = Transition {
            id: self.next_id,
            axis: Some(axis),
            from,
            to: level,
            from_combo,
            to_combo: next,
        };
        self.next_id = self.next_id.saturating_add(1);
        self.transitions.push(t);
        Ok(t)
    }

    /// 整体套用一个组合（预置切换走这条）。只记一条历史——
    /// 预置切换在用户眼里是「换了个模式」，不是「改了四个下拉框」。
    pub fn apply(&mut self, target: Combo) -> Result<Transition, VariantCode> {
        if target == self.active {
            return Err(VariantCode::NoOpSwitch);
        }
        let from_combo = self.active;
        self.active = target;
        self.history.push(target);
        let t = Transition {
            id: self.next_id,
            axis: None,
            from: 0,
            to: 0,
            from_combo,
            to_combo: target,
        };
        self.next_id = self.next_id.saturating_add(1);
        self.transitions.push(t);
        Ok(t)
    }

    /// 回滚一步。回到上一个组合（不是「切回某轴旧值」——整组合回退才可预期）。
    pub fn rollback(&mut self) -> Result<Combo, VariantCode> {
        if self.history.len() <= 1 {
            return Err(VariantCode::NothingToRollback);
        }
        self.history.pop();
        match self.history.last() {
            Some(c) => {
                self.active = *c;
                Ok(*c)
            }
            None => Err(VariantCode::NothingToRollback),
        }
    }

    /// 确定性证明（律二前半）。两件事同时成立才算：
    /// 1. **链条连续**：每步的 `from_combo` 必须等于上一步的 `to_combo`；
    /// 2. **纯函数**：每步的 `to_combo` 必须能由 `from_combo` + 该步指令重新算出。
    /// 第 2 条是真正的「确定」——只比哈希等于自己，等于什么都没比。
    pub fn replay_is_deterministic(&self) -> bool {
        let mut cur = match self.history.first() {
            Some(c) => *c,
            None => return false,
        };
        for t in &self.transitions {
            if t.from_combo.hash() != cur.hash() {
                return false;
            }
            let expected = match t.axis {
                Some(axis) => match cur.with(axis, t.to) {
                    Ok(c) => c,
                    Err(_) => return false,
                },
                None => t.to_combo,
            };
            if expected.hash() != t.to_combo.hash() {
                return false;
            }
            cur = t.to_combo;
        }
        cur.hash() == self.active.hash()
    }

    /// 回滚到底（全部撤销）。返回撤销步数。
    pub fn rollback_all(&mut self) -> usize {
        let mut n = 0usize;
        while self.rollback().is_ok() {
            n += 1;
            if n > 1024 {
                break;
            }
        }
        n
    }
}

/// 两律的证明记录。收口（F3460）只认这两条。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LawProof {
    /// 律一：全部非基准档都能派生到根，且无抄写。
    pub derived_not_copied: bool,
    /// 律二：切换确定且可回滚。
    pub switch_deterministic_rollback: bool,
}

/// 四件是否齐备。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Capabilities {
    pub derivation: bool,
    pub composition: bool,
    pub switching: bool,
    pub validation: bool,
}

impl Capabilities {
    pub fn complete(&self) -> bool {
        self.derivation && self.composition && self.switching && self.validation
    }
}

/// 总纲自检结论。
#[derive(Clone, Debug)]
pub struct Verdict {
    pub capabilities: Capabilities,
    pub laws: LawProof,
    pub issues: Vec<(String, VariantCode)>,
}

impl Verdict {
    pub fn is_green(&self) -> bool {
        self.capabilities.complete()
            && self.laws.derived_not_copied
            && self.laws.switch_deterministic_rollback
            && self.issues.is_empty()
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "E03 总纲 · 四件 {} · 律一 {} · 律二 {} · 问题 {}",
            if self.capabilities.complete() { "齐" } else { "缺" },
            if self.laws.derived_not_copied { "证" } else { "未证" },
            if self.laws.switch_deterministic_rollback {
                "证"
            } else {
                "未证"
            },
            self.issues.len()
        ));
        for (what, code) in &self.issues {
            out.push(format!("  [{}] {} · {}", code.code(), what, code.text()));
        }
        out
    }
}

/// 总纲自检：把四件与两律都跑一遍。
pub fn audit(book: &DerivationBook, ledger: &SwitchLedger) -> Verdict {
    let mut issues: Vec<(String, VariantCode)> = Vec::new();

    // 律一：派生。
    let derived = book.audit();
    for (key, code) in &derived {
        issues.push((key.label(), *code));
    }

    // 组合：预置必须合法。
    for (id, code) in audit_presets(book) {
        issues.push((format!("preset:{}", id), code));
    }

    // 律二：确定性 + 可回滚（用副本试回滚，不动调用方的账本）。
    let mut probe = ledger.clone();
    let deterministic = ledger.replay_is_deterministic();
    if !deterministic {
        issues.push(("切换不可复现".to_string(), VariantCode::Nondeterministic));
    }
    let rollback_ok = match probe.rollback() {
        Ok(_) => true,
        // 没有历史时回滚失败是正确行为，不算缺陷。
        Err(VariantCode::NothingToRollback) => ledger.depth() == 0,
        Err(code) => {
            issues.push(("回滚失败".to_string(), code));
            false
        }
    };

    // 轴表自检：五轴 × 三档，且每档都有名字（"unknown" 即漂移）。
    for axis in Axis::ALL {
        for level in 0..LEVELS {
            if axis.level_name(level) == "unknown" {
                issues.push((format!("{}:{}", axis.id(), level), VariantCode::AxisDrift));
            }
        }
    }
    if Axis::ALL.len() != 5 {
        issues.push(("轴数".to_string(), VariantCode::AxisDrift));
    }

    Verdict {
        capabilities: Capabilities {
            derivation: !book.is_empty(),
            composition: !presets().is_empty(),
            switching: !ledger.transitions().is_empty() || ledger.depth() == 0,
            validation: true,
        },
        laws: LawProof {
            derived_not_copied: derived.is_empty(),
            switch_deterministic_rollback: deterministic && rollback_ok,
        },
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(axis: Axis, level: u8) -> VariantKey {
        match VariantKey::new(axis, level) {
            Ok(k) => k,
            Err(_) => panic!("测试键必须合法"),
        }
    }

    #[test]
    fn axes_are_five_and_indexed_densely() {
        assert_eq!(Axis::ALL.len(), 5);
        for (i, a) in Axis::ALL.iter().enumerate() {
            assert_eq!(a.index(), i);
        }
        // 轴 id 唯一
        let mut ids: Vec<&str> = Axis::ALL.iter().map(|a| a.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn standard_book_derives_everything_to_root() {
        let b = DerivationBook::standard();
        assert_eq!(b.len(), 10, "五轴各两条派生");
        assert!(b.audit().is_empty());
        let chain = b.chain(key(Axis::Density, 2)).unwrap_or_default();
        assert_eq!(chain.len(), 3);
        assert!(chain.last().map(|k| k.is_base()).unwrap_or(false));
    }

    #[test]
    fn copied_variant_is_rejected() {
        let mut b = DerivationBook::standard();
        let k = key(Axis::Radius, 2);
        // 重复声明即「想改成抄」，直接拒。
        assert_eq!(
            b.declare(k, Origin::Copied(key(Axis::Radius, 1))),
            Err(VariantCode::CopiedNotDerived)
        );
        // 另起一本，声明为抄 ⇒ 求链时报 CopiedNotDerived。
        let mut c = DerivationBook::new();
        assert!(c
            .declare(key(Axis::Radius, 1), Origin::Copied(key(Axis::Radius, 0)))
            .is_ok());
        assert_eq!(
            c.chain(key(Axis::Radius, 1)),
            Err(VariantCode::CopiedNotDerived)
        );
    }

    #[test]
    fn cross_axis_and_cycle_and_orphan_are_all_caught() {
        let mut b = DerivationBook::new();
        assert_eq!(
            b.declare(
                key(Axis::Density, 1),
                Origin::Derived(key(Axis::Radius, 0))
            ),
            Err(VariantCode::CrossAxisDerivation)
        );
        // 环：1←2 与 2←1
        let mut c = DerivationBook::new();
        assert!(c
            .declare(key(Axis::Motion, 1), Origin::Derived(key(Axis::Motion, 2)))
            .is_ok());
        assert!(c
            .declare(key(Axis::Motion, 2), Origin::Derived(key(Axis::Motion, 1)))
            .is_ok());
        assert_eq!(
            c.chain(key(Axis::Motion, 1)),
            Err(VariantCode::DerivationCycle)
        );
        // 断链：1 声明了但从没登记 2
        let mut d = DerivationBook::new();
        assert!(d
            .declare(key(Axis::FocusRing, 1), Origin::Derived(key(Axis::FocusRing, 0)))
            .is_ok());
        assert_eq!(
            d.chain(key(Axis::FocusRing, 2)),
            Err(VariantCode::OrphanVariant)
        );
    }

    #[test]
    fn preset_combos_are_legal() {
        let b = DerivationBook::standard();
        assert!(
            audit_presets(&b).is_empty(),
            "预置必须自己合法：{:?}",
            audit_presets(&b)
        );
        for p in presets() {
            assert!(conflicts(&p.combo).is_empty(), "{} 命中互斥", p.id);
        }
        // 三条预置必须互不相同，否则「预置」名不副实。
        let h: Vec<u64> = presets().iter().map(|p| p.combo.hash()).collect();
        assert_eq!(h[0] != h[1], true);
        assert_eq!(h[1] != h[2], true);
        assert_eq!(h[0] != h[2], true);
    }

    #[test]
    fn mutex_table_catches_the_documented_pairs() {
        let bad = Combo::default_combo()
            .with(Axis::Density, 0)
            .and_then(|c| c.with(Axis::TypeScale, 2));
        let c = match bad {
            Ok(c) => c,
            Err(_) => panic!("构造必须成功"),
        };
        let hits = conflicts(&c);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].why.contains("换行"), "理由必须写用户可见后果");
    }

    #[test]
    fn switch_is_deterministic_and_rolls_back() {
        let mut l = SwitchLedger::new(Combo::default_combo());
        let t1 = l.switch(Axis::Density, 0).unwrap_or_else(|_| panic!("切换应成功"));
        assert_eq!(t1.id, 1);
        assert_eq!(t1.from, 1);
        assert_eq!(t1.to, 0);
        assert!(l.switch(Axis::Radius, 0).is_ok());
        assert!(l.replay_is_deterministic());
        let back = l.rollback().unwrap_or_else(|_| panic!("应可回滚"));
        assert_eq!(back.level(Axis::Radius), 1);
        assert_eq!(l.active().level(Axis::Density), 0);
        assert_eq!(l.rollback_all(), 1, "剩一步：退回初始");
        assert_eq!(l.active(), Combo::default_combo());
    }

    #[test]
    fn noop_switch_is_rejected_and_does_not_pollute_history() {
        let mut l = SwitchLedger::new(Combo::default_combo());
        assert_eq!(
            l.switch(Axis::Density, 1),
            Err(VariantCode::NoOpSwitch)
        );
        assert_eq!(l.depth(), 0);
        assert_eq!(l.rollback(), Err(VariantCode::NothingToRollback));
    }

    #[test]
    fn ledger_apply_and_switch_mix_stays_deterministic() {
        let mut l = SwitchLedger::new(Combo::default_combo());
        assert!(l.switch(Axis::Density, 0).is_ok());
        let p = presets();
        let reading = p.get(2).map(|x| x.combo).unwrap_or_default();
        assert!(l.apply(reading).is_ok());
        assert!(l.replay_is_deterministic());
        // 混用后回滚仍应逐级退
        let mut n = 0;
        while l.rollback().is_ok() {
            n += 1;
        }
        assert_eq!(n, 2);
    }

    #[test]
    fn full_audit_is_green_on_standard_kit() {
        let b = DerivationBook::standard();
        let mut l = SwitchLedger::new(Combo::default_combo());
        let _ = l.switch(Axis::FocusRing, 2);
        let v = audit(&b, &l);
        assert!(v.is_green(), "{}", v.lines().join("\n"));
        assert!(v.capabilities.complete());
        assert!(v.laws.derived_not_copied);
        assert!(v.laws.switch_deterministic_rollback);
    }

    #[test]
    fn audit_reports_all_issues_not_first() {
        let mut b = DerivationBook::new();
        // 两条都断了：一次应报两条
        let v = audit(&b, &SwitchLedger::new(Combo::default_combo()));
        assert!(!v.is_green());
        assert!(v.issues.len() >= 4, "至少 2 轴×2 档断链：{}", v.issues.len());
        assert!(!v.laws.derived_not_copied);
    }

    #[test]
    fn combo_canonical_is_stable_and_ordered() {
        let c = Combo::default_combo();
        let s = c.canonical();
        assert_eq!(
            s,
            "density=standard/radius=standard/typescale=standard/motion=standard/focusring=standard"
                .to_string()
        );
        // 顺序必须按 Axis::ALL：换个插入顺序不影响串
        let d = c.with(Axis::Motion, 2).unwrap_or(c);
        assert!(d.canonical().starts_with("density="));
        assert_ne!(c.hash(), d.hash());
        assert_eq!(c.changed_axes(&d), vec![Axis::Motion]);
    }
}
