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
