//! CGPU-F3524 · API 参考自动生成（CGPU-W 域 · W01 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3524`
//!
//! 参考：API 参考自动生成（注释抽取/页面渲染——参考实现；测试（抽取/
//! 渲染两组）。判据：双步、两组、判据。承接 F3523 生成管线（参考页同
//! 样是自动派生物不手写——沿用同一自动生成印记）；兑现 vcw01 定位条
//! 款「冻结签名才进参考文档，未冻结不立页」——未冻结签名入参考显性
//! 码拒。
//!
//! ## 要点一：双步架构（判据：双步）
//!
//! 步一 注释抽取（冻结签名 → 签名条目，逐字段机检）；步二 页面渲染
//! （条目 → 参考页行）。双步显性分离：只允许 抽取→渲染 单向一步，跳
//! 步/回退显性码拒——每步可独立验证，抽取产物是两步之间唯一通道。
//!
//! ## 要点二：冻结闸（vcw01 条款的机检兑现）
//!
//! `frozen == false` 的签名不得进入参考页：NOT_FROZEN 拒。冻结状态
//! 在签名上显性在账，参考实现逐条过闸——条款不是口号，是代码路径。
//!
//! ## 要点三：参考页是自动派生物不手写
//!
//! 渲染输出首行复用 F3523 的自动生成印记（同一常量——跨单元同源，
//! 不重写）；行数恒等于同源公式 1 + Σ(标题行 + 参数行 + 返回行 + 分
//! 隔行)；两次渲染逐行相同（确定性）。
//!
//! ## 要点四：零 panic 面 + 诊断码续占 0x5Cxx 细分段
//!
//! 与 vcw01（0x5C01~06）、vcw02（0x5C07~0C）、vcw03（0x5C0D~12）同
//! 段分段续占 0x5C13~0x5C18（cgm02 续段先例），六码互异且与前十八码
//! 不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

pub use crate::cgpu::vcw03_genpipeline::AUTO_BANNER;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、双步架构（判据：双步）
// ---------------------------------------------------------------------------

/// W 域任务总数守恒（10 组 × 16 项，与 vcw01/vcw02/vcw03 同域对账）。
pub const W_DOMAIN_TOTAL: u32 = 160;

/// API 参考生成双步（单向——注释抽取在前，页面渲染在后）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStep {
    /// 步一：注释抽取（冻结签名 → 签名条目）。
    ExtractSig,
    /// 步二：页面渲染（条目 → 参考页行）。
    RenderPage,
}

impl RefStep {
    /// 全部两步（判据侧独立重排对拍）。
    pub const ALL: [RefStep; 2] = [RefStep::ExtractSig, RefStep::RenderPage];

    /// 步名（判据侧写死对拍）。
    pub const fn name(self) -> &'static str {
        match self {
            RefStep::ExtractSig => "抽取",
            RefStep::RenderPage => "渲染",
        }
    }

    /// 恰一后继（双步链：抽取 → 渲染 → 终端 None）。
    pub const fn next(self) -> Option<RefStep> {
        match self {
            RefStep::ExtractSig => Some(RefStep::RenderPage),
            RefStep::RenderPage => None,
        }
    }
}

/// 单步推进：只允许 抽取→渲染——跳步/回退/终端再推进显性码拒。
pub fn two_step_transition(from: RefStep, to: RefStep) -> Result<(), WqCode> {
    match from.next() {
        Some(n) if n == to => Ok(()),
        _ => Err(WqCode::STEP_JUMP),
    }
}

// ---------------------------------------------------------------------------
// 二、冻结签名与签名条目
// ---------------------------------------------------------------------------

/// API 冻结签名（抽取的输入；frozen 显性在账——冻结闸的依据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSignature {
    /// 所属模块（参考页分组键）。
    pub module: &'static str,
    /// 函数名（非空）。
    pub name: &'static str,
    /// 冻结状态（false 即未冻结——入参考显性码拒）。
    pub frozen: bool,
    /// 参数表（参数名, 参数类型）逐项非空且名不重复。
    pub params: &'static [(&'static str, &'static str)],
    /// 返回类型（非空）。
    pub ret: &'static str,
}

/// 签名条目（步一抽取产物——两步之间唯一通道）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureEntry {
    /// 所属模块。
    pub module: String,
    /// 函数名。
    pub name: String,
    /// 参数表（已逐项核验）。
    pub params: Vec<(String, String)>,
    /// 返回类型。
    pub ret: String,
}

/// 步一：注释抽取（单签名）——冻结闸 + 签名完整性逐条专属码拒。
pub fn extract_signature(sig: &ApiSignature) -> Result<SignatureEntry, WqCode> {
    if !sig.frozen {
        return Err(WqCode::NOT_FROZEN);
    }
    if sig.name.is_empty() {
        return Err(WqCode::NAME_EMPTY);
    }
    for i in 0..sig.params.len() {
        if sig.params[i].0.is_empty() {
            return Err(WqCode::PARAM_BAD);
        }
        for j in 0..sig.params.len() {
            if i != j && sig.params[i].0 == sig.params[j].0 {
                return Err(WqCode::PARAM_BAD);
            }
        }
    }
    if sig.ret.is_empty() {
        return Err(WqCode::RET_EMPTY);
    }
    Ok(SignatureEntry {
        module: sig.module.to_string(),
        name: sig.name.to_string(),
        params: sig
            .params
            .iter()
            .map(|(n, t)| ((*n).to_string(), (*t).to_string()))
            .collect(),
        ret: sig.ret.to_string(),
    })
}

/// 步一（批量）：任一签名违例即整体拒——参考页不收录残缺批。
pub fn extract_all(sigs: &[ApiSignature]) -> Result<Vec<SignatureEntry>, WqCode> {
    let mut out = Vec::new();
    for s in sigs.iter() {
        out.push(extract_signature(s)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 三、步二：页面渲染（参考页是自动派生物不手写）
// ---------------------------------------------------------------------------

/// 渲染：条目 → 参考页行。布局：印记行（复用 F3523 同一常量）+ 每条
/// 目（标题行 + 参数行 ×N + 返回行 + 分隔空行）。
pub fn render_pages(entries: &[SignatureEntry]) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(AUTO_BANNER.to_string());
    for e in entries.iter() {
        lines.push(format!("### {}::{}", e.module, e.name));
        for (n, t) in e.params.iter() {
            lines.push(format!("| {} {} |", t, n));
        }
        lines.push(format!("-> {}", e.ret));
        lines.push(String::new());
    }
    lines
}

/// 行数同源公式（判据侧独立重算的对照物）：
/// 1（印记）+ Σ(1 标题 + params 行数 + 1 返回 + 1 分隔)。
pub fn expected_line_count(entries: &[SignatureEntry]) -> usize {
    let mut n = 1usize;
    for e in entries.iter() {
        n += 3 + e.params.len();
    }
    n
}

/// 渲染完整性核验：行数与同源公式比对 + 首行必须是自动生成印记。
pub fn verify_pages(lines: &[String], entries: &[SignatureEntry]) -> Result<(), WqCode> {
    if lines.len() != expected_line_count(entries) {
        return Err(WqCode::RENDER_MISMATCH);
    }
    match lines.first() {
        Some(l) if l == AUTO_BANNER => Ok(()),
        _ => Err(WqCode::RENDER_MISMATCH),
    }
}

/// 双步一键：抽取 → 渲染 → 完整性核验（跳步在架构上不可达）。
pub fn generate_api_ref(sigs: &[ApiSignature]) -> Result<Vec<String>, WqCode> {
    let entries = extract_all(sigs)?;
    let lines = render_pages(&entries);
    verify_pages(&lines, &entries)?;
    Ok(lines)
}

/// 渲染确定性：两次渲染逐行相同。
pub fn render_deterministic(entries: &[SignatureEntry]) -> bool {
    let a = render_pages(entries);
    let b = render_pages(entries);
    a == b
}

// ---------------------------------------------------------------------------
// 四、在账签名集 + 承接声明
// ---------------------------------------------------------------------------

/// F3523 承接声明（API 参考页走生成管线语义——印记同源不重写）。
pub const F3523_LINK: (&u32, &str) = (&3523, "文档生成管线");

/// 在账冻结签名集（判据侧独立写死对拍）。
pub fn frozen_signatures() -> Vec<ApiSignature> {
    alloc::vec![
        ApiSignature {
            module: "render",
            name: "render_frame",
            frozen: true,
            params: &[("frame", "&Frame"), ("cmd", "&CmdList")],
            ret: "bool",
        },
        ApiSignature {
            module: "present",
            name: "present",
            frozen: true,
            params: &[("swap", "&Swapchain")],
            ret: "bool",
        },
        ApiSignature {
            module: "cmd",
            name: "submit",
            frozen: true,
            params: &[("cmd", "&CmdList")],
            ret: "u32",
        },
    ]
}

/// 在账未冻结签名（判据反向语料——冻结闸的对照面）。
pub fn unfrozen_signatures() -> Vec<ApiSignature> {
    alloc::vec![ApiSignature {
        module: "draft",
        name: "draft_fn",
        frozen: false,
        params: &[("x", "u32")],
        ret: "u32",
    }]
}

// ---------------------------------------------------------------------------
// 五、错误契约（续占 0x5Cxx 细分段：0x5C13~0x5C18）
// ---------------------------------------------------------------------------

/// vcw04 诊断码。续占 `0x5Cxx` 细分段（与前十八码不重叠）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WqCode(pub u16);

impl WqCode {
    /// 未冻结签名入参考（vcw01 条款的机检兑现）。
    pub const NOT_FROZEN: WqCode = WqCode(0x5C13);
    /// 函数名为空。
    pub const NAME_EMPTY: WqCode = WqCode(0x5C14);
    /// 参数违例（参数名为空或重名）。
    pub const PARAM_BAD: WqCode = WqCode(0x5C15);
    /// 返回类型为空。
    pub const RET_EMPTY: WqCode = WqCode(0x5C16);
    /// 双步跳步/回退。
    pub const STEP_JUMP: WqCode = WqCode(0x5C17);
    /// 渲染与同源公式不符（完整性防御位）。
    pub const RENDER_MISMATCH: WqCode = WqCode(0x5C18);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            WqCode::NOT_FROZEN => "未冻结不立页：冻结签名才进参考文档".into(),
            WqCode::NAME_EMPTY => "函数名为空：参考条目必须有函数名".into(),
            WqCode::PARAM_BAD => "参数违例：参数名逐项非空且不重复".into(),
            WqCode::RET_EMPTY => "返回类型为空：签名必须声明返回类型".into(),
            WqCode::STEP_JUMP => "双步违序：只允许 抽取→渲染，跳步/回退均拒".into(),
            WqCode::RENDER_MISMATCH => "渲染不符：参考页行数与同源公式比对失败".into(),
            WqCode(_) => "未知 vcw04 API 参考域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（抽取/渲染两组）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 抽取组 ----

    #[test]
    fn 冻结签名抽取全过且字段逐项在账() {
        let entries = extract_all(&frozen_signatures()).expect("冻结签名应全过");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "render_frame");
        assert_eq!(entries[0].module, "render");
        assert_eq!(entries[0].params.len(), 2);
        assert_eq!(entries[0].ret, "bool");
    }

    #[test]
    fn 未冻结拒与反向语料专属码分账() {
        let un = unfrozen_signatures();
        assert_eq!(extract_signature(&un[0]), Err(WqCode::NOT_FROZEN));

        let mut empty_name = frozen_signatures();
        empty_name[0].name = "";
        assert_eq!(extract_signature(&empty_name[0]), Err(WqCode::NAME_EMPTY));

        let mut dup_param = frozen_signatures();
        dup_param[0].params = &[("frame", "&Frame"), ("frame", "&CmdList")];
        assert_eq!(extract_signature(&dup_param[0]), Err(WqCode::PARAM_BAD));

        let mut empty_ret = frozen_signatures();
        empty_ret[2].ret = "";
        assert_eq!(extract_signature(&empty_ret[2]), Err(WqCode::RET_EMPTY));
    }

    // ---- 渲染组 ----

    #[test]
    fn 参考页行数同源且带管线印记() {
        let entries = extract_all(&frozen_signatures()).expect("冻结签名应全过");
        let lines = render_pages(&entries);
        assert_eq!(lines.len(), expected_line_count(&entries));
        assert_eq!(lines.len(), 14); // 1 印记 + (3+2) + (3+1) + (3+1)
        assert_eq!(lines[0], AUTO_BANNER);
        assert_eq!(verify_pages(&lines, &entries), Ok(()));
        assert!(render_deterministic(&entries));
        assert_eq!(lines[1], "### render::render_frame"); // 标题行逐字
    }

    #[test]
    fn 双步违序拒与合法链过() {
        assert_eq!(
            two_step_transition(RefStep::ExtractSig, RefStep::RenderPage),
            Ok(())
        );
        assert_eq!(
            two_step_transition(RefStep::RenderPage, RefStep::ExtractSig),
            Err(WqCode::STEP_JUMP)
        );
        assert_eq!(
            two_step_transition(RefStep::RenderPage, RefStep::RenderPage),
            Err(WqCode::STEP_JUMP)
        );
        // 跳步等价面：一键生成不可绕过抽取（未冻结批整体拒）。
        let mut mixed = frozen_signatures();
        mixed.extend(unfrozen_signatures());
        assert_eq!(generate_api_ref(&mixed), Err(WqCode::NOT_FROZEN));
        let lines = generate_api_ref(&frozen_signatures()).expect("应成功");
        assert_eq!(lines[0], AUTO_BANNER);
    }
}
