// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

contract MockReputationRegistry {
    mapping(uint256 => uint256) private _scores;

    function setScore(uint256 agentId, uint256 score) external {
        _scores[agentId] = score;
    }

    function reputationOf(uint256 agentId) external view returns (uint256) {
        return _scores[agentId];
    }
}
