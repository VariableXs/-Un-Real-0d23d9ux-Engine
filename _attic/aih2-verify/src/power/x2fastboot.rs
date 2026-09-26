//! UNREAL-X AI-02 · 族0014 快速启动（X00326~X00350）。
//!
//! 预检缓存 + 跳过阶段：启动前把各阶段的校验结果缓存为「预检凭据」，
//! 下次启动凭据仍新鲜时直接跳过该阶段；配置变更或凭据过期即失效回退全量。
//! 纯逻辑 + 固定数组；非法输入钳制回默认，绝不 panic。

use crate::checks::CheckSet;

/// 可跳过阶段目录（固定 6 阶段）。
pub const STAGE_COUNT: usize = 6;

/// 预检凭据最长新鲜启动次数（超过即整份缓存失效）。
pub const CACHE_FRESH_BOOTS: u32 = 8;

/// 阶段固定下标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FastStage {
    FirmwarePost = 0,
    Memtest = 1,
    NetInit = 2,
    DeviceScan = 3,
    LogoWarm = 4,
    ServicePrewarm = 5,
}

impl FastStage {
    pub fn index(self) -> usize {
        self as usize
    }

    /// 全量执行基线耗时（ms，用于收益估算）。
    pub fn baseline_ms(self) -> u32 {
        match self {
            FastStage::FirmwarePost => 180,
            FastStage::Memtest => 900,
            FastStage::NetInit => 220,
            FastStage::DeviceScan => 350,
            FastStage::LogoWarm => 120,
            FastStage::ServicePrewarm => 260,
        }
    }

    /// 阶段短名（日志/微文案）。
    pub fn name(self) -> &'static str {
        match self {
            FastStage::FirmwarePost => "fw-post",
            FastStage::Memtest => "memtest",
            FastStage::NetInit => "net",
            FastStage::DeviceScan => "devscan",
            FastStage::LogoWarm => "logo",
            FastStage::ServicePrewarm => "prewarm",
        }
    }

    pub const ALL: [FastStage; STAGE_COUNT] = [
        FastStage::FirmwarePost,
        FastStage::Memtest,
        FastStage::NetInit,
        FastStage::DeviceScan,
        FastStage::LogoWarm,
        FastStage::ServicePrewarm,
    ];
}

/// 快速启动模式：≥5 档独立可交付（off = 现状）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FastMode {
    /// 关闭：与现状一致，全量执行。
    Off,
    /// 保守：只跳过 memtest/logo 两类纯校验阶段。
    Cautious,
    /// 均衡：默认档，跳过所有凭据新鲜的阶段。
    Balanced,
    /// 激进：新鲜期放宽到 2 倍（收益大、风险也大）。
    Aggressive,
    /// 静默：跳过一切可跳阶段且不打日志行。
    Silent,
}

impl FastMode {
    pub fn from_id(id: &str) -> FastMode {
        match id {
            "off" => FastMode::Off,
            "cautious" => FastMode::Cautious,
            "balanced" => FastMode::Balanced,
            "aggressive" => FastMode::Aggressive,
            "silent" => FastMode::Silent,
            _ => FastMode::Off,
        }
    }

    /// 凭据新鲜启动次数上限（档位 × CACHE_FRESH_BOOTS，off 档恒 0）。
    pub fn fresh_boots(self) -> u32 {
        match self {
            FastMode::Off => 0,
            FastMode::Cautious => 4,
            FastMode::Balanced => CACHE_FRESH_BOOTS,
            FastMode::Aggressive => CACHE_FRESH_BOOTS * 2,
            FastMode::Silent => CACHE_FRESH_BOOTS,
        }
    }
}

/// 预检凭据：某阶段上次校验通过的记录（校验和 + 剩余新鲜次数）。
#[derive(Clone, Copy, Debug)]
pub struct StageCredential {
    /// 配置指纹（配置变更即失效）。
    pub config_hash: u32,
    /// 剩余新鲜启动次数（0 = 过期）。
    pub fresh_left: u32,
}

/// 快速启动控制器。
#[derive(Clone, Copy, Debug)]
pub struct FastBoot {
    pub mode: FastMode,
    creds: [Option<StageCredential>; STAGE_COUNT],
    /// 当前会话的配置指纹。
    pub config_hash: u32,
    /// 本次启动实际跳过的阶段掩码（bit i = FastStage i）。
    pub skipped_mask: u32,
    /// 护栏：被钳制/拒绝的非法操作次数。
    pub clamped: u32,
    /// 低电量降级守护：true 时一律全量执行。
    pub low_battery: bool,
}

impl FastBoot {
    pub const fn new(mode: FastMode, config_hash: u32) -> FastBoot {
        FastBoot {
            mode,
            creds: [None; STAGE_COUNT],
            config_hash,
            skipped_mask: 0,
            clamped: 0,
            low_battery: false,
        }
    }

    /// 登记一次阶段预检通过（写入凭据，fresh_left = 档位上限）。
    pub fn register_pass(&mut self, stage: FastStage) {
        if self.mode == FastMode::Off {
            self.clamped += 1;
            return;
        }
        self.creds[stage.index()] = Some(StageCredential {
            config_hash: self.config_hash,
            fresh_left: self.mode.fresh_boots(),
        });
    }

    /// 启动前决策：凭据新鲜且同指纹 → 跳过；否则全量。
    pub fn decide(&mut self, stage: FastStage) -> bool {
        if self.mode == FastMode::Off || self.low_battery {
            return false;
        }
        if self.mode == FastMode::Cautious
            && !matches!(stage, FastStage::Memtest | FastStage::LogoWarm)
        {
            return false;
        }
        let c = self.creds[stage.index()];
        match c {
            Some(c) if c.config_hash == self.config_hash && c.fresh_left > 0 => {
                self.skipped_mask |= 1 << stage.index();
                true
            }
            _ => false,
        }
    }

    /// 一次完整启动结束后：所有凭据新鲜度 -1（饱和减法）。
    pub fn advance_boot(&mut self) {
        for slot in self.creds.iter_mut() {
            if let Some(c) = slot {
                c.fresh_left = c.fresh_left.saturating_sub(1);
            }
        }
    }

    /// 配置变更：指纹不符的凭据全部失效（缓存失效通道）。
    pub fn on_config_change(&mut self, new_hash: u32) {
        self.config_hash = new_hash;
        for slot in self.creds.iter_mut() {
            if let Some(c) = slot {
                if c.config_hash != new_hash {
                    *slot = None;
                }
            }
        }
    }

    /// 预计节省毫秒（本会话已跳过阶段收益合计）。
    pub fn saved_ms(&self) -> u32 {
        let mut acc = 0u32;
        for s in FastStage::ALL {
            if self.skipped_mask & (1 << s.index()) != 0 {
                acc = acc.saturating_add(s.baseline_ms());
            }
        }
        acc
    }

    /// 净身：清空缓存回到出厂（off = 现状）。
    pub fn reset(&mut self) {
        self.creds = [None; STAGE_COUNT];
        self.skipped_mask = 0;
        self.clamped = 0;
        self.low_battery = false;
    }
}

/// 族0014 域自检。
pub fn run_fastboot_checks() -> CheckSet {
    let mut set = CheckSet::new("power.x2fastboot");
    let mut f = FastBoot::new(FastMode::Balanced, 7);
    set.add("mode default skips nothing", !f.decide(FastStage::Memtest), "");
    f.register_pass(FastStage::Memtest);
    set.add("fresh credential skips", f.decide(FastStage::Memtest), "");
    set.add("saved ms sums baseline", f.saved_ms() == FastStage::Memtest.baseline_ms(), "");
    set.add("advance expires credentials", {
        f.advance_boot();
        f.advance_boot();
        let mut g = FastBoot::new(FastMode::Balanced, 7);
        g.register_pass(FastStage::Memtest);
        let mut n = 0;
        while n < CACHE_FRESH_BOOTS {
            g.advance_boot();
            n += 1;
        }
        g.creds[FastStage::Memtest.index()].unwrap().fresh_left == 0
    }, "");
    set.add("off mode rejects registration", {
        let mut o = FastBoot::new(FastMode::Off, 1);
        o.register_pass(FastStage::Memtest);
        o.clamped == 1 && o.decide(FastStage::Memtest) == false
    }, "");
    set.add("config change invalidates", {
        f.on_config_change(9);
        f.decide(FastStage::Memtest) == false
    }, "");
    set.add("low battery forces full run", {
        let mut h = FastBoot::new(FastMode::Balanced, 3);
        h.register_pass(FastStage::NetInit);
        h.low_battery = true;
        h.decide(FastStage::NetInit) == false
    }, "");
    set.add("cautious only skips memtest/logo", {
        let mut c = FastBoot::new(FastMode::Cautious, 2);
        c.register_pass(FastStage::DeviceScan);
        c.register_pass(FastStage::Memtest);
        !c.decide(FastStage::DeviceScan) && c.decide(FastStage::Memtest)
    }, "");
    set.add("mode lookup unknown falls to off", FastMode::from_id("nope") == FastMode::Off, "");
    set.add("silent mode budget intact", FastMode::Silent.fresh_boots() == CACHE_FRESH_BOOTS, "");
    set.add("reset wipes cache", {
        f.reset();
        f.skipped_mask == 0 && f.clamped == 0
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x00326_min_loop_credential_skips_stage() {
        let mut f = FastBoot::new(FastMode::Balanced, 42);
        assert!(!f.decide(FastStage::DeviceScan));
        f.register_pass(FastStage::DeviceScan);
        assert!(f.decide(FastStage::DeviceScan));
        assert_eq!(f.skipped_mask, 1 << FastStage::DeviceScan.index());
    }

    #[test]
    fn x00331_invalid_inputs_clamped_not_panic() {
        let mut o = FastBoot::new(FastMode::Off, 0);
        o.register_pass(FastStage::LogoWarm);
        assert_eq!(o.clamped, 1);
        // 过期/异指纹凭据一律不跳过
        let mut f = FastBoot::new(FastMode::Balanced, 1);
        f.register_pass(FastStage::NetInit);
        f.config_hash = 2;
        assert!(!f.decide(FastStage::NetInit));
    }

    #[test]
    fn x00329_snapshot_channel_expiry() {
        let mut f = FastBoot::new(FastMode::Aggressive, 5);
        f.register_pass(FastStage::Memtest);
        for _ in 0..(CACHE_FRESH_BOOTS * 2) {
            f.advance_boot();
        }
        assert_eq!(f.creds[FastStage::Memtest.index()].unwrap().fresh_left, 0);
        assert!(!f.decide(FastStage::Memtest));
    }

    #[test]
    fn x00335_reset_clean() {
        let mut f = FastBoot::new(FastMode::Silent, 3);
        f.register_pass(FastStage::LogoWarm);
        f.reset();
        assert!(f.creds.iter().all(|s| s.is_none()));
    }

    #[test]
    fn x00326_run_checks_pass() {
        assert!(run_fastboot_checks().all_passed());
    }
}
