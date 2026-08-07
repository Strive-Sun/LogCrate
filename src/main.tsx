import React, { useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { I18nProvider } from './i18n/I18nProvider';
import { AiWorkspaceProvider } from './components/AiWorkspaceContext';
import { beginStartupHandoff } from './startup';
import './styles.css';

function StartupLifecycle() {
  useEffect(() => beginStartupHandoff(), []);
  return null;
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <I18nProvider>
      <StartupLifecycle />
      <AiWorkspaceProvider>
        <App />
      </AiWorkspaceProvider>
    </I18nProvider>
  </React.StrictMode>,
);
