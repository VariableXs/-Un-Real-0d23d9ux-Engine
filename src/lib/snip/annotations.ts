/**
 * AI-07 · N-16 截图标注（矢量标注层模型，纯前端可测）：
 * - 六工具：箭头/矩形/画笔/文字/序号步进/马赛克
 * - 马赛克 = 真破坏性像素处理（导出时栅格化，标注「不可逆」并走确认门）
 * - 流向四选一：复制 / 存文件 / 进记录库 / 贴图（由 UI 层接）
 * 红线：OCR 仅本地引擎（V-42 Windows.Media.Ocr，缺语言包诚实提示）；
 * 滚动截图依赖窗口滚动协议——不可滚动的窗口如实禁用。
 */

export type AnnotationTool = "arrow" | "rect" | "pen" | "text" | "number" | "mosaic";

export interface Point {
  x: number;
  y: number;
}

export interface Annotation {
  id: string;
  tool: AnnotationTool;
  /** 箭头/矩形/文字/马赛克：起止两点；画笔：自由轨迹；序号：单点。 */
  points: Point[];
  color: string;
  size: number;
  /** 文字工具的文本；序号工具的步进号（导出时分配）。 */
  text?: string;
}

/** 破坏性判定（确认门依据：导出即像素级破坏，不可撤销）。 */
export function isDestructive(a: Annotation): boolean {
  return a.tool === "mosaic";
}

/** 序号步进分配（number 工具按创建顺序 1,2,3…）。 */
export function assignStepNumbers(annotations: Annotation[]): Annotation[] {
  let n = 0;
  return annotations.map((a) => (a.tool === "number" ? { ...a, text: String(++n) } : a));
}

/** 马赛克块参数计算（块大小 = 笔刷 size×4，向下取整）。 */
export function mosaicBlocks(a: Annotation, blockSize = 8): { x: number; y: number; w: number; h: number }[] {
  if (a.tool !== "mosaic" || a.points.length < 2) return [];
  const [p0, p1] = [a.points[0]!, a.points[a.points.length - 1]!];
  const x = Math.min(p0.x, p1.x);
  const y = Math.min(p0.y, p1.y);
  const w = Math.abs(p1.x - p0.x);
  const h = Math.abs(p1.y - p0.y);
  const blocks: { x: number; y: number; w: number; h: number }[] = [];
  for (let by = 0; by < h; by += blockSize) {
    for (let bx = 0; bx < w; bx += blockSize) {
      blocks.push({
        x: x + bx,
        y: y + by,
        w: Math.min(blockSize, w - bx),
        h: Math.min(blockSize, h - by),
      });
    }
  }
  return blocks;
}

/** 导出前校验：含破坏性标注需确认门（返回是否需要确认）。 */
export function needsMosaicConfirm(annotations: Annotation[]): boolean {
  return annotations.some(isDestructive);
}

/** OCR 引擎可用性（V-42：Windows.Media.Ocr 语言包检测，运行时探测注入）。 */
export interface OcrEngineStatus {
  available: boolean;
  /** 缺失语言包的诚实提示 key（zh/en 词典）。 */
  missingHintKey: string;
  languages: string[];
}

export function ocrStatus(languages: string[], wantsZh = true): OcrEngineStatus {
  const hasZh = languages.some((l) => l.toLowerCase().startsWith("zh"));
  const hasEn = languages.some((l) => l.toLowerCase().startsWith("en"));
  const available = wantsZh ? hasZh : hasEn;
  return {
    available,
    missingHintKey: available ? "" : "ocrMissingLangpack",
    languages,
  };
}
