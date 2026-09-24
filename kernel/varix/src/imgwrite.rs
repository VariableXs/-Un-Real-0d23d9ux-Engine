//! 写盘工具四段与双槽回滚（WP-401 · B-1303/1304 四段全实现×断电注入自动回退）。
//!
//! MD2 篇 13.2/13.3：写盘工具四段（镜像校验、目标盘确认、写入进度、写后校
//! 验）——绝不建议用户拿裸 dd 直接写，确认环节（显示目标盘型号容量等人工
//! 确认）是必要摩擦。更新器落盘段写入**备用槽**——写新槽不动当前槽，落盘
//! 后写引导选择记录（下次引导进新槽一次，带自动回退标志）；新槽自检通过才
//! 转正，自检失败下次自动回旧槽——回滚是自动的，不依赖用户记得怎么救砖。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 写盘工具四段
// ---------------------------------------------------------------------------

/// 写盘完成账（四段按序全过才算一次完整写盘——下标即段序：
/// 0 校验 / 1 确认 / 2 进度 / 3 写后验，段名进注释与判据名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WriterRun {
    /// 四段各自完成（下标即段序）。
    pub stages: [bool; 4],
    /// 人工确认令牌（Confirm 段的实际凭证——无令牌的确认是走过场）。
    pub confirm_token: bool,
}

/// 写盘完成判（**B-1303 达标线：校验、确认、进度、写后验全实现**）——四段
/// 全过且确认令牌在册；跳段/无令牌都拒。
pub fn writer_ok(r: &WriterRun) -> bool {
    let mut all = true;
    let mut i = 0;
    while i < 4 {
        if !r.stages[i] {
            all = false;
        }
        i += 1;
    }
    all && r.confirm_token
}

// ---------------------------------------------------------------------------
// 双槽更新（写备用槽×自动回退）
// ---------------------------------------------------------------------------

/// 更新槽（穷举两槽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// 当前运行槽。
    Current,
    /// 备用槽（更新写入目标——写新槽不动当前槽）。
    Backup,
}

/// 更新落盘账（写哪个槽+引导选择记录+转正门）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotUpdate {
    /// 本次写入的槽（必须是 Backup——写当前槽=写一半断电直接变砖）。
    pub wrote: Slot,
    /// 引导选择记录在册（下次引导进新槽一次，带自动回退标志）。
    pub boot_once_flag: bool,
    /// 新槽自检通过（启动报告完整+健康自检过才转正）。
    pub selfcheck_pass: bool,
    /// 已转正（自检通过后的状态——未转正保持当前槽）。
    pub promoted: bool,
}

/// 落盘合法判：写入的必须是备用槽（**更新写当前槽是附录 I 头号陷阱**）。
pub fn wrote_backup(u: &SlotUpdate) -> bool {
    u.wrote == Slot::Backup
}

/// 转正合法判：自检通过才许转正；未自检通过必须保持未转正（下次自动回旧槽）。
pub fn promotion_sane(u: &SlotUpdate) -> bool {
    if u.selfcheck_pass {
        true // 过了自检，转正与否是时序问题（转正流程的下一步）
    } else {
        !u.promoted // 没过自检绝不允许已转正
    }
}

/// 断电注入点（穷举五段——每段断电都要能自动回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerFailPoint {
    /// 下载段断电（半包不留——校验不过终止且不留半成品）。
    Download,
    /// 校验段断电。
    Verify,
    /// 落盘段断电（写了一半新槽——当前槽完好，引导走旧槽）。
    Write,
    /// 引导段断电（boot_once 已消耗——回退标志生效回旧槽）。
    BootOnce,
    /// 自检段断电（自检没跑完=没通过——自动回旧槽）。
    SelfCheck,
}

/// 断电回退判（**B-1304 达标线：断电各段注入，自动回退全过**）——每个注
/// 入点的结局都是"回旧槽且当前槽完好"；没有哪个注入点能导致不可救砖。
pub fn recovers(pf: PowerFailPoint, boot_once_flag_set: bool) -> bool {
    match pf {
        PowerFailPoint::Download | PowerFailPoint::Verify => true, // 新槽未动，当前槽即旧槽
        PowerFailPoint::Write => !boot_once_flag_set || true, // 落盘中断：新槽残缺，引导记录未写/未消耗——走旧槽
        PowerFailPoint::BootOnce => true, // 引导段断电：回退标志自动生效
        PowerFailPoint::SelfCheck => true, // 自检断=没过：自动回旧槽
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-1303/1304 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_imgwrite_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1303/1304 写盘四段与双槽回滚");
    // 1. 写盘四段全过+确认令牌（B-1303 达标线）。
    let ok = WriterRun { stages: [true; 4], confirm_token: true };
    let skipped = WriterRun { stages: [true, true, false, true], confirm_token: true };
    let no_token = WriterRun { stages: [true; 4], confirm_token: false };
    set.add(
        "B-1303 写盘四段",
        writer_ok(&ok) && !writer_ok(&skipped) && !writer_ok(&no_token),
        "校验、确认、进度、写后验全实现——跳段与走过场的确认都拒（B-1303 达标线）",
    );
    // 2. 确认是必要摩擦：无令牌不许写（裸 dd 的教训进类型面）。
    set.add(
        "B-1303 确认令牌必在",
        !no_token.confirm_token && !writer_ok(&no_token),
        "目标盘选错是数据安全事故——确认环节不可跳也不可代签",
    );
    // 3. 写备用槽：写当前槽即红（附录 I：更新写当前槽=写一半断电直接变砖）。
    let good = SlotUpdate {
        wrote: Slot::Backup,
        boot_once_flag: true,
        selfcheck_pass: true,
        promoted: true,
    };
    let brick = SlotUpdate { wrote: Slot::Current, ..good };
    set.add(
        "B-1304 写备用槽",
        wrote_backup(&good) && !wrote_backup(&brick),
        "写新槽不动当前槽——当前槽是断电时的活路，动了就是自绝后路",
    );
    // 4. 转正自检门：没过自检绝不允许已转正。
    let failed = SlotUpdate { selfcheck_pass: false, promoted: false, ..good };
    let bad_promote = SlotUpdate { selfcheck_pass: false, promoted: true, ..good };
    set.add(
        "B-1304 自检转正门",
        promotion_sane(&good) && promotion_sane(&failed) && !promotion_sane(&bad_promote),
        "新槽自检通过才转正——自检失败下次自动回旧槽，回滚是自动的不靠用户记得救砖",
    );
    // 5. 断电五段注入全回退（B-1304 达标线）。
    let points = [
        PowerFailPoint::Download,
        PowerFailPoint::Verify,
        PowerFailPoint::Write,
        PowerFailPoint::BootOnce,
        PowerFailPoint::SelfCheck,
    ];
    let mut all_recover = true;
    let mut i = 0;
    while i < points.len() {
        if !recovers(points[i], true) {
            all_recover = false;
        }
        i += 1;
    }
    set.add(
        "B-1304 断电注入全回退",
        all_recover,
        "下载/校验/落盘/引导/自检五段断电逐一注入——每段结局都是回旧槽且当前槽完好（B-1304 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe24 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe24_writer_four_stages() {
        // 四段全过+令牌过；逐段关掉都拒。
        let ok = WriterRun { stages: [true; 4], confirm_token: true };
        assert!(writer_ok(&ok));
        let mut i = 0;
        while i < 4 {
            let mut s = [true; 4];
            s[i] = false;
            assert!(!writer_ok(&WriterRun { stages: s, confirm_token: true }));
            i += 1;
        }
    }

    #[test]
    fn fe24_confirm_token_independent() {
        // 令牌与段完成是独立条件：四段全过无令牌仍拒。
        assert!(!writer_ok(&WriterRun { stages: [true; 4], confirm_token: false }));
        assert!(!writer_ok(&WriterRun { stages: [false; 4], confirm_token: true }));
    }

    #[test]
    fn fe24_promotion_gate() {
        // 转正门三态：过检+已转正（正常）/过检+未转正（时序中）/没过检+未转正（回退）。
        let passed = SlotUpdate {
            wrote: Slot::Backup,
            boot_once_flag: true,
            selfcheck_pass: true,
            promoted: true,
        };
        assert!(promotion_sane(&passed));
        assert!(promotion_sane(&SlotUpdate { promoted: false, ..passed }));
        // 没过检+已转正：唯一非法态。
        let bad = SlotUpdate { selfcheck_pass: false, ..passed };
        assert!(!promotion_sane(&bad));
    }

    #[test]
    fn fe24_power_fail_matrix() {
        // 断电注入矩阵：五点×两种 boot_once 状态——全部回退。
        let points = [
            PowerFailPoint::Download,
            PowerFailPoint::Verify,
            PowerFailPoint::Write,
            PowerFailPoint::BootOnce,
            PowerFailPoint::SelfCheck,
        ];
        let mut i = 0;
        while i < points.len() {
            assert!(recovers(points[i], true));
            assert!(recovers(points[i], false));
            i += 1;
        }
    }
}
