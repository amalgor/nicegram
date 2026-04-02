// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

contract MockReputationRegistry {
    struct Feedback {
        int128 value;
        uint8 valueDecimals;
        string tag1;
        string tag2;
        bool revoked;
    }

    mapping(uint256 => address[]) private _clients;
    mapping(uint256 => mapping(address => bool)) private _seenClient;
    mapping(uint256 => mapping(address => uint64)) private _lastIndex;
    mapping(uint256 => mapping(address => mapping(uint64 => Feedback))) private _feedback;

    function giveFeedback(
        uint256 agentId,
        int128 value,
        uint8 valueDecimals,
        string calldata tag1,
        string calldata tag2,
        string calldata,
        string calldata,
        bytes32
    ) external {
        if (!_seenClient[agentId][msg.sender]) {
            _seenClient[agentId][msg.sender] = true;
            _clients[agentId].push(msg.sender);
        }

        uint64 feedbackIndex = ++_lastIndex[agentId][msg.sender];
        _feedback[agentId][msg.sender][feedbackIndex] = Feedback({
            value: value,
            valueDecimals: valueDecimals,
            tag1: tag1,
            tag2: tag2,
            revoked: false
        });
    }

    function getClients(uint256 agentId) external view returns (address[] memory) {
        return _clients[agentId];
    }

    function getSummary(uint256 agentId, address[] calldata clientAddresses, string calldata tag1, string calldata tag2)
        external
        view
        returns (uint64 count, int128 summaryValue, uint8 summaryValueDecimals)
    {
        for (uint256 i = 0; i < clientAddresses.length; ++i) {
            (uint64 clientCount, int128 clientSummaryValue, uint8 clientSummaryDecimals) =
                _summarizeClient(agentId, clientAddresses[i], tag1, tag2);
            count += clientCount;
            summaryValue += clientSummaryValue;
            if (clientCount != 0) {
                summaryValueDecimals = clientSummaryDecimals;
            }
        }
    }

    function _summarizeClient(uint256 agentId, address client, string calldata tag1, string calldata tag2)
        private
        view
        returns (uint64 count, int128 summaryValue, uint8 summaryValueDecimals)
    {
        uint64 lastIndex = _lastIndex[agentId][client];
        for (uint64 feedbackIndex = 1; feedbackIndex <= lastIndex; ++feedbackIndex) {
            Feedback storage item = _feedback[agentId][client][feedbackIndex];
            if (!_matches(item, tag1, tag2)) {
                continue;
            }

            count += 1;
            summaryValue += item.value;
            summaryValueDecimals = item.valueDecimals;
        }
    }

    function _matches(Feedback storage item, string calldata tag1, string calldata tag2) private view returns (bool) {
        if (item.revoked) {
            return false;
        }
        if (bytes(tag1).length != 0 && keccak256(bytes(item.tag1)) != keccak256(bytes(tag1))) {
            return false;
        }
        if (bytes(tag2).length != 0 && keccak256(bytes(item.tag2)) != keccak256(bytes(tag2))) {
            return false;
        }
        return true;
    }
}
