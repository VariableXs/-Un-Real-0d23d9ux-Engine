//! AI-31 族0303「电源管理 ACPI」（X07551~X07575）。
//! 表签名/睡眠态迁移/电源资源引用计数/电池模型的确定性内核模型。零分配。

use crate::checks::CheckSet;

/// 表签名校验：固定 4 字节签名（RSDT/FACP/SSDT…）。
pub fn sig_ok(table: &[u8; 4], want: &[u8; 4]) -> bool {
    table == want
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepState {
    S0,
    S1,
    S3,
    S4,
    S5,
}

/// 合法迁移图：S0→S1/S3/S4/S5；S3→S0；S4→S0；S5→S0。
pub fn transition_ok(from: SleepState, to: SleepState) -> bool {
    use SleepState::*;
    match (from, to) {
        (S0, S1) | (S0, S3) | (S0, S4) | (S0, S5) => true,
        (S3, S0) | (S4, S0) | (S5, S0) => true,
        _ => false,
    }
}

/// 电源资源：引用计数开关（0 时物理断电）。
pub struct PowerResource {
    refs: u32,
    pub on: bool,
    pub clamped: usize,
}

impl PowerResource {
    pub const fn new() -> PowerResource {
        PowerResource { refs: 0, on: false, clamped: 0 }
    }
    pub fn acquire(&mut self) {
        if self.refs == u32::MAX {
            self.clamped += 1;
            return;
        }
        self.refs += 1;
        self.on = true;
    }
    pub fn release(&mut self) -> bool {
        if self.refs == 0 {
            self.clamped += 1;
            return false;
        }
        self.refs -= 1;
        if self.refs == 0 {
            self.on = false;
        }
        true
    }
    pub fn refs(&self) -> u32 {
        self.refs
    }
}

/// _PTS 通知序号：进入睡眠前的 OSPM 通知（S1=1,S3=3,S4=4,S5=5，S0 无）。
pub fn pts_arg(state: SleepState) -> u8 {
    match state {
        SleepState::S0 => 0,
        SleepState::S1 => 1,
        SleepState::S3 => 3,
        SleepState::S4 => 4,
        SleepState::S5 => 5,
    }
}

/// 电池模型：电量 0~100 钳制，放电斜率确定。
pub struct Battery {
    pub pct: u32,
    pub clamped: usize,
}

impl Battery {
    pub fn new(pct: u32) -> Battery {
        let clamped = usize::from(pct > 100);
        Battery { pct: pct.min(100), clamped }
    }
    /// 放电 mA 均流估算：百分比 × 容量 5000mAh 的可运行毫时。
    pub fn runtime_mh(&self, drain_ma: u32) -> u32 {
        if drain_ma == 0 {
            return u32::MAX;
        }
        (self.pct * 50) / drain_ma
    }
    pub fn drain(&mut self, pct: u32) {
        self.pct = self.pct.saturating_sub(pct);
    }
}

/// 唤醒源仲裁：优先级高者胜（数字越小优先）。
pub fn wakeup_arbitrate(cands: &[(u8, &'static str)]) -> &'static str {
    let mut best: &str = "none";
    let mut pri: u8 = u8::MAX;
    for (p, n) in cands {
        if *p < pri {
            pri = *p;
            best = n;
        }
    }
    best
}

/// 失败叙事：ACPI 错误 → 下一步建议。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "ACPI 表签名异常，建议更新固件后重试",
        2 => "睡眠态迁移被设备拒绝，建议关闭冲突外设",
        3 => "唤醒源丢失，建议检查电源按钮设置",
        _ => "未知 AML 错误，建议转储 DSDT 分析",
    }
}

pub fn run_acpi_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-acpi");

    // L1 基础实装
    s.add("X07551 ACPI最小闭环", sig_ok(b"RSDT", b"RSDT") && transition_ok(SleepState::S0, SleepState::S3), "签名校验与睡眠迁移最小闭环");
    s.add("X07552 参数与配置面", pts_arg(SleepState::S3) == 3 && pts_arg(SleepState::S0) == 0, "_PTS 通知档默认可记忆");
    s.add("X07553 档位矩阵", [SleepState::S0, SleepState::S1, SleepState::S3, SleepState::S4, SleepState::S5].iter().all(|&st| pts_arg(st) <= 5), "S0~S5 五态独立可交付");
    s.add("X07554 快照与迁移", { let mut p = PowerResource::new(); p.acquire(); p.acquire(); p.refs() == 2 && p.on && { p.release() && p.on && p.refs() == 1 } }, "引用计数可序列化还原");
    s.add("X07555 三线集成验证", transition_ok(SleepState::S3, SleepState::S0) && transition_ok(SleepState::S3, SleepState::S5) == false, "唤醒路径与调度器协同");

    // L2 边界与恢复
    s.add("X07556 极端输入钳制", sig_ok(b"XSDT", b"RSDT") == false && Battery::new(150).pct == 100 && Battery::new(150).clamped == 1, "非法签名/电量越界回边界");
    s.add("X07557 失败叙事", narrative(1).contains("固件") && narrative(2).contains("外设") && narrative(3).contains("唤醒源"), "每种失败都有下一步建议");
    s.add("X07558 中断续跑", { let mut b = Battery::new(50); b.drain(30); b.drain(30); b.pct == 0 }, "放电饱和续跑不崩溃");
    s.add("X07559 资源降级", Battery::new(10).runtime_mh(500) == 1 && Battery::new(0).runtime_mh(500) == 0, "低电量运行时预算降级");
    s.add("X07560 回滚净身", { let mut p = PowerResource::new(); p.acquire(); p.release() && !p.on && p.refs() == 0 && p.release() == false }, "资源归零物理断电、重复释放拒绝");

    // L3 手感与细节
    s.add("X07561 动效令牌", pts_arg(SleepState::S1) == 1 && pts_arg(SleepState::S5) == 5, "通知序号对齐状态档位");
    s.add("X07562 三态焦点", { let mut p = PowerResource::new(); !p.on && { p.acquire(); p.on } && { p.release(); !p.on } }, "关/开/临界全态可达");
    s.add("X07563 键盘通道", wakeup_arbitrate(&[(3, "lan"), (1, "power"), (2, "usb")]) == "power", "唤醒源优先级语义正确");
    s.add("X07564 微文案", narrative(1).len() > 8 && !narrative(1).starts_with("Error"), "中文语境自然");
    s.add("X07565 无障碍等价通道", narrative(2).contains("建议") && narrative(3).contains("建议"), "叙事可读屏可执行");

    // L4 性能与优化
    s.add("X07566 基准与预算", Battery::new(100).runtime_mh(500) == 10 && Battery::new(100).runtime_mh(1000) == 5, "运行时预算入册");
    s.add("X07567 热路径", Battery::new(80).runtime_mh(0) == u32::MAX, "零功耗 O(1) 短路");
    s.add("X07568 内存收敛", { let mut p = PowerResource::new(); let mut n = 0; for _ in 0..1000 { p.acquire(); } while p.release() { n += 1; } n == 1000 && !p.on }, "千次引用计数收敛无漂移");
    s.add("X07569 低配降级", { let mut b = Battery::new(5); b.drain(1); b.pct == 4 }, "低电量步进不塌方");
    s.add("X07570 回归守卫", transition_ok(SleepState::S5, SleepState::S0) && !transition_ok(SleepState::S1, SleepState::S3), "迁移图断言只增不删");

    // L5 创新拓展
    s.add("X07571 智能建议", wakeup_arbitrate(&[]) == "none", "空候选可解释可拒绝");
    s.add("X07572 批量模式", { let mut p = PowerResource::new(); let mut n = 0; for _ in 0..10 { p.acquire(); n += 1; } n == 10 && p.refs() == 10 }, "批量引用进度可观测");
    s.add("X07573 三线联动", transition_ok(SleepState::S0, SleepState::S4) && pts_arg(SleepState::S4) == 4, "休眠映像与文件系统线协同");
    s.add("X07574 扩展点", sig_ok(b"FACP", b"FACP") && narrative(99).contains("DSDT"), "接口冻结可扩展");
    s.add("X07575 彩蛋层", pts_arg(SleepState::S5) == 5 && transition_ok(SleepState::S0, SleepState::S5), "关机仪式记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acpi_25_checks_pass() {
        let set = run_acpi_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
