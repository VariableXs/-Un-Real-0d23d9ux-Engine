//! 深化层 · F589 拖拽上传 Edge（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F589 节）：
//! ①「落点高亮 + 计数徽标」的**落点分流器**——上传区/正文两落点的
//!   事件路由，计数徽标只在合法上传区挂账（悬到正文即收起）；
//! ②「系统不越界审计」的**越界审计器**——系统侧禁止行为白名单
//!   （自建进度 UI/读取上传文件内容/代写存储），每项「不做」可独立
//!   验证（被执行即红）；
//! ③「F553 同源」的**计数徽标对账**——DragBadge 计数与移交网页的
//!   文件数是同一份账（同源才不两面派）；
//! ④「拖离取消」的**取消账**——拖出窗口/松手在非法区 = 取消且零
//!   副作用（无落点、无系统动作、徽标清零）。

use crate::checks::CheckSet;
use crate::istar::dropupload::{DropSession, EdgeBridge, WebDropZone};
use crate::istar::ibase::ISTAR_DOMAIN;
#[cfg(test)]
use crate::istar::dragbadge::BADGE_MIN_COUNT;
use crate::istar::dragbadge::DragBadge;

// ---------------------------------------------------------------------------
// ① 落点分流器
// ---------------------------------------------------------------------------

/// 系统级拖放事件路由（徽标 + 互通桥的组合账——路由语义唯一源）。
pub struct DropRouter {
    badge: DragBadge,
    bridge: EdgeBridge,
    files: u32,
    over_upload: bool,
}

impl DropRouter {
    /// 开始拖拽：文件数同源喂两账（DragBadge 计数 + EdgeBridge 会话）。
    pub fn new(files: u32) -> DropRouter {
        let mut r = DropRouter {
            badge: DragBadge::new(),
            bridge: EdgeBridge::new(),
            files,
            over_upload: false,
        };
        r.badge.set_dragging(true);
        for i in 0..files as u64 {
            r.badge.select(100 + i);
        }
        r.bridge.drag_begin(files);
        r
    }

    /// 悬停分流：上传区挂徽标；其余落点徽标收起（计数徽标只在
    /// 合法上传区挂账——正文不显数）。
    pub fn hover(&mut self, zone: WebDropZone) {
        self.over_upload = zone == WebDropZone::UploadArea;
        if self.over_upload {
            let _ = self.bridge.hover_upload();
        }
    }

    pub fn badge_visible(&self) -> bool {
        self.over_upload && self.badge.badge_visible()
    }

    pub fn badge_count(&self) -> usize {
        self.badge.count()
    }

    /// 落点：上传区移交网页；正文等 Edge 自身语义。成功即拖放收束
    /// （徽标随落点撤展——数已交账）。
    pub fn drop(&mut self, zone: WebDropZone) -> bool {
        let ok = match zone {
            WebDropZone::UploadArea => self.bridge.drop_upload(),
            z => self.bridge.drop_elsewhere(z),
        };
        if ok {
            self.badge.set_dragging(false);
            self.over_upload = false;
        }
        ok
    }

    /// 拖离取消：拖出窗口/松手在非法区——徽标清零、会话收回
    /// （落点 None = 零落点，系统动作账保持空白 = 零副作用）。
    pub fn cancel(&mut self) -> Option<DropSession> {
        self.badge.clear();
        self.badge.set_dragging(false);
        self.over_upload = false;
        self.bridge.finish()
    }

    /// 会话收束（正常完成侧——对账取数口）。
    pub fn finish(&mut self) -> Option<DropSession> {
        self.bridge.finish()
    }

    /// 越界审计转发（互通桥白名单核对）。
    pub fn audit_violations(&mut self) -> u32 {
        self.bridge.audit()
    }

    pub fn file_count(&self) -> u32 {
        self.files
    }
}

// ---------------------------------------------------------------------------
// ② 越界审计器
// ---------------------------------------------------------------------------

/// 系统侧禁止行为白名单（浏览器的事浏览器管——每项「不做」可验证）。
pub struct ForbiddenAudit {
    /// (禁止行为名, 是否被执行)——执行位恒 false 才是本分。
    forbidden: [(&'static str, bool); 3],
}

impl ForbiddenAudit {
    pub fn new() -> ForbiddenAudit {
        ForbiddenAudit {
            forbidden: [
                // 大文件上传进度走 Edge 自身 UI（系统不抢戏）。
                ("自建进度UI", false),
                // 互通层只交文件句柄，不偷读内容。
                ("读取上传文件内容", false),
                // 落盘/存储是浏览器的事。
                ("代写存储", false),
            ],
        }
    }

    /// 标记一次行为发生（越界记红——审计的取证口）。
    pub fn perform(&mut self, name: &str) -> bool {
        for f in self.forbidden.iter_mut() {
            if f.0 == name {
                f.1 = true;
                return true;
            }
        }
        false
    }

    /// 违规数（>0 即系统越界）。
    pub fn violations(&self) -> usize {
        self.forbidden.iter().filter(|f| f.1).count()
    }

    /// 全部克制（白名单每项保持未执行）。
    pub fn all_refrained(&self) -> bool {
        self.forbidden.iter().all(|f| !f.1)
    }

    pub fn item_count(&self) -> usize {
        self.forbidden.len()
    }
}

impl Default for ForbiddenAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f589_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 合法链全程：拖 3 文件 → 悬上传区徽标可见计数 3 → 落点成立。
    let mut r = DropRouter::new(3);
    r.hover(WebDropZone::UploadArea);
    let hovering = r.badge_visible() && r.badge_count() == 3;
    let dropped = r.drop(WebDropZone::UploadArea);
    cs.add(
        "legal chain badge and drop",
        hovering && dropped && r.file_count() == 3,
        "",
    );

    // 2) 徽标只在合法上传区挂账：悬到正文徽标收起（网页原生语义的地盘）。
    let mut r2 = DropRouter::new(3);
    r2.hover(WebDropZone::UploadArea);
    let on_area = r2.badge_visible();
    r2.hover(WebDropZone::PageBody);
    cs.add(
        "badge only over upload area",
        on_area && !r2.badge_visible(),
        "",
    );

    // 3) 计数徽标同源对账：DragBadge 计数 == 移交网页的文件数（同一份账）。
    let mut r3 = DropRouter::new(4);
    r3.hover(WebDropZone::UploadArea);
    let _ = r3.drop(WebDropZone::UploadArea);
    let badge_n = r3.badge_count();
    let session = r3.finish();
    cs.add(
        "badge count same source as handover",
        badge_n == 4
            && session
                .map(|s| s.files as usize == badge_n)
                .unwrap_or(false),
        "",
    );

    // 4) 正文落点：Edge 自身语义（打开/搜索）——系统动作账零介入。
    let mut r4 = DropRouter::new(2);
    let to_body = r4.drop(WebDropZone::PageBody);
    let s4 = r4.finish();
    cs.add(
        "page body edge semantics",
        to_body
            && s4.map(|s| {
                s.zone == Some(WebDropZone::PageBody) && s.system_actions.is_empty()
            })
            .unwrap_or(false),
        "",
    );

    // 5) 越界审计：合法链全程白名单全克制、互通桥零违规。
    let mut r5 = DropRouter::new(3);
    r5.hover(WebDropZone::UploadArea);
    let _ = r5.drop(WebDropZone::UploadArea);
    let fa = ForbiddenAudit::new();
    cs.add(
        "legal chain fully refrained",
        fa.all_refrained()
            && fa.violations() == 0
            && fa.item_count() == 3
            && r5.audit_violations() == 0,
        "",
    );

    // 6) 越界取证：禁止行为被执行即记红（审计不是摆设——可红）。
    let mut fa2 = ForbiddenAudit::new();
    let known = fa2.perform("自建进度UI");
    let unknown = fa2.perform("不存在的越界");
    cs.add(
        "forbidden audit turns red",
        known && !unknown && fa2.violations() == 1 && !fa2.all_refrained(),
        "",
    );

    // 7) 拖离取消：零落点、零移交（系统动作只剩白名单内的徽标挂账，
    //    无「文件句柄移交网页」——未交给网页即零副作用）、徽标清零。
    let mut r7 = DropRouter::new(5);
    r7.hover(WebDropZone::UploadArea);
    let cancelled = r7.cancel();
    cs.add(
        "drag away cancel zero effects",
        cancelled
            .map(|s| {
                s.zone.is_none()
                    && s.system_actions.len() == 1
                    && s.system_actions[0] == "徽标计数显示"
                    && s.web_actions.len() == 1
                    && s.web_actions[0] == "落点高亮"
            })
            .unwrap_or(false)
            && r7.badge_count() == 0
            && !r7.badge_visible(),
        "",
    );

    // 8) 一次拖拽一个落点：落完上传区再落正文被拒（会话已收束）。
    let mut r8 = DropRouter::new(1);
    let first = r8.drop(WebDropZone::UploadArea);
    let second = r8.drop(WebDropZone::DownloadsBar);
    cs.add("single drop per session", first && !second, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_drag_no_badge() {
        // F553 单拖无徽标延伸：1 文件 < BADGE_MIN_COUNT 不显数。
        assert_eq!(BADGE_MIN_COUNT, 2);
        let mut r = DropRouter::new(1);
        r.hover(WebDropZone::UploadArea);
        assert!(!r.badge_visible());
        assert_eq!(r.badge_count(), 1);
    }

    #[test]
    fn cancel_without_hover_still_clean() {
        let mut r = DropRouter::new(2);
        let s = r.cancel();
        assert!(s.map(|s| s.zone.is_none()).unwrap_or(false));
    }

    #[test]
    fn perform_unknown_action_rejected() {
        let mut fa = ForbiddenAudit::new();
        assert!(!fa.perform("查无此项"));
        assert!(fa.all_refrained());
    }
}
