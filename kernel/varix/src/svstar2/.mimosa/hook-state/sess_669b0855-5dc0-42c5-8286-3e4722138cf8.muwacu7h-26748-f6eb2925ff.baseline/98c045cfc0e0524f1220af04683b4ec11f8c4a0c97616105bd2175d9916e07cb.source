//! VE-F0201 · 域自检（判据逐条对应，见 `veb01_device.rs` / `veb01_init.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - QEMU 识别并 DRIVER_OK → `B01-枚举-*`、`B01-初始化-DRIVER_OK`
//! - 旧版设备显性拒绝 → `B01-旧版-*`
//! - 能力解析完整 → `B01-能力-*`
//! - 版本协商 VERSION_1 必需（缺即拒绝三要素）→ `B01-协商-*`
//! - 注册报告过 A 域 schema → `B01-对接-*`
//! - 初始化 ≤100ms → `B01-时序-*`
//! - 零"试试看"写 → `B01-零试写-*`
//!
//! 全部确定性：快照注入、逻辑成本计时、无墙钟。

use super::veb01_device::*;
use super::veb01_init::*;
use super::veb01_report::{a11y_summary, register_with_a_domain};
use super::vea01_probe::AdapterClass;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// QEMU 现代版对拍基准快照。
fn snap() -> VirtioGpuDeviceSnapshot {
    VirtioGpuDeviceSnapshot::qemu_modern("0000:00:02.0")
}

/// 标准 happy path 初始化结果。
fn init_ok(s: &VirtioGpuDeviceSnapshot) -> InitOutcome {
    preflight(s).expect("标准快照必须过前置校验");
    InitMachine::run_standard_init(s)
}

/// VE-F0201 域自检。
pub fn run_veb01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb01");

    // ---- 判据：QEMU 识别并 DRIVER_OK ----

    // 身份命中：vendor 0x1AF4 + device 0x1050
    {
        let s = snap();
        set.add(
            "B01-枚举-商标命中",
            s.is_virtio_gpu() && s.identity_rejection().is_none(),
            "",
        );
    }
    // 完整初始化序走到 DRIVER_OK，状态位全齐
    {
        let s = snap();
        let out = init_ok(&s);
        set.add(
            "B01-初始化-DRIVER_OK",
            out.ok
                && out.stage == Stage::DriverOk
                && out.status == (STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK),
            "",
        );
    }
    // 状态位按标准序逐位点亮（中间态可见，不是一步跳到终态）
    {
        let s = snap();
        let mut m = InitMachine::new(&s);
        let a = m.acknowledge().is_ok() && m.status() == STATUS_ACKNOWLEDGE;
        let d = m.set_driver().is_ok() && m.status() == STATUS_ACKNOWLEDGE | STATUS_DRIVER;
        set.add("B01-初始化-状态位逐位点亮", a && d, "");
    }
    // 两条队列（controlq + cursorq）按规格就位
    {
        let s = snap();
        let out = init_ok(&s);
        let ok = out.queues.len() == 2
            && out.queues[0].index == 0
            && out.queues[1].index == 1
            && out.queues[0].size == 64
            && out.queues.iter().all(|q| q.ready);
        set.add("B01-队列-controlq+cursorq就位", ok, "");
    }
    // 队列深度非 2 的幂 ⇒ 显性拒绝（virtio 语义）
    {
        let mut s = snap();
        s.queue_sizes = [63, 64];
        let out = InitMachine::run_standard_init(&s);
        let ok = !out.ok
            && out
                .rejection
                .as_ref()
                .map(|r| r.code == "E_QUEUE_INVALID" && r.is_complete())
                .unwrap_or(false);
        set.add("B01-队列-非幂深度拒绝", ok, "");
    }

    // ---- 判据：旧版设备显性拒绝 ----

    {
        let s = VirtioGpuDeviceSnapshot::legacy("0000:00:02.0");
        let r = preflight(&s);
        let ok = r
            .as_ref()
            .err()
            .map(|e| e.code == "E_LEGACY_DEVICE" && e.is_complete())
            .unwrap_or(false);
        // 旧版也不许借道初始化器：run_standard_init 不绕过 preflight 的
        // 话会走协商失败路径——两条路都必须堵死
        let out = InitMachine::run_standard_init(&s);
        let also_blocked = !out.ok;
        set.add("B01-旧版-显性拒绝三要素", ok && also_blocked, "");
    }
    // 非 virtio 商标不认（vendor 走错门）
    {
        let mut s = snap();
        s.vendor_id = 0x8086;
        let r = preflight(&s);
        set.add(
            "B01-枚举-非virtio不认",
            r.as_ref().err().map(|e| e.code == "E_NOT_VIRTIO").unwrap_or(false),
            "",
        );
    }
    // virtio 家族内非 gpu 设备号不认（device 走错门）
    {
        let mut s = snap();
        s.device_id = 0x1041; // virtio-blk
        let r = preflight(&s);
        set.add(
            "B01-枚举-virtio家族内不混认",
            r.as_ref().err().map(|e| e.code == "E_NOT_VIRTIO_GPU").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：能力解析完整 ----

    // 四类齐全 + notify 乘数解析
    {
        let s = snap();
        let map = parse_capabilities(&s);
        let ok = map
            .map(|m| m.complete() && m.notify_off_multiplier == 1)
            .unwrap_or(false);
        set.add("B01-能力-四类齐全含notify乘数", ok, "");
    }
    // 空能力列表显性拒绝
    {
        let mut s = snap();
        s.caps_raw = vec![];
        let r = parse_capabilities(&s);
        set.add(
            "B01-能力-空列表拒绝",
            r.as_ref().err().map(|e| e.code == "E_NO_CAPS" && e.is_complete()).unwrap_or(false),
            "",
        );
    }
    // 缺 device cfg 显性拒绝（没有它拿不到扫描出口数）
    {
        let mut s = snap();
        s.caps_raw.retain(|c| c.cfg_type != 4);
        let r = parse_capabilities(&s);
        set.add(
            "B01-能力-缺失显性拒绝",
            r.as_ref().err().map(|e| e.code == "E_CAP_MISSING").unwrap_or(false),
            "",
        );
    }
    // 能力区域越出 BAR 边界拒绝
    {
        let mut s = snap();
        s.caps_raw[0].offset = 0x10000; // bar4 只有 0x4000
        let r = parse_capabilities(&s);
        set.add(
            "B01-能力-越界拒绝",
            r.as_ref().err().map(|e| e.code == "E_CAP_MALFORMED").unwrap_or(false),
            "",
        );
    }
    // 同类区域重复声明拒绝
    {
        let mut s = snap();
        s.caps_raw.push(PciCapRaw::new(1, 4, 0x0080, 0x0038));
        let r = parse_capabilities(&s);
        set.add(
            "B01-能力-重复声明拒绝",
            r.as_ref().err().map(|e| e.code == "E_CAP_DUP").unwrap_or(false),
            "",
        );
    }
    // 非四类的 cfg_type 拒绝
    {
        let mut s = snap();
        s.caps_raw.push(PciCapRaw::new(9, 4, 0x0090, 0x0010));
        let r = parse_capabilities(&s);
        set.add(
            "B01-能力-cfg_type越类拒绝",
            r.as_ref().err().map(|e| e.code == "E_CAP_MALFORMED").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：版本协商 VERSION_1 必需 ----

    {
        let mut s = snap();
        s.offered_features &= !(1u64 << F_VERSION_1);
        let out = InitMachine::run_standard_init(&s);
        let ok = !out.ok
            && out
                .rejection
                .as_ref()
                .map(|r| r.code == "E_NO_VERSION_1" && r.is_complete())
                .unwrap_or(false)
            && out.status & STATUS_FAILED != 0;
        set.add("B01-协商-VERSION_1缺失拒绝三要素", ok, "");
    }
    // 设备专有特征解析（virgl/edid 位图进能力表）
    {
        let s = snap();
        let out = init_ok(&s);
        let g = out.gpu_features;
        set.add(
            "B01-协商-设备特征解析",
            g.virgl && g.edid && !g.resource_blob && g.describe().contains("3D加速"),
            "",
        );
    }

    // ---- 判据：零"试试看"写 ----

    // 白名单外寄存器一律拦截
    {
        let s = snap();
        let mut m = InitMachine::new(&s);
        let r = m.record_write("probe", RegKind::Unknown(0xAB));
        set.add(
            "B01-零试写-未知寄存器拦截",
            r.as_ref().err().map(|e| e.code == "E_EXPLORATORY_WRITE" && e.is_complete()).unwrap_or(false)
                && m.stage() == Stage::Failed,
            "",
        );
    }
    // happy path 的每一次写都落在白名单内且全程留痕
    {
        let s = snap();
        let out = init_ok(&s);
        let whitelisted = out.writes.iter().all(|w| {
            !matches!(w.reg, RegKind::Unknown(_))
        });
        set.add(
            "B01-零试写-全程白名单留痕",
            whitelisted && out.writes.len() >= 10,
            "",
        );
    }
    // 跳步显性拒绝（跳过 ack 直接 set_driver）
    {
        let s = snap();
        let mut m = InitMachine::new(&s);
        let r = m.set_driver();
        set.add(
            "B01-初始化-跳步显性拒绝",
            r.as_ref().err().map(|e| e.code == "E_STATE_ILLEGAL" && e.next.contains("标准序")).unwrap_or(false),
            "",
        );
    }
    // 队列没齐不许收口 DRIVER_OK
    {
        let s = snap();
        let mut m = InitMachine::new(&s);
        let _ = m.acknowledge();
        let _ = m.set_driver();
        let _ = m.negotiate_features();
        let _ = m.features_ok();
        let _ = m.setup_queue(QueueRole::Control, 64);
        let ok = m.driver_ok().is_err()
            && m.stage() == Stage::Failed;
        set.add("B01-初始化-缺队列不收口", ok, "");
    }

    // ---- 判据：读 device cfg 拿扫描出口数与最大尺寸 ----

    {
        let s = snap();
        let ok = s.display.validate().is_ok()
            && s.display.scanouts == 1
            && s.display.max_width == 3840
            && s.display.max_height == 2160;
        set.add("B01-扫描-出口数与最大尺寸可读", ok, "");
    }
    // scanout 数越界（17 > 16）拒绝
    {
        let mut s = snap();
        s.display.scanouts = MAX_SCANOUTS + 1;
        set.add(
            "B01-扫描-越界拒绝",
            s.display.validate().as_ref().err().map(|e| e.code == "E_CFG_INVALID").unwrap_or(false),
            "",
        );
    }
    // 0 个扫描出口拒绝
    {
        let mut s = snap();
        s.display.scanouts = 0;
        set.add(
            "B01-扫描-零出口拒绝",
            s.display.validate().as_ref().err().map(|e| e.code == "E_CFG_INVALID").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：初始化 ≤100ms ----

    {
        let s = snap();
        let out = init_ok(&s);
        set.add(
            "B01-时序-百毫秒预算内",
            out.timing.within_budget() && out.timing.total_us() <= INIT_BUDGET_US,
            "",
        );
    }
    // 分步账完整且可读（可观测铁律：每一步耗时都能指认）
    {
        let s = snap();
        let out = init_ok(&s);
        let d = out.timing.describe();
        set.add(
            "B01-时序-分步账可读",
            out.timing.steps.len() == 8 && d.contains("reset") && d.contains("driver_ok"),
            "",
        );
    }

    // ---- 判据：注册报告过 A 域 schema ----

    {
        let s = snap();
        let out = init_ok(&s);
        match register_with_a_domain(&s, &out) {
            Ok(reg) => {
                let report_ok = reg
                    .report
                    .as_ref()
                    .map(|r| {
                        r.adapters.len() == 1
                            && r.adapters[0].adapter_class == AdapterClass::Virtual
                            && !r.a11y_summary.is_empty()
                    })
                    .unwrap_or(false);
                set.add(
                    "B01-对接-过A域schema",
                    reg.accepted && report_ok,
                    "",
                );
                set.add(
                    "B01-对接-主适配器候选",
                    reg.primary_candidate && reg.fingerprint != 0,
                    "",
                );
                let a11y = a11y_summary(&reg, &s);
                set.add(
                    "B01-读屏-注册状态可播",
                    a11y.contains("virtio-gpu")
                        && a11y.contains("主适配器候选")
                        && a11y.contains("扫描出口"),
                    "",
                );
            }
            Err(e) => {
                set.fail("B01-对接-过A域schema", "注册被拒");
                set.fail("B01-对接-主适配器候选", "注册被拒");
                set.fail("B01-读屏-注册状态可播", "注册被拒");
            }
        }
    }
    // 初始化失败的设备不许注册（失败状态不外推）
    {
        let mut s = snap();
        s.offered_features &= !(1u64 << F_VERSION_1);
        let out = InitMachine::run_standard_init(&s);
        let r = register_with_a_domain(&s, &out);
        set.add(
            "B01-对接-失败设备不注册",
            r.as_ref().err().map(|e| e.code == "E_NO_VERSION_1").unwrap_or(false),
            "",
        );
    }

    // ---- 确定性与指纹 ----

    {
        let s = snap();
        let a = init_ok(&s);
        let b = init_ok(&s);
        set.add(
            "B01-确定-同快照同结果同指纹",
            a.stage == b.stage
                && a.status == b.status
                && a.timing.total_us() == b.timing.total_us()
                && outcome_fingerprint(&s, &a) == outcome_fingerprint(&s, &b),
            "",
        );
    }
    // 快照指纹对字段敏感（驱动版本变 ⇒ 指纹变，与 A 域纪律同源）
    {
        let a = snap();
        let mut b = snap();
        b.driver_version = "qemu-virtio-1.1".to_string();
        set.add(
            "B01-确定-指纹对驱动版本敏感",
            a.fingerprint() != b.fingerprint(),
            "",
        );
    }
    // 三要素完整性机器校验（所有拒绝路径的公共闸）
    {
        let samples = [
            VirtioGpuDeviceSnapshot::legacy("0000:00:02.0"),
            {
                let mut s = snap();
                s.vendor_id = 0x1234;
                s
            },
        ];
        let all_complete = samples.iter().all(|s| {
            s.identity_rejection()
                .map(|r| r.is_complete())
                .unwrap_or_else(|| s.device_id == DEVICE_LEGACY)
        });
        set.add("B01-边界-拒绝必带三要素", all_complete, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svstar2::veb01_init;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veb01_checks_all_green() {
        let set = run_veb01_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-B01 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// QEMU 对拍：标准快照完整跑通探测→初始化→注册。
    #[test]
    fn qemu_snapshot_end_to_end() {
        let s = snap();
        let out = init_ok(&s);
        assert!(out.ok, "初始化必须成功：{:?}", out.rejection);
        let reg = register_with_a_domain(&s, &out).expect("注册必须成功");
        assert!(reg.accepted && reg.primary_candidate);
        assert_eq!(reg.report.as_ref().unwrap().adapters[0].slot, s.slot);
    }

    /// 同输入重跑两次结果逐位一致（确定性纪律）。
    #[test]
    fn init_is_deterministic() {
        let s = snap();
        let a = init_ok(&s);
        let b = init_ok(&s);
        assert_eq!(a.writes.len(), b.writes.len());
        assert_eq!(a.writes, b.writes);
        assert_eq!(a.timing.total_us(), b.timing.total_us());
    }

    /// 旧版设备全链路堵死：preflight 与状态机两条路都不放行。
    #[test]
    fn legacy_device_blocked_everywhere() {
        let s = VirtioGpuDeviceSnapshot::legacy("0000:00:02.0");
        assert!(veb01_init::preflight(&s).is_err());
        let out = InitMachine::run_standard_init(&s);
        assert!(!out.ok);
        // 显性拒绝带对路建议
        let r = preflight(&s).unwrap_err();
        assert!(r.next.contains("0x1050"), "建议须指向现代版设备：{}", r.next);
    }

    /// 初始化失败后 FAILED 终态拒绝一切后续步骤。
    #[test]
    fn failed_state_is_terminal() {
        let s = snap();
        let mut m = InitMachine::new(&s);
        let _ = m.record_write("probe", RegKind::Unknown(0x77));
        assert_eq!(m.stage(), Stage::Failed);
        assert!(m.acknowledge().is_err(), "失败终态不许续跑");
    }
}
