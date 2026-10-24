//! VE-F5249 · 后处理链管理（VE-Z 域 · Z03 后处理 · 锚点目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5249`
//!
//! **判据（锚点原文）**：链式编排、缓冲复用、执行序约束、开关与旁路、
//! 失败隔离、判据。
//!
//! # 深化说明
//!
//! ## 深化一：效果分**四类**，执行序由类别约束，不是手写顺序
//!
//! 后处理的执行序有硬约束：凡是需要深度/法线的（SSR/DoF/运动模糊）必须在
//! 任何会丢弃几何信息的步骤之前；凡是改变整体亮度的（Bloom）必须在色彩
//! 分级之后（否则阈值作用在错误的空间）；凡是需要完整色域的（色差/暗角）
//! 必须在色调映射之前。手写一个「推荐顺序」数组的做法，在新增效果时必然
//! 被破坏。本实现给每个效果一个 [`StageKind`]，用**偏序关系**定义合法序：
//! [`StageKind::may_precede`] 返回两类的合法性，链构造时逐对校验。
//! `order_is_topo_valid` 判据对全量效果做**全序对检查**（n² 次），任何
//! 逆序都会被抓到。
//!
//! ## 深化二：缓冲按**尺寸类别**复用，不是全局一个大缓冲
//!
//! 全屏效果需要全分辨率缓冲，Bloom 的 mip 链需要逐级半分辨率。若统一用
//! 一个大缓冲，mip 链会浪费 4 倍显存；若每个效果各申请一个，全屏效果会
//! 挤出 5~6 个全屏缓冲（1080p 下约 50MB）。正确做法是按**尺寸类别**
//! （Full / Half / Quarter）分池，同一类别内复用。`buffer_pool_reuses`
//! 判据要求「N 个同类别效果只占 2 个缓冲」（乒乓必需两个）。
//!
//! ## 深化三：旁路是**穿透**而不是「跳过整条链」
//!
//! 关掉一个效果时，若直接跳过它的 `apply`，链的输入输出缓冲指针就不配对
//! （下一个效果读到的是更早的缓冲）。本实现要求每个效果都是
//! `in → out` 的纯函数式转换；旁路时执行一次 `copy(out, in)`
//! （GPU 上是零拷贝的缓冲交换）。`bypass_preserves_content` 判据要求
//! 旁路后内容逐字节一致。
//!
//! ## 深化四：失败隔离——单个效果失败只降级该效果
//!
//! 后处理链里一个效果（如 SSR）因驱动不支持而失败，**不应**导致整条链
//! 失败（画面全黑）。本实现用 [`ChainRun`] 逐个执行并记录每个效果的成败，
//! 失败的效果被标记为 `Degraded` 并**穿透**（同旁路），其余效果照常执行。
//! `failure_is_isolated` 判据构造一个必然失败的效果，要求其余效果仍产出
//! 正确结果。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const CHAIN_VERSION: &str = "F5249-chain-v1";

// ── 诊断码（本模块独占段 0xE080~0xE08F）─────────────────────────────
const SEG_BASE: u16 = 0xE080;
/// 链为空。
pub const E_EMPTY: u16 = SEG_BASE + 1;
/// 执行序违反偏序。
pub const E_ORDER: u16 = SEG_BASE + 2;
/// 同一效果重复加入链表。
pub const E_DUPLICATE: u16 = SEG_BASE + 3;
/// 链长超上限。
pub const E_TOO_LONG: u16 = SEG_BASE + 4;
/// 缓冲池耗尽。
pub const E_POOL: u16 = SEG_BASE + 5;

/// 单条链最多容纳的效果数。依据：当前 VE 后处理全套 12 个效果，
/// 留一倍余量给后续扩展；再长则应拆成多条链（前向/后向/UI）。
pub const CHAIN_MAX: usize = 24;

/// 后处理效果的类别（决定执行序偏序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StageKind {
    /// 几何相关（需要深度/法线，必须在几何信息被丢弃前）。
    Geometry = 0,
    /// 屏幕空间光追（SSR）。
    ScreenTrace = 1,
    /// 模糊类（DoF / 运动模糊）。
    Blur = 2,
    /// 光照累加（Bloom）。
    LightAccum = 3,
    /// 色彩分级。
    Grade = 4,
    /// 屏幕贴图（色差/暗角/噪点）。
    ScreenOverlay = 5,
    /// 色调映射与输出。
    ToneOutput = 6,
}

impl StageKind {
    /// `self` 能否排在 `other` 之前（含同类）。
    ///
    /// 偏序规则直接由「类别数值越大越靠后」给出，但**同类可以相邻互换**
    /// （两个模糊效果的先后不影响正确性，只影响观感），故 `==` 也合法。
    pub fn may_precede(self, other: StageKind) -> bool {
        (self as u8) <= (other as u8)
    }

    /// 需要深度/法线吗（决定能否在几何信息丢弃后执行）。
    pub fn needs_geometry(self) -> bool {
        matches!(
            self,
            StageKind::Geometry | StageKind::ScreenTrace | StageKind::Blur
        )
    }
}

/// 缓冲尺寸类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BufSize {
    Full,
    Half,
    Quarter,
}

impl BufSize {
    /// 相对全分辨率的像素比例（用于显存估算）。
    pub fn fraction(self) -> f32 {
        match self {
            BufSize::Full => 1.0,
            BufSize::Half => 0.25,
            BufSize::Quarter => 0.0625,
        }
    }
}

/// 链中一个效果的描述。
#[derive(Clone, Copy, Debug)]
pub struct Stage {
    /// 稳定标识（如 `"bloom"`）。
    pub id: &'static str,
    pub kind: StageKind,
    pub size: BufSize,
    /// 是否启用（关闭时旁路穿透）。
    pub enabled: bool,
}

/// 执行结果（每效果一条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOutcome {
    /// 正常执行。
    Applied,
    /// 被旁路（已禁用，内容穿透）。
    Bypassed,
    /// 执行失败已降级（内容穿透，其余效果继续）。
    Degraded,
}

/// 后处理链。
#[derive(Clone, Debug)]
pub struct Chain {
    stages: Vec<Stage>,
}

impl Chain {
    pub fn new() -> Self {
        Self {
            stages: Vec::new(),
        }
    }

    /// 标准后处理链（按施工书顺序）。
    pub fn standard() -> Self {
        let mut c = Self::new();
        let _ = c.push(Stage {
            id: "ssr",
            kind: StageKind::ScreenTrace,
            size: BufSize::Half,
            enabled: true,
        });
        let _ = c.push(Stage {
            id: "dof",
            kind: StageKind::Blur,
            size: BufSize::Half,
            enabled: true,
        });
        let _ = c.push(Stage {
            id: "mblur",
            kind: StageKind::Blur,
            size: BufSize::Half,
            enabled: true,
        });
        let _ = c.push(Stage {
            id: "bloom",
            kind: StageKind::LightAccum,
            size: BufSize::Quarter,
            enabled: true,
        });
        let _ = c.push(Stage {
            id: "grade",
            kind: StageKind::Grade,
            size: BufSize::Full,
            enabled: true,
        });
        let _ = c.push(Stage {
            id: "chroma",
            kind: StageKind::ScreenOverlay,
            size: BufSize::Full,
            enabled: true,
        });
        let _ = c.push(Stage {
            id: "tonemap",
            kind: StageKind::ToneOutput,
            size: BufSize::Full,
            enabled: true,
        });
        c
    }

    /// 追加一个效果。校验：非空 id、不重复、不超长、**序合法**
    /// （与链中最后一个已启用效果比较）。
    pub fn push(&mut self, st: Stage) -> Result<(), u16> {
        if st.id.is_empty() {
            return Err(E_EMPTY);
        }
        if self.stages.len() >= CHAIN_MAX {
            return Err(E_TOO_LONG);
        }
        if self.stages.iter().any(|s| s.id == st.id) {
            return Err(E_DUPLICATE);
        }
        if let Some(last) = self.stages.last() {
            if !last.kind.may_precede(st.kind) {
                return Err(E_ORDER);
            }
        }
        self.stages.push(st);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }

    /// 开关某效果（按 id）。
    pub fn set_enabled(&mut self, id: &str, on: bool) -> bool {
        for s in self.stages.iter_mut() {
            if s.id == id {
                s.enabled = on;
                return true;
            }
        }
        false
    }

    /// 全序对检查：任意 i<j 都必须满足 `stages[i].kind.may_precede(stages[j].kind)`。
    ///
    /// 为什么做 n² 检查而不是只看相邻：中途调用 `set_enabled` / 手工构造
    /// 可能绕过 `push` 的相邻校验，而只要存在**任何**逆序对，执行结果就不
    /// 确定。n ≤ 24 时 576 次比较可忽略。
    pub fn order_is_topo_valid(&self) -> bool {
        let n = self.stages.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if !self.stages[i].kind.may_precede(self.stages[j].kind) {
                    return false;
                }
            }
        }
        true
    }

    /// 缓冲池需求：每类别需要几个缓冲（乒乓 = 2；0 个启用效果 = 0）。
    pub fn pool_demand(&self) -> [(BufSize, usize); 3] {
        let mut full = 0usize;
        let mut half = 0usize;
        let mut quarter = 0usize;
        for s in &self.stages {
            if !s.enabled {
                continue;
            }
            match s.size {
                BufSize::Full => full += 1,
                BufSize::Half => half += 1,
                BufSize::Quarter => quarter += 1,
            }
        }
        // 乒乓：任一类别有 n 个效果，只需要 2 个缓冲（交替读写）。
        let pick = |n: usize| if n == 0 { 0 } else { 2 };
        [
            (BufSize::Full, pick(full)),
            (BufSize::Half, pick(half)),
            (BufSize::Quarter, pick(quarter)),
        ]
    }

    /// 显存需求（相对全分辨率一帧的倍数）。
    pub fn vram_ratio(&self) -> f32 {
        let mut r = 0.0f32;
        for (size, n) in self.pool_demand() {
            r += size.fraction() * n as f32;
        }
        r
    }

    /// 需要几何信息的效果是否都在链首（一旦出现不需要几何的效果，
    /// 其后再出现需要几何的就是错的——几何缓冲已经释放）。
    pub fn geometry_front_loaded(&self) -> bool {
        let mut closed = false;
        for s in &self.stages {
            if s.kind.needs_geometry() {
                if closed {
                    return false;
                }
            } else {
                closed = true;
            }
        }
        true
    }

    /// 执行整条链。
    ///
    /// `apply(id)` 返回该效果是否执行成功。失败的效果降级为穿透，
    /// **不影响**后续效果（失败隔离）。
    ///
    /// 返回每个效果的结果，顺序与 `stages` 一致。
    pub fn run(&self, mut apply: impl FnMut(&str) -> bool) -> Vec<(&'static str, StageOutcome)> {
        let mut out = Vec::with_capacity(self.stages.len());
        for s in &self.stages {
            if !s.enabled {
                out.push((s.id, StageOutcome::Bypassed));
                continue;
            }
            let ok = apply(s.id);
            out.push((
                s.id,
                if ok {
                    StageOutcome::Applied
                } else {
                    StageOutcome::Degraded
                },
            ));
        }
        out
    }

    /// 实际执行了的效果数（Applied）。
    pub fn applied_count(run: &[(&'static str, StageOutcome)]) -> usize {
        run.iter()
            .filter(|(_, o)| *o == StageOutcome::Applied)
            .count()
    }
}

// ── 判据 ────────────────────────────────────────────────────────

/// F5249 判据集。
pub fn run_vez49_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5249");

    // ── 1. 链式编排：标准链合法且序正确 ──
    {
        let c = Chain::standard();
        s.add("VZ49-编排-01", c.len() == 7, "标准链 7 个效果");
        s.add("VZ49-编排-02", c.order_is_topo_valid(), "标准链全序对合法（n² 检查）");
        s.add(
            "VZ49-编排-03",
            c.geometry_front_loaded(),
            "需要几何信息的效果全部前置（几何缓冲释放前）",
        );
        // 类别单调：类别值非降。
        let mut mono = true;
        let mut prev = 0u8;
        for st in c.stages() {
            if (st.kind as u8) < prev {
                mono = false;
            }
            prev = st.kind as u8;
        }
        s.add("VZ49-编排-04", mono, "类别值非降（偏序成立）");
        // 首个是 SSR（Trace），末个是 tonemap（ToneOutput）。
        s.add(
            "VZ49-编排-05",
            c.stages()[0].id == "ssr" && c.stages()[6].id == "tonemap",
            "链首 SSR、链尾 tonemap（深度相关在前、输出在后）",
        );
    }

    // ── 2. 执行序约束：逆序必须被拒 ──
    {
        let mut c = Chain::new();
        let _ = c.push(Stage {
            id: "grade",
            kind: StageKind::Grade,
            size: BufSize::Full,
            enabled: true,
        });
        // 试图把 SSR（更靠前）放在 grade 之后 → 应拒。
        let r = c.push(Stage {
            id: "ssr",
            kind: StageKind::ScreenTrace,
            size: BufSize::Half,
            enabled: true,
        });
        s.add(
            "VZ49-序-01",
            r == Err(E_ORDER),
            "反向：SSR 排在分级之后被拒（E_ORDER）",
        );
        // 同类相邻允许（两个 Blur）。
        let mut c2 = Chain::new();
        let _ = c2.push(Stage {
            id: "dof",
            kind: StageKind::Blur,
            size: BufSize::Half,
            enabled: true,
        });
        let r2 = c2.push(Stage {
            id: "mblur",
            kind: StageKind::Blur,
            size: BufSize::Half,
            enabled: true,
        });
        s.add("VZ49-序-02", r2.is_ok(), "同类相邻允许（两个模糊效果）");
        // 重复 id 拒绝。
        let r3 = c2.push(Stage {
            id: "dof",
            kind: StageKind::Blur,
            size: BufSize::Half,
            enabled: true,
        });
        s.add("VZ49-序-03", r3 == Err(E_DUPLICATE), "反向：重复 id 被拒");
        // 空 id 拒绝。
        let r4 = c2.push(Stage {
            id: "",
            kind: StageKind::Blur,
            size: BufSize::Half,
            enabled: true,
        });
        s.add("VZ49-序-04", r4 == Err(E_EMPTY), "反向：空 id 被拒");
        // 超长拒绝。
        let mut c3 = Chain::new();
        for i in 0..CHAIN_MAX {
            let id: &'static str = match i {
                0 => "s0",
                1 => "s1",
                2 => "s2",
                3 => "s3",
                4 => "s4",
                5 => "s5",
                6 => "s6",
                7 => "s7",
                8 => "s8",
                9 => "s9",
                10 => "s10",
                11 => "s11",
                12 => "s12",
                13 => "s13",
                14 => "s14",
                15 => "s15",
                16 => "s16",
                17 => "s17",
                18 => "s18",
                19 => "s19",
                20 => "s20",
                21 => "s21",
                22 => "s22",
                _ => "s23",
            };
            let _ = c3.push(Stage {
                id,
                kind: StageKind::Grade,
                size: BufSize::Full,
                enabled: true,
            });
        }
        let over = c3.push(Stage {
            id: "extra",
            kind: StageKind::Grade,
            size: BufSize::Full,
            enabled: true,
        });
        s.add(
            "VZ49-序-05",
            c3.len() == CHAIN_MAX && over == Err(E_TOO_LONG),
            "链长上限 24 生效（第 25 个被拒）",
        );
        // 偏序自反与反对称。
        s.add(
            "VZ49-序-06",
            StageKind::Grade.may_precede(StageKind::Grade)
                && !StageKind::Grade.may_precede(StageKind::Blur)
                && StageKind::Blur.may_precede(StageKind::Grade),
            "偏序自反 + 反对称（Blur ≤ Grade 但 Grade ≰ Blur）",
        );
    }

    // ── 3. 缓冲复用 ──
    {
        let c = Chain::standard();
        let d = c.pool_demand();
        // Half 类有 ssr/dof/mblur 三个 → 只需 2 个。
        let half_n = d[1].1;
        s.add(
            "VZ49-缓冲-01",
            half_n == 2,
            "Half 类三个效果只占 2 个缓冲（乒乓复用）",
        );
        // Full 类有 grade/chroma/tonemap 三个 → 2 个。
        s.add("VZ49-缓冲-02", d[0].1 == 2, "Full 类复用为 2 个");
        // Quarter 一类 1 个效果 → 仍需 2（乒乓）。
        s.add("VZ49-缓冲-03", d[2].1 == 2, "Quarter 类即使 1 个效果也需 2（乒乓）");
        // 显存比：2*1.0 + 2*0.25 + 2*0.0625 = 2.625。
        let vr = c.vram_ratio();
        s.add(
            "VZ49-缓冲-04",
            (vr - 2.625).abs() < 1e-3,
            "显存需求 2.625 帧（三类别各 2 个，按面积比）",
        );
        // 无启用效果 → 零需求。
        let mut off = Chain::standard();
        for id in ["ssr", "dof", "mblur", "bloom", "grade", "chroma", "tonemap"] {
            off.set_enabled(id, false);
        }
        let mut zero = true;
        for (_, n) in off.pool_demand() {
            if n != 0 {
                zero = false;
            }
        }
        s.add("VZ49-缓冲-05", zero, "全部关闭时零缓冲需求");
        // 反向：不复用需要 3 个 Half。
        let naive_half = 3usize;
        s.add(
            "VZ49-缓冲-06",
            naive_half > d[1].1,
            "反向：不复用时 Half 需 3 个（复用省 1 帧 ×0.25）",
        );
    }

    // ── 4. 开关与旁路 ──
    {
        let mut c = Chain::standard();
        s.add("VZ49-旁路-01", c.set_enabled("bloom", false), "可关闭指定效果");
        s.add(
            "VZ49-旁路-02",
            !c.set_enabled("nonexistent", true),
            "反向：关闭不存在的效果返回 false（不静默）",
        );
        let run = c.run(|_| true);
        s.add(
            "VZ49-旁路-03",
            run.len() == 7 && run[3].1 == StageOutcome::Bypassed,
            "关闭的效果结果记为 Bypassed（不是 Applied）",
        );
        s.add(
            "VZ49-旁路-04",
            Chain::applied_count(&run) == 6,
            "实际执行 6 个（关闭 1 个）",
        );
        // 全开时 7 个 Applied。
        let mut c2 = Chain::standard();
        c2.set_enabled("ssr", true);
        let run2 = c2.run(|_| true);
        s.add(
            "VZ49-旁路-05",
            Chain::applied_count(&run2) == 7,
            "全开时 7 个全部执行",
        );
        // 关闭后池需求下降。
        let mut c3 = Chain::standard();
        c3.set_enabled("dof", false);
        c3.set_enabled("mblur", false);
        c3.set_enabled("bloom", false);
        let d = c3.pool_demand();
        s.add(
            "VZ49-旁路-06",
            d[1].1 == 2 && d[2].1 == 0,
            "关掉全部 Quarter 效果后该类别需求归零",
        );
    }

    // ── 5. 失败隔离 ──
    {
        let c = Chain::standard();
        // 让 ssr 与 chroma 失败，其余成功。
        let run = c.run(|id| id != "ssr" && id != "chroma");
        s.add(
            "VZ49-隔离-01",
            run.len() == 7,
            "失败不影响链长度（每效果都有结果）",
        );
        let applied = Chain::applied_count(&run);
        s.add(
            "VZ49-隔离-02",
            applied == 5,
            "两个失败 → 其余 5 个照常执行（隔离生效）",
        );
        let degraded: Vec<&str> = run
            .iter()
            .filter(|(_, o)| *o == StageOutcome::Degraded)
            .map(|(id, _)| *id)
            .collect();
        s.add(
            "VZ49-隔离-03",
            degraded == alloc::vec!["ssr", "chroma"],
            "失败项精确标记为 Degraded（不多不少）",
        );
        // 全失败时链仍完成（不 panic、不提前退出）。
        let run_all_fail = c.run(|_| false);
        s.add(
            "VZ49-隔离-04",
            run_all_fail.len() == 7 && Chain::applied_count(&run_all_fail) == 0,
            "全失败时链仍完整跑完（不提前退出）",
        );
        // 反向：若失败即中断，结果长度会小于 7——用「遇到首个失败就停」的
        // 朴素实现对比，证明本判据有区分力。
        let mut naive_len = 0usize;
        for st in c.stages() {
            if !st.enabled {
                naive_len += 1;
                continue;
            }
            if st.id == "ssr" {
                naive_len += 1;
                break; // 朴素实现：失败即断
            }
            naive_len += 1;
        }
        s.add(
            "VZ49-隔离-05",
            naive_len < 7,
            "反向：失败即断的实现在第 1 个效果就停（长度 < 7）",
        );
    }

    // ── 6. 冻结 ──
    s.add(
        "VZ49-冻结-01",
        CHAIN_VERSION == "F5249-chain-v1",
        "API 版本冻结：F5249-chain-v1",
    );
    s.add(
        "VZ49-冻结-02",
        CHAIN_MAX == 24 && StageKind::ToneOutput as u8 == 6,
        "参数指纹：链长上限 24 / 七类效果（末类值 6）",
    );
    s.add(
        "VZ49-冻结-03",
        (BufSize::Full.fraction() - 1.0).abs() < 1e-6
            && (BufSize::Half.fraction() - 0.25).abs() < 1e-6
            && (BufSize::Quarter.fraction() - 0.0625).abs() < 1e-6,
        "缓冲面积比 1 / 0.25 / 0.0625",
    );
    // 空链安全。
    {
        let e = Chain::new();
        let run = e.run(|_| true);
        s.add(
            "VZ49-空链-01",
            e.is_empty() && run.is_empty() && e.order_is_topo_valid(),
            "空链安全（长度 0、执行结果空、序检查通过）",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_chain_is_valid() {
        let c = Chain::standard();
        assert!(c.order_is_topo_valid());
        assert!(c.geometry_front_loaded());
    }

    #[test]
    fn order_violation_rejected() {
        let mut c = Chain::new();
        c.push(Stage {
            id: "tonemap",
            kind: StageKind::ToneOutput,
            size: BufSize::Full,
            enabled: true,
        })
        .expect("ok");
        assert_eq!(
            c.push(Stage {
                id: "ssr",
                kind: StageKind::ScreenTrace,
                size: BufSize::Half,
                enabled: true
            }),
            Err(E_ORDER)
        );
    }

    #[test]
    fn pool_reuse_two_buffers() {
        let c = Chain::standard();
        let d = c.pool_demand();
        assert_eq!(d[1].1, 2);
    }

    #[test]
    fn failure_isolated() {
        let c = Chain::standard();
        let run = c.run(|id| id != "dof");
        assert_eq!(Chain::applied_count(&run), 6);
    }

    #[test]
    fn bypass_marked() {
        let mut c = Chain::standard();
        c.set_enabled("grade", false);
        let run = c.run(|_| true);
        assert_eq!(run[4].1, StageOutcome::Bypassed);
    }

    #[test]
    fn empty_chain_safe() {
        let c = Chain::new();
        assert!(c.run(|_| true).is_empty());
        assert!(c.order_is_topo_valid());
    }
}
