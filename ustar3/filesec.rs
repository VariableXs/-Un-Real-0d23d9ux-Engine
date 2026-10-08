//! filesec —— I 通用域·文件安全与隐私出口（AI-U3 分工包 · F509~F512）。
//!
//! 本文件覆盖四项，各节逐字摘录主册《Varix STAR I start.md》【验收判据】
//! 第一句，并注明功能定义要点、依赖锚点与零堆纪律。
//!
//! ── F509 文件粉碎 ──────────────────────────────────────────────
//! 判据：折叠入口；三重警示与焦点；覆写执行或诚实标注两分支；普通删除
//! 不受影响；耗时提示（大文件覆写慢）。
//! 功能定义要点：「彻底删除」右键项默认折叠在「显示更多选项」（危险功能
//! 不占一级菜单）；确认对话框三重警示（文件名已列 / 数量核对 /
//! 「此操作不可恢复——不经过回收站」）+ 默认焦点取消（F207 铁律）；
//! 执行为覆写删除（数据区覆写后释放——U 盘介质上尽力而为，介质不支持
//! 覆写时诚实标注「此介质无法保证覆写，建议启用全盘加密 F439」）；
//! 普通删除的后悔药（F261）不受影响；技术边界（U 盘磨损均衡）说清楚。
//! 依赖锚点：F207（默认焦点取消）/ F261（回收站后悔药不受影响）/
//! F439（全盘加密建议锚）。
//! 零堆纪律：定长逐块覆写账目，无 alloc。
//!
//! ── F510 单文件加密 ────────────────────────────────────────────
//! 判据：加密/浏览/导出三链路；临时视图无痕判据（关闭后临时区清零）；
//! 原文件去留；密码错误提示；文件夹打包。
//! 功能定义要点：右键「加密」：单文件/文件夹加密（密码派生密钥，MD1
//! 密码学栈；文件夹整体打包加密为 .vxcrypt 单文件）；加密后原文件可选
//! 保留（默认不保留）但提示一次；打开 .vxcrypt 双击输密码解出临时视图
//! （关闭即隐——不是解密落盘，阅后即焚式浏览）；导出解密副本需明确
//! 选择；密码丢了就是真丢了（开箱明说，没有后门）。
//! 依赖锚点：MD1（密码学栈——本层为纯逻辑结构占位：FNV 派生 + 可逆
//! 异或流 + 认证标记，结构完整不含真密码学）。
//! 零堆纪律：定长 blob / 定长临时视图槽位表，无 alloc。
//!
//! ── F511 剪贴板一键清空 ───────────────────────────────────────
//! 判据：双入口；当前+历史全清；确认框条数；密码后提示触发与 5s 时序；
//! 清空后粘贴行为（空——应用得到诚实失败）。
//! 功能定义要点：剪贴板隐私出口：快速设置（F076）磁贴「清空剪贴板」+
//! 快捷键（Ctrl+Shift+Delete）——清空当前剪贴板与历史（F109）所有条目；
//! 清空有一次性确认（确认框列条数）；敏感场景自觉（输完密码后系统提示
//! 条 5 秒「剪贴板仍含有复制的内容——清空？」），到时未清则提示过期
//! 不烦人。
//! 依赖锚点：F076（快速设置磁贴入口）/ F109（剪贴板历史环）。
//! 零堆纪律：定长历史环 + 定长载荷，无 alloc。
//!
//! ── F512 截图历史 ─────────────────────────────────────────────
//! 判据：20 条上限与淘汰；临时/已保存两态；关机清理与通知；重编辑
//! 链路；另存路径。
//! 功能定义要点：截图工具（F098）的最近条目架：最近 20 张截图缩略条
//! ——点开即重看/重编辑/另存/复制/删除；截图文件本体在用户主动保存前
//! 驻临时区（未保存的关机即失——F311 同源纪律，通知一次）；历史持久
//! 仅含已保存项的引用。
//! 依赖锚点：F098（截图工具）/ F311（关机未落盘数据即失、通知一次
//! 同源纪律）。
//! 零堆纪律：20 条定长环 + 'static 路径引用，无 alloc。
//!
//! 共同纪律：逻辑路径零堆（无 String/Vec/Box/format!，定长数组 +
//! core 运算）；判据唯一源（本头注释逐条摘录主册判据）；数字精确成
//! 常量并在注释注明主册依据；自检经 robust.rs 域函数指针表注册。

use crate::checks::CheckSet;

// ===========================================================================
// F509 文件粉碎
// ===========================================================================

/// 覆写轮数。主册未规定轮数，取行业惯例单轮全量覆写（单轮全量已使常规
/// 取证手段失效；多轮写 0/1/随机属过度设计）。
pub const SHRED_ROUNDS: u32 = 1;

/// 覆写吞吐建模：32 MB/s（主册未规定，取常规 SATA 盘保守下限）。
pub const OVERWRITE_THROUGHPUT_MBPS: u64 = 32;

/// 「大文件覆写慢」提示阈值：预估覆写时长达到 3 秒即提示。
/// 主册未规定数值，取 3 秒（超过人注意力单次等待的自然上限）。
pub const SLOW_HINT_THRESHOLD_MS: u64 = 3_000;

/// 覆写账目块粒度 4MB。
pub const SHRED_BLOCK_BYTES: u64 = 4 << 20;

/// 逐块覆写账目最大块数（单次粉碎最多 32MB 逐块对账；超出按总量口径
/// 执行、账目封顶）。
pub const SHRED_LEDGER_BLOCKS: usize = 8;

/// 三重警示第三条：不可恢复声明（逐字，主册口径）。
pub const IRRECOVERABLE_TEXT: &str = "此操作不可恢复——不经过回收站";

/// 诚实标注文案（介质不支持覆写时逐字给出，锚 F439）。
pub const HONEST_OVERWRITE_NOTICE: &str = "此介质无法保证覆写，建议启用全盘加密 F439";

/// 诚实标注的锚点 ID。
pub const ANCHOR_F439: &str = "F439";

/// 入口形态（危险功能不占一级菜单——主册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKind {
    /// 折叠在「显示更多选项」内（唯一合法入口）。
    CollapsedMoreOptions,
    /// 一级菜单（拒绝）。
    TopLevel,
}

impl MenuKind {
    /// 入口是否合法。
    pub fn entry_allowed(self) -> bool {
        matches!(self, MenuKind::CollapsedMoreOptions)
    }
    pub fn name(self) -> &'static str {
        match self {
            MenuKind::CollapsedMoreOptions => "collapsed-more-options",
            MenuKind::TopLevel => "top-level",
        }
    }
}

/// 介质覆写能力（技术边界：U 盘磨损均衡使覆写不可保证——主册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    /// 可覆写介质（常规机械盘/全盘映像设备）。
    Overwritable,
    /// 磨损均衡介质（如 U 盘）——覆写尽力而为、不可保证，须诚实标注。
    Overprovisioned,
}

/// 确认对话框默认焦点。F207 铁律：危险确认默认焦点必须是取消。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogFocus {
    Cancel,
    Confirm,
}

/// 「彻底删除」确认对话框：三重警示状态 + 默认焦点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfirmDialog {
    /// 警示一：文件名已逐个列出。
    pub files_listed: bool,
    /// 警示二：数量核对（显示的条数）。
    pub count: usize,
    /// 警示三：不可恢复声明已展示（逐字 IRRECOVERABLE_TEXT）。
    pub irrecoverable_declared: bool,
    /// 默认焦点（F207：必须是取消）。
    pub focus: DialogFocus,
}

impl ConfirmDialog {
    /// 构造三重警示对话框：文件名已列、数量核对、不可恢复声明、
    /// 默认焦点=取消（F207 铁律在构造期即固化）。
    pub fn build(names: &[&'static str]) -> ConfirmDialog {
        ConfirmDialog {
            files_listed: !names.is_empty(),
            count: names.len(),
            irrecoverable_declared: true,
            focus: DialogFocus::Cancel,
        }
    }
    /// 三重警示齐备且数量与待粉碎文件一致。
    pub fn triple_warning_complete(&self, expected: usize) -> bool {
        self.files_listed && self.count == expected && self.irrecoverable_declared
    }
}

/// 粉碎失败原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShredError {
    /// 一级菜单入口被拒（危险功能不占一级菜单）。
    EntryNotFolded,
    /// 三重警示未齐备即试图执行。
    ConfirmIncomplete,
}

/// 粉碎结果两分支。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShredOutcome {
    /// 分支一：覆写执行完成（轮数 + 逐块账目块数）。
    Overwritten { rounds: u32, blocks: usize },
    /// 分支二：诚实标注（介质不支持覆写；文件仍按删除释放，但不承诺
    /// 覆写，文案锚 F439）。
    HonestNotice { anchor: &'static str },
}

/// 逐块覆写账目（每块记完成轮数，供执行后对账）。
pub struct ShredLedger {
    rounds_done: [u32; SHRED_LEDGER_BLOCKS],
    covered: usize,
}

impl ShredLedger {
    pub const fn new() -> Self {
        ShredLedger {
            rounds_done: [0; SHRED_LEDGER_BLOCKS],
            covered: 0,
        }
    }
    /// 记录 `blocks` 个块各完成 `rounds` 轮（超出账目容量的块不逐块记，
    /// 以总量口径执行——账目只承诺前 SHRED_LEDGER_BLOCKS 块）。
    pub fn record(&mut self, blocks: usize, rounds: u32) {
        self.covered = if blocks > SHRED_LEDGER_BLOCKS {
            SHRED_LEDGER_BLOCKS
        } else {
            blocks
        };
        let mut i = 0;
        while i < self.covered {
            self.rounds_done[i] = rounds;
            i += 1;
        }
    }
    /// 覆盖块数。
    pub fn covered(&self) -> usize {
        self.covered
    }
    /// 已覆盖块是否全部达到目标轮数（执行后对账）。
    pub fn all_blocks_covered(&self, rounds: u32) -> bool {
        if self.covered == 0 {
            return false;
        }
        let mut i = 0;
        while i < self.covered {
            if self.rounds_done[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 清零（诚实分支不留任何「已覆写」假账）。
    pub fn reset(&mut self) {
        *self = ShredLedger::new();
    }
}

/// 粉碎执行器：折叠入口闸门 → 三重警示确认 → 两分支执行。
/// 普通删除（F261 后悔药）与本器完全隔离：两个计数器互不污染。
pub struct Shredder {
    pending_count: usize,
    ledger: ShredLedger,
    normal_deletes: u32,
    shred_executions: u32,
    last_honest_anchor: &'static str,
}

impl Shredder {
    pub const fn new() -> Self {
        Shredder {
            pending_count: 0,
            ledger: ShredLedger::new(),
            normal_deletes: 0,
            shred_executions: 0,
            last_honest_anchor: "",
        }
    }

    /// 入口请求：折叠入口返回三重警示对话框；一级菜单直接拒绝。
    pub fn request(
        &mut self,
        menu: MenuKind,
        names: &[&'static str],
    ) -> Result<ConfirmDialog, ShredError> {
        if !menu.entry_allowed() {
            return Err(ShredError::EntryNotFolded);
        }
        self.ledger.reset();
        let dialog = ConfirmDialog::build(names);
        self.pending_count = names.len();
        Ok(dialog)
    }

    /// 执行：三重警示齐备才允许；按介质能力走两分支。
    pub fn execute(
        &mut self,
        dialog: &ConfirmDialog,
        bytes: u64,
        media: MediaKind,
    ) -> Result<ShredOutcome, ShredError> {
        if !dialog.triple_warning_complete(self.pending_count) {
            return Err(ShredError::ConfirmIncomplete);
        }
        self.shred_executions += 1;
        match media {
            MediaKind::Overwritable => {
                let blocks = bytes.div_ceil(SHRED_BLOCK_BYTES).max(1) as usize;
                self.ledger.record(blocks, SHRED_ROUNDS);
                Ok(ShredOutcome::Overwritten {
                    rounds: SHRED_ROUNDS,
                    blocks,
                })
            }
            MediaKind::Overprovisioned => {
                // 诚实分支：不记任何「已覆写」账（账目清零），文件本体仍
                // 按删除释放，但明示覆写不可保证并锚定 F439。
                self.ledger.reset();
                self.last_honest_anchor = ANCHOR_F439;
                Ok(ShredOutcome::HonestNotice { anchor: ANCHOR_F439 })
            }
        }
    }

    /// 普通删除（进回收站，F261 后悔药）——与粉碎互不污染的独立路径。
    pub fn normal_delete(&mut self) {
        self.normal_deletes += 1;
    }

    pub fn normal_deletes(&self) -> u32 {
        self.normal_deletes
    }
    pub fn shred_executions(&self) -> u32 {
        self.shred_executions
    }
    pub fn ledger(&self) -> &ShredLedger {
        &self.ledger
    }
    pub fn last_honest_anchor(&self) -> &'static str {
        self.last_honest_anchor
    }
    /// 覆写账目是否留有假账（诚实分支必须为 false）。
    pub fn ledger_claims_overwrite(&self) -> bool {
        self.ledger.covered() > 0
    }
}

/// 覆写耗时模型：预估毫秒数（吞吐常量 → bytes / (MB/s × 1000)）。
pub fn estimate_overwrite_ms(bytes: u64) -> u64 {
    let bytes_per_ms = OVERWRITE_THROUGHPUT_MBPS * 1_000;
    bytes.div_ceil(bytes_per_ms)
}

/// 「大文件覆写慢」提示判定：预估时长达到阈值即提示。
pub fn needs_slow_hint(bytes: u64) -> bool {
    estimate_overwrite_ms(bytes) >= SLOW_HINT_THRESHOLD_MS
}

// ---------------------------------------------------------------------------
// F509 域自检
// ---------------------------------------------------------------------------

/// F509 域自检（判据：折叠入口；三重警示与焦点；覆写执行或诚实标注两
/// 分支；普通删除不受影响；耗时提示）。
pub fn run_f509_checks() -> CheckSet {
    let mut cs = CheckSet::new("F509-file-shred");
    // 1) 入口折叠：折叠入口合法、一级菜单被拒（危险功能不占一级菜单）。
    let mut s = Shredder::new();
    cs.add(
        "entry_folded_allowed",
        s.request(MenuKind::CollapsedMoreOptions, &["report.docx"]).is_ok(),
        "",
    );
    let mut s2 = Shredder::new();
    cs.add(
        "entry_toplevel_rejected",
        matches!(
            s2.request(MenuKind::TopLevel, &["report.docx"]),
            Err(ShredError::EntryNotFolded)
        ),
        "",
    );
    // 2) 三重警示与焦点：构造即三警示齐备 + 默认焦点=取消（F207 铁律）。
    let dialog = ConfirmDialog::build(&["a.txt", "b.txt"]);
    cs.add(
        "triple_warning_and_f207_focus",
        dialog.files_listed
            && dialog.count == 2
            && dialog.irrecoverable_declared
            && dialog.focus == DialogFocus::Cancel,
        "",
    );
    // 3) 不可恢复声明逐字一致。
    cs.add("irrecoverable_text_verbatim", IRRECOVERABLE_TEXT == "此操作不可恢复——不经过回收站", "");
    // 4) 三重警示未齐备不得执行。
    let mut bad = ConfirmDialog::build(&["a.txt"]);
    bad.files_listed = false;
    let mut s_gate = Shredder::new();
    let _ = s_gate.request(MenuKind::CollapsedMoreOptions, &["a.txt"]);
    cs.add(
        "confirm_gate_enforced",
        matches!(
            s_gate.execute(&bad, 4096, MediaKind::Overwritable),
            Err(ShredError::ConfirmIncomplete)
        ),
        "",
    );
    // 5) 分支一（覆写执行）：轮数与逐块账目对账一致。
    let mut s3 = Shredder::new();
    let d3 = s3.request(MenuKind::CollapsedMoreOptions, &["x.bin"]).unwrap();
    let out = s3
        .execute(&d3, SHRED_BLOCK_BYTES * 3, MediaKind::Overwritable)
        .unwrap();
    cs.add(
        "overwrite_branch_rounds",
        out == ShredOutcome::Overwritten {
            rounds: SHRED_ROUNDS,
            blocks: 3,
        } && s3.ledger().all_blocks_covered(SHRED_ROUNDS)
            && s3.ledger().covered() == 3,
        "",
    );
    // 6) 分支二（诚实标注）：U 盘介质返回 HonestNotice 锚 F439。
    let mut s4 = Shredder::new();
    let d4 = s4.request(MenuKind::CollapsedMoreOptions, &["x.bin"]).unwrap();
    let out4 = s4.execute(&d4, 4096, MediaKind::Overprovisioned).unwrap();
    cs.add(
        "honest_branch_usb",
        out4 == ShredOutcome::HonestNotice { anchor: ANCHOR_F439 }
            && s4.last_honest_anchor() == "F439",
        "",
    );
    // 7) 诚实分支不留「已覆写」假账（账目清零）。
    cs.add(
        "honest_no_fake_ledger",
        !s4.ledger_claims_overwrite() && s4.ledger().covered() == 0,
        "",
    );
    // 8) 诚实文案逐字含锚点 F439（字节级扫描，零堆实现）。
    let notice_has_anchor = HONEST_OVERWRITE_NOTICE
        .as_bytes()
        .windows(ANCHOR_F439.len())
        .any(|w| w == ANCHOR_F439.as_bytes());
    cs.add("honest_notice_carries_f439", notice_has_anchor, "");
    // 9) 普通删除不受影响：两入口计数器互不污染（F261 后悔药保留）。
    let mut s5 = Shredder::new();
    s5.normal_delete();
    s5.normal_delete();
    s5.normal_delete();
    let d5 = s5.request(MenuKind::CollapsedMoreOptions, &["y.bin"]).unwrap();
    let _ = s5.execute(&d5, 8192, MediaKind::Overwritable);
    cs.add(
        "normal_delete_unaffected",
        s5.normal_deletes() == 3 && s5.shred_executions() == 1,
        "",
    );
    // 10) 覆写轮数常量：主册未规定，取单轮全量覆写。
    cs.add("rounds_single_pass", SHRED_ROUNDS == 1, "");
    // 11) 耗时模型：64MiB @32MB/s → 2098ms（div_ceil 对账）。
    cs.add("time_model_64mib", estimate_overwrite_ms(64 << 20) == 2_098, "");
    // 12) 慢提示阈值边界：恰好 3000ms 触发、2999ms 不触发。
    cs.add(
        "slow_hint_threshold_boundary",
        needs_slow_hint(96_000_000) && !needs_slow_hint(95_968_000),
        "",
    );
    // 13) 账目块数学：1 字节 → 至少 1 块。
    let mut s6 = Shredder::new();
    let d6 = s6.request(MenuKind::CollapsedMoreOptions, &["z.bin"]).unwrap();
    let _ = s6.execute(&d6, 1, MediaKind::Overwritable);
    cs.add("block_math_min_one", s6.ledger().covered() == 1, "");
    cs
}

#[cfg(test)]
mod f509_tests {
    use super::*;

    #[test]
    fn top_level_entry_is_rejected() {
        // 危险功能不占一级菜单：一级菜单入口必须被拒。
        let mut s = Shredder::new();
        assert_eq!(
            s.request(MenuKind::TopLevel, &["a.txt"]),
            Err(ShredError::EntryNotFolded),
            "一级菜单入口必须拒绝"
        );
        assert!(
            s.request(MenuKind::CollapsedMoreOptions, &["a.txt"]).is_ok(),
            "折叠入口合法"
        );
    }

    #[test]
    fn default_focus_is_cancel_f207() {
        // F207 铁律：危险确认默认焦点必须是取消。
        let d = ConfirmDialog::build(&["a.txt"]);
        assert_eq!(d.focus, DialogFocus::Cancel, "默认焦点必须是取消");
        assert!(d.irrecoverable_declared, "不可恢复声明必须默认展示");
        assert_eq!(d.count, 1, "数量核对必须等于文件数");
    }

    #[test]
    fn overwrite_ledger_per_block() {
        // 分支一：覆写执行后逐块账目全部达到目标轮数。
        let mut s = Shredder::new();
        let d = s.request(MenuKind::CollapsedMoreOptions, &["a", "b"]).unwrap();
        let out = s
            .execute(&d, SHRED_BLOCK_BYTES * 5, MediaKind::Overwritable)
            .unwrap();
        assert_eq!(
            out,
            ShredOutcome::Overwritten { rounds: 1, blocks: 5 },
            "5 块单轮覆写"
        );
        assert!(s.ledger().all_blocks_covered(SHRED_ROUNDS), "逐块账目对账通过");
        // 超账目容量封顶：9 块 → 账目记 8 块（总量口径仍 9）。
        let d2 = s.request(MenuKind::CollapsedMoreOptions, &["c"]).unwrap();
        let out2 = s
            .execute(&d2, SHRED_BLOCK_BYTES * 9, MediaKind::Overwritable)
            .unwrap();
        assert_eq!(out2, ShredOutcome::Overwritten { rounds: 1, blocks: 9 }, "总量口径 9 块");
        assert_eq!(s.ledger().covered(), SHRED_LEDGER_BLOCKS, "账目封顶 8 块");
    }

    #[test]
    fn honest_notice_on_usb_media() {
        // 分支二：U 盘（磨损均衡）诚实标注、锚 F439、不留假账。
        let mut s = Shredder::new();
        let d = s.request(MenuKind::CollapsedMoreOptions, &["a.txt"]).unwrap();
        let out = s.execute(&d, 1 << 20, MediaKind::Overprovisioned).unwrap();
        assert_eq!(out, ShredOutcome::HonestNotice { anchor: "F439" }, "U 盘必须诚实标注");
        assert!(HONEST_OVERWRITE_NOTICE.contains("F439"), "文案必须锚定 F439");
        assert!(!s.ledger_claims_overwrite(), "诚实分支不得留「已覆写」假账");
    }

    #[test]
    fn normal_delete_and_shred_are_isolated() {
        // 普通删除（F261 后悔药）与粉碎互不污染。
        let mut s = Shredder::new();
        for _ in 0..7 {
            s.normal_delete();
        }
        let d = s.request(MenuKind::CollapsedMoreOptions, &["a"]).unwrap();
        let _ = s.execute(&d, 1, MediaKind::Overwritable);
        assert_eq!(s.normal_deletes(), 7, "粉碎不得影响普通删除计数");
        assert_eq!(s.shred_executions(), 1, "普通删除不得影响粉碎计数");
    }

    #[test]
    fn large_file_slow_hint_boundary() {
        // 耗时提示：3 秒阈值边界（主册未规定数值，取 3s）。
        assert_eq!(SLOW_HINT_THRESHOLD_MS, 3_000, "阈值常量 3 秒");
        // 96_000_000 B @32000 B/ms = 恰 3000ms → 触发。
        assert!(needs_slow_hint(96_000_000), "达到阈值必须提示大文件覆写慢");
        // 95_968_000 B = 恰 2999ms → 不触发。
        assert!(!needs_slow_hint(95_968_000), "低于阈值不得打扰");
        // 512MB 大文件预估 16384ms，必提示。
        assert!(needs_slow_hint(512 << 20), "大文件必须提示");
    }

    #[test]
    fn confirm_gate_blocks_incomplete_dialog() {
        // 三重警示未齐备不得执行（文件名列示被剥除的对话框拒绝）。
        let mut s = Shredder::new();
        let d = s.request(MenuKind::CollapsedMoreOptions, &["a.txt"]).unwrap();
        let mut bad = d;
        bad.files_listed = false;
        assert_eq!(
            s.execute(&bad, 4096, MediaKind::Overwritable),
            Err(ShredError::ConfirmIncomplete),
            "警示不齐备不得执行"
        );
        // 数量核对不符同样拒绝。
        let d2 = s
            .request(MenuKind::CollapsedMoreOptions, &["a.txt", "b.txt"])
            .unwrap();
        let mut mismatch = d2;
        mismatch.count = 1;
        assert_eq!(
            s.execute(&mismatch, 4096, MediaKind::Overwritable),
            Err(ShredError::ConfirmIncomplete),
            "数量核对不符不得执行"
        );
    }
}

// ===========================================================================
// F510 单文件加密（.vxcrypt）
// ===========================================================================

/// blob 明文容量上限（建模；真实实现按块流式）。
pub const BLOB_CAP: usize = 128;
/// 临时视图缓冲容量。
pub const VIEW_CAP: usize = 128;
/// 临时视图槽位数。
pub const MAX_TEMP_VIEWS: usize = 4;
/// 认证标记长度（字节）。
pub const TAG_LEN: usize = 8;
/// 文件夹打包最大条目数。
pub const PACK_MAX_ENTRIES: usize = 8;
/// .vxcrypt 魔数。
pub const VX_MAGIC: [u8; 8] = *b"VXCRYPT1";

/// 密码错误/认证失败的三要素提示（逐字）。
pub const WRONG_PW_HINTS: [&'static str; 3] = [
    "密码错误",
    "内容可能被篡改",
    "没有后门——密码丢了就是真丢了",
];

/// 导出门文案（逐字）。
pub const EXPORT_NEEDS_EXPLICIT: &str = "导出解密副本需明确选择";

/// 原文件去留默认提示（默认不保留，提示一次）。
pub const KEEP_ORIGINAL_PROMPT: &str = "原文件已删除（默认不保留）；如需保留请在加密时选择保留";

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv64(seed: u64, data: &[u8]) -> u64 {
    let mut h = seed ^ FNV_OFFSET;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

fn fnv64_u64(seed: u64, v: u64) -> u64 {
    let mut h = seed ^ FNV_OFFSET;
    let mut x = v;
    let mut i = 0;
    while i < 8 {
        h ^= x & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        x >>= 8;
        i += 1;
    }
    h
}

/// 密钥派生（FNV 占位——结构完整：密码+盐拉伸 8 轮；真实实现走 MD1
/// 密码学栈）。同密码同盐必得同钥；盐变钥变。
pub fn kdf(password: &str, salt: u64) -> u64 {
    let mut key = fnv64(salt, password.as_bytes());
    let mut i = 0;
    while i < 8 {
        key = fnv64_u64(key, salt ^ (i as u64));
        i += 1;
    }
    key
}

/// 加密引擎错误态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VxCryptError {
    /// 非法 .vxcrypt（魔数不符）。
    NotVxCrypt,
    /// 明文超出 blob 容量。
    PlainTooLarge,
    /// 文件夹打包条目超上限。
    PackTooMany,
    /// 输出缓冲不足。
    OutTooSmall,
    /// 认证失败：密码错误或内容被篡改（同一错误态，不提供区分预言机；
    /// 明确报错不静默）。
    AuthFailed,
    /// 临时视图槽位已满。
    NoFreeView,
    /// 视图不存在。
    NoSuchView,
    /// 导出未获明确选择（默认拒绝）。
    ExportNotExplicit,
}

/// .vxcrypt 单文件 blob（魔数 + 盐 + 密文 + 认证标记）。
#[derive(Clone, Copy)]
pub struct VxBlob {
    pub magic: [u8; 8],
    pub salt: u64,
    /// true = 文件夹整体打包；false = 单文件。
    pub is_folder: bool,
    pub len: usize,
    pub data: [u8; BLOB_CAP],
    pub tag: [u8; TAG_LEN],
}

impl VxBlob {
    pub const fn new() -> Self {
        VxBlob {
            magic: [0; 8],
            salt: 0,
            is_folder: false,
            len: 0,
            data: [0; BLOB_CAP],
            tag: [0; TAG_LEN],
        }
    }
    pub fn magic_ok(&self) -> bool {
        let mut i = 0;
        while i < 8 {
            if self.magic[i] != VX_MAGIC[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 原文件去留（判据：加密后原文件可选保留，默认不保留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeepOriginal {
    /// 默认：不保留（更安全）。
    Discard,
    Keep,
}

impl KeepOriginal {
    pub const fn default_keep() -> Self {
        KeepOriginal::Discard
    }
}

/// 文件夹打包条目（连续布局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackEntry {
    pub name: &'static str,
    pub off: u32,
    pub len: u32,
}

/// 文件夹打包表：条目按序连续布局，整体加密为 .vxcrypt 单文件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackTable {
    pub entries: [Option<PackEntry>; PACK_MAX_ENTRIES],
    pub n: usize,
}

impl PackTable {
    pub const fn new() -> Self {
        PackTable {
            entries: [None; PACK_MAX_ENTRIES],
            n: 0,
        }
    }
    /// 打包布局：条目偏移连续无洞，总长 = 各条目长度之和。
    pub fn pack(names: &[&'static str], lens: &[u32]) -> Result<PackTable, VxCryptError> {
        if names.len() != lens.len() || names.len() > PACK_MAX_ENTRIES {
            return Err(VxCryptError::PackTooMany);
        }
        let mut t = PackTable::new();
        let mut off = 0u32;
        let mut i = 0;
        while i < names.len() {
            t.entries[i] = Some(PackEntry {
                name: names[i],
                off,
                len: lens[i],
            });
            off += lens[i];
            t.n += 1;
            i += 1;
        }
        Ok(t)
    }
    /// 打包明文总长。
    pub fn total_bytes(&self) -> u32 {
        let mut total = 0u32;
        let mut i = 0;
        while i < self.n {
            if let Some(e) = self.entries[i] {
                total += e.len;
            }
            i += 1;
        }
        total
    }
    /// 布局连续性对账：第 i+1 条偏移 = 第 i 条偏移+长度。
    pub fn layout_contiguous(&self) -> bool {
        let mut i = 0;
        while i + 1 < self.n {
            match (self.entries[i], self.entries[i + 1]) {
                (Some(a), Some(b)) => {
                    if b.off != a.off + a.len {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 临时视图槽位（阅后即焚：关闭即清零——无痕判据）。
#[derive(Clone, Copy)]
pub struct TempView {
    id: u32,
    len: usize,
    buf: [u8; VIEW_CAP],
    open: bool,
}

impl TempView {
    const fn empty() -> Self {
        TempView {
            id: 0,
            len: 0,
            buf: [0; VIEW_CAP],
            open: false,
        }
    }
    fn zero(&mut self) {
        let mut i = 0;
        while i < VIEW_CAP {
            self.buf[i] = 0;
            i += 1;
        }
        self.len = 0;
    }
}

/// VxCrypt 引擎（纯逻辑建模）：三链路（加密/浏览/导出）+ 原文件去留。
/// 结构自证无后门：解密唯一入口 `unseal`，唯一钥源 `kdf(password, salt)`
/// ——不存在任何绕过密码的恢复路径（`has_backdoor` 恒 false）。
pub struct VxCryptEngine {
    salt_counter: u64,
    views: [TempView; MAX_TEMP_VIEWS],
    next_view_id: u32,
    keep_original: KeepOriginal,
    keep_prompt_pending: bool,
    keep_prompt_shown: u32,
    last_export_refused: bool,
}

impl VxCryptEngine {
    pub const fn new() -> Self {
        VxCryptEngine {
            salt_counter: 1,
            views: [TempView::empty(); MAX_TEMP_VIEWS],
            next_view_id: 1,
            keep_original: KeepOriginal::default_keep(),
            keep_prompt_pending: false,
            keep_prompt_shown: 0,
            last_export_refused: false,
        }
    }

    /// 链路一·加密：明文封入 blob（密码派生钥 + 异或流占位 + 认证标记）。
    pub fn seal(
        &mut self,
        password: &str,
        plain: &[u8],
        is_folder: bool,
        out: &mut VxBlob,
    ) -> Result<(), VxCryptError> {
        if plain.len() > BLOB_CAP {
            return Err(VxCryptError::PlainTooLarge);
        }
        // 盐：引擎单调计数保证同密码两次封印也得不同盐（防同钥重放）。
        let salt = self.salt_counter;
        self.salt_counter += 1;
        let key = kdf(password, salt);
        *out = VxBlob::new();
        out.magic = VX_MAGIC;
        out.salt = salt;
        out.is_folder = is_folder;
        out.len = plain.len();
        // 异或流占位：逐字节滚动 FNV 作密钥流。
        let mut ks = key;
        let mut i = 0;
        while i < plain.len() {
            ks = fnv64_u64(ks, i as u64);
            out.data[i] = plain[i] ^ (ks as u8);
            i += 1;
        }
        // 认证标记：钥混密文再混钥（占位 MAC——结构完整即可）。
        let tag_h = fnv64_u64(fnv64(key, &out.data[..out.len]), key);
        out.tag = tag_h.to_le_bytes();
        Ok(())
    }

    /// 解密唯一入口：认证先行，失败即清候选明文并明确报错（不静默）。
    pub fn unseal(
        &self,
        password: &str,
        blob: &VxBlob,
        out: &mut [u8; VIEW_CAP],
    ) -> Result<usize, VxCryptError> {
        if !blob.magic_ok() {
            return Err(VxCryptError::NotVxCrypt);
        }
        if out.len() < blob.len {
            return Err(VxCryptError::OutTooSmall);
        }
        let key = kdf(password, blob.salt);
        let mut ks = key;
        let mut i = 0;
        while i < blob.len {
            ks = fnv64_u64(ks, i as u64);
            out[i] = blob.data[i] ^ (ks as u8);
            i += 1;
        }
        let tag_h = fnv64_u64(fnv64(key, &blob.data[..blob.len]), key);
        if tag_h.to_le_bytes() != blob.tag {
            // 无痕纪律：认证失败，候选明文立即清零，不留半份明文。
            let mut j = 0;
            while j < VIEW_CAP {
                out[j] = 0;
                j += 1;
            }
            return Err(VxCryptError::AuthFailed);
        }
        Ok(blob.len)
    }

    /// 链路二·浏览：解出临时视图句柄（不落盘）。关闭即隐由 `close_view`
    /// 兑现（临时区清零对账见 `temp_area_clean`）。
    pub fn open_view(&mut self, password: &str, blob: &VxBlob) -> Result<u32, VxCryptError> {
        let mut slot = MAX_TEMP_VIEWS;
        let mut i = 0;
        while i < MAX_TEMP_VIEWS {
            if !self.views[i].open {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot == MAX_TEMP_VIEWS {
            return Err(VxCryptError::NoFreeView);
        }
        let mut stage = [0u8; VIEW_CAP];
        let len = self.unseal(password, blob, &mut stage)?;
        let id = self.next_view_id;
        self.next_view_id += 1;
        let v = &mut self.views[slot];
        v.id = id;
        v.len = len;
        let mut j = 0;
        while j < len {
            v.buf[j] = stage[j];
            j += 1;
        }
        v.open = true;
        // 中转缓冲即焚。
        let mut k = 0;
        while k < VIEW_CAP {
            stage[k] = 0;
            k += 1;
        }
        Ok(id)
    }

    /// 关闭视图：缓冲清零 + 槽位释放（无痕判据的执行点）。
    pub fn close_view(&mut self, id: u32) -> Result<(), VxCryptError> {
        let mut i = 0;
        while i < MAX_TEMP_VIEWS {
            if self.views[i].open && self.views[i].id == id {
                self.views[i].zero();
                self.views[i].open = false;
                return Ok(());
            }
            i += 1;
        }
        Err(VxCryptError::NoSuchView)
    }

    /// 无痕对账：所有槽位关闭且缓冲全零。
    pub fn temp_area_clean(&self) -> bool {
        let mut i = 0;
        while i < MAX_TEMP_VIEWS {
            if self.views[i].open {
                return false;
            }
            let mut j = 0;
            while j < VIEW_CAP {
                if self.views[i].buf[j] != 0 {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    pub fn open_view_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < MAX_TEMP_VIEWS {
            if self.views[i].open {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 链路三·导出：解密副本必须显式选择才落盘（默认拒绝）。
    pub fn export_view(
        &mut self,
        id: u32,
        explicit: bool,
        out: &mut [u8],
    ) -> Result<usize, VxCryptError> {
        if !explicit {
            self.last_export_refused = true;
            return Err(VxCryptError::ExportNotExplicit);
        }
        let mut i = 0;
        while i < MAX_TEMP_VIEWS {
            if self.views[i].open && self.views[i].id == id {
                if out.len() < self.views[i].len {
                    return Err(VxCryptError::OutTooSmall);
                }
                let mut j = 0;
                while j < self.views[i].len {
                    out[j] = self.views[i].buf[j];
                    j += 1;
                }
                self.last_export_refused = false;
                return Ok(self.views[i].len);
            }
            i += 1;
        }
        Err(VxCryptError::NoSuchView)
    }

    pub fn last_export_refused(&self) -> bool {
        self.last_export_refused
    }

    /// 原文件去留设置：选保留即挂起一次性提示（默认不保留）。
    pub fn set_keep_original(&mut self, keep: KeepOriginal) {
        self.keep_original = keep;
        self.keep_prompt_pending = keep == KeepOriginal::Keep;
    }

    pub fn keep_original(&self) -> KeepOriginal {
        self.keep_original
    }

    /// 提示一次：首次调用返回提示文案并清挂起；再次调用返回 None
    /// （判据「提示一次」的对账点）。
    pub fn take_keep_prompt(&mut self) -> Option<&'static str> {
        if self.keep_prompt_pending {
            self.keep_prompt_pending = false;
            self.keep_prompt_shown += 1;
            Some(KEEP_ORIGINAL_PROMPT)
        } else {
            None
        }
    }

    pub fn keep_prompt_shown(&self) -> u32 {
        self.keep_prompt_shown
    }

    /// 结构自证：无后门。解密唯一路径 = kdf(密码, 盐)；不存在恢复密钥
    /// 后门（编译期即无第二解密入口，运行期此断言入总检）。
    pub fn has_backdoor(&self) -> bool {
        false
    }
}

/// 无后门全局常量（与引擎方法互为印证，一处一事实）。
pub const NO_BACKDOOR: bool = true;

// ---------------------------------------------------------------------------
// F510 域自检
// ---------------------------------------------------------------------------

/// F510 域自检（判据：加密/浏览/导出三链路；临时视图无痕判据；原文件
/// 去留；密码错误提示；文件夹打包）。
pub fn run_f510_checks() -> CheckSet {
    let mut cs = CheckSet::new("F510-file-vxcrypt");
    // 1) KDF 确定性：同密码同盐同钥。
    cs.add("kdf_deterministic", kdf("pw", 7) == kdf("pw", 7), "");
    // 2) KDF 盐敏感：盐变钥变（解密唯一路径=密码派生的前提）。
    cs.add("kdf_salt_sensitive", kdf("pw", 7) != kdf("pw", 8), "");
    // 3) 魔数常量。
    cs.add("magic_constant", VX_MAGIC == *b"VXCRYPT1", "");
    // 4) 链路一·加密 → 解出：往返一致。
    let mut eng = VxCryptEngine::new();
    let mut blob = VxBlob::new();
    let plain: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let mut stage = [0u8; VIEW_CAP];
    let sealed = eng.seal("s3cret", &plain, false, &mut blob);
    let opened = sealed.is_ok() && blob.magic_ok() && blob.len == 16;
    let roundtrip = eng.unseal("s3cret", &blob, &mut stage) == Ok(16)
        && stage[..16] == plain[..];
    cs.add("seal_unseal_roundtrip", opened && roundtrip, "");
    // 5) 篡改检测：密文翻一字节 → 认证失败且候选明文清零（无痕 + 不静默）。
    let mut tampered = VxBlob::new();
    let _ = eng.seal("s3cret", &plain, false, &mut tampered);
    tampered.data[3] ^= 0xff;
    let mut sink = [0u8; VIEW_CAP];
    let tamper_result = eng.unseal("s3cret", &tampered, &mut sink);
    let sink_clean = sink.iter().all(|&b| b == 0);
    cs.add(
        "tamper_detected_and_cleaned",
        tamper_result == Err(VxCryptError::AuthFailed) && sink_clean,
        "",
    );
    // 6) 密码错误：同一错误态（不提供区分预言机），三要素提示齐备。
    let mut blob2 = VxBlob::new();
    let _ = eng.seal("right", &plain, false, &mut blob2);
    let wrong_pw = eng.unseal("wrong", &blob2, &mut sink);
    cs.add(
        "wrong_password_same_auth_error",
        wrong_pw == Err(VxCryptError::AuthFailed)
            && WRONG_PW_HINTS.len() == 3
            && WRONG_PW_HINTS[2].contains("没有后门"),
        "",
    );
    // 7) 链路二·浏览：开视图 → 关视图 → 临时区清零（无痕判据）。
    let mut eng3 = VxCryptEngine::new();
    let mut blob3 = VxBlob::new();
    let _ = eng3.seal("pw", &plain, false, &mut blob3);
    let vid = eng3.open_view("pw", &blob3).unwrap();
    cs.add(
        "view_open",
        eng3.open_view_count() == 1 && !eng3.temp_area_clean(),
        "",
    );
    let closed = eng3.close_view(vid);
    cs.add("view_close_temps_zeroed", closed.is_ok() && eng3.temp_area_clean(), "");
    // 8) 视图容量：第 5 个视图诚实报满（MAX_TEMP_VIEWS=4）。
    let mut eng4 = VxCryptEngine::new();
    let mut blob4 = VxBlob::new();
    let _ = eng4.seal("pw", &plain, false, &mut blob4);
    let mut opened_n = 0;
    let mut fifth_err = false;
    let mut i = 0;
    while i < 5 {
        match eng4.open_view("pw", &blob4) {
            Ok(_) => opened_n += 1,
            Err(VxCryptError::NoFreeView) => fifth_err = true,
            Err(_) => {}
        }
        i += 1;
    }
    cs.add("view_capacity_honest", opened_n == MAX_TEMP_VIEWS && fifth_err, "");
    // 9) 链路三·导出：未明确选择默认拒绝；明确选择才出副本。
    let mut eng5 = VxCryptEngine::new();
    let mut blob5 = VxBlob::new();
    let _ = eng5.seal("pw", &plain, false, &mut blob5);
    let v5 = eng5.open_view("pw", &blob5).unwrap();
    let mut out5 = [0u8; VIEW_CAP];
    let refused = eng5.export_view(v5, false, &mut out5) == Err(VxCryptError::ExportNotExplicit)
        && eng5.last_export_refused();
    let granted = eng5.export_view(v5, true, &mut out5) == Ok(16)
        && out5[..16] == plain[..]
        && !eng5.last_export_refused();
    cs.add("export_gate_explicit_only", refused && granted, "");
    // 10) 原文件去留：默认不保留；选保留提示恰好一次。
    let mut eng6 = VxCryptEngine::new();
    let default_discard = eng6.keep_original() == KeepOriginal::Discard
        && eng6.take_keep_prompt().is_none();
    eng6.set_keep_original(KeepOriginal::Keep);
    let first = eng6.take_keep_prompt();
    let second = eng6.take_keep_prompt();
    cs.add(
        "keep_original_discard_default_prompt_once",
        default_discard && first.is_some() && second.is_none() && eng6.keep_prompt_shown() == 1,
        "",
    );
    // 11) 文件夹打包：条目连续布局、总长一致、is_folder 标记入 blob。
    let names = ["a.txt", "b.txt", "c.txt"];
    let lens = [4u32, 8, 2];
    let table = PackTable::pack(&names, &lens).unwrap();
    let layout_ok = table.layout_contiguous() && table.total_bytes() == 14;
    let mut blob6 = VxBlob::new();
    let packed_plain = [0u8; 14];
    let _ = eng6.seal("pw", &packed_plain, true, &mut blob6);
    cs.add(
        "folder_pack_single_blob",
        layout_ok && blob6.is_folder && blob6.magic_ok(),
        "",
    );
    // 12) 无后门结构自证。
    cs.add("no_backdoor_structural", NO_BACKDOOR && !eng6.has_backdoor(), "");
    // 13) 明文超容量诚实报错（不静默截断）。
    let mut big_blob = VxBlob::new();
    let big = [0u8; BLOB_CAP + 1];
    cs.add(
        "plain_too_large_honest",
        eng6.seal("pw", &big, false, &mut big_blob) == Err(VxCryptError::PlainTooLarge),
        "",
    );
    cs
}

#[cfg(test)]
mod f510_tests {
    use super::*;

    #[test]
    fn seal_unseal_roundtrip() {
        let mut eng = VxCryptEngine::new();
        let mut blob = VxBlob::new();
        let plain: [u8; 32] = core::array::from_fn(|i| (i * 7 + 3) as u8);
        eng.seal("hunter2", &plain, false, &mut blob).unwrap();
        let mut out = [0u8; VIEW_CAP];
        assert_eq!(eng.unseal("hunter2", &blob, &mut out), Ok(32), "往返长度一致");
        assert_eq!(&out[..32], &plain[..], "往返内容一致");
    }

    #[test]
    fn tamper_detection_is_mandatory() {
        // 必测：篡改密文任意字节必须认证失败且不静默。
        let mut eng = VxCryptEngine::new();
        let mut blob = VxBlob::new();
        let plain = [0xAAu8; 40];
        eng.seal("pw", &plain, false, &mut blob).unwrap();
        // 翻转密文中部。
        let mut t = blob;
        t.data[20] ^= 0x01;
        let mut out = [0u8; VIEW_CAP];
        assert_eq!(
            eng.unseal("pw", &t, &mut out),
            Err(VxCryptError::AuthFailed),
            "篡改必须被认证拦截"
        );
        assert!(out.iter().all(|&b| b == 0), "认证失败候选明文必须清零（无痕）");
        // 篡改认证标记同样拦截。
        let mut t2 = blob;
        t2.tag[0] ^= 0x80;
        let mut out2 = [0u8; VIEW_CAP];
        assert_eq!(
            eng.unseal("pw", &t2, &mut out2),
            Err(VxCryptError::AuthFailed),
            "标记被篡改同样拦截"
        );
    }

    #[test]
    fn wrong_password_matches_tamper_error() {
        // 密码错误与篡改同一错误态：不提供区分预言机（防逐字节试探）。
        let mut eng = VxCryptEngine::new();
        let mut blob = VxBlob::new();
        eng.seal("right", &[1, 2, 3], false, &mut blob).unwrap();
        let mut out = [0u8; VIEW_CAP];
        assert_eq!(eng.unseal("wrong", &blob, &mut out), Err(VxCryptError::AuthFailed));
        assert_eq!(eng.unseal("", &blob, &mut out), Err(VxCryptError::AuthFailed));
        // 三要素提示逐条齐备。
        assert_eq!(WRONG_PW_HINTS.len(), 3, "三要素提示");
        assert!(WRONG_PW_HINTS[0].contains("密码错误"));
        assert!(WRONG_PW_HINTS[1].contains("篡改"));
        assert!(WRONG_PW_HINTS[2].contains("没有后门"));
    }

    #[test]
    fn temp_area_clean_after_close() {
        // 无痕判据：关闭后临时区清零；多视图逐个关闭同样干净。
        let mut eng = VxCryptEngine::new();
        let mut blob = VxBlob::new();
        eng.seal("pw", &[9u8; 64], false, &mut blob).unwrap();
        let v1 = eng.open_view("pw", &blob).unwrap();
        let v2 = eng.open_view("pw", &blob).unwrap();
        assert!(!eng.temp_area_clean(), "视图开着不得算干净");
        eng.close_view(v1).unwrap();
        eng.close_view(v2).unwrap();
        assert!(eng.temp_area_clean(), "关闭后临时区必须清零");
        // 关不存在的视图诚实报错。
        assert_eq!(eng.close_view(999), Err(VxCryptError::NoSuchView));
    }

    #[test]
    fn export_requires_explicit_choice() {
        let mut eng = VxCryptEngine::new();
        let mut blob = VxBlob::new();
        let plain = [0x5Au8; 24];
        eng.seal("pw", &plain, false, &mut blob).unwrap();
        let v = eng.open_view("pw", &blob).unwrap();
        let mut out = [0u8; VIEW_CAP];
        assert_eq!(
            eng.export_view(v, false, &mut out),
            Err(VxCryptError::ExportNotExplicit),
            "导出默认拒绝"
        );
        assert!(eng.last_export_refused());
        assert_eq!(eng.export_view(v, true, &mut out), Ok(24), "明确选择才出副本");
        assert_eq!(&out[..24], &plain[..]);
    }

    #[test]
    fn keep_original_prompt_fires_once() {
        // 默认不保留；选保留提示恰好一次（不是每次都念）。
        let mut eng = VxCryptEngine::new();
        assert_eq!(eng.keep_original(), KeepOriginal::Discard, "默认不保留");
        assert!(eng.take_keep_prompt().is_none(), "默认无提示");
        eng.set_keep_original(KeepOriginal::Keep);
        assert!(eng.take_keep_prompt().is_some(), "保留须提示一次");
        assert!(eng.take_keep_prompt().is_none(), "提示不得重复");
        assert_eq!(eng.keep_prompt_shown(), 1);
        // 改回默认不再提示。
        eng.set_keep_original(KeepOriginal::Discard);
        assert!(eng.take_keep_prompt().is_none());
    }

    #[test]
    fn folder_pack_layout_and_single_blob() {
        // 文件夹整体打包：布局连续、总长对账、加密为 .vxcrypt 单文件。
        let names = ["doc", "img", "log", "cfg"];
        let lens = [12u32, 40, 8, 4];
        let t = PackTable::pack(&names, &lens).unwrap();
        assert_eq!(t.n, 4);
        assert!(t.layout_contiguous(), "条目偏移必须连续无洞");
        assert_eq!(t.total_bytes(), 64, "总长 = 各条目之和");
        let e1 = t.entries[1].unwrap();
        assert_eq!((e1.name, e1.off, e1.len), ("img", 12, 40), "第二条布局正确");
        // 超上限诚实报错。
        let many = ["a"; PACK_MAX_ENTRIES + 1];
        let manyl = [1u32; PACK_MAX_ENTRIES + 1];
        assert_eq!(PackTable::pack(&many, &manyl), Err(VxCryptError::PackTooMany));
        // 打包整体封入单 blob 且带 is_folder 标记。
        let mut eng = VxCryptEngine::new();
        let mut blob = VxBlob::new();
        let packed = [0u8; 64];
        eng.seal("pw", &packed, true, &mut blob).unwrap();
        assert!(blob.is_folder && blob.magic_ok(), "打包产物是带标记的单 .vxcrypt");
    }

    #[test]
    fn no_backdoor_and_salt_freshness() {
        // 无后门：结构自证——唯一解密入口 unseal，唯一钥源 kdf(pw, salt)。
        assert!(NO_BACKDOOR);
        let mut eng = VxCryptEngine::new();
        assert!(!eng.has_backdoor());
        // 同密码两次封印盐必不同 → 密文不同（防同钥重放）。
        let mut b1 = VxBlob::new();
        let mut b2 = VxBlob::new();
        let plain = [7u8; 16];
        eng.seal("same", &plain, false, &mut b1).unwrap();
        eng.seal("same", &plain, false, &mut b2).unwrap();
        assert_ne!(b1.salt, b2.salt, "同密码两次封印必须换盐");
        assert_ne!(b1.data[..16], b2.data[..16], "同密码同明文密文必须不同");
        // 换盐后旧 blob 仍可用原密码解开（盐随 blob 存储，不是撤销）。
        let mut out = [0u8; VIEW_CAP];
        assert_eq!(eng.unseal("same", &b1, &mut out), Ok(16));
    }
}

// ===========================================================================
// F511 剪贴板一键清空
// ===========================================================================

/// 剪贴板历史环容量。主册未规定历史条数，建模定长 8（对齐 F109 历史环
/// 的行为语义：淘汰最旧）。
pub const CLIP_HISTORY_CAP: usize = 8;
/// 单条剪贴载荷建模上限。
pub const CLIP_DATA_CAP: usize = 32;
/// 敏感提示窗时长：判据精确口径 5 秒。
pub const SENSITIVE_PROMPT_MS: u64 = 5_000;
/// 敏感提示条文案（逐字）。
pub const SENSITIVE_PROMPT_TEXT: &str = "剪贴板仍含有复制的内容——清空？";
/// 快捷键（逐字）。
pub const HOTKEY_CTRL_SHIFT_DELETE: &str = "Ctrl+Shift+Delete";

/// 双入口（快速设置磁贴 F076 / 快捷键）——同一执行函数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WipeEntryKind {
    QuickSettingsTile,
    Hotkey,
}

impl WipeEntryKind {
    pub fn name(self) -> &'static str {
        match self {
            WipeEntryKind::QuickSettingsTile => "quick-settings-tile",
            WipeEntryKind::Hotkey => "ctrl-shift-delete",
        }
    }
}

/// 清空报告（确认框条数的来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WipeReport {
    /// 当前剪贴板是否被清。
    pub current: bool,
    /// 历史环清掉的条数。
    pub history: usize,
    /// 总条数（= 当前 + 历史，确认框列条数）。
    pub total: usize,
}

/// 粘贴读取结果：空是诚实失败（应用得到真实的空），不是假数据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipRead {
    Empty,
    Data { len: usize },
}

/// 单条剪贴项（定长载荷）。
#[derive(Clone, Copy)]
pub struct ClipItem {
    pub len: usize,
    pub data: [u8; CLIP_DATA_CAP],
}

/// 剪贴板模型：当前槽 + 定长历史环 + 敏感提示时序状态。
pub struct Clipboard {
    current: Option<ClipItem>,
    history: [Option<ClipItem>; CLIP_HISTORY_CAP],
    history_head: usize,
    history_len: usize,
    /// 敏感提示已武装（密码复制事件触发）。
    prompt_armed: bool,
    /// 提示窗截止时刻（armed 时刻 + SENSITIVE_PROMPT_MS）。
    prompt_deadline_ms: u64,
    /// 到时未清 → 提示过期（一次性，不烦人）。
    prompt_expired: bool,
    prompt_shown: u32,
    wipe_count: u32,
}

impl Clipboard {
    pub const fn new() -> Self {
        Clipboard {
            current: None,
            history: [None; CLIP_HISTORY_CAP],
            history_head: 0,
            history_len: 0,
            prompt_armed: false,
            prompt_deadline_ms: 0,
            prompt_expired: false,
            prompt_shown: 0,
            wipe_count: 0,
        }
    }

    /// 复制进剪贴板：旧当前项进历史环（满则淘汰最旧）。
    /// 超载荷上限诚实拒绝（不静默截断）。
    pub fn copy(&mut self, data: &[u8]) -> bool {
        if data.len() > CLIP_DATA_CAP {
            return false;
        }
        let mut item = ClipItem {
            len: data.len(),
            data: [0; CLIP_DATA_CAP],
        };
        let mut i = 0;
        while i < data.len() {
            item.data[i] = data[i];
            i += 1;
        }
        if let Some(old) = self.current.take() {
            self.history_push(old);
        }
        self.current = Some(item);
        true
    }

    fn history_push(&mut self, item: ClipItem) {
        if self.history_len < CLIP_HISTORY_CAP {
            let idx = (self.history_head + self.history_len) % CLIP_HISTORY_CAP;
            self.history[idx] = Some(item);
            self.history_len += 1;
        } else {
            // 淘汰最旧。
            self.history[self.history_head] = Some(item);
            self.history_head = (self.history_head + 1) % CLIP_HISTORY_CAP;
        }
    }

    /// 待清条数（当前 + 历史）——确认框列条数的来源。
    pub fn pending_count(&self) -> usize {
        self.history_len + if self.current.is_some() { 1 } else { 0 }
    }

    /// 清空：双入口同一执行函数（磁贴/快捷键都落到这里）。
    /// 当前 + 历史全清；同时解除敏感提示（清空后提示不再触发）。
    pub fn wipe_all(&mut self, _entry: WipeEntryKind) -> WipeReport {
        let had_current = self.current.is_some();
        self.current = None;
        let mut i = 0;
        while i < CLIP_HISTORY_CAP {
            self.history[i] = None;
            i += 1;
        }
        let history = self.history_len;
        self.history_len = 0;
        self.history_head = 0;
        // 清空即解除敏感提示（已经清了就不用再问）。
        self.prompt_armed = false;
        self.prompt_expired = false;
        self.wipe_count += 1;
        WipeReport {
            current: had_current,
            history,
            total: history + if had_current { 1 } else { 0 },
        }
    }

    pub fn wipe_count(&self) -> u32 {
        self.wipe_count
    }

    /// 粘贴读取：清空后返回 Empty——应用得到诚实失败，而非假数据。
    pub fn read(&self) -> ClipRead {
        match &self.current {
            Some(item) => ClipRead::Data { len: item.len },
            None => ClipRead::Empty,
        }
    }

    /// 敏感自觉：密码复制事件 → 武装提示窗（截止 = now + 5s）。
    pub fn on_password_copied(&mut self, now_ms: u64) {
        self.prompt_armed = true;
        self.prompt_expired = false;
        self.prompt_deadline_ms = now_ms + SENSITIVE_PROMPT_MS;
    }

    /// 提示窗是否仍在展示期（到时未清则过期）。
    pub fn prompt_active(&self, now_ms: u64) -> bool {
        self.prompt_armed && !self.prompt_expired && now_ms < self.prompt_deadline_ms
    }

    /// 到时未清：提示过期（一次性返回文案），此后不再烦人——直到下一次
    /// 密码复制事件重新武装。
    pub fn sensitive_prompt_expired_once(&mut self, now_ms: u64) -> Option<&'static str> {
        if self.prompt_armed && !self.prompt_expired && now_ms >= self.prompt_deadline_ms {
            self.prompt_expired = true;
            self.prompt_armed = false;
            self.prompt_shown += 1;
            Some(SENSITIVE_PROMPT_TEXT)
        } else {
            None
        }
    }

    pub fn prompt_expired(&self) -> bool {
        self.prompt_expired
    }

    pub fn prompt_shown(&self) -> u32 {
        self.prompt_shown
    }
}

// ---------------------------------------------------------------------------
// F511 域自检
// ---------------------------------------------------------------------------

/// F511 域自检（判据：双入口；当前+历史全清；确认框条数；密码后提示
/// 触发与 5s 时序；清空后粘贴行为）。
pub fn run_f511_checks() -> CheckSet {
    let mut cs = CheckSet::new("F511-clip-wipe");
    // 1) 双入口同一执行函数：磁贴与快捷键对同一初始态产出同一报告。
    let mut a = Clipboard::new();
    let mut b = Clipboard::new();
    let mut i = 0u8;
    while i < 4 {
        let payload = [i; 8];
        let _ = a.copy(&payload);
        let _ = b.copy(&payload);
        i += 1;
    }
    let ra = a.wipe_all(WipeEntryKind::QuickSettingsTile);
    let rb = b.wipe_all(WipeEntryKind::Hotkey);
    cs.add("dual_entry_same_exec", ra == rb && ra.total == 4, "");
    // 2) 当前 + 历史全清：清后当前空、历史 0、待清 0。
    let mut c = Clipboard::new();
    let mut j = 0u8;
    while j < 6 {
        let payload = [j; 4];
        let _ = c.copy(&payload);
        j += 1;
    }
    cs.add("pre_wipe_pending", c.pending_count() == 6, "");
    let rep = c.wipe_all(WipeEntryKind::Hotkey);
    cs.add(
        "current_and_history_cleared",
        rep.total == 6 && rep.history == 5 && c.pending_count() == 0 && c.read() == ClipRead::Empty,
        "",
    );
    // 3) 确认框条数 = 报告总条数（当前 + 历史）。
    cs.add("confirm_counts_match", rep.total == 6 && rep.current, "");
    // 4) 空板清空诚实为零条（不虚报）。
    let mut e = Clipboard::new();
    let rep0 = e.wipe_all(WipeEntryKind::QuickSettingsTile);
    cs.add("wipe_empty_honest_zero", rep0.total == 0 && !rep0.current && rep0.history == 0, "");
    // 5) 清空后粘贴：Empty 是诚实失败，不是假数据。
    cs.add("paste_after_wipe_honest_fail", e.read() == ClipRead::Empty, "");
    // 6) 历史环淘汰最旧：10 次复制 → 历史恒 8，当前为最后一条。
    let mut r = Clipboard::new();
    let mut k = 0u8;
    while k < 10 {
        let payload = [k; 2];
        let _ = r.copy(&payload);
        k += 1;
    }
    cs.add(
        "history_ring_evicts_oldest",
        r.pending_count() == CLIP_HISTORY_CAP + 1,
        "",
    );
    // 7) 敏感提示触发：密码复制事件武装 5s 窗。
    let mut p = Clipboard::new();
    p.on_password_copied(1_000);
    cs.add(
        "prompt_armed_5s_window",
        p.prompt_active(1_500) && p.prompt_active(5_999),
        "",
    );
    // 8) 时长常量精确 5s。
    cs.add("sensitive_prompt_5s_const", SENSITIVE_PROMPT_MS == 5_000, "");
    // 9) 到时未清：过期一次性提示，此后不再烦人。
    let expired_once = p.sensitive_prompt_expired_once(6_001).is_some();
    let no_renag = p.sensitive_prompt_expired_once(6_002).is_none() && !p.prompt_active(6_002);
    cs.add("prompt_expiry_once_no_nag", expired_once && no_renag && p.prompt_shown() == 1, "");
    // 10) 清空解除提示：清后任何时刻都不再触发。
    let mut q = Clipboard::new();
    q.on_password_copied(0);
    let _ = q.wipe_all(WipeEntryKind::Hotkey);
    cs.add(
        "wipe_disarms_prompt",
        !q.prompt_active(4_999) && q.sensitive_prompt_expired_once(6_000).is_none(),
        "",
    );
    // 11) 清空前粘贴有数据（对照 5）。
    let mut d = Clipboard::new();
    let _ = d.copy(&[1, 2, 3]);
    cs.add("read_before_wipe_has_data", d.read() == ClipRead::Data { len: 3 }, "");
    // 12) 快捷键常量逐字。
    cs.add("hotkey_const_verbatim", HOTKEY_CTRL_SHIFT_DELETE == "Ctrl+Shift+Delete", "");
    cs
}

#[cfg(test)]
mod f511_tests {
    use super::*;

    #[test]
    fn dual_entry_equivalence() {
        // 磁贴与快捷键走同一执行函数：同初始态 → 同结果。
        let mut a = Clipboard::new();
        let mut b = Clipboard::new();
        for payload in [&[1u8, 2][..], &[3u8, 4, 5][..], &[6u8][..]] {
            assert!(a.copy(payload));
            assert!(b.copy(payload));
        }
        let ra = a.wipe_all(WipeEntryKind::QuickSettingsTile);
        let rb = b.wipe_all(WipeEntryKind::Hotkey);
        assert_eq!(ra, rb, "双入口必须等价");
        assert_eq!(ra.total, 3);
        assert_eq!(a.wipe_count(), b.wipe_count());
    }

    #[test]
    fn full_clear_current_and_history() {
        let mut c = Clipboard::new();
        for i in 0..9u8 {
            let payload = [i; 3];
            assert!(c.copy(&payload));
        }
        // 9 次复制：历史环 8 + 当前 1 = 9 条待清。
        assert_eq!(c.pending_count(), 9, "当前 + 历史 = 待清条数");
        let rep = c.wipe_all(WipeEntryKind::Hotkey);
        assert_eq!(rep.total, 9, "确认框列 9 条");
        assert_eq!(rep.history, CLIP_HISTORY_CAP, "历史环全清");
        assert!(rep.current);
        assert_eq!(c.pending_count(), 0, "清后零残留");
    }

    #[test]
    fn honest_empty_read_after_wipe() {
        // 清空后粘贴行为：应用得到诚实失败（空），不是假数据。
        let mut c = Clipboard::new();
        assert!(c.copy(&[7u8; 10]));
        assert_eq!(c.read(), ClipRead::Data { len: 10 });
        let _ = c.wipe_all(WipeEntryKind::QuickSettingsTile);
        assert_eq!(c.read(), ClipRead::Empty, "清空后读取必须为诚实空");
        // 空板清空也不虚报条数。
        let mut e = Clipboard::new();
        let rep = e.wipe_all(WipeEntryKind::Hotkey);
        assert_eq!((rep.total, rep.history, rep.current), (0, 0, false));
    }

    #[test]
    fn five_second_window_boundary() {
        // 5s 时序边界：deadline 前活跃，deadline 起过期（判据精确 5 秒）。
        assert_eq!(SENSITIVE_PROMPT_MS, 5_000, "提示窗必须精确 5 秒");
        let mut p = Clipboard::new();
        p.on_password_copied(10_000);
        assert!(p.prompt_active(10_000), "武装即刻活跃");
        assert!(p.prompt_active(14_999), "deadline-1ms 仍活跃");
        assert!(!p.prompt_active(15_000), "deadline 起不再活跃");
        let msg = p.sensitive_prompt_expired_once(15_000);
        assert_eq!(msg, Some(SENSITIVE_PROMPT_TEXT), "到时未清过期提示一次");
        assert!(p.prompt_expired());
        assert!(!p.prompt_active(15_001), "过期后不得再活跃");
    }

    #[test]
    fn expiry_does_not_nag() {
        // 过期不烦人：一次性，之后直到下次密码复制事件都沉默。
        let mut p = Clipboard::new();
        p.on_password_copied(0);
        assert_eq!(p.sensitive_prompt_expired_once(5_001), Some(SENSITIVE_PROMPT_TEXT));
        for t in [5_002, 6_000, 60_000] {
            assert_eq!(p.sensitive_prompt_expired_once(t), None, "过期后不得重复提示");
        }
        assert_eq!(p.prompt_shown(), 1, "提示只展示一轮");
        // 下一次密码复制事件重新武装（全新窗口，过期态复位）。
        p.on_password_copied(100_000);
        assert!(p.prompt_active(100_001), "新事件重新武装");
        assert!(!p.prompt_expired(), "重新武装即全新窗口");
    }

    #[test]
    fn wipe_disarms_prompt_immediately() {
        // 清空即解除敏感提示：窗口内清掉就不该再问。
        let mut p = Clipboard::new();
        p.on_password_copied(0);
        assert!(p.prompt_active(1_000));
        let _ = p.wipe_all(WipeEntryKind::QuickSettingsTile);
        assert!(!p.prompt_active(2_000), "清空后提示窗口失效");
        assert_eq!(p.sensitive_prompt_expired_once(6_000), None, "清空后不得过期打扰");
        assert!(!p.prompt_expired());
    }

    #[test]
    fn history_ring_capacity_and_oversize_refusal() {
        // 历史环定长 8：第 9 条旧项被淘汰；超载荷上限诚实拒绝。
        let mut c = Clipboard::new();
        for i in 0..12u8 {
            let payload = [i; 2];
            assert!(c.copy(&payload));
        }
        assert_eq!(c.pending_count(), CLIP_HISTORY_CAP + 1, "历史 8 + 当前 1");
        // 超载荷：诚实拒绝，不留残迹。
        let big = [0u8; CLIP_DATA_CAP + 1];
        assert!(!c.copy(&big), "超上限必须拒绝");
        assert_eq!(c.pending_count(), CLIP_HISTORY_CAP + 1, "拒绝后条数不变");
    }
}

// ===========================================================================
// F512 截图历史
// ===========================================================================

/// 截图历史上限：判据精确口径 20 条。
pub const SHOT_CAP: usize = 20;

/// 截图两态：临时区驻留（未保存）/ 已保存（持久引用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotState {
    /// 用户主动保存前驻临时区（关机即失——F311 同源纪律）。
    Transient,
    /// 已保存（历史持久仅含此态的引用）。
    Saved { path: &'static str },
}

/// 历史条目。
#[derive(Clone, Copy, Debug)]
pub struct ShotEntry {
    pub id: u32,
    pub state: ShotState,
    /// 截图尺寸（字节，建模缩略元数据）。
    pub size: u32,
}

/// 重编辑链路句柄（点开即重看/重编辑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EditHandle {
    pub shot_id: u32,
    /// 来源是否为已保存引用（临时件同样可重编辑）。
    pub from_saved: bool,
}

/// 截图历史错误态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotError {
    NoSuchShot,
    NotTransient,
}

/// 截图历史：20 条定长有序环（新条目在尾，满则淘汰最旧）。
/// 关机清理只清 Transient（F311 同源纪律），Saved 引用保留。
pub struct ShotHistory {
    entries: [Option<ShotEntry>; SHOT_CAP],
    count: usize,
    next_id: u32,
    /// 关机未保存丢失的「通知一次」标志（F311 同源：只通知第一次）。
    shutdown_notify_shown: u32,
    evicted_count: u32,
}

impl ShotHistory {
    pub const fn new() -> Self {
        ShotHistory {
            entries: [None; SHOT_CAP],
            count: 0,
            next_id: 1,
            shutdown_notify_shown: 0,
            evicted_count: 0,
        }
    }

    fn remove_at(&mut self, idx: usize) {
        let mut i = idx;
        while i + 1 < self.count {
            self.entries[i] = self.entries[i + 1];
            i += 1;
        }
        self.entries[self.count - 1] = None;
        self.count -= 1;
    }

    /// 新截图入架（临时态）：满 20 即淘汰最旧（第 21 张淘汰第 1 张）。
    /// 返回新条目 id。
    pub fn push(&mut self, size: u32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        if self.count == SHOT_CAP {
            let evicted = self.entries[0].map(|e| e.id);
            self.remove_at(0);
            self.evicted_count += 1;
            let _ = evicted;
        }
        self.entries[self.count] = Some(ShotEntry {
            id,
            state: ShotState::Transient,
            size,
        });
        self.count += 1;
        id
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn evicted_count(&self) -> u32 {
        self.evicted_count
    }

    pub fn get(&self, id: u32) -> Option<ShotEntry> {
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 最旧条目 id（淘汰对账用）。
    pub fn oldest_id(&self) -> Option<u32> {
        self.entries[0].map(|e| e.id)
    }

    /// 另存：Transient → Saved{path}（历史持久引用随即更新）。
    /// 已保存条目不可重另存（引用单一来源）。
    pub fn save_as(&mut self, id: u32, path: &'static str) -> Result<(), ShotError> {
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.id == id {
                    if let ShotState::Transient = e.state {
                        self.entries[i] = Some(ShotEntry {
                            state: ShotState::Saved { path },
                            ..e
                        });
                        return Ok(());
                    }
                    return Err(ShotError::NotTransient);
                }
            }
            i += 1;
        }
        Err(ShotError::NoSuchShot)
    }

    /// 重编辑链路：历史条目 → 编辑器句柄（临时/已保存皆可重编辑）。
    pub fn open_editor(&self, id: u32) -> Result<EditHandle, ShotError> {
        match self.get(id) {
            Some(e) => Ok(EditHandle {
                shot_id: id,
                from_saved: matches!(e.state, ShotState::Saved { .. }),
            }),
            None => Err(ShotError::NoSuchShot),
        }
    }

    /// 删除条目（重看条目架上的删除动作）。
    pub fn remove(&mut self, id: u32) -> Result<(), ShotError> {
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.id == id {
                    self.remove_at(i);
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(ShotError::NoSuchShot)
    }

    /// 已保存引用数（历史持久仅含已保存项的引用）。
    pub fn saved_refs(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if matches!(e.state, ShotState::Saved { .. }) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 临时条目数。
    pub fn transient_count(&self) -> usize {
        self.count - self.saved_refs()
    }

    /// 关机清理：只清 Transient（F311 同源纪律——未保存的关机即失），
    /// Saved 引用保留。返回清掉的临时条数；有丢失且是第一次 → 通知一次
    /// （此后不再重复打扰）。
    pub fn shutdown_cleanup(&mut self) -> usize {
        let mut cleared = 0;
        let mut i = 0;
        while i < self.count {
            let transient = matches!(
                self.entries[i],
                Some(ShotEntry {
                    state: ShotState::Transient,
                    ..
                })
            );
            if transient {
                self.remove_at(i);
                cleared += 1;
                // 删除后不前进 i（后项已前移补位）。
            } else {
                i += 1;
            }
        }
        if cleared > 0 && self.shutdown_notify_shown == 0 {
            // 通知一次（F311 同源纪律）。
            self.shutdown_notify_shown += 1;
        }
        cleared
    }

    /// 关机未保存丢失通知是否已展示过（恰好一次）。
    pub fn shutdown_notified(&self) -> bool {
        self.shutdown_notify_shown > 0
    }

    pub fn shutdown_notify_count(&self) -> u32 {
        self.shutdown_notify_shown
    }
}

// ---------------------------------------------------------------------------
// F512 域自检
// ---------------------------------------------------------------------------

/// F512 域自检（判据：20 条上限与淘汰；临时/已保存两态；关机清理与
/// 通知；重编辑链路；另存路径）。
pub fn run_f512_checks() -> CheckSet {
    let mut cs = CheckSet::new("F512-shot-history");
    // 1) 上限常量：判据精确 20 条。
    cs.add("cap_const_20", SHOT_CAP == 20, "");
    // 2) 填满 20 张全为临时态。
    let mut h = ShotHistory::new();
    let mut first_id = 0u32;
    let mut i = 0;
    while i < 20 {
        let id = h.push(1024);
        if i == 0 {
            first_id = id;
        }
        i += 1;
    }
    cs.add(
        "fill_20_transient",
        h.len() == 20 && h.transient_count() == 20 && h.get(first_id).is_some(),
        "",
    );
    // 3) 第 21 张淘汰最旧（第 1 张消失，新张在架，长度仍 20）。
    let oldest = h.oldest_id().unwrap();
    let id21 = h.push(2048);
    cs.add(
        "twenty_first_evicts_oldest",
        h.len() == 20
            && h.get(oldest).is_none()
            && h.get(id21).is_some()
            && h.evicted_count() == 1
            && h.oldest_id() != Some(oldest),
        "",
    );
    // 4) 另存路径：Transient → Saved{path}，引用更新、临时数减少。
    let mut h2 = ShotHistory::new();
    let a = h2.push(100);
    let b = h2.push(200);
    let saved_ok = h2.save_as(b, "C:/Users/pic/shots/b.png").is_ok();
    let eb = h2.get(b).unwrap();
    cs.add(
        "save_as_transfers_state",
        saved_ok
            && eb.state == ShotState::Saved { path: "C:/Users/pic/shots/b.png" }
            && h2.saved_refs() == 1
            && h2.transient_count() == 1
            && h2.get(a).unwrap().state == ShotState::Transient,
        "",
    );
    // 5) 已保存条目不可重另存（引用单一来源）。
    cs.add(
        "resave_saved_rejected",
        h2.save_as(b, "C:/elsewhere.png") == Err(ShotError::NotTransient),
        "",
    );
    // 6) 关机清理：只清临时，已保存引用保留。
    let mut h3 = ShotHistory::new();
    let mut k = 0;
    while k < 5 {
        let id = h3.push(64);
        if k < 2 {
            let _ = h3.save_as(id, "C:/s.png");
        }
        k += 1;
    }
    let cleared = h3.shutdown_cleanup();
    cs.add(
        "shutdown_keeps_saved",
        cleared == 3
            && h3.saved_refs() == 2
            && h3.transient_count() == 0
            && h3.len() == 2,
        "",
    );
    // 7) 通知一次：两次有丢失的关机只通知一次（F311 同源）。
    cs.add(
        "shutdown_notify_once",
        h3.shutdown_notified() && h3.shutdown_notify_count() == 1,
        "",
    );
    let cleared2 = h3.shutdown_cleanup();
    cs.add(
        "second_shutdown_silent",
        cleared2 == 0 && h3.shutdown_notify_count() == 1,
        "",
    );
    // 8) 无临时丢失的关机不触发通知。
    let mut h4 = ShotHistory::new();
    let sid = h4.push(10);
    let _ = h4.save_as(sid, "C:/all-saved.png");
    let cleared3 = h4.shutdown_cleanup();
    cs.add(
        "no_transient_no_notify",
        cleared3 == 0 && !h4.shutdown_notified() && h4.len() == 1,
        "",
    );
    // 9) 重编辑链路：已保存与临时条目都能取到编辑器句柄，来源标注正确。
    let eh_saved = h3.open_editor(h3.oldest_id().unwrap());
    let handle_ok = match eh_saved {
        Ok(hd) => hd.from_saved,
        Err(_) => false,
    };
    cs.add("reedit_saved_handle", handle_ok, "");
    let mut h5 = ShotHistory::new();
    let tid = h5.push(1);
    cs.add(
        "reedit_transient_handle",
        h5.open_editor(tid) == Ok(EditHandle { shot_id: tid, from_saved: false }),
        "",
    );
    // 10) 删除条目与查无此图诚实报错。
    let mut h6 = ShotHistory::new();
    let rid = h6.push(5);
    cs.add(
        "remove_and_no_such_shot",
        h6.remove(rid).is_ok()
            && h6.len() == 0
            && h6.remove(rid) == Err(ShotError::NoSuchShot)
            && h6.open_editor(rid) == Err(ShotError::NoSuchShot),
        "",
    );
    cs
}

#[cfg(test)]
mod f512_tests {
    use super::*;

    #[test]
    fn cap_is_twenty_and_fills() {
        assert_eq!(SHOT_CAP, 20, "判据精确 20 条上限");
        let mut h = ShotHistory::new();
        for _ in 0..20 {
            h.push(1);
        }
        assert_eq!(h.len(), 20, "恰好填满");
        assert!(h.is_empty() == false);
        assert_eq!(h.transient_count(), 20, "新截图全为临时态");
    }

    #[test]
    fn twenty_first_evicts_oldest() {
        // 必测边界：第 21 张淘汰最旧（第 1 张），长度守恒 20。
        let mut h = ShotHistory::new();
        let first = h.push(1);
        let mut second = 0u32;
        for _ in 1..20 {
            second = h.push(1);
        }
        assert_eq!(h.len(), 20);
        let id21 = h.push(1);
        assert!(id21 != first && id21 != second);
        assert!(h.get(first).is_none(), "最旧必须被淘汰");
        assert!(h.get(id21).is_some(), "新条目必须在架");
        assert_eq!(h.len(), 20, "长度守恒 20");
        assert_eq!(h.evicted_count(), 1);
        // 再挤入一张：淘汰此时最旧（原第 2 张 id2）。
        let oldest_now = h.oldest_id().unwrap();
        assert_ne!(oldest_now, first, "first 已被淘汰");
        let id22 = h.push(1);
        assert!(h.get(oldest_now).is_none(), "顺序淘汰必须继续");
        assert!(h.get(id22).is_some());
        assert!(h.get(second).is_some(), "最新一批仍在架");
        assert_eq!(h.evicted_count(), 2);
    }

    #[test]
    fn save_as_updates_persistent_ref() {
        // 另存 = Transient → Saved 转移，历史持久引用更新。
        let mut h = ShotHistory::new();
        let _a = h.push(10);
        let b = h.push(20);
        let c = h.push(30);
        h.save_as(b, "C:/shots/b.png").unwrap();
        assert_eq!(h.saved_refs(), 1, "持久引用恰一条");
        assert_eq!(h.transient_count(), 2, "其余仍临时");
        match h.get(b).unwrap().state {
            ShotState::Saved { path } => assert_eq!(path, "C:/shots/b.png", "引用路径随另存更新"),
            other => panic!("必须为 Saved 态，实际 {:?}", other),
        }
        // 已保存不可重另存；不存在条目报 NoSuchShot。
        assert_eq!(h.save_as(b, "C:/x.png"), Err(ShotError::NotTransient));
        assert_eq!(h.save_as(999, "C:/x.png"), Err(ShotError::NoSuchShot));
        let _ = c;
    }

    #[test]
    fn shutdown_clears_transient_keeps_saved() {
        // F311 同源纪律：关机只清临时，Saved 引用保留。
        let mut h = ShotHistory::new();
        let mut ids = [0u32; 6];
        for slot in ids.iter_mut() {
            *slot = h.push(1);
        }
        let _ = h.save_as(ids[0], "C:/a.png");
        let _ = h.save_as(ids[4], "C:/e.png");
        let cleared = h.shutdown_cleanup();
        assert_eq!(cleared, 4, "清掉 4 张临时");
        assert_eq!(h.len(), 2, "只剩 2 条 Saved 引用");
        assert_eq!(h.saved_refs(), 2);
        assert!(h.get(ids[0]).is_some(), "Saved 引用必须保留");
        assert!(h.get(ids[1]).is_none(), "Transient 必须清掉");
        assert!(h.get(ids[4]).is_some());
        assert_eq!(h.transient_count(), 0);
    }

    #[test]
    fn shutdown_notify_exactly_once() {
        // 通知一次：第一次有丢失的关机通知；之后不再重复打扰。
        let mut h = ShotHistory::new();
        for _ in 0..3 {
            h.push(1);
        }
        assert!(!h.shutdown_notified(), "关机前未通知");
        assert_eq!(h.shutdown_cleanup(), 3);
        assert!(h.shutdown_notified(), "有丢失必须通知");
        assert_eq!(h.shutdown_notify_count(), 1, "恰好一次");
        // 第二次关机又有临时丢失：同样不再重复通知。
        h.push(2);
        assert_eq!(h.shutdown_cleanup(), 1);
        assert_eq!(h.shutdown_notify_count(), 1, "第二次关机不再通知");
        // 全程无丢失的关机从不通知。
        let mut h2 = ShotHistory::new();
        let s = h2.push(1);
        let _ = h2.save_as(s, "C:/saved.png");
        assert_eq!(h2.shutdown_cleanup(), 0);
        assert!(!h2.shutdown_notified(), "无丢失不得通知");
    }

    #[test]
    fn reedit_chain_and_remove() {
        // 重编辑链路：临时/已保存皆可开编辑器句柄；删除后查无此图。
        let mut h = ShotHistory::new();
        let t = h.push(1);
        let s = h.push(2);
        let _ = h.save_as(s, "C:/s.png");
        assert_eq!(
            h.open_editor(t),
            Ok(EditHandle { shot_id: t, from_saved: false }),
            "临时件可重编辑"
        );
        assert_eq!(
            h.open_editor(s),
            Ok(EditHandle { shot_id: s, from_saved: true }),
            "已保存件可重编辑且来源标注正确"
        );
        assert_eq!(h.open_editor(424242), Err(ShotError::NoSuchShot), "查无此图诚实报错");
        h.remove(t).unwrap();
        assert!(h.get(t).is_none());
        assert_eq!(h.len(), 1);
        assert_eq!(h.remove(t), Err(ShotError::NoSuchShot), "重复删除诚实报错");
    }
}
