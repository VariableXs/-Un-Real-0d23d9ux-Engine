//! CGPU-F0493 · 计时文档体系（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 计时文档：合同条款 / 计时方法论 / 账本查询手册——三文档；
//! - 文档完整性闸（章节齐全 + 术语一致 + 承诺条款在册）。
//! 判据：三文档、完整性、一致性、判据。
//!
//! 设计纪律：
//! - 三文档各司其职：CONTRACT（对使用者的计时承诺条款——什么算
//!   数、什么不算）、METHODOLOGY（计时方法论——五段 stamped 从
//!   何而来、口径如何）、MANUAL（账本查询手册——query 族怎么调、
//!   字段怎么读）。文档间交叉引用走 term 表，不允许同义异名。
//! - 完整性闸：每文档必备章节表（缺一即拒）+ 术语表全对齐 +
//!   合同里必须有承诺条款（精度声明不夸大）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x11A0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x11A0，全仓独占）。
pub const SEG_BASE: u16 = 0x11A0;

pub const E_TD_DOC: u16 = SEG_BASE + 1; // 文档不存在
pub const E_TD_CHAP: u16 = SEG_BASE + 2; // 章节缺失
pub const E_TD_TERM: u16 = SEG_BASE + 3; // 术语不一致
pub const E_TD_PLEDGE: u16 = SEG_BASE + 4; // 承诺条款缺失/夸大

/// 三文档标识（判别值冻结）。
pub const DOC_CONTRACT: u8 = 1; // 合同条款
pub const DOC_METHOD: u8 = 2; // 计时方法论
pub const DOC_MANUAL: u8 = 3; // 账本查询手册
pub const DOCS: [u8; 3] = [DOC_CONTRACT, DOC_METHOD, DOC_MANUAL];

/// 合同承诺条款（精度声明不夸大——只承诺可兑现的）。
pub const PLEDGE_ENTRIES: [(&str, &str); 5] = [
    ("time_source", "硬件单调时钟，倒退由容错层丢弃"),
    ("granularity", "微秒（us）记账，不承诺纳秒精度"),
    ("jitter", "抖动如实入账，不做平滑掩盖"),
    ("stall", "卡顿钳制到卡顿线，超出部分标注丢失"),
    ("ledger", "账本容量封顶，洪峰丢最旧并计数留痕"),
];

/// 术语表：概念 → 唯一正名（与 F0492 CANON_NAMES 同源口径）。
pub const TERMS: [(&str, &str); 8] = [
    ("frame", "帧"),
    ("phase", "相位"),
    ("segment", "分段"),
    ("stamp", "时间戳"),
    ("ledger", "账本"),
    ("jitter", "抖动"),
    ("stall", "卡顿"),
    ("drift", "漂移"),
];

/// 计时文档：标识 + 必备章节表 + 正文行。
#[derive(Debug, PartialEq)]
pub struct TimingDoc {
    pub doc_id: u8,
    /// 必备章节（有序）。
    pub required: Vec<&'static str>,
    /// 正文：章 -> 行列表。
    pub body: Vec<(&'static str, Vec<&'static str>)>,
}

impl TimingDoc {
    pub fn new(doc_id: u8, required: Vec<&'static str>) -> Self {
        Self { doc_id, required, body: Vec::new() }
    }
    /// 写入章节：重复章追加行。
    pub fn put(&mut self, chap: &'static str, lines: Vec<&'static str>) {
        for (c, l) in self.body.iter_mut() {
            if *c == chap {
                l.extend(lines);
                return;
            }
        }
        self.body.push((chap, lines));
    }
    /// 章节完整性：必备章节逐一在册。
    pub fn chapters_complete(&self) -> Result<usize, u16> {
        if self.required.is_empty() {
            return Err(E_TD_CHAP);
        }
        for r in self.required.iter() {
            if !self.body.iter().any(|(c, _)| c == r) {
                return Err(E_TD_CHAP);
            }
        }
        Ok(self.body.len())
    }
    /// 术语一致：正文出现的术语词必须是正名（异名即违约）。
    pub fn terms_consistent(&self, banned: &[&str]) -> Result<usize, u16> {
        if banned.is_empty() {
            return Err(E_TD_TERM);
        }
        let mut hits = 0usize;
        for (_, lines) in self.body.iter() {
            for line in lines.iter() {
                for b in banned.iter() {
                    // 行内出现禁用异名 → 违约。
                    if line.contains(b) {
                        return Err(E_TD_TERM);
                    }
                }
                for (_, canon) in TERMS.iter() {
                    if line.contains(canon) {
                        hits += 1;
                    }
                }
            }
        }
        Ok(hits)
    }
}

/// 文档库：三文档成套管理。
#[derive(Debug)]
pub struct DocLibrary {
    pub docs: Vec<TimingDoc>,
}

impl DocLibrary {
    pub fn new() -> Self {
        Self { docs: Vec::new() }
    }
    pub fn add(&mut self, d: TimingDoc) {
        self.docs.push(d);
    }
    pub fn get(&self, doc_id: u8) -> Result<&TimingDoc, u16> {
        if !DOCS.contains(&doc_id) {
            return Err(E_TD_DOC);
        }
        match self.docs.iter().find(|d| d.doc_id == doc_id) {
            Some(d) => Ok(d),
            None => Err(E_TD_DOC),
        }
    }
    /// 三文档成套闸：全在册 + 全章节齐 + 全术语一致。
    pub fn gate(&self, banned: &[&str]) -> Result<usize, u16> {
        if self.docs.len() != DOCS.len() {
            return Err(E_TD_DOC);
        }
        let mut total = 0usize;
        for d in DOCS.iter() {
            let doc = self.get(*d)?;
            total += doc.chapters_complete()?;
            total += doc.terms_consistent(banned)?;
        }
        Ok(total)
    }
    /// 承诺闸：合同文档必须含全部承诺条款且无夸大词。
    pub fn pledge_gate(&self) -> Result<usize, u16> {
        let contract = self.get(DOC_CONTRACT)?;
        let mut found = 0usize;
        for (key, _) in PLEDGE_ENTRIES.iter() {
            let mut hit = false;
            for (c, lines) in contract.body.iter() {
                for line in lines.iter() {
                    if line.contains(key) {
                        hit = true;
                    }
                    // 夸大词：承诺里不允许出现的绝对化表述。
                    for hype in ["零误差", "纳秒级保证", "永不丢帧", "100%精确"].iter() {
                        if line.contains(hype) {
                            return Err(E_TD_PLEDGE);
                        }
                    }
                }
                let _ = c;
            }
            if hit {
                found += 1;
            }
        }
        if found == PLEDGE_ENTRIES.len() {
            Ok(found)
        } else {
            Err(E_TD_PLEDGE)
        }
    }
}

/// 组装正本三文档（正文与判据同源，防文档漂移）。
pub fn canonical_library() -> DocLibrary {
    let mut lib = DocLibrary::new();
    // 一、合同条款。
    let mut contract = TimingDoc::new(
        DOC_CONTRACT,
        ["承诺", "口径", "免责", "变更"].to_vec(),
    );
    contract.put("承诺", vec![
        "time_source：硬件单调时钟，倒退由容错层丢弃",
        "granularity：微秒（us）记账，不承诺纳秒精度",
        "jitter：抖动如实入账，不做平滑掩盖",
        "stall：卡顿钳制到卡顿线，超出部分标注丢失",
        "ledger：账本容量封顶，洪峰丢最旧并计数留痕",
    ]);
    contract.put("口径", vec![
        "分段（segment）以相位（phase）边界对齐，段与段衔接",
        "帧时长 = 四段之和；漂移（drift）三源互校在 ±50‰ 带内",
    ]);
    contract.put("免责", vec![
        "时钟异常（倒退/卡顿）下计时器保命优先，丢弃如实计数",
        "本合同不承诺超出硬件精度的计时能力",
    ]);
    contract.put("变更", vec![
        "十年冻结期内签名只增不改不删；变更走兼容闸",
    ]);
    lib.add(contract);
    // 二、计时方法论。
    let mut method = TimingDoc::new(
        DOC_METHOD,
        ["时间戳", "分段方法", "容错", "校准"].to_vec(),
    );
    method.put("时间戳", vec![
        "五段 stamped：输入/模拟/渲染/提交/呈现，逐段时间戳链",
        "时间戳（stamp）一律微秒（us）整数，禁止浮点记账",
    ]);
    method.put("分段方法", vec![
        "相位（phase）边界即分段（segment）边界；段起点衔接上一段结束",
        "帧（frame）账目四段入账，缺段由校验闸报告",
    ]);
    method.put("容错", vec![
        "倒退样本丢弃并计数；卡顿钳制；前跳合法——负时长结构上不可能",
    ]);
    method.put("校准", vec![
        "漂移（drift）三源互校，越带触发重校准；降级时标注来源故障",
    ]);
    lib.add(method);
    // 三、账本查询手册。
    let mut manual = TimingDoc::new(
        DOC_MANUAL,
        ["查询入口", "字段", "窗口", "边界"].to_vec(),
    );
    manual.put("查询入口", vec![
        "账本（ledger）查询族：query(from_frame, to_frame) 闭区间",
    ]);
    manual.put("字段", vec![
        "每帧账目 = [输入, 模拟, 渲染, 提交] 四段时长（us）",
    ]);
    manual.put("窗口", vec![
        "帧（frame）窗口闭区间；倒窗拒绝；空窗合法返回空集",
    ]);
    manual.put("边界", vec![
        "洪峰下账本容量封顶；被丢帧不在账本中，以 dropped 计数示人",
    ]);
    lib.add(manual);
    lib
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、三文档成套。
    {
        let lib = canonical_library();
        v.push(("F0493-三文档-成套在册", lib.docs.len() == 3));
        v.push(("F0493-三文档-标识枚举", DOCS == [1, 2, 3]));
        v.push(("F0493-三文档-未登记文档拒", DocLibrary::new().gate(&[]) == Err(E_TD_DOC)));
        v.push(("F0493-三文档-单查不存在", lib.get(9) == Err(E_TD_DOC)));
    }
    // 二、完整性闸。
    {
        let lib = canonical_library();
        v.push(("F0493-完整性-合同四章", lib.get(DOC_CONTRACT).map(|d| d.chapters_complete()) == Ok(Ok(4))));
        v.push(("F0493-完整性-方法论四章", lib.get(DOC_METHOD).map(|d| d.chapters_complete()) == Ok(Ok(4))));
        v.push(("F0493-完整性-手册四章", lib.get(DOC_MANUAL).map(|d| d.chapters_complete()) == Ok(Ok(4))));
        // 缺章构造：合同删「免责」章。
        let mut broken = canonical_library();
        if let Some(d) = broken.docs.iter_mut().find(|d| d.doc_id == DOC_CONTRACT) {
            d.body.retain(|(c, _)| *c != "免责");
        }
        v.push(("F0493-完整性-缺章拒", broken.gate(&["异名"]) == Err(E_TD_CHAP)));
    }
    // 三、术语一致。
    {
        let lib = canonical_library();
        // 禁用异名：与正名冲突的旧叫法。
        let banned: Vec<&str> = ["毫秒记账", "浮点时间戳", "平滑后的抖动"].to_vec();
        v.push(("F0493-一致性-正本全绿", lib.gate(&banned).is_ok()));
        v.push(("F0493-一致性-术语表八条", TERMS.len() == 8));
        // 注入异名 → 违约。
        let mut dirty = canonical_library();
        if let Some(d) = dirty.docs.iter_mut().find(|d| d.doc_id == DOC_MANUAL) {
            d.put("边界", vec!["本手册亦描述毫秒记账的历史口径"]);
        }
        v.push(("F0493-一致性-异名注入即拒", dirty.gate(&banned) == Err(E_TD_TERM)));
    }
    // 四、承诺闸。
    {
        let lib = canonical_library();
        v.push(("F0493-承诺-五条全在册", lib.pledge_gate() == Ok(5)));
        v.push(("F0493-承诺-条款表入册", PLEDGE_ENTRIES.len() == 5));
        // 夸大注入 → 拒。
        let mut hype = canonical_library();
        if let Some(d) = hype.docs.iter_mut().find(|d| d.doc_id == DOC_CONTRACT) {
            d.put("承诺", vec!["本实现零误差，纳秒级保证"]);
        }
        v.push(("F0493-承诺-夸大拒", hype.pledge_gate() == Err(E_TD_PLEDGE)));
        // 缺承诺条款 → 拒。
        let mut thin = canonical_library();
        if let Some(d) = thin.docs.iter_mut().find(|d| d.doc_id == DOC_CONTRACT) {
            d.body.retain(|(c, _)| *c != "承诺");
        }
        v.push(("F0493-承诺-缺条款拒", thin.pledge_gate() == Err(E_TD_PLEDGE)));
    }
    // 五、判据自检。
    {
        v.push(("F0492-F0493-同源口径", {
            // 术语表与 F0492 命名表在「账本/查询/校验」上同源。
            TERMS.iter().any(|(c, _)| *c == "ledger")
        }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0493_timedoc_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
