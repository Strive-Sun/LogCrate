import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const workflow = await readFile('.github/workflows/release.yml', 'utf8');

function includes(value, message) {
  assert.ok(workflow.includes(value), message);
}

function position(value) {
  const index = workflow.indexOf(value);
  assert.notEqual(index, -1, `release workflow is missing: ${value}`);
  return index;
}

includes(
  'CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}',
  'Cloudflare token is not bound',
);
includes(
  'CLOUDFLARE_ACCOUNT_ID: ${{ secrets.CLOUDFLARE_ACCOUNT_ID }}',
  'Cloudflare account is not bound',
);
includes('/pages/projects/logcrate-updates', 'Pages project preflight is missing');
includes('releaseDraft: true', 'release assets must remain draft until the fallback transition');
includes('max-parallel: 1', 'cross-platform draft uploads must be serialized');
includes('gh release create "$RELEASE_TAG"', 'preflight must create one shared release draft');
includes(
  'releaseId: ${{ needs.preflight.outputs.release-id }}',
  'matrix jobs must upload to the preflight draft id',
);
assert.ok(
  !workflow.includes('releaseDraft: false'),
  'matrix jobs must not publish the Release directly',
);
includes("wranglerVersion: '4.116.0'", 'Wrangler version must be pinned');

const prepare = position('node scripts/update-mirror.mjs');
const fallbackDeploy = position('pages deploy .pages-fallback');
const fallbackVerify = position('--run-id "$GITHUB_RUN_ID-fallback"');
const publish = position('gh release edit "$RELEASE_TAG" --draft=false --latest');
const githubVerify = position('验证公开 GitHub updater 清单');
const fullDeploy = position('pages deploy .pages-full');
const fullVerify = position('--run-id "$GITHUB_RUN_ID-full"');
const recovery = workflow.lastIndexOf('pages deploy .pages-fallback');
const recoveryVerify = position('--run-id "$GITHUB_RUN_ID-recovery"');

assert.ok(
  prepare < fallbackDeploy &&
    fallbackDeploy < fallbackVerify &&
    fallbackVerify < publish &&
    publish < githubVerify &&
    githubVerify < fullDeploy &&
    fullDeploy < fullVerify &&
    fullVerify < recovery &&
    recovery < recoveryVerify,
  'release, fallback, publication, full deployment and recovery steps are out of order',
);
includes(
  "if: failure() && steps.prepare.outcome == 'success'",
  'failure recovery guard is missing',
);
includes(
  '--branch=${{ needs.preflight.outputs.pages-production-branch }}',
  'production branch is not derived from Pages',
);

console.log('Update mirror workflow check passed.');
