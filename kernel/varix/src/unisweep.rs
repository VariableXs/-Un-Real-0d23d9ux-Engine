//! 卸载清理对账（WP-305 · B-2104 卸载无残留——SC-133 全绿）。
//!
//! MD2 篇 21.3：卸载前检查关联（MIME 表引用、自启注册），清理完整（SC-133
//! 的实现）。前端关联提示面在 starmapui（WP-206），本模块是后端的对账面：
//! 关联如实报告不静默清除、账本守恒（登记多少清多少）、越界乱清比残留
//! 更严重。SC-133 全绿 = 残留为零且账实相符。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 关联与账本（模型面）
// ---------------------------------------------------------------------------

/// 卸载前关联报告（MIME 表引用数 + 自启注册——如实报告，不静默清除）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Assoc {
    /// MIME 表引用条数。
    pub mime_refs: u8,
    /// 自启注册在册。
    pub autostart: bool,
}

/// 安装登记账本（装的时候登记了什么——卸载时按账本对账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 登记文件数。
    pub files: u16,
    /// 登记配置键数。
    pub keys: u16,
    /// 清理的 MIME 条数（与 Assoc.mime_refs 对账）。
    pub mime_cleared: u16,
    /// 清理的自启条数（与 Assoc.autostart 对账——0 或 1）。
    pub autostart_cleared: u16,
}

/// 清理裁决（SC-133 两态：全绿或残留计数——残留不许含糊）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SweepVerdict {
    Clean,
    Residual(u16),
}

/// 卸载清理对账（**B-2104 达标线：SC-133 全绿**）——三面合取：
/// 1. 账本守恒：登记==清理（漏清残留、越界乱清都不行）；
/// 2. 关联诚实：MIME 清理数==报告数、自启清理数==报告语义（报了清、没报不清）；
/// 3. 残留计数化：差多少留多少，不许含糊。
pub fn sweep(registered: &Ledger, cleared: &Ledger, assoc: &Assoc) -> SweepVerdict {
    // 账本守恒（files/keys 两面相等——多清少清都是不等）。
    if cleared.files != registered.files || cleared.keys != registered.keys {
        let mut resid: u16 = 0;
        resid += registered.files.saturating_sub(cleared.files);
        resid += cleared.files.saturating_sub(registered.files);
        resid += registered.keys.saturating_sub(cleared.keys);
        resid += cleared.keys.saturating_sub(registered.keys);
        return SweepVerdict::Residual(resid);
    }
    // 关联诚实：MIME 清理数==报告数。
    if cleared.mime_cleared != assoc.mime_refs as u16 {
        let d = if cleared.mime_cleared > assoc.mime_refs as u16 {
            cleared.mime_cleared - assoc.mime_refs as u16
        } else {
            assoc.mime_refs as u16 - cleared.mime_cleared
        };
        return SweepVerdict::Residual(d);
    }
    // 关联诚实：自启——报了在册就必须清 1，报了不在册就必须清 0。
    let expect: u16 = if assoc.autostart { 1 } else { 0 };
    if cleared.autostart_cleared != expect {
        return SweepVerdict::Residual(1);
    }
    SweepVerdict::Clean
}

// ---------------------------------------------------------------------------
// CheckSet（B-2104 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_unisweep_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2104 卸载无残留");
    // 1. 账本守恒：登记==清理，漏清即残留计数。
    let reg = Ledger { files: 24, keys: 5, mime_cleared: 0, autostart_cleared: 0 };
    let mut clean = reg;
    clean.mime_cleared = 2;
    clean.autostart_cleared = 1;
    let mut miss_one = clean;
    miss_one.files = 23;
    let assoc = Assoc { mime_refs: 2, autostart: true };
    set.add(
        "B-2104 账本守恒",
        matches!(sweep(&reg, &clean, &assoc), SweepVerdict::Clean)
            && matches!(sweep(&reg, &miss_one, &assoc), SweepVerdict::Residual(1)),
        "登记多少清多少——漏一件就有一件的残留计数，不许含糊",
    );
    // 2. 越界乱清拒：清得比登记多比残留更严重（清掉别人的东西）。
    let mut over = clean;
    over.files = 25;
    set.add(
        "B-2104 越界乱清拒",
        matches!(sweep(&reg, &over, &assoc), SweepVerdict::Residual(1)),
        "清理超出账本即红——越界乱清比残留更严重，两边都不许",
    );
    // 3. 关联诚实：MIME 清理数==报告数、自启报了清没报不清。
    let mut mime_bad = clean;
    mime_bad.mime_cleared = 3;
    let assoc_no_auto = Assoc { mime_refs: 2, autostart: false };
    set.add(
        "B-2104 关联诚实",
        matches!(sweep(&reg, &mime_bad, &assoc), SweepVerdict::Residual(1))
            && matches!(sweep(&reg, &clean, &assoc_no_auto), SweepVerdict::Residual(1)),
        "MIME 与自启的清理动作必须与关联报告一致——不静默清除",
    );
    // 4. SC-133 全绿（B-2104 达标线）：账实相符+关联诚实 → Clean。
    let assoc_auto = Assoc { mime_refs: 2, autostart: true };
    set.add(
        "B-2104 SC-133 全绿",
        matches!(sweep(&reg, &clean, &assoc_auto), SweepVerdict::Clean),
        "卸载无残留=账本守恒+关联诚实双关全过（B-2104 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe16 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn reg() -> Ledger {
        Ledger { files: 10, keys: 3, mime_cleared: 1, autostart_cleared: 0 }
    }

    fn cleared_ok() -> Ledger {
        Ledger { files: 10, keys: 3, mime_cleared: 1, autostart_cleared: 0 }
    }

    fn assoc() -> Assoc {
        Assoc { mime_refs: 1, autostart: false }
    }

    #[test]
    fn fe16_assoc_honest() {
        // MIME 清 2 报 1 / 清 0 报 1 都是不诚实。
        let mut m2 = cleared_ok();
        m2.mime_cleared = 2;
        assert!(matches!(sweep(&reg(), &m2, &assoc()), SweepVerdict::Residual(1)));
        let mut m0 = cleared_ok();
        m0.mime_cleared = 0;
        assert!(matches!(sweep(&reg(), &m0, &assoc()), SweepVerdict::Residual(1)));
        // 报告一致则过。
        assert!(matches!(sweep(&reg(), &cleared_ok(), &assoc()), SweepVerdict::Clean));
    }

    #[test]
    fn fe16_no_missing() {
        // 漏清：少一件文件、少一把键，各自计残留。
        let mut miss_f = cleared_ok();
        miss_f.files = 9;
        assert!(matches!(sweep(&reg(), &miss_f, &assoc()), SweepVerdict::Residual(1)));
        let mut miss_k = cleared_ok();
        miss_k.keys = 2;
        assert!(matches!(sweep(&reg(), &miss_k, &assoc()), SweepVerdict::Residual(1)));
        // 漏两件计两件。
        let mut miss_2 = cleared_ok();
        miss_2.files = 8;
        assert!(matches!(sweep(&reg(), &miss_2, &assoc()), SweepVerdict::Residual(2)));
    }

    #[test]
    fn fe16_no_overreach() {
        // 越界：多清文件/多清键都拒（差额计入残留方向计数）。
        let mut over_f = cleared_ok();
        over_f.files = 11;
        assert!(!matches!(sweep(&reg(), &over_f, &assoc()), SweepVerdict::Clean));
        let mut over_k = cleared_ok();
        over_k.keys = 4;
        assert!(!matches!(sweep(&reg(), &over_k, &assoc()), SweepVerdict::Clean));
    }

    #[test]
    fn fe16_autostart_semantics() {
        // 自启报了在册就必须清 1；报不在册就必须清 0——报行不一致即红。
        let a_on = Assoc { mime_refs: 1, autostart: true };
        let mut c1 = cleared_ok();
        c1.autostart_cleared = 1;
        assert!(matches!(sweep(&reg(), &c1, &a_on), SweepVerdict::Clean));
        assert!(!matches!(sweep(&reg(), &cleared_ok(), &a_on), SweepVerdict::Clean));
        let a_off = Assoc { mime_refs: 1, autostart: false };
        assert!(matches!(sweep(&reg(), &cleared_ok(), &a_off), SweepVerdict::Clean));
        let mut c2 = cleared_ok();
        c2.autostart_cleared = 1;
        assert!(!matches!(sweep(&reg(), &c2, &a_off), SweepVerdict::Clean));
    }
}
