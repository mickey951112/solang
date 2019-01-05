use ast::*;
use std::ptr::null_mut;
use std::ffi::{CString, CStr};
use std::collections::HashMap;
use std::str;
use resolve::*;
use vartable::*;

use llvm_sys::LLVMIntPredicate;
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::target_machine::*;

const TRIPLE: &'static [u8] = b"wasm32-unknown-unknown-wasm\0";

const LLVM_FALSE: LLVMBool = 0;
const LLVM_TRUE: LLVMBool = 1;

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

pub fn emit(s: SourceUnit) {
    let context;

    unsafe {
        LLVMInitializeWebAssemblyTargetInfo();
        LLVMInitializeWebAssemblyTarget();
        LLVMInitializeWebAssemblyTargetMC();
        LLVMInitializeWebAssemblyAsmPrinter();
        LLVMInitializeWebAssemblyAsmParser();
        LLVMInitializeWebAssemblyDisassembler();

        context = LLVMContextCreate();
    }

    let tm = target_machine();

    for part in &s.1 {
        if let SourceUnitPart::ContractDefinition(ref contract) = part {
            let contractname = CString::new(contract.1.to_string()).unwrap();
            let filename = CString::new(contract.1.to_string() + ".wasm").unwrap();

            unsafe {
                let module = LLVMModuleCreateWithName(contractname.as_ptr());
                LLVMSetTarget(module, TRIPLE.as_ptr() as *const _);
                let mut builder = LLVMCreateBuilderInContext(context);
                let mut obj_error = null_mut();

                for m in &contract.2 {
                    if let ContractPart::FunctionDefinition(ref func) = m {
                        if let Err(s) = emit_func(func, context, module, builder) {
                            println!("failed to compile: {}", s);
                        }
                    }
                }
                let result = LLVMTargetMachineEmitToFile(tm,
                                                        module,
                                                        filename.as_ptr() as *mut i8,
                                                        LLVMCodeGenFileType::LLVMObjectFile,
                                                        &mut obj_error);

                if result != 0 {
                    println!("obj_error: {:?}", CStr::from_ptr(obj_error as *const _));
                }

                //LLVMDumpModule(module);
                LLVMDisposeBuilder(builder);
                LLVMDisposeModule(module);
            }
        }
    }

    unsafe {
        LLVMContextDispose(context);
        LLVMDisposeTargetMachine(tm);
    }
}

unsafe fn emit_func(f: &FunctionDefinition, context: LLVMContextRef, module: LLVMModuleRef, builder: LLVMBuilderRef) -> Result<(), String> {
    let mut args = vec!();

    for p in &f.params {
        args.push(p.typ.LLVMType(context));
    }

    let fname = match f.name {
        None => {
            return Err("function with no name are not implemented yet".to_string());
        },
        Some(ref n) => {
            CString::new(n.to_string()).unwrap()
        }
    };
 
    let ret = match f.returns.len() {
        0 => LLVMVoidType(),
        1 => f.returns[0].typ.LLVMType(context),
        _ => return Err("only functions with one return value implemented".to_string())
    };

    let ftype = LLVMFunctionType(ret, args.as_mut_ptr(), args.len() as _, 0);

    let function = LLVMAddFunction(module, fname.as_ptr(), ftype);

    let bb = LLVMAppendBasicBlockInContext(context, function, b"entry\0".as_ptr() as *const _);

    LLVMPositionBuilderAtEnd(builder, bb);

    let mut emitter = FunctionEmitter{
        context: context,
        builder: builder, 
        vartable: Vartable::new(),
        basicblock: bb,
        llfunction: function,
        function: &f
    };

    // create variable table
    if let Some(ref vartable) = f.vartable {
        for (name, typ) in vartable {
            emitter.vartable.insert(name, *typ, LLVMConstInt(typ.LLVMType(context), 0, LLVM_FALSE));
        }
    }

    let mut i = 0;
    for p in &f.params {
        // Unnamed function arguments are not accessible
        if let Some(ref argname) = p.name {
            emitter.vartable.insert(argname, p.typ, LLVMGetParam(function, i));
        }
        i += 1;
    }

    visit_statement(&f.body, &mut |s| {
        if let Statement::VariableDefinition(v, Some(e)) = s {
            let value = emitter.expression(e, v.typ)?;

            emitter.vartable.insert(&v.name, v.typ, value);
        }
        Ok(())
    })?;

    emitter.statement(&f.body)
}

impl ElementaryTypeName {
    #[allow(non_snake_case)]
    fn LLVMType(&self, context: LLVMContextRef) -> LLVMTypeRef {
        match self {
            ElementaryTypeName::Bool => unsafe { LLVMInt1TypeInContext(context) },
            ElementaryTypeName::Int(n) => unsafe { LLVMIntTypeInContext(context, *n as _) },
            ElementaryTypeName::Uint(n) => unsafe { LLVMIntTypeInContext(context, *n as _) },
            _ => {
                panic!("llvm type for {:?} not implemented", self);
            }
        }
    }

    fn signed(&self) -> bool {
        match self {
            ElementaryTypeName::Int(_) => true,
            _ => false
        }
    }
}

struct FunctionEmitter<'a> {
    context: LLVMContextRef,
    builder: LLVMBuilderRef,
    llfunction: LLVMValueRef,
    basicblock: LLVMBasicBlockRef,
    function: &'a FunctionDefinition,
    vartable: Vartable,
}

impl<'a> FunctionEmitter<'a> {
    fn statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match stmt {
            Statement::VariableDefinition(_, _) => {
                // variables   
            },
            Statement::BlockStatement(block) => {
                for st in &block.0 {
                    self.statement(st)?;
                }
            },
            Statement::Return(None) => {
                unsafe {
                    LLVMBuildRetVoid(self.builder);
                }
            }
            Statement::Return(Some(expr)) => {
                let v = self.expression(expr, self.function.returns[0].typ)?;

                unsafe {
                    LLVMBuildRet(self.builder, v);
                }
            },
            Statement::Expression(expr) => {
                self.expression(expr, ElementaryTypeName::Any)?;
            }
            Statement::Empty => {
                // nop
            },
            Statement::If(cond, then, else_) => {
                let ifbb = self.basicblock;
                let thenbb = unsafe {
                    LLVMAppendBasicBlockInContext(self.context, self.llfunction, b"then\0".as_ptr() as *const _)
                };
                let endifbb = unsafe {
                    LLVMAppendBasicBlockInContext(self.context, self.llfunction, b"endif\0".as_ptr() as *const _)
                };
                let elsebb = match else_ {
                    box Some(_) => unsafe {
                        Some(LLVMAppendBasicBlockInContext(self.context, self.llfunction, b"else\0".as_ptr() as *const _))
                    },
                    box None => None
                };
                
                let v = self.expression(cond, ElementaryTypeName::Bool)?;

                unsafe {
                    LLVMBuildCondBr(self.builder, v, thenbb, match elsebb { Some(b) => b, None => endifbb});
                    LLVMPositionBuilderAtEnd(self.builder, thenbb);
                }

                self.basicblock = thenbb;

                self.vartable.new_scope();

                self.statement(then)?;

                unsafe {
                    LLVMBuildBr(self.builder, endifbb);
                }

                let thenlastbb = self.basicblock;
                let thenscope = self.vartable.leave_scope();

                let mut elsescope = if let Some(bb) = elsebb {
                    unsafe {
                        LLVMPositionBuilderAtEnd(self.builder, bb);
                    }

                    self.basicblock = bb;

                    self.vartable.new_scope();

                    if let box Some(e) = else_ {
                        self.statement(e)?;
                    }

                    unsafe {
                        LLVMBuildBr(self.builder, endifbb);
                    }

                    self.vartable.leave_scope()
                } else {
                    HashMap::new()
                };
                
                unsafe {
                    LLVMPositionBuilderAtEnd(self.builder, endifbb);
                }

                // create phi nodes
                for (name, var) in thenscope {
                    let typ = self.vartable.get_type(&name);
                    let cvalue = self.vartable.get_value(&name);
                    let phi = unsafe {
                        LLVMBuildPhi(self.builder, typ.LLVMType(self.context), b"\0".as_ptr() as *const _)
                    };

                    let mut values = vec!(cvalue, var.value);
                    let mut blocks = vec!(ifbb, thenlastbb);

                    if let Some(var) = elsescope.remove(&name) {
                        values.push(var.value);
                        blocks.push(self.basicblock);
                    }

                    unsafe {
                        LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), values.len() as _);
                    }

                    self.vartable.set_value(&name, phi);
                }

                // rest of else scope
                for (name, var) in elsescope {
                    let typ = self.vartable.get_type(&name);
                    let cvalue = self.vartable.get_value(&name);
                    let phi = unsafe {
                        LLVMBuildPhi(self.builder, typ.LLVMType(self.context), b"\0".as_ptr() as *const _)
                    };

                    let mut values = vec!(cvalue, var.value);
                    let mut blocks = vec!(ifbb, self.basicblock);

                    unsafe {
                        LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
                    }

                    self.vartable.set_value(&name, phi);
                }


                self.basicblock = endifbb;
            }
            _ => {
                return Err(format!("statement not implement: {:?}", stmt)); 
            }
        }
        
        Ok(())
    }

    fn expression(&mut self, e: &Expression, t: ElementaryTypeName) -> Result<LLVMValueRef, String> {
        match e {
            Expression::NumberLiteral(n) => {
                let ltype = if t == ElementaryTypeName::Any {
                    unsafe {
                        LLVMIntTypeInContext(self.context, n.bits() as u32)
                    }
                } else {
                    t.LLVMType(self.context)
                };

                let s = n.to_string();

                unsafe {
                    Ok(LLVMConstIntOfStringAndSize(ltype, s.as_ptr() as *const _, s.len() as _, 10))
                }
            },
            Expression::Add(l, r) => {
                let left = self.expression(l, t)?;
                let right = self.expression(r, t)?;

                unsafe {
                    Ok(LLVMBuildAdd(self.builder, left, right, b"\0".as_ptr() as *const _))
                }
            },
            Expression::Subtract(l, r) => {
                let left = self.expression(l, t)?;
                let right = self.expression(r, t)?;

                unsafe {
                    Ok(LLVMBuildSub(self.builder, left, right, b"\0".as_ptr() as *const _))
                }
            },
            Expression::Multiply(l, r) => {
                let left = self.expression(l, t)?;
                let right = self.expression(r, t)?;

                unsafe {
                    Ok(LLVMBuildMul(self.builder, left, right, b"\0".as_ptr() as *const _))
                }
            },
            Expression::Divide(l, r) => {
                let left = self.expression(l, t)?;
                let right = self.expression(r, t)?;

                if get_expression_type(self.function, l)?.signed() {
                    unsafe {
                        Ok(LLVMBuildSDiv(self.builder, left, right, b"\0".as_ptr() as *const _))
                    }
                } else {
                    unsafe {
                        Ok(LLVMBuildUDiv(self.builder, left, right, b"\0".as_ptr() as *const _))
                    }
                }
            },
            Expression::Equal(l, r) => {
                let left = self.expression(l, ElementaryTypeName::Uint(32))?;
                let right = self.expression(r, ElementaryTypeName::Uint(32))?;

                unsafe {
                    Ok(LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntEQ, left, right, b"\0".as_ptr() as *const _))
                }
            }
            Expression::Variable(s) => {
                let var = self.vartable.get(s);

                if var.typ == t || t == ElementaryTypeName::Any {
                    Ok(var.value)
                } else {
                    Ok(match t {
                        ElementaryTypeName::Uint(_) => unsafe {
                            LLVMBuildZExtOrBitCast(self.builder, var.value, t.LLVMType(self.context), "\0".as_ptr() as *const _)
                        },
                        ElementaryTypeName::Int(_) => unsafe {
                            LLVMBuildSExtOrBitCast(self.builder, var.value, t.LLVMType(self.context), "\0".as_ptr() as *const _)
                        },
                        _ => panic!("implement implicit casting for {:?} to {:?}", var.typ, t)
                    })
                }
            },
            Expression::Assign(l, r) => {
                match l {
                    box Expression::Variable(s) => {
                        let typ = self.vartable.get_type(s);
                        let value = self.expression(r, typ)?;
                        self.vartable.set_value(s, value);
                        Ok(0 as LLVMValueRef)
                    },
                    _ => panic!("cannot assign to non-lvalue")
                }
            },
            Expression::AssignAdd(l, r) => {
                match l {
                    box Expression::Variable(s) => {
                        let typ = self.vartable.get_type(s);
                        let value = self.expression(r, typ)?;
                        let lvalue = self.vartable.get_value(s);
                        self.vartable.set_value(s, value);
                        let nvalue = unsafe {
                            LLVMBuildAdd(self.builder, lvalue, value, b"\0".as_ptr() as *const _)
                        };
                        self.vartable.set_value(s, nvalue);
                        Ok(0 as LLVMValueRef)
                    },
                    _ => panic!("cannot assign to non-lvalue")
                }
            },
            Expression::AssignSubtract(l, r) => {
                match l {
                    box Expression::Variable(s) => {
                        let typ = self.vartable.get_type(s);
                        let value = self.expression(r, typ)?;
                        let lvalue = self.vartable.get_value(s);
                        self.vartable.set_value(s, value);
                        let nvalue = unsafe {
                            LLVMBuildSub(self.builder, lvalue, value, b"\0".as_ptr() as *const _)
                        };
                        self.vartable.set_value(s, nvalue);
                        Ok(0 as LLVMValueRef)
                    },
                    _ => panic!("cannot assign to non-lvalue")
                }
            },
            _ => {
                Err(format!("expression not implemented: {:?}", e))
            }
        }       
    }
}