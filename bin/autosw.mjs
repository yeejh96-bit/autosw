#!/usr/bin/env node
import { assertNode, initialize } from '../src/workspace.mjs';
import { doctor } from '../src/doctor.mjs';
import { guide } from '../src/guides.mjs';
import { setup } from '../src/setup.mjs';
import { npmSetup, setupWizard } from '../src/setup-npm.mjs';
import * as service from '../src/service.mjs';

const commands = {
  init: ['workspace'], doctor: ['workspace'], guide: ['topic'], setup: ['workspace', 'apply', 'npm'],
  list: ['workspace'], read: ['workspace', 'input'], 'write-draft': ['workspace', 'output', 'content'],
  inspect: ['workspace', 'input'], preview: ['workspace', 'kind', 'input', 'output', 'request'],
  apply: ['workspace', 'plan', 'confirm'], verify: ['workspace', 'plan'],
  render: ['workspace', 'input', 'output', 'engine'], 'check-hangul': ['workspace', 'input', 'output', 'pdf', 'plan', 'format', 'verification'],
  'ledger-preview': ['workspace', 'input'], 'ledger-apply': ['workspace', 'plan', 'confirm', 'approved'],
  'ledger-query': ['workspace', 'program', 'from', 'to', 'kind'], 'ledger-report': ['workspace', 'program', 'from', 'to', 'kind'],
  publish: ['workspace', 'report', 'confirm', 'output', 'expected', 'reviewed'], mcp: ['workspace']
};
async function main(args) {
  assertNode();
  const command = args.shift();
  if (!command || command === '--help') {
    console.log('autosw: ' + Object.keys(commands).join(' | ') + '\n작업 명령은 --workspace <개인폴더>를 사용합니다. 자세한 절차: guide 또는 README.md');
    return;
  }
  if (!commands[command]) throw new Error('알 수 없는 명령입니다. --help를 확인하세요.');
  const opts = {};
  for (let i = 0; i < args.length; i++) {
    const key = args[i].slice(2);
    if (!args[i].startsWith('--') || !commands[command].includes(key) || Object.hasOwn(opts, key)) throw new Error('잘못되거나 중복된 인자입니다.');
    if (['apply', 'reviewed', 'approved', 'npm'].includes(key)) opts[key] = true;
    else {
      if (args[i + 1] === undefined || args[i + 1].startsWith('--')) throw new Error(`--${key} 값이 필요합니다.`);
      opts[key] = args[++i];
    }
  }
  let result;
  switch (command) {
    case 'init': result = initialize(opts.workspace); break;
    case 'doctor': result = doctor(opts.workspace); break;
    case 'guide': result = guide(opts.topic); break;
    case 'setup': result = opts.npm ? npmSetup(opts.workspace, opts.apply) : !opts.workspace && !opts.apply ? await setupWizard() : setup(opts.workspace, opts.apply); break;
    case 'list': result = service.listFiles(opts.workspace); break;
    case 'read': result = service.readDraft(opts.workspace, opts.input); break;
    case 'write-draft': result = service.writeDraft(opts.workspace, opts.output, opts.content); break;
    case 'inspect': result = service.inspect(opts.workspace, opts.input); break;
    case 'preview': result = await service.preview(opts.workspace, opts); break;
    case 'apply': result = service.apply(opts.workspace, opts.plan, opts.confirm); break;
    case 'verify': result = service.verify(opts.workspace, opts.plan); break;
    case 'render': result = await service.render(opts.workspace, opts); break;
    case 'check-hangul': result = service.checkHangul(opts.workspace, { ...opts, planId: opts.plan }); break;
    case 'publish': result = service.publish(opts.workspace, { reportId: opts.report, reportHash: opts.confirm, output: opts.output, expectedHash: opts.expected, visualReviewed: opts.reviewed }); break;
    case 'ledger-preview': result = service.ledgerPreview(opts.workspace, JSON.parse(service.readDraft(opts.workspace, opts.input).content)); break;
    case 'ledger-apply': result = service.ledgerApply(opts.workspace, { planId: opts.plan, confirmHash: opts.confirm, approved: opts.approved }); break;
    case 'ledger-query':
    case 'ledger-report': {
      const { workspace, ...filter } = opts;
      result = command === 'ledger-query' ? service.ledgerQuery(workspace, filter) : service.ledgerReport(workspace, filter); break;
    }
    case 'mcp': {
      const { startServer } = await import('../src/mcp.mjs');
      await startServer(opts.workspace); return;
    }
  }
  console.log(JSON.stringify(result, null, 2));
}
try { await main(process.argv.slice(2)); }
catch (e) { console.error(`실패: ${e.message}`); process.exitCode = 1; }
