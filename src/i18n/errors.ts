import type { MessageKey } from './messages';

type Translator = (key: MessageKey, params?: Record<string, string | number>) => string;

export function localizeKnownError(message: string, t: Translator): string {
  const exact: Record<string, MessageKey> = {
    文件不存在: 'error.fileMissing',
    路径不存在: 'error.pathMissing',
    '条目已加密,暂不支持': 'error.encryptedEntry',
    '该条目不是文本日志,无法查看': 'error.notText',
    目录不在已配置的监控范围内: 'error.outsideWatch',
    名称不能为空: 'error.emptyName',
    文件名不能为空: 'error.emptyName',
    名称不能包含路径分隔符: 'error.pathSeparator',
    文件名不能包含路径分隔符: 'error.pathSeparator',
  };
  if (exact[message]) return t(exact[message]);
  const entry = message.match(/^条目不存在:\s*(.+)$/);
  if (entry) return t('error.entryMissing', { path: entry[1] });
  const directory = message.match(/^目录不存在:\s*(.+)$/);
  if (directory) return t('error.directoryMissing', { path: directory[1] });
  return message;
}
