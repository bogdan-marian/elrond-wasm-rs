use crowdfunding_esdt::*;
use elrond_wasm::{
    sc_error,
    types::{Address, SCResult},
};
use elrond_wasm_debug::{
    managed_address, managed_biguint, managed_token_id, rust_biguint, testing_framework::*,
    DebugApi,
};

const WASM_PATH: &'static str = "crowdfunding-esdt/output/crowdfunding-esdt.wasm";

struct CrowdfundingSetup<CrowdfundingObjBuilder>
where
    CrowdfundingObjBuilder:
        'static + Copy + Fn() -> crowdfunding_esdt::ContractObj<DebugApi>,
{
    pub blockchain_wrapper: BlockchainStateWrapper,
    pub owner_address: Address,
    pub first_user_address: Address,
    pub second_user_address: Address,
    pub cf_wrapper:
        ContractObjWrapper<crowdfunding_esdt::ContractObj<DebugApi>, CrowdfundingObjBuilder>,
}

const CF_TOKEN_ID: &[u8] = b"CROWD-123456";
const CF_DEADLINE: u64 = 7 * 24 * 60 * 60; // 1 week in seconds

fn setup_crowdfunding<CrowdfundingObjBuilder>(
    cf_builder: CrowdfundingObjBuilder,
) -> CrowdfundingSetup<CrowdfundingObjBuilder>
where
    CrowdfundingObjBuilder:
        'static + Copy + Fn() -> crowdfunding_esdt::ContractObj<DebugApi>,
{
    let rust_zero = rust_biguint!(0u64);
    let mut blockchain_wrapper = BlockchainStateWrapper::new();
    let owner_address = blockchain_wrapper.create_user_account(&rust_zero);
    let first_user_address = blockchain_wrapper.create_user_account(&rust_zero);
    let second_user_address = blockchain_wrapper.create_user_account(&rust_zero);
    let cf_wrapper = blockchain_wrapper.create_sc_account(
        &rust_zero,
        Some(&owner_address),
        cf_builder,
        WASM_PATH,
    );

    blockchain_wrapper.set_esdt_balance(&first_user_address, CF_TOKEN_ID, &rust_biguint!(1_000));
    blockchain_wrapper.set_esdt_balance(&second_user_address, CF_TOKEN_ID, &rust_biguint!(1_000));

    blockchain_wrapper.execute_tx(&owner_address, &cf_wrapper, &rust_zero, |sc| {
        let target = managed_biguint!(2_000);
        let token_id = managed_token_id!(CF_TOKEN_ID);

        let result = sc.init(target, CF_DEADLINE, token_id);
        assert_eq!(result, SCResult::Ok(()));

        StateChange::Commit
    });
    blockchain_wrapper.add_mandos_set_account(cf_wrapper.address_ref());

    CrowdfundingSetup {
        blockchain_wrapper,
        owner_address,
        first_user_address,
        second_user_address,
        cf_wrapper,
    }
}

#[test]
fn init_test() {
    let cf_setup = setup_crowdfunding(crowdfunding_esdt::contract_obj);
    cf_setup
        .blockchain_wrapper
        .write_mandos_output("_generated_init.scen.json");
}

