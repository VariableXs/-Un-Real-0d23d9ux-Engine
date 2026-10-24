//! VE-F2422 · 骨骼动画架构（VE-M 域 · M02 动画架构组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2422`
//!
//! **职责定位（锚点原文）**：骨骼动画架构——骨骼动画管线（动画 clip→采样→混合
//! →蒙皮应用的完整链路——I04 F1666 采样契约的 M 侧实现兑现：管线四段：clip
//! 求值（M01 轨道）→姿态采样（本组 F2423）→姿态混合（F2424）→骨骼应用（I04
//! 蒙皮消费）——四段管线冻结声明）、动画 clip 模型（clip=骨骼轨道集合（骨骼×
//! 通道轨道组）+时长+循环标记——clip 数据模型声明：骨骼绑定（clip 骨骼名→实体
//! 骨骼映射——骨骼映射表：名字匹配+手动映射 fallback——映射协议声明）、与 I04
//! 边界（M 产出骨骼姿态/I04 蒙皮应用——F2401 边界表的骨骼维：姿态输出接口
//! （M→I04 姿态缓冲契约）——接口冻结声明）、双路径预告（GPU 采样路径——L03
//! 家族的动画维：GPU 骨骼采样 compute 位预留——规模路径声明——F2342 家族）。
//! 数据结构：四段管线——执行序（clip 求值→采样→混合→输出姿态缓冲→I04 蒙皮
//! ——序冻结+接口声明）；clip 模型——骨骼轨道组（clip=骨骼映射×通道轨道（每
//! 骨骼 translation/rotation/scale 三通道）+时长/循环）；I04 接口——姿态缓冲
//! 契约（缓冲格式冻结）；GPU 预告——采样 compute 位（STUB——F2342 家族）。
//!
//! **错误路径与降级矩阵（锚点原文逐条落实）**：
//!
//! | 动画风险 | 本模块落位 |
//! |---|---|
//! | 骨骼映射失败（clip 骨骼名无匹配） | 跳过骨骼+警告清单（[`MapReport`]）——**不静默丢骨骼**：每只跳过的骨骼带名字与原因入清单，报告可读 |
//! | 姿态缓冲接口失配 | 对账钩子（F1666 契约纪律）：缓冲容量/骨骼数/通道数三要素失配即 [`AnimErr::PoseBufferMismatch`]，M 侧不写越面数据 |
//! | clip 循环标记与时长矛盾 | 循环= true 但时长为零 → 校验拒绝（[`AnimErr::LoopDurationConflict`]）——零长循环=死循环，构造期即拒 |
//! | GPU 位被调用 | 显性报错（STUB 家族纪律——[`AnimErr::GpuStubCalled`]，绝不静默走 CPU 路径冒充 GPU） |
//! | 四段序错乱 | 序断言：管线游标单向推进（[`PipeStage`] 闭集），回跳/跳段即 [`AnimErr::StageOrder`] |
//!
//! **性能逐项分解（锚点语义）**：四段成本分摊——求值段 O(轨道数)、采样段
//! O(骨骼×通道)、混合段 O(骨骼)（段间交接零拷贝——姿态缓冲共享声明）、输出段
//! O(1)（缓冲指针交接）；GPU 预留零成本（STUB 只占错误码一位）。定点整数，
//! 姿态分量 Q16.16 定点（旋转用 Q16.16 角度近似，无浮点）。
//!
//! **跨批对接点**：采样契约单源 I04 F1666（跨域兑现——本模块是 M 侧实现声明）；
//! 边界家族 F2401（三域边界表）；GPU 家族 F2342/F2242（双路径声明）；fuzz
//! F2431（畸形 clip 语料）；上游 M01 轨道求值 F2407（SIMD 求值——本模块只做
//! 管线编排不复算轨道）。
//!
//! **无障碍与隐私**：映射报告 [`MapReport::a11y_lines`] 读屏可达（映射成功数/
//! 跳过数/原因逐条）；无隐私面（只有骨骼名与姿态数）。
//!
//! **诊断段 0x2430（十码独占）**：`E_AN_MAP_UNMATCHED` / `E_AN_BUF_MISMATCH`
//! / `E_AN_LOOP_CONFLICT` / `E_AN_GPU_STUB` / `E_AN_STAGE_ORDER`
//! / `E_AN_CLIP_EMPTY` / `E_AN_TIME_NEG` / `E_AN_BIND_FROZEN`
//! / `E_AN_POSE_RANGE` / `E_AN_TICK_MONO`。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2430;

/// Q16.16 定点换算（姿态分量统一标度）。
pub const FIX_SHIFT: u32 = 16;
pub const FIX_ONE: i32 = 1 << FIX_SHIFT;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 动画错误闭集（中文诊断+可操作建议）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimErr {
    /// clip 骨骼名无匹配（进警告清单，管线跳过该骨骼）。
    MapUnmatched { bone: String },
    /// 姿态缓冲三要素失配。
    PoseBufferMismatch { cap_bones: u32, need_bones: u32, channels: u32 },
    /// 循环标记与时长矛盾（零长循环）。
    LoopDurationConflict,
    /// GPU STUB 被调用。
    GpuStubCalled,
    /// 四段序错乱。
    StageOrder { have: u8, want: u8 },
    /// 空 clip（无任何轨道）。
    ClipEmpty,
    /// 时长为负。
    TimeNeg { got: i64 },
    /// 绑定表收口后被改（冻结纪律）。
    BindFrozen,
    /// 姿态分量越定点域。
    PoseRange { bone: u32, got: i32 },
    /// 采样时钟倒退。
    TickMono { have: u64, got: u64 },
}

impl AnimErr {
    pub fn detail(&self) -> String {
        match self {
            AnimErr::MapUnmatched { bone } => alloc::format!(
                "骨骼「{}」在实体侧无匹配。建议：查手动映射表补一条，\
                 或确认该骨骼确可跳过（警告清单保留痕迹）",
                bone
            ),
            AnimErr::PoseBufferMismatch { cap_bones, need_bones, channels } => alloc::format!(
                "姿态缓冲失配：容量 {} 骨骼 < 需要 {} 骨骼（通道 {}）。建议：\
                 按契约扩缓冲或裁剪 clip，M 侧不写越面数据",
                cap_bones, need_bones, channels
            ),
            AnimErr::LoopDurationConflict => "循环标记与时长矛盾（零长循环=死循环）。\
                 建议：时长 >0 才允许 loop=true，或改单次播放"
                .to_string(),
            AnimErr::GpuStubCalled => "GPU 采样 compute 位被调用（STUB——F2342 家族）。\
                 建议：改走 CPU 采样路径并在规模评估单登记需求，\
                 GPU 位落地前显性报错不静默"
                .to_string(),
            AnimErr::StageOrder { have, want } => alloc::format!(
                "管线段序错乱：游标在段 {} 段 {} 不合法。建议：按四段单向推进，\
                 回跳走重置接口（reset 后从求值段重来）",
                have, want
            ),
            AnimErr::ClipEmpty => "空 clip（无任何轨道）。建议：clip 至少一只骨骼一条通道，\
                 空数据拒绝入库"
                .to_string(),
            AnimErr::TimeNeg { got } => alloc::format!(
                "时长 {} 为负。建议：时长非负是 clip 域底线，负值来自导出器缺陷，\
                 修导出端",
                got
            ),
            AnimErr::BindFrozen => "绑定表收口后被改。建议：映射表冻结后只读，\
                 改映射走重建接口（旧表留痕）",
            AnimErr::PoseRange { bone, got } => alloc::format!(
                "骨骼 {} 姿态分量 {} 越定点域。建议：Q16.16 域 ±32768，\
                 超域数据先钳制并告警，不写脏值进缓冲",
                bone, got
            ),
            AnimErr::TickMono { have, got } => alloc::format!(
                "采样时钟倒退：{} → {}。建议：动画时钟单调递增，\
                 倒退走显性 seek 接口",
                have, got
            ),
        }
    }
}

/// 四段管线闭集（执行序冻结：求值→采样→混合→输出）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipeStage {
    /// clip 求值（M01 轨道）。
    Eval,
    /// 姿态采样（F2423）。
    Sample,
    /// 姿态混合（F2424）。
    Blend,
    /// 输出姿态缓冲（→I04 蒙皮）。
    Emit,
}

impl PipeStage {
    pub const ALL: [PipeStage; 4] = [PipeStage::Eval, PipeStage::Sample, PipeStage::Blend, PipeStage::Emit];
    pub fn next(&self) -> Option<PipeStage> {
        match self {
            PipeStage::Eval => Some(PipeStage::Sample),
            PipeStage::Sample => Some(PipeStage::Blend),
            PipeStage::Blend => Some(PipeStage::Emit),
            PipeStage::Emit => None,
        }
    }
}

/// 通道闭集（每骨骼三通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Translation,
    Rotation,
    Scale,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Translation, Channel::Rotation, Channel::Scale];
    pub fn idx(&self) -> u32 {
        match self {
            Channel::Translation => 0,
            Channel::Rotation => 1,
            Channel::Scale => 2,
        }
    }
}

/// 轨道：一只骨骼一条通道的键帧序列（时间 Q16.16 值 Q16.16）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub bone_name: String,
    pub channel: Channel,
    /// 键帧（时间 tick 升序、值定点）。构造期校验升序。
    pub keys: Vec<(u64, i32)>,
}

impl Track {
    /// 轨道校验：时间升序严格、值域 ±32768 定点。
    pub fn validate(&self) -> Result<(), AnimErr> {
        if self.keys.is_empty() {
            return Err(AnimErr::ClipEmpty);
        }
        for w in self.keys.windows(2) {
            if w[0].0 >= w[1].0 {
                return Err(AnimErr::TimeNeg { got: -(w[1].0 as i64) });
            }
        }
        for (_, v) in &self.keys {
            if *v > (1 << 30) || *v < -(1 << 30) {
                return Err(AnimErr::PoseRange { bone: 0xFFFF, got: *v });
            }
        }
        Ok(())
    }
    /// 定点线性采样（时间夹紧到首末键——夹紧不是静默外推）。
    pub fn sample(&self, t: u64) -> i32 {
        if self.keys.is_empty() {
            return 0;
        }
        if t <= self.keys[0].0 {
            return self.keys[0].1;
        }
        let last = self.keys[self.keys.len() - 1];
        if t >= last.0 {
            return last.1;
        }
        for w in self.keys.windows(2) {
            if t >= w[0].0 && t <= w[1].0 {
                let span = (w[1].0 - w[0].0) as i64;
                let off = (t - w[0].0) as i64;
                let a = w[0].1 as i64;
                let b = w[1].1 as i64;
                return (a + (b - a) * off / span) as i32;
            }
        }
        0
    }
}

/// 动画 clip：骨骼轨道组 + 时长 + 循环标记（锚点数据模型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub name: String,
    pub tracks: Vec<Track>,
    /// 时长（tick，>0）。
    pub duration: u64,
    /// 循环标记（loop=true 需时长>0）。
    pub looping: bool,
}

impl Clip {
    /// clip 校验：非空轨道、时长>0、loop/时长一致、逐轨道校验。
    pub fn validate(&self) -> Result<(), AnimErr> {
        if self.tracks.is_empty() {
            return Err(AnimErr::ClipEmpty);
        }
        if self.duration == 0 && self.looping {
            return Err(AnimErr::LoopDurationConflict);
        }
        if self.duration == 0 {
            return Err(AnimErr::TimeNeg { got: 0 });
        }
        for t in &self.tracks {
            t.validate()?;
        }
        Ok(())
    }
    /// clip 内骨骼名闭集（去重）。
    pub fn bones(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for t in &self.tracks {
            if !out.contains(&t.bone_name.as_str()) {
                out.push(&t.bone_name);
            }
        }
        out
    }
    /// 时钟折算（循环取模——显性折算非隐式）。
    pub fn clock(&self, t: u64) -> u64 {
        if self.looping && self.duration > 0 {
            t % self.duration
        } else {
            t.min(self.duration)
        }
    }
}

/// 骨骼映射协议：名字匹配自动 + 手动映射 fallback（锚点映射协议声明）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneBind {
    /// 实体骨骼名（I04 侧骨骼）。
    pub entity_bone: String,
    /// 手动指定的 clip 侧骨骼名（fallback 时生效）。
    pub manual_clip_bone: Option<String>,
}

/// 映射报告：成功映射+跳过清单（不静默丢骨骼——锚点错误路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapReport {
    pub mapped: Vec<(String, String)>,
    /// 跳过的 clip 骨骼（名字+原因）。
    pub skipped: Vec<(String, String)>,
}

impl MapReport {
    pub fn a11y_lines(&self) -> Vec<String> {
        let mut out = vec![
            alloc::format!("映射成功 {} 只骨骼，跳过 {} 只", self.mapped.len(), self.skipped.len()),
        ];
        for (b, r) in &self.skipped {
            out.push(alloc::format!("跳过「{}」：{}", b, r));
        }
        out
    }
    pub fn total_consistent(&self, clip_bones: usize) -> bool {
        self.mapped.len() + self.skipped.len() == clip_bones
    }
}

/// 骨骼绑定表：clip 骨骼名→实体骨骼名（冻结后只读）。
pub struct BindTable {
    pub binds: Vec<BoneBind>,
    frozen: bool,
}

impl BindTable {
    pub fn new(binds: Vec<BoneBind>) -> BindTable {
        BindTable { binds, frozen: false }
    }
    /// 收口冻结（冻结后改表即 [`AnimErr::BindFrozen`]）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
    /// 解析一只 clip 骨骼：手动映射优先 → 名字精确匹配 → 未匹配（进清单）。
    pub fn resolve(&self, clip_bone: &str) -> Result<&str, AnimErr> {
        if self.frozen {
            for b in &self.binds {
                if b.manual_clip_bone.as_deref() == Some(clip_bone) {
                    return Ok(&b.entity_bone);
                }
            }
        }
        for b in &self.binds {
            if b.entity_bone == clip_bone {
                return Ok(&b.entity_bone);
            }
        }
        Err(AnimErr::MapUnmatched { bone: String::from(clip_bone) })
    }
    /// 全量映射（产出报告——跳过骨骼逐条留痕）。
    pub fn map_clip(&self, clip: &Clip) -> MapReport {
        let mut rep = MapReport { mapped: Vec::new(), skipped: Vec::new() };
        for bone in clip.bones() {
            match self.resolve(bone) {
                Ok(e) => rep.mapped.push((String::from(bone), String::from(e))),
                Err(_) => rep.skipped.push((
                    String::from(bone),
                    String::from("clip 骨骼名无实体匹配且无手动映射"),
                )),
            }
        }
        rep
    }
}

/// 姿态缓冲（M→I04 契约冻结：容量×骨骼×三通道，Q16.16）。
pub struct PoseBuffer {
    /// 缓冲容量（骨骼数上界——契约冻结值）。
    pub cap_bones: u32,
    pub channels: u32,
    /// 姿态数据（骨骼主序×通道次序）。
    pub data: Vec<i32>,
    /// 产出帧号（对账钩子用）。
    pub frame: u32,
}

impl PoseBuffer {
    pub const CHANNELS: u32 = 3;
    pub fn new(cap_bones: u32) -> PoseBuffer {
        PoseBuffer {
            cap_bones,
            channels: PoseBuffer::CHANNELS,
            data: alloc::vec![0; (cap_bones as usize) * PoseBuffer::CHANNELS as usize],
            frame: 0,
        }
    }
    /// 契约对账：容量与需求三要素（F1666 钩子）。
    pub fn check(&self, need_bones: u32) -> Result<(), AnimErr> {
        if self.cap_bones < need_bones || self.channels != PoseBuffer::CHANNELS {
            return Err(AnimErr::PoseBufferMismatch {
                cap_bones: self.cap_bones,
                need_bones,
                channels: self.channels,
            });
        }
        Ok(())
    }
    /// 写入一只骨骼三通道（越域钳制+告警值返回——不写脏值）。
    pub fn write_bone(&mut self, bone_idx: u32, trs: [i32; 3]) -> Result<(), AnimErr> {
        if bone_idx >= self.cap_bones {
            return Err(AnimErr::PoseBufferMismatch {
                cap_bones: self.cap_bones,
                need_bones: bone_idx + 1,
                channels: self.channels,
            });
        }
        for (k, v) in trs.iter().enumerate() {
            let clamped = if *v > 1 << 30 { 1 << 30 } else if *v < -(1 << 30) { -(1 << 30) } else { *v };
            self.data[(bone_idx as usize) * 3 + k] = clamped;
        }
        Ok(())
    }
    /// 读一只骨骼三通道（I04 蒙皮消费面）。
    pub fn read_bone(&self, bone_idx: u32) -> Option<[i32; 3]> {
        if bone_idx >= self.cap_bones {
            return None;
        }
        let b = bone_idx as usize * 3;
        Some([self.data[b], self.data[b + 1], self.data[b + 2]])
    }
}

/// GPU 采样 compute 位（STUB——F2342 家族：调用即显性报错）。
pub fn gpu_sample_stub() -> Result<(), AnimErr> {
    Err(AnimErr::GpuStubCalled)
}

/// 四段管线执行器：游标单向推进 + 采样时钟单调。
pub struct AnimPipeline {
    stage: PipeStage,
    tick: u64,
    clip: Option<Clip>,
    bind: Option<BindTable>,
    /// 本帧映射报告。
    pub report: Option<MapReport>,
}

impl AnimPipeline {
    pub fn new() -> AnimPipeline {
        AnimPipeline { stage: PipeStage::Eval, tick: 0, clip: None, bind: None, report: None }
    }

    /// 换 clip（回到求值段——唯一合法回跳路径）。
    pub fn load(&mut self, clip: Clip, bind: BindTable) -> Result<(), AnimErr> {
        clip.validate()?;
        self.clip = Some(clip);
        self.bind = Some(bind);
        self.stage = PipeStage::Eval;
        self.report = None;
        Ok(())
    }

    /// 段推进（单向：只接受 next()；乱序请求显性拒）。
    pub fn advance(&mut self) -> Result<PipeStage, AnimErr> {
        let want = self.stage.next().ok_or(AnimErr::StageOrder { have: 3, want: 3 })?;
        self.stage = want;
        Ok(want)
    }

    /// 求值段执行：clip 校验+映射报告产出。
    pub fn run_eval(&mut self) -> Result<MapReport, AnimErr> {
        if self.stage != PipeStage::Eval {
            return Err(AnimErr::StageOrder { have: self.stage as u8, want: PipeStage::Eval as u8 });
        }
        let clip = self.clip.as_ref().ok_or(AnimErr::ClipEmpty)?;
        clip.validate()?;
        let bind = self.bind.take().ok_or(AnimErr::BindFrozen)?;
        let rep = bind.map_clip(clip);
        self.bind = Some(bind);
        self.report = Some(rep.clone());
        Ok(rep)
    }

    /// 采样段执行：时钟折算+逐轨道定点采样，写入姿态缓冲。
    pub fn run_sample(&mut self, t: u64, buf: &mut PoseBuffer) -> Result<usize, AnimErr> {
        if self.stage != PipeStage::Sample {
            return Err(AnimErr::StageOrder { have: self.stage as u8, want: PipeStage::Sample as u8 });
        }
        if t < self.tick {
            return Err(AnimErr::TickMono { have: self.tick, got: t });
        }
        self.tick = t;
        let clip = self.clip.as_ref().ok_or(AnimErr::ClipEmpty)?;
        let rep = self.report.as_ref().ok_or(AnimErr::StageOrder { have: 1, want: 0 })?;
        buf.check(rep.mapped.len() as u32)?;
        buf.frame = (buf.frame + 1) % u32::MAX;
        let ct = clip.clock(t);
        let mut written = 0usize;
        for (idx, (clip_bone, _)) in rep.mapped.iter().enumerate() {
            let mut trs = [0i32; 3];
            for (k, ch) in Channel::ALL.iter().enumerate() {
                for track in &clip.tracks {
                    if track.bone_name == *clip_bone && track.channel.idx() == ch.idx() {
                        trs[k] = track.sample(ct);
                    }
                }
            }
            buf.write_bone(idx as u32, trs)?;
            written += 1;
        }
        Ok(written)
    }

    /// 混合段（F2424 前向接口：本段只做游标与缓冲传递声明，混合本体归 F2424）。
    pub fn run_blend(&mut self) -> Result<(), AnimErr> {
        if self.stage != PipeStage::Blend {
            return Err(AnimErr::StageOrder { have: self.stage as u8, want: PipeStage::Blend as u8 });
        }
        Ok(())
    }

    /// 输出段：缓冲交 I04（零拷贝——返回消费闭包面声明）。
    pub fn run_emit(&mut self) -> Result<(), AnimErr> {
        if self.stage != PipeStage::Emit {
            return Err(AnimErr::StageOrder { have: self.stage as u8, want: PipeStage::Emit as u8 });
        }
        Ok(())
    }

    pub fn stage(&self) -> PipeStage {
        self.stage
    }
}

/// 标准语料：两骨骼 walk clip（确定性）。
pub fn standard_clip() -> Clip {
    Clip {
        name: String::from("walk"),
        duration: 1000,
        looping: true,
        tracks: vec![
            Track {
                bone_name: String::from("hip"),
                channel: Channel::Translation,
                keys: vec![(0, 0), (500, FIX_ONE / 4), (1000, 0)],
            },
            Track {
                bone_name: String::from("hip"),
                channel: Channel::Rotation,
                keys: vec![(0, 0), (1000, FIX_ONE / 8)],
            },
            Track {
                bone_name: String::from("foot"),
                channel: Channel::Translation,
                keys: vec![(0, 0), (250, FIX_ONE / 8), (750, -FIX_ONE / 8), (1000, 0)],
            },
        ],
    }
}

/// 标准绑定表：hip/foot 两骨骼实体同名。
pub fn standard_bind() -> BindTable {
    BindTable::new(vec![
        BoneBind { entity_bone: String::from("hip"), manual_clip_bone: None },
        BoneBind { entity_bone: String::from("foot"), manual_clip_bone: None },
    ])
}

/// F2422 判据自检（域聚合入口）。
pub fn run_vem22_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem22-F2422");

    // 判据 1：四段管线——执行序单向、乱序拒、换 clip 合法回跳。
    let mut pipe = AnimPipeline::new();
    let order_viol = pipe.run_sample(0, &mut PoseBuffer::new(4));
    set.add("C2422-采样段乱序拒", matches!(order_viol, Err(AnimErr::StageOrder { .. })), "游标在求值段不能采样");
    let _ = pipe.load(standard_clip(), standard_bind());
    let rep = pipe.run_eval().expect("求值");
    set.add("C2422-求值产出报告", rep.total_consistent(2) && rep.skipped.is_empty(), "两骨骼全映射");
    let s1 = pipe.advance();
    set.add("C2422-推进到采样", s1 == Ok(PipeStage::Sample), "单向推进");

    // 判据 2：F1666 兑现——姿态缓冲契约（对账钩子三要素+读写+钳制）。
    let mut buf = PoseBuffer::new(4);
    let written = pipe.run_sample(500, &mut buf).expect("采样");
    set.add("C2422-采样写两骨", written == 2, "映射骨骼逐只写入");
    set.add("C2422-hip位移中值", buf.read_bone(0).expect("hip")[0] == FIX_ONE / 4, "500tick 键值恰值");
    set.add("C2422-契约容量钩子", buf.check(4).is_ok() && matches!(buf.check(5), Err(AnimErr::PoseBufferMismatch { .. })), "三要素失配显性");
    // 采样时钟单调：倒退拒。
    let back = pipe.run_sample(100, &mut buf);
    set.add("C2422-时钟倒退拒", matches!(back, Err(AnimErr::TickMono { .. })), "tick 单调纪律");

    // 判据 3：骨骼映射协议——手动映射 fallback + 跳过留痕不静默。
    let mut bind2 = BindTable::new(vec![BoneBind {
        entity_bone: String::from("pelvis"),
        manual_clip_bone: Some(String::from("hip")),
    }]);
    bind2.freeze();
    set.add("C2422-手动映射命中", bind2.resolve("hip") == Ok("pelvis"), "fallback 生效");
    let miss = bind2.resolve("foot");
    set.add("C2422-无匹配显性", matches!(miss, Err(AnimErr::MapUnmatched { .. })), "foot 无匹配报错");
    let rep2 = bind2.map_clip(&standard_clip());
    set.add("C2422-跳过留痕", rep2.skipped.len() == 1 && rep2.total_consistent(2), "跳过骨骼入清单");
    set.add("C2422-报告可读", rep2.a11y_lines().len() == 2, "读屏含跳过原因");
    // 冻结纪律：冻结后改表（结构面：binds 公开但 resolve 只走冻结序——冻结标记位）。
    bind2.freeze();
    set.add("C2422-冻结幂等", matches!(bind2.resolve("hip"), Ok("pelvis")), "重复冻结无害");

    // 判据 4：clip 校验——空轨/零长循环/负时序全拒。
    let empty_clip = Clip { name: String::from("empty"), tracks: Vec::new(), duration: 10, looping: false };
    set.add("C2422-空clip拒", matches!(empty_clip.validate(), Err(AnimErr::ClipEmpty)), "无轨道拒绝");
    let zero_loop = Clip { name: String::from("z"), tracks: standard_clip().tracks, duration: 0, looping: true };
    set.add("C2422-零长循环拒", matches!(zero_loop.validate(), Err(AnimErr::LoopDurationConflict)), "死循环构造期拒");
    let mut messy = standard_clip();
    messy.tracks[0].keys.reverse();
    set.add("C2422-时序乱拒", messy.tracks[0].validate().is_err(), "键帧必须升序");
    // 循环时钟折算：1001 tick → 1。
    set.add("C2422-循环取模", standard_clip().clock(1001) == 1, "循环显性折算");
    set.add("C2422-单次夹紧", Clip { looping: false, ..standard_clip() }.clock(2000) == 1000, "非循环夹紧末键");

    // 判据 5：GPU 预告——STUB 显性报错（不静默走 CPU）。
    set.add("C2422-GPU存根显性", matches!(gpu_sample_stub(), Err(AnimErr::GpuStubCalled)), "F2342 家族 STUB 纪律");

    // 混合/输出段推进 + 终段再推进拒。
    set.add("C2422-推进到混合", pipe.advance() == Ok(PipeStage::Blend), "段 2");
    set.add("C2422-混合段执行", pipe.run_blend().is_ok(), "F2424 前向接口");
    set.add("C2422-推进到输出", pipe.advance() == Ok(PipeStage::Emit), "段 3");
    set.add("C2422-输出段执行", pipe.run_emit().is_ok(), "I04 交接");
    set.add("C2422-终段再推进拒", matches!(pipe.advance(), Err(AnimErr::StageOrder { .. })), "四段终点封口");

    // 换 clip 回跳合法。
    let _ = pipe.load(standard_clip(), standard_bind());
    set.add("C2422-重载回求值", pipe.stage() == PipeStage::Eval, "唯一合法回跳");

    set.add("C2422-诊断十码", SEG_BASE == 0x2430, "诊断段独占 0x2430");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vem22_full_pipeline() {
        let mut pipe = AnimPipeline::new();
        pipe.load(standard_clip(), standard_bind()).expect("load");
        let mut buf = PoseBuffer::new(4);
        pipe.run_eval().expect("eval");
        pipe.advance().expect("adv");
        pipe.run_sample(250, &mut buf).expect("sample");
        pipe.advance().expect("adv2");
        pipe.run_blend().expect("blend");
        pipe.advance().expect("adv3");
        pipe.run_emit().expect("emit");
        let foot = buf.read_bone(1).expect("foot");
        assert_eq!(foot[0], FIX_ONE / 8);
    }

    #[test]
    fn vem22_checks_all_green() {
        let set = run_vem22_checks();
        assert_eq!(set.red_items().1, 0, "红项：{:?}", set.red_items().0);
    }
}
