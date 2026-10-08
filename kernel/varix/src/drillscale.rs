//! QEMU 演练量级与实机脚本纪律（WP-401 · B-1203/1204 十倍量级×白名单与双人规则）。
//!
//! MD2 篇 12.2/12.3：QEMU 快照机制让"断电"变成廉价操作——杀死演练因此可
//! 以跑出实机十倍的量级，实机只做最终抽测。实机自动化脚本继承 Windows 加
//! 固包四条纪律（遗言、装载纪律、写后必读、幂等）加两条实机专属：破坏性操
//! 作白名单（脚本只允许对白名单路径与配置动手）与双人规则（影响面清单在脚
//! 本头部声明，执行前输出待执行清单等确认）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 演练量级（QEMU ≥ 实机十倍）
// ---------------------------------------------------------------------------

/// 演练场景账（一场景一本账——QEMU 量与实机量分开记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrillLedger {
    /// 场景名（如 "power-cut"/"wineserver-kill"）。
    pub scene: &'static str,
    /// QEMU 侧已跑次数。
    pub qemu_runs: u32,
    /// 实机侧已跑（或计划抽测）次数。
    pub real_runs: u32,
}

/// 量级判（**B-1203 达标线：各场景 ≥ 实机十倍量**）——实机基数必须 ≥1
/// （没跑过实机的场景没有倍数可谈），QEMU 量至少十倍。
pub fn scale_ok(l: &DrillLedger) -> bool {
    l.real_runs >= 1 && l.qemu_runs >= l.real_runs * 10
}

// ---------------------------------------------------------------------------
// 实机脚本纪律（四条继承 + 白名单 + 双人规则）
// ---------------------------------------------------------------------------

/// 实机脚本纪律面（六要素合取）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RealScript {
    /// 遗言（异常退出留现场）。
    pub last_words: bool,
    /// 装载纪律（依赖显式声明）。
    pub load_discipline: bool,
    /// 写后必读（写完读回校验）。
    pub write_readback: bool,
    /// 幂等（重复执行不产生二次破坏）。
    pub idempotent: bool,
    /// 破坏性操作白名单在册（脚本只允许动白名单内路径与配置）。
    pub whitelist: bool,
    /// 双人规则在册（影响面清单头部声明+执行前输出待执行清单等确认）。
    pub two_person: bool,
}

/// 脚本审计判（**B-1204 达标线：白名单与双人规则审计通过**）——继承四条
/// 与实机两条全合取，缺一即红。
pub fn script_audit(s: &RealScript) -> bool {
    s.last_words && s.load_discipline && s.write_readback && s.idempotent && s.whitelist && s.two_person
}

/// 目标路径许可判：白名单精确匹配才许动手（前缀包含也不行——
/// "/data/tmp" 不许碰 "/data/tmp2"）。
pub fn path_allowed(whitelisted: &str, target: &str) -> bool {
    whitelisted == target
}

// ---------------------------------------------------------------------------
// CheckSet（B-1203/1204 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_drillscale_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1203/1204 演练量级与实机纪律");
    // 1. 十倍量级：实机基数 1、QEMU 十倍——恰好达标；不足十倍拒。
    let edge = DrillLedger { scene: "power-cut", qemu_runs: 100, real_runs: 10 };
    let short = DrillLedger { scene: "power-cut", qemu_runs: 99, real_runs: 10 };
    set.add(
        "B-1203 十倍量级",
        scale_ok(&edge) && !scale_ok(&short),
        "各场景 ≥ 实机十倍量——快照机制让断电廉价，量级是倍数不是感觉（B-1203 达标线）",
    );
    // 2. 零实机基数拒：没跑过实机的场景没有倍数可谈。
    let no_real = DrillLedger { scene: "usb-kill", qemu_runs: 1000, real_runs: 0 };
    set.add(
        "B-1203 实机基数门槛",
        !scale_ok(&no_real),
        "实机基数为 0 时倍数无意义——先有实机抽测基数，再谈十倍覆盖",
    );
    // 3. 脚本六要素合取（B-1204 达标线）：四条继承+两条实机专属。
    let full = RealScript {
        last_words: true,
        load_discipline: true,
        write_readback: true,
        idempotent: true,
        whitelist: true,
        two_person: true,
    };
    let no_confirm = RealScript { two_person: false, ..full };
    set.add(
        "B-1204 脚本纪律审计",
        script_audit(&full) && !script_audit(&no_confirm),
        "遗言/装载/写后必读/幂等+白名单+双人规则——六缺一即红（B-1204 达标线）",
    );
    // 4. 白名单精确匹配：前缀包含也拒（"顺手清理"是事故之源）。
    set.add(
        "B-1204 白名单精确匹配",
        path_allowed("/data/varix-test", "/data/varix-test")
            && !path_allowed("/data/varix-test", "/data/varix-test2")
            && !path_allowed("/data/varix-test", "/data"),
        "只允许对白名单路径动手——路径相等是唯一许可，前缀包含也不行",
    );
    // 5. 多场景对账：逐场景过倍数判，一场景不足全批红。
    let batch = [
        DrillLedger { scene: "power-cut", qemu_runs: 200, real_runs: 20 },
        DrillLedger { scene: "wineserver-kill", qemu_runs: 50, real_runs: 5 },
        DrillLedger { scene: "disk-pull", qemu_runs: 30, real_runs: 3 },
    ];
    let mut all_scaled = true;
    let mut i = 0;
    while i < batch.len() {
        if !scale_ok(&batch[i]) {
            all_scaled = false;
        }
        i += 1;
    }
    let bad_batch = [
        batch[0],
        batch[1],
        DrillLedger { scene: "disk-pull", qemu_runs: 29, real_runs: 3 },
    ];
    let mut any_short = false;
    i = 0;
    while i < bad_batch.len() {
        if !scale_ok(&bad_batch[i]) {
            any_short = true;
        }
        i += 1;
    }
    set.add(
        "B-1203 多场景逐账对账",
        all_scaled && any_short,
        "断电/杀 wineserver/拔盘逐场景过判——一场景量级不足全批红",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe21 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe21_scale_math() {
        // 先算再断：10 实机 × 10 = 100 QEMU 恰好达标；9 倍拒；11 倍过。
        assert!(scale_ok(&DrillLedger { scene: "a", qemu_runs: 100, real_runs: 10 }));
        assert!(!scale_ok(&DrillLedger { scene: "a", qemu_runs: 90, real_runs: 10 }));
        assert!(scale_ok(&DrillLedger { scene: "a", qemu_runs: 110, real_runs: 10 }));
        // 基数 1 的下界：10 次 QEMU 对 1 次实机恰好十倍。
        assert!(scale_ok(&DrillLedger { scene: "a", qemu_runs: 10, real_runs: 1 }));
        assert!(!scale_ok(&DrillLedger { scene: "a", qemu_runs: 9, real_runs: 1 }));
    }

    #[test]
    fn fe21_zero_real_base() {
        // 实机 0：无论 QEMU 多少都拒。
        assert!(!scale_ok(&DrillLedger { scene: "a", qemu_runs: 0, real_runs: 0 }));
        assert!(!scale_ok(&DrillLedger { scene: "a", qemu_runs: 999_999, real_runs: 0 }));
    }

    #[test]
    fn fe21_script_six_elements() {
        // 六要素逐项独立红：关掉任意一项审计即不过。
        let base = RealScript {
            last_words: true,
            load_discipline: true,
            write_readback: true,
            idempotent: true,
            whitelist: true,
            two_person: true,
        };
        assert!(script_audit(&base));
        let clones = [
            RealScript { last_words: false, ..base },
            RealScript { load_discipline: false, ..base },
            RealScript { write_readback: false, ..base },
            RealScript { idempotent: false, ..base },
            RealScript { whitelist: false, ..base },
            RealScript { two_person: false, ..base },
        ];
        let mut i = 0;
        while i < clones.len() {
            assert!(!script_audit(&clones[i]));
            i += 1;
        }
    }

    #[test]
    fn fe21_path_exact_match() {
        // 精确匹配语义：相等过、父路径拒、子路径拒、相似名拒。
        assert!(path_allowed("/esp/limine.conf", "/esp/limine.conf"));
        assert!(!path_allowed("/esp/limine.conf", "/esp"));
        assert!(!path_allowed("/esp/limine.conf", "/esp/limine.conf.bak"));
        assert!(!path_allowed("/esp", "/esp/limine.conf"));
    }
}
