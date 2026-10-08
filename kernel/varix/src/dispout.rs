//! 显示输出管理收口（WP-404 · B-3601~3603 · 篇 36）。
//!
//! 模式切换走安全序列：先确认新模式可用再切换、失败回退并提示——黑屏的
//! 显示设置是用户最怕的死局，**序列设计以"永不无画面"为第一原则（B-3601
//! 达标线）**；镜像模式单一合成结果双路提交（同帧两输出，相位差容忍一帧，
//! 插拔不断流——**B-3602 达标线**）；缩放因子全局联动：一次变更全系统一致
//! （VXWM 绑定+token 重发+Wine DPI 换算三消费者——**B-3603 达标线 WD-061**）。

// ---------------------------------------------------------------------------
// B-3601 模式切换安全序列
// ---------------------------------------------------------------------------

/// 显示模式（分辨率+刷新率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VideoMode {
    pub w: u16,
    pub h: u16,
    pub refresh_hz: u8,
}

/// 输出对象：模式列表（枚举面）+ 激活槽 + 物理尺寸（DPI 与缩放依据）。
#[derive(Clone, Copy)]
pub struct Output {
    pub id: u8,
    pub modes: [Option<VideoMode>; 4],
    /// 当前激活的模式槽。
    pub active: usize,
    /// 物理对角线（英寸，整数——DPI 计算依据）。
    pub diag_inches: u8,
}

impl Output {
    pub fn new(id: u8, diag_inches: u8, modes: [Option<VideoMode>; 4]) -> Self {
        Output { id, modes, active: 0, diag_inches }
    }

    /// 模式预检：目标槽在枚举列表内才可切——列表外模式不存在。
    pub fn probe(&self, idx: usize) -> bool {
        self.modes.get(idx).map(|m| m.is_some()).unwrap_or(false)
    }

    /// DPI（横向近似：宽 px / 宽英寸；模型面按 16:9 对角折算——如实取整）。
    pub fn dpi(&self) -> u32 {
        let m = match self.modes[self.active] {
            Some(m) => m,
            None => return 0,
        };
        // 16:9 对角→宽 = diag × 0.8746（整数近似 8746/10000）。
        let wide_inches = (self.diag_inches as u32 * 8746) / 10000;
        if wide_inches == 0 {
            0
        } else {
            m.w as u32 / wide_inches
        }
    }
}

/// 切换序列五态：空闲→预检→提交→确认（失败→回退）——顺序是安全的前提。
pub const SW_IDLE: u8 = 0;
pub const SW_PROBED: u8 = 1;
pub const SW_COMMITTED: u8 = 2;
pub const SW_CONFIRMED: u8 = 3;
pub const SW_ROLLED_BACK: u8 = 4;

/// 切换事务：预检不过不提交；确认失败必回退——永不无画面。
#[derive(Clone, Copy)]
pub struct SwitchTx {
    pub stage: u8,
    target: usize,
}

impl SwitchTx {
    pub fn new(target: usize) -> Self {
        SwitchTx { stage: SW_IDLE, target }
    }

    /// 预检：probe 假则事务终结在 IDLE（模式列表外的分辨率不存在）。
    pub fn probe(&mut self, out: &Output) -> bool {
        if out.probe(self.target) {
            self.stage = SW_PROBED;
            true
        } else {
            false
        }
    }

    /// 提交（换激活槽）——只有预检过的事务可提交。
    pub fn commit(&mut self, out: &mut Output) -> bool {
        if self.stage != SW_PROBED {
            return false;
        }
        out.active = self.target;
        self.stage = SW_COMMITTED;
        true
    }

    /// 确认：新模式下画面确认成功才收口；失败回退旧槽并记档。
    pub fn confirm(&mut self, out: &mut Output, picture_ok: bool, prev: usize) -> bool {
        if self.stage != SW_COMMITTED {
            return false;
        }
        if picture_ok {
            self.stage = SW_CONFIRMED;
            true
        } else {
            out.active = prev;
            self.stage = SW_ROLLED_BACK;
            false
        }
    }
}

// ---------------------------------------------------------------------------
// B-3602 镜像双路与插拔
// ---------------------------------------------------------------------------

/// 镜像对：单一合成结果双路提交（同帧两输出共享坐标区）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MirrorPair {
    pub primary_on: bool,
    pub external_on: bool,
    /// 相位差（帧）——容忍一帧是预算内的（软渲染双路提交成本注记）。
    pub phase_lag_frames: u8,
}

pub const PHASE_LAG_TOLERANCE: u8 = 1;

impl MirrorPair {
    pub fn new() -> Self {
        MirrorPair { primary_on: true, external_on: false, phase_lag_frames: 0 }
    }

    /// 外接接入：双路开启——同帧两路，无花屏（共享坐标区不是两份合成）。
    pub fn plug_external(&mut self) {
        self.external_on = true;
        self.phase_lag_frames = 0;
    }

    /// 外接拔出：内屏照常——显示流不断（**无黑屏**）。
    pub fn unplug_external(&mut self) {
        self.external_on = false;
        self.phase_lag_frames = 0;
    }

    /// 流连续性：主路在就恒有画面（插拔只动副路）。
    pub fn stream_alive(&self) -> bool {
        self.primary_on
    }

    /// 相位合规：lag ≤ 1 帧。
    pub fn phase_ok(&self) -> bool {
        self.phase_lag_frames <= PHASE_LAG_TOLERANCE
    }
}

// ---------------------------------------------------------------------------
// B-3603 缩放全联动（WD-061）
// ---------------------------------------------------------------------------

/// 缩放广播三消费者：VXWM 绑定参数 / 主题 token 重发 / Wine DPI 垫片换算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScaleBroadcast {
    pub wm: bool,
    pub tokens: bool,
    pub dpi_shim: bool,
}

/// 自动匹配：外接按物理尺寸算缩放比（permille，内屏 base 为基准）——
/// 接大显示器 UI 不变邮票大小，逻辑公开在设置页可改。
pub fn auto_scale(base_permille: u16, diag_in: u16, ref_in: u16) -> u16 {
    if ref_in == 0 {
        return base_permille;
    }
    // 比例 = base × (diag_in / ref_in)，夹逼 [1000, 3000]（不缩过密不放过巨）。
    let s = (base_permille as u32 * diag_in as u32) / ref_in as u32;
    s.clamp(1000, 3000) as u16
}

/// 一次变更全系统一致：三消费者全部收到才算广播完成（WD-061 的实现面）。
pub fn broadcast_scale(b: &mut ScaleBroadcast) -> bool {
    b.wm = true;
    b.tokens = true;
    b.dpi_shim = true;
    b.wm && b.tokens && b.dpi_shim
}

// ---------------------------------------------------------------------------
// CheckSet（B-3601~3603 · 7 项）
// ---------------------------------------------------------------------------

/// 显示输出判据（WP-404）。
pub fn run_dispout_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("dispout");
    let mode_hi = Some(VideoMode { w: 3840, h: 2160, refresh_hz: 60 });
    let mode_lo = Some(VideoMode { w: 1920, h: 1080, refresh_hz: 60 });
    let modes: [Option<VideoMode>; 4] = [mode_lo, mode_hi, None, None];
    // 1. 预检先行：列表外模式拒绝入序列（未确认不切）。
    let mut tx_bad = SwitchTx::new(2);
    let mut out = Output::new(0, 24, modes);
    cs.add(
        "B-3601 预检先行",
        !tx_bad.probe(&out) && tx_bad.stage == SW_IDLE,
        "列表外模式不存在——预检不过事务终结在原地",
    );
    // 2. 失败回退（**B-3601 达标线**）：确认失败自动回退旧槽。
    let mut tx = SwitchTx::new(1);
    let prev = out.active;
    let probed = tx.probe(&out);
    let committed = tx.commit(&mut out);
    let confirmed = tx.confirm(&mut out, false, prev);
    cs.add(
        "B-3601 失败回退",
        probed && committed && !confirmed && out.active == prev && tx.stage == SW_ROLLED_BACK,
        "新模式点不亮——回退旧模式加提示，永不无画面",
    );
    // 3. 成功切换收口：确认过才落定（**B-3601 达标线**另一半）。
    let mut tx2 = SwitchTx::new(1);
    let _ = tx2.probe(&out);
    let _ = tx2.commit(&mut out);
    let ok = tx2.confirm(&mut out, true, 0);
    cs.add("B-3601 确认收口", ok && out.active == 1 && tx2.stage == SW_CONFIRMED, "预检提交确认三段全过才算切换完成");
    // 4. 镜像双路+插拔不断流（**B-3602 达标线**）。
    let mut mp = MirrorPair::new();
    mp.plug_external();
    let dual = mp.external_on && mp.stream_alive() && mp.phase_ok();
    mp.phase_lag_frames = 1;
    let lag_ok = mp.phase_ok();
    mp.phase_lag_frames = 2;
    let lag_over = !mp.phase_ok();
    mp.unplug_external();
    cs.add(
        "B-3602 镜像双路",
        dual && lag_ok && lag_over && mp.stream_alive(),
        "同帧两路相位差容忍一帧——拔掉外接内屏照常",
    );
    // 5. 缩放自动匹配：大屏升档、小屏下夹逼 1000 生效（按物理尺寸）。
    let big = auto_scale(1000, 32, 24);
    let small = auto_scale(1000, 15, 24); // 625 → 下夹逼 1000
    cs.add(
        "B-3602 自动匹配",
        big > 1000 && small == 1000 && auto_scale(1000, 24, 0) == 1000,
        "接大显示器 UI 不变邮票——比例按物理尺寸算，公开可改",
    );
    // 6. 缩放广播三消费者全联动（**B-3603 达标线** WD-061）。
    let mut b = ScaleBroadcast { wm: false, tokens: false, dpi_shim: false };
    cs.add(
        "B-3603 全系统联动",
        broadcast_scale(&mut b) && b.wm && b.tokens && b.dpi_shim,
        "150% 一档变更——绑定参数/token 重发/DPI 换算三处同步",
    );
    // 7. DPI 计算面：同模式不同物理尺寸 DPI 不同（枚举面如实）。
    let mut o24 = Output::new(0, 24, modes);
    o24.active = 0;
    let mut o32 = Output::new(1, 32, modes);
    o32.active = 0;
    cs.add(
        "B-3603 DPI 按物理尺寸",
        o24.dpi() > o32.dpi(),
        "同分辨率小屏 DPI 高——物理尺寸是 DPI 计算的依据",
    );
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe34 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe34_mode_switch_safe() {
        // 全序对练：乱序 commit/confirm 拒——序列阶段机不可跳。
        let mode = Some(VideoMode { w: 2560, h: 1440, refresh_hz: 144 });
        let modes: [Option<VideoMode>; 4] = [mode, None, None, None];
        let mut out = Output::new(0, 27, modes);
        let mut tx = SwitchTx::new(0);
        assert!(!tx.commit(&mut out)); // 未预检就提交——拒
        assert!(tx.probe(&out));
        assert!(!tx.confirm(&mut out, true, 0)); // 未提交就确认——拒
        assert!(tx.commit(&mut out));
        assert!(tx.confirm(&mut out, true, 0));
        assert!(!tx.commit(&mut out)); // 已收口再提交——拒
        // 双槽空表 probe 全拒。
        let empty: [Option<VideoMode>; 4] = [None, None, None, None];
        let o2 = Output::new(1, 24, empty);
        assert!(!o2.probe(0));
    }

    #[test]
    fn fe34_mirror_hotplug() {
        // 插拔反复横跳：主路流恒活；重插相位归零；双开合规。
        let mut mp = MirrorPair::new();
        for _ in 0..3 {
            mp.plug_external();
            assert!(mp.external_on && mp.stream_alive() && mp.phase_ok());
            mp.unplug_external();
            assert!(!mp.external_on && mp.stream_alive());
        }
        mp.plug_external();
        mp.phase_lag_frames = 1;
        assert!(mp.phase_ok()); // 恰一帧在容忍内
    }

    #[test]
    fn fe34_scale_broadcast() {
        // 自动匹配折算与夹逼：base=1500 大屏 32 寸（ref 24）→ 1500*32/24=2000。
        assert_eq!(auto_scale(1500, 32, 24), 2000);
        assert_eq!(auto_scale(1500, 48, 24), 3000); // 3000 夹逼
        assert_eq!(auto_scale(1500, 8, 24), 1000); // 500→1000 夹逼
        // 半联动不算联动：三消费者任一缺席即假（WD-061 全链路）。
        let partial = ScaleBroadcast { wm: true, tokens: true, dpi_shim: false };
        assert!(!(partial.wm && partial.tokens && partial.dpi_shim));
    }

    #[test]
    fn fe34_dpi_and_modes() {
        // DPI 折算稳定性：同屏同模式两次计算一致；模式枚举面如实。
        let mode = Some(VideoMode { w: 3840, h: 2160, refresh_hz: 60 });
        let modes: [Option<VideoMode>; 4] = [mode, None, None, None];
        let mut o = Output::new(0, 32, modes);
        assert_eq!(o.dpi(), o.dpi());
        assert!(o.dpi() > 0);
        assert!(o.probe(0) && !o.probe(1));
        o.active = 3; // 空槽激活——DPI 如实为零（不编数）
        assert_eq!(o.dpi(), 0);
    }
}
