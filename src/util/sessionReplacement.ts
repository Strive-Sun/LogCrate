export interface ReplaceableSession {
  sessionId: string;
}

export interface SessionReplacementSteps<S extends ReplaceableSession> {
  /** 打开不接管选项卡的替换会话。 */
  open(): Promise<S>;
  /** 等待替换会话完成索引并返回总行数。 */
  waitForIndex(session: S): Promise<number>;
  /** 在打开前读取源修订:索引期间追加的内容会让下一次修订检查再次触发重新加载。 */
  revision(): Promise<string | undefined>;
  /** 选项卡仍存在、仍持有旧会话且没有更新的打开请求。 */
  stillCurrent(): boolean;
  /** 让选项卡改用替换会话。 */
  adopt(session: S, lineCount: number, revision: string | undefined): void;
  close(sessionId: string): Promise<void>;
}

/**
 * 无缝替换会话:替换会话就绪前旧会话保持显示;就绪后先接管再关闭旧会话。
 * 被更新的请求取代时丢弃替换会话;失败时关闭替换会话并保留旧会话,然后抛出原错误。
 */
export async function replaceSession<S extends ReplaceableSession>(
  previousSessionId: string,
  steps: SessionReplacementSteps<S>,
): Promise<'replaced' | 'superseded'> {
  const revision = await steps.revision().catch(() => undefined);
  let opened: S | null = null;
  try {
    opened = await steps.open();
    const lineCount = await steps.waitForIndex(opened);
    if (!steps.stillCurrent()) {
      await steps.close(opened.sessionId).catch(() => undefined);
      return 'superseded';
    }
    steps.adopt(opened, lineCount, revision);
  } catch (error) {
    if (opened) await steps.close(opened.sessionId).catch(() => undefined);
    throw error;
  }
  await steps.close(previousSessionId).catch(() => undefined);
  return 'replaced';
}
