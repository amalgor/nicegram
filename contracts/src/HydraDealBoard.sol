// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {IERC20} from "./interfaces/IERC20.sol";
import {IERC721} from "./interfaces/IERC721.sol";
import {IReputationRegistry} from "./interfaces/IReputationRegistry.sol";

/// @title HydraDealBoard — P2P fiat-to-USDC deal board with built-in escrow
/// @notice Dealers post offers to sell USDC for fiat. Buyers accept offers,
///         locking the dealer's USDC in escrow. After the buyer sends fiat
///         off-chain, the dealer confirms receipt and USDC is released to buyer.
///         ERC-8183-inspired state machine adapted for P2P fiat settlement.
contract HydraDealBoard {

    // ── Enums ──────────────────────────────────────────────────────────

    enum EscrowStatus {
        Funded,     // Dealer USDC locked, awaiting buyer fiat transfer
        Sent,       // Buyer marked fiat as sent, awaiting dealer confirmation
        Completed,  // Dealer confirmed fiat receipt, USDC released to buyer
        Rejected,   // Dealer rejected (before buyer sends fiat), USDC returned to dealer
        Expired     // Deadline passed without dealer confirmation, buyer claimed USDC
    }

    // ── Structs ────────────────────────────────────────────────────────

    struct DealOffer {
        address dealer;
        uint256 agentId;
        string currency;          // ISO 4217 (e.g. "RUB", "TRY", "NGN")
        uint256 rate;             // Fiat units per 1 USDC, 6 decimal precision
        uint256 minAmount;        // Min USDC amount per deal (6 decimals)
        uint256 maxAmount;        // Max USDC amount per deal (6 decimals)
        string[] paymentMethods;  // e.g. ["bank-transfer", "sbp"]
        bool active;
    }

    struct Escrow {
        uint256 offerId;
        address buyer;
        address dealer;
        uint256 usdcAmount;
        uint256 fiatAmount;       // Informational: usdcAmount * rate / 1e6
        EscrowStatus status;
        uint64 createdAt;
        uint64 expiresAt;
    }

    // ── Errors ─────────────────────────────────────────────────────────

    error ZeroAddress();
    error AgentNotOwned();
    error EmptyCurrency();
    error InvalidRate();
    error InvalidAmountRange();
    error EmptyPaymentMethods();
    error TransferFailed();
    error UnknownOffer();
    error OfferNotActive();
    error AmountBelowMin();
    error AmountAboveMax();
    error InsufficientDealerBalance();
    error UnknownEscrow();
    error WrongStatus();
    error NotBuyer();
    error NotDealer();
    error NotExpired();
    error AlreadyExpired();
    error OfferAlreadyInactive();
    error NotOfferDealer();

    // ── Events ─────────────────────────────────────────────────────────

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

    event ReputationFeedback(
        uint256 indexed escrowId,
        uint256 indexed agentId,
        bool positive
    );

    // ── Immutables & State ─────────────────────────────────────────────

    IERC20 public immutable usdc;
    address public immutable identityRegistry;
    IReputationRegistry public immutable reputationRegistry;

    /// Time window for the dealer to confirm fiat receipt after escrow creation
    uint256 public immutable escrowTimeout;

    uint256 private _offerCount;
    uint256 private _escrowCount;

    mapping(uint256 => DealOffer) private _offers;
    mapping(uint256 => Escrow) private _escrows;

    // ── Constructor ────────────────────────────────────────────────────

    constructor(
        address usdc_,
        address identityRegistry_,
        address reputationRegistry_,
        uint256 escrowTimeout_
    ) {
        if (usdc_ == address(0)) revert ZeroAddress();
        if (identityRegistry_ == address(0)) revert ZeroAddress();
        if (reputationRegistry_ == address(0)) revert ZeroAddress();
        if (escrowTimeout_ == 0) revert InvalidRate();

        usdc = IERC20(usdc_);
        identityRegistry = identityRegistry_;
        reputationRegistry = IReputationRegistry(reputationRegistry_);
        escrowTimeout = escrowTimeout_;
    }

    // ── Deal Offer Management ──────────────────────────────────────────

    /// @notice Create a new deal offer. Dealer must own the ERC-8004 agent.
    ///         No USDC is locked at this stage — only when a buyer accepts.
    function createDealOffer(
        uint256 agentId,
        string calldata currency,
        uint256 rate,
        uint256 minAmount,
        uint256 maxAmount,
        string[] calldata paymentMethods
    ) external returns (uint256 offerId) {
        if (IERC721(identityRegistry).ownerOf(agentId) != msg.sender) {
            revert AgentNotOwned();
        }
        if (bytes(currency).length == 0) revert EmptyCurrency();
        if (rate == 0) revert InvalidRate();
        if (minAmount == 0 || maxAmount == 0 || minAmount > maxAmount) {
            revert InvalidAmountRange();
        }
        if (paymentMethods.length == 0) revert EmptyPaymentMethods();

        _validateCurrency(currency);

        offerId = ++_offerCount;
        DealOffer storage offer = _offers[offerId];
        offer.dealer = msg.sender;
        offer.agentId = agentId;
        offer.currency = currency;
        offer.rate = rate;
        offer.minAmount = minAmount;
        offer.maxAmount = maxAmount;
        offer.active = true;

        for (uint256 i = 0; i < paymentMethods.length; ++i) {
            offer.paymentMethods.push(paymentMethods[i]);
        }

        emit DealOfferCreated(offerId, msg.sender, agentId, currency, rate);
    }

    /// @notice Deactivate a deal offer. Only the dealer can call.
    function deactivateDealOffer(uint256 offerId) external {
        DealOffer storage offer = _getOfferStorage(offerId);
        if (offer.dealer != msg.sender) revert NotOfferDealer();
        if (!offer.active) revert OfferAlreadyInactive();
        offer.active = false;
        emit DealOfferDeactivated(offerId, msg.sender);
    }

    // ── Escrow Lifecycle ───────────────────────────────────────────────

    /// @notice Accept a deal offer. Pulls USDC from the dealer into escrow.
    ///         The dealer must have approved this contract for >= usdcAmount.
    /// @param offerId  The deal offer to accept
    /// @param usdcAmount  Amount of USDC to buy (6 decimals)
    function acceptDeal(uint256 offerId, uint256 usdcAmount)
        external
        returns (uint256 escrowId)
    {
        DealOffer storage offer = _getOfferStorage(offerId);
        if (!offer.active) revert OfferNotActive();
        if (usdcAmount < offer.minAmount) revert AmountBelowMin();
        if (usdcAmount > offer.maxAmount) revert AmountAboveMax();

        // Calculate fiat amount: usdcAmount * rate / 1e6
        uint256 fiatAmount = (usdcAmount * offer.rate) / 1e6;

        // Pull USDC from dealer into this contract
        if (!usdc.transferFrom(offer.dealer, address(this), usdcAmount)) {
            revert TransferFailed();
        }

        escrowId = ++_escrowCount;
        uint64 now_ = uint64(block.timestamp);
        uint64 expiresAt = now_ + uint64(escrowTimeout);

        _escrows[escrowId] = Escrow({
            offerId: offerId,
            buyer: msg.sender,
            dealer: offer.dealer,
            usdcAmount: usdcAmount,
            fiatAmount: fiatAmount,
            status: EscrowStatus.Funded,
            createdAt: now_,
            expiresAt: expiresAt
        });

        emit EscrowCreated(
            escrowId, offerId, msg.sender, offer.dealer,
            usdcAmount, fiatAmount, expiresAt
        );
    }

    /// @notice Buyer signals that fiat has been sent off-chain.
    function markFiatSent(uint256 escrowId) external {
        Escrow storage escrow = _getEscrowStorage(escrowId);
        if (escrow.buyer != msg.sender) revert NotBuyer();
        if (escrow.status != EscrowStatus.Funded) revert WrongStatus();
        if (block.timestamp >= escrow.expiresAt) revert AlreadyExpired();

        escrow.status = EscrowStatus.Sent;
        emit FiatMarkedSent(escrowId, msg.sender);
    }

    /// @notice Dealer confirms fiat receipt. Releases USDC to buyer.
    ///         Auto-submits positive reputation feedback for the dealer's agent.
    function confirmReceipt(uint256 escrowId) external {
        Escrow storage escrow = _getEscrowStorage(escrowId);
        if (escrow.dealer != msg.sender) revert NotDealer();
        if (escrow.status != EscrowStatus.Sent) revert WrongStatus();

        escrow.status = EscrowStatus.Completed;

        if (!usdc.transfer(escrow.buyer, escrow.usdcAmount)) {
            revert TransferFailed();
        }

        // Auto reputation: positive feedback for the dealer's agent
        uint256 agentId = _offers[escrow.offerId].agentId;
        _submitReputationFeedback(agentId, true);

        emit EscrowCompleted(escrowId, escrow.dealer, escrow.buyer, escrow.usdcAmount);
        emit ReputationFeedback(escrowId, agentId, true);
    }

    /// @notice Dealer rejects the deal before buyer marks fiat as sent.
    ///         Returns USDC to dealer.
    function rejectDeal(uint256 escrowId) external {
        Escrow storage escrow = _getEscrowStorage(escrowId);
        if (escrow.dealer != msg.sender) revert NotDealer();
        if (escrow.status != EscrowStatus.Funded) revert WrongStatus();

        escrow.status = EscrowStatus.Rejected;

        if (!usdc.transfer(escrow.dealer, escrow.usdcAmount)) {
            revert TransferFailed();
        }

        emit EscrowRejected(escrowId, escrow.dealer);
    }

    /// @notice Claim USDC after escrow timeout. Anyone can call but USDC goes to buyer.
    ///         If the dealer failed to confirm within the timeout, the buyer is protected.
    ///         Auto-submits negative reputation feedback for the dealer's agent.
    function claimExpired(uint256 escrowId) external {
        Escrow storage escrow = _getEscrowStorage(escrowId);
        if (escrow.status != EscrowStatus.Funded && escrow.status != EscrowStatus.Sent) {
            revert WrongStatus();
        }
        if (block.timestamp < escrow.expiresAt) revert NotExpired();

        escrow.status = EscrowStatus.Expired;

        if (!usdc.transfer(escrow.buyer, escrow.usdcAmount)) {
            revert TransferFailed();
        }

        // Auto reputation: negative feedback for non-responsive dealer
        uint256 agentId = _offers[escrow.offerId].agentId;
        _submitReputationFeedback(agentId, false);

        emit EscrowExpired(escrowId, escrow.buyer, escrow.usdcAmount);
        emit ReputationFeedback(escrowId, agentId, false);
    }

    // ── View Functions ─────────────────────────────────────────────────

    function getOffer(uint256 offerId) external view returns (DealOffer memory) {
        if (offerId == 0 || offerId > _offerCount) revert UnknownOffer();
        return _copyOffer(_offers[offerId]);
    }

    function getEscrow(uint256 escrowId) external view returns (Escrow memory) {
        if (escrowId == 0 || escrowId > _escrowCount) revert UnknownEscrow();
        return _escrows[escrowId];
    }

    function totalOffers() external view returns (uint256) {
        return _offerCount;
    }

    function totalEscrows() external view returns (uint256) {
        return _escrowCount;
    }

    /// @notice Query active offers for a given fiat currency.
    function getActiveDeals(string calldata currency)
        external
        view
        returns (DealOffer[] memory matches)
    {
        uint256 count = 0;
        for (uint256 i = 1; i <= _offerCount; ++i) {
            if (_offers[i].active && _currencyMatches(_offers[i].currency, currency)) {
                ++count;
            }
        }

        matches = new DealOffer[](count);
        uint256 idx = 0;
        for (uint256 i = 1; i <= _offerCount; ++i) {
            if (_offers[i].active && _currencyMatches(_offers[i].currency, currency)) {
                matches[idx++] = _copyOffer(_offers[i]);
            }
        }
    }

    // ── Internal Helpers ───────────────────────────────────────────────

    function _getOfferStorage(uint256 offerId)
        internal
        view
        returns (DealOffer storage offer)
    {
        if (offerId == 0 || offerId > _offerCount) revert UnknownOffer();
        offer = _offers[offerId];
    }

    function _getEscrowStorage(uint256 escrowId)
        internal
        view
        returns (Escrow storage escrow)
    {
        if (escrowId == 0 || escrowId > _escrowCount) revert UnknownEscrow();
        escrow = _escrows[escrowId];
    }

    function _copyOffer(DealOffer storage offer)
        internal
        view
        returns (DealOffer memory copy)
    {
        copy.dealer = offer.dealer;
        copy.agentId = offer.agentId;
        copy.currency = offer.currency;
        copy.rate = offer.rate;
        copy.minAmount = offer.minAmount;
        copy.maxAmount = offer.maxAmount;
        copy.active = offer.active;

        string[] memory methods = new string[](offer.paymentMethods.length);
        for (uint256 i = 0; i < offer.paymentMethods.length; ++i) {
            methods[i] = offer.paymentMethods[i];
        }
        copy.paymentMethods = methods;
    }

    /// @notice Currency must be 3 uppercase ASCII letters (ISO 4217).
    function _validateCurrency(string calldata currency) internal pure {
        bytes memory b = bytes(currency);
        if (b.length != 3) revert EmptyCurrency();
        for (uint256 i = 0; i < 3; ++i) {
            uint8 c = uint8(b[i]);
            if (c < 65 || c > 90) revert EmptyCurrency(); // A-Z only
        }
    }

    function _currencyMatches(string storage a, string calldata b)
        internal
        pure
        returns (bool)
    {
        return keccak256(bytes(a)) == keccak256(bytes(b));
    }

    function _submitReputationFeedback(uint256 agentId, bool positive) internal {
        int128 value = positive ? int128(1) : int128(-1);
        reputationRegistry.giveFeedback(
            agentId,
            value,
            0,                // valueDecimals
            "deal",           // tag1
            "",               // tag2
            "",               // endpoint
            "",               // feedbackURI
            bytes32(0)        // feedbackHash
        );
    }
}
