// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {HydraRouteBook} from "../src/HydraRouteBook.sol";
import {MockUSDC} from "../src/mocks/MockUSDC.sol";
import {MockIdentityRegistry} from "../src/mocks/MockIdentityRegistry.sol";
import {MockReputationRegistry} from "../src/mocks/MockReputationRegistry.sol";
import {DeployHydraRouteBookScript} from "../script/DeployHydraRouteBook.s.sol";
import {TestBase} from "./TestBase.sol";

contract HydraRouteBookTest is TestBase {
    event OfferCreated(
        uint256 indexed offerId,
        address indexed provider,
        uint256 indexed agentId,
        string region,
        uint256 pricePerGB,
        uint256 stakeAmount
    );
    event OfferDeactivated(uint256 indexed offerId, address indexed provider, uint64 deactivatedAt);
    event StakeWithdrawn(uint256 indexed offerId, address indexed provider, uint256 amount);
    event SlashClaimSubmitted(uint256 indexed offerId, address indexed claimant, bytes32 indexed proofHash);

    uint256 internal constant ONE_USDC = 1_000_000;

    MockUSDC internal usdc;
    MockIdentityRegistry internal identityRegistry;
    MockReputationRegistry internal reputationRegistry;
    HydraRouteBook internal routeBook;

    address internal provider = address(0xA11CE);
    address internal providerTwo = address(0xB0B);
    address internal stranger = address(0xCAFE);

    function setUp() public {
        usdc = new MockUSDC();
        identityRegistry = new MockIdentityRegistry();
        reputationRegistry = new MockReputationRegistry();

        routeBook = new HydraRouteBook(address(usdc), address(identityRegistry), address(reputationRegistry), 1 days);

        usdc.mint(provider, 1_000 * ONE_USDC);
        usdc.mint(providerTwo, 1_000 * ONE_USDC);
        usdc.mint(stranger, 1_000 * ONE_USDC);

        identityRegistry.setOwner(1, provider);
        identityRegistry.setOwner(2, providerTwo);
        identityRegistry.setOwner(3, providerTwo);
    }

    function testCreateOfferEscrowsStakeAndPersistsFields() public {
        string[] memory protocols = _protocols("vless-reality", "wss");
        uint256 stakeAmount = 250 * ONE_USDC;
        uint256 pricePerGB = 5 * ONE_USDC;

        vm.startPrank(provider);
        usdc.approve(address(routeBook), stakeAmount);

        vm.expectEmit(true, true, true, true);
        emit OfferCreated(1, provider, 1, "US", pricePerGB, stakeAmount);

        uint256 offerId =
            routeBook.createOffer(1, "ciphertext://provider-1", protocols, "US", pricePerGB, stakeAmount, 250);
        vm.stopPrank();

        assertEq(offerId, 1, "offer id should increment from one");
        assertEq(routeBook.totalOffers(), 1, "total offers should track creations");
        assertEq(usdc.balanceOf(address(routeBook)), stakeAmount, "stake should be escrowed in contract");
        assertEq(usdc.balanceOf(provider), 750 * ONE_USDC, "provider balance should decrease by staked amount");

        HydraRouteBook.RouteOffer memory offer = routeBook.getOffer(offerId);
        assertEq(offer.provider, provider, "provider should be persisted");
        assertEq(offer.agentId, 1, "agent id should be persisted");
        assertEq(offer.endpointCiphertext, "ciphertext://provider-1", "endpoint ciphertext should persist");
        assertEq(offer.region, "US", "region should persist");
        assertEq(offer.pricePerGB, pricePerGB, "price should persist");
        assertEq(offer.stakeAmount, stakeAmount, "stake should persist");
        assertEq(offer.bandwidthMbps, 250, "bandwidth should persist");
        assertEq(offer.protocols.length, 2, "protocol list should persist");
        assertEq(offer.protocols[0], "vless-reality", "first protocol should persist");
        assertEq(offer.protocols[1], "wss", "second protocol should persist");
        assertTrue(offer.active, "offer should be active after creation");
        assertEq(uint256(offer.deactivatedAt), 0, "deactivatedAt should be empty until deactivation");
    }

    function testQueryOffersFiltersActiveRegionAndProtocol() public {
        uint256 matchingOfferId = _createOffer(provider, 1, "US", _protocols("vless-reality", "wss"), 200 * ONE_USDC);
        _createOffer(providerTwo, 2, "US", _protocols("wss"), 150 * ONE_USDC);
        _createOffer(providerTwo, 3, "DE", _protocols("vless-reality"), 175 * ONE_USDC);

        HydraRouteBook.RouteOffer[] memory matches = routeBook.queryOffers("US", "vless-reality");
        assertEq(matches.length, 1, "only active matching offers should be returned");
        assertEq(matches[0].provider, provider, "matching provider should be returned");
        assertEq(matches[0].agentId, 1, "matching agent should be returned");

        vm.prank(provider);
        routeBook.deactivateOffer(matchingOfferId);

        matches = routeBook.queryOffers("US", "vless-reality");
        assertEq(matches.length, 0, "deactivated offers must disappear from query results");
    }

    function testDeactivateOfferOnlyProviderCanCall() public {
        uint256 offerId = _createOffer(provider, 1, "US", _protocols("vless-reality"), 200 * ONE_USDC);

        vm.prank(stranger);
        expectRevert(HydraRouteBook.NotProvider.selector);
        routeBook.deactivateOffer(offerId);
    }

    function testDeactivateOfferEmitsEventAndUpdatesState() public {
        uint256 offerId = _createOffer(provider, 1, "US", _protocols("vless-reality"), 200 * ONE_USDC);

        vm.warp(2 days);
        vm.prank(provider);
        vm.expectEmit(true, true, false, true);
        emit OfferDeactivated(offerId, provider, uint64(block.timestamp));
        routeBook.deactivateOffer(offerId);

        HydraRouteBook.RouteOffer memory offer = routeBook.getOffer(offerId);
        assertFalse(offer.active, "offer should be inactive after deactivation");
        assertEq(uint256(offer.deactivatedAt), block.timestamp, "deactivatedAt should be stamped");
    }

    function testWithdrawStakeRequiresDelayAndReturnsEscrow() public {
        uint256 stakeAmount = 200 * ONE_USDC;
        uint256 offerId = _createOffer(provider, 1, "US", _protocols("vless-reality"), stakeAmount);

        vm.prank(provider);
        routeBook.deactivateOffer(offerId);

        vm.prank(provider);
        expectRevert(HydraRouteBook.WithdrawalNotReady.selector);
        routeBook.withdrawStake(offerId);

        vm.warp(block.timestamp + 1 days);
        vm.prank(provider);
        vm.expectEmit(true, true, false, true);
        emit StakeWithdrawn(offerId, provider, stakeAmount);
        routeBook.withdrawStake(offerId);

        assertEq(usdc.balanceOf(provider), 1_000 * ONE_USDC, "stake should be returned after withdrawal");
        assertEq(usdc.balanceOf(address(routeBook)), 0, "contract should no longer hold stake");

        HydraRouteBook.RouteOffer memory offer = routeBook.getOffer(offerId);
        assertEq(offer.stakeAmount, 0, "withdrawn offer should have zero remaining stake");
    }

    function testSubmitSlashClaimEmitsHookEvent() public {
        uint256 offerId = _createOffer(provider, 1, "US", _protocols("vless-reality"), 200 * ONE_USDC);
        bytes32 proofHash = keccak256("validator-proof-placeholder");

        vm.prank(stranger);
        vm.expectEmit(true, true, true, true);
        emit SlashClaimSubmitted(offerId, stranger, proofHash);
        routeBook.submitSlashClaim(offerId, proofHash);
    }

    function testCreateOfferRevertsWithoutAllowance() public {
        string[] memory protocols = _protocols("vless-reality");

        vm.prank(provider);
        expectRevert(HydraRouteBook.TransferFailed.selector);
        routeBook.createOffer(1, "ciphertext://provider-1", protocols, "US", 5 * ONE_USDC, 100 * ONE_USDC, 100);
    }

    function testCreateOfferRevertsOnEmptyProtocolList() public {
        string[] memory protocols = new string[](0);

        vm.startPrank(provider);
        usdc.approve(address(routeBook), 100 * ONE_USDC);
        expectRevert(HydraRouteBook.EmptyProtocols.selector);
        routeBook.createOffer(1, "ciphertext://provider-1", protocols, "US", 5 * ONE_USDC, 100 * ONE_USDC, 100);
        vm.stopPrank();
    }

    function testCreateOfferRevertsOnInvalidRegion() public {
        vm.startPrank(provider);
        usdc.approve(address(routeBook), 100 * ONE_USDC);
        expectRevert(HydraRouteBook.InvalidRegion.selector);
        routeBook.createOffer(
            1, "ciphertext://provider-1", _protocols("vless-reality"), "usa", 5 * ONE_USDC, 100 * ONE_USDC, 100
        );
        vm.stopPrank();
    }

    function testCreateOfferRevertsWhenAgentOwnerMismatch() public {
        vm.startPrank(stranger);
        usdc.approve(address(routeBook), 100 * ONE_USDC);
        expectRevert(HydraRouteBook.AgentNotOwned.selector);
        routeBook.createOffer(
            1, "ciphertext://provider-1", _protocols("vless-reality"), "US", 5 * ONE_USDC, 100 * ONE_USDC, 100
        );
        vm.stopPrank();
    }

    function testDeactivateOfferRevertsWhenAlreadyInactive() public {
        uint256 offerId = _createOffer(provider, 1, "US", _protocols("vless-reality"), 100 * ONE_USDC);

        vm.prank(provider);
        routeBook.deactivateOffer(offerId);

        vm.prank(provider);
        expectRevert(HydraRouteBook.OfferAlreadyInactive.selector);
        routeBook.deactivateOffer(offerId);
    }

    function _createOffer(
        address owner,
        uint256 agentId,
        string memory region,
        string[] memory protocols,
        uint256 stakeAmount
    ) internal returns (uint256 offerId) {
        vm.startPrank(owner);
        usdc.approve(address(routeBook), stakeAmount);
        offerId =
            routeBook.createOffer(agentId, "ciphertext://route", protocols, region, 5 * ONE_USDC, stakeAmount, 100);
        vm.stopPrank();
    }

    function _protocols(string memory first) internal pure returns (string[] memory values) {
        values = new string[](1);
        values[0] = first;
    }

    function _protocols(string memory first, string memory second) internal pure returns (string[] memory values) {
        values = new string[](2);
        values[0] = first;
        values[1] = second;
    }
}

contract DeployHydraRouteBookScriptTest is TestBase {
    DeployHydraRouteBookScript internal script;

    function setUp() public {
        script = new DeployHydraRouteBookScript();
    }

    function testLoadConfigUsesBaseSepoliaDefaults() public {
        DeployHydraRouteBookScript.DeploymentConfig memory config = script.loadConfig();

        assertEq(
            config.usdc, 0x036CbD53842c5426634e7929541eC2318f3dCF7e, "script should default to Circle Base Sepolia USDC"
        );
        assertEq(
            config.identityRegistry,
            0x8004A818BFB912233c491871b3d84c89A494BD9e,
            "identity registry should default to live Base Sepolia registry"
        );
        assertEq(
            config.reputationRegistry,
            0x8004B663056A597Dffe9eCcC1965A193B7388713,
            "reputation registry should default to live Base Sepolia registry"
        );
        assertEq(config.withdrawalDelay, 1 days, "withdrawal delay should use phase one default");
    }

    function testWriteDeploymentMetadataWritesExpectedShape() public {
        string memory path = string.concat(vm.projectRoot(), "/.tmp/deployments.smoke.json");
        if (vm.exists(path)) {
            vm.removeFile(path);
        }

        vm.setEnv("HRX_ERC8004_IDENTITY_REGISTRY", vm.toString(address(0x1001)));
        vm.setEnv("HRX_ERC8004_REPUTATION_REGISTRY", vm.toString(address(0x2002)));
        script.writeDeploymentMetadataToPath(path, address(0x3003), bytes32(uint256(0xAABBCCDD)), 123456);

        string memory json = vm.readFile(path);
        assertContains(json, "\"base-sepolia\"", "json should be keyed by base-sepolia");
        assertContains(json, "\"chain\":\"BASE-SEPOLIA\"", "json should include chain name");
        assertContains(
            json, "\"routeBook\":\"0x0000000000000000000000000000000000003003\"", "json should include contract address"
        );
        assertContains(
            json,
            "\"identityRegistry\":\"0x0000000000000000000000000000000000001001\"",
            "json should include identity registry"
        );
        assertContains(
            json,
            "\"reputationRegistry\":\"0x0000000000000000000000000000000000002002\"",
            "json should include reputation registry"
        );
        assertContains(
            json,
            "\"txHash\":\"0x00000000000000000000000000000000000000000000000000000000aabbccdd\"",
            "json should include tx hash"
        );
        assertContains(json, "\"blockNumber\":123456", "json should include block number");
    }
}
