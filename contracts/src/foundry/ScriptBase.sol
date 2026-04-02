// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {Vm} from "./Vm.sol";

abstract contract ScriptBase {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
}
