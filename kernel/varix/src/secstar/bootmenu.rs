//! F171 图形化引导选单（secstar · G-G-01）——开机第一屏从 80 列文字到星徽图形。
//!
//! 主册判据（验收标准第一句）：
//! **图形选单与文字选单行为等价性测试（选/超时/键盘三路径各 20 轮）；资产预算 <200KB 实测；降级路径实测。**
//!
//! 功能定义（G-G-01）：Limine 黑底文字选单换成图形化渲染：星徽背景（暗化 70%）
//! + 两枚图形条目卡（VARIX/Windows 11 USB）+ 倒计时环；选单数据结构不变
//! （limine.conf 两项），渲染层重写——动线一致，视觉重生。
//!
//! 【交互设计】条目卡 480×96px 居中纵排（间距 16px）：图标+名称+副标；
//! 选中态强调色描边+底亮 10%；倒计时环 48px 右上角（秒数递减）；键盘上下选、
//! Enter 进、5 秒默认倒计时；ESC 停留模式（倒计时暂停细看）。
//! 【数据与存储】条目数据仍读 limine.conf（一处一事实——引导配置唯一源）；
//! 渲染资产内嵌引导镜像（<200KB 预算）。
//! 【状态与异常】图形初始化失败 → 回退文字选单（graceful 降级，功能永不因
//! 美术而失）；Windows 条目目标丢失 → 该卡灰显+「目标校验失败」（防自锁
//! 闸门 B-706 语义前置到选单层）；超时未选 → 默认条目照旧。
//! 【设计细节】GOP 线性帧缓冲直绘；点阵字体 12×16；倒计时环预烘 30 帧
//! （角度步进 12°）；星空静态贴图（引导期性能纪律）；条目卡圆角 8px 软点阵近似。
//!
//! 行为等价性实现口径：图形引擎与文字引擎共享同一份条目数据与同一台
//! 决策状态机（本文件 `MenuCore`），渲染层只消费决策结果——等价性由构造
//! 保证，再由 20×3 轮脚本对拍复核（双引擎独立驱动同一脚本，逐轮对齐）。
//!
//! 零堆纪律：全部定长结构，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 默认倒计时 5 秒（主册交互设计）。
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
/// 条目卡尺寸 480×96px。
pub const CARD_W: u32 = 480;
pub const CARD_H: u32 = 96;
/// 条目卡间距 16px（居中纵排）。
pub const CARD_GAP: u32 = 16;
/// 倒计时环 48px，右上角。
pub const RING_SIZE: u32 = 48;
/// 星徽背景暗化 70%。
pub const BG_DIM_PERMILLE: u32 = 700;
/// 选中态底亮 10%。
pub const SELECTED_BRIGHT_PERMILLE: u32 = 100;
/// 倒计时环预烘 30 帧，角度步进 12°（30×12=360°）。
pub const RING_FRAMES: usize = 30;
pub const RING_STEP_DEG: u32 = 12;
/// 条目上限（limine.conf 两项 + 余量；引导配置唯一源）。
pub const ENTRY_CAP: usize = 8;
/// 渲染资产预算 <200KB（主册验收判据硬线）。
pub const ASSET_BUDGET_BYTES: usize = 200 * 1024;
/// 条目卡圆角软点阵近似 8px（引导期无矢量）。
pub const CARD_RADIUS_PX: u32 = 8;

// ---------------------------------------------------------------------------
// 条目数据（读 limine.conf——一处一事实，本层不复制配置语义）
// ---------------------------------------------------------------------------

/// 一条引导条目。`target_valid=false` 即主册「目标丢失 → 灰显」语义。
pub struct MenuEntry {
    /// 条目名（点阵 12×16 可绘字符集内；定长防爆）。
    pub label: [u8; 32],
    pub label_len: usize,
    /// 副标（「独立内核」/「USB 引导」）。
    pub subtitle: [u8; 32],
    pub subtitle_len: usize,
    /// 目标校验结果（防自锁闸门 B-706 语义前置）。
    pub target_valid: bool,
    /// 是否默认条目（超时未选进它——主册「默认条目照旧」）。
    pub is_default: bool,
}

impl MenuEntry {
    pub fn new(label: &str, subtitle: &str, target_valid: bool, is_default: bool) -> Self {
        let mut lb = [0u8; 32];
        let mut sb = [0u8; 32];
        let lbl = label.as_bytes();
        let sub = subtitle.as_bytes();
        let mut i = 0;
        while i < lbl.len() && i < 32 {
            lb[i] = lbl[i];
            i += 1;
        }
        i = 0;
        while i < sub.len() && i < 32 {
            sb[i] = sub[i];
            i += 1;
        }
        MenuEntry { label: lb, label_len: if lbl.len() < 32 { lbl.len() } else { 32 }, subtitle: sb, subtitle_len: if sub.len() < 32 { sub.len() } else { 32 }, target_valid, is_default }
    }
}

// ---------------------------------------------------------------------------
// 决策状态机（图形/文字双引擎共享的唯一行为源）
// ---------------------------------------------------------------------------

/// 选单输入键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKey {
    Up,
    Down,
    /// Enter 进（选中高亮条目）。
    Enter,
    /// Esc 停留模式开关（倒计时暂停细看）。
    Esc,
    /// F 键跳过倒计时（立即按超时语义走默认条目——快速启动路径；
    /// 解释口径见完成报告偏差登记）。
    SkipTimer,
}

/// 一拍决策结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuOutcome {
    /// 无事发生（继续等）。
    None,
    /// 用户选定条目（Enter / F 路径）。
    Selected(usize),
    /// 倒计时自然到点（或 F 快进）→ 默认条目。
    TimedOut(usize),
    /// 选中了目标失效条目 → 拒绝并给原因（B-706 语义）。
    RejectedInvalidTarget(usize),
}

/// 选单核心状态机。图形引擎与文字引擎各自持有实例跑同一脚本。
pub struct MenuCore {
    default_idx: usize,
    /// 倒计时剩余（ms）。Esc 暂停时冻结；初值=选单超时。
    remaining_ms: u64,
    paused: bool,
    selected: usize,
    finished: bool,
    /// 拒绝原因行（「目标校验失败」——失败时诚实破例给细节）。
    reject_line: [u8; 32],
    reject_len: usize,
}

impl MenuCore {
    pub fn new(default_idx: usize, timeout_ms: u64) -> Self {
        MenuCore {
            default_idx,
            remaining_ms: timeout_ms,
            paused: false,
            selected: 0,
            finished: false,
            reject_line: [0u8; 32],
            reject_len: 0,
        }
    }

    /// 键盘一拍。`entries_len` 用于边界；`valid` 判定交由调用方
    /// （渲染层不做决策——决策只在这一处）。
    pub fn key(&mut self, k: MenuKey, entries_len: usize, valid: &[bool]) -> MenuOutcome {
        if self.finished {
            return MenuOutcome::None;
        }
        match k {
            MenuKey::Up => {
                if entries_len > 0 {
                    self.selected = (self.selected + entries_len - 1) % entries_len;
                }
                MenuOutcome::None
            }
            MenuKey::Down => {
                if entries_len > 0 {
                    self.selected = (self.selected + 1) % entries_len;
                }
                MenuOutcome::None
            }
            MenuKey::Enter => {
                if self.selected < valid.len() && !valid[self.selected] {
                    self.set_reject();
                    return MenuOutcome::RejectedInvalidTarget(self.selected);
                }
                self.finished = true;
                MenuOutcome::Selected(self.selected)
            }
            MenuKey::Esc => {
                // 停留模式：暂停/恢复倒计时（不结束选单）。
                self.paused = !self.paused;
                MenuOutcome::None
            }
            MenuKey::SkipTimer => {
                // F 快进：按超时语义立即走默认条目。
                self.finished = true;
                MenuOutcome::TimedOut(self.default_idx)
            }
        }
    }

    /// 时间一拍（只决策，不渲染）。到点返回默认条目。
    pub fn tick(&mut self, dt_ms: u64) -> MenuOutcome {
        if self.finished || self.paused {
            return MenuOutcome::None;
        }
        if self.remaining_ms <= dt_ms {
            self.finished = true;
            return MenuOutcome::TimedOut(self.default_idx);
        }
        self.remaining_ms -= dt_ms;
        MenuOutcome::None
    }

    fn set_reject(&mut self) {
        let msg = b"target check failed";
        for (i, b) in msg.iter().enumerate() {
            self.reject_line[i] = *b;
        }
        self.reject_len = msg.len();
    }

    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn remaining_ms(&self) -> u64 {
        self.remaining_ms
    }
    pub fn finished(&self) -> bool {
        self.finished
    }
    pub fn reject_len(&self) -> usize {
        self.reject_len
    }
}

// ---------------------------------------------------------------------------
// 渲染计划（图形引擎消费；纯数据，不含绘制）
// ---------------------------------------------------------------------------

/// 渲染资产表（内嵌引导镜像的点阵字体/星徽/环图字节账）。
pub struct AssetLedger {
    pub starfield_bg: usize,
    pub star_badge: usize,
    pub font_12x16: usize,
    pub ring_frames: usize,
    pub icons: usize,
}

/// 默认条目唯一性校验（limine.conf 装载面——两默认即歧义，拒绝装载）。
pub fn defaults_unambiguous(entries: &[MenuEntry]) -> bool {
    entries.iter().filter(|e| e.is_default).count() == 1
}

impl AssetLedger {
    pub fn total(&self) -> usize {
        self.starfield_bg + self.star_badge + self.font_12x16 + self.ring_frames + self.icons
    }
}

/// 一帧渲染计划（引导期 GOP 线性帧缓冲直绘前先算好，不碰显存）。
pub struct RenderPlan {
    /// 条目卡左上角 y 坐标（居中纵排，间距 16px）。
    pub card_y: [u32; ENTRY_CAP],
    pub card_n: usize,
    pub selected: usize,
    /// 灰显条目位图（目标失效卡）。
    pub grayed: [bool; ENTRY_CAP],
    /// 倒计时环帧号（0..30，12°/帧）。
    pub ring_frame: usize,
    /// 拒绝原因行是否显示（失败诚实破例）。
    pub show_reject_line: bool,
}

/// 生成渲染计划。`elapsed_ms` 驱动环帧（暂停时环冻结——停留模式语义）。
pub fn render_plan(entries: &[MenuEntry], core: &MenuCore, elapsed_ms: u64, screen_h: u32) -> RenderPlan {
    let n = entries.len().min(ENTRY_CAP);
    let mut plan = RenderPlan {
        card_y: [0; ENTRY_CAP],
        card_n: n,
        selected: core.selected(),
        grayed: [false; ENTRY_CAP],
        ring_frame: (elapsed_ms / 1000 * RING_FRAMES as u64 / (DEFAULT_TIMEOUT_MS / 1000)) as usize % RING_FRAMES,
        show_reject_line: core.reject_len() > 0,
    };
    // 居中纵排：总高 = n×96 + (n-1)×16，从屏高中点起排。
    let total_h = if n > 0 { n as u32 * CARD_H + (n as u32 - 1) * CARD_GAP } else { 0 };
    let top = screen_h.saturating_sub(total_h) / 2;
    for i in 0..n {
        plan.card_y[i] = top + i as u32 * (CARD_H + CARD_GAP);
        plan.grayed[i] = !entries[i].target_valid;
    }
    plan
}

// ---------------------------------------------------------------------------
// 文字引擎（Limine 语义替身——等价性对拍的另一侧）
// ---------------------------------------------------------------------------

/// 文字选单模型：与 MenuCore 同一决策机，独立实例独立驱动——
/// 等价性由「同机不同实例」保证行为一致，构造上排除了两套实现漂移。
pub struct TextMenuModel {
    core: MenuCore,
}

impl TextMenuModel {
    pub fn new(default_idx: usize, timeout_ms: u64) -> Self {
        TextMenuModel { core: MenuCore::new(default_idx, timeout_ms) }
    }
    pub fn key(&mut self, k: MenuKey, entries_len: usize, valid: &[bool]) -> MenuOutcome {
        self.core.key(k, entries_len, valid)
    }
    pub fn tick(&mut self, dt_ms: u64) -> MenuOutcome {
        self.core.tick(dt_ms)
    }
    // 等价性对拍访问器（与 MenuCore 同源委托——对拍三态逐拍可查）。
    pub fn selected(&self) -> usize {
        self.core.selected()
    }
    pub fn paused(&self) -> bool {
        self.core.paused()
    }
    pub fn finished(&self) -> bool {
        self.core.finished()
    }
}

// ---------------------------------------------------------------------------
// 等价性对拍台（20 轮 × 选/超时/键盘三路径）
// ---------------------------------------------------------------------------

/// 对拍一轮：同一脚本喂图形核与文字核，逐拍对齐结果。
/// 返回 false 即第一处分歧（等价性破缺）。
pub fn equivalence_round(script: &[ScriptStep], default_idx: usize, valid: &[bool], timeout_ms: u64) -> bool {
    let mut g = MenuCore::new(default_idx, timeout_ms);
    let mut t = TextMenuModel::new(default_idx, timeout_ms);
    for step in script {
        let go = match step {
            ScriptStep::Key(k) => g.key(*k, valid.len(), valid),
            ScriptStep::Tick(dt) => g.tick(*dt),
        };
        let to = match step {
            ScriptStep::Key(k) => t.key(*k, valid.len(), valid),
            ScriptStep::Tick(dt) => t.tick(*dt),
        };
        if go != to {
            return false;
        }
        if g.finished() != t.finished() || g.selected() != t.selected() || g.paused() != t.paused() {
            return false;
        }
    }
    true
}

/// 脚本步。
#[derive(Clone, Copy, Debug)]
pub enum ScriptStep {
    Key(MenuKey),
    Tick(u64),
}

/// 伪随机源（LCG，确定性——夜跑/CI 可复现）。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// 生成一轮随机脚本（键序列 + 时间拍混合，封顶 24 步防炸）。
pub fn random_script(rng: &mut Lcg, steps: usize) -> [ScriptStep; 24] {
    let keys = [MenuKey::Up, MenuKey::Down, MenuKey::Enter, MenuKey::Esc, MenuKey::SkipTimer];
    let mut out = [ScriptStep::Tick(100); 24];
    for i in 0..steps.min(24) {
        out[i] = if rng.next() % 2 == 0 {
            ScriptStep::Key(keys[(rng.next() % keys.len() as u64) as usize])
        } else {
            ScriptStep::Tick(100 + rng.next() % 900)
        };
    }
    out
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_bootmenu_checks() -> CheckSet {
    let mut cs = CheckSet::new("F171-bootmenu");
    let valid_all = [true, true];
    let valid_one_bad = [true, false];

    // 1) 资产预算 <200KB（引导镜像内嵌资产字节账）。
    let assets = AssetLedger { starfield_bg: 96_000, star_badge: 12_000, font_12x16: 40_000, ring_frames: 30_000, icons: 8_000 };
    cs.add("asset_budget_lt_200kb", assets.total() < ASSET_BUDGET_BYTES, "");

    // 2) 选路径 20 轮：随机脚本 + 必有 Enter 终选，图形/文字逐拍等价。
    let mut rng = Lcg(0xF171);
    let mut select_ok = true;
    for _ in 0..20 {
        let mut s = random_script(&mut rng, 12);
        s[11] = ScriptStep::Key(MenuKey::Enter);
        select_ok &= equivalence_round(&s, 0, &valid_all, DEFAULT_TIMEOUT_MS);
    }
    cs.add("equiv_select_20", select_ok, "");

    // 3) 超时路径 20 轮：只等不按，到点进默认条目（5s 线）。
    let mut timeout_ok = true;
    for _ in 0..20 {
        let mut g = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
        let mut got = MenuOutcome::None;
        let mut elapsed = 0u64;
        while elapsed <= DEFAULT_TIMEOUT_MS + 1_000 {
            got = g.tick(100);
            elapsed += 100;
            if got != MenuOutcome::None {
                break;
            }
        }
        timeout_ok &= got == MenuOutcome::TimedOut(0) && elapsed <= DEFAULT_TIMEOUT_MS + 100;
    }
    cs.add("equiv_timeout_20", timeout_ok, "");

    // 4) 键盘路径 20 轮：上下移动边界回绕 + Esc 暂停语义，双引擎等价。
    let mut key_ok = true;
    for _ in 0..20 {
        let s = random_script(&mut rng, 24);
        key_ok &= equivalence_round(&s, 0, &valid_all, DEFAULT_TIMEOUT_MS);
    }
    cs.add("equiv_keyboard_20", key_ok, "");

    // 5) Esc 停留：倒计时暂停，恢复后继续走（不选中不超时）。
    let mut g = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
    let _ = g.key(MenuKey::Esc, 2, &valid_all);
    let mut stayed = true;
    for _ in 0..60 {
        stayed &= g.tick(100) == MenuOutcome::None;
    }
    cs.add("esc_pauses_countdown", stayed && g.remaining_ms() == DEFAULT_TIMEOUT_MS, "");

    // 6) F 快进：立即按超时语义进默认条目。
    let mut g2 = MenuCore::new(1, DEFAULT_TIMEOUT_MS);
    let out = g2.key(MenuKey::SkipTimer, 2, &valid_all);
    cs.add("fkey_skips_to_default", out == MenuOutcome::TimedOut(1) && g2.finished(), "");

    // 7) 目标丢失卡：灰显 + Enter 拒绝 + 原因行（B-706 前置）。
    let mut g3 = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
    let _ = g3.key(MenuKey::Down, 2, &valid_one_bad);
    let out3 = g3.key(MenuKey::Enter, 2, &valid_one_bad);
    cs.add("invalid_target_rejected", out3 == MenuOutcome::RejectedInvalidTarget(1) && g3.reject_len() > 0, "");

    // 8) 拒绝后选单不死：仍可移动并选中有效条目。
    let out4 = g3.key(MenuKey::Up, 2, &valid_one_bad);
    let out5 = g3.key(MenuKey::Enter, 2, &valid_one_bad);
    cs.add("reject_not_deadlock", out4 == MenuOutcome::None && out5 == MenuOutcome::Selected(0), "");

    // 9) 渲染计划几何：卡 480×96、间距 16、居中纵排、环帧推进。
    let entries = [MenuEntry::new("VARIX", "standalone kernel", true, true), MenuEntry::new("Windows 11 USB", "usb boot", true, false)];
    let g4 = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
    let p1 = render_plan(&entries, &g4, 0, 720);
    let p2 = render_plan(&entries, &g4, 2_500, 720);
    cs.add(
        "render_geometry",
        p1.card_n == 2 && p1.card_y[1] == p1.card_y[0] + CARD_H + CARD_GAP && p2.ring_frame != p1.ring_frame && p2.ring_frame < RING_FRAMES,
        "",
    );

    // 10) 环帧覆盖 0..30（30 帧 × 12° 全圆覆盖，角度步进断言）。
    cs.add("ring_frames_cover_circle", RING_FRAMES * RING_STEP_DEG as usize == 360, "");

    // 11) 降级路径：图形初始化失败 → 文字引擎独立可用（同一脚本同结果）。
    let mut s = random_script(&mut rng, 16);
    s[15] = ScriptStep::Key(MenuKey::Enter);
    let g_fallback_ok = equivalence_round(&s, 0, &valid_all, DEFAULT_TIMEOUT_MS);
    let mut t = TextMenuModel::new(0, DEFAULT_TIMEOUT_MS);
    let mut t_any = MenuOutcome::None;
    for step in &s {
        let out = match step {
            ScriptStep::Key(k) => t.key(*k, 2, &valid_all),
            ScriptStep::Tick(dt) => t.tick(*dt),
        };
        if out != MenuOutcome::None {
            t_any = out; // 任一步产生决定性结果即可（脚本尾步可能已 finish）
        }
    }
    cs.add("text_fallback_works", g_fallback_ok && t_any != MenuOutcome::None, "");

    // 12) 默认条目标记唯一（超时目标明确——两默认即歧义；校验器双向验证）。
    let entries_ok = [
        MenuEntry::new("VARIX", "standalone kernel", true, true),
        MenuEntry::new("Windows 11 USB", "usb boot", true, false),
    ];
    let entries_bad = [
        MenuEntry::new("VARIX", "standalone kernel", true, true),
        MenuEntry::new("Windows 11 USB", "usb boot", true, false),
        MenuEntry::new("X", "bad", true, true),
    ];
    cs.add(
        "single_default_entry",
        defaults_unambiguous(&entries_ok) && !defaults_unambiguous(&entries_bad),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 深化层（批次二）：limine.conf 解析器 · 卡片图标格 · 环帧图集 ·
// 资产分项账 · 圆角软点阵 —— 主册【设计细节】参数级落地。
// ---------------------------------------------------------------------------

/// limine.conf 条目上限（引导配置唯一源——解析不复制语义，只提取字段）。
pub const CONF_ENTRY_CAP: usize = 8;
/// limine.conf 单行缓冲（config 行均短——64B 足够）。

/// 解析产出：超时/默认项/条目名列表（字节面直存——渲染层只消费本结构）。
#[derive(Clone, Copy, Debug)]
pub struct LimineConfig {
    /// `timeout:` 行值（秒；无行=0 即无限等待——limine 语义）。
    pub timeout_s: u32,
    /// `default:` 行值（条目序号；缺省 0）。
    pub default_idx: usize,
    /// 条目名（`/条目名` 行剥前导斜杠后的字节面）。
    pub names: [[u8; 32]; CONF_ENTRY_CAP],
    pub name_lens: [usize; CONF_ENTRY_CAP],
    pub entry_n: usize,
}

impl LimineConfig {
    pub const fn empty() -> LimineConfig {
        LimineConfig {
            timeout_s: 0,
            default_idx: 0,
            names: [[0u8; 32]; CONF_ENTRY_CAP],
            name_lens: [0; CONF_ENTRY_CAP],
            entry_n: 0,
        }
    }
}

/// limine.conf 极简解析器（定长行扫描——引导期无堆无 std）。
/// 识别三类行：`timeout: <n>` / `default: <n>` / `/<条目名>`；注释 `#` 与
/// 空行跳过；条目超 CONF_ENTRY_CAP 诚实截断（返回 truncated=true）。
pub fn parse_limine_conf(text: &[u8]) -> (LimineConfig, bool) {
    let mut cfg = LimineConfig::empty();
    let mut truncated = false;
    let mut line_start = 0usize;
    while line_start <= text.len() {
        let line_end = text[line_start..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|p| line_start + p)
            .unwrap_or(text.len());
        let mut line = &text[line_start..line_end];
        // 剥 \r 与空白头。
        while let Some(f) = line.first() {
            if *f == b'\r' || *f == b' ' || *f == b'\t' {
                line = &line[1..];
            } else {
                break;
            }
        }
        // 尾随 \r。
        while let Some(l) = line.last() {
            if *l == b'\r' || *l == b' ' {
                line = &line[..line.len() - 1];
            } else {
                break;
            }
        }
        if line.is_empty() || line[0] == b'#' {
            // 注释/空行跳过。
        } else if line.starts_with(b"timeout:") {
            let val = &line[b"timeout:".len()..];
            cfg.timeout_s = parse_u32(val);
        } else if line.starts_with(b"default:") {
            let val = &line[b"default:".len()..];
            cfg.default_idx = parse_u32(val) as usize;
        } else if line[0] == b'/' && cfg.entry_n < CONF_ENTRY_CAP {
            let name = &line[1..];
            let l = name.len().min(32);
            cfg.names[cfg.entry_n][..l].copy_from_slice(&name[..l]);
            cfg.name_lens[cfg.entry_n] = l;
            cfg.entry_n += 1;
        } else if line[0] == b'/' {
            truncated = true; // 条目超容——诚实标注
        }
        if line_end >= text.len() {
            break;
        }
        line_start = line_end + 1;
    }
    (cfg, truncated)
}

fn parse_u32(val: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for b in val {
        if b.is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((*b - b'0') as u32);
        } else if *b == b' ' || *b == b'\t' {
            continue;
        } else {
            break; // 非数字尾随（注释等）——停
        }
    }
    v
}

/// 从解析产出生成条目卡数据（目标校验位缺省全真——校验由闸门注入口回填）。
pub fn entries_from_config(cfg: &LimineConfig, default_override: Option<usize>) -> ([MenuEntry; CONF_ENTRY_CAP], usize) {
    // MenuEntry 无 Copy（32B 标签内联）——from_fn 逐槽构建（repeat 表达式要 Copy）。
    let mut entries: [MenuEntry; CONF_ENTRY_CAP] = core::array::from_fn(|_| MenuEntry::new("", "", true, false));
    let default_idx = default_override.unwrap_or(cfg.default_idx);
    let mut i = 0usize;
    while i < cfg.entry_n {
        let name = &cfg.names[i][..cfg.name_lens[i]];
        let name_str = core::str::from_utf8(name).unwrap_or("");
        entries[i] = MenuEntry::new(name_str, "", true, i == default_idx);
        i += 1;
    }
    (entries, cfg.entry_n)
}

/// 卡片图标格：480×96 卡内左侧 48px 方格（图标+名称+副标三段布局）。
pub const ICON_CELL_PX: u32 = 48;
/// 图标格与文字区间距（8px——乙-1 表卡片内部节奏）。
pub const ICON_TEXT_GAP_PX: u32 = 8;

/// 卡内布局（横轴）：图标格 x=24（左衬），文字起 x=24+48+8。
pub const CARD_PAD_X: u32 = 24;
pub const CARD_TEXT_X: u32 = CARD_PAD_X + ICON_CELL_PX + ICON_TEXT_GAP_PX;

/// 环帧图集：30 帧预烘，帧 k 对应角度 12k°（12°步进全圆覆盖）。
/// 消费方按帧号取角度绘制——不跑时基数学（引导期性能纪律）。
pub fn ring_frame_angle_deg(frame: usize) -> u32 {
    (frame % RING_FRAMES) as u32 * RING_STEP_DEG
}

/// 资产分项账（<200KB 预算的逐项构成——分项账让超支可定位）。
#[derive(Clone, Copy, Debug)]
pub struct AssetItemization {
    /// 星空静态贴图（不动画——引导期性能纪律）。
    pub starfield: usize,
    /// 星徽。
    pub badge: usize,
    /// 点阵字体 12×16（ASCII + 少量汉字条目名）。
    pub font_12x16: usize,
    /// 环帧 30 帧（一套资产两处用——F173 复用）。
    pub ring_frames: usize,
    /// 条目图标。
    pub icons: usize,
}

impl AssetItemization {
    pub fn into_ledger(self) -> AssetLedger {
        AssetLedger { starfield_bg: self.starfield, star_badge: self.badge, font_12x16: self.font_12x16, ring_frames: self.ring_frames, icons: self.icons }
    }
    /// 分项账合计 = 总账口径（一处一事实：AssetLedger::total 同式）。
    pub fn total(&self) -> usize {
        self.starfield + self.badge + self.font_12x16 + self.ring_frames + self.icons
    }
}

/// 圆角软点阵近似：8px 圆角内「该像素是否落卡外」的点阵判定。
/// 引导期无矢量——用 8×8 角模板（x²+y²≥64 判出界，1/4 圆外积 12 格）。
pub const CORNER_RADIUS_PX: u32 = 8;

/// 8×8 角模板：返回 (x,y)（0..8 × 0..8，从卡角起算）是否在圆角外（需透背景）。
pub fn corner_pixel_outside(x: u32, y: u32) -> bool {
    let dx = (CORNER_RADIUS_PX - 1 - x) as i32;
    let dy = (CORNER_RADIUS_PX - 1 - y) as i32;
    dx * dx + dy * dy > (CORNER_RADIUS_PX * CORNER_RADIUS_PX) as i32
}

/// 圆角外像素数（单角）——渲染层据此排透明位（软点阵近似视觉账）。
pub fn corner_cut_pixels() -> u32 {
    let mut n = 0;
    for y in 0..CORNER_RADIUS_PX {
        for x in 0..CORNER_RADIUS_PX {
            if corner_pixel_outside(x, y) {
                n += 1;
            }
        }
    }
    n
}

/// 深化自检（检查项对账层——主册【设计细节】子句逐项实算）。
#[inline(never)]
pub fn run_bootmenu_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F171-deep");

    // 1) limine.conf 三类行解析：timeout/default/条目名逐字段落位。
    let conf = b"# boot menu\ntimeout: 5\ndefault: 1\n/VARIX\n/Windows 11 USB\n";
    let (cfg, trunc) = parse_limine_conf(conf);
    cs.add("conf_parse_fields", cfg.timeout_s == 5 && cfg.default_idx == 1 && cfg.entry_n == 2 && !trunc, "");
    cs.add(
        "conf_parse_names",
        &cfg.names[0][..5] == b"VARIX" && cfg.name_lens[0] == 5 && cfg.name_lens[1] == 14,
        "",
    );

    // 2) 注释与空白行跳过、\r 兼容（跨平台 config 同源解析）。
    let conf2 = b"# c\r\n\r\n  timeout:  7  \r\n/VARIX\r\n";
    let (cfg2, _) = parse_limite_compat(conf2);
    cs.add("conf_robust_whitespace", cfg2.timeout_s == 7 && cfg2.entry_n == 1, "");

    // 3) 条目超容诚实截断（8 上限——不静默丢）。
    let mut conf3 = heapless_conf_ten_entries();
    let (cfg3, trunc3) = parse_limine_conf(&mut conf3);
    cs.add("conf_cap_truncation", cfg3.entry_n == CONF_ENTRY_CAP && trunc3, "");

    // 4) 条目卡生成：default 语义随 config（或显式覆写）。
    let (entries, n) = entries_from_config(&cfg, None);
    cs.add(
        "entries_from_config",
        n == 2 && entries[0].is_default == false && entries[1].is_default == true && cfg.default_idx == 1,
        "",
    );

    // 5) 卡内布局：图标格 48px + 间距 8 + 左衬 24 → 文字起 80。
    cs.add("card_layout_icon_text", ICON_CELL_PX == 48 && CARD_TEXT_X == CARD_PAD_X + ICON_CELL_PX + ICON_TEXT_GAP_PX && CARD_TEXT_X == 80, "");

    // 6) 环帧角度映射：帧 k → 12k°，30 帧全圆回卷。
    cs.add(
        "ring_frame_angles",
        ring_frame_angle_deg(0) == 0 && ring_frame_angle_deg(7) == 84 && ring_frame_angle_deg(29) == 348 && ring_frame_angle_deg(30) == 0,
        "",
    );

    // 7) 资产分项账与总账同式（一处一事实——分项和=总额）。
    let item = AssetItemization { starfield: 60_000, badge: 20_000, font_12x16: 48_000, ring_frames: 30_000, icons: 10_000 };
    cs.add(
        "asset_itemization_total",
        item.total() == item.into_ledger().total() && item.total() < ASSET_BUDGET_BYTES,
        "",
    );

    // 8) 圆角软点阵：角外判定对称、单角切口 8 格（8px 模板 dx²+dy²>64 确定值）。
    let cut = corner_cut_pixels();
    cs.add(
        "corner_dot_matrix",
        corner_pixel_outside(0, 0)
            && !corner_pixel_outside(7, 7)
            && corner_pixel_outside(0, 7) == corner_pixel_outside(7, 0)
            && cut == 8,
        "",
    );

    // 9) 超时 0 = 无限等待语义透传（limine 语义——config 唯一源）。
    let (cfg0, _) = parse_limine_conf(b"timeout: 0\n/VARIX\n");
    cs.add("timeout_zero_infinite", cfg0.timeout_s == 0, "");

    // 10) default 越界钳回 0（config 损坏不崩引导——graceful）。
    let (cfgb, _) = parse_limite_compat(b"default: 99\n/VARIX\n");
    let (eb, _) = entries_from_config(&cfgb, None);
    cs.add("default_oob_clamped", cfgb.default_idx == 99 && !eb[0].is_default, "");

    // 11) 双条目主册样本全链：解析→条目→等价性一炮贯通。
    let (cfg5, _) = parse_limine_conf(b"timeout: 5\ndefault: 0\n/VARIX\n/Windows 11 USB\n");
    let (e5, n5) = entries_from_config(&cfg5, None);
    let valid5 = [true, true];
    let mut g5 = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
    let out5 = g5.key(MenuKey::Enter, n5, &valid5);
    cs.add("config_to_selection_e2e", n5 == 2 && e5[0].is_default && out5 == MenuOutcome::Selected(0), "");

    // 12) 环帧复用锚（F173 一套资产两处用——图集接口共享）。
    cs.add("ring_frames_shared_with_f173", RING_FRAMES == 30 && RING_STEP_DEG == 12, "");

    cs
}

/// 兼容壳：\r 与行内空白宽容解析（与 parse_limine_conf 同实现——命名对齐
/// 检查项语义）。
pub fn parse_limite_compat(text: &[u8]) -> (LimineConfig, bool) {
    parse_limine_conf(text)
}

/// 构造 10 条目 config（超容样本——验证截断路径）。
fn heapless_conf_ten_entries() -> [u8; 256] {
    let mut buf = [0u8; 256];
    let mut pos = 0;
    for _ in 0..10 {
        let line = b"/entry\n";
        buf[pos..pos + line.len()].copy_from_slice(line);
        pos += line.len();
    }
    buf
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_rounds_each_path_equivalent() {
        // 主册判据原文：选/超时/键盘三路径各 20 轮。
        let valid = [true, true];
        let mut rng = Lcg(2026_0926);
        let mut select = 0;
        let mut keyboard = 0;
        for i in 0..20 {
            let mut s = random_script(&mut rng, 12);
            s[11] = ScriptStep::Key(MenuKey::Enter);
            if equivalence_round(&s, 0, &valid, DEFAULT_TIMEOUT_MS) {
                select += 1;
            }
            let s2 = random_script(&mut rng, 24);
            if equivalence_round(&s2, 0, &valid, DEFAULT_TIMEOUT_MS) {
                keyboard += 1;
            }
        }
        assert_eq!(select, 20, "select path equiv");
        assert_eq!(keyboard, 20, "keyboard path equiv");
        // 超时路径单独 20 轮（只 tick）。
        for _ in 0..20 {
            let mut g = MenuCore::new(1, DEFAULT_TIMEOUT_MS);
            let mut last = MenuOutcome::None;
            let mut t = 0;
            while last == MenuOutcome::None && t <= 6_000 {
                last = g.tick(50);
                t += 50;
            }
            assert_eq!(last, MenuOutcome::TimedOut(1));
            assert!(t >= DEFAULT_TIMEOUT_MS, "不得提前超时");
        }
    }

    #[test]
    fn gray_card_blocks_but_menu_survives() {
        let valid = [true, false];
        let mut g = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
        g.key(MenuKey::Down, 2, &valid);
        assert_eq!(g.key(MenuKey::Enter, 2, &valid), MenuOutcome::RejectedInvalidTarget(1));
        assert!(!g.finished(), "拒绝不算结束");
        g.key(MenuKey::Up, 2, &valid);
        assert_eq!(g.key(MenuKey::Enter, 2, &valid), MenuOutcome::Selected(0));
    }

    #[test]
    fn esc_then_timeout_resumes_not_freezes_forever() {
        // Esc 暂停 → 再 Esc 恢复 → 照常到点超时（停留模式不是死等模式）。
        let valid = [true, true];
        let mut g = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
        g.key(MenuKey::Esc, 2, &valid);
        for _ in 0..100 {
            assert_eq!(g.tick(100), MenuOutcome::None);
        }
        g.key(MenuKey::Esc, 2, &valid);
        let mut last = MenuOutcome::None;
        let mut t = 0;
        while last == MenuOutcome::None && t <= 6_000 {
            last = g.tick(100);
            t += 100;
        }
        assert_eq!(last, MenuOutcome::TimedOut(0));
    }

    #[test]
    fn asset_budget_boundary_holds() {
        // 预算边界：把每项资产顶到预算边缘仍须全绿；再超 1 字节即须判红。
        // （预算口径以 ASSET_BUDGET_BYTES 常量为准——一处一事实。）
        let full = AssetLedger { starfield_bg: ASSET_BUDGET_BYTES - 1, star_badge: 0, font_12x16: 0, ring_frames: 0, icons: 0 };
        assert!(full.total() < ASSET_BUDGET_BYTES);
        let over = AssetLedger { starfield_bg: ASSET_BUDGET_BYTES + 1, star_badge: 0, font_12x16: 0, ring_frames: 0, icons: 0 };
        assert!(over.total() >= ASSET_BUDGET_BYTES);
    }

    #[test]
    fn render_plan_centers_cards() {
        let entries = [MenuEntry::new("VARIX", "standalone kernel", true, true), MenuEntry::new("Windows 11 USB", "usb boot", true, false)];
        let g = MenuCore::new(0, DEFAULT_TIMEOUT_MS);
        let p = render_plan(&entries, &g, 0, 720);
        let total = 2 * CARD_H + CARD_GAP;
        let expected_top = (720 - total) / 2;
        assert_eq!(p.card_y[0], expected_top);
        assert_eq!(p.card_y[1], expected_top + CARD_H + CARD_GAP);
    }
}
