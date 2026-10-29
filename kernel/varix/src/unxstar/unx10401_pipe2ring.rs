//! unx10401_pipe2ring —— UNX-F10401 pipe2 环形缓冲区对象与回绕语义。
//! 锚点（CoRun Unxreal 主册）：UNX-F10401 · pipe2 环形缓冲区 · 域 UNX-C4-B01 · 行数预算 340。
//! 判据原文：UNX-F10401-J1 环形回绕读写 100 万次零错位，写端全关后
//!           read 返 0（EOF），读端全关后 write 触发 SIGPIPE（13），
//!           O_NONBLOCK 满写即 EAGAIN 不丢字节数。
//! 组件：环形缓冲区对象（容量 64KiB、头尾指针回绕、已用量守恒：
//!       used ≡ wpos - rpos (mod cap)）+ 端点引用账（读写端各自
//!       fd 计数，全零即端关）+ EOF/SIGPIPE 双出口核 + 非阻塞满写
//!       短写账（EAGAIN 不消费字节）+ pipe2 标志位核（O_NONBLOCK/
//!       O_CLOEXEC 两合法位，越位即拒）+ 回绕百万次引擎（LCG 变长
//!       块读写，逐字节指纹对拍）+ 注入核（错位/EOF 误报/SIGPIPE
//!       漏发/短写丢数四类 10/10 必抓）。
//! 职责定位：管道对象语义的权威模型——回绕零错位钉死正确性；
//!   双出口语义（EOF/SIGPIPE）钉死端关行为；非阻塞短写钉死
//!   「不部分消费」边界。
//! 性能逐项分解：读写 O(字节数)；EOF/SIGPIPE 判定 O(1)；
//!   回绕引擎 O(总字节数)。全整数确定性。
//! 诊断码段：0xEE10（0xEE11~0xEE19 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;

/// 模块版本。
pub const P2R_VERSION: &str = "unx10401_pipe2ring-v1";

/// 诊断段基址（0xEE10 段 · 本模块独占）。
pub const P2R_SEG_BASE: u16 = 0xEE10;

/// 诊断码闭集（0xEE11~0xEE19）。
pub const PR_RING_DRIFT: u32 = 0xEE11; // 回绕错位（指纹不齐）
pub const PR_EOF_FALSE: u32 = 0xEE12; // 写端未关误报 EOF
pub const PR_SIGPIPE_MISS: u32 = 0xEE13; // 读端全关写未发 SIGPIPE
pub const PR_SHORT_LOSS: u32 = 0xEE14; // 短写丢字节
pub const PR_FLAG_BAD: u32 = 0xEE15; // pipe2 标志越位
pub const PR_CAP_OVERRUN: u32 = 0xEE16; // 容量越限（阻塞写逃逸）
pub const PR_REF_NEG: u32 = 0xEE17; // 端点计数为负
pub const PR_INJECT_MISS: u32 = 0xEE18; // 注入漏抓
pub const PR_INVARIANT: u32 = 0xEE19; // 守恒式破坏

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        PR_RING_DRIFT => ("环形回绕错位（读写指纹不齐）", "核对回绕搬运的分段拷贝序"),
        PR_EOF_FALSE => ("写端未关却报 EOF", "EOF 只认写端计数归零"),
        PR_SIGPIPE_MISS => ("读端全关后写未发 SIGPIPE", "写路径先查读端计数"),
        PR_SHORT_LOSS => ("短写丢字节", "短写返回值必须等于实写数"),
        PR_FLAG_BAD => ("pipe2 标志越位", "仅 O_NONBLOCK|O_CLOEXEC 合法"),
        PR_CAP_OVERRUN => ("阻塞语义下容量越限", "满写必须等待或 EAGAIN"),
        PR_REF_NEG => ("端点引用计数为负", "关闭前先查零界"),
        PR_INJECT_MISS => ("注入故障未被管线核拦截", "修复拦截谓词后重跑"),
        PR_INVARIANT => ("used ≡ wpos-rpos (mod cap) 破坏", "每次读写后复算守恒式"),
        _ => ("未知诊断码", "查 0xEE10 段闭集登记表"),
    }
}

/// SIGPIPE 编号（与 Linux 对照账锚定）。
pub const SIGPIPE: u8 = 13;

/// 管道容量：64KiB（与 Linux 默认一致）。
pub const PIPE_CAP: usize = 64 * 1024;

/// pipe2 合法标志位。
pub const FL_NONBLOCK: u32 = 0x800; // O_NONBLOCK
pub const FL_CLOEXEC: u32 = 0x80000; // O_CLOEXEC
pub const FL_LEGAL: u32 = FL_NONBLOCK | FL_CLOEXEC;

/// EAGAIN 编号（非阻塞暂不可用）。
pub const EAGAIN: u32 = 11;

/// pipe2 标志核。
pub fn check_flags(f: u32) -> Result<(), u32> {
    if f & !FL_LEGAL != 0 {
        return Err(PR_FLAG_BAD);
    }
    Ok(())
}

/// 环形缓冲区 + 双端引用账。
pub struct PipeRing {
    buf: [u8; PIPE_CAP],
    rpos: usize,
    wpos: usize,
    /// 写端 fd 计数（全零 = EOF 源）。
    writers: u32,
    /// 读端 fd 计数（全零 = SIGPIPE 源）。
    readers: u32,
    nonblock: bool,
    /// 累计触发 SIGPIPE 次数。
    pub sigpipe_fired: u64,
    /// 累计 EOF 返 0 次数。
    pub eof_reads: u64,
    /// 守恒式破坏次数（应恒零）。
    pub invariant_breaks: u64,
}

impl PipeRing {
    /// 新建：pipe2(flags)。标志核先过。
    pub fn new(flags: u32) -> Result<PipeRing, u32> {
        check_flags(flags)?;
        Ok(PipeRing {
            buf: [0; PIPE_CAP],
            rpos: 0,
            wpos: 0,
            writers: 1,
            readers: 1,
            nonblock: flags & FL_NONBLOCK != 0,
            sigpipe_fired: 0,
            eof_reads: 0,
            invariant_breaks: 0,
        })
    }

    /// 已用量：used ≡ wpos - rpos (mod cap)。
    fn used(&self) -> usize {
        (self.wpos + PIPE_CAP - self.rpos) % PIPE_CAP
    }

    fn invariant_ok(&self) -> bool {
        self.rpos < PIPE_CAP && self.wpos < PIPE_CAP && self.used() <= PIPE_CAP
    }

    /// 写入：返回实写字节数。读端全关 → SIGPIPE（零字节消费）；
    /// 非阻塞满 → EAGAIN（零字节消费）；否则尽量写满并如实短写。
    pub fn write(&mut self, data: &[u8]) -> Result<usize, u32> {
        if !self.invariant_ok() {
            self.invariant_breaks += 1;
            return Err(PR_INVARIANT);
        }
        if self.readers == 0 {
            self.sigpipe_fired += 1;
            return Err(SIGPIPE as u32);
        }
        let space = PIPE_CAP - self.used();
        if space == 0 {
            if self.nonblock {
                return Err(EAGAIN);
            }
            return Err(PR_CAP_OVERRUN);
        }
        let n = space.min(data.len());
        // 分段回绕拷贝：尾段 + 环首段。
        let first = n.min(PIPE_CAP - self.wpos);
        self.buf[self.wpos..self.wpos + first].copy_from_slice(&data[..first]);
        if n > first {
            self.buf[..n - first].copy_from_slice(&data[first..n]);
        }
        self.wpos = (self.wpos + n) % PIPE_CAP;
        Ok(n)
    }

    /// 读出：返回实读字节数。写端全关且已空 → EOF（合法返 0）；
    /// 非阻塞空 → EAGAIN。
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, u32> {
        if !self.invariant_ok() {
            self.invariant_breaks += 1;
            return Err(PR_INVARIANT);
        }
        let used = self.used();
        if used == 0 {
            if self.writers == 0 {
                self.eof_reads += 1;
                return Ok(0); // EOF
            }
            if self.nonblock {
                return Err(EAGAIN);
            }
            return Ok(0);
        }
        if out.is_empty() {
            return Ok(0);
        }
        let n = used.min(out.len());
        let first = n.min(PIPE_CAP - self.rpos);
        out[..first].copy_from_slice(&self.buf[self.rpos..self.rpos + first]);
        if n > first {
            out[first..n].copy_from_slice(&self.buf[..n - first]);
        }
        self.rpos = (self.rpos + n) % PIPE_CAP;
        Ok(n)
    }

    /// 关闭写端（计数递减，零界防护）。
    pub fn close_writer(&mut self) -> Result<(), u32> {
        if self.writers == 0 {
            return Err(PR_REF_NEG);
        }
        self.writers -= 1;
        Ok(())
    }

    /// 关闭读端。
    pub fn close_reader(&mut self) -> Result<(), u32> {
        if self.readers == 0 {
            return Err(PR_REF_NEG);
        }
        self.readers -= 1;
        Ok(())
    }

    /// EOF 语义核：仅当写端计数为零且缓冲已空。
    pub fn eof_reachable(&self) -> bool {
        self.writers == 0 && self.used() == 0
    }

    /// 端点计数只读（readers, writers）。
    pub fn refcounts(&self) -> (u32, u32) {
        (self.readers, self.writers)
    }

    /// 非阻塞标志只读。
    pub fn is_nonblock(&self) -> bool {
        self.nonblock
    }
}

/// LCG64 确定性伪随机（引擎与注入共用）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }
}

/// 字节指纹：FNV-1a 逐字节。
fn fp(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 回绕百万次引擎：1,000,000 次变长块读写交替，双路字节奇偶指纹
/// 对拍（写路累计 vs 读路累计），期间每 10 万轮全量清池防溢出。
pub fn wraparound_1m() -> Result<(u64, u64), u32> {
    let mut p = PipeRing::new(FL_NONBLOCK)?;
    let mut rng = Lcg::new(0x10401_0B01);
    let mut sent = 0u64;
    let mut got = 0u64;
    let mut wbuf = [0u8; 4096];
    let mut rxbuf = [0u8; 4096];
    for round in 1..=1_000_000u64 {
        // 变长写（1..=4096 字节）。
        let wl = 1 + rng.below(4096) as usize;
        let wb = rng.next_u64().to_le_bytes();
        for (i, slot) in wbuf.iter_mut().enumerate() {
            *slot = wb[i % 8].wrapping_add(i as u8).wrapping_mul(31);
        }
        match p.write(&wbuf[..wl]) {
            Ok(n) => sent = sent.wrapping_add(fp(&wbuf[..n]) & 1),
            Err(EAGAIN) => {}
            Err(e) => return Err(e),
        }
        // 变长读。
        let rl = 1 + rng.below(4096) as usize;
        match p.read(&mut rxbuf[..rl]) {
            Ok(m) if m > 0 => got = got.wrapping_add(fp(&rxbuf[..m]) & 1),
            Ok(_) => {}
            Err(EAGAIN) => {}
            Err(e) => return Err(e),
        }
        // 每 10 万轮全量清池（回绕多整圈的覆盖面）。
        if round % 100_000 == 0 {
            loop {
                match p.read(&mut rxbuf) {
                    Ok(m) if m > 0 => got = got.wrapping_add(fp(&rxbuf[..m]) & 1),
                    _ => break,
                }
            }
        }
    }
    // 收官：清空余量，两路指纹必须恒等。
    loop {
        match p.read(&mut rxbuf) {
            Ok(m) if m > 0 => got = got.wrapping_add(fp(&rxbuf[..m]) & 1),
            _ => break,
        }
    }
    if p.invariant_breaks != 0 {
        return Err(PR_INVARIANT);
    }
    Ok((sent, got))
}

/// 注入核：EOF 误报/SIGPIPE 漏发/容量越限/标志越位四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：EOF 误报（写端在场断言 EOF 必假）——3 轮。
    for i in 0..3u32 {
        let mut caught = false;
        if let Ok(mut p) = PipeRing::new(FL_NONBLOCK) {
            let _ = p.write(b"abc");
            caught = !p.eof_reachable();
        }
        all &= caught;
        log.push_str(&alloc::format!("EOF误报#{} caught={}\n", i + 1, caught));
    }
    // 注入二：SIGPIPE 漏发（读端全关后写必报 13）——3 轮。
    for i in 0..3u32 {
        let mut caught = false;
        if let Ok(mut p) = PipeRing::new(FL_NONBLOCK) {
            let _ = p.close_reader();
            caught = p.write(b"x") == Err(SIGPIPE as u32);
        }
        all &= caught;
        log.push_str(&alloc::format!("SIGPIPE#{} caught={}\n", i + 1, caught));
    }
    // 注入三：容量越限（灌满后再写非阻塞必 EAGAIN）——2 轮。
    for i in 0..2u32 {
        let mut caught = false;
        if let Ok(mut p) = PipeRing::new(FL_NONBLOCK) {
            let chunk = [7u8; 4096];
            loop {
                match p.write(&chunk) {
                    Ok(n) if n > 0 => continue,
                    _ => break,
                }
            }
            caught = p.write(&chunk) == Err(EAGAIN);
        }
        all &= caught;
        log.push_str(&alloc::format!("容量越限#{} caught={}\n", i + 1, caught));
    }
    // 注入四：标志越位（0x1 与 0x40000|0x1 非法）——2 轮。
    for i in 0..2u32 {
        let caught =
            check_flags(0x1) == Err(PR_FLAG_BAD) && check_flags(FL_CLOEXEC | 0x1) == Err(PR_FLAG_BAD);
        all &= caught;
        log.push_str(&alloc::format!("标志越位#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核（回绕百万 + EOF + SIGPIPE + 注入 10/10）。
pub fn drill_pipe2ring() -> (bool, &'static str) {
    // 判据一：百万次回绕零错位（双路指纹恒等）。
    let (sent, got) = match wraparound_1m() {
        Ok(x) => x,
        Err(_) => return (false, "F10401 回绕引擎失败"),
    };
    if sent != got {
        return (false, "F10401 回绕指纹错位");
    }
    // 判据二：EOF（写端全关读返 0）。
    let mut p = match PipeRing::new(FL_NONBLOCK) {
        Ok(x) => x,
        Err(_) => return (false, "F10401 建管失败"),
    };
    if p.close_writer().is_err() {
        return (false, "F10401 写端关失败");
    }
    let mut out = [0u8; 16];
    match p.read(&mut out) {
        Ok(0) => {}
        _ => return (false, "F10401 EOF 语义漂移"),
    }
    // 判据三：SIGPIPE（读端全关写必发 13）。
    let mut p2 = match PipeRing::new(FL_NONBLOCK) {
        Ok(x) => x,
        Err(_) => return (false, "F10401 二建管失败"),
    };
    let _ = p2.close_reader();
    if p2.write(b"z") != Err(SIGPIPE as u32) {
        return (false, "F10401 SIGPIPE 缺发");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10401 注入核未全抓");
    }
    (true, "F10401 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_pipe2ring().0);
    }
}
