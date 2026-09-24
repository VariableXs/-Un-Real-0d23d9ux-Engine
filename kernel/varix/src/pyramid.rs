//! 测试金字塔与冒烟套对齐（WP-401 · B-1201/1202 金字塔不倒挂×用例判据全对齐）。
//!
//! MD2 篇 12.1/12.2：四层结构（单元/集成/QEMU 系统层/实机），越往下跑得越
//! 快、越多、越早。层的铁律：**下层的 bug 不许漏到上层再发现**——集成层能
//! 测的报文语义问题在 QEMU 层才暴露，视为集成层用例缺口，补用例与修 bug
//! 同单完成。冒烟套与判据编号对齐：每个用例声明它守护的 B-xxx 或 WD-xxx，
//! 判据改版时用例缺口一目了然。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 四层金字塔（穷举）
// ---------------------------------------------------------------------------

/// 金字塔层（穷举四层——下标即层序：0 最底层，越下越快越多越早）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PyramidLayer {
    /// 第一层单元测试（随码随写，秒级）。
    Unit,
    /// 第二层集成测试（接口级，宿主机模拟桩，秒级）。
    Integration,
    /// 第三层系统测试（QEMU 整机镜像，分钟级）。
    SystemQemu,
    /// 第四层实机验证（真机时间稀缺——只跑必须真机的项）。
    RealMachine,
}

/// 层深度（Unit=0 … RealMachine=3——泄漏判的坐标）。
pub fn depth(l: PyramidLayer) -> u8 {
    match l {
        PyramidLayer::Unit => 0,
        PyramidLayer::Integration => 1,
        PyramidLayer::SystemQemu => 2,
        PyramidLayer::RealMachine => 3,
    }
}

/// 倒挂泄漏判：bug 属于较浅层却在较深层被发现——depth(belongs) < depth(found)
/// 即泄漏（下层能测的漏到上层才发现，就是下层用例缺口）。
pub fn is_leak(found_at: PyramidLayer, belongs_to: PyramidLayer) -> bool {
    depth(found_at) > depth(belongs_to)
}

/// 缺口闭环记录三态（**B-1201 达标线的闭环面**——泄漏登记后闭环才算修完）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GapState {
    /// 已登记（泄漏进账——不登记的缺口会再漏）。
    Logged,
    /// 用例已补（下层补用例，修 bug 同单完成）。
    Patched,
    /// 复验通过（同场景回归绿——闭环合上）。
    Closed,
}

/// 闭环完整判：三态逐级到达且终态 Closed——登记了没补、补了没复验都不算闭环。
pub fn gap_closed(states: &[GapState]) -> bool {
    // 必须依次出现 Logged → Patched → Closed（乱序与缺级都拒）。
    if states.len() != 3 {
        return false;
    }
    states[0] == GapState::Logged && states[1] == GapState::Patched && states[2] == GapState::Closed
}

// ---------------------------------------------------------------------------
// 冒烟套与判据编号对齐
// ---------------------------------------------------------------------------

/// 冒烟用例（守护声明面——空守护的用例是裸奔的用例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SmokeCase {
    /// 守护的判据编号（"B-1201"/"WD-032"——判据改版时缺口可 grep）。
    pub guards: &'static str,
    /// 用例在册（声明了但用例不存在是空头支票）。
    pub present: bool,
}

/// 编号合法性：非空且以 'B-' 或 'WD-' 起头（守护对象必须是判据或走查项）。
pub fn guard_valid(g: &str) -> bool {
    let b = g.as_bytes();
    if b.is_empty() {
        return false;
    }
    b.starts_with(b"B-") || b.starts_with(b"WD-")
}

/// 冒烟套对齐判（**B-1202 达标线：用例与判据编号全对齐**）——每条用例在册
/// 且守护编号合法；有一条空头或裸奔即整体不对齐。
pub fn smoke_aligned(cases: &[SmokeCase]) -> bool {
    let mut i = 0;
    while i < cases.len() {
        if !cases[i].present || !guard_valid(cases[i].guards) {
            return false;
        }
        i += 1;
    }
    !cases.is_empty()
}

// ---------------------------------------------------------------------------
// CheckSet（B-1201/1202 · 6 项）
// ---------------------------------------------------------------------------

pub fn run_pyramid_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1201/1202 测试金字塔与冒烟套");
    // 1. 四层穷举：层深坐标单调（0..=3 各异）。
    let layers = [
        (PyramidLayer::Unit, 0u8),
        (PyramidLayer::Integration, 1),
        (PyramidLayer::SystemQemu, 2),
        (PyramidLayer::RealMachine, 3),
    ];
    let mut depths_ok = true;
    let mut i = 0;
    while i < layers.len() {
        if depth(layers[i].0) != layers[i].1 {
            depths_ok = false;
        }
        i += 1;
    }
    set.add(
        "B-1201 四层穷举",
        depths_ok,
        "单元/集成/QEMU 系统层/实机——四层穷举，越往下越快越多越早",
    );
    // 2. 倒挂泄漏判：下层 bug 在上层发现即泄漏；同层发现不是泄漏。
    set.add(
        "B-1201 倒挂泄漏判",
        is_leak(PyramidLayer::SystemQemu, PyramidLayer::Integration)
            && is_leak(PyramidLayer::RealMachine, PyramidLayer::Unit)
            && !is_leak(PyramidLayer::Integration, PyramidLayer::Integration)
            && !is_leak(PyramidLayer::Integration, PyramidLayer::SystemQemu),
        "depth(发现层)>depth(归属层)即泄漏——集成层能测的在 QEMU 层暴露就是集成层缺口",
    );
    // 3. 闭环三态：登记→补用例→复验，缺级乱序都不算闭环（B-1201 达标线）。
    let full = [GapState::Logged, GapState::Patched, GapState::Closed];
    let short = [GapState::Logged, GapState::Closed];
    let out_of_order = [GapState::Patched, GapState::Logged, GapState::Closed];
    set.add(
        "B-1201 缺口闭环",
        gap_closed(&full) && !gap_closed(&short) && !gap_closed(&out_of_order) && !gap_closed(&[]),
        "下层缺口补用例闭环记录——登记了没补、补了没复验都留着再次泄漏的门（B-1201 达标线）",
    );
    // 4. 守护编号合法性：非空且 B-/WD- 起头。
    set.add(
        "B-1202 守护编号合法",
        guard_valid("B-1201") && guard_valid("WD-032") && !guard_valid("") && !guard_valid("smoke-boot"),
        "守护对象必须是判据（B-xxx）或走查项（WD-xxx）——自由文本进不了对账",
    );
    // 5. 冒烟套全对齐（B-1202 达标线）：在册+合法，一条空头/裸奔即红。
    let good = [
        SmokeCase { guards: "B-101", present: true },
        SmokeCase { guards: "B-501", present: true },
        SmokeCase { guards: "WD-010", present: true },
    ];
    let phantom = [good[0], SmokeCase { guards: "B-501", present: false }];
    let naked = [good[0], SmokeCase { guards: "boot-check", present: true }];
    set.add(
        "B-1202 冒烟套全对齐",
        smoke_aligned(&good) && !smoke_aligned(&phantom) && !smoke_aligned(&naked) && !smoke_aligned(&[]),
        "用例与判据编号全对齐——空头支票与裸奔用例都进不了冒烟套（B-1202 达标线）",
    );
    // 6. 判据改版缺口一目了然：判据集合减守护集合=缺口，逐编号对账。
    let covered = ["B-101", "B-501", "WD-010"];
    let want = ["B-101", "B-501", "B-701"];
    let mut gaps = 0u8;
    i = 0;
    while i < want.len() {
        let mut found = false;
        let mut j = 0;
        while j < covered.len() {
            if covered[j] == want[i] {
                found = true;
            }
            j += 1;
        }
        if !found {
            gaps += 1;
        }
        i += 1;
    }
    set.add(
        "B-1202 用例缺口可查",
        gaps == 1,
        "判据集合减守护集合=用例缺口——want 三编号中 B-701 无守护即缺口计 1，缺口一目了然",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe20 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe20_layer_depths() {
        // 四层深度坐标：0/1/2/3 单调且互异。
        assert_eq!(depth(PyramidLayer::Unit), 0);
        assert_eq!(depth(PyramidLayer::Integration), 1);
        assert_eq!(depth(PyramidLayer::SystemQemu), 2);
        assert_eq!(depth(PyramidLayer::RealMachine), 3);
        // 相邻层泄漏：隔层也泄漏（隔层泄漏隔层补——归属层不变）。
        assert!(is_leak(PyramidLayer::RealMachine, PyramidLayer::Integration));
    }

    #[test]
    fn fe20_leak_direction() {
        // 泄漏方向性：只在"深层发现浅层 bug"时成立，反向与同层都不成立。
        assert!(is_leak(PyramidLayer::RealMachine, PyramidLayer::SystemQemu));
        assert!(!is_leak(PyramidLayer::Unit, PyramidLayer::RealMachine));
        assert!(!is_leak(PyramidLayer::Unit, PyramidLayer::Unit));
    }

    #[test]
    fn fe20_gap_closed_rules() {
        // 闭环三态逐级：全序过；缺级/乱序/空序全拒。
        assert!(gap_closed(&[GapState::Logged, GapState::Patched, GapState::Closed]));
        assert!(!gap_closed(&[GapState::Patched, GapState::Closed]));
        assert!(!gap_closed(&[GapState::Logged, GapState::Patched]));
        assert!(!gap_closed(&[GapState::Closed, GapState::Patched, GapState::Logged]));
    }

    #[test]
    fn fe20_smoke_alignment() {
        // 单条合法/单条空头/单条裸奔/空套——空套不算对齐（冒烟套不能是空的）。
        assert!(smoke_aligned(&[SmokeCase { guards: "B-1304", present: true }]));
        assert!(!smoke_aligned(&[SmokeCase { guards: "B-1304", present: false }]));
        assert!(!smoke_aligned(&[SmokeCase { guards: "x", present: true }]));
        let empty: [SmokeCase; 0] = [];
        assert!(!smoke_aligned(&empty));
    }
}
