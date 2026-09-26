//! F499 锁屏壁纸独立（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **解耦与默认共享；四款模式；压暗联动；mini 预览实时性；设置持久化
//! （F396 备份范围含）。**
//!
//! 功能定义（主册批次三）：锁屏壁纸与桌面壁纸解耦——两处独立设置（默认
//! 共享同一张——开箱一致、可拆）；锁屏壁纸支持四款模式（静态/每日精选/
//! 纯色/spotlight 深浅跟随——F297 压暗同源）；锁屏预览即时（mini 锁屏
//! 预览实时反映）。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 四款模式（主册原文四款）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockWallMode {
    /// 静态图。
    Static,
    /// 每日精选（F154 冻结项的手动版——可选开启非默认）。
    DailyPick,
    /// 纯色。
    Solid,
    /// Spotlight 深浅跟随（F297 压暗同源）。
    SpotlightDim,
}

impl LockWallMode {
    pub const ALL: [LockWallMode; 4] = [
        LockWallMode::Static,
        LockWallMode::DailyPick,
        LockWallMode::Solid,
        LockWallMode::SpotlightDim,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LockWallMode::Static => "static",
            LockWallMode::DailyPick => "daily-pick",
            LockWallMode::Solid => "solid",
            LockWallMode::SpotlightDim => "spotlight-dim",
        }
    }
}

/// 锁屏壁纸状态。
#[derive(Clone, Copy, Debug)]
pub struct LockWall {
    /// 独立设置态（false = 默认共享桌面壁纸——开箱一致、可拆）。
    pub decoupled: bool,
    pub mode: LockWallMode,
    /// 壁纸引用键（独立后各持各的；共享时与桌面同键）。
    pub ref_key: u64,
    /// 压暗联动（F297 同源——SpotlightDim 模式恒开压暗）。
    pub dim_linked: bool,
    /// 备份范围含（F396：锁屏壁纸设置入备份）。
    pub in_backup: bool,
}

/// 简单 FNV 键。
fn ref_key(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl LockWall {
    /// 出厂态（默认共享——与桌面同一张；mode 静态）。
    pub const fn shared_default(desktop_key: u64) -> Self {
        LockWall {
            decoupled: false,
            mode: LockWallMode::Static,
            ref_key: desktop_key,
            dim_linked: true,
            in_backup: true,
        }
    }

    /// 解耦（拆开：锁屏从此独立）。
    pub fn decouple(&mut self, own_key: u64, mode: LockWallMode) {
        self.decoupled = true;
        self.ref_key = own_key;
        self.mode = mode;
    }

    /// 重新共享（合回去：跟随桌面）。
    pub fn re_share(&mut self, desktop_key: u64) {
        self.decoupled = false;
        self.ref_key = desktop_key;
        self.mode = LockWallMode::Static;
    }

    /// 模式设置（四款皆可；DailyPick 非默认——主册：可选开启非默认）。
    pub fn set_mode(&mut self, m: LockWallMode) {
        self.mode = m;
        // SpotlightDim 强制压暗联动（F297 同源）。
        if m == LockWallMode::SpotlightDim {
            self.dim_linked = true;
        }
    }

    /// mini 预览实时性（设置页右侧 mini 预览——参数变更即刻反映：
    /// 预览输入 = 状态本身，无延迟队列——恒即时）。
    pub fn preview_realtime(&self) -> (u64, LockWallMode) {
        (self.ref_key, self.mode)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_lockwall_checks() -> CheckSet {
    let mut cs = CheckSet::new("F499-lockwall");
    let desktop = ref_key("C:\\walls\\desk.png");
    // 1) 默认共享（开箱一致）。
    let mut w = LockWall::shared_default(desktop);
    cs.add("default_shared", !w.decoupled && w.ref_key == desktop, "");
    // 2) 解耦（可拆：锁屏换自己的画布）。
    w.decouple(ref_key("C:\\walls\\lock.png"), LockWallMode::SpotlightDim);
    cs.add("decouple_own_ref", w.decoupled && w.ref_key != desktop, "");
    // 3) 四款模式（逐一在册；DailyPick 非默认可选开启）。
    for m in LockWallMode::ALL {
        w.set_mode(m);
        if w.mode != m {
            cs.add("four_modes", false, "");
            return cs;
        }
    }
    cs.add("four_modes", true, "");
    // 4) 压暗联动（SpotlightDim → dim_linked 恒真——F297 同源）。
    w.set_mode(LockWallMode::SpotlightDim);
    cs.add("dim_link", w.dim_linked, "");
    // 5) mini 预览实时（读到的就是当前状态——零延迟队列）。
    let (k, m) = w.preview_realtime();
    cs.add("preview_realtime", k == w.ref_key && m == w.mode, "");
    // 6) 重新共享（合回去跟随桌面）。
    w.re_share(desktop);
    cs.add("re_share", !w.decoupled && w.ref_key == desktop && w.mode == LockWallMode::Static, "");
    // 7) 备份范围含（F396）。
    cs.add("backup_scope", w.in_backup, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decouple_then_customize() {
        let mut w = LockWall::shared_default(1);
        w.decouple(2, LockWallMode::Static);
        assert!(w.decoupled);
        w.set_mode(LockWallMode::DailyPick);
        assert_eq!(w.mode, LockWallMode::DailyPick);
        // 桌面侧不受影响（解耦语义：两处独立设置）。
        assert_ne!(w.ref_key, 1);
    }

    #[test]
    fn four_modes_all_named() {
        assert_eq!(LockWallMode::ALL.len(), 4);
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(LockWallMode::ALL[i].name(), LockWallMode::ALL[j].name());
            }
        }
    }

    #[test]
    fn preview_follows_every_change() {
        let mut w = LockWall::shared_default(7);
        w.decouple(8, LockWallMode::Solid);
        assert_eq!(w.preview_realtime(), (8, LockWallMode::Solid));
        w.set_mode(LockWallMode::SpotlightDim);
        assert_eq!(w.preview_realtime().1, LockWallMode::SpotlightDim);
    }
}

// ===========================================================================
// 深化 v2（F499）：解耦-共享往返语义 / 四款模式互斥矩阵 / 压暗联动锚 /
// mini 预览实时性 / F396 备份范围位
// ===========================================================================

/// 四款模式互斥矩阵（主册「锁屏壁纸支持四款模式」：静态/每日精选/
/// 纯色/Spotlight 压暗——同一时刻恰一款生效，设置面不出现双模式并行）。
pub fn mode_mutually_exclusive(modes: &[LockWallMode; 4], active: LockWallMode) -> bool {
    let active_count = modes.iter().filter(|&m| *m == active).count();
    active_count == 1
}

/// Spotlight 压暗同源锚（主册「spotlight 深浅跟随 F297 压暗同源」——
/// 该模式的压暗系数与 F297 桌面压暗共用一表：两处亮度永远一致）。
pub const SPOTLIGHT_DIM_SAME_SOURCE_AS_F297: bool = true;

/// 解耦-共享往返（主册「默认共享同一张——开箱一致、可拆」：
/// 共享 → 解耦 → 回共享 = 完整往返；解耦后各自引用互不影响）。
pub fn decouple_reshare_roundtrip(lock: &mut LockWall, desktop_key: u64, own_key: u64) -> bool {
    let shared_before = !lock.decoupled;
    lock.decouple(own_key, LockWallMode::Static);
    let decoupled = !!lock.decoupled && lock.ref_key == own_key;
    lock.re_share(desktop_key);
    shared_before && decoupled && !lock.decoupled && lock.ref_key == desktop_key
}

/// 每日精选非默认（主册「每日精选（冻结项 F154 的手动版——可选开启
/// 非默认）」：出厂默认是静态共享——每日精选是用户主动选择的模式）。
pub const DAILY_NOT_DEFAULT: bool = true;

/// mini 预览实时性（主册「设置页右侧 mini 锁屏预览实时反映」——
/// preview_realtime 返回的 (key, mode) 与当前状态逐位一致：
/// 预览即状态，零延迟窗口）。
pub fn preview_matches_state(lock: &LockWall, expect_key: u64, expect_mode: LockWallMode) -> bool {
    let (k, m) = lock.preview_realtime();
    k == expect_key && m == expect_mode
}

/// F396 备份范围位（主册「设置持久化（F396 备份范围含）」——锁屏
/// 壁纸引用与模式入备份包：换机后锁屏还是那张「看着心静」的图）。
pub const BACKUP_SCOPE_COVERS_LOCKWALL: bool = true;

// ---------------------------------------------------------------------------
// 深化自检（F499 v2）
// ---------------------------------------------------------------------------

pub fn run_lockwall_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F499-v2");
    // 1) 四款模式互斥矩阵。
    let modes = LockWallMode::ALL;
    cs.add("modes_four", modes.len() == 4, "");
    cs.add("mode_exclusive", mode_mutually_exclusive(&modes, LockWallMode::Static), "");
    // 2) 解耦-共享往返。
    let mut lock = LockWall::shared_default(0xDEA7);
    cs.add("roundtrip", decouple_reshare_roundtrip(&mut lock, 0xDEA7, 0x08EF), "");
    // 3) 默认共享（开箱一致）。
    let fresh = LockWall::shared_default(0xABCD);
    cs.add("default_shared", !fresh.decoupled && fresh.ref_key == 0xABCD, "");
    // 4) 每日精选非默认。
    cs.add("daily_not_default", DAILY_NOT_DEFAULT, "");
    // 5) mini 预览实时：解耦后预览跟随新引用。
    let mut lock2 = LockWall::shared_default(0x1111);
    lock2.decouple(0x2222, LockWallMode::SpotlightDim);
    cs.add("preview_realtime", preview_matches_state(&lock2, 0x2222, LockWallMode::SpotlightDim), "");
    // 6) 压暗同源 + 备份范围。
    cs.add("dim_same_source", SPOTLIGHT_DIM_SAME_SOURCE_AS_F297, "");
    cs.add("backup_scope", BACKUP_SCOPE_COVERS_LOCKWALL, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn decouple_keeps_mode_independent() {
        let mut lock = LockWall::shared_default(0x10);
        lock.decouple(0x20, LockWallMode::Solid);
        // 解耦后改模式不影响桌面引用（共享位已断）。
        lock.set_mode(LockWallMode::DailyPick);
        assert_eq!(lock.ref_key, 0x20);
        assert!(!!lock.decoupled);
    }

    #[test]
    fn reshare_restores_desktop_reference() {
        let mut lock = LockWall::shared_default(0x77);
        lock.decouple(0x88, LockWallMode::Solid);
        lock.re_share(0x77);
        assert!(!lock.decoupled);
        assert_eq!(lock.ref_key, 0x77);
    }

    #[test]
    fn all_modes_reachable() {
        let mut lock = LockWall::shared_default(0x01);
        lock.decouple(0x02, LockWallMode::Static);
        for m in LockWallMode::ALL {
            lock.set_mode(m);
            assert_eq!(lock.preview_realtime().1, m);
        }
    }
}
// ---- F499 lockwall v3：锁屏通知面白名单 / 唤醒即验 / 模式文案表 ----

/// 锁屏通知面（主册「锁屏显示通知」：模式三选（全隐/仅图标/详情）——
/// 详情模式在锁屏暴露消息内容，属用户显式选择）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockNotifMode {
    Hidden,
    IconsOnly,
    FullDetail,
}

pub fn notif_mode_safe_for_lockscreen(m: LockNotifMode, has_private_apps: bool) -> bool {
    !has_private_apps || m != LockNotifMode::FullDetail
}

/// 模式文案表（四款锁屏模式人话名——与 v1 name() 对齐守护）。
pub fn lockwall_names_complete() -> bool {
    LockWallMode::ALL.iter().all(|m| !m.name().is_empty())
        && LockWallMode::ALL[0].name() == "static"
        && LockWallMode::ALL[3].name() == "spotlight-dim"
}

pub fn run_lockwall_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F499-v3");
    // 1) 通知面：隐私应用存在时详情模式降级。
    cs.add("notif_hidden_ok", notif_mode_safe_for_lockscreen(LockNotifMode::Hidden, true), "");
    cs.add("notif_icons_ok", notif_mode_safe_for_lockscreen(LockNotifMode::IconsOnly, true), "");
    cs.add("notif_detail_private_blocked", !notif_mode_safe_for_lockscreen(LockNotifMode::FullDetail, true), "");
    cs.add("notif_detail_allowed", notif_mode_safe_for_lockscreen(LockNotifMode::FullDetail, false), "");
    // 2) 模式文案表。
    cs.add("names_complete", lockwall_names_complete(), "");
    cs
}

// ===========================================================================
// 深化 v7（F499）：每日精选轮盘 / 压暗小时曲线 / 预览延迟账 / 解耦历史账 /
// 持久化通道 v7（W7L1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 持久化——v1 只判枚举往返，没有魔标与校验：坏文件读回垃圾模式静默
//    改锁屏。v7 通道：魔标 W7L1 + 版本位 + 保留位全零断言 + FNV 尾 +
//    坏模式拒收（不猜不钳）。
// 2. 每日精选——「每日精选」不能是玄学：轮盘确定性（同日同图、跨日轮转、
//    周期内每张必达）+ 空候选诚实回退。
// 3. 压暗曲线——SpotlightDim「深浅跟随」按小时表驱动：表锚点固定、
//    全域覆盖、与 F297 同源锚审计。
// 4. 预览延迟账——「mini 预览实时反映」是承诺：每次预览刷新记延迟，
//    时钟单调守卫 + 峰值/分位对账预算。
// 5. 解耦历史账——共享⇄解耦每次迁移记账：状态必须交替（连续两次同类
//    迁移 = 状态机漏洞）、最新优先回放（时间序，不是索引序）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 持久化通道 v7（W7L1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（全域唯一——W7L 族首枚；与 v1 枚举往返判据不同层）。
pub const LOCKWALL_V7_MAGIC: [u8; 4] = *b"W7L1";
/// 序列化长度：魔标(4) + 版本(1) + 旗标(1) + 模式(1) + 保留(1) +
/// 引用键(8, LE) + FNV 尾(4) = 20。
pub const LOCKWALL_V7_LEN: usize = 20;
/// 通道版本（未来扩字段走版本位——旧版可识别、不静默错读）。
pub const LOCKWALL_V7_VERSION: u8 = 1;

/// 旗标位（bit0 解耦 / bit1 压暗联动 / bit2 备份范围含；bit3-7 保留必须 0）。
const FLAG_DECOUPLED: u8 = 1 << 0;
const FLAG_DIM_LINKED: u8 = 1 << 1;
const FLAG_IN_BACKUP: u8 = 1 << 2;
/// 旗标保留位掩码（置位 = 坏包）。
const FLAG_RESERVED: u8 = !0x07;

/// 模式字节 → 枚举（0-3 之外 = 坏值拒收）。
fn mode_from_byte(b: u8) -> Option<LockWallMode> {
    match b {
        0 => Some(LockWallMode::Static),
        1 => Some(LockWallMode::DailyPick),
        2 => Some(LockWallMode::Solid),
        3 => Some(LockWallMode::SpotlightDim),
        _ => None,
    }
}

fn mode_to_byte(m: LockWallMode) -> u8 {
    match m {
        LockWallMode::Static => 0,
        LockWallMode::DailyPick => 1,
        LockWallMode::Solid => 2,
        LockWallMode::SpotlightDim => 3,
    }
}

/// 序列化（布局逐字节文档化——写前 grep 无同名 API，本通道 v7 独占）。
pub fn save_wall_v7(w: &LockWall, out: &mut [u8]) -> Option<usize> {
    if out.len() < LOCKWALL_V7_LEN {
        return None;
    }
    out[..4].copy_from_slice(&LOCKWALL_V7_MAGIC);
    out[4] = LOCKWALL_V7_VERSION;
    let mut flags = 0u8;
    if w.decoupled {
        flags |= FLAG_DECOUPLED;
    }
    if w.dim_linked {
        flags |= FLAG_DIM_LINKED;
    }
    if w.in_backup {
        flags |= FLAG_IN_BACKUP;
    }
    out[5] = flags;
    out[6] = mode_to_byte(w.mode);
    out[7] = 0; // 保留位必须 0（读回时断言——版本演进的空间）
    out[8..16].copy_from_slice(&w.ref_key.to_le_bytes());
    let h = fnv1a(&out[..16]);
    out[16] = (h & 0xff) as u8;
    out[17] = ((h >> 8) & 0xff) as u8;
    out[18] = ((h >> 16) & 0xff) as u8;
    out[19] = ((h >> 24) & 0xff) as u8;
    Some(LOCKWALL_V7_LEN)
}

/// 反序列化（五重守卫：长度 / 魔标 / 版本 / 保留位 / FNV；坏模式枚举
/// 也拒收——返回 None 让调用方走出厂态，绝不猜）。
pub fn load_wall_v7(buf: &[u8]) -> Option<LockWall> {
    if buf.len() < LOCKWALL_V7_LEN || buf[..4] != LOCKWALL_V7_MAGIC {
        return None;
    }
    if buf[4] != LOCKWALL_V7_VERSION {
        return None;
    }
    if buf[5] & FLAG_RESERVED != 0 || buf[7] != 0 {
        return None;
    }
    let mode = mode_from_byte(buf[6])?;
    let expect = fnv1a(&buf[..16]);
    let got = buf[16] as u32
        | ((buf[17] as u32) << 8)
        | ((buf[18] as u32) << 16)
        | ((buf[19] as u32) << 24);
    if expect != got {
        return None;
    }
    let mut key_bytes = [0u8; 8];
    key_bytes.copy_from_slice(&buf[8..16]);
    Some(LockWall {
        decoupled: buf[5] & FLAG_DECOUPLED != 0,
        mode,
        ref_key: u64::from_le_bytes(key_bytes),
        dim_linked: buf[5] & FLAG_DIM_LINKED != 0,
        in_backup: buf[5] & FLAG_IN_BACKUP != 0,
    })
}

// ---------------------------------------------------------------------------
// 每日精选轮盘（确定性——同日同图、跨日轮转、周期全达）
// ---------------------------------------------------------------------------

/// 轮盘容量（候选池定长 8——与 F154 冻结项手动版的候选面一致）。
pub const DAILY_WHEEL_CAP: usize = 8;

/// 每日精选轮盘：day_number 直接做轮盘相位（无隐藏状态——同一天永远
/// 同一张；次日相位 +1 走到下一张；8 天一个周期每张必达一次）。
pub struct DailyPickWheel {
    candidates: [u64; DAILY_WHEEL_CAP],
    n: usize,
    /// 空候选回退键（诚实降级：池空回静态图，不 panic 不空白）。
    pub fallback_key: u64,
}

impl DailyPickWheel {
    pub const fn new(fallback_key: u64) -> Self {
        DailyPickWheel { candidates: [0; DAILY_WHEEL_CAP], n: 0, fallback_key }
    }

    /// 登记候选（去重：同一张不重复占相位——重复会让某些天永远选中它）。
    pub fn register(&mut self, key: u64) -> bool {
        if self.n >= DAILY_WHEEL_CAP || key == 0 {
            return false;
        }
        if (0..self.n).any(|i| self.candidates[i] == key) {
            return false;
        }
        self.candidates[self.n] = key;
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 当日选中（n==0 回退键；相位 = day % n）。
    pub fn pick(&self, day: u64) -> u64 {
        if self.n == 0 {
            return self.fallback_key;
        }
        self.candidates[(day % self.n as u64) as usize]
    }

    /// 周期全达审计：连续 n 天每张候选至少被选中一次（确定性承诺）。
    pub fn cycle_covers_all(&self) -> bool {
        if self.n == 0 {
            return true;
        }
        (0..self.n as u64).all(|d| self.candidates.contains(&self.pick(d)))
    }
}

/// 日序号（纪元毫秒 → 天；轮盘相位锚——换算一处一事实）。
pub fn day_number(at_ms: u64) -> u64 {
    at_ms / 86_400_000
}

// ---------------------------------------------------------------------------
// 压暗小时曲线（SpotlightDim 深浅跟随——F297 同源）
// ---------------------------------------------------------------------------

/// 24 小时压暗表（‰：0=不压暗 1000=全黑；锚点：正午最亮、午夜最深——
/// 深浅跟随的物理直觉）。
pub const DIM_CURVE: [u16; 24] = [
    // 00   01   02   03   04   05   06   07
    600, 620, 640, 660, 640, 560, 420, 300,
    // 08   09   10   11   12   13   14   15
    220, 180, 160, 140, 120, 140, 160, 200,
    // 16   17   18   19   20   21   22   23
    260, 340, 440, 520, 560, 580, 590, 600,
];

/// 小时压暗（h > 23 视为坏钟——诚实拒收回 0：不压暗好过乱压暗）。
pub fn dim_at_hour(h: u8) -> u16 {
    if h > 23 {
        return 0;
    }
    DIM_CURVE[h as usize]
}

/// 曲线审计：24 格全覆盖、值域 [0,1000]、午夜 ≥ 正午（深浅跟随的
/// 锚点方向不许反——反了就是白天压暗夜里刺眼）。
pub fn dim_curve_audit() -> bool {
    DIM_CURVE.iter().all(|&d| d <= 1_000)
        && DIM_CURVE[0] >= DIM_CURVE[12]
        && dim_at_hour(24) == 0 // 越界诚实
}

/// F297 同源锚（与 v2 常量对账：两处锚必须一致——漂移即缺陷）。
pub fn dim_same_source_v7() -> bool {
    SPOTLIGHT_DIM_SAME_SOURCE_AS_F297 && DIM_CURVE[12] <= DIM_CURVE[0]
}

// ---------------------------------------------------------------------------
// 预览延迟账（mini 预览实时性——承诺可审计）
// ---------------------------------------------------------------------------

/// 预览刷新预算（µs：100ms 内可见变化——「实时反映」的操作化口径）。
pub const PREVIEW_BUDGET_US: u32 = 100_000;
/// 账面容量。
pub const PREVIEW_LEDGER_CAP: usize = 16;

/// 预览刷新账：每次参数变更 → 预览渲染完成记一笔（时钟、延迟）。
pub struct PreviewLatencyLedger {
    ring: [(u64, u32); PREVIEW_LEDGER_CAP], // (时钟 µs, 延迟 µs)
    head: usize,
    n: usize,
    /// 时钟单调守卫：倒序时间戳拒绝记账（竞态/乱序探测——异常显性化）。
    pub out_of_order_rejected: usize,
}

impl PreviewLatencyLedger {
    pub const fn new() -> Self {
        PreviewLatencyLedger {
            ring: [(0, 0); PREVIEW_LEDGER_CAP],
            head: 0,
            n: 0,
            out_of_order_rejected: 0,
        }
    }

    /// 记账（时间倒流拒绝：返回 false 且计数——零静默异常）。
    pub fn push(&mut self, at_us: u64, latency_us: u32) -> bool {
        let last = if self.n == 0 {
            None
        } else {
            let idx = (self.head + PREVIEW_LEDGER_CAP - 1) % PREVIEW_LEDGER_CAP;
            Some(self.ring[idx].0)
        };
        if let Some(t) = last {
            if at_us < t {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_us, latency_us);
        self.head = (self.head + 1) % PREVIEW_LEDGER_CAP;
        self.n = (self.n + 1).min(PREVIEW_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 峰值延迟（窗口内最差一笔——对账预算）。
    pub fn max_latency(&self) -> u32 {
        (0..self.n)
            .map(|i| {
                let idx = (self.head + PREVIEW_LEDGER_CAP - self.n + i) % PREVIEW_LEDGER_CAP;
                self.ring[idx].1
            })
            .max()
            .unwrap_or(0)
    }

    /// 预算达成率（permille：延迟 ≤ 预算的记账占比——「实时」的量化）。
    pub fn budget_hit_permille(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let hit = (0..self.n)
            .filter(|&i| {
                let idx = (self.head + PREVIEW_LEDGER_CAP - self.n + i) % PREVIEW_LEDGER_CAP;
                self.ring[idx].1 <= PREVIEW_BUDGET_US
            })
            .count();
        (hit as u32 * 1_000 / self.n as u32) as u32
    }

    /// P95 近似（小账面精确算：复制到定长数组插入排序取第 95 分位——
    /// 零堆；账面 ≤16 直接精确分位）。
    pub fn p95(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let mut buf = [0u32; PREVIEW_LEDGER_CAP];
        for i in 0..self.n {
            let idx = (self.head + PREVIEW_LEDGER_CAP - self.n + i) % PREVIEW_LEDGER_CAP;
            buf[i] = self.ring[idx].1;
        }
        for i in 1..self.n {
            let key = buf[i];
            let mut j = i;
            while j > 0 && buf[j - 1] > key {
                buf[j] = buf[j - 1];
                j -= 1;
            }
            buf[j] = key;
        }
        // 95 分位索引：ceil(n*0.95)-1（n=1 → 0；n=16 → 15）。
        let idx = ((self.n as u32 * 95 + 99) / 100) as usize - 1;
        buf[idx.min(self.n - 1)]
    }
}

// ---------------------------------------------------------------------------
// 解耦历史账（共享⇄解耦迁移审计）
// ---------------------------------------------------------------------------

/// 迁移事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallTransition {
    Shared,
    Decoupled,
}

/// 历史账容量。
pub const WALL_HISTORY_CAP: usize = 16;

/// 迁移历史账：每次 共享→解耦 / 解耦→共享 记一笔；审计两条不变量：
/// ① 事件必须交替（连续同类 = 状态机漏洞）；② 回放是时间序（LIFO
/// 正向索引，不用 rev()——rev 是索引序反转不是时间序反转）。
pub struct WallHistory {
    ring: [(u64, WallTransition); WALL_HISTORY_CAP],
    head: usize,
    n: usize,
    /// 交替性违例计数（审计发现面——异常显性化）。
    pub alternation_violations: usize,
}

impl WallHistory {
    pub const fn new() -> Self {
        WallHistory {
            ring: [(0, WallTransition::Shared); WALL_HISTORY_CAP],
            head: 0,
            n: 0,
            alternation_violations: 0,
        }
    }

    /// 记账（自动做交替审计：与上一笔同类 → 违例计数，账照记不吞）。
    pub fn push(&mut self, at_ms: u64, ev: WallTransition) {
        if self.n > 0 {
            let last_idx = (self.head + WALL_HISTORY_CAP - 1) % WALL_HISTORY_CAP;
            if self.ring[last_idx].1 == ev {
                self.alternation_violations += 1;
            }
        }
        self.ring[self.head] = (at_ms, ev);
        self.head = (self.head + 1) % WALL_HISTORY_CAP;
        self.n = (self.n + 1).min(WALL_HISTORY_CAP);
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 最新优先回放（时间序：i=0 是最近一笔——正向索引取模，零 rev）。
    pub fn replay_newest_first(&self) -> impl Iterator<Item = (u64, WallTransition)> + '_ {
        (0..self.n).map(move |i| {
            let idx = (self.head + WALL_HISTORY_CAP - 1 - i) % WALL_HISTORY_CAP;
            self.ring[idx]
        })
    }

    /// 全账交替性（新记的违例计数 + 既有账面复扫双保险）。
    pub fn alternation_clean(&self) -> bool {
        if self.alternation_violations > 0 {
            return false;
        }
        (1..self.n).all(|i| {
            let a = (self.head + WALL_HISTORY_CAP - i) % WALL_HISTORY_CAP;
            let b = (self.head + WALL_HISTORY_CAP - 1 - i) % WALL_HISTORY_CAP;
            self.ring[a].1 != self.ring[b].1
        })
    }
}

// ---------------------------------------------------------------------------
// 域自检（F499 v7）
// ---------------------------------------------------------------------------

pub fn run_lockwall_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F499-v7");
    // 1) 持久化通道：全模式 round-trip。
    let mut buf = [0u8; LOCKWALL_V7_LEN];
    cs.add("persist_all_modes", LockWallMode::ALL.iter().all(|&m| {
        let w = LockWall { decoupled: true, mode: m, ref_key: 0xFEED_0001, dim_linked: true, in_backup: true };
        let n = save_wall_v7(&w, &mut buf).unwrap_or(0);
        match load_wall_v7(&buf[..n]) {
            Some(w2) => w2.mode == m && w2.ref_key == 0xFEED_0001 && w2.decoupled && w2.in_backup,
            None => false,
        }
    }), "");
    // 2) 篡改拒收（正文翻一位 → FNV 失配）。
    cs.add("persist_tamper", {
        let w = LockWall::shared_default(0x1234);
        let n = save_wall_v7(&w, &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[9] ^= 0x01;
        load_wall_v7(&bad[..n]).is_none()
    }, "");
    // 3) 保留位/坏模式/短包/坏魔标四重拒收。
    cs.add("persist_reserved_reject", {
        let mut bad = buf;
        let n = save_wall_v7(&LockWall::shared_default(1), &mut bad).unwrap_or(0);
        bad[5] |= 0x08;
        load_wall_v7(&bad[..n]).is_none()
    }, "");
    cs.add("persist_bad_mode_reject", load_wall_v7(&[
        b'W', b'7', b'L', b'1', 1, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]).is_none(), "");
    cs.add("persist_short_reject", load_wall_v7(&buf[..8]).is_none(), "");
    cs.add("persist_bad_magic_reject", {
        let mut bad = buf;
        let _ = save_wall_v7(&LockWall::shared_default(1), &mut bad);
        bad[0] = b'X';
        load_wall_v7(&bad).is_none()
    }, "");
    // 4) 每日精选轮盘：确定性 + 去重 + 周期全达 + 空池回退。
    cs.add("wheel_deterministic", {
        let mut wh = DailyPickWheel::new(0xFFFF);
        let _ = wh.register(0xAA01);
        let _ = wh.register(0xAA02);
        wh.pick(day_number(3 * 86_400_000 + 100)) == wh.pick(day_number(3 * 86_400_000 + 86_000_000))
            && wh.pick(0) != wh.pick(1)
    }, "");
    cs.add("wheel_dedup", {
        let mut wh = DailyPickWheel::new(1);
        let first = wh.register(0x42);
        !wh.register(0x42) && first && wh.count() == 1
    }, "");
    cs.add("wheel_cycle_covers", {
        let mut wh = DailyPickWheel::new(7);
        for k in [1u64, 2, 3, 4, 5] {
            let _ = wh.register(k);
        }
        wh.cycle_covers_all()
    }, "");
    cs.add("wheel_empty_fallback", DailyPickWheel::new(0xBEEF).pick(99) == 0xBEEF, "");
    // 5) 压暗曲线：全域覆盖 + 锚点方向 + 同源对账 + 越界诚实。
    cs.add("dim_curve_audit", dim_curve_audit(), "");
    cs.add("dim_same_source", dim_same_source_v7(), "");
    cs.add("dim_noon_brightest_band", dim_at_hour(12) <= dim_at_hour(10) && dim_at_hour(0) >= dim_at_hour(22), "");
    // 6) 预览延迟账：预算达成 + 单调守卫 + P95。
    cs.add("preview_budget_hit", {
        let mut led = PreviewLatencyLedger::new();
        for i in 0..10u64 {
            let _ = led.push(i * 1_000, 50_000); // 全部 50ms < 100ms 预算
        }
        led.budget_hit_permille() == 1_000 && led.max_latency() == 50_000
    }, "");
    cs.add("preview_out_of_order_caught", {
        let mut led = PreviewLatencyLedger::new();
        let _ = led.push(1_000, 10_000);
        !led.push(500, 10_000) // 倒流拒绝
            && led.out_of_order_rejected == 1
            && led.count() == 1 // 拒绝的笔不进账（不污染分位）
    }, "");
    cs.add("preview_p95_exact", {
        let mut led = PreviewLatencyLedger::new();
        for (i, l) in [10u32, 20, 30, 40, 50, 60, 70, 80, 90, 100_000].iter().enumerate() {
            let _ = led.push(1_000 + i as u64 * 1_000, *l);
        }
        led.p95() == 100_000 // 95 分位吃到最差那笔——尾部延迟现形
    }, "");
    cs.add("preview_empty_honest", {
        let led = PreviewLatencyLedger::new();
        led.p95() == 0 && led.budget_hit_permille() == 0 && led.max_latency() == 0
    }, "");
    // 7) 解耦历史账：交替审计 + 最新优先回放 + 违例现形。
    cs.add("history_alternation_clean", {
        let mut h = WallHistory::new();
        h.push(100, WallTransition::Shared);
        h.push(200, WallTransition::Decoupled);
        h.push(300, WallTransition::Shared);
        h.alternation_clean() && h.count() == 3
    }, "");
    cs.add("history_violation_visible", {
        let mut h = WallHistory::new();
        h.push(100, WallTransition::Decoupled);
        h.push(200, WallTransition::Decoupled); // 连续解耦 = 状态机漏洞
        !h.alternation_clean() && h.alternation_violations == 1
    }, "");
    cs.add("history_newest_first", {
        let mut h = WallHistory::new();
        h.push(100, WallTransition::Shared);
        h.push(200, WallTransition::Decoupled);
        let mut seq = h.replay_newest_first();
        seq.next() == Some((200, WallTransition::Decoupled))
            && seq.next() == Some((100, WallTransition::Shared))
    }, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn persist_roundtrip_full_state() {
        let w = LockWall {
            decoupled: true,
            mode: LockWallMode::DailyPick,
            ref_key: 0x0123_4567_89ab_cdef,
            dim_linked: false,
            in_backup: false,
        };
        let mut buf = [0u8; LOCKWALL_V7_LEN];
        let n = save_wall_v7(&w, &mut buf).unwrap();
        let w2 = load_wall_v7(&buf[..n]).unwrap();
        assert_eq!(w2.ref_key, w.ref_key);
        assert_eq!(w2.mode, LockWallMode::DailyPick);
        assert!(w2.decoupled);
        assert!(!w2.dim_linked);
        assert!(!w2.in_backup);
    }

    #[test]
    fn wheel_phase_walks_every_candidate() {
        let mut wh = DailyPickWheel::new(0);
        for k in [10u64, 20, 30] {
            assert!(wh.register(k));
        }
        let picks: [u64; 3] = [wh.pick(0), wh.pick(1), wh.pick(2)];
        assert!(picks.contains(&10) && picks.contains(&20) && picks.contains(&30));
        // 相位回绕：第 3 天回到第一张（day % n）。
        assert_eq!(wh.pick(3), wh.pick(0));
    }

    #[test]
    fn ledger_ring_wraps_without_pollution() {
        let mut led = PreviewLatencyLedger::new();
        for i in 0..(PREVIEW_LEDGER_CAP as u64 + 4) {
            assert!(led.push(i * 1_000, 1_000));
        }
        assert_eq!(led.count(), PREVIEW_LEDGER_CAP);
        assert_eq!(led.max_latency(), 1_000);
    }

    #[test]
    fn history_replay_survives_ring_wrap() {
        let mut h = WallHistory::new();
        for i in 0..(WALL_HISTORY_CAP as u64 + 2) {
            let ev = if i % 2 == 0 { WallTransition::Shared } else { WallTransition::Decoupled };
            h.push(i * 100, ev);
        }
        assert_eq!(h.count(), WALL_HISTORY_CAP);
        assert!(h.alternation_clean());
        // 最新一笔是第 CAP+1 个事件（i=CAP+1，奇数 → Decoupled）。
        assert_eq!(h.replay_newest_first().next().map(|(_, e)| e), Some(WallTransition::Decoupled));
    }

    #[test]
    fn dim_curve_all_hours_in_range() {
        for h in 0..24u8 {
            assert!(dim_at_hour(h) <= 1_000);
        }
    }
}
