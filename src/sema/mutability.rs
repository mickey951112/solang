use super::ast::{Builtin, DestructureField, Expression, Function, Namespace, Statement};
use output::Output;
use parser::pt::{FunctionTy, Loc, StateMutability};

/// check state mutablity
pub fn mutablity(ns: &mut Namespace) {
    for contract_no in 0..ns.contracts.len() {
        for function_no in 0..ns.contracts[contract_no].functions.len() {
            let diagnostics = check_mutability(contract_no, function_no, ns);

            ns.diagnostics.extend(diagnostics);
        }
    }
}

/// While we recurse through the AST, maintain some state
struct StateCheck<'a> {
    diagnostics: Vec<Output>,
    does_read_state: bool,
    does_write_state: bool,
    can_read_state: bool,
    can_write_state: bool,
    func: &'a Function,
    ns: &'a Namespace,
    contract_no: usize,
}

impl<'a> StateCheck<'a> {
    fn write(&mut self, loc: &Loc) {
        if !self.can_write_state {
            self.diagnostics.push(Output::error(
                *loc,
                format!(
                    "function declared ‘{}’ but this expression writes to state",
                    self.func.print_mutability()
                ),
            ));
        }

        self.does_write_state = true;
    }

    fn read(&mut self, loc: &Loc) {
        if !self.can_read_state {
            self.diagnostics.push(Output::error(
                *loc,
                format!(
                    "function declared ‘{}’ but this expression reads from state",
                    self.func.print_mutability()
                ),
            ));
        }

        self.does_read_state = true;
    }
}

fn check_mutability(contract_no: usize, function_no: usize, ns: &Namespace) -> Vec<Output> {
    let func = &ns.contracts[contract_no].functions[function_no];

    let mut state = StateCheck {
        diagnostics: Vec::new(),
        does_read_state: false,
        does_write_state: false,
        can_write_state: false,
        can_read_state: false,
        func,
        ns,
        contract_no,
    };

    match func.mutability {
        Some(StateMutability::Pure(_)) => (),
        Some(StateMutability::View(_)) => {
            state.can_read_state = true;
        }
        Some(StateMutability::Payable(_)) | None => {
            state.can_read_state = true;
            state.can_write_state = true;
        }
    };

    recurse_statements(&func.body, &mut state);

    if FunctionTy::Function == func.ty {
        if !state.does_write_state && !state.does_read_state {
            match func.mutability {
                Some(StateMutability::Payable(_)) | Some(StateMutability::Pure(_)) => (),
                _ => {
                    state.diagnostics.push(Output::warning(
                        func.loc,
                        format!(
                            "function declared ‘{}’ can be declared ‘pure’",
                            func.print_mutability()
                        ),
                    ));
                }
            }
        }

        if !state.does_write_state && state.does_read_state && func.mutability.is_none() {
            state.diagnostics.push(Output::warning(
                func.loc,
                "function declared can be declared ‘view’".to_string(),
            ));
        }
    }

    state.diagnostics
}

fn recurse_statements(stmts: &[Statement], state: &mut StateCheck) {
    for stmt in stmts.iter() {
        match stmt {
            Statement::VariableDecl(_, _, _, Some(expr)) => {
                expr.recurse(state, read_expression);
            }
            Statement::If(_, _, expr, then_, else_) => {
                expr.recurse(state, read_expression);
                recurse_statements(then_, state);
                recurse_statements(else_, state);
            }
            Statement::DoWhile(_, _, body, expr) | Statement::While(_, _, expr, body) => {
                expr.recurse(state, read_expression);
                recurse_statements(body, state);
            }
            Statement::For {
                init,
                cond,
                next,
                body,
                ..
            } => {
                recurse_statements(init, state);
                if let Some(cond) = cond {
                    cond.recurse(state, read_expression);
                }
                recurse_statements(next, state);
                recurse_statements(body, state);
            }
            Statement::Expression(_, _, expr) => {
                expr.recurse(state, read_expression);
            }
            Statement::Delete(loc, _, _) => state.write(loc),
            Statement::Destructure(_, fields, expr) => {
                // This is either a list or internal/external function call
                expr.recurse(state, read_expression);

                for field in fields {
                    if let DestructureField::Expression(expr) = field {
                        expr.recurse(state, write_expression);
                    }
                }
            }
            Statement::Return(_, exprs) => {
                for e in exprs {
                    e.recurse(state, read_expression);
                }
            }
            Statement::TryCatch {
                expr,
                ok_stmt,
                error,
                catch_stmt,
                ..
            } => {
                expr.recurse(state, read_expression);
                recurse_statements(ok_stmt, state);
                if let Some((_, _, s)) = error {
                    recurse_statements(s, state);
                }
                recurse_statements(catch_stmt, state);
            }
            _ => (),
        }
    }
}

fn read_expression(expr: &Expression, state: &mut StateCheck) -> bool {
    match expr {
        Expression::PreIncrement(_, _, expr)
        | Expression::PreDecrement(_, _, expr)
        | Expression::PostIncrement(_, _, expr)
        | Expression::PostDecrement(_, _, expr) => {
            expr.recurse(state, write_expression);
        }
        Expression::Assign(_, _, left, right) => {
            right.recurse(state, read_expression);
            left.recurse(state, write_expression);
        }
        Expression::StorageBytesLength(loc, _)
        | Expression::StorageBytesSubscript(loc, _, _)
        | Expression::StorageVariable(loc, _, _)
        | Expression::StorageLoad(loc, _, _) => state.read(loc),
        Expression::StorageBytesPush(loc, _, _) | Expression::StorageBytesPop(loc, _) => {
            state.write(loc);
        }
        Expression::Balance(loc, _, _) | Expression::GetAddress(loc, _) => state.read(loc),

        Expression::Builtin(loc, _, Builtin::BlockNumber, _)
        | Expression::Builtin(loc, _, Builtin::Timestamp, _)
        | Expression::Builtin(loc, _, Builtin::BlockCoinbase, _)
        | Expression::Builtin(loc, _, Builtin::BlockDifficulty, _)
        | Expression::Builtin(loc, _, Builtin::BlockHash, _)
        | Expression::Builtin(loc, _, Builtin::Sender, _)
        | Expression::Builtin(loc, _, Builtin::Origin, _)
        | Expression::Builtin(loc, _, Builtin::Gasleft, _)
        | Expression::Builtin(loc, _, Builtin::Gasprice, _)
        | Expression::Builtin(loc, _, Builtin::GasLimit, _)
        | Expression::Builtin(loc, _, Builtin::TombstoneDeposit, _)
        | Expression::Builtin(loc, _, Builtin::MinimumBalance, _)
        | Expression::Builtin(loc, _, Builtin::Random, _) => state.read(loc),
        Expression::Builtin(loc, _, Builtin::PayableSend, _)
        | Expression::Builtin(loc, _, Builtin::PayableTransfer, _)
        | Expression::Builtin(loc, _, Builtin::ArrayPush, _)
        | Expression::Builtin(loc, _, Builtin::ArrayPop, _)
        | Expression::Builtin(loc, _, Builtin::BytesPush, _)
        | Expression::Builtin(loc, _, Builtin::BytesPop, _)
        | Expression::Builtin(loc, _, Builtin::SelfDestruct, _) => state.write(loc),
        Expression::Constructor { loc, .. } => {
            state.write(loc);
        }
        Expression::InternalFunctionCall(loc, _, function_no, _) => {
            match &state.ns.contracts[state.contract_no].functions[*function_no].mutability {
                None | Some(StateMutability::Payable(_)) => state.write(loc),
                Some(StateMutability::View(_)) => state.read(loc),
                Some(StateMutability::Pure(_)) => (),
            };
        }
        Expression::ExternalFunctionCall {
            loc,
            contract_no,
            function_no,
            ..
        } => {
            match &state.ns.contracts[*contract_no].functions[*function_no].mutability {
                None | Some(StateMutability::Payable(_)) => state.write(loc),
                Some(StateMutability::View(_)) => state.read(loc),
                Some(StateMutability::Pure(_)) => (),
            };
        }
        _ => {
            return true;
        }
    }
    false
}

fn write_expression(expr: &Expression, state: &mut StateCheck) -> bool {
    if let Expression::StorageVariable(loc, _, _) = expr {
        state.write(loc);
        false
    } else {
        read_expression(expr, state)
    }
}
