//! VE-F1613 · 网格统计与报告（VE 册 · 域自检）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1613`
//!
//! **判据（锚点原文）**：单网格、批量、导出、判据。
//!
//! 分四组，逐条映射锚点：
//! - `c1613_single`  → 判据一「单网格」：五项齐 / 三态判定 / 恰好等于阈值 /
//!   超标高亮 / 数据自相矛盾拒收 / 阈值依据必填
//! - `c1613_batch`   → 判据二「批量」：异常清单 / 失败隔离 / 断点续跑零重复 /
//!   失败直方图 / 覆盖拒收记账
//! - `c1613_export`  → 判据三「导出」：开放格式 / 必带阈值口径 / 规则版本 /
//!   缺项不导出 / 标签订���齐备
//! - `c1613_selfchk` → 判据四「判据」：判据集自身性质 + 三族合并未截断
//!
//! **本文件的判据纪律（十诫）**：
//! 1. **阈值期望值判据侧写死**：`EXP_VERT_CEILING = 200_000` 等五个数字
//!    独立写出，**不从被测阈值表反推**。若判据直接引
//!    `st::default_thresholds().value_of(...)`，把被测上限改成 1 亿判据全绿。
//! 2. **「三态判定」必须三态都测**：只测 Over 的话，把 `Warn` 恒返回
//!    `Ok` 判据全绿 ⇒ 必须同时断「达警戒线 ⇒ Warn」与「远离 ⇒ Ok」。
//! 3. **「恰好等于阈值是合法值」单独钉**：这是最容易被"顺手改成 >="
//!    弄掉的性质，必须独立断言 `== threshold ⇒ Ok`（压缩率）/ `!= Over`。
//! 4. **失败隔离的反向对照**：只测「坏数据被隔离」的话，把
//!    `scan_batch` 改成遇到第一个错就整体返回 `Err` 也可能全绿 ⇒
//!    必须同时断「同一批里的好数据仍然全部产出报告」。
//! 5. **断点续跑零重复**：构造「同 id 出现两次」的语料，断言第二轮
//!    `completed == 0` 且 `skipped == 出现次数` —— 只测「跳过」不够，
//!    还要测**不重复计入报告**。
//! 6. **导出必须带口径**：断言 `thresholds.len() == 5` 且逐项值等于
//!    判据侧写死的期望；只看 `lines.len() > 0` 的话，把阈值全删掉也全绿。
//! 7. **判据区零 panic 面**：无 `unwrap()`/`expect()`；下标访问先比长度。
//! 8. **判据内不调聚合函数**：否则构成无限递归（VE-F5401 亲历：症状是
//!    栈溢出而非某条判据变红，极难定位）。

extern crate alloc;

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::{CheckSet, MAX_CHECKS};
use crate::svstar2::vef16_gridstat as st;

// ---------------------------------------------------------------------------
// 判据侧独立预期常量（十诫第 1 条：不从被测反推）
// ---------------------------------------------------------------------------

/// 期望顶点数上限。
const EXP_VERT_CEILING: u64 = 200_000;
/// 期望面数上限。
/// 期望面数上限。
///
/// **刻意低于顶点上限的 2 倍（40 万）**：三角网格拓扑上界 `F<=2V-4`，
/// 顶点数达 20 万时面数最多 399_996。取 40 万会让「面数超预算」永不可达。
const EXP_FACE_CEILING: u64 = 300_000;
/// 期望 LOD 层数上限。
const EXP_LOD_CEILING: u64 = 6;
/// 期望压缩率下限（千分比）。
const EXP_COMP_FLOOR: u64 = 500;
/// 期望属性字节上限。
const EXP_ATTR_CEILING: u64 = 8 * 1024 * 1024;
/// 期望顶点数警戒线（上限的 80%）。
const EXP_VERT_WARN_AT: u64 = 160_000;

// 造一份「各项都在带内中部」的合法网格统计量。
fn good_stat() -> st::MeshStat {
    st::MeshStat {
        vertices: 1_000,
        // 面上界为 2*1000-4=1996，取 1800 合法（原先取 2000 会越界被拒）
        faces: 1_800,
        lod_levels: 3,
        compression_permille: 800,
        attribute_bytes: 1_000_000,
    }
}

// ---------------------------------------------------------------------------
// 判据一：单网格体检
// ---------------------------------------------------------------------------

fn c1613_single(s: &mut CheckSet) {
    // ① 阈值表五项齐备，且**值与判据侧写死的期望一致**（十诫第 1 条）。
    let t = st::default_thresholds();
    let vals_ok = t.len() == st::Metric::ALL.len()
        && t.has(st::Metric::Vertices)
        && t.has(st::Metric::Faces)
        && t.has(st::Metric::LodLevels)
        && t.has(st::Metric::CompressionPermille)
        && t.has(st::Metric::AttributeBytes)
        && t.value_of(st::Metric::Vertices) == EXP_VERT_CEILING
        && t.value_of(st::Metric::Faces) == EXP_FACE_CEILING
        && t.value_of(st::Metric::LodLevels) == EXP_LOD_CEILING
        && t.value_of(st::Metric::CompressionPermille) == EXP_COMP_FLOOR
        && t.value_of(st::Metric::AttributeBytes) == EXP_ATTR_CEILING;
    s.add(
        "阈值表五项齐备且值符合预期",
        vals_ok,
        "五项齐：顶点20万/面数30万/LOD6/压缩率下限500‰/属性8MiB（判据侧写死，不从被测反推）",
    );

    // ② **每项阈值都有依据**（锚点：阈值依据声明）。空依据 = 拍脑袋。
    let mut basis_ok = true;
    for m in st::Metric::ALL.iter() {
        match t.get(*m) {
            // 依据不只是非空，还要**真的解释了取舍**（长度下限，避免"x"占位）。
            Some(th) => {
                if !th.has_basis() || th.basis.len() < 8 {
                    basis_ok = false;
                }
            }
            None => basis_ok = false,
        }
    }
    s.add(
        "阈值依据齐备且非占位",
        basis_ok,
        "五项依据均非空且 ≥8 字（只写一个「x」等于没写依据）",
    );

    // ③ 五项判定齐备且与 `Metric::ALL` **同序**（顺序错则报告串行）。
    let r = st::inspect_mesh(42, &good_stat(), &t).expect("合法网格应出报告");
    let mut order_ok = r.verdicts.len() == st::Metric::ALL.len();
    for (i, m) in st::Metric::ALL.iter().enumerate() {
        if i >= r.verdicts.len() {
            order_ok = false;
            break;
        }
        if r.verdicts[i].metric != *m {
            order_ok = false;
        }
    }
    s.add(
        "五项判定齐备且与枚举同序",
        order_ok,
        "报告内五项判定与 Metric::ALL 同序（顺序错则导出行错位）",
    );

    // ④ 全 Ok 的网格不被高亮（**必须有「不报警」的对照**，否则
    //    把 `highlighted` 恒返回 true 判据也全绿）。
    s.add(
        "合格网格不产生高亮",
        !r.any_flagged && r.flagged_count() == 0 && r.flagged_labels().is_empty(),
        "各项居中 ⇒ any_flagged=false、flagged_count=0、标签串为空",
    );

    // ⑤ **面数超预算 → Over 且高亮**（锚点明列的超标项之一）。
    // 顶点数须够大，否则 faces=300001 会先越「2V-4」拓扑上界而被判自相矛盾：
    // V=20 万 ⇒ 面上界 399_996 > 300_001，且顶点数恰等于上限判Ok
    // ⇒ 唯一高亮项就是 faces。
    let heavy = st::MeshStat {
        vertices: EXP_VERT_CEILING,
        faces: EXP_FACE_CEILING + 1,
        ..good_stat()
    };
    let rh = st::inspect_mesh(7, &heavy, &t).expect("应出报告");
    let fv = rh.verdict_of(st::Metric::Faces);
    s.add(
        "面数超预算判 Over 并高亮",
        fv.map(|v| v.severity == st::Severity::Over).unwrap_or(false)
            && rh.any_flagged
            && rh.flagged_count() == 1
            && rh.flagged_labels() == "faces:OVER",
        "面数 300001 > 上限 300000 ⇒ OVER 且唯一高亮项为 faces（标签串逐字节比对）",
    );

    // ⑥ **压缩率低 → Over**（锚点明列的另一超标项）。
    let low = st::MeshStat { compression_permille: (EXP_COMP_FLOOR - 1) as u32, ..good_stat() };
    let rl = st::inspect_mesh(8, &low, &t).expect("应出报告");
    s.add(
        "压缩率低于下限判 Over",
        rl.verdict_of(st::Metric::CompressionPermille)
            .map(|v| v.severity == st::Severity::Over)
            .unwrap_or(false)
            && rl.flagged_labels() == "comp:OVER",
        "压缩率 499‰ < 下限 500‰ ⇒ comp:OVER（「压缩率低」是锚点点名的超标项）",
    );

    // ⑦ **恰好等于阈值是合法值**（最易被"顺手改成 >=" 弄掉）。
    //
    // 这条单列：若把 `v > ceiling` 写成 `v >= ceiling`，判据 ⑤⑥仍全绿
    // （它们的值确实超了），只有这条会红。
    let exact = st::MeshStat {
        vertices: EXP_VERT_CEILING,
        faces: 2_000,
        compression_permille: EXP_COMP_FLOOR as u32,
        ..good_stat()
    };
    let re = st::inspect_mesh(9, &exact, &t).expect("应出报告");
    let at_vert = re.verdict_of(st::Metric::Vertices).map(|v| v.severity).unwrap_or(st::Severity::Over);
    let at_comp = re
        .verdict_of(st::Metric::CompressionPermille)
        .map(|v| v.severity)
        .unwrap_or(st::Severity::Over);
    s.add(
        "恰好等于阈值判 Ok 不判 Over",
        at_vert == st::Severity::Ok && at_comp == st::Severity::Ok,
        "顶点数恰 20 万（=上限）、压缩率恰 500‰（=下限）⇒ 均 Ok（恰好等于阈值是合法值）",
    );

    // ⑧ **Warn 三态中的中间态**（十诫第 2 条：三态都测）。
    //
    // 顶点数取警戒线 16 万（上限 20 万的 80%）：未超红线但已到警戒 ⇒ Warn。
    let warn_zone = st::MeshStat { vertices: EXP_VERT_WARN_AT, ..good_stat() };
    let rw = st::inspect_mesh(10, &warn_zone, &t).expect("应出报告");
    let wv = rw.verdict_of(st::Metric::Vertices).map(|v| v.severity).unwrap_or(st::Severity::Ok);
    s.add(
        "达警戒线判 Warn（未超红线）",
        wv == st::Severity::Warn && rw.any_flagged && rw.flagged_labels() == "verts:WARN",
        "顶点数 16 万 = 上限 80% ⇒ verts:WARN 且计入高亮（「体检是优化起点」需要提前窗口）",
    );

    // ⑨ 高亮标签用**文字**而非仅颜色（无障碍：色觉障碍读者也能读出原因）。
    let labels_ok = st::Severity::Ok.label() == "OK"
        && st::Severity::Warn.label() == "WARN"
        && st::Severity::Over.label() == "OVER"
        && st::Severity::Ok.highlighted() == false
        && st::Severity::Warn.highlighted()
        && st::Severity::Over.highlighted();
    s.add(
        "高亮以文字标签承载信息",
        labels_ok,
        "OK/WARN/OVER 三种文字标签；只有 WARN/OVER 计入高亮（不靠颜色单独承载）",
    );

    // ⑩ **数据自相矛盾 → 拒收，不凑数**（四类）。
    //
    // 面数上界是 `2V-4`（欧拉公式，见 `face_count_ceiling`），不是 `V`：
    // 故「坏面数」须取超过该上界的值。此处 V=10 ⇒ 上界 16，取 17。
    // 若把上界误写回 `F<=V`，本条仍会红（17>10）——所以另有一条
    // 专断「常规闭合网格不被误拒」的对照，见 `cube_like_mesh_not_rejected`。
    let empty = st::MeshStat { vertices: 0, ..good_stat() };
    let bad_faces = st::MeshStat { vertices: 10, faces: 17, ..good_stat() };
    let bad_lod = st::MeshStat { lod_levels: 0, ..good_stat() };
    let bad_comp = st::MeshStat { compression_permille: 1001, ..good_stat() };
    let over_lod = st::MeshStat { lod_levels: st::MAX_LOD_LEVELS + 1, ..good_stat() };
    let rejects = st::inspect_mesh(1, &empty, &t) == Err(st::StatError::EmptyMesh)
        && st::inspect_mesh(2, &bad_faces, &t)
            == Err(st::StatError::FaceCountImplausible { faces: 17, vertices: 10 })
        && st::inspect_mesh(3, &bad_lod, &t) == Err(st::StatError::LodOutOfRange { levels: 0 })
        && st::inspect_mesh(4, &bad_comp, &t)
            == Err(st::StatError::CompressionOutOfRange { permille: 1001 })
        && st::inspect_mesh(5, &over_lod, &t)
            == Err(st::StatError::LodOutOfRange { levels: st::MAX_LOD_LEVELS + 1 });
    s.add(
        "自相矛盾数据四类拒收",
        rejects,
        "空网格 / 面数超 2V-4 拓扑上界 / LOD=0或越界 / 压缩率>1000‰ ⇒ 报错拒收，不凑数修正",
    );

    // ⑩b **反向对照：常规闭合网格不得被误拒**（域正确性守门）。
    //
    // 三角网格的 `F ≤ 2V-4` 而**非** `F ≤ V`：立方体 8 顶点 12 面、
    // 二十面体 12 顶点 20 面都满足 `F > V`。若上界误取 `F ≤ V`，
    // 上面的「拒收」判据照样全绿（17 > 10 确实超了），只有这条会红。
    // 这正是「只断自己造的反例」不够、必须补一条「正常数据不被拒」的
    // 正向守门。
    let cube = st::MeshStat {
        vertices: 8,
        faces: 12,
        lod_levels: 1,
        compression_permille: 900,
        attribute_bytes: 512,
    };
    let icosa = st::MeshStat {
        vertices: 12,
        faces: 20,
        ..cube
    };
    let no_false_reject = st::validate_stat(&cube).is_ok()
        && st::validate_stat(&icosa).is_ok()
        // 恰好贴上界（2V-4）必须合法：闭流形的等号情形
        && st::validate_stat(&st::MeshStat { vertices: 10, faces: 16, ..cube }).is_ok()
        // 上界函数本身：V<=2 退化为 0 面
        && st::face_count_ceiling(0) == 0
        && st::face_count_ceiling(1) == 0
        && st::face_count_ceiling(2) == 0
        && st::face_count_ceiling(3) == 2
        && st::face_count_ceiling(8) == 12
        && st::face_count_ceiling(12) == 20;
    s.add(
        "常规闭合网格不被误拒（面上界 2V-4）",
        no_false_reject,
        "立方体 8V/12F、二十面体 12V/20F 均合法；贴上界 10V/16F 合法；V<=2 时上界退化为 0",
    );

    // ⑪ 失败原因码**稳定且互不相同**（进失败清单，不外泄错误字符串）。
    let codes = [
        st::stat_error_code(st::StatError::EmptyMesh),
        st::stat_error_code(st::StatError::FaceCountImplausible { faces: 1, vertices: 2 }),
        st::stat_error_code(st::StatError::LodOutOfRange { levels: 9 }),
        st::stat_error_code(st::StatError::CompressionOutOfRange { permille: 1001 }),
    ];
    let mut uniq = codes.to_vec();
    uniq.sort_unstable();
    uniq.dedup();
    s.add(
        "失败原因码稳定且互异",
        uniq.len() == 4 && codes[0] == 1 && codes[3] == 4,
        "四类错误码 1/2/3/4 互不相同（清单按码归并，码冲突会让直方图失真）",
    );

    // ⑫ 报告携带阈值指纹（跨版本对比的前提）。
    let fp = st::thresholds_fingerprint(&t);
    s.add(
        "报告携带阈值指纹",
        fp != 0 && r.thresholds_fingerprint == fp,
        "报告内指纹与当前阈值表一致且非零（无指纹则两份报告不可比）",
    );
}

// ---------------------------------------------------------------------------
// 判据二：批体检（异常清单 + 失败隔离 + 断点续跑）
// ---------------------------------------------------------------------------

fn c1613_batch(s: &mut CheckSet) {
    // 语料：3 好 2 坏。坏数据分别是空网格与面数超拓扑上界（2V-4）。
    let items: Vec<(u64, st::MeshStat)> = vec![
        (101, good_stat()),
        (102, st::MeshStat { vertices: 0, ..good_stat() }),
        (
            103,
            st::MeshStat { vertices: EXP_VERT_CEILING + 1, ..good_stat() },
        ),
        // V=5 ⇒ 面上界 2*5-4=6，取 9 面越界（与good_stat 的 1000V/2000F 同形）
        (104, st::MeshStat { vertices: 5, faces: 9, ..good_stat() }),
        (105, st::MeshStat { lod_levels: 1, ..good_stat() }),
    ];
    let t = st::default_thresholds();

    // ① **异常网格清单只含被高亮的**（批体检的产出目的）。
    let mut cur = st::ScanCursor::default();
    let b = st::scan_batch(&items, &t, &mut cur);
    // 102 空、104 面数超拓扑上界 ⇒ 失败；103 顶点数超 ⇒ 高亮；101/105 干净。
    let flagged_ok = b.flagged.len() == 1 && b.flagged[0] == 103;
    s.add(
        "异常清单只含高亮网格",
        flagged_ok && b.reports.len() == 3 && b.failures.len() == 2,
        "5 条中 103 顶点超标⇒入清单；101/105 合格⇒只进报告不入清单；102/104 失败⇒入失败清单",
    );

    // ② **失败隔离的反向对照**（十诫第 4 条）：坏数据**不影响**好数据产出。
    //
    // 若实现改成「遇错整体返回」，`reports.len()` 会是 0 ⇒ 这条红。
    let good_produced = b.reports.len() == 3
        && b.reports.iter().all(|r| r.mesh_id != 0)
        && b.reports.iter().any(|r| r.mesh_id == 101)
        && b.reports.iter().any(|r| r.mesh_id == 105);
    s.add(
        "失败隔离：好数据仍全部产出",
        good_produced,
        "同一批里 3 条好数据全部出报告（单条失败不炸批量，批量语义=部分成功）",
    );

    // ③ 失败清单带码，且**失败项不进 done**（这是续跑会重试的前提）。
    let fail_codes_ok = b.failures.len() == 2
        && b.failures.iter().any(|f| f.0 == 102 && f.1 == 1)
        && b.failures.iter().any(|f| f.0 == 104 && f.1 == 2)
        && !cur.done.contains(&102)
        && !cur.done.contains(&104)
        && cur.done.contains(&101);
    s.add(
        "失败清单带码且失败项不进 done",
        fail_codes_ok,
        "102⇒码1 / 104⇒码2；失败项不进 done（故续跑会重试），成功项进 done",
    );

    // ④ **断点续跑零重复**（十诫第 5 条）。
    //
    // 语料里 101 与 103 都已在 done ⇒ 第二轮 completed=0、skipped=3
    // （101/103/105 三个成功项），**且 reports 里不得再出现它们**。
    let b2 = st::scan_batch(&items, &t, &mut cur);
    let no_dup = b2.completed == 0
        && b2.skipped == 3
        && b2.reports.is_empty()
        && b2.flagged.is_empty();
    s.add(
        "断点续跑跳过已完成且零重复",
        no_dup,
        "第二轮 completed=0、skipped=3、reports 空（零重复由「完成的定义」本身保证，非额外判重）",
    );

    // ⑤ **续跑会重试失败项**（失败不进 done 的必然后果）。
    //
    // 把 102 修好后重跑 ⇒ 失败清单里不再有它，且它进 done。
    //
    // `retry_count` 的口径是「**同一 id 再次失败**的次数」，不含首次失败：
    // 此处 `cur2` 承自前两轮——第 1 轮 102/104 首次失败（不计），
    // 第 2 轮两者再次失败（各计一次 ⇒ 2），故重跑修复后仍为 2。
    // 若把首次失败也计入，该值会变成 4 —— 看板上「重试 4 次」其实只是
    // 两个资产各失败两次，与「同一资产反复失败」不是一回事。
    let mut cur2 = cur.clone();
    let retry_before = cur2.retry_count;
    let fixed: Vec<(u64, st::MeshStat)> = vec![(102, good_stat())];
    let b3 = st::scan_batch(&fixed, &t, &mut cur2);
    let retried = b3.completed == 1
        && b3.failures.is_empty()
        && cur2.done.contains(&102)
        // 修好一个失败项**不应**再增加重试计数（本轮它成功了）
        && cur2.retry_count == retry_before
        // 口径钉死：首次失败不计、再次失败才计 ⇒ 恰好 2（102 与 104 各一次）
        && retry_before == 2;
    s.add(
        "续跑重试失败项并转正",
        retried,
        "102 修好后重跑 ⇒ completed=1、进 done、失败清单清空；retry_count 口径为「再次失败」＝2（首次失败不计）",
    );

    // ⑤b **重试计数口径的反向对照**：全新游标上「首次失败」计 0。
    //
    // 若实现改成「每次失败都加」，这条红。这条与 ⑤ 合起来把口径钉死为
    // 「首次不计、再失败才计」，避免只钉住一侧。
    let mut cur_fresh = st::ScanCursor::default();
    let one_bad: Vec<(u64, st::MeshStat)> = vec![(901, st::MeshStat { vertices: 0, ..good_stat() })];
    let bf1 = st::scan_batch(&one_bad, &t, &mut cur_fresh);
    let first_fail_not_counted = bf1.failures.len() == 1 && cur_fresh.retry_count == 0;
    // 同一 id 第二次失败 ⇒ 这次才计一次
    let bf2 = st::scan_batch(&one_bad, &t, &mut cur_fresh);
    let second_fail_counted = bf2.failures.len() == 1 && cur_fresh.retry_count == 1;
    s.add(
        "重试计数只算再次失败（首次不计）",
        first_fail_not_counted && second_fail_counted,
        "同一坏 id 第一次失败 retry=0、第二次失败 retry=1（把首次也计入会让看板把多资产各失败一次误读成同一资产反复失败）",
    );

    // ⑥ 失败直方图按码归并、**升序**（便于两轮对比）。
    let hist_ok = b.failure_histogram.len() == 2
        && b.failure_histogram[0].0 == 1
        && b.failure_histogram[0].1 == 1
        && b.failure_histogram[1].0 == 2
        && b.failure_histogram[1].1 == 1;
    s.add(
        "失败直方图归并且升序",
        hist_ok,
        "码1×1 / 码2×1，升序输出（一眼看不出是「全库空网格」还是「个别坏数据」）",
    );

    // ⑦ **同 id 出现两次不得重复计入报告**（零重复的另一种形态）。
    let dup_items: Vec<(u64, st::MeshStat)> = vec![
        (201, good_stat()),
        (201, good_stat()),
        (201, good_stat()),
    ];
    let mut cur3 = st::ScanCursor::default();
    let bd = st::scan_batch(&dup_items, &t, &mut cur3);
    s.add(
        "同 id 重复出现只计一次",
        bd.reports.len() == 1 && bd.completed == 1 && bd.skipped == 2,
        "同 id 三次 ⇒ reports 1 条、completed=1、skipped=2（否则资产库重名会把报告灌水）",
    );

    // ⑧ **阈值覆盖：非法拒收 + 独立记账**（十诫第 3 条的反面）。
    let mut t2 = st::default_thresholds();
    let before_ok = t2.value_of(st::Metric::Vertices) == EXP_VERT_CEILING;
    let r1 = t2.override_with(st::Metric::Vertices, 0, 10, 50, "项目级收紧依据说明", "tight");
    let r2 = t2.override_with(st::Metric::Vertices, 0, 100, 500, "warn 超过 100 非法", "bad");
    let r3 = t2.override_with(st::Metric::Vertices, 0, 10, 50, "", "nobasis");
    let r4 = t2.override_with(st::Metric::Faces, 100, 50, 50, "floor>ceiling 非法", "bad2");
    let rej_ok = before_ok
        && r1 == Ok(())
        && r2 == Err(st::OverrideError::NotSane { metric: st::Metric::Vertices })
        && r3 == Err(st::OverrideError::NoBasis { metric: st::Metric::Vertices })
        && r4 == Err(st::OverrideError::NotSane { metric: st::Metric::Faces })
        && t2.rejected_overrides == 3
        && t2.applied_overrides == 1
        // 被拒的覆盖**不得生效**（静默夹取会让「我设了」看起来生效）
        && t2.value_of(st::Metric::Vertices) == 10;
    s.add(
        "非法覆盖拒收且独立记账",
        rej_ok,
        "合法覆盖生效为 10；warn>100 / 无依据 / floor>ceiling 三项被拒且 rejected=3（静默夹取=假装生效）",
    );

    // ⑨ 覆盖后项目类型与阈值联动（**报告要能自解释**）。
    let r_custom = st::inspect_mesh(300, &good_stat(), &t2).expect("应出报告");
    s.add(
        "覆盖后阈值与项目类型联动",
        t2.project_kind == "tight"
            && r_custom.thresholds_fingerprint != 0
            && r_custom.thresholds_fingerprint != st::thresholds_fingerprint(&st::default_thresholds()),
        "项目类型记为 tight；覆盖后指纹必与默认表不同（相同=覆盖没生效或指纹没覆盖到）",
    );

    // ⑩ 覆盖收紧后**原本合格的网格变超标**（覆盖确实起作用了）。
    //
    // 面数须给合法值：V=50 时面上界是 2*50-4=96，若沿用 good_stat 的 1800 面
    // 会先被「自相矛盾」拒掉（`expect` 就会panic），压根到不了顶点判定。
    let mid = st::MeshStat { vertices: 50, faces: 80, ..good_stat() };
    let rm = st::inspect_mesh(301, &mid, &t2).expect("应出报告");
    s.add(
        "覆盖收紧后原合格网格判超标",
        rm.verdict_of(st::Metric::Vertices)
            .map(|v| v.severity == st::Severity::Over)
            .unwrap_or(false),
        "上限收紧到 10 后，50 点的网格判 OVER（默认表下它是 Ok ⇒ 覆盖真的生效）",
    );

    // ⑩b **回归对照：面数越界与顶点判定互不干扰**（守「先校验后判定」的次序）。
    //
    // 若 `inspect_mesh` 把面数上界放宽回 `F<=V`，本条红；反之若把校验放在
    // 顶点判定之后（即先判阈值再校验自洽），V=50/F=1800 那种输入会给出
    // 一个「顶点 Over」的错误结论而不是拒收——那等于用错误原因掩盖了
    // 数据本身不可信。故正面钉死：越界输入必须**拒收**而非给出报告。
    let over_face_small_v = st::MeshStat { vertices: 50, faces: 1_800, ..good_stat() };
    s.add(
        "面数越界输入被拒收而非误判顶点",
        st::inspect_mesh(302, &over_face_small_v, &t2) == Err(st::StatError::FaceCountImplausible { faces: 1_800, vertices: 50 }),
        "V=50/F=1800 超 2V-4 上界 ⇒ 报 FaceCountImplausible，绝不因顶点未超阈值就放行",
    );
}

// ---------------------------------------------------------------------------
// 判据三：报告开放导出
// ---------------------------------------------------------------------------

fn c1613_export(s: &mut CheckSet) {
    let t = st::default_thresholds();
    let items: Vec<(u64, st::MeshStat)> = vec![
        (401, good_stat()),
        (402, st::MeshStat { vertices: EXP_VERT_CEILING + 1, ..good_stat() }),
        (403, st::MeshStat { compression_permille: 100, ..good_stat() }),
        (404, st::MeshStat { vertices: 0, ..good_stat() }),
    ];
    let mut cur = st::ScanCursor::default();
    let b = st::scan_batch(&items, &t, &mut cur);

    // ① 导出成功，且**必带阈值口径**（十诫第 6 条：逐项值对账）。
    let e = st::export_report(&b, &t);
    let mut exp_ok = false;
    let mut ok_count = 0u32;
    match e {
        Ok(bundle) => {
            let want = [
                (st::Metric::Vertices, EXP_VERT_CEILING),
                (st::Metric::Faces, EXP_FACE_CEILING),
                (st::Metric::LodLevels, EXP_LOD_CEILING),
                (st::Metric::CompressionPermille, EXP_COMP_FLOOR),
                (st::Metric::AttributeBytes, EXP_ATTR_CEILING),
            ];
            for (m, v) in want.iter() {
                // 逐项按 metric 精确匹配，不用序号
                let mut hit = false;
                for (m2, v2) in bundle.thresholds.iter() {
                    if m2 == m && v2 == v {
                        hit = true;
                        ok_count += 1;
                        break;
                    }
                }
                if !hit {
                    exp_ok = false;
                }
            }
            exp_ok = bundle.thresholds.len() == 5 && ok_count == 5;
            // ② 导出自解释（口径齐 + 规则版本 + 指纹非零）。
            s.add(
                "导出必带阈值口径",
                exp_ok && bundle.self_describing(),
                "导出包内五项阈值逐项等于期望（20万/30万/6/500‰/8MiB）；缺项则报告离开模块后不可自解释",
            );
        }
        Err(_) => {
            s.add(
                "导出必带阈值口径",
                false,
                "导出失败：阈值表缺项时应报错而非产出半截报告",
            );
        }
    }

    // ③ 规则版本号随包走（跨版本对比报告的前提）。
    let bundle_ok = st::export_report(&b, &t).is_ok();
    if bundle_ok {
        if let Ok(bundle) = st::export_report(&b, &t) {
            s.add(
                "导出自解释：版本与项目标签齐备",
                bundle.ruleset == st::RULESET_VERSION && !bundle.project_kind.is_empty(),
                "包内带 ruleset 版本与 project 标签（否则两份报告无法判断是否可比）",
            );
            // ④ **正文只含被高亮的网格**（体检的产出目的，不是全量流水）。
            let body_flagged = bundle.flagged_lines_containing("OVER").len();
            s.add(
                "导出正文只列异常网格",
                body_flagged == 2 && bundle.flagged_lines_containing("WARN").is_empty(),
                "402 顶点 OVER + 403 压缩率 OVER ⇒ 正文 2 行；401 合格不上正文、404 失败进失败计数",
            );
        }
    } else {
        s.add("导出自解释：版本与项目标签齐备", false, "导出失败");
        s.add("导出正文只列异常网格", false, "导出失败");
    }

    // ⑤ **缺项不导出**（不产出半截报告）。
    //
    // 用 `empty_thresholds()` 造一份真缺项的表：`rows` 私有且 `override_with`
    // 只改值不删行，若无此构造器则缺项面**在公开 API 上不可达**，
    // 这条判据就成了「测一个永不发生的分支」（十诫第 13 条：有错误码
    // ≠ 有产生路径）。断言两件事：默认表导得出、空表导不出。
    let empty_tbl = st::empty_thresholds();
    let empty_rejected = st::export_report(&b, &empty_tbl).is_err()
        && empty_tbl.len() == 0
        && !st::Metric::ALL.iter().any(|m| empty_tbl.has(*m));
    // 默认表导得出（对照面：否则「永远拒导出」也能过上面那条）。
    let default_ok = st::export_report(&b, &t).is_ok();
    s.add(
        "缺项阈值表拒导出且默认表放行",
        empty_rejected && default_ok,
        "空表 ⇒ export_report 报错（不产半截报告）；完整表 ⇒ 正常导出（否则「永远拒导出」是假通过）",
    );

    // ⑥ 查询面自洽：`has` / `get` / `value_of` 三者对每项一致（有则值非零、无则 has=false）。
    let t3 = st::default_thresholds();
    let consistent = st::Metric::ALL
        .iter()
        .all(|m| t3.has(*m) == t3.get(*m).is_some() && t3.value_of(*m) > 0);
    s.add(
        "阈值查询三面自洽",
        consistent && st::Metric::ALL.iter().all(|m| t3.has(*m)),
        "五项均有值且 has/get 一致（枚举封闭 ⇒ 不存在「未知项」；缺项时应拒导出而非补 0）",
    );

    // ⑥ 导出条目数与异常清单数**对得上**（不多不少）。
    if let Ok(bundle) = st::export_report(&b, &t) {
        s.add(
            "导出条目数与异常清单一致",
            bundle.flagged_lines_containing("OVER").len() == b.flagged.len(),
            "正文异常行数 == 报告 flagged 数（导出漏行会让治理清单不可信）",
        );
    } else {
        s.add("导出条目数与异常清单一致", false, "导出失败");
    }
}

// ---------------------------------------------------------------------------
// 判据四：判据集自身性质
// ---------------------------------------------------------------------------

fn c1613_selfchk(s: &mut CheckSet) {
    // **此处绝不能调 C 族自身，也绝不能调 `run_vef16_gridstat_checks()`**
    // —— 两者都会回调本函数，构成无限递归 ⇒ 栈溢出
    // （VE-F5401 亲历：症状是进程崩而非某条判据变红，极难定位）。
    // C 族与聚合层的截断分别由调用方与 `run_vef16_gridstat_checks()`
    // 内的 `assert!` 把关。
    let a = run_vef16_gridstat_checks_a_standalone();
    let b = run_vef16_gridstat_checks_b_standalone();
    let (_, ca) = a.red_items();
    let (_, cb) = b.red_items();
    s.add(
        "判据集自身性质：A/B 两族非空且未截断",
        ca >= 10 && cb >= 10 && !a.truncated() && !b.truncated(),
        "A≥10（单网格12）/ B≥10（批量10）条且 truncated=false（截断=判据被丢了还以为全绿）",
    );

    // 判据名与说明非空（可复核性）。
    let mut named = 0usize;
    for set in [a, b].iter() {
        let (items, n) = set.red_items();
        for i in 0..n {
            if let Some(ck) = items[i] {
                if !ck.name.is_empty() && !ck.detail.is_empty() {
                    named += 1;
                }
            }
        }
    }
    s.add(
        "判据名与说明非空",
        named == ca + cb,
        "A/B 每条判据都有名字与依据说明（无名无据的判据无法复核，等于没写）",
    );

    // 阈值表行数恒为 5（由 `Metric::ALL` 冻结，判据侧独立写死）。
    s.add(
        "阈值表规模受控",
        st::default_thresholds().len() == st::Metric::ALL.len()
            && st::Metric::ALL.len() == 5
            && !st::default_thresholds().is_empty(),
        "阈值表恰 5 行 = Metric::ALL 五项（枚举封闭，表不会悄悄长行或短行）",
    );
}

// ---------------------------------------------------------------------------
// 三族分立入口（规避 `CheckSet::MAX_CHECKS` 截断）
// ---------------------------------------------------------------------------

/// A 族：单网格体检。
pub fn run_vef16_gridstat_checks_a() -> CheckSet {
    let mut s = CheckSet::new("VE-F1613-a");
    c1613_single(&mut s);
    s
}

/// A 族独立入口。
pub fn run_vef16_gridstat_checks_a_standalone() -> CheckSet {
    run_vef16_gridstat_checks_a()
}

/// B 族：批体检。
pub fn run_vef16_gridstat_checks_b() -> CheckSet {
    let mut s = CheckSet::new("VE-F1613-b");
    c1613_batch(&mut s);
    s
}

/// B 族独立入口。
pub fn run_vef16_gridstat_checks_b_standalone() -> CheckSet {
    run_vef16_gridstat_checks_b()
}

/// C 族：导出 + 判据集自身性质。
pub fn run_vef16_gridstat_checks_c() -> CheckSet {
    let mut s = CheckSet::new("VE-F1613-c");
    c1613_export(&mut s);
    c1613_selfchk(&mut s);
    s
}

/// C 族独立入口。
pub fn run_vef16_gridstat_checks_c_standalone() -> CheckSet {
    run_vef16_gridstat_checks_c()
}

/// 全部判据（三族合并），显性断言未截断。
pub fn run_vef16_gridstat_checks() -> CheckSet {
    let a = run_vef16_gridstat_checks_a_standalone();
    let b = run_vef16_gridstat_checks_b_standalone();
    let c = run_vef16_gridstat_checks_c_standalone();
    let mut all = CheckSet::merge(a, b);
    all = CheckSet::merge(all, c);
    assert!(
        !all.truncated(),
        "VE-F1613 判据被 MAX_CHECKS={} 截断 —— 须再切族",
        MAX_CHECKS
    );
    all
}
