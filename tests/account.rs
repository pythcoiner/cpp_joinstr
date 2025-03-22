pub mod utils;
use std::{sync::Once, thread::sleep, time::Duration};

use crate::utils::bootstrap_electrs;
use cpp_joinstr::account::Account;
use joinstr::{
    bip39::Mnemonic,
    miniscript::bitcoin::{Amount, Network},
};
use utils::{generate, send_to_address};

static INIT: Once = Once::new();

pub fn setup_logger() {
    INIT.call_once(|| {
        env_logger::builder()
            // Ensures output is only printed in test mode
            .is_test(true)
            .filter_level(log::LevelFilter::Info)
            .filter_module("bitcoind", log::LevelFilter::Info)
            .filter_module("bitcoincore_rpc", log::LevelFilter::Info)
            .filter_module("cpp_joinstr::account", log::LevelFilter::Debug)
            .filter_module("joinstr::electrum", log::LevelFilter::Info)
            .filter_module("simple_electrum_client::raw_client", log::LevelFilter::Info)
            .init();
    });
}

#[test]
fn simple_wallet() {
    setup_logger();
    let (url, port, _electrsd, bitcoind) = bootstrap_electrs();
    generate(&bitcoind, 100);

    sleep(Duration::from_millis(2000));

    let look_ahead = 20;

    let mnemonic = Mnemonic::generate(12).unwrap();
    let mut account = Account::new(mnemonic, Network::Regtest, url, port, None, 0, look_ahead);
    sleep(Duration::from_millis(300));

    // normal receive flow
    let addr = account.new_recv_addr();

    send_to_address(&bitcoind, &addr, Amount::from_btc(0.1).unwrap());
    generate(&bitcoind, 10);
    sleep(Duration::from_millis(600));
    let coins = account.coins();
    assert_eq!(coins.len(), 1);

    // receive at look_ahead bound
    let addr = account.recv_at(look_ahead);
    send_to_address(&bitcoind, &addr, Amount::from_btc(0.1).unwrap());
    generate(&bitcoind, 10);
    sleep(Duration::from_millis(600));
    let coins = account.coins();
    // the coin is detected
    assert_eq!(coins.len(), 2);

    let undiscovered_tip = account.recv_watch_tip() + 1;
    // receive beyond the look_ahead bound
    let addr = account.recv_at(undiscovered_tip);
    send_to_address(&bitcoind, &addr, Amount::from_btc(0.1).unwrap());
    generate(&bitcoind, 10);
    sleep(Duration::from_millis(600));
    let coins = account.coins();
    // the coin is not detected
    assert_eq!(coins.len(), 2);

    // move the watch tip forward
    account.new_recv_addr();
    let watch_tip = account.recv_watch_tip();
    log::error!("watch_tip: {watch_tip}");
    sleep(Duration::from_millis(600));

    account.new_recv_addr();
    let watch_tip = account.recv_watch_tip();
    log::error!("watch_tip: {watch_tip}");

    sleep(Duration::from_millis(600));
    let coins = account.coins();
    // now the coin is detected
    assert_eq!(coins.len(), 3);
}
