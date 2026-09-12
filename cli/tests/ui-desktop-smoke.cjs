// Run only against an isolated store: this test imports and removes its own fixture file.
// PLAYWRIGHT_MODULE may point to an installed Playwright package; OOS_UI_TEST_URL is required.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

async function run() {
  assert.equal(process.env.OOS_UI_TEST_MUTATIONS, '1', 'Set OOS_UI_TEST_MUTATIONS=1 for an isolated test store');
  const base = process.env.OOS_UI_TEST_URL;
  assert.ok(base && new URL(base).hostname === '127.0.0.1', 'A loopback test URL is required');
  const browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_BROWSER });
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 }, reducedMotion: 'reduce' });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const name = `ui-check-${Date.now()}-O'Brien.txt`;
  const snapshot = `ui-check-${Date.now()}`;
  const output = path.resolve('target/ui-validation');
  await fs.mkdir(output, { recursive: true });
  try {
    await page.goto(base, { waitUntil: 'networkidle' });
    await page.waitForFunction(() => document.getElementById('engine-version').textContent.includes('v'));
    assert.equal(await page.locator('#view-files').isVisible(), true);
    assert.equal(await page.locator('[id]').evaluateAll(elements => new Set(elements.map(el => el.id)).size === elements.length), true);

    for (const width of [1440, 1024, 390]) {
      await page.setViewportSize({ width, height: 900 });
      for (const tab of ['files', 'overview', 'snapshots', 'upload', 'watcher', 'maintenance']) {
        await page.evaluate(tab => switchTab(tab), tab);
        assert.equal(await page.locator('#view-' + tab).isVisible(), true);
        const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
        assert.ok(scrollWidth <= width, `${tab} overflows at ${width}px: ${scrollWidth}`);
      }
    }

    await page.locator('#mobile-menu-btn').click();
    await page.locator('#tab-files').click();
    assert.equal(await page.locator('body').evaluate(el => el.classList.contains('nav-open')), false);
    await page.locator('#lang-toggle-btn').click();
    assert.equal(await page.locator('html').getAttribute('lang'), 'vi');
    await page.locator('#file-search-input').fill('no-such-file-12345');
    assert.equal(await page.locator('.empty-state').count(), 1);
    await page.screenshot({ path: path.join(output, 'mobile-empty-vi.png'), fullPage: true });
    await page.locator('#file-search-input').fill('');
    await page.locator('#lang-toggle-btn').click();
    await page.setViewportSize({ width: 1440, height: 960 });

    for (const content of ['first version\n', 'second version\nnew line\n']) {
      await page.locator('#tab-upload').click();
      await page.locator('#file-input').setInputFiles({ name, mimeType: 'text/plain', buffer: Buffer.from(content) });
      await page.locator('#upload-btn').click();
      await page.waitForFunction(() => !document.getElementById('upload-btn').disabled);
      const files = await page.request.get(base + '/api/files').then(response => response.json());
      assert.ok(files.some(file => file.name === name), 'uploaded file is listed');
    }

    await page.locator('#tab-files').click();
    await page.locator('#file-search-input').fill(name);
    assert.equal(await page.locator('#files-table-body tr[data-file]').count(), 1);
    await page.locator('#files-table-body tr[data-file]').click();
    await page.locator('#inspector-versions .history-item').first().waitFor();
    assert.equal(await page.locator('#inspector-versions .history-item').count(), 2);
    await page.locator('#file-inspector [data-file-action="preview"]').click();
    await page.waitForFunction(() => document.getElementById('preview-content-area').textContent.includes('second version'));
    await page.keyboard.press('Escape');

    await page.locator('#file-inspector [data-file-action="versions"]').click();
    await page.waitForFunction(() => document.querySelectorAll('#versions-modal-body tr').length === 2);
    await page.locator('#diff-mode-btn-diff').click();
    await page.waitForFunction(() => document.getElementById('diff-content-area').textContent.includes('first version'));
    await page.screenshot({ path: path.join(output, 'version-comparison.png') });
    await page.keyboard.press('Escape');

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#files-table-body .row-actions a').click();
    const download = await downloadPromise;
    assert.equal(await fs.readFile(await download.path(), 'utf8'), 'second version\nnew line\n');

    await page.locator('#btn-view-grid').click();
    assert.equal(await page.locator('#files-grid-container article').count(), 1);
    await page.locator('#btn-view-tree').click();
    assert.ok((await page.locator('#files-tree-body').textContent()).includes(name));
    await page.locator('#btn-view-list').click();

    await page.locator('#tab-snapshots').click();
    await page.locator('#snapshot-label-input').fill(snapshot);
    await page.locator('button[onclick="createSnapshot()"]').click();
    await page.locator('#snapshots-table-body tr').filter({ hasText: snapshot }).waitFor();
    await page.locator('#snapshots-table-body tr').filter({ hasText: snapshot }).locator('button').last().click();
    await page.locator('#confirm-execute-btn').click();
    await page.locator('#snapshots-table-body tr').filter({ hasText: snapshot }).waitFor({ state: 'detached' });

    await page.locator('#tab-maintenance').click();
    await page.locator('#fsck-btn').click();
    await page.waitForFunction(() => !document.getElementById('fsck-btn').disabled);
    assert.ok((await page.locator('#maintenance-result').textContent()).includes('Healthy'));

    await page.locator('#tab-files').click();
    await page.locator('#files-table-body [data-file-action="delete"]').click();
    await page.locator('#confirm-execute-btn').click();
    await page.waitForFunction(name => !cachedFilesList.some(file => file.name === name), name);
    await page.locator('#file-search-input').fill('');
    await page.locator('#file-sort').selectOption('size');
    const sizes = await page.locator('#files-table-body tr[data-file]').evaluateAll(rows => rows.map(row => cachedFilesList.find(file => file.name === row.dataset.file).size_bytes));
    assert.deepEqual(sizes, [...sizes].sort((a, b) => b - a));
    await page.locator('#file-sort').selectOption('name');

    const imageRow = page.locator('#files-table-body tr[data-file]').filter({ hasText: '.png' }).first();
    if (await imageRow.count()) {
      await imageRow.click();
      await page.waitForFunction(() => document.querySelector('.inspector-preview img')?.naturalWidth > 0);
      await page.screenshot({ path: path.join(output, 'image-inspector.png') });
      await page.locator('#file-inspector button[onclick="closeFileDetails()"]').click();
    }
    await page.screenshot({ path: path.join(output, 'desktop.png') });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(output, 'mobile.png'), fullPage: true });
    assert.deepEqual(errors, []);
    console.log('PASS: 18 responsive view checks, bilingual navigation, search, sorting, list/tree/grid, upload, versions, preview, download, snapshots, FSCK, deletion, and image rendering.');
  } finally {
    await browser.close();
  }
}
run().catch(error => { console.error(error); process.exitCode = 1; });
