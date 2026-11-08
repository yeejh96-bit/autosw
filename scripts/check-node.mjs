import { assertNode } from '../src/workspace.mjs';
try { assertNode(); } catch (e) { console.error(e.message); process.exitCode = 1; }
