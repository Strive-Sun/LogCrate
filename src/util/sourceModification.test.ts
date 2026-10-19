import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  DEFAULT_SOURCE_MODIFIED_ACTION,
  SOURCE_MODIFIED_ACTION_STORAGE_KEY,
  createSourceReloadQueue,
  decideSourceRevisionChange,
  loadSourceModifiedAction,
  saveSourceModifiedAction,
} from './sourceModification';

const samePath = (left: string, right: string) => left.toLowerCase() === right.toLowerCase();

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => (resolve = done));
  return { promise, resolve };
}

describe('外部修改处理偏好', () => {
  it('缺失、未知值或存储不可用时回退每次询问', () => {
    const values = new Map<string, string>();
    const storage = { getItem: (key: string) => values.get(key) ?? null };
    assert.equal(loadSourceModifiedAction(storage), DEFAULT_SOURCE_MODIFIED_ACTION);
    values.set(SOURCE_MODIFIED_ACTION_STORAGE_KEY, 'always');
    assert.equal(loadSourceModifiedAction(storage), 'ask');
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
    };
    assert.equal(loadSourceModifiedAction(broken), 'ask');
  });

  it('保存后可恢复，写入失败不抛出', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };
    saveSourceModifiedAction(storage, 'reload');
    assert.equal(loadSourceModifiedAction(storage), 'reload');
    saveSourceModifiedAction(storage, 'ignore');
    assert.equal(loadSourceModifiedAction(storage), 'ignore');
    assert.doesNotThrow(() =>
      saveSourceModifiedAction(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        'reload',
      ),
    );
  });

  it('按偏好分派修订变化，自动重载失败的同一修订不再重试', () => {
    assert.equal(decideSourceRevisionChange('ask', 'r2'), 'prompt');
    assert.equal(decideSourceRevisionChange('ignore', 'r2'), 'remember');
    assert.equal(decideSourceRevisionChange('reload', 'r2'), 'reload');
    assert.equal(decideSourceRevisionChange('reload', 'r2', 'r2'), 'none');
    assert.equal(decideSourceRevisionChange('reload', 'r3', 'r2'), 'reload');
    assert.equal(decideSourceRevisionChange('ask', 'r2', 'r2'), 'prompt');
  });
});

describe('按源串行的重新加载', () => {
  it('在途期间丢弃自动请求，显式请求在结束后合并执行一次', async () => {
    const queue = createSourceReloadQueue(samePath);
    const first = deferred();
    const calls: string[] = [];
    const running = queue.run('C:\\Logs\\a.log', 'auto', async () => {
      calls.push('auto-1');
      await first.promise;
    });
    assert.equal(queue.isRunning('c:\\logs\\A.log'), true);

    void queue.run('C:\\Logs\\a.log', 'auto', async () => void calls.push('auto-2'));
    void queue.run('C:\\Logs\\a.log', 'explicit', async () => void calls.push('manual-1'));
    void queue.run('C:\\Logs\\a.log', 'explicit', async () => void calls.push('manual-2'));
    assert.deepEqual(calls, ['auto-1']);

    first.resolve();
    await running;
    assert.deepEqual(calls, ['auto-1', 'manual-2']);
    assert.equal(queue.isRunning('C:\\Logs\\a.log'), false);
  });

  it('不同源互不阻塞，任务失败后释放该源', async () => {
    const queue = createSourceReloadQueue(samePath);
    const gate = deferred();
    const calls: string[] = [];
    const slow = queue.run('/a.log', 'auto', async () => {
      await gate.promise;
      throw new Error('open failed');
    });
    await queue.run('/b.log', 'auto', async () => void calls.push('b'));
    assert.deepEqual(calls, ['b']);

    gate.resolve();
    await slow;
    assert.equal(queue.isRunning('/a.log'), false);
    await queue.run('/a.log', 'auto', async () => void calls.push('a-retry'));
    assert.deepEqual(calls, ['b', 'a-retry']);
  });
});
