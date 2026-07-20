import { useEffect, useRef, useState } from 'react';

export type LogTabStatus = 'opening' | 'ready' | 'dormant' | 'error';

export interface LogTabItem {
  id: string;
  title: string;
  absolutePath: string;
  status: LogTabStatus;
}

interface Props {
  tabs: Readonly<Record<string, LogTabItem>>;
  visibleIds: readonly string[];
  overflowIds: readonly string[];
  activeId: string | null;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onCapacityChange: (capacity: number) => void;
}

const TAB_WIDTH = 160;
const MORE_WIDTH = 96;

export function LogTabs({
  tabs,
  visibleIds,
  overflowIds,
  activeId,
  onActivate,
  onClose,
  onCapacityChange,
}: Props) {
  const barRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const moreButtonRef = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const totalCount = visibleIds.length + overflowIds.length;

  useEffect(() => {
    const element = barRef.current;
    if (!element) return;
    const update = (width: number) => {
      const withoutMore = Math.max(1, Math.floor(width / TAB_WIDTH));
      const capacity =
        totalCount > withoutMore
          ? Math.max(1, Math.floor((width - MORE_WIDTH) / TAB_WIDTH))
          : withoutMore;
      onCapacityChange(capacity);
    };
    update(element.clientWidth);
    const observer = new ResizeObserver((entries) => update(entries[0].contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, [onCapacityChange, totalCount]);

  useEffect(() => {
    if (overflowIds.length === 0) setMenuOpen(false);
  }, [overflowIds.length]);

  useEffect(() => {
    if (!menuOpen) return;
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (menuRef.current?.contains(target) || moreButtonRef.current?.contains(target)) return;
      setMenuOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setMenuOpen(false);
      moreButtonRef.current?.focus();
    };
    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [menuOpen]);

  return (
    <div
      className={'log-tabs' + (overflowIds.length > 0 ? ' has-overflow' : '')}
      ref={barRef}
      role="tablist"
      aria-label="已打开日志"
    >
      <div className="log-tabs-visible">
        {visibleIds.map((id) => {
          const tab = tabs[id];
          if (!tab) return null;
          return (
            <div
              key={id}
              className={'log-tab' + (activeId === id ? ' active' : '')}
              title={tab.absolutePath}
            >
              <button
                type="button"
                role="tab"
                aria-selected={activeId === id}
                className="log-tab-main"
                onClick={() => onActivate(id)}
              >
                <span className={`log-tab-status ${tab.status}`} aria-hidden="true" />
                <span className="log-tab-title">{tab.title}</span>
              </button>
              <button
                type="button"
                className="log-tab-close"
                aria-label={`关闭 ${tab.title}`}
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(id);
                }}
              >
                ×
              </button>
            </div>
          );
        })}
      </div>

      {overflowIds.length > 0 && (
        <div className="log-tabs-more-wrap">
          <button
            ref={moreButtonRef}
            type="button"
            className={'log-tabs-more' + (menuOpen ? ' active' : '')}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            更多 ({overflowIds.length}) ▾
          </button>
          {menuOpen && (
            <div className="log-tabs-menu" ref={menuRef} role="menu">
              {overflowIds.map((id) => {
                const tab = tabs[id];
                if (!tab) return null;
                return (
                  <div key={id} className="log-tabs-menu-item" title={tab.absolutePath}>
                    <button
                      type="button"
                      className="log-tabs-menu-main"
                      role="menuitem"
                      onClick={() => {
                        setMenuOpen(false);
                        onActivate(id);
                      }}
                    >
                      <span className={`log-tab-status ${tab.status}`} aria-hidden="true" />
                      <span className="log-tab-title">{tab.title}</span>
                    </button>
                    <button
                      type="button"
                      className="log-tab-close"
                      aria-label={`关闭 ${tab.title}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        onClose(id);
                      }}
                    >
                      ×
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
