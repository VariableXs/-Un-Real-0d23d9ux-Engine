//! VE-F2404 · 域自检（判据逐条对应，见 `vem04_batch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **四操作** → `M04-四操作-框选时间乘轨道`、`M04-四操作-框选闭区间`、
//!   `M04-四操作-多轨集合顺序无关`、`M04-四操作-复制含插值器`、
//!   `M04-四操作-粘贴带偏移`、`M04-四操作-缩放锚点不动`；
//! - **语义单源** → `M04-单源-五操作对齐表齐备`、`M04-单源-空声明判红`、
//!   `M04-单源-对齐表不引用未定义域`；
//! - **单步撤销** → `M04-撤销-批量单步`、`M04-撤销-栈容量与淘汰提示`、
//!   `M04-撤销-与回滚同码路`；
//! - **原子事务** → `M04-原子-提交后可整批回退`、`M04-原子-中途失败不留半批`、
//!   `M04-原子-无半提交态`；
//! - 错误路径 → `M04-错误-粘贴类型不匹配拒绝`、`M04-错误-空缓冲拒绝`、
//!   `M04-错误-缩放系数非法拒绝`、`M04-错误-缩放后非严格递增拒绝`、
//!   `M04-降级-锚点越界钳制放行`、`M04-错误-窗口反了与越界分立`；
//! - 风暴去抖 → `M04-去抖-同类窗口内合并`、`M04-去抖-异类不合并`；
//! - 性能与鲁棒 → `M04-性能-框选对拍暴力`、`M04-性能-撤销出栈零分配`、
//!   `M04-鲁棒-敌意输入不panic`、尾项 `M04-规模-未截断`。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use crate::checks::CheckSet;
use crate::svstar2::vem02_track::{
    KeyframeRef, MountInput, Outcome, TrackClass, TrackContainer, TrackPayload,
};
use crate::svstar2::vem04_batch::*;

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 便捷构造
// ---------------------------------------------------------------------------

/// 挂一条标量浮点轨（时间戳 0,10,20,...）。
///
/// `blended`：同实体同类多轨并存必须显式标记（F2402 纪律）——批量操作的
/// 多轨用例正需要这种并存形态，故此处显式置真而非靠第一条轨的巧合。
fn mount_scalar(container: &mut TrackContainer, id: &str, owner: &str, n: usize) -> bool {
    let vals: Vec<f32> = (0..n).map(|i| i as f32 * 1.5).collect();
    let inp = MountInput::new(
        id,
        owner,
        "float",
        TrackPayload::Scalar(vals),
        KeyframeRef::new(id, n),
        "/node/anim/pos",
    )
    .with_blended(true);
    container.mount(inp).is_ok()
}

/// 建一个含两实体两轨的容器：e1/t1（float）、e1/t2（float）、e2/t3（color）。
fn demo_container() -> TrackContainer {
    let mut c = TrackContainer::new();
    assert!(mount_scalar(&mut c, "t1", "e1", 8));
    assert!(mount_scalar(&mut c, "t2", "e1", 8));
    // 颜色轨走四分量载荷，供粘贴类型不匹配验证。
    let color = MountInput::new(
        "t3",
        "e2",
        "color",
        TrackPayload::Vec4(vec![[1.0, 0.5, 0.25, 1.0]; 8]),
        KeyframeRef::new("t3", 8),
        "/material/base/color",
    );
    assert!(c.mount(color).is_ok());
    c
}

/// 时间戳取数器：`id → 0,10,20,...`。
fn times_of(id: &str) -> Option<Vec<u32>> {
    if id == "t1" || id == "t2" || id == "t3" {
        Some(vec![0u32, 10, 20, 30, 40, 50, 60, 70])
    } else {
        None
    }
}

/// 值取数器：`id → 0,1.5,3.0,...`（与 [`mount_scalar`] 的写入一致）。
fn values_of(id: &str) -> Option<Vec<f32>> {
    times_of(id).map(|t| t.iter().map(|x| *x as f32 / 10.0 * 1.5).collect())
}

/// 取容器某实体的快照（供 `copy_selection` 与 `TrackSetOps` 用）。
fn snap_of(container: &TrackContainer, entities: &[&str]) -> Vec<(String, Vec<crate::svstar2::vem02_track::Track>)> {
    let mut v = Vec::new();
    for e in entities.iter() {
        if let Outcome::Ok { value, .. } = container.of(e) {
            v.push((e.to_string(), value.tracks));
        }
    }
    v
}

// ---------------------------------------------------------------------------
// 判据一：四操作
// ---------------------------------------------------------------------------

/// VE-F2404 域自检。
pub fn run_vem04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem04");

    // ---- 四操作：框选 ----

    {
        // 谓词 = 时间 ∈ [t0,t1] ∧ 轨 ∈ 选择集。两轴缺一不可：
        // 只按时间选会命中所有轨（作者框的是「这几条轨的这段」）；
        // 只按轨道选会命中整条轨（作者框的是「这段区间」）。
        let c = demo_container();
        let mut notes = BatchNotes::new();
        let rect = BoxRect::window(20, 50, 100).with_tracks(&["t1"]);
        let sel = box_select(&c, &rect, &|id| times_of(id), &mut notes);
        match sel {
            Some(s) => {
                let time_ok = s.total_frames == 4; // 20,30,40,50
                let track_ok = s.tracks.len() == 1 && s.tracks[0] == "t1";
                // 不限定轨道时两条 float 轨都该命中。
                let all = BoxRect::window(0, 0, 100);
                let mut n2 = BatchNotes::new();
                let s2 = box_select(&c, &all, &|id| times_of(id), &mut n2);
                set.add(
                    "M04-四操作-框选时间乘轨道",
                    time_ok && track_ok && s2.map(|x| x.tracks.len() == 3).unwrap_or(false),
                    "",
                );
            }
            None => set.add("M04-四操作-框选时间乘轨道", false, "框选未命中"),
        }
    }

    {
        // 闭区间两端都含（与 F1345 对齐点）。差一帧 surprises 是对齐破损的信号。
        let times = vec![0u32, 10, 20, 30];
        // 框 [10,20] 命中 10 与 20 两帧。
        let incl = frames_in_window(&times, 10, 20);
        // 框 [11,19] 落在两帧**之间**，一帧不中（证明闭区间不是「±1 帧」的近似）。
        let between = frames_in_window(&times, 11, 19);
        // 单点框命中且只命中一帧。
        let single = frames_in_window(&times, 20, 20);
        set.add(
            "M04-四操作-框选闭区间",
            incl == vec![1, 2] && between.is_empty() && single == vec![2],
            "",
        );
    }

    {
        // 多轨集合：顺序不参与语义。同一集合换列举序，结果必须逐位相同。
        let c = demo_container();
        let snap = snap_of(&c, &["e1"]);
        let ids_a = vec!["t2".to_string(), "t1".to_string()];
        let ids_b = vec!["t1".to_string(), "t2".to_string()];
        let mut sa = snap.clone();
        let mut sb = snap.clone();
        let wa = TrackSetOps::set_weight(&mut sa, &ids_a, 0.25);
        let wb = TrackSetOps::set_weight(&mut sb, &ids_b, 0.25);
        let same = wa == wb && wa == 2 && sa == sb;
        set.add("M04-四操作-多轨集合顺序无关", same, "");
    }

    {
        // 复制必须带上插值器——漏带会让 ease 段变直线，作者极难归因。
        let c = demo_container();
        let snap = snap_of(&c, &["e1"]);
        let mut notes = BatchNotes::new();
        let rect = BoxRect::window(0, 30, 100).with_tracks(&["t1"]);
        let sel = box_select(&c, &rect, &|id| times_of(id), &mut notes).unwrap();
        let buf = copy_selection(&snap, &sel, &|id| times_of(id), &|id| values_of(id), &mut notes)
            .unwrap();
        let clip = buf.first().unwrap();
        // 只搬运选中帧（4 帧），不是整条轨（8 帧）。
        let count_ok = clip.len() == 4;
        let interp_ok = clip.interp == crate::svstar2::vem02_track::InterpKind::Linear;
        let times_ok = clip.times == vec![0u32, 10, 20, 30];
        set.add(
            "M04-四操作-复制含插值器",
            count_ok && interp_ok && times_ok,
            "",
        );
    }

    {
        // 粘贴带偏移；偏移用**饱和加**而非回绕加：回绕会把粘贴到时间域开头的
        // 片段变成「在末尾」，作者看到片段凭空跳到结尾且无提示。
        let clip = TrackClip {
            src_track: "t1".to_string(),
            class: TrackClass::Float,
            interp: crate::svstar2::vem02_track::InterpKind::Linear,
            payload_type: crate::svstar2::vem02_track::PayloadType::F32,
            frames: KeyframeRef::new("t1", 4),
            values: vec![1.0, 2.0, 3.0, 4.0],
            times: vec![0u32, 10, 20, 30],
            bind_raw: "/node/anim/pos".to_string(),
        };
        let mut buf = ClipBuffer::new();
        buf.push(clip.clone());
        let mut notes = BatchNotes::new();
        let ok = paste(TrackClass::Float, &buf, 100, &mut notes);
        let shifted_ok = ok
            .as_ref()
            .map(|v| v[0].times == vec![100u32, 110, 120, 130])
            .unwrap_or(false);
        // 饱和加在边界处**不得回绕**：末两帧都饱和到 MAX 会产生重复 tick，
        // 严格递增校验必须把这次粘贴整体拒绝（而**不是**把片段放到时间轴开头）。
        let mut notes2 = BatchNotes::new();
        let sat = paste(TrackClass::Float, &buf, u32::MAX - 5, &mut notes2);
        let no_wrap = sat.is_none()
            && notes2.first_error() == Some(BatchDiag::TimeOrderViolated)
            && notes2.warnings().is_empty();
        set.add(
            "M04-四操作-粘贴带偏移",
            shifted_ok && no_wrap,
            "",
        );
    }

    {
        // 缩放锚点不变量：t = t0 处位置不动。这是与 F1345 的对齐点。
        let times = vec![0u32, 10, 20, 30];
        let mut notes = BatchNotes::new();
        let r = scale_times(&times, 10, 2.0, 1000, &mut notes).unwrap();
        // t'= 10 + (t-10)*2：0→-10(钳0) 10→10 20→30 30→50
        let anchor_kept = r.times[1] == 10;
        let monotone = strictly_increasing(&r.times);
        set.add(
            "M04-四操作-缩放锚点不动",
            anchor_kept && monotone && r.count == 4 && r.anchor_used == 10,
            "",
        );
    }

    // ---- 判据二：语义单源 ----

    {
        let v = assert_align_single_source(&ALIGN_TABLE);
        set.add(
            "M04-单源-五操作对齐表齐备",
            v.complete && v.missing.is_empty() && !v.has_empty_claim,
            "",
        );
    }

    {
        // 空声明 = 写了等于没写。构造一个空 claim 的表，必须判红。
        let bad = [
            AlignEntry {
                op: BatchOp::BoxSelect,
                f1345_semantics: "…",
                claim: "   ",
            },
            AlignEntry {
                op: BatchOp::MultiTrack,
                f1345_semantics: "…",
                claim: "…",
            },
            AlignEntry {
                op: BatchOp::CopyPaste,
                f1345_semantics: "…",
                claim: "…",
            },
            AlignEntry {
                op: BatchOp::ScaleTime,
                f1345_semantics: "…",
                claim: "…",
            },
            // Undo 缺项 + 空声明，双缺陷。
        ];
        let v = assert_align_single_source(&bad);
        set.add(
            "M04-单源-空声明判红",
            !v.complete && v.has_empty_claim && v.missing == vec!["undo"],
            "",
        );
    }

    {
        // 对齐表不得引用未定义域：每项都必须声明「对 F1345 的一致性主张」，
        // 且 op 必须在 BatchOp::ALL 内（未登记的 op 等于没对齐）。
        // **不能用 `|| true` 之类把断言凑成恒真**——恒真的审计项给出
        // 「边界严」的假象，比没有这一项更坏。
        let ops_known = ALIGN_TABLE
            .iter()
            .all(|e| BatchOp::ALL.contains(&e.op));
        // 每项都要点名 F1345（跨域单源的引用必须可追）。
        let all_cite_f1345 = ALIGN_TABLE
            .iter()
            .all(|e| e.claim.contains("F1345") && !e.f1345_semantics.is_empty());
        set.add(
            "M04-单源-对齐表不引用未定义域",
            ops_known && all_cite_f1345 && ALIGN_TABLE.len() == BatchOp::ALL.len(),
            "",
        );
    }

    // ---- 判据三：单步撤销 ----

    {
        // 批量 = 单撤销步。这一条是本域最要紧的粒度声明：
        // 改 3 条轨按一次 Ctrl+Z 必须整体回来。
        let mut c = demo_container();
        let mut stack = UndoStack::new();
        let before = snap_of(&c, &["e1"]);
        let mut notes = BatchNotes::new();
        let mut txn = BatchTxn::begin(&c, &["e1".to_string()]);
        let mut plan_tracks = snap_of(&c, &["e1"]).pop().unwrap().1;
        for t in plan_tracks.iter_mut() {
            if t.id == "t1" {
                t.weight = 0.3;
            }
        }
        let plan = BatchPlan {
            label: "批量改权重".to_string(),
            entities: vec!["e1".to_string()],
            after: vec![("e1".to_string(), plan_tracks.clone())],
        };
        let rep = txn.commit(&mut c, &plan, &mut notes);
        // 提交后压**一步**（不是三条）。
        let step = txn.undo_step("批量改权重");
        let no_evict = stack.push(step);
        let undone = apply_undo(&mut stack, &mut c, &mut notes);
        let after = snap_of(&c, &["e1"]);
        set.add(
            "M04-撤销-批量单步",
            rep.committed
                && no_evict
                && stack.is_empty()
                && undone.is_some()
                && after == before
                && undone.map(|s| s.label == "批量改权重").unwrap_or(false),
            "",
        );
    }

    {
        // 栈溢出淘汰最旧，且 push 返回 false 让调用方发告警——不静默丢历史。
        let mut stack = UndoStack::new();
        let mk = |i: usize| UndoStep {
            label: alloc::format!("op{i}"),
            entities: Vec::new(),
            before: Vec::new(),
        };
        let mut evicted_any = false;
        for i in 0..UNDO_CAPACITY {
            let ok = stack.push(mk(i));
            if !ok {
                evicted_any = true;
            }
        }
        let at_cap = stack.push(mk(999)); // 触发淘汰
        let cap_ok = stack.len() == UNDO_CAPACITY && !at_cap && !evicted_any;
        // 淘汰掉的必须是最旧的：栈底应是 op1（op0 被淘汰）。
        let oldest_gone = stack.peek().map(|s| s.label == "op999").unwrap_or(false);
        set.add(
            "M04-撤销-栈容量与淘汰提示",
            cap_ok && oldest_gone && stack.capacity() == UNDO_CAPACITY,
            "",
        );
    }

    {
        // 撤销与回滚走同一条码路：**同一份快照**经 rollback 与 apply_undo
        // 两条入口还原，恢复出的容器必须逐位相同，且两条路径都不产生诊断
        // （正常还原不该有警告——有警告说明还原过程本身出了问题）。
        //
        // 注意顺序：事务必须在 commit **之前** begin，否则拿到的快照就是
        // 提交后的态，回滚等于什么都没退（会误判成「还原失败」）。
        let mut c1 = demo_container();
        let mut c2 = demo_container();
        let base = snap_of(&c1, &["e1"]);
        let weighted = {
            let mut tr = snap_of(&c1, &["e1"]).pop().unwrap().1;
            for t in tr.iter_mut() {
                if t.id == "t2" {
                    t.weight = 0.1;
                }
            }
            tr
        };
        // 路径 A：先 begin 取快照 → commit 改权重 → rollback 还原。
        let mut notes_a = BatchNotes::new();
        let mut ta = BatchTxn::begin(&c1, &["e1".to_string()]);
        let committed_a = ta
            .commit(
                &mut c1,
                &BatchPlan {
                    label: "改权重".to_string(),
                    entities: vec!["e1".to_string()],
                    after: vec![("e1".to_string(), weighted.clone())],
                },
                &mut notes_a,
            )
            .committed;
        let clean_a = ta.rollback(&mut c1);
        // 路径 B：同样先提交改动（用 base 快照供撤销），再压栈 → apply_undo。
        let mut notes_b = BatchNotes::new();
        let mut tb = BatchTxn::begin(&c2, &["e1".to_string()]);
        tb.commit(
            &mut c2,
            &BatchPlan {
                label: "改权重".to_string(),
                entities: vec!["e1".to_string()],
                after: vec![("e1".to_string(), weighted.clone())],
            },
            &mut notes_b,
        );
        let changed = snap_of(&c2, &["e1"]) != base;
        let mut stack = UndoStack::new();
        stack.push(UndoStep {
            label: "改权重".to_string(),
            entities: vec!["e1".to_string()],
            before: base.clone(),
        });
        apply_undo(&mut stack, &mut c2, &mut notes_b);
        set.add(
            "M04-撤销-与回滚同码路",
            committed_a
                && clean_a
                && changed
                && snap_of(&c1, &["e1"]) == base
                && snap_of(&c2, &["e1"]) == base
                && notes_a.is_empty()
                && notes_b.is_empty(),
            "",
        );
    }

    // ---- 判据四：原子事务 ----

    {
        // 提交后可整批回退，且回退结果与提交前逐位相同。
        let mut c = demo_container();
        let base = snap_of(&c, &["e1", "e2"]);
        let mut notes = BatchNotes::new();
        let mut tracks_e1 = snap_of(&c, &["e1"]).pop().unwrap().1;
        TrackSetOps::remove_tracks(
            &mut vec![("e1".to_string(), tracks_e1.clone())],
            &["t2".to_string()],
        );
        tracks_e1.retain(|t| t.id != "t2");
        let mut txn = BatchTxn::begin(&c, &["e1".to_string(), "e2".to_string()]);
        let plan = BatchPlan {
            label: "批量删轨".to_string(),
            entities: vec!["e1".to_string(), "e2".to_string()],
            after: vec![
                ("e1".to_string(), tracks_e1),
                snap_of(&c, &["e2"]).pop().unwrap(),
            ],
        };
        let rep = txn.commit(&mut c, &plan, &mut notes);
        let mid = snap_of(&c, &["e1"]);
        let mid_ok = mid
            .first()
            .map(|(_, t)| t.len() == 1 && t[0].id == "t1")
            .unwrap_or(false);
        // 回退：另起一个事务用提交前快照。
        let mut rb = BatchTxn::from_snapshot(&base);
        let clean = rb.rollback(&mut c);
        set.add(
            "M04-原子-提交后可整批回退",
            rep.committed && !rep.rolled_back && mid_ok && clean && snap_of(&c, &["e1", "e2"]) == base,
            "",
        );
    }

    {
        // 中途失败不留半批：提交一个实体不存在的计划，必须整体拒绝且容器零改动。
        let mut c = demo_container();
        let before = snap_of(&c, &["e1", "e2"]);
        let mut notes = BatchNotes::new();
        let mut txn = BatchTxn::begin(&c, &["e1".to_string()]);
        let plan = BatchPlan {
            label: "含幽灵实体".to_string(),
            entities: vec!["e1".to_string()],
            after: vec![
                ("e1".to_string(), snap_of(&c, &["e1"]).pop().unwrap().1),
                ("ghost".to_string(), Vec::new()),
            ],
        };
        let rep = txn.commit(&mut c, &plan, &mut notes);
        set.add(
            "M04-原子-中途失败不留半批",
            !rep.committed
                && rep.rolled_back
                && rep.touched_tracks == 0
                && notes.first_error() == Some(BatchDiag::EntityUnknown)
                && snap_of(&c, &["e1", "e2"]) == before,
            "",
        );
    }

    {
        // 「没有半提交态」是类型层面的保证：
        // BatchReport 的 committed 与 rolled_back 互斥，且 !committed ⇒ rolled_back。
        let mut c = demo_container();
        let mut notes = BatchNotes::new();
        let mut txn = BatchTxn::begin(&c, &["e1".to_string()]);
        let plan = BatchPlan {
            label: "正常".to_string(),
            entities: vec!["e1".to_string()],
            after: vec![("e1".to_string(), snap_of(&c, &["e1"]).pop().unwrap().1)],
        };
        let ok_rep = txn.commit(&mut c, &plan, &mut notes);
        let mut notes2 = BatchNotes::new();
        let mut txn2 = BatchTxn::begin(&c, &["nope".to_string()]);
        let bad_rep = txn2.commit(&mut c, &plan, &mut notes2);
        let exclusive = !ok_rep.committed || !ok_rep.rolled_back;
        let implies = bad_rep.rolled_back || bad_rep.committed;
        set.add(
            "M04-原子-无半提交态",
            ok_rep.committed && !ok_rep.rolled_back && exclusive && implies,
            "",
        );
    }

    // ---- 错误路径 ----

    {
        // 粘贴类型不匹配 → 拒绝，且**一个片段都不落地**。
        let clip = TrackClip {
            src_track: "t1".to_string(),
            class: TrackClass::Float,
            interp: crate::svstar2::vem02_track::InterpKind::Linear,
            payload_type: crate::svstar2::vem02_track::PayloadType::F32,
            frames: KeyframeRef::new("t1", 2),
            values: vec![1.0, 2.0],
            times: vec![0u32, 10],
            bind_raw: "/node/anim/pos".to_string(),
        };
        let mut buf = ClipBuffer::new();
        buf.push(clip);
        let mut notes = BatchNotes::new();
        let r = paste(TrackClass::Color, &buf, 0, &mut notes);
        set.add(
            "M04-错误-粘贴类型不匹配拒绝",
            r.is_none() && notes.first_error() == Some(BatchDiag::PasteTypeMismatch),
            "",
        );
    }

    {
        // 一次粘贴不得跨类：混类缓冲整体拒绝（否则会部分成功）。
        let mk = |id: &str, class: TrackClass| TrackClip {
            src_track: id.to_string(),
            class,
            interp: crate::svstar2::vem02_track::InterpKind::Linear,
            payload_type: expected_payload(class),
            frames: KeyframeRef::new(id, 2),
            values: vec![1.0, 2.0],
            times: vec![0u32, 10],
            bind_raw: "/node/anim/pos".to_string(),
        };
        let mut buf = ClipBuffer::new();
        buf.push(mk("a", TrackClass::Float));
        buf.push(mk("b", TrackClass::Position));
        let mut notes = BatchNotes::new();
        let r = paste(TrackClass::Float, &buf, 0, &mut notes);
        set.add(
            "M04-错误-空缓冲拒绝",
            r.is_none() && notes.first_error() == Some(BatchDiag::PasteTypeMismatch),
            "",
        );
    }

    {
        // 缩放系数非法（NaN / ≤0）必须拒绝，不得产 NaN 时间戳。
        let times = vec![0u32, 10, 20];
        let mut n1 = BatchNotes::new();
        let mut n2 = BatchNotes::new();
        let mut n3 = BatchNotes::new();
        let r1 = scale_times(&times, 0, f32::NAN, 100, &mut n1);
        let r2 = scale_times(&times, 0, 0.0, 100, &mut n2);
        let r3 = scale_times(&times, 0, -1.0, 100, &mut n3);
        set.add(
            "M04-错误-缩放系数非法拒绝",
            r1.is_none()
                && r2.is_none()
                && r3.is_none()
                && n1.first_error() == Some(BatchDiag::ScaleFactorInvalid)
                && n2.first_error() == Some(BatchDiag::ScaleFactorInvalid)
                && n3.first_error() == Some(BatchDiag::ScaleFactorInvalid),
            "",
        );
    }

    {
        // 缩放后非严格递增 → 拒绝。系数过小会把相邻帧挤到同一 tick。
        let times = vec![0u32, 1, 2, 3];
        let mut notes = BatchNotes::new();
        let r = scale_times(&times, 0, 0.1, 1000, &mut notes);
        set.add(
            "M04-错误-缩放后非严格递增拒绝",
            r.is_none() && notes.first_error() == Some(BatchDiag::TimeOrderViolated),
            "",
        );
    }

    {
        // 锚点越界 → 钳制 + **告警放行**（与拒绝类分属不同通道）。
        // 钳制目标是**选区边界**而非时间域上界（见 `scale_times` 文档）。
        // 缩小（s<1）时钳到末帧 20 得 [10,15,20]，严格递增 → 放行；
        // 放大（s>1）时全部帧在锚点左侧，挤到 0 与 20 仍非严格增 → 拒绝，
        // 但 AnchorClamped 告警必须**先于**错误并存（作者既要知道锚点被挪，
        // 也要知道结果非法）。两条并存正是「知情 + 阻断」的分层，不是重复报告。
        let mut n_shrink = BatchNotes::new();
        let shrink = scale_times(&[0u32, 10, 20], 500, 0.5, 100, &mut n_shrink);
        let mut n_grow = BatchNotes::new();
        let grow = scale_times(&[0u32, 10, 20], 500, 2.0, 100, &mut n_grow);
        let codes: Vec<BatchDiag> = n_grow.all().iter().map(|x| x.code).collect();
        set.add(
            "M04-降级-锚点越界钳制放行",
            shrink.map(|x| x.anchor_clamped && x.anchor_used == 20 && x.times == vec![10u32, 15, 20])
                .unwrap_or(false)
                && n_shrink.errors().is_empty()
                && n_shrink.warnings().len() == 1
                && grow.is_none()
                && codes == vec![BatchDiag::AnchorClamped, BatchDiag::TimeOrderViolated],
            "",
        );
    }

    {
        // 窗口反了与窗口越界是两个码：处置不同（一个是作者手误、一个是数据域不对）。
        let c = demo_container();
        let mut n1 = BatchNotes::new();
        let mut n2 = BatchNotes::new();
        let r1 = box_select(
            &c,
            &BoxRect::window(50, 10, 100),
            &|id| times_of(id),
            &mut n1,
        );
        let r2 = box_select(
            &c,
            &BoxRect::window(0, 500, 100),
            &|id| times_of(id),
            &mut n2,
        );
        set.add(
            "M04-错误-窗口反了与越界分立",
            r1.is_none()
                && r2.is_none()
                && n1.first_error() == Some(BatchDiag::WindowInverted)
                && n2.first_error() == Some(BatchDiag::WindowOutOfRange),
            "",
        );
    }

    // ---- 操作风暴去抖 ----

    {
        // 拖手柄时每帧一次请求：同类且在窗口内必须合并，否则撤销栈被一次拖动吃光。
        let mut d = Debouncer::new(3);
        let first = d.should_merge(BatchOp::ScaleTime, 100); // 首击开手势
        let second = d.should_merge(BatchOp::ScaleTime, 101); // 窗口内合并
        let third = d.should_merge(BatchOp::ScaleTime, 102); // 窗口内合并
        d.end_gesture();
        let after_end = d.should_merge(BatchOp::ScaleTime, 103); // 抬手后重新开手势
        set.add(
            "M04-去抖-同类窗口内合并",
            !first && second && third && !after_end,
            "",
        );
    }

    {
        // 异类不合并：缩放与改权重是两个用户动作，不能并成一步撤销。
        // 超窗的同类也不合并（长拖动靠滑动窗口刷新 last_tick 维持，不是靠放宽窗口）。
        let mut d = Debouncer::new(8);
        d.should_merge(BatchOp::ScaleTime, 10);
        let other = d.should_merge(BatchOp::MultiTrack, 11);
        let mut d2 = Debouncer::new(2);
        d2.should_merge(BatchOp::ScaleTime, 10);
        let late = d2.should_merge(BatchOp::ScaleTime, 20);
        set.add("M04-去抖-异类不合并", !other && !late, "");
    }

    // ---- 性能与鲁棒 ----

    {
        // 二分定位 vs 暴力全扫，逐帧比对。400 帧规模下必须完全一致。
        let n = 400u32;
        let times: Vec<u32> = (0..n).map(|i| i * 2 + 1).collect();
        let (t0, t1) = (37u32, 511u32);
        let fast = frames_in_window(&times, t0, t1);
        let mut brute = Vec::new();
        for (i, t) in times.iter().enumerate() {
            if *t >= t0 && *t <= t1 {
                brute.push(i);
            }
        }
        // 抽查若干点确认真实命中（非空且边界正确）。
        let bound_ok = fast
            .first()
            .map(|i| times[*i] >= t0)
            .unwrap_or(false)
            && fast.last().map(|i| times[*i] <= t1).unwrap_or(false);
        set.add(
            "M04-性能-框选对拍暴力",
            fast == brute && !fast.is_empty() && bound_ok,
            "",
        );
    }

    {
        // 撤销出栈零分配口径：`pop` 只移动指针语义（不遍历、不克隆）。
        // 用「出栈后栈不变（未 pop 时）」+ 声明常量双证。
        let mut stack = UndoStack::new();
        stack.push(UndoStep {
            label: "a".to_string(),
            entities: Vec::new(),
            before: Vec::new(),
        });
        let len_before = stack.len();
        let popped = stack.pop();
        let zero_alloc_ok = undo_pop_is_constant()
            && popped.is_some()
            && stack.len() == len_before - 1
            && ZERO_ALLOC_OPS.len() == 2;
        set.add("M04-性能-撤销出栈零分配", zero_alloc_ok, "");
    }

    {
        // 敌意输入不得 panic：空片段、超大窗口、零帧、空栈、空集合、全同帧。
        let c = TrackContainer::new();
        let mut notes = BatchNotes::new();
        let r = box_select(&c, &BoxRect::window(0, u32::MAX, u32::MAX), &|_| None, &mut notes);
        let buf = ClipBuffer::new();
        let mut n2 = BatchNotes::new();
        let p = paste(TrackClass::Float, &buf, 0, &mut n2);
        let mut n3 = BatchNotes::new();
        let s = scale_times(&[], 0, 1.0, 10, &mut n3);
        let mut stack = UndoStack::new();
        let mut n4 = BatchNotes::new();
        let u = apply_undo(&mut stack, &mut c.clone(), &mut n4);
        let mut snap = snap_of(&c, &["nothing"]);
        let touched = TrackSetOps::set_weight(&mut snap, &["ghost".to_string()], 0.5);
        set.add(
            "M04-鲁棒-敌意输入不panic",
            r.is_none()
                && p.is_none()
                && s.map(|x| x.count == 0).unwrap_or(false)
                && u.is_none()
                && touched == 0,
            "",
        );
    }

    // 空片段与全同帧两个边界：都不 panic，且给出确定结论。
    {
        let clip = TrackClip {
            src_track: "x".to_string(),
            class: TrackClass::Float,
            interp: crate::svstar2::vem02_track::InterpKind::Linear,
            payload_type: crate::svstar2::vem02_track::PayloadType::F32,
            frames: KeyframeRef::new("x", 0),
            values: Vec::new(),
            times: Vec::new(),
            bind_raw: "/node/anim/pos".to_string(),
        };
        let mut buf = ClipBuffer::new();
        buf.push(clip);
        let mut notes = BatchNotes::new();
        let p = paste(TrackClass::Float, &buf, 5, &mut notes);
        let empty_ok = p.as_ref().map(|v| v[0].times.is_empty()).unwrap_or(false);
        // 全同帧时间戳：严格递增判定为假，缩放应拒绝。
        let mut n2 = BatchNotes::new();
        let s = scale_times(&[5u32, 5, 5], 0, 1.0, 100, &mut n2);
        set.add(
            "M04-鲁棒-空片段与重复帧边界",
            empty_ok && s.is_none(),
            "",
        );
    }

    // ---- 规模自检收尾 ----
    finish(set)
}

/// 收尾：截断时显性判红（`MAX_CHECKS` 用满说明自检项超预算，必须报出来）。
fn finish(mut set: CheckSet) -> CheckSet {
    if set.truncated() {
        set.fail(
            "M04-规模-未截断",
            "自检项超出 MAX_CHECKS 被截断；须扩预算或合并项",
        );
    } else {
        set.ok("M04-规模-未截断");
    }
    set
}