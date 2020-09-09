// Parity Substrate style ABIs/metadata
use num_traits::ToPrimitive;
use parser::pt;
use sema::ast;
use serde::{Deserialize, Serialize};

/// Substrate contracts abi consists of a a registry of strings and types, the contract itself
#[derive(Deserialize, Serialize)]
pub struct Metadata {
    pub registry: Registry,
    storage: Storage,
    pub contract: Contract,
}

impl Metadata {
    pub fn get_function(&self, name: &str) -> Option<&Message> {
        self.contract
            .messages
            .iter()
            .find(|m| name == self.registry.get_str(m.name))
    }
}

/// The registry holds strings and types. Presumably this is to avoid duplication
#[derive(Deserialize, Serialize)]
pub struct Registry {
    strings: Vec<String>,
    types: Vec<Type>,
}

#[derive(Deserialize, Serialize)]
pub struct Array {
    #[serde(rename = "array.len")]
    len: usize,
    #[serde(rename = "array.type")]
    ty: usize,
}

#[derive(Deserialize, Serialize)]
pub struct Slice {
    #[serde(rename = "slice.type")]
    ty: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Type {
    Builtin { id: String, def: String },
    BuiltinArray { id: Array, def: String },
    BuiltinSlice { id: Slice, def: String },
    StructSimpleId { id: String, def: StructDef },
    Struct { id: CustomID, def: StructDef },
    Enum { id: CustomID, def: EnumDef },
}

#[derive(Deserialize, Serialize)]
pub struct Contract {
    pub name: usize,
    pub constructors: Vec<Constructor>,
    pub messages: Vec<Message>,
    pub events: Vec<Event>,
}

#[derive(Deserialize, Serialize)]
struct BuiltinType {
    id: String,
    def: String,
}

#[derive(Deserialize, Serialize)]
struct EnumVariant {
    name: usize,
    discriminant: usize,
}

#[derive(Deserialize, Serialize)]
struct EnumDef {
    #[serde(rename = "clike_enum.variants")]
    variants: Vec<EnumVariant>,
}

#[derive(Deserialize, Serialize)]
struct CustomID {
    #[serde(rename = "custom.name")]
    name: usize,
    #[serde(rename = "custom.namespace")]
    namespace: Vec<usize>,
    #[serde(rename = "custom.params")]
    params: Vec<usize>,
}

#[derive(Deserialize, Serialize)]
struct StructDef {
    #[serde(rename = "struct.fields")]
    fields: Vec<StructField>,
}

#[derive(Deserialize, Serialize)]
struct StructField {
    name: usize,
    #[serde(rename = "type")]
    ty: usize,
}

#[derive(Deserialize, Serialize)]
pub struct Constructor {
    pub name: usize,
    pub selector: String,
    pub docs: Vec<String>,
    args: Vec<Param>,
}

impl Constructor {
    /// Build byte string from
    pub fn selector(&self) -> Vec<u8> {
        parse_selector(&self.selector)
    }
}

#[derive(Deserialize, Serialize)]
pub struct Message {
    pub name: usize,
    pub selector: String,
    pub docs: Vec<String>,
    mutates: bool,
    args: Vec<Param>,
    return_type: Option<ParamType>,
}

impl Message {
    /// Build byte string from
    pub fn selector(&self) -> Vec<u8> {
        parse_selector(&self.selector)
    }
}

#[derive(Deserialize, Serialize)]
pub struct Event {
    docs: Vec<String>,
    name: usize,
    args: Vec<ParamIndexed>,
}

#[derive(Deserialize, Serialize)]
struct Param {
    name: usize,
    #[serde(rename = "type")]
    ty: ParamType,
}

#[derive(Deserialize, Serialize)]
struct ParamIndexed {
    #[serde(flatten)]
    param: Param,
    indexed: bool,
}

#[derive(Deserialize, Serialize)]
struct ParamType {
    ty: usize,
    display_name: Vec<usize>,
}

#[derive(Deserialize, Serialize)]
struct Storage {
    #[serde(rename = "struct.type")]
    ty: usize,
    #[serde(rename = "struct.fields")]
    fields: Vec<StorageLayout>,
}

#[derive(Deserialize, Serialize)]
struct LayoutField {
    #[serde(rename = "range.offset")]
    offset: String,
    #[serde(rename = "range.len")]
    len: String,
    #[serde(rename = "range.elem_type")]
    ty: usize,
}

#[derive(Deserialize, Serialize)]
struct StorageLayout {
    name: usize,
    layout: StorageFieldLayout,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum StorageFieldLayout {
    Field(LayoutField),
    Storage(Box<Storage>),
}

/// Create a new registry and create new entries. Note that the registry is
/// accessed by number, and the first entry is 1, not 0.
impl Registry {
    fn new() -> Self {
        Registry {
            strings: Vec::new(),
            types: Vec::new(),
        }
    }

    /// Returns index to string in registry. String is added if not already present
    fn string(&mut self, name: &str) -> usize {
        for (i, s) in self.strings.iter().enumerate() {
            if s == name {
                return i + 1;
            }
        }

        let length = self.strings.len();

        self.strings.push(name.to_owned());

        length + 1
    }

    /// Returns the string at the specified index
    pub fn get_str(&self, index: usize) -> &str {
        &self.strings[index - 1]
    }

    /// Returns index to builtin type in registry. Type is added if not already present
    fn builtin_type(&mut self, ty: &str) -> usize {
        for (i, s) in self.types.iter().enumerate() {
            match s {
                Type::Builtin { id, .. } if id == ty => {
                    return i + 1;
                }
                _ => (),
            }
        }

        let length = self.types.len();

        self.types.push(Type::Builtin {
            id: ty.to_owned(),
            def: "builtin".to_owned(),
        });

        length + 1
    }

    /// Returns index to builtin type in registry. Type is added if not already present
    fn builtin_array_type(&mut self, elem: usize, array_len: usize) -> usize {
        for (i, s) in self.types.iter().enumerate() {
            match s {
                Type::BuiltinArray {
                    id: Array { len, ty },
                    ..
                } if *len == array_len && *ty == elem => {
                    return i + 1;
                }
                _ => (),
            }
        }

        let length = self.types.len();

        self.types.push(Type::BuiltinArray {
            id: Array {
                len: array_len,
                ty: elem,
            },
            def: "builtin".to_owned(),
        });

        length + 1
    }

    /// Returns index to builtin type in registry. Type is added if not already present
    fn builtin_slice_type(&mut self, elem: usize) -> usize {
        for (i, s) in self.types.iter().enumerate() {
            match s {
                Type::BuiltinSlice {
                    id: Slice { ty }, ..
                } if *ty == elem => {
                    return i + 1;
                }
                _ => (),
            }
        }

        let length = self.types.len();

        self.types.push(Type::BuiltinSlice {
            id: Slice { ty: elem },
            def: "builtin".to_owned(),
        });

        length + 1
    }

    /// Returns index to builtin type in registry. Type is added if not already present
    fn string_type(&mut self) -> usize {
        let ty_u8 = self.builtin_type("u8");

        let elem_ty = self.builtin_slice_type(ty_u8);
        let name = self.string("elems");

        let elem_ty = self.struct_type("Vec", vec![StructField { name, ty: elem_ty }]);

        let name = self.string("vec");

        self.struct_simpleid_type("str".to_owned(), vec![StructField { name, ty: elem_ty }])
    }

    /// Returns index to builtin type in registry. Type is added if not already present
    #[allow(dead_code)]
    fn builtin_enum_type(&mut self, e: &ast::EnumDecl) -> usize {
        let length = self.types.len();
        let name = self.string(&e.name);

        let t = Type::Enum {
            id: CustomID {
                name,
                namespace: Vec::new(),
                params: Vec::new(),
            },
            def: EnumDef {
                variants: e
                    .values
                    .iter()
                    .map(|(key, val)| EnumVariant {
                        name: self.string(key),
                        discriminant: val.1,
                    })
                    .collect(),
            },
        };
        self.types.push(t);

        length + 1
    }

    /// Adds struct type to registry. Does not check for duplication (yet)
    fn struct_type(&mut self, name: &str, fields: Vec<StructField>) -> usize {
        let length = self.types.len();
        let name = self.string(name);

        self.types.push(Type::Struct {
            id: CustomID {
                name,
                namespace: Vec::new(),
                params: Vec::new(),
            },
            def: StructDef { fields },
        });

        length + 1
    }

    /// Adds struct type to registry. Does not check for duplication (yet)
    fn struct_simpleid_type(&mut self, name: String, fields: Vec<StructField>) -> usize {
        let length = self.types.len();

        self.types.push(Type::StructSimpleId {
            id: name,
            def: StructDef { fields },
        });

        length + 1
    }
}

pub fn load(bs: &str) -> Result<Metadata, serde_json::error::Error> {
    serde_json::from_str(bs)
}

pub fn gen_abi(contract_no: usize, ns: &ast::Namespace) -> Metadata {
    let mut registry = Registry::new();

    let fields = ns.contracts[contract_no]
        .variables
        .iter()
        .filter(|v| !v.is_storage())
        .map(|v| StructField {
            name: registry.string(&v.name),
            ty: ty_to_abi(&v.ty, ns, &mut registry).ty,
        })
        .collect();

    let storagety = registry.struct_type("storage", fields);

    let fields = ns.contracts[contract_no]
        .layout
        .iter()
        .filter_map(|layout| {
            let var = &ns.contracts[layout.contract_no].variables[layout.var_no];

            if !var.ty.is_mapping() {
                Some(StorageLayout {
                    name: registry.string(&var.name),
                    layout: StorageFieldLayout::Field(LayoutField {
                        offset: format!("0x{:064X}", layout.slot),
                        len: var.ty.storage_slots(ns).to_string(),
                        ty: ty_to_abi(&var.ty, ns, &mut registry).ty,
                    }),
                })
            } else {
                None
            }
        })
        .collect();

    let storage = Storage {
        ty: storagety,
        fields: vec![StorageLayout {
            name: registry.string("Storage"),
            layout: StorageFieldLayout::Storage(Box::new(Storage {
                ty: storagety,
                fields,
            })),
        }],
    };

    let mut constructors = ns.contracts[contract_no]
        .functions
        .iter()
        .filter(|f| f.is_constructor())
        .map(|f| Constructor {
            name: registry.string("new"),
            selector: render_selector(f),
            args: f
                .params
                .iter()
                .map(|p| parameter_to_abi(p, ns, &mut registry))
                .collect(),
            docs: f.doc.clone(),
        })
        .collect::<Vec<Constructor>>();

    if let Some((f, _)) = &ns.contracts[contract_no].default_constructor {
        constructors.push(Constructor {
            name: registry.string("new"),
            selector: render_selector(f),
            args: f
                .params
                .iter()
                .map(|p| parameter_to_abi(p, ns, &mut registry))
                .collect(),
            docs: f.doc.clone(),
        });
    }

    let messages = ns.contracts[contract_no]
        .all_functions
        .keys()
        .filter_map(|(base_contract_no, function_no)| {
            if ns.contracts[*base_contract_no].is_library() {
                None
            } else {
                Some(&ns.contracts[*base_contract_no].functions[*function_no])
            }
        })
        .filter(|f| match f.visibility {
            pt::Visibility::Public(_) | pt::Visibility::External(_) => {
                f.ty == pt::FunctionTy::Function
            }
            _ => false,
        })
        .map(|f| Message {
            name: registry.string(&f.name),
            mutates: f.mutability.is_none(),
            return_type: match f.returns.len() {
                0 => None,
                1 => Some(ty_to_abi(&f.returns[0].ty, ns, &mut registry)),
                _ => {
                    let fields = f
                        .returns
                        .iter()
                        .map(|f| StructField {
                            name: registry.string(&f.name),
                            ty: ty_to_abi(&f.ty, ns, &mut registry).ty,
                        })
                        .collect();

                    Some(ParamType {
                        ty: registry.struct_type("", fields),
                        display_name: vec![],
                    })
                }
            },
            selector: render_selector(f),
            args: f
                .params
                .iter()
                .map(|p| parameter_to_abi(p, ns, &mut registry))
                .collect(),
            docs: f.doc.clone(),
        })
        .collect();

    let events = ns.contracts[contract_no]
        .sends_events
        .iter()
        .map(|event_no| {
            let event = &ns.events[*event_no];

            let name = registry.string(&event.name);
            let args = event
                .fields
                .iter()
                .map(|p| ParamIndexed {
                    param: parameter_to_abi(p, ns, &mut registry),
                    indexed: p.indexed,
                })
                .collect();
            let docs = event.doc.clone();

            Event { name, args, docs }
        })
        .collect();

    let contract = Contract {
        name: registry.string(&ns.contracts[contract_no].name),
        constructors,
        messages,
        events,
    };

    Metadata {
        registry,
        storage,
        contract,
    }
}

fn ty_to_abi(ty: &ast::Type, ns: &ast::Namespace, registry: &mut Registry) -> ParamType {
    match ty {
        /* clike_enums are broken in polkadot. Use u8 for now.
        ast::Type::Enum(n) => ParamType {
            ty: registry.builtin_enum_type(&contract.enums[*n]),
            display_name: vec![registry.string(&contract.enums[*n].name)],
        },
        */
        ast::Type::Enum(_) => ParamType {
            ty: registry.builtin_type("u8"),
            display_name: vec![registry.string("u8")],
        },
        ast::Type::Bytes(n) => {
            let elem = registry.builtin_type("u8");
            ParamType {
                ty: registry.builtin_array_type(elem, *n as usize),
                display_name: vec![],
            }
        }
        ast::Type::Mapping(_, _) => unreachable!(),
        ast::Type::Array(ty, dims) => {
            let mut param_ty = ty_to_abi(ty, ns, registry);

            for d in dims {
                if let Some(d) = d {
                    param_ty = ParamType {
                        ty: registry.builtin_array_type(param_ty.ty, d.to_usize().unwrap()),
                        display_name: vec![],
                    }
                } else {
                    param_ty = ParamType {
                        ty: registry.builtin_slice_type(param_ty.ty),
                        display_name: vec![],
                    }
                }
            }

            param_ty
        }
        ast::Type::StorageRef(ty) => ty_to_abi(ty, ns, registry),
        ast::Type::Ref(ty) => ty_to_abi(ty, ns, registry),
        ast::Type::Bool
        | ast::Type::Uint(_)
        | ast::Type::Int(_)
        | ast::Type::Address(_)
        | ast::Type::Contract(_) => {
            let scalety = primitive_to_string(ty.clone());

            ParamType {
                ty: registry.builtin_type(&scalety),
                display_name: vec![registry.string(&scalety)],
            }
        }
        ast::Type::Struct(n) => {
            let def = &ns.structs[*n];
            let fields = def
                .fields
                .iter()
                .map(|f| StructField {
                    name: registry.string(&f.name),
                    ty: ty_to_abi(&f.ty, ns, registry).ty,
                })
                .collect();
            ParamType {
                ty: registry.struct_type(&def.name, fields),
                display_name: vec![],
            }
        }
        ast::Type::DynamicBytes => {
            let elem = registry.builtin_type("u8");

            ParamType {
                ty: registry.builtin_slice_type(elem),
                display_name: vec![registry.string("Vec")],
            }
        }
        ast::Type::String => ParamType {
            ty: registry.string_type(),
            display_name: vec![registry.string("str")],
        },
        _ => unreachable!(),
    }
}

// For a given primitive, give the name as Substrate would like it (i.e. 64 bits
// signed int is i64, not int64).
fn primitive_to_string(ty: ast::Type) -> String {
    match ty {
        ast::Type::Bool => "bool".into(),
        ast::Type::Uint(n) => format!("u{}", n),
        ast::Type::Int(n) => format!("i{}", n),
        ast::Type::Contract(_) | ast::Type::Address(_) => "address".into(),
        _ => unreachable!(),
    }
}

fn parameter_to_abi(param: &ast::Parameter, ns: &ast::Namespace, registry: &mut Registry) -> Param {
    Param {
        name: registry.string(&param.name),
        ty: ty_to_abi(&param.ty, ns, registry),
    }
}

/// Given an u32 selector, generate a byte string like: "[\"0xF8\",\"0x1E\",\"0x7E\",\"0x1A\"]"
fn render_selector(f: &ast::Function) -> String {
    format!(
        "[{}]",
        f.selector()
            .to_le_bytes()
            .iter()
            .map(|b| format!("\"0x{:02X}\"", *b))
            .collect::<Vec<String>>()
            .join(",")
    )
}

/// Given a selector like "[\"0xF8\",\"0x1E\",\"0x7E\",\"0x1A\"]", parse the bytes. This function
/// does not validate the input.
fn parse_selector(selector: &str) -> Vec<u8> {
    selector[1..selector.len() - 2]
        .split(',')
        .map(|b_str| u8::from_str_radix(&b_str[3..5], 16).unwrap())
        .collect()
}
