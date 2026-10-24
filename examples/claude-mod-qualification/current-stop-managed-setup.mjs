// Root step ONLY inside the disposable, marked qualification container.
import fs from 'node:fs';
import assert from 'node:assert/strict';
assert.equal(process.getuid(),0);
assert(fs.existsSync('/home/node/workspace/.aikit-qualification-only'));
const evidence='/home/node/evidence';
const test=JSON.parse(fs.readFileSync(evidence+'/case.json'));
assert(['baseline','missing','timeout'].includes(test.mode));
assert.equal(test.version,'2.1.308');assert.equal(test.production_activated,false);
const file='/etc/claude-code/managed-settings.json';
assert(!fs.existsSync(file),'never replace existing managed policy');
fs.mkdirSync('/etc/claude-code',{recursive:true});
fs.writeFileSync(file,fs.readFileSync(evidence+'/expected-settings.json'),{flag:'wx',mode:0o644});
fs.chownSync(file,0,0);fs.chmodSync(file,0o644);
const stat=fs.statSync(file);assert.equal(stat.uid,0);assert.equal(stat.mode&0o777,0o644);
console.log('disposable managed configuration verified: root/0644');
