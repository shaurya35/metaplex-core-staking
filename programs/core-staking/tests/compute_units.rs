//! Prints compute units per instruction (`cargo test --test compute_units -- --nocapture`).

mod common;

use common::*;
use solana_signer::Signer;

#[test]
fn report_compute_units() {
    let mut env = Env::new_at(DAY0 + 16 * HOUR + 59 * 60);
    let alice = env.user();
    let bob = env.user();
    let cu = |r: TxResult| r.expect("tx").compute_units_consumed;

    let a1 = env.mint_nft(&alice.pubkey());
    let a2 = env.mint_nft(&alice.pubkey());
    let a3 = env.mint_nft(&alice.pubkey());
    let mint = cu(env.try_mint_nft(
        &env.admin.insecure_clone(),
        &alice.pubkey(),
        &solana_keypair::Keypair::new(),
    ));
    let transfer = cu(env.transfer(&alice, &a3, &bob.pubkey()));
    let stake = cu(env.stake(&alice, &a1));
    env.stake(&alice, &a2).unwrap();
    env.warp_to(DAY0 + 17 * HOUR + 60);
    let crank = cu(env.crank(&bob));
    env.warp_to(env.now() + DAY);
    let claim = cu(env.claim(&alice, &a1));
    env.warp_to(env.now() + FREEZE_PERIOD);
    let unstake = cu(env.unstake(&alice, &a1));
    let burn = cu(env.burn(&alice, &a2));

    println!("| Instruction | Compute units |");
    println!("|---|---|");
    for (name, v) in [
        ("mint_nft", mint),
        ("stake", stake),
        ("claim_rewards (creates ATA)", claim),
        ("unstake", unstake),
        ("burn_staked_nft", burn),
        ("update_oracle (paid crank)", crank),
        ("transfer_nft (oracle check)", transfer),
    ] {
        println!("| `{name}` | {v} |");
    }
}
