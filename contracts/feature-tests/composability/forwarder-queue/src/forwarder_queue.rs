#![no_std]
#![allow(clippy::type_complexity)]

use multiversx_sc::api::VMApi;

multiversx_sc::imports!();
multiversx_sc::derive_imports!();

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, TypeAbi, Clone)]
pub enum QueuedCallType {
    Sync,
    LegacyAsync,
    TransferExecute,
    Promise,
}

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, TypeAbi, Clone)]
pub struct QueuedCall<M: ManagedTypeApi> {
    pub call_type: QueuedCallType,
    pub to: ManagedAddress<M>,
    pub gas_limit: u64,
    pub endpoint_name: ManagedBuffer<M>,
    pub args: ManagedArgBuffer<M>,
    pub payments: EgldOrMultiEsdtPayment<M>,
}

/// Testing multiple calls per transaction.
#[multiversx_sc::contract]
pub trait ForwarderQueue {
    #[init]
    fn init(&self) {}

    #[view]
    #[storage_mapper("queued_calls")]
    fn queued_calls(&self) -> LinkedListMapper<QueuedCall<Self::Api>>;

    #[endpoint]
    #[payable("*")]
    fn add_queued_call_sync(
        &self,
        to: ManagedAddress,
        endpoint_name: ManagedBuffer,
        args: MultiValueEncoded<ManagedBuffer>,
    ) {
        self.add_queued_call(QueuedCallType::Sync, to, 0, endpoint_name, args);
    }

    #[endpoint]
    #[payable("*")]
    fn add_queued_call_legacy_async(
        &self,
        to: ManagedAddress,
        endpoint_name: ManagedBuffer,
        args: MultiValueEncoded<ManagedBuffer>,
    ) {
        self.add_queued_call(QueuedCallType::LegacyAsync, to, 0, endpoint_name, args);
    }

    #[endpoint]
    #[payable("*")]
    fn add_queued_call_transfer_execute(
        &self,
        to: ManagedAddress,
        gas_limit: u64,
        endpoint_name: ManagedBuffer,
        args: MultiValueEncoded<ManagedBuffer>,
    ) {
        self.add_queued_call(
            QueuedCallType::TransferExecute,
            to,
            gas_limit,
            endpoint_name,
            args,
        );
    }

    #[endpoint]
    #[payable("*")]
    fn add_queued_call_promise(
        &self,
        to: ManagedAddress,
        gas_limit: u64,
        endpoint_name: ManagedBuffer,
        args: MultiValueEncoded<ManagedBuffer>,
    ) {
        self.add_queued_call(QueuedCallType::Promise, to, gas_limit, endpoint_name, args);
    }

    #[endpoint]
    #[payable("*")]
    fn add_queued_call(
        &self,
        call_type: QueuedCallType,
        to: ManagedAddress,
        gas_limit: u64,
        endpoint_name: ManagedBuffer,
        args: MultiValueEncoded<ManagedBuffer>,
    ) {
        let payments = self.call_value().any_payment();

        match &payments {
            EgldOrMultiEsdtPayment::Egld(egld_value) => {
                self.add_queued_call_egld_event(&call_type, &to, &endpoint_name, egld_value);
            },
            EgldOrMultiEsdtPayment::MultiEsdt(esdt_payments) => {
                self.add_queued_call_esdt_event(
                    &call_type,
                    &to,
                    &endpoint_name,
                    &esdt_payments.clone().into_multi_value(),
                );
            },
        }

        self.queued_calls().push_back(QueuedCall {
            call_type,
            to,
            gas_limit,
            endpoint_name,
            args: args.to_arg_buffer(),
            payments,
        });
    }

    #[callback]
    fn callback_function(&self) {
        self.forward_queued_callback_event();
    }

    #[endpoint]
    fn forward_queued_calls(&self) {
        while let Some(node) = self.queued_calls().pop_front() {
            let call = node.clone().into_value();

            let contract_call = match call.payments {
                EgldOrMultiEsdtPayment::Egld(egld_value) => {
                    self.forward_queued_call_egld_event(
                        &call.call_type,
                        &call.to,
                        &call.endpoint_name,
                        &egld_value,
                    );

                    ContractCallWithEgld::<Self::Api, ()>::new(
                        call.to.clone(),
                        call.endpoint_name.clone(),
                        egld_value,
                    )
                    .with_raw_arguments(call.args)
                },
                EgldOrMultiEsdtPayment::MultiEsdt(esdt_payments) => {
                    self.forward_queued_call_esdt_event(
                        &call.call_type,
                        &call.to,
                        &call.endpoint_name,
                        &esdt_payments.clone().into_multi_value(),
                    );

                    ContractCallWithMultiEsdt::<Self::Api, ()>::new(
                        call.to.clone(),
                        call.endpoint_name.clone(),
                        esdt_payments,
                    )
                    .with_raw_arguments(call.args)
                    .into_normalized()
                },
            };

            match call.call_type {
                QueuedCallType::Sync => {
                    contract_call.execute_on_dest_context::<()>();
                },
                QueuedCallType::LegacyAsync => {
                    contract_call.async_call().call_and_exit();
                },
                QueuedCallType::TransferExecute => {
                    contract_call
                        .with_gas_limit(call.gas_limit)
                        .transfer_execute();
                },
                QueuedCallType::Promise => {
                    #[cfg(feature = "promises")]
                    contract_call
                        .with_gas_limit(call.gas_limit)
                        .async_call_promise()
                        .with_callback(self.callbacks().callback_function())
                        .register_promise();

                    #[cfg(not(feature = "promises"))]
                    call_promise(contract_call.with_gas_limit(call.gas_limit));
                },
            }
        }
    }

    #[event("forward_queued_callback")]
    fn forward_queued_callback_event(&self);

    #[event("forward_queued_call_egld")]
    fn forward_queued_call_egld_event(
        &self,
        #[indexed] call_type: &QueuedCallType,
        #[indexed] to: &ManagedAddress,
        #[indexed] endpoint_name: &ManagedBuffer,
        #[indexed] egld_value: &BigUint,
    );

    #[event("forward_queued_call_esdt")]
    fn forward_queued_call_esdt_event(
        &self,
        #[indexed] call_type: &QueuedCallType,
        #[indexed] to: &ManagedAddress,
        #[indexed] endpoint_name: &ManagedBuffer,
        #[indexed] multi_esdt: &MultiValueEncoded<EsdtTokenPaymentMultiValue>,
    );

    #[event("add_queued_call_egld")]
    fn add_queued_call_egld_event(
        &self,
        #[indexed] call_type: &QueuedCallType,
        #[indexed] to: &ManagedAddress,
        #[indexed] endpoint_name: &ManagedBuffer,
        #[indexed] egld_value: &BigUint,
    );

    #[event("add_queued_call_esdt")]
    fn add_queued_call_esdt_event(
        &self,
        #[indexed] call_type: &QueuedCallType,
        #[indexed] to: &ManagedAddress,
        #[indexed] endpoint_name: &ManagedBuffer,
        #[indexed] multi_esdt: &MultiValueEncoded<EsdtTokenPaymentMultiValue>,
    );
}

#[cfg(feature = "promises")]
fn call_promise<A: VMApi>(
    contract_call: ContractCallWithEgld<A, ()>,
    callback_function: CallbackClosure<A>,
) {
    contract_call
        .async_call_promise()
        .with_callback(callback_function)
        .register_promise();
}

#[cfg(not(feature = "promises"))]
fn call_promise<A: VMApi>(_contract_call: ContractCallWithEgld<A, ()>) {}
