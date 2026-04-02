// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {HydraRouteBook} from "../src/HydraRouteBook.sol";
import {ScriptBase} from "../src/foundry/ScriptBase.sol";

contract DeployHydraRouteBookScript is ScriptBase {
    error UnexpectedChain();

    uint256 public constant BASE_SEPOLIA_CHAIN_ID = 84532;
    address public constant BASE_SEPOLIA_USDC = 0x036CbD53842c5426634e7929541eC2318f3dCF7e;
    address public constant BASE_SEPOLIA_IDENTITY_REGISTRY = 0x8004A818BFB912233c491871b3d84c89A494BD9e;
    address public constant BASE_SEPOLIA_REPUTATION_REGISTRY = 0x8004B663056A597Dffe9eCcC1965A193B7388713;
    uint256 public constant DEFAULT_WITHDRAWAL_DELAY = 1 days;

    string internal constant DEPLOYMENT_KEY = "base-sepolia";
    string internal constant DEPLOYMENT_CHAIN_NAME = "BASE-SEPOLIA";

    struct DeploymentConfig {
        address usdc;
        address identityRegistry;
        address reputationRegistry;
        uint256 withdrawalDelay;
    }

    function loadConfig() public returns (DeploymentConfig memory config) {
        config.usdc = vm.envOr("HRX_BASE_SEPOLIA_USDC", BASE_SEPOLIA_USDC);
        config.identityRegistry = vm.envOr("HRX_ERC8004_IDENTITY_REGISTRY", BASE_SEPOLIA_IDENTITY_REGISTRY);
        config.reputationRegistry = vm.envOr("HRX_ERC8004_REPUTATION_REGISTRY", BASE_SEPOLIA_REPUTATION_REGISTRY);
        config.withdrawalDelay = DEFAULT_WITHDRAWAL_DELAY;
    }

    function deploymentFilePath() public returns (string memory) {
        return string.concat(vm.projectRoot(), "/deployments.json");
    }

    function run() external returns (HydraRouteBook routeBook) {
        if (block.chainid != BASE_SEPOLIA_CHAIN_ID) {
            revert UnexpectedChain();
        }

        DeploymentConfig memory config = loadConfig();

        vm.startBroadcast();
        routeBook =
            new HydraRouteBook(config.usdc, config.identityRegistry, config.reputationRegistry, config.withdrawalDelay);
        vm.stopBroadcast();
    }

    function writeDeploymentMetadata(address routeBook, bytes32 txHash, uint256 blockNumber)
        external
        returns (string memory path)
    {
        path = deploymentFilePath();
        writeDeploymentMetadataToPath(path, routeBook, txHash, blockNumber);
    }

    function writeDeploymentMetadataToPath(string memory path, address routeBook, bytes32 txHash, uint256 blockNumber)
        public
    {
        DeploymentConfig memory config = loadConfig();

        string memory json = string.concat(
            '{"',
            DEPLOYMENT_KEY,
            '":{',
            '"chain":"',
            DEPLOYMENT_CHAIN_NAME,
            '",',
            '"chainId":',
            vm.toString(BASE_SEPOLIA_CHAIN_ID),
            ",",
            '"routeBook":"',
            vm.toString(routeBook),
            '",',
            '"usdc":"',
            vm.toString(config.usdc),
            '",',
            '"identityRegistry":"',
            vm.toString(config.identityRegistry),
            '",',
            '"reputationRegistry":"',
            vm.toString(config.reputationRegistry),
            '",',
            '"withdrawalDelay":',
            vm.toString(config.withdrawalDelay),
            ",",
            '"blockNumber":',
            vm.toString(blockNumber),
            ",",
            '"txHash":"',
            vm.toString(txHash),
            '"}}'
        );

        vm.writeFile(path, json);
    }
}
