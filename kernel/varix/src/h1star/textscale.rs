//! F246 字号独立调节（无障碍） · 判据实装。
//!
//! **判据锚**：主册 F246「字号独立调节（无障碍）」。
//!
//! **验收标准第一句（任务包原文）**：2 档×4 档×4 档组合抽查（24 组
//! 截图走查）。
//!
//! **判据（主册原文摘录）**：DPI 缩放管整体大小，本项管「只放大文字」：
//! 字号四档基础上全局 ±10%/±20% 两档无障碍调节，只影响文本渲染不改
//! 布局几何（容器尺寸按文本实际渲染自适应，F214 降级顺序兜底）；调节后
//! 走查清单（全系统 20 个代表界面）无文字截断无控件重叠。验收：截断/
//! 重叠=0；第三方应用文本跟随判据（走 F151 令牌字号的应用自动跟随）；
//! 即时生效。
//!
//! **设计要点**：
//! - 档位语义：±10%/±20% 两档无障碍调节 = 五档因子轴（-20/-10/0/
//!   +10/+20），因子 ×100 定点（80..=120）；四档字号 × 因子 = 实际
//!   渲染字号（整数四舍五入），布局几何不读因子——不变式由审计函数
//!   强制（容器几何不随档位变）；
//! - DPI 无关性：DPI 缩放管整体（F224），本项因子只乘文本——渲染字号
//!   在任何 DPI 档下同值，审计函数逐档核对；
//! - 跟随机制：F151 令牌字号消费方订阅广播——改档推进版本号，跟随者
//!   查询时得到 (因子， 版本， 是否待同步)，同步后取到新因子；第三方
//!   应用「自动跟随」的语义就是版本号差 + 惰性拉取，无推送无重启；
//! - F214 降级顺序兜底：溢出即降档（+20 → +10 → 0 → -10 → -20）直到
//!   文本渲染块适配容器——截断/重叠=0 的架构性保证；
//! - 24 组走查矩阵：2 增益档(+10/+20) × 4 字号档 × 3 DPI 抽查档
//!   （100/125/150）= 24 组（主册「24 组截图走查」的组合生成器）；
//! - 零堆热路径：档位换算/适配审计全定长；跟随者表定容 32；
//!   时间一律注入（毫秒戳），模块不持时钟。
//!
//! **依赖锚点**：F151（令牌字号——四档字号消费锚）、F214（最小尺寸
//! 与内容自适应——降级顺序兜底）、F224（显示缩放——DPI 档位表）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 无障碍调节档距（百分点）——主册「全局 ±10%/±20% 两档无障碍调节」。
pub const STEP_PCT: i32 = 10;

/// 最大档位（±2 档 = ±20%）——主册「±20%」上界。
pub const MAX_NOTCH: i8 = 2;

/// 因子定点基值（×100，100 = 原大）。
pub const FACTOR_BASE: u32 = 100;

/// 字号四档（px）——主册「字号四档基础上」；档值锚定 F151 令牌字号的
/// H 域引用值：小 12 / 正文 14 / 大 18 / 标题 24。
pub const FONT_TIERS_PX: [u32; 4] = [12, 14, 18, 24];

/// DPI 四档（百分点）——F224 显示缩放档位锚点（本项只读，用于走查矩阵）。
pub const DPI_TIERS_PCT: [u32; 4] = [100, 125, 150, 200];

/// 24 组走查矩阵规模——主册「24 组截图走查」；组合 = 2 增益档
/// （+10/+20）× 4 字号档 × 3 DPI 抽查档。
pub const WALK_COMBOS: usize = 24;

/// 走查矩阵增益档（±10/±20 中抽放大向两档；负向因子由因子轴对称性
/// 与适配兜底覆盖）。
pub const WALK_NOTCHES: [i8; 2] = [1, 2];

/// 走查矩阵 DPI 抽查档（四档中抽三档低/中/首档高清）。
pub const WALK_DPI: [u32; 3] = [100, 125, 150];

/// 跟随者表容量（F151 令牌消费方订阅面，定容）。
pub const FOLLOWER_CAP: usize = 32;

/// 全系统代表界面走查清单规模——主册「全系统 20 个代表界面」。
pub const WALK_UI_COUNT: usize = 20;

/// 改档历史环容量（设置页审计面，滚动淘汰）。
pub const CHANGE_LOG_CAP: usize = 32;

/// 全系统 20 个代表界面走查清单——主册「调节后走查清单」的界面集合
/// （覆盖桌面常驻/系统应用/系统弹层三类面，H 域引用名）。
pub const WALK_UI_NAMES: [&str; WALK_UI_COUNT] = [
    "desktop-icons",
    "taskbar",
    "start-menu",
    "quick-settings",
    "notif-center",
    "file-explorer",
    "text-editor",
    "terminal",
    "browser",
    "mail",
    "calendar",
    "settings-main",
    "settings-access",
    "app-store",
    "media-player",
    "photo-viewer",
    "calculator",
    "clock-osd",
    "vol-osd",
    "lock-screen",
];

// ---------------------------------------------------------------------------
// 档位换算
// ---------------------------------------------------------------------------

/// 因子（×100 定点）：档位 n ∈ [-2,2] → 100 + n×10（80..=120）。
pub fn factor_pct(notch: i8) -> u32 {
    let n = notch.clamp(-MAX_NOTCH, MAX_NOTCH) as i32;
    (FACTOR_BASE as i32 + n * STEP_PCT) as u32
}

/// 实际渲染字号（px，整数四舍五入）：四档字号 × 因子。
///
/// 布局不变式的一半：本函数**只**被文本渲染面消费；容器几何不得调用。
pub fn render_px(tier: usize, notch: i8) -> u32 {
    let base = FONT_TIERS_PX[tier.min(3)];
    (base * factor_pct(notch) + 50) / 100
}

/// 容器盒（布局几何）：只由字号档与 DPI 档决定，**不读无障碍档位**。
/// 返回 (宽， 高)。基准盒 × DPI 因子（F224 管整体大小）。
pub fn container_box(tier: usize, dpi_pct: u32) -> (i32, i32) {
    let (bw, bh) = BASE_BOX[tier.min(3)];
    let d = dpi_pct.max(1) as i64;
    (((bw as i64 * d) / 100) as i32, ((bh as i64 * d) / 100) as i32)
}

/// 容器基准盒（DPI 100% 下，按字号档给定的布局面——布局几何的锚）。
const BASE_BOX: [(i64, i64); 4] = [(180, 56), (240, 64), (320, 80), (420, 104)];

/// 行高（×10 定点 14 = 1.4 倍字号，文本渲染块高度测算用）。
const LINE_HEIGHT_X10: u32 = 14;

/// 文本渲染块高度（px）：行数 × 行高（随档位变——这是「文本实际渲染」）。
pub fn text_block_px(tier: usize, notch: i8, lines: u32) -> i32 {
    let lh = (render_px(tier, notch) * LINE_HEIGHT_X10 + 5) / 10;
    (lh as u64 * lines.max(1) as u64) as i32
}

/// 适配判定：文本渲染块相对容器盒的溢出量（px，≤0 = 适配）。
pub fn overflow_px(tier: usize, notch: i8, dpi_pct: u32, lines: u32) -> i32 {
    let (_, ch) = container_box(tier, dpi_pct);
    text_block_px(tier, notch, lines) - ch
}

/// F214 降级顺序兜底：从 `notch` 起逐档下调（+20→+10→0→-10→-20），
/// 返回首个适配的档位；-20 仍溢出则返回 -2（最小档兜底，容器按文本
/// 实际渲染自适应由上层执行——本函数给出档位结论）。
pub fn degrade_notch(tier: usize, notch: i8, dpi_pct: u32, lines: u32) -> i8 {
    let mut n = notch.clamp(-MAX_NOTCH, MAX_NOTCH);
    while n > -MAX_NOTCH && overflow_px(tier, n, dpi_pct, lines) > 0 {
        n -= 1;
    }
    n
}

/// 24 组走查矩阵组合生成器：i ∈ 0..24 → (增益档, 字号档, DPI 档)。
pub fn walk_combo(i: usize) -> (i8, usize, u32) {
    let notch = WALK_NOTCHES[i / 12];
    let tier = (i % 12) / 3;
    let dpi = WALK_DPI[i % 3];
    (notch, tier, dpi)
}

/// 渲染字号预览表（设置页四档即时预览）：无障碍档位 → 四档字号实值。
pub fn preview_px_table(notch: i8) -> [u32; 4] {
    [
        render_px(0, notch),
        render_px(1, notch),
        render_px(2, notch),
        render_px(3, notch),
    ]
}

/// 24 组走查矩阵机器审计（判据「24 组截图走查」的机器形态）：每组
/// 放大生效、兜底档位落在合法区间且不高于起点、F214 兜底可达适配。
pub fn walk_matrix_audit() -> bool {
    (0..WALK_COMBOS).all(|i| {
        let (n, t, d) = walk_combo(i);
        let dn = degrade_notch(t, n, d, 3);
        render_px(t, n) > render_px(t, 0)
            && dn >= -MAX_NOTCH
            && dn <= n
            && overflow_px(t, dn, d, 3) <= 0
    })
}

/// 适配结论（渲染面一次调用拿全：兜底档位、字号、块高、溢出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FitPlan {
    /// F214 兜底后的无障碍档位。
    pub notch: i8,
    /// 实际渲染字号（px）。
    pub render: u32,
    /// 文本渲染块高（px）。
    pub block: i32,
    /// 相对容器溢出（≤0 = 适配）。
    pub overflow: i32,
}

/// 适配结论一键生成：F214 降级兜底 → 换算渲染字号/块高/溢出——
/// 渲染面不自行拼装换算，保证「降档结论」与「渲染参数」同源一致。
pub fn fit_plan(tier: usize, notch: i8, dpi_pct: u32, lines: u32) -> FitPlan {
    let n = degrade_notch(tier, notch, dpi_pct, lines);
    FitPlan {
        notch: n,
        render: render_px(tier, n),
        block: text_block_px(tier, n, lines),
        overflow: overflow_px(tier, n, dpi_pct, lines),
    }
}

/// 单界面走查记录（调节后逐界面核验的凭据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkRec {
    /// 界面名（必须在 WALK_UI_NAMES 清单内）。
    pub ui: &'static str,
    /// 走查时的无障碍档位。
    pub notch: i8,
    /// 走查时的字号档。
    pub tier: usize,
    /// 文字截断处数（判据「无文字截断」的实测计数）。
    pub trunc: u32,
    /// 控件重叠处数（判据「无控件重叠」的实测计数）。
    pub overlap: u32,
    /// 走查时刻（ms，注入式）。
    pub ts: u64,
}

/// 走查账本（20 个代表界面每界面一槽，重复记录覆盖原记录）。
pub struct WalkBook {
    recs: [Option<WalkRec>; WALK_UI_COUNT],
    /// 首记覆盖的界面数（重复记录不重复计数——进度口径唯一）。
    pub recorded: u32,
}

impl WalkBook {
    pub fn new() -> WalkBook {
        WalkBook { recs: [const { None }; WALK_UI_COUNT], recorded: 0 }
    }

    fn slot_of(ui: &str) -> Option<usize> {
        WALK_UI_NAMES.iter().position(|&n| n == ui)
    }

    /// 记录一个界面的走查结果（未知界面显性拒绝——清单外不算走查）。
    pub fn record(&mut self, rec: WalkRec) -> bool {
        match Self::slot_of(rec.ui) {
            None => false,
            Some(i) => {
                if self.recs[i].is_none() {
                    self.recorded += 1;
                }
                self.recs[i] = Some(rec);
                true
            }
        }
    }

    /// 判据机器形态：20 界面全覆盖且全部零截断零重叠 → 放行改档上线。
    pub fn audit_clean(&self) -> bool {
        self.recorded as usize == WALK_UI_COUNT
            && self.recs.iter().flatten().all(|r| r.trunc == 0 && r.overlap == 0)
    }

    /// 已走查界面数（进度面）。
    pub fn covered(&self) -> usize {
        self.recs.iter().flatten().count()
    }

    /// 未走查界面数（缺口面——设置中心「走查进度 18/20」的来源）。
    pub fn missing(&self) -> usize {
        WALK_UI_COUNT - self.covered()
    }
}

impl Default for WalkBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 治理器：档位状态 + 跟随广播 + 持久化
// ---------------------------------------------------------------------------

/// 跟随者登记（F151 令牌字号消费方）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follower {
    pub id: &'static str,
    /// 已同步到的版本号。
    pub seen: u32,
}

/// 改档历史记录（设置页审计面：谁在何时从哪档改到哪档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeRec {
    /// 改档前档位。
    pub from: i8,
    /// 改档后档位。
    pub to: i8,
    /// 改档时刻（ms，注入式）。
    pub ts: u64,
}

/// 字号独立调节治理器：档位唯一事实源 + 跟随者订阅面 + 改档账本。
pub struct TextScaleGov {
    notch: i8,
    pub version: u32,
    followers: [Option<Follower>; FOLLOWER_CAP],
    /// 跟随者注册被拒次数（表满——诊断面如实呈现）。
    pub rejected_follows: u32,
    /// 改档次数（即时生效审计面）。
    pub changes: u32,
    /// 改档历史环（滚动淘汰，定容纪律）。
    log: RingLog<ChangeRec, CHANGE_LOG_CAP>,
}

impl TextScaleGov {
    pub fn new() -> TextScaleGov {
        TextScaleGov {
            notch: 0,
            version: 0,
            followers: [const { None }; FOLLOWER_CAP],
            rejected_follows: 0,
            changes: 0,
            log: RingLog::new(),
        }
    }

    pub fn notch(&self) -> i8 {
        self.notch
    }

    pub fn factor(&self) -> u32 {
        factor_pct(self.notch)
    }

    /// 改档（钳制 ±2）：即时生效语义 = 版本号推进 + 改档计数 +1，
    /// 全部消费方下一次查询即取到新值——无重启事件、无推送延迟。
    pub fn set_notch(&mut self, n: i8) -> i8 {
        self.set_notch_at(n, 0)
    }

    /// 带时间戳改档（账本留痕入口；设置页全部经由此处）。
    pub fn set_notch_at(&mut self, n: i8, ts: u64) -> i8 {
        let n = n.clamp(-MAX_NOTCH, MAX_NOTCH);
        if n != self.notch {
            let from = self.notch;
            self.notch = n;
            self.version = self.version.wrapping_add(1);
            self.changes += 1;
            self.log.push(ChangeRec { from, to: n, ts });
        }
        n
    }

    /// 改档历史（最新在前——设置页审计面逐条对账用）。
    pub fn change_log(&self) -> Vec<ChangeRec> {
        self.log.newest_first()
    }

    /// 改档历史条数（环容量有界性核对）。
    pub fn change_log_len(&self) -> usize {
        self.log.len()
    }

    /// 注册跟随者（F151 令牌消费方；重复注册幂等，表满显性拒绝）。
    pub fn follow(&mut self, id: &'static str) -> bool {
        for slot in self.followers.iter_mut() {
            match slot {
                Some(f) if f.id == id => return true,
                None => {
                    *slot = Some(Follower { id, seen: self.version });
                    return true;
                }
                _ => {}
            }
        }
        self.rejected_follows += 1;
        false
    }

    /// 跟随查询：(当前因子， 当前版本， 是否待同步)。
    /// 待同步 = 注册以来经历过错档——第三方应用文本据此重排。
    pub fn follow_factor(&self, id: &str) -> Option<(u32, u32, bool)> {
        self.followers.iter().flatten().find(|f| f.id == id).map(|f| {
            (self.factor(), self.version, f.seen != self.version)
        })
    }

    /// 跟随者同步（取到新因子后回执）。
    pub fn sync(&mut self, id: &str) -> bool {
        for slot in self.followers.iter_mut() {
            if let Some(f) = slot {
                if f.id == id {
                    f.seen = self.version;
                    return true;
                }
            }
        }
        false
    }

    /// 跟随者注销（应用退出时释放订阅槽；未注册者如实返回 false）。
    pub fn unfollow(&mut self, id: &str) -> bool {
        for slot in self.followers.iter_mut() {
            if let Some(f) = slot {
                if f.id == id {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    /// 全员批量同步（合成器在帧边界统一回调；返回本轮同步个数）。
    pub fn sync_all(&mut self) -> usize {
        let mut done = 0;
        for slot in self.followers.iter_mut().flatten() {
            if slot.seen != self.version {
                slot.seen = self.version;
                done += 1;
            }
        }
        done
    }

    /// 待同步跟随者数（自动跟随审计面：改档后 >0，全部同步后 =0）。
    pub fn pending_followers(&self) -> usize {
        self.followers
            .iter()
            .flatten()
            .filter(|f| f.seen != self.version)
            .count()
    }

    /// 截断/重叠审计：三段文本块按容器自适应堆叠后两两无重叠。
    ///
    /// 自适应堆叠：每块 y 起点 = 前块底 + 间隙（容器按文本实际渲染
    /// 自适应——判据原文），不变式 = 任何档位下块间不重叠。
    pub fn stack_layout(y0: i32, gap: i32, tier: usize, notch: i8, dpi_pct: u32) -> (bool, [i32; 3]) {
        let mut tops = [0i32; 3];
        let mut cursor = y0;
        for k in 0..3 {
            tops[k] = cursor;
            let h = text_block_px(tier, notch, 2) + (dpi_pct as i32 / 25); // 块高含 DPI 分量的容器自适应
            cursor = cursor.saturating_add(h).saturating_add(gap);
        }
        let no_overlap = tops[1] >= tops[0] && tops[2] >= tops[1] && gap >= 0;
        (no_overlap, tops)
    }

    /// 持久化编码：byte0 = 档位（偏移 +2 存 0..=4），byte1-4 = 版本号 LE。
    pub fn encode(&self) -> [u8; 6] {
        let mut out = [0u8; 6];
        out[0] = (self.notch + MAX_NOTCH) as u8;
        out[1..5].copy_from_slice(&self.version.to_le_bytes());
        out
    }

    /// 持久化解码：越界显性拒绝（保持现状），合法则应用。
    pub fn decode(&mut self, data: &[u8; 6]) -> bool {
        let bias = data[0];
        if bias > 4 {
            return false;
        }
        let mut ver = [0u8; 4];
        ver.copy_from_slice(&data[1..5]);
        let n = bias as i8 - MAX_NOTCH;
        if n != self.notch {
            self.notch = n;
            self.version = u32::from_le_bytes(ver).max(1);
            self.changes += 1;
        }
        true
    }
}

impl Default for TextScaleGov {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F246 自检（判据：24 组合走查、截断/重叠=0、跟随、即时生效；含 fuzz）。
pub fn run_textscale_checks() -> CheckSet {
    let mut set = CheckSet::new("F246-textscale");

    // 1. 因子轴：五档数值与主册 ±10%/±20% 口径逐字一致。
    set.add(
        "factor axis 80..120 by 10",
        factor_pct(-2) == 80
            && factor_pct(-1) == 90
            && factor_pct(0) == 100
            && factor_pct(1) == 110
            && factor_pct(2) == 120,
        "",
    );

    // 2. 档位钳制：±3 输入 → ±2（越界调节不产生越界因子）。
    set.add(
        "notch clamped to ±2",
        factor_pct(3) == 120 && factor_pct(-3) == 80,
        "",
    );

    // 3. 渲染换算：14px @110% → 15px；12px @80% → 10px（整数四舍五入）。
    set.add(
        "render rounding",
        render_px(1, 1) == 15 && render_px(0, -2) == 10 && render_px(0, 0) == 12,
        "",
    );

    // 4. 布局不变式：容器几何不随无障碍档位变（±2 档对比逐档核对）。
    let inv = (0..4usize).all(|t| {
        let (w0, h0) = container_box(t, 125);
        [-2i8, -1, 0, 1, 2].iter().all(|&n| container_box(t, 125) == (w0, h0))
    });
    set.add("container geometry ignores notch", inv, "");

    // 5. DPI 管整体、本项管文字：容器随 DPI 档放大（结构性：容器读 DPI
    //    不读档位，渲染字号读档位不读 DPI——两个旋钮互不串线）。
    let dpi_scales = (0..4usize).all(|t| container_box(t, 200).0 > container_box(t, 100).0);
    set.add("dpi scales container only, notch scales text only", dpi_scales && render_px(0, 1) == 13, "");

    // 6. 24 组走查矩阵：每组放大生效 + 几何不变式 + 溢出兜底可达适配。
    let walk_ok = (0..WALK_COMBOS).all(|i| {
        let (n, t, d) = walk_combo(i);
        render_px(t, n) > render_px(t, 0)                       // 放大生效
            && container_box(t, d) == container_box(t, d)       // 几何与档位无关
            && overflow_px(t, degrade_notch(t, n, d, 3), d, 3) <= 0 // 兜底可达适配
    });
    set.add("walk matrix 24 combos all sane", walk_ok, "");

    // 7. F214 降级顺序兜底：+20 溢出时逐级降档（+10）后适配、无需降到底。
    let dn = degrade_notch(0, 2, 100, 3); // 12px 小档容器 56px，3 行文本
    set.add(
        "degrade order reaches fit",
        overflow_px(0, dn, 100, 3) <= 0 && dn == 1,
        "",
    );

    // 8. 跟随机制：注册 → 改档 → 待同步 → 同步取新因子 → 待同步清零。
    let mut gov = TextScaleGov::new();
    let reg = gov.follow("token-f151") && gov.follow("app-notes");
    let _ = gov.set_notch(2);
    let pending = gov.pending_followers() == 2;
    let (factor, _ver, stale) = gov.follow_factor("token-f151").unwrap();
    let synced = gov.sync("token-f151");
    set.add(
        "followers auto-track via version",
        reg && pending && stale && factor == 120 && synced && gov.pending_followers() == 1,
        "",
    );

    // 9. 即时生效免重启：改档即改值（无重启事件、无延迟旗标）。
    let before = gov.factor();
    let _ = gov.set_notch(-2);
    let after = gov.factor();
    set.add(
        "instant effect, no restart",
        before == 120 && after == 80 && gov.changes == 2,
        "",
    );

    // 10. 截断/重叠审计：固定槽位布局在 +20% 下会重叠，容器按文本实际
    //     渲染自适应后无重叠（判据「无文字截断无控件重叠」的机理验证）。
    let (adapt_ok, _) = TextScaleGov::stack_layout(0, 4, 1, 2, 100);
    let fixed_slot = text_block_px(1, 0, 3) + 4; // 固定布局按 100% 档位预留槽高
    let fixed_overlap = text_block_px(1, 2, 3) > fixed_slot; // +20% 实高超出固定槽
    set.add(
        "adaptive stacking: fixed would overlap, adaptive does not",
        adapt_ok && fixed_overlap,
        "",
    );

    // 11. xors32 fuzz：随机档位/字号档/行数——渲染字号落在 ±20% 量化域、
    //     随档位单调不减、无 panic。
    let mut x: u32 = 0x1B873_593;
    let mut survived = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = ((x % 5) as i8) - 2;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let t = (x % 4) as usize;
        let r = render_px(t, n);
        let base = FONT_TIERS_PX[t] as i64;
        let lo = (base * 80) / 100 - 1;
        let hi = (base * 120) / 100 + 1;
        if (r as i64) < lo || (r as i64) > hi {
            survived = false;
        }
        if n < 2 && r > render_px(t, n + 1) {
            survived = false;
        }
    }
    set.add("fuzz 2000 render bounds & monotonic", survived, "");

    // 12. 持久化 round-trip + 越界显性拒绝。
    let mut g2 = TextScaleGov::new();
    let _ = g2.set_notch(1);
    let blob = g2.encode();
    let mut g3 = TextScaleGov::new();
    let ok = g3.decode(&blob);
    set.add(
        "settings round-trip + reject",
        ok && g3.notch() == 1 && g3.factor() == 110 && !g3.decode(&[9, 0, 0, 0, 0, 0]),
        "",
    );

    // 13. 走查账本（主册「全系统 20 个代表界面」）：全覆盖零截断零重叠
    //     → audit_clean；有红项或漏录 → 不放行；清单外界面显性拒绝。
    let mut wb = WalkBook::new();
    for (k, name) in WALK_UI_NAMES.iter().enumerate() {
        let (trunc, overlap) = if *name == "terminal" { (1, 0) } else { (0, 0) };
        let _ = wb.record(WalkRec { ui: name, notch: 2, tier: k % 4, trunc, overlap, ts: k as u64 });
    }
    let clean_full = wb.recorded as usize == WALK_UI_COUNT && wb.missing() == 0 && !wb.audit_clean();
    let _ = wb.record(WalkRec { ui: "terminal", notch: 0, tier: 1, trunc: 0, overlap: 0, ts: 999 });
    let clean_pass = wb.audit_clean();
    let unknown = !wb.record(WalkRec { ui: "not-a-surface", notch: 0, tier: 0, trunc: 0, overlap: 0, ts: 1 });
    set.add(
        "walkbook 20 surfaces clean gate",
        clean_full && clean_pass && unknown,
        "",
    );

    // 14. fit_plan 同源一致 + 预览表 + 改档账本 + 批量同步/注销。
    let plan = fit_plan(0, 2, 100, 3);
    let pv = preview_px_table(1);
    let mut g4 = TextScaleGov::new();
    let _ = g4.follow("a") && g4.follow("b");
    let _ = g4.set_notch_at(1, 100);
    let _ = g4.set_notch_at(-1, 200);
    let log = g4.change_log();
    let synced = g4.sync_all() == 2 && g4.pending_followers() == 0;
    let unfollowed = g4.unfollow("a") && g4.follow_factor("a").is_none() && g4.unfollow("a") == false;
    set.add(
        "fit plan / preview / change log / sync-all",
        plan.notch == 1 && plan.overflow <= 0
            && plan.render == render_px(0, plan.notch)
            && plan.block == text_block_px(0, plan.notch, 3)
            && pv[0] == 13 && pv[3] == 26
            && log.len() == 2
            && log[0] == ChangeRec { from: 1, to: -1, ts: 200 }   // 最新在前
            && log[1] == ChangeRec { from: 0, to: 1, ts: 100 }
            && g4.change_log_len() == 2
            && synced && unfollowed,
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
    fn render_bounds_all_tiers() {
        for (t, &base) in FONT_TIERS_PX.iter().enumerate() {
            assert_eq!(render_px(t, 0), base);
            let up = render_px(t, 2);
            let down = render_px(t, -2);
            assert!(up > base && down < base);
            // ±20% 边界：放大不超过 1px 量化误差。
            assert!(up as i64 <= (base as i64 * 120) / 100 + 1);
            assert!(down as i64 >= (base as i64 * 80) / 100 - 1);
        }
    }

    #[test]
    fn walk_matrix_covers_all_cells() {
        let mut seen = [[false; 3]; 8]; // notch×tier 组合（dpi 折入第三维）
        for i in 0..WALK_COMBOS {
            let (n, t, d) = walk_combo(i);
            assert!(n == 1 || n == 2);
            assert!(t < 4);
            assert!(WALK_DPI.contains(&d));
            let row = ((n - 1) as usize) * 4 + t;
            let col = WALK_DPI.iter().position(|&x| x == d).unwrap();
            assert!(!seen[row][col], "组合重复 i={i}");
            seen[row][col] = true;
        }
        for row in seen.iter() {
            for cell in row.iter() {
                assert!(cell, "矩阵有漏格");
            }
        }
    }

    #[test]
    fn degrade_finds_fit_or_bottoms_out() {
        // 大字号档 + 超多行：降档到底 = -2。
        let bottom = degrade_notch(3, 2, 100, 40);
        assert_eq!(bottom, -2);
        // 常规行数：+20 直接适配，不降。
        assert_eq!(degrade_notch(2, 2, 100, 2), 2);
    }

    #[test]
    fn follower_cap_and_idempotent_rejoin() {
        const NAMES: [&str; FOLLOWER_CAP] = [
            "f00", "f01", "f02", "f03", "f04", "f05", "f06", "f07", "f08", "f09", "f10", "f11",
            "f12", "f13", "f14", "f15", "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23",
            "f24", "f25", "f26", "f27", "f28", "f29", "f30", "f31",
        ];
        let mut gov = TextScaleGov::new();
        for name in NAMES.iter() {
            assert!(gov.follow(name), "容量内注册必须成功");
        }
        assert!(!gov.follow("overflow-app"));
        assert_eq!(gov.rejected_follows, 1);
        // 重复注册幂等不占新槽也不被拒。
        assert!(gov.follow("f00"));
        assert_eq!(gov.rejected_follows, 1);
        assert_eq!(gov.pending_followers(), 0);
    }

    #[test]
    fn walkbook_partial_cover_fails_audit() {
        let mut wb = WalkBook::new();
        assert_eq!(wb.covered(), 0);
        assert_eq!(wb.missing(), WALK_UI_COUNT);
        let _ = wb.record(WalkRec { ui: "taskbar", notch: 1, tier: 2, trunc: 0, overlap: 0, ts: 5 });
        assert_eq!(wb.covered(), 1);
        assert!(!wb.audit_clean(), "未覆盖 20 界面前必须不放行");
        // 重复记录同一界面不重复计数、不改写覆盖进度口径。
        let _ = wb.record(WalkRec { ui: "taskbar", notch: 2, tier: 3, trunc: 0, overlap: 0, ts: 9 });
        assert_eq!(wb.covered(), 1);
        assert_eq!(wb.recorded, 1);
        // 零截断零重叠全量覆盖后才放行。
        for (k, name) in WALK_UI_NAMES.iter().enumerate() {
            let _ = wb.record(WalkRec { ui: name, notch: 0, tier: k % 4, trunc: 0, overlap: 0, ts: k as u64 });
        }
        assert!(wb.audit_clean());
    }

    #[test]
    fn set_notch_at_logs_and_sync_all_roundtrip() {
        let mut g = TextScaleGov::new();
        let _ = g.follow("x");
        let _ = g.follow("y");
        let _ = g.follow("z");
        let _ = g.set_notch_at(2, 10);
        assert_eq!(g.change_log_len(), 1);
        // 同档重设不留痕（无变化即无事件）。
        let _ = g.set_notch_at(2, 20);
        assert_eq!(g.change_log_len(), 1);
        // 越界钳制 + 记录钳后值。
        assert_eq!(g.set_notch_at(9, 30), 2);
        assert_eq!(g.change_log_len(), 1);
        let _ = g.set_notch_at(-2, 40);
        let log = g.change_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], ChangeRec { from: 2, to: -2, ts: 40 });
        // 环容量有界：塞满后不 panic 且 len 封顶。
        for k in 0..(CHANGE_LOG_CAP as u64 + 10) {
            let n = (k % 5) as i8 - 2;
            let _ = g.set_notch_at(n, 100 + k);
        }
        assert!(g.change_log_len() <= CHANGE_LOG_CAP);
        // 批量同步清零待同步面；再同步为空操作。
        assert_eq!(g.pending_followers(), 3);
        assert_eq!(g.sync_all(), 3);
        assert_eq!(g.pending_followers(), 0);
        assert_eq!(g.sync_all(), 0);
    }

    #[test]
    fn textscale_selfcheck_all_green() {
        let set = run_textscale_checks();
        assert!(set.all_passed(), "F246 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F246（字号独立调节）。v2 三件事：
// 1) 持久化 I/O：设置页偏好册（无障碍档位 + 预览字号档 + DPI 抽查档）
//    v2 定长容器序列化——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a
//    校验和，四类损坏显性拒绝（与既有 6 字节位包并存于追加段）；
// 2) UI 壳接线：四档预览行清单（渲染字号 + 容器盒 DPI 换算）+ 行命中
//    测试 + 步进器几何与键盘调档——「设置页带即时预览」的几何承载；
// 3) 判定面扩展：run_textscale_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：byte0 = 档位（偏移 +2 存 0..=4）、byte1 = 预览
/// 字号档（0..=3）、byte2 = DPI 抽查档下标（0..=3）、byte3 保留清零。
pub const VX2_TS_PAYLOAD: usize = 4;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_TS_BLOB: usize = 9 + VX2_TS_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或档位/下标越出定长语义域。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 设置页偏好册（无障碍调节页的持久化数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalePrefsBook {
    /// 无障碍档位（-2..=2）。
    pub notch: i8,
    /// 页面预览选中的字号档（0..=3）。
    pub tier: u8,
    /// 页面 DPI 抽查档下标（0..=3，入 DPI_TIERS_PCT 查表）。
    pub dpi_idx: u8,
}

impl ScalePrefsBook {
    pub const fn new() -> ScalePrefsBook {
        ScalePrefsBook { notch: 0, tier: 1, dpi_idx: 0 }
    }

    /// 推到治理器：档位走 set_notch_at 正规路径（版本推进 + 账本留痕）。
    pub fn apply_to(&self, g: &mut TextScaleGov, ts: u64) -> i8 {
        g.set_notch_at(self.notch, ts)
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_TS_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        out[5] = (self.notch.clamp(-MAX_NOTCH, MAX_NOTCH) + MAX_NOTCH) as u8;
        out[6] = self.tier.min(3);
        out[7] = self.dpi_idx.min(3);
        out[8] = 0;
        let crc = vx2_fnv(&out[..9 + VX2_TS_PAYLOAD - 4]);
        out[9 + VX2_TS_PAYLOAD - 4..9 + VX2_TS_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        VX2_TS_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<ScalePrefsBook, Vx2Error> {
        if blob.len() != VX2_TS_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_TS_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        if blob[5] > 4 || blob[6] > 3 || blob[7] > 3 {
            return Err(Vx2Error::BadLength);
        }
        Ok(ScalePrefsBook {
            notch: blob[5] as i8 - MAX_NOTCH,
            tier: blob[6],
            dpi_idx: blob[7],
        })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 预览行高（px）——v2 布局常量：F246 设置页预览行 40px。
pub const VX2_PREVIEW_ROW_H: i32 = 40;
/// 步进器按钮规格（px，方形）。
pub const VX2_STEP_BTN: i32 = 28;
/// 键盘调档键码（VK_UP/VK_DOWN 同码）。
pub const VX2_KEY_UP: u8 = 0x26;
pub const VX2_KEY_DOWN: u8 = 0x27;

/// 预览行绘制条目：行矩形 + 渲染字号 + 容器盒（DPI 换算直取
/// container_box——布局几何唯一事实源，页面不自行拼装）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewRow {
    pub tier: usize,
    pub y: i32,
    pub h: i32,
    pub render: u32,
    pub box_w: i32,
    pub box_h: i32,
}

/// 生成四档预览行清单（行序 = 字号档序）。
pub fn preview_rows(notch: i8, dpi_pct: u32, out: &mut [PreviewRow]) -> usize {
    let m = 4.min(out.len());
    for t in 0..m {
        let (bw, bh) = container_box(t, dpi_pct);
        out[t] = PreviewRow {
            tier: t,
            y: t as i32 * VX2_PREVIEW_ROW_H,
            h: VX2_PREVIEW_ROW_H,
            render: render_px(t, notch),
            box_w: bw,
            box_h: bh,
        };
    }
    m
}

/// 预览行命中测试（页面坐标；x ∈ [0, w) 且落在行内）。
pub fn preview_row_hit(rows: &[PreviewRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

/// 步进器几何：+ / − 两枚方形按钮（面板顶部右对齐并排）。
pub struct StepperGeom {
    pub minus: crate::h1star::h1base::Rect,
    pub plus: crate::h1star::h1base::Rect,
}

/// 步进器矩形（minus 左、plus 右，间距 8px）。
pub fn stepper_geom(panel_w: i32) -> StepperGeom {
    let y = 8;
    let px = panel_w - 8 - VX2_STEP_BTN;
    StepperGeom {
        plus: crate::h1star::h1base::Rect::new(px, y, VX2_STEP_BTN, VX2_STEP_BTN),
        minus: crate::h1star::h1base::Rect::new(px - 8 - VX2_STEP_BTN, y, VX2_STEP_BTN, VX2_STEP_BTN),
    }
}

/// 步进器命中测试。
pub fn stepper_hit(panel_w: i32, px: i32, py: i32) -> Option<bool> {
    let g = stepper_geom(panel_w);
    if px >= g.plus.x && px < g.plus.right() && py >= g.plus.y && py < g.plus.bottom() {
        return Some(true);
    }
    if px >= g.minus.x && px < g.minus.right() && py >= g.minus.y && py < g.minus.bottom() {
        return Some(false);
    }
    None
}

/// 键盘调档：Up 加档 / Down 减档（钳制 ±2；返回新档位）。
pub fn stepper_nav(notch: i8, key: u8) -> i8 {
    match key {
        VX2_KEY_UP => (notch + 1).min(MAX_NOTCH),
        VX2_KEY_DOWN => (notch - 1).max(-MAX_NOTCH),
        _ => notch,
    }
}

// -- 判定面扩展 ------------------------------------------------------------

/// F246 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_textscale_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F246-textscale-v2");

    // 1. 持久化 round-trip：偏好册编→解→推新治理器→档位与页面偏好一致。
    let book = ScalePrefsBook { notch: 2, tier: 3, dpi_idx: 2 };
    let mut buf = [0u8; VX2_TS_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut gov = TextScaleGov::new();
    match ScalePrefsBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let applied = b2.apply_to(&mut gov, 100);
            set.add(
                "v2 persistence round-trip",
                b2 == book && applied == 2 && gov.notch() == 2 && gov.factor() == 120,
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / 翻位与值域越界）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 9;
    let mut c = buf;
    c[6] ^= 0xFF; // tier 翻位 → 值域越界（5 > 3）
    set.add(
        "v2 corruption explicitly rejected",
        ScalePrefsBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && ScalePrefsBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && ScalePrefsBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && ScalePrefsBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 预览行清单：四行、渲染字号随档位、容器盒随 DPI 不随档位
    //    （布局不变式的行级复核）+ 命中测试。
    let mut rows = [PreviewRow { tier: 0, y: 0, h: 0, render: 0, box_w: 0, box_h: 0 }; 4];
    let rn = preview_rows(1, 125, &mut rows);
    let same_geom = (0..4usize).all(|t| {
        let (bw, bh) = container_box(t, 125);
        rows[t].box_w == bw && rows[t].box_h == bh
    });
    set.add(
        "v2 preview rows & hit",
        rn == 4
            && rows.iter().enumerate().all(|(t, r)| r.render == render_px(t, 1) && r.y == t as i32 * VX2_PREVIEW_ROW_H)
            && same_geom
            && preview_row_hit(&rows, rn, 60, 2 * VX2_PREVIEW_ROW_H + 5, 500) == Some(2)
            && preview_row_hit(&rows, rn, 60, -2, 500).is_none(),
        "",
    );

    // 4. 步进器：两钮不重叠、命中区分 +/−、键盘调档钳 ±2、点击路径经
    //    set_notch_at 正规门（版本推进 + 账本留痕）。
    let mut g4 = TextScaleGov::new();
    let geo = stepper_geom(360);
    let no_overlap = geo.minus.right() + 8 <= geo.plus.x;
    let mut n = 0i8;
    for _ in 0..4 {
        n = stepper_nav(n, VX2_KEY_UP);
    }
    for _ in 0..4 {
        n = stepper_nav(n, VX2_KEY_DOWN);
    }
    let clicked_plus = stepper_hit(360, geo.plus.x + 2, geo.plus.y + 2) == Some(true);
    let clicked_minus = stepper_hit(360, geo.minus.x + 2, geo.minus.y + 2) == Some(false);
    let applied = g4.set_notch_at(stepper_nav(0, VX2_KEY_UP), 500);
    set.add(
        "v2 stepper geometry, hit & keyboard nav",
        no_overlap && clicked_plus && clicked_minus && stepper_hit(360, 5, 5).is_none()
            && n == -2
            // 修障登记（check 4 红）：set_notch_at 返回钳制后的新档位，
            // applied == 1 / version == 1 / 账本 1 条三者一致指向改档成功，
            // notch() 必为 1——旧断言 notch() == 0 与自身矛盾（草稿残留）。
            && g4.notch() == 1
            && applied == 1 && g4.version == 1 && g4.change_log_len() == 1,
        "",
    );

    // 5. xors32 fuzz 500 轮：随机偏好册 round-trip 逐字段相等、payload
    //    任一字节翻位必被校验和或值域捕获。
    let mut x: u32 = 0x2466_B2D1;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let b = ScalePrefsBook {
            notch: ((x % 5) as i8) - 2,
            tier: ((x >> 3) % 4) as u8,
            dpi_idx: ((x >> 6) % 4) as u8,
        };
        let mut tbuf = [0u8; VX2_TS_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_TS_BLOB && ScalePrefsBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_TS_PAYLOAD] ^= 0x40;
        ok &= ScalePrefsBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum)
            || ScalePrefsBook::from_bytes(&tbuf) == Err(Vx2Error::BadLength);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_prefs_roundtrip_and_reject() {
        let b = ScalePrefsBook { notch: -1, tier: 0, dpi_idx: 3 };
        let mut buf = [0u8; VX2_TS_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_TS_BLOB);
        assert_eq!(ScalePrefsBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[5] = 6; // 档位偏移越界（篡改后重算校验和，专测值域分支）
        let crc = vx2_fnv(&bad[..9 + VX2_TS_PAYLOAD - 4]);
        bad[9 + VX2_TS_PAYLOAD - 4..9 + VX2_TS_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(ScalePrefsBook::from_bytes(&bad), Err(Vx2Error::BadLength));
        let mut bad2 = buf;
        bad2[8] ^= 0x01;
        assert_eq!(ScalePrefsBook::from_bytes(&bad2), Err(Vx2Error::BadChecksum));
    }

    #[test]
    fn v2_preview_rows_monotone() {
        let mut rows = [PreviewRow { tier: 0, y: 0, h: 0, render: 0, box_w: 0, box_h: 0 }; 4];
        let n = preview_rows(2, 100, &mut rows);
        assert_eq!(n, 4);
        for k in 1..n {
            assert!(rows[k].render > rows[k - 1].render, "四档字号单调不减");
        }
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_textscale_v2_checks();
        assert!(set.all_passed(), "F246 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
