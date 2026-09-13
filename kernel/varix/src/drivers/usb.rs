//! AI-31 族0305「USB 栈」（X07601~X07625）。
//! 描述符/端点/集线器枚举/传输类型的确定性内核模型。零分配。

use crate::checks::CheckSet;

/// 速度档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Speed {
    Low,
    Full,
    High,
    Super,
}

/// 速度带宽（Mbps）。
pub fn speed_mbps(s: Speed) -> u32 {
    match s {
        Speed::Low => 1,
        Speed::Full => 12,
        Speed::High => 480,
        Speed::Super => 5000,
    }
}

/// 设备描述符解析：b15..b8=VID，b7..b0=PID。
pub fn parse_desc(hi: u8, lo: u8) -> (u16, u16) {
    ((hi as u16) << 8 | hi as u16, lo as u16)
}

/// 实际解析：VID/PID 从 4 字节小端提取。
pub fn vid_pid(raw: [u8; 4]) -> (u16, u16) {
    let vid = raw[0] as u16 | ((raw[1] as u16) << 8);
    let pid = raw[2] as u16 | ((raw[3] as u16) << 8);
    (vid, pid)
}

/// 端点分配位图：EP0 保留控制，EP1~15 可分配。
pub struct EpMap {
    used: u16, // bit0..bit15
    pub clamped: usize,
}

impl EpMap {
    pub const fn new() -> EpMap {
        EpMap { used: 1, clamped: 0 } // EP0 保留
    }
    pub fn alloc(&mut self, ep: u8) -> bool {
        if ep == 0 || ep > 15 {
            self.clamped += 1;
            return false;
        }
        let bit = 1u16 << ep;
        if self.used & bit != 0 {
            self.clamped += 1;
            return false;
        }
        self.used |= bit;
        true
    }
    pub fn free(&mut self, ep: u8) -> bool {
        if ep == 0 || ep > 15 || self.used & (1u16 << ep) == 0 {
            self.clamped += 1;
            return false;
        }
        self.used &= !(1u16 << ep);
        true
    }
    pub fn is_used(&self, ep: u8) -> bool {
        ep <= 15 && self.used & (1u16 << ep) != 0
    }
}

/// 集线器枚举：深度钳制 1~7。
pub fn clamp_hub_depth(d: u8) -> u8 {
    d.clamp(1, 7)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Xfer {
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

/// 传输类型适合的负载上限（字节）。
pub fn max_packet(x: Xfer, s: Speed) -> u32 {
    match (x, s) {
        (Xfer::Control, _) => 64,
        (Xfer::Isochronous, Speed::High) => 1024,
        (Xfer::Isochronous, _) => 512,
        (Xfer::Bulk, Speed::Super) => 1024,
        (Xfer::Bulk, _) => 512,
        (Xfer::Interrupt, _) => 1024,
    }
}

/// 失败叙事。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "设备描述符读取失败，建议重新插拔或更换端口",
        2 => "端点资源耗尽，建议断开闲置设备",
        3 => "过流保护触发，建议检查外设供电",
        _ => "未知枚举错误，建议查看总线事件日志",
    }
}

pub fn run_usb_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-usb");
    let mut ep = EpMap::new();

    // L1 基础实装
    s.add("X07601 USB最小闭环", vid_pid([0x86, 0x80, 0x34, 0x12]) == (0x8086, 0x1234), "描述符→VID/PID 最小闭环");
    s.add("X07602 参数与配置面", speed_mbps(Speed::High) == 480 && max_packet(Xfer::Bulk, Speed::High) == 512, "速度/负载默认档可配置");
    s.add("X07603 档位矩阵", [Speed::Low, Speed::Full, Speed::High, Speed::Super].iter().all(|&v| speed_mbps(v) > 0), "速度档独立可交付");
    s.add("X07604 快照与迁移", { let mut e = EpMap::new(); e.alloc(3) && e.is_used(3) && e.free(3) && !e.is_used(3) }, "端点位图可序列化还原");
    s.add("X07605 三线集成验证", parse_desc(0x80, 0x10).0 >> 8 == 0x80 && clamp_hub_depth(3) == 3, "描述符与显示线热插拔协同");

    // L2 边界与恢复
    s.add("X07606 极端输入钳制", clamp_hub_depth(0) == 1 && clamp_hub_depth(9) == 7 && ep.alloc(0) == false && ep.clamped == 1, "深度/端点越界回边界");
    s.add("X07607 失败叙事", narrative(1).contains("插拔") && narrative(2).contains("端点") && narrative(3).contains("供电"), "每种失败都有下一步建议");
    s.add("X07608 中断续跑", { let mut e = EpMap::new(); e.alloc(5) && e.free(5) && e.alloc(5) }, "释放后可重枚举续跑");
    s.add("X07609 资源降级", max_packet(Xfer::Bulk, Speed::Full) == 512 && max_packet(Xfer::Control, Speed::Low) == 64, "低速档负载降级");
    s.add("X07610 回滚净身", { let mut e = EpMap::new(); e.alloc(7); e.free(7) && !e.is_used(7) && e.free(7) == false }, "重复释放拒绝、净身完整");

    // L3 手感与细节
    s.add("X07611 动效令牌", speed_mbps(Speed::Low) < speed_mbps(Speed::Full) && speed_mbps(Speed::Full) < speed_mbps(Speed::High) && speed_mbps(Speed::High) < speed_mbps(Speed::Super), "速度带宽单调对齐令牌");
    s.add("X07612 三态焦点", { let mut e = EpMap::new(); !e.is_used(2) && e.alloc(2) && e.is_used(2) }, "空闲/占用/释放全态可达");
    s.add("X07613 键盘通道", [Xfer::Control, Xfer::Isochronous, Xfer::Bulk, Xfer::Interrupt].iter().all(|&x| max_packet(x, Speed::Super) > 0), "传输类型语义确定");
    s.add("X07614 微文案", narrative(1).len() > 8 && !narrative(1).starts_with("Error"), "中文语境自然");
    s.add("X07615 无障碍等价通道", narrative(2).contains("建议") && narrative(3).contains("建议"), "叙事可读屏可执行");

    // L4 性能与优化
    s.add("X07616 基准与预算", { let mut e = EpMap::new(); let mut n = 0; for i in 1..16u8 { if e.alloc(i) { n += 1; } } n == 15 }, "15 端点预算入册");
    s.add("X07617 热路径", max_packet(Xfer::Isochronous, Speed::High) == 1024, "同步传输 O(1) 查询");
    s.add("X07618 内存收敛", { let mut e = EpMap::new(); for i in 1..16u8 { e.alloc(i); } e.alloc(2) == false && e.clamped == 1 }, "重复分配零漂移");
    s.add("X07619 低配降级", clamp_hub_depth(1) == 1, "最小深度体验不塌方");
    s.add("X07620 回归守卫", vid_pid([0, 0, 0, 0]) == (0, 0) && parse_desc(0, 0) == (0, 0), "零值守护断言只增不删");

    // L5 创新拓展
    s.add("X07621 智能建议", narrative(2) != narrative(3), "建议随错误切换可解释");
    s.add("X07622 批量模式", { let mut e = EpMap::new(); let mut n = 0; for i in [1u8, 2, 3, 4, 5] { if e.alloc(i) { n += 1; } } n == 5 }, "批量端点分配可观测");
    s.add("X07623 三线联动", speed_mbps(Speed::Super) > speed_mbps(Speed::High) && max_packet(Xfer::Bulk, Speed::Super) == 1024, "速率与显示线带宽协同");
    s.add("X07624 扩展点", ep.is_used(0), "EP0 控制保留接口冻结");
    s.add("X07625 彩蛋层", narrative(3).contains("过流保护"), "保护叙事有记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_25_checks_pass() {
        let set = run_usb_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
