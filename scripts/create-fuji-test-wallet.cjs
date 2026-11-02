// Create a disposable Windows-only test wallet; never use it for mainnet assets.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const base = path.resolve(__dirname, '../target/contract-test');
const { Wallet } = require(path.join(base, 'node_modules/ethers'));
const keyFile = path.join(base, 'fuji-test-wallet.dpapi');
const addressFile = path.join(base, 'fuji-test-wallet-address.txt');
if (fs.existsSync(keyFile) || fs.existsSync(addressFile)) throw new Error('Test wallet exists; refusing to overwrite.');
const wallet = Wallet.createRandom();
const command = [
  'Add-Type -AssemblyName System.Security',
  '$p=[System.Text.Encoding]::UTF8.GetBytes([Console]::In.ReadToEnd())',
  '$e=[System.Security.Cryptography.ProtectedData]::Protect($p,$null,[System.Security.Cryptography.DataProtectionScope]::CurrentUser)',
  '$f=[System.IO.File]::Open($env:LANTERN_TEST_KEYFILE,[System.IO.FileMode]::CreateNew)',
  'try {$f.Write($e,0,$e.Length)} finally {$f.Dispose();[System.Array]::Clear($p,0,$p.Length)}'
].join(';');
const result = spawnSync('powershell.exe', ['-NoProfile','-NonInteractive','-Command',command], {
  input:wallet.privateKey, encoding:'utf8', windowsHide:true, env:{...process.env,LANTERN_TEST_KEYFILE:keyFile}
});
if (result.status !== 0 || !fs.existsSync(keyFile)) throw new Error('Could not protect test wallet with Windows DPAPI.');
fs.writeFileSync(addressFile, wallet.address + '\n', {flag:'wx'});
console.log('Disposable Fuji address: ' + wallet.address);
