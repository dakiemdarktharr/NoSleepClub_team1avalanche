// Fuji-only deployment. Windows DPAPI keeps the disposable test key out of arguments and logs.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const base = path.resolve(__dirname, '../target/contract-test');
const solc = require(path.join(base, 'node_modules/solc'));
const { Wallet, parseEther, formatEther, keccak256 } = require(path.join(base, 'node_modules/ethers'));
const expectedAddress = fs.readFileSync(path.join(base, 'fuji-test-wallet-address.txt'), 'utf8').trim();
const keyFile = path.join(base, 'fuji-test-wallet.dpapi');
const args = process.argv.slice(2);
const broadcast = args.includes('--broadcast');
const capIndex = args.indexOf('--max-gas-avax');
if (capIndex < 0 || !args[capIndex + 1]) throw new Error('Specify --max-gas-avax, at most 0.05 test AVAX.');
const cap = parseEther(args[capIndex + 1]);
if (cap <= 0n || cap > parseEther('0.05')) throw new Error('Gas cap must be > 0 and <= 0.05 test AVAX.');
const rpcUrl = 'https://api.avax-test.network/ext/bc/C/rpc';
let id = 0;
function rpc(method, params) {
  const command = "$ErrorActionPreference='Stop'; $payload=[Console]::In.ReadToEnd(); $r=Invoke-RestMethod -Uri 'https://api.avax-test.network/ext/bc/C/rpc' -Method Post -ContentType 'application/json' -Body $payload -TimeoutSec 25; ConvertTo-Json -InputObject $r -Depth 100 -Compress";
  const p = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command], { input: JSON.stringify({ jsonrpc:'2.0', id:++id, method, params }), encoding:'utf8', timeout:30000, windowsHide:true, maxBuffer:2*1024*1024 });
  if (p.status !== 0) throw new Error('Fuji RPC unavailable; do not assume a broadcast failed.');
  let result; try { result = JSON.parse(p.stdout); } catch { throw new Error('Invalid Fuji RPC response.'); }
  if (result.id !== id || result.error || !('result' in result)) throw new Error('Fuji RPC returned an error.');
  return result.result;
}
function loadWallet() {
  const command = "Add-Type -AssemblyName System.Security; $p=[System.Security.Cryptography.ProtectedData]::Unprotect([System.IO.File]::ReadAllBytes($env:LANTERN_TEST_KEYFILE),$null,[System.Security.Cryptography.DataProtectionScope]::CurrentUser); [Console]::Out.Write([System.Text.Encoding]::UTF8.GetString($p)); [System.Array]::Clear($p,0,$p.Length)";
  const p = spawnSync('powershell.exe', ['-NoProfile','-NonInteractive','-Command',command], { encoding:'utf8', windowsHide:true, env:{...process.env, LANTERN_TEST_KEYFILE:keyFile} });
  if (p.status !== 0 || !/^0x[0-9a-fA-F]{64}$/.test(p.stdout)) throw new Error('Cannot unlock disposable test wallet for current Windows user.');
  const wallet = new Wallet(p.stdout);
  if (wallet.address.toLowerCase() !== expectedAddress.toLowerCase()) throw new Error('Test wallet address mismatch.');
  return wallet;
}
async function main() {
  const source = fs.readFileSync(path.resolve(__dirname, '../contracts/ReportVersionRegistry.sol'), 'utf8');
  const input = { language:'Solidity', sources:{'ReportVersionRegistry.sol':{content:source}}, settings:{ outputSelection:{'*':{'*':['evm.bytecode.object','evm.deployedBytecode.object']}} } };
  const compiled = JSON.parse(solc.compile(JSON.stringify(input)));
  if ((compiled.errors || []).some(e => e.severity === 'error')) throw new Error('Solidity compile failed.');
  const artifact = compiled.contracts['ReportVersionRegistry.sol'].ReportVersionRegistry;
  if (rpc('eth_chainId', []) !== '0xa869') throw new Error('Wrong chain; Fuji only.');
  const balance = BigInt(rpc('eth_getBalance', [expectedAddress, 'latest']));
  console.log(`Fuji chain 43113; disposable address ${expectedAddress}; balance ${formatEther(balance)} test AVAX.`);
  if (balance === 0n) throw new Error('No test AVAX; fund the disposable address first.');
  const data = '0x' + artifact.evm.bytecode.object;
  const gasPrice = BigInt(rpc('eth_gasPrice', []));
  const estimatedGas = BigInt(rpc('eth_estimateGas', [{from:expectedAddress, data, value:'0x0'}]));
  const gasLimit = (estimatedGas * 120n + 99n) / 100n;
  const maximumFee = gasLimit * gasPrice;
  if (maximumFee > cap || maximumFee > balance) throw new Error('Estimated maximum fee exceeds cap or balance.');
  console.log(`Gas limit ${gasLimit}; maximum fee ${formatEther(maximumFee)} test AVAX; cap ${formatEther(cap)} test AVAX.`);
  if (!broadcast) { console.log('Dry run only; no transaction signed or sent.'); return; }
  if (rpc('eth_chainId', []) !== '0xa869') throw new Error('Wrong chain before signing.');
  const nonce = Number(BigInt(rpc('eth_getTransactionCount', [expectedAddress, 'pending'])));
  if (!Number.isSafeInteger(nonce)) throw new Error('Invalid nonce.');
  const raw = await loadWallet().signTransaction({chainId:43113, type:0, nonce, gasLimit, gasPrice, data, value:0});
  const hash = keccak256(raw);
  console.log(`Signed Fuji transaction hash: ${hash}. If submission is uncertain, inspect this hash before any retry.`);
  const submitted = rpc('eth_sendRawTransaction', [raw]);
  if (typeof submitted !== 'string' || submitted.toLowerCase() !== hash.toLowerCase()) throw new Error('Unexpected RPC transaction hash; inspect before retry.');
  for (let i=0; i<20; i++) {
    const receipt = rpc('eth_getTransactionReceipt', [hash]);
    if (receipt) {
      if (receipt.status !== '0x1' || !receipt.contractAddress) throw new Error('Deployment transaction failed; inspect receipt.');
      const code = rpc('eth_getCode', [receipt.contractAddress, 'latest']);
      if (code.toLowerCase() !== ('0x' + artifact.evm.deployedBytecode.object).toLowerCase()) throw new Error('Deployed code differs from local artifact; do not configure registry.');
      const block = rpc('eth_getBlockByNumber', [receipt.blockNumber, false]);
      if (!block || String(block.hash).toLowerCase() !== String(receipt.blockHash).toLowerCase()) throw new Error('Receipt block does not match current RPC canonical block.');
      console.log(`Registry ${receipt.contractAddress}; block ${receipt.blockNumber}; bytecode matched. Receipt observed, not a finality proof.`);
      console.log(`Explorer: https://explorer-test.avax.network/c-chain/tx/${hash}`);
      return;
    }
    await new Promise(resolve => setTimeout(resolve, 1500));
  }
  console.log('Receipt not yet available. Check the printed hash; do not submit again automatically.');
}
main().catch(e => { console.error(e.message); process.exitCode = 1; });
