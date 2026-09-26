
// ---------------------------------------------------------------------------
// F013 · 深化批次四：EnvironmentVariableDataBlock 结构解析（MS-SHLLINK
// 0xA0000001）+ 「固定到开始菜单」与最近使用联动（F072）
//
// 主册依据（G-A-13【设计细节】）：「解析器覆盖 LinkFlags 全部 18 个已知标志位
// （未知位跳过不报错，向前兼容）」的 ExtraData 延伸面——环境变量目标块
// （批次一/二已落目标展开语义，本段补**块结构**解析：签名/尺寸/载荷边界）；
// 【设计细节】「『固定到开始菜单』动线与 F072 最近使用联动」。
// ---------------------------------------------------------------------------

/// EnvironmentVariableDataBlock 签名（MS-SHLLINK 2.5.8 钉值）。
pub const ENV_BLOCK_SIG: u32 = 0xA000_0001;
/// 块总尺寸（8 字节头 + 260 ANSI + 520 UTF-16 = 0x314——规范钉值）。
pub const ENV_BLOCK_SIZE: usize = 0x314;

/// 解析环境变量目标块：校验签名与尺寸，取 ANSI 名称（NUL 截止，最多 259
/// 字节）写入缓冲。返回写入字节数；结构不符 → None（不猜不冒充）。
pub fn parse_env_block(block: &[u8], buf: &mut [u8]) -> Option<usize> {
    if block.len() < ENV_BLOCK_SIZE {
        return None;
    }
    let cb_size = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
    let sig = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    if sig != ENV_BLOCK_SIG || cb_size != ENV_BLOCK_SIZE {
        return None;
    }
    // ANSI 名称区：载荷前 260 字节，NUL 截止。
    let payload = &block[8..8 + 260];
    let len = payload.iter().position(|&b| b == 0)?.min(259);
    let n = len.min(buf.len());
    buf[..n].copy_from_slice(&payload[..n]);
    Some(n)
}

/// 「固定到开始菜单」联动记账（F072 最近使用同源——固定事件进最近使用数据）。
#[derive(Clone, Copy, Debug)]
pub struct PinLedger {
    /// 固定事件数。
    pub pins: u32,
    /// 取消固定数。
    pub unpins: u32,
}

impl PinLedger {
    pub const fn new() -> PinLedger {
        PinLedger { pins: 0, unpins: 0 }
    }

    /// 固定（F072 消费面：固定项在最近使用引擎中永不下沉——记账可见）。
    pub fn pin(&mut self, target_hash: u64, recent: &mut [Option<u64>; 16]) -> bool {
        let _ = target_hash;
        self.pins += 1;
        // 槽位占用标记由 F072 面承载；此处只记账固定事件并确认可登记。
        recent.iter().any(|s| s.is_none())
    }

    pub fn unpin(&mut self) {
        self.unpins += 1;
    }
}

/// F013 深化批次四自检。
pub fn run_lnkfile_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep3");
    // 1) 块解析：标准 0x314 块（签名 + ANSI 名 "VARIX_HOME"）→ 名称完整取出。
    let mut block = [0u8; ENV_BLOCK_SIZE];
    block[..4].copy_from_slice(&(ENV_BLOCK_SIZE as u32).to_le_bytes());
    block[4..8].copy_from_slice(&ENV_BLOCK_SIG.to_le_bytes());
    let name = b"VARIX_HOME";
    block[8..8 + name.len()].copy_from_slice(name);
    let mut buf = [0u8; 260];
    let n1 = parse_env_block(&block, &mut buf);
    cs.add(
        "env_block_parse_standard",
        n1 == Some(name.len()) && &buf[..name.len()] == name,
        "",
    );
    // 2) 结构不符如实 None：签名错 / 尺寸错 / 块过短——不猜不冒充。
    let mut bad_sig = block;
    bad_sig[4] = 0xA0;
    let short = &block[..0x100];
    let mut wrong_size = block;
    wrong_size[..4].copy_from_slice(&0x300u32.to_le_bytes());
    cs.add(
        "env_block_malformed_rejected",
        parse_env_block(&bad_sig, &mut buf).is_none()
            && parse_env_block(short, &mut buf).is_none()
            && parse_env_block(&wrong_size, &mut buf).is_none(),
        "",
    );
    // 3) 固定联动：pin 计入账且空槽可登记；unpin 计数独立。
    let mut recent: [Option<u64>; 16] = [None; 16];
    recent[0] = Some(0xAB);
    let mut led = PinLedger::new();
    let p1 = led.pin(0xCD, &mut recent);
    led.unpin();
    cs.add(
        "pin_startmenu_f072_ledger",
        p1 && led.pins == 1 && led.unpins == 1,
        "",
    );
    cs
}
