use super::ast::{Function, Namespace, Parameter, Symbol, Type};
use output::Output;
use parser::pt;
use Target;

pub fn function_decl(
    f: &pt::FunctionDefinition,
    i: usize,
    contract_no: usize,
    ns: &mut Namespace,
) -> bool {
    let mut success = true;

    // The parser allows constructors to have return values. This is so that we can give a
    // nicer error message than "returns unexpected"
    match f.ty {
        pt::FunctionTy::Function => {
            // Function name cannot be the same as the contract name
            if let Some(n) = &f.name {
                if n.name == ns.contracts[contract_no].name {
                    ns.diagnostics.push(Output::error(
                        f.loc,
                        "function cannot have same name as the contract".to_string(),
                    ));
                    return false;
                }
            } else {
                ns.diagnostics.push(Output::error(
                    f.name_loc,
                    "function is missing a name. did you mean ‘fallback() extern {…}’ or ‘receive() extern {…}’?".to_string(),
                ));
                return false;
            }
        }
        pt::FunctionTy::Constructor => {
            if !f.returns.is_empty() {
                ns.diagnostics.push(Output::warning(
                    f.loc,
                    "constructor cannot have return values".to_string(),
                ));
                return false;
            }
            if f.name.is_some() {
                ns.diagnostics.push(Output::warning(
                    f.loc,
                    "constructor cannot have a name".to_string(),
                ));
                return false;
            }
        }
        pt::FunctionTy::Fallback | pt::FunctionTy::Receive => {
            if !f.returns.is_empty() {
                ns.diagnostics.push(Output::warning(
                    f.loc,
                    format!("{} function cannot have return values", f.ty),
                ));
                success = false;
            }
            if !f.params.is_empty() {
                ns.diagnostics.push(Output::warning(
                    f.loc,
                    format!("{} function cannot have parameters", f.ty),
                ));
                success = false;
            }
            if f.name.is_some() {
                ns.diagnostics.push(Output::warning(
                    f.loc,
                    format!("{} function cannot have a name", f.ty),
                ));
                return false;
            }
        }
    }

    let mut mutability: Option<pt::StateMutability> = None;
    let mut visibility: Option<pt::Visibility> = None;

    for a in &f.attributes {
        match &a {
            pt::FunctionAttribute::StateMutability(m) => {
                if let Some(e) = &mutability {
                    ns.diagnostics.push(Output::error_with_note(
                        m.loc(),
                        format!("function redeclared `{}'", m.to_string()),
                        e.loc(),
                        format!("location of previous declaration of `{}'", e.to_string()),
                    ));
                    success = false;
                    continue;
                }

                mutability = Some(m.clone());
            }
            pt::FunctionAttribute::Visibility(v) => {
                if let Some(e) = &visibility {
                    ns.diagnostics.push(Output::error_with_note(
                        v.loc(),
                        format!("function redeclared `{}'", v.to_string()),
                        e.loc(),
                        format!("location of previous declaration of `{}'", e.to_string()),
                    ));
                    success = false;
                    continue;
                }

                visibility = Some(v.clone());
            }
        }
    }

    let visibility = match visibility {
        Some(v) => v,
        None => {
            ns.diagnostics
                .push(Output::error(f.loc, "no visibility specified".to_string()));
            success = false;
            // continue processing while assuming it's a public
            pt::Visibility::Public(pt::Loc(0, 0))
        }
    };

    // Reference types can't be passed through the ABI encoder/decoder, so
    // storage parameters/returns are only allowed in internal/private functions
    let storage_allowed = match visibility {
        pt::Visibility::Internal(_) | pt::Visibility::Private(_) => {
            if let Some(pt::StateMutability::Payable(loc)) = mutability {
                ns.diagnostics.push(Output::error(
                    loc,
                    "internal or private function cannot be payable".to_string(),
                ));
                success = false;
            }
            true
        }
        pt::Visibility::Public(_) | pt::Visibility::External(_) => false,
    };

    let (params, params_success) = resolve_params(f, storage_allowed, contract_no, ns);

    let (returns, returns_success) = resolve_returns(f, storage_allowed, contract_no, ns);

    if !success || !returns_success || !params_success {
        return false;
    }

    let name = match &f.name {
        Some(s) => s.name.to_owned(),
        None => "".to_owned(),
    };

    let fdecl = Function::new(
        f.loc,
        name,
        f.doc.clone(),
        f.ty.clone(),
        Some(i),
        mutability,
        visibility,
        params,
        returns,
        ns,
    );

    if f.ty == pt::FunctionTy::Constructor {
        // In the eth solidity, only one constructor is allowed
        if ns.target == Target::Ewasm {
            if let Some(prev) = ns.contracts[contract_no]
                .functions
                .iter()
                .find(|f| f.is_constructor())
            {
                ns.diagnostics.push(Output::error_with_note(
                    f.loc,
                    "constructor already defined".to_string(),
                    prev.loc,
                    "location of previous definition".to_string(),
                ));
                return false;
            }
        } else {
            let payable = fdecl.is_payable();

            if let Some(prev) = ns.contracts[contract_no]
                .functions
                .iter()
                .find(|f| f.is_constructor() && f.is_payable() != payable)
            {
                ns.diagnostics.push(Output::error_with_note(
                    f.loc,
                    "all constructors should be defined ‘payable’ or not".to_string(),
                    prev.loc,
                    "location of previous definition".to_string(),
                ));
                return false;
            }
        }

        // FIXME: Internal visibility is allowed on abstract contracts, but we don't support those yet
        match fdecl.visibility {
            pt::Visibility::Public(_) => (),
            _ => {
                ns.diagnostics.push(Output::error(
                    f.loc,
                    "constructor function must be declared public".to_owned(),
                ));
                return false;
            }
        }

        match fdecl.mutability {
            Some(pt::StateMutability::Pure(loc)) => {
                ns.diagnostics.push(Output::error(
                    loc,
                    "constructor cannot be declared pure".to_string(),
                ));
                return false;
            }
            Some(pt::StateMutability::View(loc)) => {
                ns.diagnostics.push(Output::error(
                    loc,
                    "constructor cannot be declared view".to_string(),
                ));
                return false;
            }
            _ => (),
        }

        for v in ns.contracts[contract_no]
            .functions
            .iter()
            .filter(|f| f.is_constructor())
        {
            if v.signature == fdecl.signature {
                ns.diagnostics.push(Output::error_with_note(
                    f.loc,
                    "constructor with this signature already exists".to_string(),
                    v.loc,
                    "location of previous definition".to_string(),
                ));

                return false;
            }
        }

        ns.contracts[contract_no].functions.push(fdecl);

        true
    } else if f.ty == pt::FunctionTy::Receive || f.ty == pt::FunctionTy::Fallback {
        if let Some(prev) = ns.contracts[contract_no]
            .functions
            .iter()
            .find(|o| o.ty == f.ty)
        {
            ns.diagnostics.push(Output::error_with_note(
                f.loc,
                format!("{} function already defined", f.ty),
                prev.loc,
                "location of previous definition".to_string(),
            ));
            return false;
        }

        if let pt::Visibility::External(_) = fdecl.visibility {
            // ok
        } else {
            ns.diagnostics.push(Output::error(
                f.loc,
                format!("{} function must be declared external", f.ty),
            ));
            return false;
        }

        if let Some(pt::StateMutability::Payable(_)) = fdecl.mutability {
            if f.ty == pt::FunctionTy::Fallback {
                ns.diagnostics.push(Output::error(
                    f.loc,
                    format!("{} function must not be declare payable, use ‘receive() external payable’ instead", f.ty),
                ));
                return false;
            }
        } else if f.ty == pt::FunctionTy::Receive {
            ns.diagnostics.push(Output::error(
                f.loc,
                format!("{} function must be declared payable", f.ty),
            ));
            return false;
        }

        ns.contracts[contract_no].functions.push(fdecl);

        true
    } else {
        let id = f.name.as_ref().unwrap();

        if let Some(Symbol::Function(ref mut v)) =
            ns.symbols.get_mut(&(Some(contract_no), id.name.to_owned()))
        {
            // check if signature already present
            for o in v.iter() {
                if ns.contracts[contract_no].functions[o.1].signature == fdecl.signature {
                    ns.diagnostics.push(Output::error_with_note(
                        f.loc,
                        "overloaded function with this signature already exist".to_string(),
                        o.0,
                        "location of previous definition".to_string(),
                    ));
                    return false;
                }
            }

            let pos = ns.contracts[contract_no].functions.len();

            ns.contracts[contract_no].functions.push(fdecl);

            v.push((f.loc, pos));
            return true;
        }

        let pos = ns.contracts[contract_no].functions.len();

        ns.contracts[contract_no].functions.push(fdecl);

        ns.add_symbol(Some(contract_no), id, Symbol::Function(vec![(id.loc, pos)]));

        true
    }
}

/// Resolve the parameters
fn resolve_params(
    f: &pt::FunctionDefinition,
    storage_allowed: bool,
    contract_no: usize,
    ns: &mut Namespace,
) -> (Vec<Parameter>, bool) {
    let mut params = Vec::new();
    let mut success = true;

    for (loc, p) in &f.params {
        let p = match p {
            Some(p) => p,
            None => {
                ns.diagnostics
                    .push(Output::error(*loc, "missing parameter type".to_owned()));
                success = false;
                continue;
            }
        };

        match ns.resolve_type(Some(contract_no), false, &p.ty) {
            Ok(ty) => {
                let ty = if !ty.can_have_data_location() {
                    if let Some(storage) = &p.storage {
                        ns.diagnostics.push(Output::error(
                                *storage.loc(),
                                format!("data location ‘{}’ can only be specified for array, struct or mapping",
                                storage)
                            ));
                        success = false;
                    }

                    ty
                } else if let Some(pt::StorageLocation::Storage(loc)) = p.storage {
                    if storage_allowed {
                        Type::StorageRef(Box::new(ty))
                    } else {
                        ns.diagnostics.push(Output::error(
                            loc,
                            "parameter of type ‘storage’ not allowed public or external functions"
                                .to_string(),
                        ));
                        success = false;
                        ty
                    }
                } else if ty.contains_mapping(ns) {
                    ns.diagnostics.push(Output::error(
                        p.ty.loc(),
                        "parameter with mapping type must be of type ‘storage’".to_string(),
                    ));
                    success = false;
                    ty
                } else {
                    ty
                };

                params.push(Parameter {
                    loc: *loc,
                    name: p
                        .name
                        .as_ref()
                        .map_or("".to_string(), |id| id.name.to_string()),
                    ty,
                });
            }
            Err(()) => success = false,
        }
    }

    (params, success)
}

/// Resolve the return values
fn resolve_returns(
    f: &pt::FunctionDefinition,
    storage_allowed: bool,
    contract_no: usize,
    ns: &mut Namespace,
) -> (Vec<Parameter>, bool) {
    let mut returns = Vec::new();
    let mut success = true;

    for (loc, r) in &f.returns {
        let r = match r {
            Some(r) => r,
            None => {
                ns.diagnostics
                    .push(Output::error(*loc, "missing return type".to_owned()));
                success = false;
                continue;
            }
        };

        match ns.resolve_type(Some(contract_no), false, &r.ty) {
            Ok(ty) => {
                let ty = if !ty.can_have_data_location() {
                    if let Some(storage) = &r.storage {
                        ns.diagnostics.push(Output::error(
                                *storage.loc(),
                                format!("data location ‘{}’ can only be specified for array, struct or mapping",
                                storage)
                            ));
                        success = false;
                    }

                    ty
                } else {
                    match r.storage {
                        Some(pt::StorageLocation::Calldata(loc)) => {
                            ns.diagnostics.push(Output::error(
                                loc,
                                "data location ‘calldata’ can not be used for return types"
                                    .to_string(),
                            ));
                            success = false;
                            ty
                        }
                        Some(pt::StorageLocation::Storage(loc)) => {
                            if storage_allowed {
                                Type::StorageRef(Box::new(ty))
                            } else {
                                ns.diagnostics.push(Output::error(
                                    loc,
                                    "return type of type ‘storage’ not allowed public or external functions"
                                        .to_string(),
                                ));
                                success = false;
                                ty
                            }
                        }
                        _ => {
                            if ty.contains_mapping(ns) {
                                ns.diagnostics.push(Output::error(
                                    r.ty.loc(),
                                    "return type containing mapping must be of type ‘storage’"
                                        .to_string(),
                                ));
                                success = false;
                            }

                            ty
                        }
                    }
                };

                returns.push(Parameter {
                    loc: *loc,
                    name: r
                        .name
                        .as_ref()
                        .map_or("".to_string(), |id| id.name.to_string()),
                    ty,
                });
            }
            Err(()) => success = false,
        }
    }

    (returns, success)
}

#[test]
fn signatures() {
    use super::*;

    let ns = Namespace::new(Target::Ewasm, 20);

    let fdecl = Function::new(
        pt::Loc(0, 0),
        "foo".to_owned(),
        vec![],
        pt::FunctionTy::Function,
        Some(0),
        None,
        pt::Visibility::Public(pt::Loc(0, 0)),
        vec![
            Parameter {
                loc: pt::Loc(0, 0),
                name: "".to_string(),
                ty: Type::Uint(8),
            },
            Parameter {
                loc: pt::Loc(0, 0),
                name: "".to_string(),
                ty: Type::Address(false),
            },
        ],
        Vec::new(),
        &ns,
    );

    assert_eq!(fdecl.signature, "foo(uint8,address)");
}
