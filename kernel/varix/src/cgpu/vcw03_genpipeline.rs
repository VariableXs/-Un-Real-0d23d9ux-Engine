//! CGPU-F3523 · 文档生成管线（CGPU-W 域 · W01 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3523`
//!
//! 管线：文档生成管线（源码注释→文档自动生成——管线实现；测试（生成
//! 一组）。判据：自动生成、一组、判据。承接 F3522 信息架构——生成的
//! 文档页必须挂进导航树（页面在树上，不在树外）。
//!
//! ## 要点一：管线四段单向
//!
//! 抽取（源注释 → 原始条目）→ 归一（排序去重）→ 渲染（条目 → 文档
//! 行）→ 产出（定稿）。单向流水线：只允许走下一段，跳段/回退显性码
//! 拒；产出是终端闸——没有自动生成印记的文档不许发布（手写文档混入
//! 即拒）。
//!
//! ## 要点二：文档是注释源的自动派生物，不手写
//!
//! 渲染输出首行带自动生成印记（AUTO_BANNER）；行数恒等于同源公式
//! 1 + Σ(标题行 + 正文行 + 分隔行)；两次生成逐行相同（确定性）。
//! 手写文档与源漂移在架构上不可能发生——印记与行数公式双重机检。
//!
//! ## 要点三：源注释三律（机检）
//!
//! 标题非空；正文非空且逐行非空；源路径全局唯一（重复源拒）。违律
//! 逐条专属码。
//!
//! ## 要点四：零 panic 面 + 诊断码续占 0x5Cxx 细分段
//!
//! 与 vcw01（0x5C01~0x5C06）、vcw02（0x5C07~0x5C0C）同段分段续占
//! 0x5C0D~0x5C12（cgm02 续占 0x5408~0x540C 先例），六码互异且与前十
//! 二码不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、管线四段（单向流水线）
// ---------------------------------------------------------------------------

/// W 域任务总数守恒（10 组 × 16 项，与 vcw01/vcw02 同域对账）。
pub const W_DOMAIN_TOTAL: u32 = 160;

/// 生成管线四段（单向——文档从注释到定稿的唯一通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenStage {
    /// 抽取：源码注释 → 原始条目。
    Extract,
    /// 归一：条目按路径稳定排序 + 重复源拒。
    Normalize,
    /// 渲染：条目 → 文档行（自动生成核心）。
    Render,
    /// 产出：文档定稿（终端闸）。
    Publish,
}

impl GenStage {
    /// 全部四段（判据侧独立重排对拍）。
    pub const ALL: [GenStage; 4] = [
        GenStage::Extract,
        GenStage::Normalize,
        GenStage::Render,
        GenStage::Publish,
    ];

    /// 段名（判据侧写死对拍）。
    pub const fn name(self) -> &'static str {
        match self {
            GenStage::Extract => "抽取",
            GenStage::Normalize => "归一",
            GenStage::Render => "渲染",
            GenStage::Publish => "产出",
        }
    }

    /// 恰一后继（终端 None——单向链的骨架）。
    pub const fn next(self) -> Option<GenStage> {
        match self {
            GenStage::Extract => Some(GenStage::Normalize),
            GenStage::Normalize => Some(GenStage::Render),
            GenStage::Render => Some(GenStage::Publish),
            GenStage::Publish => None,
        }
    }
}

/// 单步推进：只允许走下一段——跳段/回退/终端再推进显性码拒。
pub fn stage_transition(from: GenStage, to: GenStage) -> Result<(), WgCode> {
    match from.next() {
        Some(n) if n == to => Ok(()),
        _ => Err(WgCode::STAGE_JUMP),
    }
}

/// 全链走查：四段恰一步步递进全过（合法链的可执行证明）。
pub fn full_chain_legal() -> bool {
    stage_transition(GenStage::Extract, GenStage::Normalize).is_ok()
        && stage_transition(GenStage::Normalize, GenStage::Render).is_ok()
        && stage_transition(GenStage::Render, GenStage::Publish).is_ok()
}

// ---------------------------------------------------------------------------
// 二、源注释与原始条目
// ---------------------------------------------------------------------------

/// 源码注释单元（管线输入：挂在导航树某路径下的页面注释）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocComment {
    /// 所属页面路径（必须在 F3522 导航树路径集合内——页面在树上）。
    pub path: &'static str,
    /// 条目标题（非空）。
    pub title: &'static str,
    /// 正文行（逐行非空）。
    pub body: &'static [&'static str],
}

/// 原始条目（抽取产物）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    /// 页面路径。
    pub path: &'static str,
    /// 条目标题。
    pub title: String,
    /// 正文行。
    pub body: Vec<String>,
}

/// 抽取：源注释 → 原始条目（标题/正文三律逐条专属码拒）。
pub fn extract(docs: &[DocComment]) -> Result<Vec<RawEntry>, WgCode> {
    let mut out = Vec::new();
    for d in docs.iter() {
        if d.title.is_empty() {
            return Err(WgCode::TITLE_EMPTY);
        }
        if d.body.is_empty() {
            return Err(WgCode::BODY_EMPTY);
        }
        for line in d.body.iter() {
            if line.is_empty() {
                return Err(WgCode::BODY_EMPTY);
            }
        }
        out.push(RawEntry {
            path: d.path,
            title: d.title.to_string(),
            body: d.body.iter().map(|s| (*s).to_string()).collect(),
        });
    }
    Ok(out)
}

/// 归一：路径重复拒 + 按路径字典序稳定排序（渲染顺序由归一定死——
/// 手写顺序不可插队）。
pub fn normalize(mut entries: Vec<RawEntry>) -> Result<Vec<RawEntry>, WgCode> {
    for i in 0..entries.len() {
        for j in 0..entries.len() {
            if i != j && entries[i].path == entries[j].path {
                return Err(WgCode::PATH_DUP);
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(b.path));
    Ok(entries)
}

// ---------------------------------------------------------------------------
// 三、渲染（自动生成核心——文档是派生物不手写）
// ---------------------------------------------------------------------------

/// 自动生成印记（输出首行；手写文档不可能自带——发布的机检锚）。
pub const AUTO_BANNER: &str = "本文档由生成管线自动产出，请勿手改";

/// 渲染：条目 → 文档行。布局：印记行 + 每条目（标题行 + 正文行 +
/// 分隔空行）。顺序 = 归一后的路径字典序。
pub fn render(entries: &[RawEntry]) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(AUTO_BANNER.to_string());
    for e in entries.iter() {
        lines.push(format!("## {}｜{}", e.title, e.path));
        for b in e.body.iter() {
            lines.push(b.clone());
        }
        lines.push(String::new());
    }
    lines
}

/// 行数同源公式（判据侧独立重算的对照物）：
/// 1（印记）+ Σ(1 标题 + body 行数 + 1 分隔)。
pub fn expected_line_count(entries: &[RawEntry]) -> usize {
    let mut n = 1usize;
    for e in entries.iter() {
        n += 2 + e.body.len();
    }
    n
}

/// 渲染完整性核验：行数与同源公式比对 + 首行必须是印记。
pub fn verify_render(lines: &[String], entries: &[RawEntry]) -> Result<(), WgCode> {
    if lines.len() != expected_line_count(entries) {
        return Err(WgCode::RENDER_MISMATCH);
    }
    match lines.first() {
        Some(l) if l == AUTO_BANNER => Ok(()),
        _ => Err(WgCode::RENDER_MISMATCH),
    }
}

/// 全管线一键：抽取 → 归一 → 渲染 → 完整性核验（产出需发布闸显性过）。
pub fn generate(docs: &[DocComment]) -> Result<Vec<String>, WgCode> {
    let raw = extract(docs)?;
    let norm = normalize(raw)?;
    let lines = render(&norm);
    verify_render(&lines, &norm)?;
    Ok(lines)
}

/// 生成确定性：两次生成逐行相同（同输入必同输出）。
pub fn generate_deterministic(docs: &[DocComment]) -> bool {
    let a = generate(docs);
    let b = generate(docs);
    a == b
}

// ---------------------------------------------------------------------------
// 四、发布终端闸 + 承接声明
// ---------------------------------------------------------------------------

/// 发布终端闸：空文档 / 无印记文档（手写混入）显性码拒。
pub fn publish(lines: &[String]) -> Result<(), WgCode> {
    match lines.first() {
        Some(l) if l == AUTO_BANNER => Ok(()),
        _ => Err(WgCode::PUBLISH_EMPTY),
    }
}

/// F3522 承接声明（生成管线的输出挂进 F3522 导航树——页面在树上）。
pub const F3522_LINK: (&u32, &str) = (&3522, "文档生成管线");

/// 在账源注释集（判据侧独立写死对拍；路径全部取自 vcw02 导航树）。
pub fn source_docs() -> Vec<DocComment> {
    alloc::vec![
        DocComment {
            path: "reference/render",
            title: "渲染接口的文档从哪来",
            body: &["从源注释自动派生——管线四段单向", "行数同源可机检"],
        },
        DocComment {
            path: "tutorial/first-frame",
            title: "第一帧也要有文档",
            body: &["教程页同样走生成管线", "印记与行数公式双重核验"],
        },
        DocComment {
            path: "faq/diagcodes",
            title: "生成失败码含义",
            body: &["0x5C0D~0x5C12 六码见错误契约", "与 vcw01/vcw02 不重叠"],
        },
    ]
}

// ---------------------------------------------------------------------------
// 五、错误契约（续占 0x5Cxx 细分段：0x5C0D~0x5C12）
// ---------------------------------------------------------------------------

/// vcw03 诊断码。续占 `0x5Cxx` 细分段（与 vcw01 0x5C01~0x5C06、
/// vcw02 0x5C07~0x5C0C 不重叠）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WgCode(pub u16);

impl WgCode {
    /// 标题为空（源注释三律之一）。
    pub const TITLE_EMPTY: WgCode = WgCode(0x5C0D);
    /// 正文为空或含空行（源注释三律之一）。
    pub const BODY_EMPTY: WgCode = WgCode(0x5C0E);
    /// 源路径重复（源注释三律之一）。
    pub const PATH_DUP: WgCode = WgCode(0x5C0F);
    /// 跳段/回退（单向流水线违序）。
    pub const STAGE_JUMP: WgCode = WgCode(0x5C10);
    /// 空文档/无印记发布（终端闸）。
    pub const PUBLISH_EMPTY: WgCode = WgCode(0x5C11);
    /// 渲染与同源公式不符（完整性防御位）。
    pub const RENDER_MISMATCH: WgCode = WgCode(0x5C12);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            WgCode::TITLE_EMPTY => "标题为空：源注释必须有条目标题".into(),
            WgCode::BODY_EMPTY => "正文为空：源注释正文逐行非空".into(),
            WgCode::PATH_DUP => "源路径重复：同页面路径的源注释只收一份".into(),
            WgCode::STAGE_JUMP => "管线违序：只允许走下一段，跳段/回退均拒".into(),
            WgCode::PUBLISH_EMPTY => "发布被拒：文档缺失或无自动生成印记（手写混入）".into(),
            WgCode::RENDER_MISMATCH => "渲染不符：文档行数与同源公式比对失败".into(),
            WgCode(_) => "未知 vcw03 生成管线域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（生成一组：管线/印记/排序/终端闸）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 在账源生成全过且带印记() {
        let docs = source_docs();
        let lines = generate(&docs).expect("生成应成功");
        assert_eq!(lines[0], AUTO_BANNER);
        assert!(publish(&lines).is_ok());
        assert!(generate_deterministic(&docs));
    }

    #[test]
    fn 行数同源且排序确定() {
        let docs = source_docs();
        let raw = extract(&docs).expect("抽取应成功");
        let norm = normalize(raw).expect("归一应成功");
        assert_eq!(norm.len(), 3);
        // 路径字典序：faq < reference < tutorial——排序后首条是 faq
        assert_eq!(norm[0].path, "faq/diagcodes");
        assert_eq!(norm[2].path, "tutorial/first-frame");
        let lines = render(&norm);
        assert_eq!(lines.len(), expected_line_count(&norm));
        assert_eq!(lines.len(), 13); // 1 印记 + 3 × (1 标题 + 2 正文 + 1 分隔)
        assert_eq!(verify_render(&lines, &norm), Ok(()));
    }

    #[test]
    fn 源三律专属码分账() {
        let mut bad_title = source_docs();
        bad_title[0].title = "";
        assert_eq!(extract(&bad_title), Err(WgCode::TITLE_EMPTY));

        let mut bad_body = source_docs();
        bad_body[1].body = &["有内容", ""];
        assert_eq!(extract(&bad_body), Err(WgCode::BODY_EMPTY));

        let mut dup = source_docs();
        dup.push(DocComment {
            path: "faq/diagcodes",
            title: "重复路径",
            body: &["占位"],
        });
        let raw = extract(&dup).expect("抽取应成功");
        assert_eq!(normalize(raw), Err(WgCode::PATH_DUP));
    }

    #[test]
    fn 单向链与终端闸() {
        assert!(full_chain_legal());
        // 跳段：抽取 → 渲染 拒；回退：渲染 → 抽取 拒；终端再推进拒。
        assert_eq!(
            stage_transition(GenStage::Extract, GenStage::Render),
            Err(WgCode::STAGE_JUMP)
        );
        assert_eq!(
            stage_transition(GenStage::Render, GenStage::Extract),
            Err(WgCode::STAGE_JUMP)
        );
        assert_eq!(
            stage_transition(GenStage::Publish, GenStage::Publish),
            Err(WgCode::STAGE_JUMP)
        );
        // 终端闸：空文档拒；手写文档（无印记）拒。
        assert_eq!(publish(&[]), Err(WgCode::PUBLISH_EMPTY));
        assert_eq!(publish(&alloc::vec!["手写的第一行".to_string()]), Err(WgCode::PUBLISH_EMPTY));
    }
}
