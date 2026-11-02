// Local-only behavioral test. Dependencies are installed under target/contract-test.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const base = path.resolve(__dirname, '../target/contract-test/node_modules');
const solc = require(path.join(base, 'solc'));
const ganache = require(path.join(base, 'ganache'));
const { BrowserProvider, ContractFactory, ZeroHash } = require(path.join(base, 'ethers'));

async function main() {
  const source = fs.readFileSync(path.resolve(__dirname, '../contracts/ReportVersionRegistry.sol'), 'utf8');
  const input = { language: 'Solidity', sources: { 'ReportVersionRegistry.sol': { content: source } }, settings: { outputSelection: { '*': { '*': ['abi', 'evm.bytecode.object'] } } } };
  const output = JSON.parse(solc.compile(JSON.stringify(input)));
  const errors = (output.errors || []).filter(e => e.severity === 'error');
  assert.deepEqual(errors.map(e => e.formattedMessage), [], 'contract compilation failed');
  const artifact = output.contracts['ReportVersionRegistry.sol'].ReportVersionRegistry;
  const local = ganache.provider({ logging: { quiet: true }, chain: { chainId: 31337 } });
  try {
    const provider = new BrowserProvider(local);
    const a = await provider.getSigner(0), b = await provider.getSigner(1), c = await provider.getSigner(2);
    const contract = await new ContractFactory(artifact.abi, artifact.evm.bytecode.object, a).deploy();
    await contract.waitForDeployment();
    const first = '0x' + '11'.repeat(32), second = '0x' + '22'.repeat(32), third = '0x' + '33'.repeat(32);
    const anchor = async (signer, digest, previous = ZeroHash) => (await contract.connect(signer).anchor(digest, previous, { gasLimit: 300000 })).wait();
    const fails = async fn => { let failed = false; try { await fn(); } catch { failed = true; } assert.ok(failed, 'transaction should revert'); };
    await anchor(b, first); // An observer cannot occupy the issuer A's namespace.
    await anchor(a, first);
    await fails(() => anchor(a, first));
    await fails(() => anchor(c, second, first)); // C cannot claim A or B's version.
    await anchor(a, second, first);
    assert.equal(await contract.nextVersion(await a.getAddress(), first), second);
    assert.equal(await contract.nextVersion(await b.getAddress(), first), ZeroHash);
    await fails(() => anchor(a, third, first)); // One successor per issuer/version.
    await anchor(b, third, first);
    assert.equal((await contract.records(await a.getAddress(), second)).supersedes, first);
    await fails(() => anchor(a, ZeroHash));
    console.log('Registry local tests passed: namespace, duplicates, issuer, supersession and zero digest.');
  } finally { await local.disconnect(); }
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
