//! VE-F1616 · 域自检（判据逐条对应，见 `vef19_geobench.rs` 头注）
//!
//! **判据（锚点原文）**：四基准、入册、判据。
//!
//! 三族分列规避 `MAX_CHECKS` 截断：
//! - **a 族**：加载基准 + 量化基准（工作量非平凡、解析析算字节、压缩比
//!   判据侧重算、双桶预算）；
//! - **b 族**：简化基准 + 重排基准（面比、质量双指标、收益为正、确定性
//!   打乱可复现）；
//! - **c 族**：入册回归（版本化记录、不可比/退化/持平三态双向验证——
//!   变异基线注入测试判定器本身）+ 判据集自检。
//!
//! 判据侧纪律：**基线非平凡**（先证四基准跑出非零工作量）；**独立重算**
//! （压缩比/预算系数/面比期望由本侧写死）；**变异双向**（回归判定器
//! 用「基线×2 / 基线÷2 / 版本+1」三个注入变体证明 Degraded/Improved/
//! Incomparable 三态真可达——判定器恒绿的判据是弱门禁）。

use crate::checks::CheckSet;
use crate::svstar2::vef19_geobench as gb;

// 判据侧写死期望（不从被测反推）。
mod expect {
    /// 加载工作负载（锚点明列 10000）。
    pub const LOAD_MESHES: u32 = 10_000;
    /// 立方体流字节（8 头 + 8×12 顶点 + 12×12 面）。
    pub const CUBE_STREAM_BYTES: u64 = 8 + 96 + 144;
    /// 压缩比期望（B16：12B → 6B = 200%）。
    pub const RATIO_PCT_B16: u64 = 200;
    /// 移动预算系数。
    pub const MOBILE_FACTOR: u64 = 4;
    /// 语料面数（17×17 网格 → 16×16×2）。
    pub const CORPUS_FACES: u32 = 512;
    /// 语料顶点数。
    pub const CORPUS_VERTS: u32 = 289;
    /// 简化目标面比。
    pub const SIMPLIFY_TARGET_PCT: u64 = 50;
    /// 四域数。
    pub const DOMAINS: usize = 4;
    /// 双桶数。
    pub const BUCKETS: usize = 2;
    /// 入册满额（4×2）。
    pub const REGISTRY_FULL: usize = 8;
}

// ---------------------------------------------------------------------------
// a 族：加载 + 量化
// ---------------------------------------------------------------------------
fn c1616_load_quant(s: &mut CheckSet) {
    // ① 四域全集恰好四且短码可逆、未知短码不误映射。
    let d = gb::BenchDomain::from_wire(gb::BenchDomain::ALL[0].wire());
    s.add(
        "四域短码可逆且未知短码不误映射",
        gb::BenchDomain::ALL.len() == expect::DOMAINS
            && d == Some(gb::BenchDomain::Load)
            && gb::BenchDomain::from_wire("zz").is_none(),
        "",
    );

    // ② 双桶全集恰好二。
    s.add("双桶全集恰好二（桌面/移动）", gb::Bucket::ALL.len() == expect::BUCKETS, "");

    // ③ 移动预算 = 桌面 × 系数（判据侧独立重算，逐域）。
    let mut factor_ok = true;
    let mut i = 0;
    while i < gb::BenchDomain::ALL.len() {
        let dom = gb::BenchDomain::ALL[i];
        if gb::budget_ops(dom, gb::Bucket::Mobile)
            != gb::budget_ops(dom, gb::Bucket::Desktop) * expect::MOBILE_FACTOR
        {
            factor_ok = false;
        }
        i += 1;
    }
    s.add("移动预算=桌面×系数（逐域重算）", factor_ok, "跨设备差异显性化的预算双轨");

    // ④ 加载基准：满额成功 + 字节析算一致（10000×流长，判据侧独立算）。
    let lb = gb::run_load_bench(gb::Bucket::Desktop);
    s.add(
        "加载基准满额成功",
        lb.meshes_ok == expect::LOAD_MESHES && lb.record.workload == expect::LOAD_MESHES,
        "解析失败即红——批体检底座不能带洞",
    );
    s.add(
        "加载总字节=网格数×流长（独立重算）",
        lb.total_bytes == (expect::LOAD_MESHES as u64) * expect::CUBE_STREAM_BYTES,
        "字节账被吞会让吞吐虚报",
    );
    s.add("加载 ops 非平凡且预算内", lb.record.ops > 0 && lb.within_budget, "");

    // ⑤ 量化基准：顶点数=语料锁定值，量化/解压对数=顶点×档数（独立重算）。
    let qb = gb::run_quant_bench(gb::Bucket::Desktop);
    s.add(
        "量化语料顶点数与版本锁定值一致",
        qb.record.workload == expect::CORPUS_VERTS,
        "语料被改必须升 CORPUS_REV，否则判红",
    );
    s.add(
        "量化/解压操作数=顶点×四档（独立重算）",
        qb.quant_ops == (expect::CORPUS_VERTS as u64) * 4
            && qb.dequant_ops == qb.quant_ops,
        "压缩与解压对称计量——只测压缩会把解压成本藏起来",
    );

    // ⑥ 压缩比=B16 下 200%（12B→6B，判据侧解析）。
    s.add(
        "压缩比 200%（12B 原始对 6B 打包）",
        qb.ratio_pct_b16 == expect::RATIO_PCT_B16
            && qb.packed_bytes_b16 * 2 == qb.raw_bytes,
        "三值量化的成本收益主指标",
    );
    s.add("量化 ops 预算内（桌面）", qb.within_budget, "");

    // ⑦ 双桶记录桶位正确且记录带版本/语料（缺任一即不可比隐患）。
    let qm = gb::run_quant_bench(gb::Bucket::Mobile);
    s.add(
        "记录带版本/语料/桶位三要素",
        qb.record.version == gb::BENCH_VERSION
            && qb.record.corpus_rev == gb::CORPUS_REV
            && qb.record.bucket == gb::Bucket::Desktop
            && qm.record.bucket == gb::Bucket::Mobile,
        "",
    );

    // ⑧ 记录单元标签互异且非空（人话可读——报告可审计）。
    let units = [lb.record.unit, qb.record.unit];
    s.add("记录单位标签非空", units.iter().all(|u| !u.is_empty()), "");
}

// ---------------------------------------------------------------------------
// b 族：简化 + 重排
// ---------------------------------------------------------------------------
fn c1616_simplify_reorder(s: &mut CheckSet) {
    // ① 简化基准：面数起点=语料锁定值，目标比 50% 达成。
    let sb = gb::run_simplify_bench(gb::Bucket::Desktop);
    s.add(
        "简化起点=语料面数（版本锁定）",
        sb.faces_before == expect::CORPUS_FACES,
        "",
    );
    s.add(
        "简化到达目标面比（50%±容差）",
        sb.faces_after as u64 * 100 <= sb.faces_before as u64 * (expect::SIMPLIFY_TARGET_PCT + 5)
            && sb.faces_after >= 4,
        "MIN_FACES 下限守恒",
    );

    // ② 收缩 ops 非零（QEM 真干了活）且预算内。
    s.add(
        "收缩 ops 非零且预算内",
        sb.collapse_ops > 0 && sb.within_budget,
        "零收缩=简化器空转，质量判据全失真",
    );

    // ③ 质量双指标：Hausdorff 记账非负、质量阈值内。
    s.add(
        "质量阈值内且 Hausdorff 如实记账",
        sb.quality_within && sb.hausdorff >= 0.0,
        "质量面缺失=只有速度没有收益",
    );

    // ④ 重排基准：面数=语料锁定值、ops=面数（主循环工作量口径）。
    let rb = gb::run_reorder_bench(gb::Bucket::Desktop);
    s.add(
        "重排工作负载=语料面数",
        rb.faces == expect::CORPUS_FACES && rb.record.ops == rb.faces as u64,
        "",
    );

    // ⑤ 收益为正（F1609 优化验证——劣化起点下 Forsyth 必须赢）。
    s.add(
        "命中率收益为正",
        rb.gain_positive && rb.miss_reduction > 0,
        "确定性打乱起点下收益为负=重排器失效",
    );

    // ⑥ 重排预算内（桌面+移动双桶）。
    let rm = gb::run_reorder_bench(gb::Bucket::Mobile);
    s.add(
        "重排双桶预算内",
        rb.within_budget && rm.within_budget,
        "",
    );

    // ⑦ 确定性打乱可复现：两次运行收益一致。
    let rb2 = gb::run_reorder_bench(gb::Bucket::Desktop);
    s.add(
        "重排两次运行收益一致（LCG 确定性）",
        rb2.gain_pct_x100 == rb.gain_pct_x100 && rb2.miss_reduction == rb.miss_reduction,
        "打乱种子写死=跨版本可复现",
    );

    // ⑧ 简化两次运行收缩数一致（确定性成本模型自证）。
    let sb2 = gb::run_simplify_bench(gb::Bucket::Desktop);
    s.add(
        "简化两次运行收缩数一致",
        sb2.collapse_ops == sb.collapse_ops && sb2.faces_after == sb.faces_after,
        "",
    );
}

// ---------------------------------------------------------------------------
// c 族：入册回归 + 判据集自检
// ---------------------------------------------------------------------------
fn c1616_registry(s: &mut CheckSet) {
    // ① 回归门全绿：退化 0、不可比 0、四结构判据全过。
    let rep = gb::run_regression_gate();
    s.add(
        "回归门零退化零不可比",
        rep.degraded == 0 && rep.incomparable == 0,
        "同代码状态两轮跑必须逐位一致",
    );
    s.add(
        "回归门四结构判据全过",
        rep.budgets_ok && rep.load_full && rep.ratio_ok && rep.quality_ok && rep.gain_ok,
        "",
    );

    // ② 入册满额：4 域 × 2 桶 = 8 条（缺域/缺桶即红）。
    s.add(
        "入册满额（4域×2桶）",
        rep.records == expect::REGISTRY_FULL,
        "四基准缺一即结构性失败",
    );

    // ③ 入册表容量上界：超容如实计 dropped 不覆盖旧行。
    let mut reg = gb::BenchRegistry::new();
    let base = gb::BenchRecord {
        domain: gb::BenchDomain::Load,
        bucket: gb::Bucket::Desktop,
        workload: 1,
        ops: 100,
        unit: "iter",
        version: gb::BENCH_VERSION,
        corpus_rev: gb::CORPUS_REV,
    };
    let mut i = 0;
    while i < gb::REGISTRY_CAP + 3 {
        reg.record(base);
        i += 1;
    }
    s.add(
        "入册超容拒绝且 dropped 如实",
        reg.len() == gb::REGISTRY_CAP && reg.dropped() == 3,
        "旧行是回归基线，覆盖=毁基线",
    );

    // ④ 判定器双向验证：基线×2 → Degraded（变异注入）。
    let mut v2 = base;
    v2.ops = base.ops * 2;
    s.add(
        "判定器：基线×2 判退化",
        matches!(v2.regression_against(&base), gb::RegVerdict::Degraded(_)),
        "判定器恒绿=弱门禁，变异注入证明真可达",
    );

    // ⑤ 判定器双向：基线÷2 → Improved。
    let mut v3 = base;
    v3.ops = base.ops / 2;
    s.add(
        "判定器：基线÷2 判变快",
        matches!(v3.regression_against(&base), gb::RegVerdict::Improved(_)),
        "",
    );

    // ⑥ 判定器：同值 → Same（持平带内）。
    s.add(
        "判定器：同 ops 判持平",
        matches!(base.regression_against(&base), gb::RegVerdict::Same(_)),
        "",
    );

    // ⑦ 判定器：版本+1 → 不可比（跨版本严格分账）。
    let mut v4 = base;
    v4.version = base.version + 1;
    match v4.regression_against(&base) {
        gb::RegVerdict::Incomparable("version_diff") => {
            s.add("判定器：跨版本判不可比", true, "");
        }
        _ => s.add("判定器：跨版本判不可比", false, "不可比与退化混账=跨版本对数自欺"),
    }

    // ⑧ 判定器：桶位不同 → 不可比（跨桶禁止直比）。
    let mut v5 = base;
    v5.bucket = gb::Bucket::Mobile;
    match v5.regression_against(&base) {
        gb::RegVerdict::Incomparable("bucket_diff") => {
            s.add("判定器：跨桶判不可比", true, "");
        }
        _ => s.add("判定器：跨桶判不可比", false, ""),
    }

    // ⑨ 判定器：基线 ops=0 → 不可比（零基线比不出退化——基线侧零值触发）。
    let cur = base;
    let base_zero = gb::BenchRecord { ops: 0, ..base };
    match cur.regression_against(&base_zero) {
        gb::RegVerdict::Incomparable("baseline_zero") => {
            s.add("判定器：零基线判不可比", true, "");
        }
        _ => s.add("判定器：零基线判不可比", false, ""),
    }

    // ⑩ 判定器：语料版本不同 → 不可比（语料变更即基准重校）。
    let mut v6 = base;
    v6.corpus_rev = base.corpus_rev + 1;
    match v6.regression_against(&base) {
        gb::RegVerdict::Incomparable("corpus_diff") => {
            s.add("判定器：跨语料判不可比", true, "");
        }
        _ => s.add("判定器：跨语料判不可比", false, ""),
    }

    // ⑪ latest_of 取到的是该域该桶最新一条（同键多条取尾；用净表防满容干扰）。
    let mut reg2 = gb::BenchRegistry::new();
    reg2.record(base);
    let mut tail = base;
    tail.ops = 150;
    reg2.record(tail);
    match reg2.latest_of(gb::BenchDomain::Load, gb::Bucket::Desktop) {
        Some(r) => s.add("latest_of 取同键最新（尾条）", r.ops == 150, ""),
        None => s.add("latest_of 取同键最新（尾条）", false, "入册后查无记录"),
    }

    // ⑦ 压缩比期望与被测常量单源（判据侧解析对拍）。
    s.add(
        "压缩比期望单源（200%）",
        gb::expect_ratio_pct_b16() == expect::RATIO_PCT_B16,
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据集自身自检
// ---------------------------------------------------------------------------
fn meta_checks(s: &mut CheckSet) {
    let mut a = CheckSet::new("vef19-meta");
    c1616_load_quant(&mut a);
    let mut b = CheckSet::new("vef19-meta");
    c1616_simplify_reorder(&mut b);
    let mut c = CheckSet::new("vef19-meta");
    c1616_registry(&mut c);
    let total = a.len() + b.len() + c.len() + 3;
    s.add(
        "判据集条数与 total_check_count 一致",
        total == total_check_count() as usize,
        "聚合口径漂移会让文档与实际脱节",
    );

    // 判据名全局互异。
    let mut names: Vec<&str> = Vec::new();
    collect_names(&a, &mut names);
    collect_names(&b, &mut names);
    collect_names(&c, &mut names);
    let mut all_differ = true;
    let mut i = 0;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("判据名全局互异", all_differ, "");

    s.add(
        "三族自检全绿",
        a.all_passed() && b.all_passed() && c.all_passed(),
        "任一判据红即在此可见",
    );
}

fn collect_names(set: &CheckSet, out: &mut Vec<&'static str>) {
    let (items, n) = set.red_items();
    let mut i = 0;
    while i < n {
        if let Some(ch) = items[i] {
            out.push(ch.name);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 三族入口
// ---------------------------------------------------------------------------

/// a 族：加载 + 量化。
pub fn run_vef19_checks_a() -> CheckSet {
    let mut s = CheckSet::new("vef19-load-quant");
    c1616_load_quant(&mut s);
    s
}

/// a 族独立入口（聚合器用）。
pub fn run_vef19_checks_a_standalone() -> CheckSet {
    run_vef19_checks_a()
}

/// b 族：简化 + 重排。
pub fn run_vef19_checks_b() -> CheckSet {
    let mut s = CheckSet::new("vef19-simp-reord");
    c1616_simplify_reorder(&mut s);
    s
}

/// b 族独立入口（聚合器用）。
pub fn run_vef19_checks_b_standalone() -> CheckSet {
    run_vef19_checks_b()
}

/// c 族：入册回归 + 判据集自检。
pub fn run_vef19_checks_c() -> CheckSet {
    let mut s = CheckSet::new("vef19-registry");
    c1616_registry(&mut s);
    meta_checks(&mut s);
    s
}

/// c 族独立入口（聚合器用）。
pub fn run_vef19_checks_c_standalone() -> CheckSet {
    run_vef19_checks_c()
}

/// 三族合并（单点调用）。
pub fn run_vef19_checks() -> CheckSet {
    CheckSet::merge(
        CheckSet::merge(run_vef19_checks_a(), run_vef19_checks_b()),
        run_vef19_checks_c(),
    )
}

/// 判据集条数（供文档/聚合器自检，不参与判定）。
pub const fn total_check_count() -> u32 {
    37
}
