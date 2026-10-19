import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { replaceSession, type SessionReplacementSteps } from './sessionReplacement';

interface FakeSession {
  sessionId: string;
}

function steps(overrides: Partial<SessionReplacementSteps<FakeSession>> = {}) {
  const events: string[] = [];
  const base: SessionReplacementSteps<FakeSession> = {
    revision: async () => {
      events.push('revision');
      return 'r2';
    },
    open: async () => {
      events.push('open');
      return { sessionId: 'next' };
    },
    waitForIndex: async () => {
      events.push('index');
      return 42;
    },
    stillCurrent: () => true,
    adopt: (session, lineCount, revision) => {
      events.push(`adopt:${session.sessionId}:${lineCount}:${revision}`);
    },
    close: async (sessionId) => {
      events.push(`close:${sessionId}`);
    },
  };
  return { events, steps: { ...base, ...overrides } };
}

describe('无缝替换会话', () => {
  it('替换会话完成索引后先接管再关闭旧会话', async () => {
    const { events, steps: replacement } = steps();
    assert.equal(await replaceSession('previous', replacement), 'replaced');
    assert.deepEqual(events, ['revision', 'open', 'index', 'adopt:next:42:r2', 'close:previous']);
  });

  it('修订读取失败不阻止替换', async () => {
    const { events, steps: replacement } = steps({
      revision: async () => {
        throw new Error('stat failed');
      },
    });
    assert.equal(await replaceSession('previous', replacement), 'replaced');
    assert.equal(events.includes('adopt:next:42:undefined'), true);
  });

  it('采用打开前读取的修订,索引期间的新写入仍会被下一次检查发现', async () => {
    let onDisk = 'r1';
    const { events, steps: replacement } = steps({
      revision: async () => onDisk,
      waitForIndex: async () => {
        onDisk = 'r2';
        return 10;
      },
    });
    await replaceSession('previous', replacement);
    assert.equal(events.includes('adopt:next:10:r1'), true);
    assert.notEqual(onDisk, 'r1');
  });

  it('被更新请求取代时丢弃替换会话并保留旧会话', async () => {
    const { events, steps: replacement } = steps({ stillCurrent: () => false });
    assert.equal(await replaceSession('previous', replacement), 'superseded');
    assert.deepEqual(events, ['revision', 'open', 'index', 'close:next']);
  });

  it('索引失败时关闭替换会话、保留旧会话并抛出错误', async () => {
    const { events, steps: replacement } = steps({
      waitForIndex: async () => {
        throw new Error('index failed');
      },
    });
    await assert.rejects(replaceSession('previous', replacement), /index failed/);
    assert.deepEqual(events, ['revision', 'open', 'close:next']);
  });

  it('打开失败时不关闭任何会话', async () => {
    const { events, steps: replacement } = steps({
      open: async () => {
        throw new Error('missing');
      },
    });
    await assert.rejects(replaceSession('previous', replacement), /missing/);
    assert.deepEqual(events, ['revision']);
  });
});
