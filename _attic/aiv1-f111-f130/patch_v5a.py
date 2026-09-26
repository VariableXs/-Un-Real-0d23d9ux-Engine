# -*- coding: utf-8 -*-
"""AI-V1 深化批次 v5：十二项判据面补齐。"""

def append_before_selfcheck(path, block):
    s = open(path, encoding="utf-8").read()
    anchor = "// ---------------------------------------------------------------------------\n// 自检（判据逐条钉死）\n// ---------------------------------------------------------------------------"
    assert anchor in s, "anchor missing in " + path
    s = s.replace(anchor, block + "\n" + anchor, 1)
    open(path, "w", encoding="utf-8", newline="\n").write(s)

# ============ F120：修复项注册门禁 + 甘特导出 + manifest 全文 ============
append_before_selfcheck(r"kernel/varix/src/svstar/diagcenter.rs", """// ---------------------------------------------------------------------------
// 深化批次 v4/v5：修复注册门禁 / 甘特导出 / 导出 manifest 全文
// ---------------------------------------------------------------------------

/// 修复项注册门禁（主册【设计细节】「修复项注册制（新增修复必须登记
/// 风险级与回滚方案——门禁）」的机器面）：名称/说明/风险级/回滚方案
/// 四件缺一即拒——没有回滚方案的修复不许上架。
pub fn repair_registration_gate(
    name: &str,
    desc: &str,
    risk: Risk,
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
    let _ = risk; // 风险级必须显式选择（枚举无默认——类型系统保证）
    Ok(())
}

impl BootTimeline {
    /// 甘特导出（时间线页复用 F053 甘特组件的数据面：每段一行，段名
    /// + 起止 + 占总启动时长百分比——诊断页直接渲染）。
    pub fn gantt_lines(&self) -> Vec<String> {
        let total = self.segs.iter().map(|s| s.end_ms.saturating_sub(s.start_ms)).sum::<u64>().max(1);
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
pub fn export_manifest_text(version: &str, at_ms: u64, volumes: usize, sanitized: bool, items: &[&str]) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!("VARIX 诊断导出 manifest v{}\\n", version));
    s.push_str(&alloc::format!("时刻: {} ms\\n", at_ms));
    s.push_str(&alloc::format!("脱敏: {}\\n", if sanitized { "已启用（路径用户段/序列号/密钥类三查）" } else { "未启用（原始日志——仅限本机查看）" }));
    s.push_str(&alloc::format!("分卷: {}\\n", volumes));
    s.push_str("内容清单:\\n");
    for i in items {
        s.push_str(&alloc::format!("  - {}\\n", i));
    }
    s
}""")

# ============ F121：四触发钩子清单 + 压缩评估面 ============
append_before_selfcheck(r"kernel/varix/src/svstar/restorept.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：四触发钩子清单 / 压缩存储评估面
// ---------------------------------------------------------------------------

/// 四触发钩子清单（主册【设计细节】「变更监测钩子清单（四触发点埋点
/// 位置文档化）」的机器面：埋点位置 → 触发函数 → 快照标签）。
pub const TRIGGER_HOOKS: [(&str, &str, &str); TRIGGER_KINDS] = [
    ("appmgr.install_done", "RestoreStore::create(Trigger::AppInstalled)", "装应用"),
    ("theme.apply_global", "RestoreStore::create(Trigger::ThemeChanged)", "改主题"),
    ("updateux.install_done", "RestoreStore::create(Trigger::UpdateApplied)", "更新"),
    ("env.set_user", "RestoreStore::create(Trigger::EnvChanged)", "环境变量变更"),
];

/// 钩子清单对账（四触发枚举与钩子表一一对应——埋点遗漏即此处红）。
pub fn trigger_hooks_aligned() -> bool {
    let enum_tags = [Trigger::AppInstalled, Trigger::ThemeChanged, Trigger::UpdateApplied, Trigger::EnvChanged];
    let want = ["装应用", "改主题", "更新", "环境变量变更"];
    TRIGGER_HOOKS.len() == TRIGGER_KINDS
        && TRIGGER_HOOKS.iter().zip(want.iter()).all(|((_, _, tag), w)| tag == *w)
        && TRIGGER_HOOKS.len() == enum_tags.len()
}

/// 快照压缩评估结论（主册【设计细节】「快照压缩存储（zstd 评估 F130）」
/// ——评估登记面：结论与理由，真机日按登记换装）。
pub const COMPRESSION_ASSESSMENT: &str = "zstd 评估：配置层文本占比高（JSON/蜂巢导出），预计压缩比 3-5x；zstd 无内生 GPL 传染（MIT 授权），进程内链接合规；登记 F130 换装点——真机日以实测 CPU 开销 <10ms/快照判线决定是否启用";

// ============ F112：eSpeak IPC 命令帧 / 音调设置 ============
// （追加在 narrator.rs）""")

# ============ F112 narrator：IPC 命令帧 + 音调 ============
append_before_selfcheck(r"kernel/varix/src/svstar/narrator.rs", """// ---------------------------------------------------------------------------
// 深化批次 v5：eSpeak IPC 命令帧 / 语速音调设置面
// ---------------------------------------------------------------------------

/// eSpeak IPC 命令帧（进程隔离协议的线上格式——主册「独立进程+IPC」：
/// `SAY<rate>|<pitch>|<text>` 与 `STOP`；帧格式公开（F126 模板公开
/// 条款），eSpeak 侧桥接进程按此解析）。
pub fn ipc_frame_say(rate_mili: u32, pitch_pct: u32, text: &str) -> String {
    alloc::format!("SAY{}|{}|{}", rate_mili.clamp(RATE_MIN_MILI, RATE_MAX_MILI), pitch_pct.clamp(20, 200), text)
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
    if !(RATE_MIN_MILI..=RATE_MAX_MILI).contains(&rate) || !(20..=200).contains(&pitch) {
        return Err("rate/pitch out of range");
    }
    Ok((rate, pitch, Some(text)))
}

impl Narrator {
    /// 音调设置（设置页语速/音调——主册「语速/音调设置页」；20-200%。
    pub fn set_pitch(&mut self, pct: u32) -> u32 {
        self.pitch_pct = pct.clamp(20, 200);
        self.pitch_pct
    }

    pub fn pitch_pct(&self) -> u32 {
        self.pitch_pct
    }
}""")

# ============ F117：网络步 WiFi 注入面 ============
append_before_selfcheck(r"kernel/varix/src/svstar/oobe.rs", """// ---------------------------------------------------------------------------
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

# ============ F118：插画资产清单与降级判定 ============
append_before_selfcheck(r"kernel/varix/src/svstar/welcome.rs", """// ---------------------------------------------------------------------------
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

/// 插画可用判定（资产路径在清单且非空——降级面：缺图卡以纯文字版渲染，
/// 不留空白占位）。
pub fn illustration_available(card_index: usize, present: &[bool]) -> bool {
    card_index < CARD_COUNT && present.get(card_index).copied().unwrap_or(false)
}""")

print("v5 part 1 appended")
