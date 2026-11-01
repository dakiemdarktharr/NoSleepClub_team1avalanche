// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// A commitment proves a version existed; it does not certify source truth.
/// No names, tax IDs, scores, audio, URLs or raw report bytes belong here.
contract ReportVersionRegistry {
    struct Record { address issuer; uint64 anchoredAt; bytes32 supersedes; }
    mapping(bytes32 => Record) public records;
    mapping(bytes32 => bytes32) public nextVersion;
    event Anchored(bytes32 indexed digest, address indexed issuer, bytes32 indexed supersedes);

    function anchor(bytes32 digest, bytes32 supersedes) external {
        require(digest != bytes32(0), "empty commitment");
        require(records[digest].issuer == address(0), "already anchored");
        if (supersedes != bytes32(0)) {
            require(records[supersedes].issuer == msg.sender, "issuer mismatch");
            require(nextVersion[supersedes] == bytes32(0), "already superseded");
            nextVersion[supersedes] = digest;
        }
        records[digest] = Record(msg.sender, uint64(block.timestamp), supersedes);
        emit Anchored(digest, msg.sender, supersedes);
    }
}
