use clap::{App, Arg, ArgMatches};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use solang::abi;
use solang::codegen::codegen;
use solang::file_cache::FileCache;
use solang::sema::diagnostics;

mod doc;
mod languageserver;

#[derive(Serialize)]
pub struct EwasmContract {
    pub wasm: String,
}

#[derive(Serialize)]
pub struct JsonContract {
    abi: Vec<abi::ethereum::ABI>,
    ewasm: EwasmContract,
}

#[derive(Serialize)]
pub struct JsonResult {
    pub errors: Vec<diagnostics::OutputJson>,
    pub contracts: HashMap<String, HashMap<String, JsonContract>>,
}

fn main() {
    let matches = App::new("solang")
        .version(&*format!("version {}", env!("GIT_HASH")))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::with_name("INPUT")
                .help("Solidity input files")
                .required(true)
                .conflicts_with("LANGUAGESERVER")
                .multiple(true),
        )
        .arg(
            Arg::with_name("EMIT")
                .help("Emit compiler state at early stage")
                .long("emit")
                .takes_value(true)
                .possible_values(&["ast", "cfg", "llvm", "bc", "object"]),
        )
        .arg(
            Arg::with_name("OPT")
                .help("Set optimizer level")
                .short("O")
                .takes_value(true)
                .possible_values(&["none", "less", "default", "aggressive"])
                .default_value("default"),
        )
        .arg(
            Arg::with_name("TARGET")
                .help("Target to build for")
                .long("target")
                .takes_value(true)
                .possible_values(&["substrate", "ewasm", "sabre", "generic", "solana"])
                .default_value("substrate"),
        )
        .arg(
            Arg::with_name("STD-JSON")
                .help("mimic solidity json output on stdout")
                .long("standard-json"),
        )
        .arg(
            Arg::with_name("VERBOSE")
                .help("show debug messages")
                .short("v")
                .long("verbose"),
        )
        .arg(
            Arg::with_name("OUTPUT")
                .help("output directory")
                .short("o")
                .long("output")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("IMPORTPATH")
                .help("Directory to search for solidity files")
                .short("I")
                .long("importpath")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("LANGUAGESERVER")
                .help("Start language server")
                .conflicts_with_all(&["STD-JSON", "OUTPUT", "EMIT", "OPT", "INPUT"])
                .long("language-server"),
        )
        .arg(
            Arg::with_name("DOC")
                .help("Generate documention for contracts using doc comments")
                .long("doc"),
        )
        .get_matches();

    if matches.is_present("LANGUAGESERVER") {
        languageserver::start_server();
    }

    let mut json = JsonResult {
        errors: Vec::new(),
        contracts: HashMap::new(),
    };

    let target = match matches.value_of("TARGET") {
        Some("substrate") => solang::Target::Substrate,
        Some("ewasm") => solang::Target::Ewasm,
        Some("sabre") => solang::Target::Sabre,
        Some("generic") => solang::Target::Generic,
        Some("solana") => solang::Target::Solana,
        _ => unreachable!(),
    };

    if matches.is_present("VERBOSE") {
        eprintln!("info: Solang version {}", env!("GIT_HASH"));
    }

    let mut cache = FileCache::new();

    for filename in matches.values_of("INPUT").unwrap() {
        if let Ok(path) = PathBuf::from(filename).canonicalize() {
            cache.add_import_path(path.parent().unwrap().to_path_buf());
        }
    }

    match PathBuf::from(".").canonicalize() {
        Ok(p) => cache.add_import_path(p),
        Err(e) => {
            eprintln!(
                "error: cannot add current directory to import path: {}",
                e.to_string()
            );
            std::process::exit(1);
        }
    }

    if let Some(paths) = matches.values_of("IMPORTPATH") {
        for p in paths {
            let path = PathBuf::from(p);
            match path.canonicalize() {
                Ok(p) => cache.add_import_path(p),
                Err(e) => {
                    eprintln!("error: import path ‘{}’: {}", p, e.to_string());
                    std::process::exit(1);
                }
            }
        }
    }

    if matches.is_present("DOC") {
        let verbose = matches.is_present("VERBOSE");
        let mut success = true;
        let mut files = Vec::new();

        for filename in matches.values_of("INPUT").unwrap() {
            let ns = solang::parse_and_resolve(filename, &mut cache, target);

            diagnostics::print_messages(&mut cache, &ns, verbose);

            if ns.contracts.is_empty() {
                eprintln!("{}: error: no contracts found", filename);
                success = false;
            } else if diagnostics::any_errors(&ns.diagnostics) {
                success = false;
            } else {
                files.push(ns);
            }
        }

        if success {
            // generate docs
            doc::generate_docs(matches.value_of("OUTPUT").unwrap_or("."), &files, verbose);
        }
    } else {
        for filename in matches.values_of("INPUT").unwrap() {
            process_filename(filename, &mut cache, target, &matches, &mut json);
        }

        if matches.is_present("STD-JSON") {
            println!("{}", serde_json::to_string(&json).unwrap());
        }
    }
}

fn process_filename(
    filename: &str,
    cache: &mut FileCache,
    target: solang::Target,
    matches: &ArgMatches,
    json: &mut JsonResult,
) {
    let output_file = |stem: &str, ext: &str| -> PathBuf {
        Path::new(matches.value_of("OUTPUT").unwrap_or(".")).join(format!("{}.{}", stem, ext))
    };
    let verbose = matches.is_present("VERBOSE");
    let opt = match matches.value_of("OPT").unwrap() {
        "none" => inkwell::OptimizationLevel::None,
        "less" => inkwell::OptimizationLevel::Less,
        "default" => inkwell::OptimizationLevel::Default,
        "aggressive" => inkwell::OptimizationLevel::Aggressive,
        _ => unreachable!(),
    };
    let context = inkwell::context::Context::create();

    let mut json_contracts = HashMap::new();

    // resolve phase
    let mut ns = solang::parse_and_resolve(filename, cache, target);

    if matches.is_present("STD-JSON") {
        let mut out = diagnostics::message_as_json(cache, &ns);
        json.errors.append(&mut out);
    } else {
        diagnostics::print_messages(cache, &ns, verbose);
    }

    if ns.contracts.is_empty() || diagnostics::any_errors(&ns.diagnostics) {
        eprintln!("{}: error: no valid contracts found", filename);
        std::process::exit(1);
    }

    // codegen all the contracts
    for contract_no in 0..ns.contracts.len() {
        codegen(contract_no, &mut ns);
    }

    if let Some("ast") = matches.value_of("EMIT") {
        println!("{}", ns.print(filename));
        return;
    }

    // emit phase
    for contract_no in 0..ns.contracts.len() {
        let resolved_contract = &ns.contracts[contract_no];

        if !resolved_contract.is_concrete() {
            continue;
        }

        if let Some("cfg") = matches.value_of("EMIT") {
            println!("{}", resolved_contract.print_to_string(&ns));
            continue;
        }

        if verbose {
            eprintln!(
                "info: Generating LLVM IR for contract {} with target {}",
                resolved_contract.name, ns.target
            );
        }

        let contract = resolved_contract.emit(&ns, &context, &filename, opt);

        if let Some("llvm") = matches.value_of("EMIT") {
            if let Some(runtime) = &contract.runtime {
                // In Ethereum, an ewasm contract has two parts, deployer and runtime. The deployer code returns the runtime wasm
                // as a byte string
                let llvm_filename = output_file(&format!("{}_deploy", contract.name), "ll");

                if verbose {
                    eprintln!(
                        "info: Saving deployer LLVM {} for contract {}",
                        llvm_filename.display(),
                        contract.name
                    );
                }

                contract.dump_llvm(&llvm_filename).unwrap();

                let llvm_filename = output_file(&format!("{}_runtime", contract.name), "ll");

                if verbose {
                    eprintln!(
                        "info: Saving runtime LLVM {} for contract {}",
                        llvm_filename.display(),
                        contract.name
                    );
                }

                runtime.dump_llvm(&llvm_filename).unwrap();
            } else {
                let llvm_filename = output_file(&contract.name, "ll");

                if verbose {
                    eprintln!(
                        "info: Saving LLVM {} for contract {}",
                        llvm_filename.display(),
                        contract.name
                    );
                }

                contract.dump_llvm(&llvm_filename).unwrap();
            }
            continue;
        }

        if let Some("bc") = matches.value_of("EMIT") {
            // In Ethereum, an ewasm contract has two parts, deployer and runtime. The deployer code returns the runtime wasm
            // as a byte string
            if let Some(runtime) = &contract.runtime {
                let bc_filename = output_file(&format!("{}_deploy", contract.name), "bc");

                if verbose {
                    eprintln!(
                        "info: Saving deploy LLVM BC {} for contract {}",
                        bc_filename.display(),
                        contract.name
                    );
                }

                contract.bitcode(&bc_filename);

                let bc_filename = output_file(&format!("{}_runtime", contract.name), "bc");

                if verbose {
                    eprintln!(
                        "info: Saving runtime LLVM BC {} for contract {}",
                        bc_filename.display(),
                        contract.name
                    );
                }

                runtime.bitcode(&bc_filename);
            } else {
                let bc_filename = output_file(&contract.name, "bc");

                if verbose {
                    eprintln!(
                        "info: Saving LLVM BC {} for contract {}",
                        bc_filename.display(),
                        contract.name
                    );
                }

                contract.bitcode(&bc_filename);
            }
            continue;
        }

        if let Some("object") = matches.value_of("EMIT") {
            let obj = match contract.code(false) {
                Ok(o) => o,
                Err(s) => {
                    println!("error: {}", s);
                    std::process::exit(1);
                }
            };

            let obj_filename = output_file(&contract.name, "o");

            if verbose {
                eprintln!(
                    "info: Saving Object {} for contract {}",
                    obj_filename.display(),
                    contract.name
                );
            }

            let mut file = File::create(obj_filename).unwrap();
            file.write_all(&obj).unwrap();
            continue;
        }

        let code = match contract.code(true) {
            Ok(o) => o,
            Err(s) => {
                println!("error: {}", s);
                std::process::exit(1);
            }
        };

        if matches.is_present("STD-JSON") {
            json_contracts.insert(
                contract.name.to_owned(),
                JsonContract {
                    abi: abi::ethereum::gen_abi(contract_no, &ns),
                    ewasm: EwasmContract {
                        wasm: hex::encode_upper(code),
                    },
                },
            );
        } else {
            let bin_filename = output_file(&contract.name, target.file_extension());

            if verbose {
                eprintln!(
                    "info: Saving binary {} for contract {}",
                    bin_filename.display(),
                    contract.name
                );
            }

            let mut file = File::create(bin_filename).unwrap();
            file.write_all(&code).unwrap();

            let (abi_bytes, abi_ext) = abi::generate_abi(contract_no, &ns, &code, verbose);
            let abi_filename = output_file(&contract.name, abi_ext);

            if verbose {
                eprintln!(
                    "info: Saving ABI {} for contract {}",
                    abi_filename.display(),
                    contract.name
                );
            }

            file = File::create(abi_filename).unwrap();
            file.write_all(&abi_bytes.as_bytes()).unwrap();
        }
    }

    json.contracts.insert(filename.to_owned(), json_contracts);
}
