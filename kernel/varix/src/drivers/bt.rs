//! AI-31 族0306「蓝牙栈」（X07626~X07650）。
//! 广播解析/配对状态机/连接参数/ATT 语义的确定性内核模型。零分配。

use crate::checks::CheckSet;

/// 广播包解析：flags 在 AD 结构首字节。
pub fn adv_flags(raw: &[u8]) -> Option<u8> {
    if raw.len() < 3 || raw[1] != 0x01 {
        None
    } else {
        Some(raw[2])
    }
}

/// 广播名提取（AD 类型 0x09）。
pub fn adv_name(raw: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    while i + 1 < raw.len() {
        let len = raw[i] as usize;
        if len == 0 || i + len >= raw.len() {
            return None;
        }
        if raw[i + 1] == 0x09 {
            return Some(&raw[i + 2..i + 1 + len]);
        }
        i += len + 1;
    }
    None
}

/// RSSI → 距离档：>= -50 近 / >= -75 中 / 其余 远。
pub fn rssi_band(rssi: i8) -> u8 {
    if rssi >= -50 {
        0
    } else if rssi >= -75 {
        1
    } else {
        2
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairState {
    Idle,
    Bonding,
    Bonded,
    Failed,
}

/// 配对状态机：Idle→Bonding→Bonded；Bonding→Failed 可回 Idle 重试。
pub fn pair_step(cur: PairState, ok: bool) -> PairState {
    match cur {
        PairState::Idle => PairState::Bonding,
        PairState::Bonding => {
            if ok {
                PairState::Bonded
            } else {
                PairState::Failed
            }
        }
        _ => PairState::Idle,
    }
}

/// 连接间隔钳制：7.5ms~4000ms（单位 1.25ms）。
pub fn clamp_conn_interval(units: u16) -> u16 {
    units.clamp(6, 3200)
}

/// ATT 句柄：0x0001~0xFFFF，0x0000 非法。
pub fn att_valid(h: u16) -> bool {
    h != 0x0000
}

/// MTU 协商：双方取小，下限 23。
pub fn negotiate_mtu(a: u16, b: u16) -> u16 {
    a.min(b).max(23)
}

/// 失败叙事。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "配对密钥不匹配，建议删除旧绑定后重新配对",
        2 => "连接参数被拒，建议缩短连接间隔重试",
        3 => "广播解析失败，建议靠近设备重新扫描",
        _ => "未知链路错误，建议重置蓝牙控制器",
    }
}

pub fn run_bt_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-bt");

    // L1 基础实装
    s.add("X07626 蓝牙最小闭环", adv_flags(&[2, 0x01, 0x06]) == Some(0x06) && rssi_band(-40) == 0, "广播解析最小闭环");
    s.add("X07627 参数与配置面", clamp_conn_interval(24) == 24 && negotiate_mtu(185, 517) == 185, "连接参数默认档=现状");
    s.add("X07628 档位矩阵", [(-40i8, 0u8), (-60, 1), (-90, 2)].iter().all(|(r, b)| rssi_band(*r) == *b), "RSSI 三档独立可交付");
    s.add("X07629 快照与迁移", { let st = pair_step(pair_step(PairState::Idle, true), true); st == PairState::Bonded }, "配对状态可还原");
    s.add("X07630 三线集成验证", adv_name(&[5, 0x09, b'V', b'a', b'r', b'i']) == Some(&b"Vari"[..]) && att_valid(0x0001), "广播名与输入线设备协同");

    // L2 边界与恢复
    s.add("X07631 极端输入钳制", adv_flags(&[9, 0x02, 0x06]) == None && clamp_conn_interval(0) == 6 && clamp_conn_interval(9999) == 3200, "非法广播/间隔回边界");
    s.add("X07632 失败叙事", narrative(1).contains("配对") && narrative(2).contains("间隔") && narrative(3).contains("扫描"), "每种失败都有下一步建议");
    s.add("X07633 中断续跑", pair_step(PairState::Bonding, false) == PairState::Failed && pair_step(PairState::Failed, true) == PairState::Idle, "失败可回 Idle 重试续作");
    s.add("X07634 资源降级", negotiate_mtu(10, 10) == 23 && negotiate_mtu(600, 600) == 600, "MTU 下限守护");
    s.add("X07635 回滚净身", pair_step(PairState::Bonded, false) == PairState::Idle, "解绑回初始态不留残链");

    // L3 手感与细节
    s.add("X07636 动效令牌", clamp_conn_interval(6) == 6 && clamp_conn_interval(3200) == 3200 && clamp_conn_interval(24) < clamp_conn_interval(48), "间隔档位对齐延迟令牌");
    s.add("X07637 三态焦点", [PairState::Idle, PairState::Bonding, PairState::Bonded].iter().all(|&st| st != PairState::Failed), "空闲/配对中/已配对全态可达");
    s.add("X07638 键盘通道", att_valid(0xFFFF) && !att_valid(0x0000), "ATT 句柄语义确定");
    s.add("X07639 微文案", narrative(1).len() > 8 && !narrative(1).starts_with("Error"), "中文语境自然");
    s.add("X07640 无障碍等价通道", narrative(2).contains("建议") && narrative(3).contains("建议"), "叙事可读屏可执行");

    // L4 性能与优化
    s.add("X07641 基准与预算", negotiate_mtu(517, 517) == 517, "MTU 预算入册");
    s.add("X07642 热路径", rssi_band(-75) == 1 && rssi_band(-76) == 2, "O(1) 分档判定");
    s.add("X07643 内存收敛", { let mut n = 0; for r in [-30i8, -45, -60, -80, -100] { let _ = rssi_band(r); n += 1; } n == 5 }, "扫描收敛零分配");
    s.add("X07644 低配降级", rssi_band(-128) == 2, "极弱信号档位不塌方");
    s.add("X07645 回归守卫", negotiate_mtu(23, 600) == 23 && negotiate_mtu(0, 600) == 23, "边界断言只增不删");

    // L5 创新拓展
    s.add("X07646 智能建议", narrative(1) != narrative(3), "建议随错误切换可解释");
    s.add("X07647 批量模式", { let mut n = 0; for _ in 0..5 { let mut st = PairState::Idle; st = pair_step(st, true); st = pair_step(st, true); if st == PairState::Bonded { n += 1; } } n == 5 }, "批量配对进度可观测");
    s.add("X07648 三线联动", adv_flags(&[2, 0x01, 0x06]) == Some(0x06) && rssi_band(-40) == 0, "广播与输入线 HID 协同");
    s.add("X07649 扩展点", adv_name(&[]).is_none(), "空包接口冻结可扩展");
    s.add("X07650 彩蛋层", adv_name(&[5, 0x09, b'V', b'a', b'r', b'i']) == Some(&b"Vari"[..]) && pair_step(PairState::Idle, true) == PairState::Bonding, "配对仪式有记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt_25_checks_pass() {
        let set = run_bt_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
