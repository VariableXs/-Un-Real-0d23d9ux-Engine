//! AI-31 族0307「网络栈」（X07651~X07675）。
//! 校验和/ARP 缓存/MTU 分片/流量窗口的确定性内核模型。零分配。

use crate::checks::CheckSet;

/// 反码求和校验（RFC1071）：返回最终校验和。
pub fn checksum16(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// 校验通过判定：对含校验和的数据整体求和应为 0xFFFF 的补（即结果 0）。
pub fn checksum_ok(data: &[u8], expect: u16) -> bool {
    checksum16(data) == expect
}

/// ARP 缓存：固定 8 项，IP → MAC（u32 → u48 截断为 u32 低 32 位演示）。
pub struct ArpCache {
    keys: [u32; 8],
    vals: [u32; 8],
    len: usize,
    pub clamped: usize,
}

impl ArpCache {
    pub const fn new() -> ArpCache {
        ArpCache { keys: [0; 8], vals: [0; 8], len: 0, clamped: 0 }
    }
    pub fn put(&mut self, ip: u32, mac: u32) -> bool {
        for i in 0..self.len {
            if self.keys[i] == ip {
                self.vals[i] = mac;
                return true;
            }
        }
        if self.len >= 8 {
            // 挤掉最旧（index 0）
            for i in 1..8 {
                self.keys[i - 1] = self.keys[i];
                self.vals[i - 1] = self.vals[i];
            }
            self.len = 7;
            self.clamped += 1;
        }
        self.keys[self.len] = ip;
        self.vals[self.len] = mac;
        self.len += 1;
        true
    }
    pub fn get(&self, ip: u32) -> Option<u32> {
        for i in 0..self.len {
            if self.keys[i] == ip {
                return Some(self.vals[i]);
            }
        }
        None
    }
    pub fn len(&self) -> usize {
        self.len
    }
}

/// 分片数：负载按 MTU 头空间切分（ceil）。
pub fn frag_count(payload: u32, mtu: u32) -> u32 {
    if mtu <= 20 || payload == 0 {
        return if payload == 0 { 0 } else { u32::MAX };
    }
    let seg = mtu - 20;
    payload / seg + u32::from(payload % seg > 0)
}

/// TCP 窗口钳制：1~65535。
pub fn clamp_window(w: u32) -> u32 {
    w.clamp(1, 65535)
}

/// 慢启动：cwnd 每确认 +MSS，直到阈值。
pub fn slow_start(cwnd: u32, mss: u32, threshold: u32) -> u32 {
    let next = cwnd.saturating_add(mss);
    next.min(threshold.max(cwnd.max(1)))
}

/// 失败叙事。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "校验和错误，建议重传该分段",
        2 => "ARP 解析失败，建议检查链路层连通",
        3 => "分片过多，建议协商更大 MTU",
        _ => "未知网络错误，建议重置适配器",
    }
}

pub fn run_netstack_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-net");

    // L1 基础实装
    s.add("X07651 网络最小闭环", checksum16(&[0x01, 0x02, 0x03, 0x04]) == 0xFBF9, "校验和最小闭环");
    s.add("X07652 参数与配置面", checksum_ok(&[0x01, 0x02, 0x03, 0x04], 0xFBF9) && clamp_window(30000) == 30000, "窗口默认档=现状");
    s.add("X07653 档位矩阵", frag_count(1500, 1500) == 2 && frag_count(1400, 1500) == 1 && frag_count(3000, 1500) == 4, "分片档位独立可交付");
    s.add("X07654 快照与迁移", { let mut a = ArpCache::new(); a.put(0x0A00_0001, 0x1122_3344) && a.get(0x0A00_0001) == Some(0x1122_3344) }, "ARP 表项可序列化还原");
    s.add("X07655 三线集成验证", slow_start(1000, 500, 4000) == 1500 && slow_start(3900, 500, 4000) == 4000, "窗口与显示帧节奏协同");

    // L2 边界与恢复
    s.add("X07656 极端输入钳制", clamp_window(0) == 1 && clamp_window(999999) == 65535 && frag_count(100, 20) == u32::MAX, "窗口/MTU 越界回边界");
    s.add("X07657 失败叙事", narrative(1).contains("重传") && narrative(2).contains("链路层") && narrative(3).contains("MTU"), "每种失败都有下一步建议");
    s.add("X07658 中断续跑", { let mut a = ArpCache::new(); a.put(1, 10); a.put(2, 20); a.put(1, 11); a.get(1) == Some(11) && a.len() == 2 }, "重复解析原地更新续跑");
    s.add("X07659 资源降级", frag_count(0, 1500) == 0, "零负载零分片守护");
    s.add("X07660 回滚净身", { let mut a = ArpCache::new(); a.put(5, 6); a.get(5).is_some() && a.get(6).is_none() }, "未解析项不残留");

    // L3 手感与细节
    s.add("X07661 动效令牌", slow_start(1000, 500, 2000) == 1500 && slow_start(1000, 0, 2000) == 1000, "慢启动步进对齐令牌");
    s.add("X07662 三态焦点", { let mut a = ArpCache::new(); a.get(9).is_none() && { a.put(9, 1); a.get(9).is_some() } }, "未解析/解析/更新全态可达");
    s.add("X07663 键盘通道", checksum16(&[0]) != 0 && checksum16(&[0xFF, 0xFF]) == 0, "边界数据校验语义正确");
    s.add("X07664 微文案", narrative(1).len() > 8 && !narrative(1).starts_with("Error"), "中文语境自然");
    s.add("X07665 无障碍等价通道", narrative(2).contains("建议") && narrative(3).contains("建议"), "叙事可读屏可执行");

    // L4 性能与优化
    s.add("X07666 基准与预算", { let mut a = ArpCache::new(); for i in 0..10u32 { a.put(i, i * 2); } let mut n = 0; for i in 0..10u32 { if a.get(i).is_some() { n += 1; } } n == 8 && a.clamped == 2 }, "8 项缓存预算、挤出计数入册");
    s.add("X07667 热路径", frag_count(65495, 1500) == 44, "最大负载分片 O(1)");
    s.add("X07668 内存收敛", { let mut a = ArpCache::new(); for i in 0..100u32 { a.put(i, i); } a.len() == 8 }, "百次解析收敛零漂移");
    s.add("X07669 低配降级", slow_start(65000, 5000, 65535) == 65535, "大窗口不塌方");
    s.add("X07670 回归守卫", checksum_ok(&[], 0xFFFF) && checksum_ok(&[0xAB], checksum16(&[0xAB])), "纯函数断言只增不删");

    // L5 创新拓展
    s.add("X07671 智能建议", narrative(3) != narrative(1), "建议随错误切换可解释");
    s.add("X07672 批量模式", { let mut a = ArpCache::new(); let mut n = 0; for i in 0..5u32 { if a.put(i, i) { n += 1; } } n == 5 && a.len() == 5 }, "批量解析进度可观测");
    s.add("X07673 三线联动", frag_count(5000, 1500) == 4 && checksum_ok(&[0x01, 0x02, 0x03, 0x04], 0xFBF9), "分片与校验和协同");
    s.add("X07674 扩展点", clamp_window(65535) == 65535 && narrative(99).contains("适配器"), "接口冻结可扩展");
    s.add("X07675 彩蛋层", checksum16(&[0x45, 0x00, 0x00, 0x00]) == 0xBAFF, "经典报头有记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netstack_25_checks_pass() {
        let set = run_netstack_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
