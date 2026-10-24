//! cgb24_xfdoc — CGPU-F0237 跨帧文档（B05 组第十三件）。
//!
//! 判据原文（施工书锚点 CGPU-F0237）：跨帧文档：超图模型白皮书/别名分析
//! 算法说明/回压参数调优指南/API 手册——含每个参数默认值的依据说明。
//! 判据：四文档、参数依据。
//!
//! 职责定位：文档不是代码的附庸，是 API 的第二交付面——本模块把 B05 组
//! 的四份核心文档做成机器可审计的注册件：①文档本体入账（节结构 + 正文
//! 摘要 + 指纹，防漂移）；②参数依据表（每个默认值的来源、依据、出处模
//! 块——"含每个参数默认值的依据说明"的逐字兑现）；③文档↔代码一致性
//! 对拍（文档里写的深度硬顶/滞回窗/造环概率必须与 cgb17/cgb22 的真实
//! 常量逐位同——文档说一套代码做一套是质量事故）；④文档完整性自证
//! （四文档齐全、每份节非空、参数表无孤儿参数）。
//! 诊断码段 0x24B0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点；正文为冻结文本
//! （指纹锁定），改文档必须换指纹版本——与 cgb23 API 同流程。

use alloc::vec;
use crate::cgpu::cgb17_xfbackpress::{ADAPT_HYSTERESIS, DEPTH_FLOOR, DEPTH_HARD_CAP};
use crate::cgpu::cgb22_xffuzz::{CASES_PER_SEED, FORCED_CYCLE_PPM, STEP_BUDGET};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_DOC_VERSION: &str = "B24-xfdoc-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x24B 段，本会话独占）
// ---------------------------------------------------------------------------

/// 必备文档缺失（四文档不齐——完整性不过）。
pub const XC_DOC_MISSING: u16 = 0x24B0;
/// 文档节空（节结构存在但正文为空——空壳文档）。
pub const XC_DOC_EMPTY_SECTION: u16 = 0x24B1;
/// 文档指纹漂移（正文与注册指纹不符——未走版本流程的私改）。
pub const XC_DOC_DRIFT: u16 = 0x24B2;
/// 参数依据缺失（参数表里有参数没依据——判据二的直接违反）。
pub const XC_DOC_PARAM_NO_RATIONALE: u16 = 0x24B3;
/// 文档↔代码失配（文档写的常量与代码实态不同——文档说谎）。
pub const XC_DOC_CODE_MISMATCH: u16 = 0x24B4;
/// 参数孤儿（参数表条目在代码里找不到对应常量——文档超前于代码）。
pub const XC_DOC_PARAM_ORPHAN: u16 = 0x24B5;

// ---------------------------------------------------------------------------
// 参数依据表（判据二：每个默认值的依据说明）
// ---------------------------------------------------------------------------

/// 一条参数依据。
#[derive(Clone, Debug)]
pub struct ParamRationale {
    /// 参数名（跨模块唯一）。
    pub name: &'static str,
    /// 默认值（文档面写定值）。
    pub doc_value: u32,
    /// 依据说明（为什么是这个值——一句话）。
    pub rationale: &'static str,
    /// 出处模块（代码侧真源）。
    pub source: &'static str,
}

/// B05 组对外参数的依据总表。
/// doc_value 列与代码常量的一致性由 code_consistency_audit 逐位对拍。
pub const PARAM_TABLE: [ParamRationale; 6] = [
    ParamRationale {
        name: "DEPTH_HARD_CAP",
        doc_value: 8,
        rationale: "引擎在途窗口上限：B05 组跨帧深度声明校验的窗口边界，超窗即拒（cgb12 同窗口）。",
        source: "cgb17_xfbackpress",
    },
    ParamRationale {
        name: "DEPTH_FLOOR",
        doc_value: 1,
        rationale: "深度下限：低于 1 帧在途等于串行提交，延迟收益为负——纯开销，不设 0。",
        source: "cgb17_xfbackpress",
    },
    ParamRationale {
        name: "ADAPT_HYSTERESIS",
        doc_value: 32,
        rationale: "调深滞回窗：两次调深至少隔 32 帧，防帧耗时抖动引发深度振荡（参数振荡=计划性卡顿）。",
        source: "cgb17_xfbackpress",
    },
    ParamRationale {
        name: "STEP_BUDGET",
        doc_value: 4096,
        rationale: "fuzz 步数预算：4K 步覆盖 16 帧×2 边/帧×128 检测深度的上界组合，超顶即判悬挂。",
        source: "cgb22_xffuzz",
    },
    ParamRationale {
        name: "CASES_PER_SEED",
        doc_value: 8,
        rationale: "每种子用例数：8 用例 × 每夜 3 种子 = 24 例，覆盖环/非环/混合三类形态的最小充分样本。",
        source: "cgb22_xffuzz",
    },
    ParamRationale {
        name: "FORCED_CYCLE_PPM",
        doc_value: 2500,
        rationale: "故意造环概率 25%：环检拦截路径是 fuzz 的核心回归面，1/4 配比保证每夜必触且不淹没非环路径。",
        source: "cgb22_xffuzz",
    },
];

// ---------------------------------------------------------------------------
// 四文档注册件
// ---------------------------------------------------------------------------

/// 一份文档的注册条目。
#[derive(Clone, Debug)]
pub struct DocEntry {
    /// 文档名。
    pub name: &'static str,
    /// 节标题序列（结构面）。
    pub sections: Vec<&'static str>,
    /// 各节正文（与 sections 等长——结构账平的前提）。
    pub bodies: Vec<String>,
    /// 指纹（节序+正文 FNV——防漂移）。
    pub fingerprint: u64,
}

/// 文档指纹：节序与正文逐字节 FNV-1a。
fn doc_fingerprint(name: &str, sections: &[&str], bodies: &[String]) -> u64 {
    let mut h: u64 = 0x24B0_D0C5_1022_0001;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    for (s, body) in sections.iter().zip(bodies.iter()) {
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        for b in body.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

/// 四文档的 v1 注册正本（正文为冻结文本）。
pub fn b05_docs_v1() -> Result<Vec<DocEntry>, u16> {
    let mk = |name: &'static str, sections: Vec<&'static str>, bodies: Vec<String>| -> Result<DocEntry, u16> {
        if sections.len() != bodies.len() {
            return Err(XC_DOC_EMPTY_SECTION);
        }
        let fp = doc_fingerprint(name, &sections, &bodies);
        Ok(DocEntry { name, sections, bodies, fingerprint: fp })
    };
    let docs = vec![
        mk(
            "跨帧超图模型白皮书",
            vec!["模型定位", "历史帧引用边", "生命周期账", "深度声明", "不变量"],
            vec![
                String::from("跨帧超图是 B01 帧内依赖图的时间维扩展：节点仍为帧任务，边增加时间跨度 k（本帧任务依赖 k 帧前的产出）。超图与帧内图同构，检测核复用。"),
                String::from("历史帧引用边记三元组 (waiter, producer, k)：waiter 的提交被 producer 的产出挡住。k≥1 保证帧内依赖不混入本图（帧内归 B02）。"),
                String::from("历史缓冲按引用计数存活：帧结束只减本帧引用，计数归零且越过释放纪元才入回收队——帧边界不误回收在用缓冲。"),
                String::from("任务申报跨帧深度 k，校验不超引擎窗口（DEPTH_HARD_CAP=8）：窗口外历史产出按定义不可见，申报即拒。"),
                String::from("两条模型不变量：跨帧边必须指向已提交帧；逻辑环必须可静态判定（cgb18 在线检）。违反任一即拦截，不进入执行。"),
            ],
        )?,
        mk(
            "别名分析算法说明",
            vec!["问题定义", "别名推导", "传播规则", "保守策略"],
            vec![
                String::from("判定帧 N 的写目标与帧 N-1 的读源是否同一物理缓冲：ping-pong/环形池的槽位换算 + 池回收重命名传播。"),
                String::from("别名推导两步：槽位映射（帧号模池深）+ 重命名链追溯（池回收后新名指向旧名，别名边随之迁移）。"),
                String::from("别名图与依赖图联动传播：别名边确认后并入 B02 依赖视图——帧内依赖+跨帧别名=完整依赖。"),
                String::from("保守策略：不确定即视为别名。误别名导致数据竞争，假别名只损失并行度——两害取其轻，宁慢不错。"),
            ],
        )?,
        mk(
            "回压参数调优指南",
            vec!["三段决策链", "深度旋钮", "滞回窗", "跳帧代价", "自适应口径"],
            vec![
                String::from("回压三段链：深度上限闸（计数硬拦）→跳帧筛（代价升序牺牲非关键帧）→自适应调深（帧耗时统计驱动）。三段各司其职，不互相越权。"),
                String::from("在途深度 1..=8：交互敏感场景取 2-3（低延迟）；吞吐场景取 6-8（高并行）。默认 4——延迟与吞吐的中间档。"),
                String::from("滞回窗 32 帧：两次调深最小间隔。调参依据：帧耗时统计窗（64 样本）的收敛时间约为窗半，32 帧间隔保证统计有效再动作。"),
                String::from("跳帧代价：关键帧 10000 / 普通 1000 / 后台 100（万分制）。关键帧永不被丢；筛序 = 代价升序、同代价老帧先丢。"),
                String::from("自适应口径：帧均耗时 > 目标×9/10 加深（攒吞吐），< 目标×6/10 减浅（省延迟），带内不动。全整数交叉乘法，无除法无浮点。"),
            ],
        )?,
        mk(
            "跨帧 API 手册",
            vec!["三族总览", "声明族", "控制族", "查询族", "冻结纪律"],
            vec![
                String::from("三族七函数：声明族 2（深度申报/历史注册）、控制族 2（在途深度/回压策略）、查询族 3（别名/等待/账目）。指纹冻结见 cgb23。"),
                String::from("xf_declare_depth(frame, k)：任务申报跨帧深度；xf_register_history(res, epoch)：历史缓冲引用注册。超窗即拒——错误码查 cgb12。"),
                String::from("xf_set_inflight(n)：设深度上限 1..=8；xf_set_backpressure(on)：回压开关（含跳帧筛与自适应）。"),
                String::from("xf_query_alias(...)：别名判定（保守语义）；xf_query_wait(frame)：等待状态；xf_query_ledger(...)：资源账目（12 章透明承诺的兑现面）。"),
                String::from("冻结纪律与 B04 同流程：注册即冻结、变更走 evolve+ADR、版本只前进。指纹核对入口 ApiFreezeLedger::verify。"),
            ],
        )?,
    ];
    Ok(docs)
}

// ---------------------------------------------------------------------------
// 审计核：四文档、参数依据、文档↔代码对拍
// ---------------------------------------------------------------------------

/// 文档完整性审计（判据一：四文档齐、节非空、结构账平）。
pub fn completeness_audit(docs: &[DocEntry]) -> Result<(), u16> {
    const REQUIRED: [&str; 4] = ["跨帧超图模型白皮书", "别名分析算法说明", "回压参数调优指南", "跨帧 API 手册"];
    for req in REQUIRED.iter() {
        match docs.iter().find(|d| d.name == *req) {
            None => return Err(XC_DOC_MISSING),
            Some(d) => {
                if d.sections.is_empty() {
                    return Err(XC_DOC_MISSING);
                }
                if d.sections.len() != d.bodies.len() {
                    return Err(XC_DOC_EMPTY_SECTION);
                }
                for body in &d.bodies {
                    if body.is_empty() {
                        return Err(XC_DOC_EMPTY_SECTION);
                    }
                }
                // 指纹复核（防注册后被改写）。
                if doc_fingerprint(d.name, &d.sections, &d.bodies) != d.fingerprint {
                    return Err(XC_DOC_DRIFT);
                }
            }
        }
    }
    Ok(())
}

/// 参数依据审计（判据二：每参数有依据 + 出处模块）。
pub fn rationale_audit() -> Result<(), u16> {
    for p in PARAM_TABLE.iter() {
        if p.rationale.is_empty() || p.source.is_empty() {
            return Err(XC_DOC_PARAM_NO_RATIONALE);
        }
        // 依据必须超过一句话的下限（空话防线：至少 20 字节实质内容）。
        if p.rationale.len() < 20 {
            return Err(XC_DOC_PARAM_NO_RATIONALE);
        }
    }
    Ok(())
}

/// 文档↔代码一致性对拍（判据二的兑现面：文档写的值 = 代码实态）。
pub fn code_consistency_audit() -> Result<(), u16> {
    for p in PARAM_TABLE.iter() {
        let actual: u32 = match p.name {
            "DEPTH_HARD_CAP" => DEPTH_HARD_CAP,
            "DEPTH_FLOOR" => DEPTH_FLOOR,
            "ADAPT_HYSTERESIS" => ADAPT_HYSTERESIS,
            "STEP_BUDGET" => STEP_BUDGET as u32,
            "CASES_PER_SEED" => CASES_PER_SEED,
            "FORCED_CYCLE_PPM" => FORCED_CYCLE_PPM,
            _ => return Err(XC_DOC_PARAM_ORPHAN),
        };
        if actual != p.doc_value {
            return Err(XC_DOC_CODE_MISMATCH);
        }
        // 出处模块名必须是 B05 组真实模块（出处真实性）。
        let src_ok = p.source.starts_with("cgb17") || p.source.starts_with("cgb22");
        if !src_ok {
            return Err(XC_DOC_PARAM_ORPHAN);
        }
    }
    Ok(())
}

/// 文档指纹总账（四文档一次指纹——CI 对拍入口）。
pub fn docs_root_fingerprint(docs: &[DocEntry]) -> u64 {
    let mut h: u64 = 0x24B0_5007_1022_0001;
    for d in docs {
        h ^= d.fingerprint;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断说明。
pub fn detail(code: u16) -> String {
    match code {
        XC_DOC_MISSING => "必备文档缺失：四文档（白皮书/别名/回压/API）不齐不许宣告交付。".to_string(),
        XC_DOC_EMPTY_SECTION => "文档节空或结构不平：节标题与正文等长且非空。".to_string(),
        XC_DOC_DRIFT => "文档指纹漂移：改文档必须换指纹版本（与 API 同流程），私改即漂移。".to_string(),
        XC_DOC_PARAM_NO_RATIONALE => "参数无依据：每个默认值必须写清为什么是这个值（≥20 字节实质内容）。".to_string(),
        XC_DOC_CODE_MISMATCH => "文档↔代码失配：文档值与代码常量不同——先改代码或走文档版本流程，不许两套真相。".to_string(),
        XC_DOC_PARAM_ORPHAN => "参数孤儿：参数表条目在代码里找不到对应常量——文档不许超前于代码。".to_string(),
        _ => "未知诊断码：查 0x24B 段分配表。".to_string(),
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：四文档、参数依据的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    let docs = b05_docs_v1()?;

    // 一、四文档齐 + 节非空 + 指纹自洽。
    completeness_audit(&docs)?;
    if docs.len() != 4 {
        return Err(XC_DOC_MISSING);
    }
    // 白皮书 5 节 / 别名 4 节 / 回压 5 节 / API 5 节。
    if docs[0].sections.len() != 5 || docs[1].sections.len() != 4 || docs[2].sections.len() != 5 || docs[3].sections.len() != 5 {
        return Err(XC_DOC_EMPTY_SECTION);
    }
    // 根指纹稳定。
    let f1 = docs_root_fingerprint(&docs);
    let docs2 = b05_docs_v1()?;
    if f1 != docs_root_fingerprint(&docs2) {
        return Err(XC_DOC_DRIFT);
    }

    // 二、参数依据：六参数全有依据 + 出处。
    rationale_audit()?;
    if PARAM_TABLE.len() != 6 {
        return Err(XC_DOC_PARAM_NO_RATIONALE);
    }

    // 三、文档↔代码实态对拍（真源在 cgb17/cgb22）。
    code_consistency_audit()?;

    // 四、漂移防线：篡改一节正文 → 指纹必变。
    let mut tampered = docs[2].clone();
    let new_body = String::from("被篡改的正文");
    tampered.bodies[0] = new_body;
    tampered.fingerprint = doc_fingerprint(tampered.name, &tampered.sections, &tampered.bodies);
    if tampered.fingerprint == docs[2].fingerprint {
        return Err(XC_DOC_DRIFT);
    }
    // 篡改件过不了完整性（指纹是注册时算定的，正文换指纹必须整件重注册）。
    let mut forged = docs.clone();
    forged[2] = tampered;
    // 注意：forged[2].fingerprint 已重算，completeness 会过——所以漂移
    // 检测的正确姿势是与注册正本的根指纹比对（而非只自算自证）：
    if docs_root_fingerprint(&forged) == f1 {
        return Err(XC_DOC_DRIFT);
    }
    Ok(())
}
