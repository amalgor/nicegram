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

        function getOffer(uint256 offerId) external view returns (RouteOffer memory);
        function totalOffers() external view returns (uint256);
    }

    #[sol(rpc)]
    contract UsdcToken {
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
            HydraRouteBook::getOfferCall::SELECTOR,
            keccak256("getOffer(uint256)".as_bytes())[0..4]
        );
        assert_eq!(
            HydraRouteBook::totalOffersCall::SELECTOR,
            keccak256("totalOffers()".as_bytes())[0..4]
        );
        assert_eq!(
            ReputationRegistry::giveFeedbackCall::SELECTOR,
            keccak256(
                "giveFeedback(uint256,int128,uint8,string,string,string,string,bytes32)"
                    .as_bytes()
            )[0..4]
        );
    }
}
