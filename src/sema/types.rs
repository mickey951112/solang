use super::ast::{Contract, EnumDecl, Namespace, StructDecl, StructField, Symbol, Type};
use parser::pt;
use sema::ast::Diagnostic;
use std::collections::HashMap;
#[cfg(test)]
use Target;

/// Resolve all the types we can find (enums, structs, contracts). structs can have other
/// structs as fields, including ones that have not been declared yet.
pub fn resolve_typenames<'a>(
    s: &'a pt::SourceUnit,
    file_no: usize,
    ns: &mut Namespace,
) -> Vec<(StructDecl, &'a pt::StructDefinition, Option<usize>)> {
    let mut structs = Vec::new();

    // Find all the types: contracts, enums, and structs. Either in a contract or not
    // We do not resolve the struct fields yet as we do not know all the possible types until we're
    // done
    for part in &s.0 {
        match part {
            pt::SourceUnitPart::ContractDefinition(def) => {
                resolve_contract(&def, file_no, &mut structs, ns);
            }
            pt::SourceUnitPart::EnumDefinition(def) => {
                let _ = enum_decl(&def, file_no, None, ns);
            }
            pt::SourceUnitPart::StructDefinition(def) => {
                if ns.add_symbol(
                    file_no,
                    None,
                    &def.name,
                    Symbol::Struct(def.name.loc, ns.structs.len()),
                ) {
                    let s = StructDecl {
                        name: def.name.name.to_owned(),
                        loc: def.name.loc,
                        contract: None,
                        fields: Vec::new(),
                    };

                    structs.push((s, def, None));
                }
            }
            _ => (),
        }
    }

    structs
}

pub fn resolve_structs(
    structs: Vec<(StructDecl, &pt::StructDefinition, Option<usize>)>,
    file_no: usize,
    ns: &mut Namespace,
) {
    // now we can resolve the fields for the structs
    for (mut decl, def, contract) in structs {
        if let Some(fields) = struct_decl(def, file_no, contract, ns) {
            decl.fields = fields;
            ns.structs.push(decl);
        }
    }

    // struct can contain other structs, and we have to check for recursiveness,
    // i.e. "struct a { b f1; } struct b { a f1; }"
    for s in 0..ns.structs.len() {
        fn check(s: usize, file_no: usize, struct_fields: &mut Vec<usize>, ns: &mut Namespace) {
            let def = ns.structs[s].clone();
            let mut types_seen = Vec::new();

            for field in &def.fields {
                if let Type::Struct(n) = field.ty {
                    if types_seen.contains(&n) {
                        continue;
                    }

                    types_seen.push(n);

                    if struct_fields.contains(&n) {
                        ns.diagnostics.push(Diagnostic::error_with_note(
                            def.loc,
                            format!("struct ‘{}’ has infinite size", def.name),
                            field.loc,
                            format!("recursive field ‘{}’", field.name),
                        ));
                    } else {
                        struct_fields.push(n);
                        check(n, file_no, struct_fields, ns);
                    }
                }
            }
        };

        check(s, file_no, &mut vec![s], ns);
    }
}

/// Resolve all the types in a contract
fn resolve_contract<'a>(
    def: &'a pt::ContractDefinition,
    file_no: usize,
    structs: &mut Vec<(StructDecl, &'a pt::StructDefinition, Option<usize>)>,
    ns: &mut Namespace,
) -> bool {
    let contract_no = ns.contracts.len();
    ns.contracts
        .push(Contract::new(&def.name.name, def.ty.clone(), def.loc));

    let mut broken = !ns.add_symbol(
        file_no,
        None,
        &def.name,
        Symbol::Contract(def.loc, contract_no),
    );

    for parts in &def.parts {
        match parts {
            pt::ContractPart::EnumDefinition(ref e) => {
                if !enum_decl(e, file_no, Some(contract_no), ns) {
                    broken = true;
                }
            }
            pt::ContractPart::StructDefinition(ref s) => {
                if ns.add_symbol(
                    file_no,
                    Some(contract_no),
                    &s.name,
                    Symbol::Struct(s.name.loc, structs.len()),
                ) {
                    let decl = StructDecl {
                        name: s.name.name.to_owned(),
                        loc: s.name.loc,
                        contract: Some(def.name.name.to_owned()),
                        fields: Vec::new(),
                    };

                    structs.push((decl, s, Some(contract_no)));
                } else {
                    broken = true;
                }
            }
            _ => (),
        }
    }

    broken
}

/// Resolve a parsed struct definition. The return value will be true if the entire
/// definition is valid; however, whatever could be parsed will be added to the resolved
/// contract, so that we can continue producing compiler messages for the remainder
/// of the contract, even if the struct contains an invalid definition.
pub fn struct_decl(
    def: &pt::StructDefinition,
    file_no: usize,
    contract_no: Option<usize>,
    ns: &mut Namespace,
) -> Option<Vec<StructField>> {
    let mut valid = true;
    let mut fields: Vec<StructField> = Vec::new();

    for field in &def.fields {
        let ty = match ns.resolve_type(file_no, contract_no, false, &field.ty) {
            Ok(s) => s,
            Err(()) => {
                valid = false;
                continue;
            }
        };

        if let Some(other) = fields.iter().find(|f| f.name == field.name.name) {
            ns.diagnostics.push(Diagnostic::error_with_note(
                field.name.loc,
                format!(
                    "struct ‘{}’ has duplicate struct field ‘{}’",
                    def.name.name, field.name.name
                ),
                other.loc,
                format!("location of previous declaration of ‘{}’", other.name),
            ));
            valid = false;
            continue;
        }

        // memory/calldata make no sense for struct fields.
        // TODO: ethereum foundation solidity does not allow storage fields
        // in structs, but this is perfectly possible. The struct would not be
        // allowed as parameter/return types of public functions though.
        if let Some(storage) = &field.storage {
            ns.diagnostics.push(Diagnostic::error(
                *storage.loc(),
                format!(
                    "storage location ‘{}’ not allowed for struct field",
                    storage
                ),
            ));
            valid = false;
        }

        fields.push(StructField {
            loc: field.name.loc,
            name: field.name.name.to_string(),
            ty,
        });
    }

    if fields.is_empty() {
        if valid {
            ns.diagnostics.push(Diagnostic::error(
                def.name.loc,
                format!("struct definition for ‘{}’ has no fields", def.name.name),
            ));
        }

        valid = false;
    }

    if valid {
        Some(fields)
    } else {
        None
    }
}

/// Parse enum declaration. If the declaration is invalid, it is still generated
/// so that we can continue parsing, with errors recorded.
fn enum_decl(
    enum_: &pt::EnumDefinition,
    file_no: usize,
    contract_no: Option<usize>,
    ns: &mut Namespace,
) -> bool {
    let mut valid = true;

    let mut bits = if enum_.values.is_empty() {
        ns.diagnostics.push(Diagnostic::error(
            enum_.name.loc,
            format!("enum ‘{}’ is missing fields", enum_.name.name),
        ));
        valid = false;

        0
    } else {
        // Number of bits required to represent this enum
        std::mem::size_of::<usize>() as u32 * 8 - (enum_.values.len() - 1).leading_zeros()
    };

    // round it up to the next
    if bits <= 8 {
        bits = 8;
    } else {
        bits += 7;
        bits -= bits % 8;
    }

    // check for duplicates
    let mut entries: HashMap<String, (pt::Loc, usize)> = HashMap::new();

    for (i, e) in enum_.values.iter().enumerate() {
        if let Some(prev) = entries.get(&e.name.to_string()) {
            ns.diagnostics.push(Diagnostic::error_with_note(
                e.loc,
                format!("duplicate enum value {}", e.name),
                prev.0,
                "location of previous definition".to_string(),
            ));
            valid = false;
            continue;
        }

        entries.insert(e.name.to_string(), (e.loc, i));
    }

    let decl = EnumDecl {
        name: enum_.name.name.to_string(),
        loc: enum_.loc,
        contract: match contract_no {
            Some(c) => Some(ns.contracts[c].name.to_owned()),
            None => None,
        },
        ty: Type::Uint(bits as u16),
        values: entries,
    };

    let pos = ns.enums.len();

    ns.enums.push(decl);

    if !ns.add_symbol(
        file_no,
        contract_no,
        &enum_.name,
        Symbol::Enum(enum_.name.loc, pos),
    ) {
        valid = false;
    }

    valid
}

#[test]
fn enum_256values_is_uint8() {
    let mut e = pt::EnumDefinition {
        doc: vec![],
        loc: pt::Loc(0, 0, 0),
        name: pt::Identifier {
            loc: pt::Loc(0, 0, 0),
            name: "foo".into(),
        },
        values: Vec::new(),
    };

    let mut ns = Namespace::new(Target::Ewasm, 20, 16);

    e.values.push(pt::Identifier {
        loc: pt::Loc(0, 0, 0),
        name: "first".into(),
    });

    assert!(enum_decl(&e, 0, None, &mut ns));
    assert_eq!(ns.enums.last().unwrap().ty, Type::Uint(8));

    for i in 1..256 {
        e.values.push(pt::Identifier {
            loc: pt::Loc(0, 0, 0),
            name: format!("val{}", i),
        })
    }

    assert_eq!(e.values.len(), 256);

    e.name.name = "foo2".to_owned();
    assert!(enum_decl(&e, 0, None, &mut ns));
    assert_eq!(ns.enums.last().unwrap().ty, Type::Uint(8));

    e.values.push(pt::Identifier {
        loc: pt::Loc(0, 0, 0),
        name: "another".into(),
    });

    e.name.name = "foo3".to_owned();
    assert!(enum_decl(&e, 0, None, &mut ns));
    assert_eq!(ns.enums.last().unwrap().ty, Type::Uint(16));
}
