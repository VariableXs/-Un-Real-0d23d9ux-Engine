/**
 * N-09 第三方组件宿主：iframe sandbox="allow-scripts"（无 same-origin）。
 * - 入站仅 {type:"widget:refresh"} 心跳（e.source 严格比对，防伪造源）；
 * - 出站按 manifest 权限放行 widget:tick / widget:todos / widget:net 只读消息；
 * - 心跳超时 → bumpFailure，连续 3 次 → onCollapse（Board 收起 + toast）。
 */
import { useEffect, useRef } from "react";
import {
  bumpFailure, canSendOutbound, hasPermission, isInboundAllowed, resetFailures,
  shouldCollapse, type WidgetFailureDoc, type WidgetManifest, type WidgetOutboundMessage,
} from "../manifest";
import { loadTodos } from "./common";

export interface ThirdPartyHostProps {
  manifest: WidgetManifest;
  html: string;
  paused: boolean;
  failures: WidgetFailureDoc;
  onFailures: (next: WidgetFailureDoc) => void;
  onCollapse: (id: string) => void;
}

export default function ThirdPartyHost(props: ThirdPartyHostProps): React.ReactElement {
  const { manifest, html, paused, failures, onFailures, onCollapse } = props;
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const lastBeatRef = useRef(Date.now());

  useEffect(() => {
    const onMsg = (e: MessageEvent): void => {
      if (e.source !== frameRef.current?.contentWindow) return;
      if (!isInboundAllowed(e.data)) return; // 白名单外一律拒绝
      lastBeatRef.current = Date.now();
      onFailures(resetFailures(failures, manifest.id));
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, [failures, manifest.id, onFailures]);

  // 出站刷新通道 + 心跳超时监工
  useEffect(() => {
    const period = Math.max(5, manifest.refresh) * 1000;
    const id = window.setInterval(() => {
      if (paused) return;
      const win = frameRef.current?.contentWindow;
      if (win) {
        const out: WidgetOutboundMessage[] = [
          { type: "widget:tick", time: new Date().toISOString() },
          { type: "widget:todos", items: loadTodos().map(({ id, text, done }) => ({ id, text, done })) },
          { type: "widget:net", online: navigator.onLine },
        ];
        for (const m of out) {
          if (canSendOutbound(manifest, m)) win.postMessage(m, "*");
        }
      }
      if (Date.now() - lastBeatRef.current > Math.max(period * 3, 15_000)) {
        const next = bumpFailure(failures, manifest.id);
        onFailures(next);
        lastBeatRef.current = Date.now();
        if (shouldCollapse(next, manifest.id)) onCollapse(manifest.id);
      }
    }, period);
    return () => window.clearInterval(id);
  }, [paused, manifest, failures, onFailures, onCollapse]);

  void hasPermission; // 权限判断经 canSendOutbound 使用；保留导出语义

  return (
    <iframe
      ref={frameRef}
      className="wgt-frame"
      // 沙箱红线：只给脚本执行，不给 same-origin / 不给表单 / 不给弹窗。
      sandbox="allow-scripts"
      title={manifest.name}
      srcDoc={html}
      csp={undefined}
    />
  );
}