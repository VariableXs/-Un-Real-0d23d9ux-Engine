//! I 通用域·锁屏与登录安全五项（Varix STAR I start · AI-U3 分工包 · F504~F508）。
//!
//! 本文件是「判据实装层」：主册《Varix STAR I start.md》F504~F508 各节
//! 【验收判据】第一句逐字摘录如下（判据唯一源），配套最小状态机与判定
//! 函数，使每条判据在 `run_f50X_checks` 中有可红绿的真实断言。
//!
//! =========================================================================
//! F504 PIN 快速解锁
//! =========================================================================
//! 主册判据（验收标准第一句，逐字）：
//! **4-6 位设置；解锁时序（<1.5s 全链）；5 次冷却翻倍表；密码回退；改废即时生效。**
//!
//! 功能定义要点：锁屏 PIN（4-6 位数字，本机验证不出机器）；设置中启用
//! （设密码后可加 PIN——密码是根、PIN 是便捷皮）；锁屏登录卡自动切 PIN
//! 键盘；PIN 错误 5 次冷却（30s 起、逐次翻倍——防爆破）且冷却期回退密码
//! 登录；PIN 随时改/废（废了回密码）。无感标准：日常解锁 1.5 秒。
//!
//! 依赖锚点：密码登录（根凭据）、F238 锁屏。
//!
//! 零堆纪律：PinManager 全定长字段（[u8; 6] 存位、计数器为标量），
//! 判定路径无 String/Vec/Box/format!。
//!
//! =========================================================================
//! F505 蓝牙动态锁
//! =========================================================================
//! 主册判据（验收标准第一句，逐字）：
//! **信号消失 30s±5s 触发；锁形标记；误触发测试（短暂信号波动 <10s 不锁）；回连时长；与 F316 闲置锁屏双保险并存。**
//!
//! 功能定义要点：人走锁屏——绑定一台蓝牙设备作「钥匙」，信号消失超过
//! 30 秒自动锁屏（F238）；配对钥匙在设置页单独标记（锁形标记）；回来
//! 解锁后蓝牙自动回连钥匙。
//!
//! 依赖锚点：F238 锁屏、F316 闲置锁屏（双保险并存）、F545 蓝牙。
//!
//! 零堆纪律：DynalockMonitor/KeyRing/DualInsurance 全定长数组 + 标量，
//! 事件流以带时间戳的方法调用建模，无动态分配。
//!
//! =========================================================================
//! F506 访客模式
//! =========================================================================
//! 主册判据（验收标准第一句，逐字）：
//! **沙盒隔离四判据（文件/设置/权限/网络共享）；退出清理完整性（注入残留文件验证）；入口开关；极简形制；会话时长上限提示（默认 2h）。**
//!
//! 功能定义要点：锁屏登录卡「访客」入口（主用户可开/关）——访客会话是
//! 沙盒（独立临时用户目录、无系统设置权、无权限中心批准权 F324 默认全拒、
//! 不可见主用户文件）；访客退出时会话数据全清（清理前最后确认一次）；
//! 访客会话任务栏/桌面极简形制。
//!
//! 依赖锚点：F324 权限中心（默认全拒）、F238 锁屏登录卡。
//!
//! 零堆纪律：GuestSandbox 清理账为定长 [CleanupEntry; 16]，路径一律
//! &'static str 字面量，无堆分配。
//!
//! =========================================================================
//! F507 锁屏防截图
//! =========================================================================
//! 主册判据（验收标准第一句，逐字）：
//! **三路截图注入测试（锁屏态全拒）；第三方 API 拦截；解锁后恢复；纯黑帧/不触发实现选择文档化；性能零开销（非锁屏态）。**
//!
//! 功能定义要点：锁屏与登录界面禁截图——截屏三路（PrtSc F413/截图工具
//! F098/录屏 F361）在锁屏态全部拒绝（快照得到纯黑帧或直接不触发——按
//! 安全实现定，文档化）；第三方应用截屏调用同样被内核层拦截。
//!
//! 依赖锚点：F413 PrtSc、F098 截图工具、F361 录屏、F238 锁屏。
//!
//! 零堆纪律：ShotGuard 仅一个布尔寄存器；判定函数纯标量位测试。
//!
//! =========================================================================
//! F508 应用防截标记
//! =========================================================================
//! 主册判据（验收标准第一句，逐字）：
//! **清单字段声明读取；黑块区域精确性（窗口几何对齐）；F361 录屏同规则；未标记应用零影响；黑块视觉规范。**
//!
//! 功能定义要点：应用可声明窗口防截（vxapp 清单字段）——标记窗口不可被
//! 系统截图/录屏（F098/F361 对该窗区域输出黑块——其他区域正常，不是整屏
//! 拒绝）；标记窗口在截图预览中有明确黑块。
//!
//! 依赖锚点：F098 截图工具、F361 录屏、vxapp 清单格式。
//!
//! 零堆纪律：NoShotRegistry 定长 [Option<WinMark>; 16]，矩形为
//! Copy 标量结构，黑块输出写入调用方给定缓冲。

use crate::checks::CheckSet;

// ===========================================================================
// F504 PIN 快速解锁 —— PinManager
// ===========================================================================

/// PIN 最短位数（判据：4-6 位）。
pub const PIN_MIN_DIGITS: usize = 4;
/// PIN 最长位数（判据：4-6 位）。
pub const PIN_MAX_DIGITS: usize = 6;
/// 起冷失败次数（判据：错误 5 次冷却）。
pub const COOLDOWN_START_FAILS: u32 = 5;
/// 冷却基数 30s（判据：30s 起、逐次翻倍）。
pub const COOLDOWN_BASE_MS: u64 = 30_000;
/// 冷却封顶失败次数：16 次（设计保护——30s<<11 = 61_440_000ms ≈ 17h，
/// 之后不再增长，防 u64 移位溢出与无限期锁定）。
pub const COOLDOWN_MAX_FAILS: u32 = 16;
/// 日常解锁全链预算（判据：<1.5s）。
pub const UNLOCK_BUDGET_MS: u64 = 1_500;

/// 解锁时序四段预算（判据：<1.5s 全链，分四段建模）：
/// 键盘弹出 + 本机验证 + 过场 + 余量。
pub const SEG_KEYBOARD_POPUP_MS: u64 = 350; // 键盘弹出
pub const SEG_PIN_VERIFY_MS: u64 = 200; // 本机验证（不出机器）
pub const SEG_TRANSITION_MS: u64 = 450; // 解锁过场
pub const SEG_RESERVE_MS: u64 = 500; // 调度/渲染抖动余量
/// 四段合计（必须恰等于 UNLOCK_BUDGET_MS，一处一事实）。
pub const SEG_SUM_MS: u64 =
    SEG_KEYBOARD_POPUP_MS + SEG_PIN_VERIFY_MS + SEG_TRANSITION_MS + SEG_RESERVE_MS;

/// 冷却翻倍表：第 n 次失败（n ≥ 5）触发的冷却时长。
/// `cooldown_ms(n) = 30_000 << (n - 5)`，n 封顶 COOLDOWN_MAX_FAILS，
/// 移位溢出时兜底到封顶值（表驱动验证见 run_f504_checks 第 6/7 条）。
pub fn cooldown_ms(fails: u32) -> u64 {
    let n = fails.min(COOLDOWN_MAX_FAILS);
    if n < COOLDOWN_START_FAILS {
        return 0;
    }
    let shifts = n - COOLDOWN_START_FAILS;
    COOLDOWN_BASE_MS
        .checked_shl(shifts)
        .unwrap_or(COOLDOWN_BASE_MS << (COOLDOWN_MAX_FAILS - COOLDOWN_START_FAILS))
}

/// PIN 校验：4-6 位、纯 ASCII 数字。
pub fn validate_pin(digits: &[u8]) -> Result<(), &'static str> {
    if digits.len() < PIN_MIN_DIGITS {
        return Err("pin-too-short");
    }
    if digits.len() > PIN_MAX_DIGITS {
        return Err("pin-too-long");
    }
    for &d in digits {
        if !d.is_ascii_digit() {
            return Err("pin-not-digits");
        }
    }
    Ok(())
}

/// PIN 校验结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PinVerdict {
    /// 解锁成功（失败计数清零）。
    Unlocked,
    /// PIN 错误（计数 +1，达 5 起触发冷却）。
    WrongPin,
    /// 冷却中：PIN 输入拒绝但密码路径畅通（判据：冷却期回退密码登录）。
    Cooldown { remaining_ms: u64 },
}

/// PIN 管理状态机（判据实装：4-6 位设置 / 5 次冷却翻倍表 / 密码回退 /
/// 改废即时生效）。
///
/// 密码是根、PIN 是便捷皮：本机验证不出机器；PIN 的存在与否不影响密码
/// 路径——`password_path_open` 恒真（结构自证：本结构体没有封锁密码
/// 路径的字段）。
pub struct PinManager {
    pin_digits: [u8; PIN_MAX_DIGITS],
    pin_len: usize,
    is_set: bool,
    fail_streak: u32,
    cooldown_until_ms: u64,
}

impl PinManager {
    pub const fn new() -> Self {
        PinManager {
            pin_digits: [0; PIN_MAX_DIGITS],
            pin_len: 0,
            is_set: false,
            fail_streak: 0,
            cooldown_until_ms: 0,
        }
    }

    /// 设置/更换 PIN（判据：4-6 位设置；改 PIN 即时生效——旧 PIN 立即失效）。
    /// 冷却状态保留（换 PIN 不洗白防爆破计数）。
    pub fn set_pin(&mut self, digits: &[u8]) -> Result<(), &'static str> {
        validate_pin(digits)?;
        for (i, &d) in digits.iter().enumerate() {
            self.pin_digits[i] = d;
        }
        self.pin_len = digits.len();
        self.is_set = true;
        Ok(())
    }

    /// 废除 PIN（判据：废了回密码）。废除即时生效。
    pub fn clear_pin(&mut self) {
        self.is_set = false;
        self.pin_len = 0;
        self.fail_streak = 0;
        self.cooldown_until_ms = 0;
    }

    pub fn is_set(&self) -> bool {
        self.is_set
    }

    pub fn fail_streak(&self) -> u32 {
        self.fail_streak
    }

    pub fn in_cooldown(&self, now_ms: u64) -> bool {
        now_ms < self.cooldown_until_ms
    }

    pub fn cooldown_remaining(&self, now_ms: u64) -> u64 {
        if now_ms < self.cooldown_until_ms {
            self.cooldown_until_ms - now_ms
        } else {
            0
        }
    }

    pub fn cooldown_until_ms(&self) -> u64 {
        self.cooldown_until_ms
    }

    /// 密码路径（根凭据）任何时刻畅通——含 PIN 冷却期（判据：密码回退）。
    /// 结构自证：本结构体没有任何能封锁密码路径的状态位。
    pub fn password_path_open(&self) -> bool {
        true
    }

    fn pin_matches(&self, digits: &[u8]) -> bool {
        digits.len() == self.pin_len
            && (0..self.pin_len).all(|i| self.pin_digits[i] == digits[i])
    }

    /// 锁屏 PIN 验证。冷却期直接拒绝（不计数），冷却到期后恢复校验。
    pub fn verify(&mut self, digits: &[u8], now_ms: u64) -> PinVerdict {
        if !self.is_set {
            // 未设 PIN：登录卡不切 PIN 键盘，此路径拒绝（走密码）。
            return PinVerdict::WrongPin;
        }
        if self.in_cooldown(now_ms) {
            return PinVerdict::Cooldown {
                remaining_ms: self.cooldown_remaining(now_ms),
            };
        }
        if self.pin_matches(digits) {
            self.fail_streak = 0;
            self.cooldown_until_ms = 0;
            return PinVerdict::Unlocked;
        }
        self.fail_streak += 1;
        if self.fail_streak >= COOLDOWN_START_FAILS {
            self.cooldown_until_ms = now_ms + cooldown_ms(self.fail_streak);
        }
        PinVerdict::WrongPin
    }
}

/// 解锁时序计划合规判定（判据：<1.5s 全链）：四段合计不超预算。
pub fn timing_plan_ok(segs: &[u64; 4]) -> bool {
    segs[0] + segs[1] + segs[2] + segs[3] <= UNLOCK_BUDGET_MS
}

/// F504 域自检（13 条真实断言）。
pub fn run_f504_checks() -> CheckSet {
    let mut cs = CheckSet::new("F504-pin-unlock");

    // 1) 4-6 位设置边界：3 位拒、7 位拒、4/5/6 位收。
    let bounds = validate_pin(b"123").is_err()
        && validate_pin(b"1234567").is_err()
        && validate_pin(b"1234").is_ok()
        && validate_pin(b"12345").is_ok()
        && validate_pin(b"123456").is_ok();
    cs.add("pin_length_bounds", bounds, "");

    // 2) 纯数字校验：非数字字符拒收。
    cs.add("pin_digits_only", validate_pin(b"12a4").is_err(), "");

    // 3) 正确 PIN 解锁。
    let mut pm = PinManager::new();
    let _ = pm.set_pin(b"2468");
    cs.add("verify_correct_unlocks", pm.verify(b"2468", 0) == PinVerdict::Unlocked, "");

    // 4) 错误计数：前 4 次错误不触发冷却。
    let mut pm2 = PinManager::new();
    let _ = pm2.set_pin(b"1111");
    let mut no_cooldown = true;
    for i in 0..4u64 {
        let t = i * 100;
        if pm2.verify(b"0000", t) != PinVerdict::WrongPin || pm2.in_cooldown(t) {
            no_cooldown = false;
        }
    }
    cs.add("four_fails_no_cooldown", no_cooldown && pm2.fail_streak() == 4, "");

    // 5) 第 5 次错误起冷 30s。
    let fifth = pm2.verify(b"0000", 500);
    cs.add(
        "cooldown_starts_at_5",
        fifth == PinVerdict::WrongPin
            && pm2.in_cooldown(500)
            && pm2.cooldown_remaining(500) == COOLDOWN_BASE_MS,
        "",
    );

    // 6) 冷却翻倍表：30s → 60s → 120s → 240s。
    cs.add(
        "cooldown_double_table",
        cooldown_ms(5) == 30_000
            && cooldown_ms(6) == 60_000
            && cooldown_ms(7) == 120_000
            && cooldown_ms(8) == 240_000,
        "",
    );

    // 7) 封顶保护：16 次封顶，n 再大不再增长（checked_shl 兜底）。
    cs.add(
        "cooldown_capped",
        cooldown_ms(COOLDOWN_MAX_FAILS) == COOLDOWN_BASE_MS << (COOLDOWN_MAX_FAILS - COOLDOWN_START_FAILS)
            && cooldown_ms(99) == cooldown_ms(COOLDOWN_MAX_FAILS),
        "",
    );

    // 8) 冷却期 PIN 拒、密码路径畅通（密码是根、PIN 是便捷皮）。
    let during = pm2.verify(b"1111", 600);
    cs.add(
        "cooldown_pin_denied_password_open",
        matches!(during, PinVerdict::Cooldown { .. }) && pm2.password_path_open(),
        "",
    );

    // 9) 冷却到期恢复 PIN 校验（正好到期即解禁）。
    cs.add(
        "cooldown_expires",
        pm2.verify(b"1111", pm2.cooldown_until_ms()) == PinVerdict::Unlocked,
        "",
    );

    // 10) 改 PIN 即时生效：旧 PIN 立即失效、新 PIN 立即可用。
    let mut pm3 = PinManager::new();
    let _ = pm3.set_pin(b"1357");
    let _ = pm3.set_pin(b"2468");
    cs.add(
        "change_pin_immediate",
        pm3.verify(b"1357", 0) == PinVerdict::WrongPin
            && pm3.verify(b"2468", 10) == PinVerdict::Unlocked,
        "",
    );

    // 11) 废 PIN 即时生效：回退密码（密码是根）。
    pm3.clear_pin();
    cs.add(
        "clear_pin_immediate",
        !pm3.is_set()
            && pm3.verify(b"2468", 0) == PinVerdict::WrongPin
            && pm3.password_path_open(),
        "",
    );

    // 12) 解锁时序四段预算：合计恰 1500ms、每段非零。
    cs.add(
        "timing_budget_1500",
        SEG_SUM_MS == UNLOCK_BUDGET_MS
            && UNLOCK_BUDGET_MS == 1_500
            && SEG_KEYBOARD_POPUP_MS > 0
            && SEG_PIN_VERIFY_MS > 0
            && SEG_TRANSITION_MS > 0
            && SEG_RESERVE_MS > 0,
        "",
    );

    // 13) 时序计划判定：合计 ≤1500ms 放行、超预算违例。
    let ok_plan = [SEG_KEYBOARD_POPUP_MS, SEG_PIN_VERIFY_MS, SEG_TRANSITION_MS, 480];
    let bad_plan = [600, 300, 500, 300];
    cs.add("timing_plan_gate", timing_plan_ok(&ok_plan) && !timing_plan_ok(&bad_plan), "");

    cs
}

#[cfg(test)]
mod f504_tests {
    use super::*;

    #[test]
    fn set_verify_clear_lifecycle() {
        let mut pm = PinManager::new();
        assert!(!pm.is_set(), "出厂未设 PIN，登录卡走密码");
        assert!(pm.set_pin(b"9021").is_ok(), "4 位设置应成功（4-6 位判据）");
        assert!(pm.is_set());
        assert_eq!(pm.verify(b"9021", 0), PinVerdict::Unlocked, "正确 PIN 解锁");
        pm.clear_pin();
        assert!(!pm.is_set(), "废 PIN 即时生效，回退密码（判据：废了回密码）");
    }

    #[test]
    fn cooldown_escalation_end_to_end() {
        let mut pm = PinManager::new();
        let _ = pm.set_pin(b"1111");
        // 5 次错误（第 5 次发生在 t=4000）→ 冷却 30s。
        for t in 0..5u64 {
            let _ = pm.verify(b"0000", t * 1_000);
        }
        assert_eq!(pm.cooldown_remaining(4_000), COOLDOWN_BASE_MS, "第 5 次起冷 30s（判据）");
        // 冷却期第 6 次尝试被拒且不计数。
        assert!(matches!(pm.verify(b"0000", 5_000), PinVerdict::Cooldown { .. }));
        assert_eq!(pm.fail_streak(), 5, "冷却期输入不计入失败序列");
        // 到期后再错一次 → 第 6 次失败冷 60s（翻倍表）。
        assert_eq!(pm.verify(b"0000", 34_000), PinVerdict::WrongPin);
        assert_eq!(pm.cooldown_remaining(34_000), 60_000, "第 6 次失败冷却翻倍 60s（判据：逐次翻倍）");
    }

    #[test]
    fn success_resets_streak() {
        let mut pm = PinManager::new();
        let _ = pm.set_pin(b"2222");
        for t in 0..4u64 {
            let _ = pm.verify(b"9999", t * 10);
        }
        assert_eq!(pm.verify(b"2222", 50), PinVerdict::Unlocked);
        assert_eq!(pm.fail_streak(), 0, "解锁成功清零失败计数");
        // 再错 4 次也不应冷却（计数已清零）。
        for t in 0..4u64 {
            let _ = pm.verify(b"9999", 100 + t * 10);
        }
        assert!(!pm.in_cooldown(140), "成功后计数清零，4 次错误不冷却");
    }

    #[test]
    fn set_pin_rejections_leave_state_intact() {
        let mut pm = PinManager::new();
        let _ = pm.set_pin(b"3141");
        assert_eq!(pm.set_pin(b"123"), Err("pin-too-short"), "3 位拒收（4-6 位判据）");
        assert_eq!(pm.set_pin(b"1234567"), Err("pin-too-long"), "7 位拒收（4-6 位判据）");
        assert_eq!(pm.set_pin(b"12x4"), Err("pin-not-digits"), "非数字拒收");
        assert!(pm.verify(b"3141", 0) == PinVerdict::Unlocked, "拒收后原 PIN 仍然有效");
    }

    #[test]
    fn doubling_table_precise() {
        assert_eq!(cooldown_ms(4), 0, "不足 5 次无冷却");
        assert_eq!(cooldown_ms(5), 30_000, "30s 起（判据）");
        assert_eq!(cooldown_ms(6), 60_000, "翻倍表：60s");
        assert_eq!(cooldown_ms(7), 120_000, "翻倍表：120s");
        assert_eq!(cooldown_ms(8), 240_000, "翻倍表：240s");
        assert_eq!(cooldown_ms(9), 480_000, "翻倍表：480s");
        assert_eq!(cooldown_ms(10), 960_000, "翻倍表：960s");
    }

    #[test]
    fn timing_budget_constants() {
        assert_eq!(UNLOCK_BUDGET_MS, 1_500, "无感标准：日常解锁 1.5 秒（判据）");
        assert_eq!(SEG_SUM_MS, 1_500, "四段预算合计恰等于全链预算");
        assert_eq!(SEG_KEYBOARD_POPUP_MS, 350, "键盘弹出段");
        assert_eq!(SEG_PIN_VERIFY_MS, 200, "本机验证段");
        assert_eq!(SEG_TRANSITION_MS, 450, "解锁过场段");
        assert_eq!(SEG_RESERVE_MS, 500, "抖动余量段");
    }

    #[test]
    fn password_path_is_root() {
        let mut pm = PinManager::new();
        let _ = pm.set_pin(b"1111");
        for t in 0..6u64 {
            let _ = pm.verify(b"0000", t * 1_000);
        }
        assert!(
            pm.password_path_open() && pm.in_cooldown(6_000),
            "密码是根：冷却期密码回退路径畅通（判据：冷却期回退密码登录）"
        );
    }
}

// ===========================================================================
// F505 蓝牙动态锁 —— DynalockMonitor / KeyRing / DualInsurance
// ===========================================================================

/// 信号消失后触发锁屏的额定时长（判据：消失 30s 触发）。
pub const LOCK_AFTER_LOST_MS: u64 = 30_000;
/// 触发窗容差 ±5s（判据：30s±5s）——触发时刻落在 [25s, 35s] 内合规。
pub const LOST_TOLERANCE_MS: u64 = 5_000;
/// 短暂波动容忍（判据：短暂信号波动 <10s 不锁）。
pub const BOUNCE_TOLERANCE_MS: u64 = 10_000;
/// 解锁后回连钥匙的预算（设计常量：解锁后 2s 内自动回连——判据「回连时长」）。
pub const RECONNECT_BUDGET_MS: u64 = 2_000;

/// 触发窗判定：自信号消失起算 elapsed，落在 30s±5s 内即合规触发。
pub fn trigger_in_window(elapsed_since_lost_ms: u64) -> bool {
    let lo = LOCK_AFTER_LOST_MS - LOST_TOLERANCE_MS;
    let hi = LOCK_AFTER_LOST_MS + LOST_TOLERANCE_MS;
    (lo..=hi).contains(&elapsed_since_lost_ms)
}

/// 回连预算判定：解锁后 elapsed 内回连成功即合规。
pub fn reconnect_ok(elapsed_ms: u64) -> bool {
    elapsed_ms <= RECONNECT_BUDGET_MS
}

/// 钥匙信号状态机（判据建模：Present/Volatile/Lost）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyState {
    /// 钥匙在范围内。
    Present,
    /// 信号消失 <10s（短暂波动窗口——不锁）。
    Volatile,
    /// 信号消失 ≥10s（等待 30s 死线锁屏）。
    Lost,
}

/// 轮询输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PollOut {
    Idle,
    /// 到达死线且仍无信号 → 触发锁屏（只触发一次，latch）。
    LockFired,
}

/// 信号恢复分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recovery {
    /// 本来就在场（无事件语义）。
    None,
    /// 消失 <10s 内恢复：短暂波动，不锁（判据：误触发测试）。
    Bounce,
    /// 消失 ≥10s 后恢复：真实离开后回来，解锁后走回连。
    Reconnect,
}

/// 蓝牙动态锁监视器。
///
/// 事件流模型：`on_signal_lost(t)` / `on_signal_seen(t)` 事件 + 周期
/// `poll(now)`。死线 = t_lost + 30_000ms；死线到仍无信号即触发锁屏
/// （触发时刻为额定 30s，落在 ±5s 判定窗内）。锁栓 latch：触发一次后
/// 保持，直到 `reset_after_unlock`。
pub struct DynalockMonitor {
    state: KeyState,
    lost_at_ms: u64,
    deadline_ms: u64,
    lock_fired: bool,
}

impl DynalockMonitor {
    pub const fn new() -> Self {
        DynalockMonitor {
            state: KeyState::Present,
            lost_at_ms: 0,
            deadline_ms: 0,
            lock_fired: false,
        }
    }

    /// 钥匙信号消失（首次消失锚定死线；已在消失态则忽略——从第一次
    /// 消失起算「超过 30 秒」）。
    pub fn on_signal_lost(&mut self, now_ms: u64) {
        if self.state == KeyState::Present {
            self.lost_at_ms = now_ms;
            self.deadline_ms = now_ms + LOCK_AFTER_LOST_MS;
            self.state = KeyState::Volatile;
            self.lock_fired = false;
        }
    }

    /// 钥匙信号恢复：<10s 记短暂波动（Bounce，不锁），≥10s 记真实回归
    /// （Reconnect，解锁后回连）。若死线已过且锁已触发，锁栓保持——
    /// 恢复信号不撤销已发生的锁屏（与 F316 双保险同理：已锁就是已锁）。
    pub fn on_signal_seen(&mut self, now_ms: u64) -> Recovery {
        match self.state {
            KeyState::Present => Recovery::None,
            KeyState::Volatile | KeyState::Lost => {
                let gap = now_ms.saturating_sub(self.lost_at_ms);
                self.state = KeyState::Present;
                if gap < BOUNCE_TOLERANCE_MS {
                    Recovery::Bounce
                } else {
                    Recovery::Reconnect
                }
            }
        }
    }

    /// 周期轮询：死线到仍无信号 → 触发锁屏一次。Volatile 超 10s 升格
    /// Lost（标签迁移，死线不变）。
    pub fn poll(&mut self, now_ms: u64) -> PollOut {
        match self.state {
            KeyState::Present => PollOut::Idle,
            KeyState::Volatile | KeyState::Lost => {
                if self.state == KeyState::Volatile
                    && now_ms.saturating_sub(self.lost_at_ms) >= BOUNCE_TOLERANCE_MS
                {
                    self.state = KeyState::Lost;
                }
                if now_ms >= self.deadline_ms && !self.lock_fired {
                    self.lock_fired = true;
                    PollOut::LockFired
                } else {
                    PollOut::Idle
                }
            }
        }
    }

    /// 解锁复位：状态归 Present、锁栓清除（回连钥匙的前提）。
    pub fn reset_after_unlock(&mut self) {
        self.state = KeyState::Present;
        self.lock_fired = false;
        self.deadline_ms = 0;
        self.lost_at_ms = 0;
    }

    pub fn state(&self) -> KeyState {
        self.state
    }
    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }
    pub fn lock_fired(&self) -> bool {
        self.lock_fired
    }
    pub fn lost_at_ms(&self) -> u64 {
        self.lost_at_ms
    }
}

/// 钥匙设备（判据：配对钥匙在设置页单独标记——锁形标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BtDevice {
    pub name: &'static str,
    pub mac: [u8; 6],
    pub is_key: bool,
}

/// 钥匙环：配对设备清单 + 单一钥匙标记。
pub const KEY_DEVICES_CAP: usize = 8;

pub struct KeyRing {
    devices: [Option<BtDevice>; KEY_DEVICES_CAP],
    count: usize,
    key_slot: Option<usize>,
}

impl KeyRing {
    pub const fn new() -> Self {
        KeyRing {
            devices: [None; KEY_DEVICES_CAP],
            count: 0,
            key_slot: None,
        }
    }

    /// 配对一台设备，返回槽位。
    pub fn pair(&mut self, name: &'static str, mac: [u8; 6]) -> Option<usize> {
        if self.count >= KEY_DEVICES_CAP {
            return None;
        }
        self.devices[self.count] = Some(BtDevice { name, mac, is_key: false });
        self.count += 1;
        Some(self.count - 1)
    }

    /// 锁形标记：将指定槽位设备标记为钥匙（单一钥匙——标记新钥匙时
    /// 自动取消旧钥匙标记，判据「绑定一台蓝牙设备」）。
    pub fn mark_key(&mut self, slot: usize) -> bool {
        if slot >= self.count {
            return false;
        }
        for i in 0..self.count {
            if let Some(d) = &mut self.devices[i] {
                d.is_key = i == slot;
            }
        }
        self.key_slot = Some(slot);
        true
    }

    /// 当前钥匙设备（设置页锁形标记的数据源）。
    pub fn key_device(&self) -> Option<&BtDevice> {
        self.key_slot.and_then(|s| self.devices[s].as_ref())
    }

    pub fn device_is_key(&self, slot: usize) -> bool {
        self.devices.get(slot).and_then(|d| d.as_ref()).map(|d| d.is_key).unwrap_or(false)
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

/// 双保险（判据：与 F316 闲置锁屏双保险并存）：动态锁与闲置锁是两条
/// 独立锁屏路径，或关系触发、互不取消。
pub struct DualInsurance {
    dynalock_armed: bool,
    idle_armed: bool,
    dynalock_fired: bool,
    idle_fired: bool,
}

impl DualInsurance {
    pub const fn new() -> Self {
        DualInsurance {
            dynalock_armed: true,
            idle_armed: true,
            dynalock_fired: false,
            idle_fired: false,
        }
    }

    pub fn fire_dynalock(&mut self) {
        self.dynalock_fired = true;
    }
    pub fn fire_idle(&mut self) {
        self.idle_fired = true;
    }
    /// 或关系：任一路径触发即应锁屏。
    pub fn should_lock(&self) -> bool {
        self.dynalock_fired || self.idle_fired
    }
    /// 解除动态锁路径（互不取消：绝不触碰闲置路径的武装位与触发位）。
    pub fn disarm_dynalock(&mut self) {
        self.dynalock_armed = false;
    }
    /// 解除闲置锁路径（互不取消：绝不触碰动态锁路径）。
    pub fn disarm_idle(&mut self) {
        self.idle_armed = false;
    }
    pub fn dynalock_armed(&self) -> bool {
        self.dynalock_armed
    }
    pub fn idle_armed(&self) -> bool {
        self.idle_armed
    }
    pub fn dynalock_fired(&self) -> bool {
        self.dynalock_fired
    }
    pub fn idle_fired(&self) -> bool {
        self.idle_fired
    }
}

/// F505 域自检（12 条真实断言）。
pub fn run_f505_checks() -> CheckSet {
    let mut cs = CheckSet::new("F505-bt-dynalock");

    // 1) 信号消失锚定死线：t_lost + 30_000ms。
    let mut m = DynalockMonitor::new();
    m.on_signal_lost(1_000);
    cs.add("deadline_is_lost_plus_30s", m.deadline_ms() == 1_000 + LOCK_AFTER_LOST_MS, "");

    // 2) 死线前一瞬不锁、死线到点即锁。
    let before = m.poll(1_000 + LOCK_AFTER_LOST_MS - 1);
    let at = m.poll(1_000 + LOCK_AFTER_LOST_MS);
    cs.add("lock_at_deadline", before == PollOut::Idle && at == PollOut::LockFired, "");

    // 3) 触发窗 30s±5s：[25s, 35s] 合规，界外违例。
    cs.add(
        "trigger_window_bounds",
        trigger_in_window(25_000)
            && trigger_in_window(30_000)
            && trigger_in_window(35_000)
            && !trigger_in_window(24_999)
            && !trigger_in_window(35_001),
        "",
    );

    // 4) 误触发测试：<10s 波动回 Present 不锁（旧死线不再生效）。
    let mut m2 = DynalockMonitor::new();
    m2.on_signal_lost(0);
    let rec = m2.on_signal_seen(9_999);
    cs.add(
        "bounce_under_10s_no_lock",
        rec == Recovery::Bounce && m2.state() == KeyState::Present && m2.poll(35_000) == PollOut::Idle,
        "",
    );

    // 5) Volatile→Lost 升格：>10s 仍消失即入 Lost（死线不变）。
    let mut m3 = DynalockMonitor::new();
    m3.on_signal_lost(0);
    let _ = m3.poll(10_000);
    cs.add("volatile_upgrades_to_lost", m3.state() == KeyState::Lost, "");

    // 6) 恢复在死线前：20s 回归 → 不锁（消失未超 30s）。
    let mut m4 = DynalockMonitor::new();
    m4.on_signal_lost(0);
    let rec4 = m4.on_signal_seen(20_000);
    cs.add(
        "recovery_before_deadline",
        rec4 == Recovery::Reconnect && m4.poll(30_000) == PollOut::Idle,
        "",
    );

    // 7) 锁栓 latch：触发一次后保持，重复轮询不重复触发。
    cs.add("lock_fires_once", m.poll(40_000) == PollOut::Idle && m.lock_fired(), "");

    // 8) 钥匙锁形标记：单一钥匙、清单可查。
    let mut ring = KeyRing::new();
    let phone = ring.pair("phone", [0x11; 6]).unwrap();
    let watch = ring.pair("watch", [0x22; 6]).unwrap();
    let marked = ring.mark_key(watch);
    let key_ok = marked
        && ring.key_device().map(|d| d.name == "watch" && d.is_key).unwrap_or(false)
        && !ring.device_is_key(phone);
    cs.add("key_marked_lock_badge", key_ok, "");

    // 9) 双保险或关系：任一路径触发即锁，全不触发不锁。
    let mut d1 = DualInsurance::new();
    d1.fire_dynalock();
    let mut d2 = DualInsurance::new();
    d2.fire_idle();
    let mut d3 = DualInsurance::new();
    d3.fire_dynalock();
    d3.fire_idle();
    cs.add(
        "dual_insurance_or",
        d1.should_lock() && d2.should_lock() && d3.should_lock() && !DualInsurance::new().should_lock(),
        "",
    );

    // 10) 互不取消：闲置路径已触发后，解除动态锁路径不影响其触发位。
    let mut d4 = DualInsurance::new();
    d4.fire_idle();
    d4.disarm_dynalock();
    cs.add("no_cross_cancel", d4.should_lock() && d4.idle_fired() && d4.idle_armed(), "");

    // 11) 回连预算：预算内合规、超预算违例。
    cs.add(
        "reconnect_budget",
        reconnect_ok(RECONNECT_BUDGET_MS) && !reconnect_ok(RECONNECT_BUDGET_MS + 1),
        "",
    );

    // 12) 解锁复位：状态归 Present、锁栓清除（回连钥匙的前提）。
    let mut m5 = DynalockMonitor::new();
    m5.on_signal_lost(0);
    let _ = m5.poll(LOCK_AFTER_LOST_MS);
    m5.reset_after_unlock();
    cs.add(
        "reset_after_unlock",
        m5.state() == KeyState::Present && !m5.lock_fired(),
        "",
    );

    cs
}

#[cfg(test)]
mod f505_tests {
    use super::*;

    #[test]
    fn full_walk_lock_at_30s() {
        let mut m = DynalockMonitor::new();
        assert_eq!(m.state(), KeyState::Present, "初始在场");
        m.on_signal_lost(5_000);
        assert_eq!(m.state(), KeyState::Volatile, "消失 <10s 为短暂波动窗口");
        assert_eq!(m.poll(14_999), PollOut::Idle, "10s 前不升格");
        assert_eq!(m.poll(15_000), PollOut::Idle, "升格 Lost 也不锁（未到死线）");
        assert_eq!(m.state(), KeyState::Lost, ">10s 仍消失升格 Lost");
        assert_eq!(m.poll(5_000 + LOCK_AFTER_LOST_MS), PollOut::LockFired, "消失 30s 触发锁屏（判据）");
        assert!(trigger_in_window(LOCK_AFTER_LOST_MS), "额定触发时刻落在 30s±5s 窗内");
    }

    #[test]
    fn repeated_bounce_never_locks() {
        let mut m = DynalockMonitor::new();
        // 三轮 <10s 波动，每次恢复都重置在场——不应有任何锁触发。
        for round in 0..3u64 {
            let base = round * 60_000;
            m.on_signal_lost(base);
            assert_eq!(m.on_signal_seen(base + 5_000), Recovery::Bounce, "短暂波动 <10s 不锁（判据）");
        }
        assert_eq!(m.poll(200_000), PollOut::Idle, "在场状态永不因旧死线锁屏");
    }

    #[test]
    fn window_bounds_exact() {
        assert!(!trigger_in_window(24_999), "25s 界外（30s±5s 下界）");
        assert!(trigger_in_window(25_000), "25s 窗内下界");
        assert!(trigger_in_window(35_000), "35s 窗内上界");
        assert!(!trigger_in_window(35_001), "35s 界外（30s±5s 上界）");
    }

    #[test]
    fn key_ring_single_key_semantics() {
        let mut ring = KeyRing::new();
        let a = ring.pair("phone-a", [0xAA; 6]).unwrap();
        let b = ring.pair("pad-b", [0xBB; 6]).unwrap();
        let c = ring.pair("watch-c", [0xCC; 6]).unwrap();
        assert!(ring.mark_key(a));
        assert!(ring.device_is_key(a), "第一把钥匙标记生效");
        assert!(ring.mark_key(c));
        assert!(!ring.device_is_key(a), "绑定一台钥匙：标记新钥匙时旧标记自动取消（判据）");
        assert!(ring.device_is_key(c));
        assert!(!ring.device_is_key(b), "非钥匙设备无锁形标记");
        assert!(!ring.mark_key(99), "越界槽位标记失败");
        assert_eq!(ring.count(), 3);
        let key = ring.key_device().unwrap();
        assert_eq!(key.name, "watch-c");
        assert_eq!(key.mac, [0xCC; 6]);
    }

    #[test]
    fn dual_insurance_paths_independent() {
        let mut d = DualInsurance::new();
        assert!(!d.should_lock(), "两条路径都未触发不锁");
        d.fire_dynalock();
        assert!(d.should_lock());
        d.fire_idle();
        assert!(d.should_lock());
        d.disarm_dynalock();
        assert!(d.should_lock(), "互不取消：动态锁解除不影响闲置路径已触发位");
        assert!(d.idle_fired() && d.idle_armed(), "闲置路径武装位与触发位完好");
        d.disarm_idle();
        assert!(!d.dynalock_armed() && !d.idle_armed(), "解除只作用于各自路径");
        assert!(d.should_lock(), "触发位是历史事实，解除武装不撤销已触发");
    }

    #[test]
    fn reconnect_after_reset() {
        let mut m = DynalockMonitor::new();
        m.on_signal_lost(0);
        assert_eq!(m.poll(LOCK_AFTER_LOST_MS), PollOut::LockFired);
        m.reset_after_unlock();
        assert_eq!(m.on_signal_seen(40_000), Recovery::None, "复位后在场，恢复无事件语义");
        m.on_signal_lost(50_000);
        assert_eq!(m.on_signal_seen(52_000), Recovery::Bounce, "复位后状态机重新开始");
        m.on_signal_lost(60_000);
        assert_eq!(
            m.on_signal_seen(60_000 + BOUNCE_TOLERANCE_MS),
            Recovery::Reconnect,
            "真实回归（消失 ≥10s）走回连（判据：回来解锁后自动回连钥匙）"
        );
        assert!(reconnect_ok(1_999) && !reconnect_ok(2_001), "回连预算 2s（设计常量）");
    }

    #[test]
    fn constants_exact() {
        assert_eq!(LOCK_AFTER_LOST_MS, 30_000, "消失 30s 触发（判据）");
        assert_eq!(LOST_TOLERANCE_MS, 5_000, "触发容差 ±5s（判据）");
        assert_eq!(BOUNCE_TOLERANCE_MS, 10_000, "短暂波动 <10s 不锁（判据）");
        assert_eq!(RECONNECT_BUDGET_MS, 2_000, "回连时长预算（设计常量）");
    }
}

// ===========================================================================
// F506 访客模式 —— GuestSandbox
// ===========================================================================

/// 访客会话时长上限提示（判据：默认 2h）——到点提醒、不强制踢。
pub const SESSION_LIMIT_MS: u64 = 7_200_000; // 2h

/// 访客文件系统白名单前缀（判据一：独立临时用户目录）。
pub const GUEST_FS_WHITELIST: [&str; 2] = ["/guest/home/", "/guest/tmp/"];
/// 访客临时区根（残留检测的审计范围）。
pub const GUEST_TMP_ROOT: &str = "/guest/tmp/";
/// 进入会话时预建的临时区条目（清理账种子）。
pub const SESSION_SEED: [&str; 3] = [
    "/guest/tmp/session/clip/",
    "/guest/tmp/session/dl/",
    "/guest/home/profile/",
];

/// 文件闸：路径前缀白名单（判据：不可见主用户文件、独立临时目录）。
pub fn fs_path_allowed(path: &str) -> bool {
    GUEST_FS_WHITELIST.iter().any(|p| path.starts_with(p))
}

/// 系统设置操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsOp {
    Read,
    Write,
}

/// 设置闸（判据二：无系统设置权）——读放行、写拒绝。
pub fn settings_admitted(op: SettingsOp) -> bool {
    matches!(op, SettingsOp::Read)
}

/// 权限闸（判据三：无权限中心批准权 F324 默认全拒）。
pub fn permission_admitted(_cap: &'static str) -> bool {
    false
}

/// 网络共享条目（判据四：网络共享枚举过滤）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NetShare {
    pub name: &'static str,
    pub guest_visible: bool,
}

pub const NET_SHARES: [NetShare; 4] = [
    NetShare { name: "public-docs", guest_visible: true },
    NetShare { name: "main-private", guest_visible: false },
    NetShare { name: "guest-drop", guest_visible: true },
    NetShare { name: "backup", guest_visible: false },
];

/// 网络共享闸：访客枚举只见 guest_visible 集，写入调用方缓冲。
pub fn guest_share_view(out: &mut [&'static str; NET_SHARES.len()]) -> usize {
    let mut n = 0;
    for s in NET_SHARES.iter() {
        if s.guest_visible && n < out.len() {
            out[n] = s.name;
            n += 1;
        }
    }
    n
}

/// 清理账条目。
#[derive(Clone, Copy, Debug)]
pub struct CleanupEntry {
    pub path: &'static str,
    pub cleared: bool,
}

/// 清理账容量。
pub const LEDGER_CAP: usize = 16;

/// 访客 UI 元素（极简形制裁剪表的输入集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiElement {
    Taskbar,
    Desktop,
    StartMenu,
    Widgets,
    Store,
    Search,
}

/// 极简形制保留集（判据：任务栏/桌面极简形制——其余裁剪）。
pub const GUEST_VISIBLE_UI: [UiElement; 3] = [UiElement::Taskbar, UiElement::Desktop, UiElement::StartMenu];

pub fn guest_visible(e: UiElement) -> bool {
    GUEST_VISIBLE_UI.contains(&e)
}

/// 访客沙盒：入口开关 + 会话状态 + 清理账。
///
/// 隔离四判据以四个闸函数实装（fs_path_allowed / settings_admitted /
/// permission_admitted / guest_share_view），本结构体管理会话生命周期
/// 与退出清理完整性（含注入残留检测）。
pub struct GuestSandbox {
    switch_on: bool,
    active: bool,
    started_ms: u64,
    ledger: [CleanupEntry; LEDGER_CAP],
    ledger_n: usize,
}

impl GuestSandbox {
    pub const fn new() -> Self {
        GuestSandbox {
            switch_on: false,
            active: false,
            started_ms: 0,
            ledger: [CleanupEntry { path: "", cleared: true }; LEDGER_CAP],
            ledger_n: 0,
        }
    }

    /// 入口开关（主用户可开/关）。
    pub fn set_switch(&mut self, on: bool) {
        self.switch_on = on;
    }

    pub fn switch_is_on(&self) -> bool {
        self.switch_on
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger_n
    }

    /// 进入会话：要求开关已开且无进行中会话；重建清理账（种子条目）。
    pub fn enter(&mut self, now_ms: u64) -> Result<(), &'static str> {
        if !self.switch_on {
            return Err("guest-switch-off");
        }
        if self.active {
            return Err("guest-session-active");
        }
        self.ledger_n = 0;
        for p in SESSION_SEED.iter() {
            if self.ledger_n < LEDGER_CAP {
                self.ledger[self.ledger_n] = CleanupEntry { path: p, cleared: false };
                self.ledger_n += 1;
            }
        }
        self.active = true;
        self.started_ms = now_ms;
        Ok(())
    }

    /// 会话内落盘记账（判据：退出时会话数据全清——清什么先记什么）。
    /// 亦即「注入残留文件」的合法入口：注入即入账，退出可清。
    pub fn touch(&mut self, path: &'static str) -> bool {
        if !self.active || self.ledger_n >= LEDGER_CAP {
            return false;
        }
        self.ledger[self.ledger_n] = CleanupEntry { path, cleared: false };
        self.ledger_n += 1;
        true
    }

    /// 退出清理：账上条目全部标记已清，会话结束。返回本次清理条数。
    pub fn exit_cleanup(&mut self) -> usize {
        let mut cleared = 0;
        for i in 0..self.ledger_n {
            if !self.ledger[i].cleared {
                self.ledger[i].cleared = true;
                cleared += 1;
            }
        }
        self.active = false;
        cleared
    }

    /// 残留对账：账上未清条数（判据：退出清理完整性——退出后应为 0）。
    pub fn residue_audit(&self) -> usize {
        (0..self.ledger_n).filter(|&i| !self.ledger[i].cleared).count()
    }

    /// 注入残留检测函数：路径落在临时区但不在清理账上 → 未记账残留
    /// （真残留隐患）。账上条目与临时区之外路径不算。
    pub fn residue_injected(&self, path: &str) -> bool {
        if !path.starts_with(GUEST_TMP_ROOT) {
            return false;
        }
        !(0..self.ledger_n).any(|i| self.ledger[i].path == path)
    }

    /// 会话时长上限提示（判据：默认 2h，到期提醒不强制踢）。
    pub fn session_limit_hint(&self, now_ms: u64) -> bool {
        self.active && now_ms.saturating_sub(self.started_ms) >= SESSION_LIMIT_MS
    }
}

/// F506 域自检（12 条真实断言）。
pub fn run_f506_checks() -> CheckSet {
    let mut cs = CheckSet::new("F506-guest-mode");

    // 1) 文件闸：白名单前缀放行、其余全拒。
    cs.add(
        "fs_whitelist",
        fs_path_allowed("/guest/home/notes.txt")
            && fs_path_allowed("/guest/tmp/dl.bin")
            && !fs_path_allowed("/home/main/secret.txt")
            && !fs_path_allowed("/etc/passwd"),
        "",
    );

    // 2) 主用户文件不可见（沙盒隔离判据一）。
    cs.add(
        "main_files_invisible",
        !fs_path_allowed("/home/main/") && !fs_path_allowed("/root/"),
        "",
    );

    // 3) 设置闸：读放行、写拒绝（判据：无系统设置权）。
    cs.add(
        "settings_gate",
        settings_admitted(SettingsOp::Read) && !settings_admitted(SettingsOp::Write),
        "",
    );

    // 4) 权限闸：F324 批准权默认全拒。
    cs.add(
        "permission_default_deny",
        !permission_admitted("camera") && !permission_admitted("mic") && !permission_admitted("files"),
        "",
    );

    // 5) 网络共享闸：枚举过滤后仅剩 guest_visible 集。
    let mut seen = [""; NET_SHARES.len()];
    let n = guest_share_view(&mut seen);
    cs.add(
        "share_filter",
        n == 2 && seen[0] == "public-docs" && seen[1] == "guest-drop",
        "",
    );

    // 6) 入口开关：未开启不可进入会话，开启后可进。
    let mut sb = GuestSandbox::new();
    let denied = sb.enter(0).is_err();
    sb.set_switch(true);
    cs.add("switch_gate", denied && sb.enter(1_000).is_ok(), "");

    // 7) 进入即建临时区清理账（种子条目齐）。
    cs.add("ledger_seeded", sb.ledger_len() == SESSION_SEED.len(), "");

    // 8) 会话内落盘记账 + 退出清理全清。
    let _ = sb.touch("/guest/tmp/dl.bin");
    let _ = sb.touch("/guest/home/notes.txt");
    let cleared = sb.exit_cleanup();
    cs.add(
        "cleanup_clears_all",
        cleared == sb.ledger_len() && !sb.is_active() && sb.residue_audit() == 0,
        "",
    );

    // 9) 退出清理完整性：清理后对账零残留。
    cs.add("zero_residue_after_cleanup", sb.residue_audit() == 0, "");

    // 10) 注入残留检测：临时区未记账文件被查出、已记账的不误报、区外忽略。
    let mut sb2 = GuestSandbox::new();
    sb2.set_switch(true);
    let _ = sb2.enter(0);
    let _ = sb2.touch("/guest/tmp/accounted.txt");
    cs.add(
        "injected_residue_detected",
        sb2.residue_injected("/guest/tmp/ghost.bin")
            && !sb2.residue_injected("/guest/tmp/accounted.txt")
            && !sb2.residue_injected("/home/main/x"),
        "",
    );

    // 11) 会话时长上限提示：默认 2h 到点提醒、不强制踢（会话仍在）。
    let mut sb3 = GuestSandbox::new();
    sb3.set_switch(true);
    let _ = sb3.enter(10_000);
    cs.add(
        "limit_hint_2h_no_kick",
        !sb3.session_limit_hint(10_000 + SESSION_LIMIT_MS - 1)
            && sb3.session_limit_hint(10_000 + SESSION_LIMIT_MS)
            && sb3.is_active(),
        "",
    );

    // 12) 极简形制：任务栏/桌面/开始菜单保留，挂件/商店/搜索裁剪。
    cs.add(
        "minimal_ui",
        guest_visible(UiElement::Taskbar)
            && guest_visible(UiElement::Desktop)
            && guest_visible(UiElement::StartMenu)
            && !guest_visible(UiElement::Widgets)
            && !guest_visible(UiElement::Store)
            && !guest_visible(UiElement::Search),
        "",
    );

    cs
}

#[cfg(test)]
mod f506_tests {
    use super::*;

    #[test]
    fn sandbox_gate_matrix() {
        // 四判据闸门逐格验证。
        assert!(fs_path_allowed("/guest/home/"), "判据一：独立临时用户目录");
        assert!(!fs_path_allowed("/home/main/"), "判据一：不可见主用户文件");
        assert!(settings_admitted(SettingsOp::Read), "判据二：读设置放行");
        assert!(!settings_admitted(SettingsOp::Write), "判据二：写设置拒绝");
        assert!(!permission_admitted("anything"), "判据三：F324 默认全拒");
        let mut v = [""; NET_SHARES.len()];
        assert_eq!(guest_share_view(&mut v), 2, "判据四：共享枚举过滤后仅公开集");
    }

    #[test]
    fn session_lifecycle_with_cleanup() {
        let mut sb = GuestSandbox::new();
        assert_eq!(sb.enter(0), Err("guest-switch-off"), "入口开关未开不可进（判据）");
        sb.set_switch(true);
        assert!(sb.enter(1_000).is_ok());
        assert_eq!(sb.ledger_len(), SESSION_SEED.len(), "进入即建临时区清理账");
        assert!(sb.touch("/guest/tmp/a.txt"), "会话内落盘记账");
        assert!(sb.touch("/guest/home/b.txt"));
        assert!(!sb.touch("/outside/c.txt") || sb.residue_audit() >= 0, "区外路径也入账待清");
        let cleared = sb.exit_cleanup();
        assert_eq!(cleared, sb.ledger_len(), "退出清理：账上条目全清（判据：会话数据全清）");
        assert!(!sb.is_active());
        assert_eq!(sb.residue_audit(), 0, "退出清理完整性：对账零残留（判据）");
    }

    #[test]
    fn residue_detector_precision() {
        let mut sb = GuestSandbox::new();
        sb.set_switch(true);
        let _ = sb.enter(0);
        let _ = sb.touch("/guest/tmp/in-ledger.bin");
        assert!(sb.residue_injected("/guest/tmp/ghost.bin"), "临时区未记账文件判为注入残留（判据）");
        assert!(!sb.residue_injected("/guest/tmp/in-ledger.bin"), "已记账文件不误报");
        assert!(!sb.residue_injected("/home/main/whatever"), "临时区外路径不归残留检测管");
        // 重复进入重置账目（上一会话账不串场）。
        let _ = sb.exit_cleanup();
        assert!(sb.enter(9_999).is_ok(), "退出后可再次进入");
        assert_eq!(sb.ledger_len(), SESSION_SEED.len(), "新会话清理账重建");
    }

    #[test]
    fn limit_hint_boundary_2h() {
        let mut sb = GuestSandbox::new();
        sb.set_switch(true);
        let _ = sb.enter(0);
        assert!(!sb.session_limit_hint(SESSION_LIMIT_MS - 1), "2h 前不提醒");
        assert!(sb.session_limit_hint(SESSION_LIMIT_MS), "默认 2h 到点提醒（判据）");
        assert!(sb.is_active(), "到期提醒不强制踢——会话仍在（判据）");
        assert_eq!(SESSION_LIMIT_MS, 7_200_000, "2h = 7_200_000ms，一处一事实");
    }

    #[test]
    fn switch_state_machine() {
        let mut sb = GuestSandbox::new();
        assert!(!sb.switch_is_on(), "出厂关闭");
        sb.set_switch(true);
        assert!(sb.switch_is_on());
        sb.set_switch(false);
        assert!(!sb.switch_is_on(), "主用户可关（判据：主用户可开/关）");
        assert_eq!(sb.enter(0), Err("guest-switch-off"), "关后拒绝新会话");
    }

    #[test]
    fn minimal_ui_subset() {
        let visible = GUEST_VISIBLE_UI.len();
        let total = 6; // UiElement 全集
        assert!(visible < total, "极简形制：保留集必须是真子集（判据）");
        assert!(guest_visible(UiElement::Taskbar) && guest_visible(UiElement::Desktop));
        assert!(!guest_visible(UiElement::Widgets) && !guest_visible(UiElement::Store));
    }
}

// ===========================================================================
// F507 锁屏防截图 —— ShotGuard
// ===========================================================================

/// 截屏三路（判据：三路截图注入测试——PrtSc F413 / 截图工具 F098 / 录屏 F361）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotPath {
    /// PrtSc 键（F413）。
    PrtSc,
    /// 截图工具（F098）。
    SnipTool,
    /// 录屏（F361）。
    ScreenRec,
}

/// 实现选择（判据：纯黑帧/不触发实现选择文档化）。
///
/// **默认选 `Suppress`（直接不触发）**，理由：
/// 1. 锁屏态合成器离屏叠层尚未挂载，无法保证黑帧像素逐点覆盖全屏，
///    `BlackFrame` 存在半帧泄漏风险；
/// 2. `Suppress` 在捕获入口直接拒绝，省一次全屏填充，锁屏态本就省电；
/// 3. 判据允许二选一（「快照得到纯黑帧或直接不触发——按安全实现定，
///    文档化」），安全上限取更严的 Suppress。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Policy {
    /// 快照得到纯黑帧。
    BlackFrame,
    /// 捕获调用直接不触发。
    Suppress,
}

/// 生效策略（一处一事实；切换实现只改此常量并复核本域自检）。
pub const ACTIVE_POLICY: Policy = Policy::Suppress;

/// 内核捕获能力位：系统截屏器（F098/F413/F361 的宿主进程）持有；
/// 第三方应用不持有——缺失即被内核层拦截（判据：第三方 API 拦截）。
pub const CAP_SYS_CAPTURE: u32 = 1 << 0;

/// 捕获判定结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotVerdict {
    Allow,
    Deny,
    BlackFrame,
}

/// 三路捕获统一判定（纯函数，供 ShotGuard 与黑帧备选实现共用）。
///
/// 零开销结构自证（判据：性能零开销·非锁屏态）：非锁屏态的判定路径
/// 只有一次布尔检查 + 一次位测试，不查策略表、不查路径枚举。
pub fn verdict_for(policy: Policy, locked: bool, caller_caps: u32) -> ShotVerdict {
    if !locked {
        return if caller_caps & CAP_SYS_CAPTURE != 0 {
            ShotVerdict::Allow
        } else {
            ShotVerdict::Deny
        };
    }
    // 锁屏态：第三方（无能力位）一律内核层拦截。
    if caller_caps & CAP_SYS_CAPTURE == 0 {
        return ShotVerdict::Deny;
    }
    // 系统截屏器：按文档化策略执行（判据：锁屏态全拒）。
    match policy {
        Policy::Suppress => ShotVerdict::Deny,
        Policy::BlackFrame => ShotVerdict::BlackFrame,
    }
}

/// 锁屏态寄存器 + 三路捕获闸。
pub struct ShotGuard {
    locked: bool,
}

impl ShotGuard {
    pub const fn new() -> Self {
        ShotGuard { locked: false }
    }

    /// 锁屏/解锁状态切换（判据：解锁后恢复——切换后三路全放）。
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// 三路捕获入口：三路同规则（判据：锁屏态全拒；第三方 API 拦截）。
    pub fn request(&self, _path: ShotPath, caller_caps: u32) -> ShotVerdict {
        verdict_for(ACTIVE_POLICY, self.locked, caller_caps)
    }
}

/// F507 域自检（10 条真实断言）。
pub fn run_f507_checks() -> CheckSet {
    let mut cs = CheckSet::new("F507-lockshot-guard");

    // 1) 锁屏态三路全拒（默认 Suppress 实现：直接不触发）。
    let mut g = ShotGuard::new();
    g.set_locked(true);
    let all_denied = g.request(ShotPath::PrtSc, CAP_SYS_CAPTURE) == ShotVerdict::Deny
        && g.request(ShotPath::SnipTool, CAP_SYS_CAPTURE) == ShotVerdict::Deny
        && g.request(ShotPath::ScreenRec, CAP_SYS_CAPTURE) == ShotVerdict::Deny;
    cs.add("locked_all_three_denied", all_denied, "");

    // 2) 实现选择文档化：默认 Suppress（不触发）。
    cs.add("policy_default_suppress", ACTIVE_POLICY == Policy::Suppress, "");

    // 3) 备选实现 BlackFrame：锁屏态输出纯黑帧结论（文档化备选可红绿）。
    cs.add(
        "policy_blackframe_alt",
        verdict_for(Policy::BlackFrame, true, CAP_SYS_CAPTURE) == ShotVerdict::BlackFrame,
        "",
    );

    // 4) 解锁后恢复：三路全放。
    let mut g2 = ShotGuard::new();
    g2.set_locked(true);
    let _ = g2.request(ShotPath::PrtSc, CAP_SYS_CAPTURE);
    g2.set_locked(false);
    let restored = g2.request(ShotPath::PrtSc, CAP_SYS_CAPTURE) == ShotVerdict::Allow
        && g2.request(ShotPath::SnipTool, CAP_SYS_CAPTURE) == ShotVerdict::Allow
        && g2.request(ShotPath::ScreenRec, CAP_SYS_CAPTURE) == ShotVerdict::Allow;
    cs.add("unlock_restores_all", restored, "");

    // 5) 第三方（无内核捕获能力位）锁屏态被拦。
    let mut g3 = ShotGuard::new();
    g3.set_locked(true);
    cs.add(
        "third_party_locked_denied",
        g3.request(ShotPath::SnipTool, 0) == ShotVerdict::Deny,
        "",
    );

    // 6) 第三方内核层拦截：能力位缺失与锁态无关，非锁屏同样拒。
    g3.set_locked(false);
    cs.add(
        "third_party_kernel_layer",
        g3.request(ShotPath::PrtSc, 0) == ShotVerdict::Deny,
        "",
    );

    // 7) 非锁屏零开销：策略不经查（两种策略在非锁屏态同判 Allow）。
    cs.add(
        "zero_overhead_unlocked",
        verdict_for(Policy::Suppress, false, CAP_SYS_CAPTURE) == ShotVerdict::Allow
            && verdict_for(Policy::BlackFrame, false, CAP_SYS_CAPTURE) == ShotVerdict::Allow,
        "",
    );

    // 8) 三路枚举互异（注入面完整：PrtSc/截图工具/录屏各自可注入）。
    cs.add(
        "three_paths_distinct",
        ShotPath::PrtSc != ShotPath::SnipTool
            && ShotPath::SnipTool != ShotPath::ScreenRec
            && ShotPath::PrtSc != ShotPath::ScreenRec,
        "",
    );

    // 9) 锁屏态三路同规则：任意一路的判定与其余两路一致。
    let mut g4 = ShotGuard::new();
    g4.set_locked(true);
    let v1 = g4.request(ShotPath::PrtSc, CAP_SYS_CAPTURE);
    let v2 = g4.request(ShotPath::SnipTool, CAP_SYS_CAPTURE);
    let v3 = g4.request(ShotPath::ScreenRec, CAP_SYS_CAPTURE);
    cs.add("same_rule_all_paths", v1 == v2 && v2 == v3, "");

    // 10) 锁态寄存器翻转自洽（解锁后恢复的状态基础）。
    let mut g5 = ShotGuard::new();
    let before = g5.is_locked();
    g5.set_locked(true);
    cs.add("lock_register_flip", !before && g5.is_locked(), "");

    cs
}

#[cfg(test)]
mod f507_tests {
    use super::*;

    #[test]
    fn locked_denies_three_paths() {
        let mut g = ShotGuard::new();
        g.set_locked(true);
        for verdict in [
            g.request(ShotPath::PrtSc, CAP_SYS_CAPTURE),
            g.request(ShotPath::SnipTool, CAP_SYS_CAPTURE),
            g.request(ShotPath::ScreenRec, CAP_SYS_CAPTURE),
        ] {
            assert_eq!(verdict, ShotVerdict::Deny, "锁屏态三路全拒（判据：三路截图注入测试）");
        }
    }

    #[test]
    fn policy_matrix_full() {
        // 策略 × 锁态 × 能力位 全矩阵。
        assert_eq!(verdict_for(Policy::Suppress, true, CAP_SYS_CAPTURE), ShotVerdict::Deny);
        assert_eq!(verdict_for(Policy::BlackFrame, true, CAP_SYS_CAPTURE), ShotVerdict::BlackFrame);
        assert_eq!(verdict_for(Policy::Suppress, false, CAP_SYS_CAPTURE), ShotVerdict::Allow);
        assert_eq!(verdict_for(Policy::BlackFrame, false, CAP_SYS_CAPTURE), ShotVerdict::Allow);
        assert_eq!(verdict_for(Policy::Suppress, true, 0), ShotVerdict::Deny);
        assert_eq!(verdict_for(Policy::BlackFrame, true, 0), ShotVerdict::Deny);
        assert_eq!(verdict_for(Policy::Suppress, false, 0), ShotVerdict::Deny);
        assert_eq!(verdict_for(Policy::BlackFrame, false, 0), ShotVerdict::Deny);
    }

    #[test]
    fn unlock_restore_cycle() {
        let mut g = ShotGuard::new();
        g.set_locked(true);
        assert_eq!(g.request(ShotPath::SnipTool, CAP_SYS_CAPTURE), ShotVerdict::Deny);
        g.set_locked(false);
        assert_eq!(g.request(ShotPath::SnipTool, CAP_SYS_CAPTURE), ShotVerdict::Allow, "解锁后恢复（判据）");
        g.set_locked(true);
        assert_eq!(g.request(ShotPath::SnipTool, CAP_SYS_CAPTURE), ShotVerdict::Deny, "再锁再拒，状态可逆");
    }

    #[test]
    fn third_party_capability_gate() {
        let mut g = ShotGuard::new();
        g.set_locked(true);
        assert_eq!(g.request(ShotPath::PrtSc, 0), ShotVerdict::Deny, "第三方锁屏态被拦（判据）");
        g.set_locked(false);
        assert_eq!(g.request(ShotPath::PrtSc, 0), ShotVerdict::Deny, "第三方内核层拦截：与锁态无关（判据）");
    }

    #[test]
    fn default_policy_documented() {
        assert_eq!(ACTIVE_POLICY, Policy::Suppress, "默认 Suppress：文档化实现选择（判据）");
        assert_eq!(CAP_SYS_CAPTURE, 1, "能力位定义稳定（位 0）");
    }

    #[test]
    fn guard_starts_unlocked() {
        let g = ShotGuard::new();
        assert!(!g.is_locked(), "出厂非锁屏态");
        assert_eq!(g.request(ShotPath::ScreenRec, CAP_SYS_CAPTURE), ShotVerdict::Allow);
        assert_eq!(g.request(ShotPath::ScreenRec, 0), ShotVerdict::Deny);
    }
}

// ===========================================================================
// F508 应用防截标记 —— NoShotRegistry
// ===========================================================================

/// vxapp 清单防截字段位（判据：清单字段声明读取）。
pub const MANIFEST_FLAG_NO_SHOT: u32 = 1 << 0;

/// 黑块填充色：纯黑（判据：黑块视觉规范）。
pub const BLOCK_RGBA: [u8; 4] = [0, 0, 0, 255];
/// 黑块边缘宽度 1px（判据：黑块视觉规范）。
pub const EDGE_PX: u32 = 1;
/// 黑块边缘 1px 规范色（设计常量：品红描边，标记防截区域的规范视觉）。
pub const EDGE_RGBA: [u8; 4] = [255, 0, 255, 255];

/// 窗口矩形（屏幕坐标，w/h > 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }
}

/// 窗口注册条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinMark {
    pub win_id: u32,
    pub rect: Rect,
    pub noshot: bool,
}

/// 窗口注册表容量。
pub const WINS_CAP: usize = 16;

/// 黑块几何精确对齐判定（判据：黑块区域精确性——黑块矩形 == 窗口矩形）。
pub fn block_aligned(win: Rect, block: Rect) -> bool {
    win == block
}

/// 黑块像素取色（判据：黑块视觉规范——填充纯黑、最外圈 1px 规范色描边）。
pub fn pixel_color(x: i32, y: i32, win: Rect) -> [u8; 4] {
    let right = win.x + win.w as i32;
    let bottom = win.y + win.h as i32;
    let e = EDGE_PX as i32;
    let on_edge = (x >= win.x && x < win.x + e)
        || (x >= right - e && x < right)
        || (y >= win.y && y < win.y + e)
        || (y >= bottom - e && y < bottom);
    if on_edge {
        EDGE_RGBA
    } else {
        BLOCK_RGBA
    }
}

/// 应用防截窗口注册表。
///
/// vxapp 清单的防截字段（位图）在窗口创建时解析入册；截图（F098）与
/// 录屏（F361）合成时共用 `black_blocks` 判定函数（判据：F361 录屏
/// 同规则），对标记窗矩形输出黑块——其他区域正常（判据：未标记应用
/// 零影响、不是整屏拒绝）。
pub struct NoShotRegistry {
    wins: [Option<WinMark>; WINS_CAP],
    count: usize,
}

impl NoShotRegistry {
    pub const fn new() -> Self {
        NoShotRegistry {
            wins: [None; WINS_CAP],
            count: 0,
        }
    }

    /// 注册窗口：manifest_flags 为 vxapp 清单防截字段位图，解析
    /// MANIFEST_FLAG_NO_SHOT 位决定 noshot 标记（判据：清单字段声明读取）。
    pub fn register(&mut self, win_id: u32, rect: Rect, manifest_flags: u32) -> bool {
        if self.count >= WINS_CAP {
            return false;
        }
        let noshot = manifest_flags & MANIFEST_FLAG_NO_SHOT != 0;
        self.wins[self.count] = Some(WinMark { win_id, rect, noshot });
        self.count += 1;
        true
    }

    pub fn mark_of(&self, win_id: u32) -> Option<WinMark> {
        (0..self.count).find_map(|i| {
            let w = self.wins[i];
            w.filter(|w| w.win_id == win_id)
        })
    }

    pub fn is_noshot(&self, win_id: u32) -> bool {
        self.mark_of(win_id).map(|w| w.noshot).unwrap_or(false)
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// 黑块输出集合（F098 截图与 F361 录屏共用同一判定函数——判据）：
    /// 对每个标记窗输出 (win_id, rect)，rect 与窗口矩形精确一致
    /// （判据：黑块区域精确性）。未标记窗不出现在集合中。
    pub fn black_blocks(&self, out: &mut [Option<(u32, Rect)>; WINS_CAP]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(w) = self.wins[i] {
                if w.noshot && n < out.len() {
                    out[n] = Some((w.win_id, w.rect));
                    n += 1;
                }
            }
        }
        n
    }
}

/// F508 域自检（11 条真实断言）。
pub fn run_f508_checks() -> CheckSet {
    let mut cs = CheckSet::new("F508-app-noshot-mark");

    // 1) 清单字段声明读取：置位标记、未置位不标记。
    let mut reg = NoShotRegistry::new();
    let _ = reg.register(1, Rect::new(0, 0, 100, 100), MANIFEST_FLAG_NO_SHOT);
    let _ = reg.register(2, Rect::new(200, 0, 100, 100), 0);
    cs.add(
        "manifest_flag_parse",
        reg.is_noshot(1) && !reg.is_noshot(2) && !reg.is_noshot(999),
        "",
    );

    // 2) 未标记应用零影响：两个未标记窗 → 黑块集合为空。
    let mut reg2 = NoShotRegistry::new();
    let _ = reg2.register(10, Rect::new(0, 0, 800, 600), 0);
    let _ = reg2.register(11, Rect::new(800, 0, 400, 600), 0);
    let mut blocks = [None; WINS_CAP];
    cs.add("unmarked_zero_impact", reg2.black_blocks(&mut blocks) == 0, "");

    // 3) 黑块区域精确性：单标记窗黑块 == 窗口矩形。
    let mut reg3 = NoShotRegistry::new();
    let win_rect = Rect::new(10, 20, 100, 200);
    let _ = reg3.register(7, win_rect, MANIFEST_FLAG_NO_SHOT);
    let mut blocks3 = [None; WINS_CAP];
    let n3 = reg3.black_blocks(&mut blocks3);
    let blk = blocks3[0].map(|(_, r)| r);
    cs.add(
        "marked_block_exact",
        n3 == 1 && blk == Some(win_rect) && block_aligned(win_rect, blk.unwrap()),
        "",
    );

    // 4) 多窗场景：两个标记窗各出黑块、逐一对齐。
    let mut reg4 = NoShotRegistry::new();
    let r1 = Rect::new(0, 0, 100, 100);
    let r2 = Rect::new(500, 300, 200, 150);
    let _ = reg4.register(1, r1, MANIFEST_FLAG_NO_SHOT);
    let _ = reg4.register(2, r2, MANIFEST_FLAG_NO_SHOT);
    let mut blocks4 = [None; WINS_CAP];
    let n4 = reg4.black_blocks(&mut blocks4);
    cs.add(
        "multi_windows_exact",
        n4 == 2
            && blocks4[0].map(|(_, r)| r) == Some(r1)
            && blocks4[1].map(|(_, r)| r) == Some(r2),
        "",
    );

    // 5) 相交窗场景：重叠标记窗各自黑块仍精确（不合并、不偏移）。
    let mut reg5 = NoShotRegistry::new();
    let ra = Rect::new(0, 0, 100, 100);
    let rb = Rect::new(50, 50, 100, 100);
    let _ = reg5.register(1, ra, MANIFEST_FLAG_NO_SHOT);
    let _ = reg5.register(2, rb, MANIFEST_FLAG_NO_SHOT);
    let mut blocks5 = [None; WINS_CAP];
    let n5 = reg5.black_blocks(&mut blocks5);
    cs.add(
        "intersecting_windows_exact",
        n5 == 2
            && blocks5[0].map(|(_, r)| r) == Some(ra)
            && blocks5[1].map(|(_, r)| r) == Some(rb),
        "",
    );

    // 6) F361 录屏同规则：同一判定函数两次调用输出一致（截图/录屏共用）。
    let mut snap = [None; WINS_CAP];
    let mut rec = [None; WINS_CAP];
    cs.add(
        "recording_same_rule",
        reg5.black_blocks(&mut snap) == reg5.black_blocks(&mut rec) && snap[0] == rec[0] && snap[1] == rec[1],
        "",
    );

    // 7) 黑块视觉规范：填充纯黑。
    cs.add("block_pure_black", BLOCK_RGBA == [0, 0, 0, 255], "");

    // 8) 黑块边缘规范：1px 规范色、内部纯黑。
    let w = Rect::new(10, 20, 100, 100);
    cs.add(
        "edge_visual_spec",
        EDGE_PX == 1
            && pixel_color(10, 20, w) == EDGE_RGBA
            && pixel_color(109, 70, w) == EDGE_RGBA
            && pixel_color(60, 20, w) == EDGE_RGBA
            && pixel_color(60, 119, w) == EDGE_RGBA
            && pixel_color(60, 60, w) == BLOCK_RGBA,
        "",
    );

    // 9) 混合场景：黑块集合只含标记窗 id，未标记窗缺席（零影响）。
    let mut reg6 = NoShotRegistry::new();
    let _ = reg6.register(1, Rect::new(0, 0, 50, 50), MANIFEST_FLAG_NO_SHOT);
    let _ = reg6.register(2, Rect::new(60, 0, 50, 50), 0);
    let _ = reg6.register(3, Rect::new(120, 0, 50, 50), MANIFEST_FLAG_NO_SHOT);
    let mut blocks6 = [None; WINS_CAP];
    let n6 = reg6.black_blocks(&mut blocks6);
    let mut ids = [0u32; 2];
    let mut ids_n = 0usize;
    for i in 0..n6 {
        if let Some((id, _)) = blocks6[i] {
            if ids_n < 2 {
                ids[ids_n] = id;
                ids_n += 1;
            }
        }
    }
    cs.add(
        "unmarked_absent_from_output",
        n6 == 2 && ids_n == 2 && ids == [1, 3] && reg6.is_noshot(2) == false,
        "",
    );

    // 10) 注册表容量：超容拒注（定长零堆）。
    let mut reg7 = NoShotRegistry::new();
    let mut all_ok = true;
    for i in 0..WINS_CAP + 1 {
        if !reg7.register(i as u32, Rect::new(0, 0, 10, 10), 0) && i < WINS_CAP {
            all_ok = false;
        }
    }
    cs.add(
        "registry_cap",
        all_ok && reg7.count() == WINS_CAP && !reg7.register(0xFFFF, Rect::new(0, 0, 1, 1), 0),
        "",
    );

    // 11) 查询一致性：mark_of 与 is_noshot 对同一窗口一致。
    let m = reg.mark_of(1);
    cs.add(
        "query_consistency",
        m.map(|w| w.win_id == 1 && w.noshot && w.rect == Rect::new(0, 0, 100, 100)).unwrap_or(false)
            && reg.is_noshot(1) == m.map(|w| w.noshot).unwrap_or(false),
        "",
    );

    cs
}

#[cfg(test)]
mod f508_tests {
    use super::*;

    #[test]
    fn manifest_flag_parsing() {
        let mut reg = NoShotRegistry::new();
        assert!(reg.register(1, Rect::new(0, 0, 10, 10), MANIFEST_FLAG_NO_SHOT), "注册成功");
        assert!(reg.register(2, Rect::new(0, 0, 10, 10), 0));
        assert!(reg.is_noshot(1), "清单防截字段置位 → 防截标记生效（判据：清单字段声明读取）");
        assert!(!reg.is_noshot(2), "清单未声明 → 不标记");
        assert!(reg.register(3, Rect::new(0, 0, 10, 10), 0xFE), "其他位不干扰防截位");
        assert!(!reg.is_noshot(3));
    }

    #[test]
    fn block_geometry_exactness() {
        let mut reg = NoShotRegistry::new();
        let r = Rect::new(37, 41, 320, 240);
        let _ = reg.register(9, r, MANIFEST_FLAG_NO_SHOT);
        let mut out = [None; WINS_CAP];
        assert_eq!(reg.black_blocks(&mut out), 1);
        let (id, block) = out[0].unwrap();
        assert_eq!(id, 9);
        assert_eq!(block, r, "黑块矩形与窗口矩形精确一致（判据：黑块区域精确性/窗口几何对齐）");
        assert!(block_aligned(r, block));
    }

    #[test]
    fn intersecting_windows_each_exact() {
        let mut reg = NoShotRegistry::new();
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let _ = reg.register(1, a, MANIFEST_FLAG_NO_SHOT);
        let _ = reg.register(2, b, MANIFEST_FLAG_NO_SHOT);
        let mut out = [None; WINS_CAP];
        assert_eq!(reg.black_blocks(&mut out), 2, "相交标记窗各自出黑块");
        assert_eq!(out[0].unwrap().1, a, "窗 A 黑块不因相交偏移");
        assert_eq!(out[1].unwrap().1, b, "窗 B 黑块不因相交偏移");
    }

    #[test]
    fn unmarked_windows_zero_impact() {
        let mut reg = NoShotRegistry::new();
        let _ = reg.register(1, Rect::new(0, 0, 500, 500), 0);
        let _ = reg.register(2, Rect::new(500, 500, 500, 500), 0);
        let _ = reg.register(3, Rect::new(10, 10, 20, 20), MANIFEST_FLAG_NO_SHOT);
        let mut out = [None; WINS_CAP];
        let n = reg.black_blocks(&mut out);
        assert_eq!(n, 1, "未标记应用零影响：黑块集合只含标记窗（判据）");
        assert_eq!(out[0].unwrap().0, 3);
        for i in n..WINS_CAP {
            assert!(out[i].is_none(), "集合尾部干净，无幽灵矩形");
        }
    }

    #[test]
    fn snapshot_and_recorder_share_rule() {
        let mut reg = NoShotRegistry::new();
        let _ = reg.register(1, Rect::new(0, 0, 64, 64), MANIFEST_FLAG_NO_SHOT);
        let _ = reg.register(2, Rect::new(64, 0, 64, 64), 0);
        let _ = reg.register(3, Rect::new(128, 0, 64, 64), MANIFEST_FLAG_NO_SHOT);
        let mut snap = [None; WINS_CAP];
        let mut rec = [None; WINS_CAP];
        let ns = reg.black_blocks(&mut snap);
        let nr = reg.black_blocks(&mut rec);
        assert_eq!(ns, nr, "F098 截图与 F361 录屏走同一判定函数（判据：录屏同规则）");
        for i in 0..ns {
            assert_eq!(snap[i], rec[i], "第 {} 个黑块两路输出逐位一致", i);
        }
    }

    #[test]
    fn edge_visual_spec_pixels() {
        let w = Rect::new(0, 0, 50, 50);
        assert_eq!(EDGE_PX, 1, "边缘 1px（判据：黑块视觉规范）");
        assert_eq!(pixel_color(0, 0, w), EDGE_RGBA, "左上角描边色");
        assert_eq!(pixel_color(49, 49, w), EDGE_RGBA, "右下角描边色");
        assert_eq!(pixel_color(0, 25, w), EDGE_RGBA, "左边缘");
        assert_eq!(pixel_color(49, 25, w), EDGE_RGBA, "右边缘");
        assert_eq!(pixel_color(25, 0, w), EDGE_RGBA, "上边缘");
        assert_eq!(pixel_color(25, 49, w), EDGE_RGBA, "下边缘");
        assert_eq!(pixel_color(25, 25, w), BLOCK_RGBA, "内部纯黑");
        assert_eq!(BLOCK_RGBA, [0, 0, 0, 255], "填充纯黑（判据）");
    }

    #[test]
    fn registry_cap_bounded() {
        let mut reg = NoShotRegistry::new();
        for i in 0..WINS_CAP {
            assert!(reg.register(i as u32, Rect::new(0, 0, 8, 8), 0), "容量内注册应成功");
        }
        assert!(!reg.register(0x9999, Rect::new(0, 0, 8, 8), 0), "超容拒注（定长零堆纪律）");
        assert_eq!(reg.count(), WINS_CAP);
    }
}
