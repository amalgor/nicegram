// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

interface IIdentityRegistry {
    event Registered(uint256 indexed agentId, string agentURI, address indexed owner);

    function register() external returns (uint256 agentId);

    function ownerOf(uint256 tokenId) external view returns (address);
}
