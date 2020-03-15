use parser::ast;
use resolver;

use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use num_traits::ToPrimitive;

use super::{Contract, TargetRuntime};

pub struct SubstrateTarget {}

const ADDRESS_LENGTH: u64 = 20;

impl SubstrateTarget {
    pub fn build<'a>(
        context: &'a Context,
        contract: &'a resolver::Contract,
        filename: &'a str,
    ) -> Contract<'a> {
        let mut c = Contract::new(context, contract, filename, None);
        let b = SubstrateTarget {};

        b.declare_externals(&c);

        c.emit_functions(&b);

        b.emit_deploy(&c);
        b.emit_call(&c);

        c
    }

    fn public_function_prelude<'a>(
        &self,
        contract: &'a Contract,
        function: FunctionValue,
    ) -> (PointerValue<'a>, IntValue<'a>) {
        let entry = contract.context.append_basic_block(function, "entry");

        contract.builder.position_at_end(entry);

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
                contract.module.get_function("ext_scratch_size").unwrap(),
                &[],
                "scratch_size",
            )
            .try_as_basic_value()
            .left()
            .unwrap();

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

        contract.builder.build_call(
            contract.module.get_function("ext_scratch_read").unwrap(),
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

    fn declare_externals(&self, contract: &Contract) {
        // Access to scratch buffer
        contract.module.add_function(
            "ext_scratch_size",
            contract.context.i32_type().fn_type(&[], false),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "ext_scratch_read",
            contract.context.void_type().fn_type(
                &[
                    contract
                        .context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(), // dest_ptr
                    contract.context.i32_type().into(), // offset
                    contract.context.i32_type().into(), // len
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "ext_scratch_write",
            contract.context.void_type().fn_type(
                &[
                    contract
                        .context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(), // dest_ptr
                    contract.context.i32_type().into(), // len
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "ext_set_storage",
            contract.context.void_type().fn_type(
                &[
                    contract
                        .context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(), // key_ptr
                    contract.context.i32_type().into(), // value_non_null
                    contract
                        .context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(), // value_ptr
                    contract.context.i32_type().into(), // value_len
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "ext_get_storage",
            contract.context.i32_type().fn_type(
                &[
                    contract
                        .context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(), // key_ptr
                ],
                false,
            ),
            Some(Linkage::External),
        );

        contract.module.add_function(
            "ext_return",
            contract.context.void_type().fn_type(
                &[
                    contract
                        .context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .into(), // data_ptr
                    contract.context.i32_type().into(), // data_len
                ],
                false,
            ),
            Some(Linkage::External),
        );
    }

    fn emit_deploy(&self, contract: &Contract) {
        let initializer = contract.emit_initializer(self);

        // create deploy function
        let function = contract.module.add_function(
            "deploy",
            contract.context.i32_type().fn_type(&[], false),
            None,
        );

        let (deploy_args, deploy_args_length) = self.public_function_prelude(contract, function);

        // init our storage vars
        contract.builder.build_call(initializer, &[], "");

        let fallback_block = contract.context.append_basic_block(function, "fallback");

        contract.emit_function_dispatch(
            &contract.ns.constructors,
            &contract.constructors,
            deploy_args,
            deploy_args_length,
            function,
            fallback_block,
            self,
        );

        // emit fallback code
        contract.builder.position_at_end(fallback_block);
        contract.builder.build_unreachable();
    }

    fn emit_call(&self, contract: &Contract) {
        // create call function
        let function = contract.module.add_function(
            "call",
            contract.context.i32_type().fn_type(&[], false),
            None,
        );

        let (call_args, call_args_length) = self.public_function_prelude(contract, function);

        let fallback_block = contract.context.append_basic_block(function, "fallback");

        contract.emit_function_dispatch(
            &contract.ns.functions,
            &contract.functions,
            call_args,
            call_args_length,
            function,
            fallback_block,
            self,
        );

        // emit fallback code
        contract.builder.position_at_end(fallback_block);

        if let Some(fallback) = contract.ns.fallback_function() {
            contract
                .builder
                .build_call(contract.functions[fallback], &[], "");

            contract
                .builder
                .build_return(Some(&contract.context.i32_type().const_zero()));
        } else {
            contract.builder.build_unreachable();
        }
    }

    /// ABI decode a single primitive
    fn decode_primitive<'b>(
        &self,
        contract: &'b Contract,
        ty: ast::PrimitiveType,
        to: Option<PointerValue<'b>>,
        src: PointerValue<'b>,
    ) -> (BasicValueEnum<'b>, u64) {
        match ty {
            ast::PrimitiveType::Bool => {
                let val = contract.builder.build_int_compare(
                    IntPredicate::EQ,
                    contract
                        .builder
                        .build_load(src, "abi_bool")
                        .into_int_value(),
                    contract.context.i8_type().const_int(1, false),
                    "bool",
                );
                if let Some(p) = to {
                    contract.builder.build_store(p, val);
                }
                (val.into(), 1)
            }
            ast::PrimitiveType::Uint(n) | ast::PrimitiveType::Int(n) => {
                let int_type = contract.context.custom_width_int_type(n as u32);

                let store = to.unwrap_or_else(|| contract.builder.build_alloca(int_type, "stack"));

                let val = contract.builder.build_load(
                    contract.builder.build_pointer_cast(
                        src,
                        int_type.ptr_type(AddressSpace::Generic),
                        "",
                    ),
                    "",
                );

                let len = n as u64 / 8;

                if n <= 64 && to.is_none() {
                    (val, len)
                } else {
                    contract.builder.build_store(store, val);

                    (store.into(), len)
                }
            }
            ast::PrimitiveType::Bytes(len) => {
                let int_type = contract.context.custom_width_int_type(len as u32 * 8);

                let store = to.unwrap_or_else(|| contract.builder.build_alloca(int_type, "stack"));

                // byte order needs to be reversed. e.g. hex"11223344" should be 0x10 0x11 0x22 0x33 0x44
                contract.builder.build_call(
                    contract.module.get_function("__beNtoleN").unwrap(),
                    &[
                        src.into(),
                        contract
                            .builder
                            .build_pointer_cast(
                                store,
                                contract.context.i8_type().ptr_type(AddressSpace::Generic),
                                "",
                            )
                            .into(),
                        contract
                            .context
                            .i32_type()
                            .const_int(len as u64, false)
                            .into(),
                    ],
                    "",
                );

                if len <= 8 && to.is_none() {
                    (
                        contract.builder.build_load(store, &format!("bytes{}", len)),
                        len as u64,
                    )
                } else {
                    (store.into(), len as u64)
                }
            }
            ast::PrimitiveType::Address => {
                let int_type = contract.context.custom_width_int_type(160);

                let store =
                    to.unwrap_or_else(|| contract.builder.build_alloca(int_type, "address"));

                // byte order needs to be reversed
                contract.builder.build_call(
                    contract.module.get_function("__beNtoleN").unwrap(),
                    &[
                        src.into(),
                        contract
                            .builder
                            .build_pointer_cast(
                                store,
                                contract.context.i8_type().ptr_type(AddressSpace::Generic),
                                "",
                            )
                            .into(),
                        contract
                            .context
                            .i32_type()
                            .const_int(ADDRESS_LENGTH, false)
                            .into(),
                    ],
                    "",
                );

                (store.into(), ADDRESS_LENGTH)
            }
            _ => unimplemented!(),
        }
    }

    /// recursively encode a single ty
    fn decode_ty<'b>(
        &self,
        contract: &'b Contract,
        function: FunctionValue,
        ty: &resolver::Type,
        to: Option<PointerValue<'b>>,
        data: &mut PointerValue<'b>,
    ) -> BasicValueEnum<'b> {
        match &ty {
            resolver::Type::Primitive(e) => {
                let (arg, arglen) = self.decode_primitive(contract, *e, to, *data);

                *data = unsafe {
                    contract.builder.build_gep(
                        *data,
                        &[contract.context.i32_type().const_int(arglen, false)],
                        "abi_ptr",
                    )
                };
                arg
            }
            resolver::Type::Enum(n) => self.decode_ty(
                contract,
                function,
                &resolver::Type::Primitive(contract.ns.enums[*n].ty),
                to,
                data,
            ),
            resolver::Type::Struct(n) => {
                let to =
                    to.unwrap_or_else(|| contract.builder.build_alloca(contract.llvm_type(ty), ""));

                for (i, field) in contract.ns.structs[*n].fields.iter().enumerate() {
                    let elem = unsafe {
                        contract.builder.build_gep(
                            to,
                            &[
                                contract.context.i32_type().const_zero(),
                                contract.context.i32_type().const_int(i as u64, false),
                            ],
                            &field.name,
                        )
                    };

                    if field.ty.is_reference_type() {
                        let val = contract
                            .builder
                            .build_alloca(contract.llvm_type(&field.ty), "");

                        self.decode_ty(contract, function, &field.ty, Some(val), data);

                        contract.builder.build_store(elem, val);
                    } else {
                        self.decode_ty(contract, function, &field.ty, Some(elem), data);
                    }
                }

                to.into()
            }
            resolver::Type::Array(_, dim) => {
                let to =
                    to.unwrap_or_else(|| contract.builder.build_alloca(contract.llvm_type(ty), ""));

                if let Some(d) = &dim[0] {
                    contract.emit_static_loop_with_pointer(
                        function,
                        0,
                        d.to_u64().unwrap(),
                        data,
                        |index: IntValue<'b>, data: &mut PointerValue<'b>| {
                            let elem = unsafe {
                                contract.builder.build_gep(
                                    to,
                                    &[contract.context.i32_type().const_zero(), index],
                                    "index_access",
                                )
                            };

                            let ty = ty.array_deref();

                            if ty.is_reference_type() {
                                let val = contract
                                    .builder
                                    .build_alloca(contract.llvm_type(&ty.deref()), "");
                                self.decode_ty(contract, function, &ty, Some(val), data);
                                contract.builder.build_store(elem, val);
                            } else {
                                self.decode_ty(contract, function, &ty, Some(elem), data);
                            }
                        },
                    );
                } else {
                    // FIXME
                }

                to.into()
            }
            resolver::Type::Undef => unreachable!(),
            resolver::Type::StorageRef(_) => unreachable!(),
            resolver::Type::Ref(ty) => self.decode_ty(contract, function, ty, to, data),
        }
    }

    /// ABI encode a single primitive
    fn encode_primitive(
        &self,
        contract: &Contract,
        ty: ast::PrimitiveType,
        dest: PointerValue,
        val: BasicValueEnum,
    ) -> u64 {
        match ty {
            ast::PrimitiveType::Bool => {
                let val = if val.is_pointer_value() {
                    contract.builder.build_load(val.into_pointer_value(), "")
                } else {
                    val
                };

                contract.builder.build_store(
                    dest,
                    contract.builder.build_int_z_extend(
                        val.into_int_value(),
                        contract.context.i8_type(),
                        "bool",
                    ),
                );
                1
            }
            ast::PrimitiveType::Uint(n) | ast::PrimitiveType::Int(n) => {
                let val = if val.is_pointer_value() {
                    contract.builder.build_load(val.into_pointer_value(), "")
                } else {
                    val
                };

                contract.builder.build_store(
                    contract.builder.build_pointer_cast(
                        dest,
                        val.into_int_value()
                            .get_type()
                            .ptr_type(AddressSpace::Generic),
                        "",
                    ),
                    val.into_int_value(),
                );

                n as u64 / 8
            }
            ast::PrimitiveType::Bytes(n) => {
                let val = if val.is_pointer_value() {
                    val.into_pointer_value()
                } else {
                    let temp = contract
                        .builder
                        .build_alloca(val.into_int_value().get_type(), &format!("bytes{}", n));

                    contract.builder.build_store(temp, val.into_int_value());

                    temp
                };

                // byte order needs to be reversed. e.g. hex"11223344" should be 0x10 0x11 0x22 0x33 0x44
                contract.builder.build_call(
                    contract.module.get_function("__leNtobeN").unwrap(),
                    &[
                        contract
                            .builder
                            .build_pointer_cast(
                                val,
                                contract.context.i8_type().ptr_type(AddressSpace::Generic),
                                "",
                            )
                            .into(),
                        dest.into(),
                        contract
                            .context
                            .i32_type()
                            .const_int(n as u64, false)
                            .into(),
                    ],
                    "",
                );

                n as u64
            }
            ast::PrimitiveType::Address => {
                // byte order needs to be reversed
                contract.builder.build_call(
                    contract.module.get_function("__leNtobeN").unwrap(),
                    &[
                        contract
                            .builder
                            .build_pointer_cast(
                                val.into_pointer_value(),
                                contract.context.i8_type().ptr_type(AddressSpace::Generic),
                                "",
                            )
                            .into(),
                        dest.into(),
                        contract
                            .context
                            .i32_type()
                            .const_int(ADDRESS_LENGTH, false)
                            .into(),
                    ],
                    "",
                );

                ADDRESS_LENGTH
            }
            _ => unimplemented!(),
        }
    }

    /// recursively encode argument. The encoded data is written to the data pointer,
    /// and the pointer is updated point after the encoded data.
    pub fn encode_ty<'a>(
        &self,
        contract: &'a Contract,
        function: FunctionValue,
        ty: &resolver::Type,
        arg: BasicValueEnum,
        data: &mut PointerValue<'a>,
    ) {
        match &ty {
            resolver::Type::Primitive(e) => {
                let arglen = self.encode_primitive(contract, *e, *data, arg);

                *data = unsafe {
                    contract.builder.build_gep(
                        *data,
                        &[contract.context.i32_type().const_int(arglen, false)],
                        "",
                    )
                };
            }
            resolver::Type::Enum(n) => {
                self.encode_primitive(contract, contract.ns.enums[*n].ty, *data, arg);
            }
            resolver::Type::Array(_, dim) => {
                if let Some(d) = &dim[0] {
                    contract.emit_static_loop_with_pointer(
                        function,
                        0,
                        d.to_u64().unwrap(),
                        data,
                        |index, data| {
                            let mut elem = unsafe {
                                contract.builder.build_gep(
                                    arg.into_pointer_value(),
                                    &[contract.context.i32_type().const_zero(), index],
                                    "index_access",
                                )
                            };

                            let ty = ty.array_deref();

                            if ty.is_reference_type() {
                                elem = contract.builder.build_load(elem, "").into_pointer_value()
                            }

                            self.encode_ty(contract, function, &ty, elem.into(), data);
                        },
                    );
                } else {
                    // FIXME
                }
            }
            resolver::Type::Struct(n) => {
                for (i, field) in contract.ns.structs[*n].fields.iter().enumerate() {
                    let mut elem = unsafe {
                        contract.builder.build_gep(
                            arg.into_pointer_value(),
                            &[
                                contract.context.i32_type().const_zero(),
                                contract.context.i32_type().const_int(i as u64, false),
                            ],
                            &field.name,
                        )
                    };

                    if field.ty.is_reference_type() {
                        elem = contract.builder.build_load(elem, "").into_pointer_value();
                    }

                    self.encode_ty(contract, function, &field.ty, elem.into(), data);
                }
            }
            resolver::Type::Undef => unreachable!(),
            resolver::Type::StorageRef(_) => unreachable!(),
            resolver::Type::Ref(ty) => {
                self.encode_ty(contract, function, ty, arg, data);
            }
        };
    }

    /// Return the encoded length of the given type
    pub fn encoded_length(&self, ty: &resolver::Type, contract: &resolver::Contract) -> u64 {
        match ty {
            resolver::Type::Primitive(ast::PrimitiveType::Bool) => 1,
            resolver::Type::Primitive(ast::PrimitiveType::Uint(n))
            | resolver::Type::Primitive(ast::PrimitiveType::Int(n)) => *n as u64 / 8,
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(n)) => *n as u64,
            resolver::Type::Primitive(ast::PrimitiveType::Address) => ADDRESS_LENGTH,
            resolver::Type::Primitive(_) => unreachable!(),
            resolver::Type::Enum(n) => {
                self.encoded_length(&resolver::Type::Primitive(contract.enums[*n].ty), contract)
            }
            resolver::Type::Struct(n) => contract.structs[*n]
                .fields
                .iter()
                .map(|f| self.encoded_length(&f.ty, contract))
                .sum(),
            resolver::Type::Array(ty, dims) => {
                self.encoded_length(ty, contract)
                    * dims
                        .iter()
                        .map(|d| match d {
                            Some(d) => d.to_u64().unwrap(),
                            None => 1,
                        })
                        .product::<u64>()
            }
            resolver::Type::Undef => unreachable!(),
            resolver::Type::StorageRef(_) => unreachable!(),
            resolver::Type::Ref(r) => self.encoded_length(r, contract),
        }
    }
}

impl TargetRuntime for SubstrateTarget {
    fn clear_storage<'a>(
        &self,
        contract: &'a Contract,
        _function: FunctionValue,
        slot: PointerValue<'a>,
    ) {
        contract.builder.build_call(
            contract.module.get_function("ext_set_storage").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        slot,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract.context.i32_type().const_zero().into(), // value_not_null, 0 for remove
                contract
                    .context
                    .i8_type()
                    .ptr_type(AddressSpace::Generic)
                    .const_null()
                    .into(),
                contract.context.i32_type().const_zero().into(),
            ],
            "",
        );
    }

    fn set_storage<'a>(
        &self,
        contract: &'a Contract,
        _function: FunctionValue,
        slot: PointerValue<'a>,
        dest: PointerValue<'a>,
    ) {
        // TODO: check for non-zero
        contract.builder.build_call(
            contract.module.get_function("ext_set_storage").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        slot,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract.context.i32_type().const_int(1, false).into(),
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

    /// Read from substrate storage
    fn get_storage<'a>(
        &self,
        contract: &'a Contract,
        function: FunctionValue,
        slot: PointerValue<'a>,
        dest: PointerValue<'a>,
    ) {
        let exists = contract
            .builder
            .build_call(
                contract.module.get_function("ext_get_storage").unwrap(),
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

        let exists = contract.builder.build_int_compare(
            IntPredicate::EQ,
            exists.into_int_value(),
            contract.context.i32_type().const_zero(),
            "storage_exists",
        );

        let clear_block = contract
            .context
            .append_basic_block(function, "not_in_storage");
        let retrieve_block = contract.context.append_basic_block(function, "in_storage");
        let done_storage = contract
            .context
            .append_basic_block(function, "done_storage");

        contract
            .builder
            .build_conditional_branch(exists, retrieve_block, clear_block);

        contract.builder.position_at_end(retrieve_block);

        contract.builder.build_call(
            contract.module.get_function("ext_scratch_read").unwrap(),
            &[
                contract
                    .builder
                    .build_pointer_cast(
                        dest,
                        contract.context.i8_type().ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into(),
                contract.context.i32_type().const_zero().into(),
                dest.get_type()
                    .get_element_type()
                    .into_int_type()
                    .size_of()
                    .const_cast(contract.context.i32_type(), false)
                    .into(),
            ],
            "",
        );

        contract.builder.build_unconditional_branch(done_storage);

        contract.builder.position_at_end(clear_block);

        contract.builder.build_store(
            dest,
            dest.get_type()
                .get_element_type()
                .into_int_type()
                .const_zero(),
        );

        contract.builder.build_unconditional_branch(done_storage);

        contract.builder.position_at_end(done_storage);
    }

    fn return_empty_abi(&self, contract: &Contract) {
        // This will clear the scratch buffer
        contract.builder.build_call(
            contract.module.get_function("ext_scratch_write").unwrap(),
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

        contract
            .builder
            .build_return(Some(&contract.context.i32_type().const_zero()));
    }

    fn return_abi<'b>(&self, contract: &'b Contract, data: PointerValue<'b>, length: IntValue) {
        contract.builder.build_call(
            contract.module.get_function("ext_scratch_write").unwrap(),
            &[data.into(), length.into()],
            "",
        );

        contract
            .builder
            .build_return(Some(&contract.context.i32_type().const_zero()));
    }

    fn assert_failure<'b>(&self, contract: &'b Contract) {
        contract.builder.build_unreachable();
    }

    fn abi_decode<'b>(
        &self,
        contract: &'b Contract,
        function: FunctionValue,
        args: &mut Vec<BasicValueEnum<'b>>,
        data: PointerValue<'b>,
        datalength: IntValue,
        spec: &resolver::FunctionDecl,
    ) {
        let length = spec
            .params
            .iter()
            .map(|arg| self.encoded_length(&arg.ty, contract.ns))
            .sum();

        let decode_block = contract.context.append_basic_block(function, "abi_decode");
        let wrong_length_block = contract
            .context
            .append_basic_block(function, "wrong_abi_length");

        let is_ok = contract.builder.build_int_compare(
            IntPredicate::EQ,
            datalength,
            contract.context.i32_type().const_int(length, false),
            "correct_length",
        );

        contract
            .builder
            .build_conditional_branch(is_ok, decode_block, wrong_length_block);

        contract.builder.position_at_end(wrong_length_block);
        contract.builder.build_unreachable();

        contract.builder.position_at_end(decode_block);

        let mut argsdata = contract.builder.build_pointer_cast(
            data,
            contract.context.i8_type().ptr_type(AddressSpace::Generic),
            "",
        );

        for param in &spec.params {
            args.push(self.decode_ty(contract, function, &param.ty, None, &mut argsdata));
        }
    }

    ///  ABI encode the return values for the function
    fn abi_encode<'b>(
        &self,
        contract: &'b Contract,
        function: FunctionValue,
        args: &[BasicValueEnum<'b>],
        spec: &resolver::FunctionDecl,
    ) -> (PointerValue<'b>, IntValue<'b>) {
        let length = spec
            .returns
            .iter()
            .map(|arg| self.encoded_length(&arg.ty, contract.ns))
            .sum();

        let length = contract.context.i32_type().const_int(length, false);

        let data = contract
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

        let mut argsdata = data;

        for (i, arg) in spec.returns.iter().enumerate() {
            let val = if arg.ty.is_reference_type() {
                contract
                    .builder
                    .build_load(args[i].into_pointer_value(), "")
            } else {
                args[i]
            };

            self.encode_ty(contract, function, &arg.ty, val, &mut argsdata);
        }

        (data, length)
    }
}
