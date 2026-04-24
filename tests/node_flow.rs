use blindeye::node::Node;
use blindeye::protocol::EmissionSchedule;
use blindeye::wallet::Wallet;

#[test]
fn mines_pending_transaction_into_a_block() {
    let node = Node::new(None);
    let sender = Wallet::new();
    let miner = Wallet::new();
    let recipient = Wallet::new();

    node.fund_address_for_testing(&sender.address, 10_000)
        .expect("fund sender");

    let transaction = {
        let blockchain = node.blockchain.lock().expect("lock blockchain");
        sender
            .build_transaction(&blockchain, &recipient.address, 4_000, 500)
            .expect("build transaction")
    };

    node.submit_transaction(transaction)
        .expect("submit to mempool");
    assert_eq!(node.get_status().mempool_size, 1);

    let block = node
        .mine_one_block(&miner.address_bytes())
        .expect("mine block");

    assert_eq!(block.header.height, 1);
    let expected_reward = EmissionSchedule::default().block_reward(1) + 500;
    assert_eq!(block.transactions[0].total_output_value(), expected_reward);
    assert_eq!(node.get_status().mempool_size, 0);
    assert_eq!(node.get_best_height(), 1);
}

#[test]
fn rejects_transactions_below_the_mempool_fee_floor() {
    let node = Node::new(None);
    let sender = Wallet::new();
    let recipient = Wallet::new();

    node.fund_address_for_testing(&sender.address, 10_000)
        .expect("fund sender");

    let zero_fee_transaction = {
        let blockchain = node.blockchain.lock().expect("lock blockchain");
        sender
            .build_transaction(&blockchain, &recipient.address, 4_000, 0)
            .expect("build transaction")
    };

    let err = node
        .submit_transaction(zero_fee_transaction)
        .expect_err("zero-fee transaction should be rejected");
    assert!(err.contains("fee is too low"));
}
