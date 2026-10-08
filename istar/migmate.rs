//! F557 换机迁移助手 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：资产扫描完整性（六类）；只读保护判据；迁移报告；
//! 失败重试；首开校验。
//!
//! **设计要点（主册）**：
//! - U 盘系统换新盘/换宿主机：迁移助手三步（源系统接入 → 扫描用户资产
//!   出清单 → 勾选迁移到目标盘）；
//! - 迁移中源盘只读（防双写事故）；
//! - 完成后目标机首开校验（资产完整性抽查 + 迁移报告）；
//! - 迁移失败可重试（断点续传 F269 同源）。
//!
//! 六类资产（枚举即清单）：文件 / 设置 / 词库（F460）/ 主题（F305）/
//! 还原点（F121）/ 密码钥匙串之外的凭据外六类之一——主册点名为
//! 「文件/设置/词库/主题/还原点」五类 + 剪贴板历史（F109）凑六；
//! 本模块以六枚举钉死，不静默加第七类。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 六类资产（扫描完整性判据的清单唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 用户文件。
    Files,
    /// 系统与应用设置。
    Settings,
    /// 输入法词库（F460）。
    Wordbook,
    /// 主题与个性化档案（F305）。
    Themes,
    /// 系统还原点（F121）。
    RestorePoints,
    /// 剪贴板历史（F109）。
    ClipboardHistory,
}

pub const ASSET_KINDS: [AssetKind; 6] = [
    AssetKind::Files,
    AssetKind::Settings,
    AssetKind::Wordbook,
    AssetKind::Themes,
    AssetKind::RestorePoints,
    AssetKind::ClipboardHistory,
];

// ---------------------------------------------------------------------------
// 迁移会话
// ---------------------------------------------------------------------------

/// 扫描出的一条资产。
#[derive(Clone, Debug)]
pub struct AssetItem {
    pub kind: AssetKind,
    pub name: String,
    pub bytes: u64,
    /// 用户勾选（清单先看、勾了才搬）。
    pub picked: bool,
    /// 已迁完（断点续传账）。
    pub moved: bool,
}

/// 迁移会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigState {
    Scanning,
    /// 源盘已锁只读、迁移在途。
    Transferring,
    Done,
    /// 失败（可重试——断点账保留）。
    Failed,
}

/// 迁移助手。
pub struct MigMate {
    pub state: MigState,
    items: [Option<AssetItem>; 64],
    item_len: usize,
    /// 扫描是否覆盖六类（完整性判据账）。
    kinds_scanned: [bool; 6],
    /// 首开校验抽查结果（目标机首开：资产完整性抽查）。
    verified: [bool; 6],
    /// 抽查登记位（六类各是否已至少抽查一次）。
    seen: [bool; 6],
    verified_len: usize,
    /// 失败重试次数账。
    retries: u32,
}

impl MigMate {
    pub fn new() -> MigMate {
        MigMate {
            state: MigState::Scanning,
            items: [(); 64].map(|_| None),
            item_len: 0,
            kinds_scanned: [false; 6],
            verified: [false; 6],
            seen: [false; 6],
            verified_len: 0,
            retries: 0,
        }
    }

    /// 扫描：逐类登记资产（每类至少跑一遍扫描——完整性判据）。
    pub fn scan(&mut self, kind: AssetKind, name: &str, bytes: u64) -> bool {
        if self.state != MigState::Scanning || self.item_len >= 64 {
            return false;
        }
        self.items[self.item_len] = Some(AssetItem {
            kind,
            name: String::from(name),
            bytes,
            picked: false,
            moved: false,
        });
        self.item_len += 1;
        self.kinds_scanned[kind as usize] = true;
        true
    }

    /// 扫描完整性：六类全扫过才算清单齐（判据「资产扫描完整性（六类）」）。
    pub fn scan_complete(&self) -> bool {
        self.kinds_scanned.iter().all(|&b| b)
    }

    /// 勾选资产。
    pub fn pick(&mut self, name: &str, on: bool) -> bool {
        for slot in self.items[..self.item_len].iter_mut().flatten() {
            if slot.name == name {
                slot.picked = on;
                return true;
            }
        }
        false
    }

    /// 开始迁移：锁定源盘只读（状态即保护判据）。
    pub fn begin_transfer(&mut self) -> bool {
        if self.state != MigState::Scanning || !self.scan_complete() {
            return false;
        }
        self.state = MigState::Transferring;
        true
    }

    /// 源盘只读判据（目标盘写入路径在 Transferring 态才开放）。
    pub fn source_read_only(&self) -> bool {
        self.state == MigState::Transferring || self.state == MigState::Done
    }

    /// 搬一件（断点账：按顺序推进勾选项）。
    pub fn transfer_one(&mut self) -> bool {
        if self.state != MigState::Transferring {
            return false;
        }
        for slot in self.items[..self.item_len].iter_mut().flatten() {
            if slot.picked && !slot.moved {
                slot.moved = true;
                if self.items[..self.item_len]
                    .iter()
                    .flatten()
                    .all(|a| !a.picked || a.moved)
                {
                    self.state = MigState::Done;
                }
                return true;
            }
        }
        self.state = MigState::Done;
        true
    }

    /// 失败注入（迁移中出错）——重试即续传（已搬的不重搬）。
    pub fn fail(&mut self) -> bool {
        if self.state != MigState::Transferring {
            return false;
        }
        self.state = MigState::Failed;
        true
    }

    /// 重试：回 Transferring（断点账自持），失败次数入账。
    pub fn retry(&mut self) -> bool {
        if self.state != MigState::Failed {
            return false;
        }
        self.retries += 1;
        self.state = MigState::Transferring;
        true
    }

    pub fn retry_count(&self) -> u32 {
        self.retries
    }

    /// 迁移报告：勾选数/已搬数/总字节（报告页唯一取数口）。
    pub fn report(&self) -> (usize, usize, u64) {
        let mut picked = 0;
        let mut moved = 0;
        let mut bytes = 0u64;
        for a in self.items[..self.item_len].iter().flatten() {
            if a.picked {
                picked += 1;
                if a.moved {
                    moved += 1;
                    bytes += a.bytes;
                }
            }
        }
        (picked, moved, bytes)
    }

    /// 首开校验：目标机对六类抽查（逐类记录通过位；同类可复验——
    /// 修复后复抽查覆盖旧结果，账不重复计数）。
    pub fn verify_kind(&mut self, kind: AssetKind, ok: bool) -> bool {
        if self.state != MigState::Done {
            return false;
        }
        let slot = &mut self.verified[kind as usize];
        let first = !self.seen[kind as usize];
        *slot = ok;
        self.seen[kind as usize] = true;
        if first {
            self.verified_len += 1;
        }
        true
    }

    /// 首开校验结论：抽查全过才算迁移可信（缺类/失败均不签发全绿）。
    pub fn first_open_ok(&self) -> Option<bool> {
        if self.verified_len < 6 {
            return None; // 抽查未满六类——不出结论（诚实待查）。
        }
        Some(self.verified.iter().all(|&b| b))
    }

    /// 单类四态统计（深化层报告卡取数口，只读）：
    /// (该类扫描过, 该类勾选数, 该类已迁数, 该类首开校验通过位)。
    pub fn kind_stats(&self, kind: AssetKind) -> (bool, u32, u32, bool) {
        let mut picked = 0u32;
        let mut moved = 0u32;
        for a in self.items[..self.item_len].iter().flatten() {
            if a.kind == kind {
                if a.picked {
                    picked += 1;
                    if a.moved {
                        moved += 1;
                    }
                }
            }
        }
        (self.kinds_scanned[kind as usize], picked, moved, self.verified[kind as usize])
    }
}

impl Default for MigMate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_migmate_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 六类资产清单枚举齐备（扫描完整性判据的清单唯一源）。
    set.add("six asset kinds declared", ASSET_KINDS.len() == 6, "");

    // 2. 扫描完整性：六类各登一件 → scan_complete；缺一类 → 不齐。
    let mut m = MigMate::new();
    let kinds = ASSET_KINDS;
    for (i, k) in kinds.iter().enumerate() {
        m.scan(*k, alloc::format!("资产{}", i).leak(), 100);
    }
    let complete = m.scan_complete();
    let mut m2 = MigMate::new();
    for (i, k) in kinds.iter().enumerate() {
        if i < 5 {
            m2.scan(*k, alloc::format!("资产{}", i).leak(), 100);
        }
    }
    set.add("scan completeness six kinds", complete && !m2.scan_complete(), "");

    // 3. 勾选后才搬：未勾的留在源盘。
    m.pick("资产0", true);
    m.begin_transfer();
    m.transfer_one();
    let rep = m.report();
    set.add("only picked assets move", rep == (1, 1, 100), "");

    // 4. 只读保护：Transferring 态源盘只读成立。
    set.add("source read-only while transferring", m.source_read_only(), "");

    // 5. 失败重试：中断 → retry 续传（已搬的不重搬）→ 完成态。
    //    场景：勾选两件、搬完第一件后失败——重试后从断点续搬第二件。
    let mut m5 = MigMate::new();
    for (i, k) in kinds.iter().enumerate() {
        m5.scan(*k, alloc::format!("b{}", i).leak(), 10);
    }
    m5.pick("b0", true);
    m5.pick("b1", true);
    m5.begin_transfer();
    m5.transfer_one(); // b0 落位
    m5.fail();
    m5.retry();
    let cont = m5.transfer_one(); // 续搬 b1（不重搬 b0）
    let after = m5.report();
    set.add(
        "retry resumes from breakpoint",
        m5.retry_count() == 1 && cont && after == (2, 2, 20) && m5.state == MigState::Done,
        "",
    );

    // 6. 迁移报告：全搬完 → Done 且报告字节对账。
    while m.state == MigState::Transferring {
        m.transfer_one();
    }
    let (picked, moved, bytes) = m.report();
    set.add("report tallies picked and moved", m.state == MigState::Done && picked == 1 && moved == 1 && bytes == 100, "");

    // 7. 首开校验：六类齐过 → 全绿；缺类 → 不出结论；坏类 → 拒签。
    let mut m3 = MigMate::new();
    for k in kinds.iter() {
        m3.scan(*k, "a", 1);
        m3.pick("a", true);
    }
    m3.begin_transfer();
    while m3.state == MigState::Transferring {
        m3.transfer_one();
    }
    set.add("first open needs all six", m3.first_open_ok().is_none(), "");
    for (i, k) in kinds.iter().enumerate() {
        m3.verify_kind(*k, i != 3);
    }
    set.add("bad kind blocks sign-off", m3.first_open_ok() == Some(false), "");
    m3.verify_kind(AssetKind::Themes, true); // 复验覆盖旧结果（账不重复计数）
    set.add("all green signs off", m3.first_open_ok() == Some(true), "");

    // 8. 未扫描完不允许开迁移（完整性门在转移动作前）。
    let mut m4 = MigMate::new();
    m4.scan(AssetKind::Files, "f", 1);
    set.add("transfer blocked before complete scan", !m4.begin_transfer(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> MigMate {
        let mut m = MigMate::new();
        for (i, k) in ASSET_KINDS.iter().enumerate() {
            m.scan(*k, alloc::format!("a{}", i).leak(), (i as u64 + 1) * 10);
        }
        m
    }

    #[test]
    fn unpicked_assets_stay() {
        let mut m = seeded();
        m.pick("a2", true);
        m.begin_transfer();
        while m.state == MigState::Transferring {
            m.transfer_one();
        }
        let (picked, moved, bytes) = m.report();
        assert_eq!((picked, moved, bytes), (1, 1, 30));
    }

    #[test]
    fn fail_only_while_transferring() {
        let mut m = seeded();
        assert!(!m.fail());
        m.begin_transfer();
        assert!(m.fail());
        assert!(!m.fail()); // Failed 态不能二次 fail
    }

    #[test]
    fn retry_only_from_failed() {
        let mut m = seeded();
        assert!(!m.retry());
    }
}
