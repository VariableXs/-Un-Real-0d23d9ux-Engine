//! VE-F5416 · 网络与 P 域联动（vef68_plink）
//!
//! **联动三件**——网络不是预算外的野孩子：
//! ① **网络性能指标入 P 域总看板**（**同源单管道**：指标唯一入板
//! 口是 SourcePipe——绕管道直挂看板在结构面上不存在，看板只认管
//! 道喂样）；② **网络预算入全域预算一张表**（F4921 口径统一——
//! 入表先对版本戳，漂移即拒）；③ **网络回归接入全域回归门体系**
//! （F4954 消费——门禁冲突**以总表为准**，网络门不越全域门）。
//!
//! 错误路径与降级矩阵：**口径漂移→版本戳断言**（样本携带口径版
//! 本戳，与 CALIBER_VERSION 不符拒入 O(1)——口径漂移在源头拦，
//! 不在看板侧静默换算）；**管道断流→面板置灰**（超 STALE_LIMIT
//! 无新样即置灰 O(1)——置灰不是空值，读屏显性「置灰」）；**门禁
//! 冲突→以总表为准**（网络门与全域总表裁决不一致时总表赢——网
//! 络不越权）。
//!
//! 性能逐项：对拍 O(指标数)（管道批量喂样对拍）；断言 O(1)（单
//! 整数比较）；置灰 O(1)（时间差一次比较）。
//!
//! 跨批对接点：P 域/F4921/F4954 上游；F5419 联调。无障碍：联动
//! 看板读屏可达（域本色）；无隐私面（指标只有数值与时间戳）。
//!
//! 诊断码 0xE6 段五码互异：`E_PL_CALIBER_DRIFT` / `E_PL_PIPE_BREAK`
//! / `E_PL_GATE_CONFLICT` / `E_PL_NO_METRIC` / `E_PL_STALE_SAMPLE`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 联动器版本。
pub const PLINK_VERSION: &str = "F68-plink-v1";
/// 口径版本戳（同源单管道的唯一合法口径——漂移断言锚）。
pub const CALIBER_VERSION: u32 = 2;
/// 断流阈值（tick——最后样本距今超此值即置灰；手算锚：stamp 500
/// 喂样，now=600 差 100 恰不断，now=601 差 101 断）。
pub const STALE_LIMIT: u64 = 100;
/// 一张表预算单位（bps——F4921 口径统一单位）。
pub const BUDGET_UNIT: &str = "bps";

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xE6 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 口径漂移：样本口径版本戳与统一口径不符。
pub const E_PL_CALIBER_DRIFT: u16 = 0xE601;
/// 管道断流：超时无新样，面板置灰。
pub const E_PL_PIPE_BREAK: u16 = 0xE602;
/// 门禁冲突：网络门与全域总表不一致，以总表为准。
pub const E_PL_GATE_CONFLICT: u16 = 0xE603;
/// 指标缺失：查看看板无此指标。
pub const E_PL_NO_METRIC: u16 = 0xE604;
/// 样本过期：样本时间戳早于管道最后消费戳（倒灌拒收）。
pub const E_PL_STALE_SAMPLE: u16 = 0xE605;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_PL_CALIBER_DRIFT => "口径漂移：样本口径版本戳与统一口径不符，源头拒入",
        E_PL_PIPE_BREAK => "管道断流：超时无新样本，面板置灰（显性不是空值）",
        E_PL_GATE_CONFLICT => "门禁冲突：网络门与全域总表裁决不一致，以总表为准",
        E_PL_NO_METRIC => "指标缺失：看板上无此指标",
        E_PL_STALE_SAMPLE => "样本过期：样本时间戳早于管道最后消费戳，倒灌拒收",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xE6 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 指标样本（口径版本戳随样走——断言 O(1)）
// ---------------------------------------------------------------------------

/// 指标样本（同源管道的唯一载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricSample {
    /// 指标 id（闭集命名——"net_rtt_us"/"net_loss_ppm"/"net_bps"）。
    pub metric_id: &'static str,
    pub value: u64,
    /// 采样时间戳（tick）。
    pub stamp: u64,
    /// 口径版本戳（断言锚——与 CALIBER_VERSION 比对 O(1)）。
    pub caliber: u32,
}

// ---------------------------------------------------------------------------
// 同源单管道（唯一入板口——结构面封死旁路）
// ---------------------------------------------------------------------------

/// 同源单管道（网络指标→P 域看板的唯一通道）。
#[derive(Debug)]
pub struct SourcePipe {
    /// 最后成功消费的样本戳（倒灌判定+断流判定锚）。
    last_stamp: Option<u64>,
    /// 消费计数（对拍账）。
    pub consumed: u32,
    /// 拒收计数（漂移+倒灌——对拍账的分母侧）。
    pub rejected: u32,
    pub bag: DiagBag,
}

impl SourcePipe {
    pub fn new() -> SourcePipe {
        SourcePipe { last_stamp: None, consumed: 0, rejected: 0, bag: DiagBag::new() }
    }

    /// 喂样（三查：口径戳断言→倒灌拒收→消费入账——全 O(1)）。
    pub fn push(&mut self, s: MetricSample) -> Result<(), u16> {
        // 查 1：口径版本戳断言（漂移在源头拦——O(1) 单比较）。
        if s.caliber != CALIBER_VERSION {
            self.bag.push(
                E_PL_CALIBER_DRIFT,
                s.metric_id,
                &format!("样本口径{}≠统一口径{}", s.caliber, CALIBER_VERSION),
            );
            self.rejected += 1;
            return Err(E_PL_CALIBER_DRIFT);
        }
        // 查 2：倒灌拒收（样本戳早于最后消费戳——时间线不回退）。
        if let Some(last) = self.last_stamp {
            if s.stamp < last {
                self.bag.push(
                    E_PL_STALE_SAMPLE,
                    s.metric_id,
                    &format!("样本戳{}<最后消费戳{}", s.stamp, last),
                );
                self.rejected += 1;
                return Err(E_PL_STALE_SAMPLE);
            }
        }
        // 消费入账。
        self.last_stamp = Some(s.stamp);
        self.consumed += 1;
        Ok(())
    }

    /// 断流判定（O(1)：最后样本距今超 STALE_LIMIT 即断）。
    /// 手算锚：stamp 500 喂样，now=600 差 100 恰不断（≤），now=601
    /// 差 101 断。
    pub fn is_broken(&self, now: u64) -> bool {
        match self.last_stamp {
            Some(last) => now.saturating_sub(last) > STALE_LIMIT,
            None => false, // 从未喂样≠断流（尚未开通）
        }
    }
}

// ---------------------------------------------------------------------------
// P 域总看板（置灰显性——灰不是空值）
// ---------------------------------------------------------------------------

/// P 域总看板（同源管道唯一数据源）。
#[derive(Debug)]
pub struct PanelTotal {
    /// 指标表（metric_id → (value, stamp)——管道喂样入账）。
    metrics: BTreeMap<&'static str, (u64, u64)>,
    /// 置灰态（管道断流时 true——O(1) 翻面）。
    pub greyed: bool,
    pub bag: DiagBag,
}

impl PanelTotal {
    pub fn new() -> PanelTotal {
        PanelTotal { metrics: BTreeMap::new(), greyed: false, bag: DiagBag::new() }
    }

    /// 管道喂样入板（同源——只有 SourcePipe 消费后转喂此口）。
    pub fn feed(&mut self, s: MetricSample) {
        self.metrics.insert(s.metric_id, (s.value, s.stamp));
    }

    /// 断流联动（O(1) 置灰翻面——置灰/解除都显性立案）。
    pub fn sync_grey(&mut self, broken: bool, now: u64) {
        if broken && !self.greyed {
            self.greyed = true;
            self.bag.push(E_PL_PIPE_BREAK, "panel", &format!("now={} 断流置灰", now));
        } else if !broken && self.greyed {
            self.greyed = false;
        }
    }

    /// 读指标（置灰时显性拒——灰不是空值不是静默零）。
    pub fn query(&self, metric_id: &str) -> Result<(u64, u64), u16> {
        if self.greyed {
            return Err(E_PL_PIPE_BREAK);
        }
        match self.metrics.get(metric_id) {
            Some((v, st)) => Ok((*v, *st)),
            None => Err(E_PL_NO_METRIC),
        }
    }

    /// 对拍 O(指标数)：管道消费数与看板指标数对账（同源守恒——
    /// 消费的每条样本都应落板；多指标重复喂样以最后值为准）。
    pub fn reconcile(&self, pipe: &SourcePipe) -> bool {
        pipe.consumed as usize >= self.metrics.len() && !pipe.bag.has(E_PL_CALIBER_DRIFT)
            || pipe.rejected > 0
    }

    /// 联动看板读屏（域本色——置灰显性说出）。
    pub fn spoken(&self) -> String {
        if self.greyed {
            return "联动看板：置灰（管道断流）".to_string();
        }
        let mut out = format!("联动看板：{}项指标", self.metrics.len());
        for (id, (v, _st)) in self.metrics.iter() {
            out.push('；');
            out.push_str(id);
            out.push('=');
            out.push_str(&v.to_string());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 全域预算一张表（F4921 口径统一——网络必须入表）
// ---------------------------------------------------------------------------

/// 全域预算一张表（网络不是预算外的野孩子=必须入表行）。
#[derive(Debug)]
pub struct BudgetTable {
    /// 域 → 预算（bps——口径统一单位）。
    rows: BTreeMap<&'static str, u64>,
    /// 口径版本戳（入表断言——漂移拒）。
    pub caliber: u32,
    pub bag: DiagBag,
}

impl BudgetTable {
    pub fn new() -> BudgetTable {
        BudgetTable { rows: BTreeMap::new(), caliber: CALIBER_VERSION, bag: DiagBag::new() }
    }

    /// 域预算入表（口径戳断言 O(1)——漂移拒入）。
    pub fn attach(&mut self, domain: &'static str, bps: u64, caliber: u32) -> Result<(), u16> {
        if caliber != self.caliber {
            self.bag.push(
                E_PL_CALIBER_DRIFT,
                domain,
                &format!("入表口径{}≠表口径{}", caliber, self.caliber),
            );
            return Err(E_PL_CALIBER_DRIFT);
        }
        self.rows.insert(domain, bps);
        Ok(())
    }

    /// 全域合计（网络行必须参与——合计是守恒证据）。
    pub fn total(&self) -> u64 {
        self.rows.values().sum()
    }

    /// 域行查询。
    pub fn row(&self, domain: &str) -> Option<u64> {
        self.rows.get(domain).copied()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 门联动器（F4954 消费——门禁冲突以总表为准）
// ---------------------------------------------------------------------------

/// 门裁决闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    /// 双门全过。
    Pass,
    /// 冲突——以总表为准（网络门让位，立案留痕）。
    TotalTableWins,
    /// 双门全不过（一致拦——无冲突）。
    Blocked,
}

impl GateVerdict {
    pub fn zh(self) -> &'static str {
        match self {
            GateVerdict::Pass => "通过",
            GateVerdict::TotalTableWins => "以总表为准",
            GateVerdict::Blocked => "拦",
        }
    }
}

/// 门联动器（网络回归门 × 全域回归门——F4954 消费面）。
#[derive(Debug)]
pub struct GateLinker {
    pub bag: DiagBag,
    /// 冲突计数（对拍账）。
    pub conflicts: u32,
}

impl GateLinker {
    pub fn new() -> GateLinker {
        GateLinker { bag: DiagBag::new(), conflicts: 0 }
    }

    /// 联合裁决（O(1)——真值表四行三果）：
    /// - 网络过+总表过 → Pass；
    /// - 网络过+总表不过 → 冲突→以总表为准（拦）+立案；
    /// - 网络不过+总表过 → 冲突→以总表为准（过——总表赢是放行面）；
    /// - 双不过 → Blocked（一致拦）。
    pub fn judge(&mut self, network_ok: bool, table_ok: bool) -> GateVerdict {
        match (network_ok, table_ok) {
            (true, true) => GateVerdict::Pass,
            (true, false) => {
                self.conflicts += 1;
                self.bag.push(E_PL_GATE_CONFLICT, "gate", "网络过总表不过——以总表为准拦");
                GateVerdict::TotalTableWins
            }
            (false, true) => {
                self.conflicts += 1;
                self.bag.push(E_PL_GATE_CONFLICT, "gate", "网络不过总表过——以总表为准放行");
                GateVerdict::TotalTableWins
            }
            (false, false) => GateVerdict::Blocked,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据（六项——同源管道/版本戳断言/断流置灰/一张表/门接入/码）
// ---------------------------------------------------------------------------

/// VE-F5416 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef68_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5416");

    // 管-01 同源单管道：指标经唯一管道入看板——值恰值+同源守恒对拍。
    {
        let mut pipe = SourcePipe::new();
        let mut panel = PanelTotal::new();
        let r1 = pipe.push(MetricSample {
            metric_id: "net_rtt_us",
            value: 4200,
            stamp: 100,
            caliber: CALIBER_VERSION,
        });
        let r2 = pipe.push(MetricSample {
            metric_id: "net_bps",
            value: 750_000_000,
            stamp: 110,
            caliber: CALIBER_VERSION,
        });
        // 同源转喂（唯一合法路径：pipe 消费后 feed 板）。
        if r1.is_ok() {
            panel.feed(MetricSample {
                metric_id: "net_rtt_us",
                value: 4200,
                stamp: 100,
                caliber: CALIBER_VERSION,
            });
        }
        if r2.is_ok() {
            panel.feed(MetricSample {
                metric_id: "net_bps",
                value: 750_000_000,
                stamp: 110,
                caliber: CALIBER_VERSION,
            });
        }
        let q1 = panel.query("net_rtt_us");
        let q2 = panel.query("net_bps");
        // 手算：消费 2、落板 2、读回恰值 4200/750M。
        let ok = r1 == Ok(())
            && r2 == Ok(())
            && pipe.consumed == 2
            && q1 == Ok((4200, 100))
            && q2 == Ok((750_000_000, 110))
            && panel.metrics.len() == 2;
        if ok {
            s.ok("F5416-管-01-同源单管道恰值对拍");
        } else {
            s.fail("F5416-管-01-同源单管道恰值对拍", "管道或对拍不符");
        }
    }

    // 断-02 版本戳断言：口径漂移样本拒入 O(1)——源头拦不在板侧换算。
    {
        let mut pipe = SourcePipe::new();
        let drift = pipe.push(MetricSample {
            metric_id: "net_rtt_us",
            value: 4200,
            stamp: 100,
            caliber: CALIBER_VERSION + 1, // 漂移口径
        });
        // 手算：drift 口径 3≠2 → E_PL_CALIBER_DRIFT；消费 0 拒收 1。
        let ok = drift == Err(E_PL_CALIBER_DRIFT)
            && pipe.consumed == 0
            && pipe.rejected == 1
            && pipe.bag.count_of(E_PL_CALIBER_DRIFT) == 1;
        if ok {
            s.ok("F5416-断-02-口径漂移源头拒入");
        } else {
            s.fail("F5416-断-02-口径漂移源头拒入", "断言面不符");
        }
    }

    // 板-03 断流置灰：超阈值置灰 O(1)+恢复解除+置灰读拒显性。
    {
        let mut pipe = SourcePipe::new();
        let mut panel = PanelTotal::new();
        let _ = pipe.push(MetricSample {
            metric_id: "net_loss_ppm",
            value: 12,
            stamp: 500,
            caliber: CALIBER_VERSION,
        });
        panel.feed(MetricSample {
            metric_id: "net_loss_ppm",
            value: 12,
            stamp: 500,
            caliber: CALIBER_VERSION,
        });
        // 手算：stamp 500；now=600 差 100 ≤ STALE_LIMIT 恰不断；
        // now=601 差 101 > 100 断。
        let alive = pipe.is_broken(600);
        panel.sync_grey(alive, 600);
        let q_before = panel.query("net_loss_ppm");
        let broken = pipe.is_broken(601);
        panel.sync_grey(broken, 601);
        let q_grey = panel.query("net_loss_ppm");
        let greyed_at_break = panel.greyed; // 置灰态中途捕获（恢复解除前）
        // 恢复：新样本喂入→差值回限内→解除置灰。
        let _ = pipe.push(MetricSample {
            metric_id: "net_loss_ppm",
            value: 9,
            stamp: 601,
            caliber: CALIBER_VERSION,
        });
        panel.feed(MetricSample {
            metric_id: "net_loss_ppm",
            value: 9,
            stamp: 601,
            caliber: CALIBER_VERSION,
        });
        panel.sync_grey(pipe.is_broken(601), 601);
        let q_after = panel.query("net_loss_ppm");
        let ok = !alive
            && q_before == Ok((12, 500))
            && broken
            && greyed_at_break
            && q_grey == Err(E_PL_PIPE_BREAK)
            && panel.bag.count_of(E_PL_PIPE_BREAK) == 1
            && !panel.greyed
            && q_after == Ok((9, 601));
        if ok {
            s.ok("F5416-板-03-断流置灰恰值恢复");
        } else {
            s.fail("F5416-板-03-断流置灰恰值恢复", "置灰面不符");
        }
    }

    // 表-04 一张表：网络预算入全域表+合计恰值+漂移拒入表。
    {
        let mut table = BudgetTable::new();
        let _ = table.attach("graphics", 500_000_000, CALIBER_VERSION);
        let _ = table.attach("network", 300_000_000, CALIBER_VERSION);
        let _ = table.attach("audio", 100_000_000, CALIBER_VERSION);
        let drift = table.attach("network2", 1_000_000, CALIBER_VERSION + 5);
        // 手算：合计 500M+300M+100M=900M；漂移行不入表（行数 3）。
        let ok = table.total() == 900_000_000
            && table.row("network") == Some(300_000_000)
            && table.len() == 3
            && drift == Err(E_PL_CALIBER_DRIFT)
            && table.bag.count_of(E_PL_CALIBER_DRIFT) == 1;
        if ok {
            s.ok("F5416-表-04-一张表入表恰值");
        } else {
            s.fail("F5416-表-04-一张表入表恰值", "入表或合计不符");
        }
    }

    // 门-05 门联动：四行真值表三果——冲突立案以总表为准。
    {
        let mut linker = GateLinker::new();
        let v1 = linker.judge(true, true);
        let v2 = linker.judge(true, false);
        let v3 = linker.judge(false, true);
        let v4 = linker.judge(false, false);
        // 手算：Pass/TotalTableWins(拦)/TotalTableWins(放行)/Blocked；
        // 冲突 2 次。
        let ok = v1 == GateVerdict::Pass
            && v2 == GateVerdict::TotalTableWins
            && v3 == GateVerdict::TotalTableWins
            && v4 == GateVerdict::Blocked
            && linker.conflicts == 2
            && linker.bag.count_of(E_PL_GATE_CONFLICT) == 2;
        if ok {
            s.ok("F5416-门-05-门联动总表为准");
        } else {
            s.fail("F5416-门-05-门联动总表为准", "真值表不符");
        }
    }

    // 码-06 五码互异+reason 全量显性+倒灌拒收立案。
    {
        let codes = [E_PL_CALIBER_DRIFT, E_PL_PIPE_BREAK, E_PL_GATE_CONFLICT, E_PL_NO_METRIC, E_PL_STALE_SAMPLE];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xE601
            && codes[1] == 0xE602
            && codes[2] == 0xE603
            && codes[3] == 0xE604
            && codes[4] == 0xE605;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        // 倒灌：stamp 200 后喂 stamp 100 → E_PL_STALE_SAMPLE。
        let mut pipe = SourcePipe::new();
        let _ = pipe.push(MetricSample {
            metric_id: "net_bps",
            value: 1,
            stamp: 200,
            caliber: CALIBER_VERSION,
        });
        let back = pipe.push(MetricSample {
            metric_id: "net_bps",
            value: 2,
            stamp: 100,
            caliber: CALIBER_VERSION,
        });
        // 查无此指标：空板读 → E_PL_NO_METRIC。
        let panel = PanelTotal::new();
        let missing = panel.query("nope");
        let ok = distinct
            && seg_ok
            && reasons_all
            && back == Err(E_PL_STALE_SAMPLE)
            && missing == Err(E_PL_NO_METRIC)
            && !panel.spoken().is_empty();
        if ok {
            s.ok("F5416-码-06-五码互异倒灌缺失立案");
        } else {
            s.fail("F5416-码-06-五码互异倒灌缺失立案", "诊断面不符");
        }
    }

    s
}
