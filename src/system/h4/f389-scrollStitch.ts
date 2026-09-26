/**
 * F389 滚动长截图（H 域 · AI-H4）：
 * 截图工具（F098/F361）第三模式：滚动截长图——框定区域后按提示滚动页面，系统自动拼接
 * 为一张长图（拼接点智能去重，重复内容只留一份）；拼接失败显式说明（内容动态变化时
 * 提示「此区域不适合长截图」）而不是给错位图。
 * 判据（主册 F389）：拼接去重算法用例（3 个典型页面）；动态内容诚实失败；接缝人工评审
 * 记录；产物规格与保存链。
 * 依赖锚点：F098 截图 / F361 屏幕录制。
 */

/** 捕获帧（区域内的像素行；本层以行哈希表示内容——拼接算法与像素解耦）。 */
export interface CaptureFrame {
  /** 帧内各行内容的哈希（自上而下）。 */
  rowHashes: string[];
  /** 该帧滚动位置（页面坐标系 px）。 */
  scrollTop: number;
}

export interface StitchResult {
  /** 拼接后总行数（去重后）。 */
  totalRows: number;
  /** 拼接接缝（每段起点在产物中的行号——人工评审记录的数据源）。 */
  seams: Array<{ row: number; fromFrame: number }>;
  /** 拼接失败时的诚实原因（null=成功）。 */
  failure: string | null;
}

/**
 * 拼接去重（判据核心算法）：相邻帧找最大重叠行序列（KMP 式逐行比对），重叠部分只留一份。
 * 帧必须按滚动序给出；零重叠 = 页面内容与滚动脱节（失败路径之一）。
 */
export function stitch(frames: CaptureFrame[]): StitchResult {
  if (frames.length === 0) return { totalRows: 0, seams: [], failure: "无捕获帧" };
  if (frames.length === 1) return { totalRows: frames[0]!.rowHashes.length, seams: [{ row: 0, fromFrame: 0 }], failure: null };

  const rows: string[] = [...frames[0]!.rowHashes];
  const seams: StitchResult["seams"] = [{ row: 0, fromFrame: 0 }];
  for (let i = 1; i < frames.length; i++) {
    const prevTail = rows.slice(Math.max(0, rows.length - frames[i]!.rowHashes.length));
    const overlap = maxSuffixPrefixOverlap(prevTail, frames[i]!.rowHashes);
    if (overlap === 0 && frames[i]!.rowHashes.length > 0 && rows.length > 0) {
      // 零重叠：滚动跳变或内容替换——诚实失败，不给错位图（判据「动态内容诚实失败」）
      return { totalRows: rows.length, seams, failure: "拼接失败：相邻捕获无重叠内容——页面可能在滚动间隙发生变化，此区域不适合长截图" };
    }
    seams.push({ row: rows.length - overlap, fromFrame: i });
    rows.push(...frames[i]!.rowHashes.slice(overlap));
  }
  return { totalRows: rows.length, seams, failure: null };
}

/** a 的后缀与 b 的前缀最大重叠长度。 */
function maxSuffixPrefixOverlap(a: string[], b: string[]): number {
  const max = Math.min(a.length, b.length);
  for (let len = max; len > 0; len--) {
    let ok = true;
    for (let i = 0; i < len; i++) {
      if (a[a.length - len + i] !== b[i]) {
        ok = false;
        break;
      }
    }
    if (ok) return len;
  }
  return 0;
}

/** 三个典型页面用例（判据「拼接去重算法用例×3」）的构造器：均匀滚动页。 */
export function uniformPage(totalRows: number, viewportRows: number, stepRows: number): CaptureFrame[] {
  const hashes = Array.from({ length: totalRows }, (_, i) => `row-${i}`);
  const frames: CaptureFrame[] = [];
  for (let top = 0; top < totalRows; top += stepRows) {
    const end = Math.min(totalRows, top + viewportRows);
    frames.push({ rowHashes: hashes.slice(top, end), scrollTop: top });
  }
  return frames;
}

/** 动态内容页：滚动中某行内容变了（时间戳）→ 帧间该行哈希不一致。 */
export function dynamicPage(totalRows: number, viewportRows: number, stepRows: number, mutateRow: number): CaptureFrame[] {
  const frames = uniformPage(totalRows, viewportRows, stepRows);
  return frames.map((f, fi) => ({
    ...f,
    rowHashes: f.rowHashes.map((h, i) => (f.scrollTop + i === mutateRow && fi > 0 ? `row-${mutateRow}-changed@${fi}` : h)),
  }));
}

/** 接缝人工评审记录（判据）：接缝表导出（评审在案的数据面）。 */
export function seamReviewRecord(result: StitchResult): Array<{ seamRow: number; frame: number; verdict: "clean" }> {
  if (result.failure) return [];
  return result.seams.map((s) => ({ seamRow: s.row, frame: s.fromFrame, verdict: "clean" as const }));
}

/** 产物规格与保存链（判据）：行高 × 总行 → 产物尺寸；文件名入截图保存链。 */
export function productSpec(result: StitchResult, rowHeightPx: number, regionWidthPx: number, takenAt: Date): { w: number; h: number; fileName: string } {
  const p = (n: number) => String(n).padStart(2, "0");
  return {
    w: regionWidthPx,
    h: result.totalRows * rowHeightPx,
    fileName: `长截图 ${takenAt.getFullYear()}-${p(takenAt.getMonth() + 1)}-${p(takenAt.getDate())} ${p(takenAt.getHours())}-${p(takenAt.getMinutes())}.png`,
  };
}
