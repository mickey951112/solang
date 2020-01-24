use num_bigint::BigInt;
use num_bigint::Sign;
use num_traits::FromPrimitive;
use num_traits::Num;
use num_traits::One;
use num_traits::Zero;
use std::cmp;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::LinkedList;
use unescape::unescape;

use hex;
use output;
use output::Output;
use parser::ast;
use resolver;
use resolver::address::to_hexstr_eip55;

#[derive(PartialEq, Clone, Debug)]
pub enum Expression {
    BoolLiteral(bool),
    BytesLiteral(Vec<u8>),
    NumberLiteral(u16, BigInt),
    Add(Box<Expression>, Box<Expression>),
    Subtract(Box<Expression>, Box<Expression>),
    Multiply(Box<Expression>, Box<Expression>),
    UDivide(Box<Expression>, Box<Expression>),
    SDivide(Box<Expression>, Box<Expression>),
    UModulo(Box<Expression>, Box<Expression>),
    SModulo(Box<Expression>, Box<Expression>),
    Power(Box<Expression>, Box<Expression>),
    BitwiseOr(Box<Expression>, Box<Expression>),
    BitwiseAnd(Box<Expression>, Box<Expression>),
    BitwiseXor(Box<Expression>, Box<Expression>),
    ShiftLeft(Box<Expression>, Box<Expression>),
    ShiftRight(Box<Expression>, Box<Expression>, bool),
    Variable(ast::Loc, usize),
    ZeroExt(resolver::Type, Box<Expression>),
    SignExt(resolver::Type, Box<Expression>),
    Trunc(resolver::Type, Box<Expression>),

    UMore(Box<Expression>, Box<Expression>),
    ULess(Box<Expression>, Box<Expression>),
    UMoreEqual(Box<Expression>, Box<Expression>),
    ULessEqual(Box<Expression>, Box<Expression>),
    SMore(Box<Expression>, Box<Expression>),
    SLess(Box<Expression>, Box<Expression>),
    SMoreEqual(Box<Expression>, Box<Expression>),
    SLessEqual(Box<Expression>, Box<Expression>),
    Equal(Box<Expression>, Box<Expression>),
    NotEqual(Box<Expression>, Box<Expression>),

    Not(Box<Expression>),
    Complement(Box<Expression>),
    UnaryMinus(Box<Expression>),

    Ternary(Box<Expression>, Box<Expression>, Box<Expression>),
    IndexAccess(Box<Expression>, Box<Expression>),

    Or(Box<Expression>, Box<Expression>),
    And(Box<Expression>, Box<Expression>),

    Poison,
}

pub enum Instr {
    FuncArg {
        res: usize,
        arg: usize,
    },
    GetStorage {
        local: usize,
        storage: usize,
    },
    SetStorage {
        local: usize,
        storage: usize,
    },
    Set {
        res: usize,
        expr: Expression,
    },
    Constant {
        res: usize,
        constant: usize,
    },
    Call {
        res: Vec<usize>,
        func: usize,
        args: Vec<Expression>,
    },
    Return {
        value: Vec<Expression>,
    },
    Branch {
        bb: usize,
    },
    BranchCond {
        cond: Expression,
        true_: usize,
        false_: usize,
    },
    AssertFailure {},
}

pub struct BasicBlock {
    pub phis: Option<HashSet<usize>>,
    pub name: String,
    pub instr: Vec<Instr>,
}

impl BasicBlock {
    fn add(&mut self, ins: Instr) {
        self.instr.push(ins);
    }
}

#[derive(Default)]
pub struct ControlFlowGraph {
    pub vars: Vec<Variable>,
    pub bb: Vec<BasicBlock>,
    current: usize,
    pub writes_contract_storage: bool,
    pub reads_contract_storage: bool,
}

impl ControlFlowGraph {
    pub fn new() -> Self {
        let mut cfg = ControlFlowGraph {
            vars: Vec::new(),
            bb: Vec::new(),
            current: 0,
            reads_contract_storage: false,
            writes_contract_storage: false,
        };

        cfg.new_basic_block("entry".to_string());

        cfg
    }

    pub fn new_basic_block(&mut self, name: String) -> usize {
        let pos = self.bb.len();

        self.bb.push(BasicBlock {
            name,
            instr: Vec::new(),
            phis: None,
        });

        pos
    }

    fn set_phis(&mut self, bb: usize, phis: HashSet<usize>) {
        if !phis.is_empty() {
            self.bb[bb].phis = Some(phis);
        }
    }

    pub fn set_basic_block(&mut self, pos: usize) {
        self.current = pos;
    }

    pub fn add(&mut self, vartab: &mut Vartable, ins: Instr) {
        if let Instr::Set { res, .. } = ins {
            vartab.set_dirty(res);
        }
        self.bb[self.current].add(ins);
    }

    pub fn expr_to_string(&self, ns: &resolver::Contract, expr: &Expression) -> String {
        match expr {
            Expression::BoolLiteral(false) => "false".to_string(),
            Expression::BoolLiteral(true) => "true".to_string(),
            Expression::BytesLiteral(s) => format!("hex\"{}\"", hex::encode(s)),
            Expression::NumberLiteral(bits, n) => format!("i{} {}", bits, n.to_str_radix(10)),
            Expression::Add(l, r) => format!(
                "({} + {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::Subtract(l, r) => format!(
                "({} - {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::BitwiseOr(l, r) => format!(
                "({} | {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::BitwiseAnd(l, r) => format!(
                "({} & {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::BitwiseXor(l, r) => format!(
                "({} ^ {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::ShiftLeft(l, r) => format!(
                "({} << {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::ShiftRight(l, r, _) => format!(
                "({} >> {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::Multiply(l, r) => format!(
                "({} * {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::UDivide(l, r) | Expression::SDivide(l, r) => format!(
                "({} / {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::UModulo(l, r) | Expression::SModulo(l, r) => format!(
                "({} % {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::Power(l, r) => format!(
                "({} ** {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::Variable(_, res) => format!("%{}", self.vars[*res].id.name),

            Expression::ZeroExt(ty, e) => {
                format!("(zext {} {})", ty.to_string(ns), self.expr_to_string(ns, e))
            }
            Expression::SignExt(ty, e) => {
                format!("(sext {} {})", ty.to_string(ns), self.expr_to_string(ns, e))
            }
            Expression::Trunc(ty, e) => format!(
                "(trunc {} {})",
                ty.to_string(ns),
                self.expr_to_string(ns, e)
            ),
            Expression::SMore(l, r) => format!(
                "({} >(s) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::SLess(l, r) => format!(
                "({} <(s) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::SMoreEqual(l, r) => format!(
                "({} >=(s) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::SLessEqual(l, r) => format!(
                "({} <=(s) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::UMore(l, r) => format!(
                "({} >(u) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::ULess(l, r) => format!(
                "({} <(u) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::UMoreEqual(l, r) => format!(
                "({} >=(u) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::ULessEqual(l, r) => format!(
                "({} <=(u) {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::Equal(l, r) => format!(
                "({} = {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),
            Expression::NotEqual(l, r) => format!(
                "({} != {})",
                self.expr_to_string(ns, l),
                self.expr_to_string(ns, r)
            ),

            Expression::Not(e) => format!("!{}", self.expr_to_string(ns, e)),
            Expression::Complement(e) => format!("~{}", self.expr_to_string(ns, e)),
            Expression::UnaryMinus(e) => format!("-{}", self.expr_to_string(ns, e)),

            _ => String::from(""),
        }
    }

    pub fn instr_to_string(&self, ns: &resolver::Contract, instr: &Instr) -> String {
        match instr {
            Instr::Return { value } => {
                let mut s = String::from("return ");
                let mut first = true;

                for arg in value {
                    if !first {
                        s.push_str(", ");
                    }
                    first = false;
                    s.push_str(&self.expr_to_string(ns, arg));
                }

                s
            }
            Instr::Set { res, expr } => format!(
                "%{} = {}",
                self.vars[*res].id.name,
                self.expr_to_string(ns, expr)
            ),
            Instr::Constant { res, constant } => format!(
                "%{} = const {}",
                self.vars[*res].id.name,
                self.expr_to_string(ns, &ns.constants[*constant])
            ),
            Instr::Branch { bb } => format!("branch bb{}", bb),
            Instr::BranchCond {
                cond,
                true_,
                false_,
            } => format!(
                "branchcond {}, bb{}, bb{}",
                self.expr_to_string(ns, cond),
                true_,
                false_
            ),
            Instr::FuncArg { res, arg } => {
                format!("%{} = funcarg({})", self.vars[*res].id.name, arg)
            }
            Instr::SetStorage { local, storage } => {
                format!("setstorage %{} = %{}", *storage, self.vars[*local].id.name)
            }
            Instr::GetStorage { local, storage } => {
                format!("getstorage %{} = %{}", *storage, self.vars[*local].id.name)
            }
            Instr::AssertFailure {} => "assert-failure".to_string(),
            Instr::Call { res, func, args } => format!(
                "{} = call {} {} {}",
                {
                    let s: Vec<String> = res
                        .iter()
                        .map(|local| format!("%{}", self.vars[*local].id.name))
                        .collect();

                    s.join(", ")
                },
                *func,
                ns.functions[*func].name.to_owned(),
                {
                    let s: Vec<String> = args
                        .iter()
                        .map(|expr| self.expr_to_string(ns, expr))
                        .collect();

                    s.join(", ")
                }
            ),
        }
    }

    pub fn basic_block_to_string(&self, ns: &resolver::Contract, pos: usize) -> String {
        let mut s = format!("bb{}: # {}\n", pos, self.bb[pos].name);

        if let Some(ref phis) = self.bb[pos].phis {
            s.push_str("# phis: ");
            let mut first = true;
            for p in phis {
                if !first {
                    s.push_str(", ");
                }
                first = false;
                s.push_str(&self.vars[*p].id.name);
            }
            s.push_str("\n");
        }

        for ins in &self.bb[pos].instr {
            s.push_str(&format!("\t{}\n", self.instr_to_string(ns, ins)));
        }

        s
    }

    pub fn to_string(&self, ns: &resolver::Contract) -> String {
        let mut s = String::from("");

        for i in 0..self.bb.len() {
            s.push_str(&self.basic_block_to_string(ns, i));
        }

        s
    }
}

pub fn generate_cfg(
    ast_f: &ast::FunctionDefinition,
    resolve_f: &resolver::FunctionDecl,
    ns: &resolver::Contract,
    errors: &mut Vec<output::Output>,
) -> Result<Box<ControlFlowGraph>, ()> {
    let mut cfg = Box::new(ControlFlowGraph::new());

    let mut vartab = Vartable::new();
    let mut loops = LoopScopes::new();

    // first add function parameters
    for (i, p) in ast_f.params.iter().enumerate() {
        if let Some(ref name) = p.name {
            if let Some(pos) = vartab.add(name, resolve_f.params[i].ty.clone(), errors) {
                ns.check_shadowing(name, errors);

                cfg.add(&mut vartab, Instr::FuncArg { res: pos, arg: i });
            }
        }
    }

    // If any of the return values are named, then the return statement can be omitted at
    // the end of the function, and return values may be omitted too. Create variables to
    // store the return values
    if ast_f.returns.iter().any(|v| v.name.is_some()) {
        let mut returns = Vec::new();

        for (i, p) in ast_f.returns.iter().enumerate() {
            returns.push(if let Some(ref name) = p.name {
                if let Some(pos) = vartab.add(name, resolve_f.returns[i].ty.clone(), errors) {
                    ns.check_shadowing(name, errors);

                    // set to zero
                    cfg.add(
                        &mut vartab,
                        Instr::Set {
                            res: pos,
                            expr: resolve_f.returns[i].ty.default(ns),
                        },
                    );

                    pos
                } else {
                    // obs wrong but we had an error so will continue with bogus value to generate parser errors
                    0
                }
            } else {
                // this variable can never be assigned but will need a zero value
                let pos = vartab.temp(
                    &ast::Identifier {
                        loc: ast::Loc(0, 0),
                        name: format!("arg{}", i),
                    },
                    &resolve_f.returns[i].ty.clone(),
                );

                // set to zero
                cfg.add(
                    &mut vartab,
                    Instr::Set {
                        res: pos,
                        expr: resolve_f.returns[i].ty.default(ns),
                    },
                );

                pos
            });
        }

        vartab.returns = returns;
    }

    let reachable = statement(
        &ast_f.body,
        resolve_f,
        &mut cfg,
        ns,
        &mut vartab,
        &mut loops,
        errors,
    )?;

    // ensure we have a return instruction
    if reachable {
        check_return(ast_f, &mut cfg, &vartab, errors)?;
    }

    cfg.vars = vartab.drain();

    // walk cfg to check for use for before initialize

    Ok(cfg)
}

fn check_return(
    f: &ast::FunctionDefinition,
    cfg: &mut ControlFlowGraph,
    vartab: &Vartable,
    errors: &mut Vec<output::Output>,
) -> Result<(), ()> {
    let current = cfg.current;
    let bb = &mut cfg.bb[current];

    let num_instr = bb.instr.len();

    if num_instr > 0 {
        if let Instr::Return { .. } = bb.instr[num_instr - 1] {
            return Ok(());
        }
    }

    if f.returns.is_empty() || !vartab.returns.is_empty() {
        bb.add(Instr::Return {
            value: vartab
                .returns
                .iter()
                .map(|pos| Expression::Variable(ast::Loc(0, 0), *pos))
                .collect(),
        });

        Ok(())
    } else {
        errors.push(Output::error(
            f.body.loc(),
            "missing return statement".to_string(),
        ));
        Err(())
    }
}

fn get_contract_storage(var: &Variable, cfg: &mut ControlFlowGraph, vartab: &mut Vartable) {
    match var.storage {
        Storage::Contract(offset) => {
            cfg.reads_contract_storage = true;
            cfg.add(
                vartab,
                Instr::GetStorage {
                    local: var.pos,
                    storage: offset,
                },
            );
        }
        Storage::Constant(n) => {
            cfg.add(
                vartab,
                Instr::Constant {
                    res: var.pos,
                    constant: n,
                },
            );
        }
        Storage::Local => (),
    }
}

pub fn set_contract_storage(
    id: &ast::Identifier,
    var: &Variable,
    cfg: &mut ControlFlowGraph,
    vartab: &mut Vartable,
    errors: &mut Vec<output::Output>,
) -> Result<(), ()> {
    match var.storage {
        Storage::Contract(offset) => {
            cfg.writes_contract_storage = true;
            cfg.add(
                vartab,
                Instr::SetStorage {
                    local: var.pos,
                    storage: offset,
                },
            );

            Ok(())
        }
        Storage::Constant(_) => {
            errors.push(Output::type_error(
                id.loc,
                format!("cannot assign to constant {}", id.name),
            ));
            Err(())
        }
        Storage::Local => {
            // nothing to do
            Ok(())
        }
    }
}

fn statement(
    stmt: &ast::Statement,
    f: &resolver::FunctionDecl,
    cfg: &mut ControlFlowGraph,
    ns: &resolver::Contract,
    vartab: &mut Vartable,
    loops: &mut LoopScopes,
    errors: &mut Vec<output::Output>,
) -> Result<bool, ()> {
    match stmt {
        ast::Statement::VariableDefinition(decl, init) => {
            let var_ty = ns.resolve_type(&decl.typ, Some(errors))?;

            if var_ty.size_hint() > BigInt::from(1024 * 1024) {
                errors.push(Output::error(
                    stmt.loc(),
                    "type to large to fit into memory".to_string(),
                ));
                return Err(());
            }

            let e_t = if let Some(init) = init {
                let (expr, init_ty) = expression(init, cfg, ns, &mut Some(vartab), errors)?;

                Some(cast(
                    &decl.name.loc,
                    expr,
                    &init_ty,
                    &var_ty,
                    true,
                    ns,
                    errors,
                )?)
            } else {
                None
            };

            if let Some(pos) = vartab.add(&decl.name, var_ty, errors) {
                ns.check_shadowing(&decl.name, errors);

                if let Some(expr) = e_t {
                    cfg.add(vartab, Instr::Set { res: pos, expr });
                }
            }
            Ok(true)
        }
        ast::Statement::BlockStatement(ast::BlockStatement(bs)) => {
            vartab.new_scope();
            let mut reachable = true;

            for stmt in bs {
                if !reachable {
                    errors.push(Output::error(
                        stmt.loc(),
                        "unreachable statement".to_string(),
                    ));
                    return Err(());
                }
                reachable = statement(&stmt, f, cfg, ns, vartab, loops, errors)?;
            }

            vartab.leave_scope();

            Ok(reachable)
        }
        ast::Statement::Return(loc, returns) if returns.is_empty() => {
            let no_returns = f.returns.len();

            if vartab.returns.len() != no_returns {
                errors.push(Output::error(
                    *loc,
                    format!(
                        "missing return value, {} return values expected",
                        no_returns
                    ),
                ));
                return Err(());
            }

            cfg.add(
                vartab,
                Instr::Return {
                    value: vartab
                        .returns
                        .iter()
                        .map(|pos| Expression::Variable(ast::Loc(0, 0), *pos))
                        .collect(),
                },
            );

            Ok(false)
        }
        ast::Statement::Return(loc, returns) => {
            let no_returns = f.returns.len();

            if no_returns > 0 && returns.is_empty() {
                errors.push(Output::error(
                    *loc,
                    format!(
                        "missing return value, {} return values expected",
                        no_returns
                    ),
                ));
                return Err(());
            }

            if no_returns == 0 && !returns.is_empty() {
                errors.push(Output::error(
                    *loc,
                    "function has no return values".to_string(),
                ));
                return Err(());
            }

            if no_returns != returns.len() {
                errors.push(Output::error(
                    *loc,
                    format!(
                        "incorrect number of return values, expected {} but got {}",
                        no_returns,
                        returns.len()
                    ),
                ));
                return Err(());
            }

            let mut exprs = Vec::new();

            for (i, r) in returns.iter().enumerate() {
                let (e, ty) = expression(r, cfg, ns, &mut Some(vartab), errors)?;

                exprs.push(cast(&r.loc(), e, &ty, &f.returns[i].ty, true, ns, errors)?);
            }

            cfg.add(vartab, Instr::Return { value: exprs });

            Ok(false)
        }
        ast::Statement::Expression(expr) => {
            expression(expr, cfg, ns, &mut Some(vartab), errors)?;

            Ok(true)
        }
        ast::Statement::If(cond, then_stmt, None) => {
            let (expr, expr_ty) = expression(cond, cfg, ns, &mut Some(vartab), errors)?;

            let then = cfg.new_basic_block("then".to_string());
            let endif = cfg.new_basic_block("endif".to_string());

            cfg.add(
                vartab,
                Instr::BranchCond {
                    cond: cast(
                        &cond.loc(),
                        expr,
                        &expr_ty,
                        &resolver::Type::new_bool(),
                        true,
                        ns,
                        errors,
                    )?,
                    true_: then,
                    false_: endif,
                },
            );

            cfg.set_basic_block(then);

            vartab.new_scope();
            vartab.new_dirty_tracker();

            let reachable = statement(then_stmt, f, cfg, ns, vartab, loops, errors)?;

            if reachable {
                cfg.add(vartab, Instr::Branch { bb: endif });
            }

            vartab.leave_scope();
            cfg.set_phis(endif, vartab.pop_dirty_tracker());

            cfg.set_basic_block(endif);

            Ok(true)
        }
        ast::Statement::If(cond, then_stmt, Some(else_stmt)) => {
            let (expr, expr_ty) = expression(cond, cfg, ns, &mut Some(vartab), errors)?;

            let then = cfg.new_basic_block("then".to_string());
            let else_ = cfg.new_basic_block("else".to_string());
            let endif = cfg.new_basic_block("endif".to_string());

            cfg.add(
                vartab,
                Instr::BranchCond {
                    cond: cast(
                        &cond.loc(),
                        expr,
                        &expr_ty,
                        &resolver::Type::new_bool(),
                        true,
                        ns,
                        errors,
                    )?,
                    true_: then,
                    false_: else_,
                },
            );

            // then
            cfg.set_basic_block(then);

            vartab.new_scope();
            vartab.new_dirty_tracker();

            let then_reachable = statement(then_stmt, f, cfg, ns, vartab, loops, errors)?;

            if then_reachable {
                cfg.add(vartab, Instr::Branch { bb: endif });
            }

            vartab.leave_scope();

            // else
            cfg.set_basic_block(else_);

            vartab.new_scope();

            let else_reachable = statement(else_stmt, f, cfg, ns, vartab, loops, errors)?;

            if else_reachable {
                cfg.add(vartab, Instr::Branch { bb: endif });
            }

            vartab.leave_scope();
            cfg.set_phis(endif, vartab.pop_dirty_tracker());

            cfg.set_basic_block(endif);

            Ok(then_reachable || else_reachable)
        }
        ast::Statement::Break => match loops.do_break() {
            Some(bb) => {
                cfg.add(vartab, Instr::Branch { bb });
                Ok(false)
            }
            None => {
                errors.push(Output::error(
                    stmt.loc(),
                    "break statement not in loop".to_string(),
                ));
                Err(())
            }
        },
        ast::Statement::Continue => match loops.do_continue() {
            Some(bb) => {
                cfg.add(vartab, Instr::Branch { bb });
                Ok(false)
            }
            None => {
                errors.push(Output::error(
                    stmt.loc(),
                    "continue statement not in loop".to_string(),
                ));
                Err(())
            }
        },
        ast::Statement::DoWhile(body_stmt, cond_expr) => {
            let body = cfg.new_basic_block("body".to_string());
            let cond = cfg.new_basic_block("conf".to_string());
            let end = cfg.new_basic_block("enddowhile".to_string());

            cfg.add(vartab, Instr::Branch { bb: body });

            cfg.set_basic_block(body);

            vartab.new_scope();
            vartab.new_dirty_tracker();
            loops.new_scope(end, cond);

            let mut body_reachable = statement(body_stmt, f, cfg, ns, vartab, loops, errors)?;

            if body_reachable {
                cfg.add(vartab, Instr::Branch { bb: cond });
            }

            vartab.leave_scope();
            let control = loops.leave_scope();

            if control.no_continues > 0 {
                body_reachable = true
            }

            if body_reachable {
                cfg.set_basic_block(cond);

                let (expr, expr_ty) = expression(cond_expr, cfg, ns, &mut Some(vartab), errors)?;

                cfg.add(
                    vartab,
                    Instr::BranchCond {
                        cond: cast(
                            &cond_expr.loc(),
                            expr,
                            &expr_ty,
                            &resolver::Type::new_bool(),
                            true,
                            ns,
                            errors,
                        )?,
                        true_: body,
                        false_: end,
                    },
                );
            }

            let set = vartab.pop_dirty_tracker();
            cfg.set_phis(end, set.clone());
            cfg.set_phis(body, set.clone());
            cfg.set_phis(cond, set);

            cfg.set_basic_block(end);

            Ok(body_reachable || control.no_breaks > 0)
        }
        ast::Statement::While(cond_expr, body_stmt) => {
            let cond = cfg.new_basic_block("cond".to_string());
            let body = cfg.new_basic_block("body".to_string());
            let end = cfg.new_basic_block("endwhile".to_string());

            cfg.add(vartab, Instr::Branch { bb: cond });

            cfg.set_basic_block(cond);

            let (expr, expr_ty) = expression(cond_expr, cfg, ns, &mut Some(vartab), errors)?;

            cfg.add(
                vartab,
                Instr::BranchCond {
                    cond: cast(
                        &cond_expr.loc(),
                        expr,
                        &expr_ty,
                        &resolver::Type::new_bool(),
                        true,
                        ns,
                        errors,
                    )?,
                    true_: body,
                    false_: end,
                },
            );

            cfg.set_basic_block(body);

            vartab.new_scope();
            vartab.new_dirty_tracker();
            loops.new_scope(end, cond);

            let body_reachable = statement(body_stmt, f, cfg, ns, vartab, loops, errors)?;

            if body_reachable {
                cfg.add(vartab, Instr::Branch { bb: cond });
            }

            vartab.leave_scope();
            loops.leave_scope();
            let set = vartab.pop_dirty_tracker();
            cfg.set_phis(end, set.clone());
            cfg.set_phis(cond, set);

            cfg.set_basic_block(end);

            Ok(true)
        }
        ast::Statement::For(init_stmt, None, next_stmt, body_stmt) => {
            let body = cfg.new_basic_block("body".to_string());
            let next = cfg.new_basic_block("next".to_string());
            let end = cfg.new_basic_block("endfor".to_string());

            vartab.new_scope();

            if let Some(init_stmt) = init_stmt {
                statement(init_stmt, f, cfg, ns, vartab, loops, errors)?;
            }

            cfg.add(vartab, Instr::Branch { bb: body });

            cfg.set_basic_block(body);

            loops.new_scope(
                end,
                match next_stmt {
                    Some(_) => next,
                    None => body,
                },
            );
            vartab.new_dirty_tracker();

            let mut body_reachable = match body_stmt {
                Some(body_stmt) => statement(body_stmt, f, cfg, ns, vartab, loops, errors)?,
                None => true,
            };

            if body_reachable {
                cfg.add(vartab, Instr::Branch { bb: next });
            }

            let control = loops.leave_scope();

            if control.no_continues > 0 {
                body_reachable = true;
            }

            if body_reachable {
                if let Some(next_stmt) = next_stmt {
                    cfg.set_basic_block(next);
                    body_reachable = statement(next_stmt, f, cfg, ns, vartab, loops, errors)?;
                }

                if body_reachable {
                    cfg.add(vartab, Instr::Branch { bb: body });
                }
            }

            let set = vartab.pop_dirty_tracker();
            if control.no_continues > 0 {
                cfg.set_phis(next, set.clone());
            }
            cfg.set_phis(body, set.clone());
            cfg.set_phis(end, set);

            vartab.leave_scope();
            cfg.set_basic_block(end);

            Ok(control.no_breaks > 0)
        }
        ast::Statement::For(init_stmt, Some(cond_expr), next_stmt, body_stmt) => {
            let body = cfg.new_basic_block("body".to_string());
            let cond = cfg.new_basic_block("cond".to_string());
            let next = cfg.new_basic_block("next".to_string());
            let end = cfg.new_basic_block("endfor".to_string());

            vartab.new_scope();

            if let Some(init_stmt) = init_stmt {
                statement(init_stmt, f, cfg, ns, vartab, loops, errors)?;
            }

            cfg.add(vartab, Instr::Branch { bb: cond });

            cfg.set_basic_block(cond);

            let (expr, expr_ty) = expression(cond_expr, cfg, ns, &mut Some(vartab), errors)?;

            cfg.add(
                vartab,
                Instr::BranchCond {
                    cond: cast(
                        &cond_expr.loc(),
                        expr,
                        &expr_ty,
                        &resolver::Type::new_bool(),
                        true,
                        ns,
                        errors,
                    )?,
                    true_: body,
                    false_: end,
                },
            );

            cfg.set_basic_block(body);

            // continue goes to next, and if that does exist, cond
            loops.new_scope(
                end,
                match next_stmt {
                    Some(_) => next,
                    None => cond,
                },
            );
            vartab.new_dirty_tracker();

            let mut body_reachable = match body_stmt {
                Some(body_stmt) => statement(body_stmt, f, cfg, ns, vartab, loops, errors)?,
                None => true,
            };

            if body_reachable {
                cfg.add(vartab, Instr::Branch { bb: next });
            }

            let control = loops.leave_scope();

            if control.no_continues > 0 {
                body_reachable = true;
            }

            if body_reachable {
                if let Some(next_stmt) = next_stmt {
                    cfg.set_basic_block(next);
                    body_reachable = statement(next_stmt, f, cfg, ns, vartab, loops, errors)?;
                }

                if body_reachable {
                    cfg.add(vartab, Instr::Branch { bb: cond });
                }
            }

            vartab.leave_scope();
            cfg.set_basic_block(end);

            let set = vartab.pop_dirty_tracker();
            if control.no_continues > 0 {
                cfg.set_phis(next, set.clone());
            }
            if control.no_breaks > 0 {
                cfg.set_phis(end, set.clone());
            }
            cfg.set_phis(cond, set);

            Ok(true)
        }
        _ => panic!("not implemented"),
    }
}

fn coerce(
    l: &resolver::Type,
    l_loc: &ast::Loc,
    r: &resolver::Type,
    r_loc: &ast::Loc,
    ns: &resolver::Contract,
    errors: &mut Vec<output::Output>,
) -> Result<resolver::Type, ()> {
    if *l == *r {
        return Ok(l.clone());
    }

    coerce_int(l, l_loc, r, r_loc, true, ns, errors)
}

fn get_int_length(
    l: &resolver::Type,
    l_loc: &ast::Loc,
    allow_bytes: bool,
    ns: &resolver::Contract,
    errors: &mut Vec<output::Output>,
) -> Result<(u16, bool), ()> {
    Ok(match l {
        resolver::Type::Primitive(ast::PrimitiveType::Uint(n)) => (*n, false),
        resolver::Type::Primitive(ast::PrimitiveType::Int(n)) => (*n, true),
        resolver::Type::Primitive(ast::PrimitiveType::Bytes(n)) if allow_bytes => {
            (*n as u16 * 8, false)
        }
        resolver::Type::Primitive(t) => {
            errors.push(Output::error(
                *l_loc,
                format!("expression of type {} not allowed", t.to_string()),
            ));
            return Err(());
        }
        resolver::Type::Enum(n) => {
            errors.push(Output::error(
                *l_loc,
                format!("type enum {} not allowed", ns.enums[*n].name),
            ));
            return Err(());
        }
        resolver::Type::FixedArray(_, _) => {
            errors.push(Output::error(
                *l_loc,
                format!("type array {} not allowed", l.to_string(ns)),
            ));
            return Err(());
        }
        resolver::Type::Noreturn => {
            unreachable!();
        }
    })
}

fn coerce_int(
    l: &resolver::Type,
    l_loc: &ast::Loc,
    r: &resolver::Type,
    r_loc: &ast::Loc,
    allow_bytes: bool,
    ns: &resolver::Contract,
    errors: &mut Vec<output::Output>,
) -> Result<resolver::Type, ()> {
    match (l, r) {
        (
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(left_length)),
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(right_length)),
        ) if allow_bytes => {
            return Ok(resolver::Type::Primitive(ast::PrimitiveType::Bytes(
                std::cmp::max(*left_length, *right_length),
            )));
        }
        _ => (),
    }

    let (left_len, left_signed) = get_int_length(l, l_loc, false, ns, errors)?;

    let (right_len, right_signed) = get_int_length(r, r_loc, false, ns, errors)?;

    Ok(resolver::Type::Primitive(
        match (left_signed, right_signed) {
            (true, true) => ast::PrimitiveType::Int(cmp::max(left_len, right_len)),
            (false, false) => ast::PrimitiveType::Uint(cmp::max(left_len, right_len)),
            (true, false) => {
                ast::PrimitiveType::Int(cmp::max(left_len, cmp::min(right_len + 8, 256)))
            }
            (false, true) => {
                ast::PrimitiveType::Int(cmp::max(cmp::min(left_len + 8, 256), right_len))
            }
        },
    ))
}

/// Try to convert a BigInt into a Expression::NumberLiteral. This checks for sign,
/// width and creates to correct Type.
fn bigint_to_expression(
    loc: &ast::Loc,
    n: &BigInt,
    errors: &mut Vec<Output>,
) -> Result<(Expression, resolver::Type), ()> {
    // Return smallest type
    let bits = n.bits();

    let int_size = if bits < 7 { 8 } else { (bits + 7) & !7 } as u16;

    if n.sign() == Sign::Minus {
        if bits > 255 {
            errors.push(Output::error(*loc, format!("{} is too large", n)));
            Err(())
        } else {
            Ok((
                Expression::NumberLiteral(int_size, n.clone()),
                resolver::Type::Primitive(ast::PrimitiveType::Int(int_size)),
            ))
        }
    } else if bits > 256 {
        errors.push(Output::error(*loc, format!("{} is too large", n)));
        Err(())
    } else {
        Ok((
            Expression::NumberLiteral(int_size, n.clone()),
            resolver::Type::Primitive(ast::PrimitiveType::Uint(int_size)),
        ))
    }
}

pub fn cast(
    loc: &ast::Loc,
    expr: Expression,
    from: &resolver::Type,
    to: &resolver::Type,
    implicit: bool,
    ns: &resolver::Contract,
    errors: &mut Vec<output::Output>,
) -> Result<Expression, ()> {
    if from == to {
        return Ok(expr);
    }

    let (from_conv, to_conv) = {
        if implicit {
            (from.clone(), to.clone())
        } else {
            let from_conv = if let resolver::Type::Enum(n) = from {
                resolver::Type::Primitive(ns.enums[*n].ty)
            } else {
                from.clone()
            };

            let to_conv = if let resolver::Type::Enum(n) = to {
                resolver::Type::Primitive(ns.enums[*n].ty)
            } else {
                to.clone()
            };

            (from_conv, to_conv)
        }
    };

    // Special case: when converting literal sign can change if it fits
    match (&expr, &from_conv, &to_conv) {
        (
            &Expression::NumberLiteral(_, ref n),
            &resolver::Type::Primitive(_),
            &resolver::Type::Primitive(ast::PrimitiveType::Uint(to_len)),
        ) => {
            return if n.sign() == Sign::Minus {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion cannot change negative number to {}",
                        to.to_string(ns)
                    ),
                ));

                Err(())
            } else if n.bits() >= to_len as usize {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion would truncate from {} to {}",
                        from.to_string(ns),
                        to.to_string(ns)
                    ),
                ));

                Err(())
            } else {
                Ok(Expression::NumberLiteral(to_len, n.clone()))
            }
        }
        (
            &Expression::NumberLiteral(_, ref n),
            &resolver::Type::Primitive(_),
            &resolver::Type::Primitive(ast::PrimitiveType::Int(to_len)),
        ) => {
            return if n.bits() >= to_len as usize {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion would truncate from {} to {}",
                        from.to_string(ns),
                        to.to_string(ns)
                    ),
                ));

                Err(())
            } else {
                Ok(Expression::NumberLiteral(to_len, n.clone()))
            }
        }
        // Literal strings can be implicitly lengthened
        (
            &Expression::BytesLiteral(ref bs),
            &resolver::Type::Primitive(_),
            &resolver::Type::Primitive(ast::PrimitiveType::Bytes(to_len)),
        ) => {
            return if bs.len() > to_len as usize {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion would truncate from {} to {}",
                        from.to_string(ns),
                        to.to_string(ns)
                    ),
                ));

                Err(())
            } else {
                let mut bs = bs.to_owned();

                // Add zero's at the end as needed
                bs.resize(to_len as usize, 0);

                Ok(Expression::BytesLiteral(bs))
            };
        }
        _ => (),
    };

    #[allow(clippy::comparison_chain)]
    match (from_conv, to_conv) {
        (
            resolver::Type::Primitive(ast::PrimitiveType::Uint(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Uint(to_len)),
        ) => match from_len.cmp(&to_len) {
            Ordering::Greater => {
                if implicit {
                    errors.push(Output::type_error(
                        *loc,
                        format!(
                            "implicit conversion would truncate from {} to {}",
                            from.to_string(ns),
                            to.to_string(ns)
                        ),
                    ));
                    Err(())
                } else {
                    Ok(Expression::Trunc(to.clone(), Box::new(expr)))
                }
            }
            Ordering::Less => Ok(Expression::ZeroExt(to.clone(), Box::new(expr))),
            Ordering::Equal => Ok(expr),
        },
        (
            resolver::Type::Primitive(ast::PrimitiveType::Int(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Int(to_len)),
        ) => match from_len.cmp(&to_len) {
            Ordering::Greater => {
                if implicit {
                    errors.push(Output::type_error(
                        *loc,
                        format!(
                            "implicit conversion would truncate from {} to {}",
                            from.to_string(ns),
                            to.to_string(ns)
                        ),
                    ));
                    Err(())
                } else {
                    Ok(Expression::Trunc(to.clone(), Box::new(expr)))
                }
            }
            Ordering::Less => Ok(Expression::SignExt(to.clone(), Box::new(expr))),
            Ordering::Equal => Ok(expr),
        },
        (
            resolver::Type::Primitive(ast::PrimitiveType::Uint(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Int(to_len)),
        ) if to_len > from_len => Ok(Expression::ZeroExt(to.clone(), Box::new(expr))),
        (
            resolver::Type::Primitive(ast::PrimitiveType::Int(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Uint(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion would change sign from {} to {}",
                        from.to_string(ns),
                        to.to_string(ns)
                    ),
                ));
                Err(())
            } else if from_len > to_len {
                Ok(Expression::Trunc(to.clone(), Box::new(expr)))
            } else if from_len < to_len {
                Ok(Expression::SignExt(to.clone(), Box::new(expr)))
            } else {
                Ok(expr)
            }
        }
        (
            resolver::Type::Primitive(ast::PrimitiveType::Uint(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Int(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion would change sign from {} to {}",
                        from.to_string(ns),
                        to.to_string(ns)
                    ),
                ));
                Err(())
            } else if from_len > to_len {
                Ok(Expression::Trunc(to.clone(), Box::new(expr)))
            } else if from_len < to_len {
                Ok(Expression::ZeroExt(to.clone(), Box::new(expr)))
            } else {
                Ok(expr)
            }
        }
        // Casting int to address
        (
            resolver::Type::Primitive(ast::PrimitiveType::Uint(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Address),
        )
        | (
            resolver::Type::Primitive(ast::PrimitiveType::Int(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Address),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion from {} to address not allowed",
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else if from_len > 160 {
                Ok(Expression::Trunc(to.clone(), Box::new(expr)))
            } else if from_len < 160 {
                Ok(Expression::ZeroExt(to.clone(), Box::new(expr)))
            } else {
                Ok(expr)
            }
        }
        // Casting int address to int
        (
            resolver::Type::Primitive(ast::PrimitiveType::Address),
            resolver::Type::Primitive(ast::PrimitiveType::Uint(to_len)),
        )
        | (
            resolver::Type::Primitive(ast::PrimitiveType::Address),
            resolver::Type::Primitive(ast::PrimitiveType::Int(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion to {} from address not allowed",
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else if to_len < 160 {
                Ok(Expression::Trunc(to.clone(), Box::new(expr)))
            } else if to_len > 160 {
                Ok(Expression::ZeroExt(to.clone(), Box::new(expr)))
            } else {
                Ok(expr)
            }
        }
        // Lengthing or shorting a fixed bytes array
        (
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion would truncate from {} to {}",
                        from.to_string(ns),
                        to.to_string(ns)
                    ),
                ));
                Err(())
            } else if to_len > from_len {
                let shift = (to_len - from_len) * 8;

                Ok(Expression::ShiftLeft(
                    Box::new(Expression::ZeroExt(to.clone(), Box::new(expr))),
                    Box::new(Expression::NumberLiteral(
                        to_len as u16 * 8,
                        BigInt::from_u8(shift).unwrap(),
                    )),
                ))
            } else {
                let shift = (from_len - to_len) * 8;

                Ok(Expression::Trunc(
                    to.clone(),
                    Box::new(Expression::ShiftRight(
                        Box::new(expr),
                        Box::new(Expression::NumberLiteral(
                            from_len as u16 * 8,
                            BigInt::from_u8(shift).unwrap(),
                        )),
                        false,
                    )),
                ))
            }
        }
        // Explicit conversion from bytesN to int/uint only allowed with expliciy
        // cast and if it is the same size (i.e. no conversion required)
        (
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Uint(to_len)),
        )
        | (
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Int(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else if from_len as u16 * 8 != to_len {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else {
                Ok(expr)
            }
        }
        // Explicit conversion to bytesN from int/uint only allowed with expliciy
        // cast and if it is the same size (i.e. no conversion required)
        (
            resolver::Type::Primitive(ast::PrimitiveType::Uint(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(to_len)),
        )
        | (
            resolver::Type::Primitive(ast::PrimitiveType::Int(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else if to_len as u16 * 8 != from_len {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else {
                Ok(expr)
            }
        }
        // Explicit conversion from bytesN to address only allowed with expliciy
        // cast and if it is the same size (i.e. no conversion required)
        (
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(from_len)),
            resolver::Type::Primitive(ast::PrimitiveType::Address),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else if from_len != 20 {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else {
                Ok(expr)
            }
        }
        // Explicit conversion to bytesN from int/uint only allowed with expliciy
        // cast and if it is the same size (i.e. no conversion required)
        (
            resolver::Type::Primitive(ast::PrimitiveType::Address),
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(to_len)),
        ) => {
            if implicit {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "implicit conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else if to_len != 20 {
                errors.push(Output::type_error(
                    *loc,
                    format!(
                        "conversion to {} from {} not allowed",
                        to.to_string(ns),
                        from.to_string(ns)
                    ),
                ));
                Err(())
            } else {
                Ok(expr)
            }
        }
        // string conversions
        (
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(_)),
            resolver::Type::Primitive(ast::PrimitiveType::String),
        ) => Ok(expr),
        (
            resolver::Type::Primitive(ast::PrimitiveType::String),
            resolver::Type::Primitive(ast::PrimitiveType::Bytes(to_len)),
        ) => {
            if let Expression::BytesLiteral(from_str) = &expr {
                if from_str.len() > to_len as usize {
                    errors.push(Output::type_error(
                        *loc,
                        format!(
                            "string of {} bytes is too long to fit into {}",
                            from_str.len(),
                            to.to_string(ns)
                        ),
                    ));
                    return Err(());
                }
            }
            Ok(expr)
        }
        _ => {
            errors.push(Output::type_error(
                *loc,
                format!(
                    "conversion from {} to {} not possible",
                    from.to_string(ns),
                    to.to_string(ns)
                ),
            ));
            Err(())
        }
    }
}

pub fn expression(
    expr: &ast::Expression,
    cfg: &mut ControlFlowGraph,
    ns: &resolver::Contract,
    vartab: &mut Option<&mut Vartable>,
    errors: &mut Vec<output::Output>,
) -> Result<(Expression, resolver::Type), ()> {
    match expr {
        ast::Expression::BoolLiteral(_, v) => Ok((
            Expression::BoolLiteral(*v),
            resolver::Type::Primitive(ast::PrimitiveType::Bool),
        )),
        ast::Expression::StringLiteral(v) => {
            // Concatenate the strings
            let mut result = String::new();

            for s in v {
                // unescape supports octal escape values, solc does not
                // neither solc nor unescape support unicode code points like \u{61}
                match unescape(&s.string) {
                    Some(v) => result.push_str(&v),
                    None => {
                        // would be helpful if unescape told us what/where the problem was
                        errors.push(Output::error(
                            s.loc,
                            format!("string \"{}\" has invalid escape", s.string),
                        ));
                        return Err(());
                    }
                }
            }

            let length = result.len();

            Ok((
                Expression::BytesLiteral(result.into_bytes()),
                resolver::Type::Primitive(ast::PrimitiveType::Bytes(length as u8)),
            ))
        }
        ast::Expression::HexLiteral(v) => {
            let mut result = Vec::new();

            for s in v {
                if (s.hex.len() % 2) != 0 {
                    errors.push(Output::error(
                        s.loc,
                        format!("hex string \"{}\" has odd number of characters", s.hex),
                    ));
                    return Err(());
                } else {
                    result.extend_from_slice(&hex::decode(&s.hex).unwrap());
                }
            }

            let length = result.len();

            Ok((
                Expression::BytesLiteral(result),
                resolver::Type::Primitive(ast::PrimitiveType::Bytes(length as u8)),
            ))
        }
        ast::Expression::NumberLiteral(loc, b) => bigint_to_expression(loc, b, errors),
        ast::Expression::AddressLiteral(loc, n) => {
            let address = to_hexstr_eip55(n);

            if address == *n {
                let s: String = address.chars().skip(2).collect();

                Ok((
                    Expression::NumberLiteral(160, BigInt::from_str_radix(&s, 16).unwrap()),
                    resolver::Type::Primitive(ast::PrimitiveType::Address),
                ))
            } else {
                errors.push(Output::error(
                    *loc,
                    format!(
                        "address literal has incorrect checksum, expected ‘{}’",
                        address
                    ),
                ));
                Err(())
            }
        }
        ast::Expression::Variable(id) => {
            if let Some(ref mut tab) = *vartab {
                let v = tab.find(id, ns, errors)?;
                get_contract_storage(&v, cfg, tab);
                Ok((Expression::Variable(id.loc, v.pos), v.ty))
            } else {
                errors.push(Output::error(
                    id.loc,
                    format!("cannot read variable {} in constant expression", id.name),
                ));
                Err(())
            }
        }
        ast::Expression::Add(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                false,
                ns,
                errors,
            )?;

            Ok((
                Expression::Add(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }
        ast::Expression::Subtract(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                false,
                ns,
                errors,
            )?;

            Ok((
                Expression::Subtract(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }
        ast::Expression::BitwiseOr(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            Ok((
                Expression::BitwiseOr(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }
        ast::Expression::BitwiseAnd(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            Ok((
                Expression::BitwiseAnd(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }
        ast::Expression::BitwiseXor(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            Ok((
                Expression::BitwiseXor(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }
        ast::Expression::ShiftLeft(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            // left hand side may be bytes/int/uint
            // right hand size may be int/uint
            let _ = get_int_length(&left_type, &l.loc(), true, ns, errors)?;
            let (right_length, _) = get_int_length(&right_type, &r.loc(), false, ns, errors)?;

            Ok((
                Expression::ShiftLeft(
                    Box::new(left),
                    Box::new(cast_shift_arg(right, right_length, &left_type)),
                ),
                left_type,
            ))
        }
        ast::Expression::ShiftRight(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            // left hand side may be bytes/int/uint
            // right hand size may be int/uint
            let _ = get_int_length(&left_type, &l.loc(), true, ns, errors)?;
            let (right_length, _) = get_int_length(&right_type, &r.loc(), false, ns, errors)?;

            Ok((
                Expression::ShiftRight(
                    Box::new(left),
                    Box::new(cast_shift_arg(right, right_length, &left_type)),
                    left_type.signed(),
                ),
                left_type,
            ))
        }
        ast::Expression::Multiply(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                false,
                ns,
                errors,
            )?;

            Ok((
                Expression::Multiply(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }
        ast::Expression::Divide(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                false,
                ns,
                errors,
            )?;

            if ty.signed() {
                Ok((
                    Expression::SDivide(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    ty,
                ))
            } else {
                Ok((
                    Expression::UDivide(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    ty,
                ))
            }
        }
        ast::Expression::Modulo(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                false,
                ns,
                errors,
            )?;

            if ty.signed() {
                Ok((
                    Expression::SModulo(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    ty,
                ))
            } else {
                Ok((
                    Expression::UModulo(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    ty,
                ))
            }
        }
        ast::Expression::Power(loc, b, e) => {
            let (base, base_type) = expression(b, cfg, ns, vartab, errors)?;
            let (exp, exp_type) = expression(e, cfg, ns, vartab, errors)?;

            // solc-0.5.13 does not allow either base or exp to be signed
            if base_type.signed() || exp_type.signed() {
                errors.push(Output::error(
                    *loc,
                    "exponation (**) is not allowed with signed types".to_string(),
                ));
                return Err(());
            }

            let ty = coerce_int(&base_type, &b.loc(), &exp_type, &e.loc(), false, ns, errors)?;

            Ok((
                Expression::Power(
                    Box::new(cast(&b.loc(), base, &base_type, &ty, true, ns, errors)?),
                    Box::new(cast(&e.loc(), exp, &exp_type, &ty, true, ns, errors)?),
                ),
                ty,
            ))
        }

        // compare
        ast::Expression::More(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            if ty.signed() {
                Ok((
                    Expression::SMore(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            } else {
                Ok((
                    Expression::UMore(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            }
        }
        ast::Expression::Less(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            if ty.signed() {
                Ok((
                    Expression::SLess(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            } else {
                Ok((
                    Expression::ULess(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            }
        }
        ast::Expression::MoreEqual(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            if ty.signed() {
                Ok((
                    Expression::SMoreEqual(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            } else {
                Ok((
                    Expression::UMoreEqual(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            }
        }
        ast::Expression::LessEqual(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce_int(
                &left_type,
                &l.loc(),
                &right_type,
                &r.loc(),
                true,
                ns,
                errors,
            )?;

            if ty.signed() {
                Ok((
                    Expression::SLessEqual(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            } else {
                Ok((
                    Expression::ULessEqual(
                        Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                        Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                    ),
                    resolver::Type::new_bool(),
                ))
            }
        }
        ast::Expression::Equal(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce(&left_type, &l.loc(), &right_type, &r.loc(), ns, errors)?;

            Ok((
                Expression::Equal(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                resolver::Type::new_bool(),
            ))
        }
        ast::Expression::NotEqual(_, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;

            let ty = coerce(&left_type, &l.loc(), &right_type, &r.loc(), ns, errors)?;

            Ok((
                Expression::NotEqual(
                    Box::new(cast(&l.loc(), left, &left_type, &ty, true, ns, errors)?),
                    Box::new(cast(&r.loc(), right, &right_type, &ty, true, ns, errors)?),
                ),
                resolver::Type::new_bool(),
            ))
        }

        // unary expressions
        ast::Expression::Not(loc, e) => {
            let (expr, expr_type) = expression(e, cfg, ns, vartab, errors)?;

            Ok((
                Expression::Not(Box::new(cast(
                    &loc,
                    expr,
                    &expr_type,
                    &resolver::Type::new_bool(),
                    true,
                    ns,
                    errors,
                )?)),
                resolver::Type::new_bool(),
            ))
        }
        ast::Expression::Complement(loc, e) => {
            let (expr, expr_type) = expression(e, cfg, ns, vartab, errors)?;

            get_int_length(&expr_type, loc, true, ns, errors)?;

            Ok((Expression::Complement(Box::new(expr)), expr_type))
        }
        ast::Expression::UnaryMinus(loc, e) => {
            let (expr, expr_type) = expression(e, cfg, ns, vartab, errors)?;

            get_int_length(&expr_type, loc, false, ns, errors)?;

            Ok((Expression::UnaryMinus(Box::new(expr)), expr_type))
        }
        ast::Expression::UnaryPlus(loc, e) => {
            let (expr, expr_type) = expression(e, cfg, ns, vartab, errors)?;

            get_int_length(&expr_type, loc, false, ns, errors)?;

            Ok((expr, expr_type))
        }

        ast::Expression::Ternary(_, c, l, r) => {
            let (left, left_type) = expression(l, cfg, ns, vartab, errors)?;
            let (right, right_type) = expression(r, cfg, ns, vartab, errors)?;
            let (cond, cond_type) = expression(c, cfg, ns, vartab, errors)?;

            let cond = cast(
                &c.loc(),
                cond,
                &cond_type,
                &resolver::Type::new_bool(),
                true,
                ns,
                errors,
            )?;

            let ty = coerce(&left_type, &l.loc(), &right_type, &r.loc(), ns, errors)?;

            Ok((
                Expression::Ternary(Box::new(cond), Box::new(left), Box::new(right)),
                ty,
            ))
        }

        // pre/post decrement/increment
        ast::Expression::PostIncrement(loc, var)
        | ast::Expression::PreIncrement(loc, var)
        | ast::Expression::PostDecrement(loc, var)
        | ast::Expression::PreDecrement(loc, var) => {
            let id = match var.as_ref() {
                ast::Expression::Variable(id) => id,
                _ => unreachable!(),
            };

            let vartab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    errors.push(Output::error(
                        *loc,
                        format!("cannot access variable {} in constant expression", id.name),
                    ));
                    return Err(());
                }
            };

            let var = vartab.find(id, ns, errors)?;
            let (pos, ty) = {
                get_contract_storage(&var, cfg, vartab);

                (var.pos, var.ty.clone())
            };

            get_int_length(&ty, loc, false, ns, errors)?;

            match expr {
                ast::Expression::PostIncrement(_, _) => {
                    let temp_pos = vartab.temp(id, &ty);
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: temp_pos,
                            expr: Expression::Variable(id.loc, pos),
                        },
                    );
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: pos,
                            expr: Expression::Add(
                                Box::new(Expression::Variable(id.loc, pos)),
                                Box::new(Expression::NumberLiteral(ty.bits(), One::one())),
                            ),
                        },
                    );

                    set_contract_storage(id, &var, cfg, vartab, errors)?;

                    Ok((Expression::Variable(id.loc, temp_pos), ty))
                }
                ast::Expression::PostDecrement(_, _) => {
                    let temp_pos = vartab.temp(id, &ty);
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: temp_pos,
                            expr: Expression::Variable(id.loc, pos),
                        },
                    );
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: pos,
                            expr: Expression::Subtract(
                                Box::new(Expression::Variable(id.loc, pos)),
                                Box::new(Expression::NumberLiteral(ty.bits(), One::one())),
                            ),
                        },
                    );

                    set_contract_storage(id, &var, cfg, vartab, errors)?;

                    Ok((Expression::Variable(id.loc, temp_pos), ty))
                }
                ast::Expression::PreIncrement(_, _) => {
                    let temp_pos = vartab.temp(id, &ty);
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: pos,
                            expr: Expression::Add(
                                Box::new(Expression::Variable(id.loc, pos)),
                                Box::new(Expression::NumberLiteral(ty.bits(), One::one())),
                            ),
                        },
                    );
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: temp_pos,
                            expr: Expression::Variable(id.loc, pos),
                        },
                    );

                    set_contract_storage(id, &var, cfg, vartab, errors)?;

                    Ok((Expression::Variable(id.loc, temp_pos), ty))
                }
                ast::Expression::PreDecrement(_, _) => {
                    let temp_pos = vartab.temp(id, &ty);
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: pos,
                            expr: Expression::Subtract(
                                Box::new(Expression::Variable(id.loc, pos)),
                                Box::new(Expression::NumberLiteral(ty.bits(), One::one())),
                            ),
                        },
                    );
                    cfg.add(
                        vartab,
                        Instr::Set {
                            res: temp_pos,
                            expr: Expression::Variable(id.loc, pos),
                        },
                    );

                    set_contract_storage(id, &var, cfg, vartab, errors)?;

                    Ok((Expression::Variable(id.loc, temp_pos), ty))
                }
                _ => unreachable!(),
            }
        }

        // assignment
        ast::Expression::Assign(loc, var, e) => {
            let id = match var.as_ref() {
                ast::Expression::Variable(id) => id,
                _ => unreachable!(),
            };

            let (expr, expr_type) = expression(e, cfg, ns, vartab, errors)?;

            let vartab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    errors.push(Output::error(
                        *loc,
                        format!("cannot access variable {} in constant expression", id.name),
                    ));
                    return Err(());
                }
            };

            let var = vartab.find(id, ns, errors)?;

            cfg.add(
                vartab,
                Instr::Set {
                    res: var.pos,
                    expr: cast(&id.loc, expr, &expr_type, &var.ty, true, ns, errors)?,
                },
            );

            set_contract_storage(id, &var, cfg, vartab, errors)?;

            Ok((Expression::Variable(id.loc, var.pos), var.ty))
        }

        ast::Expression::AssignAdd(loc, var, e)
        | ast::Expression::AssignSubtract(loc, var, e)
        | ast::Expression::AssignMultiply(loc, var, e)
        | ast::Expression::AssignDivide(loc, var, e)
        | ast::Expression::AssignModulo(loc, var, e)
        | ast::Expression::AssignOr(loc, var, e)
        | ast::Expression::AssignAnd(loc, var, e)
        | ast::Expression::AssignXor(loc, var, e)
        | ast::Expression::AssignShiftLeft(loc, var, e)
        | ast::Expression::AssignShiftRight(loc, var, e) => {
            let id = match var.as_ref() {
                ast::Expression::Variable(id) => id,
                _ => unreachable!(),
            };

            let (set, set_type) = expression(e, cfg, ns, vartab, errors)?;

            let tab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    errors.push(Output::error(
                        *loc,
                        format!("cannot access variable {} in constant expression", id.name),
                    ));
                    return Err(());
                }
            };

            let var = tab.find(id, ns, errors)?;
            let (pos, ty) = {
                get_contract_storage(&var, cfg, tab);

                (var.pos, var.ty.clone())
            };

            match ty {
                resolver::Type::Primitive(ast::PrimitiveType::Bytes(_))
                | resolver::Type::Primitive(ast::PrimitiveType::Int(_))
                | resolver::Type::Primitive(ast::PrimitiveType::Uint(_)) => (),
                _ => {
                    errors.push(Output::error(
                        id.loc,
                        format!(
                            "variable {} of incorrect type {}",
                            id.name.to_string(),
                            ty.to_string(ns)
                        ),
                    ));
                    return Err(());
                }
            };

            let set = match expr {
                ast::Expression::AssignShiftLeft(_, _, _)
                | ast::Expression::AssignShiftRight(_, _, _) => {
                    let left_length = get_int_length(&ty, &loc, true, ns, errors)?;
                    let right_length = get_int_length(&set_type, &e.loc(), false, ns, errors)?;

                    // TODO: does shifting by negative value need compiletime/runtime check?
                    if left_length == right_length {
                        set
                    } else if right_length < left_length && set_type.signed() {
                        Expression::SignExt(ty.clone(), Box::new(set))
                    } else if right_length < left_length && !set_type.signed() {
                        Expression::ZeroExt(ty.clone(), Box::new(set))
                    } else {
                        Expression::Trunc(ty.clone(), Box::new(set))
                    }
                }
                _ => cast(&id.loc, set, &set_type, &ty, true, ns, errors)?,
            };

            let set = match expr {
                ast::Expression::AssignAdd(_, _, _) => {
                    Expression::Add(Box::new(Expression::Variable(id.loc, pos)), Box::new(set))
                }
                ast::Expression::AssignSubtract(_, _, _) => {
                    Expression::Subtract(Box::new(Expression::Variable(id.loc, pos)), Box::new(set))
                }
                ast::Expression::AssignMultiply(_, _, _) => {
                    Expression::Multiply(Box::new(Expression::Variable(id.loc, pos)), Box::new(set))
                }
                ast::Expression::AssignOr(_, _, _) => Expression::BitwiseOr(
                    Box::new(Expression::Variable(id.loc, pos)),
                    Box::new(set),
                ),
                ast::Expression::AssignAnd(_, _, _) => Expression::BitwiseAnd(
                    Box::new(Expression::Variable(id.loc, pos)),
                    Box::new(set),
                ),
                ast::Expression::AssignXor(_, _, _) => Expression::BitwiseXor(
                    Box::new(Expression::Variable(id.loc, pos)),
                    Box::new(set),
                ),
                ast::Expression::AssignShiftLeft(_, _, _) => Expression::ShiftLeft(
                    Box::new(Expression::Variable(id.loc, pos)),
                    Box::new(set),
                ),
                ast::Expression::AssignShiftRight(_, _, _) => Expression::ShiftRight(
                    Box::new(Expression::Variable(id.loc, pos)),
                    Box::new(set),
                    ty.signed(),
                ),
                ast::Expression::AssignDivide(_, _, _) => {
                    if ty.signed() {
                        Expression::SDivide(
                            Box::new(Expression::Variable(id.loc, pos)),
                            Box::new(set),
                        )
                    } else {
                        Expression::UDivide(
                            Box::new(Expression::Variable(id.loc, pos)),
                            Box::new(set),
                        )
                    }
                }
                ast::Expression::AssignModulo(_, _, _) => {
                    if ty.signed() {
                        Expression::SModulo(
                            Box::new(Expression::Variable(id.loc, pos)),
                            Box::new(set),
                        )
                    } else {
                        Expression::UModulo(
                            Box::new(Expression::Variable(id.loc, pos)),
                            Box::new(set),
                        )
                    }
                }
                _ => unreachable!(),
            };

            cfg.add(
                tab,
                Instr::Set {
                    res: pos,
                    expr: set,
                },
            );

            set_contract_storage(id, &var, cfg, tab, errors)?;

            Ok((Expression::Variable(id.loc, pos), ty))
        }
        ast::Expression::FunctionCall(loc, ty, args) => {
            let to = match ns.resolve_type(ty, None) {
                Ok(ty) => Some(ty),
                Err(_) => None,
            };

            // Cast
            if let Some(to) = to {
                return if args.is_empty() {
                    errors.push(Output::error(*loc, "missing argument to cast".to_string()));
                    Err(())
                } else if args.len() > 1 {
                    errors.push(Output::error(
                        *loc,
                        "too many arguments to cast".to_string(),
                    ));
                    Err(())
                } else {
                    let (expr, expr_type) = expression(&args[0], cfg, ns, vartab, errors)?;

                    Ok((cast(loc, expr, &expr_type, &to, false, ns, errors)?, to))
                };
            }

            let funcs = if let ast::Type::Unresolved(s, _) = ty {
                ns.resolve_func(s, errors)?
            } else {
                unreachable!();
            };

            let mut resolved_args = Vec::new();
            let mut resolved_types = Vec::new();

            for arg in args {
                let (expr, expr_type) = expression(arg, cfg, ns, vartab, errors)?;

                resolved_args.push(Box::new(expr));
                resolved_types.push(expr_type);
            }

            let tab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    errors.push(Output::error(
                        *loc,
                        "cannot call function in constant expression".to_string(),
                    ));
                    return Err(());
                }
            };

            let mut temp_errors = Vec::new();

            // function call
            for f in funcs {
                let func = &ns.functions[f.1];

                if func.params.len() != args.len() {
                    temp_errors.push(Output::error(
                        *loc,
                        format!(
                            "function expects {} arguments, {} provided",
                            func.params.len(),
                            args.len()
                        ),
                    ));
                    continue;
                }

                let mut matches = true;
                let mut cast_args = Vec::new();

                // check if arguments can be implicitly casted
                for (i, param) in func.params.iter().enumerate() {
                    let arg = &resolved_args[i];

                    match cast(
                        &ast::Loc(0, 0),
                        *arg.clone(),
                        &resolved_types[i],
                        &param.ty,
                        true,
                        ns,
                        &mut temp_errors,
                    ) {
                        Ok(expr) => cast_args.push(expr),
                        Err(()) => {
                            matches = false;
                            break;
                        }
                    }
                }

                if !matches {
                    continue;
                }

                // .. what about return value?
                if func.returns.len() > 1 {
                    errors.push(Output::error(
                        *loc,
                        "in expression context a function cannot return more than one value"
                            .to_string(),
                    ));
                    return Err(());
                }

                if !func.returns.is_empty() {
                    let ty = &func.returns[0].ty;
                    let id = ast::Identifier {
                        loc: ast::Loc(0, 0),
                        name: "".to_owned(),
                    };
                    let temp_pos = tab.temp(&id, ty);

                    cfg.add(
                        tab,
                        Instr::Call {
                            res: vec![temp_pos],
                            func: f.1,
                            args: cast_args,
                        },
                    );

                    return Ok((Expression::Variable(id.loc, temp_pos), ty.clone()));
                } else {
                    cfg.add(
                        tab,
                        Instr::Call {
                            res: Vec::new(),
                            func: f.1,
                            args: cast_args,
                        },
                    );

                    return Ok((Expression::Poison, resolver::Type::Noreturn));
                }
            }

            if funcs.len() == 1 {
                errors.append(&mut temp_errors);
            } else {
                errors.push(Output::error(
                    *loc,
                    "cannot find overloaded function which matches signature".to_string(),
                ));
            }

            Err(())
        }
        ast::Expression::IndexAccess(loc, var, Some(index)) => {
            let id = match var.as_ref() {
                ast::Expression::Variable(id) => id,
                _ => unreachable!(),
            };

            let (index_expr, index_type) = expression(index, cfg, ns, vartab, errors)?;

            let tab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    errors.push(Output::error(
                        *loc,
                        format!("cannot read variable {} in constant expression", id.name),
                    ));
                    return Err(());
                }
            };

            let var = tab.find(id, ns, errors)?;

            get_contract_storage(&var, cfg, tab);

            let array_length = match var.ty {
                resolver::Type::Primitive(ast::PrimitiveType::Bytes(n)) => BigInt::from(n),
                resolver::Type::FixedArray(_, _) => var.ty.array_length().clone(),
                _ => {
                    errors.push(Output::error(
                        *loc,
                        format!("variable ‘{}’ is not an array", id.name),
                    ));
                    return Err(());
                }
            };

            let (index_width, _) = get_int_length(&index_type, &index.loc(), false, ns, errors)?;
            let array_width = array_length.bits();

            let pos = tab.temp(
                &ast::Identifier {
                    name: "index".to_owned(),
                    loc: *loc,
                },
                &index_type,
            );

            cfg.add(
                tab,
                Instr::Set {
                    res: pos,
                    expr: index_expr,
                },
            );

            let out_of_range = cfg.new_basic_block("out_of_range".to_string());
            let in_range = cfg.new_basic_block("in_range".to_string());

            if index_type.signed() {
                // first check that our index is not negative
                let positive = cfg.new_basic_block("positive".to_string());

                cfg.add(
                    tab,
                    Instr::BranchCond {
                        cond: Expression::SLess(
                            Box::new(Expression::Variable(index.loc(), pos)),
                            Box::new(Expression::NumberLiteral(index_width, BigInt::zero())),
                        ),
                        true_: out_of_range,
                        false_: positive,
                    },
                );

                cfg.set_basic_block(positive);

                // If the index if of less bits than the array length, don't bother checking
                if index_width as usize >= array_width {
                    cfg.add(
                        tab,
                        Instr::BranchCond {
                            cond: Expression::SMoreEqual(
                                Box::new(Expression::Variable(index.loc(), pos)),
                                Box::new(Expression::NumberLiteral(index_width, array_length)),
                            ),
                            true_: out_of_range,
                            false_: in_range,
                        },
                    );
                } else {
                    cfg.add(tab, Instr::Branch { bb: in_range });
                }
            } else if index_width as usize <= array_width {
                cfg.add(
                    tab,
                    Instr::BranchCond {
                        cond: Expression::UMoreEqual(
                            Box::new(Expression::Variable(index.loc(), pos)),
                            Box::new(Expression::NumberLiteral(index_width, array_length)),
                        ),
                        true_: out_of_range,
                        false_: in_range,
                    },
                );
            } else {
                // if the index is less bits than the array, it is always in range
                cfg.add(tab, Instr::Branch { bb: in_range });
            }

            cfg.set_basic_block(out_of_range);
            cfg.add(tab, Instr::AssertFailure {});

            cfg.set_basic_block(in_range);

            match var.ty {
                resolver::Type::Primitive(ast::PrimitiveType::Bytes(array_length)) => {
                    let res_ty = resolver::Type::Primitive(ast::PrimitiveType::Bytes(1));

                    Ok((
                        Expression::Trunc(
                            res_ty.clone(),
                            Box::new(Expression::ShiftRight(
                                Box::new(Expression::Variable(*loc, var.pos)),
                                // shift by (array_length - 1 - index) * 8
                                Box::new(Expression::ShiftLeft(
                                    Box::new(Expression::Subtract(
                                        Box::new(Expression::NumberLiteral(
                                            array_length as u16 * 8,
                                            BigInt::from_u8(array_length - 1).unwrap(),
                                        )),
                                        Box::new(cast_shift_arg(
                                            Expression::Variable(index.loc(), pos),
                                            index_width,
                                            &var.ty,
                                        )),
                                    )),
                                    Box::new(Expression::NumberLiteral(
                                        array_length as u16 * 8,
                                        BigInt::from_u8(3).unwrap(),
                                    )),
                                )),
                                false,
                            )),
                        ),
                        res_ty,
                    ))
                }
                resolver::Type::FixedArray(_, _) => Ok((
                    Expression::IndexAccess(
                        Box::new(Expression::Variable(id.loc, var.pos)),
                        Box::new(Expression::Variable(*loc, pos)),
                    ),
                    var.ty.deref(),
                )),
                _ => {
                    // should not happen as type-checking already done
                    unreachable!();
                }
            }
        }
        ast::Expression::MemberAccess(loc, namespace, id) => {
            // Is it an enum
            if let Some(e) = ns.resolve_enum(namespace) {
                return match ns.enums[e].values.get(&id.name) {
                    Some((_, val)) => Ok((
                        Expression::NumberLiteral(
                            ns.enums[e].ty.bits(),
                            BigInt::from_usize(*val).unwrap(),
                        ),
                        resolver::Type::Enum(e),
                    )),
                    None => {
                        errors.push(Output::error(
                            id.loc,
                            format!("enum {} does not have value {}", ns.enums[e].name, id.name),
                        ));
                        Err(())
                    }
                };
            }

            // is it an bytesN.length / array.length
            if let Some(ref mut tab) = *vartab {
                let var = tab.find(namespace, ns, errors)?;

                match var.ty {
                    resolver::Type::Primitive(ast::PrimitiveType::Bytes(n)) => {
                        if id.name == "length" {
                            return Ok((
                                Expression::NumberLiteral(8, BigInt::from_u8(n).unwrap()),
                                resolver::Type::Primitive(ast::PrimitiveType::Uint(8)),
                            ));
                        }
                    }
                    resolver::Type::FixedArray(_, dim) => {
                        if id.name == "length" {
                            return bigint_to_expression(loc, dim.last().unwrap(), errors);
                        }
                    }
                    _ => (),
                }
            }

            errors.push(Output::error(*loc, "not found".to_string()));

            Err(())
        }
        ast::Expression::Or(loc, left, right) => {
            let boolty = resolver::Type::new_bool();
            let (l, l_type) = expression(left, cfg, ns, vartab, errors)?;
            let l = cast(&loc, l, &l_type, &boolty, true, ns, errors)?;

            let mut tab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    // In constant context, no side effects so short-circut not necessary
                    let (r, r_type) = expression(right, cfg, ns, vartab, errors)?;

                    return Ok((
                        Expression::Or(
                            Box::new(l),
                            Box::new(cast(
                                &loc,
                                r,
                                &r_type,
                                &resolver::Type::new_bool(),
                                true,
                                ns,
                                errors,
                            )?),
                        ),
                        resolver::Type::new_bool(),
                    ));
                }
            };

            let pos = tab.temp(
                &ast::Identifier {
                    name: "or".to_owned(),
                    loc: *loc,
                },
                &resolver::Type::new_bool(),
            );

            let right_side = cfg.new_basic_block("or_right_side".to_string());
            let end_or = cfg.new_basic_block("or_end".to_string());

            cfg.add(
                tab,
                Instr::Set {
                    res: pos,
                    expr: Expression::BoolLiteral(true),
                },
            );
            cfg.add(
                tab,
                Instr::BranchCond {
                    cond: l,
                    true_: end_or,
                    false_: right_side,
                },
            );
            cfg.set_basic_block(right_side);

            let (r, r_type) = expression(right, cfg, ns, &mut Some(&mut tab), errors)?;
            let r = cast(
                &loc,
                r,
                &r_type,
                &resolver::Type::new_bool(),
                true,
                ns,
                errors,
            )?;

            cfg.add(tab, Instr::Set { res: pos, expr: r });

            let mut phis = HashSet::new();
            phis.insert(pos);

            cfg.set_phis(end_or, phis);

            cfg.add(tab, Instr::Branch { bb: end_or });

            cfg.set_basic_block(end_or);

            Ok((Expression::Variable(*loc, pos), boolty))
        }
        ast::Expression::And(loc, left, right) => {
            let boolty = resolver::Type::new_bool();
            let (l, l_type) = expression(left, cfg, ns, vartab, errors)?;
            let l = cast(&loc, l, &l_type, &boolty, true, ns, errors)?;

            let mut tab = match vartab {
                &mut Some(ref mut tab) => tab,
                None => {
                    // In constant context, no side effects so short-circut not necessary
                    let (r, r_type) = expression(right, cfg, ns, vartab, errors)?;

                    return Ok((
                        Expression::And(
                            Box::new(l),
                            Box::new(cast(
                                &loc,
                                r,
                                &r_type,
                                &resolver::Type::new_bool(),
                                true,
                                ns,
                                errors,
                            )?),
                        ),
                        resolver::Type::new_bool(),
                    ));
                }
            };

            let pos = tab.temp(
                &ast::Identifier {
                    name: "and".to_owned(),
                    loc: *loc,
                },
                &resolver::Type::new_bool(),
            );

            let right_side = cfg.new_basic_block("and_right_side".to_string());
            let end_and = cfg.new_basic_block("and_end".to_string());

            cfg.add(
                tab,
                Instr::Set {
                    res: pos,
                    expr: Expression::BoolLiteral(false),
                },
            );
            cfg.add(
                tab,
                Instr::BranchCond {
                    cond: l,
                    true_: right_side,
                    false_: end_and,
                },
            );
            cfg.set_basic_block(right_side);

            let (r, r_type) = expression(right, cfg, ns, &mut Some(&mut tab), errors)?;
            let r = cast(
                &loc,
                r,
                &r_type,
                &resolver::Type::new_bool(),
                true,
                ns,
                errors,
            )?;

            cfg.add(tab, Instr::Set { res: pos, expr: r });

            let mut phis = HashSet::new();
            phis.insert(pos);

            cfg.set_phis(end_and, phis);

            cfg.add(tab, Instr::Branch { bb: end_and });

            cfg.set_basic_block(end_and);

            Ok((Expression::Variable(*loc, pos), boolty))
        }
        _ => panic!("unimplemented: {:?}", expr),
    }
}

// When generating shifts, llvm wants both arguments to have the same width. We want the
// result of the shift to be left argument, so this function coercies the right argument
// into the right length.
fn cast_shift_arg(expr: Expression, from_width: u16, ty: &resolver::Type) -> Expression {
    let to_width = ty.bits();

    if from_width == to_width {
        expr
    } else if from_width < to_width && ty.signed() {
        Expression::SignExt(ty.clone(), Box::new(expr))
    } else if from_width < to_width && !ty.signed() {
        Expression::ZeroExt(ty.clone(), Box::new(expr))
    } else {
        Expression::Trunc(ty.clone(), Box::new(expr))
    }
}

// Vartable
// methods
// create variable with loc, name, Type -> pos
// find variable by name -> Some(pos)
// new scope
// leave scope
// produce full Vector of all variables
#[derive(Clone)]
pub enum Storage {
    Constant(usize),
    Contract(usize),
    Local,
}

#[derive(Clone)]
pub struct Variable {
    pub id: ast::Identifier,
    pub ty: resolver::Type,
    pub pos: usize,
    pub storage: Storage,
}

struct VarScope(HashMap<String, usize>, Option<HashSet<usize>>);

#[derive(Default)]
pub struct Vartable {
    vars: Vec<Variable>,
    names: LinkedList<VarScope>,
    storage_vars: HashMap<String, usize>,
    dirty: Vec<DirtyTracker>,
    returns: Vec<usize>,
}

pub struct DirtyTracker {
    lim: usize,
    set: HashSet<usize>,
}

impl Vartable {
    pub fn new() -> Self {
        let mut list = LinkedList::new();
        list.push_front(VarScope(HashMap::new(), None));
        Vartable {
            vars: Vec::new(),
            names: list,
            storage_vars: HashMap::new(),
            dirty: Vec::new(),
            returns: Vec::new(),
        }
    }

    pub fn add(
        &mut self,
        id: &ast::Identifier,
        ty: resolver::Type,
        errors: &mut Vec<output::Output>,
    ) -> Option<usize> {
        if let Some(ref prev) = self.find_local(&id.name) {
            errors.push(Output::error_with_note(
                id.loc,
                format!("{} is already declared", id.name.to_string()),
                prev.id.loc,
                "location of previous declaration".to_string(),
            ));
            return None;
        }

        let pos = self.vars.len();

        self.vars.push(Variable {
            id: id.clone(),
            ty,
            pos,
            storage: Storage::Local,
        });

        self.names
            .front_mut()
            .unwrap()
            .0
            .insert(id.name.to_string(), pos);

        Some(pos)
    }

    fn find_local(&self, name: &str) -> Option<&Variable> {
        for scope in &self.names {
            if let Some(n) = scope.0.get(name) {
                return Some(&self.vars[*n]);
            }
        }

        None
    }

    pub fn find(
        &mut self,
        id: &ast::Identifier,
        contract: &resolver::Contract,
        errors: &mut Vec<output::Output>,
    ) -> Result<Variable, ()> {
        for scope in &self.names {
            if let Some(n) = scope.0.get(&id.name) {
                return Ok(self.vars[*n].clone());
            }
        }

        if let Some(n) = self.storage_vars.get(&id.name) {
            return Ok(self.vars[*n].clone());
        }

        let v = contract.resolve_var(&id, errors)?;
        let var = &contract.variables[v];
        let pos = self.vars.len();

        self.vars.push(Variable {
            id: id.clone(),
            ty: var.ty.clone(),
            pos,
            storage: match var.var {
                resolver::ContractVariableType::Storage(n) => Storage::Contract(n),
                resolver::ContractVariableType::Constant(n) => Storage::Constant(n),
            },
        });

        self.storage_vars.insert(id.name.to_string(), pos);

        Ok(self.vars[pos].clone())
    }

    pub fn temp(&mut self, id: &ast::Identifier, ty: &resolver::Type) -> usize {
        let pos = self.vars.len();

        self.vars.push(Variable {
            id: ast::Identifier {
                name: format!("{}.temp.{}", id.name, pos),
                loc: id.loc,
            },
            ty: ty.clone(),
            pos,
            storage: Storage::Local,
        });

        pos
    }

    pub fn new_scope(&mut self) {
        self.names.push_front(VarScope(HashMap::new(), None));
    }

    pub fn leave_scope(&mut self) {
        self.names.pop_front();
    }

    pub fn drain(self) -> Vec<Variable> {
        self.vars
    }

    // In order to create phi nodes, we need to track what vars are set in a certain scope
    pub fn set_dirty(&mut self, pos: usize) {
        for e in &mut self.dirty {
            if pos < e.lim {
                e.set.insert(pos);
            }
        }
    }

    pub fn new_dirty_tracker(&mut self) {
        self.dirty.push(DirtyTracker {
            lim: self.vars.len(),
            set: HashSet::new(),
        });
    }

    pub fn pop_dirty_tracker(&mut self) -> HashSet<usize> {
        self.dirty.pop().unwrap().set
    }
}

struct LoopScope {
    break_bb: usize,
    continue_bb: usize,
    no_breaks: usize,
    no_continues: usize,
}

struct LoopScopes(LinkedList<LoopScope>);

impl LoopScopes {
    fn new() -> Self {
        LoopScopes(LinkedList::new())
    }

    fn new_scope(&mut self, break_bb: usize, continue_bb: usize) {
        self.0.push_front(LoopScope {
            break_bb,
            continue_bb,
            no_breaks: 0,
            no_continues: 0,
        })
    }

    fn leave_scope(&mut self) -> LoopScope {
        self.0.pop_front().unwrap()
    }

    fn do_break(&mut self) -> Option<usize> {
        match self.0.front_mut() {
            Some(scope) => {
                scope.no_breaks += 1;
                Some(scope.break_bb)
            }
            None => None,
        }
    }

    fn do_continue(&mut self) -> Option<usize> {
        match self.0.front_mut() {
            Some(scope) => {
                scope.no_continues += 1;
                Some(scope.continue_bb)
            }
            None => None,
        }
    }
}

impl resolver::Type {
    fn default(&self, ns: &resolver::Contract) -> Expression {
        match self {
            resolver::Type::Primitive(e) => e.default(),
            resolver::Type::Enum(e) => ns.enums[*e].ty.default(),
            resolver::Type::Noreturn => unreachable!(),
            resolver::Type::FixedArray(_, _) => unreachable!(),
        }
    }
}

impl ast::PrimitiveType {
    fn default(self) -> Expression {
        match self {
            ast::PrimitiveType::Uint(b) | ast::PrimitiveType::Int(b) => {
                Expression::NumberLiteral(b, BigInt::from(0))
            }
            ast::PrimitiveType::Bool => Expression::BoolLiteral(false),
            ast::PrimitiveType::Address => Expression::NumberLiteral(160, BigInt::from(0)),
            ast::PrimitiveType::Bytes(n) => {
                let mut l = Vec::new();
                l.resize(n as usize, 0);
                Expression::BytesLiteral(l)
            }
            ast::PrimitiveType::DynamicBytes => unimplemented!(),
            ast::PrimitiveType::String => unimplemented!(),
        }
    }
}
