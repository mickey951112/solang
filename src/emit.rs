
use resolver;
use ast;
use cfg;
use std::ptr::null_mut;
use std::ffi::{CString, CStr};
use std::str;
use std::slice;

use std::collections::VecDeque;
use std::collections::HashMap;

use llvm_sys::LLVMIntPredicate;
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::target_machine::*;
use tiny_keccak::Keccak;

const TRIPLE: &'static [u8] = b"wasm32-unknown-unknown-wasm\0";

#[allow(dead_code)]
const LLVM_FALSE: LLVMBool = 0;
const LLVM_TRUE: LLVMBool = 1;

lazy_static::lazy_static! {
    static ref LLVM_INIT: () = unsafe {
        LLVMInitializeWebAssemblyTargetInfo();
        LLVMInitializeWebAssemblyTarget();
        LLVMInitializeWebAssemblyTargetMC();
        LLVMInitializeWebAssemblyAsmPrinter();
        LLVMInitializeWebAssemblyAsmParser();
        LLVMInitializeWebAssemblyDisassembler();
    };
}

fn target_machine() -> LLVMTargetMachineRef {
    let mut target = null_mut();
    let mut err_msg_ptr = null_mut();
    unsafe {
        if LLVMGetTargetFromTriple(TRIPLE.as_ptr() as *const _, &mut target, &mut err_msg_ptr) == LLVM_TRUE {
            let err_msg_cstr = CStr::from_ptr(err_msg_ptr as *const _);
            let err_msg = str::from_utf8(err_msg_cstr.to_bytes()).unwrap();
            panic!("failed to create llvm target: {}", err_msg);
        }
    }

    unsafe {
        LLVMCreateTargetMachine(target,
                                TRIPLE.as_ptr() as *const _,
                                b"generic\0".as_ptr() as *const _,
                                b"\0".as_ptr() as *const _,
                                LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
                                LLVMRelocMode::LLVMRelocDefault,
                                LLVMCodeModel::LLVMCodeModelDefault)
    }
}

#[derive(Clone)]
struct Variable {
    value_ref: LLVMValueRef,
    stack: bool,
}

pub struct Contract<'a> {
    pub name: String,
    pub module: LLVMModuleRef,
    context: LLVMContextRef,
    tm: LLVMTargetMachineRef,
    ns: &'a resolver::ContractNameSpace,
    functions: Vec<LLVMValueRef>,
    be32toleN: LLVMValueRef,
    init_heap: LLVMValueRef,
}

impl<'a> Contract<'a> {
    pub fn dump_llvm(&self) {
        unsafe {
            LLVMDumpModule(self.module);
        }
    }

    pub fn wasm(&self) -> Result<Vec<u8>, String> {
        let mut obj_error = null_mut();
        let mut memory_buffer = null_mut();

        unsafe {
            let result = LLVMTargetMachineEmitToMemoryBuffer(self.tm,
                                                    self.module,
                                                    LLVMCodeGenFileType::LLVMObjectFile,
                                                    &mut obj_error,
                                                    &mut memory_buffer);

            if result != 0 {
                Err(CStr::from_ptr(obj_error as *const _).to_string_lossy().to_string())
            } else {
                let obj = slice::from_raw_parts(
                    LLVMGetBufferStart(memory_buffer) as *const u8,
                    LLVMGetBufferSize(memory_buffer) as usize
                );
                let res = obj.to_vec();
                LLVMDisposeMemoryBuffer(memory_buffer);
                Ok(res)
            }
        }
    }

    pub fn new(contract: &'a resolver::ContractNameSpace, filename: &str) -> Self {
        lazy_static::initialize(&LLVM_INIT);

        let contractname = CString::new(contract.name.to_string()).unwrap();

        let mut e = Contract{
            name: contract.name.to_string(),
            module: unsafe { LLVMModuleCreateWithName(contractname.as_ptr()) },
            context: unsafe { LLVMContextCreate() },
            tm: target_machine(),
            ns: contract,
            functions: Vec::new(),
            be32toleN: null_mut(),
            init_heap: null_mut(),
        };

        // intrinsics
        let ret = unsafe { LLVMVoidType() };
        let mut args = vec![
            unsafe { LLVMPointerType(LLVMInt32TypeInContext(e.context), 0) },
            unsafe { LLVMPointerType(LLVMInt32TypeInContext(e.context), 0) },
            unsafe { LLVMInt32TypeInContext(e.context) },
        ];

        let ftype = unsafe { LLVMFunctionType(ret, args.as_mut_ptr(), args.len() as _, 0) };

        e.be32toleN = unsafe {
            LLVMAddFunction(e.module, "__be32toleN\0".as_ptr() as *const _, ftype)
        };

        let init_heap_ftype = unsafe { LLVMFunctionType(ret, null_mut(), 0, 0) };

        e.init_heap = unsafe {
            LLVMAddFunction(e.module, "__init_heap\0".as_ptr() as *const _, init_heap_ftype)
        };

        unsafe {
            LLVMSetTarget(e.module, TRIPLE.as_ptr() as *const _);
            LLVMSetSourceFileName(e.module, filename.as_ptr() as *const _, filename.len() as _);
            let builder = LLVMCreateBuilderInContext(e.context);

            for func in &contract.functions {
                let f = e.emit_func(func, builder);
                e.functions.push(f);
            }

            e.emit_constructor_dispatch(contract, builder);
            e.emit_function_dispatch(contract, builder);

            LLVMDisposeBuilder(builder);
        }

        e
    }

    fn expression(&self, builder: LLVMBuilderRef, e: &cfg::Expression, vartab: &Vec<Variable>) -> LLVMValueRef {
        match e {
            cfg::Expression::NumberLiteral(bits, n) => {
                let ty = unsafe { LLVMIntTypeInContext(self.context, *bits as _) };
                let s = n.to_string();

                unsafe {
                    LLVMConstIntOfStringAndSize(ty, s.as_ptr() as *const _, s.len() as _, 10)
                }
            },
            cfg::Expression::Add(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildAdd(builder, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::Subtract(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildSub(builder, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::Multiply(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildMul(builder, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::UDivide(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildUDiv(builder, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::SDivide(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildSDiv(builder, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::Equal(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntEQ, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::More(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntSGT, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::Less(l, r) => {
                let left = self.expression(builder, l, vartab);
                let right = self.expression(builder, r, vartab);

                unsafe {
                    LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntSLT, left, right, b"\0".as_ptr() as *const _)
                }
            },
            cfg::Expression::Variable(_, s) => {
                if vartab[*s].stack {
                    unsafe {
                        LLVMBuildLoad(builder, vartab[*s].value_ref, b"\0".as_ptr() as *const _)
                    }
                } else {
                    vartab[*s].value_ref
                }
            },
            cfg::Expression::ZeroExt(t, e) => {
                let e = self.expression(builder, e, vartab);
                let ty = t.LLVMType(self.ns, self.context);

                unsafe {
                    LLVMBuildZExt(builder, e, ty, b"\0".as_ptr() as *const _)
                }
            },
            _ => {
                panic!("expression not implemented");
            }
        }
    }

    fn emit_constructor_dispatch(&self, contract: &resolver::ContractNameSpace, builder: LLVMBuilderRef) {
        // create start function
        let ret = unsafe { LLVMVoidType() };
        let mut args = vec![ unsafe { LLVMPointerType(LLVMInt32TypeInContext(self.context), 0) } ];
        let ftype = unsafe { LLVMFunctionType(ret, args.as_mut_ptr(), args.len() as _, 0) };
        let fname = CString::new("__constructor_solabi").unwrap();
        let function = unsafe { LLVMAddFunction(self.module, fname.as_ptr(), ftype) };
        let entry = unsafe { LLVMAppendBasicBlockInContext(self.context, function, "entry\0".as_ptr() as *const _) };

        unsafe {
            LLVMPositionBuilderAtEnd(builder, entry);
            LLVMBuildCall(builder, self.init_heap, null_mut(), 0, "\0".as_ptr() as *const _);
        }

        if let Some(n) = contract.constructor_function() {
            let mut args = Vec::new();

            let arg = unsafe { LLVMGetParam(function, 0) };
            let length = unsafe { LLVMBuildLoad(builder, arg, "length\0".as_ptr() as *const _) };

            // step over length
            let mut index_one = unsafe { LLVMConstInt(LLVMInt32TypeInContext(self.context), 1, LLVM_FALSE) };
            let args_ptr = unsafe { LLVMBuildGEP(builder, arg, &mut index_one, 1 as _, "fid_ptr\0".as_ptr() as *const _) };

            // insert abi decode
            self.emit_abi_decode(builder, &mut args, args_ptr, length, &contract.functions[n]);

            unsafe {
                LLVMBuildCall(builder, self.functions[n], args.as_mut_ptr(), args.len() as _, "\0".as_ptr() as *const _);
            }
        }

        unsafe {
            LLVMBuildRetVoid(builder);
        }
    }

    fn emit_function_dispatch(&self, contract: &resolver::ContractNameSpace, builder: LLVMBuilderRef) {
        // create start function
        let ret = unsafe { LLVMVoidType() };
        let mut args = vec![ unsafe { LLVMPointerType(LLVMInt32TypeInContext(self.context), 0) } ];
        let ftype = unsafe { LLVMFunctionType(ret, args.as_mut_ptr(), args.len() as _, 0) };
        let fname  = CString::new("__function_solabi").unwrap();
        let function = unsafe { LLVMAddFunction(self.module, fname.as_ptr(), ftype) };
        let entry = unsafe { LLVMAppendBasicBlockInContext(self.context, function, "entry\0".as_ptr() as *const _) };
        let fallback_bb = unsafe { LLVMAppendBasicBlockInContext(self.context, function, "fallback\0".as_ptr() as *const _) };
        let switch_bb = unsafe { LLVMAppendBasicBlockInContext(self.context, function, "switch\0".as_ptr() as *const _) };
        unsafe { LLVMPositionBuilderAtEnd(builder, entry); }
        let arg = unsafe { LLVMGetParam(function, 0) };
        let length = unsafe { LLVMBuildLoad(builder, arg, "length\0".as_ptr() as *const _) };

        let not_fallback = unsafe { LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntUGE,
                    length, LLVMConstInt(LLVMInt32TypeInContext(self.context), 4, LLVM_FALSE),
                    "not_fallback\0".as_ptr() as *const _) };

        unsafe { LLVMBuildCondBr(builder, not_fallback, switch_bb, fallback_bb); }

        unsafe { LLVMPositionBuilderAtEnd(builder, switch_bb); }

        // step over length
        let mut index_one = unsafe { LLVMConstInt(LLVMInt32TypeInContext(self.context), 1, LLVM_FALSE) };
        let fid_ptr = unsafe { LLVMBuildGEP(builder, arg, &mut index_one, 1 as _, "fid_ptr\0".as_ptr() as *const _) };
        let id = unsafe { LLVMBuildLoad(builder, fid_ptr, "fid\0".as_ptr() as *const _) };
        let nomatch_bb = unsafe { LLVMAppendBasicBlockInContext(self.context, function, "no_match\0".as_ptr() as *const _) };

        // pointer/size for abi decoding
        let mut index_two = unsafe { LLVMConstInt(LLVMInt32TypeInContext(self.context), 2, LLVM_FALSE) };
        let args_ptr = unsafe { LLVMBuildGEP(builder, arg, &mut index_two, 1 as _, "args_ptr\0".as_ptr() as *const _) };
        let args_len = unsafe { LLVMBuildSub(builder,
                                    length,
                                    LLVMConstInt(LLVMInt32TypeInContext(self.context), 2, LLVM_FALSE),
                                    "args_len\0".as_ptr() as *const _) };
        let switch = unsafe {
            LLVMBuildSwitch(builder, id, nomatch_bb, contract.functions.iter().filter(|x| x.name != None).count() as _)
        };

        unsafe { LLVMPositionBuilderAtEnd(builder, nomatch_bb); }
        unsafe { LLVMBuildUnreachable(builder); }

        for (i, f) in contract.functions.iter().enumerate() {
            // ignore constructors and fallback
            if f.name == None {
                continue;
            }
            let mut sha3 = Keccak::new_sha3_256();
            let mut res: [u8; 32] = [0; 32];

            sha3.update(f.sig.as_bytes());
            sha3.finalize(&mut res);

            let bb = unsafe { LLVMAppendBasicBlockInContext(self.context, function, "\0".as_ptr() as *const _) };
            let fid = u32::from_le_bytes([ res[0], res[1], res[2], res[3] ]);

            unsafe {
                LLVMAddCase(switch,
                    LLVMConstInt(LLVMIntTypeInContext(self.context, 32), fid as _, LLVM_FALSE),
                    bb);
            }

            unsafe { LLVMPositionBuilderAtEnd(builder, bb); }

            let mut args = Vec::new();

            // insert abi decode
            self.emit_abi_decode(builder, &mut args, args_ptr, args_len, f);

            unsafe {
                LLVMBuildCall(builder, self.functions[i], args.as_mut_ptr(), args.len() as _, "\0".as_ptr() as *const _);

                // insert abi decode
                LLVMBuildRetVoid(builder);
            }
        }

        // emit fallback code
        unsafe { LLVMPositionBuilderAtEnd(builder, fallback_bb); }
        match contract.fallback_function() {
            Some(n) => {
                let mut args = Vec::new();

                unsafe {
                    LLVMBuildCall(builder, self.functions[n], args.as_mut_ptr(), args.len() as _, "\0".as_ptr() as *const _);
                    LLVMBuildRetVoid(builder);
                }
            },
            None => {
                unsafe {
                    LLVMBuildUnreachable(builder);
                }
            }
        }
    }

    fn emit_abi_decode(&self, builder: LLVMBuilderRef, args: &mut Vec<LLVMValueRef>, data: LLVMValueRef, length: LLVMValueRef, spec: &resolver::FunctionDecl) {
        let mut data = data;

        for arg in &spec.params {
            let ty = match &arg.ty {
                resolver::TypeName::Elementary(e) => e,
                resolver::TypeName::Enum(n) => &self.ns.enums[*n].ty
            };

            args.push(match ty {
                ast::ElementaryTypeName::Bool => {
                    // solidity checks all the 32 bytes for being non-zero; we will just look at the upper 8 bytes, else we would need four loads
                    // which is unneeded (hopefully)
                    // cast to 64 bit pointer
                    let bool_ptr = unsafe {
                        LLVMBuildPointerCast(builder, data, LLVMPointerType(LLVMInt64TypeInContext(self.context), 0), "\0".as_ptr() as *const _) };
                    // get third 64 bit value
                    let mut three = unsafe { LLVMConstInt(LLVMInt32TypeInContext(self.context), 3, LLVM_FALSE) };
                    let mut zero = unsafe { LLVMConstInt(LLVMInt64TypeInContext(self.context), 0, LLVM_FALSE) };
                    let bool_ptr = unsafe { LLVMBuildGEP(builder, bool_ptr, &mut three, 1 as _, "bool_ptr\0".as_ptr() as *const _) };
                    let bool_ = unsafe { LLVMBuildLoad(builder, bool_ptr, "bool\0".as_ptr() as *const _) };
                    unsafe { LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntEQ, bool_, zero, "iszero\0".as_ptr() as *const _) }
                },
                ast::ElementaryTypeName::Uint(8) |
                ast::ElementaryTypeName::Int(8) => {
                    let mut int8_ptr = unsafe {
                        LLVMBuildPointerCast(builder, data, LLVMPointerType(LLVMInt8TypeInContext(self.context), 0), "\0".as_ptr() as *const _)
                    };
                    let mut thirtyone = unsafe { LLVMConstInt(LLVMInt32TypeInContext(self.context), 31, LLVM_FALSE) };
                    int8_ptr = unsafe { LLVMBuildGEP(builder, int8_ptr, &mut thirtyone, 1 as _, "int8_ptr\0".as_ptr() as *const _) };
                    unsafe { LLVMBuildLoad(builder, int8_ptr, "int8\0".as_ptr() as *const _) }
                },
                ast::ElementaryTypeName::Uint(n) |
                ast::ElementaryTypeName::Int(n) => {
                    // FIXME: can be much shorter for uint8 without allocation
                    // no need to allocate space for each uint64
                    // allocate enough for type
                    let int_type = unsafe { LLVMIntTypeInContext(self.context, *n as u32) };
                    let type_size = unsafe { LLVMSizeOf(int_type) };

                    let store = unsafe {
                        LLVMBuildAlloca(builder, int_type, "stack\0".as_ptr() as *const _)
                    };

                    let mut args = vec![
                        // from
                        data,
                        // to
                        unsafe {
                            LLVMBuildPointerCast(builder, store, LLVMPointerType(LLVMInt32TypeInContext(self.context), 0), "\0".as_ptr() as *const _)
                        },
                        // type_size
                        unsafe {
                            LLVMBuildTrunc(builder, type_size, LLVMInt32TypeInContext(self.context), "size\0".as_ptr() as *const _)
                        }
                    ];
                    unsafe {
                        LLVMBuildCall(builder, self.be32toleN, args.as_mut_ptr(), args.len() as _, "\0".as_ptr() as *const _);
                    }

                    if *n <= 64 {
                        unsafe {
                            LLVMBuildLoad(builder, store, "\0".as_ptr() as *const _)
                        }
                    } else {
                        store
                    }
                },
                _ => panic!()
            });

            let mut eight = unsafe { LLVMConstInt(LLVMInt64TypeInContext(self.context), 8, LLVM_FALSE) };
            data = unsafe { LLVMBuildGEP(builder, data, &mut eight, 1 as _, "data_next\0".as_ptr() as *const _) };
        }
    }

    fn emit_func(&self, f: &resolver::FunctionDecl, builder: LLVMBuilderRef) -> LLVMValueRef {
        let mut args = vec!();

        for p in &f.params {
            args.push(p.ty.LLVMType(self.ns, self.context));
        }

        let fname = if f.constructor {
            CString::new("__constructor").unwrap()
        } else if let Some(ref name) = f.name {
            CString::new(name.to_string()).unwrap()
        } else {
            CString::new("__fallback").unwrap()
        };

        let ret = match f.returns.len() {
            0 => unsafe { LLVMVoidType() },
            1 => f.returns[0].ty.LLVMType(self.ns, self.context),
            _ => panic!("only functions with one return value implemented".to_string())
        };

        let ftype = unsafe { LLVMFunctionType(ret, args.as_mut_ptr(), args.len() as _, 0) };

        let function = unsafe { LLVMAddFunction(self.module, fname.as_ptr(), ftype) };

        let cfg = match f.cfg {
            Some(ref cfg) => cfg,
            None => panic!()
        };

        // recurse through basic blocks
        struct BasicBlock {
            bb: LLVMBasicBlockRef,
            phis: HashMap<usize, LLVMValueRef>,
        }

        struct Work {
            bb_no: usize,
            vars: Vec<Variable>
        }

        let mut blocks : HashMap<usize, BasicBlock> = HashMap::new();

        let create_bb = |bb_no| -> BasicBlock {
            let cfg_bb : &cfg::BasicBlock = &cfg.bb[bb_no];
            let mut phis = HashMap::new();

            let bb_name = CString::new(cfg_bb.name.to_string()).unwrap();
            let bb = unsafe { LLVMAppendBasicBlockInContext(self.context, function, bb_name.as_ptr() as *const _) };

            unsafe { LLVMPositionBuilderAtEnd(builder, bb); }

            if let Some(ref cfg_phis) = cfg_bb.phis {
                for v in cfg_phis {
                    // FIXME: no phis needed for stack based vars
                    let ty = cfg.vars[*v].ty.LLVMType(self.ns, self.context);
                    let name = CString::new(cfg.vars[*v].id.name.to_string()).unwrap();

                    phis.insert(*v, unsafe {
                        LLVMBuildPhi(builder, ty, name.as_ptr() as *const _)
                    });
                }
            }

            BasicBlock{bb, phis}
        };

        let mut work = VecDeque::new();

        blocks.insert(0, create_bb(0));

        // Create all the stack variables
        let mut vars = Vec::new();

        for v in &cfg.vars {
            if v.ty.stack_based() {
                let name = CString::new(v.id.name.to_string()).unwrap();

                vars.push(Variable{
                    value_ref: unsafe {
                        LLVMBuildAlloca(builder, v.ty.LLVMType(self.ns, self.context), name.as_ptr() as *const _)
                    },
                    stack: true,
                });
            } else {
                vars.push(Variable{
                    value_ref: null_mut(),
                    stack: false,
                });
            }
        }

        work.push_back(Work{
            bb_no: 0,
            vars: vars,
        });

        loop {
            let mut w = match work.pop_front() {
                Some(w) => w,
                None => break,
            };

            // ensure reference to blocks is short-lived
            let mut ll_bb = {
                let bb = blocks.get(&w.bb_no).unwrap();

                unsafe { LLVMPositionBuilderAtEnd(builder, bb.bb); }

                for (v, phi) in bb.phis.iter() {
                    w.vars[*v].value_ref = *phi;
                }

                bb.bb
            };

            for ins in &cfg.bb[w.bb_no].instr {
                match ins {
                    cfg::Instr::FuncArg{ res, arg } => {
                        w.vars[*res].value_ref = unsafe { LLVMGetParam(function, *arg as u32) };
                    },
                    cfg::Instr::Return{ value } if value.is_empty() => {
                        unsafe {
                            LLVMBuildRetVoid(builder);
                        }
                    },
                    cfg::Instr::Return{ value } if value.len() == 1 => {
                        let retval = self.expression(builder, &value[0], &w.vars);
                        unsafe {
                            LLVMBuildRet(builder, retval);
                        }
                    },
                    cfg::Instr::Set{ res, expr } => {
                        let value_ref = self.expression(builder, expr, &w.vars);
                        if w.vars[*res].stack {
                            unsafe { LLVMBuildStore(builder, value_ref, w.vars[*res].value_ref); }

                        } else {
                            w.vars[*res].value_ref = value_ref;
                        }
                    },
                    cfg::Instr::Branch{ bb: dest } => {
                        if !blocks.contains_key(&dest) {
                            blocks.insert(*dest, create_bb(*dest));
                            work.push_back(Work{
                                bb_no: *dest,
                                vars: w.vars.clone()
                            });
                        }

                        let bb = blocks.get(dest).unwrap();

                        for (v, phi) in bb.phis.iter() {
                            unsafe {
                                LLVMAddIncoming(*phi, &mut w.vars[*v].value_ref, &mut ll_bb, 1);
                            }
                        }

                        unsafe {
                            LLVMPositionBuilderAtEnd(builder, ll_bb);
                            LLVMBuildBr(builder, bb.bb);
                        }
                    },
                    cfg::Instr::BranchCond{ cond, true_, false_ } => {
                        let cond = self.expression(builder, cond, &w.vars);

                        let bb_true = {
                            if !blocks.contains_key(&true_) {
                                blocks.insert(*true_, create_bb(*true_));
                                work.push_back(Work{
                                    bb_no: *true_,
                                    vars: w.vars.clone()
                                });
                            }

                            let bb = blocks.get(true_).unwrap();

                            for (v, phi) in bb.phis.iter() {
                                unsafe {
                                    LLVMAddIncoming(*phi, &mut w.vars[*v].value_ref, &mut ll_bb, 1);
                                }
                            }

                            bb.bb
                        };

                        let bb_false = {
                            if !blocks.contains_key(&false_) {
                                blocks.insert(*false_, create_bb(*false_));
                                work.push_back(Work{
                                    bb_no: *false_,
                                    vars: w.vars.clone()
                                });
                            }

                            let bb = blocks.get(false_).unwrap();

                            for (v, phi) in bb.phis.iter() {
                                unsafe {
                                    LLVMAddIncoming(*phi, &mut w.vars[*v].value_ref, &mut ll_bb, 1);
                                }
                            }

                            bb.bb
                        };

                        unsafe {
                            LLVMPositionBuilderAtEnd(builder, ll_bb);
                            LLVMBuildCondBr(builder, cond, bb_true, bb_false);
                        }
                    },
                    _ => {
                        unreachable!();
                    }
                }
            }
        }

        function
    }
}

impl<'a> Drop for Contract<'a> {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeModule(self.module);
            LLVMContextDispose(self.context);
            LLVMDisposeTargetMachine(self.tm);
        }
    }
}

impl ast::ElementaryTypeName {
    #[allow(non_snake_case)]
    fn LLVMType(&self, context: LLVMContextRef) -> LLVMTypeRef {
        match self {
            ast::ElementaryTypeName::Bool => unsafe { LLVMInt1TypeInContext(context) },
            ast::ElementaryTypeName::Int(n) => unsafe { LLVMIntTypeInContext(context, *n as _) },
            ast::ElementaryTypeName::Uint(n) => unsafe { LLVMIntTypeInContext(context, *n as _) },
            ast::ElementaryTypeName::Address => unsafe { LLVMIntTypeInContext(context, 20*8) },
            ast::ElementaryTypeName::Bytes(n) => unsafe { LLVMIntTypeInContext(context, (*n * 8) as _) },
            _ => {
                panic!("llvm type for {:?} not implemented", self);
            }
        }
    }

    fn stack_based(&self) -> bool {
        match self {
            ast::ElementaryTypeName::Bool => false,
            ast::ElementaryTypeName::Int(n) => *n > 64,
            ast::ElementaryTypeName::Uint(n) => *n > 64,
            ast::ElementaryTypeName::Address => true,
            ast::ElementaryTypeName::Bytes(n) => *n > 8,
            _ => unimplemented!()
        }
    }
}

impl resolver::TypeName {
    #[allow(non_snake_case)]
    fn LLVMType(&self, ns: &resolver::ContractNameSpace, context: LLVMContextRef) -> LLVMTypeRef {
        match self {
            resolver::TypeName::Elementary(e) => e.LLVMType(context),
            resolver::TypeName::Enum(n) => { ns.enums[*n].ty.LLVMType(context) },
        }
    }

    fn stack_based(&self) -> bool {
        match self {
            resolver::TypeName::Elementary(e) => e.stack_based(),
            resolver::TypeName::Enum(_) => false,
        }
    }
}
