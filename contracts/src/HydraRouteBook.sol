// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {IERC20} from "./interfaces/IERC20.sol";
import {IERC721} from "./interfaces/IERC721.sol";

contract HydraRouteBook {
    struct RouteOffer {
        address provider;
        uint256 agentId;
        string endpointCiphertext;
        string[] protocols;
        string region;
        uint256 pricePerGB;
        uint256 stakeAmount;
        uint256 bandwidthMbps;
        uint64 createdAt;
        uint64 deactivatedAt;
        bool active;
    }

    error ZeroAddress();
    error InvalidRegion();
    error EmptyEndpointCiphertext();
    error EmptyProtocols();
    error EmptyProtocol();
    error ZeroStake();
    error TransferFailed();
    error AgentNotOwned();
    error UnknownOffer();
    error NotProvider();
    error OfferAlreadyInactive();
    error OfferStillActive();
    error WithdrawalNotReady();
    error StakeAlreadyWithdrawn();

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

    IERC20 public immutable usdc;
    address public immutable identityRegistry;
    address public immutable reputationRegistry;
    uint256 public immutable withdrawalDelay;

    uint256 private _offerCount;
    mapping(uint256 => RouteOffer) private _offers;

    constructor(address usdc_, address identityRegistry_, address reputationRegistry_, uint256 withdrawalDelay_) {
        if (usdc_ == address(0) || identityRegistry_ == address(0)) {
            revert ZeroAddress();
        }

        usdc = IERC20(usdc_);
        identityRegistry = identityRegistry_;
        reputationRegistry = reputationRegistry_;
        withdrawalDelay = withdrawalDelay_;
    }

    function createOffer(
        uint256 agentId,
        string calldata endpointCiphertext,
        string[] calldata protocols,
        string calldata region,
        uint256 pricePerGB,
        uint256 stakeAmount,
        uint256 bandwidthMbps
    ) external returns (uint256 offerId) {
        if (IERC721(identityRegistry).ownerOf(agentId) != msg.sender) {
            revert AgentNotOwned();
        }
        if (bytes(endpointCiphertext).length == 0) {
            revert EmptyEndpointCiphertext();
        }
        if (stakeAmount == 0) {
            revert ZeroStake();
        }

        _validateRegion(region);
        _validateProtocols(protocols);

        if (!usdc.transferFrom(msg.sender, address(this), stakeAmount)) {
            revert TransferFailed();
        }

        offerId = ++_offerCount;
        RouteOffer storage offer = _offers[offerId];
        offer.provider = msg.sender;
        offer.agentId = agentId;
        offer.endpointCiphertext = endpointCiphertext;
        offer.region = region;
        offer.pricePerGB = pricePerGB;
        offer.stakeAmount = stakeAmount;
        offer.bandwidthMbps = bandwidthMbps;
        offer.createdAt = uint64(block.timestamp);
        offer.active = true;

        for (uint256 i = 0; i < protocols.length; ++i) {
            offer.protocols.push(protocols[i]);
        }

        emit OfferCreated(offerId, msg.sender, agentId, region, pricePerGB, stakeAmount);
    }

    function deactivateOffer(uint256 offerId) external {
        RouteOffer storage offer = _getOfferStorage(offerId);
        if (offer.provider != msg.sender) {
            revert NotProvider();
        }
        if (!offer.active) {
            revert OfferAlreadyInactive();
        }

        offer.active = false;
        offer.deactivatedAt = uint64(block.timestamp);

        emit OfferDeactivated(offerId, msg.sender, offer.deactivatedAt);
    }

    function withdrawStake(uint256 offerId) external {
        RouteOffer storage offer = _getOfferStorage(offerId);
        if (offer.provider != msg.sender) {
            revert NotProvider();
        }
        if (offer.active) {
            revert OfferStillActive();
        }
        if (offer.stakeAmount == 0) {
            revert StakeAlreadyWithdrawn();
        }
        if (block.timestamp < uint256(offer.deactivatedAt) + withdrawalDelay) {
            revert WithdrawalNotReady();
        }

        uint256 amount = offer.stakeAmount;
        offer.stakeAmount = 0;

        if (!usdc.transfer(msg.sender, amount)) {
            revert TransferFailed();
        }

        emit StakeWithdrawn(offerId, msg.sender, amount);
    }

    function getOffer(uint256 offerId) external view returns (RouteOffer memory) {
        RouteOffer storage offer = _getOfferStorage(offerId);
        return _copyOffer(offer);
    }

    function totalOffers() external view returns (uint256) {
        return _offerCount;
    }

    function queryOffers(string calldata region, string calldata protocol)
        external
        view
        returns (RouteOffer[] memory offers)
    {
        _validateRegion(region);
        if (bytes(protocol).length == 0) {
            revert EmptyProtocol();
        }

        bytes32 regionHash = keccak256(bytes(region));
        bytes32 protocolHash = keccak256(bytes(protocol));
        uint256 matches;

        for (uint256 offerId = 1; offerId <= _offerCount; ++offerId) {
            RouteOffer storage offer = _offers[offerId];
            if (_matchesOffer(offer, regionHash, protocolHash)) {
                ++matches;
            }
        }

        offers = new RouteOffer[](matches);
        uint256 index;

        for (uint256 offerId = 1; offerId <= _offerCount; ++offerId) {
            RouteOffer storage offer = _offers[offerId];
            if (_matchesOffer(offer, regionHash, protocolHash)) {
                offers[index] = _copyOffer(offer);
                ++index;
            }
        }
    }

    function submitSlashClaim(uint256 offerId, bytes32 proofHash) external {
        _getOfferStorage(offerId);
        emit SlashClaimSubmitted(offerId, msg.sender, proofHash);
    }

    function _matchesOffer(RouteOffer storage offer, bytes32 regionHash, bytes32 protocolHash)
        internal
        view
        returns (bool)
    {
        if (!offer.active || keccak256(bytes(offer.region)) != regionHash) {
            return false;
        }

        for (uint256 i = 0; i < offer.protocols.length; ++i) {
            if (keccak256(bytes(offer.protocols[i])) == protocolHash) {
                return true;
            }
        }

        return false;
    }

    function _getOfferStorage(uint256 offerId) internal view returns (RouteOffer storage offer) {
        offer = _offers[offerId];
        if (offer.provider == address(0)) {
            revert UnknownOffer();
        }
    }

    function _copyOffer(RouteOffer storage offer) internal view returns (RouteOffer memory copiedOffer) {
        string[] memory protocols = new string[](offer.protocols.length);
        for (uint256 i = 0; i < offer.protocols.length; ++i) {
            protocols[i] = offer.protocols[i];
        }

        copiedOffer = RouteOffer({
            provider: offer.provider,
            agentId: offer.agentId,
            endpointCiphertext: offer.endpointCiphertext,
            protocols: protocols,
            region: offer.region,
            pricePerGB: offer.pricePerGB,
            stakeAmount: offer.stakeAmount,
            bandwidthMbps: offer.bandwidthMbps,
            createdAt: offer.createdAt,
            deactivatedAt: offer.deactivatedAt,
            active: offer.active
        });
    }

    function _validateRegion(string memory region) internal pure {
        bytes memory regionBytes = bytes(region);
        if (regionBytes.length != 2) {
            revert InvalidRegion();
        }

        for (uint256 i = 0; i < regionBytes.length; ++i) {
            bytes1 char = regionBytes[i];
            if (char < 0x41 || char > 0x5A) {
                revert InvalidRegion();
            }
        }
    }

    function _validateProtocols(string[] calldata protocols) internal pure {
        if (protocols.length == 0) {
            revert EmptyProtocols();
        }

        for (uint256 i = 0; i < protocols.length; ++i) {
            if (bytes(protocols[i]).length == 0) {
                revert EmptyProtocol();
            }
        }
    }
}
