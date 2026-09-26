
// ---------------------------------------------------------------------------
// F011 · 深化批次三：注入期一次展开纪律（百分号展开仅注入时执行一次）+
// COMPUTERNAME 取设备名（F123「关于本机」同源）
//
// 主册依据（G-A-11【设计细节】）：「变量值内百分号展开仅注入时执行一次」；
// 「COMPUTERNAME 取设备名（F123『关于本机』同源）」。expand/EnvTable 既有面
// （一处一事实），本段补纪律记账与同源取值。
// ---------------------------------------------------------------------------

/// 展开纪律台账：注入期展开计数 vs 运行期再展开违例计数。
#[derive(Clone, Copy, Debug)]
pub struct ExpandOnceLedger {
    /// 注入期展开次数（合法——每次进程创建/会话注入各一次）。
    pub injection_expansions: u32,
    /// 运行期再展开次数（违例——主册「仅注入时执行一次」的红线计数）。
    pub runtime_violations: u32,
}

impl ExpandOnceLedger {
    pub const fn new() -> ExpandOnceLedger {
        ExpandOnceLedger { injection_expansions: 0, runtime_violations: 0 }
    }

    /// 注入期展开（合法路径——放行并计数）。
    pub fn note_injection(&mut self) {
        self.injection_expansions += 1;
    }

    /// 运行期展开请求：恒拒绝（违例如实计数——运行期值就是注入后的字面值）。
    pub fn request_runtime_expand(&mut self) -> bool {
        self.runtime_violations += 1;
        false
    }

    /// 纪律恒等式：违例恒 0 才算会话面干净（诊断口径）。
    pub fn clean(&self) -> bool {
        self.runtime_violations == 0
    }
}

/// COMPUTERNAME 缓冲上限（F123 设备名口径——超长设备名如实截断计数）。
pub const COMPUTERNAME_CAP: usize = 32;

/// COMPUTERNAME 取值（F123「关于本机」同源）：设备名**原样**进会话变量，
/// 零变换零二次格式化——两处显示必须逐字节一致（一处一事实）。
/// 返回 (写入字节数, 是否被截断)。
pub fn computername_of(device_name: &str, buf: &mut [u8]) -> (usize, bool) {
    let src = device_name.as_bytes();
    let n = src.len().min(buf.len()).min(COMPUTERNAME_CAP);
    buf[..n].copy_from_slice(&src[..n]);
    (n, src.len() > n)
}

/// F011 深化批次三自检。
pub fn run_envsess_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep2");
    // 1) 注入期展开合法计数；运行期展开恒拒绝且计违例；干净判定。
    let mut led = ExpandOnceLedger::new();
    led.note_injection();
    led.note_injection();
    let runtime_ok = led.request_runtime_expand();
    cs.add(
        "expand_once_discipline",
        led.injection_expansions == 2
            && !runtime_ok
            && led.runtime_violations == 1
            && !led.clean(),
        "",
    );
    // 2) 违例清零后的干净会话（零违例 = clean——异常零静默的对偶面）。
    let mut led2 = ExpandOnceLedger::new();
    led2.note_injection();
    cs.add("expand_once_clean_session", led2.clean() && led2.injection_expansions == 1, "");
    // 3) COMPUTERNAME 同源保真：原样逐字节一致；短缓冲如实截断并报告。
    let mut buf = [0u8; COMPUTERNAME_CAP];
    let (n1, trunc1) = computername_of("Y7000-DEV", &mut buf);
    let same = &buf[..n1] == b"Y7000-DEV";
    let mut small = [0u8; 4];
    let (n2, trunc2) = computername_of("Y7000-DEV", &mut small);
    cs.add(
        "computername_same_source_as_f123",
        n1 == 9 && !trunc1 && same && n2 == 4 && trunc2,
        "",
    );
    cs
}
