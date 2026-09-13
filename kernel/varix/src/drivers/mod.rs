//! AI-31 内核硬件栈（领域13 · 族0301~0308 · X07501~X07700），勿删。
//! 八族 × 25 项 = 200 项 CheckSet；本模块仅做聚合与门禁测试。

pub mod acpi;
pub mod bt;
pub mod driver;
pub mod gpu;
pub mod irqdma;
pub mod netstack;
pub mod thermal;
pub mod usb;

use crate::checks::CheckSet;

/// AI-31 硬件栈全量自检：8 族 200 项。
pub fn run_ai31_hardware_checks() -> [CheckSet; 8] {
    [
        driver::run_driver_checks(),
        irqdma::run_irqdma_checks(),
        acpi::run_acpi_checks(),
        thermal::run_thermal_checks(),
        usb::run_usb_checks(),
        bt::run_bt_checks(),
        netstack::run_netstack_checks(),
        gpu::run_gpu_checks(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai31_hardware_200_checks_pass() {
        let sets = run_ai31_hardware_checks();
        let total: usize = sets.iter().map(|s| s.len()).sum();
        assert_eq!(total, 200);
        for s in &sets {
            let mut buf = [0u8; 4096];
            let n = s.render(&mut buf);
            assert!(s.all_passed(), "{}", core::str::from_utf8(&buf[..n]).unwrap_or(""));
        }
    }
}
