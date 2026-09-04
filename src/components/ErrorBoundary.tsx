import { Component, type ErrorInfo, type ReactNode } from "react";
import { errMessage, ipc } from "../lib/ipc";

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