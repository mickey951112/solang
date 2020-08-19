use codegen::cfg::HashTy;
use parser::pt;
use sema::ast;
use std::cell::RefCell;
use std::str;

use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::types::IntType;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;

use super::ethabiencoder;
use super::{Contract, TargetRuntime, Variable};

pub struct EwasmTarget {
    abi: ethabiencoder::EthAbiEncoder,
}

impl EwasmTarget {
    pub fn build<'a>(
        context: &'a Context,
        contract: &'a ast::Contract,
        ns: &'a ast::Namespace,
        filename: &'a str,
        opt: OptimizationLevel,
    ) -> Contract<'a> {
        // first emit runtime code
        let mut runtime_code = Contract::new(context, contract, ns, filename, opt, None);
        let mut b = EwasmTarget {
            abi: ethabiencoder::EthAbiEncoder {},
        };

        // externals
        b.declare_externals(&mut runtime_code);

        // This also emits the constructors. We are relying on DCE to eliminate them from
        // the final code.
        runtime_code.emit_functions(&mut b);

        b.emit_function_dispatch(&runtime_code);

        runtime_code.internalize(&["main"]);

        let runtime_bs = runtime_code.wasm(true).unwrap();

        // Now we have the runtime code, create the deployer
        let mut deploy_code = Contract::new(
            context,
            contract,
            ns,
            filename,
            opt,
            Some(Box::new(runtime_code)),
        );
        let mut b = EwasmTarget {
            abi: ethabiencoder::EthAbiEncoder {},
        };

        // externals
        b.declare_externals(&mut deploy_code);

        // FIXME: this emits the constructors, as well as the functions. In Ethereum Solidity,
        // no functions can be called from the constructor. We should either disallow this too
        // and not emit functions, or use lto linking to optimize any unused functions away.
        deploy_code.emit_functions(&mut b);

        b.deployer_dispatch(&mut deploy_code, &runtime_bs);

        deploy_code.internalize(&[
            "main",
            "getCallDataSize",
            "callDataCopy",
            "storageStore",
            "storageLoad",
            "finish",
            "revert",
            "codeCopy",
            "getCodeSize",
            "printMem",
            "call",
            "staticcall",
            "delegatecall",
            "create",
            "getReturnDataSize",
            "returnDataCopy",
            "getCallValue",
            "getAddress",
            "getExternalBalance",
            "getBlockHash",
            "getBlockDifficulty",
            "getGasLeft",
            "getBlockGasLimit",
            "getBlockTimestamp",
            "getBlockNumber",
            "getTxGasPrice",
            "getTxOrigin",
            "getBlockCoinbase",
            "getCaller",
        ]);

        deploy_code
    }

    fn runtime_prelude<'a>(
        &self,
        contract: &Contract<'a>,
        function: FunctionValue,
    ) -> (PointerValue<'a>, IntValue<'a>) {
        let entry = contract.context.append_basic_block(function, "entry");

        contract.builder.position_at_end(entry);

        // first thing to do is abort value transfers if we're not payable
        if contract.function_abort_value_transfers {
            contract.abort_if_value_transfer(self, function);
        }

        // init our heap
        contract.builder.build_call(
            contract.module.get_function("__init_heap").unwrap(),
            &[],
            "",
        );

        // copy arguments from scratch buffer
        let args_length = contract
            .builder
            .build_call(
                contract.module.get_function("getCallDataSize").unwrap(),
                &[],
                "calldatasize",
            )
            .try_as_basic_value()
            .left()
            .unwrap();

        contract.builder.build_store(
            contract.calldata_len.as_pointer_value(),
            args_length.into_int_value(),
        );

        let args = contract
            .builder
            .build_call(
                contract.module.get_function("__malloc").unwrap(),
                &[args_length],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        contract
            .builder
            .build_store(contract.calldata_data.as_pointer_value(), args);

        contract.builder.build_call(
            contract.module.get_function("callDataCopy").unwrap(),
            &[
                args.into(),
                contract.context.i32_type().const_zero().into(),
                args_length,
            ],
            "",
        );

        let args = contract.builder.build_pointer_cast(
            args,
            contract.context.i32_type().ptr_type(AddressSpace::Generic),
            "",
        );

        (args, args_length.into_int_value())
    }

    fn deployer_prelude<'a>(
        &self,
        contract: &mut Contract<'a>,
        function: FunctionValue,
    ) -> (PointerValue<'a>, IntValue<'a>) {
        let entry = contract.context.append_basic_block(function, "entry");

        contract.builder.position_at_end(entry);

        // first thing to do is abort value transfers if constructors not payable
        if contract.constructor_abort_value_transfers {
            contract.abort_if_value_transfer(self, function);
        }

        // init our heap
        contract.builder.build_call(
            contract.module.get_function("__init_heap").unwrap(),
            &[],
            "",
        );

        // The code_size will need to be patched later
        let code_size = contract.context.i32_type().const_int(0x4000, false);

        // copy arguments from scratch buffer
        let args_length = contract.builder.build_int_sub(
            contract
                .builder
                .build_call(
                    contract.module.get_function("getCodeSize").unwrap(),
                    &[],
                    "codesize",
                )
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value(),
            code_size,
            "",
        );

        contract
            .builder
            .build_store(contract.calldata_len.as_pointer_value(), args_length);

        let args = contract
            .builder
            .build_call(
                contract.module.get_function("__malloc").unwrap(),
                &[args_length.into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        contract
            .builder
            .build_store(contract.calldata_data.as_pointer_value(), args);

        contract.builder.build_call(
            contract.module.get_function("codeCopy").unwrap(),
            &[args.into(), code_size.into(), args_length.into()],
            "",
        );

        let args = contract.builder.build_pointer_cast(
            args,
            contract.context.i32_type().ptr_type(AddressSpace::Generic),
            "",
        );

        contract.code_size = RefCell::new(Some(code_size));

        (args, args_length)
    }

    fn declare_externals(&self, contract: &mut Contract) {
        let u8_ptr_ty = contract.context.i8_type().ptr_type(AddressSpace::Generic);
        let u32_ty = contract.context.i32_type();
        let u64_ty = contract.context.i64_type();
        let void_ty = contract.context.void_type();

        let ftype = void_ty.fn_type(&[u8_ptr_ty.into(), u8_ptr_ty.into()], false);

        contract
            .module
            .add_function("storageStore", ftype, Some(Linkage::External));
        contract
            .module
            .add_function("storageLoad", ftype, Some(Linkage::External));

        contract.module.add_function(
            "getCallDataSize",
            u32_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getCodeSize",
            u32_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getReturnDataSize",
            u32_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "callDataCopy",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // resultOffset
                    u32_ty.into(),    // dataOffset
                    u32_ty.into(),    // length
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "codeCopy",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // resultOffset
                    u32_ty.into(),    // dataOffset
                    u32_ty.into(),    // length
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "returnDataCopy",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // resultOffset
                    u32_ty.into(),    // dataOffset
                    u32_ty.into(),    // length
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "printMem",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // string_ptr
                    u32_ty.into(),    // string_length
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "create",
            u32_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // valueOffset
                    u8_ptr_ty.into(), // input offset
                    u32_ty.into(),    // input length
                    u8_ptr_ty.into(), // address result
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "call",
            u32_ty.fn_type(
                &[
                    u64_ty.into(),    // gas
                    u8_ptr_ty.into(), // address
                    u8_ptr_ty.into(), // valueOffset
                    u8_ptr_ty.into(), // input offset
                    u32_ty.into(),    // input length
                ],
                false,
            ),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "staticcall",
            u32_ty.fn_type(
                &[
                    u64_ty.into(),    // gas
                    u8_ptr_ty.into(), // address
                    u8_ptr_ty.into(), // valueOffset
                    u8_ptr_ty.into(), // input offset
                    u32_ty.into(),    // input length
                ],
                false,
            ),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "delegatecall",
            u32_ty.fn_type(
                &[
                    u64_ty.into(),    // gas
                    u8_ptr_ty.into(), // address
                    u8_ptr_ty.into(), // valueOffset
                    u8_ptr_ty.into(), // input offset
                    u32_ty.into(),    // input length
                ],
                false,
            ),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "getCallValue",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // value_ptr
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getAddress",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // value_ptr
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getCaller",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // value_ptr
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getExternalBalance",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // address_ptr
                    u8_ptr_ty.into(), // balance_ptr
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getBlockHash",
            u32_ty.fn_type(
                &[
                    u64_ty.into(),    // block number
                    u8_ptr_ty.into(), // hash_ptr result
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getBlockCoinbase",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // address_ptr result
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getBlockDifficulty",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // u256_ptr result
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getGasLeft",
            u64_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getBlockGasLimit",
            u64_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getBlockTimestamp",
            u64_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getBlockNumber",
            u64_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getTxGasPrice",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // value_ptr result
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "getTxOrigin",
            void_ty.fn_type(
                &[
                    u8_ptr_ty.into(), // address_ptr result
                ],
                false,
            ),
            Some(Linkage::External),
        );

        let noreturn = contract
            .context
            .create_enum_attribute(Attribute::get_named_enum_kind_id("noreturn"), 0);

        // mark as noreturn
        contract
            .module
            .add_function(
                "finish",
                void_ty.fn_type(
                    &[
                        u8_ptr_ty.into(), // data_ptr
                        u32_ty.into(),    // data_len
                    ],
                    false,
                ),
                Some(Linkage::External),
            )
            .add_attribute(AttributeLoc::Function, noreturn);

        // mark as noreturn
        contract
            .module
            .add_function(
                "revert",
                void_ty.fn_type(
                    &[
                        u8_ptr_ty.into(), // data_ptr
                        u32_ty.into(),    // data_len
                    ],
                    false,
                ),
                Some(Linkage::External),
            )
            .add_attribute(AttributeLoc::Function, noreturn);

        // mark as noreturn
        contract
            .module
            .add_function(
                "selfDestruct",
                void_ty.fn_type(
                    &[
                        u8_ptr_ty.into(), // address_ptr
                    ],
                    false,
                ),
                Some(Linkage::External),
            )
            .add_attribute(AttributeLoc::Function, noreturn);
    }

    fn deployer_dispatch(&mut self, contract: &mut Contract, runtime: &[u8]) {
        let initializer = contract.emit_initializer(self);

        // create start function
        let ret = contract.context.void_type();
        let ftype = ret.fn_type(&[], false);
        let function = contract.module.add_function("main", ftype, None);

        // FIXME: If there is no constructor, do not copy the calldata (but check calldatasize == 0)
        let (argsdata, length) = self.deployer_prelude(contract, function);

        // init our storage vars
        contract.builder.build_call(initializer, &[], "");

        // ewasm only allows one constructor, hence find()
        if let Some(con) = contract
            .contract
            .functions
            .iter()
            .find(|f| f.is_constructor())
        {
            let mut args = Vec::new();

            // insert abi decode
            self.abi
                .decode(contract, function, &mut args, argsdata, length, &con.params);

            contract
                .builder
                .build_call(contract.functions[&con.vsignature], &args, "");
        }

        // the deploy code should return the runtime wasm code
        let runtime_code = contract.emit_global_string("runtime_code", runtime, true);

        contract.builder.build_call(
            contract.module.get_function("finish").unwrap(),
            &[
                runtime_code.into(),
                contract
                    .context
                    .i32_type()
                    .const_int(runtime.len() as u64, false)
                    .into(),
            ],
            "",
        );

        // since finish is marked noreturn, this should be optimized away
        // however it is needed to create valid LLVM IR
        contract.builder.build_unreachable();
    }

    fn emit_function_dispatch(&self, contract: &Contract) {
        // create start function
        let ret = contract.context.void_type();
        let ftype = ret.fn_type(&[], false);
        let function = contract.module.add_function("main", ftype, None);

        let (argsdata, argslen) = self.runtime_prelude(contract, function);

        contract.emit_function_dispatch(
            pt::FunctionTy::Function,
            argsdata,
            argslen,
            function,
            None,
            self,
            |func| !contract.function_abort_value_transfers && !func.is_payable(),
        );
    }

    fn encode<'b>(
        &self,
        contract: &Contract<'b>,
        selector: Option<u32>,
        constant: Option<(PointerValue<'b>, u64)>,
        load: bool,
        function: FunctionValue,
        args: &[BasicValueEnum<'b>],
        spec: &[ast::Parameter],
    ) -> (PointerValue<'b>, IntValue<'b>) {
        let mut offset = contract.context.i32_type().const_int(
            spec.iter()
                .map(|arg| self.abi.encoded_fixed_length(&arg.ty, contract.ns))
                .sum(),
            false,
        );

        let mut length = offset;

        // now add the dynamic lengths
        for (i, s) in spec.iter().enumerate() {
            length = contract.builder.build_int_add(
                length,
                self.abi
                    .encoded_dynamic_length(args[i], load, &s.ty, function, contract),
                "",
            );
        }

        if selector.is_some() {
            length = contract.builder.build_int_add(
                length,
                contract
                    .context
                    .i32_type()
                    .const_int(std::mem::size_of::<u32>() as u64, false),
                "",
            );
        }

        if let Some((_, len)) = constant {
            length = contract.builder.build_int_add(
                length,
                contract.context.i32_type().const_int(len, false),
                "",
            );
        }

        let encoded_data = contract
            .builder
            .build_call(
                contract.module.get_function("__malloc").unwrap(),
                &[length.into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        // malloc returns u8*
        let mut data = encoded_data;

        if let Some(selector) = selector {
            contract.builder.build_store(
                contract.builder.build_pointer_cast(
                    data,
                    contract.context.i32_type().ptr_type(AddressSpace::Generic),
                    "",
                ),
                contract
                    .context
                    .i32_type()
                    .const_int(selector.to_be() as u64, false),
            );

            data = unsafe {
                contract.builder.build_gep(
                    data,
                    &[contract
                        .context
                        .i32_type()
                        .const_int(std::mem::size_of_val(&selector) as u64, false)],
                    "",
                )
            };
        }

        if let Some((code, code_len)) = constant {
            contract.builder.build_call(
                contract.module.get_function("__memcpy").unwrap(),
                &[
                    contract
                        .builder
                        .build_pointer_cast(
                            data,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into(),
                    code.into(),
                    contract
                        .context
                        .i32_type()
                        .const_int(code_len, false)
                        .into(),
                ],
                "",
            );

            data = unsafe {
                contract.builder.build_gep(
                    data,
                    &[contract.context.i32_type().const_int(code_len, false)],
                    "",
                )
            };
        }

        // We use a little trick here. The length might or might not include the selector.
        // The length will be a multiple of 32 plus the selector (4). So by dividing by 8,
        // we lose the selector.
        contract.builder.build_call(
            contract.module.get_function("__bzero8").unwrap(),
            &[
                data.into(),
                contract
                    .builder
                    .build_int_unsigned_div(
                        length,
                        contract.context.i32_type().const_int(8, false),
                        "",
                    )
                    .into(),
            ],
            "",
        );

        let mut dynamic = unsafe { contract.builder.build_gep(data, &[offset], "") };

        for (i, arg) in spec.iter().enumerate() {
            self.abi.encode_ty(
                contract,
                load,
                function,
                &arg.ty,
                args[i],
                &mut data,
                &mut offset,
                &mut dynamic,
            );
        }

        (encoded_data, length)
    }
}

impl TargetRuntime for EwasmTarget {
    fn clear_storage<'a>(
        &self,
        contract: &'a Contract,
        _function: FunctionValue,
        slot: PointerValue<'a>,
    ) {
        let value = contract
            .builder
            .build_alloca(contract.context.custom_width_int_type(256), "value");

        let value8 = contract.builder.build_pointer_cast(
            value,
            contract.context.i8_type().ptr_type(AddressSpace::Generic),
            "value8",
        );

        contract.builder.build_call(
            contract.module.get_function("__bzero8").unwrap(),
            &[
                value8.into(),
                contract.context.i32_type().const_int(4, false).into(),
            ],
            "",
        );

        contract.builder.build_call(
            contract.module.get_function("storageStore").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        slot,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                value8.into(),
            ],
            "",
        );
    }

    fn set_storage_string<'a>(
        &self,
        _contract: &'a Contract,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
        _dest: PointerValue<'a>,
    ) {
        unimplemented!();
    }

    fn get_storage_string<'a>(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue,
    ) -> PointerValue<'a> {
        unimplemented!();
    }
    fn get_storage_bytes_subscript<'a>(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
        _index: IntValue<'a>,
    ) -> IntValue<'a> {
        unimplemented!();
    }
    fn set_storage_bytes_subscript<'a>(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
        _index: IntValue<'a>,
        _val: IntValue<'a>,
    ) {
        unimplemented!();
    }
    fn storage_bytes_push<'a>(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
        _val: IntValue<'a>,
    ) {
        unimplemented!();
    }
    fn storage_bytes_pop<'a>(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
    ) -> IntValue<'a> {
        unimplemented!();
    }
    fn storage_string_length<'a>(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
    ) -> IntValue<'a> {
        unimplemented!();
    }

    fn set_storage<'a>(
        &self,
        contract: &'a Contract,
        _function: FunctionValue,
        slot: PointerValue<'a>,
        dest: PointerValue<'a>,
    ) {
        if dest
            .get_type()
            .get_element_type()
            .into_int_type()
            .get_bit_width()
            == 256
        {
            contract.builder.build_call(
                contract.module.get_function("storageStore").unwrap(),
                &[
                    contract
                        .builder
                        .build_pointer_cast(
                            slot,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into(),
                    contract
                        .builder
                        .build_pointer_cast(
                            dest,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into(),
                ],
                "",
            );
        } else {
            let value = contract
                .builder
                .build_alloca(contract.context.custom_width_int_type(256), "value");

            let value8 = contract.builder.build_pointer_cast(
                value,
                contract.context.i8_type().ptr_type(AddressSpace::Generic),
                "value8",
            );

            contract.builder.build_call(
                contract.module.get_function("__bzero8").unwrap(),
                &[
                    value8.into(),
                    contract.context.i32_type().const_int(4, false).into(),
                ],
                "",
            );

            let val = contract.builder.build_load(dest, "value");

            contract.builder.build_store(
                contract
                    .builder
                    .build_pointer_cast(value, dest.get_type(), ""),
                val,
            );

            contract.builder.build_call(
                contract.module.get_function("storageStore").unwrap(),
                &[
                    contract
                        .builder
                        .build_pointer_cast(
                            slot,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into(),
                    value8.into(),
                ],
                "",
            );
        }
    }

    fn get_storage_int<'a>(
        &self,
        contract: &Contract<'a>,
        _function: FunctionValue,
        slot: PointerValue,
        ty: IntType<'a>,
    ) -> IntValue<'a> {
        let dest = contract.builder.build_array_alloca(
            contract.context.i8_type(),
            contract.context.i32_type().const_int(32, false),
            "buf",
        );

        contract.builder.build_call(
            contract.module.get_function("storageLoad").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        slot,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract
                    .builder
                    .build_pointer_cast(
                        dest,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
            ],
            "",
        );

        contract
            .builder
            .build_load(
                contract
                    .builder
                    .build_pointer_cast(dest, ty.ptr_type(AddressSpace::Generic), ""),
                "loaded_int",
            )
            .into_int_value()
    }

    /// ewasm has no keccak256 host function, so call our implementation
    fn keccak256_hash(
        &self,
        contract: &Contract,
        src: PointerValue,
        length: IntValue,
        dest: PointerValue,
    ) {
        contract.builder.build_call(
            contract.module.get_function("sha3").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        src,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "src",
                    )
                    .into(),
                length.into(),
                contract
                    .builder
                    .build_pointer_cast(
                        dest,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "dest",
                    )
                    .into(),
                contract.context.i32_type().const_int(32, false).into(),
            ],
            "",
        );
    }

    fn return_empty_abi(&self, contract: &Contract) {
        contract.builder.build_call(
            contract.module.get_function("finish").unwrap(),
            &[
                contract
                    .context
                    .i8_type()
                    .ptr_type(AddressSpace::Generic)
                    .const_zero()
                    .into(),
                contract.context.i32_type().const_zero().into(),
            ],
            "",
        );

        // since finish is marked noreturn, this should be optimized away
        // however it is needed to create valid LLVM IR
        contract.builder.build_unreachable();
    }

    fn return_abi<'b>(&self, contract: &'b Contract, data: PointerValue<'b>, length: IntValue) {
        contract.builder.build_call(
            contract.module.get_function("finish").unwrap(),
            &[data.into(), length.into()],
            "",
        );

        // since finish is marked noreturn, this should be optimized away
        // however it is needed to create valid LLVM IR
        contract.builder.build_unreachable();
    }

    // ewasm main cannot return any value
    fn return_u32<'b>(&self, contract: &'b Contract, _ret: IntValue<'b>) {
        self.assert_failure(
            contract,
            contract
                .context
                .i8_type()
                .ptr_type(AddressSpace::Generic)
                .const_null(),
            contract.context.i32_type().const_zero(),
        );
    }

    fn assert_failure<'b>(&self, contract: &'b Contract, data: PointerValue, len: IntValue) {
        contract.builder.build_call(
            contract.module.get_function("revert").unwrap(),
            &[data.into(), len.into()],
            "",
        );

        // since revert is marked noreturn, this should be optimized away
        // however it is needed to create valid LLVM IR
        contract.builder.build_unreachable();
    }

    /// ABI encode into a vector for abi.encode* style builtin functions
    fn abi_encode_to_vector<'b>(
        &self,
        _contract: &Contract<'b>,
        _selector: Option<IntValue<'b>>,
        _function: FunctionValue,
        _packed: bool,
        _args: &[BasicValueEnum<'b>],
        _spec: &[ast::Type],
    ) -> PointerValue<'b> {
        unimplemented!();
    }

    fn abi_encode<'b>(
        &self,
        contract: &Contract<'b>,
        selector: Option<u32>,
        load: bool,
        function: FunctionValue,
        args: &[BasicValueEnum<'b>],
        spec: &[ast::Parameter],
    ) -> (PointerValue<'b>, IntValue<'b>) {
        self.encode(contract, selector, None, load, function, args, spec)
    }

    fn abi_decode<'b>(
        &self,
        contract: &Contract<'b>,
        function: FunctionValue,
        args: &mut Vec<BasicValueEnum<'b>>,
        data: PointerValue<'b>,
        length: IntValue<'b>,
        spec: &[ast::Parameter],
    ) {
        self.abi
            .decode(contract, function, args, data, length, spec);
    }

    fn print(&self, contract: &Contract, string_ptr: PointerValue, string_len: IntValue) {
        contract.builder.build_call(
            contract.module.get_function("printMem").unwrap(),
            &[string_ptr.into(), string_len.into()],
            "",
        );
    }

    fn create_contract<'b>(
        &mut self,
        contract: &Contract<'b>,
        function: FunctionValue,
        success: Option<&mut BasicValueEnum<'b>>,
        contract_no: usize,
        constructor_no: usize,
        address: PointerValue<'b>,
        args: &[BasicValueEnum<'b>],
        _gas: IntValue<'b>,
        value: Option<IntValue<'b>>,
        _salt: Option<IntValue<'b>>,
    ) {
        let resolver_contract = &contract.ns.contracts[contract_no];

        let target_contract = Contract::build(
            contract.context,
            &resolver_contract,
            contract.ns,
            "",
            contract.opt,
        );

        // wasm
        let wasm = target_contract.wasm(true).expect("compile should succeeed");

        let code = contract.emit_global_string(
            &format!("contract_{}_code", resolver_contract.name),
            &wasm,
            true,
        );

        assert_eq!(constructor_no, 0);

        let params = if let Some(f) = resolver_contract
            .functions
            .iter()
            .find(|f| f.is_constructor())
        {
            f.params.as_slice()
        } else {
            &[]
        };

        // input
        let (input, input_len) = self.encode(
            contract,
            None,
            Some((code, wasm.len() as u64)),
            false,
            function,
            args,
            params,
        );

        // value is a u128
        let value_ptr = contract
            .builder
            .build_alloca(contract.value_type(), "balance");
        contract.builder.build_store(
            value_ptr,
            match value {
                Some(v) => v,
                None => contract.value_type().const_zero(),
            },
        );

        // call create
        let ret = contract
            .builder
            .build_call(
                contract.module.get_function("create").unwrap(),
                &[
                    contract
                        .builder
                        .build_pointer_cast(
                            value_ptr,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "value_transfer",
                        )
                        .into(),
                    input.into(),
                    input_len.into(),
                    address.into(),
                ],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value();

        let is_success = contract.builder.build_int_compare(
            IntPredicate::EQ,
            ret,
            contract.context.i32_type().const_zero(),
            "success",
        );

        if let Some(success) = success {
            *success = is_success.into();
        } else {
            let success_block = contract.context.append_basic_block(function, "success");
            let bail_block = contract.context.append_basic_block(function, "bail");
            contract
                .builder
                .build_conditional_branch(is_success, success_block, bail_block);

            contract.builder.position_at_end(bail_block);

            self.assert_failure(
                contract,
                contract
                    .context
                    .i8_type()
                    .ptr_type(AddressSpace::Generic)
                    .const_null(),
                contract.context.i32_type().const_zero(),
            );

            contract.builder.position_at_end(success_block);
        }
    }

    fn external_call<'b>(
        &self,
        contract: &Contract<'b>,
        payload: PointerValue<'b>,
        payload_len: IntValue<'b>,
        address: PointerValue<'b>,
        gas: IntValue<'b>,
        value: IntValue<'b>,
        callty: ast::CallTy,
    ) -> IntValue<'b> {
        // value is a u128
        let value_ptr = contract
            .builder
            .build_alloca(contract.value_type(), "balance");
        contract.builder.build_store(value_ptr, value);

        // call create
        contract
            .builder
            .build_call(
                contract
                    .module
                    .get_function(match callty {
                        ast::CallTy::Regular => "call",
                        ast::CallTy::Static => "staticcall",
                        ast::CallTy::Delegate => "delegatecall",
                    })
                    .unwrap(),
                &[
                    gas.into(),
                    address.into(),
                    contract
                        .builder
                        .build_pointer_cast(
                            value_ptr,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "value_transfer",
                        )
                        .into(),
                    payload.into(),
                    payload_len.into(),
                ],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value()
    }

    fn return_data<'b>(&self, contract: &Contract<'b>) -> PointerValue<'b> {
        let length = contract
            .builder
            .build_call(
                contract.module.get_function("getReturnDataSize").unwrap(),
                &[],
                "returndatasize",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value();

        let malloc_length = contract.builder.build_int_add(
            length,
            contract
                .module
                .get_struct_type("struct.vector")
                .unwrap()
                .size_of()
                .unwrap()
                .const_cast(contract.context.i32_type(), false),
            "size",
        );

        let p = contract
            .builder
            .build_call(
                contract.module.get_function("__malloc").unwrap(),
                &[malloc_length.into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        let v = contract.builder.build_pointer_cast(
            p,
            contract
                .module
                .get_struct_type("struct.vector")
                .unwrap()
                .ptr_type(AddressSpace::Generic),
            "string",
        );

        let data_len = unsafe {
            contract.builder.build_gep(
                v,
                &[
                    contract.context.i32_type().const_zero(),
                    contract.context.i32_type().const_zero(),
                ],
                "data_len",
            )
        };

        contract.builder.build_store(data_len, length);

        let data_size = unsafe {
            contract.builder.build_gep(
                v,
                &[
                    contract.context.i32_type().const_zero(),
                    contract.context.i32_type().const_int(1, false),
                ],
                "data_size",
            )
        };

        contract.builder.build_store(data_size, length);

        let data = unsafe {
            contract.builder.build_gep(
                v,
                &[
                    contract.context.i32_type().const_zero(),
                    contract.context.i32_type().const_int(2, false),
                ],
                "data",
            )
        };

        contract.builder.build_call(
            contract.module.get_function("returnDataCopy").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        data,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract.context.i32_type().const_zero().into(),
                length.into(),
            ],
            "",
        );

        v
    }

    /// ewasm value is always 128 bits
    fn value_transferred<'b>(&self, contract: &Contract<'b>) -> IntValue<'b> {
        let value = contract
            .builder
            .build_alloca(contract.value_type(), "value_transferred");

        contract.builder.build_call(
            contract.module.get_function("getCallValue").unwrap(),
            &[contract
                .builder
                .build_pointer_cast(
                    value,
                    contract.context.i8_type().ptr_type(AddressSpace::Generic),
                    "",
                )
                .into()],
            "value_transferred",
        );

        contract
            .builder
            .build_load(value, "value_transferred")
            .into_int_value()
    }

    /// ewasm address is always 160 bits
    fn get_address<'b>(&self, contract: &Contract<'b>) -> IntValue<'b> {
        let value = contract
            .builder
            .build_alloca(contract.address_type(), "self_address");

        contract.builder.build_call(
            contract.module.get_function("getAddress").unwrap(),
            &[contract
                .builder
                .build_pointer_cast(
                    value,
                    contract.context.i8_type().ptr_type(AddressSpace::Generic),
                    "",
                )
                .into()],
            "self_address",
        );

        contract
            .builder
            .build_load(value, "self_address")
            .into_int_value()
    }

    /// ewasm address is always 160 bits
    fn balance<'b>(&self, contract: &Contract<'b>, addr: IntValue<'b>) -> IntValue<'b> {
        let address = contract
            .builder
            .build_alloca(contract.address_type(), "address");

        contract.builder.build_store(address, addr);

        let balance = contract
            .builder
            .build_alloca(contract.value_type(), "balance");

        contract.builder.build_call(
            contract.module.get_function("getExternalBalance").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        address,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract
                    .builder
                    .build_pointer_cast(
                        balance,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
            ],
            "balance",
        );

        contract
            .builder
            .build_load(balance, "balance")
            .into_int_value()
    }

    /// Terminate execution, destroy contract and send remaining funds to addr
    fn selfdestruct<'b>(&self, contract: &Contract<'b>, addr: IntValue<'b>) {
        let address = contract
            .builder
            .build_alloca(contract.address_type(), "address");

        contract.builder.build_store(address, addr);

        contract.builder.build_call(
            contract.module.get_function("selfDestruct").unwrap(),
            &[contract
                .builder
                .build_pointer_cast(
                    address,
                    contract.context.i8_type().ptr_type(AddressSpace::Generic),
                    "",
                )
                .into()],
            "terminated",
        );
    }

    /// Crypto Hash
    fn hash<'b>(
        &self,
        contract: &Contract<'b>,
        hash: HashTy,
        input: PointerValue<'b>,
        input_len: IntValue<'b>,
    ) -> IntValue<'b> {
        let (precompile, hashlen) = match hash {
            HashTy::Keccak256 => (0, 32),
            HashTy::Ripemd160 => (3, 20),
            HashTy::Sha256 => (2, 32),
            _ => unreachable!(),
        };

        let res = contract.builder.build_array_alloca(
            contract.context.i8_type(),
            contract.context.i32_type().const_int(hashlen, false),
            "res",
        );

        if hash == HashTy::Keccak256 {
            contract.builder.build_call(
                contract.module.get_function("sha3").unwrap(),
                &[
                    input.into(),
                    input_len.into(),
                    res.into(),
                    contract.context.i32_type().const_int(hashlen, false).into(),
                ],
                "",
            );
        } else {
            let balance = contract
                .builder
                .build_alloca(contract.value_type(), "balance");

            contract
                .builder
                .build_store(balance, contract.value_type().const_zero());

            let address = contract
                .builder
                .build_alloca(contract.address_type(), "address");

            contract.builder.build_store(
                address,
                contract.address_type().const_int(precompile, false),
            );

            contract.builder.build_call(
                contract.module.get_function("call").unwrap(),
                &[
                    contract.context.i64_type().const_zero().into(),
                    contract
                        .builder
                        .build_pointer_cast(
                            address,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "address",
                        )
                        .into(),
                    contract
                        .builder
                        .build_pointer_cast(
                            balance,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "balance",
                        )
                        .into(),
                    input.into(),
                    input_len.into(),
                ],
                "",
            );

            // We're not checking return value or returnDataSize;
            // assuming precompiles always succeed

            contract.builder.build_call(
                contract.module.get_function("returnDataCopy").unwrap(),
                &[
                    res.into(),
                    contract.context.i32_type().const_zero().into(),
                    contract.context.i32_type().const_int(hashlen, false).into(),
                ],
                "",
            );
        }

        // bytes32 needs to reverse bytes
        let temp = contract
            .builder
            .build_alloca(contract.llvm_type(&ast::Type::Bytes(hashlen as u8)), "hash");

        contract.builder.build_call(
            contract.module.get_function("__beNtoleN").unwrap(),
            &[
                res.into(),
                contract
                    .builder
                    .build_pointer_cast(
                        temp,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract.context.i32_type().const_int(hashlen, false).into(),
            ],
            "",
        );

        contract.builder.build_load(temp, "hash").into_int_value()
    }

    /// builtin expressions
    fn builtin<'b>(
        &self,
        contract: &Contract<'b>,
        expr: &ast::Expression,
        vartab: &[Variable<'b>],
        function: FunctionValue<'b>,
        runtime: &dyn TargetRuntime,
    ) -> BasicValueEnum<'b> {
        macro_rules! straight_call {
            ($name:literal, $func:literal) => {{
                contract
                    .builder
                    .build_call(contract.module.get_function($func).unwrap(), &[], $name)
                    .try_as_basic_value()
                    .left()
                    .unwrap()
            }};
        }

        macro_rules! single_value_stack {
            ($name:literal, $func:literal, $width:expr) => {{
                let value = contract
                    .builder
                    .build_alloca(contract.context.custom_width_int_type($width), $name);

                contract.builder.build_call(
                    contract.module.get_function($func).unwrap(),
                    &[contract
                        .builder
                        .build_pointer_cast(
                            value,
                            contract.context.i8_type().ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into()],
                    $name,
                );

                contract.builder.build_load(value, $name)
            }};
        }

        match expr {
            ast::Expression::Builtin(_, _, ast::Builtin::BlockNumber, _) => {
                straight_call!("block_number", "getBlockNumber")
            }
            ast::Expression::Builtin(_, _, ast::Builtin::Gasleft, _) => {
                straight_call!("gas_left", "getGasLeft")
            }
            ast::Expression::Builtin(_, _, ast::Builtin::GasLimit, _) => {
                straight_call!("gas_limit", "getBlockGasLimit")
            }
            ast::Expression::Builtin(_, _, ast::Builtin::Timestamp, _) => {
                straight_call!("time_stamp", "getBlockTimestamp")
            }
            ast::Expression::Builtin(_, _, ast::Builtin::BlockDifficulty, _) => {
                single_value_stack!("block_difficulty", "getBlockDifficulty", 256)
            }
            ast::Expression::Builtin(_, _, ast::Builtin::Origin, _) => single_value_stack!(
                "origin",
                "getTxOrigin",
                contract.ns.address_length as u32 * 8
            ),
            ast::Expression::Builtin(_, _, ast::Builtin::Sender, _) => {
                single_value_stack!("caller", "getCaller", contract.ns.address_length as u32 * 8)
            }
            ast::Expression::Builtin(_, _, ast::Builtin::BlockCoinbase, _) => single_value_stack!(
                "coinbase",
                "getBlockCoinbase",
                contract.ns.address_length as u32 * 8
            ),
            ast::Expression::Builtin(_, _, ast::Builtin::Gasprice, _) => single_value_stack!(
                "gas_price",
                "getTxGasPrice",
                contract.ns.value_length as u32 * 8
            ),
            ast::Expression::Builtin(_, _, ast::Builtin::Value, _) => {
                single_value_stack!("value", "getCallValue", contract.ns.value_length as u32 * 8)
            }
            ast::Expression::Builtin(_, _, ast::Builtin::BlockHash, args) => {
                let block_number = contract.expression(&args[0], vartab, function, runtime);

                let value = contract
                    .builder
                    .build_alloca(contract.context.custom_width_int_type(256), "block_hash");

                contract.builder.build_call(
                    contract.module.get_function("getBlockHash").unwrap(),
                    &[
                        block_number,
                        contract
                            .builder
                            .build_pointer_cast(
                                value,
                                contract.context.i8_type().ptr_type(AddressSpace::Generic),
                                "",
                            )
                            .into(),
                    ],
                    "block_hash",
                );

                contract.builder.build_load(value, "block_hash")
            }
            _ => unimplemented!(),
        }
    }
}
