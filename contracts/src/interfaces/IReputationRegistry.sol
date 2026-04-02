// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

interface IReputationRegistry {
    function reputationOf(uint256 agentId) external view returns (uint256);
}
