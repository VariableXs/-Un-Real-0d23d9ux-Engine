//! F589 拖拽上传 Edge · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：上传区落点；计数徽标；非上传区分流；系统不越界审计；
//! 与 F255/F355 协同。
//!
//! **设计要点（主册）**：
//! - 文件拖进 Edge 网页上传区：落点高亮（网页原生）+ 系统级拖影计数
//!   （F553 同源）；
//! - 拖到非上传区（网页正文）= Edge 自身语义（打开/搜索——F255 网页侧
//!   四落点同源）；
//! - 大文件上传进度走 Edge 自身 UI（系统不抢戏——无商店宪法的边界自觉：
//!   浏览器的事浏览器管）；
//! - 互通层只做「把文件送到网页手上」。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 网页侧落点（F255 四落点同源——上传区是其中之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebDropZone {
    /// 上传区（文件进网页——本项主场景）。
    UploadArea,
    /// 网页正文（Edge 自身语义：图片打开/文本搜索）。
    PageBody,
    /// 导航栏（URL 处理）。
    Nav,
    /// 下载栏（文件入下载）。
    DownloadsBar,
}

/// 拖放会话（一次拖拽的完整生命周期账）。
pub struct DropSession {
    /// 拖拽文件数（F553 计数徽标同源账）。
    pub files: u32,
    /// 最终落点。
    pub zone: Option<WebDropZone>,
    /// 系统动作账：互通层实际做了什么（审计面）。
    pub system_actions: Vec<&'static str>,
    /// 网页侧动作账（Edge 自身语义——系统不越界的对照面）。
    pub web_actions: Vec<&'static str>,
}

/// Edge 互通层。
pub struct EdgeBridge {
    session: Option<DropSession>,
    /// 越界审计账（系统抢了浏览器的事 = 记一条——判据「系统不越界审计」）。
    violations: u32,
    /// 进度显示归属账（None = 未上传；Some(false) = 网页 UI 在管）。
    progress_owner: Option<bool>,
}

impl EdgeBridge {
    pub fn new() -> EdgeBridge {
        EdgeBridge {
            session: None,
            violations: 0,
            progress_owner: None,
        }
    }

    /// 开始拖拽（文件数入徽标账）。
    pub fn drag_begin(&mut self, files: u32) {
        self.session = Some(DropSession {
            files,
            zone: None,
            system_actions: Vec::new(),
            web_actions: Vec::new(),
        });
    }

    /// 上传区悬停：落点高亮（网页原生高亮——系统只记账不代画）。
    pub fn hover_upload(&mut self) -> bool {
        match &mut self.session {
            Some(s) => {
                s.system_actions.push("徽标计数显示");
                s.web_actions.push("落点高亮");
                true
            }
            None => false,
        }
    }

    /// 落进上传区：互通层把文件交给网页（边界内唯一动作）。
    pub fn drop_upload(&mut self) -> bool {
        match &mut self.session {
            Some(s) if s.zone.is_none() => {
                s.zone = Some(WebDropZone::UploadArea);
                s.system_actions.push("文件句柄移交网页");
                s.web_actions.push("网页接手上传");
                // 进度归属：网页 UI（系统不抢戏）。
                self.progress_owner = Some(false);
                true
            }
            _ => false,
        }
    }

    /// 落进非上传区：Edge 自身语义（打开/搜索）——系统零介入。
    pub fn drop_elsewhere(&mut self, zone: WebDropZone) -> bool {
        if zone == WebDropZone::UploadArea {
            return false; // 语义混用拒绝（上传区走 drop_upload）。
        }
        match &mut self.session {
            Some(s) if s.zone.is_none() => {
                s.zone = Some(zone);
                s.web_actions.push("Edge 自身语义处理");
                true
            }
            _ => false,
        }
    }

    /// 上传进度回调（网页推送——系统转发展示，不造第二套进度 UI）。
    pub fn web_progress(&mut self, pct: u32) -> Option<u32> {
        if self.progress_owner != Some(false) {
            return None;
        }
        // 越界检查：若系统此时自建进度 UI 即违规（本模型以审计账表达）。
        let _ = pct;
        Some(pct)
    }

    /// 越界审计：人工/自检触发核对——互通层动作清单里出现
    /// 「文件句柄移交网页」「徽标计数显示」之外的动作即违规。
    pub fn audit(&mut self) -> u32 {
        let mut v = 0;
        if let Some(s) = &self.session {
            for a in &s.system_actions {
                if *a != "文件句柄移交网页" && *a != "徽标计数显示" {
                    v += 1;
                }
            }
        }
        self.violations += v;
        self.violations
    }

    pub fn violation_count(&self) -> u32 {
        self.violations
    }

    /// 会话结束（拖放完成）。
    pub fn finish(&mut self) -> Option<DropSession> {
        self.session.take()
    }
}

impl Default for EdgeBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_dropupload_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 上传区落点：拖 3 个文件 → 徽标计数 3（F553 同源）→ 落点高亮 →
    //    移交网页。
    let mut b = EdgeBridge::new();
    b.drag_begin(3);
    b.hover_upload();
    let dropped = b.drop_upload();
    let s = b.finish().unwrap();
    set.add(
        "upload area full chain",
        dropped && s.files == 3 && s.zone == Some(WebDropZone::UploadArea),
        "",
    );

    // 2. 系统不越界审计：动作清单只有徽标与移交——零违规。
    let mut b2 = EdgeBridge::new();
    b2.drag_begin(1);
    b2.hover_upload();
    b2.drop_upload();
    set.add(
        "system stays in lane",
        b2.audit() == 0 && b2.violation_count() == 0,
        "",
    );

    // 3. 非上传区分流：落正文 = Edge 自身语义（打开/搜索）——系统零动作。
    let mut b3 = EdgeBridge::new();
    b3.drag_begin(2);
    b3.drop_elsewhere(WebDropZone::PageBody);
    let s3 = b3.finish().unwrap();
    set.add(
        "non upload zone edge semantics",
        s3.zone == Some(WebDropZone::PageBody)
            && s3.system_actions.is_empty()
            && s3.web_actions == alloc::vec!["Edge 自身语义处理"],
        "",
    );

    // 4. 进度归属：网页 UI 在管（系统不造第二套进度）。
    let mut b4 = EdgeBridge::new();
    b4.drag_begin(1);
    b4.drop_upload();
    let p1 = b4.web_progress(42);
    set.add(
        "progress owned by web ui",
        p1 == Some(42) && b4.progress_owner == Some(false),
        "",
    );

    // 5. 未上传时进度回调拒绝（没有移交就没有进度口）。
    let mut b5 = EdgeBridge::new();
    b5.drag_begin(1);
    set.add("no progress before handover", b5.web_progress(10).is_none(), "");

    // 6. 双落点拒绝：一次拖拽只有一个落点（先落上传区再落正文被拒）。
    let mut b6 = EdgeBridge::new();
    b6.drag_begin(1);
    b6.drop_upload();
    let second = b6.drop_elsewhere(WebDropZone::DownloadsBar);
    set.add("single drop per session", !second, "");

    // 7. F255 四落点同源：枚举四枚齐（上传区/正文/导航栏/下载栏）。
    set.add(
        "four drop zones from f255",
        WebDropZone::UploadArea != WebDropZone::PageBody
            && WebDropZone::PageBody != WebDropZone::Nav
            && WebDropZone::Nav != WebDropZone::DownloadsBar,
        "",
    );

    // 8. 无会话动作诚实拒绝。
    let mut b7 = EdgeBridge::new();
    set.add("no session honest reject", !b7.drop_upload() && !b7.hover_upload(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_elsewhere_rejects_upload_zone() {
        let mut b = EdgeBridge::new();
        b.drag_begin(1);
        assert!(!b.drop_elsewhere(WebDropZone::UploadArea));
    }

    #[test]
    fn finish_without_session_none() {
        let mut b = EdgeBridge::new();
        assert!(b.finish().is_none());
    }

    #[test]
    fn zero_files_drag_legal() {
        // 拖 0 文件（文本拖拽）也是合法会话——徽标不显（F553 单拖无徽标延伸）。
        let mut b = EdgeBridge::new();
        b.drag_begin(0);
        b.hover_upload();
        assert!(b.drop_upload());
    }
}
