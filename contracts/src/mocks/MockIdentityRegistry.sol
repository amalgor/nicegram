// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

contract MockIdentityRegistry {
    event Registered(uint256 indexed agentId, string agentURI, address indexed owner);

    uint256 private _nextAgentId = 1;
    mapping(uint256 => address) private _owners;

    function setOwner(uint256 tokenId, address owner) external {
        _owners[tokenId] = owner;
    }

    function register() external returns (uint256 agentId) {
        agentId = _nextAgentId++;
        _owners[agentId] = msg.sender;
        emit Registered(agentId, "", msg.sender);
    }

    function ownerOf(uint256 tokenId) external view returns (address) {
        address owner = _owners[tokenId];
        require(owner != address(0), "MockIdentityRegistry: nonexistent token");
        return owner;
    }
}
