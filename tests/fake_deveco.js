import {spawn, spawnSync, execFile, execFileSync, exec, execSync} from 'node:child_process';
import fs from 'node:fs';
import {promisify} from 'node:util';
import assert from 'node:assert/strict';
const mode = fs.readFileSync('mode', 'utf8');
if (process.argv.includes('auth')) {
  if (mode === 'login') {
    await promisify(exec)('open "https://cn.devecostudio.huawei.com/console/DevEcoIDE/apply?port=1&appid=1009&code=test"');
    console.log('LOGIN_CALLBACK_STILL_ALIVE');
  } else if (mode === 'auth-hdc') {
    spawnSync('/sdk/hdc', ['list', 'targets']);
  } else console.log('Not logged in');
} else {
  const hdc = process.platform === 'win32' ? 'C:\\SDK with spaces\\hdc.exe' : '/sdk with spaces/hdc';
  const targets = spawnSync(hdc, ['list', 'targets'], {encoding: 'utf8'});
  assert.equal(targets.status, 0, targets.stderr);
  assert.equal(targets.stdout, 'A\n');
  fs.writeFileSync('bridge-started', 'yes');
  if (mode === 'timeout') await new Promise(resolve => setTimeout(resolve, 60000));
  else if (mode === 'wrong-target') spawnSync(hdc, ['-t', 'B', 'shell', 'bm', 'get', '-u']);
  else if (mode === 'maintenance') spawnSync(hdc, ['kill', '-r']);
  else if (mode === 'unknown') spawnSync(hdc, ['-t', 'A', 'shell', 'reboot']);
  else if (mode === 'shell-hdc') { try { execSync('hdc list targets'); } catch {} }
  else if (mode === 'query-fails' || mode === 'invalid-udid') spawnSync(hdc, ['-t', 'A', 'shell', 'bm', 'get', '-u']);
  else {
    const args = ['-t', 'A', 'shell', 'bm', 'get', '-u'];
    const one = execFileSync(hdc, args, {encoding: 'utf8'});
    assert.match(one, /AAAA/);
    const two = await promisify(execFile)(hdc, args);
    assert.equal(two.stdout, one);
    await new Promise((resolve, reject) => {
      const child = spawn(hdc, ['-c', '-t', 'A', 'shell', 'getprop', 'hw_sc.build.os.deviceType']);
      let out = ''; child.stdout.on('data', data => { out += data; });
      child.on('error', reject);
      child.on('exit', code => { try { assert.equal(code, 0); assert.match(out, /phone/); resolve(); } catch (e) { reject(e); } });
    });
  }
  // Mimics the official CLI swallowing a device-query error.
  console.log('Signature generation completed successfully.');
}
