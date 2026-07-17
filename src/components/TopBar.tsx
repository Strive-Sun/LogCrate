import { useEffect, useRef, useState } from 'react';
import type { AppUpdateInfo, AppUpdateProgress, NewLogItem } from '../api';
import type { UpdateStatus } from '../util/update';
import { SettingsPanel } from './SettingsPanel';

interface Props {
  theme: 'dark' | 'light';
  onToggleTheme: () => void;
  count: number;
  newItems: NewLogItem[];
  onOpenItem: (item: NewLogItem) => void;
  onMarkAll: () => void;
  appVersion: string;
  autoCheckUpdates: boolean;
  updateStatus: UpdateStatus;
  updateInfo: AppUpdateInfo | null;
  updateProgress: AppUpdateProgress | null;
  updateError: string | null;
  onAutoCheckUpdatesChange: (enabled: boolean) => void;
  onCheckForUpdates: () => void;
  onSkipUpdate: () => void;
  onDownloadUpdate: () => void;
}

export function TopBar(props: Props) {
  const { count, newItems } = props;
  const [bellOpen, setBellOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [ring, setRing] = useState(false);
  const prevCount = useRef(count);

  // 计数增加时铃铛抖动
  useEffect(() => {
    if (count > prevCount.current) {
      setRing(true);
      const t = setTimeout(() => setRing(false), 400);
      return () => clearTimeout(t);
    }
    prevCount.current = count;
  }, [count]);

  useEffect(() => {
    if (!bellOpen && !settingsOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setBellOpen(false);
      setSettingsOpen(false);
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [bellOpen, settingsOpen]);

  return (
    <div className="topbar">
      <span className="brand">LogPeek</span>
      <span className="search" title="搜索将在 M4 提供">
        🔍 搜索
      </span>
      <span className="spacer" />

      <button className="icon-btn" onClick={props.onToggleTheme} title="切换主题">
        {props.theme === 'dark' ? '🌙' : '☀️'}
      </button>
      <button
        className="icon-btn"
        onClick={() => {
          setSettingsOpen(false);
          setBellOpen((v) => !v);
        }}
        title="新日志提示"
      >
        <span className={'bell' + (ring ? ' ring' : '')}>🔔</span>
        {count > 0 && <span className="badge">{count > 99 ? '99+' : count}</span>}
      </button>
      <button
        className={'icon-btn' + (settingsOpen ? ' active' : '')}
        title="设置"
        aria-expanded={settingsOpen}
        onClick={() => {
          setBellOpen(false);
          setSettingsOpen((value) => !value);
        }}
      >
        ⚙️
      </button>

      {bellOpen && (
        <>
          <div className="backdrop" onClick={() => setBellOpen(false)} />
          <div className="pop bell-pop">
            <div className="pop-head">
              <span>新日志 ({newItems.length})</span>
              <button
                className="mark-all"
                onClick={() => {
                  props.onMarkAll();
                  setBellOpen(false);
                }}
              >
                全部标记已读
              </button>
            </div>
            {newItems.length === 0 && (
              <div className="pop-item" style={{ color: 'var(--fg-dim)' }}>
                没有未读的新日志
              </div>
            )}
            {newItems.map((it) => (
              <div
                className="pop-item"
                key={it.id}
                onClick={() => {
                  props.onOpenItem(it);
                  setBellOpen(false);
                }}
              >
                <span>{it.kind === 'archive' ? '📦' : '📄'}</span>
                <span>{it.name}</span>
                <span className="src">
                  {it.source}/ {it.age}
                </span>
              </div>
            ))}
          </div>
        </>
      )}

      {settingsOpen && (
        <>
          <div className="backdrop" onClick={() => setSettingsOpen(false)} />
          <SettingsPanel
            currentVersion={props.appVersion}
            autoCheck={props.autoCheckUpdates}
            status={props.updateStatus}
            update={props.updateInfo}
            progress={props.updateProgress}
            error={props.updateError}
            onAutoCheckChange={props.onAutoCheckUpdatesChange}
            onCheck={props.onCheckForUpdates}
            onSkip={props.onSkipUpdate}
            onDownload={props.onDownloadUpdate}
            onClose={() => setSettingsOpen(false)}
          />
        </>
      )}
    </div>
  );
}
