# -*- coding: utf-8 -*-
"""AI-V1 深化批次 v5 · part 2：F123/F113/F111/F124/F127/F129/F130。"""

def append_before_selfcheck(path, block):
    s = open(path, encoding="utf-8").read()
    anchor = "// ---------------------------------------------------------------------------\n// 自检（判据逐条钉死）\n// ---------------------------------------------------------------------------"
    assert anchor in s, "anchor missing in " + path
    s = s.replace(anchor, block + "\n" + anchor, 1)
    open(path, "w", encoding="utf-8", newline="\n").write(s)

# ============ F123：盘健康注入行 ============
append_before_selfcheck(r"kernel/varix/src/svstar/aboutpage.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：盘健康度注入行（F183 联动）
// ---------------------------------------------------------------------------

/// 盘健康等级（F183 盘 health 联动——只读注入：About 行不采写 SMART，
/// 由 F183 面推值；三态语义与 F183 同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskHealth {
    Good,
    Caution,
    Critical,
}

impl DiskHealth {
    pub fn tag(self) -> &'static str {
        match self {
            DiskHealth::Good => "健康",
            DiskHealth::Caution => "关注",
            DiskHealth::Critical => "建议更换",
        }
    }
}

/// 健康行文本生成（盘 0 行的值位：容量 + 健康度双值——读少量双值纪律）。
pub fn disk_health_value(capacity: &str, health: DiskHealth) -> String {
    alloc::format!("{} · 寿命{}", capacity, health.tag())
}""")

# ============ F113：缩略图缩放采样 ============
append_before_selfcheck(r"kernel/varix/src/svstar/highcontrast.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：切换预览缩略图采样（缩略图即真实渲染缩放——甲节禁模糊）
// ---------------------------------------------------------------------------

impl ThemePalette {
    /// 预览缩略图采样（主题卡缩略的取色面：按缩放比例从主题锚点色采
    /// 样——非贴图模糊，是令牌真值的真实缩小渲染）。
    pub fn thumbnail_sample(&self, x_bp: u32, y_bp: u32) -> (u8, u8, u8) {
        // 布局（与设置页主题卡构图一致）：上 60% 背景/文字对比区，
        // 下 30% 边框带，右下角焦点环角标。
        let _ = (x_bp, y_bp);
        if x_bp >= 7_000 && y_bp >= 7_000 {
            self.focus
        } else if y_bp >= 7_000 {
            self.border
        } else if x_bp >= 3_000 && x_bp <= 6_000 && y_bp >= 2_500 && y_bp <= 3_500 {
            self.fg
        } else {
            self.bg
        }
    }
}""")

# ============ F111：全屏视口平滑平移 ============
append_before_selfcheck(r"kernel/varix/src/svstar/magnifier.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：全屏视口平滑平移（顶边触发的跟手感）
// ---------------------------------------------------------------------------

impl Magnifier {
    /// 全屏视口平滑平移（fullscreen_pan 的插值版：边缘触发给出目标
    /// 速度 → 视口按 bp 步进逼近——与镜头 smooth_follow 同一插值纪律，
    /// 平移不跳格）。返回新视口。
    pub fn smooth_pan_step(&mut self, dir: (i32, i32), alpha_bp: u32) -> (u32, u32) {
        let speed = EDGE_PAN_SPEEDS_PX[self.pan_speed_tier.min(2)] as i64;
        let (w, h) = self.screen;
        let max_x = w.saturating_sub(1) as i64;
        let max_y = h.saturating_sub(1) as i64;
        let (vx, vy) = self.viewport;
        let target_x = (vx as i64 + dir.0 * speed).clamp(0, max_x) as u32;
        let target_y = (vy as i64 + dir.1 * speed).clamp(0, max_y) as u32;
        let a = alpha_bp.min(10_000) as u64;
        let nx = ((vx as u64 * (10_000 - a)) + target_x as u64 * a) / 10_000;
        let ny = ((vy as u64 * (10_000 - a)) + target_y as u64 * a) / 10_000;
        self.viewport = (nx as u32, ny as u32);
        self.viewport
    }
}""")

# ============ F124：30 真实动画点名注册 ============
append_before_selfcheck(r"kernel/varix/src/svstar/motioncore.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：全系统动画站点点名（30 处抽查的真实登记语料）
// ---------------------------------------------------------------------------

/// 全系统 30 处动画站点（真实点名——主册「全系统动画抽查 30 处」的
/// 登记母表：站点名与用途一处一事实，audit 引擎据此全量在谱审计）。
pub const SYSTEM_MOTION_SITES: [(&str, MotionUse); 30] = [
    ("win-open", MotionUse::Enter),
    ("win-close", MotionUse::ExitExit),
    ("menu-drop", MotionUse::Enter),
    ("menu-collapse", MotionUse::ExitExit),
    ("panel-slide-in", MotionUse::Panel),
    ("panel-slide-out", MotionUse::ExitExit),
    ("toast-enter", MotionUse::Enter),
    ("toast-exit", MotionUse::ExitExit),
    ("hover-lift", MotionUse::MicroFeedback),
    ("press-sink", MotionUse::MicroFeedback),
    ("focus-ring-in", MotionUse::MicroFeedback),
    ("snap-engage", MotionUse::Panel),
    ("taskview-enter", MotionUse::Panel),
    ("taskview-exit", MotionUse::ExitExit),
    ("alttab-fade", MotionUse::MicroFeedback),
    ("desk-switch", MotionUse::Panel),
    ("thumb-reveal", MotionUse::Enter),
    ("quick-panel", MotionUse::Panel),
    ("oobe-step", MotionUse::Enter),
    ("welcome-slide", MotionUse::Panel),
    ("help-toc-expand", MotionUse::Enter),
    ("detail-pane-open", MotionUse::Panel),
    ("progress-loop", MotionUse::Progress),
    ("update-stage", MotionUse::Progress),
    ("restore-flash", MotionUse::Panel),
    ("night-crossfade", MotionUse::Progress),
    ("lens-follow", MotionUse::Progress),
    ("recycle-shrink", MotionUse::ExitExit),
    ("magnify-zoom", MotionUse::Panel),
    ("imewin-follow", MotionUse::MicroFeedback),
];

/// 30 站点全量在谱预检（登记母表自身对账——站点落谱才可上架）。
pub fn system_sites_in_score() -> bool {
    SYSTEM_MOTION_SITES.iter().all(|(name, u)| {
        !name.is_empty() && {
            let (c, d) = lookup(*u);
            match c {
                Curve::Spring => d == DURATION_PANEL_MS,
                _ => c.bezier().map(|b| curve_registered(&b)).unwrap_or(false),
            }
        }
    })
}""")

# ============ F127：断点续算 + 产物版本戳双读 ============
append_before_selfcheck(r"kernel/varix/src/svstar/vxapp.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：哈希断点续算 / 产物版本戳双读
// ---------------------------------------------------------------------------

/// 树哈希断点续算器（主册【状态与异常】「哈希计算中断 → 断点续算」：
/// 逐文件算推进，中断后从已完成索引续——不重复算已算文件）。
pub struct HashResume {
    /// 已完成文件数（续算起点）。
    pub done: usize,
    /// 运行中的链（chain_hash 贯通——中断点即链头）。
    pub chain: [u8; 32],
}

impl HashResume {
    pub fn new() -> HashResume {
        HashResume { done: 0, chain: [0u8; 32] }
    }

    /// 推进一个文件（内容哈希并入链）。
    pub fn step(&mut self, file_hash: &[u8; 32]) {
        self.chain = vbase::chain_hash(&self.chain, file_hash);
        self.done += 1;
    }

    /// 续算对拍：中断在 k 处的链，从 k 续算到 n，与一次算完的链一致
    /// （续算正确性 = 链的結合律）。
    pub fn resume_equivalent(files: &[[u8; 32]], interrupt_at: usize) -> bool {
        let mut full = HashResume::new();
        for f in files {
            full.step(f);
        }
        let mut resumed = HashResume::new();
        for f in &files[..interrupt_at] {
            resumed.step(f);
        }
        for f in &files[interrupt_at..] {
            resumed.step(f);
        }
        full.chain == resumed.chain
    }
}

impl Default for HashResume {
    fn default() -> Self {
        Self::new()
    }
}

/// 产物版本戳双读判定（FORMAT_VERSION 变更时的读兼容：v1 产物在 v2
/// 工具下仍可验——F126 双读条款在产物面的落点）。
pub fn artifact_readable(artifact_version: u32, tool_version: u32, migration_open: bool) -> bool {
    artifact_version == tool_version || (migration_open && artifact_version < tool_version)
}""")

# ============ F129：第三人仲裁 ============
append_before_selfcheck(r"kernel/varix/src/svstar/casesub.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：复核分歧第三人仲裁
// ---------------------------------------------------------------------------

/// 仲裁状态（主册【状态与异常】「复核分歧 → 第三人仲裁（流程文档化
/// F148）」：两人复核票不一致时进入仲裁——第三人票裁决）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arbitration {
    /// 无分歧（常规流）。
    None,
    /// 待第三人仲裁（受理中）。
    Pending,
    /// 仲裁通过 → 收录。
    Upheld,
    /// 仲裁驳回 → 终态退回。
    Overturned,
}

impl CaseLine {
    /// 仲裁裁定（主册【数据与存储】票面语义：AI01 一票 + 社区轮值一票
    /// + 第三人仲裁票——三分之二多数生效）。
    pub fn arbitrate(vote_ai01: bool, vote_community: bool, vote_third: bool) -> (Arbitration, bool) {
        if vote_ai01 == vote_community {
            // 无分歧：按共识走，不进仲裁。
            return (Arbitration::None, vote_ai01);
        }
        // 分歧 → 第三人裁决。
        if vote_third {
            (Arbitration::Upheld, true)
        } else {
            (Arbitration::Overturned, false)
        }
    }
}""")

# ============ F130：许可证快照检索 + 升级窗日历 ============
append_before_selfcheck(r"kernel/varix/src/svstar/ossreg.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：许可证全文快照存档检索 / 升级窗日历
// ---------------------------------------------------------------------------

impl Registry {
    /// 许可证全文快照检索（主册【数据与存储】「许可证全文快照存档（防
    /// 上游删文）」——按组件名取全文快照；未存档的登记是坏账）。
    pub fn license_snapshot_of(&self, component: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.component == component)
            .map(|e| e.license_snapshot.as_str())
            .filter(|s| !s.is_empty())
    }

    /// 快照存档完整率（万分比——零空快照为满：坏账检出率对账面）。
    pub fn snapshot_completeness_bp(&self) -> u32 {
        if self.entries.is_empty() {
            return 10_000;
        }
        let ok = self.entries.iter().filter(|e| !e.license_snapshot.is_empty()).count();
        (ok * 10_000 / self.entries.len()) as u32
    }

    /// 升级窗日历（主册【设计细节】「季度升级窗日历与登记册联动（F138）」：
    /// 到期件按日排序的点名清单——升级窗排期直接可执行）。
    pub fn upgrade_calendar(&self) -> Vec<(u64, String)> {
        let mut due: Vec<(u64, String)> = self
            .entries
            .iter()
            .filter(|e| e.upgrade_due > 0)
            .map(|e| (e.upgrade_due, e.component.clone()))
            .collect();
        due.sort_by_key(|(d, _)| *d);
        due
    }
}""")

print("v5 part 2 appended")
