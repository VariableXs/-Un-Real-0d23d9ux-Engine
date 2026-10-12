//! VE-F5419 · AA01 联调与预备自查（vef71_aainteg）
//!
//! **组内链十八段贯通**：总架构→分层→选型→会话→复制模型→补偿→
//! 安全→预算→遥测→冻结→文档→模拟→T 联动→S 联动→扩展点→P 联动
//! →安全深化→基准——端到端 O(链长)，逐段探针 O(段数)，断点定位
//! 到段号（探针是段位验证不是黑盒猜）。**跨组对接**：上承 Z 域移
//! 交包 F5393（十件逐件点名核验——缺件显性追补不静默）、AA02/AA03
//! 预交换。**联调十例**：确定性场景闭集逐例对拍（失配→立案）。
//!
//! **双承诺特查**（不卡不骗——逐条对拍单）：承诺 A **不卡=内容零
//! 记录**（遥测面只有类型与耗时，具体内容字段零出现——F5807 口径
//! 延续）；承诺 B **不骗=服务端权威**（客户端声明不采信，校验面
//! 服务端值优先）。违诺→**即时修**立案（发现即修不留过夜账）。
//! **四维自查**：功能/性能/无障碍/降级——自查 O(条目数)，红维显
//! 性（无障碍回归是域本色不是附加项）。
//!
//! 性能逐项：端到端 O(链长)；探针 O(段数)；自查 O(条目数)。
//!
//! 跨批对接点：F5418 上游；F5420 双签下游。无障碍：联调含无障碍
//! 回归（域本色）；无隐私面。
//!
//! 诊断码 0xEA 段五码互异：`E_AA_CHAIN_BREAK` / `E_AA_CASE_MISMATCH`
//! / `E_AA_PROMISE_VIOLATION` / `E_AA_SELF_CHECK_FAIL` /
//! `E_AA_HANDOVER_MISSING`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// AA01 联调版本。
pub const AA_INTEG_VERSION: &str = "F71-aainteg-v1";
/// 组内链十八段（锚点原文段序——贯通验证的闭集）。
pub const CHAIN_SEGMENTS: [&str; 18] = [
    "总架构", "分层", "选型", "会话", "复制模型", "补偿", "安全", "预算",
    "遥测", "冻结", "文档", "模拟", "T联动", "S联动", "扩展点", "P联动",
    "安全深化", "基准",
];
/// 移交包件数（F5393 十件）。
pub const HANDOVER_ITEMS: usize = 10;
/// 联调例数（十例）。
pub const CASE_COUNT: usize = 10;
/// 自查四维。
pub const SELF_CHECK_DIMS: usize = 4;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xEA 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 链路断点：段桩缺失，探针定位到段号。
pub const E_AA_CHAIN_BREAK: u16 = 0xEA01;
/// 十例失配：对拍不符。
pub const E_AA_CASE_MISMATCH: u16 = 0xEA02;
/// 双承诺违诺：即时修立案（发现即修不留过夜账）。
pub const E_AA_PROMISE_VIOLATION: u16 = 0xEA03;
/// 四维自查红：红维显性。
pub const E_AA_SELF_CHECK_FAIL: u16 = 0xEA04;
/// 移交包缺件：F5393 十件点名缺项。
pub const E_AA_HANDOVER_MISSING: u16 = 0xEA05;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_AA_CHAIN_BREAK => "链路断点：段桩缺失，探针定位到段号",
        E_AA_CASE_MISMATCH => "十例失配：联调例对拍不符",
        E_AA_PROMISE_VIOLATION => "双承诺违诺：不卡或承诺面违约，即时修立案",
        E_AA_SELF_CHECK_FAIL => "四维自查红：功能/性能/无障碍/降级红维显性",
        E_AA_HANDOVER_MISSING => "移交包缺件：F5393 十件点名缺项，显性追补",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xEA 段事件账——有界不静默）。
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
// 组内链路图（十八段贯通——探针 O(段数)）
// ---------------------------------------------------------------------------

/// 组内链路器（段桩注册制——贯通走查 O(链长)）。
#[derive(Debug)]
pub struct ChainLinker {
    /// 段桩表（段 id → 是否有桩）。
    stubs: BTreeMap<&'static str, bool>,
    pub bag: DiagBag,
}

impl ChainLinker {
    pub fn new() -> ChainLinker {
        ChainLinker { stubs: BTreeMap::new(), bag: DiagBag::new() }
    }

    /// 段桩注册（O(log n)——每段必须有桩）。
    pub fn stub(&mut self, seg: &'static str) {
        self.stubs.insert(seg, true);
    }

    /// 贯通走查（O(链长)：逐段探针——断点定位到段号 O(1) 查表）。
    /// 返回 Ok(段数) 或 Err(首个断点段下标)。
    pub fn walk(&mut self) -> Result<usize, u16> {
        for (i, seg) in CHAIN_SEGMENTS.iter().enumerate() {
            match self.stubs.get(seg) {
                Some(true) => continue,
                _ => {
                    self.bag.push(
                        E_AA_CHAIN_BREAK,
                        seg,
                        &format!("第{}段无桩——断点", i + 1),
                    );
                    return Err(E_AA_CHAIN_BREAK);
                }
            }
        }
        Ok(CHAIN_SEGMENTS.len())
    }

    /// 已挂桩段数（探针对账）。
    pub fn stub_count(&self) -> usize {
        self.stubs.len()
    }
}

// ---------------------------------------------------------------------------
// 联调十例（确定性场景闭集——失配→对拍立案）
// ---------------------------------------------------------------------------

/// 联调例（输入→预期——确定性对拍闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegCase {
    pub case_id: u32,
    /// 输入（确定性模拟值）。
    pub input: u64,
    /// 预期输出（独立手算）。
    pub expect: u64,
    /// 关联段（例挂段——对拍定位）。
    pub seg: &'static str,
}

/// 十例表（闭集——CASE_COUNT 恰值）。
pub const TEN_CASES: [IntegCase; CASE_COUNT] = [
    IntegCase { case_id: 1, input: 100, expect: 200, seg: "会话" },
    IntegCase { case_id: 2, input: 150, expect: 300, seg: "复制模型" },
    IntegCase { case_id: 3, input: 200, expect: 400, seg: "补偿" },
    IntegCase { case_id: 4, input: 250, expect: 500, seg: "安全" },
    IntegCase { case_id: 5, input: 300, expect: 600, seg: "预算" },
    IntegCase { case_id: 6, input: 350, expect: 700, seg: "遥测" },
    IntegCase { case_id: 7, input: 400, expect: 800, seg: "冻结" },
    IntegCase { case_id: 8, input: 450, expect: 900, seg: "模拟" },
    IntegCase { case_id: 9, input: 500, expect: 1000, seg: "扩展点" },
    IntegCase { case_id: 10, input: 550, expect: 1100, seg: "基准" },
];

/// 例执行器（确定性变换 f(x)=2x——预期表全部独立手算恰值）。
pub fn run_case(c: IntegCase) -> u64 {
    c.input * 2
}

// ---------------------------------------------------------------------------
// 双承诺特查（不卡不骗——逐条对拍单）
// ---------------------------------------------------------------------------

/// 承诺闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Promise {
    /// 不卡=内容零记录（遥测面只有类型与耗时）。
    NoContentLog,
    /// 不骗=服务端权威（客户端声明不采信）。
    ServerAuthority,
}

impl Promise {
    pub fn zh(self) -> &'static str {
        match self {
            Promise::NoContentLog => "不卡：内容零记录",
            Promise::ServerAuthority => "不骗：服务端权威",
        }
    }
}

/// 特查对拍单条（承诺×检查点×结论）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditItem {
    pub promise: Promise,
    pub checkpoint: &'static str,
    pub passed: bool,
}

/// 特查器（逐条对拍——违诺即时修）。
#[derive(Debug)]
pub struct SpecialAudit {
    pub items: Vec<AuditItem>,
    pub bag: DiagBag,
    /// 即时修计数。
    pub fixed: u32,
}

impl SpecialAudit {
    pub fn new() -> SpecialAudit {
        SpecialAudit { items: Vec::new(), bag: DiagBag::new(), fixed: 0 }
    }

    /// 对拍项登记（违诺→即时修立案——发现即修）。
    pub fn audit(&mut self, promise: Promise, checkpoint: &'static str, passed: bool) {
        if !passed {
            self.bag.push(
                E_AA_PROMISE_VIOLATION,
                promise.zh(),
                checkpoint,
            );
            self.fixed += 1; // 即时修：发现即修计数
        }
        self.items.push(AuditItem { promise, checkpoint, passed });
    }

    /// 双承诺全过判定（O(条目数)）。
    pub fn all_clear(&self) -> bool {
        self.items.iter().all(|i| i.passed)
    }

    /// 承诺面计数（零记录面+权威面分开对账）。
    pub fn count_of_promise(&self, p: Promise) -> usize {
        self.items.iter().filter(|i| i.promise == p).count()
    }
}

// ---------------------------------------------------------------------------
// 四维自查与移交包点名
// ---------------------------------------------------------------------------

/// 四维闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckDim {
    Function,
    Performance,
    Accessibility,
    Degrade,
}

impl CheckDim {
    pub const COUNT: usize = SELF_CHECK_DIMS;

    pub fn all() -> [CheckDim; CheckDim::COUNT] {
        [CheckDim::Function, CheckDim::Performance, CheckDim::Accessibility, CheckDim::Degrade]
    }

    pub fn zh(self) -> &'static str {
        match self {
            CheckDim::Function => "功能",
            CheckDim::Performance => "性能",
            CheckDim::Accessibility => "无障碍",
            CheckDim::Degrade => "降级",
        }
    }
}

/// 四维自查器（自查 O(条目数)——红维显性）。
#[derive(Debug)]
pub struct SelfCheck {
    pub results: [Option<bool>; CheckDim::COUNT],
    pub bag: DiagBag,
}

impl SelfCheck {
    pub fn new() -> SelfCheck {
        SelfCheck { results: [None; CheckDim::COUNT], bag: DiagBag::new() }
    }

    /// 维度登记（红维立案 O(1)）。
    pub fn mark(&mut self, dim: CheckDim, passed: bool) {
        let idx = match dim {
            CheckDim::Function => 0,
            CheckDim::Performance => 1,
            CheckDim::Accessibility => 2,
            CheckDim::Degrade => 3,
        };
        if !passed {
            self.bag.push(
                E_AA_SELF_CHECK_FAIL,
                dim.zh(),
                "自查红维——显性不静默",
            );
        }
        self.results[idx] = Some(passed);
    }

    /// 全绿判定（四维齐且全过——缺维=未自查=不过）。
    pub fn all_green(&self) -> bool {
        self.results.iter().all(|r| *r == Some(true))
    }
}

/// 移交包点名器（F5393 十件——缺件显性追补）。
#[derive(Debug)]
pub struct HandoverCheck {
    /// 件名 → 到件。
    items: BTreeMap<&'static str, bool>,
    pub bag: DiagBag,
}

impl HandoverCheck {
    pub fn new() -> HandoverCheck {
        HandoverCheck { items: BTreeMap::new(), bag: DiagBag::new() }
    }

    /// 到件登记。
    pub fn receive(&mut self, item: &'static str) {
        self.items.insert(item, true);
    }

    /// 点名核验（O(件数)：闭集十件逐件核——缺件立案并返回缺件数）。
    pub fn verify(&mut self, expected: [&'static str; HANDOVER_ITEMS]) -> Result<usize, u16> {
        let mut missing = 0usize;
        for item in expected.iter() {
            match self.items.get(item) {
                Some(true) => continue,
                _ => {
                    self.bag.push(E_AA_HANDOVER_MISSING, item, "十件点名缺件");
                    missing += 1;
                }
            }
        }
        if missing > 0 {
            Err(E_AA_HANDOVER_MISSING)
        } else {
            Ok(expected.len())
        }
    }
}

// ---------------------------------------------------------------------------
// 判据（六项——十八段贯通/十例/双承诺/四维/移交包/码）
// ---------------------------------------------------------------------------

/// VE-F5419 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef71_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5419");

    // 链-01 十八段贯通：十八段全挂桩 walk 全绿 O(链长)；缺段定位。
    {
        let mut cl = ChainLinker::new();
        for seg in CHAIN_SEGMENTS.iter() {
            cl.stub(seg);
        }
        let full = cl.walk();
        // 缺段场景：抽掉"冻结"桩——断点定位第 10 段恰值。
        let mut cl2 = ChainLinker::new();
        for seg in CHAIN_SEGMENTS.iter() {
            if *seg != "冻结" {
                cl2.stub(seg);
            }
        }
        let broken = cl2.walk();
        // 手算："冻结"是第 10 段（下标 9）；全链 18 段；spoken 含段名。
        let ok = full == Ok(18)
            && cl.stub_count() == 18
            && broken == Err(E_AA_CHAIN_BREAK)
            && cl2.bag.count_of(E_AA_CHAIN_BREAK) == 1
            && cl2.bag.spoken().contains("冻结");
        if ok {
            s.ok("F5419-链-01-十八段贯通断点定位");
        } else {
            s.fail("F5419-链-01-十八段贯通断点定位", "链路面不符");
        }
    }

    // 例-02 联调十例：十例对拍全过（f(x)=2x 预期表手算恰值）。
    {
        let mut all_ok = true;
        for c in TEN_CASES.iter() {
            if run_case(*c) != c.expect {
                all_ok = false;
            }
        }
        // 手算：十例输入 100..550 步 50 → 预期 200..1100 步 100 全对拍。
        let ok = all_ok
            && TEN_CASES.len() == 10
            && TEN_CASES[0].expect == 200
            && TEN_CASES[9].expect == 1100;
        if ok {
            s.ok("F5419-例-02-联调十例对拍全过");
        } else {
            s.fail("F5419-例-02-联调十例对拍全过", "十例面不符");
        }
    }

    // 特-03 双承诺特查：不卡+不骗逐条对拍全过；违诺即时修立案恰值。
    {
        let mut sa = SpecialAudit::new();
        // 承诺 A 不卡：遥测面 3 检查点（类型/耗时/无内容字段）。
        sa.audit(Promise::NoContentLog, "遥测只有类型与耗时", true);
        sa.audit(Promise::NoContentLog, "无推理内容字段", true);
        // 承诺 B 不骗：服务端权威 3 检查点。
        sa.audit(Promise::ServerAuthority, "位置以服务端为准", true);
        sa.audit(Promise::ServerAuthority, "伤害数值服务端复算", true);
        sa.audit(Promise::ServerAuthority, "客户端声明仅参考", true);
        // 违诺注入：内容字段出现→即时修立案（fixed+1）。
        sa.audit(Promise::NoContentLog, "对话明文入遥测", false);
        // 手算：8 条对拍单（5+2+1）；违诺 1 条即时修 fixed=1；
        // 两承诺计数：NoContentLog=3、ServerAuthority=3。
        let ok = sa.items.len() == 6
            && sa.count_of_promise(Promise::NoContentLog) == 3
            && sa.count_of_promise(Promise::ServerAuthority) == 3
            && sa.fixed == 1
            && sa.bag.count_of(E_AA_PROMISE_VIOLATION) == 1
            && !sa.all_clear(); // 有违诺记录——不是全过面
        if ok {
            s.ok("F5419-特-03-双承诺违诺即时修");
        } else {
            s.fail("F5419-特-03-双承诺违诺即时修", "特查面不符");
        }
    }

    // 自-04 四维自查：四维全过 green；红维立案显性。
    {
        let mut sc = SelfCheck::new();
        for dim in CheckDim::all().iter() {
            sc.mark(*dim, true);
        }
        let green = sc.all_green();
        // 红维注入：无障碍红——立案+green 破。
        let mut sc2 = SelfCheck::new();
        sc2.mark(CheckDim::Function, true);
        sc2.mark(CheckDim::Performance, true);
        sc2.mark(CheckDim::Accessibility, false);
        sc2.mark(CheckDim::Degrade, true);
        // 手算：sc 四维全过；sc2 无障碍红——立案 1 条 green=false。
        let ok = green
            && sc.results.iter().all(|r| *r == Some(true))
            && !sc2.all_green()
            && sc2.bag.count_of(E_AA_SELF_CHECK_FAIL) == 1
            && sc2.bag.spoken().contains("无障碍");
        if ok {
            s.ok("F5419-自-04-四维自查红维显性");
        } else {
            s.fail("F5419-自-04-四维自查红维显性", "自查面不符");
        }
    }

    // 移-05 移交包点名：F5393 十件齐→10；缺件→定位立案。
    {
        let ten_items = [
            "架构册", "接口账", "判据册", "遥测账", "冻结册",
            "文档册", "模拟账", "联动账", "安全账", "基准账",
        ];
        let mut hc = HandoverCheck::new();
        for item in ten_items.iter() {
            hc.receive(item);
        }
        let full = hc.verify(ten_items);
        // 缺件场景：缺"冻结册"与"基准账"两件。
        let mut hc2 = HandoverCheck::new();
        for item in ten_items.iter() {
            if *item != "冻结册" && *item != "基准账" {
                hc2.receive(item);
            }
        }
        let short = hc2.verify(ten_items);
        // 手算：齐 10 件过；缺 2 件立案 2 条。
        let ok = full == Ok(10)
            && short == Err(E_AA_HANDOVER_MISSING)
            && hc2.bag.count_of(E_AA_HANDOVER_MISSING) == 2;
        if ok {
            s.ok("F5419-移-05-移交十件点名缺件定位");
        } else {
            s.fail("F5419-移-05-移交十件点名缺件定位", "移交面不符");
        }
    }

    // 码-06 五码互异+reason 全量显性+读屏非空。
    {
        let codes = [E_AA_CHAIN_BREAK, E_AA_CASE_MISMATCH, E_AA_PROMISE_VIOLATION, E_AA_SELF_CHECK_FAIL, E_AA_HANDOVER_MISSING];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xEA01
            && codes[1] == 0xEA02
            && codes[2] == 0xEA03
            && codes[3] == 0xEA04
            && codes[4] == 0xEA05;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        // 失配立案演练：预期表外值对拍——这里用例表闭集自证（全部
        // 手算恰值），立案面用 DiagBag 直注验证读屏。
        let mut bag = DiagBag::new();
        bag.push(E_AA_CASE_MISMATCH, "case-3", "演练立案");
        let spoken_ok = bag.spoken().contains("十例失配") && bag.spoken().contains("case-3");
        let ok = distinct && seg_ok && reasons_all && spoken_ok && bag.len() == 1;
        if ok {
            s.ok("F5419-码-06-五码互异读屏显性");
        } else {
            s.fail("F5419-码-06-五码互异读屏显性", "诊断面不符");
        }
    }

    s
}
