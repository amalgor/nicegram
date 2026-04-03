// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {HydraDealBoard} from "../src/HydraDealBoard.sol";
import {MockUSDC} from "../src/mocks/MockUSDC.sol";
import {MockIdentityRegistry} from "../src/mocks/MockIdentityRegistry.sol";
import {MockReputationRegistry} from "../src/mocks/MockReputationRegistry.sol";
import {TestBase} from "./TestBase.sol";

contract HydraDealBoardTest is TestBase {
    event DealOfferCreated(
        uint256 indexed offerId,
        address indexed dealer,
        uint256 indexed agentId,
        string currency,
        uint256 rate
    );
    event DealOfferDeactivated(uint256 indexed offerId, address indexed dealer);
    event EscrowCreated(
        uint256 indexed escrowId,
        uint256 indexed offerId,
        address indexed buyer,
        address dealer,
        uint256 usdcAmount,
        uint256 fiatAmount,
        uint64 expiresAt
    );
    event FiatMarkedSent(uint256 indexed escrowId, address indexed buyer);
    event EscrowCompleted(
        uint256 indexed escrowId,
        address indexed dealer,
        address indexed buyer,
        uint256 usdcAmount
    );
    event EscrowRejected(uint256 indexed escrowId, address indexed dealer);
    event EscrowExpired(uint256 indexed escrowId, address indexed buyer, uint256 usdcAmount);
    event ReputationFeedback(uint256 indexed escrowId, uint256 indexed agentId, bool positive);

    uint256 internal constant ONE_USDC = 1_000_000;
    uint256 internal constant ESCROW_TIMEOUT = 1 days;

    // Rate: 90 RUB per 1 USDC (stored with 6 decimals = 90_000_000)
    uint256 internal constant RUB_RATE = 90_000_000;

    MockUSDC internal usdc;
    MockIdentityRegistry internal identityRegistry;
    MockReputationRegistry internal reputationRegistry;
    HydraDealBoard internal dealBoard;

    address internal dealer = address(0xD001);
    address internal dealerTwo = address(0xD002);
    address internal buyer = address(0xB001);
    address internal stranger = address(0xCAFE);

    function setUp() public {
        usdc = new MockUSDC();
        identityRegistry = new MockIdentityRegistry();
        reputationRegistry = new MockReputationRegistry();

        dealBoard = new HydraDealBoard(
            address(usdc),
            address(identityRegistry),
            address(reputationRegistry),
            ESCROW_TIMEOUT
        );

        usdc.mint(dealer, 10_000 * ONE_USDC);
        usdc.mint(dealerTwo, 10_000 * ONE_USDC);
        usdc.mint(buyer, 100 * ONE_USDC);

        identityRegistry.setOwner(1, dealer);
        identityRegistry.setOwner(2, dealerTwo);

        // Dealer pre-approves the deal board for USDC transfers
        vm.prank(dealer);
        usdc.approve(address(dealBoard), type(uint256).max);

        vm.prank(dealerTwo);
        usdc.approve(address(dealBoard), type(uint256).max);
    }

    // ── createDealOffer ────────────────────────────────────────────────

    function testCreateDealOfferPersistsFields() public {
        string[] memory methods = _methods("bank-transfer", "sbp");

        vm.prank(dealer);
        vm.expectEmit(true, true, true, true);
        emit DealOfferCreated(1, dealer, 1, "RUB", RUB_RATE);

        uint256 offerId = dealBoard.createDealOffer(
            1, "RUB", RUB_RATE, 10 * ONE_USDC, 500 * ONE_USDC, methods
        );

        assertEq(offerId, 1, "offer id should start at 1");
        assertEq(dealBoard.totalOffers(), 1, "total offers should track");

        HydraDealBoard.DealOffer memory offer = dealBoard.getOffer(offerId);
        assertEq(offer.dealer, dealer, "dealer should persist");
        assertEq(offer.agentId, 1, "agent id should persist");
        assertEq(offer.currency, "RUB", "currency should persist");
        assertEq(offer.rate, RUB_RATE, "rate should persist");
        assertEq(offer.minAmount, 10 * ONE_USDC, "minAmount should persist");
        assertEq(offer.maxAmount, 500 * ONE_USDC, "maxAmount should persist");
        assertTrue(offer.active, "offer should be active");
        assertEq(offer.paymentMethods.length, 2, "payment methods should persist");
        assertEq(offer.paymentMethods[0], "bank-transfer", "first method");
        assertEq(offer.paymentMethods[1], "sbp", "second method");
    }

    function testCreateDealOfferRevertsIfNotAgentOwner() public {
        vm.prank(stranger);
        expectRevert(HydraDealBoard.AgentNotOwned.selector);
        dealBoard.createDealOffer(1, "RUB", RUB_RATE, 10 * ONE_USDC, 500 * ONE_USDC, _methods("sbp"));
    }

    function testCreateDealOfferRevertsOnEmptyCurrency() public {
        vm.prank(dealer);
        expectRevert(HydraDealBoard.EmptyCurrency.selector);
        dealBoard.createDealOffer(1, "", RUB_RATE, 10 * ONE_USDC, 500 * ONE_USDC, _methods("sbp"));
    }

    function testCreateDealOfferRevertsOnInvalidCurrency() public {
        vm.prank(dealer);
        expectRevert(HydraDealBoard.EmptyCurrency.selector);
        dealBoard.createDealOffer(1, "rub", RUB_RATE, 10 * ONE_USDC, 500 * ONE_USDC, _methods("sbp"));
    }

    function testCreateDealOfferRevertsOnZeroRate() public {
        vm.prank(dealer);
        expectRevert(HydraDealBoard.InvalidRate.selector);
        dealBoard.createDealOffer(1, "RUB", 0, 10 * ONE_USDC, 500 * ONE_USDC, _methods("sbp"));
    }

    function testCreateDealOfferRevertsOnInvalidAmountRange() public {
        vm.prank(dealer);
        expectRevert(HydraDealBoard.InvalidAmountRange.selector);
        dealBoard.createDealOffer(1, "RUB", RUB_RATE, 500 * ONE_USDC, 10 * ONE_USDC, _methods("sbp"));
    }

    function testCreateDealOfferRevertsOnEmptyPaymentMethods() public {
        string[] memory empty = new string[](0);
        vm.prank(dealer);
        expectRevert(HydraDealBoard.EmptyPaymentMethods.selector);
        dealBoard.createDealOffer(1, "RUB", RUB_RATE, 10 * ONE_USDC, 500 * ONE_USDC, empty);
    }

    // ── deactivateDealOffer ────────────────────────────────────────────

    function testDeactivateDealOfferEmitsEvent() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);

        vm.prank(dealer);
        vm.expectEmit(true, true, false, true);
        emit DealOfferDeactivated(offerId, dealer);
        dealBoard.deactivateDealOffer(offerId);

        HydraDealBoard.DealOffer memory offer = dealBoard.getOffer(offerId);
        assertFalse(offer.active, "offer should be inactive");
    }

    function testDeactivateDealOfferRevertsIfNotDealer() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);

        vm.prank(stranger);
        expectRevert(HydraDealBoard.NotOfferDealer.selector);
        dealBoard.deactivateDealOffer(offerId);
    }

    function testDeactivateDealOfferRevertsIfAlreadyInactive() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);

        vm.prank(dealer);
        dealBoard.deactivateDealOffer(offerId);

        vm.prank(dealer);
        expectRevert(HydraDealBoard.OfferAlreadyInactive.selector);
        dealBoard.deactivateDealOffer(offerId);
    }

    // ── acceptDeal ─────────────────────────────────────────────────────

    function testAcceptDealLocksUsdcInEscrow() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        uint256 dealerBalanceBefore = usdc.balanceOf(dealer);

        uint256 usdcAmount = 100 * ONE_USDC;
        uint256 expectedFiat = (usdcAmount * RUB_RATE) / 1e6; // 9000 * 1e6

        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, usdcAmount);

        assertEq(escrowId, 1, "escrow id should start at 1");
        assertEq(dealBoard.totalEscrows(), 1, "total escrows should track");
        assertEq(
            usdc.balanceOf(address(dealBoard)),
            usdcAmount,
            "USDC should be locked in contract"
        );
        assertEq(
            usdc.balanceOf(dealer),
            dealerBalanceBefore - usdcAmount,
            "dealer balance should decrease"
        );

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(escrow.offerId, offerId, "offerId should match");
        assertEq(escrow.buyer, buyer, "buyer should match");
        assertEq(escrow.dealer, dealer, "dealer should match");
        assertEq(escrow.usdcAmount, usdcAmount, "usdcAmount should match");
        assertEq(escrow.fiatAmount, expectedFiat, "fiatAmount should match");
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Funded), "status should be Funded");
    }

    function testAcceptDealRevertsOnInactiveOffer() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(dealer);
        dealBoard.deactivateDealOffer(offerId);

        vm.prank(buyer);
        expectRevert(HydraDealBoard.OfferNotActive.selector);
        dealBoard.acceptDeal(offerId, 100 * ONE_USDC);
    }

    function testAcceptDealRevertsAmountBelowMin() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);

        vm.prank(buyer);
        expectRevert(HydraDealBoard.AmountBelowMin.selector);
        dealBoard.acceptDeal(offerId, 1 * ONE_USDC); // min is 10
    }

    function testAcceptDealRevertsAmountAboveMax() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);

        vm.prank(buyer);
        expectRevert(HydraDealBoard.AmountAboveMax.selector);
        dealBoard.acceptDeal(offerId, 1000 * ONE_USDC); // max is 500
    }

    // ── markFiatSent ───────────────────────────────────────────────────

    function testMarkFiatSentUpdatesStatus() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        vm.prank(buyer);
        vm.expectEmit(true, true, false, true);
        emit FiatMarkedSent(escrowId, buyer);
        dealBoard.markFiatSent(escrowId);

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Sent), "status should be Sent");
    }

    function testMarkFiatSentRevertsIfNotBuyer() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        vm.prank(stranger);
        expectRevert(HydraDealBoard.NotBuyer.selector);
        dealBoard.markFiatSent(escrowId);
    }

    function testMarkFiatSentRevertsIfExpired() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        vm.warp(block.timestamp + ESCROW_TIMEOUT);

        vm.prank(buyer);
        expectRevert(HydraDealBoard.AlreadyExpired.selector);
        dealBoard.markFiatSent(escrowId);
    }

    // ── confirmReceipt ─────────────────────────────────────────────────

    function testConfirmReceiptReleasesUsdcToBuyer() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        uint256 usdcAmount = 100 * ONE_USDC;
        uint256 buyerBalanceBefore = usdc.balanceOf(buyer);

        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, usdcAmount);

        vm.prank(buyer);
        dealBoard.markFiatSent(escrowId);

        vm.prank(dealer);
        vm.expectEmit(true, true, true, true);
        emit EscrowCompleted(escrowId, dealer, buyer, usdcAmount);
        dealBoard.confirmReceipt(escrowId);

        assertEq(
            usdc.balanceOf(buyer),
            buyerBalanceBefore + usdcAmount,
            "buyer should receive USDC"
        );
        assertEq(
            usdc.balanceOf(address(dealBoard)),
            0,
            "contract should have no remaining USDC"
        );

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Completed), "status should be Completed");
    }

    function testConfirmReceiptRevertsIfNotDealer() public {
        (uint256 escrowId,) = _createEscrowInSentState();

        vm.prank(stranger);
        expectRevert(HydraDealBoard.NotDealer.selector);
        dealBoard.confirmReceipt(escrowId);
    }

    function testConfirmReceiptRevertsIfNotSentStatus() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        // Status is Funded, not Sent
        vm.prank(dealer);
        expectRevert(HydraDealBoard.WrongStatus.selector);
        dealBoard.confirmReceipt(escrowId);
    }

    // ── rejectDeal ─────────────────────────────────────────────────────

    function testRejectDealReturnsUsdcToDealer() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        uint256 usdcAmount = 100 * ONE_USDC;
        uint256 dealerBalanceBefore = usdc.balanceOf(dealer);

        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, usdcAmount);

        vm.prank(dealer);
        vm.expectEmit(true, true, false, true);
        emit EscrowRejected(escrowId, dealer);
        dealBoard.rejectDeal(escrowId);

        assertEq(
            usdc.balanceOf(dealer),
            dealerBalanceBefore,
            "dealer should get USDC back"
        );

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Rejected), "status should be Rejected");
    }

    function testRejectDealRevertsIfNotDealer() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        vm.prank(stranger);
        expectRevert(HydraDealBoard.NotDealer.selector);
        dealBoard.rejectDeal(escrowId);
    }

    function testRejectDealRevertsIfStatusNotFunded() public {
        (uint256 escrowId,) = _createEscrowInSentState();

        vm.prank(dealer);
        expectRevert(HydraDealBoard.WrongStatus.selector);
        dealBoard.rejectDeal(escrowId);
    }

    // ── claimExpired ───────────────────────────────────────────────────

    function testClaimExpiredReleasesUsdcToBuyerAfterTimeout() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        uint256 usdcAmount = 100 * ONE_USDC;
        uint256 buyerBalanceBefore = usdc.balanceOf(buyer);

        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, usdcAmount);

        vm.prank(buyer);
        dealBoard.markFiatSent(escrowId);

        // Fast forward past expiry
        vm.warp(block.timestamp + ESCROW_TIMEOUT);

        vm.expectEmit(true, true, false, true);
        emit EscrowExpired(escrowId, buyer, usdcAmount);
        dealBoard.claimExpired(escrowId);

        assertEq(
            usdc.balanceOf(buyer),
            buyerBalanceBefore + usdcAmount,
            "buyer should receive USDC on expiry"
        );

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Expired), "status should be Expired");
    }

    function testClaimExpiredWorksFromFundedStatus() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        uint256 usdcAmount = 50 * ONE_USDC;

        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, usdcAmount);

        vm.warp(block.timestamp + ESCROW_TIMEOUT);
        dealBoard.claimExpired(escrowId);

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Expired), "should expire from Funded");
    }

    function testClaimExpiredRevertsBeforeTimeout() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        expectRevert(HydraDealBoard.NotExpired.selector);
        dealBoard.claimExpired(escrowId);
    }

    function testClaimExpiredRevertsOnCompletedEscrow() public {
        (uint256 escrowId,) = _createEscrowInSentState();

        vm.prank(dealer);
        dealBoard.confirmReceipt(escrowId);

        vm.warp(block.timestamp + ESCROW_TIMEOUT);

        expectRevert(HydraDealBoard.WrongStatus.selector);
        dealBoard.claimExpired(escrowId);
    }

    // ── getActiveDeals ─────────────────────────────────────────────────

    function testGetActiveDealsFiltersByCurrency() public {
        _createDefaultOffer(dealer, 1);           // RUB

        vm.prank(dealerTwo);
        dealBoard.createDealOffer(2, "TRY", 30_000_000, 10 * ONE_USDC, 500 * ONE_USDC, _methods("bank-transfer"));

        HydraDealBoard.DealOffer[] memory rubOffers = dealBoard.getActiveDeals("RUB");
        assertEq(rubOffers.length, 1, "should find 1 RUB offer");
        assertEq(rubOffers[0].dealer, dealer, "RUB dealer should match");

        HydraDealBoard.DealOffer[] memory tryOffers = dealBoard.getActiveDeals("TRY");
        assertEq(tryOffers.length, 1, "should find 1 TRY offer");

        HydraDealBoard.DealOffer[] memory ngnOffers = dealBoard.getActiveDeals("NGN");
        assertEq(ngnOffers.length, 0, "should find 0 NGN offers");
    }

    function testGetActiveDealsExcludesInactive() public {
        uint256 offerId = _createDefaultOffer(dealer, 1);

        vm.prank(dealer);
        dealBoard.deactivateDealOffer(offerId);

        HydraDealBoard.DealOffer[] memory offers = dealBoard.getActiveDeals("RUB");
        assertEq(offers.length, 0, "deactivated offers should be excluded");
    }

    // ── Full lifecycle (happy path) ────────────────────────────────────

    function testFullHappyPathLifecycle() public {
        // 1. Dealer creates offer
        uint256 offerId = _createDefaultOffer(dealer, 1);

        // 2. Buyer accepts deal
        uint256 usdcAmount = 100 * ONE_USDC;
        vm.prank(buyer);
        uint256 escrowId = dealBoard.acceptDeal(offerId, usdcAmount);

        // 3. Buyer marks fiat as sent
        vm.prank(buyer);
        dealBoard.markFiatSent(escrowId);

        // 4. Dealer confirms receipt
        uint256 buyerBalanceBefore = usdc.balanceOf(buyer);
        vm.prank(dealer);
        dealBoard.confirmReceipt(escrowId);

        // Verify outcome
        assertEq(usdc.balanceOf(buyer), buyerBalanceBefore + usdcAmount, "buyer should receive USDC");
        assertEq(usdc.balanceOf(address(dealBoard)), 0, "escrow should be empty");

        HydraDealBoard.Escrow memory escrow = dealBoard.getEscrow(escrowId);
        assertEq(uint256(escrow.status), uint256(HydraDealBoard.EscrowStatus.Completed), "final status Completed");
    }

    // ── Helpers ────────────────────────────────────────────────────────

    function _createDefaultOffer(address who, uint256 agentId)
        internal
        returns (uint256 offerId)
    {
        vm.prank(who);
        offerId = dealBoard.createDealOffer(
            agentId, "RUB", RUB_RATE, 10 * ONE_USDC, 500 * ONE_USDC, _methods("bank-transfer", "sbp")
        );
    }

    function _createEscrowInSentState()
        internal
        returns (uint256 escrowId, uint256 offerId)
    {
        offerId = _createDefaultOffer(dealer, 1);

        vm.prank(buyer);
        escrowId = dealBoard.acceptDeal(offerId, 100 * ONE_USDC);

        vm.prank(buyer);
        dealBoard.markFiatSent(escrowId);
    }

    function _methods(string memory first) internal pure returns (string[] memory values) {
        values = new string[](1);
        values[0] = first;
    }

    function _methods(string memory first, string memory second) internal pure returns (string[] memory values) {
        values = new string[](2);
        values[0] = first;
        values[1] = second;
    }
}
