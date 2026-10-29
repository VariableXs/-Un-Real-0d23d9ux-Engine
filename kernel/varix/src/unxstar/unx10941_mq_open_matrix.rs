//! UNX-F10941 mq_open 错误矩阵（五错误面）（0x1d520 段）
//!
//! 纯功能模型：POSIX 消息队列名字表与 `mq_open` 裁决台。
//! 五错误面：① 无创建位查无名 → ENOENT；② 创建位+互斥位撞名
//! → EEXIST；③ 既有队列无对应读写权 → EACCES；④ 属性非法
//! （max_msg/max_count 为零）→ EINVAL；⑤ 单进程打开数达上限
//! → EMFILE。反面千轮样本只走合法打开，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d520;
/// 查无名
pub const MO_ENOENT: u32 = SEG_BASE + 0x01;
/// 撞名互斥
pub const MO_EEXIST: u32 = SEG_BASE + 0x02;
/// 无访问权
pub const MO_EACCES: u32 = SEG_BASE + 0x03;
/// 属性非法
pub const MO_EINVAL: u32 = SEG_BASE + 0x04;
/// 打开数达上限
pub const MO_EMFILE: u32 = SEG_BASE + 0x05;
/// 台内对账失败
pub const MO_LEDGER: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------- 常数
/// oflag：创建位
pub const OFLG_CREAT: u32 = 0x01;
/// oflag：互斥位
pub const OFLG_EXCL: u32 = 0x02;
/// oflag：只读
pub const OFLG_RDONLY: u32 = 0x04;
/// oflag：只写
pub const OFLG_WRONLY: u32 = 0x08;
/// 队列权限位（属主读/写）
pub const QMODE_R: u32 = 0x4;
pub const QMODE_W: u32 = 0x2;
/// 单进程打开上限
pub const MQ_OPEN_MAX: usize = 8;
/// 反面验收轮数
pub const MO_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 名字表中的一个消息队列。
#[derive(Clone, Debug)]
pub struct MqQueue {
    /// 队列名（/开头）
    pub name: String,
    /// 权限位
    pub mode: u32,
    /// 单条消息上限
    pub max_msg: usize,
    /// 消息条数上限
    pub max_count: usize,
    /// 已被 unlink（存量描述符仍可用）
    pub unlinked: bool,
}

impl MqQueue {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let bytes = self.name.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            h = (h ^ (bytes[i] as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.mode as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.max_msg as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.max_count as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.unlinked as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "MqQueue{{name={},mode={:#o},max_msg={},max_count={},unlinked={}}}",
            self.name, self.mode, self.max_msg, self.max_count, self.unlinked as u32
        )
    }
}

/// 单轮 mq_open 日志。
#[derive(Clone, Debug, PartialEq)]
pub struct OpenLog {
    /// 轮次
    pub round: usize,
    /// 目标名
    pub name: String,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_open 裁决台。
#[derive(Clone, Debug)]
pub struct MqDesk {
    /// 队列名表
    pub queues: Vec<MqQueue>,
    /// 每进程打开计数（按 pid 记）
    pub pids: Vec<(u32, usize)>,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// ENOENT 命中
    pub enoent_hits: usize,
    /// EEXIST 命中
    pub eexist_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EMFILE 命中
    pub emfile_hits: usize,
    /// 逐轮日志
    pub log: Vec<OpenLog>,
}

impl MqDesk {
    /// 建台。
    pub fn new() -> Self {
        MqDesk {
            queues: Vec::new(),
            pids: Vec::new(),
            rounds: 0,
            ok_hits: 0,
            enoent_hits: 0,
            eexist_hits: 0,
            eacces_hits: 0,
            einval_hits: 0,
            emfile_hits: 0,
            log: Vec::new(),
        }
    }

    /// 名字查找。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.queues.len() {
            if self.queues[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 进程打开计数。
    pub fn open_count(&self, pid: u32) -> usize {
        let mut i = 0;
        while i < self.pids.len() {
            if self.pids[i].0 == pid {
                return self.pids[i].1;
            }
            i += 1;
        }
        0
    }

    fn bump_open(&mut self, pid: u32) {
        let mut i = 0;
        while i < self.pids.len() {
            if self.pids[i].0 == pid {
                self.pids[i].1 = self.pids[i].1.wrapping_add(1);
                return;
            }
            i += 1;
        }
        self.pids.push((pid, 1));
    }

    /// 记账。
    fn book(&mut self, round: usize, name: &str, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == MO_ENOENT => self.enoent_hits = self.enoent_hits.wrapping_add(1),
            x if x == MO_EEXIST => self.eexist_hits = self.eexist_hits.wrapping_add(1),
            x if x == MO_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            x if x == MO_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == MO_EMFILE => self.emfile_hits = self.emfile_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(OpenLog { round, name: name.to_string(), verdict });
    }

    /// 裁决一次 mq_open。判定序：
    /// ① 属性非法（CREAT 且 max_msg/max_count 为零）→ EINVAL；
    /// ② 撞名互斥 → EEXIST；③ 查无名 → ENOENT；
    /// ④ 既有队列访问权 → EACCES；⑤ 打开数达上限 → EMFILE；⑥ 开通。
    pub fn mq_open(
        &mut self,
        pid: u32,
        name: &str,
        flags: u32,
        max_msg: usize,
        max_count: usize,
        round: usize,
    ) -> Result<u32, u32> {
        if flags & OFLG_CREAT != 0 && (max_msg == 0 || max_count == 0) {
            self.book(round, name, MO_EINVAL);
            return Err(MO_EINVAL);
        }
        match self.find(name) {
            Some(ix) => {
                if flags & OFLG_EXCL != 0 {
                    self.book(round, name, MO_EEXIST);
                    return Err(MO_EEXIST);
                }
                let need_r = flags & OFLG_RDONLY != 0;
                let need_w = flags & OFLG_WRONLY != 0;
                if (need_r && self.queues[ix].mode & QMODE_R == 0)
                    || (need_w && self.queues[ix].mode & QMODE_W == 0)
                {
                    self.book(round, name, MO_EACCES);
                    return Err(MO_EACCES);
                }
                if self.open_count(pid) >= MQ_OPEN_MAX {
                    self.book(round, name, MO_EMFILE);
                    return Err(MO_EMFILE);
                }
                self.bump_open(pid);
                self.book(round, name, 0);
                Ok(ix as u32)
            }
            None => {
                if flags & OFLG_CREAT == 0 {
                    self.book(round, name, MO_ENOENT);
                    return Err(MO_ENOENT);
                }
                if self.queues.len() >= 64 {
                    self.book(round, name, MO_LEDGER);
                    return Err(MO_LEDGER);
                }
                // 建队权限由 oflag 的读写位决定：只请求读则队列只读，
                // 后续写请求才会真裁 EACCES（与 J2 反面 EACCES 面对齐）。
                // 注意 oflag 位与 QMODE 位编码不同，必须逐位映射。
                let mut mode = 0u32;
                if flags & OFLG_RDONLY != 0 {
                    mode |= QMODE_R;
                }
                if flags & OFLG_WRONLY != 0 {
                    mode |= QMODE_W;
                }
                self.queues.push(MqQueue {
                    name: name.to_string(),
                    mode,
                    max_msg,
                    max_count,
                    unlinked: false,
                });
                self.bump_open(pid);
                self.book(round, name, 0);
                Ok((self.queues.len() - 1) as u32)
            }
        }
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.enoent_hits)
            .wrapping_add(self.eexist_hits)
            .wrapping_add(self.eacces_hits)
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.emfile_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.queues.len() {
            h = (h ^ self.queues[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "MqDesk{{queues={},rounds={},ok={},enoent={},eexist={},eacces={},einval={},emfile={}}}",
            self.queues.len(), self.rounds, self.ok_hits,
            self.enoent_hits, self.eexist_hits, self.eacces_hits, self.einval_hits, self.emfile_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = MqDesk::new();
    let _ = d.mq_open(1, "/q0", OFLG_CREAT | oflag_rdwr(), 64, 8, 9001);
    // 注入 1：撞名互斥 → EEXIST。
    caught[0] = matches!(
        d.mq_open(1, "/q0", OFLG_CREAT | OFLG_EXCL | OFLG_RDONLY, 64, 8, 9002),
        Err(x) if x == MO_EEXIST
    );
    // 注入 2：查无名 → ENOENT。
    caught[1] = matches!(d.mq_open(1, "/nope", OFLG_RDONLY, 0, 0, 9003), Err(x) if x == MO_ENOENT);
    // 注入 3：属性非法 → EINVAL。
    caught[2] = matches!(
        d.mq_open(1, "/q9", OFLG_CREAT | OFLG_RDONLY, 0, 8, 9004),
        Err(x) if x == MO_EINVAL
    );
    // 注入 4：打开数达上限 → EMFILE（先建名，再对同进程反复打开）。
    let _ = d.mq_open(7, "/f0", OFLG_CREAT | oflag_rdwr(), 64, 8, 9090);
    let mut i = 0;
    let mut hit = false;
    while i < MQ_OPEN_MAX + 4 {
        if matches!(
            d.mq_open(7, "/f0", OFLG_RDONLY, 0, 0, 9100 + i),
            Err(x) if x == MO_EMFILE
        ) {
            hit = true;
        }
        i += 1;
    }
    caught[3] = hit;
    caught
}

/// 读写双开位组合（供样本与注入复用）。
pub fn oflag_rdwr() -> u32 {
    OFLG_RDONLY | OFLG_WRONLY
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法打开占六成，五错误面轮转。返回符合预期轮数。
pub fn sample_thousand(d: &mut MqDesk) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < MO_ROUNDS {
        let low = r % 10;
        // 每轮换独立进程：单进程打开数上限只由本轮自己触碰，
        // 不污染后续合法面（否则第 9 次合法打开起全被 EMFILE 拦截）。
        let pid = 0x100u32.wrapping_add(r as u32);
        let res;
        let expect_ok;
        if low < 4 {
            // 合法面：对既有队列只读打开。
            res = d.mq_open(pid, "/work", OFLG_RDONLY, 0, 0, r);
            expect_ok = true;
        } else if low == 4 {
            // 合法面：新名创建。
            res = d.mq_open(pid, "/tmp", OFLG_CREAT | OFLG_WRONLY, 64, 8, r);
            expect_ok = true;
        } else if low == 5 {
            // ENOENT 面。
            res = d.mq_open(pid, "/ghost", OFLG_RDONLY, 0, 0, r);
            expect_ok = false;
        } else if low == 6 {
            // EEXIST 面。
            res = d.mq_open(pid, "/work", OFLG_CREAT | OFLG_EXCL | OFLG_RDONLY, 64, 8, r);
            expect_ok = false;
        } else if low == 7 {
            // EINVAL 面。
            res = d.mq_open(pid, "/work", OFLG_CREAT | OFLG_RDONLY, 0, 0, r);
            expect_ok = false;
        } else if low == 8 {
            // EMFILE 面：独立进程先建 /fill，再对同名反复打开灌满上限。
            // pid 基址须与合法面错开，否则两面的 pid 空间重叠会把
            // 已灌满的进程带进合法面，凭空裁出 EMFILE。
            let who = 0x1000u32.wrapping_add(r as u32);
            let _ = d.mq_open(who, "/fill", OFLG_CREAT | OFLG_RDONLY, 64, 8, r);
            let mut hit = false;
            let mut k = 0;
            while k < MQ_OPEN_MAX + 2 {
                if matches!(d.mq_open(who, "/fill", OFLG_RDONLY, 0, 0, r), Err(x) if x == MO_EMFILE) {
                    hit = true;
                }
                k += 1;
            }
            res = if hit { Err(MO_EMFILE) } else { Ok(0) };
            expect_ok = false;
        } else {
            // EACCES 面：/ro 建为只读队列，写请求必拒。
            res = d.mq_open(pid, "/ro", OFLG_WRONLY, 0, 0, r);
            expect_ok = false;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == MO_ENOENT
                        || *x == MO_EEXIST
                        || *x == MO_EACCES
                        || *x == MO_EINVAL
                        || *x == MO_EMFILE)
            }
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10941 判据组。
pub fn run_unx10941_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10941");
    let mk = || MqDesk::new();
    // J1 正面之一：ENOENT 十轮全中。
    let mut d1 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d1.mq_open(1, "/none", OFLG_RDONLY, 0, 0, k), Err(x) if x == MO_ENOENT) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面ENOENT", c == 10, "查无名十轮全部裁 ENOENT");
    // J1 正面之二：EEXIST 十轮全中。
    let mut d2 = mk();
    let _ = d2.mq_open(1, "/a", OFLG_CREAT | oflag_rdwr(), 64, 8, 1);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(
            d2.mq_open(1, "/a", OFLG_CREAT | OFLG_EXCL | OFLG_RDONLY, 64, 8, k),
            Err(x) if x == MO_EEXIST
        ) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EEXIST", c == 10, "撞名互斥十轮全部裁 EEXIST");
    // J1 正面之三：EACCES 十轮全中（把队列权限剥到只读再请求写）。
    let mut d3 = mk();
    let _ = d3.mq_open(1, "/b", OFLG_CREAT | oflag_rdwr(), 64, 8, 1);
    let ix3 = d3.find("/b").unwrap_or(0);
    d3.queues[ix3].mode = QMODE_R;
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d3.mq_open(1, "/b", OFLG_WRONLY, 0, 0, k), Err(x) if x == MO_EACCES) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EACCES", c == 10, "无写权队列写请求十轮全部裁 EACCES");
    // J1 正面之四：EINVAL 十轮全中。
    let mut d4 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            d4.mq_open(1, "/c", OFLG_CREAT | OFLG_RDONLY, 0, 8, k)
        } else {
            d4.mq_open(1, "/c", OFLG_CREAT | OFLG_RDONLY, 64, 0, k)
        };
        if matches!(r, Err(x) if x == MO_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "零属性建队十轮全部裁 EINVAL");
    // J1 正面之五：EMFILE 十轮全中。
    let mut d5 = mk();
    let _ = d5.mq_open(9, "/m", OFLG_CREAT | oflag_rdwr(), 64, 8, 890);
    let mut fill = 1;
    while fill < MQ_OPEN_MAX {
        let _ = d5.mq_open(9, "/m", OFLG_RDONLY, 0, 0, 891 + fill);
        fill += 1;
    }
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d5.mq_open(9, "/m", OFLG_RDONLY, 0, 0, 900 + k), Err(x) if x == MO_EMFILE) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EMFILE", c == 10, "打开数超限十轮全部裁 EMFILE");
    // J2 反面：千轮合法打开零误报。
    let mut d6 = mk();
    let _ = d6.mq_open(0x41, "/work", OFLG_CREAT | oflag_rdwr(), 64, 8, 0);
    let _ = d6.mq_open(0x41, "/ro", OFLG_CREAT | QMODE_R, 64, 8, 1);
    let _ = d6.mq_open(0x41, "/tmp", OFLG_CREAT | OFLG_WRONLY, 64, 8, 2);
    let good = sample_thousand(&mut d6);
    s.add_dyn("J2-反面零误报", good == MO_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EEXIST/ENOENT/EINVAL/EMFILE 四路坏样本全部拦截",
    );
    // J4 判定序：EINVAL 先于 EEXIST（撞名且属性非法裁 EINVAL）。
    let mut d7 = mk();
    let _ = d7.mq_open(1, "/x", OFLG_CREAT | oflag_rdwr(), 64, 8, 1);
    s.add(
        "J4-判定序",
        matches!(
            d7.mq_open(1, "/x", OFLG_CREAT | OFLG_EXCL | OFLG_RDONLY, 0, 8, 2),
            Err(x) if x == MO_EINVAL
        ),
        "属性非法先行裁定",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        MO_ENOENT > SEG_BASE && MO_LEDGER < SEG_BASE + 0x20,
        "六个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [MO_ENOENT, MO_EEXIST, MO_EACCES, MO_EINVAL, MO_EMFILE, MO_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J6-错误码互异", distinct, "六个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", d6.ledger_ok(), "千轮之后六类计数与轮次、日志逐项对平");
    // J8 打开计数与队列数一致：合法面不再膨胀。
    let oc = d6.open_count(0x41);
    s.add_dyn("J8-计数有界", oc <= MQ_OPEN_MAX, &format!("0x41 号进程打开数 {} 有界", oc));
    // J9 fold 稳定。
    let f = d6.fold();
    let clone = d6.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 渲染非空。
    let txt = d6.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("MqDesk"), "台级渲染含类型名与计数");
    // J11 unlink 名不可再开但存量保留（为 10958 铺垫）。
    let mut d8 = mk();
    let _ = d8.mq_open(1, "/y", OFLG_CREAT | oflag_rdwr(), 64, 8, 1);
    let ix8 = d8.find("/y").unwrap_or(0);
    d8.queues[ix8].unlinked = true;
    let _ = d8.queues.remove(ix8);
    s.add(
        "J11-名表一致",
        d8.find("/y").is_none() && d8.ledger_ok(),
        "删除名后查无、账不破",
    );
    // J12 动态：千轮明细。
    s.add_dyn(
        "J12-千轮明细",
        good == MO_ROUNDS,
        &format!("good={} desk={}", good, d6.render()),
    );
    // J13 动态：注入明细。
    s.add_dyn(
        "J13-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10941_mq_open_matrix_flow() {
        // 流程：建队 → 撞名 → 查无 → 属性非法 → 剥权 → EACCES → 超限。
        let mut d = MqDesk::new();
        assert!(d.mq_open(1, "/q", OFLG_CREAT | oflag_rdwr(), 64, 8, 1).is_ok());
        assert!(matches!(
            d.mq_open(1, "/q", OFLG_CREAT | OFLG_EXCL | OFLG_RDONLY, 64, 8, 2),
            Err(x) if x == MO_EEXIST
        ));
        assert!(matches!(d.mq_open(1, "/nope", OFLG_RDONLY, 0, 0, 3), Err(x) if x == MO_ENOENT));
        assert!(matches!(
            d.mq_open(1, "/new", OFLG_CREAT | OFLG_RDONLY, 0, 8, 4),
            Err(x) if x == MO_EINVAL
        ));
        let ix = d.find("/q").unwrap_or(0);
        d.queues[ix].mode = QMODE_R;
        assert!(matches!(d.mq_open(1, "/q", OFLG_WRONLY, 0, 0, 5), Err(x) if x == MO_EACCES));
        let mut i = 0;
        let mut hit = false;
        while i < MQ_OPEN_MAX + 2 {
            if matches!(d.mq_open(2, "/q", OFLG_RDONLY, 0, 0, 100 + i), Err(x) if x == MO_EMFILE) {
                hit = true;
            }
            i += 1;
        }
        assert!(hit);
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10941_checks_green() {
        let s = run_unx10941_checks();
        assert!(s.all_passed(), "UNX-F10941 判据未全绿");
    }
}
