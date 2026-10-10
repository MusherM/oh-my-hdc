// Loaded only in an omh-owned DevEco CLI process. Never edits the SDK or CLI.
const cp = require('node:child_process');
const path = require('node:path');
const { syncBuiltinESMExports } = require('node:module');
const { promisify } = require('node:util');
const original = Object.fromEntries(['spawn', 'spawnSync', 'execFile', 'execFileSync', 'exec', 'execSync'].map(k => [k, cp[k]]));
let bridgeFailed = false;
function fail(message) {
  bridgeFailed = true;
  throw new Error(`omh DevEco: ${message}`);
}
function isHdc(file) {
  return /^(hdc|hdc\.exe)$/i.test(path.win32.basename(String(file).replaceAll('/', '\\')));
}
function rewrite(file, args, options) {
  if (!isHdc(file)) return [file, args, options];
  if (!Array.isArray(args)) fail('unexpected HDC invocation');
  if (!process.env.OMH_LEASE || !process.env.OMH_BIN) fail('HDC requires an active omh signing lease');
  if (options?.shell || options?.detached) fail('shell/detached HDC is unsupported');
  return [process.env.OMH_BIN, ['__deveco-hdc', '--', ...(args || [])], options];
}
function checkOutput(args, stdout) {
  if (args?.slice(-4).join(' ') === 'shell bm get -u' && !/[A-Fa-f0-9]{64}/.test(String(stdout ?? ''))) {
    bridgeFailed = true;
  }
}
function observe(child, args) {
  let stdout = '';
  child.stdout?.on('data', data => { stdout += data; });
  child.on('error', () => { bridgeFailed = true; });
  child.on('exit', (code, signal) => { if (code !== 0 || signal) bridgeFailed = true; });
  child.on('close', () => checkOutput(args, stdout));
  return child;
}
cp.spawn = function(file, args, options) {
  const hdc = isHdc(file);
  const child = original.spawn(...rewrite(file, args, options));
  return hdc ? observe(child, args) : child;
};
cp.spawnSync = function(file, args, options) {
  const result = original.spawnSync(...rewrite(file, args, options));
  if (isHdc(file)) {
    if (result.status !== 0 || result.error) bridgeFailed = true;
    checkOutput(args, result.stdout);
  }
  return result;
};
cp.execFile = function(file, args, options, callback) {
  if (!isHdc(file)) return original.execFile(file, args, options, callback);
  if (!Array.isArray(args)) fail('unexpected HDC invocation');
  if (typeof options === 'function') { callback = options; options = undefined; }
  return observe(original.execFile(...rewrite(file, args, options), callback), args);
};
cp.execFileSync = function(file, args, options) {
  try {
    const stdout = original.execFileSync(...rewrite(file, args, options));
    if (isHdc(file)) checkOutput(args, stdout);
    return stdout;
  }
  catch (error) { if (isHdc(file)) bridgeFailed = true; throw error; }
};
function loginUrl(command) {
  const match = /^(?:open|xdg-open|start "") "(https:\/\/[^"\r\n]+)"$/.exec(command);
  if (!match) return null;
  const url = new URL(match[1]);
  if (!url.hostname.endsWith('.huawei.com') && !url.hostname.endsWith('.hicloud.com')) fail('unexpected login URL host');
  return url.href;
}
cp.exec = function(command, options, callback) {
  if (typeof options === 'function') { callback = options; options = undefined; }
  // The supported CLI uses argv-based HDC calls. Never permit a new shell path.
  if (/(?:^|[\s/\\"'])hdc(?:\.exe)?(?:[\s"']|$)/i.test(command)) fail('shell HDC is unsupported');
  const url = process.env.OMH_DEVECO_BROWSER === 'codex' ? loginUrl(command) : null;
  if (url) {
    // The agent opens this event with open_in_codex. Keep the callback server alive.
    console.log(JSON.stringify({event: 'omh.deveco.login', browser: 'codex', url}));
    return original.execFile(process.execPath, ['-e', ''], options, callback);
  }
  return original.exec(command, options, callback);
};
for (const name of ['exec', 'execFile']) {
  cp[name][promisify.custom] = (...args) => {
    let child;
    const promise = new Promise((resolve, reject) => {
      child = cp[name](...args, (error, stdout, stderr) => {
        if (error) { error.stdout = stdout; error.stderr = stderr; reject(error); }
        else resolve({stdout, stderr});
      });
    });
    promise.child = child;
    return promise;
  };
}
cp.execSync = function(command, options) {
  if (/(?:^|[\s/\\"'])hdc(?:\.exe)?(?:[\s"']|$)/i.test(command)) fail('shell HDC is unsupported');
  return original.execSync(command, options);
};
// auth login waits for an Enter before starting OAuth; let the wrapper start it.
if (process.argv.slice(-2).join(' ') === 'auth login') {
  const readline = require('node:readline');
  const question = readline.Interface.prototype.question;
  readline.Interface.prototype.question = function(query, ...args) {
    if (query === '') { queueMicrotask(() => args.at(-1)('')); return; }
    return question.call(this, query, ...args);
  };
}
process.on('exit', () => {
  // Official signing catches some device-query errors. They must not become success.
  if (bridgeFailed) {
    process.stderr.write('omh DevEco: a device query failed; signing is not verified\n');
    process.exitCode = 125;
  }
});
syncBuiltinESMExports();
