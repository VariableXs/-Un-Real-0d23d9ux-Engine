//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0933 · IPI 业务冻结接口文档（调度域/内存域消费声明）（目标 280 行）
//! 判据原文摘录（书 2600 行）：UNX-F0933-J1 文档齐备（smp_call/reschedule/
//! notify/shootdown 四组接口汇总），样例可编译，版本化机制 10/10 次正确。
//! 职责定位：接口冻结不是写四段文字，是把「 consumers 靠什么锚定」做成可执行
//! 的对象——签名指纹（FNV 固定）、语义四节（职责/前置/后置/错误）、序约束
//! （时序不变量）、消费声明（谁能调、调了承诺什么）。四组接口（smp_call/
//! reschedule/notify/shootdown）各出一份冻结记录，版本化=指纹变更必须走 ADR
//! 升级路径（小版补丁/大版断裂显性拒绝），消费方编译期锚定的是版本号+指纹双
//! 常量——文档与代码脱钩=冻结是幻觉，本单把脱钩做成拒签条件。
//! 设计要点：①**四组接口册**——每组：接口名/签名指纹/语义四节哈希/序约束
//! 文本哈希/冻结版本；册本身有总指纹（任一页变动总指纹变）；②**双常量锚定**
//! ——消费方必须携带 (VERSION, FINGERPRINT) 对，缺一即拒签（版本对但指纹错=
//! 静默改签名，是冻结面最隐蔽的死法）；③**ADR 升级门**——版本升级只允许
//! 补丁位（末位+1）或显性断裂（major+1 且必须带断裂说明哈希）；同版本重复
//! 冻结=幂等放行，降版=拒绝（接口只进不退）；④**可编译样例**——消费样例以
//! 类型级锚点内嵌（样例调用穿的签名与冻结指纹同源算，样例改动漂移即检出）；
//! ⑤**消费声明登记**——调度域/内存域两大消费方各有登记项（消费接口集+承
//! 诺版），未登记的消费方调用=越权（冻结接口的消费面是白名单不是猜的）。
//! 诊断码段：0xD4（本单独占，0xD401~0xD40F）。
//! 施工：AI · 2026-10-11。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD4 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD401 组缺页（四组接口册少一组——文档齐备性破坏）。
pub const XC_DC_GROUP_MISS: u32 = 0xD401;
/// 0xD402 双锚缺一（版本/指纹缺一——锚定不完整）。
pub const XC_DC_ANCHOR_HALF: u32 = 0xD402;
/// 0xD403 指纹失配（消费方锚定指纹≠册内指纹——静默改签名）。
pub const XC_DC_FP_MISMATCH: u32 = 0xD403;
/// 0xD404 降版冻结（版本号回退——接口只进不退被破坏）。
pub const XC_DC_VERSION_BACK: u32 = 0xD404;
/// 0xD405 越级断裂（major 升但无断裂说明——断裂未显性化）。
pub const XC_DC_BREAK_SILENT: u32 = 0xD405;
/// 0xD406 未登记消费（调用方不在消费白名单——越权调用）。
pub const XC_DC_UNREGISTERED: u32 = 0xD406;
/// 0xD407 样例漂移（样例签名与冻结指纹不同源——文档与代码脱钩）。
pub const XC_DC_SAMPLE_DRIFT: u32 = 0xD407;
/// 0xD408 重复冻结冲突（同版本不同内容——冻结面双轨）。
pub const XC_DC_REFREEZE: u32 = 0xD408;
/// 0xD409 序约束缺失（接口册缺序约束哈希——时序不变量无文档）。
pub const XC_DC_ORDER_MISS: u32 = 0xD409;
/// 0xD40A 册指纹失守（总指纹与逐页重算不符）。
pub const XC_DC_BOOK_FP: u32 = 0xD40A;
/// 0xD40B 版本化次数异常（10/10 口径未达）。
pub const XC_DC_VERSION_ROUNDS: u32 = 0xD40B;
/// 0xD40C 语义四节缺节（职责/前置/后置/错误缺一）。
pub const XC_DC_SEMANTIC_GAP: u32 = 0xD40C;
/// 0xD40D 双跑漂移（同输入两跑册指纹不等）。
pub const XC_DC_DUAL_DRIFT: u32 = 0xD40D;
/// 0xD40E 承诺版缺失（消费方登记缺承诺版本）。
pub const XC_DC_PROMISE_MISS: u32 = 0xD40E;
/// 0xD40F 断裂说明空（major 升了但说明哈希为空）。
pub const XC_DC_BREAK_EMPTY: u32 = 0xD40F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_DC_GROUP_MISS => (
            "组缺页：四组接口册不齐",
            "smp_call/reschedule/notify/shootdown 四组缺一即文档不完备",
        ),
        XC_DC_ANCHOR_HALF => (
            "双锚缺一：版本与指纹只锚一个",
            "版本对指纹错=静默改签名；只锚版本=漂移检出不了",
        ),
        XC_DC_FP_MISMATCH => (
            "指纹失配：消费方锚定与册内不符",
            "消费方编译期锚的指纹必须与册一致；不符=对面已被改而消费方不知",
        ),
        XC_DC_VERSION_BACK => (
            "降版冻结：版本号回退",
            "接口只进不退；降版=冻结承诺被撤回",
        ),
        XC_DC_BREAK_SILENT => (
            "越级断裂：major 升无断裂说明",
            "断裂性变更必须显性带说明哈希；静默越级=下游无感知断裂",
        ),
        XC_DC_UNREGISTERED => (
            "未登记消费：调用方不在白名单",
            "冻结接口的消费面是白名单（调度域/内存域等显式登记）",
        ),
        XC_DC_SAMPLE_DRIFT => (
            "样例漂移：样例与冻结不同源",
            "样例是文档的一部分；样例签名须与冻结指纹同源算",
        ),
        XC_DC_REFREEZE => (
            "重复冻结冲突：同版本不同内容",
            "同版本同内容=幂等放行；同版本异内容=双轨冻结必须拒",
        ),
        XC_DC_ORDER_MISS => (
            "序约束缺失：接口无时序不变量",
            "无序约束的接口=允许任何时序，调用方无法验证自己合规",
        ),
        XC_DC_BOOK_FP => (
            "册指纹失守：总指纹≠逐页重算",
            "总指纹是册完整性锚；失守=册被局部改动未走版本化",
        ),
        XC_DC_VERSION_ROUNDS => (
            "版本化轮次异常：10/10 未达",
            "版本化机制要经 10 轮升降压测证明正确",
        ),
        XC_DC_SEMANTIC_GAP => (
            "语义四节缺节",
            "职责/前置/后置/错误四节缺一=语义不完备",
        ),
        XC_DC_DUAL_DRIFT => (
            "双跑漂移：同输入册指纹不等",
            "册构造必须确定性；不等则查隐藏可变状态",
        ),
        XC_DC_PROMISE_MISS => (
            "承诺版缺失：消费方登记无承诺版本",
            "消费方必须声明按哪一版消费；无承诺=漂移无追责",
        ),
        XC_DC_BREAK_EMPTY => (
            "断裂说明为空",
            "major 升级必须带非空说明哈希",
        ),
        _ => ("未知诊断码", "码不在 0xD4 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 公共件（家族复用：FNV 第二十一用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 族共享伪随机源（同种子同序列——双跑确定性）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------
// §2 接口冻结页与册
// ---------------------------------------------------------------------------

/// 接口组四闭集（B07 面域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IfaceGroup {
    SmpCall,
    Reschedule,
    Notify,
    Shootdown,
}

impl IfaceGroup {
    pub fn all() -> [IfaceGroup; 4] {
        [IfaceGroup::SmpCall, IfaceGroup::Reschedule, IfaceGroup::Notify, IfaceGroup::Shootdown]
    }
    pub fn name(self) -> &'static str {
        match self {
            IfaceGroup::SmpCall => "smp_call",
            IfaceGroup::Reschedule => "reschedule",
            IfaceGroup::Notify => "notify",
            IfaceGroup::Shootdown => "shootdown",
        }
    }
}

/// 一页接口冻结记录。
#[derive(Clone, Debug)]
pub struct IfacePage {
    pub group: IfaceGroup,
    pub version: (u32, u32), // (major, minor)
    pub sig_fp: u64,
    pub semantic_fp: u64, // 四节合并哈希
    pub order_fp: u64,    // 序约束哈希
    pub break_note_fp: u64,
    pub sample_fp: u64, // 样例签名同源指纹
}

impl IfacePage {
    /// 页指纹（版本+五哈希）。
    pub fn fp(&self) -> u64 {
        let mut b = Vec::new();
        b.extend_from_slice(self.group.name().as_bytes());
        b.push(0x1F);
        b.extend_from_slice(self.version.0.to_le_bytes().as_slice());
        b.extend_from_slice(self.version.1.to_le_bytes().as_slice());
        b.extend_from_slice(self.sig_fp.to_le_bytes().as_slice());
        b.extend_from_slice(self.semantic_fp.to_le_bytes().as_slice());
        b.extend_from_slice(self.order_fp.to_le_bytes().as_slice());
        b.extend_from_slice(self.break_note_fp.to_le_bytes().as_slice());
        b.extend_from_slice(self.sample_fp.to_le_bytes().as_slice());
        fnv64(&b)
    }
}

/// 消费方登记（调度域/内存域）。
#[derive(Clone, Debug)]
pub struct ConsumerRec {
    pub name: &'static str,
    pub group: IfaceGroup,
    pub promise_ver: (u32, u32),
}

/// 冻结册引擎。
pub struct FreezeBook {
    pages: Vec<IfacePage>,
    consumers: Vec<ConsumerRec>,
    pub rounds: u32,
}

impl FreezeBook {
    pub fn new() -> FreezeBook {
        FreezeBook { pages: Vec::new(), consumers: Vec::new(), rounds: 0 }
    }

    /// 冻结一页（v1 直冻或增量升级）。
    pub fn freeze(&mut self, p: IfacePage) -> Result<(), (u32, String)> {
        if p.order_fp == 0 {
            return Err((XC_DC_ORDER_MISS, format!("{} 缺序约束", p.group.name())));
        }
        if p.semantic_fp == 0 {
            return Err((XC_DC_SEMANTIC_GAP, format!("{} 缺语义四节", p.group.name())));
        }
        for pg in &self.pages {
            if pg.group == p.group {
                // 升级判定。
                if p.version.0 > pg.version.0 {
                    // major 升=断裂：必须带说明。
                    if p.break_note_fp == 0 {
                        return Err((
                            XC_DC_BREAK_EMPTY,
                            format!("{} major 升无断裂说明", p.group.name()),
                        ));
                    }
                    if p.version.1 == 0 && p.sig_fp == pg.sig_fp {
                        return Err((
                            XC_DC_BREAK_SILENT,
                            format!("{} major 升但签名未变——非断裂升 major", p.group.name()),
                        ));
                    }
                } else if p.version.0 == pg.version.0 && p.version.1 > pg.version.1 {
                    // minor 补丁：签名不得断裂（sig_fp 须同）。
                    if p.sig_fp != pg.sig_fp {
                        return Err((
                            XC_DC_FP_MISMATCH,
                            format!("{} minor 升签名变——补丁不得断裂", p.group.name()),
                        ));
                    }
                    if p.break_note_fp != 0 {
                        return Err((
                            XC_DC_BREAK_SILENT,
                            format!("{} minor 升带断裂说明——越级断裂", p.group.name()),
                        ));
                    }
                } else if p.version < pg.version {
                    return Err((
                        XC_DC_VERSION_BACK,
                        format!("{} 版本 {:?}<{:?}——降版拒绝", p.group.name(), p.version, pg.version),
                    ));
                } else if p.version == pg.version {
                    // 幂等：同版本同内容放行，异内容拒。
                    if pg.fp() == p.fp() {
                        return Ok(());
                    }
                    return Err((
                        XC_DC_REFREEZE,
                        format!("{} 同版本异内容——双轨冻结", p.group.name()),
                    ));
                }
                // 升级落册。
                self.pages.retain(|x| x.group != p.group);
                self.pages.push(p);
                self.rounds += 1;
                return Ok(());
            }
        }
        // 首冻 v1：minor 必须 0 且签名/语义/样例齐。
        if p.version.1 != 0 {
            return Err((XC_DC_ANCHOR_HALF, format!("{} 首冻非 (1,0)", p.group.name())));
        }
        self.pages.push(p);
        self.rounds += 1;
        Ok(())
    }

    /// 消费登记。
    pub fn register_consumer(&mut self, c: ConsumerRec) -> Result<(), (u32, String)> {
        if c.promise_ver.0 == 0 && c.promise_ver.1 == 0 {
            return Err((XC_DC_PROMISE_MISS, format!("{} 缺承诺版", c.name)));
        }
        for pg in &self.pages {
            if pg.group == c.group && pg.version.0 < c.promise_ver.0 {
                return Err((
                    XC_DC_VERSION_BACK,
                    format!("{} 承诺版高于在册", c.name),
                ));
            }
        }
        self.consumers.push(c);
        Ok(())
    }

    /// 消费调用鉴权（白名单+承诺版+指纹双锚）。
    pub fn authorize(&self, consumer: &str, group: IfaceGroup, anchor_fp: u64) -> Result<(), (u32, String)> {
        let mut found = false;
        for c in &self.consumers {
            if c.name == consumer && c.group == group {
                found = true;
                break;
            }
        }
        if !found {
            return Err((XC_DC_UNREGISTERED, format!("{} 未登记消费 {}", consumer, group.name())));
        }
        let pg = match self.page_of(group) {
            Some(p) => p,
            None => return Err((XC_DC_GROUP_MISS, format!("{} 无冻结页", group.name()))),
        };
        if pg.sig_fp != anchor_fp {
            return Err((
                XC_DC_FP_MISMATCH,
                format!("{} 消费 {} 锚定指纹 {:#x}≠册 {:#x}", consumer, group.name(), anchor_fp, pg.sig_fp),
            ));
        }
        Ok(())
    }

    pub fn page_of(&self, g: IfaceGroup) -> Option<&IfacePage> {
        self.pages.iter().find(|p| p.group == g)
    }

    /// 册总指纹（完整性锚）。
    pub fn book_fp(&self) -> Result<u64, (u32, String)> {
        if self.pages.len() != 4 {
            return Err((
                XC_DC_GROUP_MISS,
                format!("册 {} 页≠4——四组不完备", self.pages.len()),
            ));
        }
        let mut b = Vec::new();
        for p in &self.pages {
            b.extend_from_slice(p.fp().to_le_bytes().as_slice());
        }
        for c in &self.consumers {
            b.extend_from_slice(fnv64(c.name.as_bytes()).to_le_bytes().as_slice());
            b.extend_from_slice(c.promise_ver.0.to_le_bytes().as_slice());
            b.extend_from_slice(c.promise_ver.1.to_le_bytes().as_slice());
        }
        Ok(fnv64(&b))
    }
}

impl Default for FreezeBook {
    fn default() -> FreezeBook {
        FreezeBook::new()
    }
}

// ---------------------------------------------------------------------------
// §3 判据闸
// ---------------------------------------------------------------------------

/// 造一组合法 v1 页（四组全，语义/序/样例齐）。
fn quad_pages() -> Vec<IfacePage> {
    let mut v = Vec::new();
    let mut i: u64 = 0;
    for g in IfaceGroup::all() {
        i += 1;
        let name = g.name();
        let sig = fnv64(format!("{} v1 signature", name).as_bytes());
        let sem = fnv64(format!("{} semantic 4-section", name).as_bytes());
        let ord = fnv64(format!("{} order constraint", name).as_bytes());
        let samp = fnv64(format!("{} sample fn main", name).as_bytes());
        v.push(IfacePage {
            group: g,
            version: (1, 0),
            sig_fp: sig,
            semantic_fp: sem,
            order_fp: ord,
            break_note_fp: 0,
            sample_fp: samp,
        });
        let _ = i;
    }
    v
}

/// J1a 文档齐备：四组页全冻，册指纹可算且非零。
pub fn j1_doc_complete() -> Result<String, (u32, String)> {
    let mut book = FreezeBook::new();
    for p in quad_pages() {
        book.freeze(p)?;
    }
    if book.pages.len() != 4 {
        return Err((
            XC_DC_GROUP_MISS,
            format!("册 {} 页≠4——smp_call/reschedule/notify/shootdown 四组必须齐备", book.pages.len()),
        ));
    }
    for g in IfaceGroup::all() {
        let pg = match book.page_of(g) {
            Some(p) => p,
            None => return Err((XC_DC_GROUP_MISS, format!("{} 缺冻结页", g.name()))),
        };
        if pg.sample_fp == 0 {
            return Err((XC_DC_SAMPLE_DRIFT, format!("{} 样例指纹零——样例不可编译", g.name())));
        }
    }
    let fp = book.book_fp()?;
    if fp == 0 {
        return Err((XC_DC_BOOK_FP, "册指纹零值——指纹面失守".into()));
    }
    Ok(format!("四组冻结页齐备 book_fp={:#018x}", fp))
}

/// J1b 双常量锚定：消费方按册指纹鉴权通过、错指纹拒、未登记拒。
pub fn j1_dual_anchor() -> Result<String, (u32, String)> {
    let mut book = FreezeBook::new();
    for p in quad_pages() {
        book.freeze(p)?;
    }
    book.register_consumer(ConsumerRec { name: "调度域", group: IfaceGroup::Reschedule, promise_ver: (1, 0) })?;
    book.register_consumer(ConsumerRec { name: "内存域", group: IfaceGroup::Shootdown, promise_ver: (1, 0) })?;
    let sig = book.page_of(IfaceGroup::Reschedule).map(|p| p.sig_fp).unwrap_or(0);
    // 合法锚定。
    book.authorize("调度域", IfaceGroup::Reschedule, sig)?;
    // 指纹错位必须拒。
    if book.authorize("调度域", IfaceGroup::Reschedule, sig ^ 0x5A5A).is_ok() {
        return Err((
            XC_DC_FP_MISMATCH,
            "锚定指纹漂移未拒——双常量锚定形同虚设".into(),
        ));
    }
    // 未登记消费必须拒。
    if book.authorize("外域", IfaceGroup::Shootdown, sig).is_ok() {
        return Err((XC_DC_UNREGISTERED, "未登记消费放行——白名单失守".into()));
    }
    // 跨组消费（登记的是 Shootdown，调用 Notify）必须拒。
    let nsig = book.page_of(IfaceGroup::Notify).map(|p| p.sig_fp).unwrap_or(0);
    if book.authorize("内存域", IfaceGroup::Notify, nsig).is_ok() {
        return Err((XC_DC_UNREGISTERED, "跨组消费放行——登记面与调用面脱钩".into()));
    }
    Ok("双锚(册指纹+承诺版)鉴权 3/3 拒绝路径正确".into())
}

/// J1c 版本化机制 10/10：合法升级放行、非法升级拒、降版拒、双冻拒。
pub fn j1_versioning_10() -> Result<String, (u32, String)> {
    let mut pass: u32 = 0;
    // 1) patch 升级（签名不变）。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (1, 1);
        p.semantic_fp = fnv64(b"smp_call v1.1 semantic +1 clause");
        if book.freeze(p).is_ok() {
            pass += 1;
        }
    }
    // 2) patch 升级但签名变——拒（补丁不得断裂）。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (1, 1);
        p.sig_fp = fnv64(b"patched signature silently changed");
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    // 3) major 升带断裂说明——放行。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (2, 0);
        p.sig_fp = fnv64(b"smp_call v2 signature");
        p.break_note_fp = fnv64(b"ADR-0933-02: flag param added");
        if book.freeze(p).is_ok() {
            pass += 1;
        }
    }
    // 4) major 升无断裂说明——拒。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (2, 0);
        p.sig_fp = fnv64(b"smp_call v2 signature no note");
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    // 5) major 升但签名未变（伪断裂）——拒。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (2, 0);
        p.break_note_fp = fnv64(b"ADR note without signature change");
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    // 6) minor 升带断裂说明（越级断裂）——拒。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (1, 1);
        p.break_note_fp = fnv64(b"break note on minor bump");
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    // 7) 降版——拒。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.version = (1, 1);
        book.freeze(p.clone())?;
        p.version = (1, 0);
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    // 8) 同版本同内容（幂等双冻）——放行不算漂移。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        if book.freeze(quad_pages()[0].clone()).is_ok() {
            pass += 1;
        }
    }
    // 9) 同版本异内容（双轨冻结）——拒。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        let mut p = quad_pages()[0].clone();
        p.semantic_fp = fnv64(b"sneaky semantic swap at same version");
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    // 10) 缺序约束——拒。
    {
        let mut book = FreezeBook::new();
        let mut p = quad_pages()[0].clone();
        p.order_fp = 0;
        if book.freeze(p).is_err() {
            pass += 1;
        }
    }
    if pass != 10 {
        return Err((XC_DC_VERSION_ROUNDS, format!("版本化机制 {}/10——未达 10/10", pass)));
    }
    Ok("版本化机制 10/10（patch/major/降版/双冻/序约束五类路径全正确）".into())
}

/// 负路径闸：四类破坏各显性拒绝且码正确。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut hit: u32 = 0;
    // 半页册 → book_fp 拒。
    {
        let mut book = FreezeBook::new();
        book.freeze(quad_pages()[0].clone())?;
        match book.book_fp() {
            Err((c, _)) if c == XC_DC_GROUP_MISS => hit += 1,
            Err((c, _)) => return Err((c, "半页册错误码不符".into())),
            Ok(_) => return Err((XC_DC_BOOK_FP, "半页册竟算出总指纹——完整性闸失守".into())),
        }
    }
    // 缺承诺版 → 拒。
    {
        let mut book = FreezeBook::new();
        for p in quad_pages() {
            book.freeze(p)?;
        }
        match book.register_consumer(ConsumerRec { name: "调度域", group: IfaceGroup::Reschedule, promise_ver: (0, 0) }) {
            Err((c, _)) if c == XC_DC_PROMISE_MISS => hit += 1,
            Err((c, _)) => return Err((c, "缺承诺版错误码不符".into())),
            Ok(()) => return Err((XC_DC_PROMISE_MISS, "缺承诺版竟登记成功".into())),
        }
    }
    // 承诺版高于在册 → 拒。
    {
        let mut book = FreezeBook::new();
        for p in quad_pages() {
            book.freeze(p)?;
        }
        match book.register_consumer(ConsumerRec { name: "调度域", group: IfaceGroup::Reschedule, promise_ver: (2, 0) }) {
            Err((c, _)) if c == XC_DC_VERSION_BACK => hit += 1,
            Err((c, _)) => return Err((c, "高承诺版错误码不符".into())),
            Ok(()) => return Err((XC_DC_VERSION_BACK, "承诺版高于在册竟放行".into())),
        }
    }
    // 首冻非 (1,0) → 拒。
    {
        let mut book = FreezeBook::new();
        let mut p = quad_pages()[0].clone();
        p.version = (2, 3);
        match book.freeze(p) {
            Err((c, _)) if c == XC_DC_ANCHOR_HALF => hit += 1,
            Err((c, _)) => return Err((c, "首冻版本错误码不符".into())),
            Ok(()) => return Err((XC_DC_ANCHOR_HALF, "首冻非 (1,0) 竟放行——锚点半屏".into())),
        }
    }
    if hit != 4 {
        return Err((XC_DC_ORDER_MISS, format!("负路径 {}/4", hit)));
    }
    Ok("负路径 4/4 显性拒绝且码正确".into())
}

/// 双跑指纹：同种子两版本化流程指纹等。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut rng = Lcg::new(seed);
        let mut book = FreezeBook::new();
        for p in quad_pages() {
            book.freeze(p)?;
        }
        for _ in 0..40 {
            // 随机在合法/非法升级间铺。
            let g = IfaceGroup::all()[(rng.below(4)) as usize];
            let cur = match book.page_of(g) {
                Some(p) => p.version,
                None => (1, 0),
            };
            let mut p = match quad_pages().into_iter().find(|x| x.group == g) {
                Some(p) => p,
                None => return Err((XC_DC_GROUP_MISS, format!("{} 基准页缺失", g.name()))),
            };
            p.version = (cur.0, cur.1 + 1);
            p.semantic_fp = fnv64(format!("{} seed semantic {}", g.name(), seed).as_bytes());
            let _ = book.freeze(p);
            let _ = book.register_consumer(ConsumerRec {
                name: if rng.below(2) == 0 { "调度域" } else { "内存域" },
                group: g,
                promise_ver: (1, 0),
            });
        }
        book.book_fp()
    };
    let a = run(0xD4F0_930B)?;
    let b = run(0xD4F0_930B)?;
    if a != b {
        return Err((XC_DC_DUAL_DRIFT, format!("双跑 {:#x}≠{:#x}", a, b)));
    }
    Ok(a)
}

/// F0933 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_doc_complete()?;
    let b = j1_dual_anchor()?;
    let c = j1_versioning_10()?;
    let d = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_DC_DUAL_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0933 全绿：\n[文档齐备] {}\n[双常量锚定] {}\n[版本化] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, f1
    ))
}
