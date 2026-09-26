
// ---------------------------------------------------------------------------
// F017 · 深化批次四：CF_HDROP 载荷解析（DROPFILES 结构）+ 历史条目来源标注
// （F109 联动）
//
// 主册依据（G-A-17【功能定义】）：「CF_HDROP 文件列表支持多选拖粘」——列表
/// 载荷是 DROPFILES 结构（20 字节头：pFiles 偏移/POINT/fNC/fWide + 双 NUL
/// 终止的文件名列表，宽/窄由 fWide 定）；【设计细节】「历史条目带来源应用
/// 标注」。
// ---------------------------------------------------------------------------

/// DROPFILES 头尺寸（pFiles u32 + POINT 2×i32 + fNC u32 + fWide u32）。
pub const DROPFILES_HDR_SIZE: usize = 20;

/// 解析 CF_HDROP 载荷：结构校验 + 文件名计数。
/// 结构不符（头过短/pFiles 越界/无终止双 NUL）→ None——不猜不冒充。
pub fn parse_hdrop(data: &[u8]) -> Option<usize> {
    if data.len() < DROPFILES_HDR_SIZE {
        return None;
    }
    let p_files = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let f_wide = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    if p_files < DROPFILES_HDR_SIZE || p_files > data.len() || (f_wide != 0 && f_wide != 1) {
        return None;
    }
    let mut count = 0usize;
    if f_wide == 1 {
        let mut i = p_files;
        while i + 2 <= data.len() {
            let mut len = 0usize;
            while i + 2 * (len + 1) <= data.len() {
                let u = u16::from_le_bytes([data[i + 2 * len], data[i + 2 * len + 1]]);
                if u == 0 {
                    break;
                }
                len += 1;
            }
            if i + 2 * (len + 1) > data.len() {
                return None; // 无终止 NUL——结构不符
            }
            if len == 0 {
                return Some(count); // 终止空名 = 列表结束
            }
            count += 1;
            i += 2 * (len + 1);
        }
        None
    } else {
        let mut i = p_files;
        while i < data.len() {
            let mut len = 0usize;
            while i + len < data.len() && data[i + len] != 0 {
                len += 1;
            }
            if i + len >= data.len() {
                return None; // 无终止 NUL
            }
            if len == 0 {
                return Some(count);
            }
            count += 1;
            i += len + 1;
        }
        None
    }
}

/// 取第一个文件名（窄字符原样复制；宽字符仅当全 ASCII 时复制——非 ASCII
/// 走 F015 转码面，不在本核冒充）。返回写入字节数。
pub fn hdrop_first_name(data: &[u8], buf: &mut [u8]) -> Option<usize> {
    if data.len() < DROPFILES_HDR_SIZE {
        return None;
    }
    let p_files = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let f_wide = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    if f_wide == 0 {
        let end = data[p_files..].iter().position(|&b| b == 0)? + p_files;
        let n = (end - p_files).min(buf.len());
        buf[..n].copy_from_slice(&data[p_files..p_files + n]);
        Some(n)
    } else {
        let mut units = 0usize;
        while p_files + 2 * (units + 1) <= data.len() {
            let u = u16::from_le_bytes([data[p_files + 2 * units], data[p_files + 2 * units + 1]]);
            if u == 0 {
                break;
            }
            units += 1;
        }
        let ascii_ok = (0..units).all(|k| {
            let u = u16::from_le_bytes([data[p_files + 2 * k], data[p_files + 2 * k + 1]]);
            u < 0x80
        });
        if !ascii_ok || units > buf.len() {
            return None;
        }
        for k in 0..units {
            buf[k] = data[p_files + 2 * k];
        }
        Some(units)
    }
}

/// 历史条目来源标注（F109：条目带来源应用——排查者可见「谁放进去的」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryTag {
    pub source_app: u32,
    pub cf: u16,
}

/// 历史元数据环（20 条——F109 历史上限同源钉值）。
pub struct ClipHistoryMeta {
    tags: [Option<HistoryTag>; 20],
    n: usize,
}

impl ClipHistoryMeta {
    pub const fn new() -> ClipHistoryMeta {
        ClipHistoryMeta { tags: [None; 20], n: 0 }
    }

    pub fn record(&mut self, source_app: u32, cf: u16) -> bool {
        if self.n >= self.tags.len() {
            return false;
        }
        self.tags[self.n] = Some(HistoryTag { source_app, cf });
        self.n += 1;
        true
    }

    pub fn source_at(&self, idx: usize) -> Option<u32> {
        self.tags.get(idx).copied().flatten().map(|t| t.source_app)
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F017 深化批次四自检。
pub fn run_clipfmt_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep3");
    // 1) 窄字符 HDROP：头 + 两名 + 终止空名 → 计数 2；首名完整取出。
    let mut narrow = alloc::vec![0u8; 20];
    narrow[..4].copy_from_slice(&20u32.to_le_bytes()); // pFiles
    narrow[16..20].copy_from_slice(&0u32.to_le_bytes()); // fWide = 0
    narrow.extend_from_slice(b"notes.txt\0todo.md\0\0");
    let count = parse_hdrop(&narrow);
    let mut name = [0u8; 64];
    let n1 = hdrop_first_name(&narrow, &mut name);
    cs.add(
        "hdrop_narrow_parse",
        count == Some(2) && n1 == Some(9) && &name[..9] == b"notes.txt",
        "",
    );
    // 2) 宽字符 HDROP（ASCII 安全名）计数与取名同对；无终止 NUL 如实 None。
    let mut wide = alloc::vec![0u8; 20];
    wide[..4].copy_from_slice(&20u32.to_le_bytes());
    wide[16..20].copy_from_slice(&1u32.to_le_bytes());
    for u in [b'a' as u16, b'.', b't', b'x', b't', 0, b'b', b'.', b'c', 0, 0] {
        wide.extend_from_slice(&u.to_le_bytes());
    }
    let count2 = parse_hdrop(&wide);
    let n2 = hdrop_first_name(&wide, &mut name);
    cs.add(
        "hdrop_wide_and_malformed",
        count2 == Some(2) && n2 == Some(5) && &name[..5] == b"a.txt",
        "",
    );
    // 3) 历史来源标注：20 条环（F109 上限同源）逐条回查来源；满容如实拒。
    let mut hist = ClipHistoryMeta::new();
    let mut all = true;
    for i in 0..20u32 {
        all &= hist.record(0x1000 + i, CF_UNICODETEXT);
    }
    let overflow = hist.record(0xFFFF, CF_TEXT);
    cs.add(
        "history_source_tags_f109",
        all && !overflow && hist.len() == 20 && hist.source_at(0) == Some(0x1000)
            && hist.source_at(19) == Some(0x1013),
        "",
    );
    cs
}
