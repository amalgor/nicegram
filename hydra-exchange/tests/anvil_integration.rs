use alloy::primitives::U256;
use alloy::providers::{ProviderBuilder, WalletProvider};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use hydra_exchange::{
    AgentRegistrar, BASE_SEPOLIA_CHAIN, BASE_SEPOLIA_CHAIN_ID, CreateOfferInput, ExchangeConfig,
    LocalWallet, ReputationClient, RouteExchangeClient,
};
use url::Url;

const ANVIL_MNEMONIC: &str = "test test test test test test test test test test test junk";
const ANVIL_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const ONE_USDC: u128 = 1_000_000;

fn anvil_path() -> String {
    std::env::var("ANVIL_BIN").unwrap_or_else(|_| {
        let home = std::env::var("HOME").expect("HOME should be set for Anvil integration tests");
        format!("{home}/.foundry/bin/anvil")
    })
}

sol!(
    #[sol(rpc)]
    MockUSDCArtifact,
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../contracts/out/MockUSDC.sol/MockUSDC.json"
    )
);

sol!(
    #[sol(rpc)]
    MockIdentityRegistryArtifact,
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../contracts/out/MockIdentityRegistry.sol/MockIdentityRegistry.json"
    )
);

sol!(
    #[sol(rpc)]
    MockReputationRegistryArtifact,
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../contracts/out/MockReputationRegistry.sol/MockReputationRegistry.json"
    )
);

sol!(
    #[sol(rpc)]
    HydraRouteBookArtifact,
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../contracts/out/HydraRouteBook.sol/HydraRouteBook.json"
    )
);

#[tokio::test]
async fn wallet_balances_and_offer_query_work_against_anvil() -> anyhow::Result<()> {
    let signer: PrivateKeySigner = ANVIL_PRIVATE_KEY.parse()?;
    let anvil = alloy::node_bindings::Anvil::new()
        .path(anvil_path())
        .spawn();
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(anvil.endpoint_url());
    let signer_address = provider.wallet().default_signer().address();

    let usdc = MockUSDCArtifact::deploy(provider.clone()).await?;
    let identity = MockIdentityRegistryArtifact::deploy(provider.clone()).await?;
    let reputation = MockReputationRegistryArtifact::deploy(provider.clone()).await?;
    let route_book = HydraRouteBookArtifact::deploy(
        provider.clone(),
        usdc.address().to_owned(),
        identity.address().to_owned(),
        reputation.address().to_owned(),
        U256::from(0_u64),
    )
    .await?;

    identity
        .setOwner(U256::from(1_u64), signer_address)
        .send()
        .await?
        .get_receipt()
        .await?;
    usdc.mint(signer_address, U256::from(5 * ONE_USDC))
        .send()
        .await?
        .get_receipt()
        .await?;
    usdc.approve(route_book.address().to_owned(), U256::from(ONE_USDC))
        .send()
        .await?
        .get_receipt()
        .await?;
    route_book
        .createOffer(
            U256::from(1_u64),
            "ciphertext://anvil-offer".to_string(),
            vec!["vless".to_string()],
            "US".to_string(),
            U256::from(ONE_USDC),
            U256::from(ONE_USDC),
            U256::from(250_u64),
        )
        .send()
        .await?
        .get_receipt()
        .await?;

    let client = RouteExchangeClient::new(ExchangeConfig {
        chain: BASE_SEPOLIA_CHAIN.to_string(),
        chain_id: BASE_SEPOLIA_CHAIN_ID,
        rpc_url: Url::parse(anvil.endpoint_url().as_str())?,
        route_book_address: route_book.address().to_owned(),
        deal_board_address: None,
        identity_registry_address: identity.address().to_owned(),
        reputation_registry_address: Some(reputation.address().to_owned()),
        usdc_address: usdc.address().to_owned(),
    });

    let balances = client.wallet_balances(signer_address).await?;
    assert_eq!(balances.usdc_balance_raw, (4 * ONE_USDC).to_string());
    assert_eq!(balances.usdc_balance, "4");
    assert!(!balances.eth_balance_wei.is_empty());

    let offers = client.query_offers("us", "VLESS").await?;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].offer_id, 1);
    assert_eq!(offers[0].region, "US");
    assert_eq!(offers[0].protocols, vec!["vless".to_string()]);
    assert_eq!(offers[0].reputation.as_ref().map(|item| item.feedback_count), Some(0));

    Ok(())
}

#[tokio::test]
async fn register_and_feedback_work_against_anvil() -> anyhow::Result<()> {
    let signer: PrivateKeySigner = ANVIL_PRIVATE_KEY.parse()?;
    let anvil = alloy::node_bindings::Anvil::new()
        .path(anvil_path())
        .spawn();
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(anvil.endpoint_url());

    let identity = MockIdentityRegistryArtifact::deploy(provider.clone()).await?;
    let reputation = MockReputationRegistryArtifact::deploy(provider.clone()).await?;
    let usdc = MockUSDCArtifact::deploy(provider.clone()).await?;
    let route_book = HydraRouteBookArtifact::deploy(
        provider.clone(),
        usdc.address().to_owned(),
        identity.address().to_owned(),
        reputation.address().to_owned(),
        U256::from(0_u64),
    )
    .await?;

    let exchange_config = ExchangeConfig {
        chain: BASE_SEPOLIA_CHAIN.to_string(),
        chain_id: BASE_SEPOLIA_CHAIN_ID,
        rpc_url: Url::parse(anvil.endpoint_url().as_str())?,
        route_book_address: route_book.address().to_owned(),
        deal_board_address: None,
        identity_registry_address: identity.address().to_owned(),
        reputation_registry_address: Some(reputation.address().to_owned()),
        usdc_address: usdc.address().to_owned(),
    };

    let wallet = LocalWallet::import(ANVIL_MNEMONIC)?;
    assert_eq!(
        wallet.address,
        format!("{:#x}", provider.wallet().default_signer().address())
    );

    let registrar = AgentRegistrar::new(exchange_config.clone());
    let registration = registrar.register(ANVIL_MNEMONIC).await?;
    assert_eq!(registration.agent_id, 1);
    assert!(registration.tx_hash.starts_with("0x"));

    let reputation_client = ReputationClient::new(exchange_config);
    let feedback = reputation_client
        .give_feedback(ANVIL_MNEMONIC, registration.agent_id, true, "availability")
        .await?;
    assert!(feedback.tx_hash.starts_with("0x"));

    let summary = reputation_client
        .summary_for_agent(registration.agent_id)
        .await?;
    assert_eq!(summary.as_ref().map(|item| item.feedback_count), Some(1));
    assert_eq!(summary.as_ref().map(|item| item.formatted_value.as_str()), Some("1"));

    Ok(())
}

#[tokio::test]
async fn create_deactivate_and_withdraw_offer_work_against_anvil() -> anyhow::Result<()> {
    let signer: PrivateKeySigner = ANVIL_PRIVATE_KEY.parse()?;
    let anvil = alloy::node_bindings::Anvil::new()
        .path(anvil_path())
        .spawn();
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(anvil.endpoint_url());
    let signer_address = provider.wallet().default_signer().address();

    let usdc = MockUSDCArtifact::deploy(provider.clone()).await?;
    let identity = MockIdentityRegistryArtifact::deploy(provider.clone()).await?;
    let reputation = MockReputationRegistryArtifact::deploy(provider.clone()).await?;
    let route_book = HydraRouteBookArtifact::deploy(
        provider.clone(),
        usdc.address().to_owned(),
        identity.address().to_owned(),
        reputation.address().to_owned(),
        U256::from(1_u64),
    )
    .await?;

    identity
        .setOwner(U256::from(7_u64), signer_address)
        .send()
        .await?
        .get_receipt()
        .await?;
    usdc.mint(signer_address, U256::from(3 * ONE_USDC))
        .send()
        .await?
        .get_receipt()
        .await?;

    let client = RouteExchangeClient::new(ExchangeConfig {
        chain: BASE_SEPOLIA_CHAIN.to_string(),
        chain_id: BASE_SEPOLIA_CHAIN_ID,
        rpc_url: Url::parse(anvil.endpoint_url().as_str())?,
        route_book_address: route_book.address().to_owned(),
        deal_board_address: None,
        identity_registry_address: identity.address().to_owned(),
        reputation_registry_address: Some(reputation.address().to_owned()),
        usdc_address: usdc.address().to_owned(),
    });

    let created = client
        .create_offer(
            ANVIL_MNEMONIC,
            CreateOfferInput {
                agent_id: 7,
                endpoint_url: "wss://relay.hydra-net.work?agent=7".to_string(),
                protocols: vec!["wss".to_string()],
                region: "US".to_string(),
                price_per_gb_raw: ONE_USDC.to_string(),
                stake_amount_raw: ONE_USDC.to_string(),
                bandwidth_mbps: 42,
            },
        )
        .await?;
    assert_eq!(created.offer_id, Some(1));
    assert!(created.tx_hash.starts_with("0x"));

    let offer = client.get_offer(1).await?;
    assert_eq!(offer.agent_id, 7);
    assert_eq!(offer.bandwidth_mbps, 42);
    assert_eq!(offer.stake_amount_raw, ONE_USDC.to_string());

    let deactivated = client.deactivate_offer(ANVIL_MNEMONIC, 1).await?;
    assert_eq!(deactivated.offer_id, Some(1));
    let offer = client.get_offer(1).await?;
    assert!(!offer.active);

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let withdrawn = client.withdraw_stake(ANVIL_MNEMONIC, 1).await?;
    assert_eq!(withdrawn.offer_id, Some(1));
    let offer = client.get_offer(1).await?;
    assert_eq!(offer.stake_amount_raw, "0");

    Ok(())
}

#[test]
fn contract_artifacts_exist_for_anvil_integration() {
    for artifact in [
        "../contracts/out/MockUSDC.sol/MockUSDC.json",
        "../contracts/out/MockIdentityRegistry.sol/MockIdentityRegistry.json",
        "../contracts/out/MockReputationRegistry.sol/MockReputationRegistry.json",
        "../contracts/out/HydraRouteBook.sol/HydraRouteBook.json",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(artifact);
        assert!(path.exists(), "missing artifact: {}", path.display());
    }
}
