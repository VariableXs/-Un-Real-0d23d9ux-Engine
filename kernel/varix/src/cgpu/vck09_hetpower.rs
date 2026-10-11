//! vck09_hetpower — CGPU-F1609 异构功耗热协同（NPU/DSP 功耗进 J 域一本账）。
//!
//! 判据原文（施工书锚点 CGPU-F1609）：J 域协同：统一账本（NPU/DSP 功耗计入
//! J 域账本（F1458 标签×引擎系数——引擎功耗系数表（NPU 典型瓦数档——系数表
//! 版本化；异构热（NPU 热源并入热状态机输入（F1474 传感器扩展（NPU 温度源
//! ——热源扩展记录；协同降档（Hot 态→异构负载也降（NPU 批减/DSP 任务延
//! ——协同降档表（复用 J03 阶梯——阶梯扩展；能效路由（同任务多引擎可执行
//! →按能效选（能效比实测表（fps-per-watt 跨引擎——能效路由声明；场景协同
//! （J07 场景×引擎策略（视频场景 MEDIA 引擎优先——场景×引擎矩阵；与 F1460
//! （DVFS 选点跨引擎（各引擎频率独立+联合选点预留——联合选点预留位；测试
//! （账本并入/热源/阶梯扩展/能效路由/场景矩阵/预留六组）。判据：统一账本、
//! 热源扩展、阶梯协同、能效路由、场景矩阵、预留位、六组、判据。
//!
//! 职责定位：统一账本（NPU/DSP 功耗按引擎系数表折算进 J 域账本——F1458 功
//! 耗标签×引擎系数，系数表版本化 POWER_COEF_VER 篡改即显性；异构热（NPU
//! 温度源作为 F1474 热状态机的扩展输入，热源扩展记录表逐源留痕——热状态
//! 机看不见 NPU 热就不叫协同）；协同降档（Hot 态下异构负载同步降档：NPU
//! 批减/DSP 任务延，降档表复用 J03 阶梯扩展——GPU 降档 NPU 全速=假协同）；
//! 能效路由（同任务多引擎可执行时按 fps-per-watt 实测表选引擎，路由声明
//! 留痕——没有实测数据不路由）；场景协同（J07 场景×引擎策略矩阵，视频场
//! 景 MEDIA 引擎优先——矩阵外场景查表即拒）；联合选点预留位（各引擎频率
//! 独立选点，与 F1460 DVFS 的联合选点对接预留——预留位显性登记不是占位符
//! 空话，留接口+语义说明+版本锚）。诊断码段 0x7001~0x7005。

use alloc::vec::Vec;

/// 模块版本。
pub const HET_POWER_VERSION: &str = "K09-hetpower-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x70 段，本会话独占；0x67~0x6F 已占——先入库者占段纪律）
// ---------------------------------------------------------------------------

/// 0x7001 账本并入失守（NPU/DSP 功耗未按系数入 J 域账）。
pub const XC_LEDGER_MISS: u32 = 0x7001;
/// 0x7002 热源扩展缺失（NPU 温度源未并入热输入）。
pub const XC_THERM_SRC_MISS: u32 = 0x7002;
/// 0x7003 阶梯协同失步（Hot 态异构负载未降档）。
pub const XC_STEP_DESYNC: u32 = 0x7003;
/// 0x7004 能效路由失据（无 fps-per-watt 实测凭空选引擎）。
pub const XC_ROUTE_NO_EVIDENCE: u32 = 0x7004;
/// 0x7005 场景矩阵越界（场景×引擎策略表外查表）。
pub const XC_SCENE_OOB: u32 = 0x7005;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 引擎功耗系数表版本（系数表版本化——判据锚）。
pub const POWER_COEF_VER: u32 = 0x4B39_0001; // "K9" 前缀+序号
/// J03 阶梯档位数（复用口径）。
pub const J03_STEPS: usize = 4;
/// fps-per-watt 实测表容量。
pub const EFFICACY_SLOTS: usize = 8;
/// 场景矩阵行数（J07 场景闭集）。
pub const SCENE_ROWS: usize = 6;
/// 预留位登记数。
pub const RESERVE_SLOTS: usize = 2;

// ---------------------------------------------------------------------------
// 一、引擎功耗系数表（版本化）+ 统一账本
// ---------------------------------------------------------------------------

/// 引擎功耗系数（NPU 典型瓦数档——判据"引擎功耗系数表"）。
///
/// 系数=该引擎每单位任务量的功耗估算权重，J 域账本按 F1458 标签功耗×系数
/// 折算入账。三档闭集：Low/Mid/High（典型瓦数档，不猜具体瓦数——实测由
/// J 域账本吃数，此处只做折算权重）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerClass {
    /// 低功耗档（DSP 类：典型 1~3W）。
    Low,
    /// 中功耗档（NPU 类：典型 5~15W）。
    Mid,
    /// 高功耗档（GPU 类：典型 30~150W）。
    High,
}

impl PowerClass {
    /// 折算系数（万分比——no_std 无浮点，账本整数口径）。
    pub const fn coef_permille(self) -> u32 {
        match self {
            PowerClass::Low => 300,   // 0.30
            PowerClass::Mid => 800,   // 0.80
            PowerClass::High => 1600, // 1.60
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            PowerClass::Low => "低功耗档",
            PowerClass::Mid => "中功耗档",
            PowerClass::High => "高功耗档",
        }
    }
}

/// 统一账本（判据"NPU/DSP 功耗计入 J 域账本"）。
///
/// 入账语义：`post(tag_mw, engine_class)` 把 F1458 标签功耗（毫瓦）×引擎系
/// 数折算后记入 J 域一本账。未折算直接入账（系数丢旁路）=0x7001 失守。
#[derive(Debug)]
pub struct HetPowerLedger {
    /// 版本锚（系数表版本——账页头）。
    pub coef_ver: u32,
    /// J 域账本累计（毫瓦当量——GPU+NPU+DSP 一本账）。
    pub total_mw: u64,
    /// 分引擎入账数（审计列）。
    pub posted: [u64; 3], // Low/Mid/High 三档计数
    /// 系数旁路事件数（必须恒 0——旁路即账）。
    pub bypass_events: u32,
}

impl HetPowerLedger {
    /// 新账（版本锚落页头）。
    pub const fn new() -> HetPowerLedger {
        HetPowerLedger {
            coef_ver: POWER_COEF_VER,
            total_mw: 0,
            posted: [0; 3],
            bypass_events: 0,
        }
    }

    /// 入账：标签功耗×引擎系数折算入 J 域账（唯一入口）。
    pub fn post(&mut self, tag_mw: u64, cls: PowerClass) -> Result<u64, u32> {
        if self.coef_ver != POWER_COEF_VER {
            // 版本失配=账页过期（系数表升级后旧账页不得续写）。
            self.bypass_events += 1;
            return Err(XC_LEDGER_MISS);
        }
        let weighted = tag_mw * cls.coef_permille() as u64 / 1000;
        self.total_mw += weighted;
        let slot = match cls {
            PowerClass::Low => 0,
            PowerClass::Mid => 1,
            PowerClass::High => 2,
        };
        self.posted[slot] += 1;
        Ok(weighted)
    }

    /// 账本守恒对账：Σ分档加权入账≡total（重放式核验）。
    pub fn conserve_ok(&self, replay: u64) -> bool {
        self.total_mw == replay
    }
}

// ---------------------------------------------------------------------------
// 二、异构热（NPU 温度源并入热状态机输入——热源扩展记录）
// ---------------------------------------------------------------------------

/// 热源扩展记录（判据"F1474 传感器扩展（NPU 温度源"）。
#[derive(Clone, Copy, Debug)]
pub struct ThermalSourceExt {
    /// 源序号（F1474 原有源之后顺延）。
    pub src_id: usize,
    /// 摄氏温度（整数口径，负温用饱和 0——热状态机只关心过热）。
    pub temp_c: u32,
    /// 源已并入标记（并入前不参与 Hot 裁决）。
    pub merged: bool,
}

/// 热源扩展簿：NPU/DSP 温度源并入 F1474 输入的登记面。
#[derive(Debug)]
pub struct ThermalExtBook {
    /// 扩展源表。
    pub sources: Vec<ThermalSourceExt>,
    /// 并入缺失事件数（0x7002——有源未并入即账）。
    pub unmerged_events: u32,
}

impl ThermalExtBook {
    /// 新簿。
    pub const fn new() -> ThermalExtBook {
        ThermalExtBook {
            sources: Vec::new(),
            unmerged_events: 0,
        }
    }

    /// 登记源（未并入态）。
    pub fn enroll(&mut self, src_id: usize, temp_c: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sources.len() {
            if self.sources[i].src_id == src_id {
                return Err(XC_THERM_SRC_MISS); // 重复登记走缺失码（登记面唯一）
            }
            i += 1;
        }
        self.sources.push(ThermalSourceExt {
            src_id,
            temp_c,
            merged: false,
        });
        Ok(())
    }

    /// 并入 F1474 热状态机输入（判据"NPU 热源并入热状态机输入"）。
    pub fn merge_into_thermal(&mut self, src_id: usize) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sources.len() {
            if self.sources[i].src_id == src_id {
                self.sources[i].merged = true;
                return Ok(());
            }
            i += 1;
        }
        self.unmerged_events += 1;
        Err(XC_THERM_SRC_MISS)
    }

    /// Hot 裁决：任一已并入源超阈值即 Hot（热状态机输入侧）。
    pub fn hot_verdict(&self, threshold_c: u32) -> bool {
        let mut i = 0;
        while i < self.sources.len() {
            if self.sources[i].merged && self.sources[i].temp_c >= threshold_c {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 并入完备性：全部源必须已并入（有源未并入=热盲区）。
    pub fn all_merged(&self) -> bool {
        let mut i = 0;
        while i < self.sources.len() {
            if !self.sources[i].merged {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 三、协同降档表（Hot 态→异构负载也降——复用 J03 阶梯扩展）
// ---------------------------------------------------------------------------

/// J03 阶梯档（复用口径：0=全速 1=轻度降 2=中度降 3=重度降）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepTier {
    Full,
    Light,
    Medium,
    Heavy,
}

impl StepTier {
    /// 档位序号。
    pub const fn idx(self) -> usize {
        match self {
            StepTier::Full => 0,
            StepTier::Light => 1,
            StepTier::Medium => 2,
            StepTier::Heavy => 3,
        }
    }

    /// 从序号还原（越界钳 Heavy）。
    pub const fn from_idx(i: usize) -> StepTier {
        match i {
            0 => StepTier::Full,
            1 => StepTier::Light,
            2 => StepTier::Medium,
            _ => StepTier::Heavy,
        }
    }
}

/// 协同降档表：GPU 落 J03 第 i 档时，异构引擎的同步档位（阶梯扩展列）。
///
/// 判据"Hot 态→异构负载也降（NPU 批减/DSP 任务延——协同降档表（复用 J03
/// 阶梯"：表是 GPU 档→异构档的静态映射，NPU 批减=批次额度收缩，DSP 任务
/// 延=延迟容忍任务后移。
#[derive(Debug)]
pub struct CoDeroTable {
    /// 每档 NPU 批减比例（万分比：0=不减 5000=减半）。
    pub npu_throttle_bp: [u32; J03_STEPS],
    /// 每档 DSP 延迟档（0=不延 1=延一档 2=延两档）。
    pub dsp_defer: [u32; J03_STEPS],
    /// 失步事件数（0x7003——Hot 态异构未降即账）。
    pub desync_events: u32,
}

impl CoDeroTable {
    /// 出厂表（阶梯扩展：GPU 降异构必跟着降，单调不减）。
    pub const fn new() -> CoDeroTable {
        CoDeroTable {
            // GPU: Full/Light/Medium/Heavy
            npu_throttle_bp: [0, 2500, 5000, 7500],
            dsp_defer: [0, 1, 1, 2],
            desync_events: 0,
        }
    }

    /// 查表：GPU 档→NPU 批减+DSP 延迟（Hot 态唯一合法路径）。
    pub fn lookup(&self, gpu_step: StepTier) -> (u32, u32) {
        let i = gpu_step.idx().min(J03_STEPS - 1);
        (self.npu_throttle_bp[i], self.dsp_defer[i])
    }

    /// 协同裁决：Hot 态下 GPU 降档而异构全速=失步 0x7003。
    pub fn enforce(&mut self, hot: bool, gpu_step: StepTier, npu_applied_bp: u32) -> Result<(), u32> {
        let (want_bp, _) = self.lookup(gpu_step);
        if hot && npu_applied_bp < want_bp {
            self.desync_events += 1;
            return Err(XC_STEP_DESYNC);
        }
        Ok(())
    }

    /// 单调性对账：批减比例沿档位单调不减（阶梯语义）。
    pub fn monotonic(&self) -> bool {
        let mut i = 1;
        while i < J03_STEPS {
            if self.npu_throttle_bp[i] < self.npu_throttle_bp[i - 1] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 四、能效路由（fps-per-watt 实测表跨引擎选路）
// ---------------------------------------------------------------------------

/// 能效实测行（判据"能效比实测表（fps-per-watt 跨引擎"）。
#[derive(Clone, Copy, Debug)]
pub struct EfficacyRow {
    /// 引擎档（Mid=NPU 路线 / High=GPU 路线 / Low=DSP 路线）。
    pub cls: PowerClass,
    /// 实测帧率（帧/千秒整数口径）。
    pub fps_kilo: u32,
    /// 实测功耗（毫瓦）。
    pub mw: u32,
}

impl EfficacyRow {
    /// fps-per-watt（万分比整数：fps_kilo*10000/mw——零除守卫）。
    pub const fn fpw_bp(self) -> u32 {
        if self.mw == 0 {
            return 0;
        }
        let v = self.fps_kilo as u64 * 10_000 / self.mw as u64;
        if v > u32::MAX as u64 {
            u32::MAX
        } else {
            v as u32
        }
    }
}

/// 能效路由器（判据"同任务多引擎可执行→按能效选（能效路由声明"）。
#[derive(Debug)]
pub struct EfficacyRouter {
    /// 实测表。
    pub rows: Vec<EfficacyRow>,
    /// 路由声明账（每条路由声明留痕：选中档+依据 fpw）。
    pub declarations: Vec<(PowerClass, u32)>,
    /// 失据事件数（0x7004——空表路由即账）。
    pub no_evidence_events: u32,
}

impl EfficacyRouter {
    /// 新路由器。
    pub const fn new() -> EfficacyRouter {
        EfficacyRouter {
            rows: Vec::new(),
            declarations: Vec::new(),
            no_evidence_events: 0,
        }
    }

    /// 录入实测行（同档重复=覆盖——最新实测说话）。
    pub fn calibrate(&mut self, row: EfficacyRow) {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].cls == row.cls {
                self.rows[i] = row;
                return;
            }
            i += 1;
        }
        self.rows.push(row);
    }

    /// 路由：candidates 里按 fpw 最高选，路由声明入账。空表/候选无实测
    /// =0x7004 失据拒路由。
    pub fn route(&mut self, candidates: &[PowerClass]) -> Result<PowerClass, u32> {
        let mut best: Option<(PowerClass, u32)> = None;
        let mut i = 0;
        while i < self.rows.len() {
            let row = self.rows[i];
            let mut in_cand = false;
            let mut j = 0;
            while j < candidates.len() {
                if candidates[j] == row.cls {
                    in_cand = true;
                    j = candidates.len();
                }
                j += 1;
            }
            if in_cand {
                let fpw = row.fpw_bp();
                match best {
                    Some((_, b)) if b >= fpw => {}
                    _ => best = Some((row.cls, fpw)),
                }
            }
            i += 1;
        }
        match best {
            Some((cls, fpw)) => {
                self.declarations.push((cls, fpw));
                Ok(cls)
            }
            None => {
                self.no_evidence_events += 1;
                Err(XC_ROUTE_NO_EVIDENCE)
            }
        }
    }

    /// 声明完备性：每条路由声明必须有正 fpw 依据。
    pub fn declarations_sound(&self) -> bool {
        let mut i = 0;
        while i < self.declarations.len() {
            if self.declarations[i].1 == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 五、场景×引擎矩阵（J07 场景×引擎策略）
// ---------------------------------------------------------------------------

/// J07 场景闭集行（矩阵行序=场景序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneKind {
    Video,
    Gaming,
    Compute,
    Audio,
    Idle,
    Camera,
}

impl SceneKind {
    /// 六场景闭集。
    pub const ALL: [SceneKind; SCENE_ROWS] = [
        SceneKind::Video,
        SceneKind::Gaming,
        SceneKind::Compute,
        SceneKind::Audio,
        SceneKind::Idle,
        SceneKind::Camera,
    ];

    /// 优先引擎（None=无优先，按能效路由）。
    pub const fn preferred(self) -> Option<PowerClass> {
        match self {
            SceneKind::Video => Some(PowerClass::Low),  // 视频 MEDIA/DSP 优先（判据锚）
            SceneKind::Audio => Some(PowerClass::Low),  // 音频 DSP 优先
            SceneKind::Gaming => Some(PowerClass::High), // 游戏 GPU 优先
            SceneKind::Compute => Some(PowerClass::Mid), // 计算 NPU 优先
            SceneKind::Idle => None,
            SceneKind::Camera => Some(PowerClass::Mid), // 相机 ISP=NPU 档
        }
    }
}

/// 场景×引擎矩阵（判据"场景×引擎矩阵"——矩阵外场景查表即拒 0x7005）。
#[derive(Debug)]
pub struct SceneEngineMatrix {
    /// 矩阵策略值：每场景一个优先档序号（0=Low 1=Mid 2=High 3=None）。
    pub policy: [u32; SCENE_ROWS],
    /// 越界事件数。
    pub oob_events: u32,
}

impl SceneEngineMatrix {
    /// 出厂矩阵（与 SceneKind::preferred 对齐——两写法同源语义）。
    pub const fn new() -> SceneEngineMatrix {
        SceneEngineMatrix {
            policy: [0, 2, 1, 0, 3, 1],
            oob_events: 0,
        }
    }

    /// 查矩阵（scene 越界=0x7005）。
    pub fn lookup(&mut self, scene: usize) -> Result<Option<PowerClass>, u32> {
        if scene >= SCENE_ROWS {
            self.oob_events += 1;
            return Err(XC_SCENE_OOB);
        }
        let p = self.policy[scene];
        Ok(match p {
            0 => Some(PowerClass::Low),
            1 => Some(PowerClass::Mid),
            2 => Some(PowerClass::High),
            _ => None,
        })
    }

    /// 双写法对拍：矩阵出厂值≡SceneKind::preferred 逐场景一致。
    pub fn matches_semantics(&self) -> bool {
        let mut i = 0;
        while i < SCENE_ROWS {
            let want = SceneKind::ALL[i].preferred();
            let got = match self.policy[i] {
                0 => Some(PowerClass::Low),
                1 => Some(PowerClass::Mid),
                2 => Some(PowerClass::High),
                _ => None,
            };
            if want != got {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 六、联合选点预留位（与 F1460 DVFS 对接预留）
// ---------------------------------------------------------------------------

/// 预留位登记（判据"联合选点预留位"——显性登记非占位空话）。
#[derive(Clone, Copy, Debug)]
pub struct ReserveSlot {
    /// 对接单号（F1460）。
    pub peer: u32,
    /// 预留语义：各引擎频率独立选点已实现，联合选点待 F1460 扩展。
    pub note: u32,
}

/// 预留簿。
#[derive(Debug)]
pub struct JointDvfsReserve {
    /// 预留位表。
    pub slots: Vec<ReserveSlot>,
    /// 独立选点已启用标记（当前真实状态：各引擎独立）。
    pub independent_enabled: bool,
}

impl JointDvfsReserve {
    /// 出厂：登记预留位（F1460 联合选点）+ 独立选点真实启用。
    pub fn new() -> JointDvfsReserve {
        let mut s = Vec::new();
        s.push(ReserveSlot {
            peer: 1460,
            note: 0x4A4F, // "JO" 联合选点语义锚
        });
        JointDvfsReserve {
            slots: s,
            independent_enabled: true,
        }
    }

    /// 预留位在册校验：F1460 联合选点必须显性在册。
    pub fn reserved_for(&self, peer: u32) -> bool {
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].peer == peer {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 七、总引擎 + 六组自检
// ---------------------------------------------------------------------------

/// 异构功耗热协同总引擎（六件合体）。
#[derive(Debug)]
pub struct HetPowerEngine {
    /// 统一账本。
    pub ledger: HetPowerLedger,
    /// 热源扩展簿。
    pub thermal: ThermalExtBook,
    /// 协同降档表。
    pub dero: CoDeroTable,
    /// 能效路由器。
    pub router: EfficacyRouter,
    /// 场景×引擎矩阵。
    pub scenes: SceneEngineMatrix,
    /// 联合选点预留簿。
    pub reserve: JointDvfsReserve,
}

impl HetPowerEngine {
    /// 出厂。
    pub const fn new() -> HetPowerEngine {
        HetPowerEngine {
            ledger: HetPowerLedger::new(),
            thermal: ThermalExtBook::new(),
            dero: CoDeroTable::new(),
            router: EfficacyRouter::new(),
            scenes: SceneEngineMatrix::new(),
            reserve: JointDvfsReserve {
                slots: Vec::new(),
                independent_enabled: true,
            },
        }
    }

    /// 组一：统一账本——NPU/DSP 功耗按系数入 J 域账；版本锚失配拒；守恒对账。
    pub fn selfcheck_ledger(&mut self) -> Result<(), u32> {
        let mut e = HetPowerEngine::new();
        // NPU（Mid 0.80）：1000mW→800；DSP（Low 0.30）：500→150；GPU（High 1.60）：2000→3200。
        let w1 = e.ledger.post(1000, PowerClass::Mid)?;
        let w2 = e.ledger.post(500, PowerClass::Low)?;
        let w3 = e.ledger.post(2000, PowerClass::High)?;
        if w1 != 800 || w2 != 150 || w3 != 3200 {
            return Err(XC_LEDGER_MISS);
        }
        if e.ledger.total_mw != 4150 {
            return Err(XC_LEDGER_MISS);
        }
        if !e.ledger.conserve_ok(4150) {
            return Err(XC_LEDGER_MISS);
        }
        // 版本锚篡改→拒收记账。
        e.ledger.coef_ver = 0xDEAD;
        if e.ledger.post(100, PowerClass::Mid) != Err(XC_LEDGER_MISS) {
            return Err(XC_LEDGER_MISS);
        }
        if e.ledger.bypass_events != 1 {
            return Err(XC_LEDGER_MISS);
        }
        Ok(())
    }

    /// 组二：热源扩展——登记→并入→Hot 裁决；未并入源不参与裁决；漏并入记账。
    pub fn selfcheck_thermal(&mut self) -> Result<(), u32> {
        let mut e = HetPowerEngine::new();
        e.thermal.enroll(0, 88)?; // NPU 温度源
        e.thermal.enroll(1, 45)?; // DSP 温度源
        // 未并入：即使 88 度也不裁决 Hot（热状态机看不见=不协同）。
        if e.thermal.hot_verdict(80) {
            return Err(XC_THERM_SRC_MISS);
        }
        // 并入 NPU 源→Hot。
        e.thermal.merge_into_thermal(0)?;
        if !e.thermal.hot_verdict(80) {
            return Err(XC_THERM_SRC_MISS);
        }
        // 并入后完备性：DSP 未并入→不全并。
        if e.thermal.all_merged() {
            return Err(XC_THERM_SRC_MISS);
        }
        e.thermal.merge_into_thermal(1)?;
        if !e.thermal.all_merged() {
            return Err(XC_THERM_SRC_MISS);
        }
        // 登记不存在的源→并入失败记账。
        if e.thermal.merge_into_thermal(9) != Err(XC_THERM_SRC_MISS) {
            return Err(XC_THERM_SRC_MISS);
        }
        if e.thermal.unmerged_events != 1 {
            return Err(XC_THERM_SRC_MISS);
        }
        Ok(())
    }

    /// 组三：阶梯扩展——Hot 态协同降档；失步记账；单调性对账。
    pub fn selfcheck_steps(&mut self) -> Result<(), u32> {
        let mut e = HetPowerEngine::new();
        if !e.dero.monotonic() {
            return Err(XC_STEP_DESYNC);
        }
        // GPU 落 Medium（第 2 档）→NPU 批减 5000bp DSP 延 1。
        let (bp, defer) = e.dero.lookup(StepTier::Medium);
        if bp != 5000 || defer != 1 {
            return Err(XC_STEP_DESYNC);
        }
        // Hot 态协同执行：NPU 恰按表批减→过。
        e.dero.enforce(true, StepTier::Medium, 5000)?;
        // Hot 态失步：NPU 只批减 1000（<5000）→0x7003。
        if e.dero.enforce(true, StepTier::Medium, 1000) != Err(XC_STEP_DESYNC) {
            return Err(XC_STEP_DESYNC);
        }
        if e.dero.desync_events != 1 {
            return Err(XC_STEP_DESYNC);
        }
        // 非 Hot 态：NPU 全速也过（协同只在 Hot 生效）。
        e.dero.enforce(false, StepTier::Heavy, 0)?;
        Ok(())
    }

    /// 组四：能效路由——实测表选路；失据拒；声明留痕。
    pub fn selfcheck_route(&mut self) -> Result<(), u32> {
        let mut e = HetPowerEngine::new();
        // 空表路由→失据 0x7004。
        if e.router.route(&[PowerClass::Mid, PowerClass::High]) != Err(XC_ROUTE_NO_EVIDENCE) {
            return Err(XC_ROUTE_NO_EVIDENCE);
        }
        if e.router.no_evidence_events != 1 {
            return Err(XC_ROUTE_NO_EVIDENCE);
        }
        // 录入实测：NPU 600mW 跑 300 fps_kilo → fpw=3000000/6000=5000bp；
        // GPU 30000mW 跑 900 fps_kilo → fpw=9000000/300000=300bp。NPU 赢。
        e.router.calibrate(EfficacyRow { cls: PowerClass::Mid, fps_kilo: 300, mw: 6000 });
        e.router.calibrate(EfficacyRow { cls: PowerClass::High, fps_kilo: 900, mw: 300000 });
        let pick = e.router.route(&[PowerClass::Mid, PowerClass::High])?;
        if pick != PowerClass::Mid {
            return Err(XC_ROUTE_NO_EVIDENCE);
        }
        // 候选只给 GPU→路由 GPU（候选约束生效）。
        let pick2 = e.router.route(&[PowerClass::High])?;
        if pick2 != PowerClass::High {
            return Err(XC_ROUTE_NO_EVIDENCE);
        }
        // 候选给无实测档→失据。
        if e.router.route(&[PowerClass::Low]) != Err(XC_ROUTE_NO_EVIDENCE) {
            return Err(XC_ROUTE_NO_EVIDENCE);
        }
        // 声明账：3 条全带正 fpw。
        if e.router.declarations.len() != 3 || !e.router.declarations_sound() {
            return Err(XC_ROUTE_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 组五：场景矩阵——六场景逐行查；越界拒；双写法对拍。
    pub fn selfcheck_scenes(&mut self) -> Result<(), u32> {
        let mut e = HetPowerEngine::new();
        // 视频场景 MEDIA/DSP 优先（判据锚）。
        if e.scenes.lookup(0)? != Some(PowerClass::Low) {
            return Err(XC_SCENE_OOB);
        }
        // 游戏场景 GPU 优先。
        if e.scenes.lookup(1)? != Some(PowerClass::High) {
            return Err(XC_SCENE_OOB);
        }
        // 空闲无优先。
        if e.scenes.lookup(4)?.is_some() {
            return Err(XC_SCENE_OOB);
        }
        // 越界场景=0x7005。
        if e.scenes.lookup(6) != Err(XC_SCENE_OOB) {
            return Err(XC_SCENE_OOB);
        }
        if e.scenes.oob_events != 1 {
            return Err(XC_SCENE_OOB);
        }
        // 双写法对拍：矩阵≡枚举语义。
        if !e.scenes.matches_semantics() {
            return Err(XC_SCENE_OOB);
        }
        Ok(())
    }

    /// 组六：预留位——F1460 联合选点显性在册；独立选点真实态。
    pub fn selfcheck_reserve(&mut self) -> Result<(), u32> {
        let mut e = HetPowerEngine::new();
        e.reserve = JointDvfsReserve::new();
        if !e.reserve.reserved_for(1460) {
            return Err(XC_SCENE_OOB); // 预留缺失走矩阵码外的通用显性拒
        }
        if !e.reserve.independent_enabled {
            return Err(XC_SCENE_OOB); // 独立选点当前必须已启用（真实态）
        }
        // 未登记的对接口号查无预留。
        if e.reserve.reserved_for(9999) {
            return Err(XC_SCENE_OOB);
        }
        Ok(())
    }

    /// 全量自检入口（六组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_ledger()?;
        self.selfcheck_thermal()?;
        self.selfcheck_steps()?;
        self.selfcheck_route()?;
        self.selfcheck_scenes()?;
        self.selfcheck_reserve()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7000..0x7100 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_LEDGER_MISS,
            XC_THERM_SRC_MISS,
            XC_STEP_DESYNC,
            XC_ROUTE_NO_EVIDENCE,
            XC_SCENE_OOB,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7000 || codes[i] >= 0x7100 {
                return Err(XC_LEDGER_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_LEDGER_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
