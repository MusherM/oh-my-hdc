// Test dispatch without opening a real browser in CI.
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const {promisify} = require('node:util');
const nativeExecFile = cp.execFile;
let nativeCalls = 0;
cp.exec = (command, options, callback) => {
  nativeCalls++;
  assert.match(command, /https:\/\/cn\.devecostudio\.huawei\.com/);
  if (typeof options === 'function') callback = options;
  return nativeExecFile(process.execPath, ['-e', ''], callback);
};
require('../src/deveco.cjs');
(async () => {
  const command = 'open "https://cn.devecostudio.huawei.com/console/DevEcoIDE/apply?port=1&code=test"';
  process.env.OMH_DEVECO_BROWSER = 'default';
  await promisify(cp.exec)(command);
  assert.equal(nativeCalls, 1);
  process.env.OMH_DEVECO_BROWSER = 'codex';
  await promisify(cp.exec)(command);
  assert.equal(nativeCalls, 1);
  console.log('BROWSER_ROUTING_OK');
})().catch(error => { console.error(error); process.exitCode = 1; });
