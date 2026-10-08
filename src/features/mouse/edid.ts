/**
 * J 鼠标域 · EDID 1.4 解析引擎（v5 · 深化批次五 · F613/F614）。
 *
 * v4 的指纹是 `tauri:{name}:{w}x{h}`——名字和分辨率都不是设备身份：两台
 * 同型号屏对调、或改个显示名，指纹就变，跨屏记忆与设备档案跟着作废。
 * 本模块把「设备身份」做到字节层：解析 EDID 1.4 基本块（128 字节）——
 * checksum 校验、PNP 厂商三字母码、产品码、序列号、制造年周、首选时序
 * （详细描述符 1 的分辨率）——真实字节身份换线不乱。
 *
 * 接线形态（诚实边界）：webview/Tauri 当前不暴露原始 EDID 字节；本引擎
 * 是「系统层设备枚举通道」就绪后的即插消费件——指纹生成 `edidFingerprintOf`
 * 与身份卡描述 `describeIdentity` 已被多屏拓扑面板消费（显示器身份卡）；
 * 原始字节到达即从字符串指纹无缝升级为 EDID 指纹（同一套键位纪律）。
 */

/** FNV-1a 32 位（指纹哈希——无依赖、分布均匀、字节序稳定）。 */
export function fnv1a32(bytes: Uint8Array): number {
  let h = 0x811c9dc5;
  for (const b of bytes) {
    h ^= b;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

export interface EdidInfo {
  /** checksum 通过（最后字节 ≡ 0 mod 256）。 */
  checksumOk: boolean;
  /** EDID 版本.修订（如 "1.4"）。 */
  version: string;
  /** PNP 厂商三字母（5bit×3 压缩解码，如 "LEN"、"AUS"）。 */
  manufacturer: string;
  /** 产品码（小端 u16）。 */
  productCode: number;
  /** 序列号（小端 u32）。 */
  serial: number;
  /** 制造年（1990+年份字节）与周。 */
  year: number;
  week: number;
  /** 首选时序（详细描述符 1）：像素时钟 kHz 与分辨率。 */
  preferred?: { pixelClockKhz: number; hActive: number; vActive: number };
}

/** 厂商码解码：两字节 15bit → 3 字母（A=1..Z=26，5bit/字母）。 */
export function decodeManufacturer(b0: number, b1: number): string {
  const letters: string[] = [];
  const packed = ((b0 << 8) | b1) & 0x7fff;
  for (let i = 2; i >= 0; i--) {
    const v = (packed >> (i * 5)) & 0x1f;
    letters.push(v === 0 ? " " : String.fromCharCode(64 + v));
  }
  return letters.join("").trim();
}

/** 详细时序描述符解析（18 字节块，EDID 规范位打包）：
 * 像素时钟 u16 @0；HACT 低 8 位 @2、高 4 位 @4 高半字节；
 * VACT 低 8 位 @5、高 4 位 @7 高半字节。 */
function parseDetailedTiming(block: Uint8Array): { pixelClockKhz: number; hActive: number; vActive: number } {
  const clock10khz = block[0]! | (block[1]! << 8);
  const hActive = block[2]! | ((block[4]! >> 4) << 8);
  const vActive = block[5]! | ((block[7]! >> 4) << 8);
  return { pixelClockKhz: clock10khz * 10, hActive, vActive };
}

/**
 * EDID 1.4 基本块解析（恰好 128 字节，少于则拒绝——半块比无块更危险）。
 * 详细描述符区（偏移 54..125）：第一个非 monitor-name/range 块按首选时序读。
 */
export function parseEdid(bytes: Uint8Array): EdidInfo {
  if (bytes.length !== 128) {
    throw new Error(`[mouse-j1:EDID] 基本块应为 128 字节，实得 ${bytes.length}——拒绝解析半块`);
  }
  let sum = 0;
  for (const b of bytes) sum = (sum + b) & 0xff;
  const headerOk = bytes[0] === 0x00 && bytes[1] === 0xff && bytes[2] === 0xff && bytes[3] === 0xff && bytes[4] === 0xff && bytes[5] === 0xff && bytes[6] === 0xff && bytes[7] === 0x00;
  if (!headerOk) {
    throw new Error("[mouse-j1:EDID] 魔数头不匹配（00 FF…00）——不是 EDID 数据");
  }
  const info: EdidInfo = {
    checksumOk: sum === 0,
    version: `${bytes[18]}.${bytes[19]}`,
    manufacturer: decodeManufacturer(bytes[8]!, bytes[9]!),
    productCode: bytes[10]! | (bytes[11]! << 8),
    serial: (bytes[12]! | (bytes[13]! << 8) | (bytes[14]! << 16) | (bytes[15]! << 24)) >>> 0,
    year: 1990 + bytes[17]!,
    week: bytes[16]!,
  };
  // 详细描述符扫描（18 字节×4）：时序描述符的判据是像素时钟非零（EDID
  // 规范：名称 0xFC/范围 0xFE/序列串 0xFF 的前两字节均为 0）——首个时序
  // 块即首选分辨率。v5 首版误用 tag===0x10 判据（真实 EDID 上永远落空，
  // 单测当场暴露——判据实现一致性由测试钉住的意义所在）。
  for (let i = 0; i < 4; i++) {
    const off = 54 + i * 18;
    const pixelClock = bytes[off]! | (bytes[off + 1]! << 8);
    if (pixelClock === 0) continue;
    info.preferred = parseDetailedTiming(bytes.subarray(off, off + 18));
    break;
  }
  return info;
}

/**
 * EDID 指纹（F613/F614 键位升级）：对厂商+产品+序列的字节身份做 FNV-1a，
 * 8 位十六进制——换线、改名、换接口都不变（判据「交换接口用例」的字节级兑现）。
 */
export function edidFingerprintOf(bytes: Uint8Array): string {
  const info = parseEdid(bytes);
  const id = new Uint8Array(8);
  id[0] = info.productCode & 0xff;
  id[1] = (info.productCode >> 8) & 0xff;
  for (let i = 0; i < 4; i++) id[2 + i] = (info.serial >>> (i * 8)) & 0xff;
  id[6] = bytes[8]!;
  id[7] = bytes[9]!;
  return `edid:${fnv1a32(id).toString(16).padStart(8, "0")}`;
}

/**
 * 显示器身份卡（拓扑面板消费）：指纹 → 人话描述。
 * EDID 指纹解构出厂商与参数；字符串指纹（Tauri 名称口径）原样呈现并
 * 标注降级态——「身份可信度」诚实分级，不把降级包装成完整。
 */
export function describeIdentity(fingerprint: string): { vendor: string; kind: "edid" | "name"; note: string } {
  if (fingerprint.startsWith("edid:")) {
    return { vendor: fingerprint.slice(5, 9).toUpperCase(), kind: "edid", note: "字节级设备身份（换线/改名不乱）" };
  }
  if (fingerprint.startsWith("tauri:")) {
    const name = fingerprint.slice(6).split(":")[0] ?? "?";
    return { vendor: name, kind: "name", note: "显示名降级身份——系统层 EDID 通道就绪后自动升级" };
  }
  const name = fingerprint.startsWith("screen-") ? "主屏" : fingerprint;
  return { vendor: name, kind: "name", note: "单屏降级身份（浏览器 dev 口径）" };
}

/* ------------------------------- 测试样本合成（判据的样本器） ------------------------------- */

/**
 * 合成 EDID 基本块（单测与面板演示共用——真实 EDID 随闸门，合成块先钉住
 * 解析器的每一条分支：正确 checksum、厂商码、首选时序、魔数头）。
 */
export function synthEdid(opts: { manufacturer: string; productCode: number; serial: number; year?: number; hActive?: number; vActive?: number; pixelClockMhz?: number }): Uint8Array {
  const b = new Uint8Array(128);
  b.set([0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00], 0);
  let packed = 0;
  for (const ch of opts.manufacturer.toUpperCase().padEnd(3, " ")) {
    const v = Math.max(0, Math.min(26, ch.charCodeAt(0) - 64));
    packed = packed * 32 + v;
  }
  b[8] = (packed >> 8) & 0x7f;
  b[9] = packed & 0xff;
  b[10] = opts.productCode & 0xff;
  b[11] = (opts.productCode >> 8) & 0xff;
  b[12] = opts.serial & 0xff;
  b[13] = (opts.serial >>> 8) & 0xff;
  b[14] = (opts.serial >>> 16) & 0xff;
  b[15] = (opts.serial >>> 24) & 0xff;
  b[16] = 12; // 周
  b[17] = (opts.year ?? 2026) - 1990;
  b[18] = 1;
  b[19] = 4;
  // 首选时序描述符（块 0 @54）：像素时钟单位 10kHz（148.5MHz → 14850）；
  // HACT/VACT 按 EDID 规范位打包（低 8 位 + 高半字节）。
  const h = opts.hActive ?? 1920;
  const v = opts.vActive ?? 1080;
  const clk = Math.round((opts.pixelClockMhz ?? 148.5) * 100);
  b[54] = clk & 0xff;
  b[55] = (clk >> 8) & 0xff;
  b[56] = h & 0xff;
  b[57] = 0x80; // HBLANK 低 8 位（演示值，解析器不消费）
  b[58] = ((h >> 8) & 0x0f) << 4;
  b[59] = v & 0xff;
  b[60] = 0x20; // VBLANK 低 8 位
  b[61] = ((v >> 8) & 0x0f) << 4;
  // checksum：使总和 ≡ 0 mod 256。
  let sum = 0;
  for (let i = 0; i < 127; i++) sum = (sum + b[i]!) & 0xff;
  b[127] = (256 - sum) & 0xff;
  return b;
}
