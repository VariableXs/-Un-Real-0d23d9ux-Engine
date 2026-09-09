import { Component, type ErrorInfo, type ReactNode } from "react";
import { errMessage, ipc } from "../lib/ipc";
import { recordError } from "../lib/errBoard";

interface Props {
  children: ReactNode;
}
interface State {
  error: { code: string; message: string } | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(e: unknown): State {
    return { error: errMessage(e) };
  }

  componentDidCatch(e: unknown, info: ErrorInfo): void {
    console.error("[Variable] render error", e, info.componentStack);
    // AI-20 M-79：错误聚合看板（环形缓冲 + PII 清洗 + 退出落盘）
    recordError("ErrorBoundary", errMessage(e).message, e instanceof Error ? e.stack : info.componentStack);
    void ipc.log("error", `render error: ${errMessage(e).message}`).catch(() => {});
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="fatal-screen">
          <div className="fatal-card">
            <h2>⚠ Variable</h2>
            <p>出现错误，但你的数据是安全的。<br />Something went wrong. Your data is safe.</p>
            <code>{this.state.error.code}: {this.state.error.message}</code>
            <button
              className="btn primary"
              onClick={() => window.location.reload()}
              type="button"
            >
              重新加载应用 / Reload
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}