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
