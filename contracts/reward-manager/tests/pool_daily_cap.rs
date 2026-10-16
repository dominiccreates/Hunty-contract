//! Test that a new reward pool without explicit daily cap setup can pay a winner (#1074).
//!
//! A pool cap of 0 or unset must mean unlimited distributions, so new pools
//! can pay winners immediately without requiring the admin to call `set_daily_pool_cap`.

use reward_manager::storage::Storage;
use reward_manager::types::{DistributionMode, RewardConfig, RewardPoolConfig};
use reward_manager::RewardManager;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, Env, Vec};

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    creator: Address,
    distributor: Address,
    player: Address,
    token_address: Address,
    hunt_id: u64,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.ledger().set_timestamp(1_700_000_000);
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let distributor = Address::generate(&env);
    let player = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let hunty_core = Address::generate(&env);

    let token_contract = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract.address();

    let contract_id = env.register(
        RewardManager,
        (admin.clone(), token_address.clone(), hunty_core),
    );

    let hunt_id = 42u64;

    env.as_contract(&contract_id, || {
        Storage::set_admin(&env, &admin);
        Storage::add_authorized_contract(&env, &distributor);

        // Seed a newly created pool with NO explicit daily pool cap set
        Storage::set_pool_config(
            &env,
            hunt_id,
            &RewardPoolConfig {
                creator: creator.clone(),
                delegates: Vec::new(&env),
                min_distribution_amount: 100,
                time_based_tiers: Vec::new(&env),
                frozen: false,
                token_address: token_address.clone(),
                nft_contract: None,
                target_amount: 10_000,
                min_distribution_interval_secs: 0,
                distribution_mode: DistributionMode::Fixed,
                vesting_period_secs: 0,
                claim_deadline: 0,
                nft_royalty_bps: 0,
                nft_transferable: true,
                rank_based_tiers: Vec::new(&env),
                frozen_by: None,
            },
        );

        // Fund the pool balance
        Storage::set_pool_balance(&env, hunt_id, 10_000);
        Storage::set_pool_total_deposited(&env, hunt_id, 10_000);
    });

    // Mint tokens to the contract so it can distribute
    let sac = token::StellarAssetClient::new(&env, &token_address);
    sac.mint(&contract_id, &10_000);

    Fixture {
        env,
        contract_id,
        admin,
        creator,
        distributor,
        player,
        token_address,
        hunt_id,
    }
}

#[test]
fn new_pool_pays_winner_without_admin_cap_setup() {
    let fx = setup();

    // Verify no cap was ever set for this pool (defaults to 0, which means unlimited)
    let initial_cap = fx.env.as_contract(&fx.contract_id, || {
        Storage::get_daily_pool_cap(&fx.env, fx.hunt_id)
    });
    assert_eq!(initial_cap, 0);

    let token_client = token::Client::new(&fx.env, &fx.token_address);
    assert_eq!(token_client.balance(&fx.player), 0);

    let reward_config = RewardConfig {
        xlm_amount: Some(1_000),
        nft_contract: None,
        nft_token_id: None,
        nft_recipient: None,
        nft_title: None,
        nft_uri: None,
        nft_rarity: 0,
        completion_rank: 0,
    };

    // Single-player distribution path
    let result = fx.env.as_contract(&fx.contract_id, || {
        RewardManager::distribute_rewards(
            fx.env.clone(),
            fx.distributor.clone(),
            fx.hunt_id,
            fx.player.clone(),
            reward_config,
        )
    });

    assert!(
        result.is_ok(),
        "Distribution must succeed without admin cap setup"
    );
    assert_eq!(token_client.balance(&fx.player), 1_000);

    let remaining_balance = fx.env.as_contract(&fx.contract_id, || {
        Storage::get_pool_balance(&fx.env, fx.hunt_id)
    });
    assert_eq!(remaining_balance, 9_000);
}

#[test]
fn explicit_daily_cap_is_still_enforced_when_positive() {
    let fx = setup();

    // Explicitly set cap to 500
    fx.env.as_contract(&fx.contract_id, || {
        RewardManager::set_daily_pool_cap(fx.env.clone(), fx.admin.clone(), fx.hunt_id, 500)
            .unwrap();
    });

    let reward_config = RewardConfig {
        xlm_amount: Some(1_000),
        nft_contract: None,
        nft_token_id: None,
        nft_recipient: None,
        nft_title: None,
        nft_uri: None,
        nft_rarity: 0,
        completion_rank: 0,
    };

    // Attempting to distribute 1000 with a cap of 500 must fail
    let result = fx.env.as_contract(&fx.contract_id, || {
        RewardManager::distribute_rewards(
            fx.env.clone(),
            fx.distributor.clone(),
            fx.hunt_id,
            fx.player.clone(),
            reward_config,
        )
    });

    assert_eq!(
        result,
        Err(reward_manager::RewardErrorCode::DailyCapExceeded),
        "Positive cap must still enforce daily limit"
    );
}
