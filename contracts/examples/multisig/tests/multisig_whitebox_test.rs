use std::borrow::Borrow;

use multisig::{
    multisig_perform::MultisigPerformModule, multisig_propose::MultisigProposeModule,
    user_role::UserRole, Multisig,
};
use multiversx_sc::{
    api::ManagedTypeApi,
    codec::multi_types::OptionalValue,
    types::{
        Address, BigUint, BoxedBytes, CodeMetadata, ManagedAddress, ManagedBuffer, ManagedVec,
    },
};
use multiversx_sc_scenario::{
    managed_address, rust_biguint,
    scenario_model::{Account, AddressValue, ScCallStep, ScDeployStep, SetStateStep},
    DebugApi, ScenarioWorld, WhiteboxContract,
};

const OWNER_ADDRESS_EXPR: &str = "address:owner";
const PROPOSER_ADDRESS_EXPR: &str = "address:proposer";
const BOARD_MEMBER_ADDRESS_EXPR: &str = "address:board-member";
const MULTISIG_ADDRESS_EXPR: &str = "sc:multisig";
const MULTISIG_PATH_EXPR: &str = "file:output/multisig.wasm";
const QUORUM_SIZE: usize = 1;
// const EGLD_TOKEN_ID: &[u8] = b"EGLD";

type RustBigUint = num_bigint::BigUint;

pub enum ActionRaw {
    _Nothing,
    AddBoardMember(Address),
    AddProposer(Address),
    RemoveUser(Address),
    ChangeQuorum(usize),
    SendTransferExecute(CallActionDataRaw),
    SendAsyncCall(CallActionDataRaw),
    SCDeployFromSource {
        amount: RustBigUint,
        source: Address,
        code_metadata: CodeMetadata,
        arguments: Vec<BoxedBytes>,
    },
    SCUpgradeFromSource {
        sc_address: Address,
        amount: RustBigUint,
        source: Address,
        code_metadata: CodeMetadata,
        arguments: Vec<BoxedBytes>,
    },
}

pub struct CallActionDataRaw {
    pub to: Address,
    pub egld_amount: RustBigUint,
    pub endpoint_name: BoxedBytes,
    pub arguments: Vec<BoxedBytes>,
}

fn world() -> ScenarioWorld {
    let mut blockchain = ScenarioWorld::new();
    blockchain.set_current_dir_from_workspace("contracts/examples/multisig");

    blockchain.register_contract(MULTISIG_PATH_EXPR, multisig::ContractBuilder);
    blockchain
}

fn setup() -> ScenarioWorld {
    // setup
    let mut world = world();
    let multisig_whitebox = WhiteboxContract::new(MULTISIG_ADDRESS_EXPR, multisig::contract_obj);
    let multisig_code = world.code_expression(MULTISIG_PATH_EXPR);

    world.set_state_step(
        SetStateStep::new()
            .put_account(OWNER_ADDRESS_EXPR, Account::new().nonce(1))
            .new_address(OWNER_ADDRESS_EXPR, 1, MULTISIG_ADDRESS_EXPR)
            .put_account(
                PROPOSER_ADDRESS_EXPR,
                Account::new().nonce(1).balance(100_000_000u64),
            )
            .put_account(BOARD_MEMBER_ADDRESS_EXPR, Account::new().nonce(1)),
    );

    // init multisig
    world.whitebox_deploy(
        &multisig_whitebox,
        ScDeployStep::new()
            .from(OWNER_ADDRESS_EXPR)
            .code(multisig_code),
        |sc| {
            let mut board_members = ManagedVec::new();
            board_members.push(managed_address!(&address_expr_to_address(
                BOARD_MEMBER_ADDRESS_EXPR
            )));

            sc.init(QUORUM_SIZE, board_members.into());
            sc.change_user_role(
                0,
                managed_address!(&address_expr_to_address(PROPOSER_ADDRESS_EXPR)),
                UserRole::Proposer,
            );
        },
    );

    world
}

#[test]
fn test_init() {
    setup();
}

fn call_propose(world: &mut ScenarioWorld, action: ActionRaw) -> usize {
    let egld_amount = match &action {
        ActionRaw::SendTransferExecute(call_data) => call_data.egld_amount.clone(),
        ActionRaw::SendAsyncCall(call_data) => call_data.egld_amount.clone(),
        ActionRaw::SCDeployFromSource { amount, .. } => amount.clone(),
        ActionRaw::SCUpgradeFromSource { amount, .. } => amount.clone(),
        _ => rust_biguint!(0),
    };
    let amount_bytes = egld_amount.to_bytes_be();
    let amount_rust_biguint = num_bigint::BigUint::from_bytes_be(amount_bytes.as_slice());

    let mut action_id = 0;

    let multisig_whitebox = WhiteboxContract::new(MULTISIG_ADDRESS_EXPR, multisig::contract_obj);

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new()
            .from(PROPOSER_ADDRESS_EXPR)
            .egld_value(amount_rust_biguint),
        |sc| {
            action_id = match action {
                ActionRaw::_Nothing => panic!("Invalid action"),
                ActionRaw::AddBoardMember(addr) => {
                    sc.propose_add_board_member(managed_address!(&addr))
                },
                ActionRaw::AddProposer(addr) => sc.propose_add_proposer(managed_address!(&addr)),
                ActionRaw::RemoveUser(addr) => sc.propose_remove_user(managed_address!(&addr)),
                ActionRaw::ChangeQuorum(new_size) => sc.propose_change_quorum(new_size),
                ActionRaw::SendTransferExecute(call_data) => {
                    let opt_endpoint = if call_data.endpoint_name.is_empty() {
                        OptionalValue::None
                    } else {
                        OptionalValue::Some(ManagedBuffer::new_from_bytes(
                            call_data.endpoint_name.as_slice(),
                        ))
                    };

                    sc.propose_transfer_execute(
                        managed_address!(&call_data.to),
                        BigUint::from_bytes_be(&call_data.egld_amount.to_bytes_be()),
                        opt_endpoint,
                        boxed_bytes_vec_to_managed(call_data.arguments).into(),
                    )
                },
                ActionRaw::SendAsyncCall(call_data) => {
                    let opt_endpoint = if call_data.endpoint_name.is_empty() {
                        OptionalValue::None
                    } else {
                        OptionalValue::Some(ManagedBuffer::new_from_bytes(
                            call_data.endpoint_name.as_slice(),
                        ))
                    };

                    sc.propose_async_call(
                        managed_address!(&call_data.to),
                        BigUint::from_bytes_be(&call_data.egld_amount.to_bytes_be()),
                        opt_endpoint,
                        boxed_bytes_vec_to_managed(call_data.arguments).into(),
                    )
                },
                ActionRaw::SCDeployFromSource {
                    amount,
                    source,
                    code_metadata,
                    arguments,
                } => sc.propose_sc_deploy_from_source(
                    BigUint::from_bytes_be(&amount.to_bytes_be()),
                    managed_address!(&source),
                    code_metadata,
                    boxed_bytes_vec_to_managed(arguments).into(),
                ),
                ActionRaw::SCUpgradeFromSource {
                    sc_address,
                    amount,
                    source,
                    code_metadata,
                    arguments,
                } => sc.propose_sc_upgrade_from_source(
                    managed_address!(&sc_address),
                    BigUint::from_bytes_be(&amount.to_bytes_be()),
                    managed_address!(&source),
                    code_metadata,
                    boxed_bytes_vec_to_managed(arguments).into(),
                ),
            }
        },
    );

    action_id
}

#[test]
fn test_add_board_member() {
    let mut world = setup();
    let multisig_whitebox = WhiteboxContract::new(MULTISIG_ADDRESS_EXPR, multisig::contract_obj);

    const NEW_BOARD_MEMBER_ADDRESS_EXPR: &str = "address:new-board-member";
    world.set_state_step(
        SetStateStep::new().put_account(NEW_BOARD_MEMBER_ADDRESS_EXPR, Account::new().nonce(1)),
    );

    world.whitebox_query(&multisig_whitebox, |sc| {
        // check role before
        let user_role = sc.user_role(managed_address!(&address_expr_to_address(
            NEW_BOARD_MEMBER_ADDRESS_EXPR
        )));
        assert_eq!(user_role, UserRole::None);
    });

    let action_id = call_propose(
        &mut world,
        ActionRaw::AddBoardMember(address_expr_to_address(NEW_BOARD_MEMBER_ADDRESS_EXPR)),
    );

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new().from(BOARD_MEMBER_ADDRESS_EXPR),
        |sc| sc.sign(action_id),
    );

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new().from(BOARD_MEMBER_ADDRESS_EXPR),
        |sc| {
            let _ = sc.perform_action_endpoint(action_id);
        },
    );

    world.whitebox_query(&multisig_whitebox, |sc| {
        // check role after
        let user_role = sc.user_role(managed_address!(&address_expr_to_address(
            NEW_BOARD_MEMBER_ADDRESS_EXPR
        )));
        assert_eq!(user_role, UserRole::BoardMember);

        let board_members = sc.get_all_board_members().to_vec();
        assert_eq!(
            (board_members.get(0).borrow() as &ManagedAddress<DebugApi>).clone(),
            managed_address!(&address_expr_to_address(BOARD_MEMBER_ADDRESS_EXPR))
        );
        assert_eq!(
            (board_members.get(1).borrow() as &ManagedAddress<DebugApi>).clone(),
            managed_address!(&address_expr_to_address(NEW_BOARD_MEMBER_ADDRESS_EXPR))
        );
    });
}

#[test]
fn test_add_proposer() {
    let mut world = setup();
    let multisig_whitebox = WhiteboxContract::new(MULTISIG_ADDRESS_EXPR, multisig::contract_obj);

    const NEW_PROPOSER_ADDRESS_EXPR: &str = "address:new-proposer";
    world.set_state_step(
        SetStateStep::new().put_account(NEW_PROPOSER_ADDRESS_EXPR, Account::new().nonce(1)),
    );

    world.whitebox_query(&multisig_whitebox, |sc| {
        // check role before
        let user_role = sc.user_role(managed_address!(&address_expr_to_address(
            NEW_PROPOSER_ADDRESS_EXPR
        )));
        assert_eq!(user_role, UserRole::None);
    });

    let action_id = call_propose(
        &mut world,
        ActionRaw::AddProposer(address_expr_to_address(NEW_PROPOSER_ADDRESS_EXPR)),
    );

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new().from(BOARD_MEMBER_ADDRESS_EXPR),
        |sc| sc.sign(action_id),
    );

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new().from(BOARD_MEMBER_ADDRESS_EXPR),
        |sc| {
            let _ = sc.perform_action_endpoint(action_id);
        },
    );

    world.whitebox_query(&multisig_whitebox, |sc| {
        // check role after
        let user_role = sc.user_role(managed_address!(&address_expr_to_address(
            NEW_PROPOSER_ADDRESS_EXPR
        )));
        assert_eq!(user_role, UserRole::Proposer);

        let proposers = sc.get_all_proposers().to_vec();
        assert_eq!(
            (proposers.get(0).borrow() as &ManagedAddress<DebugApi>).clone(),
            managed_address!(&address_expr_to_address(PROPOSER_ADDRESS_EXPR))
        );
        assert_eq!(
            (proposers.get(1).borrow() as &ManagedAddress<DebugApi>).clone(),
            managed_address!(&address_expr_to_address(NEW_PROPOSER_ADDRESS_EXPR))
        );
    });
}

#[test]
fn test_remove_proposer() {
    let mut world = setup();
    let multisig_whitebox = WhiteboxContract::new(MULTISIG_ADDRESS_EXPR, multisig::contract_obj);

    world.whitebox_query(&multisig_whitebox, |sc| {
        // check role before
        let user_role = sc.user_role(managed_address!(&address_expr_to_address(
            PROPOSER_ADDRESS_EXPR
        )));
        assert_eq!(user_role, UserRole::Proposer);
    });

    let action_id = call_propose(
        &mut world,
        ActionRaw::RemoveUser(address_expr_to_address(PROPOSER_ADDRESS_EXPR)),
    );

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new().from(BOARD_MEMBER_ADDRESS_EXPR),
        |sc| sc.sign(action_id),
    );

    world.whitebox_call(
        &multisig_whitebox,
        ScCallStep::new().from(BOARD_MEMBER_ADDRESS_EXPR),
        |sc| {
            let _ = sc.perform_action_endpoint(action_id);
        },
    );

    world.whitebox_query(&multisig_whitebox, |sc| {
        // check role after
        let user_role = sc.user_role(managed_address!(&address_expr_to_address(
            PROPOSER_ADDRESS_EXPR
        )));
        assert_eq!(user_role, UserRole::None);

        let proposers = sc.get_all_proposers().to_vec();
        assert!(proposers.is_empty());
    });
}

#[test]
fn test_try_remove_all_board_members() {}

#[test]
fn test_change_quorum() {}

#[test]
fn test_transfer_execute_to_user() {}

#[test]
fn test_transfer_execute_sc_all() {}

#[test]
fn test_async_call_to_sc() {}

#[test]
fn test_deploy_and_upgrade_from_source() {}

fn address_expr_to_address(address_expr: &str) -> Address {
    AddressValue::from(address_expr).to_address()
}

fn boxed_bytes_vec_to_managed<M: ManagedTypeApi>(
    raw_vec: Vec<BoxedBytes>,
) -> ManagedVec<M, ManagedBuffer<M>> {
    let mut managed = ManagedVec::new();
    for elem in raw_vec {
        managed.push(ManagedBuffer::new_from_bytes(elem.as_slice()));
    }

    managed
}
