
use parser::ast;
use output::{Note, Output};
use serde::Serialize;
use std::collections::HashMap;

pub mod cfg;

// FIXME: Burrow ABIs do not belong here
#[derive(Serialize)]
pub struct ABIParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Serialize)]
pub struct ABI {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub inputs: Vec<ABIParam>,
    pub outputs: Vec<ABIParam>,
    pub constant: bool,
    pub payable: bool,
    #[serde(rename = "stateMutability")]
    pub mutability: &'static str,
}

#[derive(PartialEq, Clone)]
pub enum Target {
    Substrate,
    Burrow
}

#[derive(PartialEq, Clone)]
pub enum TypeName {
    Elementary(ast::ElementaryTypeName),
    Enum(usize),
    Noreturn,
}

impl TypeName {
    pub fn to_string(&self, ns: &Contract) -> String {
        match self {
            TypeName::Elementary(e) => e.to_string(),
            TypeName::Enum(n) => format!("enum {}", ns.enums[*n].name),
            TypeName::Noreturn => "no return".to_owned(),
        }
    }

    pub fn bits(&self) -> u16 {
        match self {
            TypeName::Elementary(e) => e.bits(),
            _ => panic!("type not allowed"),
        }
    }

    pub fn signed(&self) -> bool {
        match self {
            TypeName::Elementary(e) => e.signed(),
            TypeName::Enum(_) => false,
            TypeName::Noreturn => unreachable!(),
        }
    }

    pub fn ordered(&self) -> bool {
        match self {
            TypeName::Elementary(e) => e.ordered(),
            TypeName::Enum(_) => false,
            TypeName::Noreturn => unreachable!(),
        }
    }

    pub fn new_bool() -> Self {
        TypeName::Elementary(ast::ElementaryTypeName::Bool)
    }
}

pub struct EnumDecl {
    pub name: String,
    pub ty: ast::ElementaryTypeName,
    pub values: HashMap<String, (ast::Loc, usize)>,
}

pub struct Parameter {
    pub name: String,
    pub ty: TypeName,
}

impl Parameter {
    fn to_abi(&self, ns: &Contract) -> ABIParam {
        ABIParam {
            name: self.name.to_string(),
            ty: match &self.ty {
                TypeName::Elementary(e) => e.to_string(),
                TypeName::Enum(ref i) => ns.enums[*i].ty.to_string(),
                TypeName::Noreturn => unreachable!(),
            },
        }
    }
}

pub struct FunctionDecl {
    pub loc: ast::Loc,
    pub name: String,
    pub signature: String,
    pub ast_index: usize,
    pub mutability: Option<ast::StateMutability>,
    pub visibility: ast::Visibility,
    pub params: Vec<Parameter>,
    pub returns: Vec<Parameter>,
    pub cfg: Option<Box<cfg::ControlFlowGraph>>,
}

impl FunctionDecl {
    fn new(loc: ast::Loc, name: String, ast_index: usize, mutability: Option<ast::StateMutability>,
        visibility: ast::Visibility, params: Vec<Parameter>, returns: Vec<Parameter>, ns: &Contract) -> Self {
        let mut signature = name.to_owned();

        signature.push('(');
    
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                signature.push(',');
            }
    
            signature.push_str(&match &p.ty {
                TypeName::Elementary(e) => e.to_string(),
                TypeName::Enum(i) => ns.enums[*i].ty.to_string(),
                TypeName::Noreturn => unreachable!(),
            });
        }
    
        signature.push(')');

        FunctionDecl{
            loc, name, signature, ast_index, mutability, visibility, params, returns, cfg: None
        }
    }

    pub fn wasm_symbol(&self, ns: &Contract) -> String {
        let mut sig = self.name.to_owned();

        if !self.params.is_empty() {
            sig.push_str("__");

            for (i, p) in self.params.iter().enumerate() {
                if i > 0 {
                    sig.push('_');
                }

                sig.push_str(&match &p.ty {
                    TypeName::Elementary(e) => e.to_string(),
                    TypeName::Enum(i) => ns.enums[*i].name.to_owned(),
                    TypeName::Noreturn => unreachable!(),
                });
            }
        }

        sig
    }
}

pub struct ContractVariable {
    pub name: String,
    pub ty: TypeName,
    pub visibility: ast::Visibility,
    pub storage: Option<usize>,
}

pub enum Symbol {
    Enum(ast::Loc, usize),
    Function(Vec<(ast::Loc, usize)>),
    Variable(ast::Loc, usize),
}

pub struct Contract {
    pub name: String,
    pub enums: Vec<EnumDecl>,
    // structs/events
    pub constructors: Vec<FunctionDecl>,
    pub functions: Vec<FunctionDecl>,
    pub variables: Vec<ContractVariable>,
    pub target: Target,
    top_of_contract_storage: usize,
    symbols: HashMap<String, Symbol>,
}

impl Contract {
    fn add_symbol(
        &mut self,
        id: &ast::Identifier,
        symbol: Symbol,
        errors: &mut Vec<Output>,
    ) -> bool {
        if let Some(prev) = self.symbols.get(&id.name) {
            match prev {
                Symbol::Enum(e, _) => {
                    errors.push(Output::error_with_note(
                        id.loc,
                        format!("{} is already defined as enum", id.name.to_string()),
                        e.clone(),
                        "location of previous definition".to_string(),
                    ));
                }
                Symbol::Function(v) => {
                    let mut notes = Vec::new();

                    for e in v {
                        notes.push(Note {
                            pos: e.0.clone(),
                            message: "location of previous definition".into(),
                        });
                    }

                    errors.push(Output::error_with_notes(
                        id.loc,
                        format!("{} is already defined as function", id.name.to_string()),
                        notes,
                    ));
                }
                Symbol::Variable(e, _) => {
                    errors.push(Output::error_with_note(
                        id.loc,
                        format!(
                            "{} is already defined as state variable",
                            id.name.to_string()
                        ),
                        e.clone(),
                        "location of previous definition".to_string(),
                    ));
                }
            }
            return false;
        }

        self.symbols.insert(id.name.to_string(), symbol);

        true
    }

    pub fn resolve_type(&self, id: &ast::TypeName, errors: &mut Vec<Output>) -> Result<TypeName, ()> {
        match id {
            ast::TypeName::Elementary(e) => Ok(TypeName::Elementary(*e)),
            ast::TypeName::Unresolved(s) => match self.symbols.get(&s.name) {
                None => {
                    errors.push(Output::decl_error(
                        s.loc,
                        format!("`{}' is not declared", s.name),
                    ));
                    Err(())
                }
                Some(Symbol::Enum(_, n)) => Ok(TypeName::Enum(*n)),
                Some(Symbol::Function(_)) => {
                    errors.push(Output::decl_error(
                        s.loc,
                        format!("`{}' is a function", s.name),
                    ));
                    Err(())
                }
                Some(Symbol::Variable(_, n)) => Ok(self.variables[*n].ty.clone()),
            },
        }
    }

    pub fn resolve_enum(&self, id: &ast::Identifier) -> Option<usize> {
        match self.symbols.get(&id.name) {
            Some(Symbol::Enum(_, n)) => Some(*n),
            _ => None,
        }
    }

    pub fn resolve_func(&self, id: &ast::Identifier, errors: &mut Vec<Output>) -> Result<&Vec<(ast::Loc, usize)>, ()> {
        match self.symbols.get(&id.name) {
            Some(Symbol::Function(v)) => Ok(v),
            _ => {
                errors.push(Output::error(
                    id.loc.clone(),
                    format!("unknown function or type"),
                ));

                Err(())
            }
        }
    }

    pub fn resolve_var(&self, id: &ast::Identifier, errors: &mut Vec<Output>) -> Result<usize, ()> {
        match self.symbols.get(&id.name) {
            None => {
                errors.push(Output::decl_error(
                    id.loc.clone(),
                    format!("`{}' is not declared", id.name),
                ));
                Err(())
            }
            Some(Symbol::Enum(_, _)) => {
                errors.push(Output::decl_error(
                    id.loc.clone(),
                    format!("`{}' is an enum", id.name),
                ));
                Err(())
            }
            Some(Symbol::Function(_)) => {
                errors.push(Output::decl_error(
                    id.loc.clone(),
                    format!("`{}' is a function", id.name),
                ));
                Err(())
            }
            Some(Symbol::Variable(_, n)) => Ok(*n),
        }
    }

    pub fn check_shadowing(&self, id: &ast::Identifier, errors: &mut Vec<Output>) {
        match self.symbols.get(&id.name) {
            Some(Symbol::Enum(loc, _)) => {
                errors.push(Output::warning_with_note(
                    id.loc,
                    format!("declaration of `{}' shadows enum", id.name),
                    loc.clone(),
                    format!("previous declaration of enum"),
                ));
            }
            Some(Symbol::Function(v)) => {
                let notes = v
                    .iter()
                    .map(|(pos, _)| Note {
                        pos: pos.clone(),
                        message: "previous declaration of function".to_owned(),
                    })
                    .collect();
                errors.push(Output::warning_with_notes(
                    id.loc,
                    format!("declaration of `{}' shadows function", id.name),
                    notes,
                ));
            }
            Some(Symbol::Variable(loc, _)) => {
                errors.push(Output::warning_with_note(
                    id.loc,
                    format!("declaration of `{}' shadows state variable", id.name),
                    loc.clone(),
                    format!("previous declaration of state variable"),
                ));
            }
            None => {}
        }
    }

    pub fn fallback_function(&self) -> Option<usize> {
        for (i, f) in self.functions.iter().enumerate() {
            if f.name == "" {
                return Some(i);
            }
        }
        return None;
    }

    pub fn generate_abi(&self) -> Vec<ABI> {
        let mut abis = Vec::new();

        for f in &self.constructors {
            abis.push(ABI {
                name: "".to_owned(),
                constant: match &f.cfg {
                    Some(cfg) => !cfg.writes_contract_storage,
                    None => false,
                },
                mutability: match &f.mutability {
                    Some(n) => n.to_string(),
                    None => "nonpayable",
                },
                payable: match &f.mutability {
                    Some(ast::StateMutability::Payable(_)) => true,
                    _ => false,
                },
                ty: "constructor".to_owned(),
                inputs: f.params.iter().map(|p| p.to_abi(&self)).collect(),
                outputs: f.returns.iter().map(|p| p.to_abi(&self)).collect(),
            })

        }

        for f in &self.functions {
            abis.push(ABI {
                name: f.name.to_owned(),
                constant: match &f.cfg {
                    Some(cfg) => !cfg.writes_contract_storage,
                    None => false,
                },
                mutability: match &f.mutability {
                    Some(n) => n.to_string(),
                    None => "nonpayable",
                },
                payable: match &f.mutability {
                    Some(ast::StateMutability::Payable(_)) => true,
                    _ => false,
                },
                ty: if f.name == "" {
                    "fallback".to_owned()
                } else {
                    "function".to_owned()
                },
                inputs: f.params.iter().map(|p| p.to_abi(&self)).collect(),
                outputs: f.returns.iter().map(|p| p.to_abi(&self)).collect(),
            })
        }

        abis
    }

    pub fn to_string(&self) -> String {
        let mut s = format!("#\n# Contract: {}\n", self.name);

        for f in &self.constructors {
            s.push_str(&format!("# constructor {}\n", f.signature));

            if let Some(ref cfg) = f.cfg {
                s.push_str(&cfg.to_string(self));
            }
        }

        for f in &self.functions {
            if f.name != "" {
                s.push_str(&format!("# function {}\n", f.signature));
            } else {
                s.push_str(&format!("# fallback\n"));
            }

            if let Some(ref cfg) = f.cfg {
                s.push_str(&cfg.to_string(self));
            }
        }

        s
    }
}

pub fn resolver(s: ast::SourceUnit, target: &Target) -> (Vec<Contract>, Vec<Output>) {
    let mut contracts = Vec::new();
    let mut errors = Vec::new();

    for part in s.0 {
        if let ast::SourceUnitPart::ContractDefinition(def) = part {
            if let Some(c) = resolve_contract(def, &target, &mut errors) {
                contracts.push(c)
            }
        }
    }

    (contracts, errors)
}

fn resolve_contract(
    def: Box<ast::ContractDefinition>,
    target: &Target,
    errors: &mut Vec<Output>,
) -> Option<Contract> {
    let mut ns = Contract {
        name: def.name.name.to_string(),
        enums: Vec::new(),
        constructors: Vec::new(),
        functions: Vec::new(),
        variables: Vec::new(),
        target: target.clone(),
        top_of_contract_storage: 0,
        symbols: HashMap::new(),
    };

    errors.push(Output::info(
        def.loc,
        format!("found contract {}", def.name.name),
    ));

    let mut broken = false;

    // first resolve enums
    for parts in &def.parts {
        if let ast::ContractPart::EnumDefinition(ref e) = parts {
            let pos = ns.enums.len();

            ns.enums.push(enum_decl(e, errors));

            if !ns.add_symbol(&e.name, Symbol::Enum(e.name.loc, pos), errors) {
                broken = true;
            }
        }
    }

    // FIXME: next resolve structs/event

    // resolve function signatures
    for (i, parts) in def.parts.iter().enumerate() {
        if let ast::ContractPart::FunctionDefinition(ref f) = parts {
            if !func_decl(f, i, &mut ns, errors) {
                broken = true;
            }
        }
    }

    // resolve state variables
    for parts in &def.parts {
        if let ast::ContractPart::ContractVariableDefinition(ref s) = parts {
            if !var_decl(s, &mut ns, errors) {
                broken = true;
            }
        }
    }

    // resolve constructor bodies
    for f in 0..ns.constructors.len() {
        let ast_index = ns.constructors[f].ast_index;
        if let ast::ContractPart::FunctionDefinition(ref ast_f) = def.parts[ast_index] {
            match cfg::generate_cfg(ast_f, &ns.constructors[f], &ns, errors) {
                Ok(c) =>  ns.constructors[f].cfg = Some(c),
                Err(_) => broken = true
            }
        }
    }

    // resolve function bodies
    for f in 0..ns.functions.len() {
        let ast_index = ns.functions[f].ast_index;
        if let ast::ContractPart::FunctionDefinition(ref ast_f) = def.parts[ast_index] {
            match cfg::generate_cfg(ast_f, &ns.functions[f], &ns, errors) {
                Ok(c) => {
                    match &ns.functions[f].mutability {
                        Some(ast::StateMutability::Pure(loc)) => {
                            if c.writes_contract_storage {
                                errors.push(Output::error(
                                    loc.clone(),
                                    format!("function declared pure but writes contract storage"),
                                ));
                                broken = true;
                            } else if c.reads_contract_storage {
                                errors.push(Output::error(
                                    loc.clone(),
                                    format!("function declared pure but reads contract storage"),
                                ));
                                broken = true;
                            }
                        }
                        Some(ast::StateMutability::View(loc)) => {
                            if c.writes_contract_storage {
                                errors.push(Output::error(
                                    loc.clone(),
                                    format!("function declared view but writes contract storage"),
                                ));
                                broken = true;
                            } else if !c.reads_contract_storage {
                                errors.push(Output::warning(
                                    loc.clone(),
                                    format!("function can be declared pure"),
                                ));
                            }
                        }
                        Some(ast::StateMutability::Payable(_)) => {
                            unimplemented!();
                        }
                        None => {
                            let loc = &ns.functions[f].loc;

                            if !c.writes_contract_storage && !c.reads_contract_storage {
                                errors.push(Output::warning(
                                    loc.clone(),
                                    format!("function can be declare pure"),
                                ));
                            } else if !c.writes_contract_storage {
                                errors.push(Output::warning(
                                    loc.clone(),
                                    format!("function can be declared view"),
                                ));
                            }
                        }
                    }
                    ns.functions[f].cfg = Some(c);
                }
                Err(_) => broken = true
            }
        }
    }

    if !broken {
        Some(ns)
    } else {
        None
    }
}

fn enum_decl(enum_: &ast::EnumDefinition, errors: &mut Vec<Output>) -> EnumDecl {
    // Number of bits required to represent this enum
    let mut bits =
        std::mem::size_of::<usize>() as u32 * 8 - (enum_.values.len() - 1).leading_zeros();
    // round it up to the next
    if bits <= 8 {
        bits = 8;
    } else {
        bits += 7;
        bits -= bits % 8;
    }

    // check for duplicates
    let mut entries: HashMap<String, (ast::Loc, usize)> = HashMap::new();

    for (i, e) in enum_.values.iter().enumerate() {
        if let Some(prev) = entries.get(&e.name.to_string()) {
            errors.push(Output::error_with_note(
                e.loc,
                format!("duplicate enum value {}", e.name),
                prev.0.clone(),
                "location of previous definition".to_string(),
            ));
            continue;
        }

        entries.insert(e.name.to_string(), (e.loc, i));
    }

    EnumDecl {
        name: enum_.name.name.to_string(),
        ty: ast::ElementaryTypeName::Uint(bits as u16),
        values: entries,
    }
}

#[test]
fn enum_256values_is_uint8() {
    let mut e = ast::EnumDefinition {
        name: ast::Identifier {
            loc: ast::Loc(0, 0),
            name: "foo".into(),
        },
        values: Vec::new(),
    };

    e.values.push(ast::Identifier {
        loc: ast::Loc(0, 0),
        name: "first".into(),
    });

    let f = enum_decl(&e, &mut Vec::new());
    assert_eq!(f.ty, ast::ElementaryTypeName::Uint(8));

    for i in 1..256 {
        e.values.push(ast::Identifier {
            loc: ast::Loc(0, 0),
            name: format!("val{}", i),
        })
    }

    assert_eq!(e.values.len(), 256);

    let r = enum_decl(&e, &mut Vec::new());
    assert_eq!(r.ty, ast::ElementaryTypeName::Uint(8));

    e.values.push(ast::Identifier {
        loc: ast::Loc(0, 0),
        name: "another".into(),
    });

    let r2 = enum_decl(&e, &mut Vec::new());
    assert_eq!(r2.ty, ast::ElementaryTypeName::Uint(16));
}

fn var_decl(
    s: &ast::ContractVariableDefinition,
    ns: &mut Contract,
    errors: &mut Vec<Output>,
) -> bool {
    let ty = match ns.resolve_type(&s.ty, errors) {
        Ok(s) => s,
        Err(()) => {
            return false;
        }
    };

    let mut is_constant = false;
    let mut visibility: Option<ast::Visibility> = None;

    for attr in &s.attrs {
        match &attr {
            ast::VariableAttribute::Constant(loc) => {
                if is_constant {
                    errors.push(Output::warning(
                        loc.clone(),
                        format!("duplicate constant attribute"),
                    ));
                }
                is_constant = true;
            }
            ast::VariableAttribute::Visibility(ast::Visibility::External(loc)) => {
                errors.push(Output::error(
                    loc.clone(),
                    format!("variable cannot be declared external"),
                ));
                return false;
            }
            ast::VariableAttribute::Visibility(v) => {
                if let Some(e) = &visibility {
                    errors.push(Output::error_with_note(
                        v.loc().clone(),
                        format!("variable redeclared `{}'", v.to_string()),
                        e.loc().clone(),
                        format!("location of previous declaration of `{}'", e.to_string()),
                    ));
                    return false;
                }

                visibility = Some(v.clone());
            }
        }
    }

    let visibility = match visibility {
        Some(v) => v,
        None => ast::Visibility::Private(ast::Loc(0, 0)),
    };

    if is_constant && s.initializer == None {
        errors.push(Output::decl_error(
            s.loc.clone(),
            format!("missing initializer for constant"),
        ));
        return false;
    }

    let storage = if !is_constant {
        let storage = ns.top_of_contract_storage;
        ns.top_of_contract_storage += 1;
        Some(storage)
    } else {
        None
    };

    let sdecl = ContractVariable {
        name: s.name.name.to_string(),
        storage,
        visibility,
        ty,
    };

    // FIXME: resolve init expression and check for constant (if constant)
    // init expression can call functions and access other state variables

    let pos = ns.variables.len();

    ns.variables.push(sdecl);

    ns.add_symbol(&s.name, Symbol::Variable(s.loc, pos), errors)
}

fn func_decl(
    f: &ast::FunctionDefinition,
    i: usize,
    ns: &mut Contract,
    errors: &mut Vec<Output>,
) -> bool {
    let mut params = Vec::new();
    let mut returns = Vec::new();
    let mut success = true;

    if f.constructor && !f.returns.is_empty() {
        errors.push(Output::warning(
            f.loc,
            format!("constructor cannot have return values"),
        ));
        return false;
    } else if !f.constructor && f.name == None {
        if !f.returns.is_empty() {
            errors.push(Output::warning(
                f.loc,
                format!("fallback function cannot have return values"),
            ));
            success = false;
        }

        if !f.params.is_empty() {
            errors.push(Output::warning(
                f.loc,
                format!("fallback function cannot have parameters"),
            ));
            success = false;
        }
    }

    for p in &f.params {
        match ns.resolve_type(&p.typ, errors) {
            Ok(s) => params.push(Parameter {
                name: p
                    .name
                    .as_ref()
                    .map_or("".to_string(), |id| id.name.to_string()),
                ty: s,
            }),
            Err(()) => success = false,
        }
    }

    for r in &f.returns {
        // FIXME: these should be allowed
        if let Some(ref n) = r.name {
            errors.push(Output::warning(
                n.loc,
                format!("named return value `{}' not allowed", n.name),
            ));
        }

        match ns.resolve_type(&r.typ, errors) {
            Ok(s) => returns.push(Parameter {
                name: r
                    .name
                    .as_ref()
                    .map_or("".to_string(), |id| id.name.to_string()),
                ty: s,
            }),
            Err(()) => success = false,
        }
    }

    let mut mutability: Option<ast::StateMutability> = None;
    let mut visibility: Option<ast::Visibility> = None;

    for a in &f.attributes {
        match &a {
            ast::FunctionAttribute::StateMutability(m) => {
                if let Some(e) = &mutability {
                    errors.push(Output::error_with_note(
                        m.loc().clone(),
                        format!("function redeclared `{}'", m.to_string()),
                        e.loc().clone(),
                        format!("location of previous declaration of `{}'", e.to_string()),
                    ));
                    success = false;
                    continue;
                }

                mutability = Some(m.clone());
            }
            ast::FunctionAttribute::Visibility(v) => {
                if let Some(e) = &visibility {
                    errors.push(Output::error_with_note(
                        v.loc().clone(),
                        format!("function redeclared `{}'", v.to_string()),
                        e.loc().clone(),
                        format!("location of previous declaration of `{}'", e.to_string()),
                    ));
                    success = false;
                    continue;
                }

                visibility = Some(v.clone());
            }
        }
    }

    if f.constructor {
        match mutability {
            Some(ast::StateMutability::Pure(loc)) => {
                errors.push(Output::error(
                    loc,
                    format!("constructor cannot be declared pure"),
                ));
                success = false;
            },
            Some(ast::StateMutability::View(loc)) => {
                errors.push(Output::error(
                    loc,
                    format!("constructor cannot be declared view"),
                ));
                success = false;
            },
            _ => ()
        }
    }

    if visibility == None {
        errors.push(Output::error(
            f.loc,
            format!("function has no visibility specifier"),
        ));
        success = false;
    }

    if !success {
        return false;
    }

    let name = match f.name {
        Some(ref n) => n.name.to_owned(),
        None => "".to_owned()
    };

    let fdecl = FunctionDecl::new(f.loc, name, i, mutability, visibility.unwrap(), params, returns, &ns);

    if f.constructor {
        // In the eth solidity, only one constructor is allowed
        if ns.target == Target::Burrow && !ns.constructors.is_empty() {
            let prev = &ns.constructors[i];
            errors.push(Output::error_with_note(
                f.loc,
                "constructor already defined".to_string(),
                prev.loc,
                "location of previous definition".to_string(),
            ));
            return false;
        }

        // FIXME: Internal visibility is allowed on inherented contract, but we don't support those yet
        match fdecl.visibility {
            ast::Visibility::Public(_) => (),
            _ => {
                errors.push(Output::error(
                    f.loc,
                    "constructor function must be declared public".to_owned()
                ));
                return false;
            }
        }

        for v in ns.constructors.iter() {
            if v.signature == fdecl.signature {
                errors.push(Output::error_with_note(
                    f.loc,
                    "constructor with this signature already exists".to_string(),
                    v.loc,
                    "location of previous definition".to_string(),
                ));

                return false;
            }
        }

        ns.constructors.push(fdecl);

        true
    } else if let Some(ref id) = f.name {
        if let Some(Symbol::Function(ref mut v)) = ns.symbols.get_mut(&id.name) {
            // check if signature already present
            for o in v.iter() {
                if ns.functions[o.1].signature == fdecl.signature {
                    errors.push(Output::error_with_note(
                        f.loc,
                        "overloaded function with this signature already exist".to_string(),
                        o.0.clone(),
                        "location of previous definition".to_string(),
                    ));
                    return false;
                }
            }

            let pos = ns.functions.len();

            ns.functions.push(fdecl);

            v.push((f.loc, pos));
            return true;
        }

        let pos = ns.functions.len();

        ns.functions.push(fdecl);

        ns.add_symbol(id, Symbol::Function(vec![(id.loc, pos)]), errors)
    } else {
        // fallback function
        if let Some(i) = ns.fallback_function() {
            let prev = &ns.functions[i];
            
            errors.push(Output::error_with_note(
                f.loc,
                "fallback function already defined".to_string(),
                prev.loc,
                "location of previous definition".to_string(),
            ));
            return false;
        }

        if let ast::Visibility::External(_) = fdecl.visibility {
            // ok
        } else {
            errors.push(Output::error(
                f.loc,
                "fallback function must be declared external".to_owned()
            ));
            return false;
        }
        
        ns.functions.push(fdecl);

        true
    }
}

#[test]
fn signatures() {
    let ns = Contract {
        name: String::from("foo"),
        enums: Vec::new(),
        constructors: Vec::new(),
        functions: Vec::new(),
        variables: Vec::new(),
        target: crate::resolver::Target::Burrow,
        top_of_contract_storage: 0,
        symbols: HashMap::new(),
    };

    let fdecl = FunctionDecl::new(
        ast::Loc(0, 0), "foo".to_owned(), 0, None, ast::Visibility::Public(ast::Loc(0, 0)),
        vec!(
            Parameter {
                name: "".to_string(),
                ty: TypeName::Elementary(ast::ElementaryTypeName::Uint(8))
            },
            Parameter {
                name: "".to_string(),
                ty: TypeName::Elementary(ast::ElementaryTypeName::Address)
            },
        ), Vec::new(), &ns);

    assert_eq!(fdecl.signature, "foo(uint8,address)");
}
