import type { VwmRect } from "../vwm";

/**
 * AURORA-10000 · 族0027 窗口布局引擎（F00651~F00675 · AI-06 · W1）。
 * 25 种平铺算法：给定工作区与窗口数 n，返回 n 个目标矩形。
 * 全部纯函数；调用方负责把矩形应用到 VWM（动画走 tokens）。
 */

export interface TileCtx {
  /** 工作区。 */
  work: VwmRect;
  /** 参与平铺的窗口数。 */
  n: number;
  /** 焦点窗序号（0 基，供焦点放大/鱼眼等）。 */
  focus?: number;
  /** 层叠步长（px），默认 24。 */
  cascadeStep?: number;
}

const r = (x: number, y: number, w: number, h: number): VwmRect => ({
  x: Math.round(x),
  y: Math.round(y),
  w: Math.max(1, Math.round(w)),
  h: Math.max(1, Math.round(h)),
});

/** 二分树：交替横竖递归对半。 */
function bintree(work: VwmRect, i: number, n: number, depth = 0): VwmRect {
  if (n <= 1) return work;
  const horizontal = depth % 2 === 1;
  const half = horizontal ? work.h / 2 : work.w / 2;
  if (i < Math.floor(n / 2)) {
    return bintree(
      horizontal ? r(work.x, work.y, work.w, half) : r(work.x, work.y, half, work.h),
      i,
      Math.floor(n / 2),
      depth + 1,
    );
  }
  return bintree(
    horizontal
      ? r(work.x, work.y + half, work.w, work.h - half)
      : r(work.x + half, work.y, work.w - half, work.h),
    i - Math.floor(n / 2),
    n - Math.floor(n / 2),
    depth + 1,
  );
}

/**
 * 族0027 主入口：按 ID 分派的平铺算法。
 * 返回长度恒为 n 的矩形数组（自由浮动等透传模式除外，也保持 n 长度以便统一应用）。
 */
export function tileLayout(itemId: string, ctx: FullTileCtx): VwmRect[] {
  const { work, n } = ctx;
  const focus = ctx.focus ?? 0;
  const step = ctx.cascadeStep ?? 24;
  const out: VwmRect[] = [];
  const W = work.w;
  const H = work.h;

  switch (itemId) {
    case "F00651":
      // 布局·二分树 — 方向键式二分平铺
      for (let i = 0; i < n; i++) out.push(bintree(work, i, n));
      return out;
    case "F00652": {
      // 布局·四分格 — 四窗等分网格
      const cols = n <= 2 ? n : 2;
      const rows = Math.ceil(n / cols);
      for (let i = 0; i < n; i++) {
        const c = i % cols;
        const ro = Math.floor(i / cols);
        out.push(r(work.x + (W / cols) * c, work.y + (H / rows) * ro, W / cols, H / rows));
      }
      return out;
    }
    case "F00653": {
      // 布局·主副列 — 左主右副经典 tiling
      const mw = n === 1 ? W : Math.round(W * 0.62);
      const rest = n - 1;
      out.push(r(work.x, work.y, mw, H));
      const sh = rest > 0 ? H / rest : 0;
      for (let i = 0; i < rest; i++) out.push(r(work.x + mw, work.y + sh * i, W - mw, sh));
      return out;
    }
    case "F00654": {
      // 布局·三列 — 中间宽两侧窄
      const sideW = Math.round(W * 0.25);
      const midW = W - 2 * sideW;
      for (let i = 0; i < n; i++) {
        if (i === 0) out.push(r(work.x, work.y, sideW, H));
        else if (i === n - 1 && n > 1) out.push(r(work.x + sideW + midW, work.y, sideW, H));
        else {
          const idx = i - 1;
          const cnt = Math.max(1, n - 2);
          const sh = H / cnt;
          out.push(r(work.x + sideW, work.y + sh * idx, midW, sh));
        }
      }
      return out;
    }
    case "F00655": {
      // 布局·蜂窝 — 六边形蜂窝拼布（列错位近似）
      const cols = Math.ceil(Math.sqrt(n * 1.4));
      const cw = W / cols;
      const chH = (H / Math.ceil(n / Math.max(1, cols))) * 0.9;
      for (let i = 0; i < n; i++) {
        const c = i % cols;
        const ro = Math.floor(i / cols);
        const offset = ro % 2 === 1 ? cw / 2 : 0;
        out.push(r(work.x + cw * c + offset, work.y + chH * ro, cw, chH * 1.1));
      }
      return out;
    }
    case "F00656": {
      // 布局·风车 — 四窗风车旋转对称
      const mw = Math.round(W * 0.6);
      const mh = Math.round(H * 0.6);
      const blades = [
        r(work.x, work.y, mw, mh),
        r(work.x + mw, work.y, W - mw, H - mh),
        r(work.x + mw, work.y + H - mh, mw, mh),
        r(work.x, work.y + mh, W - mw, H - mh),
      ];
      for (let i = 0; i < n; i++) out.push(blades[i % 4]!);
      return out;
    }
    case "F00657": {
      // 布局·螺旋 — 递归分割螺旋（每次切掉 1/3）
      let rect = work;
      let horizontal = false;
      for (let i = 0; i < n; i++) {
        const frac = 1 / 3;
        if (i === n - 1) {
          out.push(rect);
          break;
        }
        const cut = horizontal ? rect.h * frac : rect.w * frac;
        out.push(horizontal ? r(rect.x, rect.y, rect.w, cut) : r(rect.x, rect.y, cut, rect.h));
        rect = horizontal
          ? r(rect.x, rect.y + cut, rect.w, rect.h - cut)
          : r(rect.x + cut, rect.y, rect.w - cut, rect.h);
        horizontal = !horizontal;
      }
      return out;
    }
    case "F00658": {
      // 布局·黄金螺旋 — 黄金比 0.618 螺旋分割
      let rect = work;
      let horizontal = false;
      const phi = 0.618;
      for (let i = 0; i < n; i++) {
        if (i === n - 1) {
          out.push(rect);
          break;
        }
        const cut = horizontal ? rect.h * (1 - phi) : rect.w * phi;
        out.push(horizontal ? r(rect.x, rect.y, rect.w, cut) : r(rect.x, rect.y, cut, rect.h));
        rect = horizontal
          ? r(rect.x, rect.y + cut, rect.w, rect.h - cut)
          : r(rect.x + cut, rect.y, rect.w - cut, rect.h);
        horizontal = !horizontal;
      }
      return out;
    }
    case "F00659": {
      // 布局·名片式 — 竖向名片列表平铺
      const rh = H / n;
      for (let i = 0; i < n; i++) {
        const w = Math.min(W, Math.round(W * 0.8));
        const cx = work.x + (W - w) / 2;
        out.push(r(cx, work.y + rh * i, w, rh));
      }
      return out;
    }
    case "F00660": {
      // 布局·瀑布流 — 高度不齐瀑布排布（列高贪心最短）
      const cols = Math.min(n, Math.max(1, Math.round(W / 420)));
      const colH = new Array<number>(cols).fill(0);
      const cw = W / cols;
      for (let i = 0; i < n; i++) {
        let min = 0;
        for (let c = 1; c < cols; c++) if (colH[c]! < colH[min]!) min = c;
        const h = (H / Math.max(1, Math.ceil(n / cols))) * (1 + ((i % 3) - 1) * 0.08);
        out.push(r(work.x + cw * min, work.y + colH[min]!, cw, Math.max(h, H * 0.2)));
        colH[min] = colH[min]! + Math.max(h, H * 0.2);
      }
      return out;
    }
    case "F00661":
      // 布局·自由浮动 — 完全自由拖放模式（透传）
      return ctx.current ?? Array.from({ length: n }, () => ({ ...work }));
    case "F00662": {
      // 布局·手风琴 — 展开一窗收起其余
      const openH = H * 0.6;
      const rest = Math.max(1, n - 1);
      const closedH = (H - openH) / rest;
      for (let i = 0; i < n; i++) {
        if (i === focus) out.push(r(work.x, work.y, W, openH));
        else {
          const before = i < focus ? i : i - 1;
          out.push(r(work.x, work.y + openH + closedH * before, W, closedH));
        }
      }
      return out;
    }
    case "F00663": {
      // 布局·卡片甲板 — 底部卡片扇形甲板
      const cardW = Math.min(W / Math.max(1, n), 320);
      for (let i = 0; i < n; i++) {
        const mid = (n - 1) / 2;
        const dx = (i - mid) * (cardW * 0.55);
        const lift = Math.abs(i - mid) * 12;
        out.push(r(work.x + W / 2 - cardW / 2 + dx, work.y + H * 0.55 + lift, cardW, H * 0.45));
      }
      return out;
    }
    case "F00664": {
      // 布局·扇形 — 围绕中心扇形排布
      const cx = work.x + W / 2;
      const cy = work.y + H / 2;
      for (let i = 0; i < n; i++) {
        const ang = n === 1 ? 0 : (-Math.PI / 2) + ((i / (n - 1)) * Math.PI);
        const w = W * 0.4;
        const h = H * 0.4;
        out.push(r(cx + Math.cos(ang) * W * 0.25 - w / 2, cy + Math.sin(ang) * H * 0.25 - h / 2, w, h));
      }
      return out;
    }
    case "F00665": {
      // 布局·环形 — 焦点居中环形围绕
      const cw = W * 0.34;
      const ch = H * 0.4;
      out.splice(0, out.length);
      const rest = Math.max(0, n - 1);
      const ring = [];
      const centerRect = r(work.x + W / 2 - cw / 2, work.y + H / 2 - ch / 2, cw, ch);
      for (let i = 0; i < n; i++) {
        if (i === focus) ring.push(centerRect);
        else {
          const k = i < focus ? i : i - 1;
          const ang = (k / Math.max(1, rest)) * Math.PI * 2 - Math.PI / 2;
          ring.push(
            r(
              work.x + W / 2 + Math.cos(ang) * W * 0.32 - cw / 2,
              work.y + H / 2 + Math.sin(ang) * H * 0.32 - ch / 2,
              cw,
              ch,
            ),
          );
        }
      }
      return ring;
    }
    case "F00666": {
      // 布局·棋盘 — 黑白交错棋盘格
      const cols = Math.max(1, Math.ceil(Math.sqrt(n * 2)));
      const rows = Math.max(1, Math.ceil(n / cols));
      for (let i = 0; i < n; i++) {
        const c = i % cols;
        const ro = Math.floor(i / cols);
        out.push(r(work.x + (W / cols) * c, work.y + (H / rows) * ro, W / cols, H / rows));
      }
      return out;
    }
    case "F00667":
      // 布局·层叠 — 右下层叠 24px 链
      for (let i = 0; i < n; i++)
        out.push(r(work.x + step * i, work.y + step * i, W - step * (n - 1) * 0.4, H - step * (n - 1) * 0.4));
      return out;
    case "F00668":
      // 布局·标签组 — 同位多标签复用空间
      for (let i = 0; i < n; i++) out.push(r(work.x, work.y, W, H));
      return out;
    case "F00669": {
      // 布局·层叠平铺混合 — 左半层叠 + 右半平铺
      const halfW = W / 2;
      const cascadeCount = Math.max(1, Math.floor(n / 2));
      const tileCount = n - cascadeCount;
      for (let i = 0; i < cascadeCount; i++)
        out.push(r(work.x + step * i * 0.5, work.y + step * i * 0.5, halfW - step * i * 0.5, H - step * i * 0.5));
      const th = tileCount > 0 ? H / tileCount : 0;
      for (let i = 0; i < tileCount; i++) out.push(r(work.x + halfW, work.y + th * i, halfW, th));
      return out;
    }
    case "F00670": {
      // 布局·瀑布平铺混合 — 上平铺下瀑布
      const topH = H * 0.55;
      const tileN = Math.max(1, Math.ceil(n / 2));
      for (let i = 0; i < Math.min(tileN, n); i++)
        out.push(r(work.x + (W / tileN) * i, work.y, W / tileN, topH));
      const botStart = Math.min(tileN, n);
      for (let i = botStart; i < n; i++) {
        const k = i - botStart;
        const cnt = Math.max(1, n - botStart);
        out.push(r(work.x + (W / cnt) * k, work.y + topH, W / cnt, H - topH));
      }
      return out;
    }
    case "F00671": {
      // 布局·焦点放大 — 焦点窗 70% 其余均分
      const fw = W * 0.7;
      const fh = H * 0.7;
      const rest = Math.max(1, n - 1);
      for (let i = 0; i < n; i++) {
        if (i === focus) out.push(r(work.x + (W - fw) / 2, work.y + (H - fh) / 2, fw, fh));
        else {
          const k = i < focus ? i : i - 1;
          out.push(r(work.x, work.y + H - fh + ((H * 0.3) / rest) * k, W / rest, (H * 0.3) / rest));
        }
      }
      return out;
    }
    case "F00672": {
      // 布局·鱼眼 — 焦点区放大边缘压缩（宽度按距离衰减）
      const weights: number[] = [];
      let total = 0;
      for (let i = 0; i < n; i++) {
        const d = Math.abs(i - focus);
        const wgt = 1 / (1 + d * 0.8);
        weights.push(wgt);
        total += wgt;
      }
      let x = work.x;
      for (let i = 0; i < n; i++) {
        const w = (weights[i]! / total) * W;
        out.push(r(x, work.y, w, H));
        x += w;
      }
      return out;
    }
    case "F00673": {
      // 布局·缩略墙 — 全部缩略图均匀墙
      const cols = Math.max(1, Math.ceil(Math.sqrt(n)));
      const rows = Math.max(1, Math.ceil(n / cols));
      for (let i = 0; i < n; i++) {
        const c = i % cols;
        const ro = Math.floor(i / cols);
        const pad = 8;
        out.push(
          r(
            work.x + (W / cols) * c + pad,
            work.y + (H / rows) * ro + pad,
            W / cols - pad * 2,
            H / rows - pad * 2,
          ),
        );
      }
      return out;
    }
    case "F00674":
      // 布局·临时全屏 — 单窗全屏一键切换（焦点窗全屏，其余最小化占位）
      for (let i = 0; i < n; i++) out.push(i === focus ? r(work.x, work.y, W, H) : r(work.x, work.y, 1, 1));
      return out;
    case "F00675":
      // 布局·原样恢复 — 回到手排自由布局（透传当前矩形）
      return ctx.current ?? Array.from({ length: n }, () => ({ ...work }));
    default:
      return Array.from({ length: n }, () => ({ ...work }));
  }
}

/** TileCtx 扩展：自由浮动/原样恢复需要当前矩形。 */
export interface TileExtra {
  current?: VwmRect[];
}

/** 带扩展字段的平铺上下文。 */
export type FullTileCtx = TileCtx & TileExtra;
