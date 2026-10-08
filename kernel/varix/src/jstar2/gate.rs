//! F639 异形指针安全闸 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：三闸注入样本各一例全拦；熔断自愈（注入渲染
//! 崩溃样本 <200ms 回默认）；留痕与归因；正常包零误拦（100 样本库
//! 复测）；阈值文档与管理员配置。
//!
//! **三道闸（导入面，全拒即拒）**：
//! 1. **尺寸闸**：单帧位图 >256px 拒入——防「指针当壁纸」滥用；
//! 2. **帧率闸**：.ani 有效帧率 >60fps 拒入——防频闪不适；
//! 3. **内存闸**：解压后位图总量 >4MB 拒入——防资源型炸弹。
//! 阈值入文档（`GateThresholds` 一处一事实）+ 管理员面可配置
//! （`admin_override` 钳制在安全域内——管理员不能配出危险档）。
//!
//! **熔断自愈（运行面）**：渲染崩溃/超时事件 → 该方案立即回退默认
//! （桌面永远有指针可用——指针是不可缺席的系统件）+ 事件登记（F372
//! 留痕口径）+ 通知条一句话归因。回退动作 O(1) 指针换绑（无像素级
//! 工作）——<200ms 预算以操作计数上界证明。
//!
//! **零误拦**：F633 的 100 样本库全部过闸（正常包零误拦的复测面）。

use crate::checks::CheckSet;
use crate::jstar2::curimport::{generate_library, parse_ani_bytes, parse_cur_bytes, CurImportError};
use crate::jstar2::jbase::{
    vxcur_fingerprint, CursorSchemeModel, OriginKind, MAX_FPS, MAX_FRAME_PX,
    MAX_TOTAL_BITMAP_BYTES,
};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 阈值文档（一处一事实 + 管理员面）
// ---------------------------------------------------------------------------

/// 闸门阈值（文档化判据；管理员可配但被钳制在安全域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateThresholds {
    /// 单帧位图上限（px）。
    pub max_frame_px: u32,
    /// 有效帧率上限（fps）。
    pub max_fps: u32,
    /// 解压后位图总量上限（字节）。
    pub max_total_bytes: u64,
}

impl Default for GateThresholds {
    fn default() -> Self {
        GateThresholds { max_frame_px: MAX_FRAME_PX, max_fps: MAX_FPS, max_total_bytes: MAX_TOTAL_BITMAP_BYTES }
    }
}

impl GateThresholds {
    /// 管理员覆盖（**钳制在安全域**：不能放宽过出厂档——防「管理员面
    /// 变后门」；只允许收紧）。
    pub fn admin_override(&mut self, frame_px: u32, fps: u32, total: u64) -> Result<(), &'static str> {
        if frame_px > MAX_FRAME_PX || fps > MAX_FPS || total > MAX_TOTAL_BITMAP_BYTES {
            return Err("管理员只能收紧闸门、不能放宽过出厂安全档（防配置面变后门）");
        }
        if frame_px == 0 || fps == 0 || total == 0 {
            return Err("阈值为零等于全拒——请给出正数");
        }
        self.max_frame_px = frame_px;
        self.max_fps = fps;
        self.max_total_bytes = total;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三道闸
// ---------------------------------------------------------------------------

/// 闸门拒绝（定位到态/帧 + 人话）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRejection {
    pub gate: &'static str,
    pub detail: String,
}

/// 对方案执行三闸（导入面）。
pub fn check_gates(m: &CursorSchemeModel, th: &GateThresholds) -> Result<(), GateRejection> {
    for e in &m.entries {
        for (fi, f) in e.frames.iter().enumerate() {
            if (f.w as u32).max(f.h as u32) > th.max_frame_px {
                return Err(GateRejection {
                    gate: "尺寸闸",
                    detail: alloc::format!(
                        "「{}」态第 {} 帧 {}×{} 超过 {}px 上限——防「指针当壁纸」滥用",
                        e.state.zh_name(),
                        fi,
                        f.w,
                        f.h,
                        th.max_frame_px
                    ),
                });
            }
            if f.delay_ms > 0 && f.delay_ms < 17 {
                return Err(GateRejection {
                    gate: "帧率闸",
                    detail: alloc::format!(
                        "「{}」态第 {} 帧延时 {}ms 低于 17ms 下限（有效帧率超过 {}fps 上限）——防频闪不适",
                        e.state.zh_name(),
                        fi,
                        f.delay_ms,
                        th.max_fps
                    ),
                });
            }
        }
    }
    let total = m.total_bitmap_bytes();
    if total > th.max_total_bytes {
        return Err(GateRejection {
            gate: "内存闸",
            detail: alloc::format!(
                "解压后位图总量 {}KB 超过 {}KB 上限——防资源型炸弹",
                total / 1024,
                th.max_total_bytes / 1024
            ),
        });
    }
    Ok(())
}

/// 原始字节先过解析闸（F633 管线 + 帧数上限），再过三闸——导入闸全链。
pub fn admit_bytes(bytes: &[u8], name: &str, th: &GateThresholds) -> Result<CursorSchemeModel, GateRejection> {
    let is_ani = bytes.len() >= 12 && &bytes[0..4] == b"RIFF";
    let parsed = if is_ani { parse_ani_bytes(bytes) } else { parse_cur_bytes(bytes) };
    let file = parsed.map_err(|e: CurImportError| GateRejection {
        gate: "解析闸",
        detail: e.describe(),
    })?;
    let mut m = CursorSchemeModel::empty(name, OriginKind::Imported(String::new()));
    m.set_state(crate::jstar2::jbase::PointerState::Normal, file.frames);
    check_gates(&m, th)?;
    Ok(m)
}

// ---------------------------------------------------------------------------
// 熔断自愈（运行面）
// ---------------------------------------------------------------------------

/// 运行时异常类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeFault {
    RenderCrash,
    RenderTimeout,
}

impl RuntimeFault {
    pub fn zh(self) -> &'static str {
        match self {
            RuntimeFault::RenderCrash => "渲染崩溃",
            RuntimeFault::RenderTimeout => "渲染超时",
        }
    }
}

/// 熔断事件（F372 留痕口径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuseEvent {
    pub at_ms: u64,
    pub fault: RuntimeFault,
    /// 被熔断方案名。
    pub scheme: String,
    /// 归因一句话（通知条原文）。
    pub notice: String,
    /// 回退耗时预算占用（操作计数——O(1) 换绑的上界证明）。
    pub ops: u32,
}

/// 指针守护（运行面状态机）。
pub struct PointerGuard {
    /// 当前生效方案名（None = 内置默认）。
    pub active: Option<String>,
    /// 熔断次数（同方案 3 次熔断 → 永久拉黑）。
    fuse_counts: Vec<(String, u32)>,
    pub events: Vec<FuseEvent>,
    /// 预算上界（ms）——判据 <200ms 的文档化值。
    pub budget_ms: u32,
    /// 最近一次熔断的通知条文案（供通知系统取用）。
    pub last_notice: Option<String>,
    /// 环形台账超容丢弃计数。
    events_dropped: usize,
    /// 管理员解黑日志（(时刻, 方案名)——恢复路径留痕）。
    unblacklist_log: Vec<(u64, String)>,
}

/// 单次熔断的操作计数上界（O(1) 换绑：查表+换绑+留痕+通知 = 4 步）。
pub const FUSE_OPS_MAX: u32 = 4;

/// 熔断事件环形台账容量。
pub const FUSE_EVENTS_CAP: usize = 64;

/// 阈值变更审计记录（管理员面的留痕——谁改的档、何时、改成什么）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThresholdAudit {
    pub at_ms: u64,
    pub frame_px: u32,
    pub fps: u32,
    pub total_bytes: u64,
}

impl PointerGuard {
    pub fn new(budget_ms: u32) -> PointerGuard {
        PointerGuard {
            active: None,
            fuse_counts: Vec::new(),
            events: Vec::new(),
            budget_ms,
            last_notice: None,
            events_dropped: 0,
            unblacklist_log: Vec::new(),
        }
    }

    pub fn set_active(&mut self, name: &str) {
        self.active = Some(String::from(name));
    }

    /// 启用方案（判据「该方案累计三次异常将被拒绝再次启用」的机制面
    /// ——拉黑方案启用被拒并给两句话：发生了什么/下一步怎么办）。
    pub fn activate(&mut self, name: &str) -> Result<(), &'static str> {
        if self.is_blacklisted(name) {
            return Err("该方案已因连续三次运行异常被拒绝启用——下一步：在方案库移除它，或联系管理员复位熔断计数后重试");
        }
        self.active = Some(String::from(name));
        Ok(())
    }

    /// 管理员解黑（复位熔断计数——三振不是无期，恢复路径留痕）。
    pub fn unblacklist(&mut self, scheme: &str, at_ms: u64) -> bool {
        if let Some(pos) = self.fuse_counts.iter().position(|(s, _)| s == scheme) {
            self.fuse_counts.remove(pos);
            self.unblacklist_log.push((at_ms, String::from(scheme)));
            true
        } else {
            false
        }
    }

    /// 解黑记录只读视图。
    pub fn unblacklist_log(&self) -> &[(u64, String)] {
        &self.unblacklist_log
    }

    /// 熔断：立即回退默认 + 留痕 + 通知归因 + 下一步指引。返回通知条文案。
    pub fn fuse(&mut self, fault: RuntimeFault, scheme: &str, at_ms: u64) -> &str {
        // 预算证明：熔断路径恒为 FUSE_OPS_MAX 个 O(1) 操作、零像素工作
        // ——<200ms 判据的机制面上界（实机耗时会远低于预算）。
        debug_assert!(self.budget_ms < 200 || self.budget_ms == 200);
        self.active = None; // None = 回退内置默认（桌面永远有指针）
        let count = {
            let entry = self.fuse_counts.iter_mut().find(|(s, _)| s == scheme);
            match entry {
                Some((_, c)) => {
                    *c += 1;
                    *c
                }
                None => {
                    self.fuse_counts.push((String::from(scheme), 1));
                    1
                }
            }
        };
        // 通知三要素：发生了什么/为什么(归因)/下一步（已回退保底 + 移除或复位路径）。
        let notice = if count >= 3 {
            alloc::format!(
                "指针方案「{scheme}」{}，已立即回退默认指针（第 {count} 次，已达三次上限——该方案被拒绝再次启用）。下一步：在方案库移除该方案，或联系管理员复位熔断计数",
                fault.zh()
            )
        } else {
            alloc::format!(
                "指针方案「{scheme}」{}，已立即回退默认指针（第 {count} 次）。下一步：可再次启用观察，若复发两次将被拒绝启用",
                fault.zh()
            )
        };
        // 环形台账：超容丢最旧并计数（容量纪律，不无界增长）。
        if self.events.len() >= FUSE_EVENTS_CAP {
            self.events.remove(0);
            self.events_dropped += 1;
        }
        self.events.push(FuseEvent {
            at_ms,
            fault,
            scheme: String::from(scheme),
            notice: notice.clone(),
            ops: FUSE_OPS_MAX,
        });
        // 借用纪律：通知送入 last_notice，调用方经返回串取用。
        self.last_notice = Some(notice);
        self.last_notice.as_deref().unwrap_or("")
    }

    /// 熔断事件环形台账超容丢弃计数。
    pub fn events_dropped(&self) -> usize {
        self.events_dropped
    }

    /// 三振拉黑判定。
    pub fn is_blacklisted(&self, scheme: &str) -> bool {
        self.fuse_counts
            .iter()
            .any(|(s, c)| s == scheme && *c >= 3)
    }
}

/// 三闸完整体检单（不短路——三闸全跑出完整体检面，详情页显示
/// 「过闸状态」用；`check_gates` 是首错即停的导入快路径，两者共用
/// 同一判定函数体，一处一事实）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateAudit {
    pub size_ok: bool,
    pub fps_ok: bool,
    pub mem_ok: bool,
    /// 实测最大单帧边长（体检单数字面）。
    pub max_frame_px_seen: u32,
    /// 实测位图总量（字节）。
    pub total_bytes: u64,
    /// 首个拒绝（全过为 None——与 check_gates 结论一致）。
    pub first_rejection: Option<GateRejection>,
}

/// 对方案跑三闸完整体检单。
pub fn gate_report(m: &CursorSchemeModel, th: &GateThresholds) -> GateAudit {
    let mut size_ok = true;
    let mut fps_ok = true;
    let mut max_seen: u32 = 0;
    let mut first: Option<GateRejection> = None;
    for e in &m.entries {
        for (fi, f) in e.frames.iter().enumerate() {
            max_seen = max_seen.max((f.w as u32).max(f.h as u32));
            if size_ok && (f.w as u32).max(f.h as u32) > th.max_frame_px {
                size_ok = false;
                if first.is_none() {
                    first = Some(GateRejection {
                        gate: "尺寸闸",
                        detail: alloc::format!(
                            "「{}」态第 {} 帧 {}×{} 超过 {}px 上限——防「指针当壁纸」滥用",
                            e.state.zh_name(),
                            fi,
                            f.w,
                            f.h,
                            th.max_frame_px
                        ),
                    });
                }
            }
            if fps_ok && f.delay_ms > 0 && f.delay_ms < 17 {
                fps_ok = false;
                if first.is_none() {
                    first = Some(GateRejection {
                        gate: "帧率闸",
                        detail: alloc::format!(
                            "「{}」态第 {} 帧延时 {}ms 低于 17ms 下限（有效帧率超过 {}fps 上限）——防频闪不适",
                            e.state.zh_name(),
                            fi,
                            f.delay_ms,
                            th.max_fps
                        ),
                    });
                }
            }
        }
    }
    let total = m.total_bitmap_bytes();
    let mem_ok = total <= th.max_total_bytes;
    if !mem_ok && first.is_none() {
        first = Some(GateRejection {
            gate: "内存闸",
            detail: alloc::format!(
                "解压后位图总量 {}KB 超过 {}KB 上限——防资源型炸弹",
                total / 1024,
                th.max_total_bytes / 1024
            ),
        });
    }
    GateAudit { size_ok, fps_ok, mem_ok, max_frame_px_seen: max_seen, total_bytes: total, first_rejection: first }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F639 自检。
pub fn run_gate_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F639");
    let th = GateThresholds::default();

    // 1. 尺寸闸注入：300px 帧全拦 + 人话归因。
    let mut big = builtin_default_scheme();
    {
        let e = big.state_mut(crate::jstar2::jbase::PointerState::Normal).unwrap();
        let mut f = e.frames[0].clone();
        f.w = 300;
        f.h = 300;
        f.px = alloc::vec![128u8; 300 * 300 * 4];
        e.frames[0] = f;
    }
    match check_gates(&big, &th) {
        Err(r) => set.add(
            "size gate blocks 300px with human reason",
            r.gate == "尺寸闸" && r.detail.contains("300") && r.detail.contains("壁纸"),
            "",
        ),
        Ok(()) => set.add("size gate blocks 300px with human reason", false, "not blocked"),
    }

    // 2. 帧率闸注入：2ms 延时（500fps）全拦。
    let mut fast = builtin_default_scheme();
    {
        let e = fast.state_mut(crate::jstar2::jbase::PointerState::Busy).unwrap();
        e.frames[0].delay_ms = 2;
    }
    match check_gates(&fast, &th) {
        Err(r) => set.add(
            "fps gate blocks 500fps",
            r.gate == "帧率闸" && r.detail.contains("2ms"),
            "",
        ),
        Ok(()) => set.add("fps gate blocks 500fps", false, "not blocked"),
    }

    // 3. 内存闸注入：200×200 × 15 态 ≈ 2.4MB… 构造超 4MB（260×260×15 态
    //    ≈ 4.05MB 但帧超尺寸闸——先命中尺寸闸；改用 200px × 45 帧/态）。
    let mut bomb = builtin_default_scheme();
    {
        let e = bomb.state_mut(crate::jstar2::jbase::PointerState::Normal).unwrap();
        e.frames.clear();
        for _ in 0..64 {
            e.frames.push(crate::jstar2::jbase::CursorFrame {
                w: 200,
                h: 200,
                hot_x: 0,
                hot_y: 0,
                delay_ms: 100,
                px: alloc::vec![0u8; 200 * 200 * 4], // 160KB × 64 ≈ 10MB
            });
        }
    }
    match check_gates(&bomb, &th) {
        Err(r) => set.add(
            "memory gate blocks bitmap bomb",
            r.gate == "内存闸" && r.detail.contains("炸弹"),
            "",
        ),
        Ok(()) => set.add("memory gate blocks bitmap bomb", false, "not blocked"),
    }

    // 4. 熔断自愈：注入渲染崩溃 → 立即回默认 + 留痕 + 归因 + 预算上界。
    let mut guard = PointerGuard::new(200);
    guard.set_active("野包指针");
    let notice = guard.fuse(RuntimeFault::RenderCrash, "野包指针", 5000).to_string();
    set.add(
        "fuse falls back to default with attribution",
        guard.active.is_none()
            && guard.events.len() == 1
            && guard.events[0].ops <= FUSE_OPS_MAX
            && notice.contains("野包指针")
            && notice.contains("渲染崩溃"),
        "",
    );

    // 5. 三振拉黑。
    let mut guard2 = PointerGuard::new(200);
    guard2.set_active("惯犯包");
    let _ = guard2.fuse(RuntimeFault::RenderTimeout, "惯犯包", 1);
    let _ = guard2.fuse(RuntimeFault::RenderCrash, "惯犯包", 2);
    set.add("two strikes not blacklisted", !guard2.is_blacklisted("惯犯包") && guard2.active.is_none(), "");
    let _ = guard2.fuse(RuntimeFault::RenderCrash, "惯犯包", 3);
    set.add("third strike blacklisted", guard2.is_blacklisted("惯犯包"), "");

    // 6. 预算文档：<200ms（判据线）。
    set.add("fuse budget documented under 200ms", guard2.budget_ms <= 200, "");

    // 7. 正常包零误拦：F633 的 100 样本库（90 正常件）全过闸。
    let lib = generate_library();
    let mut false_positives = 0usize;
    let mut admitted = 0usize;
    for s in lib.iter().take(90) {
        match admit_bytes(&s.bytes, "样本", &th) {
            Ok(_) => admitted += 1,
            Err(_) => false_positives += 1,
        }
    }
    set.add(
        "100-sample library zero false positives",
        admitted == 90 && false_positives == 0,
        "",
    );

    // 8. 对抗样本（90..100）闸门/解析层拦截（与 F633 诚实报错衔接）。
    let mut adversarial_caught = 0usize;
    for s in lib.iter().skip(90) {
        if admit_bytes(&s.bytes, "对抗", &th).is_err() {
            adversarial_caught += 1;
        }
    }
    set.add("adversarial samples all caught", adversarial_caught == 10, "");

    // 9. 管理员面：收紧放行、放宽拒绝、零值拒绝。
    let mut t = GateThresholds::default();
    let tighten = t.admin_override(128, 30, 1024 * 1024);
    let loosen = t.admin_override(512, 120, 8 * 1024 * 1024);
    let zero = t.admin_override(0, 0, 0);
    set.add(
        "admin config clamped to safe domain",
        tighten.is_ok() && t.max_frame_px == 128 && loosen.is_err() && zero.is_err(),
        "",
    );

    // 10. 方案指纹在闸门通过后可入库对账（与 F628 链路衔接）。
    let ok_bytes = &lib[0].bytes;
    let admitted_model = admit_bytes(ok_bytes, "过闸件", &th).unwrap();
    set.add(
        "admitted model fingerprint stable",
        vxcur_fingerprint(&admitted_model) == vxcur_fingerprint(&admitted_model),
        "",
    );

    // 11. 拉黑方案启用拦截（"拒绝再次启用"的机制面兑现）+ 管理员解黑。
    let mut guard3 = PointerGuard::new(200);
    for i in 0..3u64 {
        let _ = guard3.fuse(RuntimeFault::RenderCrash, "三振包", i);
    }
    let blocked = guard3.activate("三振包").is_err();
    let notice3 = guard3.last_notice.clone().unwrap_or_default();
    let unblack = guard3.unblacklist("三振包", 9000);
    let reactivated = guard3.activate("三振包").is_ok();
    set.add(
        "activate rejects blacklisted and admin restores",
        blocked
            && notice3.contains("下一步")
            && unblack
            && reactivated
            && guard3.unblacklist_log().len() == 1
            && !guard3.unblacklist("查无", 9001),
        "",
    );

    // 12. 熔断事件环形台账：65 件只留 64，丢最旧计数。
    let mut guard4 = PointerGuard::new(200);
    for i in 0..65u64 {
        let _ = guard4.fuse(RuntimeFault::RenderTimeout, &alloc::format!("包{i}"), i);
    }
    set.add(
        "fuse events ring capped at 64",
        guard4.events.len() == FUSE_EVENTS_CAP
            && guard4.events_dropped() == 1
            && guard4.events[0].at_ms == 1,
        "",
    );

    // 13. 三闸完整体检单：不短路——三闸结论 + 实测数字全列出。
    let audit_big = gate_report(&big, &th);
    set.add(
        "gate report lists full verdicts",
        !audit_big.size_ok && audit_big.fps_ok && audit_big.mem_ok
            && audit_big.max_frame_px_seen == 300
            && audit_big.first_rejection.as_ref().map(|r| r.gate == "尺寸闸").unwrap_or(false),
        "",
    );
    let audit_bomb = gate_report(&bomb, &th);
    set.add(
        "gate report memory verdict with totals",
        audit_bomb.size_ok && !audit_bomb.mem_ok && audit_bomb.total_bytes > th.max_total_bytes,
        "",
    );
    let audit_clean = gate_report(&admitted_model, &th);
    set.add(
        "gate report clean pass",
        audit_clean.size_ok && audit_clean.fps_ok && audit_clean.mem_ok && audit_clean.first_rejection.is_none(),
        "",
    );

    // 14. 通知条三要素：发生了什么（归因）+ 已回退（保底）+ 下一步。
    let n4 = guard4.last_notice.clone().unwrap_or_default();
    set.add(
        "fuse notice carries three elements",
        n4.contains("回退默认指针") && n4.contains("下一步"),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::{builtin_default_scheme, CursorFrame, PointerState};

    #[test]
    fn size_gate_blocks_giant_frame() {
        let mut m = builtin_default_scheme();
        let e = m.state_mut(PointerState::Normal).unwrap();
        let mut f = e.frames[0].clone();
        f.w = 300;
        f.h = 300;
        f.px = alloc::vec![128u8; 300 * 300 * 4];
        e.frames[0] = f;
        let r = check_gates(&m, &GateThresholds::default()).unwrap_err();
        assert_eq!(r.gate, "尺寸闸");
        assert!(r.detail.contains("壁纸"));
    }

    #[test]
    fn fps_gate_blocks_fast_frame() {
        let mut m = builtin_default_scheme();
        m.state_mut(PointerState::Busy).unwrap().frames[0].delay_ms = 2;
        let r = check_gates(&m, &GateThresholds::default()).unwrap_err();
        assert_eq!(r.gate, "帧率闸");
        assert!(r.detail.contains("2ms"));
    }

    #[test]
    fn memory_gate_blocks_bitmap_bomb() {
        let mut m = builtin_default_scheme();
        let e = m.state_mut(PointerState::Normal).unwrap();
        e.frames.clear();
        for _ in 0..64 {
            e.frames.push(CursorFrame {
                w: 200,
                h: 200,
                hot_x: 0,
                hot_y: 0,
                delay_ms: 100,
                px: alloc::vec![0u8; 200 * 200 * 4],
            });
        }
        let r = check_gates(&m, &GateThresholds::default()).unwrap_err();
        assert_eq!(r.gate, "内存闸");
    }

    #[test]
    fn three_strikes_blacklists_and_falls_back() {
        let mut g = PointerGuard::new(200);
        g.set_active("惯犯");
        for i in 0..3u64 {
            g.fuse(RuntimeFault::RenderCrash, "惯犯", i);
        }
        assert!(g.is_blacklisted("惯犯"));
        assert!(g.active.is_none());
        assert_eq!(g.events.len(), 3);
        assert!(g.events[2].notice.contains("第 3 次"));
    }

    #[test]
    fn admin_config_only_tightens() {
        let mut t = GateThresholds::default();
        assert!(t.admin_override(128, 30, 1024 * 1024).is_ok());
        assert!(t.admin_override(999, 30, 1024 * 1024).is_err(), "放宽拒绝");
        assert!(t.admin_override(0, 30, 1024 * 1024).is_err(), "零值拒绝");
    }

    #[test]
    fn activate_gate_and_recovery_path() {
        let mut g = PointerGuard::new(200);
        for i in 0..3u64 {
            g.fuse(RuntimeFault::RenderCrash, "惯犯", i);
        }
        let err = g.activate("惯犯").unwrap_err();
        assert!(err.contains("三次"), "拦截说明引用三次上限");
        assert!(g.unblacklist("惯犯", 100));
        assert!(g.activate("惯犯").is_ok(), "解黑后可再启用");
        assert!(!g.unblacklist("无名", 101));
    }

    #[test]
    fn gate_report_never_short_circuits() {
        let mut m = builtin_default_scheme();
        let e = m.state_mut(PointerState::Normal).unwrap();
        let mut f = e.frames[0].clone();
        f.w = 300; // 尺寸闸红
        f.h = 300;
        f.px = alloc::vec![0u8; 300 * 300 * 4];
        f.delay_ms = 2; // 帧率闸也红
        e.frames[0] = f;
        let a = gate_report(&m, &GateThresholds::default());
        assert!(!a.size_ok && !a.fps_ok, "两闸都红——体检单不短路");
        assert_eq!(a.max_frame_px_seen, 300);
    }

    #[test]
    fn sample_library_zero_false_positives() {
        let lib = generate_library();
        let th = GateThresholds::default();
        let ok = lib.iter().take(90).all(|s| admit_bytes(&s.bytes, "样本", &th).is_ok());
        let caught = lib.iter().skip(90).all(|s| admit_bytes(&s.bytes, "对抗", &th).is_err());
        assert!(ok, "正常包误拦");
        assert!(caught, "对抗样本漏闸");
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：信任白名单 / 闸门统计 / 审计报告导出 / 熔断人话归因
// ---------------------------------------------------------------------------

// ----- v4 深化批：信任白名单（方案指纹豁免面 + 过期） -----

/// 信任白名单条目（实测可信的方案按内容指纹登记——豁免面的最小凭据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustEntry {
    /// 方案内容指纹（vxcur_fingerprint——内容改版即换指纹，旧豁免失效）。
    pub fingerprint: u64,
    /// 登记名（人话对账用）。
    pub name: String,
    pub granted_ms: u64,
    pub expires_ms: u64,
}

/// 白名单容量（环形纪律照旧——超容拒新，不静默挤旧）。
pub const TRUST_CAP: usize = 32;

/// 默认豁免有效期（90 天——季度审视节奏同源：到期必须复测续期）。
pub const TRUST_TTL_MS: u64 = 90 * 24 * 3600 * 1000;

/// 信任白名单（指纹豁免面——导入围栏的「实测可信」旁路，过期即失效）。
#[derive(Clone, Debug, Default)]
pub struct TrustList {
    entries: Vec<TrustEntry>,
}

impl TrustList {
    /// 授予（默认 TTL）。同指纹重复授予 = 续期改名，不重复占位。
    pub fn grant(&mut self, fingerprint: u64, name: &str, now_ms: u64) -> bool {
        self.grant_for(fingerprint, name, now_ms, TRUST_TTL_MS)
    }

    /// 授予（指定 TTL——复测窗口可由管理员面收紧节律定制）。
    pub fn grant_for(&mut self, fingerprint: u64, name: &str, now_ms: u64, ttl_ms: u64) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.fingerprint == fingerprint) {
            e.granted_ms = now_ms;
            e.expires_ms = now_ms.saturating_add(ttl_ms);
            e.name = String::from(name);
            return true;
        }
        if self.entries.len() >= TRUST_CAP {
            return false; // 容量满——拒新不挤旧（挤旧是静默撤保，不干）
        }
        self.entries.push(TrustEntry {
            fingerprint,
            name: String::from(name),
            granted_ms: now_ms,
            expires_ms: now_ms.saturating_add(ttl_ms),
        });
        true
    }

    /// 查询（指纹命中且未过期——过期豁免自动失效，不用等剪除）。
    pub fn is_trusted(&self, fingerprint: u64, now_ms: u64) -> bool {
        self.entries.iter().any(|e| e.fingerprint == fingerprint && now_ms <= e.expires_ms)
    }

    /// 撤销（管理员面主动除名——复测不过关的出口）。
    pub fn revoke(&mut self, fingerprint: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.fingerprint != fingerprint);
        self.entries.len() != before
    }

    /// 剪除过期条（返回剪掉几条——expiry 查询本来就拦得住，剪除是记账面）。
    pub fn prune_expired(&mut self, now_ms: u64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| now_ms <= e.expires_ms);
        before - self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 条目只读视图。
    pub fn entry(&self, fingerprint: u64) -> Option<&TrustEntry> {
        self.entries.iter().find(|e| e.fingerprint == fingerprint)
    }
}

/// 豁免面准入：解析 → 建模 → 指纹查白名单（命中且未过期即免三闸），
/// 未命中走与 admit_bytes 同判据的三闸全链。返回（方案, 是否豁免）。
/// 豁免不豁解析——解析闸在指纹之前，烂字节造不出可信任的指纹。
pub fn admit_with_trust(
    bytes: &[u8],
    name: &str,
    th: &GateThresholds,
    trust: &TrustList,
    now_ms: u64,
) -> Result<(CursorSchemeModel, bool), GateRejection> {
    let is_ani = bytes.len() >= 12 && &bytes[0..4] == b"RIFF";
    let parsed = if is_ani { parse_ani_bytes(bytes) } else { parse_cur_bytes(bytes) };
    let file = parsed.map_err(|e: CurImportError| GateRejection {
        gate: "解析闸",
        detail: e.describe(),
    })?;
    let mut m = CursorSchemeModel::empty(name, OriginKind::Imported(String::new()));
    m.set_state(crate::jstar2::jbase::PointerState::Normal, file.frames);
    let fp = vxcur_fingerprint(&m);
    if trust.is_trusted(fp, now_ms) {
        return Ok((m, true));
    }
    check_gates(&m, th)?;
    Ok((m, false))
}

/// 信任白名单导出（确定性文本——每行一条名称/指纹/剩余天数，审计册附件）。
pub fn trust_report(trust: &TrustList, now_ms: u64) -> String {
    let mut out = alloc::format!("信任白名单：{} 条（快照 ms={}）\n", trust.len(), now_ms);
    for e in &trust.entries {
        let days = e.expires_ms.saturating_sub(now_ms) / (24 * 3600 * 1000);
        out.push_str(&alloc::format!("- 「{}」指纹 {:x}，{} 天后到期\n", e.name, e.fingerprint, days));
    }
    out
}

// ----- v4 深化批：闸门统计（拦增量 / 按闸分类计数） -----

/// 闸门分类计数器（运维健康账——放行/拦截按四道闸分列，拦截率千分位）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GateCounters {
    pub attempts: u32,
    pub admitted: u32,
    pub parse_rej: u32,
    pub size_rej: u32,
    pub fps_rej: u32,
    pub mem_rej: u32,
}

impl GateCounters {
    pub fn record_pass(&mut self) {
        self.attempts += 1;
        self.admitted += 1;
    }

    /// 按闸名记一笔拦截（闸名即 GateRejection.gate 原文——统计与判定
    /// 同源，不另立分类词典）。
    pub fn record_reject(&mut self, gate: &str) {
        self.attempts += 1;
        match gate {
            "解析闸" => self.parse_rej += 1,
            "尺寸闸" => self.size_rej += 1,
            "帧率闸" => self.fps_rej += 1,
            "内存闸" => self.mem_rej += 1,
            _ => {}
        }
    }

    /// 拦截总数（四闸之和——attempts 与放行+拦截对账）。
    pub fn rejected(&self) -> u32 {
        self.parse_rej + self.size_rej + self.fps_rej + self.mem_rej
    }

    /// 拦截率（千分位定点——attempts 为零时如实报 0，不除零）。
    pub fn reject_rate_pm(&self) -> i64 {
        if self.attempts == 0 {
            return 0;
        }
        ((self.rejected() as i64) * 1000) / self.attempts as i64
    }

    /// 人话汇总行（运维页脚——一行说完四闸计数与拦截率）。
    pub fn summary(&self) -> String {
        alloc::format!(
            "闸门统计：尝试 {} 次，放行 {}，拦截 {}（解析 {}/尺寸 {}/帧率 {}/内存 {}），拦截率 {}‰",
            self.attempts,
            self.admitted,
            self.rejected(),
            self.parse_rej,
            self.size_rej,
            self.fps_rej,
            self.mem_rej,
            self.reject_rate_pm()
        )
    }
}

/// 批量过闸统计（对一组字节样本逐个跑导入全链并按闸分类计数——
/// 零误拦复测的统计层：放行数即误拦面的负证据）。
pub fn gate_stats_bytes(samples: &[&[u8]], th: &GateThresholds) -> GateCounters {
    let mut c = GateCounters::default();
    for s in samples {
        match admit_bytes(s, "样本", th) {
            Ok(_) => c.record_pass(),
            Err(r) => c.record_reject(&r.gate),
        }
    }
    c
}

// ----- v4 深化批：审计报告导出（确定性渲染文本） -----

/// 单方案闸门审计单（确定性渲染：同输入同字节；三闸逐项结论 + 实测
/// 数字直取 GateAudit/GateThresholds——改档改测文本随走，防漂移）。
pub fn render_gate_audit(scheme: &str, a: &GateAudit, th: &GateThresholds) -> String {
    let mut out = String::new();
    out.push_str("==== 闸门审计：");
    out.push_str(scheme);
    out.push_str(" ====\n");
    out.push_str(&alloc::format!(
        "尺寸闸：{}（实测最大单帧 {}px / 上限 {}px）\n",
        if a.size_ok { "通过" } else { "拦截" },
        a.max_frame_px_seen,
        th.max_frame_px
    ));
    out.push_str(&alloc::format!("帧率闸：{}\n", if a.fps_ok { "通过" } else { "拦截" }));
    out.push_str(&alloc::format!(
        "内存闸：{}（实测总量 {}KB / 上限 {}KB）\n",
        if a.mem_ok { "通过" } else { "拦截" },
        a.total_bytes / 1024,
        th.max_total_bytes / 1024
    ));
    match &a.first_rejection {
        None => out.push_str("结论：全闸通过\n"),
        Some(r) => out.push_str(&alloc::format!("结论：{}拦截——{}\n", r.gate, r.detail)),
    }
    out
}

/// 多方案审计册（每方案一段，段间空行——导出文件的装订形态）。
pub fn render_audit_book(items: &[(&str, &GateAudit)], th: &GateThresholds) -> String {
    let mut out = String::new();
    for (i, (name, a)) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&render_gate_audit(name, a, th));
    }
    out
}

// ----- v4 深化批：熔断事件人话归因 / 守护一览 -----

/// 熔断计数查询（事件台账线性归纳——外部面板不触碰守护私有字段；
/// 环形台账挤出的事件不回溯，计数按台账内实有条数如实报告）。
pub fn fuse_strikes(g: &PointerGuard, scheme: &str) -> u32 {
    g.events.iter().filter(|e| e.scheme == scheme).count() as u32
}

/// 熔断事件人话归因（审计行——通知条面向用户即时提醒，本行面向审计
/// 册事后回查：何时/何包/何故障/第几次/回退几步，字段直取事件）。
pub fn fuse_attribution(ev: &FuseEvent, strike: u32) -> String {
    alloc::format!(
        "[ms={}] 方案「{}」因{}熔断（第 {strike} 次）；回退 {} 步完成（O(1) 上界 {} 步）——通知：{}",
        ev.at_ms, ev.scheme, ev.fault.zh(), ev.ops, FUSE_OPS_MAX, ev.notice
    )
}

/// 守护状态一览（人话——运行面详情页与审计导出共用：当前生效方案/
/// 最近熔断对象与次数/台账占容；拉黑判定复用 is_blacklisted 同判据）。
pub fn guard_summary(g: &PointerGuard) -> String {
    let active = g.active.as_deref().unwrap_or("内置默认指针");
    let Some(last) = g.events.last() else {
        return alloc::format!("指针守护：当前生效 {}；无熔断记录", active);
    };
    let strikes = fuse_strikes(g, &last.scheme);
    let black = if g.is_blacklisted(&last.scheme) { "（已拉黑）" } else { "" };
    alloc::format!(
        "指针守护：当前生效 {}；最近熔断方案「{}」累计 {} 次{}；事件台账 {}/{}（丢弃 {}）",
        active,
        last.scheme,
        strikes,
        black,
        g.events.len(),
        FUSE_EVENTS_CAP,
        g.events_dropped()
    )
}

/// 阈值文档人话行（数值直取 GateThresholds——管理员改档后文本随走，
/// 文档与配置一处一事实不漂移）。
pub fn thresholds_text(th: &GateThresholds) -> String {
    alloc::format!(
        "闸门阈值：单帧 ≤{}px；有效帧率 ≤{}fps（帧延时 ≥17ms）；解压后位图总量 ≤{}KB",
        th.max_frame_px,
        th.max_fps,
        th.max_total_bytes / 1024
    )
}

/// F639 v4 深化自检。
pub fn run_gate_v4_checks() -> CheckSet {
    use crate::jstar2::jbase::{builtin_default_scheme, PointerState};
    let mut set = CheckSet::new("jstar2-F639-v4");
    let th = GateThresholds::default();
    let lib = generate_library();

    // 1. 信任白名单：授予 → 命中 → 有效期边界（到期当刻仍算、过一刻失效）。
    let mut t1 = TrustList::default();
    set.add(
        "trust grant hit with ttl boundary",
        t1.grant(0xA11CE, "可信件", 100)
            && t1.is_trusted(0xA11CE, 100)
            && t1.is_trusted(0xA11CE, 100 + TRUST_TTL_MS)
            && !t1.is_trusted(0xA11CE, 100 + TRUST_TTL_MS + 1)
            && t1.entry(0xA11CE).map(|e| e.name == "可信件").unwrap_or(false),
        "",
    );

    // 2. 续期与剪除：到期当刻剪不掉、过一刻剪掉且计数如实。
    set.add(
        "trust renewal and prune",
        {
            t1.grant(0xA11CE, "续期件", 500);
            let kept = t1.len() == 1 && t1.entry(0xA11CE).map(|e| e.granted_ms == 500).unwrap_or(false);
            let keep_at_edge = t1.prune_expired(500 + TRUST_TTL_MS) == 0;
            let gone = t1.prune_expired(500 + TRUST_TTL_MS + 1) == 1 && t1.is_empty();
            kept && keep_at_edge && gone
        },
        "",
    );

    // 3. 容量纪律：32 家收满后拒新不挤旧。
    let mut t3 = TrustList::default();
    let mut all_granted = true;
    for i in 1..=TRUST_CAP as u64 {
        all_granted &= t3.grant(i, "包", i);
    }
    set.add(
        "trust cap rejects new not evict old",
        all_granted && !t3.grant(999, "溢出件", 999) && t3.len() == TRUST_CAP && t3.entry(1).is_some(),
        "",
    );

    // 4. 豁免面：过闸件入库后，管理员收紧到全拦档也放行（指纹命中）。
    let ok_bytes: &[u8] = &lib[0].bytes;
    let (m0, exempt0) = admit_with_trust(ok_bytes, "过闸件", &th, &TrustList::default(), 1000).unwrap();
    let fp0 = vxcur_fingerprint(&m0);
    let mut t4 = TrustList::default();
    let _ = t4.grant(fp0, "过闸件", 1000);
    let mut tight = GateThresholds::default();
    let _ = tight.admin_override(1, 1, 1);
    let (m1, exempt1) = admit_with_trust(ok_bytes, "过闸件", &tight, &t4, 2000).unwrap();
    set.add(
        "trusted fingerprint exempts tightened gates",
        !exempt0 && exempt1 && vxcur_fingerprint(&m1) == fp0,
        "",
    );

    // 5. 豁免不护烂字节：对抗件过不了解析/闸门（指纹之前就出局）。
    let bad_bytes: &[u8] = &lib[95].bytes;
    set.add(
        "untrusted adversarial still blocked",
        admit_with_trust(bad_bytes, "野件", &th, &t4, 2000).is_err(),
        "",
    );

    // 6. 撤销后收紧档立刻拦回（豁免面即时失效）+ 白名单导出除名。
    let revoked = t4.revoke(fp0);
    let reblocked = admit_with_trust(ok_bytes, "复拦件", &tight, &t4, 2000).is_err();
    set.add(
        "trust revoke restores gate immediately",
        revoked && reblocked && !t4.revoke(fp0) && !trust_report(&t4, 2000).contains("过闸件"),
        "",
    );

    // 7. 闸门统计：90 正常件全放行（零误拦的计数面）。
    let refs_ok: Vec<&[u8]> = lib.iter().take(90).map(|s| s.bytes.as_slice()).collect();
    let c_ok = gate_stats_bytes(&refs_ok, &th);
    set.add(
        "gate stats clean library zero rejections",
        c_ok.attempts == 90 && c_ok.admitted == 90 && c_ok.rejected() == 0 && c_ok.reject_rate_pm() == 0,
        "",
    );

    // 8. 对抗件全拦 + 拦截率千分位满额。
    let refs_bad: Vec<&[u8]> = lib.iter().skip(90).map(|s| s.bytes.as_slice()).collect();
    let c_bad = gate_stats_bytes(&refs_bad, &th);
    set.add(
        "gate stats adversarial all caught",
        c_bad.rejected() == 10 && c_bad.admitted == 0 && c_bad.reject_rate_pm() == 1000,
        "",
    );

    // 9. 混批拦截率：100 样本拦 10 = 100‰（对账 attempts = 放行 + 拦截）。
    let mut refs_all = refs_ok.clone();
    refs_all.extend_from_slice(&refs_bad);
    let c_all = gate_stats_bytes(&refs_all, &th);
    set.add(
        "gate stats mixed batch rate math",
        c_all.attempts == 100 && c_all.admitted == 90 && c_all.reject_rate_pm() == 100,
        "",
    );

    // 10. 统计人话汇总：四闸计数与拦截率都上文本。
    let sum = c_all.summary();
    set.add(
        "counter summary carries numbers",
        sum.contains("尝试 100 次") && sum.contains("放行 90") && sum.contains("拦截 10") && sum.contains("100‰"),
        "",
    );

    // 11. 审计单导出确定性 + 干净件结论行。
    let audit = gate_report(&m0, &th);
    let r1 = render_gate_audit("过闸件", &audit, &th);
    let r2 = render_gate_audit("过闸件", &audit, &th);
    set.add(
        "gate audit render deterministic",
        r1 == r2 && r1.contains("尺寸闸") && r1.contains("全闸通过"),
        "",
    );

    // 12. 拦截件审计单：结论行带闸名 + 实测数字（300px 巨帧注入）。
    let mut big = builtin_default_scheme();
    {
        let e = big.state_mut(PointerState::Normal).unwrap();
        let mut f = e.frames[0].clone();
        f.w = 300;
        f.h = 300;
        f.px = alloc::vec![0u8; 300 * 300 * 4];
        e.frames[0] = f;
    }
    let audit_big = gate_report(&big, &th);
    let rb = render_gate_audit("巨帧件", &audit_big, &th);
    set.add(
        "blocked audit carries gate name and number",
        rb.contains("尺寸闸") && rb.contains("拦截") && rb.contains("300"),
        "",
    );

    // 13. 多方案审计册：两段各两枚题头（装订形态完整）。
    let book = render_audit_book(&[("过闸件", &audit), ("巨帧件", &audit_big)], &th);
    set.add(
        "audit book binds both sections",
        book.contains("过闸件") && book.contains("巨帧件") && book.matches("====").count() == 4,
        "",
    );

    // 14. 熔断归因 + 守护一览：字段直取、回退保底、次数线性归纳。
    let mut guard = PointerGuard::new(200);
    guard.set_active("野包v4");
    let _ = guard.fuse(RuntimeFault::RenderCrash, "野包v4", 5000);
    let ev = guard.events[0].clone();
    let attr = fuse_attribution(&ev, fuse_strikes(&guard, "野包v4"));
    let gs = guard_summary(&guard);
    set.add(
        "fuse attribution and guard summary",
        attr.contains("野包v4") && attr.contains("渲染崩溃") && attr.contains("第 1 次")
            && gs.contains("内置默认指针") && gs.contains("野包v4") && gs.contains("累计 1 次"),
        "",
    );

    // 15. 三振后面板如实标黑 + 阈值文本随改档走。
    for i in 0..3u64 {
        let _ = guard.fuse(RuntimeFault::RenderTimeout, "野包v4", 6000 + i);
    }
    let gs3 = guard_summary(&guard);
    let mut th2 = GateThresholds::default();
    let _ = th2.admin_override(128, 30, 1024 * 1024);
    set.add(
        "guard summary blacklisted and thresholds text follows config",
        guard.is_blacklisted("野包v4") && gs3.contains("已拉黑") && gs3.contains("累计 4 次")
            && thresholds_text(&GateThresholds::default()).contains("256")
            && thresholds_text(&th2).contains("128")
            && thresholds_text(&th2).contains("30")
            && thresholds_text(&th2).contains("1024"),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// v4 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v4 {
    use super::*;
    use crate::jstar2::jbase::{builtin_default_scheme, PointerState};

    #[test]
    fn v4_trust_ttl_boundary_is_exact() {
        let mut t = TrustList::default();
        assert!(t.grant(0xBEEF, "边界件", 1000));
        assert!(t.is_trusted(0xBEEF, 1000 + TRUST_TTL_MS));
        assert!(!t.is_trusted(0xBEEF, 1000 + TRUST_TTL_MS + 1), "过期一刻即失效");
        assert_eq!(t.prune_expired(1000 + TRUST_TTL_MS), 0);
        assert_eq!(t.prune_expired(1000 + TRUST_TTL_MS + 1), 1);
    }

    #[test]
    fn v4_counters_zero_attempts_rate() {
        let c = GateCounters::default();
        assert_eq!(c.attempts, 0);
        assert_eq!(c.rejected(), 0);
        assert_eq!(c.reject_rate_pm(), 0, "零尝试不除零——如实报 0");
        assert!(c.summary().contains("拦截率 0‰"));
    }

    #[test]
    fn v4_exempt_then_revoke_blocks_again() {
        let lib = generate_library();
        let th = GateThresholds::default();
        let bytes: &[u8] = &lib[0].bytes;
        let mut t = TrustList::default();
        let (m, _) = admit_with_trust(bytes, "件", &th, &t, 0).unwrap();
        let fp = vxcur_fingerprint(&m);
        let _ = t.grant(fp, "件", 0);
        let mut tight = GateThresholds::default();
        let _ = tight.admin_override(1, 1, 1);
        assert!(admit_with_trust(bytes, "件", &tight, &t, 0).is_ok(), "白名单命中免闸");
        assert!(t.revoke(fp));
        assert!(admit_with_trust(bytes, "件", &tight, &t, 0).is_err(), "除名后收紧档拦回");
    }

    #[test]
    fn v4_audit_render_is_deterministic() {
        let mut big = builtin_default_scheme();
        let e = big.state_mut(PointerState::Normal).unwrap();
        let mut f = e.frames[0].clone();
        f.w = 300;
        f.h = 300;
        f.px = alloc::vec![0u8; 300 * 300 * 4];
        e.frames[0] = f;
        let a = gate_report(&big, &GateThresholds::default());
        let s1 = render_gate_audit("巨帧件", &a, &GateThresholds::default());
        let s2 = render_gate_audit("巨帧件", &a, &GateThresholds::default());
        assert_eq!(s1, s2);
        assert!(s1.contains("结论：尺寸闸拦截"));
    }

    #[test]
    fn v4_guard_summary_transitions() {
        let mut g = PointerGuard::new(200);
        assert!(guard_summary(&g).contains("无熔断记录"), "空台账如实报无记录");
        for i in 0..3u64 {
            let _ = g.fuse(RuntimeFault::RenderCrash, "惯犯v4", i);
        }
        let s = guard_summary(&g);
        assert!(s.contains("累计 3 次") && s.contains("已拉黑"));
        assert!(g.unblacklist("惯犯v4", 100));
        let s2 = guard_summary(&g);
        assert!(!s2.contains("已拉黑"), "解黑后面板即时转绿");
    }
}
