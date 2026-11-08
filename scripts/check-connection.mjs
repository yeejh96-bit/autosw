import assert from 'node:assert/strict';
import path from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { assertNode, packageRoot, workspaceRoot } from '../src/workspace.mjs';
assertNode();
if (process.argv.length !== 4 || process.argv[2] !== '--workspace') throw new Error('--workspace <개인 작업 폴더>를 지정하세요.');
const workspace = workspaceRoot(process.argv[3]);
const client = new Client({ name: 'autosw-local-connection-check', version: '0.1.0' });
const transport = new StdioClientTransport({ command: process.execPath, args: [path.join(packageRoot, 'bin/autosw.mjs'), 'mcp', '--workspace', workspace], stderr: 'pipe' });
try {
  await client.connect(transport);
  const tools = (await client.listTools()).tools;
  for (const name of ['autosw_guide', 'autosw_doctor', 'autosw_ledger_query', 'autosw_ledger_apply']) assert.ok(tools.some(t => t.name === name), '필수 도구 누락: ' + name);
  const guide = await client.callTool({ name: 'autosw_guide', arguments: {} });
  assert.ok(!guide.isError, '작성 지침 조회 실패');
  const doctor = await client.callTool({ name: 'autosw_doctor', arguments: {} });
  assert.ok(!doctor.isError, '환경 확인 도구 오류');
  const result = JSON.parse(doctor.content[0].text);
  console.log(`로컬 MCP 연결 성공: ${tools.length}개 도구·작성 지침 확인. 업무 자료는 읽거나 쓰지 않았습니다.`);
  for (const check of result.checks) console.log(`${check.ok ? '확인' : '준비 필요'}: ${check.name}`);
  console.log('Codex 앱의 프로젝트 열기·신뢰 확인·실제 도구 호출은 별도입니다.');
  if (result.status !== 'environment-ready') process.exitCode = 1;
} finally { await client.close(); }
