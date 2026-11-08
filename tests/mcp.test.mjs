import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { initialize, packageRoot } from '../src/workspace.mjs';
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'autosw-mcp-'));
initialize(root);
const transport = new StdioClientTransport({ command: process.execPath, args: [path.join(packageRoot, 'bin', 'autosw.mjs'), 'mcp', '--workspace', root], stderr: 'pipe' });
const client = new Client({ name: 'autosw-integration-test', version: '0.1.0' });
let stderr = ''; transport.stderr?.on('data', chunk => { stderr += chunk; });
function cleanup(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) { cleanup(file); fs.rmdirSync(file); } else fs.unlinkSync(file);
  }
}
try {
  await client.connect(transport);
  assert.match(client.getInstructions(), /사용자 승인/);
  const tools = (await client.listTools()).tools;
  assert.ok(tools.some(t => t.name === 'autosw_publish' && t.annotations.destructiveHint));
  assert.ok(tools.some(t => t.name === 'autosw_inspect' && t.annotations.readOnlyHint));
  const guide = await client.callTool({ name: 'autosw_guide', arguments: {} });
  assert.ok(JSON.parse(guide.content[0].text).content.includes('autosw'));
  const written = await client.callTool({ name: 'autosw_write_draft', arguments: { output: 'drafts/테스트.md', content: '# MCP 시험\n\n합성 내용' } });
  assert.ok(!written.isError);
  const listed = await client.callTool({ name: 'autosw_list_files', arguments: {} });
  assert.equal(JSON.parse(listed.content[0].text).files[0].path, 'drafts/테스트.md');
  const escaped = await client.callTool({ name: 'autosw_read_text', arguments: { input: '../secret.md' } });
  assert.equal(escaped.isError, true);
  const preview = await client.callTool({ name: 'autosw_preview', arguments: { kind: 'create', input: 'drafts/테스트.md', output: 'outputs/테스트.hwpx' } });
  assert.ok(!preview.isError, preview.content[0].text);
  const plan = JSON.parse(preview.content[0].text);
  const generated = await client.callTool({ name: 'autosw_apply', arguments: { planId: plan.planId, confirmHash: plan.confirmHash } });
  assert.ok(!generated.isError, generated.content[0].text);
  const call = async (name, args = {}) => {
    const result = await client.callTool({ name: 'autosw_' + name, arguments: args });
    assert.ok(!result.isError, result.content[0].text); return JSON.parse(result.content[0].text);
  };
  const change = { operation: 'add', kind: 'attendance', data: { program: 'MCP 합성', date: '2026-11-09', session: '1', count: 2, participantIds: ['p1', 'p2'] } };
  const lp = await call('ledger_preview', { change });
  assert.equal((await call('ledger_query')).totals.sessions, 0);
  const denied = await client.callTool({ name: 'autosw_ledger_apply', arguments: { planId: lp.planId, confirmHash: lp.confirmHash, approved: false } });
  assert.equal(denied.isError, true);
  const args = { planId: lp.planId, confirmHash: lp.confirmHash, approved: true };
  assert.equal((await call('ledger_apply', args)).status, 'ledger-applied');
  assert.equal((await call('ledger_apply', args)).status, 'already-applied');
  assert.equal((await call('ledger_report')).totals.uniqueParticipants, 2);
  const lg = await call('guide', { topic: 'references/ledger.md' });
  assert.match(lg.content, /미리보기/);
  assert.equal(stderr, '');
  console.log(`MCP: 실제 stdio 연결·${tools.length}개 도구·지침·원고·한글 생성 통과`);
} finally { await client.close(); cleanup(root); fs.rmdirSync(root); }
