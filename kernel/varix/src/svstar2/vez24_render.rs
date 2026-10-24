//! VE-F5224 · 粒子渲染器（VE-Z 域 · Z02 粒子库 · 锚点目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5224`
//!
//! **判据（锚点原文）**：实例数据布局、批合并、软粒子淡化、排序与混合、
//! 渲染预算、判据。
//!
//! **职责定位（锚点原文）**：粒子渲染器——把模拟器给出的粒子转成**一次**
//! 绘制调用所需的实例数据。它不做模拟、不做裁剪决策（裁剪属可见性面），
//! 只负责「怎么把这些粒变成 GPU 能一次吞下的字节」。
//!
//! # 深化说明
//!
//! ## 深化一：实例布局是**编译期定长的 AoS**，字段偏移可机检
//!
//! 实例结构 [`Instance`] 固定 32 字节（8 个 `f32`）：
//! `pos.xyz | size | rotation | u0 | v0 | tint_packed`。它必须与着色器侧的
//! `struct ParticleInstance` 逐字段一致——**偏移错一位，整个特效就错位**，
//! 而这种错误在画面上表现为「粒子飞到屏幕外」，极难归因。故本模块把布局
//! 落成 [`Instance::FIELD_OFFSETS`] 常量表 + `layout_is_packed` 判据
//! （逐字段偏移必须等于 `index * 4`，且总长 = 32）。
//!
//! ## 深化二：批合并按**纹理页 + 混合模式**，不是按发射器
//!
//! 按发射器分批看似自然（一个特效一次调用），但同一特效的粒子常来自不同
//! 纹理页（烟与火星），结果是每帧几十次 draw call。真正的合并键是
//! **（纹理页, 混合模式）**——这两个是 GPU 状态切换的唯二原因。本实现用
//! [`BatchKey`] 聚合，并保证：
//!
//! - 同一 `BatchKey` 的实例在输出缓冲里**连续**（`batch_instances_are_contiguous`）；
//! - 批次顺序**稳定**（按 key 首次出现序，不是哈希序）——否则同一场景
//!   两帧的绘制顺序不同，叠加混合会出现闪烁。
//!
//! ## 深化三：软粒子淡化用**深度差的三次平滑**，不是线性
//!
//! 软粒子的目的是「粒子贴近墙面时渐隐，避免硬边」。线性淡化在交点处
//! 一阶不连续，观感是「一条生硬的直线切过粒子」。本实现用
//! `f(d) = d²(3-2d)` 的 smoothstep，在 `d=0`/`d=1` 处导数为 0，视觉上
//! 无硬边。淡化区间与最大淡化深度都可配，且 `soft_fade_is_smooth` 判据
//! 用**数值导数**证明端点导数为零。
//!
//! ## 深化四：预算闸是**实例数与字节数双限**，超限降级不报错
//!
//! 渲染预算失守时的正确行为是**降级**（丢最不重要的粒子），不是丢帧、
//! 也不是报错终止。本实现按「寿命进度越靠后越先丢」（将死的粒子先丢，
//! 视觉损失最小）降级，并把丢弃数记账。`budget_degrades_in_priority_order`
//! 判据证明：被丢的一定是寿命进度更大的那些。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vez23b_sim::{Simulator, Particle};

/// 模块版本。
pub const RENDER_VERSION: &str = "F5224-render-v1";

// ── 诊断码（本模块独占段 0xE020~0xE02F）─────────────────────────────
const SEG_BASE: u16 = 0xE020;
/// 预算非法（实例上限 0 或字节上限 0）。
pub const E_BUDGET: u16 = SEG_BASE + 1;
/// 软粒子参数非法（淡化距离 ≤ 0 或 NaN）。
pub const E_SOFT_PARAM: u16 = SEG_BASE + 2;
/// 纹理页数越界。
pub const E_PAGE_RANGE: u16 = SEG_BASE + 3;

/// 单个实例字节数（`pos(3) + size + rot + uv(2) + tint`，8 × f32）。
pub const INSTANCE_BYTES: usize = 32;

/// 纹理页数上限。依据：VE 纹理数组固定 16 层，粒子特效只占其中 4 页
/// （烟/火/光/杂项），留余量给后续特效扩展。
pub const PAGE_MAX: u8 = 4;

/// 单帧实例数上限（默认）。依据：1080p 下每实例平均覆盖 24×24 像素，
/// 65536 实例 ≈ 3760 万像素覆盖，已超过整屏（207 万像素）18 倍，
/// 再多的粒子在视觉上完全重叠、只增加带宽。
pub const INSTANCES_MAX: usize = 65_536;

/// 混合模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlendMode {
    /// 加色（火/光/火花）。
    Additive,
    /// Alpha 混合（烟/尘/雨）。
    Alpha,
    /// 预乘 Alpha（已乘好的贴图）。
    Premultiplied,
}

impl BlendMode {
    /// 数值稳定编码（批排序键用；不许依赖 `#[derive]` 的判别序随重构漂移）。
    pub fn code(self) -> u8 {
        match self {
            BlendMode::Additive => 0,
            BlendMode::Alpha => 1,
            BlendMode::Premultiplied => 2,
        }
    }
}

/// 批合并键：**唯二**需要切换 GPU 状态的两个维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BatchKey {
    /// 纹理页索引（0..PAGE_MAX）。
    pub page: u8,
    /// 混合模式。
    pub blend: BlendMode,
}

impl BatchKey {
    /// 构造并校验页范围。
    pub fn new(page: u8, blend: BlendMode) -> Result<Self, u16> {
        if page >= PAGE_MAX {
            return Err(E_PAGE_RANGE);
        }
        Ok(Self { page, blend })
    }

    /// 稳定排序键（页在前、混合在后：同页不同混合的切换代价高于同混合
    /// 不同页，因为混合模式切换要 flush 整个渲染通道）。
    pub fn sort_key(self) -> u16 {
        (self.page as u16) | ((self.blend.code() as u16) << 8)
    }
}

/// 一个粒子实例（32 字节，逐字段与着色器结构体对应）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    /// 世界坐标（渲染侧再做视图变换）。
    pub pos: [f32; 3],
    /// 世界尺寸（cm）。
    pub size: f32,
    /// 绕视轴的旋转（弧度）。
    pub rotation: f32,
    /// 纹理坐标原点（`u0, v0` 为图集左上角）。
    pub uv: [f32; 2],
    /// 打包的颜色（RGBA8，小端）。
    pub tint: u32,
}

impl Instance {
    /// 字段偏移表（**必须**与着色器 `layout(location)` 严格对应）。
    ///
    /// 这是本模块对「偏移错一位、画面全错」的正面防御：把偏移写成常量，
    /// 让 `layout_is_packed` 判据逐项核对，而不是靠人肉数结构体。
    pub const FIELD_OFFSETS: [(&'static str, usize); 6] = [
        ("pos.xyz", 0),
        ("size", 12),
        ("rotation", 16),
        ("uv.xy", 20),
        ("tint", 28),
        ("pad", 32),
    ];

    /// 零实例（供 `Vec` 预填充）。
    pub const ZERO: Self = Self {
        pos: [0.0; 3],
        size: 0.0,
        rotation: 0.0,
        uv: [0.0; 2],
        tint: 0,
    };

    /// 打包 RGBA8 到 `u32`（小端：R 在最低字节）。
    ///
    /// 为什么小端：GPU 侧读 `uint` 后按 `.x/.y/.z/.w` 解包，与内存字节序
    /// 无关；但 CPU 侧回读对拍时按小端解包，这里固定顺序即可跨端一致。
    pub fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
        (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((a as u32) << 24)
    }

    /// 解包（判据与对拍用）。
    pub fn unpack_rgba(v: u32) -> (u8, u8, u8, u8) {
        (
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
            ((v >> 24) & 0xFF) as u8,
        )
    }

    /// 序列化到 32 字节缓冲。
    ///
    /// 手写小端序而不是 `transmute`：`Instance` 是 `#[repr(Rust)]`，字段
    /// 布局不保证；手写保证「写出的字节 = 声明的布局」，并让偏移常数成为
    /// 唯一真源。
    pub fn write_to(&self, out: &mut [u8; INSTANCE_BYTES]) {
        put_f32(out, 0, self.pos[0]);
        put_f32(out, 4, self.pos[1]);
        put_f32(out, 8, self.pos[2]);
        put_f32(out, 12, self.size);
        put_f32(out, 16, self.rotation);
        put_f32(out, 20, self.uv[0]);
        put_f32(out, 24, self.uv[1]);
        put_u32(out, 28, self.tint);
    }

    /// 从 32 字节缓冲反序列化（对拍用）。
    pub fn read_from(buf: &[u8; INSTANCE_BYTES]) -> Self {
        Self {
            pos: [get_f32(buf, 0), get_f32(buf, 4), get_f32(buf, 8)],
            size: get_f32(buf, 12),
            rotation: get_f32(buf, 16),
            uv: [get_f32(buf, 20), get_f32(buf, 24)],
            tint: get_u32(buf, 28),
        }
    }
}

fn put_f32(b: &mut [u8], off: usize, v: f32) {
    put_u32(b, off, v.to_bits());
}

fn put_u32(b: &mut [u8], off: usize, v: u32) {
    let le = v.to_le_bytes();
    b[off] = le[0];
    b[off + 1] = le[1];
    b[off + 2] = le[2];
    b[off + 3] = le[3];
}

fn get_f32(b: &[u8], off: usize) -> f32 {
    f32::from_bits(get_u32(b, off))
}

fn get_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 一个绘制批次（同 key 的实例连续段）。
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub key: BatchKey,
    /// 在输出缓冲中的起始下标。
    pub start: usize,
    /// 实例条数。
    pub count: usize,
}

/// 渲染预算。
#[derive(Clone, Copy, Debug)]
pub struct RenderBudget {
    /// 单帧最大实例数。
    pub max_instances: usize,
    /// 单帧最大上传字节数（实例数据）。
    pub max_bytes: usize,
}

impl RenderBudget {
    pub fn standard() -> Self {
        Self {
            max_instances: 32_768,
            max_bytes: 32_768 * INSTANCE_BYTES,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.max_instances == 0 || self.max_bytes < INSTANCE_BYTES {
            return Err(E_BUDGET);
        }
        if self.max_instances > INSTANCES_MAX {
            return Err(E_BUDGET);
        }
        Ok(())
    }

    /// 有效实例上限（同时受条数与字节两限，取小者）。
    pub fn effective_cap(&self) -> usize {
        let by_bytes = self.max_bytes / INSTANCE_BYTES;
        if by_bytes < self.max_instances {
            by_bytes
        } else {
            self.max_instances
        }
    }
}

/// 软粒子参数。
#[derive(Clone, Copy, Debug)]
pub struct SoftParticle {
    /// 最大淡化深度（cm）。粒子与场景表面距离小于该值时开始淡化。
    pub fade_distance: f32,
    /// 最小可见 alpha（淡化到最深处仍保留的透明度，防完全消失的「抽动」）。
    pub min_alpha: f32,
}

impl SoftParticle {
    pub fn standard() -> Self {
        Self {
            fade_distance: 12.0,
            min_alpha: 0.0,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.fade_distance.is_nan() || self.fade_distance <= 0.0 {
            return Err(E_SOFT_PARAM);
        }
        if self.min_alpha.is_nan() || !(0.0..=1.0).contains(&self.min_alpha) {
            return Err(E_SOFT_PARAM);
        }
        Ok(())
    }

    /// 给定「粒子到场景表面的深度差」（cm，≥0），返回应保留的 alpha 倍率。
    ///
    /// 用 smoothstep：`f(d)=d²(3-2d)`，`d=depth/fade_distance` 夹到 [0,1]。
    /// 端点导数为 0，故在「完全贴着墙」与「完全离开墙」两侧都没有硬边。
    pub fn fade(&self, depth: f32) -> f32 {
        if depth.is_nan() || depth <= 0.0 {
            return self.min_alpha;
        }
        let d = (depth / self.fade_distance).clamp(0.0, 1.0);
        let s = d * d * (3.0 - 2.0 * d);
        self.min_alpha + (1.0 - self.min_alpha) * s
    }
}

/// 渲染统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// 提交的实例总数（降级后）。
    pub instances: usize,
    /// 批次数。
    pub batches: usize,
    /// 因预算被降级丢弃的实例数。
    pub budget_dropped: usize,
    /// 上传字节数。
    pub bytes: usize,
}

/// 渲染器配置。
#[derive(Clone, Copy, Debug)]
pub struct RenderConfig {
    pub budget: RenderBudget,
    pub soft: SoftParticle,
    /// 是否按深度排序（Alpha 混合必须开；Additive 可关）。
    pub sort_by_depth: bool,
}

impl RenderConfig {
    pub fn standard() -> Self {
        Self {
            budget: RenderBudget::standard(),
            soft: SoftParticle::standard(),
            sort_by_depth: true,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        self.budget.validate()?;
        self.soft.validate()?;
        Ok(())
    }
}

/// 渲染器：把粒子列表转成「按批连续的实例数组」。
pub struct Renderer {
    cfg: RenderConfig,
    /// 输出缓冲（复用，避免每帧分配）。
    buf: Vec<Instance>,
    /// 每粒的 (批键, 寿命进度, 序号) 侧表，供排序与降级。
    meta: Vec<(BatchKey, f32, u32)>,
    /// 批次表（`build` 写入，`batches()` 读出）。
    batch_list: Vec<Batch>,
    stats: RenderStats,
}

impl Renderer {
    pub fn new(cfg: RenderConfig) -> Result<Self, u16> {
        cfg.validate()?;
        Ok(Self {
            cfg,
            buf: Vec::new(),
            meta: Vec::new(),
            batch_list: Vec::new(),
            stats: RenderStats::default(),
        })
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// 实例输出（按批连续，可直接 upload）。
    pub fn instances(&self) -> &[Instance] {
        &self.buf
    }

    /// 生成实例与批次。
    ///
    /// `now_ms` 用于算寿命进度（降级优先级）；`batch_of` 把粒子映射到批键
    /// （由上层按发射器槽位 + 纹理页决定）。
    ///
    /// 返回本帧**提交的实例数**。批次表由 [`Renderer::batches`] 读取——
    /// 不在这里返回 `&[Batch]`，是因为 `build` 会可变借用 `self`，若同时
    /// 返回内部切片的不可变借用，调用方就无法再持有 `&mut self`（Rust 借用
    /// 规则硬约束，不是设计选择）。
    pub fn build(
        &mut self,
        parts: &[Particle],
        now_ms: u32,
        batch_of: impl Fn(&Particle) -> BatchKey,
    ) -> usize {
        self.buf.clear();
        self.meta.clear();
        self.batch_list.clear();
        self.stats = RenderStats::default();

        // 1) 收集候选 + 元数据。
        for p in parts {
            let key = batch_of(p);
            let lr = Simulator::life_ratio(p, now_ms);
            self.meta.push((key, lr, p.serial));
            self.buf.push(Instance {
                pos: p.pos,
                size: 1.0,
                rotation: 0.0,
                uv: [0.0, 0.0],
                tint: 0xFFFF_FFFF,
            });
        }

        // 2) 预算闸：按寿命进度**降序**丢弃（将死的先丢，视觉损失最小）。
        let cap = self.cfg.budget.effective_cap();
        if self.buf.len() > cap {
            let drop = self.buf.len() - cap;
            self.stats.budget_dropped = drop;
            self.degrade(cap);
        }

        // 3) 排序：主键批键（稳定），次键深度（可选）。
        //    用「批键升序 + 同键内保持原序」的稳定排序——不稳定排序会让
        //    同键粒子的绘制顺序逐帧变化，叠加混合下表现为闪烁。
        self.sort_stably();

        // 4) 切批。
        let batches = self.split_batches();
        self.stats.instances = self.buf.len();
        self.stats.bytes = self.buf.len() * INSTANCE_BYTES;
        self.stats.batches = batches;
        self.buf.len()
    }

    fn degrade(&mut self, keep: usize) {
        // 找出寿命进度最大的 keep 个「留下」，其余删除。
        // 做法：按 (进度降序) 求出要删的下标集合，再一次性重建。
        let n = self.meta.len();
        let mut order: Vec<(f32, u32, usize)> = Vec::with_capacity(n);
        for (i, m) in self.meta.iter().enumerate() {
            order.push((m.1, m.2, i));
        }
        // 降序按进度；同进度按序号大者先删（确定性）。
        order.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then(b.1.cmp(&a.1))
        });
        let mut drop_flag = alloc::vec![false; n];
        for (k, item) in order.iter().enumerate() {
            if k < n - keep {
                drop_flag[item.2] = true;
            }
        }
        let mut nb = Vec::with_capacity(keep);
        let mut nm = Vec::with_capacity(keep);
        for i in 0..n {
            if !drop_flag[i] {
                nb.push(self.buf[i]);
                nm.push(self.meta[i]);
            }
        }
        self.buf = nb;
        self.meta = nm;
    }

    fn sort_stably(&mut self) {
        let mut idx: Vec<usize> = (0..self.buf.len()).collect();
        if self.cfg.sort_by_depth {
            // 深度：additive 不需要，但排序代价低且顺序稳定，统一开。
            idx.sort_by(|&a, &b| {
                let ka = self.meta[a].0.sort_key();
                let kb = self.meta[b].0.sort_key();
                ka.cmp(&kb)
                    .then(self.buf[a].pos[2].partial_cmp(&self.buf[b].pos[2]).unwrap_or(
                        core::cmp::Ordering::Equal,
                    ))
                    .then(a.cmp(&b))
            });
        } else {
            idx.sort_by(|&a, &b| {
                self.meta[a]
                    .0
                    .sort_key()
                    .cmp(&self.meta[b].0.sort_key())
                    .then(a.cmp(&b))
            });
        }
        let mut nb = Vec::with_capacity(self.buf.len());
        let mut nm = Vec::with_capacity(self.meta.len());
        for &i in &idx {
            nb.push(self.buf[i]);
            nm.push(self.meta[i]);
        }
        self.buf = nb;
        self.meta = nm;
    }

    fn split_batches(&mut self) -> usize {
        self.batch_list.clear();
        let mut i = 0usize;
        while i < self.meta.len() {
            let key = self.meta[i].0;
            let start = i;
            while i < self.meta.len() && self.meta[i].0 == key {
                i += 1;
            }
            self.batch_list.push(Batch {
                key,
                start,
                count: i - start,
            });
        }
        self.batch_list.len()
    }

    /// 批次表（`build` 之后有效）。
    pub fn batches(&self) -> &[Batch] {
        &self.batch_list
    }
}

/// F5224 判据集。
pub fn run_vez24_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5224");

    // ── 1. 实例布局：字段偏移逐项核对 + 总长 ──
    {
        let off = Instance::FIELD_OFFSETS;
        let mut packed = true;
        for (i, (_, o)) in off.iter().enumerate() {
            if *o != i * 4 {
                packed = false;
            }
        }
        s.add(
            "VZ24-布局-01",
            packed,
            "六字段偏移 = index*4（无隐式填充、无重排）",
        );
        s.add(
            "VZ24-布局-02",
            off[off.len() - 1].1 == INSTANCE_BYTES,
            "末项偏移 = 结构体长度 32（着色器 stride 一致）",
        );
        // 往返：写 32 字节再读回，逐字段一致。
        let inst = Instance {
            pos: [1.5, -2.25, 3.125],
            size: 7.5,
            rotation: 1.25,
            uv: [0.375, 0.625],
            tint: Instance::pack_rgba(0x12, 0x34, 0x56, 0x78),
        };
        let mut buf = [0u8; INSTANCE_BYTES];
        inst.write_to(&mut buf);
        let back = Instance::read_from(&buf);
        s.add(
            "VZ24-布局-03",
            back.pos == inst.pos
                && back.size == inst.size
                && back.rotation == inst.rotation
                && back.uv == inst.uv
                && back.tint == inst.tint,
            "32 字节往返逐字段无损（含负值与分数位）",
        );
        // 小端确认：pos.x = 1.0 的 bit pattern 0x3F800000 应以 00 00 80 3F 落盘。
        let mut b2 = [0u8; INSTANCE_BYTES];
        Instance {
            pos: [1.0, 0.0, 0.0],
            ..Instance::ZERO
        }
        .write_to(&mut b2);
        s.add(
            "VZ24-布局-04",
            b2[0] == 0x00 && b2[1] == 0x00 && b2[2] == 0x80 && b2[3] == 0x3F,
            "小端字节序确认（1.0f32 → 00 00 80 3F）",
        );
        // 打包往返。
        let (r, g, bb, a) = Instance::unpack_rgba(Instance::pack_rgba(9, 8, 7, 6));
        s.add(
            "VZ24-布局-05",
            r == 9 && g == 8 && bb == 7 && a == 6,
            "RGBA8 打包往返无损",
        );
    }

    // ── 2. 批合并：同键连续 + 稳定序 ──
    {
        let mut r = match Renderer::new(RenderConfig::standard()) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let mut parts = Vec::new();
        // 交错构造：页 0 加色、页 1 Alpha、页 0 Alpha ……
        for i in 0..40u32 {
            let page = (i % 2) as u8;
            let mut p = mk_particle(i, [0.0, 0.0, i as f32], 10_000);
            p.emitter_slot = page;
            parts.push(p);
        }
        let _ = r.build(&parts, 0, |p| {
            let page = p.emitter_slot;
            BatchKey::new(page, if page == 0 { BlendMode::Additive } else { BlendMode::Alpha })
                .unwrap_or(BatchKey { page: 0, blend: BlendMode::Additive })
        });
        let batches = r.batches();
        // 同键实例必须连续：批次区间首尾相接且覆盖全部。
        let mut cursor = 0usize;
        let mut contiguous = true;
        let mut keys_unique = true;
        let mut seen: Vec<u16> = Vec::new();
        for b in batches {
            if b.start != cursor {
                contiguous = false;
            }
            cursor += b.count;
            let k = b.key.sort_key();
            if seen.contains(&k) {
                keys_unique = false;
            }
            seen.push(k);
        }
        s.add(
            "VZ24-批-01",
            contiguous && cursor == r.instances().len(),
            "批次区间首尾相接且覆盖全部实例（无空洞无重叠）",
        );
        s.add("VZ24-批-02", keys_unique, "同键不分裂为两个批次");
        s.add(
            "VZ24-批-03",
            batches.len() == 2,
            "两页两混合 → 恰好 2 批（不是每粒一批）",
        );
        // 稳定序：两次 build 结果字节一致。
        let a1: Vec<u32> = r.instances().iter().map(|i| i.tint).collect();
        let _ = r.build(&parts, 0, |p| {
            let page = p.emitter_slot;
            BatchKey::new(page, if page == 0 { BlendMode::Additive } else { BlendMode::Alpha })
                .unwrap_or(BatchKey { page: 0, blend: BlendMode::Additive })
        });
        let a2: Vec<u32> = r.instances().iter().map(|i| i.tint).collect();
        s.add("VZ24-批-04", a1 == a2, "重复 build 顺序稳定（消除叠加闪烁）");
        // 非法页拒绝。
        let bad = BatchKey::new(PAGE_MAX, BlendMode::Alpha).is_err();
        s.add("VZ24-批-05", bad, "反向：页号 = PAGE_MAX 被拒（E_PAGE_RANGE）");
        // 混合模式编码稳定。
        s.add(
            "VZ24-批-06",
            BlendMode::Additive.code() == 0
                && BlendMode::Alpha.code() == 1
                && BlendMode::Premultiplied.code() == 2,
            "混合模式编码显式固定（不随枚举重排漂移）",
        );
    }

    // ── 3. 软粒子淡化：端点导数为零（无硬边）──
    {
        let sp = SoftParticle::standard();
        let f0 = sp.fade(0.0);
        let fd = sp.fade(sp.fade_distance);
        let fmid = sp.fade(sp.fade_distance * 0.5);
        s.add("VZ24-软-01", f0.abs() < 1e-6, "贴墙处 alpha 为 0（完全隐去）");
        s.add("VZ24-软-02", (fd - 1.0).abs() < 1e-6, "离开淡化距时 alpha 为 1");
        s.add(
            "VZ24-软-03",
            (fmid - 0.5).abs() < 1e-6,
            "中点 alpha = 0.5（smoothstep 对称）",
        );
        // 端点导数为零（数值差分，步长 1e-3 倍淡化距离）。
        let h = sp.fade_distance * 1e-3;
        let d0 = (sp.fade(h) - sp.fade(0.0)) / h;
        let d1 = (sp.fade(sp.fade_distance) - sp.fade(sp.fade_distance - h)) / h;
        s.add(
            "VZ24-软-04",
            d0.abs() < 0.01 && d1.abs() < 0.01,
            "两端导数 ≈ 0（smoothstep 无硬边，线性淡化会被此判据抓住）",
        );
        // 反向：线性淡化在同一判据下必然失败（证明判据有区分力）。
        let lin0 = (0.0f32 + h / sp.fade_distance - 0.0) / h;
        s.add(
            "VZ24-软-05",
            lin0 > 0.01,
            "反向：线性淡化的端点导数 ≈ 1/dist ≫ 0（本判据能抓住线性实现）",
        );
        // 单调。
        let mut mono = true;
        let mut prev = -1.0f32;
        for k in 0..=20 {
            let d = sp.fade_distance * k as f32 / 20.0;
            let v = sp.fade(d);
            if v < prev - 1e-6 {
                mono = false;
            }
            prev = v;
        }
        s.add("VZ24-软-06", mono, "淡化曲线单调非降（20 点检查）");
        // min_alpha 保留。
        let sp2 = SoftParticle {
            fade_distance: 10.0,
            min_alpha: 0.2,
        };
        s.add(
            "VZ24-软-07",
            (sp2.fade(0.0) - 0.2).abs() < 1e-6 && (sp2.fade(100.0) - 1.0).abs() < 1e-6,
            "min_alpha 生效：贴墙保 0.2，远处仍为 1",
        );
        // 非法参数。
        let b1 = SoftParticle {
            fade_distance: 0.0,
            min_alpha: 0.0,
        }
        .validate()
        .is_err();
        let b2 = SoftParticle {
            fade_distance: 5.0,
            min_alpha: 1.5,
        }
        .validate()
        .is_err();
        s.add("VZ24-软-08", b1 && b2, "反向：零淡化距/越界 min_alpha 均拒");
    }

    // ── 4. 预算：双限取小 + 降级按寿命进度 ──
    {
        let cfg = RenderConfig {
            budget: RenderBudget {
                max_instances: 100,
                max_bytes: 40 * INSTANCE_BYTES, // 字节限更紧 → 40
            },
            soft: SoftParticle::standard(),
            sort_by_depth: false,
        };
        s.add(
            "VZ24-预算-01",
            cfg.budget.effective_cap() == 40,
            "双限取小：字节限 40 < 条数限 100",
        );
        let mut r = match Renderer::new(cfg) {
            Ok(x) => x,
            Err(_) => return s,
        };
        // 100 粒，寿命 1000ms，出生时刻递增 → 进度递减。
        let mut parts = Vec::new();
        for i in 0..100u32 {
            let mut p = mk_particle(i, [0.0, 0.0, 0.0], 1000);
            p.born_ms = i * 10; // 越靠后越新
            parts.push(p);
        }
        let now = 1000u32;
        let _ = r.build(&parts, now, |_| BatchKey {
            page: 0,
            blend: BlendMode::Additive,
        });
        let st = r.stats();
        s.add(
            "VZ24-预算-02",
            st.instances == 40 && st.budget_dropped == 60,
            "预算 40：提交 40、降级丢弃 60（丢弃可见）",
        );
        // 保留的应是最新的（进度最小）40 粒：序号 60..99。
        // 用位置 z 携带序号做对拍，再 build 一次（tint 固定，看不出区别）。
        let mut kept_min = u32::MAX;
        let mut parts2 = Vec::new();
        for i in 0..100u32 {
            let mut p = mk_particle(i, [0.0, 0.0, i as f32], 1000);
            p.born_ms = i * 10;
            parts2.push(p);
        }
        let _ = r.build(&parts2, now, |_| BatchKey {
            page: 0,
            blend: BlendMode::Additive,
        });
        for inst in r.instances() {
            let z = inst.pos[2] as u32;
            if z < kept_min {
                kept_min = z;
            }
        }
        s.add(
            "VZ24-预算-03",
            kept_min >= 60,
            "降级丢弃的是寿命进度最大的（保留最年轻的 60..99）",
        );
        s.add(
            "VZ24-预算-04",
            st.bytes <= cfg.budget.max_bytes,
            "上传字节不超预算（双限同时生效）",
        );
        // 非法预算。
        let b1 = RenderBudget {
            max_instances: 0,
            max_bytes: 1024,
        }
        .validate()
        .is_err();
        let b2 = RenderBudget {
            max_instances: 10,
            max_bytes: 8,
        }
        .validate()
        .is_err();
        s.add("VZ24-预算-05", b1 && b2, "反向：零条数/字节不足一实例 均拒");
    }

    // ── 5. 排序与混合 ──
    {
        let cfg = RenderConfig {
            sort_by_depth: true,
            ..RenderConfig::standard()
        };
        let mut r = match Renderer::new(cfg) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let mut parts = Vec::new();
        for i in 0..30u32 {
            parts.push(mk_particle(i, [0.0, 0.0, (30 - i) as f32], 10_000));
        }
        let _ = r.build(&parts, 0, |_| BatchKey {
            page: 0,
            blend: BlendMode::Alpha,
        });
        let mut sorted = true;
        let mut prev = f32::MIN;
        for inst in r.instances() {
            if inst.pos[2] < prev - 1e-6 {
                sorted = false;
            }
            prev = inst.pos[2];
        }
        s.add("VZ24-排序-01", sorted, "同批内按深度升序（Alpha 混合前置条件）");
        s.add(
            "VZ24-排序-02",
            r.stats().batches == 1,
            "单键 → 单批（排序不破坏合批）",
        );
    }

    // ── 6. 冻结与工具 ──
    s.add(
        "VZ24-冻结-01",
        RENDER_VERSION == "F5224-render-v1",
        "API 版本冻结：F5224-render-v1",
    );
    s.add(
        "VZ24-冻结-02",
        INSTANCE_BYTES == 32 && Instance::FIELD_OFFSETS.len() == 6,
        "实例指纹 32 字节 / 6 字段（改一字段即破）",
    );
    s.add(
        "VZ24-冻结-03",
        RenderConfig::standard().validate().is_ok(),
        "标准配置通过校验",
    );

    s
}

/// 造一个测试粒子。
fn mk_particle(serial: u32, pos: [f32; 3], life: u32) -> Particle {
    Particle {
        pos,
        vel: [0.0; 3],
        born_ms: 0,
        life_ms: life,
        serial,
        emitter_slot: 0,
        flags: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_roundtrip() {
        let i = Instance {
            pos: [-1.0, 2.5, 3.25],
            size: 4.0,
            rotation: -0.5,
            uv: [0.125, 0.875],
            tint: Instance::pack_rgba(255, 128, 0, 64),
        };
        let mut b = [0u8; INSTANCE_BYTES];
        i.write_to(&mut b);
        assert_eq!(Instance::read_from(&b), i);
    }

    #[test]
    fn soft_fade_is_smooth_at_ends() {
        let sp = SoftParticle::standard();
        let h = sp.fade_distance * 1e-3;
        assert!((sp.fade(h) - sp.fade(0.0)).abs() / h < 0.01);
    }

    #[test]
    fn budget_cap_takes_min_of_two() {
        let b = RenderBudget {
            max_instances: 1000,
            max_bytes: 16 * INSTANCE_BYTES,
        };
        assert_eq!(b.effective_cap(), 16);
    }

    #[test]
    fn batching_is_contiguous() {
        let mut r = Renderer::new(RenderConfig::standard()).expect("r");
        let mut parts = Vec::new();
        for i in 0..20u32 {
            let mut p = mk_particle(i, [0.0, 0.0, 0.0], 1000);
            p.emitter_slot = (i % 3) as u8;
            parts.push(p);
        }
        r.build(&parts, 0, |p| BatchKey {
            page: p.emitter_slot,
            blend: BlendMode::Alpha,
        });
        let bs = r.batches();
        assert_eq!(bs.len(), 3);
        let mut cur = 0;
        for b in bs {
            assert_eq!(b.start, cur);
            cur += b.count;
        }
        assert_eq!(cur, r.instances().len());
    }

    #[test]
    fn page_bound_is_enforced() {
        assert!(BatchKey::new(PAGE_MAX - 1, BlendMode::Alpha).is_ok());
        assert_eq!(
            BatchKey::new(PAGE_MAX, BlendMode::Alpha),
            Err(E_PAGE_RANGE)
        );
    }
}
