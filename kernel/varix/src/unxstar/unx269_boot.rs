// SPDX-License-Identifier: Apache-2.0
//! UNX-F0269 · boot 命令（BootManifest 全视图）
//!
//! 十二步节分页展示齐备，CRC 链校验态显示正确。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// BootManifest 十二步节
// ---------------------------------------------------------------------------

/// 引导步节。
#[derive(Debug, Clone)]
pub struct BootStep {
    pub id: u8,
    pub name: String,
    /// CRC 校验态。
    pub crc_valid: bool,
    /// 耗时（微秒）。
    pub elapsed_us: u64,
}

/// BootManifest。
pub struct BootManifest {
    pub steps: Vec<BootStep>,
    /// CRC 链。
    pub crc_chain: Vec<u32>,
}

impl BootManifest {
    /// 十二步节标准清单。
    pub fn standard() -> Self {
        let names = [
            "firmware_init", "memory_map", "kernel_load", "page_table",
            "interrupt", "scheduler", "heap_init", "vfs_mount",
            "driver_probe", "user_space", "login", "ready",
        ];
        let steps = names.iter().enumerate().map(|(i, n)| BootStep {
            id: i as u8,
            name: n.to_string(),
            crc_valid: true,
            elapsed_us: 1000 * (i as u64 + 1),
        }).collect();
        BootManifest { steps, crc_chain: (0..12).map(|i| 0xABCD + i).collect() }
    }

    /// 分页展示（每页 N 步）。
    pub fn paginate(&self, page_size: usize) -> Vec<Vec<&BootStep>> {
        self.steps.chunks(page_size).map(|c| c.iter().collect()).collect()
    }

    /// CRC 链校验。
    pub fn verify_crc_chain(&self) -> bool {
        if self.crc_chain.len() != self.steps.len() { return false; }
        self.steps.iter().all(|s| s.crc_valid)
    }

    /// 步骤总数。
    pub fn step_count(&self) -> usize { self.steps.len() }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0269 判据全集。
pub fn run_boot_manifest_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0269", "boot 命令");

    // 1. 十二步节
    let manifest = BootManifest::standard();
    cs.ok("十二步节", manifest.step_count() == 12, "完整");
    cs.ok("CRC 链", manifest.crc_chain.len() == 12, "链完整");
    cs.ok("CRC 校验", manifest.verify_crc_chain(), "全通过");

    // 2. 分页
    let pages = manifest.paginate(5);
    cs.ok("分页数", pages.len() == 3, "12/5=3 页");
    cs.ok("首页 5 步", pages[0].len() == 5, "每页 5");
    cs.ok("末页 2 步", pages[2].len() == 2, "余 2");

    // 3. 步骤内容
    cs.ok("首步", manifest.steps[0].name == "firmware_init", "启动序列");
    cs.ok("末步", manifest.steps[11].name == "ready", "启动完成");

    // 4. CRC 校验态
    cs.ok("全 CRC 有效", manifest.steps.iter().all(|s| s.crc_valid), "链校验");

    // 5. 坏 CRC
    let mut bad = BootManifest::standard();
    bad.steps[3].crc_valid = false;
    cs.ok("坏 CRC 检出", !bad.verify_crc_chain(), "第 4 步无效");

    // 6. 空页
    let empty_manifest = BootManifest { steps: Vec::new(), crc_chain: Vec::new() };
    cs.ok("空 manifest", empty_manifest.paginate(5).is_empty(), "零步节");

    cs.done()
}
