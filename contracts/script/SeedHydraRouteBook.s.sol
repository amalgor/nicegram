// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {HydraRouteBook} from "../src/HydraRouteBook.sol";
import {ScriptBase} from "../src/foundry/ScriptBase.sol";
import {IERC20} from "../src/interfaces/IERC20.sol";
import {IIdentityRegistry} from "../src/interfaces/IIdentityRegistry.sol";

contract SeedHydraRouteBookScript is ScriptBase {
    address public constant BASE_SEPOLIA_USDC = 0x036CbD53842c5426634e7929541eC2318f3dCF7e;
    address public constant BASE_SEPOLIA_IDENTITY_REGISTRY = 0x8004A818BFB912233c491871b3d84c89A494BD9e;

    uint256 public constant DEFAULT_PRICE_PER_GB = 1_000_000;
    uint256 public constant DEFAULT_STAKE_AMOUNT = 1_000_000;
    uint256 public constant DEFAULT_BANDWIDTH_MBPS = 100;

    struct SeedConfig {
        address routeBook;
        address usdc;
        address identityRegistry;
        uint256 agentId;
    }

    function loadConfig() public returns (SeedConfig memory config) {
        config.routeBook = vm.envAddress("HRX_BASE_SEPOLIA_ROUTE_BOOK");
        config.usdc = vm.envOr("HRX_BASE_SEPOLIA_USDC", BASE_SEPOLIA_USDC);
        config.identityRegistry = vm.envOr("HRX_ERC8004_IDENTITY_REGISTRY", BASE_SEPOLIA_IDENTITY_REGISTRY);
        config.agentId = vm.envOr("HRX_SEED_AGENT_ID", uint256(0));
    }

    function run() external returns (uint256 agentId, uint256 offerId) {
        SeedConfig memory config = loadConfig();
        string[] memory protocols = new string[](1);
        protocols[0] = "vless";

        vm.startBroadcast();

        if (config.agentId == 0) {
            agentId = IIdentityRegistry(config.identityRegistry).register();
        } else {
            agentId = config.agentId;
        }

        IERC20(config.usdc).approve(config.routeBook, DEFAULT_STAKE_AMOUNT);
        offerId = HydraRouteBook(config.routeBook).createOffer(
            agentId,
            "seed://base-sepolia/vless",
            protocols,
            "US",
            DEFAULT_PRICE_PER_GB,
            DEFAULT_STAKE_AMOUNT,
            DEFAULT_BANDWIDTH_MBPS
        );

        vm.stopBroadcast();
    }
}
