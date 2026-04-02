// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

interface Vm {
    function warp(uint256 newTimestamp) external;

    function prank(address msgSender) external;

    function startPrank(address msgSender) external;

    function stopPrank() external;

    function expectRevert(bytes calldata revertData) external;

    function expectEmit(bool checkTopic1, bool checkTopic2, bool checkTopic3, bool checkData) external;

    function setEnv(string calldata name, string calldata value) external;

    function envAddress(string calldata name) external returns (address);

    function envOr(string calldata name, address defaultValue) external returns (address);

    function envOr(string calldata name, uint256 defaultValue) external returns (uint256);

    function projectRoot() external returns (string memory);

    function writeJson(string calldata json, string calldata path) external;

    function writeJson(string calldata json, string calldata path, string calldata valueKey) external;

    function readFile(string calldata path) external view returns (string memory);

    function writeFile(string calldata path, string calldata data) external;

    function removeFile(string calldata path) external;

    function exists(string calldata path) external view returns (bool);

    function serializeAddress(string calldata objectKey, string calldata valueKey, address value)
        external
        returns (string memory json);

    function serializeUint(string calldata objectKey, string calldata valueKey, uint256 value)
        external
        returns (string memory json);

    function serializeString(string calldata objectKey, string calldata valueKey, string calldata value)
        external
        returns (string memory json);

    function startBroadcast() external;

    function stopBroadcast() external;

    function toString(address value) external pure returns (string memory);

    function toString(bytes32 value) external pure returns (string memory);

    function toString(uint256 value) external pure returns (string memory);
}
