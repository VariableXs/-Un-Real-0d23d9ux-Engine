//! F623 鼠标档案入 vxtheme · 完整设计（STAR I 主册 J-B 组）。
//!
//! **判据（主册原文）**：四件打包往返一致性；合法性校验三例（超速/
//! 超量/非法轨迹各一）；与 F391/F147 同源对账；回退链完整；超限降级
//! 清单如实呈现。
//!
//! **打包语义**：vxtheme 定制包（F391）扩容纳入鼠标行为层——指针方案
//! （原有）+ **速度曲线/滚轮档/侧键映射/手势库**（F601/F605/F615/F617
//! 四件）打包；换机导入主题包连手感一起复原。
//!
//! **合法性校验（包不能把系统调成不可用）**：
//! 1. **超速**：速度曲线增益上限（≤5000‰ 即 5×）越限 → 降回默认并
//!    如实列出；
//! 2. **超量**：手势库条数上限（64）越限 → 截断到上限并列出被截项；
//! 3. **非法轨迹**：手势方向编码必须是 8 方向码（U/D/L/R/1..4）、
//!    长度 1..=16——非法字符/超长 → 该手势剔除并列出。
//! 全部降级项进入 `DowngradeList`（超限降级清单如实呈现——不静默）。
//!
//! **同源对账**：包内 section 注册进 F391 的 section 目录 + F147 随身
//! 同步范围目录（两处登记同一 section 名——一处一事实对账）；
//! **回退链**：导入前快照 → 应用失败/用户反悔 → 整段还原（快照回放）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::fnv1a64;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 速度曲线增益上限（千分位，5×）。
pub const SPEED_GAIN_CAP_M: i64 = 5000;
/// 手势库条数上限。
pub const GESTURE_COUNT_CAP: usize = 64;
/// 单手势轨迹长度上限。
pub const TRAJECTORY_LEN_CAP: usize = 16;
/// vxtheme 鼠标 section 名（F391/F147 同源登记名）。
pub const VXTHEME_SECTION: &str = "mouse-behavior";

// ---------------------------------------------------------------------------
// 四件数据模型
// ---------------------------------------------------------------------------

/// 速度曲线（F601 接缝：曲线 id + 贝塞尔双控制点 + 增益上限）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeedCurveSpec {
    pub curve_id: String,
    /// 贝塞尔控制点 ×2（1/1000 定点 0..1000）。
    pub ctrl: [(i64, i64); 2],
    /// 用户自定义增益上限（千分位）。
    pub gain_cap_m: i64,
}

impl Default for SpeedCurveSpec {
    fn default() -> Self {
        SpeedCurveSpec { curve_id: String::from("linear"), ctrl: [(333, 0), (666, 1000)], gain_cap_m: 1000 }
    }
}

/// 滚轮档（F605 接缝：三档全局档 + 应用覆盖表）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WheelGearSpec {
    /// always-notch / always-smooth / per-app。
    pub global: String,
    /// 应用覆盖（app_id → notch/smooth）。
    pub overrides: Vec<(String, String)>,
}

impl Default for WheelGearSpec {
    fn default() -> Self {
        WheelGearSpec { global: String::from("per-app"), overrides: Vec::new() }
    }
}

/// 侧键映射（F615 接缝：XButton1/2 全局/应用级目标）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideKeySpec {
    /// (键位, 作用域[global|app_id], 目标动作)。
    pub bindings: Vec<(String, String, String)>,
}

impl Default for SideKeySpec {
    fn default() -> Self {
        SideKeySpec {
            bindings: alloc::vec![
                (String::from("x1"), String::from("global"), String::from("nav-back")),
                (String::from("x2"), String::from("global"), String::from("nav-forward")),
            ],
        }
    }
}

/// 手势库（F617 接缝：轨迹方向编码 → 动作）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GestureSpec {
    /// (手势名, 轨迹方向编码, 动作)。
    pub gestures: Vec<(String, String, String)>,
}

impl Default for GestureSpec {
    fn default() -> Self {
        GestureSpec {
            gestures: alloc::vec![
                (String::from("close-tab"), String::from("DR"), String::from("tab-close")),
                (String::from("new-tab"), String::from("D"), String::from("tab-new")),
            ],
        }
    }
}

/// 鼠标行为段（vxtheme 包内四件 + 指针方案名）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MouseBehaviorSection {
    pub pointer_scheme: String,
    pub speed: SpeedCurveSpec,
    pub wheel: WheelGearSpec,
    pub sidekeys: SideKeySpec,
    pub gestures: GestureSpec,
}

impl Default for MouseBehaviorSection {
    fn default() -> Self {
        MouseBehaviorSection {
            pointer_scheme: String::from("VARIX 默认指针"),
            speed: SpeedCurveSpec::default(),
            wheel: WheelGearSpec::default(),
            sidekeys: SideKeySpec::default(),
            gestures: GestureSpec::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// 序列化（往返一致性判据的载体；TLV 行式，确定性布局）
// ---------------------------------------------------------------------------

/// 序列化行为段（行式 TLV：key=value，`\n` 分隔；值内禁 `\n` 由
/// 写入侧转义校验）。
pub fn serialize_section(s: &MouseBehaviorSection) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("v=1\n");
    out.push_str(&alloc::format!("section={VXTHEME_SECTION}\n"));
    out.push_str(&alloc::format!("scheme={}\n", s.pointer_scheme));
    out.push_str(&alloc::format!(
        "speed={},{},{},{},{}\n",
        s.speed.curve_id, s.speed.ctrl[0].0, s.speed.ctrl[0].1, s.speed.ctrl[1].0, s.speed.ctrl[1].1
    ));
    out.push_str(&alloc::format!("speedcap={}\n", s.speed.gain_cap_m));
    out.push_str(&alloc::format!("wheel={}\n", s.wheel.global));
    for (app, gear) in &s.wheel.overrides {
        out.push_str(&alloc::format!("wheelov={app},{gear}\n"));
    }
    for (btn, scope, act) in &s.sidekeys.bindings {
        out.push_str(&alloc::format!("sidekey={btn},{scope},{act}\n"));
    }
    for (name, traj, act) in &s.gestures.gestures {
        out.push_str(&alloc::format!("gesture={name},{traj},{act}\n"));
    }
    out.into_bytes()
}

/// 反序列化（严格：未知行/缺头行报错——往返一致性以「序列化→反序列
/// 化→再序列化逐字节相等」为判）。
pub fn parse_section(d: &[u8]) -> Result<MouseBehaviorSection, String> {
    let text = core::str::from_utf8(d).map_err(|_| String::from("非 UTF-8 字节——包损坏"))?;
    let mut lines = text.lines();
    match lines.next() {
        Some("v=1") => {}
        _ => return Err(String::from("缺少版本头 v=1")),
    }
    match lines.next() {
        Some(l) if l == alloc::format!("section={VXTHEME_SECTION}") => {}
        _ => return Err(alloc::format!("缺少 section 头（应为 {VXTHEME_SECTION}）")),
    }
    let mut s = MouseBehaviorSection::default();
    s.speed.ctrl = [(333, 0), (666, 1000)]; // 恢复默认覆盖
    s.speed.curve_id = String::from("linear");
    s.speed.gain_cap_m = 1000;
    s.wheel = WheelGearSpec::default();
    s.sidekeys = SideKeySpec::default();
    s.gestures = GestureSpec::default();
    s.pointer_scheme = String::new();
    // 集合面清空（严格往返：包里有几条就是几条，不与默认值叠加）。
    s.wheel.overrides = Vec::new();
    s.sidekeys.bindings = Vec::new();
    s.gestures.gestures = Vec::new();
    for l in lines {
        let (k, v) = l.split_once('=').ok_or_else(|| alloc::format!("非法行：{l}"))?;
        match k {
            "scheme" => s.pointer_scheme = String::from(v),
            "speed" => {
                let p: Vec<&str> = v.split(',').collect();
                if p.len() != 5 {
                    return Err(alloc::format!("speed 行字段数不对：{l}"));
                }
                s.speed.curve_id = String::from(p[0]);
                s.speed.ctrl[0].0 = p[1].parse().map_err(|_| alloc::format!("speed 控制点非法：{l}"))?;
                s.speed.ctrl[0].1 = p[2].parse().map_err(|_| alloc::format!("speed 控制点非法：{l}"))?;
                s.speed.ctrl[1].0 = p[3].parse().map_err(|_| alloc::format!("speed 控制点非法：{l}"))?;
                s.speed.ctrl[1].1 = p[4].parse().map_err(|_| alloc::format!("speed 控制点非法：{l}"))?;
            }
            "speedcap" => s.speed.gain_cap_m = v.parse().map_err(|_| alloc::format!("speedcap 非法：{l}"))?,
            "wheel" => s.wheel.global = String::from(v),
            "wheelov" => {
                let p: Vec<&str> = v.split(',').collect();
                if p.len() != 2 {
                    return Err(alloc::format!("wheelov 行字段数不对：{l}"));
                }
                s.wheel.overrides.push((String::from(p[0]), String::from(p[1])));
            }
            "sidekey" => {
                let p: Vec<&str> = v.splitn(3, ',').collect();
                if p.len() != 3 {
                    return Err(alloc::format!("sidekey 行字段数不对：{l}"));
                }
                s.sidekeys.bindings.push((String::from(p[0]), String::from(p[1]), String::from(p[2])));
            }
            "gesture" => {
                let p: Vec<&str> = v.splitn(3, ',').collect();
                if p.len() != 3 {
                    return Err(alloc::format!("gesture 行字段数不对：{l}"));
                }
                s.gestures.gestures.push((String::from(p[0]), String::from(p[1]), String::from(p[2])));
            }
            other => return Err(alloc::format!("未知键「{other}」——拒绝静默跳过")),
        }
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// 合法性校验（超限降级清单）
// ---------------------------------------------------------------------------

/// 单条降级记录（如实呈现的面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downgrade {
    pub field: &'static str,
    pub why: &'static str,
    pub detail: String,
}

/// 校验 + 降级：返回（可用段, 降级清单）。原段不动（校验产出副本）。
pub fn validate_and_downgrade(raw: &MouseBehaviorSection) -> (MouseBehaviorSection, Vec<Downgrade>) {
    let mut s = raw.clone();
    let mut dg: Vec<Downgrade> = Vec::new();
    // 1. 超速：增益上限 > 5000‰ → 回默认 1000‰。
    if s.speed.gain_cap_m > SPEED_GAIN_CAP_M {
        dg.push(Downgrade {
            field: "speed.gain_cap",
            why: "速度增益超过 5× 上限——包不能把系统调成不可用",
            detail: alloc::format!("{}‰ → 回默认 1000‰", s.speed.gain_cap_m),
        });
        s.speed.gain_cap_m = 1000;
    }
    if s.speed.gain_cap_m < 100 {
        dg.push(Downgrade {
            field: "speed.gain_cap",
            why: "速度增益低于 0.1× 下限（指针不可用级迟钝）",
            detail: alloc::format!("{}‰ → 回默认 1000‰", s.speed.gain_cap_m),
        });
        s.speed.gain_cap_m = 1000;
    }
    // 2. 超量：手势条数 > 64 → 截断（被截项列出）。
    if s.gestures.gestures.len() > GESTURE_COUNT_CAP {
        let dropped: Vec<String> =
            s.gestures.gestures[GESTURE_COUNT_CAP..].iter().map(|g| g.0.clone()).collect();
        dg.push(Downgrade {
            field: "gestures.count",
            why: "手势库超过 64 条上限",
            detail: alloc::format!("截断 {} 条：{}", dropped.len(), dropped.join("、")),
        });
        s.gestures.gestures.truncate(GESTURE_COUNT_CAP);
    }
    // 3. 非法轨迹：8 方向码 + 长度 1..=16。
    let is_dir = |c: char| matches!(c, 'U' | 'D' | 'L' | 'R' | '1' | '2' | '3' | '4');
    let mut kept: Vec<(String, String, String)> = Vec::new();
    for (name, traj, act) in s.gestures.gestures.drain(..) {
        let legal_len = !traj.is_empty() && traj.chars().count() <= TRAJECTORY_LEN_CAP;
        let legal_chars = traj.chars().all(is_dir);
        if legal_len && legal_chars {
            kept.push((name, traj, act));
        } else {
            dg.push(Downgrade {
                field: "gestures.trajectory",
                why: "轨迹必须是 8 方向码（U/D/L/R/1-4）且长度 1..16",
                detail: alloc::format!("手势「{name}」轨迹「{traj}」剔除"),
            });
        }
    }
    s.gestures.gestures = kept;
    // 附带：滚轮档合法值域。
    if !matches!(s.wheel.global.as_str(), "always-notch" | "always-smooth" | "per-app") {
        dg.push(Downgrade {
            field: "wheel.global",
            why: "滚轮全局档不在合法值域（always-notch/always-smooth/per-app）",
            detail: alloc::format!("「{}」→ 回默认 per-app", s.wheel.global),
        });
        s.wheel.global = String::from("per-app");
    }
    (s, dg)
}

// ---------------------------------------------------------------------------
// 同源对账（F391 section 目录 + F147 同步范围）
// ---------------------------------------------------------------------------

/// 双目录登记（一处一事实：两处登记同一 section 名）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionRegistry {
    /// F391 vxtheme 包 section 目录。
    pub f391_sections: Vec<String>,
    /// F147 随身同步范围目录。
    pub f147_sync_scope: Vec<String>,
}

impl SectionRegistry {
    pub fn new() -> SectionRegistry {
        SectionRegistry { f391_sections: Vec::new(), f147_sync_scope: Vec::new() }
    }

    /// 登记鼠标行为段（幂等；两目录同步登记）。
    pub fn register_mouse_section(&mut self) -> bool {
        let mut changed = false;
        if !self.f391_sections.iter().any(|s| s == VXTHEME_SECTION) {
            self.f391_sections.push(String::from(VXTHEME_SECTION));
            changed = true;
        }
        if !self.f147_sync_scope.iter().any(|s| s == VXTHEME_SECTION) {
            self.f147_sync_scope.push(String::from(VXTHEME_SECTION));
            changed = true;
        }
        changed
    }

    /// 同源对账：两目录都含本 section（判据「与 F391/F147 同源对账」）。
    pub fn is_same_source(&self) -> bool {
        self.f391_sections.iter().any(|s| s == VXTHEME_SECTION)
            && self.f147_sync_scope.iter().any(|s| s == VXTHEME_SECTION)
    }
}

impl Default for SectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 导入与回退链
// ---------------------------------------------------------------------------

/// 导入执行器（应用状态面 + 快照回退）。
pub struct ThemeImporter {
    /// 当前生效段（None = 未应用过）。
    pub applied: Option<MouseBehaviorSection>,
    /// 回退快照栈（导入前快照——回退链完整）。
    snapshots: Vec<Option<MouseBehaviorSection>>,
    /// 回退记录。
    pub rollback_log: Vec<u64>,
    now_ms: u64,
}

impl ThemeImporter {
    pub fn new(now_ms: u64) -> ThemeImporter {
        ThemeImporter { applied: None, snapshots: Vec::new(), rollback_log: Vec::new(), now_ms }
    }

    /// 导入：校验降级 → 快照 → 应用。返回降级清单（如实呈现给用户）。
    pub fn import(&mut self, raw: &MouseBehaviorSection) -> Vec<Downgrade> {
        let (clean, dg) = validate_and_downgrade(raw);
        self.snapshots.push(self.applied.take());
        self.applied = Some(clean);
        dg
    }

    /// 回退（用户反悔/应用失败）——整段还原到导入前。
    pub fn rollback(&mut self) -> bool {
        let Some(prev) = self.snapshots.pop() else { return false };
        self.applied = prev;
        self.rollback_log.push(self.now_ms);
        true
    }

    pub fn rollback_count(&self) -> usize {
        self.rollback_log.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F623 自检。

// ---------------------------------------------------------------------------
// v2 深化：版本迁移 / 同步冲突差异报告
// ---------------------------------------------------------------------------

/// 字段级差异（同步冲突报告的行：字段路径 + 两边值的人话呈现）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionDiff {
    /// 字段路径（如 "speed.gain_cap_m"）。
    pub field: &'static str,
    pub local: String,
    pub incoming: String,
}

/// 本地段与包内段的字段级差异报告（F147 随身同步的冲突面：同步前
/// 先把"哪些会被覆盖"逐字段列出来——覆盖不是黑箱动作）。
pub fn diff_sections(local: &MouseBehaviorSection, incoming: &MouseBehaviorSection) -> Vec<SectionDiff> {
    let mut out = Vec::new();
    if local.pointer_scheme != incoming.pointer_scheme {
        out.push(SectionDiff {
            field: "pointer_scheme",
            local: local.pointer_scheme.clone(),
            incoming: incoming.pointer_scheme.clone(),
        });
    }
    if local.speed.curve_id != incoming.speed.curve_id {
        out.push(SectionDiff {
            field: "speed.curve_id",
            local: local.speed.curve_id.clone(),
            incoming: incoming.speed.curve_id.clone(),
        });
    }
    if local.speed.gain_cap_m != incoming.speed.gain_cap_m {
        out.push(SectionDiff {
            field: "speed.gain_cap_m",
            local: alloc::format!("{}", local.speed.gain_cap_m),
            incoming: alloc::format!("{}", incoming.speed.gain_cap_m),
        });
    }
    if local.wheel.global != incoming.wheel.global {
        out.push(SectionDiff {
            field: "wheel.global",
            local: local.wheel.global.clone(),
            incoming: incoming.wheel.global.clone(),
        });
    }
    if local.sidekeys != incoming.sidekeys {
        out.push(SectionDiff {
            field: "sidekeys.bindings",
            local: alloc::format!("{}条", local.sidekeys.bindings.len()),
            incoming: alloc::format!("{}条", incoming.sidekeys.bindings.len()),
        });
    }
    if local.gestures != incoming.gestures {
        out.push(SectionDiff {
            field: "gestures.gestures",
            local: alloc::format!("{}条", local.gestures.gestures.len()),
            incoming: alloc::format!("{}条", incoming.gestures.gestures.len()),
        });
    }
    out
}

/// 旧版段迁移（v1 首发的兼容面）：v1 段缺 v2 字段时以默认值补齐
/// ——迁移不是拒绝（旧包照常进，缺的如实补），迁移结果再走
/// validate_and_downgrade 的合法性闸。
pub fn migrate_legacy_v1(d: &[u8]) -> Result<(MouseBehaviorSection, Vec<&'static str>), String> {
    let text = core::str::from_utf8(d).map_err(|_| String::from("非 UTF-8 字节——包损坏"))?;
    let mut filled: Vec<&'static str> = Vec::new();
    // v1 与 v1 差异字段在当前模型里的呈现：v1 无手势库段。
    let has_gestures = text.lines().any(|l| l.starts_with("gesture="));
    let mut s = parse_section(d)?;
    if !has_gestures {
        s.gestures = GestureSpec::default();
        filled.push("gestures.gestures");
    }
    Ok((s, filled))
}

pub fn run_vtheme_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F623");
    let base = MouseBehaviorSection::default();

    // 1. 四件打包往返一致性：serialize → parse → serialize 逐字节相等。
    let bytes = serialize_section(&base);
    let parsed = parse_section(&bytes).expect("roundtrip parse");
    set.add(
        "four-item pack roundtrip byte-exact",
        serialize_section(&parsed) == bytes && parsed == base,
        "",
    );

    // 2. 合法性校验例①：超速（gain 8000‰）→ 降回默认 + 清单。
    let mut fast = base.clone();
    fast.speed.gain_cap_m = 8000;
    let (s1, dg1) = validate_and_downgrade(&fast);
    set.add(
        "over-speed downgraded to default",
        s1.speed.gain_cap_m == 1000
            && dg1.iter().any(|d| d.field == "speed.gain_cap" && d.detail.contains("8000")),
        "",
    );

    // 3. 例②：超量（70 手势）→ 截断 64 + 被截项列出。
    let mut many = base.clone();
    for i in 0..70 {
        many.gestures
            .gestures
            .push((alloc::format!("g{i}"), String::from("UDLR"), String::from("act")));
    }
    let (s2, dg2) = validate_and_downgrade(&many);
    set.add(
        "over-count truncated at 64 with list",
        s2.gestures.gestures.len() == 64
            && dg2.iter().any(|d| d.field == "gestures.count" && d.detail.contains("6")),
        "",
    );

    // 4. 例③：非法轨迹（坏字符/超长/空）→ 剔除并列出；合法保留。
    let mut bad = base.clone();
    bad.gestures.gestures.push((String::from("坏码"), String::from("DXZ"), String::from("a")));
    bad.gestures.gestures.push((String::from("超长"), String::from("U").repeat(17), String::from("a")));
    bad.gestures.gestures.push((String::from("合法"), String::from("URD2"), String::from("ok")));
    let (s3, dg3) = validate_and_downgrade(&bad);
    set.add(
        "illegal trajectories rejected, legal kept",
        s3.gestures.gestures.len() == 3
            && s3.gestures.gestures.iter().any(|g| g.0 == "合法")
            && dg3.iter().filter(|d| d.field == "gestures.trajectory").count() == 2,
        "",
    );

    // 5. 同源对账：双目录登记 + is_same_source。
    let mut reg = SectionRegistry::new();
    reg.register_mouse_section();
    set.add("F391/F147 same-source registry", reg.is_same_source(), "");

    // 6. 回退链完整：导入 → 快照 → 回退还原（含「从无到有」场景）。
    let mut imp = ThemeImporter::new(100);
    let mut custom = base.clone();
    custom.speed.gain_cap_m = 2000;
    let dg = imp.import(&custom);
    assert!(dg.is_empty());
    set.add(
        "import applies cleaned section",
        imp.applied.as_ref().unwrap().speed.gain_cap_m == 2000,
        "",
    );
    assert!(imp.rollback());
    set.add(
        "rollback restores pre-import state",
        imp.applied.is_none() && imp.rollback_count() == 1,
        "",
    );

    // 7. 回退到上一包（两连导入各回一步）。
    let mut imp2 = ThemeImporter::new(0);
    let mut a = base.clone();
    a.pointer_scheme = String::from("甲包");
    let mut b = base.clone();
    b.pointer_scheme = String::from("乙包");
    let _ = imp2.import(&a);
    let _ = imp2.import(&b);
    imp2.rollback();
    set.add(
        "rollback chain stepwise",
        imp2.applied.as_ref().unwrap().pointer_scheme == "甲包",
        "",
    );

    // 8. 降级清单在导入路径如实透出（不静默）。
    let mut imp3 = ThemeImporter::new(0);
    let mut evil = base.clone();
    evil.speed.gain_cap_m = 9000;
    let dg3 = imp3.import(&evil);
    set.add(
        "downgrades surfaced through import",
        dg3.len() == 1 && imp3.applied.as_ref().unwrap().speed.gain_cap_m == 1000,
        "",
    );

    // 9. 往返指纹：内容指纹（序列化字节 FNV）跨序列化稳定。
    set.add(
        "section fingerprint stable",
        fnv1a64(&serialize_section(&base)) == fnv1a64(&serialize_section(&parse_section(&serialize_section(&base)).unwrap())),
        "",
    );


    // 5. 同步冲突差异报告：改三处 → 恰好三行 diff；不改 → 空清单。
    let mut local5 = base.clone();
    local5.speed.gain_cap_m = 2500;
    local5.wheel.global = String::from("always-notch");
    local5.pointer_scheme = String::from("夜行箭");
    let diffs = diff_sections(&local5, &base);
    set.add(
        "sync conflict diff lists changed fields",
        diffs.len() == 3
            && diffs.iter().any(|d| d.field == "speed.gain_cap_m")
            && diffs.iter().any(|d| d.field == "wheel.global")
            && diffs.iter().any(|d| d.field == "pointer_scheme"),
        "",
    );
    set.add("sync diff empty when identical", diff_sections(&base, &base).is_empty(), "");

    // 6. 旧版段迁移：无手势库段 → 默认补齐并如实列出补了什么。
    let mut legacy = serialize_section(&base);
    // 摘掉 gesture 行（v1 无手势库段的形态）。
    let legacy_text = core::str::from_utf8(&legacy)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("gesture="))
        .map(|l| alloc::format!("{}\n", l))
        .collect::<String>();
    legacy = legacy_text.into_bytes();
    let (migrated, filled) = migrate_legacy_v1(&legacy).expect("legacy migrate");
    set.add(
        "legacy v1 migration fills defaults honestly",
        filled == alloc::vec!["gestures.gestures"] && migrated.gestures == GestureSpec::default(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_rejected() {
        let mut bytes = serialize_section(&MouseBehaviorSection::default());
        bytes.extend_from_slice(b"mystery=1\n");
        assert!(parse_section(&bytes).is_err(), "未知键拒绝静默跳过");
    }

    #[test]
    fn missing_header_rejected() {
        let junk = b"v=1\nscheme=x\n";
        assert!(parse_section(junk).is_err());
        let junk2 = b"section=mouse-behavior\nscheme=x\n";
        assert!(parse_section(junk2).is_err());
    }

    #[test]
    fn overrides_roundtrip() {
        let mut s = MouseBehaviorSection::default();
        s.wheel.overrides.push((String::from("term"), String::from("always-notch")));
        s.wheel.overrides.push((String::from("web"), String::from("always-smooth")));
        let bytes = serialize_section(&s);
        let p = parse_section(&bytes).unwrap();
        assert_eq!(p.wheel.overrides.len(), 2);
        assert_eq!(p.wheel.overrides[0], (String::from("term"), String::from("always-notch")));
    }

    #[test]
    fn speed_cap_lower_bound_also_guarded() {
        let mut s = MouseBehaviorSection::default();
        s.speed.gain_cap_m = 50;
        let (clean, dg) = validate_and_downgrade(&s);
        assert_eq!(clean.speed.gain_cap_m, 1000);
        assert!(dg.iter().any(|d| d.field == "speed.gain_cap"));
    }

    #[test]
    fn rollback_without_import_is_false() {
        let mut imp = ThemeImporter::new(0);
        assert!(!imp.rollback());
    }

    #[test]
    fn sidekeys_default_preserved_in_roundtrip() {
        let s = MouseBehaviorSection::default();
        let p = parse_section(&serialize_section(&s)).unwrap();
        assert_eq!(p.sidekeys.bindings.len(), 2);
        assert_eq!(p.sidekeys.bindings[0].2, "nav-back");
    }
}

// ---------------------------------------------------------------------------
// v3 深化批：v2 完整性哈希段 · 三方合并 · 档案容器导出/导入 ·
// F147 携带台账 · 篡改检测
// ---------------------------------------------------------------------------


/// v2 段头（带完整性哈希——v1 行式 TLV 之上加 fnv1a64 校验行）。
pub const SECTION_V2_HEADER: &str = "v=2";

/// 序列化行为段 v2：v1 布局 + 尾行 `hash=<hex16>`（对除 hash 行外全部
/// 字节做 fnv1a64——防传输损坏与手改，篡改可检出）。
pub fn serialize_section_v2(s: &MouseBehaviorSection) -> Vec<u8> {
    let mut body = serialize_section(s);
    // 把 v=1 换成 v=2 后再算哈希（版本头参与哈希——一处一事实）。
    body.splice(0..3, SECTION_V2_HEADER.bytes());
    let h = crate::jstar2::jbase::fnv1a64(&body);
    let mut out = body;
    out.extend_from_slice(&alloc::format!("hash={h:016x}\n").into_bytes());
    out
}

/// 反序列化 v2 段：逐字节校验哈希；哈希不符 → Err（不猜不救）。
/// 兼容入口：v1 数据（无 hash 行）走 [`parse_section`] 旧路。
pub fn parse_section_v2(d: &[u8]) -> Result<MouseBehaviorSection, String> {
    let text = core::str::from_utf8(d).map_err(|_| String::from("非 UTF-8 字节——包损坏"))?;
    let mut lines = text.lines();
    if lines.next() != Some(SECTION_V2_HEADER) {
        return Err(String::from("缺少 v2 版本头"));
    }
    let (body_end, hash_line) = {
        let v: Vec<&str> = text.lines().collect();
        if v.len() < 3 {
            return Err(String::from("v2 段过短——缺 hash 行"));
        }
        (v.len() - 2, v[v.len() - 1])
    };
    if !hash_line.starts_with("hash=") {
        return Err(String::from("缺少 hash 行"));
    }
    let want = u64::from_str_radix(hash_line.trim_start_matches("hash="), 16)
        .map_err(|_| String::from("hash 行非十六进制"))?;
    // 哈希覆盖范围：从 v=2 头到 hash 行前（含换行）。
    let mut covered = 0usize;
    for (i, l) in text.lines().enumerate() {
        if i > body_end {
            break;
        }
        covered += l.len() + 1;
    }
    let covered = covered.min(d.len());
    let got = crate::jstar2::jbase::fnv1a64(&d[..covered]);
    if got != want {
        return Err(alloc::format!("完整性哈希不符：want={want:016x} got={got:016x}——段被篡改或损坏"));
    }
    // v2 主体与 v1 布局兼容：剥掉 hash 行后走 v1 解析（含 v=1 版本头行）。
    let body = &d[..covered];
    let mut v1 = body.to_vec();
    v1.splice(0..3, b"v=1".iter().copied());
    parse_section(&v1)
}

/// 三方合并（vxtheme 同步的标准解法——F147 双设备同步语义）：
/// - 字段本地改过、远端没改 → 本地赢；
/// - 远端改过、本地没改 → 远端赢；
/// - 两边都改且不同 → 冲突，本地赢 + 冲突登记（不静默覆盖用户改动）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeOutcome {
    pub local_wins: Vec<&'static str>,
    pub incoming_wins: Vec<&'static str>,
    pub conflicts: Vec<&'static str>,
}

impl MergeOutcome {
    fn new() -> MergeOutcome {
        MergeOutcome { local_wins: Vec::new(), incoming_wins: Vec::new(), conflicts: Vec::new() }
    }
}

/// 三方合并入口（字段面：scheme / speed / wheel / sidekeys / gestures 五段，
/// 段内整体比较——段级冲突粒度与 diff_sections 同源）。
pub fn three_way_merge(
    base: &MouseBehaviorSection,
    local: &MouseBehaviorSection,
    incoming: &MouseBehaviorSection,
) -> (MouseBehaviorSection, MergeOutcome) {
    let mut out = local.clone();
    let mut oc = MergeOutcome::new();
    let field = |name: &'static str,
                     changed_local: bool,
                     changed_incoming: bool,
                     incoming_val: &MouseBehaviorSection,
                     out: &mut MouseBehaviorSection,
                     oc: &mut MergeOutcome| {
        match (changed_local, changed_incoming) {
            (false, true) => {
                oc.incoming_wins.push(name);
            }
            (true, false) => oc.local_wins.push(name),
            (true, true) => oc.conflicts.push(name),
            (false, false) => {}
        }
        if !changed_local && changed_incoming {
            match name {
                "scheme" => out.pointer_scheme = incoming_val.pointer_scheme.clone(),
                "speed" => out.speed = incoming_val.speed.clone(),
                "wheel" => out.wheel = incoming_val.wheel.clone(),
                "sidekeys" => out.sidekeys = incoming_val.sidekeys.clone(),
                _ => out.gestures = incoming_val.gestures.clone(),
            }
        }
    };
    let eq_scheme = |a: &MouseBehaviorSection, b: &MouseBehaviorSection| a.pointer_scheme == b.pointer_scheme;
    let eq_speed = |a: &MouseBehaviorSection, b: &MouseBehaviorSection| a.speed == b.speed;
    let eq_wheel = |a: &MouseBehaviorSection, b: &MouseBehaviorSection| a.wheel == b.wheel;
    let eq_side = |a: &MouseBehaviorSection, b: &MouseBehaviorSection| a.sidekeys == b.sidekeys;
    let eq_gest = |a: &MouseBehaviorSection, b: &MouseBehaviorSection| a.gestures == b.gestures;
    field("scheme", !eq_scheme(base, local), !eq_scheme(base, incoming), incoming, &mut out, &mut oc);
    field("speed", !eq_speed(base, local), !eq_speed(base, incoming), incoming, &mut out, &mut oc);
    field("wheel", !eq_wheel(base, local), !eq_wheel(base, incoming), incoming, &mut out, &mut oc);
    field("sidekeys", !eq_side(base, local), !eq_side(base, incoming), incoming, &mut out, &mut oc);
    field("gestures", !eq_gest(base, local), !eq_gest(base, incoming), incoming, &mut out, &mut oc);
    (out, oc)
}

/// 档案容器（携带面）：段字节 + 设备标签 + 携带时刻 + 容器哈希。
pub struct CarryContainer {
    pub device_tag: String,
    pub carried_at_ms: u64,
    payload: Vec<u8>,
    hash: u64,
}

impl CarryContainer {
    /// 打包（v2 段进容器——容器哈希对整段负责）。
    pub fn pack(section: &MouseBehaviorSection, device_tag: &str, at_ms: u64) -> CarryContainer {
        let payload = serialize_section_v2(section);
        let mut feed = payload.clone();
        feed.extend_from_slice(device_tag.as_bytes());
        let hash = crate::jstar2::jbase::fnv1a64(&feed);
        CarryContainer { device_tag: String::from(device_tag), carried_at_ms: at_ms, payload, hash }
    }

    /// 容器校验 + 开包（设备标签参与哈希——换标签即检出）。
    pub fn unpack(&self, device_tag: &str) -> Result<MouseBehaviorSection, String> {
        let mut feed = self.payload.clone();
        feed.extend_from_slice(device_tag.as_bytes());
        if crate::jstar2::jbase::fnv1a64(&feed) != self.hash {
            return Err(String::from("容器哈希不符——携带面损坏或标签不符"));
        }
        parse_section_v2(&self.payload)
    }

    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

/// F147 携带台账（跨设备插拔的事件面——每条带设备与时刻，环形 32 槽）。
pub struct CarryLedger {
    entries: Vec<(String, u64, u64)>, // (device_tag, at_ms, payload_len)
    pub total: u64,
}

impl CarryLedger {
    pub fn new() -> CarryLedger {
        CarryLedger { entries: Vec::new(), total: 0 }
    }

    pub fn record(&mut self, device_tag: &str, at_ms: u64, payload_len: usize) {
        if self.entries.len() >= 32 {
            self.entries.remove(0);
        }
        self.entries.push((String::from(device_tag), at_ms, payload_len as u64));
        self.total += 1;
    }

    /// 查询某设备最近一次携带时刻。
    pub fn last_for(&self, device_tag: &str) -> Option<u64> {
        self.entries.iter().rev().find(|(d, _, _)| d == device_tag).map(|(_, t, _)| *t)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
impl Default for CarryLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// v3 自检。
pub fn run_vtheme_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F623-v3");
    let base = MouseBehaviorSection::default();
    let mut local = base.clone();
    local.speed.gain_cap_m = 3200;
    let mut incoming = base.clone();
    incoming.pointer_scheme = String::from("极简白");

    // 1. v2 序列化往返：serialize→parse→再 serialize 逐字节相等。
    let v2 = serialize_section_v2(&base);
    let back = parse_section_v2(&v2).unwrap();
    set.add("v2 section roundtrip byte-exact", serialize_section_v2(&back) == v2, "");

    // 2. 篡改检测：翻一个字节 → 哈希不符报错（人话、定位到完整性）。
    let mut tampered = v2.clone();
    let at = tampered.len() / 2;
    tampered[at] ^= 0x01;
    set.add(
        "v2 tamper detected by hash",
        parse_section_v2(&tampered).err().map(|e| e.contains("篡改") || e.contains("哈希")).unwrap_or(false),
        "",
    );

    // 3. v1 数据不被 v2 入口误收（诚实拒绝 + 指路旧解析器）。
    let v1 = serialize_section(&base);
    set.add("v2 parser rejects v1 honestly", parse_section_v2(&v1).is_err(), "");

    // 4. 三方合并：远端单改 → 远端赢；本地单改 → 本地保留；双改 → 冲突登记。
    let (merged, oc) = three_way_merge(&base, &local, &incoming);
    set.add(
        "three-way merge field routing",
        oc.incoming_wins.contains(&"scheme")
            && oc.local_wins.contains(&"speed")
            && merged.pointer_scheme == "极简白"
            && merged.speed.gain_cap_m == 3200,
        "",
    );
    let mut local2 = base.clone();
    local2.wheel.global = String::from("always-notch");
    let mut incoming2 = base.clone();
    incoming2.wheel.global = String::from("always-smooth");
    let (_, oc2) = three_way_merge(&base, &local2, &incoming2);
    set.add(
        "three-way merge conflict registered not silent",
        oc2.conflicts.contains(&"wheel") && oc2.conflicts.len() == 1,
        "",
    );

    // 5. 无改动三方 = 恒等（零假冲突）。
    let (_, oc3) = three_way_merge(&base, &base, &base);
    set.add("three-way merge identity", oc3.local_wins.is_empty() && oc3.incoming_wins.is_empty() && oc3.conflicts.is_empty(), "");

    // 6. 档案容器：打包→开包往返一致；设备标签不符 → 诚实拒绝。
    let cc = CarryContainer::pack(&base, "y7000", 12345);
    let open_ok = cc.unpack("y7000").map(|s| s == base).unwrap_or(false);
    let open_bad = cc.unpack("desktop-01").is_err();
    set.add("carry container roundtrip and tag bound", open_ok && open_bad, "");

    // 7. 携带台账：记录、查最近、32 槽环形不无界增长（被驱逐的旧记录
    //    如实查不到——环的语义就是只保最近 32 条）。
    let mut ledger = CarryLedger::new();
    ledger.record("y7000", 100, cc.payload_len());
    ledger.record("desktop-01", 200, 100);
    for i in 0..40u64 {
        ledger.record("pad", 400 + i, 10);
    }
    ledger.record("y7000", 1000, cc.payload_len());
    set.add(
        "carry ledger ring and lookup",
        ledger.total == 43 && ledger.len() == 32 && ledger.last_for("y7000") == Some(1000) && ledger.last_for("desktop-01").is_none(),
        "",
    );

    // 8. 容器负载非空且带 v2 头（结构面对账）。
    set.add(
        "container payload carries v2 header",
        cc.payload_len() > 8 && core::str::from_utf8(&cc.payload[..3]) == Ok(SECTION_V2_HEADER),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn v2_roundtrip_with_overrides() {
        let mut s = MouseBehaviorSection::default();
        s.wheel.overrides.push((String::from("app-x"), String::from("notch")));
        s.gestures.gestures.push((String::from("g3"), String::from("UD"), String::from("act")));
        let v2 = serialize_section_v2(&s);
        let back = parse_section_v2(&v2).unwrap();
        assert_eq!(back.wheel.overrides.len(), 1);
        assert_eq!(back.gestures.gestures.len(), 3);
    }

    #[test]
    fn merge_local_only_change() {
        let mut local = MouseBehaviorSection::default();
        local.sidekeys.bindings.clear();
        let (m, oc) = three_way_merge(&MouseBehaviorSection::default(), &local, &MouseBehaviorSection::default());
        assert!(m.sidekeys.bindings.is_empty());
        assert!(oc.conflicts.is_empty() && oc.incoming_wins.is_empty());
    }

    #[test]
    fn container_detects_payload_corruption() {
        let mut cc = CarryContainer::pack(&MouseBehaviorSection::default(), "dev", 1);
        let i = cc.payload.len() - 5;
        cc.payload[i] ^= 0x02;
        assert!(cc.unpack("dev").is_err(), "负载损坏必须被检出");
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：打包清单人话摘要渲染 · KV 携带魔数封皮（带魔数与篡改拒绝）
// ---------------------------------------------------------------------------

/// 打包清单人话摘要（导入确认页的呈现面：方案名 + 四件逐行人话；
/// 数字全部取自段事实本身——不另记第二份状态，一处一事实）。
pub fn summarize_section(s: &MouseBehaviorSection) -> String {
    let mut out = String::new();
    out.push_str(&alloc::format!("指针方案：{}\n", s.pointer_scheme));
    out.push_str(&alloc::format!(
        "速度曲线：{}（增益上限 {}‰）\n",
        s.speed.curve_id, s.speed.gain_cap_m
    ));
    out.push_str(&alloc::format!(
        "滚轮档：{}（应用覆盖 {} 项）\n",
        s.wheel.global,
        s.wheel.overrides.len()
    ));
    out.push_str(&alloc::format!("侧键映射：{} 条\n", s.sidekeys.bindings.len()));
    out.push_str(&alloc::format!("手势库：{} 条\n", s.gestures.gestures.len()));
    out
}

/// KV 携带封皮魔数（载荷前 8 字节——封皮损坏在魔数关即拒绝，不进解析）。
pub const KV_MAGIC: &str = "VXKV1000";

/// 打包参数 KV 封皮（魔数 + v1 载荷 + fnv1a64 尾随哈希——传输面防损坏
/// 防手改；布局一处一事实：`MAGIC || payload || "kvhash=<hex16>\n"`）。
pub fn pack_kv(s: &MouseBehaviorSection) -> Vec<u8> {
    let payload = serialize_section(s);
    let h = crate::jstar2::jbase::fnv1a64(&payload);
    let mut out = Vec::new();
    out.extend_from_slice(KV_MAGIC.as_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&alloc::format!("kvhash={h:016x}\n").into_bytes());
    out
}

/// KV 封皮尾行长度（"kvhash=" 7 字节 + hex16 + '\n'）。
const KV_TAIL_LEN: usize = 7 + 16 + 1;

/// KV 封皮开包：长度关 → 魔数关 → 哈希关 → v1 解析，逐关人话拒绝
/// （篡改/损坏不猜不救——F623 打包语义在携带面的延伸）。
pub fn unpack_kv(d: &[u8]) -> Result<MouseBehaviorSection, String> {
    let mlen = KV_MAGIC.len();
    if d.len() < mlen + KV_TAIL_LEN {
        return Err(String::from("KV 封皮过短——魔数或哈希尾行不完整"));
    }
    if &d[..mlen] != KV_MAGIC.as_bytes() {
        return Err(String::from("KV 封皮魔数不符——不是 VARIX 打包参数携带面"));
    }
    let split = d.len() - KV_TAIL_LEN;
    if &d[split..split + 7] != b"kvhash=" {
        return Err(String::from("KV 载荷与哈希尾行分界损坏"));
    }
    let hexs = core::str::from_utf8(&d[split + 7..d.len() - 1])
        .map_err(|_| String::from("哈希尾行非文本"))?;
    let want = u64::from_str_radix(hexs, 16).map_err(|_| String::from("哈希尾行非十六进制"))?;
    let got = crate::jstar2::jbase::fnv1a64(&d[mlen..split]);
    if got != want {
        return Err(alloc::format!(
            "KV 载荷完整性哈希不符：want={want:016x} got={got:016x}——被篡改或损坏"
        ));
    }
    parse_section(&d[mlen..split])
}

/// v4 自检（摘要呈现 + KV 封皮三道闸 + 确定性）。
pub fn run_vtheme_v4_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F623-v4");
    let base = MouseBehaviorSection::default();

    // 1. 摘要如实呈现段事实（默认段：双手势 / 双侧键 / 增益 1000‰）。
    let sum = summarize_section(&base);
    set.add(
        "summary renders section facts",
        sum.contains("指针方案：VARIX 默认指针")
            && sum.contains("手势库：2 条")
            && sum.contains("侧键映射：2 条")
            && sum.contains("1000"),
        "",
    );

    // 2. 摘要跟随改动（改手势数 → 数字跟着变——不是快照文案）。
    let mut custom = base.clone();
    custom.gestures.gestures.push((String::from("g3"), String::from("UL"), String::from("act")));
    custom.speed.gain_cap_m = 2500;
    let sum2 = summarize_section(&custom);
    set.add(
        "summary follows mutations",
        sum2.contains("手势库：3 条") && sum2.contains("2500"),
        "",
    );

    // 3. KV 封皮往返：pack → unpack → 段相等。
    set.add(
        "kv pack/unpack roundtrip",
        unpack_kv(&pack_kv(&base)).map(|s| s == base).unwrap_or(false),
        "",
    );

    // 4. pack 确定性：同一段两次 pack 逐字节相同。
    set.add("kv pack deterministic", pack_kv(&base) == pack_kv(&base), "");

    // 5. 魔数关：首字节翻转 → 魔数不符人话拒绝。
    let mut bad_magic = pack_kv(&base);
    bad_magic[0] ^= 0x01;
    set.add(
        "kv magic gate rejects",
        unpack_kv(&bad_magic).err().map(|e| e.contains("魔数")).unwrap_or(false),
        "",
    );

    // 6. 篡改关：载荷翻一字节 → 哈希不符如实点破。
    let mut tampered = pack_kv(&base);
    let mid = KV_MAGIC.len() + 5;
    tampered[mid] ^= 0x02;
    set.add(
        "kv tamper detected by hash",
        unpack_kv(&tampered).err().map(|e| e.contains("篡改") || e.contains("哈希")).unwrap_or(false),
        "",
    );

    // 7. 截断关：砍掉哈希尾行 → 过短拒绝（不越界不误读）。
    let packed = pack_kv(&base);
    set.add(
        "kv truncated rejected",
        unpack_kv(&packed[..packed.len() - 5]).is_err(),
        "",
    );

    // 8. 裸 v1 段（无封皮）不被误收。
    set.add(
        "kv rejects bare v1 payload",
        unpack_kv(&serialize_section(&base)).is_err(),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
    use super::*;

    #[test]
    fn kv_roundtrip_with_custom_section() {
        let mut s = MouseBehaviorSection::default();
        s.wheel.overrides.push((String::from("term"), String::from("always-notch")));
        s.pointer_scheme = String::from("夜行箭");
        let back = unpack_kv(&pack_kv(&s)).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn summary_is_deterministic() {
        let s = MouseBehaviorSection::default();
        assert_eq!(summarize_section(&s), summarize_section(&s));
    }

    #[test]
    fn kv_unpack_rejects_bit_flips_across_payload() {
        let packed = pack_kv(&MouseBehaviorSection::default());
        // 载荷中段逐位翻转——全部必须被哈希关拦下。
        for off in KV_MAGIC.len()..packed.len() - KV_TAIL_LEN {
            let mut d = packed.clone();
            d[off] ^= 0x10;
            assert!(unpack_kv(&d).is_err(), "offset {} 翻转未被拦下", off);
        }
    }
}
