// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {Vm} from "../src/foundry/Vm.sol";

contract TestBase {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function assertTrue(bool condition, string memory message) internal pure {
        if (!condition) {
            require(condition, message);
        }
    }

    function assertFalse(bool condition, string memory message) internal pure {
        assertTrue(!condition, message);
    }

    function assertEq(uint256 left, uint256 right, string memory message) internal pure {
        if (left != right) {
            require(left == right, message);
        }
    }

    function assertEq(uint64 left, uint64 right, string memory message) internal pure {
        if (left != right) {
            require(left == right, message);
        }
    }

    function assertEq(address left, address right, string memory message) internal pure {
        if (left != right) {
            require(left == right, message);
        }
    }

    function assertEq(bool left, bool right, string memory message) internal pure {
        if (left != right) {
            require(left == right, message);
        }
    }

    function assertEq(bytes32 left, bytes32 right, string memory message) internal pure {
        if (left != right) {
            require(left == right, message);
        }
    }

    function assertEq(string memory left, string memory right, string memory message) internal pure {
        if (keccak256(bytes(left)) != keccak256(bytes(right))) {
            require(keccak256(bytes(left)) == keccak256(bytes(right)), message);
        }
    }

    function assertContains(string memory haystack, string memory needle, string memory message) internal pure {
        bytes memory haystackBytes = bytes(haystack);
        bytes memory needleBytes = bytes(needle);

        if (needleBytes.length == 0) {
            return;
        }
        if (needleBytes.length > haystackBytes.length) {
            require(needleBytes.length <= haystackBytes.length, message);
        }

        for (uint256 i = 0; i <= haystackBytes.length - needleBytes.length; ++i) {
            bool found = true;
            for (uint256 j = 0; j < needleBytes.length; ++j) {
                if (haystackBytes[i + j] != needleBytes[j]) {
                    found = false;
                    break;
                }
            }
            if (found) {
                return;
            }
        }

        require(false, message);
    }

    function expectRevert(bytes4 selector) internal {
        vm.expectRevert(abi.encodeWithSelector(selector));
    }
}
