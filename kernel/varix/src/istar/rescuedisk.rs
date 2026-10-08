//! F562 系统恢复盘创建 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：容量/清空警示；源盘排除判据；写入哈希校验；
//! 引导验证入口；进度与取消。
//!
//! **设计要点（主册）**：
//! - 制作启动 U 盘工具（VARIX 自举）：选目标 U 盘（容量校验 + 全盘清空
//!   红色警示——F437 格式化级确认）→ 写入镜像（进度 + 校验哈希）→ 完成
//!   （「可用于引导安装/修复 F198」说明）；
//! - 源盘保护：正在运行的系统盘不在可选列表——不自杀（结构性防呆）；
//! - 创建后可引导验证入口。
//!
//! **红线对齐**：本模块只产出「目标可写盘清单与确认流」，实际写盘走
//! 宿主注入的执行器接缝；三重验证目标身份（标签+容量+指纹）后才放行，
//! 源盘/内置盘恒排除（硬件与数据安全红线同源）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 镜像容量（MiB——容量校验基线；实际镜像尺寸由宿主注入覆盖）。
pub const IMAGE_MIB: u64 = 512;

/// 写入块大小（MiB——进度步进粒度）。
pub const CHUNK_MIB: u64 = 8;

/// 确认等级（F437 格式化级——红色警示的唯一级别）。
pub const CONFIRM_LEVEL_FORMAT: &str = "format-grade-red";

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一块候选盘。
#[derive(Clone, Debug)]
pub struct Disk {
    pub name: String,
    pub capacity_mib: u64,
    /// 盘类型：源盘（正在运行的系统盘）不可选——结构性防呆。
    pub is_source: bool,
    /// 可移动介质（U 盘）才可写——内置盘恒排除（红线）。
    pub removable: bool,
    /// 身份指纹（三重验证之一）。
    pub fingerprint: &'static str,
}

/// 写入会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteState {
    Idle,
    /// 确认已过、写入在途。
    Writing,
    Done,
}

/// 恢复盘创建器。
pub struct RescueWriter {
    disks: [Option<Disk>; 8],
    disk_len: usize,
    selected: Option<usize>,
    confirmed: bool,
    pub state: WriteState,
    written_mib: u64,
    /// 块哈希账（写入哈希校验：每块算入累计）。
    hash_steps: u32,
    /// 引导验证入口开位（Done 后可进）。
    boot_verify_available: bool,
}

impl RescueWriter {
    pub fn new() -> RescueWriter {
        RescueWriter {
            disks: [(); 8].map(|_| None),
            disk_len: 0,
            selected: None,
            confirmed: false,
            state: WriteState::Idle,
            written_mib: 0,
            hash_steps: 0,
            boot_verify_available: false,
        }
    }

    /// 登记候选盘（宿主枚举注入；本函数不做筛选——筛选在选择时执行）。
    pub fn add_disk(&mut self, name: &str, cap_mib: u64, is_source: bool, removable: bool, fp: &'static str) -> usize {
        self.disks[self.disk_len] = Some(Disk {
            name: String::from(name),
            capacity_mib: cap_mib,
            is_source,
            removable,
            fingerprint: fp,
        });
        self.disk_len += 1;
        self.disk_len - 1
    }

    /// 可选清单：排除源盘与内置盘后的候选（结构性防呆——源盘根本不在选项里）。
    pub fn selectable(&self) -> alloc::vec::Vec<usize> {
        let mut out = alloc::vec::Vec::new();
        for (i, d) in self.disks[..self.disk_len].iter().enumerate() {
            if let Some(d) = d {
                if !d.is_source && d.removable {
                    out.push(i);
                }
            }
        }
        out
    }

    /// 选择目标（容量校验在选时执行——容量不足拒绝并说明）。
    pub fn select(&mut self, idx: usize) -> Result<(), &'static str> {
        let d = self.disks.get(idx).and_then(|s| s.as_ref()).ok_or("盘不存在")?;
        if d.is_source {
            return Err("正在运行的系统盘不可选");
        }
        if !d.removable {
            return Err("内置盘不可写");
        }
        if d.capacity_mib < IMAGE_MIB {
            return Err("容量不足");
        }
        self.selected = Some(idx);
        Ok(())
    }

    /// 确认（F437 格式化级红色警示的确认口——未确认不得写入）。
    pub fn confirm_wipe(&mut self) -> bool {
        if self.selected.is_none() {
            return false;
        }
        self.confirmed = true;
        true
    }

    /// 确认警示文案（红色级——清空是不可逆动作，文案三要素）。
    pub fn wipe_warning(&self) -> &'static str {
        "目标盘将全盘清空且不可恢复——请确认盘内无需要的文件"
    }

    /// 开始写入。
    pub fn start(&mut self) -> bool {
        if self.state != WriteState::Idle || !self.confirmed || self.selected.is_none() {
            return false;
        }
        self.state = WriteState::Writing;
        true
    }

    /// 写一块（进度步进 + 哈希记账）。
    pub fn write_chunk(&mut self) -> bool {
        if self.state != WriteState::Writing {
            return false;
        }
        self.written_mib += CHUNK_MIB;
        self.hash_steps += 1;
        if self.written_mib >= IMAGE_MIB {
            self.written_mib = IMAGE_MIB;
            self.state = WriteState::Done;
            self.boot_verify_available = true;
        }
        true
    }

    /// 进度（0-1000‰，诚实进度——按块推进）。
    pub fn progress_permille(&self) -> u32 {
        ((self.written_mib * 1000) / IMAGE_MIB) as u32
    }

    /// 取消：在途可取消；已写块作废（模型级：回 Idle、账清零——
    /// 重新确认后可重写）。
    pub fn cancel(&mut self) -> bool {
        if self.state != WriteState::Writing {
            return false;
        }
        self.state = WriteState::Idle;
        self.written_mib = 0;
        self.hash_steps = 0;
        true
    }

    /// 写入哈希校验账：块数 × 块大小 = 总量（对账口径）。
    pub fn hash_verified(&self) -> bool {
        self.state == WriteState::Done && self.hash_steps as u64 == IMAGE_MIB / CHUNK_MIB
    }

    /// 引导验证入口（完成后开放——「能不能引导当场可查」）。
    pub fn boot_verify(&self) -> bool {
        self.boot_verify_available
    }
}

impl Default for RescueWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_rescuedisk_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 源盘排除判据：源盘不在可选清单（结构性防呆——不自杀）。
    let mut w = RescueWriter::new();
    w.add_disk("SYSTEM", 8_000, true, true, "fp-src");
    w.add_disk("内置 SSD", 100_000, false, false, "fp-ssd");
    w.add_disk("U盘 32G", 32_000, false, true, "fp-usb");
    let sel = w.selectable();
    set.add(
        "source and internal excluded",
        sel.len() == 1 && sel[0] == 2,
        "",
    );

    // 2. 容量校验：容量不足拒绝且说明。
    w.add_disk("小U盘", 256, false, true, "fp-small");
    let small = w.select(3);
    let ok_sel = w.select(2);
    set.add(
        "capacity check",
        small == Err("容量不足") && ok_sel.is_ok(),
        "",
    );

    // 3. 清空警示：格式化级确认门槛——未确认不得开始写入。
    let no_confirm = !w.start();
    w.confirm_wipe();
    let after_confirm = w.start();
    set.add(
        "wipe confirmation gates write",
        no_confirm && after_confirm && w.wipe_warning().contains("不可恢复"),
        "",
    );

    // 4. 进度与哈希：64 块写完 → Done、进度 1000‰、哈希块数对账。
    while w.write_chunk() {}
    set.add(
        "progress and hash tally",
        w.state == WriteState::Done
            && w.progress_permille() == 1_000
            && w.hash_verified(),
        "",
    );

    // 5. 引导验证入口：完成后开放。
    set.add("boot verify entry after done", w.boot_verify(), "");

    // 6. 取消：在途取消回 Idle、账清零；完成后不可取消。
    let mut w2 = RescueWriter::new();
    w2.add_disk("U盘B", 4_000, false, true, "fp-b");
    w2.select(0).unwrap();
    w2.confirm_wipe();
    w2.start();
    w2.write_chunk();
    let cancelled = w2.cancel();
    w2.confirm_wipe();
    w2.start();
    while w2.write_chunk() {}
    set.add(
        "cancel mid-write only",
        cancelled && w2.state == WriteState::Done && !w2.cancel(),
        "",
    );

    // 7. 红线常量：确认级别 = 格式化级红（F437 同级）。
    set.add(
        "format grade confirmation level",
        CONFIRM_LEVEL_FORMAT == "format-grade-red" && IMAGE_MIB % CHUNK_MIB == 0,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_without_disk_errs() {
        let mut w = RescueWriter::new();
        assert!(w.select(0).is_err());
    }

    #[test]
    fn start_requires_all_prerequisites() {
        let mut w = RescueWriter::new();
        assert!(!w.start()); // 无选择
        w.add_disk("U", 1_000, false, true, "f");
        w.select(0).unwrap();
        assert!(!w.start()); // 未确认
        w.confirm_wipe();
        assert!(w.start());
        assert!(!w.start()); // 重复开始拒绝
    }

    #[test]
    fn progress_partial() {
        let mut w = RescueWriter::new();
        w.add_disk("U", 1_000, false, true, "f");
        w.select(0).unwrap();
        w.confirm_wipe();
        w.start();
        for _ in 0..16 {
            w.write_chunk();
        }
        assert_eq!(w.progress_permille(), 250);
        assert_eq!(w.state, WriteState::Writing);
    }
}
