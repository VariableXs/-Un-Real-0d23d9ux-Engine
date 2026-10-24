//! VE-F3745 · 创作者包容
//!
//! 锚点：创作者包容（特殊需求创作者支持：运动障碍（语音输入创作/开关控制创作
//! （辅具创作通道（复用辅具协议跨域单源——辅具创作红线：创作工具辅具可达=身份
//! 通道（身份复述；视觉障碍（读屏创作辅助（语义化创作界面+音频预览（音频预览
//! （视觉创作听觉化——听觉化红线：视觉创作必须有非视觉反馈通道（非视觉断言
//! （预览音频化/参数播报；老年创作者（F3748 复用（复述；包容测试（特殊需求
//! 创作者实测参与（参与红线复述。
//!
//! 数据结构：辅具通道复用；非视觉断言；音频预览器；参与制度复述。
//! 错误路径与降级矩阵：辅具创作不可达→P0（复述红线实测）；非视觉缺失→断言拦截
//! （红线实测）；音频失真→校准（复述）；参与形式化→真参与（复述红线）。
//! 跨批对接点：F3149/F3156 单源复用声明；R01-R03 工具对端；辅具社区对端。
//!
//! 诊断码段：独占 0x88xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x88xx）
// ─────────────────────────────────────────────────────────────

/// F3745 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct I7Code(pub u16);

impl I7Code {
    /// 辅具创作不可达——P0 身份通道红线。
    pub const ASSIST_UNREACHABLE: I7Code = I7Code(0x8801);
    /// 非视觉反馈缺失——听觉化红线断言拦截。
    pub const NONVISUAL_MISS: I7Code = I7Code(0x8802);
    /// 音频失真——校准。
    pub const AUDIO_DISTORT: I7Code = I7Code(0x8803);
    /// 参与形式化——真参与红线。
    pub const PARTICIPATE_FAKE: I7Code = I7Code(0x8804);
    /// 辅具通道漂移——单源对账。
    pub const CHANNEL_DRIFT: I7Code = I7Code(0x8805);
    /// 参数未播报——语义化断言。
    pub const PARAM_UNANNOUNCED: I7Code = I7Code(0x8806);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8801 => "辅具创作不可达，P0 身份通道红线不通过。",
            0x8802 => "视觉创作缺非视觉反馈通道，听觉化红线断言拦截。",
            0x8803 => "音频预览失真，需校准后重验。",
            0x8804 => "包容测试参与形式化，真参与红线不通过。",
            0x8805 => "辅具通道与单源声明漂移，需对账。",
            0x8806 => "创作参数未播报，语义化断言不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 辅具创作通道（复用辅具协议跨域单源）
// ─────────────────────────────────────────────────────────────

/// 辅具类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssistKind {
    /// 语音输入创作。
    Voice,
    /// 开关控制创作。
    Switch,
    /// 读屏创作。
    ScreenReader,
    /// 眼动创作。
    EyeTrack,
    /// 放大/缩放。
    Zoom,
}

impl AssistKind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            AssistKind::Voice => 0,
            AssistKind::Switch => 1,
            AssistKind::ScreenReader => 2,
            AssistKind::EyeTrack => 3,
            AssistKind::Zoom => 4,
        }
    }
}

/// 辅具通道状态。
#[derive(Clone, Debug)]
pub struct AssistChannel {
    /// 辅具类型。
    pub kind: AssistKind,
    /// 是否可达创作工具。
    pub reachable: bool,
    /// 是否已做身份通道复述（单源）。
    pub identity_restated: bool,
    /// 单源标识。
    pub source_tag: u64,
}

impl AssistChannel {
    /// 构造。
    pub fn new(kind: AssistKind, source_tag: u64) -> AssistChannel {
        AssistChannel {
            kind,
            reachable: false,
            identity_restated: false,
            source_tag,
        }
    }

    /// P0 合规：可达 + 身份复述。
    pub fn p0_ok(&self) -> bool {
        self.reachable && self.identity_restated
    }

    /// 打通。
    pub fn open(&mut self) {
        self.reachable = true;
    }

    /// 完成身份复述。
    pub fn restate_identity(&mut self) {
        self.identity_restated = true;
    }
}

/// 辅具通道表。
#[derive(Clone, Debug, Default)]
pub struct AssistRoster {
    channels: Vec<AssistChannel>,
}

impl AssistRoster {
    /// 空表。
    pub fn new() -> AssistRoster {
        AssistRoster {
            channels: Vec::new(),
        }
    }

    /// 注册。
    pub fn enroll(&mut self, ch: AssistChannel) {
        self.channels.push(ch);
    }

    /// 取通道（可变）。
    pub fn get_mut(&mut self, kind: AssistKind) -> Option<&mut AssistChannel> {
        self.channels.iter_mut().find(|c| c.kind == kind)
    }

    /// 取通道（只读）。
    pub fn get(&self, kind: AssistKind) -> Option<&AssistChannel> {
        self.channels.iter().find(|c| c.kind == kind)
    }

    /// 扫描 P0 违规：不可达或未复述。
    pub fn scan_p0_gap(&self) -> Vec<AssistKind> {
        let mut bad = Vec::new();
        for c in self.channels.iter() {
            if !c.p0_ok() {
                bad.push(c.kind);
            }
        }
        bad
    }

    /// 可达数。
    pub fn reachable_count(&self) -> usize {
        self.channels.iter().filter(|c| c.reachable).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 非视觉断言与音频预览（听觉化红线）
// ─────────────────────────────────────────────────────────────

/// 非视觉反馈通道。
#[derive(Clone, Debug)]
pub struct NonVisualChannel {
    /// 预览音频化。
    pub audio_preview: bool,
    /// 参数播报。
    pub param_announce: bool,
    /// 操作反馈音频。
    pub op_feedback: bool,
    /// 音频质量分（0-100，低于 60 判失真）。
    pub quality: u8,
}

impl NonVisualChannel {
    /// 构造（默认全关）。
    pub fn new() -> NonVisualChannel {
        NonVisualChannel {
            audio_preview: false,
            param_announce: false,
            op_feedback: false,
            quality: 100,
        }
    }

    /// 听觉化红线：三通道全开。
    pub fn aural_ok(&self) -> bool {
        self.audio_preview && self.param_announce && self.op_feedback
    }

    /// 音频失真。
    pub fn distorted(&self) -> bool {
        self.quality < 60
    }

    /// 校准：质量拉回满值。
    pub fn calibrate(&mut self) -> u8 {
        self.quality = 100;
        self.quality
    }

    /// 缺失通道清单（名）。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if !self.audio_preview {
            m.push("audio_preview");
        }
        if !self.param_announce {
            m.push("param_announce");
        }
        if !self.op_feedback {
            m.push("op_feedback");
        }
        m
    }
}

impl Default for NonVisualChannel {
    fn default() -> NonVisualChannel {
        NonVisualChannel::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 参与制度（真参与红线）
// ─────────────────────────────────────────────────────────────

/// 单次参与记录。
#[derive(Clone, Debug)]
pub struct Participation {
    /// 参与者类别（特殊需求创作者标识）。
    pub cohort: u8,
    /// 是否实测（形式化=读文档签字）。
    pub hands_on: bool,
    /// 反馈条数。
    pub feedback_count: u32,
}

impl Participation {
    /// 构造。
    pub fn new(cohort: u8, hands_on: bool) -> Participation {
        Participation {
            cohort,
            hands_on,
            feedback_count: 0,
        }
    }

    /// 真参与：实测 + 有反馈。
    pub fn real(&self) -> bool {
        self.hands_on && self.feedback_count > 0
    }
}

/// 参与台账。
#[derive(Clone, Debug, Default)]
pub struct InclusionLog {
    records: Vec<Participation>,
}

impl InclusionLog {
    /// 空台账。
    pub fn new() -> InclusionLog {
        InclusionLog {
            records: Vec::new(),
        }
    }

    /// 登记。
    pub fn record(&mut self, p: Participation) {
        self.records.push(p);
    }

    /// 补反馈。
    pub fn add_feedback(&mut self, cohort: u8, n: u32) -> bool {
        for r in self.records.iter_mut() {
            if r.cohort == cohort {
                r.feedback_count += n;
                return true;
            }
        }
        false
    }

    /// 扫描形式化参与。
    pub fn scan_fake(&self) -> Vec<u8> {
        let mut bad = Vec::new();
        for r in self.records.iter() {
            if !r.real() {
                bad.push(r.cohort);
            }
        }
        bad
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作者包容审查器。
#[derive(Clone, Debug, Default)]
pub struct InclusionAuditor {
    roster: AssistRoster,
    nonvis: NonVisualChannel,
    log: InclusionLog,
}

impl InclusionAuditor {
    /// 构造。
    pub fn new() -> InclusionAuditor {
        InclusionAuditor {
            roster: AssistRoster::new(),
            nonvis: NonVisualChannel::new(),
            log: InclusionLog::new(),
        }
    }

    /// 取通道表（可变）。
    pub fn roster_mut(&mut self) -> &mut AssistRoster {
        &mut self.roster
    }

    /// 取通道表。
    pub fn roster(&self) -> &AssistRoster {
        &self.roster
    }

    /// 取非视觉（可变）。
    pub fn nonvis_mut(&mut self) -> &mut NonVisualChannel {
        &mut self.nonvis
    }

    /// 取非视觉。
    pub fn nonvis(&self) -> &NonVisualChannel {
        &self.nonvis
    }

    /// 取参与台账（可变）。
    pub fn log_mut(&mut self) -> &mut InclusionLog {
        &mut self.log
    }

    /// 取参与台账。
    pub fn log(&self) -> &InclusionLog {
        &self.log
    }

    /// 全量审查：返回违规码。
    pub fn audit(&self) -> Vec<I7Code> {
        let mut bad = Vec::new();
        if !self.roster.scan_p0_gap().is_empty() {
            bad.push(I7Code::ASSIST_UNREACHABLE);
        }
        if !self.nonvis.aural_ok() {
            bad.push(I7Code::NONVISUAL_MISS);
        }
        if self.nonvis.distorted() {
            bad.push(I7Code::AUDIO_DISTORT);
        }
        if !self.log.scan_fake().is_empty() {
            bad.push(I7Code::PARTICIPATE_FAKE);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3745 判据集。
pub fn run_vr08e_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3745");

    // 0x8801 辅具 P0
    {
        let mut r = AssistRoster::new();
        r.enroll(AssistChannel::new(AssistKind::Voice, 1));
        out.add("0x8801-未达P0拦", r.scan_p0_gap().len() == 1, "");
        if let Some(c) = r.get_mut(AssistKind::Voice) {
            c.open();
            c.restate_identity();
        }
        out.add("0x8801-打通复述后过", r.scan_p0_gap().is_empty(), "");
    }

    // 0x8802 听觉化红线
    {
        let mut nv = NonVisualChannel::new();
        out.add("0x8802-缺三通道拦", !nv.aural_ok() && nv.missing().len() == 3, "");
        nv.audio_preview = true;
        nv.param_announce = true;
        nv.op_feedback = true;
        out.add("0x8802-三通道齐过", nv.aural_ok() && nv.missing().is_empty(), "");
    }

    // 0x8803 音频校准
    {
        let mut nv = NonVisualChannel::new();
        nv.quality = 40;
        out.add("0x8803-失真检出", nv.distorted(), "");
        nv.calibrate();
        out.add("0x8803-校准后过", !nv.distorted() && nv.quality == 100, "");
    }

    // 0x8804 真参与
    {
        let mut log = InclusionLog::new();
        log.record(Participation::new(1, true));
        out.add("0x8804-无反馈判形式化", log.scan_fake().len() == 1, "");
        log.add_feedback(1, 2);
        out.add("0x8804-补反馈后过", log.scan_fake().is_empty(), "");
    }

    // 0x8806 参数播报
    {
        let mut nv = NonVisualChannel::new();
        nv.param_announce = false;
        out.add(
            "0x8806-参数未播报",
            nv.missing().iter().any(|m| *m == "param_announce"),
            "",
        );
    }

    // 判据
    {
        let mut a = InclusionAuditor::new();
        a.roster_mut().enroll(AssistChannel::new(AssistKind::Switch, 2));
        a.nonvis_mut().quality = 50;
        let bad = a.audit();
        out.add("0x8801-总审检出P0", bad.iter().any(|c| c.raw() == 0x8801), "");
        out.add("0x8802-总审检出非视觉", bad.iter().any(|c| c.raw() == 0x8802), "");
    }

    out
}
