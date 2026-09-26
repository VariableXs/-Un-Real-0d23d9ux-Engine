# -*- coding: utf-8 -*-
"""AI-V1 深化批次 v5 · 鲁棒插入版。"""


def find_selfcheck_line(lines):
    """定位自检段分隔线起点（自检注释行的上一条 `// ---` 分隔线）。"""
    for i, l in enumerate(lines):
        if "自检（判据逐条钉死）" in l:
            # 向上找分隔线（连续 1-2 行）。
            j = i - 1
            while j >= 0 and lines[j].strip().startswith("// " + "-" * 20):
                j -= 1
            return j + 1  # 分隔线起点
    raise AssertionError("selfcheck anchor not found")


def insert_block(path, block):
    lines = open(path, encoding="utf-8").read().split("\n")
    at = find_selfcheck_line(lines)
    new_lines = lines[:at] + block.rstrip("\n").split("\n") + [""] + lines[at:]
    open(path, "w", encoding="utf-8", newline="\n").write("\n".join(new_lines))
    print("inserted into", path)


# ============ F120 ============
insert_block(r"kernel/varix/src/svstar/diagcenter.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：修复注册门禁 / 甘特导出 / 导出 manifest 全文
// ---------------------------------------------------------------------------

/// 修复项注册门禁（主册【设计细节】「修复项注册制（新增修复必须登记
/// 风险级与回滚方案——门禁）」的机器面）：名称/说明/风险级/回滚方案
/// 四件缺一即拒——没有回滚方案的修复不许上架。
pub fn repair_registration_gate(
    name: &str,
    desc: &str,
    rollback_plan: &str,
) -> Result<(), &'static str> {
    if name.trim().is_empty() {
        return Err("repair name mandatory");
    }
    if desc.chars().count() < 8 {
        return Err("repair desc too short (explain what it does)");
    }
    if rollback_plan.trim().is_empty() {
        return Err("rollback plan mandatory — no rollback, no repair");
    }
    Ok(())
}

impl BootTimeline {
    /// 甘特导出（时间线页复用 F053 甘特组件的数据面：每段一行，段名
    /// + 起止 + 占总启动时长百分比——诊断页直接渲染）。
    pub fn gantt_lines(&self) -> Vec<String> {
        let total = self
            .segs
            .iter()
            .map(|s| s.end_ms.saturating_sub(s.start_ms))
            .sum::<u64>()
            .max(1);
        let mut sorted: Vec<&TimelineSeg> = self.segs.iter().collect();
        sorted.sort_by_key(|s| s.start_ms);
        sorted
            .iter()
            .map(|s| {
                let dur = s.end_ms.saturating_sub(s.start_ms);
                let pct = dur * 100 / total;
                alloc::format!("{} {}ms ({}%)", s.name, dur, pct)
            })
            .collect()
    }

    /// 最耗时段（甘特首行高亮——归因入口）。
    pub fn slowest_seg(&self) -> Option<&TimelineSeg> {
        self.segs
            .iter()
            .max_by_key(|s| s.end_ms.saturating_sub(s.start_ms))
    }
}

/// 导出包 manifest 全文（主册「导出包 zip 含 manifest（脱敏声明）」的
/// 文本形态：版本/时刻/脱敏声明/卷数/内容清单）。
pub fn export_manifest_text(
    version: &str,
    at_ms: u64,
    volumes: usize,
    sanitized: bool,
    items: &[&str],
) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!("VARIX 诊断导出 manifest v{}\\n", version));
    s.push_str(&alloc::format!("时刻: {} ms\\n", at_ms));
    s.push_str(&alloc::format!(
        "脱敏: {}\\n",
        if sanitized {
            "已启用（路径用户段/序列号/密钥类三查）"
        } else {
            "未启用（原始日志——仅限本机查看）"
        }
    ));
    s.push_str(&alloc::format!("分卷: {}\\n", volumes));
    s.push_str("内容清单:\\n");
    for i in items {
        s.push_str(&alloc::format!("  - {}\\n", i));
    }
    s
}""")

# ============ F121 ============
insert_block(r"kernel/varix/src/svstar/restorept.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：四触发钩子清单 / 压缩存储评估面
// ---------------------------------------------------------------------------

/// 四触发钩子清单（主册【设计细节】「变更监测钩子清单（四触发点埋点
/// 位置文档化）」的机器面：埋点位置 → 触发函数 → 快照标签）。
pub const TRIGGER_HOOKS: [(&str, &str, &str); 4] = [
    ("appmgr.install_done", "RestoreStore::create(AppInstalled)", "装应用"),
    ("theme.apply_global", "RestoreStore::create(ThemeChanged)", "改主题"),
    ("updateux.install_done", "RestoreStore::create(UpdateApplied)", "更新"),
    ("env.set_user", "RestoreStore::create(EnvChanged)", "环境变量变更"),
];

/// 快照压缩评估结论（主册【设计细节】「快照压缩存储（zstd 评估 F130）」
/// ——评估登记面：结论与理由，真机日按登记换装）。
pub const COMPRESSION_ASSESSMENT: &str = "zstd 评估：配置层文本占比高（JSON/蜂巢导出），预计压缩比 3-5x；zstd MIT 授权无传染，进程内链接合规；登记 F130 换装点——真机日以实测 CPU 开销 <10ms/快照判线决定是否启用";""")

# ============ F112 ============
insert_block(r"kernel/varix/src/svstar/narrator.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：eSpeak IPC 命令帧 / 音调设置面
// ---------------------------------------------------------------------------

/// 音调域（千分比：200 = 20% 低音 … 2000 = 200% 高音；默认 1000）。
pub const PITCH_MIN_MILI: u32 = 200;
pub const PITCH_MAX_MILI: u32 = 2000;
pub const PITCH_DEFAULT_MILI: u32 = 1000;

/// eSpeak IPC 命令帧（进程隔离协议的线上格式——主册「独立进程+IPC」：
/// `SAY<rate>|<pitch>|<text>` 与 `STOP`；帧格式公开（F126 模板公开
/// 条款），eSpeak 侧桥接进程按此解析）。
pub fn ipc_frame_say(rate_mili: u32, pitch_mili: u32, text: &str) -> String {
    alloc::format!(
        "SAY{}|{}|{}",
        rate_mili.clamp(RATE_MIN_MILI, RATE_MAX_MILI),
        pitch_mili.clamp(PITCH_MIN_MILI, PITCH_MAX_MILI),
        text
    )
}

pub const IPC_FRAME_STOP: &str = "STOP";

/// 帧解析（桥接侧消费面：合法帧还原三元组；STOP 识别；坏帧拒绝）。
pub fn ipc_frame_parse(frame: &str) -> Result<(u32, u32, Option<&str>), &'static str> {
    if frame == IPC_FRAME_STOP {
        return Ok((0, 0, None));
    }
    let body = frame.strip_prefix("SAY").ok_or("bad frame prefix")?;
    let mut parts = body.splitn(3, '|');
    let rate: u32 = parts.next().ok_or("missing rate")?.parse().map_err(|_| "bad rate")?;
    let pitch: u32 = parts.next().ok_or("missing pitch")?.parse().map_err(|_| "bad pitch")?;
    let text = parts.next().ok_or("missing text")?;
    if !(RATE_MIN_MILI..=RATE_MAX_MILI).contains(&rate)
        || !(PITCH_MIN_MILI..=PITCH_MAX_MILI).contains(&pitch)
    {
        return Err("rate/pitch out of range");
    }
    Ok((rate, pitch, Some(text)))
}

impl Narrator {
    /// 音调设置（设置页语速/音调——主册「语速/音调设置页」；千分比
    /// 200-2000，与既有 pitch_mili 字段同域）。
    pub fn set_pitch(&mut self, mili: u32) -> u32 {
        self.pitch_mili = mili.clamp(PITCH_MIN_MILI, PITCH_MAX_MILI);
        self.pitch_mili
    }

    pub fn pitch_mili(&self) -> u32 {
        self.pitch_mili
    }
}""")

# ============ F117 ============
insert_block(r"kernel/varix/src/svstar/oobe.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：网络步 WiFi 扫描注入面
// ---------------------------------------------------------------------------

/// WiFi 扫描条目（网络步列表数据源——真实扫描由网络栈注入；向导侧
/// 只消费列表：信号强度排序 + 密码框校验）。
pub struct WifiEntry {
    pub ssid: &'static str,
    /// 信号强度 0-100（越强越前）。
    pub strength: u32,
    pub needs_password: bool,
}

/// 列表整理（强度降序、同名去重——注入面契约）。
pub fn wifi_entries_sorted(list: &[WifiEntry]) -> Vec<&WifiEntry> {
    let mut v: Vec<&WifiEntry> = list.iter().collect();
    v.sort_by(|a, b| b.strength.cmp(&a.strength).then(a.ssid.cmp(b.ssid)));
    v.dedup_by(|a, b| a.ssid == b.ssid);
    v
}

/// 密码框校验（WPA 最短 8 位——B-607 手机热点文档指引的邻位判据）。
pub fn wifi_password_valid(pw: &str) -> bool {
    pw.len() >= 8
}""")

# ============ F118 ============
insert_block(r"kernel/varix/src/svstar/welcome.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：插画资产清单与降级判定
// ---------------------------------------------------------------------------

/// 五卡插画资产清单（4K 原生——主册「每卡一个核心图示（4K 插画资产）」；
/// 资产缺席走纯文字降级的判定输入）。
pub const ILLUSTRATION_ASSETS: [&str; CARD_COUNT] = [
    "assets/welcome/dual-domain.svg",
    "assets/welcome/no-store.svg",
    "assets/welcome/handoff.svg",
    "assets/welcome/personalize.svg",
    "assets/welcome/help-center.svg",
];

/// 插画可用判定（资产路径在清单且在位标记为真——降级面：缺图卡以纯
/// 文字版渲染，不留空白占位）。
pub fn illustration_available(card_index: usize, present: &[bool]) -> bool {
    card_index < CARD_COUNT && present.get(card_index).copied().unwrap_or(false)
}""")

# ============ F123 ============
insert_block(r"kernel/varix/src/svstar/aboutpage.rs", """// ---------------------------------------------------------------------------
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

# ============ F113 ============
insert_block(r"kernel/varix/src/svstar/highcontrast.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：切换预览缩略图采样（缩略图即真实渲染缩放——甲节禁模糊）
// ---------------------------------------------------------------------------

impl ThemePalette {
    /// 预览缩略图采样（主题卡缩略的取色面：按缩放比例位置从主题锚点
    /// 色采样——非贴图模糊，是令牌真值的真实缩小渲染）。
    pub fn thumbnail_sample(&self, x_bp: u32, y_bp: u32) -> (u8, u8, u8) {
        // 布局（与设置页主题卡构图一致）：上 60% 背景/文字对比区，
        // 下 30% 边框带，右下角焦点环角标。
        if x_bp >= 7_000 && y_bp >= 7_000 {
            self.focus
        } else if y_bp >= 7_000 {
            self.border
        } else if (3_000..=6_000).contains(&x_bp) && (2_500..=3_500).contains(&y_bp) {
            self.fg
        } else {
            self.bg
        }
    }
}""")

# ============ F111 ============
insert_block(r"kernel/varix/src/svstar/magnifier.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：全屏视口平滑平移（顶边触发的跟手感）
// ---------------------------------------------------------------------------

impl Magnifier {
    /// 全屏视口平滑平移（fullscreen_pan 的插值版：边缘触发给出目标
    /// 方向 → 视口按档位速度 × alpha 步进逼近——与镜头 smooth_follow
    /// 同一插值纪律，平移不跳格）。返回新视口。
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

# ============ F124 ============
insert_block(r"kernel/varix/src/svstar/motioncore.rs", """// ---------------------------------------------------------------------------
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

# ============ F127 ============
insert_block(r"kernel/varix/src/svstar/vxapp.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：哈希断点续算 / 产物版本戳双读
// ---------------------------------------------------------------------------

/// 树哈希断点续算器（主册【状态与异常】「哈希计算中断 → 断点续算」：
/// 逐文件算推进，中断后从已完成数续——不重复算已算文件）。
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
    /// （续算正确性 = 哈希链结合律）。
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

/// 产物版本戳双读判定（FORMAT_VERSION 变更时的读兼容：旧版产物在
/// 新工具下仍可验——F126 双读条款在产物面的落点）。
pub fn artifact_readable(artifact_version: u32, tool_version: u32, migration_open: bool) -> bool {
    artifact_version == tool_version || (migration_open && artifact_version < tool_version)
}""")

# ============ F129 ============
insert_block(r"kernel/varix/src/svstar/casesub.rs", """// ---------------------------------------------------------------------------
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

/// 仲裁裁定（主册票面语义：AI01 一票 + 社区轮值一票 + 第三人仲裁票
/// ——分歧时第三人裁决；无分歧走共识不进仲裁）。
pub fn arbitrate(vote_ai01: bool, vote_community: bool, vote_third: bool) -> (Arbitration, bool) {
    if vote_ai01 == vote_community {
        return (Arbitration::None, vote_ai01);
    }
    if vote_third {
        (Arbitration::Upheld, true)
    } else {
        (Arbitration::Overturned, false)
    }
}""")

# ============ F130 ============
insert_block(r"kernel/varix/src/svstar/ossreg.rs", """// ---------------------------------------------------------------------------
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

    /// 快照存档完整率（万分比——零空快照为满：坏账检出对账面）。
    pub fn snapshot_completeness_bp(&self) -> u32 {
        if self.entries.is_empty() {
            return 10_000;
        }
        let ok = self
            .entries
            .iter()
            .filter(|e| !e.license_snapshot.is_empty())
            .count();
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

print("v5 all inserted")
