// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {HydraDealBoard} from "../src/HydraDealBoard.sol";
import {ScriptBase} from "../src/foundry/ScriptBase.sol";

contract DeployHydraDealBoardScript is ScriptBase {
    error UnexpectedChain();

    uint256 public constant BASE_SEPOLIA_CHAIN_ID = 84532;
    address public constant BASE_SEPOLIA_USDC = 0x036CbD53842c5426634e7929541eC2318f3dCF7e;
    address public constant BASE_SEPOLIA_IDENTITY_REGISTRY = 0x8004A818BFB912233c491871b3d84c89A494BD9e;
    address public constant BASE_SEPOLIA_REPUTATION_REGISTRY = 0x8004B663056A597Dffe9eCcC1965A193B7388713;
    uint256 public constant DEFAULT_ESCROW_TIMEOUT = 1 days;

    struct DeploymentConfig {
        address usdc;
        address identityRegistry;
        address reputationRegistry;
        uint256 escrowTimeout;
    }

    function loadConfig() public returns (DeploymentConfig memory config) {
        config.usdc = vm.envOr("HRX_BASE_SEPOLIA_USDC", BASE_SEPOLIA_USDC);
        config.identityRegistry = vm.envOr("HRX_ERC8004_IDENTITY_REGISTRY", BASE_SEPOLIA_IDENTITY_REGISTRY);
        config.reputationRegistry = vm.envOr("HRX_ERC8004_REPUTATION_REGISTRY", BASE_SEPOLIA_REPUTATION_REGISTRY);
        config.escrowTimeout = DEFAULT_ESCROW_TIMEOUT;
    }

    function run() external returns (HydraDealBoard dealBoard) {
        if (block.chainid != BASE_SEPOLIA_CHAIN_ID) {
            revert UnexpectedChain();
        }

        DeploymentConfig memory config = loadConfig();

        vm.startBroadcast();
        dealBoard = new HydraDealBoard(
            config.usdc,
            config.identityRegistry,
            config.reputationRegistry,
            config.escrowTimeout
        );
        vm.stopBroadcast();
    }
}
