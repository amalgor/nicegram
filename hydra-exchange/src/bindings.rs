use alloy::sol;

sol!(
    #[sol(rpc)]
    IdentityRegistry,
    concat!(env!("CARGO_MANIFEST_DIR"), "/abi/IdentityRegistry.json")
);

sol!(
    #[sol(rpc)]
    ReputationRegistry,
    concat!(env!("CARGO_MANIFEST_DIR"), "/abi/ReputationRegistry.json")
);

sol! {
    #[sol(rpc)]
    contract HydraRouteBook {
        event OfferCreated(
            uint256 indexed offerId,
            address indexed provider,
            uint256 indexed agentId,
            string region,
            uint256 pricePerGB,
            uint256 stakeAmount
        );

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

        function createOffer(
            uint256 agentId,
            string endpointCiphertext,
            string[] protocols,
            string region,
            uint256 pricePerGB,
            uint256 stakeAmount,
            uint256 bandwidthMbps
        ) external returns (uint256 offerId);
        function deactivateOffer(uint256 offerId) external;
        function getOffer(uint256 offerId) external view returns (RouteOffer memory);
        function totalOffers() external view returns (uint256);
        function withdrawStake(uint256 offerId) external;
        function withdrawalDelay() external view returns (uint256);
    }

    #[sol(rpc)]
    contract HydraDealBoard {
        event DealOfferCreated(
            uint256 indexed offerId,
            address indexed dealer,
            uint256 indexed agentId,
            string currency,
            uint256 rate
        );

        event EscrowCreated(
            uint256 indexed escrowId,
            uint256 indexed offerId,
            address indexed buyer,
            address dealer,
            uint256 usdcAmount,
            uint256 fiatAmount,
            uint64 expiresAt
        );

        struct DealOffer {
            address dealer;
            uint256 agentId;
            string currency;
            uint256 rate;
            uint256 minAmount;
            uint256 maxAmount;
            string[] paymentMethods;
            bool active;
        }

        struct Escrow {
            uint256 offerId;
            address buyer;
            address dealer;
            uint256 usdcAmount;
            uint256 fiatAmount;
            uint8 status;
            uint64 createdAt;
            uint64 expiresAt;
        }

        function createDealOffer(
            uint256 agentId,
            string currency,
            uint256 rate,
            uint256 minAmount,
            uint256 maxAmount,
            string[] paymentMethods
        ) external returns (uint256 offerId);
        function deactivateDealOffer(uint256 offerId) external;
        function acceptDeal(uint256 offerId, uint256 usdcAmount) external returns (uint256 escrowId);
        function markFiatSent(uint256 escrowId) external;
        function confirmReceipt(uint256 escrowId) external;
        function rejectDeal(uint256 escrowId) external;
        function claimExpired(uint256 escrowId) external;
        function getOffer(uint256 offerId) external view returns (DealOffer memory);
        function getEscrow(uint256 escrowId) external view returns (Escrow memory);
        function totalOffers() external view returns (uint256);
        function totalEscrows() external view returns (uint256);
        function getActiveDeals(string currency) external view returns (DealOffer[] memory);
        function escrowTimeout() external view returns (uint256);
    }

    #[sol(rpc)]
    contract UsdcToken {
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{primitives::keccak256, sol_types::SolCall};

    #[test]
    fn selectors_match_expected_signatures() {
        assert_eq!(
            IdentityRegistry::register_0Call::SELECTOR,
            keccak256("register()".as_bytes())[0..4]
        );
        assert_eq!(
            IdentityRegistry::register_1Call::SELECTOR,
            keccak256("register(string,(string,bytes)[])".as_bytes())[0..4]
        );
        assert_eq!(
            IdentityRegistry::register_2Call::SELECTOR,
            keccak256("register(string)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraRouteBook::createOfferCall::SELECTOR,
            keccak256(
                "createOffer(uint256,string,string[],string,uint256,uint256,uint256)".as_bytes()
            )[0..4]
        );
        assert_eq!(
            HydraRouteBook::deactivateOfferCall::SELECTOR,
            keccak256("deactivateOffer(uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraRouteBook::getOfferCall::SELECTOR,
            keccak256("getOffer(uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraRouteBook::totalOffersCall::SELECTOR,
            keccak256("totalOffers()".as_bytes())[0..4]
        );
        assert_eq!(
            HydraRouteBook::withdrawStakeCall::SELECTOR,
            keccak256("withdrawStake(uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraRouteBook::withdrawalDelayCall::SELECTOR,
            keccak256("withdrawalDelay()".as_bytes())[0..4]
        );
        assert_eq!(
            UsdcToken::allowanceCall::SELECTOR,
            keccak256("allowance(address,address)".as_bytes())[0..4]
        );
        assert_eq!(
            UsdcToken::approveCall::SELECTOR,
            keccak256("approve(address,uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            ReputationRegistry::giveFeedbackCall::SELECTOR,
            keccak256(
                "giveFeedback(uint256,int128,uint8,string,string,string,string,bytes32)".as_bytes()
            )[0..4]
        );
        assert_eq!(
            HydraDealBoard::createDealOfferCall::SELECTOR,
            keccak256(
                "createDealOffer(uint256,string,uint256,uint256,uint256,string[])".as_bytes()
            )[0..4]
        );
        assert_eq!(
            HydraDealBoard::acceptDealCall::SELECTOR,
            keccak256("acceptDeal(uint256,uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraDealBoard::markFiatSentCall::SELECTOR,
            keccak256("markFiatSent(uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraDealBoard::confirmReceiptCall::SELECTOR,
            keccak256("confirmReceipt(uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraDealBoard::getActiveDealsCall::SELECTOR,
            keccak256("getActiveDeals(string)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraDealBoard::escrowTimeoutCall::SELECTOR,
            keccak256("escrowTimeout()".as_bytes())[0..4]
        );
    }
}
