//! UNX-F11295 诊断通道 fd 语义与重定向（0x1f6a0 段）
//!
//! 锚点：UNX-F11295-J1 fd2 默认通道、文件重定向、关闭降级三态落账，
//! 关闭后的诊断丢弃与装载器存活互不牵连。
//!
//! 纯功能模型：装载器的诊断输出默认写到 fd 2，可被重定向到文件或 fd 1，
//! 也可退回 fd 2。通道语义按三条硬约束建模：其一，每条诊断是整行原子写，
//! 不拆行、不并条；其二，目标 fd 关闭时写入静默丢弃（计入 dropped），
//! 装载器本身绝不因诊断失败而中止；其三，无论丢弃与否，attempts 恒等于
//! delivered 与 dropped 之和，账簿不因降级而失衡。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6a0;
/// 严格写入遇到已关闭目标
pub const DC_ECLOSED: u32 = SEG_BASE + 0x01;
/// 重定向路径为空或目标非法
pub const DC_ETARGET: u32 = SEG_BASE + 0x02;
/// 空诊断行
pub const DC_ELINE: u32 = SEG_BASE + 0x03;
/// 冻结后变更或写入
pub const DC_EFROZEN: u32 = SEG_BASE + 0x04;
/// fd 编号非法（文件通道要求 >=3）
pub const DC_EBADFD: u32 = SEG_BASE + 0x05;
/// 通道模式不允许该操作
pub const DC_EMODE: u32 = SEG_BASE + 0x06;
/// 单行超过 200 字节上限
pub const DC_EOVER: u32 = SEG_BASE + 0x07;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
/// 单行上限：诊断行不许无界增长。
pub const LINE_MAX: usize = 200;

fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            x = fold_byte(x, *b);
        }
        k = k.wrapping_add(1);
    }
    x
}

// ---------------------------------------------------------------- 通道目标
/// 诊断落点：fd2 / fd1 / 文件 / 空设备。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sink {
    Fd2,
    Fd1,
    File,
    Null,
}

impl Sink {
    pub fn name(&self) -> &'static str {
        match self {
            Sink::Fd2 => "fd2",
            Sink::Fd1 => "fd1",
            Sink::File => "file",
            Sink::Null => "null",
        }
    }
}

/// 诊断通道：目标 + 三个落点日志 + 全量计数。
#[derive(Clone, Debug)]
pub struct DiagChannel {
    sink: Sink,
    stderr_open: bool,
    file_fd: u8,
    file_path: String,
    file_open: bool,
    fd2_log: Vec<String>,
    fd1_log: Vec<String>,
    file_log: Vec<String>,
    pub attempts: u32,
    pub delivered: u32,
    pub dropped: u32,
    pub bytes: u32,
    pub redirects: u32,
    pub rejects: u32,
    seal: Option<u64>,
}

impl DiagChannel {
    /// 默认通道：fd2 打开，空账。
    pub fn new() -> Self {
        DiagChannel {
            sink: Sink::Fd2,
            stderr_open: true,
            file_fd: 0,
            file_path: String::new(),
            file_open: false,
            fd2_log: Vec::new(),
            fd1_log: Vec::new(),
            file_log: Vec::new(),
            attempts: 0,
            delivered: 0,
            dropped: 0,
            bytes: 0,
            redirects: 0,
            rejects: 0,
            seal: None,
        }
    }

    pub fn sink(&self) -> Sink {
        self.sink
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    /// 诊断失败从不拖垮装载器：此谓词恒真。
    pub fn operational(&self) -> bool {
        true
    }

    pub fn stderr_open(&self) -> bool {
        self.stderr_open
    }

    pub fn file_fd(&self) -> u8 {
        self.file_fd
    }

    pub fn file_path(&self) -> &str {
        self.file_path.as_str()
    }

    pub fn fd2_log(&self) -> &[String] {
        self.fd2_log.as_slice()
    }

    pub fn fd1_log(&self) -> &[String] {
        self.fd1_log.as_slice()
    }

    pub fn file_log(&self) -> &[String] {
        self.file_log.as_slice()
    }

    /// 当前目标是否可写（Null 视为主动丢弃）。
    fn writable(&self) -> bool {
        match self.sink {
            Sink::Fd2 => self.stderr_open,
            Sink::Fd1 => true,
            Sink::File => self.file_open,
            Sink::Null => false,
        }
    }

    /// 投递一条已通过校验的整行，返回含换行的字节数。
    fn deliver(&mut self, msg: &str) -> u32 {
        let n = (msg.len() as u32).wrapping_add(1);
        match self.sink {
            Sink::Fd2 => self.fd2_log.push(msg.to_string()),
            Sink::Fd1 => self.fd1_log.push(msg.to_string()),
            Sink::File => self.file_log.push(msg.to_string()),
            Sink::Null => {}
        }
        self.delivered = self.delivered.wrapping_add(1);
        self.bytes = self.bytes.wrapping_add(n);
        n
    }

    /// 公共校验：冻结、空行、超长；违例计一次拒绝。
    fn vet(&mut self, msg: &str) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EFROZEN);
        }
        if msg.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_ELINE);
        }
        if msg.len() > LINE_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EOVER);
        }
        Ok(())
    }

    /// 宽容写入：目标关闭时静默丢弃并返回 0，装载器继续运行。
    pub fn emit(&mut self, msg: &str) -> Result<u32, u32> {
        self.vet(msg)?;
        self.attempts = self.attempts.wrapping_add(1);
        if self.writable() {
            Ok(self.deliver(msg))
        } else {
            self.dropped = self.dropped.wrapping_add(1);
            Ok(0)
        }
    }

    /// 严格写入：目标关闭时报 ECLOSED，但同样记入 attempts/dropped。
    pub fn emit_strict(&mut self, msg: &str) -> Result<u32, u32> {
        self.vet(msg)?;
        self.attempts = self.attempts.wrapping_add(1);
        if self.writable() {
            Ok(self.deliver(msg))
        } else {
            self.dropped = self.dropped.wrapping_add(1);
            self.rejects = self.rejects.wrapping_add(1);
            Err(DC_ECLOSED)
        }
    }

    /// 重定向到文件：fd 必须 >=3，路径非空；open 表示目标是否可写。
    pub fn redirect_file(&mut self, fd: u8, open: bool, path: &str) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EFROZEN);
        }
        if fd < 3 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EBADFD);
        }
        if path.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_ETARGET);
        }
        self.sink = Sink::File;
        self.file_fd = fd;
        self.file_path = path.to_string();
        self.file_open = open;
        self.redirects = self.redirects.wrapping_add(1);
        Ok(())
    }

    /// 重定向到 fd1（仅用于对照通道）。
    pub fn redirect_stdout(&mut self) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EFROZEN);
        }
        self.sink = Sink::Fd1;
        self.redirects = self.redirects.wrapping_add(1);
        Ok(())
    }

    /// 退回默认 fd2 通道。
    pub fn restore(&mut self) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EFROZEN);
        }
        self.sink = Sink::Fd2;
        self.redirects = self.redirects.wrapping_add(1);
        Ok(())
    }

    /// 只允许在文件通道上调整可写位。
    pub fn set_file_open(&mut self, open: bool) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EFROZEN);
        }
        if self.sink != Sink::File {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DC_EMODE);
        }
        self.file_open = open;
        Ok(())
    }

    pub fn close_stderr(&mut self) {
        self.stderr_open = false;
    }

    pub fn open_stderr(&mut self) {
        self.stderr_open = true;
    }

    /// 记账守恒：attempts 恒等于 delivered + dropped。
    pub fn accounted(&self) -> u32 {
        self.delivered.wrapping_add(self.dropped)
    }

    pub fn balanced(&self) -> bool {
        self.attempts == self.accounted()
    }

    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.sink.name());
        h = fold_byte(h, self.stderr_open as u8);
        h = fold_byte(h, self.file_fd);
        h = fold_byte(h, self.file_open as u8);
        h = fold_str(h, &self.file_path);
        h = fold_str(
            h,
            &format!(
                "{}:{}:{}:{}:{}:{}",
                self.attempts, self.delivered, self.dropped, self.bytes, self.redirects, self.rejects
            ),
        );
        h = fold_log(h, 2, &self.fd2_log);
        h = fold_log(h, 1, &self.fd1_log);
        fold_log(h, 3, &self.file_log)
    }

    /// 幂等冻结：配置与日志一并定格。
    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

fn fold_log(h: u64, tag: u8, log: &[String]) -> u64 {
    let mut x = fold_byte(h, tag);
    let mut k = 0usize;
    while k < log.len() {
        if let Some(line) = log.get(k) {
            x = fold_str(x, line);
            x = fold_byte(x, b'\n');
        }
        k = k.wrapping_add(1);
    }
    x
}

// ---------------------------------------------------------------- 基准案例
/// 基准通道：七条诊断，跨 fd2 / 文件 / 关闭 / 重开四态。
pub struct ChannelCase {
    pub ch: DiagChannel,
    pub seal: Option<u64>,
}

pub fn build_case() -> ChannelCase {
    let mut ch = DiagChannel::new();
    let _ = ch.emit("probe: stage=1");
    let _ = ch.emit("probe: stage=2");
    let _ = ch.emit("probe: stage=3");
    let _ = ch.redirect_file(4, true, "/tmp/probe.1234");
    let _ = ch.emit("probe: stage=4");
    let _ = ch.restore();
    let _ = ch.emit("probe: stage=5");
    ch.close_stderr();
    let _ = ch.emit("probe: stage=6");
    ch.open_stderr();
    let _ = ch.emit("probe: stage=7");
    let seal = ch.freeze();
    ChannelCase { ch, seal }
}

/// 生成指定长度的诊断行（不依赖 repeat 扩展）。
pub fn long_line(n: usize) -> String {
    let mut s = String::new();
    let mut k = 0usize;
    while k < n {
        s.push('x');
        k = k.wrapping_add(1);
    }
    s
}

/// 四路对抗：空行、超长、非法 fd、冻结写，全部必须被拒。
pub fn inject_four() -> [bool; 4] {
    let mut hits = [false; 4];

    let mut a = DiagChannel::new();
    hits[0] = a.emit("") == Err(DC_ELINE);

    let long = long_line(LINE_MAX.wrapping_add(1));
    hits[1] = a.emit(long.as_str()) == Err(DC_EOVER);

    let mut b = DiagChannel::new();
    hits[2] = b.redirect_file(2, true, "/tmp/probe.1234") == Err(DC_EBADFD);

    let mut c = build_case().ch;
    hits[3] = c.emit("late") == Err(DC_EFROZEN);

    hits
}

// ---------------------------------------------------------------- 判据
pub fn run_unx11295_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11295");
    let mut case = build_case();

    // J1 默认 fd2 通道
    let mut fresh = DiagChannel::new();
    let wrote = fresh.emit("boot: ok");
    s.add(
        "J1-默认fd2通道",
        fresh.sink() == Sink::Fd2
            && wrote == Ok(9)
            && fresh.fd2_log().len() == 1
            && fresh.fd1_log().is_empty()
            && fresh.file_log().is_empty(),
        "新通道默认写 fd2，单条整行",
    );

    // J2 七条总账
    let ch = &case.ch;
    s.add(
        "J2-七条总账",
        ch.attempts == 7
            && ch.delivered == 6
            && ch.dropped == 1
            && ch.bytes == 90
            && ch.redirects == 2
            && ch.balanced(),
        "7 次尝试 / 6 次投递 / 1 次丢弃 / 90 字节",
    );

    // J3 行序与分落点
    let fd2: Vec<&str> = ch.fd2_log().iter().map(|x| x.as_str()).collect();
    let file: Vec<&str> = ch.file_log().iter().map(|x| x.as_str()).collect();
    s.add(
        "J3-行序与落点",
        fd2 == ["probe: stage=1", "probe: stage=2", "probe: stage=3", "probe: stage=5", "probe: stage=7"]
            && file == ["probe: stage=4"],
        "fd2 五条按序，文件通道仅 stage=4",
    );

    // J4 空行拒
    let mut a = DiagChannel::new();
    let before = a.rejects;
    s.add(
        "J4-空行拒",
        a.emit("") == Err(DC_ELINE)
            && a.rejects == before.wrapping_add(1)
            && a.attempts == 0
            && a.fd2_log().is_empty(),
        "空行不进账且拒绝计数加一",
    );

    // J5 超长拒与边界
    let mut b = DiagChannel::new();
    let over = b.emit(long_line(LINE_MAX.wrapping_add(1)).as_str());
    let edge = b.emit(long_line(LINE_MAX).as_str());
    s.add(
        "J5-超长拒",
        over == Err(DC_EOVER) && edge == Ok(LINE_MAX as u32 + 1) && b.balanced(),
        "201 字节拒，200 字节收",
    );

    // J6 文件重定向
    let mut c = DiagChannel::new();
    let red = c.redirect_file(4, true, "/var/log/ld.so.log");
    let sent = c.emit("probe: redirected");
    s.add(
        "J6-文件重定向",
        red.is_ok()
            && c.sink() == Sink::File
            && c.file_fd() == 4
            && c.file_path() == "/var/log/ld.so.log"
            && sent == Ok(18)
            && c.file_log().len() == 1
            && c.fd2_log().is_empty(),
        "fd4 文件通道接管后续诊断",
    );

    // J7 重定向前后落点隔离
    let mut d = DiagChannel::new();
    let _ = d.emit("before: 1");
    let _ = d.emit("before: 2");
    let _ = d.redirect_file(5, true, "/tmp/ld.777");
    let _ = d.emit("after: 1");
    s.add(
        "J7-落点隔离",
        d.fd2_log().len() == 2 && d.file_log().len() == 1 && d.delivered == 3,
        "重定向前两条进 fd2，之后一条进文件",
    );

    // J8 退回 fd2
    let mut e = DiagChannel::new();
    let _ = e.redirect_file(6, true, "/tmp/ld.888");
    let _ = e.emit("in file");
    let back = e.restore();
    let _ = e.emit("back home");
    s.add(
        "J8-退回fd2",
        back.is_ok()
            && e.sink() == Sink::Fd2
            && e.file_log().len() == 1
            && e.fd2_log().len() == 1
            && e.redirects == 2,
        "restore 后新诊断回到 fd2",
    );

    // J9 关闭降级
    let mut f = DiagChannel::new();
    let _ = f.emit("first");
    f.close_stderr();
    let quiet = f.emit("lost");
    let still = f.emit("lost too");
    s.add(
        "J9-关闭降级",
        quiet == Ok(0)
            && still == Ok(0)
            && f.dropped == 2
            && f.fd2_log().len() == 1
            && f.operational()
            && f.balanced(),
        "fd2 关闭后静默丢弃，装载器仍存活",
    );

    // J10 重开续写
    let mut g = DiagChannel::new();
    g.close_stderr();
    let _ = g.emit("gone");
    g.open_stderr();
    let resumed = g.emit("resumed");
    s.add(
        "J10-重开续写",
        resumed == Ok(8)
            && g.dropped == 1
            && g.delivered == 1
            && g.fd2_log().len() == 1
            && g.fd2_log().first().map(|x| x.as_str()) == Some("resumed"),
        "重开 fd2 后诊断继续，丢弃不回填",
    );

    // J11 降级一致性
    let mut open_ch = DiagChannel::new();
    let mut shut = DiagChannel::new();
    shut.close_stderr();
    let lines = ["probe: a", "probe: b", "probe: c"];
    let mut k = 0usize;
    while k < lines.len() {
        if let Some(m) = lines.get(k) {
            let _ = open_ch.emit(m);
            let _ = shut.emit(m);
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "J11-降级一致性",
        open_ch.attempts == shut.attempts
            && open_ch.attempts == 3
            && open_ch.accounted() == shut.accounted()
            && open_ch.bytes == 27
            && shut.bytes == 0
            && shut.fd2_log().is_empty(),
        "同输入下开/闭通道记账守恒一致",
    );

    // J12 非法目标拒
    let mut h = DiagChannel::new();
    let bad_fd = h.redirect_file(2, true, "/tmp/ld.9");
    let bad_path = h.redirect_file(3, true, "");
    s.add(
        "J12-非法目标拒",
        bad_fd == Err(DC_EBADFD) && bad_path == Err(DC_ETARGET) && h.sink() == Sink::Fd2,
        "fd<3 与空路径均拒且落点不变",
    );

    // J13 文件未开降级
    let mut i = DiagChannel::new();
    let mode = i.set_file_open(true);
    let _ = i.redirect_file(4, false, "/tmp/ld.closed");
    let lost = i.emit("no file");
    let reopened = i.set_file_open(true);
    let got = i.emit("yes file");
    s.add(
        "J13-文件未开降级",
        mode == Err(DC_EMODE)
            && lost == Ok(0)
            && reopened.is_ok()
            && got == Ok(9)
            && i.dropped == 1
            && i.file_log().len() == 1,
        "未开文件静默丢弃，开启后恢复投递",
    );

    // J14 严格模式报错
    let mut j = DiagChannel::new();
    j.close_stderr();
    let strict = j.emit_strict("must fail");
    s.add(
        "J14-严格模式报错",
        strict == Err(DC_ECLOSED)
            && j.attempts == 1
            && j.dropped == 1
            && j.rejects == 1
            && j.balanced(),
        "严格写入报 ECLOSED，账簿仍守恒",
    );

    // J15 冻结拒写入与重定向
    let before = case.ch.rejects;
    let late = case.ch.emit("late");
    let red = case.ch.redirect_file(7, true, "/tmp/ld.late");
    s.add(
        "J15-冻结拒变更",
        late == Err(DC_EFROZEN)
            && red == Err(DC_EFROZEN)
            && case.ch.rejects == before.wrapping_add(2)
            && case.ch.attempts == 7,
        "冻结后写入与重定向均拒，尝试数不变",
    );

    // J16 冻结幂等
    let again = case.ch.freeze();
    s.add(
        "J16-冻结幂等",
        again.is_some() && again == case.seal && case.ch.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J17 注入四拒
    let hits = inject_four();
    s.add(
        "J17-注入四拒",
        hits.iter().all(|h| *h),
        "空行/超长/非法fd/冻结写四路全拒",
    );

    // J18 通道明细
    s.add_dyn(
        "J18-通道明细",
        case.ch.balanced() && case.ch.fd2_log().len() == 5,
        &format!(
            "sink={} att={} del={} drop={} bytes={} redir={} fd2={} file={}",
            case.ch.sink().name(),
            case.ch.attempts,
            case.ch.delivered,
            case.ch.dropped,
            case.ch.bytes,
            case.ch.redirects,
            case.ch.fd2_log().len(),
            case.ch.file_log().len()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11295_channel_flow() {
        let mut case = build_case();
        assert_eq!(case.ch.attempts, 7);
        assert_eq!(case.ch.delivered, 6);
        assert_eq!(case.ch.dropped, 1);
        assert_eq!(case.ch.bytes, 90);
        assert_eq!(case.ch.fd2_log().len(), 5);
        assert_eq!(case.ch.file_log().len(), 1);
        assert_eq!(case.ch.file_fd(), 4);
        assert!(case.ch.balanced());
        assert!(case.ch.freeze() == case.seal);
        assert_eq!(case.ch.emit("late").err(), Some(DC_EFROZEN));

        let mut ch = DiagChannel::new();
        assert_eq!(ch.emit("0123456789"), Ok(11));
        assert_eq!(ch.emit(""), Err(DC_ELINE));
        assert_eq!(ch.redirect_file(2, true, "/tmp/x"), Err(DC_EBADFD));
        assert!(ch.redirect_file(3, true, "/tmp/x").is_ok());
        assert!(ch.set_file_open(false).is_ok());
        assert_eq!(ch.emit("gone"), Ok(0));
        assert_eq!(ch.set_file_open(true), Ok(()));
        assert_eq!(ch.emit("back"), Ok(5));

        let mut shut = DiagChannel::new();
        shut.close_stderr();
        assert_eq!(shut.emit_strict("x"), Err(DC_ECLOSED));
        assert!(shut.balanced());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11295_checks_green() {
        let s = run_unx11295_checks();
        assert!(s.all_passed(), "UNX-F11295 判据未全绿");
    }
}
