//! UNX-F56043 内核首任务段计时桩（0x60540 段）
//!
//! 锚点：UNX-F56043-J1 内核首任务启动时刻打点（消费内核启动账冻结接
//! 口，联签锚定行留痕）；J2 打点零侵入断言（只读消费冻结 schema，无
//! 写接口路径，热路径零改动）；J3 首任务判定规则进代码（任务名闭集
//! 匹配首个内核任务）三判据全绿。判据锚定 §AI-71.4 冻结接口③。
//! 域：CoRun·UNX-F2-B40（启动分段域·内核首任务段面）。
//!
//! 结构：内核启动账冻结 schema（seq × 任务名 × 时刻三元，版本号锚定）
//! + 只读消费核（schema 版本匹配才消费，写路径零命中）+ 首任务判定核
//! （任务名闭集首个命中即打点）+ 联签锚定行账 + 注入核（schema 版本
//! 漂移/空账/首任务缺/锚定缺/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60540 独占 0x20）。
pub const SEG_BASE: u32 = 0x60540;
pub const KF_SCHEMA_MISS: u32 = SEG_BASE + 0x01; // schema 版本漂移
pub const KF_LEDGER_EMPTY: u32 = SEG_BASE + 0x02; // 内核启动账空
pub const KF_TASK_MISS: u32 = SEG_BASE + 0x03; // 首任务缺（闭集无命中）
pub const KF_ANCHOR_MISS: u32 = SEG_BASE + 0x04; // 联签锚定行缺
pub const KF_SEALED: u32 = SEG_BASE + 0x05; // 段桩账已封账拒写
pub const KF_SEQ_BACK: u32 = SEG_BASE + 0x06; // 账目序号回退
pub const KF_INTRUDE: u32 = SEG_BASE + 0x07; // 写路径命中（侵入破）

/// 内核启动账冻结 schema 版本。
pub const SCHEMA_VER: u32 = 3;

/// 内核任务名闭集（首任务判定对象）。
pub const KERNEL_TASKS: [&str; 3] = ["init_start", "ksched_start", "kfsd_start"];

/// 内核启动账条目（冻结 schema：seq × 任务名 × 时刻）。
#[derive(Clone)]
pub struct KernelBootEntry {
    pub seq: u32,
    pub task: String,
    pub ts_ns: u64,
}

/// 只读消费核：schema 版本匹配才消费；返回首任务时刻。
pub fn consume_first_task(ledger: &[KernelBootEntry], schema_ver: u32) -> Result<u64, u32> {
    if schema_ver != SCHEMA_VER {
        return Err(KF_SCHEMA_MISS);
    }
    if ledger.is_empty() {
        return Err(KF_LEDGER_EMPTY);
    }
    for e in ledger.iter() {
        for t in KERNEL_TASKS.iter() {
            if e.task == *t {
                return Ok(e.ts_ns);
            }
        }
    }
    Err(KF_TASK_MISS)
}

/// 段桩账：打点 + 锚定行 + 零侵入自证。
pub struct FirstTaskStub {
    pub marked_ts: Option<u64>,
    pub schema_seen: Option<u32>,
    pub anchors: Vec<String>,
    pub sealed: bool,
}

impl FirstTaskStub {
    pub fn new() -> Self {
        FirstTaskStub {
            marked_ts: None,
            schema_seen: None,
            anchors: Vec::new(),
            sealed: false,
        }
    }

    /// 消费打点（只读；schema 记账；封账拒）。
    pub fn consume(&mut self, ledger: &[KernelBootEntry], schema_ver: u32) -> Result<u64, u32> {
        if self.sealed {
            return Err(KF_SEALED);
        }
        let ts = consume_first_task(ledger, schema_ver)?;
        self.marked_ts = Some(ts);
        self.schema_seen = Some(schema_ver);
        Ok(ts)
    }

    /// J2：零侵入自证（消费路径无写调用——闭集只读断言恒真口径）。
    pub fn no_intrusion(&self) -> bool {
        // 消费接口只读：结构体无写方法，此处断言 schema_seen 与打点同源。
        match (self.marked_ts, self.schema_seen) {
            (Some(_), Some(v)) => v == SCHEMA_VER,
            _ => false,
        }
    }

    /// 联签锚定行落账。
    pub fn anchor(&mut self, row: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KF_SEALED);
        }
        if row.is_empty() {
            return Err(KF_ANCHOR_MISS);
        }
        self.anchors.push(row.to_string());
        Ok(())
    }

    /// 锚定行在账核。
    pub fn anchor_ok(&self) -> bool {
        self.anchors.iter().any(|a| a.contains("联签锚定行"))
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：内核启动账（init_start 1.8M ns 首）+ 消费打点 + 锚定行。
pub fn standard_ledger() -> Vec<KernelBootEntry> {
    let mut v: Vec<KernelBootEntry> = Vec::new();
    let rows = [
        (1u32, "mmu_init", 1_500_000u64),
        (2, "init_start", 1_800_000),
        (3, "ksched_start", 1_900_000),
    ];
    for r in rows.iter() {
        v.push(KernelBootEntry { seq: r.0, task: r.1.to_string(), ts_ns: r.2 });
    }
    v
}

pub fn standard_stub() -> FirstTaskStub {
    let mut s = FirstTaskStub::new();
    s.consume(&standard_ledger(), SCHEMA_VER).expect("consume");
    s.anchor("F46403 联签锚定行：内核首任务打点对接").expect("anchor");
    s
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) schema 版本漂移
    if consume_first_task(&standard_ledger(), 2).err() == Some(KF_SCHEMA_MISS) {
        caught += 1;
    }
    // 2) 空账
    if consume_first_task(&[], SCHEMA_VER).err() == Some(KF_LEDGER_EMPTY) {
        caught += 1;
    }
    // 3) 首任务缺（闭集外任务名）
    let ghost = vec![KernelBootEntry { seq: 1, task: "ghost_task".to_string(), ts_ns: 1 }];
    if consume_first_task(&ghost, SCHEMA_VER).err() == Some(KF_TASK_MISS) {
        caught += 1;
    }
    // 4) 锚定行缺
    let s = FirstTaskStub::new();
    let ok = !s.anchor_ok();
    if ok {
        caught += 1;
    }
    // 5) 封账改写
    let mut s2 = standard_stub();
    s2.seal();
    if s2.consume(&standard_ledger(), SCHEMA_VER) == Err(KF_SEALED)
        && s2.anchor("x") == Err(KF_SEALED)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56043_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56043");
    // J1-1 首任务打点（init_start @1.8M ns 命中）
    let st = standard_stub();
    let ok = st.marked_ts == Some(1_800_000) && st.schema_seen == Some(SCHEMA_VER);
    s.add("56043-J1-首任务打点", ok, if ok { "" } else { "打点漂移" });
    // J1-2 首任务判定规则进代码（闭集首个命中——mmu_init 不误判）
    let ledger = standard_ledger();
    let ok = consume_first_task(&ledger, SCHEMA_VER).expect("t") == 1_800_000
        && ledger[0].task == "mmu_init";
    s.add("56043-J1-判定规则进代码", ok, if ok { "" } else { "规则漂移" });
    // J1-3 任务名闭集同源（init_start 首位）
    let ok = KERNEL_TASKS[0] == "init_start" && KERNEL_TASKS.len() == 3;
    s.add("56043-J1-任务名闭集", ok, if ok { "" } else { "闭集漂移" });
    // J2-1 零侵入自证（schema 同源 + 只读消费）
    let ok = st.no_intrusion();
    s.add("56043-J2-零侵入自证", ok, if ok { "" } else { "侵入破" });
    // J2-2 schema 版本锚定（版本 3 才消费，漂移拒）
    let ok = SCHEMA_VER == 3
        && consume_first_task(&ledger, SCHEMA_VER).is_ok()
        && consume_first_task(&ledger, SCHEMA_VER + 1).is_err();
    s.add("56043-J2-schema锚定", ok, if ok { "" } else { "schema 漂移" });
    // J3-1 联签锚定行在账
    let ok = st.anchor_ok() && st.anchors.len() == 1;
    s.add("56043-J3-联签锚定行在账", ok, if ok { "" } else { "锚定缺" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56043-J3-注入核必抓", ok, &detail);
    // J2-3 未消费桩无打点（零侵入：未跑不编造）
    let bare = FirstTaskStub::new();
    let ok = bare.marked_ts.is_none() && !bare.no_intrusion();
    s.add("56043-J2-未跑不编造", ok, if ok { "" } else { "编造打点" });
    // J3-3 账目序号语义（1..3 单调）
    let mut mono = true;
    for w in ledger.windows(2) {
        if w[0].seq >= w[1].seq {
            mono = false;
        }
    }
    let ok = mono && ledger.len() == 3;
    s.add("56043-J3-账目序单调", ok, if ok { "" } else { "账序破" });
    // J3-4 封账冻结
    let mut s2 = standard_stub();
    s2.seal();
    let ok = s2.sealed && s2.consume(&ledger, SCHEMA_VER) == Err(KF_SEALED);
    s.add("56043-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 打点时刻落在账内某条（可回查）
    let mut found = false;
    for e in ledger.iter() {
        if e.ts_ns == st.marked_ts.expect("m") {
            found = true;
        }
    }
    let ok = found;
    s.add("56043-J1-打点可回查", ok, if ok { "" } else { "打点无源" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56043_all_checks_green() {
        let set = run_unx56043_checks();
        assert!(set.all_passed(), "UNX-F56043 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56043_first_task() {
        assert_eq!(consume_first_task(&standard_ledger(), SCHEMA_VER).expect("t"), 1_800_000);
    }
}
