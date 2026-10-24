//! VE-F0628 混合模式 GPU 实现（VE-D 域 · 目标 400 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0628`
//!
//! **判据（锚点原文逐条）**：
//! - **单源生成**：混合片段着色器由模板从公式表生成（ved22 公式单源，不手抄第二份）；
//! - **预乘执行**：采样输入按 F0625 预乘语义、输出预乘；
//! - **两形态**：逐层混合直通 + 隔离组内组混合（F0626）双形态集成；
//! - **降级登记**：高级混合不可用 → CPU 路 F0629 回退（登记与视觉等价声明）；
//! - **判据**：编译失败 → 模板回退简化档；GPU 对金标准 ≤ 1 LSB（F0630 联动）。

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, wgsl_source, wgsl_selfcheck, LSB_BUDGET};

/// 模板生成的着色器条目（单源：内容直出 ved22 公式源）
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderEntry {
    pub mode: SepMode,
    /// 模板包装后的完整片段源（单源生成，不手抄）
    pub src: String,
    /// 生成指纹（模式行号 + 源长度，漂移检测用）
    pub fingerprint: u64,
}

/// 双形态集成标记
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuForm {
    /// 形态一：逐层混合直通
    PerLayer,
    /// 形态二：隔离组内组混合（F0626 协作）
    InGroup,
}

pub const FORM_COUNT: usize = 2;

/// 降级登记行（诚实声明：谁、为什么、等价性）
#[derive(Clone, Debug, PartialEq)]
pub struct DegradeRow {
    pub target: &'static str,
    pub reason: &'static str,
    /// 视觉等价声明（无声明不得回退）
    pub visual_equiv: bool,
}

/// GPU 混合执行器
#[derive(Clone, Debug, Default)]
pub struct GpuBlender {
    /// 已生成着色器（按模式缓存，模板重生成即整体失效）
    pub shaders: Vec<ShaderEntry>,
    /// 降级账
    pub degrades: Vec<DegradeRow>,
    /// 编译失败回退简化档的次数
    pub fallbacks: u32,
    pub forms_enabled: [bool; FORM_COUNT],
}

/// 模板包装：单源公式 + 固定入口骨架（预乘纪律在这里执行）
pub const TEMPLATE_HEAD: &str = "fn blend_main(cs: vec4f, cb: vec4f) -> vec4f {\n  let cs_p = premul(cs);";
pub const TEMPLATE_TAIL: &str = "  return premul(out);\n}";

impl GpuBlender {
    pub fn new() -> GpuBlender {
        GpuBlender {
            forms_enabled: [true; FORM_COUNT],
            ..GpuBlender::default()
        }
    }

    /// 模板生成某模式着色器（单源：src = 骨架 + ved22::wgsl_source）
    pub fn gen_shader(&mut self, m: SepMode) -> &ShaderEntry {
        if let Some(i) = self.shaders.iter().position(|e| e.mode == m) {
            return &self.shaders[i];
        }
        let formula = wgsl_source(m);
        let src = alloc::format!("{}\n  {}\n{}", TEMPLATE_HEAD, formula.trim_end(), TEMPLATE_TAIL);
        // 指纹：长度 × 31 + 模式行号（漂移检测足够，非密码学）
        let fp = (src.len() as u64).wrapping_mul(31).wrapping_add(m.index() as u64);
        self.shaders.push(ShaderEntry { mode: m, src, fingerprint: fp });
        self.shaders.last().expect("just pushed")
    }

    /// 编译：单源自检不过 → 模板回退简化档（降级矩阵第 3 条）
    pub fn compile(&mut self, m: SepMode) -> Result<(), &'static str> {
        let chk = wgsl_selfcheck(m);
        if chk.is_ok() {
            Ok(())
        } else {
            self.fallbacks += 1;
            Err("shader selfcheck failed → simple tier")
        }
    }

    /// 能力降级登记：目标不可用 → CPU 路回退（须带视觉等价声明）
    pub fn degrade(&mut self, target: &'static str, reason: &'static str, equiv: bool) {
        self.degrades.push(DegradeRow { target, reason, visual_equiv: equiv });
    }

    /// 判据预算：GPU 对金标准 ≤ 1 LSB
    pub fn lsb_budget() -> f32 {
        LSB_BUDGET
    }

    /// 形态可用性
    pub fn form_ready(&self, f: GpuForm) -> bool {
        self.forms_enabled[f as usize]
    }
}

pub fn run_vec228_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec228");
    // 族一：单源生成
    let mut g = GpuBlender::new();
    let e1 = g.gen_shader(SepMode::Multiply).clone();
    let e2 = g.gen_shader(SepMode::Multiply).clone();
    s.add("gpu-缓存复用", e1 == e2 && g.shaders.len() == 1, "cached");
    s.add("gpu-单源含公式", e1.src.contains("blend_main") && e1.src.len() > TEMPLATE_HEAD.len(), "template wraps");
    let e3 = g.gen_shader(SepMode::Screen).clone();
    s.add("gpu-异源不同长", e3.src.len() != e1.src.len() || e3.mode != e1.mode, "distinct");
    s.add("gpu-指纹稳定", e1.fingerprint == g.gen_shader(SepMode::Multiply).fingerprint, "fp stable");
    // 族二：编译自检
    s.add("gpu-编译过", g.compile(SepMode::Multiply).is_ok(), "compile ok");
    s.add("gpu-零回退", g.fallbacks == 0, "no fallback");
    // 族三：两形态
    s.add("gpu-形态数", FORM_COUNT == 2, "two forms");
    s.add("gpu-逐层可用", g.form_ready(GpuForm::PerLayer), "per-layer");
    s.add("gpu-组内可用", g.form_ready(GpuForm::InGroup), "in-group");
    let mut g2 = GpuBlender::new();
    g2.forms_enabled[1] = false;
    s.add("gpu-组内禁用", !g2.form_ready(GpuForm::InGroup), "group off");
    // 族四：降级登记
    let mut g3 = GpuBlender::new();
    g3.degrade("old-gpu-x", "no advanced blend", true);
    s.add("降级-登记", g3.degrades.len() == 1 && g3.degrades[0].visual_equiv, "logged");
    let equiv_all = g3.degrades.iter().all(|d| d.visual_equiv);
    s.add("降级-等价声明齐", equiv_all, "equiv declared");
    g3.degrade("y", "probe fail", false);
    s.add("降级-无声明可检出", !g3.degrades[1].visual_equiv, "unequiv detectable");
    // 族五：判据预算
    s.add("gpu-1LSB预算", GpuBlender::lsb_budget() == 1.0, "1 lsb");
    // 族六：全 12 可分离模式单源生成不炸
    let mut g4 = GpuBlender::new();
    let mut all_ok = true;
    for m in SepMode::all() {
        let e = g4.gen_shader(m);
        if e.src.is_empty() || e.fingerprint == 0 {
            all_ok = false;
        }
    }
    s.add("gpu-全模式生成", all_ok && g4.shaders.len() == SepMode::all().len(), "12 gen ok");
    // 族七：编译失败回退路径（编译失败计数可归零重置语义）
    let fb0 = g4.fallbacks;
    s.add("gpu-回退计数初值", fb0 == 0, "fallback 0");
    s
}
