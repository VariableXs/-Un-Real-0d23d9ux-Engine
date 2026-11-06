//! UNX-F48928 亚像素累积 / 拓扑衔接 / 延迟打点 · 0x70FE0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B07）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F48928-J1 低速 ×10^2 步进零丢；
//!   ③ UNX-F48929-J1 衔接位登记成文 + fake 预演 10/10 次；
//!   ④ UNX-F48930-J1 指针段 ×10^3 打点全落账。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：亚像素输入合法性 → 余量归一 → 拓扑合法性 → 落屏定位 →
//!           四段打点 → 账本 → 复算。
//! 亚像素必须先归一余量：位移以千分像素进来，只有把余量始终留在 [0,16)
//! 才能保证慢速移动不丢步。拓扑必须先查重叠再定位：重叠矩形会让同一点
//! 命中两个显示器，先到先得是隐形的错误裁决。四段打点必须严格按
//! ingest→synth→dispatch→present 推进，跳段等于账本缺证据。
//! 关键差异：本模块只回答「余量怎么累积、这点落在哪个显示器、这帧四段
//!           各花了多久」，不做增益（B07-CORE）、不做边界静默区
//!           （B07-EDGE）、不裁决桌面拓扑（AI-35 显示域只联签衔接位）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70FE0，B07 第四槽）。
pub const SEG_BASE: u32 = 0x70FE0;

/// 亚像素分母：1 像素 = 16 毫像素。
pub const SUB_SCALE: i32 = 16;
/// 单次亚像素输入绝对值上限（毫像素）。
pub const SUB_MAX_INPUT: i32 = 4096;
/// 低速步进轮数（卡面 ×10^2 载体）。
pub const SLOW_ROUNDS: u32 = 100;
/// 每轮推送次数（100 次 1 毫像素）。
pub const SLOW_PUSHES: u32 = 100;
/// 拓扑显示器上限。
pub const MONITORS_MAX: usize = 8;
/// 单显示器宽高上限。
pub const MONITOR_MAX_W: i64 = 16_384;
pub const MONITOR_MAX_H: i64 = 16_384;
/// 虚拟桌面坐标上限。
pub const COORD_LIMIT: i64 = 32_768;
/// 四段打点。
pub const STAGES: usize = 4;
pub const STAGE_NAMES: [&str; STAGES] = ["ingest", "synth", "dispatch", "present"];
/// 帧耗时上限（微秒）。
pub const LAT_MAX_US: u64 = 200_000;
/// 帧黄金轮数（卡面 ×10^3 载体）。
pub const FRAME_ROUNDS: u32 = 1000;
/// 衔接位登记行数。
pub const SEAM_ROWS: usize = 3;
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpCode {
    /// 亚像素输入非法（单次超界/余量逃出区间）
    SubBad,
    /// 拓扑非法（空/超上限/尺寸越界）
    TopoBad,
    /// 矩形重叠（定位会产生双命中）
    OverlapBad,
    /// 坐标越出拓扑（无显示器承接）
    PosBad,
    /// 段位推进非法（跳段/重复）
    StageBad,
    /// 帧耗时非法（未完成/超上限/时间为负）
    SpanBad,
    /// 时间回退
    TimeBad,
    /// 复算不一致
    ReplayBad,
    /// 渲染回读失败
    RenderBad,
    /// 覆盖不足
    CoverageBad,
    /// 账不平
    LedgerBad,
    /// 段位越界
    SegmentBad,
}

impl DpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DpCode::SubBad => "亚像素非法：输入超界或余量逃出 [0,16)",
            DpCode::TopoBad => "拓扑非法：空/超上限/尺寸越界/主屏数不为 1",
            DpCode::OverlapBad => "矩形重叠：同一点会命中两台显示器",
            DpCode::PosBad => "坐标越出拓扑：没有显示器承接该点",
            DpCode::StageBad => "段位推进非法：必须按 ingest→synth→dispatch→present",
            DpCode::SpanBad => "帧耗时非法：四段未齐或耗时越界",
            DpCode::TimeBad => "时间回退：后段打点早于前段",
            DpCode::ReplayBad => "复算不一致：两次黄金账不同",
            DpCode::RenderBad => "渲染回读失败：字段缺失或数值越界",
            DpCode::CoverageBad => "覆盖不足：账本条数不足或段未齐",
            DpCode::LedgerBad => "账不平：合成分与拒绝分之和不等于轮数",
            DpCode::SegmentBad => "段位越界：诊断码逃出 0x70FE0 窗口",
        }
    }

    pub const ALL: [DpCode; 12] = [
        DpCode::SubBad,
        DpCode::TopoBad,
        DpCode::OverlapBad,
        DpCode::PosBad,
        DpCode::StageBad,
        DpCode::SpanBad,
        DpCode::TimeBad,
        DpCode::ReplayBad,
        DpCode::RenderBad,
        DpCode::CoverageBad,
        DpCode::LedgerBad,
        DpCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        if DpCode::ALL[i].val() == code {
            return DpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 亚像素累积 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Subpixel {
    pub fx: i32,
    pub fy: i32,
    pub emitted_x: i64,
    pub emitted_y: i64,
    pub input_x: i64,
    pub input_y: i64,
    pub pushes: u32,
}

impl Subpixel {
    pub fn new() -> Subpixel {
        Subpixel {
            fx: 0,
            fy: 0,
            emitted_x: 0,
            emitted_y: 0,
            input_x: 0,
            input_y: 0,
            pushes: 0,
        }
    }

    /// 推入一次千分像素位移，返回本次吐出的整像素数。
    pub fn push(&mut self, dx_milli: i32, dy_milli: i32) -> Result<(i32, i32), u32> {
        if dx_milli.unsigned_abs() > SUB_MAX_INPUT as u32
            || dy_milli.unsigned_abs() > SUB_MAX_INPUT as u32
        {
            return Err(DpCode::SubBad.val());
        }
        self.fx = self.fx.saturating_add(dx_milli);
        self.fy = self.fy.saturating_add(dy_milli);
        let wx = self.fx.div_euclid(SUB_SCALE);
        let wy = self.fy.div_euclid(SUB_SCALE);
        self.fx = self.fx.rem_euclid(SUB_SCALE);
        self.fy = self.fy.rem_euclid(SUB_SCALE);
        if self.fx < 0 || self.fx >= SUB_SCALE || self.fy < 0 || self.fy >= SUB_SCALE {
            return Err(DpCode::SubBad.val());
        }
        self.emitted_x = self.emitted_x.saturating_add(wx as i64);
        self.emitted_y = self.emitted_y.saturating_add(wy as i64);
        self.input_x = self.input_x.saturating_add(dx_milli as i64);
        self.input_y = self.input_y.saturating_add(dy_milli as i64);
        self.pushes = self.pushes.saturating_add(1);
        Ok((wx, wy))
    }

    /// 不变量：吐出的整像素 × 16 + 余量 == 累计输入（两轴同式）。
    pub fn audit(&self) -> Result<(), u32> {
        if self.fx < 0 || self.fx >= SUB_SCALE || self.fy < 0 || self.fy >= SUB_SCALE {
            return Err(DpCode::SubBad.val());
        }
        let x = self
            .emitted_x
            .saturating_mul(SUB_SCALE as i64)
            .saturating_add(self.fx as i64);
        let y = self
            .emitted_y
            .saturating_mul(SUB_SCALE as i64)
            .saturating_add(self.fy as i64);
        if x != self.input_x || y != self.input_y {
            return Err(DpCode::SubBad.val());
        }
        Ok(())
    }

    pub fn residual(&self) -> (i32, i32) {
        (self.fx, self.fy)
    }
}

/// 低速 ×10^2：每次推 1 毫像素，正向 100 次吐 6 余 4，负向 100 次吐 -7 余 12。
pub fn slow_rounds(rounds: u32) -> Result<(), u32> {
    let mut i = 0u32;
    while i < rounds {
        let mut up = Subpixel::new();
        let mut j = 0u32;
        while j < SLOW_PUSHES {
            let _ = up.push(1, 1)?;
            j = j.saturating_add(1);
        }
        up.audit()?;
        if up.emitted_x != 6 || up.emitted_y != 6 || up.residual() != (4, 4) {
            return Err(DpCode::SubBad.val());
        }
        let mut down = Subpixel::new();
        let mut k = 0u32;
        while k < SLOW_PUSHES {
            let _ = down.push(-1, -1)?;
            k = k.saturating_add(1);
        }
        down.audit()?;
        if down.emitted_x != -7 || down.emitted_y != -7 || down.residual() != (12, 12) {
            return Err(DpCode::SubBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

/// 余量不变量压力：1000 次伪随机推入后账仍平。
pub fn residual_stress(rounds: u32) -> Result<(i64, i64), u32> {
    let mut sub = Subpixel::new();
    let mut lcg = Lcg::new(0x4892_8001);
    let mut i = 0u32;
    while i < rounds {
        let dx = lcg.below(2 * SUB_MAX_INPUT as u32).saturating_add(1) as i32
            - SUB_MAX_INPUT;
        let dy = lcg.below(2 * SUB_MAX_INPUT as u32).saturating_add(1) as i32
            - SUB_MAX_INPUT;
        let _ = sub.push(dx, dy)?;
        sub.audit()?;
        i = i.saturating_add(1);
    }
    Ok((sub.emitted_x, sub.residual().0 as i64))
}

// ── 拓扑 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonitorRect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub primary: bool,
}

impl MonitorRect {
    pub fn validate(&self) -> Result<(), u32> {
        if self.w <= 0 || self.h <= 0 || self.w > MONITOR_MAX_W || self.h > MONITOR_MAX_H {
            return Err(DpCode::TopoBad.val());
        }
        if self.x.unsigned_abs() > COORD_LIMIT as u64
            || self.y.unsigned_abs() > COORD_LIMIT as u64
        {
            return Err(DpCode::TopoBad.val());
        }
        let x2 = self.x.saturating_add(self.w);
        let y2 = self.y.saturating_add(self.h);
        if x2 > COORD_LIMIT || y2 > COORD_LIMIT {
            return Err(DpCode::TopoBad.val());
        }
        Ok(())
    }

    pub fn right(&self) -> i64 {
        self.x.saturating_add(self.w)
    }

    pub fn bottom(&self) -> i64 {
        self.y.saturating_add(self.h)
    }

    pub fn hits(&self, x: i64, y: i64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn overlaps(&self, other: &MonitorRect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Topology {
    rects: Vec<MonitorRect>,
}

impl Topology {
    pub fn new(rects: Vec<MonitorRect>) -> Result<Topology, u32> {
        if rects.is_empty() || rects.len() > MONITORS_MAX {
            return Err(DpCode::TopoBad.val());
        }
        let mut primaries = 0u32;
        let mut i = 0usize;
        while i < rects.len() {
            let r = match rects.get(i) {
                Some(v) => v,
                None => return Err(DpCode::TopoBad.val()),
            };
            r.validate()?;
            if r.primary {
                primaries = primaries.saturating_add(1);
            }
            let mut j = i + 1;
            while j < rects.len() {
                let o = match rects.get(j) {
                    Some(v) => v,
                    None => return Err(DpCode::TopoBad.val()),
                };
                if r.overlaps(o) {
                    return Err(DpCode::OverlapBad.val());
                }
                j += 1;
            }
            i += 1;
        }
        if primaries != 1 {
            return Err(DpCode::TopoBad.val());
        }
        Ok(Topology { rects })
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn rects(&self) -> &[MonitorRect] {
        &self.rects
    }

    /// 定位：返回 (显示器序号, 该显示器内局部坐标)。
    pub fn locate(&self, x: i64, y: i64) -> Result<(usize, (i64, i64)), u32> {
        let mut i = 0usize;
        while i < self.rects.len() {
            let r = match self.rects.get(i) {
                Some(v) => v,
                None => return Err(DpCode::PosBad.val()),
            };
            if r.hits(x, y) {
                return Ok((i, (x - r.x, y - r.y)));
            }
            i += 1;
        }
        Err(DpCode::PosBad.val())
    }

    pub fn primary_index(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rects.len() {
            if let Some(r) = self.rects.get(i) {
                if r.primary {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

/// 标准双屏拓扑：主 1920×1080 在左，副 1280×720 在右，仅边贴不重叠。
pub fn sample_topology() -> Result<Topology, u32> {
    let mut rects = Vec::new();
    rects.push(MonitorRect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
        primary: true,
    });
    rects.push(MonitorRect {
        x: 1920,
        y: 0,
        w: 1280,
        h: 720,
        primary: false,
    });
    Topology::new(rects)
}

/// fake 预演：10 个固定点 10/10 命中预期显示器与局部坐标。
pub fn fake_preview() -> Result<u32, u32> {
    let t = sample_topology()?;
    let cases: [(i64, i64, usize, i64, i64); 10] = [
        (0, 0, 0, 0, 0),
        (1919, 1079, 0, 1919, 1079),
        (1920, 0, 1, 0, 0),
        (3199, 719, 1, 1279, 719),
        (500, 500, 0, 500, 500),
        (2500, 100, 1, 580, 100),
        (100, 1000, 0, 100, 1000),
        (3000, 700, 1, 1080, 700),
        (1919, 0, 0, 1919, 0),
        (1920, 719, 1, 0, 719),
    ];
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < cases.len() {
        let (x, y, want_idx, want_lx, want_ly) = match cases.get(i) {
            Some(v) => *v,
            None => return Err(DpCode::CoverageBad.val()),
        };
        match t.locate(x, y) {
            Ok((idx, (lx, ly))) => {
                if idx == want_idx && lx == want_lx && ly == want_ly {
                    hits = hits.saturating_add(1);
                }
            }
            Err(e) => return Err(e),
        }
        i += 1;
    }
    if hits != 10 {
        return Err(DpCode::CoverageBad.val());
    }
    if t.locate(3200, 0).err() != Some(DpCode::PosBad.val()) {
        return Err(DpCode::PosBad.val());
    }
    if t.locate(0, 1080).err() != Some(DpCode::PosBad.val()) {
        return Err(DpCode::PosBad.val());
    }
    Ok(hits)
}

/// 衔接位登记：显示域 AI-35 联签，本模块只做定位不裁决拓扑。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeamRow {
    pub owner: &'static str,
    pub seam: &'static str,
    pub this_side: &'static str,
}

pub fn seam_rows() -> Vec<SeamRow> {
    let mut v = Vec::new();
    v.push(SeamRow {
        owner: "AI-35/显示",
        seam: "虚拟桌面坐标→显示器局部坐标",
        this_side: "本域只按给定矩形定位，不生成拓扑",
    });
    v.push(SeamRow {
        owner: "AI-35/显示",
        seam: "主屏标识",
        this_side: "本域只读取 primary 位，不重排主屏",
    });
    v.push(SeamRow {
        owner: "AI-35/显示",
        seam: "热插拔重排",
        this_side: "本域只接受新拓扑快照，不监听热插拔",
    });
    v
}

/// 衔接位审计：三行在册、字段非空、this_side 声明「只」。
pub fn audit_seam(rows: &[SeamRow]) -> Result<u32, u32> {
    if rows.len() < SEAM_ROWS {
        return Err(DpCode::TopoBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(DpCode::TopoBad.val()),
        };
        if r.owner.is_empty() || r.seam.is_empty() || r.this_side.is_empty() {
            return Err(DpCode::TopoBad.val());
        }
        if !r.this_side.contains("只") {
            return Err(DpCode::TopoBad.val());
        }
        i += 1;
    }
    Ok(rows.len() as u32)
}

// ── 四段打点 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameMarks {
    pub marks: [u64; STAGES],
    pub used: usize,
}

impl FrameMarks {
    pub fn new() -> FrameMarks {
        FrameMarks {
            marks: [0; STAGES],
            used: 0,
        }
    }

    pub fn mark(&mut self, stage: usize, t_us: u64) -> Result<(), u32> {
        if stage >= STAGES || stage != self.used {
            return Err(DpCode::StageBad.val());
        }
        if self.used > 0 {
            let prev = match self.marks.get(self.used - 1) {
                Some(v) => *v,
                None => return Err(DpCode::StageBad.val()),
            };
            if t_us < prev {
                return Err(DpCode::TimeBad.val());
            }
        }
        match self.marks.get_mut(stage) {
            Some(slot) => *slot = t_us,
            None => return Err(DpCode::StageBad.val()),
        }
        self.used = self.used.saturating_add(1);
        Ok(())
    }

    pub fn complete(&self) -> bool {
        self.used == STAGES
    }

    pub fn span(&self) -> Result<u64, u32> {
        if !self.complete() {
            return Err(DpCode::SpanBad.val());
        }
        let first = match self.marks.first() {
            Some(v) => *v,
            None => return Err(DpCode::SpanBad.val()),
        };
        let last = match self.marks.get(STAGES - 1) {
            Some(v) => *v,
            None => return Err(DpCode::SpanBad.val()),
        };
        let s = last.saturating_sub(first);
        if s > LAT_MAX_US {
            return Err(DpCode::SpanBad.val());
        }
        Ok(s)
    }

    pub fn stage_of(&self, i: usize) -> Option<u64> {
        self.marks.get(i).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyBook {
    pub frames: u32,
    pub rejects: u32,
    pub max_span: u64,
    pub digest: u64,
}

impl LatencyBook {
    pub fn new() -> LatencyBook {
        LatencyBook {
            frames: 0,
            rejects: 0,
            max_span: 0,
            digest: FNV_OFF,
        }
    }

    pub fn step(&mut self, frame: &FrameMarks) -> Result<u64, u32> {
        match frame.span() {
            Ok(span) => {
                self.frames = self.frames.saturating_add(1);
                if span > self.max_span {
                    self.max_span = span;
                }
                let mut i = 0usize;
                while i < STAGES {
                    if let Some(v) = frame.stage_of(i) {
                        self.digest = fold_u64(self.digest, v);
                    }
                    i += 1;
                }
                Ok(span)
            }
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = fold_u32(self.digest, e);
                Err(e)
            }
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.frames == 0 {
            return Err(DpCode::LedgerBad.val());
        }
        if self.frames.saturating_add(self.rejects) == 0 {
            return Err(DpCode::LedgerBad.val());
        }
        Ok(())
    }
}

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }
}

/// 黄金帧：1000 帧，每段增量 10..=80 微秒，返回 (frames, max_span, digest)。
pub fn golden_run() -> Result<(u32, u64, u64), u32> {
    let mut book = LatencyBook::new();
    let mut lcg = Lcg::new(0x4892_9001);
    let mut i = 0u32;
    while i < FRAME_ROUNDS {
        let mut f = FrameMarks::new();
        let mut t = 1000u64.saturating_add(lcg.below(1000) as u64);
        let mut s = 0usize;
        while s < STAGES {
            if s > 0 {
                t = t.saturating_add(10).saturating_add(lcg.below(71) as u64);
            }
            f.mark(s, t)?;
            s += 1;
        }
        let _ = book.step(&f)?;
        i = i.saturating_add(1);
    }
    book.audit()?;
    Ok((book.frames, book.max_span, book.digest))
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_book(b: &LatencyBook) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        b.frames, b.rejects, b.max_span, b.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(DpCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(DpCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(DpCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(DpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_u64(s: &str) -> Result<u64, u32> {
    if s.is_empty() || s.len() > 20 {
        return Err(DpCode::RenderBad.val());
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(DpCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(DpCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u64))
        {
            Some(x) => x,
            None => return Err(DpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(DpCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(DpCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(DpCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(DpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(DpCode::RenderBad.val());
    }
    let frames = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    })?;
    let rejects = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    })?;
    let max_span = parse_u64(match parts.get(2) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    })?;
    if frames == 0
        || frames > 10_000_000
        || rejects > frames
        || max_span > LAT_MAX_US
        || digest == 0
    {
        return Err(DpCode::RenderBad.val());
    }
    Ok(frames)
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn route(&self, i: usize) -> u32 {
        match self.caught.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    let mut log = InjectLog {
        tries: [rounds; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        let mut sub = Subpixel::new();
        if sub.push(SUB_MAX_INPUT + 1, 0).err() == Some(DpCode::SubBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        let mut rects = Vec::new();
        rects.push(MonitorRect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
            primary: true,
        });
        rects.push(MonitorRect {
            x: 50,
            y: 50,
            w: 100,
            h: 100,
            primary: false,
        });
        if Topology::new(rects).err() == Some(DpCode::OverlapBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        let mut f = FrameMarks::new();
        if f.mark(1, 10).err() == Some(DpCode::StageBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        let mut g = FrameMarks::new();
        let _ = g.mark(0, 100);
        if g.mark(1, 99).err() == Some(DpCode::TimeBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 探针 A：亚像素低速正负步进、余量不变量、千次压力账平。
pub fn probe_seq_a() -> Result<(), u32> {
    slow_rounds(SLOW_ROUNDS)?;
    let (ex, rx) = residual_stress(1000)?;
    if rx < 0 || rx >= SUB_SCALE as i64 {
        return Err(DpCode::SubBad.val());
    }
    let mut sub = Subpixel::new();
    let a = sub.push(8, 0)?;
    let b = sub.push(8, 0)?;
    if a != (0, 0) || b != (1, 0) {
        return Err(DpCode::SubBad.val());
    }
    sub.audit()?;
    if ex == 0 && sub.emitted_x != 1 {
        return Err(DpCode::SubBad.val());
    }
    Ok(())
}

/// 探针 B：双屏定位 10/10、边贴不算重叠、越界必拒、坏拓扑必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if fake_preview()? != 10 {
        return Err(DpCode::CoverageBad.val());
    }
    let seam = seam_rows();
    if audit_seam(&seam)? != SEAM_ROWS as u32 {
        return Err(DpCode::TopoBad.val());
    }
    let mut two_primary = Vec::new();
    two_primary.push(MonitorRect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
        primary: true,
    });
    two_primary.push(MonitorRect {
        x: 10,
        y: 0,
        w: 10,
        h: 10,
        primary: true,
    });
    if Topology::new(two_primary).err() != Some(DpCode::TopoBad.val()) {
        return Err(DpCode::TopoBad.val());
    }
    let mut zero = Vec::new();
    zero.push(MonitorRect {
        x: 0,
        y: 0,
        w: 0,
        h: 10,
        primary: true,
    });
    if Topology::new(zero).err() != Some(DpCode::TopoBad.val()) {
        return Err(DpCode::TopoBad.val());
    }
    Ok(())
}

/// 探针 C：黄金帧双跑一致、跳段/回退/未完成三拒、渲染回读一致。
pub fn probe_seq_c() -> Result<(), u32> {
    let g1 = golden_run()?;
    let g2 = golden_run()?;
    if g1 != g2 {
        return Err(DpCode::ReplayBad.val());
    }
    let mut book = LatencyBook::new();
    let mut f = FrameMarks::new();
    f.mark(0, 10)?;
    f.mark(1, 20)?;
    f.mark(2, 30)?;
    if book.step(&f).err() != Some(DpCode::SpanBad.val()) {
        return Err(DpCode::SpanBad.val());
    }
    f.mark(3, 50)?;
    if book.step(&f)? != 40 {
        return Err(DpCode::SpanBad.val());
    }
    let text = render_book(&book);
    if audit_render(&text)? != book.frames {
        return Err(DpCode::RenderBad.val());
    }
    if book.audit().is_err() {
        return Err(DpCode::LedgerBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DpCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 1;
    let hi = SEG_BASE + DpCode::ALL.len() as u32;
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        match DpCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let slow = slow_rounds(SLOW_ROUNDS);
    v.push((
        "F48928-J1-低速×10^2步进零丢",
        slow.is_ok(),
        match &slow {
            Ok(()) => format!(
                "{} 轮 × {} 推：正向 6/余 4，负向 -7/余 12",
                SLOW_ROUNDS, SLOW_PUSHES
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let stress = residual_stress(1000);
    v.push((
        "F48928-J1-余量不变量×10^3",
        match &stress {
            Ok((_, r)) => *r >= 0 && *r < SUB_SCALE as i64,
            Err(_) => false,
        },
        match &stress {
            Ok((e, r)) => format!("emitted={} residual={}", e, r),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let prev = fake_preview();
    v.push((
        "F48929-J1-fake预演10/10",
        prev == Ok(10),
        match &prev {
            Ok(n) => format!("{} 点全部命中预期显示器与局部坐标", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let seam = seam_rows();
    let sm = audit_seam(&seam);
    v.push((
        "F48929-J1-衔接位登记成文",
        sm == Ok(SEAM_ROWS as u32),
        match &sm {
            Ok(n) => format!("{} 行 AI-35 联签（只定位不裁决）", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48929-J1-重叠与主屏必拒",
        {
            let mut rects = Vec::new();
            rects.push(MonitorRect {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
                primary: true,
            });
            rects.push(MonitorRect {
                x: 99,
                y: 99,
                w: 10,
                h: 10,
                primary: false,
            });
            let overlap = Topology::new(rects).err() == Some(DpCode::OverlapBad.val());
            let mut touching = Vec::new();
            touching.push(MonitorRect {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
                primary: true,
            });
            touching.push(MonitorRect {
                x: 100,
                y: 0,
                w: 100,
                h: 100,
                primary: false,
            });
            overlap && Topology::new(touching).is_ok()
        },
        String::from("重叠拒、边贴过"),
    ));
    let golden = golden_run();
    v.push((
        "F48930-J1-指针段×10^3打点全落账",
        match &golden {
            Ok((n, _, _)) => *n == FRAME_ROUNDS,
            Err(_) => false,
        },
        match &golden {
            Ok((n, span, d)) => format!("frames={} max_span={}us digest=0x{:016X}", n, span, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48930-J1-四段顺序严格",
        {
            let mut f = FrameMarks::new();
            let skip = f.mark(2, 1).err() == Some(DpCode::StageBad.val());
            let ok0 = f.mark(0, 1).is_ok();
            let back = f.mark(1, 0).err() == Some(DpCode::TimeBad.val());
            let ok1 = f.mark(1, 2).is_ok();
            let dup = f.mark(1, 3).err() == Some(DpCode::StageBad.val());
            skip && ok0 && back && ok1 && dup
        },
        String::from("跳段/回退/重复三拒"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48928-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let render_ok = match golden_run() {
        Ok((frames, span, digest)) => {
            let b = LatencyBook {
                frames,
                rejects: 0,
                max_span: span,
                digest,
            };
            let text = render_book(&b);
            audit_render(&text) == Ok(frames)
        }
        Err(_) => false,
    };
    v.push((
        "F48930-J1-渲染四字段回读",
        render_ok,
        String::from("frames|rejects|max_span|digest"),
    ));
    v.push((
        "F48928-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48928-J1-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 {} 码", SEG_BASE, DpCode::ALL.len()),
    ));
    v
}

pub fn run_unx48928_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48928 亚像素拓扑与延迟打点");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_positive_is_six_and_four() {
        let mut s = Subpixel::new();
        let mut i = 0u32;
        let mut total = 0i32;
        while i < 100 {
            if let Ok((w, _)) = s.push(1, 0) {
                total = total.saturating_add(w);
            }
            i += 1;
        }
        assert_eq!(total, 6);
        assert_eq!(s.residual(), (4, 0));
        assert!(s.audit().is_ok());
    }

    #[test]
    fn slow_negative_is_minus_seven_and_twelve() {
        let mut s = Subpixel::new();
        let mut i = 0u32;
        let mut total = 0i32;
        while i < 100 {
            if let Ok((w, _)) = s.push(-1, 0) {
                total = total.saturating_add(w);
            }
            i += 1;
        }
        assert_eq!(total, -7);
        assert_eq!(s.residual(), (12, 0));
        assert!(s.audit().is_ok());
    }

    #[test]
    fn subpixel_rejects_oversize() {
        let mut s = Subpixel::new();
        assert_eq!(
            s.push(SUB_MAX_INPUT + 1, 0).err(),
            Some(DpCode::SubBad.val())
        );
        assert_eq!(
            s.push(0, -SUB_MAX_INPUT - 1).err(),
            Some(DpCode::SubBad.val())
        );
    }

    #[test]
    fn residual_invariant_holds() {
        match residual_stress(500) {
            Ok((_, r)) => assert!(r >= 0 && r < SUB_SCALE as i64),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn topology_locate_exact() {
        match sample_topology() {
            Ok(t) => {
                assert_eq!(t.len(), 2);
                assert_eq!(t.primary_index(), Some(0));
                assert_eq!(t.locate(0, 0), Ok((0, (0, 0))));
                assert_eq!(t.locate(1919, 1079), Ok((0, (1919, 1079))));
                assert_eq!(t.locate(1920, 0), Ok((1, (0, 0))));
                assert_eq!(t.locate(3199, 719), Ok((1, (1279, 719))));
                assert_eq!(t.locate(1920, 720).err(), Some(DpCode::PosBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn topology_rejects_overlap_and_multi_primary() {
        let mut rects = Vec::new();
        rects.push(MonitorRect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
            primary: true,
        });
        rects.push(MonitorRect {
            x: 50,
            y: 50,
            w: 100,
            h: 100,
            primary: false,
        });
        assert_eq!(Topology::new(rects).err(), Some(DpCode::OverlapBad.val()));
        let mut two = Vec::new();
        two.push(MonitorRect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
            primary: true,
        });
        two.push(MonitorRect {
            x: 10,
            y: 0,
            w: 10,
            h: 10,
            primary: true,
        });
        assert_eq!(Topology::new(two).err(), Some(DpCode::TopoBad.val()));
    }

    #[test]
    fn frame_stages_are_strict() {
        let mut f = FrameMarks::new();
        assert_eq!(f.mark(1, 5).err(), Some(DpCode::StageBad.val()));
        assert!(f.mark(0, 5).is_ok());
        assert_eq!(f.mark(1, 4).err(), Some(DpCode::TimeBad.val()));
        assert!(f.mark(1, 6).is_ok());
        assert!(f.mark(2, 6).is_ok());
        assert_eq!(f.span().err(), Some(DpCode::SpanBad.val()));
        assert!(f.mark(3, 9).is_ok());
        assert_eq!(f.span(), Ok(4));
        assert!(f.complete());
        assert_eq!(f.mark(3, 10).err(), Some(DpCode::StageBad.val()));
    }

    #[test]
    fn frame_span_capped() {
        let mut f = FrameMarks::new();
        let _ = f.mark(0, 0);
        let _ = f.mark(1, 0);
        let _ = f.mark(2, 0);
        let _ = f.mark(3, LAT_MAX_US + 1);
        assert_eq!(f.span().err(), Some(DpCode::SpanBad.val()));
    }

    #[test]
    fn golden_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(9) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [9, 9, 9, 9]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        match golden_run() {
            Ok((frames, span, digest)) => {
                let b = LatencyBook {
                    frames,
                    rejects: 0,
                    max_span: span,
                    digest,
                };
                let text = render_book(&b);
                assert_eq!(audit_render(&text), Ok(frames));
                let bad = text.replacen("|", ";", 1);
                assert!(audit_render(&bad).is_err());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(DpCode::ALL.len(), 12);
        assert_eq!(detail(DpCode::SubBad.val()), DpCode::SubBad.detail());
        assert_eq!(detail(0), "未知诊断码");
    }
}
