import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { assertNode, defaultWorkspace } from './workspace.mjs';
const require = createRequire(import.meta.url);

export function doctor(requested) {
  const checks = [];
  try { assertNode(); checks.push({ name: 'Node', ok: true, version: process.versions.node }); }
  catch (e) { checks.push({ name: 'Node', ok: false, detail: e.message }); }
  checks.push({ name: 'Windows', ok: process.platform === 'win32' });
  for (const pkg of ['@modelcontextprotocol/sdk', '@xmldom/xmldom', 'jszip', 'playwright-core', 'zod']) {
    try { require.resolve(pkg === '@modelcontextprotocol/sdk' ? pkg + '/server/mcp.js' : pkg); checks.push({ name: pkg, ok: true }); }
    catch { checks.push({ name: pkg, ok: false, detail: 'npm ci가 필요합니다.' }); }
  }
  if (process.platform === 'win32') {
    const probe = spawnSync('powershell.exe', ['-NoProfile', '-Command', "if ([Type]::GetTypeFromProgID('HWPFrame.HwpObject')) { Write-Output 'registered' } else { exit 1 }"], { encoding: 'utf8', windowsHide: true, timeout: 15000 });
    checks.push({ name: '한글 COM 등록', ok: probe.status === 0, detail: '설치 등록 확인이며 실제 파일 열기·출력 성공을 뜻하지 않습니다.' });
  }
  const browsers = ['C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', 'C:/Program Files/Microsoft/Edge/Application/msedge.exe'];
  checks.push({ name: '설치 브라우저', ok: browsers.some(p => fs.existsSync(p)), detail: '자체 미리보기는 Chrome/Edge 또는 기존 Playwright 브라우저를 사용합니다.' });
  const workspace = path.resolve(requested || process.env.AUTOSW_WORKSPACE || defaultWorkspace());
  return { status: checks.every(c => c.ok) ? 'environment-ready' : 'needs-attention', checks, workspace, workspaceInitialized: fs.existsSync(path.join(workspace, 'autosw.config.json')), note: 'AI 앱 연결과 한글 실제 출력은 별도로 시험해야 합니다.' };
}
