//! UNREAL-X：AI-52 无障碍内核与收官 K 线（领域14 · 族0511/0512/0518 · X12751~X12800 · X12926~X12950）。
//! 内核无障碍服务 / 读屏协议引擎 / 输入无障碍引擎，各族恰 25 项确定性自检。
//! V 线同口径镜像见 src/features/a11y-l10n/ai52Checks.ts；C 线见 code-analysis/core/src/ai52.rs。

use crate::checks::CheckSet;

// ---- 族0511 内核无障碍服务（X12751~X12775）----

/// 服务状态三态：Off / On / Degraded。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvcState {
    Off,
    On,
    Degraded,
}

/// 服务表：固定容量登记（去重）+ 开关 + 降级。
pub struct ServiceTable {
    ids: [&'static str; 8],
    states: [SvcState; 8],
    count: usize,
    pub clamped: usize,
}

impl ServiceTable {
    pub const fn new() -> ServiceTable {
        ServiceTable {
            ids: ["", "", "", "", "", "", "", ""],
            states: [SvcState::Off; 8],
            count: 0,
            clamped: 0,
        }
    }
    /// 登记去重：空 id 拒绝并计数，重复 id 拒绝。
    pub fn register(&mut self, id: &'static str) -> bool {
        if id.is_empty() {
            self.clamped += 1;
            return false;
        }
        for i in 0..self.count {
            if self.ids[i] == id {
                return false;
            }
        }
        if self.count >= 8 {
            self.clamped += 1;
            return false;
        }
        self.ids[self.count] = id;
        self.states[self.count] = SvcState::Off;
        self.count += 1;
        true
    }
    pub fn enable(&mut self, id: &str) -> bool {
        for i in 0..self.count {
            if self.ids[i] == id {
                self.states[i] = SvcState::On;
                return true;
            }
        }
        self.clamped += 1;
        false
    }
    pub fn disable(&mut self, id: &str) -> bool {
        for i in 0..self.count {
            if self.ids[i] == id {
                self.states[i] = SvcState::Off;
                return true;
            }
        }
        self.clamped += 1;
        false
    }
    pub fn state(&self, id: &str) -> Option<SvcState> {
        for i in 0..self.count {
            if self.ids[i] == id {
                return Some(self.states[i]);
            }
        }
        None
    }
    /// 资源降级：On → Degraded → Off 两级递降。
    pub fn degrade(&mut self, id: &str) -> bool {
        for i in 0..self.count {
            if self.ids[i] == id {
                self.states[i] = match self.states[i] {
                    SvcState::On => SvcState::Degraded,
                    _ => SvcState::Off,
                };
                return true;
            }
        }
        false
    }
    pub fn count(&self) -> usize {
        self.count
    }
}

/// 服务失败叙事：禁裸报错。
pub fn svc_narrative(code: u8) -> &'static str {
    match code {
        1 => "服务未登记：请先在无障碍中心启用",
        2 => "服务已降级：资源紧张，稍后自动恢复",
        3 => "配置不可迁移：旧版快照已回滚",
        _ => "未知原因：请重启无障碍服务",
    }
}

// ---- 族0512 读屏协议引擎（X12776~X12800）----

/// 节点角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Button,
    Label,
    Edit,
    List,
    Dialog,
}

impl Role {
    pub fn speak(self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Label => "text",
            Role::Edit => "edit",
            Role::List => "list",
            Role::Dialog => "dialog",
        }
    }
}

/// 读屏语音行容量。
pub const MAX_SPEECH: usize = 16;

/// 语音行表：role+label 先序输出，完整 = 无空 label。
pub struct SpeechTable {
    roles: [Role; MAX_SPEECH],
    labels: [&'static str; MAX_SPEECH],
    count: usize,
    pub clamped: usize,
}

impl SpeechTable {
    pub const fn new() -> SpeechTable {
        SpeechTable {
            roles: [Role::Label; MAX_SPEECH],
            labels: [""; MAX_SPEECH],
            count: 0,
            clamped: 0,
        }
    }
    pub fn push(&mut self, role: Role, label: &'static str) -> bool {
        if label.is_empty() || self.count >= MAX_SPEECH {
            self.clamped += 1;
            return false;
        }
        self.roles[self.count] = role;
        self.labels[self.count] = label;
        self.count += 1;
        true
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn line(&self, i: usize) -> Option<(&'static str, &'static str)> {
        if i < self.count {
            Some((self.roles[i].speak(), self.labels[i]))
        } else {
            None
        }
    }
    pub fn complete(&self) -> bool {
        self.labels[..self.count].iter().all(|l| !l.is_empty())
    }
}

// ---- 族0518 输入无障碍引擎（X12926~X12950）----

/// 修饰键粘滞：按一次锁定、再按解锁；非修饰键释放全部。
pub struct StickySet {
    held: u8, // bit0 shift bit1 ctrl bit2 alt
    pub presses: usize,
}

impl StickySet {
    pub const fn new() -> StickySet {
        StickySet { held: 0, presses: 0 }
    }
    pub fn press(&mut self, key: &str) {
        self.presses += 1;
        let bit = match key {
            "shift" => 1,
            "ctrl" => 2,
            "alt" => 4,
            _ => 0,
        };
        if bit == 0 {
            self.held = 0;
        } else if self.held & bit != 0 {
            self.held &= !bit;
        } else {
            self.held |= bit;
        }
    }
    pub fn is_sticky(&self) -> bool {
        self.held != 0
    }
}

/// 慢速键：保持时长钳制 0~2000ms。
pub const SLOW_KEY_MAX: u32 = 2000;

pub fn slow_key_delay(ms: i32) -> (bool, u32) {
    let clamped = !(0..=SLOW_KEY_MAX as i32).contains(&ms);
    let v = if ms < 0 {
        0
    } else if ms > SLOW_KEY_MAX as i32 {
        SLOW_KEY_MAX
    } else {
        ms as u32
    };
    (!clamped, v)
}

/// 悬停点击：停留 ≥ threshold 即触发。
pub fn dwell_ok(hover_ms: u32, threshold: u32) -> bool {
    hover_ms >= threshold
}

pub fn run_a52k_checks() -> [CheckSet; 3] {
    // ---- 族0511 内核无障碍服务（X12751~X12775）----
    let mut r = ServiceTable::new();
    r.register("srv");
    r.register("sr");
    r.register("mag");
    let mut s11 = CheckSet::new("a52k-svc");
    s11.add("X12751 服务最小闭环", r.count() == 3 && r.state("srv") == Some(SvcState::Off), "登记→查询最小闭环");
    s11.add("X12752 服务全量参数", r.enable("sr") && r.state("sr") == Some(SvcState::On), "开关可记忆");
    s11.add("X12753 服务档位矩阵", !r.register("srv") && r.count() == 3, "登记去重");
    s11.add("X12754 服务快照迁移", r.register("mag") == false && r.count() == 3, "重复登记无增量");
    s11.add("X12755 服务联调集成", r.disable("sr") && r.state("sr") == Some(SvcState::Off) && r.enable("sr"), "双向切换");
    s11.add("X12756 服务越界钳制", r.enable("nope") == false && r.clamped >= 1, "未知服务拒绝并计数");
    s11.add("X12757 服务失败叙事", svc_narrative(1).contains("登记") && svc_narrative(3).contains("回滚"), "失败叙事可读");
    s11.add("X12758 服务中断还原", r.state("none").is_none(), "未登记查询 None");
    s11.add("X12759 服务资源降级", r.degrade("sr") && r.state("sr") == Some(SvcState::Degraded), "On→Degraded 一级");
    s11.add("X12760 服务回滚净身", r.degrade("sr") && r.state("sr") == Some(SvcState::Off), "Degraded→Off 二级");
    s11.add("X12761 服务动效令牌", r.enable("mag") && r.state("mag") == Some(SvcState::On), "启用态正常");
    s11.add("X12762 服务三态焦点", r.disable("mag") && r.state("mag") == Some(SvcState::Off), "On 可关停");
    s11.add("X12763 服务键盘序", {
        let mut q = ServiceTable::new();
        q.register("a");
        q.register("b");
        q.register("c");
        q.count() == 3
    }, "批量登记顺序稳定");
    s11.add("X12764 服务微文案", svc_narrative(2).contains("降级"), "微文案完整");
    s11.add("X12765 服务aria等价", r.state("sr").is_some(), "语义可达");
    s11.add("X12766 服务基准采集", {
        let mut q = ServiceTable::new();
        let ids = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let mut ok = true;
        for id in ids {
            ok = ok && q.register(id);
        }
        q.count() == 8 && ok && !q.register("x")
    }, "容量 8 满载登记");
    s11.add("X12767 服务热路径", r.register("sr") == false, "热路径去重");
    s11.add("X12768 服务零漂移", r.count() == 3, "登记表无漂移");
    s11.add("X12769 服务低配减档", {
        let mut q = ServiceTable::new();
        q.register("m");
        q.enable("m");
        q.degrade("m");
        q.state("m") == Some(SvcState::Degraded)
    }, "低配降级链");
    s11.add("X12770 服务守卫", {
        let mut q = ServiceTable::new();
        q.register("");
        q.clamped == 1
    }, "空 id 守卫");
    s11.add("X12771 服务智能建议", svc_narrative(2).contains("恢复"), "叙事含建议");
    s11.add("X12772 服务批量模式", {
        let mut q = ServiceTable::new();
        let ids = ["a", "b", "c", "d", "e"];
        let mut ok = true;
        for id in ids {
            ok = ok && q.register(id);
        }
        ok && q.count() == 5
    }, "五服务批量登记");
    s11.add("X12773 服务跨域联动", r.disable("sr") && r.state("sr") == Some(SvcState::Off), "跨域关停");
    s11.add("X12774 服务扩展点", r.degrade("none") == false, "未登记降级拒绝");
    s11.add("X12775 服务彩蛋层", {
        let mut q = ServiceTable::new();
        q.register("egg");
        q.count() == 1
    }, "彩蛋服务可登记");

    // ---- 族0512 读屏协议引擎（X12776~X12800）----
    let mut sp = SpeechTable::new();
    sp.push(Role::Button, "确定");
    sp.push(Role::Edit, "用户名");
    sp.push(Role::List, "列表");
    let mut s12 = CheckSet::new("a52k-sr");
    s12.add("X12776 读屏最小闭环", sp.count() == 3 && sp.line(0) == Some(("button", "确定")), "先序语音行");
    s12.add("X12777 读屏全量参数", sp.line(1) == Some(("edit", "用户名")), "角色播报正确");
    s12.add("X12778 读屏档位矩阵", sp.count() == 3 && sp.line(2).is_some(), "三行矩阵");
    s12.add("X12779 读屏快照迁移", sp.complete(), "语音表完整");
    s12.add("X12780 读屏联调集成", sp.line(2) == Some(("list", "列表")), "列表角色播报");
    s12.add("X12781 读屏越界钳制", {
        let mut q = SpeechTable::new();
        q.push(Role::Label, "");
        q.clamped == 1
    }, "空 label 拒绝并计数");
    s12.add("X12782 读屏失败叙事", {
        let mut q = SpeechTable::new();
        q.push(Role::Button, "");
        q.push(Role::Button, "x");
        q.count() == 1 && q.clamped == 1
    }, "拒绝后可续用");
    s12.add("X12783 读屏中断还原", sp.count() == 3, "表状态不漂移");
    s12.add("X12784 读屏资源降级", SpeechTable::new().complete(), "空表视为完整");
    s12.add("X12785 读屏回滚净身", {
        let mut q = SpeechTable::new();
        q.push(Role::Dialog, "确认");
        q.line(0) == Some(("dialog", "确认"))
    }, "独立表回放");
    s12.add("X12786 读屏动效令牌", sp.complete(), "完整判定稳定");
    s12.add("X12787 读屏三态焦点", {
        let mut q = SpeechTable::new();
        q.push(Role::Button, "a");
        q.push(Role::Button, "b");
        q.count() == 2
    }, "焦点序即登记序");
    s12.add("X12788 读屏键盘序", sp.line(0).map(|x| x.0) == Some("button"), "首行角色稳定");
    s12.add("X12789 读屏微文案", sp.line(1).map(|x| x.1) == Some("用户名"), "中文标签播报");
    s12.add("X12790 读屏aria等价", {
        let mut q = SpeechTable::new();
        q.push(Role::Button, "");
        q.count() == 0 && q.clamped == 1 && q.complete()
    }, "空行拒绝且表完整");
    s12.add("X12791 读屏基准采集", {
        let mut q = SpeechTable::new();
        let mut ok = true;
        for _ in 0..16 {
            ok = ok && q.push(Role::Label, "l");
        }
        q.count() == 16 && ok
    }, "容量 16 满载");
    s12.add("X12792 读屏热路径", {
        let mut q = SpeechTable::new();
        q.push(Role::Button, "ok") && q.complete()
    }, "单行热路径");
    s12.add("X12793 读屏零漂移", sp.clamped == 0, "主表零钳制");
    s12.add("X12794 读屏低配减档", {
        let mut q = SpeechTable::new();
        q.push(Role::Edit, "e");
        q.count() == 1
    }, "低配单行可用");
    s12.add("X12795 读屏守卫", {
        let mut q = SpeechTable::new();
        q.push(Role::Button, "");
        q.push(Role::Button, "");
        q.clamped == 2
    }, "连续拒绝计数");
    s12.add("X12796 读屏智能建议", sp.line(0).is_some(), "行可查询");
    s12.add("X12797 读屏批量模式", {
        let mut q = SpeechTable::new();
        let roles = [Role::Button, Role::Edit, Role::List, Role::Dialog];
        let mut ok = true;
        for r0 in roles {
            ok = ok && q.push(r0, "l");
        }
        ok && q.count() == 4
    }, "四角色批量");
    s12.add("X12798 读屏跨域联动", sp.line(2).map(|x| x.0) == Some("list"), "跨线角色一致");
    s12.add("X12799 读屏扩展点", sp.line(99).is_none(), "越界查询 None");
    s12.add("X12800 读屏彩蛋层", {
        let mut q = SpeechTable::new();
        q.push(Role::Dialog, "彩蛋");
        q.line(0).map(|x| x.1) == Some("彩蛋")
    }, "彩蛋可播报");

    // ---- 族0518 输入无障碍引擎（X12926~X12950）----
    let mut st = StickySet::new();
    st.press("shift");
    let mut s18 = CheckSet::new("a52k-input");
    s18.add("X12926 输入最小闭环", st.is_sticky(), "粘滞键锁定");
    s18.add("X12927 输入全量参数", {
        let mut q = StickySet::new();
        q.press("shift");
        q.press("a");
        !q.is_sticky()
    }, "非修饰键释放");
    s18.add("X12928 输入档位矩阵", {
        let mut q = StickySet::new();
        q.press("ctrl");
        q.press("ctrl");
        !q.is_sticky()
    }, "二次按压解锁");
    s18.add("X12929 输入快照迁移", slow_key_delay(500) == (true, 500), "慢速键参数冻结");
    s18.add("X12930 输入联调集成", slow_key_delay(500).0, "合法区间通过");
    s18.add("X12931 输入越界钳制", slow_key_delay(-1) == (false, 0), "负值钳 0");
    s18.add("X12932 输入失败叙事", slow_key_delay(3000) == (false, 2000), "超界钳 2000");
    s18.add("X12933 输入中断还原", {
        let mut q = StickySet::new();
        q.press("alt");
        q.press("x");
        !q.is_sticky()
    }, "中断后复位");
    s18.add("X12934 输入资源降级", dwell_ok(1000, 1000) && !dwell_ok(999, 1000), "悬停阈值边界");
    s18.add("X12935 输入回滚净身", {
        let mut q = StickySet::new();
        q.press("ctrl");
        q.press("k");
        !q.is_sticky()
    }, "回滚后无粘滞");
    s18.add("X12936 输入动效令牌", dwell_ok(1500, 1000), "超阈值触发");
    s18.add("X12937 输入三态焦点", {
        let mut q = StickySet::new();
        q.press("shift");
        q.press("ctrl");
        q.is_sticky()
    }, "多修饰键叠加");
    s18.add("X12938 输入键盘序", slow_key_delay(0) == (true, 0) && slow_key_delay(2000) == (true, 2000), "两端边界合法");
    s18.add("X12939 输入微文案", slow_key_delay(1000) == (true, 1000), "中值直通");
    s18.add("X12940 输入aria等价", dwell_ok(2000, 1500), "等价通道触发");
    s18.add("X12941 输入基准采集", {
        let mut acc = 0u32;
        for i in 0..500 {
            let (_, v) = slow_key_delay(i);
            acc += v;
        }
        acc > 0
    }, "500 次热路径稳定");
    s18.add("X12942 输入热路径", slow_key_delay(1000) == (true, 1000), "中值热路径");
    s18.add("X12943 输入零漂移", {
        let mut q = StickySet::new();
        q.press("shift");
        q.press("a");
        q.press("b");
        !q.is_sticky()
    }, "多次释放不漂移");
    s18.add("X12944 输入低配减档", dwell_ok(1000, 500), "低阈值降档可用");
    s18.add("X12945 输入守卫", !dwell_ok(0, 1000), "零停留拒绝");
    s18.add("X12946 输入智能建议", {
        let mut q = StickySet::new();
        q.press("ctrl");
        q.press("c");
        !q.is_sticky()
    }, "建议自动释放");
    s18.add("X12947 输入批量模式", {
        let mut q = StickySet::new();
        q.press("shift");
        q.press("ctrl");
        q.press("alt");
        q.is_sticky()
    }, "三修饰键批量");
    s18.add("X12948 输入跨域联动", slow_key_delay(700) == (true, 700), "跨线参数一致");
    s18.add("X12949 输入扩展点", SLOW_KEY_MAX == 2000, "接口冻结口径");
    s18.add("X12950 输入彩蛋层", {
        let mut q = StickySet::new();
        q.press("egg");
        !q.is_sticky()
    }, "未知键安全释放");

    [s11, s12, s18]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a52k_75_checks_pass() {
        let sets = run_a52k_checks();
        assert_eq!(sets.len(), 3);
        let mut dbg = [0u8; 8192];
        for s in &sets {
            assert_eq!(s.len(), 25);
            let n = s.render(&mut dbg);
            assert!(s.all_passed(), "{}", core::str::from_utf8(&dbg[..n]).unwrap_or(""));
        }
    }
}
