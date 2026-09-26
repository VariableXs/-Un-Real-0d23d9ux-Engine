//! 星图安装服务与更新前端收口（WP-404 · B-3201~3204 · 篇 32）。
//!
//! 四段原子性（解析→校验→落盘→登记，半安装零呈现——**B-3201 达标线**）；
//! 升级先装后切旧版本保留（回滚按钮全通——**B-3202 达标线**）；卸载引用
//! 计数未解引用拒卸并指名（SC-133——**B-3203 达标线**）；更新五态与交接
//! 互斥（交接期更新按钮禁用——**B-3204 达标线**）。
//! 铁律：原子性不是"多数时候成功"，是"要么全段完成要么呈现面零痕迹"。

// ---------------------------------------------------------------------------
// B-3201 安装四段原子性
// ---------------------------------------------------------------------------

/// 安装四段（篇 32.1：解析/校验/落盘/登记——段序是合同，乱序即拒）。
pub const INSTALL_STAGES: usize = 4;
pub const STAGE_PARSE: usize = 0;
pub const STAGE_VERIFY: usize = 1;
pub const STAGE_LAND: usize = 2;
pub const STAGE_REGISTER: usize = 3;

/// 安装流水线：段序机 + 呈现面分离。
/// land（落盘）失败时 presented 恒 false——半安装对用户不存在。
#[derive(Clone, Copy)]
pub struct InstallPipeline {
    stage_done: [bool; INSTALL_STAGES],
    stage_ok: [bool; INSTALL_STAGES],
    stage_count: usize,
    registered: bool,
}

impl InstallPipeline {
    pub fn new() -> Self {
        InstallPipeline {
            stage_done: [false; INSTALL_STAGES],
            stage_ok: [false; INSTALL_STAGES],
            stage_count: 0,
            registered: false,
        }
    }

    /// 按序执行一段：idx 必须==已完成数（乱序即拒），ok=false 记失败但停在原段。
    pub fn run_stage(&mut self, idx: usize, ok: bool) -> bool {
        if idx != self.stage_count || idx >= INSTALL_STAGES {
            return false;
        }
        self.stage_done[idx] = true;
        self.stage_ok[idx] = ok;
        if ok {
            self.stage_count += 1;
            if idx == STAGE_REGISTER {
                self.registered = true;
            }
        }
        ok
    }

    /// 段完成数（进度条四段的分子）。
    pub fn stage_count(&self) -> usize {
        self.stage_count
    }

    /// 呈现面：登记段完成才算安装成功——半安装零呈现（**B-3201 核心**）。
    pub fn presented(&self) -> bool {
        self.registered
    }

    /// 全段成功且序号连续（审计用：跳段安装不存在）。
    pub fn atomic_ok(&self) -> bool {
        self.registered
            && self.stage_ok.iter().all(|&o| o)
            && self.stage_count == INSTALL_STAGES
    }
}

// ---------------------------------------------------------------------------
// B-3202 升级回切：新版本目录先装后切，旧版本保留一版
// ---------------------------------------------------------------------------

/// 双槽升级位（篇 32.1：新版本目录先装后切——active 指当前激活槽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpgradeSlots {
    /// 0=旧版本激活 1=新版本激活（先装后切：切之前 new_landed 必须为真）。
    pub active: u8,
    pub old_present: bool,
    pub new_landed: bool,
}

impl UpgradeSlots {
    pub fn fresh(old_present: bool) -> Self {
        UpgradeSlots { active: 0, old_present, new_landed: false }
    }

    /// 新版本落盘完成才允许切换（先装后切——装不是切，切不是装）。
    pub fn switch_to_new(&mut self) -> bool {
        if self.new_landed {
            self.active = 1;
        }
        self.new_landed
    }

    /// 回滚按钮：旧版本目录在册才可回切（保留一版的语义）。
    pub fn rollback(&mut self) -> bool {
        if self.old_present {
            self.active = 0;
        }
        self.old_present
    }

    pub fn on_new(&self) -> bool {
        self.active == 1
    }
}

// ---------------------------------------------------------------------------
// B-3203 卸载引用检查：未解引用拒卸并指名（SC-133）
// ---------------------------------------------------------------------------

/// 引用面（MIME 表与自启清单——清零才允许卸载）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefTable {
    pub mime_refs: u32,
    pub autostart_refs: u32,
}

/// 拒卸原因：指名是哪个引用面没清——"不能卸"不是可行动信息，"谁还引用着"才是。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UninstallDeny {
    MimeBusy,
    AutostartBusy,
}

/// 卸载裁决：引用清零放行，未清指名拒卸。
pub fn uninstall_check(rt: &RefTable) -> Result<(), UninstallDeny> {
    if rt.mime_refs > 0 {
        return Err(UninstallDeny::MimeBusy);
    }
    if rt.autostart_refs > 0 {
        return Err(UninstallDeny::AutostartBusy);
    }
    Ok(())
}

/// 解引用（逆操作按登记清单：先解引用再卸载）。
pub fn dereference(rt: &mut RefTable, mime: bool, autostart: bool) {
    if mime && rt.mime_refs > 0 {
        rt.mime_refs -= 1;
    }
    if autostart && rt.autostart_refs > 0 {
        rt.autostart_refs -= 1;
    }
}

// ---------------------------------------------------------------------------
// B-3204 更新五态与交接互斥
// ---------------------------------------------------------------------------

/// 更新器五态（篇 32.2：检查→提示→下载→落盘→完成；0=空闲）。
pub const UPD_STATES: usize = 5;
pub const UPD_IDLE: u8 = 0;
pub const UPD_CHECK: u8 = 1;
pub const UPD_PROMPT: u8 = 2;
pub const UPD_DOWNLOAD: u8 = 3;
pub const UPD_LAND: u8 = 4;
pub const UPD_DONE: u8 = 5;

/// 更新状态机：态序推进 + 交接会话锁（两台状态机同时动引导配置是自造事故）。
#[derive(Clone, Copy)]
pub struct UpdMachine {
    state: u8,
    /// 交接流程占用会话锁时为真——更新按钮禁用（**B-3204 核心**）。
    pub handover_lock: bool,
}

impl UpdMachine {
    pub fn new() -> Self {
        UpdMachine { state: UPD_IDLE, handover_lock: false }
    }

    /// 按序推进：只能从当前态到下一态（乱序即拒）。
    pub fn advance(&mut self) -> bool {
        if self.handover_lock || self.state >= UPD_DONE {
            return false;
        }
        self.state += 1;
        true
    }

    /// 交接锁占用即禁用——无论状态机在哪一态。
    pub fn update_allowed(&self) -> bool {
        !self.handover_lock && self.state <= UPD_CHECK
    }

    pub fn state(&self) -> u8 {
        self.state
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3201~3204 · 10 项）
// ---------------------------------------------------------------------------

/// 安装更新收口判据（WP-404）。
pub fn run_instup_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("instup");
    // 1. 四段段序穷举：乱序即拒、顺序全过（安装段序是合同）。
    let mut p1 = InstallPipeline::new();
    let ooo = !p1.run_stage(STAGE_VERIFY, true); // 跳过解析段——拒
    let mut p2 = InstallPipeline::new();
    let seq = p2.run_stage(STAGE_PARSE, true)
        && p2.run_stage(STAGE_VERIFY, true)
        && p2.run_stage(STAGE_LAND, true)
        && p2.run_stage(STAGE_REGISTER, true)
        && p2.atomic_ok();
    cs.add("B-3201 段序机", ooo && seq, "四段乱序即拒，顺序全过才算安装");
    // 2. 半安装零呈现（**B-3201 达标线**）：land 失败呈现面恒关。
    let mut p3 = InstallPipeline::new();
    let _ = p3.run_stage(STAGE_PARSE, true);
    let _ = p3.run_stage(STAGE_VERIFY, true);
    let land_fail = !p3.run_stage(STAGE_LAND, false);
    cs.add(
        "B-3201 半安装零呈现",
        land_fail && !p3.presented(),
        "落盘失败即停在原地——用户看不到半截安装",
    );
    // 3. 原子性成立判据：全段成功才 presented（**B-3201 达标线**）。
    cs.add(
        "B-3201 呈现面分离",
        p2.presented() && !p3.presented(),
        "呈现开关只挂登记段——四段完成与呈现是同一时刻",
    );
    // 4. 先装后切：new_landed 假时切拒（装与切是两步不是一步）。
    let mut u1 = UpgradeSlots::fresh(true);
    let cut_early = !u1.switch_to_new() && !u1.on_new();
    cs.add("B-3202 先装后切", cut_early, "新版本没落盘就切——拒");
    // 5. 回滚按钮全通（**B-3202 达标线**）：切新→回切旧，旧版本在册。
    let mut u2 = UpgradeSlots::fresh(true);
    u2.new_landed = true;
    let full = u2.switch_to_new() && u2.on_new() && u2.rollback() && !u2.on_new();
    cs.add("B-3202 升级回切", full, "切新成功、回旧成功——旧版本保留一版不是口号");
    // 6. 无旧版本回滚拒：old_present 假时回滚失败（首次安装无回滚对象）。
    let mut u3 = UpgradeSlots::fresh(false);
    u3.new_landed = true;
    let _ = u3.switch_to_new();
    cs.add(
        "B-3202 回滚对象在册",
        !u3.rollback(),
        "旧版本目录不在册——回滚按钮置灰而不是假装成功",
    );
    // 7. 引用清零才卸 + 未清指名拒（**B-3203 达标线**）。
    let busy = RefTable { mime_refs: 1, autostart_refs: 0 };
    let busy2 = RefTable { mime_refs: 0, autostart_refs: 2 };
    let clean = RefTable { mime_refs: 0, autostart_refs: 0 };
    let d1 = uninstall_check(&busy);
    let d2 = uninstall_check(&busy2);
    cs.add(
        "B-3203 拒卸指名",
        d1 == Err(UninstallDeny::MimeBusy)
            && d2 == Err(UninstallDeny::AutostartBusy)
            && uninstall_check(&clean).is_ok(),
        "MIME 与自启两引用面分别指名——不是笼统的失败",
    );
    // 8. 解引用后放行（逆操作按登记清单）。
    let mut rt = RefTable { mime_refs: 1, autostart_refs: 1 };
    dereference(&mut rt, true, true);
    cs.add(
        "B-3203 解引用放行",
        uninstall_check(&rt).is_ok(),
        "先解引用再卸载——SC-133 的实现纪律",
    );
    // 9. 五态序贯 + 交接互斥（**B-3204 达标线**）。
    let mut m1 = UpdMachine::new();
    let mut seq_ok = true;
    for _ in 0..UPD_STATES {
        seq_ok &= m1.advance();
    }
    let over = !m1.advance(); // 完成态再推进——拒
    cs.add("B-3204 五态序贯", seq_ok && over && m1.state() == UPD_DONE, "检查提示下载落盘完成——乱序与越态不存在");
    // 10. 交接期更新按钮禁用：锁占用时推进与准入全拒。
    let mut m2 = UpdMachine::new();
    m2.handover_lock = true;
    cs.add(
        "B-3204 交接互斥",
        !m2.advance() && !m2.update_allowed(),
        "交接进行中更新禁用——两台状态机同时动引导配置是自造事故",
    );
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe31 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe31_install_atomic() {
        // 四段全过=呈现；任一段败=零呈现；乱序=拒（原子性三面）。
        let mut ok = InstallPipeline::new();
        for i in 0..INSTALL_STAGES {
            assert!(ok.run_stage(i, true));
        }
        assert!(ok.presented() && ok.atomic_ok());
        let mut half = InstallPipeline::new();
        let _ = half.run_stage(STAGE_PARSE, true);
        let _ = half.run_stage(STAGE_VERIFY, true);
        assert!(!half.run_stage(STAGE_LAND, false));
        assert!(!half.run_stage(STAGE_REGISTER, true)); // 败段之后不许续跑
        assert!(!half.presented());
        let mut jump = InstallPipeline::new();
        assert!(!jump.run_stage(STAGE_REGISTER, true)); // 直接登记段——拒
        assert_eq!(jump.stage_count(), 0);
    }

    #[test]
    fn fe31_upgrade_rollback() {
        // 先装后切+回切全链；切换幂等性：已在新槽再切仍是新槽。
        let mut u = UpgradeSlots::fresh(true);
        assert!(!u.switch_to_new());
        u.new_landed = true;
        assert!(u.switch_to_new() && u.on_new());
        assert!(u.switch_to_new() && u.on_new()); // 幂等
        assert!(u.rollback() && !u.on_new());
        assert!(u.rollback()); // 回滚幂等
    }

    #[test]
    fn fe31_uninstall_refcheck() {
        // 引用对账：两引用面独立清零，解引用不越零（下溢不存在）。
        let mut rt = RefTable { mime_refs: 2, autostart_refs: 1 };
        dereference(&mut rt, true, true);
        dereference(&mut rt, true, true);
        dereference(&mut rt, true, true); // mime 已零——不再减
        assert_eq!(rt.mime_refs, 0);
        assert_eq!(rt.autostart_refs, 0);
        assert!(uninstall_check(&rt).is_ok());
        let busy = RefTable { mime_refs: 3, autostart_refs: 3 };
        assert_eq!(uninstall_check(&busy), Err(UninstallDeny::MimeBusy));
    }

    #[test]
    fn fe31_update_mutex() {
        // 五态序贯+锁语义：锁在推进拒、锁清准入恢复、完成后锁不解也不许动。
        let mut m = UpdMachine::new();
        assert!(m.update_allowed());
        m.handover_lock = true;
        assert!(!m.advance());
        m.handover_lock = false;
        for _ in 0..UPD_STATES {
            assert!(m.advance());
        }
        assert_eq!(m.state(), UPD_DONE);
        assert!(!m.advance());
        assert!(!m.update_allowed()); // 完成态不再是准入态
    }
}
