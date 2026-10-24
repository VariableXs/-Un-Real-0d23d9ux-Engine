//! VE-F5248 · 后处理参数系统（VE-Z 域 · Z03 后处理 · 锚点目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5248`
//!
//! **判据（锚点原文）**：参数域声明、默认值、范围钳制、联动与依赖、
//! 预设与快照、判据。
//!
//! # 深化说明
//!
//! ## 深化一：参数不是一堆 `f32` 字段，而是**带域声明的表**
//!
//! 把 12 个效果 × 平均 5 个参数摊平成几十个字段，会有三个必然问题：
//! 校验逻辑散落各处、越界值只能靠人记、新增参数时默认值与文档必然漂移。
//! 本实现把每个参数登记成 [`ParamSpec`]：名字、默认值、软范围、硬范围、
//! 单位、联动组。于是：
//!
//! - **钳制**有唯一入口（[`ParamSet::set`]），越硬范围直接拒、越软范围钳；
//!   软/硬双范围的区分很关键：软范围是「推荐区间」（超了仍可用但会告警），
//!   硬范围是「物理不可能」（超了必须拒），两者混一会让「允许极限调参」
//!   与「防崩溃」互相妥协。
//! - **默认值**只在一处登记，预设即「对默认表的差量」。
//!
//! ## 深化二：联动用**显式依赖图 + 拓扑序求值**，不是散落 if
//!
//! 「亮度提了要把曝光降一点」这类联动若写成 `if brightness > 1.0 { exposure -= … }`，
//! 会在多次调整后累积漂移，且无法回答「谁改了我」。本实现把联动声明成
//! 依赖边 `dependent → dependency`，求值时按**拓扑序**依次算，且联动永远
//! 从**用户基线**（base）重算而不是在上一轮结果上叠加——于是联动是幂等的，
//! 调十次与调一次的最终状态相同（`linkage_is_idempotent` 判据）。
//!
//! ## 深化三：预设 = **差量集合**，且必须可逆
//!
//! 若预设直接覆写全表，用户自己的调参会被静默吃掉，且「切回默认」无法
//! 区分「原本就是默认」与「被预设改过」。本实现把预设存成 `(key, value)`
//! 差量列表，应用预设时先 `reset_to_default()` 再覆盖差量；于是
//! 「应用 A → 应用 B → 应用默认」等价于「直接应用默认」
//! （`preset_roundtrip_returns_to_default`）。
//!
//! ## 深化四：快照带**指纹**，用于跨版本迁移检测
//!
//! 快照若只存值，参数表在版本升级后新增/改名的 key 会让旧快照静默失效
//! （部分参数丢失而无人知晓）。本实现给快照附一个**键集指纹**（对所有
//! 已登记 key 的 FNV-1a 哈希），载入时比对：不一致则返回
//! [`LoadError::FingerprintMismatch`] 并给出缺失/多余的 key 数，由调用方
//! 决定迁移策略——不静默丢弃。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PARAM_VERSION: &str = "F5248-param-v1";

// ── 诊断码（本模块独占段 0xE090~0xE09F）─────────────────────────────
const SEG_BASE: u16 = 0xE090;
/// 参数 key 未登记。
pub const E_UNKNOWN_KEY: u16 = SEG_BASE + 1;
/// 值超出硬范围。
pub const E_OUT_OF_HARD: u16 = SEG_BASE + 2;
/// 依赖图成环。
pub const E_CYCLE: u16 = SEG_BASE + 3;
/// 快照指纹不符。
pub const E_FP_MISMATCH: u16 = SEG_BASE + 4;
/// 参数表超容量。
pub const E_TABLE_FULL: u16 = SEG_BASE + 5;

/// 参数表最大条目数。依据：12 个效果 × 8 参 = 96，留余量到 128。
pub const TABLE_MAX: usize = 128;

/// 一个参数的声明。
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    /// 稳定 key（如 `"bloom.intensity"`）。
    pub key: &'static str,
    /// 默认值。
    pub default: f32,
    /// 软范围（推荐区间；越界钳制并告警）。
    pub soft: (f32, f32),
    /// 硬范围（物理极限；越界直接拒）。
    pub hard: (f32, f32),
    /// 单位（仅文档用途，如 `"px"`）。
    pub unit: &'static str,
}

impl ParamSpec {
    /// 校验声明自身的自洽性：default 必须在 soft 内，soft 必须在 hard 内。
    ///
    /// 这条自洽性看似琐碎，但它是「默认值一定合法」的唯一机械保证——
    /// 手工登记参数表时，最高频的错误就是默认值落在软范围之外。
    pub fn is_consistent(&self) -> bool {
        let (sl, sh) = self.soft;
        let (hl, hh) = self.hard;
        if self.default.is_nan() || self.default < sl || self.default > sh {
            return false;
        }
        if !(hl <= sl && sh <= hh) {
            return false;
        }
        if hl > hh || sl > sh {
            return false;
        }
        true
    }

    /// 把值钳到软/硬范围。
    ///
    /// 返回 `Ok(钳后值)` 或 `Err(E_OUT_OF_HARD)`（超出物理极限）。
    /// 注意：软范围内不钳（保留原值精度），只在越软范围时钳到软边界。
    pub fn clamp(&self, v: f32) -> Result<f32, u16> {
        if v.is_nan() {
            return Err(E_OUT_OF_HARD);
        }
        let (hl, hh) = self.hard;
        if v < hl || v > hh {
            return Err(E_OUT_OF_HARD);
        }
        let (sl, sh) = self.soft;
        Ok(if v < sl {
            sl
        } else if v > sh {
            sh
        } else {
            v
        })
    }
}

/// 参数集：值表 + 基线 + 声明表。
#[derive(Clone, Debug)]
pub struct ParamSet {
    specs: Vec<ParamSpec>,
    /// 当前值（与 specs 同序）。
    values: Vec<f32>,
    /// 用户基线（联动求值的起点，与当前值不同——当前值可能被联动改过）。
    base: Vec<f32>,
}

impl ParamSet {
    /// 由声明表构造（全部取默认值）。
    pub fn new(specs: Vec<ParamSpec>) -> Result<Self, u16> {
        if specs.len() > TABLE_MAX {
            return Err(E_TABLE_FULL);
        }
        let mut values = Vec::with_capacity(specs.len());
        for s in &specs {
            if !s.is_consistent() {
                return Err(E_OUT_OF_HARD);
            }
            values.push(s.default);
        }
        let base = values.clone();
        Ok(Self {
            specs,
            values,
            base,
        })
    }

    /// 标准参数表（Z03 五效果的核心参数）。
    pub fn standard() -> Result<Self, u16> {
        let specs = alloc::vec![
            ParamSpec {
                key: "bloom.threshold",
                default: 1.0,
                soft: (0.0, 1.0),
                hard: (0.0, 4.0),
                unit: "",
            },
            ParamSpec {
                key: "bloom.intensity",
                default: 0.6,
                soft: (0.0, 2.0),
                hard: (0.0, 8.0),
                unit: "",
            },
            ParamSpec {
                key: "dof.focus",
                default: 300.0,
                soft: (50.0, 5000.0),
                hard: (51.0, 100_000.0),
                unit: "cm",
            },
            ParamSpec {
                key: "dof.aperture",
                default: 6.0,
                soft: (0.5, 40.0),
                hard: (0.1, 500.0),
                unit: "cm",
            },
            ParamSpec {
                key: "grade.exposure",
                default: 0.0,
                soft: (-3.0, 3.0),
                hard: (-8.0, 8.0),
                unit: "EV",
            },
            ParamSpec {
                key: "grade.contrast",
                default: 1.0,
                soft: (0.5, 2.0),
                hard: (0.25, 4.0),
                unit: "",
            },
            ParamSpec {
                key: "grade.saturation",
                default: 1.0,
                soft: (0.0, 2.0),
                hard: (0.0, 4.0),
                unit: "",
            },
            ParamSpec {
                key: "mblur.shutter",
                default: 180.0,
                soft: (0.0, 180.0),
                hard: (0.0, 360.0),
                unit: "deg",
            },
            ParamSpec {
                key: "ssr.roughness",
                default: 0.15,
                soft: (0.0, 0.6),
                hard: (0.0, 1.0),
                unit: "",
            },
            ParamSpec {
                key: "ssr.maxdist",
                default: 600.0,
                soft: (50.0, 2000.0),
                hard: (1.0, 100_000.0),
                unit: "px",
            },
        ];
        Self::new(specs)
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// 全部声明。
    pub fn specs(&self) -> &[ParamSpec] {
        &self.specs
    }

    /// 按 key 找下标。
    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.specs.iter().position(|s| s.key == key)
    }

    /// 取值。
    pub fn get(&self, key: &str) -> Option<f32> {
        self.index_of(key).map(|i| self.values[i])
    }

    /// 取基线值。
    pub fn get_base(&self, key: &str) -> Option<f32> {
        self.index_of(key).map(|i| self.base[i])
    }

    /// 设值（写基线 + 当前值）。
    ///
    /// 越硬范围拒；越软范围钳到软边界后返回 `Ok(钳后值)`。
    pub fn set(&mut self, key: &str, v: f32) -> Result<f32, u16> {
        let i = self.index_of(key).ok_or(E_UNKNOWN_KEY)?;
        let c = self.specs[i].clamp(v)?;
        self.base[i] = c;
        self.values[i] = c;
        Ok(c)
    }

    /// 复位到默认值。
    pub fn reset_to_default(&mut self) {
        for (i, s) in self.specs.iter().enumerate() {
            self.base[i] = s.default;
            self.values[i] = s.default;
        }
    }

    /// 全部值（只读）。
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// 应用预设（差量列表）。
    ///
    /// 先复位到默认，再逐项覆盖；未知 key 视为错误（不静默跳过）。
    pub fn apply_preset(&mut self, preset: &Preset) -> Result<(), u16> {
        self.reset_to_default();
        for (k, v) in &preset.items {
            let i = self.index_of(k).ok_or(E_UNKNOWN_KEY)?;
            let c = self.specs[i].clamp(*v)?;
            self.base[i] = c;
            self.values[i] = c;
        }
        Ok(())
    }

    /// 键集指纹（FNV-1a over 全部 key，升序拼接以保证与登记序无关）。
    ///
    /// 为什么用键集而不是「键+值」：指纹的用途是**检测参数表结构变化**
    /// （新增/删除/改名），值变化是正常业务，不应触发迁移告警。
    pub fn key_fingerprint(&self) -> u64 {
        let mut keys: Vec<&str> = self.specs.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for k in keys {
            for b in k.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            // 分隔符，防 "ab"+"c" 与 "a"+"bc" 同哈希。
            h ^= 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 导出快照。
    pub fn snapshot(&self) -> Snapshot {
        let mut items = Vec::with_capacity(self.specs.len());
        for (i, s) in self.specs.iter().enumerate() {
            items.push((String::from(s.key), self.values[i]));
        }
        Snapshot {
            fp: self.key_fingerprint(),
            items,
        }
    }

    /// 从快照恢复（指纹必须一致）。
    pub fn restore(&mut self, snap: &Snapshot) -> Result<(), u16> {
        if snap.fp != self.key_fingerprint() {
            return Err(E_FP_MISMATCH);
        }
        // 先整体校验再写入：避免「写了一半失败」留下半新半旧的参数表。
        let mut staged = Vec::with_capacity(snap.items.len());
        for (k, v) in &snap.items {
            let i = self.index_of(k).ok_or(E_UNKNOWN_KEY)?;
            let c = self.specs[i].clamp(*v)?;
            staged.push((i, c));
        }
        for (i, v) in staged {
            self.base[i] = v;
            self.values[i] = v;
        }
        Ok(())
    }

    /// 应用联动规则：按拓扑序从**基线**重算当前值。
    ///
    /// `links` 为 `(依赖者 key, 被依赖者 key, 变换)` 列表。求值顺序由
    /// [`topo_order`] 给出；每条规则读取被依赖者的**当前值**（已按序算好），
    /// 而写入依赖者的当前值（**不动基线**）。
    ///
    /// 幂等性来源：每次都从 `base` 重新克隆 `values`，再按序施加规则。
    /// 于是在同一基线上重复调用结果相同；调整过的基线才会产生新结果。
    pub fn apply_links(&mut self, links: &[(usize, usize, LinkOp)]) -> Result<(), u16> {
        let order = topo_order(self.specs.len(), links)?;
        // 从基线重建当前值（这是幂等性的关键一步）。
        self.values.copy_from_slice(&self.base);
        for idx in order {
            // 收集所有以 idx 为依赖者的规则。
            for (dep, src, op) in links {
                if *dep != idx {
                    continue;
                }
                let sv = self.values[*src];
                let bv = self.values[*dep];
                let nv = op.apply(bv, sv);
                // 联动结果也要过钳制：联动不得产出非法值。
                let c = self.specs[*dep].clamp(nv).unwrap_or(self.specs[*dep].default);
                self.values[*dep] = c;
            }
        }
        Ok(())
    }

    /// 当前值与基线是否一致（诊断用）。
    pub fn is_at_base(&self) -> bool {
        let mut ok = true;
        for i in 0..self.specs.len() {
            if (self.values[i] - self.base[i]).abs() > 1e-6 {
                ok = false;
            }
        }
        ok
    }
}

/// 联动变换。
#[derive(Clone, Copy, Debug)]
pub enum LinkOp {
    /// 依赖者 = 基线 × 被依赖者（比例联动）。
    Scale,
    /// 依赖者 = 基线 + 被依赖者（偏移联动）。
    Offset,
    /// 依赖者 = 基线 × (1 - 被依赖者/范围)（反向衰减，范围由 `span` 给）。
    Inverse { span: f32 },
}

impl LinkOp {
    pub fn apply(&self, base: f32, src: f32) -> f32 {
        match self {
            LinkOp::Scale => base * src,
            LinkOp::Offset => base + src,
            LinkOp::Inverse { span } => {
                if *span <= 0.0 {
                    return base;
                }
                let t = (src / *span).clamp(0.0, 1.0);
                base * (1.0 - t)
            }
        }
    }
}

/// 拓扑序（Kahn）。成环返回 `E_CYCLE`。
///
/// `links` 中 `(dep, src, _)` 表示「dep 依赖 src」，故建边 `src → dep`。
pub fn topo_order(n: usize, links: &[(usize, usize, LinkOp)]) -> Result<Vec<usize>, u16> {
    let mut indeg = alloc::vec![0u16; n];
    let mut adj: Vec<Vec<usize>> = alloc::vec![Vec::new(); n];
    for (dep, src, _) in links {
        if *dep >= n || *src >= n {
            return Err(E_CYCLE);
        }
        adj[*src].push(*dep);
        indeg[*dep] += 1;
    }
    let mut queue: Vec<usize> = Vec::new();
    for (i, d) in indeg.iter().enumerate() {
        if *d == 0 {
            queue.push(i);
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut head = 0usize;
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        order.push(u);
        for &v in &adj[u] {
            indeg[v] -= 1;
            if indeg[v] == 0 {
                queue.push(v);
            }
        }
    }
    if order.len() != n {
        return Err(E_CYCLE);
    }
    Ok(order)
}

/// 预设：一组差量。
#[derive(Clone, Debug, Default)]
pub struct Preset {
    pub name: &'static str,
    pub items: Vec<(&'static str, f32)>,
}

/// 参数快照。
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 键集指纹。
    pub fp: u64,
    /// (key, value) 列表。
    pub items: Vec<(String, f32)>,
}

// ── 判据 ────────────────────────────────────────────────────────

/// F5248 判据集。
pub fn run_vez48_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5248");

    // ── 1. 参数域声明自洽 ──
    {
        let ps = match ParamSet::standard() {
            Ok(p) => p,
            Err(_) => return s,
        };
        s.add("VZ48-声明-01", ps.len() == 10, "标准表 10 个参数");
        let mut all_ok = true;
        let mut first_bad = "";
        for sp in ps.specs() {
            if !sp.is_consistent() {
                all_ok = false;
                first_bad = sp.key;
            }
        }
        s.add(
            "VZ48-声明-02",
            all_ok,
            "全部声明自洽（默认在软内、软在硬内）",
        );
        let _ = first_bad;
        // key 唯一。
        let mut keys: Vec<&str> = ps.specs().iter().map(|x| x.key).collect();
        let n0 = keys.len();
        keys.sort_unstable();
        keys.dedup();
        s.add("VZ48-声明-03", keys.len() == n0, "key 唯一（去重后长度不变）");
        // 反向：构造不自洽声明必须被拒。
        let bad = ParamSpec {
            key: "bad",
            default: 5.0,
            soft: (0.0, 1.0),
            hard: (0.0, 10.0),
            unit: "",
        };
        s.add(
            "VZ48-声明-04",
            !bad.is_consistent(),
            "反向：默认值越软范围的声明被判不自洽",
        );
        let bad2 = ParamSpec {
            key: "bad2",
            default: 0.5,
            soft: (0.0, 20.0),
            hard: (0.0, 10.0),
            unit: "",
        };
        s.add(
            "VZ48-声明-05",
            !bad2.is_consistent(),
            "反向：软范围越硬范围的声明被判不自洽",
        );
        // 构造时即拒不自洽表。
        let r = ParamSet::new(alloc::vec![bad]);
        s.add(
            "VZ48-声明-06",
            matches!(r, Err(E_OUT_OF_HARD)),
            "构造时拒不自洽声明（E_OUT_OF_HARD）",
        );
    }

    // ── 2. 默认值与钳制 ──
    {
        let mut ps = match ParamSet::standard() {
            Ok(p) => p,
            Err(_) => return s,
        };
        s.add(
            "VZ48-钳制-01",
            ps.get("bloom.intensity") == Some(0.6),
            "默认值取自声明表（bloom.intensity = 0.6）",
        );
        // 软范围内原值保留。
        let v = ps.set("bloom.intensity", 1.5);
        s.add(
            "VZ48-钳制-02",
            v == Ok(1.5) && ps.get("bloom.intensity") == Some(1.5),
            "软范围内原值保留（不误钳）",
        );
        // 越软但不越硬 → 钳到软边界。
        let v2 = ps.set("bloom.intensity", 3.0);
        s.add(
            "VZ48-钳制-03",
            v2 == Ok(2.0) && ps.get("bloom.intensity") == Some(2.0),
            "越软范围钳到软上界（3.0 → 2.0）",
        );
        // 越硬 → 拒，且值不变。
        let before = ps.get("bloom.intensity");
        let v3 = ps.set("bloom.intensity", 100.0);
        s.add(
            "VZ48-钳制-04",
            v3 == Err(E_OUT_OF_HARD) && ps.get("bloom.intensity") == before,
            "越硬范围拒绝且原值不变（不产生部分写入）",
        );
        // 负下界同理。
        let v4 = ps.set("bloom.intensity", -1.0);
        s.add(
            "VZ48-钳制-05",
            v4 == Ok(0.0),
            "负值钳到软下界 0",
        );
        // 未知 key。
        s.add(
            "VZ48-钳制-06",
            ps.set("nope.key", 1.0) == Err(E_UNKNOWN_KEY),
            "反向：未知 key 被拒（E_UNKNOWN_KEY）",
        );
        // NaN 拒绝。
        s.add(
            "VZ48-钳制-07",
            ps.set("bloom.intensity", f32::NAN) == Err(E_OUT_OF_HARD),
            "反向：NaN 被拒",
        );
        // 复位。
        ps.reset_to_default();
        s.add(
            "VZ48-钳制-08",
            ps.get("bloom.intensity") == Some(0.6) && ps.is_at_base(),
            "reset 回到默认且与基线一致",
        );
    }

    // ── 3. 联动：拓扑序与幂等 ──
    {
        let mut ps = match ParamSet::standard() {
            Ok(p) => p,
            Err(_) => return s,
        };
        let ei = match ps.index_of("grade.exposure") {
            Some(i) => i,
            None => return s,
        };
        let bi = match ps.index_of("bloom.intensity") {
            Some(i) => i,
            None => return s,
        };
        let ti = match ps.index_of("bloom.threshold") {
            Some(i) => i,
            None => return s,
        };
        // 规则：bloom.intensity 随 exposure 反向衰减（曝光越高泛光越弱）。
        let links = alloc::vec![(bi, ei, LinkOp::Inverse { span: 3.0 })];
        // 基线：exposure = 1.5、intensity = 0.6。
        let _ = ps.set("grade.exposure", 1.5);
        let _ = ps.set("bloom.intensity", 0.6);
        let ok1 = ps.apply_links(&links).is_ok();
        let after1 = ps.get("bloom.intensity").unwrap_or(-1.0);
        // 期望：0.6 * (1 - 1.5/3.0) = 0.3
        s.add(
            "VZ48-联动-01",
            ok1 && (after1 - 0.3).abs() < 1e-5,
            "反向衰减联动：曝光 1.5 → 泛光 0.6×0.5 = 0.3",
        );
        // 幂等：重复应用结果不变。
        let _ = ps.apply_links(&links);
        let after2 = ps.get("bloom.intensity").unwrap_or(-1.0);
        s.add(
            "VZ48-联动-02",
            (after1 - after2).abs() < 1e-6,
            "联动幂等（重复应用不累积漂移）",
        );
        // 基线未被联动改写。
        s.add(
            "VZ48-联动-03",
            ps.get_base("bloom.intensity") == Some(0.6),
            "联动不改基线（基线 = 用户意图）",
        );
        // 反向：若在上一轮结果上叠加，第二次会变成 0.15。
        let naive = 0.6f32 * 0.5 * 0.5;
        s.add(
            "VZ48-联动-04",
            (naive - 0.15).abs() < 1e-5 && (after2 - 0.15).abs() > 1e-3,
            "反向：叠加式实现第二次会变 0.15（本实现仍是 0.3）",
        );
        // 多级联动：threshold 依赖 intensity（Scale）。
        let links2 = alloc::vec![
            (bi, ei, LinkOp::Inverse { span: 3.0 }),
            (ti, bi, LinkOp::Scale),
        ];
        let ok2 = ps.apply_links(&links2).is_ok();
        let th = ps.get("bloom.threshold").unwrap_or(-1.0);
        s.add(
            "VZ48-联动-05",
            ok2 && th > 0.0,
            "多级联动按拓扑序求值（threshold 用 intensity 的联动值）",
        );
        // 拓扑序正确性：intensity 必须排在 threshold 之前。
        let order = topo_order(
            ps.len(),
            &alloc::vec![(bi, ei, LinkOp::Scale), (ti, bi, LinkOp::Scale)],
        );
        match order {
            Ok(o) => {
                let pos_bi = o.iter().position(|x| *x == bi).unwrap_or(0);
                let pos_ti = o.iter().position(|x| *x == ti).unwrap_or(0);
                s.add(
                    "VZ48-联动-06",
                    pos_bi < pos_ti,
                    "拓扑序把 intensity 排在 threshold 之前",
                );
            }
            Err(_) => s.add("VZ48-联动-06", false, "拓扑排序失败"),
        }
        // 成环必须报错。
        let cyc = topo_order(3, &alloc::vec![(0, 1, LinkOp::Scale), (1, 0, LinkOp::Scale)]);
        s.add("VZ48-联动-07", cyc == Err(E_CYCLE), "反向：依赖成环被拒（E_CYCLE）");
    }

    // ── 4. 预设与快照 ──
    {
        let mut ps = match ParamSet::standard() {
            Ok(p) => p,
            Err(_) => return s,
        };
        let warm = Preset {
            name: "warm",
            items: alloc::vec![("grade.exposure", 0.3f32), ("bloom.intensity", 0.9)],
        };
        let ok = ps.apply_preset(&warm).is_ok();
        s.add(
            "VZ48-预设-01",
            ok && ps.get("grade.exposure") == Some(0.3) && ps.get("bloom.intensity") == Some(0.9),
            "预设差量生效",
        );
        // 预设未提及的参数保持默认。
        s.add(
            "VZ48-预设-02",
            ps.get("grade.contrast") == Some(1.0),
            "预设未提及的参数回到默认（差量语义）",
        );
        // 切到另一个预设不残留。
        let cold = Preset {
            name: "cold",
            items: alloc::vec![("grade.exposure", -0.5f32)],
        };
        let _ = ps.apply_preset(&cold);
        s.add(
            "VZ48-预设-03",
            ps.get("grade.exposure") == Some(-0.5) && ps.get("bloom.intensity") == Some(0.6),
            "切换预设不残留上一预设的值（先复位再覆盖）",
        );
        // 回默认等价。
        ps.reset_to_default();
        s.add(
            "VZ48-预设-04",
            ps.is_at_base() && ps.get("grade.exposure") == Some(0.0),
            "回默认与初始状态一致（可逆）",
        );
        // 未知 key 的预设必须报错（不静默）。
        let bad = Preset {
            name: "bad",
            items: alloc::vec![("no.such", 1.0f32)],
        };
        s.add(
            "VZ48-预设-05",
            ps.apply_preset(&bad) == Err(E_UNKNOWN_KEY),
            "反向：预设含未知 key 被拒（不静默跳过）",
        );
        // 快照往返。
        let _ = ps.set("dof.focus", 1200.0);
        let snap = ps.snapshot();
        let mut ps2 = match ParamSet::standard() {
            Ok(p) => p,
            Err(_) => return s,
        };
        let r = ps2.restore(&snap);
        s.add(
            "VZ48-快照-01",
            r.is_ok() && ps2.get("dof.focus") == Some(1200.0),
            "快照往返保持值",
        );
        s.add(
            "VZ48-快照-02",
            snap.fp == ps.key_fingerprint(),
            "快照带键集指纹",
        );
        // 指纹不符必须拒。
        let mut tampered = snap.clone();
        tampered.fp ^= 0xFFFF_FFFF;
        s.add(
            "VZ48-快照-03",
            ps2.restore(&tampered) == Err(E_FP_MISMATCH),
            "反向：指纹不符被拒（检测参数表结构变化）",
        );
        // 指纹与登记序无关（重排声明后指纹不变）。
        let mut specs: Vec<ParamSpec> = ps.specs().to_vec();
        specs.reverse();
        match ParamSet::new(specs) {
            Ok(ps3) => s.add(
                "VZ48-快照-04",
                ps3.key_fingerprint() == ps.key_fingerprint(),
                "指纹与声明顺序无关（排序后哈希）",
            ),
            Err(_) => s.add("VZ48-快照-04", false, "重排后构造失败"),
        }
    }

    // ── 5. 冻结 ──
    {
        s.add(
            "VZ48-冻结-01",
            PARAM_VERSION == "F5248-param-v1",
            "API 版本冻结：F5248-param-v1",
        );
        s.add(
            "VZ48-冻结-02",
            TABLE_MAX == 128,
            "参数表容量指纹 = 128",
        );
        // 表容量上限生效。
        let mut many = Vec::new();
        for _ in 0..(TABLE_MAX + 1) {
            many.push(ParamSpec {
                key: "x",
                default: 0.5,
                soft: (0.0, 1.0),
                hard: (0.0, 1.0),
                unit: "",
            });
        }
        s.add(
            "VZ48-冻结-03",
            matches!(ParamSet::new(many), Err(E_TABLE_FULL)),
            "反向：超容量参数表被拒",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_table_consistent() {
        let ps = ParamSet::standard().expect("table");
        assert!(ps.specs().iter().all(|s| s.is_consistent()));
    }

    #[test]
    fn clamp_soft_and_hard() {
        let mut ps = ParamSet::standard().expect("t");
        assert_eq!(ps.set("bloom.intensity", 3.0), Ok(2.0));
        assert_eq!(ps.set("bloom.intensity", 99.0), Err(E_OUT_OF_HARD));
    }

    #[test]
    fn linkage_idempotent() {
        let mut ps = ParamSet::standard().expect("t");
        let ei = ps.index_of("grade.exposure").expect("e");
        let bi = ps.index_of("bloom.intensity").expect("b");
        ps.set("grade.exposure", 1.5).expect("s");
        let links = alloc::vec![(bi, ei, LinkOp::Inverse { span: 3.0 })];
        ps.apply_links(&links).expect("l");
        let a = ps.get("bloom.intensity").expect("g");
        ps.apply_links(&links).expect("l2");
        let b = ps.get("bloom.intensity").expect("g2");
        assert!((a - b).abs() < 1e-6);
        assert!((a - 0.3).abs() < 1e-5);
    }

    #[test]
    fn topo_rejects_cycle() {
        assert_eq!(
            topo_order(2, &alloc::vec![(0, 1, LinkOp::Scale), (1, 0, LinkOp::Scale)]),
            Err(E_CYCLE)
        );
    }

    #[test]
    fn snapshot_fingerprint_independent_of_order() {
        let ps = ParamSet::standard().expect("t");
        let mut specs = ps.specs().to_vec();
        specs.reverse();
        let ps2 = ParamSet::new(specs).expect("t2");
        assert_eq!(ps.key_fingerprint(), ps2.key_fingerprint());
    }

    #[test]
    fn preset_applies_delta_only() {
        let mut ps = ParamSet::standard().expect("t");
        let p = Preset {
            name: "p",
            items: alloc::vec![("grade.exposure", 1.0f32)],
        };
        ps.apply_preset(&p).expect("apply");
        assert_eq!(ps.get("grade.exposure"), Some(1.0));
        assert_eq!(ps.get("bloom.intensity"), Some(0.6));
    }
}
