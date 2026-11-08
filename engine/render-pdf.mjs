// 두 PDF 진입점의 브라우저 선택·렌더·정리를 한 곳에서 관리한다.
import { existsSync, readdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { pathToFileURL } from 'node:url';

function findLocalChromeExecutable() {
  const baseDir = join(process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local'), 'ms-playwright');
  let entries;
  try { entries = readdirSync(baseDir); }
  catch { return null; }
  for (const entry of entries) {
    if (!entry.startsWith('chromium-')) continue;
    const candidate = join(baseDir, entry, 'chrome-win64', 'chrome.exe');
    try { if (statSync(candidate).isFile()) return candidate; }
    catch { /* 없으면 다음 후보를 확인한다. */ }
  }
  return null;
}

// 설치된 Edge → Chrome → 기존 Playwright 캐시만 사용하며 다운로드하지 않는다.
export async function launchBrowser(chromium, findExecutable = findLocalChromeExecutable) {
  for (const channel of ['msedge', 'chrome']) {
    try { return await chromium.launch({ channel }); }
    catch { /* 다음 설치본으로 진행한다. */ }
  }
  const executablePath = findExecutable();
  if (executablePath) {
    try { return await chromium.launch({ executablePath }); }
    catch { /* 모든 후보가 실패하면 설치 확인 안내를 반환한다. */ }
  }
  throw new Error(process.platform === 'win32'
    ? '브라우저를 찾지 못했습니다. Microsoft Edge 설치를 확인해 주세요.'
    : '브라우저를 찾지 못했습니다. 구글 크롬(google-chrome) 설치를 확인해 주세요.');
}

export async function renderPdf(browser, { htmlPath, pdfPath, pageOptions, pdfOptions }) {
  try {
    const page = await browser.newPage(pageOptions);
    await page.goto(pathToFileURL(htmlPath).href);
    await page.waitForLoadState('load');
    await page.evaluate(() => document.fonts.ready);
    await page.pdf({
      ...pdfOptions,
      path: pdfPath,
      printBackground: true,
      margin: { top: '0', right: '0', bottom: '0', left: '0' },
    });
  } finally {
    await browser.close();
  }
}

export async function runPdfCli({ usage, pageOptions, pdfOptions }) {
  const [htmlPath, pdfPath] = process.argv.slice(2).filter(arg => !arg.startsWith('--'));
  try {
    if (!htmlPath || !pdfPath) throw new Error(`사용법: ${usage}`);
    if (!existsSync(htmlPath)) throw new Error(`html 을 찾을 수 없습니다: ${htmlPath}`);
    let playwright;
    try { playwright = await import('playwright-core'); }
    catch { throw new Error('playwright-core 가 설치돼 있지 않습니다. 프로젝트 폴더에서 npm ci 를 실행해 주세요.'); }
    const browser = await launchBrowser(playwright.chromium);
    await renderPdf(browser, { htmlPath, pdfPath, pageOptions, pdfOptions });
    console.log(`PDF 저장 완료: ${pdfPath}`);
  } catch (error) {
    console.error(`렌더 중 오류가 발생했습니다: ${error.message}`);
    process.exitCode = 1;
  }
}

async function selftest() {
  const assert = (await import('node:assert/strict')).default;
  const attempts = [];
  const fakeBrowser = {};
  const chromium = { launch: async options => {
    attempts.push(options);
    if (options.executablePath) return fakeBrowser;
    throw new Error('검증용 실행 실패');
  } };
  assert.equal(await launchBrowser(chromium, () => 'cached-chrome'), fakeBrowser);
  assert.deepEqual(attempts, [{ channel: 'msedge' }, { channel: 'chrome' }, { executablePath: 'cached-chrome' }]);
  await assert.rejects(launchBrowser(chromium, () => null), /브라우저를 찾지 못했습니다/);
  await assert.rejects(launchBrowser({ launch: async () => { throw new Error('실패'); } }, () => 'cache'), /브라우저를 찾지 못했습니다/);
  for (const stage of ['newPage', 'goto', 'pdf', 'success']) {
    let closed = false;
    const failure = new Error(`검증용 ${stage} 실패`);
    const browser = {
      newPage: async () => {
        if (stage === 'newPage') throw failure;
        return {
          goto: async () => { if (stage === 'goto') throw failure; },
          waitForLoadState: async () => {}, evaluate: async () => {},
          pdf: async () => { if (stage === 'pdf') throw failure; },
        };
      },
      close: async () => { closed = true; },
    };
    const rendered = renderPdf(browser, { htmlPath: resolve('test # 한글.html'), pdfPath: 'unused.pdf' });
    if (stage === 'success') await rendered;
    else await assert.rejects(rendered, error => error === failure);
    assert.equal(closed, true, `${stage}: 브라우저 정리`);
  }
  console.log('SELFTEST_OK (PDF 브라우저 대체 실행·실패 안내·자원 정리)');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href
    && process.argv.includes('--selftest')) {
  await selftest();
}
