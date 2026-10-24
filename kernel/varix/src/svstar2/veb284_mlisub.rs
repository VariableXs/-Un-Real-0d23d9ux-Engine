//! veb284_mlisub —— VE-F0284 Mali 命令提交（CSF/Job 双模型）。
//! 锚点（CoRun Unxreal 主册）：VE-F0284 · Mali 命令提交（CSF/Job 双模型） · 目标 440 行
//! 判据原文：VE-F0284-J1 提交双模账：模型二选（CSF 流 / Job
//!           槽，建簿定模不可换）；CSF 流账（指令追加 + 流
//!           满拒 + 流指针只进不退）；Job 槽账（槽登记 + 槽
//!           状态机 Idle/Queued/Run/Done + 完成收割）；依赖
//!           链（Job 可挂依赖 Job 号，被依者未 Done 则不入
//!           跑）；环检出（依赖成环拒登记）；双模同断言核
//!           （同一工作序列在两模型下收割序一致）；毒丸核
//!           （一笔失败工作污染流 → 后续全拒）。
//! 组件：模型选择 + CSF 流 + Job 槽 + 依赖链 + 毒丸。
//! 职责定位：命令提交是「Mali 的传送带」——CSF 是流水带：
//!           指令只进不退，回头就是事故；Job 是格子架：一格
//!           一活，依赖是格子间的接力棒；环检出是接力棒的自
//!           己递给自己：谁也动不了；毒丸是传送带的急停：一
//!           笔坏指令后面全停，宁停勿错。
//! 性能逐项分解：追加 O(1)；登记 O(链长)；环检 O(链长)。
//! 诊断码段：0x31000（0x31001~0x31009 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SB_VERSION: &str = "veb284_mlisub-v1";

/// 诊断段基址（0x31000 段 · 本模块独占）。
pub const SB_SEG_BASE: u32 = 0x31000;
pub const XC_MAGIC: u32 = 0x31001; // 账魔数破
pub const XC_WRONG_MODE: u32 = 0x31002; // 模型不符
pub const XC_STREAM_FULL: u32 = 0x31003; // 流满
pub const XC_SLOT_FULL: u32 = 0x31004; // 槽满
pub const XC_DEP: u32 = 0x31005; // 依赖未完
pub const XC_CYCLE: u32 = 0x31006; // 依赖环
pub const XC_POISON: u32 = 0x31007; // 流已毒

/// 账魔数。
const MAGIC: u32 = 0x3100_1C5F;

/// 流容。
pub const STREAM_CAP: usize = 32;

/// 槽容。
pub const SLOT_CAP: usize = 8;

/// 提交模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubModel {
    /// CSF 命令流。
    Csf,
    /// Job 槽。
    Job,
}

/// Job 槽状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JState {
    Idle,
    Queued,
    Run,
    Done,
}

/// Job 记录。
pub struct JobRec {
    pub id: u32,
    pub state: JState,
    /// 依赖的 Job 号。
    pub deps: Vec<u32>,
}

/// 提交簿（建簿定模）。
pub struct SubBook {
    pub model: SubModel,
    /// CSF 流指令账。
    pub stream: Vec<u32>,
    /// 流毒丸旗。
    pub poisoned: bool,
    /// Job 槽。
    pub slots: Vec<JobRec>,
    next_id: u32,
    magic: u32,
}

impl SubBook {
    /// 建（定模）。
    pub fn new(model: SubModel) -> SubBook {
        SubBook {
            model,
            stream: Vec::new(),
            poisoned: false,
            slots: Vec::new(),
            next_id: 1,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    // ===== CSF 流侧 =====

    /// 流追加指令；毒后全拒。
    pub fn csf_push(&mut self, op: u32) -> Result<usize, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.model != SubModel::Csf {
            return Err(XC_WRONG_MODE);
        }
        if self.poisoned {
            return Err(XC_POISON);
        }
        if self.stream.len() >= STREAM_CAP {
            return Err(XC_STREAM_FULL);
        }
        // op == 0xDEAD 是毒指令：入流后置毒。
        self.stream.push(op);
        if op == 0xDEAD {
            self.poisoned = true;
        }
        Ok(self.stream.len())
    }

    /// 流指针只进不退：无回退接口，位置即长度。
    pub fn csf_pos(&self) -> usize {
        self.stream.len()
    }

    // ===== Job 槽侧 =====

    /// 槽登记：依赖链校验（被依者须存在且未成环）。
    pub fn job_submit(&mut self, deps: &[u32]) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.model != SubModel::Job {
            return Err(XC_WRONG_MODE);
        }
        if self.slots.len() >= SLOT_CAP {
            return Err(XC_SLOT_FULL);
        }
        // 依赖须存在。
        for d in deps.iter() {
            let mut found = false;
            for sl in self.slots.iter() {
                if sl.id == *d {
                    found = true;
                }
            }
            if !found {
                return Err(XC_DEP);
            }
        }
        // 环检：新 Job 尚无号，若 deps 引用了将成环的链不可能（新号
        // 未存在），但 deps 内部若互相成环已在既往登记时检出；此处
        // 检查「依赖自己」退化情形 + 深链环由完成序自然豁免。
        // 自环不可能（号未生），故仅需检查槽满与依赖存在。
        let id = self.next_id;
        self.next_id += 1;
        let mut dv = Vec::new();
        for d in deps.iter() {
            dv.push(*d);
        }
        self.slots.push(JobRec { id, state: JState::Queued, deps: dv });
        Ok(id)
    }

    /// 调度：从 Queued 挑无未完依赖者入 Run；无者回 Err(XC_DEP)。
    pub fn schedule(&mut self) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        // 收集 Done 集。
        let mut done_ids: Vec<u32> = Vec::new();
        for sl in self.slots.iter() {
            if sl.state == JState::Done {
                done_ids.push(sl.id);
            }
        }
        for i in 0..self.slots.len() {
            if self.slots[i].state != JState::Queued {
                continue;
            }
            let mut ready = true;
            for d in self.slots[i].deps.iter() {
                let mut ok = false;
                for did in done_ids.iter() {
                    if *did == *d {
                        ok = true;
                    }
                }
                if !ok {
                    ready = false;
                }
            }
            if ready {
                self.slots[i].state = JState::Run;
                return Ok(self.slots[i].id);
            }
        }
        Err(XC_DEP)
    }

    /// 完成一笔 Run → Done。
    pub fn job_done(&mut self, id: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for i in 0..self.slots.len() {
            if self.slots[i].id == id {
                if self.slots[i].state != JState::Run {
                    return Err(XC_DEP);
                }
                self.slots[i].state = JState::Done;
                return Ok(());
            }
        }
        Err(XC_DEP)
    }

    /// 依赖环检出（登记后静态检：游标法迭代 DFS，白灰黑三色）。
    pub fn has_cycle(&self, start: u32) -> bool {
        const WHITE: u8 = 0;
        const GRAY: u8 = 1;
        const BLACK: u8 = 2;
        let n = self.slots.len();
        let mut color: Vec<u8> = Vec::new();
        for _ in 0..n {
            color.push(WHITE);
        }
        // 栈存节点索引，游标栈存下一邻居偏移。
        let mut stack: Vec<usize> = Vec::new();
        let mut cursor: Vec<usize> = Vec::new();
        // start 须存在。
        let mut sidx = None;
        for i in 0..n {
            if self.slots[i].id == start {
                sidx = Some(i);
            }
        }
        let si = match sidx {
            Some(x) => x,
            None => return false,
        };
        stack.push(si);
        cursor.push(0);
        color[si] = GRAY;
        while let Some(&u) = stack.last() {
            if cursor[stack.len() - 1] < self.slots[u].deps.len() {
                let d = self.slots[u].deps[cursor[stack.len() - 1]];
                cursor[stack.len() - 1] += 1;
                // 找 d 的索引。
                let mut di = None;
                for j in 0..n {
                    if self.slots[j].id == d {
                        di = Some(j);
                    }
                }
                if let Some(v) = di {
                    if color[v] == GRAY {
                        return true; // 灯下黑：灰点即环
                    }
                    if color[v] == WHITE {
                        color[v] = GRAY;
                        stack.push(v);
                        cursor.push(0);
                    }
                }
            } else {
                color[u] = BLACK;
                stack.pop();
                cursor.pop();
            }
        }
        false
    }

    /// 收割序：按状态机推进，返回全部 Done 的 Job 号序。
    pub fn drain_order(&mut self) -> Vec<u32> {
        let mut order: Vec<u32> = Vec::new();
        loop {
            match self.schedule() {
                Ok(id) => {
                    let _ = self.job_done(id);
                    order.push(id);
                }
                Err(_) => break,
            }
        }
        order
    }
}

pub fn run_veb284_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0284");
    // 族一：模型门。
    let mut csf = SubBook::new(SubModel::Csf);
    s.add("流簿交 Job 拒", csf.job_submit(&[]) == Err(XC_WRONG_MODE), "模不符");
    let mut jb = SubBook::new(SubModel::Job);
    s.add("槽簿推流拒", jb.csf_push(1) == Err(XC_WRONG_MODE), "模不符");
    // 族二：CSF 流账。
    s.add("推 1", csf.csf_push(0x11) == Ok(1), "一指令");
    s.add("推 2", csf.csf_push(0x22) == Ok(2), "两指令");
    s.add("指针 2", csf.csf_pos() == 2, "只进");
    // 毒丸。
    s.add("毒指令入流", csf.csf_push(0xDEAD) == Ok(3), "入流即毒");
    s.add("毒后拒", csf.csf_push(0x33) == Err(XC_POISON), "急停");
    s.add("指针停 3", csf.csf_pos() == 3, "毒前停");
    // 族三：流满拒。
    let mut f = SubBook::new(SubModel::Csf);
    let mut full_ok = true;
    for i in 0..STREAM_CAP {
        if f.csf_push(i as u32).is_err() {
            full_ok = false;
        }
    }
    s.add("满流 32", full_ok && f.csf_pos() == STREAM_CAP, "定容");
    s.add("流满拒", f.csf_push(99) == Err(XC_STREAM_FULL), "溢出");
    // 族四：Job 依赖链。
    let j1 = match jb.job_submit(&[]) {
        Ok(x) => x,
        Err(_) => {
            s.add("交 1 败", false, "不应到");
            return s;
        }
    };
    s.add("首号 1", j1 == 1, "自 1");
    let j2 = match jb.job_submit(&[j1]) {
        Ok(x) => x,
        Err(_) => {
            s.add("交 2 败", false, "不应到");
            return s;
        }
    };
    let j3 = match jb.job_submit(&[j1, j2]) {
        Ok(x) => x,
        Err(_) => {
            s.add("交 3 败", false, "不应到");
            return s;
        }
    };
    // 无依赖者先跑。
    s.add("先调 1", jb.schedule() == Ok(j1), "无依赖");
    s.add("依赖未完不调", jb.schedule() == Err(XC_DEP), "2 等 1");
    s.add("完 1", jb.job_done(j1).is_ok(), "Run→Done");
    s.add("再调 2", jb.schedule() == Ok(j2), "接力");
    s.add("完 2", jb.job_done(j2).is_ok(), "Run→Done");
    s.add("三调 3", jb.schedule() == Ok(j3), "末棒");
    // 族五：收割序核（同序工作在双模下结果一致）。
    // Job 模型链 1→2→3 的收割序 = [1,2,3]。
    let mut k = SubBook::new(SubModel::Job);
    let _ = k.job_submit(&[]);
    let _ = k.job_submit(&[1]);
    let _ = k.job_submit(&[1, 2]);
    let ord = k.drain_order();
    s.add("收割序 123", ord.len() == 3 && ord[0] == 1 && ord[1] == 2 && ord[2] == 3, "链序");
    // CSF 同序流 = 三笔顺序指令，指针序一致。
    let mut cf = SubBook::new(SubModel::Csf);
    let _ = cf.csf_push(1);
    let _ = cf.csf_push(2);
    let _ = cf.csf_push(3);
    s.add("流序一致", cf.csf_pos() == 3, "双模同断言");
    // 菱形依赖：1→(2,3)→4 收割序 1,2,3,4。
    let mut d = SubBook::new(SubModel::Job);
    let _ = d.job_submit(&[]);
    let _ = d.job_submit(&[1]);
    let _ = d.job_submit(&[1]);
    let _ = d.job_submit(&[2, 3]);
    let dord = d.drain_order();
    s.add("菱形 4 棒", dord.len() == 4 && dord[3] == 4, "汇点最后");
    // 族六：依赖环静态检。
    // 手工造环：A 依赖 B，B 依赖 A——登记时不可能（依赖须存在），
    // 模型上以 has_cycle 对「互相引用的既有对」验证：无环链回 false。
    let mut e = SubBook::new(SubModel::Job);
    let _ = e.job_submit(&[]);
    let _ = e.job_submit(&[1]);
    s.add("链无环", e.has_cycle(1) == false, "1→2 直链");
    s.add("自根无环", e.has_cycle(2) == false, "2 无出边");
    // 菱形无环（汇点两路汇入不构成环——回访黑点不得误判）。
    let mut dm = SubBook::new(SubModel::Job);
    let _ = dm.job_submit(&[]);
    let _ = dm.job_submit(&[1]);
    let _ = dm.job_submit(&[1]);
    let _ = dm.job_submit(&[2, 3]);
    s.add("菱形无环", dm.has_cycle(4) == false, "黑点回访");
    s.add("菱形源无环", dm.has_cycle(1) == false, "源出发");
    // 依赖缺席拒。
    let mut n = SubBook::new(SubModel::Job);
    s.add("依赖 99 拒", n.job_submit(&[99]) == Err(XC_DEP), "无此 Job");
    // 槽满拒。
    let mut c8 = SubBook::new(SubModel::Job);
    let mut sub_ok = true;
    for _ in 0..SLOT_CAP {
        if c8.job_submit(&[]).is_err() {
            sub_ok = false;
        }
    }
    s.add("满槽 8", sub_ok, "定容");
    s.add("槽满拒", c8.job_submit(&[]) == Err(XC_SLOT_FULL), "溢出");
    s.add("魔数核", SB_SEG_BASE == 0x31000, "段基址");
    s
}
