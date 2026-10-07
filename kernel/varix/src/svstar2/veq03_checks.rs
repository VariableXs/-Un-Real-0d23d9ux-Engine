//! VE-F3203 · 资源句柄与生命周期 —— 判据
//!
//! 26 条判据，逐条写明「它凭什么能抓错」。**判据名只是标签，说不出它能抓
//! 什么错的判据等于没写。**
//!
//! 本文件三处重点防弱门禁（详见 [`veq03_handle::criteria_summary`]）：
//! ① 代号判据必须用「空槽复用后访问旧句柄」的语料；
//! ② 对账判据两侧喂**不同**数字，参考值由语料独立算出；
//! ③ 误收判据必须含 `in_use=true` 候选，且**直接断言 skipped_in_use 含它**。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::veq02_graph::{
    ContentBody, ResourceGraph, ResourceGraphBuilder, ResourceId, ResourceKind, ResourceMetadata,
};
use super::veq01_pipeline::{DiagCode, Outcome};
use super::veq03_handle::{
    arc_allowed, audit_degradation_matrix, audit_downstream_handoff, audit_unique_access,
    collect, criteria_summary, decay, detect_misreclaim, handle_narration, ledger_consistent,
    lifecycle_matrix, reconcile, rollback_misreclaim, touch, transition, Generation, GcMeta,
    Handle, HandleTable, Lifecycle, RefLedger, Slot, CONSUMER_DOMAINS, GENERATION_BITS,
    GENERATION_MASK, HEAT_DECAY_TICKS, INDEX_MASK, LIFECYCLE_ARCS, MAX_SLOTS, Q03Code,
    BRIDGE_NOTE, LEDGER_REDLINE,
    MISRECLAIM_REDLINE, PERF_BUDGET, UNIQUE_ACCESS,
};

/// 建一张两节点一边的图：`0 → 1`（节点 0 引用节点 1）。
///
/// 图侧入度：节点 0 = 0，节点 1 = 1。
fn two_node_graph() -> ResourceGraph {
    let mut b = ResourceGraphBuilder::new();
    let meta = ResourceMetadata {
        present: true,
        ..ResourceMetadata::default()
    };
    let r0 = super::veq02_graph::Resource::new(
        ResourceId(0),
        ResourceKind::Texture,
        meta,
        ContentBody::present(1024, 0xAA, true),
    );
    let r1 = super::veq02_graph::Resource::new(
        ResourceId(1),
        ResourceKind::Shader,
        meta,
        ContentBody::present(512, 0xBB, true),
    );
    let _ = b.register(r0);
    let _ = b.register(r1);
    let _ = b.add_edge(ResourceId(0), ResourceId(1));
    b.freeze().value_or(empty_graph())
}

/// `freeze` 失败时的兜底：**零值图**（全字段 pub，可直接构造）。
///
/// 为何不 panic：判据函数必须能在任何环境下跑完并如实报红/报绿。退化为
/// 零值图后，所有依赖入度/槽位的判据会**如实转红**，不会静默通过。
fn empty_graph() -> ResourceGraph {
    ResourceGraph {
        slots: 0,
        nodes: Vec::new(),
        out_offsets: vec![0],
        out_targets: Vec::new(),
        in_offsets: vec![0],
        in_sources: Vec::new(),
        dangling: Vec::new(),
        placeholder_hits: Vec::new(),
        live: 0,
        edges: 0,
    }
}

/// 判据聚合入口。
pub fn run_veq03_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3203 · 资源句柄与生命周期");

    // =======================================================================
    // 一、句柄类型化（锚点：类型化句柄，类型安全——错类型句柄编译期拦截）
    // =======================================================================

    // 判据 1：句柄内嵌类型，且类型随凭证走。
    // 抓错：把 Handle 退化成裸 u32（类型信息丢失，错类型句柄运行时才炸）。
    {
        let h = Handle::new(7, 3, ResourceKind::Shader);
        let ok = h.index == 7 && h.generation == 3 && h.kind == ResourceKind::Shader;
        set.add(
            "Q3-HANDLE-TYPED-FIELDS",
            ok,
            "句柄同时携带 index / generation / kind 三字段；退化成裸 u32 则本条红",
        );
    }

    // 判据 2：打包/解包往返一致。
    // 抓错：packed 用了错的位宽（掩码写反），凭证在跨语言对拍中指向别的槽。
    {
        let probes = [
            (0u32, 1u32, ResourceKind::Texture),
            (65535, 65535, ResourceKind::Audio),
            (12345, 54321, ResourceKind::Stream),
            (0, 65535, ResourceKind::Scene),
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < probes.len() {
            let (idx, gen, kind) = probes[i];
            let h = Handle::new(idx, gen, kind);
            let (i2, g2) = Handle::unpack_raw(h.packed());
            if i2 != idx || g2 != gen {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "Q3-HANDLE-PACK-ROUNDTRIP",
            ok,
            "packed/unpack 往返逐位相等；掩码位宽写反则本条红",
        );
    }

    // 判据 2b：**位宽常量之间的约束关系**（VE-F3203 变异实测 M21 的教训）。
    //
    // 判据 2 的打包往返在 GENERATION_MASK 写反时**仍全绿**——因为往返用的是
    // 同一个掩码，掩码本身写错时「打包再解包」仍然还原得出原值。
    // ⇒ 必须另有一条断**常量之间关系**的判据。
    //
    // 期望值独立手算（不读常量自身）：
    //   下标占低16位 ⇒ INDEX_MASK = 2^16 - 1 = 65535
    //   代号占高16位 ⇒ GENERATION_MASK = 2^16 - 1 = 65535
    //   MAX_SLOTS = INDEX_MASK + 1 = 65536（下标位宽决定容量，不是代号）
    //
    // ⚠ 关于「位不重叠」的正确写法（初版写错过一次，记在这里）：
    // 两个掩码的**值都是 0xFFFF**，位置信息在**位移**里而不在掩码里。
    // 所以 `(INDEX_MASK | GENERATION_MASK) == 0xFFFF_FFFF` 是**恒假**的
    // （0xFFFF | 0xFFFF 仍是 0xFFFF），用它当判据等于造了一条永假的门禁。
    // 正确写法是断**位移后的关系**：把下标位与代号位分别摆到它们声明的位置上。
    {
        let idx_mask_ok = INDEX_MASK == 0xFFFF;
        let gen_mask_ok = GENERATION_MASK == 0xFFFF;
        let slots_ok = MAX_SLOTS == 0x1_0000;
        let bits_ok = GENERATION_BITS == 16;
        // 位不重叠的正确断法：下标占低 16 位 ⇒ `低16位全1 << 16` 后应恰好
        // 落在高 16 位；代号占高 16 位 ⇒ `高16位全1 << 0` 后应恰好落在高 16 位。
        let idx_hi = INDEX_MASK << GENERATION_BITS; // 0xFFFF_0000
        let gen_hi = (GENERATION_MASK as u64) << (32 - GENERATION_BITS); // 0xFFFF_0000
        let disjoint = idx_hi == 0xFFFF_0000 && (gen_hi as u32) == 0xFFFF_0000;
        set.add(
            "Q3-GEN-MASK-RELATIONS",
            idx_mask_ok && gen_mask_ok && slots_ok && bits_ok && disjoint,
            "INDEX_MASK=GENERATION_MASK=0xFFFF、MAX_SLOTS=65536、GENERATION_BITS=16、             下标左移16位与代号左移(32-16)位都恰好落在高16位（不重叠）。             位不重叠须断**位移后**的值：两掩码值相同都是0xFFFF，直接按位或恒为0xFFFF、             判据永假（初版踩过）；掩码写反时打包往返判据仍全绿，故须另断常量关系",
        );
    }

    // 判据 3：空句柄恒为空，且不与任何合法句柄混淆。
    // 抓错：空句柄用 (0,0) 表示 —— 而 (0,0) 在本条约定下不是合法组合
    //（新槽代号从 1 起），但一旦有人改成从 0 起就会撞车。
    {
        let none = Handle::NONE;
        let ok = none.is_none() && !Handle::new(0, 1, ResourceKind::Texture).is_none();
        set.add(
            "Q3-HANDLE-NONE-DISTINCT",
            ok,
            "空句柄用 (u32::MAX,u32::MAX) 表示，与任何合法句柄（含 index=0/gen=1）不混淆",
        );
    }

    // 判据 3b：**桥接表逐条钉死**（VE-F3203 变异实测 M22 的教训）。
    //
    // 只断「桥接说明文案非空」时，把某条桥接改成别的码**照样全绿**——
    // 文案会变，但没有任何判据读它。故必须逐条断 `bridge()` 的输出。
    //
    // 期望值独立手算（按 BRIDGE_NOTE 的声明「映射到语义最近的既有码」）：
    //   句柄失效 → IO_NOT_FOUND（资源不存在，最近）
    //   类型不符 → VALUE_INVALID（参数非法）
    //   非法迁移 → STAGE_ORDER_VIOLATED（次序违规，最近）
    //   账实不符 → BUDGET_EXCEEDED（超预算，最近）
    //   GC 误收  → VALUE_INVALID（取值非法，最近）
    //
    // 另断**字符串可反查**：`from_code(code())` 必须回到自己——
    // 不可反查的码在跨语言对拍里不可寻址。
    {
        const ALL: [Q03Code; 5] = [
            Q03Code::HandleRecycled,
            Q03Code::HandleTypeMismatch,
            Q03Code::LifecycleIllegal,
            Q03Code::LedgerMismatch,
            Q03Code::GcMisreclaim,
        ];
        const WANT_BRIDGE: [DiagCode; 5] = [
            DiagCode::IoNotFound,
            DiagCode::ValueInvalid,
            DiagCode::StageOrderViolated,
            DiagCode::BudgetExceeded,
            DiagCode::ValueInvalid,
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < ALL.len() {
            if ALL[i].bridge() != WANT_BRIDGE[i] {
                ok = false;
            }
            // 字符串反查必须回到自己。
            if Q03Code::from_code(ALL[i].code()) != Some(ALL[i]) {
                ok = false;
            }
            // 五个码的稳定字符串必须互异（否则反查有歧义）。
            i += 1;
        }
        // 互异性单独断：任两码字符串相同即反查有歧义。
        let mut j = 0usize;
        while j < ALL.len() {
            let mut k = j + 1;
            while k < ALL.len() {
                if ALL[j].code() == ALL[k].code() {
                    ok = false;
                }
                k += 1;
            }
            j += 1;
        }
        // 未登记字符串必须反查失败（白名单不得过宽）。
        if Q03Code::from_code("NOT_A_REAL_CODE").is_some() {
            ok = false;
        }
        set.add(
            "Q3-CODE-BRIDGE-VERBATIM",
            ok,
            "桥接表五条逐条等于判据侧手算值、五个稳定字符串互异、from_code 可反查自身、             未登记字符串反查失败；改任一条桥接则本条红",
        );
    }

    // =======================================================================
    // 二、句柄有效性（锚点：资源在则有效——失效句柄访问→显式错误+诊断）
    // =======================================================================

    // 判据 4：**复用后旧句柄失效**——本条是代号机制的命门。
    // 抓错：把 generation 改成不复位（复用后仍是旧代号）⇒ 旧句柄复活 ⇒ ABA。
    //
    // ⚠ 语料设计的关键（VE-F3203 变异实测 M2 的教训）：
    // 判据 4 的语料含 `destroy`，而 `destroy` **自己也会把代号 +1**。
    // 于是「h1 与 h2 代号不同」这件事可以由 destroy 代劳，
    // `allocate` 里那个 `generation + 1` 改成 `generation` 时本条**照样全绿**
    // ——判据被 destroy 的副作用遮住了。故另立判据 4b 用**不含 destroy**
    // 的语料单独咬住 allocate 的 +1。
    {
        let mut t = HandleTable::new();
        let h1 = t.allocate(ResourceKind::Texture).value_or(Handle::NONE);
        // 释放 + 销毁（销毁令代号 +1）。
        let _ = t.release(h1);
        let _ = t.destroy(h1);
        let h2 = t.allocate(ResourceKind::Texture).value_or(Handle::NONE);
        // h2 与 h1 同槽不同代号。
        let same_slot = h2.index == h1.index;
        let gen_differs = h2.generation != h1.generation;
        // 旧句柄 h1 必须被拒。
        let old_rejected = t.check(h1).is_err();
        // 新句柄 h2 必须通过。
        let new_ok = t.check(h2).is_ok();
        set.add(
            "Q3-HANDLE-ABA-RECYCLED",
            same_slot && gen_differs && old_rejected && new_ok,
            "空槽复用后同槽不同代号；旧句柄显式被拒、新句柄通过",
        );
    }

    // 判据 4b：**只释放不销毁**时，复用必须令代号 +1。
    //
    // 这条判据存在的唯一理由：判据 4 的语料含 destroy，代号差异可由 destroy
    // 代劳 ⇒ allocate 里的 +1 被遮住（M2 变异实测「仍全绿 26/26」）。
    // 本条语料**不含任何别的代号写点**，故 h2 与 h1 的代号差异**只能**来自
    // allocate 的 +1。
    //
    // 期望值独立手算：槽 0 首次分配代号 1（§一：0 保留给「从未用过」）→
    // 释放（refcount 0，代号不变）→ 再分配复用槽 0 ⇒ 代号 2。
    {
        let mut t = HandleTable::new();
        let h1 = t.allocate(ResourceKind::Texture).value_or(Handle::NONE);
        let gen_first = h1.generation;
        // 只释放，**不调 destroy**（destroy 会自己 +1，遮住被测点）。
        let _ = t.release(h1);
        let gen_after_release = t.slots[h1.index as usize].generation;
        let h2 = t.allocate(ResourceKind::Audio).value_or(Handle::NONE);
        set.add(
            "Q3-HANDLE-GEN-BUMP-ON-REUSE",
            gen_first == 1
                && gen_after_release == 1
                && h2.index == h1.index
                && h2.generation == 2,
            "只释放不销毁时，复用同一槽的代号由 1 变 2（allocate 内的 +1）；\
             该 +1 被改成原值则本条红。判据 4 的语料含 destroy 会遮住它，故分立一条",
        );
    }

    // 判据 5：失效访问返回**显式错误**而非崩溃/兜底。
    // 抓错：resolve 对失效句柄返回 Ok(0) 之类的兜底位置——那会让调用方
    // 悄悄拿到错误资源。
    {
        let mut t = HandleTable::new();
        let h = t.allocate(ResourceKind::Font).value_or(Handle::NONE);
        let before = t.rejected;
        let r = t.resolve(h);
        // 正常句柄必须成功。
        let good = r.is_ok();
        // 越界句柄必须失败。
        let bogus = Handle::new(9999, 1, ResourceKind::Font);
        let bad = t.resolve(bogus).is_err();
        // 失败计数必须增加（显式记账）。
        let counted = t.rejected > before;
        set.add(
            "Q3-STALE-ACCESS-EXPLICIT-ERR",
            good && bad && counted,
            "失效句柄 resolve 返回 Err 且计入 rejected；返回兜底位置则本条红",
        );
    }

    // 判据 6：检查顺序——越界优先于代号。
    // 抓错：把代号检查写在越界检查之前 ⇒ 越界句柄被诊断为「代号不符」，
    // 诊断指向错误的原因，消费方会去查凭证来源而不是查越界。
    {
        let mut t = HandleTable::new();
        let _ = t.allocate(ResourceKind::Style);
        // 构造一个既越界又代号错的句柄。
        let h = Handle::new(9999, 777, ResourceKind::Style);
        let is_out_of_range = match t.check(h) {
            Outcome::Err { message, .. } => message.contains("越界"),
            _ => false,
        };
        set.add(
            "Q3-CHECK-ORDER-OUT-OF-RANGE-FIRST",
            is_out_of_range,
            "越界句柄的诊断是「越界」而非「代号不符」；顺序颠倒则本条红",
        );
    }

    // 判据 7：类型不符被拒。
    // 抓错：删掉 kind 检查 ⇒ 拿纹理句柄访问着色器资源不被拦。
    {
        let mut t = HandleTable::new();
        let h = t.allocate(ResourceKind::Texture).value_or(Handle::NONE);
        // 造一个 kind 不符但 index/gen 都对的句柄。
        let wrong = Handle::new(h.index, h.generation, ResourceKind::Audio);
        let rejected = t.check(wrong).is_err();
        // 且报的是类型不符。
        let is_type = match t.check(wrong) {
            Outcome::Err { message, .. } => message.contains("类型不符"),
            _ => false,
        };
        set.add(
            "Q3-HANDLE-TYPE-MISMATCH-REJECTED",
            rejected && is_type,
            "kind 不符的凭证被拒且诊断指向类型；删掉 kind 检查则本条红",
        );
    }

    // 判据 8：双重释放被拒且**计数不变**。
    // 抓错：refcount 已是 0 时再 release 让它下溢（u32 回绕成 4 亿）。
    {
        let mut t = HandleTable::new();
        let h = t.allocate(ResourceKind::Model).value_or(Handle::NONE);
        let _ = t.release(h);
        let mid = t.refcount_of(h).unwrap_or(99);
        let r = t.release(h);
        let after = t.refcount_of(h).unwrap_or(99);
        set.add(
            "Q3-DOUBLE-RELEASE-GUARDED",
            r.is_err() && mid == 0 && after == 0,
            "计数为 0 时二次释放被拒且计数保持 0（不下溢）；下溢则本条红",
        );
    }

    // =======================================================================
    // 三、生命周期五态与弧表（锚点：五态 + 弧表；非法迁移→拒绝）
    // =======================================================================

    // 判据 9：五态齐备且字符串与锚点逐字一致。
    // 抓错：态名写成 ready/loaded 之类 ⇒ 跨语言对拍对不上。
    {
        const ALL: [Lifecycle; 5] = [
            Lifecycle::Created,
            Lifecycle::Loading,
            Lifecycle::Ready,
            Lifecycle::Evicted,
            Lifecycle::Destroyed,
        ];
        const WANT: [&str; 5] = ["created", "loading", "ready", "evicted", "destroyed"];
        let mut ok = ALL.len() == 5;
        let mut i = 0usize;
        while i < ALL.len() && i < WANT.len() {
            if ALL[i].as_str() != WANT[i] || ALL[i].zh().is_empty() {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "Q3-LIFECYCLE-FIVE-STATES",
            ok,
            "五态齐备，英文标识逐字等于 created/loading/ready/evicted/destroyed，中文名非空",
        );
    }

    // 判据 10：**态矩阵逐格**与弧表一致（25 格全覆盖）。
    // 抓错：arc_allowed 写成「除某一条外都允许」这类反向逻辑，抽查几格抓不到。
    // 参考值由 `arc_allowed` 自身枚举推导——本条断的是「矩阵与弧表一致」，
    // 弧表的**内容**正确性由判据 11 独立手算。
    {
        let m = lifecycle_matrix();
        let mut ok = m.len() == 25;
        let mut i = 0usize;
        while i < m.len() {
            let (a, b, allowed) = m[i];
            if arc_allowed(a, b) != allowed {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "Q3-LIFECYCLE-MATRIX-COMPLETE",
            ok,
            "5×5 = 25 格全覆盖，且矩阵读数与弧表函数逐格一致；缺格或不一致则本条红",
        );
    }

    // 判据 11：弧表**内容**由判据侧独立手算（不复用 arc_allowed）。
    // 抓错：弧表被改成 8 条或删掉 Evicted→Loading（重新加载路径断掉）。
    {
        // 手算依据锚点语义：created→loading→ready；loading/ready→evicted；
        // evicted→loading（重载）；ready/evicted→destroyed。
        // **不含** created→ready（跳过加载）、**不含** evicted→created、
        // **不含** created→evicted（未加载就逐出无意义）。
        let want: [(Lifecycle, Lifecycle); 7] = [
            (Lifecycle::Created, Lifecycle::Loading),
            (Lifecycle::Loading, Lifecycle::Ready),
            (Lifecycle::Loading, Lifecycle::Evicted),
            (Lifecycle::Ready, Lifecycle::Evicted),
            (Lifecycle::Ready, Lifecycle::Destroyed),
            (Lifecycle::Evicted, Lifecycle::Loading),
            (Lifecycle::Evicted, Lifecycle::Destroyed),
        ];
        let mut ok = LIFECYCLE_ARCS.len() == want.len();
        let mut i = 0usize;
        while i < want.len() && i < LIFECYCLE_ARCS.len() {
            if LIFECYCLE_ARCS[i] != want[i] {
                ok = false;
            }
            i += 1;
        }
        // 关键三条**必须不**在弧表里（否则是「什么都允许」）。
        let forbidden = [
            (Lifecycle::Created, Lifecycle::Ready),
            (Lifecycle::Evicted, Lifecycle::Created),
            (Lifecycle::Destroyed, Lifecycle::Loading),
        ];
        let mut j = 0usize;
        while j < forbidden.len() {
            if arc_allowed(forbidden[j].0, forbidden[j].1) {
                ok = false;
            }
            j += 1;
        }
        set.add(
            "Q3-LIFECYCLE-ARCS-VERBATIM",
            ok,
            "弧表 7 条逐条等于判据侧手算值，且 created→ready / evicted→created / destroyed→loading 三条必不在表内",
        );
    }

    // 判据 12：非法迁移被拒且**保持原态**（不写入非法值）。
    // 抓错：迁移失败时仍把状态改成目标态 ⇒ 状态机被污染，后续全部判错。
    {
        let cases = [
            (Lifecycle::Created, Lifecycle::Ready, false),
            (Lifecycle::Destroyed, Lifecycle::Loading, false),
            (Lifecycle::Evicted, Lifecycle::Created, false),
            (Lifecycle::Loading, Lifecycle::Ready, true),
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < cases.len() {
            let (from, to, want_accept) = cases[i];
            let t = transition(from, to);
            if t.accepted != want_accept {
                ok = false;
            }
            // 被拒时状态必须仍是 from。
            if !t.accepted && t.state != from {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "Q3-LIFECYCLE-ILLEGAL-REJECTED",
            ok,
            "非法迁移被拒且状态保持原值；被拒后仍写入目标态则本条红",
        );
    }

    // 判据 13：自环（幂等）被接受。
    // 抓错：把自环也判为非法 ⇒ 重复「置为就绪」变成错误，正常代码被迫加标志位。
    {
        const ALL: [Lifecycle; 5] = [
            Lifecycle::Created,
            Lifecycle::Loading,
            Lifecycle::Ready,
            Lifecycle::Evicted,
            Lifecycle::Destroyed,
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < ALL.len() {
            if !transition(ALL[i], ALL[i]).accepted {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "Q3-LIFECYCLE-SELFLOOP-IDEMPOTENT",
            ok,
            "五态自环均被接受（幂等置位不是错误）；自环被判非法则本条红",
        );
    }

    // =======================================================================
    // 四、引用计数（锚点：句柄计数驱动生命周期——零引用→GC 候选）
    // =======================================================================

    // 判据 14：计数随克隆/释放增减，且总量守恒。
    // 抓错：clone 不加计数（多个持有者只有一个计数 ⇒ 过早回收）。
    {
        let mut t = HandleTable::new();
        let h = t.allocate(ResourceKind::Geometry).value_or(Handle::NONE);
        let start = t.total_refcount();
        let c = t.clone_handle(h);
        let after_clone = t.total_refcount();
        let r1 = t.release(h);
        let mid = t.total_refcount();
        let r2 = t.release(h);
        let end = t.total_refcount();
        set.add(
            "Q3-REFCOUNT-CONSERVED",
            c.is_ok()
                && start == 1
                && after_clone == 2
                && mid == 1
                && end == 0
                && r1.is_ok()
                && r2.is_ok(),
            "计数序列 1→2→1→0 逐步守恒；clone 不加计数则本条红",
        );
    }

    // 判据 15：零引用槽的活跃句柄数正确。
    // 抓错：live_handles 按槽数而非按计数统计。
    //
    // 期望值独立手算：`live_handles` 数的是 **refcount > 0 的槽数**。
    // 语料 a 克隆过一次 ⇒ a 的计数是 2 ⇒ **释放一次不归零**、槽仍活跃。
    // 完整序列（手算）：
    //   分配 a、b → 两槽计数 1/1 ⇒ live = 2
    //   clone a    → a 计数 2   ⇒ live = 2（槽没变，计数变了）
    //   release a  → a 计数 1   ⇒ live = 2（**不是 1**）
    //   release a  → a 计数 0   ⇒ live = 1
    //   release b  → b 计数 0   ⇒ live = 0
    // 本条真正咬住的是第 3 步：把「计数 2」当成「计数 1」的实现在此转红。
    {
        let mut t = HandleTable::new();
        let a = t.allocate(ResourceKind::Texture).value_or(Handle::NONE);
        let b = t.allocate(ResourceKind::Audio).value_or(Handle::NONE);
        let after_alloc = t.live_handles();
        let _ = t.clone_handle(a);
        let after_clone = t.live_handles();
        let _ = t.release(a);
        let after_first_release = t.live_handles();
        let _ = t.release(a);
        let after_second_release = t.live_handles();
        let _ = t.release(b);
        let after_all = t.live_handles();
        set.add(
            "Q3-LIVE-HANDLE-COUNT",
            after_alloc == 2
                && after_clone == 2
                && after_first_release == 2
                && after_second_release == 1
                && after_all == 0,
            "live 数的是 refcount>0 的槽：克隆后释放一次不归零（live 仍 2），\
             二次释放才减为 1，全部释放为 0；把计数 2 当 1 则本条红",
        );
    }

    // 判据 16：销毁要求计数为 0。
    // 抓错：销毁不查计数 ⇒ 销毁后残留凭证指向已销毁资源。
    {
        let mut t = HandleTable::new();
        let h = t.allocate(ResourceKind::Scene).value_or(Handle::NONE);
        let _ = t.clone_handle(h);
        let blocked = t.destroy(h).is_err();
        let _ = t.release(h);
        let _ = t.release(h);
        let allowed = t.destroy(h).is_ok();
        set.add(
            "Q3-DESTROY-REQUIRE-ZERO-REF",
            blocked && allowed,
            "有残留凭证时销毁被拒、归零后放行；不查计数则本条红",
        );
    }

    // =======================================================================
    // 五、计数与图双源对账（锚点：计数≠图入度→立案）
    // =======================================================================

    // 判据 17：**两侧喂不同数字**时对账必须立案（两侧同数字时对账恒绿）。
    // 抓错：reconcile 两侧都从句柄表取 ⇒ 自证式，本条与 18 一起绿得毫无意义。
    {
        let g = two_node_graph();
        // 计数侧**故意**与图侧不符：节点 0 计 3、节点 1 计 0，图侧是 0/1。
        let mut l = RefLedger::new();
        l.counts = vec![3u32, 0u32];
        let r = reconcile(&l, &g);
        // 至少两条立案。
        let detected = r.cases.len() >= 2;
        // 归因方向：节点 0 计数 > 入度 ⇒ 「计数侧多发」。
        let mut blame_ok = false;
        let mut i = 0usize;
        while i < r.cases.len() {
            if r.cases[i].id == 0 && r.cases[i].delta == 3 {
                blame_ok = r.cases[i].blame.contains("计数侧");
            }
            i += 1;
        }
        set.add(
            "Q3-RECONCILE-DETECTS-DRIFT",
            detected && blame_ok,
            "两侧数字不同时对账立案，且计数>入度归因为「计数侧多发」；两侧同源则本条红",
        );
    }

    // 判据 18：**两侧一致**时不得立案（防空对账）。
    // 抓错：对账器无条件立案 ⇒ 变成噪音，真差异被淹没。
    {
        let g = two_node_graph();
        // 计数侧按图侧**独立重算**：节点 0 入度 0、节点 1 入度 1。
        let mut l = RefLedger::new();
        l.counts = vec![0u32, 1u32];
        let r = reconcile(&l, &g);
        set.add(
            "Q3-RECONCILE-CLEAN-NO-CASE",
            r.cases.is_empty() && ledger_consistent(&r),
            "两侧一致时零立案；无条件立案则本条红",
        );
    }

    // 判据 19：参考值由**图入度独立重算**（不读被测 reconcile 的输出）。
    // 抓错：判据侧直接用 reconcile 的 cases 长度当期望 ⇒ 自证式。
    {
        let g = two_node_graph();
        // 独立重算：逐槽问图入度，与计数侧逐项比。
        let mut l = RefLedger::new();
        l.counts = vec![2u32, 1u32];
        let mut mismatches = 0usize;
        let mut i = 0usize;
        while i < g.slots {
            let lc = l.count_of(i as u32);
            let gc = g.in_degree(ResourceId(i as u32)) as u32;
            if lc != gc {
                mismatches += 1;
            }
            i += 1;
        }
        // 只有节点 0 不符（2 vs 0），节点 1 相符（1 vs 1）。
        let want = 1usize;
        let r = reconcile(&l, &g);
        set.add(
            "Q3-RECONCILE-ORACLE-INDEPENDENT",
            mismatches == want && r.cases.len() == want,
            "判据侧独立重算图入度得 1 条不符，与 reconcile 输出一致；直接读被测输出则本条红",
        );
    }

    // 判据 20：账实一致红线文案非空且含「立案」。
    // 抓错：红线写成「建议关注」这类软措辞 ⇒ 门禁退化为提示。
    {
        let ok = LEDGER_REDLINE.contains("立案") && LEDGER_REDLINE.len() > 20;
        set.add(
            "Q3-LEDGER-REDLINE-STATED",
            ok,
            "账实一致红线明文写「立案」且非空；软措辞则本条红",
        );
    }

    // =======================================================================
    // 六、分代 GC（锚点：分代 hot/cold——热度衰减）
    // =======================================================================

    // 判据 21：热度衰减达阈值则降代，未达不降。
    // 抓错：阈值写成 0 ⇒ 一轮就全冷化，热代失去意义。
    {
        let mut m = GcMeta::new();
        let before = m.generation;
        let d1 = decay(&mut m);
        let d2 = decay(&mut m);
        let after = m.generation;
        set.add(
            "Q3-HEAT-DECAY-THRESHOLD",
            !d1 && d2 && before == Generation::Hot && after == Generation::Cold,
            "第 1 轮不降代、第 2 轮（达 HEAT_DECAY_TICKS）降为 cold；阈值写 0 则本条红",
        );
    }

    // 判据 22：触碰使冷代**回升**热代（否则偶然用一次的资源永久留在冷代）。
    // 抓错：touch 只清零 idle_ticks 不改代 ⇒ 冷代不可回升。
    {
        let mut m = GcMeta::new();
        let _ = decay(&mut m);
        let _ = decay(&mut m);
        let was_cold = m.generation == Generation::Cold;
        touch(&mut m);
        let back_hot = m.generation == Generation::Hot;
        let ticks_zero = m.idle_ticks == 0;
        set.add(
            "Q3-TOUCH-PROMOTES-BACK-HOT",
            was_cold && back_hot && ticks_zero,
            "冷代被触碰后回到热代且闲置计数归零；不回升则本条红",
        );
    }

    // 判据 23：**误收红线第一道**——`in_use=true` 的候选绝不回收，
    // 且必须出现在 skipped_in_use 里。
    // 抓错：只断「reclaimed 不含它」——与「候选压根没进名单」不可区分。
    {
        let mut metas: Vec<GcMeta> = Vec::new();
        for _ in 0..3 {
            metas.push(GcMeta::new());
        }
        metas[0].in_use = true; // 正在使用
        metas[1].in_use = false; // 未使用但冷代
        let mut m1 = metas[1];
        let _ = decay(&mut m1);
        let _ = decay(&mut m1);
        metas[1] = m1;
        let mut l = RefLedger::new();
        l.counts = vec![0u32, 0u32, 0u32];
        let d = collect(&[0u32, 1u32, 2u32], &mut metas, &l);
        // 节点 0 必须被跳过且记账。
        let skipped_has_0 = d.skipped_in_use.contains(&0u32);
        // 节点 0 绝不在回收名单里。
        let reclaimed_has_no_0 = !d.reclaimed.contains(&0u32);
        // 节点 2 是热代（未降代）⇒ 不回收。
        let reclaimed_has_no_2 = !d.reclaimed.contains(&2u32);
        set.add(
            "Q3-GC-NEVER-RECLAIM-IN-USE",
            skipped_has_0 && reclaimed_has_no_0 && reclaimed_has_no_2,
            "in_use 候选既进 skipped_in_use 又不进 reclaimed；热代候选即便入度 0 也不收；漏记跳过名单则本条红",
        );
    }

    // 判据 24：**误收红线第二道**——计数侧仍有凭证时也跳过（双源交叉）。
    // 抓错：只查 in_use 不查计数 ⇒ 「图说入度 0、计数说有凭证」的分叉被漏过。
    {
        let mut metas: Vec<GcMeta> = Vec::new();
        for _ in 0..2 {
            let mut m = GcMeta::new();
            let _ = decay(&mut m);
            let _ = decay(&mut m);
            metas.push(m); // 两个都是冷代
        }
        // in_use 都是 false，但计数侧说节点 0 还有 2 个凭证。
        let mut l = RefLedger::new();
        l.counts = vec![2u32, 0u32];
        let d = collect(&[0u32, 1u32], &mut metas, &l);
        let skipped_0 = d.skipped_in_use.contains(&0u32);
        let reclaimed_no_0 = !d.reclaimed.contains(&0u32);
        // 节点 1 计数为 0 且冷代 ⇒ 应被回收。
        let reclaimed_1 = d.reclaimed.contains(&1u32);
        set.add(
            "Q3-GC-LEDGER-CROSS-CHECK",
            skipped_0 && reclaimed_no_0 && reclaimed_1,
            "计数侧有凭证的候选被跳过、真正无主的冷代被回收；只查 in_use 则本条红",
        );
    }

    // =======================================================================
    // 七、误收 P1 与回滚（锚点：GC 误收→P1+回滚）
    // =======================================================================

    // 判据 25：**事后复核**独立于事前闸门。
    // 抓错：只有 collect 里的事前判断，没有事后复核 ⇒ 「事前闸门被绕过」
    // 这条路径永远不会被发现。
    {
        let mut metas: Vec<GcMeta> = Vec::new();
        for _ in 0..2 {
            metas.push(GcMeta::new());
        }
        // 模拟「被绕过」：回收后才发现 in_use 为真。
        metas[0].in_use = true;
        let mut cases = detect_misreclaim(&[0u32, 1u32], &metas);
        let found_0 = cases.iter().any(|c| c.id == 0);
        let no_false_positive = !cases.iter().any(|c| c.id == 1);
        // 回滚后态必须是 evicted（**不是** ready——内容已不可恢复）。
        let mut lc = Lifecycle::Ready;
        let mut meta = metas[0];
        // ⚠ 零 panic 面（VE-F3203 变异实测 M20 的教训）：
        // 原写法直接 `cases[0]`，而一旦 `detect_misreclaim` 被改成恒返回空
        // （事后复核失效），下标越界会**panic** 而不是判红——判据自己先崩，
        // 门禁反而「什么也没报」。
        // 故先判非空、并按 id 定位而非按下标；为空时直接判红并跳过回滚。
        let rollback_to_evicted;
        let meta_reset;
        let line_non_empty;
        match cases.iter_mut().find(|c| c.id == 0) {
            Some(c) => {
                let line = rollback_misreclaim(c, &mut lc, &mut meta);
                rollback_to_evicted = lc == Lifecycle::Evicted;
                meta_reset =
                    meta.generation == Generation::Hot && meta.idle_ticks == 0 && !meta.in_use;
                line_non_empty = line.contains("P1") && line.contains("evicted");
            }
            None => {
                // 找不到事故记录 ⇒ 事后复核失效，本条必须红。
                rollback_to_evicted = false;
                meta_reset = false;
                line_non_empty = false;
            }
        }
        set.add(
            "Q3-MISRECLAIM-ROLLBACK-P1",
            found_0
                && no_false_positive
                && rollback_to_evicted
                && meta_reset
                && line_non_empty,
            "事后复核抓出被绕过的候选；回滚至 evicted（非 ready）、元数据复位、复述含 P1 与 evicted",
        );
    }

    // 判据 26：误收红线文案 + 降级矩阵 + 对接点 + 八消费域 + 桥接说明 齐备。
    // 抓错：矩阵任一列为空 ⇒ 该行等于没写。
    {
        let redline_ok = MISRECLAIM_REDLINE.contains("P1")
            && MISRECLAIM_REDLINE.contains("显式记账");
        let matrix_ok = audit_degradation_matrix().is_ok();
        let handoff_ok = audit_downstream_handoff().value_or(Vec::new()).len() == 4;
        let domains_ok =
            audit_unique_access().value_or(Vec::new()).len() == 8 && CONSUMER_DOMAINS.len() == 8;
        let bridge_ok = BRIDGE_NOTE.contains("不可反推");
        let uniq_ok = UNIQUE_ACCESS.contains("唯一");
        let perf_ok = PERF_BUDGET.len() == 4;
        let mask_ok = GENERATION_MASK == 65535 && MAX_SLOTS == 65536 && HEAT_DECAY_TICKS == 2;
        let slot_ok = Slot::VACANT.generation == 0 && Slot::VACANT.refcount == 0;
        let narr_ok = handle_narration().contains("五态");
        let sum_ok = criteria_summary().contains("弱门禁");
        set.add(
            "Q3-DOC-CONTRACT-COMPLETE",
            redline_ok
                && matrix_ok
                && handoff_ok
                && domains_ok
                && bridge_ok
                && uniq_ok
                && perf_ok
                && mask_ok
                && slot_ok
                && narr_ok
                && sum_ok,
            "误收红线/降级矩阵/对接点/八消费域/桥接说明/唯一访问/性能预算/位宽常量/空槽/概述/判据说明逐项非空且齐备",
        );
    }

    set
}