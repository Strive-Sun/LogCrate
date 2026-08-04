import { useEffect, useState } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { listen } from '@tauri-apps/api/event';
import { api } from '../api';
import type { AiHistoryMessage, AiHistorySummary } from '../api';

export function AiConversationWindow() {
  const [history, setHistory] = useState<AiHistorySummary[]>([]);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [messages, setMessages] = useState<AiHistoryMessage[]>([]);
  const [title, setTitle] = useState('新对话');

  useEffect(() => {
    void api.listAiHistory().then(setHistory);
    let unlisten: (() => void) | undefined;
    void listen<{ providerId: string; model: string; content: string; selectedText: string }>('ai-result', (event) => {
      setMessages([{ role: 'user', content: event.payload.selectedText }, { role: 'assistant', content: event.payload.content }]);
      setTitle(`${event.payload.providerId} · ${event.payload.model}`);
    }).then((cleanup) => { unlisten = cleanup; });
    return () => unlisten?.();
  }, []);

  return <div className="ai-window-shell">
    <header className="ai-window-header">
      <div className="ai-history-menu">
        <button type="button" onClick={() => setHistoryOpen((open) => !open)}>历史对话⌄</button>
        {historyOpen && <div className="ai-history-dropdown">
          {history.length === 0 ? <div className="ai-history-empty">暂无历史对话</div> : history.map((item) => <button key={item.id} type="button" onClick={async () => { const record = await api.loadAiHistory(item.id); setMessages(record.messages); setTitle(record.title); setHistoryOpen(false); }}>{item.title}<small>{new Date(item.updatedAt).toLocaleString()}</small></button>)}
        </div>}
      </div>
      <strong>{title}</strong>
      <button type="button" className="ai-window-close" aria-label="关闭 AI 窗口" onClick={() => void getCurrentWebviewWindow().close()}>×</button>
    </header>
    <main className="ai-window-messages">
      {messages.length === 0 ? <div className="ai-window-empty"><div className="ai-window-logo">AI</div><h2>LogCrate AI</h2><p>选择日志文本并使用右键“AI 分析”，或从历史对话中继续。</p></div> : messages.map((message, index) => <div key={`${message.role}-${index}`} className={`ai-chat-message ${message.role}`}><div className="ai-chat-avatar">{message.role === 'user' ? '你' : 'AI'}</div><div className="ai-chat-bubble">{message.content}</div></div>)}
    </main>
  </div>;
}
