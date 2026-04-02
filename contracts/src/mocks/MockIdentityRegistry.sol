// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

contract MockIdentityRegistry {
    mapping(uint256 => address) private _owners;

    function setOwner(uint256 tokenId, address owner) external {
        _owners[tokenId] = owner;
    }

    function ownerOf(uint256 tokenId) external view returns (address) {
        address owner = _owners[tokenId];
        require(owner != address(0), "MockIdentityRegistry: nonexistent token");
        return owner;
    }
}
