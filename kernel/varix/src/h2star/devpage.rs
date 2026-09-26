//! F290 外设状态页 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：清单实时性（插拔 <2s 反映）；电量显示（有则显示
//! 无则不占位）；诊断人话输出用例；改名持久化。
//!
//! **设计要点（主册）**：设置中心「设备」页一屏看全：已连接外设清单
//! （键鼠/耳机/存储/显示，各自电量若有、连接状态、最后活跃时间），离线
//! 设备灰显不消失（保留记录便于排查）；每设备详情页给三动作（改名/
//! 解绑/诊断——诊断输出人话）。
//!
//! 实装：设备表（插拔事件即时反映——无延迟队列）；电量可选（有则显示
//! 无则不占位——Option 语义）；诊断器（按信号质量输出人话）；改名
//! （持久化快照可逆）；离线灰显（不删除记录）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 设备类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Headset,
    Storage,
    Display,
}

/// 一台外设。
#[derive(Clone, Debug)]
pub struct Device {
    pub kind: DeviceKind,
    pub name: String,
    pub online: bool,
    /// 电量百分比（无电量计为 None——「无则不占位」）。
    pub battery: Option<u8>,
    /// 最后活跃（分钟戳注入）。
    pub last_active_min: u64,
    /// 信号质量 0-100（诊断输入）。
    pub link_quality: u8,
}

impl Device {
    /// 电量渲染：有则显示、无则不占位（None → 空串）。
    pub fn battery_text(&self) -> String {
        match self.battery {
            Some(p) => alloc::format!("{}%", p),
            None => String::new(),
        }
    }

    /// 诊断输出（人话——判据原文口径：「设备响应正常」或「连接不稳，
    /// 试试换接口」级）。
    pub fn diagnose(&self) -> &'static str {
        if !self.online {
            return "设备当前离线——检查电源或重新配对";
        }
        match self.link_quality {
            80..=100 => "设备响应正常",
            50..=79 => "连接尚可——偶有延迟属正常范围",
            _ => "连接不稳，试试换接口",
        }
    }
}

/// 设备页服务。
pub struct DevicePage {
    devices: Vec<Device>,
}

impl DevicePage {
    pub fn new() -> DevicePage {
        DevicePage { devices: Vec::new() }
    }

    /// 插入（即时反映——列表立即更新）。
    pub fn plug(&mut self, d: Device) {
        self.devices.push(d);
    }

    /// 拔出：离线灰显**不删除**（保留记录便于排查——判据）。
    pub fn unplug(&mut self, name: &str) -> bool {
        match self.devices.iter_mut().find(|d| d.name == name) {
            Some(d) => {
                d.online = false;
                true
            }
            None => false,
        }
    }

    /// 清单实时性：插拔立即反映（本调用即终态——无 2s 延迟队列）。
    pub fn list(&self) -> &[Device] {
        &self.devices
    }

    /// 改名（持久化——快照可逆）。
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        match self.devices.iter_mut().find(|d| d.name == old) {
            Some(d) => {
                d.name = String::from(new);
                true
            }
            None => false,
        }
    }

    /// 解绑：从清单彻底移除（与拔出区分——用户显式动作）。
    pub fn unbind(&mut self, name: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.name != name);
        self.devices.len() != before
    }

    /// 持久化快照。
    pub fn snapshot(&self) -> Vec<Device> {
        self.devices.clone()
    }

    pub fn restore(&mut self, snap: Vec<Device>) {
        self.devices = snap;
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn mouse() -> Device {
    Device {
        kind: DeviceKind::Mouse,
        name: String::from("无线鼠标"),
        online: true,
        battery: Some(76),
        last_active_min: 100,
        link_quality: 90,
    }
}

pub fn run_devpage_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F290");
    let mut page = DevicePage::new();
    // 清单实时性：插入立即可见。
    page.plug(mouse());
    page.plug(Device {
        kind: DeviceKind::Headset,
        name: String::from("有线耳机"),
        online: true,
        battery: None,
        last_active_min: 101,
        link_quality: 60,
    });
    set.add("F290 instant list", page.list().len() == 2, "no delay queue");
    // 电量显示：有则显示无则不占位。
    set.add(
        "F290 battery optional",
        page.list()[0].battery_text() == "76%" && page.list()[1].battery_text().is_empty(),
        "None = empty",
    );
    // 诊断人话输出。
    set.add(
        "F290 diagnose human",
        page.list()[0].diagnose() == "设备响应正常"
            && page.list()[1].diagnose() == "连接尚可——偶有延迟属正常范围",
        "quality tiers",
    );
    // 离线灰显不消失。
    let unplugged = page.unplug("有线耳机");
    set.add(
        "F290 offline kept",
        unplugged && page.list().len() == 2 && !page.list()[1].online,
        "grey not gone",
    );
    // 改名持久化。
    let _ = page.rename("无线鼠标", "办公鼠");
    let snap = page.snapshot();
    let mut page2 = DevicePage::new();
    page2.restore(snap);
    set.add(
        "F290 rename persists",
        page2.list()[0].name == "办公鼠",
        "round-trip",
    );
    // 解绑（彻底移除）与离线的区别。
    let unbound = page2.unbind("有线耳机");
    set.add(
        "F290 unbind removes",
        unbound && page2.list().len() == 1,
        "explicit action",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f290_device_page() {
        let set = run_devpage_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F290 自检红 {f}/{p}");
    }

    #[test]
    fn offline_diagnosis_honest() {
        let mut d = mouse();
        d.online = false;
        assert!(d.diagnose().contains("离线"), "离线设备诊断不装在线");
    }
}
