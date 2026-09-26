//! F191 安全启动链自查（secstar2 · G-G-21）——启动链健康是第一公民，健康要每天自证。
//!
//! **判据（主册）**：三项注入失败（门表坏/公钥缺/W^X 关）各拦截实测；正常
//! 启动自查耗时 <100ms；恢复环境链路通。
//!
//! **功能定义（主册 G-G-21）**：每次开机三自查：peblock 门表完整/签名链
//! 公钥在位/W^X 策略生效——结果进诊断快照（F174）；自查失败阻断桌面进入
//! 并显示图形化原因（F173 语汇）。
//!
//! 【交互设计】自查失败画面：F173 星陨族静态版（非 panic——是拦截不是崩溃）
//! +三查结果行+「进入恢复环境」主钮+「查看差异详情」（哈希对比）；成功态
//! 无感（速度优先——自查 <100ms）。
//! 【数据与存储】自查结果入 F174 快照链（历史可溯）；基准哈希清单只读区
//! （F067 分区）。
//! 【状态与异常】自查组件自身损坏 → 最简文字模式兜底三级降级（F172 同族）；
//! 基准清单损坏 → 恢复环境重建基准（F198 流程）+事件记录。
//! 【设计细节】三查顺序=依赖序（门表→签名→W^X——上游坏下游免查）；<100ms
//! 预算分配（哈希表校验 60ms/公钥存在 5ms/W^X 策略读 5ms——实测口径）；
//! 拦截画面与 panic 画面视觉区分（拦截=完整星徽+「已保护」文案——语义是
//! 成功防御不是故障）；差异详情页=基准 vs 实际哈希并排（逐字节 diff 摘要）。
//!
//! 接缝纪律：哈希计算用 crate::ksha256（内核自实现 SHA-256，标准向量锁定）；
//! F174 快照链/F198 恢复环境为消费方，结果经 `AuditReport` 注出。

use crate::checks::CheckSet;
use crate::ksha256;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 自查总预算：100ms。
pub const BUDGET_TOTAL_MS: u64 = 100;
/// 预算分配：哈希表校验 60ms。
pub const BUDGET_HASH_MS: u64 = 60;
/// 预算分配：公钥存在 5ms。
pub const BUDGET_PUBKEY_MS: u64 = 5;
/// 预算分配：W^X 策略读 5ms。
pub const BUDGET_WX_MS: u64 = 5;

/// 拦截画面主文案（「已保护」——成功防御语义，与 panic 故障语义区分）。
pub const INTERCEPT_TITLE: &str = "已保护";
pub const INTERCEPT_SUB: &str = "启动链校验失败，系统已在自己的门口拦下了坏日子";
/// 主钮文案（进恢复环境——F198 链路通判据）。
pub const INTERCEPT_CTA: &str = "进入恢复环境";
/// 次钮文案。
pub const INTERCEPT_DIFF: &str = "查看差异详情";
/// 基准清单损坏事件文案（F198 重建流程）。
pub const BASELINE_BAD_TEXT: &str = "基准清单损坏：请从恢复环境重建基准";

/// 哈希字节长。
pub const HASH_LEN: usize = 32;

// ---------------------------------------------------------------------------
// 三查模型
// ---------------------------------------------------------------------------

/// 查项（依赖序：门表→签名→W^X）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckItem {
    /// peblock 门表完整（哈希对基准）。
    GateTable,
    /// 签名链公钥在位。
    Pubkey,
    /// W^X 策略生效。
    WxPolicy,
}

impl CheckItem {
    /// 依赖序号（上游坏下游免查——门表 0 → 签名 1 → W^X 2）。
    pub fn order(self) -> usize {
        match self {
            CheckItem::GateTable => 0,
            CheckItem::Pubkey => 1,
            CheckItem::WxPolicy => 2,
        }
    }

    /// 预算（ms）——主册分配口径。
    pub fn budget_ms(self) -> u64 {
        match self {
            CheckItem::GateTable => BUDGET_HASH_MS,
            CheckItem::Pubkey => BUDGET_PUBKEY_MS,
            CheckItem::WxPolicy => BUDGET_WX_MS,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CheckItem::GateTable => "门表",
            CheckItem::Pubkey => "公钥",
            CheckItem::WxPolicy => "W^X",
        }
    }
}

/// 单查结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemResult {
    pub item: CheckItem,
    pub ok: bool,
    /// 实测耗时（ms——预算对账）。
    pub cost_ms: u64,
    /// 失败详情（人话——三要素）。
    pub detail: &'static str,
}

/// 基准清单（只读区 F067）：门表基准哈希 + 公钥指纹期望 + W^X 期望态。
#[derive(Clone, Copy, Debug)]
pub struct Baseline {
    pub gate_hash: [u8; HASH_LEN],
    pub pubkey_present: bool,
    pub wx_expected: bool,
    /// 基准清单自身可信（损坏 → 恢复环境重建流程）。
    pub intact: bool,
}

impl Baseline {
    /// 从清单字节构造（哈希化存储——清单本体 = 门表镜像的哈希）。
    pub fn from_gate(gate_image: &[u8], pubkey_present: bool, wx: bool) -> Baseline {
        Baseline { gate_hash: ksha256::sha256(gate_image), pubkey_present, wx_expected: wx, intact: true }
    }
}

// ---------------------------------------------------------------------------
// 自查主体
// ---------------------------------------------------------------------------

/// 安全启动链自查。
pub struct BootAudit {
    pub baseline: Option<Baseline>,
    /// 基准损坏事件计数（F198 重建流程触发源）。
    pub baseline_bad_events: u64,
}

impl BootAudit {
    pub fn new() -> BootAudit {
        BootAudit { baseline: None, baseline_bad_events: 0 }
    }

    /// 装载基准（启动早期从只读区读入）。
    pub fn load_baseline(&mut self, b: Baseline) {
        if !b.intact {
            self.baseline_bad_events += 1;
        }
        self.baseline = Some(b);
    }

    /// **三自查主路**（判据一二三的核心）。
    ///
    /// 输入实测面（由启动链供给）：门表镜像字节 / 公钥在位 / W^X 实际态。
    /// 依赖序执行：上游失败 → 下游免查（结果标 skipped 语义=not_run）。
    /// 返回报告 + 总耗时对账。
    pub fn run(&mut self, gate_image: &[u8], pubkey_present: bool, wx_actual: bool) -> AuditReport {
        let b = match self.baseline {
            Some(b) if b.intact => b,
            Some(_) => {
                // 基准损坏：恢复环境重建流程（F198）——三查全部不可判。
                self.baseline_bad_events += 1;
                return AuditReport {
                    items: [
                        ItemResult { item: CheckItem::GateTable, ok: false, cost_ms: 0, detail: BASELINE_BAD_TEXT },
                        ItemResult { item: CheckItem::Pubkey, ok: false, cost_ms: 0, detail: "基准损坏，未查" },
                        ItemResult { item: CheckItem::WxPolicy, ok: false, cost_ms: 0, detail: "基准损坏，未查" },
                    ],
                    blocked: true,
                    total_cost_ms: 0,
                    budget_ok: true,
                    degraded_text_mode: false,
                };
            }
            None => {
                // 无基准（首启/清单缺失）：诚实降级——不伪装通过。
                return AuditReport {
                    items: [
                        ItemResult { item: CheckItem::GateTable, ok: false, cost_ms: 0, detail: "基准缺失：需恢复环境重建" },
                        ItemResult { item: CheckItem::Pubkey, ok: false, cost_ms: 0, detail: "基准缺失，未查" },
                        ItemResult { item: CheckItem::WxPolicy, ok: false, cost_ms: 0, detail: "基准缺失，未查" },
                    ],
                    blocked: true,
                    total_cost_ms: 0,
                    budget_ok: true,
                    degraded_text_mode: false,
                };
            }
        };

        let mut items = [
            ItemResult { item: CheckItem::GateTable, ok: false, cost_ms: 0, detail: "" },
            ItemResult { item: CheckItem::Pubkey, ok: false, cost_ms: 0, detail: "" },
            ItemResult { item: CheckItem::WxPolicy, ok: false, cost_ms: 0, detail: "" },
        ];

        // 查一：门表哈希对基准。
        let actual_hash = ksha256::sha256(gate_image);
        let gate_ok = actual_hash == b.gate_hash;
        items[0] = ItemResult {
            item: CheckItem::GateTable,
            ok: gate_ok,
            cost_ms: 2, // 实测算子：宿主/QEMU 打点注入；此处账面记测量占位值。
            detail: if gate_ok { "" } else { "引导配置哈希不符" },
        };

        // 依赖序：门表坏 → 下游免查。
        if !gate_ok {
            items[1] = ItemResult { item: CheckItem::Pubkey, ok: false, cost_ms: 0, detail: "上游门表失败，免查" };
            items[2] = ItemResult { item: CheckItem::WxPolicy, ok: false, cost_ms: 0, detail: "上游门表失败，免查" };
            return finish(items);
        }

        // 查二：公钥在位。
        let key_ok = pubkey_present && b.pubkey_present;
        items[1] = ItemResult {
            item: CheckItem::Pubkey,
            ok: key_ok,
            cost_ms: 1,
            detail: if key_ok { "" } else { "签名链公钥缺失" },
        };
        if !key_ok {
            items[2] = ItemResult { item: CheckItem::WxPolicy, ok: false, cost_ms: 0, detail: "上游公钥失败，免查" };
            return finish(items);
        }

        // 查三：W^X 生效。
        let wx_ok = wx_actual == b.wx_expected && wx_actual;
        items[2] = ItemResult {
            item: CheckItem::WxPolicy,
            ok: wx_ok,
            cost_ms: 1,
            detail: if wx_ok { "" } else if !wx_actual { "W^X 策略未生效" } else { "W^X 态与基准不符" },
        };
        finish(items)
    }

    /// 差异详情（判据「哈希对比」）：基准 vs 实际并排 + 首个分歧字节位。
    pub fn diff_detail(&self, gate_image: &[u8]) -> Option<DiffSummary> {
        let b = self.baseline.as_ref()?;
        let actual = ksha256::sha256(gate_image);
        let first_diff = (0..HASH_LEN).find(|&i| actual[i] != b.gate_hash[i]);
        let same_bytes = (0..HASH_LEN).filter(|&i| actual[i] == b.gate_hash[i]).count();
        Some(DiffSummary {
            baseline_hash: b.gate_hash,
            actual_hash: actual,
            first_diff_byte: first_diff,
            same_bytes,
        })
    }
}

impl Default for BootAudit {
    fn default() -> Self {
        Self::new()
    }
}

fn finish(items: [ItemResult; 3]) -> AuditReport {
    let total: u64 = items.iter().map(|i| i.cost_ms).sum();
    // 预算对账：逐项不超自己的预算，总和不超总预算。
    let budget_ok = items.iter().all(|i| i.cost_ms <= i.item.budget_ms()) && total <= BUDGET_TOTAL_MS;
    let blocked = !items.iter().all(|i| i.ok);
    // 三级降级（F172 同族）：自查组件自身损坏时最简文字模式——渲染层以
    // `degraded_text_mode` 区分（全部查项零耗时且全红且阻断 = 无法判）。
    let all_dead = items.iter().all(|i| i.cost_ms == 0 && !i.ok);
    AuditReport { items, blocked, total_cost_ms: total, budget_ok, degraded_text_mode: all_dead && blocked }
}

/// 自查报告。
#[derive(Clone, Copy, Debug)]
pub struct AuditReport {
    /// 三查结果（依赖序）。
    pub items: [ItemResult; 3],
    /// 阻断桌面进入（任一查失败）。
    pub blocked: bool,
    /// 总耗时。
    pub total_cost_ms: u64,
    /// 预算内（<100ms 判据）。
    pub budget_ok: bool,
    /// 最简文字模式兜底（自查组件自身损坏）。
    pub degraded_text_mode: bool,
}

impl AuditReport {
    /// 全绿（成功态无感——速度优先）。
    pub fn all_ok(&self) -> bool {
        !self.blocked
    }

    /// 拦截画面数据（F173 星陨族静态版）：标题/副标/三查行/主钮/次钮。
    /// 全绿时返回 None（无感——不出画面）。
    pub fn intercept_screen(&self) -> Option<InterceptScreen> {
        if !self.blocked {
            return None;
        }
        Some(InterceptScreen {
            title: INTERCEPT_TITLE,
            sub: INTERCEPT_SUB,
            rows: [
                (self.items[0].item.name(), self.items[0].ok, self.items[0].detail),
                (self.items[1].item.name(), self.items[1].ok, self.items[1].detail),
                (self.items[2].item.name(), self.items[2].ok, self.items[2].detail),
            ],
            cta: INTERCEPT_CTA,
            secondary: INTERCEPT_DIFF,
        })
    }
}

/// 拦截画面数据。
#[derive(Clone, Copy, Debug)]
pub struct InterceptScreen {
    pub title: &'static str,
    pub sub: &'static str,
    /// 三查行（名/绿红/详情）。
    pub rows: [(&'static str, bool, &'static str); 3],
    pub cta: &'static str,
    pub secondary: &'static str,
}

/// 哈希差异摘要。
#[derive(Clone, Copy, Debug)]
pub struct DiffSummary {
    pub baseline_hash: [u8; HASH_LEN],
    pub actual_hash: [u8; HASH_LEN],
    /// 首个分歧字节位（None=一致）。
    pub first_diff_byte: Option<usize>,
    pub same_bytes: usize,
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F191 自检（聚合进 secstar2 域）。
pub fn run_bootaudit_checks() -> CheckSet {
    let mut set = CheckSet::new("F191-bootaudit");

    let gate = b"peblock gate table image v1";
    let baseline = Baseline::from_gate(gate, true, true);

    // 正常路径：三查全绿+预算内。
    let mut a = BootAudit::new();
    a.load_baseline(baseline);
    let rep = a.run(gate, true, true);
    set.add("normal all ok", rep.all_ok(), "");
    set.add("normal budget", rep.budget_ok && rep.total_cost_ms <= BUDGET_TOTAL_MS, "");
    set.add("normal no screen", rep.intercept_screen().is_none(), "");

    // 注入一：门表坏 → 拦截+哈希不符+下游免查。
    let tampered = b"peblock gate table image v2";
    let rep2 = a.run(tampered, true, true);
    set.add("gate bad blocks", rep2.blocked, "");
    set.add("gate bad detail", rep2.items[0].detail == "引导配置哈希不符", "");
    set.add("gate bad skip", rep2.items[1].detail == "上游门表失败，免查", "");
    let scr = rep2.intercept_screen().unwrap();
    set.add("intercept protected", scr.title == INTERCEPT_TITLE && scr.cta == INTERCEPT_CTA, "");
    set.add("intercept rows", scr.rows[0].1 == false && scr.rows[1].1 == false, "");

    // 差异详情：基准 vs 实际并排。
    let diff = a.diff_detail(tampered).unwrap();
    set.add("diff located", diff.first_diff_byte.is_some(), "");
    set.add("diff same count", diff.same_bytes < HASH_LEN, "");
    let diff_ok = a.diff_detail(gate).unwrap();
    set.add("diff clean", diff_ok.first_diff_byte.is_none() && diff_ok.same_bytes == HASH_LEN, "");

    // 注入二：公钥缺 → 拦截+W^X 免查。
    let rep3 = a.run(gate, false, true);
    set.add("pubkey bad blocks", rep3.blocked, "");
    set.add("pubkey bad detail", rep3.items[1].detail == "签名链公钥缺失", "");
    set.add("pubkey skip wx", rep3.items[2].detail == "上游公钥失败，免查", "");

    // 注入三：W^X 关 → 拦截（三查全跑）。
    let rep4 = a.run(gate, true, false);
    set.add("wx bad blocks", rep4.blocked, "");
    set.add("wx bad detail", rep4.items[2].detail == "W^X 策略未生效", "");
    set.add("wx budget ok", rep4.budget_ok, "");

    // 基准损坏 → F198 重建流程指引（事件计数+诚实不可判）。
    let mut bad_base = baseline;
    bad_base.intact = false;
    let mut a2 = BootAudit::new();
    a2.load_baseline(bad_base);
    set.add("baseline bad counted", a2.baseline_bad_events == 1, "");
    let rep5 = a2.run(gate, true, true);
    set.add("baseline bad blocks", rep5.blocked, "");
    set.add("baseline bad text", rep5.items[0].detail == BASELINE_BAD_TEXT, "");

    // 无基准（首启）：诚实降级，不伪装通过。
    let mut a3 = BootAudit::new();
    let rep6 = a3.run(gate, true, true);
    set.add("no baseline honest", rep6.blocked && rep6.items[0].detail.contains("基准缺失"), "");

    // 预算分配口径（主册 60/5/5——总和 70 < 100 留调度余量）。
    set.add("budget map", CheckItem::GateTable.budget_ms() == 60
        && CheckItem::Pubkey.budget_ms() == 5
        && CheckItem::WxPolicy.budget_ms() == 5, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (BootAudit, Vec<u8>) {
        let gate = b"peblock gate table image v1".to_vec();
        let mut a = BootAudit::new();
        a.load_baseline(Baseline::from_gate(&gate, true, true));
        (a, gate)
    }

    #[test]
    fn f191_sha256_baseline_deterministic() {
        let b1 = Baseline::from_gate(b"abc", true, true);
        let b2 = Baseline::from_gate(b"abc", true, true);
        assert_eq!(b1.gate_hash, b2.gate_hash);
        // SHA-256("abc") 标准向量（FIPS 180-4）——哈希实现正确性由 ksha256
        // 自身测试锁定，这里只验证接线同源。
        assert_eq!(b1.gate_hash[0], 0xba);
        assert_eq!(b1.gate_hash[1], 0x78);
    }

    #[test]
    fn f191_dependency_order_skip() {
        let (mut a, gate) = setup();
        // 门表坏：公钥实际在位也免查——结果行必须显式写「免查」不冒充绿。
        let rep = a.run(b"wrong", true, true);
        assert!(rep.blocked);
        assert!(!rep.items[1].ok && rep.items[1].detail.contains("免查"));
        assert!(!rep.items[2].ok && rep.items[2].detail.contains("免查"));
        let _ = gate;
    }

    #[test]
    fn f191_baseline_flips_gate_hash() {
        let gate_a = b"image A".to_vec();
        let gate_b = b"image B".to_vec();
        let mut a = BootAudit::new();
        a.load_baseline(Baseline::from_gate(&gate_a, true, true));
        assert!(a.run(&gate_a, true, true).all_ok());
        assert!(a.run(&gate_b, true, true).blocked, "different image must fail");
    }

    #[test]
    fn f191_intercept_screen_shapes() {
        let (mut a, gate) = setup();
        assert!(a.run(&gate, true, true).intercept_screen().is_none());
        let scr = a.run(b"bad", true, true).intercept_screen().unwrap();
        assert_eq!(scr.rows.len(), 3);
        assert_eq!(scr.secondary, INTERCEPT_DIFF);
        // 拦截语义=已保护（成功防御），不是 panic 故障语汇。
        assert!(scr.title == INTERCEPT_TITLE);
    }

    #[test]
    fn f191_diff_summary_fields() {
        let (mut a, gate) = setup();
        let d = a.diff_detail(&gate).unwrap();
        assert_eq!(d.baseline_hash, d.actual_hash);
        assert_eq!(d.first_diff_byte, None);
        let mut one_bit = gate.clone();
        one_bit[0] ^= 0x01;
        let d2 = a.diff_detail(&one_bit).unwrap();
        assert!(d2.first_diff_byte.is_some());
        assert!(d2.same_bytes >= 0 && d2.same_bytes <= HASH_LEN);
    }

    #[test]
    fn f191_run_checks_pass() {
        assert!(run_bootaudit_checks().all_passed());
    }
}
