use crate::codegen::cfg::HashTy;
use crate::parser::pt;
use crate::sema::ast;
use std::collections::HashMap;
use std::str;

use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::types::IntType;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;

use super::ethabiencoder;
use super::{Contract, TargetRuntime, Variable};

pub struct GenericTarget {
    abi: ethabiencoder::EthAbiDecoder,
}

impl GenericTarget {
    pub fn build<'a>(
        context: &'a Context,
        contract: &'a ast::Contract,
        ns: &'a ast::Namespace,
        filename: &'a str,
        opt: OptimizationLevel,
        math_overflow_check: bool,
    ) -> Contract<'a> {
        let mut b = GenericTarget {
            abi: ethabiencoder::EthAbiDecoder { bswap: false },
        };

        let mut c = Contract::new(
            context,
            contract,
            ns,
            filename,
            opt,
            math_overflow_check,
            None,
        );

        // externals
        b.declare_externals(&mut c);

        b.emit_functions(&mut c);

        b.emit_constructor(&mut c);
        b.emit_function(&mut c);

        c
    }

    fn declare_externals(&self, contract: &mut Contract) {
        let void_ty = contract.context.void_type();
        let u8_ptr = contract.context.i8_type().ptr_type(AddressSpace::Generic);
        let u32_ty = contract.context.i32_type();

        contract.module.add_function(
            "solang_storage_delete",
            void_ty.fn_type(&[u8_ptr.into()], false),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "solang_storage_set",
            void_ty.fn_type(
                &[
                    u8_ptr.into(),
                    u8_ptr.into(),
                    contract.context.i32_type().into(),
                ],
                false,
            ),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "solang_storage_size",
            u32_ty.fn_type(&[u8_ptr.into()], false),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "solang_storage_get",
            void_ty.fn_type(&[u8_ptr.into(), u8_ptr.into()], false),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "solang_malloc",
            u8_ptr.fn_type(&[contract.context.i32_type().into()], false),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "solang_print",
            void_ty.fn_type(&[u8_ptr.into(), u32_ty.into()], false),
            Some(Linkage::External),
        );
        contract.module.add_function(
            "solang_set_return",
            void_ty.fn_type(&[u8_ptr.into(), u32_ty.into()], false),
            Some(Linkage::External),
        );
    }

    fn emit_constructor(&mut self, contract: &mut Contract) {
        let initializer = self.emit_initializer(contract);

        let u8_ptr_ty = contract.context.i8_type().ptr_type(AddressSpace::Generic);
        let u32_ty = contract.context.i32_type();

        let ret = contract.context.i32_type();
        let ftype = ret.fn_type(&[u8_ptr_ty.into(), u32_ty.into()], false);
        let function = contract
            .module
            .add_function("solang_constructor", ftype, None);

        let entry = contract.context.append_basic_block(function, "entry");

        contract.builder.position_at_end(entry);

        // we should not use our heap; use sabre provided heap instead
        let argsdata = function.get_nth_param(0).unwrap().into_pointer_value();
        let argslen = function.get_nth_param(1).unwrap().into_int_value();

        // init our storage vars
        contract.builder.build_call(initializer, &[], "");

        if let Some((cfg_no, con)) = contract
            .contract
            .functions
            .iter()
            .enumerate()
            .map(|(cfg_no, function_no)| (cfg_no, &contract.ns.functions[*function_no]))
            .find(|(_, f)| f.is_constructor())
        {
            let mut args = Vec::new();

            // insert abi decode
            self.abi.decode(
                contract,
                function,
                &mut args,
                argsdata,
                argslen,
                &con.params,
            );

            contract
                .builder
                .build_call(contract.functions[&cfg_no], &args, "");
        }

        // return 0 for success
        contract
            .builder
            .build_return(Some(&contract.context.i32_type().const_int(0, false)));
    }

    // emit function dispatch
    fn emit_function<'s>(&'s mut self, contract: &'s mut Contract) {
        let u8_ptr_ty = contract.context.i8_type().ptr_type(AddressSpace::Generic);
        let u32_ty = contract.context.i32_type();

        let ret = contract.context.i32_type();
        let ftype = ret.fn_type(&[u8_ptr_ty.into(), u32_ty.into()], false);
        let function = contract.module.add_function("solang_function", ftype, None);

        let entry = contract.context.append_basic_block(function, "entry");

        contract.builder.position_at_end(entry);

        // we should not use our heap; use sabre provided heap instead
        let argsdata = function.get_nth_param(0).unwrap().into_pointer_value();
        let argslen = function.get_nth_param(1).unwrap().into_int_value();

        let argsdata = contract.builder.build_pointer_cast(
            argsdata,
            contract.context.i32_type().ptr_type(AddressSpace::Generic),
            "argsdata32",
        );

        self.emit_function_dispatch(
            contract,
            pt::FunctionTy::Function,
            argsdata,
            argslen,
            function,
            None,
            |_| false,
        );
    }
}

impl<'a> TargetRuntime<'a> for GenericTarget {
    fn storage_delete_single_slot(
        &self,
        contract: &Contract,
        _function: FunctionValue,
        slot: PointerValue,
    ) {
        contract.builder.build_call(
            contract
                .module
                .get_function("solang_storage_delete")
                .unwrap(),
            &[slot.into()],
            "",
        );
    }

    fn set_storage(
        &self,
        contract: &Contract,
        _function: FunctionValue,
        slot: PointerValue,
        dest: PointerValue,
    ) {
        // TODO: check for non-zero
        contract.builder.build_call(
            contract.module.get_function("solang_storage_set").unwrap(),
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
                dest.get_type()
                    .get_element_type()
                    .into_int_type()
                    .size_of()
                    .const_cast(contract.context.i32_type(), false)
                    .into(),
            ],
            "",
        );
    }

    fn set_storage_string(
        &self,
        contract: &Contract<'a>,
        _function: FunctionValue<'a>,
        slot: PointerValue<'a>,
        dest: BasicValueEnum<'a>,
    ) {
        // TODO: check for non-zero
        contract.builder.build_call(
            contract.module.get_function("solang_storage_set").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        slot,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract.vector_bytes(dest).into(),
                contract.vector_len(dest).into(),
            ],
            "",
        );
    }

    fn get_storage_string(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
    ) -> PointerValue<'a> {
        unimplemented!();
    }
    fn get_storage_bytes_subscript(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: IntValue<'a>,
        _index: IntValue<'a>,
    ) -> IntValue<'a> {
        unimplemented!();
    }
    fn set_storage_extfunc(
        &self,
        _contract: &Contract,
        _function: FunctionValue,
        _slot: PointerValue,
        _dest: PointerValue,
    ) {
        unimplemented!();
    }
    fn get_storage_extfunc(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _slot: PointerValue<'a>,
    ) -> PointerValue<'a> {
        unimplemented!();
    }
    fn set_storage_bytes_subscript(
        &self,
        _contract: &Contract,
        _function: FunctionValue,
        _slot: IntValue,
        _index: IntValue,
        _val: IntValue,
    ) {
        unimplemented!();
    }
    fn storage_push(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _ty: &ast::Type,
        _slot: IntValue<'a>,
        _val: BasicValueEnum<'a>,
    ) -> BasicValueEnum<'a> {
        unimplemented!();
    }
    fn storage_pop(
        &self,
        _contract: &Contract<'a>,
        _function: FunctionValue,
        _ty: &ast::Type,
        _slot: IntValue<'a>,
    ) -> BasicValueEnum<'a> {
        unimplemented!();
    }
    fn storage_array_length(
        &self,
        contract: &Contract<'a>,
        _function: FunctionValue,
        slot: IntValue<'a>,
        _ty: &ast::Type,
    ) -> IntValue<'a> {
        let slot_ptr = contract.builder.build_alloca(slot.get_type(), "slot");
        contract.builder.build_store(slot_ptr, slot);

        contract
            .builder
            .build_call(
                contract.module.get_function("solang_storage_size").unwrap(),
                &[contract
                    .builder
                    .build_pointer_cast(
                        slot_ptr,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value()
    }

    fn get_storage_int(
        &self,
        contract: &Contract<'a>,
        function: FunctionValue,
        slot: PointerValue<'a>,
        ty: IntType<'a>,
    ) -> IntValue<'a> {
        let exists = contract
            .builder
            .build_call(
                contract.module.get_function("solang_storage_size").unwrap(),
                &[contract
                    .builder
                    .build_pointer_cast(
                        slot,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap();

        let data_size = ty.size_of().const_cast(contract.context.i32_type(), false);

        let exists = contract.builder.build_int_compare(
            IntPredicate::EQ,
            exists.into_int_value(),
            data_size,
            "storage_exists",
        );

        let entry = contract.builder.get_insert_block().unwrap();
        let retrieve_block = contract.context.append_basic_block(function, "in_storage");
        let done_storage = contract
            .context
            .append_basic_block(function, "done_storage");

        contract
            .builder
            .build_conditional_branch(exists, retrieve_block, done_storage);

        contract.builder.position_at_end(retrieve_block);

        let dest = contract.builder.build_alloca(ty, "int");

        contract.builder.build_call(
            contract.module.get_function("solang_storage_get").unwrap(),
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

        let loaded_int = contract.builder.build_load(dest, "int");

        contract.builder.build_unconditional_branch(done_storage);

        contract.builder.position_at_end(done_storage);

        let res = contract.builder.build_phi(ty, "storage_res");

        res.add_incoming(&[(&loaded_int, retrieve_block), (&ty.const_zero(), entry)]);

        res.as_basic_value().into_int_value()
    }

    /// sabre has no keccak256 host function, so call our implementation
    fn keccak256_hash(
        &self,
        contract: &Contract,
        src: PointerValue,
        length: IntValue,
        dest: PointerValue,
    ) {
        contract.builder.build_call(
            contract.module.get_function("keccak256").unwrap(),
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
            ],
            "",
        );
    }

    fn return_empty_abi(&self, contract: &Contract) {
        // return 0 for success
        contract
            .builder
            .build_return(Some(&contract.context.i32_type().const_int(0, false)));
    }

    fn return_abi<'b>(&self, contract: &'b Contract, data: PointerValue<'b>, length: IntValue) {
        contract.builder.build_call(
            contract.module.get_function("solang_set_return").unwrap(),
            &[data.into(), length.into()],
            "",
        );
        contract
            .builder
            .build_return(Some(&contract.context.i32_type().const_int(0, false)));
    }

    fn assert_failure<'b>(&self, contract: &'b Contract, data: PointerValue, length: IntValue) {
        contract.builder.build_call(
            contract.module.get_function("solang_set_return").unwrap(),
            &[data.into(), length.into()],
            "",
        );
        contract
            .builder
            .build_return(Some(&contract.context.i32_type().const_int(2, false)));
    }

    /// ABI encode into a vector for abi.encode* style builtin functions
    fn abi_encode_to_vector<'b>(
        &self,
        contract: &Contract<'b>,
        function: FunctionValue<'b>,
        packed: &[BasicValueEnum<'b>],
        args: &[BasicValueEnum<'b>],
        tys: &[ast::Type],
    ) -> PointerValue<'b> {
        ethabiencoder::encode_to_vector(contract, function, packed, args, tys, false)
    }

    fn abi_encode<'b>(
        &self,
        contract: &Contract<'b>,
        selector: Option<IntValue<'b>>,
        load: bool,
        function: FunctionValue<'b>,
        args: &[BasicValueEnum<'b>],
        tys: &[ast::Type],
    ) -> (PointerValue<'b>, IntValue<'b>) {
        let mut tys = tys.to_vec();

        let packed = if let Some(selector) = selector {
            tys.insert(0, ast::Type::Uint(32));
            vec![selector.into()]
        } else {
            vec![]
        };

        let encoder = ethabiencoder::EncoderBuilder::new(
            contract, function, load, args, &packed, &tys, false,
        );

        let length = encoder.encoded_length();

        let encoded_data = contract
            .builder
            .build_call(
                contract.module.get_function("solang_malloc").unwrap(),
                &[length.into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        encoder.finish(contract, function, encoded_data);

        (encoded_data, length)
    }

    fn abi_decode<'b>(
        &self,
        contract: &Contract<'b>,
        function: FunctionValue<'b>,
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
            contract.module.get_function("solang_print").unwrap(),
            &[string_ptr.into(), string_len.into()],
            "",
        );
    }

    /// Create new contract
    fn create_contract<'b>(
        &mut self,
        _contract: &Contract<'b>,
        _function: FunctionValue,
        _success: Option<&mut BasicValueEnum<'b>>,
        _contract_no: usize,
        _constructor_no: Option<usize>,
        _address: PointerValue<'b>,
        _args: &[BasicValueEnum],
        _gas: IntValue<'b>,
        _value: Option<IntValue<'b>>,
        _salt: Option<IntValue<'b>>,
    ) {
        panic!("generic cannot create new contracts");
    }

    /// Call external contract
    fn external_call<'b>(
        &self,
        _contract: &Contract<'b>,
        _function: FunctionValue,
        _success: Option<&mut BasicValueEnum<'b>>,
        _payload: PointerValue<'b>,
        _payload_len: IntValue<'b>,
        _address: Option<PointerValue<'b>>,
        _gas: IntValue<'b>,
        _value: IntValue<'b>,
        _ty: ast::CallTy,
    ) {
        panic!("generic cannot call other contracts");
    }

    /// Get return buffer for external call
    fn return_data<'b>(&self, _contract: &Contract<'b>) -> PointerValue<'b> {
        panic!("generic cannot call other contracts");
    }

    fn return_code<'b>(&self, contract: &'b Contract, ret: IntValue<'b>) {
        contract.builder.build_return(Some(&ret));
    }

    /// Sabre does not know about balances
    fn value_transferred<'b>(&self, contract: &Contract<'b>) -> IntValue<'b> {
        contract.value_type().const_zero()
    }

    /// Terminate execution, destroy contract and send remaining funds to addr
    fn selfdestruct<'b>(&self, _contract: &Contract<'b>, _addr: IntValue<'b>) {
        panic!("generic does not have the concept of selfdestruct");
    }

    /// Send event
    fn send_event<'b>(
        &self,
        _contract: &Contract<'b>,
        _event_no: usize,
        _data: PointerValue<'b>,
        _data_len: IntValue<'b>,
        _topics: Vec<(PointerValue<'b>, IntValue<'b>)>,
    ) {
        unimplemented!();
    }

    /// builtin expressions
    fn builtin<'b>(
        &self,
        _contract: &Contract<'b>,
        _expr: &ast::Expression,
        _vartab: &HashMap<usize, Variable<'b>>,
        _function: FunctionValue<'b>,
    ) -> BasicValueEnum<'b> {
        unimplemented!();
    }

    /// Crypto Hash
    fn hash<'b>(
        &self,
        _contract: &Contract<'b>,
        _hash: HashTy,
        _input: PointerValue<'b>,
        _input_len: IntValue<'b>,
    ) -> IntValue<'b> {
        unimplemented!()
    }
}
