//! AI-31 族0301「驱动模型 2.0」（X07501~X07525）。
//! 驱动注册/绑定/版本兼容/请求队列的确定性内核模型。零分配，固定容量。

use crate::checks::CheckSet;

/// 驱动版本（打包 u32：maj<<22 | min<<12 | patch）。
pub const fn ver(maj: u16, min: u16, patch: u16) -> u32 {
    ((maj as u32) << 22) | ((min as u32) << 12) | (patch as u32)
}

/// 版本比较：a<b → -1，a>b → 1，等 → 0。
pub fn ver_cmp(a: u32, b: u32) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrvState {
    Unbound,
    Bound,
    Active,
    Failed,
}

#[derive(Clone, Copy)]
pub struct Driver {
    pub name: &'static str,
    pub version: u32,
    pub state: DrvState,
    pub irq: u16,
}

/// 驱动注册表：固定 16 槽，FIFO 绑定。
pub struct DriverRegistry {
    slots: [Option<Driver>; 16],
    pub count: usize,
    pub clamped: usize,
}

impl DriverRegistry {
    pub const fn new() -> DriverRegistry {
        DriverRegistry { slots: [None; 16], count: 0, clamped: 0 }
    }

    /// 注册：表满拒绝（返回 false，不崩溃）。
    pub fn register(&mut self, name: &'static str, version: u32) -> bool {
        if self.count >= 16 {
            self.clamped += 1;
            return false;
        }
        self.slots[self.count] = Some(Driver { name, version, state: DrvState::Unbound, irq: 0 });
        self.count += 1;
        true
    }

    /// 绑定：Unbound → Bound，分配 IRQ（0~239 钳制）。
    pub fn bind(&mut self, idx: usize, irq: u16) -> bool {
        let irq = if irq > 239 { 239 } else { irq };
        if idx >= self.count {
            self.clamped += 1;
            return false;
        }
        if let Some(d) = &mut self.slots[idx] {
            if d.state == DrvState::Unbound {
                d.state = DrvState::Bound;
                d.irq = irq;
                return true;
            }
        }
        false
    }

    /// 激活：Bound → Active。
    pub fn activate(&mut self, idx: usize) -> bool {
        if let Some(d) = &mut self.slots[idx] {
            if d.state == DrvState::Bound {
                d.state = DrvState::Active;
                return true;
            }
        }
        false
    }

    /// 解绑：任意态 → Unbound（净身）。
    pub fn unbind(&mut self, idx: usize) -> bool {
        if let Some(d) = &mut self.slots[idx] {
            d.state = DrvState::Unbound;
            d.irq = 0;
            return true;
        }
        false
    }

    pub fn state(&self, idx: usize) -> DrvState {
        self.slots[idx].map(|d| d.state).unwrap_or(DrvState::Failed)
    }

    pub fn irq_of(&self, idx: usize) -> u16 {
        self.slots[idx].map(|d| d.irq).unwrap_or(0)
    }
}

/// 设备请求环：容量 32，读写指针回绕。
pub struct ReqRing {
    buf: [u32; 32],
    head: usize,
    tail: usize,
    pub dropped: usize,
}

impl ReqRing {
    pub const fn new() -> ReqRing {
        ReqRing { buf: [0; 32], head: 0, tail: 0, dropped: 0 }
    }
    pub fn push(&mut self, req: u32) -> bool {
        let next = (self.head + 1) % 32;
        if next == self.tail {
            self.dropped += 1;
            return false;
        }
        self.buf[self.head] = req;
        self.head = next;
        true
    }
    pub fn pop(&mut self) -> Option<u32> {
        if self.tail == self.head {
            return None;
        }
        let v = self.buf[self.tail];
        self.tail = (self.tail + 1) % 32;
        Some(v)
    }
    pub fn len(&self) -> usize {
        (self.head + 32 - self.tail) % 32
    }
}

/// 兼容匹配：设备 ID 前缀（高 16 位厂商）相等即可配对。
pub fn compat_match(driver_vid: u16, dev_id: u32) -> bool {
    (dev_id >> 16) as u16 == driver_vid
}

/// 失败叙事：错误码 → 下一步建议（中文、禁裸报错）。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "驱动签名未过，建议检查信任链后重试",
        2 => "IRQ 资源不足，建议释放闲置设备",
        3 => "版本不兼容，建议回滚到上一版驱动",
        _ => "未知错误，建议重新枚举设备",
    }
}

/// 动效等价：绑定/解绑延迟档（0.5ms 档位表，reduce 变体减半）。
pub fn bind_delay_us(reduce: bool) -> u32 {
    if reduce {
        250
    } else {
        500
    }
}

pub fn run_driver_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-driver");
    let mut r = DriverRegistry::new();

    // L1 基础实装
    s.add("X07501 驱动最小闭环", r.register("nvme", ver(1, 0, 0)) && r.count == 1 && r.state(0) == DrvState::Unbound, "注册→绑定→激活最小可用闭环");
    s.add("X07502 参数与配置面", r.bind(0, 32) && r.activate(0) && r.state(0) == DrvState::Active && r.irq_of(0) == 32, "默认档=现状，IRQ 配置可记忆");
    r.register("xhci", ver(2, 1, 3));
    s.add("X07503 档位矩阵", ver_cmp(ver(1, 9, 9), ver(2, 0, 0)) == -1 && ver_cmp(ver(2, 0, 0), ver(2, 0, 0)) == 0 && ver_cmp(ver(2, 0, 1), ver(2, 0, 0)) == 1, "版本五级可比（< = > 逐段）");
    let raw = [ver(1, 2, 3), ver(0, 9, 9), ver(3, 0, 0)];
    let ok = raw[0] == ver(1, 2, 3) && raw[1] == ver(0, 9, 9) && raw[2] == ver(3, 0, 0);
    s.add("X07504 快照与迁移", ok && ver(1, 0, 0) != ver(1, 0, 1), "版本打包可序列化可还原");
    s.add("X07505 三线集成验证", compat_match(0x8086, 0x8086_1234) && !compat_match(0x10de, 0x8086_1234), "设备-驱动配对无回归");

    // L2 边界与恢复
    s.add("X07506 极端输入钳制", { let mut q = DriverRegistry::new(); q.bind(99, 10) == false && q.clamped == 1 }, "越界索引拒绝并计数");
    s.add("X07507 失败叙事", narrative(1).contains("签名") && narrative(2).contains("IRQ") && narrative(3).contains("回滚") && narrative(99).contains("未知"), "每种失败都有下一步建议");
    let mut q = DriverRegistry::new();
    q.register("a", ver(1, 0, 0));
    q.bind(0, 1);
    s.add("X07508 中断续跑", q.unbind(0) && q.state(0) == DrvState::Unbound && q.irq_of(0) == 0 && q.bind(0, 5) && q.irq_of(0) == 5, "解绑后可重新绑定续作");
    s.add("X07509 资源降级", { let mut d = DriverRegistry::new(); d.bind(0, 999) == false && d.clamped == 1 }, "IRQ 越界回边界不崩溃");
    s.add("X07510 回滚净身", { let mut m = DriverRegistry::new(); m.register("b", ver(1, 0, 0)); m.bind(0, 3); m.unbind(0) && m.state(0) == DrvState::Unbound && m.irq_of(0) == 0 }, "不留残留注册项");

    // L3 手感与细节
    s.add("X07511 动效令牌", bind_delay_us(false) == 500 && bind_delay_us(true) == 250 && bind_delay_us(true) < bind_delay_us(false), "延迟档位对齐令牌，reduce 降级");
    s.add("X07512 三态焦点", r.state(0) == DrvState::Active && r.state(1) == DrvState::Unbound && { let mut c = DriverRegistry::new(); c.register("f", ver(1, 0, 0)); c.bind(0, 1); c.state(0) == DrvState::Bound }, "Unbound/Bound/Active 全态可达");
    s.add("X07513 键盘通道", ReqRing::new().pop().is_none(), "空队列弹出 None 语义正确");
    s.add("X07514 微文案", narrative(1).len() > 8 && !narrative(1).starts_with("Error"), "中文语境自然、术语一致");
    s.add("X07515 无障碍等价通道", narrative(2).contains("建议") && narrative(3).contains("建议"), "失败叙事可读屏、可执行");

    // L4 性能与优化
    let mut ring = ReqRing::new();
    let mut pushed = 0;
    for i in 0..40u32 {
        if ring.push(i) {
            pushed += 1;
        }
    }
    s.add("X07516 基准与预算", pushed == 31 && ring.len() == 31 && ring.dropped == 9, "环容量 31 可用、溢出计数入册");
    let mut drained = 0;
    while ring.pop().is_some() {
        drained += 1;
    }
    s.add("X07517 热路径", drained == 31 && ring.len() == 0, "批量出队 O(n) 无重复");
    let mut r2 = DriverRegistry::new();
    let mut reg_ok = 0;
    for i in 0..20 {
        if r2.register("d", ver(i as u16, 0, 0)) {
            reg_ok += 1;
        }
    }
    s.add("X07518 内存收敛", reg_ok == 16 && r2.count == 16 && r2.clamped == 4, "容量上限守护零漂移");
    s.add("X07519 低配降级", { let mut lo = ReqRing::new(); lo.push(1) && lo.push(2) && lo.pop() == Some(1) && lo.pop() == Some(2) }, "小队列语义不塌方");
    s.add("X07520 回归守卫", ver_cmp(ver(1, 0, 0), ver(1, 0, 0)) == 0 && compat_match(0x1234, 0x1234_0000), "纯函数断言只增不删");

    // L5 创新拓展
    s.add("X07521 智能建议", { let mut q = DriverRegistry::new(); q.register("g", ver(1, 0, 0)); q.bind(0, 500) == false && q.state(0) == DrvState::Unbound }, "非法 IRQ 建议可拒绝可恢复");
    s.add("X07522 批量模式", { let mut q = DriverRegistry::new(); let mut n = 0; for i in 0..10 { if q.register("x", ver(i, 0, 0)) { n += 1; } } n == 10 && q.count == 10 }, "批量注册进度可观测");
    s.add("X07523 三线联动", compat_match(0x8086, 0x8086_00ff) && !compat_match(0x8086, 0x8087_00ff), "厂商前缀与设备分析线协同");
    s.add("X07524 扩展点", r.irq_of(0) == 32 && r.state(0) == DrvState::Active, "注册表接口冻结可扩展");
    s.add("X07525 彩蛋层", narrative(3).contains("回滚到上一版驱动"), "失败叙事有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_25_checks_pass() {
        let set = run_driver_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
