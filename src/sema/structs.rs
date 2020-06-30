use super::{Namespace, StructDecl, StructField, Symbol};
use output::Output;
use parser::ast;

/// Resolve a parsed struct definition. The return value will be true if the entire
/// definition is valid; however, whatever could be parsed will be added to the resolved
/// contract, so that we can continue producing compiler messages for the remainder
/// of the contract, even if the struct contains an invalid definition.
pub fn struct_decl(
    def: &pt::StructDefinition,
    contract_no: Option<usize>,
    ns: &mut Namespace,
    errors: &mut Vec<Output>,
) -> bool {
    let mut valid = true;
    let mut fields: Vec<StructField> = Vec::new();

    for field in &def.fields {
        let ty = match ns.resolve_type(contract_no, &field.ty, errors) {
            Ok(s) => s,
            Err(()) => {
                valid = false;
                continue;
            }
        };

        if let Some(other) = fields.iter().find(|f| f.name == field.name.name) {
            errors.push(Output::error_with_note(
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
            errors.push(Output::error(
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
            errors.push(Output::error(
                def.name.loc,
                format!("struct definition for ‘{}’ has no fields", def.name.name),
            ));
        }

        valid = false;
    }

    if valid {
        let pos = ns.structs.len();

        ns.structs.push(StructDecl {
            name: def.name.name.to_string(),
            contract: match contract_no {
                Some(ref contract_no) => Some(ns.contracts[*contract_no].name.to_owned()),
                None => None,
            },
            fields,
        });

        if !ns.add_symbol(
            contract_no,
            &def.name,
            Symbol::Struct(def.name.loc, pos),
            errors,
        ) {
            valid = false;
        }
    }

    valid
}
