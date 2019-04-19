use ast;
use cfg;
use output::{Output,Note};
use std::collections::HashMap;

#[derive(PartialEq,Clone)]
pub enum TypeName {
    Elementary(ast::ElementaryTypeName),
    Enum(usize),
}

impl TypeName {
    pub fn to_string(&self, ns: &ContractNameSpace) -> String {
        match self {
            TypeName::Elementary(e) => e.to_string(),
            TypeName::Enum(n) => format!("enum {}", ns.enums[*n].name)
        }
    }

    pub fn bits(&self) -> u16 {
       match self {
            TypeName::Elementary(e) => e.bits(),
            _ => panic!("type not allowed")
        }
    }

    pub fn signed(&self) -> bool {
       match self {
            TypeName::Elementary(e) => e.signed(),
            TypeName::Enum(_) => false
        }
    }

    pub fn ordered(&self) -> bool {
       match self {
            TypeName::Elementary(e) => e.ordered(),
            TypeName::Enum(_) => false
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

pub struct FunctionDecl {
    pub loc: ast::Loc,
    pub name: Option<String>,
    pub sig: String,
    pub ast_index: usize,
    pub params: Vec<TypeName>,
    pub returns: Vec<TypeName>,
    pub cfg: Option<Box<cfg::ControlFlowGraph>>,
}

pub enum Symbol {
    Enum(ast::Loc, usize),
    Function(Vec<(ast::Loc, usize)>),
}

pub struct ContractNameSpace {
    pub name: String,
    pub enums: Vec<EnumDecl>,
    // structs/events
    pub functions: Vec<FunctionDecl>,
    // state variables
    // constants
    symbols: HashMap<String, Symbol>,
}

impl ContractNameSpace {
    fn add_symbol(&mut self, id: &ast::Identifier, symbol: Symbol, errors: &mut Vec<Output>) {
        if let Some(prev) = self.symbols.get(&id.name) {
            match prev {
                Symbol::Enum(e, _) => {
                    errors.push(Output::error_with_note(id.loc, format!("{} is already defined as enum", id.name.to_string()),
                            e.clone(), "location of previous definition".to_string()));
                },
                Symbol::Function(v) => {
                    let mut notes = Vec::new();

                    for e in v {
                        notes.push(Note{pos: e.0.clone(), message: "location of previous definition".into()});
                    }

                    errors.push(Output::error_with_notes(id.loc, format!("{} is already defined as function", id.name.to_string()),
                            notes));
                }
            }
            return
        }

        self.symbols.insert(id.name.to_string(), symbol);
    }

    pub fn resolve(&self, id: &ast::TypeName, errors: &mut Vec<Output>) -> Option<TypeName> {
        match id {
            ast::TypeName::Elementary(e) => Some(TypeName::Elementary(*e)),
            ast::TypeName::Unresolved(s) => {
                match self.symbols.get(&s.name) {
                    None => {
                        errors.push(Output::error(s.loc, format!("`{}' is not declared", s.name)));
                        None
                    },
                    Some(Symbol::Enum(_, n)) => {
                        Some(TypeName::Enum(*n))
                    }
                    Some(Symbol::Function(_)) => {
                        errors.push(Output::error(s.loc, format!("`{}' is a function", s.name)));
                        None
                    }
                }
            }
        }
    }

    pub fn check_shadowing(&self, id: &ast::Identifier, errors: &mut Vec<Output>) {
        match self.symbols.get(&id.name) {
            Some(Symbol::Enum(_, _)) => {
                errors.push(Output::warning(id.loc, format!("declaration of `{}' shadows enum", id.name)));
                // FIXME: add location of enum
            },
            Some(Symbol::Function(_)) => {
                errors.push(Output::warning(id.loc, format!("declaration of `{}' shadows function", id.name)));
                // FIXME: add location of functionS
            },
            None => {}
        }
    }

    fn fallback_function(&self) -> Option<usize> {
        for (i, f) in self.functions.iter().enumerate() {
            if let None = f.name {
                return Some(i);
            }
        }
        return None;
    }

    pub fn to_string(&self) -> String {
        let mut s = String::new();

        for f in &self.functions {
            if let Some(ref name) = f.name {
                s.push_str(&format!("# function {}\n", name));
            } else {
                s.push_str(&format!("# constructor\n"));
            }

            if let Some(ref cfg) = f.cfg {
                s.push_str(&cfg.to_string(self));
            }
        }

        s
    }
}

pub fn resolver(s: ast::SourceUnit) -> (Vec<ContractNameSpace>, Vec<Output>) {
    let mut namespace = Vec::new();
    let mut errors = Vec::new();

    for part in s.parts {
        if let ast::SourceUnitPart::ContractDefinition(def) = part {
            if let Some(c) = resolve_contract(def, &mut errors) {
                namespace.push(c)
            }
        }
    }

    (namespace, errors)
}

fn resolve_contract(def: Box<ast::ContractDefinition>, errors: &mut Vec<Output>) -> Option<ContractNameSpace> {
    let mut ns = ContractNameSpace{
        name: def.name.name.to_string(),
        enums: Vec::new(),
        functions: Vec::new(),
        symbols: HashMap::new(),
    };

    // first resolve enums
    for parts in &def.parts {
        if let ast::ContractPart::EnumDefinition(ref e) = parts {
            let pos = ns.enums.len();

            ns.enums.push(enum_decl(e, errors));

            ns.add_symbol(&e.name, Symbol::Enum(e.name.loc, pos), errors);
        }
    }

    // FIXME: next resolve structs/event

    // FIXME: next resolve state variables

    // resolve function signatures
    for (i, parts) in def.parts.iter().enumerate() {
        if let ast::ContractPart::FunctionDefinition(ref f) = parts {
            func_decl(f, i, &mut ns, errors);
        }
    }

    let mut all_done = true;

    // resolve function bodies
    for f in 0..ns.functions.len() {
        let ast_index = ns.functions[f].ast_index;
        if let ast::ContractPart::FunctionDefinition(ref ast_f) = def.parts[ast_index] {
            match cfg::generate_cfg(ast_f, &ns.functions[f], &ns, errors) {
                Ok(c) => ns.functions[f].cfg = Some(c),
                Err(_) => all_done = false
            }
        }
    }

    if all_done {
        Some(ns)
    } else {
        None
    }
}

fn enum_decl(enum_: &ast::EnumDefinition, errors: &mut Vec<Output>) -> EnumDecl {
    // Number of bits required to represent this enum
    let mut bits = std::mem::size_of::<usize>() as u32 * 8 - (enum_.values.len() - 1).leading_zeros();
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
            errors.push(Output::error_with_note(e.loc, format!("duplicate enum value {}", e.name),
                prev.0.clone(), "location of previous definition".to_string()));
            continue;
        }
        
        entries.insert(e.name.to_string(), (e.loc, i));
    }

    EnumDecl{
        name: enum_.name.name.to_string(),
        ty: ast::ElementaryTypeName::Uint(bits as u16),
        values: entries
    }
}

#[test]
fn enum_256values_is_uint8() {
    let mut e = ast::EnumDefinition{
        name: ast::Identifier{loc: ast::Loc(0, 0), name: "foo".into()},
        values: Vec::new(),
    };

    e.values.push(ast::Identifier{loc: ast::Loc(0, 0), name: "first".into()});

    let f = enum_decl(&e, &mut Vec::new());
    assert_eq!(f.ty, ast::ElementaryTypeName::Uint(8));

    for i in 1..256 {
        e.values.push(ast::Identifier{loc: ast::Loc(0, 0), name: format!("val{}", i)})
    }

    assert_eq!(e.values.len(), 256);

    let r = enum_decl(&e, &mut Vec::new());
    assert_eq!(r.ty, ast::ElementaryTypeName::Uint(8));

    e.values.push(ast::Identifier{loc: ast::Loc(0, 0), name: "another".into()});

    let r2 = enum_decl(&e, &mut Vec::new());
    assert_eq!(r2.ty, ast::ElementaryTypeName::Uint(16));
}

fn func_decl(f: &ast::FunctionDefinition, i: usize, ns: &mut ContractNameSpace, errors: &mut Vec<Output>) {
    let mut params = Vec::new();
    let mut returns = Vec::new();
    let mut broken = false;

    for p in &f.params {
        match ns.resolve(&p.typ, errors) {
            Some(s) => params.push(s),
            None => { broken = true },
        }
    }

    for r in &f.returns {
        if let Some(ref n) = r.name {
            errors.push(Output::warning(n.loc, format!("named return value `{}' not allowed", n.name)));
        }

        match ns.resolve(&r.typ, errors) {
            Some(s) => returns.push(s),
            None => { broken = true },
        }
    }

    if broken {
        return;
    }

    let name = match f.name {
        Some(ref n) => Some(n.name.to_string()),
        None => None,
    };

    let fdecl = FunctionDecl{
        loc: f.loc,
        sig: external_signature(&name, &params, &ns),
        name: name,
        ast_index: i,
        params,
        returns,
        cfg: None
    };

    if let Some(ref id) = f.name {
        if let Some(Symbol::Function(ref mut v)) = ns.symbols.get_mut(&id.name) {
            // check if signature already present
            for o in v.iter() {
                if fdecl.sig == ns.functions[o.1].sig {
                    errors.push(Output::error_with_note(f.loc, "overloaded function with this signature already exist".to_string(),
                            o.0.clone(), "location of previous definition".to_string()));
                    return;
                }
            }

            let pos = ns.functions.len();

            ns.functions.push(fdecl);

            v.push((f.loc, pos));
            return;
        }

        let pos = ns.functions.len();

        ns.functions.push(fdecl);

        ns.add_symbol(id, Symbol::Function(vec!((id.loc, pos))), errors);
    } else {
        // fallback function
        if let Some(i) = ns.fallback_function() {
            let prev = &ns.functions[i];

            errors.push(Output::error_with_note(f.loc, "fallback function already defined".to_string(),
                    prev.loc, "location of previous definition".to_string()));

            return;
        }

        ns.functions.push(fdecl);
    }
}

pub fn external_signature(name: &Option<String>, params: &Vec<TypeName>, ns: &ContractNameSpace) -> String {
    let mut sig = match name { Some(ref n) => n.to_string(), None => "".to_string() };

    sig.push('(');

    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            sig.push(',');
        }

        sig.push_str(&match p {
            TypeName::Elementary(e) => e.to_string(),
            TypeName::Enum(i) => ns.enums[*i].ty.to_string()
        });
    }

    sig.push(')');

    sig
}

#[test]
fn signatures() {
    let ns = ContractNameSpace{
        name: String::from("foo"),
        enums: Vec::new(),
        functions: Vec::new(),
        symbols: HashMap::new(),
    };

    assert_eq!(external_signature(&Some("foo".to_string()), &vec!(
        TypeName::Elementary(ast::ElementaryTypeName::Uint(8)),
        TypeName::Elementary(ast::ElementaryTypeName::Address)), &ns),
        "foo(uint8,address)");
}