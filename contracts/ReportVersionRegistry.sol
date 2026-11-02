// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice A commitment records an issuer's digest; it does not certify source truth.
/// @dev No names, tax IDs, scores, audio, URLs or raw report bytes belong here.
contract ReportVersionRegistry {
    struct Record { bool exists; uint64 anchoredAt; bytes32 supersedes; }
    /// @notice Each issuer has a separate namespace, including for identical digests.
    mapping(address => mapping(bytes32 => Record)) public records;
    /// @notice The issuer's next version, or zero if not yet superseded.
    mapping(address => mapping(bytes32 => bytes32)) public nextVersion;
    event Anchored(bytes32 indexed digest, address indexed issuer, bytes32 indexed supersedes);

    /// @notice Record a digest and optionally replace a prior digest issued by the caller.
    /// @param digest Salted commitment to the off-chain report version.
    /// @param supersedes Prior digest for the same caller, or zero for the first version.
    function anchor(bytes32 digest, bytes32 supersedes) external {
        require(digest != bytes32(0), "empty commitment");
        require(!records[msg.sender][digest].exists, "already anchored");
        if (supersedes != bytes32(0)) {
            require(records[msg.sender][supersedes].exists, "unknown prior version");
            require(nextVersion[msg.sender][supersedes] == bytes32(0), "already superseded");
            nextVersion[msg.sender][supersedes] = digest;
        }
        records[msg.sender][digest] = Record(true, uint64(block.timestamp), supersedes);
        emit Anchored(digest, msg.sender, supersedes);
    }
}
