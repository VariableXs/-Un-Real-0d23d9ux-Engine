//! cgb742_scenelink —— CGPU-F0742 画像与场景联动。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0742 画像与场景联动  <SEG 0x4b00>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0742；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 画像（Profile）是工作负载的静态指纹，场景（Scene）是运行期容器；
//!      联动 = 场景按亲和度选画像，选择过程全程落账、可复算；
//!   2. 亲和度用整数打分（0~1000），禁浮点；同分取场景表序小者（确定性）；
//!   3. 联动状态机五态闭集：Unbound→Probed→Linked→Active→Retired，
//!      非法边一律 XcCode 红码拒绝，不允许静默降级；
//!   4. 联动账为定容环形账（64 槽），守恒式：used = filled − evicted 恒成立；
//!   5. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4b00 段为本模块独占（SEG_BASE=0x4b00），XcCode 判别式
//!   SEG_BASE+1..+N，不与其他模块段位重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const SL_VERSION: &str = "cgb742_scenelink-v1";

/// 诊断段基址（0x4b00 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4B00;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4b01 画像不存在（查表未命中）。
pub const XC_PROFILE_MISSING: u16 = SEG_BASE + 1;
/// 0x4b02 场景不存在（查表未命中）。
pub const XC_SCENE_MISSING: u16 = SEG_BASE + 2;
/// 0x4b03 联动状态机非法边（当前态不允许该转移）。
pub const XC_ILLEGAL_EDGE: u16 = SEG_BASE + 3;
/// 0x4b04 画像容量满（注册表 32 槽已尽）。
pub const XC_PROFILE_FULL: u16 = SEG_BASE + 4;
/// 0x4b05 联动账溢出（环形账 64 槽已被占满且不可逐出）。
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 5;
/// 0x4b06 画像重复注册（同名画像已存在）。
pub const XC_PROFILE_DUP: u16 = SEG_BASE + 6;
/// 0x4b07 亲和度越界（打分必须落在 0..=1000）。
pub const XC_AFFINITY_RANGE: u16 = SEG_BASE + 7;
/// 0x4b08 画像处于不可联动态（Retired 后禁止再激活）。
pub const XC_PROFILE_RETIRED: u16 = SEG_BASE + 8;
/// 0x4b09 场景资源权重和为零（无法承载任何画像）。
pub const XC_WEIGHT_ZERO: u16 = SEG_BASE + 9;

/// 画像注册表容量（定容，写死：册内施工书 §画像注册表 容量 32）。
pub const PROFILE_CAP: usize = 32;
/// 联动环形账容量（定容，写死：册内施工书 §联动账 容量 64）。
pub const LEDGER_CAP: usize = 64;
/// 亲和度满分（整数 1000 = 1.0，禁浮点故按千分位整数计）。
pub const AFFINITY_MAX: u32 = 1000;

/// 场景类别（运行期容器大类，闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneKind {
    /// 渲染类场景（光栅/合成为主）。
    Render,
    /// 计算类场景（kernel 吞吐为主）。
    Compute,
    /// 媒体类场景（编解码管线为主）。
    Media,
    /// 空闲类场景（功耗墙纸）。
    Idle,
}

/// 工作负载画像：静态指纹（注册后不可变）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// 画像短标识（注册序号，0 起）。
    pub pid: u16,
    /// 资源权重：光栅份额（千分位）。
    pub w_raster: u32,
    /// 资源权重：计算份额（千分位）。
    pub w_compute: u32,
    /// 资源权重：媒体份额（千分位）。
    pub w_media: u32,
    /// 画像候选场景位图（bit0=Render bit1=Compute bit2=Media bit3=Idle）。
    pub scene_mask: u8,
}

impl Profile {
    /// 权重守恒检查：三份额之和必须恰为 AFFINITY_MAX（千分位闭环）。
    pub fn weights_conserved(&self) -> bool {
        self.w_raster + self.w_compute + self.w_media == AFFINITY_MAX
    }

    /// 画像是否允许挂到某场景类别。
    pub fn allows(&self, kind: SceneKind) -> bool {
        let bit = match kind {
            SceneKind::Render => 0u8,
            SceneKind::Compute => 1u8,
            SceneKind::Media => 2u8,
            SceneKind::Idle => 3u8,
        };
        (self.scene_mask >> bit) & 1 == 1
    }
}

/// 联动状态机五态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// 未绑定：场景尚无画像候选。
    Unbound,
    /// 已探测：亲和度打分已完成。
    Probed,
    /// 已联动：画像已挂接、等待激活。
    Linked,
    /// 激活中：联动生效、画像随场景运行。
    Active,
    /// 已退役：联动解除、画像归还注册表。
    Retired,
}

/// 合法边闭集（当前态 → 允许的下一态集合）。
pub fn legal_edges(s: LinkState) -> &'static [LinkState] {
    match s {
        LinkState::Unbound => &[LinkState::Probed],
        LinkState::Probed => &[LinkState::Linked, LinkState::Unbound],
        LinkState::Linked => &[LinkState::Active, LinkState::Retired],
        LinkState::Active => &[LinkState::Retired],
        LinkState::Retired => &[],
    }
}

/// 状态转移核：合法则返回新态，否则红码。
pub fn step(cur: LinkState, next: LinkState) -> Result<LinkState, u16> {
    if legal_edges(cur).iter().any(|n| *n == next) {
        Ok(next)
    } else {
        Err(XC_ILLEGAL_EDGE)
    }
}

/// 场景描述：运行期容器。
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    /// 场景标识。
    pub sid: u16,
    /// 场景类别。
    pub kind: SceneKind,
    /// 场景压力（0..=100，用于亲和度加权）。
    pub pressure: u32,
}

/// 亲和度打分：整数千分位。
/// 规则（写死）：base = 画像含该场景类别则 400 否则 0；
/// bonus = 压力 × 类别主份额 / 1000；结果截到 0..=1000。
pub fn affinity(p: &Profile, s: &Scene) -> Result<u32, u16> {
    if s.pressure > 100 {
        return Err(XC_AFFINITY_RANGE);
    }
    let base = if p.allows(s.kind) { 400u32 } else { 0 };
    let main_w = match s.kind {
        SceneKind::Render => p.w_raster,
        SceneKind::Compute => p.w_compute,
        SceneKind::Media => p.w_media,
        SceneKind::Idle => 1000,
    };
    if main_w == 0 && s.kind != SceneKind::Idle {
        return Err(XC_WEIGHT_ZERO);
    }
    let bonus = s.pressure * main_w / 100;
    let score = base + bonus;
    Ok(if score > AFFINITY_MAX { AFFINITY_MAX } else { score })
}

/// 联动账目：一次场景↔画像绑定的完整留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkEntry {
    /// 场景标识。
    pub sid: u16,
    /// 画像标识。
    pub pid: u16,
    /// 达成时的亲和度。
    pub score: u32,
    /// 达成时的状态机终态（Linked 或 Active）。
    pub settled: LinkState,
}

/// 定容环形联动账：写满即逐出最老账目（账目数守恒）。
pub struct LinkLedger {
    buf: [Option<LinkEntry>; LEDGER_CAP],
    head: usize,
    filled: usize,
    evicted: usize,
}

impl LinkLedger {
    /// 新账：全空槽。
    pub fn new() -> Self {
        LinkLedger {
            buf: [None; LEDGER_CAP],
            head: 0,
            filled: 0,
            evicted: 0,
        }
    }

    /// 落账一条；满则逐出最老。返回被逐出账目（如有）。
    pub fn push(&mut self, e: LinkEntry) -> Option<LinkEntry> {
        let ev = if self.filled < LEDGER_CAP {
            self.filled += 1;
            None
        } else {
            let old = self.buf[self.head];
            self.evicted += 1;
            old
        };
        self.buf[self.head] = Some(e);
        self.head = (self.head + 1) % LEDGER_CAP;
        ev
    }

    /// 当前在账目数。
    pub fn used(&self) -> usize {
        self.filled - self.evicted.min(self.filled)
    }

    /// 守恒恒等式：used ≤ LEDGER_CAP 且 used = 累计写入 − 累计逐出。
    pub fn conserved(&self, written: usize) -> bool {
        let in_ring = self.buf.iter().filter(|s| s.is_some()).count();
        in_ring == self.used() && self.used() <= LEDGER_CAP && written >= self.used()
    }

    /// 按场景号取最新在账账目。
    pub fn latest_by_scene(&self, sid: u16) -> Option<LinkEntry> {
        // 环形账自 head 逆时针即从新到旧。
        for i in 0..LEDGER_CAP {
            let idx = (self.head + LEDGER_CAP - 1 - i) % LEDGER_CAP;
            if let Some(e) = self.buf[idx] {
                if e.sid == sid {
                    return Some(e);
                }
            }
        }
        None
    }
}

/// 画像注册表：定容 32 槽，pid 即槽位序。
pub struct ProfileRegistry {
    slots: Vec<Profile>,
}

impl ProfileRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        ProfileRegistry { slots: Vec::new() }
    }

    /// 注册画像（pid 自动分配 = 当前槽位数）。
    pub fn register(
        &mut self,
        w_raster: u32,
        w_compute: u32,
        w_media: u32,
        scene_mask: u8,
    ) -> Result<u16, u16> {
        if self.slots.len() >= PROFILE_CAP {
            return Err(XC_PROFILE_FULL);
        }
        let cand = Profile {
            pid: self.slots.len() as u16,
            w_raster,
            w_compute,
            w_media,
            scene_mask,
        };
        if !cand.weights_conserved() {
            return Err(XC_AFFINITY_RANGE);
        }
        if self.slots.iter().any(|p| *p == cand) {
            return Err(XC_PROFILE_DUP);
        }
        let pid = cand.pid;
        self.slots.push(cand);
        Ok(pid)
    }

    /// 按 pid 取画像。
    pub fn get(&self, pid: u16) -> Result<&Profile, u16> {
        self.slots.get(pid as usize).ok_or(XC_PROFILE_MISSING)
    }

    /// 在册画像数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }
}

/// 场景注册表：定容 16 槽（写死：册内 §场景表 容量 16）。
pub const SCENE_CAP: usize = 16;

/// 联动引擎：画像注册表 + 场景表 + 状态机 + 环形账 的总装。
pub struct SceneLinker {
    pub profiles: ProfileRegistry,
    scenes: [Option<Scene>; SCENE_CAP],
    state: LinkState,
    ledger: LinkLedger,
    /// 已完成的激活计数（审计面）。
    pub activations: u32,
}

impl SceneLinker {
    /// 空引擎：状态 Unbound。
    pub fn new() -> Self {
        SceneLinker {
            profiles: ProfileRegistry::new(),
            scenes: [None; SCENE_CAP],
            state: LinkState::Unbound,
            ledger: LinkLedger::new(),
            activations: 0,
        }
    }

    /// 登记场景（同号覆盖视为红码——场景号一次性）。
    pub fn put_scene(&mut self, s: Scene) -> Result<(), u16> {
        let idx = s.sid as usize;
        if idx >= SCENE_CAP {
            return Err(XC_SCENE_MISSING);
        }
        if self.scenes[idx].is_some() {
            return Err(XC_SCENE_MISSING);
        }
        if s.pressure > 100 {
            return Err(XC_AFFINITY_RANGE);
        }
        self.scenes[idx] = Some(s);
        Ok(())
    }

    /// 场景查表。
    pub fn scene(&self, sid: u16) -> Result<&Scene, u16> {
        self.scenes
            .get(sid as usize)
            .and_then(|o| o.as_ref())
            .ok_or(XC_SCENE_MISSING)
    }

    /// 联动主路径：对场景 sid 在全部在册画像中选亲和度最高者；
    /// 同分取 pid 小者（确定性）。走状态机 Unbound→Probed→Linked→落账。
    pub fn link(&mut self, sid: u16) -> Result<LinkEntry, u16> {
        let sc = *self.scene(sid)?;
        let mut best: Option<(u32, u16)> = None;
        for i in 0..self.profiles.len() {
            let p = self.profiles.get(i as u16)?;
            let score = affinity(p, &sc)?;
            match best {
                Some((bs, bp)) if bs >= score => {}
                _ => best = Some((score, i as u16)),
            }
        }
        let (score, pid) = best.ok_or(XC_PROFILE_MISSING)?;
        // 状态机走两步合法边：Unbound→Probed→Linked。
        self.state = step(self.state, LinkState::Probed)?;
        self.state = step(self.state, LinkState::Linked)?;
        let e = LinkEntry {
            sid,
            pid,
            score,
            settled: LinkState::Linked,
        };
        if self.ledger.push(e).is_some() {
            // 正常逐出是合法行为；仅当逐出导致守恒破坏才红码。
            if !self.ledger.conserved((self.ledger.used() + 1) as usize) {
                return Err(XC_LEDGER_FULL);
            }
        }
        Ok(e)
    }

    /// 激活：Linked→Active，并回写账目终态为 Active。
    pub fn activate(&mut self, sid: u16) -> Result<(), u16> {
        self.state = step(self.state, LinkState::Active)?;
        self.activations += 1;
        // 账内最新同场景账目终态改写为 Active（ settled 覆写是账面事实）。
        if let Some(e) = self.ledger.latest_by_scene(sid) {
            let upd = LinkEntry {
                settled: LinkState::Active,
                ..e
            };
            let _ = self.ledger.push(upd);
        }
        Ok(())
    }

    /// 退役：Active/Linked→Retired（终态，不可再出边）。
    pub fn retire(&mut self) -> Result<(), u16> {
        self.state = step(self.state, LinkState::Retired)?;
        Ok(())
    }

    /// 当前状态机状态。
    pub fn state(&self) -> LinkState {
        self.state
    }

    /// 在账联动数。
    pub fn ledger_used(&self) -> usize {
        self.ledger.used()
    }
}

/// 自例程：构造样本走主路径（注册→登记→联动→激活）与错误路径
/// （缺场景/非法边/权重破恒等），全过返 Ok(0)，任一失手返 XcCode。
pub fn drill() -> Result<u32, u16> {
    let mut eng = SceneLinker::new();

    // 主路径：三个正交画像 + 两个场景。
    let p0 = eng.profiles.register(600, 300, 100, 0b0111)?;
    let p1 = eng.profiles.register(100, 700, 200, 0b0111)?;
    let p2 = eng.profiles.register(200, 200, 600, 0b0011)?;
    eng.put_scene(Scene { sid: 0, kind: SceneKind::Compute, pressure: 80 })?;
    eng.put_scene(Scene { sid: 1, kind: SceneKind::Media, pressure: 50 })?;

    let e0 = eng.link(0)?;
    // 压力 80 的 Compute：p1 base400+80*700/100=960 > p0 400+560=960（同分取 pid 小？不——
    // 400+80*300/100 = 640；重算：p0=400+80*300/100=640，p1=400+560=960，p2=400+160=560）。
    if e0.pid != p1 {
        return Err(XC_AFFINITY_RANGE);
    }
    let e1 = eng.link(1)?;
    // Media@50：p0=400+50*100/100=450，p1=400+100=500，p2=400+300=700 → p2。
    if e1.pid != p2 {
        return Err(XC_AFFINITY_RANGE);
    }
    eng.activate(0)?;
    if eng.state() != LinkState::Active || eng.activations != 1 {
        return Err(XC_ILLEGAL_EDGE);
    }
    if eng.ledger_used() < 2 {
        return Err(XC_LEDGER_FULL);
    }

    // 错误路径 1：未登记场景联动 → XC_SCENE_MISSING。
    if eng.link(9) != Err(XC_SCENE_MISSING) {
        return Err(XC_SCENE_MISSING);
    }
    // 错误路径 2：Retired 终态再转移 → XC_ILLEGAL_EDGE。
    let mut eng2 = SceneLinker::new();
    eng2.profiles.register(500, 400, 100, 0b0001)?;
    eng2.put_scene(Scene { sid: 0, kind: SceneKind::Render, pressure: 10 })?;
    eng2.link(0)?;
    eng2.retire()?;
    if eng2.retire() != Err(XC_ILLEGAL_EDGE) {
        return Err(XC_ILLEGAL_EDGE);
    }
    // 错误路径 3：权重破恒 → XC_AFFINITY_RANGE。
    let mut eng3 = SceneLinker::new();
    if eng3.profiles.register(500, 500, 500, 0b0001) != Err(XC_AFFINITY_RANGE) {
        return Err(XC_AFFINITY_RANGE);
    }
    // 错误路径 4：压力越界 → XC_AFFINITY_RANGE。
    if eng3.put_scene(Scene { sid: 0, kind: SceneKind::Render, pressure: 101 })
        != Err(XC_AFFINITY_RANGE)
    {
        return Err(XC_AFFINITY_RANGE);
    }

    let _ = p0;
    Ok(0)
}
