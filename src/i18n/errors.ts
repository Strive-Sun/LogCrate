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
  if (message.includes('已加密') || message.includes('密码缺失')) {
    return t('error.encryptedArchive');
  }
  if (message.includes('分卷归档暂不支持') || message.includes('换卷')) {
    return t('error.multiVolumeArchive');
  }
  if (message.includes('嵌套归档超过最大深度')) return t('error.nestedDepth');
  if (message.includes('累计解码内容超过') || message.includes('实际解码内容超过')) {
    return t('error.nestedBytes');
  }
  if (message.includes('归档条目数量超过安全上限')) return t('error.archiveEntries');
  if (message.includes('归档扫描输入超过') || message.includes('归档扫描解码内容超过')) {
    return t('error.archiveScanBytes');
  }
  if (message.includes('归档扫描超过') && message.includes('时间上限')) {
    return t('error.archiveScanTime');
  }
  if (message.includes('归档已损坏') || message.includes('ChecksumVerificationFailed')) {
    return t('error.archiveDamaged');
  }
  const entry = message.match(/^条目不存在:\s*(.+)$/);
  if (entry) return t('error.entryMissing', { path: entry[1] });
  const directory = message.match(/^目录不存在:\s*(.+)$/);
  if (directory) return t('error.directoryMissing', { path: directory[1] });
  return message;
}
